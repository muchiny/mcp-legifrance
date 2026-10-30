//! Configuration : `EnvSource`, `AppConfig`, table des variables d'environnement (02 §8.2),
//! compatibilité `FR_ELI_*` et variables héritées ignorées. Seul lieu où l'environnement du
//! processus est lu (`clippy.toml`, `disallowed-methods`).
//!
//! # Principes
//!
//! - Lecture **unique** au démarrage : [`ProcessEnv::capture`] copie les variables de la liste
//!   fermée de 00 §1, puis [`load`] les lit par le trait [`EnvSource`] (une table en test : PT4).
//! - [`load`] ne fait ni E/S ni journal : il rend un [`Loaded`] qui porte la configuration, les
//!   [`Notice`] (avertissements et problèmes) et une ligne par variable pour `check-config`.
//!   L'appelant initialise `tracing` d'après [`AppConfig::log_filter`], puis appelle
//!   [`Loaded::emit`] (02 §3.7, piège 1).
//! - Une valeur invalide n'empêche jamais le démarrage : les réglages avancés reprennent leur
//!   défaut, et un problème PISTE met le serveur en mode dégradé (00 §1, 02 §8.2).
//! - Rien de ce qui est lu n'est recopié dans un message : ni secret, ni URL, ni valeur refusée.
//!   Les messages ne citent que des noms de variables et des bornes.
//! - Aucune lecture de fichier (`piste.env` compris, ADR-0005).
//! - Source des textes (M9) : PISTE, sauf si `LF_DILA_DIR` désigne l'index local DILA
//!   ([`CorpusSource`]). Le binaire ne télécharge aucun dump (plan « fin du projet » : script hors
//!   binaire) ; le répertoire contient un index déjà bâti. Si la source locale est demandée sans
//!   pouvoir servir, le serveur est dégradé et ne se rabat jamais sur PISTE.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    num::{NonZeroU16, NonZeroU32},
    path::PathBuf,
    time::Duration,
};

use lf_adapter_piste::{
    ConfigProblem, ConfigWarning, PisteConfig, PisteCredentials, PisteEndpoints, PisteTuning,
    RateQuota,
};
use lf_application::{audited::AuditMode, ports::PisteEnvironment, service::ConfigurationState};
use secrecy::SecretString;
use tracing_subscriber::EnvFilter;

/// `LF_PISTE_ENV` : environnement PISTE.
const PISTE_ENV: &str = "LF_PISTE_ENV";
/// `LF_PISTE_CLIENT_ID` : identifiant OAuth.
const CLIENT_ID: &str = "LF_PISTE_CLIENT_ID";
/// `LF_PISTE_CLIENT_SECRET` : secret OAuth.
const CLIENT_SECRET: &str = "LF_PISTE_CLIENT_SECRET";
/// `LF_PISTE_OAUTH_URL` : surcharge de l'URL du jeton.
const OAUTH_URL: &str = "LF_PISTE_OAUTH_URL";
/// `LF_PISTE_BASE_URL` : surcharge de l'URL de base.
const BASE_URL: &str = "LF_PISTE_BASE_URL";
/// `LF_AUDIT_DIR` : répertoire du journal d'audit.
const AUDIT_DIR: &str = "LF_AUDIT_DIR";
/// `LF_AUDIT_MODE` : politique en cas d'échec d'écriture de l'audit.
const AUDIT_MODE: &str = "LF_AUDIT_MODE";
/// `LF_LOG` : filtre `tracing`.
const LOG: &str = "LF_LOG";
/// `LF_HTTP_TIMEOUT_S` : délai par requête HTTP.
const HTTP_TIMEOUT: &str = "LF_HTTP_TIMEOUT_S";
/// `LF_TOOL_DEADLINE_S` : délai global par appel d'outil.
const TOOL_DEADLINE: &str = "LF_TOOL_DEADLINE_S";
/// `LF_PISTE_RPS` : requêtes par seconde.
const RPS: &str = "LF_PISTE_RPS";
/// `LF_PISTE_BURST` : rafale du limiteur.
const BURST: &str = "LF_PISTE_BURST";
/// `LF_PISTE_CONCURRENCY` : requêtes API en vol.
const CONCURRENCY: &str = "LF_PISTE_CONCURRENCY";
/// `LF_PISTE_MOJIBAKE_REPAIR` : réparation du mojibake.
const MOJIBAKE_REPAIR: &str = "LF_PISTE_MOJIBAKE_REPAIR";
/// `LF_DILA_DIR` : répertoire de l'index local DILA (M9). Défini, il remplace PISTE comme source
/// des textes ; absent, PISTE reste la source.
const DILA_DIR: &str = "LF_DILA_DIR";

/// Équivalent hérité de `LF_PISTE_CLIENT_ID`.
const LEGACY_CLIENT_ID: &str = "FR_ELI_CLIENT_ID";
/// Équivalent hérité de `LF_PISTE_CLIENT_SECRET`.
const LEGACY_CLIENT_SECRET: &str = "FR_ELI_CLIENT_SECRET";
/// Équivalent hérité de `LF_PISTE_OAUTH_URL`.
const LEGACY_OAUTH_URL: &str = "FR_ELI_OAUTH_URL";
/// Équivalent hérité de `LF_PISTE_BASE_URL`.
const LEGACY_BASE_URL: &str = "FR_ELI_BASE_URL";
/// Équivalent hérité de `LF_AUDIT_DIR`.
const LEGACY_AUDIT_DIR: &str = "FR_ELI_AUDIT_DIR";

/// Variables héritées qui ne sont jamais lues : définies, elles ne produisent qu'un avertissement
/// « ignored » (00 §1).
const IGNORED_LEGACY: [&str; 5] = [
    "FR_ELI_CACHE_DIR",
    "FR_ELI_RUNTIME_URL",
    "FR_ELI_ENV",
    "PISTE_CLIENT_ID",
    "PISTE_CLIENT_SECRET",
];

/// Variables dont la valeur est lue : la liste fermée de 00 §1, `LF_DILA_DIR` (M9, à ajouter à 00 §1
/// et à 02 §8.2) et les `FR_ELI_*` de compatibilité.
const READ_VARIABLES: [&str; 20] = [
    PISTE_ENV,
    CLIENT_ID,
    CLIENT_SECRET,
    OAUTH_URL,
    BASE_URL,
    AUDIT_DIR,
    AUDIT_MODE,
    LOG,
    HTTP_TIMEOUT,
    TOOL_DEADLINE,
    RPS,
    BURST,
    CONCURRENCY,
    MOJIBAKE_REPAIR,
    DILA_DIR,
    LEGACY_CLIENT_ID,
    LEGACY_CLIENT_SECRET,
    LEGACY_OAUTH_URL,
    LEGACY_BASE_URL,
    LEGACY_AUDIT_DIR,
];

/// `User-Agent` des requêtes vers PISTE (04 §4.2) : nom, version du binaire et dépôt.
const USER_AGENT: &str = concat!(
    "mcp-legifrance/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/muchiny/mcp-legifrance)"
);

/// Source des variables d'environnement et des répertoires de la plateforme.
///
/// En production, [`ProcessEnv`] ; en test, une table : l'environnement du processus ne bouge pas
/// (la modification de l'environnement est interdite par `forbid(unsafe_code)`, PT4).
pub(crate) trait EnvSource {
    /// Valeur brute de `name`, absente si la variable n'est pas définie.
    fn get(&self, name: &str) -> Option<String>;

    /// Vrai si `name` est définie avec une valeur non vide. Sert aux variables jamais lues : la
    /// valeur n'a pas à être conservée.
    fn is_set(&self, name: &str) -> bool {
        self.get(name).is_some_and(|value| !value.trim().is_empty())
    }

    /// Répertoire personnel de l'utilisateur, pour développer un `~/` en tête de `LF_AUDIT_DIR`.
    fn home_dir(&self) -> Option<PathBuf>;

    /// Répertoire de données de la plateforme, base du défaut de `LF_AUDIT_DIR` (V-167).
    fn data_dir(&self) -> Option<PathBuf>;
}

/// Environnement du processus, copié une fois au démarrage.
///
/// Ne garde que les variables de la liste fermée : le reste de l'environnement n'entre pas en
/// mémoire. Les variables héritées jamais lues ne gardent qu'une marque « définie ».
#[derive(Clone, Default)]
pub(crate) struct ProcessEnv {
    /// Valeurs des variables lues.
    values: BTreeMap<&'static str, String>,
    /// Variables héritées ignorées qui sont définies avec une valeur non vide.
    ignored_set: BTreeSet<&'static str>,
}

impl fmt::Debug for ProcessEnv {
    /// Les noms seuls : une valeur peut être un secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessEnv")
            .field("values", &self.values.keys().collect::<Vec<_>>())
            .field("ignored_set", &self.ignored_set)
            .finish()
    }
}

impl ProcessEnv {
    /// Copie les variables de la liste fermée. Seule lecture de l'environnement du processus.
    ///
    /// Une valeur qui n'est pas de l'Unicode valide est convertie avec perte : elle échouera à
    /// la validation de son format, ou, pour un secret, sera refusée par PISTE.
    #[expect(
        clippy::disallowed_methods,
        reason = "seul lieu de lecture de l'environnement du processus (02 §8.2)"
    )]
    pub(crate) fn capture() -> Self {
        let values = READ_VARIABLES
            .into_iter()
            .filter_map(|name| {
                std::env::var_os(name).map(|value| (name, value.to_string_lossy().into_owned()))
            })
            .collect();
        let ignored_set = IGNORED_LEGACY
            .into_iter()
            .filter(|name| {
                std::env::var_os(name)
                    .is_some_and(|value| !value.to_string_lossy().trim().is_empty())
            })
            .collect();
        Self { values, ignored_set }
    }
}

impl EnvSource for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.values.get(name).cloned()
    }

    fn is_set(&self, name: &str) -> bool {
        self.ignored_set.contains(name)
            || self.values.get(name).is_some_and(|value| !value.trim().is_empty())
    }

    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }

    fn data_dir(&self) -> Option<PathBuf> {
        dirs::data_dir()
    }
}

/// Répertoire d'audit inutilisable : `audit_error` au premier appel en mode strict (02 §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuditDirProblem {
    /// `LF_AUDIT_DIR` n'est pas un chemin absolu.
    NotAbsolute,
    /// `LF_AUDIT_DIR` commence par `~/` et le répertoire personnel est introuvable.
    NoHome,
    /// `LF_AUDIT_DIR` est absent et le répertoire de données de la plateforme est introuvable.
    NoDataDir,
}

impl fmt::Display for AuditDirProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAbsolute => "LF_AUDIT_DIR must be an absolute path",
            Self::NoHome => "LF_AUDIT_DIR starts with ~/ but the home directory is unknown",
            Self::NoDataDir => {
                "no user data directory found for the audit log: set LF_AUDIT_DIR to an absolute path"
            }
        })
    }
}

/// `LF_DILA_DIR` inutilisable : la source locale est demandée, mais ne peut pas servir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DilaDirProblem {
    /// `LF_DILA_DIR` n'est pas un chemin absolu.
    NotAbsolute,
    /// `LF_DILA_DIR` commence par `~/` et le répertoire personnel est introuvable.
    NoHome,
    /// Ce binaire est compilé sans la feature `dila` : il n'a pas d'index local.
    NotCompiled,
}

impl fmt::Display for DilaDirProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAbsolute => "LF_DILA_DIR must be an absolute path",
            Self::NoHome => "LF_DILA_DIR starts with ~/ but the home directory is unknown",
            Self::NotCompiled => {
                "LF_DILA_DIR is set but this build has no local DILA index: it needs the dila feature"
            }
        })
    }
}

/// Source des textes juridiques (02 §8.5) : PISTE par défaut, l'index local DILA si `LF_DILA_DIR`
/// est défini.
///
/// Une source locale demandée mais inutilisable n'est **jamais** remplacée par PISTE : le choix de
/// l'hors-ligne est souvent un choix de confidentialité, et une requête ne doit pas partir sur le
/// réseau contre la volonté de l'utilisateur. Le serveur démarre alors en mode dégradé
/// (`[config_error]` sur chaque outil), comme pour tout réglage invalide (00 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CorpusSource {
    /// L'API PISTE, derrière le cache (défaut).
    Piste,
    /// L'index local DILA du répertoire (feature `dila`).
    Dila(PathBuf),
    /// La source locale est demandée et inutilisable : aucun corpus.
    Unusable(DilaDirProblem),
}

