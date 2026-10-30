//! Sortie de `fr_resolve` (v0.2 (M6)) : `ResolveOutput` et sa métrique d'audit (05 §5.10, annexe B
//! §7.8).
//!
//! La forme canonique de la citation reconnue (`interpreted_as`), au plus cinq candidats (le
//! meilleur d'abord) et les notes, que la vue de `lf-adapter-mcp` rend en anglais, jointes en une
//! seule `note`. Une liste de candidats vide n'est pas une erreur : la note dit pourquoi.

use crate::{
    audited::AuditMetric,
    citation::{Candidate, Note},
};

/// Réponse de `fr_resolve`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOutput {
    /// Forme canonique de la citation reconnue (« Code civil, art. 9 »), absente quand le parseur
    /// n'en a pas de sûre : le modèle y lit une éventuelle mauvaise lecture.
    pub interpreted_as: Option<String>,
    /// Enregistrements possibles, au plus cinq, le plus sûr d'abord.
    pub candidates: Vec<Candidate>,
    /// Remarques, dans l'ordre : le motif d'une liste vide, un écart, les autres citations de
    /// l'entrée.
    pub notes: Vec<Note>,
}

impl AuditMetric for ResolveOutput {
    /// Toujours 1 : une citation résolue par appel (00 §7). Une erreur n'arrive pas ici, `Audited`
    /// y inscrit 0.
    fn output_count_or_size(&self) -> u64 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_audit_metric_is_one_even_without_candidates() {
        let empty = ResolveOutput {
            interpreted_as: None,
            candidates: vec![],
            notes: vec![Note::NoCitationRecognised],
        };
        assert_eq!(empty.output_count_or_size(), 1);
    }
}
