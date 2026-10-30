//! Tests du spike rmcp 3.5.1 (unité 0A du plan « fin du projet », V-28 et V-29).
//!
//! Chaque test rejoue une transcription JSON-RPC brute sur un `tokio::io::duplex`, comme le
//! harnais `protocol_raw` de 06 §6.1 : le serveur tourne derrière `PreLifecycleFilter` et
//! `AsyncRwTransport::new_server`, exactement comme en production sur stdin et stdout.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "harnais de test brut : l'échec d'une lecture ou d'une écriture doit faire tomber le test"
)]

use std::{
    io::{BufRead, Write},
    pin::Pin,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use lf_application::{
    audited::Audited,
    error::ErrorCode,
    ports::{AuditError, AuditLog, AuditRecord, AuditStatus},
    testing::{FakeCorpus, MemoryAuditLog},
};
use rmcp::{
    ErrorData, ServiceExt,
    handler::server::{
        common::schema_for_empty_input, router::tool::ToolRoute, tool::ToolCallContext,
        wrapper::Parameters,
    },
    model::{CallToolResponse, CallToolResult, ContentBlock, Tool},
    service::ServerInitializeError,
    transport::async_rw::AsyncRwTransport,
};
use rmcp_stdio_spike::{
    PreLifecycleFilter, SearchParams, ServeError, ServeOutcome, Spike, input_schema, serve_io,
    spike_policy,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

/// Délai au-delà duquel une réponse attendue est déclarée manquante.
const WAIT: Duration = Duration::from_secs(5);

/// Journal d'audit partagé : `Audited` prend son journal par valeur, le test garde l'autre bout.
#[derive(Clone, Default)]
struct SharedLog(Arc<MemoryAuditLog>);

impl SharedLog {
    fn records(&self) -> Vec<AuditRecord> {
        self.0.records()
    }

    /// Attend `count` lignes d'audit (au plus [`WAIT`]).
    async fn wait_for(&self, count: usize) -> Vec<AuditRecord> {
        let started = Instant::now();
        loop {
            let records = self.records();
            if records.len() >= count {
                return records;
            }
            assert!(started.elapsed() < WAIT, "audit: {} ligne(s) sur {count}", records.len());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl AuditLog for SharedLog {
    async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        self.0.record(record).await
    }
}

type TestSpike = Spike<FakeCorpus, SharedLog>;

fn new_spike(latency: Duration) -> (TestSpike, SharedLog) {
    let log = SharedLog::default();
    let audited = Arc::new(Audited::new(log.clone(), spike_policy()));
    (Spike::new(Arc::new(FakeCorpus::new()), audited).with_latency(latency), log)
}

/// Client brut : écrit des lignes JSON et lit les lignes du serveur.
struct Wire {
    write: Option<WriteHalf<DuplexStream>>,
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    server: JoinHandle<Result<ServeOutcome, ServeError>>,
    shutdown: CancellationToken,
}

impl Wire {
    fn start(spike: TestSpike) -> Self {
        let (client, server) = tokio::io::duplex(1 << 20);
        let (server_read, server_write) = tokio::io::split(server);
        let shutdown = CancellationToken::new();
        let server = tokio::spawn(serve_io(spike, server_read, server_write, shutdown.clone()));
        let (read, write) = tokio::io::split(client);
        Self { write: Some(write), lines: BufReader::new(read).lines(), server, shutdown }
    }

    async fn send(&mut self, message: &Value) {
        let write = self.write.as_mut().expect("client stdin already closed");
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        write.write_all(&line).await.unwrap();
        write.flush().await.unwrap();
    }

    /// Prochaine ligne brute, ou `None` après `wait` sans rien (ou à la fin du flux).
    async fn try_recv_raw(&mut self, wait: Duration) -> Option<String> {
        tokio::time::timeout(wait, self.lines.next_line()).await.ok().and_then(Result::unwrap)
    }

    async fn recv_raw(&mut self) -> String {
        self.try_recv_raw(WAIT).await.expect("no line from the server")
    }

    async fn recv(&mut self) -> Value {
        serde_json::from_str(&self.recv_raw().await).unwrap()
    }

    /// Lit jusqu'à la réponse d'identifiant `id` ; rend aussi les lignes lues avant elle.
    async fn recv_id(&mut self, id: u64) -> (Value, Vec<Value>) {
        let mut before = Vec::new();
        loop {
            let message = self.recv().await;
            if message["id"] == json!(id) {
                return (message, before);
            }
            before.push(message);
        }
    }

    /// Poignée de main legacy 2025-11-25, puis `notifications/initialized`.
    async fn initialize(&mut self) {
        self.send(&initialize(1)).await;
        let response = self.recv().await;
        assert_eq!(response["id"], json!(1), "{response}");
        assert_eq!(response["result"]["protocolVersion"], json!("2025-11-25"), "{response}");
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
    }

    async fn call(&mut self, id: u64, name: &str, arguments: &Value) -> Value {
        self.send(&call_tool(id, name, arguments)).await;
        self.recv_id(id).await.0
    }

    /// Ferme stdin (EOF) et attend la fin du serveur.
    async fn close(mut self) -> (Result<ServeOutcome, ServeError>, Duration) {
        // `shutdown` et non `drop` : la moitié de lecture garde le duplex en vie.
        let mut write = self.write.take().expect("client stdin already closed");
        write.shutdown().await.unwrap();
        let started = Instant::now();
        let outcome = tokio::time::timeout(Duration::from_secs(10), self.server)
            .await
            .expect("server did not stop after EOF")
            .unwrap();
        (outcome, started.elapsed())
    }
}

fn initialize(id: u64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": {
        "protocolVersion": "2025-11-25",
        "capabilities": {},
        "clientInfo": {"name": "spike-test", "version": "0.0.0"}
    }})
}

fn call_tool(id: u64, name: &str, arguments: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": name, "arguments": arguments}})
}

fn text_of(response: &Value) -> &str {
    response["result"]["content"][0]["text"].as_str().unwrap_or_default()
}

/// Réponse boxée d'une route écrite à la main (`DynCallToolHandler`, `tool.rs:192-194`).
type RouteFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CallToolResponse, ErrorData>> + Send + 'a>>;

/// Route de test : renvoie ses arguments bruts en texte.
fn echo_route(context: ToolCallContext<'_, TestSpike>) -> RouteFuture<'_> {
    let arguments = context.arguments.unwrap_or_default();
    Box::pin(async move {
        let text = serde_json::to_string(&arguments).unwrap();
        Ok(CallToolResponse::from(CallToolResult::success(vec![ContentBlock::text(text)])))
    })
}

/// Route de test : panique (V-36).
fn boom_route(_: ToolCallContext<'_, TestSpike>) -> RouteFuture<'_> {
    Box::pin(async { panic!("tool panic (spike)") })
}

/// Outil à extracteur typé `Parameters<P>`, pour le contraste avec `JsonObject`.
fn typed_tool(
    _: &TestSpike,
    Parameters(_params): Parameters<SearchParams>,
) -> Pin<Box<dyn Future<Output = String> + Send + '_>> {
    Box::pin(async { "reached".to_owned() })
}

#[tokio::test]
async fn generic_server_combines_routers() {
    let (spike, _) = new_spike(Duration::ZERO);
    // Trois blocs #[tool_router(router = …, vis = "pub(crate)")] combinés par `+`, dont un vide
    // (`allow_empty`) : le routeur du champ porte les outils des deux blocs non vides.
    let names: Vec<_> = spike.router().list_all().into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, ["fr_coverage", "fr_search"]);
    assert_eq!(Arc::strong_count(spike.corpus()), 1);

    let mut wire = Wire::start(spike);
    wire.initialize().await;
    wire.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let listed = wire.recv().await;
    let names: Vec<_> =
        listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].clone()).collect();
    assert_eq!(names, [json!("fr_coverage"), json!("fr_search")]);
    let coverage = wire.call(3, "fr_coverage", &json!({})).await;
    assert_eq!(coverage["result"]["structuredContent"]["tools"], json!(2), "{coverage}");
}

