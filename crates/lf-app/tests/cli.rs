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

/// Doctrine fiscale (BOFiP, jalon MB) dans la composition réelle : coupée, la source n'envoie
/// rien ; active, elle n'ouvre qu'une connexion, vers le seul hôte du portail (G4, G5, G9 du plan,
/// ADR-0029).
///
/// Le binaire vise une adresse fixe, sans variable de surcharge : aucun faux portail ne peut le
/// servir. Le chemin de succès des deux outils est prouvé ailleurs (`FakeDoctrine` dans
/// `lf-adapter-mcp`, faux serveurs dans `lf-adapter-bofip`). Ici, le proxy du processus est un
/// piège qui ne répond jamais : il compte les connexions et lit l'hôte qu'elles demandaient.
#[cfg(test)]
mod bofip {
    use std::path::Path;

    use serde_json::{Value, json};

    use crate::common::{Launch, Session, Trap, audit_lines, call, first_text, is_tool_error};

    /// Le texte fixe d'un outil du BOFiP quand la source est coupée.
    const DISABLED: &str = "[config_error] the BOFiP source is turned off (LF_BOFIP is off or \
        invalid, or the server runs offline with LF_DILA_DIR): fr_search_bofip and fr_get_bofip \
        are unavailable. The operator can turn it on with LF_BOFIP=on; search words are then sent \
        to the DGFiP open data portal.";

    /// Mot cherché que rien ne doit recopier : ni journal, ni message d'erreur.
    const SENTINEL: &str = "zzsentinellezz";

    /// La seule destination admise d'une connexion du BOFiP, telle qu'un proxy la reçoit.
    const PORTAL_TUNNEL: &str = "CONNECT data.economie.gouv.fr:443 HTTP/1.1";

    const BOI: &str = "BOI-ENR-DMTG-10-50-20";

    /// Une façon de couper la source.
    struct Off {
        /// Ce que le cas montre, pour les messages d'échec.
        what: &'static str,
        /// Variable posée en plus des identifiants.
        var: (&'static str, &'static str),
    }

    const OFF: [Off; 4] = [
        Off { what: "LF_BOFIP=off", var: ("LF_BOFIP", "off") },
        Off { what: "LF_BOFIP=OFF", var: ("LF_BOFIP", "OFF") },
        // Échec fermé : un refus mal écrit coupe la source au lieu de la laisser active.
        Off { what: "invalid LF_BOFIP", var: ("LF_BOFIP", "false") },
        // Mode hors ligne : `LF_DILA_DIR` défini, même inutilisable.
        Off { what: "unusable LF_DILA_DIR", var: ("LF_DILA_DIR", "relative/index") },
    ];

    /// Lancement dont le proxy est `trap`, délai par requête d'une seconde : une requête qui
    /// partirait arriverait au piège, et l'appel se terminerait vite.
    fn launch(audit: &Path, trap: &Trap) -> Launch {
        Launch::new(audit).credentials().proxy(trap).env("LF_HTTP_TIMEOUT_S", "1")
    }

    /// Appelle les deux outils du BOFiP et rend leurs réponses.
    async fn call_both(session: &mut Session) -> [Value; 2] {
        let query = format!("{SENTINEL} donation");
        let search = session.request(&call(2, "fr_search_bofip", &json!({"query": query}))).await;
        let get = session.request(&call(3, "fr_get_bofip", &json!({"id": BOI}))).await;
        [search, get]
    }

    /// G4 et G5 : source coupée (demande, valeur invalide, mode hors ligne), les deux outils
    /// répondent le texte fixe, audité, sans qu'aucune connexion ne s'ouvre. Contraste, sans
    /// lequel un piège muet ne prouverait rien : source active, chaque appel ouvre exactement une
    /// connexion, vers `data.economie.gouv.fr:443`, et son erreur ne cite ni l'hôte ni les mots.
    #[tokio::test]
    async fn offline_and_off_send_nothing_through_proxy_witness() {
        for case in &OFF {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let (name, value) = case.var;
            let mut session = launch(audit.path(), &trap).env(name, value).spawn();
            session.handshake().await;
            for answer in call_both(&mut session).await {
                assert!(is_tool_error(&answer), "{}: {answer}", case.what);
                assert_eq!(first_text(&answer), DISABLED, "{}", case.what);
            }
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{}: stderr:\n{}", case.what, finished.stderr);
            assert_eq!(trap.contacts(), Vec::<String>::new(), "{}", case.what);
            assert!(!finished.stderr.contains(SENTINEL), "{}: {}", case.what, finished.stderr);

            let lines = audit_lines(audit.path());
            let audited: Vec<(&Value, &Value, &Value)> = lines
                .iter()
                .map(|line| (&line["tool"], &line["status"], &line["error_code"]))
                .collect();
            assert_eq!(
                audited,
                [
                    (&json!("fr_search_bofip"), &json!("error"), &json!("config_error")),
                    (&json!("fr_get_bofip"), &json!("error"), &json!("config_error")),
                ],
                "{}",
                case.what
            );
        }

        // Contraste : sans aucun réglage, la source est active et joint le portail, par le proxy.
        // Rejoué au niveau de journal le plus bavard : les bibliothèques HTTP et TLS écrivent
        // par `log`, que le binaire relaie vers stderr, et ne doivent pas y citer les mots.
        for log in [None, Some("trace")] {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let mut launch = launch(audit.path(), &trap);
            if let Some(level) = log {
                launch = launch.env("LF_LOG", level);
            }
            let mut session = launch.spawn();
            session.handshake().await;
            let answers = call_both(&mut session).await;
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{log:?}: stderr:\n{}", finished.stderr);
            for answer in &answers {
                assert!(is_tool_error(answer), "{log:?}: {answer}");
                let text = first_text(answer);
                assert!(text.starts_with("[upstream_error] "), "{log:?}: {text}");
                assert!(!text.contains(SENTINEL) && !text.contains("economie"), "{log:?}: {text}");
            }
            // Une connexion par appel, jamais de nouvelle tentative, et un seul hôte.
            assert_eq!(trap.contacts(), [PORTAL_TUNNEL, PORTAL_TUNNEL], "{log:?}");
            assert!(!finished.stderr.contains(SENTINEL), "{log:?}: {}", finished.stderr);
            // Le niveau demandé est bien en vigueur : sans cela, l'absence du mot ne prouverait
            // rien.
            assert_eq!(finished.stderr.contains("TRACE"), log.is_some(), "{log:?}");
        }
    }

