//! `CachedCorpus<C>` : décorateur en mémoire du port `LegalCorpus` (00 §9, 02 §7.2, ADR-0007).
//!
//! Un seul `moka::future::Cache<CacheKey, CacheValue>` sert les douze opérations : une seule
//! capacité, un seul `weigher`, et une `Expiry` qui choisit la durée de vie d'après l'opération.
//! Chaque méthode du port passe par [`CachedCorpus::fetch`], qui :
//! 1. cherche l'entrée de la clé `(environnement, opération)` ;
//! 2. sinon évalue l'appel décoré, **une seule fois** pour tous les appelants concurrents
//!    identiques (`or_try_insert_with`) ;
//! 3. n'insère ni une erreur ni une réponse vide : l'initialisation rend alors un `Err(Bypass)`
//!    que moka ne met pas en cache, et que l'on reconvertit en `Err` ou en `Ok` pour l'appelant.

use std::{any::Any, future::Future, sync::Arc};

use lf_application::ports::{ArticleAt, CorpusError, LegalCorpus};
use lf_domain::{
    coverage::PisteEnvironment,
    date::VersionDate,
    entity::{
        Act, ArticleText, ArticleVersion, CodeListing, CompanyAgreement, Convention, Decision,
        Deliberation, RelatedLinks, SearchHit, SearchPage,
    },
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::{NumberQuery, SearchQuery},
};
use moka::future::Cache;
use tracing::{debug, warn};

use crate::{
    key::{CacheKey, Op},
    policy::{CachePolicy, OpExpiry, Weigh, saturating_weight},
};

/// Valeur d'une entrée : la réponse, partagée, et son poids calculé une fois à l'insertion.
///
/// Le type de la réponse est effacé (`Any`) : la clé détermine l'opération, donc le type que
/// [`CachedCorpus::fetch`] relit. Un type inattendu, qui ne peut venir que d'une erreur de
/// programmation, n'est jamais servi : l'entrée est retirée et l'appel part vers le corpus.
#[derive(Clone)]
struct CacheValue {
    value: Arc<dyn Any + Send + Sync>,
    weight: usize,
}

impl CacheValue {
    fn new<T: Weigh + Send + Sync + 'static>(value: T) -> Self {
        let weight = value.weight();
        Self { value: Arc::new(value), weight }
    }

    fn get<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }
}

/// Réponse que le cache sait garder.
trait Cacheable: Weigh + Clone + Send + Sync + 'static {
    /// Réponse vide, qui ne doit pas rester en cache (00 §9) : recherche sans résultat, liste
    /// vide. Un « introuvable » transitoire ne doit pas persister (ADR-0007, alternatives).
    fn is_empty_result(&self) -> bool {
        false
    }
}

impl Cacheable for SearchPage {
    fn is_empty_result(&self) -> bool {
        self.hits.is_empty()
    }
}

impl Cacheable for Vec<SearchHit> {
    fn is_empty_result(&self) -> bool {
        self.is_empty()
    }
}

impl Cacheable for Vec<CodeListing> {
    fn is_empty_result(&self) -> bool {
        self.is_empty()
    }
}

impl Cacheable for Vec<ArticleVersion> {
    fn is_empty_result(&self) -> bool {
        self.is_empty()
    }
}

impl Cacheable for RelatedLinks {
    fn is_empty_result(&self) -> bool {
        self.cites.is_empty() && self.cited_by.is_empty()
    }
}

impl Cacheable for Act {}
impl Cacheable for Convention {}
impl Cacheable for ArticleText {}
impl Cacheable for Decision {}
impl Cacheable for Deliberation {}
impl Cacheable for CompanyAgreement {}

/// Issue d'une initialisation qui ne doit pas être insérée. moka la rend à tous les appelants
/// coalescés sous la forme `Arc<Bypass<T>>`, d'où `CorpusError: Clone` (02 §3.4, piège 1).
enum Bypass<T> {
    /// L'appel décoré a échoué : l'erreur est rendue telle quelle, jamais mise en cache.
    Failed(CorpusError),
    /// L'appel a réussi mais la réponse est vide : rendue en `Ok`, jamais mise en cache.
    Empty(T),
}

