//! Schémas JSON des descripteurs d'outils (05 §2.5, §3.3, §3.4).
//!
//! `inputSchema` et `outputSchema` sont donnés explicitement à `#[tool]`, jamais déduits des
//! extracteurs, et ne portent jamais la clé `$schema` : les descripteurs des deux ères sont
//! ainsi identiques et leur taille reste bornée. Ils ne portent pas non plus les mots-clés que
//! schemars ajoute et que la norme ne connaît pas (`format: uint32`…), ni de `default: null`
//! sur un type qui n'accepte pas `null` : un client strict les refuserait (L5-01, protocol-9).

use std::{any::type_name, sync::Arc};

use serde_json::Value;

use rmcp::{
    handler::server::tool::{schema_for_input, schema_for_output},
    model::JsonObject,
};
use schemars::JsonSchema;

/// `inputSchema` de `P` : racine de type `object`, sans `title`, `description` ni `$schema`.
///
/// `schema_for_input` rend un `Result` que l'attribut `input_schema = …` n'accepte pas tel
/// quel : l'erreur est un défaut de programmation, qui doit tomber au montage du routeur
/// (le test de descripteurs de 0W l'attrape avant tout client).
///
/// # Panics
///
/// Si la racine du schéma de `P` n'est pas un objet.
pub(crate) fn input_schema<P: JsonSchema + 'static>() -> Arc<JsonObject> {
    object_root_or_panic(schema_for_input::<P>(), type_name::<P>())
}

/// Corps non générique d'[`input_schema`] : le même code pour tous les `P`, testé une fois.
#[expect(clippy::panic, reason = "défaut de programmation au montage du routeur, jamais à l'appel")]
fn object_root_or_panic(
    schema: Result<Arc<JsonObject>, String>,
    type_name: &str,
) -> Arc<JsonObject> {
    match schema {
        Ok(schema) => published(&schema, Side::Input),
        Err(error) => panic!("invalid inputSchema for {type_name}: {error}"),
    }
}

/// `outputSchema` de `V`, sans `title`, `description` ni `$schema`. Sans cet appel explicite,
/// rmcp déduit le schéma du type de retour `Json<V>` **avec** `$schema` (05 §3.4).
pub(crate) fn output_schema<V: JsonSchema + 'static>() -> Arc<JsonObject> {
    published(&schema_for_output::<V>(), Side::Output)
}

/// Le sens d'un schéma : ce que le client envoie ou ce qu'il reçoit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Input,
    Output,
}

/// Les `format` du vocabulaire standard de JSON Schema 2020-12 (validation, §7.3). Les
/// `uint32`, `uint`, `uint16` et `uint64` que schemars écrit pour les entiers non signés n'en
/// font pas partie : `minimum: 0` dit déjà la même chose. À garder identique à `xtask`
/// (`lock_descriptors/checks.rs`) et à `tests/descriptors.rs` : un test les compare.
const STANDARD_FORMATS: [&str; 19] = [
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "idn-email",
    "hostname",
    "idn-hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uri-reference",
    "iri",
    "iri-reference",
    "uuid",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

/// Mots-clés dont la valeur est un objet de sous-schémas, indexés par nom.
const SCHEMA_MAPS: [&str; 5] =
    ["properties", "$defs", "definitions", "patternProperties", "dependentSchemas"];

/// Mots-clés dont la valeur est un sous-schéma (`items` aussi, en liste, avec `SCHEMA_LISTS`).
const SCHEMA_SINGLES: [&str; 10] = [
    "items",
    "additionalProperties",
    "not",
    "if",
    "then",
    "else",
    "contains",
    "propertyNames",
    "unevaluatedItems",
    "unevaluatedProperties",
];

/// Mots-clés dont la valeur est une liste de sous-schémas.
const SCHEMA_LISTS: [&str; 5] = ["anyOf", "oneOf", "allOf", "prefixItems", "items"];

/// Schéma tel que le serveur le publie : copie de `schema` sans `$schema` (05 §2.5) et
/// [`standardize`].
fn published(schema: &JsonObject, side: Side) -> Arc<JsonObject> {
    let mut object = schema.clone();
    object.remove("$schema");
    standardize(&mut object, side);
    Arc::new(object)
}

/// Retire d'un schéma, à toute profondeur, ce que la norme ne connaît pas :
///
/// - un `format` hors de [`STANDARD_FORMATS`] ;
/// - un `default: null` (schemars l'écrit pour tout `#[serde(default)]` sur un `Option`, y compris
///   sur un `string` qui n'accepte pas `null`) ;
/// - en sortie seulement, `"null"` dans un `type` en liste (`["string", "null"]` devient
///   `"string"`) : les vues omettent un champ absent (`skip_serializing_if`), elles n'écrivent
///   jamais `null` (L5-01, point d).
///
/// Le parcours est conscient du schéma, jamais aveugle : il ne descend que dans les sous-schémas
/// (`properties`, `$defs`, `items`, `anyOf`…), jamais dans `enum`, `const`, `default` ou
/// `examples`, et une propriété *nommée* `format` ou `default` n'est qu'un nom. `minimum: 0` est
/// gardé, aucun `maximum` n'est ajouté.
fn standardize(schema: &mut JsonObject, side: Side) {
    if schema
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|name| !STANDARD_FORMATS.contains(&name))
    {
        schema.remove("format");
    }
    if schema.get("default").is_some_and(Value::is_null) {
        schema.remove("default");
    }
    if side == Side::Output {
        drop_null_type(schema);
    }
    for key in SCHEMA_MAPS {
        let named = schema.get_mut(key).and_then(Value::as_object_mut);
        for sub in named.into_iter().flat_map(|schemas| schemas.values_mut()) {
            standardize_value(sub, side);
        }
    }
    for key in SCHEMA_SINGLES {
        if let Some(sub) = schema.get_mut(key) {
            standardize_value(sub, side);
        }
    }
    for key in SCHEMA_LISTS {
        for sub in schema.get_mut(key).and_then(Value::as_array_mut).into_iter().flatten() {
            standardize_value(sub, side);
        }
    }
}

