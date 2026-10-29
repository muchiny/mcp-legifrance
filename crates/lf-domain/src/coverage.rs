//! Couverture déclarée du connecteur (`coverage.py`, élément 9 du canon `MateMatic`).
//!
//! Invariant : la liste des lacunes connues n'est jamais vide — une liste vide
//! voudrait dire « personne n'a vérifié », pas « aucune lacune » (`coverage.py:1-8`).
//! Les données déclarées (qui nomment les outils MCP) vivent dans l'adapter MCP.

use std::fmt;

use crate::{date::CivilDate, error::DomainError, fond::Fond};

/// État de la source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    /// Source joignable.
    Ok,
    /// Source partiellement joignable.
    Degraded,
    /// Source injoignable.
    Failed,
}

/// Famille de données exposée.
///
/// Forme reprise de `CoverageFamily` de la référence (`coverage.py:29-33`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageFamily {
    /// Nom de la famille.
    pub name: String,
    /// Outil(s) qui y donnent accès.
    pub tool: String,
    /// Source officielle.
    pub source: String,
    /// Date de capture, absente pour une interrogation en direct.
    pub captured_at: Option<CivilDate>,
    /// Interrogée en direct plutôt que depuis un instantané.
    pub live: bool,
}

/// Lacune connue et moyen de la contourner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageGap {
    /// Identifiant stable, ex. `FR-001`.
    pub id: String,
    /// Famille concernée.
    pub family: String,
    /// Ce qui manque.
    pub missing: String,
    /// Où aller à la place.
    pub fallback: String,
}

/// Environnement de l'API PISTE interrogée.
///
/// Seul type d'environnement du workspace : il n'existe pas de double (R3), de sorte que
/// l'adapter PISTE, le cache et l'audit l'emploient sans dépendre les uns des autres.
/// La lecture de `LF_PISTE_ENV` n'appartient pas au domaine : elle se fait dans `lf-app`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PisteEnvironment {
    /// Bac à sable de PISTE.
    Sandbox,
    /// Production de PISTE.
    Production,
}

impl PisteEnvironment {
    /// Nom de l'environnement, valeur de `LF_PISTE_ENV`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sandbox => "sandbox",
            Self::Production => "production",
        }
    }
}

impl fmt::Display for PisteEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Couverture du connecteur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    status: CoverageStatus,
    as_of_note: String,
    families: Vec<CoverageFamily>,
    known_gaps: Vec<CoverageGap>,
}

impl Coverage {
    /// Construit la déclaration.
    ///
    /// # Errors
    ///
    /// [`DomainError::EmptyCoverageGaps`] si `known_gaps` est vide.
    pub fn new(
        status: CoverageStatus,
        as_of_note: String,
        families: Vec<CoverageFamily>,
        known_gaps: Vec<CoverageGap>,
    ) -> Result<Self, DomainError> {
        Self { status, as_of_note, families, known_gaps }.checked()
    }

    /// La déclaration, si elle nomme au moins une lacune. Le contrôle vit ici et non dans
    /// `new`, que cargo-mutants ne mute jamais (`CONTRIBUTING.md`).
    fn checked(self) -> Result<Self, DomainError> {
        let has_gaps = !self.known_gaps.is_empty();
        has_gaps.then_some(self).ok_or(DomainError::EmptyCoverageGaps)
    }

    /// État de la source.
    pub fn status(&self) -> CoverageStatus {
        self.status
    }

    /// Ce que signifient (et ne garantissent pas) les dates.
    pub fn as_of_note(&self) -> &str {
        &self.as_of_note
    }

    /// Familles exposées.
    pub fn families(&self) -> &[CoverageFamily] {
        &self.families
    }

    /// Lacunes connues, jamais vides.
    pub fn known_gaps(&self) -> &[CoverageGap] {
        &self.known_gaps
    }
}

/// Ce que la configuration du processus apprend à la couverture (03 §4.8) : trois faits, sans
/// secret ni phrase. La liste des problèmes détectés reste dans `lf-application`
/// (`ConfigurationState`) et n'entre pas dans le domaine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigFacts {
    /// Les identifiants PISTE sont présents (FR-102 sinon).
    pub credentials_configured: bool,
    /// Un hôte hors des quatre hôtes officiels est configuré (FR-103).
    pub custom_host: bool,
    /// Au moins un problème de configuration est détecté, mode dégradé compris (FR-104).
    pub has_problems: bool,
}

