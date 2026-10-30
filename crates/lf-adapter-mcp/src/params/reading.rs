//! Paramètres de lecture communs aux trois lectures longues (05 §5.0.2) : `fr_get_text`,
//! `fr_get_decision` et `fr_get_deliberation`.
//!
//! Les descriptions anglaises du schéma (`inputSchema`) sont posées par `#[schemars(description)]`
//! (05 §5.0.2) : les doc comments des champs restent des notes de code en français. Tous les champs
//! sont des `Option` : la validation (bornes, ordre, messages de
//! parité) reste au cas d'usage (`lf_application::budget::ReadingRequest::parse`), qui produit les
//! phrases de 05 §7.3. Les entiers passent par [`lenient::opt_int`](crate::lenient::opt_int).

use lf_application::input::ReadingInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les cinq paramètres de lecture, à aplatir (`#[serde(flatten)]`) dans les paramètres d'un outil
/// de lecture longue.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadingParams {
    /// Niveau de détail ; insensible à la casse, vérifié par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = r#""summary" (default): metadata and a short summary. "full": the text from offset, up to max_chars."#,
        extend("enum" = ["summary", "full"])
    )]
    pub(crate) detail: Option<String>,
    /// Position de départ en caractères (mode `full`).
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = r#"Start position in characters for detail="full"; pass the next_offset of the previous call."#,
        extend("default" = 0)
    )]
    pub(crate) offset: Option<i64>,
    /// Plafond de caractères du texte rendu.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Maximum characters of text returned (500 to 40000).",
        range(min = 500, max = 40000),
        extend("default" = 4000)
    )]
    pub(crate) max_chars: Option<i64>,
    /// Mots à retrouver (mode passages).
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "A few words: return the best matching passages instead of the summary or window.",
        length(min = 1, max = 500)
    )]
    pub(crate) focus: Option<String>,
    /// Nombre de passages rendus avec `focus`.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Number of passages returned with focus.",
        range(min = 1, max = 10),
        extend("default" = 3)
    )]
    pub(crate) passages: Option<i64>,
}

impl From<ReadingParams> for ReadingInput {
    fn from(params: ReadingParams) -> Self {
        Self {
            detail: params.detail,
            offset: params.offset,
            max_chars: params.max_chars,
            focus: params.focus,
            passages: params.passages,
        }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::budget::ResponseBudget;
    use lf_domain::text::FOCUS_MAX_CHARS;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::input_schema;

    fn decode(value: &Value) -> Result<ReadingParams, String> {
        serde_path_to_error::deserialize::<_, ReadingParams>(value.clone())
            .map_err(|error| error.to_string())
    }

    /// `properties.<name>` du schéma d'entrée.
    fn property(schema: &serde_json::Map<String, Value>, name: &str) -> Value {
        schema["properties"][name].clone()
    }

    #[test]
    fn no_argument_means_every_field_absent() {
        let params = decode(&json!({})).unwrap();
        assert_eq!(ReadingInput::from(params), ReadingInput::default());
    }

    #[test]
    fn the_five_fields_reach_the_application_input() {
        let params = decode(&json!({
            "detail": "full", "offset": 120, "max_chars": 2000, "focus": "bail mobilité", "passages": 4
        }))
        .unwrap();
        assert_eq!(
            ReadingInput::from(params),
            ReadingInput {
                detail: Some("full".to_owned()),
                offset: Some(120),
                max_chars: Some(2000),
                focus: Some("bail mobilité".to_owned()),
                passages: Some(4),
            }
        );
    }

    #[test]
    fn null_is_read_as_absent() {
        let params = decode(&json!({
            "detail": null, "offset": null, "max_chars": null, "focus": null, "passages": null
        }))
        .unwrap();
        assert_eq!(ReadingInput::from(params), ReadingInput::default());
    }

    #[test]
    fn values_are_passed_through_unvalidated() {
        // Les bornes et l'énumération sont vérifiées par le cas d'usage, pas ici.
        let params =
            decode(&json!({"detail": "SUMMARY", "max_chars": 100, "passages": 0})).unwrap();
        let input = ReadingInput::from(params);
        assert_eq!(input.detail.as_deref(), Some("SUMMARY"));
        assert_eq!(input.max_chars, Some(100));
        assert_eq!(input.passages, Some(0));
    }

    #[test]
    fn a_wrong_type_names_the_field() {
        assert_eq!(decode(&json!({"offset": [1]})).unwrap_err().split(':').next(), Some("offset"));
        assert_eq!(decode(&json!({"detail": 3})).unwrap_err().split(':').next(), Some("detail"));
    }

    #[test]
    fn an_unknown_field_is_refused_with_the_expected_ones() {
        let message = decode(&json!({"detail": "full", "foo": 1})).unwrap_err();
        assert_eq!(
            message,
            "foo: unknown field `foo`, expected one of `detail`, `offset`, `max_chars`, `focus`, `passages`"
        );
    }

    #[test]
    fn the_schema_is_an_object_with_five_optional_closed_properties() {
        let schema = input_schema::<ReadingParams>();
        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert!(!schema.contains_key("$schema"));
        assert_eq!(schema.get("required"), None, "aucun paramètre n'est obligatoire");
        let mut names: Vec<_> = schema["properties"].as_object().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["detail", "focus", "max_chars", "offset", "passages"]);
    }

