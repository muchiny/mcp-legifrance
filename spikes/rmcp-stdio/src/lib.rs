//! Spike rmcp 3.5.1 (unité 0A du plan « fin du projet » ; T-M0-13, T-M0-14, V-28, V-29).
//!
//! Serveur stdio minimal et générique, `Spike<C: LegalCorpus, A: AuditLog>`, qui éprouve les
//! motifs que copieront `lf-adapter-mcp` (0M, 0W) et les tranches d'outils :
//!
//! - plusieurs blocs `#[tool_router(router = …, vis = "pub(crate)")]` (dont un vide,
//!   `allow_empty`) combinés par `+` dans le champ `tool_router` ;
//! - `#[tool_handler(router = self.tool_router)]` sur l'`impl ServerHandler` ;
//! - un outil qui reçoit ses arguments bruts (`JsonObject`), avec `input_schema` et
//!   `output_schema` explicites, décodés par notre code et audités par le vrai `Audited` ;
//! - le jeton d'annulation de rmcp transmis à `Audited::run` ;
//! - `PreLifecycleFilter` autour d'`AsyncRwTransport::new_server`.
//!
//! Le verdict est dans `README.md`.

use std::{borrow::Cow, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use lf_application::{
    audited::{AuditMetric, AuditMode, AuditPolicy, Audited, CallMeta},
    error::AppError,
    input::{CanonicalInput, InputError, ToolCall},
    ports::{AuditLog, LegalCorpus, PisteEnvironment},
    tools::ToolName,
};
use rmcp::{
    Json, RoleServer, ServerHandler, ServiceExt,
    handler::server::{
        router::tool::ToolRouter,
        tool::{schema_for_input, schema_for_output},
    },
    model::{
        ClientJsonRpcMessage, ClientRequest, ContentBlock, GetMeta, Implementation, IntoContents,
        JsonObject, ProtocolVersion, ServerCapabilities, ServerConfig,
    },
    service::{QuitReason, RxJsonRpcMessage, ServerInitializeError, TxJsonRpcMessage},
    tool, tool_handler, tool_router,
    transport::{Transport, async_rw::AsyncRwTransport},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

/// Versions annoncées (00 §6) ; jamais `ProtocolVersion::KNOWN_VERSIONS`.
pub const SUPPORTED: &[ProtocolVersion] =
    &[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2025_06_18];

/// Description de `fr_search` : une constante, comme `text::FR_SEARCH` (05 §3.1).
pub const FR_SEARCH_DESCRIPTION: &str = "Search Légifrance (spike): free-text query.";

/// Politique d'audit du spike : stricte, 60 s, bac à sable.
#[must_use]
pub fn spike_policy() -> AuditPolicy {
    AuditPolicy {
        mode: AuditMode::Strict,
        deadline: Duration::from_secs(60),
        env: PisteEnvironment::Sandbox,
        server_version: env!("CARGO_PKG_VERSION"),
    }
}

/// Paramètres de `fr_search` (05 §3.3) : `Option` partout, la validation reste au cas d'usage.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    /// Free-text query.
    pub query: Option<String>,
    /// Results per page (1..100).
    pub page_size: Option<i64>,
}

/// Vue de sortie de `fr_search`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchView {
    /// Requête reçue.
    pub query: String,
    /// Taille de page retenue.
    pub page_size: i64,
    /// Résultats (toujours vides dans le spike).
    pub hits: Vec<String>,
}

impl AuditMetric for SearchView {
    fn output_count_or_size(&self) -> u64 {
        self.hits.len() as u64
    }
}

/// Vue de sortie de `fr_coverage` (schéma de sortie déduit du type de retour).
#[derive(Debug, Serialize, JsonSchema)]
pub struct CoverageView {
    /// Nombre d'outils servis.
    pub tools: usize,
}

/// `inputSchema` explicite : `schema_for_input` (`common.rs:77-96`) rend un `Result` que
/// l'attribut n'accepte pas tel quel ; on retire aussi `$schema` (05 §2.5).
///
/// # Panics
///
/// À la construction du routeur si la racine du schéma n'est pas un objet : une erreur de
/// programmation, que le test de descripteurs de 0W attrape avant tout client.
#[must_use]
#[expect(clippy::panic, reason = "défaut de programmation au montage du routeur, jamais à l'appel")]
pub fn input_schema<P: JsonSchema + 'static>() -> Arc<JsonObject> {
    let schema = schema_for_input::<P>()
        .unwrap_or_else(|e| panic!("invalid inputSchema for {}: {e}", std::any::type_name::<P>()));
    without_dollar_schema(&schema)
}

/// `outputSchema` explicite, sans `$schema` (`schema_for_output`, `common.rs:121`).
#[must_use]
pub fn output_schema<V: JsonSchema + 'static>() -> Arc<JsonObject> {
    without_dollar_schema(&schema_for_output::<V>())
}

