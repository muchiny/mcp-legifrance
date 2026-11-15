//! [`BofipDoctrine`] : le port `TaxDoctrine` servi par le portail ouvert de la DGFiP.
//!
//! Un appel suit toujours le même chemin : l'adresse est bâtie à partir de valeurs typées
//! (`query`), l'appel attend son tour (`pace`), un seul `GET` part (`http`), le statut est jugé
//! ici, et le corps d'une réponse 200 est contrôlé avant de devenir une valeur du domaine
//! (`mapping`). Aucun réessai, aucun cache.

use std::time::Duration;

use lf_application::{
    deadline::Deadline,
    ports::{Clock, DoctrineError, TaxDoctrine},
};
use lf_domain::doctrine::{BoiId, DoctrineDoc, DoctrinePage, DoctrineQuery};
use reqwest::{Client, Url};
use tokio::sync::OnceCell;

use crate::{
    http::{self, GET_MAX_BYTES, Reply, SEARCH_MAX_BYTES},
    mapping::{self, Anomalies, Decoded},
    pace::Pace,
    query,
    time::{Timer, TokioTimer},
};

/// La seule adresse que le client de production appelle : les enregistrements du jeu
/// `bofip-vigueur`. Aucune variable ne la change (ADR-0028, décision 4).
pub const RECORDS_URL: &str =
    "https://data.economie.gouv.fr/api/explore/v2.1/catalog/datasets/bofip-vigueur/records";

/// Réglages du client, posés par le binaire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BofipConfig {
    user_agent: String,
    request_timeout: Duration,
    tool_deadline: Duration,
}

impl BofipConfig {
    /// Réglages : `user_agent` (le même que le client PISTE), `request_timeout` (délai d'une
    /// requête, `LF_HTTP_TIMEOUT_S`) et `tool_deadline` (échéance d'un appel d'outil quand
    /// aucune n'est publiée, `LF_TOOL_DEADLINE_S`).
    pub fn new(user_agent: String, request_timeout: Duration, tool_deadline: Duration) -> Self {
        Self { user_agent, request_timeout, tool_deadline }
    }
}

/// Les deux requêtes que le client sait faire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Recherche de publications.
    Search,
    /// Lecture des publications d'un identifiant.
    Get,
}

impl Route {
    /// Gabarit de la requête : la seule trace de l'adresse dans les journaux et les erreurs.
    const fn template(self) -> &'static str {
        match self {
            Self::Search => "records?search",
            Self::Get => "records?get",
        }
    }

    /// Taille maximale du corps d'une réponse 200, après décompression.
    const fn max_bytes(self) -> u64 {
        match self {
            Self::Search => SEARCH_MAX_BYTES,
            Self::Get => GET_MAX_BYTES,
        }
    }
}

/// Client du jeu `bofip-vigueur` : la doctrine fiscale en vigueur, lue en direct.
///
/// `new` ne fait aucune E/S : le client HTTP naît au premier appel. Le débit (`Pace`) est
/// porté par l'instance : une instance par processus.
pub struct BofipDoctrine<W: Clock, T: Timer = TokioTimer> {
    config: BofipConfig,
    /// Adresse des enregistrements : [`RECORDS_URL`], sauf pour une base de test.
    base: String,
    /// Vrai pour une base de test seulement : le client admet alors `http`.
    plain_http: bool,
    client: OnceCell<Client>,
    pace: Pace<W, T>,
}

impl<W: Clock> BofipDoctrine<W> {
    /// Client de production : [`RECORDS_URL`], `https` seul. `clock` donne le jour UTC du budget
    /// de requêtes.
    pub fn new(config: BofipConfig, clock: W) -> Self {
        Self::with_parts(RECORDS_URL.to_owned(), false, config, clock, TokioTimer::start())
    }
}

impl<W: Clock, T: Timer> BofipDoctrine<W, T> {
    /// Assemble le client.
    fn with_parts(base: String, plain_http: bool, config: BofipConfig, clock: W, timer: T) -> Self {
        Self { config, base, plain_http, client: OnceCell::new(), pace: Pace::new(clock, timer) }
    }

    /// Client d'une base de test, en `http` : la seule façon de viser un autre hôte, compilée
    /// pour les tests de ce crate seulement (ADR-0029, décision 6).
    #[cfg(test)]
    pub(crate) fn for_test(base: &str, config: BofipConfig, clock: W, timer: T) -> Self {
        Self::with_parts(base.to_owned(), true, config, clock, timer)
    }

