//! Textes consolidés : loi/décret (LODA) et convention collective (KALI).
//! Même forme côté API (`citations.py:204-228` et `403-436`).

use crate::{
    citation::{Citable, CitationContract},
    date::CivilDate,
    id::{ArticleId, KaliTextId, LegiTextId},
    url::{ResourceKind, legifrance_url},
};

/// Entrée de table des matières : un article résoluble par `get_article`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché, ex. `L. 1234-5`.
    pub num: Option<String>,
    /// État juridique brut, ex. `VIGUEUR`.
    pub etat: Option<String>,
}

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
    /// Articles, dans l'ordre du texte.
    pub articles: Vec<TocEntry>,
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
    fn citation(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Loda, self.text_id.as_str()),
            self.body.title.clone(),
        )
    }
}

impl Citable for Convention {
    fn citation(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Kali, self.text_id.as_str()),
            self.body.title.clone(),
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
            body: body("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique"),
        };
        assert_eq!(
            act.citation(),
            CitationContract::from_url(
                "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014".into(),
                Some("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique".into()),
            )
        );
    }

    #[test]
    fn convention_uses_conv_coll_url() {
        let convention = Convention {
            text_id: KaliTextId::parse("KALITEXT000049878950").unwrap(),
            body: body("Avenant"),
        };
        let citation = convention.citation();
        assert_eq!(
            citation.source_url,
            "https://www.legifrance.gouv.fr/conv_coll/id/KALITEXT000049878950"
        );
        assert_eq!(citation.human_readable_citation.as_deref(), Some("Avenant"));
    }
}
