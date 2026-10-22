//! Jeton d'accès OAuth 2.0 `client_credentials` de PISTE (04 §3).
//!
//! Une seule requête token à la fois : un `Mutex` couvre la lecture du jeton en cache et son
//! renouvellement, les appelants concurrents attendent puis lisent le jeton neuf. Le jeton
//! n'est jamais écrit sur disque ni dans un journal, et le secret du client ne sort d'ici que
//! dans le formulaire de la requête. Pas de limiteur de débit : les quotas visent l'API, et
//! le jeton se demande environ une fois par heure (04 §3.3). La classification des erreurs
//! propre au jeton (400, 401, 403, 404, 405) tient dans `status_error` ; le reste suit la
//! classification de l'API (`classify`).

use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use lf_application::ports::CorpusError;
use lf_domain::coverage::PisteEnvironment;
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{Mutex, MutexGuard};

use crate::classify::{Outcome, classify};
use crate::config::{PisteConfig, PisteCredentials};
use crate::deadline::CallDeadline;
use crate::dto::oauth::{OAuthErrorDto, TokenResponseDto, is_header_safe};
use crate::http::{Attempt, Payload, RawResponse, SharedClient, TransportError, send_once};
use crate::retry::{RetryContext, SystemJitter, run};
use crate::time::{MonoClock, Sleeper, race};

/// Libellé de l'endpoint dans les `detail` de transport et de classification (jamais l'URL).
const TOKEN_ENDPOINT: &str = "PISTE OAuth";
/// Marge de renouvellement avant l'expiration.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);
/// La marge ne dépasse jamais la moitié de la durée de vie.
const MARGIN_DIVISOR: u32 = 2;
/// Durée de vie retenue quand `expires_in` manque, n'est pas un nombre ou vaut zéro.
const DEFAULT_EXPIRES_IN_SECS: u64 = 3_600;
/// Durée de vie maximale retenue.
const MAX_EXPIRES_IN_SECS: u64 = 86_400;
/// Les seuls codes d'erreur repris dans un `detail` : ceux de la RFC 6749 §5.2. Toute autre
/// valeur (un secret, un identifiant, un jeton, un UUID renvoyés par une passerelle) est omise.
const RFC_6749_ERROR_CODES: [&str; 6] = [
    "invalid_request",
    "invalid_client",
    "invalid_grant",
    "unauthorized_client",
    "unsupported_grant_type",
    "invalid_scope",
];

/// Jeton prêté à un appelant, avec le numéro de génération à rendre à `invalidate`.
#[derive(Debug, Clone)]
pub(crate) struct TokenLease {
    /// Jeton d'accès ; son `Debug` est rédigé.
    pub value: SecretString,
    /// Génération du jeton : croissante, à partir de 1.
    pub generation: u64,
}

/// Jeton en cache.
#[derive(Debug)]
struct CachedToken {
    /// Jeton d'accès.
    value: SecretString,
    /// Instant (horloge du gestionnaire) à partir duquel il faut le renouveler.
    refresh_at: Duration,
    /// Génération, attribuée à l'obtention.
    generation: u64,
}

impl CachedToken {
    /// Prête une copie du jeton.
    fn lease(&self) -> TokenLease {
        TokenLease { value: self.value.clone(), generation: self.generation }
    }
}

/// État protégé par le verrou.
#[derive(Debug)]
struct State {
    /// Jeton courant, s'il y en a un.
    cached: Option<CachedToken>,
    /// Nombre de jetons obtenus depuis le début : la dernière génération attribuée.
    issued: u64,
}

/// Jeton obtenu de PISTE et sa durée de vie.
#[derive(Debug)]
struct Grant {
    /// Jeton d'accès.
    value: SecretString,
    /// Durée de vie retenue, bornée.
    lifetime: Duration,
}

/// Gestionnaire du jeton : un jeton en mémoire, renouvelé avant son expiration.
pub(crate) struct TokenManager<K, S> {
    /// Client HTTP partagé.
    client: Arc<SharedClient>,
    /// Configuration de l'adapter.
    config: Arc<PisteConfig>,
    /// Horloge monotone, la même que celle des échéances.
    clock: K,
    /// Seule porte d'entrée des attentes.
    sleeper: S,
    /// Jeton courant et compteur de générations.
    state: Mutex<State>,
}

impl<K: MonoClock, S: Sleeper> TokenManager<K, S> {
    /// Crée un gestionnaire sans jeton ; aucune E/S.
    pub(crate) fn new(
        client: Arc<SharedClient>,
        config: Arc<PisteConfig>,
        clock: K,
        sleeper: S,
    ) -> Self {
        Self {
            client,
            config,
            clock,
            sleeper,
            state: Mutex::new(State { cached: None, issued: 0 }),
        }
    }

    /// Rend un jeton valable, en le renouvelant si besoin (une seule requête à la fois).
    ///
    /// L'état n'est écrit qu'après un succès : un appel abandonné laisse tout intact.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Timeout`] si le verrou ou la requête dépasse `deadline` ;
    /// [`CorpusError::Config`] (identifiants ou adresses absents, réponse inexploitable) ;
    /// [`CorpusError::Auth`] si PISTE refuse les identifiants ; sinon l'erreur classée de la
    /// requête.
    pub(crate) async fn token(&self, deadline: &CallDeadline) -> Result<TokenLease, CorpusError> {
        let mut state = self.lock(deadline).await?;
        if let Some(cached) = &state.cached
            && self.clock.now() < cached.refresh_at
        {
            return Ok(cached.lease());
        }
        let (grant, obtained_at) = self.fetch(deadline).await?;
        state.issued += 1;
        let cached = CachedToken {
            value: grant.value,
            refresh_at: refresh_at(obtained_at, grant.lifetime),
            generation: state.issued,
        };
        let lease = cached.lease();
        state.cached = Some(cached);
        Ok(lease)
    }

    /// Oublie le jeton de la génération `generation`, et seulement lui : un jeton plus récent
    /// obtenu entre-temps par un autre appelant est gardé.
    ///
    /// Le verrou est attendu comme dans [`Self::token`], au plus jusqu'à l'échéance de l'appelant
    /// (04 §5.3) : sans cette borne, l'attente serait la durée de tous les renouvellements en
    /// file, chacun borné par l'échéance de son seul appelant. Rien n'est modifié si l'attente
    /// est écourtée.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Timeout`] si le verrou n'est pas pris avant `deadline` ; le jeton refusé
    /// reste alors en mémoire.
    pub(crate) async fn invalidate(
        &self,
        generation: u64,
        deadline: &CallDeadline,
    ) -> Result<(), CorpusError> {
        let mut state = self.lock(deadline).await?;
        if state.cached.as_ref().is_some_and(|cached| cached.generation == generation) {
            state.cached = None;
        }
        Ok(())
    }

