//! Outil `fr_search_judilibre` (05 §5.18) : la recherche dans les décisions de justice de
//! Judilibre (open data de la Cour de cassation).
//!
//! La source n'est pas Légifrance : c'est le port `JudicialOpenData`, servi par
//! `lf-adapter-piste` avec le même jeton PISTE (ADR-0035). Motif de `tools/mod.rs` : arguments bruts
//! (`JsonObject`), décodage par `to_call`, appel de la façade avec le jeton d'annulation de la
//! requête, vue de sortie à schéma explicite.

use lf_application::{
    input::SearchJudilibreInput,
    ports::{AuditLog, Clock, JudicialOpenData, LegalCorpus, PublicGuidance, TaxDoctrine},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::search_judilibre::SearchJudilibreParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::search_judilibre::CaseLawSearchView,
};

/// Description du descripteur, mot pour mot (05 §5.18, 621 caractères, 4 phrases, sans adresse ;
/// « for example » plutôt que « e.g. », AM-63). Elle dit ce que couvre la source (bien au-delà de
/// la Cour de cassation), ce que rend la recherche (jamais le texte) et l'interdiction de profiler
/// les magistrats et les greffiers (ADR-0035, décision 7).
pub(crate) const DESCRIPTION: &str = "Search Judilibre, the open data of French judicial court decisions published by the Cour de cassation: the Cour de cassation itself, but also courts of appeal, judicial and commercial courts and labour courts, most of which are not on Légifrance. `courts` defaults to all of them; `part` limits the search to one part of the decision, for example the reasons (motivations), which is slower. Returns ids to read with fr_get_judilibre, with court, seat, chamber and date, never the full text. Decisions are pseudonymised; French law forbids using them to evaluate, compare or predict the practice of a judge or court clerk.";