    /// Un appel : attend son tour, envoie un seul `GET`, juge le statut, décode.
    async fn call<R>(
        &self,
        url: Url,
        route: Route,
        decode: impl FnOnce(&[u8]) -> Result<Decoded<R>, DoctrineError>,
    ) -> Result<R, DoctrineError> {
        let timer = self.pace.timer();
        let asked = timer.now();
        let budget = Deadline::current_or(self.config.tool_deadline);
        // Gardé jusqu'à la fin du décodage : deux lectures en vol au plus.
        let _permit = self.pace.acquire(budget).await?;
        let left = budget.saturating_sub(timer.now().saturating_sub(asked));
        let client = self.client().await?;
        let timeout = self.config.request_timeout.min(left);
        let reply = http::send_once(client, url, timeout, route.max_bytes()).await?;
        tracing::debug!(
            route = route.template(),
            status = reply.status,
            bytes = reply.body.len(),
            "BOFiP portal answered"
        );
        self.settle(route, &reply, decode)
    }

    /// Le client HTTP, construit au premier appel. Un échec n'est pas mémorisé.
    async fn client(&self) -> Result<&Client, DoctrineError> {
        self.client
            .get_or_try_init(|| async {
                http::build_client(
                    &self.config.user_agent,
                    self.config.request_timeout,
                    self.plain_http,
                )
            })
            .await
    }

    /// Juge une réponse : seul un statut 200 est décodé. Un refus 429 ouvre le repos de la
    /// source ; aucun statut n'est réessayé.
    fn settle<R>(
        &self,
        route: Route,
        reply: &Reply,
        decode: impl FnOnce(&[u8]) -> Result<Decoded<R>, DoctrineError>,
    ) -> Result<R, DoctrineError> {
        match reply.status {
            200 => {
                let decoded = decode(&reply.body)?;
                report(route, decoded.anomalies);
                Ok(decoded.value)
            }
            400 => Err(DoctrineError::InvalidRequest {
                status: 400,
                code: http::refusal_code(&reply.body),
            }),
            429 => {
                let rest = self.pace.rest(http::retry_after(reply.retry_after.as_deref()));
                Err(DoctrineError::RateLimited { retry_after: Some(rest) })
            }
            status => Err(DoctrineError::Upstream {
                status: Some(status),
                retryable: (500..=599).contains(&status),
                detail: format!("HTTP {status} on {}", route.template()),
            }),
        }
    }
}

/// Signale aux journaux ce qu'une réponse acceptée avait d'inattendu : des compteurs, jamais des
/// valeurs.
fn report(route: Route, anomalies: Anomalies) {
    if !anomalies.is_empty() {
        tracing::warn!(
            route = route.template(),
            unknown_entities = anomalies.unknown_entities,
            dropped_fields = anomalies.dropped_fields,
            "BOFiP answer accepted with anomalies"
        );
    }
}

impl<W: Clock, T: Timer> TaxDoctrine for BofipDoctrine<W, T> {
    async fn search(&self, query: &DoctrineQuery) -> Result<DoctrinePage, DoctrineError> {
        let url = query::search_url(&self.base, query)?;
        let limit = query.page_size().get();
        self.call(url, Route::Search, |body| mapping::page(body, limit, query.kind())).await
    }

