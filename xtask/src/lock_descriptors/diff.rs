//! Différences entre deux verrous : rapport de `--check` et classement `SemVer` de `--compare`
//! (05 §9.4, §9.5).
//!
//! Le classement est volontairement prudent : une forme de schéma que les règles ne savent pas
//! comparer (`anyOf`, `format`, `pattern`, `$ref`, champ inconnu d'un descripteur…) est une
//! rupture, jamais un ajout. Une montée de schemars qui reformule un schéma demande donc au moins
//! autant de version qu'un vrai changement.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::json::{changed_paths, escape_token};

/// Familles de descripteurs du verrou.
const FAMILIES: [&str; 4] = ["tools", "prompts", "resources", "resource_templates"];

/// Annotations de sûreté (05 §5.0.1) : nom, valeur sûre, valeur par défaut de MCP quand elle manque.
const SAFETY_HINTS: [(&str, bool, bool); 4] = [
    ("readOnlyHint", true, false),
    ("destructiveHint", false, true),
    ("idempotentHint", true, false),
    ("openWorldHint", false, true),
];

/// Classe d'un changement de surface (05 §9.5), de la moins à la plus lourde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Class {
    /// Correctif : reformulation à sens constant.
    Fix,
    /// Ajout : tout ce qui élargit la surface sans casser un appelant.
    Addition,
    /// Rupture : retrait, restriction, sens changé.
    Breaking,
}

impl Class {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Fix => "fix",
            Self::Addition => "addition",
            Self::Breaking => "breaking",
        }
    }
}

/// Un changement classé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub class: Class,
    /// Pointeur JSON dans la famille concernée (`/tools/fr_search/inputSchema/...`).
    pub path: String,
    pub what: String,
}

/// Côté d'un schéma : une restriction de l'entrée casse l'appelant, une évolution de la sortie
/// casse celui qui la lit, et toute évolution de la sortie hors ajout de champ est une rupture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Input,
    Output,
}

struct Sink<'a> {
    out: &'a mut Vec<Change>,
}

impl Sink<'_> {
    fn push(&mut self, class: Class, path: &str, what: impl Into<String>) {
        self.out.push(Change { class, path: path.to_owned(), what: what.into() });
    }
}

fn entries<'a>(lock: &'a Value, family: &str) -> Option<&'a Map<String, Value>> {
    lock.get(family).and_then(Value::as_object)
}

/// Valeur de `key` dans `entries`, `null` si absente (jamais le cas pour une clé commune).
fn member<'a>(entries: Option<&'a Map<String, Value>>, key: &str) -> &'a Value {
    entries.and_then(|map| map.get(key)).unwrap_or(&Value::Null)
}

/// Rapport de `--check` : outils (et autres familles) ajoutés, retirés, modifiés avec leurs chemins
/// JSON, puis les autres écarts du fichier. Vide si `old == new`.
pub(crate) fn report(old: &Value, new: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    for family in FAMILIES {
        let (before, after) = (entries(old, family), entries(new, family));
        let keys = |entries: Option<&Map<String, Value>>| -> BTreeSet<String> {
            entries.into_iter().flatten().map(|(key, _)| key.clone()).collect()
        };
        let (before_keys, after_keys) = (keys(before), keys(after));
        let join = |names: Vec<&String>| names.into_iter().cloned().collect::<Vec<_>>().join(", ");
        let added = after_keys.difference(&before_keys).collect::<Vec<_>>();
        let removed = before_keys.difference(&after_keys).collect::<Vec<_>>();
        if !added.is_empty() {
            lines.push(format!("{family} added: {}", join(added)));
        }
        if !removed.is_empty() {
            lines.push(format!("{family} removed: {}", join(removed)));
        }
        for key in before_keys.intersection(&after_keys) {
            let paths = changed_paths(member(before, key), member(after, key));
            if !paths.is_empty() {
                lines.push(format!("{family} changed: {key} ({})", paths.join(", ")));
            }
        }
    }
    let strip = |lock: &Value| {
        let mut copy = lock.clone();
        if let Some(map) = copy.as_object_mut() {
            for family in FAMILIES {
                map.remove(family);
            }
        }
        copy
    };
    let others = changed_paths(&strip(old), &strip(new));
    if !others.is_empty() {
        lines.push(format!("other fields changed: {}", others.join(", ")));
    }
    lines
}

