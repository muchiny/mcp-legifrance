//! Vue de sortie de `fr_get_decision` (05 §5.4) : `DecisionView`.
//!
//! Construite depuis le [`GetDecisionOutput`] de l'application : les métadonnées de la décision
//! (identifiant, ECLI natif, juridiction, formation, solution, numéros d'affaire, date, nature,
//! titre), sa citation et son URL source, puis les champs de lecture communs ([`ReadingView`],
//! aplatis). Les champs absents sont omis (jamais `null`), `title` aussi quand `citation` le
//! contient. L'ECLI n'est jamais fabriqué : il n'est rendu que s'il est natif (04 §9.2).
//!
//! Les commentaires des champs sont des `//` : un commentaire de documentation partirait dans
//! l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::output::get_decision::GetDecisionOutput;
use lf_domain::contract::Citable;
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    common::{date_unless_sentinel, title_unless_in_citation},
    fit::fit_under_cap,
    reading::ReadingView,
};

// Les noms de champs et leur ordre sont ceux de 05 §5.4.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct DecisionView {
    decision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "Native ECLI only, never derived.")]
    ecli: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    juridiction: Option<String>,
    // Brut : un code pour JURI, un texte pour CETAT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    formation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    solution: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    numero_affaire: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    nature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "Omitted when citation already contains it.")]
    title: Option<String>,
    citation: String,
    source_url: String,
    #[serde(flatten)]
    reading: ReadingView,
}

impl From<GetDecisionOutput> for DecisionView {
    fn from(output: GetDecisionOutput) -> Self {
        let GetDecisionOutput { decision, reading } = output;
        let contract = decision.contract();
        let mut view = Self {
            decision_id: decision.decision_id.to_string(),
            // L'ECLI du contrat est l'ECLI natif de la décision, validé : jamais déduit du numéro.
            ecli: contract.ecli.as_ref().map(ToString::to_string),
            juridiction: non_blank(decision.juridiction),
            formation: non_blank(decision.formation),
            solution: non_blank(decision.solution),
            numero_affaire: decision
                .numero_affaire
                .into_iter()
                .filter_map(non_blank_text)
                .collect(),
            date: date_unless_sentinel(decision.date_texte),
            nature: non_blank(decision.nature),
            title: title_unless_in_citation(decision.title.as_deref(), contract.citation.as_str()),
            citation: contract.citation.as_str().to_owned(),
            source_url: contract.source_url.to_string(),
            reading: reading.into(),
        };
        // L'application ne voit pas les échappements JSON : la mesure exacte tranche, et la
        // fenêtre raccourcie garde un `next_offset` exact.
        fit_under_cap(&mut view, |view| view.reading.degrade());
        view
    }
}

/// Valeur de la métadonnée, sauf si elle est absente ou blanche.
fn non_blank(value: Option<String>) -> Option<String> {
    value.and_then(non_blank_text)
}

