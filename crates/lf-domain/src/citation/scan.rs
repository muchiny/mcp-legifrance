//! Ancres de la détection (annexe B §4.16).
//!
//! Une ancre est une plage de `Normalized::text` où une famille a une chance de commencer ; les
//! grammaires la lisent ensuite dans une fenêtre `[ancre - 80, ancre + 400]` caractères. Les
//! classes suivent le tableau du §4.16 : mots-clés repérés par `aho-corasick` sur la vue clé,
//! jetons atomiques repérés par `RegexSet` sur le texte.
//!
//! STUB(V0) → V2-13 : [`find_anchors`] ne rend aucune ancre pour l'instant ; [`Anchor`] et
//! [`AnchorClass`] sont figés pour que les grammaires de V2 les construisent à la main.

use std::ops::Range;

use crate::citation::normalize::Normalized;

/// Classe d'une ancre : décide quelles grammaires l'essaient.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[allow(dead_code, reason = "STUB(V0) → V2-10 : construit par les grammaires et `find_anchors`")]
pub(crate) enum AnchorClass {
    /// Mot-clé d'article ou de subdivision : `article`, `art.`, `alinea`, `phrase`.
    ArticleKeyword,
    /// Nature de texte : `loi`, `ordonnance`, `decret`, `l. n`, `ord.`.
    TextNature,
    /// Juridiction, chambre ou formation : `cass`, `civ.`, `cour d'appel`, `conseil d'etat`.
    Court,
    /// Code : titre replié, abréviation, anaphore (`meme code`, `dudit code`).
    Code,
    /// Autre famille : `cnil`, `idcc`, `avenant`, `jorf`, `nor`, `/eli/`, `constitution`.
    Family,
    /// Puits : `directive`, `cjue`, `jcp`, `bull.`, `leb.`.
    Sink,
    /// `ECLI:`.
    Ecli,
    /// NOR : `[A-Z]{4}\d{7}[A-Z]`.
    Nor,
    /// Identifiant Légifrance : `(LEGITEXT|…)\d{12}`.
    LegifranceId,
    /// Numéro de pourvoi : `\d{2}-\d{2}\.?\d{3}`.
    Pourvoi,
    /// Numéro de CAA : `\d{2}(PA|VE|MA|…)\d{5}`.
    CaaNumber,
    /// Répertoire général : `\d{2}/\d{4,5}`.
    Rg,
    /// Numéro du Conseil constitutionnel : `\d{4}-\d+(/\d+)?\s?(DC|QPC|…)`.
    CcNumber,
    /// Sigle en majuscules : `CE`, `CC`, `CGI`.
    Sigle,
    /// Préfixe d'article suivi d'un chiffre : `(LO|L|R|D|A)\*{0,2}\.?\s?\d`.
    ArticlePrefix,
}

/// Une ancre : sa classe et sa plage, en octets de `Normalized::text`.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code, reason = "STUB(V0) → V2-10 : lu par les grammaires")]
pub(crate) struct Anchor {
    /// Classe de l'ancre.
    pub class: AnchorClass,
    /// Plage en octets de `Normalized::text`.
    pub range: Range<usize>,
}

/// Toutes les ancres du texte, triées par début.
pub(crate) fn find_anchors(_n: &Normalized) -> Vec<Anchor> {
    // STUB(V0) → V2-13
    Vec::new()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::normalize::{Steps, normalize};

    #[test]
    fn anchors_are_built_by_hand() {
        let n = normalize("art. L. 761-1 CE", Steps::default());
        let hand = Anchor { class: AnchorClass::Sigle, range: 14..16 };
        assert_eq!(n.text.get(hand.range.clone()), Some("CE"));
        assert_eq!(hand.clone(), hand);
        assert_ne!(AnchorClass::Code, AnchorClass::Sink);
        // Les ancres d'un texte sans repère ne sont pas inventées.
        assert_eq!(find_anchors(&normalize("", Steps::default())), Vec::<Anchor>::new());
    }
}
