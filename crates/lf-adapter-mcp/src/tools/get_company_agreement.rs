//! Outil `fr_get_company_agreement` (05 §5.7) : les métadonnées d'un accord d'entreprise (ACCO).
//!
//! Le texte d'un accord n'existe que comme pièce jointe `.docx` : l'outil ne le rend pas (P-036).
//! Motif de `tools/mod.rs` : arguments bruts (`JsonObject`), décodage par `to_call`, appel de la
//! façade avec le jeton d'annulation de la requête, vue de sortie à schéma explicite.

use lf_application::{
    input::GetCompanyAgreementInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::get_company_agreement::GetCompanyAgreementParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::get_company_agreement::CompanyAgreementView,
};

/// Description de l'outil (349 caractères, 3 phrases ; 05 §5.7).
pub(crate) const DESCRIPTION: &str = "Read a company-level agreement (accord d'entreprise) by its ACCOTEXT id from fr_search on fond ACCO. Returns METADATA ONLY: company, SIRET, IDCC, APE code, sector, themes, signatory unions, dates, citation and source_url. Légifrance distributes the full text only as a .docx attachment, which this tool does not return: point the user to source_url.";

#[tool_router(router = get_company_agreement_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_get_company_agreement",
        title = "Read a company agreement (metadata)",
        description = DESCRIPTION,
        input_schema = input_schema::<GetCompanyAgreementParams>(),
        output_schema = output_schema::<CompanyAgreementView>(),
        annotations(
            title = "Read a company agreement (metadata)",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_company_agreement(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<CompanyAgreementView>, ToolFailure> {
        let call = to_call::<GetCompanyAgreementParams, GetCompanyAgreementInput>(args);
        let output = self.service.get_company_agreement(call, ct.cancelled()).await?;
        Ok(Json(CompanyAgreementView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 349);
        assert!(DESCRIPTION.starts_with("Read a company-level agreement"));
        assert!(DESCRIPTION.contains("METADATA ONLY"));
        assert!(DESCRIPTION.ends_with("point the user to source_url."));
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{
            error::ErrorCode,
            ports::{AuditStatus, CorpusError},
            tools::ToolName,
        };
        use lf_domain::{
            contract::Idcc, date::CivilDate, entity::CompanyAgreement, id::AccoTextId,
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        const ID: &str = "ACCOTEXT000053643828";

        fn agreement() -> CompanyAgreement {
            CompanyAgreement {
                agreement_id: AccoTextId::parse(ID).unwrap(),
                nature: Some("ACCORD".to_owned()),
                title: Some("Accord d'entreprise relatif au télétravail".to_owned()),
                raison_sociale: Some("CORSAIR (Télétravail)".to_owned()),
                siret: Some("32862158600143".to_owned()),
                idcc: Idcc::parse("275"),
                code_ape: Some("5110Z".to_owned()),
                secteur: Some("Transports aériens de passagers".to_owned()),
                themes: vec!["Télétravail".to_owned()],
                syndicats: vec!["CFTC".to_owned(), "CFE-CGC".to_owned(), "Autre".to_owned()],
                date_texte: CivilDate::new(2025, 12, 19),
                date_effet: CivilDate::new(2025, 12, 20),
                date_depot: CivilDate::new(2026, 2, 27),
            }
        }

        fn request(arguments: &Value) -> CallToolRequestParams {
            let mut params = CallToolRequestParams::new("fr_get_company_agreement");
            params.arguments = arguments.as_object().cloned();
            params
        }

        #[tokio::test]
        async fn the_descriptor_carries_title_schemas_and_annotations() {
            let client = Fixture::default().connect().await;
            let tools = client.list_all_tools().await.unwrap();
            let tool = tools.iter().find(|tool| tool.name == "fr_get_company_agreement").unwrap();
            assert_eq!(tool.title.as_deref(), Some("Read a company agreement (metadata)"));
            assert_eq!(tool.description.as_deref(), Some(super::super::DESCRIPTION));
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.title.as_deref(), tool.title.as_deref());
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(true));
            let input = Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(input["required"], json!(["agreement_id"]));
            assert_eq!(input["additionalProperties"], false);
            assert!(input.get("$schema").is_none());
            let output = Value::Object(tool.output_schema.as_ref().unwrap().as_ref().clone());
            assert!(output["properties"].get("text").is_none());
            assert!(output.get("$schema").is_none());
        }

        #[tokio::test]
        async fn a_read_returns_the_structured_view_and_is_audited() {
            let fixture = Fixture::default();
            fixture.corpus.push_get_company_agreement(Ok(agreement()));
            let client = fixture.connect().await;
            let result =
                client.call_tool(request(&json!({"agreement_id": " accotext000053643828 "}))).await;
            let result = result.unwrap();
            assert_eq!(result.is_error, Some(false));
            let structured = result.structured_content.unwrap();
            assert_eq!(structured["agreement_id"], ID);
            assert_eq!(structured["raison_sociale"], "CORSAIR (Télétravail)");
            assert_eq!(structured["idcc"], "275");
            assert!(structured.get("text").is_none());
            // Le bloc texte est le même JSON (compatibilité des clients sans structuredContent).
            let text = result.content[0].as_text().unwrap().text.clone();
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), structured);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_GET_COMPANY_AGREEMENT);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(records[0].output_count_or_size, 1);
            assert_eq!(fixture.corpus.calls(), 1);
        }

        #[tokio::test]
        async fn an_unknown_agreement_is_an_in_band_not_found() {
            let fixture = Fixture::default();
            fixture.corpus.push_get_company_agreement(Err(CorpusError::NotFound {
                what: "/consult/acco".to_owned(),
            }));
            let client = fixture.connect().await;
            let result = client.call_tool(request(&json!({"agreement_id": ID}))).await.unwrap();
            assert_eq!(result.is_error, Some(true));
            assert!(result.structured_content.is_none());
            let text = &result.content[0].as_text().unwrap().text;
            assert!(
                text.starts_with(&format!("[not_found] No company agreement found for {ID}.")),
                "{text}"
            );
            let records = fixture.audit.records();
            assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
            assert_eq!(records[0].output_count_or_size, 0);
        }

        #[tokio::test]
        async fn bad_arguments_are_an_audited_invalid_arg_without_a_port_call() {
            let fixture = Fixture::default();
            let client = fixture.connect().await;
            for (arguments, expected) in [
                (json!({}), "[invalid_arg] agreement_id is required.".to_owned()),
                (
                    json!({"agreement_id": 5}),
                    "[invalid_arg] agreement_id: invalid type: integer `5`, expected a string"
                        .to_owned(),
                ),
            ] {
                let result = client.call_tool(request(&arguments)).await.unwrap();
                assert_eq!(result.is_error, Some(true), "{arguments}");
                assert_eq!(result.content[0].as_text().unwrap().text, expected);
            }
            assert_eq!(fixture.corpus.calls(), 0);
            let records = fixture.audit.records();
            assert_eq!(records.len(), 2);
            assert!(records.iter().all(|record| record.error_code == Some(ErrorCode::InvalidArg)));
        }
    }
}
