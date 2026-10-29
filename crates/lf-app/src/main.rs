//! Serveur MCP Légifrance : binaire `mcp-legifrance` (stdio) et interface en ligne de commande.
//!
//! Racine de composition (02 §3.7, §8.4) : seul ce crate lit l'environnement, l'horloge et les
//! signaux, puis assemble les adapters. `main` enchaîne, dans cet ordre :
//!
//! 1. la ligne de commande (`cli`) : une erreur d'usage sort avec le code 2, `--help` et
//!    `--version` avec le code 0 ;
//! 2. la configuration, lue une fois (`config`), sans E/S ;
//! 3. le journal sur stderr (`logging`), installé avant tout avertissement ;
//! 4. le runtime tokio multi-thread, construit à la main pour rendre un `ExitCode` et borner
//!    l'arrêt (02 §8.7) ;
//! 5. la sous-commande : `serve` (`wiring::run`, par défaut) ou `check-config` (`check`).
//!
//! Codes de sortie : 0 (EOF de stdin, arrêt demandé, configuration valide), 1 (échec du service,
//! du runtime ou de l'écriture du rapport), 2 (erreur d'usage, configuration invalide pour
//! `check-config`), 3 et 4 (identifiants refusés ou réseau en échec pour `check-config --online`).
//! stdout ne porte que le JSON-RPC de `serve` et le rapport de `check-config`.

mod check;
mod cli;
mod clock;
mod config;
mod logging;
mod shutdown;
mod wiring;

use std::{io, process::ExitCode, time::Duration};

use anyhow::Context as _;
use clap::Parser as _;

use cli::{CheckConfigArgs, Cli, Command, ReportFormat};
use config::{Loaded, ProcessEnv};

/// Code de sortie d'un échec du service, du runtime ou de l'écriture du rapport (02 §8.2).
const EXIT_FAILURE: u8 = 1;

/// Délai laissé aux tâches restantes quand le runtime s'arrête (02 §8.7, §8.8).
const RUNTIME_SHUTDOWN: Duration = Duration::from_millis(250);

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return usage_exit(&error),
    };
    // Seule lecture de l'environnement du processus ; `load` ne fait ni E/S ni journal.
    let loaded = config::load(&ProcessEnv::capture());
    // Le journal d'abord : les avertissements de la configuration passent par stderr (02 §3.7).
    logging::init_logging(loaded.config.log_filter.as_deref().unwrap_or_default());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("lf-worker")
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(%error, "cannot start the async runtime");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let result = runtime.block_on(dispatch(cli.resolve(), loaded));
    runtime.shutdown_timeout(RUNTIME_SHUTDOWN);
    exit_code(result)
}

/// Sort d'une erreur de clap : l'aide et la version vont sur stdout (code 0), une erreur d'usage
/// sur stderr (code 2, 02 §8.1).
fn usage_exit(error: &clap::Error) -> ExitCode {
    // Si l'écriture échoue (stdout ou stderr fermé), il n'y a nulle part où le dire.
    drop(error.print());
    ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2))
}

/// Exécute la sous-commande.
async fn dispatch(command: Command, loaded: Loaded) -> anyhow::Result<ExitCode> {
    match command {
        Command::Serve => {
            loaded.emit();
            // Les écouteurs de signaux ne servent qu'à `serve` : `check-config` reste
            // interruptible par défaut.
            wiring::run(loaded.config, shutdown::shutdown_token()).await
        }
        // Le verrou de stdout n'est pris que pour le rapport : `serve` écrit sur stdout depuis
        // une autre tâche (`tokio::io::stdout`), qu'un verrou tenu par `main` bloquerait.
        Command::CheckConfig(args) => report(&loaded, args, &mut io::stdout().lock()).await,
    }
}

/// `check-config` : le rapport sur `out`, et le code de sortie de `check::exit_code`.
async fn report(
    loaded: &Loaded,
    args: CheckConfigArgs,
    out: &mut impl io::Write,
) -> anyhow::Result<ExitCode> {
    let format = match args.format {
        ReportFormat::Text => check::Format::Text,
        ReportFormat::Json => check::Format::Json,
    };
    check::run(loaded, check::Options { format, online: args.online }, out)
        .await
        .context("cannot write the check-config report")
}

/// Code de sortie d'une commande : celui qu'elle rend, ou 1 après avoir journalisé son échec.
fn exit_code(result: anyhow::Result<ExitCode>) -> ExitCode {
    result.unwrap_or_else(|error| {
        tracing::error!(error = %format!("{error:#}"), "command failed");
        ExitCode::from(EXIT_FAILURE)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load, test_env::MapEnv};
    use clap::error::ErrorKind;

    /// `ExitCode` n'est pas comparable : on compare sa représentation.
    fn code(value: u8) -> String {
        format!("{:?}", ExitCode::from(value))
    }

    fn args(online: bool, format: ReportFormat) -> CheckConfigArgs {
        CheckConfigArgs { online, format }
    }

    #[test]
    fn usage_errors_exit_with_code_2_and_help_with_code_0() {
        let usage = Cli::try_parse_from(["mcp-legifrance", "frobnicate"]).unwrap_err();
        assert_eq!(format!("{:?}", usage_exit(&usage)), code(2));
        let help = Cli::try_parse_from(["mcp-legifrance", "--help"]).unwrap_err();
        assert_eq!(help.kind(), ErrorKind::DisplayHelp);
        assert_eq!(format!("{:?}", usage_exit(&help)), code(0));
    }

    #[test]
    fn a_failed_command_exits_with_code_1_and_a_finished_one_with_its_own_code() {
        assert_eq!(format!("{:?}", exit_code(Ok(ExitCode::from(4)))), code(4));
        assert_eq!(format!("{:?}", exit_code(Err(anyhow::anyhow!("boom")))), code(1));
    }

    #[tokio::test]
    async fn check_config_writes_the_report_and_the_exit_code_of_the_configuration() {
        let valid = load(&MapEnv::new(&[
            ("LF_PISTE_CLIENT_ID", "11112222-3333-4444-5555-666677778888"),
            ("LF_PISTE_CLIENT_SECRET", "99990000-aaaa-bbbb-cccc-ddddeeeeffff"),
        ]));
        let mut text = Vec::new();
        let ok = report(&valid, args(false, ReportFormat::Text), &mut text).await.unwrap();
        assert_eq!(format!("{ok:?}"), code(0));
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("Result: valid"), "{text}");
        assert!(!text.contains("99990000"), "{text}");

        // Sans identifiants : configuration invalide, code 2, rapport en JSON.
        let mut json = Vec::new();
        let invalid = report(&load(&MapEnv::new(&[])), args(false, ReportFormat::Json), &mut json)
            .await
            .unwrap();
        assert_eq!(format!("{invalid:?}"), code(2));
        let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(json["valid"], false);
    }

    /// Une sortie qui refuse l'écriture est un échec du processus (code 1 par `exit_code`).
    #[tokio::test]
    async fn a_closed_stdout_fails_the_report() {
        struct Closed;
        impl io::Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let loaded = load(&MapEnv::new(&[]));
        let error = report(&loaded, args(false, ReportFormat::Text), &mut Closed).await;
        let error = error.unwrap_err();
        assert_eq!(error.to_string(), "cannot write the check-config report");
        assert_eq!(format!("{:?}", exit_code(Err(error))), code(1));
    }
}
