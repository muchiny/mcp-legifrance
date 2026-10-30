//! Réglages de `serve-http` (M8, profil local seul, Q4) : l'adresse et les origines de la ligne de
//! commande, le jeton statique de `LF_MCP_HTTP_TOKEN` (02 §8.2 ; annexe D §6.2).
//!
//! - Le jeton ne se donne jamais en option : il se lit dans l'environnement, qui n'est pas dans
//!   `ps`. Une valeur vide ou blanche compte comme absente, comme pour les autres variables.
//! - Sans jeton, `serve-http` refuse de démarrer avec un message explicite (code de sortie 2,
//!   comme une configuration invalide) : l'écoute sur la boucle locale n'est pas une frontière
//!   d'authentification.
//! - L'adresse doit être de la boucle locale ; sinon le démarrage est refusé de la même façon.
//! - Un jeton de moins de 32 octets démarre, avec un avertissement : l'annexe D §6.2 en demande au
//!   moins 32 aléatoires.

use std::{ffi::OsString, fmt};

use lf_adapter_mcp::{HttpOptions, HttpOptionsError, HttpToken, HttpTokenError};

use crate::cli::ServeHttpArgs;

/// Variable du jeton statique du profil local (00 §1, nom figé en M8).
pub(crate) const TOKEN_VARIABLE: &str = "LF_MCP_HTTP_TOKEN";

/// Longueur à partir de laquelle un jeton n'est plus jugé faible (annexe D §6.2).
const RECOMMENDED_TOKEN_BYTES: usize = 32;

/// Réglages prêts à servir.
#[derive(Debug)]
pub(crate) struct HttpSettings {
    /// Options de `serve_http` : adresse, jeton, origines.
    pub(crate) options: HttpOptions,
    /// Le jeton a moins de 32 octets : avertir au démarrage.
    pub(crate) weak_token: bool,
}

/// Raison pour laquelle `serve-http` ne démarre pas.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HttpConfigError {
    /// `LF_MCP_HTTP_TOKEN` absente ou blanche.
    MissingToken,
    /// Valeur de `LF_MCP_HTTP_TOKEN` inutilisable. Le message ne cite jamais la valeur.
    InvalidToken(HttpTokenError),
    /// Adresse d'écoute hors de la boucle locale.
    Listen(HttpOptionsError),
}

impl fmt::Display for HttpConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingToken => write!(
                f,
                "serve-http needs a static token: set {TOKEN_VARIABLE} to at least \
                 {RECOMMENDED_TOKEN_BYTES} random bytes (for example `openssl rand -hex 32`); \
                 the token is never accepted on the command line"
            ),
            Self::InvalidToken(error) => write!(f, "{TOKEN_VARIABLE} is unusable: {error}"),
            Self::Listen(error) => write!(f, "--listen is refused: {error}"),
        }
    }
}

impl std::error::Error for HttpConfigError {}

/// Lit le jeton dans l'environnement du processus, sans blancs de bord ; `None` s'il est absent
/// ou blanc.
///
/// `ProcessEnv` ne garde que la liste fermée de 00 §1, où `LF_MCP_HTTP_TOKEN` (M8 seulement) ne
/// figure pas : cette variable ne sert qu'à `serve-http`, et sa valeur ne quitte pas ce module
/// avant d'être enfermée dans un `HttpToken`, dont le `Debug` ne la montre pas.
#[expect(
    clippy::disallowed_methods,
    reason = "variable de la seule commande serve-http, absente de la liste fermée de ProcessEnv"
)]
pub(crate) fn token_from_environment() -> Option<String> {
    clean(std::env::var_os(TOKEN_VARIABLE))
}

/// Valeur de la variable sans blancs de bord ; `None` si elle est absente ou blanche.
fn clean(value: Option<OsString>) -> Option<String> {
    let value = value?.to_string_lossy().trim().to_owned();
    (!value.is_empty()).then_some(value)
}

