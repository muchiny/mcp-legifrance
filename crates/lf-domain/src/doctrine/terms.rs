//! Mots d'une recherche dans le BOFiP (ADR-0029, décision 3).
//!
//! La recherche du portail (`search()`) exige tous les mots, prend le dernier pour un préfixe et
//! n'a aucun opérateur. Elle retrouve aussi les mots approchants : la règle des mots exacts est
//! celle de [`DoctrineMatch`](super::DoctrineMatch) (ADR-0031), sur les mêmes mots. Mesures du
//! 2026-11-15 :
//!
//! - un mot à trait d'union qui n'est pas le dernier ne trouve rien (« plus-value immobilière » :
//!   0 résultat ; « plus value immobilière » : 357) ;
//! - une entrée Unicode décomposée trouve moins (18 résultats contre 75 en forme composée) ;
//! - un guillemet non échappé dans le littéral change le filtre envoyé au portail.
//!
//! D'où la règle, appliquée avant tout appel : forme NFC, puis tout caractère qui n'est ni une
//! lettre ni un chiffre devient un espace. Il ne reste que des mots séparés par une espace : ni
//! guillemet, ni barre oblique inverse, ni parenthèse. L'adapter échappe quand même le littéral
//! (défense en profondeur), mais il n'a plus rien à échapper.

use unicode_normalization::UnicodeNormalization;

use crate::{
    error::DomainError,
    search::{QUERY_MAX_CHARS, QueryText},
    text::is_invisible,
};

/// Mots d'une recherche : lettres et chiffres en forme NFC, séparés par une seule espace, au
/// moins un mot, [`QUERY_MAX_CHARS`] caractères au plus.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DoctrineTerms(String);