    /// G5 et G9 : `check-config` et `fr_coverage` n'envoient rien, que la source soit active ou
    /// coupée. Sans `LF_BOFIP`, la sortie de `check-config` ne nomme pas la variable ;
    /// `fr_coverage` rend la famille `BOFIP`, avant celle de Service-Public, et la lacune de
    /// l'état de la source.
    #[tokio::test]
    async fn check_config_and_coverage_send_nothing() {
        for (vars, line, gap) in [
            (&[][..], None, "FR-106"),
            (&[("LF_BOFIP", "on")][..], Some(["LF_BOFIP", "LF_BOFIP", "on", "ok"]), "FR-106"),
            (&[("LF_BOFIP", "off")][..], Some(["LF_BOFIP", "LF_BOFIP", "off", "ok"]), "FR-107"),
        ] {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let mut launch = launch(audit.path(), &trap);
            for (name, value) in vars {
                launch = launch.env(name, value);
            }

            let report = launch.clone().args(&["check-config"]).run_to_end().await;
            assert_eq!(report.code, Some(0), "{vars:?}: {report:?}");
            assert!(report.stdout.contains("Result: valid"), "{vars:?}: {}", report.stdout);
            let shown: Option<Vec<&str>> = report
                .stdout
                .lines()
                .find(|row| row.starts_with("LF_BOFIP"))
                .map(|row| row.split_whitespace().collect());
            assert_eq!(shown, line.map(Vec::from), "{vars:?}: {}", report.stdout);

            let mut session = launch.spawn();
            session.handshake().await;
            let coverage = session.request(&call(2, "fr_coverage", &json!({}))).await;
            assert!(!is_tool_error(&coverage), "{vars:?}: {coverage}");
            let view = &coverage["result"]["structuredContent"];
            // La source du BOFiP ne change jamais le statut : il reste celui de PISTE.
            assert_eq!(view["status"], "ok", "{vars:?}: {coverage}");
            // Les familles du BOFiP, de Service-Public puis de Judilibre ferment la liste
            // (05 §5.8).
            let families = view["families"].as_array().expect("families");
            let tools: Vec<&Value> = families.iter().rev().take(3).map(|f| &f["tools"]).collect();
            assert_eq!(
                tools,
                [
                    &json!(["fr_search_judilibre", "fr_get_judilibre"]),
                    &json!(["fr_search_service_public", "fr_get_service_public"]),
                    &json!(["fr_search_bofip", "fr_get_bofip"]),
                ],
                "{vars:?}: {coverage}"
            );
            let gaps = view["gaps"].as_array().expect("gaps");
            let bofip: Vec<&Value> =
                gaps.iter().filter(|g| g["fond"] == "BOFIP").map(|g| &g["id"]).collect();
            assert_eq!(bofip, [&json!(gap)], "{vars:?}");
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{vars:?}: stderr:\n{}", finished.stderr);
            assert_eq!(trap.contacts(), Vec::<String>::new(), "{vars:?}");
        }
    }

    /// Échec fermé (ADR-0029, décision 8) : `LF_BOFIP` ni `on` ni `off` coupe la source. Le
    /// serveur démarre et le dit (`error!` sur stderr, problème de `check-config`, code 2), sans
    /// recopier la valeur ; PISTE n'est pas dégradé pour autant.
    #[tokio::test]
    async fn invalid_lf_bofip_turns_source_off() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let launch = launch(audit.path(), &trap).env("LF_BOFIP", SENTINEL);

        let report = launch.clone().args(&["check-config"]).run_to_end().await;
        assert_eq!(report.code, Some(2), "{report:?}");
        assert!(report.stdout.contains("Result: invalid"), "{}", report.stdout);
        assert!(
            report
                .stdout
                .contains("  - LF_BOFIP must be on or off: the BOFiP source is turned off"),
            "{}",
            report.stdout
        );
        let row = report.stdout.lines().find(|row| row.starts_with("LF_BOFIP")).expect("a row");
        assert_eq!(
            row.split_whitespace().collect::<Vec<_>>(),
            ["LF_BOFIP", "LF_BOFIP", "off", "(invalid", "value)", "problem"]
        );
        assert!(!report.stdout.contains(SENTINEL) && !report.stderr.contains(SENTINEL));

