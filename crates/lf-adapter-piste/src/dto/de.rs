//! Lecteurs serde tolérants (04 §8.2).
//!
//! Les listes passent par [`bounded_list`] et [`capped_list`], qui paient chaque élément lu sur le
//! budget du document (`budget.rs`, 04 §8.5, limite 1) ; [`null_default`] ne sert plus de liste,
//! seulement deux champs de struct. Les autres lecteurs ne comptent rien : soit ils lisent une
//! valeur scalaire, soit ils ignorent une valeur de tout autre type sans la construire.

use std::fmt;
use std::marker::PhantomData;

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, Error, IgnoredAny, MapAccess, SeqAccess, Visitor},
};

use super::budget;

/// Lit une valeur ou `null` et rend la valeur par défaut du type pour `null`.
///
/// À employer avec `#[serde(default)]` : un champ absent et un champ `null` donnent alors la même
/// valeur par défaut. Ne sert plus de liste : une liste passe par [`bounded_list`], qui paie ses
/// éléments sur le budget du document. Reste pour les champs de struct. Une valeur d'un autre
/// type reste une erreur.
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

/// Lit une liste, `null` ou absente donnant `Vec::new()` ; chaque élément lu coûte 1 au budget du
/// document (04 §8.5, limite 1 (a)).
///
/// À employer avec `#[serde(default)]`. Hors de `decode`, qui arme le budget, toute liste non vide
/// est refusée (`decoding budget not armed`). Une valeur qui n'est ni `null` ni un tableau garde
/// l'erreur de type de `serde` (`expected a sequence`).
///
/// # Errors
///
/// L'erreur du désérialiseur si la valeur n'est pas un tableau, si un élément est mal formé, ou
/// (`too many elements`) si le budget du document est épuisé.
pub(crate) fn bounded_list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    capped_list(deserializer, usize::MAX)
}

/// Comme [`bounded_list`], avec au plus `cap` éléments : le suivant donne `too many elements`.
///
/// Un élément refusé par le plafond ne coûte rien au budget du document.
///
/// # Errors
///
/// Celles de [`bounded_list`], et `too many elements` au `cap + 1`-ième élément.
pub(crate) fn capped_list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
    cap: usize,
) -> Result<Vec<T>, D::Error> {
    deserializer.deserialize_option(ListVisitor { cap, marker: PhantomData })
}

/// Lecteur de liste : `null` donne une liste vide, un tableau est lu élément par élément.
struct ListVisitor<T> {
    /// Nombre maximal d'éléments admis dans la liste.
    cap: usize,
    marker: PhantomData<fn() -> T>,
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for ListVisitor<T> {
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a sequence")
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(Vec::new())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq
            .next_element_seed(Element { under_cap: items.len() < self.cap, marker: PhantomData })?
        {
            items.push(item);
        }
        Ok(items)
    }
}

/// Un élément de liste : admis (plafond et budget) avant d'être lu.
struct Element<T> {
    /// Vrai tant que la liste n'a pas atteint son plafond.
    under_cap: bool,
    marker: PhantomData<fn() -> T>,
}

impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for Element<T> {
    type Value = T;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<T, D::Error> {
        admit(self.under_cap)?;
        T::deserialize(deserializer)
    }
}

/// Admet un élément : refusé au-delà du plafond, sinon payé sur le budget du document.
fn admit<E: Error>(under_cap: bool) -> Result<(), E> {
    if under_cap { budget::charge() } else { Err(E::custom(budget::TOO_MANY)) }
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

/// Résultat d'un lecteur tolérant : la valeur lue, ou « ignorée » (type ou valeur non retenus).
///
/// Un enum et non un `Option` : « ignorée » n'est pas une valeur par défaut, et chaque méthode
/// de visiteur nomme ainsi explicitement ce qu'elle rend.
enum Reading<T> {
    /// Valeur lue.
    Kept(T),
    /// Valeur d'un type ou d'une forme que le lecteur n'en retient pas.
    Ignored,
}

impl<T> Reading<T> {
    /// `Some` de la valeur lue, `None` pour une valeur ignorée.
    fn into_option(self) -> Option<T> {
        match self {
            Self::Kept(value) => Some(value),
            Self::Ignored => None,
        }
    }
}

impl<T> From<Option<T>> for Reading<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Ignored, Self::Kept)
    }
}

