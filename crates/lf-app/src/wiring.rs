//! Câblage : assemble les adapters en un `LegalService`, puis le sert sur stdio (02 §8.4).
//!
//! La composition de production est
//! `LegalService<AnyCorpus, AppAudit, SystemClock, AnyDoctrine, AnyGuidance>` ([`AppService`]) :
//!
//! - [`AnyCorpus`] choisit la source des textes à l'exécution (02 §8.5), sans `Box<dyn>` : PISTE
//!   derrière le cache (défaut), l'index local DILA si `LF_DILA_DIR` le demande (feature `dila`),
//!   ou une source indisponible qui répond `[config_error]` à chaque opération ;
//! - `PisteCorpus::new` ne fait aucune E/S : le client HTTP et le jeton OAuth naissent au premier
//!   appel, et un problème de configuration ou des identifiants absents donnent le mode dégradé
//!   (`[config_error]` sur les outils PISTE, sans réseau) plutôt qu'un refus de démarrer (00 §1) ;
//! - `CachedCorpus` décore le port de PISTE seul, l'audit restant au-dessus, dans la façade
//!   (ADR-0008) ; l'index local n'a pas de cache ;
//! - [`AppAudit`] est `JsonlAuditLog`, ou un journal inutilisable qui échoue à chaque ligne quand
//!   `LF_AUDIT_DIR` est inutilisable : `audit_error` dès le premier appel en mode `strict`
//!   (02 §8.2), jamais un refus de démarrer ;
//! - le mode d'audit (`LF_AUDIT_MODE`, `strict` par défaut, Q9) et l'échéance d'un appel
//!   (`LF_TOOL_DEADLINE_S`) arrivent dans l'[`AuditPolicy`] de la façade par
//!   [`service_settings`] ;
//! - [`AnyDoctrine`] est la source de doctrine fiscale (jalon MB) : le portail ouvert du BOFiP,
//!   ou rien. `BofipDoctrine::new` ne fait aucune E/S (le client HTTP naît au premier appel), et
//!   la source ne dépend pas de PISTE : elle sert même sans identifiants. Coupée (`LF_BOFIP`,
//!   mode hors ligne), aucun client n'est construit et chaque outil du BOFiP répond
//!   `[config_error]`, sans réseau ;
//! - [`AnyGuidance`] est la source des fiches pratiques (jalon MS) : les archives du jour de
//!   Service-Public, ou rien, mêmes règles (`LF_SERVICE_PUBLIC`). `ServicePublicGuidance::new` ne
//!   fait aucune E/S : rien n'est téléchargé avant le premier appel d'un de ses outils.
//!
//! # Source locale DILA (M9)
//!
//! - **Aucun téléchargement** : le binaire ne contacte pas `echanges.dila.gouv.fr` (plan « fin du
//!   projet » : dumps téléchargés par script, hors binaire). `LF_DILA_DIR` désigne un index déjà
//!   bâti ; l'ouvrir lit ce répertoire, seule E/S de [`build`]. Un répertoire absent, ou un index
//!   sans aucun document, rend la source indisponible (`config_error`) : un index « sain » et vide
//!   déclarerait introuvable toute id demandée directement.
//! - **Jamais de repli sur PISTE** : une source locale demandée mais inutilisable (chemin refusé,
//!   feature absente, index illisible) ne sert aucun texte. Une requête ne doit pas partir sur le
//!   réseau quand l'utilisateur a choisi l'hors-ligne.
//! - **V-180** : l'index local ne porte que le fonds LEGI. Les contenus `dereferenced` et `inap`
//!   (jurisprudence, CNIL, accords, JORF) n'y entrent pas, et aucune variable ne les admet.
//! - **Environnement** (02 §8.5) : la source locale se déclare `production` dans l'audit et dans
//!   `fr_coverage` (le bac à sable de PISTE n'y est pour rien), sans identifiants exigés.
//! - **Instantané** : un index local peut être périmé, et sa déclaration `production` ne dit pas
//!   qu'il prouve une absence. Le drapeau `local_snapshot` de la configuration (posé à la lecture
//!   de `LF_DILA_DIR`, voir `config.rs`) le dit à `lf-application` : `fr_verify_citations` et
//!   `fr_resolve` rendent une absence `unresolvable`, jamais `not_found`, avec une note qui
//!   parle de l'instantané ; `fr_coverage` décrit la source comme un instantané des dumps LEGI
//!   (statut `degraded`, lacune FR-105, familles LODA et CODE seules), non comme du direct.
//!
//! [`run`] est la commande `serve` : stdin et stdout portent le JSON-RPC (stdout n'a pas d'autre
//! usage), les journaux vont sur stderr. Le code de sortie est 0 à l'EOF de stdin et à l'arrêt
//! demandé, et une erreur (code 1 dans `main`) si le service échoue (02 §8.8).

use std::{path::Path, process::ExitCode, sync::Arc};

use anyhow::Context as _;
use lf_adapter_audit::JsonlAuditLog;
use lf_adapter_bofip::BofipDoctrine;
use lf_adapter_cache::{CachePolicy, CachedCorpus};
#[cfg(feature = "dila")]
use lf_adapter_dila::DilaCorpus;
use lf_adapter_mcp::{LegifranceServer, serve_stdio};
use lf_adapter_piste::PisteCorpus;
use lf_adapter_service_public::ServicePublicGuidance;
use lf_application::{
    audited::AuditPolicy,
    budget::ResponseBudget,
    ports::{
        ArticleAt, AuditError, AuditErrorKind, AuditLog, AuditRecord, ConfigKind, CorpusError,
        DoctrineError, GuidanceError, GuidanceFound, GuidanceRead, LegalCorpus, PisteEnvironment,
        PublicGuidance, TaxDoctrine,
    },
    service::{LegalService, ServiceSettings},
};
use lf_domain::{
    date::VersionDate,
    doctrine::{BoiId, DoctrineDoc, DoctrinePage, DoctrineQuery},
    entity::{
        Act, ArticleText, ArticleVersion, CodeListing, CompanyAgreement, Convention, Decision,
        Deliberation, RelatedLinks, SearchHit, SearchPage,
    },
    guidance::{Audience, GuidanceQuery, SheetId},
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::{NumberQuery, SearchQuery},
};
use tokio_util::sync::CancellationToken;

use crate::{
    clock::SystemClock,
    config::{AppConfig, AuditSettings, BofipSource, CorpusSource, ServicePublicSource},
};

