//! Version d'un article (`canonicalArticleVersion`, M7, 03 §4.11).

use crate::{date::CivilDate, id::LegiArtiId, status::LegalStatus};

/// Une version d'un article, pour `article_versions` et `fr_diff_article`.
///
/// Les dates suivent le sens de 03 §6.2 : `date_fin` à `None` veut dire « en vigueur, sans date
/// de fin » (sentinelle de l'API déjà écartée par le mapper), `date_debut` à `None` « date
/// d'entrée en vigueur non déterminée ».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleVersion {
    /// Identifiant de cette version de l'article.
    pub id: LegiArtiId,
    /// Numéro affiché, ex. `L. 1234-5`.
    pub num: Option<String>,
    /// État juridique ; `None` : absent ou non reconnu.
    pub etat: Option<LegalStatus>,
    /// Date d'entrée en vigueur.
    pub date_debut: Option<CivilDate>,
    /// Date de fin de vigueur, absente pour la version en vigueur.
    pub date_fin: Option<CivilDate>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn version_compares_by_value() {
        let version = ArticleVersion {
            id: LegiArtiId::parse("LEGIARTI000006419288").unwrap(),
            num: Some("9".into()),
            etat: Some(LegalStatus::Vigueur),
            date_debut: CivilDate::new(2016, 10, 1),
            date_fin: None,
        };
        assert_eq!(version.clone(), version);
        assert_ne!(
            ArticleVersion { date_fin: CivilDate::new(2020, 1, 1), ..version.clone() },
            version
        );
        assert_eq!(version.id.as_str(), "LEGIARTI000006419288");
    }
}
