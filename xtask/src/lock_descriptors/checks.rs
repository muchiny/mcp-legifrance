//! Contrôles du verrou (05 §9.2 point 8) : un échec est bloquant, une heuristique est un avertissement.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{
    json::canonical,
    surface::{SERVER_NAME, Surface},
};

/// Versions de protocole que les trois sessions négocient (00 §6) : `supportedVersions` les contient.
pub(crate) const SESSION_VERSIONS: [&str; 3] = ["2026-07-28", "2025-11-25", "2025-06-18"];

/// Plafond des instructions, en caractères Unicode (05 §4, §6.6).
pub(crate) const INSTRUCTIONS_MAX_CHARS: usize = 3_500;

/// Plafond d'une description d'outil, en caractères (05 §6.6).
const DESCRIPTION_MAX_CHARS: usize = 1_000;

/// Annotations exigées sur les 13 outils (05 §5.0.1) : nom et valeur.
const HINTS: [(&str, bool); 4] = [
    ("readOnlyHint", true),
    ("destructiveHint", false),
    ("idempotentHint", true),
    ("openWorldHint", true),
];

/// Budgets de `tools/list` en octets (05 §6.6) : jalon v0.1, v0.2, v0.3. Valeurs initiales pour
/// v0.1 et v0.2. Le budget de v0.3 est posé par F1 sur la surface finale, les treize outils :
/// 31 853 octets mesurés, plus 3 % (32 808,6), arrondis à la centaine supérieure. Le test
/// `descriptor_size_budget` de `lf-adapter-mcp` porte la même valeur.
const TOOLS_LIST_BUDGETS: [usize; 3] = [20_000, 24_000, 32_900];

/// Outils qui n'existent qu'à partir de v0.2 (M6) et de v0.3 (M7) : ils fixent le budget.
const V02_TOOLS: [&str; 2] = ["fr_verify_citations", "fr_resolve"];
const V03_TOOLS: [&str; 3] = ["fr_get_related", "fr_diff_article", "fr_search_all"];

/// Résultat des contrôles.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Findings {
    /// Échecs bloquants.
    pub errors: Vec<String>,
    /// Heuristiques qui n'échouent pas.
    pub warnings: Vec<String>,
}

/// Octets du `tools/list` : longueur du JSON canonique du tableau des descripteurs.
pub(crate) fn tools_list_bytes(surface: &Surface) -> usize {
    canonical(&Value::Array(surface.tools.values().cloned().collect())).len()
}

/// Budget de `tools/list` du jalon que révèlent les noms d'outils.
fn tools_list_budget(surface: &Surface) -> usize {
    let has = |names: &[&str]| names.iter().any(|name| surface.tools.contains_key(*name));
    if has(&V03_TOOLS) {
        TOOLS_LIST_BUDGETS[2]
    } else if has(&V02_TOOLS) {
        TOOLS_LIST_BUDGETS[1]
    } else {
        TOOLS_LIST_BUDGETS[0]
    }
}

/// `^fr_[a-z_]{1,61}$`.
fn valid_tool_name(name: &str) -> bool {
    name.strip_prefix("fr_").is_some_and(|rest| {
        (1..=61).contains(&rest.len())
            && rest.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    })
}

/// `^[A-Za-z0-9_.-]{1,64}$`.
fn valid_property_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// Nombre de phrases : une fin de phrase (`.`, `!`, `?`) suivie d'un blanc ou de la fin du texte.
fn sentence_count(text: &str) -> usize {
    let chars: Vec<char> = text.trim().chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|(index, character)| {
            matches!(character, '.' | '!' | '?')
                && chars.get(index.saturating_add(1)).is_none_or(|next| next.is_whitespace())
        })
        .count()
}

/// Noms de la forme `fr_…` cités dans `text` (frontière de mot avant, minuscules et `_` après).
/// Une famille (`fr_get_*`) n'est pas un nom d'outil : elle est ignorée.
fn cited_tool_names(text: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut from = 0_usize;
    while let Some(offset) = text[from..].find("fr_") {
        let start = from.saturating_add(offset);
        let bytes = text.as_bytes();
        let bounded = start
            .checked_sub(1)
            .and_then(|before| bytes.get(before))
            .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'));
        let mut end = start.saturating_add(3);
        while bytes.get(end).is_some_and(|b| b.is_ascii_lowercase() || *b == b'_') {
            end = end.saturating_add(1);
        }
        let name = text[start..end].trim_end_matches('_');
        if bounded && name.len() > 3 && bytes.get(end) != Some(&b'*') {
            names.push(name);
        }
        from = end;
    }
    names
}

