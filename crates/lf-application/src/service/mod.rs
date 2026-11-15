//! Façade `LegalService` (02 §6.3) : un `impl` par outil, chacun sous `Audited::run`.
//!
//! Le motif gelé, que chaque tranche écrit dans son fichier `service/<outil>.rs` :
//!
//! ```text
//! pub async fn <outil>(
//!     &self,
//!     call: ToolCall<XInput>,
//!     cancel: impl Future<Output = ()> + Send,
//! ) -> Result<XOutput, AppError>
//! ```
//!
//! `cancel` se résout quand le client annule (le `ct.cancelled()` de rmcp, passé par l'adapter MCP
//! sans qu'aucun type de rmcp n'entre ici). Les modules enfants voient les champs privés de
//! [`LegalService`]. Exceptions : `read_resource` (audité sous `resource:<gabarit>`) et `complete`
//! (non audité, liste vide en cas d'erreur), dans `resource.rs` et `complete.rs`.

use lf_domain::coverage::ConfigFacts;
pub use lf_domain::coverage::DoctrineSource;

use crate::{
    audited::{AuditPolicy, Audited},
    budget::ResponseBudget,
    citation::resolve::Proof,
    ports::{AuditLog, Clock, LegalCorpus, NoDoctrine, TaxDoctrine},
    usecase::complete::CompleteGate,
};

mod complete;
mod coverage;
mod diff_article;
mod get_act;
mod get_bofip;
mod get_company_agreement;
mod get_convention;
mod get_decision;
mod get_deliberation;
mod get_related;
mod get_text;
mod resolve;
mod resource;
mod search;
mod search_all;
mod search_bofip;
mod verify_citations;

/// État de la configuration du processus, tel que `fr_coverage` le rend (05 §5.8). Rempli par
/// `lf-app` au démarrage ; la liste des problèmes est faite de phrases sans secret.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigurationState {
    /// Les identifiants PISTE sont présents.
    pub credentials_configured: bool,
    /// Un hôte hors des quatre hôtes officiels est configuré (avertissement `CustomHost`).
    pub custom_host: bool,
    /// Problèmes de configuration détectés, mode dégradé compris (00 §1).
    pub problems: Vec<String>,
    /// La source des textes est l'instantané local de l'index DILA (LEGI seul), non PISTE en
    /// direct. Posé par `lf-app` quand `LF_DILA_DIR` choisit l'index local. Un dump peut être
    /// périmé : une absence n'y prouve rien (`fr_verify_citations` rend `unresolvable`, jamais
    /// `not_found`), et `fr_coverage` décrit la source comme un instantané. L'environnement de
    /// l'audit reste celui d'un texte publié (`production`) : seul ce drapeau distingue l'index
    /// local.
    pub local_snapshot: bool,
    /// État de la source de doctrine fiscale (BOFiP), pour `fr_coverage` : active (FR-106),
    /// coupée (FR-107), ou `NotServed`, le défaut, tant que le serveur ne sert pas ses outils.
    /// Posé par `lf-app` avec la source elle-même (`LegalService::with_doctrine`).
    pub doctrine: DoctrineSource,
}

impl ConfigurationState {
    /// Les cinq faits que le domaine lit pour calculer la couverture ; la liste des problèmes
    /// n'y entre pas.
    #[must_use]
    pub fn facts(&self) -> ConfigFacts {
        ConfigFacts {
            credentials_configured: self.credentials_configured,
            custom_host: self.custom_host,
            has_problems: !self.problems.is_empty(),
            local_snapshot: self.local_snapshot,
            doctrine: self.doctrine,
        }
    }
}

/// Réglages de la façade (D13 du plan « fin du projet », qui amende 02 §6.3) : la politique
/// d'audit porte déjà le mode, l'échéance, l'environnement et la version du serveur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSettings {
    /// Politique de [`Audited`] : mode d'audit, échéance d'un appel, environnement, version.
    pub policy: AuditPolicy,
    /// État de la configuration, pour `fr_coverage`.
    pub configuration: ConfigurationState,
    /// Bornes de réponse.
    pub budget: ResponseBudget,
}

impl ServiceSettings {
    /// Ce que prouve l'absence d'un enregistrement dans la source interrogée : l'environnement de
    /// la politique, sauf pour l'instantané local, que seul le drapeau de la configuration
    /// distingue.
    pub(crate) const fn proof(&self) -> Proof {
        Proof::of(self.policy.env, self.configuration.local_snapshot)
    }
}

