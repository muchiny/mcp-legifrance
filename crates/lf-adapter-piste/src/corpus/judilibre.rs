//! `PisteJudilibre` : la jurisprudence judiciaire de Judilibre, lue par l'API PISTE (ADR-0035).
//!
//! `PisteJudilibre` tient un `PisteCorpus` : même jeton, même client, même limiteur, même
//! sémaphore, mêmes réessais, même échéance et même plafond de corps que Légifrance (décision 2).
//! `LF_PISTE_RPS`, `LF_PISTE_BURST` et `LF_PISTE_CONCURRENCY` comptent donc les deux API ensemble.
//! Les requêtes sont des `GET` sur deux chemins (`request::judilibre`), sous les genres
//! `CaseLawSearch` et `CaseLawDecision` de `classify`.
//!
//! Un 403 de l'API (application non abonnée) est retenu 10 minutes : pendant ce temps, aucun appel
//! ne part, jeton compris (décision 9). Une page HTML en 403 est celle de la passerelle, pas un
//! refus d'abonnement : elle n'est pas retenue. Un redémarrage efface ce souvenir.
//!
//! Les erreurs de `CorpusError` deviennent des `CaseLawError` (`case_law_error`) : aucune ne porte
//! l'adresse ni les mots cherchés.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use std::{fmt, future::Future};

use lf_application::ports::{AuthOrigin, CaseLawError, CorpusError, JudicialOpenData};
use lf_domain::judilibre::{CaseLawDoc, CaseLawPage, CaseLawQuery, JudilibreId};
use reqwest::Url;

use super::PisteCorpus;
use crate::classify::EndpointKind;
use crate::http::Payload;
use crate::map::judilibre::{decision, page};
use crate::request::judilibre::{DECISION, SEARCH, decision_params, search_params, url};
use crate::time::{MonoClock, Sleeper, TokioClock, TokioSleeper};

/// Durée pendant laquelle un refus 403 de l'API est retenu (ADR-0035, décision 9).
pub(crate) const REFUSAL_MEMORY: Duration = Duration::from_secs(600);

