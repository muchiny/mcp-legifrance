//! Couverture déclarée du connecteur (`coverage.py`, élément 9 du canon `MateMatic`).
//!
//! Invariant : la liste des lacunes connues n'est jamais vide — une liste vide
//! voudrait dire « personne n'a vérifié », pas « aucune lacune » (`coverage.py:1-8`).
//!
//! La forme livrée est gardée (D11 du plan « fin du projet », qui tranche M1-D20) ; la fonction
//! pure [`coverage`] la remplit (03 §4.8) : le catalogue statique des lacunes FR-001 à FR-009 et
//! les lacunes calculées FR-101 à FR-104 d'après l'environnement, la configuration
//! ([`ConfigFacts`]) et les fonds servis. Le domaine ne nomme aucun outil MCP : une famille porte
//! le nom du fond (`Fond::name`), et c'est l'adapter MCP qui rattache libellé et outils à ce
//! fond. De même, une lacune nomme le fond concerné dans `family`, vide quand elle vaut pour le
//! connecteur entier.

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
    /// Nom de la famille : pour [`coverage`], le nom du fond servi (`Fond::name`, ex. `LODA`).
    pub name: String,
    /// Outil(s) qui y donnent accès. [`coverage`] le laisse vide : le domaine ne nomme pas les
    /// outils MCP, l'adapter les rattache au fond.
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
    /// Famille concernée. Pour [`coverage`], le nom du fond (`Fond::name`, ex. `ACCO`) ou une
    /// chaîne vide quand la lacune vaut pour le connecteur entier.
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

/// Ce que la configuration du processus apprend à la couverture (03 §4.8) : quatre faits, sans
/// secret ni phrase. La liste des problèmes détectés reste dans `lf-application`
/// (`ConfigurationState`) et n'entre pas dans le domaine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "quatre faits indépendants, chacun donnant sa lacune FR-102 à FR-105 : aucune machine à états ne les range"
)]
pub struct ConfigFacts {
    /// Les identifiants PISTE sont présents (FR-102 sinon).
    pub credentials_configured: bool,
    /// Un hôte hors des quatre hôtes officiels est configuré (FR-103).
    pub custom_host: bool,
    /// Au moins un problème de configuration est détecté, mode dégradé compris (FR-104).
    pub has_problems: bool,
    /// La source des textes est l'instantané local de l'index DILA (LEGI seul), non PISTE en
    /// direct (FR-105) : la couverture le dit, et son statut n'est jamais `Ok`.
    pub local_snapshot: bool,
}

/// Source des données de chaque famille (`SOURCE`, `coverage.py:57`).
pub const SOURCE: &str = "legifrance.gouv.fr via the PISTE API";

/// Source des données de chaque famille quand la source des textes est l'index local DILA
/// ([`ConfigFacts::local_snapshot`]) : un instantané des dumps LEGI, pas une interrogation en
/// direct de PISTE.
pub const SNAPSHOT_SOURCE: &str =
    "local snapshot of the DILA LEGI dumps (laws, decrees and codes), not queried live";

/// Ce que signifient, et ne garantissent pas, les dates (`AS_OF_NOTE`, `coverage.py:59-63`) :
/// la valeur `as_of_note` de `fr_coverage` (05 §5.8), mot pour mot.
pub const AS_OF_NOTE: &str = "Data is queried live against the source, so there is no local snapshot date. \
Any date on a record states when the source published it - not that this connector verified the \
text is still in force today.";

/// Ce que signifient, et ne garantissent pas, les dates quand la source est l'index local DILA :
/// la valeur `as_of_note` de `fr_coverage` en mode instantané. Elle ne dit plus « interrogé en
/// direct » : le dump peut être périmé, et sa date n'est pas rendue.
pub const SNAPSHOT_AS_OF_NOTE: &str = "Data comes from a local snapshot of the DILA LEGI dumps, not from a live query: texts published or \
amended after the dump are missing, and the dump date is not reported. Any date on a record states \
when the source published it - not that this connector verified the text is still in force today.";

