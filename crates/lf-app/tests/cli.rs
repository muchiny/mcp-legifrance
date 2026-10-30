//! Composition réelle du binaire `mcp-legifrance` : configuration, mode dégradé, audit, cache et
//! `User-Agent` (T-M4-07, 06 §2.7).
//!
//! Chaque test lance le processus avec `env_clear()`, un `LF_AUDIT_DIR` temporaire, un faux PISTE
//! (`wiremock`) ou un port où personne n'écoute, et un proxy en échec : le réseau n'est jamais
//! joint hors de la boucle locale. Les journaux de diagnostic sont lus sur stderr, le journal
//! d'audit dans `LF_AUDIT_DIR`.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#![expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]

#[cfg(test)]
mod common;

/// Identifiants absents, configuration invalide : le serveur démarre, répond et dégrade (00 §1).
#[cfg(test)]
mod degraded {
    use serde_json::{Value, json};

    use crate::common::{
        FakePiste, Launch, Trap, audit_lines, call, discover, first_text, is_tool_error,
        modern_call,
    };

    /// S6 (D-39) : sans identifiants, `fr_search` rend `[config_error]` sans aucune E/S réseau (0
    /// requête chez le faux PISTE), avec une ligne d'audit `config_error` ; le serveur sort avec le
    /// code 0 à l'EOF et dit le problème sur stderr.
    #[tokio::test]
    async fn e2e_missing_credentials_config_error_audited() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::silent().await;
        let mut session = Launch::new(audit.path()).piste(&piste).spawn();
        session.handshake().await;

        let search =
            session.request(&call(2, "fr_search", &json!({"query": "bail commercial"}))).await;
        assert!(is_tool_error(&search), "{search}");
        assert!(
            first_text(&search).starts_with("[config_error] PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET"),
            "{search}"
        );
        assert!(search["result"].get("structuredContent").is_none(), "{search}");

        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(piste.requests().await.is_empty(), "a degraded server must not reach PISTE");
        assert!(finished.stderr.contains("PISTE credentials missing"), "{}", finished.stderr);
        assert!(!finished.stderr.contains("bail commercial"), "{}", finished.stderr);

        let lines = audit_lines(audit.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        assert_eq!(line["tool"], "fr_search", "{line}");
        assert_eq!(line["status"], "error", "{line}");
        assert_eq!(line["error_code"], "config_error", "{line}");
        assert_eq!(line["env"], "sandbox", "{line}");
        assert_eq!(line["server_version"], env!("CARGO_PKG_VERSION"), "{line}");
    }

    /// Un cas de configuration PISTE invalide.
    struct Invalid {
        /// Ce que le cas montre, pour les messages d'échec.
        what: &'static str,
        /// Variables posées en plus des identifiants.
        vars: &'static [(&'static str, &'static str)],
    }

    const INVALID_CONFIGURATIONS: [Invalid; 2] = [
        // Valeur PISTE invalide (`InvalidEnv`, 05 §7.2).
        Invalid { what: "invalid LF_PISTE_ENV", vars: &[("LF_PISTE_ENV", "staging")] },
        // Hôtes de production sans `LF_PISTE_ENV` (`EnvMismatch`, 00 §1, AM-27).
        Invalid {
            what: "production hosts without LF_PISTE_ENV",
            vars: &[
                ("LF_PISTE_OAUTH_URL", "https://oauth.piste.gouv.fr/api/oauth/token"),
                ("LF_PISTE_BASE_URL", "https://api.piste.gouv.fr/dila/legifrance/lf-engine-app"),
            ],
        },
    ];

    fn launch_with(case: &Invalid, audit: &std::path::Path, trap: &Trap) -> Launch {
        let mut launch = Launch::new(audit).credentials().proxy(trap);
        for (name, value) in case.vars {
            launch = launch.env(name, value);
        }
        launch
    }

