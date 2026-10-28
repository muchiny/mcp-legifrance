//! Côté entrant : le seul contrat d'entrée de l'application est [`ToolCall`] (02 §5.4).

use std::{fmt, sync::Arc};

/// Un appel d'outil tel que reçu : l'adapter entrant a tenté le décodage, mais c'est
/// l'application qui audite le résultat. Une erreur de décodage devient donc un `invalid_arg`
/// audité (E-29, D-02).
#[derive(Debug, Clone)]
pub struct ToolCall<I> {
    /// L'entrée décodée, ou la raison pour laquelle elle ne l'a pas été.
    pub input: Result<I, InputError>,
    /// Les arguments bruts en JSON canonique, pour l'audit (calcul de `input_hash`).
    pub canonical: CanonicalInput,
}

/// JSON canonique de l'objet `arguments` brut. Le clone ne copie pas le texte : l'appel, la
/// ligne d'audit et la trace d'un même appel le partagent.
#[derive(Debug, Clone)]
pub struct CanonicalInput(Arc<str>);

impl CanonicalInput {
    /// Enveloppe un texte déjà canonique ; la canonicalisation est faite par l'adapter entrant.
    #[must_use]
    pub fn new(text: impl Into<Arc<str>>) -> Self {
        Self(text.into())
    }

    /// Le texte JSON.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Échec de décodage des arguments, ex. `page_size: invalid type: string "dix", expected u32`.
/// Le message est celui que le cas d'usage rend au modèle en `invalid_arg`.
#[derive(Debug, Clone)]
pub struct InputError {
    /// Message de décodage, sans valeur secrète : il cite les arguments du modèle, rien d'autre.
    pub message: String,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    #[test]
    fn canonical_input_clones_share_the_text() {
        let input = CanonicalInput::new(r#"{"query":"bail"}"#);
        let clone = input.clone();
        assert_eq!(clone.as_str(), r#"{"query":"bail"}"#);
        assert!(Arc::ptr_eq(&input.0, &clone.0));
        let shared: Arc<str> = Arc::from("{}");
        assert!(Arc::ptr_eq(&CanonicalInput::new(Arc::clone(&shared)).0, &shared));
    }

    #[test]
    fn input_error_displays_its_message() {
        let error = InputError { message: "page_size: invalid type".to_owned() };
        assert_eq!(error.to_string(), "page_size: invalid type");
    }

    #[test]
    fn tool_call_carries_the_decoding_result_and_the_canonical_text() {
        let call: ToolCall<u32> = ToolCall {
            input: Err(InputError { message: "bad".to_owned() }),
            canonical: CanonicalInput::new("{}"),
        };
        assert_eq!(call.input.unwrap_err().message, "bad");
        assert_eq!(call.canonical.as_str(), "{}");
    }
}
