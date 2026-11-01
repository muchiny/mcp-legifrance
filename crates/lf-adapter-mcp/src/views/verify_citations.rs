//! Vue de sortie de `fr_verify_citations` (05 §5.9) : `VerifyView`.
//!
//! Construite depuis le [`VerifyCitationsOutput`] de l'application : `found`, `checked`,
//! `truncated?` (vrai seulement si `found > checked`), `citations` (toujours) et, exceptionnellement,
//! une `note` quand la liste a été raccourcie. Chaque citation porte son `span` en points de code
//! Unicode (déjà converti par le cas d'usage), l'extrait d'origine, sa famille en `snake_case`, son
//! statut, l'enregistrement officiel trouvé, le verdict sur l'extrait cité et la note rendue en
//! anglais ([`Note::text`]).
//!
//! La mesure exacte du JSON compact suit la construction (`views::fit`) : l'estimation de
//! l'application ignore les échappements JSON, et la vue retire encore des citations par la fin si
//! le plafond dur est dépassé, en le disant dans `note`. Les commentaires des champs sont des
//! `//` : un doc comment partirait dans l'`outputSchema`, dont la taille compte (05 §3.4).

use lf_application::{
    citation::{CitationCheck, ResolvedRecord},
    output::verify_citations::VerifyCitationsOutput,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::{common::ValidityFields, fit::fit_under_cap};

/// Note ajoutée quand la vue doit encore raccourcir la liste après l'estimation de l'application.
const FURTHER_NOTE: &str = "List shortened to keep the response under 45,000 characters; call \
                            again on the rest of the text.";

/// Ce que `fr_verify_citations` rend : le contrôle de chaque citation, dans l'ordre du texte.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct VerifyView {
    // Citations reconnues dans le texte.
    found: usize,
    // Citations contrôlées et rendues (au plus `max_citations`).
    checked: usize,
    // Vrai si `found > checked` : rappeler sur la suite du texte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool", description = "True if found > checked.")]
    truncated: Option<bool>,
    // Un contrôle par citation, toujours présent.
    citations: Vec<CitationView>,
    // Notes de la réponse : liste raccourcie par la mesure exacte du JSON (`truncated`), aucune
    // citation reconnue, passage ou subdivision non contrôlés. Absente quand rien n'est à dire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    note: Option<String>,
}

// Le contrôle d'une citation.
#[derive(Debug, Serialize, JsonSchema)]
struct CitationView {
    // Position dans le texte reçu, en points de code Unicode, intervalle semi-ouvert.
    span: SpanView,
    // Extrait d'origine.
    raw: String,
    // Famille : code_article, text_article, text, decision, ecli, cnil_deliberation, convention,
    // jorf, nor, eli, legifrance_id ou unsupported.
    kind: String,
    // verified, not_found, ambiguous, unresolvable ou invalid.
    status: String,
    // Enregistrement officiel trouvé, y compris quand il contredit la citation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ResolvedView")]
    resolved: Option<ResolvedView>,
    // match ou quote_mismatch, si un extrait entre guillemets jouxte la citation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    quote: Option<String>,
    // Explication actionnable : écart, candidats, motif de non-résolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    note: Option<String>,
}

// Intervalle semi-ouvert `[start, end)`.
#[derive(Debug, Serialize, JsonSchema)]
struct SpanView {
    start: usize,
    end: usize,
}

// L'enregistrement officiel d'une citation.
#[derive(Debug, Serialize, JsonSchema)]
struct ResolvedView {
    id: String,
    citation: String,
    source_url: String,
    // Article seulement : état de la version lue (VIGUEUR, MODIFIE…) et ses dates (D-03).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    etat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date_debut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    date_fin: Option<String>,
}

impl From<VerifyCitationsOutput> for VerifyView {
    /// Vue de la réponse, ramenée sous le plafond dur de 45 000 caractères.
    fn from(output: VerifyCitationsOutput) -> Self {
        let mut view = Self::build(output);
        view.fit();
        view
    }
}

impl VerifyView {
    /// Vue de `output`, sans mesure du plafond.
    fn build(output: VerifyCitationsOutput) -> Self {
        let VerifyCitationsOutput { found, checked, truncated, citations, note } = output;
        Self {
            found,
            checked,
            truncated: truncated.then_some(true),
            citations: citations.into_iter().map(CitationView::from).collect(),
            note,
        }
    }

    /// Mesure la vue et retire des citations par la fin tant qu'elle dépasse le plafond dur
    /// (05 §5.9, 05 §6.4). `checked` suit la liste, `truncated` passe à vrai, une note le dit.
    fn fit(&mut self) {
        fit_under_cap(self, |view| {
            if view.citations.pop().is_none() {
                return false;
            }
            view.checked = view.citations.len();
            view.truncated = Some(true);
            view.note = Some(match view.note.take() {
                Some(note) if !note.contains(FURTHER_NOTE) => format!("{note} {FURTHER_NOTE}"),
                Some(note) => note,
                None => FURTHER_NOTE.to_owned(),
            });
            true
        });
    }
}

