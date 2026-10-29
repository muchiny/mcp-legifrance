//! Paramètres de `fr_get_convention` (05 §5.6).
//!
//! Ceux de `fr_get_act` sans la date : l'API ne versionne pas `/consult/kaliText` (00 §7). Tous les
//! champs sont des `Option`, la validation reste au cas d'usage ; `convention_id` est annoncé
//! `required` dans le schéma d'entrée.

use lf_application::input::GetConventionInput;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lenient;

/// Les trois paramètres de `fr_get_convention`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetConventionParams {
    /// Identifiant `KALITEXT`.
    #[schemars(
        required,
        with = "String",
        description = "A KALITEXT... id (from fr_search on fond KALI)."
    )]
    pub(crate) convention_id: Option<String>,
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

impl From<GetConventionParams> for GetConventionInput {
    fn from(params: GetConventionParams) -> Self {
        Self {
            convention_id: params.convention_id,
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

    fn decoded(arguments: &Value) -> Result<GetConventionInput, String> {
        to_call::<GetConventionParams, GetConventionInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    #[test]
    fn the_three_arguments_reach_the_application_input() {
        let input = decoded(&json!({
            "convention_id": "KALITEXT000049878950", "toc_offset": 3, "toc_limit": 20,
        }))
        .unwrap();

        assert_eq!(
            input,
            GetConventionInput {
                convention_id: Some("KALITEXT000049878950".to_owned()),
                toc_offset: Some(3),
                toc_limit: Some(20),
            }
        );
        assert_eq!(decoded(&json!({})).unwrap(), GetConventionInput::default());
        assert_eq!(
            decoded(&json!({"convention_id": "KALITEXT000049878950", "toc_offset": null}))
                .unwrap()
                .toc_offset,
            None
        );
    }

    #[test]
    fn convention_has_no_date_param() {
        // Pas de `date` (00 §7) : l'API ne versionne pas `/consult/kaliText`.
        assert_eq!(
            decoded(&json!({"convention_id": "KALITEXT000049878950", "date": "2020-01-01"}))
                .unwrap_err(),
            "date: unknown field `date`, expected one of `convention_id`, `toc_offset`, `toc_limit`"
        );
        let schema = Value::Object(input_schema::<GetConventionParams>().as_ref().clone());
        let properties = schema["properties"].as_object().unwrap();
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["convention_id", "toc_limit", "toc_offset"]);
    }

    #[test]
    fn the_input_schema_announces_defaults_and_bounds() {
        let schema = Value::Object(input_schema::<GetConventionParams>().as_ref().clone());

        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["convention_id"]));
        let properties = &schema["properties"];
        assert_eq!(properties["convention_id"]["type"], "string");
        assert_eq!(
            properties["convention_id"]["description"],
            "A KALITEXT... id (from fr_search on fond KALI)."
        );
        assert_eq!(properties["toc_offset"]["minimum"], 0);
        assert_eq!(properties["toc_limit"]["maximum"], 500);
        assert_eq!(properties["toc_limit"]["default"], 100);
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(input_schema::<GetConventionParams>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema);
        });
    }
}
