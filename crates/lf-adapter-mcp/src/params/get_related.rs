//! Paramètres de `fr_get_related` (05 §5.11).
//!
//! Les champs sont des `Option` : la validation (identifiant requis, famille lue, sens de
//! `relation`) reste au cas d'usage, qui rédige les messages de parité. `id` est pourtant annoncé `required` dans le schéma
//! d'entrée : un client qui valide le voit avant l'appel, et le message `id is required.` reste
//! celui du cas d'usage. La description du schéma est posée par `#[schemars(description)]` ; les
//! doc comments restent des notes de code.

use lf_application::input::GetRelatedInput;
use schemars::JsonSchema;
use serde::Deserialize;

/// Les paramètres de `fr_get_related` : `id`, et `relation` (facultatif, D-05).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetRelatedParams {
    /// Identifiant d'un article (`LEGIARTI`) ou d'une décision (`JURITEXT`, `CONSTEXT`,
    /// `CETATEXT`).
    #[schemars(
        required,
        with = "String",
        description = "A LEGIARTI... article id or a JURITEXT..., CONSTEXT... or CETATEXT... decision id."
    )]
    pub(crate) id: Option<String>,
    /// Sens des seuls liens demandés ; insensible à la casse, vérifié par le cas d'usage.
    #[serde(default)]
    #[schemars(
        with = "String",
        description = "Only the links of this relation: cites or cited_by for an article, cites_case_law or applies_text for a decision; the other family is refused. Omit it to get them all. The filter is applied before the 100-link cap, and total counts the links of that relation.",
        extend("enum" = ["cites", "cited_by", "cites_case_law", "applies_text"])
    )]
    pub(crate) relation: Option<String>,
}

impl From<GetRelatedParams> for GetRelatedInput {
    fn from(params: GetRelatedParams) -> Self {
        Self { id: params.id, relation: params.relation }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode::to_call, schema::input_schema};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn decoded(arguments: &Value) -> Result<GetRelatedInput, String> {
        to_call::<GetRelatedParams, GetRelatedInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    fn schema() -> Value {
        Value::Object(input_schema::<GetRelatedParams>().as_ref().clone())
    }

    #[test]
    fn the_id_reaches_the_application_input() {
        let input = decoded(&json!({"id": "LEGIARTI000006419288"})).unwrap();
        assert_eq!(
            input,
            GetRelatedInput { id: Some("LEGIARTI000006419288".to_owned()), relation: None }
        );
    }

    #[test]
    fn the_relation_reaches_the_application_input_as_given() {
        // La casse est réglée par le cas d'usage, pas par le décodeur.
        let input =
            decoded(&json!({"id": "LEGIARTI000006419288", "relation": "Cited_By"})).unwrap();
        assert_eq!(
            input,
            GetRelatedInput {
                id: Some("LEGIARTI000006419288".to_owned()),
                relation: Some("Cited_By".to_owned())
            }
        );
        let input = decoded(&json!({"id": "LEGIARTI000006419288", "relation": null})).unwrap();
        assert_eq!(input.relation, None);
    }

    #[test]
    fn no_argument_and_null_mean_absent() {
        // `id is required.` est le message de parité du cas d'usage, pas du décodeur.
        assert_eq!(decoded(&json!({})).unwrap(), GetRelatedInput::default());
        assert_eq!(decoded(&json!({"id": null})).unwrap(), GetRelatedInput::default());
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_name() {
        assert_eq!(
            decoded(&json!({"id": "LEGIARTI000006419288", "foo": 1})).unwrap_err(),
            "foo: unknown field `foo`, expected `id` or `relation`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"id": 7})).unwrap_err(),
            "id: invalid type: integer `7`, expected a string"
        );
    }

    #[test]
    fn a_mistyped_relation_names_its_path() {
        assert_eq!(
            decoded(&json!({"id": "LEGIARTI000006419288", "relation": 2})).unwrap_err(),
            "relation: invalid type: integer `2`, expected a string"
        );
    }

    #[test]
    fn the_relation_is_an_optional_closed_enumeration() {
        let schema = schema();
        assert_eq!(schema["required"], json!(["id"]), "relation n'est pas requise");
        let relation = &schema["properties"]["relation"];
        assert_eq!(relation["type"], "string");
        assert_eq!(
            relation["enum"],
            json!(["cites", "cited_by", "cites_case_law", "applies_text"])
        );
        let description = relation["description"].as_str().unwrap();
        assert!(description.chars().count() <= 1000, "{description}");
        assert!(description.contains("the other family is refused"), "{description}");
    }

    #[test]
    fn the_input_schema_requires_the_id() {
        let schema = schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["id"]));
        assert!(schema.get("$schema").is_none());
        let id = &schema["properties"]["id"];
        assert_eq!(id["type"], "string");
        assert_eq!(
            id["description"],
            "A LEGIARTI... article id or a JURITEXT..., CONSTEXT... or CETATEXT... decision id."
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
