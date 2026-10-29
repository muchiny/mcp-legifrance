//! Serveur de la suite de conformité officielle HTTP (T-M8-04, 06 §6.2, annexe D §8.3). Un
//! binaire de test, **jamais distribué** : le job `conformance-http` le lance, puis lui oppose
//! `@modelcontextprotocol/conformance`.
//!
//! ```text
//! cargo run -p lf-adapter-mcp --example conformance_server --all-features -- [--port 3000]
//! ```
//!
//! Il sert [`LegifranceServer`] sur `127.0.0.1` seulement, avec le corpus factice de
//! `lf_adapter_mcp::testing` (aucun réseau, aucun identifiant), **sans authentification**, par
//! `serve_http` : la configuration HTTP est celle de la production (annexe D §5.2). Le serveur de
//! production n'est pas modifié. La suite exige des outils, des resources et des prompts de test
//! (`test_simple_text`, `test://static-text`, `test_simple_prompt`...) que le catalogue de
//! production ne porte pas : `ConformanceServer` les ajoute devant lui et lui délègue tout le
//! reste. Ses capacités annoncent ce qu'il sert (outils, resources, prompts, complétions).
//!
//! Ce que le serveur ne sert pas, et que la suite éprouve quand même (journalisation,
//! échantillonnage, sollicitation, abonnement aux resources, résultats `input_required`), est porté
//! avec sa raison dans `tests/conformance-baseline.yml`.
//!
//! La logique n'existe qu'avec les features `http` (stub `serve_http` du préambule M8) et
//! `test-support` (corpus factice) : `--all-features` les active. Sans elles, l'exemple se réduit
//! à un message d'erreur, pour que `cargo test` ou `cargo build --examples` hors `--all-features`
//! compilent encore.

// La feature `http` est déclarée par le préambule M8 (V4) ; tant qu'elle manque, le lint `unexpected_cfgs` la signale.
#![allow(
    unexpected_cfgs,
    reason = "la feature `http` vient du préambule M8 ; sans elle l'exemple se réduit à son message"
)]

use std::process::ExitCode;

/// Logique de l'exemple : compilée avec `http` et `test-support` seulement.
#[cfg(all(feature = "http", feature = "test-support"))]
mod harness {
    use std::{
        borrow::Cow,
        net::{Ipv4Addr, SocketAddr},
        process::ExitCode,
        sync::Arc,
        time::Duration,
    };

    use lf_adapter_mcp::{
        HttpOptions, ServeOutcome, serve_http,
        testing::{Fixture, FixtureServer},
    };
    use rmcp::{
        ErrorData, RoleServer, ServerHandler,
        model::{
            CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult,
            ClientCapabilities, CompleteRequestParams, CompleteResult, ContentBlock,
            DiscoverResult, GetPromptRequestParams, GetPromptResponse, GetPromptResult, JsonObject,
            ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
            PaginatedRequestParams, ProgressNotificationParam, Prompt, PromptArgument,
            PromptMessage, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
            ReadResourceResult, Resource, ResourceContents, ResourceTemplate, Role,
            ServerCapabilities, ServerConfig, SubscriptionFilter, Tool,
        },
        service::RequestContext,
    };
    use serde_json::{Value, json};
    use tokio_util::sync::CancellationToken;

    /// Port d'écoute par défaut, celui de la commande du serveur Rust dans la suite.
    const DEFAULT_PORT: u16 = 3000;

    /// Image PNG d'un pixel, en base64 (`test_image_content`, `test://static-binary`).
    const TEST_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

    /// Fichier WAV minimal, en base64 (`test_audio_content`).
    const TEST_WAV: &str = "UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAAIAEAABAAgAZGF0YQAAAAA=";

    /// Fraîcheur annoncée aux pairs modernes pour les lectures de test : une heure, comme le
    /// serveur de production (Q8).
    const CACHE_TTL_MS: u64 = 3_600_000;

    /// Début des URI du gabarit de test (`test://template/{id}/data`).
    const TEST_TEMPLATE_PREFIX: &str = "test://template/";

    /// Fin des URI du gabarit de test.
    const TEST_TEMPLATE_SUFFIX: &str = "/data";

