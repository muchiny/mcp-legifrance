//! Lecture d'un corps JSON avec le chemin de l'erreur (04 §8.5).

use lf_application::ports::CorpusError;
use serde::de::DeserializeOwned;
use serde_json::error::Category;
use serde_path_to_error::{Path, Segment};

/// Chemin d'une erreur située à la racine du document.
const ROOT: &str = ".";

/// Longueur maximale d'une clé du corps gardée dans un chemin.
const MAX_KEY_LEN: usize = 64;

/// Message `serde` qui cite la valeur lue : on ne garde que le genre et l'attendu.
const WITH_VALUE: [&str; 2] = ["invalid type: ", "invalid value: "];

/// Message `serde` qui cite un nom pris dans le corps : on ne garde que le préfixe.
const WITH_NAME: [&str; 2] = ["unknown field", "unknown variant"];

/// Message `serde` fait seulement de noms de DTO et de nombres : gardé entier.
const STATIC: [&str; 3] = ["missing field ", "duplicate field ", "invalid length "];

/// Catégorie rendue quand un message ne se laisse pas expurger.
const UNRECOGNIZED: &str = "invalid data";

/// Lit un corps JSON dans `T`, sans jamais citer le corps dans l'erreur.
///
/// Le `detail` de [`CorpusError::Decode`] a la forme `<endpoint>: <chemin>: <catégorie>` : le
/// chemin JSON de l'erreur (`text.sommaire[1].abstrats`, `.` à la racine) et la catégorie
/// `serde` expurgée (`invalid type: integer, expected a string`), jamais la valeur lue. Le
/// message brut de `serde` cite la valeur, d'où `redact` ; une clé du corps qui ne ressemble pas
/// à un identifiant est masquée par `?` dans le chemin. Les erreurs de syntaxe et de fin de flux
/// gardent leur texte fixe et leur position.
///
/// Un texte après le document est une erreur (`trailing characters`), comme avec
/// `serde_json::from_str`. Une imbrication de plus de 128 tableaux et objets est une erreur
/// (limite de `serde_json`, non levée). Le texte a déjà passé l'UTF-8 strict et la réparation du
/// mojibake (`encoding`) : cette fonction ne s'en occupe pas.
///
/// # Errors
///
/// [`CorpusError::Decode`] si le texte n'est pas du JSON valide ou ne correspond pas à `T`.
pub(crate) fn decode<T: DeserializeOwned>(text: &str, endpoint: &str) -> Result<T, CorpusError> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = serde_path_to_error::deserialize(&mut deserializer)
        .map_err(|error| decode_error(endpoint, &render_path(error.path()), error.inner()))?;
    deserializer.end().map_err(|error| decode_error(endpoint, ROOT, &error))?;
    Ok(value)
}

/// Construit l'erreur `Decode` : `<endpoint>: <chemin>: <catégorie>`.
fn decode_error(endpoint: &str, path: &str, error: &serde_json::Error) -> CorpusError {
    CorpusError::Decode { detail: format!("{endpoint}: {path}: {}", category(error)) }
}

/// Écrit le chemin lui-même, sans passer par `Path::to_string` : une clé du corps y est masquée.
///
/// Un indice de tableau s'accole au segment précédent (`sommaire[1]`), un nom est séparé par un
/// point, un segment inconnu s'écrit `?` et un chemin vide s'écrit `.`.
fn render_path(path: &Path) -> String {
    let mut rendered = String::new();
    for segment in path {
        match segment {
            Segment::Seq { index } => {
                rendered.push('[');
                rendered.push_str(&index.to_string());
                rendered.push(']');
            }
            Segment::Map { key } => push_name(&mut rendered, safe_key(key)),
            Segment::Enum { variant } => push_name(&mut rendered, safe_key(variant)),
            Segment::Unknown => push_name(&mut rendered, "?"),
        }
    }
    if rendered.is_empty() {
        rendered.push_str(ROOT);
    }
    rendered
}