/// Lecteur de date : accepte n'importe quelle valeur JSON et rend `None` pour ce qui n'est pas une
/// date.
///
/// Écrit à la main plutôt qu'avec un enum `untagged`, qui met toute la valeur en mémoire tampon
/// avant d'essayer les variantes (mesuré : seize fois la taille du corps pour un tableau géant).
/// Un tableau ou un objet est consommé élément par élément, sans rien garder (`ignore_seq`,
/// `ignore_map`).
struct DateVisitor;

impl<'de> Visitor<'de> for DateVisitor {
    type Value = Reading<RawDate>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a date as a number, a string or any other JSON value")
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Reading::Kept(RawDate::Millis(value)))
    }

    /// Un entier au-delà d'un `i64` n'est pas une date.
    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(i64::try_from(value).ok().map(RawDate::Millis).into())
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Self::Value, E> {
        Ok(whole_float_to_millis(value).map(RawDate::Millis).into())
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(text_to_date(value).into())
    }

    fn visit_bool<E: Error>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        ignore_seq(seq)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        ignore_map(map)
    }
}

/// Consomme un tableau sans rien en garder et rend `Reading::Ignored`.
fn ignore_seq<'de, A: SeqAccess<'de>, T>(seq: A) -> Result<Reading<T>, A::Error> {
    IgnoredAny.visit_seq(seq)?;
    Ok(Reading::Ignored)
}

/// Consomme un objet sans rien en garder et rend `Reading::Ignored`.
fn ignore_map<'de, A: MapAccess<'de>, T>(map: A) -> Result<Reading<T>, A::Error> {
    IgnoredAny.visit_map(map)?;
    Ok(Reading::Ignored)
}

/// Lit une date facultative : millisecondes (nombre ou chaîne de chiffres) ou texte.
///
/// Un booléen, un objet, un tableau, une chaîne blanche, un flottant fractionnaire et un nombre
/// hors d'un `i64` donnent `None`. Aucune de ces tolérances n'est comptée ici : le décompte
/// éventuel appartient au mapping. Seul un JSON mal formé est une erreur : un tableau ou un objet
/// est parcouru par `IgnoredAny`, que `serde_json` lit sans récursion, donc à toute profondeur,
/// sans construire de valeur (mémoire bornée par la taille du corps).
///
/// # Errors
///
/// L'erreur du désérialiseur si le JSON lui-même est mal formé.
pub(crate) fn opt_raw_date<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RawDate>, D::Error> {
    deserializer.deserialize_any(DateVisitor).map(Reading::into_option)
}

/// Millisecondes d'un flottant qui est un entier exact et tient dans un `i64`.
///
/// Passe par le texte et non par `as i64`, qui sature en silence.
fn whole_float_to_millis(value: f64) -> Option<i64> {
    if value.fract() == 0.0 { format!("{value:.0}").parse().ok() } else { None }
}

/// Texte de date : blanc donne `None`, une suite de chiffres ASCII donne des millisecondes
/// (`None` si elle déborde un `i64`), le reste est gardé tel quel.
fn text_to_date(text: &str) -> Option<RawDate> {
    if is_blank(text) {
        None
    } else if text.bytes().all(|byte| byte.is_ascii_digit()) {
        text.parse().ok().map(RawDate::Millis)
    } else {
        Some(RawDate::Text(text.to_owned()))
    }
}

/// Lecteur de code : accepte n'importe quelle valeur JSON et rend `None` pour ce qui n'est ni un
/// texte non blanc ni un entier. Même principe que [`DateVisitor`] : aucune mémoire tampon.
struct CodeVisitor;

impl<'de> Visitor<'de> for CodeVisitor {
    type Value = Reading<String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a code as a string, a number or any other JSON value")
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Reading::Kept(value.to_string()))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Reading::Kept(value.to_string()))
    }

    /// Un flottant n'est pas un code.
    fn visit_f64<E: Error>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok((!is_blank(value)).then(|| value.to_owned()).into())
    }

    fn visit_bool<E: Error>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        ignore_seq(seq)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        ignore_map(map)
    }
}