    /// Prend le verrou, au plus jusqu'à l'échéance. Le verrou est interrogé en premier : un
    /// verrou libre est pris même si l'échéance est passée.
    async fn lock(&self, deadline: &CallDeadline) -> Result<MutexGuard<'_, State>, CorpusError> {
        let wait = pin!(self.sleeper.sleep(deadline.remaining(&self.clock)));
        race(self.state.lock(), wait).await.ok_or(CorpusError::Timeout)
    }

    /// Demande un jeton à PISTE, avec la politique de réessai, et rend l'instant d'obtention.
    ///
    /// Cet instant est lu une seule fois, avant la première tentative : les attentes de
    /// réessai ne repoussent pas le renouvellement.
    async fn fetch(&self, deadline: &CallDeadline) -> Result<(Grant, Duration), CorpusError> {
        let (url, credentials) = request_parts(&self.config)?;
        if deadline.is_expired(&self.clock) {
            return Err(CorpusError::Timeout);
        }
        let client = self.client.get(&self.config).await?;
        let obtained_at = self.clock.now();
        // Seul `expose_secret()` du formulaire OAuth.
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", credentials.client_id()),
            ("client_secret", credentials.client_secret().expose_secret()),
            ("scope", "openid"),
        ];
        let ctx = RetryContext {
            clock: &self.clock,
            sleeper: &self.sleeper,
            deadline,
            jitter: &SystemJitter,
        };
        let grant = run(&ctx, || async {
            let attempt = Attempt {
                url,
                endpoint: TOKEN_ENDPOINT,
                payload: Payload::Form(&form),
                bearer: None,
                timeout: deadline.cap(&self.clock, self.config.tuning().request_timeout),
                max_body_bytes: self.config.tuning().max_body_bytes,
            };
            token_outcome(send_once(client, &attempt).await, self.config.env(), url)
        })
        .await?;
        Ok((grant, obtained_at))
    }
}

/// Adresse du jeton et identifiants, ou l'erreur de configuration (mode dégradé, 04 §2.4).
fn request_parts(config: &PisteConfig) -> Result<(&Url, &PisteCredentials), CorpusError> {
    let endpoints = config
        .endpoints()
        .map_err(|problem| CorpusError::Config { detail: problem.to_string() })?;
    let credentials = config
        .credentials()
        .ok_or_else(|| CorpusError::Config { detail: "PISTE credentials missing".to_owned() })?;
    Ok((endpoints.oauth_url(), credentials))
}

/// Durée de vie retenue d'après `expires_in` : absent ou nul donne 3 600 s, au plus 86 400 s.
fn lifetime_of(expires_in: Option<u64>) -> Duration {
    match expires_in {
        None | Some(0) => Duration::from_secs(DEFAULT_EXPIRES_IN_SECS),
        Some(seconds) => Duration::from_secs(seconds.min(MAX_EXPIRES_IN_SECS)),
    }
}

/// Instant de renouvellement : obtention plus durée de vie, moins la marge (60 s, ou la
/// moitié de la durée de vie si elle est plus courte).
fn refresh_at(obtained_at: Duration, lifetime: Duration) -> Duration {
    let margin = REFRESH_MARGIN.min(lifetime / MARGIN_DIVISOR);
    obtained_at.saturating_add(lifetime).saturating_sub(margin)
}

/// Classe le résultat d'une tentative : les statuts propres au jeton d'abord, puis la
/// classification de l'API.
fn token_outcome(
    result: Result<RawResponse, TransportError>,
    env: PisteEnvironment,
    url: &Url,
) -> Outcome<Grant> {
    if let Ok(response) = &result
        && let Some(error) = status_error(response, env, url)
    {
        return Outcome::Fatal(error);
    }
    from_classified(classify(result, TOKEN_ENDPOINT), url)
}

/// Reprend une issue classée : seul un succès se lit, un 401 n'a pas de sens ici (il est
/// traité avant par `status_error`) et devient définitif.
fn from_classified(outcome: Outcome<RawResponse>, url: &Url) -> Outcome<Grant> {
    match outcome {
        Outcome::Success(response) => grant_from(&response, url),
        Outcome::Transient { error, retry_after, budget } => {
            Outcome::Transient { error, retry_after, budget }
        }
        Outcome::Fatal(error) | Outcome::Unauthorized(error) => Outcome::Fatal(error),
    }
}

/// Erreur propre à l'endpoint token : 400, 401 et 403 refusent les identifiants, 404 et 405
/// désignent une mauvaise adresse. Les autres statuts suivent la classification de l'API.
fn status_error(response: &RawResponse, env: PisteEnvironment, url: &Url) -> Option<CorpusError> {
    match response.status {
        400 | 401 | 403 => Some(rejection(response, env)),
        404 | 405 => Some(CorpusError::Config {
            detail: format!(
                "PISTE OAuth endpoint answered HTTP {} at {}",
                response.status,
                location(url)
            ),
        }),
        _ => None,
    }
}

/// Refus des identifiants, avec le code `error` du corps s'il est l'un des six codes de la
/// RFC 6749 §5.2.
/// `error_description` n'est jamais recopié : il pourrait citer la requête.
fn rejection(response: &RawResponse, env: PisteEnvironment) -> CorpusError {
    let code = serde_json::from_slice::<OAuthErrorDto>(&response.body)
        .ok()
        .and_then(|dto| dto.error)
        .filter(|code| is_rfc_6749_code(code));
    let quoted = code.map(|code| format!(", error {code}")).unwrap_or_default();
    CorpusError::Auth {
        detail: format!(
            "OAuth token request rejected: HTTP {}{quoted}, environment {env}",
            response.status
        ),
    }
}

/// Vrai pour un code d'erreur citable : l'un des six codes de la RFC 6749 §5.2, comparaison
/// exacte. Une liste fermée et non un filtre de caractères : un secret au format UUID ou
/// hexadécimal passerait n'importe quel filtre de forme.
fn is_rfc_6749_code(code: &str) -> bool {
    RFC_6749_ERROR_CODES.contains(&code)
}

/// `scheme://hôte[:port]/chemin` : l'adresse sans identifiants, requête ni fragment.
fn location(url: &Url) -> String {
    let mut shown = url.clone();
    // L'URL peut ne pas porter d'identifiants (ou ne pas pouvoir en porter) : sans effet alors.
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    shown.set_query(None);
    shown.set_fragment(None);
    shown.to_string()
}

