//! Transport Streamable HTTP sans session (M8, profil local seul, Q4).
//!
//! [`serve_http`] sert un serveur MCP derrière un routeur axum (annexe D §5.2, §6.2) :
//!
//! - **un seul endpoint**, `POST /mcp`, sans session : GET et DELETE reçoivent 405 avec
//!   `Allow: POST` (C-40, V-40), aucun `Mcp-Session-Id` n'est émis ; les clients legacy sont servis
//!   par `initialize` à chaque requête (`with_legacy_session_mode(false)`) ;
//! - **écoute sur la boucle locale seulement** : [`HttpOptions::local`] refuse toute adresse non
//!   loopback (C-43) ; `Host` hors de la liste admise est rejeté par rmcp ;
//! - **`Origin` validé par nous** (`with_allowed_origins` + `enforce_origin_validation()`) : rmcp ne
//!   valide rien avec la liste vide par défaut ; un `Origin` hors liste reçoit 403 (C-42) ;
//! - **jeton statique** (profil local, C-56) : `Authorization: Bearer <jeton>` sur chaque
//!   requête, comparé en temps constant ; sinon 401 et `WWW-Authenticate`, sans corps ; le jeton
//!   n'est ni journalisé ni affiché par `Debug` ;
//! - **couche de version** devant rmcp (C-48, V-39) : un POST autre qu'`initialize` sans
//!   `MCP-Protocol-Version` reçoit 400 et `-32020` ; sans `_meta` moderne, un en-tête qui nomme une
//!   version hors de [`SUPPORTED`] (`2025-03-26`, `2024-11-05`, ou toute autre) reçoit 400 et
//!   `-32022`, avec `data.supported`, `data.requested` et l'`id` repris. rmcp ne rejette aucun des
//!   deux cas : il suppose 2025-03-26 quand l'en-tête manque, et ne compare l'en-tête qu'à
//!   `KNOWN_VERSIONS` ;
//! - **débit des invocations** (C-34) : un seau à jetons par processus borne les `tools/call` ;
//!   au-delà, 429 et `Retry-After`. Les valeurs sont figées ([`Limits::PRODUCTION`]), sans
//!   nouvelle variable d'environnement (00 §1, §6) ;
//! - **corps borné à 256 Kio** (C-54) : 413 au-delà ;
//! - la fabrique sans état de rmcp est appelée à chaque requête : elle clone un prototype dont le
//!   routeur d'outils est construit une fois (02 §3.6).
//!
//! Les couches s'appliquent dans cet ordre : authentification, puis version, puis débit, puis
//! rmcp (`Host`, `Origin`, taille, protocole). Aucune n'écrit un texte de requête dans un journal
//! ou dans une réponse.

use std::{
    future::poll_fn,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::{Body, HttpBody},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
};
use rmcp::{
    ServerHandler,
    model::{ErrorCode, ErrorData, JsonRpcError, RequestId},
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    stdio::{ServeError, ServeOutcome},
    versions::SUPPORTED,
};

/// Chemin de l'unique endpoint (annexe D §5.2).
const MCP_PATH: &str = "/mcp";

/// Taille maximale d'un corps de requête : 256 Kio (05 §10.1). `fr_verify_citations` accepte au
/// plus 20 000 caractères ; le défaut de rmcp est de 4 Mio.
const MAX_REQUEST_BODY_BYTES: usize = 256 * 1024;

/// Délai laissé aux requêtes en vol quand l'arrêt est demandé, avant de fermer les connexions.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(5);

/// Clé de `params._meta` qui désigne une requête de l'ère moderne (2026-07-28) : rmcp compare
/// alors lui-même la version du `_meta` à l'en-tête et à `supported_protocol_versions()`.
const MODERN_META_KEY: &str = "io.modelcontextprotocol/protocolVersion";

/// Défi renvoyé avec le 401 du profil local (annexe D §6.2).
const CHALLENGE: &str = "Bearer realm=\"mcp-legifrance\"";

/// Hôtes admis par défaut dans l'en-tête `Host` : la boucle locale (défaut de rmcp).
const DEFAULT_ALLOWED_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// Réglages de l'écoute HTTP du profil local : adresse (boucle locale), jeton statique, hôtes et
/// origines admis.
///
/// Les champs sont privés : [`HttpOptions::local`] est le seul constructeur et refuse toute
/// adresse qui n'est pas de la boucle locale. Sans jeton ([`HttpOptions::with_token`]), aucune
/// authentification : ce n'est admis que pour le binaire de test de la suite de conformité ;
/// `lf-app` refuse de démarrer sans jeton.
#[derive(Debug, Clone)]
pub struct HttpOptions {
    /// Adresse d'écoute, de la boucle locale.
    listen: SocketAddr,
    /// Jeton statique du profil local ; `None` : pas d'authentification.
    token: Option<HttpToken>,
    /// Hôtes admis dans `Host` (anti-rebinding DNS).
    allowed_hosts: Vec<String>,
    /// Origines admises dans `Origin` ; liste vide : tout `Origin` présent est refusé (403).
    allowed_origins: Vec<String>,
}

impl HttpOptions {
    /// Options du profil local pour l'adresse `listen`, sans jeton, avec les hôtes de la boucle
    /// locale et aucune origine admise.
    ///
    /// # Errors
    ///
    /// [`HttpOptionsError::NotLoopback`] si l'adresse n'est pas de la boucle locale (`127.0.0.0/8`
    /// ou `::1`) : le profil local n'écoute jamais ailleurs (annexe D §6.2).
    pub fn local(listen: SocketAddr) -> Result<Self, HttpOptionsError> {
        if !listen.ip().is_loopback() {
            return Err(HttpOptionsError::NotLoopback(listen.ip()));
        }
        Ok(Self {
            listen,
            token: None,
            allowed_hosts: DEFAULT_ALLOWED_HOSTS.map(String::from).to_vec(),
            allowed_origins: Vec::new(),
        })
    }

    /// Exige `Authorization: Bearer <token>` sur chaque requête (C-56).
    #[must_use]
    pub fn with_token(mut self, token: HttpToken) -> Self {
        self.token = Some(token);
        self
    }

