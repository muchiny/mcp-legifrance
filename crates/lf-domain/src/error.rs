//! Erreurs du domaine.

use thiserror::Error;

/// Longueur maximale, en caractères, d'une entrée recopiée dans un message d'erreur.
pub const ECHO_MAX_CHARS: usize = 64;

/// Erreur de validation d'une valeur du domaine.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// Identifiant Légifrance absent ou malformé.
    #[error("invalid Legifrance identifier {input:?}: expected {expected} followed by 12 digits")]
    InvalidId {
        /// Entrée reçue, tronquée à [`ECHO_MAX_CHARS`] caractères.
        input: String,
        /// Préfixe(s) accepté(s), ex. `LEGITEXT`.
        expected: &'static str,
    },
    /// Date absente ou malformée.
    #[error("invalid date {input:?}: expected YYYY-MM-DD")]
    InvalidDate {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// ECLI malformé.
    #[error("invalid ECLI {input:?}: expected ECLI:FR:<court>:<year>:<ordinal>")]
    InvalidEcli {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// Fond inconnu.
    #[error(
        "unknown fond {input:?}: expected one of LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO"
    )]
    UnknownFond {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// Déclaration de couverture sans lacune connue.
    #[error("coverage must declare at least one known gap")]
    EmptyCoverageGaps,
}

/// Recopie `input` pour un message d'erreur, tronquée à [`ECHO_MAX_CHARS`] caractères.
pub fn echo(input: &str) -> String {
    let mut chars = input.chars();
    let head: String = chars.by_ref().take(ECHO_MAX_CHARS).collect();
    if chars.next().is_some() { head + "…" } else { head }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        date::CivilDate,
        ecli::Ecli,
        fond::Fond,
        id::{AnyId, ArticleId, DecisionId, LegiTextId},
    };
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// Entrée recopiée dans l'erreur, quelle que soit la variante.
    fn echoed(error: &DomainError) -> &str {
        match error {
            DomainError::InvalidId { input, .. }
            | DomainError::InvalidDate { input }
            | DomainError::InvalidEcli { input }
            | DomainError::UnknownFond { input } => input,
            DomainError::EmptyCoverageGaps => "",
        }
    }

    #[test]
    fn echo_keeps_short_input() {
        assert_eq!(echo("LEGITEXT"), "LEGITEXT");
    }

    #[test]
    fn echo_keeps_input_of_exactly_max_chars() {
        let input = "é".repeat(ECHO_MAX_CHARS);
        assert_eq!(echo(&input), input);
    }

    #[test]
    fn echo_truncates_long_input_with_ellipsis() {
        let input = "x".repeat(ECHO_MAX_CHARS + 1);
        assert_eq!(echo(&input), format!("{}…", "x".repeat(ECHO_MAX_CHARS)));
    }

    #[test]
    fn messages_are_actionable() {
        let err = DomainError::InvalidId { input: "ABC".into(), expected: "LEGITEXT" };
        assert_eq!(
            err.to_string(),
            "invalid Legifrance identifier \"ABC\": expected LEGITEXT followed by 12 digits"
        );
        assert_eq!(
            DomainError::InvalidDate { input: "x".into() }.to_string(),
            "invalid date \"x\": expected YYYY-MM-DD"
        );
        assert_eq!(
            DomainError::InvalidEcli { input: "x".into() }.to_string(),
            "invalid ECLI \"x\": expected ECLI:FR:<court>:<year>:<ordinal>"
        );
        assert_eq!(
            DomainError::UnknownFond { input: "x".into() }.to_string(),
            "unknown fond \"x\": expected one of LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO"
        );
        assert_eq!(
            DomainError::EmptyCoverageGaps.to_string(),
            "coverage must declare at least one known gap"
        );
    }

    #[test]
    fn every_parser_echoes_a_truncated_input() {
        let long = "A".repeat(10_000);
        let errors = [
            LegiTextId::parse(&long).unwrap_err(),
            AnyId::parse(&long).unwrap_err(),
            ArticleId::parse(&long).unwrap_err(),
            DecisionId::parse(&long).unwrap_err(),
            CivilDate::parse(&long).unwrap_err(),
            Ecli::parse(&long).unwrap_err(),
            Fond::parse(&long).unwrap_err(),
        ];
        for error in &errors {
            assert_eq!(echoed(error), echo(&long));
            assert!(error.to_string().chars().count() < 250, "message too long: {error}");
        }
    }

    #[test]
    fn variants_without_input_echo_nothing() {
        assert_eq!(echoed(&DomainError::EmptyCoverageGaps), "");
    }

    #[test]
    fn messages_escape_control_characters() {
        let raw = "a\nb\0\u{1b}";
        let messages = [
            DomainError::InvalidId { input: raw.into(), expected: "LEGITEXT" }.to_string(),
            DomainError::InvalidDate { input: raw.into() }.to_string(),
        ];
        for message in &messages {
            assert!(message.contains(r"a\nb\0\u{1b}"), "not escaped: {message}");
            assert!(!message.contains('\n') && !message.contains('\0'), "raw control: {message}");
        }
    }

    proptest! {
        #[test]
        fn echo_truncates_at_max_chars(s in "\\PC{0,200}") {
            let out = echo(&s);
            let truncated = s.chars().count() > ECHO_MAX_CHARS;
            prop_assert!(out.chars().count() <= ECHO_MAX_CHARS + 1);
            if truncated {
                let head: String = s.chars().take(ECHO_MAX_CHARS).collect();
                prop_assert_eq!(out, format!("{head}…"));
            } else {
                prop_assert_eq!(out, s);
            }
        }
    }
}
