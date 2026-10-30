//! Résumé d'un texte long (`detail="summary"`, 03 §7.9) : sommaire officiel, sinon dispositif,
//! sinon début du texte.
//!
//! Ordre de préférence (00 §4) :
//!
//! 1. le sommaire officiel de la décision : `analyses`, sinon `titrages`, joints par une ligne
//!    vide ;
//! 2. le dispositif, quand la juridiction a une [`DispositifRule`] et que ses deux marqueurs sont
//!    dans le texte ;
//! 3. le début du texte, jusqu'à au moins [`LEAD_MIN_CHARS`] caractères.
//!
//! Dans tous les cas le texte rendu tient dans `min(max_chars, SUMMARY_CAP_CHARS)` caractères et
//! est coupé, de préférence, en fin de phrase. Les paragraphes purement décoratifs (tirets,
//! soulignés, égalités, étoiles et blancs, comme la ligne de tirets du Conseil d'État) sont
//! ignorés par les étapes 2 et 3 (03 §7.2). Les marqueurs de dispositif ont été validés sur une
//! décision de chaque juridiction : les variantes anciennes sont NON VÉRIFIÉES (V-110, V-111) et
//! retombent sur le début du texte.

use std::fmt;

use super::LongText;
use super::fold::fold;
use super::window::window;
use crate::entity::OfficialSummary;

/// Séparateur des blocs d'un résumé, le même que celui des paragraphes du texte canonique.
const BLOCK_SEPARATOR: &str = "\n\n";

/// Plafond, en caractères, d'un résumé : le résumé rendu compte au plus
/// `min(max_chars, SUMMARY_CAP_CHARS)` caractères (05 §6.1).
pub const SUMMARY_CAP_CHARS: usize = 1_200;

/// Longueur minimale, en caractères, du début de texte rendu comme résumé quand ni sommaire
/// officiel ni dispositif n'existent.
pub const LEAD_MIN_CHARS: usize = 600;

/// D'où vient le résumé (`summary_source` de la sortie, 05 §5.0.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarySource {
    /// Sommaire officiel de la décision (`analyses`, puis `titrages`).
    Official,
    /// Dispositif repéré dans le texte, selon la règle de la juridiction.
    Dispositif,
    /// Début du texte, faute de mieux.
    Lead,
}

impl SummarySource {
    /// Valeur de `summary_source` sur le fil : `"official"`, `"dispositif"` ou `"lead"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Dispositif => "dispositif",
            Self::Lead => "lead",
        }
    }
}

/// Règle de repérage du dispositif, choisie par l'appelant selon la juridiction de la décision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositifRule {
    /// Décision du Conseil constitutionnel : entre « décide » et « juge par ».
    ConseilConstitutionnel,
    /// Décision du Conseil d'État : après les considérants, paragraphes décoratifs écartés.
    ConseilEtat,
    /// Pas de règle de dispositif (autres fonds).
    None,
}

/// Résumé d'un texte long.
///
/// Son `Debug` n'écrit jamais le texte (extrait d'un document), seulement la source, la longueur
/// en caractères et le drapeau `truncated`.
#[derive(Clone, PartialEq, Eq)]
pub struct Summary {
    /// Origine du résumé.
    pub source: SummarySource,
    /// Texte du résumé, d'au plus `min(max_chars, SUMMARY_CAP_CHARS)` caractères.
    pub text: String,
    /// Le texte d'origine a été coupé pour tenir dans la borne.
    pub truncated: bool,
}

impl fmt::Debug for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Summary {{ source: {:?}, chars: {}, truncated: {} }}",
            self.source,
            self.text.chars().count(),
            self.truncated
        )
    }
}

/// Marqueurs d'un dispositif, sur la vue repliée des paragraphes (casse, accents et blancs
/// ignorés, 03 §7.9).
struct Markers {
    /// Paragraphe d'en-tête, entier, avec ou sans `:` final. Exclu du résumé.
    header: &'static str,
    /// Début du paragraphe qui ferme le dispositif. Exclu du résumé.
    footer: &'static str,
}

/// Conseil constitutionnel : « LE CONSEIL CONSTITUTIONNEL DÉCIDE : » … « Jugé par ».
const CONSEIL_CONSTITUTIONNEL: Markers =
    Markers { header: "leconseilconstitutionneldecide", footer: "jugepar" };

