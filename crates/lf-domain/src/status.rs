//! Statut juridique d'un texte ou d'un article : [`LegalStatus`] et [`status_of`] (03 §3.3).
//!
//! L'API emploie `etat` (`VIGUEUR`, `ABROGE`) et `jurisState` (`Vigueur` pour LODA,
//! `VIGUEUR_ETEN` pour KALI), avec une casse variable. La règle est unique pour tous les fonds :
//! `etat` s'il n'est pas vide, sinon `jurisState` (corrige E-10). La liste est fermée à 17 codes ;
//! [`LegalStatus::Other`] garde tout code inconnu (évolution de l'API), tronqué à
//! [`ECHO_MAX_CHARS`](crate::error::ECHO_MAX_CHARS) caractères.

use crate::error::echo;

/// Statut juridique : les 17 codes de l'API, plus [`Other`](Self::Other) pour un code inconnu.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LegalStatus {
    /// `VIGUEUR`.
    Vigueur,
    /// `VIGUEUR_ETEN` : en vigueur étendu (KALI).
    VigueurEtendue,
    /// `VIGUEUR_NON_ETEN` : en vigueur non étendu (KALI).
    VigueurNonEtendue,
    /// `VIGUEUR_DIFF` : en vigueur différée.
    VigueurDifferee,
    /// `ABROGE`.
    Abroge,
    /// `ABROGE_DIFF` : abrogation différée.
    AbrogeDiff,
    /// `MODIFIE`.
    Modifie,
    /// `MODIFIE_MORT_NE` : modifié avant son entrée en vigueur.
    ModifieMortNe,
    /// `PERIME`.
    Perime,
    /// `ANNULE`.
    Annule,
    /// `DISJOINT`.
    Disjoint,
    /// `TRANSFERE`.
    Transfere,
    /// `REMPLACE`.
    Remplace,
    /// `DENONCE` : dénoncé (courant en KALI).
    Denonce,
    /// `INITIALE`.
    Initiale,
    /// `SANS_ETAT`.
    SansEtat,
    /// `SUBSTITUE`.
    Substitue,
    /// Code inconnu de la liste, conservé (rogné, tronqué à
    /// [`ECHO_MAX_CHARS`](crate::error::ECHO_MAX_CHARS) caractères plus `…`).
    Other(String),
}

impl LegalStatus {
    /// Lit un code d'API : `trim`, insensible à la casse. Blanc ⇒ `None` ; code inconnu ⇒
    /// [`Other`](Self::Other).
    ///
    /// ```
    /// use lf_domain::status::LegalStatus;
    ///
    /// assert_eq!(LegalStatus::parse(" vigueur_eten "), Some(LegalStatus::VigueurEtendue));
    /// assert_eq!(LegalStatus::parse("  "), None);
    /// ```
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(
            code_to_status(&trimmed.to_ascii_uppercase())
                .unwrap_or_else(|| Self::Other(echo(trimmed))),
        )
    }

    /// Code de l'API (`"VIGUEUR"`, `"VIGUEUR_ETEN"`, …) ; pour [`Other`](Self::Other), le texte
    /// conservé.
    pub fn as_api_code(&self) -> &str {
        match self {
            Self::Vigueur => "VIGUEUR",
            Self::VigueurEtendue => "VIGUEUR_ETEN",
            Self::VigueurNonEtendue => "VIGUEUR_NON_ETEN",
            Self::VigueurDifferee => "VIGUEUR_DIFF",
            Self::Abroge => "ABROGE",
            Self::AbrogeDiff => "ABROGE_DIFF",
            Self::Modifie => "MODIFIE",
            Self::ModifieMortNe => "MODIFIE_MORT_NE",
            Self::Perime => "PERIME",
            Self::Annule => "ANNULE",
            Self::Disjoint => "DISJOINT",
            Self::Transfere => "TRANSFERE",
            Self::Remplace => "REMPLACE",
            Self::Denonce => "DENONCE",
            Self::Initiale => "INITIALE",
            Self::SansEtat => "SANS_ETAT",
            Self::Substitue => "SUBSTITUE",
            Self::Other(raw) => raw,
        }
    }
}

/// Table des 17 codes : `upper` est déjà rogné et en majuscules ASCII.
fn code_to_status(upper: &str) -> Option<LegalStatus> {
    Some(match upper {
        "VIGUEUR" => LegalStatus::Vigueur,
        "VIGUEUR_ETEN" => LegalStatus::VigueurEtendue,
        "VIGUEUR_NON_ETEN" => LegalStatus::VigueurNonEtendue,
        "VIGUEUR_DIFF" => LegalStatus::VigueurDifferee,
        "ABROGE" => LegalStatus::Abroge,
        "ABROGE_DIFF" => LegalStatus::AbrogeDiff,
        "MODIFIE" => LegalStatus::Modifie,
        "MODIFIE_MORT_NE" => LegalStatus::ModifieMortNe,
        "PERIME" => LegalStatus::Perime,
        "ANNULE" => LegalStatus::Annule,
        "DISJOINT" => LegalStatus::Disjoint,
        "TRANSFERE" => LegalStatus::Transfere,
        "REMPLACE" => LegalStatus::Remplace,
        "DENONCE" => LegalStatus::Denonce,
        "INITIALE" => LegalStatus::Initiale,
        "SANS_ETAT" => LegalStatus::SansEtat,
        "SUBSTITUE" => LegalStatus::Substitue,
        _ => return None,
    })
}

