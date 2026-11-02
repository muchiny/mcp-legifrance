//! Vue de sortie de `fr_resolve` (05 §5.10) : `ResolveView`.
//!
//! Construite depuis le [`ResolveOutput`] de l'application : `interpreted_as?` (la forme canonique
//! de la citation lue), `candidates` (toujours, au plus cinq, le meilleur d'abord, chacun avec son
//! `id`, sa famille, sa citation, l'URL de la page et sa confiance) et `note?` (les notes de
//! l'application rendues en anglais et jointes par une espace).
//!
//! La mesure exacte du JSON compact suit la construction (`views::fit`) : la réponse tient
//! largement sous le plafond dur de 45 000 caractères avec cinq candidats aux bornes du domaine,
//! mais la vue retire encore des candidats par la fin si le plafond était dépassé, en le disant
//! dans `note`. Les commentaires des champs sont des `//` : un doc comment partirait dans
//! l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::{
    citation::{Candidate, Note},
    output::resolve::ResolveOutput,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::{common::ValidityFields, fit::fit_under_cap};

/// Note ajoutée quand la vue doit retirer des candidats pour tenir sous le plafond.
const FURTHER_NOTE: &str = "Candidates were removed to keep the response under 45,000 characters.";

/// Ce que `fr_resolve` rend : la lecture de la citation et les enregistrements possibles.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ResolveView {
    // Forme canonique de la citation reconnue, par exemple « Code civil, art. 9 ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    interpreted_as: Option<String>,
    // Au plus cinq enregistrements, le plus sûr d'abord ; vide si rien n'a été trouvé.
    candidates: Vec<CandidateView>,
    // Remarque : motif d'une liste vide, écart, autres citations de l'entrée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    note: Option<String>,
}

// Un enregistrement qui pourrait être celui que la citation désigne.
#[derive(Debug, Serialize, JsonSchema)]
struct CandidateView {
    id: String,
    // article, loda, code, juri, constit, cetat, cnil, kali ou acco.
    kind: String,
    citation: String,
    source_url: String,
    // high, medium ou low.
    confidence: String,
    // Article seulement : état de la version lue (VIGUEUR, MODIFIE…) et ses dates (D-03). Les
    // descriptions (D-12) renvoient au glossaire des instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "In force: VIGUEUR*, ABROGE_DIFF; other codes not.")]
    etat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "First day of this version, YYYY-MM-DD.")]
    date_debut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Day the next version (or the repeal) takes effect; absent: no known end."
    )]
    date_fin: Option<String>,
}

impl From<ResolveOutput> for ResolveView {
    /// Vue de la réponse, ramenée sous le plafond dur de 45 000 caractères.
    fn from(output: ResolveOutput) -> Self {
        let ResolveOutput { interpreted_as, candidates, notes } = output;
        let note =
            (!notes.is_empty()).then(|| notes.iter().map(Note::text).collect::<Vec<_>>().join(" "));
        let mut view = Self {
            interpreted_as,
            candidates: candidates.into_iter().map(CandidateView::from).collect(),
            note,
        };
        view.fit();
        view
    }
}

impl ResolveView {
    /// Mesure la vue et retire des candidats par la fin tant qu'elle dépasse le plafond dur
    /// (05 §6.4) ; une note le dit.
    fn fit(&mut self) {
        fit_under_cap(self, |view| {
            if view.candidates.pop().is_none() {
                return false;
            }
            view.note = Some(match view.note.take() {
                Some(note) if !note.contains(FURTHER_NOTE) => format!("{note} {FURTHER_NOTE}"),
                Some(note) => note,
                None => FURTHER_NOTE.to_owned(),
            });
            true
        });
    }
}

