//! Configuration de l'adapter PISTE et validation des URL surchargées (04 §2, T-M2-01).
//!
//! L'adapter reçoit des valeurs déjà typées : la lecture des variables `LF_*` et
//! `FR_ELI_*`, les bornes 1..=100 et les avertissements « ignored » sont à `lf-app`.
//! Rien n'est lu dans l'environnement ici. Un [`ConfigProblem`] n'empêche pas de
//! construire une [`PisteConfig`] : il reste dans `endpoints` et met le serveur en mode
//! dégradé (04 §2.4, D-39).
//!
//! Aucun message d'erreur ne recopie un morceau d'une URL (une URL surchargée peut porter des
//! identifiants) : seuls l'hôte officiel, l'hôte d'une surcharge dans
//! [`ConfigWarning::CustomHost`] et la raison fixe de l'analyseur d'URL y figurent. Seule valeur
//! citée : celle de `LF_PISTE_ENV` refusée ([`ConfigProblem::InvalidEnv`], texte de 05 §7.2), et
//! seulement si elle fait 1 à 16 lettres ASCII ; sinon, sa longueur seule
//! ([`ConfigProblem::invalid_env`]).
//!
//! Le `Display` d'un [`ConfigProblem`] devient le `detail` de `CorpusError::Config` de nature
//! `Settings`, que `lf-application::messages` rend tel quel au modèle : les textes que 05 §7.2
//! fixe (`InvalidEnv`, `EnvMismatch`) y sont repris au caractère près, point final compris.

use std::fmt;
use std::num::{NonZeroU16, NonZeroU32};
use std::time::Duration;

use lf_domain::coverage::PisteEnvironment;
use reqwest::Url;
use secrecy::SecretString;
use thiserror::Error;

/// Chemin de l'API Légifrance sur les hôtes API officiels de PISTE.
const OFFICIAL_API_PATH: &str = "/dila/legifrance/lf-engine-app";
/// URL du jeton OAuth, bac à sable.
const SANDBOX_OAUTH_URL: &str = "https://sandbox-oauth.piste.gouv.fr/api/oauth/token";
/// URL du jeton OAuth, production.
const PRODUCTION_OAUTH_URL: &str = "https://oauth.piste.gouv.fr/api/oauth/token";
/// URL de base de l'API, bac à sable.
const SANDBOX_BASE_URL: &str = "https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app";
/// URL de base de l'API, production.
const PRODUCTION_BASE_URL: &str = "https://api.piste.gouv.fr/dila/legifrance/lf-engine-app";

/// Délai de connexion TCP et TLS.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Délai d'une requête HTTP complète.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);
/// Délai total d'un appel d'outil, réessais compris.
const TOOL_DEADLINE: Duration = Duration::from_secs(60);
/// Débit soutenu, en requêtes par seconde.
const RATE_PER_SECOND: NonZeroU32 = NonZeroU32::new(5).unwrap();
/// Rafale admise par le limiteur de débit.
const RATE_BURST: NonZeroU32 = NonZeroU32::new(10).unwrap();
/// Requêtes simultanées au plus.
const MAX_CONCURRENCY: NonZeroU16 = NonZeroU16::new(4).unwrap();
/// Taille maximale d'un corps de réponse (00 §8).
const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;
/// Longueur, en octets (donc en lettres ASCII), d'une valeur de `LF_PISTE_ENV` refusée que le
/// message peut encore citer (M-1) : au-delà, ce n'est plus une faute de frappe de `sandbox` ou
/// de `production`, et la valeur pourrait être un secret collé par erreur.
const ENV_QUOTED_MAX_LEN: usize = 16;

/// Hôte officiel de PISTE.
struct OfficialHost {
    /// Nom d'hôte, en minuscules.
    host: &'static str,
    /// Environnement auquel l'hôte appartient.
    env: PisteEnvironment,
    /// Vrai pour un hôte d'API, dont le chemin est imposé.
    api: bool,
}

/// Les quatre hôtes officiels : jeton et API, bac à sable et production.
const OFFICIAL_HOSTS: [OfficialHost; 4] = [
    OfficialHost {
        host: "sandbox-oauth.piste.gouv.fr",
        env: PisteEnvironment::Sandbox,
        api: false,
    },
    OfficialHost { host: "oauth.piste.gouv.fr", env: PisteEnvironment::Production, api: false },
    OfficialHost { host: "sandbox-api.piste.gouv.fr", env: PisteEnvironment::Sandbox, api: true },
    OfficialHost { host: "api.piste.gouv.fr", env: PisteEnvironment::Production, api: true },
];

/// Rôle d'une URL de la configuration.
#[derive(Debug, Clone, Copy)]
enum Role {
    /// URL du jeton OAuth.
    Oauth,
    /// URL de base de l'API.
    Base,
}

impl Role {
    /// Libellé du rôle dans les messages (pas un nom de variable : `lf-app` peut le préfixer).
    fn setting(self) -> &'static str {
        match self {
            Self::Oauth => "OAuth URL",
            Self::Base => "base URL",
        }
    }

    /// Variable d'environnement de l'URL, que nomme le message d'[`ConfigProblem::EnvMismatch`]
    /// (05 §7.2 : il demande de corriger `LF_PISTE_ENV` ou cette URL).
    fn variable(self) -> &'static str {
        match self {
            Self::Oauth => "LF_PISTE_OAUTH_URL",
            Self::Base => "LF_PISTE_BASE_URL",
        }
    }
}

/// Avertissement sur une URL surchargée acceptée.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigWarning {
    /// URL en `http` clair, tolérée parce que l'hôte est une boucle locale.
    ///
    /// Avec `HTTP_PROXY`/`HTTPS_PROXY` posés et sans `NO_PROXY` couvrant la boucle locale, le
    /// formulaire OAuth et l'en-tête `Authorization` partent vers le proxy, en clair : la
    /// boucle locale n'est pas contournée d'office (04 §4.2).
    #[error("{setting} uses plain http on a loopback host")]
    InsecureLoopback {
        /// Libellé de l'URL concernée.
        setting: &'static str,
    },
    /// Hôte qui n'est pas l'un des hôtes officiels de PISTE.
    #[error("{setting} points to a non-official host {host}")]
    CustomHost {
        /// Libellé de l'URL concernée.
        setting: &'static str,
        /// Hôte de la surcharge.
        host: String,
    },
}

