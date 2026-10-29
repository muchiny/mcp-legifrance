//! Aides communes des tests de bout en bout du binaire `mcp-legifrance` (V3-01, 06 §2.7).
//!
//! - [`Launch`] compose un lancement : environnement vidé (`env_clear()`), puis `LF_AUDIT_DIR`
//!   temporaire, des URL PISTE vers un faux PISTE ([`FakePiste`], `wiremock`) ou vers un port où
//!   personne n'écoute, des identifiants factices ou absents, et un proxy en échec
//!   (`HTTPS_PROXY=http://127.0.0.1:9`).
//! - [`Session`] est le processus lancé : stdin, stdout lu ligne à ligne, stderr drainé à part.
//! - [`replay_on_child`] rejoue une [`Transcript`] de `lf-adapter-mcp` sur le binaire, avec les
//!   mêmes instructions et les mêmes contrôles communs que `replay_in_process` (C-03, C-26), plus
//!   le délai de sortie mesuré sur un vrai processus (`expect_exit`).
//!
//! Chaque cible de test inclut ce module et n'emploie qu'une partie de ses aides.
//!
//! Tout le module est sous `cfg(test)` (déclaré ainsi par les cibles) : les aides partagent les
//! règles de lint des tests (`unwrap` et `expect` permis, `clippy.toml`).

#![allow(dead_code, reason = "chaque cible de test n'emploie qu'une partie de ces aides")]

use std::{
    collections::{VecDeque, hash_map::RandomState},
    fs,
    hash::{BuildHasher as _, Hasher as _},
    io::ErrorKind,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::{Duration, Instant},
};

use lf_adapter_mcp::testing::{Need, Replay, ReplayError, Step, Transcript, protocol_violation};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    task::JoinHandle,
    time::timeout,
};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{any, method, path},
};

/// Binaire sous test : construit par cargo pour cette cible de test.
pub(crate) const BINARY: &str = env!("CARGO_BIN_EXE_mcp-legifrance");

/// Attente maximale de la fin d'un processus ou d'une ligne qui doit arriver (CI lente).
pub(crate) const WAIT: Duration = Duration::from_secs(30);

/// Attente d'un message du serveur dans une transcription (celle de `replay_in_process`).
pub(crate) const MESSAGE_WAIT: Duration = Duration::from_secs(5);

/// Pause entre le dernier message attendu et la fermeture de stdin d'une transcription rejouée.
///
/// Défaut connu du binaire (à lever, voir `eof_right_after_listen_ack_exits_within_1s`) : un EOF
/// qui arrive dans la milliseconde qui suit l'accusé d'un `subscriptions/listen` fait attendre
/// 5 s le drainage de rmcp (`timed out draining in-flight responses`) au lieu de clore le `listen`
/// aussitôt (mesuré : 3 lancements sur 8 sans pause, 0 sur 40 avec une pause de 1 ms ou plus). 50 ms rendent les
/// transcriptions déterministes sans masquer le défaut, que ce test dédié garde en vue.
pub(crate) const EOF_SETTLE: Duration = Duration::from_millis(50);

/// Durée d'un appel amont « lent » : bien plus que n'importe quel test.
pub(crate) const SLOW_UPSTREAM: Duration = Duration::from_secs(30);

/// Identifiants factices : le faux PISTE accepte n'importe quel couple.
pub(crate) const FAKE_CLIENT_ID: &str = "11112222-3333-4444-5555-666677778888";
pub(crate) const FAKE_CLIENT_SECRET: &str = "99990000-aaaa-bbbb-cccc-ddddeeeeffff";

/// Chemin du jeton OAuth du faux PISTE.
pub(crate) const TOKEN_PATH: &str = "/oauth/token";

/// Chemin de l'API Légifrance du faux PISTE (celui des hôtes officiels, 04 §2.3).
pub(crate) const API_BASE: &str = "/dila/legifrance/lf-engine-app";

/// Réponse de `/search` pour le fond LODA : la fixture de référence enregistrée par l'adapter.
const SEARCH_LODA: &[u8] =
    include_bytes!("../../../lf-adapter-piste/tests/fixtures/reference/search_loda.json");

/// Contenu d'un enregistrement de la référence, lu à la compilation.
macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("../../../lf-adapter-piste/tests/fixtures/reference/", $name))
    };
}

