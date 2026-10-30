//! Transport Streamable HTTP sur une vraie socket (T-M8-03, annexe D §8.2, 06 §6.2) : complément
//! des tests en processus de `http.rs` (`tower::ServiceExt::oneshot`, qui portent la couverture).
//!
//! Le serveur tourne sur la boucle locale (`serve_http`, port choisi par le système), un client
//! HTTP/1.1 minimal écrit sur `std::net::TcpStream` lui parle : aucun `reqwest` en dépendance de
//! développement (ban cargo-deny, 00 §0, AM-36).
//!
//! - `http_simple_call_is_json` (C-50), `http_legacy_initialize_then_call` (C-52),
//!   `http_no_session_header` (C-41), `http_notification_202` (C-49) et `http_get_delete_405`
//!   (C-40) : le service derrière les couches, sur le fil ;
//! - `http_local_token_required_on_the_wire` (C-56) et `http_default_bind_loopback` (C-43) ;
//! - `http_descriptors_equal_stdio` (C-55, 05 §10.1) : `initialize` et `tools/list` servis en HTTP
//!   sont ceux du client en processus (stdio) ;
//! - `http_disconnect_cancels_upstream` (C-51, V-41) : fermer la connexion pendant un appel amont
//!   annule l'appel, qui laisse une ligne d'audit `cancelled by client` bien avant la fin de
//!   l'amont lent (30 s) ;
//! - `http_shutdown_stops_the_server` : l'annulation du jeton arrête l'écoute.
//!
//! Le harnais en processus n'existe qu'avec les features `http` et `test-support`
//! (`--all-features`). Tout le fichier est sous `cfg(test)` : les aides partagent les règles de
//! lint des tests (`unwrap` et `expect` permis, `clippy.toml`).

#![expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]

#[cfg(test)]
#[cfg(all(feature = "http", feature = "test-support"))]
mod http {
    use std::{
        fmt::Write as _,
        io::{Read as _, Write as _},
        net::{SocketAddr, TcpStream},
        time::Duration,
    };

    use lf_adapter_mcp::{
        HttpOptions, HttpToken, ServeError, ServeOutcome, serve_http, testing::Fixture,
    };
    use lf_application::{ports::AuditStatus, testing::PortRequest};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use tokio::task::JoinHandle;
    use tokio_util::sync::CancellationToken;

    /// Jeton du profil local, dans les tests qui l'exigent.
    const TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";

    /// Attente maximale d'un événement asynchrone.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// Réponse HTTP lue jusqu'à la fermeture de la connexion.
    #[derive(Debug)]
    struct Reply {
        status: u16,
        /// En-têtes, noms en minuscules, dans l'ordre reçu.
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Reply {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
        }

        fn json(&self) -> Value {
            serde_json::from_slice(&self.body).expect("a JSON body")
        }
    }

    /// Une requête HTTP/1.1 brute vers `/mcp`.
    fn raw_request(addr: SocketAddr, method: &str, headers: &[(&str, &str)], body: &str) -> String {
        let mut request =
            format!("{method} /mcp HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
        for (name, value) in headers {
            write!(request, "{name}: {value}\r\n").unwrap();
        }
        if method == "POST" {
            write!(request, "Content-Length: {}\r\n", body.len()).unwrap();
        }
        request.push_str("\r\n");
        request.push_str(body);
        request
    }

    /// Lit la réponse jusqu'à la fermeture et la découpe (corps à longueur fixe).
    fn parse_reply(raw: &[u8]) -> Reply {
        let split =
            raw.windows(4).position(|window| window == b"\r\n\r\n").expect("a header block");
        let head = std::str::from_utf8(&raw[..split]).expect("UTF-8 headers");
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse().ok())
            .expect("a status line");
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        Reply { status, headers, body: raw[split + 4..].to_vec() }
    }

    /// Envoie une requête et lit la réponse (bloquant : à lancer par `spawn_blocking`).
    fn exchange(addr: SocketAddr, method: &str, headers: &[(&str, &str)], body: &str) -> Reply {
        let mut stream = TcpStream::connect(addr).expect("a connection");
        stream.set_read_timeout(Some(Duration::from_secs(10))).expect("a read timeout");
        stream.write_all(raw_request(addr, method, headers, body).as_bytes()).expect("a write");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("a response");
        parse_reply(&raw)
    }