impl From<Candidate> for CandidateView {
    fn from(candidate: Candidate) -> Self {
        let ValidityFields { etat, date_debut, date_fin } = candidate.record.validity.into();
        Self {
            id: candidate.record.id.as_str().to_owned(),
            kind: candidate.kind.as_str().to_owned(),
            citation: candidate.record.citation,
            source_url: candidate.record.source_url,
            confidence: candidate.confidence.as_str().to_owned(),
            etat,
            date_debut,
            date_fin,
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::citation::{CandidateKind, ResolvedRecord, Validity};
    use lf_domain::{citation::Confidence, date::CivilDate, id::DocumentId, status::LegalStatus};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    fn candidate(id: &str, citation: &str, confidence: Confidence) -> Candidate {
        let document = DocumentId::parse(id).unwrap();
        let page = match CandidateKind::of(&document) {
            CandidateKind::Article => "codes/article_lc",
            CandidateKind::Juri => "juri/id",
            _ => "loda/id",
        };
        Candidate {
            kind: CandidateKind::of(&document),
            record: ResolvedRecord {
                source_url: format!("https://www.legifrance.gouv.fr/{page}/{}", document.as_str()),
                id: document,
                citation: citation.to_owned(),
                ecli: None,
                validity: None,
            },
            confidence,
        }
    }

    fn article_9() -> Candidate {
        candidate("LEGIARTI000006419288", "Code civil, art. 9", Confidence::High)
    }

    fn output(candidates: Vec<Candidate>, notes: Vec<Note>) -> ResolveOutput {
        ResolveOutput { interpreted_as: Some("Code civil, art. 9".to_owned()), candidates, notes }
    }

    fn json_of(output: ResolveOutput) -> Value {
        serde_json::to_value(ResolveView::from(output)).unwrap()
    }

    #[test]
    fn the_documented_example_renders_field_by_field() {
        // 05 §5.10 : `{"citation": "art. 9 c. civ."}`.
        assert_eq!(
            json_of(output(vec![article_9()], vec![])),
            json!({
                "interpreted_as": "Code civil, art. 9",
                "candidates": [{
                    "id": "LEGIARTI000006419288",
                    "kind": "article",
                    "citation": "Code civil, art. 9",
                    "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                    "confidence": "high"
                }]
            })
        );
    }

    #[test]
    fn the_documented_example_matches_its_snapshot() {
        let view = output(
            vec![
                article_9(),
                candidate("LEGITEXT000006070721", "Code civil", Confidence::Low),
                candidate(
                    "JURITEXT000051743650",
                    "Cour de cassation, 4 juin 2025",
                    Confidence::Medium,
                ),
            ],
            vec![Note::OtherCitations { count: 2 }],
        );
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("the_documented_example", json_of(view));
        });
    }

    #[test]
    fn an_empty_answer_keeps_the_candidates_list_and_says_why() {
        let empty = ResolveOutput {
            interpreted_as: None,
            candidates: vec![],
            notes: vec![Note::NoCitationRecognised],
        };
        assert_eq!(
            json_of(empty),
            json!({"candidates": [], "note": Note::NoCitationRecognised.text()})
        );
    }

    #[test]
    fn the_notes_are_rendered_and_joined_in_order() {
        let notes = vec![
            Note::VersionSince(lf_domain::date::CivilDate::new(2016, 10, 1).unwrap()),
            Note::OtherCitations { count: 1 },
        ];
        let value = json_of(output(vec![article_9()], notes));
        assert_eq!(
            value["note"],
            "Current version in force since 2016-10-01. 1 more citation(s) found; only the \
             first is resolved, use fr_verify_citations for the whole text."
        );
    }

    fn article_with(validity: Validity) -> Candidate {
        let mut candidate = article_9();
        candidate.record.validity = Some(validity);
        candidate
    }

    fn period(
        etat: Option<LegalStatus>,
        debut: Option<CivilDate>,
        fin: Option<CivilDate>,
    ) -> Validity {
        Validity { etat, date_debut: debut, date_fin: fin }
    }