/// Toutes les chaînes portées par une clé `description`, à n'importe quelle profondeur.
fn descriptions<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                match item {
                    Value::String(text) if key == "description" => out.push(text),
                    _ => descriptions(item, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| descriptions(item, out)),
        _ => {}
    }
}

/// Valeurs de `$ref` qui ne commencent pas par `#` (référence externe).
fn external_refs<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                match item {
                    Value::String(target) if key == "$ref" => {
                        if !target.starts_with('#') {
                            out.push(target);
                        }
                    }
                    _ => external_refs(item, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| external_refs(item, out)),
        _ => {}
    }
}

/// Contrôles d'un schéma d'entrée (`input`) ou de sortie.
fn check_schema(
    tool: &str,
    field: &str,
    schema: Option<&Value>,
    input: bool,
    found: &mut Findings,
) {
    let at = |what: &str| format!("tool `{tool}`: {field} {what}");
    let value = schema.unwrap_or(&Value::Null);
    let Some(root) = value.as_object() else {
        found.errors.push(at("is missing or not an object"));
        return;
    };
    if root.get("type").and_then(Value::as_str) != Some("object") {
        found.errors.push(at("root must have type \"object\""));
    }
    for combinator in ["anyOf", "oneOf", "allOf"] {
        if root.contains_key(combinator) {
            found.errors.push(at(&format!("root must not use {combinator}")));
        }
    }
    if root.contains_key("$schema") {
        found.errors.push(at("must not carry a $schema key (05 §2.5)"));
    }
    let mut refs = Vec::new();
    external_refs(value, &mut refs);
    for target in refs {
        found.errors.push(at(&format!("has the external $ref `{target}`")));
    }
    if input {
        if root.get("additionalProperties") != Some(&Value::Bool(false)) {
            found.errors.push(at("must set additionalProperties to false"));
        }
        for property in root
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(key, _)| key)
        {
            if !valid_property_name(property) {
                found.errors.push(at(&format!(
                    "has the property name `{property}` outside ^[A-Za-z0-9_.-]{{1,64}}$"
                )));
            }
        }
    }
    if let Err(error) = jsonschema::draft202012::meta::validate(value) {
        found.errors.push(at(&format!("is not a valid JSON Schema 2020-12 document: {error}")));
    }
}

/// Contrôles d'un outil.
fn check_tool(name: &str, tool: &Value, found: &mut Findings) {
    let at = |what: &str| format!("tool `{name}`: {what}");
    if !valid_tool_name(name) {
        found.errors.push(at("the name does not match ^fr_[a-z_]{1,61}$"));
    }
    let title = tool.get("title").and_then(Value::as_str).filter(|title| !title.is_empty());
    if title.is_none() {
        found.errors.push(at("title is missing"));
    }
    match tool.get("annotations").and_then(Value::as_object) {
        None => found.errors.push(at("annotations are missing")),
        Some(annotations) => {
            if annotations.get("title").and_then(Value::as_str) != title {
                found.errors.push(at("annotations.title differs from title"));
            }
            for (hint, expected) in HINTS {
                if annotations.get(hint) != Some(&Value::Bool(expected)) {
                    found.errors.push(at(&format!("annotation {hint} must be {expected}")));
                }
            }
            for key in annotations.keys() {
                if key != "title" && !HINTS.iter().any(|(hint, _)| hint == key) {
                    found.errors.push(at(&format!("unexpected annotation `{key}`")));
                }
            }
        }
    }
    match tool.get("description").and_then(Value::as_str) {
        None => found.errors.push(at("description is missing")),
        Some(description) => {
            let chars = description.chars().count();
            if chars > DESCRIPTION_MAX_CHARS {
                found.errors.push(at(&format!(
                    "description has {chars} characters, over {DESCRIPTION_MAX_CHARS}"
                )));
            }
            let sentences = sentence_count(description);
            if !(3..=6).contains(&sentences) {
                found.warnings.push(at(&format!(
                    "description has {sentences} sentences, 3 to 6 expected (00 §6)"
                )));
            }
        }
    }
    check_schema(name, "inputSchema", tool.get("inputSchema"), true, found);
    check_schema(name, "outputSchema", tool.get("outputSchema"), false, found);
}

