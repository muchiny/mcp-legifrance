//! JSON canonique du verrou (05 §9.2 points 6 et 7, §9.3) : clés triées, aucun flottant.
//!
//! La forme compacte est octet pour octet celle de
//! `json.dumps(sort_keys=True, ensure_ascii=False, separators=(",", ":"))` : `serde_json` échappe
//! `"`, `\`, les contrôles (`\n \r \t \b \f`, puis `\u00xx` en minuscules) et laisse le reste brut,
//! comme Python sans `ensure_ascii`. Le tri est explicite (point de code, soit l'ordre des octets
//! UTF-8) pour ne pas dépendre de la feature `preserve_order` de `serde_json`, que l'unification des
//! features du graphe peut activer.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::sha256;

/// Clés d'un objet dans l'ordre des points de code (l'ordre des octets UTF-8).
fn sorted_entries(map: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    entries
}

/// Copie de `value` aux clés triées à chaque profondeur.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        Value::Object(map) => Value::Object(
            sorted_entries(map)
                .into_iter()
                .map(|(key, item)| (key.clone(), sorted(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Encodage d'un jeton de pointeur JSON (RFC 6901) : `~` puis `/`.
pub(crate) fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Refuse les nombres flottants : un descripteur n'en porte pas (05 §9.2 point 6), car leur
/// écriture diffère d'un langage à l'autre et ferait varier l'empreinte. À appeler avant
/// [`canonical`], [`pretty`] et [`digest`], qui mettraient en forme un flottant sans le signaler.
///
/// # Errors
///
/// Le pointeur JSON du premier nombre flottant rencontré (clés dans l'ordre des points de code).
pub(crate) fn reject_floats(value: &Value) -> Result<(), String> {
    first_float(value, "")
}

fn first_float(value: &Value, path: &str) -> Result<(), String> {
    match value {
        Value::Number(number) if number.is_f64() => Err(format!(
            "{path}: floating-point number {number} (no float is allowed in a descriptor)"
        )),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(index, item)| first_float(item, &format!("{path}/{index}"))),
        Value::Object(map) => sorted_entries(map).into_iter().try_for_each(|(key, item)| {
            first_float(item, &format!("{path}/{}", escape_token(key)))
        }),
        _ => Ok(()),
    }
}

/// JSON canonique compact de `value`.
pub(crate) fn canonical(value: &Value) -> String {
    sorted(value).to_string()
}

/// JSON indenté de deux espaces, clés triées, saut de ligne final : forme du fichier de verrou.
pub(crate) fn pretty(value: &Value) -> String {
    format!("{:#}\n", sorted(value))
}

/// Empreinte SHA-256 hexadécimale du JSON canonique de `value`.
pub(crate) fn digest(value: &Value) -> String {
    sha256::hex(&sha256::sha256(canonical(value).as_bytes()))
}

/// Pointeurs JSON des feuilles qui diffèrent entre `old` et `new`, triés. Un tableau de longueur
/// différente est signalé en bloc ; un type différent aussi.
pub(crate) fn changed_paths(old: &Value, new: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_changes(old, new, "", &mut paths);
    paths.sort();
    paths
}

fn collect_changes(old: &Value, new: &Value, path: &str, out: &mut Vec<String>) {
    match (old, new) {
        _ if old == new => {}
        (Value::Object(left), Value::Object(right)) => {
            let keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
            for key in keys {
                let child = format!("{path}/{}", escape_token(key));
                match (left.get(key), right.get(key)) {
                    (Some(before), Some(after)) => collect_changes(before, after, &child, out),
                    _ => out.push(child),
                }
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (before, after)) in left.iter().zip(right).enumerate() {
                collect_changes(before, after, &format!("{path}/{index}"), out);
            }
        }
        _ => out.push(path.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn canonical_json_matches_python() {
        // Vecteurs de 06 §2.8, calculés avec
        // `json.dumps(sort_keys=True, ensure_ascii=False, separators=(",", ":"))`.
        let sample = json!({"b": 1, "a": "é", "c": {"z": null, "y": [true, 2]}});
        assert_eq!(canonical(&sample), r#"{"a":"é","b":1,"c":{"y":[true,2],"z":null}}"#);
        assert_eq!(
            digest(&sample),
            "ba3d64630739f6049ffe92346e10b9805aa8833d82fb2f08d2ba379cd5471296"
        );
        let coverage = json!({
            "name": "fr_coverage",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        });
        assert_eq!(
            digest(&coverage),
            "662718e129b8211e1a3da8d30d1200902cffe8f3f3009c55dbbcc78bab1775b2"
        );
    }

    #[test]
    fn canonical_json_escapes_like_python_without_ensure_ascii() {
        // Contrôles en `\u00XX` minuscule, `\n \r \t \b \f` courts, `"` et `\` échappés ;
        // `/`, DEL, U+2028 et les caractères hors BMP restent bruts.
        let text = "q\"\\/\n\r\t\u{8}\u{c}\u{1}\u{1f}\u{7f}\u{2028}😀é";
        let expected = "\"q\\\"\\\\/\\n\\r\\t\\b\\f\\u0001\\u001f\u{7f}\u{2028}😀é\"";
        assert_eq!(canonical(&json!(text)), expected);
    }

    #[test]
    fn canonical_json_sorts_keys_by_code_point_at_every_depth() {
        // « Z » (U+005A) < « a » (U+0061) < « é » (U+00E9) < « 😀 » (U+1F600), quel que soit
        // l'ordre d'insertion.
        let mut inner = serde_json::Map::new();
        for key in ["😀", "é", "a", "Z"] {
            inner.insert(key.to_owned(), json!(1));
        }
        let value = json!({"x": [{"b": inner.clone(), "a": 0}]});
        assert_eq!(canonical(&value), r#"{"x":[{"a":0,"b":{"Z":1,"a":1,"é":1,"😀":1}}]}"#);
    }

    #[test]
    fn floats_are_rejected_with_their_json_pointer() {
        for text in [r#"{"a": 1.5}"#, r#"{"a": 1.0}"#, r#"{"a": 1e3}"#] {
            let value: Value = serde_json::from_str(text).unwrap();
            let error = reject_floats(&value).unwrap_err();
            assert!(error.contains("floating-point"), "{text} : {error}");
        }
        let nested: Value = serde_json::from_str(r#"{"tools": [{"max": [0, 2.5]}]}"#).unwrap();
        assert_eq!(
            reject_floats(&nested).unwrap_err(),
            "/tools/0/max/1: floating-point number 2.5 (no float is allowed in a descriptor)"
        );
        // Le premier flottant dans l'ordre des clés, et les clés échappées dans le chemin.
        let two: Value = serde_json::from_str(r#"{"b": 2.5, "a/~": {"c": 0.5}}"#).unwrap();
        assert_eq!(
            reject_floats(&two).unwrap_err(),
            "/a~1~0/c: floating-point number 0.5 (no float is allowed in a descriptor)"
        );
        // Les entiers de toute taille et signe, les chaînes et les nuls passent.
        let integers = json!({"n": -3, "big": u64::MAX, "s": "1.5", "z": null, "t": [true]});
        assert_eq!(reject_floats(&integers), Ok(()));
        assert_eq!(
            canonical(&integers),
            r#"{"big":18446744073709551615,"n":-3,"s":"1.5","t":[true],"z":null}"#
        );
    }

    #[test]
    fn pretty_json_is_indented_by_two_sorted_and_ends_with_a_newline() {
        let value = json!({"b": [], "a": {"y": {}, "x": [1, "é"]}});
        assert_eq!(
            pretty(&value),
            "{\n  \"a\": {\n    \"x\": [\n      1,\n      \"é\"\n    ],\n    \"y\": {}\n  },\n  \"b\": []\n}\n"
        );
    }

    #[test]
    fn changed_paths_lists_the_leaves_that_differ() {
        let old = json!({"a": 1, "b": {"c": [1, 2], "d": "x"}, "gone": true, "list": [1, 2]});
        let new = json!({"a": 2, "b": {"c": [1, 3], "d": "x"}, "new": 0, "list": [1, 2, 3]});
        assert_eq!(changed_paths(&old, &new), ["/a", "/b/c/1", "/gone", "/list", "/new"]);
        assert_eq!(changed_paths(&old, &old), Vec::<String>::new());
    }

    #[test]
    fn changed_paths_escapes_pointer_tokens() {
        let old = json!({"a/b": {"~": 1}});
        let new = json!({"a/b": {"~": 2}});
        assert_eq!(changed_paths(&old, &new), ["/a~1b/~0"]);
        assert_eq!(changed_paths(&json!(1), &json!("1")), [""]);
    }
}
