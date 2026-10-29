//! Parité de la sortie des outils avec le golden de la référence, budgets des sorties par défaut
//! et catalogue de non-régression (T-M5-05, V3-03) : le binaire `mcp-legifrance` réel, lancé avec
//! `env_clear()`, parle à un `wiremock` qui sert les seize fixtures de référence
//! (`lf-adapter-piste/tests/fixtures/reference`) et un jeton factice.
//!
//! - `tool_output_matches_golden` (niveau 2 du harnais golden, annexe A §5.4) : un `tools/call` par
//!   fixture, avec les arguments du harnais (`detail: "full"`, `max_chars: 40000`, `toc_limit: 500`),
//!   dans les deux ères (`initialize` 2025-11-25, puis `server/discover` 2026-07-28 avec `_meta`). Les
//!   deux `structuredContent` doivent être identiques (00 §6). Le golden `tool_output` subit les
//!   onze transformations de l'annexe A §5.4, dans l'ordre, puis chaque clé du golden transformé doit
//!   exister côté Rust avec la même valeur, et chaque clé Rust en plus figurer dans la liste blanche
//!   `ADDED`.
//! - `default_outputs_within_target` : la taille de chaque appel par défaut (caractères de JSON
//!   compact) ne dépasse pas la colonne « Cible » de 05 §6.5 plus 10 % (06 §12.2).
//! - `default_outputs_match_snapshots` : les seize sorties par défaut, figées dans
//!   `tests/snapshots/parity_tool_output__*.snap` (format d'instantané `insta`, sans la dépendance :
//!   `lf-app` n'a pas `insta` en dépendance de développement).
//! - `regression_catalog_is_implemented` (D-43) : les noms de test du catalogue 06 §3.1 existent
//!   dans le code.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` et `expect` permis, `clippy.toml`).

