//! Extrait cité qui jouxte une citation (annexe B §7.9).
//!
//! STUB(V0) → V2-14 : [`adjacent_quote`] ne trouve aucun extrait pour l'instant.

use crate::citation::types::Span;

/// Span de l'extrait entre guillemets qui précède ou suit la citation de `span`, en octets de
/// `input`. `None` s'il n'y en a pas.
pub fn adjacent_quote(_input: &str, _span: Span) -> Option<Span> {
    // STUB(V0) → V2-14
    None
}