/// Changements classés entre le verrou `old` (référence) et `new`. Les deux sont des verrous déjà
/// relus par [`super::lock::parse`] : `server_version` n'est pas comparé ici.
#[expect(
    clippy::indexing_slicing,
    reason = "indexation de `serde_json::Value` : un champ absent donne `Null`, sans panique"
)]
pub(crate) fn classify(old: &Value, new: &Value) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut sink = Sink { out: &mut changes };
    for family in FAMILIES {
        let empty = Map::new();
        let before = entries(old, family).unwrap_or(&empty);
        let after = entries(new, family).unwrap_or(&empty);
        for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
            let path = format!("/{family}/{}", escape_token(key));
            match (before.get(key), after.get(key)) {
                (Some(_), None) => sink.push(Class::Breaking, &path, "removed"),
                (Some(left), Some(right)) => {
                    descriptor(&path, &left["descriptor"], &right["descriptor"], &mut sink);
                }
                // Une clé vient de l'union des deux tables : `(None, None)` n'existe pas.
                (None, _) => sink.push(Class::Addition, &path, "added"),
            }
        }
    }
    if old["instructions"]["text"] != new["instructions"]["text"] {
        sink.push(Class::Fix, "/instructions", "instructions reworded");
    }
    capabilities(&old["capabilities"], &new["capabilities"], &mut sink);
    versions(&old["protocol_versions"], &new["protocol_versions"], &mut sink);
    changes.sort_by(|left, right| left.path.cmp(&right.path).then(left.what.cmp(&right.what)));
    changes
}

/// Classe la plus lourde, ou `None` sans changement.
pub(crate) fn overall(changes: &[Change]) -> Option<Class> {
    changes.iter().map(|change| change.class).max()
}

fn capabilities(old: &Value, new: &Value, sink: &mut Sink<'_>) {
    let empty = Map::new();
    let (before, after) = (old.as_object().unwrap_or(&empty), new.as_object().unwrap_or(&empty));
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let path = format!("/capabilities/{}", escape_token(key));
        match (before.get(key), after.get(key)) {
            (Some(_), None) => sink.push(Class::Breaking, &path, "capability removed"),
            (None, Some(_)) => sink.push(Class::Addition, &path, "capability added"),
            (Some(left), Some(right)) if left != right => {
                sink.push(Class::Breaking, &path, "capability changed");
            }
            _ => {}
        }
    }
}

fn versions(old: &Value, new: &Value, sink: &mut Sink<'_>) {
    let set = |value: &Value| -> BTreeSet<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let (before, after) = (set(old), set(new));
    for version in before.difference(&after) {
        sink.push(
            Class::Breaking,
            "/protocol_versions",
            format!("protocol version {version} no longer served"),
        );
    }
    for version in after.difference(&before) {
        sink.push(
            Class::Addition,
            "/protocol_versions",
            format!("protocol version {version} now served"),
        );
    }
}

/// Descripteur d'un outil, d'un prompt, d'une resource ou d'un gabarit.
fn descriptor(path: &str, old: &Value, new: &Value, sink: &mut Sink<'_>) {
    let empty = Map::new();
    let (before, after) = (old.as_object().unwrap_or(&empty), new.as_object().unwrap_or(&empty));
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let (left, right) = (before.get(key), after.get(key));
        if left == right {
            continue;
        }
        let child = format!("{path}/{}", escape_token(key));
        match (key.as_str(), left, right) {
            ("description" | "title", ..) => sink.push(Class::Fix, &child, "text reworded"),
            ("annotations", ..) => annotations(&child, left, right, sink),
            ("inputSchema", Some(l), Some(r)) => schema(Side::Input, &child, l, r, sink),
            ("outputSchema", Some(l), Some(r)) => schema(Side::Output, &child, l, r, sink),
            ("arguments", Some(l), Some(r)) => prompt_arguments(&child, l, r, sink),
            _ => sink.push(Class::Breaking, &child, "field changed"),
        }
    }
}

fn annotations(path: &str, old: Option<&Value>, new: Option<&Value>, sink: &mut Sink<'_>) {
    let empty = Map::new();
    let before = old.and_then(Value::as_object).unwrap_or(&empty);
    let after = new.and_then(Value::as_object).unwrap_or(&empty);
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let (left, right) = (before.get(key), after.get(key));
        if left == right {
            continue;
        }
        let child = format!("{path}/{}", escape_token(key));
        let Some((_, safe, unsafe_default)) =
            SAFETY_HINTS.iter().find(|hint| hint.0 == key.as_str())
        else {
            if key == "title" {
                sink.push(Class::Fix, &child, "annotation title reworded");
            } else {
                sink.push(Class::Breaking, &child, "annotation changed");
            }
            continue;
        };
        let effective =
            |value: Option<&Value>| value.and_then(Value::as_bool).unwrap_or(*unsafe_default);
        let (was, is) = (effective(left), effective(right));
        if was == is {
            sink.push(Class::Fix, &child, "annotation spelled differently, same meaning");
        } else if is == *safe {
            sink.push(Class::Addition, &child, "annotation moved to the safer value");
        } else {
            sink.push(Class::Breaking, &child, "annotation moved to the less safe value");
        }
    }
}

fn prompt_arguments(path: &str, old: &Value, new: &Value, sink: &mut Sink<'_>) {
    let index = |value: &Value| -> Map<String, Value> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|argument| {
                Some((argument.get("name")?.as_str()?.to_owned(), argument.clone()))
            })
            .collect()
    };
    let (before, after) = (index(old), index(new));
    for name in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let child = format!("{path}/{}", escape_token(name));
        match (before.get(name), after.get(name)) {
            (Some(_), None) => sink.push(Class::Breaking, &child, "argument removed"),
            (None, Some(added)) if added["required"] == true => {
                sink.push(Class::Breaking, &child, "required argument added");
            }
            (None, _) => sink.push(Class::Addition, &child, "optional argument added"),
            (Some(left), Some(right)) => argument(&child, left, right, sink),
        }
    }
}

