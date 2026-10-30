//! Sortie de `fr_diff_article` (v0.3, M7) : `DiffArticleOutput` et sa métrique d'audit (05 §5.12).
//!
//! L'application rend la comparaison déjà bornée : le texte des hunks tient sous 8 000 caractères
//! et l'estimation de la réponse sous la cible (05 §6.4). Les règles de rendu pures (dates
//! sentinelles omises, `etat` en code d'API) sont celles de la vue, dans `lf-adapter-mcp`.

use lf_domain::{date::CivilDate, id::LegiArtiId, status::LegalStatus};

use crate::audited::AuditMetric;

/// Une des deux versions comparées : celle qui est en vigueur à la date demandée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOut {
    /// Identifiant de cette version de l'article.
    pub article_id: LegiArtiId,
    /// Début de vigueur ; `None` : non déterminé.
    pub date_debut: Option<CivilDate>,
    /// Fin de vigueur ; `None` : sans date de fin.
    pub date_fin: Option<CivilDate>,
    /// État juridique de la version.
    pub etat: Option<LegalStatus>,
}

/// Un changement, de la version A vers la version B, au niveau du mot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkOut {
    /// Position du changement dans le texte de la version A, en caractères (points de code).
    pub at: u32,
    /// Les 60 caractères au plus qui précèdent le changement dans la version A, coupés à une
    /// limite de mot (précédés de `… ` quand le début manque).
    pub context: Option<String>,
    /// Passage retiré (présent dans A, absent de B) ; `None` pour une pure insertion.
    pub removed: Option<String>,
    /// Passage ajouté (absent de A, présent dans B) ; `None` pour une pure suppression.
    pub added: Option<String>,
}

/// Réponse de `fr_diff_article`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffArticleOutput {
    /// Identifiant demandé (n'importe quelle version de l'article).
    pub article_id: LegiArtiId,
    /// Citation de l'article, calculée sur la version B.
    pub citation: String,
    /// Version en vigueur à `date_a`.
    pub version_a: VersionOut,
    /// Version en vigueur à `date_b`.
    pub version_b: VersionOut,
    /// Vrai quand les deux dates tombent dans la même version.
    pub identical: bool,
    /// Changements, dans l'ordre du texte de A ; vide si `identical`.
    pub hunks: Vec<HunkOut>,
    /// Vrai quand des changements ou des passages ont été retirés pour tenir sous les bornes.
    pub truncated: bool,
}

impl AuditMetric for DiffArticleOutput {
    /// Nombre de hunks renvoyés (05 §5.0.1).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.hunks.len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn version(id: &str) -> VersionOut {
        VersionOut {
            article_id: LegiArtiId::parse(id).unwrap(),
            date_debut: None,
            date_fin: None,
            etat: None,
        }
    }

    fn output(hunks: Vec<HunkOut>) -> DiffArticleOutput {
        DiffArticleOutput {
            article_id: LegiArtiId::parse("LEGIARTI000000000001").unwrap(),
            citation: "Code civil, art. 9".to_owned(),
            version_a: version("LEGIARTI000000000001"),
            version_b: version("LEGIARTI000000000002"),
            identical: false,
            hunks,
            truncated: false,
        }
    }

    fn hunk(at: u32) -> HunkOut {
        HunkOut { at, context: None, removed: Some("a".to_owned()), added: None }
    }

    #[test]
    fn audit_metric_is_the_number_of_hunks() {
        assert_eq!(output(vec![]).output_count_or_size(), 0);
        assert_eq!(output(vec![hunk(0), hunk(10), hunk(20)]).output_count_or_size(), 3);
    }
}