/// Une consultation du faux PISTE : chemin, filtre `(champ du corps, préfixe)` et réponse.
type Consult = (&'static str, Option<(&'static str, &'static str)>, &'static [u8]);

// --- Nombres au hasard ----------------------------------------------------------------------------

/// Une chaîne hexadécimale de 32 caractères, tirée au hasard (graine du système, sans dépendance).
pub(crate) fn random_hex() -> String {
    let word = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(0);
        hasher.finish()
    };
    format!("{:016x}{:016x}", word(), word())
}

// --- Faux PISTE -----------------------------------------------------------------------------------

/// Jeton OAuth que délivre le faux PISTE.
pub(crate) const FAKE_TOKEN: &str = "fake-token";

/// Monte la route du jeton OAuth : toujours un jeton valable une heure.
async fn mount_token(server: &MockServer) {
    let body =
        format!(r#"{{"access_token":"{FAKE_TOKEN}","token_type":"Bearer","expires_in":3600}}"#);
    Mock::given(method("POST"))
        .and(path(TOKEN_PATH))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(body.into_bytes(), "application/json"),
        )
        .mount(server)
        .await;
}

/// Monte `body` pour `endpoint` : à toute requête, ou seulement à celles dont le champ
/// `key` du corps JSON commence par `prefix` (`Some((key, prefix))`).
async fn mount_fixture(
    server: &MockServer,
    endpoint: &str,
    field: Option<(&'static str, &'static str)>,
    body: &'static [u8],
) {
    let mock = Mock::given(method("POST")).and(path(format!("{API_BASE}{endpoint}")));
    let mock = match field {
        None => mock,
        Some((key, prefix)) => mock.and(move |request: &Request| {
            serde_json::from_slice::<Value>(&request.body)
                .ok()
                .and_then(|value| {
                    value.get(key).and_then(Value::as_str).map(|text| text.starts_with(prefix))
                })
                .unwrap_or(false)
        }),
    };
    mock.respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(server)
        .await;
}

/// Faux PISTE : un `wiremock` sur la boucle locale, qui sert le jeton OAuth et, selon le cas,
/// `/search` ; il enregistre toutes les requêtes reçues.
pub(crate) struct FakePiste {
    server: MockServer,
}

impl FakePiste {
    /// Aucune route : toute requête reçue est un 404 et se retrouve dans [`FakePiste::requests`].
    pub(crate) async fn silent() -> Self {
        Self { server: MockServer::start().await }
    }

    /// Sert le jeton, puis `/search` avec la fixture LODA de la référence.
    pub(crate) async fn serving_search() -> Self {
        Self::serving_search_times(None).await
    }

    /// Comme [`FakePiste::serving_search`], et `/search` ne doit être reçu qu'une fois
    /// (`expect(1)`, contrôlé par [`FakePiste::verify`]).
    pub(crate) async fn serving_search_once() -> Self {
        Self::serving_search_times(Some(1)).await
    }

    async fn serving_search_times(expected: Option<u64>) -> Self {
        let server = MockServer::start().await;
        mount_token(&server).await;
        let mut search = Mock::given(method("POST"))
            .and(path(format!("{API_BASE}/search")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SEARCH_LODA, "application/json"));
        if let Some(times) = expected {
            search = search.expect(times);
        }
        search.mount(&server).await;
        Self { server }
    }

    /// Sert le jeton, puis les seize réponses enregistrées par la référence : `/search` par fond
    /// (le champ `fond` du corps) et chaque consultation, quel que soit l'identifiant demandé
    /// (`/consult/juri` selon le préfixe `JURITEXT`, `CONSTEXT` ou `CETATEXT`).
    pub(crate) async fn reference() -> Self {
        let server = MockServer::start().await;
        mount_token(&server).await;
        let searches: [(&str, &[u8]); 8] = [
            ("LODA_DATE", fixture!("search_loda.json")),
            ("CODE_DATE", fixture!("search_code.json")),
            ("JURI", fixture!("search_juri.json")),
            ("CONSTIT", fixture!("search_constit.json")),
            ("CETAT", fixture!("search_cetat.json")),
            ("CNIL", fixture!("search_cnil.json")),
            ("KALI", fixture!("search_kali.json")),
            ("ACCO", fixture!("search_acco.json")),
        ];
        for (fond, body) in searches {
            mount_fixture(&server, "/search", Some(("fond", fond)), body).await;
        }
        let consults: [Consult; 8] = [
            ("/consult/lawDecree", None, fixture!("consult_loda.json")),
            ("/consult/getArticle", None, fixture!("consult_article.json")),
            ("/consult/juri", Some(("textId", "JURITEXT")), fixture!("consult_juri.json")),
            ("/consult/juri", Some(("textId", "CONSTEXT")), fixture!("consult_juri_constit.json")),
            ("/consult/juri", Some(("textId", "CETATEXT")), fixture!("consult_juri_cetat.json")),
            ("/consult/cnil", None, fixture!("consult_cnil.json")),
            ("/consult/kaliText", None, fixture!("consult_kali.json")),
            ("/consult/acco", None, fixture!("consult_acco.json")),
        ];
        for (endpoint, prefix, body) in consults {
            mount_fixture(&server, endpoint, prefix, body).await;
        }
        Self { server }
    }