/// Ajoute un nom au chemin, séparé du précédent par un point.
fn push_name(rendered: &mut String, name: &str) {
    if !rendered.is_empty() {
        rendered.push('.');
    }
    rendered.push_str(name);
}

/// Garde une clé de la forme `[A-Za-z0-9_-]{1,64}`, sinon `?`.
///
/// `serde_path_to_error` ne distingue pas un champ du DTO d'une clé lue dans le corps : toute clé
/// qui pourrait être du texte libre est masquée. Une clé de forme identifiant venue du corps
/// passe quand même : les DTO ne déclarent donc ni table indexée par du texte du corps ni
/// `deny_unknown_fields` (04 §8.1).
fn safe_key(key: &str) -> &str {
    let plain = !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if plain { key } else { "?" }
}

/// Texte de l'erreur `serde_json` sans aucune valeur du corps.
///
/// Les erreurs de syntaxe et de fin de flux ont des textes fixes, gardés avec leur position. Les
/// erreurs de données citent la valeur lue : elles passent par [`redact`].
fn category(error: &serde_json::Error) -> String {
    match error.classify() {
        Category::Syntax | Category::Eof => error.to_string(),
        Category::Data | Category::Io => redact(&error.to_string()),
    }
}

/// Expurge un message `serde` : jamais une valeur ni un nom pris dans le corps.
///
/// Retire la position finale, puis selon le message : `invalid type` et `invalid value` gardent
/// le genre et l'attendu, `unknown field` et `unknown variant` le préfixe seul, `missing field`,
/// `duplicate field` et `invalid length` restent entiers (noms de DTO et nombres), tout autre
/// message qui contient un guillemet ou un accent grave devient `invalid data`.
fn redact(message: &str) -> String {
    let message = strip_position(message);
    for prefix in WITH_VALUE {
        if let Some(rest) = message.strip_prefix(prefix) {
            return redact_value(prefix, rest);
        }
    }
    for prefix in WITH_NAME {
        if message.starts_with(prefix) {
            return prefix.to_owned();
        }
    }
    if STATIC.iter().any(|prefix| message.starts_with(prefix)) {
        return message.to_owned();
    }
    if message.contains(['"', '`']) { UNRECOGNIZED.to_owned() } else { message.to_owned() }
}

/// Retire ` at line L column C` en fin de message, et lui seul : un ` at line ` cité dans une
/// valeur n'est pas une position.
fn strip_position(message: &str) -> &str {
    match message.rsplit_once(" at line ") {
        Some((head, tail)) if is_position(tail) => head,
        _ => message,
    }
}

/// Vrai pour `L column C`, où `L` et `C` sont des nombres.
fn is_position(tail: &str) -> bool {
    tail.split_once(" column ").is_some_and(|(line, column)| is_number(line) && is_number(column))
}

