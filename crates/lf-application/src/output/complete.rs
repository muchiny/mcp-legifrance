//! Sortie de la complétion de la variable `code` du gabarit `legifrance://code/{code}/article/{num}`
//! (v0.3, M7).
//!
//! La complétion n'est pas auditée (00 §7) : la sortie n'a donc pas de métrique d'audit. Elle ne
//! porte que ce que l'adapter MCP met dans `completion` (05 §8.4) : les valeurs, leur nombre total
//! quand il est connu, et l'indication qu'il en reste.

/// Ce que `CompleteUseCase` rend : au plus 100 slugs de codes triés par pertinence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompleteOutput {
    /// Slugs proposés (`code-civil`, `code-du-travail`…), préfixes d'abord, 100 au plus.
    pub values: Vec<String>,
    /// Nombre de slugs qui correspondent à la saisie, tous confondus. `None` quand la liste est
    /// vide faute de réponse (variable inconnue, débit dépassé, erreur amont) : aucune valeur
    /// n'est alors annoncée.
    pub total: Option<usize>,
    /// Il reste des correspondances au-delà des valeurs rendues.
    pub has_more: bool,
}

impl CompleteOutput {
    /// La liste vide sans information : rien à proposer.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_empty_output_announces_nothing() {
        let empty = CompleteOutput::empty();
        assert_eq!(empty, CompleteOutput { values: Vec::new(), total: None, has_more: false });
        assert_eq!(empty, CompleteOutput::default());
    }
}