    async fn get(&self, id: &BoiId) -> Result<Vec<DoctrineDoc>, DoctrineError> {
        let url = query::get_url(&self.base, id)?;
        self.call(url, Route::Get, |body| mapping::docs(body, id)).await
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_application::ports::PortalCode;
    use lf_domain::{
        doctrine::{DoctrineTerms, KindFilter, Serie},
        search::PageSize,
    };
    use pretty_assertions::assert_eq;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use super::*;
    use crate::{
        pace::DAILY_BUDGET,
        query::{GET_SELECT, SEARCH_SELECT},
        testing::{
            MIDDAY, TestWall, closed_port, fixture, gzip, journal, raw_server, start_journal,
        },
        time::ManualTimer,
    };

    const AGENT: &str = "mcp-legifrance-tests/0";
    const ID: &str = "BOI-ENR-DMTG-10-50-20";
    /// Mot que rien, hors de la requête, ne doit jamais porter.
    const SENTINEL: &str = "zzsentinellezz";

    type TestDoctrine = BofipDoctrine<TestWall, ManualTimer>;

    const fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn config() -> BofipConfig {
        BofipConfig::new(AGENT.to_owned(), s(20), s(60))
    }

    fn id() -> BoiId {
        BoiId::parse(ID).unwrap()
    }

    fn query(words: &str) -> DoctrineQuery {
        DoctrineQuery::new(
            DoctrineTerms::parse(words).unwrap(),
            None,
            KindFilter::Doctrine,
            NonZeroU32::MIN,
            PageSize::DEFAULT,
        )
        .unwrap()
    }

    /// Client de test sur `base`, avec son minuteur.
    fn doctrine_on(base: &str, config: BofipConfig) -> (TestDoctrine, ManualTimer) {
        let timer = ManualTimer::default();
        let records = format!("{base}/records");
        (BofipDoctrine::for_test(&records, config, TestWall::at(MIDDAY), timer.clone()), timer)
    }

    /// Faux portail qui répond `template` à tout `GET /records`, et son client.
    async fn portal(template: ResponseTemplate) -> (MockServer, TestDoctrine, ManualTimer) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/records"))
            .respond_with(template)
            .mount(&server)
            .await;
        let (doctrine, timer) = doctrine_on(&server.uri(), config());
        (server, doctrine, timer)
    }

    fn ok(name: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_bytes(fixture(name))
    }

    async fn requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    fn limited(seconds: u64) -> DoctrineError {
        DoctrineError::RateLimited { retry_after: Some(s(seconds)) }
    }

    // ---- les deux requêtes -----------------------------------------------------------------------

