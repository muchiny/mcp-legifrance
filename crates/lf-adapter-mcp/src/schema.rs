//! Schémas JSON des descripteurs d'outils (05 §2.5, §3.3, §3.4).
//!
//! `inputSchema` et `outputSchema` sont donnés explicitement à `#[tool]`, jamais déduits des
//! extracteurs, et ne portent jamais la clé `$schema` : les descripteurs des deux ères sont
//! ainsi identiques et leur taille reste bornée.

use std::{any::type_name, sync::Arc};

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
        Ok(schema) => without_dollar_schema(&schema),
        Err(error) => panic!("invalid inputSchema for {type_name}: {error}"),
    }
}

/// `outputSchema` de `V`, sans `title`, `description` ni `$schema`. Sans cet appel explicite,
/// rmcp déduit le schéma du type de retour `Json<V>` **avec** `$schema` (05 §3.4).
pub(crate) fn output_schema<V: JsonSchema + 'static>() -> Arc<JsonObject> {
    without_dollar_schema(&schema_for_output::<V>())
}

/// Copie de `schema` privée de sa clé `$schema` (05 §2.5).
pub(crate) fn without_dollar_schema(schema: &JsonObject) -> Arc<JsonObject> {
    let mut object = schema.clone();
    object.remove("$schema");
    Arc::new(object)
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
    fn without_dollar_schema_only_removes_that_key() {
        let schema =
            json!({"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object"});
        let object = schema.as_object().unwrap();
        let stripped = without_dollar_schema(object);
        assert_eq!(serde_json::Value::Object(stripped.as_ref().clone()), json!({"type": "object"}));
        assert!(object.contains_key("$schema"), "l'original n'est pas modifié");
    }
}