impl From<CitationCheck> for CitationView {
    fn from(check: CitationCheck) -> Self {
        Self {
            span: SpanView { start: check.span.start, end: check.span.end },
            raw: check.raw,
            kind: check.kind.snake_case().to_owned(),
            status: check.status.as_str().to_owned(),
            resolved: check.resolved.map(ResolvedView::from),
            quote: check.quote.map(|quote| quote.as_str().to_owned()),
            note: check.note.map(|note| note.text()),
        }
    }
}

impl From<ResolvedRecord> for ResolvedView {
    fn from(record: ResolvedRecord) -> Self {
        let ValidityFields { etat, date_debut, date_fin } = record.validity.into();
        Self {
            id: record.id.as_str().to_owned(),
            citation: record.citation,
            source_url: record.source_url,
            etat,
            date_debut,
            date_fin,
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::citation::{CheckStatus, Note, QuoteCheck, Validity};
    use lf_domain::{
        citation::{CitationKind, Span},
        date::CivilDate,
        id::DocumentId,
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::output_schema;

    fn record() -> ResolvedRecord {
        ResolvedRecord {
            id: DocumentId::parse("LEGIARTI000006419288").unwrap(),
            citation: "Code civil, art. 9".to_owned(),
            source_url: "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
                .to_owned(),
            ecli: None,
            validity: None,
        }
    }

    fn verified() -> CitationCheck {
        CitationCheck {
            span: Span { start: 8, end: 31 },
            raw: "article 9 du code civil".to_owned(),
            kind: CitationKind::CodeArticle,
            status: CheckStatus::Verified,
            resolved: Some(record()),
            quote: Some(QuoteCheck::Match),
            note: None,
        }
    }

    fn not_found() -> CitationCheck {
        CitationCheck {
            span: Span { start: 253, end: 279 },
            raw: "article 9999 du code civil".to_owned(),
            kind: CitationKind::CodeArticle,
            status: CheckStatus::NotFound,
            resolved: None,
            quote: None,
            note: Some(Note::NotInForce {
                code: "Code civil".to_owned(),
                num: "9999".to_owned(),
                date: lf_domain::date::VersionDate::parse("2026-10-15").unwrap(),
            }),
        }
    }

    fn output(citations: Vec<CitationCheck>, found: usize) -> VerifyCitationsOutput {
        VerifyCitationsOutput {
            found,
            checked: citations.len(),
            truncated: found > citations.len(),
            citations,
            note: None,
        }
    }

    fn json_of(output: VerifyCitationsOutput) -> Value {
        serde_json::to_value(VerifyView::from(output)).unwrap()
    }

    #[test]
    fn the_documented_example_renders_field_by_field() {
        let view = json_of(output(vec![verified(), not_found()], 2));
        assert_eq!(
            view,
            json!({
                "found": 2,
                "checked": 2,
                "citations": [
                    {
                        "span": {"start": 8, "end": 31},
                        "raw": "article 9 du code civil",
                        "kind": "code_article",
                        "status": "verified",
                        "resolved": {
                            "id": "LEGIARTI000006419288",
                            "citation": "Code civil, art. 9",
                            "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
                        },
                        "quote": "match"
                    },
                    {
                        "span": {"start": 253, "end": 279},
                        "raw": "article 9999 du code civil",
                        "kind": "code_article",
                        "status": "not_found",
                        "note": "Code civil has no article 9999 in force on 2026-10-15. The article may exist in earlier versions; retry with a date."
                    }
                ]
            })
        );
    }

    fn with_validity(validity: Validity) -> CitationCheck {
        let mut check = verified();
        check.resolved.as_mut().unwrap().validity = Some(validity);
        check
    }

    /// D-03 : `resolved` d'un article porte la période de la version lue ; la fin sentinelle 2999
    /// et les champs absents sont omis, une décision (sans période) n'en porte aucun.
    #[test]
    fn a_resolved_article_renders_its_validity_period() {
        let dated = with_validity(Validity {
            etat: Some(LegalStatus::Modifie),
            date_debut: CivilDate::new(2010, 5, 1),
            date_fin: CivilDate::new(2011, 6, 12),
        });
        let open = with_validity(Validity {
            etat: Some(LegalStatus::Vigueur),
            date_debut: CivilDate::new(2013, 1, 1),
            date_fin: CivilDate::new(2999, 1, 1),
        });
        let value = json_of(output(vec![dated, open, verified()], 3));
        assert_eq!(
            value["citations"][0]["resolved"],
            json!({
                "id": "LEGIARTI000006419288",
                "citation": "Code civil, art. 9",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "etat": "MODIFIE",
                "date_debut": "2010-05-01",
                "date_fin": "2011-06-12"
            })
        );
        assert_eq!(
            value["citations"][1]["resolved"],
            json!({
                "id": "LEGIARTI000006419288",
                "citation": "Code civil, art. 9",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "etat": "VIGUEUR",
                "date_debut": "2013-01-01"
            })
        );
        let plain = value["citations"][2]["resolved"].as_object().unwrap();
        assert_eq!(plain.len(), 3, "{plain:?}");
    }

    #[test]
    fn the_view_with_a_period_validates_against_its_output_schema() {
        let schema = Value::Object(output_schema::<VerifyView>().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();
        let dated = with_validity(Validity {
            etat: Some(LegalStatus::Modifie),
            date_debut: CivilDate::new(2010, 5, 1),
            date_fin: CivilDate::new(2011, 6, 12),
        });
        let value = json_of(output(vec![dated, verified()], 2));
        assert!(validator.is_valid(&value), "{value}");
    }

    /// D-03, budget : cent citations avec extrait, enregistrement et note (le test de volume de
    /// gap1-5) tiennent encore sous le plafond avec la période de chaque article, sans coupe ; le
    /// surcoût mesuré est borné par la taille des trois champs.
    #[test]
    fn a_hundred_dated_citations_still_fit_under_the_cap_and_the_overhead_is_small() {
        let entry = |n: usize, dated: bool| {
            let mut check = CitationCheck {
                raw: format!("article {n} du code civil, selon lequel « Texte. »"),
                quote: (n < 30).then_some(QuoteCheck::Match),
                note: Some(Note::NoRecord { family: "text", value: "é".repeat(40), date: None }),
                ..verified()
            };
            if dated {
                check.resolved.as_mut().unwrap().validity = Some(Validity {
                    etat: Some(LegalStatus::Modifie),
                    date_debut: CivilDate::new(2010, 5, 1),
                    date_fin: CivilDate::new(2011, 6, 12),
                });
            }
            check
        };
        let size = |dated: bool| {
            let citations: Vec<CitationCheck> = (0..100).map(|n| entry(n, dated)).collect();
            let json = serde_json::to_string(&VerifyView::from(output(citations, 100))).unwrap();
            let value: Value = serde_json::from_str(&json).unwrap();
            assert_eq!(value["citations"].as_array().unwrap().len(), 100, "aucune coupe");
            assert!(value.get("truncated").is_none() && value.get("note").is_none());
            json.chars().count()
        };
        let (plain, dated) = (size(false), size(true));
        assert!(dated <= 45_000, "{dated}");
        // Cas le plus lourd (MODIFIE, deux dates) : 67 caractères par article résolu (le plan
        // en prévoyait 75) ; une version en vigueur sans fin en coûte une quarantaine.
        assert_eq!(dated - plain, 100 * 67, "surcoût par article ; plain={plain} dated={dated}");
    }

    #[test]
    fn the_documented_example_matches_its_snapshot() {
        let mut view = output(vec![verified(), not_found()], 3);
        view.note = Some("List shortened to the first 2 citations.".to_owned());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("the_documented_example", json_of(view));
        });
    }

    #[test]
    fn truncated_appears_only_when_citations_were_left_out() {
        let full = json_of(output(vec![verified()], 1));
        assert!(full.get("truncated").is_none() && full.get("note").is_none());
        let cut = json_of(output(vec![verified()], 3));
        assert_eq!(cut["truncated"], true);
        assert_eq!((cut["found"].clone(), cut["checked"].clone()), (json!(3), json!(1)));
        let empty = json_of(output(vec![], 0));
        assert_eq!(empty["citations"], json!([]));
    }

    #[test]
    fn the_note_of_the_application_is_rendered() {
        let mut shortened = output(vec![verified()], 9);
        shortened.note = Some("List shortened to the first 1 citations.".to_owned());
        assert_eq!(json_of(shortened)["note"], "List shortened to the first 1 citations.");
    }

    #[test]
    fn a_response_over_the_hard_cap_loses_citations_from_the_end_and_says_so() {
        // 100 citations de 300 caractères d'extrait et d'une note : bien au-delà de 45 000.
        let heavy = |n: usize| CitationCheck {
            raw: format!("{n} {}", "x".repeat(300)),
            note: Some(Note::NoRecord { family: "text", value: "é".repeat(150), date: None }),
            ..verified()
        };
        let citations: Vec<CitationCheck> = (0..100).map(heavy).collect();
        let view = VerifyView::from(output(citations, 100));
        let json = serde_json::to_string(&view).unwrap();
        let length = json.chars().count();
        assert!(length <= 45_000, "{length}");
        let value: Value = serde_json::from_str(&json).unwrap();
        let kept = value["citations"].as_array().unwrap().len();
        assert!(kept > 0 && kept < 100, "{kept}");
        assert_eq!(value["checked"], kept);
        assert_eq!(value["found"], 100);
        assert_eq!(value["truncated"], true);
        assert!(value["note"].as_str().unwrap().contains("45,000"));
        // Les citations gardées sont les premières.
        assert!(value["citations"][0]["raw"].as_str().unwrap().starts_with("0 "));
    }

    #[test]
    fn a_hundred_citations_with_quotes_fit_under_the_cap_without_shortening_gap1_5() {
        // Complément de vue du cas d'usage `max_citations_100_is_reached_with_quotes_gap1_5`
        // (lf-application), qui prouve la correction de L2-13 : ce test-ci réussissait déjà avant,
        // il fixe seulement que la vue ne coupe pas à la taille réelle de gap1-5.
        // Cas réel de gap1-5 : cent citations dont trente suivies d'un extrait, l'enregistrement et
        // une note chacune (environ 400 caractères de JSON). Seule la mesure exacte coupe : ici
        // rien n'est retiré, `max_citations=100` est atteint.
        let entry = |n: usize| CitationCheck {
            raw: format!("article {n} du code civil, selon lequel « Texte. »"),
            quote: (n < 30).then_some(QuoteCheck::Match),
            note: Some(Note::NoRecord { family: "text", value: "é".repeat(40), date: None }),
            ..verified()
        };
        let citations: Vec<CitationCheck> = (0..100).map(entry).collect();
        let json = serde_json::to_string(&VerifyView::from(output(citations, 100))).unwrap();
        assert!(json.chars().count() <= 45_000);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["citations"].as_array().unwrap().len(), 100);
        assert_eq!(value["checked"], 100);
        assert!(value.get("truncated").is_none() && value.get("note").is_none());
    }

    #[test]
    fn the_note_of_the_application_comes_first_when_the_view_shortens_the_list_again() {
        let heavy = |n: usize| CitationCheck {
            raw: format!("{n} {}", "x".repeat(300)),
            note: Some(Note::NoRecord { family: "text", value: "é".repeat(150), date: None }),
            ..verified()
        };
        let citations: Vec<CitationCheck> = (0..100).map(heavy).collect();
        let mut over = output(citations, 100);
        let application_note = "List shortened to the first 100 citations.";
        over.note = Some(application_note.to_owned());
        let value = json_of(over);
        // Le retrait a eu lieu plusieurs fois, la note de la vue n'est ajoutée qu'une fois.
        assert!(value["citations"].as_array().unwrap().len() < 100);
        assert_eq!(value["note"], format!("{application_note} {FURTHER_NOTE}"));
    }

    #[test]
    fn an_empty_list_over_the_cap_has_nothing_to_remove_and_keeps_its_note() {
        // Aucune citation à retirer : le plafond reste dépassé par la seule note, qui part telle
        // quelle, sans `truncated` ni note de la vue.
        let mut over = output(vec![], 0);
        let long_note = "z".repeat(50_000);
        over.note = Some(long_note.clone());
        let value = json_of(over);
        assert_eq!(value["citations"], json!([]));
        assert_eq!(value["checked"], 0);
        assert_eq!(value["note"], long_note);
        assert!(value.get("truncated").is_none());
    }

    #[test]
    fn a_view_that_cannot_be_reduced_is_sent_as_it_is() {
        // Une seule citation démesurée : le retrait la viderait, la liste devient vide.
        let giant = CitationCheck { raw: "y".repeat(50_000), ..verified() };
        let value = json_of(output(vec![giant], 1));
        assert_eq!(value["citations"], json!([]));
        assert_eq!(value["checked"], 0);
        assert_eq!(value["truncated"], true);
    }

    #[test]
    fn every_status_and_quote_has_its_wire_name() {
        let mut statuses = Vec::new();
        for status in [
            CheckStatus::Verified,
            CheckStatus::NotFound,
            CheckStatus::Ambiguous,
            CheckStatus::Unresolvable,
            CheckStatus::Invalid,
        ] {
            let check =
                CitationCheck { status, quote: Some(QuoteCheck::QuoteMismatch), ..verified() };
            let value = json_of(output(vec![check], 1));
            statuses.push(value["citations"][0]["status"].as_str().unwrap().to_owned());
            assert_eq!(value["citations"][0]["quote"], "quote_mismatch");
        }
        assert_eq!(statuses, ["verified", "not_found", "ambiguous", "unresolvable", "invalid"]);
    }

    #[test]
    fn output_schema_snapshot() {
        let schema = Value::Object(output_schema::<VerifyView>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["found", "checked", "citations"]));
        assert!(schema.get("$schema").is_none());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("output_schema", schema);
        });
    }
}