        let mut session = launch.spawn();
        session.handshake().await;
        for answer in call_both(&mut session).await {
            assert_eq!(first_text(&answer), DISABLED);
        }
        let coverage = session.request(&call(4, "fr_coverage", &json!({}))).await;
        let view = &coverage["result"]["structuredContent"];
        assert_eq!(view["status"], "ok", "{coverage}");
        assert!(view["configuration"].get("problems").is_none(), "{coverage}");
        let bofip: Vec<&Value> = view["gaps"]
            .as_array()
            .expect("gaps")
            .iter()
            .filter(|gap| gap["fond"] == "BOFIP")
            .map(|gap| &gap["id"])
            .collect();
        assert_eq!(bofip, [&json!("FR-107")], "{coverage}");
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(
            finished
                .stderr
                .lines()
                .any(|line| line.contains("ERROR") && line.contains("LF_BOFIP must be on or off")),
            "{}",
            finished.stderr
        );
        assert!(!finished.stderr.contains(SENTINEL), "{}", finished.stderr);
        assert_eq!(trap.contacts(), Vec::<String>::new());
    }

    /// Mode hors ligne : `LF_DILA_DIR` défini coupe la source d'office, même quand il est
    /// inutilisable, et `check-config` le montre. Seul `LF_BOFIP=on` la rétablit : la source
    /// joint alors le portail, alors que les textes de Légifrance restent indisponibles.
    #[tokio::test]
    async fn unusable_dila_dir_turns_source_off() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let offline = launch(audit.path(), &trap).env("LF_DILA_DIR", "relative/index");

        let report = offline.clone().args(&["check-config"]).run_to_end().await;
        let row = report.stdout.lines().find(|row| row.starts_with("LF_BOFIP")).expect("a row");
        assert_eq!(
            row.split_whitespace().collect::<Vec<_>>(),
            ["LF_BOFIP", "default", "off", "(LF_DILA_DIR", "is", "set)", "ok"]
        );
        // Le seul problème est celui de `LF_DILA_DIR` : la coupure d'office n'en est pas un.
        assert_eq!(report.code, Some(2), "{report:?}");
        assert!(!report.stdout.contains("LF_BOFIP must be"), "{}", report.stdout);
        assert_eq!(trap.contacts(), Vec::<String>::new());

        // `LF_BOFIP=on` explicite : la source revient, et elle seule joint le réseau.
        let mut session = offline.env("LF_BOFIP", "on").spawn();
        session.handshake().await;
        let [search, get] = call_both(&mut session).await;
        for answer in [&search, &get] {
            assert!(first_text(answer).starts_with("[upstream_error] "), "{answer}");
        }
        let legifrance = session.request(&call(4, "fr_search", &json!({"query": "bail"}))).await;
        assert!(
            first_text(&legifrance).starts_with("[config_error] LF_DILA_DIR must be an absolute"),
            "{legifrance}"
        );
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert_eq!(trap.contacts(), [PORTAL_TUNNEL, PORTAL_TUNNEL]);
    }
}

/// Jalon MS : la source Service-Public dans le binaire (T-MS-34 ; ADR-0033, « Tests qui la
/// gardent »). Le proxy témoin ne voit que `CONNECT hôte:443` et ne répond jamais : que les
/// requêtes ne portent aucun mot ni numéro (G10) est prouvé sur faux serveur, dans l'adapter.
#[cfg(test)]
mod service_public {
    use std::path::Path;

    use serde_json::{Value, json};

    use crate::common::{Launch, Session, Trap, audit_lines, call, first_text, is_tool_error};

    /// Le texte fixe d'un outil de Service-Public quand la source est coupée.
    const DISABLED: &str = "[config_error] the Service-Public source is turned off \
        (LF_SERVICE_PUBLIC is off or invalid, or the server runs offline with LF_DILA_DIR): \
        fr_search_service_public and fr_get_service_public are unavailable. The operator can turn \
        it on with LF_SERVICE_PUBLIC=on; the day's archive is then downloaded from the DILA \
        server, and neither search words nor sheet numbers are sent.";

    /// Réponse d'un appel dont l'attente s'achève pendant le téléchargement.
    const DOWNLOADING: &str =
        "[upstream_error] Service-Public archive is still downloading; retry in a minute.";

    /// Mot cherché que rien ne doit recopier : ni journal, ni message d'erreur.
    const SENTINEL: &str = "zzsentinellezz";

    /// La seule destination admise d'une connexion de Service-Public, telle qu'un proxy la reçoit.
    const ARCHIVE_TUNNEL: &str = "CONNECT lecomarquage.service-public.gouv.fr:443 HTTP/1.1";

    /// Une façon de couper la source.
    struct Off {
        /// Ce que le cas montre, pour les messages d'échec.
        what: &'static str,
        /// Variable posée en plus des identifiants.
        var: (&'static str, &'static str),
    }

    const OFF: [Off; 4] = [
        Off { what: "LF_SERVICE_PUBLIC=off", var: ("LF_SERVICE_PUBLIC", "off") },
        Off { what: "LF_SERVICE_PUBLIC=OFF", var: ("LF_SERVICE_PUBLIC", "OFF") },
        // Échec fermé : un refus mal écrit coupe la source au lieu de la laisser active.
        Off { what: "invalid LF_SERVICE_PUBLIC", var: ("LF_SERVICE_PUBLIC", "false") },
        // Mode hors ligne : `LF_DILA_DIR` défini, même inutilisable.
        Off { what: "unusable LF_DILA_DIR", var: ("LF_DILA_DIR", "relative/index") },
    ];

    /// Lancement dont le proxy est `trap`, échéance de 2 s par appel. Seule l'échéance borne
    /// l'attente du téléchargement (ADR-0033) : un appel attend 1 s (l'échéance moins sa marge),
    /// le temps que la connexion atteigne le piège, puis répond que l'archive se télécharge.
    fn launch(audit: &Path, trap: &Trap) -> Launch {
        Launch::new(audit).credentials().proxy(trap).env("LF_TOOL_DEADLINE_S", "2")
    }

    /// Appelle les deux outils de Service-Public, sur l'archive de Particuliers, et rend leurs
    /// réponses.
    async fn call_both(session: &mut Session) -> [Value; 2] {
        let query = format!("{SENTINEL} logement");
        let search =
            session.request(&call(2, "fr_search_service_public", &json!({"query": query}))).await;
        let get = session.request(&call(3, "fr_get_service_public", &json!({"id": "F1437"}))).await;
        [search, get]
    }

    /// Les lacunes de `fr_coverage` du fonds `SERVICE_PUBLIC`.
    fn service_public_gaps(view: &Value) -> Vec<&Value> {
        view["gaps"]
            .as_array()
            .expect("gaps")
            .iter()
            .filter(|gap| gap["fond"] == "SERVICE_PUBLIC")
            .map(|gap| &gap["id"])
            .collect()
    }