/// Le corpus de production : PISTE derrière le cache, ou l'index local DILA (02 §8.5).
pub(crate) type AppCorpus = AnyCorpus;

/// La façade de production : corpus, journal d'audit, horloge, source de doctrine et source des
/// fiches pratiques de [`build`].
pub(crate) type AppService =
    LegalService<AppCorpus, AppAudit, SystemClock, AnyDoctrine, AnyGuidance>;

/// Ce que le modèle lit quand l'index local DILA demandé ne peut pas être ouvert : ni chemin ni
/// cause, que le journal de diagnostic porte.
#[cfg(feature = "dila")]
const INDEX_UNREADABLE: &str =
    "the local DILA index cannot be opened: check LF_DILA_DIR and the server log";

/// Ce que le modèle lit quand l'index local DILA a été bâti par une autre version de l'analyseur
/// de textes : ni chemin ni numéro de version.
#[cfg(feature = "dila")]
const INDEX_STALE: &str = "the local DILA index was built by another version of this server and \
    must be rebuilt with mcp-legifrance build-dila-index (see the README, offline mode)";

/// Ce que le modèle lit quand `LF_DILA_DIR` ne désigne aucun répertoire, ou que l'index n'a aucun
/// document : une source vide ne prouve l'absence de rien.
#[cfg(feature = "dila")]
const INDEX_EMPTY: &str =
    "the local DILA index is missing or empty: build it from the LEGI dumps and check LF_DILA_DIR";

/// Source des textes, choisie une fois au démarrage (02 §8.5). Un enum de dispatch statique : ni
/// `Box<dyn>`, ni futur boxé (ADR-0004, ADR-0012). Aucune méthode du port n'a de défaut.
pub(crate) enum AnyCorpus {
    /// PISTE derrière le cache (le cache ne sert qu'à PISTE).
    Piste(CachedCorpus<PisteCorpus>),
    /// L'index local des dumps DILA (LEGI) : déjà local, donc sans cache.
    #[cfg(feature = "dila")]
    Dila(DilaCorpus),
    /// La source locale demandée ne peut pas servir : chaque opération rend ce texte, que le
    /// modèle lit (`[config_error]`), sans aucune E/S.
    Unavailable(String),
}

/// Erreur d'une source indisponible : une erreur de configuration, au texte déjà rédigé.
fn unavailable(detail: &str) -> CorpusError {
    CorpusError::Config { kind: ConfigKind::Settings, detail: detail.to_owned() }
}

/// Envoie l'opération à la variante du corpus. Une source indisponible répond sans rien appeler.
macro_rules! dispatch {
    ($corpus:expr, $method:ident($($argument:expr),*)) => {
        match $corpus {
            AnyCorpus::Piste(corpus) => corpus.$method($($argument),*).await,
            #[cfg(feature = "dila")]
            AnyCorpus::Dila(corpus) => corpus.$method($($argument),*).await,
            AnyCorpus::Unavailable(detail) => Err(unavailable(detail)),
        }
    };
}

impl LegalCorpus for AnyCorpus {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        dispatch!(self, search(query))
    }

    async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
        dispatch!(self, get_act(id, date))
    }

    async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
        dispatch!(self, get_article(id))
    }

    async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
        dispatch!(self, get_decision(id))
    }

    async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
        dispatch!(self, get_deliberation(id))
    }

    async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
        dispatch!(self, get_convention(id))
    }

    async fn get_company_agreement(
        &self,
        id: &AccoTextId,
    ) -> Result<CompanyAgreement, CorpusError> {
        dispatch!(self, get_company_agreement(id))
    }

    async fn find_article(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        dispatch!(self, find_article(text, num, at))
    }

    async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        dispatch!(self, find_by_number(query))
    }

    async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        dispatch!(self, list_codes())
    }

    async fn article_versions(&self, id: &ArticleId) -> Result<Vec<ArticleVersion>, CorpusError> {
        dispatch!(self, article_versions(id))
    }

    async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
        dispatch!(self, related_links(id))
    }
}

/// Le corpus que la configuration demande. PISTE n'est jamais le repli d'une source locale
/// inutilisable.
pub(crate) fn corpus_for(config: &AppConfig) -> AnyCorpus {
    match &config.source {
        CorpusSource::Piste => {
            let piste = PisteCorpus::new(config.piste.clone());
            AnyCorpus::Piste(CachedCorpus::new(piste, config.piste.env(), CachePolicy::default()))
        }
        CorpusSource::Dila(dir) => local_index(dir),
        CorpusSource::Unusable(problem) => AnyCorpus::Unavailable(problem.to_string()),
    }
}

/// Ouvre l'index local DILA de `dir` ; un échec laisse la source indisponible, de même qu'un
/// répertoire absent (l'ouvrir le créerait vide : une faute de frappe de `LF_DILA_DIR` donnerait
/// un index « sain » où toute id directe est introuvable) ou un index sans aucun document.
#[cfg(feature = "dila")]
fn local_index(dir: &Path) -> AnyCorpus {
    if !dir.is_dir() {
        tracing::error!("LF_DILA_DIR does not name an existing directory");
        return AnyCorpus::Unavailable(INDEX_EMPTY.to_owned());
    }
    match DilaCorpus::open(dir) {
        Ok(corpus) if corpus.engine().is_empty() => {
            tracing::error!("the local DILA index holds no document");
            AnyCorpus::Unavailable(INDEX_EMPTY.to_owned())
        }
        Ok(corpus) => AnyCorpus::Dila(corpus),
        Err(lf_adapter_dila::OpenError::Stale) => {
            tracing::error!(
                "the local DILA index must be rebuilt: it was built by another version of the \
                 text analyser"
            );
            AnyCorpus::Unavailable(INDEX_STALE.to_owned())
        }
        Err(error) => {
            // L'opérateur lit la cause dans stderr ; le modèle n'en voit ni cause ni chemin.
            tracing::error!(%error, "cannot open the local DILA index");
            AnyCorpus::Unavailable(INDEX_UNREADABLE.to_owned())
        }
    }
}

/// Sans la feature `dila`, il n'y a pas d'index local : la source demandée est indisponible.
#[cfg(not(feature = "dila"))]
fn local_index(_dir: &Path) -> AnyCorpus {
    AnyCorpus::Unavailable(crate::config::DilaDirProblem::NotCompiled.to_string())
}

