//! Statut juridique d'un texte ou d'un article : [`LegalStatus`] et [`status_of`] (03 §3.3).
//!
//! L'API emploie `etat` (`VIGUEUR`, `ABROGE`) et `jurisState` (`Vigueur` pour LODA,
//! `VIGUEUR_ETEN` pour KALI), avec une casse variable. La règle est unique pour tous les fonds :
//! `etat` s'il n'est pas vide, sinon `jurisState` (corrige E-10). La liste est fermée à 17 codes,
//! reconnus sans égard à la casse ni aux accents (`Abrogé` ⇒ `ABROGE`) ;
//! [`LegalStatus::Other`] garde tout code inconnu (évolution de l'API), sans contrôle ni
//! caractère invisible, tronqué à [`ECHO_MAX_CHARS`](crate::error::ECHO_MAX_CHARS) caractères.

use unicode_normalization::{UnicodeNormalization, char::canonical_combining_class};

use crate::{
    error::echo,
    text::{fold::fold, is_invisible},
};

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
    /// Code inconnu de la liste, conservé (sans contrôle ni invisible, rogné, tronqué à
    /// [`ECHO_MAX_CHARS`](crate::error::ECHO_MAX_CHARS) caractères plus `…`).
    ///
    /// À construire par [`LegalStatus::parse`] seulement : la variante est publique (pour le
    /// filtrage par motif), mais une valeur écrite à la main n'a ni ce nettoyage ni cette borne,
    /// et `Other("VIGUEUR".into())` n'est pas égal à [`Vigueur`](Self::Vigueur).
    Other(String),
}

