//! Fixtures de référence des tests de mapping : les seize réponses de `tests/fixtures/reference`.
//!
//! Les fichiers sont lus par `include_str!` et jamais reformatés ni réparés sur disque : les
//! tests de mapping partent des octets capturés, avec leurs sentinelles, leurs chaînes vides et
//! leurs formes inattendues. Ce module n'existe que sous `cfg(test)`.

use lf_domain::fond::Fond;
use serde::de::DeserializeOwned;

use super::endpoint;
use crate::dto::decode::decode;
use crate::encoding::{DecodedBody, decode_body};

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

/// Genre de réponse d'une fixture : ce que le mapper et le DTO attendent d'elle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Réponse de `/search` pour un fond.
    Search(Fond),
    /// Texte de loi ou de décret, ou code.
    LawDecree,
    /// Texte de convention collective.
    KaliText,
    /// Article.
    GetArticle,
    /// Décision de justice (judiciaire, administrative ou constitutionnelle).
    Juri,
    /// Délibération de la CNIL.
    Cnil,
    /// Accord d'entreprise.
    Acco,
}

/// Une fixture de référence : son nom, l'endpoint qui l'a produite et l'identifiant demandé.
#[derive(Debug)]
pub(crate) struct Fixture {
    /// Nom du fichier, sans extension (un élément de [`NAMES`]).
    pub(crate) name: &'static str,
    /// Gabarit d'endpoint de la réponse.
    pub(crate) endpoint: &'static str,
    /// Genre de réponse.
    pub(crate) kind: Kind,
    /// Identifiant demandé à la capture, vide pour une recherche.
    pub(crate) requested: &'static str,
}

impl Fixture {
    /// Octets de la réponse, tels qu'écrits sur disque.
    pub(crate) fn bytes(&self) -> &'static [u8] {
        text(self.name).as_bytes()
    }
}

/// Les seize fixtures, dans l'ordre de [`NAMES`].
pub(crate) static ALL: [Fixture; 16] = [
    Fixture {
        name: "consult_acco",
        endpoint: endpoint::ACCO,
        kind: Kind::Acco,
        requested: "ACCOTEXT000053643828",
    },
    Fixture {
        name: "consult_article",
        endpoint: endpoint::GET_ARTICLE,
        kind: Kind::GetArticle,
        requested: "LEGIARTI000006419288",
    },
    Fixture {
        name: "consult_cnil",
        endpoint: endpoint::CNIL,
        kind: Kind::Cnil,
        requested: "CNILTEXT000053352643",
    },
    Fixture {
        name: "consult_juri",
        endpoint: endpoint::JURI,
        kind: Kind::Juri,
        requested: "JURITEXT000051743650",
    },
    Fixture {
        name: "consult_juri_cetat",
        endpoint: endpoint::JURI,
        kind: Kind::Juri,
        requested: "CETATEXT000054178503",
    },
    Fixture {
        name: "consult_juri_constit",
        endpoint: endpoint::JURI,
        kind: Kind::Juri,
        requested: "CONSTEXT000052555275",
    },
    Fixture {
        name: "consult_kali",
        endpoint: endpoint::KALI_TEXT,
        kind: Kind::KaliText,
        requested: "KALITEXT000049878950",
    },
    Fixture {
        name: "consult_loda",
        endpoint: endpoint::LAW_DECREE,
        kind: Kind::LawDecree,
        requested: "LEGITEXT000033205014",
    },
    Fixture {
        name: "search_acco",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Acco),
        requested: "",
    },
    Fixture {
        name: "search_cetat",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Cetat),
        requested: "",
    },
    Fixture {
        name: "search_cnil",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Cnil),
        requested: "",
    },
    Fixture {
        name: "search_code",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Code),
        requested: "",
    },
    Fixture {
        name: "search_constit",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Constit),
        requested: "",
    },
    Fixture {
        name: "search_juri",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Juri),
        requested: "",
    },
    Fixture {
        name: "search_kali",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Kali),
        requested: "",
    },
    Fixture {
        name: "search_loda",
        endpoint: endpoint::SEARCH,
        kind: Kind::Search(Fond::Loda),
        requested: "",
    },
];

