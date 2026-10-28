//! Doublures de test des ports de `lf-application` (06 §2.2), derrière la feature `test-support`.
//!
//! [`FakeCorpus`] rejoue des réponses programmées, [`FixedClock`] fige l'heure, [`MemoryAuditLog`]
//! garde les lignes écrites et [`FailingAuditLog`] échoue toujours. Les doublures servent aux
//! tests de ce crate (`cfg(test)`) et, par la feature, aux dev-dependencies des autres membres :
//! `cargo xtask check-arch` (R9) interdit de l'activer en dépendance normale, aucun binaire ne les
//! embarque.
//!
//! Les verrous sont des `std::sync::Mutex`, jamais tenus à travers un `.await` : une section
//! critique est courte et synchrone. Un verrou empoisonné (un test qui a paniqué) se reprend tel
//! quel plutôt que de faire paniquer un deuxième test.

use std::{
    collections::VecDeque,
    future::{Future, poll_fn, ready},
    sync::{
        Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Poll, Waker},
    time::Duration,
};

use chrono::{DateTime, Utc};
use lf_domain::{
    date::{CivilDate, VersionDate},
    entity::{Act, ArticleText, CompanyAgreement, Convention, Decision, Deliberation, SearchPage},
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::SearchQuery,
};

use crate::{
    deadline::Deadline,
    ports::{
        AuditError, AuditErrorKind, AuditLog, AuditRecord, Clock, ConfigKind, CorpusError,
        LegalCorpus,
    },
};

/// Durée relevée par [`FakeCorpus::seen_deadlines`] quand aucune échéance n'est publiée : « sans
/// limite », pour la distinguer d'une échéance réelle.
pub const NO_DEADLINE: Duration = Duration::MAX;

/// Prend un verrou sans `unwrap` : un verrou empoisonné rend ses données, déjà cohérentes ici
/// (chaque section critique est une seule opération sur la collection).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// File de réponses d'une méthode du port.
type Responses<T> = Mutex<VecDeque<Result<T, CorpusError>>>;