    /// Le serveur de production, précédé des outils, resources et prompts de test de la suite.
    #[derive(Clone)]
    struct ConformanceServer {
        /// `LegifranceServer` sur les doubles de `testing::Fixture` : tout ce que la suite
        /// n'adresse pas à un élément de test lui est délégué.
        inner: Arc<FixtureServer>,
    }

    /// Capacités servies : celles de la surface complète (outils, resources, prompts, complétions).
    /// La surface annoncée par `SURFACE` peut être plus étroite que ce que le serveur sait faire ;
    /// la suite compare `server/discover` aux méthodes qui répondent.
    fn capabilities() -> ServerCapabilities {
        ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_prompts()
            .enable_completions()
            .build()
    }

    /// Schéma d'entrée d'un outil de test : un objet sans propriété.
    fn empty_object_schema() -> Arc<JsonObject> {
        let mut schema = JsonObject::new();
        schema.insert("type".to_owned(), json!("object"));
        schema.insert("properties".to_owned(), json!({}));
        Arc::new(schema)
    }

    /// Les outils de test, avec leur description (exigée par `tools-list`).
    fn test_tools() -> Vec<Tool> {
        [
            ("test_simple_text", "Returns a fixed text (conformance fixture)."),
            ("test_image_content", "Returns a one-pixel PNG (conformance fixture)."),
            ("test_audio_content", "Returns a minimal WAV file (conformance fixture)."),
            ("test_embedded_resource", "Returns an embedded text resource (conformance fixture)."),
            (
                "test_multiple_content_types",
                "Returns text, image and embedded resource (conformance fixture).",
            ),
            ("test_error_handling", "Always returns an error result (conformance fixture)."),
            (
                "test_tool_with_progress",
                "Sends three progress notifications, 50 ms apart (conformance fixture).",
            ),
            (
                "test_missing_capability",
                "Requires the client to declare sampling, else answers -32021 (conformance fixture).",
            ),
            (
                "test_logging_tool",
                "Returns a text and logs nothing when no log level is set (conformance fixture).",
            ),
        ]
        .into_iter()
        .map(|(name, description)| Tool::new(name, description, empty_object_schema()))
        .collect()
    }

    /// Le résultat de l'outil de test `name` ; `None` si `name` n'en est pas un.
    async fn call_test_tool(
        name: &str,
        context: &RequestContext<RoleServer>,
    ) -> Option<Result<CallToolResult, ErrorData>> {
        let reply = |blocks: Vec<ContentBlock>| Some(Ok(CallToolResult::success(blocks)));
        match name {
            "test_simple_text" => {
                reply(vec![ContentBlock::text("This is a simple text response for testing.")])
            }
            "test_image_content" => reply(vec![ContentBlock::image(TEST_PNG, "image/png")]),
            "test_audio_content" => reply(vec![ContentBlock::audio(TEST_WAV, "audio/wav")]),
            "test_embedded_resource" => reply(vec![ContentBlock::resource(
                ResourceContents::text(
                    "This is an embedded resource content.",
                    "test://embedded-resource",
                )
                .with_mime_type("text/plain"),
            )]),
            "test_multiple_content_types" => reply(vec![
                ContentBlock::text("Multiple content types test:"),
                ContentBlock::image(TEST_PNG, "image/png"),
                ContentBlock::resource(
                    ResourceContents::text(
                        r#"{"test":"data","value":123}"#,
                        "test://mixed-content-resource",
                    )
                    .with_mime_type("application/json"),
                ),
            ]),
            "test_error_handling" => Some(Ok(CallToolResult::error(vec![ContentBlock::text(
                "This tool intentionally returns an error for testing",
            )]))),
            "test_logging_tool" => {
                reply(vec![ContentBlock::text("Logging tool executed without a log level.")])
            }
            "test_tool_with_progress" => {
                report_progress(context).await;
                reply(vec![ContentBlock::text("Progress test completed.")])
            }
            "test_missing_capability" => Some(require_sampling(context)),
            _ => None,
        }
    }

