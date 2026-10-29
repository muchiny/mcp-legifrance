//! Sortie de `fr_search_all` (v0.3, M7) : `SearchAllOutput` et sa métrique d'audit (05 §5.13).
//!
//! Un groupe par fond demandé, dans l'ordre de `fonds`. Un fond qui échoue donne une entrée
//! d'erreur et ne fait pas échouer les autres ; l'erreur garde son type (`AppError`), et c'est la
//! vue de `lf-adapter-mcp` qui la rend en `[code] message`. Les résultats ont la forme de ceux de
//! `fr_search` (`HitOut`), contrat de citation compris.

use lf_domain::fond::Fond;

use crate::{audited::AuditMetric, error::AppError, output::search::HitOut};

/// Ce qu'un fond a trouvé : la première page de ses résultats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FondHits {
    /// Nombre total de résultats annoncé par la source pour ce fond.
    pub total: u64,
    /// Vrai si d'autres résultats existent au-delà de ceux qui sont rendus et atteignables
    /// (`fr_search` les donne, page par page).
    pub has_more: bool,
    /// Les `per_fond` premiers résultats, dans l'ordre de l'API ; jamais retirés par le plafond
    /// de réponse.
    pub hits: Vec<HitOut>,
}

/// Un groupe de la réponse : un fond, et ce qu'il a donné.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOut {
    /// Fond interrogé.
    pub fond: Fond,
    /// Résultats du fond, ou l'erreur qu'il a rendue.
    pub result: Result<FondHits, AppError>,
}

/// Réponse de `fr_search_all`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchAllOutput {
    /// Un groupe par fond, dans l'ordre de `fonds`.
    pub groups: Vec<GroupOut>,
    /// Remarque exceptionnelle : contenu retiré pour tenir sous le plafond (05 §6.4).
    pub note: Option<String>,
}

impl AuditMetric for SearchAllOutput {
    /// Somme des résultats renvoyés par tous les fonds (05 §5.0.1).
    fn output_count_or_size(&self) -> u64 {
        self.groups
            .iter()
            .filter_map(|group| group.result.as_ref().ok())
            .map(|found| u64::try_from(found.hits.len()).unwrap_or(u64::MAX))
            .fold(0, u64::saturating_add)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::{entity::SearchHit, id::DocumentId};
    use pretty_assertions::assert_eq;

    fn hit(n: u32) -> HitOut {
        let id = DocumentId::parse(&format!("JURITEXT{n:012}")).unwrap();
        HitOut {
            hit: SearchHit::new(Fond::Juri, id, None).unwrap(),
            citation: "Cour de cassation".to_owned(),
            source_url: "https://www.legifrance.gouv.fr/juri/id/JURITEXT000000000001".to_owned(),
        }
    }

    fn found(hits: u32) -> GroupOut {
        GroupOut {
            fond: Fond::Juri,
            result: Ok(FondHits { total: 99, has_more: true, hits: (1..=hits).map(hit).collect() }),
        }
    }

    #[test]
    fn audit_metric_sums_the_hits_of_the_groups_and_ignores_failed_ones() {
        let failed = GroupOut { fond: Fond::Cnil, result: Err(AppError::config("no credentials")) };
        let output = SearchAllOutput { groups: vec![found(3), failed, found(2)], note: None };
        assert_eq!(output.output_count_or_size(), 5, "les hits renvoyés, pas les totaux");
        let empty = SearchAllOutput { groups: vec![], note: None };
        assert_eq!(empty.output_count_or_size(), 0);
    }
}
