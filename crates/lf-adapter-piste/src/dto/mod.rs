//! Structures de désérialisation des réponses de PISTE (04 §3.2, §8).
//!
//! Ces types ne servent qu'à lire du JSON : ils ne portent aucune règle métier. Ils sont
//! tolérants (champs facultatifs, nombres lus aussi en chaîne) parce que le format réel de
//! PISTE n'est pas garanti.

pub(crate) mod acco;
pub(crate) mod article;
pub(crate) mod budget;
pub(crate) mod consult_text;
pub(crate) mod de;
pub(crate) mod decode;
pub(crate) mod error;
pub(crate) mod juri;
pub(crate) mod oauth;
pub(crate) mod search;

#[cfg(test)]
mod tests {
    use super::decode::decode;
    use lf_application::ports::CorpusError;
    use serde::de::DeserializeOwned;

    /// Liste JSON de `count` fois `element`.
    pub(crate) fn repeated(element: &str, count: usize) -> String {
        format!("[{}]", vec![element; count].join(","))
    }

    /// Vérifie que `raw` se décode en `T` par l'erreur `Decode` d'un budget épuisé au `path` de
    /// l'`endpoint`. Le succès n'est jamais affiché : un DTO de 200 001 éléments remplirait la sortie.
    pub(crate) fn assert_too_many<T: DeserializeOwned>(raw: &str, endpoint: &str, path: &str) {
        let expected =
            CorpusError::Decode { detail: format!("{endpoint}: {path}: too many elements") };
        assert_eq!(decode::<T>(raw, endpoint).err(), Some(expected), "{path}");
    }

    /// Vérifie qu'un numéro (D-24) se lit en chaîne ou en nombre, rendu en chaîne : `field` est la
    /// clé JSON du champ et `get` le champ du DTO. Chaîne blanche, `null`, absent, flottant,
    /// booléen, tableau et objet donnent `None`.
    pub(crate) fn assert_number_reading<T: DeserializeOwned>(
        field: &str,
        endpoint: &str,
        get: fn(&T) -> &Option<String>,
    ) {
        let read = |raw: &str| {
            let dto = decode::<T>(&format!(r#"{{"{field}":{raw}}}"#), endpoint).unwrap();
            get(&dto).clone()
        };
        let text = |value: &str| Some(value.to_owned());
        assert_eq!(read("12500399"), text("12500399"), "entier");
        assert_eq!(read(r#""12500399""#), text("12500399"), "chaîne");
        assert_eq!(read(r#""0045""#), text("0045"), "zéros initiaux");
        for raw in [r#""""#, r#""  ""#, "null", "2.5", "true", "[1]", r#"{"a":1}"#] {
            assert_eq!(read(raw), None, "{raw}");
        }
        let absent = decode::<T>("{}", endpoint).unwrap();
        assert_eq!(get(&absent), &None, "absent");
    }

    /// Code de production d'un fichier de DTO : tout ce qui précède son module de test.
    fn production(source: &'static str) -> &'static str {
        source.split("#[cfg(test)]").next().unwrap()
    }

    /// Les sources de production des cinq fichiers de DTO à listes.
    fn sources() -> [&'static str; 5] {
        [
            production(include_str!("search.rs")),
            production(include_str!("consult_text.rs")),
            production(include_str!("article.rs")),
            production(include_str!("juri.rs")),
            production(include_str!("acco.rs")),
        ]
    }

    #[test]
    fn every_dto_list_is_bounded() {
        let mut lists = 0;
        for source in sources() {
            let lines: Vec<&str> = source.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                let line = line.trim_start();
                if !(line.starts_with("pub ") && line.contains(": Vec<")) {
                    continue;
                }
                lists += 1;
                let above = &lines[index.saturating_sub(3)..index];
                assert!(
                    above.iter().any(|attribute| attribute.contains("serde(")
                        && (attribute.contains("bounded_list")
                            || attribute.contains("bounded_results"))),
                    "liste hors budget : {line}"
                );
            }
        }
        assert_eq!(lists, 25);
    }

    #[test]
    fn null_default_serves_only_the_two_struct_fields() {
        let mut uses = 0;
        for source in sources() {
            let lines: Vec<&str> = source.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if !line.contains(r#"deserialize_with = "de::null_default""#) {
                    continue;
                }
                uses += 1;
                let field = lines[index + 1..]
                    .iter()
                    .take(3)
                    .find(|next| next.trim_start().starts_with("pub "))
                    .unwrap();
                assert!(!field.contains("Vec<"), "liste en null_default : {field}");
            }
        }
        assert_eq!(uses, 2);
    }
}
