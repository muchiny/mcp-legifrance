//! Le serveur MCP : `LegifranceServer` assemble les 13 routeurs d'outils et délègue les
//! resources, les prompts et les complétions (05 §3, §8).
//!
//! Le serveur est générique sur les trois ports de la façade (`C`, `A`, `K`), comme
//! `LegalService`. Les macros rmcp n'ajoutent aucune borne : `LegalCorpus`, `AuditLog` et `Clock`
//! portent déjà `Send + Sync + 'static`, ce qu'exige `ToolRouter<Self>` (spike 0A, V-15).
//!
//! # Protocole
//!
//! - `get_info` rend une `ServerConfig` précalculée, la même pour `initialize` (ère legacy) et
//!   pour `server/discover` (ère moderne) : identité (`mcp-legifrance`, version du crate, titre),
//!   capacités de la surface annoncée ([`Surface`]) et instructions (`text.rs`). Jamais
//!   `Implementation::from_build_env()`, qui rend l'identité de rmcp (05 §2.2).
//! - `discover` et `list_tools` sont écrits à la main : rmcp pose `ttlMs: 0` quand on ne le fait
//!   pas, alors que ces deux résultats sont statiques pour un binaire donné (Q8, 05 §2.3). Les
//!   indices ne partent qu'aux pairs de l'ère moderne : rmcp ne retire que `resultType` aux
//!   autres.
//! - `call_tool` est écrit à la main pour intercepter la panique d'un outil (V-36) : sans cela
//!   rmcp ne répond rien à la requête, qui reste pendue côté client (`spikes/rmcp-stdio`).
//! - Les resources, les prompts et les complétions (v0.3, M7) sont écrits à la main aussi
//!   (`resources.rs`, `prompts.rs`, `completions.rs`) : `#[prompt_handler]` poserait `ttlMs: 0` et
//!   remplacerait ces méthodes. Les cinq résultats cachables (`tools/list`, `prompts/list`,
//!   `resources/list`, `resources/templates/list`, `resources/read`) portent `ttlMs` et
//!   `cacheScope` aux seuls pairs modernes ([`cached`], Q8, 05 §2.3).

use std::{
    any::Any,
    borrow::Cow,
    future::Future,
    panic::{self, AssertUnwindSafe},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use lf_application::{
    output::complete::CompleteOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    service::LegalService,
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext},
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CompleteRequestParams, CompleteResult,
        DiscoverResult, GetPromptRequestParams, GetPromptResponse, Implementation,
        ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
        PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
        ReadResourceResult, ServerCapabilities, ServerConfig, SubscriptionFilter,
    },
    service::RequestContext,
    tool_handler,
};

use crate::{
    completions, prompts, resources,
    text::{SURFACE, Surface},
    versions::SUPPORTED,
};

/// Nombre d'outils du catalogue (05 §3) : un routeur par fichier de `tools/`.
const ROUTERS: usize = 13;

/// Nom du serveur dans `serverInfo` (05 §2.2).
const SERVER_NAME: &str = "mcp-legifrance";

/// Titre lisible du serveur, en anglais (Q14).
const SERVER_TITLE: &str = "Légifrance (PISTE)";

/// Dépôt du serveur, renommé `mcp-legifrance` (Q1, décidée le 2026-10-16).
const WEBSITE_URL: &str = "https://github.com/muchiny/mcp-legifrance";

/// Durée de fraîcheur annoncée aux pairs modernes pour `server/discover` et `tools/list` : une
/// heure (Q8). Ces deux résultats sont identiques pour tous les utilisateurs et statiques.
const CACHE_TTL_MS: u64 = 3_600_000;

/// Message de la réponse `-32603` quand un outil panique. Générique : la cause reste dans les
/// journaux, jamais un texte de requête ni un secret (05 §7.1).
const INTERNAL_ERROR_MESSAGE: &str = "internal error";

/// Serveur MCP de Légifrance : les outils, puis les resources, prompts et complétions.
pub struct LegifranceServer<C: LegalCorpus, A: AuditLog, K: Clock> {
    /// La façade des cas d'usage, lue par les outils (`tools/*.rs`).
    pub(crate) service: Arc<LegalService<C, A, K>>,
    /// Identité, capacités et instructions, calculées une fois : rmcp appelle `get_info` à chaque
    /// `initialize`, `server/discover` et `subscriptions/listen`.
    info: ServerConfig,
    /// Le routeur servi : le seul que lit `#[tool_handler]`.
    tool_router: ToolRouter<Self>,
}

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    /// Serveur dont le routeur additionne les treize routeurs d'outils de `tools/`, annonçant la
    /// surface [`SURFACE`].
    #[must_use]
    pub fn new(service: Arc<LegalService<C, A, K>>) -> Self {
        Self::with_surface(service, SURFACE)
    }

    /// [`LegifranceServer::new`] pour une surface donnée : ses capacités et ses instructions.
    pub(crate) fn with_surface(service: Arc<LegalService<C, A, K>>, surface: Surface) -> Self {
        let tool_router =
            Self::routers().into_iter().fold(ToolRouter::new(), |all, router| all + router);
        Self { service, info: server_config(surface), tool_router }
    }

    /// Les routeurs des 13 outils, dans l'ordre du catalogue (05 §3). La longueur du tableau est
    /// vérifiée à la compilation : retirer ou oublier un routeur ne compile pas.
    fn routers() -> [ToolRouter<Self>; ROUTERS] {
        [
            Self::search_router(),
            Self::get_act_router(),
            Self::get_text_router(),
            Self::get_decision_router(),
            Self::get_deliberation_router(),
            Self::get_convention_router(),
            Self::get_company_agreement_router(),
            Self::coverage_router(),
            Self::verify_citations_router(),
            Self::resolve_router(),
            Self::get_related_router(),
            Self::diff_article_router(),
            Self::search_all_router(),
        ]
    }

    /// Le routeur servi, pour ajouter ou retirer une route depuis un test (`add_route`,
    /// `remove_route`) : seul le routeur de ce champ est lu par `#[tool_handler]`.
    #[cfg(all(test, feature = "test-support"))]
    pub(crate) fn router_mut(&mut self) -> &mut ToolRouter<Self> {
        &mut self.tool_router
    }
}