    /// Trois notifications de progression (0, 50 et 100 sur 100), espacées de 50 ms, si la requête
    /// porte un `progressToken` ; seulement les délais sinon.
    async fn report_progress(context: &RequestContext<RoleServer>) {
        let token = context.meta.get_progress_token();
        for step in [0.0_f64, 50.0, 100.0] {
            if let Some(token) = token.clone() {
                let params = ProgressNotificationParam::new(token, step).with_total(100.0);
                // Un client parti ne change pas le résultat de l'outil.
                drop(context.peer.notify_progress(params).await);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// `test_missing_capability` (SEP-2575) : refuse, en `-32021`, un client qui n'a pas déclaré
    /// `sampling` dans `_meta.io.modelcontextprotocol/clientCapabilities`.
    #[expect(
        deprecated,
        reason = "l'échantillonnage est déprécié (SEP-2577), mais la suite exige un -32021 qui le nomme"
    )]
    fn require_sampling(context: &RequestContext<RoleServer>) -> Result<CallToolResult, ErrorData> {
        let declared = context.meta.client_capabilities().is_some_and(|c| c.sampling.is_some());
        if declared {
            Ok(CallToolResult::success(vec![ContentBlock::text("Sampling is declared.")]))
        } else {
            Err(ErrorData::missing_required_client_capability(
                ClientCapabilities::builder().enable_sampling().build(),
            ))
        }
    }

    /// Les resources directes de test.
    fn test_resources() -> Vec<Resource> {
        vec![
            Resource::new("test://static-text", "static-text")
                .with_description("A static text resource (conformance fixture).")
                .with_mime_type("text/plain"),
            Resource::new("test://static-binary", "static-binary")
                .with_description("A static PNG resource (conformance fixture).")
                .with_mime_type("image/png"),
        ]
    }

    /// Le gabarit de resource de test.
    fn test_templates() -> Vec<ResourceTemplate> {
        vec![
            ResourceTemplate::new("test://template/{id}/data", "template-data")
                .with_description("A templated JSON resource (conformance fixture).")
                .with_mime_type("application/json"),
        ]
    }

    /// Le contenu de la resource de test `uri` ; `None` si ce n'en est pas une.
    fn read_test_resource(uri: &str) -> Option<ResourceContents> {
        match uri {
            "test://static-text" => Some(
                ResourceContents::text("This is the content of the static text resource.", uri)
                    .with_mime_type("text/plain"),
            ),
            "test://static-binary" => {
                Some(ResourceContents::blob(TEST_PNG, uri).with_mime_type("image/png"))
            }
            _ => {
                let id =
                    uri.strip_prefix(TEST_TEMPLATE_PREFIX)?.strip_suffix(TEST_TEMPLATE_SUFFIX)?;
                let body =
                    json!({"id": id, "templateTest": true, "data": format!("Data for ID: {id}")});
                Some(
                    ResourceContents::text(body.to_string(), uri)
                        .with_mime_type("application/json"),
                )
            }
        }
    }

    /// Les prompts de test.
    fn test_prompts() -> Vec<Prompt> {
        let argument = |name: &str, description: &str| {
            PromptArgument::new(name).with_description(description).with_required(true)
        };
        vec![
            Prompt::new(
                "test_simple_prompt",
                Some("A prompt without argument (conformance fixture)."),
                None,
            ),
            Prompt::new(
                "test_prompt_with_arguments",
                Some("A prompt with two required arguments (conformance fixture)."),
                Some(vec![
                    argument("arg1", "First test argument"),
                    argument("arg2", "Second test argument"),
                ]),
            ),
            Prompt::new(
                "test_prompt_with_embedded_resource",
                Some("A prompt that embeds a resource (conformance fixture)."),
                Some(vec![argument("resourceUri", "URI of the resource to embed")]),
            ),
            Prompt::new(
                "test_prompt_with_image",
                Some("A prompt that carries an image (conformance fixture)."),
                None,
            ),
        ]
    }

    /// Un argument de prompt requis, ou `-32602`.
    fn required_argument(
        request: &GetPromptRequestParams,
        name: &str,
    ) -> Result<String, ErrorData> {
        request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| ErrorData::invalid_params(format!("{name} is required"), None))
    }

