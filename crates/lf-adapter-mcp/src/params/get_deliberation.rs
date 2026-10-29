//! Paramètres de `fr_get_deliberation` (05 §5.5) : `deliberation_id`, plus les cinq paramètres de
//! lecture communs aux trois lectures longues (05 §5.0.2).
//!
//! `deliberation_id` est un `Option` pour que son absence (ou `null`) atteigne le cas d'usage, qui
//! rend la phrase de parité `deliberation_id is required.` ; le schéma, lui, le déclare requis
//! (`#[schemars(required)]`). Les descriptions du schéma sont en anglais (Q14), posées par
//! `#[schemars(description)]` et non par un commentaire de doc.
//!
//! Les cinq champs de lecture sont déclarés ici et non aplatis depuis
//! [`ReadingParams`](super::reading::ReadingParams) : à travers un `#[serde(flatten)]`, serde
//! tamponne les valeurs et `serde_path_to_error` perd le chemin du champ (`invalid type: sequence,
//! expected an integer` au lieu de `offset: invalid type: …`), et `deny_unknown_fields` ne peut plus
//! citer les champs attendus. Le test `reading_properties_equal_the_shared_reading_schema` garde
//! ces déclarations identiques, au schéma près, à celles de `ReadingParams`.

use lf_application::input::{GetDeliberationInput, ReadingInput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Arguments de l'outil, tels que le client les envoie.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetDeliberationParams {
    // CNILTEXT + 12 chiffres, vérifié par le cas d'usage.
    #[schemars(required, description = "A CNILTEXT... id (from fr_search on fond CNIL).")]
    pub(crate) deliberation_id: Option<String>,
    // Niveau de détail ; insensible à la casse, vérifié par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = r#""summary" (default): metadata and a short summary. "full": the text from offset, up to max_chars."#,
        extend("enum" = ["summary", "full"])
    )]
    pub(crate) detail: Option<String>,
    // Position de départ en caractères (mode `full`).
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = r#"Start position in characters for detail="full"; pass the next_offset of the previous call."#,
        extend("default" = 0)
    )]
    pub(crate) offset: Option<i64>,
    // Plafond de caractères du texte rendu.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Maximum characters of text returned (500 to 40000).",
        range(min = 500, max = 40000),
        extend("default" = 4000)
    )]
    pub(crate) max_chars: Option<i64>,
    // Mots à retrouver (mode passages).
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "A few words: return the best matching passages instead of the summary or window.",
        length(min = 1, max = 500)
    )]
    pub(crate) focus: Option<String>,
    // Nombre de passages rendus avec `focus`.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Number of passages returned with focus.",
        range(min = 1, max = 10),
        extend("default" = 3)
    )]
    pub(crate) passages: Option<i64>,
}

