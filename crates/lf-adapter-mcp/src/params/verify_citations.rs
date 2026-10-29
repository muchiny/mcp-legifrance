//! Paramètres de `fr_verify_citations` (05 §5.9).
//!
//! Les deux champs sont des `Option` : la validation (longueur du texte, bornes de
//! `max_citations`) reste au cas d'usage, qui rédige les messages de parité. `text` est pourtant
//! annoncé `required` dans le schéma d'entrée, et son absence rend `text is required.`. Les
//! descriptions du schéma sont posées par `#[schemars(description)]` ; les doc comments restent
//! des notes de code.

use lf_application::input::VerifyCitationsInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les deux paramètres de `fr_verify_citations`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifyCitationsParams {
    /// Texte dont on contrôle les citations, 1 à 20 000 caractères.
    #[schemars(
        required,
        with = "String",
        description = "The text whose citations must be checked (up to 20000 characters).",
        length(min = 1, max = 20_000)
    )]
    pub(crate) text: Option<String>,
    /// Nombre maximal de citations contrôlées, dans l'ordre d'apparition.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Maximum citations checked, in order of appearance.",
        range(min = 1, max = 100),
        extend("default" = 50)
    )]
    pub(crate) max_citations: Option<i64>,
}

impl From<VerifyCitationsParams> for VerifyCitationsInput {
    fn from(params: VerifyCitationsParams) -> Self {
        Self { text: params.text, max_citations: params.max_citations }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode::to_call, schema::input_schema};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn decoded(arguments: &Value) -> Result<VerifyCitationsInput, String> {
        to_call::<VerifyCitationsParams, VerifyCitationsInput>(
            arguments.as_object().unwrap().clone(),
        )
        .input
        .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<VerifyCitationsParams>().as_ref().clone())
    }

    #[test]
    fn the_two_arguments_reach_the_application_input() {
        let input =
            decoded(&json!({"text": "article 9 du code civil", "max_citations": 20})).unwrap();
        assert_eq!(
            input,
            VerifyCitationsInput {
                text: Some("article 9 du code civil".to_owned()),
                max_citations: Some(20),
            }
        );
    }

    #[test]
    fn no_argument_and_null_mean_absent_and_max_citations_reads_a_string() {
        assert_eq!(decoded(&json!({})).unwrap(), VerifyCitationsInput::default());
        let input = decoded(&json!({"text": null, "max_citations": null})).unwrap();
        assert_eq!(input, VerifyCitationsInput::default());
        assert_eq!(decoded(&json!({"max_citations": "7"})).unwrap().max_citations, Some(7));
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"text": "x", "foo": 1})).unwrap_err(),
            "foo: unknown field `foo`, expected `text` or `max_citations`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"text": 42})).unwrap_err(),
            "text: invalid type: integer `42`, expected a string"
        );
        assert_eq!(
            decoded(&json!({"max_citations": [1]})).unwrap_err().split(':').next(),
            Some("max_citations")
        );
    }

    #[test]
    fn the_input_schema_requires_only_the_text() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["text"]));
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn the_schema_states_the_bounds_and_the_default() {
        let schema = schema();
        let properties = &schema["properties"];
        assert_eq!(properties["text"]["minLength"], 1);
        assert_eq!(properties["text"]["maxLength"], 20_000);
        assert_eq!(properties["max_citations"]["minimum"], 1);
        assert_eq!(properties["max_citations"]["maximum"], 100);
        assert_eq!(properties["max_citations"]["default"], 50);
    }

    #[test]
    fn the_bounds_are_the_application_bounds() {
        assert_eq!(lf_application::budget::ResponseBudget::MAX_CITATIONS_MAX, 100);
        assert_eq!(lf_application::budget::ResponseBudget::MAX_CITATIONS_DEFAULT, 50);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
