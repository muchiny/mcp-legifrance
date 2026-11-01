//! Résumé d'un texte long (`detail="summary"`, 03 §7.9) : sommaire officiel, sinon dispositif,
//! sinon début du texte.
//!
//! Ordre de préférence (00 §4) :
//!
//! 1. le sommaire officiel de la décision : `analyses`, sinon `titrages`, joints par une ligne
//!    vide, les entrées de moins de 20 caractères alphanumériques
//!    (« X ») étant ignorées ;
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

/// Nombre minimal de caractères alphanumériques (`char::is_alphanumeric`) d'une entrée du sommaire
/// officiel pour qu'elle serve de résumé : en dessous, la donnée amont est dégénérée (« X » pour
/// une ordonnance du TJ de Paris) et le résumé retombe sur le dispositif, puis sur le début du
/// texte (D-07, 03 §7.9).
const OFFICIAL_MIN_ALNUM_CHARS: usize = 20;

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
    /// Paragraphes d'en-tête acceptés, entiers, avec ou sans `:` final. Exclu du résumé.
    headers: &'static [&'static str],
    /// Débuts de paragraphe qui ferment le dispositif. Exclu du résumé.
    footers: &'static [&'static str],
    /// Sans clôture, le dispositif court jusqu'à la fin du texte (il faut un en-tête et un corps
    /// non vide).
    open_ended: bool,
}

/// Conseil constitutionnel : « LE CONSEIL CONSTITUTIONNEL DÉCIDE : » ou « DÉCIDE : » (décisions
/// ORGA) … « Jugé par », « Délibéré par » ou « Rendu public » (décisions PDR, sans « Jugé par »).
const CONSEIL_CONSTITUTIONNEL: Markers = Markers {
    headers: &["leconseilconstitutionneldecide", "decide"],
    footers: &["jugepar", "delibere", "rendupublic"],
    open_ended: true,
};

/// Conseil d'État : « D E C I D E : » (ou « DECIDE : ») … « Délibéré ».
const CONSEIL_ETAT: Markers =
    Markers { headers: &["decide"], footers: &["delibere"], open_ended: false };

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