    /// G4 et G5 : source coupée (demande, valeur invalide, mode hors ligne), les deux outils
    /// répondent le texte fixe, audité, sans qu'aucune connexion ne s'ouvre. Contraste, sans
    /// lequel un piège muet ne prouverait rien : source active, les deux appels partagent un seul
    /// téléchargement, donc une seule connexion, vers `lecomarquage.service-public.gouv.fr:443`,
    /// et leur réponse ne cite ni l'hôte ni les mots.
    #[tokio::test]
    async fn offline_and_off_send_nothing_through_proxy_witness() {
        for case in &OFF {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let (name, value) = case.var;
            let mut session = launch(audit.path(), &trap).env(name, value).spawn();
            session.handshake().await;
            for answer in call_both(&mut session).await {
                assert!(is_tool_error(&answer), "{}: {answer}", case.what);
                assert_eq!(first_text(&answer), DISABLED, "{}", case.what);
            }
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{}: stderr:\n{}", case.what, finished.stderr);
            assert_eq!(trap.contacts(), Vec::<String>::new(), "{}", case.what);
            assert!(!finished.stderr.contains(SENTINEL), "{}: {}", case.what, finished.stderr);

            let lines = audit_lines(audit.path());
            let audited: Vec<(&Value, &Value, &Value)> = lines
                .iter()
                .map(|line| (&line["tool"], &line["status"], &line["error_code"]))
                .collect();
            assert_eq!(
                audited,
                [
                    (&json!("fr_search_service_public"), &json!("error"), &json!("config_error")),
                    (&json!("fr_get_service_public"), &json!("error"), &json!("config_error")),
                ],
                "{}",
                case.what
            );
        }

        // Contraste : sans aucun réglage, la source est active et télécharge l'archive, par le
        // proxy. Rejoué au niveau de journal le plus bavard : les bibliothèques HTTP et TLS
        // écrivent par `log`, que le binaire relaie vers stderr, et ne doivent pas y citer les
        // mots.
        for log in [None, Some("trace")] {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let mut launch = launch(audit.path(), &trap);
            if let Some(level) = log {
                launch = launch.env("LF_LOG", level);
            }
            let mut session = launch.spawn();
            session.handshake().await;
            let answers = call_both(&mut session).await;
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{log:?}: stderr:\n{}", finished.stderr);
            for answer in &answers {
                assert!(is_tool_error(answer), "{log:?}: {answer}");
                assert_eq!(first_text(answer), DOWNLOADING, "{log:?}");
            }
            // Un seul téléchargement pour les deux appels, et un seul hôte.
            assert_eq!(trap.contacts(), [ARCHIVE_TUNNEL], "{log:?}");
            assert!(!finished.stderr.contains(SENTINEL), "{log:?}: {}", finished.stderr);
            // Le niveau demandé est bien en vigueur : sans cela, l'absence du mot ne prouverait
            // rien.
            assert_eq!(finished.stderr.contains("TRACE"), log.is_some(), "{log:?}");
        }
    }

    /// G5 et G9 : `check-config` et `fr_coverage` n'envoient rien, que la source soit active ou
    /// coupée. Sans `LF_SERVICE_PUBLIC`, la sortie de `check-config` ne nomme pas la variable ;
    /// `fr_coverage` rend la famille `SERVICE_PUBLIC`, l'avant-dernière (avant celle de
    /// Judilibre), et la lacune de l'état de la source.
    #[tokio::test]
    async fn check_config_and_coverage_send_nothing() {
        let on = ["LF_SERVICE_PUBLIC", "LF_SERVICE_PUBLIC", "on", "ok"];
        let off = ["LF_SERVICE_PUBLIC", "LF_SERVICE_PUBLIC", "off", "ok"];
        for (vars, line, gap) in [
            (&[][..], None, "FR-108"),
            (&[("LF_SERVICE_PUBLIC", "on")][..], Some(on), "FR-108"),
            (&[("LF_SERVICE_PUBLIC", "off")][..], Some(off), "FR-109"),
        ] {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let mut launch = launch(audit.path(), &trap);
            for (name, value) in vars {
                launch = launch.env(name, value);
            }

            let report = launch.clone().args(&["check-config"]).run_to_end().await;
            assert_eq!(report.code, Some(0), "{vars:?}: {report:?}");
            assert!(report.stdout.contains("Result: valid"), "{vars:?}: {}", report.stdout);
            let shown: Option<Vec<&str>> = report
                .stdout
                .lines()
                .find(|row| row.starts_with("LF_SERVICE_PUBLIC"))
                .map(|row| row.split_whitespace().collect());
            assert_eq!(shown, line.map(Vec::from), "{vars:?}: {}", report.stdout);

            let mut session = launch.spawn();
            session.handshake().await;
            let coverage = session.request(&call(2, "fr_coverage", &json!({}))).await;
            assert!(!is_tool_error(&coverage), "{vars:?}: {coverage}");
            let view = &coverage["result"]["structuredContent"];
            // La source de Service-Public ne change jamais le statut : il reste celui de PISTE.
            assert_eq!(view["status"], "ok", "{vars:?}: {coverage}");
            let families = view["families"].as_array().expect("families");
            assert_eq!(
                families.iter().rev().nth(1).expect("a family")["tools"],
                json!(["fr_search_service_public", "fr_get_service_public"]),
                "{vars:?}: {coverage}"
            );
            assert_eq!(service_public_gaps(view), [&json!(gap)], "{vars:?}");
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{vars:?}: stderr:\n{}", finished.stderr);
            assert_eq!(trap.contacts(), Vec::<String>::new(), "{vars:?}");
        }
    }

