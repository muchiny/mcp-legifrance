//! Sortie de `fr_search` (v0.1) : `SearchOutput` et sa métrique d'audit (05 §5.1).
//!
//! L'application rend des résultats déjà mis en forme pour la réponse : le contrat de citation
//! (`citation`, `source_url`) est calculé ici, avant toute dégradation, car la citation d'un
//! résultat est tirée de son titre (03 §5) et le plafond de réponse (05 §6.4) peut retirer ce
//! titre. Les règles de rendu pures (titre omis s'il est dans la citation, `etat` en code d'API,
//! date sentinelle omise, `idcc` et `total_articles` selon la surface) sont celles de la vue, dans
//! `lf-adapter-mcp`.

use lf_domain::entity::SearchHit;

use crate::audited::AuditMetric;

/// Un résultat de recherche et son contrat de citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitOut {
    /// Le résultat du corpus. La politique de budget peut en retirer l'extrait, la liste
    /// d'articles et les champs optionnels (`title`, `nature`, `etat`, `date`), jamais l'identifiant
    /// ni le fond.
    pub hit: SearchHit,
    /// Citation selon l'usage français (`CitationContract::citation`), calculée avant dégradation.
    pub citation: String,
    /// Page Légifrance de la ressource (`CitationContract::source_url`).
    pub source_url: String,
}

/// Réponse de `fr_search` : une page de résultats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutput {
    /// Nombre total de résultats annoncé par la source (`totalResultNumber`), sinon le nombre de
    /// résultats reçus.
    pub total: u64,
    /// Page servie, à partir de 1.
    pub page: u32,
    /// Vrai si la page suivante existe **et** est atteignable (`lf_domain::search::has_more`).
    pub has_more: bool,
    /// Nombre total d'articles correspondants (fond `CODE`, si l'API le donne).
    pub total_articles: Option<u64>,
    /// Résultats de la page, dans l'ordre de l'API ; jamais retirés par le plafond de réponse.
    pub hits: Vec<HitOut>,
    /// Remarque exceptionnelle : fin de la fenêtre atteignable, extraits retirés (05 §5.1, §6.4).
    pub note: Option<String>,
}

impl AuditMetric for SearchOutput {
    /// Nombre de résultats renvoyés, et non `total` (05 §5.0.1, parité).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.hits.len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::{fond::Fond, id::DocumentId};
    use pretty_assertions::assert_eq;

    fn hit_out(n: u32) -> HitOut {
        let id = DocumentId::parse(&format!("JURITEXT{n:012}")).unwrap();
        HitOut {
            hit: SearchHit::new(Fond::Juri, id, None).unwrap(),
            citation: "Cour de cassation".to_owned(),
            source_url: "https://www.legifrance.gouv.fr/juri/id/JURITEXT000000000001".to_owned(),
        }
    }

    fn output(hits: Vec<HitOut>, total: u64) -> SearchOutput {
        SearchOutput { total, page: 1, has_more: false, total_articles: None, hits, note: None }
    }

    #[test]
    fn audit_metric_is_the_number_of_hits_returned_not_the_total() {
        assert_eq!(output(vec![], 0).output_count_or_size(), 0);
        assert_eq!(
            output(vec![hit_out(1), hit_out(2), hit_out(3)], 51_531).output_count_or_size(),
            3
        );
    }
}