/// Sommaire officiel : `analyses`, sinon `titrages`, entrées rognées et d'au moins
/// [`OFFICIAL_MIN_ALNUM_CHARS`] caractères alphanumériques, jointes par une ligne vide. `None`
/// quand ni l'une ni l'autre liste n'a d'entrée assez longue : le résumé retombe alors sur le
/// dispositif, puis sur le début du texte.
fn official_body(official: &OfficialSummary) -> Option<String> {
    [&official.analyses, &official.titrages]
        .into_iter()
        .map(|entries| {
            let kept: Vec<&str> = entries
                .iter()
                .map(|entry| entry.trim())
                .filter(|entry| {
                    entry.chars().filter(|c| c.is_alphanumeric()).count()
                        >= OFFICIAL_MIN_ALNUM_CHARS
                })
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
/// clôture, les deux exclus, joints par une ligne vide. `None` si la règle est `None`, si
/// l'en-tête manque, si la clôture manque (la clôture ne compte qu'après l'en-tête) sauf pour une
/// règle à dispositif ouvert (fin du texte), ou si le dispositif est vide.
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
            let header = key.strip_suffix(':').unwrap_or(&key);
            inside = markers.headers.contains(&header);
        } else if markers.footers.iter().any(|footer| key.starts_with(footer)) {
            return (!body.is_empty()).then(|| body.join(BLOCK_SEPARATOR));
        } else {
            body.push(paragraph);
        }
    }
    (markers.open_ended && !body.is_empty()).then(|| body.join(BLOCK_SEPARATOR))
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
    const CASSATION: &str =
        include_str!("../../../lf-adapter-piste/tests/fixtures/reference/consult_juri.json");
    const CETAT_OLD: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/F-juri-consult-CETATEXT000007742504.json"
    );
    const CETAT_NEW: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/F-juri-consult-CETATEXT000052137642.json"
    );
    const C4_CIV1: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-chambre-civ1-JURITEXT000038322046.json"
    );
    const C4_CIV3: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-chambre-civ3-JURITEXT000032056669.json"
    );
    const C4_COM: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-chambre-com-JURITEXT000054218381.json"
    );
    const C4_CRIM: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-chambre-crim-JURITEXT000021854686.json"
    );
    const C4_SOCIALE: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-chambre-formation-CHAMBRE_SOCIALE-JURITEXT000054929168.json"
    );
    const C4_I7: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-chambre-formation-I7-JURITEXT000045653019.json"
    );
    const C4_89: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-89-15.231-JURITEXT000007026394.json"
    );
    const C4_05: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-05-13.255-JURITEXT000007052928.json"
    );
    const C4_14: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-14-24.444-JURITEXT000031539712.json"
    );
    const C4_06: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-06-12.769-JURITEXT000017739957.json"
    );
    const C16_LIENS: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C16-juri-consult-liens-JURITEXT000038427064.json"
    );
    /// 2022-196 PDR : « DÉCIDE » puis « Délibéré par… / Rendu public… », sans « Jugé par ».
    const PDR: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C14-juri-consult-CC-2022-196-PDR-CONSTEXT000045802100.json"
    );
    /// 2021-150 ORGA : « DÉCIDE : » seul comme en-tête.
    const ORGA: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C14-juri-consult-CC-2021-150-ORGA-CONSTEXT000044239153.json"
    );

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
        let both = official(
            &["Analyse A du sommaire officiel.", "Analyse B du sommaire officiel."],
            &["Titrage"],
        );
        assert_eq!(
            parts(&summary(&body, Some(&both), rule, 4_000)),
            (
                SummarySource::Official,
                "Analyse A du sommaire officiel.\n\nAnalyse B du sommaire officiel.",
                false
            )
        );

        // Sans analyse, les titrages.
        let titrages =
            official(&[], &["Titrage numéro 1 du sommaire", "Titrage numéro 2 du sommaire"]);
        assert_eq!(
            parts(&summary(&body, Some(&titrages), rule, 4_000)),
            (
                SummarySource::Official,
                "Titrage numéro 1 du sommaire\n\nTitrage numéro 2 du sommaire",
                false
            )
        );

        // Des analyses blanches ne comptent pas : les titrages prennent le relais, rognés.
        let blank = official(&["", "   "], &["  Titrage du sommaire officiel  "]);
        assert_eq!(
            parts(&summary(&body, Some(&blank), rule, 4_000)),
            (SummarySource::Official, "Titrage du sommaire officiel", false)
        );
        let padded = official(
            &["  Analyse du sommaire officiel  ", "", "Suite de l'analyse officielle."],
            &[],
        );
        assert_eq!(
            parts(&summary(&body, Some(&padded), rule, 4_000)),
            (
                SummarySource::Official,
                "Analyse du sommaire officiel\n\nSuite de l'analyse officielle.",
                false
            )
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

    /// TJ Paris, ordonnance du JME du 5 juillet 2022 (JURITEXT000046652043) : la donnée amont est
    /// `sommaire[].abstrats = "X"` (D-07, défaut decisions-1). Capture réelle.
    const TJ_PARIS_X: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/juri/c101/C4-juri-consult-chambre-formation-CT0196-JURITEXT000046652043.json"
    );

    /// Sommaire officiel tel que le mapper l'écrit pour une fixture réelle : `abstrats` en
    /// analyses puis `resumePrincipal` et `autreResume` en titrages pour la Cour de cassation, les
    /// deux inversés pour le Conseil d'État (`map::decision::summary_of`).
    fn recorded_official(json: &str, cetat: bool) -> OfficialSummary {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let entries = value["text"]["sommaire"].as_array().cloned().unwrap_or_default();
        let pick = |keys: &[&str]| -> Vec<String> {
            entries
                .iter()
                .flat_map(|entry| keys.iter().filter_map(|key| entry[*key].as_str()))
                .map(str::to_owned)
                .collect()
        };
        let (abstrats, resumes) = (pick(&["abstrats"]), pick(&["resumePrincipal", "autreResume"]));
        if cetat {
            OfficialSummary::new(resumes, abstrats)
        } else {
            OfficialSummary::new(abstrats, resumes)
        }
    }

    /// Nombre de caractères alphanumériques de `s`, comme la règle du seuil.
    fn alnum(s: &str) -> usize {
        s.chars().filter(|c| c.is_alphanumeric()).count()
    }

    #[test]
    fn a_trivial_official_summary_falls_back_to_the_lead() {
        // Cas réel decisions-1 : « X » ne doit plus passer pour un sommaire officiel.
        let value: serde_json::Value = serde_json::from_str(TJ_PARIS_X).unwrap();
        assert_eq!(value["text"]["sommaire"][0]["abstrats"], "X");
        let decision = fixture(TJ_PARIS_X);
        let given = recorded_official(TJ_PARIS_X, false);
        assert_eq!(given.analyses, ["X"]);
        let got = summary(&decision, Some(&given), DispositifRule::None, 4_000);
        assert_eq!(got.source, SummarySource::Lead);
        assert!(got.text.starts_with("TRIBUNAL\n\nJUDICIAIRE\n\nDE PARIS"), "{}", got.text);
        assert!(chars(&got.text) >= LEAD_MIN_CHARS);

        // Synthétique : avec une règle de dispositif, c'est le dispositif qui prend le relais.
        let decision = long(&["Visa.", "D E C I D E :", "Article 1er : A.", "Délibéré à Paris."]);
        let trivial = official(&["X"], &[]);
        let got = summary(&decision, Some(&trivial), DispositifRule::ConseilEtat, 4_000);
        assert_eq!(parts(&got), (SummarySource::Dispositif, "Article 1er : A.", false));

        // Jumeau : un sommaire officiel d'au moins 20 caractères alphanumériques est conservé.
        let real = official(&["Le sommaire est assez long."], &[]);
        let got = summary(&decision, Some(&real), DispositifRule::ConseilEtat, 4_000);
        assert_eq!(parts(&got), (SummarySource::Official, "Le sommaire est assez long.", false));
    }

    #[test]
    fn the_official_threshold_is_20_alphanumeric_characters() {
        let body = long(&["Texte de repli, assez long pour servir de début."]);
        let source = |entry: &str| {
            summary(&body, Some(&official(&[entry], &[])), DispositifRule::None, 4_000).source
        };
        assert_eq!(OFFICIAL_MIN_ALNUM_CHARS, 20);

        // Bornes 19 et 20, en analyses comme en titrages.
        assert_eq!(source(&filler(19)), SummarySource::Lead);
        assert_eq!(source(&filler(20)), SummarySource::Official);
        let titrage = |entry: &str| {
            summary(&body, Some(&official(&[], &[entry])), DispositifRule::None, 4_000).source
        };
        assert_eq!(titrage(&filler(19)), SummarySource::Lead);
        assert_eq!(titrage(&filler(20)), SummarySource::Official);

        // Seuls les caractères alphanumériques comptent : blancs, ponctuation, tirets et points
        // de conduite n'en font pas partie, les lettres accentuées et les chiffres si.
        let padded = format!("{} . - ... ;\t, !?", filler(19));
        assert_eq!(source(&padded), SummarySource::Lead);
        assert_eq!(source(&format!("{padded} z")), SummarySource::Official);
        assert_eq!(source(&"é".repeat(20)), SummarySource::Official);
        assert_eq!(source(&"é".repeat(19)), SummarySource::Lead);
        assert_eq!(source(&"7".repeat(20)), SummarySource::Official);
        assert_eq!(source(&format!("{}c", "a b ".repeat(9))), SummarySource::Lead);
        assert_eq!(source(&"a b ".repeat(10)), SummarySource::Official);
        assert_eq!(source(&"-".repeat(300)), SummarySource::Lead);
    }

    #[test]
    fn trivial_entries_are_dropped_one_by_one() {
        let body = long(&["Texte de repli."]);
        let given = official(&["X", "Une analyse suffisamment longue.", "12"], &["Titrage"]);
        let got = summary(&body, Some(&given), DispositifRule::None, 4_000);
        assert_eq!(
            parts(&got),
            (SummarySource::Official, "Une analyse suffisamment longue.", false)
        );

        // Des analyses toutes triviales cèdent la place aux titrages, eux-mêmes filtrés.
        let given = official(&["X", "Y"], &["Z", "Un titrage suffisamment long."]);
        let got = summary(&body, Some(&given), DispositifRule::None, 4_000);
        assert_eq!(parts(&got), (SummarySource::Official, "Un titrage suffisamment long.", false));

        // Jumeau : deux entrées assez longues sont jointes comme avant.
        let given =
            official(&["Première analyse assez longue.", "Seconde analyse assez longue."], &[]);
        let got = summary(&body, Some(&given), DispositifRule::None, 4_000);
        assert_eq!(
            parts(&got),
            (
                SummarySource::Official,
                "Première analyse assez longue.\n\nSeconde analyse assez longue.",
                false
            )
        );

        // Tout trivial : repli.
        let given = official(&["X"], &["Y"]);
        let got = summary(&body, Some(&given), DispositifRule::None, 4_000);
        assert_eq!(parts(&got), (SummarySource::Lead, "Texte de repli.", false));
    }

    #[test]
    fn recorded_official_summaries_all_clear_the_threshold() {
        // Sommaires officiels réels (Cour de cassation et Conseil d'État) : après la
        // normalisation du mapper, aucune entrée utile ne tombe sous le seuil. Le Conseil
        // constitutionnel n'a pas de sommaire officiel (le mapper n'en lit pas). Seule exception
        // enregistrée : le « X » du TJ Paris, que la règle vise.
        let recorded: [(&str, bool); 15] = [
            (CASSATION, false),
            (CETAT, true),
            (CETAT_OLD, true),
            (CETAT_NEW, true),
            (C4_CIV1, false),
            (C4_CIV3, false),
            (C4_COM, false),
            (C4_CRIM, false),
            (C4_SOCIALE, false),
            (C4_I7, false),
            (C4_89, false),
            (C4_05, false),
            (C4_14, false),
            (C4_06, false),
            (C16_LIENS, false),
        ];
        let mut shortest = usize::MAX;
        let mut entries = 0;
        for (json, cetat) in recorded {
            let given = recorded_official(json, cetat);
            for entry in given.analyses.iter().chain(&given.titrages) {
                entries += 1;
                shortest = shortest.min(alnum(entry));
            }
        }
        assert!(entries >= 30, "{entries}");
        assert!(shortest >= OFFICIAL_MIN_ALNUM_CHARS, "{shortest}");
        assert_eq!(shortest, 83, "plus court sommaire officiel réel conservé");
    }

    #[test]
    fn official_wins_over_the_dispositif() {
        let decision = long(&[
            "Visa.",
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            "Article 1er. - Rejet.",
            "Jugé par le Conseil constitutionnel.",
        ]);
        let given = official(&["Sommaire officiel de la décision."], &[]);
        let got = summary(&decision, Some(&given), DispositifRule::ConseilConstitutionnel, 4_000);
        assert_eq!(
            parts(&got),
            (SummarySource::Official, "Sommaire officiel de la décision.", false)
        );
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
    fn dispositif_constit_needs_a_header_and_a_body() {
        let rule = DispositifRule::ConseilConstitutionnel;
        let header = "LE CONSEIL CONSTITUTIONNEL DÉCIDE :";
        let footer = "Jugé par le Conseil constitutionnel.";
        let cases: [&[&str]; 6] = [
            // en-tête introuvable
            &["Visa.", "Article 1er. - A.", footer],
            // dispositif vide, avec ou sans clôture
            &["Visa.", header, footer],
            &["Visa.", header],
            &["Visa.", header, "-----", "====="],
            // une phrase qui contient « décide » n'est pas l'en-tête
            &["Visa.", "Le Conseil constitutionnel décide que la loi est conforme.", footer],
            &["Visa.", "Le Conseil constitutionnel décide que la loi est conforme."],
        ];
        for paragraphs in cases {
            let decision = long(paragraphs);
            let got = summary(&decision, None, rule, 4_000);
            assert_eq!(got.source, SummarySource::Lead, "{paragraphs:?}");
        }
    }

    #[test]
    fn dispositif_constit_pdr_without_jugé_par() {
        let rule = DispositifRule::ConseilConstitutionnel;

        // 2022-196 PDR (1806 caractères) : « Délibéré par… / Rendu public… » ferment le dispositif.
        let decision = fixture(PDR);
        assert_eq!(decision.char_len(), 1_806);
        let got = summary(&decision, None, rule, 4_000);
        assert_eq!(got.source, SummarySource::Dispositif);
        assert!(!got.truncated);
        assert!(
            got.text.starts_with("Article 1er. - Les deux candidats habilités"),
            "{}",
            got.text
        );
        assert!(got.text.contains("Monsieur Emmanuel MACRON et Madame Marine LE PEN."));
        assert!(got.text.contains("Article 2. - La présente décision sera publiée"));
        assert!(!got.text.contains("Délibéré") && !got.text.contains("Rendu public"));
        assert!(!got.text.contains("DÉCIDE"));

        // Texte synthétique : « Rendu public » ferme seul, de même que « Délibéré par ».
        for footer in
            ["Rendu public le 13 avril 2022.", "Délibéré par le Conseil, où siégeaient : X."]
        {
            let decision = long(&[
                "Visa.",
                "Le Conseil constitutionnel décide :",
                "Article 1er. - A.",
                footer,
            ]);
            let got = summary(&decision, None, rule, 4_000);
            assert_eq!(
                parts(&got),
                (SummarySource::Dispositif, "Article 1er. - A.", false),
                "{footer}"
            );
        }

        // Jumeau : une phrase qui contient seulement « rendu public » ne ferme rien.
        let decision = long(&[
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            "Article 1er. - A.",
            "Article 2. - Le texte est rendu public.",
        ]);
        let got = summary(&decision, None, rule, 4_000);
        assert_eq!(
            parts(&got),
            (
                SummarySource::Dispositif,
                "Article 1er. - A.\n\nArticle 2. - Le texte est rendu public.",
                false
            )
        );
    }

    #[test]
    fn dispositif_constit_orga_with_decide_alone() {
        let rule = DispositifRule::ConseilConstitutionnel;

        // 2021-150 ORGA : l'en-tête est « DÉCIDE : » seul, la clôture « Délibéré par ».
        let decision = fixture(ORGA);
        let got = summary(&decision, None, rule, 4_000);
        assert_eq!(got.source, SummarySource::Dispositif);
        assert!(got.text.starts_with("Article 1er. - L'ordre selon lequel est établie la liste"));
        // Le dispositif dépasse le plafond de 1 200 caractères : coupé, sans clôture.
        assert!(got.truncated);
        assert!(chars(&got.text) <= SUMMARY_CAP_CHARS);
        assert!(!got.text.contains("Délibéré") && !got.text.contains("DÉCIDE"));

        for header in ["DÉCIDE :", "Décide", "D E C I D E :", "DECIDE:"] {
            let decision =
                long(&["Visa.", header, "Article 1er. - A.", "Délibéré par le Conseil."]);
            let got = summary(&decision, None, rule, 4_000);
            assert_eq!(
                parts(&got),
                (SummarySource::Dispositif, "Article 1er. - A.", false),
                "{header}"
            );
        }
    }

    #[test]
    fn dispositif_constit_without_footer_runs_to_the_end() {
        let rule = DispositifRule::ConseilConstitutionnel;
        let decision = long(&[
            "Visa.",
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            "Article 1er. - A.",
            "-----",
            "Article 2. - B.",
        ]);
        let got = summary(&decision, None, rule, 4_000);
        assert_eq!(
            parts(&got),
            (SummarySource::Dispositif, "Article 1er. - A.\n\nArticle 2. - B.", false)
        );

        // Un « Jugé par » avant l'en-tête ne ferme rien : le dispositif court jusqu'à la fin.
        let decision = long(&["Jugé par erreur.", "DÉCIDE :", "Article 1er. - A."]);
        let got = summary(&decision, None, rule, 4_000);
        assert_eq!(parts(&got), (SummarySource::Dispositif, "Article 1er. - A.", false));

        // Jumeau : la règle du Conseil d'État garde l'exigence des deux marqueurs.
        let decision = long(&["Visa.", "D E C I D E :", "Article 1er : A."]);
        let got = summary(&decision, None, DispositifRule::ConseilEtat, 4_000);
        assert_eq!(got.source, SummarySource::Lead);
    }

    #[test]
    fn dispositif_of_a_dc_or_qpc_without_closure_is_open_ended_by_choice() {
        // L4-03 : la règle du Conseil constitutionnel est la même pour toutes ses décisions. Sans
        // « Jugé par », « Délibéré » ni « Rendu public », le dispositif court jusqu'à la fin du
        // texte, signatures comprises : choix assumé (03 §7.9), figé ici pour DC et QPC.
        let rule = DispositifRule::ConseilConstitutionnel;
        let decision = long(&[
            "Décision n° 2025-1000 DC du 6 novembre 2025.",
            "Vu la Constitution.",
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            "Article 1er. - La loi est conforme à la Constitution.",
            "Article 2. - Cette décision sera publiée au Journal officiel.",
            "Le président, Laurent FABIUS.",
        ]);
        let got = summary(&decision, None, rule, 4_000);
        assert_eq!(
            parts(&got),
            (
                SummarySource::Dispositif,
                "Article 1er. - La loi est conforme à la Constitution.\n\nArticle 2. - Cette \
                 décision sera publiée au Journal officiel.\n\nLe président, Laurent FABIUS.",
                false
            )
        );

        // Fixture QPC réelle privée de sa clôture : mêmes articles, puis tout ce qui suit.
        let full = fixture(CONSTIT);
        let paragraphs: Vec<&str> = full.paragraphs().map(|(_, p)| p).collect();
        let footer = paragraphs.iter().position(|p| marker_key(p).starts_with("jugepar")).unwrap();
        let mut open: Vec<&str> = paragraphs[..footer].to_vec();
        open.push("Le président, Laurent FABIUS.");
        let got = summary(&long(&open), None, rule, 4_000);
        assert_eq!(got.source, SummarySource::Dispositif);
        assert!(got.text.starts_with("Article 1er. - Le renvoi opéré"));
        assert!(got.text.ends_with("Le président, Laurent FABIUS."));
        assert_eq!(chars(&got.text), 551 + 2 + "Le président, Laurent FABIUS.".chars().count());

        // Jumeau : avec la clôture, la signature n'est pas dans le dispositif.
        let closed = long(&[
            "LE CONSEIL CONSTITUTIONNEL DÉCIDE :",
            "Article 1er. - A.",
            "Jugé par le Conseil constitutionnel.",
            "Le président, Laurent FABIUS.",
        ]);
        assert_eq!(parts(&summary(&closed, None, rule, 4_000)).1, "Article 1er. - A.");
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
        fn an_official_source_always_has_enough_alphanumerics(
            analyses in proptest::collection::vec("[ a-zA-Z0-9é.\\-]{0,40}", 0..4),
            titrages in proptest::collection::vec("[ a-zA-Z0-9é.\\-]{0,40}", 0..4),
        ) {
            let body = long(&["Texte de repli."]);
            let given = OfficialSummary { analyses, titrages };
            let got = summary(&body, Some(&given), DispositifRule::None, SUMMARY_CAP_CHARS);
            // Quatre entrées de 40 caractères au plus tiennent dans le plafond : jamais coupé.
            prop_assert!(!got.truncated);
            if got.source == SummarySource::Official {
                prop_assert!(alnum(&got.text) >= OFFICIAL_MIN_ALNUM_CHARS);
            } else {
                prop_assert_eq!(got.text.as_str(), "Texte de repli.");
            }
        }

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