#[tokio::test]
async fn tool_handler_dispatches_to_router_field() {
    let (mut spike, _) = new_spike(Duration::ZERO);
    // `#[tool_handler(router = self.tool_router)]` : call_tool, list_tools et get_tool lisent le
    // champ ; un routeur reconstruit (Self::tool_router()) garderait fr_coverage.
    spike.router_mut().remove_route("fr_coverage");
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    wire.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let listed = wire.recv().await;
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 1, "{listed}");
    let removed = wire.call(3, "fr_coverage", &json!({})).await;
    assert_eq!(removed["error"]["code"], json!(-32602), "{removed}");
    assert_eq!(removed["error"]["message"], json!("tool not found"), "{removed}");
    let kept = wire.call(4, "fr_search", &json!({"query": "bail"})).await;
    assert_eq!(kept["result"]["isError"], json!(false), "{kept}");
}

#[tokio::test]
async fn jsonobject_tool_receives_raw_arguments() {
    let (spike, log) = new_spike(Duration::ZERO);
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    let response = wire.call(2, "fr_search", &json!({"page_size": 20, "query": "bail"})).await;
    assert_eq!(response["result"]["isError"], json!(false), "{response}");
    let structured = &response["result"]["structuredContent"];
    assert_eq!(*structured, json!({"query": "bail", "page_size": 20, "hits": []}));
    // Json<T> : le texte JSON est aussi rendu en bloc texte (02 §1.2 b).
    assert_eq!(serde_json::from_str::<Value>(text_of(&response)).unwrap(), *structured);
    // L'audit reçoit les arguments BRUTS, en JSON canonique (clés triées).
    let records = log.wait_for(1).await;
    assert_eq!(records[0].input.as_str(), r#"{"page_size":20,"query":"bail"}"#);
    assert_eq!(records[0].status, AuditStatus::Ok);
    assert_eq!(records[0].output_count_or_size, 0);
}

#[tokio::test]
async fn invalid_arguments_reach_the_tool() {
    let (spike, log) = new_spike(Duration::ZERO);
    let mut wire = Wire::start(spike);
    wire.initialize().await;

    // Type invalide, champ inconnu, objet vide : décodés par NOTRE code, isError + [invalid_arg].
    let cases = [
        (
            json!({"page_size": "x"}),
            "[invalid_arg] page_size: invalid type: string \"x\", expected i64",
        ),
        (
            json!({"query": "bail", "foo": 1}),
            "[invalid_arg] foo: unknown field `foo`, expected `query` or `page_size`",
        ),
        (json!({}), "[invalid_arg] query is required."),
    ];
    for (id, (arguments, expected)) in (2..).zip(&cases) {
        let response = wire.call(id, "fr_search", arguments).await;
        assert_eq!(response["result"]["isError"], json!(true), "{response}");
        assert_eq!(text_of(&response), *expected, "{response}");
        assert!(response["result"].get("structuredContent").is_none(), "{response}");
        assert!(!text_of(&response).contains("failed to deserialize"), "{response}");
    }
    // Chaque rejet est audité (D-02) : une ligne par appel, en invalid_arg.
    let records = log.wait_for(cases.len()).await;
    assert!(records.iter().all(|r| r.error_code == Some(ErrorCode::InvalidArg)));
    assert_eq!(records[1].input.as_str(), r#"{"foo":1,"query":"bail"}"#);

    // `arguments` absent : l'extracteur JsonObject rend un objet vide (tool.rs:253-257).
    wire.send(&json!({"jsonrpc": "2.0", "id": 10, "method": "tools/call",
                      "params": {"name": "fr_search"}}))
        .await;
    let (absent, _) = wire.recv_id(10).await;
    assert_eq!(text_of(&absent), "[invalid_arg] query is required.", "{absent}");

    // `arguments` qui n'est pas un objet : rejeté par rmcp au décodage de la requête, avant notre
    // code et sans audit. Le code observé est -32601 (méthode introuvable), pas -32602 : c'est
    // le #1347 amont de 09 R-02.
    let not_object = wire.call(11, "fr_search", &json!("bail")).await;
    assert_eq!(not_object["error"]["code"], json!(-32601), "{not_object}");
    assert_eq!(log.records().len(), cases.len() + 1);

    // Contraste : l'extracteur Parameters<P> rend « failed to deserialize parameters » (message
    // fixé en tool.rs:244, converti en isError par router/tool.rs:146-155 et :575), hors audit.
    let (mut typed, _) = new_spike(Duration::ZERO);
    typed.router_mut().add_route(ToolRoute::new(
        Tool::new("typed", "typed", input_schema::<SearchParams>()),
        typed_tool,
    ));
    let mut wire = Wire::start(typed);
    wire.initialize().await;
    let typed = wire.call(2, "typed", &json!({"page_size": "x"})).await;
    assert_eq!(typed["result"]["isError"], json!(true), "{typed}");
    assert!(text_of(&typed).starts_with("failed to deserialize parameters: "), "{typed}");
}

#[tokio::test]
async fn test_route_can_be_added() {
    let (mut spike, _) = new_spike(Duration::ZERO);
    // API de ToolRouter pour une route de test (V1-10) : add_route(ToolRoute::new_dyn(..)).
    spike.router_mut().add_route(ToolRoute::new_dyn(
        Tool::new("echo", "echo the arguments", schema_for_empty_input()),
        echo_route,
    ));
    spike.router_mut().add_route(ToolRoute::new_dyn(
        Tool::new("boom", "panics", schema_for_empty_input()),
        boom_route,
    ));
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    let echoed = wire.call(2, "echo", &json!({"a": [1, 2]})).await;
    assert_eq!(text_of(&echoed), r#"{"a":[1,2]}"#, "{echoed}");

    // Une panique dans un outil (V-36) : rmcp ne rend RIEN pour cet appel (la tâche de la
    // requête meurt, service.rs:1603-1618), mais le serveur continue.
    wire.send(&call_tool(3, "boom", &json!({}))).await;
    wire.send(&json!({"jsonrpc": "2.0", "id": 4, "method": "ping"})).await;
    let (pong, before) = wire.recv_id(4).await;
    assert_eq!(pong["result"], json!({}), "{pong}");
    assert!(before.is_empty(), "{before:?}");
    assert_eq!(wire.try_recv_raw(Duration::from_millis(300)).await, None);
}

#[tokio::test]
async fn tools_list_has_explicit_input_and_output_schema() {
    let (spike, _) = new_spike(Duration::ZERO);
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    wire.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let raw = wire.recv_raw().await;
    let listed: Value = serde_json::from_str(&raw).unwrap();
    let tools = listed["result"]["tools"].as_array().unwrap();
    let search = &tools[1];
    assert_eq!(search["name"], json!("fr_search"));
    assert_eq!(search["title"], json!("Search Légifrance (spike)"));
    assert_eq!(
        search["inputSchema"],
        json!({
            "type": "object",
            "properties": {
                "query": {"description": "Free-text query.", "type": ["string", "null"]},
                "page_size": {"description": "Results per page (1..100).", "type": ["integer", "null"], "format": "int64"}
            },
            "additionalProperties": false
        }),
        "{search}"
    );
    // outputSchema explicite : sans `$schema` (05 §2.5).
    assert_eq!(search["outputSchema"]["type"], json!("object"), "{search}");
    assert!(search["outputSchema"].get("$schema").is_none(), "{search}");
    assert_eq!(search["annotations"]["readOnlyHint"], json!(true), "{search}");
    // outputSchema DÉDUIT de `Result<Json<CoverageView>, ToolFailure>` écrit littéralement :
    // présent, mais avec le `$schema` de schemars (d'où l'attribut explicite).
    let coverage = &tools[0];
    assert_eq!(coverage["name"], json!("fr_coverage"));
    assert_eq!(coverage["inputSchema"], json!({"type": "object", "properties": {}}));
    assert!(coverage["outputSchema"].get("$schema").is_some(), "{coverage}");
    // Taille du tools/list des deux outils (README) : ligne JSON-RPC complète et tableau seul.
    let tools_bytes = serde_json::to_string(&listed["result"]["tools"]).unwrap().len();
    assert_eq!((raw.len(), tools_bytes), (1266, 1222), "tools/list size (line, tools array)");
}

#[tokio::test]
async fn cancellation_reaches_audited_run() {
    // Latence simulée de 30 s : seule l'annulation peut finir l'appel.
    let (spike, log) = new_spike(Duration::from_secs(30));
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    wire.send(&call_tool(2, "fr_search", &json!({"query": "bail"}))).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = Instant::now();
    wire.send(&json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
                      "params": {"requestId": 2, "reason": "user"}}))
        .await;
    // Le jeton `ct` de rmcp, passé en `ct.cancelled()` à Audited::run, abandonne le travail et
    // donne une ligne d'audit « cancelled by client » sans code.
    let records = log.wait_for(1).await;
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(records[0].status, AuditStatus::Error);
    assert_eq!(records[0].error_code, None);
    assert_eq!(records[0].error.as_deref(), Some("cancelled by client"));
    // rmcp 3.5.1 JETTE la réponse de la requête annulée (service.rs:1503-1505).
    tokio::time::sleep(Duration::from_millis(100)).await;
    wire.send(&json!({"jsonrpc": "2.0", "id": 3, "method": "ping"})).await;
    let (_, before) = wire.recv_id(3).await;
    assert!(before.is_empty(), "response of the cancelled call was sent: {before:?}");
    assert_eq!(wire.try_recv_raw(Duration::from_millis(300)).await, None);
}

#[tokio::test]
async fn notification_before_initialize_is_ignored() {
    let premature = [
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 7}}),
        json!({"jsonrpc": "2.0", "id": 99, "result": {}}),
        json!({"jsonrpc": "2.0", "id": 98, "error": {"code": -1, "message": "client error"}}),
    ];
    let (spike, _) = new_spike(Duration::ZERO);
    let mut wire = Wire::start(spike);
    for message in &premature {
        wire.send(message).await;
    }
    // Un ping avant toute ère reste servi, et la poignée de main aboutit ensuite.
    wire.send(&json!({"jsonrpc": "2.0", "id": 5, "method": "ping"})).await;
    let (pong, before) = wire.recv_id(5).await;
    assert_eq!(pong["result"], json!({}), "{pong}");
    assert!(before.is_empty(), "{before:?}");
    wire.initialize().await;
    wire.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    assert_eq!(wire.recv().await["result"]["tools"].as_array().unwrap().len(), 2);

    // Contraste : sans le filtre, rmcp 3.5.1 arrête le service (service/server.rs:681-685).
    let (spike, _) = new_spike(Duration::ZERO);
    let (client, server) = tokio::io::duplex(1 << 16);
    let (read, write) = tokio::io::split(server);
    let serving = tokio::spawn(
        spike.serve_with_ct(AsyncRwTransport::new_server(read, write), CancellationToken::new()),
    );
    let (_client_read, mut client_write) = tokio::io::split(client);
    client_write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let failed = tokio::time::timeout(WAIT, serving).await.unwrap().unwrap();
    assert!(matches!(failed, Err(ServerInitializeError::ExpectedInitializeRequest(Some(_)))));
}

