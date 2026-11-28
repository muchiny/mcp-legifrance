//! Outil `fr_get_service_public` (05 §5.17) : la lecture d'une fiche pratique ou d'un dossier de
//! Service-Public.gouv.fr (DILA), avec les textes de Légifrance sur lesquels la fiche s'appuie.
//!
//! La source n'est pas Légifrance : c'est le port `PublicGuidance`, servi par
//! `lf-adapter-service-public` (ADR-0033). Motif de `tools/mod.rs` : arguments bruts
//! (`JsonObject`), décodage par `to_call`, appel de la façade avec le jeton d'annulation de la
//! requête, vue de sortie à schéma explicite.

use lf_application::{
    input::GetServicePublicInput,
    ports::{AuditLog, Clock, JudicialOpenData, LegalCorpus, PublicGuidance, TaxDoctrine},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_service_public::GetServicePublicParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_service_public::GuidanceView,
};

/// Description du descripteur, mot pour mot (05 §5.17, 641 caractères, 5 phrases, sans adresse ;
/// les points de suspension sont en ASCII). Elle nomme les outils qui lisent les références.
pub(crate) const DESCRIPTION: &str = "Read one Service-Public.gouv.fr practical sheet (F...) or folder (N...) by its id from fr_search_service_public, for its audience. A sheet returns its title, description, date, citation, source_url and legal_references: the Légifrance texts it relies on, with the ids that fr_get_text, fr_get_act, fr_get_convention or fr_get_decision read. A folder lists its sheets. The sheet is read from the day's archive kept in memory (downloaded once if no search came first). This is administrative guidance written for the public, not the law, and only today's version is served: read the texts in legal_references before relying on a precise point.";

#[tool_router(router = get_service_public_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance, J: JudicialOpenData>
    LegifranceServer<C, A, K, D, G, J>
{
    #[tool(
        name = "fr_get_service_public",
        title = "Read a Service-Public sheet",
        description = DESCRIPTION,
        input_schema = input_schema::<GetServicePublicParams>(),
        output_schema = output_schema::<GuidanceView>(),
        annotations(
            title = "Read a Service-Public sheet",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_service_public(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<GuidanceView>, ToolFailure> {
        let call = to_call::<GetServicePublicParams, GetServicePublicInput>(args);
        let output = self.service.get_service_public(call, ct.cancelled()).await?;
        Ok(Json(GuidanceView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 641);
        assert!(DESCRIPTION.starts_with("Read one Service-Public.gouv.fr practical sheet (F...)"));
        assert!(
            DESCRIPTION.contains("fr_get_text, fr_get_act, fr_get_convention or fr_get_decision")
        );
        assert!(DESCRIPTION.contains("not the law"));
        assert!(DESCRIPTION.ends_with("before relying on a precise point."));
        assert!(!DESCRIPTION.contains("http"));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{
            error::ErrorCode,
            ports::{ArchiveFile, AuditStatus, GuidanceError, GuidanceRead},
            testing::GuidanceRequest,
            tools::ToolName,
        };
        use lf_domain::{
            date::CivilDate,
            guidance::{
                Audience, Block, GuidanceContent, GuidanceDoc, Inline, ServicePublicUrl, SheetId,
                legal_references, render,
            },
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        const ARTICLE: &str =
            "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000032041571";

        fn sheet(audience: Audience, raw: &str) -> GuidanceRead {
            let id = SheetId::parse(raw).unwrap();
            let blocks = [Block::Paragraph(vec![Inline::Text("Le texte de la fiche.".to_owned())])];
            let doc = GuidanceDoc {
                source_url: ServicePublicUrl::read(None, audience, &id),
                id,
                audience,
                sheet_type: Some("Fiche d'information".to_owned()),
                title: "Solde bancaire insaisissable (SBI)".to_owned(),
                description: Some("Somme laissée sur le compte.".to_owned()),
                theme: Some("Argent".to_owned()),
                path: vec!["Justice".to_owned()],
                updated: CivilDate::new(2026, 4, 1),
                content: GuidanceContent::Sheet {
                    text: render(&blocks).unwrap(),
                    references: legal_references([(Some("Code civil"), ARTICLE)]),
                },
            };
            GuidanceRead { doc, file: ArchiveFile { audience, last_modified: None } }
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_get_service_public");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_get_service_public").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Read a Service-Public sheet"));
            assert_eq!(tool.description.as_deref(), Some(super::super::DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(true));
            let input = Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(input["required"], json!(["id"]));
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert!(output["properties"].get("legal_references").is_some());
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn a_sheet_is_read_with_its_references_and_is_audited() {
            let fixture = Fixture::default();
            fixture.guidance.push_get(Ok(sheet(Audience::Entreprendre, "F1437")));
            let client = fixture.connect().await;
            let arguments = json!({"id": "f1437", "audience": "Entreprendre"});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["id"], "F1437");
            assert_eq!(structured["audience"], "entreprendre");
            assert_eq!(structured["text"], "Le texte de la fiche.");
            assert_eq!(structured["legal_references"][0]["id"], "LEGIARTI000032041571");
            assert_eq!(structured["source_file"]["entry"], "F1437.xml");
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);

            let id = SheetId::parse("F1437").unwrap();
            assert_eq!(
                fixture.guidance.seen_requests(),
                [GuidanceRequest::Get(Audience::Entreprendre, id)]
            );
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_GET_SERVICE_PUBLIC);
            assert_eq!(records[0].status, AuditStatus::Ok);
            // Caractères rendus : le texte entier (21) et le résumé officiel, la description (28).
            assert_eq!(records[0].output_count_or_size, 49);
            assert_eq!((fixture.corpus.calls(), fixture.doctrine.calls()), (0, 0));
        }

        #[tokio::test]
        async fn an_absent_sheet_names_the_other_audience() {
            let fixture = Fixture::default();
            fixture.guidance.push_get(Err(GuidanceError::NotFound));
            let client = fixture.connect().await;
            let result = client.call_tool(request(&json!({"id": "F9999"}))).await.unwrap();
            assert_eq!(result.is_error, Some(true));
            assert_eq!(
                result.content[0].as_text().unwrap().text,
                "[not_found] No Service-Public sheet or folder F9999 for particuliers. Try \
                 audience entreprendre, or search with fr_search_service_public."
            );
            let records = fixture.audit.records();
            assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
        }

        #[tokio::test]
        async fn bad_arguments_are_refused_before_the_port() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            for arguments in [
                json!({}),
                json!({"id": "X12"}),
                json!({"id": "F1437", "audience": "all"}),
                json!({"id": "F1437", "detail": "none"}),
            ] {
                let result = client.call_tool(request(&arguments)).await.unwrap();
                let text = &result.content[0].as_text().unwrap().text;
                assert!(text.starts_with("[invalid_arg] "), "{arguments}: {text}");
            }
            assert_eq!(fixture.guidance.calls(), 0);
        }
    }
}