/// `text`, sauf s'il est vide ou fait de blancs.
fn non_blank_text(text: String) -> Option<String> {
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use lf_application::budget::{PassageOut, Reading};
    use lf_domain::{
        date::CivilDate,
        ecli::Ecli,
        entity::{Decision, OfficialSummary},
        id::DecisionId,
        text::{LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    const CONSTIT: &str = "CONSTEXT000052555275";
    const JURI: &str = "JURITEXT000048465611";
    const CETAT: &str = "CETATEXT000051655317";

    const CONSTIT_TITLE: &str = "Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de publicité des débats devant le juge des libertés et de la détention statuant sur la détention provisoire du prévenu] - Conformité";
    const DISPOSITIF: &str = "Article 1er. - Le renvoi opéré par la première phrase du troisième alinéa de l’article 397-1-1 du code de procédure pénale, dans sa rédaction résultant de la loi du 23 mars 2019 de programmation 2018-2022 et de réforme pour la justice, aux mots « en chambre du conseil » figurant au premier alinéa de l’article 396 du même code est conforme à la Constitution.\n\nArticle 2. - Cette décision sera publiée au Journal officiel de la République française et notifiée dans les conditions prévues à l’article 23-11 de l’ordonnance du 7 novembre 1958 susvisée.";

    /// Une décision qui ne porte que son identifiant.
    fn bare(id: &str) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: Vec::new(),
            date_texte: None,
            nature: None,
            title: None,
            text: LongText::default(),
            official_summary: OfficialSummary::default(),
            cited_case_law: None,
            applied_texts: vec![],
        }
    }

    /// La décision de l'exemple 1 de 05 §5.4 (fixture `consult_juri_constit.json`).
    fn constit() -> Decision {
        Decision {
            ecli: Ecli::parse("ECLI:FR:CC:2025:2025.1173.QPC").ok(),
            num: Some("2025-1173".to_owned()),
            juridiction: Some("Conseil constitutionnel".to_owned()),
            solution: Some("Conformité".to_owned()),
            date_texte: CivilDate::new(2025, 11, 7),
            nature: Some("QPC".to_owned()),
            title: Some(CONSTIT_TITLE.to_owned()),
            ..bare(CONSTIT)
        }
    }

    fn reading(text_length: u32) -> Reading {
        Reading {
            summary: None,
            text: None,
            text_length,
            next_offset: None,
            passages: None,
            note: None,
            summary_truncated: false,
        }
    }

    fn summary_reading() -> Reading {
        Reading {
            summary: Some(Summary {
                source: SummarySource::Dispositif,
                text: DISPOSITIF.to_owned(),
                truncated: false,
            }),
            ..reading(14_332)
        }
    }

    fn output(decision: Decision, reading: Reading) -> GetDecisionOutput {
        GetDecisionOutput { decision, reading }
    }

    fn view(decision: Decision) -> Value {
        serde_json::to_value(DecisionView::from(output(decision, reading(100)))).unwrap()
    }

    /// Clés de l'objet, triées (un `Value` range ses clés par ordre alphabétique).
    fn keys(value: &Value) -> Vec<&str> {
        value.as_object().unwrap().keys().map(String::as_str).collect()
    }

    /// Le JSON compact de la vue écrit `names` dans cet ordre, qui est celui de 05 §5.4.
    fn assert_written_in_order(view: &DecisionView, names: &[&str]) {
        let written = serde_json::to_string(view).unwrap();
        let positions: Vec<usize> =
            names.iter().map(|name| written.find(&format!("\"{name}\":")).expect(name)).collect();
        assert!(positions.is_sorted(), "{names:?} aux positions {positions:?} dans {written}");
    }

    #[test]
    fn the_view_is_the_documented_example() {
        assert_written_in_order(
            &DecisionView::from(output(constit(), summary_reading())),
            &[
                "decision_id",
                "ecli",
                "juridiction",
                "solution",
                "date",
                "nature",
                "title",
                "citation",
                "source_url",
                "summary",
                "summary_source",
                "text_length",
            ],
        );
        let actual =
            serde_json::to_value(DecisionView::from(output(constit(), summary_reading()))).unwrap();
        let expected = json!({
            "decision_id": "CONSTEXT000052555275",
            "ecli": "ECLI:FR:CC:2025:2025.1173.QPC",
            "juridiction": "Conseil constitutionnel",
            "solution": "Conformité",
            "date": "2025-11-07",
            "nature": "QPC",
            "title": CONSTIT_TITLE,
            "citation": "Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025",
            "source_url": "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000052555275",
            "summary": DISPOSITIF,
            "summary_source": "dispositif",
            "text_length": 14_332
        });
        assert_eq!(actual, expected);
        // 1 147 caractères de JSON compact (05 §5.4, §6.5).
        assert_eq!(serde_json::to_string(&actual).unwrap().chars().count(), 1_147);
    }

    #[test]
    fn ecli_native_only() {
        // Le Conseil constitutionnel a un numéro, une nature et une date : un ECLI s'en déduirait
        // (`ECLI:FR:CC:2025:2025.1173.QPC`). Sans ECLI natif, la vue n'en porte aucun.
        let without = Decision { ecli: None, ..constit() };
        let actual = view(without);
        assert!(actual.get("ecli").is_none(), "{actual}");
        assert!(!actual.to_string().contains("ECLI"), "{actual}");
        // Un ECLI natif est rendu tel quel, quel que soit le fond.
        for (id, native) in
            [(JURI, "ECLI:FR:CCASS:2025:C100399"), (CETAT, "ECLI:FR:CECHR:2026:506507.20260529")]
        {
            let decision = Decision { ecli: Ecli::parse(native).ok(), ..bare(id) };
            assert_eq!(view(decision)["ecli"], native);
        }
        // Aucun fond ne reçoit d'ECLI quand le corpus n'en a pas.
        for id in [JURI, CONSTIT, CETAT] {
            assert!(view(bare(id)).get("ecli").is_none(), "{id}");
        }
    }

    #[test]
    fn absent_fields_are_omitted_not_null() {
        let actual = view(bare(JURI));
        assert_eq!(keys(&actual), ["citation", "decision_id", "source_url", "text_length"]);
        // Sans titre, la citation est le repli sur l'identifiant.
        assert_eq!(actual["citation"], "Légifrance JURITEXT000048465611");
        assert_eq!(
            actual["source_url"],
            "https://www.legifrance.gouv.fr/juri/id/JURITEXT000048465611"
        );
        assert!(!actual.to_string().contains("null"));
    }

    #[test]
    fn blank_metadata_is_omitted() {
        let decision = Decision {
            juridiction: Some("  ".to_owned()),
            formation: Some(String::new()),
            solution: Some("\n".to_owned()),
            nature: Some(" ".to_owned()),
            numero_affaire: vec![String::new(), "  ".to_owned()],
            ..bare(JURI)
        };
        assert_eq!(keys(&view(decision)), ["citation", "decision_id", "source_url", "text_length"]);
    }

    #[test]
    fn juri_and_cetat_metadata_is_rendered_raw() {
        // JURI : `formation` est un code ; CETAT : un texte, et le numéro d'affaire est `[num]`.
        let juri = Decision {
            juridiction: Some("Cour de cassation".to_owned()),
            formation: Some("FORMATION_PLENIERE".to_owned()),
            solution: Some("Cassation".to_owned()),
            numero_affaire: vec!["24-13.470".to_owned()],
            nature: Some("ARRET".to_owned()),
            date_texte: CivilDate::new(2025, 6, 4),
            ..bare(JURI)
        };
        let actual = view(juri);
        assert_eq!(actual["formation"], "FORMATION_PLENIERE");
        assert_eq!(actual["numero_affaire"], json!(["24-13.470"]));
        assert_eq!(actual["date"], "2025-06-04");
        let cetat = Decision {
            formation: Some("2ème - 7ème chambres réunies".to_owned()),
            numero_affaire: vec!["506507".to_owned()],
            ..bare(CETAT)
        };
        let actual = view(cetat);
        assert_eq!(actual["formation"], "2ème - 7ème chambres réunies");
        assert_eq!(actual["numero_affaire"], json!(["506507"]));
    }

    #[test]
    fn sentinel_dates_are_omitted() {
        // 2222-02-22 et 2999-01-01 valent « pas de date » (03 §6.2).
        for (year, month, day) in [(2222, 2, 22), (2999, 1, 1)] {
            let decision = Decision { date_texte: CivilDate::new(year, month, day), ..bare(JURI) };
            assert!(view(decision).get("date").is_none());
        }
    }

    #[test]
    fn the_title_is_omitted_when_the_citation_is_the_title() {
        // JURI et CETAT : la citation est le titre nettoyé (05 §5.0.4).
        for (id, title) in [
            (
                JURI,
                "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin",
            ),
            (CETAT, "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507"),
        ] {
            let decision = Decision { title: Some(title.to_owned()), ..bare(id) };
            let actual = view(decision);
            assert!(actual.get("title").is_none(), "{id}");
            assert_eq!(actual["citation"], title);
        }
        // Conseil constitutionnel : le titre porte l'objet, que la citation n'a pas.
        assert_eq!(view(constit())["title"], CONSTIT_TITLE);
    }

    #[test]
    fn a_title_longer_than_the_citation_cap_is_rendered() {
        let long = format!("Décision {}", "très longue ".repeat(120));
        let decision = Decision { title: Some(long.clone()), ..bare(JURI) };
        let actual = view(decision);
        assert!(actual["citation"].as_str().unwrap().chars().count() <= 1_000);
        assert_eq!(actual["title"], long.trim());
    }

    #[test]
    fn the_reading_fields_follow_the_metadata() {
        let window = Reading {
            text: Some("Première page. ".to_owned()),
            next_offset: Some(15),
            note: Some("n".to_owned()),
            ..reading(40)
        };
        assert_written_in_order(
            &DecisionView::from(output(bare(JURI), window)),
            &[
                "decision_id",
                "citation",
                "source_url",
                "text",
                "text_length",
                "next_offset",
                "note",
            ],
        );
        let passages = Reading {
            passages: Some(vec![PassageOut {
                offset: 11_008,
                text: "14. Dès lors.".to_owned(),
                score: 1.0,
            }]),
            ..reading(14_332)
        };
        let actual =
            serde_json::to_value(DecisionView::from(output(bare(JURI), passages))).unwrap();
        assert_eq!(
            actual["passages"],
            json!([{"offset": 11_008, "text": "14. Dès lors.", "score": 1.0}])
        );
        assert_eq!(actual["text_length"], 14_332);
    }

    #[test]
    fn a_window_rich_in_escapes_is_shortened_under_the_hard_cap() {
        // Guillemets, antislashs et emoji : l'estimation de l'application ne voit pas les
        // échappements, la mesure exacte de la vue raccourcit la fenêtre (05 §6.4, point 4).
        let text = "\"\\😀 ".repeat(10_000);
        let window = Reading { text: Some(text.clone()), ..reading(40_000) };
        let actual = DecisionView::from(output(constit(), window));
        let size = serde_json::to_string(&actual).unwrap().chars().count();
        assert!(size <= 45_000, "{size} caractères");
        let kept = actual.reading.text.as_deref().unwrap();
        assert!(kept.len() < text.len() && text.starts_with(kept));
        assert_eq!(
            actual.reading.next_offset,
            Some(u32::try_from(kept.chars().count()).unwrap()),
            "next_offset reste exact"
        );
    }

    #[test]
    fn the_output_schema_requires_the_four_always_present_fields() {
        let schema = Value::Object(output_schema::<DecisionView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        let mut required: Vec<_> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect();
        required.sort();
        assert_eq!(required, ["citation", "decision_id", "source_url", "text_length"]);
        assert!(schema.get("$schema").is_none());
        // Aucun champ ne se déclare `null` : les champs absents sont omis.
        assert!(!schema.to_string().contains("\"null\""), "{schema}");
        // La sortie reste ouverte (05 §2.5).
        assert!(schema.get("additionalProperties").is_none());
    }

    #[test]
    fn the_output_schema_snapshot() {
        let schema = Value::Object(output_schema::<DecisionView>().as_ref().clone());
        // Règle S : les snapshots vivent dans `src/snapshots/`, pas dans `src/views/snapshots/`.
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }

    #[test]
    fn the_three_reading_modes_match_their_snapshots() {
        let to_value =
            |reading| serde_json::to_value(DecisionView::from(output(constit(), reading))).unwrap();
        let window = Reading {
            text: Some(
                "Vu la Constitution ;\n\nVu l’ordonnance n° 58-1067 du 7 novembre 1958 ;"
                    .to_owned(),
            ),
            next_offset: Some(68),
            ..reading(14_332)
        };
        let passages = Reading {
            passages: Some(vec![
                PassageOut {
                    offset: 11_008,
                    text: "14. Dès lors, le grief tiré de la méconnaissance du principe de publicité des débats du procès pénal ne peut qu’être écarté.".to_owned(),
                    score: 3.12,
                },
                PassageOut { offset: 7_016, text: "4. Le requérant reproche à ces dispositions […]".to_owned(), score: 2.04 },
            ]),
            ..reading(14_332)
        };
        // Règle S : les snapshots vivent dans `src/snapshots/`, pas dans `src/views/snapshots/`.
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("constit_summary", to_value(summary_reading()));
            insta::assert_json_snapshot!("constit_full_page", to_value(window));
            insta::assert_json_snapshot!("constit_focus", to_value(passages));
        });
    }
}