    /// Remplace les hôtes admis dans `Host`.
    #[must_use]
    pub fn with_allowed_hosts(
        mut self,
        hosts: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_hosts = hosts.into_iter().map(Into::into).collect();
        self
    }

    /// Remplace les origines admises dans `Origin` (schéma obligatoire, `:*` pour tout port, par
    /// exemple `http://localhost:*`). Vide : tout `Origin` présent reçoit 403.
    #[must_use]
    pub fn with_allowed_origins(
        mut self,
        origins: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_origins = origins.into_iter().map(Into::into).collect();
        self
    }

    /// L'adresse d'écoute.
    #[must_use]
    pub fn listen(&self) -> SocketAddr {
        self.listen
    }
}

/// Réglages HTTP refusés.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HttpOptionsError {
    /// Le profil local n'écoute que sur la boucle locale.
    #[error(
        "the local HTTP profile listens on the loopback interface only; {0} is not a loopback address"
    )]
    NotLoopback(IpAddr),
}

/// Jeton statique du profil local (`LF_MCP_HTTP_TOKEN`, annexe D §6.2). Son `Debug` ne montre pas
/// la valeur.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpToken(String);

impl HttpToken {
    /// Jeton de valeur `value`.
    ///
    /// # Errors
    ///
    /// [`HttpTokenError::Empty`] si la valeur est vide, [`HttpTokenError::NotPrintable`] si elle
    /// contient un blanc, un contrôle ou un octet hors ASCII : un en-tête `Authorization` ne peut
    /// pas la porter, elle ne correspondrait jamais.
    pub fn new(value: impl Into<String>) -> Result<Self, HttpTokenError> {
        let value = value.into();
        if value.is_empty() {
            Err(HttpTokenError::Empty)
        } else if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
            Err(HttpTokenError::NotPrintable)
        } else {
            Ok(Self(value))
        }
    }

    /// Nombre d'octets du jeton.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Toujours faux : un jeton n'est jamais vide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Vrai si `presented` est le jeton, comparé en temps constant.
    fn matches(&self, presented: &str) -> bool {
        constant_time_eq(self.0.as_bytes(), presented.as_bytes())
    }
}

impl std::fmt::Debug for HttpToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HttpToken(<redacted>)")
    }
}

/// Jeton HTTP refusé.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HttpTokenError {
    /// Valeur vide.
    #[error("the HTTP token is empty")]
    Empty,
    /// Valeur qui ne tient pas dans un en-tête `Authorization`.
    #[error("the HTTP token must be printable ASCII without spaces")]
    NotPrintable,
}

/// Égalité d'octets dont la durée ne dépend pas de la position du premier octet différent.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    std::hint::black_box(diff) == 0
}

/// Débit des invocations d'outils (C-34) : capacité du seau et durée de recharge d'un jeton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Limits {
    /// Nombre de `tools/call` admis en rafale.
    burst: u32,
    /// Durée pour retrouver un jeton.
    refill_every: Duration,
}

impl Limits {
    /// Valeurs figées en M8, par processus : 30 invocations en rafale, puis 10 par seconde. Le
    /// quota PISTE reste borné en aval par le limiteur de l'adapter (00 §8).
    const PRODUCTION: Self = Self { burst: 30, refill_every: Duration::from_millis(100) };
}

/// Seau à jetons : `try_acquire` prend un jeton ou dit combien attendre.
#[derive(Debug)]
struct Bucket {
    /// Capacité du seau.
    capacity: u32,
    /// Durée pour retrouver un jeton (jamais nulle).
    refill_every: Duration,
    /// Jetons disponibles.
    tokens: u32,
    /// Début de la période de recharge en cours.
    refilled_at: Instant,
}

impl Bucket {
    /// Seau plein à l'instant `now`.
    fn new(limits: Limits, now: Instant) -> Self {
        Self {
            capacity: limits.burst,
            refill_every: limits.refill_every.max(Duration::from_nanos(1)),
            tokens: limits.burst,
            refilled_at: now,
        }
    }

    /// Prend un jeton à l'instant `now`.
    ///
    /// # Errors
    ///
    /// Le temps à attendre avant le prochain jeton, quand le seau est vide.
    fn try_acquire(&mut self, now: Instant) -> Result<(), Duration> {
        let elapsed = now.saturating_duration_since(self.refilled_at);
        let gained =
            u32::try_from(elapsed.as_nanos() / self.refill_every.as_nanos()).unwrap_or(u32::MAX);
        if gained > 0 {
            self.tokens = self.tokens.saturating_add(gained).min(self.capacity);
            // Le reste de la période écoulée compte pour la suivante ; seau plein, on repart de `now`.
            self.refilled_at = if self.tokens == self.capacity {
                now
            } else {
                self.refilled_at + self.refill_every * gained
            };
        }
        if self.tokens == 0 {
            return Err(self
                .refill_every
                .saturating_sub(now.saturating_duration_since(self.refilled_at)));
        }
        self.tokens -= 1;
        Ok(())
    }
}

/// Secondes de `Retry-After` : l'attente arrondie au-dessus, d'une seconde au moins.
fn retry_after_seconds(wait: Duration) -> u64 {
    (wait.as_secs() + u64::from(wait.subsec_nanos() > 0)).max(1)
}

/// Sert `make()` en Streamable HTTP sans session sur `opts.listen()` jusqu'à l'annulation de
/// `shutdown` : l'écoute s'arrête, les requêtes en vol ont 5 s pour finir, puis les connexions
/// restantes sont fermées.
///
/// `make` est appelé à chaque requête (fabrique sans état de rmcp) : il clone un prototype dont le
/// routeur d'outils est construit une fois. Les couches de [`HttpOptions`] précèdent rmcp ; la
/// fermeture d'une connexion annule la requête en vol (C-51, V-41).
///
/// # Errors
///
/// [`ServeError::Http`] si l'adresse ne peut pas être écoutée ou si le service s'interrompt sur une
/// erreur d'entrée-sortie.
pub async fn serve_http<S>(
    make: impl Fn() -> S + Send + Sync + 'static,
    opts: HttpOptions,
    shutdown: CancellationToken,
) -> Result<ServeOutcome, ServeError>
where
    S: ServerHandler + Clone,
{
    let listener = TcpListener::bind(opts.listen).await.map_err(ServeError::Http)?;
    serve_on(listener, make, opts, shutdown, SHUTDOWN_DRAIN).await
}