    /// Amont lent : chaque requête n'est répondue qu'après [`SLOW_UPSTREAM`].
    pub(crate) async fn slow() -> Self {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_delay(SLOW_UPSTREAM))
            .mount(&server)
            .await;
        Self { server }
    }

    /// Sert un statut et un corps fixes à toute requête (le jeton compris).
    pub(crate) async fn answering(status: u16, body: &str) -> Self {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_raw(body.as_bytes().to_vec(), "application/json"),
            )
            .mount(&server)
            .await;
        Self { server }
    }

    /// Jeton OAuth bon, puis ce statut et ce corps à tout appel de l'API.
    pub(crate) async fn token_then_api(status: u16, body: &str) -> Self {
        let server = MockServer::start().await;
        mount_token(&server).await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_raw(body.as_bytes().to_vec(), "application/json"),
            )
            .with_priority(10)
            .mount(&server)
            .await;
        Self { server }
    }

    /// `LF_PISTE_OAUTH_URL`.
    pub(crate) fn oauth_url(&self) -> String {
        format!("{}{TOKEN_PATH}", self.server.uri())
    }

    /// `LF_PISTE_BASE_URL`.
    pub(crate) fn base_url(&self) -> String {
        format!("{}{API_BASE}", self.server.uri())
    }

    /// Les requêtes reçues, dans l'ordre.
    pub(crate) async fn requests(&self) -> Vec<Request> {
        self.server.received_requests().await.unwrap_or_default()
    }

    /// Attend, au plus [`WAIT`], d'avoir reçu au moins `count` requêtes ; rend celles reçues.
    pub(crate) async fn wait_for_requests(&self, count: usize) -> Vec<Request> {
        let started = Instant::now();
        loop {
            let requests = self.requests().await;
            if requests.len() >= count || started.elapsed() > WAIT {
                return requests;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Nombre de requêtes reçues sur `route` (chemin complet).
    pub(crate) async fn count(&self, route: &str) -> usize {
        self.requests().await.iter().filter(|request| request.url.path() == route).count()
    }

    /// Vérifie les attentes (`expect`) des routes montées.
    pub(crate) async fn verify(&self) {
        self.server.verify().await;
    }
}

/// Un port local où personne ne doit se connecter : hôte PISTE ou proxy de substitution.
pub(crate) struct Trap {
    listener: TcpListener,
}

impl Trap {
    pub(crate) fn bind() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        listener.set_nonblocking(true).expect("a non-blocking listener");
        Self { listener }
    }

    /// `http://127.0.0.1:<port><route>`.
    pub(crate) fn url(&self, route: &str) -> String {
        let port = self.listener.local_addr().expect("a bound listener").port();
        format!("http://127.0.0.1:{port}{route}")
    }

    /// Vrai si une connexion attend d'être acceptée, donc si le binaire a joint ce port.
    pub(crate) fn was_contacted(&self) -> bool {
        match self.listener.accept() {
            Ok(_) => true,
            Err(error) => {
                assert_eq!(error.kind(), ErrorKind::WouldBlock, "{error}");
                false
            }
        }
    }
}

// --- Lancement ------------------------------------------------------------------------------------

/// Réglage d'un lancement : environnement vidé, puis ces variables, puis ces arguments.
#[derive(Clone)]
pub(crate) struct Launch {
    vars: Vec<(String, String)>,
    args: Vec<String>,
}

impl Launch {
    /// Lancement de base : journal d'audit dans `audit_dir`, proxy en échec, ni identifiants ni
    /// URL PISTE, `serve` par défaut.
    pub(crate) fn new(audit_dir: &Path) -> Self {
        let mut launch = Self { vars: Vec::new(), args: Vec::new() };
        launch
            .set("LF_AUDIT_DIR", audit_dir.to_str().expect("a UTF-8 temporary path"))
            .set("HTTPS_PROXY", "http://127.0.0.1:9")
            .set("HTTP_PROXY", "http://127.0.0.1:9");
        launch
    }

    fn set(&mut self, name: &str, value: &str) -> &mut Self {
        self.vars.retain(|(known, _)| known != name);
        self.vars.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Pose `name` (remplace une valeur déjà posée).
    pub(crate) fn env(mut self, name: &str, value: &str) -> Self {
        self.set(name, value);
        self
    }

    /// Les identifiants factices.
    pub(crate) fn credentials(self) -> Self {
        self.env("LF_PISTE_CLIENT_ID", FAKE_CLIENT_ID)
            .env("LF_PISTE_CLIENT_SECRET", FAKE_CLIENT_SECRET)
    }

    /// Identifiants choisis par l'appelant.
    pub(crate) fn credentials_of(self, id: &str, secret: &str) -> Self {
        self.env("LF_PISTE_CLIENT_ID", id).env("LF_PISTE_CLIENT_SECRET", secret)
    }

    /// URL OAuth et URL de base vers `piste`. La boucle locale échappe au proxy en échec (un client
    /// qui l'honore y enverrait aussi le faux PISTE) ; tout autre hôte reste derrière lui.
    pub(crate) fn piste(self, piste: &FakePiste) -> Self {
        self.env("LF_PISTE_OAUTH_URL", &piste.oauth_url())
            .env("LF_PISTE_BASE_URL", &piste.base_url())
            .env("NO_PROXY", "127.0.0.1,localhost")
    }

    /// URL OAuth et URL de base vers un port où personne n'écoute.
    pub(crate) fn trap(self, trap: &Trap) -> Self {
        self.env("LF_PISTE_OAUTH_URL", &trap.url(TOKEN_PATH))
            .env("LF_PISTE_BASE_URL", &trap.url("/api"))
    }

    /// Le proxy du processus : `trap` au lieu du port fermé de base.
    pub(crate) fn proxy(self, trap: &Trap) -> Self {
        let url = trap.url("");
        self.env("HTTPS_PROXY", &url).env("HTTP_PROXY", &url).env("ALL_PROXY", &url)
    }

    /// Retire le proxy en échec : le réseau est joint pour de bon (tests live seulement).
    pub(crate) fn without_proxy(mut self) -> Self {
        self.vars.retain(|(name, _)| !name.ends_with("_PROXY"));
        self
    }

    /// Arguments de la ligne de commande (`serve` quand il n'y en a pas).
    pub(crate) fn args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self
    }

    /// Valeur posée pour `name`, s'il y en a une.
    pub(crate) fn var(&self, name: &str) -> Option<&str> {
        self.vars.iter().find(|(known, _)| known == name).map(|(_, value)| value.as_str())
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command.args(&self.args).env_clear().kill_on_drop(true);
        forward_platform_environment(&mut command);
        for (name, value) in &self.vars {
            command.env(name, value);
        }
        command
    }

    /// Lance le processus avec stdin, stdout et stderr en tubes.
    pub(crate) fn spawn(&self) -> Session {
        Session::spawn(self.command())
    }

    /// Lance le processus sans stdin et attend sa fin (`check-config`, `--version`).
    pub(crate) async fn run_to_end(&self) -> Output {
        let mut command = self.command();
        command.stdin(Stdio::null());
        let output = timeout(WAIT, command.output())
            .await
            .expect("the command ends within the wait")
            .expect("the binary runs");
        Output {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
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

/// Ce qu'une commande qui se termine seule a laissé.
#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

// --- Processus ------------------------------------------------------------------------------------

/// Ce qu'une session laisse une fois le processus terminé.
#[derive(Debug)]
pub(crate) struct Finished {
    pub(crate) status: ExitStatus,
    /// Durée entre la fermeture de stdin (ou l'envoi du signal) et la fin du processus.
    pub(crate) elapsed: Duration,
    /// Toutes les lignes de stdout, dans l'ordre.
    pub(crate) stdout: Vec<String>,
    pub(crate) stderr: String,
}

impl Finished {
    /// Code de sortie.
    pub(crate) fn code(&self) -> Option<i32> {
        self.status.code()
    }
}

/// Ce que rend une lecture de stdout.
pub(crate) enum Read {
    /// Une ligne.
    Line(String),
    /// Rien n'est arrivé pendant la durée donnée.
    Silence,
    /// stdout est fermé.
    Eof,
}

/// Le processus, ses tubes et le fil qui draine stderr.
pub(crate) struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Lines<BufReader<ChildStdout>>,
    stderr: Option<JoinHandle<String>>,
    /// Toutes les lignes de stdout lues jusqu'ici.
    seen: Vec<String>,
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
        let stderr = tokio::spawn(async move {
            let mut text = String::new();
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                text.push_str(&line);
                text.push('\n');
            }
            text
        });
        Self {
            child,
            stdin,
            lines: BufReader::new(out).lines(),
            stderr: Some(stderr),
            seen: Vec::new(),
        }
    }

    /// Identifiant du processus.
    pub(crate) fn pid(&self) -> u32 {
        self.child.id().expect("the child is running")
    }

    /// Les lignes de stdout lues jusqu'ici.
    pub(crate) fn stdout_seen(&self) -> &[String] {
        &self.seen
    }

    /// Écrit une ligne brute. Un échec d'écriture (tube fermé) n'est pas une erreur du test : la
    /// lecture suivante dit ce qui s'est passé.
    pub(crate) async fn send_raw(&mut self, text: &str) {
        let Some(stdin) = self.stdin.as_mut() else { return };
        let line = format!("{text}\n");
        drop(stdin.write_all(line.as_bytes()).await);
        drop(stdin.flush().await);
    }

    /// Écrit le message sur une ligne.
    pub(crate) async fn send(&mut self, message: &Value) {
        self.send_raw(&message.to_string()).await;
    }

    /// Ferme stdin (EOF).
    pub(crate) async fn close_stdin(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            drop(stdin.shutdown().await);
        }
    }

    /// Prochaine ligne de stdout, ou le silence, ou l'EOF.
    pub(crate) async fn read_line(&mut self, wait: Duration) -> Read {
        match timeout(wait, self.lines.next_line()).await {
            Err(_) => Read::Silence,
            Ok(Ok(Some(line))) => {
                self.seen.push(line.clone());
                Read::Line(line)
            }
            Ok(Ok(None) | Err(_)) => Read::Eof,
        }
    }

    /// Prochaine ligne de stdout, lue comme du JSON.
    pub(crate) async fn recv(&mut self) -> Value {
        match self.read_line(WAIT).await {
            Read::Line(line) => serde_json::from_str(&line).expect("stdout carries JSON"),
            Read::Silence => fail("no line from the server within the wait"),
            Read::Eof => fail("the server closed stdout"),
        }
    }

    /// Envoie `request` et rend la réponse suivante, qui doit porter son `id`.
    pub(crate) async fn request(&mut self, request: &Value) -> Value {
        self.send(request).await;
        let response = self.recv().await;
        assert_eq!(response["id"], request["id"], "{response}");
        response
    }

    /// `initialize` (ère legacy 2025-11-25) puis `notifications/initialized`.
    pub(crate) async fn handshake(&mut self) -> Value {
        let response = self.request(&initialize(1)).await;
        assert_eq!(response["result"]["serverInfo"]["name"], "mcp-legifrance", "{response}");
        self.send(&initialized()).await;
        response
    }

    /// Attend la fin du processus ; le tue au-delà de `wait`. Rend son statut et l'instant de sa fin.
    pub(crate) async fn wait_exit(&mut self, wait: Duration) -> Option<(ExitStatus, Instant)> {
        if let Ok(status) = timeout(wait, self.child.wait()).await {
            return Some((status.expect("the child can be waited for"), Instant::now()));
        }
        drop(self.child.kill().await);
        None
    }

    /// Ferme stdin (EOF), attend la fin du processus et rend ce qu'il a écrit.
    pub(crate) async fn finish(self) -> Finished {
        self.stop(true).await
    }

    /// Envoie SIGTERM au processus, stdin restant ouvert : seul le signal peut l'arrêter.
    #[cfg(unix)]
    pub(crate) async fn terminate(self) -> Finished {
        let status = Command::new("kill")
            .args(["-TERM", &self.pid().to_string()])
            .status()
            .await
            .expect("kill runs");
        assert!(status.success(), "kill -TERM failed: {status}");
        self.stop(false).await
    }

    /// Attend la fin du processus, après avoir fermé stdin si `close_stdin`.
    async fn stop(mut self, close_stdin: bool) -> Finished {
        let started = Instant::now();
        if close_stdin {
            self.close_stdin().await;
        }
        let exited = self.wait_exit(WAIT).await;
        let (status, ended) = exited.expect("the server did not stop within the wait");
        // Ce qui reste dans le tube de stdout, une fois le processus terminé.
        while let Read::Line(_) = self.read_line(Duration::from_secs(2)).await {}
        let stderr = self.take_stderr().await;
        Finished { status, elapsed: ended - started, stdout: self.seen, stderr }
    }
}