impl DoctrineTerms {
    /// Valide `input` comme une requête de `fr_search` ([`QueryText::parse`] : 1 à
    /// [`QUERY_MAX_CHARS`] caractères une fois les blancs repliés), puis n'en garde que les mots.
    ///
    /// ```
    /// use lf_domain::doctrine::DoctrineTerms;
    ///
    /// let terms = DoctrineTerms::parse("plus-value immobilière : l'abattement")?;
    /// assert_eq!(terms.as_str(), "plus value immobilière l abattement");
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    ///
    /// Une requête sans lettre ni chiffre est refusée, sans appel réseau :
    ///
    /// ```
    /// use lf_domain::{DomainError, doctrine::DoctrineTerms};
    ///
    /// assert_eq!(DoctrineTerms::parse("« … » ?"), Err(DomainError::QueryWithoutWords));
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::EmptyQuery`] et [`DomainError::QueryTooLong`] comme [`QueryText::parse`] ;
    /// [`DomainError::QueryWithoutWords`] s'il ne reste aucun mot ; [`DomainError::QueryTooLong`]
    /// aussi quand la forme NFC allonge la requête au-delà de la borne (certaines lettres s'y
    /// écrivent en plusieurs caractères).
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        search_words(input).map(Self)
    }

    /// Les mots, séparés par une espace.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Mots d'une requête de recherche, règle commune au BOFiP et à Service-Public (ADR-0033,
/// décision 3) : `input` validé comme une requête de `fr_search`, puis réduit à ses mots
/// ([`DoctrineTerms::parse`] en donne les erreurs).
pub(crate) fn search_words(input: &str) -> Result<String, DomainError> {
    let query = QueryText::parse(input)?;
    let words = plain_words(query.as_str());
    match words.chars().count() {
        0 => Err(DomainError::QueryWithoutWords),
        len if len > QUERY_MAX_CHARS => {
            Err(DomainError::QueryTooLong { len, max: QUERY_MAX_CHARS })
        }
        _ => Ok(words),
    }
}

/// Mots de `text` : forme NFC, tout caractère qui n'est pas un caractère de mot remplacé par une
/// espace, mots joints par une seule espace.
pub(crate) fn plain_words(text: &str) -> String {
    let spaced: String = text.nfc().map(|c| if is_word_char(c) { c } else { ' ' }).collect();
    spaced.split(' ').filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Caractère d'un mot : une lettre ou un chiffre, sauf ceux qui ne se voient pas. U+3164
/// (remplissage hangûl) est une lettre pour Unicode et ne s'affiche pas : gardée, elle
/// souderait deux mots en un seul que le portail ne trouverait pas.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() && !is_invisible(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn words(input: &str) -> String {
        DoctrineTerms::parse(input).unwrap().as_str().to_owned()
    }

    #[test]
    fn keeps_letters_and_digits() {
        assert_eq!(words("abattement 100 000 euros"), "abattement 100 000 euros");
        assert_eq!(words("Donation ÉPOUX œuvre naïf"), "Donation ÉPOUX œuvre naïf");
        assert_eq!(words("article 790 G"), "article 790 G");
        assert_eq!(words("m² ½ Ⅳ"), "m² ½ Ⅳ");
    }

    #[test]
    fn punctuation_becomes_one_space() {
        for (input, expected) in [
            ("plus-value immobilière", "plus value immobilière"),
            ("l'abattement", "l abattement"),
            ("l’abattement", "l abattement"),
            ("micro-BIC, meublé (tourisme) : abattement ?", "micro BIC meublé tourisme abattement"),
            ("a - - b", "a b"),
            ("...donation...", "donation"),
            ("art. L. 80 A", "art L 80 A"),
            ("TVA/IS", "TVA IS"),
            ("50%", "50"),
            ("a_b", "a b"),
        ] {
            assert_eq!(words(input), expected);
        }
    }

    /// Les caractères qui changeraient le filtre du portail s'ils atteignaient le littéral.
    #[test]
    fn query_language_characters_never_survive() {
        assert_eq!(words("donation\") or search(titre,\"x"), "donation or search titre x");
        assert_eq!(words("a\\b\\\"c"), "a b c");
        assert_eq!(words("a\"b'c`d"), "a b c d");
        assert_eq!(words("type=\"Contenu\""), "type Contenu");
    }

    #[test]
    fn punctuation_only_query_refused_before_network() {
        for input in ["-", "...", "« … » ?", "\"\"", "\\", "()", "' '", "%", "_", "+-*/", "€ $"]
        {
            assert_eq!(DoctrineTerms::parse(input), Err(DomainError::QueryWithoutWords));
        }
    }

    #[test]
    fn empty_and_blank_queries_are_required() {
        for input in ["", "   ", "\t\n", "\u{0}\u{1}"] {
            assert_eq!(DoctrineTerms::parse(input), Err(DomainError::EmptyQuery));
        }
    }

    #[test]
    fn invisible_letters_and_nfd_normalized() {
        // Forme décomposée (`e` puis accent combinant) : rendue en forme composée.
        assert_eq!(words("immobilie\u{300}re de\u{301}ce\u{300}s"), "immobilière décès");
        assert_eq!(words("immobilie\u{300}re").chars().count(), 11);
        // Lettres que Unicode range parmi les lettres mais qui ne s'affichent pas : U+3164,
        // U+115F, U+1160, U+FFA0. Elles séparent les mots au lieu de les souder.
        for filler in ['\u{3164}', '\u{115f}', '\u{1160}', '\u{ffa0}'] {
            assert!(filler.is_alphanumeric() && is_invisible(filler));
            assert_eq!(words(&format!("plus{filler}value")), "plus value");
            assert_eq!(
                DoctrineTerms::parse(&filler.to_string()),
                Err(DomainError::QueryWithoutWords)
            );
        }
        // Largeur nulle, marque de sens d'écriture, trait d'union conditionnel.
        assert_eq!(words("plus\u{200b}value\u{202e} im\u{ad}mobilière"), "plus value im mobilière");
        // Espace insécable et espace fine.
        assert_eq!(words("100\u{a0}000\u{202f}euros"), "100 000 euros");
    }

    #[test]
    fn length_is_checked_before_and_after_normalization() {
        let longest = "a".repeat(QUERY_MAX_CHARS);
        assert_eq!(words(&longest), longest);
        assert_eq!(
            DoctrineTerms::parse(&"a".repeat(QUERY_MAX_CHARS + 1)),
            Err(DomainError::QueryTooLong { len: 451, max: 450 })
        );
        // U+FB2C s'écrit en trois lettres en forme NFC : la requête s'allonge de deux caractères.
        assert_eq!("\u{fb2c}".nfc().count(), 3);
        assert!("\u{fb2c}".nfc().all(is_word_char));
        let at_limit = format!("{}\u{fb2c}", "a".repeat(447));
        assert_eq!(words(&at_limit).chars().count(), 450);
        let over_limit = format!("{}\u{fb2c}", "a".repeat(448));
        assert_eq!(over_limit.chars().count(), 449);
        assert_eq!(
            DoctrineTerms::parse(&over_limit),
            Err(DomainError::QueryTooLong { len: 451, max: 450 })
        );
    }

    proptest! {
        /// Quelle que soit l'entrée, ce qui est accepté n'est fait que de mots : aucun caractère
        /// du langage de requête du portail, aucun caractère invisible, espaces simples.
        #[test]
        fn accepted_terms_are_plain_words(input in "\\PC{0,200}") {
            if let Ok(terms) = DoctrineTerms::parse(&input) {
                let text = terms.as_str();
                prop_assert!(text.chars().all(|c| c == ' ' || is_word_char(c)), "{text:?}");
                prop_assert!(!text.contains("  ") && text.trim() == text && !text.is_empty());
                prop_assert!((1..=QUERY_MAX_CHARS).contains(&text.chars().count()));
                prop_assert_eq!(text.nfc().collect::<String>(), text);
                prop_assert_eq!(DoctrineTerms::parse(text), Ok(terms.clone()));
            }
        }

        /// Une entrée sans lettre ni chiffre n'est jamais acceptée.
        #[test]
        fn wordless_input_is_never_accepted(input in "[ -/:-@\\[-`{-~«»…€]{0,40}") {
            prop_assert!(DoctrineTerms::parse(&input).is_err());
        }
    }
}
