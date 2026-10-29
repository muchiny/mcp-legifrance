//! Cycle de vie stdio : filtre des messages d'avant l'ère, EOF et arrêt (annexe D §4.3).
//!
//! Trois comportements de rmcp 3.5.1 que ce module corrige (mesurés au spike, V-29 et V-36) :
//!
//! - une notification, une réponse ou une erreur reçue **avant** `initialize` arrête le service
//!   (`ServerInitializeError::ExpectedInitializeRequest`, `service/server.rs:681-685`) :
//!   [`PreLifecycleFilter`] les écarte ;
//! - à l'EOF, rmcp attend jusqu'à 5 s la réponse d'un appel en vol (`service.rs:1762-1766`) :
//!   le filtre annule le service à l'EOF, qui se termine alors en moins d'une seconde ;
//! - l'EOF ou l'arrêt avant toute ère (sonde d'un client) n'est pas une erreur : code 0.

use rmcp::{
    RoleServer, ServerHandler, ServiceExt,
    model::{ClientJsonRpcMessage, ClientRequest, GetMeta, ProtocolVersion},
    service::{QuitReason, RxJsonRpcMessage, ServerInitializeError, TxJsonRpcMessage},
    transport::{Transport, async_rw::AsyncRwTransport},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

use crate::{
    stdio::{ServeError, ServeOutcome},
    versions::SUPPORTED,
};

/// Écarte notifications, réponses et erreurs du client tant qu'aucune ère n'est ouverte, et annule
/// un jeton à la fin de l'entrée (EOF).
pub(crate) struct PreLifecycleFilter<T> {
    inner: T,
    opened: bool,
    eof: Option<CancellationToken>,
}

impl<T> PreLifecycleFilter<T> {
    /// Enveloppe `inner` : aucune ère n'est ouverte, aucun jeton n'est annulé à l'EOF.
    pub(crate) fn new(inner: T) -> Self {
        Self { inner, opened: false, eof: None }
    }

    /// Annule `token` quand l'entrée se ferme : le service n'attend alors pas les appels en vol.
    #[must_use]
    pub(crate) fn cancel_on_eof(mut self, token: CancellationToken) -> Self {
        self.eof = Some(token);
        self
    }
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for PreLifecycleFilter<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            let Some(message) = self.inner.receive().await else {
                if let Some(eof) = &self.eof {
                    eof.cancel();
                }
                return None;
            };
            if self.opened {
                return Some(message);
            }
            // Une requête reste toujours transmise : rmcp répond lui-même au `ping`, à
            // `server/discover` et aux requêtes sans `_meta` complet.
            if let ClientJsonRpcMessage::Request(request) = &message {
                self.opened = opens_lifecycle(&request.request);
                return Some(message);
            }
            tracing::debug!("pre-lifecycle client message dropped");
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

/// `initialize`, ou requête autre que `server/discover` et `ping` dont le `_meta` est complet et
/// la version dans [`SUPPORTED`] : la règle d'ouverture de rmcp (`service/server.rs:597-668`).
fn opens_lifecycle(request: &ClientRequest) -> bool {
    match request {
        ClientRequest::InitializeRequest(_) => true,
        ClientRequest::DiscoverRequest(_) | ClientRequest::PingRequest(_) => false,
        other => {
            let meta = other.get_meta();
            meta.missing_required_keys(&ProtocolVersion::V_2026_07_28).is_empty()
                && meta.protocol_version().is_some_and(|version| SUPPORTED.contains(&version))
        }
    }
}

/// Sert `server` sur `read`/`write` derrière [`PreLifecycleFilter`] ; `serve_stdio` y passe
/// `tokio::io::stdin()` et `tokio::io::stdout()`.
///
/// Deux jetons : `shutdown` est le jeton du processus, `serving` son enfant. L'EOF n'annule que
/// l'enfant, ce qui distingue [`ServeOutcome::Eof`] de [`ServeOutcome::Shutdown`].
pub(crate) async fn serve_io<S, R, W>(
    server: S,
    read: R,
    write: W,
    shutdown: CancellationToken,
) -> Result<ServeOutcome, ServeError>
where
    S: ServerHandler,
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let serving = shutdown.child_token();
    let transport = PreLifecycleFilter::new(AsyncRwTransport::new_server(read, write))
        .cancel_on_eof(serving.clone());
    let ended = match server.serve_with_ct(transport, serving).await {
        Ok(running) => quit_end(running.waiting().await),
        Err(error) => initialization_end(error),
    };
    ended.map(|()| if shutdown.is_cancelled() { ServeOutcome::Shutdown } else { ServeOutcome::Eof })
}

/// Fin du service après une ère ouverte : seule une tâche interrompue est un échec.
fn quit_end(quit: Result<QuitReason, tokio::task::JoinError>) -> Result<(), ServeError> {
    match quit {
        Ok(QuitReason::JoinError(error)) | Err(error) => Err(ServeError::Join(error)),
        Ok(_) => Ok(()),
    }
}

/// Fin du service sans ère ouverte : l'EOF ou l'arrêt avant `initialize` est normal.
fn initialization_end(error: ServerInitializeError) -> Result<(), ServeError> {
    match error {
        ServerInitializeError::ConnectionClosed(_) | ServerInitializeError::Cancelled => Ok(()),
        other => Err(ServeError::Initialize(Box::new(other))),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        sync::{Arc, Mutex, PoisonError},
        time::{Duration, Instant},
    };

    use pretty_assertions::assert_eq;
    use rmcp::{
        ErrorData,
        model::{
            Implementation, ListToolsResult, NotificationNoParam, PaginatedRequestParams,
            ServerCapabilities, ServerConfig,
        },
        service::RequestContext,
    };
    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf},
        task::JoinHandle,
    };

    use super::*;

    /// Borne d'attente : au-delà, une réponse attendue est déclarée manquante.
    const WAIT: Duration = Duration::from_secs(5);

    // ---- le filtre, sur un transport scénarisé -------------------------------------------

    /// Transport interne scénarisé : rend ses messages dans l'ordre puis l'EOF, et note ce qu'on
    /// lui envoie et s'il a été fermé.
    #[derive(Default)]
    struct Scripted {
        queue: VecDeque<RxJsonRpcMessage<RoleServer>>,
        sent: Arc<Mutex<Vec<TxJsonRpcMessage<RoleServer>>>>,
        closed: Arc<Mutex<bool>>,
    }

    impl Transport<RoleServer> for Scripted {
        type Error = Infallible;

        fn send(
            &mut self,
            item: TxJsonRpcMessage<RoleServer>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            let sent = Arc::clone(&self.sent);
            async move {
                sent.lock().unwrap_or_else(PoisonError::into_inner).push(item);
                Ok(())
            }
        }

        fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
            std::future::ready(self.queue.pop_front())
        }

        fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
            let closed = Arc::clone(&self.closed);
            async move {
                *closed.lock().unwrap_or_else(PoisonError::into_inner) = true;
                Ok(())
            }
        }
    }

    fn message(value: &Value) -> RxJsonRpcMessage<RoleServer> {
        serde_json::from_value(value.clone()).unwrap()
    }

    fn scripted(messages: &[Value]) -> Scripted {
        Scripted { queue: messages.iter().map(message).collect(), ..Scripted::default() }
    }

    fn initialize(id: u64) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "lifecycle-test", "version": "0.0.0"}
        }})
    }

    fn initialized() -> Value {
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    }

    fn cancelled() -> Value {
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 7}})
    }

    fn client_response() -> Value {
        json!({"jsonrpc": "2.0", "id": 99, "result": {}})
    }

    fn client_error() -> Value {
        json!({"jsonrpc": "2.0", "id": 98, "error": {"code": -1, "message": "client error"}})
    }

    fn ping(id: u64) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "ping"})
    }

    fn discover(id: u64) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "server/discover", "params": {"_meta": modern_meta("2026-07-28")}})
    }

    /// `_meta` d'une requête de l'ère moderne : version du protocole et capacités du client.
    fn modern_meta(version: &str) -> Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": version,
            "io.modelcontextprotocol/clientInfo": {"name": "lifecycle-test", "version": "0.0.0"},
            "io.modelcontextprotocol/clientCapabilities": {}
        })
    }

    fn tools_list(id: u64, meta: Option<Value>) -> Value {
        match meta {
            Some(meta) => {
                json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {"_meta": meta}})
            }
            None => json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}),
        }
    }

    /// Les `n` prochains messages que le filtre transmet, puis `None` si l'EOF suit.
    async fn drain(filter: &mut PreLifecycleFilter<Scripted>) -> Vec<Value> {
        let mut passed = Vec::new();
        while let Some(message) = filter.receive().await {
            passed.push(serde_json::to_value(message).unwrap());
        }
        passed
    }

    fn methods(messages: &[Value]) -> Vec<String> {
        messages
            .iter()
            .map(|message| {
                message["method"].as_str().map_or_else(|| "(réponse)".to_owned(), str::to_owned)
            })
            .collect()
    }

    #[tokio::test]
    async fn notifications_responses_and_errors_are_dropped_before_the_lifecycle_opens() {
        let mut filter = PreLifecycleFilter::new(scripted(&[
            initialized(),
            cancelled(),
            client_response(),
            client_error(),
            initialize(1),
        ]));
        assert_eq!(methods(&drain(&mut filter).await), ["initialize"]);
    }

    #[tokio::test]
    async fn after_initialize_every_message_passes_through() {
        let mut filter = PreLifecycleFilter::new(scripted(&[
            initialize(1),
            initialized(),
            client_response(),
            cancelled(),
        ]));
        assert_eq!(
            methods(&drain(&mut filter).await),
            ["initialize", "notifications/initialized", "(réponse)", "notifications/cancelled"]
        );
    }

    #[tokio::test]
    async fn ping_and_discover_do_not_open_the_lifecycle() {
        let mut filter = PreLifecycleFilter::new(scripted(&[
            ping(1),
            initialized(),
            discover(2),
            cancelled(),
            initialize(3),
            initialized(),
        ]));
        assert_eq!(
            methods(&drain(&mut filter).await),
            ["ping", "server/discover", "initialize", "notifications/initialized"]
        );
    }

    #[tokio::test]
    async fn a_modern_request_with_complete_meta_opens_the_lifecycle() {
        let mut filter = PreLifecycleFilter::new(scripted(&[
            tools_list(1, Some(modern_meta("2026-07-28"))),
            initialized(),
        ]));
        assert_eq!(methods(&drain(&mut filter).await), ["tools/list", "notifications/initialized"]);
    }

    #[tokio::test]
    async fn a_modern_request_without_meta_does_not_open_the_lifecycle() {
        let mut filter = PreLifecycleFilter::new(scripted(&[
            tools_list(1, None),
            initialized(),
            tools_list(2, Some(json!({"io.modelcontextprotocol/protocolVersion": "2026-07-28"}))),
            initialized(),
        ]));
        assert_eq!(methods(&drain(&mut filter).await), ["tools/list", "tools/list"]);
    }

    #[tokio::test]
    async fn a_modern_request_for_an_unsupported_version_does_not_open_the_lifecycle() {
        let mut filter = PreLifecycleFilter::new(scripted(&[
            tools_list(1, Some(modern_meta("2025-03-26"))),
            initialized(),
        ]));
        assert_eq!(methods(&drain(&mut filter).await), ["tools/list"]);
    }

    #[tokio::test]
    async fn eof_cancels_the_token_given_to_cancel_on_eof() {
        let token = CancellationToken::new();
        let mut filter =
            PreLifecycleFilter::new(scripted(&[initialize(1)])).cancel_on_eof(token.clone());
        assert!(filter.receive().await.is_some());
        assert!(!token.is_cancelled(), "pas d'annulation tant que l'entrée est ouverte");
        assert!(filter.receive().await.is_none());
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn eof_without_a_token_simply_ends_the_stream() {
        let mut filter = PreLifecycleFilter::new(scripted(&[]));
        assert!(filter.receive().await.is_none());
    }

    #[tokio::test]
    async fn send_and_close_are_delegated_to_the_inner_transport() {
        let inner = scripted(&[]);
        let (sent, closed) = (Arc::clone(&inner.sent), Arc::clone(&inner.closed));
        let mut filter = PreLifecycleFilter::new(inner);
        filter
            .send(TxJsonRpcMessage::<RoleServer>::notification(
                rmcp::model::ServerNotification::ToolListChangedNotification(
                    NotificationNoParam::default(),
                ),
            ))
            .await
            .unwrap();
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert!(!*closed.lock().unwrap());
        filter.close().await.unwrap();
        assert!(*closed.lock().unwrap());
    }

    // ---- la fin du service ----------------------------------------------------------------

    #[tokio::test]
    async fn only_an_interrupted_task_is_a_failure_after_the_lifecycle_opened() {
        assert!(quit_end(Ok(QuitReason::Closed)).is_ok());
        assert!(quit_end(Ok(QuitReason::Cancelled)).is_ok());
        let interrupted = || async {
            let handle = tokio::spawn(std::future::pending::<()>());
            handle.abort();
            handle.await.unwrap_err()
        };
        for ended in [
            quit_end(Err(interrupted().await)),
            quit_end(Ok(QuitReason::JoinError(interrupted().await))),
        ] {
            assert!(ended.unwrap_err().to_string().starts_with("MCP service task failed: "));
        }
    }

    #[test]
    fn eof_and_shutdown_before_the_lifecycle_are_not_failures() {
        assert!(initialization_end(ServerInitializeError::ConnectionClosed("eof".into())).is_ok());
        assert!(initialization_end(ServerInitializeError::Cancelled).is_ok());
    }

    #[test]
    fn any_other_initialization_error_is_a_failure_with_its_message() {
        let error = initialization_end(ServerInitializeError::InitializeFailed(
            ErrorData::internal_error("nope", None),
        ))
        .unwrap_err();
        assert!(error.to_string().starts_with("MCP initialization failed: "), "{error}");
    }

    // ---- `serve_io` de bout en bout, sur un tube mémoire ------------------------------------

    /// Serveur minimal : un outil fantôme dont `tools/list` dure `delay`, pour un appel en vol.
    struct Bare {
        delay: Duration,
    }

    impl ServerHandler for Bare {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
                .with_server_info(Implementation::new("lifecycle-test", "0.0.0"))
        }

        fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
            std::borrow::Cow::Borrowed(SUPPORTED)
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            // Annulation coopérative, comme un vrai outil qui passe `ct.cancelled()` à `Audited::run` :
            // rmcp n'interrompt pas de force un gestionnaire en vol.
            tokio::select! {
                () = tokio::time::sleep(self.delay) => Ok(ListToolsResult::default()),
                () = context.ct.cancelled() => Err(ErrorData::internal_error("cancelled", None)),
            }
        }
    }

    /// Client brut : écrit des lignes JSON, lit les lignes du serveur.
    struct Wire {
        write: WriteHalf<DuplexStream>,
        lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
        server: JoinHandle<Result<ServeOutcome, ServeError>>,
        shutdown: CancellationToken,
    }

    impl Wire {
        fn start(delay: Duration) -> Self {
            let (client, server) = tokio::io::duplex(1 << 20);
            let (server_read, server_write) = tokio::io::split(server);
            let shutdown = CancellationToken::new();
            let server =
                tokio::spawn(serve_io(Bare { delay }, server_read, server_write, shutdown.clone()));
            let (read, write) = tokio::io::split(client);
            Self { write, lines: BufReader::new(read).lines(), server, shutdown }
        }

        async fn send(&mut self, message: &Value) {
            let mut line = serde_json::to_vec(message).unwrap();
            line.push(b'\n');
            self.write.write_all(&line).await.unwrap();
            self.write.flush().await.unwrap();
        }

        /// Prochaine ligne JSON du serveur (au plus [`WAIT`] d'attente).
        async fn recv(&mut self) -> Value {
            let line = tokio::time::timeout(WAIT, self.lines.next_line())
                .await
                .expect("no line from the server")
                .unwrap()
                .expect("the server closed its output");
            serde_json::from_str(&line).unwrap()
        }

        /// Poignée de main legacy 2025-11-25, puis `notifications/initialized`.
        async fn handshake(&mut self) {
            self.send(&initialize(1)).await;
            let response = self.recv().await;
            assert_eq!(response["id"], json!(1), "{response}");
            assert_eq!(response["result"]["protocolVersion"], json!("2025-11-25"), "{response}");
            self.send(&initialized()).await;
        }

        /// Ferme stdin (EOF) et attend la fin du service ; rend son issue et la durée écoulée.
        async fn close(mut self) -> (Result<ServeOutcome, ServeError>, Duration) {
            // `shutdown` et non `drop` : la moitié de lecture garde le tube ouvert.
            self.write.shutdown().await.unwrap();
            let started = Instant::now();
            let outcome = tokio::time::timeout(WAIT, self.server)
                .await
                .expect("the service did not stop after EOF")
                .unwrap();
            (outcome, started.elapsed())
        }
    }

    #[tokio::test]
    async fn messages_before_initialize_are_ignored_then_the_handshake_succeeds() {
        let mut wire = Wire::start(Duration::ZERO);
        for premature in [initialized(), cancelled(), client_response(), client_error()] {
            wire.send(&premature).await;
        }
        // Un `ping` avant toute ère reste servi par rmcp lui-même (`{}`), sans autre ligne avant.
        wire.send(&ping(5)).await;
        let pong = wire.recv().await;
        assert_eq!(pong["id"], json!(5), "{pong}");
        assert_eq!(pong["result"], json!({}), "{pong}");
        wire.handshake().await;
        wire.send(&tools_list(2, None)).await;
        assert_eq!(wire.recv().await["result"]["tools"], json!([]));
    }

    #[tokio::test]
    async fn eof_before_any_lifecycle_ends_with_the_eof_outcome_within_a_second() {
        let (outcome, elapsed) = Wire::start(Duration::ZERO).close().await;
        assert_eq!(outcome.unwrap(), ServeOutcome::Eof);
        assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    }

    #[tokio::test]
    async fn eof_after_initialize_ends_with_the_eof_outcome_within_a_second() {
        let mut wire = Wire::start(Duration::ZERO);
        wire.handshake().await;
        let (outcome, elapsed) = wire.close().await;
        assert_eq!(outcome.unwrap(), ServeOutcome::Eof);
        assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    }

    #[tokio::test]
    async fn eof_with_a_call_in_flight_does_not_wait_for_it() {
        // Sans `cancel_on_eof`, rmcp attendrait jusqu'à 5 s la réponse de l'appel en vol.
        let mut wire = Wire::start(Duration::from_secs(30));
        wire.handshake().await;
        wire.send(&tools_list(2, None)).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let (outcome, elapsed) = wire.close().await;
        assert_eq!(outcome.unwrap(), ServeOutcome::Eof);
        assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    }

    #[tokio::test]
    async fn shutdown_after_initialize_ends_with_the_shutdown_outcome() {
        let mut wire = Wire::start(Duration::ZERO);
        wire.handshake().await;
        wire.shutdown.cancel();
        let outcome = tokio::time::timeout(WAIT, &mut wire.server).await.unwrap().unwrap();
        assert_eq!(outcome.unwrap(), ServeOutcome::Shutdown);
    }

    #[tokio::test]
    async fn shutdown_before_any_message_ends_with_the_shutdown_outcome() {
        let wire = Wire::start(Duration::ZERO);
        wire.shutdown.cancel();
        let outcome = tokio::time::timeout(WAIT, wire.server).await.unwrap().unwrap();
        assert_eq!(outcome.unwrap(), ServeOutcome::Shutdown);
    }

    #[tokio::test]
    async fn a_write_failure_during_initialization_is_a_failure_not_an_eof() {
        // Le client écrit un `ping` puis disparaît : la réponse du serveur ne peut plus être écrite.
        let (mut client, server) = tokio::io::duplex(1 << 16);
        client.write_all(format!("{}\n", ping(1)).as_bytes()).await.unwrap();
        drop(client);
        let (read, write) = tokio::io::split(server);
        let outcome =
            serve_io(Bare { delay: Duration::ZERO }, read, write, CancellationToken::new()).await;
        assert!(outcome.unwrap_err().to_string().starts_with("MCP initialization failed: "));
    }
}