    /// D-03 (journey_patrimoine-14) : article 779 du CGI lu au 2011-06-01, version MODIFIE du
    /// 2010-05-01 au 2011-06-12 (valeurs de la réponse réelle, reconstituées).
    #[test]
    fn a_candidate_article_renders_its_validity_period() {
        let candidate = article_with(period(
            Some(LegalStatus::Modifie),
            CivilDate::new(2010, 5, 1),
            CivilDate::new(2011, 6, 12),
        ));
        assert_eq!(
            json_of(output(vec![candidate], vec![])),
            json!({
                "interpreted_as": "Code civil, art. 9",
                "candidates": [{
                    "id": "LEGIARTI000006419288",
                    "kind": "article",
                    "citation": "Code civil, art. 9",
                    "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                    "confidence": "high",
                    "etat": "MODIFIE",
                    "date_debut": "2010-05-01",
                    "date_fin": "2011-06-12"
                }]
            })
        );
    }

    /// Jumeau : la version en vigueur n'a pas de fin (absente ou sentinelle 2999), le champ est
    /// omis ; sans période (décision, texte), aucun des trois champs n'apparaît.
    #[test]
    fn an_open_ended_or_missing_period_is_omitted() {
        for fin in [None, CivilDate::new(2999, 1, 1)] {
            let candidate =
                article_with(period(Some(LegalStatus::Vigueur), CivilDate::new(2013, 1, 1), fin));
            let value = json_of(output(vec![candidate], vec![]));
            let rendered = &value["candidates"][0];
            assert_eq!(rendered["etat"], "VIGUEUR");
            assert_eq!(rendered["date_debut"], "2013-01-01");
            assert!(rendered.get("date_fin").is_none(), "{rendered}");
        }
        // Un début sentinelle ou absent, un état absent : champs omis un à un.
        let candidate = article_with(period(None, None, CivilDate::new(2011, 6, 12)));
        let value = json_of(output(vec![candidate], vec![]));
        let rendered = value["candidates"][0].as_object().unwrap();
        assert!(!rendered.contains_key("etat") && !rendered.contains_key("date_debut"));
        assert_eq!(rendered["date_fin"], "2011-06-12");
        let decision = candidate_of_decision();
        let value = json_of(output(vec![decision], vec![]));
        let rendered = value["candidates"][0].as_object().unwrap();
        assert!(!rendered.contains_key("etat") && !rendered.contains_key("date_debut"));
        assert!(!rendered.contains_key("date_fin"));
    }

    fn candidate_of_decision() -> Candidate {
        candidate("JURITEXT000051743650", "Cour de cassation, 4 juin 2025", Confidence::Medium)
    }

