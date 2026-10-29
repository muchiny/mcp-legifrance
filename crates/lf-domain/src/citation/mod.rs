//! Parseur de citations juridiques (M6, annexe B de `docs/plan` et 03 §8) : reconnaissance,
//! spans, normalisation et forme canonique, sans entrée-sortie.
//!
//! Le pipeline de [`parse_with`] : normalisation avec table d'offsets (`normalize`), ancres
//! (`scan`), grammaires par famille (`grammar`), arbitrage et contexte (`post`). Les spans rendus
//! sont des offsets en octets du texte reçu ([`Span`]).
//!
//! STUB(V0) → V2-10 à V2-14 : le socle est livré (normalisation, types, dictionnaires), les
//! grammaires, l'arbitrage, la forme canonique et l'extrait cité arrivent en V2 ; d'ici là
//! [`parse_with`] ne reconnaît rien.

pub mod dict;
pub mod format;
mod grammar;
mod normalize;
mod post;
mod quote;
mod scan;
pub mod types;

pub use dict::codes::{CODES, CodeEntry, CodeKey, CodeRefKey};
pub use quote::adjacent_quote;
pub use types::*;

use crate::date::CivilDate;

/// Valeur par défaut de [`ParseOptions::max_citations`].
const DEFAULT_MAX_CITATIONS: usize = 1_000;

/// Réglages de l'analyse (annexe B §6.1, 00 §4).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParseOptions {
    /// Retirer les balises HTML et décoder les entités (défaut `false` ; `true` pour
    /// `citationJp` et les liens).
    pub strip_html: bool,
    /// Réparer le mojibake Latin-1 (défaut `false`).
    pub repair_mojibake: bool,
    /// Contexte implicite du document (défaut `None`).
    pub default_text: Option<TextContext>,
    /// Garde-fou du domaine (défaut 1 000) : au-delà, la sortie est tronquée. La borne métier de
    /// `fr_verify_citations` est appliquée par le cas d'usage, après l'analyse.
    pub max_citations: usize,
    /// Date du jour, pour les drapeaux `annee_future` et `annee_improbable` (défaut `None` : le
    /// domaine n'a pas d'horloge, l'application la fournit).
    pub today: Option<CivilDate>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            strip_html: false,
            repair_mojibake: false,
            default_text: None,
            max_citations: DEFAULT_MAX_CITATIONS,
            today: None,
        }
    }
}

/// Contexte d'un document : le code ou le texte dont il fait partie.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TextContext {
    /// Code du document (« l'article R. 132-5 » dans un article du code de l'urbanisme).
    pub code: Option<CodeKey>,
    /// Texte du document.
    pub text: Option<TextCite>,
}

/// Citations du texte, avec les réglages par défaut. Équivaut à
/// `parse_with(input, &ParseOptions::default())`.
pub fn parse(input: &str) -> Vec<Spanned<CitationRef>> {
    parse_with(input, &ParseOptions::default())
}

/// Citations du texte, triées par début de span puis par longueur décroissante. Aucune entrée ne
/// fait paniquer la fonction ; la pile de contexte repart de zéro à chaque appel.
pub fn parse_with(input: &str, opts: &ParseOptions) -> Vec<Spanned<CitationRef>> {
    let steps =
        normalize::Steps { strip_html: opts.strip_html, repair_mojibake: opts.repair_mojibake };
    let normalized = normalize::normalize(input, steps);
    let anchors = scan::find_anchors(&normalized);
    let candidates = grammar::scan_all(&normalized, &anchors);
    post::arbitrate(candidates, opts)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn parse_options_default_matches_annex_b_6_1() {
        let options = ParseOptions::default();
        assert!(!options.strip_html);
        assert!(!options.repair_mojibake);
        assert_eq!(options.default_text, None);
        assert_eq!(options.max_citations, 1_000);
        assert_eq!(options.today, None);
        assert_eq!(TextContext::default(), TextContext { code: None, text: None });
    }

    #[test]
    fn parse_is_parse_with_default_options() {
        for input in
            ["", "article 1240 du code civil", "<p>art. 9 C. civ.</p>", "n\u{c2}\u{b0} 21-23.174"]
        {
            assert_eq!(parse(input), parse_with(input, &ParseOptions::default()), "{input:?}");
        }
        let html =
            ParseOptions { strip_html: true, repair_mojibake: true, ..ParseOptions::default() };
        assert_eq!(parse_with("", &html), []);
    }

    #[test]
    fn the_dictionary_is_reachable_from_the_module_root() {
        assert_eq!(CODES.len(), 77);
        assert_eq!(CodeKey::CodeCivil.title(), "Code civil");
        assert_eq!(adjacent_quote("« x » art. 1", Span { start: 7, end: 12 }), None);
    }
}
