//! Surface MCP observée par les sessions : normalisation et égalité des ères (05 §9.2 points 4 et 5).
//!
//! Une [`Capture`] est ce qu'une session a lu sur le binaire. La [`Surface`] en est la forme
//! normalisée : champs hors périmètre retirés, éléments indexés par leur clé, pages aplaties.
//! Trois captures (moderne, 2025-11-25, 2025-06-18) se réduisent à une seule surface ou font
//! échouer la commande.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::json::{changed_paths, reject_floats};

/// Nom que le serveur doit annoncer (`serverInfo.name`, 05 §2.2).
pub(crate) const SERVER_NAME: &str = "mcp-legifrance";

/// Champs d'un résultat ou d'un descripteur hors du périmètre du verrou (05 §9.1).
const OUT_OF_SCOPE: [&str; 4] = ["resultType", "ttlMs", "cacheScope", "nextCursor"];

/// Clé de `_meta` qui porte `serverInfo` (annexe D §3.2) : vérifiée à part, jamais verrouillée.
const SERVER_INFO_META: &str = "io.modelcontextprotocol/serverInfo";

/// Ce qu'une session a lu sur le serveur, avant normalisation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Capture {
    /// Nom de la session dans les messages (« discover 2026-07-28 »).
    pub label: String,
    pub server_name: String,
    pub server_version: String,
    /// `supportedVersions` de `server/discover` ; absent des sessions legacy.
    pub supported_versions: Option<Vec<String>>,
    pub capabilities: Value,
    pub instructions: Option<String>,
    pub tools: Vec<Value>,
    pub prompts: Vec<Value>,
    pub resources: Vec<Value>,
    pub resource_templates: Vec<Value>,
}

/// Surface normalisée du serveur : tout ce que couvre le verrou.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Surface {
    pub server_name: String,
    pub server_version: String,
    pub protocol_versions: Vec<String>,
    pub capabilities: Value,
    pub instructions: Option<String>,
    /// Descripteurs par nom d'outil.
    pub tools: BTreeMap<String, Value>,
    /// Descripteurs par nom de prompt.
    pub prompts: BTreeMap<String, Value>,
    /// Descripteurs par `uri`.
    pub resources: BTreeMap<String, Value>,
    /// Descripteurs par `uriTemplate`.
    pub resource_templates: BTreeMap<String, Value>,
}

/// Descripteur sans les champs hors périmètre (point 4 de 05 §9.2). `_meta` disparaît s'il ne
/// portait que `serverInfo`.
fn normalize_descriptor(descriptor: &Value) -> Value {
    let Value::Object(map) = descriptor else {
        return descriptor.clone();
    };
    let mut map = map.clone();
    for key in OUT_OF_SCOPE {
        map.remove(key);
    }
    if let Some(Value::Object(meta)) = map.get_mut("_meta")
        && meta.remove(SERVER_INFO_META).is_some()
        && meta.is_empty()
    {
        map.remove("_meta");
    }
    Value::Object(map)
}

/// Descripteurs normalisés, indexés par la chaîne `key_field`.
fn keyed(items: &[Value], key_field: &str, what: &str) -> Result<BTreeMap<String, Value>, String> {
    let mut out = BTreeMap::new();
    for item in items {
        let key = item
            .get(key_field)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{what} without a string `{key_field}`: {item}"))?;
        if out.insert(key.to_owned(), normalize_descriptor(item)).is_some() {
            return Err(format!("duplicate {what} `{key}`"));
        }
    }
    Ok(out)
}

impl Surface {
    /// Surface d'une session.
    ///
    /// # Errors
    ///
    /// Un descripteur sans clé, ou deux descripteurs de même clé.
    pub(crate) fn from_capture(capture: &Capture) -> Result<Self, String> {
        let label = &capture.label;
        let context = |error: String| format!("{label}: {error}");
        Ok(Self {
            server_name: capture.server_name.clone(),
            server_version: capture.server_version.clone(),
            protocol_versions: capture.supported_versions.clone().unwrap_or_default(),
            capabilities: capture.capabilities.clone(),
            instructions: capture.instructions.clone(),
            tools: keyed(&capture.tools, "name", "tool").map_err(context)?,
            prompts: keyed(&capture.prompts, "name", "prompt").map_err(context)?,
            resources: keyed(&capture.resources, "uri", "resource").map_err(context)?,
            resource_templates: keyed(&capture.resource_templates, "uriTemplate", "template")
                .map_err(context)?,
        })
    }