impl CorpusSource {
    /// Nom de la source dans le journal de démarrage.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Piste => "piste",
            Self::Dila(_) => "dila",
            Self::Unusable(_) => "unavailable",
        }
    }
}

/// Réglages de l'audit (00 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuditSettings {
    /// Sort du résultat quand l'écriture échoue : `Strict` par défaut (Q9).
    pub(crate) mode: AuditMode,
    /// Répertoire du journal, ou la raison pour laquelle il est inutilisable.
    pub(crate) dir: Result<PathBuf, AuditDirProblem>,
}

/// Configuration du processus, lue une fois au démarrage (02 §8.2). `Debug` ne montre aucun secret :
/// `PisteConfig` ne garde le secret que dans un `SecretString`.
#[derive(Debug, Clone)]
pub(crate) struct AppConfig {
    /// Configuration de l'adapter PISTE (mode dégradé compris).
    pub(crate) piste: PisteConfig,
    /// Réglages de l'audit.
    pub(crate) audit: AuditSettings,
    /// Délai global par appel d'outil (`LF_TOOL_DEADLINE_S`, au plus 600 s).
    pub(crate) tool_deadline: Duration,
    /// Filtre `LF_LOG` s'il est défini et valide ; `None` : filtre par défaut de `logging`.
    pub(crate) log_filter: Option<String>,
    /// État de la configuration, tel que `fr_coverage` le rend.
    pub(crate) configuration: ConfigurationState,
    /// Source des textes : PISTE, ou l'index local DILA (`LF_DILA_DIR`, M9).
    pub(crate) source: CorpusSource,
}

/// Avertissement ou problème relevé en lisant la configuration (jamais de secret ni de valeur lue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notice {
    /// Variable `FR_ELI_*` lue faute de son remplaçant `LF_*`.
    Deprecated {
        /// Variable héritée lue.
        variable: &'static str,
        /// Variable à employer.
        replacement: &'static str,
    },
    /// Variable `FR_ELI_*` ignorée parce que son remplaçant `LF_*` est aussi défini.
    Shadowed {
        /// Variable héritée ignorée.
        variable: &'static str,
        /// Variable `LF_*` qui l'emporte.
        replacement: &'static str,
    },
    /// Variable héritée jamais lue, mais définie.
    Ignored {
        /// Variable héritée ignorée.
        variable: &'static str,
    },
    /// Réglage invalide, remplacé par son défaut.
    InvalidSetting {
        /// Variable concernée.
        variable: &'static str,
        /// Ce qui était attendu (bornes ou valeurs).
        expected: String,
        /// Valeur effective.
        default: String,
    },
    /// `LF_LOG` n'est pas un filtre valide : filtre par défaut.
    InvalidLogFilter,
    /// `LF_AUDIT_MODE` inconnu : `strict`.
    UnknownAuditMode,
    /// Avertissement sur une URL surchargée acceptée.
    Warning(ConfigWarning),
    /// Réglage PISTE refusé : mode dégradé.
    Problem(ConfigProblem),
    /// Répertoire d'audit inutilisable.
    AuditDir(AuditDirProblem),
    /// Index local DILA demandé et inutilisable : mode dégradé, jamais un repli sur PISTE.
    DilaDir(DilaDirProblem),
    /// Identifiants PISTE absents : mode dégradé.
    CredentialsMissing,
}

impl Notice {
    /// Vrai pour un problème (`error!`, code 2 de `check-config`), faux pour un avertissement.
    pub(crate) fn is_problem(&self) -> bool {
        matches!(
            self,
            Self::UnknownAuditMode
                | Self::Problem(_)
                | Self::AuditDir(_)
                | Self::DilaDir(_)
                | Self::CredentialsMissing
        )
    }

    /// Écrit la notice dans le journal `tracing` : `error!` pour un problème, `warn!` sinon.
    fn emit(&self) {
        match self {
            Self::Deprecated { variable, replacement } => {
                tracing::warn!(variable, replacement, "deprecated environment variable");
            }
            _ if self.is_problem() => tracing::error!("{self}"),
            _ => tracing::warn!("{self}"),
        }
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deprecated { variable, replacement } => {
                write!(f, "{variable} is deprecated: use {replacement}")
            }
            Self::Shadowed { variable, replacement } => {
                write!(f, "{variable} ignored: {replacement} is also set")
            }
            Self::Ignored { variable } => write!(f, "{variable} ignored: it is never read"),
            Self::InvalidSetting { variable, expected, default } => {
                write!(f, "{variable} is invalid (expected {expected}): using {default}")
            }
            Self::InvalidLogFilter => {
                write!(f, "{LOG} is not a valid filter: using the default filter")
            }
            Self::UnknownAuditMode => {
                write!(f, "{AUDIT_MODE} must be strict or best-effort: using strict")
            }
            Self::Warning(warning) => warning.fmt(f),
            Self::Problem(problem) => problem.fmt(f),
            Self::AuditDir(problem) => problem.fmt(f),
            Self::DilaDir(problem) => problem.fmt(f),
            Self::CredentialsMissing => {
                write!(f, "PISTE credentials missing: set {CLIENT_ID} and {CLIENT_SECRET}")
            }
        }
    }
}

/// D'où vient la valeur effective d'une variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// La variable de ce nom (`LF_*`, ou `FR_ELI_*` lue par compatibilité).
    Variable(&'static str),
    /// Le défaut.
    Default,
}

impl Source {
    /// Libellé affiché : nom de la variable lue, ou `default`.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Variable(name) => name,
            Self::Default => "default",
        }
    }
}

/// État d'une variable dans `check-config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineState {
    /// Valeur valide, ou défaut.
    Ok,
    /// Variable non définie et sans défaut (identifiants).
    Unset,
    /// Valeur invalide remplacée par le défaut.
    Replaced,
    /// Valeur valide mais à relire (hôte hors des hôtes officiels).
    Warning,
    /// Valeur refusée : mode dégradé.
    Problem,
}

impl LineState {
    /// Libellé affiché.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unset => "not set",
            Self::Replaced => "invalid, default used",
            Self::Warning => "warning",
            Self::Problem => "problem",
        }
    }

    /// Clé stable pour la sortie JSON.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unset => "unset",
            Self::Replaced => "replaced",
            Self::Warning => "warning",
            Self::Problem => "problem",
        }
    }
}

/// Une ligne de `check-config` : nom, source, valeur et état d'une variable de la liste fermée.
/// La valeur est déjà masquée pour les secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingLine {
    /// Nom de la variable `LF_*`.
    pub(crate) name: &'static str,
    /// Origine de la valeur effective.
    pub(crate) source: Source,
    /// Valeur effective, sans secret.
    pub(crate) value: String,
    /// État.
    pub(crate) state: LineState,
}

/// Résultat de [`load`] : la configuration, ce qu'elle a fait remarquer, et le détail par variable.
#[derive(Debug, Clone)]
pub(crate) struct Loaded {
    /// Configuration effective.
    pub(crate) config: AppConfig,
    /// Avertissements et problèmes, dans l'ordre de lecture.
    pub(crate) notices: Vec<Notice>,
    /// Une ligne par variable de la liste fermée, dans l'ordre de 02 §8.2, plus `LF_DILA_DIR` en
    /// dernier quand elle est définie (sans elle, la table garde ses quatorze lignes).
    pub(crate) lines: Vec<SettingLine>,
}

impl Loaded {
    /// Vrai si aucun problème n'est relevé (les avertissements sont admis).
    pub(crate) fn is_valid(&self) -> bool {
        !self.notices.iter().any(Notice::is_problem)
    }

    /// Écrit chaque notice dans le journal (`warn!`, ou `error!` pour un problème). À appeler une
    /// fois `tracing` initialisé.
    pub(crate) fn emit(&self) {
        for notice in &self.notices {
            notice.emit();
        }
    }
}

/// Défaut de `LF_HTTP_TIMEOUT_S`, en secondes (borne 1..=300).
const DEFAULT_HTTP_TIMEOUT_S: u32 = 40;
/// Défaut de `LF_TOOL_DEADLINE_S`, en secondes (borne 1..=600 : `AuditPolicy::deadline` suppose
/// au plus 600 s).
const DEFAULT_TOOL_DEADLINE_S: u32 = 60;
/// Défaut de `LF_PISTE_RPS` (borne 1..=100).
const DEFAULT_RPS: u32 = 5;
/// Défaut de `LF_PISTE_BURST` (borne 1..=100).
const DEFAULT_BURST: u32 = 10;
/// Défaut de `LF_PISTE_CONCURRENCY` (borne 1..=16).
const DEFAULT_CONCURRENCY: u32 = 4;
/// Nombre de caractères d'un identifiant client que `check-config` laisse voir.
const CLIENT_ID_SHOWN_CHARS: usize = 4;

/// Valeur lue d'une variable, avec son origine.
struct Read {
    /// Valeur sans blancs de bord ; `None` si la variable est absente ou blanche.
    value: Option<String>,
    /// Variable dont vient la valeur.
    source: Source,
}

/// Lecteur de la configuration : accumule les notices et les lignes de `check-config`.
struct Collector<'a, E: EnvSource> {
    /// Source des variables.
    env: &'a E,
    /// Avertissements et problèmes, dans l'ordre de lecture.
    notices: Vec<Notice>,
    /// Une ligne par variable, dans l'ordre de la table de 02 §8.2.
    lines: Vec<SettingLine>,
}