/// Adapter PISTE de Judilibre : le cœur de [`PisteCorpus`], et le souvenir d'un 403.
pub struct PisteJudilibre<K: MonoClock + Clone = TokioClock, S: Sleeper + Clone = TokioSleeper> {
    corpus: PisteCorpus<K, S>,
    /// Instant monotone du dernier refus 403 de l'API, s'il y en a eu un.
    refused_at: Arc<Mutex<Option<Duration>>>,
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> Clone for PisteJudilibre<K, S> {
    fn clone(&self) -> Self {
        Self { corpus: self.corpus.clone(), refused_at: Arc::clone(&self.refused_at) }
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> fmt::Debug for PisteJudilibre<K, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PisteJudilibre")
            .field("env", &self.corpus.environment())
            .finish_non_exhaustive()
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// L'adapter de Judilibre sur le même cœur que cet adapter : un jeton, un limiteur et un
    /// sémaphore pour les deux API. Aucune E/S.
    #[must_use]
    pub fn judilibre(&self) -> PisteJudilibre<K, S> {
        PisteJudilibre { corpus: self.clone(), refused_at: Arc::default() }
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteJudilibre<K, S> {
    /// Exécute `call`, sauf si un 403 a été reçu il y a moins de [`REFUSAL_MEMORY`] ; retient le
    /// 403 que `call` reçoit.
    async fn guarded<T>(
        &self,
        call: impl Future<Output = Result<T, CorpusError>>,
    ) -> Result<T, CaseLawError> {
        if self.refused_recently() {
            return Err(CaseLawError::NotSubscribed);
        }
        let result = call.await.map_err(case_law_error);
        if matches!(result, Err(CaseLawError::NotSubscribed)) {
            *self.refusal() = Some(self.corpus.inner.clock.now());
        }
        result
    }

    /// Vrai si le dernier 403 date de moins de [`REFUSAL_MEMORY`].
    fn refused_recently(&self) -> bool {
        let now = self.corpus.inner.clock.now();
        self.refusal().is_some_and(|at| now.saturating_sub(at) < REFUSAL_MEMORY)
    }

    /// Le souvenir du dernier 403 ; un verrou empoisonné reste utilisable (une valeur `Copy`).
    fn refusal(&self) -> MutexGuard<'_, Option<Duration>> {
        self.refused_at.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> JudicialOpenData for PisteJudilibre<K, S> {
    async fn search(&self, query: &CaseLawQuery) -> Result<CaseLawPage, CaseLawError> {
        let params = search_params(query);
        let locate = |base: &Url| Ok(url(base, SEARCH, &params));
        let kind = EndpointKind::CaseLawSearch;
        self.guarded(self.corpus.fetch_with(kind, SEARCH, locate, Payload::Get, page)).await
    }

    async fn get(&self, id: &JudilibreId) -> Result<CaseLawDoc, CaseLawError> {
        let params = decision_params(id);
        let locate = |base: &Url| Ok(url(base, DECISION, &params));
        let read = |dto, diags: &mut _| decision(id, dto, diags);
        let kind = EndpointKind::CaseLawDecision;
        self.guarded(self.corpus.fetch_with(kind, DECISION, locate, Payload::Get, read)).await
    }
}

/// Erreur du port de Judilibre pour une erreur du cœur partagé.
///
/// - 403 de l'API hors page HTML : [`CaseLawError::NotSubscribed`] ;
/// - 400 (et 403 de la passerelle), 416, 423 : [`CaseLawError::Rejected`], jamais réessayés ;
/// - le reste garde sa nature, son statut et son détail de journal (texte fixe, sans adresse).
fn case_law_error(error: CorpusError) -> CaseLawError {
    match error {
        CorpusError::NotFound { .. } => CaseLawError::NotFound,
        CorpusError::InvalidRequest { status, .. } => CaseLawError::Rejected { status },
        CorpusError::Upstream { status: Some(status @ (416 | 423)), .. } => {
            CaseLawError::Rejected { status: Some(status) }
        }
        CorpusError::Upstream { status, retryable, detail } => {
            CaseLawError::Upstream { status, retryable, detail }
        }
        CorpusError::Timeout => CaseLawError::Timeout,
        CorpusError::RateLimited { retry_after } => CaseLawError::RateLimited { retry_after },
        CorpusError::Auth { origin: AuthOrigin::Api, status: Some(403), .. } => {
            CaseLawError::NotSubscribed
        }
        CorpusError::Auth { origin, status, detail } => {
            CaseLawError::Auth { origin, status, detail }
        }
        CorpusError::Config { kind, detail } => CaseLawError::Config { kind, detail },
        CorpusError::Decode { detail } => CaseLawError::Decode { detail },
        CorpusError::TooLarge { detail } => CaseLawError::TooLarge { detail },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::num::NonZeroU32;
    use std::time::Duration;

    use lf_application::ports::ConfigKind;
    use lf_domain::judilibre::{CaseLawFilters, CaseLawSort, CourtSet};
    use lf_domain::search::{MatchMode, PageSize, QueryText};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;
    use crate::corpus::tests::{API_BASE, Rig, TOKEN_PATH, json_reply, rate, secs, token_mock};
    use crate::request::judilibre::{DECISION_PARAMS, SEARCH_PARAMS};
    use crate::testing::capture_at;

    const CC_ID: &str = "5fca624b7eeb384a40cafb85";
    /// Mot cherché qu'aucune erreur ni aucun journal ne doit recopier.
    const WORDS: &str = "zorglub usufruit";

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/judilibre")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn query() -> CaseLawQuery {
        let filters = CaseLawFilters {
            mode: MatchMode::All,
            courts: CourtSet::ALL,
            part: None,
            date_from: None,
            date_to: None,
            sort: CaseLawSort::Relevance,
        };
        CaseLawQuery::new(
            QueryText::parse(WORDS).unwrap(),
            filters,
            NonZeroU32::new(2).unwrap(),
            PageSize::DEFAULT,
        )
        .unwrap()
    }

    fn id() -> JudilibreId {
        JudilibreId::parse(CC_ID).unwrap()
    }

    /// `route` de Judilibre répond `template` à volonté.
    async fn judilibre_always(server: &MockServer, route: &str, template: ResponseTemplate) {
        Mock::given(method("GET")).and(path(route)).respond_with(template).mount(server).await;
    }

    /// `route` de Judilibre répond `template` `times` fois exactement, dans l'ordre de montage.
    async fn judilibre_mock(
        server: &MockServer,
        route: &str,
        template: ResponseTemplate,
        times: u64,
    ) {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(template)
            .up_to_n_times(times)
            .expect(times)
            .mount(server)
            .await;
    }

    async fn received(server: &MockServer, route: &str) -> Vec<Request> {
        let requests = server.received_requests().await.unwrap();
        requests.into_iter().filter(|request| request.url.path() == route).collect()
    }

    /// Noms des paramètres d'une requête reçue.
    fn names(request: &Request) -> BTreeSet<String> {
        request.url.query_pairs().map(|(name, _)| name.into_owned()).collect()
    }

    /// G10 : deux chemins, une liste fermée de paramètres, `jurisdiction` toujours présent, un
    /// `GET` sans corps avec le jeton de PISTE.
    #[tokio::test]
    async fn a_search_is_a_closed_get_with_every_court() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        judilibre_mock(&rig.server, SEARCH, json_reply(200, &fixture("search-cc.json")), 1).await;
        let page = rig.corpus.judilibre().search(&query()).await.unwrap();
        assert_eq!((page.total, page.hits.len()), (240, 3));
        let requests = received(&rig.server, SEARCH).await;
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method.as_str(), "GET");
        assert_eq!(request.body, Vec::<u8>::new());
        assert_eq!(request.headers.get("authorization").unwrap(), "Bearer tok-1");
        assert!(names(request).iter().all(|name| SEARCH_PARAMS.contains(&name.as_str())));
        let pairs: Vec<(String, String)> = request.url.query_pairs().into_owned().collect();
        let courts: Vec<&str> = pairs
            .iter()
            .filter(|(name, _)| name == "jurisdiction")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(courts, ["cc", "ca", "tj", "tcom", "cph"]);
        assert!(pairs.contains(&("query".to_owned(), WORDS.to_owned())));
        assert!(pairs.contains(&("page".to_owned(), "1".to_owned())));
        assert!(pairs.contains(&("resolve_references".to_owned(), "true".to_owned())));
        // Aucune autre route : ni Légifrance, ni une autre route de Judilibre.
        let all = rig.server.received_requests().await.unwrap();
        let routes: BTreeSet<&str> = all.iter().map(|request| request.url.path()).collect();
        assert_eq!(routes, BTreeSet::from([TOKEN_PATH, SEARCH]));
    }

    #[tokio::test]
    async fn a_decision_is_read_by_its_id_in_plain_words() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        judilibre_mock(&rig.server, DECISION, json_reply(200, &fixture("decision-cc.json")), 1)
            .await;
        let doc = rig.corpus.judilibre().get(&id()).await.unwrap();
        assert_eq!(doc.id, id());
        assert!(doc.text.starts_with("CIV. 1"));
        let requests = received(&rig.server, DECISION).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            names(&requests[0]),
            BTreeSet::from(["id".to_owned(), "resolve_references".to_owned()])
        );
        assert!(names(&requests[0]).iter().all(|name| DECISION_PARAMS.contains(&name.as_str())));
        assert_eq!(
            requests[0].url.query(),
            Some("id=5fca624b7eeb384a40cafb85&resolve_references=true")
        );
    }

    #[tokio::test]
    async fn another_decision_than_the_requested_one_is_a_decode_error() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        judilibre_always(&rig.server, DECISION, json_reply(200, &fixture("decision-ca.json")))
            .await;
        let error = rig.corpus.judilibre().get(&id()).await.unwrap_err();
        assert_eq!(
            error,
            CaseLawError::Decode { detail: format!("{DECISION}: id: not the requested decision") }
        );
        assert_eq!(rig.corpus.stats().id_mismatch, 1);
    }