/// Source de doctrine fiscale, choisie une fois au démarrage : le portail du BOFiP, ou rien. Un
/// enum de dispatch statique, comme [`AnyCorpus`].
#[expect(
    clippy::large_enum_variant,
    reason = "une seule valeur par processus, tenue par la façade : une boîte n'économiserait rien"
)]
pub(crate) enum AnyDoctrine {
    /// Le portail ouvert de la DGFiP (`data.economie.gouv.fr`, jeu `bofip-vigueur`).
    Bofip(BofipDoctrine<SystemClock>),
    /// Source coupée : chaque opération répond [`DoctrineError::Disabled`], que le modèle lit en
    /// `[config_error]`, sans client ni réseau.
    Off,
}

impl TaxDoctrine for AnyDoctrine {
    async fn search(&self, query: &DoctrineQuery) -> Result<DoctrinePage, DoctrineError> {
        match self {
            Self::Bofip(portal) => portal.search(query).await,
            Self::Off => Err(DoctrineError::Disabled),
        }
    }

    async fn get(&self, id: &BoiId) -> Result<Vec<DoctrineDoc>, DoctrineError> {
        match self {
            Self::Bofip(portal) => portal.get(id).await,
            Self::Off => Err(DoctrineError::Disabled),
        }
    }
}

/// La source de doctrine que la configuration demande. Coupée, aucun client n'est construit ;
/// active, la construction ne fait aucune E/S.
pub(crate) fn doctrine_for(config: &AppConfig) -> AnyDoctrine {
    match &config.bofip {
        BofipSource::On(settings) => {
            AnyDoctrine::Bofip(BofipDoctrine::new(settings.clone(), SystemClock))
        }
        BofipSource::Off(_) => AnyDoctrine::Off,
    }
}

/// Source des fiches pratiques, choisie une fois au démarrage : les archives du jour de
/// Service-Public, ou rien. Un enum de dispatch statique, comme [`AnyDoctrine`].
pub(crate) enum AnyGuidance {
    /// Les archives du jour de la DILA (`lecomarquage.service-public.gouv.fr`), gardées en
    /// mémoire.
    ServicePublic(ServicePublicGuidance<SystemClock>),
    /// Source coupée : chaque opération répond [`GuidanceError::Disabled`], que le modèle lit en
    /// `[config_error]`, sans client ni réseau.
    Off,
}

impl PublicGuidance for AnyGuidance {
    async fn search(&self, query: &GuidanceQuery) -> Result<GuidanceFound, GuidanceError> {
        match self {
            Self::ServicePublic(archives) => archives.search(query).await,
            Self::Off => Err(GuidanceError::Disabled),
        }
    }

    async fn get(&self, audience: Audience, id: &SheetId) -> Result<GuidanceRead, GuidanceError> {
        match self {
            Self::ServicePublic(archives) => archives.get(audience, id).await,
            Self::Off => Err(GuidanceError::Disabled),
        }
    }
}

/// La source des fiches que la configuration demande. Coupée, aucun client n'est construit ;
/// active, la construction ne fait aucune E/S.
pub(crate) fn guidance_for(config: &AppConfig) -> AnyGuidance {
    match &config.service_public {
        ServicePublicSource::On(settings) => {
            AnyGuidance::ServicePublic(ServicePublicGuidance::new(settings, SystemClock))
        }
        ServicePublicSource::Off(_) => AnyGuidance::Off,
    }
}

/// Journal d'audit du processus : le fichier JSONL, ou l'impossibilité d'en avoir un.
///
/// Une configuration d'audit inutilisable (`LF_AUDIT_DIR` relatif, répertoire personnel ou de
/// données introuvable) ne doit pas empêcher le démarrage, mais faire échouer chaque appel en mode
/// `strict` : [`AppAudit::Unusable`] rend la même erreur que l'écriture d'un fichier impossible.
#[derive(Debug)]
pub(crate) enum AppAudit {
    /// Journal JSONL du répertoire de `LF_AUDIT_DIR` (créé au premier appel).
    File(JsonlAuditLog),
    /// Aucun journal possible : chaque ligne échoue avec ce genre et ce détail.
    Unusable {
        /// Genre de l'échec, tel que `Audited` le nomme.
        kind: AuditErrorKind,
        /// Raison, sans secret ni valeur lue : elle ne va que dans le journal de diagnostic.
        detail: String,
    },
}

impl AppAudit {
    /// Ouvre le journal des réglages d'audit, sans E/S (`JsonlAuditLog::open` n'en fait pas).
    pub(crate) fn open(settings: &AuditSettings) -> Self {
        let unusable = |kind, detail: String| Self::Unusable { kind, detail };
        match &settings.dir {
            Ok(dir) => JsonlAuditLog::open(dir, JsonlAuditLog::DEFAULT_ROTATE_BYTES)
                .map_or_else(|error| unusable(error.kind, error.detail), Self::File),
            Err(problem) => unusable(AuditErrorKind::Io, problem.to_string()),
        }
    }
}

impl AuditLog for AppAudit {
    async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        match self {
            Self::File(log) => log.record(record).await,
            Self::Unusable { kind, detail } => {
                // Même journal de diagnostic que `JsonlAuditLog` : le détail n'en sort pas.
                tracing::error!(kind = ?kind, detail = %detail, "audit write failed");
                Err(AuditError { kind: *kind, detail: detail.clone() })
            }
        }
    }
}

/// Réglages de la façade d'après la configuration (D13) : la politique d'audit porte le mode
/// (`strict` par défaut, Q9), l'échéance d'un appel, l'environnement PISTE et la version du
/// serveur ; l'état de la configuration alimente `fr_coverage`.
pub(crate) fn service_settings(config: &AppConfig) -> ServiceSettings {
    ServiceSettings {
        policy: AuditPolicy {
            mode: config.audit.mode,
            deadline: config.tool_deadline,
            env: audit_environment(config),
            server_version: env!("CARGO_PKG_VERSION"),
        },
        configuration: config.configuration.clone(),
        budget: ResponseBudget,
    }
}

/// Environnement déclaré dans l'audit et dans `fr_coverage` (02 §8.5, second point à trancher) :
/// celui de PISTE pour PISTE, `production` pour l'index local, dont les textes sont les textes
/// publiés (le bac à sable de PISTE ne s'y applique pas, et `PisteEnvironment` n'a pas de
/// troisième valeur).
///
/// `production` ne dit pas que l'index prouve une absence : le drapeau `local_snapshot` de la
/// configuration, lu par `lf-application`, l'en empêche (voir « Source locale DILA » en tête de ce
/// fichier).
fn audit_environment(config: &AppConfig) -> PisteEnvironment {
    match config.source {
        CorpusSource::Dila(_) => PisteEnvironment::Production,
        CorpusSource::Piste | CorpusSource::Unusable(_) => config.piste.env(),
    }
}

