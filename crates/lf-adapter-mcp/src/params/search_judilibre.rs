//! Paramètres de `fr_search_judilibre` (05 §5.18).
//!
//! Tous les champs sont des `Option` : la validation (mots, juridictions, partie, dates, tri,
//! pagination) reste au cas d'usage, qui rédige les messages. `query` est pourtant annoncée
//! `required` dans le schéma d'entrée, et son absence rend `query is required.`. Les descriptions
//! du schéma sont en anglais (Q14), posées par `#[schemars(description)]` ; les doc comments
//! restent des notes de code.

use lf_application::input::SearchJudilibreInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les neuf paramètres de `fr_search_judilibre`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchJudilibreParams {
    /// Mots cherchés, 1 à 450 caractères (comme `fr_search`).
    #[schemars(
        required,
        with = "String",
        description = "A few plain words, matched in the decisions (Judilibre search engine).",
        length(min = 1, max = 450)
    )]
    pub(crate) query: Option<String>,
    /// Mode de correspondance (`match` côté MCP, mot-clé Rust côté code).
    #[serde(default, rename = "match")]
    #[schemars(
        with = "String",
        description = r#""all": all words (default); "any": any word; "exact": exact phrase."#,
        extend("enum" = ["any", "all", "exact"], "default" = "all")
    )]
    pub(crate) match_mode: Option<String>,
    /// Juridictions ; insensibles à la casse, vérifiées par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "Vec<String>",
        description = "Courts: cc (Cour de cassation), ca (courts of appeal), tj (judicial courts), tcom (commercial courts), cph (labour courts). Default: all five.",
        length(min = 1, max = 5),
        extend(
            "items" = { "type": "string", "enum": ["cc", "ca", "tj", "tcom", "cph"] },
            "uniqueItems" = true,
            "default" = ["cc", "ca", "tj", "tcom", "cph"]
        )
    )]
    pub(crate) courts: Option<Vec<String>>,
    /// Partie où chercher ; absente : tout le texte.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Search only this part of the decisions (slower, especially motivations). Default: the whole text.",
        extend("enum" = ["motivations", "dispositif", "expose", "moyens", "summary", "visa"])
    )]
    pub(crate) part: Option<String>,
    /// Date de décision la plus ancienne, `AAAA-MM-JJ`.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Earliest decision date YYYY-MM-DD.",
        extend("format" = "date")
    )]
    pub(crate) date_from: Option<String>,
    /// Date de décision la plus récente, `AAAA-MM-JJ`.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Latest decision date YYYY-MM-DD.",
        extend("format" = "date")
    )]
    pub(crate) date_to: Option<String>,
    /// Ordre des résultats.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = r#""relevance" (default) or "date" (newest first)."#,
        extend("enum" = ["relevance", "date"], "default" = "relevance")
    )]
    pub(crate) sort: Option<String>,
    /// Numéro de page, à partir de 1.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Page number, from 1; page x page_size at most 10000.",
        range(min = 1),
        extend("default" = 1)
    )]
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

