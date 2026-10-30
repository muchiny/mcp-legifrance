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
/// `page_size: invalid type: sequence, expected an integer`), seul message que l'adapter rédige
/// (02 §9.3). L'empreinte (`canonical`) porte sur les arguments bruts, valides ou non.
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
    use lf_application::messages;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::lenient;

    /// Paramètres témoins, au schéma des outils : champs optionnels, entier tolérant, champs
    /// inconnus refusés.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Params {
        query: Option<String>,
        #[serde(default, deserialize_with = "lenient::opt_int")]
        page_size: Option<i64>,
    }

    /// Paramètres à quatre champs, ceux de `fr_get_act` (05 §3.3, message « expected one of »).
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code, reason = "champs lus par serde seulement : le test regarde les messages")]
    struct ActParams {
        text_id: Option<String>,
        date: Option<String>,
        #[serde(default, deserialize_with = "lenient::opt_int")]
        toc_offset: Option<i64>,
        #[serde(default, deserialize_with = "lenient::opt_int")]
        toc_limit: Option<i64>,
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

    impl From<ActParams> for Input {
        fn from(params: ActParams) -> Self {
            Self { query: params.text_id, page_size: params.toc_limit }
        }
    }

    fn call(arguments: &Value) -> ToolCall<Input> {
        to_call::<Params, Input>(arguments.as_object().unwrap().clone())
    }

    /// Arguments lus depuis leur texte JSON, comme sur le fil (un entier hors d'un `u64` y devient
    /// un flottant, ce que `json!` ne peut pas écrire).
    fn wire(json: &str) -> ToolCall<Input> {
        to_call::<Params, Input>(serde_json::from_str(json).unwrap())
    }

    fn act_error(arguments: &Value) -> String {
        to_call::<ActParams, Input>(arguments.as_object().unwrap().clone())
            .input
            .unwrap_err()
            .message
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
    fn lenient_integers_and_null_decode_but_the_canonical_input_stays_raw() {
        // `"5"` et `5.0` valent 5 ; `null` vaut « absent ». L'empreinte garde pourtant ce que le
        // client a envoyé (02 §6.1) : ni `5`, ni champ retiré.
        for (arguments, canonical) in [
            (r#"{"page_size": "5", "query": "bail"}"#, r#"{"page_size":"5","query":"bail"}"#),
            (r#"{"page_size": 5.0, "query": "bail"}"#, r#"{"page_size":5.0,"query":"bail"}"#),
        ] {
            let decoded = wire(arguments);
            assert_eq!(decoded.canonical.as_str(), canonical);
            assert_eq!(
                decoded.input.unwrap(),
                Input { query: Some("bail".into()), page_size: Some(5) }
            );
        }
        let decoded = wire(r#"{"page_size": null, "query": null}"#);
        assert_eq!(decoded.input.unwrap(), Input { query: None, page_size: None });
        assert_eq!(decoded.canonical.as_str(), r#"{"page_size":null,"query":null}"#);
    }

    #[test]
    fn four_field_arguments_decode_through_the_same_path() {
        let arguments = json!({
            "text_id": "LEGITEXT000006070721", "date": null, "toc_offset": "10", "toc_limit": 20.0
        });
        let decoded = to_call::<ActParams, Input>(arguments.as_object().unwrap().clone());
        assert_eq!(
            decoded.input.unwrap(),
            Input { query: Some("LEGITEXT000006070721".into()), page_size: Some(20) }
        );
        assert_eq!(
            decoded.canonical.as_str(),
            r#"{"date":null,"text_id":"LEGITEXT000006070721","toc_limit":20.0,"toc_offset":"10"}"#
        );
    }

    #[test]
    fn type_mismatch_message_is_path_then_reason() {
        let decoded = call(&json!({"page_size": [1]}));
        assert_eq!(
            decoded.input.unwrap_err().message,
            "page_size: invalid type: sequence, expected an integer"
        );
        let decoded = call(&json!({"page_size": "x"}));
        assert_eq!(
            decoded.input.unwrap_err().message,
            r#"page_size: invalid value: string "x", expected an integer"#
        );
        let decoded = call(&json!({"query": 3}));
        assert_eq!(
            decoded.input.unwrap_err().message,
            "query: invalid type: integer `3`, expected a string"
        );
    }

    #[test]
    fn unknown_field_message_lists_expected() {
        let decoded = call(&json!({"foo": 1}));
        assert_eq!(
            decoded.input.unwrap_err().message,
            "foo: unknown field `foo`, expected `query` or `page_size`"
        );
        // Avec plus de deux champs attendus, serde les liste tous (05 §3.3, §7.3).
        assert_eq!(
            act_error(&json!({"foo": 1})),
            "foo: unknown field `foo`, expected one of `text_id`, `date`, `toc_offset`, `toc_limit`"
        );
        // Le premier champ inconnu est nommé, même au milieu d'arguments valides.
        assert_eq!(
            act_error(&json!({"text_id": "LEGITEXT000006070721", "toc_limt": 5})),
            "toc_limt: unknown field `toc_limt`, expected one of `text_id`, `date`, `toc_offset`, `toc_limit`"
        );
    }

    #[test]
    fn invalid_arguments_keep_canonical_input() {
        // Le décodage échoue, l'empreinte porte pourtant sur les arguments bruts, clés triées.
        let decoded = call(&json!({"query": "bail", "page_size": "x"}));
        assert!(decoded.input.is_err());
        assert_eq!(decoded.canonical.as_str(), r#"{"page_size":"x","query":"bail"}"#);

        let decoded = call(&json!({"query": "vie privée", "foo": {"b": 1, "a": [true]}}));
        assert!(decoded.input.is_err());
        assert_eq!(
            decoded.canonical.as_str(),
            r#"{"foo":{"a":[true],"b":1},"query":"vie priv\u00e9e"}"#
        );

        let decoded = wire(r#"{"page_size": 1.5}"#);
        assert_eq!(
            decoded.input.unwrap_err().message,
            "page_size: invalid value: floating point `1.5`, expected an integer"
        );
        assert_eq!(decoded.canonical.as_str(), r#"{"page_size":1.5}"#);
    }

    #[test]
    fn huge_integer_decodes_to_a_value_the_use_case_rejects_as_out_of_range() {
        let decoded = wire(r#"{"page_size": 100000000000000000000, "query": "bail"}"#);
        // L'empreinte écrit le flottant que serde_json a lu, comme Python l'écrirait pour 1e20.
        assert_eq!(decoded.canonical.as_str(), r#"{"page_size":1e+20,"query":"bail"}"#);
        let page_size = decoded.input.unwrap().page_size.unwrap();
        assert_eq!(page_size, i64::MAX);
        assert_eq!(
            messages::out_of_range("page_size", page_size, 1, 100).message(),
            "page_size=9223372036854775807 out of range (1..100)."
        );
    }
}
