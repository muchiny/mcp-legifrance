//! Cas d'usage de `fr_coverage` (v0.1) : la déclaration de couverture (`CoverageUseCase`).
//!
//! Aucun appel au port ni au réseau (05 §5.8) : il lit les réglages de la façade
//! (`ServiceSettings` : environnement de la politique d'audit, `ConfigurationState::facts()`),
//! calcule la couverture par la fonction pure du domaine, et date la consultation avec l'horloge
//! (jour courant à Paris, Q15).

use lf_domain::{coverage::coverage, fond::Fond};

use crate::{
    input::CoverageInput,
    output::coverage::{CoverageConfiguration, CoverageOutput},
    ports::Clock,
    service::ServiceSettings,
};

/// Fonds servis par `fr_search` en v0.1 : tous ceux que connaît le domaine (JORF n'en fait pas
/// partie, d'où la lacune FR-007).
const SERVED: [Fond; 8] = Fond::ALL;

/// Fonds que sert l'instantané local de l'index DILA : LEGI seul, les codes et les textes
/// consolidés. Décisions, délibérations CNIL, conventions et accords n'y sont pas.
const SERVED_BY_SNAPSHOT: [Fond; 2] = [Fond::Loda, Fond::Code];

/// Déclare ce que le serveur couvre, et ce qu'il ne couvre pas.
pub(crate) struct CoverageUseCase<'a, K: Clock> {
    settings: &'a ServiceSettings,
    clock: &'a K,
}

impl<'a, K: Clock> CoverageUseCase<'a, K> {
    /// Cas d'usage qui lit `settings` et date la consultation avec `clock`.
    pub(crate) fn new(settings: &'a ServiceSettings, clock: &'a K) -> Self {
        Self { settings, clock }
    }

