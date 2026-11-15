//! Paramètres de `fr_search_bofip` (05 §5.14).
//!
//! Tous les champs sont des `Option` : la validation (mots de la requête, code de série, nature,
//! bornes de la pagination) reste au cas d'usage, qui rédige les messages. `query` est pourtant
//! annoncée `required` dans le schéma d'entrée, et son absence rend `query is required.`. Les
//! descriptions du schéma sont en anglais (Q14), posées par `#[schemars(description)]` ; les doc
//! comments restent des notes de code.

use lf_application::input::SearchBofipInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les cinq paramètres de `fr_search_bofip`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchBofipParams {
    /// Mots cherchés, 1 à 450 caractères (comme `fr_search`).
    #[schemars(
        required,
        with = "String",
        description = "A few plain words: all must appear in the publication, the last one also as a prefix. No operators, no quoted phrases.",
        length(min = 1, max = 450)
    )]
    pub(crate) query: Option<String>,
    /// Code de série, insensible à la casse ; liste ouverte, vérifiée en forme par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Series code, e.g. ENR (registration and inheritance duties), IR (income tax), RFPI (property income and capital gains).",
        length(min = 1, max = 16)
    )]
    pub(crate) serie: Option<String>,
    /// Nature des publications ; insensible à la casse, vérifiée par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = r#""doctrine" (default): doctrine pages; "news": news items; "all": both."#,
        extend("enum" = ["doctrine", "news", "all"], "default" = "doctrine")
    )]
    pub(crate) kind: Option<String>,
    /// Numéro de page, à partir de 1.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(with = "u32", description = "Page number, from 1.", range(min = 1), extend("default" = 1))]
    pub(crate) page: Option<i64>,
    /// Résultats par page.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Hits per page (1..100).",
        range(min = 1, max = 100),
        extend("default" = 10)
    )]
    pub(crate) page_size: Option<i64>,
}

impl From<SearchBofipParams> for SearchBofipInput {
    fn from(params: SearchBofipParams) -> Self {
        Self {
            query: params.query,
            serie: params.serie,
            kind: params.kind,
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

    fn decoded(arguments: &Value) -> Result<SearchBofipInput, String> {
        to_call::<SearchBofipParams, SearchBofipInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<SearchBofipParams>().as_ref().clone())
    }

    #[test]
    fn the_five_arguments_reach_the_application_input() {
        let input = decoded(&json!({
            "query": "donation abattement", "serie": "enr", "kind": "ALL", "page": 2, "page_size": 25,
        }))
        .unwrap();
        assert_eq!(
            input,
            SearchBofipInput {
                query: Some("donation abattement".to_owned()),
                serie: Some("enr".to_owned()),
                kind: Some("ALL".to_owned()),
                page: Some(2),
                page_size: Some(25),
            }
        );
    }

    #[test]
    fn no_argument_and_null_mean_absent_and_integers_read_a_string() {
        assert_eq!(decoded(&json!({})).unwrap(), SearchBofipInput::default());
        let nulls =
            json!({"query": null, "serie": null, "kind": null, "page": null, "page_size": null});
        assert_eq!(decoded(&nulls).unwrap(), SearchBofipInput::default());
        let lenient = decoded(&json!({"page": "3", "page_size": 20.0})).unwrap();
        assert_eq!((lenient.page, lenient.page_size), (Some(3), Some(20)));
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"query": "donation", "fond": "CODE"})).unwrap_err(),
            "fond: unknown field `fond`, expected one of `query`, `serie`, `kind`, `page`, `page_size`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"query": 42})).unwrap_err(),
            "query: invalid type: integer `42`, expected a string"
        );
        assert_eq!(
            decoded(&json!({"serie": ["ENR"]})).unwrap_err(),
            "serie: invalid type: sequence, expected a string"
        );
        assert_eq!(
            decoded(&json!({"kind": true})).unwrap_err(),
            "kind: invalid type: boolean `true`, expected a string"
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
    fn the_schema_states_the_kinds_the_defaults_and_the_bounds() {
        let schema = schema();
        let properties = &schema["properties"];
        assert_eq!(properties["query"]["minLength"], 1);
        assert_eq!(properties["query"]["maxLength"], 450);
        assert_eq!(properties["serie"]["maxLength"], 16);
        assert_eq!(properties["kind"]["enum"], json!(["doctrine", "news", "all"]));
        assert_eq!(properties["kind"]["default"], "doctrine");
        assert_eq!(properties["page"]["minimum"], 1);
        assert_eq!(properties["page"]["default"], 1);
        assert_eq!(properties["page_size"]["minimum"], 1);
        assert_eq!(properties["page_size"]["maximum"], 100);
        assert_eq!(properties["page_size"]["default"], 10);
    }

    /// La description de `query` dit la règle mesurée du portail : tous les mots, le dernier en
    /// préfixe, ni opérateur ni expression entre guillemets (ADR-0029).
    #[test]
    fn the_query_states_the_matching_rule_of_the_portal() {
        let schema = schema();
        let query = schema["properties"]["query"]["description"].as_str().unwrap();
        assert!(query.contains("all must appear in the publication"), "{query}");
        assert!(query.contains("the last one also as a prefix"), "{query}");
        assert!(query.contains("No operators, no quoted phrases"), "{query}");
    }

    #[test]
    fn the_bounds_are_the_domain_bounds() {
        assert_eq!(lf_domain::search::QUERY_MAX_CHARS, 450);
        assert_eq!(lf_domain::doctrine::SERIE_MAX_BYTES, 16);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