/// Réglage refusé (valeur de `LF_PISTE_ENV`, URL surchargée) : le serveur passe en mode dégradé.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigProblem {
    /// Valeur de `LF_PISTE_ENV` qui n'est ni `sandbox` ni `production` (04 §2.2), construite par
    /// `lf-app`, seul à lire l'environnement, par [`ConfigProblem::invalid_env`]. Texte de 05 §7.2.
    #[error("{value}")]
    InvalidEnv {
        /// Ce qu'un message peut montrer de la valeur lue ; jamais la valeur brute.
        value: RejectedEnv,
    },
    /// Texte qui n'est pas une URL.
    #[error("invalid {setting}: {reason}")]
    InvalidUrl {
        /// Libellé de l'URL concernée.
        setting: &'static str,
        /// Raison donnée par l'analyseur d'URL.
        reason: String,
    },
    /// Schéma autre que `https` (`http` n'est admis que sur une boucle locale).
    #[error("{setting} must use https (http is only accepted for loopback hosts)")]
    InsecureScheme {
        /// Libellé de l'URL concernée.
        setting: &'static str,
    },
    /// URL qui porte des identifiants, une requête ou un fragment.
    #[error("{setting} must not contain {part}")]
    ForbiddenPart {
        /// Libellé de l'URL concernée.
        setting: &'static str,
        /// Partie refusée : `credentials`, `query` ou `fragment`.
        part: &'static str,
    },
    /// Hôte officiel d'un autre environnement que celui configuré. Texte de 05 §7.2 : la
    /// variable à corriger, sans l'hôte, et aucune déduction implicite de l'environnement.
    #[error(
        "{variable} points to the PISTE {host_env} host but LF_PISTE_ENV is {env}: \
         set LF_PISTE_ENV={host_env}, or fix the URL."
    )]
    EnvMismatch {
        /// Variable d'environnement de l'URL concernée : `LF_PISTE_OAUTH_URL` ou
        /// `LF_PISTE_BASE_URL`.
        variable: &'static str,
        /// Hôte officiel de l'URL.
        host: String,
        /// Environnement configuré.
        env: PisteEnvironment,
        /// Environnement auquel l'hôte appartient.
        host_env: PisteEnvironment,
    },
    /// Chemin différent de celui de l'API sur un hôte API officiel.
    #[error("{setting} path on official host {host} must be /dila/legifrance/lf-engine-app")]
    UnexpectedApiPath {
        /// Libellé de l'URL concernée.
        setting: &'static str,
        /// Hôte API officiel de l'URL.
        host: String,
    },
}

impl ConfigProblem {
    /// `LF_PISTE_ENV` refusée : seule une faute de frappe plausible (1 à 16 lettres ASCII :
    /// `prod`, `Production`) est gardée pour être citée ; de toute autre valeur, il ne reste que
    /// le nombre de caractères (M-1, dérogation à la citation de 05 §7.2).
    ///
    /// Un secret collé là par erreur (un identifiant de 36 caractères, une ligne de `piste.env`
    /// entière) n'atteint donc ni le modèle, ni le journal d'audit, ni le `Debug` : la valeur
    /// brute n'est jamais conservée. Une valeur citée n'a rien à échapper ni à couper.
    #[must_use]
    pub fn invalid_env(raw: &str) -> Self {
        let plausible = !raw.is_empty()
            && raw.len() <= ENV_QUOTED_MAX_LEN
            && raw.bytes().all(|byte| byte.is_ascii_alphabetic());
        let shown = if plausible {
            Shown::Quoted(raw.to_owned())
        } else {
            Shown::Hidden { chars: raw.chars().count() }
        };
        Self::InvalidEnv { value: RejectedEnv(shown) }
    }
}

/// Valeur refusée de `LF_PISTE_ENV`, réduite à ce qu'un message peut en montrer. Seul
/// [`ConfigProblem::invalid_env`] la construit (champ privé) ; son `Display` est le texte entier
/// de [`ConfigProblem::InvalidEnv`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedEnv(Shown);

/// Ce qui reste de la valeur : elle-même (faute de frappe plausible) ou sa longueur.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shown {
    /// 1 à 16 lettres ASCII, citées telles quelles.
    Quoted(String),
    /// Toute autre valeur : son nombre de caractères (points de code).
    Hidden {
        /// Nombre de caractères de la valeur lue.
        chars: usize,
    },
}

impl fmt::Display for RejectedEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Shown::Quoted(value) => {
                write!(f, "LF_PISTE_ENV='{value}' is invalid: expected sandbox or production.")
            }
            Shown::Hidden { chars } => write!(
                f,
                "LF_PISTE_ENV is invalid (value not shown, {chars} characters): expected sandbox \
                 or production."
            ),
        }
    }
}

/// Adresses de l'adapter : jeton OAuth et base de l'API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PisteEndpoints {
    /// URL du jeton OAuth.
    oauth_url: Url,
    /// URL de base de l'API, sans `/` final : un endpoint s'y concatène, il ne s'y joint pas.
    base_url: Url,
    /// Vrai quand les deux URL sont sur des hôtes officiels (une surcharge absente compte
    /// comme officielle).
    official: bool,
}

impl PisteEndpoints {
    /// Adresses officielles de `env` (04 §2.1).
    #[must_use]
    pub fn official(env: PisteEnvironment) -> Self {
        Self {
            oauth_url: official_url(env, Role::Oauth),
            base_url: official_url(env, Role::Base),
            official: true,
        }
    }

    /// Adresses de `env`, avec surcharges éventuelles du jeton et de la base (04 §2.3).
    ///
    /// Une surcharge absente donne l'URL officielle de `env`. Les avertissements sont ceux
    /// du jeton puis ceux de la base.
    ///
    /// # Errors
    ///
    /// Le premier [`ConfigProblem`] rencontré, en examinant le jeton puis la base.
    pub fn with_overrides(
        env: PisteEnvironment,
        oauth: Option<&str>,
        base: Option<&str>,
    ) -> Result<(Self, Vec<ConfigWarning>), ConfigProblem> {
        let oauth = check_optional(env, Role::Oauth, oauth)?;
        let base = check_optional(env, Role::Base, base)?;
        let official = oauth.official && base.official;
        let mut warnings = oauth.warnings;
        warnings.extend(base.warnings);
        Ok((Self { oauth_url: oauth.url, base_url: base.url, official }, warnings))
    }

    /// URL du jeton OAuth.
    #[must_use]
    pub fn oauth_url(&self) -> &Url {
        &self.oauth_url
    }

    /// URL de base de l'API, sans `/` final : un endpoint s'y concatène, il ne s'y joint pas.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Vrai quand les deux URL sont sur des hôtes officiels (une surcharge absente compte
    /// comme officielle).
    #[must_use]
    pub fn is_official(&self) -> bool {
        self.official
    }
}

/// Une URL validée, avec ses avertissements.
struct Checked {
    /// L'URL, normalisée.
    url: Url,
    /// Avertissements de cette URL, dans l'ordre.
    warnings: Vec<ConfigWarning>,
    /// Vrai quand l'hôte est l'un des hôtes officiels.
    official: bool,
}