fn argument(path: &str, old: &Value, new: &Value, sink: &mut Sink<'_>) {
    let empty = Map::new();
    let (before, after) = (old.as_object().unwrap_or(&empty), new.as_object().unwrap_or(&empty));
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let (left, right) = (before.get(key), after.get(key));
        if left == right {
            continue;
        }
        let child = format!("{path}/{}", escape_token(key));
        match key.as_str() {
            "description" | "title" => sink.push(Class::Fix, &child, "text reworded"),
            "required" if right == Some(&Value::Bool(true)) => {
                sink.push(Class::Breaking, &child, "argument became required");
            }
            "required" => sink.push(Class::Addition, &child, "argument became optional"),
            _ => sink.push(Class::Breaking, &child, "argument field changed"),
        }
    }
}

/// Valeurs d'un ensemble JSON (`required`, `enum`, `type`) sous forme de textes comparables.
fn members(value: Option<&Value>) -> Option<BTreeSet<String>> {
    match value? {
        Value::Array(items) => Some(items.iter().map(Value::to_string).collect()),
        scalar => Some(BTreeSet::from([scalar.to_string()])),
    }
}

fn number(value: Option<&Value>) -> Option<i128> {
    let number = value?.as_number()?;
    number.as_i64().map(i128::from).or_else(|| number.as_u64().map(i128::from))
}

/// Mots-clés de borne : `true` pour une borne basse (la resserrer la relève), `false` pour une haute.
fn bound_kind(key: &str) -> Option<bool> {
    match key {
        "minimum" | "exclusiveMinimum" | "minLength" | "minItems" | "minProperties" => Some(true),
        "maximum" | "exclusiveMaximum" | "maxLength" | "maxItems" | "maxProperties" => Some(false),
        _ => None,
    }
}

fn schema(side: Side, path: &str, old: &Value, new: &Value, sink: &mut Sink<'_>) {
    if old == new {
        return;
    }
    let (Some(before), Some(after)) = (old.as_object(), new.as_object()) else {
        sink.push(Class::Breaking, path, "schema changed shape");
        return;
    };
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let (left, right) = (before.get(key), after.get(key));
        if left == right {
            continue;
        }
        let child = format!("{path}/{}", escape_token(key));
        keyword(side, key, &child, (left, right), sink);
    }
}

/// Un mot-clé qui diffère entre l'ancien et le nouveau schéma.
fn keyword(
    side: Side,
    key: &str,
    path: &str,
    (old, new): (Option<&Value>, Option<&Value>),
    sink: &mut Sink<'_>,
) {
    let input = side == Side::Input;
    match key {
        "description" | "title" | "$comment" | "examples" => {
            sink.push(Class::Fix, path, "text reworded");
        }
        "properties" => properties(side, path, old, new, sink),
        "$defs" | "definitions" => definitions(side, path, old, new, sink),
        "items" | "additionalProperties"
            if old.is_some_and(Value::is_object) && new.is_some_and(Value::is_object) =>
        {
            schema(side, path, old.unwrap_or(&Value::Null), new.unwrap_or(&Value::Null), sink);
        }
        "additionalProperties" if input && old == Some(&Value::Bool(false)) => {
            sink.push(Class::Addition, path, "input opened to extra properties");
        }
        "required" => {
            let (before, after) =
                (members(old).unwrap_or_default(), members(new).unwrap_or_default());
            let (gained, lost) = (!after.is_subset(&before), !before.is_subset(&after));
            if gained {
                let class = if input { Class::Breaking } else { Class::Addition };
                sink.push(class, path, "property now required");
            }
            if lost {
                let class = if input { Class::Addition } else { Class::Breaking };
                sink.push(class, path, "property no longer required");
            }
        }
        "type" | "enum" if input => {
            let (before, after) = (members(old), members(new));
            match (before, after) {
                (Some(left), Some(right)) if left.is_subset(&right) => {
                    sink.push(Class::Addition, path, format!("{key} widened"));
                }
                _ => sink.push(Class::Breaking, path, format!("{key} restricted or changed")),
            }
        }
        _ if input && bound_kind(key).is_some() => {
            let lower = bound_kind(key).unwrap_or(false);
            match (number(old), number(new)) {
                (None, Some(_)) if old.is_none() => sink.push(Class::Breaking, path, "bound added"),
                (Some(_), None) if new.is_none() => {
                    sink.push(Class::Addition, path, "bound removed");
                }
                (Some(was), Some(is)) if (is > was) == lower => {
                    sink.push(Class::Breaking, path, "bound tightened");
                }
                (Some(_), Some(_)) => sink.push(Class::Addition, path, "bound widened"),
                _ => sink.push(Class::Breaking, path, "bound changed shape"),
            }
        }
        _ => sink.push(Class::Breaking, path, format!("`{key}` changed")),
    }
}

