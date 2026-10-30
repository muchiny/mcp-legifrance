//! Méthode de façade `LegalService::get_company_agreement` pour `fr_get_company_agreement` (v0.1).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_GET_COMPANY_AGREEMENT` : le décodage des arguments, la validation et l'appel au
//! port forment l'exécution complète auditée (D-02).

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetCompanyAgreementInput, ToolCall},
    output::get_company_agreement::GetCompanyAgreementOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::get_company_agreement::GetCompanyAgreementUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_get_company_agreement` : métadonnées d'un accord d'entreprise (05 §5.7). Métrique
    /// d'audit : 1.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, identifiant), `not_found`, les erreurs communes du port, le
    /// dépassement de l'échéance globale, et `audit_error` en mode strict.
    pub async fn get_company_agreement(
        &self,
        call: ToolCall<GetCompanyAgreementInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetCompanyAgreementOutput, AppError> {
        let meta =
            CallMeta::new(ToolName::FR_GET_COMPANY_AGREEMENT, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = GetCompanyAgreementUseCase::new(&self.corpus);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc, time::Duration};

    use chrono::DateTime;
    use lf_domain::{date::CivilDate, entity::CompanyAgreement, id::AccoTextId};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        error::ErrorCode,
        input::{CanonicalInput, InputError},
        ports::{AuditError, AuditRecord, AuditStatus, CorpusError, PisteEnvironment},
        service::{ConfigurationState, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };

    const ID: &str = "ACCOTEXT000053643828";

    /// Journal partagé : le service possède le sien, le test en garde une poignée.
    struct SharedAudit(Arc<MemoryAuditLog>);

    impl AuditLog for SharedAudit {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    fn service() -> (LegalService<FakeCorpus, SharedAudit, FixedClock>, Arc<MemoryAuditLog>) {
        let log = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "0.0.0",
            },
            configuration: ConfigurationState::default(),
            budget: ResponseBudget,
        };
        let clock = FixedClock::new(
            DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
            CivilDate::new(2026, 10, 29).unwrap(),
        );
        let service =
            LegalService::new(FakeCorpus::new(), SharedAudit(Arc::clone(&log)), clock, settings);
        (service, log)
    }

    fn call(arguments: &str, id: Option<&str>) -> ToolCall<GetCompanyAgreementInput> {
        ToolCall {
            input: Ok(GetCompanyAgreementInput { agreement_id: id.map(str::to_owned) }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    fn agreement() -> CompanyAgreement {
        CompanyAgreement {
            agreement_id: AccoTextId::parse(ID).unwrap(),
            nature: None,
            title: None,
            raison_sociale: None,
            siret: None,
            idcc: None,
            code_ape: None,
            secteur: None,
            themes: Vec::new(),
            syndicats: Vec::new(),
            date_texte: None,
            date_effet: None,
            date_depot: None,
        }
    }

    #[tokio::test]
    async fn a_read_is_audited_with_metric_one() {
        let (service, log) = service();
        service.corpus.push_get_company_agreement(Ok(agreement()));
        let arguments = format!(r#"{{"agreement_id":"{ID}"}}"#);
        let output =
            service.get_company_agreement(call(&arguments, Some(ID)), pending()).await.unwrap();
        assert_eq!(output.agreement, agreement());
        let records = log.records();
        assert_eq!(records.len(), 1);
        let line = &records[0];
        assert_eq!(line.tool, ToolName::FR_GET_COMPANY_AGREEMENT);
        assert_eq!(line.status, AuditStatus::Ok);
        assert_eq!(line.output_count_or_size, 1);
        assert_eq!(line.error_code, None);
        assert_eq!(line.input.as_str(), arguments);
        assert_eq!(line.ts.timestamp(), 1_790_000_000);
        assert_eq!(line.env, PisteEnvironment::Sandbox);
    }

    #[tokio::test]
    async fn an_unknown_agreement_is_audited_as_not_found() {
        let (service, log) = service();
        service
            .corpus
            .push_get_company_agreement(Err(CorpusError::NotFound { what: "acco".to_owned() }));
        let error =
            service.get_company_agreement(call("{}", Some(ID)), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_network() {
        let (service, log) = service();
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.get_company_agreement(rejected, pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("foo: unknown field `foo`"));
        assert_eq!(service.corpus.calls(), 0);
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].input.as_str(), r#"{"foo":1}"#);
    }

    #[tokio::test]
    async fn a_missing_id_is_audited_as_invalid_arg_without_network() {
        let (service, log) = service();
        let error = service.get_company_agreement(call("{}", None), pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("agreement_id is required."));
        assert_eq!(service.corpus.calls(), 0);
        assert_eq!(log.records().len(), 1);
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let (service, log) = service();
        let error = service
            .get_company_agreement(call("{}", Some(ID)), std::future::ready(()))
            .await
            .unwrap_err();
        assert_eq!(error.code(), None);
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, None);
    }
}
