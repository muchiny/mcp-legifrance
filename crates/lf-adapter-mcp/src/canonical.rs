//! JSON canonique des arguments bruts, pour l'empreinte d'audit (02 §6.1, 06 §11.4).
//!
//! La forme est celle de `json.dumps(obj, sort_keys=True, separators=(",", ":"))` de Python avec
//! l'échappement `ensure_ascii` (vecteurs P-041) :
//!
//! - clés d'objet triées récursivement par points de code, séparateurs `,` et `:` sans espace ;
//! - chaînes entre guillemets : `\"`, `\\`, `\n`, `\r`, `\t`, `\b` et `\f` pour les cinq contrôles
//!   et les deux délimiteurs que JSON nomme, et `\uXXXX` en hexadécimal **minuscule** pour tout
//!   autre caractère hors de U+0020 à U+007E (DEL, contrôles C0 et C1, tout le non-ASCII), avec une
//!   paire de substitution au-delà du plan de base (U+1F600 donne `\ud83d\ude00`) ; `/` n'est pas
//!   échappé ;
//! - entiers en décimal, `true`, `false`, `null` ;
//! - nombres non entiers au format `repr` de Python (`1.0`, `1e+16`, `1e-05`), que `serde_json`
//!   n'écrit pas ainsi (`1e16`).
//!
//! Ce n'est **pas** la forme du verrou des descripteurs (`ensure_ascii=False`, 05 §9.2), qui a sa
//! propre fonction (06 §11.4, « piège »).
//!
//! Limites de la lecture amont, que ce module ne peut pas corriger : `serde_json` lit un entier
//! hors d'un `u64` ou d'un `i64` comme un flottant (Python en garde les chiffres), et `-0` comme
//! `-0.0` (Python : l'entier `0`). Ces cas sont hors de tout argument d'outil.
//!
//! Le hachage (SHA-256) est fait par `lf-adapter-audit`, qui vérifie les mêmes vecteurs.

use std::{cmp::Ordering, fmt::Write as _};

use lf_application::input::CanonicalInput;
use rmcp::model::JsonObject;
use serde_json::{Number, Value};

/// Chiffres hexadécimaux minuscules de `\uXXXX`.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// JSON canonique de l'objet `arguments` **brut**, avant tout décodage : les arguments invalides
/// ont donc eux aussi une empreinte. Des arguments absents valent `{}` (l'appelant passe alors un
/// objet vide).
pub(crate) fn of(args: &JsonObject) -> CanonicalInput {
    let mut out = String::new();
    push_object(&mut out, args.iter().collect());
    CanonicalInput::new(out)
}

/// Écrit une valeur JSON sous sa forme canonique.
fn push_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => push_number(out, number),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                push_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => push_object(out, map.iter().collect()),
    }
}

/// Écrit un objet, clés triées par points de code. Le tri est fait ici et non laissé à
/// l'itération de la table : il tient même si `preserve_order` de `serde_json` est activé par une
/// autre dépendance. Comparer les octets UTF-8 revient à comparer les points de code.
fn push_object(out: &mut String, mut entries: Vec<(&String, &Value)>) {
    entries.sort_unstable_by(|left, right| compare_keys(left.0, right.0));
    out.push('{');
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        push_value(out, value);
    }
    out.push('}');
}

/// Ordre des clés : celui des points de code (Python trie les `str` ainsi, pas par unités UTF-16).
fn compare_keys(left: &str, right: &str) -> Ordering {
    left.as_bytes().cmp(right.as_bytes())
}

/// Écrit une chaîne avec l'échappement `ensure_ascii` de Python.
#[expect(clippy::indexing_slicing, reason = "indice masqué à 4 bits dans une table de 16 chiffres")]
fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(character),
            _ => {
                // Une unité UTF-16 pour le plan de base, deux (paire de substitution) au-delà.
                for unit in character.encode_utf16(&mut [0; 2]) {
                    out.push_str("\\u");
                    for shift in [12, 8, 4, 0] {
                        out.push(char::from(HEX[usize::from((*unit >> shift) & 0xf)]));
                    }
                }
            }
        }
    }
    out.push('"');
}