/// Libère à sa chute l'emplacement pris dans le compteur d'appels en vol, y compris quand le
/// futur est abandonné (annulation, délai d'`Audited`).
struct InFlight<'a>(&'a AtomicUsize);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Faux corpus : une file de réponses par méthode du port, rejouées dans l'ordre.
///
/// Une file vide rend `Config { kind: Settings, .. }` qui nomme la méthode : un test qui oublie de
/// programmer une réponse échoue avec un message lisible plutôt que de bloquer. Le délai
/// optionnel utilise `tokio::time::sleep`, donc le temps virtuel des tests `start_paused`.
#[derive(Default)]
pub struct FakeCorpus {
    search: Responses<SearchPage>,
    get_act: Responses<Act>,
    get_article: Responses<ArticleText>,
    get_decision: Responses<Decision>,
    get_deliberation: Responses<Deliberation>,
    get_convention: Responses<Convention>,
    get_company_agreement: Responses<CompanyAgreement>,
    delay: Option<Duration>,
    /// Nom de chaque méthode appelée, dans l'ordre des appels.
    calls: Mutex<Vec<&'static str>>,
    /// Durée restante de l'échéance publiée, relevée à chaque appel.
    deadlines: Mutex<Vec<Duration>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl FakeCorpus {
    /// Faux corpus sans réponse programmée ni délai.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Chaque appel attend `delay` (temps de `tokio`) avant de répondre.
    #[must_use]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Programme la réponse suivante de `search`.
    pub fn push_search(&self, response: Result<SearchPage, CorpusError>) {
        lock(&self.search).push_back(response);
    }

    /// Programme la réponse suivante de `get_act`.
    pub fn push_get_act(&self, response: Result<Act, CorpusError>) {
        lock(&self.get_act).push_back(response);
    }

    /// Programme la réponse suivante de `get_article`.
    pub fn push_get_article(&self, response: Result<ArticleText, CorpusError>) {
        lock(&self.get_article).push_back(response);
    }

    /// Programme la réponse suivante de `get_decision`.
    pub fn push_get_decision(&self, response: Result<Decision, CorpusError>) {
        lock(&self.get_decision).push_back(response);
    }

    /// Programme la réponse suivante de `get_deliberation`.
    pub fn push_get_deliberation(&self, response: Result<Deliberation, CorpusError>) {
        lock(&self.get_deliberation).push_back(response);
    }

    /// Programme la réponse suivante de `get_convention`.
    pub fn push_get_convention(&self, response: Result<Convention, CorpusError>) {
        lock(&self.get_convention).push_back(response);
    }

    /// Programme la réponse suivante de `get_company_agreement`.
    pub fn push_get_company_agreement(&self, response: Result<CompanyAgreement, CorpusError>) {
        lock(&self.get_company_agreement).push_back(response);
    }

    /// Nombre total d'appels reçus, toutes méthodes confondues.
    #[must_use]
    pub fn calls(&self) -> usize {
        lock(&self.calls).len()
    }

    /// Nombre d'appels reçus par la méthode `method` (nom du port : `"search"`, `"get_act"`, ...).
    #[must_use]
    pub fn calls_of(&self, method: &str) -> usize {
        lock(&self.calls).iter().filter(|called| **called == method).count()
    }

    /// Plus grand nombre d'appels simultanément en vol depuis la création.
    #[must_use]
    pub fn max_concurrency(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }

    /// Durée restante de l'échéance publiée à chaque appel, dans l'ordre ([`NO_DEADLINE`] hors de
    /// tout `Deadline::scope`).
    #[must_use]
    pub fn seen_deadlines(&self) -> Vec<Duration> {
        lock(&self.deadlines).clone()
    }

    /// Cœur commun des sept méthodes : note l'appel et l'échéance, tient un emplacement en vol
    /// pendant le délai, puis sert la réponse suivante de la file.
    async fn serve<T>(
        &self,
        method: &'static str,
        responses: &Responses<T>,
    ) -> Result<T, CorpusError> {
        lock(&self.calls).push(method);
        lock(&self.deadlines).push(Deadline::current_or(NO_DEADLINE));
        let now_in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        let _slot = InFlight(&self.in_flight);
        self.max_in_flight.fetch_max(now_in_flight, Ordering::SeqCst);
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        // Le verrou est pris et rendu dans la même instruction : jamais tenu à travers un `await`.
        let next = lock(responses).pop_front();
        next.unwrap_or_else(|| {
            Err(CorpusError::Config {
                kind: ConfigKind::Settings,
                detail: format!("FakeCorpus: no response programmed for {method}"),
            })
        })
    }
}

impl LegalCorpus for FakeCorpus {
    async fn search(&self, _: &SearchQuery) -> Result<SearchPage, CorpusError> {
        self.serve("search", &self.search).await
    }

    async fn get_act(&self, _: &LegiTextId, _: VersionDate) -> Result<Act, CorpusError> {
        self.serve("get_act", &self.get_act).await
    }

    async fn get_article(&self, _: &ArticleId) -> Result<ArticleText, CorpusError> {
        self.serve("get_article", &self.get_article).await
    }

    async fn get_decision(&self, _: &DecisionId) -> Result<Decision, CorpusError> {
        self.serve("get_decision", &self.get_decision).await
    }

    async fn get_deliberation(&self, _: &CnilTextId) -> Result<Deliberation, CorpusError> {
        self.serve("get_deliberation", &self.get_deliberation).await
    }

    async fn get_convention(&self, _: &KaliTextId) -> Result<Convention, CorpusError> {
        self.serve("get_convention", &self.get_convention).await
    }

    async fn get_company_agreement(&self, _: &AccoTextId) -> Result<CompanyAgreement, CorpusError> {
        self.serve("get_company_agreement", &self.get_company_agreement).await
    }
}

/// Horloge figée : reçoit ses deux valeurs, pour que `lf-application` n'ait pas besoin de
/// `chrono-tz` (R2, 02 §5.3).
#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    now_utc: DateTime<Utc>,
    today_paris: CivilDate,
}

impl FixedClock {
    /// Horloge qui rend toujours `now_utc` et `today_paris`.
    #[must_use]
    pub fn new(now_utc: DateTime<Utc>, today_paris: CivilDate) -> Self {
        Self { now_utc, today_paris }
    }
}

impl Clock for FixedClock {
    fn now_utc(&self) -> DateTime<Utc> {
        self.now_utc
    }

