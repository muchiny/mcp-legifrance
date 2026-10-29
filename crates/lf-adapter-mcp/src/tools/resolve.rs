//! Outil `fr_resolve` (05 §5.10) : la transformation d'une citation libre en identifiants
//! Légifrance, sans jamais deviner.
//!
//! Le motif est celui de `tools/mod.rs` : arguments bruts (`JsonObject`), décodés par
//! [`decode::to_call`] (l'échec voyage dans l'appel et devient un `invalid_arg` audité), schémas
//! explicites sans `$schema`, jeton d'annulation transmis à la façade. La vue ramène elle-même la
//! réponse sous le plafond dur de 45 000 caractères (05 §6.4).

use lf_application::{
    input::ResolveInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode,
    failure::ToolFailure,
    params::resolve::ResolveParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::resolve::ResolveView,
};

/// Description de l'outil (586 caractères, 4 phrases ; 05 §5.10). « for example » et non « e.g. » :
/// le contrôle de `xtask` compte chaque point suivi d'une espace pour une fin de phrase. Elle cite
/// `fr_verify_citations`, pour le texte entier, et le motif `fr_get_*` des outils de lecture.
pub(crate) const DESCRIPTION: &str = "Turn one free-text legal citation into Légifrance ids, for example \"article 1240 du code civil\", \"loi n° 78-17 du 6 janvier 1978\", \"Cass. civ. 1re, 4 juin 2025, n° 24-13.470\" or an ECLI. Returns `candidates` (id, kind, citation, source_url, confidence high/medium/low), best first; nothing is guessed or fabricated, and an empty list comes with a `note` saying why. Pass `date` (YYYY-MM-DD, default: today in Paris) for the article version in force on that day. Then open a candidate with the fr_get_* tool for its kind; to check all citations of a text at once use fr_verify_citations.";

