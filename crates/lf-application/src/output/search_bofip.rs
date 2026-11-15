//! Sortie de `fr_search_bofip` (jalon MB) : `SearchBofipOutput` et sa métrique d'audit (05 §5.14).
//!
//! Une page de publications du BOFiP, jamais leur texte. Les deux champs constants de la réponse
//! (`attribution`, `doctrine_note`) sont écrits par la vue de `lf-adapter-mcp` : la politique de
//! taille leur réserve [`BOFIP_CONSTANTS_RESERVE_CHARS`] caractères.

use lf_domain::doctrine::DoctrineHit;

use crate::audited::AuditMetric;

/// Caractères réservés, dans le calcul de taille d'une réponse, aux deux champs constants que la
/// vue ajoute : `attribution` et `doctrine_note`, clés, guillemets et virgules compris. La vue
/// vérifie par un test que ses constantes tiennent dans cette réserve.
pub const BOFIP_CONSTANTS_RESERVE_CHARS: usize = 1_000;

/// Réponse de `fr_search_bofip` : une page de résultats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchBofipOutput {
    /// Nombre total de publications trouvées, toutes pages confondues.
    pub total: u64,
    /// Page servie, à partir de 1.
    pub page: u32,
    /// Vrai si la page suivante existe **et** est atteignable (`lf_domain::search::has_more`).
    pub has_more: bool,
    /// Résultats de la page, dans l'ordre du portail ; jamais retirés par le plafond de réponse,
    /// qui ne fait que raccourcir des titres.
    pub hits: Vec<DoctrineHit>,
    /// Remarque exceptionnelle : aucun résultat, fin de la fenêtre atteignable, titres raccourcis.
    pub note: Option<String>,
}

impl AuditMetric for SearchBofipOutput {
    /// Nombre de résultats renvoyés, et non `total` (05 §5.14).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.hits.len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::fixtures::bofip;

    #[test]
    fn the_metric_counts_the_hits_returned_not_the_total() {
        let mut output =
            SearchBofipOutput { total: 357, page: 1, has_more: true, hits: Vec::new(), note: None };
        assert_eq!(output.output_count_or_size(), 0);
        output.hits = vec![bofip::hit(3350, "a"), bofip::hit(3351, "b"), bofip::hit(3352, "c")];
        assert_eq!(output.output_count_or_size(), 3);
    }
}