/// Échoue le test avec `message`. Rend un `T` pour s'employer en position d'expression.
#[expect(clippy::panic, reason = "aide d'échec de test : le message est ce que le test dit")]
pub(crate) fn fail<T>(message: &str) -> T {
    std::panic::panic_any(message.to_owned())
}

// --- Messages JSON-RPC ----------------------------------------------------------------------------

/// `initialize` de l'ère legacy 2025-11-25.
pub(crate) fn initialize(id: u64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": {
        "protocolVersion": "2025-11-25",
        "capabilities": {},
        "clientInfo": {"name": "e2e", "version": "0.0.0"}
    }})
}

/// `notifications/initialized`.
pub(crate) fn initialized() -> Value {
    json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
}

/// `_meta` de l'ère moderne.
pub(crate) fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "e2e", "version": "0.0.0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// `server/discover` de l'ère moderne : sans `initialize`, avec le `_meta` que l'ère exige.
pub(crate) fn discover(id: u64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "server/discover",
           "params": {"_meta": modern_meta()}})
}

/// `tools/call` en ère moderne : chaque requête porte son `_meta`, sans `initialize`.
pub(crate) fn modern_call(id: u64, tool: &str, arguments: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": tool, "arguments": arguments, "_meta": modern_meta()}})
}

/// `tools/call` en ère legacy (après `initialize`).
pub(crate) fn call(id: u64, tool: &str, arguments: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": tool, "arguments": arguments}})
}

