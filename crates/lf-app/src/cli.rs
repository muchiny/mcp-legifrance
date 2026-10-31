//! Ligne de commande (clap) : sous-commandes et options (02 §3.7, §8.1).
//!
//! ```text
//! mcp-legifrance [serve]                       # défaut : serveur MCP stdio
//! mcp-legifrance check-config [--online] [--format text|json]
//! mcp-legifrance serve-http [--listen 127.0.0.1:<port>] [--allow-origin <origine>]...   # feature `http`
//! mcp-legifrance build-dila-index --archives <dossier> [--index <dossier>]               # feature `dila`
//! mcp-legifrance --version | --help
//! ```
//!
//! Aucune option ne porte un secret : un argument de ligne de commande se lit dans `ps`. La
//! configuration vient de l'environnement (`config.rs`), et le jeton de `serve-http` de
//! `LF_MCP_HTTP_TOKEN` (`http_config.rs`). `serve-http` (M8, profil local, Q4) n'existe que dans
//! un binaire compilé avec la feature `http`.

#[cfg(feature = "http")]
use std::net::SocketAddr;
#[cfg(feature = "dila")]
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Adresse d'écoute par défaut de `serve-http` : la boucle locale (annexe D §5.2, §6.2).
#[cfg(feature = "http")]
const DEFAULT_LISTEN: &str = "127.0.0.1:8765";

/// Arguments du binaire `mcp-legifrance`.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
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
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub(crate) enum Command {
    /// Lance le serveur MCP sur stdin et stdout (défaut quand aucune sous-commande n'est donnée).
    Serve,
    /// Charge et valide la configuration, puis l'affiche sans révéler aucun secret.
    CheckConfig(CheckConfigArgs),
    /// Lance le serveur MCP en Streamable HTTP sur la boucle locale (profil local : jeton statique
    /// dans `LF_MCP_HTTP_TOKEN`, refus de démarrer sans lui).
    #[cfg(feature = "http")]
    ServeHttp(ServeHttpArgs),
    /// Bâtit ou met à jour l'index local DILA depuis des archives LEGI déjà téléchargées (aucun
    /// accès réseau).
    #[cfg(feature = "dila")]
    BuildDilaIndex(BuildDilaIndexArgs),
}

/// Options de `build-dila-index`.
#[cfg(feature = "dila")]
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct BuildDilaIndexArgs {
    /// Dossier des archives `tar.gz` du fonds LEGI (un global, puis ses incréments).
    #[arg(long, value_name = "DIR")]
    pub(crate) archives: PathBuf,
    /// Dossier de l'index à bâtir ; par défaut, celui de `LF_DILA_DIR`.
    #[arg(long, value_name = "DIR")]
    pub(crate) index: Option<PathBuf>,
}

/// Options de `serve-http`. Aucune ne porte le jeton, qui se lirait dans `ps` : il vient de
/// l'environnement.
#[cfg(feature = "http")]
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct ServeHttpArgs {
    /// Adresse d'écoute, de la boucle locale seulement (`127.0.0.0/8` ou `[::1]`).
    #[arg(long, value_name = "ADDR", default_value = DEFAULT_LISTEN)]
    pub(crate) listen: SocketAddr,
    /// Origine admise dans l'en-tête `Origin` (schéma obligatoire, `:*` pour tout port, par
    /// exemple `http://localhost:*`) ; répétable. Sans option, tout `Origin` présent est refusé
    /// (les clients qui ne sont pas des navigateurs n'en envoient pas).
    #[arg(long = "allow-origin", value_name = "ORIGIN")]
    pub(crate) allow_origin: Vec<String>,
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

    #[cfg(feature = "http")]
    #[test]
    fn serve_http_listens_on_loopback_by_default_and_takes_origins() {
        let default =
            ServeHttpArgs { listen: DEFAULT_LISTEN.parse().unwrap(), allow_origin: vec![] };
        assert!(default.listen.ip().is_loopback());
        assert_eq!(parse(&["serve-http"]).unwrap().resolve(), Command::ServeHttp(default));

        let cli = parse(&[
            "serve-http",
            "--listen",
            "127.0.0.1:9000",
            "--allow-origin",
            "http://localhost:*",
            "--allow-origin=https://app.test:443",
        ])
        .unwrap();
        assert_eq!(
            cli.resolve(),
            Command::ServeHttp(ServeHttpArgs {
                listen: "127.0.0.1:9000".parse().unwrap(),
                allow_origin: vec![
                    "http://localhost:*".to_owned(),
                    "https://app.test:443".to_owned()
                ],
            })
        );
        // Une adresse illisible est une erreur d'usage (code 2).
        let err = parse(&["serve-http", "--listen", "not-an-address"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.kind(), ErrorKind::ValueValidation);
    }

    /// Le jeton ne passe jamais par la ligne de commande : `serve-http` n'a aucune option pour lui.
    #[cfg(feature = "http")]
    #[test]
    fn serve_http_has_no_token_option() {
        for flag in ["--token", "--http-token", "--password", "--secret"] {
            let err = parse(&["serve-http", flag, "valeur"]).unwrap_err();
            assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{flag}");
        }
    }

    #[cfg(feature = "http")]
    #[test]
    fn help_lists_serve_http() {
        let help = parse(&["--help"]).unwrap_err().to_string();
        assert!(help.contains("serve-http"), "{help}");
    }

    #[cfg(feature = "dila")]
    #[test]
    fn build_dila_index_takes_archives_and_an_optional_index() {
        let cli = parse(&["build-dila-index", "--archives", "dumps"]).unwrap();
        assert_eq!(
            cli.resolve(),
            Command::BuildDilaIndex(BuildDilaIndexArgs { archives: "dumps".into(), index: None })
        );
        let cli = parse(&["build-dila-index", "--archives=dumps", "--index", "idx"]).unwrap();
        assert_eq!(
            cli.resolve(),
            Command::BuildDilaIndex(BuildDilaIndexArgs {
                archives: "dumps".into(),
                index: Some("idx".into()),
            })
        );
    }

    #[cfg(feature = "dila")]
    #[test]
    fn build_dila_index_requires_the_archive_directory() {
        let err = parse(&["build-dila-index"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[cfg(feature = "dila")]
    #[test]
    fn help_lists_build_dila_index() {
        let help = parse(&["--help"]).unwrap_err().to_string();
        assert!(help.contains("build-dila-index"), "{help}");
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