    /// Échec fermé (ADR-0033, décision 10) : `LF_SERVICE_PUBLIC` ni `on` ni `off` coupe la
    /// source. Le serveur démarre et le dit (`error!` sur stderr, problème de `check-config`,
    /// code 2), sans recopier la valeur ; PISTE n'est pas dégradé pour autant.
    #[tokio::test]
    async fn invalid_lf_service_public_turns_source_off() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let launch = launch(audit.path(), &trap).env("LF_SERVICE_PUBLIC", SENTINEL);

        let report = launch.clone().args(&["check-config"]).run_to_end().await;
        assert_eq!(report.code, Some(2), "{report:?}");
        assert!(report.stdout.contains("Result: invalid"), "{}", report.stdout);
        let problem = "  - LF_SERVICE_PUBLIC must be on or off: the Service-Public source is \
                       turned off";
        assert!(report.stdout.contains(problem), "{}", report.stdout);
        let row =
            report.stdout.lines().find(|row| row.starts_with("LF_SERVICE_PUBLIC")).expect("a row");
        assert_eq!(
            row.split_whitespace().collect::<Vec<_>>(),
            ["LF_SERVICE_PUBLIC", "LF_SERVICE_PUBLIC", "off", "(invalid", "value)", "problem"]
        );
        assert!(!report.stdout.contains(SENTINEL) && !report.stderr.contains(SENTINEL));

        let mut session = launch.spawn();
        session.handshake().await;
        for answer in call_both(&mut session).await {
            assert_eq!(first_text(&answer), DISABLED);
        }
        let coverage = session.request(&call(4, "fr_coverage", &json!({}))).await;
        let view = &coverage["result"]["structuredContent"];
        assert_eq!(view["status"], "ok", "{coverage}");
        assert!(view["configuration"].get("problems").is_none(), "{coverage}");
        assert_eq!(service_public_gaps(view), [&json!("FR-109")], "{coverage}");
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(
            finished.stderr.lines().any(|line| {
                line.contains("ERROR") && line.contains("LF_SERVICE_PUBLIC must be on or off")
            }),
            "{}",
            finished.stderr
        );
        assert!(!finished.stderr.contains(SENTINEL), "{}", finished.stderr);
        assert_eq!(trap.contacts(), Vec::<String>::new());
    }

    /// Mode hors ligne : `LF_DILA_DIR` défini coupe la source d'office, même quand il est
    /// inutilisable, et `check-config` le montre. Seul `LF_SERVICE_PUBLIC=on` la rétablit : la
    /// source télécharge alors l'archive, alors que les textes de Légifrance restent
    /// indisponibles et que le BOFiP reste coupé.
    #[tokio::test]
    async fn unusable_dila_dir_turns_source_off() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let offline = launch(audit.path(), &trap).env("LF_DILA_DIR", "relative/index");

        let report = offline.clone().args(&["check-config"]).run_to_end().await;
        let row =
            report.stdout.lines().find(|row| row.starts_with("LF_SERVICE_PUBLIC")).expect("a row");
        assert_eq!(
            row.split_whitespace().collect::<Vec<_>>(),
            ["LF_SERVICE_PUBLIC", "default", "off", "(LF_DILA_DIR", "is", "set)", "ok"]
        );
        // Le seul problème est celui de `LF_DILA_DIR` : la coupure d'office n'en est pas un.
        assert_eq!(report.code, Some(2), "{report:?}");
        assert!(!report.stdout.contains("LF_SERVICE_PUBLIC must be"), "{}", report.stdout);
        assert_eq!(trap.contacts(), Vec::<String>::new());

        // `LF_SERVICE_PUBLIC=on` explicite : la source revient, et elle seule joint le réseau.
        let mut session = offline.env("LF_SERVICE_PUBLIC", "on").spawn();
        session.handshake().await;
        for answer in call_both(&mut session).await {
            assert_eq!(first_text(&answer), DOWNLOADING, "{answer}");
        }
        let bofip = session.request(&call(4, "fr_search_bofip", &json!({"query": "don"}))).await;
        assert!(
            first_text(&bofip).starts_with("[config_error] the BOFiP source is turned off"),
            "{bofip}"
        );
        let legifrance = session.request(&call(5, "fr_search", &json!({"query": "bail"}))).await;
        assert!(
            first_text(&legifrance).starts_with("[config_error] LF_DILA_DIR must be an absolute"),
            "{legifrance}"
        );
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert_eq!(trap.contacts(), [ARCHIVE_TUNNEL]);
    }
}

/// Jalon MJ : la source Judilibre dans le binaire (T-MJ-34 ; ADR-0035, « Tests qui la gardent »).
///
/// Deux témoins. Le proxy piège ne voit que `CONNECT hôte:443` et ne répond jamais : coupée, la
/// source n'ouvre aucune connexion ; active, l'appel part vers PISTE. Le faux PISTE (`wiremock` sur
/// la boucle locale) sert Légifrance et Judilibre sur le même hôte : il montre l'adresse de chaque
/// requête (G10) et le jeton unique des deux API (G12).
#[cfg(test)]
mod judilibre {
    use std::path::Path;

    use serde_json::{Value, json};

    use crate::common::{
        API_BASE, FAKE_TOKEN, FakePiste, JUDILIBRE_DECISION, JUDILIBRE_SEARCH, Launch, Session,
        TOKEN_PATH, Trap, audit_lines, call, first_text, is_tool_error,
    };

    /// Le texte fixe d'un outil de Judilibre quand la source est coupée.
    const DISABLED: &str = "[config_error] the Judilibre source is turned off (LF_JUDILIBRE is \
        off or invalid, or the server runs offline with LF_DILA_DIR): fr_search_judilibre and \
        fr_get_judilibre are unavailable. The operator can turn it on with LF_JUDILIBRE=on; search \
        words are then sent to the Judilibre API through PISTE.";

    /// Mot cherché que rien ne doit recopier : ni journal, ni message d'erreur.
    const SENTINEL: &str = "zzsentinellezz";

