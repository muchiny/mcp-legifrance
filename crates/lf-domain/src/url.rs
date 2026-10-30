//! URL stables de legifrance.gouv.fr (`citations.py:76-102`).

use std::fmt;

use crate::fond::Fond;

/// Racine du site public.
pub const WEB_BASE: &str = "https://www.legifrance.gouv.fr";

/// Type de page Légifrance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// Loi ou décret : `/loda/id/`.
    Loda,
    /// Code : `/codes/texte_lc/`.
    Code,
    /// Article `LEGIARTI` : `/codes/article_lc/`.
    ///
    /// **NON VÉRIFIÉ** pour l'article d'une loi non codifiée (question Q17 =
    /// V-141 [v:url-codes-article-lc-loi], D-29/E-19 ; `citations.py:262`) :
    /// à vérifier en M2 en ouvrant l'URL d'un LEGIARTI tiré de
    /// `consult_loda.json`.
    CodeArticle,
    /// Décision judiciaire : `/juri/id/`.
    Juri,
    /// Décision du Conseil constitutionnel : `/cons/id/`.
    Constit,
    /// Décision administrative : `/ceta/id/`.
    Cetat,
    /// Texte du JO : `/jorf/id/`.
    Jorf,
    /// Délibération CNIL : `/cnil/id/`.
    Cnil,
    /// Convention collective et ses articles : `/conv_coll/id/`.
    Kali,
    /// Accord d'entreprise : `/acco/id/`.
    Acco,
}

impl ResourceKind {
    fn path(self) -> &'static str {
        match self {
            Self::Loda => "loda/id",
            Self::Code => "codes/texte_lc",
            Self::CodeArticle => "codes/article_lc",
            Self::Juri => "juri/id",
            Self::Constit => "cons/id",
            Self::Cetat => "ceta/id",
            Self::Jorf => "jorf/id",
            Self::Cnil => "cnil/id",
            Self::Kali => "conv_coll/id",
            Self::Acco => "acco/id",
        }
    }
}

impl From<Fond> for ResourceKind {
    fn from(fond: Fond) -> Self {
        match fond {
            Fond::Loda => Self::Loda,
            Fond::Code => Self::Code,
            Fond::Juri => Self::Juri,
            Fond::Constit => Self::Constit,
            Fond::Cetat => Self::Cetat,
            Fond::Cnil => Self::Cnil,
            Fond::Kali => Self::Kali,
            Fond::Acco => Self::Acco,
        }
    }
}

/// URL publique d'une ressource legifrance.gouv.fr (`source_url` du contrat v2,
/// ADR-0009). Seule [`legifrance_url`] la construit, depuis un identifiant typé :
/// jamais vide, toujours sous [`WEB_BASE`], sans échappement nécessaire
/// (`[A-Z0-9]`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceUrl(String);

impl SourceUrl {
    /// L'URL.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// URL publique d'une ressource ; `id` est un identifiant déjà validé.
///
/// Divergence avec `citations.py:76-102`, où la référence renvoie `None` pour
/// un id vide ou un `kind` inconnu : ici la fonction est totale, parce que
/// `kind` est un enum et que l'id vient toujours d'un identifiant typé déjà
/// validé. Elle est publique depuis M7 : `fr_get_related` (application) en tire
/// la page d'un lien d'article, au lieu de recopier la table des chemins ; l'appelant
/// passe `id` d'un identifiant typé (`DocumentId::as_str`), jamais d'un texte du corps.
#[must_use]
pub fn legifrance_url(kind: ResourceKind, id: &str) -> SourceUrl {
    SourceUrl(format!("{WEB_BASE}/{}/{id}", kind.path()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn urls_match_reference() {
        let id = "X";
        let cases = [
            (ResourceKind::Loda, "https://www.legifrance.gouv.fr/loda/id/X"),
            (ResourceKind::Code, "https://www.legifrance.gouv.fr/codes/texte_lc/X"),
            (ResourceKind::CodeArticle, "https://www.legifrance.gouv.fr/codes/article_lc/X"),
            (ResourceKind::Juri, "https://www.legifrance.gouv.fr/juri/id/X"),
            (ResourceKind::Constit, "https://www.legifrance.gouv.fr/cons/id/X"),
            (ResourceKind::Cetat, "https://www.legifrance.gouv.fr/ceta/id/X"),
            (ResourceKind::Jorf, "https://www.legifrance.gouv.fr/jorf/id/X"),
            (ResourceKind::Cnil, "https://www.legifrance.gouv.fr/cnil/id/X"),
            (ResourceKind::Kali, "https://www.legifrance.gouv.fr/conv_coll/id/X"),
            (ResourceKind::Acco, "https://www.legifrance.gouv.fr/acco/id/X"),
        ];
        for (kind, url) in cases {
            assert_eq!(legifrance_url(kind, id).as_str(), url);
        }
    }

    #[test]
    fn source_url_displays_its_text() {
        let url: SourceUrl = legifrance_url(ResourceKind::Cnil, "CNILTEXT000053352643");
        assert_eq!(url.to_string(), "https://www.legifrance.gouv.fr/cnil/id/CNILTEXT000053352643");
    }

    #[test]
    fn fond_maps_to_its_page_kind() {
        let kinds: Vec<_> = Fond::ALL.into_iter().map(ResourceKind::from).collect();
        assert_eq!(
            kinds,
            [
                ResourceKind::Loda,
                ResourceKind::Code,
                ResourceKind::Juri,
                ResourceKind::Constit,
                ResourceKind::Cetat,
                ResourceKind::Cnil,
                ResourceKind::Kali,
                ResourceKind::Acco,
            ]
        );
    }
}
