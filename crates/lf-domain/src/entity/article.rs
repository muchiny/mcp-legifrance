//! Article seul (`fr_get_text`, `citations.py:248-276`).

use crate::{
    contract::{Citable, CitationContract, article_citation},
    date::CivilDate,
    id::ArticleId,
    url::{ResourceKind, legifrance_url},
};

/// Texte d'un article de code, de loi ou de convention collective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleText {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché.
    pub num: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// Titre du code (`citations.py:236-245`, `_code_title` : entrée `textTitles`
    /// de nature `CODE` en priorité, sinon la première).
    pub code_title: Option<String>,
    /// Chemin des sections.
    pub full_sections_titre: Option<String>,
    /// Début de vigueur.
    pub date_debut: Option<CivilDate>,
    /// Fin de vigueur.
    pub date_fin: Option<CivilDate>,
    /// Texte brut de l'article.
    pub text: Option<String>,
}

impl Citable for ArticleText {
    fn contract(&self) -> CitationContract {
        let kind = match self.article_id {
            ArticleId::Legi(_) => ResourceKind::CodeArticle,
            ArticleId::Kali(_) => ResourceKind::Kali,
        };
        CitationContract::from_url(
            legifrance_url(kind, self.article_id.as_str()).to_string(),
            article_citation(self.code_title.as_deref(), self.num.as_deref()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn article(id: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(id).unwrap(),
            num: Some("9".into()),
            etat: None,
            code_title: Some("Code civil".into()),
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: None,
        }
    }

    #[test]
    fn legi_article_cites_code_and_number() {
        let citation = article("LEGIARTI000006419288").contract();
        assert_eq!(
            citation,
            CitationContract::from_url(
                "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288".into(),
                Some("Code civil, art. 9".into()),
            )
        );
    }

    #[test]
    fn kali_article_uses_conv_coll_url() {
        // test_kali_article_url_uses_conv_coll de la référence.
        let citation = article("KALIARTI000005805295").contract();
        assert_eq!(
            citation.source_url,
            "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
        );
    }
}
