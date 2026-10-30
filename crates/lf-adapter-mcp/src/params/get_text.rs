//! Paramètres de `fr_get_text` (05 §5.3) : l'identifiant de l'article, plus les cinq paramètres de
//! lecture communs ([`ReadingParams`], 05 §5.0.2), aplatis dans le schéma d'entrée.
//!
//! Tous les champs sont des `Option` : la validation (forme de l'identifiant, bornes, ordre, messages
//! de parité) reste au cas d'usage, qui rédige les phrases de 05 §7.3. `article_id` est pourtant
//! annoncé `required` dans le schéma d'entrée : un client qui valide le voit avant l'appel, et le
//! message `article_id is required.` reste celui du cas d'usage. Les descriptions du schéma sont
//! posées par `#[schemars(description)]` ; les doc comments restent des notes de code.
//!
//! # Décodage
//!
//! Le schéma aplatit [`ReadingParams`] (`#[serde(flatten)]`), mais le décodage ne le fait pas :
//! serde tamponne les champs aplatis, ce qui perd le chemin de l'erreur (`invalid type: sequence,
//! expected an integer`, sans le nom du champ) et la liste des champs attendus d'un champ inconnu
//! (05 §7.3). `GetTextParams` se désérialise donc depuis [`Wire`], une structure plate à six champs
//! qui refuse les champs inconnus et garde le message « chemin: motif » de `decode::to_call`.

use lf_application::input::GetTextInput;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use super::reading::ReadingParams;
use crate::lenient;

/// Les six paramètres de `fr_get_text`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetTextParams {
    /// Identifiant `LEGIARTI` ou `KALIARTI`.
    #[schemars(
        required,
        with = "String",
        description = "A LEGIARTI... id (from fr_get_act or a CODE hit) or a KALIARTI... id (from fr_get_convention)."
    )]
    pub(crate) article_id: Option<String>,
    /// Les cinq paramètres de lecture, au premier niveau de l'objet d'arguments.
    #[serde(flatten)]
    pub(crate) reading: ReadingParams,
}

/// Les six champs à plat, dans l'ordre du schéma : la forme lue sur le fil.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    article_id: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default, deserialize_with = "lenient::opt_int")]
    offset: Option<i64>,
    #[serde(default, deserialize_with = "lenient::opt_int")]
    max_chars: Option<i64>,
    #[serde(default)]
    focus: Option<String>,
    #[serde(default, deserialize_with = "lenient::opt_int")]
    passages: Option<i64>,
}

impl<'de> Deserialize<'de> for GetTextParams {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            article_id: wire.article_id,
            reading: ReadingParams {
                detail: wire.detail,
                offset: wire.offset,
                max_chars: wire.max_chars,
                focus: wire.focus,
                passages: wire.passages,
            },
        })
    }
}