    /// D-39, D-18, AM-27 : une configuration PISTE invalide ne refuse pas le démarrage. Le
    /// processus démarre (`error!` sur stderr), `server/discover` répond, chaque outil PISTE rend
    /// `[config_error] …` sans E/S réseau, `fr_coverage` rend `status: degraded` avec la lacune
    /// FR-104, et `check-config` décrit le problème (code 2).
    #[tokio::test]
    async fn e2e_invalid_config_starts_degraded() {
        for case in &INVALID_CONFIGURATIONS {
            let audit = tempfile::tempdir().expect("a temporary directory");
            // Ni hôte PISTE ni proxy ne doit recevoir une connexion.
            let trap = Trap::bind();
            let launch = launch_with(case, audit.path(), &trap);
            let mut session = launch.spawn();

            let discovered = session.request(&discover(1)).await;
            assert_eq!(
                discovered["result"]["resultType"], "complete",
                "{}: {discovered}",
                case.what
            );

            let search =
                session.request(&modern_call(2, "fr_search", &json!({"query": "bail"}))).await;
            assert!(is_tool_error(&search), "{}: {search}", case.what);
            assert!(first_text(&search).starts_with("[config_error] "), "{}: {search}", case.what);
            assert!(first_text(&search).contains("LF_PISTE_ENV"), "{}: {search}", case.what);

            let coverage = session.request(&modern_call(3, "fr_coverage", &json!({}))).await;
            assert!(!is_tool_error(&coverage), "{}: {coverage}", case.what);
            let view = &coverage["result"]["structuredContent"];
            assert_eq!(view["status"], "degraded", "{}: {coverage}", case.what);
            assert_eq!(view["configuration"]["credentials_configured"], true, "{}", case.what);
            let problems = view["configuration"]["problems"].as_array().expect("problems");
            assert!(!problems.is_empty(), "{}: {coverage}", case.what);
            let gaps: Vec<&Value> = view["gaps"].as_array().expect("gaps").iter().collect();
            assert!(gaps.iter().any(|gap| gap["id"] == "FR-104"), "{}: {coverage}", case.what);

            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{}: stderr:\n{}", case.what, finished.stderr);
            assert!(
                finished.stderr.lines().any(|line| line.contains("ERROR")),
                "{}: no error! on stderr:\n{}",
                case.what,
                finished.stderr
            );
            assert!(!trap.was_contacted(), "{}: the server opened a connection", case.what);

            // `check-config` décrit le problème et sort avec le code 2.
            let report = launch.clone().args(&["check-config"]).run_to_end().await;
            assert_eq!(report.code, Some(2), "{}: {report:?}", case.what);
            assert!(report.stdout.contains("Result: invalid"), "{}: {}", case.what, report.stdout);
            assert!(report.stdout.contains("Problems:"), "{}: {}", case.what, report.stdout);
            assert!(report.stdout.contains("LF_PISTE_ENV"), "{}: {}", case.what, report.stdout);
            let json_report =
                launch.clone().args(&["check-config", "--format", "json"]).run_to_end().await;
            assert_eq!(json_report.code, Some(2), "{}: {json_report:?}", case.what);
            let document: Value = serde_json::from_str(&json_report.stdout).expect("a JSON report");
            assert_eq!(document["valid"], false, "{}: {document}", case.what);
            assert!(
                document["problems"].as_array().is_some_and(|problems| !problems.is_empty()),
                "{}: {document}",
                case.what
            );
            assert!(!trap.was_contacted(), "{}: check-config opened a connection", case.what);
        }
    }
}

/// Composition réelle avec un faux PISTE qui répond : cache et `User-Agent`.
#[cfg(test)]
mod upstream {
    use serde_json::json;

    use crate::common::{
        API_BASE, FakePiste, Launch, TOKEN_PATH, audit_lines, call, is_tool_error,
    };

    /// Les arguments d'une recherche LODA, la même à chaque appel.
    fn loda_search() -> serde_json::Value {
        json!({"query": "republique numerique", "fond": "LODA", "page_size": 5})
    }

    /// D-01 : deux `fr_search` LODA identiques dans la composition réelle (PISTE derrière le
    /// cache) : `/search` n'est reçu qu'une fois, le jeton une fois, et les deux résultats sont
    /// identiques. Chaque appel a sa ligne d'audit, servi par le cache ou non.
    #[tokio::test]
    async fn second_identical_loda_search_hits_cache() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::serving_search_once().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        session.handshake().await;

        let first = session.request(&call(2, "fr_search", &loda_search())).await;
        assert!(!is_tool_error(&first), "{first:.500}");
        let hits = first["result"]["structuredContent"]["hits"].as_array().expect("hits");
        assert!(!hits.is_empty(), "the fixture serves hits: {first:.500}");
        let second = session.request(&call(3, "fr_search", &loda_search())).await;
        assert!(!is_tool_error(&second), "{second:.500}");
        assert_eq!(
            first["result"]["structuredContent"], second["result"]["structuredContent"],
            "the cached answer differs"
        );

        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert_eq!(piste.count(&format!("{API_BASE}/search")).await, 1);
        assert_eq!(piste.count(TOKEN_PATH).await, 1);
        piste.verify().await;
        let lines = audit_lines(audit.path());
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines.iter().all(|line| line["tool"] == "fr_search" && line["status"] == "ok"));
    }

    /// D-43, P-025 : chaque requête vers PISTE (jeton et API) porte un `User-Agent` avec le nom
    /// et la version du binaire (04 §4.2).
    #[tokio::test]
    async fn user_agent_contains_crate_version() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::serving_search().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        session.handshake().await;
        let search = session.request(&call(2, "fr_search", &loda_search())).await;
        assert!(!is_tool_error(&search), "{search:.500}");
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);

        let requests = piste.requests().await;
        assert_eq!(requests.len(), 2, "one token request and one search: {requests:?}");
        let expected = format!("mcp-legifrance/{}", env!("CARGO_PKG_VERSION"));
        for request in &requests {
            let agent = request
                .headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            assert!(
                agent.starts_with(&expected),
                "{} {}: User-Agent is {agent:?}",
                request.method,
                request.url.path()
            );
        }
    }
}