/// Conseil d'État : « D E C I D E : » (ou « DECIDE : ») … « Délibéré ».
const CONSEIL_ETAT: Markers = Markers { header: "decide", footer: "delibere" };

/// Résumé de `text`, d'au plus `min(max_chars, SUMMARY_CAP_CHARS)` caractères.
///
/// La source est la première qui donne un texte : le sommaire officiel `official` (analyses,
/// sinon titrages), puis le dispositif que `rule` repère dans `text`, puis le début de `text`.
/// Le résultat est coupé au plafond en fin de phrase (de préférence) et `truncated` dit si une
/// coupure a eu lieu. Avec `max_chars == 0`, le texte est vide.
#[must_use]
pub fn summary(
    text: &LongText,
    official: Option<&OfficialSummary>,
    rule: DispositifRule,
    max_chars: usize,
) -> Summary {
    let (source, body) = official
        .and_then(official_body)
        .map(|body| (SummarySource::Official, body))
        .or_else(|| dispositif(text, rule).map(|body| (SummarySource::Dispositif, body)))
        .unwrap_or_else(|| (SummarySource::Lead, lead(text)));
    let (text, truncated) = cut(&body, max_chars.min(SUMMARY_CAP_CHARS));
    Summary { source, text, truncated }
}

/// Sommaire officiel : `analyses`, sinon `titrages`, entrées rognées et non vides, jointes par
/// une ligne vide. `None` quand ni l'une ni l'autre liste n'a d'entrée.
fn official_body(official: &OfficialSummary) -> Option<String> {
    [&official.analyses, &official.titrages]
        .into_iter()
        .map(|entries| {
            let kept: Vec<&str> = entries
                .iter()
                .map(|entry| entry.trim())
                .filter(|entry| !entry.is_empty())
                .collect();
            kept.join(BLOCK_SEPARATOR)
        })
        .find(|body| !body.is_empty())
}

/// Paragraphes de `text`, sans les paragraphes décoratifs.
fn content_paragraphs(text: &LongText) -> impl Iterator<Item = &str> {
    text.paragraphs().map(|(_, paragraph)| paragraph).filter(|paragraph| !is_decorative(paragraph))
}

/// Vrai pour un paragraphe fait seulement de `-`, `_`, `=`, `*` et de blancs.
fn is_decorative(paragraph: &str) -> bool {
    paragraph.chars().all(|c| c.is_whitespace() || matches!(c, '-' | '_' | '=' | '*'))
}

/// Vue repliée d'un paragraphe pour les marqueurs : minuscules, accents repliés, blancs retirés
/// (« D E C I D E : » donne `decide:`).
fn marker_key(paragraph: &str) -> String {
    fold(paragraph).chars().filter(|c| !c.is_whitespace()).collect()
}

/// Dispositif de `text` selon `rule` : les paragraphes entre l'en-tête et le paragraphe de
/// clôture, les deux exclus, joints par une ligne vide. `None` si la règle est `None`, si l'un
/// des marqueurs manque (la clôture ne compte qu'après l'en-tête) ou si le dispositif est vide.
fn dispositif(text: &LongText, rule: DispositifRule) -> Option<String> {
    let markers = match rule {
        DispositifRule::ConseilConstitutionnel => CONSEIL_CONSTITUTIONNEL,
        DispositifRule::ConseilEtat => CONSEIL_ETAT,
        DispositifRule::None => return None,
    };
    let mut body = Vec::new();
    let mut inside = false;
    for paragraph in content_paragraphs(text) {
        let key = marker_key(paragraph);
        if !inside {
            inside = key.strip_suffix(':').unwrap_or(&key) == markers.header;
        } else if key.starts_with(markers.footer) {
            return (!body.is_empty()).then(|| body.join(BLOCK_SEPARATOR));
        } else {
            body.push(paragraph);
        }
    }
    None
}

/// Début de `text` : les premiers paragraphes, jusqu'à ce que le total atteigne au moins
/// [`LEAD_MIN_CHARS`] caractères (séparateurs compris).
fn lead(text: &LongText) -> String {
    let mut lead = String::new();
    for paragraph in content_paragraphs(text) {
        if !lead.is_empty() {
            lead.push_str(BLOCK_SEPARATOR);
        }
        lead.push_str(paragraph);
        if lead.chars().count() >= LEAD_MIN_CHARS {
            break;
        }
    }
    lead
}

