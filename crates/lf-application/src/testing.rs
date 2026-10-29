//! Doublures de test des ports de `lf-application` (06 §2.2), derrière la feature `test-support`.
//!
//! [`FakeCorpus`] rejoue des réponses programmées et note chaque appel avec ses arguments
//! ([`PortRequest`]), [`FixedClock`] fige l'heure, [`MemoryAuditLog`] garde les lignes écrites et
//! [`FailingAuditLog`] échoue toujours. Les doublures servent aux tests de ce crate (`cfg(test)`)
//! et, par la feature, aux dev-dependencies des autres membres : `cargo xtask check-arch` (R10)
//! interdit de l'activer en dépendance normale, aucun binaire ne les embarque.
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
    entity::{
        Act, ArticleText, ArticleVersion, CodeListing, CompanyAgreement, Convention, Decision,
        Deliberation, RelatedLinks, SearchHit, SearchPage,
    },
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::{NumberQuery, SearchQuery},
};

use crate::{
    deadline::Deadline,
    ports::{
        ArticleAt, AuditError, AuditErrorKind, AuditLog, AuditRecord, Clock, ConfigKind,
        CorpusError, LegalCorpus,
    },
};

/// Durée relevée par [`FakeCorpus::seen_deadlines`] quand aucune échéance n'est publiée : « sans
/// limite », pour la distinguer d'une échéance réelle. Sentinelle de comparaison, pas un budget :
/// `Deadline::after(NO_DEADLINE)` ne panique plus (l'échéance sature à environ 30 ans), mais la
/// durée restante relevée ensuite vaudrait ce repli, jamais `NO_DEADLINE`.
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

/// Un appel reçu par [`FakeCorpus`] : la méthode du port et une copie de ses arguments (I-3 de la
/// revue finale). Un test de cas d'usage vérifie ainsi ce qui est demandé au port (identifiant
/// validé, date de version, requête de recherche), pas seulement combien de fois.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortRequest {
    /// `search(query)`.
    Search(SearchQuery),
    /// `get_act(id, date)`.
    GetAct(LegiTextId, VersionDate),
    /// `get_article(id)`.
    GetArticle(ArticleId),
    /// `get_decision(id)`.
    GetDecision(DecisionId),
    /// `get_deliberation(id)`.
    GetDeliberation(CnilTextId),
    /// `get_convention(id)`.
    GetConvention(KaliTextId),
    /// `get_company_agreement(id)`.
    GetCompanyAgreement(AccoTextId),
    /// `find_article(text, num, at)`.
    FindArticle(LegiTextId, String, ArticleAt),
    /// `find_by_number(query)`.
    FindByNumber(NumberQuery),
    /// `list_codes()`.
    ListCodes,
    /// `article_versions(id)`.
    ArticleVersions(ArticleId),
    /// `related_links(id)`.
    RelatedLinks(ArticleId),
}

impl PortRequest {
    /// Nom de la méthode du port appelée (`"search"`, `"get_act"`, ...), celui de
    /// [`FakeCorpus::calls_of`].
    #[must_use]
    pub const fn method(&self) -> &'static str {
        match self {
            Self::Search(_) => "search",
            Self::GetAct(..) => "get_act",
            Self::GetArticle(_) => "get_article",
            Self::GetDecision(_) => "get_decision",
            Self::GetDeliberation(_) => "get_deliberation",
            Self::GetConvention(_) => "get_convention",
            Self::GetCompanyAgreement(_) => "get_company_agreement",
            Self::FindArticle(..) => "find_article",
            Self::FindByNumber(_) => "find_by_number",
            Self::ListCodes => "list_codes",
            Self::ArticleVersions(_) => "article_versions",
            Self::RelatedLinks(_) => "related_links",
        }
    }
}