/// Façade des cas d'usage, partagée par `Arc` entre les tâches. Elle est `Send + Sync` parce que
/// `C`, `A`, `K` et `D` le sont par les bornes des ports.
///
/// `D` est la source de doctrine fiscale (BOFiP, ADR-0028). Son défaut, [`NoDoctrine`], est une
/// source coupée : [`LegalService::new`] assemble une façade sans doctrine, et
/// [`LegalService::with_doctrine`] pose la source.
pub struct LegalService<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine = NoDoctrine> {
    corpus: C,
    doctrine: D,
    audited: Audited<A>,
    clock: K,
    settings: ServiceSettings,
    /// Débit des complétions, un état par processus (02 §6.2).
    complete_gate: CompleteGate,
}

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// Assemble la façade, sans source de doctrine ([`NoDoctrine`]) ; `audit` est enveloppé dans
    /// un [`Audited`] qui suit `settings.policy`.
    #[must_use]
    pub fn new(corpus: C, audit: A, clock: K, settings: ServiceSettings) -> Self {
        let audited = Audited::new(audit, settings.policy);
        Self {
            corpus,
            doctrine: NoDoctrine,
            audited,
            clock,
            settings,
            complete_gate: CompleteGate::new(),
        }
    }
}

impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine> LegalService<C, A, K, D> {
    /// La même façade, avec `doctrine` pour source de doctrine fiscale. Le reste (corpus, audit,
    /// horloge, réglages, débit des complétions) est repris tel quel.
    #[must_use]
    pub fn with_doctrine<D2: TaxDoctrine>(self, doctrine: D2) -> LegalService<C, A, K, D2> {
        LegalService {
            corpus: self.corpus,
            doctrine,
            audited: self.audited,
            clock: self.clock,
            settings: self.settings,
            complete_gate: self.complete_gate,
        }
    }

    /// Réglages de la façade.
    #[must_use]
    pub fn settings(&self) -> &ServiceSettings {
        &self.settings
    }
}

/// Façades de test des deux outils du BOFiP : la même façade avec et sans source de doctrine, et
/// une poignée sur son journal d'audit.
#[cfg(test)]
pub(crate) mod bofip_fixtures {
    use std::{sync::Arc, time::Duration};

    use chrono::DateTime;
    use lf_domain::date::CivilDate;

