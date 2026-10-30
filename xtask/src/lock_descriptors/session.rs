//! Sessions JSON-RPC sur le binaire (05 §9.2 points 2 et 3) : ère moderne, puis ères legacy.
//!
//! Le pilote ne connaît que le trait [`Transport`] : lignes JSON envoyées et reçues. Le vrai
//! transport (processus fils, stdio) est dans `process.rs` ; les tests branchent un serveur
//! synthétique en mémoire.

use serde_json::{Value, json};

use super::surface::Capture;

/// Version de l'ère moderne (`server/discover`, `_meta` sur chaque requête).
pub(crate) const MODERN_VERSION: &str = "2026-07-28";

/// Clé de `_meta` qui porte `serverInfo` dans un résultat moderne (annexe D §3.2).
const SERVER_INFO_META: &str = "io.modelcontextprotocol/serverInfo";

/// Nombre maximal de pages suivies par liste : le serveur n'en rend qu'une (05 §9.2 point 4).
const MAX_PAGES: usize = 16;

/// Une des trois sessions du verrou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionKind {
    /// `server/discover` puis les listes, `_meta` 2026-07-28 sur chaque requête.
    Modern,
    /// `initialize` à la version donnée, `notifications/initialized`, puis les listes.
    Legacy(&'static str),
}

/// Les trois sessions, dans l'ordre de 05 §9.2.
pub(crate) const SESSIONS: [SessionKind; 3] =
    [SessionKind::Modern, SessionKind::Legacy("2025-11-25"), SessionKind::Legacy("2025-06-18")];

impl SessionKind {
    /// Nom de la session dans les messages.
    pub(crate) fn label(self) -> String {
        match self {
            Self::Modern => format!("discover {MODERN_VERSION}"),
            Self::Legacy(version) => format!("initialize {version}"),
        }
    }
}

/// Canal de lignes JSON avec le serveur.
pub(crate) trait Transport {
    /// Envoie un message JSON-RPC.
    fn send(&mut self, message: &Value) -> Result<(), String>;
    /// Prochain message JSON du serveur.
    fn receive(&mut self) -> Result<Value, String>;
}

struct Client<'a> {
    transport: &'a mut dyn Transport,
    next_id: u64,
}

impl Client<'_> {
    /// Envoie une requête et rend son `result`. Les notifications du serveur sont ignorées ;
    /// une requête du serveur au client est une erreur (aucune n'est attendue, 05 §2.4).
    fn request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.transport
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        loop {
            let message = self.transport.receive().map_err(|error| format!("{method}: {error}"))?;
            if let Some(from_server) = message.get("method").and_then(Value::as_str) {
                if message.get("id").is_some() {
                    return Err(format!(
                        "{method}: the server sent the request `{from_server}` to the client"
                    ));
                }
                continue;
            }
            if message.get("id") != Some(&json!(id)) {
                return Err(format!(
                    "{method}: response with an unexpected id: {}",
                    message.get("id").unwrap_or(&Value::Null)
                ));
            }
            if let Some(error) = message.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or_default();
                let text = error.get("message").and_then(Value::as_str).unwrap_or("(no message)");
                return Err(format!("{method}: JSON-RPC error {code}: {text}"));
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| format!("{method}: response without result"));
        }
    }

    /// Liste paginée : suit `nextCursor` jusqu'à épuisement et aplatit les pages.
    #[expect(
        clippy::indexing_slicing,
        reason = "`params` est un objet JSON construit par l'appelant ; l'écriture y ajoute une clé"
    )]
    fn list(&mut self, method: &str, key: &str, base: &Value) -> Result<Vec<Value>, String> {
        let mut items = Vec::new();
        let mut params = base.clone();
        for _ in 0..MAX_PAGES {
            let page = self.request(method, &params)?;
            let batch = page
                .get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{method}: the result has no `{key}` array"))?;
            items.extend(batch.iter().cloned());
            match page.get("nextCursor").and_then(Value::as_str) {
                Some(cursor) => params["cursor"] = json!(cursor),
                None => return Ok(items),
            }
        }
        Err(format!("{method}: more than {MAX_PAGES} pages"))
    }
}

