//! Paramètres de `fr_get_judilibre` (05 §5.19) : `id`, `part`, plus les cinq paramètres de lecture
//! communs aux lectures longues ([`ReadingParams`], aplatis).
//!
//! `id` est `Option` pour que son absence (ou `null`) atteigne le cas d'usage, qui rend
//! `id is required.` ; le schéma, lui, le déclare requis (`#[schemars(required)]`). Les
//! descriptions du schéma sont en anglais (Q14), posées par `#[schemars(description)]` et non par
//! un commentaire de doc.

use lf_application::input::GetJudilibreInput;
use schemars::JsonSchema;
use serde::Deserialize;

use super::reading::ReadingParams;

/// Arguments de l'outil, tels que le client les envoie.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetJudilibreParams {
    // 24 caractères hexadécimaux, mis en minuscules et vérifiés par le cas d'usage.
    #[schemars(
        required,
        description = "A Judilibre decision id: 24 hexadecimal characters (from fr_search_judilibre)."
    )]
    pub(crate) id: Option<String>,
    // Partie à lire ; insensible à la casse, vérifiée par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Read only this part of the decision. Default: the whole decision.",
        extend(
            "enum" = ["introduction", "expose", "moyens", "motivations", "dispositif", "annexes"]
        )
    )]
    pub(crate) part: Option<String>,
    #[serde(flatten)]
    pub(crate) reading: ReadingParams,
}

impl From<GetJudilibreParams> for GetJudilibreInput {
    fn from(params: GetJudilibreParams) -> Self {
        Self { id: params.id, part: params.part, reading: params.reading.into() }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::input::{ReadingInput, ToolCall};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, schema::input_schema};

    fn decode(arguments: &Value) -> ToolCall<GetJudilibreInput> {
        to_call::<GetJudilibreParams, GetJudilibreInput>(arguments.as_object().unwrap().clone())
    }

    fn schema() -> Value {
        Value::Object(input_schema::<GetJudilibreParams>().as_ref().clone())
    }

    #[test]
    fn the_id_and_the_part_are_decoded_as_given() {
        let call = decode(&json!({"id": " 5FCA624B7EEB384A40CAFB85 ", "part": "Motivations"}));
        assert_eq!(
            call.input.unwrap(),
            GetJudilibreInput {
                id: Some(" 5FCA624B7EEB384A40CAFB85 ".to_owned()),
                part: Some("Motivations".to_owned()),
                reading: ReadingInput::default(),
            }
        );
        assert_eq!(
            call.canonical.as_str(),
            r#"{"id":" 5FCA624B7EEB384A40CAFB85 ","part":"Motivations"}"#
        );
    }

    #[test]
    fn the_reading_parameters_are_flattened() {
        let call =
            decode(&json!({"id": "x", "offset": 2000, "max_chars": "500", "detail": "full"}));
        let reading = call.input.unwrap().reading;
        assert_eq!((reading.offset, reading.max_chars), (Some(2000), Some(500)));
        assert_eq!(reading.detail.as_deref(), Some("full"));
        assert_eq!(decode(&json!({})).input.unwrap(), GetJudilibreInput::default());
    }

    #[test]
    fn an_unknown_argument_is_refused() {
        // Avec `flatten`, serde ne dit plus la liste des noms attendus ni le chemin.
        let error = decode(&json!({"id": "x", "judge": "y"})).input.unwrap_err();
        assert_eq!(error.message, "unknown field `judge`");
    }

    #[test]
    fn the_schema_requires_the_id_and_lists_the_parts() {
        let schema = schema();
        assert_eq!(schema["required"], json!(["id"]));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["part"]["enum"],
            json!(lf_domain::judilibre::DecisionPart::NAMES)
        );
        for reading in ["detail", "offset", "max_chars", "focus"] {
            assert!(schema["properties"].get(reading).is_some(), "{reading}");
        }
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
