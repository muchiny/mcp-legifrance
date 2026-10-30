//! Paramètres de `fr_search_all` (05 §5.13).
//!
//! Tous les champs sont des `Option` : la validation (fonds connus et uniques, bornes de
//! `per_fond`) reste au cas d'usage, qui rédige les messages de parité. `query` est pourtant
//! annoncée `required` dans le schéma d'entrée, et son absence rend `query is required.`. Les
//! descriptions du schéma sont posées par `#[schemars(description)]` ; les doc comments restent
//! des notes de code.

use lf_application::input::SearchAllInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les trois paramètres de `fr_search_all`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchAllParams {
    /// Texte de la requête, 1 à 450 caractères après normalisation (comme `fr_search`).
    #[schemars(
        required,
        with = "String",
        description = "Free-text query.",
        length(min = 1, max = 450)
    )]
    pub(crate) query: Option<String>,
    /// Fonds interrogés, insensibles à la casse ; `LODA`, `CODE`, `JURI`, `CETAT` et `CONSTIT`
    /// par défaut.
    #[serde(default)]
    #[schemars(
        with = "Vec<String>",
        description = "Fonds to search (default LODA, CODE, JURI, CETAT, CONSTIT).",
        length(min = 1, max = 8),
        extend(
            "items" = {
                "type": "string",
                "enum": ["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"]
            },
            "uniqueItems" = true,
            "default" = ["LODA", "CODE", "JURI", "CETAT", "CONSTIT"]
        )
    )]
    pub(crate) fonds: Option<Vec<String>>,
    /// Résultats par fond.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Hits per fond (1..10).",
        range(min = 1, max = 10),
        extend("default" = 3)
    )]
    pub(crate) per_fond: Option<i64>,
}

impl From<SearchAllParams> for SearchAllInput {
    fn from(params: SearchAllParams) -> Self {
        Self { query: params.query, fonds: params.fonds, per_fond: params.per_fond }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode::to_call, schema::input_schema};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn decoded(arguments: &Value) -> Result<SearchAllInput, String> {
        to_call::<SearchAllParams, SearchAllInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<SearchAllParams>().as_ref().clone())
    }

    #[test]
    fn the_three_arguments_reach_the_application_input() {
        let input =
            decoded(&json!({"query": "bail", "fonds": ["loda", "JURI"], "per_fond": 5})).unwrap();
        assert_eq!(
            input,
            SearchAllInput {
                query: Some("bail".to_owned()),
                fonds: Some(vec!["loda".to_owned(), "JURI".to_owned()]),
                per_fond: Some(5),
            }
        );
    }

    #[test]
    fn no_argument_and_null_mean_absent_and_per_fond_reads_a_string() {
        assert_eq!(decoded(&json!({})).unwrap(), SearchAllInput::default());
        let input = decoded(&json!({"query": null, "fonds": null, "per_fond": null})).unwrap();
        assert_eq!(input, SearchAllInput::default());
        assert_eq!(decoded(&json!({"per_fond": "7"})).unwrap().per_fond, Some(7));
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"query": "bail", "foo": 1})).unwrap_err(),
            "foo: unknown field `foo`, expected one of `query`, `fonds`, `per_fond`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"query": 42})).unwrap_err(),
            "query: invalid type: integer `42`, expected a string"
        );
        assert_eq!(
            decoded(&json!({"fonds": "LODA"})).unwrap_err(),
            "fonds: invalid type: string \"LODA\", expected a sequence"
        );
        assert_eq!(
            decoded(&json!({"fonds": ["LODA", 3]})).unwrap_err(),
            "fonds[1]: invalid type: integer `3`, expected a string"
        );
    }

    #[test]
    fn the_input_schema_requires_only_the_query() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["query"]));
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn the_schema_states_the_fonds_enum_the_defaults_and_the_bounds() {
        let schema = schema();
        let properties = &schema["properties"];
        assert_eq!(properties["query"]["maxLength"], 450);
        let fonds = &properties["fonds"];
        assert_eq!(fonds["type"], "array");
        assert_eq!(fonds["minItems"], 1);
        assert_eq!(fonds["maxItems"], 8);
        assert_eq!(fonds["uniqueItems"], true);
        assert_eq!(
            fonds["items"]["enum"],
            json!(["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"])
        );
        assert_eq!(fonds["default"], json!(["LODA", "CODE", "JURI", "CETAT", "CONSTIT"]));
        assert_eq!(properties["per_fond"]["minimum"], 1);
        assert_eq!(properties["per_fond"]["maximum"], 10);
        assert_eq!(properties["per_fond"]["default"], 3);
    }

    #[test]
    fn the_query_bound_is_the_domain_bound() {
        assert_eq!(lf_domain::search::QUERY_MAX_CHARS, 450);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
