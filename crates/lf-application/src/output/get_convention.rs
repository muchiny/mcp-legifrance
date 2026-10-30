//! Sortie de `fr_get_convention` (v0.1) : `GetConventionOutput` et sa métrique d'audit.
//!
//! La table des matières est celle de `fr_get_act` ([`TocLine`]).

use lf_domain::entity::Convention;

use super::get_act::TocLine;
use crate::audited::AuditMetric;

/// Ce que `fr_get_convention` rend à l'adapter : les métadonnées de la convention et la page de
/// sa table des matières. Pas de `date` : la convention est lue telle que l'API la sert (00 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetConventionOutput {
    /// La convention lue. Sa table des matières est vidée : la page rendue est dans `toc`.
    pub convention: Convention,
    /// Entrées de la page, après raccourcissement sous la cible du budget.
    pub toc: Vec<TocLine>,
    /// Nombre d'entrées de la table entière (`toc_total`).
    pub toc_total: u32,
    /// Début de la page suivante (`next_toc_offset`), absent en fin de table.
    pub next_toc_offset: Option<u32>,
}

impl AuditMetric for GetConventionOutput {
    /// Entrées de table des matières renvoyées (05 §5.0.1).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.toc.len()).unwrap_or(u64::MAX)
    }
}