/// Clone manuel, car `derive` exigerait `C: Clone`, `A: Clone` et `K: Clock + Clone` alors que la
/// façade est partagée par `Arc`. Il sert la fabrique sans état du transport HTTP (M8), appelée à
/// chaque requête : on clone un prototype dont le routeur est construit une fois (02 §3.6, 05 §10.1).
impl<C: LegalCorpus, A: AuditLog, K: Clock> Clone for LegifranceServer<C, A, K> {
    fn clone(&self) -> Self {
        Self {
            service: Arc::clone(&self.service),
            info: self.info.clone(),
            tool_router: self.tool_router.clone(),
        }
    }
}

/// Capacités de la surface (05 §2.2) : les outils seuls en v0.1 et v0.2, plus les resources, les
/// prompts et les complétions en v0.3. Jamais `listChanged`, `subscribe`, `logging`,
/// `extensions` ni `experimental`.
fn capabilities(surface: Surface) -> ServerCapabilities {
    let tools = ServerCapabilities::builder().enable_tools();
    match surface {
        Surface::V01 | Surface::V02 => tools.build(),
        Surface::V03 => tools.enable_resources().enable_prompts().enable_completions().build(),
    }
}

/// Identité, capacités et instructions de la surface, pour les deux ères.
///
/// `with_protocol_version` fixe le repli d'`initialize` : sans lui, `ServerConfig::new` prend
/// `ProtocolVersion::default()`, soit la dernière version du SDK, qui change à une mineure de
/// rmcp (annexe D §3.2).
fn server_config(surface: Surface) -> ServerConfig {
    ServerConfig::new(capabilities(surface))
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
        .with_server_info(
            Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION"))
                .with_title(SERVER_TITLE)
                .with_website_url(WEBSITE_URL),
        )
        .with_instructions(surface.instructions())
}

/// Vrai si la requête est servie en ère moderne (2026-07-28 ou plus), seule à connaître `ttlMs`
/// et `cacheScope`.
fn is_modern(context: &RequestContext<RoleServer>) -> bool {
    context.protocol_version().is_some_and(|version| version >= ProtocolVersion::V_2026_07_28)
}

/// Un résultat cachable (SEP-2549) : il porte `ttlMs` et `cacheScope`.
trait Cacheable: Sized {
    /// Le résultat avec ses indications de cache.
    fn with_cache_hints(self, ttl_ms: u64, scope: CacheScope) -> Self;
}

/// Les cinq résultats cachables du serveur (05 §2.3) ; tous ont les mêmes deux constructeurs.
macro_rules! cacheable {
    ($($result:ty),+ $(,)?) => {
        $(impl Cacheable for $result {
            fn with_cache_hints(self, ttl_ms: u64, scope: CacheScope) -> Self {
                self.with_ttl_ms(ttl_ms).with_cache_scope(scope)
            }
        })+
    };
}

cacheable!(
    ListToolsResult,
    ListPromptsResult,
    ListResourcesResult,
    ListResourceTemplatesResult,
    ReadResourceResult,
);

/// `result` avec les indications de cache de Q8 (`ttlMs` d'une heure, `cacheScope` public) si le
/// pair est de l'ère moderne ; tel quel sinon : rmcp ne retire que `resultType` aux pairs legacy,
/// pour qui ces champs n'existent pas (05 §2.3).
fn cached<T: Cacheable>(result: T, context: &RequestContext<RoleServer>) -> T {
    if is_modern(context) {
        result.with_cache_hints(CACHE_TTL_MS, CacheScope::Public)
    } else {
        result
    }
}

/// Futur qui rend la panique du futur interne comme une valeur (`FutureExt::catch_unwind` de
/// `futures`, sans la dépendance). Le futur interne est boxé : il reste ainsi `Unpin`, sans
/// `unsafe` ni projection de broches.
struct CatchUnwind<F> {
    inner: Pin<Box<F>>,
}

impl<F: Future> CatchUnwind<F> {
    fn new(inner: F) -> Self {
        Self { inner: Box::pin(inner) }
    }
}

impl<F: Future> Future for CatchUnwind<F> {
    type Output = Result<F::Output, Box<dyn Any + Send>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.inner.as_mut();
        // Le futur n'est plus sondé après une panique : on rend `Ready`, donc aucun état
        // incohérent ne peut être observé (d'où `AssertUnwindSafe`).
        match panic::catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    }
}

// `#[tool_handler]` génère des `async fn` sans `.await` (`get_tool`) et nos délégations n'en ont
// pas non plus : pedantic les signale sur le site de la macro.
#[expect(
    clippy::unused_async_trait_impl,
    reason = "méthodes async générées par #[tool_handler] et délégations sans .await"
)]
#[tool_handler(router = self.tool_router)]
impl<C: LegalCorpus, A: AuditLog, K: Clock> ServerHandler for LegifranceServer<C, A, K> {
    fn get_info(&self) -> ServerConfig {
        self.info.clone()
    }

