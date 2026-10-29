//! Outil `fr_get_decision` (05 §5.4) : une décision de justice (JURI, CONSTIT ou CETAT), son ECLI
//! natif, son résumé déterministe, une fenêtre de son texte ou ses passages.
//!
//! Motif de `tools/mod.rs` : arguments bruts (`JsonObject`), décodage par `to_call`, appel de la
//! façade avec le jeton d'annulation de la requête, vue de sortie à schéma explicite. La vue tient
//! elle-même le plafond dur de 45 000 caractères (`views::get_decision`).

use lf_application::{
    input::GetDecisionInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject, tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_decision::GetDecisionParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_decision::DecisionView,
};

/// Description du descripteur, mot pour mot (05 §5.4, 687 caractères, 4 phrases).
const DESCRIPTION: &str = r#"Read a court decision by id: JURITEXT (Cour de cassation, courts of appeal), CONSTEXT (Conseil constitutionnel) or CETATEXT (Conseil d'État, administrative courts), as found by fr_search. Returns court, formation, solution, case numbers, date, the native `ecli` when Légifrance has one (never invented), citation and source_url. By default it adds a short `summary`: the official summary when published, else the operative part or the beginning of the text; a short decision comes back whole in `text`. Use `detail`="full" with `offset`/`max_chars` (default 4,000 characters) to read the text page by page (follow `next_offset`), or `focus` to get the best matching passages (default 3)."#;