/// La fixture `name`.
///
/// # Panics
///
/// Si `name` n'est pas dans [`NAMES`] : une faute de frappe d'un test, pas une donnée.
pub(crate) fn fixture(name: &str) -> &'static Fixture {
    ALL.iter().find(|fixture| fixture.name == name).expect("known fixture name")
}

/// Corps de `fixture` passé par le décodage de production : UTF-8 strict, détection du mojibake
/// et, si `repair`, sa réparation.
///
/// # Panics
///
/// Si les octets ne sont pas de l'UTF-8 : les fixtures le sont toutes.
pub(crate) fn decoded(fixture: &Fixture, repair: bool) -> DecodedBody {
    decode_body(fixture.bytes(), fixture.endpoint, repair).expect("fixtures are UTF-8")
}

/// Fixture `name` lue comme le fait la production : `decoded(fixture(name), true)`, puis
/// `decode` à l'endpoint de la fixture.
///
/// # Panics
///
/// Si `name` est inconnu ou si le corps ne se lit pas dans `T`.
pub(crate) fn decode_as<T: DeserializeOwned>(name: &str) -> T {
    let fixture = fixture(name);
    decode(&decoded(fixture, true).text, fixture.endpoint).expect("fixture decodes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::acco::ConsultAccoResponseDto;
    use crate::dto::article::GetArticleResponseDto;
    use crate::dto::consult_text::{ConsultTextDto, TocSectionDto};
    use crate::dto::de::RawDate;
    use crate::dto::juri::ConsultJuriResponseDto;
    use crate::dto::search::SearchResponseDto;

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

    /// Table attendue : nom, gabarit d'endpoint, genre et identifiant demandé, écrits en dur.
    fn expected_table() -> [(&'static str, &'static str, Kind, &'static str); 16] {
        [
            ("consult_acco", endpoint::ACCO, Kind::Acco, "ACCOTEXT000053643828"),
            ("consult_article", endpoint::GET_ARTICLE, Kind::GetArticle, "LEGIARTI000006419288"),
            ("consult_cnil", endpoint::CNIL, Kind::Cnil, "CNILTEXT000053352643"),
            ("consult_juri", endpoint::JURI, Kind::Juri, "JURITEXT000051743650"),
            ("consult_juri_cetat", endpoint::JURI, Kind::Juri, "CETATEXT000054178503"),
            ("consult_juri_constit", endpoint::JURI, Kind::Juri, "CONSTEXT000052555275"),
            ("consult_kali", endpoint::KALI_TEXT, Kind::KaliText, "KALITEXT000049878950"),
            ("consult_loda", endpoint::LAW_DECREE, Kind::LawDecree, "LEGITEXT000033205014"),
            ("search_acco", endpoint::SEARCH, Kind::Search(Fond::Acco), ""),
            ("search_cetat", endpoint::SEARCH, Kind::Search(Fond::Cetat), ""),
            ("search_cnil", endpoint::SEARCH, Kind::Search(Fond::Cnil), ""),
            ("search_code", endpoint::SEARCH, Kind::Search(Fond::Code), ""),
            ("search_constit", endpoint::SEARCH, Kind::Search(Fond::Constit), ""),
            ("search_juri", endpoint::SEARCH, Kind::Search(Fond::Juri), ""),
            ("search_kali", endpoint::SEARCH, Kind::Search(Fond::Kali), ""),
            ("search_loda", endpoint::SEARCH, Kind::Search(Fond::Loda), ""),
        ]
    }

    #[test]
    fn fixtures_table_lists_the_sixteen_reference_files() {
        assert_eq!(ALL.len(), 16);
        let names: Vec<&str> = ALL.iter().map(|fixture| fixture.name).collect();
        assert_eq!(names, NAMES);
        for (fixture, (name, endpoint, kind, requested)) in ALL.iter().zip(expected_table()) {
            assert_eq!(fixture.name, name);
            assert_eq!(fixture.endpoint, endpoint, "{name}");
            assert_eq!(fixture.kind, kind, "{name}");
            assert_eq!(fixture.requested, requested, "{name}");
            assert!(!fixture.bytes().is_empty(), "{name}");
            assert_eq!(fixture.bytes(), text(name).as_bytes(), "{name}");
        }
    }

    #[test]
    fn fixture_looks_up_by_name() {
        assert_eq!(fixture("search_code").kind, Kind::Search(Fond::Code));
        assert_eq!(fixture("consult_acco").requested, "ACCOTEXT000053643828");
    }

    #[test]
    fn mojibake_detection_per_fixture() {
        let repaired = [
            "consult_article",
            "consult_juri",
            "consult_loda",
            "search_code",
            "search_juri",
            "search_loda",
        ];
        for fixture in &ALL {
            let name = fixture.name;
            let expected = if repaired.contains(&name) {
                (true, true)
            } else if name == "consult_juri_constit" {
                (true, false)
            } else {
                (false, false)
            };
            let with_repair = decoded(fixture, true);
            assert_eq!((with_repair.detected, with_repair.repaired), expected, "{name}");
            let without = decoded(fixture, false);
            assert_eq!(without.detected, expected.0, "{name}");
            assert!(!without.repaired, "{name}");
            assert_eq!(without.text.as_bytes(), fixture.bytes(), "{name}");
        }
    }

    /// Nombre de sections et d'articles d'un arbre, sections de tête comprises.
    fn count_toc(sections: &[TocSectionDto]) -> (usize, usize) {
        sections.iter().fold((sections.len(), 0), |(sections_count, articles_count), section| {
            let (nested_sections, nested_articles) = count_toc(&section.sections);
            (
                sections_count + nested_sections,
                articles_count + section.articles.len() + nested_articles,
            )
        })
    }

    /// Nombre d'extraits de chaque résultat de recherche.
    fn extracts_per_result(response: &SearchResponseDto) -> Vec<usize> {
        response
            .results
            .iter()
            .map(|result| result.sections.iter().map(|section| section.extracts.len()).sum())
            .collect()
    }

    /// Ligne attendue d'une recherche : nom, résultats, total, total d'articles, pagination et
    /// extraits de chaque résultat.
    type SearchRow = (&'static str, usize, u64, u64, &'static str, &'static [usize]);

    /// Valeurs affirmées sur ce groupe de fixtures.
    fn assert_search_fixtures() {
        let table: [SearchRow; 8] = [
            ("search_acco", 2, 54_491, 0, "DEFAUT", &[2, 2]),
            ("search_cetat", 3, 51_531, 0, "DEFAUT", &[2, 2, 2]),
            ("search_cnil", 2, 1_134, 0, "DEFAUT", &[0, 0]),
            ("search_code", 1, 1, 1, "ARTICLE", &[1]),
            ("search_constit", 3, 1_817, 0, "DEFAUT", &[0, 0, 0]),
            ("search_juri", 2, 122_147, 0, "DEFAUT", &[4, 4]),
            ("search_kali", 2, 27_337, 0, "DEFAUT", &[1, 3]),
            ("search_loda", 2, 1_786, 0, "DEFAUT", &[0, 0]),
        ];
        for (name, results, total, articles, pagination, extracts) in table {
            let response = decode_as::<SearchResponseDto>(name);
            assert_eq!(response.results.len(), results, "{name}");
            assert_eq!(response.total_result_number, Some(total), "{name}");
            assert_eq!(response.total_article_result_number, Some(articles), "{name}");
            assert_eq!(response.type_pagination.as_deref(), Some(pagination), "{name}");
            assert_eq!(extracts_per_result(&response), extracts, "{name}");
        }
        let first_title = |name: &str, index: usize| {
            decode_as::<SearchResponseDto>(name).results[index].titles[0].id.clone()
        };
        assert_eq!(
            first_title("search_loda", 0).as_deref(),
            Some("LEGITEXT000033205014_08-09-2023")
        );
        assert_eq!(
            first_title("search_loda", 1).as_deref(),
            Some("LEGITEXT000033696785_28-12-2016")
        );
        assert_eq!(
            first_title("search_code", 0).as_deref(),
            Some("LEGITEXT000006070721_09-04-2026")
        );
    }

    /// Valeurs affirmées sur ce groupe de fixtures.
    fn assert_text_fixtures() {
        let loda = decode_as::<ConsultTextDto>("consult_loda");
        assert_eq!(loda.id.as_deref(), Some("LEGITEXT000033205014_08-09-2023"));
        assert_eq!(loda.sections.len(), 3);
        assert_eq!(count_toc(&loda.sections), (26, 62));
        assert_eq!(loda.conteneurs.len(), 0);

        let kali = decode_as::<ConsultTextDto>("consult_kali");
        assert_eq!(count_toc(&kali.sections), (1, 1));
        assert_eq!(kali.conteneurs.len(), 1);
        assert_eq!(kali.conteneurs[0].num.as_deref(), Some("2121"));
    }

    /// Valeurs affirmées sur ce groupe de fixtures.
    fn assert_article_fixture() {
        let article = decode_as::<GetArticleResponseDto>("consult_article")
            .article
            .expect("article is present");
        assert_eq!(article.num.as_deref(), Some("9"));
        assert_eq!(article.text_titles.len(), 1);
        assert_eq!(article.text_titles[0].nature.as_deref(), Some("CODE"));
        assert_eq!(article.context.titres_tm.len(), 2);
        assert_eq!(article.context.titre_txt.len(), 1);
        assert_eq!(article.texte_html, None);
    }

    /// Valeurs affirmées sur ce groupe de fixtures.
    fn assert_decision_fixtures() {
        let juri = decode_as::<ConsultJuriResponseDto>("consult_juri").text.expect("text");
        assert_eq!(juri.num.as_deref(), Some("12500399"));
        assert_eq!(juri.numero_affaire, ["24-13.470"]);
        assert_eq!(juri.sommaire.len(), 1);
        assert_eq!(juri.liens.len(), 2);
        assert_eq!(juri.ecli.as_deref(), Some("ECLI:FR:CCASS:2025:C100399"));

        let cetat = decode_as::<ConsultJuriResponseDto>("consult_juri_cetat").text.expect("text");
        assert_eq!(cetat.num.as_deref(), Some("506507"));
        assert_eq!(cetat.sommaire.len(), 6);
        assert_eq!(cetat.liens.len(), 0);

        let constit =
            decode_as::<ConsultJuriResponseDto>("consult_juri_constit").text.expect("text");
        assert_eq!(constit.num.as_deref(), Some("2025-1173"));
        assert_eq!(constit.sommaire.len(), 0);

        let cnil = decode_as::<ConsultJuriResponseDto>("consult_cnil").text.expect("text");
        assert_eq!(cnil.num.as_deref(), Some("SAN-2026-002"));
        assert_eq!(cnil.sommaire.len(), 0);
        assert_eq!(cnil.liens.len(), 0);
    }

    /// Valeurs affirmées sur ce groupe de fixtures.
    fn assert_agreement_fixture() {
        let acco = decode_as::<ConsultAccoResponseDto>("consult_acco").acco.expect("acco");
        assert_eq!(acco.code_idcc.as_deref(), Some("275"));
        assert_eq!(acco.themes.len(), 1);
        assert_eq!(acco.syndicats.len(), 3);
        assert_eq!(acco.date_fin, Some(RawDate::Millis(32_472_144_000_000)));
    }

    #[test]
    fn dto_parses_every_fixture() {
        assert_search_fixtures();
        assert_text_fixtures();
        assert_article_fixture();
        assert_decision_fixtures();
        assert_agreement_fixture();
    }

    #[test]
    fn text_returns_the_raw_file() {
        let loda = text("consult_loda");
        assert!(loda.starts_with('{'));
        assert!(loda.contains("\"LEGITEXT000033205014_08-09-2023\""));
    }
}