    /// Les trois versions de 00 §6, jamais `ProtocolVersion::KNOWN_VERSIONS`.
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED)
    }

    /// `server/discover` : statique, sans réseau, jamais de `context.peer` (muet avant toute ère,
    /// annexe D §3.2). Les mêmes capacités et instructions qu'`initialize`.
    async fn discover(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> Result<DiscoverResult, ErrorData> {
        Ok(DiscoverResult::from_server_info(SUPPORTED.to_vec(), self.get_info())
            .with_ttl_ms(CACHE_TTL_MS)
            .with_cache_scope(CacheScope::Public))
    }

    /// `subscriptions/listen` (C-24, 00 §6 AM-10) : l'accusé minimal, un filtre vide dont le
    /// premier message est `notifications: {}`, puis le silence jusqu'à l'annulation (le `listen`
    /// par défaut de rmcp). Le serveur n'annonce aucune capacité de notification : rmcp réduit de
    /// toute façon le filtre aux capacités de `get_info`. Aucun contexte n'est passé ici, et il
    /// n'en faut pas : rmcp répond `-32601` lui-même aux pairs legacy, avant d'appeler ce filtre.
    fn accepted_subscription_filter(
        &self,
        _requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        Some(SubscriptionFilter::new())
    }

    /// `tools/list` : les routes, triées par nom ; `ttlMs` et `cacheScope` aux seuls pairs
    /// modernes (05 §2.3).
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let listed = ListToolsResult::with_all_items(self.tool_router.list_all());
        Ok(cached(listed, &context))
    }

    /// `tools/call` : le routeur du champ, avec la panique d'un outil rendue en `-32603`
    /// (V-36) au lieu du silence de rmcp. Le serveur continue ; le message est générique et la
    /// charge de la panique, qui peut citer la requête, n'est ni lue ni journalisée.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let tool = request.name.clone();
        let call = self.tool_router.call(ToolCallContext::new(self, request, context));
        match CatchUnwind::new(call).await {
            Ok(response) => response,
            Err(_payload) => {
                tracing::error!(%tool, "tool call panicked; answering -32603");
                Err(ErrorData::internal_error(INTERNAL_ERROR_MESSAGE, None))
            }
        }
    }

    /// `resources/list` : la resource statique `legifrance://coverage` (05 §8.2).
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(cached(resources::list(), &context))
    }

    /// `resources/templates/list` : les quatre gabarits (05 §8.2).
    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(cached(resources::list_templates(), &context))
    }

    /// `resources/read` : la lecture auditée sous `resource:<gabarit>` ; introuvable en `-32002`
    /// (legacy) ou `-32602` (moderne, réécrit par rmcp), URI ou identifiant invalide en `-32602`
    /// (05 §8.3).
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let uri = request.uri;
        // Boxé : la lecture, qui embarque les cas d'usage de lecture, grossit sinon de plus de
        // 20 kio le futur de `serve_stdio` (clippy::large_futures).
        let reading = self.service.read_resource(resources::call(&uri), context.ct.cancelled());
        let output = Box::pin(reading).await.map_err(|failure| resources::error(&uri, &failure))?;
        Ok(ReadResourceResponse::from(cached(resources::contents(&uri, output), &context)))
    }

    /// `prompts/list` : les trois prompts français (05 §8.5).
    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(cached(prompts::list(), &context))
    }

    /// `prompts/get` : jamais d'audit ni de cache ; `-32602` pour un nom inconnu ou un argument
    /// invalide.
    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        prompts::get(&request.name, request.arguments.as_ref()).map(GetPromptResponse::from)
    }

    /// `completion/complete` : les slugs de codes de la variable `code`, jamais une erreur ni un
    /// audit (05 §8.4).
    async fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, ErrorData> {
        let output = match completions::input(&request) {
            // Boxé comme la lecture de `resources/read` (clippy::large_futures).
            Some(input) => Box::pin(self.service.complete(input)).await,
            None => CompleteOutput::empty(),
        };
        Ok(completions::result(output))
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        ports::PisteEnvironment,
        service::{ConfigurationState, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };
    use lf_domain::date::CivilDate;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::time::Duration;

    use super::*;

    type TestServer = LegifranceServer<FakeCorpus, MemoryAuditLog, FixedClock>;

    fn server_on(surface: Surface) -> TestServer {
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
        #[expect(
            clippy::default_trait_access,
            reason = "chrono n'est pas une dépendance de ce crate : l'instant par défaut est l'époque Unix"
        )]
        let now = Default::default();
        let clock = FixedClock::new(now, CivilDate::new(2026, 10, 29).unwrap());
        let service = LegalService::new(FakeCorpus::new(), MemoryAuditLog::new(), clock, settings);
        LegifranceServer::with_surface(Arc::new(service), surface)
    }

    fn server() -> TestServer {
        server_on(SURFACE)
    }

    #[test]
    fn the_served_router_is_the_sum_of_the_thirteen() {
        assert_eq!(TestServer::routers().len(), ROUTERS);
        let tools: usize = TestServer::routers().iter().map(|router| router.list_all().len()).sum();
        assert_eq!(server().tool_router.list_all().len(), tools);
    }

    #[test]
    fn server_info_is_mcp_legifrance_with_crate_version() {
        let info = server().get_info().server_info;
        assert_eq!(info.name, "mcp-legifrance");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        // Jamais l'identité de rmcp (`Implementation::from_build_env`, 05 §2.2).
        assert_ne!(info.name, Implementation::from_build_env().name);
        assert_eq!(info.title.as_deref(), Some("Légifrance (PISTE)"));
        assert_eq!(info.website_url.as_deref(), Some("https://github.com/muchiny/mcp-legifrance"));
    }

    #[test]
    fn the_instructions_and_the_initialize_fallback_follow_the_surface() {
        for surface in [Surface::V01, Surface::V02, Surface::V03] {
            let info = server_on(surface).get_info();
            assert_eq!(info.instructions.as_deref(), Some(surface.instructions()), "{surface:?}");
            // Repli explicite d'`initialize` : jamais la dernière version du SDK (annexe D §3.2).
            assert_eq!(info.protocol_version, ProtocolVersion::V_2025_11_25, "{surface:?}");
        }
    }

    #[test]
    fn capabilities_follow_the_surface() {
        let tools_only = json!({"tools": {}});
        assert_eq!(serde_json::to_value(capabilities(Surface::V01)).unwrap(), tools_only);
        assert_eq!(serde_json::to_value(capabilities(Surface::V02)).unwrap(), tools_only);
        // v0.3 : resources, prompts et complétions, sans `listChanged` ni `subscribe` (05 §2.2).
        assert_eq!(
            serde_json::to_value(capabilities(Surface::V03)).unwrap(),
            json!({"tools": {}, "resources": {}, "prompts": {}, "completions": {}})
        );
        assert_eq!(
            serde_json::to_value(server().get_info().capabilities).unwrap(),
            serde_json::to_value(capabilities(SURFACE)).unwrap()
        );
    }

    #[test]
    fn supported_versions_are_explicit() {
        let versions = server().supported_protocol_versions();
        let listed: Vec<&str> = versions.iter().map(ProtocolVersion::as_str).collect();
        assert_eq!(listed, ["2026-07-28", "2025-11-25", "2025-06-18"]);
        // Jamais la liste du SDK, qui suit rmcp et non nos engagements (00 §6).
        assert_ne!(&*versions, ProtocolVersion::KNOWN_VERSIONS);
    }

    #[test]
    fn the_service_is_shared_not_copied() {
        let service = Arc::clone(&server().service);
        assert_eq!(service.settings().policy.server_version, "0.0.0");
    }

    // ---- CatchUnwind ------------------------------------------------------------------------

    #[expect(clippy::panic, reason = "futur de test qui panique, pour éprouver CatchUnwind")]
    fn explode() -> u32 {
        panic!("boom")
    }

    #[tokio::test]
    async fn catch_unwind_passes_the_output_through_pending_polls() {
        let caught = CatchUnwind::new(async {
            tokio::task::yield_now().await;
            7_u32
        })
        .await;
        assert_eq!(caught.unwrap(), 7);
    }

    #[tokio::test]
    async fn catch_unwind_returns_the_panic_as_a_value_before_and_after_a_yield() {
        let at_once = CatchUnwind::new(async { explode() }).await;
        assert_eq!(*at_once.unwrap_err().downcast::<&str>().unwrap(), "boom");

        let after_yield = CatchUnwind::new(async {
            tokio::task::yield_now().await;
            explode()
        })
        .await;
        assert!(after_yield.is_err());
    }

    // ---- sur le fil -------------------------------------------------------------------------

    /// Les tests de bout en bout passent par `testing::connect` (feature `test-support`).
    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use std::{future::Future, pin::Pin};

        use rmcp::{
            handler::server::{common::schema_for_empty_input, router::tool::ToolRoute},
            model::{CallToolResult, ContentBlock, Tool},
        };
        use serde_json::Value;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            lifecycle::testkit::{
                Wire, call_tool, discover, initialize, initialized, modern_meta, ping, tools_list,
            },
            testing::{Fixture, FixtureServer, connect},
        };

        /// Réponse boxée d'une route écrite à la main (`DynCallToolHandler`).
        type RouteFuture<'a> =
            Pin<Box<dyn Future<Output = Result<CallToolResponse, ErrorData>> + Send + 'a>>;

        /// Route de test : rend ses arguments bruts en texte.
        fn echo_route(context: ToolCallContext<'_, FixtureServer>) -> RouteFuture<'_> {
            let arguments = context.arguments.unwrap_or_default();
            Box::pin(async move {
                let text = serde_json::to_string(&arguments).unwrap();
                Ok(CallToolResponse::from(CallToolResult::success(vec![ContentBlock::text(text)])))
            })
        }

        /// Route de test : panique après un tour d'ordonnanceur (V-36).
        #[expect(clippy::panic, reason = "route de test : un outil qui panique (V-36)")]
        fn boom_route(_: ToolCallContext<'_, FixtureServer>) -> RouteFuture<'_> {
            Box::pin(async {
                tokio::task::yield_now().await;
                panic!("tool panic (V-36)")
            })
        }

        /// Serveur de test portant les routes `echo` et `boom`, avec les poignées de sa fixture.
        fn server_with_test_routes(fixture: &Fixture) -> FixtureServer {
            let mut server = fixture.server();
            server.router_mut().add_route(ToolRoute::new_dyn(
                Tool::new("echo", "echo the arguments", schema_for_empty_input()),
                echo_route,
            ));
            server.router_mut().add_route(ToolRoute::new_dyn(
                Tool::new("boom", "panics", schema_for_empty_input()),
                boom_route,
            ));
            server
        }

        /// `result` de la réponse à `initialize` (ère legacy, 2025-11-25).
        async fn legacy_initialize(wire: &mut Wire) -> Value {
            let response = wire.handshake().await;
            response["result"].clone()
        }

        /// `result` de la réponse à `server/discover` (ère moderne, sans `initialize`).
        async fn modern_discover(wire: &mut Wire) -> Value {
            wire.send(&discover(1)).await;
            let response = wire.recv().await;
            assert_eq!(response["id"], json!(1), "{response}");
            response["result"].clone()
        }

        #[tokio::test]
        async fn the_handshake_reports_the_server_and_tools_only() {
            let client = connect(server()).await;
            let info = client.peer_info().expect("le serveur s'est présenté");
            assert_eq!(info.server_info.as_ref().unwrap().name, "mcp-legifrance");
            assert_eq!(info.server_info.as_ref().unwrap().version, env!("CARGO_PKG_VERSION"));
            assert!(info.capabilities.tools.is_some());
            assert!(info.capabilities.resources.is_none());
        }

        #[tokio::test]
        async fn server_info_and_instructions_are_the_same_in_both_eras() {
            let identity = json!({
                "name": "mcp-legifrance",
                "title": "Légifrance (PISTE)",
                "version": env!("CARGO_PKG_VERSION"),
                "websiteUrl": "https://github.com/muchiny/mcp-legifrance",
            });
            let mut legacy = Wire::serve(server());
            let initialized = legacy_initialize(&mut legacy).await;
            assert_eq!(initialized["serverInfo"], identity, "{initialized}");
            assert_eq!(initialized["instructions"], json!(SURFACE.instructions()));
            assert_eq!(initialized["protocolVersion"], json!("2025-11-25"));

            let mut modern = Wire::serve(server());
            let discovered = modern_discover(&mut modern).await;
            assert_eq!(
                discovered["_meta"]["io.modelcontextprotocol/serverInfo"], identity,
                "{discovered}"
            );
            assert_eq!(discovered["instructions"], initialized["instructions"]);
        }

        #[tokio::test]
        async fn capabilities_same_in_both_eras() {
            let mut legacy = Wire::serve(server());
            let initialized = legacy_initialize(&mut legacy).await;
            let mut modern = Wire::serve(server());
            let discovered = modern_discover(&mut modern).await;
            assert_eq!(initialized["capabilities"], discovered["capabilities"]);
            assert_eq!(discovered["capabilities"], json!({"tools": {}}), "{discovered}");
        }

        #[tokio::test]
        async fn discover_is_static_explicit_and_cacheable() {
            let mut wire = Wire::serve(server());
            let discovered = modern_discover(&mut wire).await;
            assert_eq!(discovered["resultType"], json!("complete"));
            assert_eq!(
                discovered["supportedVersions"],
                json!(["2026-07-28", "2025-11-25", "2025-06-18"])
            );
            assert_eq!(discovered["ttlMs"], json!(3_600_000));
            assert_eq!(discovered["cacheScope"], json!("public"));
        }

        #[tokio::test]
        async fn tools_list_carries_cache_hints_for_modern_peers_only() {
            let mut modern = Wire::serve(server());
            modern.send(&tools_list(1, Some(modern_meta("2026-07-28")))).await;
            let listed = modern.recv().await;
            assert_eq!(listed["result"]["ttlMs"], json!(3_600_000), "{listed}");
            assert_eq!(listed["result"]["cacheScope"], json!("public"), "{listed}");

            let mut legacy = Wire::serve(server());
            legacy.handshake().await;
            legacy.send(&tools_list(2, None)).await;
            let listed = legacy.recv().await;
            let result = listed["result"].as_object().unwrap();
            assert!(!result.contains_key("ttlMs"), "{listed}");
            assert!(!result.contains_key("cacheScope"), "{listed}");
            assert!(!result.contains_key("resultType"), "{listed}");
        }

        #[tokio::test]
        async fn a_route_added_to_the_router_field_is_served() {
            let mut server = Fixture::default().server();
            server.router_mut().add_route(ToolRoute::new_dyn(
                Tool::new("echo", "echo the arguments", schema_for_empty_input()),
                echo_route,
            ));
            let client = connect(server).await;

            let names: Vec<_> =
                client.list_all_tools().await.unwrap().into_iter().map(|tool| tool.name).collect();
            assert!(names.iter().any(|name| name == "echo"), "{names:?}");

            let mut params = CallToolRequestParams::new("echo");
            params.arguments = json!({"a": [1, 2]}).as_object().cloned();
            let result = client.call_tool(params).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            assert_eq!(result.content[0].as_text().unwrap().text, r#"{"a":[1,2]}"#);
        }

        #[tokio::test]
        async fn unknown_tool_is_jsonrpc_32602_without_audit() {
            let fixture = Fixture::default();
            let mut wire = Wire::serve(fixture.server());
            wire.handshake().await;
            wire.send(&call_tool(2, "fr_nope", &json!({"query": "bail"}))).await;
            let response = wire.recv().await;
            assert_eq!(response["id"], json!(2), "{response}");
            assert_eq!(response["error"]["code"], json!(-32602), "{response}");
            assert_eq!(response["error"]["message"], json!("tool not found"), "{response}");
            assert!(response.get("result").is_none(), "{response}");
            // Rien n'a été exécuté : ni ligne d'audit, ni appel au port (02 §9.2).
            assert_eq!(fixture.audit.records().len(), 0);
            assert_eq!(fixture.corpus.calls(), 0);
        }

        #[tokio::test]
        async fn tool_panic_is_internal_error_and_server_continues() {
            let fixture = Fixture::default();
            let mut wire = Wire::serve(server_with_test_routes(&fixture));
            wire.handshake().await;

            // V-36 : rmcp ne répondrait rien à cet `id` ; le client attendrait pour toujours.
            wire.send(&call_tool(2, "boom", &json!({}))).await;
            let response = wire.recv().await;
            assert_eq!(response["id"], json!(2), "{response}");
            assert_eq!(response["error"]["code"], json!(-32603), "{response}");
            assert_eq!(response["error"]["message"], json!("internal error"), "{response}");
            assert!(response.get("result").is_none(), "{response}");

            // Le serveur continue : ping, puis un autre outil, servis normalement.
            wire.send(&ping(3)).await;
            let pong = wire.recv().await;
            assert_eq!((pong["id"].clone(), pong["result"].clone()), (json!(3), json!({})));
            wire.send(&call_tool(4, "echo", &json!({"still": "alive"}))).await;
            let echoed = wire.recv().await;
            assert_eq!(
                echoed["result"]["content"][0]["text"],
                json!(r#"{"still":"alive"}"#),
                "{echoed}"
            );
            assert!(!wire.server.is_finished());
        }

        #[tokio::test]
        async fn two_panics_in_a_row_leave_the_server_serving() {
            let fixture = Fixture::default();
            let mut wire = Wire::serve(server_with_test_routes(&fixture));
            wire.handshake().await;
            for id in [2, 3] {
                wire.send(&call_tool(id, "boom", &json!({}))).await;
                let response = wire.recv().await;
                assert_eq!(response["error"]["code"], json!(-32603), "{response}");
            }
            wire.send(&initialized()).await;
            wire.send(&ping(4)).await;
            assert_eq!(wire.recv().await["result"], json!({}));
        }

        #[tokio::test]
        async fn each_process_serves_its_own_era() {
            // Deux processus, deux ères : l'un ne déteint pas sur l'autre (état par processus).
            let mut modern = Wire::serve(server());
            modern_discover(&mut modern).await;
            let mut legacy = Wire::serve(server());
            legacy.send(&initialize(1)).await;
            assert_eq!(legacy.recv().await["result"]["protocolVersion"], json!("2025-11-25"));
        }

        // ---- resources, prompts et complétions (v0.3, M7) ---------------------------------------

        use lf_application::{
            error::ErrorCode,
            ports::{AuditStatus, CorpusError},
        };
        use lf_domain::{
            entity::{Act, ArticleText, CodeListing, Decision, OfficialSummary, TextBody},
            id::{ArticleId, DecisionId, LegiTextId},
            status::LegalStatus,
            text::LongText,
        };

        use crate::testing::{SharedAudit, SharedCorpus};

        const ARTICLE_URI: &str = "legifrance://article/LEGIARTI000006419288";
        const DECISION_URI: &str = "legifrance://decision/JURITEXT000037999394";
        const ACT_URI: &str = "legifrance://act/LEGITEXT000006070721";
        const CODE_ARTICLE_URI: &str = "legifrance://code/code-civil/article/9";
        const COVERAGE_URI: &str = "legifrance://coverage";
        const CODE_TEMPLATE: &str = "legifrance://code/{code}/article/{num}";
        const TEXT: &str = "Chacun a droit au respect de sa vie privée.";

        /// Le serveur de la surface v0.3 sur les doubles de `fixture` : `SURFACE` reste v0.1
        /// jusqu'à la bascule, mais ces tests éprouvent la surface complète.
        fn v03_server(fixture: &Fixture) -> FixtureServer {
            #[expect(
                clippy::default_trait_access,
                reason = "chrono n'est pas une dépendance de ce crate : l'instant par défaut est l'époque Unix"
            )]
            let now = Default::default();
            let service = LegalService::new(
                SharedCorpus(Arc::clone(&fixture.corpus)),
                SharedAudit(Arc::clone(&fixture.audit)),
                FixedClock::new(now, fixture.today),
                fixture.settings.clone(),
            );
            LegifranceServer::with_surface(Arc::new(service), Surface::V03)
        }

        /// Une requête brute : `_meta` moderne complet pour l'ère moderne, rien pour la legacy.
        fn request(id: u64, method: &str, params: Value, modern: bool) -> Value {
            let mut params = params;
            if modern {
                params["_meta"] = modern_meta("2026-07-28");
            }
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        }

        /// Un pair de la v0.3 : poignée de main faite en legacy, aucune en moderne (chaque
        /// requête porte son `_meta`).
        async fn peer(fixture: &Fixture, modern: bool) -> Wire {
            let mut wire = Wire::serve(v03_server(fixture));
            if !modern {
                wire.handshake().await;
            }
            wire
        }

        /// Pose une question et rend la réponse à son `id`.
        async fn ask(wire: &mut Wire, id: u64, method: &str, params: Value, modern: bool) -> Value {
            wire.send(&request(id, method, params, modern)).await;
            let response = wire.recv().await;
            assert_eq!(response["id"], json!(id), "{response}");
            response
        }

        fn read_params(uri: &str) -> Value {
            json!({ "uri": uri })
        }

        fn complete_params(template: &str, variable: &str, typed: &str) -> Value {
            json!({
                "ref": {"type": "ref/resource", "uri": template},
                "argument": {"name": variable, "value": typed}
            })
        }

        fn article() -> ArticleText {
            ArticleText {
                article_id: ArticleId::parse("LEGIARTI000006419288").unwrap(),
                num: Some("9".to_owned()),
                etat: Some(LegalStatus::Vigueur),
                code_title: Some("Code civil".to_owned()),
                parent: None,
                path: Vec::new(),
                full_sections_titre: None,
                date_debut: None,
                date_fin: None,
                text: LongText::from_plain(TEXT).unwrap(),
                nota: None,
            }
        }

        fn decision() -> Decision {
            Decision {
                decision_id: DecisionId::parse("JURITEXT000037999394").unwrap(),
                ecli: None,
                num: None,
                juridiction: Some("Cour de cassation".to_owned()),
                formation: None,
                solution: None,
                numero_affaire: Vec::new(),
                date_texte: None,
                nature: None,
                title: Some("Cass. civ. 1re, 5 juin 2019".to_owned()),
                text: LongText::from_plain("La Cour rejette le pourvoi.").unwrap(),
                official_summary: OfficialSummary::default(),
            }
        }

        fn act() -> Act {
            Act {
                text_id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
                body: TextBody { title: Some("Code civil".to_owned()), ..TextBody::default() },
            }
        }

        fn codes() -> Vec<CodeListing> {
            ["Code civil", "Code du travail", "Code de la sécurité sociale"]
                .into_iter()
                .map(|title| CodeListing {
                    id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
                    title: title.to_owned(),
                    etat: Some(LegalStatus::Vigueur),
                })
                .collect()
        }

        #[tokio::test]
        async fn v03_announces_resources_prompts_and_completions_in_both_eras() {
            let all = json!({"tools": {}, "resources": {}, "prompts": {}, "completions": {}});
            let mut legacy = Wire::serve(v03_server(&Fixture::default()));
            let initialized = legacy_initialize(&mut legacy).await;
            assert_eq!(initialized["capabilities"], all, "{initialized}");

            let mut modern = Wire::serve(v03_server(&Fixture::default()));
            let discovered = modern_discover(&mut modern).await;
            assert_eq!(discovered["capabilities"], all, "{discovered}");
        }

        #[tokio::test]
        async fn the_lists_describe_the_resource_the_templates_and_the_prompts() {
            let fixture = Fixture::default();
            let mut wire = peer(&fixture, false).await;

            let resources = ask(&mut wire, 2, "resources/list", json!({}), false).await;
            let listed = resources["result"]["resources"].as_array().unwrap();
            assert_eq!(listed.len(), 1, "{resources}");
            assert_eq!(listed[0]["uri"], json!(COVERAGE_URI));
            assert_eq!(listed[0]["mimeType"], json!("application/json"));

            let templates = ask(&mut wire, 3, "resources/templates/list", json!({}), false).await;
            let listed = templates["result"]["resourceTemplates"].as_array().unwrap();
            let uris: Vec<&str> =
                listed.iter().map(|template| template["uriTemplate"].as_str().unwrap()).collect();
            assert_eq!(
                uris,
                [
                    "legifrance://article/{article_id}",
                    "legifrance://decision/{decision_id}",
                    "legifrance://act/{text_id}",
                    "legifrance://code/{code}/article/{num}",
                ]
            );
            assert!(listed.iter().all(|template| template["mimeType"] == json!("text/markdown")));

            let prompts = ask(&mut wire, 4, "prompts/list", json!({}), false).await;
            let names: Vec<&str> = prompts["result"]["prompts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|prompt| prompt["name"].as_str().unwrap())
                .collect();
            assert_eq!(names, ["analyser_decision", "verifier_citations", "etat_du_droit"]);
            // Lister n'exécute rien : ni port, ni audit.
            assert_eq!((fixture.corpus.calls(), fixture.audit.records().len()), (0, 0));
        }

        #[tokio::test]
        async fn resource_read_audited_as_resource_name() {
            let fixture = Fixture::default();
            fixture.corpus.push_get_article(Ok(article()));
            fixture.corpus.push_get_decision(Ok(decision()));
            fixture.corpus.push_get_act(Ok(act()));
            fixture.corpus.push_list_codes(Ok(codes()));
            fixture.corpus.push_find_article(Ok(article()));
            let mut wire = peer(&fixture, false).await;

            let uris = [COVERAGE_URI, ARTICLE_URI, DECISION_URI, ACT_URI, CODE_ARTICLE_URI];
            let mut sizes = Vec::new();
            for (id, uri) in (2..).zip(uris) {
                let response = ask(&mut wire, id, "resources/read", read_params(uri), false).await;
                let contents = response["result"]["contents"].as_array().unwrap();
                assert_eq!(contents.len(), 1, "{response}");
                assert_eq!(contents[0]["uri"], json!(uri), "{response}");
                let mime = if uri == COVERAGE_URI { "application/json" } else { "text/markdown" };
                assert_eq!(contents[0]["mimeType"], json!(mime), "{response}");
                sizes.push(contents[0]["text"].as_str().unwrap().chars().count());
            }
            // Une sixième lecture, sans plus rien en file : le double répond une erreur.
            let failed = ask(&mut wire, 7, "resources/read", read_params(ARTICLE_URI), false).await;
            assert!(failed.get("error").is_some(), "{failed}");

            let lines = fixture.audit.records();
            let names: Vec<&str> = lines.iter().map(|line| line.tool.as_str()).collect();
            assert_eq!(
                names,
                [
                    "resource:coverage",
                    "resource:article",
                    "resource:decision",
                    "resource:act",
                    "resource:code_article",
                    "resource:article",
                ]
            );
            // Les caractères renvoyés, pour un Markdown ; la dernière lecture a échoué : zéro.
            for index in 1..5 {
                assert_eq!(lines[index].status, AuditStatus::Ok);
                assert_eq!(lines[index].output_count_or_size, u64::try_from(sizes[index]).unwrap());
            }
            assert_eq!(lines[5].status, AuditStatus::Error);
            assert_eq!(lines[5].output_count_or_size, 0);
            assert_eq!(lines[1].input.as_str(), format!("{{\"uri\":\"{ARTICLE_URI}\"}}"));
        }

        #[tokio::test]
        async fn resource_not_found_codes_by_era() {
            for (modern, code) in [(false, -32002), (true, -32602)] {
                let fixture = Fixture::default();
                fixture.corpus.push_get_article(Err(CorpusError::NotFound {
                    what: "/consult/getArticle".into(),
                }));
                fixture.corpus.push_list_codes(Ok(codes()));
                let mut wire = peer(&fixture, modern).await;

                // Un enregistrement introuvable, un code inconnu, puis une URI sans gabarit.
                let unknown_record = "legifrance://article/LEGIARTI000006419999";
                let unknown_code = "legifrance://code/code-inconnu/article/9";
                let unknown_template = "legifrance://unknown/x";
                for (id, uri) in (2..).zip([unknown_record, unknown_code, unknown_template]) {
                    let response =
                        ask(&mut wire, id, "resources/read", read_params(uri), modern).await;
                    assert_eq!(response["error"]["code"], json!(code), "{uri}: {response}");
                    assert_eq!(response["error"]["data"], json!({"uri": uri}), "{response}");
                    // Jamais `contents: []`.
                    assert!(response.get("result").is_none(), "{response}");
                }

                // Les deux lectures qui ont exécuté un cas d'usage sont auditées ; l'URI sans
                // gabarit, qui n'a rien exécuté, ne l'est pas.
                let lines = fixture.audit.records();
                let audited: Vec<(&str, AuditStatus, Option<ErrorCode>)> = lines
                    .iter()
                    .map(|line| (line.tool.as_str(), line.status, line.error_code))
                    .collect();
                assert_eq!(
                    audited,
                    [
                        ("resource:article", AuditStatus::Error, Some(ErrorCode::NotFound)),
                        ("resource:code_article", AuditStatus::Error, Some(ErrorCode::NotFound)),
                    ]
                );
            }
        }

        #[tokio::test]
        async fn malformed_resource_uri() {
            let long = format!("legifrance://article/{}", "A".repeat(600));
            for modern in [false, true] {
                let fixture = Fixture::default();
                let mut wire = peer(&fixture, modern).await;
                let outside_grammar = [
                    "file:///etc/passwd",
                    "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288",
                    "legifrance://article/..",
                    "legifrance://code/../article/9",
                    "legifrance://article/%2e%2e",
                    "legifrance://article/%zz",
                    "legifrance://article/%ff",
                    "legifrance://article/a b",
                    "",
                ];
                // `-32602` dans les deux ères, sans recopier l'URI (texte libre du client, que rmcp
                // journaliserait) ; rien n'est exécuté ni audité.
                for (id, uri) in (2..).zip(outside_grammar) {
                    let response =
                        ask(&mut wire, id, "resources/read", read_params(uri), modern).await;
                    assert_eq!(response["error"]["code"], json!(-32602), "{uri:?}: {response}");
                    assert!(response["error"].get("data").is_none(), "{uri:?}: {response}");
                    assert!(response.get("result").is_none(), "{response}");
                }
                // Une URI de plus de 512 octets n'est pas recopiée.
                let response =
                    ask(&mut wire, 20, "resources/read", read_params(&long), modern).await;
                assert_eq!(response["error"]["code"], json!(-32602), "{response}");
                assert!(response["error"].get("data").is_none(), "{response}");
                assert_eq!((fixture.corpus.calls(), fixture.audit.records().len()), (0, 0));

                // Un identifiant invalide dans un gabarit reconnu : `-32602` aussi, et la lecture,
                // qui a été tentée, est auditée.
                for (id, uri) in
                    [(30, "legifrance://decision/not-an-id"), (31, "legifrance://act/")]
                {
                    let response =
                        ask(&mut wire, id, "resources/read", read_params(uri), modern).await;
                    assert_eq!(response["error"]["code"], json!(-32602), "{uri}: {response}");
                    assert_eq!(response["error"]["data"], json!({"uri": uri}), "{response}");
                }
                let lines = fixture.audit.records();
                let audited: Vec<(&str, Option<ErrorCode>)> =
                    lines.iter().map(|line| (line.tool.as_str(), line.error_code)).collect();
                assert_eq!(
                    audited,
                    [
                        ("resource:decision", Some(ErrorCode::InvalidArg)),
                        ("resource:act", Some(ErrorCode::InvalidArg)),
                    ]
                );
                assert_eq!(fixture.corpus.calls(), 0);
            }
        }

        #[tokio::test]
        async fn other_resource_errors_are_internal_errors_with_the_tool_text() {
            let fixture = Fixture::default();
            // Rien de programmé : le double répond une erreur, que la lecture rend en `-32603`.
            let mut wire = peer(&fixture, false).await;

            let response =
                ask(&mut wire, 2, "resources/read", read_params(ARTICLE_URI), false).await;

            assert_eq!(response["error"]["code"], json!(-32603), "{response}");
            let message = response["error"]["message"].as_str().unwrap();
            let lines = fixture.audit.records();
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].tool.as_str(), "resource:article");
            let code = lines[0].error_code.unwrap();
            assert!(message.starts_with(&format!("[{}] ", code.as_str())), "{message}");
            assert!(response["error"].get("data").is_none(), "{response}");
        }

        #[tokio::test]
        async fn ttl_and_cache_scope_modern_only() {
            let cacheable: [(&str, Value); 5] = [
                ("tools/list", json!({})),
                ("prompts/list", json!({})),
                ("resources/list", json!({})),
                ("resources/templates/list", json!({})),
                ("resources/read", read_params(COVERAGE_URI)),
            ];
            let not_cacheable: [(&str, Value); 2] = [
                ("prompts/get", json!({"name": "etat_du_droit"})),
                ("completion/complete", complete_params(CODE_TEMPLATE, "code", "c")),
            ];
            for modern in [true, false] {
                let fixture = Fixture::default();
                fixture.corpus.push_list_codes(Ok(codes()));
                let mut wire = peer(&fixture, modern).await;
                for (id, (method, params)) in (2..).zip(&cacheable) {
                    let response = ask(&mut wire, id, method, params.clone(), modern).await;
                    let result = response["result"].as_object().unwrap();
                    if modern {
                        assert_eq!(result["ttlMs"], json!(3_600_000), "{method}: {response}");
                        assert_eq!(result["cacheScope"], json!("public"), "{method}: {response}");
                        assert_eq!(result["resultType"], json!("complete"), "{method}: {response}");
                    } else {
                        for key in ["ttlMs", "cacheScope", "resultType"] {
                            assert!(!result.contains_key(key), "{method} {key}: {response}");
                        }
                    }
                }
                // `prompts/get` et `completion/complete` ne sont jamais cachables (05 §2.3).
                for (id, (method, params)) in (20..).zip(&not_cacheable) {
                    let response = ask(&mut wire, id, method, params.clone(), modern).await;
                    let result = response["result"].as_object().unwrap();
                    for key in ["ttlMs", "cacheScope"] {
                        assert!(!result.contains_key(key), "{method} {key}: {response}");
                    }
                }
            }
        }

        #[tokio::test]
        async fn prompts_over_the_wire_in_both_eras() {
            for modern in [false, true] {
                let fixture = Fixture::default();
                let mut wire = peer(&fixture, modern).await;

                let wanted = json!({
                    "name": "analyser_decision",
                    "arguments": {"decision_id": "JURITEXT000037999394"}
                });
                let got = ask(&mut wire, 2, "prompts/get", wanted, modern).await;
                let message = &got["result"]["messages"][0];
                assert_eq!(message["role"], json!("user"), "{got}");
                assert_eq!(message["content"]["type"], json!("text"), "{got}");
                let text = message["content"]["text"].as_str().unwrap();
                assert!(
                    text.starts_with("Analyse la décision Légifrance JURITEXT000037999394."),
                    "{got}"
                );

                let invalid = [
                    json!({"name": "analyse"}),
                    json!({"name": "analyser_decision"}),
                    json!({"name": "analyser_decision", "arguments": {"decision_id": "x"}}),
                    json!({"name": "etat_du_droit", "arguments": {"question": "bail"}}),
                ];
                for (id, params) in (3..).zip(invalid) {
                    let response = ask(&mut wire, id, "prompts/get", params.clone(), modern).await;
                    assert_eq!(response["error"]["code"], json!(-32602), "{params}: {response}");
                    assert!(response.get("result").is_none(), "{response}");
                }
                // Un prompt n'est pas un outil : ni port, ni audit.
                assert_eq!((fixture.corpus.calls(), fixture.audit.records().len()), (0, 0));
            }
        }

        #[tokio::test]
        async fn completion_only_for_code_variable_over_the_wire() {
            let fixture = Fixture::default();
            fixture.corpus.push_list_codes(Ok(codes()));
            let mut wire = peer(&fixture, false).await;

            // Les slugs, préfixes d'abord ; `total` et `hasMore` renseignés.
            let params = complete_params(CODE_TEMPLATE, "code", "code-d");
            let response = ask(&mut wire, 2, "completion/complete", params, false).await;
            let completion = &response["result"]["completion"];
            assert_eq!(
                completion["values"],
                json!(["code-de-la-securite-sociale", "code-du-travail"]),
                "{response}"
            );
            assert_eq!(completion["total"], json!(2), "{response}");
            assert_eq!(completion["hasMore"], json!(false), "{response}");
            assert_eq!(fixture.corpus.calls_of("list_codes"), 1);

            // Toute autre variable, tout autre gabarit, tout prompt : liste vide, sans port.
            let empty_cases = [
                complete_params(CODE_TEMPLATE, "num", "9"),
                complete_params("legifrance://article/{article_id}", "article_id", "LEGI"),
                complete_params("legifrance://article/{article_id}", "code", "c"),
                json!({
                    "ref": {"type": "ref/prompt", "name": "analyser_decision"},
                    "argument": {"name": "decision_id", "value": "JURI"}
                }),
            ];
            for (id, params) in (3..).zip(empty_cases) {
                let response =
                    ask(&mut wire, id, "completion/complete", params.clone(), false).await;
                assert_eq!(
                    response["result"]["completion"]["values"],
                    json!([]),
                    "{params}: {response}"
                );
                assert!(response.get("error").is_none(), "{response}");
            }
            assert_eq!(fixture.corpus.calls(), 1);
            // Les complétions ne sont pas auditées.
            assert_eq!(fixture.audit.records().len(), 0);
        }

        #[tokio::test]
        async fn completions_are_rate_limited() {
            let fixture = Fixture::default();
            for _ in 0..30 {
                fixture.corpus.push_list_codes(Ok(codes()));
            }
            let mut wire = peer(&fixture, false).await;

            let mut served = Vec::new();
            for id in 2..32 {
                let params = complete_params(CODE_TEMPLATE, "code", "");
                let response = ask(&mut wire, id, "completion/complete", params, false).await;
                // Jamais une erreur : au-delà du débit, une liste vide.
                assert!(response.get("error").is_none(), "{response}");
                let values = response["result"]["completion"]["values"].as_array().unwrap();
                served.push(!values.is_empty());
            }

            // Dix par seconde pour le processus : les premières passent, les dernières non, et
            // un seau qui se remplit à 10 par seconde ne laisse pas passer les trente.
            assert!(served[..10].iter().all(|ok| *ok), "{served:?}");
            assert!(!served[29], "{served:?}");
            let passed = served.iter().filter(|ok| **ok).count();
            assert!((10..30).contains(&passed), "{passed} : {served:?}");
            assert_eq!(fixture.corpus.calls_of("list_codes"), passed);
        }

        #[tokio::test]
        async fn the_in_process_client_lists_reads_and_completes() {
            let fixture = Fixture::default();
            fixture.corpus.push_list_codes(Ok(codes()));
            let client = connect(v03_server(&fixture)).await;
            let info = client.peer_info().expect("le serveur s'est présenté");
            assert!(info.capabilities.resources.is_some());
            assert!(info.capabilities.prompts.is_some());
            assert!(info.capabilities.completions.is_some());

            assert_eq!(client.list_all_resources().await.unwrap().len(), 1);
            assert_eq!(client.list_all_resource_templates().await.unwrap().len(), 4);
            assert_eq!(client.list_all_prompts().await.unwrap().len(), 3);
            let read =
                client.read_resource(ReadResourceRequestParams::new(COVERAGE_URI)).await.unwrap();
            assert_eq!(read.contents.len(), 1);

            // Le client en processus négocie l'ère legacy : -32002.
            let missing = client
                .read_resource(ReadResourceRequestParams::new("legifrance://unknown/x"))
                .await
                .unwrap_err();
            assert!(missing.to_string().starts_with("Mcp error: -32002: "), "{missing}");
            let prompt =
                client.get_prompt(GetPromptRequestParams::new("anything")).await.unwrap_err();
            assert_eq!(prompt.to_string(), "Mcp error: -32602: unknown prompt");
        }
    }
}
