//! Méthode de façade `LegalService::get_act` pour `fr_get_act` (v0.1).
//!
//! Le motif gelé de `service/mod.rs` : l'exécution complète (décodage, validation, appel au port)
//! tourne sous `Audited::run`, avec `ToolName::FR_GET_ACT`.

use std::future::Future;

use super::LegalService;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetActInput, ToolCall},
    output::get_act::GetActOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::get_act::GetActUseCase,
};

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_get_act` : un texte consolidé et une page de sa table des matières. `cancel` se
    /// résout quand le client annule.
    ///
    /// # Errors
    ///
    /// Une [`AppError`] par échec de l'appel, dont `invalid_arg` pour un décodage ou une
    /// validation refusés ; l'issue est auditée dans tous les cas.
    pub async fn get_act(
        &self,
        call: ToolCall<GetActInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetActOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_GET_ACT, self.clock.now_utc(), call.canonical);
        self.audited
            .run(meta, cancel, async {
                let input = call.input?;
                GetActUseCase::new(&self.corpus, &self.clock).execute(input).await
            })
            .await
    }
}

#[cfg(test)]
pub(super) mod fixtures {
    //! Façade de test partagée par `fr_get_act` et `fr_get_convention` : le test garde une poignée
    //! sur le journal d'audit que la façade possède.

    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        ports::{AuditError, AuditLog, AuditRecord, PisteEnvironment},
        service::{ConfigurationState, LegalService, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };
    use chrono::DateTime;
    use lf_domain::date::CivilDate;
    use std::{sync::Arc, time::Duration};

    /// Journal partagé : la façade écrit, le test relit.
    pub(in crate::service) struct SharedAudit(pub(in crate::service) Arc<MemoryAuditLog>);

    impl AuditLog for SharedAudit {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    /// Façade stricte de 60 s, un 29 octobre 2026, avec la poignée sur son journal.
    pub(in crate::service) fn service(
        corpus: FakeCorpus,
    ) -> (LegalService<FakeCorpus, SharedAudit, FixedClock>, Arc<MemoryAuditLog>) {
        let audit = Arc::new(MemoryAuditLog::new());
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
        let clock = FixedClock::new(DateTime::UNIX_EPOCH, CivilDate::new(2026, 10, 29).unwrap());
        let service = LegalService::new(corpus, SharedAudit(Arc::clone(&audit)), clock, settings);
        (service, audit)
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::service;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, GetActInput, InputError, ToolCall},
        ports::{AuditStatus, CorpusError},
        testing::FakeCorpus,
        tools::ToolName,
        usecase::get_act::fixtures::{act, toc},
    };
    use pretty_assertions::assert_eq;
    use std::future::pending;

    fn call(input: GetActInput) -> ToolCall<GetActInput> {
        ToolCall { input: Ok(input), canonical: CanonicalInput::new("{\"text_id\":\"x\"}") }
    }

    fn input(text_id: &str) -> GetActInput {
        GetActInput { text_id: Some(text_id.to_owned()), ..GetActInput::default() }
    }

    #[tokio::test]
    async fn get_act_writes_one_audit_line_with_the_toc_count() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Ok(act("LOI", toc(2, 3, "Titre"))));
        let (service, audit) = service(corpus);
        let wanted = GetActInput { toc_limit: Some(5), ..input("LEGITEXT000033205014") };

        let output = service.get_act(call(wanted), pending()).await.unwrap();

        assert_eq!(output.toc.len(), 5);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool, ToolName::FR_GET_ACT);
        assert_eq!(lines[0].status, AuditStatus::Ok);
        assert_eq!(lines[0].output_count_or_size, 5);
        assert_eq!(lines[0].input.as_str(), "{\"text_id\":\"x\"}");
    }

    #[tokio::test]
    async fn get_act_decoding_failure_is_audited_as_invalid_arg_without_network() {
        let corpus = FakeCorpus::new();
        let (service, audit) = service(corpus);
        let rejected = ToolCall {
            input: Err(InputError {
                message: "toc_limit: invalid type: string \"x\", expected i64".to_owned(),
            }),
            canonical: CanonicalInput::new("{\"toc_limit\":\"x\"}"),
        };

        let error = service.get_act(rejected, pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error.message(), "toc_limit: invalid type: string \"x\", expected i64");
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].status, AuditStatus::Error);
        assert_eq!(lines[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(lines[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn get_act_not_found_is_audited_with_the_tool_message() {
        let corpus = FakeCorpus::new();
        corpus.push_get_act(Err(CorpusError::NotFound { what: "x".to_owned() }));
        let (service, audit) = service(corpus);

        let error =
            service.get_act(call(input("LEGITEXT000033205014")), pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::NotFound));
    }
}
