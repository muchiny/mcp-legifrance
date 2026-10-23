//! `Idcc` : identifiant de convention collective, de 1 à 9999 (03 §4.6).
//!
//! L'API le sert sous forme de chaîne (`"275"`, `"0000000000002121"`) ou de nombre.
//! [`Idcc::parse`] accepte des chiffres ASCII seulement, ignore les zéros de tête et
//! refuse 0 et tout ce qui dépasse 9999. La borne supérieure est une hypothèse
//! (**NON VÉRIFIÉ**, recherche 07 §4.12, V-132) : les numéros connus tiennent sur quatre
//! chiffres. `Display` n'affiche pas de zéros de tête.

use std::fmt;

/// Plus grand IDCC accepté (quatre chiffres significatifs).
const IDCC_MAX_DIGITS: usize = 4;

/// Identifiant de convention collective, de 1 à 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Idcc(u16);

impl Idcc {
    /// Lit un IDCC : `trim`, chiffres ASCII uniquement, zéros de tête ignorés, valeur 1..=9999.
    ///
    /// ```
    /// use lf_domain::contract::Idcc;
    ///
    /// assert_eq!(Idcc::parse("0000000000002121").map(Idcc::get), Some(2121));
    /// assert_eq!(Idcc::parse("0275").map(|i| i.to_string()), Some("275".to_owned()));
    /// assert_eq!(Idcc::parse("10000"), None);
    /// ```
    pub fn parse(raw: &str) -> Option<Self> {
        idcc_value(raw.trim()).map(Self)
    }

    /// Valeur numérique, de 1 à 9999.
    pub fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for Idcc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Valeur de `digits` (déjà rogné) : uniquement des chiffres ASCII, au moins un chiffre
/// significatif, au plus [`IDCC_MAX_DIGITS`] après les zéros de tête.
fn idcc_value(digits: &str) -> Option<u16> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let significant = digits.trim_start_matches('0');
    if significant.len() > IDCC_MAX_DIGITS {
        return None;
    }
    // Vide (« 0 », « 0000 ») : l'analyse échoue, donc `None`.
    significant.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn idcc(raw: &str) -> Option<u16> {
        Idcc::parse(raw).map(Idcc::get)
    }

    #[test]
    fn idcc_strips_leading_zeros() {
        assert_eq!(idcc("0000000000002121"), Some(2121));
        assert_eq!(idcc("275"), Some(275));
        assert_eq!(idcc("0275"), Some(275));
        assert_eq!(idcc(" 0275 "), Some(275));
        assert_eq!(idcc("1"), Some(1));
        assert_eq!(idcc("9999"), Some(9999));
        assert_eq!(Idcc::parse("0275").unwrap().to_string(), "275");
        assert_eq!(Idcc::parse("2121").unwrap().to_string(), "2121");
    }

    #[test]
    fn idcc_rejects_out_of_range_and_garbage() {
        for bad in [
            "0",
            "0000",
            "10000",
            "12a",
            "-5",
            "+5",
            "",
            "  ",
            "2 121",
            "２１２１",
            "99999999999999999999999999",
        ] {
            assert_eq!(idcc(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn idcc_value_decides_on_digits_only() {
        assert_eq!(idcc_value("2121"), Some(2121));
        assert_eq!(idcc_value("0001"), Some(1));
        assert_eq!(idcc_value("00009999"), Some(9999));
        assert_eq!(idcc_value("10000"), None);
        assert_eq!(idcc_value("0"), None);
        assert_eq!(idcc_value(""), None);
        // Pas de rognage dans la fonction de décision : c'est `parse` qui rogne.
        assert_eq!(idcc_value(" 275"), None);
        assert_eq!(idcc_value("27a"), None);
        assert_eq!(idcc_value("２１２１"), None);
    }

    proptest! {
        #[test]
        fn idcc_roundtrips(value in 1u16..=9999) {
            let parsed = Idcc::parse(&value.to_string()).unwrap();
            prop_assert_eq!(parsed.get(), value);
            prop_assert_eq!(Idcc::parse(&parsed.to_string()), Some(parsed));
            prop_assert_eq!(Idcc::parse(&format!("{value:016}")), Some(parsed));
        }

        #[test]
        fn out_of_range_is_rejected(value in 10_000u64..=u64::MAX) {
            prop_assert_eq!(Idcc::parse(&value.to_string()), None);
        }

        #[test]
        fn parse_never_panics(s in "\\PC{0,40}") {
            let _ = Idcc::parse(&s);
        }
    }
}