impl<E: EnvSource> Collector<'_, E> {
    /// Lit `name`, puis son équivalent hérité `legacy`. Une valeur vide ou blanche compte comme
    /// absente (les clients MCP transmettent souvent des chaînes vides) ; `name` l'emporte.
    fn read(&mut self, name: &'static str, legacy: Option<&'static str>) -> Read {
        let primary = non_blank(self.env.get(name));
        let secondary =
            legacy.and_then(|old| non_blank(self.env.get(old)).map(|value| (old, value)));
        match (primary, secondary) {
            (Some(value), shadowed) => {
                if let Some((variable, _)) = shadowed {
                    self.notices.push(Notice::Shadowed { variable, replacement: name });
                }
                Read { value: Some(value), source: Source::Variable(name) }
            }
            (None, Some((variable, value))) => {
                self.notices.push(Notice::Deprecated { variable, replacement: name });
                Read { value: Some(value), source: Source::Variable(variable) }
            }
            (None, None) => Read { value: None, source: Source::Default },
        }
    }

    /// Ajoute la ligne de `name`.
    fn line(
        &mut self,
        name: &'static str,
        source: Source,
        value: impl Into<String>,
        state: LineState,
    ) {
        self.lines.push(SettingLine { name, source, value: value.into(), state });
    }

    /// `LF_PISTE_ENV` : `sandbox` ou `production` (blancs et casse ignorés), `sandbox` par défaut.
    /// Une autre valeur donne un problème et `sandbox` pour la suite.
    fn environment(&mut self) -> (PisteEnvironment, Option<ConfigProblem>) {
        let read = self.read(PISTE_ENV, None);
        let Some(raw) = read.value else {
            self.line(PISTE_ENV, Source::Default, PisteEnvironment::Sandbox.name(), LineState::Ok);
            return (PisteEnvironment::Sandbox, None);
        };
        if let Some(env) = parse_environment(&raw) {
            self.line(PISTE_ENV, read.source, env.name(), LineState::Ok);
            return (env, None);
        }
        self.line(PISTE_ENV, read.source, "(rejected)", LineState::Problem);
        (PisteEnvironment::Sandbox, Some(ConfigProblem::invalid_env(&raw)))
    }

    /// Identifiants OAuth : les deux ou aucun. Le secret est enveloppé dès sa lecture ; seule sa
    /// longueur et le début de l'identifiant sont montrés. Leur absence n'est signalée que si
    /// PISTE est la source (`needed`) : l'index local DILA n'en exige pas.
    fn credentials(&mut self, needed: bool) -> Option<PisteCredentials> {
        let id = self.read(CLIENT_ID, Some(LEGACY_CLIENT_ID));
        let secret = self.read(CLIENT_SECRET, Some(LEGACY_CLIENT_SECRET));
        match &id.value {
            Some(value) => self.line(CLIENT_ID, id.source, short_id(value), LineState::Ok),
            None => self.line(CLIENT_ID, Source::Default, "not set", LineState::Unset),
        }
        match &secret.value {
            Some(value) => {
                let shown = format!("set ({} characters)", value.chars().count());
                self.line(CLIENT_SECRET, secret.source, shown, LineState::Ok);
            }
            None => self.line(CLIENT_SECRET, Source::Default, "not set", LineState::Unset),
        }
        let credentials = match (id.value, secret.value) {
            (Some(id), Some(secret)) => Some(PisteCredentials::new(id, SecretString::from(secret))),
            _ => None,
        };
        if needed && credentials.is_none() {
            self.notices.push(Notice::CredentialsMissing);
        }
        credentials
    }

    /// Surcharges d'URL : validées par l'adapter (04 §2.3). Rend les adresses, ou le problème qui
    /// met le serveur en mode dégradé, et vrai si un hôte hors des hôtes officiels est configuré.
    fn endpoints(
        &mut self,
        env: PisteEnvironment,
        env_problem: Option<ConfigProblem>,
    ) -> (Result<PisteEndpoints, ConfigProblem>, bool) {
        let oauth = self.read(OAUTH_URL, Some(LEGACY_OAUTH_URL));
        let base = self.read(BASE_URL, Some(LEGACY_BASE_URL));
        let outcome = match env_problem {
            Some(problem) => Err(problem),
            None => {
                PisteEndpoints::with_overrides(env, oauth.value.as_deref(), base.value.as_deref())
            }
        };
        match outcome {
            Ok((endpoints, warnings)) => {
                let shown = [
                    (OAUTH_URL, "OAuth URL", &oauth, endpoints.oauth_url().as_str()),
                    (BASE_URL, "base URL", &base, endpoints.base_url().as_str()),
                ];
                for (name, setting, read, url) in shown {
                    let flagged = warnings.iter().any(|warning| concerns(warning, setting));
                    let state = if flagged { LineState::Warning } else { LineState::Ok };
                    self.line(name, read.source, url, state);
                }
                let custom_host = warnings
                    .iter()
                    .any(|warning| matches!(warning, ConfigWarning::CustomHost { .. }));
                self.notices.extend(warnings.into_iter().map(Notice::Warning));
                (Ok(endpoints), custom_host)
            }
            Err(problem) => {
                // Une valeur d'environnement refusée n'a pas permis d'examiner les URL.
                let checked = !matches!(problem, ConfigProblem::InvalidEnv { .. });
                let official = PisteEndpoints::official(env);
                let defaults = [
                    (OAUTH_URL, &oauth, official.oauth_url().as_str()),
                    (BASE_URL, &base, official.base_url().as_str()),
                ];
                for (name, read, default) in defaults {
                    if read.value.is_none() {
                        self.line(name, Source::Default, default, LineState::Ok);
                    } else {
                        let shown = if checked { "(rejected)" } else { "(not checked)" };
                        self.line(name, read.source, shown, LineState::Problem);
                    }
                }
                self.notices.push(Notice::Problem(problem.clone()));
                (Err(problem), false)
            }
        }
    }

    /// `LF_AUDIT_DIR` : chemin absolu (un `~/` en tête est développé), sinon le répertoire de
    /// données de la plateforme.
    fn audit_dir(&mut self) -> Result<PathBuf, AuditDirProblem> {
        let read = self.read(AUDIT_DIR, Some(LEGACY_AUDIT_DIR));
        let resolved = match read.value.as_deref() {
            Some(raw) => explicit_audit_dir(self.env, raw),
            None => self
                .env
                .data_dir()
                .map(|dir| dir.join("mcp-legifrance").join("audit"))
                .ok_or(AuditDirProblem::NoDataDir),
        };
        match &resolved {
            Ok(dir) => {
                self.line(AUDIT_DIR, read.source, dir.display().to_string(), LineState::Ok);
            }
            Err(problem) => {
                self.line(AUDIT_DIR, read.source, "(unusable)", LineState::Problem);
                self.notices.push(Notice::AuditDir(*problem));
            }
        }
        resolved
    }

    /// `LF_DILA_DIR` (M9) : l'index local DILA à la place de PISTE.
    ///
    /// Absent ou blanc : PISTE, et aucune ligne pour `check-config`. Défini : un chemin absolu
    /// (un `~/` en tête est développé) et un binaire compilé avec la feature `dila` ; sinon la
    /// source est inutilisable, avec un problème, et jamais remplacée par PISTE.
    fn corpus_source(&mut self) -> CorpusSource {
        let read = self.read(DILA_DIR, None);
        let Some(raw) = read.value else {
            return CorpusSource::Piste;
        };
        let resolved = match absolute_path(self.env, &raw) {
            Ok(dir) => dila_dir_if_compiled(dir, cfg!(feature = "dila")),
            Err(PathProblem::NotAbsolute) => Err(DilaDirProblem::NotAbsolute),
            Err(PathProblem::NoHome) => Err(DilaDirProblem::NoHome),
        };
        match resolved {
            Ok(dir) => {
                self.line(DILA_DIR, read.source, dir.display().to_string(), LineState::Ok);
                CorpusSource::Dila(dir)
            }
            Err(problem) => {
                self.line(DILA_DIR, read.source, "(unusable)", LineState::Problem);
                self.notices.push(Notice::DilaDir(problem));
                CorpusSource::Unusable(problem)
            }
        }
    }

    /// `LF_AUDIT_MODE` : `strict` (défaut, Q9) ou `best-effort`. Une valeur inconnue donne
    /// `strict`, le choix prudent, et un problème.
    fn audit_mode(&mut self) -> AuditMode {
        let read = self.read(AUDIT_MODE, None);
        let Some(raw) = read.value else {
            self.line(AUDIT_MODE, Source::Default, "strict", LineState::Ok);
            return AuditMode::Strict;
        };
        if raw.eq_ignore_ascii_case("strict") {
            self.line(AUDIT_MODE, read.source, "strict", LineState::Ok);
            AuditMode::Strict
        } else if raw.eq_ignore_ascii_case("best-effort") {
            self.line(AUDIT_MODE, read.source, "best-effort", LineState::Ok);
            AuditMode::BestEffort
        } else {
            self.notices.push(Notice::UnknownAuditMode);
            self.line(AUDIT_MODE, Source::Default, "strict", LineState::Problem);
            AuditMode::Strict
        }
    }

    /// `LF_LOG` : filtre `tracing`, gardé seulement s'il est valide.
    fn log_filter(&mut self) -> Option<String> {
        let read = self.read(LOG, None);
        let Some(raw) = read.value else {
            self.line(LOG, Source::Default, "default filter", LineState::Ok);
            return None;
        };
        if EnvFilter::try_new(&raw).is_ok() {
            self.line(LOG, read.source, raw.clone(), LineState::Ok);
            Some(raw)
        } else {
            self.notices.push(Notice::InvalidLogFilter);
            self.line(LOG, Source::Default, "default filter", LineState::Replaced);
            None
        }
    }

    /// Entier de `name` dans `min..=max`. Absent : `default`. Invalide ou hors bornes : `default`
    /// et un avertissement, sans citer la valeur lue.
    fn bounded(&mut self, name: &'static str, min: u32, max: u32, default: u32) -> u32 {
        let read = self.read(name, None);
        let Some(raw) = read.value else {
            self.line(name, Source::Default, default.to_string(), LineState::Ok);
            return default;
        };
        let accepted = raw.parse::<u32>().ok().filter(|value| (min..=max).contains(value));
        if let Some(value) = accepted {
            self.line(name, read.source, value.to_string(), LineState::Ok);
            value
        } else {
            self.notices.push(Notice::InvalidSetting {
                variable: name,
                expected: format!("an integer from {min} to {max}"),
                default: default.to_string(),
            });
            self.line(name, Source::Default, default.to_string(), LineState::Replaced);
            default
        }
    }

    /// `LF_PISTE_MOJIBAKE_REPAIR` : `on` ou `off` (casse ignorée), `on` par défaut.
    fn mojibake_repair(&mut self) -> bool {
        let read = self.read(MOJIBAKE_REPAIR, None);
        let Some(raw) = read.value else {
            self.line(MOJIBAKE_REPAIR, Source::Default, "on", LineState::Ok);
            return true;
        };
        let parsed = if raw.eq_ignore_ascii_case("on") {
            Some(true)
        } else if raw.eq_ignore_ascii_case("off") {
            Some(false)
        } else {
            None
        };
        if let Some(on) = parsed {
            self.line(MOJIBAKE_REPAIR, read.source, if on { "on" } else { "off" }, LineState::Ok);
            on
        } else {
            self.notices.push(Notice::InvalidSetting {
                variable: MOJIBAKE_REPAIR,
                expected: "on or off".to_owned(),
                default: "on".to_owned(),
            });
            self.line(MOJIBAKE_REPAIR, Source::Default, "on", LineState::Replaced);
            true
        }
    }
}

/// Valeur sans blancs de bord, ou `None` si elle est vide ou blanche.
fn non_blank(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
}

/// `sandbox` ou `production`, blancs de bord et casse ignorés.
fn parse_environment(raw: &str) -> Option<PisteEnvironment> {
    let raw = raw.trim();
    [PisteEnvironment::Sandbox, PisteEnvironment::Production]
        .into_iter()
        .find(|env| raw.eq_ignore_ascii_case(env.name()))
}

/// Début d'un identifiant client : `check-config` n'en montre pas davantage.
fn short_id(id: &str) -> String {
    let head: String = id.chars().take(CLIENT_ID_SHOWN_CHARS).collect();
    if id.chars().count() > CLIENT_ID_SHOWN_CHARS { format!("{head}...") } else { head }
}

/// Vrai si l'avertissement porte sur l'URL de libellé `setting` (« OAuth URL » ou « base URL »).
fn concerns(warning: &ConfigWarning, setting: &str) -> bool {
    match warning {
        ConfigWarning::InsecureLoopback { setting: concerned }
        | ConfigWarning::CustomHost { setting: concerned, .. } => *concerned == setting,
    }
}

/// Pourquoi le chemin d'une variable est refusé.
enum PathProblem {
    /// Le chemin n'est pas absolu.
    NotAbsolute,
    /// Le chemin commence par `~/` et le répertoire personnel est introuvable.
    NoHome,
}

/// Chemin d'une variable de répertoire : `~/` développé, puis chemin absolu exigé.
fn absolute_path(env: &impl EnvSource, raw: &str) -> Result<PathBuf, PathProblem> {
    let path = match raw.strip_prefix("~/") {
        Some(rest) => env.home_dir().ok_or(PathProblem::NoHome)?.join(rest),
        None => PathBuf::from(raw),
    };
    if path.is_absolute() { Ok(path) } else { Err(PathProblem::NotAbsolute) }
}

/// Le répertoire DILA, à condition que le binaire embarque l'index local (`compiled`).
///
/// `compiled` est un paramètre et non un `cfg!` dans une garde : sous la feature, la garde valait
/// `false` pour tous les tests et sa mutation était indiscernable.
fn dila_dir_if_compiled(dir: PathBuf, compiled: bool) -> Result<PathBuf, DilaDirProblem> {
    if compiled { Ok(dir) } else { Err(DilaDirProblem::NotCompiled) }
}

/// `LF_AUDIT_DIR` posé explicitement : `~/` développé, puis chemin absolu exigé.
fn explicit_audit_dir(env: &impl EnvSource, raw: &str) -> Result<PathBuf, AuditDirProblem> {
    absolute_path(env, raw).map_err(|problem| match problem {
        PathProblem::NotAbsolute => AuditDirProblem::NotAbsolute,
        PathProblem::NoHome => AuditDirProblem::NoHome,
    })
}

/// Entier non nul à partir d'une valeur déjà bornée à 1 au moins.
fn non_zero_u32(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap_or(NonZeroU32::MIN)
}

/// Entier non nul de 16 bits à partir d'une valeur déjà bornée à 1..=16.
fn non_zero_u16(value: u32) -> NonZeroU16 {
    u16::try_from(value).ok().and_then(NonZeroU16::new).unwrap_or(NonZeroU16::MIN)
}

