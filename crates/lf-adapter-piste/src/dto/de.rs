//! Lecteurs serde tolérants (04 §8.2).

use serde::{Deserialize, Deserializer, de::IgnoredAny};

/// Lit une valeur ou `null` et rend la valeur par défaut du type pour `null`.
///
/// À employer avec `#[serde(default)]` : un champ absent et un champ `null` donnent alors la même
/// valeur par défaut (liste vide…). Une valeur d'un autre type reste une erreur.
///
/// # Errors
///
/// L'erreur du désérialiseur si la valeur n'est pas du type `T`.
pub(crate) fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

/// Lit une chaîne facultative ; une chaîne vide ou blanche (blancs Unicode) donne `None`.
///
/// Le texte non blanc est rendu tel quel, espaces compris : le texte juridique n'est jamais
/// retouché dans le DTO. Une valeur qui n'est pas une chaîne reste une erreur de type.
///
/// # Errors
///
/// L'erreur du désérialiseur si la valeur n'est ni `null` ni une chaîne.
pub(crate) fn opt_nonempty<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.filter(|text| !is_blank(text)))
}

/// Vrai si le texte est vide ou ne contient que des blancs Unicode.
fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// Date brute lue dans un corps, avant tout calcul de calendrier (04 §8.2, §8.4).
///
/// Le mapping (plan 2e) la convertit en date civile ; ce module ne le fait pas. Le texte reçu
/// peut venir du corps : un type qui contient des `RawDate` ne se journalise pas avec `{:?}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawDate {
    /// Millisecondes depuis l'époque Unix, lues en entier, en flottant entier ou en chaîne de
    /// chiffres ASCII.
    Millis(i64),
    /// Texte reçu tel quel (`2024-03-15`, `2024-03-15T00:00:00.000+0000`…).
    Text(String),
}

/// Date JSON : nombre, chaîne ou autre chose, sans échouer sur un JSON bien formé de profondeur
/// ordinaire.
///
/// Un `untagged` met la valeur en mémoire tampon avant d'essayer les variantes : la limite de 128
/// niveaux d'imbrication de `serde_json` s'applique encore, et un sous-arbre plus profond est une
/// erreur de décodage.
#[derive(Deserialize)]
#[serde(untagged)]
enum DateInput {
    /// Entier signé qui tient dans un `i64`.
    Integer(i64),
    /// Flottant, ou entier trop grand pour un `i64`.
    Float(f64),
    /// Texte.
    Text(String),
    /// Toute autre valeur (booléen, objet, tableau, `null`) : ignorée.
    Other(IgnoredAny),
}

/// Lit une date facultative : millisecondes (nombre ou chaîne de chiffres) ou texte.
///
/// Un booléen, un objet, un tableau, une chaîne blanche, un flottant fractionnaire et un nombre
/// hors d'un `i64` donnent `None`. Aucune de ces tolérances n'est comptée ici : le décompte
/// éventuel appartient au mapping. Seuls un JSON mal formé et une imbrication de plus de 128
/// niveaux sont une erreur.
///
/// # Errors
///
/// L'erreur du désérialiseur si le JSON lui-même est mal formé.
pub(crate) fn opt_raw_date<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RawDate>, D::Error> {
    Ok(match DateInput::deserialize(deserializer)? {
        DateInput::Integer(millis) => Some(RawDate::Millis(millis)),
        DateInput::Float(value) => whole_float_to_millis(value).map(RawDate::Millis),
        DateInput::Text(text) => text_to_date(text),
        DateInput::Other(_) => None,
    })
}

/// Millisecondes d'un flottant qui est un entier exact et tient dans un `i64`.
///
/// Passe par le texte et non par `as i64`, qui sature en silence.
fn whole_float_to_millis(value: f64) -> Option<i64> {
    if value.fract() == 0.0 { format!("{value:.0}").parse().ok() } else { None }
}

/// Texte de date : blanc donne `None`, une suite de chiffres ASCII donne des millisecondes
/// (`None` si elle déborde un `i64`), le reste est gardé tel quel.
fn text_to_date(text: String) -> Option<RawDate> {
    if is_blank(&text) {
        None
    } else if text.bytes().all(|byte| byte.is_ascii_digit()) {
        text.parse().ok().map(RawDate::Millis)
    } else {
        Some(RawDate::Text(text))
    }
}

/// Code JSON : chaîne, nombre ou autre chose.
#[derive(Deserialize)]
#[serde(untagged)]
enum CodeInput {
    /// Texte.
    Text(String),
    /// Nombre JSON.
    Number(serde_json::Number),
    /// Toute autre valeur : ignorée.
    Other(IgnoredAny),
}

