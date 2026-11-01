//! Outil `fr_get_deliberation` (05 §5.5) : la lecture d'une délibération ou d'une décision de la
//! CNIL, en résumé (son début), en fenêtre de texte ou en passages.
//!
//! Motif de `tools/mod.rs` : arguments bruts (`JsonObject`), décodage par `to_call`, appel de la
//! façade avec le jeton d'annulation de la requête, vue de sortie à schéma explicite. La vue
//! ramène elle-même la réponse sous le plafond dur de 45 000 caractères (05 §6.4).

use lf_application::{
    input::GetDeliberationInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_deliberation::GetDeliberationParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_deliberation::DeliberationView,
};

/// Description de l'outil (485 caractères, 3 phrases ; 05 §5.5).
pub(crate) const DESCRIPTION: &str = "Read a CNIL deliberation or decision by its CNILTEXT id (from fr_search on fond CNIL): number, nature (e.g. Sanction), status, dates, citation and source_url. By default a short text comes back whole in `text` and a long one as a `summary` (its beginning); `detail`=\"full\" pages through the text with `offset`/`max_chars` (default 4,000 characters) and `next_offset`, and `focus` returns the most relevant passages (default 3). Personal data is redacted by the CNIL in published texts.";

#[tool_router(router = get_deliberation_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_get_deliberation",
        title = "Read a CNIL deliberation",
        description = DESCRIPTION,
        input_schema = input_schema::<GetDeliberationParams>(),
        output_schema = output_schema::<DeliberationView>(),
        annotations(
            title = "Read a CNIL deliberation",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_deliberation(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<DeliberationView>, ToolFailure> {
        let call = to_call::<GetDeliberationParams, GetDeliberationInput>(args);
        let output = self.service.get_deliberation(call, ct.cancelled()).await?;
        Ok(Json(DeliberationView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 485);
        assert!(DESCRIPTION.starts_with("Read a CNIL deliberation or decision by its CNILTEXT id"));
        assert!(DESCRIPTION.contains("`detail`=\"full\""));
        assert!(DESCRIPTION.ends_with("Personal data is redacted by the CNIL in published texts."));
        // L'outil cité l'est par son nom d'enregistrement (05 §4.3).
        assert!(DESCRIPTION.contains("fr_search"));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{
            error::ErrorCode,
            ports::{AuditStatus, CorpusError},
            tools::ToolName,
        };
        use lf_domain::{
            date::CivilDate, entity::Deliberation, id::CnilTextId, status::LegalStatus,
            text::LongText,
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        const ID: &str = "CNILTEXT000053352643";

        /// Les trois paragraphes (593 caractères) de la fixture `consult_cnil.json`, texte tronqué.
        const PARAGRAPHS: [&str; 3] = [
            "- Les développements de la délibération comportant des données à caractère personnel ou des secrets protégés par la loi sont remplacés par le signe […] -",
            "La Commission nationale de l’informatique et des libertés, réunie en sa formation restreinte composée de M. Philippe-Pierre CABOURDIN, président, M. Vincent LESCLOUS, vice-président, Mmes Laurence FRANCESCHINI et Isabelle LATOURNARIE-WILLEMS, MM. KLING, membres,",
            "Vu le règlement (UE) 2016/679 du Parlement européen et du Conseil du 27 avril 2016 relatif à la protection des personnes physiques à l’égard du traitement des données à [...]",
        ];

        fn deliberation(text: &str) -> Deliberation {
            Deliberation {
                deliberation_id: CnilTextId::parse(ID).unwrap(),
                num: Some("SAN-2026-002".to_owned()),
                nature: Some("Délibération".to_owned()),
                nature_delib: Some("Sanction".to_owned()),
                etat: LegalStatus::parse("VIGUEUR"),
                date_texte: CivilDate::new(2026, 1, 8),
                date_publication: CivilDate::new(2026, 1, 14),
                title: Some("Délibération SAN-2026-002 du 8 janvier 2026".to_owned()),
                title_long: Some(
                    "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 prononçant une sanction pécuniaire à l'encontre de la société FREE"
                        .to_owned(),
                ),
                text: LongText::from_plain(text).unwrap(),
            }
        }

        fn fixture_text() -> String {
            PARAGRAPHS.join("\n")
        }

        /// Une phrase de 70 caractères environ.
        fn sentence(n: usize) -> String {
            format!("Considérant que le manquement n° {n} à l’article 32 est caractérisé. ")
        }

        /// Une longue délibération : 30 paragraphes de quatre phrases.
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "tests : valeurs bornées par les données du test"
        )]
        fn long_text() -> String {
            let paragraph = |p: usize| (0..4).map(|s| sentence(p * 10 + s)).collect::<String>();
            (0..30).map(|p| paragraph(p).trim_end().to_owned()).collect::<Vec<_>>().join("\n")
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_get_deliberation");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_get_deliberation").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Read a CNIL deliberation"));
            assert_eq!(tool.description.as_deref(), Some(super::super::DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(true));
            let input = Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(input["required"], json!(["deliberation_id"]));
            assert_eq!(input["additionalProperties"], false);
            assert!(input.get("$schema").is_none());
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert!(output["properties"].get("summary_source").is_some());
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn a_short_deliberation_comes_back_whole_and_is_audited() {
            let fixture = Fixture::default();
            fixture.corpus.push_get_deliberation(Ok(deliberation(&fixture_text())));
            let client = fixture.connect().await;
            let result = client
                .call_tool(request(&json!({"deliberation_id": " cniltext000053352643 "})))
                .await;
            let result = result.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["deliberation_id"], ID);
            assert_eq!(structured["num"], "SAN-2026-002");
            assert_eq!(structured["nature_delib"], "Sanction");
            assert_eq!(structured["etat"], "VIGUEUR");
            assert_eq!(structured["date"], "2026-01-08");
            assert_eq!(structured["date_publication"], "2026-01-14");
            assert_eq!(
                structured["citation"],
                "CNIL, délibération n° SAN-2026-002 du 8 janvier 2026"
            );
            assert_eq!(
                structured["source_url"],
                "https://www.legifrance.gouv.fr/cnil/id/CNILTEXT000053352643"
            );
            assert_eq!(structured["text"], PARAGRAPHS.join("\n\n"));
            assert_eq!(structured["text_length"], 593);
            assert!(structured.get("summary").is_none());
            assert!(structured.get("next_offset").is_none());
            // 1 099 caractères de JSON compact (05 §6.5).
            assert_eq!(serde_json::to_string(&structured).unwrap().chars().count(), 1_099);
            // Le bloc texte est le même JSON (compatibilité des clients sans structuredContent).
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_GET_DELIBERATION);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(records[0].output_count_or_size, 593);
            assert_eq!(fixture.corpus.calls(), 1);
        }

        #[tokio::test]
        async fn a_long_deliberation_is_summarised_by_its_lead_over_the_wire() {
            let fixture = Fixture::default();
            let text = long_text();
            fixture.corpus.push_get_deliberation(Ok(deliberation(&text)));
            let client = fixture.connect().await;
            let result = client.call_tool(request(&json!({"deliberation_id": ID}))).await.unwrap();
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["summary_source"], "lead");
            let summary = structured["summary"].as_str().unwrap();
            assert!((600..=1_200).contains(&summary.chars().count()));
            assert!(text.replace('\n', "\n\n").starts_with(summary));
            assert!(structured.get("text").is_none());
            assert_eq!(structured["text_length"], LongText::from_plain(&text).unwrap().char_len());
            let records = fixture.audit.records();
            assert_eq!(
                records[0].output_count_or_size,
                u64::try_from(summary.chars().count()).unwrap()
            );
        }

        #[tokio::test]
        async fn pages_follow_next_offset_and_rebuild_the_text() {
            let fixture = Fixture::default();
            let text = long_text();
            let canonical = text.replace('\n', "\n\n");
            let client = fixture.connect().await;
            let mut rebuilt = String::new();
            let mut offset = Value::from(0);
            let mut pages = 0;
            loop {
                fixture.corpus.push_get_deliberation(Ok(deliberation(&text)));
                // Entiers tolérants : `max_chars` en chaîne décimale.
                let arguments = json!({
                    "deliberation_id": ID, "detail": "FULL", "offset": offset, "max_chars": "1500"
                });
                let result = client.call_tool(request(&arguments)).await.unwrap();
                assert_eq!(result.is_error, Some(false));
                let page = result.structured_content.unwrap();
                rebuilt.push_str(page["text"].as_str().unwrap());
                pages += 1;
                assert!(pages < 50, "pas de progression");
                match page.get("next_offset") {
                    Some(next) => offset = next.clone(),
                    None => break,
                }
            }
            assert!(pages > 2, "{pages} pages");
            assert_eq!(rebuilt, canonical);
            assert_eq!(fixture.audit.records().len(), pages);
        }

        #[tokio::test]
        async fn focus_returns_the_best_passages_with_offsets() {
            let fixture = Fixture::default();
            let text = "Premier paragraphe sur la procédure.\nLe responsable de traitement n’a pas \
                        notifié la violation de données.\nTroisième paragraphe sur les mesures.";
            fixture.corpus.push_get_deliberation(Ok(deliberation(text)));
            let client = fixture.connect().await;
            let arguments = json!({"deliberation_id": ID, "focus": "notifié violation"});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            let structured = result.structured_content.unwrap();
            let passages = structured["passages"].as_array().unwrap();
            assert!(passages[0]["text"].as_str().unwrap().starts_with("Le responsable"));
            assert_eq!(passages[0]["offset"], 38);
            assert!(structured.get("summary").is_none() && structured.get("text").is_none());
            assert!(passages.len() <= 3);
        }

        #[tokio::test]
        async fn an_escape_rich_window_stays_under_the_hard_cap() {
            let fixture = Fixture::default();
            // Guillemets, antislashs, emoji hors BMP et sauts de ligne : 6 caractères de JSON pour 4.
            let nasty = "\"\\😀 ".repeat(10_000);
            fixture.corpus.push_get_deliberation(Ok(deliberation(&nasty)));
            let client = fixture.connect().await;
            let arguments = json!({"deliberation_id": ID, "detail": "full", "max_chars": 40_000});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            let structured = result.structured_content.unwrap();
            let size = serde_json::to_string(&structured).unwrap().chars().count();
            assert!(size <= 45_000, "{size} caractères");
            let kept = structured["text"].as_str().unwrap().chars().count();
            assert!(kept < 40_000);
            assert_eq!(structured["next_offset"], kept);
        }

        #[tokio::test]
        async fn an_unknown_deliberation_is_an_in_band_not_found() {
            let fixture = Fixture::default();
            fixture.corpus.push_get_deliberation(Err(CorpusError::NotFound {
                what: "/consult/cnil".to_owned(),
            }));
            let client = fixture.connect().await;
            let result = client.call_tool(request(&json!({"deliberation_id": ID}))).await.unwrap();
            assert_eq!(result.is_error, Some(true));
            assert!(result.structured_content.is_none());
            let text = &result.content[0].as_text().unwrap().text;
            assert_eq!(
                text,
                &format!(
                    "[not_found] No CNIL deliberation found for {ID}. Check the id, or search with fr_search."
                )
            );
            let records = fixture.audit.records();
            assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
            assert_eq!(records[0].output_count_or_size, 0);
        }

        #[tokio::test]
        async fn bad_arguments_are_an_audited_invalid_arg_without_a_port_call() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            let cases = [
                (json!({}), "[invalid_arg] deliberation_id is required."),
                (
                    json!({"deliberation_id": 5}),
                    "[invalid_arg] deliberation_id: invalid type: integer `5`, expected a string",
                ),
                (
                    json!({"deliberation_id": ID, "foo": 1}),
                    "[invalid_arg] foo: unknown field `foo`, expected one of `deliberation_id`, \
                     `detail`, `offset`, `max_chars`, `focus`, `passages`",
                ),
                (
                    json!({"deliberation_id": ID, "offset": [1]}),
                    "[invalid_arg] offset: invalid type: sequence, expected an integer",
                ),
                (
                    json!({"deliberation_id": ID, "detail": "long"}),
                    "[invalid_arg] detail='long' must be one of ['full', 'summary'].",
                ),
                (
                    json!({"deliberation_id": ID, "max_chars": 100}),
                    "[invalid_arg] max_chars=100 out of range (500..40000).",
                ),
                (
                    json!({"deliberation_id": ID, "detail": "summary", "offset": 5}),
                    "[invalid_arg] offset requires detail=\"full\" (or omit detail).",
                ),
                (
                    json!({"deliberation_id": "JURITEXT000037999394"}),
                    "[invalid_arg] deliberation_id='JURITEXT000037999394' must be a Legifrance \
                     CNILTEXT... identifier. JURITEXT... identifies a court decision: use \
                     fr_get_decision.",
                ),
            ];
            for (arguments, expected) in &cases {
                let result = client.call_tool(request(arguments)).await.unwrap();
                assert_eq!(result.is_error, Some(true), "{arguments}");
                let text = &result.content[0].as_text().unwrap().text;
                assert!(text.starts_with(expected), "{arguments}: {text}");
            }
            assert_eq!(fixture.corpus.calls(), 0);
            let records = fixture.audit.records();
            assert_eq!(records.len(), cases.len());
            assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        }

        #[tokio::test]
        async fn an_offset_past_the_end_is_an_audited_invalid_arg() {
            let fixture = Fixture::default();
            fixture.corpus.push_get_deliberation(Ok(deliberation(&fixture_text())));
            let client = fixture.connect().await;
            let arguments = json!({"deliberation_id": ID, "offset": 20_000});
            let result = client.call_tool(request(&arguments)).await.unwrap();
            assert_eq!(result.is_error, Some(true));
            assert_eq!(
                result.content[0].as_text().unwrap().text,
                "[invalid_arg] offset=20000 is at or past the end of the text (text_length=593); \
                 restart from offset=0 or omit offset."
            );
            assert_eq!(fixture.audit.records()[0].error_code, Some(ErrorCode::InvalidArg));
        }
    }
}
