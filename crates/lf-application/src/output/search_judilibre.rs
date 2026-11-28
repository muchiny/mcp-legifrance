//! Sortie de `fr_search_judilibre` (jalon MJ) : `SearchJudilibreOutput` et sa métrique d'audit
//! (05 §5.18).
//!
//! Une page de décisions, jamais leur texte. Les champs constants de la réponse (`attribution`,
//! `case_law_note`) et ceux qui se tirent de chaque décision (`citation`) sont écrits par la vue
//! de `lf-adapter-mcp` : la politique de taille leur réserve [`CASE_LAW_CONSTANTS_RESERVE_CHARS`]
//! caractères, et compte la citation de chaque résultat.

use lf_domain::{date::CivilDate, judilibre::CaseLawHit};

use crate::audited::AuditMetric;

/// Caractères réservés, dans le calcul de taille d'une réponse, aux champs constants que la vue
/// ajoute : `attribution` et `case_law_note`, clés, guillemets et virgules compris. La vue
/// vérifie par un test que ses constantes tiennent dans cette réserve.
pub const CASE_LAW_CONSTANTS_RESERVE_CHARS: usize = 1_000;

/// Réponse de `fr_search_judilibre` : une page de résultats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchJudilibreOutput {
    /// Nombre total de décisions trouvées, toutes pages confondues.
    pub total: u64,
    /// Page servie, à partir de 1.
    pub page: u32,
    /// Vrai si la page suivante existe **et** est atteignable (`lf_domain::search::has_more`).
    pub has_more: bool,
    /// Résultats de la page, dans l'ordre de Judilibre ; jamais retirés par le plafond de
    /// réponse, qui ne fait que raccourcir des sommaires.
    pub hits: Vec<CaseLawHit>,
    /// Jour de la consultation, Europe/Paris : mention exigée par les conditions de réutilisation.
    pub consulted_on: CivilDate,
    /// Remarque exceptionnelle : aucun résultat, page vide, fin de la fenêtre atteignable,
    /// sommaires raccourcis.
    pub note: Option<String>,
}

impl AuditMetric for SearchJudilibreOutput {
    /// Nombre de résultats renvoyés, et non `total` (05 §5.18).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.hits.len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::fixtures::judilibre;

    #[test]
    fn the_metric_counts_the_hits_returned_not_the_total() {
        let mut output = SearchJudilibreOutput {
            total: 1_234,
            page: 1,
            has_more: true,
            hits: Vec::new(),
            consulted_on: CivilDate::new(2026, 11, 27).unwrap(),
            note: None,
        };
        assert_eq!(output.output_count_or_size(), 0);
        output.hits = vec![judilibre::hit(judilibre::CC_ID), judilibre::hit(judilibre::CA_ID)];
        assert_eq!(output.output_count_or_size(), 2);
    }
}
