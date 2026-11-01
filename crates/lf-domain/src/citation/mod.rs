//! Parseur de citations juridiques (M6, annexe B de `docs/plan` et 03 §8) : reconnaissance,
//! spans, normalisation et forme canonique, sans entrée-sortie.
//!
//! Le pipeline de [`parse_with`] : normalisation avec table d'offsets (`normalize`), ancres
//! (`scan`), grammaires par famille (`grammar`), arbitrage et contexte (`post`). Les spans rendus
//! sont des offsets en octets du texte reçu ([`Span`]).
//!
//! Garanties de [`parse_with`] : aucune entrée ne le fait paniquer ; chaque span est un intervalle
//! non vide dont `input.get(start..end)` est `Some` ; la sortie est triée par début de span puis
//! par longueur décroissante ; la pile de contexte (dernier code, derniers textes) repart de zéro
//! à chaque appel ; la sortie compte au plus [`ParseOptions::max_citations`] citations, de sorte
//! qu'une sortie de cette longueur peut avoir été tronquée (le cas d'usage le signale).

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
///
/// Au plus [`ParseOptions::max_citations`] citations sont rendues, les premières du texte. Une
/// entrée de plus de 1 Mio est analysée jusqu'à la dernière frontière de caractère avant cette
/// borne. Avec `repair_mojibake`, toutes les citations d'un texte réparé portent le drapeau
/// `mojibake_repare` (la réparation est tout ou rien).
pub fn parse_with(input: &str, opts: &ParseOptions) -> Vec<Spanned<CitationRef>> {
    let steps =
        normalize::Steps { strip_html: opts.strip_html, repair_mojibake: opts.repair_mojibake };
    let normalized = normalize::normalize(input, steps);
    let anchors = scan::find_anchors(&normalized);
    let candidates = grammar::scan_all(&normalized, &anchors);
    finish(post::arbitrate(candidates, opts), input, normalized.mojibake_repaired)
}

