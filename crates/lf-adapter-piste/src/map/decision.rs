//! Décision de justice de `/consult/juri` vers `Decision` (03 §4.4 ; 04 §9.1 a à f, §9.2).
//!
//! Un objet sans identité est un `NotFound`. L'identifiant renvoyé est gardé, même s'il diffère
//! de celui demandé (compté) ; absent ou blanc, c'est l'identifiant demandé (compté aussi). C'est
//! lui qui donne le fond de la décision, donc son URL et la répartition de son sommaire : un champ
//! `origine` qui le contredit est seulement compté (`origin_mismatch`). La date est
//! `dateTexteComputed` si elle est présente, sinon `dateTexte`. Le texte intégral préfère le HTML
//! complet au texte brut et ses invisibles retirés sont comptés ; les champs courts passent par
//! `clean_field`, sans compter les leurs. La décision attaquée n'est pas lue.
//!
//! Trois règles sont propres aux décisions : l'ECLI natif ([`native_ecli`]), le numéro d'affaire
//! ([`numero_affaire`], D30) et la répartition du sommaire officiel par fond ([`summary_of`],
//! D29). Aucune valeur du corps ne sort d'ici : un ECLI natif refusé est compté
//! (`invalid_native_ecli`) puis omis, sans erreur.

use lf_application::ports::CorpusError;
use lf_domain::entity::{Decision, OfficialSummary};
use lf_domain::id::DecisionId;
use lf_domain::{ecli::Ecli, fond::Fond, text::clean_field};

use super::body::non_blank;
use super::dates::prefer_computed;
use super::endpoint::JURI;
use super::rules::{check_origin, long_text, require, resolve_id};
use super::{Diagnostics, Field};
use crate::dto::juri::{ConsultJuriResponseDto, SommaireDto};

/// Champ `text.id` de la réponse.
const ID: Field = Field { endpoint: JURI, path: "text.id" };
/// Date de la décision si elle est valide, lue en priorité.
const DATE_COMPUTED: Field = Field { endpoint: JURI, path: "text.dateTexteComputed" };
/// Date brute de la décision.
const DATE_TEXTE: Field = Field { endpoint: JURI, path: "text.dateTexte" };
/// Texte intégral, en HTML (`texteHtml`) ou brut (`texte`).
const TEXTE: Field = Field { endpoint: JURI, path: "text.texte" };

/// `/consult/juri` ⇒ `Decision` (03 §4.4). Sur `Err(Decode)`, `diags` garde les anomalies
/// relevées avant l'erreur : `Stats::record_mapping` est appelé sur `Ok` comme sur `Err`
/// (04 §9.3), appelé par `PisteCorpus::fetch` ; un `NotFound` ne touche pas `diags`.
///
/// Ordre d'évaluation figé, la première erreur l'emporte : l'identité de l'objet, l'identifiant
/// renvoyé, la date, puis le texte intégral. L'origine est contrôlée contre l'identifiant
/// retenu, jamais contre celui demandé.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`/consult/juri`) si l'enveloppe `text` est absente ou `null`, ou si
/// l'objet n'a ni `id`, ni `titre`, ni `titreLong` non blanc ; `diags` n'est alors pas touché.
/// [`CorpusError::Decode`] du premier champ refusé : `text.id` (`invalid identifier`),
/// `text.dateTexteComputed` ou `text.dateTexte` (`unreadable date`, seul le champ choisi est lu),
/// `text.texte` (`text too large`).
pub(crate) fn decision(
    dto: ConsultJuriResponseDto,
    requested: &DecisionId,
    diags: &mut Diagnostics,
) -> Result<Decision, CorpusError> {
    let juri = require(dto.text, JURI)?;
    let decision_id = resolve_id(requested, juri.id.as_deref(), ID, diags)?;
    let fond = decision_id.fond();
    check_origin(juri.origine.as_deref(), decision_id.as_str(), diags);
    let date_texte = prefer_computed(
        (juri.date_texte_computed.as_ref(), DATE_COMPUTED),
        (juri.date_texte.as_ref(), DATE_TEXTE),
        diags,
    )?;
    let text = long_text(juri.texte_html.as_deref(), juri.texte.as_deref(), TEXTE, diags)?;
    Ok(Decision {
        decision_id,
        num: clean_field(juri.num.as_deref()),
        juridiction: clean_field(juri.juridiction.as_deref()),
        formation: clean_field(juri.formation.as_deref()),
        solution: clean_field(juri.solution.as_deref()),
        nature: clean_field(juri.nature.as_deref()),
        title: clean_field(juri.titre.as_deref()),
        ecli: native_ecli(juri.ecli.as_deref(), diags),
        numero_affaire: numero_affaire(fond, &juri.numero_affaire, juri.num.as_deref()),
        official_summary: summary_of(fond, &juri.sommaire),
        date_texte,
        text,
    })
}

