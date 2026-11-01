//! Paramètres de `fr_search` (05 §5.1, §3.3).
//!
//! Tous les champs sont des `Option` : la validation (bornes, ordre, messages de parité) reste au
//! cas d'usage (`lf_application::usecase::search`). `query` est pourtant annoncée requise dans le
//! schéma (`required`), pour que le modèle la fournisse ; son absence rend le message de parité
//! `query is required.` (P-012). Les descriptions du schéma sont en anglais ; les doc comments des
//! champs restent des notes de code en français.
//!
//! Schéma : `with = "String"` fait lire le champ comme un `String` obligatoire, d'où `query` seule
//! dans `required` ; les autres champs portent `#[serde(default, skip_serializing_if = ...)]`, qui
//! les sort de `required` sans ajouter `"default": null` au schéma (faux pour un `string`). Le
//! `skip_serializing_if` ne sert qu'à schemars : la struct n'est que désérialisée.

use lf_application::input::SearchInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Arguments de `fr_search`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchParams {
    /// Texte de la requête, 1 à 450 caractères après normalisation (V-81).
    #[schemars(
        required,
        with = "String",
        description = r#"Free-text query, e.g. "republique numerique"."#,
        length(min = 1, max = 450)
    )]
    pub(crate) query: Option<String>,
    /// Fond interrogé, insensible à la casse ; `LODA` par défaut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Fond to search (case-insensitive).",
        extend(
            "enum" = ["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"],
            "default" = "LODA"
        )
    )]
    pub(crate) fond: Option<String>,
    /// Mode de correspondance (`match` côté MCP, mot-clé Rust côté code).
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = r#""any": any word (default); "all": all words; "exact": exact phrase."#,
        extend("enum" = ["any", "all", "exact"], "default" = "any")
    )]
    pub(crate) match_mode: Option<String>,
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
    /// Début de période, `AAAA-MM-JJ`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "Earliest date YYYY-MM-DD (fond date filter).",
        extend("format" = "date")
    )]
    pub(crate) date_from: Option<String>,
    /// Fin de période, `AAAA-MM-JJ`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", description = "Latest date YYYY-MM-DD.", extend("format" = "date"))]
    pub(crate) date_to: Option<String>,
    /// Date de version pour `LODA` et `CODE`, `AAAA-MM-JJ`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "String",
        description = "LODA and CODE only: version in force on this date, YYYY-MM-DD (default today, Paris).",
        extend("format" = "date")
    )]
    pub(crate) version_date: Option<String>,
    /// Textes en vigueur seulement (`KALI` seulement, D-08).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "bool",
        description = "KALI only: keep texts in force (VIGUEUR, VIGUEUR_ETEN, VIGUEUR_NON_ETEN), drop DENONCE, MODIFIE, PERIME, REMPLACE, ABROGE. It filters texts, not the status of their articles.",
        extend("default" = false)
    )]
    pub(crate) in_force_only: Option<bool>,
}

