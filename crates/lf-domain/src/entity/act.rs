//! Textes consolidés : loi/décret (LODA) et convention collective (KALI).
//! Même forme côté API (`citations.py:204-228` et `403-436`).

use crate::{
    contract::{Citable, CitationContract},
    date::CivilDate,
    entity::Toc,
    id::{KaliTextId, LegiTextId},
    url::{ResourceKind, legifrance_url},
};

/// Métadonnées et sommaire communs aux textes consolidés.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBody {
    /// Identifiant chronologique (CID).
    pub cid: Option<String>,
    /// Numéro NOR.
    pub nor: Option<String>,
    /// Titre officiel.
    pub title: Option<String>,
    /// Nature, ex. `LOI`.
    pub nature: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// Date de parution au JO.
    pub date_parution: Option<CivilDate>,
    /// Numéro de parution au JO.
    pub num_parution: Option<String>,
    /// Début de la version consolidée.
    pub date_debut_version: Option<CivilDate>,
    /// Fin de la version consolidée (`2999-01-01` = sans fin connue).
    pub date_fin_version: Option<CivilDate>,
    /// Table des matières en arbre, sections et articles ([`Toc::build`]).
    pub toc: Toc,
}

/// Loi ou décret consolidé (`fr_get_act`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Act {
    /// Identifiant du texte.
    pub text_id: LegiTextId,
    /// Métadonnées et sommaire.
    pub body: TextBody,
}

/// Convention collective (`fr_get_convention`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convention {
    /// Identifiant du texte.
    pub text_id: KaliTextId,
    /// Métadonnées et sommaire.
    pub body: TextBody,
}

impl Citable for Act {
    fn contract(&self) -> CitationContract {
        let id = self.text_id.as_str();
        CitationContract::from_url(
            legifrance_url(ResourceKind::Loda, id),
            self.body.title.as_deref(),
            id,
        )
    }
}

impl Citable for Convention {
    fn contract(&self) -> CitationContract {
        let id = self.text_id.as_str();
        CitationContract::from_url(
            legifrance_url(ResourceKind::Kali, id),
            self.body.title.as_deref(),
            id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn body(title: &str) -> TextBody {
        TextBody { title: Some(title.into()), ..TextBody::default() }
    }

    #[test]
    fn act_cites_its_title_and_loda_url() {
        let act = Act {
            text_id: LegiTextId::parse("LEGITEXT000033205014").unwrap(),
            // L'appel de note « (1) » du titre est gardé (ADR-0009).
            body: body("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)"),
        };
        let c = act.contract();
        assert_eq!(
            (c.citation.as_str(), c.source_url.as_str()),
            (
                "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)",
                "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014"
            )
        );
    }

    #[test]
    fn convention_uses_conv_coll_url() {
        let convention = Convention {
            text_id: KaliTextId::parse("KALITEXT000049878950").unwrap(),
            body: body("Avenant"),
        };
        let citation = convention.contract();
        assert_eq!(
            citation.source_url.as_str(),
            "https://www.legifrance.gouv.fr/conv_coll/id/KALITEXT000049878950"
        );
        assert_eq!(citation.citation.as_str(), "Avenant");
    }
}
