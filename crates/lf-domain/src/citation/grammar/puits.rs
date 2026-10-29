//! Grammaire des références hors périmètre (puits anti-faux-positifs) (annexe B §4.14).
//!
//! STUB(V0) → V2-12 : aucune grammaire pour l'instant, `scan` ne rend aucun candidat. Les
//! plages des candidats sont des offsets du texte d'ORIGINE (`Normalized::orig_range`), celles
//! des ancres des offsets de `Normalized::text`.

use crate::citation::normalize::Normalized;
use crate::citation::scan::Anchor;

use super::Candidate;

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(_n: &Normalized, _anchors: &[Anchor]) -> Vec<Candidate> {
    // STUB(V0) → V2-12
    Vec::new()
}
