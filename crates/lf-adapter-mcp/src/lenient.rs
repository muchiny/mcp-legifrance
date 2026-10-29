//! Désérialiseurs tolérants des paramètres d'outils (05 §3.3).
//!
//! STUB(V0) → V1-09 : seul l'entier JSON est lu pour l'instant. V1-09 ajoute le flottant à
//! partie fractionnaire nulle (`10.0`), la chaîne décimale (`"10"`) et le dépassement des bornes
//! d'un `i64`, qui rend une erreur de bornes et jamais une panique.

use serde::{Deserialize, Deserializer};

/// Entier optionnel d'un paramètre : `#[serde(default, deserialize_with = "lenient::opt_int")]`.
/// `null` vaut « absent » (des clients envoient `null`, 05 §3.3) ; les bornes sont vérifiées par
/// le cas d'usage, qui rend le message de parité.
///
/// # Errors
///
/// L'erreur du désérialiseur si la valeur n'est ni `null` ni un entier JSON.
pub(crate) fn opt_int<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    // STUB(V0) → V1-09 : entiers JSON seuls.
    Option::<i64>::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Paramètres témoins : un entier tolérant, absent par défaut.
    #[derive(Deserialize)]
    struct Params {
        #[serde(default, deserialize_with = "opt_int")]
        page_size: Option<i64>,
    }

    fn page_size(value: &serde_json::Value) -> Option<i64> {
        Params::deserialize(value).unwrap().page_size
    }

    #[test]
    fn json_integer_is_read() {
        assert_eq!(page_size(&json!({"page_size": 10})), Some(10));
        assert_eq!(page_size(&json!({"page_size": -3})), Some(-3));
    }

    #[test]
    fn null_and_missing_are_absent() {
        assert_eq!(page_size(&json!({"page_size": null})), None);
        assert_eq!(page_size(&json!({})), None);
    }

    #[test]
    fn a_sequence_is_an_error_naming_the_field() {
        let error =
            serde_path_to_error::deserialize::<_, Params>(json!({"page_size": [1]})).err().unwrap();
        assert_eq!(error.path().to_string(), "page_size");
    }
}
