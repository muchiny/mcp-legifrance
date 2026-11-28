//! Identifiant d'une décision de Judilibre (ADR-0035, décision 11).
//!
//! Mesure du 2026-11-27 : plus de quarante décisions vues, toutes d'identifiant à 24 caractères
//! hexadécimaux minuscules (`5fca624b7eeb384a40cafb85`), la forme d'un `ObjectId` de la base de
//! la Cour de cassation.

use std::fmt;

use crate::error::{DomainError, echo};

/// Nombre de caractères d'un identifiant de Judilibre.
pub const JUDILIBRE_ID_CHARS: usize = 24;

/// Identifiant d'une décision de Judilibre : 24 caractères hexadécimaux, en minuscules.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JudilibreId(String);

impl JudilibreId {
    /// Valide `input` : espaces autour retirés, lettres ASCII mises en minuscules avant le
    /// contrôle.
    ///
    /// ```
    /// use lf_domain::judilibre::JudilibreId;
    ///
    /// let id = JudilibreId::parse(" 5FCA624B7EEB384A40CAFB85 ")?;
    /// assert_eq!(id.as_str(), "5fca624b7eeb384a40cafb85");
    /// assert!(JudilibreId::parse("JURITEXT000045653019").is_err());
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidJudilibreId`] si l'entrée n'est pas faite d'exactement
    /// [`JUDILIBRE_ID_CHARS`] chiffres hexadécimaux ASCII.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        let valid = trimmed.len() == JUDILIBRE_ID_CHARS
            && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit());
        if valid {
            Ok(Self(trimmed.to_ascii_lowercase()))
        } else {
            Err(DomainError::InvalidJudilibreId { input: echo(input) })
        }
    }

    /// Forme normalisée, en minuscules.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JudilibreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn judilibre_id_is_24_hex() {
        for (raw, kept) in [
            ("5fca624b7eeb384a40cafb85", "5fca624b7eeb384a40cafb85"),
            ("63C8EEFDDC5B777C90992FD8", "63c8eefddc5b777c90992fd8"),
            ("\t6253cd8cbd3db21cbdd93a2f\n", "6253cd8cbd3db21cbdd93a2f"),
            ("000000000000000000000000", "000000000000000000000000"),
        ] {
            assert_eq!(JudilibreId::parse(raw).map(|id| id.as_str().to_owned()), Ok(kept.into()));
        }
        for raw in [
            "",
            "5fca624b7eeb384a40cafb8",
            "5fca624b7eeb384a40cafb851",
            "5fca624b7eeb384a40cafb8g",
            "5fca624b 7eeb384a40cafb8",
            "JURITEXT000045653019",
            "ｆfca624b7eeb384a40cafb8",
        ] {
            assert_eq!(
                JudilibreId::parse(raw),
                Err(DomainError::InvalidJudilibreId { input: echo(raw) })
            );
        }
    }

    #[test]
    fn display_is_the_normalised_form() {
        let id = JudilibreId::parse("5FCA624B7EEB384A40CAFB85").unwrap();
        assert_eq!(id.to_string(), "5fca624b7eeb384a40cafb85");
    }

    proptest! {
        /// Toute suite de 24 chiffres hexadécimaux passe, en minuscules ; toute autre longueur
        /// est refusée.
        #[test]
        fn any_24_hex_digits_pass(raw in "[0-9a-fA-F]{24}", other in "[0-9a-f]{0,23}|[0-9a-f]{25,40}") {
            let id = JudilibreId::parse(&raw).unwrap();
            prop_assert_eq!(id.as_str(), raw.to_ascii_lowercase());
            prop_assert!(JudilibreId::parse(&other).is_err());
        }
    }
}
