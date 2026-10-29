//! Termes de focalisation d'une lecture (`focus`, 03 §7.8) : mots recherchables, repliés et
//! dédoublonnés, qui servent au classement BM25 des passages.
//!
//! STUB(V0) → V1-05 : la forme et les signatures sont gelées par la vague 0 (V1-06 les consomme).
//! L'analyse est provisoire : découpe aux espaces, sans repli, sans mots vides ni borne de
//! longueur.

use super::{fold::fold, stopwords::is_stopword};
use crate::error::DomainError;

/// Longueur maximale, en caractères, de `focus` (03 §7.8).
pub const FOCUS_MAX_CHARS: usize = 500;

/// Termes de focalisation d'une lecture : au moins un, jamais vide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    terms: Vec<String>,
}

impl Focus {
    /// Analyse la valeur brute du paramètre `focus`.
    ///
    /// STUB(V0) → V1-05 : découpe aux espaces et écarte les mots vides (aucun pour l'instant).
    ///
    /// # Errors
    ///
    /// [`DomainError::FocusWithoutTerms`] si aucun mot recherchable ne reste.
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        // STUB(V0) → V1-05
        let terms: Vec<String> = raw
            .split_whitespace()
            .filter(|word| !is_stopword(&fold(word)))
            .map(str::to_owned)
            .collect();
        if terms.is_empty() { Err(DomainError::FocusWithoutTerms) } else { Ok(Self { terms }) }
    }

    /// Les termes retenus, dans l'ordre de saisie.
    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn stub_splits_on_whitespace() {
        // STUB(V0) → V1-05 : repli, mots vides et borne de longueur sont éprouvés là-bas.
        let focus = Focus::parse("  publicité   des débats ").unwrap();
        assert_eq!(focus.terms(), ["publicité", "des", "débats"]);
    }

    #[test]
    fn blank_focus_has_no_terms() {
        assert_eq!(Focus::parse(" \t\n").unwrap_err(), DomainError::FocusWithoutTerms);
        assert_eq!(Focus::parse("").unwrap_err(), DomainError::FocusWithoutTerms);
    }

    #[test]
    fn max_chars_is_the_documented_value() {
        assert_eq!(FOCUS_MAX_CHARS, 500);
    }
}