#[tool_router(router = search_judilibre_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance, J: JudicialOpenData>
    LegifranceServer<C, A, K, D, G, J>
{
    #[tool(
        name = "fr_search_judilibre",
        title = "Search Judilibre court decisions",
        description = DESCRIPTION,
        input_schema = input_schema::<SearchJudilibreParams>(),
        output_schema = output_schema::<CaseLawSearchView>(),
        annotations(
            title = "Search Judilibre court decisions",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_search_judilibre(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<CaseLawSearchView>, ToolFailure> {
        let call = to_call::<SearchJudilibreParams, SearchJudilibreInput>(args);
        let output = self.service.search_judilibre(call, ct.cancelled()).await?;
        Ok(Json(CaseLawSearchView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 621);
        assert!(DESCRIPTION.starts_with("Search Judilibre, the open data of French judicial"));
        assert!(DESCRIPTION.contains("most of which are not on Légifrance"));
        assert!(DESCRIPTION.contains("to read with fr_get_judilibre"));
        assert!(DESCRIPTION.contains("never the full text"));
        assert!(
            DESCRIPTION
                .ends_with("evaluate, compare or predict the practice of a judge or court clerk.")
        );
        // Aucune adresse : seuls les liens vers Légifrance sont admis dans un descripteur.
        assert!(!DESCRIPTION.contains("http"));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use std::time::Duration;

        use lf_application::{
            error::ErrorCode,
            messages::JUDILIBRE_DISABLED_MESSAGE,
            ports::{AuditStatus, CaseLawError},
            testing::CaseLawRequest,
            tools::ToolName,
        };
        use std::num::NonZeroU32;

        use lf_domain::{
            judilibre::{
                CaseLawFilters, CaseLawHit, CaseLawMention, CaseLawPage, CaseLawQuery, CaseLawSort,
                Court, CourtSet, JudilibreId,
            },
            search::{MatchMode, PageSize, QueryText},
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::{
            testing::Fixture,
            views::search_judilibre::{ATTRIBUTION, CASE_LAW_NOTE},
        };

        fn hit(raw: &str) -> CaseLawHit {
            CaseLawHit {
                id: JudilibreId::parse(raw).unwrap(),
                jurisdiction: Court::Appeal,
                mention: CaseLawMention {
                    court: "Cour d'appel de Paris".to_owned(),
                    chamber: Some("Pôle 3 - Chambre 1".to_owned()),
                    formation: None,
                    date: "2023-01-18".to_owned(),
                },
                number: Some("20/17369".to_owned()),
                ecli: None,
                solution: Some("Infirmation".to_owned()),
                publication: None,
                summary: None,
                excerpt: Some("la donation consentie entre époux".to_owned()),
                themes: vec!["Donation".to_owned()],
            }
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_search_judilibre");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_search_judilibre").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Search Judilibre court decisions"));
            assert_eq!(tool.description.as_deref(), Some(super::super::DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(true));
            let input = Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(input["required"], json!(["query"]));
            assert_eq!(input["additionalProperties"], false);
            // Aucun paramètre ne désigne une personne, une chambre, une formation ni un siège.
            for forbidden in ["chamber", "formation", "location", "judge", "theme"] {
                assert!(input["properties"].get(forbidden).is_none(), "{forbidden}");
            }
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert!(output["properties"]["hits"]["items"]["properties"].get("text").is_none());
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn a_search_returns_the_structured_page_and_is_audited() {
            let fixture = Fixture::default();
            let page = CaseLawPage {
                total: 120,
                hits: vec![hit("63c8eefddc5b777c90992fd8"), hit("63c8eefddc5b777c90992fd9")],
            };
            fixture.case_law.push_search(Ok(page));
            let client = fixture.connect().await;
            let arguments = json!({"query": "donation", "courts": ["CA"], "page_size": 2});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["total"], 120);
            assert_eq!(structured["has_more"], true);
            assert_eq!(structured["hits"].as_array().unwrap().len(), 2);
            assert_eq!(
                structured["hits"][0]["citation"],
                "Cour d'appel de Paris, Pôle 3 - Chambre 1, 18 janvier 2023, n° 20/17369"
            );
            assert_eq!(structured["attribution"], ATTRIBUTION);
            assert_eq!(structured["case_law_note"], CASE_LAW_NOTE);
            assert_eq!(structured["consulted_on"], "2026-10-29");
            // Le bloc texte est le même JSON (compatibilité des clients sans structuredContent).
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);

            // Le port reçoit une requête typée : la seule cour d'appel demandée, les autres
            // filtres par défaut.
            let filters = CaseLawFilters {
                mode: MatchMode::All,
                courts: CourtSet::of([Court::Appeal]).unwrap(),
                part: None,
                date_from: None,
                date_to: None,
                sort: CaseLawSort::Relevance,
            };
            let expected = CaseLawQuery::new(
                QueryText::parse("donation").unwrap(),
                filters,
                NonZeroU32::MIN,
                PageSize::new(2).unwrap(),
            )
            .unwrap();
            assert_eq!(fixture.case_law.seen_requests(), [CaseLawRequest::Search(expected)]);

            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_SEARCH_JUDILIBRE);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(records[0].output_count_or_size, 2);
            // Judilibre ne passe jamais par le corpus Légifrance ni par les autres sources.
            assert_eq!(
                (fixture.corpus.calls(), fixture.doctrine.calls(), fixture.guidance.calls()),
                (0, 0, 0)
            );
        }

        #[tokio::test]
        async fn a_source_failure_is_an_in_band_error_that_names_no_word_of_the_query() {
            let fixture = Fixture::default();
            fixture.case_law.push_search(Err(CaseLawError::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            }));
            fixture.case_law.push_search(Err(CaseLawError::NotSubscribed));
            fixture.case_law.push_search(Err(CaseLawError::Rejected { status: Some(416) }));
            let client = fixture.connect().await;

            let mut texts = Vec::new();
            for _ in 0..3 {
                let result =
                    client.call_tool(request(&json!({"query": "zzsentinellezz"}))).await.unwrap();
                assert_eq!(result.is_error, Some(true));
                assert!(result.structured_content.is_none());
                texts.push(result.content[0].as_text().unwrap().text.clone());
            }
            let prefixes: Vec<_> =
                texts.iter().map(|text| text.split_once(' ').map(|(code, _)| code)).collect();
            assert_eq!(
                prefixes,
                [Some("[rate_limited]"), Some("[config_error]"), Some("[upstream_error]")],
                "{texts:?}"
            );
            assert!(texts.iter().all(|text| !text.contains("zzsentinellezz")));

            let codes: Vec<_> =
                fixture.audit.records().iter().map(|record| record.error_code).collect();
            assert_eq!(
                codes,
                [
                    Some(ErrorCode::RateLimited),
                    Some(ErrorCode::ConfigError),
                    Some(ErrorCode::UpstreamError)
                ]
            );
        }

        #[tokio::test]
        async fn bad_arguments_are_an_audited_invalid_arg_without_a_port_call() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            for arguments in [
                json!({}),
                json!({"query": 42}),
                json!({"query": "bail", "courts": []}),
                json!({"query": "bail", "courts": ["ce"]}),
                json!({"query": "bail", "part": "judges"}),
                json!({"query": "bail", "chamber": "civ1"}),
                json!({"query": "bail", "page_size": 51}),
                json!({"query": "bail", "page": 201, "page_size": 50}),
            ] {
                let result = client.call_tool(request(&arguments)).await.unwrap();
                assert_eq!(result.is_error, Some(true), "{arguments}");
                let text = &result.content[0].as_text().unwrap().text;
                assert!(text.starts_with("[invalid_arg] "), "{arguments}: {text}");
            }
            assert_eq!(fixture.case_law.calls(), 0);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 8);
            assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        }

        #[tokio::test]
        async fn a_server_without_judilibre_says_so() {
            let mut fixture = Fixture::default();
            fixture.settings.configuration.judilibre =
                lf_application::service::JudilibreSource::Off;
            fixture.case_law.push_search(Err(CaseLawError::Disabled));
            let client = fixture.connect().await;
            let result = client.call_tool(request(&json!({"query": "bail"}))).await.unwrap();
            assert_eq!(
                result.content[0].as_text().unwrap().text,
                format!("[config_error] {JUDILIBRE_DISABLED_MESSAGE}")
            );
        }
    }
}
