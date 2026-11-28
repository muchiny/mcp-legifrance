//! Outil `fr_get_judilibre` (05 §5.19) : la lecture d'une décision de Judilibre, entière ou par
//! partie, avec le retour progressif des lectures longues.
//!
//! La source n'est pas Légifrance : c'est le port `JudicialOpenData`, servi par
//! `lf-adapter-piste` avec le même jeton PISTE (ADR-0035). Motif de `tools/mod.rs` : arguments bruts
//! (`JsonObject`), décodage par `to_call`, appel de la façade avec le jeton d'annulation de la
//! requête, vue de sortie à schéma explicite.

use lf_application::{
    input::GetJudilibreInput,
    ports::{AuditLog, Clock, JudicialOpenData, LegalCorpus, PublicGuidance, TaxDoctrine},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_judilibre::GetJudilibreParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_judilibre::CaseLawView,
};

/// Description du descripteur, mot pour mot (05 §5.19, 629 caractères, 4 phrases, sans adresse ;
/// « for example » plutôt que « e.g. », AM-63). Elle dit les mentions à citer, la lecture par
/// partie, les liens rendus et l'interdiction de réidentifier ou de profiler (ADR-0035,
/// décisions 6 et 7).
pub(crate) const DESCRIPTION: &str = "Read one decision from Judilibre (Cour de cassation open data) by its id from fr_search_judilibre. Returns the court, seat, chamber or formation and date, which must be cited with the decision, then its text as published, progressively like fr_get_text; `part` reads only one part, for example motivations (the reasons) or dispositif (the ruling). Also lists the texts applied (visa, to check with fr_resolve), related decisions with their ids, and the decision under appeal. Decisions are pseudonymised: never try to re-identify a party, and never use them to evaluate, compare or predict the practice of a judge or court clerk.";

#[tool_router(router = get_judilibre_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance, J: JudicialOpenData>
    LegifranceServer<C, A, K, D, G, J>
{
    #[tool(
        name = "fr_get_judilibre",
        title = "Read a Judilibre decision",
        description = DESCRIPTION,
        input_schema = input_schema::<GetJudilibreParams>(),
        output_schema = output_schema::<CaseLawView>(),
        annotations(
            title = "Read a Judilibre decision",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_judilibre(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<CaseLawView>, ToolFailure> {
        let call = to_call::<GetJudilibreParams, GetJudilibreInput>(args);
        let output = self.service.get_judilibre(call, ct.cancelled()).await?;
        Ok(Json(CaseLawView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 629);
        assert!(DESCRIPTION.starts_with("Read one decision from Judilibre"));
        assert!(DESCRIPTION.contains("which must be cited with the decision"));
        assert!(DESCRIPTION.contains("progressively like fr_get_text"));
        assert!(DESCRIPTION.contains("to check with fr_resolve"));
        assert!(DESCRIPTION.contains("never try to re-identify a party"));
        // Aucune adresse : seuls les liens vers Légifrance sont admis dans un descripteur.
        assert!(!DESCRIPTION.contains("http"));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{
            error::ErrorCode,
            ports::{AuditStatus, CaseLawError},
            testing::CaseLawRequest,
            tools::ToolName,
        };
        use lf_domain::judilibre::{
            CaseLawDoc, CaseLawMention, Court, DecisionPart, JudilibreId, ZoneSpan, Zones,
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::{testing::Fixture, views::search_judilibre::CASE_LAW_NOTE};

        const ID: &str = "5fca624b7eeb384a40cafb85";

        /// Une décision de la Cour de cassation : motifs puis dispositif, délimités par ses zones.
        fn decision() -> CaseLawDoc {
            let motifs = "Vu l'article 894 du code civil. La donation est irrévocable.";
            let dispositif = "CASSE ET ANNULE.";
            let text = format!("{motifs}\n{dispositif}");
            // Unités UTF-16 : tout le texte est dans le plan de base, un caractère par unité.
            let units = |text: &str| u32::try_from(text.encode_utf16().count()).unwrap();
            let start = units(motifs).checked_add(1).unwrap();
            CaseLawDoc {
                id: JudilibreId::parse(ID).unwrap(),
                jurisdiction: Court::Cassation,
                mention: CaseLawMention {
                    court: "Cour de cassation".to_owned(),
                    chamber: Some("Première chambre civile".to_owned()),
                    formation: None,
                    date: "2020-12-02".to_owned(),
                },
                number: Some("19-20.000".to_owned()),
                ecli: None,
                solution: Some("Cassation".to_owned()),
                publication: None,
                themes: Vec::new(),
                summary: None,
                zones: Zones::of([
                    (DecisionPart::Motivations, ZoneSpan { start: 0, end: units(motifs) }),
                    (DecisionPart::Dispositif, ZoneSpan { start, end: units(&text) }),
                ]),
                text,
                visa: vec!["article 894 du code civil".to_owned()],
                related: Vec::new(),
                contested: None,
                updated: Some("2024-03-01".to_owned()),
                partial: false,
                to_be_deleted: false,
            }
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_get_judilibre");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_get_judilibre").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Read a Judilibre decision"));
            assert_eq!(tool.description.as_deref(), Some(super::super::DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(true));
            let input = Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(input["required"], json!(["id"]));
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert!(output["properties"].get("text").is_some());
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn a_part_is_read_with_the_mentions_and_is_audited() {
            let fixture = Fixture::default();
            fixture.case_law.push_get(Ok(decision()));
            let client = fixture.connect().await;
            let arguments =
                json!({"id": ID.to_uppercase(), "part": "dispositif", "detail": "full"});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["id"], ID);
            assert_eq!(structured["court"], "Cour de cassation");
            assert_eq!(
                structured["citation"],
                "Cour de cassation, Première chambre civile, 2 décembre 2020, n° 19-20.000"
            );
            assert_eq!(structured["visa"], json!([{"title": "article 894 du code civil"}]));
            assert_eq!(structured["case_law_note"], CASE_LAW_NOTE);
            assert_eq!(structured["text"], "CASSE ET ANNULE.");
            assert_eq!(structured["text_length"], 16);
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);

            let id = JudilibreId::parse(ID).unwrap();
            assert_eq!(fixture.case_law.seen_requests(), [CaseLawRequest::Get(id)]);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_GET_JUDILIBRE);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(fixture.corpus.calls(), 0);
        }

        #[tokio::test]
        async fn an_unknown_decision_is_not_found_and_bad_arguments_never_reach_the_port() {
            let fixture = Fixture::default();
            fixture.case_law.push_get(Err(CaseLawError::NotFound));
            let client = fixture.connect().await;
            let result = client.call_tool(request(&json!({"id": ID}))).await.unwrap();
            assert_eq!(
                result.content[0].as_text().unwrap().text,
                format!("[not_found] No Judilibre decision {ID}. Search with fr_search_judilibre.")
            );
            for arguments in [
                json!({}),
                json!({"id": "JURITEXT000043486135"}),
                json!({"id": ID, "part": "judges"}),
                json!({"id": ID, "offset": -1}),
            ] {
                let result = client.call_tool(request(&arguments)).await.unwrap();
                let text = &result.content[0].as_text().unwrap().text;
                assert!(text.starts_with("[invalid_arg] "), "{arguments}: {text}");
            }
            assert_eq!(fixture.case_law.calls(), 1);
            let codes: Vec<_> =
                fixture.audit.records().iter().map(|record| record.error_code).collect();
            assert_eq!(codes[0], Some(ErrorCode::NotFound));
            assert!(codes[1..].iter().all(|code| *code == Some(ErrorCode::InvalidArg)));
        }
    }
}
