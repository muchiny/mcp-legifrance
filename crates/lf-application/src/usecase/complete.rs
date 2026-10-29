//! Cas d'usage de la complétion de la variable `code` du gabarit
//! `legifrance://code/{code}/article/{num}` (v0.3, M7) et son limiteur de débit.
//!
//! Le cas d'usage lui-même est écrit par l'unité V3-07. La vague 0 ne pose que [`CompleteGate`],
//! que la façade `LegalService` porte déjà (D13 du plan « fin du projet »).

/// Limiteur de débit des complétions : au plus 10 par seconde et par processus (02 §6.2), un état
/// que la façade tient pour toute la vie du serveur.
///
/// STUB(V0) → V3-07 : sans état pour l'instant ; V3-07 y met le compteur de la fenêtre courante
/// et la méthode qui accorde ou refuse une complétion.
#[derive(Debug, Default)]
pub struct CompleteGate(());

impl CompleteGate {
    /// Limiteur neuf, fenêtre vide.
    #[must_use]
    pub fn new() -> Self {
        // STUB(V0) → V3-07
        Self(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_is_shareable_between_tasks() {
        // La façade est partagée par `Arc` entre les tâches : le limiteur doit l'être aussi.
        fn assert_shareable<T: Send + Sync + 'static>(_: &T) {}
        assert_shareable(&CompleteGate::new());
        assert!(format!("{:?}", CompleteGate::default()).starts_with("CompleteGate"));
    }
}