    /// Décision de la fixture `decision-cc.json`, première de `search-cc.json`.
    const DECISION: &str = "5fca624b7eeb384a40cafb85";

    /// Première connexion d'un appel à PISTE (bac à sable), telle qu'un proxy la reçoit : le jeton.
    const OAUTH_TUNNEL: &str = "CONNECT sandbox-oauth.piste.gouv.fr:443 HTTP/1.1";

    /// Paramètres admis d'une recherche (ADR-0035, décision 3).
    const SEARCH_PARAMS: [&str; 11] = [
        "query",
        "operator",
        "field",
        "jurisdiction",
        "date_start",
        "date_end",
        "sort",
        "order",
        "page",
        "page_size",
        "resolve_references",
    ];

    /// Une façon de couper la source.
    struct Off {
        /// Ce que le cas montre, pour les messages d'échec.
        what: &'static str,
        /// Variable posée en plus des identifiants.
        var: (&'static str, &'static str),
    }

    const OFF: [Off; 4] = [
        Off { what: "LF_JUDILIBRE=off", var: ("LF_JUDILIBRE", "off") },
        Off { what: "LF_JUDILIBRE=OFF", var: ("LF_JUDILIBRE", "OFF") },
        // Échec fermé : un refus mal écrit coupe la source au lieu de la laisser active.
        Off { what: "invalid LF_JUDILIBRE", var: ("LF_JUDILIBRE", "false") },
        // Mode hors ligne : `LF_DILA_DIR` défini, même inutilisable.
        Off { what: "unusable LF_DILA_DIR", var: ("LF_DILA_DIR", "relative/index") },
    ];

    /// Lancement vers les hôtes officiels du bac à sable, identifiants factices, proxy `trap`,
    /// échéance d'une seconde par appel : une requête qui partirait arriverait au piège, et
    /// l'appel se terminerait vite.
    fn launch(audit: &Path, trap: &Trap) -> Launch {
        Launch::new(audit).credentials().proxy(trap).env("LF_TOOL_DEADLINE_S", "1")
    }

    /// Appelle les deux outils de Judilibre et rend leurs réponses.
    async fn call_both(session: &mut Session) -> [Value; 2] {
        let query = format!("{SENTINEL} donation");
        let search =
            session.request(&call(2, "fr_search_judilibre", &json!({"query": query}))).await;
        let get = session.request(&call(3, "fr_get_judilibre", &json!({"id": DECISION}))).await;
        [search, get]
    }

    /// Les lacunes de `fr_coverage` du fonds `JUDILIBRE`.
    fn judilibre_gaps(view: &Value) -> Vec<&Value> {
        view["gaps"]
            .as_array()
            .expect("gaps")
            .iter()
            .filter(|gap| gap["fond"] == "JUDILIBRE")
            .map(|gap| &gap["id"])
            .collect()
    }

    /// G4 et G5 : source coupée (demande, valeur invalide, mode hors ligne), les deux outils
    /// répondent le texte fixe, audité, sans qu'aucune connexion ne s'ouvre. Contraste, sans
    /// lequel un piège muet ne prouverait rien : source active, l'appel part vers PISTE (la
    /// demande de jeton), et sa réponse ne cite pas les mots.
    #[tokio::test]
    async fn offline_and_off_send_nothing_through_proxy_witness() {
        for case in &OFF {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let (name, value) = case.var;
            let mut session = launch(audit.path(), &trap).env(name, value).spawn();
            session.handshake().await;
            for answer in call_both(&mut session).await {
                assert!(is_tool_error(&answer), "{}: {answer}", case.what);
                assert_eq!(first_text(&answer), DISABLED, "{}", case.what);
            }
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{}: stderr:\n{}", case.what, finished.stderr);
            assert_eq!(trap.contacts(), Vec::<String>::new(), "{}", case.what);
            assert!(!finished.stderr.contains(SENTINEL), "{}: {}", case.what, finished.stderr);

            let lines = audit_lines(audit.path());
            let audited: Vec<(&Value, &Value, &Value)> = lines
                .iter()
                .map(|line| (&line["tool"], &line["status"], &line["error_code"]))
                .collect();
            assert_eq!(
                audited,
                [
                    (&json!("fr_search_judilibre"), &json!("error"), &json!("config_error")),
                    (&json!("fr_get_judilibre"), &json!("error"), &json!("config_error")),
                ],
                "{}",
                case.what
            );
        }

        // Contraste : sans aucun réglage, la source est active et l'appel joint PISTE, par le
        // proxy, qui ne répond jamais : l'échéance d'une seconde le termine.
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let mut session = launch(audit.path(), &trap).spawn();
        session.handshake().await;
        let query = format!("{SENTINEL} donation");
        let answer =
            session.request(&call(2, "fr_search_judilibre", &json!({"query": query}))).await;
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(is_tool_error(&answer), "{answer}");
        assert!(!first_text(&answer).contains(SENTINEL), "{answer}");
        assert_eq!(trap.contacts(), [OAUTH_TUNNEL]);
        assert!(!finished.stderr.contains(SENTINEL), "{}", finished.stderr);
    }