/// Lit un code (IDCC…) donné en chaîne ou en entier, rendu en chaîne.
///
/// Une chaîne non blanche est gardée telle quelle (zéros initiaux compris) ; un entier, signé ou
/// non, est écrit en décimal. Une chaîne blanche, un flottant, un booléen, un tableau, un objet
/// et `null` donnent `None`. Seuls un JSON mal formé et une imbrication de plus de 128 niveaux sont
/// une erreur.
///
/// # Errors
///
/// L'erreur du désérialiseur si le JSON lui-même est mal formé.
pub(crate) fn opt_string_or_number<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(match CodeInput::deserialize(deserializer)? {
        CodeInput::Text(text) if !is_blank(&text) => Some(text),
        CodeInput::Number(number) if !number.is_f64() => Some(number.to_string()),
        CodeInput::Text(_) | CodeInput::Number(_) | CodeInput::Other(_) => None,
    })
}

/// Valeur JSON lue sans échouer sur un JSON bien formé de profondeur ordinaire.
///
/// Même réserve que `DateInput` : un `untagged` met la valeur en mémoire tampon, et la limite de
/// 128 niveaux d'imbrication de `serde_json` s'applique encore.
#[derive(Deserialize)]
#[serde(untagged)]
enum Lenient {
    /// Entier non négatif qui tient dans un `u64`.
    Number(u64),
    /// Texte, à lire comme un nombre s'il n'est fait que de chiffres.
    Text(String),
    /// Toute autre valeur (négatif, flottant, booléen, tableau, objet, `null`, trop grand) :
    /// ignorée.
    Other(IgnoredAny),
}

