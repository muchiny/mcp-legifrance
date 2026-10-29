//! Paramètres de `fr_diff_article` (05 §5.12).
//!
//! Tous les champs sont des `Option` : la validation (identifiant, dates) reste au cas d'usage,
//! qui rédige les messages de parité. `article_id` et `date_a` sont pourtant annoncés `required`
//! dans le schéma d'entrée : un client qui valide le voit avant l'appel, et les messages
//! `article_id is required.` et `date_a is required.` restent ceux du cas d'usage. Les
//! descriptions du schéma sont posées par `#[schemars(description)]` ; les doc comments restent
//! des notes de code.

use lf_application::input::DiffArticleInput;
use schemars::JsonSchema;
use serde::Deserialize;

/// Les trois paramètres de `fr_diff_article`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiffArticleParams {
    /// Identifiant `LEGIARTI` d'une version quelconque de l'article.
    #[schemars(
        required,
        with = "String",
        description = "A LEGIARTI... id of any version of the article."
    )]
    pub(crate) article_id: Option<String>,
    /// Première date, `AAAA-MM-JJ`.
    #[schemars(
        required,
        with = "String",
        description = "First date YYYY-MM-DD.",
        extend("format" = "date")
    )]
    pub(crate) date_a: Option<String>,
    /// Seconde date, `AAAA-MM-JJ` ; aujourd'hui à Paris par défaut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Second date YYYY-MM-DD (default: today in Paris).",
        extend("format" = "date")
    )]
    pub(crate) date_b: Option<String>,
}

impl From<DiffArticleParams> for DiffArticleInput {
    fn from(params: DiffArticleParams) -> Self {
        Self { article_id: params.article_id, date_a: params.date_a, date_b: params.date_b }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode::to_call, schema::input_schema};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn decoded(arguments: &Value) -> Result<DiffArticleInput, String> {
        to_call::<DiffArticleParams, DiffArticleInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<DiffArticleParams>().as_ref().clone())
    }

    #[test]
    fn the_three_arguments_reach_the_application_input() {
        let input = decoded(&json!({
            "article_id": "LEGIARTI000006419288", "date_a": "2020-01-01", "date_b": "2026-01-01"
        }))
        .unwrap();
        assert_eq!(
            input,
            DiffArticleInput {
                article_id: Some("LEGIARTI000006419288".to_owned()),
                date_a: Some("2020-01-01".to_owned()),
                date_b: Some("2026-01-01".to_owned()),
            }
        );
    }

    #[test]
    fn no_argument_and_null_mean_absent() {
        // `article_id is required.` est le message de parité du cas d'usage, pas du décodeur.
        assert_eq!(decoded(&json!({})).unwrap(), DiffArticleInput::default());
        let input = decoded(&json!({"article_id": null, "date_a": null, "date_b": null})).unwrap();
        assert_eq!(input, DiffArticleInput::default());
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"article_id": "LEGIARTI000006419288", "foo": 1})).unwrap_err(),
            "foo: unknown field `foo`, expected one of `article_id`, `date_a`, `date_b`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"date_a": 2020})).unwrap_err(),
            "date_a: invalid type: integer `2020`, expected a string"
        );
    }

    #[test]
    fn the_input_schema_requires_the_article_and_the_first_date() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["article_id", "date_a"]));
        assert!(schema.get("$schema").is_none());
        let properties = &schema["properties"];
        assert_eq!(
            properties["article_id"]["description"],
            "A LEGIARTI... id of any version of the article."
        );
        for date in ["date_a", "date_b"] {
            assert_eq!(properties[date]["type"], "string", "{date}");
            assert_eq!(properties[date]["format"], "date", "{date}");
        }
        assert_eq!(
            properties["date_b"]["description"],
            "Second date YYYY-MM-DD (default: today in Paris)."
        );
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
