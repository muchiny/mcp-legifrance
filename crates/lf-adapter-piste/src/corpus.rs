//! `PisteCorpus` : assemblage de l'adapter et appel de l'API Légifrance de bout en bout
//! (04 §1.3, §2.4, §3.4, §5, T-M2-02 à T-M2-04).
//!
//! Un appel suit toujours le même chemin : vérification de la configuration (mode dégradé,
//! sans la moindre E/S), puis, à chaque tentative, client HTTP, barrière de charge, jeton,
//! envoi borné et classification ; la boucle de réessai rejoue la tentative, et seul le corps
//! d'une réponse 2xx est décodé (celui d'un 400 ou d'un 404 est lu par `classify`, borné et
//! expurgé). Le permis de concurrence de la tentative réussie n'est rendu qu'après le décodage et
//! la lecture de ce corps (00 §11.1). Rien n'est lu dans l'environnement et rien n'est ouvert à
//! la construction : `PisteCorpus::new` ne décide de rien.
//!
//! Ni la tentative (`Attempt`), ni un corps de requête ou de réponse, ni le secret, ni le jeton
//! ne sont jamais journalisés ni recopiés dans une erreur. Seule exception : le message d'erreur
//! PISTE d'un 400, nettoyé, borné à 200 caractères et privé du jeton et du secret client, qui
//! devient le détail de `InvalidRequest`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use lf_application::ports::CorpusError;
use lf_domain::coverage::PisteEnvironment;
use reqwest::Url;
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;

use crate::auth::{TokenLease, TokenManager};
use crate::classify::{EndpointKind, ErrorContext, Outcome, try_send};
use crate::config::{ConfigProblem, PisteConfig, PisteEndpoints};
use crate::deadline::CallDeadline;
use crate::dto::decode::decode;
use crate::encoding::decode_body;
use crate::http::{Attempt, Payload, RawResponse, SharedClient};
use crate::limit::{Permit, RateGate};
use crate::map::Diagnostics;
use crate::memo::PaginationMemo;
use crate::retry::{RetryContext, SystemJitter, run};
use crate::stats::{Stats, StatsSnapshot};
use crate::time::{MonoClock, Sleeper, TokioClock, TokioSleeper};

mod port;

/// Message de parité avec la référence quand les identifiants manquent (04 §2.4).
const CREDENTIALS_MISSING: &str = "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials)";

/// Adapter PISTE : client de l'API Légifrance.
pub struct PisteCorpus<K: MonoClock + Clone = TokioClock, S: Sleeper + Clone = TokioSleeper> {
    inner: Arc<Inner<K, S>>,
}

struct Inner<K: MonoClock + Clone, S: Sleeper> {
    config: Arc<PisteConfig>,
    client: Arc<SharedClient>,
    tokens: TokenManager<K, S>,
    gate: RateGate<K, S>,
    stats: Stats,
    /// `typePagination` reçu par requête sans page (04 §6.6, §12.3), lu et écrit par `search`.
    memo: PaginationMemo<K>,
    clock: K,
    sleeper: S,
}

/// Ce qui ne change pas d'une tentative à l'autre d'un même appel.
struct Call<'a> {
    url: Url,
    /// Ce qu'on interroge : il décide si une réponse peut signifier « introuvable ».
    kind: EndpointKind,
    /// Les deux adresses sont officielles : un 404 de consultation prouve alors l'absence.
    official_host: bool,
    endpoint: &'a str,
    body: &'a [u8],
    deadline: CallDeadline,
    refreshed: AtomicBool,
}

/// Réponse d'une tentative réussie, le permis de concurrence qui l'a laissée partir et le jeton
/// qui l'a obtenue : ils la suivent jusqu'à `post_json`, qui ne rend le permis qu'après le
/// décodage et la lecture du corps (00 §11.1, 04 §5.5), et ne garde le jeton que pour le retirer
/// du détail d'une erreur de lecture (I-5).
struct Reply {
    response: RawResponse,
    permit: Permit,
    lease: TokenLease,
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> Clone for PisteCorpus<K, S> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> fmt::Debug for PisteCorpus<K, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PisteCorpus").field("env", &self.environment()).finish_non_exhaustive()
    }
}

impl PisteCorpus {
    /// Assemble l'adapter avec l'horloge et l'attente de production ; aucune E/S.
    #[must_use]
    pub fn new(config: PisteConfig) -> Self {
        Self::with_parts(config, TokioClock::new(), TokioSleeper)
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Assemble l'adapter avec une horloge et une attente données (tests).
    pub(crate) fn with_parts(config: PisteConfig, clock: K, sleeper: S) -> Self {
        let config = Arc::new(config);
        let client = Arc::new(SharedClient::new());
        let tokens = TokenManager::new(
            Arc::clone(&client),
            Arc::clone(&config),
            clock.clone(),
            sleeper.clone(),
        );
        let gate = RateGate::new(
            clock.clone(),
            sleeper.clone(),
            config.tuning().rate,
            config.tuning().max_concurrency,
        );
        Self {
            inner: Arc::new(Inner {
                config,
                client,
                tokens,
                gate,
                stats: Stats::default(),
                memo: PaginationMemo::new(clock.clone()),
                clock,
                sleeper,
            }),
        }
    }

    /// Environnement PISTE configuré.
    #[must_use]
    pub fn environment(&self) -> PisteEnvironment {
        self.inner.config.env()
    }

    /// Problème d'adresses qui met l'adapter en mode dégradé, s'il y en a un.
    #[must_use]
    pub fn config_problem(&self) -> Option<&ConfigProblem> {
        self.inner.config.endpoints().err()
    }

    /// Vrai si des identifiants OAuth sont configurés.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.inner.config.credentials().is_some()
    }

    /// Nombre de corps dont le mojibake a été réparé.
    #[must_use]
    pub fn mojibake_repairs(&self) -> u64 {
        self.inner.stats.snapshot().mojibake_repairs
    }

    /// Instantané des compteurs.
    #[must_use]
    pub fn stats(&self) -> StatsSnapshot {
        self.inner.stats.snapshot()
    }

    /// Vérifie les identifiants en obtenant un jeton, sans appeler l'API.
    ///
    /// Ni barrière de charge ni requête d'API : une seule requête de jeton, ou aucune si un jeton
    /// valable est déjà en mémoire. Le délai est celui de l'appel en cours (ou `tool_deadline`).
    ///
    /// # Errors
    ///
    /// [`CorpusError::Config`] en mode dégradé (adresses refusées ou identifiants absents, sans
    /// aucune E/S) ; [`CorpusError::Auth`] si PISTE refuse les identifiants ; sinon l'erreur du
    /// gestionnaire de jeton.
    pub async fn check_credentials(&self) -> Result<(), CorpusError> {
        let inner = &*self.inner;
        ready(&inner.config)?;
        let deadline =
            CallDeadline::from_task_local(&inner.clock, inner.config.tuning().tool_deadline);
        inner.tokens.token(&deadline).await.map(|_lease| ())
    }

    /// Envoie `body` en POST sur `endpoint` (gabarit de chemin, `/consult/juri`), décode le texte
    /// de la réponse 2xx (UTF-8, mojibake réparé si la configuration le demande) et rend ce que
    /// `read` en tire.
    ///
    /// `kind` dit ce qu'on interroge : il décide si une réponse peut signifier « introuvable ».
    ///
    /// `read` s'exécute **sous le permis de concurrence** de la tentative réussie, rendu
    /// seulement après lui : la borne de mémoire de 04 §8.5 (`max_concurrency` corps à la fois)
    /// couvre aussi le décodage et le mapping (00 §11.1). `read` est synchrone : le budget de
    /// décodage, état du fil, ne traverse aucun `await`. Il reçoit le texte décodé, le corps brut
    /// déjà rendu ; le texte n'est jamais rendu à l'appelant : l'API empêche d'emprunter le texte
    /// hors permis, pas de le recopier dans `T`, ce qu'aucun lecteur de production ne fait.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Config`] en mode dégradé, sans E/S, et pour un 404 qui ne prouve pas
    /// l'absence (recherche, hôte non officiel, page HTML) ou un 405 ;
    /// [`CorpusError::InvalidRequest`] si `endpoint` ne commence pas par `/` ou pour un 400
    /// (détail du message PISTE nettoyé pour une consultation, générique pour une recherche) ;
    /// [`CorpusError::NotFound`] pour une consultation dont le 400 porte le marqueur d'un
    /// identifiant inconnu, dont le 404 vient de l'hôte officiel hors page HTML, ou dont le 2xx
    /// est vide ; sinon l'erreur classée de la dernière tentative (`Timeout`, `RateLimited`,
    /// `Auth`, `Upstream`, `Decode`, dont le 2xx vide d'une recherche) ; enfin l'erreur de
    /// `read`.
    async fn post_json<T>(
        &self,
        kind: EndpointKind,
        endpoint: &str,
        body: &[u8],
        read: impl FnOnce(&str) -> Result<T, CorpusError>,
    ) -> Result<T, CorpusError> {
        let inner = &*self.inner;
        let endpoints = ready(&inner.config)?;
        let call = Call {
            url: api_url(endpoints.base_url(), endpoint)?,
            kind,
            official_host: endpoints.is_official(),
            endpoint,
            body,
            deadline: CallDeadline::from_task_local(
                &inner.clock,
                inner.config.tuning().tool_deadline,
            ),
            refreshed: AtomicBool::new(false),
        };
        let ctx = RetryContext {
            clock: &inner.clock,
            sleeper: &inner.sleeper,
            deadline: &call.deadline,
            jitter: &SystemJitter,
        };
        // Seul le corps d'une 2xx est décodé ici ; `classify` a déjà lu ceux d'un 400 et d'un 404.
        let Reply { response, permit, lease } = run(&ctx, || self.attempt(&call)).await?;
        let decoded = decode_body(&response.body, endpoint, inner.config.tuning().repair_mojibake);
        // Le corps brut tombe avant la lecture : un seul exemplaire du corps vit pendant `read`.
        drop(response);
        let decoded = decoded?;
        inner.stats.record_decode(endpoint, &decoded);
        let value =
            read(&decoded.text).map_err(|error| without_secrets(error, &lease, &inner.config));
        // Le texte décodé tombe avant le permis : `max_concurrency` corps au plus vivent à la
        // fois, texte compris. Le permis ne tombe qu'après la lecture, jamais avant : le
        // décodage et le mapping bornent la mémoire comme la lecture du corps (04 §8.5).
        drop(decoded);
        drop(permit);
        value
    }