/// Lit un entier positif donné en nombre ou en chaîne de chiffres ; tout le reste donne `None`.
///
/// Un champ absent, `null`, négatif, flottant, hors d'un `u64` ou d'un autre type ne fait
/// pas échouer le document : seuls un JSON mal formé et une imbrication de plus de 128 niveaux
/// sont une erreur. Pour une chaîne, seuls
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

    #[derive(Deserialize)]
    struct Listed {
        #[serde(default, deserialize_with = "null_default")]
        items: Vec<String>,
    }

    #[derive(Deserialize)]
    struct Named {
        #[serde(default, deserialize_with = "opt_nonempty")]
        name: Option<String>,
    }

    #[derive(Deserialize)]
    struct Dated {
        #[serde(default, deserialize_with = "opt_raw_date")]
        date: Option<RawDate>,
    }

    #[derive(Deserialize)]
    struct Coded {
        #[serde(default, deserialize_with = "opt_string_or_number")]
        code: Option<String>,
    }

    /// Lit `{"items":<raw>}`.
    fn items(raw: &str) -> Vec<String> {
        serde_json::from_str::<Listed>(&format!(r#"{{"items":{raw}}}"#)).unwrap().items
    }

    /// Lit `{"name":<raw>}`.
    fn name(raw: &str) -> Option<String> {
        serde_json::from_str::<Named>(&format!(r#"{{"name":{raw}}}"#)).unwrap().name
    }

    /// Lit `{"date":<raw>}`.
    fn date(raw: &str) -> Option<RawDate> {
        serde_json::from_str::<Dated>(&format!(r#"{{"date":{raw}}}"#)).unwrap().date
    }

    /// Lit `{"code":<raw>}`.
    fn code(raw: &str) -> Option<String> {
        serde_json::from_str::<Coded>(&format!(r#"{{"code":{raw}}}"#)).unwrap().code
    }

    fn millis(value: i64) -> RawDate {
        RawDate::Millis(value)
    }

    fn text(value: &str) -> RawDate {
        RawDate::Text(value.to_owned())
    }

    #[test]
    fn null_list_is_empty_vec() {
        assert_eq!(items("null"), Vec::<String>::new());
    }

    #[test]
    fn missing_list_is_empty_vec() {
        assert_eq!(serde_json::from_str::<Listed>("{}").unwrap().items, Vec::<String>::new());
    }

    #[test]
    fn a_present_list_is_kept() {
        assert_eq!(items(r#"["a","","b"]"#), ["a", "", "b"]);
    }

    #[test]
    fn a_list_of_the_wrong_type_is_still_an_error() {
        assert!(serde_json::from_str::<Listed>(r#"{"items":"a"}"#).is_err());
    }

    #[test]
    fn empty_string_is_none() {
        let raws = [r#""""#, r#""   ""#, r#""\t\n""#, "null"];
        assert_eq!(
            (raws.map(name), serde_json::from_str::<Named>("{}").unwrap().name),
            ([const { None }; 4], None)
        );
    }

    #[test]
    fn a_non_blank_string_is_kept_untouched() {
        let raws = [r#""abc""#, r#"" a b ""#, r#""\n x""#];
        assert_eq!(
            raws.map(name),
            [Some("abc".to_owned()), Some(" a b ".to_owned()), Some("\n x".to_owned())]
        );
    }

    #[test]
    fn a_non_string_is_still_an_error() {
        let raws = ["12", "true", "[]"];
        assert_eq!(
            raws.map(|raw| serde_json::from_str::<Named>(&format!(r#"{{"name":{raw}}}"#)).is_err()),
            [true; 3]
        );
    }

    #[test]
    fn raw_date_accepts_ms_numeric_string_ymd_iso() {
        let raws = [
            "1710460800000",
            r#""1710460800000""#,
            r#""2024-03-15""#,
            r#""2024-03-15T00:00:00.000+0000""#,
        ];
        assert_eq!(
            raws.map(date),
            [
                Some(millis(1_710_460_800_000)),
                Some(millis(1_710_460_800_000)),
                Some(text("2024-03-15")),
                Some(text("2024-03-15T00:00:00.000+0000")),
            ]
        );
    }

    #[test]
    fn negative_ms_1804() {
        assert_eq!(
            ["-5231606400000", "0", "-1"].map(date),
            [Some(millis(-5_231_606_400_000)), Some(millis(0)), Some(millis(-1))]
        );
    }

    #[test]
    fn bool_is_not_a_date() {
        let absent = serde_json::from_str::<Dated>("{}").unwrap().date;
        assert_eq!((["true", "false", "null"].map(date), absent), ([const { None }; 3], None));
    }

    #[test]
    fn a_whole_float_is_milliseconds_and_a_fractional_one_is_none() {
        assert_eq!(
            ["1710460800000.0", "1.7104608e12", "-5231606400000.0", "1710460800000.5", "0.5"]
                .map(date),
            [
                Some(millis(1_710_460_800_000)),
                Some(millis(1_710_460_800_000)),
                Some(millis(-5_231_606_400_000)),
                None,
                None,
            ]
        );
    }

    #[test]
    fn numbers_beyond_i64_are_none() {
        let raws = [
            "9223372036854775808",
            "18446744073709551615",
            "1e30",
            "-1e30",
            r#""99999999999999999999""#,
        ];
        assert_eq!(raws.map(date), [const { None }; 5]);
    }

    #[test]
    fn blank_strings_objects_and_arrays_are_none() {
        let raws = [r#""""#, r#""  ""#, "{}", r#"{"a":1}"#, "[]", "[1710460800000]"];
        assert_eq!(raws.map(date), [const { None }; 6]);
    }

    #[test]
    fn a_non_numeric_string_is_kept_as_text() {
        let raws = [r#"" 2024-03-15""#, r#""-5231606400000""#, r#""12 ""#, r#""٣""#];
        assert_eq!(
            raws.map(date),
            [
                Some(text(" 2024-03-15")),
                Some(text("-5231606400000")),
                Some(text("12 ")),
                Some(text("٣")),
            ]
        );
    }

    #[test]
    fn broken_json_in_a_date_is_still_an_error() {
        assert_eq!(
            [
                serde_json::from_str::<Dated>(r#"{"date":tru}"#).is_err(),
                serde_json::from_str::<Dated>(r#"{"date":[1,}"#).is_err(),
            ],
            [true, true]
        );
    }

    #[test]
    fn idcc_accepts_string_and_number() {
        let raws =
            ["1261", r#""1261""#, r#""0044""#, "0", "18446744073709551615", "-7", r#"" x ""#];
        assert_eq!(
            raws.map(code),
            [
                Some("1261".to_owned()),
                Some("1261".to_owned()),
                Some("0044".to_owned()),
                Some("0".to_owned()),
                Some("18446744073709551615".to_owned()),
                Some("-7".to_owned()),
                Some(" x ".to_owned()),
            ]
        );
    }

    #[test]
    fn idcc_other_values_are_none() {
        let raws = ["null", r#""""#, r#""  ""#, "12.5", "1e3", "true", "[]", "{}"];
        let absent = serde_json::from_str::<Coded>("{}").unwrap().code;
        assert_eq!((raws.map(code), absent), ([const { None }; 8], None));
    }

    #[test]
    fn broken_json_in_a_code_is_still_an_error() {
        assert_eq!(
            [
                serde_json::from_str::<Coded>(r#"{"code":tru}"#).is_err(),
                serde_json::from_str::<Coded>(r#"{"code":[1,}"#).is_err(),
            ],
            [true, true]
        );
    }
}