/// Faux corpus : une file de réponses par méthode du port, rejouées dans l'ordre.
///
/// Une file vide rend `Config { kind: Settings, .. }` qui nomme la méthode : un test qui oublie de
/// programmer une réponse échoue avec un message lisible plutôt que de bloquer, et l'appel est
/// compté par [`FakeCorpus::unserved`]. Chaque appel est noté avec ses arguments
/// ([`FakeCorpus::seen_requests`]). Le délai optionnel utilise `tokio::time::sleep`, donc le
/// temps virtuel des tests `start_paused`.
#[derive(Default)]
pub struct FakeCorpus {
    search: Responses<SearchPage>,
    get_act: Responses<Act>,
    get_article: Responses<ArticleText>,
    get_decision: Responses<Decision>,
    get_deliberation: Responses<Deliberation>,
    get_convention: Responses<Convention>,
    get_company_agreement: Responses<CompanyAgreement>,
    find_article: Responses<ArticleText>,
    find_by_number: Responses<Vec<SearchHit>>,
    list_codes: Responses<Vec<CodeListing>>,
    article_versions: Responses<Vec<ArticleVersion>>,
    related_links: Responses<RelatedLinks>,
    delay: Option<Duration>,
    /// Chaque appel reçu, méthode et arguments, dans l'ordre des appels.
    requests: Mutex<Vec<PortRequest>>,
    /// Appels qui ont trouvé la file de leur méthode vide.
    unserved: AtomicUsize,
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

    /// Programme la réponse suivante de `find_article`.
    pub fn push_find_article(&self, response: Result<ArticleText, CorpusError>) {
        lock(&self.find_article).push_back(response);
    }

    /// Programme la réponse suivante de `find_by_number`.
    pub fn push_find_by_number(&self, response: Result<Vec<SearchHit>, CorpusError>) {
        lock(&self.find_by_number).push_back(response);
    }

    /// Programme la réponse suivante de `list_codes`.
    pub fn push_list_codes(&self, response: Result<Vec<CodeListing>, CorpusError>) {
        lock(&self.list_codes).push_back(response);
    }

    /// Programme la réponse suivante de `article_versions`.
    pub fn push_article_versions(&self, response: Result<Vec<ArticleVersion>, CorpusError>) {
        lock(&self.article_versions).push_back(response);
    }

    /// Programme la réponse suivante de `related_links`.
    pub fn push_related_links(&self, response: Result<RelatedLinks, CorpusError>) {
        lock(&self.related_links).push_back(response);
    }

    /// Nombre total d'appels reçus, toutes méthodes confondues.
    #[must_use]
    pub fn calls(&self) -> usize {
        lock(&self.requests).len()
    }

    /// Nombre d'appels reçus par la méthode `method` (nom du port : `"search"`, `"get_act"`, ...).
    #[must_use]
    pub fn calls_of(&self, method: &str) -> usize {
        lock(&self.requests).iter().filter(|request| request.method() == method).count()
    }

    /// Chaque appel reçu, avec une copie de ses arguments, dans l'ordre des appels ; un appel
    /// abandonné pendant le délai y figure (il est noté à l'entrée).
    #[must_use]
    pub fn seen_requests(&self) -> Vec<PortRequest> {
        lock(&self.requests).clone()
    }

    /// Nombre d'appels qui ont trouvé vide la file de leur méthode (réponse non programmée).
    #[must_use]
    pub fn unserved(&self) -> usize {
        self.unserved.load(Ordering::SeqCst)
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

    /// Cœur commun des douze méthodes : à l'entrée (premier sondage du futur), note l'appel, ses
    /// arguments et l'échéance, tient un emplacement en vol et RETIRE la réponse de la file
    /// (ou compte l'appel comme non servi) ; puis dort le délai et rend la réponse déjà retirée.
    /// Les réponses suivent ainsi l'ordre des appels, même sous `join!` avec un délai. Un appel
    /// abandonné pendant le délai (annulation, `timeout`) consomme donc sa réponse.
    async fn serve<T>(
        &self,
        request: PortRequest,
        responses: &Responses<T>,
    ) -> Result<T, CorpusError> {
        let method = request.method();
        lock(&self.requests).push(request);
        lock(&self.deadlines).push(Deadline::current_or(NO_DEADLINE));
        let now_in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        let _slot = InFlight(&self.in_flight);
        self.max_in_flight.fetch_max(now_in_flight, Ordering::SeqCst);
        // Le verrou est pris et rendu dans la même instruction : jamais tenu à travers un `await`.
        let next = lock(responses).pop_front();
        if next.is_none() {
            self.unserved.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        next.unwrap_or_else(|| {
            Err(CorpusError::Config {
                kind: ConfigKind::Settings,
                detail: format!("FakeCorpus: no response programmed for {method}"),
            })
        })
    }
}

impl LegalCorpus for FakeCorpus {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        self.serve(PortRequest::Search(query.clone()), &self.search).await
    }

    async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
        self.serve(PortRequest::GetAct(id.clone(), date), &self.get_act).await
    }

