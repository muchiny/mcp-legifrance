//! Identifiants du BOFiP : identifiant juridique, numéro de publication, série.
//!
//! Mesure du 2026-11-15 sur les 9 173 publications du jeu `bofip-vigueur` : l'identifiant le plus
//! long fait 26 caractères, tous sont faits de majuscules, de chiffres et de traits d'union ;
//! 23 identifiants désignent deux publications, que seul le numéro de publication départage
//! (ADR-0029, décision 1) ; 30 codes de série, de `AIS` à `TVA`.

use std::fmt;

use crate::error::{DomainError, echo};

/// Longueur maximale d'un identifiant du BOFiP, en octets (05 §5.15).
pub const BOI_ID_MAX_BYTES: usize = 64;

/// Longueur maximale d'un code de série, en octets (05 §5.14).
pub const SERIE_MAX_BYTES: usize = 16;

/// Identifiant juridique d'une publication du BOFiP, ex. `BOI-ENR-DMTG-10-50-20` ou
/// `ACTU-2023-00012` : majuscules ASCII, chiffres et traits d'union, [`BOI_ID_MAX_BYTES`] octets
/// au plus.
///
/// Un identifiant peut désigner deux publications : il ne suffit pas toujours à en lire une
/// ([`PublicationId`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BoiId(String);

