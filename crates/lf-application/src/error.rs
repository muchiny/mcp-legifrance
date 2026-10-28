//! Codes d'erreur d'outil exposés au modèle (02 §9, 05 §7).
//!
//! `ErrorCode` est la source unique des six codes ; `AppError` est l'erreur que les cas d'usage
//! rendent à l'adapter entrant, et `UpstreamCause` détaille sa variante `Upstream` pour les
//! logs et l'audit sans créer de code MCP (02 §9.1).

use std::time::Duration;

use thiserror::Error;

use crate::input::InputError;

/// Code d'erreur d'un appel d'outil : préfixe `[code]` du message vu par le modèle et champ
/// `error_code` de la ligne d'audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Argument refusé : forme, plage ou combinaison invalide.
    InvalidArg,
    /// Ressource introuvable.
    NotFound,
    /// Panne ou réponse inexploitable du corpus amont.
    UpstreamError,
    /// Quota atteint.
    RateLimited,
    /// Configuration absente ou invalide, identifiants refusés.
    ConfigError,
    /// Journal d'audit indisponible : l'appel n'est pas rendu sans trace (D-16).
    AuditError,
}

impl ErrorCode {
    /// Tous les codes, dans l'ordre de 02 §9 (garde de dérive).
    pub const ALL: [Self; 6] = [
        Self::InvalidArg,
        Self::NotFound,
        Self::UpstreamError,
        Self::RateLimited,
        Self::ConfigError,
        Self::AuditError,
    ];

    /// Nom du code sur le fil et dans le journal.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArg => "invalid_arg",
            Self::NotFound => "not_found",
            Self::UpstreamError => "upstream_error",
            Self::RateLimited => "rate_limited",
            Self::ConfigError => "config_error",
            Self::AuditError => "audit_error",
        }
    }
}

/// Détail de la panne amont derrière `AppError::Upstream` : pour les logs et l'audit, jamais un
/// code MCP de plus (02 §9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamCause {
    /// Réponse HTTP d'échec du corpus, avec son statut.
    Status(u16),
    /// Connexion impossible ou coupée.
    Network,
    /// Délai de l'appel HTTP dépassé.
    Timeout,
    /// Échéance globale de l'appel d'outil dépassée.
    Deadline,
    /// Réponse reçue mais inexploitable.
    Decode,
    /// Appel annulé par le client : aucune réponse ne sera lue, donc aucun code.
    Cancelled,
}

/// Erreur d'un cas d'usage. Chaque variante porte le message final, tel que le modèle le lira :
/// il est rédigé par le module `messages` (02 §3.2), jamais recopié d'un `detail` amont ici.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// Argument refusé.
    #[error("{message}")]
    InvalidArgument {
        /// Message final.
        message: String,
    },
    /// Ressource introuvable.
    #[error("{message}")]
    NotFound {
        /// Message final.
        message: String,
    },
    /// Panne du corpus amont.
    #[error("{message}")]
    Upstream {
        /// Message final.
        message: String,
        /// Cause détaillée, pour les logs et l'audit.
        cause: UpstreamCause,
    },
    /// Quota atteint.
    #[error("{message}")]
    RateLimited {
        /// Message final.
        message: String,
        /// Délai avant nouvel essai, quand le corpus l'indique.
        retry_after: Option<Duration>,
    },
    /// Configuration absente ou invalide, identifiants refusés.
    #[error("{message}")]
    Config {
        /// Message final.
        message: String,
    },
    /// Journal d'audit indisponible : l'appel n'est pas rendu sans trace (D-16).
    #[error("{message}")]
    Audit {
        /// Message final.
        message: String,
        /// Code que l'appel aurait rendu sans la panne d'audit, s'il y en avait un.
        original: Option<ErrorCode>,
    },
}

impl AppError {
    /// Erreur d'argument.
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument { message: message.into() }
    }

    /// Erreur « introuvable ».
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound { message: message.into() }
    }

    /// Erreur de configuration.
    #[must_use]
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config { message: message.into() }
    }

    /// Code d'erreur MCP ; `None` seulement pour un appel annulé, dont personne ne lira la
    /// réponse.
    #[must_use]
    pub const fn code(&self) -> Option<ErrorCode> {
        match self {
            Self::InvalidArgument { .. } => Some(ErrorCode::InvalidArg),
            Self::NotFound { .. } => Some(ErrorCode::NotFound),
            Self::Upstream { cause: UpstreamCause::Cancelled, .. } => None,
            Self::Upstream { .. } => Some(ErrorCode::UpstreamError),
            Self::RateLimited { .. } => Some(ErrorCode::RateLimited),
            Self::Config { .. } => Some(ErrorCode::ConfigError),
            Self::Audit { .. } => Some(ErrorCode::AuditError),
        }
    }

    /// Message final, sans préfixe de code.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::InvalidArgument { message }
            | Self::NotFound { message }
            | Self::Upstream { message, .. }
            | Self::RateLimited { message, .. }
            | Self::Config { message }
            | Self::Audit { message, .. } => message,
        }
    }

    /// Cause amont, pour la seule variante `Upstream`.
    #[must_use]
    pub const fn cause(&self) -> Option<UpstreamCause> {
        match self {
            Self::Upstream { cause, .. } => Some(*cause),
            _ => None,
        }
    }
}