/// `body` coupé à `cap` caractères au plus, en fin de phrase de préférence (la coupe douce de
/// [`window`], 03 §7.6), sans blanc final ; le drapeau dit si `body` a été raccourci.
fn cut(body: &str, cap: usize) -> (String, bool) {
    if cap == 0 {
        return (String::new(), !body.is_empty());
    }
    let first = window(body, 0, cap);
    (first.text.trim_end().to_owned(), first.next_offset.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    const CONSTIT: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/reference/consult_juri_constit.json"
    );
    const CETAT: &str =
        include_str!("../../../lf-adapter-piste/tests/fixtures/reference/consult_juri_cetat.json");

    /// Texte canonique d'une fixture réelle (`text.texteHtml`).
    fn fixture(json: &str) -> LongText {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        LongText::from_html(value["text"]["texteHtml"].as_str().unwrap()).unwrap()
    }

    /// Texte dont chaque élément de `paragraphs` est un paragraphe.
    fn long(paragraphs: &[&str]) -> LongText {
        LongText::from_plain(&paragraphs.join("\n")).unwrap()
    }

    fn official(analyses: &[&str], titrages: &[&str]) -> OfficialSummary {
        OfficialSummary {
            analyses: analyses.iter().map(|s| (*s).to_owned()).collect(),
            titrages: titrages.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// Chaîne de `n` caractères sans blanc ni fin de phrase.
    fn filler(n: usize) -> String {
        "x".repeat(n)
    }

    /// `n` phrases de moins de 32 caractères chacune, séparées par une espace.
    fn sentences(n: usize) -> String {
        (0..n).map(|i| format!("Phrase numéro {i} du jugement.")).collect::<Vec<_>>().join(" ")
    }

    fn chars(s: &str) -> usize {
        s.chars().count()
    }

    /// Source, texte et drapeau, pour comparer sans passer par le `Debug` du résumé.
    fn parts(s: &Summary) -> (SummarySource, &str, bool) {
        (s.source, s.text.as_str(), s.truncated)
    }

    #[test]
    fn source_names_are_the_wire_values() {
        assert_eq!(SummarySource::Official.as_str(), "official");
        assert_eq!(SummarySource::Dispositif.as_str(), "dispositif");
        assert_eq!(SummarySource::Lead.as_str(), "lead");
    }

    #[test]
    fn caps_are_the_documented_values() {
        assert_eq!((SUMMARY_CAP_CHARS, LEAD_MIN_CHARS), (1_200, 600));
    }

    #[test]
    fn debug_never_writes_the_text() {
        let summary = Summary {
            source: SummarySource::Official,
            text: "contenu confidentiel".to_owned(),
            truncated: true,
        };
        let shown = format!("{summary:?}");
        assert_eq!(shown, "Summary { source: Official, chars: 20, truncated: true }");
        assert!(!shown.contains("confidentiel"));
    }

    #[test]
    fn official_analyses_first_then_titrages() {
        let body = long(&["Premier paragraphe.", "Second paragraphe."]);
        let rule = DispositifRule::ConseilEtat;

        // Les analyses l'emportent sur les titrages, jointes par une ligne vide.
        let both = official(&["Analyse A.", "Analyse B."], &["Titrage"]);
        assert_eq!(
            parts(&summary(&body, Some(&both), rule, 4_000)),
            (SummarySource::Official, "Analyse A.\n\nAnalyse B.", false)
        );

        // Sans analyse, les titrages.
        let titrages = official(&[], &["Titrage 1", "Titrage 2"]);
        assert_eq!(
            parts(&summary(&body, Some(&titrages), rule, 4_000)),
            (SummarySource::Official, "Titrage 1\n\nTitrage 2", false)
        );

        // Des analyses blanches ne comptent pas : les titrages prennent le relais, rognés.
        let blank = official(&["", "   "], &["  Titrage  "]);
        assert_eq!(
            parts(&summary(&body, Some(&blank), rule, 4_000)),
            (SummarySource::Official, "Titrage", false)
        );
        let padded = official(&["  Analyse  ", "", "Suite."], &[]);
        assert_eq!(
            parts(&summary(&body, Some(&padded), rule, 4_000)),
            (SummarySource::Official, "Analyse\n\nSuite.", false)
        );
    }

    #[test]
    fn official_without_content_is_not_a_source() {
        let body = long(&["Premier paragraphe.", "Second paragraphe."]);
        let empty = OfficialSummary::default();
        let blank = official(&[" "], &["\t"]);
        for given in [None, Some(&empty), Some(&blank)] {
            assert_eq!(
                parts(&summary(&body, given, DispositifRule::None, 4_000)),
                (SummarySource::Lead, "Premier paragraphe.\n\nSecond paragraphe.", false)
            );
        }
    }

    #[test]
    fn official_wins_over_the_dispositif() {
        let decision = long(&[
            "Visa.",
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            "Article 1er. - Rejet.",
            "Jugé par le Conseil constitutionnel.",
        ]);
        let given = official(&["Sommaire officiel."], &[]);
        let got = summary(&decision, Some(&given), DispositifRule::ConseilConstitutionnel, 4_000);
        assert_eq!(parts(&got), (SummarySource::Official, "Sommaire officiel.", false));
    }

    #[test]
    fn official_is_cut_at_the_cap_like_any_other_source() {
        let body = long(&["Texte."]);
        let long_analysis = sentences(100);
        let given = official(&[&long_analysis], &[]);
        let got = summary(&body, Some(&given), DispositifRule::None, 5_000);
        assert_eq!(got.source, SummarySource::Official);
        assert!(got.truncated);
        assert!(chars(&got.text) <= SUMMARY_CAP_CHARS);
        assert!(long_analysis.starts_with(&got.text));
    }

    #[test]
    fn dispositif_constit_between_decide_and_juge_par() {
        let rule = DispositifRule::ConseilConstitutionnel;

        // Fixture réelle : deux paragraphes, 551 caractères, en-tête et « Jugé par » exclus.
        let decision = fixture(CONSTIT);
        let got = summary(&decision, None, rule, 4_000);
        let articles: Vec<&str> = decision.paragraphs().map(|(_, p)| p).skip(45).take(2).collect();
        assert_eq!(got.source, SummarySource::Dispositif);
        assert_eq!(got.text, articles.join("\n\n"));
        assert_eq!(chars(&got.text), 551);
        assert!(got.text.starts_with("Article 1er. - Le renvoi opéré"));
        assert!(!got.truncated);
        assert!(!got.text.contains("DÉCIDE") && !got.text.contains("Jugé par"));

        // Texte synthétique : seul ce qui est entre les deux marqueurs est gardé.
        let synthetic = long(&[
            "Visa.",
            "Le Conseil constitutionnel décide :",
            "Article 1er. - A.",
            "Article 2. - Il est jugé par le Conseil que B.",
            "Jugé par le Conseil constitutionnel dans sa séance du 6 novembre 2025.",
            "Rendu public.",
        ]);
        let got = summary(&synthetic, None, rule, 4_000);
        assert_eq!(
            parts(&got),
            (
                SummarySource::Dispositif,
                "Article 1er. - A.\n\nArticle 2. - Il est jugé par le Conseil que B.",
                false
            )
        );
    }

    #[test]
    fn dispositif_constit_markers_ignore_case_accents_and_colon_spacing() {
        for header in [
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            "LE CONSEIL CONSTITUTIONNEL DECIDE:",
            "Le Conseil constitutionnel décide",
        ] {
            for footer in ["Jugé par le Conseil", "JUGE PAR LE CONSEIL", "jugé par"] {
                let decision = long(&["Visa.", header, "Article 1er. - A.", footer]);
                let got = summary(&decision, None, DispositifRule::ConseilConstitutionnel, 4_000);
                assert_eq!(
                    parts(&got),
                    (SummarySource::Dispositif, "Article 1er. - A.", false),
                    "{header} / {footer}"
                );
            }
        }
    }

    #[test]
    fn dispositif_constit_needs_both_markers_and_a_body() {
        let rule = DispositifRule::ConseilConstitutionnel;
        let header = "LE CONSEIL CONSTITUTIONNEL DÉCIDE :";
        let footer = "Jugé par le Conseil constitutionnel.";
        let cases: [&[&str]; 7] = [
            // en-tête introuvable
            &["Visa.", "Article 1er. - A.", footer],
            // « Jugé par » introuvable
            &["Visa.", header, "Article 1er. - A."],
            // dispositif vide
            &["Visa.", header, footer],
            // « Jugé par » avant l'en-tête : il ne ferme rien
            &[footer, header, "Article 1er. - A."],
            // une phrase qui contient « décide » n'est pas l'en-tête
            &["Visa.", "Le Conseil constitutionnel décide que la loi est conforme.", footer],
            // un « Jugé par » au milieu d'un paragraphe n'est pas le marqueur
            &["Visa.", header, "Article 1er. - Jugé par erreur."],
            // le marqueur de l'autre juridiction ne compte pas
            &["Visa.", "D E C I D E :", "Article 1er : A.", "Délibéré à l'issue de la séance."],
        ];
        for paragraphs in cases {
            let decision = long(paragraphs);
            let got = summary(&decision, None, rule, 4_000);
            assert_eq!(got.source, SummarySource::Lead, "{paragraphs:?}");
            assert_eq!(got.text, decision.as_str(), "{paragraphs:?}");
        }
    }

    #[test]
    fn dispositif_cetat_skips_decorative_paragraphs() {
        let rule = DispositifRule::ConseilEtat;

        // Fixture réelle : trois articles, 411 caractères, la ligne de tirets écartée.
        let decision = fixture(CETAT);
        let got = summary(&decision, None, rule, 4_000);
        let articles: Vec<&str> = decision.paragraphs().map(|(_, p)| p).skip(32).take(3).collect();
        assert_eq!(got.source, SummarySource::Dispositif);
        assert_eq!(got.text, articles.join("\n\n"));
        assert_eq!(chars(&got.text), 411);
        assert!(got.text.starts_with("Article 1er : Le pourvoi"));
        assert!(!got.truncated);
        assert!(!got.text.contains("---") && !got.text.contains("Délibéré"));

        // Synthétique : tirets, soulignés, égalités, étoiles et blancs sont ignorés, partout.
        let synthetic = long(&[
            "Visa.",
            "D E C I D E :",
            "--------------",
            "Article 1er : A.",
            "_ _ _",
            "Article 2 : B.",
            "==**",
            "- * -",
            "Délibéré à l'issue de la séance du 6 mai 2026.",
            "Rendu le 29 mai 2026.",
        ]);
        let got = summary(&synthetic, None, rule, 4_000);
        assert_eq!(
            parts(&got),
            (SummarySource::Dispositif, "Article 1er : A.\n\nArticle 2 : B.", false)
        );
    }

    #[test]
    fn dispositif_cetat_header_variants() {
        for header in ["D E C I D E :", "DECIDE :", "DÉCIDE :", "D E C I D E", "Décide:"] {
            let decision = long(&["Visa.", header, "Article 1er : A.", "Délibéré à l'issue."]);
            let got = summary(&decision, None, DispositifRule::ConseilEtat, 4_000);
            assert_eq!(
                parts(&got),
                (SummarySource::Dispositif, "Article 1er : A.", false),
                "{header}"
            );
        }
    }

    #[test]
    fn dispositif_cetat_needs_both_markers_and_a_body() {
        let rule = DispositifRule::ConseilEtat;
        let footer = "Délibéré à l'issue de la séance.";
        let cases: [&[&str]; 7] = [
            // en-tête introuvable
            &["Visa.", "Article 1er : A.", footer],
            // « Délibéré » introuvable
            &["Visa.", "D E C I D E :", "Article 1er : A."],
            // dispositif vide, ou réduit à des séparateurs
            &["Visa.", "D E C I D E :", footer],
            &["Visa.", "D E C I D E :", "-----", "=====", footer],
            // « Délibéré » avant l'en-tête : il ne ferme rien
            &[footer, "D E C I D E :", "Article 1er : A."],
            // une phrase qui contient « décide » n'est pas l'en-tête
            &["Le juge décide que le pourvoi est rejeté.", "Article 1er : A.", footer],
            // le marqueur de l'autre juridiction ne compte pas
            &[
                "Visa.",
                "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
                "Article 1er. - A.",
                "Jugé par le Conseil.",
            ],
        ];
        for paragraphs in cases {
            let decision = long(paragraphs);
            let got = summary(&decision, None, rule, 4_000);
            assert_eq!(got.source, SummarySource::Lead, "{paragraphs:?}");
        }
    }

    #[test]
    fn dispositif_is_cut_at_the_cap() {
        let paragraph = sentences(100);
        let decision = long(&[
            "Visa.",
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            &paragraph,
            "Jugé par le Conseil constitutionnel.",
        ]);
        let got = summary(&decision, None, DispositifRule::ConseilConstitutionnel, 5_000);
        assert_eq!(got.source, SummarySource::Dispositif);
        assert!(got.truncated);
        assert!(chars(&got.text) <= SUMMARY_CAP_CHARS);
        assert!(got.text.ends_with("du jugement."));
        assert!(paragraph.starts_with(&got.text));
    }

    #[test]
    fn no_dispositif_falls_back_to_lead() {
        let decision =
            long(&["Vu la requête.", "Considérant ce qui suit :", "1. Le pourvoi est rejeté."]);
        let all = "Vu la requête.\n\nConsidérant ce qui suit :\n\n1. Le pourvoi est rejeté.";
        for rule in [
            DispositifRule::ConseilConstitutionnel,
            DispositifRule::ConseilEtat,
            DispositifRule::None,
        ] {
            let got = summary(&decision, None, rule, 4_000);
            assert_eq!(parts(&got), (SummarySource::Lead, all, false), "{rule:?}");
        }

        // Même avec un dispositif bien formé, la règle `None` ne le cherche pas.
        let with_markers = long(&["Visa.", "D E C I D E :", "Article 1er : A.", "Délibéré."]);
        let got = summary(&with_markers, None, DispositifRule::None, 4_000);
        assert_eq!(got.source, SummarySource::Lead);

        // Texte vide : rien à résumer, ni coupure.
        let got = summary(&LongText::default(), None, DispositifRule::ConseilEtat, 4_000);
        assert_eq!(parts(&got), (SummarySource::Lead, "", false));
    }

    #[test]
    fn lead_reaches_600_chars() {
        let rule = DispositifRule::None;
        let (a, b, c) = (filler(299), filler(299), filler(299));

        // 299 + 2 + 299 = 600 : le seuil est atteint, le troisième paragraphe reste dehors.
        let got = summary(&long(&[&a, &b, &c]), None, rule, 4_000);
        assert_eq!(parts(&got), (SummarySource::Lead, format!("{a}\n\n{b}").as_str(), false));
        assert_eq!(chars(&got.text), LEAD_MIN_CHARS);

        // 299 + 2 + 298 = 599 : un caractère trop court, le paragraphe suivant s'ajoute.
        let short_b = filler(298);
        let got = summary(&long(&[&a, &short_b, &c]), None, rule, 4_000);
        assert_eq!(got.text, format!("{a}\n\n{short_b}\n\n{c}"));
        assert_eq!(chars(&got.text), 599 + 2 + 299);

        // Un seul paragraphe de 600 caractères suffit ; de 599, il en faut un autre.
        let (p600, p599, tail) = (filler(600), filler(599), filler(10));
        let got = summary(&long(&[&p600, &tail]), None, rule, 4_000);
        assert_eq!(got.text, p600);
        let got = summary(&long(&[&p599, &tail]), None, rule, 4_000);
        assert_eq!(got.text, format!("{p599}\n\n{tail}"));

        // Un paragraphe énorme est rendu coupé au plafond, pas au seuil.
        let huge = sentences(100);
        let got = summary(&long(&[&huge]), None, rule, 4_000);
        assert!(chars(&got.text) > LEAD_MIN_CHARS);
        assert!(got.truncated);
    }

    #[test]
    fn lead_short_text_is_whole_and_skips_decorative_paragraphs() {
        let decision = long(&["-----------", "Vu la requête.", "_ _", "Le pourvoi est rejeté."]);
        let got = summary(&decision, None, DispositifRule::None, 4_000);
        assert_eq!(
            parts(&got),
            (SummarySource::Lead, "Vu la requête.\n\nLe pourvoi est rejeté.", false)
        );

        // Un paragraphe qui commence par un tiret mais porte du texte n'est pas décoratif.
        let visas = long(&["- la Constitution ;", "- le code de procédure pénale ;"]);
        let got = summary(&visas, None, DispositifRule::None, 4_000);
        assert_eq!(got.text, "- la Constitution ;\n\n- le code de procédure pénale ;");

        // Un texte qui n'est que décor n'a pas de résumé.
        let got = summary(&long(&["-----", "=="]), None, DispositifRule::None, 4_000);
        assert_eq!(parts(&got), (SummarySource::Lead, "", false));
    }

    #[test]
    fn summary_capped_at_min_max_chars_1200_cut_at_sentence() {
        let paragraph = sentences(120);
        let body = long(&[&paragraph]);
        for (max_chars, cap) in
            [(50_000, 1_200), (1_201, 1_200), (1_200, 1_200), (800, 800), (500, 500)]
        {
            let got = summary(&body, None, DispositifRule::None, max_chars);
            let n = chars(&got.text);
            assert_eq!(got.source, SummarySource::Lead);
            assert!(got.truncated, "{max_chars}");
            assert!(n <= cap, "{max_chars}: {n} > {cap}");
            // La coupe tombe sur la dernière fin de phrase qui tient : au plus une phrase de moins.
            assert!(n + 33 > cap, "{max_chars}: {n} trop loin de {cap}");
            assert!(got.text.ends_with("du jugement."), "{max_chars}");
            assert!(paragraph.starts_with(&got.text), "{max_chars}");
            assert!(paragraph[got.text.len()..].starts_with(' '), "{max_chars}");
        }
    }

    #[test]
    fn summary_cap_boundary_is_not_a_cut() {
        let body = long(&["Texte."]);
        let exact = filler(SUMMARY_CAP_CHARS);
        let got = summary(&body, Some(&official(&[&exact], &[])), DispositifRule::None, 50_000);
        assert_eq!(parts(&got), (SummarySource::Official, exact.as_str(), false));

        let over = filler(SUMMARY_CAP_CHARS + 1);
        let got = summary(&body, Some(&official(&[&over], &[])), DispositifRule::None, 50_000);
        assert_eq!(got.source, SummarySource::Official);
        assert!(got.truncated);
        assert_eq!(chars(&got.text), SUMMARY_CAP_CHARS);

        // Même frontière avec une borne plus basse que le plafond.
        let at_budget = filler(700);
        let got = summary(&body, Some(&official(&[&at_budget], &[])), DispositifRule::None, 700);
        assert_eq!(parts(&got), (SummarySource::Official, at_budget.as_str(), false));
        let got = summary(&body, Some(&official(&[&at_budget], &[])), DispositifRule::None, 699);
        assert_eq!((chars(&got.text), got.truncated), (699, true));
    }

    #[test]
    fn zero_budget_gives_an_empty_summary() {
        let got = summary(&long(&["Du texte."]), None, DispositifRule::None, 0);
        assert_eq!(parts(&got), (SummarySource::Lead, "", true));
        let got = summary(&LongText::default(), None, DispositifRule::None, 0);
        assert_eq!(parts(&got), (SummarySource::Lead, "", false));
    }

    #[test]
    fn summary_of_real_decisions_fits_a_small_budget() {
        for (json, rule) in [
            (CONSTIT, DispositifRule::ConseilConstitutionnel),
            (CETAT, DispositifRule::ConseilEtat),
        ] {
            // Les dispositifs font 551 et 411 caractères : tous deux dépassent 300.
            let got = summary(&fixture(json), None, rule, 300);
            assert_eq!(got.source, SummarySource::Dispositif);
            assert!(got.truncated);
            assert!(chars(&got.text) <= 300);
            assert!(got.text.starts_with("Article 1er"));
        }
    }

    proptest! {
        #[test]
        fn summary_never_exceeds_its_budget(
            text in "[a-zé .!?\n]{0,2500}",
            max_chars in 0usize..2_000,
        ) {
            let body = LongText::from_plain(&text).unwrap();
            let got = summary(&body, None, DispositifRule::None, max_chars);
            prop_assert!(chars(&got.text) <= max_chars.min(SUMMARY_CAP_CHARS));
            prop_assert_eq!(got.text.trim(), got.text.as_str());
            prop_assert_eq!(got.source, SummarySource::Lead);
        }
    }
}
