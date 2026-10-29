//! Outil `fr_get_related` (05 §5.11) : les liens d'un article ou d'une décision.
//!
//! Le motif est celui de `tools/mod.rs` : arguments bruts (`JsonObject`), décodés par
//! [`decode::to_call`] (l'échec voyage dans l'appel et devient un `invalid_arg` audité), schémas
//! explicites sans `$schema`, jeton d'annulation transmis à la façade.

use lf_application::{
    input::GetRelatedInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject, tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode,
    failure::ToolFailure,
    params::get_related::GetRelatedParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_related::RelatedView,
};

/// Description de l'outil (05 §5.11, 489 caractères, 4 phrases). Source unique : elle ne cite que
/// `fr_resolve`, pour les liens de décision qui n'ont pas d'identifiant.
const DESCRIPTION: &str = "List the legal links of an article (LEGIARTI) or a decision (JURITEXT, \
CONSTEXT, CETATEXT): texts it cites, texts citing it, and case law it refers to. Article links \
come from Légifrance with their ids; decision links are parsed from the decision's references \
and may lack an id (pass them to fr_resolve). Each link has a `relation`, a `citation` and, when \
known, an `id` and `source_url`. At most 100 links are returned: `truncated` is true when there \
are more, and there is no next page.";

#[tool_router(router = get_related_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_get_related",
        title = "Related texts and decisions",
        description = DESCRIPTION,
        input_schema = input_schema::<GetRelatedParams>(),
        output_schema = output_schema::<RelatedView>(),
        annotations(
            title = "Related texts and decisions",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_related(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<RelatedView>, ToolFailure> {
        let call = decode::to_call::<GetRelatedParams, GetRelatedInput>(args);
        let output = self.service.get_related(call, ct.cancelled()).await.map_err(ToolFailure)?;
        Ok(Json(RelatedView::from(output)))
    }
}

// Les tests passent par `testing::connect`, qui n'existe qu'avec la feature `test-support`.
#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests {
    use lf_application::{
        budget::ResponseBudget,
        error::ErrorCode,
        ports::{AuditStatus, CorpusError},
        testing::PortRequest,
        tools::ToolName,
    };
    use lf_domain::{
        entity::{
            ArticleText, Decision, OfficialSummary, ParentText, RelatedLink, RelatedLinks,
            RelatedNature,
        },
        id::{ArticleId, DecisionId, DocumentId},
        text::LongText,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
    use serde_json::{Value, json};

    use super::*;
    use crate::testing::Fixture;

    const ARTI: &str = "LEGIARTI000006419288";
    const JURI: &str = "JURITEXT000051743650";

    fn article() -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTI).unwrap(),
            num: Some("9".to_owned()),
            etat: None,
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
            nota: None,
        }
    }

    /// `citationJp` de `JURITEXT000051743650` (`consult_juri.json`, mojibake réparé).
    const CITATION_JP: &str = "Sur l'interprétation du droit interne à la lumière de la directive \
        85/374/CEE du Conseil du 25 juillet 1985, à rapprocher : 1re Civ., 25 mai 2023, pourvoi \
        n° 21-23.174 (cassation partielle) et l'arrêt cité.";

    /// Une décision JURI qui porte ces références (`citationJp` et titres des liens de citation).
    fn decision(cited: Option<&str>, applied: Vec<String>) -> Decision {
        Decision {
            decision_id: DecisionId::parse(JURI).unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: None,
            nature: None,
            title: Some("Cour de cassation, civile, Chambre civile 1, 4 juin 2025".to_owned()),
            text: LongText::default(),
            official_summary: OfficialSummary::default(),
            cited_case_law: cited.map(str::to_owned),
            applied_texts: applied,
        }
    }

    fn link(nature: RelatedNature, id: Option<&str>, title: &str) -> RelatedLink {
        RelatedLink {
            target: id.and_then(|id| DocumentId::parse(id).ok()),
            nature,
            title: Some(title.to_owned()),
            date: None,
        }
    }

    fn serve_article(fixture: &Fixture, links: RelatedLinks) {
        fixture.corpus.push_get_article(Ok(article()));
        fixture.corpus.push_related_links(Ok(links));
    }

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_get_related");
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    async fn descriptor(fixture: &Fixture) -> Tool {
        let client = fixture.connect().await;
        let tools = client.list_all_tools().await.unwrap();
        tools
            .into_iter()
            .find(|tool| tool.name == "fr_get_related")
            .expect("fr_get_related est enregistré")
    }

    fn text_of(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    #[tokio::test]
    async fn an_article_answers_with_structured_content_and_one_audit_line() {
        let fixture = Fixture::default();
        serve_article(
            &fixture,
            RelatedLinks {
                cites: vec![link(
                    RelatedNature::Code,
                    Some("LEGIARTI000032041571"),
                    "Code civil, art. 1240",
                )],
                cited_by: vec![link(
                    RelatedNature::Loda,
                    None,
                    "Circulaire du 14 septembre 2011 - art.",
                )],
            },
        );

        let result = call(&fixture, &json!({"id": ARTI})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.clone().unwrap();
        assert_eq!(
            content,
            json!({
                "id": ARTI,
                "citation": "Code civil, art. 9",
                "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                "total": 2,
                "links": [
                    {
                        "relation": "cites",
                        "kind": "code_article",
                        "citation": "Code civil, art. 1240",
                        "id": "LEGIARTI000032041571",
                        "source_url": "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000032041571"
                    },
                    {
                        "relation": "cited_by",
                        "kind": "unsupported",
                        "citation": "Circulaire du 14 septembre 2011 - art."
                    }
                ]
            })
        );
        // Le bloc texte porte le même JSON que `structuredContent`.
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), content);

        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_RELATED);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 2, "le nombre de liens");
        let mut requests = fixture.corpus.seen_requests();
        requests.sort_by_key(PortRequest::method);
        let id = ArticleId::parse(ARTI).unwrap();
        assert_eq!(requests, [PortRequest::GetArticle(id.clone()), PortRequest::RelatedLinks(id)]);
    }

    #[tokio::test]
    async fn related_capped_at_100_with_truncated_in_the_json() {
        let fixture = Fixture::default();
        let jorf = |n: u32| link(RelatedNature::JorfCont, None, &format!("Loi n° {n}"));
        serve_article(
            &fixture,
            RelatedLinks { cites: (1..=150).map(jorf).collect(), cited_by: vec![] },
        );

        let result = call(&fixture, &json!({"id": ARTI})).await;

        let content = result.structured_content.unwrap();
        assert_eq!(content["links"].as_array().unwrap().len(), 100);
        assert_eq!(content["total"], 150);
        assert_eq!(content["truncated"], true);
        assert_eq!(fixture.audit.records()[0].output_count_or_size, 100);
    }

    #[tokio::test]
    async fn a_decision_answers_with_the_links_of_its_references_and_one_port_call() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_decision(Ok(decision(
            Some(CITATION_JP),
            vec!["1 du code civil.".to_owned(), "Article 1240 du code civil".to_owned()],
        )));

        let result = call(&fixture, &json!({"id": JURI})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.clone().unwrap();
        // « 1 du code civil. » n'a pas d'ancre : aucun lien. Les liens d'une décision n'ont ni
        // `id` ni `source_url` (ils ne sont pas résolus), et `truncated` est absent.
        assert_eq!(
            content,
            json!({
                "id": JURI,
                "citation": "Cour de cassation, civile, Chambre civile 1, 4 juin 2025",
                "source_url": format!("https://www.legifrance.gouv.fr/juri/id/{JURI}"),
                "total": 3,
                "links": [
                    {
                        "relation": "cites_case_law",
                        "kind": "decision",
                        "citation": "Cass. 1re civ., 25 mai 2023, n° 21-23.174"
                    },
                    {
                        "relation": "applies_text",
                        "kind": "unsupported",
                        "citation": "directive 85/374/CEE du Conseil du 25 juillet 1985"
                    },
                    {
                        "relation": "applies_text",
                        "kind": "code_article",
                        "citation": "Code civil, art. 1240"
                    }
                ]
            })
        );
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), content);

        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_RELATED);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 3, "le nombre de liens");
        assert_eq!(
            fixture.corpus.seen_requests(),
            [PortRequest::GetDecision(DecisionId::parse(JURI).unwrap())]
        );
    }

    #[tokio::test]
    async fn a_decision_without_references_answers_with_an_empty_list() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_decision(Ok(decision(None, vec![])));

        let result = call(&fixture, &json!({"id": JURI})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.unwrap();
        assert_eq!(content["id"], JURI);
        assert_eq!(content["links"], json!([]));
        assert_eq!(content["total"], 0);
        assert!(content.get("truncated").is_none());
        assert_eq!(fixture.corpus.calls(), 1);
    }

    #[tokio::test]
    async fn an_unknown_decision_is_a_not_found_error_of_its_fond() {
        let fixture = Fixture::default();
        fixture
            .corpus
            .push_get_decision(Err(CorpusError::NotFound { what: "/consult/juri".into() }));

        let result = call(&fixture, &json!({"id": JURI})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            "[not_found] No JURI decision found for JURITEXT000051743650. Check the id, or \
             search with fr_search."
        );
        assert_eq!(fixture.audit.records()[0].error_code, Some(ErrorCode::NotFound));
    }

    #[tokio::test]
    async fn a_decision_with_many_references_stays_under_the_hard_cap() {
        // 300 textes appliqués, des plus longs que le parseur retienne (règlements européens) :
        // cent liens au plus, et la réponse mesurée (JSON compact, en caractères) sous 45 000.
        let texts: Vec<String> = (1..=300)
            .map(|n| {
                format!(
                    "Règlement (UE) 2016/{n} du Parlement européen et du Conseil du 27 avril 2016"
                )
            })
            .collect();
        let fixture = Fixture::default();
        fixture.corpus.push_get_decision(Ok(decision(Some(CITATION_JP), texts)));

        let result = call(&fixture, &json!({"id": JURI})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.clone().unwrap();
        let links = content["links"].as_array().unwrap();
        assert_eq!(links.len(), 100);
        assert_eq!(content["total"], 302);
        assert_eq!(content["truncated"], true);
        assert_eq!(links[0]["relation"], "cites_case_law");
        let size = serde_json::to_string(&content).unwrap().chars().count();
        assert!(size <= ResponseBudget::HARD_CAP_CHARS, "{size} caractères");
        let text = text_of(&result).chars().count();
        assert!(text <= ResponseBudget::HARD_CAP_CHARS, "{text} caractères");
        assert_eq!(fixture.audit.records()[0].output_count_or_size, 100);
    }

    #[tokio::test]
    async fn an_unknown_article_is_a_not_found_error() {
        let fixture = Fixture::default();
        fixture
            .corpus
            .push_get_article(Err(CorpusError::NotFound { what: "/consult/getArticle".into() }));
        fixture
            .corpus
            .push_related_links(Err(CorpusError::NotFound { what: "/consult/related".into() }));

        let result = call(&fixture, &json!({"id": ARTI})).await;

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            text_of(&result),
            "[not_found] No article found for LEGIARTI000006419288. Check the id, or find the \
             article with fr_get_act or fr_search."
        );
        assert_eq!(fixture.audit.records()[0].error_code, Some(ErrorCode::NotFound));
    }

    #[tokio::test]
    async fn a_port_failure_is_an_in_band_error() {
        let fixture = Fixture::default();
        fixture.corpus.push_get_article(Err(CorpusError::Timeout));
        fixture.corpus.push_related_links(Err(CorpusError::Timeout));
        let result = call(&fixture, &json!({"id": ARTI})).await;
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).starts_with("[upstream_error] "));

        fixture.corpus.push_get_decision(Err(CorpusError::Timeout));
        let result = call(&fixture, &json!({"id": JURI})).await;
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).starts_with("[upstream_error] "));
    }

    #[tokio::test]
    async fn invalid_arguments_are_audited_invalid_arg_without_network() {
        let fixture = Fixture::default();
        let wrong_type = call(&fixture, &json!({"id": 7})).await;
        assert_eq!(
            text_of(&wrong_type),
            "[invalid_arg] id: invalid type: integer `7`, expected a string"
        );
        let missing = call(&fixture, &json!({})).await;
        assert_eq!(text_of(&missing), "[invalid_arg] id is required.");
        let kaliarti = call(&fixture, &json!({"id": "KALIARTI000049886059"})).await;
        let message = text_of(&kaliarti);
        assert!(
            message.starts_with("[invalid_arg] id='KALIARTI000049886059' must be a LEGIARTI"),
            "{message}"
        );
        let unknown = call(&fixture, &json!({"id": ARTI, "foo": 1})).await;
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

        serve_article(
            &fixture,
            RelatedLinks {
                cites: vec![link(RelatedNature::Code, Some(ARTI), "Code civil, art. 9")],
                cited_by: (1..=150)
                    .map(|n| link(RelatedNature::JorfCont, None, &format!("Loi n° {n}")))
                    .collect(),
            },
        );
        let result = call(&fixture, &json!({"id": ARTI})).await;
        let content = result.structured_content.unwrap();
        assert!(validator.is_valid(&content), "{content}");
        // Le cas vide et le cas tronqué passent aussi.
        serve_article(&fixture, RelatedLinks::default());
        let empty = call(&fixture, &json!({"id": ARTI})).await.structured_content.unwrap();
        assert!(validator.is_valid(&empty), "{empty}");
        // Une décision, avec et sans références.
        fixture.corpus.push_get_decision(Ok(decision(Some(CITATION_JP), vec![])));
        let linked = call(&fixture, &json!({"id": JURI})).await.structured_content.unwrap();
        assert!(validator.is_valid(&linked), "{linked}");
        fixture.corpus.push_get_decision(Ok(decision(None, vec![])));
        let bare = call(&fixture, &json!({"id": JURI})).await.structured_content.unwrap();
        assert!(validator.is_valid(&bare), "{bare}");
    }

    #[tokio::test]
    async fn the_descriptor_follows_the_authoring_rules() {
        let tool = descriptor(&Fixture::default()).await;

        assert_eq!(tool.name, "fr_get_related");
        assert_eq!(tool.title.as_deref(), Some("Related texts and decisions"));
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(true));

        let description = tool.description.as_deref().unwrap();
        assert_eq!(description, DESCRIPTION);
        assert_eq!(description.chars().count(), 489, "05 §5.11");
        let sentences = description.split(". ").count();
        assert!((3..=6).contains(&sentences), "{sentences} phrases");
        // Les outils cités existent : `ToolName::ALL` ne compte que ceux déjà enregistrés, `fr_resolve`
        // s'y ajoute à l'intégration de M6.
        let known = [ToolName::FR_RESOLVE];
        for cited in description.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if cited.starts_with("fr_") && !cited.ends_with('_') {
                let registered =
                    ToolName::ALL.iter().chain(&known).any(|name| name.as_str() == cited);
                assert!(registered, "{cited} n'est pas un outil");
            }
        }
        assert!(!description.contains("fr_verify_citations"));

        let input = Value::Object(tool.input_schema.as_ref().clone());
        assert_eq!(input["required"], json!(["id"]));
        assert!(input.get("$schema").is_none());
        let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
        assert_eq!(output["type"], "object");
        assert!(output.get("$schema").is_none());
        jsonschema::validator_for(&input).unwrap();
        jsonschema::validator_for(&output).unwrap();
    }
}