fn without_dollar_schema(schema: &Arc<JsonObject>) -> Arc<JsonObject> {
    let mut object = schema.as_ref().clone();
    object.remove("$schema");
    Arc::new(object)
}

/// `JsonObject` → `ToolCall<I>` : jamais d'échec ici, l'échec de décodage voyage dans
/// `ToolCall::input` et devient un `invalid_arg` audité (02 §5.4).
pub fn to_call<P, I>(args: JsonObject) -> ToolCall<I>
where
    P: DeserializeOwned + Into<I>,
{
    let value = serde_json::Value::Object(args);
    // serde_json sans `preserve_order` : clés triées, JSON canonique pour l'empreinte d'audit.
    let canonical = CanonicalInput::new(value.to_string());
    let input = serde_path_to_error::deserialize::<_, P>(value)
        .map(Into::into)
        .map_err(|error| InputError { message: error.to_string() });
    ToolCall { input, canonical }
}

/// Échec in-band (05 §3.2) : un bloc texte `[code] message`, `isError: true`, sans
/// `structuredContent` (`handler/server/tool.rs:123-146`).
#[derive(Debug)]
pub struct ToolFailure(pub AppError);

impl IntoContents for ToolFailure {
    fn into_contents(self) -> Vec<ContentBlock> {
        let text = match self.0.code() {
            Some(code) => format!("[{}] {}", code.as_str(), self.0.message()),
            None => self.0.message().to_owned(),
        };
        vec![ContentBlock::text(text)]
    }
}

/// Serveur du spike. Bornes sur la struct (comme `Audited<A: AuditLog>`) ; aucune n'est ajoutée
/// par les macros : `ToolRouter<Self>` exige `Self: Send + Sync + 'static`, déjà vrai.
pub struct Spike<C: LegalCorpus, A: AuditLog> {
    corpus: Arc<C>,
    audited: Arc<Audited<A>>,
    /// Latence simulée du port, pour les tests d'annulation et d'EOF.
    latency: Duration,
    tool_router: ToolRouter<Self>,
}

impl<C: LegalCorpus, A: AuditLog> Spike<C, A> {
    /// Serveur dont le routeur combine les trois blocs `#[tool_router]`.
    #[must_use]
    pub fn new(corpus: Arc<C>, audited: Arc<Audited<A>>) -> Self {
        Self {
            corpus,
            audited,
            latency: Duration::ZERO,
            tool_router: Self::search_router() + Self::coverage_router() + Self::empty_router(),
        }
    }

    /// Latence simulée avant la réponse de `fr_search`.
    #[must_use]
    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = latency;
        self
    }

    /// Le port de corpus (générique `C`).
    #[must_use]
    pub fn corpus(&self) -> &Arc<C> {
        &self.corpus
    }

    /// Le routeur servi.
    #[must_use]
    pub fn router(&self) -> &ToolRouter<Self> {
        &self.tool_router
    }

    /// Le routeur servi, pour ajouter ou retirer une route depuis un test.
    pub fn router_mut(&mut self) -> &mut ToolRouter<Self> {
        &mut self.tool_router
    }
}

#[tool_router(router = search_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog> Spike<C, A> {
    #[tool(
        name = "fr_search",
        title = "Search Légifrance (spike)",
        description = FR_SEARCH_DESCRIPTION,
        input_schema = input_schema::<SearchParams>(),
        output_schema = output_schema::<SearchView>(),
        annotations(
            title = "Search Légifrance (spike)",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_search(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<SearchView>, ToolFailure> {
        let call = to_call::<SearchParams, SearchParams>(args);
        let meta = CallMeta::new(ToolName::FR_SEARCH, DateTime::<Utc>::UNIX_EPOCH, call.canonical);
        let latency = self.latency;
        let work = async move {
            let input = call.input?;
            let query = input
                .query
                .filter(|query| !query.trim().is_empty())
                .ok_or_else(|| AppError::invalid_argument("query is required."))?;
            tokio::time::sleep(latency).await;
            Ok(SearchView { query, page_size: input.page_size.unwrap_or(10), hits: Vec::new() })
        };
        let view = self.audited.run(meta, ct.cancelled(), work).await.map_err(ToolFailure)?;
        Ok(Json(view))
    }
}

#[tool_router(router = coverage_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog> Spike<C, A> {
    /// Coverage of the spike server.
    #[tool(name = "fr_coverage", annotations(read_only_hint = true))]
    async fn fr_coverage(&self) -> Result<Json<CoverageView>, ToolFailure> {
        Ok(Json(CoverageView { tools: self.tool_router.list_all().len() }))
    }
}

// Forme des stubs de 0W : un bloc vide par outil, combiné dès V0.
#[tool_router(router = empty_router, vis = "pub(crate)", allow_empty)]
impl<C: LegalCorpus, A: AuditLog> Spike<C, A> {}

