//! Liens d'un article (`/consult/relatedLinksArticle`, M7, 03 §4.11, 05 §5.11).

use crate::{date::CivilDate, id::DocumentId};

/// Nature d'un lien (`LiensRelatifsDTO.nature`). Une nature inconnue de l'API est gardée telle
/// quelle dans [`RelatedNature::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelatedNature {
    /// Texte de loi ou de décret consolidé.
    Loda,
    /// Texte du Journal officiel.
    Jorf,
    /// Code.
    Code,
    /// Convention collective.
    Kali,
    /// Conteneur du Journal officiel.
    JorfCont,
    /// Nature non reconnue, recopiée nettoyée.
    Other(String),
}

/// Un lien vers ou depuis un article.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedLink {
    /// Cible du lien, quand son identifiant porte un type consultable. Un identifiant sans type
    /// consultable (`JORFCONT`, `JORFARTI`) donne `None` : le lien garde son titre, sa nature et
    /// sa date, sans `source_url`.
    pub target: Option<DocumentId>,
    /// Nature du lien.
    pub nature: RelatedNature,
    /// Titre du lien, nettoyé.
    pub title: Option<String>,
    /// Date de vigueur du lien (sentinelles de 03 §6.2 déjà écartées).
    pub date: Option<CivilDate>,
}

/// Les deux sens des liens d'un article.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelatedLinks {
    /// Textes que l'article cite (`liensCite`).
    pub cites: Vec<RelatedLink>,
    /// Textes qui citent l'article (`liensCitePar`).
    pub cited_by: Vec<RelatedLink>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn links_default_to_empty_and_compare_by_value() {
        assert_eq!(RelatedLinks::default(), RelatedLinks { cites: vec![], cited_by: vec![] });
        let link = RelatedLink {
            target: Some(DocumentId::parse("LEGITEXT000006070721").unwrap()),
            nature: RelatedNature::Other("AUTRE".into()),
            title: Some("Code civil".into()),
            date: CivilDate::new(2020, 1, 1),
        };
        let links = RelatedLinks { cites: vec![link.clone()], cited_by: vec![] };
        assert_eq!(links.clone(), links);
        assert_ne!(RelatedLink { nature: RelatedNature::Loda, ..link.clone() }, link);
        assert_ne!(RelatedNature::Code, RelatedNature::Kali);
        assert_ne!(RelatedNature::Jorf, RelatedNature::JorfCont);
    }
}
