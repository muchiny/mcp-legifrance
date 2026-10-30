//! Le binaire `mcp-legifrance` réel sur stdio, deux ères (T-M4-08, 06 §2.7 et §6.1).
//!
//! - Les 22 transcriptions JSON-RPC de `lf-adapter-mcp` (`tests/jsonrpc/`) sont rejouées sur le
//!   processus par [`common::replay_on_child`], dont `eof_with_open_listen`, qui ne l'est que
//!   sur le binaire (V-35). Lèvent V-30 (aucun message inattendu ne fait quitter le processus :
//!   `garbage_and_batch`, `premature_notification`, `modern_removed_methods`, suivies d'un
//!   `expect_alive`) et V-35.
//! - Fin de vie : EOF après `server/discover`, EOF en session, EOF pendant un appel en vol
//!   (Review Focus 5) et SIGTERM pendant un appel en vol, avec les délais de 06 §12.1.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#[cfg(test)]
mod common;

#[cfg(test)]
mod transcripts {
    use std::path::PathBuf;

    use lf_adapter_mcp::testing::{Transcript, transcripts_dir};

    use crate::common::replay_on_child;

    /// Rejoue `tests/jsonrpc/<name>.jsonl` sur le binaire.
    async fn replay_file(name: &str) {
        let file: PathBuf = transcripts_dir().join(format!("{name}.jsonl"));
        let transcript = Transcript::load(&file).expect("the transcript loads");
        assert_eq!(transcript.name(), name);
        // Le message de l'échec dit la transcription et la ligne de l'instruction en cause.
        let replayed = replay_on_child(&transcript).await.map_err(|error| error.to_string());
        replayed.expect("the transcript replays on the child process");
    }

    /// Un test par transcription, nommé comme son fichier (06 §6.1), et la liste de ces noms.
    macro_rules! replay_every_transcript {
        ($($name:ident),+ $(,)?) => {
            /// Les transcriptions rejouées ci-dessous.
            const REPLAYED: &[&str] = &[$(stringify!($name)),+];

            $(
                #[tokio::test]
                async fn $name() {
                    replay_file(stringify!($name)).await;
                }
            )+
        };
    }

    replay_every_transcript!(
        cancel_then_silence,
        discover_after_initialize,
        eof_after_discover,
        eof_with_open_listen,
        garbage_and_batch,
        initialize_negotiation,
        interleaved_two_conversations,
        legacy_2025_06_18,
        legacy_2025_11_25,
        legacy_then_modern,
        modern_discover,
        modern_envelope_legacy_version,
        modern_listen,
        modern_meta_incomplete,
        modern_removed_methods,
        modern_tools_call,
        modern_tools_list,
        modern_unsupported_version,
        mrtr_fields_tolerated,
        premature_notification,
        results_carry_server_info,
        unknown_method_lifecycle,
    );

    /// La liste minimale de 06 §6.1 est complète : 22 transcriptions, toutes rejouées sur le
    /// binaire, et aucun fichier de `tests/jsonrpc/` n'est laissé de côté.
    #[test]
    fn every_transcript_file_is_replayed_on_the_binary() {
        let on_disk: Vec<String> = Transcript::load_all(transcripts_dir())
            .expect("the transcripts load")
            .iter()
            .map(|transcript| transcript.name().to_owned())
            .collect();
        assert_eq!(on_disk.len(), 22, "{on_disk:?}");
        let mut replayed: Vec<&str> = REPLAYED.to_vec();
        replayed.sort_unstable();
        assert_eq!(replayed, on_disk, "a transcript is missing from replay_every_transcript!");
    }
}

/// Le rejeu sur le binaire doit refuser ce qui s'écarte de la transcription : sinon les 22 verts
/// ci-dessus ne prouveraient rien.
#[cfg(test)]
mod harness {
    use lf_adapter_mcp::testing::Transcript;

    use crate::common::replay_on_child;