/// Vrai pour une suite non vide de chiffres ASCII.
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// `invalid type: <genre> <valeur>, expected <attendu>` devient `<préfixe><genre>, expected
/// <attendu>`.
///
/// L'attendu suit la **dernière** occurrence de `, expected ` : une valeur qui en contient ne
/// peut ainsi pas truquer le détail. Sans attendu, le message est `invalid data`.
fn redact_value(prefix: &str, rest: &str) -> String {
    let Some((described, expected)) = rest.rsplit_once(", expected ") else {
        return UNRECOGNIZED.to_owned();
    };
    let (kind, _) = described.split_once(['"', '`']).unwrap_or((described, ""));
    format!("{prefix}{}, expected {expected}", kind.trim_end())
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::Deserialize;

    use super::*;

    const ENDPOINT: &str = "/consult/juri";
    const SECRET: &str = "sekret-42-Ω";

    #[derive(Debug, PartialEq, Deserialize)]
    struct Envelope {
        text: Option<Text>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Text {
        #[serde(default)]
        numero_affaire: Vec<String>,
        #[serde(default)]
        sommaire: Vec<Summary>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Summary {
        abstrats: Option<String>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Probe {
        a: Option<u8>,
        b: Option<String>,
        #[serde(default)]
        c: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Strict {}

    #[derive(Debug, PartialEq, Deserialize)]
    enum Kind {
        Alpha,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct HasKind {
        kind: Kind,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    enum Shape {
        Circle(u8),
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct HasShape {
        shape: Shape,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Required {
        name: String,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct HasPair {
        pair: (u8, u8),
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Section {
        #[serde(default)]
        sections: Vec<Section>,
    }

    /// L'erreur de décodage de `text` vers `T`.
    fn failure<T: DeserializeOwned + Debug>(text: &str) -> CorpusError {
        decode::<T>(text, ENDPOINT).unwrap_err()
    }

    fn decode_error(detail: &str) -> CorpusError {
        CorpusError::Decode { detail: detail.to_owned() }
    }

    /// `levels` sections imbriquées : `{"sections":[` répété, `{}`, puis `]}` répété.
    fn nested_sections(levels: usize) -> String {
        format!("{}{{}}{}", r#"{"sections":["#.repeat(levels), "]}".repeat(levels))
    }

    #[test]
    fn valid_json_is_decoded() {
        let text = r#"{"text":{"numeroAffaire":["A1"],"sommaire":[{"abstrats":"ok"}]},"inconnu":{"x":[1]}}"#;
        assert_eq!(
            decode::<Envelope>(text, ENDPOINT),
            Ok(Envelope {
                text: Some(Text {
                    numero_affaire: vec!["A1".to_owned()],
                    sommaire: vec![Summary { abstrats: Some("ok".to_owned()) }],
                }),
            })
        );
    }

    #[test]
    fn invalid_json_maps_to_decode_with_path() {
        assert_eq!(
            failure::<Envelope>(r#"{"text":{"numeroAffaire":12}}"#),
            decode_error(
                "/consult/juri: text.numeroAffaire: invalid type: integer, expected a sequence"
            )
        );
    }

    #[test]
    fn the_path_carries_list_indexes() {
        assert_eq!(
            failure::<Envelope>(r#"{"text":{"sommaire":[{"abstrats":"ok"},{"abstrats":7}]}}"#),
            decode_error(
                "/consult/juri: text.sommaire[1].abstrats: invalid type: integer, expected a string"
            )
        );
    }

    #[test]
    fn the_path_carries_enum_variants() {
        assert_eq!(
            decode::<HasShape>(r#"{"shape":{"Circle":3}}"#, ENDPOINT),
            Ok(HasShape { shape: Shape::Circle(3) })
        );
        assert_eq!(
            failure::<HasShape>(r#"{"shape":{"Circle":"x"}}"#),
            decode_error("/consult/juri: shape.Circle: invalid type: string, expected u8")
        );
    }

    #[test]
    fn a_path_below_a_root_array_has_no_leading_dot() {
        assert_eq!(
            failure::<Vec<Probe>>(r#"[{"a":"x"}]"#),
            decode_error("/consult/juri: [0].a: invalid type: string, expected u8")
        );
        assert_eq!(
            failure::<Vec<Probe>>("[1]"),
            decode_error("/consult/juri: [0]: invalid type: integer, expected struct Probe")
        );
    }

    #[test]
    fn html_body_maps_to_decode() {
        assert_eq!(
            failure::<Envelope>("<html><body>502 Bad Gateway</body></html>"),
            decode_error("/consult/juri: .: expected value at line 1 column 1")
        );
    }

    #[test]
    fn json_null_body_maps_to_decode() {
        assert_eq!(
            failure::<Envelope>("null"),
            decode_error("/consult/juri: .: invalid type: null, expected struct Envelope")
        );
    }

    #[test]
    fn empty_and_truncated_bodies_map_to_decode() {
        assert_eq!(
            failure::<Envelope>(""),
            decode_error("/consult/juri: .: EOF while parsing a value at line 1 column 0")
        );
        assert_eq!(
            failure::<Envelope>(r#"{"text":{"numeroAf"#),
            decode_error("/consult/juri: text.?: EOF while parsing a string at line 1 column 18")
        );
    }

    #[test]
    fn text_after_the_document_maps_to_decode() {
        assert_eq!(
            failure::<Envelope>(r#"{"text":null} {"text":null}"#),
            decode_error("/consult/juri: .: trailing characters at line 1 column 15")
        );
    }

    #[test]
    fn a_syntax_error_inside_the_document_keeps_its_path() {
        assert_eq!(
            failure::<Envelope>(r#"{"text":{"numeroAffaire":["a",]}}"#),
            decode_error("/consult/juri: text.numeroAffaire: trailing comma at line 1 column 31")
        );
    }

    #[test]
    fn nesting_beyond_the_serde_json_limit_maps_to_decode() {
        let debug = format!("{:?}", failure::<Section>(&nested_sections(200)));
        assert_eq!(
            (
                debug.starts_with(r#"Decode { detail: "/consult/juri: sections[0].sections[0]."#),
                debug.contains("recursion limit exceeded at line 1 column"),
            ),
            (true, true)
        );
    }

    #[test]
    fn nesting_just_under_the_limit_is_decoded() {
        let section = decode::<Section>(&nested_sections(63), ENDPOINT).unwrap();
        assert_eq!(section.sections.len(), 1);
        assert!(decode::<Section>(&nested_sections(64), ENDPOINT).is_err());
    }

    #[test]
    fn deep_values_the_dto_ignores_do_not_fail() {
        let text = format!(r#"{{"x":{}{}}}"#, "[".repeat(200), "]".repeat(200));
        assert_eq!(decode::<Probe>(&text, ENDPOINT), Ok(Probe { a: None, b: None, c: vec![] }));
    }

    #[test]
    fn serde_categories_are_kept_without_the_value() {
        let cases = [
            (failure::<Probe>(r#"{"a":300}"#), "a: invalid value: integer, expected u8"),
            (failure::<Probe>(r#"{"a":"x"}"#), "a: invalid type: string, expected u8"),
            (failure::<Probe>(r#"{"a":1.5}"#), "a: invalid type: floating point, expected u8"),
            (failure::<Probe>(r#"{"a":true}"#), "a: invalid type: boolean, expected u8"),
            (failure::<Probe>(r#"{"b":{}}"#), "b: invalid type: map, expected a string"),
            (failure::<Probe>(r#"{"c":"x"}"#), "c: invalid type: string, expected a sequence"),
            (failure::<Probe>(r#"{"a":1e999}"#), "a: number out of range at line 1 column 10"),
            (failure::<Strict>(r#"{"zzz":1}"#), "zzz: unknown field"),
            (failure::<HasKind>(r#"{"kind":"Beta"}"#), "kind: unknown variant"),
            (failure::<Required>("{}"), ".: missing field `name`"),
            (
                failure::<HasPair>(r#"{"pair":[1]}"#),
                "pair: invalid length 1, expected a tuple of size 2",
            ),
            (failure::<Probe>(r#"{"a":1,"a":2}"#), ".: duplicate field `a`"),
        ];
        for (error, expected) in cases {
            assert_eq!(error, decode_error(&format!("{ENDPOINT}: {expected}")));
        }
    }

    #[test]
    fn a_secret_in_the_body_never_reaches_the_detail() {
        let bodies = [
            format!(r#"{{"a":"{SECRET}"}}"#),
            format!(r#"{{"a":"{SECRET}, expected u8"}}"#),
            format!(r#"{{"a":"`{SECRET}\" at line 1 column 1"}}"#),
            format!(r#"{{"c":["{SECRET}",1]}}"#),
            format!(r#"{{"b":{{"{SECRET}":1}}}}"#),
            format!(r#"{{"{SECRET}":1}}"#),
            format!(r#"{{"kind":"{SECRET}"}}"#),
            format!("{{}} {SECRET}"),
            format!(r#"{{"b":"{SECRET}"#),
            format!("<html>{SECRET}</html>"),
            SECRET.to_owned(),
        ];
        for body in bodies {
            let errors = [
                decode::<Probe>(&body, ENDPOINT).err(),
                decode::<Strict>(&body, ENDPOINT).err(),
                decode::<HasKind>(&body, ENDPOINT).err(),
            ];
            assert!(errors.iter().any(Option::is_some), "no failure for {body}");
            for error in errors.into_iter().flatten() {
                assert!(!format!("{error:?}").contains(SECRET), "Debug leaks for {body}");
                assert!(!error.to_string().contains(SECRET), "Display leaks for {body}");
            }
        }
    }

    #[test]
    fn keys_that_do_not_look_like_identifiers_are_masked_in_the_path() {
        let long = "k".repeat(64);
        let too_long = "k".repeat(65);
        let keys = [long.as_str(), too_long.as_str(), "a.b", "a b", "é", "", "Id_9-x"];
        let shown = [long.as_str(), "?", "?", "?", "?", "?", "Id_9-x"];
        for (key, shown) in keys.into_iter().zip(shown) {
            assert_eq!(
                failure::<Strict>(&format!(r#"{{"{key}":1}}"#)),
                decode_error(&format!("{ENDPOINT}: {shown}: unknown field"))
            );
        }
    }

    #[test]
    fn a_value_that_looks_like_a_message_cannot_forge_the_detail() {
        assert_eq!(
            failure::<Probe>(r#"{"a":"x, expected y\", expected z"}"#),
            decode_error("/consult/juri: a: invalid type: string, expected u8")
        );
    }

    #[test]
    fn redact_keeps_the_kind_and_the_expectation_only() {
        let messages = [
            "invalid type: integer `12`, expected a string at line 1 column 7",
            "invalid value: integer `300`, expected u8",
            r#"invalid type: string "a`b, expected c", expected u64 at line 1 column 9"#,
            "invalid type: unit value, expected a string",
            "invalid type: sequence",
            "invalid value: integer `1`",
        ];
        assert_eq!(
            messages.map(redact),
            [
                "invalid type: integer, expected a string",
                "invalid value: integer, expected u8",
                "invalid type: string, expected u64",
                "invalid type: unit value, expected a string",
                "invalid data",
                "invalid data",
            ]
        );
    }

    #[test]
    fn redact_drops_names_taken_from_the_body() {
        assert_eq!(
            [
                "unknown field `secret`, expected `a` or `b` at line 1 column 3",
                "unknown variant `secret`, expected `Alpha` or `Beta` at line 1 column 3",
            ]
            .map(redact),
            ["unknown field", "unknown variant"]
        );
    }

    #[test]
    fn redact_keeps_static_messages_and_masks_unknown_quoting_ones() {
        assert_eq!(
            [
                "missing field `name` at line 1 column 2",
                "duplicate field `a`",
                "invalid length 1, expected a tuple of size 2",
                "number out of range at line 1 column 10",
                "custom message without value",
                "custom message with `quoted` name",
                "custom message with \"quoted\" name",
            ]
            .map(redact),
            [
                "missing field `name`",
                "duplicate field `a`",
                "invalid length 1, expected a tuple of size 2",
                "number out of range",
                "custom message without value",
                "invalid data",
                "invalid data",
            ]
        );
    }

    #[test]
    fn a_location_inside_a_value_is_not_mistaken_for_the_suffix() {
        assert_eq!(
            redact(r#"invalid type: string "x at line 9", expected u8 at line 1 column 12"#),
            "invalid type: string, expected u8"
        );
    }

    #[test]
    fn only_a_real_position_suffix_is_stripped() {
        let messages = [
            r#"invalid type: string "x at line 9", expected u8"#,
            "missing field `a` at line x column 1",
            "missing field `a` at line 1 column x",
            "missing field `a` at line  column 1",
            "missing field `a` at line 1 column ",
            "missing field `a` at line 12",
            "missing field `a` at line 12 column 345",
        ];
        assert_eq!(
            messages.map(redact),
            [
                "invalid type: string, expected u8",
                "missing field `a` at line x column 1",
                "missing field `a` at line 1 column x",
                "missing field `a` at line  column 1",
                "missing field `a` at line 1 column ",
                "missing field `a` at line 12",
                "missing field `a`",
            ]
        );
    }
}
