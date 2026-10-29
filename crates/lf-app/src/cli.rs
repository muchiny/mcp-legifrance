//! Ligne de commande (clap) : sous-commandes et options (02 §3.7, §8.1).
//!
//! ```text
//! mcp-legifrance [serve]                       # défaut : serveur MCP stdio
//! mcp-legifrance check-config [--online] [--format text|json]
//! mcp-legifrance --version | --help
//! ```
//!
//! Aucune option ne porte un secret : un argument de ligne de commande se lit dans `ps`. La
//! configuration vient de l'environnement (`config.rs`). `serve-http` (M8) n'existe pas encore.

#![allow(
    dead_code,
    reason = "consommé par la racine de composition (V2-04, `wiring::run`) ; retirer à ce moment"
)]

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Arguments du binaire `mcp-legifrance`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser)]
#[command(name = "mcp-legifrance", version, about)]
pub(crate) struct Cli {
    /// Sous-commande ; `serve` quand elle est absente.
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

impl Cli {
    /// La sous-commande demandée, `serve` par défaut (02 §8.1 : « pas de sous-commande : `serve` »).
    pub(crate) fn resolve(self) -> Command {
        self.command.unwrap_or(Command::Serve)
    }
}

/// Sous-commandes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub(crate) enum Command {
    /// Lance le serveur MCP sur stdin et stdout (défaut quand aucune sous-commande n'est donnée).
    Serve,
    /// Charge et valide la configuration, puis l'affiche sans révéler aucun secret.
    CheckConfig(CheckConfigArgs),
}

/// Options de `check-config`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Args)]
pub(crate) struct CheckConfigArgs {
    /// Tente aussi d'obtenir un jeton auprès de PISTE, sans appeler l'API.
    #[arg(long)]
    pub(crate) online: bool,
    /// Format du rapport sur stdout.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) format: ReportFormat,
}

/// Format du rapport de `check-config`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum ReportFormat {
    /// Une ligne par variable, lisible par une personne.
    #[default]
    Text,
    /// Document JSON, pour un outil.
    Json,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, error::ErrorKind};

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("mcp-legifrance").chain(args.iter().copied()))
    }

    #[test]
    fn cli_defaults_to_serve() {
        let cli = parse(&[]).unwrap();
        assert_eq!(cli.command, None);
        assert_eq!(cli.resolve(), Command::Serve);
    }

    #[test]
    fn serve_is_an_explicit_subcommand() {
        let cli = parse(&["serve"]).unwrap();
        assert_eq!(cli.command, Some(Command::Serve));
        assert_eq!(cli.resolve(), Command::Serve);
    }

    #[test]
    fn check_config_is_offline_text_by_default() {
        let cli = parse(&["check-config"]).unwrap();
        assert_eq!(
            cli.resolve(),
            Command::CheckConfig(CheckConfigArgs { online: false, format: ReportFormat::Text })
        );
    }

    #[test]
    fn check_config_takes_online_and_format() {
        let cli = parse(&["check-config", "--online", "--format", "json"]).unwrap();
        assert_eq!(
            cli.resolve(),
            Command::CheckConfig(CheckConfigArgs { online: true, format: ReportFormat::Json })
        );
        let cli = parse(&["check-config", "--format=text"]).unwrap();
        assert_eq!(
            cli.resolve(),
            Command::CheckConfig(CheckConfigArgs { online: false, format: ReportFormat::Text })
        );
    }

    /// Une erreur d'usage sort avec le code 2, comme le code 2 de `check-config` (02 §8.1).
    #[test]
    fn usage_errors_exit_with_code_2() {
        for args in [
            &["check-config", "--format", "xml"][..],
            &["serve", "--online"],
            &["frobnicate"],
            &["--no-such-flag"],
        ] {
            let err = parse(args).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{args:?}");
            assert!(err.use_stderr(), "{args:?}");
        }
        let err = parse(&["check-config", "--format", "xml"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }

    /// Aucune option pour les secrets : ils se lisent dans `ps` (02 §8.1).
    #[test]
    fn secrets_are_never_command_line_options() {
        for flag in ["--client-id", "--client-secret", "--secret", "--token", "--password"] {
            let err = parse(&["check-config", flag, "valeur"]).unwrap_err();
            assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{flag}");
            let err = parse(&[flag, "valeur"]).unwrap_err();
            assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{flag}");
        }
    }

    #[test]
    fn version_and_help_exit_with_code_0_on_stdout() {
        let err = parse(&["--version"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
        assert_eq!(err.exit_code(), 0);
        assert!(err.to_string().contains(env!("CARGO_PKG_VERSION")));

        let err = parse(&["--help"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        assert_eq!(err.exit_code(), 0);
        let help = err.to_string();
        assert!(help.contains("check-config"), "{help}");
        assert!(help.contains("serve"), "{help}");
    }

    /// Cohérence de la définition (noms en double, valeurs par défaut invalides…).
    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }
}