    #[test]
    fn the_property_types_are_plain_without_null() {
        let schema = input_schema::<ReadingParams>();
        for (name, kind) in [
            ("detail", "string"),
            ("offset", "integer"),
            ("max_chars", "integer"),
            ("focus", "string"),
            ("passages", "integer"),
        ] {
            assert_eq!(property(&schema, name)["type"], json!(kind), "{name}");
        }
    }

    #[test]
    fn detail_is_a_closed_enumeration() {
        let schema = input_schema::<ReadingParams>();
        assert_eq!(property(&schema, "detail")["enum"], json!(["summary", "full"]));
    }

    #[test]
    fn the_schema_bounds_are_the_budget_constants() {
        let schema = input_schema::<ReadingParams>();
        let max_chars = property(&schema, "max_chars");
        assert_eq!(max_chars["minimum"], json!(ResponseBudget::MAX_CHARS_MIN));
        assert_eq!(max_chars["maximum"], json!(ResponseBudget::MAX_CHARS_MAX));
        assert_eq!(max_chars["default"], json!(ResponseBudget::MAX_CHARS_DEFAULT));
        let passages = property(&schema, "passages");
        assert_eq!(passages["minimum"], json!(1));
        assert_eq!(passages["maximum"], json!(ResponseBudget::PASSAGES_MAX));
        assert_eq!(passages["default"], json!(ResponseBudget::PASSAGES_DEFAULT));
        let focus = property(&schema, "focus");
        assert_eq!(focus["minLength"], json!(1));
        assert_eq!(focus["maxLength"], json!(FOCUS_MAX_CHARS));
        assert_eq!(property(&schema, "offset")["default"], json!(0));
    }

    #[test]
    fn the_descriptions_are_the_english_texts_of_the_specification() {
        let schema = input_schema::<ReadingParams>();
        let described =
            |name: &str| property(&schema, name)["description"].as_str().unwrap().to_owned();
        assert_eq!(
            described("detail"),
            r#""summary" (default): metadata and a short summary. "full": the text from offset, up to max_chars."#
        );
        assert_eq!(
            described("offset"),
            r#"Start position in characters for detail="full"; pass the next_offset of the previous call."#
        );
        assert_eq!(described("max_chars"), "Maximum characters of text returned (500 to 40000).");
        assert_eq!(
            described("focus"),
            "A few words: return the best matching passages instead of the summary or window."
        );
        assert_eq!(described("passages"), "Number of passages returned with focus.");
    }

    /// Paramètres d'un outil de lecture longue : l'identifiant, plus les cinq champs aplatis.
    #[derive(Debug, Deserialize, JsonSchema)]
    struct TextParams {
        #[schemars(description = "Identifier of the text to read.")]
        text_id: Option<String>,
        #[serde(flatten)]
        reading: ReadingParams,
    }

    #[test]
    fn flattened_into_a_tool_the_fields_decode_and_the_schema_merges() {
        let params = serde_path_to_error::deserialize::<_, TextParams>(json!({
            "text_id": "LEGIARTI000006419280", "offset": 40, "passages": null, "focus": "loyer"
        }))
        .unwrap();
        assert_eq!(params.text_id.as_deref(), Some("LEGIARTI000006419280"));
        let reading = ReadingInput::from(params.reading);
        assert_eq!(reading.offset, Some(40));
        assert_eq!(reading.passages, None);
        assert_eq!(reading.focus.as_deref(), Some("loyer"));

        let schema = input_schema::<TextParams>();
        assert_eq!(schema["type"], json!("object"));
        let mut names: Vec<_> = schema["properties"].as_object().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["detail", "focus", "max_chars", "offset", "passages", "text_id"]);
        assert_eq!(property(&schema, "max_chars")["maximum"], json!(40_000));
    }
}