/// [`serve_http`] sur un écouteur déjà ouvert, avec le délai d'évacuation `drain` (réglable pour
/// les tests).
async fn serve_on<S>(
    listener: TcpListener,
    make: impl Fn() -> S + Send + Sync + 'static,
    opts: HttpOptions,
    shutdown: CancellationToken,
    drain: Duration,
) -> Result<ServeOutcome, ServeError>
where
    S: ServerHandler + Clone,
{
    let app = router(make, &opts, shutdown.child_token(), Limits::PRODUCTION);
    // L'adresse réelle (utile avec le port 0) ; à défaut, celle demandée.
    let addr = listener.local_addr().unwrap_or(opts.listen);
    tracing::info!(%addr, "serving MCP over HTTP");
    let graceful = shutdown.clone();
    let serving = async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move { graceful.cancelled().await })
            .await
    };
    let expired = async {
        shutdown.cancelled().await;
        tokio::time::sleep(drain).await;
    };
    tokio::select! {
        result = serving => finish(result),
        () = expired => {
            tracing::warn!("in-flight HTTP requests did not finish in time; closing");
            Ok(ServeOutcome::Shutdown)
        }
    }
}

/// Issue de `axum::serve` : l'arrêt demandé, ou l'erreur d'entrée-sortie qui l'a interrompu.
fn finish(result: std::io::Result<()>) -> Result<ServeOutcome, ServeError> {
    result.map(|()| ServeOutcome::Shutdown).map_err(ServeError::Http)
}

/// Le routeur axum : `POST /mcp` servi par rmcp, derrière les couches d'authentification, de
/// version et de débit.
fn router<S>(
    make: impl Fn() -> S + Send + Sync + 'static,
    opts: &HttpOptions,
    cancel: CancellationToken,
    limits: Limits,
) -> Router
where
    S: ServerHandler + Clone,
{
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(opts.allowed_hosts.iter().cloned())
        .with_allowed_origins(opts.allowed_origins.iter().cloned())
        .enforce_origin_validation()
        .with_max_request_body_bytes(MAX_REQUEST_BODY_BYTES)
        .with_cancellation_token(cancel);
    // Sans event store (`LocalSessionManager::default()`), GET et DELETE reçoivent 405 (V-40).
    let mcp = StreamableHttpService::new(
        move || Ok(make()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let gate = Gate { limiter: Arc::new(Mutex::new(Bucket::new(limits, Instant::now()))) };
    let app =
        Router::new().route_service(MCP_PATH, mcp).layer(from_fn_with_state(gate, gate_layer));
    match &opts.token {
        Some(token) => app.layer(from_fn_with_state(Arc::new(token.clone()), authenticate)),
        None => app,
    }
}

/// État de la couche de version et de débit.
#[derive(Clone)]
struct Gate {
    /// Seau à jetons des `tools/call`, partagé par toutes les requêtes du processus.
    limiter: Arc<Mutex<Bucket>>,
}

/// Couche d'authentification du profil local (C-56) : 401 sans `Authorization: Bearer <jeton>`.
async fn authenticate(
    State(expected): State<Arc<HttpToken>>,
    request: Request,
    next: Next,
) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer);
    if presented.is_some_and(|token| expected.matches(token)) {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, CHALLENGE)]).into_response()
    }
}

/// Le jeton d'un en-tête `Authorization: Bearer <jeton>` (schéma insensible à la casse).
fn bearer(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("Bearer").then_some(token)
}

/// Couche de version puis de débit (C-48, C-34), devant rmcp. Elle lit le corps d'un POST
/// (borné à 256 Kio) pour reconnaître `initialize`, `tools/call`, l'`id` et le `_meta`, puis le
/// rend intact à rmcp.
async fn gate_layer(State(gate): State<Gate>, request: Request, next: Next) -> Response {
    if request.method() != Method::POST {
        // GET, DELETE et le reste : 405 de rmcp.
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let announced = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if announced.is_some_and(|length| length > MAX_REQUEST_BODY_BYTES) {
        return payload_too_large();
    }
    let bytes = match read_limited(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(BodyRejection::TooLarge) => return payload_too_large(),
        Err(BodyRejection::Unreadable) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let probe = Probe::of(&bytes);
    if let Some(rejection) = check_version(&parts.headers, &probe) {
        return rejection;
    }
    if probe.method.as_deref() == Some("tools/call") {
        let acquired =
            gate.limiter.lock().unwrap_or_else(PoisonError::into_inner).try_acquire(Instant::now());
        if let Err(wait) = acquired {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, retry_after_seconds(wait).to_string())],
            )
                .into_response();
        }
    }
    next.run(Request::from_parts(parts, Body::from(bytes))).await
}

/// 413 : corps au-delà de 256 Kio (C-54).
fn payload_too_large() -> Response {
    StatusCode::PAYLOAD_TOO_LARGE.into_response()
}

/// Pourquoi un corps n'a pas pu être lu.
#[derive(Debug, PartialEq, Eq)]
enum BodyRejection {
    /// Plus de `limit` octets.
    TooLarge,
    /// Lecture interrompue (connexion coupée, corps mal formé).
    Unreadable,
}

/// Lit le corps en s'arrêtant dès qu'il dépasse `limit` octets, avec ou sans `Content-Length`.
async fn read_limited(mut body: Body, limit: usize) -> Result<Vec<u8>, BodyRejection> {
    let mut bytes = Vec::new();
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        // Les trailers ne portent rien pour nous : une trame qui n'est pas de données compte pour
        // zéro octet.
        let data = frame.map_err(|_| BodyRejection::Unreadable)?.into_data().unwrap_or_default();
        if data.len() > limit - bytes.len() {
            return Err(BodyRejection::TooLarge);
        }
        bytes.extend_from_slice(&data);
    }
    Ok(bytes)
}

