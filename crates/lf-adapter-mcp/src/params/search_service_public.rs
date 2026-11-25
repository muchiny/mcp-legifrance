//! Paramètres de `fr_search_service_public` (05 §5.16).
//!
//! Tous les champs sont des `Option` : la validation (mots de la requête, public, bornes de la
//! pagination) reste au cas d'usage, qui rédige les messages. `query` est pourtant annoncée
//! `required` dans le schéma d'entrée, et son absence rend `query is required.`. Les descriptions
//! du schéma sont en anglais (Q14), posées par `#[schemars(description)]` ; les doc comments
//! restent des notes de code.

use lf_application::input::SearchServicePublicInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les quatre paramètres de `fr_search_service_public`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchServicePublicParams {
    /// Mots cherchés, 1 à 450 caractères (comme `fr_search`).
    #[schemars(
        required,
        with = "String",
        description = "A few plain words, matched against sheet and folder titles and summaries (accents and case aside; a word also matches the longer words it starts).",
        length(min = 1, max = 450)
    )]
    pub(crate) query: Option<String>,
    /// Public cherché ; insensible à la casse, vérifié par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = r#""particuliers" (default): individuals; "entreprendre": businesses and the self-employed; "all": both."#,
        extend("enum" = ["particuliers", "entreprendre", "all"], "default" = "particuliers")
    )]
    pub(crate) audience: Option<String>,
    /// Numéro de page, à partir de 1.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(with = "u32", description = "Page number, from 1.", range(min = 1), extend("default" = 1))]
    pub(crate) page: Option<i64>,
    /// Résultats par page.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Hits per page (1..50).",
        range(min = 1, max = 50),
        extend("default" = 10)
    )]
    pub(crate) page_size: Option<i64>,
}

impl From<SearchServicePublicParams> for SearchServicePublicInput {
    fn from(params: SearchServicePublicParams) -> Self {
        Self {
            query: params.query,
            audience: params.audience,
            page: params.page,
            page_size: params.page_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, schema::input_schema};

    fn decoded(arguments: &Value) -> Result<SearchServicePublicInput, String> {
        to_call::<SearchServicePublicParams, SearchServicePublicInput>(
            arguments.as_object().unwrap().clone(),
        )
        .input
        .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<SearchServicePublicParams>().as_ref().clone())
    }

    #[test]
    fn the_four_arguments_reach_the_application_input() {
        let input = decoded(&json!({
            "query": "solde bancaire", "audience": "ALL", "page": 2, "page_size": 25,
        }))
        .unwrap();
        assert_eq!(
            input,
            SearchServicePublicInput {
                query: Some("solde bancaire".to_owned()),
                audience: Some("ALL".to_owned()),
                page: Some(2),
                page_size: Some(25),
            }
        );
    }

    #[test]
    fn no_argument_and_null_mean_absent_and_integers_read_a_string() {
        assert_eq!(decoded(&json!({})).unwrap(), SearchServicePublicInput::default());
        let nulls = json!({"query": null, "audience": null, "page": null, "page_size": null});
        assert_eq!(decoded(&nulls).unwrap(), SearchServicePublicInput::default());
        let lenient = decoded(&json!({"page": "3", "page_size": 20.0})).unwrap();
        assert_eq!((lenient.page, lenient.page_size), (Some(3), Some(20)));
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"query": "loyer", "serie": "IR"})).unwrap_err(),
            "serie: unknown field `serie`, expected one of `query`, `audience`, `page`, `page_size`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"query": 42})).unwrap_err(),
            "query: invalid type: integer `42`, expected a string"
        );
        assert_eq!(
            decoded(&json!({"audience": ["all"]})).unwrap_err(),
            "audience: invalid type: sequence, expected a string"
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
    fn the_schema_states_the_audiences_the_defaults_and_the_bounds() {
        let schema = schema();
        let properties = &schema["properties"];
        assert_eq!(properties["query"]["minLength"], 1);
        assert_eq!(properties["query"]["maxLength"], 450);
        assert_eq!(properties["audience"]["enum"], json!(["particuliers", "entreprendre", "all"]));
        assert_eq!(properties["audience"]["default"], "particuliers");
        assert_eq!(properties["page"]["minimum"], 1);
        assert_eq!(properties["page"]["default"], 1);
        assert_eq!(properties["page_size"]["minimum"], 1);
        assert_eq!(properties["page_size"]["maximum"], 50);
        assert_eq!(properties["page_size"]["default"], 10);
    }

    /// Les noms annoncés sont ceux que le domaine accepte.
    #[test]
    fn the_audiences_and_bounds_are_the_domain_ones() {
        let mut names = lf_domain::guidance::AudienceFilter::NAMES.to_vec();
        names.sort_unstable();
        assert_eq!(names, ["all", "entreprendre", "particuliers"]);
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
