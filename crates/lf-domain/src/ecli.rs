//! European Case Law Identifier, tel que renvoyé par PISTE — jamais fabriqué.
//!
//! Règle nouvelle, absente de la référence, qui stocke l'ECLI brut
//! (`models.py:177`, `server.py:48`).
//!
//! Forme : `ECLI:FR:<juridiction 1-7>:<année 4>:<numéro 1-25>` (conclusions du
//! Conseil de l'UE 2011/C 127/01, annexe). Exemples des fixtures :
//! `ECLI:FR:CC:2025:2025.1173.QPC`, `ECLI:FR:CCASS:2025:C100399`,
//! `ECLI:FR:CECHR:2026:506507.20260529`.

use std::fmt;

use crate::error::{DomainError, echo};

/// ECLI français validé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ecli(String);

fn court_ok(court: &str) -> bool {
    (1..=7).contains(&court.len())
        && court.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn year_ok(year: &str) -> bool {
    year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit())
}

fn ordinal_ok(ordinal: &str) -> bool {
    (1..=25).contains(&ordinal.len())
        && ordinal.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

impl Ecli {
    /// Valide `input` (blancs Unicode autour tolérés, `str::trim`).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidEcli`] si la forme ne convient pas.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        let mut parts = trimmed.split(':');
        let valid = parts.next() == Some("ECLI")
            && parts.next() == Some("FR")
            && parts.next().is_some_and(court_ok)
            && parts.next().is_some_and(year_ok)
            && parts.next().is_some_and(ordinal_ok)
            && parts.next().is_none();
        if valid {
            Ok(Self(trimmed.to_owned()))
        } else {
            Err(DomainError::InvalidEcli { input: echo(input) })
        }
    }

    /// Forme textuelle.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ecli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn accepts_fixture_eclis() {
        for ecli in [
            "ECLI:FR:CC:2025:2025.1173.QPC",
            "ECLI:FR:CCASS:2025:C100399",
            "ECLI:FR:CECHR:2026:506507.20260529",
        ] {
            assert_eq!(Ecli::parse(ecli).unwrap().as_str(), ecli);
        }
        assert_eq!(Ecli::parse(" ECLI:FR:CC:2025:1 ").unwrap().to_string(), "ECLI:FR:CC:2025:1");
    }

    #[test]
    fn length_bounds() {
        assert!(Ecli::parse("ECLI:FR:ABCDEF7:2025:1").is_ok());
        assert!(Ecli::parse("ECLI:FR:ABCDEFG8:2025:1").is_err());
        assert!(Ecli::parse(&format!("ECLI:FR:CC:2025:{}", "1".repeat(25))).is_ok());
        assert!(Ecli::parse(&format!("ECLI:FR:CC:2025:{}", "1".repeat(26))).is_err());
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "ECLI",
            "ecli:FR:CC:2025:1",
            "ECLI:BE:CC:2025:1",
            "ECLI:FR::2025:1",
            "ECLI:FR:cc:2025:1",
            "ECLI:FR:C-C:2025:1",
            "ECLI:FR:CC:25:1",
            "ECLI:FR:CC:20255:1",
            "ECLI:FR:CC:2O25:1",
            "ECLI:FR:CC:2025:",
            "ECLI:FR:CC:2025:1-2",
            "ECLI:FR:CC:2025:1:extra",
            "ECLI:FR:CC:2025",
        ] {
            assert_eq!(
                Ecli::parse(bad).unwrap_err(),
                DomainError::InvalidEcli { input: bad.into() },
                "{bad:?}"
            );
        }
    }

    proptest! {
        #[test]
        fn parse_never_panics(s in "\\PC*") {
            let _ = Ecli::parse(&s);
        }
    }
}