/// `_meta` d'une requête moderne : version, informations et capacités du client (annexe D §3.1).
#[expect(
    clippy::indexing_slicing,
    reason = "`params` est un objet JSON construit par l'appelant ; l'écriture y ajoute une clé"
)]
fn modern_params(extra: Value) -> Value {
    let mut params = extra;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
        "io.modelcontextprotocol/clientInfo": {"name": "xtask-lock-descriptors", "version": "0"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    params
}

fn text(info: &Value, field: &str) -> Result<String, String> {
    info.get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("serverInfo.{field} is missing"))
}

/// Joue une session complète et rend ce qu'elle a lu.
///
/// # Errors
///
/// Le motif de l'échec, préfixé par le nom de la session : erreur de transport, erreur JSON-RPC,
/// réponse mal formée, version négociée différente de celle demandée.
pub(crate) fn run_session(
    transport: &mut dyn Transport,
    kind: SessionKind,
) -> Result<Capture, String> {
    play(transport, kind).map_err(|error| format!("{}: {error}", kind.label()))
}

fn play(transport: &mut dyn Transport, kind: SessionKind) -> Result<Capture, String> {
    let mut client = Client { transport, next_id: 0 };
    let modern = kind == SessionKind::Modern;
    let opening = match kind {
        SessionKind::Modern => client.request("server/discover", &modern_params(json!({})))?,
        SessionKind::Legacy(version) => {
            let reply = client.request(
                "initialize",
                &json!({
                    "protocolVersion": version,
                    "capabilities": {},
                    "clientInfo": {"name": "xtask-lock-descriptors", "version": "0"},
                }),
            )?;
            let negotiated = reply.get("protocolVersion").and_then(Value::as_str);
            if negotiated != Some(version) {
                return Err(format!("the server negotiated {negotiated:?} instead of {version}"));
            }
            client
                .transport
                .send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
            reply
        }
    };
    let info = if modern {
        opening.get("_meta").and_then(|meta| meta.get(SERVER_INFO_META))
    } else {
        opening.get("serverInfo")
    }
    .ok_or("serverInfo is missing")?;
    let supported_versions = if modern {
        let versions = opening
            .get("supportedVersions")
            .and_then(Value::as_array)
            .ok_or("`supportedVersions` is missing")?;
        Some(
            versions
                .iter()
                .map(|version| version.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or("`supportedVersions` holds a non-string")?,
        )
    } else {
        None
    };
    let (server_name, server_version) = (text(info, "name")?, text(info, "version")?);
    let capabilities = opening.get("capabilities").cloned().ok_or("`capabilities` is missing")?;
    let base = if modern { modern_params(json!({})) } else { json!({}) };
    let declared = |name: &str| capabilities.get(name).is_some();
    let tools = client.list("tools/list", "tools", &base)?;
    let prompts = if declared("prompts") {
        client.list("prompts/list", "prompts", &base)?
    } else {
        Vec::new()
    };
    let (resources, resource_templates) = if declared("resources") {
        (
            client.list("resources/list", "resources", &base)?,
            client.list("resources/templates/list", "resourceTemplates", &base)?,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    Ok(Capture {
        label: kind.label(),
        server_name,
        server_version,
        supported_versions,
        capabilities,
        instructions: opening.get("instructions").and_then(Value::as_str).map(str::to_owned),
        tools,
        prompts,
        resources,
        resource_templates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_descriptors::fixtures::{FakeServer, VERSION, VERSIONS, tool};
    use pretty_assertions::assert_eq;
    use std::collections::VecDeque;

    /// Transport en mémoire devant un [`FakeServer`] ; garde les requêtes envoyées.
    struct Loopback {
        server: FakeServer,
        sent: Vec<Value>,
        queue: VecDeque<Value>,
        /// Messages à faire précéder les réponses (notifications, requêtes du serveur).
        inject: Vec<Value>,
    }

    impl Loopback {
        fn new(server: FakeServer) -> Self {
            Self { server, sent: Vec::new(), queue: VecDeque::new(), inject: Vec::new() }
        }

        fn methods(&self) -> Vec<&str> {
            self.sent.iter().filter_map(|message| message["method"].as_str()).collect()
        }
    }

    impl Transport for Loopback {
        fn send(&mut self, message: &Value) -> Result<(), String> {
            self.sent.push(message.clone());
            self.queue.extend(self.inject.drain(..));
            self.queue.extend(self.server.handle(message));
            Ok(())
        }

        fn receive(&mut self) -> Result<Value, String> {
            self.queue.pop_front().ok_or_else(|| "the server closed its output".to_owned())
        }
    }

    fn server() -> FakeServer {
        FakeServer::new(&[tool("fr_a"), tool("fr_b"), tool("fr_c")])
    }

    #[test]
    fn the_modern_session_discovers_then_lists_with_the_meta_block() {
        let mut full = server();
        full.capabilities = json!({"tools": {}, "prompts": {}, "resources": {}});
        full.prompts = vec![json!({"name": "p"})];
        full.resources = vec![json!({"uri": "u"})];
        full.templates = vec![json!({"uriTemplate": "t"})];
        let mut link = Loopback::new(full);
        let capture = run_session(&mut link, SessionKind::Modern).unwrap();
        assert_eq!(
            link.methods(),
            [
                "server/discover",
                "tools/list",
                "prompts/list",
                "resources/list",
                "resources/templates/list"
            ]
        );
        for request in &link.sent {
            let meta = &request["params"]["_meta"];
            assert_eq!(meta["io.modelcontextprotocol/protocolVersion"], "2026-07-28");
            assert_eq!(meta["io.modelcontextprotocol/clientCapabilities"], json!({}));
            assert_eq!(request["jsonrpc"], "2.0");
        }
        assert_eq!(
            link.sent.iter().map(|request| request["id"].clone()).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        assert_eq!(capture.label, "discover 2026-07-28");
        assert_eq!(
            (capture.server_name.as_str(), capture.server_version.as_str()),
            ("mcp-legifrance", VERSION)
        );
        assert_eq!(capture.supported_versions, Some(VERSIONS.map(str::to_owned).to_vec()));
        assert_eq!(capture.capabilities, json!({"tools": {}, "prompts": {}, "resources": {}}));
        assert_eq!(
            capture.instructions.as_deref(),
            Some("French law from Légifrance. Start with fr_search.")
        );
        assert_eq!(capture.tools.len(), 3);
        assert_eq!(capture.prompts, [json!({"name": "p"})]);
        assert_eq!(capture.resources, [json!({"uri": "u"})]);
        assert_eq!(capture.resource_templates, [json!({"uriTemplate": "t"})]);
    }

    #[test]
    fn a_session_only_lists_what_the_capabilities_declare() {
        let mut link = Loopback::new(server());
        let capture = run_session(&mut link, SessionKind::Modern).unwrap();
        assert_eq!(link.methods(), ["server/discover", "tools/list"]);
        assert_eq!(
            (capture.prompts.len(), capture.resources.len(), capture.resource_templates.len()),
            (0, 0, 0)
        );
    }

    #[test]
    fn a_legacy_session_initializes_announces_itself_then_lists_without_meta() {
        for (kind, version) in [
            (SessionKind::Legacy("2025-11-25"), "2025-11-25"),
            (SessionKind::Legacy("2025-06-18"), "2025-06-18"),
        ] {
            let mut link = Loopback::new(server());
            let capture = run_session(&mut link, kind).unwrap();
            assert_eq!(link.methods(), ["initialize", "notifications/initialized", "tools/list"]);
            assert_eq!(link.sent[0]["params"]["protocolVersion"], version);
            assert_eq!(link.sent[0]["params"]["capabilities"], json!({}));
            assert_eq!(link.sent[1].get("id"), None, "une notification n'a pas d'id");
            assert_eq!(link.sent[2]["params"], json!({}));
            assert_eq!(capture.label, format!("initialize {version}"));
            assert_eq!(capture.supported_versions, None);
            assert_eq!(capture.tools.len(), 3);
        }
    }

    #[test]
    fn pages_are_followed_and_flattened() {
        let mut paged = server();
        paged.page = 2;
        for kind in [SessionKind::Modern, SessionKind::Legacy("2025-11-25")] {
            let mut link = Loopback::new(paged.clone());
            let capture = run_session(&mut link, kind).unwrap();
            assert_eq!(
                capture.tools.iter().map(|t| t["name"].as_str().unwrap()).collect::<Vec<_>>(),
                ["fr_a", "fr_b", "fr_c"]
            );
            let lists: Vec<&Value> =
                link.sent.iter().filter(|r| r["method"] == "tools/list").collect();
            assert_eq!(lists.len(), 2);
            assert_eq!(lists[0]["params"].get("cursor"), None);
            assert_eq!(lists[1]["params"]["cursor"], "2");
        }
        let mut endless = Loopback::new(paged);
        endless.server.tools = (0..40).map(|n| tool(&format!("fr_t{n}"))).collect();
        endless.server.page = 1;
        let error = run_session(&mut endless, SessionKind::Modern).unwrap_err();
        assert_eq!(error, "discover 2026-07-28: tools/list: more than 16 pages");
    }

    #[test]
    fn notifications_from_the_server_are_skipped_but_server_requests_are_refused() {
        let mut link = Loopback::new(server());
        link.inject =
            vec![json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {}})];
        assert_eq!(run_session(&mut link, SessionKind::Modern).unwrap().tools.len(), 3);
        let mut asked = Loopback::new(server());
        asked.inject = vec![json!({"jsonrpc": "2.0", "id": 77, "method": "roots/list"})];
        assert_eq!(
            run_session(&mut asked, SessionKind::Modern).unwrap_err(),
            "discover 2026-07-28: server/discover: the server sent the request `roots/list` to the client"
        );
    }

    /// Transport dont les réponses sont écrites d'avance.
    struct Scripted {
        replies: VecDeque<Result<Value, String>>,
        /// Écritures déjà faites.
        sent: usize,
        /// `(rang, erreur)` : l'écriture de ce rang (à partir de 0) échoue.
        send_error: Option<(usize, String)>,
    }

    impl Scripted {
        fn replying(replies: Vec<Value>) -> Self {
            Self { replies: replies.into_iter().map(Ok).collect(), sent: 0, send_error: None }
        }

        fn failing_to_send_at(mut self, rank: usize, error: &str) -> Self {
            self.send_error = Some((rank, error.to_owned()));
            self
        }
    }

    impl Transport for Scripted {
        fn send(&mut self, _: &Value) -> Result<(), String> {
            let rank = self.sent;
            self.sent += 1;
            match &self.send_error {
                Some((at, error)) if *at == rank => Err(error.clone()),
                _ => Ok(()),
            }
        }

        fn receive(&mut self) -> Result<Value, String> {
            self.replies
                .pop_front()
                .unwrap_or_else(|| Err("the server closed its output".to_owned()))
        }
    }

    fn answer(id: u64, result: Value) -> Value {
        let mut message = json!({"jsonrpc": "2.0", "id": id});
        message["result"] = result;
        message
    }

    #[test]
    fn protocol_failures_name_the_session_and_the_method() {
        let discover = |reply: Value| {
            run_session(&mut Scripted::replying(vec![reply]), SessionKind::Modern).unwrap_err()
        };
        assert_eq!(
            discover(answer(9, json!({}))),
            "discover 2026-07-28: server/discover: response with an unexpected id: 9"
        );
        assert_eq!(
            discover(json!({"jsonrpc": "2.0", "result": {}})),
            "discover 2026-07-28: server/discover: response with an unexpected id: null"
        );
        let error = json!({"code": -32022, "message": "bad version", "data": {"secret": 1}});
        assert_eq!(
            discover(json!({"jsonrpc": "2.0", "id": 1, "error": error})),
            "discover 2026-07-28: server/discover: JSON-RPC error -32022: bad version"
        );
        assert_eq!(
            discover(json!({"jsonrpc": "2.0", "id": 1, "error": {}})),
            "discover 2026-07-28: server/discover: JSON-RPC error 0: (no message)"
        );
        assert_eq!(
            discover(json!({"jsonrpc": "2.0", "id": 1})),
            "discover 2026-07-28: server/discover: response without result"
        );
        let mut mute = Scripted::replying(Vec::new());
        mute.replies.push_back(Err("timed out".to_owned()));
        assert_eq!(
            run_session(&mut mute, SessionKind::Legacy("2025-06-18")).unwrap_err(),
            "initialize 2025-06-18: initialize: timed out"
        );
        let mut broken = Scripted::replying(Vec::new()).failing_to_send_at(0, "broken pipe");
        assert_eq!(
            run_session(&mut broken, SessionKind::Modern).unwrap_err(),
            "discover 2026-07-28: broken pipe"
        );
        let mut closed = Scripted::replying(Vec::new());
        assert_eq!(
            run_session(&mut closed, SessionKind::Modern).unwrap_err(),
            "discover 2026-07-28: server/discover: the server closed its output"
        );
    }

    #[test]
    fn malformed_openings_and_lists_are_reported() {
        let info = json!({"name": "mcp-legifrance", "version": "0.1.0"});
        let opening = |patch: &dyn Fn(&mut Value)| {
            let mut result = json!({
                "supportedVersions": ["2026-07-28"],
                "capabilities": {"tools": {}},
                "_meta": {SERVER_INFO_META: info.clone()},
            });
            patch(&mut result);
            run_session(&mut Scripted::replying(vec![answer(1, result)]), SessionKind::Modern)
                .unwrap_err()
        };
        let expect = |patch: &dyn Fn(&mut Value), message: &str| {
            assert_eq!(opening(patch), format!("discover 2026-07-28: {message}"));
        };
        expect(&|r| r["_meta"] = json!({}), "serverInfo is missing");
        expect(
            &|r| r["_meta"][SERVER_INFO_META] = json!({"name": "x"}),
            "serverInfo.version is missing",
        );
        expect(
            &|r| r["_meta"][SERVER_INFO_META] = json!({"version": "1"}),
            "serverInfo.name is missing",
        );
        expect(
            &|r| {
                r.as_object_mut().unwrap().remove("supportedVersions");
            },
            "`supportedVersions` is missing",
        );
        expect(&|r| r["supportedVersions"] = json!([1]), "`supportedVersions` holds a non-string");
        expect(
            &|r| {
                r.as_object_mut().unwrap().remove("capabilities");
            },
            "`capabilities` is missing",
        );
        // La liste d'outils sans tableau `tools`.
        let bare = json!({"supportedVersions": ["2026-07-28"], "capabilities": {}, "_meta": {SERVER_INFO_META: info}});
        let mut script =
            Scripted::replying(vec![answer(1, bare), answer(2, json!({"tools": "nope"}))]);
        assert_eq!(
            run_session(&mut script, SessionKind::Modern).unwrap_err(),
            "discover 2026-07-28: tools/list: the result has no `tools` array"
        );
        // Un `initialize` qui négocie une autre version, ou qui oublie `serverInfo`.
        let legacy = |result: Value| {
            run_session(
                &mut Scripted::replying(vec![answer(1, result)]),
                SessionKind::Legacy("2025-11-25"),
            )
            .unwrap_err()
        };
        assert_eq!(
            legacy(json!({"protocolVersion": "2025-06-18", "capabilities": {}})),
            "initialize 2025-11-25: the server negotiated Some(\"2025-06-18\") instead of 2025-11-25"
        );
        assert_eq!(
            legacy(json!({"protocolVersion": "2025-11-25", "capabilities": {}})),
            "initialize 2025-11-25: serverInfo is missing"
        );
        assert_eq!(
            legacy(json!({"capabilities": {}})),
            "initialize 2025-11-25: the server negotiated None instead of 2025-11-25"
        );
    }

    #[test]
    fn a_list_that_fails_names_its_method() {
        let info = json!({"name": "mcp-legifrance", "version": "0.1.0"});
        let opening = |capabilities: Value| {
            answer(
                1,
                json!({
                    "supportedVersions": ["2026-07-28"],
                    "capabilities": capabilities,
                    "_meta": {SERVER_INFO_META: info.clone()},
                }),
            )
        };
        let tools = answer(2, json!({"tools": []}));
        let resources = answer(3, json!({"resources": []}));
        let cases = [
            (json!({}), vec![], "tools/list"),
            (json!({"prompts": {}}), vec![tools.clone()], "prompts/list"),
            (json!({"resources": {}}), vec![tools.clone()], "resources/list"),
            (json!({"resources": {}}), vec![tools, resources], "resources/templates/list"),
        ];
        for (capabilities, replies, method) in cases {
            let mut script = Scripted::replying(vec![opening(capabilities)]);
            script.replies.extend(replies.into_iter().map(Ok));
            assert_eq!(
                run_session(&mut script, SessionKind::Modern).unwrap_err(),
                format!("discover 2026-07-28: {method}: the server closed its output")
            );
        }
    }

    #[test]
    fn a_legacy_session_that_cannot_announce_itself_is_reported() {
        let hello = json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "serverInfo": {"name": "mcp-legifrance", "version": "0.1.0"},
        });
        let mut script =
            Scripted::replying(vec![answer(1, hello)]).failing_to_send_at(1, "broken pipe");
        assert_eq!(
            run_session(&mut script, SessionKind::Legacy("2025-11-25")).unwrap_err(),
            "initialize 2025-11-25: broken pipe"
        );
    }

    #[test]
    fn the_loopback_reports_a_closed_output_when_nothing_is_queued() {
        let mut link = Loopback::new(server());
        assert_eq!(link.receive().unwrap_err(), "the server closed its output");
    }
}
