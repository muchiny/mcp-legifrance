//! JSON canonique des arguments bruts, pour l'empreinte d'audit (02 §6.1).
//!
//! STUB(V0) → V1-09 : la forme actuelle est le JSON compact de `serde_json`, aux clés triées (le
//! crate n'active pas `preserve_order`). V1-09 la rend identique à
//! `json.dumps(sort_keys=True, separators=(",", ":"))` avec l'échappement `\uXXXX` d'`ensure_ascii`
//! (vecteurs P-041).
#![allow(dead_code, reason = "STUB(V0) → V1 : employé par `decode::to_call` des tranches d'outils")]

use lf_application::input::CanonicalInput;
use rmcp::model::JsonObject;
use serde_json::Value;

/// JSON canonique de l'objet `arguments` **brut**, avant tout décodage : les arguments invalides
/// ont donc eux aussi une empreinte.
pub(crate) fn of(args: &JsonObject) -> CanonicalInput {
    // STUB(V0) → V1-09 : sérialisation compacte de serde_json.
    CanonicalInput::new(Value::Object(args.clone()).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn canonical(value: &Value) -> String {
        of(value.as_object().unwrap()).as_str().to_owned()
    }

    #[test]
    fn keys_are_sorted_and_the_output_is_compact() {
        assert_eq!(
            canonical(&json!({"b": 1, "a": [true, null], "c": "x"})),
            r#"{"a":[true,null],"b":1,"c":"x"}"#
        );
    }

    #[test]
    fn nested_objects_are_sorted_too() {
        assert_eq!(canonical(&json!({"z": {"b": 1, "a": 2}})), r#"{"z":{"a":2,"b":1}}"#);
    }

    #[test]
    fn empty_arguments_give_an_empty_object() {
        assert_eq!(canonical(&json!({})), "{}");
    }
}