fn properties(
    side: Side,
    path: &str,
    old: Option<&Value>,
    new: Option<&Value>,
    sink: &mut Sink<'_>,
) {
    let empty = Map::new();
    let before = old.and_then(Value::as_object).unwrap_or(&empty);
    let after = new.and_then(Value::as_object).unwrap_or(&empty);
    for name in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let child = format!("{path}/{}", escape_token(name));
        match (before.get(name), after.get(name)) {
            (Some(_), None) => sink.push(Class::Breaking, &child, "property removed"),
            (None, _) => sink.push(Class::Addition, &child, "property added"),
            (Some(left), Some(right)) => schema(side, &child, left, right, sink),
        }
    }
}

/// `$defs` : un ajout ou un retrait de définition est neutre (la propriété qui la référence
/// porte la classe) ; une définition modifiée est comparée comme un schéma.
fn definitions(
    side: Side,
    path: &str,
    old: Option<&Value>,
    new: Option<&Value>,
    sink: &mut Sink<'_>,
) {
    let empty = Map::new();
    let before = old.and_then(Value::as_object).unwrap_or(&empty);
    let after = new.and_then(Value::as_object).unwrap_or(&empty);
    for name in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let child = format!("{path}/{}", escape_token(name));
        match (before.get(name), after.get(name)) {
            (Some(left), Some(right)) => schema(side, &child, left, right, sink),
            _ => sink.push(Class::Fix, &child, "definition added or removed"),
        }
    }
}

