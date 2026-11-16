//! Outil `fr_get_bofip` (05 §5.15) : la lecture d'une publication du BOFiP par son identifiant,
//! avec le retour progressif des lectures longues (résumé, fenêtre, passages).
//!
//! La source n'est pas Légifrance : c'est le port `TaxDoctrine`, servi par `lf-adapter-bofip`
//! (ADR-0028). Motif de `tools/mod.rs` : arguments bruts (`JsonObject`), décodage par `to_call`,
//! appel de la façade avec le jeton d'annulation de la requête, vue de sortie à schéma explicite.

use lf_application::{
    input::GetBofipInput,
    ports::{AuditLog, Clock, LegalCorpus, TaxDoctrine},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_bofip::GetBofipParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_bofip::BofipView,
};

/// Description du descripteur, mot pour mot (05 §5.15, 878 caractères, sans adresse).
pub(crate) const DESCRIPTION: &str = r#"Read one BOFiP publication by its id (BOI-... or ACTU-..., from fr_search_bofip): title, kind, serie, valid_from, citation, source_url and cited_articles (the code articles its text cites, at most 50). BOFiP is the tax administration's doctrine, not the law; only the publication in force today is served, never a past version, and it may predate a change of the law: check cited_articles with fr_verify_citations before relying on it. By default a text of about 1,200 characters or less comes back whole in `text` and a longer one as a `summary` (its beginning). `detail`="full" pages through the text with `offset`/`max_chars` (default 4,000 characters) and `next_offset`; `focus` returns the most relevant passages (default 3) and takes precedence over `detail` and `offset`. A few ids designate two publications: the answer then lists them, and `publication_id` selects one."#;