/// Lit un code (IDCC…) donné en chaîne ou en entier, rendu en chaîne.
///
/// Une chaîne non blanche est gardée telle quelle (zéros initiaux compris) ; un entier, signé ou
/// non, est écrit en décimal. Une chaîne blanche, un flottant, un booléen, un tableau, un objet
/// et `null` donnent `None`. Seul un JSON mal formé est une erreur : un tableau ou un objet est
/// parcouru par `IgnoredAny`, que `serde_json` lit sans récursion, donc à toute profondeur,
/// sans construire de valeur (mémoire bornée par la taille du corps).
///
/// # Errors
///
/// L'erreur du désérialiseur si le JSON lui-même est mal formé.
pub(crate) fn opt_string_or_number<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    deserializer.deserialize_any(CodeVisitor).map(Reading::into_option)
}

/// Entier d'une chaîne qui n'est faite que de chiffres ASCII, `None` sinon ou s'il déborde un `u64`.
///
/// Seuls les chiffres ASCII comptent, ce qui écarte un signe, un espace ou une notation
/// scientifique (`u64::from_str` accepte un `+` initial, d'où le test des octets avant).
fn text_to_u64(text: &str) -> Option<u64> {
    if text.bytes().all(|byte| byte.is_ascii_digit()) { text.parse().ok() } else { None }
}

/// Lecteur d'entier positif : accepte n'importe quelle valeur JSON et rend `None` pour ce qui
/// n'est ni un `u64` ni une chaîne de chiffres. Même principe que [`DateVisitor`] : aucune
/// mémoire tampon.
struct LenientVisitor;

impl<'de> Visitor<'de> for LenientVisitor {
    type Value = Reading<u64>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an unsigned integer, a string of digits or any other JSON value")
    }

    /// Un entier négatif n'est pas lu.
    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(u64::try_from(value).ok().into())
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Reading::Kept(value))
    }

    /// Un flottant n'est pas lu, même entier.
    fn visit_f64<E: Error>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(text_to_u64(value).into())
    }

    fn visit_bool<E: Error>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(Reading::Ignored)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        ignore_seq(seq)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        ignore_map(map)
    }
}

