//! Désérialiseurs tolérants des paramètres d'outils (05 §3.3).
//!
//! Les clients MCP envoient les entiers sous plusieurs formes : le nombre entier (`10`), le
//! flottant sans partie fractionnaire (`10.0`) et la chaîne décimale (`"10"`) ; certains
//! envoient `null` pour un paramètre qu'ils ne renseignent pas. [`opt_int`] lit les trois formes
//! et traite `null` comme « absent ». Tout le reste (flottant fractionnaire, chaîne qui n'est pas
//! un entier décimal, booléen, tableau, objet) est une erreur de décodage, que `decode::to_call`
//! rend en `invalid_arg` avec le chemin du champ.
//!
//! Un entier hors des bornes d'un `i64` ne panique pas et ne déborde pas : il **sature** à
//! `i64::MIN` ou `i64::MAX`. Aucun paramètre n'admet ces valeurs, donc le cas d'usage, qui
//! vérifie les bornes dans l'ordre de la référence, rend son message de parité
//! (`page_size=9223372036854775807 out of range (1..100).`, 05 §7.3) au lieu d'une erreur de
//! décodage hors de cet ordre.

use std::fmt;

use serde::{
    Deserializer,
    de::{Error, Unexpected, Visitor},
};

/// Entier optionnel d'un paramètre : `#[serde(default, deserialize_with = "lenient::opt_int")]`.
/// `null` vaut « absent » (des clients envoient `null`, 05 §3.3) ; les bornes sont vérifiées par
/// le cas d'usage, qui rend le message de parité. Formes lues : entier JSON, flottant à partie
/// fractionnaire nulle, chaîne décimale (signe `+` ou `-` facultatif, chiffres seuls) ; hors d'un
/// `i64`, la valeur sature (voir la documentation du module).
///
/// # Errors
///
/// L'erreur du désérialiseur si la valeur n'est ni `null` ni un entier sous l'une de ces formes,
/// par exemple `invalid type: sequence, expected an integer`.
pub(crate) fn opt_int<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    deserializer.deserialize_option(OptInt)
}

/// Visiteur de [`opt_int`]. Un seul visiteur traite l'option (`visit_none`, `visit_unit`,
/// `visit_some`) et la valeur (`visit_i64`…), pour que les formats qui n'enveloppent pas la valeur
/// dans une option (contenu tamponné d'un `#[serde(flatten)]`) passent par le même code.
struct OptInt;

impl<'de> Visitor<'de> for OptInt {
    type Value = Option<i64>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an integer")
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Some(value))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Some(i64::try_from(value).unwrap_or(i64::MAX)))
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Self::Value, E> {
        // `fract()` vaut NaN pour l'infini et le NaN : ils ne passent pas (JSON n'en écrit pas).
        if value.fract() == 0.0 {
            Ok(Some(saturating(value)))
        } else {
            Err(E::invalid_value(Unexpected::Float(value), &self))
        }
    }

    fn visit_str<E: Error>(self, text: &str) -> Result<Self::Value, E> {
        decimal(text).map(Some).ok_or_else(|| E::invalid_value(Unexpected::Str(text), &self))
    }
}

/// Flottant entier vers `i64`, saturant : hors des bornes, `i64::MIN` ou `i64::MAX`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    reason = "la conversion `as` d'un flottant sature : c'est la lecture voulue des entiers hors bornes"
)]
fn saturating(value: f64) -> i64 {
    value as i64
}

/// Chaîne décimale vers `i64` : signe facultatif puis un ou plusieurs chiffres ASCII, rien
/// d'autre (ni espace, ni séparateur, ni exposant, ni point). Hors d'un `i64`, elle sature.
/// `None` si la chaîne n'est pas un entier décimal.
fn decimal(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    // Les chiffres sont vérifiés d'abord : `parse` signalerait un dépassement avant de voir un
    // caractère étranger placé après les vingt premiers chiffres.
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let overflow = if negative { i64::MIN } else { i64::MAX };
    Some(text.parse().unwrap_or(overflow))
}

#[cfg(test)]
mod tests {
    use lf_application::{budget::ResponseBudget, messages};
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::{Value, json};

    use super::*;

    /// Paramètres témoins : un entier tolérant, absent par défaut.
    #[derive(Deserialize)]
    struct Params {
        #[serde(default, deserialize_with = "opt_int")]
        page_size: Option<i64>,
    }

    /// Les trois entiers de `ReadingParams` (`params/reading.rs`, privé à `params`), à aplatir.
    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Reading {
        #[serde(default, deserialize_with = "opt_int")]
        offset: Option<i64>,
        #[serde(default, deserialize_with = "opt_int")]
        max_chars: Option<i64>,
        #[serde(default, deserialize_with = "opt_int")]
        passages: Option<i64>,
    }