// `#[tool_handler]` génère des `async fn` sans `.await` (`list_tools`, `get_tool`) : pedantic les
// signale sur le site de la macro, donc 0M et 0W poseront le même `expect`.
#[expect(
    clippy::unused_async_trait_impl,
    reason = "méthodes async générées par #[tool_handler], sans .await"
)]
#[tool_handler(router = self.tool_router)]
impl<C: LegalCorpus, A: AuditLog> ServerHandler for Spike<C, A> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("rmcp-stdio-spike", env!("CARGO_PKG_VERSION")))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED)
    }
}

/// Écarte notifications, réponses et erreurs du client tant qu'aucune ère n'est ouverte :
/// rmcp 3.5.1 arrête le service sur un tel message (`service/server.rs:681-685`). Annule aussi
/// `eof` à la fin de l'entrée, pour que l'arrêt n'attende pas les appels en vol.
pub struct PreLifecycleFilter<T> {
    inner: T,
    opened: bool,
    eof: Option<CancellationToken>,
}

impl<T> PreLifecycleFilter<T> {
    /// Enveloppe `inner`.
    pub fn new(inner: T) -> Self {
        Self { inner, opened: false, eof: None }
    }

    /// Annule `token` quand l'entrée se ferme (EOF).
    #[must_use]
    pub fn cancel_on_eof(mut self, token: CancellationToken) -> Self {
        self.eof = Some(token);
        self
    }
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for PreLifecycleFilter<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            let Some(message) = self.inner.receive().await else {
                if let Some(eof) = &self.eof {
                    eof.cancel();
                }
                return None;
            };
            if self.opened {
                return Some(message);
            }
            if let ClientJsonRpcMessage::Request(request) = &message {
                self.opened = opens_lifecycle(&request.request);
                return Some(message);
            }
            tracing::debug!("pre-lifecycle client message dropped");
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

/// `initialize`, ou requête autre que `server/discover` et `ping` dont le `_meta` est complet et
/// la version dans [`SUPPORTED`] : la règle d'ouverture de rmcp (`service/server.rs:597-668`).
fn opens_lifecycle(request: &ClientRequest) -> bool {
    match request {
        ClientRequest::InitializeRequest(_) => true,
        ClientRequest::DiscoverRequest(_) | ClientRequest::PingRequest(_) => false,
        other => {
            let meta = other.get_meta();
            meta.missing_required_keys(&ProtocolVersion::V_2026_07_28).is_empty()
                && meta.protocol_version().is_some_and(|version| SUPPORTED.contains(&version))
        }
    }
}

/// Fin normale du service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeOutcome {
    /// Entrée fermée (EOF), avant ou après l'ouverture d'une ère : code 0.
    Eof,
    /// Arrêt demandé par `shutdown` (signal) : code 0.
    Shutdown,
}

/// Échec du service : code 1.
#[derive(Debug)]
pub enum ServeError {
    /// Initialisation impossible (hors EOF et arrêt). Boîtée : `ServerInitializeError` pèse
    /// 368 octets (`clippy::large_enum_variant` et `result_large_err`).
    Initialize(Box<ServerInitializeError>),
    /// La tâche de service a paniqué.
    Join(tokio::task::JoinError),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initialize(error) => write!(f, "MCP initialization failed: {error}"),
            Self::Join(error) => write!(f, "MCP service task failed: {error}"),
        }
    }
}

impl std::error::Error for ServeError {}

/// Sert `server` sur `read`/`write` derrière [`PreLifecycleFilter`] ; `serve_stdio` y passe
/// `tokio::io::stdin()` et `tokio::io::stdout()`.
///
/// # Errors
///
/// [`ServeError`] si l'initialisation échoue autrement que par EOF ou arrêt, ou si la tâche de
/// service panique.
pub async fn serve_io<S, R, W>(
    server: S,
    read: R,
    write: W,
    shutdown: CancellationToken,
) -> Result<ServeOutcome, ServeError>
where
    S: ServerHandler,
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    // Jeton enfant : l'EOF l'annule (appels en vol abandonnés) sans toucher `shutdown`, qui seul
    // distingue l'arrêt demandé de l'EOF.
    let serving = shutdown.child_token();
    let transport = PreLifecycleFilter::new(AsyncRwTransport::new_server(read, write))
        .cancel_on_eof(serving.clone());
    match server.serve_with_ct(transport, serving).await {
        Ok(running) => match running.waiting().await {
            Ok(QuitReason::JoinError(error)) | Err(error) => return Err(ServeError::Join(error)),
            Ok(_) => {}
        },
        Err(ServerInitializeError::ConnectionClosed(_) | ServerInitializeError::Cancelled) => {}
        Err(error) => return Err(ServeError::Initialize(Box::new(error))),
    }
    Ok(if shutdown.is_cancelled() { ServeOutcome::Shutdown } else { ServeOutcome::Eof })
}