/// Lit une réponse 200 : un jeton utilisable et sa durée de vie, ou une erreur de configuration.
fn grant_from(response: &RawResponse, url: &Url) -> Outcome<Grant> {
    let Ok(dto) = serde_json::from_slice::<TokenResponseDto>(&response.body) else {
        return Outcome::Fatal(CorpusError::Config {
            detail: format!("PISTE OAuth response is not valid JSON at {}", location(url)),
        });
    };
    let expires_in = dto.expires_in;
    let not_bearer =
        dto.token_type.as_deref().is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"));
    let Some(value) = dto.usable_token() else {
        return Outcome::Fatal(CorpusError::Config {
            detail: "PISTE OAuth returned no access_token".to_owned(),
        });
    };
    if !is_header_safe(&value) {
        return Outcome::Fatal(CorpusError::Config {
            detail: "PISTE OAuth returned an unusable access_token".to_owned(),
        });
    }
    if not_bearer {
        tracing::warn!("PISTE OAuth token_type is not Bearer, using the token anyway");
    }
    Outcome::Success(Grant { value, lifetime: lifetime_of(expires_in) })
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use lf_domain::coverage::PisteEnvironment;
    use secrecy::{ExposeSecret, SecretString};
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinSet;
    use tracing_subscriber::fmt::MakeWriter;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::classify::Budget;
    use crate::config::{PisteConfig, PisteCredentials, PisteEndpoints, PisteTuning};
    use crate::testing::ClosedPort;
    use crate::time::{ManualClock, RecordingSleeper, TokioClock, TokioSleeper};

    const CLIENT_ID: &str = "my-client-id";
    const CLIENT_SECRET: &str = "my-client-secret";
    const FORM_BODY: &str = "grant_type=client_credentials&client_id=my-client-id&client_secret=my-client-secret&scope=openid";
    /// Base de l'API : jamais jointe par ces tests, le port 9 n'écoute pas.
    const UNUSED_API: &str = "http://127.0.0.1:9/dila/legifrance/lf-engine-app";
    const UNUSED_URI: &str = "http://127.0.0.1:9";
    const SANDBOX: PisteEnvironment = PisteEnvironment::Sandbox;

    const fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Un nanoseconde avant `instant` (un `-` brut sur `Duration` est refusé par clippy).
    fn before(instant: Duration) -> Duration {
        instant.saturating_sub(Duration::from_nanos(1))
    }

    fn credentials(secret: &str) -> PisteCredentials {
        PisteCredentials::new(CLIENT_ID.to_owned(), SecretString::from(secret.to_owned()))
    }

    fn config_with(
        env: PisteEnvironment,
        oauth_url: &str,
        credentials: Option<PisteCredentials>,
    ) -> PisteConfig {
        config_with_agent(env, oauth_url, credentials, "lf-test/0".to_owned())
    }

    /// Comme `config_with`, avec un `User-Agent` explicite.
    fn config_with_agent(
        env: PisteEnvironment,
        oauth_url: &str,
        credentials: Option<PisteCredentials>,
        user_agent: String,
    ) -> PisteConfig {
        let (endpoints, _) =
            PisteEndpoints::with_overrides(env, Some(oauth_url), Some(UNUSED_API)).unwrap();
        PisteConfig::new(env, Ok(endpoints), credentials, user_agent)
    }

    fn config_with_secret(uri: &str, secret: &str) -> PisteConfig {
        config_with(SANDBOX, &format!("{uri}/oauth/token"), Some(credentials(secret)))
    }

    fn config_for(uri: &str) -> PisteConfig {
        config_with_secret(uri, CLIENT_SECRET)
    }

    /// Gestionnaire sur horloge manuelle, avec ce qu'il faut pour observer le temps.
    struct Fixture {
        manager: TokenManager<ManualClock, RecordingSleeper>,
        clock: ManualClock,
        sleeper: RecordingSleeper,
        oauth_url: String,
    }

    impl Fixture {
        fn with_config(config: PisteConfig) -> Self {
            let oauth_url =
                config.endpoints().map(|e| e.oauth_url().to_string()).unwrap_or_default();
            let clock = ManualClock::new();
            let sleeper = RecordingSleeper::new(clock.clone());
            let manager = TokenManager::new(
                Arc::new(SharedClient::new()),
                Arc::new(config),
                clock.clone(),
                sleeper.clone(),
            );
            Self { manager, clock, sleeper, oauth_url }
        }

        fn on(server: &MockServer) -> Self {
            Self::with_config(config_for(&server.uri()))
        }

        /// Échéance de 60 s à partir de l'instant courant.
        fn deadline(&self) -> CallDeadline {
            CallDeadline::start(&self.clock, secs(60))
        }

        /// Place l'horloge à `instant` (elle ne recule jamais).
        fn at(&self, instant: Duration) {
            self.clock.advance(instant.saturating_sub(self.clock.now()));
        }

        async fn token(&self) -> Result<TokenLease, CorpusError> {
            self.manager.token(&self.deadline()).await
        }

        /// Valeur et génération du jeton obtenu à `instant`.
        async fn lease_at(&self, instant: Duration) -> (String, u64) {
            self.at(instant);
            let lease = self.token().await.unwrap();
            (lease.value.expose_secret().to_owned(), lease.generation)
        }
    }

    async fn reply(server: &MockServer, template: ResponseTemplate, times: u64) {
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(template)
            .up_to_n_times(times)
            .expect(times)
            .mount(server)
            .await;
    }

    async fn reply_always(server: &MockServer, template: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(template)
            .mount(server)
            .await;
    }

    async fn requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    fn json_reply(status: u16, body: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_raw(body, "application/json")
    }

    fn body(token: &str, expires_in: &Value) -> String {
        json!({
            "access_token": token,
            "token_type": "Bearer",
            "expires_in": expires_in,
            "scope": "openid",
        })
        .to_string()
    }

    fn ok(token: &str, expires_in: &Value) -> ResponseTemplate {
        json_reply(200, &body(token, expires_in))
    }

    /// Réponse d'échec rejouée à volonté : l'erreur, le nombre de requêtes et le gestionnaire.
    async fn failure_with(
        secret: &str,
        template: ResponseTemplate,
    ) -> (CorpusError, usize, Fixture) {
        let server = MockServer::start().await;
        reply_always(&server, template).await;
        let fixture = Fixture::with_config(config_with_secret(&server.uri(), secret));
        let error = fixture.token().await.unwrap_err();
        (error, requests(&server).await, fixture)
    }

    async fn failure(template: ResponseTemplate) -> (CorpusError, usize, Fixture) {
        failure_with(CLIENT_SECRET, template).await
    }

    /// Nombre cumulé de requêtes après chaque `token()` demandé aux `instants`.
    async fn requests_at(text: &str, instants: &[Duration]) -> Vec<usize> {
        let server = MockServer::start().await;
        reply_always(&server, json_reply(200, text)).await;
        let fixture = Fixture::on(&server);
        let mut counts = Vec::new();
        for instant in instants {
            fixture.at(*instant);
            fixture.token().await.unwrap();
            counts.push(requests(&server).await);
        }
        counts
    }

    /// Journal `tracing` capté en mémoire.
    #[derive(Clone, Default)]
    struct Capture {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl Capture {
        fn text(&self) -> String {
            io::Write::flush(&mut self.clone()).unwrap();
            String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
        }
    }

    impl io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl MakeWriter<'_> for Capture {
        type Writer = Capture;

        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    /// Abonné local au thread courant (le runtime des tests est mono-fil).
    fn capture_at(level: tracing::Level) -> (Capture, tracing::subscriber::DefaultGuard) {
        let capture = Capture::default();
        let subscriber =
            tracing_subscriber::fmt().with_writer(capture.clone()).with_max_level(level).finish();
        (capture, tracing::subscriber::set_default(subscriber))
    }

    // ---- Cycle de vie du jeton ------------------------------------------------------

    #[tokio::test]
    async fn oauth_success_form_body_and_token_reuse() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(header("content-type", "application/x-www-form-urlencoded"))
            .and(header("accept", "application/json"))
            .and(body_string(FORM_BODY))
            .respond_with(ok("tok", &json!(3600)))
            .expect(1)
            .mount(&server)
            .await;
        let fixture = Fixture::on(&server);
        let mut seen = Vec::new();
        for _ in 0..5 {
            let lease = fixture.token().await.unwrap();
            seen.push((lease.value.expose_secret().to_owned(), lease.generation));
        }
        assert_eq!(seen, vec![("tok".to_owned(), 1); 5]);
        assert_eq!((fixture.sleeper.waits(), requests(&server).await), (vec![], 1));
    }

    #[tokio::test]
    async fn oauth_token_refreshed_60s_before_expiry() {
        let server = MockServer::start().await;
        reply(&server, ok("first", &json!(3600)), 1).await;
        reply(&server, ok("second", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let seen = [
            fixture.lease_at(Duration::ZERO).await,
            fixture.lease_at(secs(3539)).await,
            fixture.lease_at(secs(3541)).await,
        ];
        assert_eq!(
            seen,
            [("first".to_owned(), 1), ("first".to_owned(), 1), ("second".to_owned(), 2)]
        );
    }

    #[tokio::test]
    async fn refresh_happens_exactly_at_the_refresh_instant() {
        let at = secs(3540);
        let counts =
            requests_at(&body("tok", &json!(3600)), &[Duration::ZERO, before(at), at, at]).await;
        assert_eq!(counts, [1, 1, 2, 2]);
    }

    #[tokio::test]
    async fn oauth_expires_in_tiny_uses_half_margin() {
        let at = secs(15);
        let counts = requests_at(&body("tok", &json!(30)), &[Duration::ZERO, before(at), at]).await;
        assert_eq!(counts, [1, 1, 2]);
    }

    #[tokio::test]
    async fn the_margin_is_half_the_lifetime_up_to_60_seconds() {
        let mut seen = Vec::new();
        for (expires_in, refresh) in [(100, secs(50)), (120, secs(60)), (1, ms(500))] {
            let text = body("tok", &json!(expires_in));
            seen.push(requests_at(&text, &[Duration::ZERO, before(refresh), refresh]).await);
        }
        assert_eq!(seen, vec![vec![1, 1, 2]; 3]);
    }

    #[tokio::test]
    async fn oauth_expires_in_null_uses_default_3600() {
        let bodies = [
            body("tok", &Value::Null),
            body("tok", &json!("3600")),
            body("tok", &json!("soon")),
            r#"{"access_token":"tok"}"#.to_owned(),
        ];
        let at = secs(3540);
        let mut seen = Vec::new();
        for text in &bodies {
            seen.push(requests_at(text, &[Duration::ZERO, before(at), at]).await);
        }
        assert_eq!(seen, vec![vec![1, 1, 2]; 4]);
    }

    #[tokio::test]
    async fn oauth_expires_in_is_clamped() {
        let default = secs(3540);
        let capped = secs(86_340);
        let mut seen = Vec::new();
        for (expires_in, refresh) in
            [(0, default), (1_000_000_000, capped), (86_401, capped), (86_400, capped)]
        {
            let text = body("tok", &json!(expires_in));
            seen.push(requests_at(&text, &[Duration::ZERO, before(refresh), refresh]).await);
        }
        assert_eq!(seen, vec![vec![1, 1, 2]; 4]);
    }

    #[test]
    fn lifetime_and_refresh_instant_are_pure_functions() {
        assert_eq!(
            [None, Some(0), Some(1), Some(86_401), Some(u64::MAX)].map(lifetime_of),
            [secs(3600), secs(3600), secs(1), secs(86_400), secs(86_400)]
        );
        assert_eq!(
            (
                refresh_at(secs(10), secs(3600)),
                refresh_at(secs(10), secs(30)),
                refresh_at(Duration::MAX, secs(3600)),
            ),
            (secs(3550), secs(25), Duration::MAX.saturating_sub(secs(60)))
        );
    }

    #[tokio::test]
    async fn token_lifetime_counts_from_before_the_first_attempt() {
        let server = MockServer::start().await;
        reply(&server, ResponseTemplate::new(503), 1).await;
        reply(&server, ok("tok", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let first = fixture.token().await.unwrap();
        // L'attente de réessai (500 ms à 1 s) a fait avancer l'horloge ; l'instant d'obtention
        // reste celui d'avant la première tentative.
        let waited = fixture.clock.now();
        assert!((ms(500)..secs(1)).contains(&waited), "{waited:?}");
        let before_refresh = fixture.lease_at(before(secs(3540))).await;
        let count = requests(&server).await;
        // Aucun mock ne reste : la troisième requête reçoit un 404, preuve du renouvellement.
        fixture.at(secs(3540));
        let refresh = fixture.token().await.unwrap_err();
        assert_eq!(
            (first.generation, before_refresh, count, refresh, requests(&server).await),
            (
                1,
                ("tok".to_owned(), 1),
                2,
                CorpusError::Config {
                    detail: format!(
                        "PISTE OAuth endpoint answered HTTP 404 at {}",
                        fixture.oauth_url
                    )
                },
                3
            )
        );
    }

    // ---- Mono-vol et annulation -----------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_calls_share_one_token_refresh() {
        let server = MockServer::start().await;
        reply(&server, ok("shared", &json!(3600)).set_delay(ms(100)), 1).await;
        let clock = TokioClock::new();
        let manager = Arc::new(TokenManager::new(
            Arc::new(SharedClient::new()),
            Arc::new(config_for(&server.uri())),
            clock,
            TokioSleeper,
        ));
        let mut tasks = JoinSet::new();
        for _ in 0..10 {
            let manager = Arc::clone(&manager);
            tasks.spawn(async move {
                let deadline = CallDeadline::start(&clock, secs(30));
                manager.token(&deadline).await.unwrap().generation
            });
        }
        let generations = tasks.join_all().await;
        assert_eq!((generations, requests(&server).await), (vec![1; 10], 1));
    }

    #[tokio::test]
    async fn invalidate_with_current_generation_forces_refresh() {
        let server = MockServer::start().await;
        reply(&server, ok("old", &json!(3600)), 1).await;
        reply(&server, ok("new", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let old = fixture.lease_at(Duration::ZERO).await;
        fixture.manager.invalidate(1, &fixture.deadline()).await.unwrap();
        let new = fixture.lease_at(Duration::ZERO).await;
        assert_eq!(
            (old, new, requests(&server).await),
            (("old".to_owned(), 1), ("new".to_owned(), 2), 2)
        );
    }

    #[tokio::test]
    async fn invalidate_does_not_drop_fresher_token() {
        let server = MockServer::start().await;
        reply(&server, ok("old", &json!(3600)), 1).await;
        reply(&server, ok("new", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        fixture.lease_at(Duration::ZERO).await;
        fixture.manager.invalidate(1, &fixture.deadline()).await.unwrap();
        let new = fixture.lease_at(Duration::ZERO).await;
        fixture.manager.invalidate(1, &fixture.deadline()).await.unwrap();
        fixture.manager.invalidate(new.1 + 1, &fixture.deadline()).await.unwrap();
        let kept = fixture.lease_at(Duration::ZERO).await;
        assert_eq!(
            (new, kept, requests(&server).await),
            (("new".to_owned(), 2), ("new".to_owned(), 2), 2)
        );
    }

    #[tokio::test]
    async fn invalidate_without_a_token_does_nothing() {
        let server = MockServer::start().await;
        reply(&server, ok("tok", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        fixture.manager.invalidate(0, &fixture.deadline()).await.unwrap();
        fixture.manager.invalidate(1, &fixture.deadline()).await.unwrap();
        let issued_before = fixture.manager.state.try_lock().unwrap().issued;
        let lease = fixture.lease_at(Duration::ZERO).await;
        assert_eq!((issued_before, lease), (0, ("tok".to_owned(), 1)));
    }

    #[tokio::test]
    async fn generations_increase_with_each_token() {
        let server = MockServer::start().await;
        reply_always(&server, ok("tok", &json!(3600))).await;
        let fixture = Fixture::on(&server);
        let mut generations = Vec::new();
        for instant in [Duration::ZERO, secs(3540), secs(7080)] {
            generations.push(fixture.lease_at(instant).await.1);
        }
        assert_eq!(generations, [1, 2, 3]);
    }

    #[tokio::test]
    async fn cancel_during_refresh_releases_lock() {
        let server = MockServer::start().await;
        // Le bouchon lent signale l'arrivée de la requête : on coupe le renouvellement à ce
        // moment-là, et non après un délai fixe. Un délai englobant la construction du client
        // (premier chargement des racines, lent sous charge) pouvait expirer avant l'envoi,
        // laissant le bouchon lent à la requête suivante. Le renouvellement lent dure 5 s : s'il
        // finissait avant le signal, la branche du jeton l'emporterait et l'assertion échouerait
        // au lieu de pendre.
        let arrived = Arc::new(tokio::sync::Notify::new());
        let signal = Arc::clone(&arrived);
        let slow = ok("slow", &json!(3600)).set_delay(secs(5));
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(move |_: &wiremock::Request| {
                signal.notify_one();
                slow.clone()
            })
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        reply(&server, ok("fast", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let mut cut = pin!(arrived.notified());
        let outcome = race(fixture.token(), cut.as_mut()).await;
        assert!(outcome.is_none(), "the slow refresh must be cut once its request has arrived");
        let intact = {
            let state = fixture.manager.state.try_lock().unwrap();
            (state.cached.is_none(), state.issued)
        };
        let lease = fixture.lease_at(Duration::ZERO).await;
        assert_eq!((intact, lease), ((true, 0), ("fast".to_owned(), 1)));
    }

    #[tokio::test]
    async fn lock_wait_is_bounded_by_the_deadline() {
        let fixture = Fixture::with_config(config_for(UNUSED_URI));
        let guard = fixture.manager.state.lock().await;
        let error = fixture.token().await.unwrap_err();
        drop(guard);
        assert_eq!((error, fixture.sleeper.waits()), (CorpusError::Timeout, vec![secs(60)]));
    }

    #[tokio::test]
    async fn invalidate_wait_is_bounded_by_the_deadline() {
        let server = MockServer::start().await;
        reply(&server, ok("tok", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let first = fixture.lease_at(Duration::ZERO).await;
        let guard = fixture.manager.state.lock().await;
        let short = CallDeadline::start(&fixture.clock, ms(200));
        let outcome = fixture.manager.invalidate(first.1, &short).await;
        drop(guard);
        let waits = fixture.sleeper.waits();
        // l'état est intact : le jeton refusé est resté en mémoire, sans nouvelle requête
        let kept = fixture.lease_at(Duration::ZERO).await;
        assert_eq!(
            (outcome, waits, kept, requests(&server).await),
            (Err(CorpusError::Timeout), vec![ms(200)], ("tok".to_owned(), 1), 1)
        );
    }

    #[tokio::test]
    async fn free_lock_is_taken_without_waiting_for_the_deadline() {
        let server = MockServer::start().await;
        reply(&server, ok("tok", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let deadline = fixture.deadline();
        let first = fixture.manager.token(&deadline).await.unwrap().generation;
        fixture.clock.advance(secs(120));
        let again = fixture.manager.token(&deadline).await.unwrap().generation;
        assert_eq!(
            (first, again, fixture.sleeper.waits(), requests(&server).await),
            (1, 1, vec![], 1)
        );
    }

    #[tokio::test]
    async fn expired_deadline_times_out_before_any_request() {
        let server = MockServer::start().await;
        let fixture = Fixture::on(&server);
        let deadline = fixture.deadline();
        fixture.clock.advance(secs(61));
        let error = fixture.manager.token(&deadline).await.unwrap_err();
        assert_eq!((error, requests(&server).await), (CorpusError::Timeout, 0));
    }

    #[tokio::test]
    async fn refresh_is_bounded_by_the_call_deadline() {
        let server = MockServer::start().await;
        reply_always(&server, ok("tok", &json!(3600)).set_delay(secs(2))).await;
        let fixture = Fixture::on(&server);
        let deadline = CallDeadline::start(&fixture.clock, ms(100));
        let started = Instant::now();
        let error = fixture.manager.token(&deadline).await.unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!((error, requests(&server).await), (CorpusError::Timeout, 1));
        assert!(elapsed < secs(1), "{elapsed:?}");
    }

    // ---- Classification des erreurs de l'endpoint token (04 §3.5) --------------------

    const INVALID_CLIENT: &str = r#"{"error":"invalid_client","error_description":"Client authentication failed: client_secret=my-client-secret"}"#;

    fn rejected(status: u16, code: Option<&str>) -> CorpusError {
        let quoted = code.map(|code| format!(", error {code}")).unwrap_or_default();
        CorpusError::Auth {
            detail: format!(
                "OAuth token request rejected: HTTP {status}{quoted}, environment sandbox"
            ),
        }
    }

    #[tokio::test]
    async fn oauth_invalid_client_401_maps_to_auth_no_retry() {
        for status in [401, 400] {
            let (error, count, fixture) = failure(json_reply(status, INVALID_CLIENT)).await;
            let shown = format!("{error} {error:?}");
            assert_eq!(
                (&error, count, fixture.sleeper.waits()),
                (&rejected(status, Some("invalid_client")), 1, vec![])
            );
            for forbidden in ["Client authentication", CLIENT_SECRET, "error_description"] {
                assert!(!shown.contains(forbidden), "{shown}");
            }
        }
    }

    #[tokio::test]
    async fn rejection_names_the_production_environment() {
        let server = MockServer::start().await;
        reply_always(&server, json_reply(401, INVALID_CLIENT)).await;
        let config = config_with(
            PisteEnvironment::Production,
            &format!("{}/oauth/token", server.uri()),
            Some(credentials(CLIENT_SECRET)),
        );
        let error = Fixture::with_config(config).token().await.unwrap_err();
        assert_eq!(
            error,
            CorpusError::Auth {
                detail: "OAuth token request rejected: HTTP 401, error invalid_client, environment production".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn oauth_400_maps_to_auth() {
        let (error, count, _) = failure(json_reply(400, r#"{"error":"invalid_grant"}"#)).await;
        assert_eq!((error, count), (rejected(400, Some("invalid_grant")), 1));
    }

    #[tokio::test]
    async fn oauth_403_maps_to_auth() {
        let (error, count, fixture) =
            failure(ResponseTemplate::new(403).set_body_string("denied")).await;
        assert_eq!(
            (error, count, fixture.sleeper.waits()),
            (
                CorpusError::Auth {
                    detail: "OAuth token request rejected: HTTP 403, environment sandbox"
                        .to_owned()
                },
                1,
                vec![]
            )
        );
    }

    #[test]
    fn only_the_six_rfc_6749_error_codes_are_quoted() {
        let url = Url::parse("http://127.0.0.1:9/oauth/token").unwrap();
        let reject = |body: &str| {
            let response = RawResponse {
                status: 401,
                retry_after: None,
                date: None,
                body: body.as_bytes().to_vec(),
            };
            status_error(&response, SANDBOX, &url)
        };
        let coded = |code: &str| json!({ "error": code }).to_string();
        let short_but_unknown = "a".repeat(64);
        let too_long = "a".repeat(65);
        let mut cases = vec![
            (coded(&short_but_unknown), None),
            (coded(&too_long), None),
            (coded(CLIENT_SECRET), None),
            (coded(CLIENT_ID), None),
            (coded("123e4567-e89b-12d3-a456-426614174000"), None),
            (coded("tok-visible-if-leaked"), None),
            (coded(""), None),
            (coded("é"), None),
            (coded("two words"), None),
            (coded("tab\there"), None),
            (coded("Invalid_Client"), None),
            (coded("invalid_client "), None),
            (coded("invalid_clients"), None),
            (coded("invalid"), None),
            ("{}".to_owned(), None),
            ("<html>".to_owned(), None),
            ("[]".to_owned(), None),
        ];
        for code in [
            "invalid_request",
            "invalid_client",
            "invalid_grant",
            "unauthorized_client",
            "unsupported_grant_type",
            "invalid_scope",
        ] {
            cases.push((coded(code), Some(code)));
        }
        for (text, code) in cases {
            assert_eq!(reject(&text), Some(rejected(401, code)), "{text}");
        }
    }

    #[tokio::test]
    async fn oauth_404_maps_to_config_not_not_found() {
        for status in [404, 405] {
            let (error, count, fixture) = failure(ResponseTemplate::new(status)).await;
            assert_eq!(
                (error, count),
                (
                    CorpusError::Config {
                        detail: format!(
                            "PISTE OAuth endpoint answered HTTP {status} at {}",
                            fixture.oauth_url
                        )
                    },
                    1
                )
            );
        }
    }

    #[test]
    fn location_drops_credentials_query_and_fragment() {
        let full =
            Url::parse("https://user:pw@auth.example.org:8443/api/oauth/token?a=b#frag").unwrap();
        let opaque = Url::parse("mailto:someone@example.org").unwrap();
        assert_eq!(
            (location(&full), location(&opaque)),
            (
                "https://auth.example.org:8443/api/oauth/token".to_owned(),
                "mailto:someone@example.org".to_owned()
            )
        );
    }

    #[tokio::test]
    async fn oauth_html_page_maps_to_config() {
        let page =
            ResponseTemplate::new(200).set_body_raw("<html><body>Login</body></html>", "text/html");
        let (error, count, fixture) = failure(page).await;
        assert_eq!(
            (error, count),
            (
                CorpusError::Config {
                    detail: format!(
                        "PISTE OAuth response is not valid JSON at {}",
                        fixture.oauth_url
                    )
                },
                1
            )
        );
    }

    #[tokio::test]
    async fn oauth_200_without_access_token_maps_to_config() {
        let no_token =
            CorpusError::Config { detail: "PISTE OAuth returned no access_token".to_owned() };
        for text in
            ["{}", r#"{"access_token":null}"#, r#"{"token_type":"Bearer","expires_in":3600}"#]
        {
            let (error, count, _) = failure(json_reply(200, text)).await;
            assert_eq!((error, count), (no_token.clone(), 1), "{text}");
        }
    }

    #[tokio::test]
    async fn oauth_200_empty_access_token_maps_to_config() {
        let (error, count, _) = failure(json_reply(200, r#"{"access_token":""}"#)).await;
        assert_eq!(
            (error, count),
            (CorpusError::Config { detail: "PISTE OAuth returned no access_token".to_owned() }, 1)
        );
    }

    #[tokio::test]
    async fn an_access_token_that_is_not_a_header_value_is_unusable_and_never_cached() {
        let unusable = CorpusError::Config {
            detail: "PISTE OAuth returned an unusable access_token".to_owned(),
        };
        for token in
            ["a\nb", "a\r\nX-Injected: 1", "a\u{0}b", "tok\u{7f}", "tök", "tok\u{e9}n", "日本"]
        {
            let body = json!({ "access_token": token, "expires_in": 3600 }).to_string();
            // Le serveur reste vivant pendant tout le tour : il sert aussi la seconde demande.
            let server = MockServer::start().await;
            reply_always(&server, json_reply(200, &body)).await;
            let fixture = Fixture::on(&server);
            let error = fixture.token().await.unwrap_err();
            assert_eq!((&error, requests(&server).await), (&unusable, 1), "{token:?}");
            assert!(!format!("{error} {error:?}").contains(token), "{token:?}");
            assert!(fixture.manager.state.lock().await.cached.is_none(), "{token:?}");
            // Jamais mis en cache : la demande suivante interroge de nouveau l'endpoint.
            assert_eq!(fixture.token().await.unwrap_err(), unusable, "{token:?}");
            assert_eq!(requests(&server).await, 2, "{token:?}");
        }
    }

    #[tokio::test]
    async fn an_access_token_with_spaces_and_punctuation_is_still_usable() {
        let server = MockServer::start().await;
        reply_always(&server, ok("a b\tc=.~+/-_", &json!(3600))).await;
        let fixture = Fixture::on(&server);
        assert_eq!(fixture.token().await.unwrap().value.expose_secret(), "a b\tc=.~+/-_");
    }

    #[tokio::test]
    async fn oauth_200_unreadable_json_maps_to_config() {
        for text in ["not json", "", "[1,2,3]", r#"{"access_token":42}"#] {
            let (error, count, fixture) = failure(json_reply(200, text)).await;
            assert_eq!(
                (error, count),
                (
                    CorpusError::Config {
                        detail: format!(
                            "PISTE OAuth response is not valid JSON at {}",
                            fixture.oauth_url
                        )
                    },
                    1
                ),
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn oauth_503_is_retried() {
        let server = MockServer::start().await;
        reply(&server, ResponseTemplate::new(503), 1).await;
        reply(&server, ok("tok", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let lease = fixture.lease_at(Duration::ZERO).await;
        let waits = fixture.sleeper.waits();
        assert_eq!((lease, requests(&server).await, waits.len()), (("tok".to_owned(), 1), 2, 1));
        assert!((ms(500)..secs(1)).contains(&waits[0]), "{waits:?}");
    }

    #[tokio::test]
    async fn oauth_429_three_times_maps_to_rate_limited() {
        let (error, count, fixture) = failure(ResponseTemplate::new(429)).await;
        assert_eq!(
            (error, count, fixture.sleeper.waits().len()),
            (CorpusError::RateLimited { retry_after: None }, 3, 2)
        );
    }

    #[tokio::test]
    async fn oauth_429_honours_retry_after() {
        let server = MockServer::start().await;
        reply(&server, ResponseTemplate::new(429).insert_header("Retry-After", "3"), 1).await;
        reply(&server, ok("tok", &json!(3600)), 1).await;
        let fixture = Fixture::on(&server);
        let lease = fixture.lease_at(Duration::ZERO).await;
        assert_eq!((lease, fixture.sleeper.waits()), (("tok".to_owned(), 1), vec![secs(3)]));
    }

    #[tokio::test]
    async fn oauth_gateway_errors_are_retried_then_upstream() {
        for status in [502, 503, 504] {
            let (error, count, _) = failure(ResponseTemplate::new(status)).await;
            assert_eq!(
                (error, count),
                (
                    CorpusError::Upstream {
                        status: Some(status),
                        retryable: true,
                        detail: format!("HTTP {status} on PISTE OAuth"),
                    },
                    3
                )
            );
        }
    }

    #[tokio::test]
    async fn oauth_500_twice_maps_to_upstream_not_retryable() {
        let (error, count, fixture) = failure(ResponseTemplate::new(500)).await;
        assert_eq!(
            (error, count, fixture.sleeper.waits().len()),
            (
                CorpusError::Upstream {
                    status: Some(500),
                    retryable: false,
                    detail: "HTTP 500 on PISTE OAuth".to_owned(),
                },
                2,
                1
            )
        );
    }

    #[tokio::test]
    async fn oauth_network_error_maps_to_upstream() {
        let closed = ClosedPort::new();
        let fixture = Fixture::with_config(config_for(&closed.uri()));
        let error = fixture.token().await.unwrap_err();
        assert_eq!(
            (error, fixture.sleeper.waits().len()),
            (
                CorpusError::Upstream {
                    status: None,
                    retryable: true,
                    detail: "network error (connect) on PISTE OAuth".to_owned(),
                },
                2
            )
        );
    }

    #[tokio::test]
    async fn oauth_timeout_maps_to_timeout_after_three_attempts() {
        let server = MockServer::start().await;
        reply_always(&server, ok("tok", &json!(3600)).set_delay(secs(1))).await;
        let config = config_for(&server.uri())
            .with_tuning(PisteTuning { request_timeout: ms(100), ..PisteTuning::default() })
            .unwrap();
        let error = Fixture::with_config(config).token().await.unwrap_err();
        assert_eq!((error, requests(&server).await), (CorpusError::Timeout, 3));
    }

    #[tokio::test]
    async fn oauth_tls_failure_maps_to_upstream_not_retryable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer).await;
            let _ =
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await;
            let _ = stream.shutdown().await;
        });
        let config = config_with(
            SANDBOX,
            &format!("https://127.0.0.1:{port}/oauth/token"),
            Some(credentials(CLIENT_SECRET)),
        );
        let fixture = Fixture::with_config(config);
        let error = fixture.token().await.unwrap_err();
        assert_eq!(
            (error, fixture.sleeper.waits()),
            (
                CorpusError::Upstream {
                    status: None,
                    retryable: false,
                    detail: "TLS handshake failed on PISTE OAuth".to_owned(),
                },
                vec![]
            )
        );
    }

    #[tokio::test]
    async fn other_statuses_follow_the_api_classification() {
        for status in [302, 418] {
            let (error, count, _) = failure(ResponseTemplate::new(status)).await;
            assert_eq!(
                (error, count),
                (
                    CorpusError::Upstream {
                        status: Some(status),
                        retryable: false,
                        detail: format!("HTTP {status} on PISTE OAuth"),
                    },
                    1
                )
            );
        }
    }

    #[test]
    fn classified_outcomes_are_kept_except_success() {
        let url = Url::parse("http://127.0.0.1:9/oauth/token").unwrap();
        let error = CorpusError::Auth { detail: "d".to_owned() };
        let transient = Outcome::Transient {
            error: error.clone(),
            retry_after: Some(secs(3)),
            budget: Budget::Reduced,
        };
        let success = RawResponse {
            status: 200,
            retry_after: None,
            date: None,
            body: br#"{"access_token":"tok-secret","expires_in":900}"#.to_vec(),
        };
        let shown = [
            from_classified(Outcome::Unauthorized(error.clone()), &url),
            from_classified(Outcome::Fatal(error.clone()), &url),
            from_classified(transient, &url),
            from_classified(Outcome::Success(success), &url),
        ]
        .map(|outcome| format!("{outcome:?}"));
        assert_eq!(
            shown,
            [
                r#"Fatal(Auth { detail: "d" })"#,
                r#"Fatal(Auth { detail: "d" })"#,
                r#"Transient { error: Auth { detail: "d" }, retry_after: Some(3s), budget: Reduced }"#,
                "Success(Grant { value: SecretBox<str>([REDACTED]), lifetime: 900s })",
            ]
        );
    }

    // ---- Mode dégradé : aucune E/S ---------------------------------------------------

    #[tokio::test]
    async fn a_client_that_cannot_be_built_is_a_config_error_without_io() {
        let server = MockServer::start().await;
        let config = config_with_agent(
            SANDBOX,
            &format!("{}/oauth/token", server.uri()),
            Some(credentials(CLIENT_SECRET)),
            "bad\nagent".to_owned(),
        );
        let error = Fixture::with_config(config).token().await.unwrap_err();
        assert_eq!(
            (error, requests(&server).await),
            (
                CorpusError::Config {
                    detail: "could not build the HTTP client: builder error: failed to parse header value".to_owned()
                },
                0
            )
        );
    }

    #[tokio::test]
    async fn missing_credentials_are_a_config_error_without_io() {
        let server = MockServer::start().await;
        let config = config_with(SANDBOX, &format!("{}/oauth/token", server.uri()), None);
        let error = Fixture::with_config(config).token().await.unwrap_err();
        assert_eq!(
            (error, requests(&server).await),
            (CorpusError::Config { detail: "PISTE credentials missing".to_owned() }, 0)
        );
    }

    #[tokio::test]
    async fn invalid_endpoints_are_a_config_error_without_io() {
        let problem =
            PisteEndpoints::with_overrides(SANDBOX, Some("http://example.org/token"), None)
                .unwrap_err();
        let config = PisteConfig::new(
            SANDBOX,
            Err(problem.clone()),
            Some(credentials(CLIENT_SECRET)),
            "lf-test/0".to_owned(),
        );
        let fixture = Fixture::with_config(config);
        let error = fixture.token().await.unwrap_err();
        assert_eq!(
            (error, fixture.sleeper.waits()),
            (CorpusError::Config { detail: problem.to_string() }, vec![])
        );
    }

    // ---- Secrets (D-40) --------------------------------------------------------------

    fn grant_body(token_type: &str) -> String {
        format!(r#"{{"access_token":"tok-mac","token_type":{token_type},"expires_in":3600}}"#)
    }

    #[tokio::test]
    async fn non_bearer_token_type_warns_and_is_used() {
        let (capture, _guard) = capture_at(tracing::Level::WARN);
        let server = MockServer::start().await;
        reply_always(&server, json_reply(200, &grant_body(r#""MAC""#))).await;
        let fixture = Fixture::on(&server);
        let lease = fixture.lease_at(Duration::ZERO).await;
        let logs = capture.text();
        assert_eq!(lease, ("tok-mac".to_owned(), 1));
        assert!(logs.contains("WARN"), "{logs}");
        assert!(logs.contains("token_type is not Bearer"), "{logs}");
        assert!(!logs.contains("tok-mac"), "{logs}");
        assert!(!logs.contains("MAC"), "{logs}");
    }

    #[tokio::test]
    async fn bearer_in_any_case_or_absent_does_not_warn() {
        let (capture, _guard) = capture_at(tracing::Level::WARN);
        let bodies = [
            grant_body(r#""Bearer""#),
            grant_body(r#""bearer""#),
            grant_body(r#""BEARER""#),
            grant_body("null"),
            r#"{"access_token":"tok-mac"}"#.to_owned(),
        ];
        for text in &bodies {
            let server = MockServer::start().await;
            reply_always(&server, json_reply(200, text)).await;
            let fixture = Fixture::on(&server);
            assert_eq!(fixture.lease_at(Duration::ZERO).await, ("tok-mac".to_owned(), 1), "{text}");
        }
        let logs = capture.text();
        assert!(!logs.contains("token_type"), "{logs}");
    }

    #[tokio::test]
    async fn missing_token_does_not_warn_about_its_type() {
        let (capture, _guard) = capture_at(tracing::Level::WARN);
        let (error, _, _) = failure(json_reply(200, r#"{"token_type":"MAC"}"#)).await;
        let logs = capture.text();
        assert_eq!(
            error,
            CorpusError::Config { detail: "PISTE OAuth returned no access_token".to_owned() }
        );
        assert!(!logs.contains("token_type"), "{logs}");
    }

    #[test]
    fn token_lease_debug_hides_secret() {
        let lease =
            TokenLease { value: SecretString::from("tok-visible-if-leaked"), generation: 4 };
        let cached = CachedToken {
            value: SecretString::from("tok-visible-if-leaked"),
            refresh_at: secs(5),
            generation: 7,
        };
        let grant = Grant { value: SecretString::from("tok-visible-if-leaked"), lifetime: secs(9) };
        let shown = [format!("{lease:?}"), format!("{cached:?}"), format!("{grant:?}")];
        for (text, generation) in
            shown.iter().zip(["generation: 4", "generation: 7", "lifetime: 9s"])
        {
            assert!(text.contains("REDACTED") && text.contains(generation), "{text}");
            assert!(!text.contains("tok-visible-if-leaked"), "{text}");
        }
    }

    #[tokio::test]
    async fn error_detail_never_contains_secret_or_token() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let secret = format!("s3cr3t-{nanos:x}");
        let token = format!("t0k3n-{nanos:x}");
        let leak = format!("{secret} {token}");
        let described = json!({ "error": "invalid_client", "error_description": leak }).to_string();
        let text = |status: u16| ResponseTemplate::new(status).set_body_string(leak.clone());
        let as_error = |value: &str| json!({ "error": value }).to_string();
        let replies = vec![
            json_reply(401, &as_error(&secret)),
            json_reply(400, &as_error(&token)),
            json_reply(403, &as_error(CLIENT_ID)),
            json_reply(401, &as_error("123e4567-e89b-12d3-a456-426614174000")),
            json_reply(401, &described),
            json_reply(400, &described),
            text(403),
            text(404),
            text(405),
            ResponseTemplate::new(200).set_body_raw(format!("<html>{leak}</html>"), "text/html"),
            json_reply(
                200,
                &json!({ "token_type": "Bearer", "error_description": leak }).to_string(),
            ),
            json_reply(200, &json!({ "access_token": 42, "echo": leak }).to_string()),
            text(429),
            text(500),
            text(503),
            text(418),
        ];
        let mut shown = Vec::new();
        for template in replies {
            let (error, _, _) = failure_with(&secret, template).await;
            shown.push(format!("{error} {error:?}"));
        }
        let server = MockServer::start().await;
        reply_always(&server, ok(&token, &json!(3600))).await;
        let fixture = Fixture::with_config(config_with_secret(&server.uri(), &secret));
        let lease = fixture.token().await.unwrap();
        shown.push(format!("{lease:?}"));
        shown.push(format!("{:?}", fixture.manager.state.lock().await));
        shown.push(capture.text());
        for text in &shown {
            assert!(!text.contains(&secret), "{text}");
            assert!(!text.contains(&token), "{text}");
            assert!(!text.contains(CLIENT_ID), "{text}");
        }
        assert_eq!(shown.len(), 19);
    }
}
