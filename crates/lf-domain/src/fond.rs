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
