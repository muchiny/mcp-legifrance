//! Outil `fr_diff_article` (05 §5.12) : ce qui a changé dans un article entre deux dates.
//!
//! Le motif est celui de `tools/mod.rs` : arguments bruts (`JsonObject`), décodés par
//! [`decode::to_call`] (l'échec voyage dans l'appel et devient un `invalid_arg` audité), schémas
//! explicites sans `$schema`, jeton d'annulation transmis à la façade.

use lf_application::{
    input::DiffArticleInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode,
    failure::ToolFailure,
    params::diff_article::DiffArticleParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::diff_article::DiffView,
};

/// Description de l'outil (05 §5.12, 4 phrases). Source unique : elle ne cite que `fr_get_text`.
const DESCRIPTION: &str = "Compare the versions of a code or law article (LEGIARTI) in force on \
`date_a` and `date_b` (YYYY-MM-DD; `date_b` defaults to today in Paris). Returns both versions \
with their validity dates (no `date_fin`: no end date) and a compact word-level diff: removed \
and added passages with a little context. `identical` is true when both dates fall in the same \
version. To read a single version use fr_get_text.";

#[tool_router(router = diff_article_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_diff_article",
        title = "Compare article versions",
        description = DESCRIPTION,
        input_schema = input_schema::<DiffArticleParams>(),
        output_schema = output_schema::<DiffView>(),
        annotations(
            title = "Compare article versions",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_diff_article(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<DiffView>, ToolFailure> {
        let call = decode::to_call::<DiffArticleParams, DiffArticleInput>(args);
        let output = self.service.diff_article(call, ct.cancelled()).await.map_err(ToolFailure)?;
        Ok(Json(DiffView::from(output)))
    }
}

// Les tests passent par `testing::connect`, qui n'existe qu'avec la feature `test-support`.
#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests {
    use lf_application::{
        error::ErrorCode,
        ports::{AuditStatus, CorpusError},
        testing::PortRequest,
        tools::ToolName,
    };
    use lf_domain::{
        date::CivilDate,
        entity::{ArticleText, ArticleVersion, ParentText},
        id::{ArticleId, DocumentId, LegiArtiId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
    use serde_json::{Value, json};

    use super::*;
    use crate::testing::Fixture;

    fn arti(n: u32) -> LegiArtiId {
        LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap()
    }

    fn version(n: u32, from: (u16, u8, u8), to: Option<(u16, u8, u8)>) -> ArticleVersion {
        ArticleVersion {
            id: arti(n),
            num: Some("222-1".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            date_debut: CivilDate::new(from.0, from.1, from.2),
            date_fin: to.and_then(|(y, m, d)| CivilDate::new(y, m, d)),
        }
    }

    fn article(n: u32, text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::Legi(arti(n)),
            num: Some("222-1".to_owned()),
            etat: None,
            code_title: Some("Code pénal".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070719").unwrap(),
                Some("CODE"),
            )),
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    /// Deux versions de l'article : la 1 jusqu'au 2023-09-07, la 2 depuis le lendemain.
    fn two_versions(fixture: &Fixture, old: &str, new: &str) {
        fixture.corpus.push_article_versions(Ok(vec![
            version(1, (2016, 10, 9), Some((2023, 9, 7))),
            version(2, (2023, 9, 8), None),
        ]));
        fixture.corpus.push_get_article(Ok(article(1, old)));
        fixture.corpus.push_get_article(Ok(article(2, new)));
    }

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_diff_article");
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    async fn descriptor(fixture: &Fixture) -> Tool {
        let client = fixture.connect().await;
        let tools = client.list_all_tools().await.unwrap();
        tools
            .into_iter()
            .find(|tool| tool.name == "fr_diff_article")
            .expect("fr_diff_article est enregistré")
    }

    fn text_of(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    #[tokio::test]
    async fn a_diff_answers_with_structured_content_and_one_audit_line() {
        let fixture = Fixture::default();
        two_versions(&fixture, "est puni de un an", "est puni de deux ans");

        let result = call(
            &fixture,
            &json!({"article_id": arti(2).as_str(), "date_a": "2020-01-01", "date_b": "2026-01-01"}),
        )
        .await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.clone().unwrap();
        assert_eq!(
            content,
            json!({
                "article_id": "LEGIARTI000000000002",
                "citation": "Code pénal, art. 222-1",
                "version_a": {
                    "article_id": "LEGIARTI000000000001",
                    "date_debut": "2016-10-09",
                    "date_fin": "2023-09-07",
                    "etat": "VIGUEUR"
                },
                "version_b": {
                    "article_id": "LEGIARTI000000000002",
                    "date_debut": "2023-09-08",
                    "etat": "VIGUEUR"
                },
                "identical": false,
                "hunks": [{
                    "at": 12,
                    "context": "est puni de",
                    "removed": "un an",
                    "added": "deux ans"
                }]
            })
        );
        // Le bloc texte porte le même JSON que `structuredContent`.
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), content);

        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_DIFF_ARTICLE);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 1, "le nombre de hunks");
        let requests = fixture.corpus.seen_requests();
        assert_eq!(requests[0], PortRequest::ArticleVersions(ArticleId::Legi(arti(2))));
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn without_date_b_the_second_version_is_the_one_in_force_today_in_paris() {
        let fixture = Fixture::default();
        two_versions(&fixture, "avant", "après");
        let result =
            call(&fixture, &json!({"article_id": arti(1).as_str(), "date_a": "2020-01-01"})).await;
        let content = result.structured_content.unwrap();
        assert_eq!(content["version_b"]["article_id"], "LEGIARTI000000000002");
        assert_eq!(content["identical"], false);
    }

    #[tokio::test]
    async fn two_dates_in_one_version_are_identical() {
        let fixture = Fixture::default();
        fixture.corpus.push_article_versions(Ok(vec![version(1, (2016, 10, 9), None)]));
        fixture.corpus.push_get_article(Ok(article(1, "texte")));
        let result = call(
            &fixture,
            &json!({"article_id": arti(1).as_str(), "date_a": "2020-01-01", "date_b": "2021-01-01"}),
        )
        .await;
        let content = result.structured_content.unwrap();
        assert_eq!(content["identical"], true);
        assert_eq!(content["hunks"], json!([]));
        assert_eq!(fixture.audit.records()[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_date_before_the_first_version_is_a_not_found_error() {
        let fixture = Fixture::default();
        fixture.corpus.push_article_versions(Ok(vec![version(1, (2016, 10, 9), None)]));
        let result =
            call(&fixture, &json!({"article_id": arti(1).as_str(), "date_a": "2010-01-01"})).await;
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            "[not_found] No version of LEGIARTI000000000001 is in force on 2010-01-01."
        );
        assert_eq!(fixture.audit.records()[0].error_code, Some(ErrorCode::NotFound));
    }

    #[tokio::test]
    async fn a_port_failure_is_an_in_band_error() {
        let fixture = Fixture::default();
        fixture.corpus.push_article_versions(Err(CorpusError::Timeout));
        let result =
            call(&fixture, &json!({"article_id": arti(1).as_str(), "date_a": "2020-01-01"})).await;
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).starts_with("[upstream_error] "));
    }

    #[tokio::test]
    async fn invalid_arguments_are_audited_invalid_arg_without_network() {
        let fixture = Fixture::default();
        let wrong_type = call(&fixture, &json!({"article_id": 7, "date_a": "2020-01-01"})).await;
        assert_eq!(
            text_of(&wrong_type),
            "[invalid_arg] article_id: invalid type: integer `7`, expected a string"
        );
        let missing = call(&fixture, &json!({"article_id": arti(1).as_str()})).await;
        assert_eq!(text_of(&missing), "[invalid_arg] date_a is required.");
        let bad_date =
            call(&fixture, &json!({"article_id": arti(1).as_str(), "date_a": "07/10/2016"})).await;
        assert!(text_of(&bad_date).starts_with("[invalid_arg] date_a='07/10/2016'"));
        let unknown = call(&fixture, &json!({"article_id": arti(1).as_str(), "foo": 1})).await;
        assert!(text_of(&unknown).contains("unknown field `foo`"));

        let records = fixture.audit.records();
        assert_eq!(records.len(), 4, "une ligne d'audit par appel");
        assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        assert_eq!(fixture.corpus.calls(), 0, "aucun appel au port");
    }

    #[tokio::test]
    async fn the_response_validates_against_the_advertised_output_schema() {
        let fixture = Fixture::default();
        let tool = descriptor(&fixture).await;
        let schema = Value::Object(tool.output_schema.unwrap().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        two_versions(&fixture, "est puni de un an", "est puni de deux ans");
        let result = call(
            &fixture,
            &json!({"article_id": arti(1).as_str(), "date_a": "2020-01-01", "date_b": "2026-01-01"}),
        )
        .await;
        let content = result.structured_content.unwrap();
        assert!(validator.is_valid(&content), "{content}");
    }

    #[tokio::test]
    async fn the_descriptor_follows_the_authoring_rules() {
        let tool = descriptor(&Fixture::default()).await;

        assert_eq!(tool.name, "fr_diff_article");
        assert_eq!(tool.title.as_deref(), Some("Compare article versions"));
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(true));

        let description = tool.description.as_deref().unwrap();
        assert_eq!(description, DESCRIPTION);
        assert!(description.chars().count() <= 1_000);
        let sentences = description.split(". ").count();
        assert!((3..=6).contains(&sentences), "{sentences} phrases");
        for cited in description.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if cited.starts_with("fr_") && !cited.ends_with('_') {
                let known = ToolName::ALL.iter().any(|name| name.as_str() == cited);
                assert!(known, "{cited} n'est pas un outil");
            }
        }

        let input = Value::Object(tool.input_schema.as_ref().clone());
        assert_eq!(input["required"], json!(["article_id", "date_a"]));
        assert!(input.get("$schema").is_none());
        let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
        assert_eq!(output["type"], "object");
        assert!(output.get("$schema").is_none());
        jsonschema::validator_for(&input).unwrap();
        jsonschema::validator_for(&output).unwrap();
    }
}