#[tool_router(router = resolve_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_resolve",
        title = "Resolve a legal citation",
        description = DESCRIPTION,
        input_schema = input_schema::<ResolveParams>(),
        output_schema = output_schema::<ResolveView>(),
        annotations(
            title = "Resolve a legal citation",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_resolve(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<ResolveView>, ToolFailure> {
        let call = decode::to_call::<ResolveParams, ResolveInput>(args);
        let output = self.service.resolve(call, ct.cancelled()).await?;
        Ok(Json(ResolveView::from(output)))
    }
}

// Les tests passent par `testing::connect`, qui n'existe qu'avec la feature `test-support`.
#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests {
    use lf_application::{
        error::ErrorCode,
        ports::{ArticleAt, AuditStatus, CorpusError},
        testing::PortRequest,
        tools::ToolName,
    };
    use lf_domain::{
        date::{CivilDate, VersionDate},
        entity::{ArticleText, CodeListing, SearchHit},
        fond::Fond,
        id::{ArticleId, DocumentId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
    use serde_json::{Value, json};

    use super::*;
    use crate::testing::Fixture;

    const CIVIL: &str = "LEGITEXT000006070721";

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_resolve");
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    async fn descriptor(fixture: &Fixture) -> Tool {
        let client = fixture.connect().await;
        let tools = client.list_all_tools().await.unwrap();
        tools.into_iter().find(|tool| tool.name == "fr_resolve").expect("fr_resolve est enregistré")
    }

    fn text_of(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    fn civil_code_listed(fixture: &Fixture) {
        fixture.corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse(CIVIL).unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
    }

    fn article_9() -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse("LEGIARTI000006419288").unwrap(),
            num: Some("9".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: CivilDate::new(2016, 10, 1),
            date_fin: None,
            text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
            nota: None,
        }
    }

    fn texts(count: u32) -> Vec<SearchHit> {
        (1..=count)
            .map(|n| {
                let id = DocumentId::parse(&format!("LEGITEXT{n:012}")).unwrap();
                SearchHit::new(Fond::Loda, id, None).unwrap()
            })
            .collect()
    }

    #[tokio::test]
    async fn interpreted_as_present() {
        let fixture = Fixture::default();
        civil_code_listed(&fixture);
        fixture.corpus.push_find_article(Ok(article_9()));

        let result = call(&fixture, &json!({"citation": "art. 9 C. civ."})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.clone().unwrap();
        assert_eq!(
            content,
            json!({
                "interpreted_as": "Code civil, art. 9",
                "candidates": [{
                    "id": "LEGIARTI000006419288",
                    "kind": "article",
                    "citation": "Code civil, art. 9",
                    "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                    "confidence": "high"
                }],
                "note": "Current version in force since 2016-10-01."
            })
        );
        // Le bloc texte porte le même JSON que `structuredContent`.
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), content);

        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_RESOLVE);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 1);
    }

    #[tokio::test]
    async fn at_most_5_candidates_best_first() {
        let fixture = Fixture::default();
        fixture.corpus.push_find_by_number(Ok(texts(8)));

        let result = call(&fixture, &json!({"citation": "loi n° 2016-1321"})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.unwrap();
        let candidates = content["candidates"].as_array().unwrap();
        assert_eq!(candidates.len(), 5);
        let ids: Vec<&str> = candidates.iter().map(|c| c["id"].as_str().unwrap()).collect();
        let expected: Vec<String> = (1..=5).map(|n| format!("LEGITEXT{n:012}")).collect();
        assert_eq!(ids, expected);
        assert!(candidates.iter().all(|c| c["confidence"] == "low" && c["kind"] == "loda"));
        assert!(content.get("note").is_none(), "{content}");
    }

    #[tokio::test]
    async fn date_defaults_today_paris() {
        // Sans `date`, le jour de l'horloge de Paris de la fixture ; avec `date`, ce jour-là.
        let fixture = Fixture { today: CivilDate::new(2027, 1, 5).unwrap(), ..Fixture::default() };
        for (arguments, expected) in [
            (
                json!({"citation": "art. 9 C. civ."}),
                ArticleAt::Today(VersionDate::parse("2027-01-05").unwrap()),
            ),
            (
                json!({"citation": "art. 9 C. civ.", "date": "2027-01-05"}),
                ArticleAt::Today(VersionDate::parse("2027-01-05").unwrap()),
            ),
            (
                json!({"citation": "art. 9 C. civ.", "date": "2016-09-30"}),
                ArticleAt::On(VersionDate::parse("2016-09-30").unwrap()),
            ),
        ] {
            civil_code_listed(&fixture);
            fixture.corpus.push_find_article(Ok(article_9()));
            let result = call(&fixture, &arguments).await;
            assert_eq!(result.is_error, Some(false), "{arguments}");
            let seen = fixture.corpus.seen_requests();
            assert_eq!(
                seen.last(),
                Some(&PortRequest::FindArticle(
                    LegiTextId::parse(CIVIL).unwrap(),
                    "9".to_owned(),
                    expected
                )),
                "{arguments}"
            );
        }
    }

    #[tokio::test]
    async fn nothing_found_is_a_success_with_an_empty_list_and_the_reason() {
        // L'environnement de la fixture est le bac à sable : jamais `not_found`.
        let fixture = Fixture::default();
        civil_code_listed(&fixture);
        fixture.corpus.push_find_article(Err(CorpusError::NotFound { what: "x".to_owned() }));

        let result = call(&fixture, &json!({"citation": "article 9999 du code civil"})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.unwrap();
        assert_eq!(content["candidates"], json!([]));
        assert_eq!(content["interpreted_as"], "Code civil, art. 9999");
        assert!(content["note"].as_str().unwrap().contains("sandbox data differ"), "{content}");
        assert_eq!(fixture.audit.records()[0].status, AuditStatus::Ok);
    }

    #[tokio::test]
    async fn invalid_arguments_are_audited_as_invalid_arg() {
        let fixture = Fixture::default();
        for (arguments, message) in [
            (json!({}), "[invalid_arg] citation is required."),
            (
                json!({"citation": "art. 9 C. civ.", "date": "2016-13-45"}),
                "[invalid_arg] date='2016-13-45' is not a valid date (expected YYYY-MM-DD, e.g. \
                 2026-10-15).",
            ),
            (
                json!({"citation": "art. 9 C. civ.", "date": 20_161_001}),
                "[invalid_arg] date: invalid type: integer `20161001`, expected a string",
            ),
        ] {
            let result = call(&fixture, &arguments).await;
            assert_eq!(result.is_error, Some(true), "{arguments}");
            assert_eq!(text_of(&result), message);
            assert!(result.structured_content.is_none());
        }
        assert_eq!(fixture.corpus.calls(), 0);
        let records = fixture.audit.records();
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        assert!(records.iter().all(|record| record.output_count_or_size == 0));
    }

    #[tokio::test]
    async fn an_upstream_failure_fails_the_call() {
        let fixture = Fixture::default();
        civil_code_listed(&fixture);
        fixture.corpus.push_find_article(Err(CorpusError::Timeout));
        let result = call(&fixture, &json!({"citation": "article 9 du code civil"})).await;
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).starts_with("[upstream_error] "), "{}", text_of(&result));
        assert_eq!(fixture.audit.records()[0].error_code, Some(ErrorCode::UpstreamError));
    }

    #[tokio::test]
    async fn the_response_validates_against_the_output_schema() {
        let fixture = Fixture::default();
        let tool = descriptor(&fixture).await;
        let schema = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        civil_code_listed(&fixture);
        fixture.corpus.push_find_article(Ok(article_9()));
        let found = call(&fixture, &json!({"citation": "art. 9 C. civ."})).await;
        let content = found.structured_content.unwrap();
        assert!(validator.is_valid(&content), "{content}");
        // Le cas vide passe aussi.
        let empty = call(&fixture, &json!({"citation": "bonjour"})).await;
        let content = empty.structured_content.unwrap();
        assert!(validator.is_valid(&content), "{content}");
    }

    #[tokio::test]
    async fn the_descriptor_follows_the_authoring_rules() {
        let tool = descriptor(&Fixture::default()).await;

        assert_eq!(tool.name, "fr_resolve");
        assert_eq!(tool.title.as_deref(), Some("Resolve a legal citation"));
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(true));

        let description = tool.description.as_deref().unwrap();
        assert_eq!(description, DESCRIPTION);
        assert_eq!(description.chars().count(), 586, "05 §5.10");
        // Une phrase finit par un point suivi d'une espace et d'une majuscule (« Cass. civ. » non).
        let chars: Vec<char> = description.chars().collect();
        let boundaries =
            chars.windows(3).filter(|w| w[0] == '.' && w[1] == ' ' && w[2].is_uppercase()).count();
        assert!((3..=6).contains(&(boundaries + 1)), "{} phrases", boundaries + 1);
        // Les outils cités existent.
        for cited in description.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if cited.starts_with("fr_") && !cited.ends_with('_') {
                let registered = ToolName::ALL.iter().any(|name| name.as_str() == cited);
                assert!(registered, "{cited} n'est pas un outil");
            }
        }
        assert!(description.contains("fr_get_*") && description.contains("fr_verify_citations"));

        let input = Value::Object(tool.input_schema.as_ref().clone());
        assert_eq!(input["required"], json!(["citation"]));
        assert!(input.get("$schema").is_none());
        let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
        assert_eq!(output["type"], "object");
        assert_eq!(output["required"], json!(["candidates"]));
        assert!(output.get("$schema").is_none());
        jsonschema::validator_for(&input).unwrap();
        jsonschema::validator_for(&output).unwrap();
    }
}