    async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
        self.serve(PortRequest::GetArticle(id.clone()), &self.get_article).await
    }

    async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
        self.serve(PortRequest::GetDecision(id.clone()), &self.get_decision).await
    }

    async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
        self.serve(PortRequest::GetDeliberation(id.clone()), &self.get_deliberation).await
    }

    async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
        self.serve(PortRequest::GetConvention(id.clone()), &self.get_convention).await
    }

    async fn get_company_agreement(
        &self,
        id: &AccoTextId,
    ) -> Result<CompanyAgreement, CorpusError> {
        let request = PortRequest::GetCompanyAgreement(id.clone());
        self.serve(request, &self.get_company_agreement).await
    }

    async fn find_article(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        let request = PortRequest::FindArticle(text.clone(), num.to_owned(), at);
        self.serve(request, &self.find_article).await
    }

    async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        self.serve(PortRequest::FindByNumber(query.clone()), &self.find_by_number).await
    }

    async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        self.serve(PortRequest::ListCodes, &self.list_codes).await
    }

    async fn article_versions(&self, id: &ArticleId) -> Result<Vec<ArticleVersion>, CorpusError> {
        self.serve(PortRequest::ArticleVersions(id.clone()), &self.article_versions).await
    }

    async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
        self.serve(PortRequest::RelatedLinks(id.clone()), &self.related_links).await
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
                // Un futur re-sondé avec le même réveil ne s'inscrit qu'une fois.
                if !state.waiters.iter().any(|waiter| waiter.will_wake(cx.waker())) {
                    state.waiters.push(cx.waker().clone());
                }
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

