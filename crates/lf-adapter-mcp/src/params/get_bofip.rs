//! Paramètres de `fr_get_bofip` (05 §5.15) : `id`, `publication_id`, plus les cinq paramètres de
//! lecture communs aux lectures longues ([`ReadingParams`], aplatis).
//!
//! `id` est `Option` pour que son absence (ou `null`) atteigne le cas d'usage, qui rend
//! `id is required.` ; le schéma, lui, le déclare requis (`#[schemars(required)]`). Les
//! descriptions du schéma sont en anglais (Q14), posées par `#[schemars(description)]` et non par
//! un commentaire de doc.

use lf_application::input::GetBofipInput;
use schemars::JsonSchema;
use serde::Deserialize;

use super::reading::ReadingParams;
use crate::lenient;

/// Arguments de l'outil, tels que le client les envoie.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetBofipParams {
    // `BOI-…` ou `ACTU-…`, mis en majuscules et vérifié par le cas d'usage.
    #[schemars(required, description = "A BOFiP id: BOI-... or ACTU-... (from fr_search_bofip).")]
    pub(crate) id: Option<String>,
    // Numéro de la publication : ne sert que si l'identifiant en désigne deux.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Selects one publication when the id designates two (value from fr_search_bofip or from `publications`).",
        range(min = 1, max = 9_999_999)
    )]
    pub(crate) publication_id: Option<i64>,
    #[serde(flatten)]
    pub(crate) reading: ReadingParams,
}

impl From<GetBofipParams> for GetBofipInput {
    fn from(params: GetBofipParams) -> Self {
        Self {
            id: params.id,
            publication_id: params.publication_id,
            reading: params.reading.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::input::{ReadingInput, ToolCall};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, schema::input_schema};

    const ID: &str = "BOI-ENR-DMTG-10-50-20";

    fn decode(arguments: &Value) -> ToolCall<GetBofipInput> {
        to_call::<GetBofipParams, GetBofipInput>(arguments.as_object().unwrap().clone())
    }

    fn schema() -> Value {
        Value::Object(input_schema::<GetBofipParams>().as_ref().clone())
    }

    #[test]
    fn the_id_is_decoded_as_given() {
        let call = decode(&json!({"id": " boi-enr-dmtg-10-50-20 "}));
        assert_eq!(
            call.input.unwrap(),
            GetBofipInput {
                id: Some(" boi-enr-dmtg-10-50-20 ".to_owned()),
                publication_id: None,
                reading: ReadingInput::default(),
            }
        );
        assert_eq!(call.canonical.as_str(), r#"{"id":" boi-enr-dmtg-10-50-20 "}"#);
    }

    #[test]
    fn an_absent_or_null_id_reaches_the_use_case_as_absent() {
        for arguments in [json!({}), json!({"id": null, "publication_id": null})] {
            assert_eq!(decode(&arguments).input.unwrap(), GetBofipInput::default());
        }
    }

    #[test]
    fn the_publication_number_and_the_reading_parameters_are_decoded_leniently() {
        let call = decode(&json!({
            "id": ID, "publication_id": "3369", "detail": "FULL", "offset": "120",
            "max_chars": 2000.0, "focus": "réduction de droits", "passages": null
        }));
        assert_eq!(
            call.input.unwrap(),
            GetBofipInput {
                id: Some(ID.to_owned()),
                publication_id: Some(3369),
                reading: ReadingInput {
                    detail: Some("FULL".to_owned()),
                    offset: Some(120),
                    max_chars: Some(2000),
                    focus: Some("réduction de droits".to_owned()),
                    passages: None,
                },
            }
        );
    }

    #[test]
    fn a_wrong_type_is_a_decoding_error() {
        // Un champ propre de la struct garde son chemin, même si elle en aplatit une autre.
        let id = decode(&json!({"id": 5}));
        assert_eq!(
            id.input.unwrap_err().message,
            "id: invalid type: integer `5`, expected a string"
        );
        let number = decode(&json!({"id": ID, "publication_id": [1]}));
        assert_eq!(
            number.input.unwrap_err().message,
            "publication_id: invalid type: sequence, expected an integer"
        );
        let detail = decode(&json!({"id": ID, "detail": 3}));
        assert_eq!(
            detail.input.unwrap_err().message,
            "invalid type: integer `3`, expected a string"
        );
    }

    #[test]
    fn an_unknown_field_is_a_decoding_error() {
        let call = decode(&json!({"id": ID, "valid_from": "2023-05-24"}));
        assert_eq!(call.input.unwrap_err().message, "unknown field `valid_from`");
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
        assert_eq!(
            names,
            ["detail", "focus", "id", "max_chars", "offset", "passages", "publication_id"]
        );
    }

    #[test]
    fn the_schema_bounds_the_publication_number() {
        let schema = schema();
        let number = &schema["properties"]["publication_id"];
        assert_eq!(number["type"], "integer");
        assert_eq!(number["minimum"], 1);
        assert_eq!(number["maximum"], 9_999_999);
        assert!(number.get("default").is_none());
        assert_eq!(
            schema["properties"]["id"],
            json!({
                "type": "string",
                "description": "A BOFiP id: BOI-... or ACTU-... (from fr_search_bofip)."
            })
        );
    }

    #[test]
    fn the_bound_is_the_domain_bound() {
        assert_eq!(lf_domain::doctrine::PublicationId::MAX, 9_999_999);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