    /// G10 et G12 dans la composition réelle : Légifrance puis Judilibre sur le même faux PISTE.
    /// Un seul jeton sert les deux API ; la recherche de Judilibre est un `GET` sur son chemin,
    /// avec la liste fermée de paramètres et `jurisdiction` pour chaque juridiction demandée ; la
    /// lecture ne porte que l'identifiant. Rejoué au niveau de journal le plus bavard : les
    /// bibliothèques HTTP écrivent par `log`, que le binaire relaie vers stderr, et ne doivent pas
    /// y citer les mots, que l'adresse porte.
    #[tokio::test]
    async fn judilibre_shares_the_piste_token_and_sends_only_listed_parameters() {
        for log in [None, Some("trace")] {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let piste = FakePiste::serving_judilibre().await;
            let trap = Trap::bind();
            let mut launch = Launch::new(audit.path()).credentials().proxy(&trap).piste(&piste);
            if let Some(level) = log {
                launch = launch.env("LF_LOG", level);
            }
            let mut session = launch.spawn();
            session.handshake().await;

            let loda = json!({"query": "republique numerique", "fond": "LODA", "page_size": 5});
            let legifrance = session.request(&call(2, "fr_search", &loda)).await;
            assert!(!is_tool_error(&legifrance), "{log:?}: {legifrance:.500}");
            let arguments =
                json!({"query": format!("{SENTINEL} donation"), "courts": ["cc", "ca"]});
            let search = session.request(&call(3, "fr_search_judilibre", &arguments)).await;
            assert!(!is_tool_error(&search), "{log:?}: {search:.500}");
            let hits = search["result"]["structuredContent"]["hits"].as_array().expect("hits");
            assert_eq!(hits.first().expect("a hit")["id"], DECISION, "{log:?}");
            let read =
                session.request(&call(4, "fr_get_judilibre", &json!({"id": DECISION}))).await;
            assert!(!is_tool_error(&read), "{log:?}: {read:.500}");
            assert_eq!(read["result"]["structuredContent"]["id"], DECISION, "{log:?}");

            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{log:?}: stderr:\n{}", finished.stderr);
            assert!(!finished.stderr.contains(SENTINEL), "{log:?}: {}", finished.stderr);
            assert_eq!(finished.stderr.contains("TRACE"), log.is_some(), "{log:?}");
            // Rien d'autre n'est sorti de la boucle locale.
            assert_eq!(trap.contacts(), Vec::<String>::new(), "{log:?}");

            let requests = piste.requests().await;
            let routes: Vec<(&str, &str)> = requests
                .iter()
                .map(|request| (request.method.as_str(), request.url.path()))
                .collect();
            let legifrance_search = format!("{API_BASE}/search");
            assert_eq!(
                routes,
                [
                    ("POST", TOKEN_PATH),
                    ("POST", legifrance_search.as_str()),
                    ("GET", JUDILIBRE_SEARCH),
                    ("GET", JUDILIBRE_DECISION),
                ],
                "{log:?}"
            );
            for request in requests.iter().skip(1) {
                let bearer = request.headers.get("authorization").and_then(|v| v.to_str().ok());
                assert_eq!(bearer, Some(format!("Bearer {FAKE_TOKEN}").as_str()), "{log:?}");
            }
            let pairs = |index: usize| -> Vec<(String, String)> {
                requests
                    .get(index)
                    .expect("a request")
                    .url
                    .query_pairs()
                    .map(|(name, value)| (name.into_owned(), value.into_owned()))
                    .collect()
            };
            let search_pairs = pairs(2);
            assert!(
                search_pairs.iter().all(|(name, _)| SEARCH_PARAMS.contains(&name.as_str())),
                "{log:?}: {search_pairs:?}"
            );
            let courts: Vec<&str> = search_pairs
                .iter()
                .filter(|(name, _)| name == "jurisdiction")
                .map(|(_, value)| value.as_str())
                .collect();
            assert_eq!(courts, ["cc", "ca"], "{log:?}");
            assert!(
                search_pairs.contains(&("query".to_owned(), format!("{SENTINEL} donation"))),
                "{log:?}: {search_pairs:?}"
            );
            assert_eq!(
                pairs(3),
                [
                    ("id".to_owned(), DECISION.to_owned()),
                    ("resolve_references".to_owned(), "true".to_owned()),
                ],
                "{log:?}"
            );

            let lines = audit_lines(audit.path());
            let audited: Vec<(&Value, &Value)> =
                lines.iter().map(|line| (&line["tool"], &line["status"])).collect();
            assert_eq!(
                audited,
                [
                    (&json!("fr_search"), &json!("ok")),
                    (&json!("fr_search_judilibre"), &json!("ok")),
                    (&json!("fr_get_judilibre"), &json!("ok")),
                ],
                "{log:?}"
            );
        }
    }

