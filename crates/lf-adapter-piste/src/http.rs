//! Client HTTP unique de l'adapter, tentative d'envoi et lecture bornée du corps
//! (04 §4.1 à §4.3, §13.1, §14.2 à §14.4, T-M2-03).
//!
//! Le module ne décide d'aucun réessai ni d'aucun statut : `send_once` fait une seule
//! tentative et rend le statut, deux en-têtes et le corps tels quels ; la couche de réessai
//! et le jugement des statuts sont ailleurs. Aucun message d'erreur ne contient d'URL
//! complète ni d'en-tête : seul le gabarit de chemin (`Attempt::endpoint`) y figure.
//!
//! Un seul [`Client`] sert tous les appels (`SharedClient`) : il porte le réservoir de
//! connexions et la configuration TLS, coûteux à recréer. `reqwest` ne suit aucune
//! redirection, ne réessaie rien de lui-même et n'accepte jamais un certificat invalide.

use std::error::Error;
use std::fmt;
use std::time::Duration;

use lf_application::ports::CorpusError;
use reqwest::header::{
    ACCEPT, CONTENT_TYPE, DATE, HeaderMap, HeaderName, HeaderValue, RETRY_AFTER,
};
use reqwest::{Client, ClientBuilder, RequestBuilder, Response, Url, redirect};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::OnceCell;

use crate::config::{PisteConfig, PisteEndpoints};

/// Mots qui, en minuscules, signalent une panne TLS dans la chaîne d'une erreur (second
/// critère : `classify_parts` exige d'abord que la panne survienne à la connexion).
///
/// Limite : reconnaissance par le texte, donc fragile (`rustls` n'est pas une dépendance
/// directe, ses types ne sont pas nommables). Seule une réponse en clair est testée ici ;
/// les vrais certificats refusés relèvent du spike TLS.
const TLS_WORDS: [&str; 5] = ["tls", "certificate", "handshake", "alert", "corrupt message"];

/// Corps d'une requête déjà prêt à envoyer.
///
/// `Debug` rédigé à la main : il n'affiche ni valeurs de formulaire (`client_secret`) ni octets
/// de JSON, seulement les noms de champs ou la longueur.
#[derive(Clone, Copy)]
pub(crate) enum Payload<'a> {
    /// JSON déjà sérialisé (la sérialisation des requêtes se teste à part, 04 §6.10).
    Json(&'a [u8]),
    /// Paires d'un formulaire `application/x-www-form-urlencoded`.
    Form(&'a [(&'a str, &'a str)]),
}

/// Une tentative d'envoi : tout ce que `send_once` doit savoir.
///
/// `Debug` rédigé à la main : ni URL complète, ni valeur de formulaire, ni jeton ; le gabarit
/// d'endpoint, la forme du corps, la présence d'un jeton et les bornes seulement.
#[derive(Clone, Copy)]
pub(crate) struct Attempt<'a> {
    /// URL complète de la requête (jamais écrite dans un message d'erreur).
    pub url: &'a Url,
    /// Gabarit de chemin (`/consult/juri`) : seule trace de la route dans les erreurs.
    pub endpoint: &'a str,
    /// Corps de la requête.
    pub payload: Payload<'a>,
    /// Jeton porteur, posé dans un en-tête sensible.
    pub bearer: Option<&'a SecretString>,
    /// Délai de cette tentative, calculé par l'appelant ; il remplace celui du client.
    pub timeout: Duration,
    /// Taille maximale du corps de la réponse, après décompression.
    pub max_body_bytes: u64,
}

/// Réponse brute : statut, deux en-têtes utiles et corps, sans interprétation.
///
/// `Debug` rédigé à la main : le corps (qui porte `access_token` pour la réponse OAuth)
/// n'est jamais affiché, seulement sa longueur.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RawResponse {
    /// Code de statut HTTP.
    pub status: u16,
    /// `Retry-After`, s'il est présent et en ASCII.
    pub retry_after: Option<String>,
    /// `Date`, s'il est présent et en ASCII.
    pub date: Option<String>,
    /// Corps, déjà décompressé, d'au plus `Attempt::max_body_bytes` octets.
    pub body: Vec<u8>,
}

impl fmt::Debug for Payload<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(bytes) => f.debug_struct("Json").field("bytes", &bytes.len()).finish(),
            Self::Form(pairs) => f
                .debug_struct("Form")
                .field("fields", &pairs.iter().map(|(name, _)| *name).collect::<Vec<_>>())
                .finish(),
        }
    }
}

impl fmt::Debug for Attempt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attempt")
            .field("endpoint", &self.endpoint)
            .field("payload", &self.payload)
            .field("bearer", &self.bearer.is_some())
            .field("timeout", &self.timeout)
            .field("max_body_bytes", &self.max_body_bytes)
            .finish()
    }
}

impl fmt::Debug for RawResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawResponse")
            .field("status", &self.status)
            .field("retry_after", &self.retry_after)
            .field("date", &self.date)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Échec d'une tentative, avant tout jugement de la couche de réessai.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransportError {
    /// Délai de la tentative dépassé.
    Timeout,
    /// Panne réseau (connexion, requête, corps interrompu ou gzip corrompu).
    Network {
        /// Libellé fixe et gabarit de chemin, jamais d'URL.
        detail: String,
    },
    /// Échec TLS (poignée de main, certificat).
    Tls {
        /// Libellé fixe et gabarit de chemin.
        detail: String,
    },
    /// Corps plus gros que la borne.
    TooLarge {
        /// Borne et gabarit de chemin.
        detail: String,
    },
}

impl From<TransportError> for CorpusError {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Timeout => Self::Timeout,
            TransportError::Network { detail } => {
                Self::Upstream { status: None, retryable: true, detail }
            }
            TransportError::Tls { detail } => {
                Self::Upstream { status: None, retryable: false, detail }
            }
            TransportError::TooLarge { detail } => Self::Decode { detail },
        }
    }
}

/// Construit le client HTTP de l'adapter (04 §4.1).
///
/// # Errors
///
/// [`CorpusError::Config`] si `reqwest` ne peut pas construire le client (en-tête
/// `User-Agent` invalide, TLS inutilisable) ; le détail ne cite aucune URL.
pub(crate) fn build_client(
    config: &PisteConfig,
    endpoints: &PisteEndpoints,
) -> Result<Client, CorpusError> {
    let mut defaults = HeaderMap::new();
    defaults.insert(ACCEPT, HeaderValue::from_static("application/json"));
    finish_client(
        Client::builder()
            .connect_timeout(config.tuning().connect_timeout)
            // Simple filet : le délai effectif est toujours `Attempt::timeout`.
            .timeout(config.tuning().request_timeout)
            .user_agent(config.user_agent())
            .default_headers(defaults)
            .gzip(true)
            .redirect(redirect::Policy::none())
            .https_only(!allows_plain_http(endpoints)),
    )
}

