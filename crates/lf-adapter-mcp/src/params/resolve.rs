//! Paramètres de `fr_resolve` (05 §5.10).
//!
//! Les deux champs sont des `Option` : la validation (longueur de la citation, date) reste au cas
//! d'usage, qui rédige les messages de parité. `citation` est pourtant annoncée `required` dans le
//! schéma d'entrée, et son absence rend `citation is required.`. Les descriptions du schéma sont
//! posées par `#[schemars(description)]` ; les doc comments restent des notes de code.

use lf_application::input::ResolveInput;
use schemars::JsonSchema;
use serde::Deserialize;

/// Les deux paramètres de `fr_resolve`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolveParams {
    /// Une citation en texte libre, 1 à 1 000 caractères.
    #[schemars(
        required,
        with = "String",
        description = "One citation in free text, e.g. \"art. L. 1234-5 du code du travail\" or an ECLI.",
        length(min = 1, max = 1_000)
    )]
    pub(crate) citation: Option<String>,
    /// Date de la version des articles, `AAAA-MM-JJ`.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Date YYYY-MM-DD for the article version (default: today in Paris).",
        extend("format" = "date")
    )]
    pub(crate) date: Option<String>,
}

impl From<ResolveParams> for ResolveInput {
    fn from(params: ResolveParams) -> Self {
        Self { citation: params.citation, date: params.date }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode::to_call, schema::input_schema};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn decoded(arguments: &Value) -> Result<ResolveInput, String> {
        to_call::<ResolveParams, ResolveInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<ResolveParams>().as_ref().clone())
    }

    #[test]
    fn the_two_arguments_reach_the_application_input() {
        let input = decoded(&json!({"citation": "art. 9 C. civ.", "date": "2016-09-30"})).unwrap();
        assert_eq!(
            input,
            ResolveInput {
                citation: Some("art. 9 C. civ.".to_owned()),
                date: Some("2016-09-30".to_owned()),
            }
        );
    }

    #[test]
    fn no_argument_and_null_mean_absent() {
        assert_eq!(decoded(&json!({})).unwrap(), ResolveInput::default());
        let input = decoded(&json!({"citation": null, "date": null})).unwrap();
        assert_eq!(input, ResolveInput::default());
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"citation": "x", "foo": 1})).unwrap_err(),
            "foo: unknown field `foo`, expected `citation` or `date`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"citation": 42})).unwrap_err(),
            "citation: invalid type: integer `42`, expected a string"
        );
        assert_eq!(
            decoded(&json!({"citation": "x", "date": 20_261_029})).unwrap_err().split(':').next(),
            Some("date")
        );
    }

    #[test]
    fn the_input_schema_requires_only_the_citation() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["citation"]));
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn the_schema_states_the_bounds_and_the_date_format() {
        let schema = schema();
        let properties = &schema["properties"];
        assert_eq!(properties["citation"]["minLength"], 1);
        assert_eq!(properties["citation"]["maxLength"], 1_000);
        assert_eq!(properties["date"]["format"], "date");
        assert_eq!(properties["date"]["type"], "string");
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
