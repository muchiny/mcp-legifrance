//! Outil `fr_search_service_public` (05 §5.16) : la recherche dans les fiches pratiques de
//! Service-Public.gouv.fr (DILA).
//!
//! La source n'est pas Légifrance : c'est le port `PublicGuidance`, servi par
//! `lf-adapter-service-public` (ADR-0033). Motif de `tools/mod.rs` : arguments bruts
//! (`JsonObject`), décodage par `to_call`, appel de la façade avec le jeton d'annulation de la
//! requête, vue de sortie à schéma explicite.

use lf_application::{
    input::SearchServicePublicInput,
    ports::{AuditLog, Clock, LegalCorpus, PublicGuidance, TaxDoctrine},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::search_service_public::SearchServicePublicParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::search_service_public::GuidanceSearchView,
};

/// Description du descripteur, mot pour mot (05 §5.16, 663 caractères, 5 phrases, sans adresse ;
/// « for example » plutôt que « e.g. », AM-63). Elle dit ce que lit la recherche (titres et
/// résumés, jamais le texte entier), la règle des mots (préfixes) et le téléchargement du jour.
pub(crate) const DESCRIPTION: &str = "Search the practical sheets of Service-Public.gouv.fr, the French administration's plain-language guide to rights and procedures (DILA): what the rules mean for a given situation, not the law itself. The search runs locally on the titles and summaries of the sheets and folders, never on their full text: use a few plain words, accents and case aside; a word also matches the longer words it starts. `audience` picks particuliers (default), entreprendre (businesses and the self-employed) or all. The first search of the day downloads the day's archive (about 24 MB). Returns ids (F for a sheet, N for a folder) to read with fr_get_service_public, never the text.";