#[cfg(test)]
mod parity {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::{Path, PathBuf},
        process::Stdio,
        time::Duration,
    };

    use lf_domain::{
        fond::Fond,
        text::{clean, encoding::repair_latin1_mojibake},
    };
    use serde_json::{Map, Value, json};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
        process::{Child, ChildStdin, ChildStdout, Command},
        task::JoinHandle,
    };
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method, path},
    };

    // ---- le faux PISTE -------------------------------------------------------------------------

    /// Binaire sous test : construit par cargo pour cette cible de test.
    const BINARY: &str = env!("CARGO_BIN_EXE_mcp-legifrance");
    /// Préfixe des points d'accès de l'API (celui de la sandbox).
    const API_BASE: &str = "/dila/legifrance/lf-engine-app";
    const TOKEN_PATH: &str = "/oauth/token";
    /// Attente maximale d'une ligne de stdout ou de la fin du processus (CI lente).
    const WAIT: Duration = Duration::from_secs(30);

    macro_rules! reference {
        ($name:literal) => {
            include_bytes!(concat!(
                "../../lf-adapter-piste/tests/fixtures/reference/",
                $name,
                ".json"
            ))
        };
    }

    const GOLDEN: &[u8] = reference!("parite-golden");

    fn search_fixture(fond: Fond) -> &'static [u8] {
        match fond {
            Fond::Loda => reference!("search_loda"),
            Fond::Code => reference!("search_code"),
            Fond::Juri => reference!("search_juri"),
            Fond::Constit => reference!("search_constit"),
            Fond::Cetat => reference!("search_cetat"),
            Fond::Cnil => reference!("search_cnil"),
            Fond::Kali => reference!("search_kali"),
            Fond::Acco => reference!("search_acco"),
        }
    }

    fn reply(body: &'static [u8]) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body, "application/json")
    }

    /// Le corps de `endpoint` pour `request` (sous-ensemble du JSON envoyé) est servi tel quel.
    async fn serve(server: &MockServer, endpoint: &str, request: &Value, body: &'static [u8]) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .and(body_partial_json(request))
            .respond_with(reply(body))
            .mount(server)
            .await;
    }

    /// Un jeton factice, puis les seize fixtures : la recherche de chaque fond, et la consultation
    /// de chaque identifiant des fixtures.
    async fn piste() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(reply(
                br#"{"access_token":"parity-token","token_type":"Bearer","expires_in":3600}"#,
            ))
            .mount(&server)
            .await;
        for fond in Fond::ALL {
            serve(&server, "/search", &json!({"fond": fond.api_name()}), search_fixture(fond))
                .await;
        }
        for case in CASES.iter().filter(|case| case.endpoint != "/search") {
            let body: Option<&'static [u8]> = match case.fixture {
                "consult_loda" => Some(reference!("consult_loda")),
                "consult_article" => Some(reference!("consult_article")),
                "consult_juri" => Some(reference!("consult_juri")),
                "consult_juri_constit" => Some(reference!("consult_juri_constit")),
                "consult_juri_cetat" => Some(reference!("consult_juri_cetat")),
                "consult_cnil" => Some(reference!("consult_cnil")),
                "consult_kali" => Some(reference!("consult_kali")),
                "consult_acco" => Some(reference!("consult_acco")),
                _ => None,
            };
            let body = body.expect("every consultation case names a reference fixture");
            serve(&server, case.endpoint, &json!({case.key: case.id}), body).await;
        }
        server
    }

    // ---- les seize cas -------------------------------------------------------------------------

    /// Un appel d'outil sur une fixture.
    struct Case {
        /// Nom de la fixture, sans extension (la clé du golden est `<fixture>.json`).
        fixture: &'static str,
        tool: &'static str,
        /// Point d'accès PISTE servi (`/search` pour les huit recherches).
        endpoint: &'static str,
        /// Clé et valeur du corps de la requête PISTE qui désignent la fixture (hors recherche).
        key: &'static str,
        id: &'static str,
        /// Nom de l'argument d'identifiant de l'outil (ou `fond` pour une recherche).
        argument: &'static str,
        /// Arguments du harnais golden (annexe A §5.4) en plus de l'identifiant.
        golden_extra: &'static [(&'static str, ExtraValue)],
    }

    #[derive(Clone, Copy)]
    enum ExtraValue {
        Str(&'static str),
        Int(i64),
    }

    impl Case {
        /// Arguments de l'appel par défaut : l'identifiant seul (ou la requête et le fond).
        fn default_arguments(&self) -> Value {
            if self.tool == "fr_search" {
                json!({"query": "golden", "fond": self.id})
            } else {
                json!({ self.argument: self.id })
            }
        }

        /// Arguments du harnais golden : le défaut, plus la lecture complète.
        fn golden_arguments(&self) -> Value {
            let mut arguments = self.default_arguments();
            for (name, value) in self.golden_extra {
                arguments[*name] = match value {
                    ExtraValue::Str(text) => json!(text),
                    ExtraValue::Int(number) => json!(number),
                };
            }
            arguments
        }
    }

    const FULL: &[(&str, ExtraValue)] =
        &[("detail", ExtraValue::Str("full")), ("max_chars", ExtraValue::Int(40_000))];
    const TOC_500: &[(&str, ExtraValue)] = &[("toc_limit", ExtraValue::Int(500))];

    const fn search(fixture: &'static str, fond: &'static str) -> Case {
        Case {
            fixture,
            tool: "fr_search",
            endpoint: "/search",
            key: "fond",
            id: fond,
            argument: "fond",
            golden_extra: &[],
        }
    }

    const fn consult(
        fixture: &'static str,
        tool: &'static str,
        endpoint: &'static str,
        key: &'static str,
        id: &'static str,
        argument: &'static str,
        golden_extra: &'static [(&'static str, ExtraValue)],
    ) -> Case {
        Case { fixture, tool, endpoint, key, id, argument, golden_extra }
    }

    /// Les seize fixtures : huit recherches, puis huit consultations (cinq lectures par outil).
    const CASES: [Case; 16] = [
        search("search_loda", "LODA"),
        search("search_code", "CODE"),
        search("search_juri", "JURI"),
        search("search_constit", "CONSTIT"),
        search("search_cetat", "CETAT"),
        search("search_cnil", "CNIL"),
        search("search_kali", "KALI"),
        search("search_acco", "ACCO"),
        consult(
            "consult_loda",
            "fr_get_act",
            "/consult/lawDecree",
            "textId",
            "LEGITEXT000033205014",
            "text_id",
            TOC_500,
        ),
        consult(
            "consult_article",
            "fr_get_text",
            "/consult/getArticle",
            "id",
            "LEGIARTI000006419288",
            "article_id",
            FULL,
        ),
        consult(
            "consult_juri",
            "fr_get_decision",
            "/consult/juri",
            "textId",
            "JURITEXT000051743650",
            "decision_id",
            FULL,
        ),
        consult(
            "consult_juri_constit",
            "fr_get_decision",
            "/consult/juri",
            "textId",
            "CONSTEXT000052555275",
            "decision_id",
            FULL,
        ),
        consult(
            "consult_juri_cetat",
            "fr_get_decision",
            "/consult/juri",
            "textId",
            "CETATEXT000054178503",
            "decision_id",
            FULL,
        ),
        consult(
            "consult_cnil",
            "fr_get_deliberation",
            "/consult/cnil",
            "textId",
            "CNILTEXT000053352643",
            "deliberation_id",
            FULL,
        ),
        consult(
            "consult_kali",
            "fr_get_convention",
            "/consult/kaliText",
            "id",
            "KALITEXT000049878950",
            "convention_id",
            TOC_500,
        ),
        consult(
            "consult_acco",
            "fr_get_company_agreement",
            "/consult/acco",
            "id",
            "ACCOTEXT000053643828",
            "agreement_id",
            &[],
        ),
    ];

    // ---- le binaire et son protocole -----------------------------------------------------------

    /// Les deux ères du protocole (annexe D) : le même appel d'outil dans chacune.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Era {
        /// `initialize` 2025-11-25.
        Legacy,
        /// `server/discover`, puis `_meta` 2026-07-28 sur chaque requête.
        Modern,
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

    fn modern_meta() -> Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "parity", "version": "0.0.0"},
            "io.modelcontextprotocol/clientCapabilities": {}
        })
    }

    /// Une session JSON-RPC avec le binaire : stdin, stdout ligne à ligne, stderr drainé.
    struct Session {
        child: Child,
        stdin: ChildStdin,
        out: Lines<BufReader<ChildStdout>>,
        stderr: JoinHandle<String>,
        era: Era,
        next_id: u64,
    }

    impl Session {
        /// Lance `mcp-legifrance serve` vers `server`, puis fait la poignée de main de `era`.
        async fn start(server: &MockServer, audit: &Path, era: Era) -> Self {
            let uri = server.uri();
            let mut command = Command::new(BINARY);
            command.arg("serve").env_clear();
            forward_platform_environment(&mut command);
            command
                .env("LF_AUDIT_DIR", audit)
                .env("LF_PISTE_OAUTH_URL", format!("{uri}{TOKEN_PATH}"))
                .env("LF_PISTE_BASE_URL", format!("{uri}{API_BASE}"))
                .env("LF_PISTE_CLIENT_ID", "11112222-3333-4444-5555-666677778888")
                .env("LF_PISTE_CLIENT_SECRET", "99990000-aaaa-bbbb-cccc-ddddeeeeffff")
                // Le limiteur par défaut (5 req/s) étalerait les 32 appels sur plusieurs secondes.
                .env("LF_PISTE_RPS", "100")
                .env("LF_PISTE_BURST", "100")
                // Un proxy en échec : seul le serveur local (`NO_PROXY`) peut répondre.
                .env("HTTPS_PROXY", "http://127.0.0.1:9")
                .env("HTTP_PROXY", "http://127.0.0.1:9")
                .env("NO_PROXY", "127.0.0.1,localhost")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let mut child = command.spawn().expect("the binary starts");
            let stdin = child.stdin.take().expect("a piped stdin");
            let out = BufReader::new(child.stdout.take().expect("a piped stdout")).lines();
            let mut err = BufReader::new(child.stderr.take().expect("a piped stderr")).lines();
            let stderr = tokio::spawn(async move {
                let mut text = String::new();
                while let Ok(Some(line)) = err.next_line().await {
                    text.push_str(&line);
                    text.push('\n');
                }
                text
            });
            let mut session = Self { child, stdin, out, stderr, era, next_id: 1 };
            session.handshake().await;
            session
        }

        async fn handshake(&mut self) {
            match self.era {
                Era::Legacy => {
                    let response = self
                        .request(
                            "initialize",
                            json!({
                                "protocolVersion": "2025-11-25",
                                "capabilities": {},
                                "clientInfo": {"name": "parity", "version": "0.0.0"}
                            }),
                        )
                        .await;
                    assert_eq!(response["result"]["protocolVersion"], "2025-11-25", "{response}");
                    self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                        .await;
                }
                Era::Modern => {
                    let response =
                        self.request("server/discover", json!({"_meta": modern_meta()})).await;
                    assert_eq!(response["result"]["resultType"], "complete", "{response}");
                }
            }
        }

        async fn send(&mut self, message: &Value) {
            let mut line = serde_json::to_vec(message).expect("a serializable message");
            line.push(b'\n');
            self.stdin.write_all(&line).await.expect("write to the server");
            self.stdin.flush().await.expect("flush to the server");
        }

        /// Envoie une requête et rend la réponse qui porte son `id` (les notifications du serveur
        /// qui s'intercaleraient sont écartées).
        async fn request(&mut self, name: &str, params: Value) -> Value {
            let id = self.next_id;
            self.next_id += 1;
            self.send(&json!({"jsonrpc": "2.0", "id": id, "method": name, "params": params})).await;
            loop {
                let line = tokio::time::timeout(WAIT, self.out.next_line())
                    .await
                    .expect("a line before the timeout")
                    .expect("stdout is readable")
                    .expect("the server is still running");
                let message: Value = serde_json::from_str(&line).expect("stdout carries JSON");
                if message.get("id") == Some(&json!(id)) {
                    return message;
                }
            }
        }

        /// `tools/call` dans l'ère de la session : le résultat de la réponse.
        async fn call(&mut self, tool: &str, arguments: &Value) -> Value {
            let mut params = json!({"name": tool, "arguments": arguments});
            if self.era == Era::Modern {
                params["_meta"] = modern_meta();
            }
            let response = self.request("tools/call", params).await;
            assert!(response.get("error").is_none(), "{tool}: {response}");
            response["result"].clone()
        }

        /// Ferme stdin (EOF) et rend ce que le processus a écrit sur stderr.
        async fn finish(mut self) -> String {
            drop(self.stdin);
            let status = tokio::time::timeout(WAIT, self.child.wait())
                .await
                .expect("the server stops")
                .expect("the status is readable");
            assert_eq!(status.code(), Some(0));
            self.stderr.await.expect("stderr is drained")
        }
    }

    /// Une réponse d'outil réussie : `structuredContent`, vérifié contre `content[0].text`.
    fn structured(result: &Value, what: &str) -> Value {
        assert_eq!(result["isError"], json!(false), "{what}: {result}");
        let structured = result["structuredContent"].clone();
        assert!(structured.is_object(), "{what}: {result}");
        let text = result["content"][0]["text"].as_str().expect("a text block");
        let parsed: Value = serde_json::from_str(text).expect("the text block is JSON");
        assert_eq!(parsed, structured, "{what}: the text block differs from structuredContent");
        structured
    }

    /// Les seize appels de `arguments`, dans l'ordre de [`CASES`].
    async fn run(server: &MockServer, era: Era, arguments: impl Fn(&Case) -> Value) -> Vec<Value> {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let mut session = Session::start(server, audit.path(), era).await;
        let mut outputs = Vec::new();
        for case in &CASES {
            let result = session.call(case.tool, &arguments(case)).await;
            outputs.push(structured(&result, case.fixture));
        }
        let stderr = session.finish().await;
        assert!(!stderr.contains("panicked"), "{stderr}");
        outputs
    }

    // ---- le golden -----------------------------------------------------------------------------

    /// Les fixtures marquées `mojibake` dans leur `.meta.toml` (04 §13.3) : six sur seize.
    fn is_mojibake(fixture: &str) -> bool {
        let sidecar = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../lf-adapter-piste/tests/fixtures/reference")
            .join(format!("{fixture}.meta.toml"));
        let text = fs::read_to_string(&sidecar).map_err(|e| format!("{sidecar:?}: {e}")).unwrap();
        text.lines().any(|line| line.starts_with("known_issues") && line.contains("\"mojibake\""))
    }

    /// Champs que la sortie d'un outil ajoute au golden de la référence (annexe A §4.1,
    /// `fields_added` : liste blanche). Tout champ Rust absent du golden transformé doit y figurer.
    ///
    /// Le binaire annonce la surface complète (v0.3) : `fr_search` y rend en plus `total_articles`
    /// et l'`idcc` des conventions, que la surface v0.1 omettait (05 §5.1).
    fn added(tool: &str) -> &'static [&'static str] {
        match tool {
            "fr_search" => {
                &["page", "has_more", "snippet", "date", "articles", "total_articles", "idcc"]
            }
            "fr_get_act" => &["date", "jo_reference", "aliases", "toc_total", "next_toc_offset"],
            "fr_get_text" => &[
                "parent_id",
                "path",
                "nota",
                "text_length",
                "next_offset",
                "summary",
                "summary_source",
            ],
            "fr_get_decision" | "fr_get_deliberation" => {
                &["summary", "summary_source", "text_length", "next_offset", "passages", "note"]
            }
            "fr_get_convention" => {
                &["idcc", "container_title", "bulletin", "toc_total", "next_toc_offset"]
            }
            _ => &[],
        }
    }

    /// Champs de la référence que la sortie ne rend plus (`fields_not_rendered`, 05 §1.4 règle 8).
    fn not_rendered(tool: &str) -> &'static [&'static str] {
        match tool {
            "fr_search" => &["cid", "idcc", "raison_sociale"],
            "fr_get_act" => &["cid"],
            "fr_get_text" => &["full_sections_titre"],
            "fr_get_convention" => {
                &["cid", "nature", "num_parution", "date_debut_version", "date_fin_version"]
            }
            _ => &[],
        }
    }

    /// Valeurs explicites (annexe A §5.4, `Override`) du niveau 2 : l'entrée, le champ du golden
    /// (avant renommage), la valeur attendue et la divergence qui la justifie.
    const OVERRIDES: [(&str, &str, &str, &str); 4] = [
        (
            "consult_juri_constit",
            "/human_readable_citation",
            r#""Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025""#,
            "citation_typography",
        ),
        (
            "consult_cnil",
            "/human_readable_citation",
            r#""CNIL, délibération n° SAN-2026-002 du 8 janvier 2026""#,
            "citation_typography",
        ),
        ("consult_loda", "/etat", r#""VIGUEUR""#, "etat_juris_state_fallback"),
        ("consult_juri_cetat", "/numero_affaire", r#"["506507"]"#, "numero_affaire_cetat"),
    ];

    /// Les divergences de l'annexe A §4 que le niveau 2 doit toutes exercer au moins une fois sur
    /// les seize fixtures (`returned_id_kept` relève du niveau 1 : il demande la variante KALIARTI).
    const EXERCISED: [&str; 18] = [
        "citation_renamed",
        "eli_uri_dropped",
        "notes_dropped",
        "nulls_omitted",
        "search_echo_dropped",
        "fields_renamed",
        "fields_not_rendered",
        "title_omitted_when_in_citation",
        "fields_added",
        "citation_typography",
        "sentinel_dates_none",
        "mojibake_repaired",
        "short_text_cleaned",
        "long_text_structured",
        "byte_size_to_text_length",
        "toc_tree",
        "etat_juris_state_fallback",
        "numero_affaire_cetat",
    ];

    /// Aplatit les blancs (NBSP compris) en une espace et rogne les bords.
    fn fold_ws(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Applique `f` à chaque chaîne de `value`, sauf sous les clés de `skip`.
    fn map_strings(value: &mut Value, skip: &[&str], f: &mut dyn FnMut(&str) -> Option<String>) {
        match value {
            Value::String(text) => {
                if let Some(changed) = f(text) {
                    *text = changed;
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| map_strings(item, skip, f)),
            Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if !skip.contains(&key.as_str()) {
                        map_strings(item, skip, f);
                    }
                }
            }
            _ => {}
        }
    }

    /// Une date `AAAA-MM-JJ` de 2222 ou plus : la sentinelle de l'API (`2999-01-01`, ADR-0011).
    fn is_sentinel_date(text: &str) -> bool {
        let bytes = text.as_bytes();
        bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && text[..4].parse::<u32>().is_ok_and(|year| year >= 2222)
    }

    /// Retire de partout les entrées dont la valeur est une date sentinelle ; rend leur nombre.
    fn drop_sentinels(value: &mut Value) -> usize {
        match value {
            Value::Array(items) => items.iter_mut().map(drop_sentinels).sum(),
            Value::Object(map) => {
                let before = map.len();
                map.retain(|_, item| !item.as_str().is_some_and(is_sentinel_date));
                map.len().abs_diff(before) + map.values_mut().map(drop_sentinels).sum::<usize>()
            }
            _ => 0,
        }
    }

    /// Retire les valeurs `null` et les listes optionnelles vides (`nulls_omitted`) ; les listes
    /// principales (`hits`, `toc`) restent, même vides (05 §1.4 règle 1). Rend le nombre de retraits.
    fn drop_nulls(value: &mut Value) -> usize {
        match value {
            Value::Array(items) => items.iter_mut().map(drop_nulls).sum(),
            Value::Object(map) => {
                let before = map.len();
                map.retain(|key, item| {
                    let empty_optional = item.as_array().is_some_and(Vec::is_empty)
                        && !matches!(key.as_str(), "hits" | "toc");
                    !item.is_null() && !empty_optional
                });
                map.len().abs_diff(before) + map.values_mut().map(drop_nulls).sum::<usize>()
            }
            _ => 0,
        }
    }

    /// Retire `names` de l'objet de `value` et des objets de ses `hits` ; rend le nombre de retraits.
    fn drop_fields(value: &mut Value, names: &[&str]) -> usize {
        let mut dropped = 0;
        let mut strip = |map: &mut Map<String, Value>| {
            for name in names {
                dropped += usize::from(map.remove(*name).is_some());
            }
        };
        if let Some(map) = value.as_object_mut() {
            strip(map);
            if let Some(hits) = map.get_mut("hits").and_then(Value::as_array_mut) {
                for hit in hits.iter_mut().filter_map(Value::as_object_mut) {
                    strip(hit);
                }
            }
        }
        dropped
    }

    /// `title` retiré quand la citation le contient (`title_omitted_when_in_citation`).
    fn omit_title_in_citation(object: &mut Map<String, Value>) -> bool {
        let contained = match (object.get("title"), object.get("citation")) {
            (Some(Value::String(title)), Some(Value::String(citation))) => {
                citation.contains(title.as_str())
            }
            _ => false,
        };
        if contained {
            object.remove("title");
        }
        contained
    }

    /// Ce qu'une fixture a exercé : les clés des divergences de l'annexe A §4 appliquées.
    type Applied = BTreeSet<&'static str>;

    /// Le texte d'un golden jusqu'à la première marque ` [...]` (coupe de la fixture tronquée).
    fn before_cut_marker(text: &str) -> &str {
        text.find(" [...]").map_or(text, |at| &text[..at])
    }

    /// Compare `text` du golden à celui de la sortie (`long_text_structured`, annexe A §5.4).
    fn compare_long_text(fixture: &str, golden: &str, actual: &Value) {
        let rust = fold_ws(actual["text"].as_str().expect("a text"));
        let mut expected = fold_ws(golden);
        // Le texte canonique supprime `<RECH_ECLI>…</RECH_ECLI>` avec son contenu (03 §7.2), alors
        // que le `texte` de la référence finit par l'ECLI.
        if matches!(fixture, "consult_juri_constit" | "consult_juri_cetat") {
            let ecli = actual["ecli"].as_str().expect("an ECLI");
            expected = expected
                .strip_suffix(&format!(" {ecli}"))
                .ok_or_else(|| format!("{fixture}: the golden text does not end with {ecli}"))
                .unwrap()
                .to_owned();
        }
        if fixture == "consult_cnil" {
            // Fixture tronquée à deux endroits différents : préfixe commun jusqu'à la première
            // marque ` [...]` de l'un ou l'autre côté (annexe A §5.4, `long_text_structured`).
            let (a, b) = (before_cut_marker(&expected), before_cut_marker(&rust));
            let common = a.len().min(b.len());
            assert!(common > 500, "{fixture}: only {common} common bytes");
            assert_eq!(a[..common], b[..common], "{fixture}: the text prefix differs");
        } else {
            assert_eq!(rust, expected, "{fixture}: the long text differs");
        }
        // `text_length` compte les caractères du texte canonique (P-037) ; la fenêtre couvre tout.
        let length = actual["text_length"].as_u64().expect("a text_length");
        let chars = actual["text"].as_str().expect("a text").chars().count() as u64;
        assert_eq!(length, chars, "{fixture}: text_length counts the characters of the text");
        assert!(actual.get("next_offset").is_none(), "{fixture}: the window must cover the text");
    }

    /// Les onze transformations de l'annexe A §5.4 appliquées au golden `tool_output` de
    /// `fixture`, dans l'ordre, puis la comparaison à `actual`. Rend les divergences exercées.
    #[expect(
        clippy::too_many_lines,
        reason = "les onze transformations de l'annexe A §5.4, lues dans l'ordre du document"
    )]
    fn check_against_golden(case: &Case, golden: &Value, actual: &Value) -> Applied {
        let fixture = case.fixture;
        let tool = case.tool;
        let mut applied = Applied::new();
        let mut g = golden.clone();
        let mut a = actual.clone();

        // 1. mojibake_repaired : `repair_latin1_mojibake` sur chaque chaîne des six fixtures marquées.
        if is_mojibake(fixture) {
            let mut repaired = false;
            map_strings(&mut g, &[], &mut |text| {
                let fixed = repair_latin1_mojibake(text);
                (fixed != text).then(|| {
                    repaired = true;
                    fixed.into_owned()
                })
            });
            if repaired {
                applied.insert("mojibake_repaired");
            }
        }

        // 2. short_text_cleaned : `text::clean` sur les champs courts (tout sauf `text`).
        let mut cleaned = false;
        map_strings(&mut g, &["text"], &mut |text| {
            let fixed = clean(text);
            (fixed != text).then(|| {
                cleaned = true;
                fixed
            })
        });
        if cleaned {
            applied.insert("short_text_cleaned");
        }

        // 3. long_text_structured : `text` comparé en `LongText`, puis retiré des deux côtés.
        if let Some(Value::String(text)) = g.as_object_mut().and_then(|map| map.remove("text")) {
            compare_long_text(fixture, &text, &a);
            a.as_object_mut().expect("an object").remove("text");
            applied.insert("long_text_structured");
        }

        // 4. sentinel_dates_none.
        if drop_sentinels(&mut g) > 0 {
            applied.insert("sentinel_dates_none");
        }

        // 5. Overrides : les valeurs explicites du niveau 2.
        for (entry, pointer, expected, because) in OVERRIDES {
            if entry == fixture {
                let slot = g
                    .pointer_mut(pointer)
                    .ok_or_else(|| format!("{fixture}: no {pointer} to override ({because})"))
                    .unwrap();
                *slot = serde_json::from_str(expected).expect("a JSON override");
                applied.insert(because);
            }
        }

        // 6. toc_tree : les articles du golden sont la projection de la `toc` rendue.
        if let Some(Value::Array(articles)) =
            g.as_object_mut().and_then(|map| map.remove("articles"))
        {
            let text_etat = a.get("etat").cloned();
            let toc = a.as_object_mut().and_then(|map| map.remove("toc")).expect("a toc");
            let projected: Vec<Value> = toc
                .as_array()
                .expect("a toc list")
                .iter()
                .filter(|entry| entry.get("article_id").is_some())
                .map(|entry| {
                    // Un `etat` absent d'une entrée égale celui du texte (05 §5.0.4).
                    let etat = entry.get("etat").cloned().or_else(|| text_etat.clone());
                    json!({"article_id": entry["article_id"], "num": entry["num"], "etat": etat})
                })
                .collect();
            assert_eq!(Value::Array(projected), Value::Array(articles), "{fixture}: toc_tree");
            applied.insert("toc_tree");
        }

        // 7. citation_renamed, fields_renamed.
        let g_object = g.as_object_mut().expect("an object");
        if let Some(hits) = g_object.get_mut("hits").and_then(Value::as_array_mut) {
            for hit in hits.iter_mut().filter_map(Value::as_object_mut) {
                if let Some(value) = hit.remove("human_readable_citation") {
                    hit.insert("citation".to_owned(), value);
                    applied.insert("citation_renamed");
                }
            }
        }
        let mut rename = |from: &str, to: &str, kind: &'static str| {
            if let Some(value) = g_object.remove(from) {
                g_object.insert(to.to_owned(), value);
                applied.insert(kind);
            }
        };
        rename("human_readable_citation", "citation", "citation_renamed");
        if matches!(tool, "fr_get_decision" | "fr_get_deliberation") {
            rename("date_texte", "date", "fields_renamed");
        }
        if tool == "fr_get_convention" {
            rename("text_id", "convention_id", "fields_renamed");
            rename("date_parution", "date_publication", "fields_renamed");
        }
        if tool == "fr_get_deliberation" {
            // Le titre rendu est le titre long, s'il n'est pas contenu dans la citation (05 §5.5).
            if let Some(long) = g_object.remove("title_long").filter(|long| !long.is_null()) {
                g_object.insert("title".to_owned(), long);
            }
            applied.insert("fields_renamed");
        }

        // 8. title_omitted_when_in_citation : à la racine et dans chaque résultat.
        let mut omitted = omit_title_in_citation(g_object);
        if let Some(hits) = g_object.get_mut("hits").and_then(Value::as_array_mut) {
            for hit in hits.iter_mut().filter_map(Value::as_object_mut) {
                omitted |= omit_title_in_citation(hit);
            }
        }
        if omitted {
            applied.insert("title_omitted_when_in_citation");
        }

        // 9. eli_uri_dropped, notes_dropped, byte_size_to_text_length, search_echo_dropped,
        //    fields_not_rendered.
        for (names, kind) in [
            (&["eli_uri"][..], "eli_uri_dropped"),
            (&["eli_note", "dataset_note"][..], "notes_dropped"),
            (&["byte_size"][..], "byte_size_to_text_length"),
            (&["fond", "query"][..], "search_echo_dropped"),
            (not_rendered(tool), "fields_not_rendered"),
        ] {
            if drop_fields(&mut g, names) > 0 {
                applied.insert(kind);
            }
        }

        // 10. nulls_omitted.
        if drop_nulls(&mut g) > 0 {
            applied.insert("nulls_omitted");
        }

        // 11. fields_added : tout champ Rust absent du golden transformé figure dans la liste blanche.
        let extra = compare(fixture, "", &g, &a, added(tool));
        if extra > 0 {
            applied.insert("fields_added");
        }
        applied
    }

    /// Chaque clé de `golden` existe dans `actual` avec la même valeur ; chaque clé de `actual` en
    /// plus figure dans `added`. L'ordre des clés ne compte pas. Rend le nombre de champs ajoutés.
    fn compare(fixture: &str, at: &str, golden: &Value, actual: &Value, added: &[&str]) -> usize {
        match (golden, actual) {
            (Value::Object(g), Value::Object(a)) => {
                let mut extra = 0;
                for (key, expected) in g {
                    let found = a
                        .get(key)
                        .ok_or_else(|| format!("{fixture}: {at}/{key} is in the golden only"))
                        .unwrap();
                    extra += compare(fixture, &format!("{at}/{key}"), expected, found, added);
                }
                for key in a.keys().filter(|key| !g.contains_key(*key)) {
                    assert!(
                        added.contains(&key.as_str()),
                        "{fixture}: {at}/{key} is in the output but neither in the golden nor in \
                         the added fields"
                    );
                    extra += 1;
                }
                extra
            }
            (Value::Array(g), Value::Array(a)) => {
                assert_eq!(g.len(), a.len(), "{fixture}: {at} has a different length");
                g.iter()
                    .zip(a)
                    .enumerate()
                    .map(|(index, (g, a))| compare(fixture, &format!("{at}[{index}]"), g, a, added))
                    .sum()
            }
            _ => {
                assert_eq!(golden, actual, "{fixture}: {at} differs from the golden");
                0
            }
        }
    }

    fn golden_outputs() -> Map<String, Value> {
        let golden: Value = serde_json::from_slice(GOLDEN).expect("the golden is JSON");
        golden["tool_output"].as_object().expect("a tool_output section").clone()
    }

    /// Régression : P-004, P-038 (niveau 2 du harnais golden, deux ères).
    #[tokio::test(flavor = "multi_thread")]
    async fn tool_output_matches_golden() {
        let server = piste().await;
        let golden = golden_outputs();
        assert_eq!(golden.len(), 16);

        let legacy = run(&server, Era::Legacy, Case::golden_arguments).await;
        let modern = run(&server, Era::Modern, Case::golden_arguments).await;

        let mut applied = Applied::new();
        for ((case, legacy), modern) in CASES.iter().zip(&legacy).zip(&modern) {
            assert_eq!(legacy, modern, "{}: the two eras differ", case.fixture);
            let entry = golden
                .get(&format!("{}.json", case.fixture))
                .ok_or_else(|| format!("no golden for {}", case.fixture))
                .unwrap();
            applied.extend(check_against_golden(case, entry, legacy));
        }
        // Aucune divergence morte : chacune sert sur au moins une fixture.
        let missing: Vec<&str> =
            EXERCISED.iter().copied().filter(|key| !applied.contains(key)).collect();
        assert!(missing.is_empty(), "divergences never exercised by the 16 fixtures: {missing:?}");
    }

    /// Régression : D-04. Sur le scénario complet des deux ères, toute requête reçue par le faux
    /// PISTE vise un point d'accès connu de l'API, et le jeton n'est demandé qu'une fois par
    /// processus.
    #[tokio::test(flavor = "multi_thread")]
    async fn no_network_call_except_piste() {
        let server = piste().await;
        run(&server, Era::Legacy, Case::golden_arguments).await;
        run(&server, Era::Modern, Case::default_arguments).await;

        let known: BTreeSet<String> = ["/search", "/consult/lawDecree"]
            .into_iter()
            .chain(CASES.iter().map(|case| case.endpoint))
            .map(|endpoint| format!("{API_BASE}{endpoint}"))
            .chain([TOKEN_PATH.to_owned()])
            .collect();
        let requests = server.received_requests().await.expect("recorded requests");
        let mut by_path: BTreeMap<String, usize> = BTreeMap::new();
        for request in &requests {
            *by_path.entry(request.url.path().to_owned()).or_default() += 1;
            assert!(known.contains(request.url.path()), "unexpected request: {}", request.url);
        }
        assert_eq!(by_path[TOKEN_PATH], 2, "one token per process: {by_path:?}");
        // Seize appels par ère, chacun une requête (ni N+1, ni URL lue dans une réponse).
        assert_eq!(requests.len(), 2 + 2 * 16, "{by_path:?}");
    }

    // ---- budgets des sorties par défaut --------------------------------------------------------

    /// Colonne « Cible, appel par défaut » de 05 §6.5, en caractères de JSON compact du
    /// `structuredContent`. Trois recherches dépassent la cible du prototype (CODE 408, KALI 1 031,
    /// ACCO 821) : la sortie réelle porte l'extrait du résultat en plus de ceux de ses articles
    /// (CODE), les articles trouvés (KALI) et un extrait que le prototype n'a pas comptés (ACCO).
    /// Conformément à 06 §12.2 (« les valeurs mesurées en M5 remplacent la colonne Cible »), la
    /// valeur mesurée tient lieu de cible pour ces trois lignes ; 05 §6.5 est à mettre à jour en F6.
    const TARGETS: [(&str, usize); 16] = [
        ("search_loda", 782),
        ("search_code", 546),
        ("search_juri", 1_099),
        ("search_constit", 1_853),
        ("search_cetat", 1_501),
        ("search_cnil", 1_108),
        ("search_kali", 2_100),
        ("search_acco", 931),
        ("consult_loda", 6_035),
        ("consult_article", 686),
        ("consult_juri", 1_881),
        ("consult_juri_constit", 1_147),
        ("consult_juri_cetat", 1_171),
        ("consult_cnil", 1_099),
        ("consult_kali", 710),
        ("consult_acco", 664),
    ];

    /// Budget de 06 §12.2 : la cible plus 10 %.
    const fn budget(target: usize) -> usize {
        target + target / 10
    }

    /// Caractères du JSON compact de `value` (la mesure de 06 §12.3).
    fn compact_chars(value: &Value) -> usize {
        serde_json::to_string(value).expect("serializable").chars().count()
    }

    /// Régression : D-33. Chaque sortie par défaut tient dans la cible de 05 §6.5 plus 10 %, et les
    /// décisions CONSTIT et CETAT en résumé dans 2 000 caractères.
    #[tokio::test(flavor = "multi_thread")]
    async fn default_outputs_within_target() {
        let server = piste().await;
        let outputs = run(&server, Era::Legacy, Case::default_arguments).await;

        let mut over = Vec::new();
        for (case, output) in CASES.iter().zip(&outputs) {
            let (_, target) = TARGETS
                .iter()
                .find(|(fixture, _)| *fixture == case.fixture)
                .ok_or_else(|| format!("no target for {}", case.fixture))
                .unwrap();
            let target = *target;
            let size = compact_chars(output);
            if size > budget(target) {
                over.push(format!(
                    "{}: {size} > {} (cible {target} + 10 %)",
                    case.fixture,
                    budget(target)
                ));
            }
        }
        assert!(over.is_empty(), "default outputs over budget:\n{}", over.join("\n"));

        for fixture in ["consult_juri_constit", "consult_juri_cetat"] {
            let index = CASES.iter().position(|case| case.fixture == fixture).unwrap();
            let size = compact_chars(&outputs[index]);
            assert!(size <= 2_000, "{fixture}: {size} characters for a decision summary");
            assert!(outputs[index].get("summary").is_some(), "{fixture}: a summary is expected");
        }
    }

    // ---- catalogue de non-régression -----------------------------------------------------------

    /// Racine du dépôt (deux niveaux au-dessus de `crates/lf-app`).
    fn repository() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Tous les fichiers `.rs` sous `dir`, hors `target` et `.git`.
    fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target" && name != ".git") {
                    rust_files(&path, found);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    /// Les noms de fonction (`fn <nom>`) du code du dépôt : crates, xtask, fuzz et spikes. Les cibles
    /// de fuzz comptent aussi par le nom de leur fichier.
    fn implemented_names() -> BTreeSet<String> {
        let root = repository();
        let mut files = Vec::new();
        for dir in ["crates", "xtask", "fuzz", "spikes"] {
            rust_files(&root.join(dir), &mut files);
        }
        let mut names = BTreeSet::new();
        for file in files {
            let text = fs::read_to_string(&file).unwrap_or_default();
            let mut words = text
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|word| !word.is_empty());
            while let Some(word) = words.next() {
                if word == "fn" {
                    names.extend(words.next().map(str::to_owned));
                }
            }
        }
        names
    }

    /// Les jetons en code (`` `nom` ``) d'une cellule de tableau qui ressemblent à un nom de test.
    fn test_names(cell: &str) -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .filter(|token| {
                token.contains('_')
                    && token.starts_with(|c: char| c.is_ascii_lowercase())
                    && token
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            })
            .map(str::to_owned)
            .collect()
    }

    /// Les lignes d'un tableau markdown dont la première cellule commence par `prefix`, découpées en
    /// cellules.
    fn table_rows<'a>(section: &'a str, prefix: &str) -> Vec<Vec<&'a str>> {
        section
            .lines()
            .filter(|line| line.starts_with(prefix))
            .map(|line| line.trim().trim_matches('|').split('|').map(str::trim).collect())
            .collect()
    }

    /// Jetons du catalogue qui ne sont pas des tests Rust : (nom, ce que c'est).
    const NOT_RUST_TESTS: [(&str, &str); 4] = [
        ("changelog_has_entry_for_version", "contrôle du job de release (06 §13.3)"),
        ("dto_decode", "cible de fuzz (plan 2f), pas un test"),
        ("piste_request", "nom du span de 02 §11.1, pas un test"),
        ("toc_limit", "paramètre d'outil, pas un test"),
    ];

    /// Noms du catalogue livrés sous un autre nom : (nom du catalogue, test livré). Le catalogue sera
    /// aligné sur le code lors de la passe documentaire de F6 (D16 : aucune modification de
    /// `docs/plan` pendant les vagues) ; le test livré doit exister.
    const RENAMED: [(&str, &str); 11] = [
        ("audit_line_key_order_is_normative", "line_fields_in_order"),
        ("audit_record_schema_snapshot", "audit_line_validates_against_schema"),
        ("concurrent_writers_produce_valid_jsonl", "no_torn_lines_under_concurrent_writers"),
        ("two_writers_same_dir_interleave_whole_lines", "no_torn_lines_under_concurrent_writers"),
        ("rotation_renames_under_lock", "rotation_under_same_lock"),
        ("invalid_input_audited", "invalid_type_is_audited_invalid_arg"),
        ("focus_returns_relevant_paragraphs", "focus_returns_passages_with_offsets"),
        ("coverage_lists_known_gaps", "coverage_gaps_fr001_to_fr009"),
        ("has_more_computed_from_total", "has_more_false_at_reachable_window_end"),
        ("instructions_drift", "instructions_cite_exactly_the_tools_of_their_surface"),
        // La moitié « audit » de D-39 ; l'adapter PISTE n'a pas de test de ce nom (voir PENDING).
        ("new_does_no_io", "open_does_no_io_until_the_first_record"),
    ];

    /// Noms du catalogue que le code n'a pas encore et qui ne sont que des écarts de nom ou de
    /// document : (nom, ce que la passe documentaire de F6 tranche : écrire le test, ou aligner le
    /// catalogue sur un test existant). Un nom qui apparaît dans le code, ou qui quitte le
    /// catalogue, fait échouer le test : il faut alors le retirer d'ici.
    const PENDING: [(&str, &str); 11] = [
        ("article_9_has_two_paragraphs", "F6 : dépend de V-90 (texteHtml de getArticle)"),
        ("duration_includes_normalization", "F6 : prévu au plan 3b (façade), jamais livré"),
        ("get_act_on_code_uses_table_matieres", "F6 : conditionnel à V-96 (00 §8)"),
        ("law_article_url_uses_loda_article_lc", "F6 : règle d'URL après V-140 et V-142"),
        ("piste_request_span_fields", "F6 : span piste_request de 02 §11.1"),
        ("ported_search_loda_hits", "F6 : les 19 `ported_*` de D-30 et D-35 n'existent pas"),
        ("readme_mentions_only_supported_env_vars", "F6 : README de V3-10, sans test Rust"),
        ("recorded_fixtures_have_no_mojibake", "F6 : seul `recorded_fixtures_pass_integrity`"),
        (
            "tool_call_emits_one_info_event",
            "F6 : `tool_call_event_level_follows_the_outcome` couvre le niveau",
        ),
        ("tools_list_snapshot", "F6 : second filet derrière le verrou de V3-02"),
        ("versions_are_consistent", "F6 : paquet, serverInfo, verrou, server.json et manifeste"),
    ];

    /// Noms du catalogue qui portent une garantie de sécurité ou d'audit (D-04, D-05, D-07, D-16,
    /// D-18, D-30, D-38, D-40) et que **aucun test n'exerce encore** : F6 est une passe
    /// documentaire, qui n'écrit aucun test. Ils sont à écrire en F3 (plan §6), avant la revue de
    /// sécurité de F4. Tant que la liste n'est pas vide, `security_regressions_are_all_implemented`
    /// échoue (il est ignoré par défaut pour ne pas bloquer les vagues, et se lance avec
    /// `--run-ignored only -E 'test(security_regressions)'`).
    const SECURITY_PENDING: [(&str, &str); 10] = [
        (
            "default_env_is_sandbox",
            "F3 : D-18, le défaut sandbox de lf-app/config.rs n'a pas de test (V1-11)",
        ),
        ("every_tool_goes_through_audited", "F3 : D-07, une ligne d'audit par outil enregistré"),
        ("no_view_has_eli_uri", "F3 : D-30, aucune vue n'a de champ eli_uri"),
        ("ported_no_fabricated_eli", "F3 : D-30, un ELI vient de la source ou n'existe pas"),
        ("production_sources_contain_only_allowlisted_urls", "F3 : D-04, liste blanche d'URL"),
        ("record_returns_only_after_sync", "F3 : D-16, durabilité de JsonlAuditLog"),
        ("redact_url_drops_userinfo_and_query", "F3 : D-40, fonction de rédaction d'URL"),
        ("server_answers_discover_while_tool_waits_upstream", "F3 : D-05, transcription ou test"),
        ("startup_logs_sandbox_notice", "F3 : D-18, avertissement bac à sable sur stderr"),
        ("tool_calls_progress_while_audit_write_is_slow", "F3 : D-38"),
    ];

    /// Les noms que le code n'a pas encore, quelle qu'en soit la raison.
    fn pending_entries() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
        PENDING.iter().chain(SECURITY_PENDING.iter())
    }

    /// Régression : D-43, volet sécurité. Un catalogue « implémenté » ne dit rien d'un nom que
    /// `SECURITY_PENDING` excuse : cette garantie n'a alors aucune preuve.
    #[test]
    #[ignore = "F3 (plan §6) : tests de sécurité du catalogue à écrire, voir SECURITY_PENDING"]
    fn security_regressions_are_all_implemented() {
        let missing: Vec<String> =
            SECURITY_PENDING.iter().map(|(name, unit)| format!("{name} ({unit})")).collect();
        assert!(missing.is_empty(), "security regressions without a test:\n{}", missing.join("\n"));
    }

    /// Régression : D-43. Chaque nom de test du catalogue 06 §3.1 existe dans le code, ou est couvert
    /// par la table de correspondance « livré » du même paragraphe. Lit le document du dépôt.
    #[test]
    fn regression_catalog_is_implemented() {
        let document = repository().join("docs/plan/06-qualite.md");
        let text = fs::read_to_string(&document).map_err(|e| format!("{document:?}: {e}")).unwrap();
        let start = text.find("### 3.1 Défauts D-01").expect("the catalogue section");
        let end = text.find("### 3.2 Points de parité").expect("the end of the catalogue");
        let section = &text[start..end];

        // Les noms du catalogue (colonne « Test(s) de non-régression ») et leur défaut.
        let mut catalogue: BTreeMap<String, String> = BTreeMap::new();
        for row in table_rows(section, "| D-") {
            for name in test_names(row[2]) {
                catalogue.entry(name).or_insert_with(|| row[0].to_owned());
            }
        }
        assert!(catalogue.len() > 150, "only {} names parsed from the catalogue", catalogue.len());

        let implemented = implemented_names();
        // La table de correspondance « livré » : un nom du catalogue est couvert quand tous les tests
        // livrés qu'elle lui associe existent.
        let mut delivered: BTreeSet<String> = BTreeSet::new();
        for row in table_rows(section, "| `") {
            let tests = test_names(row[1]);
            if row.len() == 3 && !tests.is_empty() && tests.iter().all(|t| implemented.contains(t))
            {
                delivered.extend(test_names(row[0]));
            }
        }

        let covered = |name: &str| {
            implemented.contains(name)
                || delivered.contains(name)
                || NOT_RUST_TESTS.iter().any(|(not_test, _)| *not_test == name)
                || RENAMED.iter().any(|(old, new)| *old == name && implemented.contains(*new))
        };
        let pending: BTreeSet<&str> = pending_entries().map(|(name, _)| *name).collect();

        let unexplained: Vec<String> = catalogue
            .iter()
            .filter(|(name, _)| !covered(name) && !pending.contains(name.as_str()))
            .map(|(name, defect)| format!("{defect} {name}"))
            .collect();
        assert!(
            unexplained.is_empty(),
            "{} catalogue names are neither implemented nor listed as pending:\n{}",
            unexplained.len(),
            unexplained.join("\n")
        );

        // Les listes d'exceptions ne vieillissent pas : une entrée devenue inutile est retirée.
        let mut stale = Vec::new();
        for &(name, unit) in pending_entries() {
            if !catalogue.contains_key(name) || covered(name) {
                stale.push(format!("PENDING {name} ({unit}) is implemented or left the catalogue"));
            }
        }
        for (old, new) in RENAMED {
            if !catalogue.contains_key(old) || !implemented.contains(new) {
                stale.push(format!("RENAMED {old} -> {new} no longer holds"));
            }
        }
        for (name, _) in NOT_RUST_TESTS {
            if !catalogue.contains_key(name) {
                stale.push(format!("NOT_RUST_TESTS {name} left the catalogue"));
            }
        }
        assert!(stale.is_empty(), "stale exceptions:\n{}", stale.join("\n"));
    }

    // ---- instantanés ---------------------------------------------------------------------------

    fn snapshot_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("snapshots")
    }

    /// Compare `value` à `tests/snapshots/parity_tool_output__<name>.snap` (même format qu'`insta` :
    /// un en-tête YAML, puis le JSON). `LF_UPDATE_SNAPSHOTS=1` écrit les fichiers au lieu de comparer.
    #[expect(
        clippy::disallowed_methods,
        reason = "test : l'environnement du test commande l'écriture des instantanés"
    )]
    fn assert_snapshot(name: &str, value: &Value) {
        let file = snapshot_dir().join(format!("parity_tool_output__{name}.snap"));
        let body = serde_json::to_string_pretty(value).expect("serializable");
        let expected = format!(
            "---\nsource: crates/lf-app/tests/parity_tool_output.rs\nexpression: {name}\n---\n{body}\n"
        );
        if std::env::var_os("LF_UPDATE_SNAPSHOTS").is_some() {
            fs::create_dir_all(snapshot_dir()).expect("the snapshot directory");
            fs::write(&file, &expected).expect("the snapshot is written");
            return;
        }
        let stored = fs::read_to_string(&file)
            .map_err(|error| format!("{file:?}: {error}; run with LF_UPDATE_SNAPSHOTS=1"))
            .unwrap();
        // Seul le corps compte : l'en-tête peut recevoir des champs de plus (`assertion_line`).
        let body_of = |snapshot: &str| snapshot.splitn(3, "---\n").nth(2).unwrap_or("").to_owned();
        assert_eq!(
            body_of(&stored),
            body_of(&expected),
            "{file:?} differs; review it, then run with LF_UPDATE_SNAPSHOTS=1"
        );
    }

    /// « Aujourd'hui » à Paris est la date par défaut de `fr_get_act` : le seul champ qui change
    /// d'un jour à l'autre.
    fn stable(case: &Case, output: &Value) -> Value {
        let mut output = output.clone();
        if case.tool == "fr_get_act" {
            output["date"] = json!("[today in Paris]");
        }
        output
    }

    /// Régression : P-004. Les seize sorties par défaut, figées.
    #[tokio::test(flavor = "multi_thread")]
    async fn default_outputs_match_snapshots() {
        let server = piste().await;
        let outputs = run(&server, Era::Legacy, Case::default_arguments).await;
        for (case, output) in CASES.iter().zip(&outputs) {
            assert_snapshot(case.fixture, &stable(case, output));
        }
    }
}