/// Applique tous les contrôles de 05 §9.2 point 8 à `surface`, pour une version de crate donnée.
pub(crate) fn run(surface: &Surface, crate_version: &str) -> Findings {
    let mut found = Findings::default();
    if surface.server_name != SERVER_NAME {
        found.errors.push(format!(
            "serverInfo.name is `{}`, expected `{SERVER_NAME}`",
            surface.server_name
        ));
    }
    if surface.server_version != crate_version {
        found.errors.push(format!(
            "serverInfo.version is `{}`, the crate version is `{crate_version}`",
            surface.server_version
        ));
    }
    for version in SESSION_VERSIONS {
        if !surface.protocol_versions.iter().any(|served| served == version) {
            found.errors.push(format!("supportedVersions does not list {version}"));
        }
    }
    match &surface.instructions {
        None => found.errors.push("the server returned no instructions".to_owned()),
        Some(text) => {
            let chars = text.chars().count();
            if chars > INSTRUCTIONS_MAX_CHARS {
                found.errors.push(format!(
                    "instructions have {chars} characters, over {INSTRUCTIONS_MAX_CHARS}"
                ));
            }
        }
    }
    if surface.tools.is_empty() {
        found.errors.push("the server exposes no tool".to_owned());
    }
    for (name, tool) in &surface.tools {
        check_tool(name, tool, &mut found);
    }
    // Sans flottant, la taille est celle du JSON canonique ; avec, elle ne veut rien dire.
    if let Err(error) = surface.reject_floats() {
        found.errors.push(error);
    } else {
        let (bytes, budget) = (tools_list_bytes(surface), tools_list_budget(surface));
        if bytes > budget {
            found.errors.push(format!(
                "tools/list is {bytes} bytes, over the {budget} bytes budget (05 §6.6)"
            ));
        }
    }
    let registered: BTreeSet<&str> = surface.tools.keys().map(String::as_str).collect();
    let described = surface
        .tools
        .iter()
        .map(|(name, item)| ("tool", name, item))
        .chain(surface.prompts.iter().map(|(name, item)| ("prompt", name, item)));
    for (kind, name, item) in described {
        let mut texts = Vec::new();
        descriptions(item, &mut texts);
        // Triés et sans doublon : l'ordre ne dépend pas de celui des clés du descripteur.
        let cited: BTreeSet<&str> = texts.into_iter().flat_map(cited_tool_names).collect();
        for unknown in cited.difference(&registered) {
            found.errors.push(format!("{kind} `{name}` cites the unregistered tool `{unknown}`"));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_descriptors::fixtures::{VERSION, surface, tool};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn findings_with(patch: impl FnOnce(&mut Surface)) -> Findings {
        let mut surface = surface();
        patch(&mut surface);
        run(&surface, VERSION)
    }

    fn tool_findings(patch: impl FnOnce(&mut Value)) -> Findings {
        findings_with(|surface| patch(surface.tools.get_mut("fr_search").unwrap()))
    }

    #[test]
    fn a_conforming_surface_has_no_finding() {
        assert_eq!(run(&surface(), VERSION), Findings::default());
    }

    /// Modification d'un descripteur d'outil.
    type Patch = Box<dyn Fn(&mut Value)>;

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "table des formes interdites (05 §9.2 point 8) : un seul test, nommé par le plan"
    )]
    fn lock_controls_reject_forbidden_shapes() {
        let forbidden: Vec<(&str, Patch, &str)> = vec![
            (
                "$schema key",
                Box::new(|t| {
                    t["inputSchema"]["$schema"] =
                        json!("https://json-schema.org/draft/2020-12/schema");
                }),
                "inputSchema must not carry a $schema key",
            ),
            (
                "$schema key in the output",
                Box::new(|t| t["outputSchema"]["$schema"] = json!("x")),
                "outputSchema must not carry a $schema key",
            ),
            (
                "root that is not an object",
                Box::new(|t| t["outputSchema"]["type"] = json!("array")),
                "outputSchema root must have type \"object\"",
            ),
            (
                "root without a type",
                Box::new(|t| {
                    t["inputSchema"].as_object_mut().unwrap().remove("type");
                }),
                "inputSchema root must have type \"object\"",
            ),
            (
                "combinator at the root",
                Box::new(|t| t["inputSchema"]["anyOf"] = json!([{"type": "object"}])),
                "inputSchema root must not use anyOf",
            ),
            (
                "oneOf at the root",
                Box::new(|t| t["outputSchema"]["oneOf"] = json!([{"type": "object"}])),
                "outputSchema root must not use oneOf",
            ),
            (
                "allOf at the root",
                Box::new(|t| t["outputSchema"]["allOf"] = json!([{"type": "object"}])),
                "outputSchema root must not use allOf",
            ),
            (
                "external $ref",
                Box::new(|t| {
                    t["inputSchema"]["properties"]["query"] =
                        json!({"$ref": "https://example.com/q.json"});
                }),
                "has the external $ref `https://example.com/q.json`",
            ),
            (
                "property name longer than 64 characters",
                Box::new(|t| {
                    t["inputSchema"]["properties"][&"p".repeat(65)] = json!({"type": "string"});
                }),
                "outside ^[A-Za-z0-9_.-]{1,64}$",
            ),
            (
                "property name with a forbidden character",
                Box::new(|t| t["inputSchema"]["properties"]["a b"] = json!({"type": "string"})),
                "has the property name `a b`",
            ),
            (
                "open input",
                Box::new(|t| t["inputSchema"]["additionalProperties"] = json!(true)),
                "inputSchema must set additionalProperties to false",
            ),
            (
                "missing input schema",
                Box::new(|t| {
                    t.as_object_mut().unwrap().remove("inputSchema");
                }),
                "inputSchema is missing or not an object",
            ),
            (
                "invalid 2020-12 schema",
                Box::new(|t| t["outputSchema"]["properties"]["hits"]["type"] = json!("list")),
                "outputSchema is not a valid JSON Schema 2020-12 document",
            ),
            (
                "title different from annotations.title",
                Box::new(|t| t["annotations"]["title"] = json!("Other")),
                "annotations.title differs from title",
            ),
            (
                "missing title",
                Box::new(|t| {
                    t.as_object_mut().unwrap().remove("title");
                }),
                "title is missing",
            ),
            (
                "annotation that is not exact",
                Box::new(|t| t["annotations"]["readOnlyHint"] = json!(false)),
                "annotation readOnlyHint must be true",
            ),
            (
                "destructive annotation",
                Box::new(|t| t["annotations"]["destructiveHint"] = json!(true)),
                "annotation destructiveHint must be false",
            ),
            (
                "annotation outside the list",
                Box::new(|t| t["annotations"]["extra"] = json!(true)),
                "unexpected annotation `extra`",
            ),
            (
                "missing annotations",
                Box::new(|t| {
                    t.as_object_mut().unwrap().remove("annotations");
                }),
                "annotations are missing",
            ),
            (
                "missing description",
                Box::new(|t| {
                    t.as_object_mut().unwrap().remove("description");
                }),
                "description is missing",
            ),
            (
                "description over 1000 characters",
                Box::new(|t| t["description"] = json!(format!("{}.", "d".repeat(1_000)))),
                "description has 1001 characters, over 1000",
            ),
            (
                "float in a descriptor",
                Box::new(|t| t["inputSchema"]["properties"]["query"]["maxLength"] = json!(2.5)),
                "floating-point number 2.5",
            ),
        ];
        for (shape, patch, expected) in forbidden {
            let found = tool_findings(patch);
            assert!(
                found.errors.iter().any(|error| error.contains(expected)),
                "{shape} : `{expected}` absent de {:?}",
                found.errors
            );
        }
    }

    #[test]
    fn tool_names_outside_the_grammar_are_refused() {
        let too_long = format!("fr_{}", "a".repeat(62));
        for name in ["search", "fr_", "fr_Search", "fr_search1", "fr_é", too_long.as_str()] {
            let found = findings_with(|surface| {
                surface.tools.insert(name.to_owned(), tool(name));
            });
            assert!(
                found
                    .errors
                    .iter()
                    .any(|error| error.contains("the name does not match ^fr_[a-z_]{1,61}$")),
                "{name} : {:?}",
                found.errors
            );
        }
        let longest = format!("fr_{}", "a".repeat(61));
        let found = findings_with(|surface| {
            surface.tools.insert(longest.clone(), tool(&longest));
        });
        assert_eq!(found.errors, Vec::<String>::new());
    }

    #[test]
    fn sentence_count_is_a_warning_and_length_an_error() {
        let found = tool_findings(|t| t["description"] = json!("One sentence only."));
        assert_eq!(found.errors, Vec::<String>::new());
        assert_eq!(
            found.warnings,
            ["tool `fr_search`: description has 1 sentences, 3 to 6 expected (00 §6)"]
        );
        let found = tool_findings(|t| t["description"] = json!("A. B. C. D. E. F. G."));
        assert_eq!(found.warnings.len(), 1);
        for ok in ["A. B. C.", "A! B? C. D. E. F."] {
            assert_eq!(
                tool_findings(|t| t["description"] = json!(ok)).warnings,
                Vec::<String>::new(),
                "{ok}"
            );
        }
        assert_eq!(sentence_count("v1.2 is out. Really? Yes"), 2);
        assert_eq!(sentence_count(""), 0);
    }

    #[test]
    fn cited_tool_names_follow_the_word_boundary_rules() {
        assert_eq!(
            cited_tool_names("Use fr_search, then (fr_get_text). Not afr_x, fr_*, fr_ or fr_a_."),
            ["fr_search", "fr_get_text", "fr_a"]
        );
        assert_eq!(cited_tool_names("fr_search"), ["fr_search"]);
        assert_eq!(cited_tool_names("the fr_get_* family, then fr_search"), ["fr_search"]);
        assert_eq!(cited_tool_names("écrit fr_x"), ["fr_x"]);
        assert_eq!(cited_tool_names("no tool here"), Vec::<&str>::new());
    }

    #[test]
    fn unregistered_tool_names_in_descriptions_and_prompts_are_refused() {
        let found = tool_findings(|t| {
            t["description"] = json!("Call fr_ghost first. Then stop. Done.");
            t["inputSchema"]["properties"]["query"]["description"] =
                json!("See fr_coverage and fr_phantom.");
        });
        assert_eq!(
            found.errors,
            [
                "tool `fr_search` cites the unregistered tool `fr_ghost`",
                "tool `fr_search` cites the unregistered tool `fr_phantom`",
            ]
        );
        let found = findings_with(|surface| {
            surface.prompts.insert(
                "review".into(),
                json!({"name": "review", "description": "Ask fr_search or fr_ghost.",
                       "arguments": [{"name": "q", "description": "Then fr_other"}]}),
            );
        });
        assert_eq!(
            found.errors,
            [
                "prompt `review` cites the unregistered tool `fr_ghost`",
                "prompt `review` cites the unregistered tool `fr_other`",
            ]
        );
    }

    #[test]
    fn identity_versions_instructions_and_budget_are_controlled() {
        let found = findings_with(|surface| {
            surface.server_name = "other".into();
            surface.server_version = "9.9.9".into();
            surface.protocol_versions = vec!["2026-07-28".into(), "2025-06-18".into()];
            surface.instructions = Some("é".repeat(INSTRUCTIONS_MAX_CHARS + 1));
        });
        assert_eq!(
            found.errors,
            [
                "serverInfo.name is `other`, expected `mcp-legifrance`",
                "serverInfo.version is `9.9.9`, the crate version is `0.1.0`",
                "supportedVersions does not list 2025-11-25",
                "instructions have 3501 characters, over 3500",
            ]
        );
        let at_limit = findings_with(|surface| {
            surface.instructions = Some("é".repeat(INSTRUCTIONS_MAX_CHARS));
        });
        assert_eq!(at_limit, Findings::default());
        let found = findings_with(|surface| surface.instructions = None);
        assert_eq!(found.errors, ["the server returned no instructions"]);
        let found = findings_with(|surface| surface.tools.clear());
        assert_eq!(found.errors, ["the server exposes no tool"]);
    }

    #[test]
    fn tools_list_budget_follows_the_milestone() {
        let padded = |surface: &mut Surface, target: usize| {
            surface.tools.get_mut("fr_search").unwrap()["_meta"] = json!({"pad": ""});
            let base = tools_list_bytes(surface);
            surface.tools.get_mut("fr_search").unwrap()["_meta"] =
                json!({"pad": "x".repeat(target - base)});
            assert_eq!(tools_list_bytes(surface), target);
        };
        let mut v01 = surface();
        padded(&mut v01, 20_000);
        assert_eq!(run(&v01, VERSION).errors, Vec::<String>::new());
        padded(&mut v01, 20_001);
        assert_eq!(
            run(&v01, VERSION).errors,
            ["tools/list is 20001 bytes, over the 20000 bytes budget (05 §6.6)"]
        );
        for (extra, budget) in [("fr_resolve", 24_000), ("fr_search_all", 32_900)] {
            let mut bigger = surface();
            bigger.tools.insert(extra.to_owned(), tool(extra));
            padded(&mut bigger, budget);
            assert_eq!(run(&bigger, VERSION).errors, Vec::<String>::new(), "{extra}");
            padded(&mut bigger, budget + 1);
            assert_eq!(
                run(&bigger, VERSION).errors,
                [format!(
                    "tools/list is {} bytes, over the {budget} bytes budget (05 §6.6)",
                    budget + 1
                )],
                "{extra}"
            );
        }
    }

    #[test]
    fn internal_refs_are_accepted() {
        let found = tool_findings(|tool| {
            tool["inputSchema"]["properties"]["query"] = json!({"$ref": "#/$defs/query"});
        });
        assert_eq!(found.errors, Vec::<String>::new());
    }
}
