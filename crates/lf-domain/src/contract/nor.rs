//! `Nor` : numéro NOR validé, en majuscules (03 §4.2).
//!
//! Un NOR compte 12 caractères : quatre lettres, sept chiffres, une lettre
//! (`ASET2450455M`). L'API de recherche le renvoie parfois en minuscules ;
//! [`Nor::parse`] rogne, passe en majuscules ASCII et valide la forme. Tout le
//! reste donne `None` : pas de variante d'erreur, le mapper écarte un NOR
//! invalide avec un diagnostic.

use std::fmt;

/// NOR valide, en majuscules : `[A-Z]{4}[0-9]{7}[A-Z]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Nor(String);

impl Nor {
    /// Valide `raw` (rogné, mis en majuscules ASCII) ; `None` si ce n'est pas un NOR.
    ///
    /// ```
    /// use lf_domain::contract::Nor;
    ///
    /// assert_eq!(Nor::parse("aset2450455m").map(|n| n.to_string()), Some("ASET2450455M".to_owned()));
    /// assert_eq!(Nor::parse("ASET245045M"), None);
    /// ```
    pub fn parse(raw: &str) -> Option<Self> {
        let upper = raw.trim().to_ascii_uppercase();
        is_nor_shape(&upper).then_some(Self(upper))
    }

    /// Forme canonique, en majuscules.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Nor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Vrai si `s` est exactement quatre lettres majuscules ASCII, sept chiffres ASCII, une lettre
/// majuscule ASCII.
fn is_nor_shape(s: &str) -> bool {
    matches!(
        s.as_bytes(),
        [l0, l1, l2, l3, d0, d1, d2, d3, d4, d5, d6, l4]
            if [l0, l1, l2, l3, l4].into_iter().all(u8::is_ascii_uppercase)
                && [d0, d1, d2, d3, d4, d5, d6].into_iter().all(u8::is_ascii_digit)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn nor(raw: &str) -> Option<String> {
        Nor::parse(raw).map(|n| n.as_str().to_owned())
    }

    #[test]
    fn nor_uppercases_and_validates() {
        assert_eq!(nor("cscl2115694s"), Some("CSCL2115694S".to_owned()));
        assert_eq!(nor("  cscl2115694S \n"), Some("CSCL2115694S".to_owned()));
        for ok in ["ASET2450455M", "ECFI1524250L", "CSCX2531180S"] {
            assert_eq!(nor(ok), Some(ok.to_owned()));
        }
        let parsed = Nor::parse("cscl2115694s").unwrap();
        assert_eq!(parsed.to_string(), "CSCL2115694S");
    }

    #[test]
    fn nor_rejects_malformed() {
        for bad in [
            "CSCL211569S",         // 11 caractères
            "CSCL21156944S",       // 13 caractères
            "1SCL2115694S",        // chiffre en tête
            "CS1L2115694S",        // chiffre dans les lettres de tête
            "CSCL2115A94S",        // lettre dans les chiffres
            "CSCL21156944",        // chiffre en queue
            "CSCL2115694",         // sans lettre finale, 11 caractères
            "",                    // vide
            "   ",                 // blanc
            "CSCL 115694S",        // espace interne
            "CSCL2115694É",        // lettre non ASCII en queue
            "ÇSCL2115694S",        // lettre non ASCII en tête
            "CSCL２１１５６９４S", // chiffres pleine chasse
        ] {
            assert_eq!(nor(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn is_nor_shape_checks_every_position() {
        assert!(is_nor_shape("CSCL2115694S"));
        // Chaque position fautive, une à la fois.
        for i in 0..12 {
            let mut bytes = *b"CSCL2115694S";
            bytes[i] = if (4..11).contains(&i) { b'X' } else { b'7' };
            let s = std::str::from_utf8(&bytes).unwrap();
            assert!(!is_nor_shape(s), "{s}");
        }
        // Minuscules refusées : la mise en majuscules est faite par `parse`.
        assert!(!is_nor_shape("cscl2115694s"));
        assert!(!is_nor_shape("CSCL2115694s"));
        assert!(!is_nor_shape("CSCL2115694SS"));
        assert!(!is_nor_shape("CSCL2115694"));
        assert!(!is_nor_shape(""));
    }

    proptest! {
        #[test]
        fn nor_roundtrips(raw in "[A-Z]{4}[0-9]{7}[A-Z]") {
            let parsed = Nor::parse(&raw).unwrap();
            prop_assert_eq!(parsed.as_str(), raw.as_str());
            prop_assert_eq!(Nor::parse(&parsed.to_string()), Some(parsed));
        }

        #[test]
        fn lowercase_input_gives_the_same_nor(raw in "[A-Z]{4}[0-9]{7}[A-Z]") {
            prop_assert_eq!(Nor::parse(&raw.to_lowercase()), Nor::parse(&raw));
        }

        #[test]
        fn parse_never_panics(s in "\\PC{0,40}") {
            let _ = Nor::parse(&s);
        }
    }
}