/// Fabriques partagées par les tests du crate (`cfg(test)` seulement : hors de la feature
/// `test-support`, elles ne font pas partie des doublures exportées). Une seule copie de la
/// requête de recherche et de `poll_once` pour `audited`, `testing` et les ports (m-9).
#[cfg(test)]
pub(crate) mod fixtures {
    use std::{
        future::Future,
        num::NonZeroU32,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    use lf_domain::{
        fond::Fond,
        search::{MatchMode, PageSize, QueryText, SearchQuery},
    };

    /// Requête valide : `bail` sur le fond JURI, première page, réglages par défaut.
    pub(crate) fn query() -> SearchQuery {
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

    /// Exige un futur `Send` et le sonde une fois, sans réveil : un port factice qui n'attend
    /// rien est prêt au premier sondage.
    pub(crate) fn poll_once<F: Future + Send>(future: F) -> Poll<F::Output> {
        pin!(future).poll(&mut Context::from_waker(Waker::noop()))
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::query, *};
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
    async fn fake_corpus_records_the_arguments_of_every_call() {
        let corpus = FakeCorpus::new();
        let cnil = SearchQuery::new(
            Fond::Cnil,
            QueryText::parse("données de santé").unwrap(),
            NonZeroU32::new(2).unwrap(),
            PageSize::default(),
            MatchMode::default(),
            None,
            None,
        )
        .unwrap();
        let act: LegiTextId = "LEGITEXT000006070721".parse().unwrap();
        let date = VersionDate::parse("2024-01-01").unwrap();
        let decision: DecisionId = "JURITEXT000037999394".parse().unwrap();
        let deliberation: CnilTextId = "CNILTEXT000053352643".parse().unwrap();
        let convention: KaliTextId = "KALITEXT000005652329".parse().unwrap();
        let agreement: AccoTextId = "ACCOTEXT000037731479".parse().unwrap();
        assert_eq!(corpus.seen_requests(), Vec::<PortRequest>::new());

        // Réponses non programmées : seuls les arguments reçus importent ici.
        let _ = corpus.search(&query()).await;
        let _ = corpus.get_act(&act, date).await;
        let _ = corpus.get_article(&article_id()).await;
        let _ = corpus.get_decision(&decision).await;
        let _ = corpus.get_deliberation(&deliberation).await;
        let _ = corpus.get_convention(&convention).await;
        let _ = corpus.get_company_agreement(&agreement).await;
        let _ = corpus.search(&cnil).await;
        let _ = corpus.get_act(&act, VersionDate::MIN).await;

        assert_eq!(
            corpus.seen_requests(),
            vec![
                PortRequest::Search(query()),
                PortRequest::GetAct(act.clone(), date),
                PortRequest::GetArticle(article_id()),
                PortRequest::GetDecision(decision),
                PortRequest::GetDeliberation(deliberation),
                PortRequest::GetConvention(convention),
                PortRequest::GetCompanyAgreement(agreement),
                PortRequest::Search(cnil),
                PortRequest::GetAct(act, VersionDate::MIN),
            ]
        );
        // Chaque requête porte le nom de sa méthode, celui que comptent `calls_of` et `calls`.
        let methods: Vec<&str> = corpus.seen_requests().iter().map(PortRequest::method).collect();
        assert_eq!(
            methods,
            [
                "search",
                "get_act",
                "get_article",
                "get_decision",
                "get_deliberation",
                "get_convention",
                "get_company_agreement",
                "search",
                "get_act",
            ]
        );
        assert_eq!((corpus.calls(), corpus.calls_of("get_act")), (9, 2));
    }

    fn article_text() -> ArticleText {
        ArticleText {
            article_id: article_id(),
            num: Some("9".to_owned()),
            etat: None,
            code_title: None,
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: lf_domain::text::LongText::default(),
            nota: None,
        }
    }

    fn number_query() -> NumberQuery {
        NumberQuery::new(
            Fond::Juri,
            lf_domain::search::NumberKind::Case,
            lf_domain::search::NumberValue::parse("24-13.470").unwrap(),
            VersionDate::MIN,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn fake_corpus_serves_and_records_the_m6_and_m7_methods() {
        let corpus = FakeCorpus::new();
        let text: LegiTextId = "LEGITEXT000006070721".parse().unwrap();
        let today = ArticleAt::Today(VersionDate::parse("2026-10-29").unwrap());
        let on = ArticleAt::On(VersionDate::parse("2020-01-01").unwrap());
        let hit =
            SearchHit::new(Fond::Juri, "JURITEXT000037999394".parse().unwrap(), None).unwrap();
        let listing = CodeListing { id: text.clone(), title: "Code civil".to_owned(), etat: None };
        let version = ArticleVersion {
            id: "LEGIARTI000006419288".parse().unwrap(),
            num: Some("9".to_owned()),
            etat: None,
            date_debut: None,
            date_fin: None,
        };
        corpus.push_find_article(Ok(article_text()));
        corpus.push_find_by_number(Ok(vec![hit.clone()]));
        corpus.push_list_codes(Ok(vec![listing.clone()]));
        corpus.push_article_versions(Ok(vec![version.clone()]));
        corpus.push_related_links(Ok(RelatedLinks::default()));

        assert_eq!(corpus.find_article(&text, "9", today).await, Ok(article_text()));
        assert_eq!(corpus.find_by_number(&number_query()).await, Ok(vec![hit]));
        assert_eq!(corpus.list_codes().await, Ok(vec![listing]));
        assert_eq!(corpus.article_versions(&article_id()).await, Ok(vec![version]));
        assert_eq!(corpus.related_links(&article_id()).await, Ok(RelatedLinks::default()));

        // Les arguments reçus sont notés, l'indicateur « aujourd'hui » compris.
        assert_eq!(
            corpus.seen_requests(),
            vec![
                PortRequest::FindArticle(text.clone(), "9".to_owned(), today),
                PortRequest::FindByNumber(number_query()),
                PortRequest::ListCodes,
                PortRequest::ArticleVersions(article_id()),
                PortRequest::RelatedLinks(article_id()),
            ]
        );
        for method in
            ["find_article", "find_by_number", "list_codes", "article_versions", "related_links"]
        {
            assert_eq!(corpus.calls_of(method), 1, "{method}");
        }

        // Files vides : chaque méthode nomme la sienne, et l'appel est compté comme non servi.
        let unprogrammed = |method: &str| CorpusError::Config {
            kind: ConfigKind::Settings,
            detail: format!("FakeCorpus: no response programmed for {method}"),
        };
        assert_eq!(corpus.find_article(&text, "9", on).await, Err(unprogrammed("find_article")));
        assert_eq!(
            corpus.find_by_number(&number_query()).await,
            Err(unprogrammed("find_by_number"))
        );
        assert_eq!(corpus.list_codes().await, Err(unprogrammed("list_codes")));
        assert_eq!(
            corpus.article_versions(&article_id()).await,
            Err(unprogrammed("article_versions"))
        );
        assert_eq!(corpus.related_links(&article_id()).await, Err(unprogrammed("related_links")));
        assert_eq!(corpus.unserved(), 5);
        assert_eq!(corpus.seen_requests().last(), Some(&PortRequest::RelatedLinks(article_id())));
    }

    #[tokio::test(start_paused = true)]
    async fn fake_corpus_records_an_abandoned_request_at_entry() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_secs(60));
        corpus.push_get_article(Err(timeout()));
        let abandoned =
            tokio::time::timeout(Duration::from_secs(1), corpus.get_article(&article_id())).await;
        assert!(abandoned.is_err());
        assert_eq!(corpus.seen_requests(), vec![PortRequest::GetArticle(article_id())]);
    }

    #[tokio::test]
    async fn fake_corpus_counts_calls_that_found_an_empty_queue() {
        let corpus = FakeCorpus::new();
        assert_eq!(corpus.unserved(), 0);
        corpus.push_search(Ok(page(1)));
        assert_eq!(corpus.search(&query()).await, Ok(page(1)));
        assert_eq!(corpus.unserved(), 0);
        // La file de `search` est vide, celle de `get_article` ne l'a jamais remplie.
        let _ = corpus.search(&query()).await;
        let _ = corpus.get_article(&article_id()).await;
        assert_eq!((corpus.calls(), corpus.unserved()), (3, 2));
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
        let unprogrammed = |method: &str| CorpusError::Config {
            kind: ConfigKind::Settings,
            detail: format!("FakeCorpus: no response programmed for {method}"),
        };
        corpus.push_search(Err(not_found("search")));
        corpus.push_get_act(Err(not_found("get_act")));
        corpus.push_get_article(Err(not_found("get_article")));
        corpus.push_get_decision(Err(not_found("get_decision")));
        corpus.push_get_deliberation(Err(not_found("get_deliberation")));
        corpus.push_get_convention(Err(not_found("get_convention")));
        corpus.push_get_company_agreement(Err(not_found("get_company_agreement")));
        let query = query();
        let act = "LEGITEXT000006070721".parse().unwrap();
        let article = article_id();
        let decision = "JURITEXT000037999394".parse().unwrap();
        let deliberation = "CNILTEXT000053352643".parse().unwrap();
        let convention = "KALITEXT000005652329".parse().unwrap();
        let agreement = "ACCOTEXT000037731479".parse().unwrap();

        // Première série : chaque méthode lit sa propre file ; chacune compte un appel sous son nom.
        assert_eq!(corpus.search(&query).await, Err(not_found("search")));
        assert_eq!(corpus.get_act(&act, VersionDate::MIN).await, Err(not_found("get_act")));
        assert_eq!(corpus.get_article(&article).await, Err(not_found("get_article")));
        assert_eq!(corpus.get_decision(&decision).await, Err(not_found("get_decision")));
        assert_eq!(
            corpus.get_deliberation(&deliberation).await,
            Err(not_found("get_deliberation"))
        );
        assert_eq!(corpus.get_convention(&convention).await, Err(not_found("get_convention")));
        assert_eq!(
            corpus.get_company_agreement(&agreement).await,
            Err(not_found("get_company_agreement"))
        );
        let methods = [
            "search",
            "get_act",
            "get_article",
            "get_decision",
            "get_deliberation",
            "get_convention",
            "get_company_agreement",
        ];
        for method in methods {
            assert_eq!(corpus.calls_of(method), 1, "{method}");
        }

        // Seconde série : les files sont vides, chaque méthode nomme la sienne.
        assert_eq!(corpus.search(&query).await, Err(unprogrammed("search")));
        assert_eq!(corpus.get_act(&act, VersionDate::MIN).await, Err(unprogrammed("get_act")));
        assert_eq!(corpus.get_article(&article).await, Err(unprogrammed("get_article")));
        assert_eq!(corpus.get_decision(&decision).await, Err(unprogrammed("get_decision")));
        assert_eq!(
            corpus.get_deliberation(&deliberation).await,
            Err(unprogrammed("get_deliberation"))
        );
        assert_eq!(corpus.get_convention(&convention).await, Err(unprogrammed("get_convention")));
        assert_eq!(
            corpus.get_company_agreement(&agreement).await,
            Err(unprogrammed("get_company_agreement"))
        );
        for method in methods {
            assert_eq!(corpus.calls_of(method), 2, "{method}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn fake_corpus_hands_out_responses_in_call_order_under_a_delay() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_millis(50));
        corpus.push_search(Ok(page(1)));
        corpus.push_search(Ok(page(2)));
        let query = query();
        let (call0, call1) = tokio::join!(corpus.search(&query), corpus.search(&query));
        assert_eq!((call0, call1), (Ok(page(1)), Ok(page(2))));
    }

    #[tokio::test(start_paused = true)]
    async fn fake_corpus_abandoned_call_consumes_its_response() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_secs(60));
        corpus.push_search(Ok(page(1)));
        corpus.push_search(Ok(page(2)));
        let query = query();
        let abandoned = tokio::time::timeout(Duration::from_secs(1), corpus.search(&query)).await;
        assert!(abandoned.is_err());
        assert_eq!(corpus.search(&query).await, Ok(page(2)));
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
        let unprogrammed = |method: &str| CorpusError::Config {
            kind: ConfigKind::Settings,
            detail: format!("FakeCorpus: no response programmed for {method}"),
        };
        assert_eq!(first, Err(unprogrammed("search")));
        let article = Err(unprogrammed("get_article"));
        assert_eq!((second, third), (article.clone(), article));
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

    /// Nombre d'écritures parquées derrière la porte.
    fn parked(log: &MemoryAuditLog) -> usize {
        lock(log.gate.as_ref().unwrap()).waiters.len()
    }

    /// Réveil qui compte ses réveils ; ses clones partagent le même `Arc`, donc `will_wake`.
    #[derive(Default)]
    struct CountingWake(AtomicUsize);

    impl std::task::Wake for CountingWake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn gated_audit_log_registers_one_waker_per_waiting_task() {
        let log = MemoryAuditLog::with_gate();
        let entry = record(ToolName::FR_SEARCH, 1);
        let counter = std::sync::Arc::new(CountingWake::default());
        let waker = Waker::from(std::sync::Arc::clone(&counter));
        let mut write = pin!(log.record(&entry));
        let mut cx = Context::from_waker(&waker);
        for _ in 0..3 {
            assert!(write.as_mut().poll(&mut cx).is_pending());
        }
        // Re-sonder avec le même réveil ne l'enregistre pas deux fois : un seul réveil à l'ouverture.
        assert_eq!(parked(&log), 1);
        log.open_gate();
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert!(write.as_mut().poll(&mut cx).is_ready());
    }

    #[tokio::test(start_paused = true)]
    async fn gated_audit_log_wakes_a_waiting_task() {
        let log = std::sync::Arc::new(MemoryAuditLog::with_gate());
        let writer = {
            let log = std::sync::Arc::clone(&log);
            tokio::spawn(async move { log.record(&record(ToolName::FR_SEARCH, 1)).await })
        };
        tokio::task::yield_now().await;
        assert!(log.records().is_empty());
        // L'écrivain est bien parqué derrière la porte, pas simplement pas encore ordonnancé.
        assert_eq!(parked(&log), 1);
        log.open_gate();
        // Bornée (temps virtuel) : sans réveil à l'ouverture, l'écrivain resterait parqué pour
        // toujours et le test pendrait au lieu d'échouer.
        tokio::time::timeout(Duration::from_secs(60), writer).await.unwrap().unwrap().unwrap();
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