    /// ADR-0035, décision 9 : un 403 de l'API est retenu 10 minutes, sans aucune requête
    /// pendant ce temps (jeton compris) ; au bout de 10 minutes, l'appel suivant repart.
    #[test]
    fn the_refusal_is_remembered_ten_minutes() {
        assert_eq!(REFUSAL_MEMORY, Duration::from_secs(600));
    }

    #[tokio::test]
    async fn forbidden_is_remembered_ten_minutes() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        judilibre_mock(&rig.server, SEARCH, json_reply(403, r#"{"error":"forbidden"}"#), 1).await;
        let judilibre = rig.corpus.judilibre();
        assert_eq!(judilibre.search(&query()).await, Err(CaseLawError::NotSubscribed));
        rig.clock.advance(Duration::from_millis(599_999));
        // Les deux méthodes et les clones partagent le souvenir.
        assert_eq!(judilibre.clone().get(&id()).await, Err(CaseLawError::NotSubscribed));
        assert_eq!(judilibre.search(&query()).await, Err(CaseLawError::NotSubscribed));
        assert_eq!(rig.server.received_requests().await.unwrap().len(), 2);
        rig.clock.advance(Duration::from_millis(1));
        judilibre_mock(&rig.server, SEARCH, json_reply(200, &fixture("search-ca.json")), 1).await;
        assert_eq!(judilibre.search(&query()).await.unwrap().total, 58);
        // Un autre adapter tiré du même cœur repart de zéro : le souvenir est le sien.
        let fresh = rig.corpus.judilibre();
        assert!(!fresh.refused_recently());
    }

    /// Une page HTML en 403 est celle de la passerelle : refus de la requête, non retenu.
    #[tokio::test]
    async fn a_gateway_page_in_403_is_not_remembered() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let page = ResponseTemplate::new(403).set_body_raw("<html>Forbidden</html>", "text/html");
        judilibre_mock(&rig.server, SEARCH, page, 1).await;
        judilibre_mock(&rig.server, SEARCH, json_reply(200, &fixture("search-ca.json")), 1).await;
        let judilibre = rig.corpus.judilibre();
        assert_eq!(
            judilibre.search(&query()).await,
            Err(CaseLawError::Rejected { status: Some(403) })
        );
        assert_eq!(judilibre.search(&query()).await.unwrap().total, 58);
    }