/// Réglages de `serve-http` d'après la ligne de commande et le jeton lu dans l'environnement.
///
/// # Errors
///
/// [`HttpConfigError`] si le jeton manque ou ne convient pas, ou si l'adresse n'est pas de la
/// boucle locale.
pub(crate) fn resolve(
    args: &ServeHttpArgs,
    token: Option<&str>,
) -> Result<HttpSettings, HttpConfigError> {
    let token = token.map(str::trim).filter(|value| !value.is_empty());
    let token = HttpToken::new(token.ok_or(HttpConfigError::MissingToken)?)
        .map_err(HttpConfigError::InvalidToken)?;
    let weak_token = token.len() < RECOMMENDED_TOKEN_BYTES;
    let options = HttpOptions::local(args.listen)
        .map_err(HttpConfigError::Listen)?
        .with_token(token)
        .with_allowed_origins(args.allow_origin.iter().cloned());
    Ok(HttpSettings { options, weak_token })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jeton de 64 caractères, comme `openssl rand -hex 32`.
    const STRONG: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn args(listen: &str, origins: &[&str]) -> ServeHttpArgs {
        ServeHttpArgs {
            listen: listen.parse().unwrap(),
            allow_origin: origins.iter().map(|origin| (*origin).to_owned()).collect(),
        }
    }

    #[test]
    fn the_token_comes_from_the_environment_never_from_the_command_line() {
        // Le nom de la variable est figé en M8 (00 §1).
        assert_eq!(TOKEN_VARIABLE, "LF_MCP_HTTP_TOKEN");
        // Absente, vide ou blanche : pas de jeton ; sinon la valeur sans blancs de bord.
        assert_eq!(clean(None), None);
        for blank in ["", "   ", "\t\n"] {
            assert_eq!(clean(Some(OsString::from(blank))), None, "{blank:?}");
        }
        assert_eq!(clean(Some(OsString::from("  abc\n"))), Some("abc".to_owned()));
        // La lecture réelle ne panique pas, quel que soit l'environnement du test.
        drop(token_from_environment());
    }

    #[test]
    fn serve_http_refuses_to_start_without_a_token() {
        for missing in [None, Some(""), Some("   "), Some("\t\n")] {
            let error = resolve(&args("127.0.0.1:8765", &[]), missing).unwrap_err();
            assert_eq!(error, HttpConfigError::MissingToken, "{missing:?}");
            let message = error.to_string();
            // Le message est explicite : variable, longueur, et l'interdit de la ligne de commande.
            assert!(message.contains("LF_MCP_HTTP_TOKEN"), "{message}");
            assert!(message.contains("32"), "{message}");
            assert!(message.contains("command line"), "{message}");
        }
    }

    #[test]
    fn a_valid_token_and_a_loopback_address_make_the_settings() {
        let settings =
            resolve(&args("127.0.0.1:9000", &["http://localhost:*"]), Some(STRONG)).unwrap();
        assert_eq!(settings.options.listen(), "127.0.0.1:9000".parse().unwrap());
        assert!(!settings.weak_token);
        // Les blancs de bord sont retirés, comme pour les autres variables.
        let settings = resolve(&args("[::1]:9000", &[]), Some(&format!("  {STRONG}\n"))).unwrap();
        assert!(!settings.weak_token);
        // Le jeton ne sort jamais par `Debug`.
        assert!(!format!("{settings:?}").contains(STRONG));
    }

    #[test]
    fn a_short_token_starts_with_a_warning() {
        let settings = resolve(&args("127.0.0.1:8765", &[]), Some("short-but-valid")).unwrap();
        assert!(settings.weak_token);
        // 32 octets suffisent ; 31 sont jugés faibles.
        let settings = resolve(&args("127.0.0.1:8765", &[]), Some(&STRONG[..32])).unwrap();
        assert!(!settings.weak_token);
        let settings = resolve(&args("127.0.0.1:8765", &[]), Some(&STRONG[..31])).unwrap();
        assert!(settings.weak_token);
    }

    #[test]
    fn an_unusable_token_is_named_without_being_shown() {
        let secret = "has a space inside";
        let error = resolve(&args("127.0.0.1:8765", &[]), Some(secret)).unwrap_err();
        assert_eq!(error, HttpConfigError::InvalidToken(HttpTokenError::NotPrintable));
        let message = error.to_string();
        assert!(message.contains("LF_MCP_HTTP_TOKEN"), "{message}");
        assert!(!message.contains("space inside"), "{message}");
    }

    #[test]
    fn a_non_loopback_address_is_refused() {
        for bad in ["0.0.0.0:8765", "192.168.1.10:8765", "[::]:8765"] {
            let error = resolve(&args(bad, &[]), Some(STRONG)).unwrap_err();
            let ip = bad.parse::<std::net::SocketAddr>().unwrap().ip();
            assert_eq!(error, HttpConfigError::Listen(HttpOptionsError::NotLoopback(ip)), "{bad}");
            let message = error.to_string();
            assert!(message.contains("--listen"), "{message}");
            assert!(message.contains("loopback"), "{message}");
        }
    }
}
