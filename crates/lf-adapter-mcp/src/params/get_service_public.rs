//! Paramètres de `fr_get_service_public` (05 §5.17) : `id`, `audience`, plus les cinq paramètres
//! de lecture communs aux lectures longues ([`ReadingParams`], aplatis).
//!
//! `id` est `Option` pour que son absence (ou `null`) atteigne le cas d'usage, qui rend
//! `id is required.` ; le schéma, lui, le déclare requis (`#[schemars(required)]`). Les
//! descriptions du schéma sont en anglais (Q14), posées par `#[schemars(description)]` et non par
//! un commentaire de doc.

use lf_application::input::GetServicePublicInput;
use schemars::JsonSchema;
use serde::Deserialize;

use super::reading::ReadingParams;

/// Arguments de l'outil, tels que le client les envoie.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetServicePublicParams {
    // `F…` ou `N…`, mis en majuscules et vérifié par le cas d'usage.
    #[schemars(
        required,
        description = "A sheet (F...) or folder (N...) id from fr_search_service_public."
    )]
    pub(crate) id: Option<String>,
    // Public de l'archive lue ; insensible à la casse, vérifié par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Audience of the sheet, as given by fr_search_service_public.",
        extend("enum" = ["particuliers", "entreprendre"], "default" = "particuliers")
    )]
    pub(crate) audience: Option<String>,
    #[serde(flatten)]
    pub(crate) reading: ReadingParams,
}

impl From<GetServicePublicParams> for GetServicePublicInput {
    fn from(params: GetServicePublicParams) -> Self {
        Self { id: params.id, audience: params.audience, reading: params.reading.into() }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::input::{ReadingInput, ToolCall};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, schema::input_schema};

    fn decode(arguments: &Value) -> ToolCall<GetServicePublicInput> {
        to_call::<GetServicePublicParams, GetServicePublicInput>(
            arguments.as_object().unwrap().clone(),
        )
    }

    fn schema() -> Value {
        Value::Object(input_schema::<GetServicePublicParams>().as_ref().clone())
    }

    #[test]
    fn the_id_and_the_audience_are_decoded_as_given() {
        let call = decode(&json!({"id": " f1437 ", "audience": "Entreprendre"}));
        assert_eq!(
            call.input.unwrap(),
            GetServicePublicInput {
                id: Some(" f1437 ".to_owned()),
                audience: Some("Entreprendre".to_owned()),
                reading: ReadingInput::default(),
            }
        );
        assert_eq!(call.canonical.as_str(), r#"{"audience":"Entreprendre","id":" f1437 "}"#);
    }

    #[test]
    fn an_absent_or_null_id_reaches_the_use_case_as_absent() {
        for arguments in [json!({}), json!({"id": null, "audience": null})] {
            assert_eq!(decode(&arguments).input.unwrap(), GetServicePublicInput::default());
        }
    }

    #[test]
    fn the_reading_parameters_are_decoded_leniently() {
        let call = decode(&json!({
            "id": "F1437", "detail": "FULL", "offset": "120", "max_chars": 2000.0,
            "focus": "solde insaisissable", "passages": null
        }));
        assert_eq!(
            call.input.unwrap(),
            GetServicePublicInput {
                id: Some("F1437".to_owned()),
                audience: None,
                reading: ReadingInput {
                    detail: Some("FULL".to_owned()),
                    offset: Some(120),
                    max_chars: Some(2000),
                    focus: Some("solde insaisissable".to_owned()),
                    passages: None,
                },
            }
        );
    }

    #[test]
    fn a_wrong_type_is_a_decoding_error() {
        let id = decode(&json!({"id": 5}));
        assert_eq!(
            id.input.unwrap_err().message,
            "id: invalid type: integer `5`, expected a string"
        );
        let audience = decode(&json!({"id": "F1437", "audience": true}));
        assert_eq!(
            audience.input.unwrap_err().message,
            "audience: invalid type: boolean `true`, expected a string"
        );
    }

    #[test]
    fn an_unknown_field_is_a_decoding_error() {
        let call = decode(&json!({"id": "F1437", "publication_id": 3}));
        assert_eq!(call.input.unwrap_err().message, "unknown field `publication_id`");
    }

    #[test]
    fn the_schema_requires_the_id_and_closes_the_object() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["id"]));
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema.get("$schema").is_none());
        let mut names: Vec<_> = schema["properties"].as_object().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["audience", "detail", "focus", "id", "max_chars", "offset", "passages"]);
        let audience = &schema["properties"]["audience"];
        assert_eq!(audience["enum"], json!(["particuliers", "entreprendre"]));
        assert_eq!(audience["default"], "particuliers");
    }

    /// Une lecture vise un public ; `all` n'a de sens que pour la recherche.
    #[test]
    fn the_audiences_are_the_domain_ones() {
        let mut names = lf_domain::guidance::Audience::NAMES.to_vec();
        names.sort_unstable();
        assert_eq!(names, ["entreprendre", "particuliers"]);
        assert_eq!(lf_domain::guidance::SHEET_ID_MAX_DIGITS, 7);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