    /// Envoie `body` en POST sur `endpoint`, lit la réponse 2xx dans le DTO `D` puis la fait
    /// traverser `map`, qui rend le résultat du port (04 §8.5, §9.3).
    ///
    /// Le décodage (`decode`, qui arme le budget d'éléments et nomme `endpoint` et le chemin de
    /// l'erreur dans le `detail`) et le mapping s'exécutent tous deux dans le lecteur de
    /// `post_json`, donc **sous le permis de concurrence** (00 §11.1). `map` reçoit des
    /// diagnostics vides à chaque appel ; ils sont versés aux compteurs (`record_mapping`) une
    /// fois par appel, sur `Ok` comme sur `Err` du mapping : un résultat écarté avant l'échec
    /// compte quand même. Un `NotFound`, une erreur de transport ou un corps illisible n'ont
    /// rien à compter et n'y touchent pas.
    ///
    /// # Errors
    ///
    /// Celles de [`Self::post_json`] ; [`CorpusError::Decode`] si le corps ne correspond pas à
    /// `D` ; enfin l'erreur de `map`.
    async fn fetch<D: DeserializeOwned, T>(
        &self,
        kind: EndpointKind,
        endpoint: &str,
        body: &[u8],
        map: impl FnOnce(D, &mut Diagnostics) -> Result<T, CorpusError>,
    ) -> Result<T, CorpusError> {
        let stats = &self.inner.stats;
        self.post_json(kind, endpoint, body, |text| {
            let mut diags = Diagnostics::default();
            let mapped = decode(text, endpoint).and_then(|dto| map(dto, &mut diags));
            stats.record_mapping(endpoint, &diags);
            mapped
        })
        .await
    }

    /// Une tentative de `run` : un envoi, et, sur un premier 401, un second envoi immédiat avec
    /// un jeton neuf (04 §3.4).
    ///
    /// Le renvoi sort du budget de `run` : ni attente, ni l'une des trois requêtes transitoires.
    /// `refreshed` vit autant que l'appel, donc un 401 après un échec passager est bien le
    /// second. Un `Unauthorized` qui reste devient l'erreur finale `Auth`.
    async fn attempt(&self, call: &Call<'_>) -> Outcome<Reply> {
        let mut sent = self.send(call).await;
        if matches!(sent, Ok(Outcome::Unauthorized(_))) && !call.refreshed.swap(true, Relaxed) {
            sent = self.send(call).await;
        }
        match sent {
            Ok(Outcome::Unauthorized(_)) => {
                Outcome::Fatal(auth_twice(call.endpoint, self.inner.config.env()))
            }
            Ok(outcome) => outcome,
            Err(error) => Outcome::Fatal(error),
        }
    }

    /// Un envoi : client, barrière de charge, jeton, requête bornée, classification.
    ///
    /// Le client vient **avant** la barrière : un échec de construction est une erreur de
    /// configuration, sans attendre le limiteur (après le jeton, il serait inatteignable). Le
    /// délai de la requête est recalculé ici, à chaque envoi, sur le reste de l'échéance. Une
    /// erreur de barrière ou de jeton sort en `Err`, donc `Fatal` : déjà « au plus vite » ou déjà
    /// réessayée en interne.
    ///
    /// Le permis de concurrence suit la réponse d'un succès dans [`Reply`], jusqu'après la
    /// lecture du corps par `post_json` ; celui de toute autre issue tombe dès la classification,
    /// avec la fermeture de [`Outcome::map`], **avant** l'oubli du jeton refusé : un 401 n'occupe
    /// pas une place de concurrence pendant l'attente du verrou du jeton.
    ///
    /// Sur un 401 (le premier comme le second : un jeton refusé ne doit pas servir à l'appel
    /// suivant), le jeton refusé est oublié par `invalidate`, dont l'attente du verrou est bornée
    /// par l'échéance de l'appel comme celle de `token` (04 §5.3). Si elle est écourtée
    /// (`Timeout`), `send` n'échoue pas pour autant et ignore ce résultat : l'échéance est alors
    /// atteinte, et `gate.acquire` rendra `Timeout` en tête de la tentative suivante.
    async fn send(&self, call: &Call<'_>) -> Result<Outcome<Reply>, CorpusError> {
        let inner = &*self.inner;
        let client = inner.client.get(&inner.config).await?;
        let permit = inner.gate.acquire(&call.deadline).await?;
        let lease = inner.tokens.token(&call.deadline).await?;
        let attempt = Attempt {
            url: &call.url,
            endpoint: call.endpoint,
            payload: Payload::Json(call.body),
            bearer: Some(&lease.value),
            timeout: call.deadline.cap(&inner.clock, inner.config.tuning().request_timeout),
            max_body_bytes: inner.config.tuning().max_body_bytes,
        };
        // Cinquième point d'exposition du jeton et du secret client (04 §3.6) : leurs valeurs ne
        // servent qu'à être retirées du message d'un corps d'erreur, sans copie conservée. Un
        // secret vide (identifiants absents) est ignoré par `redact`.
        let secrets = [
            lease.value.expose_secret(),
            inner
                .config
                .credentials()
                .map_or("", |credentials| credentials.client_secret().expose_secret()),
        ];
        let context = ErrorContext {
            kind: call.kind,
            official_host: call.official_host,
            repair_mojibake: inner.config.tuning().repair_mojibake,
            secrets: &secrets,
        };
        let generation = lease.generation;
        let outcome = try_send(client, &attempt, &context).await.map(|response| Reply {
            response,
            permit,
            lease,
        });
        if matches!(outcome, Outcome::Unauthorized(_)) {
            let _ = inner.tokens.invalidate(generation, &call.deadline).await;
        }
        Ok(outcome)
    }
}

/// Adresses de l'adapter si l'appel peut partir : mode dégradé sinon (04 §2.4).
///
/// Problème d'adresses d'abord, identifiants absents ensuite ; aucune E/S.
fn ready(config: &PisteConfig) -> Result<&PisteEndpoints, CorpusError> {
    let endpoints = config
        .endpoints()
        .map_err(|problem| CorpusError::Config { detail: problem.to_string() })?;
    if config.credentials().is_none() {
        return Err(CorpusError::Config { detail: CREDENTIALS_MISSING.to_owned() });
    }
    Ok(endpoints)
}

/// URL d'un endpoint : le gabarit se **concatène** au chemin de la base (`Url::join`
/// remplacerait son dernier segment). Le `/` final de la base, présent pour la racine, est
/// retiré pour ne jamais produire `//`.
fn api_url(base: &Url, endpoint: &str) -> Result<Url, CorpusError> {
    if !endpoint.starts_with('/') {
        return Err(CorpusError::InvalidRequest {
            status: None,
            detail: "endpoint template must start with a slash".to_owned(),
        });
    }
    let mut url = base.clone();
    url.set_path(&format!("{}{endpoint}", base.path().trim_end_matches('/')));
    Ok(url)
}

/// Retire du détail d'une erreur `Decode` de lecture les valeurs exactes du jeton et du secret
/// client (I-5, plan 2h ; même règle que le détail d'un 400, 04 §11.2 règle 3).
///
/// `decode` ne cite jamais une valeur du corps, mais garde dans le chemin une clé de forme
/// identifiant (`[A-Za-z0-9_-]{1,64}`, `dto::decode::safe_key`) : une réponse qui recopierait le
/// jeton ou le secret **en clé** d'un champ non déclaré mal formé le ferait sortir dans le
/// détail. Le retrait se fait par valeur, après coup ; les autres erreurs passent telles quelles.
fn without_secrets(error: CorpusError, lease: &TokenLease, config: &PisteConfig) -> CorpusError {
    match error {
        CorpusError::Decode { detail } => {
            let secret = config
                .credentials()
                .map_or("", |credentials| credentials.client_secret().expose_secret());
            CorpusError::Decode {
                detail: crate::error_body::redact(&detail, &[lease.value.expose_secret(), secret]),
            }
        }
        other => other,
    }
}

