//! Outil `fr_coverage` (05 §5.8) : ce que le serveur couvre, son environnement et l'état de sa
//! configuration, sans appel réseau.
//!
//! Aucun argument : le schéma d'entrée est un objet fermé et vide, et tout argument est un
//! `invalid_arg` audité (décodage de `params::coverage`). Motif de `tools/mod.rs`.

use lf_application::{
    input::CoverageInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::coverage::CoverageParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::coverage::CoverageView,
};

/// Description de l'outil (497 caractères, 5 phrases ; 05 §5.8).
pub(crate) const DESCRIPTION: &str = "Describe what this server covers, how it is sourced and what it does NOT cover. Call it before telling a user that the law \"does not contain\" something, and whenever a search comes back empty: the gap may be in this server, not in the law. Each gap comes with a fallback. It also reports the PISTE environment (sandbox or production), whether the configuration is complete, the data source and licence with today's consultation date, and the note on ELI identifiers. No arguments, no network call.";

#[tool_router(router = coverage_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_coverage",
        title = "Coverage and known gaps",
        description = DESCRIPTION,
        input_schema = input_schema::<CoverageParams>(),
        output_schema = output_schema::<CoverageView>(),
        annotations(
            title = "Coverage and known gaps",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_coverage(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<CoverageView>, ToolFailure> {
        let call = to_call::<CoverageParams, CoverageInput>(args);
        let output = self.service.coverage(call, ct.cancelled()).await?;
        Ok(Json(CoverageView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 497);
        assert!(DESCRIPTION.starts_with("Describe what this server covers"));
        assert!(DESCRIPTION.ends_with("No arguments, no network call."));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{error::ErrorCode, ports::AuditStatus, tools::ToolName};
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        fn request(arguments: Option<&Value>) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_coverage");
            params.arguments = arguments.and_then(|value| value.as_object().cloned());
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_coverage").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Coverage and known gaps"));
            assert_eq!(tool.description.as_deref(), Some(super::super::DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), Some("Coverage and known gaps"));
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(true));
            assert_eq!(
                Value::Object(tool.input_schema.as_ref().clone()),
                json!({"type": "object", "properties": {}, "additionalProperties": false})
            );
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert_eq!(output["type"], "object");
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn coverage_makes_no_port_call() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            let result = client.call_tool(request(None)).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["status"], "ok");
            assert_eq!(structured["environment"], "sandbox");
            assert_eq!(structured["consulted_on"], "2026-10-29");
            assert_eq!(fixture.corpus.calls(), 0);
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_COVERAGE);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(records[0].input.as_str(), "{}");
            // Bac à sable avec identifiants : neuf lacunes du catalogue et FR-101.
            assert_eq!(records[0].output_count_or_size, 10);
        }

        #[tokio::test]
        async fn empty_arguments_and_no_arguments_are_the_same_call() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            let without = client.call_tool(request(None)).await.unwrap();
            let with = client.call_tool(request(Some(&json!({})))).await.unwrap();
            assert_eq!(without.structured_content, with.structured_content);
        }

        #[tokio::test]
        async fn coverage_rejects_any_argument() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            let result = client.call_tool(request(Some(&json!({"query": "bail"})))).await.unwrap();
            assert_eq!(result.is_error, Some(true));
            assert!(result.structured_content.is_none());
            assert_eq!(
                result.content[0].as_text().unwrap().text,
                "[invalid_arg] query: unknown field `query`, there are no fields"
            );
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].status, AuditStatus::Error);
            assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
            assert_eq!(records[0].output_count_or_size, 0);
            assert_eq!(fixture.corpus.calls(), 0);
        }

        #[tokio::test]
        async fn coverage_degraded_without_credentials() {
            let mut fixture = Fixture::default();
            fixture.settings.configuration.credentials_configured = false;
            let client = fixture.connect().await;
            // L'outil répond même sans identifiants : c'est lui qui l'explique (FR-102).
            let result = client.call_tool(request(None)).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["status"], "degraded");
            assert_eq!(structured["configuration"]["credentials_configured"], false);
            let gaps = structured["gaps"].as_array().unwrap();
            assert!(gaps.iter().any(|gap| gap["id"] == "FR-102"));
        }
    }
}