    /// Réduit les sessions à une surface unique (point 5 de 05 §9.2). Les versions de protocole
    /// viennent de la session qui les annonce (`server/discover`).
    ///
    /// # Errors
    ///
    /// Aucune session, une session illisible, aucune annonce de versions, ou la liste des chemins
    /// JSON qui diffèrent entre deux sessions.
    #[expect(
        clippy::indexing_slicing,
        reason = "`split_first` a réussi : `captures` n'est pas vide"
    )]
    pub(crate) fn from_captures(captures: &[Capture]) -> Result<Self, String> {
        let surfaces = captures.iter().map(Self::from_capture).collect::<Result<Vec<_>, _>>()?;
        let (first, others) = surfaces.split_first().ok_or("no session was captured")?;
        let mut merged = first.clone();
        for (surface, capture) in others.iter().zip(&captures[1..]) {
            let paths = changed_paths(&first.comparable(), &surface.comparable());
            if !paths.is_empty() {
                return Err(format!(
                    "eras differ: `{}` is not identical to `{}` at {}",
                    capture.label,
                    captures[0].label,
                    paths.join(", ")
                ));
            }
        }
        if let Some(announced) =
            surfaces.iter().find(|surface| !surface.protocol_versions.is_empty())
        {
            merged.protocol_versions.clone_from(&announced.protocol_versions);
        }
        if merged.protocol_versions.is_empty() {
            return Err("no session announced supportedVersions (server/discover)".to_owned());
        }
        Ok(merged)
    }

    /// Refuse un nombre flottant dans les capacités ou les descripteurs (05 §9.2 point 6) : à
    /// appeler avant de calculer une empreinte ou une taille.
    ///
    /// # Errors
    ///
    /// Le pointeur JSON du premier nombre flottant.
    pub(crate) fn reject_floats(&self) -> Result<(), String> {
        reject_floats(&self.comparable())
    }

    /// Ce que les ères doivent partager : tout sauf les versions annoncées.
    fn comparable(&self) -> Value {
        json!({
            "server_info": {"name": self.server_name, "version": self.server_version},
            "capabilities": self.capabilities,
            "instructions": self.instructions,
            "tools": self.tools,
            "prompts": self.prompts,
            "resources": self.resources,
            "resource_templates": self.resource_templates,
        })
    }

    /// Surface relue dans un fichier de verrou (`tests/tools.lock.json`).
    ///
    /// # Errors
    ///
    /// Le premier champ absent ou mal typé.
    pub(crate) fn from_lock(lock: &Value) -> Result<Self, String> {
        let field = |name: &str| lock.get(name).ok_or_else(|| format!("lock: `{name}` is missing"));
        let text = |name: &str| {
            field(name)?
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("lock: `{name}` is not a string"))
        };
        let descriptors = |name: &str| -> Result<BTreeMap<String, Value>, String> {
            let entries = field(name)?
                .as_object()
                .ok_or_else(|| format!("lock: `{name}` is not an object"))?;
            entries
                .iter()
                .map(|(key, entry)| {
                    entry
                        .get("descriptor")
                        .map(|descriptor| (key.clone(), descriptor.clone()))
                        .ok_or_else(|| format!("lock: `{name}.{key}` has no `descriptor`"))
                })
                .collect()
        };
        let versions = field("protocol_versions")?
            .as_array()
            .ok_or("lock: `protocol_versions` is not an array")?
            .iter()
            .map(|version| version.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or("lock: `protocol_versions` holds a non-string")?;
        let instructions = field("instructions")?
            .get("text")
            .and_then(Value::as_str)
            .ok_or("lock: `instructions.text` is missing")?
            .to_owned();
        Ok(Self {
            server_name: SERVER_NAME.to_owned(),
            server_version: text("server_version")?,
            protocol_versions: versions,
            capabilities: field("capabilities")?.clone(),
            instructions: Some(instructions),
            tools: descriptors("tools")?,
            prompts: descriptors("prompts")?,
            resources: descriptors("resources")?,
            resource_templates: descriptors("resource_templates")?,
        })
    }
}

