//! Fixtures de référence des tests de mapping : les seize réponses de `tests/fixtures/reference`.
//!
//! Les fichiers sont lus par `include_str!` et jamais reformatés ni réparés sur disque : les
//! tests de mapping partent des octets capturés, avec leurs sentinelles, leurs chaînes vides et
//! leurs formes inattendues. Ce module n'existe que sous `cfg(test)`.

/// Noms des fixtures, triés et sans doublon : `consult_*` puis `search_*`.
pub(crate) const NAMES: [&str; 16] = [
    "consult_acco",
    "consult_article",
    "consult_cnil",
    "consult_juri",
    "consult_juri_cetat",
    "consult_juri_constit",
    "consult_kali",
    "consult_loda",
    "search_acco",
    "search_cetat",
    "search_cnil",
    "search_code",
    "search_constit",
    "search_juri",
    "search_kali",
    "search_loda",
];

/// Nom et contenu brut de chaque fixture.
const FIXTURES: [(&str, &str); 16] = [
    ("consult_acco", include_str!("../../tests/fixtures/reference/consult_acco.json")),
    ("consult_article", include_str!("../../tests/fixtures/reference/consult_article.json")),
    ("consult_cnil", include_str!("../../tests/fixtures/reference/consult_cnil.json")),
    ("consult_juri", include_str!("../../tests/fixtures/reference/consult_juri.json")),
    ("consult_juri_cetat", include_str!("../../tests/fixtures/reference/consult_juri_cetat.json")),
    (
        "consult_juri_constit",
        include_str!("../../tests/fixtures/reference/consult_juri_constit.json"),
    ),
    ("consult_kali", include_str!("../../tests/fixtures/reference/consult_kali.json")),
    ("consult_loda", include_str!("../../tests/fixtures/reference/consult_loda.json")),
    ("search_acco", include_str!("../../tests/fixtures/reference/search_acco.json")),
    ("search_cetat", include_str!("../../tests/fixtures/reference/search_cetat.json")),
    ("search_cnil", include_str!("../../tests/fixtures/reference/search_cnil.json")),
    ("search_code", include_str!("../../tests/fixtures/reference/search_code.json")),
    ("search_constit", include_str!("../../tests/fixtures/reference/search_constit.json")),
    ("search_juri", include_str!("../../tests/fixtures/reference/search_juri.json")),
    ("search_kali", include_str!("../../tests/fixtures/reference/search_kali.json")),
    ("search_loda", include_str!("../../tests/fixtures/reference/search_loda.json")),
];

/// Contenu brut de la fixture `name`, tel qu'écrit sur disque.
///
/// # Panics
///
/// Si `name` n'est pas dans [`NAMES`] : une faute de frappe d'un test, pas une donnée.
pub(crate) fn text(name: &str) -> &'static str {
    FIXTURES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, content)| *content)
        .expect("known fixture name")
}

/// Fixture `name` lue comme valeur JSON.
///
/// # Panics
///
/// Si `name` est inconnu ou si le fichier n'est pas du JSON.
pub(crate) fn value(name: &str) -> serde_json::Value {
    serde_json::from_str(text(name)).expect("fixture is JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_names_are_sorted_and_unique() {
        assert_eq!(NAMES.len(), 16);
        assert!(NAMES.windows(2).all(|pair| pair[0] < pair[1]), "{NAMES:?}");
        let listed: Vec<&str> = FIXTURES.iter().map(|(name, _)| *name).collect();
        assert_eq!(listed, NAMES);
    }

    #[test]
    fn every_fixture_is_a_json_object() {
        for name in NAMES {
            assert!(value(name).is_object(), "{name}");
        }
    }

    #[test]
    fn text_returns_the_raw_file() {
        let loda = text("consult_loda");
        assert!(loda.starts_with('{'));
        assert!(loda.contains("\"LEGITEXT000033205014_08-09-2023\""));
    }
}