    /// Le résultat du prompt de test `request.name` ; `None` si ce n'en est pas un.
    fn get_test_prompt(
        request: &GetPromptRequestParams,
    ) -> Option<Result<GetPromptResult, ErrorData>> {
        let user_text = |text: String| PromptMessage::new_text(Role::User, text);
        let messages = match request.name.as_str() {
            "test_simple_prompt" => {
                Ok(vec![user_text("This is a simple prompt for testing.".to_owned())])
            }
            "test_prompt_with_arguments" => required_argument(request, "arg1").and_then(|arg1| {
                let arg2 = required_argument(request, "arg2")?;
                Ok(vec![user_text(format!("Prompt with arguments: arg1='{arg1}', arg2='{arg2}'"))])
            }),
            "test_prompt_with_embedded_resource" => {
                required_argument(request, "resourceUri").map(|uri| {
                    let embedded =
                        ResourceContents::text("Embedded resource content for testing.", uri)
                            .with_mime_type("text/plain");
                    vec![
                        PromptMessage::new(Role::User, ContentBlock::resource(embedded)),
                        user_text("Please process the embedded resource above.".to_owned()),
                    ]
                })
            }
            "test_prompt_with_image" => Ok(vec![
                PromptMessage::new(Role::User, ContentBlock::image(TEST_PNG, "image/png")),
                user_text("Please analyze the image above.".to_owned()),
            ]),
            _ => return None,
        };
        Some(messages.map(GetPromptResult::new))
    }

    /// Vrai si la requête est servie en ère moderne (2026-07-28 ou plus), seule à connaître
    /// `ttlMs` et `cacheScope`.
    fn is_modern(context: &RequestContext<RoleServer>) -> bool {
        context.protocol_version().is_some_and(|version| version >= ProtocolVersion::V_2026_07_28)
    }

    // Les méthodes que `LegifranceServer` redéfinit sont reprises une à une : les autres
    // (`ping`, `initialize`, `set_level`, `subscribe`...) gardent le comportement par défaut de
    // rmcp, qui est aussi celui du serveur de production.
    impl ServerHandler for ConformanceServer {
        fn get_info(&self) -> ServerConfig {
            let mut info = self.inner.get_info();
            info.capabilities = capabilities();
            info
        }

        fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
            self.inner.supported_protocol_versions()
        }

        async fn discover(
            &self,
            context: RequestContext<RoleServer>,
        ) -> Result<DiscoverResult, ErrorData> {
            let mut discovered = self.inner.discover(context).await?;
            discovered.capabilities = capabilities();
            Ok(discovered)
        }

        fn accepted_subscription_filter(
            &self,
            requested: &SubscriptionFilter,
        ) -> Option<SubscriptionFilter> {
            self.inner.accepted_subscription_filter(requested)
        }

        fn get_tool(&self, name: &str) -> Option<Tool> {
            self.inner.get_tool(name)
        }

