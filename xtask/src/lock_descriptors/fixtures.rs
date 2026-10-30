//! Données synthétiques des tests : un serveur de trois outils sans rapport avec le vrai binaire.

use std::{
    env,
    ffi::OsString,
    io::{self, BufRead, Write},
    process, thread,
    time::Duration,
};

use serde_json::{Value, json};

use super::{
    process::Launch,
    surface::{Capture, SERVER_NAME, Surface},
};

/// Version du crate dans les tests.
pub(crate) const VERSION: &str = "0.1.0";

/// Les trois versions de protocole servies (00 §6).
pub(crate) const VERSIONS: [&str; 3] = ["2026-07-28", "2025-11-25", "2025-06-18"];

/// Descripteur conforme à tous les contrôles de 05 §9.2 point 8.
pub(crate) fn tool(name: &str) -> Value {
    let title = format!("Title of {name}");
    json!({
        "name": name,
        "title": title,
        "description": "Searches the synthetic corpus. Returns at most ten hits. Needs a query.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Words to look for.", "minLength": 1}
            },
            "required": ["query"],
            "additionalProperties": false
        },
        "outputSchema": {
            "type": "object",
            "properties": {"hits": {"type": "array", "items": {"type": "string"}}},
            "required": ["hits"]
        },
        "annotations": {
            "title": title,
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": true
        }
    })
}

/// Session qui a lu `tools` sur un serveur conforme.
pub(crate) fn capture(label: &str, tools: &[Value]) -> Capture {
    Capture {
        label: label.to_owned(),
        server_name: SERVER_NAME.to_owned(),
        server_version: VERSION.to_owned(),
        supported_versions: None,
        capabilities: json!({"tools": {}}),
        instructions: Some("French law from Légifrance. Start with fr_search.".to_owned()),
        tools: tools.to_vec(),
        prompts: Vec::new(),
        resources: Vec::new(),
        resource_templates: Vec::new(),
    }
}

/// Surface de deux outils, ères identiques.
pub(crate) fn surface() -> Surface {
    let tools = [tool("fr_search"), tool("fr_coverage")];
    let mut modern = capture("discover 2026-07-28", &tools);
    modern.supported_versions = Some(VERSIONS.map(str::to_owned).to_vec());
    Surface::from_captures(&[modern, capture("initialize 2025-11-25", &tools)]).unwrap()
}

/// Serveur MCP synthétique, à la forme des réponses de 05 §9.2 : de quoi éprouver les sessions sans
/// binaire. L'ère se lit comme le fait rmcp, à la présence de `_meta` dans les paramètres.
#[derive(Clone)]
pub(crate) struct FakeServer {
    pub tools: Vec<Value>,
    pub prompts: Vec<Value>,
    pub resources: Vec<Value>,
    pub templates: Vec<Value>,
    pub capabilities: Value,
    pub version: String,
    /// Éléments par page de liste (0 : une seule page).
    pub page: usize,
}

impl FakeServer {
    pub(crate) fn new(tools: &[Value]) -> Self {
        Self {
            tools: tools.to_vec(),
            prompts: Vec::new(),
            resources: Vec::new(),
            templates: Vec::new(),
            capabilities: json!({"tools": {}}),
            version: VERSION.to_owned(),
            page: 0,
        }
    }

    fn info(&self) -> Value {
        json!({"name": SERVER_NAME, "title": "Synthetic", "version": self.version})
    }

    fn list(&self, items: &[Value], key: &str, params: &Value, modern: bool) -> Value {
        let start =
            params["cursor"].as_str().and_then(|cursor| cursor.parse::<usize>().ok()).unwrap_or(0);
        let end = if self.page == 0 { items.len() } else { (start + self.page).min(items.len()) };
        let mut result = json!({ key: items[start..end] });
        if end < items.len() {
            result["nextCursor"] = json!(end.to_string());
        }
        if modern {
            result["resultType"] = json!("complete");
            result["ttlMs"] = json!(3_600_000);
            result["cacheScope"] = json!("public");
        }
        result
    }

