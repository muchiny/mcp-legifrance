//! Sortie de `fr_search_service_public` (jalon MS) : `SearchServicePublicOutput` et sa métrique
//! d'audit (05 §5.16).
//!
//! Une page de fiches et de dossiers de Service-Public, jamais leur texte. Les champs constants
//! de la réponse (`attribution`, `source_files`, `guidance_note`) sont écrits par la vue de
//! `lf-adapter-mcp` à partir de `audience` et `files` : la politique de taille leur réserve
//! [`GUIDANCE_CONSTANTS_RESERVE_CHARS`] caractères.

use lf_domain::guidance::{AudienceFilter, GuidanceHit};

use crate::{audited::AuditMetric, ports::ArchiveFile};

/// Caractères réservés, dans le calcul de taille d'une réponse de Service-Public, aux champs que
/// la vue ajoute : `attribution` (deux sites au plus), `source_files` (deux archives au plus) ou
/// `source_file`, et `guidance_note`, clés, guillemets et virgules compris. La vue vérifie par un
/// test que ses constantes tiennent dans cette réserve.
pub const GUIDANCE_CONSTANTS_RESERVE_CHARS: usize = 1_500;

/// Réponse de `fr_search_service_public` : une page de résultats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchServicePublicOutput {
    /// Publics cherchés : la vue en tire `attribution`.
    pub audience: AudienceFilter,
    /// Nombre total de pages trouvées, toutes pages de résultats confondues.
    pub total: u64,
    /// Page servie, à partir de 1.
    pub page: u32,
    /// Vrai si la page suivante existe (`lf_domain::search::has_more`).
    pub has_more: bool,
    /// Résultats de la page, dans l'ordre du classement ; jamais retirés par le plafond de
    /// réponse, qui ne fait que raccourcir des titres.
    pub hits: Vec<GuidanceHit>,
    /// Archive de chaque public cherché, dans l'ordre de `audience` (Particuliers d'abord).
    pub files: Vec<ArchiveFile>,
    /// Remarque exceptionnelle : aucun résultat, titres raccourcis.
    pub note: Option<String>,
}

impl AuditMetric for SearchServicePublicOutput {
    /// Nombre de résultats renvoyés, et non `total`.
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.hits.len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::guidance::Audience;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::fixtures::service_public::{file, hit};

    #[test]
    fn the_metric_is_the_number_of_hits_returned() {
        let output = SearchServicePublicOutput {
            audience: AudienceFilter::All,
            total: 312,
            page: 1,
            has_more: true,
            hits: vec![
                hit(Audience::Particuliers, "F1437", "Compte bancaire"),
                hit(Audience::Entreprendre, "F31901", "Compte professionnel"),
            ],
            files: Audience::ALL.map(file).into(),
            note: None,
        };
        assert_eq!(output.output_count_or_size(), 2);
    }
}
