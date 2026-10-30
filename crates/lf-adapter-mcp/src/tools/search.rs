//! Outil `fr_search` (05 §5.1) : recherche par mots-clés dans un fond.
//!
//! Le motif est celui de `tools/mod.rs` : arguments bruts (`JsonObject`), décodés par
//! [`decode::to_call`] (l'échec voyage dans l'appel et devient un `invalid_arg` audité), schémas
//! explicites sans `$schema`, jeton d'annulation transmis à la façade.

use lf_application::{
    input::SearchInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode,
    failure::ToolFailure,
    params::search::SearchParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::search::SearchView,
};

/// Description de l'outil (05 §5.1, 5 phrases). Source unique : elle ne cite que des outils de la
/// surface v0.1.
const DESCRIPTION: &str = "Keyword search in one Légifrance fond (default LODA): laws and \
decrees (LODA), codes (CODE), case law (JURI, CONSTIT, CETAT), CNIL deliberations (CNIL), \
collective agreements (KALI) or company agreements (ACCO). Returns one page of hits (default 10) \
with id, title, snippet, citation and source_url; ask for the next `page` while `has_more` is \
true. `match` defaults to any word; use `all` or `exact` for precise queries, and \
`date_from`/`date_to` to filter by date. LODA and CODE hits are the versions in force on \
`version_date` (default: today in Paris). Open a hit with the fr_get_* tool for its id prefix \
(LEGITEXT: fr_get_act, JURITEXT/CONSTEXT/CETATEXT: fr_get_decision, CNILTEXT: \
fr_get_deliberation, KALITEXT: fr_get_convention, ACCOTEXT: fr_get_company_agreement); CODE hits \
also list matching `articles` for fr_get_text.";

