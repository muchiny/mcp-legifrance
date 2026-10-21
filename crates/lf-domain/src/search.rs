//! Objets de valeur de `fr_search` : texte de la requête, taille de page, mode de
//! correspondance (03 §4.1, 00 §7).
//!
//! Validés sans réseau : une valeur construite est déjà conforme, l'adapter n'a plus qu'à la
//! traduire.

use crate::error::DomainError;

/// Longueur maximale, en caractères, d'une requête normalisée.
///
/// La borne de 500 est celle du contrat (05 §7.3) ; la limite réelle de l'API reste à
/// vérifier [V-81 · longueur-max-query].
pub const QUERY_MAX_CHARS: usize = 500;

/// Texte de recherche normalisé : sans caractère de contrôle, blancs repliés en une espace,
/// ni vide ni plus long que [`QUERY_MAX_CHARS`] caractères.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryText(String);

impl QueryText {
    /// Normalise `input` puis le valide.
    ///
    /// La normalisation retire les blancs de tête et de queue, replie chaque suite de blancs
    /// Unicode en une espace et supprime les caractères de contrôle. La longueur est mesurée
    /// après normalisation. Borne : [V-81 · longueur-max-query].
    ///
    /// ```
    /// use lf_domain::search::QueryText;
    ///
    /// let query = QueryText::parse("  droit \t de\u{a0}grève \u{0}").unwrap();
    /// assert_eq!(query.as_str(), "droit de grève");
    /// ```
    ///
    /// Une requête faite de blancs ou de contrôles est refusée :
    ///
    /// ```
    /// use lf_domain::{DomainError, search::QueryText};
    ///
    /// assert_eq!(QueryText::parse(" \t\u{1} "), Err(DomainError::EmptyQuery));
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::EmptyQuery`] si rien ne reste après normalisation,
    /// [`DomainError::QueryTooLong`] au-delà de [`QUERY_MAX_CHARS`] caractères.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let normalized = normalize(input);
        match normalized.chars().count() {
            0 => Err(DomainError::EmptyQuery),
            len if len > QUERY_MAX_CHARS => {
                Err(DomainError::QueryTooLong { len, max: QUERY_MAX_CHARS })
            }
            _ => Ok(Self(normalized)),
        }
    }

    /// Texte normalisé.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Découpe sur les blancs Unicode, retire les caractères de contrôle de chaque mot, écarte les
/// mots devenus vides et joint par une espace.
fn normalize(input: &str) -> String {
    input
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Nombre de résultats par page de `fr_search`, entre [`MIN`](Self::MIN) et
/// [`MAX`](Self::MAX) inclus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageSize(u32);

impl PageSize {
    /// Plus petite taille de page.
    pub const MIN: u32 = 1;
    /// Plus grande taille de page.
    pub const MAX: u32 = 100;
    /// Taille de page par défaut (parité avec la référence).
    pub const DEFAULT: Self = Self(10);

    /// Valide `value`.
    ///
    /// # Errors
    ///
    /// [`DomainError::OutOfRange`] hors de `MIN..=MAX`.
    pub fn new(value: u32) -> Result<Self, DomainError> {
        Self::checked(value)
    }

    /// Valeur numérique.
    pub fn get(self) -> u32 {
        self.0
    }

    fn checked(value: u32) -> Result<Self, DomainError> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::OutOfRange {
                field: "page_size",
                value: i64::from(value),
                min: i64::from(Self::MIN),
                max: i64::from(Self::MAX),
            })
        }
    }
}

impl Default for PageSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Mode de correspondance des mots de la requête (paramètre `match`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MatchMode {
    /// Au moins un des mots (`UN_DES_MOTS`) ; défaut, par parité avec la référence.
    #[default]
    Any,
    /// Tous les mots (`TOUS_LES_MOTS_DANS_UN_CHAMP`).
    All,
    /// Expression exacte (`EXACTE`).
    Exact,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DomainError;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn text(s: &str) -> QueryText {
        QueryText::parse(s).unwrap()
    }

    #[test]
    fn query_is_trimmed_and_folded() {
        assert_eq!(text("  droit \t\r\n de\u{a0}\u{a0}grève  ").as_str(), "droit de grève");
    }

    #[test]
    fn query_control_chars_are_stripped() {
        assert_eq!(text("a\u{0}b \u{1} c\u{7f}\u{9b}").as_str(), "ab c");
    }

    #[test]
    fn blank_query_is_empty() {
        for blank in ["", "   ", "\t\n", "\u{0}\u{1}", " \u{7} \u{1b} "] {
            assert_eq!(QueryText::parse(blank), Err(DomainError::EmptyQuery), "{blank:?}");
        }
    }

    #[test]
    fn query_too_long_is_invalid_arg() {
        let at_max = "é".repeat(QUERY_MAX_CHARS);
        assert_eq!(text(&at_max).as_str(), at_max);
        assert_eq!(
            QueryText::parse(&"é".repeat(QUERY_MAX_CHARS + 1)),
            Err(DomainError::QueryTooLong { len: 501, max: 500 })
        );
    }

    #[test]
    fn query_length_is_counted_after_normalization() {
        let padded = format!("  {}  \u{0}", "a".repeat(QUERY_MAX_CHARS));
        assert_eq!(text(&padded).as_str().len(), QUERY_MAX_CHARS);
    }

    #[test]
    fn page_size_bounds() {
        let out = |value| DomainError::OutOfRange { field: "page_size", value, min: 1, max: 100 };
        assert_eq!(PageSize::new(0), Err(out(0)));
        assert_eq!(PageSize::new(1).map(PageSize::get), Ok(1));
        assert_eq!(PageSize::new(100).map(PageSize::get), Ok(100));
        assert_eq!(PageSize::new(101), Err(out(101)));
    }

    #[test]
    fn defaults_match_reference() {
        assert_eq!(PageSize::default().get(), 10);
        assert_eq!(MatchMode::default(), MatchMode::Any);
    }

    proptest! {
        #[test]
        fn parse_never_panics_and_output_is_normal(s in "\\PC*|[\\x00-\\x20a ]*") {
            if let Ok(q) = QueryText::parse(&s) {
                let t = q.as_str();
                prop_assert!(!t.chars().any(char::is_control));
                prop_assert!(!t.contains("  ") && t.trim() == t && !t.is_empty());
                prop_assert!(t.chars().count() <= QUERY_MAX_CHARS);
                prop_assert_eq!(QueryText::parse(t), Ok(q.clone()));
            }
        }
    }
}
