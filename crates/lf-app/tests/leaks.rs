//! Fuites du binaire `mcp-legifrance` (T-M4-07, 06 §2.7 et §10) : aucun secret, aucun identifiant
//! client, aucune partie d'URL à identifiants et aucun texte de requête ne sort, ni sur stdout, ni
//! sur stderr (en `LF_LOG=trace`), ni dans le journal d'audit, ni dans les messages d'erreur.
//!
//! Les valeurs sont tirées au hasard à chaque test et injectées par l'environnement. Les faux
//! PISTE répondent le pire : des corps d'erreur qui recopient l'identifiant, le secret et le jeton.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#[cfg(test)]
mod common;

#[cfg(test)]
mod leaks {
    use std::path::Path;

    use serde_json::{Value, json};

    use crate::common::{
        FAKE_TOKEN, FakePiste, Launch, Session, audit_lines, call, first_text, random_hex,
    };

    /// Les valeurs qui ne doivent apparaître nulle part.
    struct Secrets {
        client_id: String,
        client_secret: String,
        /// Mot de passe d'une URL à identifiants (`user:motdepasse@hôte`).
        password: String,
    }

    impl Secrets {
        fn new() -> Self {
            Self { client_id: random_hex(), client_secret: random_hex(), password: random_hex() }
        }

        /// Le corps d'erreur le plus bavard qu'un amont puisse rendre : il recopie tout.
        fn echoing_body(&self) -> String {
            format!(
                r#"{{"error":"invalid_client","status":"Unauthorized","message":"client_id={} client_secret={} access_token={FAKE_TOKEN} Authorization: Bearer {FAKE_TOKEN} url=http://user:{}@127.0.0.1:1/x","errorCode":null}}"#,
                self.client_id, self.client_secret, self.password
            )
        }

        fn launch(&self, audit: &Path) -> Launch {
            Launch::new(audit)
                .credentials_of(&self.client_id, &self.client_secret)
                .env("LF_LOG", "trace")
        }

        /// Le corps d'une erreur ordinaire de l'amont, qui ne recopie rien.
        fn plain_body() -> String {
            r#"{"error":400,"status":"Bad Request","message":"Bad request.","errorCode":null}"#
                .to_owned()
        }

        /// Échoue si `text` cite l'une des valeurs.
        fn assert_clean(&self, label: &str, text: &str) {
            for (name, value) in [
                ("client id", &self.client_id),
                ("client secret", &self.client_secret),
                ("URL password", &self.password),
                ("access token", &FAKE_TOKEN.to_owned()),
            ] {
                assert!(!text.contains(value.as_str()), "{label} leaks the {name}:\n{text}");
            }
        }

        /// Échoue si `text` cite le secret client ou le jeton : ce que l'adapter retire du détail
        /// d'une erreur de l'amont (« jeton et secret client retirés », annexe A S10). Le reste de
        /// ce que l'amont recopie dans son message (identifiant client, URL) est relayé tel quel.
        fn assert_secrets_clean(&self, label: &str, text: &str) {
            for (name, value) in
                [("client secret", &self.client_secret), ("access token", &FAKE_TOKEN.to_owned())]
            {
                assert!(!text.contains(value.as_str()), "{label} leaks the {name}:\n{text}");
            }
        }
    }

    /// Les appels qui traversent la composition réelle : recherche, lecture d'un article, couverture.
    async fn exercise(session: &mut Session) -> Vec<Value> {
        let mut responses = Vec::new();
        for (id, tool, arguments) in [
            (2, "fr_search", json!({"query": "bail commercial"})),
            (3, "fr_get_text", json!({"article_id": "LEGIARTI000006419288"})),
            (4, "fr_coverage", json!({})),
        ] {
            responses.push(session.request(&call(id, tool, &arguments)).await);
        }
        responses
    }

