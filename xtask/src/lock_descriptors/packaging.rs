//! Manifeste MCPB et `server.json` (05 §9.4, 08 §4.4 et §6.2) : générés ou contrôlés depuis le verrou.
//!
//! Le manifeste garde tout ce que ses auteurs y écrivent ; la commande ne possède que `version`,
//! `tools` et les champs `prompts` (08 §4.3 : le schéma 0.3 n'accepte que `name` et `description`
//! par outil, et les prompts sont rendus par le serveur, donc jamais listés). `server.json` ne
//! porte pas de liste d'outils (08 §6.2) : seules ses versions sont contrôlées.

use serde_json::{Map, Value, json};

/// Chemin du manifeste MCPB, relatif à la racine du dépôt (08 §4.4).
pub(crate) const MANIFEST_PATH: &str = "packaging/mcpb/manifest.json";

/// Chemin de `server.json`, relatif à la racine du dépôt (08 §6.2).
pub(crate) const SERVER_JSON_PATH: &str = "server.json";

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| format!("{what} is not a JSON object"))
}

fn lock_version(lock: &Value) -> Result<&str, String> {
    lock.get("server_version")
        .and_then(Value::as_str)
        .ok_or_else(|| "lock: `server_version` is missing".to_owned())
}

/// Manifeste MCPB attendu : `current` avec la version et la liste d'outils du verrou.
///
/// # Errors
///
/// Un manifeste qui n'est pas un objet, ou un outil du verrou sans description.
pub(crate) fn manifest(current: &Value, lock: &Value) -> Result<Value, String> {
    let mut out = object(current, "the MCPB manifest")?.clone();
    out.insert("version".to_owned(), json!(lock_version(lock)?));
    let tools = lock.get("tools").and_then(Value::as_object).ok_or("lock: `tools` is missing")?;
    let listed = tools
        .iter()
        .map(|(name, entry)| {
            let description = entry["descriptor"]["description"]
                .as_str()
                .ok_or_else(|| format!("lock: tool `{name}` has no description"))?;
            Ok(json!({"name": name, "description": description}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    out.insert("tools".to_owned(), Value::Array(listed));
    out.remove("prompts");
    let has_prompts =
        lock.get("prompts").and_then(Value::as_object).is_some_and(|prompts| !prompts.is_empty());
    if has_prompts {
        out.insert("prompts_generated".to_owned(), json!(true));
    } else {
        out.remove("prompts_generated");
    }
    Ok(Value::Object(out))
}

/// `server.json` attendu : `current` avec la version du verrou, au niveau du serveur et de chaque
/// paquet qui en porte une (le paquet OCI n'en a pas, 08 §6.2).
///
/// # Errors
///
/// Un `server.json` qui n'est pas un objet, ou des `packages` qui ne sont pas un tableau d'objets.
pub(crate) fn server_json(current: &Value, lock: &Value) -> Result<Value, String> {
    let version = json!(lock_version(lock)?);
    let mut out = object(current, "server.json")?.clone();
    out.insert("version".to_owned(), version.clone());
    if let Some(packages) = out.get_mut("packages") {
        let packages = packages.as_array_mut().ok_or("server.json: `packages` is not an array")?;
        for package in packages {
            let package =
                package.as_object_mut().ok_or("server.json: a package is not a JSON object")?;
            if package.contains_key("version") {
                package.insert("version".to_owned(), version.clone());
            }
        }
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_descriptors::{fixtures::surface, lock::build};
    use pretty_assertions::assert_eq;

    fn lock() -> Value {
        build(&surface()).unwrap()
    }

    #[test]
    fn the_manifest_takes_its_version_and_tools_from_the_lock() {
        let template = json!({
            "manifest_version": "0.3",
            "name": "mcp-legifrance",
            "version": "0.0.0",
            "tools": [{"name": "stale", "description": "gone"}],
            "tools_generated": false,
            "prompts": [{"name": "old"}],
            "prompts_generated": true,
            "server": {"type": "binary"},
        });
        let generated = manifest(&template, &lock()).unwrap();
        assert_eq!(generated["version"], "0.1.0");
        let description = "Searches the synthetic corpus. Returns at most ten hits. Needs a query.";
        assert_eq!(
            generated["tools"],
            json!([
                {"name": "fr_coverage", "description": description},
                {"name": "fr_search", "description": description},
            ])
        );
        // Les autres champs sont intacts ; les prompts n'existent pas tant que le verrou n'en a pas.
        assert_eq!(generated["manifest_version"], "0.3");
        assert_eq!(generated["tools_generated"], false);
        assert_eq!(generated["server"], json!({"type": "binary"}));
        assert_eq!(generated.get("prompts"), None);
        assert_eq!(generated.get("prompts_generated"), None);
        // Idempotent.
        assert_eq!(manifest(&generated, &lock()).unwrap(), generated);
    }

    #[test]
    fn prompts_are_declared_as_generated_and_never_listed() {
        let mut with_prompts = lock();
        with_prompts["prompts"] =
            json!({"review": {"sha256": "x", "descriptor": {"name": "review"}}});
        let generated = manifest(&json!({"prompts": [{"name": "review"}]}), &with_prompts).unwrap();
        assert_eq!(generated["prompts_generated"], true);
        assert_eq!(generated.get("prompts"), None);
        let mut broken = lock();
        broken["prompts"] = json!(null);
        assert_eq!(manifest(&json!({}), &broken).unwrap().get("prompts_generated"), None);
    }

    #[test]
    fn the_manifest_refuses_what_it_cannot_generate() {
        assert_eq!(
            manifest(&json!([]), &lock()).unwrap_err(),
            "the MCPB manifest is not a JSON object"
        );
        let mut nameless = lock();
        nameless["tools"]["fr_search"]["descriptor"].as_object_mut().unwrap().remove("description");
        assert_eq!(
            manifest(&json!({}), &nameless).unwrap_err(),
            "lock: tool `fr_search` has no description"
        );
        let mut no_tools = lock();
        no_tools.as_object_mut().unwrap().remove("tools");
        assert_eq!(manifest(&json!({}), &no_tools).unwrap_err(), "lock: `tools` is missing");
        let mut no_version = lock();
        no_version.as_object_mut().unwrap().remove("server_version");
        assert_eq!(
            manifest(&json!({}), &no_version).unwrap_err(),
            "lock: `server_version` is missing"
        );
    }

    #[test]
    fn server_json_version_is_set_on_the_server_and_on_versioned_packages_only() {
        let template = json!({
            "name": "io.github.muchiny/mcp-legifrance",
            "version": "0.0.1",
            "packages": [
                {"registryType": "mcpb", "version": "0.0.1", "identifier": "https://example/mcp.mcpb"},
                {"registryType": "oci", "identifier": "ghcr.io/muchiny/mcp-legifrance:0.0.1"},
            ],
        });
        let generated = server_json(&template, &lock()).unwrap();
        assert_eq!(generated["version"], "0.1.0");
        assert_eq!(generated["packages"][0]["version"], "0.1.0");
        assert_eq!(generated["packages"][1].get("version"), None);
        assert_eq!(generated["packages"][1]["identifier"], "ghcr.io/muchiny/mcp-legifrance:0.0.1");
        assert_eq!(server_json(&generated, &lock()).unwrap(), generated);
        // Sans paquets, la version est quand même posée.
        assert_eq!(server_json(&json!({}), &lock()).unwrap(), json!({"version": "0.1.0"}));
    }

    #[test]
    fn server_json_refuses_misshapen_content() {
        assert_eq!(
            server_json(&json!("x"), &lock()).unwrap_err(),
            "server.json is not a JSON object"
        );
        assert_eq!(
            server_json(&json!({"packages": {}}), &lock()).unwrap_err(),
            "server.json: `packages` is not an array"
        );
        assert_eq!(
            server_json(&json!({"packages": [1]}), &lock()).unwrap_err(),
            "server.json: a package is not a JSON object"
        );
        assert_eq!(
            server_json(&json!({}), &json!({})).unwrap_err(),
            "lock: `server_version` is missing"
        );
    }
}
