//! Fonds Légifrance interrogeables (`server.py:91-100`).

use std::{fmt, str::FromStr};

use crate::error::{DomainError, echo};

/// Fond exposé à l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Lit un nom de fond, sans tenir compte de la casse ni des espaces.
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