/// [`standardize`] d'un sous-schéma, qui peut être un booléen (`items: true`) : alors rien à faire.
fn standardize_value(value: &mut Value, side: Side) {
    if let Some(object) = value.as_object_mut() {
        standardize(object, side);
    }
}

/// `type: ["string", "null"]` devient `type: "string"` ; `["integer", "string", "null"]` devient
/// `["integer", "string"]`. Un `type` réduit à `null` seul reste tel quel.
fn drop_null_type(schema: &mut JsonObject) {
    let Some(Value::Array(types)) = schema.get("type") else { return };
    let kept: Vec<Value> = types.iter().filter(|name| **name != "null").cloned().collect();
    if kept.is_empty() || kept.len() == types.len() {
        return;
    }
    let reduced = match kept.as_slice() {
        [only] => only.clone(),
        _ => Value::Array(kept),
    };
    schema.insert("type".to_owned(), reduced);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::Serialize;
    use serde_json::json;

    /// Paramètres témoins : un champ obligatoire et un optionnel.
    #[derive(JsonSchema)]
    #[allow(dead_code, reason = "lu par schemars seulement")]
    struct Params {
        /// Free-text query.
        query: String,
        page_size: Option<i64>,
    }

    /// Sortie témoin.
    #[derive(Serialize, JsonSchema)]
    #[allow(dead_code, reason = "lu par schemars seulement")]
    struct View {
        count: u32,
    }

    #[test]
    fn input_schema_is_an_object_without_the_dollar_schema_key() {
        let schema = input_schema::<Params>();
        assert_eq!(schema.get("type"), Some(&json!("object")));
        assert_eq!(schema.get("required"), Some(&json!(["query"])));
        assert!(schema.contains_key("properties"));
        assert!(!schema.contains_key("$schema"));
    }

    #[test]
    fn output_schema_is_an_object_without_the_dollar_schema_key() {
        let schema = output_schema::<View>();
        assert_eq!(schema.get("type"), Some(&json!("object")));
        assert!(schema.contains_key("properties"));
        assert!(!schema.contains_key("$schema"));
    }

    #[test]
    fn a_schema_error_panics_at_mounting_time_with_the_type_name() {
        let payload = std::panic::catch_unwind(|| {
            object_root_or_panic(Err("root must be an object".to_owned()), "my::Params")
        })
        .unwrap_err();
        assert_eq!(
            payload.downcast_ref::<String>().unwrap(),
            "invalid inputSchema for my::Params: root must be an object"
        );
    }

    #[test]
    fn rmcp_refuses_a_non_object_root() {
        let error = schema_for_input::<String>().unwrap_err();
        assert!(error.contains("root type 'object'"), "{error}");
    }

    #[test]
    fn published_removes_the_dollar_schema_key_and_leaves_the_original_alone() {
        let schema =
            json!({"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object"});
        let object = schema.as_object().unwrap();
        let stripped = published(object, Side::Input);
        assert_eq!(serde_json::Value::Object(stripped.as_ref().clone()), json!({"type": "object"}));
        assert!(object.contains_key("$schema"), "l'original n'est pas modifié");
    }

    /// Paramètres témoins de L5-01 : un entier borné avec défaut, une date, deux défauts nuls.
    #[derive(JsonSchema)]
    #[allow(dead_code, reason = "lu par schemars seulement")]
    struct Bounded {
        #[schemars(with = "u32", range(min = 1, max = 100), extend("default" = 10))]
        page_size: Option<u32>,
        #[schemars(with = "String", extend("format" = "date"))]
        date: Option<String>,
        #[serde(default)]
        focus: Option<String>,
        count: u64,
    }

    #[test]
    fn a_bounded_integer_keeps_its_bounds_and_default_without_a_uint_format() {
        let schema = input_schema::<Bounded>();
        let page_size = &schema["properties"]["page_size"];
        assert_eq!(page_size["minimum"], 1);
        assert_eq!(page_size["maximum"], 100);
        assert_eq!(page_size["default"], 10);
        assert_eq!(page_size.get("format"), None, "{page_size}");
        assert_eq!(schema["properties"]["count"].get("format"), None);
        assert_eq!(schema["properties"]["count"]["minimum"], 0, "minimum: 0 est gardé");
        assert_eq!(schema["properties"]["count"].get("maximum"), None, "aucun maximum ajouté");
    }

    #[test]
    fn a_null_default_goes_but_the_date_format_stays() {
        let schema = input_schema::<Bounded>();
        assert_eq!(schema["properties"]["date"]["format"], "date");
        assert_eq!(schema["properties"]["focus"].get("default"), None, "{schema:?}");
        assert_eq!(
            schema["properties"]["focus"]["type"],
            json!(["string", "null"]),
            "en entrée, `null` reste accepté"
        );
    }

    #[test]
    fn standardize_walks_defs_items_and_combinators_and_spares_data() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "format": {"type": "string", "format": "uint32"},
                "default": {"type": "string", "default": null},
                "mode": {
                    "type": "string",
                    "enum": ["format", null],
                    "const": {"format": "uint32", "default": null},
                    "default": {"format": "uint32", "default": null},
                    "examples": [{"format": "uint32", "default": null}]
                },
                "list": {"type": "array", "items": {"type": "integer", "format": "uint"}},
                "pair": {"type": "array", "items": [{"format": "uint16"}], "prefixItems": [{"format": "uint64"}]},
                "either": {"anyOf": [{"type": "integer", "format": "uint32"}, {"type": "null"}]},
                "open": {"type": "object", "additionalProperties": {"format": "uint32"}, "not": true}
            },
            "$defs": {"Count": {"type": "integer", "format": "uint32", "default": null}},
            "allOf": [{"format": "uint32"}],
            "oneOf": [{"format": "uint32"}],
            "format": "uint32"
        });
        standardize(schema.as_object_mut().unwrap(), Side::Input);
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {
                    "format": {"type": "string"},
                    "default": {"type": "string"},
                    "mode": {
                        "type": "string",
                        "enum": ["format", null],
                        "const": {"format": "uint32", "default": null},
                        "default": {"format": "uint32", "default": null},
                        "examples": [{"format": "uint32", "default": null}]
                    },
                    "list": {"type": "array", "items": {"type": "integer"}},
                    "pair": {"type": "array", "items": [{}], "prefixItems": [{}]},
                    "either": {"anyOf": [{"type": "integer"}, {"type": "null"}]},
                    "open": {"type": "object", "additionalProperties": {}, "not": true}
                },
                "$defs": {"Count": {"type": "integer"}},
                "allOf": [{}],
                "oneOf": [{}]
            })
        );
    }

    /// Les chaînes de la constante `name` dans `source` (`const NAME: [&str; N] = [ "a", "b" ];`).
    fn quoted_items(source: &str, name: &str) -> Vec<String> {
        let head = format!("const {name}: [&str;");
        let from = source.find(&head).expect("constante introuvable");
        let body = source[from..].split_once("] =").unwrap().1.split_once(';').unwrap().0;
        body.split('"').skip(1).step_by(2).map(str::to_owned).collect()
    }

    /// Les copies de la liste des formats et des mots-clés de sous-schémas (ici, `xtask` et le
    /// test des descripteurs) restent identiques : le crate MCP ne peut pas être importé par
    /// `xtask` (L5-01).
    #[test]
    fn the_copies_of_the_schema_constants_stay_identical() {
        let checks = include_str!("../../../xtask/src/lock_descriptors/checks.rs");
        let descriptors = include_str!("../tests/descriptors.rs");
        let own = |name: &str| -> Vec<String> {
            match name {
                "STANDARD_FORMATS" => STANDARD_FORMATS.map(str::to_owned).to_vec(),
                "SCHEMA_MAPS" => SCHEMA_MAPS.map(str::to_owned).to_vec(),
                "SCHEMA_SINGLES" => SCHEMA_SINGLES.map(str::to_owned).to_vec(),
                _ => SCHEMA_LISTS.map(str::to_owned).to_vec(),
            }
        };
        for name in ["STANDARD_FORMATS", "SCHEMA_MAPS", "SCHEMA_SINGLES", "SCHEMA_LISTS"] {
            assert_eq!(quoted_items(checks, name), own(name), "{name} : xtask a dérivé");
        }
        assert_eq!(
            quoted_items(descriptors, "STANDARD_FORMATS"),
            own("STANDARD_FORMATS"),
            "STANDARD_FORMATS : tests/descriptors.rs a dérivé"
        );
    }

    #[test]
    fn standard_formats_survive_everywhere() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "a": {"type": "string", "format": "date"},
                "b": {"type": "string", "format": "date-time"},
                "c": {"type": "string", "format": "uri"}
            }
        });
        let before = schema.clone();
        standardize(schema.as_object_mut().unwrap(), Side::Output);
        assert_eq!(schema, before);
        for name in STANDARD_FORMATS {
            let mut one = json!({"format": name});
            standardize(one.as_object_mut().unwrap(), Side::Input);
            assert_eq!(one, json!({"format": name}));
        }
    }

    /// Sortie témoin : des champs absents plutôt que nuls (`skip_serializing_if`).
    #[derive(Serialize, JsonSchema)]
    #[allow(dead_code, reason = "lu par schemars seulement")]
    struct Optional {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        next_offset: Option<u32>,
        items: Vec<Entry>,
    }

    #[derive(Serialize, JsonSchema)]
    #[allow(dead_code, reason = "lu par schemars seulement")]
    struct Entry {
        #[serde(skip_serializing_if = "Option::is_none")]
        num: Option<String>,
    }

    #[test]
    fn an_output_schema_never_lists_null_but_keeps_its_types() {
        let schema = Value::Object(output_schema::<Optional>().as_ref().clone());
        assert_eq!(schema["properties"]["title"]["type"], "string", "{schema}");
        assert_eq!(schema["properties"]["next_offset"]["type"], "integer");
        assert_eq!(schema["properties"]["next_offset"]["minimum"], 0);
        assert_eq!(schema["properties"]["next_offset"].get("format"), None);
        assert!(!schema.to_string().contains("null"), "{schema}");
        assert_eq!(schema["properties"]["items"]["type"], "array");
    }

    #[test]
    fn null_is_dropped_from_output_types_only_when_another_type_remains() {
        let run = |types: Value, side: Side| {
            let mut schema = json!({"type": types});
            standardize(schema.as_object_mut().unwrap(), side);
            schema["type"].clone()
        };
        assert_eq!(run(json!(["string", "null"]), Side::Output), json!("string"));
        assert_eq!(run(json!(["null", "string"]), Side::Output), json!("string"));
        assert_eq!(
            run(json!(["integer", "string", "null"]), Side::Output),
            json!(["integer", "string"])
        );
        // Rien à retirer, ou rien qui resterait : le type est laissé tel quel.
        assert_eq!(run(json!(["string", "integer"]), Side::Output), json!(["string", "integer"]));
        assert_eq!(run(json!(["null"]), Side::Output), json!(["null"]));
        assert_eq!(run(json!(["null", "null"]), Side::Output), json!(["null", "null"]));
        assert_eq!(run(json!("null"), Side::Output), json!("null"));
        assert_eq!(run(json!("string"), Side::Output), json!("string"));
        // En entrée, `null` est accepté : le type n'est pas touché.
        assert_eq!(run(json!(["string", "null"]), Side::Input), json!(["string", "null"]));
    }
}
