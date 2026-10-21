//! Lecteurs serde tolérants (04 §8.2).

use serde::{Deserialize, Deserializer, de::IgnoredAny};

/// Valeur JSON lue sans jamais échouer sur un JSON bien formé.
#[derive(Deserialize)]
#[serde(untagged)]
enum Lenient {
    /// Entier non négatif qui tient dans un `u64`.
    Number(u64),
    /// Texte, à lire comme un nombre s'il n'est fait que de chiffres.
    Text(String),
    /// Toute autre valeur (négatif, flottant, booléen, tableau, objet, `null`, trop grand) :
    /// ignorée, ce qui rend la fonction totale.
    Other(IgnoredAny),
}

/// Lit un entier positif donné en nombre ou en chaîne de chiffres ; tout le reste donne `None`.
///
/// Un champ absent, `null`, négatif, flottant, hors d'un `u64` ou d'un autre type ne fait
/// jamais échouer le document : seul un JSON mal formé est une erreur. Pour une chaîne, seuls
/// les chiffres ASCII comptent, ce qui écarte un signe, un espace ou une notation
/// scientifique (`u64::from_str` accepte un `+` initial, d'où le test des octets avant).
///
/// # Errors
///
/// L'erreur du désérialiseur si le JSON lui-même est mal formé.
pub(crate) fn opt_u64_lenient<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    Ok(match Lenient::deserialize(deserializer)? {
        Lenient::Number(number) => Some(number),
        Lenient::Text(text) if text.bytes().all(|byte| byte.is_ascii_digit()) => text.parse().ok(),
        Lenient::Text(_) | Lenient::Other(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Holder {
        #[serde(default, deserialize_with = "opt_u64_lenient")]
        value: Option<u64>,
    }

    /// Lit `{"value":<raw>}` ; `raw` est du JSON brut.
    fn read(raw: &str) -> Option<u64> {
        let holder: Holder = serde_json::from_str(&format!(r#"{{"value":{raw}}}"#)).unwrap();
        holder.value
    }

    #[test]
    fn a_json_number_is_kept() {
        assert_eq!(
            ["3600", "0", "18446744073709551615"].map(read),
            [Some(3600), Some(0), Some(u64::MAX)]
        );
    }

    #[test]
    fn a_numeric_string_is_parsed() {
        assert_eq!([r#""3600""#, r#""0""#, r#""0042""#].map(read), [Some(3600), Some(0), Some(42)]);
    }

    #[test]
    fn null_and_absence_are_none() {
        let absent: Holder = serde_json::from_str("{}").unwrap();
        assert_eq!((read("null"), absent.value), (None, None));
    }

    #[test]
    fn strings_that_are_not_plain_digits_are_none() {
        let raws = [
            r#""""#,
            r#"" ""#,
            r#""+5""#,
            r#""-5""#,
            r#"" 5""#,
            r#""5 ""#,
            r#""5.0""#,
            r#""1e3""#,
            r#""abc""#,
            r#""0x10""#,
            r#""٣٦٠٠""#,
            r#""18446744073709551616""#,
        ];
        assert_eq!(raws.map(read), [None; 12]);
    }

    #[test]
    fn other_json_values_are_none_without_failing_the_document() {
        let raws = ["-1", "3600.5", "3600.0", "1e3", "true", "[1,2]", "{}", "18446744073709551616"];
        assert_eq!(raws.map(read), [None; 8]);
    }

    #[test]
    fn broken_json_is_still_an_error() {
        assert_eq!(
            [
                serde_json::from_slice::<Holder>(b"tru").is_err(),
                serde_json::from_slice::<Holder>(br#"{"value":[1,}"#).is_err(),
            ],
            [true, true]
        );
    }
}