#[tool_router(router = search_service_public_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance>
    LegifranceServer<C, A, K, D, G>
{
    #[tool(
        name = "fr_search_service_public",
        title = "Search Service-Public practical sheets",
        description = DESCRIPTION,
        input_schema = input_schema::<SearchServicePublicParams>(),
        output_schema = output_schema::<GuidanceSearchView>(),
        annotations(
            title = "Search Service-Public practical sheets",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_search_service_public(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<GuidanceSearchView>, ToolFailure> {
        let call = to_call::<SearchServicePublicParams, SearchServicePublicInput>(args);
        let output = self.service.search_service_public(call, ct.cancelled()).await?;
        Ok(Json(GuidanceSearchView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 663);
        assert!(DESCRIPTION.starts_with("Search the practical sheets of Service-Public.gouv.fr"));
        assert!(DESCRIPTION.contains("not the law itself"));
        assert!(DESCRIPTION.contains("never on their full text"));
        assert!(DESCRIPTION.contains("a word also matches the longer words it starts"));
        assert!(DESCRIPTION.contains("to read with fr_get_service_public, never the text."));
        // Aucune adresse : seuls les liens vers Légifrance sont admis dans un descripteur.
        assert!(!DESCRIPTION.contains("http"));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use std::{num::NonZeroU32, time::Duration};

        use lf_application::{
            error::ErrorCode,
            messages::{GUIDANCE_DISABLED_MESSAGE, GUIDANCE_DOWNLOADING_MESSAGE},
            ports::{ArchiveFile, AuditStatus, GuidanceError, GuidanceFound},
            testing::GuidanceRequest,
            tools::ToolName,
        };
        use lf_domain::{
            date::CivilDate,
            guidance::{
                Audience, AudienceFilter, GuidanceHit, GuidanceKind, GuidancePage, GuidanceQuery,
                GuidanceTerms, ServicePublicUrl, SheetId,
            },
            search::PageSize,
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        fn hit(audience: Audience, raw: &str) -> GuidanceHit {
            let id = SheetId::parse(raw).unwrap();
            GuidanceHit {
                source_url: ServicePublicUrl::read(None, audience, &id),
                audience,
                id,
                kind: GuidanceKind::Sheet,
                title: "Solde bancaire insaisissable (SBI)".to_owned(),
                path: vec!["Justice".to_owned(), "Saisies et recouvrements".to_owned()],
                updated: CivilDate::new(2026, 4, 1),
            }
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_search_service_public");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_search_service_public").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Search Service-Public practical sheets"));
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
            assert!(input.get("$schema").is_none());
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert!(output["properties"]["hits"]["items"]["properties"].get("text").is_none());
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn a_search_returns_the_structured_page_and_is_audited() {
            let fixture = Fixture::default();
            let files = Audience::ALL
                .map(|audience| ArchiveFile { audience, last_modified: None })
                .to_vec();
            let page = GuidancePage {
                total: 12,
                hits: vec![
                    hit(Audience::Particuliers, "F1437"),
                    hit(Audience::Entreprendre, "F2408"),
                ],
            };
            fixture.guidance.push_search(Ok(GuidanceFound { page, files }));
            let client = fixture.connect().await;
            let arguments = json!({"query": "solde, bancaire", "audience": "ALL", "page_size": 2});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["total"], 12);
            assert_eq!(structured["has_more"], true);
            assert_eq!(structured["hits"].as_array().unwrap().len(), 2);
            assert_eq!(structured["hits"][1]["audience"], "entreprendre");
            assert_eq!(structured["source_files"].as_array().unwrap().len(), 2);
            assert!(structured["attribution"].as_str().unwrap().contains(" ; "));
            assert!(structured["guidance_note"].as_str().unwrap().contains("not the law"));
            // Le bloc texte est le même JSON (compatibilité des clients sans structuredContent).
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);

            // Le port reçoit une requête typée : mots nettoyés, les deux publics.
            let expected = GuidanceQuery {
                terms: GuidanceTerms::parse("solde bancaire").unwrap(),
                audience: AudienceFilter::All,
                page: NonZeroU32::MIN,
                page_size: PageSize::new(2).unwrap(),
            };
            assert_eq!(fixture.guidance.seen_requests(), [GuidanceRequest::Search(expected)]);

            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_SEARCH_SERVICE_PUBLIC);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(records[0].output_count_or_size, 2);
            // Service-Public ne passe jamais par le corpus Légifrance ni par le BOFiP.
            assert_eq!((fixture.corpus.calls(), fixture.doctrine.calls()), (0, 0));
        }

        #[tokio::test]
        async fn a_source_failure_is_an_in_band_error_that_names_no_word_of_the_query() {
            let fixture = Fixture::default();
            fixture.guidance.push_search(Err(GuidanceError::RateLimited {
                retry_after: Some(Duration::from_secs(900)),
            }));
            fixture.guidance.push_search(Err(GuidanceError::Disabled));
            fixture.guidance.push_search(Err(GuidanceError::Downloading));
            let client = fixture.connect().await;

            let mut texts = Vec::new();
            for _ in 0..3 {
                let result =
                    client.call_tool(request(&json!({"query": "zzsentinellezz"}))).await.unwrap();
                assert_eq!(result.is_error, Some(true));
                assert!(result.structured_content.is_none());
                texts.push(result.content[0].as_text().unwrap().text.clone());
            }
            assert_eq!(
                texts[0],
                "[rate_limited] Service-Public server quota reached; retry in 900 s."
            );
            assert_eq!(texts[1], format!("[config_error] {GUIDANCE_DISABLED_MESSAGE}"));
            assert_eq!(texts[2], format!("[upstream_error] {GUIDANCE_DOWNLOADING_MESSAGE}"));
            assert!(texts.iter().all(|text| !text.contains("zzsentinellezz")));

            let records = fixture.audit.records();
            let codes: Vec<_> = records.iter().map(|record| record.error_code).collect();
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
            for (arguments, expected) in [
                (json!({}), "[invalid_arg] query is required."),
                (
                    json!({"query": 42}),
                    "[invalid_arg] query: invalid type: integer `42`, expected a string",
                ),
            ] {
                let result = client.call_tool(request(&arguments)).await.unwrap();
                assert_eq!(result.is_error, Some(true), "{arguments}");
                assert_eq!(result.content[0].as_text().unwrap().text, expected);
            }
            // Une requête sans aucun mot, ou un public inconnu, est refusé avant le port.
            for arguments in
                [json!({"query": "?!, --"}), json!({"query": "loyer", "audience": "x"})]
            {
                let result = client.call_tool(request(&arguments)).await.unwrap();
                let text = &result.content[0].as_text().unwrap().text;
                assert!(text.starts_with("[invalid_arg] "), "{text}");
            }
            assert_eq!(fixture.guidance.calls(), 0);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 4);
            assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        }
    }
}