/// Texte d'une lacune du catalogue (05 §5.8) : `fond` est le fond concerné, `None` quand elle
/// vaut pour le connecteur entier.
struct GapText {
    id: &'static str,
    fond: Option<Fond>,
    missing: &'static str,
    fallback: &'static str,
}

impl GapText {
    /// La lacune telle que `Coverage` la porte : `family` nomme le fond, vide sans fond.
    fn to_gap(&self) -> CoverageGap {
        CoverageGap {
            id: self.id.to_owned(),
            family: self.fond.map(Fond::name).unwrap_or_default().to_owned(),
            missing: self.missing.to_owned(),
            fallback: self.fallback.to_owned(),
        }
    }
}

/// Catalogue statique FR-001 à FR-009, toujours déclaré (05 §5.8). FR-007 vaut « tant que le fond
/// JORF n'est pas servi » : `Fond` n'a pas de variante JORF (surface JORF écartée, T-M7-01), donc
/// aucun `served` ne la lève.
const STATIC_GAPS: [GapText; 9] = [
    GapText {
        id: "FR-001",
        fond: None,
        missing: "The API returns no native ELI for legislation.",
        fallback: "Cite source_url; never build an /eli/ path.",
    },
    GapText {
        id: "FR-002",
        fond: None,
        missing: "Legal force for a given case is not evaluated: texts are returned as published.",
        fallback: "Check etat, validity dates and transitional provisions on the record itself.",
    },
    GapText {
        id: "FR-003",
        fond: Some(Fond::Acco),
        missing: "The full text of company agreements is only a .docx attachment and is not returned.",
        fallback: "Send the user to source_url.",
    },
    GapText {
        id: "FR-004",
        fond: Some(Fond::Cetat),
        missing: "Many CAA and TA decisions have no ECLI, and tribunal decisions are only partly published.",
        fallback: "Cite citation and source_url; never invent an ECLI.",
    },
    GapText {
        id: "FR-005",
        fond: Some(Fond::Juri),
        missing: "Only a selection of courts of appeal and almost no first-instance decisions are on Légifrance; Judilibre is not integrated.",
        fallback: "Search Judilibre (Cour de cassation open data) for other judicial decisions.",
    },
    GapText {
        id: "FR-006",
        fond: Some(Fond::Juri),
        missing: "Published decisions are pseudonymised (JURI) and CNIL texts redacted: party names are replaced.",
        fallback: "Do not try to re-identify parties.",
    },
    GapText {
        id: "FR-007",
        fond: None,
        missing: "The Journal officiel (JORF) fond is not searchable yet.",
        fallback: "Search LODA: consolidated texts carry their JO reference.",
    },
    GapText {
        id: "FR-008",
        fond: None,
        missing: "Search pages hold at most 100 hits and results beyond 10,000 are not reachable.",
        fallback: "Narrow the query (match=all or exact, date_from/date_to).",
    },
    GapText {
        id: "FR-009",
        fond: None,
        missing: "Not covered: EU law, CJEU and ECHR case law, doctrine, circulars, parliamentary documents.",
        fallback: "Use EUR-Lex, HUDOC or the relevant official source.",
    },
];

/// FR-101 : environnement bac à sable (D-18).
const SANDBOX_GAP: GapText = GapText {
    id: "FR-101",
    fond: None,
    missing: "Running on the PISTE sandbox: data may differ from production and availability is not guaranteed.",
    fallback: "Set LF_PISTE_ENV=production with production credentials.",
};

/// FR-102 : identifiants absents.
const NO_CREDENTIALS_GAP: GapText = GapText {
    id: "FR-102",
    fond: None,
    missing: "PISTE credentials are not configured: every tool except fr_coverage fails with config_error.",
    fallback: "Set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET.",
};