/// Journal d'audit en panne et partagé (Review Focus 4, Q9, S17).
#[cfg(test)]
mod audit {
    use serde_json::json;

    use crate::common::{FakePiste, Launch, Session, audit_lines, call, first_text, is_tool_error};

    /// Texte de l'échec d'audit en mode strict (00 §5), sans code d'origine.
    const AUDIT_ERROR: &str = "[audit_error] audit log unavailable; the call was not recorded and its result is withheld (see server logs).";

    /// Un répertoire en lecture seule ne tient que pour un utilisateur ordinaire : `root` écrit
    /// partout. Rend vrai si l'on peut encore y créer un fichier.
    #[cfg(unix)]
    fn can_still_write(dir: &std::path::Path) -> bool {
        let probe = dir.join("probe");
        let written = std::fs::write(&probe, b"x").is_ok();
        drop(std::fs::remove_file(probe));
        written
    }

    /// Passe `dir` en lecture seule (0o555).
    #[cfg(unix)]
    fn make_read_only(dir: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    }

    /// Rend les droits d'écriture, pour que le dossier temporaire se supprime.
    #[cfg(unix)]
    fn make_writable(dir: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt as _;
        drop(std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)));
    }

    /// Les deux cas du scénario S17 : un répertoire existant en lecture seule, et un chemin non
    /// créable sous un parent en lecture seule. Les droits sont rendus à la fin, même si le test
    /// échoue, pour que le dossier temporaire se supprime.
    #[cfg(unix)]
    struct Unwritable {
        root: tempfile::TempDir,
        /// Le répertoire existant en lecture seule, puis le chemin non créable.
        cases: [std::path::PathBuf; 2],
    }

    #[cfg(unix)]
    impl Unwritable {
        /// `None` quand l'utilisateur peut écrire malgré tout (`root`) : les cas ne s'appliquent pas.
        fn new() -> Option<Self> {
            let root = tempfile::tempdir().expect("a temporary directory");
            let existing = root.path().join("existing");
            std::fs::create_dir(&existing).expect("a directory");
            let parent = root.path().join("parent");
            std::fs::create_dir(&parent).expect("a directory");
            let unwritable = Self { cases: [existing, parent.join("not").join("creatable")], root };
            make_read_only(&unwritable.cases[0]);
            make_read_only(&unwritable.root.path().join("parent"));
            (!can_still_write(&unwritable.cases[0])).then_some(unwritable)
        }
    }

    #[cfg(unix)]
    impl Drop for Unwritable {
        fn drop(&mut self) {
            make_writable(&self.cases[0]);
            make_writable(&self.root.path().join("parent"));
        }
    }

    /// Lance le serveur sur `dir` (mode `mode`), appelle `fr_coverage` puis un appel invalide,
    /// et rend les deux réponses avec ce que le processus a écrit sur stderr.
    #[cfg(unix)]
    async fn two_calls(
        dir: &std::path::Path,
        mode: Option<&str>,
    ) -> (serde_json::Value, serde_json::Value, String) {
        let piste = FakePiste::silent().await;
        let mut launch = Launch::new(dir).credentials().piste(&piste);
        if let Some(mode) = mode {
            launch = launch.env("LF_AUDIT_MODE", mode);
        }
        let mut session = launch.spawn();
        session.handshake().await;
        let coverage = session.request(&call(2, "fr_coverage", &json!({}))).await;
        let invalid =
            session.request(&call(3, "fr_search", &json!({"query": "bail", "page_size": 0}))).await;
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(piste.requests().await.is_empty());
        (coverage, invalid, finished.stderr)
    }

    /// P-043, Q9, S17 : en mode `strict` (le défaut), un journal inscriptible nulle part donne
    /// `[audit_error]` à la place du résultat, pour toutes les issues (succès et `invalid_arg`),
    /// dans les deux cas du scénario : répertoire existant en lecture seule, et chemin non créable
    /// sous un parent en lecture seule. Le résultat est écarté (ni `structuredContent`, ni texte
    /// de l'outil), l'erreur d'origine est citée et le détail ne sort que sur stderr.
    #[cfg(unix)]
    #[tokio::test]
    async fn audit_unwritable_dir_fails_call() {
        // `root` écrit partout : les cas de lecture seule ne s'appliquent pas.
        let Some(unwritable) = Unwritable::new() else { return };
        let dirs = &unwritable.cases;
        for dir in dirs {
            let (coverage, invalid, stderr) = two_calls(dir, None).await;
            assert!(is_tool_error(&coverage), "{dir:?}: {coverage}");
            assert_eq!(first_text(&coverage), AUDIT_ERROR, "{dir:?}");
            assert!(coverage["result"].get("structuredContent").is_none(), "{dir:?}: {coverage}");
            assert!(is_tool_error(&invalid), "{dir:?}: {invalid}");
            assert_eq!(
                first_text(&invalid),
                format!("{AUDIT_ERROR} Original error: [invalid_arg].")
            );
            assert!(stderr.contains("audit write failed"), "{dir:?}: {stderr}");
            // Le détail (chemin local compris) ne va jamais au modèle.
            assert!(!first_text(&coverage).contains("denied"), "{dir:?}");
        }
        // Rien n'a été créé : ni fichier dans le répertoire en lecture seule, ni chemin sous le parent.
        assert_eq!(std::fs::read_dir(&dirs[0]).expect("a directory").count(), 0);
        assert!(!dirs[1].exists());
    }

    /// P-043, Q9, S17 : en mode `best-effort`, dans les mêmes deux cas, le résultat est rendu et
    /// l'échec n'est signalé que par un `warn!` sur stderr.
    #[cfg(unix)]
    #[tokio::test]
    async fn best_effort_unwritable_dir_returns_result() {
        let Some(unwritable) = Unwritable::new() else { return };
        for dir in &unwritable.cases {
            let (coverage, invalid, stderr) = two_calls(dir, Some("best-effort")).await;
            assert!(!is_tool_error(&coverage), "{dir:?}: {coverage}");
            assert!(coverage["result"]["structuredContent"].is_object(), "{dir:?}: {coverage}");
            assert!(is_tool_error(&invalid), "{dir:?}: {invalid}");
            assert!(first_text(&invalid).starts_with("[invalid_arg] "), "{dir:?}: {invalid}");
            assert!(stderr.contains("audit write failed"), "{dir:?}: {stderr}");
        }
    }

    /// Nombre d'appels par processus dans `two_processes_share_audit_file`.
    const CALLS_PER_PROCESS: u64 = 40;

    /// Envoie `CALLS_PER_PROCESS` appels d'affilée sans attendre, puis lit les réponses.
    async fn burst(session: &mut Session, first_id: u64) {
        for index in 0..CALLS_PER_PROCESS {
            session.send(&call(first_id + index, "fr_coverage", &json!({}))).await;
        }
        let mut answered = 0;
        while answered < CALLS_PER_PROCESS {
            let response = session.recv().await;
            assert!(!is_tool_error(&response), "{response}");
            answered += 1;
        }
    }

    /// D-16, Review Focus 4 : deux binaires appelés en parallèle sur le même `LF_AUDIT_DIR` : toutes
    /// les lignes sont complètes et lisibles, aucune n'est coupée ni perdue (le verrou `fs4` couvre
    /// l'écriture).
    #[tokio::test]
    async fn two_processes_share_audit_file() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::silent().await;
        let launch = Launch::new(audit.path()).credentials().piste(&piste);
        let mut first = launch.spawn();
        let mut second = launch.spawn();
        let (first_ready, second_ready) = tokio::join!(first.handshake(), second.handshake());
        assert!(first_ready["result"].is_object() && second_ready["result"].is_object());

        tokio::join!(burst(&mut first, 100), burst(&mut second, 1000));
        let (first_done, second_done) = tokio::join!(first.finish(), second.finish());
        assert_eq!(first_done.code(), Some(0), "stderr:\n{}", first_done.stderr);
        assert_eq!(second_done.code(), Some(0), "stderr:\n{}", second_done.stderr);

        // `audit_lines` échoue sur une ligne qui n'est pas du JSON ou sur un fichier qui ne finit
        // pas par un saut de ligne.
        let lines = audit_lines(audit.path());
        let expected = usize::try_from(2 * CALLS_PER_PROCESS).expect("a small count");
        assert_eq!(lines.len(), expected, "lines were lost or torn");
        for line in &lines {
            assert_eq!(line["tool"], "fr_coverage", "{line}");
            assert_eq!(line["status"], "ok", "{line}");
            assert_eq!(line["server_version"], env!("CARGO_PKG_VERSION"), "{line}");
            assert_eq!(line["input_hash"].as_str().map(str::len), Some(64), "{line}");
        }
        assert!(piste.requests().await.is_empty());
    }
}