/// Couverture déclarée du connecteur pour un environnement, une configuration et les fonds
/// servis (03 §4.8 : catalogue FR-001 à FR-009 et lacunes calculées FR-007, FR-101 à FR-104).
///
/// STUB(V0) → V1-03 : forme et signature gelées, corps provisoire. Il rend une seule lacune
/// fixe et ignore ses arguments, pour que `fr_coverage` compile avant que V1-03 n'écrive le
/// catalogue.
#[must_use]
pub fn coverage(_env: PisteEnvironment, _facts: &ConfigFacts, _served: &[Fond]) -> Coverage {
    // STUB(V0) → V1-03
    Coverage {
        status: CoverageStatus::Ok,
        as_of_note: String::new(),
        families: Vec::new(),
        known_gaps: vec![CoverageGap {
            id: "FR-001".to_owned(),
            family: "Legislation (LODA, codes)".to_owned(),
            missing: "native ELI".to_owned(),
            fallback: "Cite source_url; never build an /eli/ path.".to_owned(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn environment_names_are_the_env_values() {
        assert_eq!(PisteEnvironment::Sandbox.name(), "sandbox");
        assert_eq!(PisteEnvironment::Production.name(), "production");
    }

    #[test]
    fn environment_displays_its_name() {
        assert_eq!(PisteEnvironment::Sandbox.to_string(), "sandbox");
        assert_eq!(PisteEnvironment::Production.to_string(), "production");
    }

    #[test]
    fn environment_hashes_by_value() {
        // trois valeurs dont un doublon : l'égalité et l'empreinte vont par valeur
        let set: HashSet<PisteEnvironment> =
            [PisteEnvironment::Sandbox, PisteEnvironment::Production, PisteEnvironment::Sandbox]
                .into_iter()
                .collect();
        assert_eq!(set.len(), 2);
        assert!(set.contains(&PisteEnvironment::Sandbox));
        assert!(set.contains(&PisteEnvironment::Production));
        assert_ne!(PisteEnvironment::Sandbox, PisteEnvironment::Production);
    }

    #[test]
    fn environment_is_copy() {
        let a = PisteEnvironment::Production;
        let b = a; // `a` reste employable : le test cesse de compiler si `Copy` disparaît
        assert_eq!(a, b);
    }

    fn gap() -> CoverageGap {
        CoverageGap {
            id: "FR-001".into(),
            family: "Legislation (LODA, codes)".into(),
            missing: "native ELI".into(),
            fallback: "Cite source_url; never build an /eli/ path.".into(),
        }
    }

    fn family() -> CoverageFamily {
        CoverageFamily {
            name: "Case law (JURI)".into(),
            tool: "fr_get_decision".into(),
            source: "legifrance.gouv.fr via the PISTE API".into(),
            captured_at: None,
            live: true,
        }
    }

    #[test]
    fn config_facts_are_plain_copyable_values() {
        let facts =
            ConfigFacts { credentials_configured: true, custom_host: false, has_problems: false };
        let copy = facts; // `facts` reste employable : le test cesse de compiler sans `Copy`
        assert_eq!(facts, copy);
        assert_ne!(facts, ConfigFacts { custom_host: true, ..facts });
        assert!(format!("{facts:?}").contains("credentials_configured: true"));
    }

    #[test]
    fn stub_coverage_names_at_least_one_gap() {
        // STUB(V0) → V1-03 : seule la forme est gelée, la liste des lacunes est vérifiée là-bas.
        let facts =
            ConfigFacts { credentials_configured: false, custom_host: false, has_problems: true };
        let c = coverage(PisteEnvironment::Sandbox, &facts, &[Fond::Loda]);
        assert_eq!(c.known_gaps().len(), 1);
        assert_eq!(c.known_gaps()[0].id, "FR-001");
    }

    #[test]
    fn refuses_empty_gaps() {
        assert_eq!(
            Coverage::new(CoverageStatus::Ok, "note".into(), vec![family()], vec![]),
            Err(DomainError::EmptyCoverageGaps)
        );
    }

    #[test]
    fn exposes_its_parts() {
        let c = Coverage::new(CoverageStatus::Degraded, "note".into(), vec![family()], vec![gap()])
            .unwrap();
        assert_eq!(c.status(), CoverageStatus::Degraded);
        assert_eq!(c.as_of_note(), "note");
        assert_eq!(c.families(), [family()]);
        assert_eq!(c.known_gaps(), [gap()]);
    }
}