/// Une requête sans paramètres.
pub(crate) fn method_request(id: u64, name: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": name})
}

/// Texte du premier bloc de contenu d'un résultat d'outil.
pub(crate) fn first_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"].as_str().expect("a text content block")
}

/// Vrai si le résultat d'outil est une erreur (`isError: true`).
pub(crate) fn is_tool_error(response: &Value) -> bool {
    response["result"]["isError"] == json!(true)
}

// --- Audit ----------------------------------------------------------------------------------------

/// Toutes les lignes d'audit de `dir` (fichier courant et archives), lues comme du JSON ; une
/// ligne illisible fait échouer le test (aucune ligne coupée, Review Focus 4).
pub(crate) fn audit_lines(dir: &Path) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
            let jsonl = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"));
            jsonl && name.is_some_and(|name| name.starts_with("mcp-legifrance"))
        })
        .collect();
    files.sort();
    let mut lines = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).expect("the audit file is readable");
        assert!(text.is_empty() || text.ends_with('\n'), "a torn last line in {file:?}");
        for line in text.lines() {
            let value: Value = serde_json::from_str(line)
                .unwrap_or_else(|error| fail(&format!("torn audit line {line:?}: {error}")));
            lines.push(value);
        }
    }
    lines
}

// --- Transcriptions sur le binaire ----------------------------------------------------------------