/// ECLI natif de `raw`, validé par `Ecli::parse` et jamais fabriqué.
///
/// Absent ou blanc : `None`, sans diagnostic. Présent mais refusé : `None`, et l'anomalie est
/// comptée (`invalid_native_ecli`).
fn native_ecli(raw: Option<&str>, diags: &mut Diagnostics) -> Option<Ecli> {
    let raw = non_blank(raw)?;
    let ecli = Ecli::parse(raw).ok();
    if ecli.is_none() {
        diags.note_invalid_ecli();
    }
    ecli
}

/// Numéros d'affaire de la décision (D30).
///
/// Les entrées de `listed` passent par `clean_field`, dans l'ordre reçu, celles qui sont vides
/// écartées, sans dédoublonnage. Si aucune n'est gardée et que le fond est `Cetat`, le numéro
/// `num` du texte, nettoyé, la remplace quand il n'est pas vide ; les autres fonds n'ont pas de
/// repli.
fn numero_affaire(fond: Fond, listed: &[String], num: Option<&str>) -> Vec<String> {
    let cleaned: Vec<String> = listed.iter().filter_map(|raw| clean_field(Some(raw))).collect();
    if cleaned.is_empty() && fond == Fond::Cetat {
        return clean_field(num).into_iter().collect();
    }
    cleaned
}

/// Sommaire officiel de la décision, réparti selon le fond (D29).
///
/// - `Fond::Juri` : `abstrats` donne les analyses ; `resumePrincipal` puis `autreResume`
///   donnent les titrages.
/// - `Fond::Cetat` : `resumePrincipal` donne les analyses ; `abstrats` puis `autreResume`
///   donnent les titrages (`autreResume` n'est pas attesté pour ce fond).
/// - Tout autre fond (`CONSTIT`, `CNIL`…) : sommaire vide.
///
/// L'ordre des entrées est gardé ; le nettoyage, le retrait du code de classement et le
/// dédoublonnage sont ceux d'[`OfficialSummary::new`]. Les `titrages` de premier niveau de la
/// réponse (identifiants de nomenclature) ne sont jamais lus.
fn summary_of(fond: Fond, sommaire: &[SommaireDto]) -> OfficialSummary {
    let (analyses, titrages) = match fond {
        Fond::Juri => (
            texts(sommaire, |entry| [entry.abstrats.as_deref(), None]),
            texts(sommaire, |entry| {
                [entry.resume_principal.as_deref(), entry.autre_resume.as_deref()]
            }),
        ),
        Fond::Cetat => (
            texts(sommaire, |entry| [entry.resume_principal.as_deref(), None]),
            texts(sommaire, |entry| [entry.abstrats.as_deref(), entry.autre_resume.as_deref()]),
        ),
        _ => return OfficialSummary::default(),
    };
    OfficialSummary::new(analyses, titrages)
}

/// Textes présents que `pick` tire de chaque entrée, entrée après entrée.
fn texts<'a>(
    sommaire: &'a [SommaireDto],
    pick: impl Fn(&'a SommaireDto) -> [Option<&'a str>; 2],
) -> Vec<&'a str> {
    sommaire.iter().flat_map(pick).flatten().collect()
}

#[cfg(test)]
mod tests {
    use lf_domain::contract::Citable;
    use lf_domain::date::CivilDate;
    use lf_domain::text::LongText;

    use super::*;
    use crate::dto::de::RawDate;
    use crate::dto::decode::decode;
    use crate::dto::juri::{ConsultJuriResponseDto, JuriTextDto};
    use crate::map::fixtures::{self, decode_as, decode_error, juri_with, read};

    /// Identifiant demandé de la fixture `consult_juri`.
    const JURI_ID: &str = "JURITEXT000051743650";
    /// Identifiant demandé de la fixture `consult_juri_cetat`.
    const CETAT_ID: &str = "CETATEXT000054178503";
    /// Identifiant demandé de la fixture `consult_juri_constit`.
    const CONSTIT_ID: &str = "CONSTEXT000052555275";
    /// Les trois fixtures de `/consult/juri`.
    const DECISIONS: [&str; 3] = ["consult_juri", "consult_juri_cetat", "consult_juri_constit"];

