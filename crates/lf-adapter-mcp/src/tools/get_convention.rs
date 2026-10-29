//! Outil `fr_get_convention` (05 §5.6) : un texte de convention collective, ses métadonnées, sa
//! citation et une page de sa table des matières. Le `#[tool]` suit le motif de `tools/mod.rs`.

use lf_application::{
    input::GetConventionInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_convention::GetConventionParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_convention::ConventionView,
};

/// Description du descripteur, mot pour mot (05 §5.6, 432 caractères).
const DESCRIPTION: &str = "Read a collective labour agreement text (convention collective, avenant, annexe) by its KALITEXT id from fr_search on fond KALI: title, IDCC, parent convention, status, citation, source_url and a table of contents. Article entries carry KALIARTI ids: read them with fr_get_text. Page the table of contents with `toc_offset`/`toc_limit` (default 100) and `next_toc_offset`. For a company-level agreement use fr_get_company_agreement.";

#[tool_router(router = get_convention_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_get_convention",
        title = "Read a collective agreement",
        description = DESCRIPTION,
        input_schema = input_schema::<GetConventionParams>(),
        output_schema = output_schema::<ConventionView>(),
        annotations(
            title = "Read a collective agreement",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_convention(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<ConventionView>, ToolFailure> {
        let call = to_call::<GetConventionParams, GetConventionInput>(args);
        let output =
            self.service.get_convention(call, ct.cancelled()).await.map_err(ToolFailure)?;
        Ok(Json(ConventionView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::Fixture;
    use lf_application::{ports::AuditStatus, testing::PortRequest, tools::ToolName};
    use lf_domain::{
        contract::{Idcc, Nor},
        entity::{ArticleDraft, Convention, SectionDraft, TextBody, Toc, TocDraft},
        id::KaliTextId,
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult};
    use serde_json::{Value, json};

    const ID: &str = "KALITEXT000049878950";
    const DESCRIPTION: &str = "Read a collective labour agreement text (convention collective, avenant, annexe) by its KALITEXT id from fr_search on fond KALI: title, IDCC, parent convention, status, citation, source_url and a table of contents. Article entries carry KALIARTI ids: read them with fr_get_text. Page the table of contents with `toc_offset`/`toc_limit` (default 100) and `next_toc_offset`. For a company-level agreement use fr_get_company_agreement.";

    /// Convention de l'exemple de 05 §5.6 : un chapitre et son article, de même état que le texte.
    fn convention() -> Convention {
        let section = SectionDraft {
            id: Some("KALISCTA000049886058".to_owned()),
            title: Some("Chapitre 1er : Recours au CDD d'usage".to_owned()),
            articles: vec![ArticleDraft {
                id: Some("KALIARTI000049886059".to_owned()),
                num: Some("19".to_owned()),
                etat: Some("VIGUEUR_ETEN".to_owned()),
                ..ArticleDraft::default()
            }],
            ..SectionDraft::default()
        };
        Convention {
            text_id: KaliTextId::parse(ID).unwrap(),
            body: TextBody {
                nor: Nor::parse("ASET2450455M"),
                title: Some("Annexe IX - Stipulations spécifiques".to_owned()),
                etat: LegalStatus::parse("VIGUEUR_ETEN"),
                toc: Toc::build(TocDraft { sections: vec![section], articles: Vec::new() })
                    .unwrap(),
                ..TextBody::default()
            },
            idcc: Idcc::parse("2121"),
            container_title: Some("Convention collective nationale de l'édition".to_owned()),
            container_id: Some("KALICONT000005635096".to_owned()),
            bulletin: Some("BO n°2024-20".to_owned()),
        }
    }

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_get_convention");
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    fn text_of(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    #[tokio::test]
    async fn the_descriptor_announces_title_description_hints_and_schemas() {
        let client = Fixture::default().connect().await;

        let tools = client.list_all_tools().await.unwrap();

        let tool = tools.iter().find(|tool| tool.name == "fr_get_convention").expect("tool");
        assert_eq!(tool.title.as_deref(), Some("Read a collective agreement"));
        assert_eq!(tool.description.as_deref(), Some(DESCRIPTION));
        assert_eq!(DESCRIPTION.chars().count(), 432);
        let hints = tool.annotations.as_ref().unwrap();
        assert_eq!(hints.title.as_deref(), Some("Read a collective agreement"));
        assert_eq!(
            (
                hints.read_only_hint,
                hints.destructive_hint,
                hints.idempotent_hint,
                hints.open_world_hint
            ),
            (Some(true), Some(false), Some(true), Some(true))
        );
        assert_eq!(tool.input_schema["required"], json!(["convention_id"]));
        let properties = tool.input_schema["properties"].as_object().unwrap();
        assert!(!properties.contains_key("date"), "pas de date (00 §7)");
        let output = tool.output_schema.as_ref().unwrap();
        assert_eq!(output["type"], "object");
        assert!(output.get("$schema").is_none());
    }

    #[tokio::test]
    async fn a_call_returns_the_view_of_the_spec_example_and_writes_one_audit_line() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_convention(Ok(convention()));

        let result = call(&fixture, &json!({"convention_id": ID})).await;

        assert_eq!(result.is_error, Some(false));
        let view = result.structured_content.clone().unwrap();
        assert_eq!(
            view,
            json!({
                "convention_id": ID,
                "idcc": "2121",
                "nor": "ASET2450455M",
                "container_title": "Convention collective nationale de l'édition",
                "etat": "VIGUEUR_ETEN",
                "bulletin": "BO n°2024-20",
                "citation": "Annexe IX - Stipulations spécifiques",
                "source_url": format!("https://www.legifrance.gouv.fr/conv_coll/id/{ID}"),
                "toc": [
                    {"depth": 1, "title": "Chapitre 1er : Recours au CDD d'usage"},
                    {"depth": 2, "article_id": "KALIARTI000049886059", "num": "19"},
                ],
                "toc_total": 2,
            })
        );
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), view);
        assert_eq!(
            fixture.corpus.seen_requests(),
            [PortRequest::GetConvention(KaliTextId::parse(ID).unwrap())]
        );
        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_CONVENTION);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 2);
    }

    #[tokio::test]
    async fn the_table_of_contents_is_paged() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_convention(Ok(convention()));

        let result = call(&fixture, &json!({"convention_id": ID, "toc_limit": 1})).await;

        let view = result.structured_content.unwrap();
        assert_eq!(view["toc"].as_array().unwrap().len(), 1);
        assert_eq!(view["toc_total"], 2);
        assert_eq!(view["next_toc_offset"], 1);
    }

    #[tokio::test]
    async fn a_date_is_refused_as_an_unknown_argument_before_any_network_call() {
        let fixture = Fixture::default();

        let result = call(&fixture, &json!({"convention_id": ID, "date": "2020-01-01"})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            "[invalid_arg] date: unknown field `date`, expected one of `convention_id`, \
             `toc_offset`, `toc_limit`"
        );
        assert_eq!(fixture.corpus.calls(), 0);
        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
    }

    #[tokio::test]
    async fn the_wrong_family_points_to_the_right_tool() {
        let fixture = Fixture::default();

        let result = call(&fixture, &json!({"convention_id": "LEGITEXT000033205014"})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            "[invalid_arg] convention_id='LEGITEXT000033205014' must be a Legifrance KALITEXT... \
             identifier. LEGITEXT... identifies a whole text: use fr_get_act."
        );
        assert_eq!(fixture.corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_unknown_agreement_is_an_in_band_not_found() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_convention(Err(lf_application::ports::CorpusError::NotFound {
            what: "x".to_owned(),
        }));

        let result = call(&fixture, &json!({"convention_id": ID})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            format!(
                "[not_found] No collective agreement text found for {ID}. Check the id, or search with fr_search."
            )
        );
    }
}