    #[tokio::test]
    async fn a_search_sends_the_typed_query_and_decodes_the_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/records"))
            .and(query_param("select", SEARCH_SELECT))
            .and(query_param(
                "where",
                r#"search(contenu, "donation abattement") and type = "Contenu" and serie = "ENR""#,
            ))
            .and(query_param("limit", "20"))
            .and(query_param("offset", "40"))
            .respond_with(ok("search-donation-page1.json"))
            .expect(1)
            .mount(&server)
            .await;
        let (doctrine, _timer) = doctrine_on(&server.uri(), config());
        let query = DoctrineQuery::new(
            DoctrineTerms::parse("donation, abattement !").unwrap(),
            Some(Serie::parse("enr").unwrap()),
            KindFilter::Doctrine,
            NonZeroU32::new(3).unwrap(),
            PageSize::new(20).unwrap(),
        )
        .unwrap();
        let page = doctrine.search(&query).await.unwrap();
        assert_eq!((page.total, page.hits.len()), (433, 10));
        assert_eq!(page.hits[0].id.as_str(), "BOI-ENR-DMTG-20-30-20-30");
        assert_eq!(server.received_requests().await.unwrap()[0].url.query_pairs().count(), 4);
    }

    #[tokio::test]
    async fn a_read_sends_the_identifier_and_decodes_the_publication() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/records"))
            .and(query_param("select", GET_SELECT))
            .and(query_param("where", r#"identifiant_juridique = "BOI-ENR-DMTG-10-50-20""#))
            .and(query_param("limit", "10"))
            .respond_with(ok("get-single.json"))
            .expect(1)
            .mount(&server)
            .await;
        let (doctrine, _timer) = doctrine_on(&server.uri(), config());
        let docs = doctrine.get(&id()).await.unwrap();
        assert_eq!((docs.len(), docs[0].publication_id.get()), (1, 3369));
        assert_eq!(server.received_requests().await.unwrap()[0].url.query_pairs().count(), 3);
    }

    #[tokio::test]
    async fn an_unknown_identifier_is_not_found_and_an_empty_search_is_a_result() {
        let (_server, doctrine, _timer) = portal(ok("get-unknown.json")).await;
        assert_eq!(doctrine.get(&id()).await, Err(DoctrineError::NotFound));
        assert_eq!(
            doctrine.search(&query("donation")).await,
            Ok(DoctrinePage { total: 0, hits: Vec::new() })
        );
    }

    /// Le contrôle de la réponse joue de bout en bout : une publication d'un autre identifiant,
    /// ou une page plus longue que demandé, fait échouer l'appel.
    #[tokio::test]
    async fn a_non_conforming_answer_fails_the_call() {
        let (_server, doctrine, _timer) = portal(ok("get-double-same-date.json")).await;
        assert_eq!(
            doctrine.get(&id()).await,
            Err(DoctrineError::Decode {
                detail: "returned identifier differs from the requested one".to_owned()
            })
        );
        let (_server, doctrine, _timer) = portal(ok("search-donation-page1.json")).await;
        let short = DoctrineQuery::new(
            DoctrineTerms::parse("donation").unwrap(),
            None,
            KindFilter::Doctrine,
            NonZeroU32::MIN,
            PageSize::new(9).unwrap(),
        )
        .unwrap();
        assert_eq!(
            doctrine.search(&short).await,
            Err(DoctrineError::Decode {
                detail: "more results than the requested limit".to_owned()
            })
        );
        // Une recherche de doctrine seule refuse une page qui porte des actualités.
        let (_server, doctrine, _timer) = portal(ok("search-all.json")).await;
        assert_eq!(
            doctrine.search(&query("donation")).await,
            Err(DoctrineError::Decode {
                detail: "search result outside the requested kind".to_owned()
            })
        );
    }

    // ---- production ------------------------------------------------------------------------------

    #[test]
    fn the_production_source_targets_the_fixed_https_address() {
        let doctrine = BofipDoctrine::new(config(), TestWall::at(MIDDAY));
        assert_eq!((doctrine.base.as_str(), doctrine.plain_http), (RECORDS_URL, false));
        assert_eq!(doctrine.config, config());
        assert!(!doctrine.client.initialized());
        assert_eq!(
            RECORDS_URL,
            "https://data.economie.gouv.fr/api/explore/v2.1/catalog/datasets/bofip-vigueur/records"
        );
        // Une base de test, elle, admet `http`.
        let (test, _timer) = doctrine_on("http://127.0.0.1:9", config());
        assert_eq!((test.base.as_str(), test.plain_http), ("http://127.0.0.1:9/records", true));
    }

    #[test]
    fn each_route_has_its_template_and_its_limit() {
        assert_eq!(
            [Route::Search, Route::Get].map(|route| (route.template(), route.max_bytes())),
            [("records?search", 1_048_576), ("records?get", 4_194_304)]
        );
    }

    /// La borne du corps dépend de la requête : 1 Mio pour une recherche, 4 Mio pour une lecture.
    #[tokio::test]
    async fn the_body_limit_follows_the_route() {
        let body = vec![b' '; 1_048_577];
        let (_server, doctrine, _timer) =
            portal(ResponseTemplate::new(200).set_body_bytes(body)).await;
        assert_eq!(
            doctrine.search(&query("donation")).await,
            Err(DoctrineError::TooLarge { detail: "response body over 1048576 bytes".to_owned() })
        );
        // Le même corps passe la borne d'une lecture : il échoue plus loin, au décodage.
        assert_eq!(
            doctrine.get(&id()).await,
            Err(DoctrineError::Decode {
                detail: "get response is not the expected JSON (line 1, column 1048577)".to_owned()
            })
        );
    }

    #[tokio::test]
    async fn the_client_is_built_at_the_first_call_and_kept() {
        let (server, doctrine, _timer) = portal(ok("search-empty.json")).await;
        assert!(!doctrine.client.initialized());
        assert!(doctrine.search(&query("donation")).await.is_ok());
        assert!(doctrine.client.initialized());
        let first = std::ptr::from_ref(doctrine.client().await.unwrap());
        assert!(doctrine.search(&query("donation")).await.is_ok());
        assert_eq!(std::ptr::from_ref(doctrine.client().await.unwrap()), first);
        assert_eq!(requests(&server).await, 2);
    }

    #[tokio::test]
    async fn a_client_that_cannot_be_built_fails_the_call_without_any_request() {
        let server = MockServer::start().await;
        let bad = BofipConfig::new("agent\nsur deux lignes".to_owned(), s(20), s(60));
        let (doctrine, _timer) = doctrine_on(&server.uri(), bad);
        let expected = DoctrineError::Upstream {
            status: None,
            retryable: false,
            detail: "could not build the HTTP client".to_owned(),
        };
        assert_eq!(doctrine.get(&id()).await, Err(expected.clone()));
        // L'échec n'est pas mémorisé : l'appel suivant réessaie de construire.
        assert_eq!(doctrine.search(&query("donation")).await, Err(expected));
        assert_eq!((requests(&server).await, doctrine.client.initialized()), (0, false));
    }

    #[tokio::test]
    async fn a_base_that_is_not_an_address_fails_before_any_wait() {
        let (doctrine, timer) = doctrine_on("pas une adresse", config());
        let expected = DoctrineError::Upstream {
            status: None,
            retryable: false,
            detail: "the portal address is not a URL".to_owned(),
        };
        assert_eq!(doctrine.get(&id()).await, Err(expected.clone()));
        assert_eq!(doctrine.search(&query("donation")).await, Err(expected));
        assert_eq!(timer.sleeps(), Vec::new());
    }

    // ---- statuts ---------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_400_keeps_only_a_well_formed_error_code() {
        let template = ResponseTemplate::new(400).set_body_bytes(fixture("error-400-syntax.json"));
        let (_server, doctrine, _timer) = portal(template).await;
        let error = doctrine.search(&query("donation")).await.unwrap_err();
        assert_eq!(
            error,
            DoctrineError::InvalidRequest {
                status: 400,
                code: PortalCode::parse("ODSQLSyntaxError")
            }
        );
        // Le message du portail, qui recopie la requête, n'est nulle part.
        assert!(!format!("{error} {error:?}").contains("search(contenu"));

        let hostile = br#"{"error_code": "search(contenu, \"donation\")", "message": "m"}"#;
        let (_server, doctrine, _timer) =
            portal(ResponseTemplate::new(400).set_body_bytes(hostile.to_vec())).await;
        assert_eq!(
            doctrine.get(&id()).await,
            Err(DoctrineError::InvalidRequest { status: 400, code: None })
        );
    }

    #[tokio::test]
    async fn other_statuses_are_upstream_errors_and_only_5xx_are_worth_a_retry() {
        let rows = [
            (204, false),
            (302, false),
            (401, false),
            (403, false),
            (404, false),
            (499, false),
            (500, true),
            (503, true),
            (599, true),
        ];
        for (status, retryable) in rows {
            let (server, doctrine, _timer) = portal(ResponseTemplate::new(status)).await;
            assert_eq!(
                doctrine.search(&query("donation")).await,
                Err(DoctrineError::Upstream {
                    status: Some(status),
                    retryable,
                    detail: format!("HTTP {status} on records?search"),
                })
            );
            assert_eq!(
                doctrine.get(&id()).await,
                Err(DoctrineError::Upstream {
                    status: Some(status),
                    retryable,
                    detail: format!("HTTP {status} on records?get"),
                })
            );
            // Jamais de réessai : une requête par appel.
            assert_eq!(requests(&server).await, 2, "{status}");
        }
    }

    // ---- débit et quota --------------------------------------------------------------------------

    #[tokio::test]
    async fn second_call_after_429_stays_local() {
        let template = ResponseTemplate::new(429).insert_header("retry-after", "120");
        let (server, doctrine, timer) = portal(template).await;
        assert_eq!(doctrine.search(&query("donation")).await, Err(limited(120)));
        assert_eq!(requests(&server).await, 1);
        // Pendant le repos, recherche et lecture sont refusées sans réseau, avec ce qu'il reste.
        timer.advance(s(20));
        assert_eq!(doctrine.search(&query("donation")).await, Err(limited(100)));
        assert_eq!(doctrine.get(&id()).await, Err(limited(100)));
        timer.advance(s(99));
        assert_eq!(doctrine.get(&id()).await, Err(limited(1)));
        assert_eq!(requests(&server).await, 1);
        // Le repos fini, l'appel suivant repart vers le portail.
        timer.advance(s(1));
        assert_eq!(doctrine.get(&id()).await, Err(limited(120)));
        assert_eq!(requests(&server).await, 2);
    }

    #[tokio::test]
    async fn the_rest_after_a_429_is_bounded() {
        // Zéro seconde n'annonce rien d'utilisable : un quart d'heure, comme sans en-tête.
        let rows = [
            (Some("86400"), 3_600),
            (Some("3600"), 3_600),
            (Some("1"), 1),
            (Some("0"), 900),
            (None, 900),
        ];
        for (header, rest) in rows {
            let template = header.iter().fold(ResponseTemplate::new(429), |template, value| {
                template.insert_header("retry-after", *value)
            });
            let (_server, doctrine, _timer) = portal(template).await;
            assert_eq!(doctrine.get(&id()).await, Err(limited(rest)), "{header:?}");
        }
        // Une date HTTP n'est pas lue : un quart d'heure, comme sans en-tête.
        let dated = ResponseTemplate::new(429)
            .insert_header("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT");
        let (_server, doctrine, _timer) = portal(dated).await;
        assert_eq!(doctrine.get(&id()).await, Err(limited(900)));
    }

    #[tokio::test]
    async fn an_exhausted_daily_budget_sends_nothing() {
        let (server, doctrine, _timer) = portal(ok("search-empty.json")).await;
        for _ in 0..DAILY_BUDGET {
            drop(doctrine.pace.acquire(s(60)).await.unwrap());
            // Rend la main : sans cela, tokio met en attente un futur qui n'a jamais cédé, et le
            // minuteur de test, toujours prêt, y verrait une échéance dépassée.
            tokio::task::yield_now().await;
        }
        assert_eq!(doctrine.search(&query("donation")).await, Err(limited(43_200)));
        assert_eq!(doctrine.get(&id()).await, Err(limited(43_200)));
        assert_eq!(requests(&server).await, 0);
    }

    #[tokio::test]
    async fn calls_start_half_a_second_apart() {
        let (server, doctrine, timer) = portal(ok("search-empty.json")).await;
        for _ in 0..3 {
            assert!(doctrine.search(&query("donation")).await.is_ok());
        }
        assert_eq!(
            timer.sleeps(),
            [Duration::ZERO, Duration::from_millis(500), Duration::from_millis(500)]
        );
        assert_eq!(requests(&server).await, 3);
    }

    // ---- échéance --------------------------------------------------------------------------------

    #[tokio::test]
    async fn an_expired_deadline_is_a_timeout_without_any_request() {
        let (server, doctrine, _timer) = portal(ok("search-empty.json")).await;
        let expired = Deadline::after(Duration::ZERO);
        assert_eq!(expired.scope(doctrine.get(&id())).await, Err(DoctrineError::Timeout));
        // Hors de toute échéance publiée, c'est celle de la configuration qui vaut.
        let (none, _timer) =
            doctrine_on(&server.uri(), BofipConfig::new(AGENT.to_owned(), s(20), Duration::ZERO));
        assert_eq!(none.search(&query("donation")).await, Err(DoctrineError::Timeout));
        assert_eq!(requests(&server).await, 0);
    }

    #[tokio::test]
    async fn a_request_is_bounded_by_what_is_left_of_the_deadline() {
        let slow = ResponseTemplate::new(200).set_delay(s(30));
        let (_server, doctrine, _timer) = portal(slow).await;
        // Délai de requête de 20 s, mais 300 ms d'échéance : c'est l'échéance qui coupe.
        let short = Deadline::after(Duration::from_millis(300));
        assert_eq!(short.scope(doctrine.get(&id())).await, Err(DoctrineError::Timeout));
    }

    #[tokio::test]
    async fn a_request_is_bounded_by_the_request_timeout() {
        let slow = ResponseTemplate::new(200).set_delay(s(30));
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(slow).mount(&server).await;
        let quick = BofipConfig::new(AGENT.to_owned(), Duration::from_millis(300), s(60));
        let (doctrine, _timer) = doctrine_on(&server.uri(), quick);
        assert_eq!(doctrine.search(&query("donation")).await, Err(DoctrineError::Timeout));
    }

    // ---- journal ---------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_call_logs_its_template_its_status_and_its_size_never_its_words() {
        let (_server, doctrine, _timer) = portal(ok("search-empty.json")).await;
        start_journal();
        assert!(doctrine.search(&query(SENTINEL)).await.is_ok());
        let text = journal();
        assert!(text.contains("DEBUG") && text.contains("BOFiP portal answered"));
        assert!(
            text.contains("records?search")
                && text.contains("status=200")
                && text.contains("bytes=33")
        );
        assert!(!text.contains(SENTINEL) && !text.contains("where") && !text.contains("WARN"));
    }

    #[tokio::test]
    async fn anomalies_are_logged_as_counts() {
        let body = String::from_utf8(fixture("get-no-text.json"))
            .unwrap()
            .replace(r#""contenu": null"#, r#""contenu": "&eacute;t&eacute; &bull;""#)
            .replace(r#""serie": "INT""#, r#""serie": "IN-T""#);
        let (_server, doctrine, _timer) =
            portal(ResponseTemplate::new(200).set_body_bytes(body.into_bytes())).await;
        start_journal();
        let docs = doctrine.get(&BoiId::parse("BOI-LETTRE-000150").unwrap()).await.unwrap();
        assert_eq!((docs.len(), docs[0].serie.is_none()), (1, true));
        let text = journal();
        assert!(text.contains("WARN") && text.contains("BOFiP answer accepted with anomalies"));
        assert!(
            text.contains("records?get")
                && text.contains("unknown_entities=3")
                && text.contains("dropped_fields=1")
        );
        assert!(!text.contains("eacute") && !text.contains("IN-T"));
    }

    #[test]
    fn nothing_is_reported_without_anomalies() {
        start_journal();
        report(Route::Search, Anomalies::default());
        assert_eq!(journal(), "");
        report(Route::Search, Anomalies { unknown_entities: 0, dropped_fields: 1 });
        assert!(journal().contains("dropped_fields=1"));
    }

    // ---- aucune panne ne porte les mots de la requête ----------------------------------------------

    /// Cinq pannes de transport, avec un mot sentinelle dans la requête : ni l'affichage de
    /// l'erreur, ni sa forme de débogage, ni le journal ne portent ce mot, la clause, ou l'adresse
    /// du serveur.
    #[tokio::test]
    async fn transport_errors_never_contain_query_words() {
        let network = |kind: &str| DoctrineError::Upstream {
            status: None,
            retryable: true,
            detail: format!("network error ({kind})"),
        };
        let quick = || BofipConfig::new(AGENT.to_owned(), Duration::from_millis(300), s(60));

        // 1. Refus de connexion.
        let refused = closed_port().await;
        // 2. Délai dépassé.
        let slow = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(s(30)))
            .mount(&slow)
            .await;
        // 3. Corps coupé en route.
        let cut =
            raw_server(b"HTTP/1.1 200 OK\r\ncontent-length: 500\r\n\r\n{\"total_count\"").await;
        // 4. Faux gzip.
        let false_gzip = MockServer::start().await;
        let template = ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .set_body_bytes(b"ceci n'est pas du gzip".to_vec());
        Mock::given(method("GET")).respond_with(template).mount(&false_gzip).await;
        // 5. Poignée de main TLS qui échoue.
        let plain = raw_server(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        let tls = plain.replacen("http://", "https://", 1);

        let rows = [
            (refused, config(), network("connect")),
            (slow.uri(), quick(), DoctrineError::Timeout),
            (cut, config(), network("body")),
            (false_gzip.uri(), config(), network("body")),
            (tls, config(), network("connect")),
        ];
        for (base, config, expected) in rows {
            let (doctrine, _timer) = doctrine_on(&base, config);
            start_journal();
            let error = doctrine.search(&query(SENTINEL)).await.unwrap_err();
            assert_eq!(error, expected);
            let shown = format!("{error} | {error:?}");
            assert!(!shown.contains(SENTINEL) && !shown.contains("contenu"));
            assert!(!shown.contains("127.0") && !shown.contains("http"));
            // Le journal, tous niveaux confondus : la bibliothèque HTTP y nomme l'hôte et le port
            // de la connexion, jamais le chemin ni la requête.
            let logged = journal();
            assert!(logged.contains("hyper_util") && logged.contains("127.0.0.1"));
            assert!(!logged.contains(SENTINEL) && !logged.contains("contenu"));
            assert!(!logged.contains("where") && !logged.contains("records"));
        }
    }

    /// Le gzip du portail est lu de bout en bout.
    #[tokio::test]
    async fn a_gzip_answer_is_decoded() {
        let template = ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .set_body_bytes(gzip(&fixture("get-longest.json")));
        let (_server, doctrine, _timer) = portal(template).await;
        let docs = doctrine.get(&BoiId::parse("BOI-ANNX-000050").unwrap()).await.unwrap();
        assert_eq!((docs.len(), docs[0].publication_id.get()), (1, 6273));
    }
}