/// Décorateur de cache du port `LegalCorpus`.
///
/// La composition de production est `CachedCorpus<PisteCorpus>`, construite par
/// `CachedCorpus::new(piste, env, CachePolicy::default())` ; l'audit reste au-dessus, au niveau du
/// cas d'usage (ADR-0008).
pub struct CachedCorpus<C> {
    inner: C,
    env: PisteEnvironment,
    cache: Cache<CacheKey, CacheValue>,
}

impl<C: LegalCorpus> CachedCorpus<C> {
    /// Décore `inner` : les clés sont préfixées par `env` (jamais de réponse du bac à sable
    /// resservie en production), les durées de vie et la capacité viennent de `policy`.
    pub fn new(inner: C, env: PisteEnvironment, policy: CachePolicy) -> Self {
        let cache = Cache::builder()
            .max_capacity(policy.capacity_bytes)
            .weigher(|key: &CacheKey, value: &CacheValue| {
                saturating_weight(key.weight().saturating_add(value.weight))
            })
            .expire_after(OpExpiry::new(policy))
            .build();
        Self { inner, env, cache }
    }

    /// Sert `op` depuis le cache, ou évalue `call` (une seule fois pour des appels concurrents
    /// identiques) et garde la réponse si elle est un succès non vide.
    ///
    /// Sur un succès en cache : ni jeton OAuth, ni attente du limiteur, ni réessai (ADR-0007 §8),
    /// puisque `call` n'est pas évalué. Les journaux ne portent que le nom de l'opération et
    /// l'issue du cache (`hit`, `miss` ou `bypass`), jamais un argument.
    async fn fetch<T, F, Fut>(&self, op: Op, call: F) -> Result<T, CorpusError>
    where
        T: Cacheable,
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, CorpusError>> + Send,
    {
        let key = CacheKey { env: self.env, op };
        let name = key.op.name();
        let load = async {
            match call().await {
                Err(error) => Err(Bypass::Failed(error)),
                Ok(value) if value.is_empty_result() => Err(Bypass::Empty(value)),
                Ok(value) => Ok(CacheValue::new(value)),
            }
        };
        let outcome = self.cache.entry(key.clone()).or_try_insert_with(load).await;
        match outcome {
            Ok(entry) => {
                let cache = if entry.is_fresh() { "miss" } else { "hit" };
                if let Some(value) = entry.value().get::<T>() {
                    debug!(op = name, cache, "corpus call");
                    return Ok(value.clone());
                }
                warn!(op = name, "cache entry of an unexpected type, evicted");
                self.cache.invalidate(&key).await;
                call().await
            }
            Err(bypass) => {
                debug!(op = name, cache = "bypass", "corpus call");
                match bypass.as_ref() {
                    Bypass::Failed(error) => Err(error.clone()),
                    Bypass::Empty(value) => Ok(value.clone()),
                }
            }
        }
    }
}

