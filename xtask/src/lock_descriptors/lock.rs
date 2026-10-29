//! Fichier `tests/tools.lock.json` (05 §9.3) : construction depuis la surface, relecture stricte.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{
    checks::tools_list_bytes,
    json::{changed_paths, digest},
    sha256,
    surface::{Surface, as_object},
};

/// Version du format du fichier (`lock_format`).
pub(crate) const LOCK_FORMAT: u64 = 1;

/// `{clé: {sha256, descriptor}}` pour une famille de descripteurs.
fn entries(items: &BTreeMap<String, Value>) -> Value {
    Value::Object(
        items
            .iter()
            .map(|(key, descriptor)| {
                (key.clone(), json!({"sha256": digest(descriptor), "descriptor": descriptor}))
            })
            .collect(),
    )
}

/// Empreinte SHA-256 des octets UTF-8 d'un texte (recalculable avec `sha256sum`).
fn text_digest(text: &str) -> String {
    sha256::hex(&sha256::sha256(text.as_bytes()))
}

/// Contenu du verrou pour `surface` (05 §9.3). Les ères identiques sont une condition de la
/// construction de la surface : `eras_identical` vaut toujours `true`.
///
/// # Errors
///
/// Un nombre flottant dans un descripteur, ou des instructions absentes.
pub(crate) fn build(surface: &Surface) -> Result<Value, String> {
    let instructions = surface.instructions.as_deref().ok_or("the surface has no instructions")?;
    surface.reject_floats()?;
    let tools = as_object(&surface.tools);
    let surface_sha256 = digest(&json!({
        "tools": tools,
        "prompts": as_object(&surface.prompts),
        "resources": as_object(&surface.resources),
        "resource_templates": as_object(&surface.resource_templates),
        "instructions": instructions,
        "capabilities": surface.capabilities,
        "supported_versions": surface.protocol_versions,
    }));
    let chars = instructions.chars().count();
    Ok(json!({
        "lock_format": LOCK_FORMAT,
        "server_version": surface.server_version,
        "protocol_versions": surface.protocol_versions,
        "eras_identical": true,
        "toolset_sha256": digest(&tools),
        "surface_sha256": surface_sha256,
        "sizes": {"tools_list_bytes": tools_list_bytes(surface), "instructions_chars": chars},
        "capabilities": surface.capabilities,
        "instructions": {"sha256": text_digest(instructions), "chars": chars, "text": instructions},
        "tools": entries(&surface.tools),
        "prompts": entries(&surface.prompts),
        "resources": entries(&surface.resources),
        "resource_templates": entries(&surface.resource_templates),
    }))
}

/// Relit un verrou. Un verrou vide, sans outil, d'un autre format, ou dont les empreintes, tailles
/// et indicateurs ne se déduisent pas de son contenu n'est jamais une base valable (P-007).
///
/// # Errors
///
/// Le motif du refus.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let lock: Value = serde_json::from_str(text)
        .map_err(|error| format!("lock file is not valid JSON: {error}"))?;
    if lock.as_object().is_none_or(Map::is_empty) {
        return Err("lock file is empty: an empty baseline is never accepted".to_owned());
    }
    if lock.get("lock_format") != Some(&json!(LOCK_FORMAT)) {
        return Err(format!("lock file: `lock_format` is not {LOCK_FORMAT}"));
    }
    let surface = Surface::from_lock(&lock)?;
    if surface.tools.is_empty() {
        return Err("lock file lists no tool: an empty baseline is never accepted".to_owned());
    }
    let rebuilt = build(&surface)?;
    if rebuilt != lock {
        return Err(format!(
            "lock file is inconsistent with its own content (hashes, sizes or flags) at {}",
            changed_paths(&lock, &rebuilt).join(", ")
        ));
    }
    Ok(lock)
}

