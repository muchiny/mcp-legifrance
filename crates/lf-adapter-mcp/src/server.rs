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
        ServerCapabilities, ServerConfig,
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
    #[allow(dead_code, reason = "lu par les outils des tranches (tools/*.rs), pas encore écrits")]
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

    /// `tools/list` : les routes, triées par nom ; `ttlMs` et `cacheScope` aux seuls pairs
    /// modernes (05 §2.3).
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let listed = ListToolsResult::with_all_items(self.tool_router.list_all());
        Ok(if is_modern(&context) {
            listed.with_ttl_ms(CACHE_TTL_MS).with_cache_scope(CacheScope::Public)
        } else {
            listed
        })
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

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(resources::list())
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(resources::list_templates())
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        Err(resources::read())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(prompts::list())
    }

    async fn get_prompt(
        &self,
        _request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        Err(prompts::get())
    }

    async fn complete(
        &self,
        _request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, ErrorData> {
        Ok(completions::complete())
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

        #[tokio::test]
        async fn resources_prompts_and_completions_are_empty_stubs() {
            let client = connect(server()).await;
            assert_eq!(client.list_all_resources().await.unwrap().len(), 0);
            assert_eq!(client.list_all_resource_templates().await.unwrap().len(), 0);
            assert_eq!(client.list_all_prompts().await.unwrap().len(), 0);
            assert_eq!(client.complete_prompt_simple("any", "arg", "val").await.unwrap().len(), 0);
        }

        #[tokio::test]
        async fn reading_a_resource_is_not_found_and_getting_a_prompt_is_invalid_params() {
            let client = connect(server()).await;
            // Le client en processus négocie l'ère legacy : -32002. L'ère moderne rend -32602
            // (SEP-2164) ; V3-07 fige le code par ère.
            let read = client
                .read_resource(ReadResourceRequestParams::new("legifrance://coverage"))
                .await
                .unwrap_err();
            assert_eq!(read.to_string(), "Mcp error: -32002: resource not found");
            let prompt =
                client.get_prompt(GetPromptRequestParams::new("anything")).await.unwrap_err();
            assert_eq!(prompt.to_string(), "Mcp error: -32602: unknown prompt");
        }
    }
}