    /// Identifiant de décision valide.
    fn decision_id(raw: &str) -> DecisionId {
        DecisionId::parse(raw).unwrap()
    }

    /// `decision` de `dto` demandée en `requested`, et les diagnostics qu'elle laisse.
    fn decision_of(
        dto: ConsultJuriResponseDto,
        requested: &str,
    ) -> (Result<Decision, CorpusError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let decision = decision(dto, &decision_id(requested), &mut diags);
        (decision, diags)
    }

    /// Décision de la fixture `name` modifiée par `edit`, demandée sous l'identifiant de sa
    /// capture, et les diagnostics qu'elle laisse.
    fn edited(name: &str, edit: impl FnOnce(&mut JuriTextDto)) -> (Decision, Diagnostics) {
        let mut dto = decode_as::<ConsultJuriResponseDto>(name);
        edit(dto.text.as_mut().unwrap());
        let (decision, diags) = decision_of(dto, fixtures::fixture(name).requested);
        (decision.unwrap(), diags)
    }

    /// Décision de la fixture `name`, telle que capturée, et les diagnostics qu'elle laisse.
    fn fixture_decision(name: &str) -> (Decision, Diagnostics) {
        edited(name, |_| ())
    }

    /// Texte synthétique d'identifiant [`JURI_ID`], modifié ensuite par `edit`.
    fn juri_text(edit: impl FnOnce(&mut JuriTextDto)) -> ConsultJuriResponseDto {
        juri_with(|text| {
            text.id = Some(JURI_ID.to_owned());
            edit(text);
        })
    }

    /// Nombre de caractères de `text`.
    fn chars(text: &str) -> usize {
        text.chars().count()
    }

    /// Chaque entrée du sommaire tient sur une ligne, sans balise.
    fn assert_flat(summary: &OfficialSummary) {
        for entry in summary.analyses.iter().chain(&summary.titrages) {
            assert!(!entry.contains('<') && !entry.contains('\n'), "{entry}");
        }
    }

