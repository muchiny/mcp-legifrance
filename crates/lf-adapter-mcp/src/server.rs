//! Le serveur MCP : `LegifranceServer` assemble les 13 routeurs d'outils et délègue les
//! resources, les prompts et les complétions (05 §3, §8).
//!
//! Le serveur est générique sur les trois ports de la façade (`C`, `A`, `K`), comme
//! `LegalService`. Les macros rmcp n'ajoutent aucune borne : `LegalCorpus`, `AuditLog` et `Clock`
//! portent déjà `Send + Sync + 'static`, ce qu'exige `ToolRouter<Self>` (spike 0A, V-15).

use std::{borrow::Cow, sync::Arc};

use lf_application::{
    ports::{AuditLog, Clock, LegalCorpus},
    service::LegalService,
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{
        CompleteRequestParams, CompleteResult, GetPromptRequestParams, GetPromptResponse,
        Implementation, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
        PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
        ServerCapabilities, ServerConfig,
    },
    service::RequestContext,
    tool_handler,
};

use crate::{completions, prompts, resources, versions::SUPPORTED};

/// Nombre d'outils du catalogue (05 §3) : un routeur par fichier de `tools/`.
const ROUTERS: usize = 13;

/// Serveur MCP de Légifrance : les outils, puis les resources, prompts et complétions.
pub struct LegifranceServer<C: LegalCorpus, A: AuditLog, K: Clock> {
    /// La façade des cas d'usage, lue par les outils (`tools/*.rs`).
    #[allow(dead_code, reason = "STUB(V0) → V1-01 : lu par les outils des tranches (tools/*.rs)")]
    pub(crate) service: Arc<LegalService<C, A, K>>,
    /// Le routeur servi : le seul que lit `#[tool_handler]`.
    tool_router: ToolRouter<Self>,
}

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    /// Serveur dont le routeur additionne les treize routeurs d'outils de `tools/`.
    #[must_use]
    pub fn new(service: Arc<LegalService<C, A, K>>) -> Self {
        let tool_router =
            Self::routers().into_iter().fold(ToolRouter::new(), |all, router| all + router);
        Self { service, tool_router }
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

// `#[tool_handler]` génère des `async fn` sans `.await` (`list_tools`, `get_tool`, `call_tool`) :
// pedantic les signale sur le site de la macro ; les délégations ci-dessous aussi.
#[expect(
    clippy::unused_async_trait_impl,
    reason = "méthodes async générées par #[tool_handler] et délégations sans .await"
)]
#[tool_handler(router = self.tool_router)]
impl<C: LegalCorpus, A: AuditLog, K: Clock> ServerHandler for LegifranceServer<C, A, K> {
    // STUB(V0) → V1-10 : `get_info` minimal (outils seuls) ; V1-10 y met les instructions, le
    // titre et les capacités des deux ères, V3-07 celles des resources et des prompts.
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mcp-legifrance", env!("CARGO_PKG_VERSION")))
    }

    /// Les trois versions de 00 §6, jamais `ProtocolVersion::KNOWN_VERSIONS`.
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED)
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
    use std::time::Duration;

    use super::*;

    type TestServer = LegifranceServer<FakeCorpus, MemoryAuditLog, FixedClock>;

    fn server() -> TestServer {
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
        LegifranceServer::new(Arc::new(service))
    }

    #[test]
    fn the_thirteen_routers_are_empty_until_their_slice() {
        assert_eq!(TestServer::routers().len(), ROUTERS);
        for router in TestServer::routers() {
            assert_eq!(router.list_all().len(), 0);
        }
        assert_eq!(server().tool_router.list_all().len(), 0);
    }

    #[test]
    fn get_info_announces_the_server_and_the_tools_capability_only() {
        let info = server().get_info();
        assert_eq!(info.server_info.name, "mcp-legifrance");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_none());
        assert!(info.capabilities.prompts.is_none());
        assert!(info.capabilities.completions.is_none());
    }

    #[test]
    fn the_supported_versions_are_ours_not_the_sdk_list() {
        let versions = server().supported_protocol_versions();
        let listed: Vec<&str> = versions.iter().map(ProtocolVersion::as_str).collect();
        assert_eq!(listed, ["2026-07-28", "2025-11-25", "2025-06-18"]);
    }

    #[test]
    fn the_service_is_shared_not_copied() {
        let service = Arc::clone(&server().service);
        assert_eq!(service.settings().policy.server_version, "0.0.0");
    }

    /// Les tests de bout en bout passent par `testing::connect` (feature `test-support`).
    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use std::{future::Future, pin::Pin};

        use rmcp::{
            handler::server::{
                common::schema_for_empty_input, router::tool::ToolRoute, tool::ToolCallContext,
            },
            model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Tool},
        };
        use serde_json::json;

        use super::*;
        use crate::testing::connect;
        use pretty_assertions::assert_eq;

        /// Réponse boxée d'une route écrite à la main (`DynCallToolHandler`).
        type RouteFuture<'a> =
            Pin<Box<dyn Future<Output = Result<CallToolResponse, ErrorData>> + Send + 'a>>;

        /// Route de test : rend ses arguments bruts en texte.
        fn echo_route(context: ToolCallContext<'_, TestServer>) -> RouteFuture<'_> {
            let arguments = context.arguments.unwrap_or_default();
            Box::pin(async move {
                let text = serde_json::to_string(&arguments).unwrap();
                Ok(CallToolResponse::from(CallToolResult::success(vec![ContentBlock::text(text)])))
            })
        }

        #[tokio::test]
        async fn the_handshake_reports_the_server_and_tools_only() {
            let client = connect(server()).await;
            let info = client.peer_info().expect("le serveur s'est présenté");
            assert_eq!(info.server_info.as_ref().unwrap().name, "mcp-legifrance");
            assert!(info.capabilities.tools.is_some());
            assert!(info.capabilities.resources.is_none());
        }

        #[tokio::test]
        async fn a_route_added_to_the_router_field_is_served() {
            let mut server = server();
            server.router_mut().add_route(ToolRoute::new_dyn(
                Tool::new("echo", "echo the arguments", schema_for_empty_input()),
                echo_route,
            ));
            let client = connect(server).await;

            let names: Vec<_> =
                client.list_all_tools().await.unwrap().into_iter().map(|tool| tool.name).collect();
            assert_eq!(names, ["echo"]);

            let mut params = CallToolRequestParams::new("echo");
            params.arguments = json!({"a": [1, 2]}).as_object().cloned();
            let result = client.call_tool(params).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            assert_eq!(result.content[0].as_text().unwrap().text, r#"{"a":[1,2]}"#);
        }

        #[tokio::test]
        async fn an_unknown_tool_is_a_jsonrpc_error() {
            let client = connect(server()).await;
            let error = client.call_tool(CallToolRequestParams::new("fr_nope")).await.unwrap_err();
            assert!(error.to_string().starts_with("Mcp error: -32602"), "{error}");
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