#[tool_router(router = get_decision_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_get_decision",
        title = "Read a court decision",
        description = DESCRIPTION,
        input_schema = input_schema::<GetDecisionParams>(),
        output_schema = output_schema::<DecisionView>(),
        annotations(
            title = "Read a court decision",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_decision(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<DecisionView>, ToolFailure> {
        let call = to_call::<GetDecisionParams, GetDecisionInput>(args);
        let output = self.service.get_decision(call, ct.cancelled()).await.map_err(ToolFailure)?;
        Ok(Json(DecisionView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    /// Description du descripteur, mot pour mot (05 §5.4, 687 caractères, 4 phrases).
    const DESCRIPTION: &str = r#"Read a court decision by id: JURITEXT (Cour de cassation, courts of appeal), CONSTEXT (Conseil constitutionnel) or CETATEXT (Conseil d'État, administrative courts), as found by fr_search. Returns court, formation, solution, case numbers, date, the native `ecli` when Légifrance has one (never invented), citation and source_url. By default it adds a short `summary`: the official summary when published, else the operative part or the beginning of the text; a short decision comes back whole in `text`. Use `detail`="full" with `offset`/`max_chars` (default 4,000 characters) to read the text page by page (follow `next_offset`), or `focus` to get the best matching passages (default 3)."#;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 687);
        assert!(DESCRIPTION.starts_with("Read a court decision by id: JURITEXT"));
        assert!(DESCRIPTION.ends_with("the best matching passages (default 3)."));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{
            error::ErrorCode,
            ports::{AuditStatus, CorpusError},
            testing::PortRequest,
            tools::ToolName,
        };
        use lf_domain::{
            date::CivilDate,
            ecli::Ecli,
            entity::{Decision, OfficialSummary},
            id::DecisionId,
            text::LongText,
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::{CallToolRequestParams, CallToolResult};
        use serde_json::{Value, json};

        use super::DESCRIPTION;
        use crate::testing::Fixture;

        const JURI: &str = "JURITEXT000048465611";
        const CONSTIT: &str = "CONSTEXT000052555275";
        const CETAT: &str = "CETATEXT000051655317";

        const CONSTIT_TITLE: &str = "Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de publicité des débats devant le juge des libertés et de la détention statuant sur la détention provisoire du prévenu] - Conformité";

        /// Un paragraphe de remplissage d'environ 170 caractères.
        fn filler(n: usize) -> String {
            format!(
                "{n}. Considérant que la règle posée par le législateur s'impose à tous les \
                 justiciables, sans que puisse y faire obstacle aucune disposition contraire."
            )
        }

        fn fillers(count: usize) -> String {
            (1..=count).map(filler).collect::<Vec<_>>().join("\n\n")
        }

        /// Texte d'une décision du Conseil constitutionnel d'environ 14 300 caractères, dispositif
        /// compris (la fixture de l'exemple 1 de 05 §5.4 en compte 14 332).
        fn constit_text() -> String {
            format!(
                "{}\n\nLE CONSEIL CONSTITUTIONNEL DÉCIDE :\n\nArticle 1er. - Le renvoi opéré par la \
                 première phrase du troisième alinéa de l’article 397-1-1 du code de procédure \
                 pénale est conforme à la Constitution.\n\nArticle 2. - Cette décision sera \
                 publiée au Journal officiel de la République française.\n\nJugé par le Conseil \
                 constitutionnel dans sa séance du 6 novembre 2025.",
                fillers(80)
            )
        }

        fn decision(id: &str, text: &str) -> Decision {
            Decision {
                decision_id: DecisionId::parse(id).unwrap(),
                ecli: None,
                num: None,
                juridiction: None,
                formation: None,
                solution: None,
                numero_affaire: Vec::new(),
                date_texte: None,
                nature: None,
                title: None,
                text: LongText::from_plain(text).unwrap(),
                official_summary: OfficialSummary::default(),
                cited_case_law: None,
                applied_texts: vec![],
            }
        }

        fn constit() -> Decision {
            Decision {
                ecli: Ecli::parse("ECLI:FR:CC:2025:2025.1173.QPC").ok(),
                num: Some("2025-1173".to_owned()),
                juridiction: Some("Conseil constitutionnel".to_owned()),
                solution: Some("Conformité".to_owned()),
                date_texte: CivilDate::new(2025, 11, 7),
                nature: Some("QPC".to_owned()),
                title: Some(CONSTIT_TITLE.to_owned()),
                ..decision(CONSTIT, &constit_text())
            }
        }

        /// Une décision du Conseil d'État dont le sommaire officiel est démesuré.
        fn cetat() -> Decision {
            let analysis = "Projet de plan local d’urbanisme dont la mise en œuvre est susceptible \
                            d’avoir des effets notables sur l’environnement d’un autre Etat membre. "
                .repeat(40);
            Decision {
                ecli: Ecli::parse("ECLI:FR:CECHR:2026:506507.20260529").ok(),
                juridiction: Some("Conseil d'Etat".to_owned()),
                formation: Some("2ème - 7ème chambres réunies".to_owned()),
                solution: Some("Annulation".to_owned()),
                numero_affaire: vec!["506507".to_owned()],
                date_texte: CivilDate::new(2026, 5, 29),
                nature: Some("Texte".to_owned()),
                title: Some(
                    "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507".to_owned(),
                ),
                official_summary: OfficialSummary::new([analysis.as_str()], [""; 0]),
                ..decision(CETAT, &fillers(90))
            }
        }

        async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
            let client = fixture.connect().await;
            let mut params = CallToolRequestParams::new("fr_get_decision");
            params.arguments = arguments.as_object().cloned();
            client.call_tool(params).await.unwrap()
        }

        fn text_of(result: &CallToolResult) -> &str {
            &result.content[0].as_text().unwrap().text
        }

        fn compact_chars(value: &Value) -> usize {
            serde_json::to_string(value).unwrap().chars().count()
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_get_decision").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Read a court decision"));
            assert_eq!(tool.description.as_deref(), Some(DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
            assert_eq!(
                (
                    annotations.read_only_hint,
                    annotations.destructive_hint,
                    annotations.idempotent_hint,
                    annotations.open_world_hint
                ),
                (Some(true), Some(false), Some(true), Some(true))
            );
            let input = Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(input["required"], json!(["decision_id"]));
            assert_eq!(input["additionalProperties"], false);
            assert_eq!(input["properties"].as_object().unwrap().len(), 6);
            assert!(input.get("$schema").is_none());
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert_eq!(output["type"], "object");
            assert!(output["properties"].get("ecli").is_some());
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn default_decision_response_under_budget() {
            // 05 §6.5 : l'appel par défaut d'une décision de 14 000 caractères pèse 1 147
            // caractères (CONSTIT, dispositif) ou 1 171 (CETAT, sommaire officiel) ; T-M5-02 fixe
            // le plafond de ce test à 2 000 caractères de JSON compact.
            for (given, source) in [(constit(), "dispositif"), (cetat(), "official")] {
                let id = given.decision_id.to_string();
                let text_length = given.text.char_len();
                assert!(text_length > 10_000, "{id}: {text_length}");
                let fixture = Fixture::default();
                fixture.corpus.push_get_decision(Ok(given));

                let result = call(&fixture, &json!({"decision_id": id})).await;

                assert_eq!(result.is_error, Some(false), "{id}");
                let view = result.structured_content.clone().unwrap();
                let size = compact_chars(&view);
                assert!(size <= 2_000, "{id} : {size} caractères");
                assert_eq!(view["summary_source"], source);
                assert_eq!(view["text_length"], text_length);
                assert!(view.get("text").is_none() && view.get("next_offset").is_none());
                // Le bloc texte est le même JSON (clients sans `structuredContent`).
                assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), view);
                assert_eq!(text_of(&result).chars().count(), size);
            }
        }

        #[tokio::test]
        async fn a_default_call_returns_the_view_and_writes_one_audit_line() {
            let fixture = Fixture::default();
            fixture.corpus.push_get_decision(Ok(constit()));

            let result = call(&fixture, &json!({"decision_id": " constext000052555275 "})).await;

            let view = result.structured_content.clone().unwrap();
            assert_eq!(view["decision_id"], CONSTIT);
            assert_eq!(view["ecli"], "ECLI:FR:CC:2025:2025.1173.QPC");
            assert_eq!(
                view["citation"],
                "Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025"
            );
            assert_eq!(view["title"], CONSTIT_TITLE);
            assert_eq!(
                view["source_url"],
                "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000052555275"
            );
            assert!(view["summary"].as_str().unwrap().starts_with("Article 1er. - Le renvoi"));
            assert_eq!(
                fixture.corpus.seen_requests(),
                [PortRequest::GetDecision(DecisionId::parse(CONSTIT).unwrap())]
            );
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_GET_DECISION);
            assert_eq!(records[0].status, AuditStatus::Ok);
            // Métrique : les caractères du résumé rendu (05 §5.0.1).
            let rendered = view["summary"].as_str().unwrap().chars().count();
            assert_eq!(records[0].output_count_or_size, u64::try_from(rendered).unwrap());
        }

        #[tokio::test]
        async fn the_full_mode_follows_next_offset_to_the_end() {
            let text = fillers(40);
            let total = LongText::from_plain(&text).unwrap().char_len();
            let fixture = Fixture::default();
            let mut offset = 0_u32;
            let mut pages = 0;
            loop {
                fixture.corpus.push_get_decision(Ok(decision(JURI, &text)));
                let arguments = json!({
                    "decision_id": JURI, "detail": "full", "offset": offset, "max_chars": 1_000
                });
                let view = call(&fixture, &arguments).await.structured_content.unwrap();
                // Chaque page est exactement la tranche du texte canonique qui commence à `offset`.
                let page = view["text"].as_str().unwrap();
                let expected: String =
                    text.chars().skip(offset as usize).take(page.chars().count()).collect();
                assert_eq!(page, expected);
                assert_eq!(view["text_length"], total);
                pages += 1;
                assert!(pages < 20, "la lecture ne finit pas");
                match view.get("next_offset") {
                    Some(next) => offset = u32::try_from(next.as_u64().unwrap()).unwrap(),
                    None => break,
                }
            }
            assert!(pages > 3, "{pages} pages");
        }

        #[tokio::test]
        async fn focus_returns_the_best_passages() {
            let fixture = Fixture::default();
            let text = format!(
                "{}\n\nLe principe de publicité des débats du procès pénal est méconnu.\n\n{}",
                fillers(8),
                fillers(8)
            );
            fixture.corpus.push_get_decision(Ok(decision(CONSTIT, &text)));

            let result =
                call(&fixture, &json!({"decision_id": CONSTIT, "focus": "publicité des débats"}))
                    .await;

            let view = result.structured_content.unwrap();
            let passages = view["passages"].as_array().unwrap();
            assert_eq!(
                passages[0]["text"],
                "Le principe de publicité des débats du procès pénal est méconnu."
            );
            assert!(view.get("summary").is_none() && view.get("text").is_none());
        }

        #[tokio::test]
        async fn an_unknown_decision_is_an_in_band_not_found_naming_the_fond_of_the_prefix() {
            for (id, fond) in [(JURI, "JURI"), (CONSTIT, "CONSTIT"), (CETAT, "CETAT")] {
                let fixture = Fixture::default();
                fixture.corpus.push_get_decision(Err(CorpusError::NotFound {
                    what: "/consult/juri".to_owned(),
                }));

                let result = call(&fixture, &json!({"decision_id": id})).await;

                assert_eq!(result.is_error, Some(true));
                assert!(result.structured_content.is_none());
                assert_eq!(
                    text_of(&result),
                    format!(
                        "[not_found] No {fond} decision found for {id}. Check the id, or search \
                         with fr_search."
                    )
                );
                let records = fixture.audit.records();
                assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
                assert_eq!(records[0].output_count_or_size, 0);
            }
        }

        #[tokio::test]
        async fn bad_arguments_are_audited_invalid_args_without_a_port_call() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            let cases = [
                (json!({}), "[invalid_arg] decision_id is required."),
                (json!({"decision_id": JURI, "foo": 1}), "[invalid_arg] unknown field `foo`"),
                (
                    json!({"decision_id": 5}),
                    "[invalid_arg] decision_id: invalid type: integer `5`, expected a string",
                ),
                (
                    json!({"decision_id": JURI, "detail": "summary", "offset": 10}),
                    "[invalid_arg] offset requires detail=\"full\" (or omit detail).",
                ),
                (
                    json!({"decision_id": JURI, "max_chars": 100}),
                    "[invalid_arg] max_chars=100 out of range (500..40000).",
                ),
            ];
            for (arguments, expected) in &cases {
                let mut params = CallToolRequestParams::new("fr_get_decision");
                params.arguments = arguments.as_object().cloned();
                let result = client.call_tool(params).await.unwrap();
                assert_eq!(result.is_error, Some(true), "{arguments}");
                assert_eq!(text_of(&result), *expected, "{arguments}");
            }
            // Un identifiant de la CNIL renvoie à l'outil de ses délibérations.
            let mut params = CallToolRequestParams::new("fr_get_decision");
            params.arguments = json!({"decision_id": "CNILTEXT000053352643"}).as_object().cloned();
            let result = client.call_tool(params).await.unwrap();
            let message = text_of(&result);
            assert!(message.contains("fr_get_deliberation"), "{message}");
            assert_eq!(fixture.corpus.calls(), 0);
            let records = fixture.audit.records();
            assert_eq!(records.len(), cases.len() + 1);
            assert!(records.iter().all(|record| record.status == AuditStatus::Error));
            assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        }

        #[tokio::test]
        async fn an_escape_rich_window_stays_under_the_hard_cap() {
            // 40 000 caractères de guillemets, d'antislashs et d'emoji : le JSON en pèse bien plus
            // de 45 000 ; la vue raccourcit la fenêtre, `next_offset` restant exact.
            let text = "\"\\😀 ".repeat(10_000);
            let fixture = Fixture::default();
            fixture.corpus.push_get_decision(Ok(decision(JURI, &text)));

            let result = call(
                &fixture,
                &json!({"decision_id": JURI, "detail": "full", "max_chars": 40_000}),
            )
            .await;

            let view = result.structured_content.unwrap();
            let size = compact_chars(&view);
            assert!(size <= 45_000, "{size} caractères");
            let kept = view["text"].as_str().unwrap().chars().count();
            assert!(kept < 40_000);
            assert_eq!(view["next_offset"], kept);
        }
    }
}