    #[test]
    fn the_view_with_a_period_validates_against_its_output_schema() {
        let schema = Value::Object(output_schema::<ResolveView>().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();
        let dated = article_with(period(
            Some(LegalStatus::Modifie),
            CivilDate::new(2010, 5, 1),
            CivilDate::new(2011, 6, 12),
        ));
        let value = json_of(output(vec![dated, candidate_of_decision()], vec![]));
        assert!(validator.is_valid(&value), "{value}");
    }

    #[test]
    fn every_kind_and_confidence_has_its_wire_name() {
        let ids = [
            ("LEGIARTI000006419288", "article"),
            ("LEGITEXT000006070721", "loda"),
            ("KALITEXT000005635096", "kali"),
            ("JURITEXT000051743650", "juri"),
            ("CONSTEXT000052555275", "constit"),
            ("CETATEXT000054178503", "cetat"),
            ("CNILTEXT000053352643", "cnil"),
            ("ACCOTEXT000045000001", "acco"),
        ];
        let candidates = ids.iter().map(|(id, _)| candidate(id, "x", Confidence::Low)).collect();
        let value = json_of(output(candidates, vec![]));
        let kinds: Vec<&str> = value["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ids.map(|(_, kind)| kind));
        let confidences: Vec<&str> = [Confidence::High, Confidence::Medium, Confidence::Low]
            .iter()
            .map(|confidence| confidence.as_str())
            .collect();
        assert_eq!(confidences, ["high", "medium", "low"]);
        let rendered = json_of(output(
            vec![
                candidate("LEGITEXT000006070721", "x", Confidence::Medium),
                candidate("LEGITEXT000006070721", "x", Confidence::Low),
            ],
            vec![],
        ));
        assert_eq!(rendered["candidates"][0]["confidence"], "medium");
        assert_eq!(rendered["candidates"][1]["confidence"], "low");
    }

    #[test]
    fn a_response_over_the_hard_cap_loses_candidates_from_the_end_and_says_so() {
        // Cinq candidats dont la citation, riche en échappements, pèse bien plus de 45 000
        // caractères de JSON : les derniers partent, la note le dit.
        let heavy = |n: u32| {
            candidate(
                &format!("LEGITEXT{n:012}"),
                &format!("{n}{}", "\"\\\u{1}\u{1F600}".repeat(2_000)),
                Confidence::Low,
            )
        };
        let candidates: Vec<Candidate> = (1..=5).map(heavy).collect();
        let view = ResolveView::from(output(candidates, vec![Note::NoCitationRecognised]));
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.chars().count() <= 45_000);
        let value: Value = serde_json::from_str(&json).unwrap();
        let kept = value["candidates"].as_array().unwrap().len();
        assert!((1..5).contains(&kept), "{kept}");
        assert_eq!(
            value["note"],
            format!("{} {FURTHER_NOTE}", Note::NoCitationRecognised.text()),
            "la note existante est gardée, celle du plafond s'y ajoute"
        );
        // Les candidats gardés sont les premiers.
        assert!(value["candidates"][0]["citation"].as_str().unwrap().starts_with('1'));
    }

    #[test]
    fn the_cap_note_is_not_repeated_when_the_last_candidates_are_removed() {
        // Un seul candidat démesuré : le retrait le viderait, la note du plafond est posée une fois.
        let giant = candidate("LEGITEXT000006070721", &"y".repeat(50_000), Confidence::Low);
        let value = json_of(output(vec![giant], vec![]));
        assert_eq!(value["candidates"], json!([]));
        assert_eq!(value["note"], FURTHER_NOTE);
        // Deux candidats démesurés : le premier retrait pose la note, le second ne la répète pas.
        let giants = (1..=2)
            .map(|n| candidate(&format!("LEGITEXT{n:012}"), &"y".repeat(50_000), Confidence::Low))
            .collect();
        let value = json_of(output(giants, vec![]));
        assert_eq!(value["candidates"], json!([]));
        assert_eq!(value["note"], FURTHER_NOTE);
    }

    #[test]
    fn an_irreducible_view_is_sent_as_it_is() {
        // Aucun candidat à retirer et la vue dépasse le plafond (la citation lue est démesurée) :
        // la réponse part telle quelle, sans note de plafond (rien n'a été retiré).
        let irreducible = ResolveOutput {
            interpreted_as: Some("y".repeat(50_000)),
            candidates: vec![],
            notes: vec![],
        };
        let value = json_of(irreducible);
        assert_eq!(value["candidates"], json!([]));
        assert_eq!(value["interpreted_as"].as_str().map(str::len), Some(50_000));
        assert!(value.get("note").is_none(), "{value}");
    }

    /// D-12 : les trois champs de validité d'un candidat disent ce qu'ils sont (D-03).
    #[test]
    fn the_validity_fields_of_a_candidate_are_described() {
        let schema = Value::Object(output_schema::<ResolveView>().as_ref().clone());
        let props = &schema["$defs"]["CandidateView"]["properties"];
        assert!(
            props["etat"]["description"]
                .as_str()
                .unwrap()
                .contains("In force: VIGUEUR*, ABROGE_DIFF;"),
            "{props}"
        );
        assert!(props["date_debut"]["description"].as_str().unwrap().contains("First day"));
        assert!(
            props["date_fin"]["description"]
                .as_str()
                .unwrap()
                .contains("(or the repeal) takes effect; absent: no known end"),
            "{props}"
        );
    }

    #[test]
    fn output_schema_snapshot() {
        let schema = Value::Object(output_schema::<ResolveView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["candidates"]));
        assert!(schema.get("$schema").is_none());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