/// Rejoue `transcript` sur le binaire (`replay_in_process` sur un vrai processus, 06 §6.1).
///
/// Le processus est lancé avec des identifiants factices et un faux PISTE : lent si la
/// transcription l'exige (`needs: slow_upstream`), muet sinon (il ne doit alors recevoir aucune
/// requête). Chaque message du serveur passe les contrôles communs ([`protocol_violation`]). À la
/// fin, le serveur n'a rien dit de plus que ce que la transcription attendait, s'arrête avec le
/// code 0 sur l'EOF, et n'a pas paniqué.
///
/// # Errors
///
/// [`ReplayError`] au premier écart, avec la ligne de l'instruction.
pub(crate) async fn replay_on_child(transcript: &Transcript) -> Result<Replay, ReplayError> {
    let audit = tempfile::tempdir().expect("a temporary directory");
    let slow = transcript.needs(Need::SlowUpstream);
    let piste = if slow { FakePiste::slow().await } else { FakePiste::silent().await };
    let session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
    let mut run = ChildRun {
        session,
        pending: VecDeque::new(),
        sent_ids: Vec::new(),
        probes: 0,
        closed_at: None,
        exited: false,
        replay: Replay::default(),
    };
    let fail = |line: usize, message: String| ReplayError {
        transcript: transcript.name().to_owned(),
        line,
        message,
    };
    for instruction in transcript.steps() {
        run.apply(&instruction.step).await.map_err(|message| fail(instruction.line, message))?;
    }
    let last = transcript.steps().last().map_or(0, |instruction| instruction.line);
    let stderr = run.finish().await.map_err(|message| fail(last, message))?;
    if !stderr.is_empty() && stderr.contains("panicked") {
        return Err(fail(last, format!("the server panicked:\n{stderr}")));
    }
    if !slow && !piste.requests().await.is_empty() {
        return Err(fail(last, "the server contacted PISTE during the transcript".to_owned()));
    }
    Ok(run.replay)
}