impl From<SearchJudilibreParams> for SearchJudilibreInput {
    fn from(params: SearchJudilibreParams) -> Self {
        Self {
            query: params.query,
            match_mode: params.match_mode,
            courts: params.courts,
            part: params.part,
            date_from: params.date_from,
            date_to: params.date_to,
            sort: params.sort,
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

    fn decoded(arguments: &Value) -> Result<SearchJudilibreInput, String> {
        to_call::<SearchJudilibreParams, SearchJudilibreInput>(
            arguments.as_object().unwrap().clone(),
        )
        .input
        .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<SearchJudilibreParams>().as_ref().clone())
    }

    #[test]
    fn the_nine_arguments_reach_the_application_input() {
        let input = decoded(&json!({
            "query": "donation indirecte", "match": "ANY", "courts": ["ca", "CC"],
            "part": "motivations", "date_from": "2020-01-01", "date_to": "2023-12-31",
            "sort": "date", "page": 2, "page_size": 25,
        }))
        .unwrap();
        assert_eq!(
            input,
            SearchJudilibreInput {
                query: Some("donation indirecte".to_owned()),
                match_mode: Some("ANY".to_owned()),
                courts: Some(vec!["ca".to_owned(), "CC".to_owned()]),
                part: Some("motivations".to_owned()),
                date_from: Some("2020-01-01".to_owned()),
                date_to: Some("2023-12-31".to_owned()),
                sort: Some("date".to_owned()),
                page: Some(2),
                page_size: Some(25),
            }
        );
    }

    #[test]
    fn no_argument_and_null_mean_absent_and_integers_read_a_string() {
        assert_eq!(decoded(&json!({})).unwrap(), SearchJudilibreInput::default());
        let nulls = json!({
            "query": null, "match": null, "courts": null, "part": null, "date_from": null,
            "date_to": null, "sort": null, "page": null, "page_size": null,
        });
        assert_eq!(decoded(&nulls).unwrap(), SearchJudilibreInput::default());
        let lenient = decoded(&json!({"page": "3", "page_size": 20.0})).unwrap();
        assert_eq!((lenient.page, lenient.page_size), (Some(3), Some(20)));
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"query": "bail", "chamber": "civ1"})).unwrap_err(),
            "chamber: unknown field `chamber`, expected one of `query`, `match`, `courts`, `part`, \
             `date_from`, `date_to`, `sort`, `page`, `page_size`"
        );
        assert!(decoded(&json!({"match_mode": "all"})).unwrap_err().starts_with("match_mode:"));
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"courts": "cc"})).unwrap_err(),
            "courts: invalid type: string \"cc\", expected a sequence"
        );
        assert_eq!(
            decoded(&json!({"courts": ["cc", 2]})).unwrap_err(),
            "courts[1]: invalid type: integer `2`, expected a string"
        );
    }

    #[test]
    fn the_schema_states_the_values_the_defaults_and_the_bounds() {
        let schema = schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["query"]));
        let properties = &schema["properties"];
        assert_eq!(properties["query"]["maxLength"], 450);
        assert_eq!(properties["match"]["enum"], json!(["any", "all", "exact"]));
        assert_eq!(properties["match"]["default"], "all");
        assert_eq!(properties["courts"]["items"]["enum"], json!(["cc", "ca", "tj", "tcom", "cph"]));
        assert_eq!(
            (properties["courts"]["minItems"].clone(), properties["courts"]["maxItems"].clone()),
            (json!(1), json!(5))
        );
        assert_eq!(properties["courts"]["uniqueItems"], true);
        assert_eq!(
            properties["part"]["enum"],
            json!(["motivations", "dispositif", "expose", "moyens", "summary", "visa"])
        );
        assert_eq!(properties["date_from"]["format"], "date");
        assert_eq!(properties["sort"]["enum"], json!(["relevance", "date"]));
        assert_eq!(properties["page_size"]["maximum"], 50);
        assert_eq!(properties["page_size"]["default"], 10);
        assert!(properties.get("match_mode").is_none(), "la clé du fil est `match`");
    }

    /// Les valeurs annoncées sont celles que le domaine accepte.
    #[test]
    fn the_announced_values_are_the_domain_ones() {
        use lf_domain::judilibre::{CASE_LAW_PAGE_SIZE_MAX, CaseLawSort, Court, SearchPart};
        let properties = &schema()["properties"];
        assert_eq!(properties["courts"]["items"]["enum"], json!(Court::CODES));
        assert_eq!(properties["part"]["enum"], json!(SearchPart::NAMES));
        assert_eq!(properties["sort"]["enum"], json!(CaseLawSort::NAMES));
        assert_eq!(properties["page_size"]["maximum"], CASE_LAW_PAGE_SIZE_MAX);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema());
        });
    }
}