    /// Valeurs attendues du mapping d'une fixture de décision.
    struct Expected {
        /// Nom de la fixture.
        name: &'static str,
        /// Identifiant de la décision.
        decision_id: &'static str,
        /// ECLI natif.
        ecli: &'static str,
        /// Numéro.
        num: &'static str,
        /// Juridiction.
        juridiction: &'static str,
        /// Formation et solution.
        formation_solution: (Option<&'static str>, Option<&'static str>),
        /// Numéros d'affaire.
        numero_affaire: &'static [&'static str],
        /// Date de la décision.
        date_texte: Option<CivilDate>,
        /// Nature.
        nature: &'static str,
        /// Titre.
        title: &'static str,
        /// Nombre de paragraphes et de caractères du texte.
        shape: (usize, u32),
        /// Début du texte.
        starts_with: &'static str,
        /// Fin du texte, quand elle est affirmée.
        ends_with: Option<&'static str>,
        /// Nombre d'analyses et de titrages du sommaire.
        summary: (usize, usize),
    }

    /// Les trois cas de [`decision_fixture_maps`].
    fn expected() -> [Expected; 3] {
        [
            Expected {
                name: "consult_juri",
                decision_id: JURI_ID,
                ecli: "ECLI:FR:CCASS:2025:C100399",
                num: "12500399",
                juridiction: "Cour de cassation",
                formation_solution: (Some("CHAMBRE_CIVILE_1"), Some("Cassation")),
                numero_affaire: &["24-13.470"],
                date_texte: CivilDate::new(2025, 6, 4),
                nature: "ARRET",
                title: "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, \
                        Publié au bulletin",
                shape: (1, 577),
                starts_with: "LA COUR DE CASSATION, PREMIÈRE CHAMBRE CIVILE",
                ends_with: None,
                summary: (1, 2),
            },
            Expected {
                name: "consult_juri_cetat",
                decision_id: CETAT_ID,
                ecli: "ECLI:FR:CECHR:2026:506507.20260529",
                num: "506507",
                juridiction: "Conseil d'État",
                formation_solution: (Some("2ème - 7ème chambres réunies"), None),
                numero_affaire: &["506507"],
                date_texte: CivilDate::new(2026, 5, 29),
                nature: "Texte",
                title: "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507",
                shape: (43, 14_608),
                starts_with: "Vu la procédure suivante :",
                ends_with: Some("Signé : Mme Sandrine Mendy"),
                summary: (1, 3),
            },
            Expected {
                name: "consult_juri_constit",
                decision_id: CONSTIT_ID,
                ecli: "ECLI:FR:CC:2025:2025.1173.QPC",
                num: "2025-1173",
                juridiction: "Conseil constitutionnel",
                formation_solution: (None, Some("Conformité")),
                numero_affaire: &[],
                date_texte: CivilDate::new(2025, 11, 7),
                nature: "QPC",
                title: "Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de \
                        publicité des débats devant le juge des libertés et de la détention \
                        statuant sur la détention provisoire du prévenu] - Conformité",
                shape: (49, 14_332),
                starts_with: "LE CONSEIL CONSTITUTIONNEL A ÉTÉ SAISI le 8 août 2025",
                ends_with: Some("Rendu public le 7 novembre 2025."),
                summary: (0, 0),
            },
        ]
    }

    #[test]
    fn decision_fixture_maps() {
        for case in expected() {
            let name = case.name;
            let (decision, diags) = fixture_decision(name);
            assert_eq!(decision.decision_id, decision_id(case.decision_id), "{name}");
            assert_eq!(decision.decision_id.as_str(), case.decision_id, "{name}");
            assert_eq!(decision.ecli.as_ref().map(Ecli::as_str), Some(case.ecli), "{name}");
            assert_eq!(decision.num.as_deref(), Some(case.num), "{name}");
            assert_eq!(decision.juridiction.as_deref(), Some(case.juridiction), "{name}");
            assert_eq!(
                (decision.formation.as_deref(), decision.solution.as_deref()),
                case.formation_solution,
                "{name}"
            );
            assert_eq!(decision.numero_affaire, case.numero_affaire, "{name}");
            assert_eq!(decision.date_texte, case.date_texte, "{name}");
            assert_eq!(decision.nature.as_deref(), Some(case.nature), "{name}");
            assert_eq!(decision.title.as_deref(), Some(case.title), "{name}");

            let text = &decision.text;
            assert_eq!((text.paragraphs().count(), text.char_len()), case.shape, "{name}");
            assert!(text.as_str().starts_with(case.starts_with), "{name}");
            assert!(case.ends_with.is_none_or(|end| text.as_str().ends_with(end)), "{name}");

            let summary = &decision.official_summary;
            assert_eq!((summary.analyses.len(), summary.titrages.len()), case.summary, "{name}");
            assert_eq!(diags, Diagnostics::default(), "{name}");
        }
        assert_eq!(
            fixture_decision("consult_juri_constit").0.official_summary,
            OfficialSummary::default()
        );
    }

    #[test]
    fn summary_by_fond_juri() {
        let summary = fixture_decision("consult_juri").0.official_summary;
        assert_eq!(summary.analyses.len(), 1);
        let analysis = &summary.analyses[0];
        assert_eq!(chars(analysis), 829);
        assert!(
            analysis
                .starts_with("L'action en responsabilité dirigée contre le fabricant d'un produit")
        );
        assert!(analysis.ends_with("et de l'identité du producteur"));

        let titrages = &summary.titrages;
        assert_eq!(titrages.iter().map(|entry| chars(entry)).collect::<Vec<_>>(), [179, 915]);
        assert!(titrages[0].starts_with(
            "Cassation civil - RESPONSABILITE DU FAIT DES PRODUITS DEFECTUEUX - Producteur"
        ));
        assert!(titrages[0].ends_with("Prescription - Délai - Détermination"));
        assert!(titrages[1].ends_with("du Conseil du 25 juillet 1985"));
        assert_flat(&summary);
    }

    #[test]
    fn summary_by_fond_cetat() {
        let summary = fixture_decision("consult_juri_cetat").0.official_summary;
        assert_eq!(summary.analyses.len(), 1);
        let analysis = &summary.analyses[0];
        assert_eq!(chars(analysis), 736);
        assert!(
            analysis
                .starts_with("Projet de plan local d\u{2019}urbanisme dont la mise en œuvre est")
        );
        assert!(analysis.ends_with("d\u{2019}une telle consultation."));

        let heads = [
            (468, "ACTES LÉGISLATIFS ET ADMINISTRATIFS. - VALIDITÉ DE"),
            (344, "NATURE ET ENVIRONNEMENT. - OBLIGATION DE TRANSMISS"),
            (457, "URBANISME ET AMÉNAGEMENT DU TERRITOIRE. - PLANS D"),
        ];
        assert_eq!(summary.titrages.len(), heads.len());
        for (titrage, (len, head)) in summary.titrages.iter().zip(heads) {
            assert_eq!(chars(titrage), len, "{head}");
            assert!(titrage.starts_with(head), "{head}");
            assert!(titrage.ends_with("SEULES COMMUNES LIMITROPHES DU PROJET."), "{head}");
        }
        for entry in summary.analyses.iter().chain(&summary.titrages) {
            assert!(!entry.starts_with(|c: char| c.is_ascii_digit()), "{entry}");
            assert!(!entry.starts_with("CETAT"), "{entry}");
        }
        assert_flat(&summary);
    }

    #[test]
    fn native_ecli_is_validated_not_fabricated() {
        let (decision, diags) = edited("consult_juri_constit", |text| text.ecli = None);
        assert_eq!((decision.ecli.as_ref(), decision.contract().ecli), (None, None));
        assert_eq!(diags, Diagnostics::default());

        let (decision, diags) =
            edited("consult_juri_constit", |text| text.ecli = Some("ECLI:FR:CC:2025".to_owned()));
        assert_eq!((decision.ecli.as_ref(), decision.contract().ecli), (None, None));
        assert_eq!(diags, Diagnostics { invalid_native_ecli: 1, ..Diagnostics::default() });

        for name in DECISIONS {
            let decision = fixture_decision(name).0;
            assert!(decision.ecli.is_some(), "{name}");
            assert_eq!(decision.contract().ecli, decision.ecli, "{name}");
        }
    }

    #[test]
    fn empty_text_envelope_maps_to_not_found() {
        let before =
            Diagnostics { id_mismatch: 2, invalid_native_ecli: 3, ..Diagnostics::default() };
        for json in [
            "{}",
            r#"{"text":null}"#,
            r#"{"text":{}}"#,
            r#"{"text":{"origine":"JURI","texte":"abc"}}"#,
        ] {
            let mut diags = before;
            assert_eq!(
                decision(read(json, JURI), &decision_id(JURI_ID), &mut diags),
                Err(CorpusError::NotFound { what: "/consult/juri".into() }),
                "{json}"
            );
            assert_eq!(diags, before, "{json}");
        }
    }

    #[test]
    fn decision_returned_id_wins_and_drives_the_fond() {
        let (mapped, diags) = decision_of(read(r#"{"text":{"titre":"T"}}"#, JURI), JURI_ID);
        let mapped = mapped.unwrap();
        assert_eq!(mapped.decision_id, decision_id(JURI_ID));
        assert_eq!(mapped.title.as_deref(), Some("T"));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });

        let json = r#"{"text":{"id":"CETATEXT000054178503","origine":"CETAT","sommaire":[
            {"resumePrincipal":"01-02 Analyse un."},{"abstrats":"CETAT01-02 TITRAGE UN."}]}}"#;
        let (mapped, diags) = decision_of(read(json, JURI), JURI_ID);
        let mapped = mapped.unwrap();
        assert_eq!(mapped.decision_id, decision_id(CETAT_ID));
        assert_eq!(mapped.decision_id.fond(), Fond::Cetat);
        assert_eq!((diags.id_mismatch, diags.origin_mismatch), (1, 0));
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });
        assert_eq!(mapped.official_summary, summary(&["Analyse un."], &["TITRAGE UN."]));
    }

    #[test]
    fn decision_suffixed_or_foreign_id_follows_the_shared_rules() {
        let (mapped, diags) = decision_of(
            read(r#"{"text":{"id":"JURITEXT000051743650_01-01-2020"}}"#, JURI),
            JURI_ID,
        );
        assert_eq!(mapped.unwrap().decision_id, decision_id(JURI_ID));
        assert_eq!(diags, Diagnostics { upstream_suffix_stripped: 1, ..Diagnostics::default() });

        let (mapped, diags) =
            decision_of(read(r#"{"text":{"id":"CNILTEXT000053352643"}}"#, JURI), JURI_ID);
        assert_eq!(mapped, Err(decode_error(JURI, "text.id: invalid identifier")));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn decision_origin_mismatch_is_counted_and_never_routes() {
        let dto = juri_with(|text| {
            text.id = Some(CONSTIT_ID.to_owned());
            text.origine = Some("JURI".to_owned());
        });
        let (mapped, diags) = decision_of(dto, CONSTIT_ID);
        let mapped = mapped.unwrap();
        assert_eq!(mapped.decision_id, decision_id(CONSTIT_ID));
        assert_eq!(diags, Diagnostics { origin_mismatch: 1, ..Diagnostics::default() });
        assert_eq!(
            mapped.contract().source_url.as_str(),
            "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000052555275"
        );
    }

    #[test]
    fn decision_date_prefers_computed() {
        let computed = RawDate::Millis(1_748_995_200_000);
        let plain = RawDate::Millis(1_762_473_600_000);
        let sentinel = RawDate::Millis(32_472_144_000_000);
        let cases = [
            (Some(&computed), Some(&plain), CivilDate::new(2025, 6, 4)),
            (Some(&computed), None, CivilDate::new(2025, 6, 4)),
            (None, Some(&plain), CivilDate::new(2025, 11, 7)),
            (Some(&sentinel), Some(&plain), None),
            (None, None, None),
        ];
        for (computed, plain, expected) in cases {
            let dto = juri_text(|text| {
                text.date_texte_computed = computed.cloned();
                text.date_texte = plain.cloned();
            });
            let (mapped, diags) = decision_of(dto, JURI_ID);
            assert_eq!(mapped.unwrap().date_texte, expected, "{computed:?} {plain:?}");
            assert_eq!(diags, Diagnostics::default(), "{computed:?} {plain:?}");
        }
    }

    #[test]
    fn decision_unreadable_date_is_decode() {
        let junk = RawDate::Text("hier".to_owned());
        let good = RawDate::Millis(1_762_473_600_000);
        let cases = [
            (None, Some(&junk), "text.dateTexte: unreadable date"),
            (Some(&junk), Some(&good), "text.dateTexteComputed: unreadable date"),
        ];
        for (computed, plain, detail) in cases {
            let dto = juri_text(|text| {
                text.date_texte_computed = computed.cloned();
                text.date_texte = plain.cloned();
            });
            assert_eq!(decision_of(dto, JURI_ID).0, Err(decode_error(JURI, detail)), "{detail}");
        }
    }

    #[test]
    fn a_decode_error_keeps_the_anomalies_counted_before_it() {
        let json = r#"{"text":{"id":"CETATEXT000054178503","origine":"JURI","dateTexte":"hier"}}"#;
        let (mapped, diags) = decision_of(read(json, JURI), JURI_ID);
        assert_eq!(mapped, Err(decode_error(JURI, "text.dateTexte: unreadable date")));
        assert_eq!(
            diags,
            Diagnostics { id_mismatch: 1, origin_mismatch: 1, ..Diagnostics::default() }
        );
    }

    #[test]
    fn cetat_numero_affaire_falls_back_to_num() {
        assert_eq!(fixture_decision("consult_juri_cetat").0.numero_affaire, ["506507"]);

        let (decision, _) = edited("consult_juri_cetat", |text| {
            text.numero_affaire = vec!["22NC02259".to_owned()];
        });
        assert_eq!(decision.numero_affaire, ["22NC02259"]);

        let (decision, diags) = edited("consult_juri", |text| text.numero_affaire.clear());
        assert_eq!(decision.numero_affaire, Vec::<String>::new());
        assert_eq!(decision.num.as_deref(), Some("12500399"));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn invisible_chars_are_counted_for_the_text_only() {
        let (mapped, diags) =
            decision_of(juri_text(|text| text.texte = Some("a\u{202e}b".to_owned())), JURI_ID);
        assert_eq!(mapped.unwrap().text.as_str(), "ab");
        assert_eq!(diags, Diagnostics { invisible_chars_removed: 1, ..Diagnostics::default() });

        let dto = juri_with(|text| {
            text.id = Some(JURI_ID.to_owned());
            text.texte = Some("a\u{202e}b".to_owned());
            text.juridiction = Some("Cour\u{200b}".to_owned());
        });
        let (mapped, diags) = decision_of(dto, JURI_ID);
        let mapped = mapped.unwrap();
        assert_eq!((mapped.text.as_str(), mapped.juridiction.as_deref()), ("ab", Some("Cour")));
        assert_eq!(diags, Diagnostics { invisible_chars_removed: 1, ..Diagnostics::default() });
    }

    #[test]
    fn the_attacked_decision_is_never_read() {
        // `decisionAttaquee` n'est plus déclarée (plan 2g, D30 renvoyée à M7) : de toute forme,
        // elle ne change pas la décision.
        let fixture = fixtures::fixture("consult_juri");
        let mut body: serde_json::Value =
            serde_json::from_str(&fixtures::decoded(fixture, true).text).unwrap();
        body["text"]["decisionAttaquee"] = serde_json::json!([1, {"formation": true}]);
        let dto = read::<ConsultJuriResponseDto>(&body.to_string(), JURI);
        let (decision, diags) = decision_of(dto, fixture.requested);
        assert_eq!((decision.unwrap(), diags), fixture_decision("consult_juri"));
    }

    #[test]
    fn decision_errors_never_echo_body_values() {
        let cases = [
            (r#"{"text":{"id":"JURITEXT-TOPSECRET"}}"#, "text.id: invalid identifier"),
            (
                r#"{"text":{"id":"JURITEXT000051743650","dateTexte":"TOPSECRET"}}"#,
                "text.dateTexte: unreadable date",
            ),
        ];
        for (json, detail) in cases {
            let error = decision_of(read(json, JURI), JURI_ID).0.unwrap_err();
            assert_eq!(error, decode_error(JURI, detail), "{detail}");
            assert!(!format!("{error:?} {error}").contains("TOPSECRET"), "{detail}");
        }
    }

    #[test]
    fn decision_errors_follow_the_field_order() {
        let id_then_date = read(r#"{"text":{"id":"JURITEXT-X","dateTexte":"hier"}}"#, JURI);
        assert_eq!(
            decision_of(id_then_date, JURI_ID).0,
            Err(decode_error(JURI, "text.id: invalid identifier"))
        );

        let size = usize::try_from(LongText::MAX_CHARS).unwrap() + 1;
        let cases = [
            (Some(RawDate::Text("hier".to_owned())), "text.dateTexte: unreadable date"),
            (None, "text.texte: text too large"),
        ];
        for (date, detail) in cases {
            let huge = juri_text(|text| {
                text.texte = Some("a".repeat(size));
                text.date_texte = date;
            });
            assert_eq!(decision_of(huge, JURI_ID).0, Err(decode_error(JURI, detail)), "{detail}");
        }
    }

    /// Entrée de sommaire de `abstrats`, `resumePrincipal` et `autreResume`.
    fn entry(abstrats: Option<&str>, principal: Option<&str>, autre: Option<&str>) -> SommaireDto {
        SommaireDto {
            abstrats: abstrats.map(str::to_owned),
            resume_principal: principal.map(str::to_owned),
            autre_resume: autre.map(str::to_owned),
        }
    }

    /// `values` en `Vec<String>`.
    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    /// Sommaire attendu de `analyses` et `titrages`.
    fn summary(analyses: &[&str], titrages: &[&str]) -> OfficialSummary {
        OfficialSummary { analyses: owned(analyses), titrages: owned(titrages) }
    }

    #[test]
    fn native_ecli_keeps_a_valid_value() {
        for raw in [
            "ECLI:FR:CCASS:2025:C100399",
            "ECLI:FR:CC:2025:2025.1173.QPC",
            "ECLI:FR:CECHR:2026:506507.20260529",
        ] {
            let mut diags = Diagnostics::default();
            let ecli = native_ecli(Some(raw), &mut diags);
            assert_eq!(ecli.as_ref().map(Ecli::as_str), Some(raw));
            assert_eq!(diags, Diagnostics::default(), "{raw}");
        }
    }

    #[test]
    fn native_ecli_invalid_is_none_and_counted() {
        for raw in [
            "ECLI:FR:CC:2025",
            "ecli:fr:cc:2025:2025.1173.qpc",
            "ECLI:EN:CC:2025:1",
            "ECLI:FR:CC:2025:2025.1173.QPC:extra",
        ] {
            let mut diags = Diagnostics::default();
            assert_eq!(native_ecli(Some(raw), &mut diags), None, "{raw}");
            assert_eq!(diags.invalid_native_ecli, 1, "{raw}");

            let mut diags = Diagnostics { invalid_native_ecli: 2, ..Diagnostics::default() };
            assert_eq!(native_ecli(Some(raw), &mut diags), None, "{raw}");
            assert_eq!(diags.invalid_native_ecli, 3, "{raw}");
        }
    }

    #[test]
    fn native_ecli_absent_or_blank_is_none_and_uncounted() {
        for raw in [None, Some(""), Some("  \n")] {
            let mut diags = Diagnostics { invalid_native_ecli: 2, ..Diagnostics::default() };
            assert_eq!(native_ecli(raw, &mut diags), None, "{raw:?}");
            assert_eq!(diags, Diagnostics { invalid_native_ecli: 2, ..Diagnostics::default() });
        }
    }

    #[test]
    fn numero_affaire_keeps_the_listed_values() {
        assert_eq!(numero_affaire(Fond::Juri, &owned(&["24-13.470"]), None), owned(&["24-13.470"]));
        assert_eq!(
            numero_affaire(Fond::Juri, &owned(&["", " 24-13.470 ", "  ", "25-00.001"]), None),
            owned(&["24-13.470", "25-00.001"])
        );
        assert_eq!(
            numero_affaire(Fond::Juri, &owned(&["24-13.470", "24-13.470"]), Some("ignoré")),
            owned(&["24-13.470", "24-13.470"])
        );
    }

    #[test]
    fn numero_affaire_fallback_is_for_cetat_only() {
        for num in [Some("506507"), Some(" 506507 ")] {
            assert_eq!(numero_affaire(Fond::Cetat, &[], num), owned(&["506507"]), "{num:?}");
        }
        assert_eq!(
            numero_affaire(Fond::Cetat, &owned(&["", " "]), Some("506507")),
            owned(&["506507"])
        );
        assert_eq!(
            numero_affaire(Fond::Cetat, &owned(&["22NC02259"]), Some("506507")),
            owned(&["22NC02259"])
        );
        for num in [None, Some("  ")] {
            assert_eq!(numero_affaire(Fond::Cetat, &[], num), Vec::<String>::new(), "{num:?}");
        }
        for fond in [Fond::Juri, Fond::Constit] {
            assert_eq!(numero_affaire(fond, &[], Some("506507")), Vec::<String>::new(), "{fond:?}");
        }
    }

    #[test]
    fn summary_of_juri_splits_abstrats_from_titrages() {
        let one = [entry(Some("Analyse A"), Some("Résumé principal"), Some("Autre résumé"))];
        assert_eq!(
            summary_of(Fond::Juri, &one),
            summary(&["Analyse A"], &["Résumé principal", "Autre résumé"])
        );
        let two = [entry(Some("A1"), Some("R1"), Some("X1")), entry(Some("A2"), Some("R2"), None)];
        assert_eq!(summary_of(Fond::Juri, &two), summary(&["A1", "A2"], &["R1", "X1", "R2"]));
    }

    #[test]
    fn summary_of_cetat_splits_resume_from_abstrats() {
        let entries = [
            entry(None, Some("01-03-02-02 Analyse un."), None),
            entry(None, Some("44 Analyse un."), None),
            entry(Some("CETAT01-03-02-02 TITRAGE UN."), None, None),
            entry(Some("CETAT44 TITRAGE DEUX."), None, None),
        ];
        assert_eq!(
            summary_of(Fond::Cetat, &entries),
            summary(&["Analyse un."], &["TITRAGE UN.", "TITRAGE DEUX."])
        );
    }

    #[test]
    fn summary_of_cetat_keeps_autre_resume_as_titrage() {
        let entries = [entry(Some("T1"), Some("A1"), Some("T2"))];
        assert_eq!(summary_of(Fond::Cetat, &entries), summary(&["A1"], &["T1", "T2"]));
    }

    #[test]
    fn summary_of_other_fonds_is_empty() {
        let full = [entry(Some("A"), Some("R"), Some("X"))];
        for fond in [Fond::Constit, Fond::Cnil] {
            assert_eq!(summary_of(fond, &full), OfficialSummary::default(), "{fond:?}");
        }
        for fond in [Fond::Juri, Fond::Cetat] {
            assert_eq!(summary_of(fond, &[]), OfficialSummary::default(), "{fond:?}");
            assert_eq!(
                summary_of(fond, &[SommaireDto::default()]),
                OfficialSummary::default(),
                "{fond:?}"
            );
        }
    }

    #[test]
    fn summary_ignores_the_nomenclature_ids() {
        let json = r#"{"titrages": ["JURINOME000007644451-JURINOME000007645358"], "sommaire": []}"#;
        let dto: JuriTextDto = decode(json, "/consult/juri").unwrap();
        for fond in [Fond::Juri, Fond::Cetat] {
            assert_eq!(summary_of(fond, &dto.sommaire), OfficialSummary::default(), "{fond:?}");
        }
    }
}