    /// G5 : sans identifiants, la source reste active mais rien ne part : les deux outils
    /// répondent le problème des identifiants, comme ceux de Légifrance.
    #[tokio::test]
    async fn without_credentials_nothing_is_sent() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let launch = Launch::new(audit.path()).proxy(&trap).env("LF_TOOL_DEADLINE_S", "1");
        let mut session = launch.spawn();
        session.handshake().await;
        for answer in call_both(&mut session).await {
            assert!(is_tool_error(&answer), "{answer}");
            assert!(
                first_text(&answer).starts_with("[config_error] PISTE credentials missing"),
                "{answer}"
            );
        }
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert_eq!(trap.contacts(), Vec::<String>::new());
        assert!(!finished.stderr.contains(SENTINEL), "{}", finished.stderr);
    }

    /// G5 et G9 : `check-config` et `fr_coverage` n'envoient rien, que la source soit active ou
    /// coupée. Sans `LF_JUDILIBRE`, la sortie de `check-config` ne nomme pas la variable ;
    /// `fr_coverage` rend la famille `JUDILIBRE`, la dernière, et la lacune de l'état de la
    /// source.
    #[tokio::test]
    async fn check_config_and_coverage_send_nothing() {
        let on = ["LF_JUDILIBRE", "LF_JUDILIBRE", "on", "ok"];
        let off = ["LF_JUDILIBRE", "LF_JUDILIBRE", "off", "ok"];
        for (vars, line, gap) in [
            (&[][..], None, "FR-110"),
            (&[("LF_JUDILIBRE", "on")][..], Some(on), "FR-110"),
            (&[("LF_JUDILIBRE", "off")][..], Some(off), "FR-111"),
        ] {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let trap = Trap::bind();
            let mut launch = launch(audit.path(), &trap);
            for (name, value) in vars {
                launch = launch.env(name, value);
            }

            let report = launch.clone().args(&["check-config"]).run_to_end().await;
            assert_eq!(report.code, Some(0), "{vars:?}: {report:?}");
            assert!(report.stdout.contains("Result: valid"), "{vars:?}: {}", report.stdout);
            let shown: Option<Vec<&str>> = report
                .stdout
                .lines()
                .find(|row| row.starts_with("LF_JUDILIBRE"))
                .map(|row| row.split_whitespace().collect());
            assert_eq!(shown, line.map(Vec::from), "{vars:?}: {}", report.stdout);

            let mut session = launch.spawn();
            session.handshake().await;
            let coverage = session.request(&call(2, "fr_coverage", &json!({}))).await;
            assert!(!is_tool_error(&coverage), "{vars:?}: {coverage}");
            let view = &coverage["result"]["structuredContent"];
            // La source de Judilibre ne change jamais le statut : il reste celui de PISTE.
            assert_eq!(view["status"], "ok", "{vars:?}: {coverage}");
            let families = view["families"].as_array().expect("families");
            assert_eq!(
                families.last().expect("a family")["tools"],
                json!(["fr_search_judilibre", "fr_get_judilibre"]),
                "{vars:?}: {coverage}"
            );
            assert_eq!(judilibre_gaps(view), [&json!(gap)], "{vars:?}");
            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "{vars:?}: stderr:\n{}", finished.stderr);
            assert_eq!(trap.contacts(), Vec::<String>::new(), "{vars:?}");
        }
    }

    /// Échec fermé (ADR-0035, décision 10) : `LF_JUDILIBRE` ni `on` ni `off` coupe la source. Le
    /// serveur démarre et le dit (`error!` sur stderr, problème de `check-config`, code 2), sans
    /// recopier la valeur ; PISTE n'est pas dégradé pour autant.
    #[tokio::test]
    async fn invalid_lf_judilibre_turns_source_off() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let launch = launch(audit.path(), &trap).env("LF_JUDILIBRE", SENTINEL);

        let report = launch.clone().args(&["check-config"]).run_to_end().await;
        assert_eq!(report.code, Some(2), "{report:?}");
        assert!(report.stdout.contains("Result: invalid"), "{}", report.stdout);
        let problem = "  - LF_JUDILIBRE must be on or off: the Judilibre source is turned off";
        assert!(report.stdout.contains(problem), "{}", report.stdout);
        let row = report.stdout.lines().find(|row| row.starts_with("LF_JUDILIBRE")).expect("a row");
        assert_eq!(
            row.split_whitespace().collect::<Vec<_>>(),
            ["LF_JUDILIBRE", "LF_JUDILIBRE", "off", "(invalid", "value)", "problem"]
        );
        assert!(!report.stdout.contains(SENTINEL) && !report.stderr.contains(SENTINEL));

        let mut session = launch.spawn();
        session.handshake().await;
        for answer in call_both(&mut session).await {
            assert_eq!(first_text(&answer), DISABLED);
        }
        let coverage = session.request(&call(4, "fr_coverage", &json!({}))).await;
        let view = &coverage["result"]["structuredContent"];
        assert_eq!(view["status"], "ok", "{coverage}");
        assert!(view["configuration"].get("problems").is_none(), "{coverage}");
        assert_eq!(judilibre_gaps(view), [&json!("FR-111")], "{coverage}");
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(
            finished.stderr.lines().any(|line| {
                line.contains("ERROR") && line.contains("LF_JUDILIBRE must be on or off")
            }),
            "{}",
            finished.stderr
        );
        assert!(!finished.stderr.contains(SENTINEL), "{}", finished.stderr);
        assert_eq!(trap.contacts(), Vec::<String>::new());
    }

    /// Mode hors ligne : `LF_DILA_DIR` défini coupe la source d'office, même quand il est
    /// inutilisable, et `check-config` le montre. Seul `LF_JUDILIBRE=on` la rétablit : elle joint
    /// alors PISTE, alors que les textes de Légifrance restent indisponibles.
    #[tokio::test]
    async fn unusable_dila_dir_turns_source_off() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let trap = Trap::bind();
        let offline = launch(audit.path(), &trap).env("LF_DILA_DIR", "relative/index");

        let report = offline.clone().args(&["check-config"]).run_to_end().await;
        let row = report.stdout.lines().find(|row| row.starts_with("LF_JUDILIBRE")).expect("a row");
        assert_eq!(
            row.split_whitespace().collect::<Vec<_>>(),
            ["LF_JUDILIBRE", "default", "off", "(LF_DILA_DIR", "is", "set)", "ok"]
        );
        // Le seul problème est celui de `LF_DILA_DIR` : la coupure d'office n'en est pas un.
        assert_eq!(report.code, Some(2), "{report:?}");
        assert!(!report.stdout.contains("LF_JUDILIBRE must be"), "{}", report.stdout);
        assert_eq!(trap.contacts(), Vec::<String>::new());

        // `LF_JUDILIBRE=on` explicite : la source revient, et elle seule joint le réseau.
        let mut session = offline.env("LF_JUDILIBRE", "on").spawn();
        session.handshake().await;
        let legifrance = session.request(&call(2, "fr_search", &json!({"query": "bail"}))).await;
        assert!(
            first_text(&legifrance).starts_with("[config_error] LF_DILA_DIR must be an absolute"),
            "{legifrance}"
        );
        let query = format!("{SENTINEL} donation");
        let answer =
            session.request(&call(3, "fr_search_judilibre", &json!({"query": query}))).await;
        assert!(is_tool_error(&answer), "{answer}");
        assert_ne!(first_text(&answer), DISABLED, "{answer}");
        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert_eq!(trap.contacts(), [OAUTH_TUNNEL]);
        assert!(!finished.stderr.contains(SENTINEL), "{}", finished.stderr);
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