    /// Réponses (zéro ou une) à `message`.
    pub(crate) fn handle(&self, message: &Value) -> Vec<Value> {
        let (Some(id), Some(method)) = (message.get("id"), message["method"].as_str()) else {
            return Vec::new();
        };
        let params = &message["params"];
        let modern = params.get("_meta").is_some();
        let result = match method {
            "server/discover" => json!({
                "resultType": "complete",
                "supportedVersions": VERSIONS,
                "capabilities": self.capabilities,
                "instructions": "French law from Légifrance. Start with fr_search.",
                "ttlMs": 3_600_000,
                "cacheScope": "public",
                "_meta": {"io.modelcontextprotocol/serverInfo": self.info()},
            }),
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"],
                "capabilities": self.capabilities,
                "serverInfo": self.info(),
                "instructions": "French law from Légifrance. Start with fr_search.",
            }),
            "tools/list" => self.list(&self.tools, "tools", params, modern),
            "prompts/list" => self.list(&self.prompts, "prompts", params, modern),
            "resources/list" => self.list(&self.resources, "resources", params, modern),
            "resources/templates/list" => {
                self.list(&self.templates, "resourceTemplates", params, modern)
            }
            other => {
                let error =
                    json!({"code": -32601, "message": format!("method not found: {other}")});
                return vec![json!({"jsonrpc": "2.0", "id": id, "error": error})];
            }
        };
        vec![json!({"jsonrpc": "2.0", "id": id, "result": result})]
    }
}

/// Arguments qui font jouer au binaire de test le rôle d'un pair (voir [`peer_main`]).
pub(crate) fn peer_args(mode: &str) -> Vec<OsString> {
    let filter = format!("LOCK_PEER={mode}");
    ["--exact", "lock_descriptors::fixtures::peer_main", filter.as_str(), "--nocapture"]
        .map(OsString::from)
        .to_vec()
}

/// Lancement du binaire de test comme pair `mode`. L'environnement du fils est vidé, sauf le profil
/// de couverture : sans lui, ce que le fils exécute ne compterait pas dans la couverture.
#[expect(
    clippy::disallowed_methods,
    reason = "LLVM_PROFILE_FILE : la couverture du fils doit rejoindre celle du parent"
)]
pub(crate) fn peer(mode: &str) -> Launch {
    let mut launch = Launch::serve(env::current_exe().unwrap());
    launch.args = peer_args(mode);
    launch.env.extend(
        env::var_os("LLVM_PROFILE_FILE")
            .map(|profile| (OsString::from("LLVM_PROFILE_FILE"), profile)),
    );
    launch.read_timeout = Duration::from_secs(20);
    launch.exit_grace = Duration::from_secs(10);
    launch
}

/// Faux pair : le binaire de test relancé sur ce seul test, avec `LOCK_PEER=<mode>` en filtre
/// supplémentaire. Lancé sans ce filtre (cas ordinaire), le test ne fait rien.
///
/// Modes : `server` (sert les sessions), `noisy` (une ligne parasite d'abord), `silent` (lit sans
/// jamais répondre), `sleep=<ms>` (sert, puis attend après l'EOF avant de quitter), `fail` (sert,
/// puis quitte avec le code 3 à l'EOF), `cargo` (annonce son propre exécutable comme `cargo build`
/// et quitte sans rien lire).
#[test]
#[expect(clippy::exit, reason = "le mode `fail` doit quitter avec un code non nul")]
fn peer_main() {
    let Some(mode) = env::args().find_map(|arg| arg.strip_prefix("LOCK_PEER=").map(str::to_owned))
    else {
        return;
    };
    let mut out = io::stdout().lock();
    if mode == "cargo" {
        let artifact = json!({
            "reason": "compiler-artifact",
            "target": {"name": "mcp-legifrance", "kind": ["bin"]},
            "executable": env::current_exe().unwrap(),
        });
        writeln!(out, "{artifact}").unwrap();
        return;
    }
    if mode == "noisy" {
        writeln!(out, "banner that is not JSON-RPC").unwrap();
    }
    let server = FakeServer::new(&[tool("fr_search"), tool("fr_coverage")]);
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        let message: Value = serde_json::from_str(&line).unwrap();
        let replies = if mode == "silent" { Vec::new() } else { server.handle(&message) };
        for reply in replies {
            writeln!(out, "{reply}").unwrap();
            out.flush().unwrap();
        }
    }
    if let Some(millis) = mode.strip_prefix("sleep=") {
        thread::sleep(Duration::from_millis(millis.parse().unwrap()));
    }
    if mode == "fail" {
        process::exit(3);
    }
}

#[test]
fn the_fake_server_answers_unknown_methods_with_a_json_rpc_error() {
    let message = json!({"jsonrpc": "2.0", "id": 4, "method": "nope", "params": {}});
    assert_eq!(
        FakeServer::new(&[]).handle(&message),
        [json!({
            "jsonrpc": "2.0",
            "id": 4,
            "error": {"code": -32601, "message": "method not found: nope"}
        })]
    );
}