        async fn list_tools(
            &self,
            request: Option<PaginatedRequestParams>,
            context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            let mut listed = self.inner.list_tools(request, context).await?;
            listed.tools.extend(test_tools());
            Ok(listed)
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParams,
            context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, ErrorData> {
            match call_test_tool(&request.name, &context).await {
                Some(result) => result.map(CallToolResponse::from),
                None => self.inner.call_tool(request, context).await,
            }
        }

        async fn list_resources(
            &self,
            request: Option<PaginatedRequestParams>,
            context: RequestContext<RoleServer>,
        ) -> Result<ListResourcesResult, ErrorData> {
            let mut listed = self.inner.list_resources(request, context).await?;
            // En tête : le scénario `caching` lit la première resource de la liste.
            listed.resources.splice(0..0, test_resources());
            Ok(listed)
        }

        async fn list_resource_templates(
            &self,
            request: Option<PaginatedRequestParams>,
            context: RequestContext<RoleServer>,
        ) -> Result<ListResourceTemplatesResult, ErrorData> {
            let mut listed = self.inner.list_resource_templates(request, context).await?;
            listed.resource_templates.extend(test_templates());
            Ok(listed)
        }

        async fn read_resource(
            &self,
            request: ReadResourceRequestParams,
            context: RequestContext<RoleServer>,
        ) -> Result<ReadResourceResponse, ErrorData> {
            let Some(contents) = read_test_resource(&request.uri) else {
                if request.uri.starts_with("test://") {
                    // SEP-2164 : introuvable avec l'URI en `data` ; rmcp réécrit le code en -32602 pour
                    // les pairs modernes. La production, elle, ne cite pas une URI hors de sa grammaire.
                    return Err(ErrorData::resource_not_found(
                        "Resource not found",
                        Some(json!({ "uri": request.uri })),
                    ));
                }
                return self.inner.read_resource(request, context).await;
            };
            let read = ReadResourceResult::new(vec![contents]);
            let read = if is_modern(&context) {
                read.with_ttl_ms(CACHE_TTL_MS).with_cache_scope(CacheScope::Public)
            } else {
                read
            };
            Ok(ReadResourceResponse::from(read))
        }

        async fn list_prompts(
            &self,
            request: Option<PaginatedRequestParams>,
            context: RequestContext<RoleServer>,
        ) -> Result<ListPromptsResult, ErrorData> {
            let mut listed = self.inner.list_prompts(request, context).await?;
            listed.prompts.extend(test_prompts());
            Ok(listed)
        }

        async fn get_prompt(
            &self,
            request: GetPromptRequestParams,
            context: RequestContext<RoleServer>,
        ) -> Result<GetPromptResponse, ErrorData> {
            match get_test_prompt(&request) {
                Some(result) => result.map(GetPromptResponse::from),
                None => self.inner.get_prompt(request, context).await,
            }
        }

        async fn complete(
            &self,
            request: CompleteRequestParams,
            context: RequestContext<RoleServer>,
        ) -> Result<CompleteResult, ErrorData> {
            self.inner.complete(request, context).await
        }
    }

    /// Le port demandé par `--port <n>` (3000 par défaut), ou le message d'usage.
    fn port(args: impl IntoIterator<Item = String>) -> Result<u16, String> {
        let mut args = args.into_iter();
        let mut port = DEFAULT_PORT;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--port" => {
                    port = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .ok_or_else(|| "--port expects a port number".to_owned())?;
                }
                other => return Err(format!("unexpected argument `{other}` (usage: --port <n>)")),
            }
        }
        Ok(port)
    }

    /// Écrit sur stderr : l'exemple n'a ni journal ni sortie JSON-RPC.
    #[expect(
        clippy::print_stderr,
        reason = "binaire de test jamais distribué : annonce l'adresse et les échecs sur stderr"
    )]
    fn announce(message: &str) {
        eprintln!("{message}");
    }

    /// Point d'entrée : sert jusqu'à ce que le processus soit tué (la CI l'arrête à la fin du job).
    #[tokio::main(flavor = "current_thread")]
    pub(super) async fn run() -> ExitCode {
        let port = match port(std::env::args().skip(1)) {
            Ok(port) => port,
            Err(message) => {
                announce(&message);
                return ExitCode::from(2);
            }
        };
        let listen = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let server = ConformanceServer { inner: Arc::new(Fixture::default().server()) };
        announce(&format!("conformance_server: listening on http://{listen}/mcp"));
        // Sans jeton : la suite de conformité n'envoie aucun en-tête d'authentification (V-43).
        let options = match HttpOptions::local(listen) {
            Ok(options) => options,
            Err(error) => {
                announce(&format!("conformance_server: {error}"));
                return ExitCode::FAILURE;
            }
        };
        match serve_http(move || server.clone(), options, CancellationToken::new()).await {
            Ok(ServeOutcome::Eof | ServeOutcome::Shutdown) => ExitCode::SUCCESS,
            Err(error) => {
                announce(&format!("conformance_server: {error}"));
                ExitCode::FAILURE
            }
        }
    }
}

/// Avec `http` et `test-support` : le serveur de conformité.
#[cfg(all(feature = "http", feature = "test-support"))]
fn main() -> ExitCode {
    harness::run()
}

/// Sans l'une des deux features : rien à servir.
#[cfg(not(all(feature = "http", feature = "test-support")))]
#[expect(
    clippy::print_stderr,
    reason = "binaire de test jamais distribué : explique pourquoi il ne sert rien"
)]
fn main() -> ExitCode {
    eprintln!(
        "conformance_server needs the `http` and `test-support` features (use --all-features)"
    );
    ExitCode::from(2)
}