#[tokio::test]
async fn eof_exits_zero() {
    // EOF avant toute ère (sonde d'un client) : fin normale, rapide.
    let (spike, _) = new_spike(Duration::ZERO);
    let (outcome, elapsed) = Wire::start(spike).close().await;
    assert!(matches!(outcome, Ok(ServeOutcome::Eof)), "{outcome:?}");
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");

    // EOF après la poignée de main.
    let (spike, _) = new_spike(Duration::ZERO);
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    let (outcome, elapsed) = wire.close().await;
    assert!(matches!(outcome, Ok(ServeOutcome::Eof)), "{outcome:?}");
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");

    // EOF pendant un appel en vol (latence de 30 s) : le filtre annule le service à l'EOF, sinon
    // rmcp attendrait jusqu'à 5 s la réponse en vol (service.rs:1762-1771).
    let (spike, log) = new_spike(Duration::from_secs(30));
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    wire.send(&call_tool(2, "fr_search", &json!({"query": "bail"}))).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (outcome, elapsed) = wire.close().await;
    assert!(matches!(outcome, Ok(ServeOutcome::Eof)), "{outcome:?}");
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    assert_eq!(log.records().len(), 1);

    // Arrêt demandé (signal) : Shutdown, distinct de l'EOF.
    let (spike, _) = new_spike(Duration::ZERO);
    let mut wire = Wire::start(spike);
    wire.initialize().await;
    wire.shutdown.cancel();
    let outcome = tokio::time::timeout(WAIT, &mut wire.server).await.unwrap().unwrap();
    assert!(matches!(outcome, Ok(ServeOutcome::Shutdown)), "{outcome:?}");

    // Le binaire : stdin fermé après initialize, code 0, stdout ne porte que du JSON-RPC.
    let mut child = Command::new(env!("CARGO_BIN_EXE_rmcp-stdio-spike"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", initialize(1)).unwrap();
    drop(stdin);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(started.elapsed() < WAIT, "binary did not exit after EOF");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(0));
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
    let lines: Vec<_> =
        std::io::BufReader::new(child.stdout.take().unwrap()).lines().map(Result::unwrap).collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let response: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(response["jsonrpc"], json!("2.0"));
    assert_eq!(response["result"]["serverInfo"]["name"], json!("rmcp-stdio-spike"));
}

#[tokio::test]
async fn eof_without_cancel_on_eof_waits_for_the_drain() {
    // Contraste de `eof_exits_zero` : sans `cancel_on_eof`, l'EOF ferme le service avec
    // `QuitReason::Closed`, et rmcp attend jusqu'à 5 s la réponse de l'appel en vol
    // (service.rs:1762-1766) : bien au-delà de la seconde exigée par 00 §6.
    let (spike, _) = new_spike(Duration::from_secs(30));
    let (client, server) = tokio::io::duplex(1 << 20);
    let (server_read, server_write) = tokio::io::split(server);
    let transport =
        PreLifecycleFilter::new(AsyncRwTransport::new_server(server_read, server_write));
    let serving = tokio::spawn(spike.serve_with_ct(transport, CancellationToken::new()));
    let (client_read, mut write) = tokio::io::split(client);
    let mut lines = BufReader::new(client_read).lines();
    for message in [
        initialize(1),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        call_tool(2, "fr_search", &json!({"query": "bail"})),
    ] {
        let mut line = serde_json::to_vec(&message).unwrap();
        line.push(b'\n');
        write.write_all(&line).await.unwrap();
    }
    lines.next_line().await.unwrap().expect("initialize response");
    tokio::time::sleep(Duration::from_millis(200)).await;
    write.shutdown().await.unwrap();
    let started = Instant::now();
    let running = tokio::time::timeout(WAIT, serving).await.unwrap().unwrap().unwrap();
    let quit = tokio::time::timeout(Duration::from_secs(10), running.waiting()).await.unwrap();
    assert!(matches!(quit, Ok(rmcp::service::QuitReason::Closed)), "{quit:?}");
    assert!(started.elapsed() >= Duration::from_secs(4), "{:?}", started.elapsed());
}
