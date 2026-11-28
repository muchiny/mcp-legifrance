//! Méthode de façade `LegalService::get_service_public` pour `fr_get_service_public` (jalon MS).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_GET_SERVICE_PUBLIC` : le décodage des arguments, la validation, la lecture de la
//! page dans l'archive et la lecture de son texte forment l'exécution complète auditée.

use std::future::Future;

use crate::ports::JudicialOpenData;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetServicePublicInput, ToolCall},
    output::get_service_public::GetServicePublicOutput,
    ports::{AuditLog, Clock, LegalCorpus, PublicGuidance, TaxDoctrine},
    tools::ToolName,
    usecase::get_service_public::GetServicePublicUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance, J: JudicialOpenData>
    LegalService<C, A, K, D, G, J>
{
    /// `fr_get_service_public` : lecture d'une fiche ou d'une page de navigation de
    /// Service-Public (05 §5.17). Métrique d'audit : les caractères renvoyés pour une fiche, le
    /// nombre d'entrées pour une page de navigation.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, arguments, `offset` au-delà du texte), `not_found` (numéro absent
    /// de l'archive du public : le message nomme l'autre), `config_error` (source coupée, sans
    /// aucune E/S), `rate_limited`, `upstream_error`, le dépassement de l'échéance globale, et
    /// `audit_error` en mode strict.
    pub async fn get_service_public(
        &self,
        call: ToolCall<GetServicePublicInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetServicePublicOutput, AppError> {
        let meta =
            CallMeta::new(ToolName::FR_GET_SERVICE_PUBLIC, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = GetServicePublicUseCase::new(&self.guidance);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use lf_domain::guidance::Audience;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, InputError},
        messages,
        ports::{AuditStatus, GuidanceError},
        service::bofip_fixtures::{service_with_guidance, service_without_doctrine},
        testing::fixtures::service_public::{entry, folder, read},
    };

    fn call(arguments: &str, id: &str) -> ToolCall<GetServicePublicInput> {
        ToolCall {
            input: Ok(GetServicePublicInput {
                id: Some(id.to_owned()),
                ..GetServicePublicInput::default()
            }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    #[tokio::test]
    async fn a_folder_read_is_audited_with_its_number_of_entries() {
        let (service, log) = service_with_guidance();
        let entries = vec![entry("F1437", "Compte"), entry("F1903", "Temps de travail")];
        service.guidance.push_get(Ok(read(folder(Audience::Particuliers, "N31675", entries))));
        let arguments = r#"{"id":"N31675"}"#;
        let output = service.get_service_public(call(arguments, "N31675"), pending()).await;
        assert_eq!(output.unwrap().entry(), "N31675.xml");
        let records = log.records();
        assert_eq!(records.len(), 1);
        let line = &records[0];
        assert_eq!(line.tool, ToolName::FR_GET_SERVICE_PUBLIC);
        assert_eq!((line.status, line.output_count_or_size), (AuditStatus::Ok, 2));
        assert_eq!(line.input.as_str(), arguments);
        assert_eq!((service.guidance.calls(), service.corpus.calls()), (1, 0));
    }

    #[tokio::test]
    async fn a_missing_page_is_audited_as_not_found() {
        let (service, log) = service_with_guidance();
        service.guidance.push_get(Err(GuidanceError::NotFound));
        let error = service.get_service_public(call("{}", "F31901"), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(
            error.message(),
            "No Service-Public sheet or folder F31901 for particuliers. Try audience entreprendre, \
             or search with fr_search_service_public."
        );
        let records = log.records();
        assert_eq!(
            (records[0].status, records[0].error_code),
            (AuditStatus::Error, Some(ErrorCode::NotFound))
        );
    }

    #[tokio::test]
    async fn a_rejected_input_is_audited_as_invalid_arg_without_reading() {
        let (service, log) = service_with_guidance();
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.get_service_public(rejected, pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("foo: unknown field `foo`"));
        assert_eq!(log.records()[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(service.guidance.calls(), 0);
    }

    /// Une façade assemblée par `LegalService::new` n'a pas de source de Service-Public : l'outil
    /// répond `config_error`, l'appel est audité, et rien n'est demandé au corpus.
    #[tokio::test]
    async fn without_a_guidance_source_the_tool_is_a_config_error() {
        let (service, log) = service_without_doctrine();
        let error = service.get_service_public(call("{}", "F1437"), pending()).await.unwrap_err();
        assert_eq!(error, AppError::config(messages::GUIDANCE_DISABLED_MESSAGE));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_SERVICE_PUBLIC);
        assert_eq!(records[0].error_code, Some(ErrorCode::ConfigError));
        assert_eq!(service.corpus.calls(), 0);
    }
}
