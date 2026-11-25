//! Numéro d'une fiche ou d'une page de navigation de Service-Public (ADR-0033, décision 6).
//!
//! Mesure du 2026-11-23 sur l'archive du jour : les fiches s'écrivent `F` suivi de chiffres
//! (`F1437`, `F37954`), les dossiers et thèmes `N` suivi de chiffres (`N31675`, `N19804`), les
//! ressources `R` suivi de chiffres. Les ressources ne sont pas servies (décision 13) : leur
//! numéro est refusé avant toute lecture.

use std::fmt;

use crate::error::{DomainError, echo};

/// Nombre maximal de chiffres d'un numéro (05 §5.17).
pub const SHEET_ID_MAX_DIGITS: usize = 7;

/// Numéro d'une fiche (`F1437`) ou d'une page de navigation, dossier ou thème (`N31675`) :
/// `F` ou `N`, puis 1 à [`SHEET_ID_MAX_DIGITS`] chiffres ASCII.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SheetId(String);

impl SheetId {
    /// Valide `input` : espaces autour retirés, lettres ASCII mises en majuscules avant le
    /// contrôle.
    ///
    /// ```
    /// use lf_domain::guidance::SheetId;
    ///
    /// assert_eq!(SheetId::parse(" f1437 ")?.as_str(), "F1437");
    /// assert!(SheetId::parse("N31675")?.is_navigation());
    /// assert!(SheetId::parse("R42").is_err());
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidSheetId`] si l'entrée ne commence pas par `F` ou `N`, ou si ce qui
    /// suit n'est pas fait de 1 à [`SHEET_ID_MAX_DIGITS`] chiffres ASCII.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        let valid = match trimmed.as_bytes().split_first() {
            Some((prefix, digits)) => {
                matches!(prefix.to_ascii_uppercase(), b'F' | b'N')
                    && (1..=SHEET_ID_MAX_DIGITS).contains(&digits.len())
                    && digits.iter().all(u8::is_ascii_digit)
            }
            None => false,
        };
        if valid {
            Ok(Self(trimmed.to_ascii_uppercase()))
        } else {
            Err(DomainError::InvalidSheetId { input: echo(input) })
        }
    }

    /// Forme normalisée, ex. `F1437`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Le nombre écrit après la lettre (`1437` pour `F1437`) : à score égal, les résultats d'une
    /// recherche suivent l'ordre croissant de ce nombre (05 §5.16).
    pub fn number(&self) -> u32 {
        // Sept chiffres au plus : la valeur tient dans un `u32`, sans débordement possible.
        self.0.bytes().skip(1).fold(0, |number: u32, digit| {
            number.saturating_mul(10).saturating_add(u32::from(digit.saturating_sub(b'0')))
        })
    }

    /// Vrai pour une page de navigation (`N…` : dossier ou thème), faux pour une fiche (`F…`).
    pub fn is_navigation(&self) -> bool {
        self.0.starts_with('N')
    }
}

impl fmt::Display for SheetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn id(input: &str) -> String {
        SheetId::parse(input).unwrap().as_str().to_owned()
    }

    #[test]
    fn sheet_id_accepts_only_f_or_n_and_seven_digits() {
        for (input, expected) in [
            ("F1437", "F1437"),
            ("f1437", "F1437"),
            ("  n31675\t", "N31675"),
            ("F1", "F1"),
            ("N9999999", "N9999999"),
            ("F0001", "F0001"),
        ] {
            assert_eq!(id(input), expected);
        }
        for input in [
            "",
            "   ",
            "F",
            "N",
            "F12345678",
            "R42",
            "r42",
            "X1",
            "1437",
            "F14 37",
            "F-1437",
            "F1437.xml",
            "FF1437",
            "F１４３７",
            "Ｆ1437",
            "F\u{0}1",
            "../F1",
        ] {
            assert_eq!(
                SheetId::parse(input),
                Err(DomainError::InvalidSheetId { input: echo(input) }),
                "{input:?}"
            );
        }
    }

    #[test]
    fn sheets_and_navigation_pages_are_told_apart() {
        assert!(!SheetId::parse("F1437").unwrap().is_navigation());
        assert!(SheetId::parse("n31675").unwrap().is_navigation());
    }

    #[test]
    fn number_is_the_value_of_the_digits() {
        for (input, number) in [("F1437", 1437), ("n0042", 42), ("F9999999", 9_999_999), ("F0", 0)]
        {
            assert_eq!(SheetId::parse(input).unwrap().number(), number);
        }
    }

    #[test]
    fn displays_its_normal_form() {
        assert_eq!(SheetId::parse(" f468 ").unwrap().to_string(), "F468");
    }

    #[test]
    fn a_long_refused_input_is_echoed_truncated() {
        let input = format!("F{}", "1".repeat(500));
        assert_eq!(
            SheetId::parse(&input),
            Err(DomainError::InvalidSheetId { input: echo(&input) })
        );
        assert!(echo(&input).chars().count() < 100);
    }

    proptest! {
        /// Ce qui est accepté est exactement `F` ou `N` suivi de 1 à 7 chiffres ASCII, et se
        /// relit à l'identique.
        #[test]
        fn accepted_ids_have_the_strict_form(
            input in prop_oneof!["\\PC{0,12}", "[ \t]{0,2}[FfNnRr][0-9]{0,9}[ \t]{0,2}"],
        ) {
            if let Ok(id) = SheetId::parse(&input) {
                let text = id.as_str();
                let (prefix, digits) = text.split_at(1);
                prop_assert!(prefix == "F" || prefix == "N", "{text:?}");
                prop_assert!((1..=SHEET_ID_MAX_DIGITS).contains(&digits.len()));
                prop_assert!(digits.bytes().all(|byte| byte.is_ascii_digit()));
                prop_assert_eq!(SheetId::parse(text), Ok(id.clone()));
            }
        }

        /// Toute forme valide est acceptée, quelle que soit la casse.
        #[test]
        fn every_valid_form_is_accepted(prefix in "[FfNn]", digits in "[0-9]{1,7}") {
            let input = format!("{prefix}{digits}");
            prop_assert_eq!(id(&input), input.to_ascii_uppercase());
        }
    }
}
