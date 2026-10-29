//! Méthode de façade `LegalService::verify_citations` pour `fr_verify_citations` (v0.2 (M6),
//! 05 §5.9).
//!
//! Elle suit le motif gelé de `service/mod.rs` : l'identité de l'appel (outil, heure, arguments
//! canoniques) est posée avant tout, puis le décodage, la validation et le cas d'usage s'exécutent
//! ensemble sous [`Audited::run`](crate::audited::Audited::run). L'échéance publiée par `Audited`
//! fonde le budget de 80 % du contrôle.

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{ToolCall, VerifyCitationsInput},
    output::verify_citations::VerifyCitationsOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::verify_citations::VerifyCitationsUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_verify_citations` : contrôle les citations d'un texte (05 §5.9), auditée sous
    /// `fr_verify_citations` (métrique : nombre de citations contrôlées).
    ///
    /// `cancel` se résout quand le client annule l'appel ; les résolutions en vol sont alors
    /// abandonnées et l'appel est audité comme annulé.
    ///
    /// # Errors
    ///
    /// L'erreur du cas d'usage (`invalid_arg` pour un décodage ou une validation refusés,
    /// l'erreur amont quand toutes les citations soumises au corpus échouent), `upstream_error`
    /// au-delà de l'échéance, `audit_error` en mode strict quand la ligne ne peut pas être
    /// écrite.
    pub async fn verify_citations(
        &self,
        call: ToolCall<VerifyCitationsInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<VerifyCitationsOutput, AppError> {
        let meta =
            CallMeta::new(ToolName::FR_VERIFY_CITATIONS, self.clock.now_utc(), call.canonical);
        let input = call.input;
        self.audited
            .run(meta, cancel, async {
                let input = input.map_err(AppError::from)?;
                let policy = &self.settings.policy;
                VerifyCitationsUseCase::new(&self.corpus, &self.clock, policy.env, policy.deadline)
                    .execute(input)
                    .await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, future::ready, sync::Arc, time::Duration};

    use chrono::{TimeZone, Utc};
    use lf_domain::{date::CivilDate, entity::CodeListing, id::LegiTextId, status::LegalStatus};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        citation::CheckStatus,
        error::{ErrorCode, UpstreamCause},
        input::{CanonicalInput, InputError},
        ports::{AuditError, AuditRecord, AuditStatus, CorpusError, PisteEnvironment},
        service::{ConfigurationState, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };

    /// Journal partagé : le service possède le sien, le test garde une poignée.
    struct SharedLog(Arc<MemoryAuditLog>);

    impl AuditLog for SharedLog {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    /// Un service sur `corpus` dans l'environnement `env`, et le journal d'audit qu'il alimente.
    fn rig(
        corpus: FakeCorpus,
        env: PisteEnvironment,
    ) -> (LegalService<FakeCorpus, SharedLog, FixedClock>, Arc<MemoryAuditLog>) {
        let audit = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env,
                server_version: "9.9.9",
            },
            configuration: ConfigurationState::default(),
            budget: ResponseBudget,
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap();
        let clock = FixedClock::new(now, CivilDate::new(2026, 10, 29).unwrap());
        let service = LegalService::new(corpus, SharedLog(Arc::clone(&audit)), clock, settings);
        (service, audit)
    }

    fn call(text: &str) -> ToolCall<VerifyCitationsInput> {
        let input = VerifyCitationsInput { text: Some(text.to_owned()), max_citations: None };
        ToolCall { input: Ok(input), canonical: CanonicalInput::new("{\"text\":\"…\"}") }
    }

    fn civil_code_listed(corpus: &FakeCorpus) {
        corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
    }

    #[tokio::test]
    async fn verify_citations_is_audited_under_its_name_with_the_number_checked() {
        let corpus = FakeCorpus::new();
        civil_code_listed(&corpus);
        corpus.push_find_article(Err(CorpusError::NotFound { what: "x".to_owned() }));
        let (service, audit) = rig(corpus, PisteEnvironment::Production);

        let text =
            "article 9999 du code civil ; la directive 85/374/CEE du Conseil du 25 juillet 1985";
        let output = service.verify_citations(call(text), pending()).await.unwrap();
        assert_eq!((output.found, output.checked), (2, 2));
        assert_eq!(output.citations[0].status, CheckStatus::NotFound);

        let records = audit.records();
        assert_eq!(records.len(), 1, "une seule ligne par appel");
        let record = &records[0];
        assert_eq!(record.tool, ToolName::FR_VERIFY_CITATIONS);
        assert_eq!(record.status, AuditStatus::Ok);
        assert_eq!(record.output_count_or_size, 2, "le nombre de citations contrôlées");
        assert_eq!(record.server_version, "9.9.9");
    }

    #[tokio::test]
    async fn the_environment_of_the_policy_decides_not_found_against_unresolvable() {
        let corpus = FakeCorpus::new();
        civil_code_listed(&corpus);
        corpus.push_find_article(Err(CorpusError::NotFound { what: "x".to_owned() }));
        let (service, _audit) = rig(corpus, PisteEnvironment::Sandbox);
        let output =
            service.verify_citations(call("article 9999 du code civil"), pending()).await.unwrap();
        assert_eq!(output.citations[0].status, CheckStatus::Unresolvable);
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_calling_the_port() {
        let (service, audit) = rig(FakeCorpus::new(), PisteEnvironment::Production);
        let failed = ToolCall::<VerifyCitationsInput> {
            input: Err(InputError { message: "max_citations: invalid type: string".to_owned() }),
            canonical: CanonicalInput::new("{\"max_citations\":\"3\"}"),
        };
        let error = service.verify_citations(failed, pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn an_upstream_failure_on_every_citation_is_audited_as_upstream_error() {
        let corpus = FakeCorpus::new();
        civil_code_listed(&corpus);
        corpus.push_find_article(Err(CorpusError::Timeout));
        let (service, audit) = rig(corpus, PisteEnvironment::Production);
        let error =
            service.verify_citations(call("article 9 du code civil"), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::UpstreamError));
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let (service, audit) = rig(FakeCorpus::new(), PisteEnvironment::Production);
        let error = service.verify_citations(call("article 9 du code civil"), ready(())).await;
        let error = error.unwrap_err();
        assert_eq!(error.code(), None);
        assert_eq!(error.cause(), Some(UpstreamCause::Cancelled));
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
    }
}
