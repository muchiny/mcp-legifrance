//! Complétions MCP (`completion/complete`).
//!
//! STUB(V0) → V3-07 : stub fonctionnel. Aucune valeur n'est proposée en v0.1 ; V3-07 complète les
//! arguments des gabarits de resources (au plus 100 valeurs, 10 complétions par seconde, 02 §6.2).

use rmcp::model::CompleteResult;

/// `completion/complete` : aucune valeur à proposer.
pub(crate) fn complete() -> CompleteResult {
    CompleteResult::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_value_is_proposed() {
        assert_eq!(complete().completion.values.len(), 0);
    }
}