impl<C: LegalCorpus> LegalCorpus for CachedCorpus<C> {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        self.fetch(Op::Search(query.clone()), || self.inner.search(query)).await
    }

    async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
        self.fetch(Op::Act(id.clone(), date), || self.inner.get_act(id, date)).await
    }

    async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
        self.fetch(Op::Article(id.clone()), || self.inner.get_article(id)).await
    }

    async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
        self.fetch(Op::Decision(id.clone()), || self.inner.get_decision(id)).await
    }

    async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
        self.fetch(Op::Deliberation(id.clone()), || self.inner.get_deliberation(id)).await
    }

    async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
        self.fetch(Op::Convention(id.clone()), || self.inner.get_convention(id)).await
    }

    async fn get_company_agreement(
        &self,
        id: &AccoTextId,
    ) -> Result<CompanyAgreement, CorpusError> {
        self.fetch(Op::Agreement(id.clone()), || self.inner.get_company_agreement(id)).await
    }

    async fn find_article(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        let op = Op::FindArticle(text.clone(), num.to_owned(), at);
        self.fetch(op, || self.inner.find_article(text, num, at)).await
    }

    async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        self.fetch(Op::FindByNumber(query.clone()), || self.inner.find_by_number(query)).await
    }

    async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        self.fetch(Op::Codes, || self.inner.list_codes()).await
    }

    async fn article_versions(&self, id: &ArticleId) -> Result<Vec<ArticleVersion>, CorpusError> {
        self.fetch(Op::Versions(id.clone()), || self.inner.article_versions(id)).await
    }

    async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
        self.fetch(Op::Related(id.clone()), || self.inner.related_links(id)).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use lf_application::testing::FakeCorpus;
    use pretty_assertions::assert_eq;
    use tokio::{task::JoinSet, time::timeout};

    use super::*;
    use crate::fixtures::{
        act, agreement, article, article_id, codes, convention, day, decision, deliberation,
        empty_page, every_op, number_query, page, query, related, text_id, versions,
    };

    const SANDBOX: PisteEnvironment = PisteEnvironment::Sandbox;

    fn cached(fake: FakeCorpus) -> CachedCorpus<FakeCorpus> {
        CachedCorpus::new(fake, SANDBOX, CachePolicy::default())
    }

    fn timeout_error() -> CorpusError {
        CorpusError::Timeout
    }

    #[tokio::test]
    async fn second_identical_search_calls_inner_once() {
        let fake = FakeCorpus::new();
        fake.push_search(Ok(page("a", 2)));
        let corpus = cached(fake);
        let q = query("bail");
        let first = corpus.search(&q).await;
        let second = corpus.search(&q).await;
        assert_eq!(first, Ok(page("a", 2)));
        assert_eq!(second, first);
        assert_eq!(corpus.inner.calls_of("search"), 1);
        assert_eq!(corpus.inner.unserved(), 0);
    }

    #[tokio::test]
    async fn every_operation_is_served_from_the_cache_on_the_second_call() {
        let fake = FakeCorpus::new();
        fake.push_search(Ok(page("a", 1)));
        fake.push_get_act(Ok(act("a")));
        fake.push_get_article(Ok(article("a")));
        fake.push_get_decision(Ok(decision("a")));
        fake.push_get_deliberation(Ok(deliberation("a")));
        fake.push_get_convention(Ok(convention("a")));
        fake.push_get_company_agreement(Ok(agreement("a")));
        fake.push_find_article(Ok(article("b")));
        fake.push_find_by_number(Ok(page("a", 1).hits));
        fake.push_list_codes(Ok(codes("a", 2)));
        fake.push_article_versions(Ok(versions("a", 2)));
        fake.push_related_links(Ok(related("a", 1, 1)));
        let corpus = cached(fake);
        let (q, n) = (query("bail"), number_query("24-13.470"));
        let today = day("2026-10-29");
        for _ in 0..2 {
            assert_eq!(corpus.search(&q).await, Ok(page("a", 1)));
            assert_eq!(corpus.get_act(&text_id(), today).await, Ok(act("a")));
            assert_eq!(corpus.get_article(&article_id()).await, Ok(article("a")));
            let id = "JURITEXT000037999394".parse().unwrap();
            assert_eq!(corpus.get_decision(&id).await, Ok(decision("a")));
            let id = "CNILTEXT000053352643".parse().unwrap();
            assert_eq!(corpus.get_deliberation(&id).await, Ok(deliberation("a")));
            let id = "KALITEXT000005652329".parse().unwrap();
            assert_eq!(corpus.get_convention(&id).await, Ok(convention("a")));
            let id = "ACCOTEXT000037731479".parse().unwrap();
            assert_eq!(corpus.get_company_agreement(&id).await, Ok(agreement("a")));
            let found = corpus.find_article(&text_id(), "9", ArticleAt::Today(today)).await;
            assert_eq!(found, Ok(article("b")));
            assert_eq!(corpus.find_by_number(&n).await, Ok(page("a", 1).hits));
            assert_eq!(corpus.list_codes().await, Ok(codes("a", 2)));
            assert_eq!(corpus.article_versions(&article_id()).await, Ok(versions("a", 2)));
            assert_eq!(corpus.related_links(&article_id()).await, Ok(related("a", 1, 1)));
        }
        // Douze opérations, un appel chacune : les douze méthodes du port sont décorées.
        assert_eq!(corpus.inner.calls(), 12);
        assert_eq!(corpus.inner.unserved(), 0);
        let per_method: Vec<(&str, usize)> =
            every_op().iter().map(|op| (op.name(), corpus.inner.calls_of(op.name()))).collect();
        assert_eq!(per_method, every_op().iter().map(|op| (op.name(), 1)).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn different_arguments_are_different_entries() {
        let fake = FakeCorpus::new();
        for filler in ["a", "b", "c"] {
            fake.push_search(Ok(page(filler, 1)));
        }
        fake.push_get_act(Ok(act("a")));
        fake.push_get_act(Ok(act("b")));
        fake.push_find_article(Ok(article("a")));
        fake.push_find_article(Ok(article("b")));
        let corpus = cached(fake);
        assert_eq!(corpus.search(&query("bail")).await, Ok(page("a", 1)));
        assert_eq!(corpus.search(&query("Bail")).await, Ok(page("b", 1)));
        assert_eq!(corpus.search(&query("bail commercial")).await, Ok(page("c", 1)));
        let (today, tomorrow) = (day("2026-10-29"), day("2026-10-30"));
        assert_eq!(corpus.get_act(&text_id(), today).await, Ok(act("a")));
        assert_eq!(corpus.get_act(&text_id(), tomorrow).await, Ok(act("b")));
        // « Aujourd'hui » et « à cette date » sont deux signaux, donc deux entrées.
        let text = text_id();
        let find = |at| corpus.find_article(&text, "9", at);
        assert_eq!(find(ArticleAt::Today(today)).await, Ok(article("a")));
        assert_eq!(find(ArticleAt::On(today)).await, Ok(article("b")));
        assert_eq!(find(ArticleAt::Today(today)).await, Ok(article("a")));
        assert_eq!(corpus.inner.calls(), 7);
    }

    #[tokio::test]
    async fn cache_namespaced_by_env() {
        let fake = FakeCorpus::new();
        fake.push_search(Ok(page("sandbox", 1)));
        fake.push_search(Ok(page("production", 1)));
        let mut corpus = cached(fake);
        let q = query("bail");
        assert_eq!(corpus.search(&q).await, Ok(page("sandbox", 1)));
        // Même requête, autre environnement : l'entrée du bac à sable n'est pas resservie.
        corpus.env = PisteEnvironment::Production;
        assert_eq!(corpus.search(&q).await, Ok(page("production", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
        // Chaque environnement garde sa propre entrée.
        assert_eq!(corpus.search(&q).await, Ok(page("production", 1)));
        corpus.env = SANDBOX;
        assert_eq!(corpus.search(&q).await, Ok(page("sandbox", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_identical_calls_coalesce() {
        let fake = FakeCorpus::new().with_delay(Duration::from_millis(100));
        fake.push_search(Ok(page("a", 3)));
        let corpus = Arc::new(cached(fake));
        let q = query("bail");
        let mut tasks = JoinSet::new();
        for _ in 0..10 {
            let (corpus, q) = (Arc::clone(&corpus), q.clone());
            tasks.spawn(async move { corpus.search(&q).await });
        }
        let mut results = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            results.push(joined.unwrap());
        }
        assert_eq!(results, vec![Ok(page("a", 3)); 10]);
        assert_eq!(corpus.inner.calls_of("search"), 1);
        assert_eq!(corpus.inner.max_concurrency(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn coalesced_callers_share_the_error_and_the_next_call_retries() {
        let fake = FakeCorpus::new().with_delay(Duration::from_millis(100));
        fake.push_search(Err(timeout_error()));
        fake.push_search(Ok(page("a", 1)));
        let corpus = Arc::new(cached(fake));
        let q = query("bail");
        let mut tasks = JoinSet::new();
        for _ in 0..5 {
            let (corpus, q) = (Arc::clone(&corpus), q.clone());
            tasks.spawn(async move { corpus.search(&q).await });
        }
        let mut results = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            results.push(joined.unwrap());
        }
        assert_eq!(results, vec![Err(timeout_error()); 5]);
        assert_eq!(corpus.inner.calls_of("search"), 1);
        // L'erreur n'est pas restée en cache.
        assert_eq!(corpus.search(&q).await, Ok(page("a", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
    }

    #[tokio::test]
    async fn errors_and_empty_results_are_not_cached() {
        let fake = FakeCorpus::new();
        // Erreur puis succès : deux appels, et le succès seul est gardé.
        fake.push_search(Err(timeout_error()));
        fake.push_search(Ok(page("a", 1)));
        // Page vide deux fois : deux appels, et l'appelant reçoit bien `Ok` (chemin `Bypass`).
        fake.push_search(Ok(empty_page()));
        fake.push_search(Ok(empty_page()));
        let corpus = cached(fake);
        let q = query("bail");
        assert_eq!(corpus.search(&q).await, Err(timeout_error()));
        assert_eq!(corpus.search(&q).await, Ok(page("a", 1)));
        assert_eq!(corpus.search(&q).await, Ok(page("a", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
        let empty = query("introuvable");
        assert_eq!(corpus.search(&empty).await, Ok(empty_page()));
        assert_eq!(corpus.search(&empty).await, Ok(empty_page()));
        assert_eq!(corpus.inner.calls_of("search"), 4);
    }

    #[tokio::test]
    async fn empty_lists_are_not_cached_for_every_list_operation() {
        let fake = FakeCorpus::new();
        for _ in 0..2 {
            fake.push_find_by_number(Ok(Vec::new()));
            fake.push_list_codes(Ok(Vec::new()));
            fake.push_article_versions(Ok(Vec::new()));
            fake.push_related_links(Ok(related("a", 0, 0)));
        }
        // Un seul côté vide ne suffit pas à écarter la réponse.
        fake.push_related_links(Ok(related("a", 0, 1)));
        let corpus = cached(fake);
        let n = number_query("24-13.470");
        for _ in 0..2 {
            assert_eq!(corpus.find_by_number(&n).await, Ok(Vec::new()));
            assert_eq!(corpus.list_codes().await, Ok(Vec::new()));
            assert_eq!(corpus.article_versions(&article_id()).await, Ok(Vec::new()));
            assert_eq!(corpus.related_links(&article_id()).await, Ok(related("a", 0, 0)));
        }
        for method in ["find_by_number", "list_codes", "article_versions", "related_links"] {
            assert_eq!(corpus.inner.calls_of(method), 2, "{method}");
        }
        let links = related("a", 0, 1);
        assert_eq!(corpus.related_links(&article_id()).await, Ok(links.clone()));
        assert_eq!(corpus.related_links(&article_id()).await, Ok(links));
        assert_eq!(corpus.inner.calls_of("related_links"), 3);
    }

    #[tokio::test]
    async fn every_error_kind_is_passed_through_unchanged() {
        let errors = [
            CorpusError::NotFound { what: "/consult/juri".into() },
            CorpusError::Timeout,
            CorpusError::RateLimited { retry_after: Some(Duration::from_secs(12)) },
            CorpusError::TooLarge { detail: "response too large".into() },
        ];
        let fake = FakeCorpus::new();
        for error in &errors {
            fake.push_get_article(Err(error.clone()));
        }
        let corpus = cached(fake);
        for error in errors {
            assert_eq!(corpus.get_article(&article_id()).await, Err(error));
        }
        assert_eq!(corpus.inner.calls_of("get_article"), 4);
    }

    #[tokio::test]
    async fn capacity_is_bounded_by_weight() {
        const CAPACITY: u64 = 20_000;
        const ARTICLES: usize = 40;
        let policy = CachePolicy { capacity_bytes: CAPACITY, ..CachePolicy::default() };
        let fake = FakeCorpus::new();
        let filler = "x".repeat(300);
        let ids: Vec<ArticleId> =
            (1..=ARTICLES).map(|n| format!("LEGIARTI{n:012}").parse().unwrap()).collect();
        for _ in &ids {
            fake.push_get_article(Ok(article(&filler)));
        }
        let corpus = CachedCorpus::new(fake, SANDBOX, policy);
        for id in &ids {
            assert!(corpus.get_article(id).await.is_ok());
        }
        corpus.cache.run_pending_tasks().await;
        // 40 articles pèsent bien plus que 20 000 octets : la capacité est comptée en octets (le
        // nombre d'entrées, lui, tiendrait largement), donc des entrées sont évincées.
        let one = u64::try_from(article(&filler).weight()).unwrap();
        assert!(one * 40 > CAPACITY, "poids d'un article : {one}");
        let (weighted, count) = (corpus.cache.weighted_size(), corpus.cache.entry_count());
        assert!(weighted <= CAPACITY, "poids total : {weighted}");
        assert!(count < 40, "entrées : {count}");
        assert!(weighted > 0);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_initializer_lets_next_caller_retry() {
        let fake = FakeCorpus::new().with_delay(Duration::from_secs(1));
        // La première réponse est consommée par l'appel abandonné.
        fake.push_search(Ok(page("abandonnée", 1)));
        fake.push_search(Ok(page("reprise", 1)));
        let corpus = cached(fake);
        let q = query("bail");
        let abandoned = timeout(Duration::from_millis(100), corpus.search(&q)).await;
        assert!(abandoned.is_err(), "le délai coupe l'appel en cours d'initialisation");
        // L'appelant suivant n'est pas bloqué par l'initialisation abandonnée : il relance l'appel.
        assert_eq!(corpus.search(&q).await, Ok(page("reprise", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
        // Et la réponse de la reprise est gardée.
        assert_eq!(corpus.search(&q).await, Ok(page("reprise", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_leader_hands_over_to_the_waiting_caller() {
        let fake = FakeCorpus::new().with_delay(Duration::from_secs(1));
        fake.push_search(Ok(page("abandonnée", 1)));
        fake.push_search(Ok(page("reprise", 1)));
        let corpus = Arc::new(cached(fake));
        let q = query("bail");
        let spawn = |corpus: &Arc<CachedCorpus<FakeCorpus>>, q: &SearchQuery| {
            let (corpus, q) = (Arc::clone(corpus), q.clone());
            tokio::spawn(async move { corpus.search(&q).await })
        };
        let leader = spawn(&corpus, &q);
        tokio::time::sleep(Duration::from_millis(10)).await;
        let follower = spawn(&corpus, &q);
        tokio::time::sleep(Duration::from_millis(10)).await;
        leader.abort();
        assert!(leader.await.unwrap_err().is_cancelled());
        // Le suiveur, qui attendait l'initialisation du meneur, la reprend à son compte.
        assert_eq!(follower.await.unwrap(), Ok(page("reprise", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
    }

    #[tokio::test]
    async fn entry_of_unexpected_type_is_evicted_and_not_served() {
        let fake = FakeCorpus::new();
        fake.push_search(Ok(page("a", 1)));
        fake.push_search(Ok(page("b", 1)));
        let corpus = cached(fake);
        let q = query("bail");
        // Une valeur d'un autre type sous la clé de la recherche : impossible par construction,
        // jamais servie si elle arrivait.
        let key = CacheKey { env: SANDBOX, op: Op::Search(q.clone()) };
        corpus.cache.insert(key, CacheValue::new(related("a", 1, 0))).await;
        assert_eq!(corpus.search(&q).await, Ok(page("a", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 1);
        // L'entrée a été retirée : l'appel suivant repart vers le corpus, puis se met en cache.
        assert_eq!(corpus.search(&q).await, Ok(page("b", 1)));
        assert_eq!(corpus.search(&q).await, Ok(page("b", 1)));
        assert_eq!(corpus.inner.calls_of("search"), 2);
    }

    #[tokio::test]
    async fn entries_expire_after_their_family_ttl() {
        // V-08 : moka lit l'horloge système, pas celle de tokio. Ce test à attente réelle (120 ms)
        // prouve seulement que l'`Expiry` est branchée sur le cache et que chaque famille garde
        // sa durée ; la table des durées se teste sans attente (`policy::tests`).
        let policy = CachePolicy {
            search_ttl: Duration::from_millis(30),
            list_ttl: Duration::from_millis(30),
            ..CachePolicy::default()
        };
        let fake = FakeCorpus::new();
        fake.push_search(Ok(page("a", 1)));
        fake.push_search(Ok(page("b", 1)));
        fake.push_get_article(Ok(article("a")));
        fake.push_get_article(Ok(article("b")));
        let corpus = CachedCorpus::new(fake, SANDBOX, policy);
        let q = query("bail");
        assert_eq!(corpus.search(&q).await, Ok(page("a", 1)));
        assert_eq!(corpus.get_article(&article_id()).await, Ok(article("a")));
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(corpus.search(&q).await, Ok(page("b", 1)));
        // La consultation garde ses 24 h : elle est toujours en cache.
        assert_eq!(corpus.get_article(&article_id()).await, Ok(article("a")));
        assert_eq!(corpus.inner.calls_of("search"), 2);
        assert_eq!(corpus.inner.calls_of("get_article"), 1);
    }
}