/// Écrit un nombre : un entier en décimal, un flottant au format `repr` de Python.
fn push_number(out: &mut String, number: &Number) {
    if number.is_f64() {
        push_float(out, number.as_f64().unwrap_or_default());
    } else {
        out.push_str(&number.to_string());
    }
}

/// Écrit un flottant fini comme le `repr` de Python 3 : les plus courts chiffres qui reviennent à
/// la même valeur, en notation positionnelle quand l'exposant décimal reste dans `-4 < e <= 16`
/// (`0.0001`, `1000000000000000.0`), sinon en notation scientifique à exposant signé d'au moins
/// deux chiffres (`1e+16`, `1e-05`, `1.5e-07`). Rust et Python choisissent les mêmes chiffres les
/// plus courts, le plus proche de la valeur exacte.
fn push_float(out: &mut String, value: f64) {
    if value.is_sign_negative() {
        out.push('-');
    }
    let magnitude = value.abs();
    if magnitude == 0.0 {
        out.push_str("0.0");
        return;
    }
    let (digits, exponent) = shortest_digits(magnitude);
    // La valeur vaut 0.CHIFFRES × 10^point : `point` est la place de la virgule dans les chiffres.
    let point = exponent.saturating_add(1);
    if (-3..=16).contains(&point) {
        push_positional(out, &digits, point);
    } else {
        push_scientific(out, &digits, exponent);
    }
}

/// Les chiffres décimaux les plus courts qui reviennent à `magnitude` (un flottant fini et
/// positif), et l'exposant décimal du premier : `1.5e-7` donne `("15", -7)`.
///
/// `{:e}` donne déjà les chiffres les plus courts, mais départage deux candidats équidistants de
/// la valeur exacte vers le haut (`2049908751614195.25` donne `…195.3`), là où Python prend le
/// chiffre pair (`…195.2`). L'arrondi exact du même nombre de chiffres, `{:.N e}`, arrondit au
/// pair ; il remplace le résultat quand il revient bien à la même valeur. Il peut ne pas revenir à
/// la valeur aux puissances de deux, où l'intervalle d'arrondi est asymétrique : la forme la plus
/// courte reste alors celle de Python (`7.120236347223045e-307`).
fn shortest_digits(magnitude: f64) -> (String, i32) {
    let shortest = format!("{magnitude:e}");
    let places = scientific_parts(&shortest).0.len().saturating_sub(1);
    let rounded = format!("{magnitude:.places$e}");
    let reads_back = rounded.parse::<f64>().is_ok_and(|back| back.to_bits() == magnitude.to_bits());
    scientific_parts(if reads_back { &rounded } else { &shortest })
}

/// Sépare une notation `d.ddde-7` de Rust en chiffres (sans la virgule) et exposant.
fn scientific_parts(scientific: &str) -> (String, i32) {
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific, "0"));
    (mantissa.chars().filter(char::is_ascii_digit).collect(), exponent.parse().unwrap_or_default())
}

/// Notation positionnelle : `point` chiffres avant la virgule ; si `point` est nul ou négatif, un
/// `0.` suivi de `-point` zéros précède les chiffres.
fn push_positional(out: &mut String, digits: &str, point: i32) {
    let places = usize::try_from(point.unsigned_abs()).unwrap_or_default();
    if point <= 0 {
        // `places` zéros entre la virgule et les chiffres : 0.0001 = 0.CHIFFRES avec point = -3.
        out.push_str("0.");
        out.push_str(&"0".repeat(places));
        out.push_str(digits);
    } else if places >= digits.len() {
        out.push_str(digits);
        out.push_str(&"0".repeat(places.saturating_sub(digits.len())));
        out.push_str(".0");
    } else {
        let (integer, fraction) = digits.split_at(places);
        out.push_str(integer);
        out.push('.');
        out.push_str(fraction);
    }
}