impl From<GetTextParams> for GetTextInput {
    fn from(params: GetTextParams) -> Self {
        Self { article_id: params.article_id, reading: params.reading.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode::to_call, schema::input_schema};
    use lf_application::input::ReadingInput;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn decoded(arguments: &Value) -> Result<GetTextInput, String> {
        to_call::<GetTextParams, GetTextInput>(arguments.as_object().unwrap().clone())
            .input
            .map_err(|error| error.message)
    }

    #[test]
    fn the_six_arguments_reach_the_application_input() {
        let input = decoded(&json!({
            "article_id": "LEGIARTI000006419288",
            "detail": "full",
            "offset": 120,
            "max_chars": 2000,
            "focus": "vie privée",
            "passages": 4,
        }))
        .unwrap();

        assert_eq!(
            input,
            GetTextInput {
                article_id: Some("LEGIARTI000006419288".to_owned()),
                reading: ReadingInput {
                    detail: Some("full".to_owned()),
                    offset: Some(120),
                    max_chars: Some(2000),
                    focus: Some("vie privée".to_owned()),
                    passages: Some(4),
                },
            }
        );
    }

    #[test]
    fn every_argument_is_optional_for_the_decoder_and_null_is_absent() {
        // `article_id is required.` est le message de parité du cas d'usage, pas du décodeur.
        assert_eq!(decoded(&json!({})).unwrap(), GetTextInput::default());
        let input = decoded(&json!({
            "article_id": "KALIARTI000005805295", "detail": null, "offset": null,
            "max_chars": null, "focus": null, "passages": null,
        }))
        .unwrap();
        assert_eq!(
            input,
            GetTextInput {
                article_id: Some("KALIARTI000005805295".to_owned()),
                ..GetTextInput::default()
            }
        );
    }

    #[test]
    fn lenient_integers_are_read_through_the_flattened_fields() {
        let input =
            decoded(&json!({"offset": "40", "max_chars": 2000.0, "passages": "3"})).unwrap();

        assert_eq!(
            input.reading,
            ReadingInput {
                offset: Some(40),
                max_chars: Some(2000),
                passages: Some(3),
                ..ReadingInput::default()
            }
        );
    }

    #[test]
    fn an_unknown_argument_is_refused_with_the_expected_names() {
        let expected = "foo: unknown field `foo`, expected one of `article_id`, `detail`, \
                        `offset`, `max_chars`, `focus`, `passages`";
        assert_eq!(decoded(&json!({"foo": 1})).unwrap_err(), expected);
        // Au milieu d'arguments valides, et pour une faute de frappe sur un champ de lecture.
        assert_eq!(
            decoded(&json!({"article_id": "LEGIARTI000006419288", "foo": 1, "offset": 3}))
                .unwrap_err(),
            expected
        );
        assert_eq!(
            decoded(&json!({"article_id": "LEGIARTI000006419288", "max_char": 900})).unwrap_err(),
            "max_char: unknown field `max_char`, expected one of `article_id`, `detail`, \
             `offset`, `max_chars`, `focus`, `passages`"
        );
    }

    #[test]
    fn a_mistyped_argument_names_its_path() {
        assert_eq!(
            decoded(&json!({"article_id": 5})).unwrap_err(),
            "article_id: invalid type: integer `5`, expected a string"
        );
        for (arguments, field) in [
            (json!({"offset": ["x"]}), "offset: "),
            (json!({"max_chars": {"a": 1}}), "max_chars: "),
            (json!({"passages": true}), "passages: "),
            (json!({"detail": 3}), "detail: "),
            (json!({"focus": 3}), "focus: "),
        ] {
            let message = decoded(&arguments).unwrap_err();
            assert!(message.starts_with(field), "{message}");
        }
    }

    #[test]
    fn the_input_schema_announces_the_identifier_and_the_five_reading_fields() {
        let schema = Value::Object(input_schema::<GetTextParams>().as_ref().clone());

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        // Seul `article_id` est requis, et il est annoncé en `string` (pas `["string","null"]`).
        assert_eq!(schema["required"], json!(["article_id"]));
        let properties = schema["properties"].as_object().unwrap();
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["article_id", "detail", "focus", "max_chars", "offset", "passages"]);
        assert_eq!(properties["article_id"]["type"], "string");
        assert_eq!(
            properties["article_id"]["description"],
            "A LEGIARTI... id (from fr_get_act or a CODE hit) or a KALIARTI... id (from \
             fr_get_convention)."
        );
        assert_eq!(properties["detail"]["enum"], json!(["summary", "full"]));
        assert_eq!(properties["offset"]["default"], 0);
        assert_eq!(properties["max_chars"]["minimum"], 500);
        assert_eq!(properties["max_chars"]["maximum"], 40_000);
        assert_eq!(properties["max_chars"]["default"], 4_000);
        assert_eq!(properties["focus"]["maxLength"], 500);
        assert_eq!(properties["passages"]["maximum"], 10);
        assert!(schema.get("$schema").is_none());
        assert!(schema.get("$defs").is_none(), "{schema}");
    }

    #[test]
    fn every_property_of_the_schema_is_accepted_by_the_decoder() {
        // Le schéma aplatit `ReadingParams`, le décodeur lit six champs à plat : un champ ajouté à
        // l'un sans l'autre serait annoncé sans être lu (ou lu sans être annoncé).
        let schema = input_schema::<GetTextParams>();
        for (name, kind) in schema["properties"].as_object().unwrap() {
            let value = if kind["type"] == "integer" { json!(1000) } else { json!("x") };
            let arguments = json!({ name: value });
            assert!(decoded(&arguments).is_ok(), "{name}");
        }
    }

    #[test]
    fn input_schema_snapshot() {
        // Le schéma complet, tel qu'annoncé aux clients : un changement est une décision (05 §9.5).
        let schema = Value::Object(input_schema::<GetTextParams>().as_ref().clone());
        insta::with_settings!({ snapshot_path => "../snapshots" }, {
            insta::assert_json_snapshot!("input_schema", schema);
        });
    }
}