/// Une transcription en cours sur le binaire.
struct ChildRun {
    session: Session,
    /// Messages lus en cherchant un `id`, pas encore attendus.
    pending: VecDeque<Value>,
    /// `id` des requêtes envoyées.
    sent_ids: Vec<Value>,
    /// Nombre de sondes `expect_alive` envoyées.
    probes: u32,
    /// Instant de la fermeture de stdin : départ du délai de sortie.
    closed_at: Option<Instant>,
    /// Le processus a déjà été attendu.
    exited: bool,
    replay: Replay,
}

impl ChildRun {
    async fn send(&mut self, message: &Value) {
        if message.get("method").is_some() {
            self.sent_ids.extend(message.get("id").cloned());
        }
        self.session.send(message).await;
    }

    /// Prochaine ligne du serveur, ou le silence ou l'EOF. Une ligne qui n'est pas du JSON est
    /// une violation de protocole (stdout ne porte que du JSON-RPC, C-01).
    async fn read(&mut self, wait: Duration) -> Result<Read, String> {
        let line = match self.session.read_line(wait).await {
            Read::Line(line) => line,
            other => return Ok(other),
        };
        let message = serde_json::from_str(&line).unwrap_or(Value::String(line.clone()));
        if let Some(violation) = protocol_violation(&message, &self.sent_ids) {
            return Err(violation);
        }
        self.replay.received.push(message);
        Ok(Read::Line(line))
    }

    /// Un message du serveur, au plus [`MESSAGE_WAIT`] d'attente.
    async fn read_message(&mut self) -> Result<Value, String> {
        match self.read(MESSAGE_WAIT).await? {
            Read::Line(line) => Ok(serde_json::from_str(&line).map_err(|error| error.to_string())?),
            Read::Silence => Err("no message from the server within 5 s".to_owned()),
            Read::Eof => Err("the server closed its output".to_owned()),
        }
    }

    /// Le prochain message : un message passé en cherchant un `id`, sinon le suivant du serveur.
    async fn next_message(&mut self) -> Result<Value, String> {
        match self.pending.pop_front() {
            Some(message) => Ok(message),
            None => self.read_message().await,
        }
    }

    /// Le message qui porte `id` : parmi ceux déjà lus, sinon en lisant. Les autres restent en
    /// attente pour les instructions suivantes.
    async fn message_with_id(&mut self, id: &Value) -> Result<Value, String> {
        loop {
            let found = self.pending.iter().position(|message| message.get("id") == Some(id));
            if let Some(message) = found.and_then(|position| self.pending.remove(position)) {
                return Ok(message);
            }
            let message =
                self.read_message().await.map_err(|error| format!("{error} (id {id})"))?;
            self.pending.push_back(message);
        }
    }

    async fn close_stdin(&mut self) {
        if self.closed_at.is_none() {
            self.closed_at = Some(Instant::now());
        }
        self.session.close_stdin().await;
    }