    /// Ce que l'amont recopie dans le message qu'il rend.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Upstream {
        /// Rien : tout ce qui sort vient du serveur, et rien de ce qui est secret n'en fait partie.
        Quiet,
        /// Le secret, le jeton, l'identifiant client et une URL à identifiants : le détail de l'erreur
        /// de l'amont est relayé (nettoyé du secret et du jeton, annexe A S10).
        Echoing,
    }

    /// Lance le binaire avec `launch`, joue [`exercise`], puis contrôle stdout, stderr, l'audit,
    /// les messages et le rapport de `check-config`. Rend ce que le serveur a écrit sur stdout.
    async fn assert_nothing_leaks(
        label: &str,
        secrets: &Secrets,
        launch: &Launch,
        audit: &Path,
        upstream: Upstream,
    ) -> String {
        // Le détail d'une erreur de l'amont est relayé aux messages, à stdout (qui les porte), aux
        // journaux et à l'audit : c'est là que seuls le secret et le jeton sont retirés.
        let relayed = |text: &str, channel: &str| match upstream {
            Upstream::Quiet => secrets.assert_clean(&format!("{label}: {channel}"), text),
            Upstream::Echoing => secrets.assert_secrets_clean(&format!("{label}: {channel}"), text),
        };
        let mut session = launch.spawn();
        session.handshake().await;
        let responses = exercise(&mut session).await;
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "{label}: stderr:\n{}", finished.stderr);
        assert!(!finished.stderr.is_empty(), "{label}: LF_LOG=trace logs nothing");

        let stdout = finished.stdout.join("\n");
        relayed(&stdout, "stdout");
        relayed(&finished.stderr, "stderr");
        for response in &responses {
            let text = match response["result"]["content"].get(0) {
                Some(_) => first_text(response).to_owned(),
                None => response.to_string(),
            };
            relayed(&text, "message");
        }
        let audit_text: String = audit_lines(audit).iter().map(Value::to_string).collect();
        relayed(&audit_text, "audit");

        for format in ["text", "json"] {
            let report =
                launch.clone().args(&["check-config", "--format", format]).run_to_end().await;
            secrets.assert_clean(&format!("{label}: check-config {format}"), &report.stdout);
            secrets.assert_clean(&format!("{label}: check-config {format} stderr"), &report.stderr);
        }
        stdout
    }

    /// D-40, C-02, C-34 : un secret et un identifiant aléatoires, injectés par l'environnement,
    /// n'apparaissent ni dans stdout, ni dans stderr (en `LF_LOG=trace`), ni dans le journal
    /// d'audit, ni dans les messages d'erreur, ni dans `check-config`, quelle que soit l'issue :
    /// jeton refusé, appel refusé, ou configuration à URL à identifiants.
    #[tokio::test]
    async fn error_messages_never_contain_secret_or_userinfo() {
        // 1. Le jeton est refusé (401) par un amont qui recopie les identifiants.
        let secrets = Secrets::new();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::answering(401, &secrets.echoing_body()).await;
        let launch = secrets.launch(audit.path()).piste(&piste);
        assert_nothing_leaks("token refused", &secrets, &launch, audit.path(), Upstream::Quiet)
            .await;
        assert!(!piste.requests().await.is_empty(), "the scenario never reached the upstream");

        // 2. Le jeton est bon, l'appel est refusé par un amont qui ne recopie rien, puis par un
        //    amont qui recopie tout : le secret et le jeton ne sortent jamais.
        for (status, echoing) in
            [(400, false), (401, false), (403, false), (400, true), (401, true), (403, true)]
        {
            let secrets = Secrets::new();
            let audit = tempfile::tempdir().expect("a temporary directory");
            let (body, upstream) = if echoing {
                (secrets.echoing_body(), Upstream::Echoing)
            } else {
                (Secrets::plain_body(), Upstream::Quiet)
            };
            let piste = FakePiste::token_then_api(status, &body).await;
            let launch = secrets.launch(audit.path()).piste(&piste);
            let label = format!("API {status}, echoing: {echoing}");
            let stdout =
                assert_nothing_leaks(&label, &secrets, &launch, audit.path(), upstream).await;
            assert!(piste.requests().await.len() > 1, "{label}: no call reached the API");
            // Le test n'est pas vide : sur un 400, le détail de l'amont est bien relayé, et c'est le
            // retrait du secret et du jeton qui l'a nettoyé.
            if echoing && status == 400 {
                assert!(
                    stdout.contains("[redacted]"),
                    "{label}: the upstream detail was not relayed"
                );
                assert!(stdout.contains("client_secret=[redacted]"), "{label}: {stdout:.2000}");
            }
        }

        // 3. Les URL PISTE portent des identifiants, dont le vrai secret : la configuration est
        //    refusée (mode dégradé) sans citer l'URL ni ses identifiants.
        let secrets = Secrets::new();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let launch = secrets
            .launch(audit.path())
            .env(
                "LF_PISTE_OAUTH_URL",
                &format!(
                    "http://{}:{}@127.0.0.1:1/oauth/token",
                    secrets.client_id, secrets.client_secret
                ),
            )
            .env("LF_PISTE_BASE_URL", &format!("http://user:{}@127.0.0.1:1/api", secrets.password));
        assert_nothing_leaks("userinfo in URLs", &secrets, &launch, audit.path(), Upstream::Quiet)
            .await;
        let mut session = launch.spawn();
        session.handshake().await;
        let search = session.request(&call(2, "fr_search", &json!({"query": "bail"}))).await;
        assert!(first_text(&search).starts_with("[config_error] "), "{search}");
        assert!(first_text(&search).contains("must not contain credentials"), "{search}");
        session.finish().await;
    }

    /// `check-config` masque le secret et ne montre qu'un début de l'identifiant client (02 §8.1).
    #[tokio::test]
    async fn check_config_masks_secret_and_client_id() {
        let secrets = Secrets::new();
        let audit = tempfile::tempdir().expect("a temporary directory");
        let launch = secrets.launch(audit.path());
        let report = launch.clone().args(&["check-config"]).run_to_end().await;
        assert_eq!(report.code, Some(0), "{report:?}");
        secrets.assert_clean("check-config", &report.stdout);
        let secret_line = report
            .stdout
            .lines()
            .find(|line| line.starts_with("LF_PISTE_CLIENT_SECRET"))
            .expect("a line");
        assert!(secret_line.contains("set (32 characters)"), "{secret_line}");
        let id_line = report
            .stdout
            .lines()
            .find(|line| line.starts_with("LF_PISTE_CLIENT_ID"))
            .expect("a line");
        assert!(id_line.contains(&secrets.client_id[..4]), "{id_line}");
        assert!(!id_line.contains(&secrets.client_id[..5]), "{id_line}");
    }

    /// Une requête JSON-RPC `method` de paramètres `params`.
    fn method(id: u64, method: &str, params: &Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    /// §10, D-15 : une requête marquée n'apparaît pas dans stderr, même en `trace`, ni dans le
    /// journal d'audit (qui n'a que son empreinte), quelle que soit l'issue de l'appel : succès,
    /// servi par le cache, refus de l'amont, argument invalide, identifiants absents.
    #[tokio::test]
    async fn logs_never_contain_query_text() {
        let marker = format!("lf-test-query-{}", random_hex());
        let launches: Vec<(&str, FakePiste, bool)> = vec![
            ("success", FakePiste::serving_search().await, true),
            ("upstream refusal", FakePiste::token_then_api(400, r#"{"message":"no"}"#).await, true),
            ("missing credentials", FakePiste::silent().await, false),
        ];
        for (label, piste, credentials) in launches {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let mut launch = Launch::new(audit.path()).piste(&piste).env("LF_LOG", "trace");
            if credentials {
                launch = launch.credentials();
            }
            let mut session = launch.spawn();
            session.handshake().await;
            let search = json!({"query": marker});
            // Deux fois : la seconde est servie par le cache quand l'amont a répondu.
            for id in [2, 3] {
                session.request(&call(id, "fr_search", &search)).await;
            }
            // Un argument invalide, qui porte la requête, et un texte à `focus` (requête aussi).
            session.request(&call(4, "fr_search", &json!({"query": marker, "page_size": 0}))).await;
            session
                .request(&call(
                    5,
                    "fr_get_text",
                    &json!({"article_id": "LEGIARTI000006419288", "detail": "focus", "focus": marker}),
                ))
                .await;
            // Resources, complétion et prompt : une URI hors grammaire, une valeur de complétion
            // et un argument de prompt qui portent la marque. rmcp journalise toute erreur de
            // handler avec son `data` : aucune ne doit la recopier.
            let outside_grammar = format!("legifrance://article/{marker} x");
            for (id, request) in [(6, json!({"uri": marker})), (7, json!({"uri": outside_grammar}))]
            {
                session.request(&method(id, "resources/read", &request)).await;
            }
            let completion = json!({
                "ref": {"type": "ref/resource", "uri": "legifrance://code/{code}/article/{num}"},
                "argument": {"name": "code", "value": marker},
            });
            session.request(&method(8, "completion/complete", &completion)).await;
            let prompt = json!({"name": "analyser_decision", "arguments": {"decision_id": marker}});
            session.request(&method(9, "prompts/get", &prompt)).await;
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{label}: stderr:\n{}", finished.stderr);
            assert!(!finished.stderr.is_empty(), "{label}: LF_LOG=trace logs nothing");
            assert!(
                !finished.stderr.contains(&marker),
                "{label}: the query is in the logs:\n{}",
                finished.stderr
            );
            let audit_text: String =
                audit_lines(audit.path()).iter().map(Value::to_string).collect();
            assert!(!audit_text.is_empty(), "{label}: nothing was audited");
            assert!(
                !audit_text.contains(&marker),
                "{label}: the query is in the audit:\n{audit_text}"
            );
        }
    }
}