/// Décodage impossible ⇒ `invalid_arg`, avec le message de décodage tel quel (02 §9.2, ligne 1).
impl From<InputError> for AppError {
    fn from(error: InputError) -> Self {
        Self::InvalidArgument { message: error.message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn codes_have_their_wire_names() {
        let names = ErrorCode::ALL.map(ErrorCode::as_str);
        assert_eq!(
            names,
            [
                "invalid_arg",
                "not_found",
                "upstream_error",
                "rate_limited",
                "config_error",
                "audit_error"
            ]
        );
    }

    #[test]
    fn all_lists_every_code_once() {
        // Un `match` exhaustif : une variante ajoutée sans mise à jour de `ALL` casse ce test.
        let rank = |code: ErrorCode| match code {
            ErrorCode::InvalidArg => 0,
            ErrorCode::NotFound => 1,
            ErrorCode::UpstreamError => 2,
            ErrorCode::RateLimited => 3,
            ErrorCode::ConfigError => 4,
            ErrorCode::AuditError => 5,
        };
        assert_eq!(ErrorCode::ALL.map(rank), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn app_error_code_per_variant() {
        let upstream = |cause| AppError::Upstream { message: "m".to_owned(), cause };
        let audit = |original| AppError::Audit { message: "m".to_owned(), original };
        let table = [
            (AppError::invalid_argument("m"), Some(ErrorCode::InvalidArg)),
            (AppError::not_found("m"), Some(ErrorCode::NotFound)),
            (upstream(UpstreamCause::Status(503)), Some(ErrorCode::UpstreamError)),
            (upstream(UpstreamCause::Network), Some(ErrorCode::UpstreamError)),
            (upstream(UpstreamCause::Timeout), Some(ErrorCode::UpstreamError)),
            (upstream(UpstreamCause::Deadline), Some(ErrorCode::UpstreamError)),
            (upstream(UpstreamCause::Decode), Some(ErrorCode::UpstreamError)),
            // Annulation : aucun code, personne ne lira la réponse (02 §9.2).
            (upstream(UpstreamCause::Cancelled), None),
            (
                AppError::RateLimited { message: "m".to_owned(), retry_after: None },
                Some(ErrorCode::RateLimited),
            ),
            (AppError::config("m"), Some(ErrorCode::ConfigError)),
            (audit(None), Some(ErrorCode::AuditError)),
            (audit(Some(ErrorCode::NotFound)), Some(ErrorCode::AuditError)),
        ];
        for (error, expected) in table {
            assert_eq!(error.code(), expected, "{error:?}");
        }
    }

    #[test]
    fn app_error_cause_only_on_upstream() {
        let upstream =
            AppError::Upstream { message: "m".to_owned(), cause: UpstreamCause::Status(500) };
        assert_eq!(upstream.cause(), Some(UpstreamCause::Status(500)));
        assert_eq!(AppError::not_found("m").cause(), None);
        assert_eq!(AppError::config("m").cause(), None);
    }

    #[test]
    fn app_error_message_of_every_variant() {
        let errors = [
            AppError::invalid_argument("a"),
            AppError::not_found("a"),
            AppError::Upstream { message: "a".to_owned(), cause: UpstreamCause::Network },
            AppError::RateLimited {
                message: "a".to_owned(),
                retry_after: Some(Duration::from_secs(3)),
            },
            AppError::config("a"),
            AppError::Audit { message: "a".to_owned(), original: None },
        ];
        for error in errors {
            assert_eq!(error.message(), "a", "{error:?}");
        }
    }

    #[test]
    fn input_error_becomes_invalid_argument_with_same_text() {
        let input = InputError { message: "page_size: invalid type".to_owned() };
        let error = AppError::from(input);
        assert_eq!(error, AppError::invalid_argument("page_size: invalid type"));
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
    }

    #[test]
    fn display_is_the_final_message() {
        let errors = [
            AppError::invalid_argument("final text"),
            AppError::not_found("final text"),
            AppError::Upstream { message: "final text".to_owned(), cause: UpstreamCause::Timeout },
            AppError::RateLimited { message: "final text".to_owned(), retry_after: None },
            AppError::config("final text"),
            AppError::Audit {
                message: "final text".to_owned(),
                original: Some(ErrorCode::NotFound),
            },
        ];
        for error in errors {
            assert_eq!(error.to_string(), "final text", "{error:?}");
        }
    }
}