impl BoiId {
    /// Valide `input` : espaces autour retirés, lettres ASCII mises en majuscules.
    ///
    /// Seules les lettres ASCII changent de casse. Un repli Unicode (`ß` en `SS`, `ı` en `I`)
    /// ferait accepter une entrée qui n'est pas l'identifiant écrit : elle est refusée.
    ///
    /// ```
    /// use lf_domain::doctrine::BoiId;
    ///
    /// let id = BoiId::parse(" boi-enr-dmtg-10-50-20 ")?;
    /// assert_eq!(id.as_str(), "BOI-ENR-DMTG-10-50-20");
    /// assert!(BoiId::parse("BOI-ENR\"").is_err());
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidBoiId`] si l'entrée est vide, dépasse [`BOI_ID_MAX_BYTES`] octets ou
    /// porte un autre caractère qu'une lettre ASCII, un chiffre ou un trait d'union.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        ascii_upper(input, BOI_ID_MAX_BYTES, is_id_byte)
            .map(Self)
            .ok_or_else(|| DomainError::InvalidBoiId { input: echo(input) })
    }

    /// Forme normalisée, ex. `BOI-ENR-DMTG-10-50-20`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BoiId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Octet admis dans un identifiant, avant la mise en majuscules.
fn is_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

/// Code de série du BOFiP, ex. `ENR` (enregistrement) ou `IR` (impôt sur le revenu) : majuscules
/// ASCII et chiffres, [`SERIE_MAX_BYTES`] octets au plus.
///
/// La liste est ouverte : la DGFiP peut ajouter une série, et un code inconnu donne zéro
/// résultat, pas une erreur (05 §5.14).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Serie(String);

impl Serie {
    /// Valide `input` : espaces autour retirés, lettres ASCII mises en majuscules.
    ///
    /// ```
    /// use lf_domain::doctrine::Serie;
    ///
    /// assert_eq!(Serie::parse(" enr ")?.as_str(), "ENR");
    /// assert!(Serie::parse("EN-R").is_err());
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidSerie`] si l'entrée est vide, dépasse [`SERIE_MAX_BYTES`] octets ou
    /// porte un autre caractère qu'une lettre ASCII ou un chiffre.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        ascii_upper(input, SERIE_MAX_BYTES, is_serie_byte)
            .map(Self)
            .ok_or_else(|| DomainError::InvalidSerie { input: echo(input) })
    }

    /// Forme normalisée, ex. `ENR`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Serie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Octet admis dans un code de série, avant la mise en majuscules.
fn is_serie_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

/// `input` sans ses espaces de bord et en majuscules ASCII, s'il compte de 1 à `max_bytes` octets
/// tous admis par `allowed` ; `None` sinon.
///
/// Le contrôle porte sur l'entrée telle qu'écrite : la mise en majuscules ASCII ne change ni la
/// longueur ni un octet non ASCII, donc contrôler avant ou après revient au même, et une entrée
/// énorme n'est jamais recopiée.
fn ascii_upper(input: &str, max_bytes: usize, allowed: fn(u8) -> bool) -> Option<String> {
    let trimmed = input.trim();
    let valid = (1..=max_bytes).contains(&trimmed.len()) && trimmed.bytes().all(allowed);
    valid.then(|| trimmed.to_ascii_uppercase())
}

/// Numéro d'une publication sur le site du BOFiP, entre [`MIN`](Self::MIN) et
/// [`MAX`](Self::MAX) inclus.
///
/// Il est lu dans le permalien (`…/bofip/<numéro>-PGP.html`) et il est unique : c'est lui qui
/// départage deux publications de même identifiant (ADR-0029, décision 1). Le plus grand mesuré
/// le 2026-11-15 vaut 15 227.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PublicationId(u32);

impl PublicationId {
    /// Plus petit numéro.
    pub const MIN: u32 = 1;
    /// Plus grand numéro : sept chiffres.
    pub const MAX: u32 = 9_999_999;

    /// Valide `value`.
    ///
    /// # Errors
    ///
    /// [`DomainError::OutOfRange`] hors de `MIN..=MAX`.
    pub fn new(value: u32) -> Result<Self, DomainError> {
        Self::checked(value)
    }

    /// Valeur numérique.
    pub const fn get(self) -> u32 {
        self.0
    }

    fn checked(value: u32) -> Result<Self, DomainError> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::OutOfRange {
                field: "publication_id",
                value: i64::from(value),
                min: i64::from(Self::MIN),
                max: i64::from(Self::MAX),
            })
        }
    }
}

impl fmt::Display for PublicationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn boi_error(input: &str) -> Result<BoiId, DomainError> {
        Err(DomainError::InvalidBoiId { input: echo(input) })
    }

    fn serie_error(input: &str) -> Result<Serie, DomainError> {
        Err(DomainError::InvalidSerie { input: echo(input) })
    }

    #[test]
    fn boi_id_is_trimmed_and_uppercased() {
        for (input, expected) in [
            ("BOI-ENR-DMTG-10-50-20", "BOI-ENR-DMTG-10-50-20"),
            (" boi-enr-dmtg-10-50-20\n", "BOI-ENR-DMTG-10-50-20"),
            ("actu-2023-00012", "ACTU-2023-00012"),
            ("Boi-Annx-000050", "BOI-ANNX-000050"),
            ("7", "7"),
            ("-", "-"),
        ] {
            let id = BoiId::parse(input).unwrap();
            assert_eq!((id.as_str(), id.to_string().as_str()), (expected, expected));
            assert_eq!(BoiId::parse(expected), Ok(id));
        }
    }

    #[test]
    fn boi_id_rejects_other_characters() {
        for input in [
            "",
            "   ",
            "BOI ENR",
            "BOI_ENR",
            "BOI-ENR\"",
            "BOI-ENR\\",
            "BOI-ENR'",
            "BOI-ENR)",
            "BOI-ENR%22",
            "BOI/ENR",
            "BOI-ENR\u{0}",
            "BOI-\u{a0}ENR",
            "BOI-ÉNR",
        ] {
            assert_eq!(BoiId::parse(input), boi_error(input));
        }
    }

    /// Un repli de casse Unicode ferait de chacune de ces entrées un identifiant valide
    /// (`ß` → `SS`, `ı` → `I`, `ſ` → `S`, `ﬁ` → `FI`) : seul le repli ASCII est appliqué, et
    /// l'entrée est refusée.
    #[test]
    fn boi_id_rejects_non_ascii_case_folds() {
        for input in ["BOI-ß", "boi-enr-dmtg-ı0", "BOI-ſJ", "BOI-ﬁ"] {
            // Le repli Unicode donnerait bien une suite d'octets admise : le refus vient du
            // contrôle fait sur l'entrée telle qu'écrite, pas d'un autre défaut de forme.
            assert!(input.to_uppercase().bytes().all(is_id_byte));
            assert_eq!(BoiId::parse(input), boi_error(input));
        }
        // Signe Kelvin (U+212A), que le repli vers les minuscules rend en `k`.
        assert_eq!(BoiId::parse("BOI-\u{212a}"), boi_error("BOI-\u{212a}"));
    }

    #[test]
    fn boi_id_length_is_bounded_in_bytes() {
        let longest = "A".repeat(BOI_ID_MAX_BYTES);
        assert_eq!(BoiId::parse(&longest).unwrap().as_str(), longest);
        // Les espaces de bord ne comptent pas.
        assert_eq!(BoiId::parse(&format!("  {longest}  ")).unwrap().as_str(), longest);
        let too_long = "A".repeat(BOI_ID_MAX_BYTES + 1);
        assert_eq!(BoiId::parse(&too_long), boi_error(&too_long));
    }

    #[test]
    fn serie_is_trimmed_and_uppercased() {
        for (input, expected) in [("ENR", "ENR"), (" rfpi\t", "RFPI"), ("Tva", "TVA"), ("9", "9")] {
            let serie = Serie::parse(input).unwrap();
            assert_eq!((serie.as_str(), serie.to_string().as_str()), (expected, expected));
            assert_eq!(Serie::parse(expected), Ok(serie));
        }
    }

    #[test]
    fn serie_rejects_other_characters_and_lengths() {
        let longest = "A".repeat(SERIE_MAX_BYTES);
        assert_eq!(Serie::parse(&longest).unwrap().as_str(), longest);
        let too_long = "A".repeat(SERIE_MAX_BYTES + 1);
        for input in ["", " ", "EN-R", "EN R", "ENR\"", "ÉNR", "ß", too_long.as_str()] {
            assert_eq!(Serie::parse(input), serie_error(input));
        }
    }

    #[test]
    fn publication_id_bounds_are_inclusive() {
        for value in [1, 2, 15_227, 9_999_998, 9_999_999] {
            let id = PublicationId::new(value).unwrap();
            assert_eq!((id.get(), id.to_string()), (value, value.to_string()));
        }
        for value in [0, 10_000_000, u32::MAX] {
            assert_eq!(
                PublicationId::new(value),
                Err(DomainError::OutOfRange {
                    field: "publication_id",
                    value: i64::from(value),
                    min: 1,
                    max: 9_999_999,
                })
            );
        }
    }

    #[test]
    fn publication_ids_sort_by_number() {
        let mut ids = [15_227, 3, 9_999_999].map(|value| PublicationId::new(value).unwrap());
        ids.sort_unstable();
        assert_eq!(ids.map(PublicationId::get), [3, 15_227, 9_999_999]);
    }

    proptest! {
        /// Est accepté exactement ce que dit la grammaire ; seule la casse ASCII change, donc
        /// aucun octet dangereux pour un littéral de la requête amont ne peut sortir.
        #[test]
        fn boi_id_accepts_exactly_its_grammar(input in "[ ]?[A-Za-z0-9\\-_é\"]{0,70}[ ]?") {
            let trimmed = input.trim();
            let well_formed = (1..=BOI_ID_MAX_BYTES).contains(&trimmed.len())
                & trimmed.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
            prop_assert_eq!(
                BoiId::parse(&input).ok().map(|id| id.as_str().to_owned()),
                well_formed.then_some(trimmed.to_ascii_uppercase())
            );
        }

        #[test]
        fn serie_accepts_exactly_its_grammar(input in "[ ]?[A-Za-z0-9\\-é]{0,20}[ ]?") {
            let trimmed = input.trim();
            let well_formed = (1..=SERIE_MAX_BYTES).contains(&trimmed.len())
                & trimmed.bytes().all(|b| b.is_ascii_alphanumeric());
            prop_assert_eq!(
                Serie::parse(&input).ok().map(|serie| serie.as_str().to_owned()),
                well_formed.then_some(trimmed.to_ascii_uppercase())
            );
        }

        #[test]
        fn publication_id_accepts_exactly_its_range(value in any::<u32>()) {
            prop_assert_eq!(
                PublicationId::new(value).is_ok(),
                (1..=9_999_999).contains(&value)
            );
        }
    }
}