/// Charge la configuration depuis `env` : aucune E/S, aucun journal.
///
/// Les lignes de [`Loaded::lines`] suivent l'ordre de la table de 02 §8.2, et les [`Notice`] celui
/// de la lecture. Une valeur invalide ne fait jamais échouer : voir le module.
pub(crate) fn load(env: &impl EnvSource) -> Loaded {
    let mut reader = Collector { env, notices: Vec::new(), lines: Vec::new() };

    let (piste_env, env_problem) = reader.environment();
    // L'index local ne demande pas d'identifiants PISTE : leur absence n'est alors pas signalée.
    let credentials = reader.credentials(!env.is_set(DILA_DIR));
    let (endpoints, custom_host) = reader.endpoints(piste_env, env_problem);
    let mut problems: Vec<String> =
        endpoints.as_ref().err().map(ToString::to_string).into_iter().collect();

    let audit_dir = reader.audit_dir();
    let audit = AuditSettings { dir: audit_dir, mode: reader.audit_mode() };
    let log_filter = reader.log_filter();

    let request_timeout = reader.bounded(HTTP_TIMEOUT, 1, 300, DEFAULT_HTTP_TIMEOUT_S);
    let tool_deadline = reader.bounded(TOOL_DEADLINE, 1, 600, DEFAULT_TOOL_DEADLINE_S);
    let per_second = reader.bounded(RPS, 1, 100, DEFAULT_RPS);
    let burst = reader.bounded(BURST, 1, 100, DEFAULT_BURST);
    let concurrency = reader.bounded(CONCURRENCY, 1, 16, DEFAULT_CONCURRENCY);
    let repair_mojibake = reader.mojibake_repair();
    let source = reader.corpus_source();
    // `credentials_configured` dit que la source n'attend rien de plus pour répondre (FR-102,
    // statut de `fr_coverage`) : l'index local DILA n'a pas besoin d'identifiants PISTE.
    let credentials_configured = credentials.is_some() || !matches!(source, CorpusSource::Piste);
    if let CorpusSource::Unusable(problem) = &source {
        problems.push(problem.to_string());
    }

    // Les variables héritées jamais lues ne sont que signalées.
    for variable in IGNORED_LEGACY {
        if env.is_set(variable) {
            reader.notices.push(Notice::Ignored { variable });
        }
    }

    let tool_deadline = Duration::from_secs(u64::from(tool_deadline));
    let tuning = PisteTuning {
        request_timeout: Duration::from_secs(u64::from(request_timeout)),
        tool_deadline,
        rate: RateQuota::new(non_zero_u32(per_second), non_zero_u32(burst)),
        max_concurrency: non_zero_u16(concurrency),
        repair_mojibake,
        ..PisteTuning::default()
    };
    let base = PisteConfig::new(piste_env, endpoints, credentials, USER_AGENT.to_owned());
    // Les bornes ci-dessus excluent tout réglage nul : le repli sur les défauts est inatteignable.
    let piste = base.clone().with_tuning(tuning).unwrap_or(base);

    let config = AppConfig {
        piste,
        audit,
        tool_deadline,
        log_filter,
        configuration: ConfigurationState {
            credentials_configured,
            custom_host,
            problems,
            // L'index local est un instantané : une absence n'y prouve rien et `fr_coverage` ne le
            // présente pas comme du direct. Une source inutilisable ne sert aucun texte.
            local_snapshot: matches!(source, CorpusSource::Dila(_)),
        },
        source,
    };
    Loaded { config, notices: reader.notices, lines: reader.lines }
}

/// Environnement de test partagé avec `check` : une table, jamais l'environnement du processus (PT4).
#[cfg(test)]
pub(crate) mod test_env {
    use super::{BTreeMap, EnvSource, PathBuf};

