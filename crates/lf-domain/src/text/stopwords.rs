//! Mots vides du français juridique, écartés de `focus` (03 §7.8).
//!
//! STUB(V0) → V1-05 : aucun mot n'est encore vide.

/// Le mot, déjà replié (minuscules, accents français retirés), est-il un mot vide ?
///
/// STUB(V0) → V1-05 : rend toujours `false`.
pub(crate) fn is_stopword(_folded: &str) -> bool {
    // STUB(V0) → V1-05
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_keeps_every_word() {
        // STUB(V0) → V1-05 : la liste des mots vides est éprouvée là-bas.
        assert!(!is_stopword("le"));
        assert!(!is_stopword("bail"));
    }
}