#[tool_router(router = search_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_search",
        title = "Search Légifrance",
        description = DESCRIPTION,
        input_schema = input_schema::<SearchParams>(),
        output_schema = output_schema::<SearchView>(),
        annotations(
            title = "Search Légifrance",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_search(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<SearchView>, ToolFailure> {
        let call = decode::to_call::<SearchParams, SearchInput>(args);
        let output = self.service.search(call, ct.cancelled()).await.map_err(ToolFailure)?;
        Ok(Json(SearchView::from(output)))
    }
}

// Les tests passent par `testing::connect`, qui n'existe qu'avec la feature `test-support`.
#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests {
    use lf_application::{error::ErrorCode, ports::AuditStatus, tools::ToolName};
    use lf_domain::{
        date::CivilDate,
        entity::{CodeArticleRef, SearchHit, SearchPage, Snippet},
        fond::{Fond, PaginationMode},
        id::{ArticleId, DocumentId},
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
    use serde_json::{Value, json};

    use super::*;
    use crate::testing::Fixture;

    /// Un résultat JURI avec un extrait de 280 caractères.
    fn juri_hit(n: u32) -> SearchHit {
        let id = DocumentId::parse(&format!("JURITEXT{n:012}")).unwrap();
        SearchHit {
            title: Some(format!("Cour de cassation, chambre civile 1, {n}")),
            date: CivilDate::new(2024, 1, 2),
            nature: Some("ARRET".to_owned()),
            snippet: Snippet::new(&"mot ".repeat(70)),
            ..SearchHit::new(Fond::Juri, id, None).unwrap()
        }
    }

    fn juri_page(hits: u32, total: u64) -> SearchPage {
        SearchPage::new(
            Fond::Juri,
            "bail",
            Some(total),
            None,
            (1..=hits).map(juri_hit).collect(),
            0,
            PaginationMode::Defaut,
        )
    }

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_search");
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    async fn descriptor(fixture: &Fixture) -> Tool {
        let client = fixture.connect().await;
        let tools = client.list_all_tools().await.unwrap();
        tools.into_iter().find(|tool| tool.name == "fr_search").expect("fr_search est enregistré")
    }

    fn text_of(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    #[tokio::test]
    async fn a_search_answers_with_structured_content_and_one_audit_line() {
        let fixture = Fixture::default();
        fixture.corpus.push_search(Ok(juri_page(2, 51_531)));

        let result =
            call(&fixture, &json!({"query": "bail", "fond": "juri", "page_size": 2})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.clone().unwrap();
        assert_eq!(content["total"], 51_531);
        assert_eq!(content["page"], 1);
        assert_eq!(content["has_more"], true);
        let hits = content["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0]["kind"], "juri");
        assert_eq!(hits[0]["id"], "JURITEXT000000000001");
        assert_eq!(hits[0]["date"], "2024-01-02");
        assert!(hits[0]["citation"].as_str().unwrap().contains("Cour de cassation"));
        assert!(
            hits[0]["source_url"].as_str().unwrap().starts_with("https://www.legifrance.gouv.fr/")
        );
        // Le bloc texte porte le même JSON que `structuredContent`.
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), content);

        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_SEARCH);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 2);
        assert_eq!(
            records[0].input.as_str(),
            r#"{"fond":"juri","page_size":2,"query":"bail"}"#,
            "l'empreinte porte sur les arguments bruts"
        );
        let requests = fixture.corpus.seen_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method(), "search");
    }

    #[tokio::test]
    async fn an_empty_search_is_a_success() {
        let fixture = Fixture::default();
        fixture.corpus.push_search(Ok(SearchPage::new(
            Fond::Loda,
            "zzz",
            Some(0),
            None,
            vec![],
            0,
            PaginationMode::Defaut,
        )));
        let result = call(&fixture, &json!({"query": "zzz"})).await;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result.structured_content.unwrap(),
            json!({"total": 0, "page": 1, "has_more": false, "hits": []})
        );
    }

    #[tokio::test]
    async fn snippets_removed_from_last_hits_when_over_cap() {
        let fixture = Fixture::default();
        // 60 résultats de 280 caractères d'extrait : près de 58 000 caractères estimés.
        fixture.corpus.push_search(Ok(juri_page(60, 5_000)));

        let result =
            call(&fixture, &json!({"query": "bail", "fond": "JURI", "page_size": 60})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.unwrap();
        let compact = serde_json::to_string(&content).unwrap();
        let length = compact.chars().count();
        assert!(length <= 45_000, "{length}");

        let hits = content["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 60, "aucun résultat n'est retiré");
        let without: Vec<bool> = hits.iter().map(|hit| hit.get("snippet").is_none()).collect();
        let removed = without.iter().filter(|gone| **gone).count();
        assert!(removed > 0 && removed < 60, "{removed}");
        // En partant du dernier, sans trou : les premiers gardent leur extrait.
        let first_removed = 60 - removed;
        assert!(without[..first_removed].iter().all(|gone| !gone));
        assert!(without[first_removed..].iter().all(|gone| *gone));
        assert_eq!(
            content["note"],
            format!(
                "Snippets were removed from the last {removed} hits to keep the response under \
                 45,000 characters; use a smaller page_size to get them."
            )
        );
        // La citation et l'identifiant restent sur les résultats dégradés.
        assert!(hits[59]["citation"].as_str().unwrap().contains("Cour de cassation"));
        assert_eq!(hits[59]["id"], "JURITEXT000000000060");
        // L'audit compte les résultats renvoyés.
        assert_eq!(fixture.audit.records()[0].output_count_or_size, 60);
    }

    #[tokio::test]
    async fn a_code_hit_lists_its_articles() {
        let fixture = Fixture::default();
        let code = SearchHit {
            title: Some("Code civil".to_owned()),
            nature: Some("code".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            articles: vec![CodeArticleRef::new(
                ArticleId::parse("LEGIARTI000006419288").unwrap(),
                Some("9".to_owned()),
                "Chacun a droit au respect de sa vie privée.",
            )],
            ..SearchHit::new(Fond::Code, DocumentId::parse("LEGITEXT000006070721").unwrap(), None)
                .unwrap()
        };
        fixture.corpus.push_search(Ok(SearchPage::new(
            Fond::Code,
            "vie privée",
            Some(1),
            Some(4),
            vec![code],
            0,
            PaginationMode::Defaut,
        )));

        let result = call(&fixture, &json!({"query": "vie privée", "fond": "CODE"})).await;

        let content = result.structured_content.unwrap();
        let hit = &content["hits"][0];
        assert_eq!(hit["kind"], "code");
        assert_eq!(hit["etat"], "VIGUEUR");
        assert_eq!(hit["citation"], "Code civil");
        assert!(hit.get("title").is_none(), "le titre est déjà la citation");
        assert_eq!(
            hit["articles"],
            json!([{
                "article_id": "LEGIARTI000006419288",
                "num": "9",
                "snippet": "Chacun a droit au respect de sa vie privée."
            }])
        );
        assert_eq!(
            hit["source_url"],
            "https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006070721"
        );
    }

    #[tokio::test]
    async fn the_response_validates_against_the_advertised_output_schema() {
        let fixture = Fixture::default();
        let tool = descriptor(&fixture).await;
        let schema = Value::Object(tool.output_schema.unwrap().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        fixture.corpus.push_search(Ok(juri_page(3, 30)));
        let result = call(&fixture, &json!({"query": "bail", "fond": "JURI"})).await;
        let content = result.structured_content.unwrap();
        assert!(validator.is_valid(&content), "{content}");
    }

    #[tokio::test]
    async fn schema_has_enum_for_fond_and_bounds_for_page_size() {
        let tool = descriptor(&Fixture::default()).await;

        assert_eq!(tool.name, "fr_search");
        assert_eq!(tool.title.as_deref(), Some("Search Légifrance"));
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(true));

        let input = Value::Object(tool.input_schema.as_ref().clone());
        assert_eq!(input["type"], "object");
        assert_eq!(input["additionalProperties"], false);
        assert_eq!(input["required"], json!(["query"]));
        assert!(input.get("$schema").is_none());
        let props = &input["properties"];
        assert_eq!(
            props["fond"]["enum"],
            json!(["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"])
        );
        assert_eq!(props["match"]["enum"], json!(["any", "all", "exact"]));
        assert_eq!(props["page_size"]["minimum"], 1);
        assert_eq!(props["page_size"]["maximum"], 100);
        assert_eq!(props["page"]["minimum"], 1);
        assert_eq!(props["query"]["maxLength"], 450);

        let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
        assert_eq!(output["type"], "object");
        assert!(output.get("$schema").is_none());
        // Les deux schémas sont du JSON Schema valide.
        jsonschema::validator_for(&input).unwrap();
        jsonschema::validator_for(&output).unwrap();
    }

    #[tokio::test]
    async fn the_description_follows_the_authoring_rules() {
        let tool = descriptor(&Fixture::default()).await;
        let description = tool.description.as_deref().unwrap();
        assert_eq!(description, DESCRIPTION);
        let length = description.chars().count();
        assert!(length <= 1_000, "{length}");
        let sentences = description.split(". ").count();
        assert!((3..=6).contains(&sentences), "{sentences} phrases");
        // Seuls des outils de la surface v0.1 sont cités : les huit premiers de `ToolName::ALL`.
        let v01 = &ToolName::ALL[..8];
        for cited in description.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if cited.starts_with("fr_") && !cited.ends_with('_') {
                let known = v01.iter().any(|name| name.as_str() == cited);
                assert!(known, "{cited} n'est pas un outil");
            }
        }
        assert!(!description.contains("fr_search_all"));
    }

    #[tokio::test]
    async fn invalid_type_is_audited_invalid_arg() {
        let fixture = Fixture::default();

        let result = call(&fixture, &json!({"query": 42})).await;

        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.is_none());
        assert_eq!(
            text_of(&result),
            "[invalid_arg] query: invalid type: integer `42`, expected a string"
        );
        let records = fixture.audit.records();
        assert_eq!(records.len(), 1, "une seule ligne d'audit");
        assert_eq!(records[0].tool, ToolName::FR_SEARCH);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(
            records[0].error_code.map(ErrorCode::as_str),
            Some("invalid_arg"),
            "rejet audité en invalid_arg"
        );
        assert_eq!(records[0].input.as_str(), r#"{"query":42}"#);
        assert_eq!(fixture.corpus.calls(), 0, "aucun appel au port");
    }

    #[tokio::test]
    async fn an_unknown_argument_and_a_missing_query_are_invalid_arg_without_network() {
        let fixture = Fixture::default();
        let unknown = call(&fixture, &json!({"query": "bail", "foo": 1})).await;
        assert_eq!(unknown.is_error, Some(true));
        let unknown_text = text_of(&unknown);
        assert!(unknown_text.starts_with("[invalid_arg] "), "{unknown_text}");
        assert!(unknown_text.contains("unknown field `foo`"), "{unknown_text}");

        let missing = call(&fixture, &json!({})).await;
        assert_eq!(text_of(&missing), "[invalid_arg] query is required.");

        let order = call(&fixture, &json!({"query": "bail", "fond": "NOPE"})).await;
        let order_text = text_of(&order);
        assert!(order_text.starts_with("[invalid_arg] "), "{order_text}");

        assert_eq!(fixture.audit.records().len(), 3);
        assert_eq!(fixture.corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_port_failure_is_an_in_band_error() {
        let fixture = Fixture::default();
        fixture.corpus.push_search(Err(lf_application::ports::CorpusError::Timeout));
        let result = call(&fixture, &json!({"query": "bail"})).await;
        assert_eq!(result.is_error, Some(true));
        let text = text_of(&result);
        assert!(text.starts_with("[upstream_error] "), "{text}");
        assert_eq!(fixture.audit.records().len(), 1);
    }
}
