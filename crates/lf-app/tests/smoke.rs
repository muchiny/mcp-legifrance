//! Fumée du binaire `mcp-legifrance` sur stdio (T-M4-07, 06 §2.7) : le processus réel, lancé avec
//! `env_clear()`, répond à `initialize` et à `server/discover`, ne met que du JSON-RPC sur stdout,
//! n'ouvre aucune connexion vers PISTE pour y répondre, et démarre en mode dégradé sans
//! identifiants. Les transcriptions, l'arrêt par signal, l'audit et les fuites sont l'affaire de
//! `e2e_stdio.rs`, `cli.rs` et `leaks.rs` (V3-01).
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#[cfg(test)]
mod smoke {
    use std::{
        io::{BufRead as _, BufReader, ErrorKind, Write as _},
        net::TcpListener,
        process::{Child, ChildStdin, Command, ExitStatus, Stdio},
        sync::mpsc,
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    use serde_json::{Value, json};

    /// Binaire sous test : construit par cargo pour cette cible de test.
    const BINARY: &str = env!("CARGO_BIN_EXE_mcp-legifrance");

    /// Attente maximale d'une ligne de stdout ou de la fin du processus (CI lente).
    const WAIT: Duration = Duration::from_secs(30);

    /// Identifiants factices : le réseau n'est jamais joint, ils ne servent qu'à la configuration.
    const FAKE_CLIENT_ID: &str = "11112222-3333-4444-5555-666677778888";
    const FAKE_CLIENT_SECRET: &str = "99990000-aaaa-bbbb-cccc-ddddeeeeffff";

    /// Hôte PISTE de substitution : un port local où personne ne doit se connecter.
    struct Piste {
        listener: TcpListener,
    }

    impl Piste {
        fn bind() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
            listener.set_nonblocking(true).expect("a non-blocking listener");
            Self { listener }
        }

        fn url(&self, path: &str) -> String {
            let port = self.listener.local_addr().expect("a bound listener").port();
            format!("http://127.0.0.1:{port}{path}")
        }

        /// Vrai si une connexion attend d'être acceptée, donc si le binaire a joint l'hôte.
        fn was_contacted(&self) -> bool {
            match self.listener.accept() {
                Ok(_) => true,
                Err(error) => {
                    assert_eq!(error.kind(), ErrorKind::WouldBlock, "{error}");
                    false
                }
            }
        }
    }

