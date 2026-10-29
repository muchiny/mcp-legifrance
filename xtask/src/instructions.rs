//! `cargo xtask instructions-size` : taille des instructions du serveur (05 §4, budget D-33).
//!
//! STUB(V0) → V1-10 : V1-10 écrit `run` (interroge `server/discover`, vérifie que les
//! instructions font au plus 3 500 caractères Unicode, affiche la marge et la taille totale des
//! descripteurs).

/// Mesure les instructions du serveur et échoue au-delà de 3 500 caractères.
///
/// # Errors
///
/// Le message de l'échec. Tant que V1-10 n'a pas écrit la mesure, toujours une erreur.
pub fn run() -> Result<(), String> {
    // STUB(V0) → V1-10 : lecture de `server/discover` sur le binaire, comptage des caractères.
    Err("instructions-size: pas encore implémenté (écrit par V1-10)".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_is_a_stub_that_fails_loudly() {
        assert!(run().unwrap_err().contains("pas encore implémenté"));
    }
}
