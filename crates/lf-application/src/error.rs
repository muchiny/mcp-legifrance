//! Codes d'erreur d'outil exposés au modèle (02 §9, 05 §7).
//!
//! Le plan 3a, tâche 2, ajoute dans ce fichier `UpstreamCause` et `AppError` ; la tâche 1 n'en
//! pose que le code, dont `AuditRecord` dépend.

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
}