/// Saut de version exigé par `class`, pour une version de départ donnée (05 §9.5) : avant 1.0, une
/// rupture et un ajout demandent `0.(x+1).0` ; à partir de 1.0, rupture = majeure, ajout = mineure ;
/// un correctif demande une version corrective.
fn required_jump(old_major: u64, class: Class) -> Jump {
    match class {
        Class::Fix => Jump::Patch,
        Class::Addition => Jump::Minor,
        Class::Breaking if old_major == 0 => Jump::Minor,
        Class::Breaking => Jump::Major,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Jump {
    None,
    Patch,
    Minor,
    Major,
}

fn parse_version(text: &str) -> Result<[u64; 3], String> {
    let parts = text
        .split('.')
        .map(|part| part.parse::<u64>().ok().filter(|_| !part.starts_with('+')))
        .collect::<Option<Vec<_>>>();
    match parts.as_deref() {
        Some(&[major, minor, patch]) => Ok([major, minor, patch]),
        _ => Err(format!("`{text}` is not a MAJOR.MINOR.PATCH version")),
    }
}

/// Contrôle que `new` saute assez de versions depuis `old` pour la classe `class` des changements.
///
/// # Errors
///
/// Une version illisible, une version qui recule, ou un saut trop petit.
pub(crate) fn check_version_jump(
    old: &str,
    new: &str,
    class: Option<Class>,
) -> Result<Jump, String> {
    let (from, to) = (parse_version(old)?, parse_version(new)?);
    if to < from {
        return Err(format!("version {new} is lower than the reference version {old}"));
    }
    let jump = if to[0] > from[0] {
        Jump::Major
    } else if to[1] > from[1] {
        Jump::Minor
    } else if to[2] > from[2] {
        Jump::Patch
    } else {
        Jump::None
    };
    if let Some(class) = class {
        let required = required_jump(from[0], class);
        if jump < required {
            return Err(format!(
                "{} changes need at least a {required:?} version bump from {old}, but {new} is a {jump:?} bump",
                class.label()
            ));
        }
    }
    Ok(jump)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_descriptors::{
        fixtures::{capture, surface, tool},
        lock::build,
        surface::Surface,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Verrou d'un serveur dont le seul outil est `fr_a`, modifié par `patch`.
    fn lock_with(patch: impl FnOnce(&mut Value)) -> Value {
        let mut descriptor = tool("fr_a");
        patch(&mut descriptor);
        let mut modern = capture("discover 2026-07-28", &[descriptor]);
        modern.supported_versions =
            Some(vec!["2026-07-28".into(), "2025-11-25".into(), "2025-06-18".into()]);
        build(&Surface::from_captures(&[modern]).unwrap()).unwrap()
    }

    fn classes(
        old_patch: impl FnOnce(&mut Value),
        new_patch: impl FnOnce(&mut Value),
    ) -> Vec<(Class, String)> {
        classify(&lock_with(old_patch), &lock_with(new_patch))
            .into_iter()
            .map(|change| (change.class, change.path))
            .collect()
    }

    fn one(old_patch: impl FnOnce(&mut Value), new_patch: impl FnOnce(&mut Value)) -> Class {
        let changes = classes(old_patch, new_patch);
        assert_eq!(changes.len(), 1, "{changes:?}");
        changes[0].0
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "table de cas de classement (05 §9.5) : un seul test, nommé par le plan"
    )]
    fn lock_diff_classification() {
        use Class::{Addition, Breaking, Fix};
        let same = classify(&lock_with(|_| {}), &lock_with(|_| {}));
        assert_eq!(same, Vec::new());
        assert_eq!(overall(&same), None);

        // Outils, instructions, capacités, versions de protocole.
        let base = build(&surface()).unwrap();
        let mut fewer = base.clone();
        fewer["tools"].as_object_mut().unwrap().remove("fr_coverage");
        let removed = classify(&base, &fewer);
        assert_eq!(
            removed,
            [Change { class: Breaking, path: "/tools/fr_coverage".into(), what: "removed".into() }]
        );
        let added = classify(&fewer, &base);
        assert_eq!(
            added,
            [Change { class: Addition, path: "/tools/fr_coverage".into(), what: "added".into() }]
        );
        let mut reworded = base.clone();
        reworded["instructions"]["text"] = json!("Other text.");
        assert_eq!(classify(&base, &reworded)[0].class, Fix);
        let mut wider = base.clone();
        wider["capabilities"]["resources"] = json!({});
        assert_eq!(
            classify(&base, &wider)[0],
            Change {
                class: Addition,
                path: "/capabilities/resources".into(),
                what: "capability added".into(),
            }
        );
        assert_eq!(classify(&wider, &base)[0].class, Breaking);
        let mut changed = base.clone();
        changed["capabilities"]["tools"] = json!({"listChanged": true});
        assert_eq!(classify(&base, &changed)[0].what, "capability changed");
        let mut fewer_versions = base.clone();
        fewer_versions["protocol_versions"] = json!(["2026-07-28", "2025-11-25"]);
        assert_eq!(classify(&base, &fewer_versions)[0].class, Breaking);
        assert_eq!(classify(&fewer_versions, &base)[0].class, Addition);

        // Textes et annotations d'un outil.
        assert_eq!(one(|_| {}, |t| t["description"] = json!("Reworded.")), Fix);
        assert_eq!(one(|_| {}, |t| t["title"] = json!("New title")), Fix);
        assert_eq!(one(|_| {}, |t| t["annotations"]["title"] = json!("New title")), Fix);
        assert_eq!(one(|_| {}, |t| t["annotations"]["readOnlyHint"] = json!(false)), Breaking);
        assert_eq!(one(|_| {}, |t| t["annotations"]["destructiveHint"] = json!(true)), Breaking);
        assert_eq!(one(|_| {}, |t| t["annotations"]["idempotentHint"] = json!(false)), Breaking);
        assert_eq!(one(|_| {}, |t| t["annotations"]["openWorldHint"] = json!(false)), Addition);
        assert_eq!(one(|t| t["annotations"]["destructiveHint"] = json!(true), |_| {}), Addition);
        assert_eq!(
            one(
                |_| {},
                |t| {
                    t["annotations"].as_object_mut().unwrap().remove("openWorldHint");
                }
            ),
            Fix
        );
        assert_eq!(
            one(
                |_| {},
                |t| {
                    t["annotations"].as_object_mut().unwrap().remove("readOnlyHint");
                }
            ),
            Breaking
        );
        assert_eq!(one(|_| {}, |t| t["annotations"]["extra"] = json!(1)), Breaking);
        assert_eq!(one(|_| {}, |t| t["icons"] = json!([])), Breaking);

        // Entrée : propriétés, requis, types, énumérations, bornes.
        assert_eq!(
            one(
                |_| {},
                |t| t["inputSchema"]["properties"]["query"]["description"] = json!("Words.")
            ),
            Fix
        );
        assert_eq!(
            one(|_| {}, |t| t["inputSchema"]["properties"]["extra"] = json!({"type": "integer"})),
            Addition
        );
        let without_query = |t: &mut Value| {
            t["inputSchema"]["properties"].as_object_mut().unwrap().remove("query");
            t["inputSchema"]["required"] = json!([]);
        };
        let removed = classes(|_| {}, without_query);
        assert!(
            removed.contains(&(Breaking, "/tools/fr_a/inputSchema/properties/query".to_owned())),
            "{removed:?}"
        );
        assert_eq!(
            one(|t| t["inputSchema"]["required"] = json!([]), |_| {}),
            Breaking,
            "un paramètre devenu requis"
        );
        assert_eq!(one(|_| {}, |t| t["inputSchema"]["required"] = json!([])), Addition);
        assert_eq!(
            one(
                |_| {},
                |t| t["inputSchema"]["properties"]["query"]["type"] = json!(["string", "null"])
            ),
            Addition
        );
        assert_eq!(
            one(
                |t| t["inputSchema"]["properties"]["query"]["type"] = json!(["string", "null"]),
                |_| {}
            ),
            Breaking
        );
        assert_eq!(
            one(|_| {}, |t| t["inputSchema"]["properties"]["query"]["type"] = json!("integer")),
            Breaking
        );
        let with_enum = |values: Value| {
            move |t: &mut Value| t["inputSchema"]["properties"]["query"]["enum"] = values
        };
        assert_eq!(one(with_enum(json!(["a"])), with_enum(json!(["a", "b"]))), Addition);
        assert_eq!(one(with_enum(json!(["a", "b"])), with_enum(json!(["a"]))), Breaking);
        assert_eq!(one(with_enum(json!(["a"])), with_enum(json!(["b"]))), Breaking);
        assert_eq!(one(|_| {}, with_enum(json!(["a"]))), Breaking, "un enum ajouté restreint");
        let min_length = |n: u64| {
            move |t: &mut Value| t["inputSchema"]["properties"]["query"]["minLength"] = json!(n)
        };
        assert_eq!(one(min_length(1), min_length(2)), Breaking);
        assert_eq!(one(min_length(2), min_length(1)), Addition);
        assert_eq!(
            one(
                |_| {},
                |t| {
                    t["inputSchema"]["properties"]["query"]
                        .as_object_mut()
                        .unwrap()
                        .remove("minLength");
                }
            ),
            Addition
        );
        let max_length = |n: u64| {
            move |t: &mut Value| t["inputSchema"]["properties"]["query"]["maxLength"] = json!(n)
        };
        assert_eq!(one(|_| {}, max_length(10)), Breaking);
        assert_eq!(one(max_length(10), max_length(20)), Addition);
        assert_eq!(one(max_length(20), max_length(10)), Breaking);
        assert_eq!(
            one(max_length(10), |t| {
                t["inputSchema"]["properties"]["query"]
                    .as_object_mut()
                    .unwrap()
                    .remove("maxLength");
            }),
            Addition
        );
        assert_eq!(
            one(|_| {}, |t| t["inputSchema"]["properties"]["query"]["minLength"] = json!("2")),
            Breaking
        );
        assert_eq!(
            one(|_| {}, |t| t["inputSchema"]["properties"]["query"]["pattern"] = json!("^a")),
            Breaking
        );
        assert_eq!(
            one(|_| {}, |t| t["inputSchema"]["properties"]["query"]["default"] = json!("a")),
            Breaking
        );
        assert_eq!(
            one(|_| {}, |t| t["inputSchema"]["properties"]["query"] = json!(true)),
            Breaking
        );
        assert_eq!(
            one(|_| {}, |t| t["inputSchema"]["additionalProperties"] = json!(true)),
            Addition
        );
        assert_eq!(
            one(|t| t["inputSchema"]["additionalProperties"] = json!(true), |_| {}),
            Breaking
        );

        // Sortie : seul l'ajout de champ est un ajout.
        assert_eq!(
            one(
                |_| {},
                |t| t["outputSchema"]["properties"]["hits"]["description"] = json!("Hits.")
            ),
            Fix
        );
        assert_eq!(
            one(|_| {}, |t| t["outputSchema"]["properties"]["more"] = json!({"type": "boolean"})),
            Addition
        );
        let without_hits = |t: &mut Value| {
            t["outputSchema"]["properties"].as_object_mut().unwrap().remove("hits");
            t["outputSchema"]["required"] = json!([]);
        };
        assert!(
            classes(|_| {}, without_hits)
                .contains(&(Breaking, "/tools/fr_a/outputSchema/properties/hits".into()))
        );
        assert_eq!(
            one(|_| {}, |t| t["outputSchema"]["properties"]["hits"]["type"] = json!("string")),
            Breaking
        );
        assert_eq!(
            one(
                |_| {},
                |t| t["outputSchema"]["properties"]["hits"]["items"]["type"] = json!("integer")
            ),
            Breaking
        );
        assert_eq!(one(|_| {}, |t| t["outputSchema"]["required"] = json!(["hits", "x"])), Addition);
        assert_eq!(one(|_| {}, |t| t["outputSchema"]["required"] = json!([])), Breaking);
        assert_eq!(
            one(|_| {}, |t| t["outputSchema"]["properties"]["hits"]["minItems"] = json!(1)),
            Breaking
        );
        assert_eq!(
            one(|_| {}, |t| t["outputSchema"]["properties"]["hits"]["enum"] = json!([[]])),
            Breaking
        );

        // `$defs` : l'ajout est neutre, la modification est comparée.
        let defs = |t: &mut Value| t["outputSchema"]["$defs"] = json!({"Hit": {"type": "string"}});
        assert_eq!(one(|_| {}, defs), Fix);
        assert_eq!(
            one(defs, |t| t["outputSchema"]["$defs"] = json!({"Hit": {"type": "integer"}})),
            Breaking
        );
        assert_eq!(
            one(
                |t| t["inputSchema"]["$defs"] =
                    json!({"Hit": {"type": "string", "description": "a"}}),
                |t| t["inputSchema"]["$defs"] =
                    json!({"Hit": {"type": "string", "description": "b"}})
            ),
            Fix
        );
        assert_eq!(overall(&classify(&base, &fewer)), Some(Breaking));
        let mixed = classify(
            &lock_with(|_| {}),
            &lock_with(|t| {
                t["description"] = json!("Reworded.");
                t["inputSchema"]["properties"]["extra"] = json!({"type": "integer"});
            }),
        );
        assert_eq!(overall(&mixed), Some(Addition));
        assert_eq!(
            mixed.iter().map(|change| (change.class, change.path.as_str())).collect::<Vec<_>>(),
            [
                (Fix, "/tools/fr_a/description"),
                (Addition, "/tools/fr_a/inputSchema/properties/extra")
            ]
        );
    }

    #[test]
    fn lock_diff_classification_of_prompts_resources_and_templates() {
        use Class::{Addition, Breaking, Fix};
        let with_prompt = |prompt: Option<Value>| {
            let mut lock = build(&surface()).unwrap();
            if let Some(prompt) = prompt {
                lock["prompts"] = json!({"review": {"sha256": "x", "descriptor": prompt}});
            }
            lock
        };
        let prompt = |arguments: Value| json!({"name": "review", "description": "Review.", "arguments": arguments});
        let classify_prompts =
            |old: Value, new: Value| classify(&with_prompt(Some(old)), &with_prompt(Some(new)));
        let optional = json!([{"name": "q", "required": false, "description": "Query."}]);
        let required = json!([{"name": "q", "required": true, "description": "Query."}]);
        let none = json!([]);
        assert_eq!(
            classify(&with_prompt(None), &with_prompt(Some(prompt(none.clone()))))[0].class,
            Addition
        );
        assert_eq!(
            classify(&with_prompt(Some(prompt(none.clone()))), &with_prompt(None))[0].class,
            Breaking
        );
        assert_eq!(
            classify_prompts(prompt(none.clone()), prompt(optional.clone()))[0].class,
            Addition
        );
        assert_eq!(
            classify_prompts(prompt(none.clone()), prompt(required.clone()))[0].class,
            Breaking
        );
        assert_eq!(
            classify_prompts(prompt(optional.clone()), prompt(none.clone()))[0].class,
            Breaking
        );
        assert_eq!(
            classify_prompts(prompt(optional.clone()), prompt(required.clone()))[0].class,
            Breaking
        );
        assert_eq!(
            classify_prompts(prompt(required.clone()), prompt(optional.clone()))[0].class,
            Addition
        );
        let reworded = json!([{"name": "q", "required": false, "description": "Words."}]);
        assert_eq!(classify_prompts(prompt(optional.clone()), prompt(reworded))[0].class, Fix);
        let retyped = json!([{"name": "q", "required": false, "description": "Query.", "x": 1}]);
        assert_eq!(classify_prompts(prompt(optional.clone()), prompt(retyped))[0].class, Breaking);
        let mut lock = with_prompt(None);
        lock["resources"] =
            json!({"u": {"sha256": "x", "descriptor": {"uri": "u", "description": "d"}}});
        let mut other = lock.clone();
        other["resources"]["u"]["descriptor"]["description"] = json!("e");
        assert_eq!(classify(&lock, &other)[0].class, Fix);
        other["resources"]["u"]["descriptor"]["mimeType"] = json!("text/plain");
        assert_eq!(
            classify(&lock, &other).iter().map(|c| c.class).collect::<Vec<_>>(),
            [Fix, Breaking]
        );
    }

    #[test]
    fn the_check_report_lists_added_removed_and_changed_with_json_paths() {
        let base = build(&surface()).unwrap();
        assert_eq!(report(&base, &base), Vec::<String>::new());
        let mut edited = base.clone();
        edited["tools"].as_object_mut().unwrap().remove("fr_coverage");
        edited["tools"]["fr_extra"] = base["tools"]["fr_search"].clone();
        edited["tools"]["fr_search"]["descriptor"]["description"] = json!("Reworded.");
        edited["tools"]["fr_search"]["descriptor"]["inputSchema"]["properties"]["query"]["type"] =
            json!("integer");
        edited["instructions"]["text"] = json!("Changed.");
        edited["prompts"]["p"] = json!({});
        edited["resources"] = json!({"u": {}});
        let mut shrunk = edited.clone();
        shrunk["resources"] = json!({});
        assert_eq!(
            report(&base, &edited),
            [
                "tools added: fr_extra",
                "tools removed: fr_coverage",
                "tools changed: fr_search (/descriptor/description, /descriptor/inputSchema/properties/query/type)",
                "prompts added: p",
                "resources added: u",
                "other fields changed: /instructions/text",
            ]
        );
        assert_eq!(report(&edited, &shrunk), ["resources removed: u"]);
        let mut other = base.clone();
        other["server_version"] = json!("0.2.0");
        assert_eq!(report(&base, &other), ["other fields changed: /server_version"]);
    }

    #[test]
    fn version_jumps_follow_the_semver_policy_of_the_surface() {
        use Class::{Addition, Breaking, Fix};
        let ok = |old: &str, new: &str, class| check_version_jump(old, new, class);
        // Avant 1.0 : rupture et ajout = 0.(x+1).0, correctif = 0.x.(y+1).
        assert_eq!(ok("0.1.0", "0.2.0", Some(Breaking)), Ok(Jump::Minor));
        assert_eq!(ok("0.1.0", "0.2.0", Some(Addition)), Ok(Jump::Minor));
        assert_eq!(ok("0.1.0", "0.1.1", Some(Fix)), Ok(Jump::Patch));
        assert_eq!(ok("0.1.0", "1.0.0", Some(Breaking)), Ok(Jump::Major));
        assert_eq!(ok("0.1.0", "0.1.0", None), Ok(Jump::None));
        assert_eq!(ok("0.1.0", "0.1.3", None), Ok(Jump::Patch));
        assert_eq!(
            ok("0.1.0", "0.1.1", Some(Breaking)).unwrap_err(),
            "breaking changes need at least a Minor version bump from 0.1.0, but 0.1.1 is a Patch bump"
        );
        assert!(ok("0.1.0", "0.1.1", Some(Addition)).is_err());
        assert!(ok("0.1.0", "0.1.0", Some(Fix)).unwrap_err().contains("is a None bump"));
        // À partir de 1.0 : rupture = majeure, ajout = mineure, correctif = corrective.
        assert_eq!(ok("1.2.3", "2.0.0", Some(Breaking)), Ok(Jump::Major));
        assert!(ok("1.2.3", "1.3.0", Some(Breaking)).is_err());
        assert_eq!(ok("1.2.3", "1.3.0", Some(Addition)), Ok(Jump::Minor));
        assert!(ok("1.2.3", "1.2.4", Some(Addition)).is_err());
        assert_eq!(ok("1.2.3", "1.2.4", Some(Fix)), Ok(Jump::Patch));
        // Versions illisibles ou qui reculent.
        for bad in ["1.2", "1.2.3.4", "a.b.c", "1.2.x", "1.2.3-rc.1", "1.+2.3", "", "-1.0.0"] {
            assert_eq!(
                ok(bad, "1.0.0", None).unwrap_err(),
                format!("`{bad}` is not a MAJOR.MINOR.PATCH version"),
                "{bad}"
            );
            assert!(ok("1.0.0", bad, None).is_err(), "{bad}");
        }
        assert_eq!(
            ok("0.2.0", "0.1.9", None).unwrap_err(),
            "version 0.1.9 is lower than the reference version 0.2.0"
        );
        assert_eq!(ok("0.1.9", "0.1.10", Some(Fix)), Ok(Jump::Patch));
        assert_eq!(Class::Fix.label(), "fix");
    }

    #[test]
    fn numbers_beyond_i64_are_still_comparable_and_floats_are_not() {
        assert_eq!(number(Some(&json!(-3))), Some(-3));
        assert_eq!(number(Some(&json!(u64::MAX))), Some(i128::from(u64::MAX)));
        assert_eq!(number(Some(&json!(1.5))), None);
        assert_eq!(number(Some(&json!("7"))), None);
        assert_eq!(number(None), None);
    }

    #[test]
    fn prompt_arguments_without_a_usable_name_are_ignored() {
        let with_arguments = |arguments: Value| {
            let mut lock = build(&surface()).unwrap();
            let prompt = json!({"name": "review", "arguments": arguments});
            lock["prompts"] = json!({"review": {"sha256": "x", "descriptor": prompt}});
            lock
        };
        let nameless = json!([{"required": true}, {"name": 5, "required": true}]);
        assert_eq!(classify(&with_arguments(json!([])), &with_arguments(nameless)), Vec::new());
    }

    #[test]
    fn a_schema_keyword_that_appears_or_disappears_is_classified_without_recursing() {
        let tags = |items: Option<Value>| {
            move |tool: &mut Value| {
                let mut schema = json!({"type": "array"});
                if let Some(items) = items {
                    schema["items"] = items;
                }
                tool["inputSchema"]["properties"]["tags"] = schema;
            }
        };
        // Un `items` qui apparaît ou disparaît n'a pas de forme connue des deux côtés : cassant
        // par prudence, dans les deux sens.
        let string = Some(json!({"type": "string"}));
        let path = "/tools/fr_a/inputSchema/properties/tags/items";
        assert_eq!(classes(tags(string.clone()), tags(None)), [(Class::Breaking, path.into())]);
        assert_eq!(classes(tags(None), tags(string.clone())), [(Class::Breaking, path.into())]);
        // Un schéma remplacé par un booléen : même prudence.
        assert_eq!(
            classes(tags(string), tags(Some(json!(true)))),
            [(Class::Breaking, path.into())]
        );
    }

    #[test]
    fn a_report_between_values_that_are_not_locks_names_the_root() {
        assert_eq!(report(&json!(null), &json!(true)), ["other fields changed: "]);
    }

    #[test]
    fn sub_schemas_of_items_and_additional_properties_are_compared_in_depth() {
        let with = |keyword: &'static str, schema: Value| {
            move |tool: &mut Value| {
                tool["inputSchema"]["properties"]["field"] =
                    json!({"type": "object", "properties": {}, keyword: schema});
            }
        };
        for keyword in ["items", "additionalProperties"] {
            assert_eq!(
                classes(
                    with(keyword, json!({"type": "string"})),
                    with(keyword, json!({"type": "integer"}))
                ),
                [(
                    Class::Breaking,
                    format!("/tools/fr_a/inputSchema/properties/field/{keyword}/type")
                )],
                "{keyword}"
            );
        }
    }
}
