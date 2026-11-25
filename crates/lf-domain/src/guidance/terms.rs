//! Mots d'une recherche dans les fiches de Service-Public (ADR-0033, décision 3 ; 05 §5.16).
//!
//! La recherche est locale : aucun mot ne part sur le réseau. La règle de validation est celle
//! du BOFiP (forme NFC, tout caractère qui n'est ni une lettre ni un chiffre devient une espace,
//! au moins un mot), pour que les deux outils de recherche refusent les mêmes requêtes. Chaque
//! mot a aussi une forme de comparaison, sans casse ni accents ([`fold_word`]), la même que
//! celle de l'index : c'est la règle mesurée à T-MS-03 (17 succès sur 20).

use unicode_normalization::{UnicodeNormalization, char::canonical_combining_class};

use crate::{doctrine::search_words, error::DomainError};

/// Mots d'une recherche : le texte validé, et la forme de comparaison de chaque mot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GuidanceTerms {
    /// Les mots en forme NFC, séparés par une espace.
    words: String,
    /// Les mots repliés par [`fold_word`], dans l'ordre, sans mot vide.
    folded: Vec<String>,
}

impl GuidanceTerms {
    /// Valide `input` comme une requête de `fr_search`, n'en garde que les mots, puis replie
    /// chaque mot.
    ///
    /// ```
    /// use lf_domain::guidance::GuidanceTerms;
    ///
    /// let terms = GuidanceTerms::parse("Congé de MATERNITÉ : l'indemnité ?")?;
    /// assert_eq!(terms.as_str(), "Congé de MATERNITÉ l indemnité");
    /// assert_eq!(terms.folded(), ["conge", "de", "maternite", "l", "indemnite"]);
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Celles de [`DoctrineTerms::parse`](crate::doctrine::DoctrineTerms::parse) ;
    /// [`DomainError::QueryWithoutWords`] aussi quand aucun mot ne survit au repli (un mot fait
    /// de seules marques combinantes).
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let words = search_words(input)?;
        let folded: Vec<String> =
            words.split(' ').map(fold_word).filter(|word| !word.is_empty()).collect();
        if folded.is_empty() {
            return Err(DomainError::QueryWithoutWords);
        }
        Ok(Self { words, folded })
    }

    /// Les mots, séparés par une espace.
    pub fn as_str(&self) -> &str {
        &self.words
    }

    /// La forme de comparaison de chaque mot, dans l'ordre de la requête.
    pub fn folded(&self) -> &[String] {
        &self.folded
    }
}

/// Forme de comparaison d'un mot : minuscules, puis décomposition canonique (NFD) dont les
/// marques combinantes sont retirées. « Maternité » devient « maternite », « ÉTÉ » « ete » ;
/// « œ » reste « œ », qui n'a pas de décomposition canonique.
///
/// ```
/// use lf_domain::guidance::fold_word;
///
/// assert_eq!(fold_word("Indemnité"), "indemnite");
/// assert_eq!(fold_word("ÇA"), "ca");
/// ```
pub fn fold_word(word: &str) -> String {
    let lower: String = word.chars().flat_map(char::to_lowercase).collect();
    lower.nfd().filter(|&c| canonical_combining_class(c) == 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{doctrine::DoctrineTerms, search::QUERY_MAX_CHARS};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn folded(input: &str) -> Vec<String> {
        GuidanceTerms::parse(input).unwrap().folded().to_vec()
    }

    #[test]
    fn accents_and_case_fold_for_matching() {
        assert_eq!(folded("Congé MATERNITÉ"), ["conge", "maternite"]);
        assert_eq!(folded("Œuvre naïf ÇA déjà"), ["œuvre", "naif", "ca", "deja"]);
        // Forme décomposée en entrée : même repli que la forme composée.
        assert_eq!(folded("conge\u{301}"), ["conge"]);
        assert_eq!(folded("Ångström ÿ"), ["angstrom", "y"]);
        // Les chiffres restent.
        assert_eq!(folded("CAP 2026"), ["cap", "2026"]);
    }

    #[test]
    fn words_are_the_same_as_for_the_bofip() {
        for input in ["plus-value immobilière", "l'abattement", "art. L. 80 A", "100\u{a0}000"] {
            let terms = GuidanceTerms::parse(input).unwrap();
            assert_eq!(terms.as_str(), DoctrineTerms::parse(input).unwrap().as_str());
            assert_eq!(terms.folded().len(), terms.as_str().split(' ').count());
        }
    }

    #[test]
    fn punctuation_only_query_refused_before_network() {
        for input in ["-", "...", "« … » ?", "\"\"", "()", "%", "_", "€ $"] {
            assert_eq!(GuidanceTerms::parse(input), Err(DomainError::QueryWithoutWords));
        }
        for input in ["", "   ", "\u{0}"] {
            assert_eq!(GuidanceTerms::parse(input), Err(DomainError::EmptyQuery));
        }
        assert_eq!(
            GuidanceTerms::parse(&"a".repeat(QUERY_MAX_CHARS + 1)),
            Err(DomainError::QueryTooLong { len: 451, max: 450 })
        );
    }

    /// U+0345 est une lettre pour Unicode (le mot passe la validation) mais une marque
    /// combinante (le repli l'efface) : un mot qui n'est fait que d'elle disparaît, et une
    /// requête qui n'a qu'elle est refusée.
    #[test]
    fn a_word_of_combining_marks_only_vanishes() {
        assert!('\u{345}'.is_alphanumeric());
        assert_eq!(fold_word("\u{345}"), "");
        assert_eq!(folded("congé \u{345}"), ["conge"]);
        assert_eq!(GuidanceTerms::parse("\u{345}"), Err(DomainError::QueryWithoutWords));
        assert_eq!(GuidanceTerms::parse("\u{345} \u{345}"), Err(DomainError::QueryWithoutWords));
    }

    proptest! {
        /// Chaque forme repliée est non vide, en minuscules, sans marque combinante, et se
        /// replie en elle-même.
        #[test]
        fn folded_words_are_stable(input in "\\PC{0,120}") {
            if let Ok(terms) = GuidanceTerms::parse(&input) {
                prop_assert!(!terms.folded().is_empty());
                for word in terms.folded() {
                    prop_assert!(!word.is_empty());
                    prop_assert!(!word.contains(' '));
                    prop_assert!(word.chars().all(|c| canonical_combining_class(c) == 0));
                    prop_assert_eq!(&fold_word(word), word);
                }
            }
        }
    }
}
