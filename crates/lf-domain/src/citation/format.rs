//! Forme canonique et résolvabilité d'une citation (annexe B §6.5, 03 §8.7).
//!
//! STUB(V0) → V2-14 : [`canonical`] ne propose aucune forme et toute citation est déclarée
//! résoluble pour l'instant.

use crate::citation::types::{CitationRef, Resolvability, Spanned};

/// Forme canonique de ce que le parseur a reconnu (« Code civil, art. 9 »), valeur
/// `interpreted_as` de `fr_resolve`. `None` pour une citation sans forme sûre.
pub fn canonical(_c: &CitationRef) -> Option<String> {
    // STUB(V0) → V2-14
    None
}

impl CitationRef {
    /// La citation peut-elle être cherchée dans le corpus ?
    pub fn resolvability(&self) -> Resolvability {
        // STUB(V0) → V2-14
        Resolvability::Resolvable
    }
}

impl Spanned<CitationRef> {
    /// Résolvabilité compte tenu des drapeaux `Impossible` et des ambiguïtés bloquantes.
    pub fn effective_resolvability(&self) -> Resolvability {
        // STUB(V0) → V2-14
        self.value.resolvability()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::types::Span;
    use crate::contract::Nor;

    #[test]
    fn a_citation_without_flag_has_the_resolvability_of_its_value() {
        let nor = Nor::parse("ECFI1524250L").expect("NOR valide");
        let value = CitationRef::Nor(nor);
        let spanned = Spanned::new(value.clone(), Span { start: 0, end: 12 });
        assert_eq!(spanned.effective_resolvability(), value.resolvability());
        // La forme canonique, quelle qu'elle soit, ne dépend que de la valeur.
        assert_eq!(canonical(&value), canonical(&spanned.value));
    }
}