/// Notation scientifique : `d[.ddd]e±XX`, exposant d'au moins deux chiffres.
fn push_scientific(out: &mut String, digits: &str, exponent: i32) {
    let (first, rest) = digits.split_at_checked(1).unwrap_or((digits, ""));
    out.push_str(first);
    if !rest.is_empty() {
        out.push('.');
        out.push_str(rest);
    }
    out.push('e');
    out.push(if exponent < 0 { '-' } else { '+' });
    // Deux chiffres au moins (`e-05`, `e+16`), comme le `%+.02d` de Python. L'écriture dans un
    // `String` ne peut pas échouer.
    let _ = write!(out, "{:02}", exponent.unsigned_abs());
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Forme canonique d'un objet JSON (le texte est lu comme sur le fil).
    fn canonical(value: &Value) -> String {
        of(value.as_object().unwrap()).as_str().to_owned()
    }

    fn canonical_text(json: &str) -> String {
        canonical(&serde_json::from_str(json).unwrap())
    }

    /// Forme canonique d'un seul nombre, lu depuis son texte JSON : `{"n": <json>}` sans l'enveloppe.
    fn number(json: &str) -> String {
        let canonical = canonical_text(&format!(r#"{{"n": {json}}}"#));
        canonical
            .strip_prefix(r#"{"n":"#)
            .and_then(|rest| rest.strip_suffix('}'))
            .unwrap()
            .to_owned()
    }

    /// Le signe de l'exposant : `-` seulement s'il est négatif, `+` sinon, zéro compris (que
    /// `push_float` n'envoie jamais ici, mais que la fonction sait écrire).
    #[test]
    fn scientific_notation_signs_and_pads_the_exponent() {
        let scientific = |digits: &str, exponent: i32| {
            let mut out = String::new();
            push_scientific(&mut out, digits, exponent);
            out
        };
        assert_eq!(scientific("1", 0), "1e+00");
        assert_eq!(scientific("15", 0), "1.5e+00");
        assert_eq!(scientific("1", 16), "1e+16");
        assert_eq!(scientific("15", -7), "1.5e-07");
        assert_eq!(scientific("1", -5), "1e-05");
    }

    #[test]
    fn keys_are_sorted_and_the_output_is_compact() {
        assert_eq!(
            canonical(&json!({"b": 1, "a": [true, null], "c": "x"})),
            r#"{"a":[true,null],"b":1,"c":"x"}"#
        );
    }

    #[test]
    fn nested_objects_are_sorted_too() {
        assert_eq!(canonical(&json!({"z": {"b": 1, "a": 2}})), r#"{"z":{"a":2,"b":1}}"#);
    }

    #[test]
    fn empty_arguments_give_an_empty_object() {
        assert_eq!(canonical(&json!({})), "{}");
    }

    /// Les 8 vecteurs de l'annexe A §6.6 et le vecteur d'échappement de 06 §11.4 : l'objet haché
    /// et sa chaîne canonique, celle de `json.dumps(sort_keys=True, separators=(",", ":"))`. Les
    /// SHA-256 correspondants sont vérifiés par `lf-adapter-audit` (`input_hash_matches_p041_vectors`).
    #[test]
    fn canonical_input_matches_python_vectors() {
        let vectors: [(Value, &str); 9] = [
            (json!({}), "{}"),
            (
                json!({"query": "republique numerique", "fond": "LODA", "page_size": 5}),
                r#"{"fond":"LODA","page_size":5,"query":"republique numerique"}"#,
            ),
            (
                json!({"query": "vie privée", "fond": "CODE", "page_size": 10}),
                r#"{"fond":"CODE","page_size":10,"query":"vie priv\u00e9e"}"#,
            ),
            (
                json!({"query": "  vie privée ", "fond": "CODE", "page_size": 10}),
                r#"{"fond":"CODE","page_size":10,"query":"  vie priv\u00e9e "}"#,
            ),
            (
                json!({"query": "emoji 😀", "fond": "JURI", "page_size": 3}),
                r#"{"fond":"JURI","page_size":3,"query":"emoji \ud83d\ude00"}"#,
            ),
            (
                json!({"article_id": "LEGIARTI000006419288"}),
                r#"{"article_id":"LEGIARTI000006419288"}"#,
            ),
            (
                json!({"text_id": "LEGITEXT000033205014", "date": "2026-10-15"}),
                r#"{"date":"2026-10-15","text_id":"LEGITEXT000033205014"}"#,
            ),
            (
                json!({"decision_id": "CONSTEXT000052555275"}),
                r#"{"decision_id":"CONSTEXT000052555275"}"#,
            ),
            // 06 §11.4 : « a », DEL, « b », U+2028, « c ».
            (json!({"q": "a\u{7f}b\u{2028}c"}), r#"{"q":"a\u007fb\u2028c"}"#),
        ];
        for (arguments, expected) in vectors {
            assert_eq!(canonical(&arguments), expected, "{arguments}");
        }
    }

    #[test]
    fn strings_follow_ensure_ascii() {
        // Échappements courts de JSON, puis tout ce qui sort de U+0020..U+007E en \uXXXX minuscule.
        assert_eq!(canonical(&json!({"s": "\"\\\n\r\t\u{8}\u{c}"})), r#"{"s":"\"\\\n\r\t\b\f"}"#);
        assert_eq!(
            canonical(&json!({"s": "\u{0}\u{1}\u{b}\u{1f}"})),
            r#"{"s":"\u0000\u0001\u000b\u001f"}"#
        );
        // Bornes de l'intervalle imprimable : espace et tilde passent, DEL et C1 sont échappés.
        assert_eq!(
            canonical(&json!({"s": " ~\u{7f}\u{80}\u{9f}\u{a0}"})),
            r#"{"s":" ~\u007f\u0080\u009f\u00a0"}"#
        );
        // `/`, apostrophe et chevrons ne sont pas échappés (Python : `ensure_ascii` seul).
        assert_eq!(canonical(&json!({"s": "a/b'c<d>&e"})), r#"{"s":"a/b'c<d>&e"}"#);
        // BMP : minuscules ; hors BMP : paire de substitution.
        assert_eq!(canonical(&json!({"s": "éÀ€\u{ffff}"})), r#"{"s":"\u00e9\u00c0\u20ac\uffff"}"#);
        assert_eq!(
            canonical(&json!({"s": "😀\u{10000}\u{10ffff}"})),
            r#"{"s":"\ud83d\ude00\ud800\udc00\udbff\udfff"}"#
        );
    }

    #[test]
    fn keys_are_escaped_and_sorted_by_code_point() {
        // U+FFFF (BMP) précède U+10000 en points de code ; l'ordre UTF-16 les inverserait.
        assert_eq!(
            canonical(&json!({"\u{10000}": 1, "\u{ffff}": 2, "é": 3, "z": 4, "Z": 5, "\"": 6})),
            r#"{"\"":6,"Z":5,"z":4,"\u00e9":3,"\uffff":2,"\ud800\udc00":1}"#
        );
    }

    #[test]
    fn keys_are_sorted_whatever_order_the_map_iterates_in() {
        // `serde_json` avec `preserve_order` (activable par une dépendance) itère en ordre
        // d'insertion : le tri ne doit pas dépendre de la table.
        let values = [Value::from(1), Value::from(2), Value::from(3), Value::from(4)];
        let keys = ["b".to_owned(), "a".to_owned(), "\u{10000}".to_owned(), "\u{ffff}".to_owned()];
        let mut out = String::new();
        push_object(&mut out, keys.iter().zip(values.iter()).collect());
        assert_eq!(out, r#"{"a":2,"b":1,"\uffff":4,"\ud800\udc00":3}"#);
    }

    #[test]
    fn scalars_and_containers_are_python_literals() {
        assert_eq!(
            canonical(
                &json!({"a": [], "b": {}, "c": "", "d": [[], [{}], [1, [2]]], "e": false, "f": true, "g": null})
            ),
            r#"{"a":[],"b":{},"c":"","d":[[],[{}],[1,[2]]],"e":false,"f":true,"g":null}"#
        );
        // Les objets d'un tableau sont triés eux aussi, l'ordre du tableau est gardé.
        assert_eq!(
            canonical(&json!({"l": [{"b": 1, "a": 2}, {"d": 3, "c": 4}]})),
            r#"{"l":[{"a":2,"b":1},{"c":4,"d":3}]}"#
        );
    }

    #[test]
    fn integers_are_written_in_decimal() {
        assert_eq!(number("0"), "0");
        assert_eq!(number("-1"), "-1");
        assert_eq!(number("10"), "10");
        assert_eq!(number("9007199254740993"), "9007199254740993");
        assert_eq!(number("9223372036854775807"), "9223372036854775807");
        assert_eq!(number("-9223372036854775808"), "-9223372036854775808");
        assert_eq!(number("18446744073709551615"), "18446744073709551615");
    }

    /// Les sept valeurs de 06 §11.4, mesurées avec Python 3 (`json.dumps` écrit `repr`).
    #[test]
    fn canonical_float_matches_python_repr() {
        for (json, python) in [
            ("1.0", "1.0"),
            ("0.1", "0.1"),
            ("123.456", "123.456"),
            ("1e16", "1e+16"),
            ("0.00001", "1e-05"),
            ("1.5e-7", "1.5e-07"),
            ("1000000000000000.0", "1000000000000000.0"),
        ] {
            assert_eq!(number(json), python, "{json}");
        }
    }

    #[test]
    fn canonical_float_edges_match_python_repr() {
        for (json, python) in [
            ("10.0", "10.0"),
            ("-2.5", "-2.5"),
            ("0.0", "0.0"),
            ("-0.0", "-0.0"),
            ("0.0001", "0.0001"),
            ("0.00012345", "0.00012345"),
            ("0.000099", "9.9e-05"),
            ("2.5e-5", "2.5e-05"),
            ("1e15", "1000000000000000.0"),
            ("1.5e15", "1500000000000000.0"),
            ("9999999999999998.0", "9999999999999998.0"),
            ("12345678901234567.0", "1.2345678901234568e+16"),
            ("123456789012345.6", "123456789012345.6"),
            ("1e22", "1e+22"),
            ("1e100", "1e+100"),
            ("-1e-100", "-1e-100"),
            ("1.7976931348623157e308", "1.7976931348623157e+308"),
            ("5e-324", "5e-324"),
            ("2.2250738585072014e-308", "2.2250738585072014e-308"),
            ("0.30000000000000004", "0.30000000000000004"),
            // Un entier JSON trop grand pour un u64 est lu comme flottant par serde_json.
            ("18446744073709551616", "1.8446744073709552e+19"),
            ("99999999999999999999999", "1e+23"),
        ] {
            assert_eq!(number(json), python, "{json}");
        }
    }

    /// Les deux cas où `{:e}` seul s'écarte de Python, relevés en comparant 500 000 flottants à
    /// `json.dumps` : deux candidats équidistants (Python prend le chiffre pair) et une puissance
    /// de deux dont l'arrondi exact ne revient pas à la valeur (la forme la plus courte reste).
    #[test]
    fn canonical_float_ties_and_binade_edges_match_python_repr() {
        for (json, python) in [
            ("2049908751614195.25", "2049908751614195.2"),
            ("-2049908751614195.25", "-2049908751614195.2"),
            ("2049908751614195.75", "2049908751614195.8"),
            ("0.5", "0.5"),
            ("7.120236347223045e-307", "7.120236347223045e-307"),
            ("8.209073602596753e-289", "8.209073602596753e-289"),
            ("9007199254740992.0", "9007199254740992.0"),
        ] {
            assert_eq!(number(json), python, "{json}");
        }
    }
}