    #[tokio::test]
    async fn blocked_423_is_not_retried() {
        for status in [400, 416, 423] {
            let rig = Rig::new().await;
            token_mock(&rig.server, "tok-1", 1).await;
            judilibre_mock(&rig.server, SEARCH, json_reply(status, &fixture("error-416.json")), 1)
                .await;
            let error = rig.corpus.judilibre().search(&query()).await.unwrap_err();
            assert_eq!(error, CaseLawError::Rejected { status: Some(status) });
            assert_eq!(rig.sleeper.waits(), Vec::<Duration>::new());
        }
    }

    /// 429 et 5xx : les règles de Légifrance, réessais compris.
    #[tokio::test]
    async fn rate_limits_and_server_errors_follow_the_rules_in_place() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let busy = ResponseTemplate::new(429).insert_header("retry-after", "2");
        judilibre_mock(&rig.server, DECISION, busy, 1).await;
        judilibre_mock(&rig.server, DECISION, ResponseTemplate::new(503), 1).await;
        judilibre_mock(&rig.server, DECISION, json_reply(200, &fixture("decision-cc.json")), 1)
            .await;
        assert_eq!(rig.corpus.judilibre().get(&id()).await.unwrap().id, id());
        assert_eq!(rig.sleeper.waits()[0], secs(2));
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        judilibre_always(&rig.server, DECISION, ResponseTemplate::new(500)).await;
        let error = rig.corpus.judilibre().get(&id()).await.unwrap_err();
        assert_eq!(
            error,
            CaseLawError::Upstream {
                status: Some(500),
                retryable: false,
                detail: format!("HTTP 500 on {DECISION}"),
            }
        );
    }

    /// Sur un hôte de test, un 404 dit une adresse erronée, jamais une décision absente.
    #[tokio::test]
    async fn a_404_of_a_custom_host_points_at_the_address() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        judilibre_always(&rig.server, DECISION, json_reply(404, "{}")).await;
        assert_eq!(
            rig.corpus.judilibre().get(&id()).await,
            Err(CaseLawError::Config {
                kind: ConfigKind::ApiEndpoint,
                detail: format!("HTTP 404 on {DECISION}"),
            })
        );
    }

    #[tokio::test]
    async fn without_credentials_nothing_is_sent() {
        let rig = Rig::tuned(|setup| setup.credentials = None).await;
        let judilibre = rig.corpus.judilibre();
        let missing = CaseLawError::Config {
            kind: ConfigKind::CredentialsMissing,
            detail: crate::corpus::CREDENTIALS_MISSING.to_owned(),
        };
        assert_eq!(judilibre.search(&query()).await, Err(missing.clone()));
        assert_eq!(judilibre.get(&id()).await, Err(missing));
        assert_eq!(rig.server.received_requests().await.unwrap().len(), 0);
    }

    /// Aucune erreur ni aucun journal ne recopie un mot de la recherche (ADR-0035, décision 9).
    #[tokio::test]
    async fn errors_never_echo_the_query() {
        let echo = format!(r#"{{"message":"no result for {WORDS}","query":"{WORDS}"}}"#);
        for status in [400, 403, 404, 416, 423, 500, 502, 200] {
            let rig = Rig::tuned(|setup| setup.tuning.rate = rate(100, 100)).await;
            token_mock(&rig.server, "tok-1", 1).await;
            // Le 200 rend un corps qui n'est pas une page : erreur de décodage.
            judilibre_always(&rig.server, SEARCH, json_reply(status, &echo)).await;
            let (capture, _guard) = capture_at(tracing::Level::TRACE);
            let error = rig.corpus.judilibre().search(&query()).await.unwrap_err();
            for shown in [format!("{error:?}"), error.to_string(), capture.text()] {
                assert!(!shown.contains("zorglub"), "{status}: {shown}");
            }
        }
    }

    /// G12 : Légifrance et Judilibre passent par un seul limiteur, un seul sémaphore et un seul
    /// jeton.
    #[tokio::test]
    async fn mixed_calls_share_one_limiter() {
        let rig = Rig::tuned(|setup| setup.tuning.rate = rate(1, 1)).await;
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}/consult/juri")))
            .respond_with(json_reply(200, "{}"))
            .expect(1)
            .mount(&rig.server)
            .await;
        judilibre_mock(&rig.server, SEARCH, json_reply(200, &fixture("search-ca.json")), 1).await;
        let legifrance = rig
            .corpus
            .post_json(EndpointKind::Consult, "/consult/juri", b"{}", |text| Ok(text.to_owned()))
            .await;
        assert_eq!(legifrance, Ok("{}".to_owned()));
        let judilibre = rig.corpus.judilibre();
        assert_eq!(judilibre.search(&query()).await.unwrap().total, 58);
        // Le second appel, de l'autre API, attend le jeton du limiteur commun.
        assert_eq!(rig.sleeper.waits(), [secs(1)]);
        assert!(Arc::ptr_eq(&rig.corpus.inner, &judilibre.corpus.inner));
    }

    #[test]
    fn every_corpus_error_has_its_case_law_error() {
        let detail = || "HTTP 503 on /cassation/judilibre/v1.0/search".to_owned();
        let cases = [
            (CorpusError::NotFound { what: DECISION.to_owned() }, CaseLawError::NotFound),
            (
                CorpusError::InvalidRequest { status: None, detail: detail() },
                CaseLawError::Rejected { status: None },
            ),
            (
                CorpusError::Upstream { status: Some(423), retryable: false, detail: detail() },
                CaseLawError::Rejected { status: Some(423) },
            ),
            (
                CorpusError::Upstream { status: Some(503), retryable: true, detail: detail() },
                CaseLawError::Upstream { status: Some(503), retryable: true, detail: detail() },
            ),
            (
                CorpusError::Upstream { status: None, retryable: true, detail: detail() },
                CaseLawError::Upstream { status: None, retryable: true, detail: detail() },
            ),
            (CorpusError::Timeout, CaseLawError::Timeout),
            (
                CorpusError::RateLimited { retry_after: Some(secs(3)) },
                CaseLawError::RateLimited { retry_after: Some(secs(3)) },
            ),
            (
                CorpusError::Auth { origin: AuthOrigin::Api, status: Some(403), detail: detail() },
                CaseLawError::NotSubscribed,
            ),
            (
                CorpusError::Auth {
                    origin: AuthOrigin::Token,
                    status: Some(403),
                    detail: detail(),
                },
                CaseLawError::Auth {
                    origin: AuthOrigin::Token,
                    status: Some(403),
                    detail: detail(),
                },
            ),
            (
                CorpusError::Auth { origin: AuthOrigin::Api, status: Some(401), detail: detail() },
                CaseLawError::Auth { origin: AuthOrigin::Api, status: Some(401), detail: detail() },
            ),
            (
                CorpusError::Config { kind: ConfigKind::Settings, detail: detail() },
                CaseLawError::Config { kind: ConfigKind::Settings, detail: detail() },
            ),
            (CorpusError::Decode { detail: detail() }, CaseLawError::Decode { detail: detail() }),
            (
                CorpusError::TooLarge { detail: detail() },
                CaseLawError::TooLarge { detail: detail() },
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(case_law_error(error), expected);
        }
    }

    #[tokio::test]
    async fn debug_shows_the_environment_only() {
        let rig = Rig::new().await;
        let judilibre = rig.corpus.judilibre();
        assert_eq!(format!("{judilibre:?}"), "PisteJudilibre { env: Sandbox, .. }");
    }
}
