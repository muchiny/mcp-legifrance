//! Outil `fr_search_bofip` (05 §5.14) : la recherche dans le BOFiP, la doctrine publiée de
//! l'administration fiscale.
//!
//! La source n'est pas Légifrance : c'est le port `TaxDoctrine`, servi par `lf-adapter-bofip`
//! (ADR-0028). Motif de `tools/mod.rs` : arguments bruts (`JsonObject`), décodage par `to_call`,
//! appel de la façade avec le jeton d'annulation de la requête, vue de sortie à schéma explicite.

use lf_application::{
    input::SearchBofipInput,
    ports::{AuditLog, Clock, LegalCorpus, TaxDoctrine},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::search_bofip::SearchBofipParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::search_bofip::BofipSearchView,
};

/// Description du descripteur, mot pour mot (05 §5.14, 848 caractères, 6 phrases, sans adresse ;
/// « for example » et non « e.g. », que le contrôle du nombre de phrases compte comme une fin de
/// phrase, AM-63). Elle dit la règle d'ADR-0031 : les mots tels qu'ils sont écrits, puis la
/// recherche approchée en secours, signalée dans `note`.
pub(crate) const DESCRIPTION: &str = "Search BOFiP, the published doctrine of the French tax administration (DGFiP): how the administration reads tax law, not the law itself. Every word of `query` must appear in a publication exactly as written, accents and case aside: no prefixes, no operators, no quoted phrases, so use a few plain words and try both singular and plural forms. If no publication holds them all, the search falls back to approximate spelling and says so in `note`. Narrow with `serie` (for example ENR for registration and inheritance duties, IR for income tax, RFPI for property income and capital gains); `kind` selects doctrine pages (default), news items, or all. Returns id, publication_id, title, kind, serie, valid_from and source_url, paged with `page` and `has_more`; never the text: read it with fr_get_bofip. Only publications in force today are available.";