    /// Chemin absolu d'un seul composant, sur toutes les plateformes.
    pub(crate) fn abs(component: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!("C:\\{component}"))
        } else {
            PathBuf::from(format!("/{component}"))
        }
    }

    /// Chaîne d'un chemin absolu (valeur de variable d'environnement).
    pub(crate) fn abs_str(component: &str) -> String {
        abs(component).to_string_lossy().into_owned()
    }

    /// Une table de variables, et des répertoires de plateforme réglables.
    pub(crate) struct MapEnv {
        /// Variables définies.
        pub(crate) vars: BTreeMap<String, String>,
        /// Répertoire personnel (`/home` par défaut).
        pub(crate) home: Option<PathBuf>,
        /// Répertoire de données (`/data` par défaut).
        pub(crate) data: Option<PathBuf>,
    }

    impl MapEnv {
        /// Table des couples `(nom, valeur)`.
        pub(crate) fn new(pairs: &[(&str, &str)]) -> Self {
            Self {
                vars: pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
                home: Some(abs("home")),
                data: Some(abs("data")),
            }
        }
    }

    impl EnvSource for MapEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn home_dir(&self) -> Option<PathBuf> {
            self.home.clone()
        }
        fn data_dir(&self) -> Option<PathBuf> {
            self.data.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_env::{MapEnv, abs, abs_str};
    use super::*;
    use secrecy::ExposeSecret;
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    const ID: &str = "11112222-3333-4444-5555-666677778888";
    const SECRET: &str = "99990000-aaaa-bbbb-cccc-ddddeeeeffff";

    fn loaded(pairs: &[(&str, &str)]) -> Loaded {
        load(&MapEnv::new(pairs))
    }

    /// Configuration avec identifiants valides, plus `extra`.
    fn with_credentials(extra: &[(&str, &str)]) -> Loaded {
        let mut pairs = vec![(CLIENT_ID, ID), (CLIENT_SECRET, SECRET)];
        pairs.extend_from_slice(extra);
        loaded(&pairs)
    }

    /// Tampon qui reçoit le journal `tracing`.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Ce que `Loaded::emit` écrit dans le journal.
    fn logs_of(loaded: &Loaded) -> String {
        let sink = Sink::default();
        let writer = sink.clone();
        let subscriber =
            tracing_subscriber::fmt().with_writer(move || writer.clone()).without_time().finish();
        tracing::subscriber::with_default(subscriber, || loaded.emit());
        let bytes = sink.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    fn line<'a>(loaded: &'a Loaded, name: &str) -> &'a SettingLine {
        loaded.lines.iter().find(|line| line.name == name).unwrap()
    }

    fn tuning_of(loaded: &Loaded) -> PisteTuning {
        *loaded.config.piste.tuning()
    }

    fn problem_of(loaded: &Loaded) -> ConfigProblem {
        loaded.config.piste.endpoints().unwrap_err().clone()
    }

    // --- Défauts --------------------------------------------------------------------------------

    #[test]
    fn defaults_match_the_spec() {
        let loaded = with_credentials(&[]);
        assert!(loaded.is_valid());
        assert_eq!(loaded.notices, Vec::new());
        let config = &loaded.config;
        assert_eq!(config.piste.env(), PisteEnvironment::Sandbox);
        assert_eq!(
            config.piste.endpoints().unwrap(),
            &PisteEndpoints::official(PisteEnvironment::Sandbox)
        );
        assert_eq!(config.piste.credentials().unwrap().client_id(), ID);
        assert_eq!(
            config.piste.user_agent(),
            format!(
                "mcp-legifrance/{} (+https://github.com/muchiny/mcp-legifrance)",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(config.audit.mode, AuditMode::Strict);
        assert_eq!(config.audit.dir, Ok(abs("data").join("mcp-legifrance").join("audit")));
        assert_eq!(config.log_filter, None);
        assert_eq!(config.tool_deadline, Duration::from_secs(60));
        assert_eq!(
            config.configuration,
            ConfigurationState {
                credentials_configured: true,
                custom_host: false,
                problems: Vec::new(),
                local_snapshot: false
            }
        );
    }

    #[test]
    fn tuning_defaults_are_the_ones_of_the_adapter() {
        let tuning = tuning_of(&with_credentials(&[]));
        assert_eq!(tuning, PisteTuning::default());
        assert_eq!(tuning.request_timeout, Duration::from_secs(40));
        assert_eq!(tuning.tool_deadline, Duration::from_secs(60));
        assert_eq!(tuning.rate.per_second().get(), 5);
        assert_eq!(tuning.rate.burst().get(), 10);
        assert_eq!(tuning.max_concurrency.get(), 4);
        assert!(tuning.repair_mojibake);
    }

    #[test]
    fn missing_credentials_start_degraded_with_one_problem() {
        let loaded = loaded(&[]);
        assert_eq!(loaded.notices, vec![Notice::CredentialsMissing]);
        assert!(!loaded.is_valid());
        assert!(loaded.config.piste.credentials().is_none());
        assert!(!loaded.config.configuration.credentials_configured);
        // L'absence d'identifiants a son drapeau ; la liste de problèmes est celle de FR-104.
        assert_eq!(loaded.config.configuration.problems, Vec::<String>::new());
        assert_eq!(
            Notice::CredentialsMissing.to_string(),
            "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET"
        );
    }

    #[test]
    fn one_credential_alone_is_not_enough() {
        for pairs in [[(CLIENT_ID, ID)], [(CLIENT_SECRET, SECRET)]] {
            let loaded = loaded(&pairs);
            assert!(loaded.config.piste.credentials().is_none());
            assert_eq!(loaded.notices, vec![Notice::CredentialsMissing]);
        }
    }

    #[test]
    fn credentials_are_trimmed_and_blank_means_absent() {
        let loaded = loaded(&[(CLIENT_ID, "  abc  "), (CLIENT_SECRET, "\t s3 \n")]);
        let credentials = loaded.config.piste.credentials().unwrap();
        assert_eq!(credentials.client_id(), "abc");
        assert_eq!(credentials.client_secret().expose_secret(), "s3");
        let blank = loaded_blank();
        assert!(blank.config.piste.credentials().is_none());
        assert_eq!(blank.notices, vec![Notice::CredentialsMissing]);
    }

    fn loaded_blank() -> Loaded {
        loaded(&[(CLIENT_ID, "   "), (CLIENT_SECRET, "")])
    }

    // --- Compatibilité FR_ELI_* -----------------------------------------------------------------

    #[test]
    fn fr_eli_variables_are_read_with_deprecation_warning() {
        let legacy_dir = abs_str("legacy-audit");
        let loaded = loaded(&[
            (LEGACY_CLIENT_ID, ID),
            (LEGACY_CLIENT_SECRET, SECRET),
            (LEGACY_OAUTH_URL, "https://proxy.example/oauth"),
            (LEGACY_BASE_URL, "https://proxy.example/api/"),
            (LEGACY_AUDIT_DIR, legacy_dir.as_str()),
        ]);
        // Les valeurs héritées sont bien lues…
        let config = &loaded.config;
        assert_eq!(config.piste.credentials().unwrap().client_id(), ID);
        let endpoints = config.piste.endpoints().unwrap();
        assert_eq!(endpoints.oauth_url().as_str(), "https://proxy.example/oauth");
        assert_eq!(endpoints.base_url().as_str(), "https://proxy.example/api");
        assert_eq!(config.audit.dir, Ok(abs("legacy-audit")));
        assert!(config.configuration.credentials_configured);
        assert!(config.configuration.custom_host);
        // … avec un avertissement de dépréciation par variable.
        let deprecated: Vec<_> = loaded
            .notices
            .iter()
            .filter(|notice| matches!(notice, Notice::Deprecated { .. }))
            .cloned()
            .collect();
        assert_eq!(
            deprecated,
            vec![
                Notice::Deprecated { variable: LEGACY_CLIENT_ID, replacement: CLIENT_ID },
                Notice::Deprecated { variable: LEGACY_CLIENT_SECRET, replacement: CLIENT_SECRET },
                Notice::Deprecated { variable: LEGACY_OAUTH_URL, replacement: OAUTH_URL },
                Notice::Deprecated { variable: LEGACY_BASE_URL, replacement: BASE_URL },
                Notice::Deprecated { variable: LEGACY_AUDIT_DIR, replacement: AUDIT_DIR },
            ]
        );
        assert_eq!(
            deprecated[0].to_string(),
            "FR_ELI_CLIENT_ID is deprecated: use LF_PISTE_CLIENT_ID"
        );
        // Le journal porte le champ `variable` et son remplaçant, au niveau `warn`.
        let logs = logs_of(&loaded);
        assert!(logs.contains("WARN"), "{logs}");
        assert!(logs.contains("deprecated environment variable"), "{logs}");
        assert!(logs.contains(r#"variable="FR_ELI_CLIENT_ID""#), "{logs}");
        assert!(logs.contains(r#"replacement="LF_PISTE_CLIENT_ID""#), "{logs}");
        assert_eq!(logs.matches("deprecated environment variable").count(), 5, "{logs}");
        // La ligne de `check-config` nomme la variable héritée comme source.
        assert_eq!(line(&loaded, CLIENT_ID).source, Source::Variable(LEGACY_CLIENT_ID));
        assert_eq!(line(&loaded, CLIENT_ID).source.label(), "FR_ELI_CLIENT_ID");
    }

    #[test]
    fn lf_variables_win_over_fr_eli_with_an_ignored_warning() {
        let other = abs_str("other");
        let audit_dir = abs_str("audit");
        let loaded = loaded(&[
            (CLIENT_ID, ID),
            (LEGACY_CLIENT_ID, "legacy-id"),
            (CLIENT_SECRET, SECRET),
            (AUDIT_DIR, audit_dir.as_str()),
            (LEGACY_AUDIT_DIR, other.as_str()),
        ]);
        assert_eq!(loaded.config.piste.credentials().unwrap().client_id(), ID);
        assert_eq!(loaded.config.audit.dir, Ok(abs("audit")));
        assert_eq!(
            loaded.notices,
            vec![
                Notice::Shadowed { variable: LEGACY_CLIENT_ID, replacement: CLIENT_ID },
                Notice::Shadowed { variable: LEGACY_AUDIT_DIR, replacement: AUDIT_DIR },
            ]
        );
        assert_eq!(
            loaded.notices[0].to_string(),
            "FR_ELI_CLIENT_ID ignored: LF_PISTE_CLIENT_ID is also set"
        );
        assert!(loaded.is_valid());
        assert_eq!(line(&loaded, CLIENT_ID).source, Source::Variable(CLIENT_ID));
        assert!(logs_of(&loaded).contains("FR_ELI_CLIENT_ID ignored"));
    }

    #[test]
    fn blank_lf_variable_lets_the_legacy_one_through() {
        let loaded = loaded(&[(CLIENT_ID, "  "), (LEGACY_CLIENT_ID, ID), (CLIENT_SECRET, SECRET)]);
        assert_eq!(loaded.config.piste.credentials().unwrap().client_id(), ID);
        assert_eq!(
            loaded.notices,
            vec![Notice::Deprecated { variable: LEGACY_CLIENT_ID, replacement: CLIENT_ID }]
        );
    }

    #[test]
    fn legacy_unread_variables_warn_ignored() {
        let loaded = loaded(&[
            ("FR_ELI_CACHE_DIR", "/cache/LEAK"),
            ("FR_ELI_RUNTIME_URL", "http://127.0.0.1:9/LEAK"),
            ("FR_ELI_ENV", "production"),
            ("PISTE_CLIENT_ID", "leak-id"),
            ("PISTE_CLIENT_SECRET", "leak-secret"),
        ]);
        let ignored: Vec<_> = loaded
            .notices
            .iter()
            .filter_map(|notice| match notice {
                Notice::Ignored { variable } => Some(*variable),
                _ => None,
            })
            .collect();
        assert_eq!(
            ignored,
            [
                "FR_ELI_CACHE_DIR",
                "FR_ELI_RUNTIME_URL",
                "FR_ELI_ENV",
                "PISTE_CLIENT_ID",
                "PISTE_CLIENT_SECRET"
            ]
        );
        assert_eq!(
            Notice::Ignored { variable: "FR_ELI_ENV" }.to_string(),
            "FR_ELI_ENV ignored: it is never read"
        );
        // Jamais lues : ni l'environnement, ni les identifiants n'en dépendent.
        assert_eq!(loaded.config.piste.env(), PisteEnvironment::Sandbox);
        assert!(loaded.config.piste.credentials().is_none());
        assert!(!format!("{loaded:?}").contains("LEAK"));
        assert!(!format!("{loaded:?}").to_lowercase().contains("leak-"));
        // Journalisé au démarrage, au niveau `warn`.
        let logs = logs_of(&loaded);
        assert_eq!(logs.matches("ignored: it is never read").count(), 5, "{logs}");
        assert!(logs.contains("WARN"), "{logs}");
        // Une valeur blanche compte comme absente.
        let blank = loaded_with_blank_legacy();
        assert!(!blank.notices.iter().any(|notice| matches!(notice, Notice::Ignored { .. })));
    }

    fn loaded_with_blank_legacy() -> Loaded {
        loaded(&[("FR_ELI_ENV", "  "), ("PISTE_CLIENT_ID", "")])
    }

    #[test]
    fn fr_eli_runtime_url_is_ignored() {
        let loaded = with_credentials(&[("FR_ELI_RUNTIME_URL", "http://127.0.0.1:9/manifest")]);
        assert_eq!(loaded.notices, vec![Notice::Ignored { variable: "FR_ELI_RUNTIME_URL" }]);
        assert!(loaded.is_valid(), "un avertissement n'invalide pas la configuration");
        // Aucune adresse ne dépend de cette variable : les URL restent celles du bac à sable.
        let endpoints = loaded.config.piste.endpoints().unwrap();
        assert_eq!(endpoints, &PisteEndpoints::official(PisteEnvironment::Sandbox));
        assert!(!format!("{loaded:?}").contains("127.0.0.1"));
        assert!(!loaded.config.configuration.custom_host);
        let logs = logs_of(&loaded);
        assert!(logs.contains("FR_ELI_RUNTIME_URL ignored"), "{logs}");
        assert!(logs.contains("WARN"), "{logs}");
        assert!(!logs.contains("127.0.0.1"), "{logs}");
    }

    // --- Réglages avancés -----------------------------------------------------------------------

    /// Valeur lue d'un réglage numérique, en unités de l'adapter.
    fn setting_value(loaded: &Loaded, name: &str) -> u64 {
        let tuning = tuning_of(loaded);
        [
            (HTTP_TIMEOUT, tuning.request_timeout.as_secs()),
            (TOOL_DEADLINE, tuning.tool_deadline.as_secs()),
            (RPS, u64::from(tuning.rate.per_second().get())),
            (BURST, u64::from(tuning.rate.burst().get())),
            (CONCURRENCY, u64::from(tuning.max_concurrency.get())),
        ]
        .into_iter()
        .find_map(|(candidate, value)| (candidate == name).then_some(value))
        .unwrap()
    }

    /// Réglages numériques : nom, borne basse, borne haute, défaut.
    const NUMERIC: [(&str, u32, u32, u32); 5] = [
        (HTTP_TIMEOUT, 1, 300, 40),
        (TOOL_DEADLINE, 1, 600, 60),
        (RPS, 1, 100, 5),
        (BURST, 1, 100, 10),
        (CONCURRENCY, 1, 16, 4),
    ];

    #[test]
    fn numeric_settings_accept_exactly_their_bounds() {
        for (name, min, max, default) in NUMERIC {
            for accepted in [min, max, u32::midpoint(min, max)] {
                let value = accepted.to_string();
                let loaded = with_credentials(&[(name, value.as_str())]);
                assert_eq!(setting_value(&loaded, name), u64::from(accepted), "{name}={value}");
                assert_eq!(loaded.notices, Vec::new(), "{name}={value}: {:?}", loaded.notices);
                assert_eq!(line(&loaded, name).value, value);
                assert_eq!(line(&loaded, name).state, LineState::Ok);
                assert_eq!(line(&loaded, name).source, Source::Variable(name));
            }
            // Blancs de bord ignorés.
            let padded_value = format!("  {max} \n");
            let padded = with_credentials(&[(name, padded_value.as_str())]);
            assert_eq!(setting_value(&padded, name), u64::from(max));
            // Absent ou blanc : défaut, sans avertissement.
            for absent in [None, Some(""), Some("   ")] {
                let pairs: Vec<(&str, &str)> =
                    absent.map(|value| (name, value)).into_iter().collect();
                let loaded = with_credentials(&pairs);
                assert_eq!(setting_value(&loaded, name), u64::from(default), "{name}");
                assert_eq!(loaded.notices, Vec::new());
                assert_eq!(line(&loaded, name).source, Source::Default);
            }
        }
    }

    #[test]
    fn numeric_settings_reject_everything_else_with_a_warning() {
        for (name, min, max, default) in NUMERIC {
            let too_big = (max + 1).to_string();
            for rejected in ["0", &too_big, "x", "-1", "1.5", "4294967296", "1e2", "dix", "LEAK-1"]
            {
                let loaded = with_credentials(&[(name, rejected)]);
                assert_eq!(setting_value(&loaded, name), u64::from(default), "{name}={rejected}");
                assert_eq!(
                    loaded.notices,
                    vec![Notice::InvalidSetting {
                        variable: name,
                        expected: format!("an integer from {min} to {max}"),
                        default: default.to_string(),
                    }],
                    "{name}={rejected}"
                );
                assert!(
                    loaded.is_valid(),
                    "un réglage avancé invalide n'invalide pas la configuration"
                );
                let shown = line(&loaded, name);
                assert_eq!(shown.value, default.to_string());
                assert_eq!(shown.state, LineState::Replaced);
                assert_eq!(shown.source, Source::Default);
                assert!(!format!("{loaded:?}").contains("LEAK"));
            }
        }
    }

    #[test]
    fn invalid_advanced_setting_falls_back_to_default_with_warn() {
        let loaded = with_credentials(&[
            (RPS, "0"),
            (BURST, "1000"),
            (CONCURRENCY, "x"),
            (MOJIBAKE_REPAIR, "maybe"),
        ]);
        // Défauts appliqués.
        let tuning = tuning_of(&loaded);
        assert_eq!(tuning.rate.per_second().get(), 5);
        assert_eq!(tuning.rate.burst().get(), 10);
        assert_eq!(tuning.max_concurrency.get(), 4);
        assert!(tuning.repair_mojibake);
        // Un avertissement par réglage, qui n'invalide pas la configuration.
        assert_eq!(loaded.notices.len(), 4, "{:?}", loaded.notices);
        assert!(loaded.is_valid());
        assert_eq!(
            loaded.notices[0].to_string(),
            "LF_PISTE_RPS is invalid (expected an integer from 1 to 100): using 5"
        );
        assert_eq!(
            loaded.notices[3].to_string(),
            "LF_PISTE_MOJIBAKE_REPAIR is invalid (expected on or off): using on"
        );
        let logs = logs_of(&loaded);
        assert_eq!(logs.matches("WARN").count(), 4, "{logs}");
        assert!(!logs.contains("ERROR"), "{logs}");
        // La valeur effective est celle que `check-config` affiche, marquée comme remplacée.
        for (name, value) in
            [(RPS, "5"), (BURST, "10"), (CONCURRENCY, "4"), (MOJIBAKE_REPAIR, "on")]
        {
            assert_eq!(line(&loaded, name).value, value, "{name}");
            assert_eq!(line(&loaded, name).state, LineState::Replaced, "{name}");
            assert_eq!(line(&loaded, name).source, Source::Default, "{name}");
        }
    }

    #[test]
    fn mojibake_repair_accepts_on_and_off_in_any_case() {
        for (raw, expected) in [
            ("on", true),
            ("ON", true),
            (" On ", true),
            ("off", false),
            ("OFF", false),
            (" oFf\n", false),
        ] {
            let loaded = with_credentials(&[(MOJIBAKE_REPAIR, raw)]);
            assert_eq!(tuning_of(&loaded).repair_mojibake, expected, "{raw:?}");
            assert_eq!(loaded.notices, Vec::new(), "{raw:?}");
            assert_eq!(line(&loaded, MOJIBAKE_REPAIR).value, if expected { "on" } else { "off" });
        }
        for blank in ["", "  "] {
            let loaded = with_credentials(&[(MOJIBAKE_REPAIR, blank)]);
            assert!(tuning_of(&loaded).repair_mojibake);
            assert_eq!(loaded.notices, Vec::new());
        }
    }

    #[test]
    fn tool_deadline_capped_at_600s() {
        // 600 s est la dernière valeur admise (00 §1) et reste telle quelle.
        let at_cap = with_credentials(&[(TOOL_DEADLINE, "600")]);
        assert_eq!(at_cap.config.tool_deadline, Duration::from_secs(600));
        assert_eq!(tuning_of(&at_cap).tool_deadline, Duration::from_secs(600));
        assert_eq!(at_cap.notices, Vec::new());
        // Au-delà, la valeur est invalide (02 §8.2) : défaut de 60 s et avertissement, jamais
        // un délai supérieur à 600 s, quelle que soit l'entrée.
        for beyond in
            ["601", "900", "3600", "100000", "4294967295", "4294967296", "18446744073709551616"]
        {
            let loaded = with_credentials(&[(TOOL_DEADLINE, beyond)]);
            assert_eq!(loaded.config.tool_deadline, Duration::from_secs(60), "{beyond}");
            assert_eq!(tuning_of(&loaded).tool_deadline, Duration::from_secs(60), "{beyond}");
            assert!(matches!(loaded.notices[..], [Notice::InvalidSetting { .. }]), "{beyond}");
        }
        for raw in ["0", "1", "59", "60", "599", "600", "601", "-5", "", "x"] {
            let loaded = with_credentials(&[(TOOL_DEADLINE, raw)]);
            assert!(loaded.config.tool_deadline <= Duration::from_secs(600), "{raw}");
            assert!(loaded.config.tool_deadline >= Duration::from_secs(1), "{raw}");
            assert_eq!(loaded.config.tool_deadline, tuning_of(&loaded).tool_deadline, "{raw}");
        }
    }

    // --- Environnement et adresses --------------------------------------------------------------

    #[test]
    fn piste_env_is_trimmed_and_case_insensitive() {
        for (raw, expected) in [
            ("sandbox", PisteEnvironment::Sandbox),
            (" Sandbox ", PisteEnvironment::Sandbox),
            ("production", PisteEnvironment::Production),
            ("PRODUCTION", PisteEnvironment::Production),
            ("  Production\n", PisteEnvironment::Production),
        ] {
            let loaded = with_credentials(&[(PISTE_ENV, raw)]);
            assert_eq!(loaded.config.piste.env(), expected, "{raw:?}");
            assert_eq!(
                loaded.config.piste.endpoints().unwrap(),
                &PisteEndpoints::official(expected)
            );
            assert_eq!(loaded.notices, Vec::new(), "{raw:?}");
            assert_eq!(line(&loaded, PISTE_ENV).value, expected.name());
            assert_eq!(line(&loaded, PISTE_ENV).state, LineState::Ok);
        }
        assert_eq!(line(&with_credentials(&[]), PISTE_ENV).source, Source::Default);
        assert_eq!(
            line(&with_credentials(&[(PISTE_ENV, "production")]), PISTE_ENV).source,
            Source::Variable(PISTE_ENV)
        );
    }

    #[test]
    fn invalid_piste_env_degrades_the_server_without_echoing_a_long_value() {
        let loaded = with_credentials(&[(PISTE_ENV, "prod")]);
        let text = "LF_PISTE_ENV='prod' is invalid: expected sandbox or production.";
        assert_eq!(problem_of(&loaded).to_string(), text);
        assert_eq!(loaded.notices, vec![Notice::Problem(ConfigProblem::invalid_env("prod"))]);
        assert!(!loaded.is_valid());
        assert_eq!(loaded.config.piste.env(), PisteEnvironment::Sandbox);
        assert_eq!(loaded.config.configuration.problems, vec![text.to_owned()]);
        assert_eq!(line(&loaded, PISTE_ENV).state, LineState::Problem);
        assert_eq!(line(&loaded, PISTE_ENV).value, "(rejected)");
        assert!(logs_of(&loaded).contains("ERROR"));

        let long = "a-very-long-value-that-looks-like-a-secret-0123456789";
        let hidden = with_credentials(&[(PISTE_ENV, long)]);
        let shown = problem_of(&hidden).to_string();
        assert!(shown.contains("value not shown"), "{shown}");
        assert!(!shown.contains(long));
    }

    #[test]
    fn production_hosts_without_production_env_is_an_env_mismatch() {
        let loaded = with_credentials(&[
            (BASE_URL, "https://api.piste.gouv.fr/dila/legifrance/lf-engine-app"),
            (OAUTH_URL, "https://oauth.piste.gouv.fr/api/oauth/token"),
        ]);
        let problem = problem_of(&loaded);
        assert!(matches!(problem, ConfigProblem::EnvMismatch { .. }), "{problem:?}");
        assert!(problem.to_string().contains("set LF_PISTE_ENV=production"));
        assert!(!loaded.is_valid());
        assert_eq!(loaded.config.configuration.problems.len(), 1);
        assert!(!loaded.config.configuration.custom_host);
        assert_eq!(line(&loaded, OAUTH_URL).state, LineState::Problem);
        assert_eq!(line(&loaded, OAUTH_URL).value, "(rejected)");
        assert_eq!(line(&loaded, BASE_URL).state, LineState::Problem);
        // Avec l'environnement explicite, les mêmes hôtes sont valides.
        let fixed = with_credentials(&[
            (PISTE_ENV, "production"),
            (BASE_URL, "https://api.piste.gouv.fr/dila/legifrance/lf-engine-app"),
            (OAUTH_URL, "https://oauth.piste.gouv.fr/api/oauth/token"),
        ]);
        assert!(fixed.is_valid());
        assert_eq!(fixed.notices, Vec::new());
    }

    #[test]
    fn invalid_env_hides_the_url_lines_it_did_not_check() {
        let loaded =
            with_credentials(&[(PISTE_ENV, "prod"), (BASE_URL, "https://proxy.example/api")]);
        assert_eq!(line(&loaded, BASE_URL).value, "(not checked)");
        assert_eq!(line(&loaded, BASE_URL).state, LineState::Problem);
        // L'URL non surchargée montre l'adresse par défaut.
        let oauth = line(&loaded, OAUTH_URL);
        assert_eq!(oauth.value, "https://sandbox-oauth.piste.gouv.fr/api/oauth/token");
        assert_eq!(oauth.state, LineState::Ok);
        assert_eq!(oauth.source, Source::Default);
    }

    #[test]
    fn custom_host_is_a_warning_not_a_problem() {
        let loaded = with_credentials(&[(OAUTH_URL, "https://proxy.example/oauth")]);
        assert!(loaded.is_valid());
        assert!(loaded.config.configuration.custom_host);
        assert_eq!(loaded.config.configuration.problems, Vec::<String>::new());
        assert_eq!(
            loaded.notices,
            vec![Notice::Warning(ConfigWarning::CustomHost {
                setting: "OAuth URL",
                host: "proxy.example".to_owned()
            })]
        );
        assert_eq!(
            loaded.notices[0].to_string(),
            "OAuth URL points to a non-official host proxy.example"
        );
        assert_eq!(line(&loaded, OAUTH_URL).state, LineState::Warning);
        assert_eq!(line(&loaded, OAUTH_URL).value, "https://proxy.example/oauth");
        assert_eq!(line(&loaded, BASE_URL).state, LineState::Ok);
        assert_eq!(line(&loaded, BASE_URL).source, Source::Default);
        let logs = logs_of(&loaded);
        assert!(logs.contains("WARN") && !logs.contains("ERROR"), "{logs}");
    }

    #[test]
    fn loopback_http_warns_twice_and_flags_only_its_own_line() {
        let loaded = with_credentials(&[(BASE_URL, "http://127.0.0.1:8080/api/")]);
        assert!(loaded.is_valid());
        assert_eq!(loaded.notices.len(), 2, "{:?}", loaded.notices);
        assert!(loaded.config.configuration.custom_host);
        assert_eq!(line(&loaded, BASE_URL).state, LineState::Warning);
        assert_eq!(line(&loaded, BASE_URL).value, "http://127.0.0.1:8080/api");
        assert_eq!(line(&loaded, OAUTH_URL).state, LineState::Ok);
    }

    #[test]
    fn urls_with_userinfo_are_rejected_without_being_shown() {
        for raw in [
            "https://uinfo-name:LEAK@proxy.example/x",
            "uinfo-name:LEAK@proxy.example",
            "http://uinfo-name:LEAK@127.0.0.1/x",
            "https://proxy.example/x?token=LEAK",
        ] {
            let loaded = with_credentials(&[(BASE_URL, raw)]);
            assert!(!loaded.is_valid(), "{raw}");
            assert!(loaded.config.piste.endpoints().is_err(), "{raw}");
            assert_eq!(loaded.config.configuration.problems.len(), 1);
            let everything = format!("{loaded:?}{}{}", loaded.notices[0], logs_of(&loaded));
            assert!(!everything.contains("LEAK"), "{raw}: {everything}");
            assert!(!everything.contains("uinfo-name"), "{raw}: {everything}");
        }
    }

    // --- Audit ----------------------------------------------------------------------------------

    #[test]
    fn audit_mode_defaults_to_strict() {
        let loaded = loaded(&[]);
        assert_eq!(loaded.config.audit.mode, AuditMode::Strict);
        assert_eq!(line(&loaded, AUDIT_MODE).value, "strict");
        assert_eq!(line(&loaded, AUDIT_MODE).source, Source::Default);
        assert_eq!(line(&loaded, AUDIT_MODE).state, LineState::Ok);
        // Absent ou blanc : toujours strict, sans avertissement (Q9).
        for blank in ["", "   "] {
            let loaded = with_credentials(&[(AUDIT_MODE, blank)]);
            assert_eq!(loaded.config.audit.mode, AuditMode::Strict);
            assert_eq!(loaded.notices, Vec::new());
        }
    }

    #[test]
    fn audit_mode_accepts_both_values_in_any_case() {
        for (raw, expected) in [
            ("strict", AuditMode::Strict),
            ("STRICT", AuditMode::Strict),
            ("best-effort", AuditMode::BestEffort),
            (" Best-Effort ", AuditMode::BestEffort),
        ] {
            let loaded = with_credentials(&[(AUDIT_MODE, raw)]);
            assert_eq!(loaded.config.audit.mode, expected, "{raw:?}");
            assert_eq!(loaded.notices, Vec::new(), "{raw:?}");
            assert_eq!(line(&loaded, AUDIT_MODE).source, Source::Variable(AUDIT_MODE));
        }
        assert_eq!(
            line(&with_credentials(&[(AUDIT_MODE, "best-effort")]), AUDIT_MODE).value,
            "best-effort"
        );
    }

    #[test]
    fn unknown_audit_mode_falls_back_to_strict_with_an_error() {
        for raw in ["best_effort", "lenient", "off", "LEAK-1234"] {
            let loaded = with_credentials(&[(AUDIT_MODE, raw)]);
            assert_eq!(loaded.config.audit.mode, AuditMode::Strict, "{raw}");
            assert_eq!(loaded.notices, vec![Notice::UnknownAuditMode], "{raw}");
            assert!(!loaded.is_valid());
            assert_eq!(line(&loaded, AUDIT_MODE).state, LineState::Problem);
            assert_eq!(line(&loaded, AUDIT_MODE).value, "strict");
            assert_eq!(line(&loaded, AUDIT_MODE).source, Source::Default);
            let logs = logs_of(&loaded);
            assert!(logs.contains("ERROR"), "{logs}");
            assert!(!logs.contains("LEAK"), "{logs}");
        }
        assert_eq!(
            Notice::UnknownAuditMode.to_string(),
            "LF_AUDIT_MODE must be strict or best-effort: using strict"
        );
    }

    #[test]
    fn audit_dir_comes_from_the_variable_the_home_or_the_platform() {
        let padded_dir = format!("  {}  ", abs_str("var"));
        // Chemin absolu explicite.
        let explicit = with_credentials(&[(AUDIT_DIR, padded_dir.as_str())]);
        assert_eq!(explicit.config.audit.dir, Ok(abs("var")));
        assert_eq!(line(&explicit, AUDIT_DIR).value, abs_str("var"));
        assert_eq!(line(&explicit, AUDIT_DIR).source, Source::Variable(AUDIT_DIR));
        assert_eq!(explicit.notices, Vec::new());
        // `~/` en tête développé avec le répertoire personnel.
        let tilde = with_credentials(&[(AUDIT_DIR, "~/journal")]);
        assert_eq!(tilde.config.audit.dir, Ok(abs("home").join("journal")));
        // Défaut : répertoire de données de la plateforme.
        let default = with_credentials(&[]);
        let expected = abs("data").join("mcp-legifrance").join("audit");
        assert_eq!(default.config.audit.dir, Ok(expected.clone()));
        assert_eq!(line(&default, AUDIT_DIR).value, expected.display().to_string());
        assert_eq!(line(&default, AUDIT_DIR).source, Source::Default);
        // Blanc : comme absent.
        assert_eq!(with_credentials(&[(AUDIT_DIR, " ")]).config.audit.dir, Ok(expected));
    }

    #[test]
    fn unusable_audit_dir_is_a_problem_not_a_failure() {
        let relative = with_credentials(&[(AUDIT_DIR, "relative/LEAK")]);
        assert_eq!(relative.config.audit.dir, Err(AuditDirProblem::NotAbsolute));
        assert_eq!(relative.notices, vec![Notice::AuditDir(AuditDirProblem::NotAbsolute)]);
        assert!(!relative.is_valid());
        assert_eq!(line(&relative, AUDIT_DIR).state, LineState::Problem);
        assert_eq!(line(&relative, AUDIT_DIR).value, "(unusable)");
        assert!(!format!("{relative:?}").contains("LEAK"));
        // Le mode d'audit n'est pas touché ; la configuration PISTE non plus.
        assert_eq!(relative.config.configuration.problems, Vec::<String>::new());

        let mut env = MapEnv::new(&[(AUDIT_DIR, "~/x")]);
        env.home = None;
        assert_eq!(load(&env).config.audit.dir, Err(AuditDirProblem::NoHome));

        let mut env = MapEnv::new(&[]);
        env.data = None;
        let loaded = load(&env);
        assert_eq!(loaded.config.audit.dir, Err(AuditDirProblem::NoDataDir));
        assert!(loaded.notices.contains(&Notice::AuditDir(AuditDirProblem::NoDataDir)));
        // Même sans HOME, un chemin absolu explicite reste utilisable.
        let var_dir = abs_str("var");
        let mut env = MapEnv::new(&[(AUDIT_DIR, var_dir.as_str())]);
        env.home = None;
        env.data = None;
        assert_eq!(load(&env).config.audit.dir, Ok(abs("var")));
    }

    #[test]
    fn audit_dir_problem_messages_name_the_variable() {
        assert_eq!(
            AuditDirProblem::NotAbsolute.to_string(),
            "LF_AUDIT_DIR must be an absolute path"
        );
        assert_eq!(
            AuditDirProblem::NoHome.to_string(),
            "LF_AUDIT_DIR starts with ~/ but the home directory is unknown"
        );
        assert!(AuditDirProblem::NoDataDir.to_string().contains("set LF_AUDIT_DIR"));
    }

    // --- Source locale DILA (M9) ----------------------------------------------------------------

    #[test]
    fn piste_is_the_source_unless_a_dila_dir_is_set() {
        let plain = with_credentials(&[]);
        assert_eq!(plain.config.source, CorpusSource::Piste);
        assert!(plain.lines.iter().all(|line| line.name != DILA_DIR));
        // Blanc : comme absent, et la table de `check-config` garde ses 14 lignes.
        let blank = with_credentials(&[(DILA_DIR, "  ")]);
        assert_eq!(blank.config.source, CorpusSource::Piste);
        assert_eq!(blank.lines.len(), 14);
        assert_eq!(blank.notices, Vec::new());
    }

    #[cfg(feature = "dila")]
    #[test]
    fn a_dila_dir_selects_the_local_index_and_needs_no_credentials() {
        let dir = abs_str("index");
        // Sans identifiants PISTE : la source locale n'en exige pas, la configuration est saine.
        let local = loaded(&[(DILA_DIR, format!("  {dir}  ").as_str())]);
        assert_eq!(local.config.source, CorpusSource::Dila(abs("index")));
        assert_eq!(local.notices, Vec::new());
        assert!(local.is_valid());
        assert!(local.config.configuration.credentials_configured);
        assert_eq!(local.config.configuration.problems, Vec::<String>::new());
        // L'index local est un instantané ; PISTE, même avec des identifiants, n'en est pas un.
        assert!(local.config.configuration.local_snapshot);
        // La ligne de `check-config` vient après les quatorze autres, et seulement si définie.
        assert_eq!(local.lines.len(), 15);
        let shown = local.lines.last().unwrap();
        assert_eq!(shown.name, DILA_DIR);
        assert_eq!(shown.source, Source::Variable(DILA_DIR));
        assert_eq!(shown.value, abs("index").display().to_string());
        assert_eq!(shown.state, LineState::Ok);
        // Le choix explicite l'emporte sur des identifiants PISTE valides.
        let both = with_credentials(&[(DILA_DIR, dir.as_str())]);
        assert_eq!(both.config.source, CorpusSource::Dila(abs("index")));
        assert!(both.config.configuration.local_snapshot);
        // `~/` en tête développé avec le répertoire personnel.
        let tilde = loaded(&[(DILA_DIR, "~/index")]);
        assert_eq!(tilde.config.source, CorpusSource::Dila(abs("home").join("index")));
    }

    #[test]
    fn a_dila_dir_is_kept_only_when_the_binary_embeds_the_local_index() {
        assert_eq!(dila_dir_if_compiled(abs("index"), true), Ok(abs("index")));
        assert_eq!(dila_dir_if_compiled(abs("index"), false), Err(DilaDirProblem::NotCompiled));
    }

    #[cfg(not(feature = "dila"))]
    #[test]
    fn a_dila_dir_without_the_feature_is_unusable_and_never_falls_back_to_piste() {
        let dir = abs_str("index");
        let loaded = with_credentials(&[(DILA_DIR, dir.as_str())]);
        assert_eq!(loaded.config.source, CorpusSource::Unusable(DilaDirProblem::NotCompiled));
        assert_eq!(loaded.notices, vec![Notice::DilaDir(DilaDirProblem::NotCompiled)]);
        assert!(!loaded.is_valid());
        assert_eq!(loaded.config.configuration.problems, [DilaDirProblem::NotCompiled.to_string()]);
        assert_eq!(line(&loaded, DILA_DIR).state, LineState::Problem);
    }

    #[test]
    fn an_unusable_dila_dir_is_a_problem_and_never_falls_back_to_piste() {
        // Des identifiants valides n'y changent rien : la source demandée n'est pas PISTE.
        let relative = with_credentials(&[(DILA_DIR, "relative/LEAK")]);
        assert_eq!(relative.config.source, CorpusSource::Unusable(DilaDirProblem::NotAbsolute));
        assert_eq!(relative.notices, vec![Notice::DilaDir(DilaDirProblem::NotAbsolute)]);
        assert!(!relative.is_valid());
        assert_eq!(
            relative.config.configuration.problems,
            ["LF_DILA_DIR must be an absolute path".to_owned()]
        );
        assert_eq!(line(&relative, DILA_DIR).state, LineState::Problem);
        // Une source inutilisable ne sert aucun texte : ce n'est pas un instantané.
        assert!(!relative.config.configuration.local_snapshot);
        assert_eq!(line(&relative, DILA_DIR).value, "(unusable)");
        assert!(!format!("{relative:?}").contains("LEAK"));
        // Sans identifiants non plus, aucune notice sur PISTE : il n'est pas employé.
        let bare = loaded(&[(DILA_DIR, "relative")]);
        assert_eq!(bare.notices, vec![Notice::DilaDir(DilaDirProblem::NotAbsolute)]);

        let mut env = MapEnv::new(&[(DILA_DIR, "~/index")]);
        env.home = None;
        let no_home = load(&env);
        assert_eq!(no_home.config.source, CorpusSource::Unusable(DilaDirProblem::NoHome));
        assert!(no_home.notices.contains(&Notice::DilaDir(DilaDirProblem::NoHome)));
    }

    #[test]
    fn the_startup_log_names_the_source_without_its_path() {
        assert_eq!(CorpusSource::Piste.label(), "piste");
        assert_eq!(CorpusSource::Dila(abs("secret-place")).label(), "dila");
        assert_eq!(CorpusSource::Unusable(DilaDirProblem::NoHome).label(), "unavailable");
    }

    #[test]
    fn dila_dir_problem_messages_name_the_variable_and_never_the_value() {
        assert_eq!(DilaDirProblem::NotAbsolute.to_string(), "LF_DILA_DIR must be an absolute path");
        assert_eq!(
            DilaDirProblem::NoHome.to_string(),
            "LF_DILA_DIR starts with ~/ but the home directory is unknown"
        );
        assert_eq!(
            DilaDirProblem::NotCompiled.to_string(),
            "LF_DILA_DIR is set but this build has no local DILA index: it needs the dila feature"
        );
        for problem in
            [DilaDirProblem::NotAbsolute, DilaDirProblem::NoHome, DilaDirProblem::NotCompiled]
        {
            let notice = Notice::DilaDir(problem);
            assert!(notice.is_problem(), "{notice}");
            assert_eq!(notice.to_string(), problem.to_string());
        }
    }

    /// V-180 : les contenus `dereferenced` et `inap` ne sont pas indexés. Aucune variable de la
    /// liste fermée ne permet de les admettre, ni d'élargir la source locale aux fonds qui les
    /// portent : l'exclusion ne se désactive pas par la configuration.
    #[test]
    fn no_variable_admits_dereferenced_or_inap_content() {
        for name in READ_VARIABLES {
            let upper = name.to_ascii_uppercase();
            for forbidden in ["DEREFERENCED", "INAP", "PUBLIC", "ALL_FUNDS", "JORF"] {
                assert!(!upper.contains(forbidden), "{name} could widen the local source");
            }
        }
        // La seule variable de la source locale désigne un répertoire, rien d'autre.
        assert_eq!(READ_VARIABLES.iter().filter(|name| name.contains("DILA")).count(), 1);
    }

    // --- Journal --------------------------------------------------------------------------------

    #[test]
    fn log_filter_is_kept_only_when_valid() {
        let valid = with_credentials(&[(LOG, "debug,rmcp=warn")]);
        assert_eq!(valid.config.log_filter.as_deref(), Some("debug,rmcp=warn"));
        assert_eq!(valid.notices, Vec::new());
        assert_eq!(line(&valid, LOG).value, "debug,rmcp=warn");

        for invalid in ["info,mcp=notalevel", "LEAK-1234["] {
            let loaded = with_credentials(&[(LOG, invalid)]);
            assert_eq!(loaded.config.log_filter, None, "{invalid}");
            assert_eq!(loaded.notices, vec![Notice::InvalidLogFilter], "{invalid}");
            assert!(loaded.is_valid());
            assert_eq!(line(&loaded, LOG).state, LineState::Replaced);
            assert_eq!(line(&loaded, LOG).value, "default filter");
            assert!(!format!("{loaded:?}").contains("LEAK"));
        }
        assert_eq!(
            Notice::InvalidLogFilter.to_string(),
            "LF_LOG is not a valid filter: using the default filter"
        );
        // Absent ou blanc : filtre par défaut de `logging`.
        for blank in [None, Some("  ")] {
            let pairs: Vec<(&str, &str)> = blank.map(|value| (LOG, value)).into_iter().collect();
            let loaded = with_credentials(&pairs);
            assert_eq!(loaded.config.log_filter, None);
            assert_eq!(line(&loaded, LOG).source, Source::Default);
            assert_eq!(loaded.notices, Vec::new());
        }
    }

    // --- Lignes de check-config -----------------------------------------------------------------

    #[test]
    fn lines_follow_the_table_of_the_spec() {
        let loaded = with_credentials(&[]);
        let names: Vec<_> = loaded.lines.iter().map(|line| line.name).collect();
        assert_eq!(
            names,
            [
                "LF_PISTE_ENV",
                "LF_PISTE_CLIENT_ID",
                "LF_PISTE_CLIENT_SECRET",
                "LF_PISTE_OAUTH_URL",
                "LF_PISTE_BASE_URL",
                "LF_AUDIT_DIR",
                "LF_AUDIT_MODE",
                "LF_LOG",
                "LF_HTTP_TIMEOUT_S",
                "LF_TOOL_DEADLINE_S",
                "LF_PISTE_RPS",
                "LF_PISTE_BURST",
                "LF_PISTE_CONCURRENCY",
                "LF_PISTE_MOJIBAKE_REPAIR",
            ]
        );
        // Même sans rien de défini, les 14 variables ont leur ligne.
        assert_eq!(self::loaded(&[]).lines.len(), 14);
        assert_eq!(
            line(&loaded, OAUTH_URL).value,
            "https://sandbox-oauth.piste.gouv.fr/api/oauth/token"
        );
        assert_eq!(
            line(&loaded, BASE_URL).value,
            "https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app"
        );
    }

    #[test]
    fn secret_is_masked_and_client_id_truncated() {
        let loaded = with_credentials(&[]);
        let secret = line(&loaded, CLIENT_SECRET);
        assert_eq!(secret.value, format!("set ({} characters)", SECRET.chars().count()));
        assert_eq!(secret.state, LineState::Ok);
        let id = line(&loaded, CLIENT_ID);
        assert_eq!(id.value, "1111...");
        assert!(!format!("{:?}", loaded.lines).contains(SECRET));
        assert!(!format!("{:?}", loaded.lines).contains(ID));
        // Non défini : pas de valeur à montrer.
        let none = self::loaded(&[]);
        assert_eq!(line(&none, CLIENT_ID).value, "not set");
        assert_eq!(line(&none, CLIENT_ID).state, LineState::Unset);
        assert_eq!(line(&none, CLIENT_SECRET).value, "not set");
        assert_eq!(line(&none, CLIENT_SECRET).source, Source::Default);
        // Le décompte est en caractères, pas en octets.
        let accents = self::loaded(&[(CLIENT_ID, "é"), (CLIENT_SECRET, "ééé")]);
        assert_eq!(line(&accents, CLIENT_SECRET).value, "set (3 characters)");
    }

    #[test]
    fn short_id_shows_at_most_four_characters() {
        assert_eq!(short_id(""), "");
        assert_eq!(short_id("ab"), "ab");
        assert_eq!(short_id("abcd"), "abcd");
        assert_eq!(short_id("abcde"), "abcd...");
        assert_eq!(short_id("éàüöx"), "éàüö...");
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(Source::Default.label(), "default");
        assert_eq!(Source::Variable("LF_X").label(), "LF_X");
        let labels = [
            (LineState::Ok, "ok"),
            (LineState::Unset, "not set"),
            (LineState::Replaced, "invalid, default used"),
            (LineState::Warning, "warning"),
            (LineState::Problem, "problem"),
        ];
        for (state, label) in labels {
            assert_eq!(state.label(), label);
        }
        let keys = [
            (LineState::Ok, "ok"),
            (LineState::Unset, "unset"),
            (LineState::Replaced, "replaced"),
            (LineState::Warning, "warning"),
            (LineState::Problem, "problem"),
        ];
        for (state, key) in keys {
            assert_eq!(state.key(), key);
        }
    }

    // --- Secrets --------------------------------------------------------------------------------

    const LEAK: &str = "LEAK-s3cr3t-0123456789abcdef";

    #[test]
    fn config_errors_never_contain_secret_or_userinfo() {
        let userinfo = format!("uinfo-name:{LEAK}@");
        let scenarios: Vec<(&str, String)> = vec![
            (PISTE_ENV, LEAK.to_owned()),
            (OAUTH_URL, format!("https://{userinfo}sandbox-oauth.piste.gouv.fr/api/oauth/token")),
            (BASE_URL, format!("https://{userinfo}evil.example/x")),
            (BASE_URL, format!("{userinfo}evil.example")),
            (BASE_URL, format!("{LEAK} is not a url")),
            (BASE_URL, format!("https://evil.example/x?token={LEAK}")),
            (BASE_URL, format!("https://evil.example/x#{LEAK}")),
            (OAUTH_URL, format!("http://{userinfo}127.0.0.1/x")),
            (LEGACY_BASE_URL, format!("https://{userinfo}evil.example/x")),
            (LEGACY_OAUTH_URL, format!("ftp://{userinfo}evil.example/x")),
            (AUDIT_MODE, LEAK.to_owned()),
            (AUDIT_DIR, format!("relative/{LEAK}")),
            (LEGACY_AUDIT_DIR, LEAK.to_owned()),
            (LOG, format!("{LEAK}[")),
            (HTTP_TIMEOUT, LEAK.to_owned()),
            (TOOL_DEADLINE, LEAK.to_owned()),
            (RPS, LEAK.to_owned()),
            (BURST, LEAK.to_owned()),
            (CONCURRENCY, LEAK.to_owned()),
            (MOJIBAKE_REPAIR, LEAK.to_owned()),
            ("PISTE_CLIENT_SECRET", LEAK.to_owned()),
            ("FR_ELI_CACHE_DIR", LEAK.to_owned()),
        ];
        for (variable, value) in &scenarios {
            // Le secret réel est lui aussi en place : il ne doit jamais ressortir.
            let loaded =
                loaded(&[(CLIENT_ID, ID), (CLIENT_SECRET, LEAK), (*variable, value.as_str())]);
            assert!(!loaded.notices.is_empty(), "{variable} : le scénario ne produit rien");
            let mut shown = format!("{loaded:?}");
            for notice in &loaded.notices {
                shown.push_str(&notice.to_string());
            }
            for state in &loaded.config.configuration.problems {
                shown.push_str(state);
            }
            shown.push_str(&logs_of(&loaded));
            assert!(!shown.contains(LEAK), "{variable}={value} : {shown}");
            assert!(!shown.contains("uinfo-name"), "{variable}={value} : {shown}");
        }
    }

    #[test]
    fn debug_of_config_redacts_secret() {
        let loaded = with_credentials(&[]);
        let shown = format!("{:?}", loaded.config);
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(shown.contains("REDACTED"), "{shown}");
        assert!(!format!("{loaded:?}").contains(SECRET));
    }

    // --- Messages -------------------------------------------------------------------------------

    #[test]
    fn notice_levels_split_problems_from_warnings() {
        let problems = [
            Notice::UnknownAuditMode,
            Notice::CredentialsMissing,
            Notice::AuditDir(AuditDirProblem::NotAbsolute),
            Notice::DilaDir(DilaDirProblem::NotAbsolute),
            Notice::Problem(ConfigProblem::invalid_env("x")),
        ];
        for notice in problems {
            assert!(notice.is_problem(), "{notice}");
        }
        let warnings = [
            Notice::Deprecated { variable: "A", replacement: "B" },
            Notice::Shadowed { variable: "A", replacement: "B" },
            Notice::Ignored { variable: "A" },
            Notice::InvalidSetting { variable: "A", expected: "x".into(), default: "y".into() },
            Notice::InvalidLogFilter,
            Notice::Warning(ConfigWarning::InsecureLoopback { setting: "base URL" }),
        ];
        for notice in warnings {
            assert!(!notice.is_problem(), "{notice}");
        }
    }

    #[test]
    fn problems_are_logged_as_errors_and_warnings_as_warnings() {
        let loaded = loaded(&[("FR_ELI_ENV", "x")]);
        let logs = logs_of(&loaded);
        assert!(logs.contains("ERROR"), "{logs}");
        assert!(logs.contains("PISTE credentials missing"), "{logs}");
        assert!(logs.contains("WARN"), "{logs}");
        assert!(logs.contains("FR_ELI_ENV ignored"), "{logs}");
    }

    // --- Environnement du processus -------------------------------------------------------------

    #[test]
    fn process_env_keeps_only_the_closed_list() {
        let env = ProcessEnv::capture();
        // PATH existe presque toujours dans l'environnement du processus : il n'est pas copié.
        assert_eq!(env.get("PATH"), None);
        assert!(!env.is_set("PATH"));
        assert_eq!(env.get("LF_NOT_A_VARIABLE"), None);
    }

    /// Marque du processus fils de `process_env_captures_the_closed_list_only`, posée dans une
    /// variable de la liste fermée : l'environnement réel d'un développeur ne la contient pas.
    const CHILD_MARKER: &str = "probe-child-marker";

    #[test]
    fn process_env_captures_the_closed_list_only() {
        // `set_var` est interdit (`forbid(unsafe_code)`) : la capture se prouve dans un processus
        // fils, qui rejoue la sonde ci-dessous avec un environnement choisi.
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args([
            "--exact",
            "config::tests::process_env_child_probe",
            "--nocapture",
            "--test-threads=1",
        ]);
        for name in READ_VARIABLES.iter().chain(&IGNORED_LEGACY) {
            child.env_remove(name);
        }
        child
            .env(CLIENT_ID, CHILD_MARKER)
            .env(CLIENT_SECRET, "  child-secret  ")
            .env("FR_ELI_ENV", "production")
            .env("FR_ELI_CACHE_DIR", "  ")
            .env("PISTE_CLIENT_ID", "legacy")
            .env("CHILD_PROBE_OTHER", "other");
        let output = child.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}{stderr}");
        assert!(stdout.contains("child probe ran"), "{stdout}{stderr}");
    }

    /// Sonde du fils : sans la marque dans `LF_PISTE_CLIENT_ID`, elle ne fait rien.
    #[test]
    #[expect(clippy::print_stdout, reason = "la sonde signale son exécution au test parent")]
    fn process_env_child_probe() {
        let env = ProcessEnv::capture();
        if env.get(CLIENT_ID).as_deref() != Some(CHILD_MARKER) {
            return;
        }
        assert_eq!(env.get(CLIENT_SECRET).as_deref(), Some("  child-secret  "));
        assert_eq!(env.get(OAUTH_URL), None);
        // Hors de la liste fermée : jamais copié.
        assert_eq!(env.get("CHILD_PROBE_OTHER"), None);
        // Variables héritées ignorées : seule leur présence (non blanche) est gardée.
        assert!(env.is_set("FR_ELI_ENV"));
        assert_eq!(env.get("FR_ELI_ENV"), None);
        assert!(env.is_set("PISTE_CLIENT_ID"));
        assert!(!env.is_set("FR_ELI_CACHE_DIR"), "une valeur blanche n'est pas définie");
        assert!(!env.is_set("FR_ELI_RUNTIME_URL"));
        // Chargé de bout en bout, comme le fera `main`.
        let loaded = load(&env);
        let credentials = loaded.config.piste.credentials().unwrap();
        assert_eq!(credentials.client_id(), CHILD_MARKER);
        assert_eq!(credentials.client_secret().expose_secret(), "child-secret");
        let ignored: Vec<_> = loaded
            .notices
            .iter()
            .filter_map(|notice| match notice {
                Notice::Ignored { variable } => Some(*variable),
                _ => None,
            })
            .collect();
        assert_eq!(ignored, ["FR_ELI_ENV", "PISTE_CLIENT_ID"]);
        assert_eq!(loaded.config.piste.env(), PisteEnvironment::Sandbox);
        println!("child probe ran");
    }

    #[test]
    fn process_env_reports_presence_without_keeping_ignored_values() {
        let env = ProcessEnv {
            values: BTreeMap::from([(CLIENT_SECRET, "hunter2".to_owned()), (LOG, "  ".to_owned())]),
            ignored_set: BTreeSet::from(["FR_ELI_ENV"]),
        };
        assert_eq!(env.get(CLIENT_SECRET).as_deref(), Some("hunter2"));
        assert!(env.is_set(CLIENT_SECRET));
        assert!(!env.is_set(LOG), "une valeur blanche n'est pas définie");
        assert!(env.is_set("FR_ELI_ENV"));
        assert_eq!(
            env.get("FR_ELI_ENV"),
            None,
            "la valeur d'une variable ignorée n'est pas gardée"
        );
        assert!(!env.is_set("FR_ELI_CACHE_DIR"));
        // `Debug` ne montre que des noms.
        let shown = format!("{env:?}");
        assert!(shown.contains("LF_PISTE_CLIENT_SECRET"), "{shown}");
        assert!(!shown.contains("hunter2"), "{shown}");
        // Les répertoires de plateforme viennent de `dirs`.
        assert_eq!(env.home_dir(), dirs::home_dir());
        assert_eq!(env.data_dir(), dirs::data_dir());
    }

    #[test]
    fn closed_list_covers_every_documented_variable() {
        let lf = [
            "LF_PISTE_ENV",
            "LF_PISTE_CLIENT_ID",
            "LF_PISTE_CLIENT_SECRET",
            "LF_PISTE_OAUTH_URL",
            "LF_PISTE_BASE_URL",
            "LF_AUDIT_DIR",
            "LF_AUDIT_MODE",
            "LF_LOG",
            "LF_HTTP_TIMEOUT_S",
            "LF_TOOL_DEADLINE_S",
            "LF_PISTE_RPS",
            "LF_PISTE_BURST",
            "LF_PISTE_CONCURRENCY",
            "LF_PISTE_MOJIBAKE_REPAIR",
            "LF_DILA_DIR",
        ];
        let legacy = [
            "FR_ELI_CLIENT_ID",
            "FR_ELI_CLIENT_SECRET",
            "FR_ELI_OAUTH_URL",
            "FR_ELI_BASE_URL",
            "FR_ELI_AUDIT_DIR",
        ];
        let mut read: Vec<_> = READ_VARIABLES.to_vec();
        read.sort_unstable();
        let mut expected: Vec<_> = lf.iter().chain(&legacy).copied().collect();
        expected.sort_unstable();
        assert_eq!(read, expected);
        // Aucune variable n'est à la fois lue et ignorée.
        assert!(IGNORED_LEGACY.iter().all(|name| !READ_VARIABLES.contains(name)));
        assert_eq!(IGNORED_LEGACY.len(), 5);
    }
}