    /// La couverture : statut et lacunes d'après la configuration, environnement, configuration
    /// et date de consultation. Sans erreur possible : l'outil ne prend aucun argument, et le
    /// refus d'un argument se fait au décodage (`invalid_arg`).
    pub(crate) fn execute(&self, _input: &CoverageInput) -> CoverageOutput {
        let configuration = &self.settings.configuration;
        let environment = self.settings.policy.env;
        let served: &[Fond] =
            if configuration.local_snapshot { &SERVED_BY_SNAPSHOT } else { &SERVED };
        CoverageOutput {
            coverage: coverage(environment, &configuration.facts(), served),
            environment,
            configuration: CoverageConfiguration {
                credentials_configured: configuration.credentials_configured,
                audit_mode: self.settings.policy.mode,
                problems: configuration.problems.clone(),
                local_snapshot: configuration.local_snapshot,
            },
            consulted_on: self.clock.today_paris(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        ports::PisteEnvironment,
        service::ConfigurationState,
        testing::FixedClock,
    };
    use chrono::DateTime;
    use lf_domain::{coverage::CoverageStatus, date::CivilDate};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn settings(
        env: PisteEnvironment,
        mode: AuditMode,
        configuration: ConfigurationState,
    ) -> ServiceSettings {
        ServiceSettings {
            policy: AuditPolicy {
                mode,
                deadline: Duration::from_secs(60),
                env,
                server_version: "0.0.0",
            },
            configuration,
            budget: ResponseBudget,
        }
    }

    fn configured() -> ConfigurationState {
        ConfigurationState { credentials_configured: true, ..ConfigurationState::default() }
    }

    fn clock() -> FixedClock {
        FixedClock::new(DateTime::UNIX_EPOCH, CivilDate::new(2026, 10, 29).unwrap())
    }

    fn run(settings: &ServiceSettings) -> CoverageOutput {
        CoverageUseCase::new(settings, &clock()).execute(&CoverageInput)
    }

    fn gap_ids(output: &CoverageOutput) -> Vec<&str> {
        output.coverage.known_gaps().iter().map(|gap| gap.id.as_str()).collect()
    }

    #[test]
    fn a_configured_sandbox_is_ok_with_the_sandbox_gap() {
        let output = run(&settings(PisteEnvironment::Sandbox, AuditMode::Strict, configured()));
        assert_eq!(output.coverage.status(), CoverageStatus::Ok);
        assert_eq!(output.environment, PisteEnvironment::Sandbox);
        assert_eq!(gap_ids(&output).last(), Some(&"FR-101"));
        assert_eq!(output.coverage.families().len(), 8);
        assert_eq!(
            output.configuration,
            CoverageConfiguration {
                credentials_configured: true,
                audit_mode: AuditMode::Strict,
                problems: Vec::new(),
                local_snapshot: false,
            }
        );
    }

    /// V4-02 : depuis l'index local DILA, la couverture ne se présente plus comme du direct. Elle
    /// ne liste que les familles que l'instantané sert (LEGI), n'est jamais `ok` et dit pourquoi.
    #[test]
    fn a_local_snapshot_is_degraded_lists_only_legi_and_says_it_is_not_live() {
        let configuration = ConfigurationState { local_snapshot: true, ..configured() };
        let output = run(&settings(PisteEnvironment::Production, AuditMode::Strict, configuration));
        assert_eq!(output.coverage.status(), CoverageStatus::Degraded);
        assert_eq!(gap_ids(&output).last(), Some(&"FR-105"));
        assert!(output.configuration.local_snapshot);
        let names: Vec<&str> =
            output.coverage.families().iter().map(|family| family.name.as_str()).collect();
        assert_eq!(names, ["LODA", "CODE"]);
        assert!(output.coverage.families().iter().all(|family| !family.live));
        assert!(!output.coverage.as_of_note().contains("queried live against the source"));
        // Le catalogue statique, lui, est inchangé.
        assert_eq!(gap_ids(&output).len(), 10);
    }

    #[test]
    fn coverage_degraded_without_credentials() {
        let output = run(&settings(
            PisteEnvironment::Production,
            AuditMode::Strict,
            ConfigurationState::default(),
        ));
        assert_eq!(output.coverage.status(), CoverageStatus::Degraded);
        assert_eq!(gap_ids(&output).last(), Some(&"FR-102"));
        assert!(!output.configuration.credentials_configured);
    }

    #[test]
    fn coverage_degraded_on_config_problem_and_lists_the_problems() {
        let configuration = ConfigurationState {
            problems: vec!["LF_PISTE_ENV is invalid".to_owned()],
            ..configured()
        };
        let output =
            run(&settings(PisteEnvironment::Production, AuditMode::BestEffort, configuration));
        assert_eq!(output.coverage.status(), CoverageStatus::Degraded);
        assert_eq!(gap_ids(&output).last(), Some(&"FR-104"));
        assert_eq!(output.configuration.problems, ["LF_PISTE_ENV is invalid"]);
        assert_eq!(output.configuration.audit_mode, AuditMode::BestEffort);
    }

    #[test]
    fn coverage_custom_host_is_a_gap_not_a_problem() {
        let configuration = ConfigurationState { custom_host: true, ..configured() };
        let output = run(&settings(PisteEnvironment::Production, AuditMode::Strict, configuration));
        assert_eq!(output.coverage.status(), CoverageStatus::Ok);
        assert_eq!(gap_ids(&output).last(), Some(&"FR-103"));
        assert_eq!(output.configuration.problems, Vec::<String>::new());
    }

    #[test]
    fn coverage_attribution_and_consultation_date() {
        // La date de consultation est le jour courant à Paris de l'horloge, pas la date UTC :
        // 22 h 30 UTC le 28 octobre est déjà le 29 à Paris.
        let late_utc = DateTime::from_timestamp(1_793_226_600, 0).unwrap();
        assert_eq!(late_utc.to_rfc3339(), "2026-10-28T22:30:00+00:00");
        let clock = FixedClock::new(late_utc, CivilDate::new(2026, 10, 29).unwrap());
        let settings = settings(PisteEnvironment::Production, AuditMode::Strict, configured());
        let output = CoverageUseCase::new(&settings, &clock).execute(&CoverageInput);
        assert_eq!(output.consulted_on, CivilDate::new(2026, 10, 29).unwrap());
    }
}