impl LegalStatus {
    /// Lit un code d'API : contrôles (`\n`, ESC…) et invisibles (bidi, largeur nulle…, jeu de
    /// `clean`) retirés, puis `trim`, insensible à la casse. Rien de visible ⇒ `None` ; code
    /// inconnu ⇒ [`Other`](Self::Other), dont le texte n'a donc jamais de contrôle ni
    /// d'invisible (audit L-2 : `as_api_code` est rendu tel quel).
    ///
    /// ```
    /// use lf_domain::status::LegalStatus;
    ///
    /// assert_eq!(LegalStatus::parse(" vigueur_eten "), Some(LegalStatus::VigueurEtendue));
    /// assert_eq!(LegalStatus::parse("  "), None);
    /// ```
    pub fn parse(raw: &str) -> Option<Self> {
        let visible = visible_code(raw);
        let trimmed = visible.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(code_to_status(&table_key(trimmed)).unwrap_or_else(|| Self::Other(echo(trimmed))))
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

/// `raw` sans ses contrôles (`\t`, `\n` et `\r` compris) ni ses caractères invisibles.
fn visible_code(raw: &str) -> String {
    raw.chars().filter(|&c| !(c.is_control() || is_invisible(c))).collect()
}

/// Clé de la table pour `trimmed` (déjà rogné) : marques combinantes retirées après décomposition
/// (`Abrogé` composé ou `e` + accent aigu), accents et ligatures repliés par [`fold`], puis
/// majuscules. L'insensibilité à la casse et aux accents vaut pour la clé seule : [`Other`]
/// garde le texte d'origine (get_act-4 : `jurisState` « Abrogé » ⇒ `ABROGE`).
///
/// [`Other`]: LegalStatus::Other
fn table_key(trimmed: &str) -> String {
    let bare: String = trimmed.nfd().filter(|&c| canonical_combining_class(c) == 0).collect();
    fold(&bare).to_ascii_uppercase()
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
    fn legal_status_accented_labels_fold_to_the_api_codes() {
        // get_act-4 : `jurisState` arrive en libellé accentué quand `etat` est null (LODA).
        let labels = [
            ("Abrogé", LegalStatus::Abroge),
            ("ABROGÉ", LegalStatus::Abroge),
            ("abrogé", LegalStatus::Abroge),
            ("Modifié", LegalStatus::Modifie),
            ("Périmé", LegalStatus::Perime),
            ("Annulé", LegalStatus::Annule),
            ("Transféré", LegalStatus::Transfere),
            ("Remplacé", LegalStatus::Remplace),
            ("Dénoncé", LegalStatus::Denonce),
            ("Substitué", LegalStatus::Substitue),
        ];
        for (raw, status) in labels {
            assert_eq!(LegalStatus::parse(raw), Some(status.clone()), "{raw}");
            assert_eq!(LegalStatus::parse(&format!(" {raw} ")), Some(status), "{raw}");
        }
        // Forme décomposée (e + accent aigu combinant) : même résultat que la forme composée.
        assert_eq!(LegalStatus::parse("Abroge\u{301}"), Some(LegalStatus::Abroge));
        assert_eq!(LegalStatus::parse("ABROGE\u{301}"), Some(LegalStatus::Abroge));
        assert_eq!(LegalStatus::parse("Pe\u{301}rime\u{301}"), Some(LegalStatus::Perime));
        // Un code à tiret bas accentué se replie aussi.
        assert_eq!(LegalStatus::parse("Modifié_mort_né"), Some(LegalStatus::ModifieMortNe));
        // Le texte porté par `status_of` suit.
        assert_eq!(status_of(None, Some("Abrogé")), Some(LegalStatus::Abroge));
        assert_eq!(status_of(Some("  "), Some("Abrogé")), Some(LegalStatus::Abroge));
        assert_eq!(status_of(Some("VIGUEUR"), Some("Abrogé")), Some(LegalStatus::Vigueur));
    }

    #[test]
    fn legal_status_unknown_accented_label_keeps_its_original_text() {
        // Jumeau : un libellé inconnu n'est pas replié dans la valeur conservée.
        let status = LegalStatus::parse("Échu");
        assert_eq!(status, Some(LegalStatus::Other("Échu".to_owned())));
        assert_eq!(status.as_ref().map(LegalStatus::as_api_code), Some("Échu"));
        // L'accent n'invente pas de code : « Abrogéx » reste inconnu, texte d'origine gardé.
        assert_eq!(LegalStatus::parse("Abrogéx"), Some(LegalStatus::Other("Abrogéx".to_owned())));
        // Un libellé à espaces n'est pas reconnu (pas de table de libellés, V-113).
        assert_eq!(
            LegalStatus::parse("Abrogé différé"),
            Some(LegalStatus::Other("Abrogé différé".to_owned()))
        );
    }

    #[test]
    fn legal_status_blank_is_none() {
        for raw in ["", "  ", "\t\n"] {
            assert_eq!(LegalStatus::parse(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn legal_status_other_drops_controls_and_invisibles() {
        // Audit L-2 : bidi, largeur nulle, saut de ligne et séquence ANSI ne passent pas par
        // `as_api_code` ; seul le texte visible reste.
        let status = LegalStatus::parse("X\u{202E}\u{200B}\nY\u{1b}[31m");
        assert_eq!(status, Some(LegalStatus::Other("XY[31m".to_owned())));
        assert_eq!(status.as_ref().map(LegalStatus::as_api_code), Some("XY[31m"));
        assert_eq!(
            LegalStatus::parse("EN\tCOURS\r\u{7f}\u{85}"),
            Some(LegalStatus::Other("ENCOURS".to_owned()))
        );
        // Le nettoyage précède la table : un code connu masqué par un invisible est reconnu.
        assert_eq!(LegalStatus::parse("VIG\u{200b}UEUR"), Some(LegalStatus::Vigueur));
        assert_eq!(LegalStatus::parse("\u{feff} abroge \u{2066}"), Some(LegalStatus::Abroge));
        // Rien de visible : comme un blanc.
        assert_eq!(LegalStatus::parse("\u{200b}\u{202e}\u{1b}"), None);
        // La troncature compte les caractères gardés.
        let raw = format!("{}{}", "\u{200b}".repeat(10), "x".repeat(100));
        let expected = format!("{}…", "x".repeat(ECHO_MAX_CHARS));
        assert_eq!(LegalStatus::parse(&raw), Some(LegalStatus::Other(expected)));
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
        fn parse_never_panics_and_keeps_other_bounded(
            s in prop_oneof!["\\PC{0,200}", "[ \t\n\u{200b}\u{202e}\u{feff}]{0,8}"],
        ) {
            if let Some(status) = LegalStatus::parse(&s) {
                prop_assert!(status.as_api_code().chars().count() <= ECHO_MAX_CHARS + 1);
            } else {
                prop_assert!(s.chars().all(|c| c.is_whitespace() || c.is_control() || is_invisible(c)));
            }
        }

        #[test]
        fn other_never_keeps_a_control_or_invisible_char(
            parts in proptest::collection::vec(
                prop_oneof![
                    "[A-Z_]{1,4}",
                    Just("\n".to_owned()),
                    Just("\u{1b}[31m".to_owned()),
                    Just("\u{202e}".to_owned()),
                    Just("\u{200b}".to_owned()),
                    Just("\u{fe0f}".to_owned()),
                    any::<char>().prop_map(String::from),
                ],
                0..12,
            ),
        ) {
            let raw = parts.concat();
            let code = LegalStatus::parse(&raw).map(|status| status.as_api_code().to_owned());
            let code = code.unwrap_or_default();
            prop_assert!(code.chars().all(|c| !c.is_control() && !is_invisible(c)), "{code:?}");
        }
    }
}