/// Ce que la couche lit d'un message JSON-RPC : sa méthode, son `id`, et s'il porte un `_meta`
/// moderne. Un corps qui n'est pas un objet JSON n'a rien de tout cela : rmcp le refuse lui-même.
#[derive(Debug, Default, PartialEq)]
struct Probe {
    /// Méthode du message ; absente d'une réponse ou d'une erreur du client.
    method: Option<String>,
    /// Identifiant de la requête, repris dans les erreurs.
    id: Option<RequestId>,
    /// `params._meta` porte la version du protocole (ère moderne).
    modern_meta: bool,
}

impl Probe {
    /// Lit `body` ; un corps illisible donne une sonde vide.
    fn of(body: &[u8]) -> Self {
        let Ok(Value::Object(message)) = serde_json::from_slice::<Value>(body) else {
            return Self::default();
        };
        Self {
            method: message.get("method").and_then(Value::as_str).map(str::to_owned),
            id: message.get("id").and_then(|id| serde_json::from_value(id.clone()).ok()),
            modern_meta: message
                .get("params")
                .and_then(|params| params.get("_meta"))
                .and_then(|meta| meta.get(MODERN_META_KEY))
                .is_some(),
        }
    }
}

/// Couche de version (C-48, annexe D §5.2), pour tout POST autre qu'`initialize`.
///
/// - `MCP-Protocol-Version` absent ou illisible : 400 et `-32020` ;
/// - sans `_meta` moderne, une version hors de [`SUPPORTED`] : 400 et `-32022` ;
/// - une requête à `_meta` moderne est laissée à rmcp (égalité en-tête et `_meta`, version).
///
/// Un corps qui n'est pas un message JSON-RPC (sonde vide) subit le même contrôle d'en-tête : il
/// est refusé ensuite par rmcp. Rend la réponse de rejet, ou `None` si la requête passe.
fn check_version(headers: &HeaderMap, probe: &Probe) -> Option<Response> {
    if probe.method.as_deref() == Some("initialize") {
        return None;
    }
    let Some(value) = headers.get("mcp-protocol-version") else {
        return Some(jsonrpc_rejection(
            probe.id.clone(),
            ErrorData::header_mismatch("MCP-Protocol-Version header is required", None),
        ));
    };
    let Ok(requested) = value.to_str() else {
        return Some(jsonrpc_rejection(
            probe.id.clone(),
            ErrorData::header_mismatch("MCP-Protocol-Version header is not valid text", None),
        ));
    };
    if probe.modern_meta || SUPPORTED.iter().any(|version| version.as_str() == requested) {
        return None;
    }
    Some(jsonrpc_rejection(
        probe.id.clone(),
        ErrorData::new(
            ErrorCode::UNSUPPORTED_PROTOCOL_VERSION,
            "Unsupported protocol version",
            Some(json!({ "supported": SUPPORTED, "requested": requested })),
        ),
    ))
}

