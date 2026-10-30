//! Méthodes de façade `LegalService::search_all` et `search_all_with_progress` pour
//! `fr_search_all` (v0.3, M7, 05 §5.13).
//!
//! Elles suivent le motif gelé de `service/mod.rs` : l'identité de l'appel (outil, heure,
//! arguments canoniques) est posée avant tout, puis le décodage, la validation et le cas d'usage
//! s'exécutent ensemble sous [`Audited::run`](crate::audited::Audited::run). `search_all` est la
//! forme gelée ; `search_all_with_progress` y ajoute un rappel appelé à la fin de chaque fond,
//! pour la progression optionnelle de l'adaptateur (05 §2.4). Le rappel est consommé avec la
//! méthode : quand elle rend la main, il n'existe plus, ce qui ferme proprement le canal qu'il
//! alimente.

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{SearchAllInput, ToolCall},
    output::search_all::SearchAllOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::search_all::SearchAllUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_search_all` : recherche dans plusieurs fonds à la fois (05 §5.13), auditée sous
    /// `fr_search_all` (métrique : somme des résultats rendus).
    ///
    /// `cancel` se résout quand le client annule l'appel ; toutes les recherches en vol sont alors
    /// abandonnées et l'appel est audité comme annulé.
    ///
    /// # Errors
    ///
    /// L'erreur du cas d'usage (`invalid_arg` pour un décodage ou une validation refusés, l'erreur
    /// commune quand tous les fonds échouent avec le même code), `upstream_error` au-delà de
    /// l'échéance, `audit_error` en mode strict quand la ligne ne peut pas être écrite.
    pub async fn search_all(
        &self,
        call: ToolCall<SearchAllInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<SearchAllOutput, AppError> {
        self.search_all_with_progress(call, cancel, |_, _| {}).await
    }

    /// Comme [`search_all`](Self::search_all), avec `progress(faits, total)` appelé à la fin de
    /// chaque fond, dans l'ordre d'arrivée.
    ///
    /// # Errors
    ///
    /// Celles de [`search_all`](Self::search_all).
    pub async fn search_all_with_progress(
        &self,
        call: ToolCall<SearchAllInput>,
        cancel: impl Future<Output = ()> + Send,
        progress: impl FnMut(u32, u32) + Send,
    ) -> Result<SearchAllOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_SEARCH_ALL, self.clock.now_utc(), call.canonical);
        let input = call.input;
        self.audited
            .run(meta, cancel, async {
                let input = input.map_err(AppError::from)?;
                SearchAllUseCase::new(&self.corpus, &self.clock).execute(input, progress).await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, future::ready, sync::Arc, sync::Mutex, time::Duration};

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

    /// Un service sur `corpus`, et le journal d'audit qu'il alimente.
    fn rig(
        corpus: FakeCorpus,
    ) -> (LegalService<FakeCorpus, SharedLog, FixedClock>, Arc<MemoryAuditLog>) {
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
        let service = LegalService::new(corpus, SharedLog(Arc::clone(&audit)), clock, settings);
        (service, audit)
    }

    fn page(fond: Fond, hits: u32) -> SearchPage {
        let hits = (1..=hits)
            .map(|n| {
                let id = DocumentId::parse(&format!("{}{n:012}", fond.hit_prefix())).unwrap();
                SearchHit::new(fond, id, None).unwrap()
            })
            .collect();
        SearchPage::new(fond, "bail", Some(40), None, hits, 0, PaginationMode::Defaut)
    }

    fn call(fonds: &[&str]) -> ToolCall<SearchAllInput> {
        let input = SearchAllInput {
            query: Some("bail".to_owned()),
            fonds: Some(fonds.iter().map(|fond| (*fond).to_owned()).collect()),
            per_fond: None,
        };
        ToolCall { input: Ok(input), canonical: CanonicalInput::new("{\"query\":\"bail\"}") }
    }

    #[tokio::test]
    async fn search_all_is_audited_under_its_name_with_the_sum_of_the_hits() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page(Fond::Loda, 3)));
        corpus.push_search(Ok(page(Fond::Juri, 2)));
        let (service, audit) = rig(corpus);

        let output = service.search_all(call(&["LODA", "JURI"]), pending()).await.unwrap();
        assert_eq!(output.groups.len(), 2);

        let records = audit.records();
        assert_eq!(records.len(), 1, "une seule ligne par appel");
        let record = &records[0];
        assert_eq!(record.tool, ToolName::FR_SEARCH_ALL);
        assert_eq!(record.status, AuditStatus::Ok);
        assert_eq!(record.output_count_or_size, 5, "la somme des hits rendus");
        assert_eq!(record.input.as_str(), "{\"query\":\"bail\"}");
        assert_eq!(record.ts, Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap());
        assert_eq!((record.error_code, record.error.as_deref()), (None, None));
        assert_eq!(record.server_version, "9.9.9");
    }

    #[tokio::test]
    async fn the_progress_callback_hears_each_fond_finish() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page(Fond::Loda, 1)));
        corpus.push_search(Err(CorpusError::Timeout));
        let (service, _audit) = rig(corpus);
        let seen = Mutex::new(Vec::new());

        service
            .search_all_with_progress(call(&["LODA", "JURI"]), pending(), |done, total| {
                seen.lock().unwrap().push((done, total));
            })
            .await
            .unwrap();

        assert_eq!(*seen.lock().unwrap(), [(1, 2), (2, 2)]);
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_calling_the_port() {
        let (service, audit) = rig(FakeCorpus::new());
        let failed = ToolCall::<SearchAllInput> {
            input: Err(InputError { message: "per_fond: invalid type: string".to_owned() }),
            canonical: CanonicalInput::new("{\"per_fond\":\"3\"}"),
        };
        let error = service.search_all(failed, pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error.message(), "per_fond: invalid type: string");
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_shared_failure_and_a_local_rejection_are_audited_with_their_codes() {
        let down = FakeCorpus::new();
        down.push_search(Err(CorpusError::Timeout));
        let (service, audit) = rig(down);
        let error = service.search_all(call(&["JURI"]), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::UpstreamError));

        let (service, audit) = rig(FakeCorpus::new());
        let error = service.search_all(call(&[]), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::InvalidArg));
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let (service, audit) = rig(FakeCorpus::new());
        let error = service.search_all(call(&["LODA"]), ready(())).await.unwrap_err();
        assert_eq!(error.code(), None);
        assert_eq!(error.cause(), Some(UpstreamCause::Cancelled));
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, None);
    }
}