/// Table `clé -> descripteur` en objet JSON.
pub(crate) fn as_object(items: &BTreeMap<String, Value>) -> Value {
    Value::Object(
        items.iter().map(|(key, item)| (key.clone(), item.clone())).collect::<Map<_, _>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_descriptors::fixtures::{capture, tool};
    use pretty_assertions::assert_eq;

    #[test]
    fn descriptors_lose_the_out_of_scope_fields_and_are_keyed_by_name() {
        let mut noisy = tool("fr_search");
        noisy["resultType"] = json!("complete");
        noisy["ttlMs"] = json!(3_600_000);
        noisy["cacheScope"] = json!("public");
        noisy["_meta"] = json!({SERVER_INFO_META: {"name": "x"}});
        let mut kept = tool("fr_coverage");
        kept["_meta"] = json!({SERVER_INFO_META: {"name": "x"}, "vendor/key": 1});
        let surface = Surface::from_capture(&capture("s", &[noisy, kept])).unwrap();
        assert_eq!(surface.tools["fr_search"], tool("fr_search"));
        let mut expected = tool("fr_coverage");
        expected["_meta"] = json!({"vendor/key": 1});
        assert_eq!(surface.tools["fr_coverage"], expected);
        assert_eq!(surface.tools.keys().collect::<Vec<_>>(), ["fr_coverage", "fr_search"]);
    }

    #[test]
    fn descriptors_without_a_key_or_with_a_duplicate_are_refused() {
        let mut nameless = tool("fr_search");
        nameless.as_object_mut().unwrap().remove("name");
        let error = Surface::from_capture(&capture("s", &[nameless])).unwrap_err();
        assert!(error.starts_with("s: tool without a string `name`"), "{error}");
        let error =
            Surface::from_capture(&capture("s", &[tool("fr_a"), tool("fr_a")])).unwrap_err();
        assert_eq!(error, "s: duplicate tool `fr_a`");
        let mut with_prompts = capture("s", &[tool("fr_a")]);
        with_prompts.prompts = vec![json!({"name": "p"}), json!({"name": "p"})];
        assert_eq!(Surface::from_capture(&with_prompts).unwrap_err(), "s: duplicate prompt `p`");
        let mut with_resources = capture("s", &[tool("fr_a")]);
        with_resources.resources = vec![json!({"uri": "u"}), json!({"name": "n"})];
        let error = Surface::from_capture(&with_resources).unwrap_err();
        assert!(error.contains("resource without a string `uri`"), "{error}");
        let mut with_templates = capture("s", &[tool("fr_a")]);
        with_templates.resource_templates = vec![json!({"name": "n"})];
        let error = Surface::from_capture(&with_templates).unwrap_err();
        assert!(error.contains("template without a string `uriTemplate`"), "{error}");
    }

    #[test]
    fn identical_eras_merge_into_one_surface_with_the_announced_versions() {
        let mut modern = capture("discover 2026-07-28", &[tool("fr_b"), tool("fr_a")]);
        modern.supported_versions = Some(vec!["2026-07-28".into(), "2025-11-25".into()]);
        let legacy = capture("initialize 2025-11-25", &[tool("fr_a"), tool("fr_b")]);
        // Une session legacy qui précède la moderne ne change pas le résultat.
        let merged = Surface::from_captures(&[legacy.clone(), modern.clone()]).unwrap();
        assert_eq!(merged.protocol_versions, ["2026-07-28", "2025-11-25"]);
        assert_eq!(merged, Surface::from_captures(&[modern, legacy]).unwrap());
    }

    #[test]
    fn eras_that_differ_fail_with_the_changed_paths() {
        let mut modern = capture("discover 2026-07-28", &[tool("fr_a")]);
        modern.supported_versions = Some(vec!["2026-07-28".into()]);
        let mut legacy = capture("initialize 2025-11-25", &[tool("fr_a")]);
        legacy.tools[0]["description"] = json!("Another text.");
        legacy.instructions = Some("other".into());
        let error = Surface::from_captures(&[modern.clone(), legacy]).unwrap_err();
        assert_eq!(
            error,
            "eras differ: `initialize 2025-11-25` is not identical to `discover 2026-07-28` at \
             /instructions, /tools/fr_a/description"
        );
        let mut missing = capture("initialize 2025-06-18", &[]);
        missing.server_version = "9.9.9".into();
        let error = Surface::from_captures(&[modern, missing]).unwrap_err();
        assert!(error.contains("/server_info/version") && error.contains("/tools/fr_a"), "{error}");
    }

    #[test]
    fn no_session_or_no_announced_versions_is_an_error() {
        assert_eq!(Surface::from_captures(&[]).unwrap_err(), "no session was captured");
        let legacy = capture("initialize 2025-11-25", &[tool("fr_a")]);
        let error = Surface::from_captures(&[legacy]).unwrap_err();
        assert!(error.contains("no session announced supportedVersions"), "{error}");
        let mut broken = capture("s", &[tool("fr_a"), tool("fr_a")]);
        broken.supported_versions = Some(vec!["v".into()]);
        assert!(Surface::from_captures(&[broken]).is_err());
    }

    #[test]
    fn from_lock_reads_each_field_and_names_the_missing_one() {
        let lock = json!({
            "server_version": "0.1.0",
            "protocol_versions": ["2026-07-28"],
            "capabilities": {"tools": {}},
            "instructions": {"text": "hello", "chars": 5, "sha256": "x"},
            "tools": {"fr_a": {"sha256": "x", "descriptor": {"name": "fr_a"}}},
            "prompts": {}, "resources": {}, "resource_templates": {},
        });
        let surface = Surface::from_lock(&lock).unwrap();
        assert_eq!(surface.server_version, "0.1.0");
        assert_eq!(surface.instructions.as_deref(), Some("hello"));
        assert_eq!(surface.tools["fr_a"], json!({"name": "fr_a"}));
        assert_eq!(as_object(&surface.tools), json!({"fr_a": {"name": "fr_a"}}));
        let broken = |patch: Value| {
            let mut copy = lock.clone();
            copy.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            Surface::from_lock(&copy).unwrap_err()
        };
        assert_eq!(broken(json!({"server_version": 1})), "lock: `server_version` is not a string");
        assert_eq!(broken(json!({"tools": []})), "lock: `tools` is not an object");
        assert_eq!(broken(json!({"tools": {"a": {}}})), "lock: `tools.a` has no `descriptor`");
        assert_eq!(
            broken(json!({"protocol_versions": 1})),
            "lock: `protocol_versions` is not an array"
        );
        assert_eq!(
            broken(json!({"protocol_versions": [1]})),
            "lock: `protocol_versions` holds a non-string"
        );
        assert_eq!(broken(json!({"instructions": {}})), "lock: `instructions.text` is missing");
        for name in [
            "server_version",
            "protocol_versions",
            "capabilities",
            "instructions",
            "tools",
            "prompts",
            "resources",
            "resource_templates",
        ] {
            let mut absent = lock.clone();
            absent.as_object_mut().unwrap().remove(name);
            assert_eq!(
                Surface::from_lock(&absent).unwrap_err(),
                format!("lock: `{name}` is missing")
            );
        }
        for name in ["prompts", "resources", "resource_templates"] {
            let mut patch = Map::new();
            patch.insert(name.to_owned(), json!([]));
            assert_eq!(broken(Value::Object(patch)), format!("lock: `{name}` is not an object"));
        }
    }

    #[test]
    fn a_descriptor_that_is_not_an_object_is_kept_as_is() {
        assert_eq!(normalize_descriptor(&json!("not an object")), json!("not an object"));
    }
}
