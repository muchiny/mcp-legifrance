//! Outil `fr_search_all` (05 §5.13) : un premier repérage multi-fonds en un appel.
//!
//! Le motif est celui de `tools/mod.rs` : arguments bruts (`JsonObject`), décodés par
//! [`decode::to_call`] (l'échec voyage dans l'appel et devient un `invalid_arg` audité), schémas
//! explicites sans `$schema`, jeton d'annulation transmis à la façade.
//!
//! # Progression (05 §2.4, optionnelle)
//!
//! Quand le client fournit un `progressToken`, chaque fond terminé donne une
//! `notifications/progress` (`progress` = fonds terminés, `total` = fonds demandés). La façade
//! appelle un rappel synchrone ; il pose les couples dans un canal, et [`relay`] les envoie, en
//! parallèle de l'appel (`tokio::join!`, sans tâche détachée). Après une annulation, rmcp ne jette
//! que la réponse et laisse partir les notifications, alors que la spécification interdit tout
//! message ultérieur pour cette requête : [`relay`] teste donc `ct.is_cancelled()` avant chaque
//! notification et écarte ce qui reste dans le canal. Une notification qui échoue (client parti)
//! ne fait pas échouer l'appel.

use std::future::Future;

use lf_application::{
    input::SearchAllInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{
    Json, Peer, RoleServer,
    model::{JsonObject, ProgressNotificationParam, RequestMetaObject},
};
use rmcp_macros::{tool, tool_router};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio_util::sync::CancellationToken;

use crate::{
    decode,
    failure::ToolFailure,
    params::search_all::SearchAllParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::search_all::SearchAllView,
};

/// Description de l'outil (05 §5.13, 3 phrases). Source unique : elle ne cite que `fr_search`.
const DESCRIPTION: &str = "Search several Légifrance fonds at once (default LODA, CODE, JURI, \
CETAT, CONSTIT) and return the top `per_fond` hits of each (default 3, max 10), grouped by fond \
with their totals. Use it for a first overview when you do not know where the answer is; then \
refine with fr_search on the relevant fond, which supports paging and filters. A fond that fails \
yields an error entry without failing the others.";

#[tool_router(router = search_all_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_search_all",
        title = "Search several fonds",
        description = DESCRIPTION,
        input_schema = input_schema::<SearchAllParams>(),
        output_schema = output_schema::<SearchAllView>(),
        annotations(
            title = "Search several fonds",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_search_all(
        &self,
        args: JsonObject,
        ct: CancellationToken,
        meta: RequestMetaObject,
        peer: Peer<RoleServer>,
    ) -> Result<Json<SearchAllView>, ToolFailure> {
        let call = decode::to_call::<SearchAllParams, SearchAllInput>(args);
        let (sender, receiver) = unbounded_channel();
        // Le rappel possède l'émetteur : quand l'appel se termine ou est abandonné, le canal se
        // ferme et `relay` rend la main.
        let work =
            self.service.search_all_with_progress(call, ct.cancelled(), move |done, total| {
                let _ = sender.send((done, total));
            });
        let token = meta.get_progress_token();
        let notify = |done: u32, total: u32| {
            let peer = &peer;
            let token = token.clone();
            async move {
                if let Some(token) = token {
                    let params = ProgressNotificationParam::new(token, f64::from(done))
                        .with_total(f64::from(total));
                    // Le client a pu partir : la progression est facultative.
                    let _ = peer.notify_progress(params).await;
                }
            }
        };
        let (output, ()) = tokio::join!(work, relay(receiver, &ct, notify));
        Ok(Json(SearchAllView::from(output.map_err(ToolFailure)?)))
    }
}

/// Envoie la progression `(faits, total)` reçue de la façade, sans jamais rien émettre après
/// l'annulation de la requête : le jeton est testé avant chaque envoi, et ce qui reste dans le
/// canal est écarté. Rend la main quand l'émetteur est fermé.
async fn relay<F: Future<Output = ()>>(
    mut progress: UnboundedReceiver<(u32, u32)>,
    ct: &CancellationToken,
    mut notify: impl FnMut(u32, u32) -> F,
) {
    while let Some((done, total)) = progress.recv().await {
        if !ct.is_cancelled() {
            notify(done, total).await;
        }
    }
}

// Les tests passent par `testing::connect`, qui n'existe qu'avec la feature `test-support`.
#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use lf_application::{
        error::ErrorCode,
        ports::{AuditStatus, CorpusError},
        testing::{FakeCorpus, PortRequest},
        tools::ToolName,
    };
    use lf_domain::{
        date::CivilDate,
        entity::{SearchHit, SearchPage, Snippet},
        fond::{Fond, PaginationMode},
        id::DocumentId,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
    use serde_json::{Value, json};
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;
    use crate::{
        lifecycle::testkit::{Wire, ping},
        testing::Fixture,
    };

    fn page(fond: Fond, hits: u32, total: u64) -> SearchPage {
        let hits = (1..=hits)
            .map(|n| {
                let id = DocumentId::parse(&format!("{}{n:012}", fond.hit_prefix())).unwrap();
                SearchHit {
                    title: Some(format!("{} décision {n}", fond.name())),
                    date: CivilDate::new(2024, 1, 2),
                    snippet: Snippet::new(&"mot ".repeat(70)),
                    ..SearchHit::new(fond, id, None).unwrap()
                }
            })
            .collect();
        SearchPage::new(fond, "bail", Some(total), None, hits, 0, PaginationMode::Defaut)
    }

    async fn call(fixture: &Fixture, arguments: &Value) -> CallToolResult {
        let client = fixture.connect().await;
        let mut params = CallToolRequestParams::new("fr_search_all");
        params.arguments = arguments.as_object().cloned();
        client.call_tool(params).await.unwrap()
    }

    async fn descriptor(fixture: &Fixture) -> Tool {
        let client = fixture.connect().await;
        let tools = client.list_all_tools().await.unwrap();
        tools
            .into_iter()
            .find(|tool| tool.name == "fr_search_all")
            .expect("fr_search_all est enregistré")
    }

    fn text_of(result: &CallToolResult) -> &str {
        &result.content[0].as_text().unwrap().text
    }

    #[tokio::test]
    async fn the_groups_come_back_in_the_requested_order_with_one_audit_line() {
        let fixture = Fixture::default();
        fixture.corpus.push_search(Ok(page(Fond::Juri, 2, 51_531)));
        fixture.corpus.push_search(Ok(page(Fond::Loda, 1, 1)));

        let result =
            call(&fixture, &json!({"query": "bail", "fonds": ["juri", "LODA"], "per_fond": 2}))
                .await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.clone().unwrap();
        let groups = content["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["fond"], "JURI");
        assert_eq!(groups[0]["total"], 51_531);
        assert_eq!(groups[0]["has_more"], true);
        assert_eq!(groups[0]["hits"].as_array().unwrap().len(), 2);
        assert_eq!(groups[0]["hits"][0]["kind"], "juri");
        assert_eq!(groups[1]["fond"], "LODA");
        assert_eq!(groups[1]["has_more"], false);
        // Le bloc texte porte le même JSON que `structuredContent`.
        assert_eq!(serde_json::from_str::<Value>(text_of(&result)).unwrap(), content);

        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_SEARCH_ALL);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 3, "la somme des hits rendus");
        assert_eq!(fixture.corpus.calls_of("search"), 2);
        assert!(
            fixture
                .corpus
                .seen_requests()
                .iter()
                .all(|request| matches!(request, PortRequest::Search(_)))
        );
    }

    /// D-10 (`search_all-9`) : tous les fonds vides, la réponse porte la note, valide contre
    /// l'`outputSchema` (champ `note` existant, aucun champ nouveau).
    #[tokio::test]
    async fn a_search_with_no_result_anywhere_carries_the_no_match_note() {
        let fixture = Fixture::default();
        let tool = descriptor(&fixture).await;
        let schema = Value::Object(tool.output_schema.unwrap().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();
        for fond in [Fond::Loda, Fond::Juri] {
            fixture.corpus.push_search(Ok(page(fond, 0, 0)));
        }

        let result =
            call(&fixture, &json!({"query": "zzqxwv pfrtgh", "fonds": ["LODA", "JURI"]})).await;

        assert_eq!(result.is_error, Some(false));
        let content = result.structured_content.unwrap();
        assert!(validator.is_valid(&content), "{content}");
        assert_eq!(
            content,
            json!({
                "groups": [
                    {"fond": "LODA", "total": 0, "has_more": false, "hits": []},
                    {"fond": "JURI", "total": 0, "has_more": false, "hits": []}
                ],
                "note": "No match in any fond: try other words or other fonds, or call \
                         fr_coverage; an empty result does not mean the law is silent."
            })
        );
    }

    #[tokio::test]
    async fn a_failing_fond_is_an_error_entry_and_does_not_fail_the_call() {
        let fixture = Fixture::default();
        fixture.corpus.push_search(Ok(page(Fond::Loda, 1, 1)));
        fixture.corpus.push_search(Err(CorpusError::Timeout));

        let result = call(&fixture, &json!({"query": "bail", "fonds": ["LODA", "JURI"]})).await;

        assert_eq!(result.is_error, Some(false));
        let groups = result.structured_content.unwrap()["groups"].clone();
        assert_eq!(groups[0]["fond"], "LODA");
        assert_eq!(groups[1]["fond"], "JURI");
        assert!(groups[1].get("hits").is_none() && groups[1].get("total").is_none());
        let error = groups[1]["error"].as_str().unwrap();
        assert!(error.starts_with("[upstream_error] "), "{error}");
        assert_eq!(fixture.audit.records()[0].status, AuditStatus::Ok);
    }

    #[tokio::test]
    async fn when_every_fond_fails_the_same_way_the_call_fails_with_that_code() {
        let fixture = Fixture::default();
        for _ in 0..5 {
            fixture.corpus.push_search(Err(CorpusError::Timeout));
        }
        let result = call(&fixture, &json!({"query": "bail"})).await;
        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.is_none());
        assert!(text_of(&result).starts_with("[upstream_error] "));
        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::UpstreamError));
    }

    #[tokio::test]
    async fn the_default_request_asks_five_fonds_three_hits_each() {
        let fixture = Fixture::default();
        for fond in [Fond::Loda, Fond::Code, Fond::Juri, Fond::Cetat, Fond::Constit] {
            fixture.corpus.push_search(Ok(page(fond, 3, 9)));
        }
        let result = call(&fixture, &json!({"query": "bail"})).await;
        let groups = result.structured_content.unwrap()["groups"].clone();
        let fonds: Vec<&str> =
            groups.as_array().unwrap().iter().map(|g| g["fond"].as_str().unwrap()).collect();
        assert_eq!(fonds, ["LODA", "CODE", "JURI", "CETAT", "CONSTIT"]);
        assert_eq!(fixture.audit.records()[0].output_count_or_size, 15);
    }

    #[tokio::test]
    async fn invalid_arguments_are_audited_invalid_arg_without_network() {
        let fixture = Fixture::default();
        let wrong_type = call(&fixture, &json!({"query": 42})).await;
        assert_eq!(
            text_of(&wrong_type),
            "[invalid_arg] query: invalid type: integer `42`, expected a string"
        );
        let missing = call(&fixture, &json!({})).await;
        assert_eq!(text_of(&missing), "[invalid_arg] query is required.");
        let empty = call(&fixture, &json!({"query": "bail", "fonds": []})).await;
        assert!(text_of(&empty).starts_with("[invalid_arg] fonds is empty"));
        let twice = call(&fixture, &json!({"query": "bail", "fonds": ["LODA", "loda"]})).await;
        assert!(text_of(&twice).contains("more than once"));
        let range = call(&fixture, &json!({"query": "bail", "per_fond": 11})).await;
        assert_eq!(text_of(&range), "[invalid_arg] per_fond=11 out of range (1..10).");

        let records = fixture.audit.records();
        assert_eq!(records.len(), 5, "une ligne d'audit par appel");
        assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        assert_eq!(fixture.corpus.calls(), 0, "aucun appel au port");
    }

    #[tokio::test]
    async fn the_response_validates_against_the_advertised_output_schema() {
        let fixture = Fixture::default();
        let tool = descriptor(&fixture).await;
        let schema = Value::Object(tool.output_schema.unwrap().as_ref().clone());
        let validator = jsonschema::validator_for(&schema).unwrap();

        fixture.corpus.push_search(Ok(page(Fond::Juri, 3, 30)));
        fixture.corpus.push_search(Err(CorpusError::Timeout));
        let result = call(&fixture, &json!({"query": "bail", "fonds": ["JURI", "CNIL"]})).await;
        let content = result.structured_content.unwrap();
        assert!(validator.is_valid(&content), "{content}");
    }

    #[tokio::test]
    async fn the_descriptor_follows_the_authoring_rules() {
        let tool = descriptor(&Fixture::default()).await;

        assert_eq!(tool.name, "fr_search_all");
        assert_eq!(tool.title.as_deref(), Some("Search several fonds"));
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
        assert_eq!(input["required"], json!(["query"]));
        assert_eq!(input["properties"]["fonds"]["maxItems"], 8);
        assert!(input.get("$schema").is_none());
        let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
        assert_eq!(output["required"], json!(["groups"]));
        assert!(output.get("$schema").is_none());
        jsonschema::validator_for(&input).unwrap();
        jsonschema::validator_for(&output).unwrap();
    }

    // ---------------------------------------------------------------------------------------
    // Progression
    // ---------------------------------------------------------------------------------------

    /// `tools/call` de `fr_search_all` (ère legacy), avec un `progressToken` si `token` est donné.
    fn search_all_call(id: u64, arguments: &Value, token: Option<&str>) -> Value {
        let mut params = json!({"name": "fr_search_all", "arguments": arguments});
        if let Some(token) = token {
            params["_meta"] = json!({"progressToken": token});
        }
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params})
    }

    /// Un serveur de test relié à un client brut, poignée de main faite.
    async fn handshaken(fixture: &Fixture) -> Wire {
        let mut wire = Wire::serve(fixture.server());
        wire.handshake().await;
        wire
    }

    #[tokio::test(start_paused = true)]
    async fn each_finished_fond_sends_a_progress_notification_before_the_response() {
        let fixture = Fixture::default();
        for fond in [Fond::Loda, Fond::Juri, Fond::Cetat] {
            fixture.corpus.push_search(Ok(page(fond, 1, 1)));
        }
        let mut wire = handshaken(&fixture).await;

        let arguments = json!({"query": "bail", "fonds": ["LODA", "JURI", "CETAT"]});
        wire.send(&search_all_call(2, &arguments, Some("p1"))).await;
        let mut progress = Vec::new();
        let response = loop {
            let line = wire.recv().await;
            if line["method"] == "notifications/progress" {
                progress.push(line["params"].clone());
            } else {
                break line;
            }
        };

        assert_eq!(response["id"], 2);
        assert_eq!(response["result"]["isError"], false, "{response}");
        let counts: Vec<(f64, f64)> = progress
            .iter()
            .map(|params| (params["progress"].as_f64().unwrap(), params["total"].as_f64().unwrap()))
            .collect();
        assert_eq!(counts, [(1.0, 3.0), (2.0, 3.0), (3.0, 3.0)]);
        assert!(progress.iter().all(|params| params["progressToken"] == "p1"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_without_a_progress_token_gets_the_response_and_nothing_else() {
        let fixture = Fixture::default();
        fixture.corpus.push_search(Ok(page(Fond::Loda, 1, 1)));
        fixture.corpus.push_search(Ok(page(Fond::Juri, 1, 1)));
        let mut wire = handshaken(&fixture).await;

        wire.send(&search_all_call(2, &json!({"query": "bail", "fonds": ["LODA", "JURI"]}), None))
            .await;
        let first = wire.recv().await;
        assert_eq!(first["id"], 2, "la réponse arrive sans notification : {first}");
        assert_eq!(first["result"]["isError"], false);
        // Rien ne suit : la réponse au ping est la ligne suivante.
        wire.send(&ping(3)).await;
        assert_eq!(wire.recv().await["id"], 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_cancelled_request_gets_no_response_and_no_progress() {
        let fixture = Fixture {
            corpus: Arc::new(FakeCorpus::new().with_delay(Duration::from_secs(1))),
            ..Fixture::default()
        };
        for fond in [Fond::Loda, Fond::Juri] {
            fixture.corpus.push_search(Ok(page(fond, 1, 1)));
        }
        let mut wire = handshaken(&fixture).await;

        let arguments = json!({"query": "bail", "fonds": ["LODA", "JURI"]});
        wire.send(&search_all_call(2, &arguments, Some("p1"))).await;
        // Les deux recherches sont parties et attendent leur délai.
        while fixture.corpus.calls() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        wire.send(&json!({
            "jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 2}
        }))
        .await;
        tokio::time::sleep(Duration::from_secs(10)).await;
        // Ni réponse ni progression : la ligne suivante est celle du ping.
        wire.send(&ping(3)).await;
        assert_eq!(wire.recv().await["id"], 3);

        let records = fixture.audit.records();
        assert_eq!(records.len(), 1, "une ligne d'audit, même annulé");
        assert_eq!(records[0].tool, ToolName::FR_SEARCH_ALL);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, None);
    }

    /// Les couples que `relay` envoie, pour un canal rempli de `pairs` puis fermé.
    async fn relayed(
        pairs: &[(u32, u32)],
        ct: &CancellationToken,
        cancel_at: Option<u32>,
    ) -> Vec<(u32, u32)> {
        let sent = Mutex::new(Vec::new());
        let (sender, receiver) = unbounded_channel();
        for pair in pairs {
            sender.send(*pair).unwrap();
        }
        drop(sender);
        relay(receiver, ct, |done, total| {
            sent.lock().unwrap().push((done, total));
            if cancel_at == Some(done) {
                ct.cancel();
            }
            std::future::ready(())
        })
        .await;
        sent.into_inner().unwrap()
    }

    #[tokio::test]
    async fn the_relay_sends_every_pair_while_the_request_lives() {
        let sent = relayed(&[(1, 3), (2, 3), (3, 3)], &CancellationToken::new(), None).await;
        assert_eq!(sent, [(1, 3), (2, 3), (3, 3)]);
        assert_eq!(relayed(&[], &CancellationToken::new(), None).await, []);
    }

    #[tokio::test]
    async fn no_progress_after_cancel() {
        // La requête est annulée pendant l'envoi de la deuxième notification : les trois
        // suivantes, déjà dans le canal, ne partent pas.
        let ct = CancellationToken::new();
        let sent = relayed(&[(1, 5), (2, 5), (3, 5), (4, 5), (5, 5)], &ct, Some(2)).await;
        assert_eq!(sent, [(1, 5), (2, 5)]);
        assert!(ct.is_cancelled());
    }

    #[tokio::test]
    async fn nothing_is_sent_for_a_request_cancelled_before_any_fond_finished() {
        let ct = CancellationToken::new();
        ct.cancel();
        assert_eq!(relayed(&[(1, 2), (2, 2)], &ct, None).await, []);
    }
}
