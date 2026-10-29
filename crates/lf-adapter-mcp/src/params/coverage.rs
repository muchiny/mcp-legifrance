//! Paramètres de `fr_coverage` (05 §5.8) : aucun. Le schéma d'entrée est un objet fermé et vide ;
//! tout argument est refusé au décodage (`invalid_arg`), audité comme les autres.

use lf_application::input::CoverageInput;
use schemars::JsonSchema;
use serde::Deserialize;

/// Aucun argument : `{}` seul est valide.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("properties" = {}))]
pub(crate) struct CoverageParams {}

impl From<CoverageParams> for CoverageInput {
    fn from(CoverageParams {}: CoverageParams) -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use lf_application::input::ToolCall;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, schema::input_schema};

    fn decode(arguments: &Value) -> ToolCall<CoverageInput> {
        to_call::<CoverageParams, CoverageInput>(arguments.as_object().unwrap().clone())
    }

    #[test]
    fn empty_arguments_are_valid() {
        assert_eq!(decode(&json!({})).input.unwrap(), CoverageInput);
    }

    #[test]
    fn coverage_rejects_any_argument() {
        // L'échec voyage dans `ToolCall::input` : la façade le rend en `invalid_arg` audité.
        let call = decode(&json!({"foo": 1}));
        assert_eq!(
            call.input.unwrap_err().message,
            "foo: unknown field `foo`, there are no fields"
        );
        // Même une valeur nulle ou vide est un argument.
        assert!(decode(&json!({"query": null})).input.is_err());
        assert!(decode(&json!({"detail": ""})).input.is_err());
        // L'empreinte porte sur les arguments bruts, valides ou non.
        assert_eq!(call.canonical.as_str(), r#"{"foo":1}"#);
    }

    #[test]
    fn the_schema_is_a_closed_empty_object() {
        let schema = Value::Object(input_schema::<CoverageParams>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], json!({}));
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema.get("required").is_none());
        assert!(schema.get("$schema").is_none());
    }
}
