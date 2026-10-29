//! `JsonObject` → `ToolCall<I>` : le décodage des arguments est le nôtre, pas celui de rmcp
//! (05 §3.3, ADR-0008).
//!
//! Avec l'extracteur `Parameters<P>`, un échec de désérialisation deviendrait un résultat
//! `failed to deserialize parameters: …` produit par le routeur **avant** notre code : ni audité,
//! ni préfixé `[invalid_arg]`. Ici, l'échec voyage dans `ToolCall::input` jusqu'à `Audited::run`,
//! qui le rend en `invalid_arg` et écrit sa ligne d'audit.

use lf_application::input::{InputError, ToolCall};
use rmcp::model::JsonObject;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::canonical;

/// Décode `args` en `P` puis le convertit en entrée applicative `I`. Ne rend jamais d'échec
/// elle-même : l'échec de décodage est porté par `ToolCall::input`, avec le message
/// « chemin: motif » de `serde_path_to_error` (par exemple
/// `page_size: invalid type: string "x", expected i64`), seul message que l'adapter rédige
/// (02 §9.3). L'empreinte (`canonical`) porte sur les arguments bruts, valides ou non.
#[allow(dead_code, reason = "STUB(V0) → V1 : employé par les tranches d'outils (tools/*.rs)")]
pub(crate) fn to_call<P, I>(args: JsonObject) -> ToolCall<I>
where
    P: DeserializeOwned + Into<I>,
{
    let canonical = canonical::of(&args);
    let input = serde_path_to_error::deserialize::<_, P>(Value::Object(args))
        .map(Into::into)
        .map_err(|error| InputError { message: error.to_string() });
    ToolCall { input, canonical }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::json;

    /// Paramètres témoins, au schéma des outils : champs optionnels, champs inconnus refusés.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Params {
        query: Option<String>,
        page_size: Option<i64>,
    }

    /// Entrée applicative témoin.
    #[derive(Debug, PartialEq, Eq)]
    struct Input {
        query: Option<String>,
        page_size: Option<i64>,
    }

    impl From<Params> for Input {
        fn from(params: Params) -> Self {
            Self { query: params.query, page_size: params.page_size }
        }
    }

    fn call(arguments: &Value) -> ToolCall<Input> {
        to_call::<Params, Input>(arguments.as_object().unwrap().clone())
    }

    #[test]
    fn valid_arguments_are_decoded_and_converted() {
        let decoded = call(&json!({"query": "bail", "page_size": 5}));
        assert_eq!(
            decoded.input.unwrap(),
            Input { query: Some("bail".into()), page_size: Some(5) }
        );
        assert_eq!(decoded.canonical.as_str(), r#"{"page_size":5,"query":"bail"}"#);
    }

    #[test]
    fn empty_arguments_decode_when_every_field_is_optional() {
        let decoded = call(&json!({}));
        assert_eq!(decoded.input.unwrap(), Input { query: None, page_size: None });
        assert_eq!(decoded.canonical.as_str(), "{}");
    }

    #[test]
    fn type_mismatch_message_is_path_then_reason() {
        let decoded = call(&json!({"page_size": "x"}));
        assert_eq!(
            decoded.input.unwrap_err().message,
            r#"page_size: invalid type: string "x", expected i64"#
        );
    }

    #[test]
    fn unknown_field_message_names_the_field_and_the_expected_ones() {
        let decoded = call(&json!({"foo": 1}));
        assert_eq!(
            decoded.input.unwrap_err().message,
            "foo: unknown field `foo`, expected `query` or `page_size`"
        );
    }

    #[test]
    fn decoding_error_keeps_the_canonical_input_of_the_raw_arguments() {
        let decoded = call(&json!({"query": "bail", "page_size": "x"}));
        assert!(decoded.input.is_err());
        assert_eq!(decoded.canonical.as_str(), r#"{"page_size":"x","query":"bail"}"#);
    }
}