    fn today_paris(&self) -> CivilDate {
        self.today_paris
    }
}

/// État de la porte d'un [`MemoryAuditLog`] : ouverte ou non, et les réveils des écritures qui
/// attendent.
#[derive(Default)]
struct GateState {
    open: bool,
    waiters: Vec<Waker>,
}

/// Journal d'audit en mémoire : garde les lignes dans l'ordre d'écriture.
///
/// Avec [`MemoryAuditLog::with_gate`], `record` reste en attente jusqu'à [`MemoryAuditLog::open_gate`] :
/// un test d'`Audited` observe ainsi une écriture en vol (la ligne n'est gardée qu'à la fin de
/// l'écriture, comme l'accusé de D-16). La porte est écrite à la main (verrou et réveils) car la
/// feature `sync` de `tokio` n'est pas admise dans `lf-application` (R2).
#[derive(Default)]
pub struct MemoryAuditLog {
    records: Mutex<Vec<AuditRecord>>,
    gate: Option<Mutex<GateState>>,
}

impl MemoryAuditLog {
    /// Journal sans porte : chaque écriture aboutit tout de suite.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Journal dont les écritures attendent [`MemoryAuditLog::open_gate`].
    #[must_use]
    pub fn with_gate() -> Self {
        Self { records: Mutex::default(), gate: Some(Mutex::default()) }
    }

    /// Ouvre la porte (définitivement) et réveille les écritures en attente ; sans effet sur un
    /// journal sans porte.
    pub fn open_gate(&self) {
        let Some(gate) = &self.gate else { return };
        let waiters = {
            let mut state = lock(gate);
            state.open = true;
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            waiter.wake();
        }
    }

    /// Lignes écrites jusqu'ici, dans l'ordre.
    #[must_use]
    pub fn records(&self) -> Vec<AuditRecord> {
        lock(&self.records).clone()
    }

    /// Attend l'ouverture de la porte ; rend la main tout de suite sans porte ou si elle est
    /// déjà ouverte.
    async fn wait_gate(&self) {
        let Some(gate) = &self.gate else { return };
        poll_fn(|cx| {
            let mut state = lock(gate);
            if state.open {
                Poll::Ready(())
            } else {
                state.waiters.push(cx.waker().clone());
                Poll::Pending
            }
        })
        .await;
    }
}

impl AuditLog for MemoryAuditLog {
    async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        self.wait_gate().await;
        lock(&self.records).push(record.clone());
        Ok(())
    }
}

/// Journal d'audit qui échoue toujours, avec le genre demandé (D-16 : l'échec d'écriture).
#[derive(Debug, Clone, Copy)]
pub struct FailingAuditLog {
    kind: AuditErrorKind,
}

impl FailingAuditLog {
    /// Journal dont chaque écriture échoue avec `kind`.
    #[must_use]
    pub fn new(kind: AuditErrorKind) -> Self {
        Self { kind }
    }
}