    /// Réglage d'un lancement : environnement vidé, puis ces variables.
    struct Launch<'a> {
        piste: &'a Piste,
        audit_dir: &'a std::path::Path,
        credentials: bool,
        extra: Vec<(&'static str, &'static str)>,
    }

    impl<'a> Launch<'a> {
        fn new(piste: &'a Piste, audit_dir: &'a std::path::Path) -> Self {
            Self { piste, audit_dir, credentials: true, extra: Vec::new() }
        }

        fn without_credentials(mut self) -> Self {
            self.credentials = false;
            self
        }

        fn with(mut self, name: &'static str, value: &'static str) -> Self {
            self.extra.push((name, value));
            self
        }

        /// `mcp-legifrance serve` avec stdin, stdout et stderr en tubes.
        fn start(self) -> Session {
            let mut command = Command::new(BINARY);
            command.arg("serve").env_clear();
            forward_platform_environment(&mut command);
            command
                .env("LF_AUDIT_DIR", self.audit_dir)
                .env("LF_PISTE_OAUTH_URL", self.piste.url("/oauth/token"))
                .env("LF_PISTE_BASE_URL", self.piste.url("/api"))
                // Un proxy en échec : même un client qui l'honorerait ne sortirait pas.
                .env("HTTPS_PROXY", "http://127.0.0.1:9")
                .env("HTTP_PROXY", "http://127.0.0.1:9");
            if self.credentials {
                command
                    .env("LF_PISTE_CLIENT_ID", FAKE_CLIENT_ID)
                    .env("LF_PISTE_CLIENT_SECRET", FAKE_CLIENT_SECRET);
            }
            for (name, value) in self.extra {
                command.env(name, value);
            }
            Session::spawn(command)
        }
    }

    /// Windows a besoin de `SYSTEMROOT` pour initialiser Winsock, même sans réseau.
    #[cfg(windows)]
    #[expect(
        clippy::disallowed_methods,
        reason = "test : seule variable de la plateforme transmise à un processus à environnement vidé"
    )]
    fn forward_platform_environment(command: &mut Command) {
        if let Some(root) = std::env::var_os("SYSTEMROOT") {
            command.env("SYSTEMROOT", root);
        }
    }

    #[cfg(not(windows))]
    fn forward_platform_environment(_command: &mut Command) {}

    /// Le processus, ses tubes et les fils qui drainent stdout et stderr.
    struct Session {
        child: Child,
        stdin: Option<ChildStdin>,
        lines: mpsc::Receiver<String>,
        stdout: JoinHandle<Vec<String>>,
        stderr: JoinHandle<String>,
    }

    /// Ce qu'une session laisse une fois le processus terminé.
    struct Finished {
        status: ExitStatus,
        /// Durée entre la fermeture de stdin et la fin du processus.
        elapsed: Duration,
        /// Toutes les lignes de stdout, dans l'ordre.
        stdout: Vec<String>,
        stderr: String,
    }

    impl Session {
        fn spawn(mut command: Command) -> Self {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("the binary starts");
            let stdin = child.stdin.take();
            let out = child.stdout.take().expect("a piped stdout");
            let err = child.stderr.take().expect("a piped stderr");
            let (sender, lines) = mpsc::channel();
            let stdout = thread::spawn(move || {
                let mut all = Vec::new();
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    all.push(line.clone());
                    // Le récepteur disparaît quand le test a fini de lire : on continue de drainer.
                    drop(sender.send(line));
                }
                all
            });
            let stderr = thread::spawn(move || {
                let mut text = String::new();
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    text.push_str(&line);
                    text.push('\n');
                }
                text
            });
            Self { child, stdin, lines, stdout, stderr }
        }

        fn send(&mut self, message: &Value) {
            let stdin = self.stdin.as_mut().expect("stdin is still open");
            let mut line = serde_json::to_vec(message).expect("a serializable message");
            line.push(b'\n');
            stdin.write_all(&line).expect("write to the server");
            stdin.flush().expect("flush to the server");
        }

        /// Prochaine ligne de stdout, lue comme du JSON.
        fn recv(&self) -> Value {
            let line = self.lines.recv_timeout(WAIT).expect("a line from the server");
            serde_json::from_str(&line).expect("stdout carries JSON")
        }

        /// Envoie la requête `request` et rend la réponse suivante, qui doit porter son `id`.
        fn request(&mut self, request: &Value) -> Value {
            self.send(request);
            let response = self.recv();
            assert_eq!(response["id"], request["id"], "{response}");
            response
        }

        /// Ferme stdin (EOF), attend la fin du processus et rend ce qu'il a écrit.
        fn finish(self) -> Finished {
            self.stop(true)
        }

        /// Envoie SIGTERM au processus, stdin restant ouvert : seul le signal peut l'arrêter.
        #[cfg(unix)]
        fn terminate(self) -> Finished {
            let status = Command::new("kill")
                .args(["-TERM", &self.child.id().to_string()])
                .status()
                .expect("kill runs");
            assert!(status.success(), "kill -TERM failed: {status}");
            self.stop(false)
        }

        /// Attend la fin du processus, après avoir fermé stdin si `close_stdin`.
        fn stop(mut self, close_stdin: bool) -> Finished {
            if close_stdin {
                drop(self.stdin.take());
            }
            let started = Instant::now();
            let status = loop {
                if let Some(status) = self.child.try_wait().expect("the child can be polled") {
                    break status;
                }
                if started.elapsed() > WAIT {
                    self.child.kill().expect("the stuck child is killed");
                    break self.child.wait().expect("the killed child is reaped");
                }
                thread::sleep(Duration::from_millis(10));
            };
            let elapsed = started.elapsed();
            assert!(elapsed <= WAIT, "the server did not stop within {WAIT:?}");
            Finished {
                status,
                elapsed,
                stdout: self.stdout.join().expect("the stdout reader ends"),
                stderr: self.stderr.join().expect("the stderr reader ends"),
            }
        }
    }

    // --- Messages JSON-RPC ----------------------------------------------------------------------

    fn initialize(id: u64) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "smoke", "version": "0.0.0"}
        }})
    }

    fn initialized() -> Value {
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    }

    /// `server/discover` de l'ère moderne : sans `initialize`, avec le `_meta` que l'ère exige.
    fn discover(id: u64) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "server/discover", "params": {"_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "smoke", "version": "0.0.0"},
            "io.modelcontextprotocol/clientCapabilities": {}
        }}})
    }

    fn call(id: u64, tool: &str, arguments: &Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
               "params": {"name": tool, "arguments": arguments}})
    }

    fn method(id: u64, name: &str) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": name})
    }

    /// Texte du premier bloc de contenu d'un résultat d'outil.
    fn first_text(response: &Value) -> &str {
        response["result"]["content"][0]["text"].as_str().expect("a text content block")
    }

    fn assert_clean_exit(finished: &Finished) {
        assert_eq!(finished.status.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(finished.elapsed < Duration::from_secs(10), "{:?}", finished.elapsed);
    }

    // --- Tests ----------------------------------------------------------------------------------

    /// Le binaire réel répond à `initialize` (ère legacy) avec son identité, puis sort avec le
    /// code 0 à l'EOF de stdin.
    #[test]
    fn binary_answers_initialize() {
        let piste = Piste::bind();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let mut session = Launch::new(&piste, audit.path()).start();

        let response = session.request(&initialize(1));
        let result = &response["result"];
        assert_eq!(result["protocolVersion"], "2025-11-25", "{response}");
        assert_eq!(result["serverInfo"]["name"], "mcp-legifrance", "{response}");
        assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"), "{response}");
        assert!(result["capabilities"]["tools"].is_object(), "{response}");
        session.send(&initialized());

        let finished = session.finish();
        assert_clean_exit(&finished);
        assert_eq!(finished.stdout.len(), 1, "{:?}", finished.stdout);
    }

    /// Chaque ligne de stdout est un message JSON-RPC 2.0, succès, erreur métier et erreurs de
    /// protocole compris, aussi avec `LF_LOG=trace` : les journaux vont sur stderr seulement.
    #[test]
    fn stdout_contains_only_jsonrpc() {
        for log in [None, Some("trace")] {
            let piste = Piste::bind();
            let audit = tempfile::tempdir().expect("a temporary directory");
            let mut launch = Launch::new(&piste, audit.path());
            if let Some(filter) = log {
                launch = launch.with("LF_LOG", filter);
            }
            let mut session = launch.start();

            let mut answered = 0;
            let initialized_ok = session.request(&initialize(1));
            assert!(initialized_ok["result"].is_object(), "{initialized_ok}");
            session.send(&initialized());
            answered += 1;
            for request in [
                method(2, "ping"),
                method(3, "tools/list"),
                call(4, "fr_coverage", &json!({})),
                // Erreur métier : un argument que l'outil refuse (`invalid_arg`, in-band).
                call(5, "fr_coverage", &json!({"unexpected": 1})),
                // Erreurs de protocole : outil inconnu, méthode inconnue.
                call(6, "no_such_tool", &json!({})),
                method(7, "no/such/method"),
            ] {
                let response = session.request(&request);
                answered += 1;
                assert!(
                    response.get("result").is_some() || response.get("error").is_some(),
                    "{response}"
                );
            }

            let finished = session.finish();
            assert_clean_exit(&finished);
            assert_eq!(finished.stdout.len(), answered, "{:?}", finished.stdout);
            for line in &finished.stdout {
                let message: Value = serde_json::from_str(line).expect("a JSON line on stdout");
                assert_eq!(message["jsonrpc"], "2.0", "{line}");
                let response = message.get("id").is_some()
                    && (message.get("result").is_some() != message.get("error").is_some());
                let notification_or_request = message.get("method").is_some();
                assert!(response || notification_or_request, "not a JSON-RPC message: {line}");
            }
            if log.is_some() {
                // Le niveau `trace` journalise bien, et ailleurs que sur stdout.
                assert!(!finished.stderr.is_empty(), "no log on stderr at LF_LOG=trace");
            }
        }
    }

    /// `server/discover` (ère moderne) et `initialize` (ère legacy) répondent sans identifiants,
    /// avec PISTE vers un port local et un proxy en échec, et sans qu'aucune connexion parte.
    #[test]
    fn e2e_discover_without_network() {
        let piste = Piste::bind();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let mut session = Launch::new(&piste, audit.path()).without_credentials().start();

        let discovered = session.request(&discover(1));
        let result = &discovered["result"];
        assert_eq!(result["resultType"], "complete", "{discovered}");
        assert_eq!(
            result["supportedVersions"],
            json!(["2026-07-28", "2025-11-25", "2025-06-18"]),
            "{discovered}"
        );
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "mcp-legifrance",
            "{discovered}"
        );

        let initialized_ok = session.request(&initialize(2));
        assert_eq!(initialized_ok["result"]["serverInfo"]["name"], "mcp-legifrance");
        session.send(&initialized());

        let finished = session.finish();
        assert_clean_exit(&finished);
        assert!(!piste.was_contacted(), "the server contacted PISTE to answer a handshake");
    }

    /// SIGTERM, stdin restant ouvert, arrête le serveur avec le code 0 : le jeton d'arrêt du
    /// processus est bien celui de `serve_stdio` (02 §8.8).
    #[cfg(unix)]
    #[test]
    fn sigterm_stops_the_server_with_code_0() {
        let piste = Piste::bind();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let mut session = Launch::new(&piste, audit.path()).start();
        let handshake = session.request(&initialize(1));
        assert_eq!(handshake["result"]["serverInfo"]["name"], "mcp-legifrance", "{handshake}");
        session.send(&initialized());

        let finished = session.terminate();
        assert_clean_exit(&finished);
        assert!(finished.stderr.contains("shutdown signal received"), "{}", finished.stderr);
    }

    /// Sans identifiants, le serveur démarre (code 0 à l'EOF, `error!` sur stderr), répond, rend
    /// `fr_coverage` en `degraded` et un `[config_error]` sur un outil PISTE, sans réseau.
    #[test]
    fn missing_credentials_starts_degraded() {
        let piste = Piste::bind();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let mut session = Launch::new(&piste, audit.path()).without_credentials().start();

        let handshake = session.request(&initialize(1));
        assert_eq!(handshake["result"]["serverInfo"]["name"], "mcp-legifrance", "{handshake}");
        session.send(&initialized());

        let coverage = session.request(&call(2, "fr_coverage", &json!({})));
        assert_ne!(coverage["result"]["isError"], json!(true), "{coverage}");
        assert_eq!(coverage["result"]["structuredContent"]["status"], "degraded", "{coverage}");

        let search = session.request(&call(3, "fr_search", &json!({"query": "bail commercial"})));
        assert_eq!(search["result"]["isError"], json!(true), "{search}");
        assert!(first_text(&search).starts_with("[config_error]"), "{search}");

        let finished = session.finish();
        assert_clean_exit(&finished);
        assert!(!piste.was_contacted(), "a degraded server must not reach PISTE");
        assert!(
            finished.stderr.contains("PISTE credentials missing"),
            "the missing credentials are not reported on stderr:\n{}",
            finished.stderr
        );
        assert!(!finished.stderr.contains("bail commercial"), "{}", finished.stderr);
    }
}
