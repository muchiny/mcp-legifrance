//! Sortie de `fr_coverage` (v0.1) : `CoverageOutput` et sa métrique d'audit.
//!
//! Elle réunit ce que la vue `CoverageView` rend (05 §5.8) : la couverture calculée par le domaine
//! (statut, familles, lacunes), l'environnement PISTE, l'état de la configuration, et la date de
//! consultation (attribution des données, Q15). Les textes de présentation (`source`,
//! `attribution`, `eli_note`, noms d'outils des familles) sont ceux de l'adapter MCP.

use lf_domain::{
    coverage::{Coverage, PisteEnvironment},
    date::CivilDate,
};

use crate::audited::{AuditMetric, AuditMode};

/// État de la configuration, tel que `fr_coverage` le rend : sans secret ni valeur de variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageConfiguration {
    /// Les identifiants PISTE sont présents.
    pub credentials_configured: bool,
    /// Mode d'audit en vigueur (`strict` ou `best-effort`).
    pub audit_mode: AuditMode,
    /// Problèmes de configuration détectés, en phrases sans secret (vide si aucun).
    pub problems: Vec<String>,
}

/// Ce que `CoverageUseCase` rend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageOutput {
    /// Couverture calculée par `lf_domain::coverage::coverage` : statut, familles, lacunes
    /// (jamais vides).
    pub coverage: Coverage,
    /// Environnement PISTE interrogé (`LF_PISTE_ENV`).
    pub environment: PisteEnvironment,
    /// État de la configuration.
    pub configuration: CoverageConfiguration,
    /// Date de consultation : le jour courant à Paris (`Clock::today_paris`).
    pub consulted_on: CivilDate,
}

/// Métrique d'audit : le nombre de lacunes déclarées (05 §5.8).
impl AuditMetric for CoverageOutput {
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.coverage.known_gaps().len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::{
        coverage::{ConfigFacts, coverage},
        fond::Fond,
    };
    use pretty_assertions::assert_eq;

    fn output(env: PisteEnvironment, credentials: bool) -> CoverageOutput {
        let facts = ConfigFacts {
            credentials_configured: credentials,
            custom_host: false,
            has_problems: false,
        };
        CoverageOutput {
            coverage: coverage(env, &facts, &Fond::ALL),
            environment: env,
            configuration: CoverageConfiguration {
                credentials_configured: credentials,
                audit_mode: AuditMode::Strict,
                problems: Vec::new(),
            },
            consulted_on: CivilDate::new(2026, 10, 29).unwrap(),
        }
    }

    #[test]
    fn the_metric_is_the_number_of_gaps() {
        // Production avec identifiants : les neuf lacunes du catalogue ; la sandbox en ajoute une,
        // l'absence d'identifiants aussi.
        assert_eq!(output(PisteEnvironment::Production, true).output_count_or_size(), 9);
        assert_eq!(output(PisteEnvironment::Sandbox, true).output_count_or_size(), 10);
        assert_eq!(output(PisteEnvironment::Sandbox, false).output_count_or_size(), 11);
    }
}