#[tool_router(router = search_bofip_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine> LegifranceServer<C, A, K, D> {
    #[tool(
        name = "fr_search_bofip",
        title = "Search the tax doctrine (BOFiP)",
        description = DESCRIPTION,
        input_schema = input_schema::<SearchBofipParams>(),
        output_schema = output_schema::<BofipSearchView>(),
        annotations(
            title = "Search the tax doctrine (BOFiP)",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_search_bofip(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<BofipSearchView>, ToolFailure> {
        let call = to_call::<SearchBofipParams, SearchBofipInput>(args);
        let output = self.service.search_bofip(call, ct.cancelled()).await?;
        Ok(Json(BofipSearchView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 848);
        assert!(DESCRIPTION.starts_with("Search BOFiP, the published doctrine"));
        assert!(DESCRIPTION.contains("not the law itself"));
        // La règle d'ADR-0031 : mots exacts, plus de préfixe, secours dit dans `note`.
        assert!(DESCRIPTION.contains("exactly as written, accents and case aside: no prefixes"));
        assert!(DESCRIPTION.contains("falls back to approximate spelling and says so in `note`"));
        assert!(!DESCRIPTION.contains("matches as a prefix"));
        assert!(DESCRIPTION.contains("read it with fr_get_bofip"));
        assert!(DESCRIPTION.ends_with("Only publications in force today are available."));
        // Aucune adresse : seuls les liens vers Légifrance sont admis dans un descripteur.
        assert!(!DESCRIPTION.contains("http"));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use std::{num::NonZeroU32, time::Duration};

        use lf_application::{
            error::ErrorCode,
            messages::DOCTRINE_DISABLED_MESSAGE,
            ports::{AuditStatus, DoctrineError},
            testing::DoctrineRequest,
            tools::ToolName,
        };
        use lf_domain::{
            date::CivilDate,
            doctrine::{
                BoiId, DoctrineHit, DoctrineKind, DoctrinePage, DoctrineQuery, DoctrineTerms,
                KindFilter, Permalink, Serie,
            },
            search::PageSize,
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        const ID: &str = "BOI-ENR-DMTG-10-50-20";

        fn hit(number: u32) -> DoctrineHit {
            let raw = format!(
                "https://bofip.impots.gouv.fr/bofip/{number}-PGP.html/identifiant={ID}-20230524"
            );
            let link = Permalink::parse(&raw).unwrap();
            DoctrineHit {
                id: BoiId::parse(ID).unwrap(),
                publication_id: link.publication_id,
                title: "ENR - Mutations à titre gratuit - Donations".to_owned(),
                kind: DoctrineKind::Doctrine,
                serie: Some(Serie::parse("ENR").unwrap()),
                valid_from: CivilDate::new(2023, 5, 24),
                source_url: link.url,
            }
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_search_bofip");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_search_bofip").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Search the tax doctrine (BOFiP)"));
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
            fixture
                .doctrine
                .push_search(Ok(DoctrinePage { total: 433, hits: vec![hit(2699), hit(3369)] }));
            let client = fixture.connect().await;
            let arguments =
                json!({"query": "donation, abattement", "serie": "enr", "page_size": 2});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["total"], 433);
            assert_eq!(structured["page"], 1);
            assert_eq!(structured["has_more"], true);
            assert_eq!(structured["hits"].as_array().unwrap().len(), 2);
            assert_eq!(structured["hits"][0]["id"], ID);
            assert_eq!(structured["hits"][0]["publication_id"], 2699);
            assert_eq!(structured["hits"][0]["kind"], "doctrine");
            assert_eq!(
                structured["attribution"],
                "Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0"
            );
            assert!(structured["doctrine_note"].as_str().unwrap().contains("not the law itself"));
            // Le bloc texte est le même JSON (compatibilité des clients sans structuredContent).
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);

            // Le port reçoit une requête typée : mots nettoyés, série en majuscules, doctrine seule.
            let expected = DoctrineQuery::new(
                DoctrineTerms::parse("donation abattement").unwrap(),
                Some(Serie::parse("ENR").unwrap()),
                KindFilter::Doctrine,
                NonZeroU32::MIN,
                PageSize::new(2).unwrap(),
            )
            .unwrap();
            assert_eq!(fixture.doctrine.seen_requests(), [DoctrineRequest::Search(expected)]);

            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_SEARCH_BOFIP);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(records[0].output_count_or_size, 2);
            // Le BOFiP ne passe jamais par le corpus Légifrance.
            assert_eq!(fixture.corpus.calls(), 0);
        }

        #[tokio::test]
        async fn a_portal_refusal_is_an_in_band_error_that_names_no_word_of_the_query() {
            let fixture = Fixture::default();
            fixture.doctrine.push_search(Err(DoctrineError::RateLimited {
                retry_after: Some(Duration::from_secs(120)),
            }));
            fixture.doctrine.push_search(Err(DoctrineError::Disabled));
            let client = fixture.connect().await;

            let limited =
                client.call_tool(request(&json!({"query": "zzsentinellezz"}))).await.unwrap();
            assert_eq!(limited.is_error, Some(true));
            assert!(limited.structured_content.is_none());
            assert_eq!(
                limited.content[0].as_text().unwrap().text,
                "[rate_limited] BOFiP portal quota reached; retry in 120 s."
            );

            let off = client.call_tool(request(&json!({"query": "zzsentinellezz"}))).await.unwrap();
            assert_eq!(off.is_error, Some(true));
            assert_eq!(
                off.content[0].as_text().unwrap().text,
                format!("[config_error] {DOCTRINE_DISABLED_MESSAGE}")
            );

            let records = fixture.audit.records();
            let codes: Vec<_> = records.iter().map(|record| record.error_code).collect();
            assert_eq!(codes, [Some(ErrorCode::RateLimited), Some(ErrorCode::ConfigError)]);
            assert!(records.iter().all(|record| record.output_count_or_size == 0));
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
            // Une requête sans aucun mot est refusée avant le port.
            let empty = client.call_tool(request(&json!({"query": "?!, --"}))).await.unwrap();
            assert!(empty.content[0].as_text().unwrap().text.starts_with("[invalid_arg] "));
            assert_eq!(fixture.doctrine.calls(), 0);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 3);
            assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        }
    }
}