/// Statut d'un texte : `etat` s'il n'est pas vide après `trim`, sinon `juris_state`.
///
/// ```
/// use lf_domain::status::{LegalStatus, status_of};
///
/// // LODA : `etat` est null, `jurisState` vaut `Vigueur`.
/// assert_eq!(status_of(None, Some("Vigueur")), Some(LegalStatus::Vigueur));
/// // `etat` prime.
/// assert_eq!(status_of(Some("ABROGE"), Some("Vigueur")), Some(LegalStatus::Abroge));
/// ```
pub fn status_of(etat: Option<&str>, juris_state: Option<&str>) -> Option<LegalStatus> {
    etat.and_then(LegalStatus::parse).or_else(|| juris_state.and_then(LegalStatus::parse))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ECHO_MAX_CHARS;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// Les 17 codes de l'API (03 §3.3), dans l'ordre des variantes.
    fn table() -> Vec<(&'static str, LegalStatus)> {
        vec![
            ("VIGUEUR", LegalStatus::Vigueur),
            ("VIGUEUR_ETEN", LegalStatus::VigueurEtendue),
            ("VIGUEUR_NON_ETEN", LegalStatus::VigueurNonEtendue),
            ("VIGUEUR_DIFF", LegalStatus::VigueurDifferee),
            ("ABROGE", LegalStatus::Abroge),
            ("ABROGE_DIFF", LegalStatus::AbrogeDiff),
            ("MODIFIE", LegalStatus::Modifie),
            ("MODIFIE_MORT_NE", LegalStatus::ModifieMortNe),
            ("PERIME", LegalStatus::Perime),
            ("ANNULE", LegalStatus::Annule),
            ("DISJOINT", LegalStatus::Disjoint),
            ("TRANSFERE", LegalStatus::Transfere),
            ("REMPLACE", LegalStatus::Remplace),
            ("DENONCE", LegalStatus::Denonce),
            ("INITIALE", LegalStatus::Initiale),
            ("SANS_ETAT", LegalStatus::SansEtat),
            ("SUBSTITUE", LegalStatus::Substitue),
        ]
    }

    #[test]
    fn legal_status_table_roundtrips_the_17_codes() {
        let table = table();
        assert_eq!(table.len(), 17);
        for (code, status) in table {
            assert_eq!(LegalStatus::parse(code), Some(status.clone()), "{code}");
            assert_eq!(status.as_api_code(), code);
            assert_eq!(code_to_status(code), Some(status.clone()), "{code}");
            // Casse mêlée et espaces acceptés en entrée.
            let mixed = format!("  {}  ", code.to_lowercase());
            assert_eq!(LegalStatus::parse(&mixed), Some(status), "{mixed:?}");
        }
    }

    #[test]
    fn code_to_status_rejects_unknown_and_lowercase() {
        assert_eq!(code_to_status("EN_COURS"), None);
        assert_eq!(code_to_status(""), None);
        // La fonction attend la forme majuscule : la mise en casse est faite par `parse`.
        assert_eq!(code_to_status("Vigueur"), None);
    }

    #[test]
    fn legal_status_unknown_code_is_other() {
        let status = LegalStatus::parse("EN_COURS");
        assert_eq!(status, Some(LegalStatus::Other("EN_COURS".to_owned())));
        assert_eq!(status.as_ref().map(LegalStatus::as_api_code), Some("EN_COURS"));
        // Le texte inconnu est conservé tel quel (casse comprise), rogné.
        assert_eq!(
            LegalStatus::parse("  En_Cours "),
            Some(LegalStatus::Other("En_Cours".to_owned()))
        );
    }

    #[test]
    fn legal_status_blank_is_none() {
        for raw in ["", "  ", "\t\n"] {
            assert_eq!(LegalStatus::parse(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn legal_status_other_is_truncated() {
        let raw = "x".repeat(100);
        let expected = format!("{}…", "x".repeat(ECHO_MAX_CHARS));
        assert_eq!(LegalStatus::parse(&raw), Some(LegalStatus::Other(expected.clone())));
        assert_eq!(
            LegalStatus::parse(&raw).as_ref().map(LegalStatus::as_api_code),
            Some(expected.as_str())
        );
        // Exactement à la limite : rien n'est coupé.
        let exact = "y".repeat(ECHO_MAX_CHARS);
        assert_eq!(LegalStatus::parse(&exact), Some(LegalStatus::Other(exact)));
    }

    #[test]
    fn legal_status_from_etat_or_juris_state() {
        assert_eq!(status_of(None, Some("Vigueur")), Some(LegalStatus::Vigueur)); // LODA : etat null
        assert_eq!(status_of(None, Some("VIGUEUR_ETEN")), Some(LegalStatus::VigueurEtendue)); // KALI
        assert_eq!(status_of(Some("  "), Some("Vigueur")), Some(LegalStatus::Vigueur)); // etat blanc
        assert_eq!(status_of(Some("ABROGE"), Some("Vigueur")), Some(LegalStatus::Abroge)); // etat prime
        assert_eq!(status_of(Some("ABROGE"), None), Some(LegalStatus::Abroge));
        assert_eq!(status_of(Some(""), Some("  ")), None);
        assert_eq!(status_of(Some(" "), None), None);
        assert_eq!(status_of(None, Some("")), None);
        assert_eq!(status_of(None, None), None);
    }

    proptest! {
        #[test]
        fn parse_never_panics_and_keeps_other_bounded(s in "\\PC{0,200}") {
            if let Some(status) = LegalStatus::parse(&s) {
                prop_assert!(status.as_api_code().chars().count() <= ECHO_MAX_CHARS + 1);
            } else {
                prop_assert!(s.trim().is_empty());
            }
        }
    }
}