impl AuditLog for FailingAuditLog {
    fn record(&self, _: &AuditRecord) -> impl Future<Output = Result<(), AuditError>> + Send {
        ready(Err(AuditError {
            kind: self.kind,
            detail: format!("FailingAuditLog: simulated {:?} failure", self.kind),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        input::CanonicalInput,
        ports::{AuditStatus, PisteEnvironment},
        tools::ToolName,
    };
    use lf_domain::{
        date::CivilDate,
        fond::{Fond, PaginationMode},
        search::{MatchMode, PageSize, QueryText},
    };
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, pin::pin, task::Context};
    use tokio::time::Instant;

    fn query() -> SearchQuery {
        SearchQuery::new(
            Fond::Juri,
            QueryText::parse("bail").unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            None,
        )
        .unwrap()
    }

    fn page(total: u64) -> SearchPage {
        SearchPage::new(Fond::Juri, "bail", Some(total), None, vec![], 0, PaginationMode::Defaut)
    }

    fn article_id() -> ArticleId {
        "LEGIARTI000006419288".parse().unwrap()
    }

    fn timeout() -> CorpusError {
        CorpusError::Timeout
    }

    fn record(tool: ToolName, duration_ms: u64) -> AuditRecord {
        AuditRecord {
            ts: DateTime::UNIX_EPOCH,
            tool,
            input: CanonicalInput::new("{}"),
            output_count_or_size: 0,
            duration_ms,
            status: AuditStatus::Ok,
            error_code: None,
            error: None,
            env: PisteEnvironment::Sandbox,
            server_version: "0.0.0",
        }
    }

    #[tokio::test]
    async fn fake_corpus_returns_programmed_responses_in_order() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page(1)));
        corpus.push_search(Err(timeout()));
        corpus.push_search(Ok(page(3)));
        assert_eq!(corpus.search(&query()).await, Ok(page(1)));
        assert_eq!(corpus.search(&query()).await, Err(timeout()));
        assert_eq!(corpus.search(&query()).await, Ok(page(3)));
    }

    #[tokio::test]
    async fn fake_corpus_serves_each_method_from_its_own_queue() {
        let corpus = FakeCorpus::new();
        let not_found = |what: &str| CorpusError::NotFound { what: what.to_owned() };
        corpus.push_get_act(Err(not_found("act")));
        corpus.push_get_article(Err(not_found("article")));
        corpus.push_get_decision(Err(not_found("decision")));
        corpus.push_get_deliberation(Err(not_found("deliberation")));
        corpus.push_get_convention(Err(not_found("convention")));
        corpus.push_get_company_agreement(Err(not_found("agreement")));
        let act = "LEGITEXT000006070721".parse().unwrap();
        assert_eq!(corpus.get_act(&act, VersionDate::MIN).await, Err(not_found("act")));
        assert_eq!(corpus.get_article(&article_id()).await, Err(not_found("article")));
        let decision = "JURITEXT000037999394".parse().unwrap();
        assert_eq!(corpus.get_decision(&decision).await, Err(not_found("decision")));
        let deliberation = "CNILTEXT000053352643".parse().unwrap();
        assert_eq!(corpus.get_deliberation(&deliberation).await, Err(not_found("deliberation")));
        let convention = "KALITEXT000005652329".parse().unwrap();
        assert_eq!(corpus.get_convention(&convention).await, Err(not_found("convention")));
        let agreement = "ACCOTEXT000037731479".parse().unwrap();
        assert_eq!(corpus.get_company_agreement(&agreement).await, Err(not_found("agreement")));
    }

    #[tokio::test]
    async fn fake_corpus_counts_calls_per_method() {
        let corpus = FakeCorpus::new();
        assert_eq!((corpus.calls(), corpus.calls_of("search")), (0, 0));
        corpus.push_search(Ok(page(1)));
        let _ = corpus.search(&query()).await;
        let _ = corpus.get_article(&article_id()).await;
        let _ = corpus.get_article(&article_id()).await;
        assert_eq!(corpus.calls(), 3);
        assert_eq!(corpus.calls_of("search"), 1);
        assert_eq!(corpus.calls_of("get_article"), 2);
        assert_eq!(corpus.calls_of("get_act"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn fake_corpus_delay_uses_virtual_time() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_millis(200));
        corpus.push_search(Ok(page(1)));
        let start = Instant::now();
        assert_eq!(corpus.search(&query()).await, Ok(page(1)));
        assert_eq!(start.elapsed(), Duration::from_millis(200));
    }

    #[tokio::test(start_paused = true)]
    async fn fake_corpus_tracks_max_concurrency() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_millis(50));
        assert_eq!(corpus.max_concurrency(), 0);
        let (query, id) = (query(), article_id());
        let (first, second, third) =
            tokio::join!(corpus.search(&query), corpus.get_article(&id), corpus.get_article(&id),);
        assert!(first.is_err() && second.is_err() && third.is_err());
        assert_eq!(corpus.max_concurrency(), 3);
        // Les appels suivants, un par un, ne relèvent pas le maximum et ne le font pas baisser.
        let _ = corpus.search(&query).await;
        assert_eq!(corpus.max_concurrency(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn fake_corpus_releases_its_slot_when_a_call_is_dropped() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_secs(60));
        let query = query();
        // Le délai d'attente abandonne l'appel en vol à 1 s : son emplacement doit être rendu.
        let abandoned = tokio::time::timeout(Duration::from_secs(1), corpus.search(&query)).await;
        assert!(abandoned.is_err());
        assert_eq!(corpus.max_concurrency(), 1);
        let (_, _) = tokio::join!(corpus.search(&query), corpus.search(&query));
        // Deux appels simultanés, et non trois : l'abandonné ne compte plus.
        assert_eq!(corpus.max_concurrency(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn fake_corpus_reports_remaining_deadline() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_secs(5));
        let _ =
            Deadline::after(Duration::from_secs(30)).scope(corpus.get_article(&article_id())).await;
        let _ = corpus.get_article(&article_id()).await;
        assert_eq!(corpus.seen_deadlines(), vec![Duration::from_secs(30), NO_DEADLINE]);
    }

    #[tokio::test]
    async fn empty_queue_is_a_config_error_naming_the_method() {
        let corpus = FakeCorpus::new();
        assert_eq!(
            corpus.search(&query()).await,
            Err(CorpusError::Config {
                kind: ConfigKind::Settings,
                detail: "FakeCorpus: no response programmed for search".to_owned(),
            })
        );
        assert_eq!(
            corpus.get_article(&article_id()).await,
            Err(CorpusError::Config {
                kind: ConfigKind::Settings,
                detail: "FakeCorpus: no response programmed for get_article".to_owned(),
            })
        );
    }

    #[test]
    fn fixed_clock_returns_both_values() {
        let today = CivilDate::new(2026, 10, 28).unwrap();
        let clock = FixedClock::new(DateTime::UNIX_EPOCH, today);
        assert_eq!(clock.now_utc(), DateTime::UNIX_EPOCH);
        assert_eq!(clock.today_paris(), today);
    }

    #[tokio::test]
    async fn memory_audit_log_keeps_records_in_order() {
        let log = MemoryAuditLog::new();
        assert!(log.records().is_empty());
        log.record(&record(ToolName::FR_SEARCH, 1)).await.unwrap();
        log.record(&record(ToolName::FR_GET_ACT, 2)).await.unwrap();
        let kept: Vec<_> = log.records().iter().map(|r| (r.tool, r.duration_ms)).collect();
        assert_eq!(kept, vec![(ToolName::FR_SEARCH, 1), (ToolName::FR_GET_ACT, 2)]);
        // Ouvrir une porte qui n'existe pas ne change rien.
        log.open_gate();
        assert_eq!(log.records().len(), 2);
    }

    #[tokio::test]
    async fn gated_audit_log_blocks_until_opened() {
        let log = MemoryAuditLog::with_gate();
        let entry = record(ToolName::FR_SEARCH, 7);
        let mut first = pin!(log.record(&entry));
        let mut second = pin!(log.record(&entry));
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        assert!(log.records().is_empty());
        log.open_gate();
        assert!(first.as_mut().poll(&mut cx).is_ready());
        assert!(second.as_mut().poll(&mut cx).is_ready());
        assert_eq!(log.records().len(), 2);
        // Une fois ouverte, la porte reste ouverte.
        log.record(&entry).await.unwrap();
        assert_eq!(log.records().len(), 3);
    }

    #[tokio::test]
    async fn gated_audit_log_wakes_a_waiting_task() {
        let log = std::sync::Arc::new(MemoryAuditLog::with_gate());
        let writer = {
            let log = std::sync::Arc::clone(&log);
            tokio::spawn(async move { log.record(&record(ToolName::FR_SEARCH, 1)).await })
        };
        tokio::task::yield_now().await;
        assert!(log.records().is_empty());
        log.open_gate();
        writer.await.unwrap().unwrap();
        assert_eq!(log.records().len(), 1);
    }

    #[tokio::test]
    async fn failing_audit_log_fails_with_each_kind() {
        for kind in [
            AuditErrorKind::Io,
            AuditErrorKind::DiskFull,
            AuditErrorKind::PermissionDenied,
            AuditErrorKind::Encode,
            AuditErrorKind::WriterGone,
        ] {
            let error = FailingAuditLog::new(kind).record(&record(ToolName::FR_SEARCH, 1)).await;
            let error = error.unwrap_err();
            assert_eq!(error.kind, kind);
            assert_eq!(error.detail, format!("FailingAuditLog: simulated {kind:?} failure"));
        }
    }
}