/// Verrou de référence lu dans le fichier ou le dépôt : un fichier absent n'est pas une base.
///
/// # Errors
///
/// `what` absent, ou le motif de refus de [`parse`].
pub(crate) fn baseline(text: Option<&str>, what: &str) -> Result<Value, String> {
    match text {
        None => Err(format!("{what} is missing: an absent baseline is never accepted")),
        Some(text) => parse(text).map_err(|error| format!("{what}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_descriptors::{
        fixtures::{VERSION, surface},
        json::pretty,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn the_lock_has_the_shape_of_05_section_9_3() {
        let lock = build(&surface()).unwrap();
        assert_eq!(lock["lock_format"], 1);
        assert_eq!(lock["server_version"], VERSION);
        assert_eq!(lock["protocol_versions"], json!(["2026-07-28", "2025-11-25", "2025-06-18"]));
        assert_eq!(lock["eras_identical"], true);
        assert_eq!(lock["capabilities"], json!({"tools": {}}));
        assert_eq!(lock["prompts"], json!({}));
        assert_eq!(lock["resources"], json!({}));
        assert_eq!(lock["resource_templates"], json!({}));
        let instructions = "French law from Légifrance. Start with fr_search.";
        assert_eq!(lock["instructions"]["text"], instructions);
        assert_eq!(lock["instructions"]["chars"], 49);
        assert_eq!(lock["sizes"]["instructions_chars"], 49);
        assert_eq!(lock["instructions"]["sha256"], text_digest(instructions));
        let coverage = &lock["tools"]["fr_coverage"];
        assert_eq!(coverage["sha256"], digest(&coverage["descriptor"]));
        assert_eq!(coverage["descriptor"]["name"], "fr_coverage");
        let toolset = json!({
            "fr_coverage": lock["tools"]["fr_coverage"]["descriptor"],
            "fr_search": lock["tools"]["fr_search"]["descriptor"],
        });
        assert_eq!(lock["toolset_sha256"], digest(&toolset));
        assert_eq!(
            lock["sizes"]["tools_list_bytes"].as_u64().unwrap(),
            u64::try_from(tools_list_bytes(&surface())).unwrap()
        );
        assert_eq!(lock["surface_sha256"].as_str().unwrap().len(), 64);
    }

    /// Modification d'une surface.
    type Mutation = Box<dyn Fn(&mut Surface)>;

    /// Altération d'un verrou.
    type Tamper = Box<dyn Fn(&mut Value)>;

    #[test]
    fn the_surface_hash_covers_every_part_of_the_surface() {
        let base = build(&surface()).unwrap()["surface_sha256"].clone();
        let changed: [Mutation; 5] = [
            Box::new(|s| s.instructions = Some("other".into())),
            Box::new(|s| s.capabilities = json!({"tools": {}, "prompts": {}})),
            Box::new(|s| {
                s.protocol_versions.pop();
            }),
            Box::new(|s| {
                s.prompts.insert("p".into(), json!({"name": "p"}));
            }),
            Box::new(|s| {
                s.resources.insert("u".into(), json!({"uri": "u"}));
            }),
        ];
        for (index, patch) in changed.iter().enumerate() {
            let mut other = surface();
            patch(&mut other);
            assert_ne!(build(&other).unwrap()["surface_sha256"], base, "patch {index}");
        }
        let mut templated = surface();
        templated.resource_templates.insert("t".into(), json!({"uriTemplate": "t"}));
        assert_ne!(build(&templated).unwrap()["surface_sha256"], base);
    }

    #[test]
    fn building_needs_instructions_and_no_float() {
        let mut bare = surface();
        bare.instructions = None;
        assert_eq!(build(&bare).unwrap_err(), "the surface has no instructions");
        let mut floaty = surface();
        floaty.tools.get_mut("fr_search").unwrap()["max"] = json!(1.5);
        assert!(build(&floaty).unwrap_err().contains("floating-point"));
    }

    #[test]
    fn a_lock_written_as_pretty_json_is_read_back_unchanged() {
        let lock = build(&surface()).unwrap();
        let text = pretty(&lock);
        assert!(text.starts_with("{\n  \"capabilities\""), "{text}");
        assert!(text.ends_with("}\n"));
        assert_eq!(parse(&text).unwrap(), lock);
        assert_eq!(baseline(Some(&text), "tests/tools.lock.json").unwrap(), lock);
    }

    #[test]
    fn descriptor_lock_rejects_empty_baseline() {
        let lock = build(&surface()).unwrap();
        let mut without_tools = lock.clone();
        without_tools["tools"] = json!({});
        let without_tools = without_tools.to_string();
        let empty_baselines = [
            ("", "lock file is not valid JSON"),
            ("   \n", "lock file is not valid JSON"),
            ("{}", "lock file is empty: an empty baseline is never accepted"),
            ("[]", "lock file is empty: an empty baseline is never accepted"),
            ("null", "lock file is empty: an empty baseline is never accepted"),
            (
                without_tools.as_str(),
                "lock file lists no tool: an empty baseline is never accepted",
            ),
        ];
        for (text, expected) in empty_baselines {
            let error = parse(text).unwrap_err();
            assert!(error.contains(expected), "{text:?} : {error}");
            assert!(baseline(Some(text), "base").unwrap_err().starts_with("base: "), "{text:?}");
        }
        assert_eq!(
            baseline(None, "tests/tools.lock.json").unwrap_err(),
            "tests/tools.lock.json is missing: an absent baseline is never accepted"
        );
    }

    #[test]
    fn a_lock_whose_tools_are_gone_but_whose_hashes_are_consistent_is_still_empty() {
        let mut surface = surface();
        surface.tools.clear();
        let lock = build(&surface).unwrap();
        let error = parse(&lock.to_string()).unwrap_err();
        assert_eq!(error, "lock file lists no tool: an empty baseline is never accepted");
    }

    #[test]
    fn a_lock_that_contradicts_itself_or_has_another_format_is_refused() {
        let lock = build(&surface()).unwrap();
        let tampered: [(Tamper, &str); 6] = [
            (
                Box::new(|l| l["tools"]["fr_search"]["sha256"] = json!("0".repeat(64))),
                "/tools/fr_search/sha256",
            ),
            (Box::new(|l| l["toolset_sha256"] = json!("0".repeat(64))), "/toolset_sha256"),
            (Box::new(|l| l["sizes"]["tools_list_bytes"] = json!(1)), "/sizes/tools_list_bytes"),
            (Box::new(|l| l["eras_identical"] = json!(false)), "/eras_identical"),
            (Box::new(|l| l["instructions"]["chars"] = json!(1)), "/instructions/chars"),
            (Box::new(|l| l["extra"] = json!(true)), "/extra"),
        ];
        for (patch, path) in tampered {
            let mut copy = lock.clone();
            patch(&mut copy);
            let error = parse(&copy.to_string()).unwrap_err();
            assert!(error.starts_with("lock file is inconsistent"), "{path} : {error}");
            assert!(error.ends_with(path), "{path} : {error}");
        }
        let mut other_format = lock.clone();
        other_format["lock_format"] = json!(2);
        assert_eq!(
            parse(&other_format.to_string()).unwrap_err(),
            "lock file: `lock_format` is not 1"
        );
        let mut broken = lock;
        broken.as_object_mut().unwrap().remove("tools");
        assert_eq!(parse(&broken.to_string()).unwrap_err(), "lock: `tools` is missing");
    }

    #[test]
    fn a_lock_that_holds_a_float_is_refused() {
        let mut lock = build(&surface()).unwrap();
        lock["tools"]["fr_search"]["descriptor"]["inputSchema"]["properties"]["query"]["maxLength"] =
            json!(2.5);
        let error = parse(&pretty(&lock)).unwrap_err();
        assert!(error.contains("floating-point number 2.5"), "{error}");
    }
}