impl From<GetDeliberationParams> for GetDeliberationInput {
    fn from(params: GetDeliberationParams) -> Self {
        Self {
            deliberation_id: params.deliberation_id,
            reading: ReadingInput {
                detail: params.detail,
                offset: params.offset,
                max_chars: params.max_chars,
                focus: params.focus,
                passages: params.passages,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::input::ToolCall;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, params::reading::ReadingParams, schema::input_schema};

    fn decode(arguments: &Value) -> ToolCall<GetDeliberationInput> {
        to_call::<GetDeliberationParams, GetDeliberationInput>(
            arguments.as_object().unwrap().clone(),
        )
    }

    #[test]
    fn the_id_is_decoded_as_given() {
        let call = decode(&json!({"deliberation_id": " cniltext000053352643 "}));
        assert_eq!(
            call.input.unwrap(),
            GetDeliberationInput {
                deliberation_id: Some(" cniltext000053352643 ".to_owned()),
                reading: ReadingInput::default(),
            }
        );
        assert_eq!(call.canonical.as_str(), r#"{"deliberation_id":" cniltext000053352643 "}"#);
    }

    #[test]
    fn the_five_reading_parameters_reach_the_application_input() {
        let call = decode(&json!({
            "deliberation_id": "CNILTEXT000053352643", "detail": "full", "offset": 120,
            "max_chars": "2000", "focus": "sanction", "passages": 4.0
        }));
        assert_eq!(
            call.input.unwrap(),
            GetDeliberationInput {
                deliberation_id: Some("CNILTEXT000053352643".to_owned()),
                reading: ReadingInput {
                    detail: Some("full".to_owned()),
                    offset: Some(120),
                    max_chars: Some(2000),
                    focus: Some("sanction".to_owned()),
                    passages: Some(4),
                },
            }
        );
    }

    #[test]
    fn an_absent_or_null_argument_reaches_the_use_case_as_absent() {
        for arguments in [
            json!({}),
            json!({"deliberation_id": null}),
            json!({
                "deliberation_id": null, "detail": null, "offset": null, "max_chars": null,
                "focus": null, "passages": null
            }),
        ] {
            assert_eq!(decode(&arguments).input.unwrap(), GetDeliberationInput::default());
        }
    }

    #[test]
    fn values_are_passed_through_unvalidated() {
        // Les bornes et l'énumération sont vérifiées par le cas d'usage, pas ici ; un entier hors
        // d'un i64 sature au lieu de faire échouer le décodage.
        let call = decode(&json!({
            "deliberation_id": "CNILTEXT000053352643", "detail": "SUMMARY", "max_chars": 100,
            "passages": 0, "offset": 1e30
        }));
        let reading = call.input.unwrap().reading;
        assert_eq!(reading.detail.as_deref(), Some("SUMMARY"));
        assert_eq!((reading.max_chars, reading.passages), (Some(100), Some(0)));
        assert_eq!(reading.offset, Some(i64::MAX));
    }

    #[test]
    fn a_wrong_type_names_the_field() {
        for (arguments, expected) in [
            (
                json!({"deliberation_id": 5}),
                "deliberation_id: invalid type: integer `5`, expected a string",
            ),
            (
                json!({"deliberation_id": "CNILTEXT000053352643", "offset": [1]}),
                "offset: invalid type: sequence, expected an integer",
            ),
            (
                json!({"deliberation_id": "CNILTEXT000053352643", "max_chars": "beaucoup"}),
                "max_chars: invalid value: string \"beaucoup\", expected an integer",
            ),
            (
                json!({"deliberation_id": "CNILTEXT000053352643", "detail": 3}),
                "detail: invalid type: integer `3`, expected a string",
            ),
            (
                json!({"deliberation_id": "CNILTEXT000053352643", "focus": true}),
                "focus: invalid type: boolean `true`, expected a string",
            ),
            (
                json!({"deliberation_id": "CNILTEXT000053352643", "passages": 2.5}),
                "passages: invalid value: floating point `2.5`, expected an integer",
            ),
        ] {
            assert_eq!(decode(&arguments).input.unwrap_err().message, expected);
        }
    }

    #[test]
    fn an_unknown_field_is_refused_with_the_expected_ones() {
        let call = decode(&json!({"deliberation_id": "CNILTEXT000053352643", "foo": 1}));
        assert_eq!(
            call.input.unwrap_err().message,
            "foo: unknown field `foo`, expected one of `deliberation_id`, `detail`, `offset`, \
             `max_chars`, `focus`, `passages`"
        );
        // Le paramètre d'une autre lecture longue n'est pas celui-ci.
        let article = decode(&json!({"article_id": "LEGIARTI000006419280"}));
        assert!(article.input.unwrap_err().message.starts_with("article_id: unknown field"));
    }

    #[test]
    fn the_schema_requires_the_id_and_closes_the_object() {
        let schema = Value::Object(input_schema::<GetDeliberationParams>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["deliberation_id"]));
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema.get("$schema").is_none());
        let mut names: Vec<_> = schema["properties"].as_object().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(
            names,
            ["deliberation_id", "detail", "focus", "max_chars", "offset", "passages"]
        );
        assert_eq!(
            schema["properties"]["deliberation_id"],
            json!({
                "type": "string",
                "description": "A CNILTEXT... id (from fr_search on fond CNIL)."
            })
        );
    }

    #[test]
    fn reading_properties_equal_the_shared_reading_schema() {
        let own = input_schema::<GetDeliberationParams>();
        let shared = input_schema::<ReadingParams>();
        for name in ["detail", "offset", "max_chars", "focus", "passages"] {
            assert_eq!(own["properties"][name], shared["properties"][name], "{name}");
        }
    }

    #[test]
    fn the_reading_parameters_keep_their_bounds_and_defaults_in_the_schema() {
        let schema = Value::Object(input_schema::<GetDeliberationParams>().as_ref().clone());
        let property = |name: &str| schema["properties"][name].clone();
        assert_eq!(property("detail")["enum"], json!(["summary", "full"]));
        assert_eq!(property("offset")["default"], json!(0));
        let max_chars = property("max_chars");
        assert_eq!((&max_chars["minimum"], &max_chars["maximum"]), (&json!(500), &json!(40000)));
        assert_eq!(max_chars["default"], json!(4000));
        let passages = property("passages");
        assert_eq!((&passages["minimum"], &passages["maximum"]), (&json!(1), &json!(10)));
        assert_eq!(passages["default"], json!(3));
        assert_eq!(property("focus")["maxLength"], json!(500));
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(input_schema::<GetDeliberationParams>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema);
        });
    }
}