/// FR-103 : hôte hors des quatre hôtes officiels (avertissement `CustomHost`, 00 §1).
const CUSTOM_HOST_GAP: GapText = GapText {
    id: "FR-103",
    fond: None,
    missing: "A custom API host is configured (LF_PISTE_BASE_URL or LF_PISTE_OAUTH_URL): responses may not come from Légifrance.",
    fallback: "Remove the override unless it is intended.",
};

/// FR-104 : configuration invalide, mode dégradé de 00 §1 (dont `EnvMismatch`).
const INVALID_CONFIG_GAP: GapText = GapText {
    id: "FR-104",
    fond: None,
    missing: "The server configuration is invalid (see configuration.problems): every tool except fr_coverage fails with config_error.",
    fallback: "Fix the variables listed in configuration.problems; run mcp-legifrance check-config.",
};

/// FR-105 : source locale, un instantané des dumps LEGI (index DILA, M9, V-180).
const LOCAL_SNAPSHOT_GAP: GapText = GapText {
    id: "FR-105",
    fond: None,
    missing: "Texts come from a local snapshot of the DILA LEGI dumps (laws, decrees and codes only), not from a live query: it may be out of date, and case law, CNIL deliberations and collective or company agreements are not in it.",
    fallback: "Treat an absence as unproven; unset LF_DILA_DIR to query Légifrance live.",
};

/// Famille d'un fond servi : le nom du fond, sans outil (hors du domaine). Interrogée en direct,
/// sauf depuis l'instantané local, qui n'a pas de date de capture rendue.
fn family_of(fond: Fond, local_snapshot: bool) -> CoverageFamily {
    CoverageFamily {
        name: fond.name().to_owned(),
        tool: String::new(),
        source: if local_snapshot { SNAPSHOT_SOURCE } else { SOURCE }.to_owned(),
        captured_at: None,
        live: !local_snapshot,
    }
}