#[tool_router(router = get_bofip_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine> LegifranceServer<C, A, K, D> {
    #[tool(
        name = "fr_get_bofip",
        title = "Read a BOFiP publication",
        description = DESCRIPTION,
        input_schema = input_schema::<GetBofipParams>(),
        output_schema = output_schema::<BofipView>(),
        annotations(
            title = "Read a BOFiP publication",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_bofip(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<BofipView>, ToolFailure> {
        let call = to_call::<GetBofipParams, GetBofipInput>(args);
        let output = self.service.get_bofip(call, ct.cancelled()).await?;
        Ok(Json(BofipView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 878);
        assert!(DESCRIPTION.starts_with("Read one BOFiP publication by its id"));
        assert!(DESCRIPTION.contains("not the law"));
        assert!(DESCRIPTION.contains("never a past version"));
        // ADR-0032 : la liste des articles cités, et comment la vérifier.
        assert!(
            DESCRIPTION.contains("cited_articles (the code articles its text cites, at most 50)")
        );
        assert!(DESCRIPTION.contains("check cited_articles with fr_verify_citations"));
        assert!(DESCRIPTION.ends_with("and `publication_id` selects one."));
        // Aucune adresse : seuls les liens vers Légifrance sont admis dans un descripteur.
        assert!(!DESCRIPTION.contains("http"));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{
            error::ErrorCode,
            ports::{AuditStatus, DoctrineError},
            testing::DoctrineRequest,
            tools::ToolName,
        };
        use lf_domain::{
            date::CivilDate,
            doctrine::{BoiId, DoctrineDoc, DoctrineKind, Permalink, Serie, clean_text},
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        const ID: &str = "BOI-ENR-DMTG-10-50-20";

        /// Publication `number` de [`ID`], de texte `text`.
        fn doc(number: u32, text: &str) -> DoctrineDoc {
            let raw = format!(
                "https://bofip.impots.gouv.fr/bofip/{number}-PGP.html/identifiant={ID}-20230524"
            );
            let link = Permalink::parse(&raw).unwrap();
            DoctrineDoc {
                id: BoiId::parse(ID).unwrap(),
                publication_id: link.publication_id,
                title: format!("ENR - Mutations à titre gratuit - Donations ({number})"),
                kind: DoctrineKind::Doctrine,
                serie: Some(Serie::parse("ENR").unwrap()),
                division: Some("DMTG".to_owned()),
                valid_from: CivilDate::new(2023, 5, 24),
                source_url: link.url,
                text: clean_text(text).unwrap().0,
            }
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_get_bofip");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_get_bofip").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Read a BOFiP publication"));
            assert_eq!(tool.description.as_deref(), Some(super::super::DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(true));
            let input = Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(input["required"], json!(["id"]));
            assert_eq!(input["additionalProperties"], false);
            assert!(input.get("$schema").is_none());
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert_eq!(output["required"], json!(["id", "attribution", "doctrine_note"]));
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn a_short_publication_comes_back_whole_and_is_audited() {
            let fixture = Fixture::default();
            let text = "Les réductions de droits sont prévues à l'article 790 du CGI.";
            fixture.doctrine.push_get(Ok(vec![doc(3369, text)]));
            let client = fixture.connect().await;
            let result =
                client.call_tool(request(&json!({"id": " boi-enr-dmtg-10-50-20 "}))).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["id"], ID);
            assert_eq!(structured["publication_id"], 3369);
            assert_eq!(structured["kind"], "doctrine");
            assert_eq!(structured["citation"], "BOFiP, BOI-ENR-DMTG-10-50-20, 24 mai 2023");
            assert_eq!(structured["text"], text);
            assert_eq!(structured["text_length"], text.chars().count());
            assert_eq!(
                structured["attribution"],
                "Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0"
            );
            assert!(structured["doctrine_note"].as_str().unwrap().contains("L. 80 A"));
            assert!(structured.get("publications").is_none());
            // Le bloc texte est le même JSON (compatibilité des clients sans structuredContent).
            let block = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&block).unwrap(), structured);

            // Le port reçoit l'identifiant validé, en majuscules.
            assert_eq!(
                fixture.doctrine.seen_requests(),
                [DoctrineRequest::Get(BoiId::parse(ID).unwrap())]
            );
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_GET_BOFIP);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(
                records[0].output_count_or_size,
                u64::try_from(text.chars().count()).unwrap()
            );
            // Le BOFiP ne passe jamais par le corpus Légifrance.
            assert_eq!(fixture.corpus.calls(), 0);
        }

        /// Deux publications sous le même identifiant : sans `publication_id`, leur liste ; avec,
        /// la lecture de celle qui est choisie.
        #[tokio::test]
        async fn two_publications_are_listed_then_one_is_read_by_its_number() {
            let fixture = Fixture::default();
            fixture
                .doctrine
                .push_get(Ok(vec![doc(648, "Premier texte."), doc(11040, "Second texte.")]));
            fixture
                .doctrine
                .push_get(Ok(vec![doc(648, "Premier texte."), doc(11040, "Second texte.")]));
            let client = fixture.connect().await;

            let listed = client.call_tool(request(&json!({"id": ID}))).await.unwrap();
            let structured = listed.structured_content.unwrap();
            let numbers: Vec<_> = structured["publications"]
                .as_array()
                .unwrap()
                .iter()
                .map(|publication| publication["publication_id"].clone())
                .collect();
            assert_eq!(numbers, [json!(648), json!(11040)]);
            assert!(structured.get("text").is_none() && structured.get("text_length").is_none());
            assert!(structured["note"].as_str().unwrap().contains("publication_id"));

            let read = client
                .call_tool(request(&json!({"id": ID, "publication_id": 11040})))
                .await
                .unwrap();
            let structured = read.structured_content.unwrap();
            assert_eq!(structured["publication_id"], 11040);
            assert_eq!(structured["text"], "Second texte.");

            let sizes: Vec<_> =
                fixture.audit.records().iter().map(|record| record.output_count_or_size).collect();
            assert_eq!(sizes, [2, 13]);
        }

        #[tokio::test]
        async fn an_unknown_publication_is_an_in_band_not_found() {
            let fixture = Fixture::default();
            fixture.doctrine.push_get(Err(DoctrineError::NotFound));
            let client = fixture.connect().await;
            let result = client.call_tool(request(&json!({"id": ID}))).await.unwrap();
            assert_eq!(result.is_error, Some(true));
            assert!(result.structured_content.is_none());
            let text = &result.content[0].as_text().unwrap().text;
            assert!(
                text.starts_with(&format!("[not_found] No BOFiP publication found for {ID}.")),
                "{text}"
            );
            let records = fixture.audit.records();
            assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
            assert_eq!(records[0].output_count_or_size, 0);
        }

        #[tokio::test]
        async fn bad_arguments_are_an_audited_invalid_arg_without_a_port_call() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            for (arguments, expected) in [
                (json!({}), "[invalid_arg] id is required."),
                (
                    json!({"id": 5}),
                    "[invalid_arg] id: invalid type: integer `5`, expected a string",
                ),
            ] {
                let result = client.call_tool(request(&arguments)).await.unwrap();
                assert_eq!(result.is_error, Some(true), "{arguments}");
                assert_eq!(result.content[0].as_text().unwrap().text, expected);
            }
            // Un identifiant qui n'en a pas la forme est refusé avant le port.
            let shaped = client.call_tool(request(&json!({"id": "LEGIARTI000006417749!"}))).await;
            assert!(
                shaped.unwrap().content[0].as_text().unwrap().text.starts_with("[invalid_arg] ")
            );
            assert_eq!(fixture.doctrine.calls(), 0);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 3);
            assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        }
    }
}