/// Valide la surcharge de `role`, ou prend l'URL officielle de `env` si elle est absente.
fn check_optional(
    env: PisteEnvironment,
    role: Role,
    raw: Option<&str>,
) -> Result<Checked, ConfigProblem> {
    match raw {
        None => Ok(Checked { url: official_url(env, role), warnings: Vec::new(), official: true }),
        Some(raw) => check_url(env, role, raw),
    }
}

/// Applique à une surcharge les règles de 04 §2.3, dans l'ordre : analyse, schéma, parties
/// interdites, environnement, puis (base seulement) `/` finaux et chemin de l'API.
fn check_url(env: PisteEnvironment, role: Role, raw: &str) -> Result<Checked, ConfigProblem> {
    let setting = role.setting();
    let mut url = Url::parse(raw)
        .map_err(|error| ConfigProblem::InvalidUrl { setting, reason: error.to_string() })?;
    let loopback = check_scheme(setting, &url)?;
    check_parts(setting, &url)?;
    // Un nom de domaine absolu (`api.piste.gouv.fr.`) désigne le même hôte : UN point final
    // est retiré avant la comparaison, pour que `EnvMismatch` et `official` se comportent
    // comme sans lui (le garde-fou ne se contourne pas par une faute de saisie). Le port
    // reste ignoré. L'URL elle-même n'est pas réécrite.
    let host = url.host_str().unwrap_or_default();
    let host = host.strip_suffix('.').unwrap_or(host).to_owned();
    let official = OFFICIAL_HOSTS.iter().find(|official| official.host == host);
    check_env(role, env, &host, official)?;
    if matches!(role, Role::Base) {
        trim_trailing_slashes(&mut url);
    }
    check_api_path(setting, &url, &host, official)?;

    let mut warnings = Vec::new();
    if loopback {
        warnings.push(ConfigWarning::InsecureLoopback { setting });
    }
    if official.is_none() {
        warnings.push(ConfigWarning::CustomHost { setting, host });
    }
    Ok(Checked { url, warnings, official: official.is_some() })
}

/// Vrai pour une URL `http` de boucle locale, faux pour `https`, refus sinon.
fn check_scheme(setting: &'static str, url: &Url) -> Result<bool, ConfigProblem> {
    match url.scheme() {
        "https" => Ok(false),
        "http" if matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost")) => Ok(true),
        _ => Err(ConfigProblem::InsecureScheme { setting }),
    }
}

/// Refuse identifiants, requête et fragment, dans cet ordre.
fn check_parts(setting: &'static str, url: &Url) -> Result<(), ConfigProblem> {
    let part = if !url.username().is_empty() || url.password().is_some() {
        "credentials"
    } else if url.query().is_some() {
        "query"
    } else if url.fragment().is_some() {
        "fragment"
    } else {
        return Ok(());
    };
    Err(ConfigProblem::ForbiddenPart { setting, part })
}

/// Refuse un hôte officiel d'un autre environnement que `env` ; l'hôte ne sert jamais à
/// déduire l'environnement.
fn check_env(
    role: Role,
    env: PisteEnvironment,
    host: &str,
    official: Option<&OfficialHost>,
) -> Result<(), ConfigProblem> {
    match official {
        Some(official) if official.env != env => Err(ConfigProblem::EnvMismatch {
            variable: role.variable(),
            host: host.to_owned(),
            env,
            host_env: official.env,
        }),
        _ => Ok(()),
    }
}

/// Retire les `/` finaux du chemin ; la racine reste `/` (`set_path("")` redonne `/`).
fn trim_trailing_slashes(url: &mut Url) {
    let trimmed = url.path().trim_end_matches('/').to_owned();
    url.set_path(&trimmed);
}

/// Sur un hôte API officiel, impose le chemin de l'API Légifrance.
fn check_api_path(
    setting: &'static str,
    url: &Url,
    host: &str,
    official: Option<&OfficialHost>,
) -> Result<(), ConfigProblem> {
    match official {
        Some(official) if official.api && url.path() != OFFICIAL_API_PATH => {
            Err(ConfigProblem::UnexpectedApiPath { setting, host: host.to_owned() })
        }
        _ => Ok(()),
    }
}

/// Analyse une constante d'URL officielle.
#[expect(
    clippy::expect_used,
    reason = "constantes littérales du fichier, toutes analysées par official_endpoints_match_spec"
)]
fn parse_official(raw: &'static str) -> Url {
    Url::parse(raw).expect("official URL constant is valid")
}

/// URL officielle de `env` pour `role`.
fn official_url(env: PisteEnvironment, role: Role) -> Url {
    parse_official(match (env, role) {
        (PisteEnvironment::Sandbox, Role::Oauth) => SANDBOX_OAUTH_URL,
        (PisteEnvironment::Production, Role::Oauth) => PRODUCTION_OAUTH_URL,
        (PisteEnvironment::Sandbox, Role::Base) => SANDBOX_BASE_URL,
        (PisteEnvironment::Production, Role::Base) => PRODUCTION_BASE_URL,
    })
}

/// Identifiants OAuth `client_credentials` de l'application PISTE.
#[derive(Debug, Clone)]
pub struct PisteCredentials {
    /// Identifiant du client (non secret).
    client_id: String,
    /// Secret du client ; son `Debug` est rédigé (`[REDACTED]`).
    client_secret: SecretString,
}

impl PisteCredentials {
    /// Assemble les identifiants ; ne décide de rien : `lf-app` retire les blancs et écarte
    /// les valeurs vides avant d'appeler.
    #[must_use]
    pub fn new(client_id: String, client_secret: SecretString) -> Self {
        Self { client_id, client_secret }
    }

    /// Identifiant du client (non secret).
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Secret du client ; son `Debug` est rédigé (`[REDACTED]`).
    #[must_use]
    pub fn client_secret(&self) -> &SecretString {
        &self.client_secret
    }
}

/// Quota de débit vers PISTE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateQuota {
    /// Débit soutenu, en requêtes par seconde.
    per_second: NonZeroU32,
    /// Rafale admise.
    burst: NonZeroU32,
}

impl RateQuota {
    /// Assemble le quota ; les deux valeurs sont non nulles par leur type. Aucun ordre n'est
    /// imposé entre elles : `governor` accepte tout couple non nul.
    #[must_use]
    pub fn new(per_second: NonZeroU32, burst: NonZeroU32) -> Self {
        Self { per_second, burst }
    }

    /// Débit soutenu, en requêtes par seconde.
    #[must_use]
    pub fn per_second(&self) -> NonZeroU32 {
        self.per_second
    }