/// Erreur finale d'un 401 qui se répète après le renouvellement du jeton.
fn auth_twice(endpoint: &str, env: PisteEnvironment) -> CorpusError {
    CorpusError::Auth { detail: format!("API returned 401 twice on {endpoint} ({env})") }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU16, NonZeroU32};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use lf_application::deadline::Deadline;
    use secrecy::SecretString;
    use tokio::task::JoinSet;
    use wiremock::matchers::{body_bytes, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;
    use crate::config::{PisteCredentials, PisteTuning, RateQuota};
    use crate::testing::{ClosedPort, capture_at};
    use crate::time::{ManualClock, RecordingSleeper};

    pub(super) const SECRET: &str = "my-client-secret";
    const ENDPOINT: &str = "/consult/juri";
    pub(super) const API_BASE: &str = "/dila/legifrance/lf-engine-app";
    const API_PATH: &str = "/dila/legifrance/lf-engine-app/consult/juri";
    pub(super) const TOKEN_PATH: &str = "/oauth/token";
    const BODY: &[u8] = br#"{"textId":"JURITEXT000000000001"}"#;
    const MISSING: &str = "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials)";
    const SANDBOX: PisteEnvironment = PisteEnvironment::Sandbox;

    type TestCorpus = PisteCorpus<ManualClock, RecordingSleeper>;

    pub(super) const fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    pub(super) const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    pub(super) fn rate(per_second: u32, burst: u32) -> RateQuota {
        RateQuota::new(NonZeroU32::new(per_second).unwrap(), NonZeroU32::new(burst).unwrap())
    }

    fn credentials(secret: &str) -> PisteCredentials {
        PisteCredentials::new("my-client-id".to_owned(), SecretString::from(secret.to_owned()))
    }

    fn endpoints_for(uri: &str) -> PisteEndpoints {
        let (endpoints, _) = PisteEndpoints::with_overrides(
            SANDBOX,
            Some(&format!("{uri}{TOKEN_PATH}")),
            Some(&format!("{uri}{API_BASE}")),
        )
        .unwrap();
        endpoints
    }

    fn config_for(uri: &str, credentials: Option<PisteCredentials>) -> PisteConfig {
        PisteConfig::new(SANDBOX, Ok(endpoints_for(uri)), credentials, "lf-test/0".to_owned())
    }

    /// Ce qu'un test peut ajuster avant la construction de la configuration : les champs de
    /// `PisteConfig` sont privés, la configuration se reconstruit donc d'un bloc.
    pub(super) struct Setup {
        uri: String,
        pub(super) credentials: Option<PisteCredentials>,
        problem: Option<ConfigProblem>,
        user_agent: String,
        pub(super) tuning: PisteTuning,
    }

    impl Setup {
        /// Configuration ordinaire vers `uri` : identifiants `SECRET`, réglages par défaut.
        fn new(uri: String) -> Self {
            Self {
                uri,
                credentials: Some(credentials(SECRET)),
                problem: None,
                user_agent: "lf-test/0".to_owned(),
                tuning: PisteTuning::default(),
            }
        }

        fn build(self) -> PisteConfig {
            let endpoints = self.problem.map_or_else(|| Ok(endpoints_for(&self.uri)), Err);
            PisteConfig::new(SANDBOX, endpoints, self.credentials, self.user_agent)
                .with_tuning(self.tuning)
                .unwrap()
        }
    }

    pub(super) fn json_reply(status: u16, body: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_raw(body, "application/json")
    }

    /// Corps JSON dont le texte est du mojibake synthétique (octets UTF-8 relus en latin-1).
    fn mojibake_json(texte: &str) -> String {
        let mojibake: String = texte.bytes().map(char::from).collect();
        format!(r#"{{"texte":"{mojibake}"}}"#)
    }

    fn token_reply(token: &str) -> ResponseTemplate {
        json_reply(
            200,
            &format!(r#"{{"access_token":"{token}","token_type":"Bearer","expires_in":3600}}"#),
        )
    }

    /// Le jeton `token`, servi `times` fois exactement.
    pub(super) async fn token_mock(server: &MockServer, token: &str, times: u64) {
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(token_reply(token))
            .up_to_n_times(times)
            .expect(times)
            .mount(server)
            .await;
    }

    /// Le point d'accès du jeton répond `template` à volonté.
    async fn token_always(server: &MockServer, template: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(template)
            .mount(server)
            .await;
    }

    /// L'API répond `template` `times` fois exactement, dans l'ordre de montage.
    async fn api_mock(server: &MockServer, template: ResponseTemplate, times: u64) {
        Mock::given(method("POST"))
            .and(path(API_PATH))
            .respond_with(template)
            .up_to_n_times(times)
            .expect(times)
            .mount(server)
            .await;
    }

    /// L'API répond `template` une fois, à la seule requête qui porte le jeton `token`.
    async fn api_mock_for(server: &MockServer, token: &str, template: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(API_PATH))
            .and(header("authorization", format!("Bearer {token}").as_str()))
            .respond_with(template)
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
    }

    async fn api_always(server: &MockServer, template: ResponseTemplate) {
        Mock::given(method("POST")).and(path(API_PATH)).respond_with(template).mount(server).await;
    }

    /// `route` ne doit jamais être appelée (wiremock le vérifie à la chute du serveur).
    pub(super) async fn never(server: &MockServer, route: &str) {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(server)
            .await;
    }

    pub(super) async fn requests_to(server: &MockServer, route: &str) -> usize {
        let requests = server.received_requests().await.unwrap();
        requests.iter().filter(|request| request.url.path() == route).count()
    }

    /// Lecteur de `post_json` qui rend le texte décodé tel quel.
    #[expect(clippy::unnecessary_wraps, reason = "signature imposée par post_json")]
    fn owned(text: &str) -> Result<String, CorpusError> {
        Ok(text.to_owned())
    }

    /// Serveur simulé, horloge et attente manuelles, adapter sous test.
    pub(super) struct Rig {
        pub(super) server: MockServer,
        clock: ManualClock,
        pub(super) sleeper: RecordingSleeper,
        pub(super) corpus: TestCorpus,
        config: PisteConfig,
    }

    impl Rig {
        pub(super) async fn new() -> Self {
            Self::tuned(|_| {}).await
        }

        /// Configuration ordinaire, ajustée par `tune` avant la construction.
        pub(super) async fn tuned(tune: impl FnOnce(&mut Setup)) -> Self {
            let server = MockServer::start().await;
            let mut setup = Setup::new(server.uri());
            tune(&mut setup);
            let config = setup.build();
            let clock = ManualClock::new();
            let sleeper = RecordingSleeper::new(clock.clone());
            let corpus = TestCorpus::with_parts(config.clone(), clock.clone(), sleeper.clone());
            Self { server, clock, sleeper, corpus, config }
        }

        async fn call(&self) -> Result<String, CorpusError> {
            self.corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await
        }

        async fn tokens(&self) -> usize {
            requests_to(&self.server, TOKEN_PATH).await
        }

        async fn apis(&self) -> usize {
            requests_to(&self.server, API_PATH).await
        }

        async fn all_requests(&self) -> usize {
            self.server.received_requests().await.unwrap().len()
        }

        /// Tout ce qu'un journal ou un message pourrait montrer de cette erreur.
        fn describe(&self, error: &CorpusError) -> [String; 4] {
            [
                format!("{error}"),
                format!("{error:?}"),
                format!("{:?}", self.corpus),
                format!("{:?}", self.config),
            ]
        }
    }

    /// Mode dégradé : ni attente, ni temps écoulé, ni requête, ni client construit.
    async fn assert_untouched(rig: &Rig) {
        assert_eq!(
            (
                rig.sleeper.waits(),
                rig.clock.now(),
                rig.all_requests().await,
                rig.corpus.inner.client.is_built()
            ),
            (Vec::new(), Duration::ZERO, 0, false)
        );
    }

    // ---- 401 : un seul renouvellement par appel (04 §3.4) --------------------------------

    #[tokio::test]
    async fn api_401_refreshes_once_then_succeeds() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        token_mock(&rig.server, "tok-2", 1).await;
        api_mock_for(&rig.server, "tok-1", ResponseTemplate::new(401)).await;
        api_mock_for(&rig.server, "tok-2", json_reply(200, r#"{"ok":true}"#)).await;
        assert_eq!(rig.call().await, Ok(r#"{"ok":true}"#.to_owned()));
        assert_eq!((rig.tokens().await, rig.apis().await), (2, 2));
        assert_eq!(rig.sleeper.waits(), Vec::<Duration>::new());
    }

    #[tokio::test]
    async fn api_401_twice_maps_to_auth() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        token_mock(&rig.server, "tok-2", 1).await;
        api_mock(&rig.server, ResponseTemplate::new(401), 2).await;
        assert_eq!(
            rig.call().await,
            Err(CorpusError::Auth {
                detail: "API returned 401 twice on /consult/juri (sandbox)".to_owned()
            })
        );
        assert_eq!((rig.tokens().await, rig.apis().await), (2, 2));
        assert_eq!(rig.sleeper.waits(), Vec::<Duration>::new());
    }

    #[tokio::test]
    async fn a_second_401_after_a_transient_failure_is_still_the_second() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        token_mock(&rig.server, "tok-2", 1).await;
        api_mock(&rig.server, ResponseTemplate::new(401), 1).await;
        api_mock(&rig.server, ResponseTemplate::new(503), 1).await;
        api_mock(&rig.server, ResponseTemplate::new(401), 1).await;
        assert_eq!(
            rig.call().await,
            Err(CorpusError::Auth {
                detail: "API returned 401 twice on /consult/juri (sandbox)".to_owned()
            })
        );
        assert_eq!((rig.tokens().await, rig.apis().await), (2, 3));
    }

    #[tokio::test]
    async fn the_401_budget_is_per_call() {
        let rig = Rig::new().await;
        for token in ["tok-1", "tok-2", "tok-3"] {
            token_mock(&rig.server, token, 1).await;
        }
        for status in [401, 200, 401, 200] {
            api_mock(&rig.server, json_reply(status, "{}"), 1).await;
        }
        assert_eq!(rig.call().await, Ok("{}".to_owned()));
        assert_eq!(rig.call().await, Ok("{}".to_owned()));
        assert_eq!((rig.tokens().await, rig.apis().await), (3, 4));
    }

    #[tokio::test]
    async fn status_401_then_503_then_200_requests_exactly_two_tokens() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        token_mock(&rig.server, "tok-2", 1).await;
        api_mock(&rig.server, ResponseTemplate::new(401), 1).await;
        api_mock(&rig.server, ResponseTemplate::new(503), 1).await;
        api_mock(&rig.server, json_reply(200, "{}"), 1).await;
        assert_eq!(rig.call().await, Ok("{}".to_owned()));
        assert_eq!((rig.tokens().await, rig.apis().await), (2, 3));
        let waits = rig.sleeper.waits();
        assert_eq!(waits.len(), 1);
        assert!((ms(500)..secs(1)).contains(&waits[0]), "{waits:?}");
    }

    // ---- Concurrence et débit ------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_calls_share_one_token_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(token_reply("shared").set_delay(ms(100)))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        api_always(&server, json_reply(200, "{}")).await;
        let corpus = PisteCorpus::new(config_for(&server.uri(), Some(credentials(SECRET))));
        let mut tasks = JoinSet::new();
        for _ in 0..10 {
            let corpus = corpus.clone();
            tasks.spawn(async move {
                corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await
            });
        }
        let results = tasks.join_all().await;
        assert_eq!(results, vec![Ok("{}".to_owned()); 10]);
        assert_eq!(
            (requests_to(&server, TOKEN_PATH).await, requests_to(&server, API_PATH).await),
            (1, 10)
        );
    }

    /// Plus grand nombre d'arrivées dans une fenêtre de `window` qui s'ouvre sur l'une d'elles.
    fn peak_in_flight(arrivals: &[Instant], window: Duration) -> usize {
        arrivals
            .iter()
            .map(|first| {
                arrivals
                    .iter()
                    .filter(|at| at.checked_duration_since(*first).is_some_and(|gap| gap < window))
                    .count()
            })
            .max()
            .unwrap_or(0)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrency_cap_is_respected() {
        let server = MockServer::start().await;
        token_mock(&server, "tok-1", 1).await;
        // Un `Respond` rend la main avant le délai de la réponse : on ne peut pas y compter les
        // requêtes « en vol », seulement noter les arrivées.
        let arrivals = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&arrivals);
        Mock::given(method("POST"))
            .and(path(API_PATH))
            .respond_with(move |_: &Request| {
                seen.lock().unwrap().push(Instant::now());
                json_reply(200, "{}").set_delay(ms(200))
            })
            .mount(&server)
            .await;
        let mut setup = Setup::new(server.uri());
        setup.tuning.rate = rate(1_000, 1_000);
        let corpus = PisteCorpus::new(setup.build());
        let mut tasks = JoinSet::new();
        for _ in 0..10 {
            let corpus = corpus.clone();
            tasks.spawn(async move {
                corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await
            });
        }
        assert_eq!(tasks.join_all().await, vec![Ok("{}".to_owned()); 10]);
        let arrivals = arrivals.lock().unwrap().clone();
        assert_eq!(arrivals.len(), 10);
        // Par vagues de 4 espacées de 200 ms : aucune fenêtre de 100 ms n'en contient plus (sans
        // plafond, les dix arrivent ensemble).
        let peak = peak_in_flight(&arrivals, ms(100));
        assert!((2..=4).contains(&peak), "{peak}");
    }

    #[tokio::test]
    async fn rate_limiter_spaces_api_calls() {
        let rig = Rig::tuned(|setup| setup.tuning.rate = rate(2, 1)).await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, json_reply(200, "{}")).await;
        for _ in 0..5 {
            assert_eq!(rig.call().await, Ok("{}".to_owned()));
        }
        assert_eq!(rig.sleeper.waits(), vec![ms(500); 4]);
        assert_eq!(rig.clock.now(), ms(2_000));
    }

    #[tokio::test]
    async fn every_attempt_goes_through_the_rate_limiter() {
        let rig = Rig::tuned(|setup| setup.tuning.rate = rate(1, 1)).await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock(&rig.server, ResponseTemplate::new(503), 1).await;
        api_mock(&rig.server, json_reply(200, "{}"), 1).await;
        assert_eq!(rig.call().await, Ok("{}".to_owned()));
        let waits = rig.sleeper.waits();
        assert_eq!(waits.len(), 2);
        assert!((ms(500)..secs(1)).contains(&waits[0]), "{waits:?}");
        assert_eq!(waits[0] + waits[1], secs(1));
    }

    #[tokio::test]
    async fn rate_limit_wait_beyond_deadline_fails_fast() {
        let rig = Rig::tuned(|setup| {
            setup.tuning.rate = rate(1, 1);
            setup.tuning.tool_deadline = ms(500);
        })
        .await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, json_reply(200, "{}")).await;
        assert_eq!(rig.call().await, Ok("{}".to_owned()));
        assert_eq!(rig.call().await, Err(CorpusError::RateLimited { retry_after: Some(secs(1)) }));
        assert_eq!(rig.sleeper.waits(), Vec::<Duration>::new());
        assert_eq!(rig.apis().await, 1);
    }

    // ---- Permis de concurrence tenu pendant la lecture du corps (00 §11.1, 04 §5.5) -------

    #[tokio::test]
    async fn the_concurrency_permit_is_held_while_the_body_is_read() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock(&rig.server, json_reply(200, "{}"), 1).await;
        let gate = &rig.corpus.inner.gate;
        let free = gate.available();
        let read = rig
            .corpus
            .post_json(EndpointKind::Consult, ENDPOINT, BODY, |text| {
                Ok((text.len(), gate.available()))
            })
            .await;
        assert_eq!(read, Ok((2, free - 1)));
        assert_eq!(gate.available(), free);
    }

    #[tokio::test]
    async fn a_failed_attempt_gives_its_permit_back_before_the_retry() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock(&rig.server, ResponseTemplate::new(503), 1).await;
        api_mock(&rig.server, json_reply(200, "{}"), 1).await;
        let gate = &rig.corpus.inner.gate;
        let free = gate.available();
        let read = rig
            .corpus
            .post_json(EndpointKind::Consult, ENDPOINT, BODY, |_| Ok(gate.available()))
            .await;
        assert_eq!(read, Ok(free - 1));
        assert_eq!(rig.apis().await, 2);
    }

    #[tokio::test]
    async fn the_permit_is_given_back_when_the_reader_fails() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock(&rig.server, json_reply(200, "{}"), 1).await;
        let gate = &rig.corpus.inner.gate;
        let free = gate.available();
        let refused = CorpusError::Decode { detail: "/consult/juri: .: invalid data".to_owned() };
        let read = rig
            .corpus
            .post_json(EndpointKind::Consult, ENDPOINT, BODY, |_| Err::<(), _>(refused.clone()))
            .await;
        assert_eq!(read, Err(refused));
        assert_eq!(gate.available(), free);
    }

    #[tokio::test]
    async fn the_permit_is_given_back_when_the_reader_panics() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock(&rig.server, json_reply(200, "{}"), 1).await;
        let free = rig.corpus.inner.gate.available();
        let corpus = rig.corpus.clone();
        let task = tokio::spawn(async move {
            corpus
                .post_json(EndpointKind::Consult, ENDPOINT, BODY, |_| {
                    Ok("not a number".parse::<u8>().expect("reader exploded"))
                })
                .await
        });
        let failure = task.await.unwrap_err();
        assert!(failure.is_panic());
        let message = failure.into_panic().downcast::<String>().unwrap();
        assert!(message.starts_with("reader exploded:"), "{message}");
        assert_eq!(rig.corpus.inner.gate.available(), free);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_waits_while_another_reads_its_body() {
        let server = MockServer::start().await;
        token_mock(&server, "tok-1", 1).await;
        api_always(&server, json_reply(200, "{}")).await;
        let mut setup = Setup::new(server.uri());
        setup.tuning.max_concurrency = NonZeroU16::MIN;
        let corpus = PisteCorpus::new(setup.build());
        let (reading, started) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let first = {
            let corpus = corpus.clone();
            tokio::spawn(async move {
                let read = move |text: &str| {
                    reading.send(()).unwrap();
                    // `block_in_place` cède le cœur du fil : un `recv()` nu garderait la tâche de
                    // connexion dans l'emplacement LIFO du fil bloqué, et le second appel
                    // attendrait même avec un permis rendu trop tôt.
                    tokio::task::block_in_place(|| released.recv()).unwrap();
                    owned(text)
                };
                corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, read).await
            })
        };
        started.await.unwrap();
        let second = {
            let corpus = corpus.clone();
            tokio::spawn(async move {
                corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await
            })
        };
        tokio::time::sleep(ms(300)).await;
        assert_eq!(requests_to(&server, API_PATH).await, 1);
        release.send(()).unwrap();
        assert_eq!(first.await.unwrap(), Ok("{}".to_owned()));
        assert_eq!(second.await.unwrap(), Ok("{}".to_owned()));
        assert_eq!(requests_to(&server, API_PATH).await, 2);
    }

    // ---- fetch : décodage sous budget, mapping et diagnostics (04 §8.5, §9.3) --------------

    /// DTO de test : sa liste exige le budget que seul `decode` arme.
    #[derive(serde::Deserialize)]
    struct Items {
        #[serde(default, deserialize_with = "crate::dto::de::bounded_list")]
        items: Vec<u8>,
    }

    /// Mapper de test : note un `id_mismatch` et rend le nombre d'éléments.
    #[expect(clippy::unnecessary_wraps, reason = "signature imposée par fetch")]
    fn count(Items { items }: Items, diags: &mut Diagnostics) -> Result<usize, CorpusError> {
        diags.note_id_mismatch();
        Ok(items.len())
    }

    /// Mapper de test qui note un résultat écarté puis échoue.
    fn refuse(_items: Items, diags: &mut Diagnostics) -> Result<usize, CorpusError> {
        diags.note_dropped_hit();
        Err(CorpusError::Decode {
            detail: "/consult/juri: text.dateTexte: unreadable date".to_owned(),
        })
    }

    async fn fetch_with<T>(
        rig: &Rig,
        body: &str,
        map: impl FnOnce(Items, &mut Diagnostics) -> Result<T, CorpusError>,
    ) -> Result<T, CorpusError> {
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock(&rig.server, json_reply(200, body), 1).await;
        rig.corpus.fetch(EndpointKind::Consult, ENDPOINT, BODY, map).await
    }

    #[tokio::test]
    async fn fetch_maps_the_decoded_body_and_records_its_diagnostics() {
        let rig = Rig::new().await;
        assert_eq!(fetch_with(&rig, r#"{"items":[1,2,3]}"#, count).await, Ok(3));
        assert_eq!(
            rig.corpus.stats(),
            StatsSnapshot { id_mismatch: 1, ..StatsSnapshot::default() }
        );
    }

    #[tokio::test]
    async fn fetch_records_the_diagnostics_of_a_mapping_that_fails() {
        let rig = Rig::new().await;
        assert_eq!(
            fetch_with(&rig, r#"{"items":[]}"#, refuse).await,
            Err(CorpusError::Decode {
                detail: "/consult/juri: text.dateTexte: unreadable date".to_owned()
            })
        );
        assert_eq!(
            rig.corpus.stats(),
            StatsSnapshot { dropped_hits: 1, ..StatsSnapshot::default() }
        );
    }

    #[tokio::test]
    async fn fetch_decodes_with_the_endpoint_and_the_path_in_the_detail() {
        let rig = Rig::new().await;
        assert_eq!(
            fetch_with(&rig, r#"{"items":[1,"sekret"]}"#, count).await,
            Err(CorpusError::Decode {
                detail: "/consult/juri: items[1]: invalid type: string, expected u8".to_owned()
            })
        );
        assert_eq!(rig.corpus.stats(), StatsSnapshot::default());
    }

    #[tokio::test]
    async fn a_decode_detail_never_carries_the_token_or_the_secret_as_a_path_key() {
        // Clé de forme identifiant d'un champ non déclaré et mal formé : gardée dans le chemin
        // par `decode`, retirée par valeur quand c'est le jeton ou le secret client (I-5).
        for (key, shown) in
            [("tok-1", "[redacted]"), (SECRET, "[redacted]"), ("free-key", "free-key")]
        {
            let rig = Rig::new().await;
            let body = format!(r#"{{"items":[1],"{key}":[1,}}"#);
            assert_eq!(
                fetch_with(&rig, &body, count).await,
                Err(CorpusError::Decode {
                    detail: format!(
                        "/consult/juri: {shown}: expected value at line 1 column {}",
                        body.len()
                    )
                }),
                "{key}"
            );
        }
    }

    #[test]
    fn without_secrets_leaves_other_errors_alone() {
        let lease = TokenLease { value: SecretString::from("tok-1".to_owned()), generation: 1 };
        let config = PisteConfig::new(
            SANDBOX,
            Ok(endpoints_for("http://127.0.0.1:9")),
            None,
            "ua".to_owned(),
        );
        let not_found = CorpusError::NotFound { what: "tok-1".to_owned() };
        assert_eq!(without_secrets(not_found.clone(), &lease, &config), not_found);
        assert_eq!(
            without_secrets(
                CorpusError::Decode { detail: "x: tok-1: y".to_owned() },
                &lease,
                &config
            ),
            CorpusError::Decode { detail: "x: [redacted]: y".to_owned() }
        );
    }

    // ---- Mode dégradé (04 §2.4) ----------------------------------------------------------

    #[tokio::test]
    async fn missing_credentials_makes_no_network_call() {
        let rig = Rig::tuned(|setup| setup.credentials = None).await;
        never(&rig.server, TOKEN_PATH).await;
        never(&rig.server, API_PATH).await;
        assert_eq!(rig.call().await, Err(CorpusError::Config { detail: MISSING.to_owned() }));
        assert_untouched(&rig).await;
        assert!(!rig.corpus.has_credentials());
        assert_eq!(rig.corpus.config_problem(), None);
    }

    #[tokio::test]
    async fn config_problem_starts_in_degraded_mode() {
        let problem = ConfigProblem::EnvMismatch {
            setting: "base URL",
            host: "api.piste.gouv.fr".to_owned(),
            env: PisteEnvironment::Sandbox,
            host_env: PisteEnvironment::Production,
        };
        let rig = Rig::tuned(|setup| setup.problem = Some(problem.clone())).await;
        never(&rig.server, TOKEN_PATH).await;
        never(&rig.server, API_PATH).await;
        assert_eq!(rig.call().await, Err(CorpusError::Config { detail: problem.to_string() }));
        assert_untouched(&rig).await;
        assert_eq!(rig.corpus.config_problem(), Some(&problem));
        assert!(rig.corpus.has_credentials());
    }

    #[tokio::test]
    async fn a_healthy_configuration_reports_no_problem() {
        let rig = Rig::new().await;
        assert_eq!(
            (rig.corpus.config_problem(), rig.corpus.has_credentials(), rig.corpus.environment()),
            (None, true, SANDBOX)
        );
    }

    #[tokio::test]
    async fn debug_of_corpus_shows_the_environment_only() {
        let rig = Rig::new().await;
        assert_eq!(format!("{:?}", rig.corpus), "PisteCorpus { env: Sandbox, .. }");
    }

    #[tokio::test]
    async fn endpoint_template_must_start_with_a_slash() {
        let rig = Rig::new().await;
        assert_eq!(
            rig.corpus.post_json(EndpointKind::Consult, "consult/juri", BODY, owned).await,
            Err(CorpusError::InvalidRequest {
                status: None,
                detail: "endpoint template must start with a slash".to_owned()
            })
        );
        assert_untouched(&rig).await;
    }

    #[tokio::test]
    async fn an_unbuildable_client_is_a_config_error_without_io() {
        let rig = Rig::tuned(|setup| setup.user_agent = "bad\nagent".to_owned()).await;
        assert_eq!(
            rig.call().await,
            Err(CorpusError::Config {
                detail:
                    "could not build the HTTP client: builder error: failed to parse header value"
                        .to_owned()
            })
        );
        assert_untouched(&rig).await;
    }

    // ---- check_credentials ---------------------------------------------------------------

    #[tokio::test]
    async fn check_credentials_makes_exactly_one_token_request() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        never(&rig.server, API_PATH).await;
        assert_eq!(rig.corpus.check_credentials().await, Ok(()));
        assert_eq!((rig.tokens().await, rig.apis().await), (1, 0));
        assert_eq!(rig.corpus.check_credentials().await, Ok(()));
        assert_eq!((rig.tokens().await, rig.apis().await), (1, 0));
    }

    fn rejected() -> CorpusError {
        CorpusError::Auth {
            detail:
                "OAuth token request rejected: HTTP 401, error invalid_client, environment sandbox"
                    .to_owned(),
        }
    }

    #[tokio::test]
    async fn check_credentials_reports_rejected_credentials() {
        let rig = Rig::new().await;
        token_always(&rig.server, json_reply(401, r#"{"error":"invalid_client"}"#)).await;
        assert_eq!(rig.corpus.check_credentials().await, Err(rejected()));
        assert_eq!(rig.tokens().await, 1);
    }

    #[tokio::test]
    async fn check_credentials_in_degraded_mode_is_config() {
        let problem = ConfigProblem::InsecureScheme { setting: "OAuth URL" };
        let rig = Rig::tuned(|setup| setup.credentials = None).await;
        never(&rig.server, TOKEN_PATH).await;
        assert_eq!(
            rig.corpus.check_credentials().await,
            Err(CorpusError::Config { detail: MISSING.to_owned() })
        );
        assert_untouched(&rig).await;
        let rig = Rig::tuned(|setup| setup.problem = Some(problem.clone())).await;
        never(&rig.server, TOKEN_PATH).await;
        assert_eq!(
            rig.corpus.check_credentials().await,
            Err(CorpusError::Config { detail: problem.to_string() })
        );
        assert_untouched(&rig).await;
    }

    #[tokio::test]
    async fn a_token_failure_is_final_and_reaches_no_api() {
        let rig = Rig::new().await;
        token_always(&rig.server, json_reply(401, r#"{"error":"invalid_client"}"#)).await;
        never(&rig.server, API_PATH).await;
        assert_eq!(rig.call().await, Err(rejected()));
        assert_eq!((rig.tokens().await, rig.apis().await), (1, 0));
    }

    // ---- Échéances -----------------------------------------------------------------------

    #[tokio::test]
    async fn deadline_from_task_local_is_used() {
        let rig = Rig::new().await;
        // Origine arbitraire : seule la durée restante de l'échéance compte.
        rig.clock.advance(secs(1_000_000));
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, ResponseTemplate::new(503)).await;
        let result = Deadline::after(ms(400)).scope(rig.call()).await;
        assert_eq!(
            result,
            Err(CorpusError::Upstream {
                status: Some(503),
                retryable: true,
                detail: "HTTP 503 on /consult/juri".to_owned()
            })
        );
        assert_eq!(rig.sleeper.waits(), Vec::<Duration>::new());
        assert_eq!(rig.clock.now(), secs(1_000_000));
    }

    #[tokio::test]
    async fn no_deadline_published_uses_tool_deadline() {
        let rig = Rig::tuned(|setup| setup.tuning.tool_deadline = ms(400)).await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, ResponseTemplate::new(503)).await;
        assert_eq!(
            rig.call().await,
            Err(CorpusError::Upstream {
                status: Some(503),
                retryable: true,
                detail: "HTTP 503 on /consult/juri".to_owned()
            })
        );
        assert_eq!(rig.sleeper.waits(), Vec::<Duration>::new());
        assert_eq!(rig.clock.now(), Duration::ZERO);
    }

    #[tokio::test]
    async fn tool_deadline_caps_retries() {
        // Le backoff vaut au moins 500 ms et la gigue est aléatoire : seulement des invariants.
        let rig = Rig::tuned(|setup| setup.tuning.tool_deadline = secs(1)).await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, ResponseTemplate::new(503)).await;
        rig.call().await.unwrap_err();
        let (apis, now) = (rig.apis().await, rig.clock.now());
        assert!((1..=2).contains(&apis), "{apis}");
        assert!(now <= secs(1), "{now:?}");
        // Témoin sans borne serrée : les trois requêtes ont lieu.
        let witness = Rig::new().await;
        token_mock(&witness.server, "tok-1", 1).await;
        api_always(&witness.server, ResponseTemplate::new(503)).await;
        witness.call().await.unwrap_err();
        assert_eq!(witness.apis().await, 3);
    }

    #[tokio::test]
    async fn each_attempt_gets_the_time_left_not_the_time_at_the_start() {
        // Après le premier backoff (500 ms à 1 s), il reste moins de 700 ms sur 1,2 s : la
        // seconde tentative, dont la réponse tarde 900 ms, doit expirer. Avec un délai calculé
        // une seule fois (1,2 s), elle aboutirait.
        let rig = Rig::tuned(|setup| {
            setup.tuning.tool_deadline = ms(1_200);
            setup.tuning.request_timeout = secs(10);
        })
        .await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock(&rig.server, ResponseTemplate::new(503), 1).await;
        api_mock(&rig.server, json_reply(200, "{}").set_delay(ms(900)), 1).await;
        assert_eq!(rig.call().await, Err(CorpusError::Timeout));
        assert_eq!(rig.apis().await, 2);
    }

    #[tokio::test]
    async fn a_401_does_not_wait_for_another_call_beyond_its_own_deadline() {
        let server = MockServer::start().await;
        // Premier jeton immédiat, second retardé de 2 s : le renouvellement de B tient le verrou.
        token_mock(&server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(token_reply("tok-2").set_delay(secs(2)))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        // Requête 0 (A) : 401 après 150 ms ; requête 1 (B) : 401 aussitôt ; ensuite : 200.
        let arrived = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&arrived);
        Mock::given(method("POST"))
            .and(path(API_PATH))
            .respond_with(move |_: &Request| match counter.fetch_add(1, Ordering::SeqCst) {
                0 => json_reply(401, "{}").set_delay(ms(150)),
                1 => ResponseTemplate::new(401),
                _ => json_reply(200, "{}"),
            })
            .mount(&server)
            .await;
        let corpus = PisteCorpus::new(config_for(&server.uri(), Some(credentials(SECRET))));
        // Préchauffage : client construit et `tok-1` en cache avant la mesure. Sinon l'échéance de
        // A comprendrait la construction du client (lente sous macOS et Windows) et le premier
        // jeton, et A pourrait expirer avant d'atteindre l'API.
        assert_eq!(corpus.check_credentials().await, Ok(()));
        let call_a = {
            let corpus = corpus.clone();
            Deadline::after(ms(600)).scope(async move {
                let started = Instant::now();
                let result = corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await;
                (result, started.elapsed())
            })
        };
        let task_a = tokio::spawn(call_a);
        // Borné : si l'appel de A échoue avant d'atteindre l'API, le test échoue au lieu de pendre.
        let wait_for_a = async {
            while arrived.load(Ordering::SeqCst) < 1 {
                tokio::time::sleep(ms(5)).await;
            }
        };
        tokio::time::timeout(secs(5), wait_for_a).await.unwrap();
        // B reçoit son 401 avant A, invalide le jeton puis en demande un neuf (2 s) sous le verrou.
        let task_b = {
            let corpus = corpus.clone();
            tokio::spawn(async move {
                corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await
            })
        };
        let (result_a, elapsed_a) = task_a.await.unwrap();
        // L'invalidation de A attend ce verrou, mais pas au-delà de l'échéance de A.
        assert_eq!(result_a, Err(CorpusError::Timeout));
        assert!(elapsed_a < ms(1_500), "{elapsed_a:?}");
        assert_eq!(task_b.await.unwrap(), Ok("{}".to_owned()));
    }

    #[tokio::test]
    async fn a_401_gives_its_permit_back_before_waiting_for_the_token_lock() {
        let server = MockServer::start().await;
        // Premier jeton immédiat, second retardé de 2 s : le renouvellement de B tient le verrou.
        token_mock(&server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(token_reply("tok-2").set_delay(secs(2)))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        // Requête 0 (A) : 401 après 400 ms ; requête 1 (B) : 401 aussitôt ; ensuite : 200.
        let arrived = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&arrived);
        Mock::given(method("POST"))
            .and(path(API_PATH))
            .respond_with(move |_: &Request| match counter.fetch_add(1, Ordering::SeqCst) {
                0 => json_reply(401, "{}").set_delay(ms(400)),
                1 => ResponseTemplate::new(401),
                _ => json_reply(200, "{}"),
            })
            .mount(&server)
            .await;
        let mut setup = Setup::new(server.uri());
        setup.tuning.max_concurrency = NonZeroU16::new(2).unwrap();
        let corpus = PisteCorpus::new(setup.build());
        // Préchauffage : client construit et `tok-1` en cache avant la mesure.
        assert_eq!(corpus.check_credentials().await, Ok(()));
        let gate = &corpus.inner.gate;
        let total = gate.available();
        assert_eq!(total, 2);
        let task_a = {
            let corpus = corpus.clone();
            tokio::spawn(Deadline::after(ms(1_500)).scope(async move {
                corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await
            }))
        };
        let seen = &*arrived;
        let wait_for = |count: usize| async move {
            while seen.load(Ordering::SeqCst) < count {
                tokio::time::sleep(ms(5)).await;
            }
        };
        tokio::time::timeout(secs(5), wait_for(1)).await.unwrap();
        // B reçoit son 401 avant A, invalide le jeton puis en demande un neuf (2 s) sous le
        // verrou, avec le permis de son second envoi.
        let task_b = {
            let corpus = corpus.clone();
            tokio::spawn(async move {
                corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await
            })
        };
        tokio::time::timeout(secs(5), wait_for(2)).await.unwrap();
        // Le 401 de A tombe à 400 ms et son invalidation attend le verrou de B jusqu'à 1,5 s : à
        // 800 ms, A attend encore, et seul le permis de B est tenu.
        tokio::time::sleep(ms(800)).await;
        assert!(!task_a.is_finished());
        assert_eq!(gate.available(), total - 1);
        assert_eq!(task_a.await.unwrap(), Err(CorpusError::Timeout));
        assert_eq!(task_b.await.unwrap(), Ok("{}".to_owned()));
    }

    // ---- Client et requête ---------------------------------------------------------------

    #[tokio::test]
    async fn piste_client_is_built_once_across_calls() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, json_reply(200, "{}")).await;
        let client = &rig.corpus.inner.client;
        assert!(!client.is_built());
        rig.call().await.unwrap();
        let first = std::ptr::from_ref(client.get(&rig.config).await.unwrap());
        rig.call().await.unwrap();
        rig.call().await.unwrap();
        let last = std::ptr::from_ref(client.get(&rig.config).await.unwrap());
        assert_eq!((client.is_built(), first), (true, last));
    }

    #[tokio::test]
    async fn api_request_carries_bearer_accept_and_user_agent() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(API_PATH))
            .and(header("authorization", "Bearer tok-1"))
            .and(header("accept", "application/json"))
            .and(header("content-type", "application/json"))
            .and(header("user-agent", "lf-test/0"))
            .and(body_bytes(BODY))
            .respond_with(json_reply(200, "{}"))
            .expect(1)
            .mount(&rig.server)
            .await;
        assert_eq!(rig.call().await, Ok("{}".to_owned()));
        assert_eq!(rig.apis().await, 1);
    }

    #[test]
    fn api_url_joins_the_template_to_the_base_without_double_slash() {
        let joined = |base: &str, endpoint: &str| {
            api_url(&Url::parse(base).unwrap(), endpoint).unwrap().to_string()
        };
        assert_eq!(
            [
                joined("https://api.example.org/dila/legifrance/lf-engine-app", "/consult/juri"),
                joined("https://api.example.org/dila/legifrance/lf-engine-app/", "/consult/juri"),
                joined("https://api.example.org/", "/consult/juri"),
                joined("https://api.example.org", "/consult/juri"),
                joined("http://127.0.0.1:8080/base", "/a/b/c"),
            ],
            [
                "https://api.example.org/dila/legifrance/lf-engine-app/consult/juri",
                "https://api.example.org/dila/legifrance/lf-engine-app/consult/juri",
                "https://api.example.org/consult/juri",
                "https://api.example.org/consult/juri",
                "http://127.0.0.1:8080/base/a/b/c",
            ]
        );
    }

    // ---- Corps ---------------------------------------------------------------------------

    #[tokio::test]
    async fn mojibake_body_is_repaired_and_counted() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let body = mojibake_json("PREMIÈRE CHAMBRE CIVILE");
        api_always(&rig.server, ResponseTemplate::new(200).set_body_string(body)).await;
        assert_eq!(rig.call().await, Ok(r#"{"texte":"PREMIÈRE CHAMBRE CIVILE"}"#.to_owned()));
        assert_eq!(rig.corpus.mojibake_repairs(), 1);
        assert_eq!(
            rig.corpus.stats(),
            StatsSnapshot { mojibake_repairs: 1, ..StatsSnapshot::default() }
        );
        let log = capture.text();
        assert!(log.contains("WARN"), "{log}");
        assert!(log.contains(r#"endpoint="/consult/juri""#), "{log}");
        assert!(!log.contains("CHAMBRE"), "{log}");
    }

    #[tokio::test]
    async fn mojibake_repair_can_be_switched_off() {
        let rig = Rig::tuned(|setup| setup.tuning.repair_mojibake = false).await;
        token_mock(&rig.server, "tok-1", 1).await;
        let body = mojibake_json("PREMIÈRE CHAMBRE CIVILE");
        api_always(&rig.server, ResponseTemplate::new(200).set_body_string(body.clone())).await;
        assert_eq!(rig.call().await, Ok(body));
        assert_eq!(
            rig.corpus.stats(),
            StatsSnapshot { mojibake_unrepaired: 1, ..StatsSnapshot::default() }
        );
    }

    #[tokio::test]
    async fn clean_body_is_untouched() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let body = r#"{"texte":"PREMIÈRE CHAMBRE CIVILE"}"#;
        api_always(&rig.server, ResponseTemplate::new(200).set_body_string(body)).await;
        assert_eq!(rig.call().await, Ok(body.to_owned()));
        assert_eq!(rig.corpus.stats(), StatsSnapshot::default());
        let log = capture.text();
        assert!(!log.contains("WARN"), "{log}");
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let rig = Rig::tuned(|setup| setup.tuning.max_body_bytes = 1_000).await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, ResponseTemplate::new(200).set_body_string("a".repeat(2_000)))
            .await;
        assert_eq!(
            rig.call().await,
            Err(CorpusError::Decode {
                detail: "response too large (> 1000 bytes) on /consult/juri".to_owned()
            })
        );
        assert_eq!(rig.apis().await, 1);
        assert_eq!(rig.sleeper.waits(), Vec::<Duration>::new());
    }

    #[tokio::test]
    async fn invalid_utf8_body_maps_to_decode() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        api_always(&rig.server, ResponseTemplate::new(200).set_body_bytes(vec![b'{', 0xff, b'}']))
            .await;
        assert_eq!(
            rig.call().await,
            Err(CorpusError::Decode {
                detail: "invalid UTF-8 at byte 1 on /consult/juri".to_owned()
            })
        );
    }

    #[tokio::test]
    async fn clones_share_one_state() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let body = mojibake_json("PREMIÈRE CHAMBRE CIVILE");
        api_always(&rig.server, ResponseTemplate::new(200).set_body_string(body)).await;
        let twin = rig.corpus.clone();
        twin.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await.unwrap();
        rig.call().await.unwrap();
        assert_eq!(
            (rig.tokens().await, rig.corpus.mojibake_repairs(), twin.mojibake_repairs()),
            (1, 2, 2)
        );
    }

    // ---- Aucun secret --------------------------------------------------------------------

    /// Préfixe et nanosecondes d'horloge murale en hexadécimal : imprévisible, sans dépendance.
    fn random_word(prefix: &str) -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        format!("{prefix}-{nanos:x}")
    }

    /// Les trois valeurs que le serveur recopie dans ses corps d'erreur.
    struct Leak {
        secret: String,
        token: String,
        marker: String,
    }

    impl Leak {
        fn new() -> Self {
            Self {
                secret: random_word("s3cr3t"),
                token: random_word("t0k3n"),
                marker: random_word("b0dy"),
            }
        }

        fn body(&self) -> String {
            let Self { secret, token, marker } = self;
            format!("{secret} {token} {marker} Authorization: Bearer {token}")
        }

        fn reply(&self, status: u16) -> ResponseTemplate {
            ResponseTemplate::new(status).set_body_string(self.body())
        }

        async fn rig(&self, tune: impl FnOnce(&mut Setup)) -> Rig {
            Rig::tuned(|setup| {
                setup.credentials = Some(credentials(&self.secret));
                tune(setup);
            })
            .await
        }
    }

    #[tokio::test]
    async fn error_messages_never_contain_secret_or_token() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        let leak = Leak::new();
        let mut shown: Vec<String> = Vec::new();

        // L'API répond en erreur, corps recopiant les secrets.
        for status in [400, 403, 404, 418, 429, 500, 503] {
            let rig = leak.rig(|_| {}).await;
            token_mock(&rig.server, &leak.token, 1).await;
            api_always(&rig.server, leak.reply(status)).await;
            shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        }
        // 401 deux fois.
        let rig = leak.rig(|_| {}).await;
        token_mock(&rig.server, &leak.token, 2).await;
        api_mock(&rig.server, leak.reply(401), 2).await;
        shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        // Le point d'accès du jeton répond en erreur, par `post_json` et par `check_credentials`.
        for status in [400, 401, 403, 404, 200] {
            let rig = leak.rig(|_| {}).await;
            token_always(&rig.server, leak.reply(status)).await;
            shown.extend(rig.describe(&rig.call().await.unwrap_err()));
            let rig = leak.rig(|_| {}).await;
            token_always(&rig.server, leak.reply(status)).await;
            shown.extend(rig.describe(&rig.corpus.check_credentials().await.unwrap_err()));
        }
        // Corps trop gros, UTF-8 invalide.
        let rig = leak.rig(|setup| setup.tuning.max_body_bytes = 1_000).await;
        token_mock(&rig.server, &leak.token, 1).await;
        let huge = ResponseTemplate::new(200).set_body_string(leak.body().repeat(40));
        api_always(&rig.server, huge).await;
        shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        let rig = leak.rig(|_| {}).await;
        token_mock(&rig.server, &leak.token, 1).await;
        let invalid = [leak.body().into_bytes(), vec![0xff]].concat();
        api_always(&rig.server, ResponseTemplate::new(200).set_body_bytes(invalid)).await;
        shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        // Port fermé.
        let closed = ClosedPort::new();
        let dead_uri = closed.uri();
        let rig = leak.rig(|setup| setup.uri.clone_from(&dead_uri)).await;
        shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        // Mode dégradé, deux fois.
        let rig = leak.rig(|setup| setup.credentials = None).await;
        shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        let problem = ConfigProblem::InsecureScheme { setting: "base URL" };
        let rig = leak.rig(|setup| setup.problem = Some(problem.clone())).await;
        shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        // Débit local épuisé.
        let rig = leak
            .rig(|setup| {
                setup.tuning.rate = rate(1, 1);
                setup.tuning.tool_deadline = ms(500);
            })
            .await;
        token_mock(&rig.server, &leak.token, 1).await;
        api_always(&rig.server, json_reply(200, "{}")).await;
        rig.call().await.unwrap();
        shown.extend(rig.describe(&rig.call().await.unwrap_err()));
        // Un succès réparé : seul le journal est à contrôler.
        let rig = leak.rig(|_| {}).await;
        token_mock(&rig.server, &leak.token, 1).await;
        let repaired = mojibake_json(&format!("{} PREMIÈRE", leak.marker));
        api_always(&rig.server, ResponseTemplate::new(200).set_body_string(repaired)).await;
        rig.call().await.unwrap();
        shown.push(format!("{:?}", rig.corpus));
        shown.push(capture.text());

        let count = shown.len();
        assert!(count > 20, "{count}");
        let haystack = shown.join("\n").to_lowercase();
        for forbidden in [&leak.secret, &leak.token, &leak.marker] {
            assert!(!haystack.contains(&forbidden.to_lowercase()), "{forbidden}");
        }
        assert!(!haystack.contains("authorization"));
    }

    // ---- Corps d'erreur et « introuvable » (T-M2-08, 04 §11) ------------------------------

    const MARKER_BODY: &str = r#"{"timestamp":1,"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null,"exceptionDetails":null}"#;

    /// L'API répond `template` une fois exactement, à `{API_BASE}{endpoint}`.
    async fn api_mock_at(server: &MockServer, endpoint: &str, template: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .respond_with(template)
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
    }

    /// Un appel `post_json` sur `rig`, le jeton monté une fois et l'API répondant `template` une
    /// fois.
    async fn call_on(
        rig: &Rig,
        kind: EndpointKind,
        endpoint: &str,
        template: ResponseTemplate,
    ) -> Result<String, CorpusError> {
        token_mock(&rig.server, "tok-1", 1).await;
        api_mock_at(&rig.server, endpoint, template).await;
        rig.corpus.post_json(kind, endpoint, BODY, owned).await
    }

    async fn call_once(
        kind: EndpointKind,
        endpoint: &str,
        template: ResponseTemplate,
    ) -> (Rig, Result<String, CorpusError>) {
        let rig = Rig::new().await;
        let result = call_on(&rig, kind, endpoint, template).await;
        (rig, result)
    }

    /// Le texte lu comme du latin-1 : le mojibake que PISTE a déjà servi.
    fn mojibake(text: &str) -> String {
        text.bytes().map(char::from).collect()
    }

    fn invalid_request(detail: &str) -> CorpusError {
        CorpusError::InvalidRequest { status: Some(400), detail: detail.to_owned() }
    }

    #[tokio::test]
    async fn unknown_juri_id_400_maps_to_not_found() {
        let (rig, result) =
            call_once(EndpointKind::Consult, ENDPOINT, json_reply(400, MARKER_BODY)).await;
        assert_eq!(result, Err(CorpusError::NotFound { what: ENDPOINT.to_owned() }));
        assert_eq!(rig.sleeper.waits(), vec![]);
    }

    #[tokio::test]
    async fn the_400_marker_means_not_found_only_on_a_consultation() {
        let (_rig, result) =
            call_once(EndpointKind::Search, "/search", json_reply(400, MARKER_BODY)).await;
        assert_eq!(result, Err(invalid_request("HTTP 400 on /search")));
    }

    #[tokio::test]
    async fn error_body_marker_survives_mojibake() {
        for repair in [true, false] {
            let rig = Rig::tuned(|setup| setup.tuning.repair_mojibake = repair).await;
            let template = json_reply(400, &mojibake(MARKER_BODY));
            let result = call_on(&rig, EndpointKind::Consult, ENDPOINT, template).await;
            assert_eq!(
                result,
                Err(CorpusError::NotFound { what: ENDPOINT.to_owned() }),
                "repair {repair}"
            );
            // Un corps d'erreur réparé n'est pas compté : `classify` est pur, sans compteur.
            assert_eq!(rig.corpus.stats(), StatsSnapshot::default(), "repair {repair}");
        }
    }

    #[tokio::test]
    async fn the_400_detail_follows_the_mojibake_setting() {
        let body = format!(r#"{{"error":400,"message":"{}"}}"#, mojibake("Facette inconnue : é"));
        for (repair, detail) in
            [(true, "Facette inconnue : é".to_owned()), (false, mojibake("Facette inconnue : é"))]
        {
            let rig = Rig::tuned(|setup| setup.tuning.repair_mojibake = repair).await;
            let result =
                call_on(&rig, EndpointKind::Consult, ENDPOINT, json_reply(400, &body)).await;
            assert_eq!(result, Err(invalid_request(&detail)), "repair {repair}");
        }
    }

    #[tokio::test]
    async fn the_400_is_not_retried_and_a_bodyless_one_keeps_the_generic_detail() {
        let (rig, result) =
            call_once(EndpointKind::Consult, ENDPOINT, ResponseTemplate::new(400)).await;
        assert_eq!(result, Err(invalid_request("HTTP 400 on /consult/juri")));
        assert_eq!(rig.apis().await, 1);
    }

    #[tokio::test]
    async fn a_400_message_repeating_the_token_or_the_secret_never_reaches_the_detail() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        let leak = Leak::new();
        let rig = leak.rig(|_| {}).await;
        token_mock(&rig.server, &leak.token, 1).await;
        let message = format!("rejected Bearer {} for {} !", leak.token, leak.secret);
        let body = serde_json::json!({ "error": 400, "message": message }).to_string();
        api_mock_at(&rig.server, ENDPOINT, json_reply(400, &body)).await;
        let error =
            rig.corpus.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned).await.unwrap_err();
        assert_eq!(error, invalid_request("rejected Bearer [redacted] for [redacted] !"));
        let mut shown = rig.describe(&error).to_vec();
        shown.push(capture.text());
        let haystack = shown.join("\n").to_lowercase();
        for forbidden in [&leak.token, &leak.secret] {
            assert!(!haystack.contains(&forbidden.to_lowercase()), "{forbidden}");
        }
    }

    #[tokio::test]
    async fn the_404_on_a_custom_host_is_config_for_every_kind() {
        for (kind, endpoint) in
            [(EndpointKind::Consult, ENDPOINT), (EndpointKind::Search, "/search")]
        {
            let (rig, result) = call_once(kind, endpoint, ResponseTemplate::new(404)).await;
            assert_eq!(
                result,
                Err(CorpusError::Config { detail: format!("HTTP 404 on {endpoint}") }),
                "{endpoint}"
            );
            let route = format!("{API_BASE}{endpoint}");
            assert_eq!(requests_to(&rig.server, &route).await, 1, "{endpoint}");
        }
    }

    #[tokio::test]
    async fn empty_2xx_body_on_consult_is_not_found() {
        for template in [
            ResponseTemplate::new(200),
            ResponseTemplate::new(204),
            ResponseTemplate::new(200).set_body_string(" \n"),
        ] {
            let (_rig, result) = call_once(EndpointKind::Consult, ENDPOINT, template).await;
            assert_eq!(result, Err(CorpusError::NotFound { what: ENDPOINT.to_owned() }));
        }
    }

    #[tokio::test]
    async fn empty_2xx_body_on_search_is_decode() {
        let (_rig, result) =
            call_once(EndpointKind::Search, "/search", ResponseTemplate::new(200)).await;
        assert_eq!(
            result,
            Err(CorpusError::Decode { detail: "empty response body on /search".to_owned() })
        );
    }

    // ---- Compilation ---------------------------------------------------------------------

    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    fn assert_send<T: Send>(_: T) {}

    #[test]
    fn piste_corpus_is_send_sync_static() {
        assert_send_sync_static::<PisteCorpus>();
        assert_send_sync_static::<TestCorpus>();
        let production = PisteCorpus::new(config_for("http://127.0.0.1:9", None));
        assert_send(production.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned));
        assert_send(production.check_credentials());
        let clock = ManualClock::new();
        let sleeper = RecordingSleeper::new(clock.clone());
        let manual = TestCorpus::with_parts(config_for("http://127.0.0.1:9", None), clock, sleeper);
        assert_send(manual.post_json(EndpointKind::Consult, ENDPOINT, BODY, owned));
        assert_send(manual.check_credentials());
    }
}
