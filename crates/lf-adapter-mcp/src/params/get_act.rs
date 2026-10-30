//! Paramètres de `fr_get_act` (05 §5.2).
//!
//! Tous les champs sont des `Option` : la validation (identifiant, date, bornes de la table des
//! matières) reste au cas d'usage, qui rédige les messages de parité (05 §7.3). `text_id` est
//! pourtant annoncé `required` dans le schéma d'entrée : un client qui valide le voit avant
//! l'appel, et le message `text_id is required.` reste celui du cas d'usage. Les descriptions du
//! schéma sont posées par `#[schemars(description)]` ; les doc comments restent des notes de code.

use lf_application::input::GetActInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les quatre paramètres de `fr_get_act`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetActParams {
    /// Identifiant `LEGITEXT`, suffixe `_JJ-MM-AAAA` admis.
    #[schemars(
        required,
        with = "String",
        description = "A LEGITEXT... id (from fr_search on LODA or CODE)."
    )]
    pub(crate) text_id: Option<String>,
    /// Date de la version, `AAAA-MM-JJ`.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Version date YYYY-MM-DD (default: the version named by a suffixed text_id, else today in Paris).",
        extend("format" = "date")
    )]
    pub(crate) date: Option<String>,
    /// Première entrée de la table des matières.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "First table-of-contents entry (pass next_toc_offset).",
        extend("default" = 0)
    )]
    pub(crate) toc_offset: Option<i64>,
    /// Plafond d'entrées de la table des matières.
    #[serde(default, deserialize_with = "lenient::opt_int")]
    #[schemars(
        with = "u32",
        description = "Maximum table-of-contents entries (1..500).",
        range(min = 1, max = 500),
        extend("default" = 100)
    )]
    pub(crate) toc_limit: Option<i64>,
}

impl From<GetActParams> for GetActInput {
    fn from(params: GetActParams) -> Self {
        Self {
            text_id: params.text_id,
            date: params.date,
            toc_offset: params.toc_offset,
            toc_limit: params.toc_limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode::to_call, schema::input_schema};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn decoded(arguments: &Value) -> Result<GetActInput, String> {
        to_call::<GetActParams, GetActInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    #[test]
    fn the_four_arguments_reach_the_application_input() {
        let input = decoded(&json!({
            "text_id": "LEGITEXT000033205014_08-09-2023",
            "date": "2026-10-15",
            "toc_offset": 8,
            "toc_limit": 50,
        }))
        .unwrap();

        assert_eq!(
            input,
            GetActInput {
                text_id: Some("LEGITEXT000033205014_08-09-2023".to_owned()),
                date: Some("2026-10-15".to_owned()),
                toc_offset: Some(8),
                toc_limit: Some(50),
            }
        );
    }

    #[test]
    fn every_argument_is_optional_for_the_decoder_and_null_is_absent() {
        // `text_id is required.` est le message de parité du cas d'usage, pas du décodeur.
        assert_eq!(decoded(&json!({})).unwrap(), GetActInput::default());
        let input = decoded(&json!({
            "text_id": "LEGITEXT000033205014", "date": null, "toc_offset": null, "toc_limit": null,
        }))
        .unwrap();
        assert_eq!(
            input,
            GetActInput { text_id: Some("LEGITEXT000033205014".to_owned()), ..Default::default() }
        );
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        assert_eq!(
            decoded(&json!({"text_id": "LEGITEXT000033205014", "foo": 1})).unwrap_err(),
            "foo: unknown field `foo`, expected one of `text_id`, `date`, `toc_offset`, `toc_limit`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"text_id": 5})).unwrap_err(),
            "text_id: invalid type: integer `5`, expected a string"
        );
        // Le motif exact d'un entier mal typé appartient à `lenient` ; le chemin, lui, est le nôtre.
        let message = decoded(&json!({"toc_limit": ["x"]})).unwrap_err();
        assert!(message.starts_with("toc_limit: "), "{message}");
    }

    #[test]
    fn the_input_schema_announces_defaults_bounds_and_the_date_format() {
        let schema = Value::Object(input_schema::<GetActParams>().as_ref().clone());

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        // Seul `text_id` est requis, et il est annoncé en `string` (pas `["string","null"]`).
        assert_eq!(schema["required"], json!(["text_id"]));
        let properties = schema["properties"].as_object().unwrap();
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["date", "text_id", "toc_limit", "toc_offset"]);
        assert_eq!(properties["text_id"]["type"], "string");
        assert_eq!(
            properties["text_id"]["description"],
            "A LEGITEXT... id (from fr_search on LODA or CODE)."
        );
        assert_eq!(properties["date"]["type"], "string");
        assert_eq!(properties["date"]["format"], "date");
        assert_eq!(properties["toc_offset"]["minimum"], 0);
        assert_eq!(properties["toc_offset"]["default"], 0);
        assert_eq!(properties["toc_limit"]["minimum"], 1);
        assert_eq!(properties["toc_limit"]["maximum"], 500);
        assert_eq!(properties["toc_limit"]["default"], 100);
        assert!(schema.get("$schema").is_none());
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(input_schema::<GetActParams>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema);
        });
    }
}