    /// Rafale admise.
    #[must_use]
    pub fn burst(&self) -> NonZeroU32 {
        self.burst
    }
}

/// Réglages avancés de l'adapter : délais, débit, concurrence, taille de corps.
///
/// L'adapter ne refuse que les états absurdes (voir [`PisteConfig::with_tuning`]) : un délai
/// nul fait échouer toute requête, une borne de corps nulle refuse toute réponse. Le débit et
/// la concurrence sont non nuls par leur type. Ne sont **volontairement pas imposés** :
/// - les bornes hautes (`LF_HTTP_TIMEOUT_S` 1..=300, `LF_TOOL_DEADLINE_S` 1..=600,
///   `LF_PISTE_RPS` et `LF_PISTE_BURST` 1..=100, `LF_PISTE_CONCURRENCY` 1..=16) : politique
///   d'utilisateur, que `lf-app` applique avant d'appeler (04 §2.2) ;
/// - un ordre entre les délais : `CallDeadline::cap` plafonne déjà chaque tentative ;
/// - `burst >= per_second` : `governor` accepte tout couple non nul ;
/// - `repair_mojibake` : booléen libre ;
/// - la validité du `User-Agent` comme en-tête, vérifiée à la construction du client.
///
/// Les champs sont publics : la validation a lieu à l'entrée de la configuration, et
/// [`PisteConfig::tuning`] ne rend qu'une référence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PisteTuning {
    /// Délai de connexion TCP et TLS.
    pub connect_timeout: Duration,
    /// Délai d'une requête HTTP complète.
    pub request_timeout: Duration,
    /// Délai total d'un appel d'outil, réessais compris.
    pub tool_deadline: Duration,
    /// Quota de débit.
    pub rate: RateQuota,
    /// Requêtes simultanées au plus.
    pub max_concurrency: NonZeroU16,
    /// Taille maximale d'un corps de réponse, en octets.
    pub max_body_bytes: u64,
    /// Vrai pour réparer le texte dont l'encodage est abîmé (mojibake).
    pub repair_mojibake: bool,
}

impl Default for PisteTuning {
    /// Valeurs par défaut de 04 §2.2 ; ne décide de rien.
    fn default() -> Self {
        Self {
            connect_timeout: CONNECT_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            tool_deadline: TOOL_DEADLINE,
            rate: RateQuota { per_second: RATE_PER_SECOND, burst: RATE_BURST },
            max_concurrency: MAX_CONCURRENCY,
            max_body_bytes: MAX_BODY_BYTES,
            repair_mojibake: true,
        }
    }
}

/// Réglage refusé par [`PisteConfig::with_tuning`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TuningError {
    /// Réglage nul alors qu'une valeur strictement positive est exigée.
    #[error("{setting} must not be zero")]
    Zero {
        /// Libellé du réglage : `connect timeout`, `request timeout`, `tool deadline` ou
        /// `max body bytes`.
        setting: &'static str,
    },
}

/// Contrôle les réglages dans l'ordre connexion, requête, appel, corps ; nomme le premier nul.
fn check_tuning(tuning: &PisteTuning) -> Result<(), TuningError> {
    let setting = if tuning.connect_timeout.is_zero() {
        "connect timeout"
    } else if tuning.request_timeout.is_zero() {
        "request timeout"
    } else if tuning.tool_deadline.is_zero() {
        "tool deadline"
    } else if tuning.max_body_bytes == 0 {
        "max body bytes"
    } else {
        return Ok(());
    };
    Err(TuningError::Zero { setting })
}

/// Configuration complète de l'adapter PISTE.
#[derive(Debug, Clone)]
pub struct PisteConfig {
    /// Environnement PISTE configuré.
    env: PisteEnvironment,
    /// Adresses, ou le problème qui les a fait refuser (mode dégradé).
    endpoints: Result<PisteEndpoints, ConfigProblem>,
    /// Identifiants OAuth, absents tant qu'ils ne sont pas configurés.
    credentials: Option<PisteCredentials>,
    /// En-tête `User-Agent` des requêtes.
    user_agent: String,
    /// Réglages avancés, validés par [`PisteConfig::with_tuning`].
    tuning: PisteTuning,
}

impl PisteConfig {
    /// Assemble la configuration avec les réglages par défaut de 04 §2.2 ; ne décide de rien.
    #[must_use]
    pub fn new(
        env: PisteEnvironment,
        endpoints: Result<PisteEndpoints, ConfigProblem>,
        credentials: Option<PisteCredentials>,
        user_agent: String,
    ) -> Self {
        Self { env, endpoints, credentials, user_agent, tuning: PisteTuning::default() }
    }

    /// Remplace les réglages avancés, le reste de la configuration est conservé.
    ///
    /// La configuration est **consommée même en cas d'erreur** : l'appelant qui veut un repli la
    /// clone avant l'appel (`PisteConfig` est `Clone`).
    ///
    /// # Errors
    ///
    /// [`TuningError::Zero`] pour le premier réglage nul, dans l'ordre : délai de connexion,
    /// délai de requête, délai d'appel, taille maximale du corps.
    pub fn with_tuning(self, tuning: PisteTuning) -> Result<Self, TuningError> {
        check_tuning(&tuning)?;
        Ok(Self { tuning, ..self })
    }

    /// Environnement PISTE configuré.
    #[must_use]
    pub fn env(&self) -> PisteEnvironment {
        self.env
    }

    /// Adresses, ou le problème qui les a fait refuser (mode dégradé).
    ///
    /// # Errors
    ///
    /// Le [`ConfigProblem`] retenu à la construction, quand les URL surchargées ont été
    /// refusées.
    pub fn endpoints(&self) -> Result<&PisteEndpoints, &ConfigProblem> {
        self.endpoints.as_ref()
    }

    /// Identifiants OAuth, absents tant qu'ils ne sont pas configurés.
    #[must_use]
    pub fn credentials(&self) -> Option<&PisteCredentials> {
        self.credentials.as_ref()
    }