/// Vrai si l'une des deux URL est en `http` (la configuration ne l'admet que sur la
/// boucle locale).
fn allows_plain_http(endpoints: &PisteEndpoints) -> bool {
    endpoints.oauth_url().scheme() == "http" || endpoints.base_url().scheme() == "http"
}

/// Termine la construction ; l'échec devient une erreur de configuration sans URL.
fn finish_client(builder: ClientBuilder) -> Result<Client, CorpusError> {
    builder.build().map_err(|error| CorpusError::Config {
        detail: format!("could not build the HTTP client: {}", describe(&error.without_url())),
    })
}

/// Maillons d'une erreur, de l'effet à la cause.
fn chain<'a>(error: &'a (dyn Error + 'static)) -> impl Iterator<Item = &'a (dyn Error + 'static)> {
    std::iter::successors(Some(error), |link| (*link).source())
}

/// Texte d'une erreur et de ses causes, joints par `: ` (le `Display` de `reqwest::Error`
/// n'imprime pas ses causes).
fn describe(error: &(dyn Error + 'static)) -> String {
    chain(error).map(ToString::to_string).collect::<Vec<_>>().join(": ")
}

/// Vrai si un maillon de la chaîne contient un mot de [`TLS_WORDS`], sans tenir compte de
/// la casse.
fn is_tls_failure(error: &(dyn Error + 'static)) -> bool {
    chain(error).any(|link| {
        let text = link.to_string().to_lowercase();
        TLS_WORDS.iter().any(|word| text.contains(word))
    })
}

/// Client unique, construit au premier appel (`PisteCorpus::new` ne fait aucune E/S).
#[derive(Debug)]
pub(crate) struct SharedClient {
    /// Le client, une fois construit.
    cell: OnceCell<Client>,
}

impl SharedClient {
    /// Cellule vide.
    pub(crate) fn new() -> Self {
        Self { cell: OnceCell::new() }
    }

    /// Vrai une fois le client construit (les tests de `PisteCorpus` : `new` ne construit rien).
    #[cfg(test)]
    pub(crate) fn is_built(&self) -> bool {
        self.cell.initialized()
    }

    /// Rend le client, en le construisant avec `build` si besoin. Un échec n'est pas
    /// mémorisé : l'appel suivant reconstruit.
    ///
    /// # Errors
    ///
    /// L'erreur de `build`.
    pub(crate) async fn get_or_build(
        &self,
        build: impl FnOnce() -> Result<Client, CorpusError>,
    ) -> Result<&Client, CorpusError> {
        self.cell.get_or_try_init(|| async { build() }).await
    }

    /// Rend le client construit d'après `config`.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Config`] si les adresses ont été refusées (mode dégradé, 04 §2.4) ou
    /// si le client ne se construit pas.
    pub(crate) async fn get(&self, config: &PisteConfig) -> Result<&Client, CorpusError> {
        self.get_or_build(|| client_for(config)).await
    }
}

/// Client de `config`, ou l'erreur de configuration qui a mis le serveur en mode dégradé.
fn client_for(config: &PisteConfig) -> Result<Client, CorpusError> {
    match config.endpoints() {
        Ok(endpoints) => build_client(config, endpoints),
        Err(problem) => Err(CorpusError::Config { detail: problem.to_string() }),
    }
}

/// Une seule tentative d'envoi, sans réessai ni jugement du statut.
///
/// # Errors
///
/// [`TransportError`] : délai dépassé, panne réseau ou TLS, corps plus gros que la borne.
pub(crate) async fn send_once(
    client: &Client,
    attempt: &Attempt<'_>,
) -> Result<RawResponse, TransportError> {
    let response = request_builder(client, attempt)
        .send()
        .await
        .map_err(|error| classify(error, attempt.endpoint))?;
    let status = response.status().as_u16();
    let retry_after = header_text(&response, &RETRY_AFTER);
    let date = header_text(&response, &DATE);
    let body = read_body(response, attempt.max_body_bytes, attempt.endpoint).await?;
    Ok(RawResponse { status, retry_after, date, body })
}

/// Requête de `attempt`. `RequestBuilder::timeout` remplace le délai du client, même
/// plus long : le délai effectif est toujours `Attempt::timeout`.
fn request_builder(client: &Client, attempt: &Attempt<'_>) -> RequestBuilder {
    let builder = client.post(attempt.url.clone()).timeout(attempt.timeout);
    let builder = match attempt.payload {
        Payload::Json(bytes) => {
            builder.header(CONTENT_TYPE, "application/json").body(bytes.to_vec())
        }
        Payload::Form(pairs) => builder.form(pairs),
    };
    match attempt.bearer {
        // Seul `expose_secret()` de ce fichier ; reqwest marque l'en-tête comme sensible.
        Some(token) => builder.bearer_auth(token.expose_secret()),
        None => builder,
    }
}

/// Valeur d'un en-tête, gardée seulement si c'est du texte ASCII.
fn header_text(response: &Response, name: &HeaderName) -> Option<String> {
    response.headers().get(name)?.to_str().ok().map(str::to_owned)
}

/// Lit le corps en refusant de dépasser `limit` octets.
///
/// Le refus est immédiat quand `Content-Length` annonce plus que la borne (corps non
/// compressé seulement : `reqwest` n'expose pas la longueur d'une réponse gzip, qu'il
/// décompresse en flux). Sinon les morceaux, déjà décompressés, sont comptés : une bombe
/// gzip s'arrête donc à la borne.
async fn read_body(
    mut response: Response,
    limit: u64,
    endpoint: &str,
) -> Result<Vec<u8>, TransportError> {
    if response.content_length().is_some_and(|length| length > limit) {
        return Err(too_large(limit, endpoint));
    }
    let mut body = Vec::new();
    let mut received: u64 = 0;
    while let Some(chunk) = response.chunk().await.map_err(|error| classify(error, endpoint))? {
        received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if received > limit {
            return Err(too_large(limit, endpoint));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Erreur « corps trop gros », au libellé de la borne configurée.
fn too_large(limit: u64, endpoint: &str) -> TransportError {
    TransportError::TooLarge {
        detail: format!("response too large (> {}) on {endpoint}", format_limit(limit)),
    }
}

/// `16 MiB` quand la borne est un nombre entier de mébioctets, sinon `1000 bytes`.
fn format_limit(limit: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if limit.is_multiple_of(MIB) {
        format!("{} MiB", limit / MIB)
    } else {
        format!("{limit} bytes")
    }
}

/// Range une erreur de `reqwest` dans une [`TransportError`] : délai, TLS, ou réseau avec
/// l'un de trois libellés fixes. Le DNS est indistinguable d'un refus de connexion
/// (`connect`), ce qui colle à la politique de 04 §3.5 (« réseau : oui »).
fn classify(error: reqwest::Error, endpoint: &str) -> TransportError {
    let error = error.without_url();
    classify_parts(
        error.is_timeout(),
        error.is_connect(),
        error.is_body() || error.is_decode(),
        &error,
        endpoint,
    )
}

/// Cœur de [`classify`], sur les constats de `reqwest` : `timeout`, panne de `connect`
/// (phase de connexion, TLS compris) et panne de `body` (corps interrompu ou gzip corrompu).
///
/// Délai d'abord. Une panne TLS n'est reconnue que dans la phase de connexion et hors du
/// corps : une poignée de main ou un certificat refusé survient dans le connecteur, alors
/// que `rustls` signale aussi une coupure du pair sans `close_notify` (« peer closed
/// connection without sending TLS `close_notify` ») pendant la réponse ou sur une connexion
/// réutilisée, ce qui est une coupure réseau à réessayer. Les mots de [`TLS_WORDS`] ne
/// servent qu'en second critère.
fn classify_parts(
    timeout: bool,
    connect: bool,
    body: bool,
    error: &(dyn Error + 'static),
    endpoint: &str,
) -> TransportError {
    if timeout {
        return TransportError::Timeout;
    }
    if connect && !body && is_tls_failure(error) {
        return TransportError::Tls { detail: format!("TLS handshake failed on {endpoint}") };
    }
    let kind = if connect {
        "connect"
    } else if body {
        "body"
    } else {
        "request"
    };
    TransportError::Network { detail: format!("network error ({kind}) on {endpoint}") }
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::future::Future;
    use std::io::Write as _;
    use std::process::Output;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use lf_domain::coverage::PisteEnvironment;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use wiremock::matchers::{
        any, body_bytes, body_string, header, header_exists, header_regex, method, path,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::PisteTuning;
    use crate::testing::ClosedPort;

    const ENDPOINT: &str = "/consult/juri";
    const DEFAULT_LIMIT: u64 = 16 * 1024 * 1024;
    /// Borne réduite des tests de limite, pour la vitesse.
    const SMALL_LIMIT: u64 = 1000;
    /// Variable sentinelle de la sonde en sous-processus.
    const PROBE_ENV: &str = "LF_HTTP_PROBE_URL";
    /// Toutes les variantes de noms de proxy, retirées avant chaque sonde.
    const PROXY_VARS: [&str; 8] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ];

    fn user_agent() -> String {
        format!(
            "mcp-legifrance/{} (+https://github.com/muchiny/mcp-legifrance)",
            env!("CARGO_PKG_VERSION")
        )
    }

    /// Configuration dont les deux URL pointent vers `uri` (boucle locale, en `http`).
    fn config_for(uri: &str) -> (PisteConfig, PisteEndpoints) {
        config_with_agent(uri, user_agent())
    }

    /// Comme `config_for`, avec un `User-Agent` explicite.
    fn config_with_agent(uri: &str, user_agent: String) -> (PisteConfig, PisteEndpoints) {
        let (endpoints, _) = PisteEndpoints::with_overrides(
            PisteEnvironment::Sandbox,
            Some(&format!("{uri}/oauth/token")),
            Some(&format!("{uri}/dila/legifrance/lf-engine-app")),
        )
        .unwrap();
        let config =
            PisteConfig::new(PisteEnvironment::Sandbox, Ok(endpoints.clone()), None, user_agent);
        (config, endpoints)
    }

    /// `config` avec un autre délai de requête, le reste des réglages inchangé.
    fn with_request_timeout(config: PisteConfig, request_timeout: Duration) -> PisteConfig {
        let tuning = PisteTuning { request_timeout, ..*config.tuning() };
        config.with_tuning(tuning).unwrap()
    }

    fn client_for_uri(uri: &str) -> Client {
        let (config, endpoints) = config_for(uri);
        build_client(&config, &endpoints).unwrap()
    }

    fn url_of(base: &str, route: &str) -> Url {
        Url::parse(&format!("{base}{route}")).unwrap()
    }

    fn attempt<'a>(url: &'a Url, payload: Payload<'a>) -> Attempt<'a> {
        Attempt {
            url,
            endpoint: ENDPOINT,
            payload,
            bearer: None,
            timeout: Duration::from_secs(5),
            max_body_bytes: DEFAULT_LIMIT,
        }
    }

    /// Un POST JSON sur `ENDPOINT` de `uri`, avec la borne `limit`.
    async fn fetch(uri: &str, limit: u64) -> Result<RawResponse, TransportError> {
        let client = client_for_uri(uri);
        let url = url_of(uri, ENDPOINT);
        let mut request = attempt(&url, Payload::Json(b"{}"));
        request.max_body_bytes = limit;
        send_once(&client, &request).await
    }

    async fn mount_post(server: &MockServer, response: ResponseTemplate) {
        Mock::given(method("POST")).and(path(ENDPOINT)).respond_with(response).mount(server).await;
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    /// Serveur TCP à usage unique : `handler` reçoit la première connexion.
    async fn serve_once<F, Fut>(handler: F) -> String
    where
        F: FnOnce(TcpStream) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handler(stream).await;
        });
        uri
    }

    /// Vrai quand `received` contient les en-têtes puis `Content-Length` octets de corps.
    fn request_is_complete(received: &[u8]) -> bool {
        let Some(end) = received.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let head = String::from_utf8_lossy(&received[..end]).to_lowercase();
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .map_or(0, |value| value.trim().parse::<usize>().unwrap());
        received.len() >= end + 4 + length
    }

    /// Lit la requête entière (en-têtes, puis `Content-Length` octets de corps).
    async fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut received = Vec::new();
        let mut buffer = [0_u8; 4096];
        while !request_is_complete(&received) {
            let count = stream.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            received.extend_from_slice(&buffer[..count]);
        }
        received
    }

    /// Serveur brut : lit la requête entière, puis laisse `script` répondre. Sans cette
    /// lecture complète, la fermeture avec des octets non lus provoque un RST qui détruit
    /// la réponse en route.
    async fn raw_server<F, Fut>(script: F) -> String
    where
        F: FnOnce(TcpStream) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        serve_once(move |mut stream| async move {
            read_request(&mut stream).await;
            script(stream).await;
        })
        .await
    }

    /// Attend que le client ferme la connexion.
    async fn hold_until_closed(stream: &mut TcpStream) {
        let _ = stream.read_to_end(&mut Vec::new()).await;
    }

    /// Serveur brut qui répond en `chunked`, un morceau de `x` par taille de `sizes`.
    async fn chunked_server(sizes: &'static [usize]) -> String {
        raw_server(move |mut stream| async move {
            let _ =
                stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").await;
            for size in sizes {
                let mut chunk = format!("{size:x}\r\n").into_bytes();
                chunk.extend(std::iter::repeat_n(b'x', *size));
                chunk.extend_from_slice(b"\r\n");
                let _ = stream.write_all(&chunk).await;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let _ = stream.write_all(b"0\r\n\r\n").await;
            let _ = stream.shutdown().await;
        })
        .await
    }

    fn too_large(detail: &str) -> TransportError {
        TransportError::TooLarge { detail: detail.to_owned() }
    }

    fn network(detail: &str) -> TransportError {
        TransportError::Network { detail: detail.to_owned() }
    }

    // ---------- Construction et instance unique

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn piste_client_is_built_once() {
        let (config, endpoints) = config_for("http://127.0.0.1:1");
        let shared = Arc::new(SharedClient::new());
        let builds = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let shared = Arc::clone(&shared);
            let builds = Arc::clone(&builds);
            let (config, endpoints) = (config.clone(), endpoints.clone());
            tasks.push(tokio::spawn(async move {
                let client = shared
                    .get_or_build(|| {
                        builds.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(20));
                        build_client(&config, &endpoints)
                    })
                    .await
                    .unwrap();
                std::ptr::from_ref(client).addr()
            }));
        }
        let mut addresses = Vec::new();
        for task in tasks {
            addresses.push(task.await.unwrap());
        }
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert!(addresses.iter().all(|a| *a == addresses[0]), "{addresses:?}");
    }

    #[tokio::test]
    async fn failed_build_is_not_cached() {
        let (config, endpoints) = config_for("http://127.0.0.1:1");
        let shared = SharedClient::new();
        let failure = shared
            .get_or_build(|| Err(CorpusError::Config { detail: "boom".to_owned() }))
            .await
            .unwrap_err();
        assert_eq!(failure, CorpusError::Config { detail: "boom".to_owned() });
        let builds = AtomicUsize::new(0);
        shared
            .get_or_build(|| {
                builds.fetch_add(1, Ordering::SeqCst);
                build_client(&config, &endpoints)
            })
            .await
            .unwrap();
        assert_eq!(builds.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shared_client_is_built_from_the_config() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ENDPOINT))
            .and(header("user-agent", user_agent().as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;
        let (config, _) = config_for(&server.uri());
        let shared = SharedClient::new();
        let client = shared.get(&config).await.unwrap();
        let url = url_of(&server.uri(), ENDPOINT);
        let raw = send_once(client, &attempt(&url, Payload::Json(b"{}"))).await.unwrap();
        assert_eq!(raw.status, 200);
    }

    #[tokio::test]
    async fn config_problem_yields_config_error_without_building() {
        let problem = PisteEndpoints::with_overrides(
            PisteEnvironment::Sandbox,
            None,
            Some("http://example.com/api"),
        )
        .unwrap_err();
        let config =
            PisteConfig::new(PisteEnvironment::Sandbox, Err(problem.clone()), None, user_agent());
        let shared = SharedClient::new();
        let failure = shared.get(&config).await.unwrap_err();
        assert_eq!(failure, CorpusError::Config { detail: problem.to_string() });
        assert!(shared.cell.get().is_none());
    }

    #[test]
    fn invalid_user_agent_is_a_config_error_without_secret() {
        let (config, endpoints) = config_with_agent("http://127.0.0.1:1", "bad\nagent".to_owned());
        assert_eq!(
            build_client(&config, &endpoints).unwrap_err(),
            CorpusError::Config {
                detail:
                    "could not build the HTTP client: builder error: failed to parse header value"
                        .to_owned()
            }
        );
    }

    #[test]
    fn unusable_tls_setup_is_a_config_error() {
        assert_eq!(
            finish_client(Client::builder().use_preconfigured_tls(())).unwrap_err(),
            CorpusError::Config {
                detail: "could not build the HTTP client: builder error: Unknown TLS backend passed to `use_preconfigured_tls`"
                    .to_owned()
            }
        );
    }

    #[test]
    fn plain_http_is_allowed_when_one_endpoint_is_loopback_http() {
        let sandbox = PisteEnvironment::Sandbox;
        assert!(!allows_plain_http(&PisteEndpoints::official(sandbox)));
        let (oauth_only, _) =
            PisteEndpoints::with_overrides(sandbox, Some("http://127.0.0.1:1/oauth/token"), None)
                .unwrap();
        assert!(allows_plain_http(&oauth_only));
        let (base_only, _) =
            PisteEndpoints::with_overrides(sandbox, None, Some("http://127.0.0.1:1/api")).unwrap();
        assert!(allows_plain_http(&base_only));
    }

    #[tokio::test]
    async fn https_only_refuses_plain_http_for_official_endpoints() {
        let server = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;
        let endpoints = PisteEndpoints::official(PisteEnvironment::Sandbox);
        let config =
            PisteConfig::new(PisteEnvironment::Sandbox, Ok(endpoints.clone()), None, user_agent());
        let client = build_client(&config, &endpoints).unwrap();
        let url = url_of(&server.uri(), ENDPOINT);
        let failure = send_once(&client, &attempt(&url, Payload::Json(b"{}"))).await.unwrap_err();
        assert_eq!(
            failure,
            TransportError::Network {
                detail: "network error (request) on /consult/juri".to_owned()
            }
        );
    }

    // ---------- En-têtes et corps de requête

    #[tokio::test]
    async fn user_agent_contains_crate_version() {
        let expected = user_agent();
        assert!(expected.contains(env!("CARGO_PKG_VERSION")), "{expected}");
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("user-agent", expected.as_str()))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap().status, 200);
    }

    #[tokio::test]
    async fn accept_json_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap().status, 200);
    }

    #[tokio::test]
    async fn json_payload_is_sent_with_its_content_type() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ENDPOINT))
            .and(header("content-type", "application/json"))
            .and(body_bytes(br#"{"textId":"JURITEXT000001"}"#.to_vec()))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_for_uri(&server.uri());
        let url = url_of(&server.uri(), ENDPOINT);
        let payload = Payload::Json(br#"{"textId":"JURITEXT000001"}"#);
        assert_eq!(send_once(&client, &attempt(&url, payload)).await.unwrap().status, 200);
    }

    #[tokio::test]
    async fn form_payload_is_urlencoded() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ENDPOINT))
            .and(header("content-type", "application/x-www-form-urlencoded"))
            .and(body_string("grant_type=client_credentials&scope=openid"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_for_uri(&server.uri());
        let url = url_of(&server.uri(), ENDPOINT);
        let pairs = [("grant_type", "client_credentials"), ("scope", "openid")];
        let raw = send_once(&client, &attempt(&url, Payload::Form(&pairs))).await.unwrap();
        assert_eq!(raw.status, 200);
    }

    #[tokio::test]
    async fn bearer_token_is_sent_only_when_given() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/with"))
            .and(header("authorization", "Bearer tok-123"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/without"))
            .and(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/without"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_for_uri(&server.uri());
        let token = SecretString::from("tok-123");
        let with_url = url_of(&server.uri(), "/with");
        let mut with = attempt(&with_url, Payload::Json(b"{}"));
        with.bearer = Some(&token);
        assert_eq!(send_once(&client, &with).await.unwrap().status, 200);
        let without_url = url_of(&server.uri(), "/without");
        let without = attempt(&without_url, Payload::Json(b"{}"));
        assert_eq!(send_once(&client, &without).await.unwrap().status, 204);
    }

    #[test]
    fn bearer_header_is_sensitive() {
        let client = client_for_uri("http://127.0.0.1:1");
        let token = SecretString::from("tok-123");
        let url = url_of("http://127.0.0.1:1", ENDPOINT);
        let mut with = attempt(&url, Payload::Json(b"{}"));
        with.bearer = Some(&token);
        let authorization = request_builder(&client, &with)
            .build()
            .unwrap()
            .headers()
            .get("authorization")
            .unwrap()
            .clone();
        assert!(authorization.is_sensitive());
        assert_eq!(format!("{authorization:?}"), "Sensitive");
    }

    // ---------- Réponse

    #[tokio::test]
    async fn redirect_is_not_followed() {
        let target = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(200)).expect(0).mount(&target).await;
        let origin = MockServer::start().await;
        mount_post(
            &origin,
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}{ENDPOINT}", target.uri())),
        )
        .await;
        let raw = fetch(&origin.uri(), DEFAULT_LIMIT).await.unwrap();
        assert_eq!(raw.status, 302);
    }

    #[tokio::test]
    async fn status_and_two_headers_are_exposed() {
        let server = MockServer::start().await;
        mount_post(
            &server,
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "2")
                .insert_header("Date", "Wed, 21 Oct 2026 07:28:00 GMT")
                .set_body_string("slow down"),
        )
        .await;
        assert_eq!(
            fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap(),
            RawResponse {
                status: 429,
                retry_after: Some("2".to_owned()),
                date: Some("Wed, 21 Oct 2026 07:28:00 GMT".to_owned()),
                body: b"slow down".to_vec(),
            }
        );

        let plain = MockServer::start().await;
        mount_post(&plain, ResponseTemplate::new(200)).await;
        let raw = fetch(&plain.uri(), DEFAULT_LIMIT).await.unwrap();
        assert_eq!((raw.status, raw.retry_after), (200, None));
    }

    #[tokio::test]
    async fn non_ascii_header_is_dropped() {
        let uri = raw_server(|mut stream| async move {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nRetry-After: \xff\xfe\r\nContent-Length: 2\r\n\r\nok",
                )
                .await
                .unwrap();
            stream.shutdown().await.unwrap();
        })
        .await;
        let raw = fetch(&uri, DEFAULT_LIMIT).await.unwrap();
        assert_eq!((raw.status, raw.retry_after, raw.body), (200, None, b"ok".to_vec()));
    }

    // ---------- Borne du corps

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let server = MockServer::start().await;
        mount_post(&server, ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 2000])).await;
        let failure = fetch(&server.uri(), SMALL_LIMIT).await.unwrap_err();
        let detail = "response too large (> 1000 bytes) on /consult/juri";
        assert_eq!(failure, too_large(detail));
        assert_eq!(CorpusError::from(failure), CorpusError::Decode { detail: detail.to_owned() });
    }

    #[tokio::test]
    async fn announced_length_over_the_bound_is_refused_without_reading() {
        let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
        let uri = raw_server(|mut stream| async move {
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", DEFAULT_LIMIT + 1);
            stream.write_all(head.as_bytes()).await.unwrap();
            hold_until_closed(&mut stream).await;
            closed_tx.send(()).unwrap();
        })
        .await;
        let started = Instant::now();
        let failure = fetch(&uri, DEFAULT_LIMIT).await.unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!(failure, too_large("response too large (> 16 MiB) on /consult/juri"));
        assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
        // Le client a bien fermé la connexion sans lire le corps annoncé.
        tokio::time::timeout(Duration::from_secs(2), closed_rx).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn body_at_the_bound_is_accepted_by_content_length() {
        let server = MockServer::start().await;
        mount_post(&server, ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 1000])).await;
        let raw = fetch(&server.uri(), SMALL_LIMIT).await.unwrap();
        assert_eq!(raw.body, vec![b'x'; 1000]);
    }

    #[tokio::test]
    async fn oversized_body_without_content_length_is_rejected() {
        let uri = chunked_server(&[600, 600]).await;
        let failure = fetch(&uri, SMALL_LIMIT).await.unwrap_err();
        assert_eq!(failure, too_large("response too large (> 1000 bytes) on /consult/juri"));
    }

    #[tokio::test]
    async fn one_byte_over_the_bound_without_content_length_is_rejected() {
        let uri = chunked_server(&[1000, 1]).await;
        let failure = fetch(&uri, SMALL_LIMIT).await.unwrap_err();
        assert_eq!(failure, too_large("response too large (> 1000 bytes) on /consult/juri"));
    }

    #[tokio::test]
    async fn body_at_the_bound_without_content_length_is_accepted() {
        let uri = chunked_server(&[500, 500]).await;
        let raw = fetch(&uri, SMALL_LIMIT).await.unwrap();
        assert_eq!(raw.body, vec![b'x'; 1000]);
    }

    #[tokio::test]
    async fn oversized_body_at_the_default_bound_is_rejected() {
        let server = MockServer::start().await;
        let size = usize::try_from(DEFAULT_LIMIT).unwrap() + 1;
        mount_post(&server, ResponseTemplate::new(200).set_body_bytes(vec![b'x'; size])).await;
        let failure = fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap_err();
        assert_eq!(failure, too_large("response too large (> 16 MiB) on /consult/juri"));
    }

    #[tokio::test]
    async fn body_of_exactly_the_default_bound_is_accepted() {
        let server = MockServer::start().await;
        let size = usize::try_from(DEFAULT_LIMIT).unwrap();
        mount_post(&server, ResponseTemplate::new(200).set_body_bytes(vec![b'x'; size])).await;
        let raw = fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap();
        assert_eq!(raw.body.len(), size);
    }

    #[test]
    fn limit_is_written_in_mib_when_whole() {
        assert_eq!(format_limit(16 * 1024 * 1024), "16 MiB");
        assert_eq!(format_limit(3 * 1024 * 1024), "3 MiB");
        assert_eq!(format_limit(1000), "1000 bytes");
        assert_eq!(format_limit(1024 * 1024 + 1), "1048577 bytes");
    }

    // ---------- gzip (V-70, côté client)

    #[tokio::test]
    async fn gzip_body_is_limited_after_decompression() {
        let compressed = gzip(&vec![0_u8; usize::try_from(DEFAULT_LIMIT).unwrap() + 1]);
        let size = compressed.len();
        assert!(size < 64 * 1024, "{size}");
        let server = MockServer::start().await;
        mount_post(
            &server,
            ResponseTemplate::new(200)
                .insert_header("Content-Encoding", "gzip")
                .set_body_bytes(compressed),
        )
        .await;
        let failure = fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap_err();
        assert_eq!(failure, too_large("response too large (> 16 MiB) on /consult/juri"));
    }

    #[tokio::test]
    async fn gzip_is_negotiated() {
        let plain = br#"{"text":{"id":"JURITEXT000001"}}"#;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ENDPOINT))
            .and(header_regex("accept-encoding", "gzip"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Encoding", "gzip")
                    .set_body_bytes(gzip(plain)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let raw = fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap();
        assert_eq!(raw.body, plain.to_vec());
    }

    #[tokio::test]
    async fn corrupt_gzip_body_is_a_network_error() {
        let server = MockServer::start().await;
        mount_post(
            &server,
            ResponseTemplate::new(200)
                .insert_header("Content-Encoding", "gzip")
                .set_body_bytes(b"this is not gzip at all".to_vec()),
        )
        .await;
        let failure = fetch(&server.uri(), DEFAULT_LIMIT).await.unwrap_err();
        assert_eq!(failure, network("network error (body) on /consult/juri"));
    }

    // ---------- Délais et réseau

    #[tokio::test]
    async fn request_timeout_maps_to_timeout() {
        // Durée réelle, pas d'horloge tokio gelée (04 §13.1).
        let server = MockServer::start().await;
        mount_post(&server, ResponseTemplate::new(200).set_delay(Duration::from_secs(1))).await;
        let (config, endpoints) = config_for(&server.uri());
        let config = with_request_timeout(config, Duration::from_millis(200));
        let client = build_client(&config, &endpoints).unwrap();
        let url = url_of(&server.uri(), ENDPOINT);
        let mut request = attempt(&url, Payload::Json(b"{}"));
        request.timeout = config.tuning().request_timeout;
        let started = Instant::now();
        let failure = send_once(&client, &request).await.unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!(failure, TransportError::Timeout);
        assert!(elapsed < Duration::from_millis(900), "{elapsed:?}");
        assert_eq!(CorpusError::from(failure), CorpusError::Timeout);
    }

    #[tokio::test]
    async fn attempt_timeout_overrides_the_client_timeout() {
        let server = MockServer::start().await;
        mount_post(&server, ResponseTemplate::new(200).set_delay(Duration::from_secs(1))).await;
        let (config, endpoints) = config_for(&server.uri());
        let config = with_request_timeout(config, Duration::from_secs(5));
        let client = build_client(&config, &endpoints).unwrap();
        let url = url_of(&server.uri(), ENDPOINT);
        let mut request = attempt(&url, Payload::Json(b"{}"));
        request.timeout = Duration::from_millis(200);
        let started = Instant::now();
        let failure = send_once(&client, &request).await.unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!(failure, TransportError::Timeout);
        assert!(elapsed < Duration::from_millis(900), "{elapsed:?}");
    }

    #[tokio::test]
    async fn client_level_timeout_is_the_configured_request_timeout() {
        let server = MockServer::start().await;
        mount_post(&server, ResponseTemplate::new(200).set_delay(Duration::from_secs(1))).await;
        let (config, endpoints) = config_for(&server.uri());
        let config = with_request_timeout(config, Duration::from_millis(200));
        let client = build_client(&config, &endpoints).unwrap();
        let started = Instant::now();
        let error = client.post(url_of(&server.uri(), ENDPOINT)).send().await.unwrap_err();
        let elapsed = started.elapsed();
        assert!(error.is_timeout(), "{error}");
        assert!(elapsed < Duration::from_millis(900), "{elapsed:?}");
    }

    /// Serveur brut qui répond vite les en-têtes (`chunked`), puis un octet toutes les 50 ms
    /// pendant `drip` : une lecture du corps au goutte-à-goutte.
    async fn slow_body_server(drip: Duration) -> String {
        raw_server(move |mut stream| async move {
            let head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
            let _ = stream.write_all(head).await;
            let started = Instant::now();
            while started.elapsed() < drip {
                let _ = stream.write_all(b"1\r\nx\r\n").await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let _ = stream.write_all(b"0\r\n\r\n").await;
        })
        .await
    }

    #[tokio::test]
    async fn a_slow_body_is_bounded_by_the_attempt_timeout() {
        // Les en-têtes arrivent tout de suite ; seul le corps goutte : la borne doit couvrir
        // la lecture du corps, pas seulement l'attente des en-têtes (04 §5.3).
        let uri = slow_body_server(Duration::from_secs(3)).await;
        let client = client_for_uri(&uri);
        let url = url_of(&uri, ENDPOINT);
        let mut request = attempt(&url, Payload::Json(b"{}"));
        request.timeout = Duration::from_millis(200);
        let started = Instant::now();
        let failure = send_once(&client, &request).await.unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!(failure, TransportError::Timeout);
        assert!(elapsed < Duration::from_millis(900), "{elapsed:?}");
    }

    #[tokio::test]
    async fn a_slow_body_that_finishes_within_the_attempt_timeout_is_read() {
        let uri = slow_body_server(Duration::from_millis(150)).await;
        let client = client_for_uri(&uri);
        let url = url_of(&uri, ENDPOINT);
        let request = attempt(&url, Payload::Json(b"{}"));
        let response = send_once(&client, &request).await.unwrap();
        assert!(response.body.len() >= 2 && response.body.iter().all(|byte| *byte == b'x'));
    }

    #[tokio::test]
    async fn a_longer_attempt_timeout_wins_over_a_shorter_client_timeout() {
        let server = MockServer::start().await;
        mount_post(&server, ResponseTemplate::new(200).set_delay(Duration::from_millis(500))).await;
        let (config, endpoints) = config_for(&server.uri());
        let config = with_request_timeout(config, Duration::from_millis(100));
        let client = build_client(&config, &endpoints).unwrap();
        let url = url_of(&server.uri(), ENDPOINT);
        let mut request = attempt(&url, Payload::Json(b"{}"));
        request.timeout = Duration::from_secs(5);
        let response = send_once(&client, &request).await.unwrap();
        assert_eq!(response.status, 200);
    }

    // ---------- Debug sans secret

    #[test]
    fn debug_of_the_request_types_shows_no_value() {
        let url =
            Url::parse("https://user:pw@api.example/dila/secret-path?key=url-secret").unwrap();
        let token = SecretString::from("tok-visible-if-leaked");
        let form = [("client_id", "id-visible"), ("client_secret", "secret-visible")];
        let mut request = attempt(&url, Payload::Form(&form));
        request.bearer = Some(&token);
        let json = Payload::Json(br#"{"q":"json-visible"}"#);
        assert_eq!(format!("{json:?}"), "Json { bytes: 20 }");
        assert_eq!(
            format!("{:?}", request.payload),
            r#"Form { fields: ["client_id", "client_secret"] }"#
        );
        assert_eq!(
            format!("{request:?}"),
            "Attempt { endpoint: \"/consult/juri\", payload: Form { fields: [\"client_id\", \"client_secret\"] }, bearer: true, timeout: 5s, max_body_bytes: 16777216 }"
        );
        request.bearer = None;
        assert!(format!("{request:?}").contains("bearer: false"));
    }

    #[test]
    fn debug_of_a_raw_response_shows_no_body() {
        let response = RawResponse {
            status: 200,
            retry_after: Some("30".to_owned()),
            date: None,
            body: br#"{"access_token":"tok-visible-if-leaked"}"#.to_vec(),
        };
        assert_eq!(
            format!("{response:?}"),
            r#"RawResponse { status: 200, retry_after: Some("30"), date: None, body_len: 40 }"#
        );
        let outcome = crate::classify::Outcome::Success(response);
        let shown = format!("{outcome:?}");
        assert!(
            shown.starts_with("Success(RawResponse {") && !shown.contains("tok-visible"),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn connection_refused_is_a_network_error() {
        let closed = ClosedPort::new();
        let failure = fetch(&closed.uri(), DEFAULT_LIMIT).await.unwrap_err();
        let detail = "network error (connect) on /consult/juri";
        assert_eq!(failure, network(detail));
        assert_eq!(
            CorpusError::from(failure),
            CorpusError::Upstream { status: None, retryable: true, detail: detail.to_owned() }
        );
    }

    #[tokio::test]
    async fn truncated_body_is_a_network_error() {
        let uri = raw_server(|mut stream| async move {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n0123456789")
                .await
                .unwrap();
            stream.shutdown().await.unwrap();
        })
        .await;
        let failure = fetch(&uri, DEFAULT_LIMIT).await.unwrap_err();
        assert_eq!(failure, network("network error (body) on /consult/juri"));
    }

    #[tokio::test]
    async fn network_error_message_never_contains_the_url() {
        let closed = ClosedPort::new();
        let uri = closed.uri();
        let client = client_for_uri(&uri);
        let url = url_of(&uri, "/secret-route?key=abc123");
        let failure = send_once(&client, &attempt(&url, Payload::Json(b"{}"))).await.unwrap_err();
        let debug = format!("{failure:?}");
        let shown = CorpusError::from(failure).to_string();
        for text in [debug, shown] {
            assert!(!text.contains("abc123"), "{text}");
            assert!(!text.contains("127.0.0.1"), "{text}");
        }
    }

    // ---------- TLS (reconnaissance par le texte)

    #[tokio::test]
    async fn tls_handshake_failure_is_a_tls_error() {
        // Réponse en clair à un ClientHello ; le ClientHello ne contient pas `\r\n\r\n`.
        let uri = serve_once(|mut stream| async move {
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer).await;
            let _ =
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await;
            let _ = stream.shutdown().await;
        })
        .await;
        let endpoints = PisteEndpoints::official(PisteEnvironment::Sandbox);
        let config =
            PisteConfig::new(PisteEnvironment::Sandbox, Ok(endpoints.clone()), None, user_agent());
        let client = build_client(&config, &endpoints).unwrap();
        let url = url_of(&uri.replace("http://", "https://"), ENDPOINT);
        let failure = send_once(&client, &attempt(&url, Payload::Json(b"{}"))).await.unwrap_err();
        let detail = "TLS handshake failed on /consult/juri";
        assert_eq!(failure, TransportError::Tls { detail: detail.to_owned() });
        assert_eq!(
            CorpusError::from(failure),
            CorpusError::Upstream { status: None, retryable: false, detail: detail.to_owned() }
        );
    }

    /// Erreur de test à cause chaînée.
    #[derive(Debug)]
    struct Link {
        text: &'static str,
        cause: Option<Box<Link>>,
    }

    impl fmt::Display for Link {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.text)
        }
    }

    impl Error for Link {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.cause.as_deref().map(|cause| cause as &(dyn Error + 'static))
        }
    }

    fn chain_of(texts: &[&'static str]) -> Link {
        let mut texts = texts.iter().rev();
        let mut link = Link { text: texts.next().unwrap(), cause: None };
        for text in texts {
            link = Link { text, cause: Some(Box::new(link)) };
        }
        link
    }

    #[test]
    fn tls_failure_is_found_anywhere_in_the_chain() {
        assert!(is_tls_failure(&chain_of(&[
            "outer",
            "middle",
            "received fatal alert: HandshakeFailure"
        ])));
        assert!(is_tls_failure(&chain_of(&["TLS error", "inner"])));
        assert!(!is_tls_failure(&chain_of(&["outer", "middle", "inner"])));
    }

    #[test]
    fn tls_words_are_matched_case_insensitively() {
        for text in [
            "received fatal alert: HandshakeFailure",
            "invalid Certificate",
            "TLS error",
            "received corrupt message of type InvalidContentType",
        ] {
            assert!(is_tls_failure(&chain_of(&[text])), "{text}");
        }
        assert!(!is_tls_failure(&chain_of(&["connection reset by peer"])));
    }

    #[test]
    fn peer_closing_without_close_notify_is_a_network_error() {
        let closed = chain_of(&[
            "error decoding response body",
            "peer closed connection without sending TLS close_notify",
        ]);
        // En cours de réponse, puis à l'envoi sur une connexion du pool : jamais `Tls`.
        assert_eq!(
            classify_parts(false, false, true, &closed, ENDPOINT),
            network("network error (body) on /consult/juri")
        );
        assert_eq!(
            classify_parts(false, false, false, &closed, ENDPOINT),
            network("network error (request) on /consult/juri")
        );
        // Même dans la phase de connexion, un échec du corps n'est pas une poignée de main.
        assert_eq!(
            classify_parts(false, true, true, &closed, ENDPOINT),
            network("network error (connect) on /consult/juri")
        );
    }

    #[test]
    fn handshake_and_certificate_failures_during_connect_stay_tls() {
        let tls =
            TransportError::Tls { detail: "TLS handshake failed on /consult/juri".to_owned() };
        for text in
            ["received fatal alert: HandshakeFailure", "invalid peer certificate: UnknownIssuer"]
        {
            let failure = chain_of(&["client error (Connect)", text]);
            assert_eq!(classify_parts(false, true, false, &failure, ENDPOINT), tls);
        }
        // Dans la phase de connexion, une panne sans mot TLS reste une panne réseau.
        assert_eq!(
            classify_parts(false, true, false, &chain_of(&["connection reset by peer"]), ENDPOINT),
            network("network error (connect) on /consult/juri")
        );
    }

    #[test]
    fn timeout_wins_over_every_other_finding() {
        let tls = chain_of(&["received fatal alert: HandshakeFailure"]);
        assert_eq!(classify_parts(true, true, true, &tls, ENDPOINT), TransportError::Timeout);
    }

    #[test]
    fn error_chain_is_described_from_effect_to_cause() {
        assert_eq!(describe(&chain_of(&["outer", "middle", "inner"])), "outer: middle: inner");
        assert_eq!(describe(&chain_of(&["alone"])), "alone");
    }

    #[test]
    fn transport_errors_map_to_corpus_errors() {
        let detail = || "d".to_owned();
        assert_eq!(CorpusError::from(TransportError::Timeout), CorpusError::Timeout);
        assert_eq!(
            CorpusError::from(TransportError::Network { detail: detail() }),
            CorpusError::Upstream { status: None, retryable: true, detail: detail() }
        );
        assert_eq!(
            CorpusError::from(TransportError::Tls { detail: detail() }),
            CorpusError::Upstream { status: None, retryable: false, detail: detail() }
        );
        assert_eq!(
            CorpusError::from(TransportError::TooLarge { detail: detail() }),
            CorpusError::Decode { detail: detail() }
        );
    }

    // ---------- Proxy (V-03), en sous-processus : `unsafe_code` interdit `set_var`

    /// Sonde : ne fait rien sans la variable sentinelle ; sinon un `send_once` vers cette URL.
    #[tokio::test]
    async fn proxy_child_probe() {
        let Ok(target) = std::env::var(PROBE_ENV) else {
            return;
        };
        let client = client_for_uri("http://127.0.0.1:1");
        let url = Url::parse(&target).unwrap();
        let result = send_once(&client, &attempt(&url, Payload::Json(b"{}"))).await;
        if url.scheme() == "http" {
            let raw = result.unwrap();
            assert_eq!((raw.status, raw.body), (200, b"probe-ok".to_vec()));
        }
    }

    /// Relance ce binaire de test sur la sonde, avec les seules variables de proxy de `env`.
    async fn run_probe(target: &str, env: &[(&str, &str)]) -> Output {
        let exe = std::env::current_exe().unwrap();
        let target = target.to_owned();
        let env: Vec<(String, String)> =
            env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        tokio::task::spawn_blocking(move || {
            let mut command = std::process::Command::new(exe);
            command.args([
                "--exact",
                "http::tests::proxy_child_probe",
                "--nocapture",
                "--test-threads=1",
            ]);
            for name in PROXY_VARS {
                command.env_remove(name);
            }
            command.env(PROBE_ENV, target);
            for (name, value) in env {
                command.env(name, value);
            }
            command.output().unwrap()
        })
        .await
        .unwrap()
    }

    fn probe_log(output: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    async fn probe_server() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("probe-ok"))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn proxy_env_is_honoured() {
        let proxy = probe_server().await;
        let output =
            run_probe("http://upstream.test:8080/consult/juri", &[("HTTP_PROXY", &proxy.uri())])
                .await;
        let log = probe_log(&output);
        assert!(output.status.success(), "{log}");
        let requests = proxy.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "{log}");
        assert_eq!(requests[0].url.as_str(), "http://upstream.test:8080/consult/juri");
        assert_eq!(
            requests[0].headers.get("host").unwrap().to_str().unwrap(),
            "upstream.test:8080"
        );
    }

    #[tokio::test]
    async fn https_proxy_env_asks_for_a_tunnel() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let first_line = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 1024];
            let count = stream.read(&mut buffer).await.unwrap();
            String::from_utf8_lossy(&buffer[..count]).lines().next().unwrap().to_owned()
        });
        let output =
            run_probe("https://upstream.test:443/consult/juri", &[("HTTPS_PROXY", &proxy)]).await;
        let log = probe_log(&output);
        assert!(output.status.success(), "{log}");
        let line = tokio::time::timeout(Duration::from_secs(5), first_line).await.unwrap().unwrap();
        assert_eq!(line, "CONNECT upstream.test:443 HTTP/1.1");
    }

    #[tokio::test]
    async fn no_proxy_env_bypasses_the_proxy() {
        let proxy = probe_server().await;
        let origin = probe_server().await;
        let target = format!("{}{ENDPOINT}", origin.uri());

        // Contraste : sans NO_PROXY, la boucle locale passe par le proxy.
        let output = run_probe(&target, &[("HTTP_PROXY", &proxy.uri())]).await;
        let log = probe_log(&output);
        assert!(output.status.success(), "{log}");
        assert_eq!(proxy.received_requests().await.unwrap().len(), 1, "{log}");
        assert_eq!(origin.received_requests().await.unwrap().len(), 0, "{log}");

        // NO_PROXY : l'origine est jointe directement, le proxy ne voit rien de plus.
        let output = run_probe(
            &target,
            &[("HTTP_PROXY", &proxy.uri()), ("NO_PROXY", "127.0.0.1,localhost")],
        )
        .await;
        let log = probe_log(&output);
        assert!(output.status.success(), "{log}");
        assert_eq!(proxy.received_requests().await.unwrap().len(), 1, "{log}");
        assert_eq!(origin.received_requests().await.unwrap().len(), 1, "{log}");
    }
}
