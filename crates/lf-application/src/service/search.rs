//! Méthode de façade `LegalService::search` pour `fr_search` (v0.1, 05 §5.1).
//!
//! La méthode suit le motif gelé de `service/mod.rs` : l'identité de l'appel (outil, heure,
//! arguments canoniques) est posée avant tout, puis le décodage, la validation et le cas d'usage
//! s'exécutent ensemble sous [`Audited::run`](crate::audited::Audited::run). Un rejet local ou un
//! échec de décodage laisse donc, comme tout le reste, exactement une ligne d'audit (D-02).

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{SearchInput, ToolCall},
    output::search::SearchOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::search::SearchUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_search` : recherche plein texte dans un fond (05 §5.1), auditée sous `fr_search`.
    ///
    /// `cancel` se résout quand le client annule l'appel ; l'appel est alors abandonné et audité
    /// comme annulé.
    ///
    /// # Errors
    ///
    /// L'erreur du cas d'usage (`invalid_arg` pour un décodage ou une validation refusés,
    /// erreurs du corpus traduites), `upstream_error` au-delà de l'échéance, `audit_error` en
    /// mode strict quand la ligne ne peut pas être écrite.
    pub async fn search(
        &self,
        call: ToolCall<SearchInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<SearchOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_SEARCH, self.clock.now_utc(), call.canonical);
        let input = call.input;
        self.audited
            .run(meta, cancel, async {
                let input = input.map_err(AppError::from)?;
                SearchUseCase::new(&self.corpus, &self.clock).execute(input).await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, future::ready, pin::Pin, sync::Arc, time::Duration};

    use chrono::{TimeZone, Utc};
    use lf_domain::{
        date::CivilDate,
        entity::{SearchHit, SearchPage},
        fond::{Fond, PaginationMode},
        id::DocumentId,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
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

    /// Futur d'annulation : un seul type pour tous les tests, donc une seule instanciation de
    /// `search` (et le corps de l'appel est exécuté dans celle-ci).
    type Cancel = Pin<Box<dyn Future<Output = ()> + Send>>;

    /// Le client n'annule jamais.
    fn never() -> Cancel {
        Box::pin(pending())
    }

    /// Le client a déjà annulé.
    fn cancelled() -> Cancel {
        Box::pin(ready(()))
    }

    struct Rig {
        service: LegalService<FakeCorpus, SharedLog, FixedClock>,
        audit: Arc<MemoryAuditLog>,
    }

    fn rig() -> Rig {
        let audit = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "9.9.9",
            },
            configuration: ConfigurationState::default(),
            budget: ResponseBudget,
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap();
        let clock = FixedClock::new(now, CivilDate::new(2026, 10, 29).unwrap());
        let service =
            LegalService::new(FakeCorpus::new(), SharedLog(Arc::clone(&audit)), clock, settings);
        Rig { service, audit }
    }

    fn call(query: Option<&str>) -> ToolCall<SearchInput> {
        let input = SearchInput { query: query.map(str::to_owned), ..SearchInput::default() };
        ToolCall { input: Ok(input), canonical: CanonicalInput::new("{\"query\":\"bail\"}") }
    }

    fn juri_page(hits: u32) -> SearchPage {
        let hits = (1..=hits)
            .map(|n| {
                let id = DocumentId::parse(&format!("LEGITEXT{n:012}")).unwrap();
                SearchHit::new(Fond::Loda, id, None).unwrap()
            })
            .collect();
        SearchPage::new(Fond::Loda, "bail", Some(40), None, hits, 0, PaginationMode::Defaut)
    }

    #[tokio::test]
    async fn search_is_audited_under_fr_search_with_the_number_of_hits() {
        let rig = rig();
        rig.service.corpus.push_search(Ok(juri_page(3)));
        let output = rig.service.search(call(Some("bail")), never()).await.unwrap();
        assert_eq!(output.hits.len(), 3);
        assert_eq!(output.total, 40);

        let records = rig.audit.records();
        assert_eq!(records.len(), 1, "une seule ligne par appel");
        let record = &records[0];
        assert_eq!(record.tool, ToolName::FR_SEARCH);
        assert_eq!(record.status, AuditStatus::Ok);
        assert_eq!(record.output_count_or_size, 3, "les hits renvoyés, pas le total");
        assert_eq!(record.input.as_str(), "{\"query\":\"bail\"}");
        assert_eq!(record.ts, Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap());
        assert_eq!((record.error_code, record.error.as_deref()), (None, None));
        assert_eq!(record.server_version, "9.9.9");
        assert_eq!(rig.service.corpus.calls_of("search"), 1);
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_calling_the_port() {
        let rig = rig();
        let failed = ToolCall::<SearchInput> {
            input: Err(InputError {
                message: "query: invalid type: integer `42`, expected a string".to_owned(),
            }),
            canonical: CanonicalInput::new("{\"query\":42}"),
        };
        let error = rig.service.search(failed, never()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error.message(), "query: invalid type: integer `42`, expected a string");

        let records = rig.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].output_count_or_size, 0);
        assert_eq!(records[0].input.as_str(), "{\"query\":42}", "l'empreinte porte sur le brut");
        assert_eq!(rig.service.corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_local_rejection_is_audited_as_invalid_arg() {
        let rig = rig();
        let error = rig.service.search(call(None), never()).await.unwrap_err();
        assert_eq!(error.message(), "query is required.");
        let records = rig.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].error.as_deref(), Some("query is required."));
        assert_eq!(rig.service.corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_port_failure_is_audited_with_its_code() {
        let rig = rig();
        rig.service.corpus.push_search(Err(CorpusError::Timeout));
        let error = rig.service.search(call(Some("bail")), never()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        let records = rig.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::UpstreamError));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let rig = rig();
        let error = rig.service.search(call(Some("bail")), cancelled()).await.unwrap_err();
        assert_eq!(error.code(), None);
        assert_eq!(error.cause(), Some(UpstreamCause::Cancelled));
        let records = rig.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, None);
        assert_eq!(rig.service.corpus.calls(), 0, "annulé avant le premier sondage du port");
    }
}