    const INITIALIZE: &str = r#"{"send":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#;

    /// Rejoue `lines` (une instruction par ligne) et rend le message de l'écart attendu.
    async fn failure_of(lines: &[&str]) -> String {
        let transcript = Transcript::parse("harness", &lines.join("\n")).expect("a transcript");
        let error = replay_on_child(&transcript).await.expect_err("the replay must fail");
        format!("line {}: {}", error.line, error.message)
    }

    #[tokio::test]
    async fn replay_on_child_rejects_a_wrong_expectation() {
        let wrong =
            r#"{"expect":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"1999-01-01"}}}"#;
        let message = failure_of(&[INITIALIZE, wrong]).await;
        assert!(message.starts_with("line 2: "), "{message}");
        assert!(message.contains("/result/protocolVersion"), "{message}");
    }

    #[tokio::test]
    async fn replay_on_child_rejects_a_message_nobody_expected() {
        let ping = r#"{"send":{"jsonrpc":"2.0","id":2,"method":"ping"}}"#;
        let message = failure_of(&[INITIALIZE, ping]).await;
        assert!(message.contains("said more than expected"), "{message}");
    }

    #[tokio::test]
    async fn replay_on_child_rejects_a_wrong_exit_code_and_a_late_exit() {
        let wrong_code = r#"{"expect_exit":{"code":1,"within_ms":1000}}"#;
        let message = failure_of(&[r#"{"close_stdin":true}"#, wrong_code]).await;
        assert!(message.contains("exited with code 0, expected 1"), "{message}");
        let too_late = r#"{"expect_exit":{"code":0,"within_ms":0}}"#;
        let message = failure_of(&[r#"{"close_stdin":true}"#, too_late]).await;
        assert!(message.contains("expected within"), "{message}");
    }

    #[tokio::test]
    async fn replay_on_child_rejects_a_server_that_stays_silent() {
        let expected = r#"{"expect":{"jsonrpc":"2.0","id":7,"result":{}}}"#;
        let message = failure_of(&[expected]).await;
        assert!(message.contains("no message from the server"), "{message}");
    }
}

#[cfg(test)]
mod lifecycle {
    use std::time::Duration;

    use serde_json::json;

    use crate::common::{
        FakePiste, Launch, Read, audit_lines, call, discover, first_text, initialize, initialized,
        is_tool_error, method_request, modern_call, modern_meta,
    };

    /// Le délai de sortie après l'EOF de stdin (C-09, 06 §12.1).
    const EOF_EXIT: Duration = Duration::from_secs(1);

    /// Le délai de sortie après SIGTERM (06 §12.1 : SIGINT, SIGTERM 100 ms plus tard, puis SIGKILL
    /// 400 ms après).
    #[cfg(unix)]
    const SIGTERM_EXIT: Duration = Duration::from_millis(500);

    /// Un appel `fr_search` en vol : le jeton OAuth du faux PISTE met 30 s à venir.
    fn search_call(id: u64) -> serde_json::Value {
        modern_call(id, "fr_search", &json!({"query": "bail"}))
    }

    /// C-09 : l'EOF juste après `server/discover` termine le processus avec le code 0 en moins
    /// d'une seconde, sans aucun message de plus.
    #[tokio::test]
    async fn e2e_eof_after_discover_exits_zero() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::silent().await;
        let mut session = Launch::new(audit.path()).piste(&piste).spawn();
        let discovered = session.request(&discover(1)).await;
        assert_eq!(discovered["result"]["resultType"], "complete", "{discovered}");

        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(finished.elapsed < EOF_EXIT, "{:?}", finished.elapsed);
        assert_eq!(finished.stdout.len(), 1, "{:?}", finished.stdout);
    }

    /// Lancements du test de la fenêtre de l'EOF : avant la pause du filtre de `lf-adapter-mcp`
    /// (`LISTEN_EOF_GRACE`), l'échec était fréquent (3 sur 8 mesurés) ; dix lancements le rendent
    /// quasi certain si le défaut revient.
    const RACE_LAUNCHES: usize = 10;

    /// C-09, C-24, V-35 : l'EOF **aussitôt après l'accusé** d'un `subscriptions/listen` ouvert
    /// clôt le `listen` par son résultat final et termine le processus avec le code 0 en moins
    /// d'une seconde.
    ///
    /// Défaut corrigé à l'intégration de V3 (constaté sur le binaire par V3-01) : la boucle de rmcp
    /// pouvait s'arrêter avant d'avoir traité la fin de l'écriture de l'accusé, et le serveur
    /// attendait alors les 5 s du drainage (`timed out draining in-flight responses`, « failed to
    /// acknowledge subscription: Transport closed »), fermait stdout sans résultat final pour le
    /// `listen`, puis sortait avec le code 0. Le filtre de stdin laisse maintenant `LISTEN_EOF_GRACE`
    /// à la boucle avant de rendre l'EOF d'une entrée qui a ouvert une écoute.
    #[tokio::test]
    async fn eof_right_after_listen_ack_exits_within_1s() {
        for launch in 0..RACE_LAUNCHES {
            let audit = tempfile::tempdir().expect("a temporary directory");
            let mut session = Launch::new(audit.path()).spawn();
            let listen = json!({"jsonrpc": "2.0", "id": "listen-1", "method": "subscriptions/listen",
                "params": {"notifications": {}, "_meta": modern_meta()}});
            session.send(&listen).await;
            let ack = session.recv().await;
            assert_eq!(ack["method"], "notifications/subscriptions/acknowledged", "{ack}");

            let finished = session.finish().await;
            assert_eq!(finished.code(), Some(0), "launch {launch}: stderr:\n{}", finished.stderr);
            assert!(
                finished.elapsed < EOF_EXIT,
                "launch {launch}: exit after {:?}, stderr:\n{}",
                finished.elapsed,
                finished.stderr
            );
            let last = finished.stdout.last().expect("the listen is closed");
            let closing: serde_json::Value = serde_json::from_str(last).expect("JSON on stdout");
            assert_eq!(closing["id"], "listen-1", "launch {launch}: {closing}");
            assert_eq!(closing["result"]["resultType"], "complete", "launch {launch}: {closing}");
        }
    }

    /// C-09 : l'EOF en cours de session (ère legacy, outils appelés) termine le processus avec le
    /// code 0 en moins d'une seconde.
    #[tokio::test]
    async fn e2e_eof_in_session_exits_zero() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::silent().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        session.handshake().await;
        let coverage = session.request(&call(2, "fr_coverage", &json!({}))).await;
        assert!(!is_tool_error(&coverage), "{coverage}");
        let tools = session.request(&method_request(3, "tools/list")).await;
        assert!(tools["result"]["tools"].is_array(), "{tools}");

        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(finished.elapsed < EOF_EXIT, "{:?}", finished.elapsed);
        assert_eq!(finished.stdout.len(), 3, "{:?}", finished.stdout);
        // Aucune requête vers PISTE, une ligne d'audit pour `fr_coverage`.
        assert!(piste.requests().await.is_empty());
        let lines = audit_lines(audit.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0]["tool"], "fr_coverage");
    }

    /// Review Focus 5 : l'EOF pendant un appel en vol termine le processus avec le code 0 en
    /// moins d'une seconde, sans attendre les 30 s de l'amont ni les 5 s que rmcp accorde sinon
    /// aux réponses en vol. L'appel garde sa ligne d'audit (Q9) ; sa réponse, si rmcp l'écrit
    /// encore, dit que le client l'a annulé.
    #[tokio::test]
    async fn eof_with_inflight_call_exits_zero_within_1s() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::slow().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        session.send(&search_call(1)).await;
        // L'appel est en vol : l'amont dure 30 s, rien ne répond.
        // La requête de jeton est arrivée chez l'amont, qui ne répondra pas avant 30 s.
        assert_eq!(piste.wait_for_requests(1).await.len(), 1, "the call reached the upstream");
        assert!(matches!(session.read_line(Duration::from_millis(300)).await, Read::Silence));

        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(finished.elapsed < EOF_EXIT, "{:?}", finished.elapsed);
        assert!(!finished.stderr.contains("panicked"), "{}", finished.stderr);
        // Au plus la réponse de l'appel annulé, sur stdout.
        assert!(finished.stdout.len() <= 1, "{:?}", finished.stdout);
        if let Some(line) = finished.stdout.first() {
            let late: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
            assert_eq!(late["id"], json!(1), "{late}");
            assert_eq!(late["result"]["isError"], json!(true), "{late}");
            assert_eq!(first_text(&late), "cancelled by client", "{late}");
        }
        // Une ligne d'audit par appel, même annulé par l'EOF (Q9).
        let lines = audit_lines(audit.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0]["tool"], "fr_search");
        assert_eq!(lines[0]["status"], "error");
        assert_eq!(lines[0]["error"], "cancelled by client");
    }

    /// 02 §8.8, 06 §12.1 : SIGTERM pendant un appel en vol, stdin restant ouvert, arrête le
    /// processus avec le code 0 en moins de 500 ms.
    #[cfg(unix)]
    #[tokio::test]
    async fn e2e_sigterm_exits_quickly() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::slow().await;
        let mut session = Launch::new(audit.path()).credentials().piste(&piste).spawn();
        session.send(&search_call(1)).await;
        // La requête de jeton est arrivée chez l'amont, qui ne répondra pas avant 30 s.
        assert_eq!(piste.wait_for_requests(1).await.len(), 1, "the call reached the upstream");
        assert!(matches!(session.read_line(Duration::from_millis(300)).await, Read::Silence));

        let finished = session.terminate().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
        assert!(finished.elapsed < SIGTERM_EXIT, "{:?}", finished.elapsed);
        assert!(finished.stderr.contains("shutdown signal received"), "{}", finished.stderr);
        assert!(!finished.stderr.contains("panicked"), "{}", finished.stderr);
    }

    /// P-001 : la liste des outils est la même dans les deux ères, dans la même session :
    /// `server/discover` (qui n'ouvre aucune ère), puis `initialize` 2025-11-25 et `tools/list`
    /// legacy, puis `tools/list` moderne avec son `_meta` (l'ère ouverte en premier reste celle de
    /// la session, annexe D §4.3 : l'ordre inverse est un cas de `legacy_then_modern`).
    #[tokio::test]
    async fn stdio_tools_list_both_eras() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let piste = FakePiste::silent().await;
        let mut session = Launch::new(audit.path()).piste(&piste).spawn();

        let discovered = session.request(&discover(1)).await;
        assert_eq!(discovered["result"]["resultType"], "complete", "{discovered}");
        let handshake = session.request(&initialize(2)).await;
        assert_eq!(handshake["result"]["protocolVersion"], "2025-11-25", "{handshake}");
        session.send(&initialized()).await;
        let legacy = session.request(&method_request(3, "tools/list")).await;
        assert!(legacy["result"].get("resultType").is_none(), "{legacy:.300}");
        let legacy_tools = legacy["result"]["tools"].clone();
        assert!(legacy_tools.as_array().is_some_and(|tools| !tools.is_empty()), "{legacy:.300}");

        let modern = session
            .request(&json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list",
                             "params": {"_meta": modern_meta()}}))
            .await;
        assert_eq!(modern["result"]["resultType"], "complete", "{modern:.300}");
        assert_eq!(modern["result"]["tools"], legacy_tools, "the eras list different tools");

        let finished = session.finish().await;
        assert_eq!(finished.code(), Some(0), "stderr:\n{}", finished.stderr);
    }
}