    /// Paramètres d'une lecture longue : l'identifiant, plus les champs de lecture aplatis. Les
    /// valeurs passent alors par le contenu tamponné de serde, pas par la valeur JSON directe.
    #[derive(Deserialize)]
    struct Flattened {
        id: Option<String>,
        #[serde(flatten)]
        reading: Reading,
    }

    /// Décode `{"page_size": <json>}` depuis le texte JSON, comme sur le fil.
    fn wire(json: &str) -> Result<Option<i64>, String> {
        let text = format!(r#"{{"page_size": {json}}}"#);
        let value: Value = serde_json::from_str(&text).unwrap();
        serde_path_to_error::deserialize::<_, Params>(value)
            .map(|params| params.page_size)
            .map_err(|error| error.to_string())
    }

    fn page_size(value: &Value) -> Option<i64> {
        Params::deserialize(value).unwrap().page_size
    }

    #[test]
    fn json_integer_is_read() {
        assert_eq!(page_size(&json!({"page_size": 10})), Some(10));
        assert_eq!(page_size(&json!({"page_size": -3})), Some(-3));
        assert_eq!(page_size(&json!({"page_size": 0})), Some(0));
    }

    #[test]
    fn lenient_int_accepts_integer_float_and_decimal_string() {
        // Les trois formes de 05 §3.3 donnent la même valeur : 10, 10.0 et "10".
        for json in ["10", "10.0", r#""10""#, "1e1", "10.000", r#""+10""#, r#""010""#] {
            assert_eq!(wire(json), Ok(Some(10)), "{json}");
        }
        for json in ["-3", "-3.0", r#""-3""#, "-3e0", "-0.3e1"] {
            assert_eq!(wire(json), Ok(Some(-3)), "{json}");
        }
        // Zéro, y compris signé : `-0.0` n'a pas de partie fractionnaire.
        for json in ["0", "0.0", "-0.0", r#""0""#, r#""-0""#] {
            assert_eq!(wire(json), Ok(Some(0)), "{json}");
        }
        // Les extrêmes d'un i64 sont lus exacts, sans passer par un flottant.
        assert_eq!(wire("9223372036854775807"), Ok(Some(i64::MAX)));
        assert_eq!(wire("-9223372036854775808"), Ok(Some(i64::MIN)));
        assert_eq!(wire(r#""9223372036854775807""#), Ok(Some(i64::MAX)));
        assert_eq!(wire(r#""-9223372036854775808""#), Ok(Some(i64::MIN)));
        // 2^53 + 1 n'est pas représentable en f64 : l'entier JSON doit rester exact.
        assert_eq!(wire("9007199254740993"), Ok(Some(9_007_199_254_740_993)));
        // La valeur JSON déjà construite passe aussi.
        assert_eq!(page_size(&json!({"page_size": 10.0})), Some(10));
        assert_eq!(page_size(&json!({"page_size": "10"})), Some(10));
    }

    #[test]
    fn a_fractional_float_is_an_error() {
        assert_eq!(
            wire("1.5"),
            Err("page_size: invalid value: floating point `1.5`, expected an integer".to_owned())
        );
        assert_eq!(
            wire("-0.1"),
            Err("page_size: invalid value: floating point `-0.1`, expected an integer".to_owned())
        );
        assert_eq!(
            wire("10.000000001"),
            Err("page_size: invalid value: floating point `10.000000001`, expected an integer"
                .to_owned())
        );
    }

    #[test]
    fn a_string_that_is_not_a_decimal_integer_is_an_error() {
        for text in [
            "x", "", " ", "10.0", " 10", "10 ", "1 0", "1_0", "0x10", "1e1", "+", "-", "--1", "+-1",
        ] {
            let json = Value::String(text.to_owned()).to_string();
            assert_eq!(
                wire(&json),
                Err(format!("page_size: invalid value: string {json}, expected an integer")),
                "{text:?}"
            );
        }
        // Un dépassement suivi de caractères étrangers n'est pas un dépassement : c'est un refus.
        assert_eq!(
            wire(r#""99999999999999999999abc""#),
            Err(r#"page_size: invalid value: string "99999999999999999999abc", expected an integer"#
                .to_owned())
        );
    }

    #[test]
    fn other_json_types_are_errors_naming_the_field() {
        assert_eq!(
            wire("[1]"),
            Err("page_size: invalid type: sequence, expected an integer".to_owned())
        );
        assert_eq!(
            wire(r#"{"a": 1}"#),
            Err("page_size: invalid type: map, expected an integer".to_owned())
        );
        assert_eq!(
            wire("true"),
            Err("page_size: invalid type: boolean `true`, expected an integer".to_owned())
        );
        let error =
            serde_path_to_error::deserialize::<_, Params>(json!({"page_size": [1]})).err().unwrap();
        assert_eq!(error.path().to_string(), "page_size");
    }

    #[test]
    fn null_and_missing_are_absent() {
        assert_eq!(page_size(&json!({"page_size": null})), None);
        assert_eq!(page_size(&json!({})), None);
    }

    #[test]
    fn null_is_absent_for_optional_params() {
        // Valeur JSON directe, puis texte JSON lu sans passer par `Value`.
        assert_eq!(wire("null"), Ok(None));
        let params: Params = serde_json::from_str(r#"{"page_size": null}"#).unwrap();
        assert_eq!(params.page_size, None);
        let params: Params = serde_json::from_str("{}").unwrap();
        assert_eq!(params.page_size, None);
        // Les entiers de lecture à `null` valent « absent » (des clients l'envoient).
        let absent = Reading { offset: None, max_chars: None, passages: None };
        let reading: Reading =
            serde_json::from_str(r#"{"offset": null, "max_chars": null, "passages": null}"#)
                .unwrap();
        assert_eq!(reading, absent);
        // Aplatis dans un outil, `null` traverse le contenu tamponné de serde (variante `Unit`).
        let flattened: Flattened = serde_json::from_str(
            r#"{"id": null, "offset": null, "max_chars": null, "passages": null}"#,
        )
        .unwrap();
        assert_eq!(flattened.id, None);
        assert_eq!(flattened.reading, absent);
    }

    #[test]
    fn lenient_forms_are_read_through_flattened_content_too() {
        let flattened: Flattened = serde_json::from_str(
            r#"{"id": "LEGIARTI000006419280", "offset": "40", "max_chars": 2000.0, "passages": 4}"#,
        )
        .unwrap();
        assert_eq!(flattened.id.as_deref(), Some("LEGIARTI000006419280"));
        assert_eq!(
            flattened.reading,
            Reading { offset: Some(40), max_chars: Some(2000), passages: Some(4) }
        );
        let error = serde_json::from_str::<Flattened>(r#"{"offset": 1.5}"#).err().unwrap();
        assert!(
            error.to_string().contains("invalid value: floating point `1.5`, expected an integer"),
            "{error}"
        );
    }

    #[test]
    fn huge_integer_is_out_of_range_not_panic() {
        // Au-delà d'un i64, la lecture ne panique pas et ne déborde pas : elle sature, et la
        // valeur sort de toutes les bornes, donc le cas d'usage rend son message de parité.
        for json in [
            "9223372036854775808",  // i64::MAX + 1, lu comme u64
            "18446744073709551615", // u64::MAX
            "18446744073709551616", // u64::MAX + 1, lu comme flottant par serde_json
            "99999999999999999999999",
            "1e30",
            "1e300",
            "9.3e18",
            "9223372036854775808.0",
            r#""9223372036854775808""#,
            r#""99999999999999999999999999999999""#,
            r#""+99999999999999999999999""#,
        ] {
            assert_eq!(wire(json), Ok(Some(i64::MAX)), "{json}");
        }
        for json in [
            "-9223372036854775809",
            "-99999999999999999999999",
            "-1e30",
            "-1e300",
            "-9.3e18",
            r#""-9223372036854775809""#,
            r#""-99999999999999999999999999999999""#,
        ] {
            assert_eq!(wire(json), Ok(Some(i64::MIN)), "{json}");
        }
        // Le message de bornes de parité (05 §7.3) est celui du cas d'usage, sur la valeur saturée.
        let (min, max) = (
            i64::try_from(ResponseBudget::MAX_CHARS_MIN).unwrap(),
            i64::try_from(ResponseBudget::MAX_CHARS_MAX).unwrap(),
        );
        let value = wire("1e30").unwrap().unwrap();
        assert!(!(min..=max).contains(&value));
        assert_eq!(
            messages::out_of_range("max_chars", value, min, max).message(),
            "max_chars=9223372036854775807 out of range (500..40000)."
        );
        let value = wire("-1e30").unwrap().unwrap();
        assert!(!(min..=max).contains(&value));
        assert_eq!(
            messages::out_of_range("max_chars", value, min, max).message(),
            "max_chars=-9223372036854775808 out of range (500..40000)."
        );
    }
}