    /// En-tête `User-Agent` des requêtes.
    #[must_use]
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Réglages avancés.
    #[must_use]
    pub fn tuning(&self) -> &PisteTuning {
        &self.tuning
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SANDBOX: PisteEnvironment = PisteEnvironment::Sandbox;
    const PRODUCTION: PisteEnvironment = PisteEnvironment::Production;
    const API_PATH: &str = "/dila/legifrance/lf-engine-app";
    const SANDBOX_OAUTH: &str = "https://sandbox-oauth.piste.gouv.fr/api/oauth/token";
    const PRODUCTION_OAUTH: &str = "https://oauth.piste.gouv.fr/api/oauth/token";
    const SANDBOX_API: &str = "https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app";
    const PRODUCTION_API: &str = "https://api.piste.gouv.fr/dila/legifrance/lf-engine-app";

    fn base_problem(env: PisteEnvironment, base: &str) -> ConfigProblem {
        PisteEndpoints::with_overrides(env, None, Some(base)).unwrap_err()
    }

    fn oauth_problem(env: PisteEnvironment, oauth: &str) -> ConfigProblem {
        PisteEndpoints::with_overrides(env, Some(oauth), None).unwrap_err()
    }

    #[test]
    fn official_endpoints_match_spec() {
        let sandbox = PisteEndpoints::official(SANDBOX);
        assert_eq!(sandbox.oauth_url().as_str(), SANDBOX_OAUTH);
        assert_eq!(sandbox.base_url().as_str(), SANDBOX_API);
        assert!(sandbox.is_official());
        let production = PisteEndpoints::official(PRODUCTION);
        assert_eq!(production.oauth_url().as_str(), PRODUCTION_OAUTH);
        assert_eq!(production.base_url().as_str(), PRODUCTION_API);
        assert!(production.is_official());

        for (env, oauth, base) in
            [(SANDBOX, SANDBOX_OAUTH, SANDBOX_API), (PRODUCTION, PRODUCTION_OAUTH, PRODUCTION_API)]
        {
            let expected = PisteEndpoints::official(env);
            assert_eq!(
                PisteEndpoints::with_overrides(env, None, None),
                Ok((expected.clone(), Vec::new()))
            );
            assert_eq!(
                PisteEndpoints::with_overrides(env, Some(oauth), Some(base)),
                Ok((expected, Vec::new()))
            );
        }
    }

    #[test]
    fn config_rejects_http_scheme_except_loopback() {
        for raw in ["http://example.com/x", "ftp://example.com/x"] {
            assert_eq!(
                base_problem(SANDBOX, raw),
                ConfigProblem::InsecureScheme { setting: "base URL" }
            );
        }
        assert_eq!(
            oauth_problem(SANDBOX, "http://sandbox-oauth.piste.gouv.fr/api/oauth/token"),
            ConfigProblem::InsecureScheme { setting: "OAuth URL" }
        );

        for host in ["127.0.0.1:8080", "localhost:9", "[::1]:9", "LOCALHOST"] {
            let url = format!("http://{host}/api");
            let (endpoints, warnings) =
                PisteEndpoints::with_overrides(SANDBOX, Some(&url), Some(&url)).unwrap();
            assert!(!endpoints.is_official());
            let host = endpoints.base_url().host_str().unwrap().to_owned();
            assert_eq!(
                warnings,
                vec![
                    ConfigWarning::InsecureLoopback { setting: "OAuth URL" },
                    ConfigWarning::CustomHost { setting: "OAuth URL", host: host.clone() },
                    ConfigWarning::InsecureLoopback { setting: "base URL" },
                    ConfigWarning::CustomHost { setting: "base URL", host },
                ]
            );
        }
    }

    #[test]
    fn config_rejects_userinfo_query_fragment() {
        let cases = [
            ("https://user:pw@example.com/api", "credentials"),
            ("https://user@example.com/api", "credentials"),
            ("https://:pw@example.com/api", "credentials"),
            ("https://example.com/api?x=1", "query"),
            ("https://example.com/api?", "query"),
            ("https://example.com/api#frag", "fragment"),
        ];
        for (raw, part) in cases {
            assert_eq!(
                base_problem(SANDBOX, raw),
                ConfigProblem::ForbiddenPart { setting: "base URL", part }
            );
            assert_eq!(
                oauth_problem(SANDBOX, raw),
                ConfigProblem::ForbiddenPart { setting: "OAuth URL", part }
            );
        }
    }

    #[test]
    fn config_rejects_unparsable_url() {
        for raw in ["", "not a url", "https://exa mple.com/", "/relative/path"] {
            let reason = Url::parse(raw).unwrap_err().to_string();
            let problem = base_problem(SANDBOX, raw);
            assert_eq!(problem.to_string(), format!("invalid base URL: {reason}"));
            assert_eq!(problem, ConfigProblem::InvalidUrl { setting: "base URL", reason });
        }
        let reason = Url::parse("").unwrap_err().to_string();
        assert_eq!(
            oauth_problem(SANDBOX, ""),
            ConfigProblem::InvalidUrl { setting: "OAuth URL", reason }
        );
    }

    #[test]
    fn config_warns_on_custom_host() {
        let (endpoints, warnings) = PisteEndpoints::with_overrides(
            SANDBOX,
            Some("https://auth.example.org/token"),
            Some("https://proxy.example.org/dila/x"),
        )
        .unwrap();
        assert!(!endpoints.is_official());
        assert_eq!(
            warnings,
            vec![
                ConfigWarning::CustomHost {
                    setting: "OAuth URL",
                    host: "auth.example.org".to_owned()
                },
                ConfigWarning::CustomHost {
                    setting: "base URL",
                    host: "proxy.example.org".to_owned()
                },
            ]
        );

        let (endpoints, warnings) = PisteEndpoints::with_overrides(
            PRODUCTION,
            None,
            Some("https://proxy.example.org/dila/x"),
        )
        .unwrap();
        assert!(!endpoints.is_official());
        assert_eq!(endpoints.oauth_url().as_str(), PRODUCTION_OAUTH);
        assert_eq!(
            warnings,
            vec![ConfigWarning::CustomHost {
                setting: "base URL",
                host: "proxy.example.org".to_owned()
            }]
        );

        let (endpoints, warnings) =
            PisteEndpoints::with_overrides(SANDBOX, Some("https://auth.example.org/token"), None)
                .unwrap();
        assert!(!endpoints.is_official());
        assert_eq!(endpoints.base_url().as_str(), SANDBOX_API);
        assert_eq!(
            warnings,
            vec![ConfigWarning::CustomHost {
                setting: "OAuth URL",
                host: "auth.example.org".to_owned()
            }]
        );
    }

    #[test]
    fn config_rejects_env_mismatch() {
        assert_eq!(
            PisteEndpoints::with_overrides(SANDBOX, Some(SANDBOX_OAUTH), Some(PRODUCTION_API)),
            Err(ConfigProblem::EnvMismatch {
                variable: "LF_PISTE_BASE_URL",
                host: "api.piste.gouv.fr".to_owned(),
                env: SANDBOX,
                host_env: PRODUCTION,
            })
        );

        let problem =
            PisteEndpoints::with_overrides(PRODUCTION, Some(SANDBOX_OAUTH), Some(PRODUCTION_API))
                .unwrap_err();
        assert_eq!(
            problem,
            ConfigProblem::EnvMismatch {
                variable: "LF_PISTE_OAUTH_URL",
                host: "sandbox-oauth.piste.gouv.fr".to_owned(),
                env: PRODUCTION,
                host_env: SANDBOX,
            }
        );
        assert!(problem.to_string().contains("LF_PISTE_ENV=sandbox"));

        // la divergence d'environnement prime sur le chemin
        assert_eq!(
            base_problem(PRODUCTION, "https://sandbox-api.piste.gouv.fr/autre"),
            ConfigProblem::EnvMismatch {
                variable: "LF_PISTE_BASE_URL",
                host: "sandbox-api.piste.gouv.fr".to_owned(),
                env: PRODUCTION,
                host_env: SANDBOX,
            }
        );
    }

    #[test]
    fn a_trailing_dot_on_the_host_is_the_same_official_host() {
        // Un nom de domaine absolu (`api.piste.gouv.fr.`) désigne le même hôte que sans le point.
        let dotted = |url: &str| url.replacen(".gouv.fr/", ".gouv.fr./", 1);
        for (env, oauth, base) in
            [(SANDBOX, SANDBOX_OAUTH, SANDBOX_API), (PRODUCTION, PRODUCTION_OAUTH, PRODUCTION_API)]
        {
            let (endpoints, warnings) =
                PisteEndpoints::with_overrides(env, Some(&dotted(oauth)), Some(&dotted(base)))
                    .unwrap();
            assert!(endpoints.is_official(), "{env:?}");
            assert_eq!(warnings, Vec::new(), "{env:?}");
        }
        // L'environnement reste contrôlé : un hôte de production en sandbox est refusé, avec
        // ou sans point final.
        for (raw, base) in [(dotted(PRODUCTION_API), true), (dotted(PRODUCTION_OAUTH), false)] {
            let problem =
                if base { base_problem(SANDBOX, &raw) } else { oauth_problem(SANDBOX, &raw) };
            assert_eq!(
                problem,
                ConfigProblem::EnvMismatch {
                    variable: if base { "LF_PISTE_BASE_URL" } else { "LF_PISTE_OAUTH_URL" },
                    host: if base { "api.piste.gouv.fr" } else { "oauth.piste.gouv.fr" }.to_owned(),
                    env: SANDBOX,
                    host_env: PRODUCTION,
                },
                "{raw}"
            );
        }
        assert_eq!(
            base_problem(PRODUCTION, &dotted(SANDBOX_API)),
            ConfigProblem::EnvMismatch {
                variable: "LF_PISTE_BASE_URL",
                host: "sandbox-api.piste.gouv.fr".to_owned(),
                env: PRODUCTION,
                host_env: SANDBOX,
            }
        );
        // Le chemin de l'API reste imposé sur l'hôte officiel écrit avec un point final.
        assert_eq!(
            base_problem(SANDBOX, "https://sandbox-api.piste.gouv.fr./autre"),
            ConfigProblem::UnexpectedApiPath {
                setting: "base URL",
                host: "sandbox-api.piste.gouv.fr".to_owned()
            }
        );
    }

    #[test]
    fn only_one_trailing_dot_is_removed_from_the_host() {
        let (endpoints, warnings) = PisteEndpoints::with_overrides(
            SANDBOX,
            None,
            Some("https://sandbox-api.piste.gouv.fr../dila/legifrance/lf-engine-app"),
        )
        .unwrap();
        assert!(!endpoints.is_official());
        assert_eq!(
            warnings,
            vec![ConfigWarning::CustomHost {
                setting: "base URL",
                host: "sandbox-api.piste.gouv.fr.".to_owned()
            }]
        );
    }

    #[test]
    fn config_env_mismatch_on_production_host_with_default_env() {
        let cases = [
            (
                Some(PRODUCTION_OAUTH),
                Some(PRODUCTION_API),
                "LF_PISTE_OAUTH_URL",
                "oauth.piste.gouv.fr",
            ),
            (Some(PRODUCTION_OAUTH), None, "LF_PISTE_OAUTH_URL", "oauth.piste.gouv.fr"),
            (None, Some(PRODUCTION_API), "LF_PISTE_BASE_URL", "api.piste.gouv.fr"),
        ];
        for (oauth, base, variable, host) in cases {
            let problem = PisteEndpoints::with_overrides(SANDBOX, oauth, base).unwrap_err();
            assert_eq!(
                problem,
                ConfigProblem::EnvMismatch {
                    variable,
                    host: host.to_owned(),
                    env: SANDBOX,
                    host_env: PRODUCTION,
                }
            );
            // Texte de 05 §7.2, rendu tel quel au modèle : la variable, jamais l'hôte ni l'URL.
            assert_eq!(
                problem.to_string(),
                format!(
                    "{variable} points to the PISTE production host but LF_PISTE_ENV is sandbox: \
                     set LF_PISTE_ENV=production, or fix the URL."
                )
            );
            assert_eq!(
                PisteEndpoints::with_overrides(PRODUCTION, oauth, base),
                Ok((PisteEndpoints::official(PRODUCTION), Vec::new()))
            );
        }
    }

    #[test]
    fn official_api_path_must_be_exact() {
        let paths = [
            "",
            "/",
            "/dila/legifrance",
            "/dila/legifrance/lf-engine-app/v2",
            "/DILA/legifrance/lf-engine-app",
            "/x/dila/legifrance/lf-engine-app",
        ];
        for (env, host) in
            [(SANDBOX, "sandbox-api.piste.gouv.fr"), (PRODUCTION, "api.piste.gouv.fr")]
        {
            for path in paths {
                let problem = base_problem(env, &format!("https://{host}{path}"));
                assert_eq!(
                    problem,
                    ConfigProblem::UnexpectedApiPath { setting: "base URL", host: host.to_owned() }
                );
            }
            assert_eq!(
                PisteEndpoints::with_overrides(
                    env,
                    None,
                    Some(&format!("https://{host}{API_PATH}"))
                ),
                Ok((PisteEndpoints::official(env), Vec::new()))
            );
        }

        assert_eq!(
            base_problem(SANDBOX, "https://sandbox-api.piste.gouv.fr/").to_string(),
            "base URL path on official host sandbox-api.piste.gouv.fr must be \
             /dila/legifrance/lf-engine-app"
        );
        // la règle est par hôte : un hôte API passé comme URL de jeton y est soumis
        assert_eq!(
            oauth_problem(SANDBOX, "https://sandbox-api.piste.gouv.fr/x"),
            ConfigProblem::UnexpectedApiPath {
                setting: "OAuth URL",
                host: "sandbox-api.piste.gouv.fr".to_owned()
            }
        );

        // le chemin n'est contraint que sur un hôte API officiel
        let (endpoints, _) = PisteEndpoints::with_overrides(
            PRODUCTION,
            Some("https://oauth.piste.gouv.fr/autre/chemin"),
            None,
        )
        .unwrap();
        assert_eq!(endpoints.oauth_url().path(), "/autre/chemin");
        let (endpoints, _) =
            PisteEndpoints::with_overrides(SANDBOX, None, Some("https://proxy.example.org/"))
                .unwrap();
        assert_eq!(endpoints.base_url().path(), "/");
    }

    #[test]
    fn trailing_slash_of_base_url_is_removed() {
        for suffix in ["/", "//"] {
            let base = format!("{SANDBOX_API}{suffix}");
            let (endpoints, warnings) =
                PisteEndpoints::with_overrides(SANDBOX, None, Some(&base)).unwrap();
            assert_eq!(endpoints.base_url().as_str(), SANDBOX_API);
            assert!(endpoints.is_official());
            assert_eq!(warnings, Vec::new());
        }

        let (endpoints, _) =
            PisteEndpoints::with_overrides(SANDBOX, None, Some("https://example.org/prefix//"))
                .unwrap();
        assert_eq!(endpoints.base_url().path(), "/prefix");
        let (endpoints, _) =
            PisteEndpoints::with_overrides(SANDBOX, None, Some("https://example.org")).unwrap();
        assert_eq!(endpoints.base_url().path(), "/");

        let (endpoints, _) =
            PisteEndpoints::with_overrides(SANDBOX, Some("https://example.org/token/"), None)
                .unwrap();
        assert_eq!(endpoints.oauth_url().path(), "/token/");
    }

    fn secret_credentials() -> (PisteCredentials, String) {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let secret = format!("s3cr3t-{nanos:x}");
        let credentials =
            PisteCredentials::new("client-1".to_owned(), SecretString::from(secret.clone()));
        (credentials, secret)
    }

    #[test]
    fn debug_of_config_redacts_secret() {
        let (credentials, secret) = secret_credentials();
        let config = PisteConfig::new(
            SANDBOX,
            Ok(PisteEndpoints::official(SANDBOX)),
            Some(credentials.clone()),
            "lf/0".to_owned(),
        );
        for rendered in
            [format!("{config:?}"), format!("{credentials:?}"), format!("{:#?}", config.clone())]
        {
            assert!(!rendered.contains(&secret), "{rendered}");
            assert!(rendered.contains("[REDACTED]"), "{rendered}");
            assert!(rendered.contains("client-1"), "{rendered}");
        }
    }

    #[test]
    fn config_defaults_match_spec() {
        let endpoints = PisteEndpoints::official(PRODUCTION);
        let config = PisteConfig::new(PRODUCTION, Ok(endpoints.clone()), None, "lf/0".to_owned());
        assert_eq!(config.env(), PRODUCTION);
        assert_eq!(config.endpoints(), Ok(&endpoints));
        assert!(config.credentials().is_none());
        assert_eq!(config.user_agent(), "lf/0");
        assert_eq!(config.tuning(), &PisteTuning::default());

        let problem = ConfigProblem::ForbiddenPart { setting: "base URL", part: "query" };
        let degraded = PisteConfig::new(SANDBOX, Err(problem.clone()), None, "lf/0".to_owned());
        assert_eq!(degraded.endpoints(), Err(&problem));
    }

    fn other_tuning() -> PisteTuning {
        PisteTuning {
            connect_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(2),
            tool_deadline: Duration::from_secs(3),
            rate: RateQuota::new(NonZeroU32::new(7).unwrap(), NonZeroU32::new(8).unwrap()),
            max_concurrency: NonZeroU16::new(9).unwrap(),
            max_body_bytes: 10,
            repair_mojibake: false,
        }
    }

    #[test]
    fn tuning_defaults_match_spec() {
        let tuning = PisteTuning::default();
        assert_eq!(tuning.connect_timeout, Duration::from_secs(10));
        assert_eq!(tuning.request_timeout, Duration::from_secs(40));
        assert_eq!(tuning.tool_deadline, Duration::from_secs(60));
        assert_eq!(
            tuning.rate,
            RateQuota::new(NonZeroU32::new(5).unwrap(), NonZeroU32::new(10).unwrap())
        );
        assert_eq!(tuning.max_concurrency, NonZeroU16::new(4).unwrap());
        assert_eq!(tuning.max_body_bytes, 16 * 1024 * 1024);
        assert!(tuning.repair_mojibake);
    }

    #[test]
    fn with_tuning_replaces_the_settings_and_keeps_the_rest() {
        let (credentials, _) = secret_credentials();
        let endpoints = PisteEndpoints::official(PRODUCTION);
        let config = PisteConfig::new(
            PRODUCTION,
            Ok(endpoints.clone()),
            Some(credentials),
            "lf/0".to_owned(),
        )
        .with_tuning(other_tuning())
        .unwrap();
        assert_eq!(config.tuning(), &other_tuning());
        assert_eq!(config.env(), PRODUCTION);
        assert_eq!(config.endpoints(), Ok(&endpoints));
        assert_eq!(config.credentials().map(PisteCredentials::client_id), Some("client-1"));
        assert_eq!(config.user_agent(), "lf/0");
    }

    #[test]
    fn with_tuning_refuses_zero_and_names_the_setting() {
        let zero = Duration::ZERO;
        let base = PisteTuning::default();
        let cases = [
            (PisteTuning { connect_timeout: zero, ..base }, "connect timeout"),
            (PisteTuning { request_timeout: zero, ..base }, "request timeout"),
            (PisteTuning { tool_deadline: zero, ..base }, "tool deadline"),
            (PisteTuning { max_body_bytes: 0, ..base }, "max body bytes"),
            (
                PisteTuning { connect_timeout: zero, request_timeout: zero, ..base },
                "connect timeout",
            ),
            (PisteTuning { request_timeout: zero, tool_deadline: zero, ..base }, "request timeout"),
            (PisteTuning { tool_deadline: zero, max_body_bytes: 0, ..base }, "tool deadline"),
        ];
        for (tuning, setting) in cases {
            let config = PisteConfig::new(
                SANDBOX,
                Ok(PisteEndpoints::official(SANDBOX)),
                None,
                "lf/0".to_owned(),
            );
            let error = config.with_tuning(tuning).unwrap_err();
            assert_eq!(error, TuningError::Zero { setting });
            assert_eq!(error.to_string(), format!("{setting} must not be zero"));
        }
    }

    #[test]
    fn with_tuning_accepts_the_smallest_non_zero_values() {
        let nano = Duration::from_nanos(1);
        let tuning = PisteTuning {
            connect_timeout: nano,
            request_timeout: nano,
            tool_deadline: nano,
            max_body_bytes: 1,
            ..PisteTuning::default()
        };
        let config = PisteConfig::new(
            SANDBOX,
            Ok(PisteEndpoints::official(SANDBOX)),
            None,
            "lf/0".to_owned(),
        )
        .with_tuning(tuning)
        .unwrap();
        assert_eq!(config.tuning(), &tuning);
    }

    #[test]
    fn accessors_return_the_constructor_arguments() {
        let (credentials, secret) = secret_credentials();
        assert_eq!(credentials.client_id(), "client-1");
        assert_eq!(credentials.client_secret().expose_secret(), secret);
        let rate = RateQuota::new(NonZeroU32::new(3).unwrap(), NonZeroU32::new(4).unwrap());
        assert_eq!(rate.per_second(), NonZeroU32::new(3).unwrap());
        assert_eq!(rate.burst(), NonZeroU32::new(4).unwrap());
    }

    #[test]
    fn problem_messages_never_contain_userinfo() {
        let raws = [
            "https://user:pw@sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app",
            "https://user:pw@example.com/x?token=pw#pw",
            "https://user:pw@api.piste.gouv.fr/dila/legifrance/lf-engine-app",
            "ftp://user:pw@example.com/x",
            "http://user:pw@example.com/x",
            "https://user:pw@exa mple.com/x",
            "user:pw@example.com",
        ];
        for raw in raws {
            for problem in [base_problem(SANDBOX, raw), oauth_problem(SANDBOX, raw)] {
                for rendered in [problem.to_string(), format!("{problem:?}")] {
                    assert!(!rendered.contains("user"), "{rendered}");
                    assert!(!rendered.contains("pw"), "{rendered}");
                }
            }
        }
    }

    #[test]
    fn warning_and_problem_messages_are_stable() {
        assert_eq!(
            ConfigWarning::InsecureLoopback { setting: "base URL" }.to_string(),
            "base URL uses plain http on a loopback host"
        );
        assert_eq!(
            ConfigWarning::CustomHost { setting: "OAuth URL", host: "h.example".to_owned() }
                .to_string(),
            "OAuth URL points to a non-official host h.example"
        );
        assert_eq!(
            ConfigProblem::InsecureScheme { setting: "base URL" }.to_string(),
            "base URL must use https (http is only accepted for loopback hosts)"
        );
        assert_eq!(
            ConfigProblem::ForbiddenPart { setting: "base URL", part: "query" }.to_string(),
            "base URL must not contain query"
        );
    }

    /// Le `Display` d'un `ConfigProblem` est le `detail` de `CorpusError::Config { kind: Settings }`,
    /// que `lf-application::messages` rend tel quel au modèle : les deux textes de 05 §7.2, au
    /// caractère près, point final compris.
    #[test]
    fn env_invalid_and_env_mismatch_display_match_05_7_2() {
        assert_eq!(
            ConfigProblem::invalid_env("prod").to_string(),
            "LF_PISTE_ENV='prod' is invalid: expected sandbox or production."
        );
        assert_eq!(
            base_problem(SANDBOX, PRODUCTION_API).to_string(),
            "LF_PISTE_BASE_URL points to the PISTE production host but LF_PISTE_ENV is sandbox: \
             set LF_PISTE_ENV=production, or fix the URL."
        );
        assert_eq!(
            oauth_problem(PRODUCTION, SANDBOX_OAUTH).to_string(),
            "LF_PISTE_OAUTH_URL points to the PISTE sandbox host but LF_PISTE_ENV is production: \
             set LF_PISTE_ENV=sandbox, or fix the URL."
        );
    }

    /// Texte d'une valeur de `LF_PISTE_ENV` non citée, de `chars` caractères (M-1).
    fn hidden_env(chars: usize) -> String {
        format!(
            "LF_PISTE_ENV is invalid (value not shown, {chars} characters): expected sandbox or \
             production."
        )
    }

    #[test]
    fn an_invalid_env_value_is_quoted_only_when_it_is_a_short_ascii_word() {
        // Faute de frappe plausible : 1 à 16 lettres ASCII, citées telles quelles.
        for value in ["prod", "Production", "SANDBOX", "abcdefghijklmnop"] {
            assert_eq!(
                ConfigProblem::invalid_env(value).to_string(),
                format!("LF_PISTE_ENV='{value}' is invalid: expected sandbox or production."),
                "{value}"
            );
        }
        // Tout le reste n'est compté qu'en caractères (points de code), jamais cité : trop long,
        // chiffre, tiret, espace, lettre non ASCII, retour à la ligne, valeur vide.
        let rows = [
            ("abcdefghijklmnopq", 17),
            ("prod1", 5),
            ("pre-prod", 8),
            (" prod", 5),
            ("prodé", 5),
            ("prod\n", 5),
            ("", 0),
        ];
        for (value, chars) in rows {
            assert_eq!(
                ConfigProblem::invalid_env(value).to_string(),
                hidden_env(chars),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_secret_pasted_into_lf_piste_env_is_never_kept() {
        // Une valeur de la forme d'un identifiant client (UUID, 36 caractères, factice à
        // l'évidence) et une valeur multi-ligne qui recopie une affectation de secret : ni le
        // `Display` (modèle, audit) ni le `Debug` (journaux) ne les portent, en tout ou en partie.
        let uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let multiline = "sandbox\nLF_PISTE_CLIENT_SECRET=placeholder";
        for (value, chars, fragments) in [
            (uuid, 36, &["aaaa", "bbbb", "eeeeeeeeeeee"][..]),
            // « sandbox » figure dans le texte fixe (« expected sandbox or production ») : on
            // cherche la suite de la valeur et le retour à la ligne, brut ou échappé.
            (multiline, 42, &["CLIENT_SECRET", "placeholder", "\n", "\\n"][..]),
        ] {
            let problem = ConfigProblem::invalid_env(value);
            assert_eq!(problem.to_string(), hidden_env(chars), "{chars}");
            for rendered in [problem.to_string(), format!("{problem:?}")] {
                for fragment in fragments {
                    assert!(!rendered.contains(fragment), "{fragment} in {rendered}");
                }
            }
        }
    }

    #[test]
    fn a_quoted_env_value_shows_in_debug_and_a_hidden_one_only_by_length() {
        assert_eq!(
            format!("{:?}", ConfigProblem::invalid_env("prod")),
            r#"InvalidEnv { value: RejectedEnv(Quoted("prod")) }"#
        );
        assert_eq!(
            format!("{:?}", ConfigProblem::invalid_env("prod1")),
            "InvalidEnv { value: RejectedEnv(Hidden { chars: 5 }) }"
        );
    }
}
