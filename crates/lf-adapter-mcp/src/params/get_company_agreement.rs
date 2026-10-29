//! Paramètres de `fr_get_company_agreement` (05 §5.7) : un seul, `agreement_id`.
//!
//! Le champ est `Option` pour que son absence (ou `null`) atteigne le cas d'usage, qui rend la
//! phrase de parité `agreement_id is required.` ; le schéma, lui, le déclare requis
//! (`#[schemars(required)]`). Les descriptions du schéma sont en anglais (Q14), posées par
//! `#[schemars(description)]` et non par un commentaire de doc.

use lf_application::input::GetCompanyAgreementInput;
use schemars::JsonSchema;
use serde::Deserialize;

/// Arguments de l'outil, tels que le client les envoie.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetCompanyAgreementParams {
    // ACCOTEXT + 12 chiffres, vérifié par le cas d'usage.
    #[schemars(required, description = "An ACCOTEXT... id (from fr_search on fond ACCO).")]
    pub(crate) agreement_id: Option<String>,
}

impl From<GetCompanyAgreementParams> for GetCompanyAgreementInput {
    fn from(params: GetCompanyAgreementParams) -> Self {
        Self { agreement_id: params.agreement_id }
    }
}

#[cfg(test)]
mod tests {
    use lf_application::input::ToolCall;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    use super::*;
    use crate::{decode::to_call, schema::input_schema};

    fn decode(arguments: &Value) -> ToolCall<GetCompanyAgreementInput> {
        to_call::<GetCompanyAgreementParams, GetCompanyAgreementInput>(
            arguments.as_object().unwrap().clone(),
        )
    }

    #[test]
    fn the_id_is_decoded_as_given() {
        let call = decode(&json!({"agreement_id": " accotext000053643828 "}));
        assert_eq!(
            call.input.unwrap(),
            GetCompanyAgreementInput { agreement_id: Some(" accotext000053643828 ".to_owned()) }
        );
        assert_eq!(call.canonical.as_str(), r#"{"agreement_id":" accotext000053643828 "}"#);
    }

    #[test]
    fn an_absent_or_null_id_reaches_the_use_case_as_absent() {
        for arguments in [json!({}), json!({"agreement_id": null})] {
            assert_eq!(decode(&arguments).input.unwrap(), GetCompanyAgreementInput::default());
        }
    }

    #[test]
    fn a_wrong_type_or_an_unknown_field_is_a_decoding_error() {
        assert_eq!(
            decode(&json!({"agreement_id": 5})).input.unwrap_err().message,
            "agreement_id: invalid type: integer `5`, expected a string"
        );
        assert_eq!(
            decode(&json!({"agreement_id": "ACCOTEXT000053643828", "foo": 1}))
                .input
                .unwrap_err()
                .message,
            "foo: unknown field `foo`, expected `agreement_id`"
        );
    }

    #[test]
    fn the_schema_requires_a_string_id_and_closes_the_object() {
        let schema = Value::Object(input_schema::<GetCompanyAgreementParams>().as_ref().clone());
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["agreement_id"]));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["agreement_id"],
            json!({
                "type": "string",
                "description": "An ACCOTEXT... id (from fr_search on fond ACCO)."
            })
        );
        assert!(schema.get("$schema").is_none());
    }
}