    /// En-têtes d'un POST JSON d'un client legacy (2025-11-25), plus `extra`.
    fn json_headers<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut headers = vec![
            ("Content-Type", "application/json"),
            ("Accept", "application/json, text/event-stream"),
            ("MCP-Protocol-Version", "2025-11-25"),
        ];
        headers.extend_from_slice(extra);
        headers
    }

    /// `exchange` hors de la boucle du serveur, qui tourne sur le même runtime.
    async fn call(
        addr: SocketAddr,
        method: &'static str,
        headers: Vec<(String, String)>,
        body: String,
    ) -> Reply {
        tokio::task::spawn_blocking(move || {
            let pairs: Vec<(&str, &str)> =
                headers.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
            exchange(addr, method, &pairs, &body)
        })
        .await
        .expect("the client thread")
    }

    async fn post(addr: SocketAddr, extra: &[(&str, &str)], body: &Value) -> Reply {
        let headers = json_headers(extra)
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        call(addr, "POST", headers, body.to_string()).await
    }

    /// Un serveur HTTP en marche sur la boucle locale.
    struct Running {
        addr: SocketAddr,
        shutdown: CancellationToken,
        task: JoinHandle<Result<ServeOutcome, ServeError>>,
    }

    impl Running {
        async fn stop(self) -> ServeOutcome {
            self.shutdown.cancel();
            tokio::time::timeout(PATIENCE, self.task)
                .await
                .expect("the server stops")
                .expect("no panic")
                .expect("a clean stop")
        }
    }

    /// Démarre `fixture` en HTTP, avec ou sans jeton, et attend qu'il accepte les connexions.
    async fn start(fixture: &Fixture, token: Option<&str>) -> Running {
        // Un port libre choisi par le système, relâché juste avant que le serveur ne le prenne.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("a free port")
            .port();
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut options = HttpOptions::local(addr).expect("a loopback address");
        if let Some(token) = token {
            options = options.with_token(HttpToken::new(token).expect("a valid token"));
        }
        let proto = fixture.server();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(serve_http(move || proto.clone(), options, shutdown.clone()));
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while TcpStream::connect(addr).is_err() {
            assert!(tokio::time::Instant::now() < deadline, "the server never listened on {addr}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Running { addr, shutdown, task }
    }

    fn initialize() -> Value {
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "http-test", "version": "0" }
            }
        })
    }

    fn call_tool(id: u64, name: &str, arguments: &Value) -> Value {
        json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        })
    }

    // ---- le service derrière les couches, sur le fil ----------------------------------------

    #[tokio::test]
    async fn http_default_bind_loopback() {
        let running = start(&Fixture::default(), None).await;
        assert!(running.addr.ip().is_loopback());
        running.stop().await;
        // Le profil local ne se règle pas sur une autre interface : refusé à la construction.
        for bad in ["0.0.0.0:8765", "192.168.1.10:8765", "[::]:8765"] {
            assert!(HttpOptions::local(bad.parse().expect("an address")).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn http_simple_call_is_json() {
        let fixture = Fixture::default();
        let running = start(&fixture, None).await;
        let reply = post(running.addr, &[], &call_tool(5, "fr_coverage", &json!({}))).await;
        assert_eq!(reply.status, 200, "{reply:?}");
        assert!(
            reply.header("content-type").is_some_and(|value| value.starts_with("application/json")),
            "{reply:?}"
        );
        let body = reply.json();
        assert_eq!(body["id"], 5);
        assert_eq!(body["result"]["isError"], json!(false), "{body}");
        // L'appel a laissé exactement une ligne d'audit, comme sur stdio.
        assert_eq!(fixture.audit.records().len(), 1);
        running.stop().await;
    }

    #[tokio::test]
    async fn http_legacy_initialize_then_call() {
        let running = start(&Fixture::default(), None).await;
        let reply = post(running.addr, &[], &initialize()).await;
        assert_eq!(reply.status, 200, "{reply:?}");
        let init = reply.json();
        assert_eq!(init["result"]["serverInfo"]["name"], "mcp-legifrance");
        // Sans session : l'appel suivant arrive sur une autre connexion, avec le seul en-tête de version.
        let reply = post(running.addr, &[], &call_tool(2, "fr_coverage", &json!({}))).await;
        assert_eq!(reply.status, 200, "{reply:?}");
        assert_eq!(reply.json()["id"], 2);
        running.stop().await;
    }

    #[tokio::test]
    async fn http_no_session_header() {
        let running = start(&Fixture::default(), None).await;
        // `Mcp-Session-Id` et `Last-Event-ID` reçus sont ignorés, aucun n'est émis.
        let extra = [("Mcp-Session-Id", "unknown-session"), ("Last-Event-ID", "42")];
        for body in [initialize(), call_tool(3, "fr_coverage", &json!({}))] {
            let reply = post(running.addr, &extra, &body).await;
            assert_eq!(reply.status, 200, "{reply:?}");
            assert_eq!(reply.header("mcp-session-id"), None, "{reply:?}");
        }
        running.stop().await;
    }

    #[tokio::test]
    async fn http_notification_202() {
        let running = start(&Fixture::default(), None).await;
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let reply = post(running.addr, &[], &notification).await;
        assert_eq!(reply.status, 202, "{reply:?}");
        assert_eq!(reply.body, Vec::<u8>::new());
        running.stop().await;
    }

    #[tokio::test]
    async fn http_get_delete_405() {
        let running = start(&Fixture::default(), None).await;
        for method in ["GET", "DELETE"] {
            let headers = vec![("Accept".to_owned(), "text/event-stream".to_owned())];
            let reply = call(running.addr, method, headers, String::new()).await;
            assert_eq!(reply.status, 405, "{method}: {reply:?}");
            assert_eq!(reply.header("allow"), Some("POST"), "{method}");
        }
        running.stop().await;
    }

    #[tokio::test]
    async fn http_local_token_required_on_the_wire() {
        let running = start(&Fixture::default(), Some(TOKEN)).await;
        let reply = post(running.addr, &[], &initialize()).await;
        assert_eq!(reply.status, 401, "{reply:?}");
        assert_eq!(reply.header("www-authenticate"), Some("Bearer realm=\"mcp-legifrance\""));
        assert_eq!(reply.body, Vec::<u8>::new());
        let reply = post(running.addr, &[("Authorization", "Bearer wrong")], &initialize()).await;
        assert_eq!(reply.status, 401, "{reply:?}");
        let bearer = format!("Bearer {TOKEN}");
        let reply = post(running.addr, &[("Authorization", &bearer)], &initialize()).await;
        assert_eq!(reply.status, 200, "{reply:?}");
        running.stop().await;
    }

    // ---- C-55 : mêmes descripteurs qu'en stdio ----------------------------------------------

    #[tokio::test]
    async fn http_descriptors_equal_stdio() {
        let fixture = Fixture::default();
        let running = start(&fixture, None).await;
        let client = fixture.connect().await;

        // Les outils : ceux du client en processus (le chemin stdio), octet pour octet en JSON.
        let stdio_tools = serde_json::to_value(client.list_all_tools().await.expect("tools/list"))
            .expect("serializable tools");
        let list = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" });
        let http = post(running.addr, &[], &list).await.json();
        assert_eq!(http["result"]["tools"], stdio_tools);
        assert!(stdio_tools.as_array().is_some_and(|tools| !tools.is_empty()));

        // L'identité, les capacités et les instructions du serveur.
        let stdio_info = serde_json::to_value(client.peer_info().expect("server info"))
            .expect("serializable server info");
        let init = post(running.addr, &[], &initialize()).await.json();
        for field in ["serverInfo", "capabilities", "instructions"] {
            assert_eq!(init["result"][field], stdio_info[field], "{field}");
        }
        running.stop().await;
    }

    // ---- C-51, V-41 : la connexion fermée annule l'appel -------------------------------------

    #[tokio::test]
    async fn http_disconnect_cancels_upstream() {
        // L'amont dure 30 s : sans annulation, la ligne d'audit n'arriverait pas avant.
        let fixture = Fixture::slow_upstream();
        let running = start(&fixture, None).await;
        let body = call_tool(8, "fr_search", &json!({ "query": "bail" })).to_string();
        let mut stream = TcpStream::connect(running.addr).expect("a connection");
        let headers = json_headers(&[]);
        stream
            .write_all(raw_request(running.addr, "POST", &headers, &body).as_bytes())
            .expect("a write");

        // L'appel est parti vers l'amont…
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while !fixture
            .corpus
            .seen_requests()
            .iter()
            .any(|request| matches!(request, PortRequest::Search(_)))
        {
            assert!(tokio::time::Instant::now() < deadline, "the call never reached the corpus");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(fixture.audit.records().is_empty(), "the call is still in flight");

        // … le client coupe, le serveur annule et journalise l'annulation.
        drop(stream);
        let deadline = tokio::time::Instant::now() + PATIENCE;
        let record = loop {
            if let Some(record) = fixture.audit.records().into_iter().next() {
                break record;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "closing the connection did not cancel the call"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(record.status, AuditStatus::Error);
        assert_eq!(record.error_code, None);
        assert_eq!(record.error.as_deref(), Some("cancelled by client"));
        assert!(record.duration_ms < 5_000, "{record:?}");
        running.stop().await;
    }

    // ---- arrêt -------------------------------------------------------------------------------

    #[tokio::test]
    async fn http_shutdown_stops_the_server() {
        let running = start(&Fixture::default(), None).await;
        let addr = running.addr;
        assert_eq!(running.stop().await, ServeOutcome::Shutdown);
        // Plus personne n'écoute.
        assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
    }
}
