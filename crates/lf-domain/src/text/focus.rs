//! Termes de focalisation d'une lecture (`focus`, 03 §7.8) : mots recherchables, repliés et
//! dédoublonnés, qui servent au classement BM25 des passages.
//!
//! La découpe est celle de [`rank`](crate::text::rank) : les termes d'un [`Focus`], joints par
//! des espaces, redonnent les mêmes jetons quand `rank` découpe la requête. Les numéros
//! juridiques (`24-13.470`, `2025-1173`, `761-1`, `85/374`) restent donc des termes entiers.

use super::{rank::legal_tokens, stopwords::is_stopword};
use crate::error::DomainError;

/// Longueur maximale, en caractères, de `focus` (03 §7.8).
pub const FOCUS_MAX_CHARS: usize = 500;

/// Termes de focalisation d'une lecture : au moins un, jamais vide.
///
/// Le nombre de termes est borné par [`FOCUS_MAX_CHARS`] (M1-D3) : au plus 250, car un terme
/// occupe au moins un caractère et deux termes sont séparés par au moins un caractère.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    terms: Vec<String>,
}

impl Focus {
    /// Analyse la valeur brute du paramètre `focus` : blancs de bord retirés, au plus
    /// [`FOCUS_MAX_CHARS`] caractères, puis découpe en jetons repliés, sans mot vide ni doublon.
    ///
    /// # Errors
    ///
    /// - [`DomainError::OutOfRange`] (`focus`, 1 à 500) si la valeur, une fois ses blancs de bord
    ///   retirés, dépasse [`FOCUS_MAX_CHARS`] caractères ;
    /// - [`DomainError::FocusWithoutTerms`] si aucun mot recherchable ne reste (vide, ponctuation
    ///   seule ou mots vides seuls).
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let trimmed = raw.trim();
        let len = trimmed.chars().count();
        if len > FOCUS_MAX_CHARS {
            return Err(DomainError::OutOfRange {
                field: "focus",
                value: i64::try_from(len).unwrap_or(i64::MAX),
                min: 1,
                max: i64::try_from(FOCUS_MAX_CHARS).unwrap_or(i64::MAX),
            });
        }
        let mut terms: Vec<String> = Vec::new();
        for token in legal_tokens(trimmed) {
            if !is_stopword(&token) && !terms.contains(&token) {
                terms.push(token);
            }
        }
        if terms.is_empty() { Err(DomainError::FocusWithoutTerms) } else { Ok(Self { terms }) }
    }

    /// Les termes retenus, repliés et distincts, dans l'ordre de première apparition.
    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn focus_rejects_only_stopwords() {
        for raw in [
            "le de la",
            "Les  DES  Du",
            "au",
            "l'",
            "à de ou",
            "Pour moi, sans lui",
            "!!! ... ?",
            "§ - / .",
            " \t\n",
            "",
        ] {
            assert_eq!(Focus::parse(raw).unwrap_err(), DomainError::FocusWithoutTerms, "{raw:?}");
        }
    }

    #[test]
    fn stopwords_are_dropped_around_meaningful_words() {
        let focus = Focus::parse("le bail de la mobilité, sans le préavis").unwrap();
        assert_eq!(focus.terms(), ["bail", "mobilite", "preavis"]);
        // Les mots juridiquement porteurs ne sont pas des mots vides.
        let focus = Focus::parse("non conformité de la loi et du code").unwrap();
        assert_eq!(focus.terms(), ["non", "conformite", "loi", "code"]);
    }

    #[test]
    fn focus_terms_folded_and_deduplicated() {
        let focus =
            Focus::parse("  Publicité des DÉBATS, publicite débats ; Œuvre œuvre  ").unwrap();
        assert_eq!(focus.terms(), ["publicite", "debats", "oeuvre"]);
        // Ordre de première apparition, quelle que soit la casse ou l'accent.
        let focus = Focus::parse("Zèbre alpha ZEBRE Alpha beta").unwrap();
        assert_eq!(focus.terms(), ["zebre", "alpha", "beta"]);
        // Un même terme séparé par de la ponctuation ou une apostrophe n'est gardé qu'une fois.
        let focus = Focus::parse("l'État, l'etat; ÉTAT").unwrap();
        assert_eq!(focus.terms(), ["etat"]);
    }

    #[test]
    fn legal_numbers_stay_whole_terms() {
        let focus =
            Focus::parse("article 761-1 décision 2025-1173 QPC directive 85/374 décret 24-13.470")
                .unwrap();
        assert_eq!(
            focus.terms(),
            [
                "article",
                "761-1",
                "decision",
                "2025-1173",
                "qpc",
                "directive",
                "85/374",
                "decret",
                "24-13.470"
            ]
        );
    }

    #[test]
    fn focus_over_500_chars_rejected() {
        let word = "a".repeat(501);
        assert_eq!(
            Focus::parse(&word).unwrap_err(),
            DomainError::OutOfRange { field: "focus", value: 501, min: 1, max: 500 }
        );
        // 500 caractères exactement : accepté, un seul terme.
        assert_eq!(Focus::parse(&"a".repeat(500)).unwrap().terms(), ["a".repeat(500)]);
        // Les blancs de bord ne comptent pas.
        let padded = format!("  \n{}\t ", "b".repeat(500));
        assert_eq!(Focus::parse(&padded).unwrap().terms(), ["b".repeat(500)]);
        // La longueur se compte en caractères, pas en octets (500 × « é » = 1 000 octets).
        assert_eq!(Focus::parse(&"é".repeat(500)).unwrap().terms(), ["e".repeat(500)]);
        assert_eq!(
            Focus::parse(&"é".repeat(501)).unwrap_err(),
            DomainError::OutOfRange { field: "focus", value: 501, min: 1, max: 500 }
        );
        // La borne passe avant les mots vides : 600 caractères de mots vides sont refusés
        // comme trop longs, pas comme vides.
        let stop = "le ".repeat(200);
        assert_eq!(
            Focus::parse(&stop).unwrap_err(),
            DomainError::OutOfRange { field: "focus", value: 599, min: 1, max: 500 }
        );
    }

    #[test]
    fn focus_terms_are_bounded_by_the_length_cap() {
        // Pire cas : le plus de jetons distincts de 2 caractères dans 500 caractères.
        let raw: String = ('a'..='z')
            .flat_map(|a| ('a'..='z').map(move |b| format!("{a}{b}")))
            .filter(|token| !is_stopword(token))
            .collect::<Vec<_>>()
            .join(" ");
        let focus = Focus::parse(&raw[..FOCUS_MAX_CHARS]).unwrap();
        let count = focus.terms().len();
        assert!((100..=167).contains(&count), "{count}");
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

    proptest! {
        /// Tout focus accepté a au moins un terme, sans mot vide ni doublon, et ses termes
        /// joints par des espaces se redécoupent en les mêmes termes (c'est ainsi que `rank`
        /// reçoit la requête).
        #[test]
        fn parsed_terms_are_distinct_searchable_and_stable(raw in "\\PC{0,200}") {
            if let Ok(focus) = Focus::parse(&raw) {
                let terms = focus.terms();
                prop_assert!(!terms.is_empty());
                prop_assert!(terms.iter().all(|t| !is_stopword(t)));
                let mut distinct = terms.to_vec();
                distinct.sort();
                distinct.dedup();
                prop_assert_eq!(distinct.len(), terms.len());
                prop_assert_eq!(legal_tokens(&terms.join(" ")), terms.to_vec());
            }
        }
    }
}
