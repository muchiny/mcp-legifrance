//! Méthode de façade `LegalService::coverage` pour `fr_coverage` (v0.1).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_COVERAGE`. L'empreinte d'audit est celle des arguments bruts, `{}` pour un appel
//! valide (02 §6.2) ; un argument quelconque est refusé au décodage, et cet `invalid_arg` est
//! audité comme les autres.

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{CoverageInput, ToolCall},
    output::coverage::CoverageOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::coverage::CoverageUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_coverage` : ce que le serveur couvre, son environnement et l'état de sa configuration,
    /// sans appel au port (05 §5.8). Métrique d'audit : le nombre de lacunes.
    ///
    /// # Errors
    ///
    /// `invalid_arg` si l'appel porte un argument (rejet au décodage), le dépassement de
    /// l'échéance globale, et `audit_error` en mode strict.
    pub async fn coverage(
        &self,
        call: ToolCall<CoverageInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<CoverageOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_COVERAGE, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = CoverageUseCase::new(&self.settings, &self.clock);
        self.audited.run(meta, cancel, async move { Ok(use_case.execute(&input?)) }).await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc, time::Duration};

    use chrono::DateTime;
    use lf_domain::{coverage::CoverageStatus, date::CivilDate};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        error::ErrorCode,
        input::{CanonicalInput, InputError},
        ports::{AuditError, AuditRecord, AuditStatus, PisteEnvironment},
        service::{ConfigurationState, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };

    /// Journal partagé : le service possède le sien, le test en garde une poignée.
    struct SharedAudit(Arc<MemoryAuditLog>);

    impl AuditLog for SharedAudit {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    fn service(
        configuration: ConfigurationState,
    ) -> (LegalService<FakeCorpus, SharedAudit, FixedClock>, Arc<MemoryAuditLog>) {
        let log = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "0.0.0",
            },
            configuration,
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

    fn call() -> ToolCall<CoverageInput> {
        ToolCall { input: Ok(CoverageInput), canonical: CanonicalInput::new("{}") }
    }

    #[tokio::test]
    async fn coverage_makes_no_port_call() {
        let configuration =
            ConfigurationState { credentials_configured: true, ..ConfigurationState::default() };
        let (service, log) = service(configuration);
        let output = service.coverage(call(), pending()).await.unwrap();
        assert_eq!(output.coverage.status(), CoverageStatus::Ok);
        assert_eq!(service.corpus.calls(), 0);
        let records = log.records();
        assert_eq!(records.len(), 1);
        let line = &records[0];
        assert_eq!(line.tool, ToolName::FR_COVERAGE);
        assert_eq!(line.status, AuditStatus::Ok);
        // Sandbox avec identifiants : neuf lacunes du catalogue et FR-101.
        assert_eq!(line.output_count_or_size, 10);
        assert_eq!(line.input.as_str(), "{}");
        assert_eq!(line.ts.timestamp(), 1_790_000_000);
        assert_eq!(output.consulted_on, CivilDate::new(2026, 10, 29).unwrap());
    }

    #[tokio::test]
    async fn coverage_rejects_any_argument() {
        // Le décodage de l'adapter refuse tout argument ; l'échec voyage dans `call.input` et la
        // façade l'audite comme un `invalid_arg`, sans appel au port.
        let (service, log) = service(ConfigurationState::default());
        let rejected = ToolCall {
            input: Err(InputError {
                message: "foo: unknown field `foo`, there are no fields".to_owned(),
            }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.coverage(rejected, pending()).await.unwrap_err();
        assert_eq!(
            error,
            AppError::invalid_argument("foo: unknown field `foo`, there are no fields")
        );
        assert_eq!(service.corpus.calls(), 0);
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].output_count_or_size, 0);
        assert_eq!(records[0].input.as_str(), r#"{"foo":1}"#);
    }

    #[tokio::test]
    async fn a_degraded_configuration_is_still_answered_and_audited() {
        // `fr_coverage` répond même sans identifiants : c'est l'outil qui l'explique.
        let (service, log) = service(ConfigurationState::default());
        let output = service.coverage(call(), pending()).await.unwrap();
        assert_eq!(output.coverage.status(), CoverageStatus::Degraded);
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Ok);
        // Sandbox sans identifiants : catalogue, FR-101 et FR-102.
        assert_eq!(records[0].output_count_or_size, 11);
    }
}