/// Lit un entier positif donné en nombre ou en chaîne de chiffres ; tout le reste donne `None`.
///
/// Un champ absent, `null`, négatif, flottant, hors d'un `u64` ou d'un autre type ne fait pas
/// échouer le document : seul un JSON mal formé est une erreur (un tableau ou un objet est
/// parcouru par `IgnoredAny`, que `serde_json` lit sans récursion, donc à toute profondeur, sans
/// construire de valeur : mémoire bornée par la taille du corps).
/// Pour une chaîne, seuls les chiffres ASCII comptent.
///
/// # Errors
///
/// L'erreur du désérialiseur si le JSON lui-même est mal formé.
pub(crate) fn opt_u64_lenient<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    deserializer.deserialize_any(LenientVisitor).map(Reading::into_option)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::budget;
    use crate::dto::decode::decode;
    use lf_application::ports::CorpusError;
    use serde::Deserialize;
    use serde::de::value::{self, BytesDeserializer};

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

    #[derive(Debug, Deserialize)]
    struct Lists {
        #[serde(default, deserialize_with = "bounded_list")]
        a: Vec<String>,
        #[serde(default, deserialize_with = "bounded_list")]
        b: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct Capped {
        #[serde(default, deserialize_with = "two_at_most")]
        items: Vec<u8>,
    }

    #[derive(Debug, Deserialize)]
    struct CappedAtHundred {
        #[serde(default, deserialize_with = "hundred_at_most")]
        items: Vec<String>,
    }

    fn two_at_most<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        capped_list(d, 2)
    }

    fn hundred_at_most<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
        capped_list(d, 100)
    }

    /// Erreur `Decode` attendue, bâtie à la main.
    fn decode_failure(detail: &str) -> CorpusError {
        CorpusError::Decode { detail: detail.to_owned() }
    }

    /// Tableau JSON de `count` chaînes `"x"`.
    fn strings(count: usize) -> String {
        format!("[{}]", [r#""x""#].repeat(count).join(","))
    }

    #[test]
    fn a_bounded_list_reads_null_absent_and_empty_as_empty() {
        for raw in [r#"{"a":null}"#, "{}", r#"{"a":[]}"#] {
            let lists = decode::<Lists>(raw, "/e").unwrap();
            assert_eq!((lists.a, lists.b), (Vec::<String>::new(), Vec::<String>::new()), "{raw}");
        }
    }

    #[test]
    fn a_bounded_list_keeps_the_elements_in_order() {
        let lists = decode::<Lists>(r#"{"a":["x","y","z"]}"#, "/e").unwrap();
        assert_eq!(lists.a, ["x", "y", "z"]);
    }

    #[test]
    fn a_bounded_list_of_the_wrong_type_keeps_the_sequence_message() {
        let cases = [
            (r#"{"a":"secret-text-42"}"#, "string"),
            (r#"{"a":{}}"#, "map"),
            (r#"{"a":7}"#, "integer"),
        ];
        for (raw, kind) in cases {
            assert_eq!(
                decode::<Lists>(raw, "/e").unwrap_err(),
                decode_failure(&format!("/e: a: invalid type: {kind}, expected a sequence")),
                "{raw}"
            );
        }
    }

    #[test]
    fn a_bounded_list_element_error_keeps_its_path() {
        assert_eq!(
            decode::<Lists>(r#"{"a":["ok",5]}"#, "/e").unwrap_err(),
            decode_failure("/e: a[1]: invalid type: integer, expected a string")
        );
    }

    #[test]
    fn the_budget_is_global_across_lists() {
        let refused = format!(r#"{{"a":{},"b":{}}}"#, strings(100_000), strings(100_001));
        assert_eq!(
            decode::<Lists>(&refused, "/e").unwrap_err(),
            decode_failure("/e: b[100000]: too many elements")
        );
        let accepted = format!(r#"{{"a":{},"b":{}}}"#, strings(100_000), strings(100_000));
        let lists = decode::<Lists>(&accepted, "/e").unwrap();
        assert_eq!((lists.a.len(), lists.b.len()), (100_000, 100_000));
    }

    #[test]
    fn a_capped_list_accepts_exactly_the_cap() {
        assert_eq!(decode::<Capped>(r#"{"items":[1,2]}"#, "/e").unwrap().items, [1, 2]);
        assert_eq!(
            decode::<Capped>(r#"{"items":[1,2,3]}"#, "/e").unwrap_err(),
            decode_failure("/e: items[2]: too many elements")
        );
    }

    #[test]
    fn a_capped_list_refusal_does_not_spend_budget() {
        let _armed = budget::arm();
        assert!(serde_json::from_str::<Capped>(r#"{"items":[1,2,3]}"#).is_err());
        assert_eq!(budget::remaining(), Some(budget::MAX_ELEMENTS - 2));
    }

    #[test]
    fn a_list_read_outside_decode_is_refused() {
        let error = serde_json::from_str::<Lists>(r#"{"a":["x"]}"#).unwrap_err();
        assert!(error.to_string().starts_with("decoding budget not armed"), "{error}");
    }

    #[test]
    fn a_refusal_never_carries_a_value() {
        let accepted = format!(r#"{{"items":[{}]}}"#, [r#""sekret-42-Ω""#].repeat(100).join(","));
        assert_eq!(decode::<CappedAtHundred>(&accepted, "/e").unwrap().items.len(), 100);
        let raw = format!(r#"{{"items":[{}]}}"#, [r#""sekret-42-Ω""#].repeat(101).join(","));
        let error = decode::<CappedAtHundred>(&raw, "/e").unwrap_err();
        assert_eq!(error, decode_failure("/e: items[100]: too many elements"));
        assert!(!format!("{error:?}").contains("sekret"));
        assert!(!error.to_string().contains("sekret"));
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

    #[test]
    fn a_broken_object_is_still_an_error_for_the_three_readers() {
        assert_eq!(
            [
                serde_json::from_str::<Dated>(r#"{"date":{"a":}}"#).is_err(),
                serde_json::from_str::<Coded>(r#"{"code":{"a":}}"#).is_err(),
                serde_json::from_str::<Holder>(r#"{"value":{"a":}}"#).is_err(),
                serde_json::from_str::<Holder>(r#"{"value":{"a":1"#).is_err(),
            ],
            [true; 4]
        );
    }

    /// `[1,1,…,1]` de `count` éléments.
    fn big_array(count: usize) -> String {
        format!("[{}]", vec!["1"; count].join(","))
    }

    /// `{"k0":1,"k1":1,…}` de `count` clés.
    fn big_object(count: usize) -> String {
        let entries: Vec<String> = (0..count).map(|index| format!(r#""k{index}":1"#)).collect();
        format!("{{{}}}", entries.join(","))
    }

    #[test]
    fn a_huge_array_or_object_is_ignored_by_the_three_readers() {
        let array = big_array(100_000);
        let object = big_object(100_000);
        // Plusieurs centaines de Kio chacun : le contenu est consommé sans être gardé.
        assert!(array.len() > 150_000 && object.len() > 800_000);
        assert_eq!(
            (
                [array.as_str(), object.as_str()].map(date),
                [array.as_str(), object.as_str()].map(code),
                [array.as_str(), object.as_str()].map(read),
            ),
            ([None, None], [None, None], [None, None])
        );
    }

    /// `levels` tableaux imbriqués : `[[[…]]]`.
    fn nested_arrays(levels: usize) -> String {
        format!("{}{}", "[".repeat(levels), "]".repeat(levels))
    }

    /// `levels` objets imbriqués : `{"a":{"a":…{}…}}`.
    fn nested_objects(levels: usize) -> String {
        format!("{}{{}}{}", r#"{"a":"#.repeat(levels), "}".repeat(levels))
    }

    #[test]
    fn nesting_of_any_depth_is_ignored_by_the_three_readers() {
        // Le tableau ou l'objet n'est lu que par `IgnoredAny`, que `serde_json` parcourt sans
        // récursion ni limite de profondeur : 63 niveaux comme 5 000 donnent `None`. (Avec
        // `untagged`, la mise en mémoire tampon échouait dès 127 tableaux imbriqués.)
        for levels in [63, 127, 128, 200, 5_000] {
            for raw in [nested_arrays(levels), nested_objects(levels)] {
                assert_eq!(
                    (date(&raw), code(&raw), read(&raw)),
                    (None, None, None),
                    "{levels} niveaux"
                );
            }
        }
    }

    #[test]
    fn deep_nesting_is_ignored_through_decode_too() {
        let body = |field: &str| format!(r#"{{"{field}":{}}}"#, nested_arrays(200));
        let dated = decode::<Dated>(&body("date"), "/e").map(|dated| dated.date);
        let coded = decode::<Coded>(&body("code"), "/e").map(|coded| coded.code);
        let lenient = decode::<Holder>(&body("value"), "/e").map(|holder| holder.value);
        assert_eq!((dated, coded, lenient), (Ok(None), Ok(None), Ok(None)));
    }

    #[test]
    fn the_boundary_values_keep_their_outputs() {
        let raws = [r#""""#, r#""  ""#, "-1", "1.5", "1.0", "true", "false", "null"];
        assert_eq!(
            (raws.map(date), raws.map(code), raws.map(read)),
            (
                [None, None, Some(millis(-1)), None, Some(millis(1)), None, None, None],
                [None, None, Some("-1".to_owned()), None, None, None, None, None],
                [None; 8]
            )
        );
    }

    #[test]
    fn an_unexpected_kind_is_refused_with_a_fixed_message_that_names_no_value() {
        let secret = b"sekret-42";
        let errors = [
            opt_raw_date(BytesDeserializer::<value::Error>::new(secret)).unwrap_err().to_string(),
            opt_string_or_number(BytesDeserializer::<value::Error>::new(secret))
                .unwrap_err()
                .to_string(),
            opt_u64_lenient(BytesDeserializer::<value::Error>::new(secret))
                .unwrap_err()
                .to_string(),
        ];
        assert_eq!(
            errors,
            [
                "invalid type: byte array, expected a date as a number, a string or any other JSON value",
                "invalid type: byte array, expected a code as a string, a number or any other JSON value",
                "invalid type: byte array, expected an unsigned integer, a string of digits or any other JSON value",
            ]
        );
    }
}