impl From<SearchParams> for SearchInput {
    fn from(params: SearchParams) -> Self {
        Self {
            query: params.query,
            fond: params.fond,
            match_mode: params.match_mode,
            page: params.page,
            page_size: params.page_size,
            date_from: params.date_from,
            date_to: params.date_to,
            version_date: params.version_date,
            in_force_only: params.in_force_only,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::schema::input_schema;

    fn decode(value: &Value) -> Result<SearchInput, String> {
        serde_path_to_error::deserialize::<_, SearchParams>(value.clone())
            .map(SearchInput::from)
            .map_err(|error| error.to_string())
    }

    fn schema() -> Value {
        Value::Object(input_schema::<SearchParams>().as_ref().clone())
    }

    #[test]
    fn no_argument_means_every_field_absent() {
        assert_eq!(decode(&json!({})).unwrap(), SearchInput::default());
    }

    #[test]
    fn the_nine_arguments_reach_the_application_input_and_match_is_renamed() {
        let input = decode(&json!({
            "query": "bail", "fond": "code", "match": "all", "page": 2, "page_size": 25,
            "date_from": "2020-01-01", "date_to": "2021-01-01", "version_date": "2026-01-01",
            "in_force_only": true
        }))
        .unwrap();
        assert_eq!(
            input,
            SearchInput {
                query: Some("bail".to_owned()),
                fond: Some("code".to_owned()),
                match_mode: Some("all".to_owned()),
                page: Some(2),
                page_size: Some(25),
                date_from: Some("2020-01-01".to_owned()),
                date_to: Some("2021-01-01".to_owned()),
                version_date: Some("2026-01-01".to_owned()),
                in_force_only: Some(true),
            }
        );
    }

    #[test]
    fn in_force_only_is_a_strict_optional_boolean() {
        assert_eq!(decode(&json!({"in_force_only": false})).unwrap().in_force_only, Some(false));
        assert_eq!(decode(&json!({"in_force_only": null})).unwrap().in_force_only, None);
        assert_eq!(
            decode(&json!({"in_force_only": "yes"})).unwrap_err(),
            "in_force_only: invalid type: string \"yes\", expected a boolean"
        );
    }

    #[test]
    fn the_schema_describes_in_force_only_as_an_optional_boolean_defaulting_to_false() {
        let schema = schema();
        let prop = &schema["properties"]["in_force_only"];
        assert_eq!(prop["type"], "boolean");
        assert_eq!(prop["default"], false);
        assert_eq!(schema["required"], json!(["query"]));
        let description = prop["description"].as_str().unwrap();
        assert!(description.starts_with("KALI only: keep texts in force"), "{description}");
        for state in ["VIGUEUR_NON_ETEN", "DENONCE", "MODIFIE", "PERIME", "REMPLACE", "ABROGE"] {
            assert!(description.contains(state), "{state}");
        }
    }

    #[test]
    fn null_is_read_as_absent() {
        let input = decode(&json!({
            "query": null, "fond": null, "match": null, "page": null, "page_size": null,
            "date_from": null, "date_to": null, "version_date": null, "in_force_only": null
        }))
        .unwrap();
        assert_eq!(input, SearchInput::default());
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        let error = decode(&json!({"foo": 1})).unwrap_err();
        assert!(
            error.starts_with("foo: unknown field `foo`, expected one of `query`, `fond`, `match`"),
            "{error}"
        );
    }

    #[test]
    fn a_wrong_type_names_the_field() {
        assert_eq!(
            decode(&json!({"query": 42})).unwrap_err(),
            "query: invalid type: integer `42`, expected a string"
        );
        assert_eq!(
            decode(&json!({"fond": ["LODA"]})).unwrap_err(),
            "fond: invalid type: sequence, expected a string"
        );
    }

    #[test]
    fn the_input_schema_is_closed_and_requires_only_the_query() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["query"]));
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn the_schema_states_enums_defaults_and_bounds() {
        let schema = schema();
        let props = &schema["properties"];
        assert_eq!(
            props["fond"]["enum"],
            json!(["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"])
        );
        assert_eq!(props["fond"]["default"], "LODA");
        assert_eq!(props["match"]["enum"], json!(["any", "all", "exact"]));
        assert_eq!(props["match"]["default"], "any");
        assert_eq!(props["page_size"]["minimum"], 1);
        assert_eq!(props["page_size"]["maximum"], 100);
        assert_eq!(props["page_size"]["default"], 10);
        assert_eq!(props["page"]["minimum"], 1);
        assert_eq!(props["query"]["minLength"], 1);
        assert_eq!(props["query"]["maxLength"], 450);
        for date in ["date_from", "date_to", "version_date"] {
            assert_eq!(props[date]["type"], "string", "{date}");
            assert_eq!(props[date]["format"], "date", "{date}");
        }
        assert!(props.get("match_mode").is_none(), "la clé du fil est `match`");
    }

    #[test]
    fn the_query_bound_is_the_domain_bound() {
        assert_eq!(lf_domain::search::QUERY_MAX_CHARS, 450);
    }

    #[test]
    fn input_schema_snapshot() {
        insta::assert_json_snapshot!("input_schema", schema());
    }
}
