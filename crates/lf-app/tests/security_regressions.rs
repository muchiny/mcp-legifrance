//! Garanties de sécurité et d'audit du catalogue de non-régression (06 §3.1 ; plan de fin de
//! projet §6, F3) : D-04, D-05, D-07, D-16, D-18, D-30, D-38 et D-40. Chaque test porte le nom que
//! le catalogue lui donne et exerce le binaire `mcp-legifrance` réel (environnement vidé, faux
//! PISTE `wiremock`), sauf `record_returns_only_after_sync` qui exerce `JsonlAuditLog` en
//! processus et `production_sources_contain_only_allowlisted_urls` qui lit les sources.
//!
//! Les deux tests de D-30 qui lisent les seize fixtures (`no_view_has_eli_uri`,
//! `ported_no_fabricated_eli`) vivent dans `parity_tool_output.rs`, qui porte déjà le harnais des
//! seize appels. `security_regressions_cover_the_security_catalogue` est l'ensemble : il vérifie
//! que chaque garantie a son test et que la liste d'excuses de la parité est vide.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#[cfg(test)]
mod common;

#[cfg(test)]
mod security {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    use lf_adapter_audit::JsonlAuditLog;
    use lf_application::{
        input::CanonicalInput,
        ports::{AuditErrorKind, AuditLog, AuditRecord, AuditStatus, PisteEnvironment},
        tools::ToolName,
    };
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{any, method, path},
    };

    use crate::common::{
        API_BASE, FakePiste, Launch, Read, TOKEN_PATH, audit_lines, call, fail, is_tool_error,
        method_request, modern_call, random_hex,
    };

    /// Racine du dépôt (deux niveaux au-dessus de `crates/lf-app`).
    fn repository() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    // ---- D-18 : la sandbox par défaut, et dite ---------------------------------------------------

    /// L'entrée `LF_PISTE_ENV` du rapport JSON de `check-config`.
    async fn reported_env(launch: &Launch) -> Value {
        let report = launch.clone().args(&["check-config", "--format", "json"]).run_to_end().await;
        assert_eq!(report.code, Some(0), "stderr:\n{}", report.stderr);
        let json: Value = serde_json::from_str(&report.stdout).expect("a JSON report");
        json["variables"]
            .as_array()
            .expect("a variable list")
            .iter()
            .find(|variable| variable["name"] == "LF_PISTE_ENV")
            .cloned()
            .expect("LF_PISTE_ENV is reported")
    }

    /// Lance le serveur, lit `fr_coverage`, ferme : l'environnement que le serveur déclare au
    /// modèle (`structuredContent.environment`) et celui qu'il écrit dans l'audit.
    async fn declared_env(launch: &Launch, audit: &Path) -> (String, String) {
        let mut session = launch.spawn();
        session.handshake().await;
        let coverage = session.request(&call(2, "fr_coverage", &json!({}))).await;
        assert!(!is_tool_error(&coverage), "{coverage}");
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        let declared = coverage["result"]["structuredContent"]["environment"]
            .as_str()
            .expect("fr_coverage names the environment")
            .to_owned();
        let lines = audit_lines(audit);
        assert_eq!(lines.len(), 1, "{lines:?}");
        (declared, lines[0]["env"].as_str().expect("an env in the audit line").to_owned())
    }

    /// Régression : D-18. Sans `LF_PISTE_ENV`, l'environnement est `sandbox` partout où il est dit :
    /// rapport de `check-config` (source `default`), `fr_coverage` et ligne d'audit ; avec
    /// `LF_PISTE_ENV=production`, les trois disent `production` (le défaut est bien ce qui change).
    #[tokio::test]
    async fn default_env_is_sandbox() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let launch = Launch::new(audit.path()).credentials();
        assert_eq!(launch.var("LF_PISTE_ENV"), None, "the default must come from the binary");

        let reported = reported_env(&launch).await;
        assert_eq!(reported["source"], "default", "{reported}");
        assert_eq!(reported["value"], "sandbox", "{reported}");
        let (declared, audited) = declared_env(&launch, audit.path()).await;
        assert_eq!((declared.as_str(), audited.as_str()), ("sandbox", "sandbox"));

        let audit = tempfile::tempdir().expect("a temporary directory");
        let launch = Launch::new(audit.path()).credentials().env("LF_PISTE_ENV", "production");
        let reported = reported_env(&launch).await;
        assert_eq!(reported["source"], "LF_PISTE_ENV", "{reported}");
        assert_eq!(reported["value"], "production", "{reported}");
        let (declared, audited) = declared_env(&launch, audit.path()).await;
        assert_eq!((declared.as_str(), audited.as_str()), ("production", "production"));
    }

    /// La ligne de démarrage (`starting`) de stderr, si le serveur en a écrit une.
    fn starting_line(stderr: &str) -> Option<&str> {
        stderr.lines().find(|line| line.contains("starting") && line.contains("INFO"))
    }

    /// Régression : D-18. Au démarrage, en sandbox, stderr porte un événement `info` qui nomme
    /// l'environnement (`env=sandbox`) ; en production il nomme `production`, et le mot `sandbox`
    /// n'y figure pas. Le rappel vient donc de l'environnement réel et non d'un texte fixe.
    #[tokio::test]
    async fn startup_logs_sandbox_notice() {
        for (setting, expected, other) in [
            (None, "env=sandbox", "env=production"),
            (Some("production"), "env=production", "sandbox"),
        ] {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let mut launch = Launch::new(audit.path()).credentials();
            if let Some(value) = setting {
                launch = launch.env("LF_PISTE_ENV", value);
            }
            let mut session = launch.spawn();
            session.handshake().await;
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
            let line = starting_line(&finished.stderr).unwrap_or_else(|| {
                fail(&format!("no info `starting` event on stderr:\n{}", finished.stderr))
            });
            assert!(line.contains(expected), "{line}");
            assert!(!line.contains(other), "{line}");
        }
    }

    // ---- D-07 : tout outil passe par Audited -----------------------------------------------------

    /// Régression : D-07. Chaque outil que `tools/list` annonce, appelé avec une entrée vide (rejetée
    /// pour la plupart, acceptée pour `fr_coverage`), produit exactement une ligne d'audit à son nom,
    /// écrite avant que la réponse ne parte. La liste annoncée est celle des treize outils
    /// enregistrés (`ToolName::ALL`) : un outil ajouté sans audit ou sans test ferait échouer ce test.
    #[tokio::test]
    async fn every_tool_goes_through_audited() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::silent().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        session.handshake().await;

        let listed = session.request(&method_request(2, "tools/list")).await;
        let mut names: Vec<String> = listed["result"]["tools"]
            .as_array()
            .expect("a tool list")
            .iter()
            .map(|tool| tool["name"].as_str().expect("a tool name").to_owned())
            .collect();
        names.sort();
        let mut registered: Vec<String> =
            ToolName::ALL.iter().map(|tool| tool.as_str().to_owned()).collect();
        registered.sort();
        assert_eq!(names, registered, "tools/list and the registered tools differ");

        for (index, name) in names.iter().enumerate() {
            let answer = session.request(&call(10 + index as u64, name, &json!({}))).await;
            assert!(answer.get("error").is_none(), "{name}: protocol error {answer}");
            let lines = audit_lines(audit.path());
            let audited: Vec<&Value> = lines.iter().map(|line| &line["tool"]).collect();
            assert_eq!(lines.len(), index + 1, "{name}: one audit line per call, got {audited:?}");
            let line = &lines[index];
            assert_eq!(line["tool"], json!(name), "{line}");
            let accepted = line["status"] == "ok";
            assert_eq!(accepted, !is_tool_error(&answer), "{name}: audit and answer disagree");
            if !accepted {
                assert_eq!(line["error_code"], "invalid_arg", "{name}: {line}");
            }
        }
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        // Aucun de ces appels n'a besoin du réseau : l'audit seul les a vus.
        assert!(piste.requests().await.is_empty());
    }

    // ---- D-05 : le serveur reste vivant pendant qu'un outil attend l'amont ----------------------

    /// Un faux PISTE qui sert le jeton tout de suite et fait attendre `delay` toute autre requête.
    async fn upstream_waiting(delay: Duration) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    br#"{"access_token":"slow-token","token_type":"Bearer","expires_in":3600}"#
                        .to_vec(),
                    "application/json",
                ),
            )
            .mount(&server)
            .await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_delay(delay).set_body_raw(
                br#"{"totalResultNumber":0,"results":[]}"#.to_vec(),
                "application/json",
            ))
            .with_priority(10)
            .mount(&server)
            .await;
        server
    }

    /// Plafond de la réponse à `server/discover` pendant l'attente de l'amont. Le catalogue vise
    /// 100 ms ; la marge couvre une CI lente, et reste très en deçà des 2 s de l'amont : un serveur
    /// qui bloquerait sa boucle d'événements (D-05) ne répondrait qu'après elles.
    const DISCOVER_CEILING: Duration = Duration::from_millis(1000);

    /// Régression : D-05. Un appel d'outil attend un amont lent de 2 s ; `server/discover`, envoyé
    /// pendant l'attente, reçoit sa réponse avant le plafond et avant la réponse de l'outil, qui
    /// arrive ensuite, intacte, avec sa ligne d'audit.
    #[tokio::test]
    async fn server_answers_discover_while_tool_waits_upstream() {
        let delay = Duration::from_secs(2);
        let server = upstream_waiting(delay).await;
        let audit = tempfile::tempdir().expect("a temporary directory");
        let mut session = Launch::new(audit.path())
            .credentials()
            .env("LF_PISTE_OAUTH_URL", &format!("{}{TOKEN_PATH}", server.uri()))
            .env("LF_PISTE_BASE_URL", &format!("{}{API_BASE}", server.uri()))
            .env("NO_PROXY", "127.0.0.1,localhost")
            .spawn();

        session.send(&modern_call(1, "fr_search", &json!({"query": "bail commercial"}))).await;
        // L'appel est chez l'amont : la requête de recherche attend ses 2 s.
        let started = Instant::now();
        loop {
            let seen = server.received_requests().await.unwrap_or_default();
            if seen.iter().any(|request| request.url.path() == format!("{API_BASE}/search")) {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the call never reached the upstream"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let sent = Instant::now();
        session.send(&crate::common::discover(2)).await;
        let Read::Line(line) = session.read_line(Duration::from_secs(10)).await else {
            return fail("no answer to server/discover");
        };
        let answered = sent.elapsed();
        let answer: Value = serde_json::from_str(&line).expect("a JSON line");
        assert_eq!(answer["id"], json!(2), "the tool answered first: {answer:.200}");
        assert_eq!(answer["result"]["resultType"], "complete", "{answer:.200}");
        assert!(
            answered < DISCOVER_CEILING,
            "server/discover took {answered:?} behind a 2 s upstream"
        );

        // Puis l'outil répond, après l'attente de l'amont.
        let result = session.recv().await;
        assert_eq!(result["id"], json!(1), "{result:.200}");
        assert!(started.elapsed() >= delay, "the tool answered before the upstream did");
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        let lines = audit_lines(audit.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0]["tool"], "fr_search");
    }

    // ---- D-38 : une écriture d'audit lente ne fige ni la boucle ni les autres appels --------------

    /// Retient le verrou consultatif du journal d'audit (`.mcp-legifrance.lock`, `flock`) pendant
    /// `hold` secondes, comme le ferait un autre processus qui écrit. Rend quand le verrou est pris.
    #[cfg(target_os = "linux")]
    async fn hold_audit_lock(dir: &Path, hold: u64) -> tokio::process::Child {
        use std::process::Stdio;

        use tokio::io::{AsyncBufReadExt as _, BufReader};

        let lock = dir.join(".mcp-legifrance.lock");
        let script = format!("exec 9>'{}'; flock -x 9; echo locked; sleep {hold}", lock.display());
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", &script])
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("sh and flock are available");
        let mut out = BufReader::new(child.stdout.take().expect("a piped stdout")).lines();
        let line = tokio::time::timeout(Duration::from_secs(10), out.next_line())
            .await
            .expect("the lock is taken")
            .expect("readable")
            .expect("a line");
        assert_eq!(line, "locked");
        child
    }

    /// Régression : D-38. Un autre écrivain tient le verrou du journal 3 s : l'écriture d'audit de
    /// chaque appel attend. Pendant ce temps le serveur répond à `ping`, et un second appel
    /// progresse jusqu'à l'amont (il n'est pas rangé derrière le premier) ; aucune réponse d'outil
    /// ne part avant l'accusé de l'audit. Le verrou rendu, les deux appels aboutissent et ont
    /// chacun leur ligne.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn tool_calls_progress_while_audit_write_is_slow() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::serving_search().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        session.handshake().await;

        let hold = 3;
        let _holder = hold_audit_lock(audit.path(), hold).await;
        let held_since = Instant::now();
        // Premier appel : sans réseau, il n'attend que l'écriture de son audit.
        session.send(&call(2, "fr_coverage", &json!({}))).await;
        // Second appel : il doit atteindre l'amont malgré le premier.
        session.send(&call(3, "fr_search", &json!({"query": "bail commercial"}))).await;
        let requests = piste.wait_for_requests(2).await;
        assert!(requests.len() >= 2, "the second call never reached the upstream: {requests:?}");
        assert!(
            held_since.elapsed() < Duration::from_secs(hold),
            "the lock was released too early"
        );

        // La boucle vit : `ping` reçoit sa réponse alors que l'audit est bloqué.
        session.send(&method_request(4, "ping")).await;
        let Read::Line(line) = session.read_line(Duration::from_secs(2)).await else {
            return fail("no answer to ping while the audit write is slow");
        };
        let pong: Value = serde_json::from_str(&line).expect("a JSON line");
        assert_eq!(pong["id"], json!(4), "a tool answered before its audit line: {pong:.200}");

        // Les deux appels attendent l'accusé de l'audit : rien ne part avant.
        assert!(matches!(session.read_line(Duration::from_millis(300)).await, Read::Silence));
        assert!(
            held_since.elapsed() < Duration::from_secs(hold),
            "the lock was released too early"
        );

        // Le verrou rendu, les deux réponses arrivent.
        let mut ids = Vec::new();
        for _ in 0..2 {
            let answer = session.recv().await;
            assert!(!is_tool_error(&answer), "{answer:.300}");
            ids.push(answer["id"].clone());
        }
        ids.sort_by_key(Value::to_string);
        assert_eq!(ids, [json!(2), json!(3)]);
        assert!(
            held_since.elapsed() >= Duration::from_secs(hold),
            "answers came out under the lock"
        );
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        let mut tools: Vec<String> = audit_lines(audit.path())
            .iter()
            .map(|line| line["tool"].as_str().expect("a tool").to_owned())
            .collect();
        tools.sort();
        assert_eq!(tools, ["fr_coverage", "fr_search"]);
    }

    // ---- D-16 : l'accusé de l'audit vient après l'écriture ---------------------------------------

    fn record(index: u64) -> AuditRecord {
        AuditRecord {
            ts: chrono::DateTime::UNIX_EPOCH,
            tool: ToolName::FR_SEARCH,
            input: CanonicalInput::new(format!(r#"{{"query":"q{index}"}}"#)),
            output_count_or_size: index,
            duration_ms: 1,
            status: AuditStatus::Ok,
            error_code: None,
            error: None,
            env: PisteEnvironment::Sandbox,
            server_version: "0.0.0",
        }
    }

    /// Les lignes du journal actif de `dir`, lues par un descripteur indépendant de l'écrivain.
    fn journal(dir: &Path) -> String {
        fs::read_to_string(dir.join("mcp-legifrance.jsonl")).unwrap_or_default()
    }

    /// Régression : D-16. `record` ne rend `Ok` qu'une fois la ligne écrite, entière, dans le
    /// journal : lu par un autre descripteur dès le retour, le fichier porte exactement les lignes
    /// accusées, terminées par un saut de ligne, dans l'ordre. Quand l'écriture est impossible
    /// (répertoire sans droit d'écriture, ou chemin qui est un fichier), `record` rend l'erreur
    /// et n'accuse rien : il n'y a pas d'accusé sans écriture.
    ///
    /// Limite : l'appel `sync_data` lui-même n'est pas observable d'un test (le noyau sert les
    /// pages non synchronisées à tout lecteur) ; ce test garde l'ordre accusé après écriture.
    #[tokio::test]
    async fn record_returns_only_after_sync() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let log = JsonlAuditLog::open(dir.path(), JsonlAuditLog::DEFAULT_ROTATE_BYTES)
            .expect("the log opens without any I/O");
        assert_eq!(journal(dir.path()), "", "nothing is written before the first record");
        for index in 1..=20_u64 {
            log.record(&record(index)).await.expect("the line is acknowledged");
            let text = journal(dir.path());
            assert!(text.ends_with('\n'), "an acknowledged line is whole: {text:?}");
            let lines: Vec<Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).expect("a whole JSON line"))
                .collect();
            assert_eq!(lines.len() as u64, index, "{text}");
            assert_eq!(lines[lines.len() - 1]["output_count_or_size"], json!(index));
        }

        // Aucun accusé quand rien n'a pu être écrit : le « répertoire » du journal est un fichier.
        let blocker = tempfile::NamedTempFile::new().expect("a temporary file");
        let broken = JsonlAuditLog::open(blocker.path(), JsonlAuditLog::DEFAULT_ROTATE_BYTES)
            .expect("the log opens without any I/O");
        let error = broken.record(&record(1)).await.expect_err("nothing could be written");
        assert!(
            matches!(
                error.kind,
                AuditErrorKind::Io | AuditErrorKind::PermissionDenied | AuditErrorKind::WriterGone
            ),
            "{error:?}"
        );
        assert_eq!(error.to_string(), format!("audit log unavailable ({:?})", error.kind));
    }

    // ---- D-40 : aucune partie d'URL à identifiants n'atteint une sortie -----------------------

    /// Les valeurs d'une URL piégée qui ne doivent apparaître nulle part.
    struct UrlParts {
        user: String,
        password: String,
        query: String,
        fragment: String,
    }

    impl UrlParts {
        fn new() -> Self {
            Self {
                user: format!("u{}", random_hex()),
                password: format!("p{}", random_hex()),
                query: format!("q{}", random_hex()),
                fragment: format!("f{}", random_hex()),
            }
        }

        /// `scheme://user:pw@host/path?query#fragment`.
        fn url(&self, scheme_and_host: &str, path: &str) -> String {
            let (scheme, host) = scheme_and_host.split_once("://").expect("a scheme");
            format!(
                "{scheme}://{}:{}@{host}{path}?token={}#{}",
                self.user, self.password, self.query, self.fragment
            )
        }

        fn assert_clean(&self, label: &str, text: &str) {
            for (name, value) in [
                ("user name", &self.user),
                ("password", &self.password),
                ("query", &self.query),
                ("fragment", &self.fragment),
            ] {
                assert!(!text.contains(value.as_str()), "{label} shows the URL {name}:\n{text}");
            }
        }
    }

    /// Joue `fr_search` et `fr_coverage` sur `launch` (journaux en `trace`), puis rend tout ce que
    /// le processus a montré : stdout, stderr, audit, et les deux rapports de `check-config`.
    async fn everything_shown(launch: &Launch, audit: &Path) -> String {
        let launch = launch.clone().env("LF_LOG", "trace");
        let mut session = launch.spawn();
        session.handshake().await;
        let search = session.request(&call(2, "fr_search", &json!({"query": "bail"}))).await;
        assert!(is_tool_error(&search), "{search}");
        let coverage = session.request(&call(3, "fr_coverage", &json!({}))).await;
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        let mut shown =
            format!("{search}\n{coverage}\n{}\n{}", finished.stdout.join("\n"), finished.stderr);
        shown.push('\n');
        shown.push_str(
            &audit_lines(audit).iter().map(Value::to_string).collect::<Vec<_>>().join("\n"),
        );
        for format in ["text", "json"] {
            let report =
                launch.clone().args(&["check-config", "--format", format]).run_to_end().await;
            shown.push('\n');
            shown.push_str(&report.stdout);
            shown.push('\n');
            shown.push_str(&report.stderr);
        }
        shown
    }

    /// Régression : D-40. Une URL de configuration piégée (identifiant, mot de passe, requête,
    /// fragment aléatoires) est refusée sans que rien n'en soit montré : ni dans la réponse de
    /// l'outil, ni sur stdout, ni sur stderr en `trace`, ni dans l'audit, ni dans `check-config`.
    /// Même chose quand l'amont redirige vers une telle URL (`Location`) : l'appel échoue sans
    /// la suivre ni la répéter. Le refus lui-même reste dit (`config_error`, `upstream_error`).
    #[tokio::test]
    async fn redact_url_drops_userinfo_and_query() {
        // 1. Les URL de configuration.
        let parts = UrlParts::new();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let launch = Launch::new(audit.path())
            .credentials()
            .env(
                "LF_PISTE_OAUTH_URL",
                &parts.url("https://sandbox-oauth.piste.gouv.fr", "/api/oauth/token"),
            )
            .env(
                "LF_PISTE_BASE_URL",
                &parts.url("https://sandbox-api.piste.gouv.fr", "/dila/legifrance/lf-engine-app"),
            );
        let shown = everything_shown(&launch, audit.path()).await;
        parts.assert_clean("a rejected configuration", &shown);
        assert!(shown.contains("[config_error]"), "the refusal is still reported:\n{shown}");
        assert!(shown.contains("LF_PISTE_BASE_URL"), "the setting is named:\n{shown}");

        // 2. Une redirection de l'amont vers une URL à identifiants.
        let parts = UrlParts::new();
        let trap = parts.url("http://127.0.0.1:1", "/trap");
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    br#"{"access_token":"redirect-token","token_type":"Bearer","expires_in":3600}"#
                        .to_vec(),
                    "application/json",
                ),
            )
            .mount(&server)
            .await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(302).insert_header("Location", trap.as_str()))
            .with_priority(10)
            .mount(&server)
            .await;
        let audit = tempfile::tempdir().expect("a temporary directory");
        let launch = Launch::new(audit.path())
            .credentials()
            .env("LF_PISTE_OAUTH_URL", &format!("{}{TOKEN_PATH}", server.uri()))
            .env("LF_PISTE_BASE_URL", &format!("{}{API_BASE}", server.uri()))
            .env("NO_PROXY", "127.0.0.1,localhost");
        let shown = everything_shown(&launch, audit.path()).await;
        parts.assert_clean("a redirect to a booby-trapped URL", &shown);
        assert!(shown.contains("[upstream_error]"), "the failure is still reported:\n{shown}");
    }

    // ---- D-04 : aucune URL de production hors liste blanche --------------------------------------

    /// La partie « production » d'un fichier source : tout ce qui précède le premier module de test
    /// (`#[cfg(test)]` en tête de ligne).
    fn production_part(text: &str) -> &str {
        match text.find("\n#[cfg(test)]") {
            Some(end) => &text[..end],
            None => text,
        }
    }

    /// Les hôtes des URL littérales (`http://` ou `https://`) du code de `text`, avec leur ligne.
    /// Les lignes de commentaire (`//`, `///`, `//!`) sont ignorées : elles ne sont jamais jointes.
    fn url_hosts(text: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let mut rest = line;
            while let Some(at) = rest.find("://") {
                let scheme = rest[..at].rsplit(|c: char| !c.is_ascii_alphabetic()).next();
                let after = &rest[at + 3..];
                if matches!(scheme, Some("http" | "https")) {
                    let authority = after
                        .split(['/', '"', '\\', '?', '#', ' ', ')'])
                        .next()
                        .unwrap_or_default();
                    found.push((
                        number + 1,
                        format!(
                            "{}{}",
                            scheme.unwrap_or_default(),
                            authority_part(authority, after)
                        ),
                    ));
                }
                rest = after;
            }
        }
        found
    }

    /// `://<authority>` : l'hôte (userinfo et port compris) et, pour un dépôt GitHub, son début de chemin.
    fn authority_part(authority: &str, after: &str) -> String {
        if authority == "github.com" {
            let path: String =
                after.chars().take_while(|c| !matches!(c, '"' | '\\' | ' ' | ')')).collect();
            format!("://{path}")
        } else {
            format!("://{authority}")
        }
    }

    /// Vrai si l'URL (`scheme://hôte[/chemin]`) est sur la liste blanche de 06 §10 : les hôtes
    /// PISTE (`piste.gouv.fr` et ses sous-domaines), `www.legifrance.gouv.fr`, le dépôt du projet,
    /// et la boucle locale en `http` (surcharge locale, 04 §2.3). Un userinfo ou un hôte qui n'est
    /// qu'un préfixe d'un autre (`legifrance.gouv.fr.example.org`) n'y est pas.
    fn allowlisted(url: &str) -> bool {
        let Some((scheme, rest)) = url.split_once("://") else { return false };
        if rest.contains('@') {
            return false;
        }
        let host = rest.split('/').next().unwrap_or_default();
        let name = host
            .rsplit_once(':')
            .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
            .map_or(host, |(name, _)| name);
        match scheme {
            "https" => {
                name == "piste.gouv.fr"
                    || name.ends_with(".piste.gouv.fr")
                    || name == "www.legifrance.gouv.fr"
                    || rest.starts_with("github.com/muchiny/mcp-legifrance")
            }
            "http" => matches!(name, "127.0.0.1" | "localhost" | "[::1]"),
            _ => false,
        }
    }

    /// Les fichiers `src/**/*.rs` de chaque crate.
    fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    /// Régression : D-04. Aucune URL littérale du code de production (`crates/*/src`, hors modules
    /// de test) ne sort de la liste blanche : hôtes `*.piste.gouv.fr`, `www.legifrance.gouv.fr`,
    /// dépôt du projet, boucle locale. Une URL pour un autre hôte (télémétrie, manifeste distant) y
    /// serait vue. L'analyse elle-même est vérifiée sur des exemples, et ne lit pas zéro fichier.
    #[test]
    fn production_sources_contain_only_allowlisted_urls() {
        // L'analyse voit les URL piégées et laisse passer les permises.
        let sample = "let a = \"https://www.legifrance.gouv.fr/loda/id/X\";\n\
                      // https://ignored.example.org\n\
                      let b = \"https://telemetry.example.org/v1\";\n\
                      let c = \"https://user:pw@sandbox-api.piste.gouv.fr/x\";\n\
                      let d = \"https://www.legifrance.gouv.fr.example.org/\";\n\
                      let e = \"https://github.com/muchiny/mcp-legifrance\";\n\
                      let f = \"http://127.0.0.1:8080/x\";\n\
                      let g = \"http://sandbox-api.piste.gouv.fr/x\";\n\
                      #[cfg(test)]\nmod tests { let h = \"https://tests.example.org\"; }\n";
        let verdicts: Vec<(usize, bool)> = url_hosts(production_part(sample))
            .iter()
            .map(|(line, url)| (*line, allowlisted(url)))
            .collect();
        assert_eq!(
            verdicts,
            [(1, true), (3, false), (4, false), (5, false), (6, true), (7, true), (8, false)]
        );

        let mut files = Vec::new();
        let crates = repository().join("crates");
        for entry in fs::read_dir(&crates).expect("the crates directory").flatten() {
            sources(&entry.path().join("src"), &mut files);
        }
        assert!(files.len() > 100, "only {} source files scanned", files.len());
        let mut scanned_urls = 0;
        let mut offenders = Vec::new();
        for file in &files {
            // `testing.rs` est le module des doublons d'essai (feature `test-support`), absent du
            // binaire : ses URL visent le faux serveur local du test (`http://{}`).
            if file.file_name().is_some_and(|name| name == "testing.rs") {
                continue;
            }
            let text = fs::read_to_string(file).expect("a UTF-8 source file");
            for (line, url) in url_hosts(production_part(&text)) {
                scanned_urls += 1;
                if !allowlisted(&url) {
                    offenders.push(format!(
                        "{}:{line}: {url}",
                        file.strip_prefix(&crates).unwrap_or(file).display()
                    ));
                }
            }
        }
        assert!(scanned_urls >= 5, "only {scanned_urls} URLs found: the scan is blind");
        assert!(offenders.is_empty(), "URLs outside the allowlist:\n{}", offenders.join("\n"));
    }

    // ---- l'ensemble -----------------------------------------------------------------------------

    /// Les huit défauts de sécurité du catalogue et le test de ce fichier ou de
    /// `parity_tool_output.rs` qui porte chaque garantie : (défaut, nom du test).
    const GUARANTEES: [(&str, &str); 10] = [
        ("D-04", "production_sources_contain_only_allowlisted_urls"),
        ("D-05", "server_answers_discover_while_tool_waits_upstream"),
        ("D-07", "every_tool_goes_through_audited"),
        ("D-16", "record_returns_only_after_sync"),
        ("D-18", "default_env_is_sandbox"),
        ("D-18", "startup_logs_sandbox_notice"),
        ("D-30", "no_view_has_eli_uri"),
        ("D-30", "ported_no_fabricated_eli"),
        ("D-38", "tool_calls_progress_while_audit_write_is_slow"),
        ("D-40", "redact_url_drops_userinfo_and_query"),
    ];

    /// Régression : D-43, volet sécurité. Chaque garantie de sécurité du catalogue 06 §3.1 est une
    /// ligne du catalogue, a un test du même nom écrit dans ces deux fichiers, et la liste
    /// `SECURITY_PENDING` des excuses de `parity_tool_output.rs` est vide. Se lance avec
    /// `cargo nextest run -p mcp-legifrance --run-ignored only -E 'test(security_regressions)'`.
    #[test]
    #[ignore = "ensemble de F3 (plan §6) : se lance à part, voir --run-ignored only -E 'test(security_regressions)'"]
    fn security_regressions_cover_the_security_catalogue() {
        let catalogue = fs::read_to_string(repository().join("docs/plan/06-qualite.md"))
            .expect("the quality plan");
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let ours = fs::read_to_string(tests.join("security_regressions.rs")).expect("this file");
        let parity = fs::read_to_string(tests.join("parity_tool_output.rs")).expect("parity file");
        for (defect, name) in GUARANTEES {
            let row = catalogue
                .lines()
                .find(|line| line.starts_with(&format!("| {defect} ")))
                .unwrap_or_else(|| fail(&format!("{defect} is not in the catalogue")));
            assert!(row.contains(&format!("`{name}`")), "{defect} no longer names `{name}`");
            let written = format!("fn {name}()");
            assert!(
                ours.contains(&format!("async {written}"))
                    || ours.contains(&format!(" {written}"))
                    || parity.contains(&written)
                    || parity.contains(&format!("async {written}")),
                "{defect}: no test named {name}"
            );
        }
        assert!(
            parity.contains("const SECURITY_PENDING: [(&str, &str); 0] = [];"),
            "SECURITY_PENDING must stay empty"
        );
        assert!(
            !parity.contains("#[ignore = \"F3"),
            "security_regressions_are_all_implemented must run by default"
        );
    }
}
