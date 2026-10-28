//! Fonds Légifrance interrogeables (`server.py:91-100`).

use std::{fmt, str::FromStr};

use crate::error::{DomainError, echo};

/// Fond exposé à l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fond {
    /// Lois et décrets.
    Loda,
    /// Codes.
    Code,
    /// Cour de cassation, cours d'appel.
    Juri,
    /// Conseil constitutionnel.
    Constit,
    /// Conseil d'État, CAA, TA.
    Cetat,
    /// Délibérations CNIL.
    Cnil,
    /// Conventions collectives.
    Kali,
    /// Accords d'entreprise.
    Acco,
}

impl Fond {
    /// Tous les fonds, dans l'ordre de la référence.
    pub const ALL: [Self; 8] = [
        Self::Loda,
        Self::Code,
        Self::Juri,
        Self::Constit,
        Self::Cetat,
        Self::Cnil,
        Self::Kali,
        Self::Acco,
    ];

    /// Nom côté utilisateur, ex. `LODA`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Loda => "LODA",
            Self::Code => "CODE",
            Self::Juri => "JURI",
            Self::Constit => "CONSTIT",
            Self::Cetat => "CETAT",
            Self::Cnil => "CNIL",
            Self::Kali => "KALI",
            Self::Acco => "ACCO",
        }
    }

    /// Nom côté API PISTE, ex. `LODA_DATE`.
    pub fn api_name(self) -> &'static str {
        match self {
            Self::Loda => "LODA_DATE",
            Self::Code => "CODE_DATE",
            other => other.name(),
        }
    }

    /// Vrai si le fond se consulte à une date de version (`version_date`) : `LODA` et `CODE`.
    pub const fn needs_version_date(self) -> bool {
        matches!(self, Self::Loda | Self::Code)
    }

    /// Vrai si le fond accepte un filtre de période (`date_from`, `date_to`) : tous sauf `CODE`.
    pub const fn has_date_filter(self) -> bool {
        !matches!(self, Self::Code)
    }

    /// Préfixe des identifiants de résultats de recherche de ce fond, ex. `LEGITEXT` pour `LODA`
    /// et `CODE`.
    pub const fn hit_prefix(self) -> &'static str {
        match self {
            Self::Loda | Self::Code => "LEGITEXT",
            Self::Juri => "JURITEXT",
            Self::Constit => "CONSTEXT",
            Self::Cetat => "CETATEXT",
            Self::Cnil => "CNILTEXT",
            Self::Kali => "KALITEXT",
            Self::Acco => "ACCOTEXT",
        }
    }

    /// Lit un nom de fond, sans tenir compte de la casse ni des espaces
    /// (parité avec `(fond or "").strip().upper()`, `server.py:191`).
    ///
    /// # Errors
    ///
    /// [`DomainError::UnknownFond`] si le nom ne correspond à aucun fond.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let key = input.trim().to_ascii_uppercase();
        Self::ALL
            .into_iter()
            .find(|fond| fond.name() == key)
            .ok_or_else(|| DomainError::UnknownFond { input: echo(input) })
    }
}

/// Mode de pagination de l'API de recherche : valeur opaque de `typePagination` (04 §6.6).
///
/// Jamais exposé au modèle (00 §7). Le mode reçu peut différer du mode demandé, et c'est lui que
/// l'on retient pour les pages suivantes : la fixture de référence montre `CODE_DATE` répondant
/// `ARTICLE` à une requête `DEFAUT`, alors qu'en sandbox (2026-10-28, V-82) il répond `DEFAUT` à
/// une page 1 en `DEFAUT`, et qu'une page 2 envoyée en `ARTICLE` y rejoue la page 1. Sans mémo,
/// une page > 1 repart donc en `DEFAUT` pour tous les fonds (04 §6.6).
/// Placé à côté de [`Fond`] parce que c'est un nom d'API de la même famille.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PaginationMode {
    /// Pagination par texte (`DEFAUT`).
    #[default]
    Defaut,
    /// Pagination par article (`ARTICLE`).
    Article,
}

impl PaginationMode {
    /// Nom côté API PISTE, ex. `DEFAUT`.
    pub const fn api_name(self) -> &'static str {
        match self {
            Self::Defaut => "DEFAUT",
            Self::Article => "ARTICLE",
        }
    }

    /// Lit la valeur `typePagination` d'une réponse, sans tenir compte de la casse ASCII
    /// ni des espaces autour ; `None` si elle est vide ou inconnue.
    ///
    /// Le domaine ne journalise pas : l'adapter journalise puis retombe sur
    /// [`PaginationMode::default`].
    pub fn from_api(raw: &str) -> Option<Self> {
        let key = raw.trim();
        [Self::Defaut, Self::Article]
            .into_iter()
            .find(|mode| mode.api_name().eq_ignore_ascii_case(key))
    }
}