/// Réponse 400 portant une erreur JSON-RPC.
fn jsonrpc_rejection(id: Option<RequestId>, error: ErrorData) -> Response {
    let body = serde_json::to_vec(&JsonRpcError::new(id, error)).unwrap_or_default();
    (StatusCode::BAD_REQUEST, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

#[cfg(test)]
mod tests {
    // Le harnais en processus n'existe qu'avec la feature `test-support` (`--all-features`).
    #[cfg(feature = "test-support")]
    mod served {
        use axum::body::to_bytes;
        use pretty_assertions::assert_eq;
        use tower::ServiceExt as _;

        use super::super::*;
        use crate::testing::{Fixture, FixtureServer};

        /// Jeton de test : 40 caractères imprimables.
        const TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";

        /// Corps d'un `initialize` legacy.
        const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;

        fn loopback() -> SocketAddr {
            SocketAddr::from(([127, 0, 0, 1], 0))
        }

        fn options() -> HttpOptions {
            HttpOptions::local(loopback()).expect("a loopback address")
        }

        fn with_token(options: HttpOptions) -> HttpOptions {
            options.with_token(HttpToken::new(TOKEN).expect("a valid token"))
        }

        /// Un limiteur large : les tests de débit réglent le leur.
        const ROOMY: Limits = Limits { burst: 1_000, refill_every: Duration::from_millis(1) };

        /// La fabrique de rmcp : clone d'un serveur de test.
        fn maker() -> impl Fn() -> FixtureServer + Send + Sync + 'static {
            let proto = Fixture::default().server();
            move || proto.clone()
        }

        fn app(options: &HttpOptions, limits: Limits) -> Router {
            router(maker(), options, CancellationToken::new(), limits)
        }

        /// Un POST de `body` avec les en-têtes d'un client legacy (`2025-11-25`) de la boucle locale.
        fn post() -> axum::http::request::Builder {
            Request::builder()
                .method(Method::POST)
                .uri(MCP_PATH)
                .header(header::HOST, "127.0.0.1:8765")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::CONTENT_TYPE, "application/json")
                .header("mcp-protocol-version", "2025-11-25")
        }

        async fn send(app: &Router, request: Request) -> (StatusCode, HeaderMap, Vec<u8>) {
            let response = app.clone().oneshot(request).await.expect("an infallible service");
            let (parts, body) = response.into_parts();
            let bytes = to_bytes(body, usize::MAX).await.expect("a readable body");
            (parts.status, parts.headers, bytes.to_vec())
        }

        async fn post_json(app: &Router, body: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
            send(app, post().body(Body::from(body.to_owned())).expect("a request")).await
        }

        fn json_of(bytes: &[u8]) -> Value {
            serde_json::from_slice(bytes).expect("a JSON body")
        }

        // ---- C-40 : un seul endpoint, GET et DELETE → 405 ---------------------------------------

        #[tokio::test]
        async fn get_and_delete_are_405() {
            let app = app(&with_token(options()), ROOMY);
            for method in [Method::GET, Method::DELETE, Method::PUT, Method::HEAD] {
                let request = Request::builder()
                    .method(method.clone())
                    .uri(MCP_PATH)
                    .header(header::HOST, "127.0.0.1:8765")
                    .header(header::ACCEPT, "text/event-stream")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .expect("a request");
                let (status, headers, _) = send(&app, request).await;
                assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
                assert_eq!(headers.get(header::ALLOW).expect("Allow"), "POST", "{method}");
                // Aucune session n'est jamais annoncée (C-41).
                assert!(!headers.contains_key("mcp-session-id"), "{method}");
            }
        }

        // ---- C-43 : Host hors liste → rejet ------------------------------------------------------

        #[tokio::test]
        async fn allowed_hosts_enforced() {
            let app = app(&options(), ROOMY);
            let with_host = |host: &str| {
                let mut request = post().body(Body::from(INITIALIZE)).expect("a request");
                request.headers_mut().insert(header::HOST, host.parse().expect("a header value"));
                request
            };
            for host in ["localhost", "127.0.0.1:9000", "[::1]:9000"] {
                let (status, ..) = send(&app, with_host(host)).await;
                assert_eq!(status, StatusCode::OK, "{host}");
            }
            for host in ["evil.example", "evil.example:8765", "127.0.0.1.evil.example"] {
                let (status, ..) = send(&app, with_host(host)).await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{host}");
            }
            // Les hôtes admis se remplacent.
            let custom = self::app(&options().with_allowed_hosts(["mcp.test"]), ROOMY);
            let (status, ..) = send(&custom, with_host("mcp.test")).await;
            assert_eq!(status, StatusCode::OK);
            let (status, ..) = send(&custom, with_host("localhost")).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
        }

        // ---- C-48 : couche de version ------------------------------------------------------------

        #[tokio::test]
        async fn http_missing_version_header_rejected() {
            let app = app(&options(), ROOMY);
            let body = r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#;
            let mut request = post().body(Body::from(body)).expect("a request");
            request.headers_mut().remove("mcp-protocol-version");
            let (status, headers, bytes) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(
                headers.get(header::CONTENT_TYPE).expect("a content type"),
                "application/json"
            );
            let error = json_of(&bytes);
            assert_eq!(error["jsonrpc"], "2.0");
            assert_eq!(error["id"], 7);
            assert_eq!(error["error"]["code"], -32020);

            // Une notification est un POST comme un autre : sans en-tête, même refus, sans `id`.
            let notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
            let mut request = post().body(Body::from(notification)).expect("a request");
            request.headers_mut().remove("mcp-protocol-version");
            let (status, _, bytes) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(json_of(&bytes)["error"]["code"], -32020);
            assert!(json_of(&bytes).get("id").is_none());

            // Un corps qui n'est pas JSON, sans en-tête : le même refus, avant rmcp.
            let mut request = post().body(Body::from("not json")).expect("a request");
            request.headers_mut().remove("mcp-protocol-version");
            let (status, _, bytes) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(json_of(&bytes)["error"]["code"], -32020);

            // `initialize` est exempté : la poignée de main legacy peut omettre l'en-tête (C-52).
            let mut request = post().body(Body::from(INITIALIZE)).expect("a request");
            request.headers_mut().remove("mcp-protocol-version");
            let (status, ..) = send(&app, request).await;
            assert_eq!(status, StatusCode::OK);
        }

        #[tokio::test]
        async fn http_unsupported_known_version_header_rejected() {
            let app = app(&options(), ROOMY);
            let body = r#"{"jsonrpc":"2.0","id":"abc","method":"tools/list"}"#;
            for requested in ["2025-03-26", "2024-11-05", "1999-01-01", "not-a-version", ""] {
                let mut request = post().body(Body::from(body)).expect("a request");
                request
                    .headers_mut()
                    .insert("mcp-protocol-version", requested.parse().expect("value"));
                let (status, _, bytes) = send(&app, request).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{requested}");
                let error = json_of(&bytes);
                assert_eq!(error["id"], "abc", "{requested}");
                assert_eq!(error["error"]["code"], -32022, "{requested}");
                assert_eq!(
                    error["error"]["data"],
                    json!({
                        "supported": ["2026-07-28", "2025-11-25", "2025-06-18"],
                        "requested": requested,
                    }),
                    "{requested}"
                );
            }
            // Les versions servies passent la couche (rmcp répond ensuite).
            for served in ["2025-11-25", "2025-06-18"] {
                let mut request = post().body(Body::from(body)).expect("a request");
                request
                    .headers_mut()
                    .insert("mcp-protocol-version", served.parse().expect("value"));
                let (status, ..) = send(&app, request).await;
                assert_eq!(status, StatusCode::OK, "{served}");
            }
            // Un en-tête qui n'est pas du texte : refusé comme absent.
            let mut request = post().body(Body::from(body)).expect("a request");
            request.headers_mut().insert(
                "mcp-protocol-version",
                axum::http::HeaderValue::from_bytes(&[0xff, 0xfe]).expect("opaque bytes"),
            );
            let (status, _, bytes) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(json_of(&bytes)["error"]["code"], -32020);
        }

        /// Une requête à `_meta` moderne est laissée à rmcp, même avec une version que la couche
        /// refuserait : rmcp compare lui-même l'en-tête et le `_meta`.
        #[tokio::test]
        async fn a_modern_meta_request_is_left_to_rmcp() {
            let app = app(&options(), ROOMY);
            let body = json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/list",
                "params": { "_meta": { MODERN_META_KEY: "2026-07-28" } }
            })
            .to_string();
            let mut request = post().body(Body::from(body.clone())).expect("a request");
            request
                .headers_mut()
                .insert("mcp-protocol-version", "2025-03-26".parse().expect("value"));
            let (status, _, bytes) = send(&app, request).await;
            // rmcp refuse la différence entre l'en-tête et le `_meta` : -32020 de rmcp, pas -32022 de la couche.
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(json_of(&bytes)["error"]["code"], -32020);
            assert!(!String::from_utf8_lossy(&bytes).contains("Unsupported protocol version"));
        }

        // ---- C-42 : Origin validé par nous -------------------------------------------------------

        #[tokio::test]
        async fn http_origin_forbidden() {
            let app = app(&options().with_allowed_origins(["http://localhost:*"]), ROOMY);
            let with_origin = |origin: &str| {
                let mut request = post().body(Body::from(INITIALIZE)).expect("a request");
                request
                    .headers_mut()
                    .insert(header::ORIGIN, origin.parse().expect("a header value"));
                request
            };
            for origin in ["https://evil.example", "http://localhost.evil.example:3000", "null"] {
                let (status, ..) = send(&app, with_origin(origin)).await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{origin}");
            }
            let (status, ..) = send(&app, with_origin("http://localhost:3000")).await;
            assert_eq!(status, StatusCode::OK);

            // Liste vide : la validation reste active, tout `Origin` présent est refusé.
            let strict = self::app(&options(), ROOMY);
            let (status, ..) = send(&strict, with_origin("http://localhost:3000")).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            // Sans `Origin` (client non navigateur), la requête passe.
            let (status, ..) = post_json(&strict, INITIALIZE).await;
            assert_eq!(status, StatusCode::OK);
        }

        // ---- C-54 : corps borné ------------------------------------------------------------------

        #[tokio::test]
        async fn http_body_too_large_413() {
            let app = app(&options(), ROOMY);
            // Juste sous le plafond : accepté par la couche (rmcp refuse ensuite le JSON).
            let pad = "a".repeat(MAX_REQUEST_BODY_BYTES - 64);
            let fits = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","pad":"{pad}"}}"#);
            assert!(fits.len() <= MAX_REQUEST_BODY_BYTES);
            let (status, ..) = post_json(&app, &fits).await;
            assert_ne!(status, StatusCode::PAYLOAD_TOO_LARGE);

            // Au-delà, avec `Content-Length` : refusé avant toute lecture.
            let big = "a".repeat(MAX_REQUEST_BODY_BYTES + 1);
            let request = post()
                .header(header::CONTENT_LENGTH, big.len())
                .body(Body::from(big.clone()))
                .expect("a request");
            let (status, ..) = send(&app, request).await;
            assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

            // Sans `Content-Length` annoncé, la lecture s'arrête au plafond.
            let (status, ..) = post_json(&app, &big).await;
            assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

            // Sans `Content-Length` (flux), le plafond tient aussi.
            let chunks = futures::stream::iter(
                (0..5).map(|_| Ok::<_, std::io::Error>(vec![b'a'; MAX_REQUEST_BODY_BYTES / 4])),
            );
            let request = post().body(Body::from_stream(chunks)).expect("a request");
            let (status, ..) = send(&app, request).await;
            assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        }

        #[tokio::test]
        async fn an_unreadable_body_is_a_bad_request() {
            let app = app(&options(), ROOMY);
            let chunks = futures::stream::iter(vec![
                Ok(vec![b'{']),
                Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "cut")),
            ]);
            let request = post().body(Body::from_stream(chunks)).expect("a request");
            let (status, ..) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }

        // ---- C-56 : jeton statique ---------------------------------------------------------------

        #[tokio::test]
        async fn http_local_token_required() {
            let app = app(&with_token(options()), ROOMY);
            let with_auth = |value: Option<&str>| {
                let mut request = post().body(Body::from(INITIALIZE)).expect("a request");
                if let Some(value) = value {
                    request
                        .headers_mut()
                        .insert(header::AUTHORIZATION, value.parse().expect("value"));
                }
                request
            };
            let wrong = format!("Bearer {TOKEN}x");
            let short = format!("Bearer {}", &TOKEN[..TOKEN.len() - 1]);
            let basic = format!("Basic {TOKEN}");
            let bare = TOKEN.to_owned();
            for presented in [
                None,
                Some(wrong.as_str()),
                Some(short.as_str()),
                Some("Bearer "),
                Some(basic.as_str()),
                Some(bare.as_str()),
            ] {
                let (status, headers, bytes) = send(&app, with_auth(presented)).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{presented:?}");
                assert_eq!(
                    headers.get(header::WWW_AUTHENTICATE).expect("a challenge"),
                    "Bearer realm=\"mcp-legifrance\"",
                    "{presented:?}"
                );
                // Sans corps détaillé, et sans écho de la valeur reçue.
                assert_eq!(bytes, Vec::<u8>::new(), "{presented:?}");
            }
            for scheme in ["Bearer", "bearer", "BEARER"] {
                let value = format!("{scheme} {TOKEN}");
                let (status, ..) = send(&app, with_auth(Some(&value))).await;
                assert_eq!(status, StatusCode::OK, "{scheme}");
            }
            // Une requête non authentifiée n'atteint ni la couche de version ni rmcp : le 401 vient
            // avant le 400 d'un en-tête de version absent.
            let mut request = with_auth(None);
            request.headers_mut().remove("mcp-protocol-version");
            let (status, ..) = send(&app, request).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }

        // ---- C-34 : débit des invocations --------------------------------------------------------

        #[tokio::test]
        async fn http_tool_invocations_are_rate_limited() {
            // Deux jetons, rechargés en une heure : la rafale est déterministe.
            let limits = Limits { burst: 2, refill_every: Duration::from_secs(3_600) };
            let app = app(&options(), limits);
            let call = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"fr_coverage","arguments":{}}}"#;
            for _ in 0..2 {
                let (status, ..) = post_json(&app, call).await;
                assert_eq!(status, StatusCode::OK);
            }
            let (status, headers, _) = post_json(&app, call).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
            let retry: u64 = headers
                .get(header::RETRY_AFTER)
                .expect("Retry-After")
                .to_str()
                .expect("text")
                .parse()
                .expect("seconds");
            assert!((3_599..=3_600).contains(&retry), "{retry}");
            // Seules les invocations d'outils sont comptées : `tools/list` et `initialize` passent.
            let list = r#"{"jsonrpc":"2.0","id":10,"method":"tools/list"}"#;
            let (status, ..) = post_json(&app, list).await;
            assert_eq!(status, StatusCode::OK);
            let (status, ..) = post_json(&app, INITIALIZE).await;
            assert_eq!(status, StatusCode::OK);
            // Le seau est vide, mais la couche de version passe avant le débit : 400, pas 429.
            let mut request = post().body(Body::from(call)).expect("a request");
            request.headers_mut().remove("mcp-protocol-version");
            let (status, ..) = send(&app, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }

        #[test]
        fn the_bucket_refills_one_token_per_period_up_to_its_capacity() {
            let start = Instant::now();
            let step = Duration::from_secs(1);
            let mut bucket = Bucket::new(Limits { burst: 3, refill_every: step }, start);
            for _ in 0..3 {
                assert_eq!(bucket.try_acquire(start), Ok(()));
            }
            // Vide : il faut attendre une période entière, puis ce qui reste de la période en cours.
            assert_eq!(bucket.try_acquire(start), Err(step));
            assert_eq!(bucket.try_acquire(start + step / 4), Err(step * 3 / 4));
            // Une période écoulée : un jeton, pas deux.
            assert_eq!(bucket.try_acquire(start + step), Ok(()));
            assert_eq!(bucket.try_acquire(start + step), Err(step));
            // Le reste d'une période compte pour la suivante (1,5 période, puis 2 périodes).
            assert_eq!(bucket.try_acquire(start + step * 3 / 2 + step), Ok(()));
            assert_eq!(bucket.try_acquire(start + step * 3), Ok(()));
            // Après une longue pause, le seau est plein mais pas au-delà de sa capacité.
            let later = start + step * 100;
            for _ in 0..3 {
                assert_eq!(bucket.try_acquire(later), Ok(()));
            }
            assert_eq!(bucket.try_acquire(later), Err(step));
        }

        #[test]
        fn a_zero_refill_period_is_clamped_instead_of_dividing_by_zero() {
            let start = Instant::now();
            let mut bucket = Bucket::new(Limits { burst: 1, refill_every: Duration::ZERO }, start);
            assert_eq!(bucket.try_acquire(start), Ok(()));
            assert_eq!(bucket.try_acquire(start + Duration::from_secs(1)), Ok(()));
        }

        #[test]
        fn retry_after_rounds_up_to_whole_seconds_and_is_never_zero() {
            for (wait, seconds) in [
                (Duration::ZERO, 1),
                (Duration::from_millis(1), 1),
                (Duration::from_millis(999), 1),
                (Duration::from_secs(1), 1),
                (Duration::from_millis(1_001), 2),
                (Duration::from_secs(3_600), 3_600),
            ] {
                assert_eq!(retry_after_seconds(wait), seconds, "{wait:?}");
            }
        }

        // ---- C-41, C-49, C-50, C-52 : le service rmcp derrière les couches ----------------------

        #[tokio::test]
        async fn http_legacy_initialize_then_call_without_a_session() {
            let app = app(&options(), ROOMY);
            let (status, headers, bytes) = post_json(&app, INITIALIZE).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                headers.get(header::CONTENT_TYPE).expect("a content type"),
                "application/json"
            );
            assert!(!headers.contains_key("mcp-session-id"));
            let reply = json_of(&bytes);
            assert_eq!(reply["result"]["serverInfo"]["name"], "mcp-legifrance");

            // Chaque requête se suffit : pas de session, un `tools/list` legacy répond directement.
            let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
            let (status, headers, bytes) = post_json(&app, list).await;
            assert_eq!(status, StatusCode::OK);
            assert!(!headers.contains_key("mcp-session-id"));
            assert!(
                json_of(&bytes)["result"]["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
            );
        }

        #[tokio::test]
        async fn http_notification_202() {
            let app = app(&options(), ROOMY);
            let notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
            let (status, _, bytes) = post_json(&app, notification).await;
            assert_eq!(status, StatusCode::ACCEPTED);
            assert_eq!(bytes, Vec::<u8>::new());
        }

        // ---- options, jeton, constructeurs -------------------------------------------------------

        #[test]
        fn the_local_profile_listens_on_loopback_only() {
            for good in ["127.0.0.1:0", "127.0.0.1:8765", "127.1.2.3:80", "[::1]:8765"] {
                let listen: SocketAddr = good.parse().expect("an address");
                assert_eq!(HttpOptions::local(listen).expect(good).listen(), listen);
            }
            for bad in [
                "0.0.0.0:8765",
                "192.168.1.10:8765",
                "10.0.0.1:80",
                "[::]:8765",
                "[::ffff:127.0.0.1]:80",
            ] {
                let listen: SocketAddr = bad.parse().expect("an address");
                assert_eq!(
                    HttpOptions::local(listen).unwrap_err(),
                    HttpOptionsError::NotLoopback(listen.ip()),
                    "{bad}"
                );
            }
            let error = HttpOptions::local("0.0.0.0:1".parse().expect("an address")).unwrap_err();
            assert!(error.to_string().contains("loopback"), "{error}");
        }

        #[test]
        fn a_token_is_non_empty_printable_ascii_and_never_shown() {
            assert_eq!(HttpToken::new(""), Err(HttpTokenError::Empty));
            for bad in ["has space", "tab\there", "new\nline", "accentué", "\u{7f}"] {
                assert_eq!(HttpToken::new(bad), Err(HttpTokenError::NotPrintable), "{bad:?}");
            }
            let token = HttpToken::new(TOKEN).expect("a valid token");
            assert_eq!(token.len(), TOKEN.len());
            assert!(!token.is_empty());
            assert_eq!(format!("{token:?}"), "HttpToken(<redacted>)");
            let options = with_token(options());
            assert!(!format!("{options:?}").contains(TOKEN));
            assert!(HttpTokenError::Empty.to_string().contains("empty"));
            assert!(HttpTokenError::NotPrintable.to_string().contains("printable"));
        }

        #[test]
        fn the_constant_time_comparison_is_an_equality() {
            let token = HttpToken::new(TOKEN).expect("a valid token");
            assert!(token.matches(TOKEN));
            assert!(!token.matches(""));
            assert!(!token.matches(&TOKEN[..TOKEN.len() - 1]));
            assert!(!token.matches(&format!("{TOKEN}0")));
            let mut flipped = TOKEN.as_bytes().to_vec();
            flipped[0] ^= 1;
            assert!(!constant_time_eq(TOKEN.as_bytes(), &flipped));
            assert!(constant_time_eq(b"", b""));
        }

        #[test]
        fn a_bearer_credential_is_the_token_after_a_case_insensitive_scheme() {
            assert_eq!(bearer("Bearer abc"), Some("abc"));
            assert_eq!(bearer("bearer abc"), Some("abc"));
            assert_eq!(bearer("Bearer "), Some(""));
            assert_eq!(bearer("Basic abc"), None);
            assert_eq!(bearer("abc"), None);
            assert_eq!(bearer(""), None);
        }

        #[test]
        fn the_probe_reads_method_id_and_modern_meta_from_an_object_only() {
            let probe = Probe::of(br#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#);
            assert_eq!(probe.method.as_deref(), Some("tools/call"));
            assert_eq!(probe.id, Some(RequestId::Number(5)));
            assert!(probe.modern_meta);
            let probe = Probe::of(br#"{"id":"x","method":"ping","params":{"_meta":{}}}"#);
            assert_eq!(probe.id, Some(RequestId::String("x".into())));
            assert!(!probe.modern_meta);
            // Un `id` qui n'est ni nombre ni chaîne, un corps non objet ou illisible : sonde vide.
            assert_eq!(Probe::of(br#"{"id":{"a":1},"method":3}"#), Probe::default());
            for body in [&b"[1,2]"[..], b"null", b"", b"{", b"\xff\xfe"] {
                assert_eq!(Probe::of(body), Probe::default(), "{body:?}");
            }
        }

        // ---- le serveur sur une vraie socket ----------------------------------------------------

        /// Requête brute d'un client minimal, réponse lue jusqu'à la fermeture.
        async fn raw_exchange(addr: SocketAddr, request: &str) -> String {
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
            let mut stream = tokio::net::TcpStream::connect(addr).await.expect("a connection");
            stream.write_all(request.as_bytes()).await.expect("a write");
            let mut reply = String::new();
            stream.read_to_string(&mut reply).await.expect("a read");
            reply
        }

        fn get_request(authorization: &str) -> String {
            format!(
                "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n{authorization}Connection: close\r\n\r\n"
            )
        }

        #[tokio::test]
        async fn serve_http_answers_over_a_socket_and_stops_on_shutdown() {
            let options = with_token(options());
            let listener = TcpListener::bind(loopback()).await.expect("an ephemeral port");
            let addr = listener.local_addr().expect("an address");
            let shutdown = CancellationToken::new();
            let server = tokio::spawn(serve_on(
                listener,
                maker(),
                options,
                shutdown.clone(),
                Duration::from_secs(5),
            ));

            let reply = raw_exchange(addr, &get_request("")).await;
            assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
            let reply =
                raw_exchange(addr, &get_request(&format!("Authorization: Bearer {TOKEN}\r\n")))
                    .await;
            assert!(reply.starts_with("HTTP/1.1 405"), "{reply}");
            assert!(reply.to_ascii_lowercase().contains("allow: post"), "{reply}");

            shutdown.cancel();
            let outcome = tokio::time::timeout(Duration::from_secs(5), server)
                .await
                .expect("the server stops")
                .expect("no panic");
            assert_eq!(outcome.expect("a clean stop"), ServeOutcome::Shutdown);
        }

        /// Une connexion qui ne finit jamais sa requête ne retient pas l'arrêt au-delà du délai.
        #[tokio::test]
        async fn shutdown_closes_connections_that_do_not_drain() {
            use tokio::io::AsyncWriteExt as _;
            let options = options();
            let listener = TcpListener::bind(loopback()).await.expect("an ephemeral port");
            let addr = listener.local_addr().expect("an address");
            let shutdown = CancellationToken::new();
            let server = tokio::spawn(serve_on(
                listener,
                maker(),
                options,
                shutdown.clone(),
                Duration::from_millis(100),
            ));
            // En-têtes jamais terminés : la requête reste en cours de lecture.
            let mut stream = tokio::net::TcpStream::connect(addr).await.expect("a connection");
            stream.write_all(b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\n").await.expect("a write");
            tokio::time::sleep(Duration::from_millis(50)).await;

            shutdown.cancel();
            let outcome = tokio::time::timeout(Duration::from_secs(5), server)
                .await
                .expect("the server stops within the drain delay")
                .expect("no panic");
            assert_eq!(outcome.expect("a clean stop"), ServeOutcome::Shutdown);
        }

        #[tokio::test]
        async fn serve_http_reports_an_address_it_cannot_listen_on() {
            let taken = std::net::TcpListener::bind(loopback()).expect("an ephemeral port");
            let options =
                HttpOptions::local(taken.local_addr().expect("an address")).expect("loopback");
            let outcome = serve_http(maker(), options, CancellationToken::new()).await;
            let error = outcome.expect_err("the port is taken");
            assert!(error.to_string().starts_with("MCP HTTP transport failed"), "{error}");
        }

        /// `serve_http` lie l'adresse puis sert ; un arrêt déjà demandé le fait sortir aussitôt.
        #[tokio::test]
        async fn serve_http_binds_then_stops_on_a_shutdown_already_requested() {
            let shutdown = CancellationToken::new();
            shutdown.cancel();
            let outcome = tokio::time::timeout(
                Duration::from_secs(5),
                serve_http(maker(), options(), shutdown),
            )
            .await
            .expect("the server stops");
            assert_eq!(outcome.expect("a clean stop"), ServeOutcome::Shutdown);
        }

        /// L'issue d'`axum::serve` : arrêt propre, ou erreur d'entrée-sortie du transport.
        #[test]
        fn finish_maps_the_serve_result() {
            assert_eq!(finish(Ok(())).expect("a clean stop"), ServeOutcome::Shutdown);
            let failed = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe");
            let error = finish(Err(failed)).expect_err("a transport failure");
            assert_eq!(error.to_string(), "MCP HTTP transport failed: pipe");
        }
    }
}