/// Dernière garde de [`parse_with`] : écarte toute citation dont le span ne désigne pas un extrait
/// non vide de `input` (aucune grammaire ne doit en produire), et marque les citations d'un texte
/// dont le mojibake a été réparé.
fn finish(
    citations: Vec<Spanned<CitationRef>>,
    input: &str,
    mojibake_repaired: bool,
) -> Vec<Spanned<CitationRef>> {
    citations
        .into_iter()
        .filter(|c| c.span.start < c.span.end && input.get(c.span.start..c.span.end).is_some())
        .map(|mut citation| {
            if mojibake_repaired && !citation.flags.contains(&Flag::MojibakeRepare) {
                citation.flags.push(Flag::MojibakeRepare);
            }
            citation
        })
        .collect()
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
        assert_eq!(adjacent_quote("", Span { start: 0, end: 0 }), None);
    }

    fn cited(start: usize, end: usize) -> Spanned<CitationRef> {
        Spanned {
            value: CitationRef::Unsupported(UnsupportedCite {
                family: UnsupportedFamily::Doctrine,
                key: None,
            }),
            span: Span { start, end },
            confidence: Confidence::High,
            ambiguity: None,
            flags: Vec::new(),
            enum_group: None,
        }
    }

    #[test]
    fn finish_drops_spans_that_do_not_cut_the_input() {
        let input = "é et 1";
        // Valide ; vide ; inversé ; hors du texte ; au milieu du « é » (2 octets).
        let kept = finish(
            vec![cited(0, 2), cited(3, 3), cited(5, 4), cited(4, 99), cited(0, 1)],
            input,
            false,
        );
        assert_eq!(kept, [cited(0, 2)]);
        assert_eq!(finish(Vec::new(), input, true), []);
    }

    /// Vrai si la sortie est triée par début de span puis par longueur décroissante.
    fn in_output_order(citations: &[Spanned<CitationRef>]) -> bool {
        let key = |c: &Spanned<CitationRef>| (c.span.start, std::cmp::Reverse(c.span.end));
        citations.windows(2).all(|w| key(&w[0]) <= key(&w[1]))
    }

    #[test]
    fn output_order_is_start_then_longest_first() {
        assert!(in_output_order(&[]));
        assert!(in_output_order(&[cited(0, 9), cited(0, 4), cited(4, 6)]));
        assert!(!in_output_order(&[cited(0, 4), cited(0, 9)]));
        assert!(!in_output_order(&[cited(5, 6), cited(2, 3)]));
    }

    #[test]
    fn finish_marks_every_citation_of_a_repaired_text_once() {
        let input = "art. 1 et art. 2";
        let mut already = cited(11, 16);
        already.flags = vec![Flag::MojibakeRepare, Flag::DateInvalide];
        let marked = finish(vec![cited(0, 6), already], input, true);
        assert_eq!(marked[0].flags, [Flag::MojibakeRepare]);
        assert_eq!(marked[1].flags, [Flag::MojibakeRepare, Flag::DateInvalide]);
        // Texte non réparé : les drapeaux restent ceux de l'arbitrage.
        let plain = finish(vec![cited(0, 6)], input, false);
        assert_eq!(plain[0].flags, []);
    }

    #[test]
    fn parse_options_are_applied_by_the_pipeline() {
        // Les garde-fous traversent le pipeline sans citation à reconnaître : pas de panique,
        // sortie vide, quelle que soit la combinaison de réglages.
        for strip_html in [false, true] {
            for repair_mojibake in [false, true] {
                let opts = ParseOptions {
                    strip_html,
                    repair_mojibake,
                    default_text: Some(TextContext::default()),
                    max_citations: 0,
                    today: CivilDate::new(2026, 10, 15),
                };
                assert_eq!(parse_with("<b>article L. 1 du code civil</b> \u{c3}\u{a9}", &opts), []);
            }
        }
    }

    #[test]
    fn versioned_legifrance_ids_are_found_by_the_pipeline() {
        // P198 : le suffixe `_JJ-MM-AAAA` suit l'identifiant ; l'ancre ne doit pas s'y arrêter.
        let input = "voir LEGITEXT000033205014_08-09-2023.";
        let found = parse(input);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            input.get(found[0].span.start..found[0].span.end),
            Some("LEGITEXT000033205014_08-09-2023")
        );
        assert!(matches!(
            &found[0].value,
            CitationRef::LegifranceId(id)
                if id.digits == 33_205_014 && id.version_start == CivilDate::new(2023, 9, 8)
        ));
        // Sans suffixe, ou avec un suffixe qui n'est pas une date, l'identifiant reste cité seul.
        assert_eq!(parse("LEGITEXT000033205014").len(), 1);
        let loose = parse("LEGITEXT000033205014_foo");
        assert!(
            matches!(&loose[..], [c] if matches!(&c.value, CitationRef::LegifranceId(id) if id.version_start.is_none()))
        );
    }

    #[test]
    fn parse_calls_are_independent() {
        // Deux appels de suite sur le même texte rendent la même chose : rien ne subsiste du
        // premier (dernier code, derniers textes). La pile elle-même est testée dans `post`.
        let opts =
            ParseOptions { default_text: Some(TextContext::default()), ..ParseOptions::default() };
        let text = "l'article 1240 du code civil et du même code, la loi n° 2016-1321 susvisée";
        let first = parse_with(text, &opts);
        let alone = parse_with("du même code, l'article 5", &opts);
        assert_eq!(first, parse_with(text, &opts));
        assert_eq!(alone, parse_with("du même code, l'article 5", &opts));
    }

    /// Citations complètes (code compris) du corpus positif, dont les formes de L1-01.
    const COMPLETE: [&str; 10] = [
        "l'article 1240 du code civil",
        "art. L. 1234-5 du code du travail",
        "CGI, art. 150-0 B ter",
        "article 150-0 B ter du code général des impôts",
        "art. 199 terdecies-0 A du CGI",
        "art. 199 undecies B du CGI",
        "art. 787 B CGI",
        "C. civ., art. 9",
        "art. 121-3 CP",
        "art. 163 quatervicies du CGI",
    ];

    mod no_panic {
        use proptest::prelude::*;

        use super::*;

        proptest! {
            #[test]
            fn no_panic_on_arbitrary_input(
                input in "\\PC{0,200}",
                strip_html in any::<bool>(),
                repair_mojibake in any::<bool>(),
                max in 0usize..5,
            ) {
                let opts = ParseOptions {
                    strip_html,
                    repair_mojibake,
                    max_citations: max,
                    default_text: Some(TextContext::default()),
                    today: CivilDate::new(2026, 10, 15),
                };
                let out = parse_with(&input, &opts);
                prop_assert!(out.len() <= max);
                for c in &out {
                    prop_assert!(c.span.start < c.span.end);
                    prop_assert!(input.get(c.span.start..c.span.end).is_some());
                }
            }

            #[test]
            fn no_panic_on_citation_shaped_input(
                parts in prop::collection::vec(
                    prop::sample::select(vec![
                        "article ", "art. ", "L. ", "R. 12-3", "1240", " du code civil", " C. civ.",
                        " et ", ", ", "loi n° 2016-1321", " du 7 octobre 2016", " susvisée",
                        "Cass. civ. 1re, 5 mai 2021, n° 19-12.345", "ECLI:FR:CCASS:2021:C100399",
                        "NOR : ECFI1524250L", "150-0 B ter", "199 terdecies-0 A", " bis", " ter", "-0 ", "<p>", "</p>", "&nbsp;", "Ã©", "\u{200b}", "\n",
                    ]),
                    0..20,
                ),
            ) {
                let input = parts.concat();
                let opts = ParseOptions {
                    strip_html: true,
                    repair_mojibake: true,
                    ..ParseOptions::default()
                };
                for c in parse_with(&input, &opts) {
                    prop_assert!(input.get(c.span.start..c.span.end).is_some());
                }
                prop_assert!(in_output_order(&parse(&input)));
            }

            /// Composition : `parse(A ; B) == parse(A) ++ parse(B)` pour deux citations complètes
            /// (code compris). Les fragments non autonomes (« art. 9 et 10 du code civil », un
            /// article sans code suivi d'un code) sont exclus : l'invariant est faux pour eux.
            #[test]
            fn parsing_composes_over_complete_citations(
                first in 0..COMPLETE.len(),
                second in 0..COMPLETE.len(),
                sep in prop::sample::select(vec![" ; ", ". ", ", ", " et "]),
            ) {
                let (a, b) = (COMPLETE[first], COMPLETE[second]);
                let values = |input: &str| -> Vec<CitationRef> {
                    parse(input).into_iter().map(|c| c.value).collect()
                };
                let mut expected = values(a);
                expected.extend(values(b));
                prop_assert_eq!(values(&format!("{a}{sep}{b}")), expected);
            }
        }
    }
}