impl fmt::Display for Fond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Fond {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::HashSet;

    #[test]
    fn hash_agrees_with_eq() {
        let read = ["loda", " LODA ", "CODE"].map(|s| Fond::parse(s).unwrap());
        assert_eq!(HashSet::from(read), HashSet::from([Fond::Loda, Fond::Code]));
    }

    #[test]
    fn names_match_reference() {
        let names: Vec<_> = Fond::ALL.iter().map(|f| f.name()).collect();
        assert_eq!(names, ["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"]);
    }

    #[test]
    fn api_names_match_reference() {
        let api: Vec<_> = Fond::ALL.iter().map(|f| f.api_name()).collect();
        assert_eq!(
            api,
            ["LODA_DATE", "CODE_DATE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"]
        );
    }

    #[test]
    fn version_date_and_date_filter_by_fond() {
        let table: Vec<_> = Fond::ALL
            .iter()
            .map(|f| (f.name(), f.needs_version_date(), f.has_date_filter()))
            .collect();
        assert_eq!(
            table,
            [
                ("LODA", true, true),
                ("CODE", true, false),
                ("JURI", false, true),
                ("CONSTIT", false, true),
                ("CETAT", false, true),
                ("CNIL", false, true),
                ("KALI", false, true),
                ("ACCO", false, true),
            ]
        );
    }

    #[test]
    fn hit_prefix_per_fond() {
        let table: Vec<_> = Fond::ALL.iter().map(|f| (f.name(), f.hit_prefix())).collect();
        assert_eq!(
            table,
            [
                ("LODA", "LEGITEXT"),
                ("CODE", "LEGITEXT"),
                ("JURI", "JURITEXT"),
                ("CONSTIT", "CONSTEXT"),
                ("CETAT", "CETATEXT"),
                ("CNIL", "CNILTEXT"),
                ("KALI", "KALITEXT"),
                ("ACCO", "ACCOTEXT"),
            ]
        );
    }

    #[test]
    fn parse_is_case_and_space_insensitive() {
        for fond in Fond::ALL {
            assert_eq!(Fond::parse(fond.name()).unwrap(), fond);
            assert_eq!(format!(" {} ", fond.name().to_lowercase()).parse::<Fond>().unwrap(), fond);
        }
    }

    #[test]
    fn parse_rejects_api_names_and_unknowns() {
        assert_eq!(
            Fond::parse("LODA_DATE").unwrap_err(),
            DomainError::UnknownFond { input: "LODA_DATE".into() }
        );
        assert!(Fond::parse("").is_err());
    }

    #[test]
    fn display_is_user_name() {
        assert_eq!(Fond::Constit.to_string(), "CONSTIT");
    }

    #[test]
    fn pagination_api_names_match_swagger() {
        assert_eq!(PaginationMode::Defaut.api_name(), "DEFAUT");
        assert_eq!(PaginationMode::Article.api_name(), "ARTICLE");
    }

    #[test]
    fn pagination_from_api_reads_both_values() {
        assert_eq!(PaginationMode::from_api("DEFAUT"), Some(PaginationMode::Defaut));
        assert_eq!(PaginationMode::from_api("ARTICLE"), Some(PaginationMode::Article));
    }

    #[test]
    fn pagination_from_api_ignores_case_and_surrounding_spaces() {
        assert_eq!(PaginationMode::from_api("article"), Some(PaginationMode::Article));
        assert_eq!(PaginationMode::from_api(" Defaut\t"), Some(PaginationMode::Defaut));
    }

    #[test]
    fn pagination_from_api_flags_unknown_and_empty() {
        for raw in ["", "  ", "PAGE", "DEFAUT_X", "ARTICLES", "DÉFAUT"] {
            assert_eq!(PaginationMode::from_api(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn pagination_defaults_to_defaut() {
        assert_eq!(PaginationMode::default(), PaginationMode::Defaut);
    }

    #[test]
    fn pagination_hash_agrees_with_eq() {
        let read = ["defaut", "DEFAUT", "article"].map(|s| PaginationMode::from_api(s).unwrap());
        assert_eq!(
            HashSet::from(read),
            HashSet::from([PaginationMode::Defaut, PaginationMode::Article])
        );
    }

    proptest! {
        #[test]
        fn parse_never_panics(s in "\\PC*") {
            let _ = Fond::parse(&s);
        }

        #[test]
        fn parse_round_trips(
            fond in proptest::sample::select(Fond::ALL.to_vec()),
            lower in any::<bool>(),
        ) {
            let name = if lower { fond.name().to_lowercase() } else { fond.name().to_owned() };
            prop_assert_eq!(Fond::parse(&name), Ok(fond));
        }
    }
}