/// Couverture déclarée du connecteur pour un environnement, une configuration et les fonds
/// servis (03 §4.8, 05 §5.8).
///
/// - **Statut** : `Ok` si les identifiants sont présents, qu'aucun problème n'est détecté et que
///   la source est PISTE en direct, `Degraded` sinon (l'instantané local ne couvre que LEGI et
///   peut être périmé) ; `Failed` n'est jamais produit en v0.x. Un hôte personnalisé est un
///   avertissement, pas un problème : il donne FR-103 sans changer le statut.
/// - **Familles** : une par fond de `served`, dans l'ordre reçu, sans doublon. Interrogées en
///   direct (`SOURCE`), sauf depuis l'instantané local (`SNAPSHOT_SOURCE`, `live: false`).
/// - **Note de date** : `AS_OF_NOTE`, ou `SNAPSHOT_AS_OF_NOTE` depuis l'instantané local.
/// - **Lacunes** : le catalogue FR-001 à FR-009, puis les lacunes calculées dans l'ordre FR-101
///   (bac à sable), FR-102 (identifiants absents), FR-103 (hôte personnalisé), FR-104 (problème
///   de configuration) et FR-105 (instantané local, LEGI seul). Jamais vide.
#[must_use]
pub fn coverage(env: PisteEnvironment, facts: &ConfigFacts, served: &[Fond]) -> Coverage {
    let healthy = facts.credentials_configured && !facts.has_problems && !facts.local_snapshot;
    let status = if healthy { CoverageStatus::Ok } else { CoverageStatus::Degraded };

    let mut families: Vec<CoverageFamily> = Vec::with_capacity(served.len());
    for &fond in served {
        if families.iter().all(|family| family.name != fond.name()) {
            families.push(family_of(fond, facts.local_snapshot));
        }
    }

    let computed = [
        (env == PisteEnvironment::Sandbox, &SANDBOX_GAP),
        (!facts.credentials_configured, &NO_CREDENTIALS_GAP),
        (facts.custom_host, &CUSTOM_HOST_GAP),
        (facts.has_problems, &INVALID_CONFIG_GAP),
        (facts.local_snapshot, &LOCAL_SNAPSHOT_GAP),
    ];
    let known_gaps = STATIC_GAPS
        .iter()
        .chain(computed.into_iter().filter_map(|(active, gap)| active.then_some(gap)))
        .map(GapText::to_gap)
        .collect();

    let as_of_note = if facts.local_snapshot { SNAPSHOT_AS_OF_NOTE } else { AS_OF_NOTE };

    // Construction directe : le catalogue statique n'est jamais vide (test `gaps_never_empty`).
    Coverage { status, as_of_note: as_of_note.to_owned(), families, known_gaps }
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
        let facts = HEALTHY;
        let copy = facts; // `facts` reste employable : le test cesse de compiler sans `Copy`
        assert_eq!(facts, copy);
        assert_ne!(facts, ConfigFacts { custom_host: true, ..facts });
        assert_ne!(facts, ConfigFacts { local_snapshot: true, ..facts });
        assert!(format!("{facts:?}").contains("credentials_configured: true"));
    }

    /// Configuration saine : identifiants présents, ni hôte personnalisé, ni problème, PISTE en
    /// direct.
    const HEALTHY: ConfigFacts = ConfigFacts {
        credentials_configured: true,
        custom_host: false,
        has_problems: false,
        local_snapshot: false,
    };

    /// Les seize combinaisons de faits de configuration.
    fn all_facts() -> Vec<ConfigFacts> {
        let flags = [false, true];
        flags
            .into_iter()
            .flat_map(|credentials_configured| {
                flags.into_iter().flat_map(move |custom_host| {
                    flags.into_iter().flat_map(move |has_problems| {
                        flags.into_iter().map(move |local_snapshot| ConfigFacts {
                            credentials_configured,
                            custom_host,
                            has_problems,
                            local_snapshot,
                        })
                    })
                })
            })
            .collect()
    }

    fn ids(c: &Coverage) -> Vec<&str> {
        c.known_gaps().iter().map(|gap| gap.id.as_str()).collect()
    }

    fn gap_of<'a>(c: &'a Coverage, id: &str) -> &'a CoverageGap {
        c.known_gaps().iter().find(|gap| gap.id == id).unwrap()
    }

    #[test]
    fn coverage_invariants_ported() {
        // Les invariants de la référence (`refuses_empty_gaps` et `exposes_its_parts` ci-dessus,
        // `coverage.py:1-8`) tiennent aussi pour la couverture calculée, quelle que soit l'entrée.
        assert_eq!(
            Coverage::new(CoverageStatus::Ok, "n".into(), vec![family()], vec![]),
            Err(DomainError::EmptyCoverageGaps)
        );
        let served_sets: [&[Fond]; 4] = [&[], &[Fond::Acco], &Fond::ALL, &[Fond::Loda, Fond::Loda]];
        for env in [PisteEnvironment::Sandbox, PisteEnvironment::Production] {
            for facts in all_facts() {
                for served in served_sets {
                    let c = coverage(env, &facts, served);
                    assert!(!c.known_gaps().is_empty(), "{env:?} {facts:?}");
                    let (note, source) = if facts.local_snapshot {
                        (SNAPSHOT_AS_OF_NOTE, SNAPSHOT_SOURCE)
                    } else {
                        (AS_OF_NOTE, SOURCE)
                    };
                    assert_eq!(c.as_of_note(), note);
                    // `Failed` est une énumération de parité, jamais produite en v0.x.
                    assert_ne!(c.status(), CoverageStatus::Failed);
                    // Jamais de date de capture ; en direct, sauf depuis l'instantané local.
                    assert!(c.families().iter().all(|f| f.captured_at.is_none()));
                    assert!(c.families().iter().all(|f| f.live != facts.local_snapshot));
                    assert!(c.families().iter().all(|f| f.source == source));
                    // Les identifiants de lacune sont uniques.
                    let unique: std::collections::HashSet<_> = ids(&c).into_iter().collect();
                    assert_eq!(unique.len(), c.known_gaps().len());
                    // Le résultat respecte le constructeur faillible livré.
                    assert_eq!(
                        Coverage::new(
                            c.status(),
                            c.as_of_note().to_owned(),
                            c.families().to_vec(),
                            c.known_gaps().to_vec()
                        ),
                        Ok(c.clone())
                    );
                }
            }
        }
    }

    #[test]
    fn as_of_note_is_the_reference_text() {
        assert_eq!(AS_OF_NOTE.chars().count(), 202);
        assert!(AS_OF_NOTE.starts_with("Data is queried live against the source"));
        assert!(AS_OF_NOTE.ends_with("is still in force today."));
        assert_eq!(SOURCE, "legifrance.gouv.fr via the PISTE API");
    }

    #[test]
    fn coverage_gaps_fr001_to_fr009() {
        // Production et configuration saine : seul le catalogue statique, dans l'ordre.
        let c = coverage(PisteEnvironment::Production, &HEALTHY, &Fond::ALL);
        assert_eq!(
            ids(&c),
            [
                "FR-001", "FR-002", "FR-003", "FR-004", "FR-005", "FR-006", "FR-007", "FR-008",
                "FR-009"
            ]
        );
        let table: Vec<(&str, &str, &str, &str)> = c
            .known_gaps()
            .iter()
            .map(|g| (g.id.as_str(), g.family.as_str(), g.missing.as_str(), g.fallback.as_str()))
            .collect();
        assert_eq!(
            table,
            [
                (
                    "FR-001",
                    "",
                    "The API returns no native ELI for legislation.",
                    "Cite source_url; never build an /eli/ path."
                ),
                (
                    "FR-002",
                    "",
                    "Legal force for a given case is not evaluated: texts are returned as published.",
                    "Check etat, validity dates and transitional provisions on the record itself."
                ),
                (
                    "FR-003",
                    "ACCO",
                    "The full text of company agreements is only a .docx attachment and is not returned.",
                    "Send the user to source_url."
                ),
                (
                    "FR-004",
                    "CETAT",
                    "Many CAA and TA decisions have no ECLI, and tribunal decisions are only partly published.",
                    "Cite citation and source_url; never invent an ECLI."
                ),
                (
                    "FR-005",
                    "JURI",
                    "Only a selection of courts of appeal and almost no first-instance decisions are on Légifrance; Judilibre is not integrated.",
                    "Search Judilibre (Cour de cassation open data) for other judicial decisions."
                ),
                (
                    "FR-006",
                    "JURI",
                    "Published decisions are pseudonymised (JURI) and CNIL texts redacted: party names are replaced.",
                    "Do not try to re-identify parties."
                ),
                (
                    "FR-007",
                    "",
                    "The Journal officiel (JORF) fond is not searchable yet.",
                    "Search LODA: consolidated texts carry their JO reference."
                ),
                (
                    "FR-008",
                    "",
                    "Search pages hold at most 100 hits and results beyond 10,000 are not reachable.",
                    "Narrow the query (match=all or exact, date_from/date_to)."
                ),
                (
                    "FR-009",
                    "",
                    "Not covered: EU law, CJEU and ECHR case law, doctrine, circulars, parliamentary documents.",
                    "Use EUR-Lex, HUDOC or the relevant official source."
                ),
            ]
        );
        // Le catalogue statique ne dépend ni des fonds servis ni de l'environnement.
        let nothing_served = coverage(PisteEnvironment::Production, &HEALTHY, &[]);
        assert_eq!(c.known_gaps(), nothing_served.known_gaps());
    }

    #[test]
    fn jorf_gap_stays_because_no_fond_serves_it() {
        // `Fond` n'a pas de variante JORF : même tous les fonds servis, FR-007 reste déclarée.
        let c = coverage(PisteEnvironment::Production, &HEALTHY, &Fond::ALL);
        assert!(gap_of(&c, "FR-007").missing.contains("Journal officiel"));
    }

    #[test]
    fn coverage_lists_pseudonymisation_gap() {
        let c = coverage(PisteEnvironment::Production, &HEALTHY, &Fond::ALL);
        let gap = gap_of(&c, "FR-006");
        assert!(gap.missing.contains("pseudonymised"));
        assert!(gap.missing.contains("CNIL texts redacted"));
        assert_eq!(gap.fallback, "Do not try to re-identify parties.");
    }

    #[test]
    fn coverage_degraded_without_credentials() {
        let without = ConfigFacts { credentials_configured: false, ..HEALTHY };
        let c = coverage(PisteEnvironment::Production, &without, &Fond::ALL);
        assert_eq!(c.status(), CoverageStatus::Degraded);
        assert_eq!(ids(&c).last(), Some(&"FR-102"));
        assert_eq!(
            gap_of(&c, "FR-102").missing,
            "PISTE credentials are not configured: every tool except fr_coverage fails with config_error."
        );
        assert_eq!(
            gap_of(&c, "FR-102").fallback,
            "Set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET."
        );
        // Avec identifiants et sans problème : `ok`, et FR-102 disparaît.
        let ok = coverage(PisteEnvironment::Production, &HEALTHY, &Fond::ALL);
        assert_eq!(ok.status(), CoverageStatus::Ok);
        assert!(!ids(&ok).contains(&"FR-102"));
    }

    #[test]
    fn coverage_degraded_on_config_problem() {
        // FR-104 : configuration invalide (mode dégradé, `EnvMismatch`), même avec identifiants.
        let facts = ConfigFacts { has_problems: true, ..HEALTHY };
        let c = coverage(PisteEnvironment::Production, &facts, &Fond::ALL);
        assert_eq!(c.status(), CoverageStatus::Degraded);
        assert_eq!(ids(&c).last(), Some(&"FR-104"));
        assert!(!ids(&c).contains(&"FR-102"));
        assert_eq!(
            gap_of(&c, "FR-104").fallback,
            "Fix the variables listed in configuration.problems; run mcp-legifrance check-config."
        );
    }

    #[test]
    fn coverage_custom_host_is_a_gap_not_a_problem() {
        // FR-103 : un avertissement, le statut ne change pas.
        let facts = ConfigFacts { custom_host: true, ..HEALTHY };
        let c = coverage(PisteEnvironment::Production, &facts, &Fond::ALL);
        assert_eq!(c.status(), CoverageStatus::Ok);
        assert_eq!(ids(&c).last(), Some(&"FR-103"));
        assert_eq!(gap_of(&c, "FR-103").fallback, "Remove the override unless it is intended.");
        assert!(gap_of(&c, "FR-103").missing.starts_with("A custom API host is configured"));
    }

    /// FR-105 : depuis l'index local DILA, la couverture ne dit plus « PISTE en direct ». La source
    /// et la note de date parlent d'un instantané, les familles ne sont plus en direct, une lacune
    /// « instantané local, LEGI seul » apparaît et le statut n'est jamais `Ok`.
    #[test]
    fn coverage_from_the_local_snapshot_is_not_presented_as_live() {
        let facts = ConfigFacts { local_snapshot: true, ..HEALTHY };
        let c = coverage(PisteEnvironment::Production, &facts, &[Fond::Loda, Fond::Code]);
        assert_eq!(c.status(), CoverageStatus::Degraded);
        assert_eq!(c.as_of_note(), SNAPSHOT_AS_OF_NOTE);
        assert!(!c.as_of_note().contains("queried live against the source"));
        assert!(c.as_of_note().ends_with("is still in force today."));
        assert_eq!(
            SNAPSHOT_SOURCE,
            "local snapshot of the DILA LEGI dumps (laws, decrees and codes), not queried live"
        );
        assert!(!SNAPSHOT_SOURCE.contains("PISTE"));
        assert!(!SNAPSHOT_AS_OF_NOTE.contains("PISTE"));
        let names: Vec<&str> = c.families().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["LODA", "CODE"]);
        assert!(c.families().iter().all(|f| !f.live && f.source == SNAPSHOT_SOURCE));
        assert_eq!(ids(&c).last(), Some(&"FR-105"));
        let gap = gap_of(&c, "FR-105");
        assert_eq!(gap.family, "");
        assert!(gap.missing.starts_with("Texts come from a local snapshot of the DILA LEGI dumps"));
        assert!(gap.missing.contains("it may be out of date"));
        assert_eq!(
            gap.fallback,
            "Treat an absence as unproven; unset LF_DILA_DIR to query Légifrance live."
        );
        // Sans instantané local, ni FR-105 ni changement de statut.
        let live = coverage(PisteEnvironment::Production, &HEALTHY, &[Fond::Loda, Fond::Code]);
        assert!(!ids(&live).contains(&"FR-105"));
        assert_eq!(live.status(), CoverageStatus::Ok);
        assert_eq!(live.as_of_note(), AS_OF_NOTE);
    }

    #[test]
    fn coverage_lists_sandbox_gap() {
        // FR-101 : la sandbox est un avertissement (D-18), le statut reste `ok`.
        let sandbox = coverage(PisteEnvironment::Sandbox, &HEALTHY, &Fond::ALL);
        assert_eq!(sandbox.status(), CoverageStatus::Ok);
        assert_eq!(ids(&sandbox).last(), Some(&"FR-101"));
        assert_eq!(ids(&sandbox).len(), 10);
        assert_eq!(
            gap_of(&sandbox, "FR-101").fallback,
            "Set LF_PISTE_ENV=production with production credentials."
        );
        let production = coverage(PisteEnvironment::Production, &HEALTHY, &Fond::ALL);
        assert!(!ids(&production).contains(&"FR-101"));
    }

    #[test]
    fn computed_gaps_follow_the_catalogue_in_id_order() {
        let everything = ConfigFacts {
            credentials_configured: false,
            custom_host: true,
            has_problems: true,
            local_snapshot: true,
        };
        let c = coverage(PisteEnvironment::Sandbox, &everything, &Fond::ALL);
        assert_eq!(&ids(&c)[9..], ["FR-101", "FR-102", "FR-103", "FR-104", "FR-105"]);
        assert_eq!(c.status(), CoverageStatus::Degraded);
    }

    #[test]
    fn every_served_fond_has_a_family() {
        let c = coverage(PisteEnvironment::Production, &HEALTHY, &Fond::ALL);
        let names: Vec<&str> = c.families().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"]);
        // Le domaine ne nomme pas les outils.
        assert!(c.families().iter().all(|f| f.tool.is_empty()));
        // Une famille par fond servi, dans l'ordre reçu et sans doublon.
        let some =
            coverage(PisteEnvironment::Production, &HEALTHY, &[Fond::Acco, Fond::Loda, Fond::Acco]);
        let names: Vec<&str> = some.families().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["ACCO", "LODA"]);
        // Aucun fond servi : aucune famille, mais toujours des lacunes.
        let none = coverage(PisteEnvironment::Production, &HEALTHY, &[]);
        assert_eq!(none.families().len(), 0);
    }

    #[test]
    fn gaps_never_empty() {
        for env in [PisteEnvironment::Sandbox, PisteEnvironment::Production] {
            for facts in all_facts() {
                assert!(coverage(env, &facts, &[]).known_gaps().len() >= STATIC_GAPS.len());
            }
        }
        assert_eq!(STATIC_GAPS.len(), 9);
    }

    #[test]
    fn status_follows_credentials_problems_and_the_local_snapshot_only() {
        for env in [PisteEnvironment::Sandbox, PisteEnvironment::Production] {
            for facts in all_facts() {
                let wanted =
                    if facts.credentials_configured && !facts.has_problems && !facts.local_snapshot
                    {
                        CoverageStatus::Ok
                    } else {
                        CoverageStatus::Degraded
                    };
                assert_eq!(coverage(env, &facts, &Fond::ALL).status(), wanted, "{env:?} {facts:?}");
            }
        }
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
