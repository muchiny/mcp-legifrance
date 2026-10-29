//! Outil `fr_get_act` (05 §5.2) : un texte consolidé, ses métadonnées, sa citation et une page de
//! sa table des matières. Le `#[tool]` suit le motif de `tools/mod.rs`.

use lf_application::{
    input::GetActInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject, tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_act::GetActParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_act::ActView,
};

/// Description du descripteur, mot pour mot (05 §5.2, 630 caractères).
const DESCRIPTION: &str = "Read a consolidated law, decree or code by its LEGITEXT id (from fr_search on LODA or CODE): metadata, citation, source_url and a table of contents of sections and articles with their `article_id`. Returns the version in force on `date` (YYYY-MM-DD; default: the version named by a suffixed text_id, else today in Paris); without `date_fin_version` that version has no end date. The table of contents is paged: up to `toc_limit` entries (default 100) from `toc_offset`; continue with `next_toc_offset`. It does not return article text: read an article with fr_get_text. For a collective agreement (KALITEXT) use fr_get_convention.";

#[tool_router(router = get_act_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_get_act",
        title = "Read a law, decree or code",
        description = DESCRIPTION,
        input_schema = input_schema::<GetActParams>(),
        output_schema = output_schema::<ActView>(),
        annotations(
            title = "Read a law, decree or code",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_act(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<ActView>, ToolFailure> {
        let call = to_call::<GetActParams, GetActInput>(args);
        let output = self.service.get_act(call, ct.cancelled()).await.map_err(ToolFailure)?;
        Ok(Json(ActView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::Fixture;
    use lf_application::{ports::AuditStatus, testing::PortRequest, tools::ToolName};
    use lf_domain::{
        date::VersionDate,
        entity::{Act, ArticleDraft, SectionDraft, TextBody, Toc, TocDraft},
        id::LegiTextId,
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult};
    use serde_json::{Value, json};

    const ID: &str = "LEGITEXT000033205014";
    const DESCRIPTION: &str = "Read a consolidated law, decree or code by its LEGITEXT id (from fr_search on LODA or CODE): metadata, citation, source_url and a table of contents of sections and articles with their `article_id`. Returns the version in force on `date` (YYYY-MM-DD; default: the version named by a suffixed text_id, else today in Paris); without `date_fin_version` that version has no end date. The table of contents is paged: up to `toc_limit` entries (default 100) from `toc_offset`; continue with `next_toc_offset`. It does not return article text: read an article with fr_get_text. For a collective agreement (KALITEXT) use fr_get_convention.";

    fn toc(sections: u32) -> Toc {
        let sections = (1..=sections)
            .map(|n| SectionDraft {
                id: Some(format!("LEGISCTA{n:012}")),
                title: Some(format!("Chapitre {n}")),
                articles: vec![ArticleDraft {
                    id: Some(format!("LEGIARTI{n:012}")),
                    num: Some(n.to_string()),
                    etat: Some("VIGUEUR".to_owned()),
                    ..ArticleDraft::default()
                }],
                ..SectionDraft::default()
            })
            .collect();
        Toc::build(TocDraft { sections, articles: Vec::new() }).unwrap()
    }

    fn act(nature: &str, sections: u32) -> Act {
        Act {
            text_id: LegiTextId::parse(ID).unwrap(),
            body: TextBody {
                title: Some("LOI n° 2016-1321 du 7 octobre 2016".to_owned()),
                nature: Some(nature.to_owned()),
                etat: LegalStatus::parse("VIGUEUR"),
                toc: toc(sections),
                ..TextBody::default()
            },
        }
    }

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_get_act");
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

        let tool = tools.iter().find(|tool| tool.name == "fr_get_act").expect("fr_get_act");
        assert_eq!(tool.title.as_deref(), Some("Read a law, decree or code"));
        assert_eq!(tool.description.as_deref(), Some(DESCRIPTION));
        assert_eq!(DESCRIPTION.chars().count(), 630);
        let hints = tool.annotations.as_ref().unwrap();
        assert_eq!(hints.title.as_deref(), Some("Read a law, decree or code"));
        assert_eq!(
            (
                hints.read_only_hint,
                hints.destructive_hint,
                hints.idempotent_hint,
                hints.open_world_hint
            ),
            (Some(true), Some(false), Some(true), Some(true))
        );
        assert_eq!(tool.input_schema["required"], json!(["text_id"]));
        assert_eq!(tool.input_schema["additionalProperties"], false);
        let output = tool.output_schema.as_ref().unwrap();
        assert_eq!(output["type"], "object");
        assert!(output.get("$schema").is_none());
    }

    #[tokio::test]
    async fn a_call_returns_the_view_and_writes_one_audit_line() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_act(Ok(act("LOI", 3)));

        let result = call(&fixture, &json!({"text_id": ID, "toc_limit": 4})).await;

        assert_eq!(result.is_error, Some(false));
        let view = result.structured_content.clone().unwrap();
        assert_eq!(view["text_id"], ID);
        assert_eq!(view["date"], "2026-10-29");
        assert_eq!(view["source_url"], format!("https://www.legifrance.gouv.fr/loda/id/{ID}"));
        assert_eq!(view["toc_total"], 6);
        assert_eq!(view["toc"].as_array().unwrap().len(), 4);
        assert_eq!(view["next_toc_offset"], 4);
        // La même vue sert de texte (parité des clients sans `structuredContent`).
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), view);
        assert_eq!(
            fixture.corpus.seen_requests(),
            [PortRequest::GetAct(
                LegiTextId::parse(ID).unwrap(),
                VersionDate::parse("2026-10-29").unwrap()
            )]
        );
        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_ACT);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 4);
    }

    #[tokio::test]
    async fn get_act_on_a_code_carries_the_note_and_the_code_url() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_act(Ok(act("CODE", 1)));

        let result = call(&fixture, &json!({"text_id": ID})).await;

        let view = result.structured_content.unwrap();
        assert_eq!(
            view["note"],
            "This is a whole code: its table of contents can be very long. Page it with \
             toc_offset/toc_limit, or find an article with fr_search fond=CODE."
        );
        assert_eq!(
            view["source_url"],
            format!("https://www.legifrance.gouv.fr/codes/texte_lc/{ID}")
        );
    }

    #[tokio::test]
    async fn a_suffixed_identifier_names_the_version_that_is_read() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_act(Ok(act("LOI", 1)));

        let result = call(&fixture, &json!({"text_id": format!("{ID}_08-09-2023")})).await;

        assert_eq!(result.structured_content.unwrap()["date"], "2023-09-08");
        assert_eq!(
            fixture.corpus.seen_requests(),
            [PortRequest::GetAct(
                LegiTextId::parse(ID).unwrap(),
                VersionDate::parse("2023-09-08").unwrap()
            )]
        );
    }

    #[tokio::test]
    async fn a_bad_argument_is_an_in_band_invalid_arg_that_is_audited_without_network() {
        let fixture = Fixture::default();

        let unknown = call(&fixture, &json!({"text_id": ID, "foo": 1})).await;
        let missing = call(&fixture, &json!({})).await;

        assert_eq!(unknown.is_error, Some(true));
        assert!(unknown.structured_content.is_none());
        assert_eq!(
            text_of(&unknown),
            "[invalid_arg] foo: unknown field `foo`, expected one of `text_id`, `date`, \
             `toc_offset`, `toc_limit`"
        );
        assert_eq!(text_of(&missing), "[invalid_arg] text_id is required.");
        assert_eq!(fixture.corpus.calls(), 0);
        let records = fixture.audit.records();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.status == AuditStatus::Error));
        assert!(records.iter().all(|record| record.output_count_or_size == 0));
    }

    #[tokio::test]
    async fn an_unknown_text_is_an_in_band_not_found() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_act(Err(lf_application::ports::CorpusError::NotFound {
            what: "x".to_owned(),
        }));

        let result = call(&fixture, &json!({"text_id": ID})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            format!(
                "[not_found] No LODA text found for {ID}. Check the id, or search with fr_search \
                 fond=LODA or CODE."
            )
        );
    }
}