    use super::{ConfigurationState, LegalService, ServiceSettings};
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        ports::{AuditError, AuditLog, AuditRecord, NoDoctrine, PisteEnvironment},
        testing::{FakeCorpus, FakeDoctrine, FixedClock, MemoryAuditLog},
    };

    /// Journal partagé : le service possède le sien, le test en garde une poignée.
    pub(crate) struct SharedAudit(Arc<MemoryAuditLog>);

    impl AuditLog for SharedAudit {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    /// Façade de test, de source de doctrine `D`.
    pub(crate) type Service<D> = LegalService<FakeCorpus, SharedAudit, FixedClock, D>;

    /// Réglages des façades de test.
    pub(crate) fn settings() -> ServiceSettings {
        ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "0.0.0",
            },
            configuration: ConfigurationState::default(),
            budget: ResponseBudget,
        }
    }

    /// Façade telle que `LegalService::new` l'assemble : sans source de doctrine.
    pub(crate) fn service_without_doctrine() -> (Service<NoDoctrine>, Arc<MemoryAuditLog>) {
        let log = Arc::new(MemoryAuditLog::new());
        let clock = FixedClock::new(
            DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
            CivilDate::new(2026, 10, 29).unwrap(),
        );
        let service =
            LegalService::new(FakeCorpus::new(), SharedAudit(Arc::clone(&log)), clock, settings());
        (service, log)
    }

    /// La même façade, avec une fausse source de doctrine.
    pub(crate) fn service_with_doctrine() -> (Service<FakeDoctrine>, Arc<MemoryAuditLog>) {
        let (service, log) = service_without_doctrine();
        (service.with_doctrine(FakeDoctrine::new()), log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audited::AuditMode,
        ports::PisteEnvironment,
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };
    use chrono::DateTime;
    use lf_domain::date::CivilDate;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn settings(configuration: ConfigurationState) -> ServiceSettings {
        ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "0.0.0",
            },
            configuration,
            budget: ResponseBudget,
        }
    }

    #[test]
    fn configuration_facts_reflect_the_three_flags() {
        let ok = ConfigurationState { credentials_configured: true, ..Default::default() };
        assert_eq!(
            ok.facts(),
            ConfigFacts {
                credentials_configured: true,
                custom_host: false,
                has_problems: false,
                local_snapshot: false,
                doctrine: DoctrineSource::NotServed,
            }
        );
        let degraded = ConfigurationState {
            credentials_configured: false,
            custom_host: true,
            problems: vec!["LF_PISTE_ENV is invalid".to_owned()],
            local_snapshot: false,
            doctrine: DoctrineSource::NotServed,
        };
        assert_eq!(
            degraded.facts(),
            ConfigFacts {
                credentials_configured: false,
                custom_host: true,
                has_problems: true,
                local_snapshot: false,
                doctrine: DoctrineSource::NotServed,
            }
        );
        let snapshot = ConfigurationState { local_snapshot: true, ..Default::default() };
        assert_eq!(
            snapshot.facts(),
            ConfigFacts {
                credentials_configured: false,
                custom_host: false,
                has_problems: false,
                local_snapshot: true,
                doctrine: DoctrineSource::NotServed,
            }
        );
        // L'état de la source de doctrine passe tel quel, dans ses trois valeurs.
        for doctrine in [DoctrineSource::NotServed, DoctrineSource::Active, DoctrineSource::Off] {
            let state = ConfigurationState { doctrine, ..Default::default() };
            assert_eq!(state.facts().doctrine, doctrine);
        }
        // Une configuration par défaut n'a ni identifiants ni problème, PISTE pour source, et
        // aucun outil du BOFiP servi.
        assert_eq!(
            ConfigurationState::default().facts(),
            ConfigFacts {
                credentials_configured: false,
                custom_host: false,
                has_problems: false,
                local_snapshot: false,
                doctrine: DoctrineSource::NotServed,
            }
        );
    }

    /// Le drapeau d'instantané local l'emporte sur l'environnement de la politique, que l'index
    /// local déclare `production` : sans lui, un dump périmé prouverait l'inexistence.
    #[test]
    fn the_proof_follows_the_environment_unless_the_source_is_a_local_snapshot() {
        let proof = |env, local_snapshot| {
            let mut settings =
                settings(ConfigurationState { local_snapshot, ..Default::default() });
            settings.policy.env = env;
            settings.proof()
        };
        assert_eq!(proof(PisteEnvironment::Production, false), Proof::Production);
        assert_eq!(proof(PisteEnvironment::Sandbox, false), Proof::Sandbox);
        assert_eq!(proof(PisteEnvironment::Production, true), Proof::Snapshot);
    }

    #[test]
    fn service_exposes_its_settings_and_is_shareable() {
        fn assert_shareable<T: Send + Sync + 'static>(_: &T) {}
        let wanted = settings(ConfigurationState::default());
        let service = LegalService::new(
            FakeCorpus::new(),
            MemoryAuditLog::new(),
            FixedClock::new(DateTime::UNIX_EPOCH, CivilDate::new(2026, 10, 29).unwrap()),
            wanted.clone(),
        );
        assert_eq!(service.settings(), &wanted);
        assert_shareable(&service);
    }
    /// `with_doctrine` ne change que la source de doctrine : réglages, corpus, horloge et journal
    /// d'audit sont ceux de la façade d'origine.
    #[tokio::test]
    async fn with_doctrine_keeps_everything_but_the_doctrine_source() {
        use crate::{
            error::ErrorCode,
            input::{CanonicalInput, GetDeliberationInput, ToolCall},
            ports::{Clock, CorpusError},
            testing::FakeDoctrine,
            tools::ToolName,
        };
        use std::future::pending;

        let (plain, log) = bofip_fixtures::service_without_doctrine();
        // Programmé avant le changement de source : le corpus doit suivre.
        plain.corpus.push_get_deliberation(Err(CorpusError::Timeout));
        let now = plain.clock.now_utc();
        let service = plain.with_doctrine(FakeDoctrine::new());
        assert_eq!(service.settings(), &bofip_fixtures::settings());
        assert_eq!(service.clock.now_utc(), now);
        assert_eq!(service.doctrine.calls(), 0);
        let call = ToolCall {
            input: Ok(GetDeliberationInput {
                deliberation_id: Some("CNILTEXT000053352643".to_owned()),
                ..GetDeliberationInput::default()
            }),
            canonical: CanonicalInput::new("{}"),
        };
        let error = service.get_deliberation(call, pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_eq!((service.corpus.calls(), service.doctrine.calls()), (1, 0));
        // Le journal partagé reçoit toujours la ligne.
        let tools: Vec<ToolName> = log.records().iter().map(|line| line.tool).collect();
        assert_eq!(tools, [ToolName::FR_GET_DELIBERATION]);
    }
}