/// Assemble la façade de production. Aucune E/S réseau ni du journal d'audit ; l'index local DILA,
/// s'il est demandé, ouvre son répertoire, qui doit exister et porter des documents.
pub(crate) fn build(config: &AppConfig) -> AppService {
    tracing::info!(source = config.source.label(), "corpus source");
    tracing::info!(source = config.bofip.label(), "tax doctrine source");
    tracing::info!(source = config.service_public.label(), "public guidance source");
    LegalService::new(
        corpus_for(config),
        AppAudit::open(&config.audit),
        SystemClock,
        service_settings(config),
    )
    .with_doctrine(doctrine_for(config))
    .with_guidance(guidance_for(config))
}

/// Commande `serve` : sert le serveur MCP sur stdin et stdout jusqu'à l'EOF de stdin ou jusqu'à
/// l'annulation de `shutdown` (signal du processus).
///
/// # Errors
///
/// L'échec du service MCP (initialisation impossible hors EOF et arrêt, ou tâche de service
/// interrompue) : `main` le journalise et sort avec le code 1.
pub(crate) async fn run(
    config: AppConfig,
    shutdown: CancellationToken,
) -> anyhow::Result<ExitCode> {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        env = %audit_environment(&config),
        "starting"
    );
    let server = LegifranceServer::new(Arc::new(build(&config)));
    let outcome = serve_stdio(server, shutdown).await.context("the MCP server failed")?;
    tracing::info!(?outcome, "stopped");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use std::{future::pending, num::NonZeroU32, path::Path};

    use lf_application::{
        audited::AuditMode,
        error::ErrorCode,
        input::{
            CanonicalInput, CoverageInput, GetBofipInput, GetServicePublicInput, SearchBofipInput,
            SearchServicePublicInput, ToolCall,
        },
        messages::{DOCTRINE_DISABLED_MESSAGE, GUIDANCE_DISABLED_MESSAGE},
        output::coverage::CoverageOutput,
        ports::AuditStatus,
        service::{DoctrineSource, GuidanceSource},
        tools::ToolName,
    };
    use lf_domain::{
        doctrine::{DoctrineTerms, KindFilter},
        fond::Fond,
        guidance::{AudienceFilter, GuidanceTerms},
        search::{MatchMode, NumberKind, NumberValue, PageSize, QueryText},
    };

    use super::*;
    use crate::config::{load, test_env::MapEnv};

    const ID: &str = "11112222-3333-4444-5555-666677778888";
    const SECRET: &str = "99990000-aaaa-bbbb-cccc-ddddeeeeffff";

    /// Configuration chargée depuis une table, identifiants fournis sauf mention contraire.
    fn config_with(pairs: &[(&str, &str)]) -> AppConfig {
        let mut all = vec![("LF_PISTE_CLIENT_ID", ID), ("LF_PISTE_CLIENT_SECRET", SECRET)];
        all.extend_from_slice(pairs);
        load(&MapEnv::new(&all)).config
    }

    fn audit_dir(dir: &Path) -> (&'static str, &str) {
        ("LF_AUDIT_DIR", dir.to_str().expect("a UTF-8 temporary path"))
    }

    fn coverage_call() -> ToolCall<CoverageInput> {
        ToolCall { input: Ok(CoverageInput), canonical: CanonicalInput::new("{}") }
    }

    fn record() -> AuditRecord {
        AuditRecord {
            ts: chrono::DateTime::UNIX_EPOCH,
            tool: ToolName::FR_COVERAGE,
            input: CanonicalInput::new("{}"),
            output_count_or_size: 0,
            duration_ms: 1,
            status: AuditStatus::Ok,
            error_code: None,
            error: None,
            env: PisteEnvironment::Sandbox,
            server_version: "0.0.0",
        }
    }

    /// Q9 : sans `LF_AUDIT_MODE`, le mode `strict` posé par `lf-app` arrive dans la politique de la
    /// façade, et un échec d'audit retient le résultat. `best-effort` en option le rend.
    #[tokio::test]
    async fn audit_mode_strict_reaches_policy() {
        // Le réglage de la configuration arrive tel quel dans la politique.
        let defaulted = config_with(&[]);
        assert_eq!(defaulted.audit.mode, AuditMode::Strict);
        assert_eq!(service_settings(&defaulted).policy.mode, AuditMode::Strict);
        let unknown = config_with(&[("LF_AUDIT_MODE", "sometimes")]);
        assert_eq!(service_settings(&unknown).policy.mode, AuditMode::Strict);
        let relaxed = config_with(&[("LF_AUDIT_MODE", "best-effort")]);
        assert_eq!(service_settings(&relaxed).policy.mode, AuditMode::BestEffort);

        // Et la façade assemblée s'y tient : un répertoire d'audit inutilisable (relatif) fait
        // échouer l'appel en `strict`, pas en `best-effort`.
        let strict = build(&config_with(&[("LF_AUDIT_DIR", "relative/audit")]));
        let error = strict.coverage(coverage_call(), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::AuditError), "{error}");
        let best_effort = build(&config_with(&[
            ("LF_AUDIT_DIR", "relative/audit"),
            ("LF_AUDIT_MODE", "best-effort"),
        ]));
        best_effort.coverage(coverage_call(), pending()).await.expect("the result is returned");
    }

    #[test]
    fn settings_carry_environment_deadline_version_and_configuration() {
        let config = config_with(&[("LF_PISTE_ENV", "production"), ("LF_TOOL_DEADLINE_S", "120")]);
        let settings = service_settings(&config);
        assert_eq!(settings.policy.env, PisteEnvironment::Production);
        assert_eq!(settings.policy.deadline, std::time::Duration::from_secs(120));
        assert_eq!(settings.policy.server_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(settings.configuration, config.configuration);
        assert!(settings.configuration.credentials_configured);
        assert_eq!(settings.budget, ResponseBudget);

        // Sans identifiants : le mode dégradé arrive dans l'état de la configuration.
        let degraded = load(&MapEnv::new(&[])).config;
        let settings = service_settings(&degraded);
        assert!(!settings.configuration.credentials_configured);
        assert_eq!(settings.policy.env, PisteEnvironment::Sandbox);
    }

    #[tokio::test]
    async fn a_usable_audit_dir_receives_one_line_per_call() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let service = build(&config_with(&[audit_dir(&dir.path().join("audit"))]));
        // Aucune E/S avant le premier appel.
        assert!(!dir.path().join("audit").exists());

        service.coverage(coverage_call(), pending()).await.expect("fr_coverage answers");

        let journal = dir.path().join("audit").join("mcp-legifrance.jsonl");
        let text = std::fs::read_to_string(journal).expect("the journal exists");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1, "{text}");
        let line: serde_json::Value = serde_json::from_str(lines[0]).expect("a JSON line");
        assert_eq!(line["tool"], "fr_coverage");
        assert_eq!(line["status"], "ok");
        assert_eq!(line["env"], "sandbox");
        assert_eq!(line["server_version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn an_unusable_audit_setting_fails_each_line_with_its_kind() {
        let audit = AppAudit::open(&AuditSettings {
            mode: AuditMode::Strict,
            dir: Err(crate::config::AuditDirProblem::NotAbsolute),
        });
        assert!(matches!(&audit, AppAudit::Unusable { kind: AuditErrorKind::Io, .. }), "{audit:?}");
        for _ in 0..2 {
            let error = audit.record(&record()).await.unwrap_err();
            assert_eq!(error.kind, AuditErrorKind::Io);
            assert_eq!(error.detail, "LF_AUDIT_DIR must be an absolute path");
            // Le message vu par le modèle ne cite que le genre.
            assert_eq!(error.to_string(), "audit log unavailable (Io)");
        }
    }

    #[test]
    fn an_unopenable_journal_is_unusable_not_a_startup_failure() {
        let audit = AppAudit::open(&AuditSettings {
            mode: AuditMode::Strict,
            dir: Ok(std::path::PathBuf::new()),
        });
        assert!(matches!(&audit, AppAudit::Unusable { .. }), "{audit:?}");
        if let AppAudit::Unusable { kind, detail } = audit {
            assert_eq!(kind, AuditErrorKind::Io);
            assert_eq!(detail, "the audit directory is empty");
        }
    }

    // --- Sélection de la source (M9, 02 §8.5) ---------------------------------------------------

    /// Les douze opérations du port avec une entrée valide : le résultat de chacune, sans sa valeur.
    async fn every_operation(corpus: &AnyCorpus) -> Vec<(&'static str, Result<(), CorpusError>)> {
        let at = VersionDate::parse("2026-10-15").unwrap();
        let text = LegiTextId::parse("LEGITEXT000006070721").unwrap();
        let article = ArticleId::parse("LEGIARTI000006419288").unwrap();
        let number = NumberQuery::new(
            Fond::Juri,
            NumberKind::Case,
            NumberValue::parse("24-13.470").unwrap(),
            at,
        )
        .unwrap();
        vec![
            ("search", corpus.search(&search(Fond::Loda)).await.map(drop)),
            ("get_act", corpus.get_act(&text, at).await.map(drop)),
            ("get_article", corpus.get_article(&article).await.map(drop)),
            (
                "get_decision",
                corpus
                    .get_decision(&DecisionId::parse("JURITEXT000007042010").unwrap())
                    .await
                    .map(drop),
            ),
            (
                "get_deliberation",
                corpus
                    .get_deliberation(&CnilTextId::parse("CNILTEXT000017653838").unwrap())
                    .await
                    .map(drop),
            ),
            (
                "get_convention",
                corpus
                    .get_convention(&KaliTextId::parse("KALITEXT000005635534").unwrap())
                    .await
                    .map(drop),
            ),
            (
                "get_company_agreement",
                corpus
                    .get_company_agreement(&AccoTextId::parse("ACCOTEXT000042000000").unwrap())
                    .await
                    .map(drop),
            ),
            (
                "find_article",
                corpus.find_article(&text, "L36-11", ArticleAt::On(at)).await.map(drop),
            ),
            ("find_by_number", corpus.find_by_number(&number).await.map(drop)),
            ("list_codes", corpus.list_codes().await.map(drop)),
            ("article_versions", corpus.article_versions(&article).await.map(drop)),
            ("related_links", corpus.related_links(&article).await.map(drop)),
        ]
    }

    /// Une recherche plein texte sur `fond`, à la date du jour des tests.
    fn search(fond: Fond) -> SearchQuery {
        SearchQuery::new(
            fond,
            QueryText::parse("responsabilité").unwrap(),
            NonZeroU32::MIN,
            PageSize::new(10).unwrap(),
            MatchMode::Any,
            None,
            fond.needs_version_date().then(|| VersionDate::parse("2026-10-15").unwrap()),
        )
        .unwrap()
    }

    /// Le détail d'une erreur de configuration que le modèle lira ; toute autre erreur donne un
    /// texte qui la nomme, pour que l'assertion qui suit échoue en la montrant.
    fn settings_detail(error: &CorpusError) -> String {
        match error {
            CorpusError::Config { kind: ConfigKind::Settings, detail } => detail.clone(),
            other => format!("not a settings error: {other:?}"),
        }
    }

    // --- Source de doctrine fiscale (jalon MB) --------------------------------------------------

    const BOI: &str = "BOI-ENR-DMTG-10-50-20";

    fn doctrine_query() -> DoctrineQuery {
        DoctrineQuery::new(
            DoctrineTerms::parse("donation").unwrap(),
            None,
            KindFilter::Doctrine,
            NonZeroU32::MIN,
            PageSize::new(10).unwrap(),
        )
        .unwrap()
    }

    /// Les trois façons de couper la source : la demande, une valeur invalide, le mode hors ligne
    /// (ici avec un `LF_DILA_DIR` inutilisable).
    const BOFIP_OFF: [(&str, &str); 3] =
        [("LF_BOFIP", "off"), ("LF_BOFIP", "false"), ("LF_DILA_DIR", "relative/index")];

    /// Par défaut, la source est le portail : la construire ne fait aucune E/S, et elle ne
    /// dépend pas des identifiants PISTE.
    #[test]
    fn the_bofip_portal_is_the_doctrine_source_by_default() {
        assert!(matches!(doctrine_for(&config_with(&[])), AnyDoctrine::Bofip(_)));
        let bare = load(&MapEnv::new(&[])).config;
        assert!(matches!(doctrine_for(&bare), AnyDoctrine::Bofip(_)));
        assert_eq!(service_settings(&bare).configuration.doctrine, DoctrineSource::Active);
        // `LF_BOFIP=on` rétablit la source en mode hors ligne.
        let forced = config_with(&[("LF_DILA_DIR", "relative/index"), ("LF_BOFIP", "on")]);
        assert!(matches!(doctrine_for(&forced), AnyDoctrine::Bofip(_)));
    }

    /// Source coupée : les deux opérations répondent `Disabled`, sans client ni réseau.
    #[tokio::test]
    async fn a_bofip_source_turned_off_answers_disabled_on_both_operations() {
        for pair in BOFIP_OFF {
            let config = config_with(&[pair]);
            assert_eq!(service_settings(&config).configuration.doctrine, DoctrineSource::Off);
            let doctrine = doctrine_for(&config);
            assert!(matches!(doctrine, AnyDoctrine::Off), "{pair:?}");
            assert_eq!(doctrine.search(&doctrine_query()).await, Err(DoctrineError::Disabled));
            assert_eq!(
                doctrine.get(&BoiId::parse(BOI).unwrap()).await,
                Err(DoctrineError::Disabled)
            );
        }
    }

    /// La façade assemblée : source coupée, les deux outils du BOFiP répondent `[config_error]`
    /// avec le texte fixe, audité, et `fr_coverage` rend la lacune FR-107 ; par défaut, FR-106.
    #[tokio::test]
    async fn the_built_service_serves_the_bofip_tools_and_reports_the_source_state() {
        let search = || ToolCall {
            input: Ok(SearchBofipInput {
                query: Some("donation".to_owned()),
                ..SearchBofipInput::default()
            }),
            canonical: CanonicalInput::new(r#"{"query":"donation"}"#),
        };
        let get = || ToolCall {
            input: Ok(GetBofipInput { id: Some(BOI.to_owned()), ..GetBofipInput::default() }),
            canonical: CanonicalInput::new(r#"{"id":"BOI-ENR-DMTG-10-50-20"}"#),
        };
        for pair in BOFIP_OFF {
            let dir = tempfile::tempdir().expect("a temporary directory");
            let service = build(&config_with(&[audit_dir(dir.path()), pair]));
            let error = service.search_bofip(search(), pending()).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::ConfigError), "{pair:?}");
            assert_eq!(error.message(), DOCTRINE_DISABLED_MESSAGE, "{pair:?}");
            let error = service.get_bofip(get(), pending()).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::ConfigError), "{pair:?}");
            assert_eq!(error.message(), DOCTRINE_DISABLED_MESSAGE, "{pair:?}");
            let coverage = service.coverage(coverage_call(), pending()).await.unwrap();
            assert_eq!(gaps_of(&coverage, "BOFIP"), ["FR-107"], "{pair:?}");
        }
        let dir = tempfile::tempdir().expect("a temporary directory");
        let service = build(&config_with(&[audit_dir(dir.path())]));
        let coverage = service.coverage(coverage_call(), pending()).await.unwrap();
        assert_eq!(gaps_of(&coverage, "BOFIP"), ["FR-106"]);
    }

    /// Les lacunes de `fr_coverage` du fonds `fond`, dans l'ordre.
    fn gaps_of(output: &CoverageOutput, fond: &str) -> Vec<String> {
        output
            .coverage
            .known_gaps()
            .iter()
            .filter(|gap| gap.family == fond)
            .map(|gap| gap.id.clone())
            .collect()
    }

    /// Les trois façons de couper la source des fiches : la demande, une valeur invalide, le mode
    /// hors ligne (ici avec un `LF_DILA_DIR` inutilisable).
    const SERVICE_PUBLIC_OFF: [(&str, &str); 3] = [
        ("LF_SERVICE_PUBLIC", "off"),
        ("LF_SERVICE_PUBLIC", "false"),
        ("LF_DILA_DIR", "relative/index"),
    ];

    fn guidance_query() -> GuidanceQuery {
        GuidanceQuery {
            terms: GuidanceTerms::parse("logement").unwrap(),
            audience: AudienceFilter::Particuliers,
            page: NonZeroU32::MIN,
            page_size: PageSize::new(10).unwrap(),
        }
    }

    /// Par défaut, la source des fiches est Service-Public : la construire ne fait aucune E/S, et
    /// elle ne dépend pas des identifiants PISTE.
    #[test]
    fn service_public_is_the_guidance_source_by_default() {
        assert!(matches!(guidance_for(&config_with(&[])), AnyGuidance::ServicePublic(_)));
        let bare = load(&MapEnv::new(&[])).config;
        assert!(matches!(guidance_for(&bare), AnyGuidance::ServicePublic(_)));
        assert_eq!(service_settings(&bare).configuration.guidance, GuidanceSource::Active);
        // `LF_SERVICE_PUBLIC=on` rétablit la source en mode hors ligne.
        let forced = config_with(&[("LF_DILA_DIR", "relative/index"), ("LF_SERVICE_PUBLIC", "on")]);
        assert!(matches!(guidance_for(&forced), AnyGuidance::ServicePublic(_)));
    }

    /// Source coupée : les deux opérations répondent `Disabled`, sans client ni réseau.
    #[tokio::test]
    async fn a_service_public_source_turned_off_answers_disabled_on_both_operations() {
        for pair in SERVICE_PUBLIC_OFF {
            let config = config_with(&[pair]);
            assert_eq!(service_settings(&config).configuration.guidance, GuidanceSource::Off);
            let guidance = guidance_for(&config);
            assert!(matches!(guidance, AnyGuidance::Off), "{pair:?}");
            let found = guidance.search(&guidance_query()).await.map(|found| found.files);
            assert_eq!(found, Err(GuidanceError::Disabled), "{pair:?}");
            let id = SheetId::parse("F1437").unwrap();
            let read = guidance.get(Audience::Particuliers, &id).await.map(|read| read.file);
            assert_eq!(read, Err(GuidanceError::Disabled), "{pair:?}");
        }
    }

    /// La façade assemblée : source coupée, les deux outils de Service-Public répondent
    /// `[config_error]` avec le texte fixe, et `fr_coverage` rend la lacune FR-109 ; par défaut,
    /// FR-108.
    #[tokio::test]
    async fn the_built_service_serves_the_service_public_tools_and_reports_the_source_state() {
        let search = || ToolCall {
            input: Ok(SearchServicePublicInput {
                query: Some("logement".to_owned()),
                ..SearchServicePublicInput::default()
            }),
            canonical: CanonicalInput::new(r#"{"query":"logement"}"#),
        };
        let get = || ToolCall {
            input: Ok(GetServicePublicInput {
                id: Some("F1437".to_owned()),
                ..GetServicePublicInput::default()
            }),
            canonical: CanonicalInput::new(r#"{"id":"F1437"}"#),
        };
        for pair in SERVICE_PUBLIC_OFF {
            let dir = tempfile::tempdir().expect("a temporary directory");
            let service = build(&config_with(&[audit_dir(dir.path()), pair]));
            let error = service.search_service_public(search(), pending()).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::ConfigError), "{pair:?}");
            assert_eq!(error.message(), GUIDANCE_DISABLED_MESSAGE, "{pair:?}");
            let error = service.get_service_public(get(), pending()).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::ConfigError), "{pair:?}");
            assert_eq!(error.message(), GUIDANCE_DISABLED_MESSAGE, "{pair:?}");
            let coverage = service.coverage(coverage_call(), pending()).await.unwrap();
            assert_eq!(gaps_of(&coverage, "SERVICE_PUBLIC"), ["FR-109"], "{pair:?}");
        }
        let dir = tempfile::tempdir().expect("a temporary directory");
        let service = build(&config_with(&[audit_dir(dir.path())]));
        let coverage = service.coverage(coverage_call(), pending()).await.unwrap();
        assert_eq!(gaps_of(&coverage, "SERVICE_PUBLIC"), ["FR-108"]);
    }

    #[test]
    fn piste_stays_the_source_unless_a_local_index_is_asked_for() {
        let corpus = corpus_for(&config_with(&[]));
        assert!(matches!(corpus, AnyCorpus::Piste(_)));
        // Blanc : comme absent.
        let blank = corpus_for(&config_with(&[("LF_DILA_DIR", "  ")]));
        assert!(matches!(blank, AnyCorpus::Piste(_)));
        // Environnement de la politique : celui de PISTE.
        let settings = service_settings(&config_with(&[("LF_PISTE_ENV", "production")]));
        assert_eq!(settings.policy.env, PisteEnvironment::Production);
    }

    #[tokio::test]
    async fn an_unusable_source_answers_a_config_error_on_every_operation_without_piste() {
        // Des identifiants PISTE valides sont là : la source demandée est locale et inutilisable,
        // aucune requête ne doit partir vers PISTE.
        let config = config_with(&[("LF_DILA_DIR", "relative/index")]);
        let corpus = corpus_for(&config);
        assert!(matches!(corpus, AnyCorpus::Unavailable(_)));
        let results = every_operation(&corpus).await;
        assert_eq!(results.len(), 12);
        for (operation, result) in results {
            let error = result.unwrap_err();
            assert_eq!(
                settings_detail(&error),
                "LF_DILA_DIR must be an absolute path",
                "{operation}"
            );
        }
    }

    #[tokio::test]
    async fn an_unusable_source_degrades_the_coverage_and_names_the_problem() {
        let audit = tempfile::tempdir().expect("a temporary directory");
        let service =
            build(&config_with(&[audit_dir(audit.path()), ("LF_DILA_DIR", "relative/index")]));
        let output = service.coverage(coverage_call(), pending()).await.expect("fr_coverage");
        assert_eq!(output.coverage.status(), lf_domain::coverage::CoverageStatus::Degraded);
        assert_eq!(output.configuration.problems, ["LF_DILA_DIR must be an absolute path"]);
    }

    #[cfg(not(feature = "dila"))]
    #[tokio::test]
    async fn without_the_dila_feature_a_local_index_request_is_unavailable() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let config = config_with(&[("LF_DILA_DIR", dir.path().to_str().unwrap())]);
        let corpus = corpus_for(&config);
        assert!(matches!(corpus, AnyCorpus::Unavailable(_)));
        let error = corpus.list_codes().await.unwrap_err();
        assert!(settings_detail(&error).contains("dila feature"), "{error}");
    }

    /// Bâtit un vrai index local dans `index` : la mini-archive du global de V1-16 (fixtures de
    /// `lf-adapter-dila`), sous un nom d'archive de la DILA.
    #[cfg(feature = "dila")]
    async fn filled_index(index: &Path) {
        let archives = tempfile::tempdir().expect("a temporary directory");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../lf-adapter-dila/tests/fixtures/archives/legi-global-mini.tar.gz");
        std::fs::copy(fixture, archives.path().join("Freemium_legi_global_20250713-140000.tar.gz"))
            .expect("the fixture archive is copied");
        let work = tempfile::tempdir().expect("a temporary directory");
        let corpus = lf_adapter_dila::DilaCorpus::open(index).expect("an index is created");
        corpus
            .sync(
                archives.path().to_path_buf(),
                work.path().join("registry.toml"),
                lf_adapter_dila::IngestLimits::default(),
            )
            .await
            .expect("the archive is applied");
        assert!(!corpus.engine().is_empty());
    }

    #[cfg(feature = "dila")]
    #[tokio::test]
    async fn dila_feature_selects_dila_corpus() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let index = dir.path().join("index");
        filled_index(&index).await;
        let config = config_with(&[("LF_DILA_DIR", index.to_str().unwrap())]);
        let corpus = corpus_for(&config);
        assert!(matches!(corpus, AnyCorpus::Dila(_)), "the local index is the corpus");
        // L'index est lu : `list_codes` répond depuis lui (le petit code des architectes de la
        // mini-archive), sans réseau ni erreur.
        let codes = corpus.list_codes().await.unwrap();
        assert!(codes.iter().any(|code| code.id.as_str() == "LEGITEXT000006074232"), "{codes:?}");
        // La source locale n'est pas l'environnement PISTE : ni bac à sable, ni identifiants exigés.
        assert_eq!(service_settings(&config).policy.env, PisteEnvironment::Production);
        let audit = tempfile::tempdir().expect("a temporary directory");
        let bare = load(&MapEnv::new(&[
            audit_dir(audit.path()),
            ("LF_DILA_DIR", index.to_str().unwrap()),
        ]))
        .config;
        assert!(service_settings(&bare).configuration.local_snapshot);
        let service = build(&bare);
        let output = service.coverage(coverage_call(), pending()).await.expect("fr_coverage");
        // Ni identifiants ni problème, mais un instantané n'est pas du direct : `degraded`, avec
        // la lacune FR-105, les seules familles LEGI et une note de date qui ne dit pas « live ».
        assert_eq!(output.coverage.status(), lf_domain::coverage::CoverageStatus::Degraded);
        assert_eq!(output.environment, PisteEnvironment::Production);
        assert_eq!(output.configuration.problems, Vec::<String>::new());
        assert!(output.configuration.local_snapshot);
        // La lacune de l'instantané, puis celles du BOFiP et de Service-Public, coupés d'office en
        // mode hors ligne.
        let gaps: Vec<&str> =
            output.coverage.known_gaps().iter().map(|gap| gap.id.as_str()).collect();
        assert_eq!(gaps[gaps.len() - 3..], ["FR-105", "FR-107", "FR-109"]);
        let families: Vec<&str> =
            output.coverage.families().iter().map(|family| family.name.as_str()).collect();
        assert_eq!(families, ["LODA", "CODE"]);
        assert!(!output.coverage.as_of_note().contains("queried live against the source"));
    }

    /// Un index absent ou sans document ne sert rien : ouvert tel quel, il répondrait « introuvable »
    /// à toute id demandée directement, comme une preuve d'absence.
    #[cfg(feature = "dila")]
    #[tokio::test]
    async fn a_missing_or_empty_index_is_unavailable_and_nothing_is_created() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let missing = dir.path().join("typo-in-the-path");
        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        for index in [&missing, &empty] {
            let config = config_with(&[("LF_DILA_DIR", index.to_str().unwrap())]);
            let corpus = corpus_for(&config);
            assert!(matches!(corpus, AnyCorpus::Unavailable(_)), "{index:?}");
            for (operation, result) in every_operation(&corpus).await {
                let detail = settings_detail(&result.unwrap_err());
                assert!(detail.contains("missing or empty"), "{operation}: {detail}");
                assert!(!detail.contains("typo-in-the-path"), "{operation}: {detail}");
            }
        }
        // Un répertoire absent n'est pas créé par la lecture.
        assert!(!missing.exists());
    }

    /// Un index d'une autre version de l'analyseur se lit mal : la source est indisponible, le
    /// modèle lit de reconstruire l'index (texte fixe, sans chemin), et rien ne repasse par PISTE.
    #[cfg(feature = "dila")]
    #[tokio::test]
    async fn a_stale_index_is_unavailable_and_asks_for_a_rebuild() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let index = dir.path().join("stale-index");
        filled_index(&index).await;
        let meta = index.join("meta.json");
        let json = std::fs::read_to_string(&meta).expect("the index descriptor is read");
        let stamp = lf_adapter_dila::index::INDEX_STAMP;
        assert!(json.contains(stamp), "the build stamped the index");
        std::fs::write(&meta, json.replace(stamp, "x")).expect("the stamp is changed");
        let config = config_with(&[("LF_DILA_DIR", index.to_str().unwrap())]);
        let corpus = corpus_for(&config);
        assert!(matches!(corpus, AnyCorpus::Unavailable(_)));
        for (operation, result) in every_operation(&corpus).await {
            let detail = settings_detail(&result.unwrap_err());
            assert_eq!(detail, INDEX_STALE, "{operation}");
            assert!(detail.contains("build-dila-index"), "{operation}: {detail}");
            assert!(!detail.contains("stale-index"), "{operation}: {detail}");
        }
        assert!(
            INDEX_STALE.is_ascii() && !INDEX_STALE.chars().any(|c| c.is_ascii_digit()),
            "English, with no path and no version number"
        );
    }

    #[cfg(feature = "dila")]
    #[tokio::test]
    async fn an_index_that_cannot_be_opened_is_unavailable_and_hides_its_path() {
        // Le répertoire existe, mais le descripteur de l'index est corrompu.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let broken = dir.path().join("broken-index");
        std::fs::create_dir(&broken).unwrap();
        std::fs::write(broken.join("meta.json"), b"not an index").unwrap();
        let config = config_with(&[("LF_DILA_DIR", broken.to_str().unwrap())]);
        let corpus = corpus_for(&config);
        assert!(matches!(corpus, AnyCorpus::Unavailable(_)));
        for (operation, result) in every_operation(&corpus).await {
            let error = result.unwrap_err();
            let detail = settings_detail(&error);
            assert!(detail.contains("local DILA index cannot be opened"), "{operation}: {detail}");
            assert!(!detail.contains("broken-index"), "{operation}: {detail}");
        }
    }

    /// V-180 : les contenus `dereferenced` et `inap` (décisions, délibérations, accords, JORF) ne
    /// sont jamais dans l'index local, qui ne porte que le fonds LEGI. Les opérations des autres
    /// fonds rendent une erreur de configuration, jamais un contenu, et rien ne repasse par PISTE.
    #[cfg(feature = "dila")]
    #[tokio::test]
    async fn dereferenced_and_inap_excluded() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let index = dir.path().join("index");
        filled_index(&index).await;
        let config = config_with(&[("LF_DILA_DIR", index.to_str().unwrap())]);
        let corpus = corpus_for(&config);
        assert!(matches!(corpus, AnyCorpus::Dila(_)));
        let results = every_operation(&corpus).await;
        let by_name =
            |name: &str| &results.iter().find(|(operation, _)| *operation == name).unwrap().1;
        // Les consultations des fonds de jurisprudence, CNIL, KALI et ACCO sont refusées.
        for name in ["get_decision", "get_deliberation", "get_convention", "get_company_agreement"]
        {
            let error = by_name(name).as_ref().unwrap_err();
            assert!(settings_detail(error).contains("LEGI fund only"), "{name}: {error}");
        }
        // Les recherches aussi, fond par fond.
        for fond in [Fond::Juri, Fond::Constit, Fond::Cetat, Fond::Cnil, Fond::Kali, Fond::Acco] {
            let error = corpus.search(&search(fond)).await.unwrap_err();
            assert!(settings_detail(&error).contains("LEGI fund only"), "{}: {error}", fond.name());
        }
        // Et la recherche par numéro de décision ou d'affaire.
        let error = by_name("find_by_number").as_ref().unwrap_err();
        assert!(settings_detail(error).contains("LEGI fund only"), "{error}");
        // Les fonds LEGI, eux, sont servis : l'exclusion n'est pas une panne générale.
        corpus.search(&search(Fond::Code)).await.expect("the CODE fund is served");
        corpus.search(&search(Fond::Loda)).await.expect("the LODA fund is served");
    }

    #[tokio::test]
    async fn degraded_configuration_still_builds_and_answers_without_network() {
        // Aucun identifiant : le service se construit et `fr_coverage` répond sans réseau.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let config = load(&MapEnv::new(&[audit_dir(dir.path())])).config;
        let service = build(&config);
        let output = service.coverage(coverage_call(), pending()).await.expect("fr_coverage");
        assert_eq!(output.coverage.status(), lf_domain::coverage::CoverageStatus::Degraded);
    }
}
