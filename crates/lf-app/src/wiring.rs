//! Câblage : assemble les adapters en un `LegalService`, puis le sert sur stdio (02 §8.4).
//!
//! La composition de production est `LegalService<CachedCorpus<PisteCorpus>, AppAudit,
//! SystemClock>` ([`AppService`]) :
//!
//! - `PisteCorpus::new` ne fait aucune E/S : le client HTTP et le jeton OAuth naissent au premier
//!   appel, et un problème de configuration ou des identifiants absents donnent le mode dégradé
//!   (`[config_error]` sur les outils PISTE, sans réseau) plutôt qu'un refus de démarrer (00 §1) ;
//! - `CachedCorpus` décore le port, l'audit restant au-dessus, dans la façade (ADR-0008) ;
//! - [`AppAudit`] est `JsonlAuditLog`, ou un journal inutilisable qui échoue à chaque ligne quand
//!   `LF_AUDIT_DIR` est inutilisable : `audit_error` dès le premier appel en mode `strict`
//!   (02 §8.2), jamais un refus de démarrer ;
//! - le mode d'audit (`LF_AUDIT_MODE`, `strict` par défaut, Q9) et l'échéance d'un appel
//!   (`LF_TOOL_DEADLINE_S`) arrivent dans l'[`AuditPolicy`] de la façade par
//!   [`service_settings`].
//!
//! [`run`] est la commande `serve` : stdin et stdout portent le JSON-RPC (stdout n'a pas d'autre
//! usage), les journaux vont sur stderr. Le code de sortie est 0 à l'EOF de stdin et à l'arrêt
//! demandé, et une erreur (code 1 dans `main`) si le service échoue (02 §8.8).

use std::{process::ExitCode, sync::Arc};

use anyhow::Context as _;
use lf_adapter_audit::JsonlAuditLog;
use lf_adapter_cache::{CachePolicy, CachedCorpus};
use lf_adapter_mcp::{LegifranceServer, serve_stdio};
use lf_adapter_piste::PisteCorpus;
use lf_application::{
    audited::AuditPolicy,
    budget::ResponseBudget,
    ports::{AuditError, AuditErrorKind, AuditLog, AuditRecord},
    service::{LegalService, ServiceSettings},
};
use tokio_util::sync::CancellationToken;

use crate::{
    clock::SystemClock,
    config::{AppConfig, AuditSettings},
};

/// Le corpus de production : PISTE derrière le cache (02 §8.4).
pub(crate) type AppCorpus = CachedCorpus<PisteCorpus>;

/// La façade de production : corpus, journal d'audit et horloge de [`build`].
pub(crate) type AppService = LegalService<AppCorpus, AppAudit, SystemClock>;

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
            env: config.piste.env(),
            server_version: env!("CARGO_PKG_VERSION"),
        },
        configuration: config.configuration.clone(),
        budget: ResponseBudget,
    }
}

/// Assemble la façade de production. Aucune E/S : ni réseau, ni fichier, ni répertoire.
pub(crate) fn build(config: &AppConfig) -> AppService {
    let piste = PisteCorpus::new(config.piste.clone());
    let corpus = CachedCorpus::new(piste, config.piste.env(), CachePolicy::default());
    LegalService::new(corpus, AppAudit::open(&config.audit), SystemClock, service_settings(config))
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
        env = %config.piste.env(),
        "starting"
    );
    let server = LegifranceServer::new(Arc::new(build(&config)));
    let outcome = serve_stdio(server, shutdown).await.context("the MCP server failed")?;
    tracing::info!(?outcome, "stopped");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use std::{future::pending, path::Path};

    use lf_application::{
        audited::AuditMode,
        error::ErrorCode,
        input::{CanonicalInput, CoverageInput, ToolCall},
        ports::{AuditStatus, PisteEnvironment},
        tools::ToolName,
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
