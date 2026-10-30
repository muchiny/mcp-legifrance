//! Paramètres de `fr_get_decision` (05 §5.4) : `decision_id`, plus les cinq paramètres de lecture
//! communs aux lectures longues ([`ReadingParams`], aplatis).
//!
//! Le champ d'identifiant est `Option` pour que son absence (ou `null`) atteigne le cas d'usage, qui
//! rend la phrase de parité `decision_id is required.` ; le schéma, lui, le déclare requis
//! (`#[schemars(required)]`). Les descriptions du schéma sont en anglais (Q14), posées par
//! `#[schemars(description)]` et non par un commentaire de doc.

use lf_application::input::GetDecisionInput;
use schemars::JsonSchema;
use serde::Deserialize;

use super::reading::ReadingParams;

/// Arguments de l'outil, tels que le client les envoie.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetDecisionParams {
    // JURITEXT, CONSTEXT ou CETATEXT + 12 chiffres, vérifié par le cas d'usage.
    #[schemars(
        required,
        description = "A JURITEXT..., CONSTEXT... or CETATEXT... id (from fr_search on JURI, CONSTIT or CETAT)."
    )]
    pub(crate) decision_id: Option<String>,
    #[serde(flatten)]
    pub(crate) reading: ReadingParams,
}

impl From<GetDecisionParams> for GetDecisionInput {
    fn from(params: GetDecisionParams) -> Self {
        Self { decision_id: params.decision_id, reading: params.reading.into() }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::input::{ReadingInput, ToolCall};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, schema::input_schema};

    fn decode(arguments: &Value) -> ToolCall<GetDecisionInput> {
        to_call::<GetDecisionParams, GetDecisionInput>(arguments.as_object().unwrap().clone())
    }

    fn schema() -> Value {
        Value::Object(input_schema::<GetDecisionParams>().as_ref().clone())
    }

    #[test]
    fn the_id_is_decoded_as_given() {
        let call = decode(&json!({"decision_id": " constext000052555275 "}));
        assert_eq!(
            call.input.unwrap(),
            GetDecisionInput {
                decision_id: Some(" constext000052555275 ".to_owned()),
                reading: ReadingInput::default()
            }
        );
        assert_eq!(call.canonical.as_str(), r#"{"decision_id":" constext000052555275 "}"#);
    }

    #[test]
    fn an_absent_or_null_id_reaches_the_use_case_as_absent() {
        for arguments in [json!({}), json!({"decision_id": null})] {
            assert_eq!(decode(&arguments).input.unwrap(), GetDecisionInput::default());
        }
    }

    #[test]
    fn the_reading_parameters_reach_the_application_input() {
        let call = decode(&json!({
            "decision_id": "CETATEXT000051655317", "detail": "FULL", "offset": "120",
            "max_chars": 2000.0, "focus": "bail mobilité", "passages": null
        }));
        assert_eq!(
            call.input.unwrap(),
            GetDecisionInput {
                decision_id: Some("CETATEXT000051655317".to_owned()),
                reading: ReadingInput {
                    detail: Some("FULL".to_owned()),
                    offset: Some(120),
                    max_chars: Some(2000),
                    focus: Some("bail mobilité".to_owned()),
                    passages: None,
                },
            }
        );
    }

    #[test]
    fn a_wrong_type_names_the_field() {
        assert_eq!(
            decode(&json!({"decision_id": 5})).input.unwrap_err().message,
            "decision_id: invalid type: integer `5`, expected a string"
        );
    }

    #[test]
    fn a_wrong_reading_type_is_a_decoding_error() {
        // Les champs aplatis passent par le contenu tamponné de serde, qui perd le chemin : le
        // message ne nomme pas le champ, mais dit ce qui est attendu.
        let offset = decode(&json!({"decision_id": "JURITEXT000048465611", "offset": [1]}));
        assert_eq!(
            offset.input.unwrap_err().message,
            "invalid type: sequence, expected an integer"
        );
        let detail = decode(&json!({"decision_id": "JURITEXT000048465611", "detail": 3}));
        assert_eq!(
            detail.input.unwrap_err().message,
            "invalid type: integer `3`, expected a string"
        );
    }

    #[test]
    fn an_unknown_field_is_a_decoding_error() {
        let call = decode(&json!({"decision_id": "JURITEXT000048465611", "foo": 1}));
        assert_eq!(call.input.unwrap_err().message, "unknown field `foo`");
    }

    #[test]
    fn the_schema_requires_the_id_and_closes_the_object() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["decision_id"]));
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema.get("$schema").is_none());
        let mut names: Vec<_> = schema["properties"].as_object().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["decision_id", "detail", "focus", "max_chars", "offset", "passages"]);
        assert_eq!(
            schema["properties"]["decision_id"],
            json!({
                "type": "string",
                "description": "A JURITEXT..., CONSTEXT... or CETATEXT... id (from fr_search on JURI, CONSTIT or CETAT)."
            })
        );
    }

    #[test]
    fn the_reading_properties_keep_their_bounds_and_enumeration() {
        let properties = schema()["properties"].clone();
        assert_eq!(properties["detail"]["enum"], json!(["summary", "full"]));
        assert_eq!(properties["max_chars"]["minimum"], 500);
        assert_eq!(properties["max_chars"]["maximum"], 40_000);
        assert_eq!(properties["max_chars"]["default"], 4_000);
        assert_eq!(properties["passages"]["maximum"], 10);
        assert_eq!(properties["focus"]["maxLength"], 500);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
