//! Méthode de façade `LegalService::complete` (v0.3, M7).
//!
//! Exception au motif des outils : la signature est `complete(&self, input: CompleteInput) ->
//! CompleteOutput`. La complétion n'est pas auditée (00 §7) et rend une liste vide en cas
//! d'erreur ; `complete_gate` tient son débit (10 par seconde et par processus, D13).

use super::LegalService;
use crate::{
    input::CompleteInput,
    output::complete::CompleteOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    usecase::complete::CompleteUseCase,
};

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// Complète la variable `code` du gabarit `legifrance://code/{code}/article/{num}` : au plus
    /// 100 slugs de codes, préfixes d'abord. Jamais d'erreur ni de ligne d'audit : une variable
    /// autre que `code`, un débit dépassé (10 par seconde pour tout le processus), une erreur du
    /// port ou un délai donnent la liste vide.
    pub async fn complete(&self, input: CompleteInput) -> CompleteOutput {
        let deadline = self.settings.policy.deadline;
        CompleteUseCase::new(&self.corpus, &self.complete_gate, deadline).execute(input).await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc, time::Duration};

    use chrono::DateTime;
    use lf_domain::{date::CivilDate, entity::CodeListing, id::LegiTextId, status::LegalStatus};
    use pretty_assertions::assert_eq;

    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        input::{CanonicalInput, CompleteInput, CoverageInput, ToolCall},
        ports::{AuditError, AuditLog, AuditRecord, CorpusError, PisteEnvironment},
        service::{ConfigurationState, LegalService, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };

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

    fn codes() -> Vec<CodeListing> {
        ["Code civil", "Code du travail"]
            .into_iter()
            .map(|title| CodeListing {
                id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
                title: title.to_owned(),
                etat: Some(LegalStatus::Vigueur),
            })
            .collect()
    }

    fn typed(value: &str) -> CompleteInput {
        CompleteInput { argument: Some("code".to_owned()), value: Some(value.to_owned()) }
    }

    #[tokio::test]
    async fn complete_proposes_codes_and_writes_no_audit_line() {
        let (service, audit) = service();
        service.corpus.push_list_codes(Ok(codes()));
        // Témoin : un outil écrit bien une ligne dans ce journal.
        let call = ToolCall { input: Ok(CoverageInput), canonical: CanonicalInput::new("{}") };
        service.coverage(call, pending()).await.unwrap();
        assert_eq!(audit.records().len(), 1);

        let output = service.complete(typed("code d")).await;

        assert_eq!(output.values, ["code-du-travail"]);
        assert_eq!(output.total, Some(1));
        assert_eq!(service.corpus.calls_of("list_codes"), 1);
        // La complétion n'a rien ajouté.
        assert_eq!(audit.records().len(), 1);
    }

    #[tokio::test]
    async fn complete_answers_an_empty_list_on_error_and_on_other_variables() {
        let (service, audit) = service();
        service.corpus.push_list_codes(Err(CorpusError::Timeout));

        assert_eq!(service.complete(typed("c")).await.values, Vec::<String>::new());
        let other = CompleteInput { argument: Some("num".to_owned()), value: Some("9".to_owned()) };
        assert_eq!(service.complete(other).await.values, Vec::<String>::new());
        assert_eq!(service.corpus.calls(), 1);
        assert_eq!(audit.records().len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_gate_is_shared_by_every_completion_of_the_service() {
        let (service, _audit) = service();
        for _ in 0..12 {
            service.corpus.push_list_codes(Ok(codes()));
        }

        let mut served = 0;
        for _ in 0..12 {
            if !service.complete(typed("")).await.values.is_empty() {
                served += 1;
            }
        }

        // Dix par seconde pour le processus, pas pour l'appelant.
        assert_eq!(served, 10);
        assert_eq!(service.corpus.calls_of("list_codes"), 10);
    }
}