    /// Attend la fin du processus ; rend son code de sortie et le délai écoulé depuis la
    /// fermeture de stdin (ou depuis maintenant, si elle n'a pas eu lieu).
    async fn exit(&mut self) -> Result<(i32, Duration), String> {
        let started = self.closed_at.unwrap_or_else(Instant::now);
        let Some((status, ended)) = self.session.wait_exit(WAIT).await else {
            return Err(format!("the service did not exit within {WAIT:?}"));
        };
        self.exited = true;
        Ok((status.code().unwrap_or(-1), ended - started))
    }

    async fn apply(&mut self, step: &Step) -> Result<(), String> {
        match step {
            Step::Comment(_) | Step::Needs(_) | Step::BinaryOnly(_) => Ok(()),
            Step::Send(message) => {
                self.send(message).await;
                Ok(())
            }
            Step::SendRaw(text) => {
                if let Ok(message) = serde_json::from_str::<Value>(text) {
                    self.sent_ids.extend(message.get("id").cloned());
                }
                self.session.send_raw(text).await;
                Ok(())
            }
            Step::Expect(expect) => {
                let message = match expect.id() {
                    Some(id) => self.message_with_id(id).await?,
                    None => self.next_message().await?,
                };
                expect.check(&message)?;
                self.replay.snapshots.extend(expect.capture(&message));
                Ok(())
            }
            Step::ExpectSilence(wait) => {
                if let Some(stray) = self.pending.front() {
                    return Err(format!("a message arrived before the silence: {stray}"));
                }
                match self.read(*wait).await? {
                    Read::Silence => Ok(()),
                    Read::Line(line) => Err(format!("a message broke the silence: {line}")),
                    Read::Eof => Err("the server closed its output during the silence".to_owned()),
                }
            }
            Step::ExpectAlive => {
                self.probes += 1;
                let id = json!(format!("alive-{}", self.probes));
                let probe = json!({"jsonrpc": "2.0", "id": id, "method": "server/discover",
                "params": {"_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {}
                }}});
                self.send(&probe).await;
                let reply = self.message_with_id(&id).await?;
                if reply.get("result").is_some() {
                    Ok(())
                } else {
                    Err(format!("server/discover failed, the service is not alive: {reply}"))
                }
            }
            Step::CloseStdin => {
                // Un client lit ce qu'il attendait avant de fermer : l'EOF n'arrive pas dans la
                // milliseconde qui suit l'accusé d'un `listen` (voir `EOF_SETTLE`).
                tokio::time::sleep(EOF_SETTLE).await;
                self.close_stdin().await;
                Ok(())
            }
            Step::ExpectExit { code, within } => {
                let (actual, elapsed) = self.exit().await?;
                if actual != *code {
                    return Err(format!("the service exited with code {actual}, expected {code}"));
                }
                if elapsed > *within {
                    return Err(format!(
                        "the service exited after {elapsed:?}, expected within {within:?}"
                    ));
                }
                Ok(())
            }
        }
    }

    /// Fin de la transcription : ferme l'entrée, attend la fin du service (code 0), puis exige que
    /// le serveur n'ait rien dit de plus que ce qui a été attendu. Rend stderr.
    async fn finish(&mut self) -> Result<String, String> {
        self.close_stdin().await;
        if !self.exited {
            let started = Instant::now();
            let Some((status, _)) = self.session.wait_exit(MESSAGE_WAIT).await else {
                return Err(format!("the service did not exit within {MESSAGE_WAIT:?} of the EOF"));
            };
            self.exited = true;
            if status.code() != Some(0) {
                return Err(format!(
                    "the service exited with {status} after {:?}",
                    started.elapsed()
                ));
            }
        }
        if let Some(stray) = self.pending.pop_front() {
            return Err(format!("the server said more than expected: {stray}"));
        }
        if let Read::Line(stray) = self.read(MESSAGE_WAIT).await? {
            return Err(format!("the server said more than expected: {stray}"));
        }
        Ok(self.session.take_stderr().await)
    }
}

impl Session {
    /// stderr, une fois le processus terminé.
    async fn take_stderr(&mut self) -> String {
        match self.stderr.take() {
            Some(handle) => handle.await.expect("the stderr reader ends"),
            None => String::new(),
        }
    }
}
