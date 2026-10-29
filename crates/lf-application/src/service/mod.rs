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

use crate::{
    audited::{AuditPolicy, Audited},
    budget::ResponseBudget,
    citation::resolve::Proof,
    ports::{AuditLog, Clock, LegalCorpus},
    usecase::complete::CompleteGate,
};

mod complete;
mod coverage;
mod diff_article;
mod get_act;
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
}

impl ConfigurationState {
    /// Les quatre faits que le domaine lit pour calculer la couverture ; la liste des problèmes
    /// n'y entre pas.
    #[must_use]
    pub fn facts(&self) -> ConfigFacts {
        ConfigFacts {
            credentials_configured: self.credentials_configured,
            custom_host: self.custom_host,
            has_problems: !self.problems.is_empty(),
            local_snapshot: self.local_snapshot,
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
/// `C`, `A` et `K` le sont par les bornes des ports.
pub struct LegalService<C: LegalCorpus, A: AuditLog, K: Clock> {
    corpus: C,
    audited: Audited<A>,
    clock: K,
    settings: ServiceSettings,
    /// Débit des complétions, un état par processus (02 §6.2).
    complete_gate: CompleteGate,
}

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// Assemble la façade ; `audit` est enveloppé dans un [`Audited`] qui suit `settings.policy`.
    #[must_use]
    pub fn new(corpus: C, audit: A, clock: K, settings: ServiceSettings) -> Self {
        let audited = Audited::new(audit, settings.policy);
        Self { corpus, audited, clock, settings, complete_gate: CompleteGate::new() }
    }

    /// Réglages de la façade.
    #[must_use]
    pub fn settings(&self) -> &ServiceSettings {
        &self.settings
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
                local_snapshot: false
            }
        );
        let degraded = ConfigurationState {
            credentials_configured: false,
            custom_host: true,
            problems: vec!["LF_PISTE_ENV is invalid".to_owned()],
            local_snapshot: false,
        };
        assert_eq!(
            degraded.facts(),
            ConfigFacts {
                credentials_configured: false,
                custom_host: true,
                has_problems: true,
                local_snapshot: false
            }
        );
        let snapshot = ConfigurationState { local_snapshot: true, ..Default::default() };
        assert_eq!(
            snapshot.facts(),
            ConfigFacts {
                credentials_configured: false,
                custom_host: false,
                has_problems: false,
                local_snapshot: true
            }
        );
        // Une configuration par défaut n'a ni identifiants ni problème, et PISTE pour source.
        assert_eq!(
            ConfigurationState::default().facts(),
            ConfigFacts {
                credentials_configured: false,
                custom_host: false,
                has_problems: false,
                local_snapshot: false
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
}
