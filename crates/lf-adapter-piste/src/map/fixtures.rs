//! Fixtures de référence des tests de mapping : les seize réponses de `tests/fixtures/reference`.
//!
//! Les fichiers sont lus par `include_str!` et jamais reformatés ni réparés sur disque : les
//! tests de mapping partent des octets capturés, avec leurs sentinelles, leurs chaînes vides et
//! leurs formes inattendues. [`juri_with`] construit en plus une réponse synthétique de
//! `/consult/juri`, sans passer par le JSON. [`map_body`] et [`map_fixture`] mènent un corps
//! jusqu'à l'entité du domaine ([`Mapped`]) par le décodage et le mapper de production. Ce module
//! n'existe que sous `cfg(test)`.
//!
//! Il porte aussi les aides communes aux tests des mappers : [`read`] (un corps de test lu comme
//! le fait la production), [`decode_error`] (l'erreur `Decode` attendue), [`d`] (une date civile)
//! et la macro `text!` (`Some(valeur.to_owned())`), importée par `use crate::map::fixtures::text`.

use std::num::NonZeroU32;

use lf_application::ports::CorpusError;
use lf_domain::contract::{Citable, CitationContract};
use lf_domain::date::CivilDate;
use lf_domain::entity::{
    Act, ArticleText, CompanyAgreement, Convention, Decision, Deliberation, SearchPage,
};
use lf_domain::fond::Fond;
use lf_domain::id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId};
use lf_domain::search::{MatchMode, PageSize, QueryText, SearchQuery};
use serde::de::DeserializeOwned;

use super::act::act;
use super::agreement::agreement;
use super::article::article;
use super::convention::convention;
use super::decision::decision;
use super::deliberation::deliberation;
use super::search::search_page;
use super::{Diagnostics, endpoint};
use crate::dto::decode::decode;
use crate::dto::juri::{ConsultJuriResponseDto, JuriTextDto};
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

/// Golden de parité (`parite-golden.json`) lu comme valeur JSON : la sortie des outils de la
/// référence pour les seize fixtures.
///
/// # Panics
///
/// Si le fichier n'est pas du JSON.
pub(crate) fn golden() -> serde_json::Value {
    serde_json::from_str(include_str!("../../tests/fixtures/reference/parite-golden.json"))
        .expect("golden is JSON")
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

/// `Some(valeur.to_owned())` : une macro et non une fonction, que `clippy::unnecessary_wraps`
/// refuserait. Exportée sous le nom `text` ; un autre nom ici, car la fonction [`text`] occupe déjà
/// ce nom dans l'espace des valeurs du module.
macro_rules! some_owned {
    ($value:expr) => {
        Some($value.to_owned())
    };
}

pub(crate) use some_owned as text;

/// Corps de test `json` lu dans `T` comme le fait la production : `decode` à l'endpoint
/// `endpoint`.
///
/// # Panics
///
/// Si le corps ne se lit pas dans `T` : un corps de test mal écrit.
pub(crate) fn read<T: DeserializeOwned>(json: &str, endpoint: &str) -> T {
    decode(json, endpoint).expect("test body decodes")
}

/// [`CorpusError::Decode`] attendu, de détail `<endpoint>: <rest>`.
pub(crate) fn decode_error(endpoint: &str, rest: &str) -> CorpusError {
    CorpusError::Decode { detail: format!("{endpoint}: {rest}") }
}

/// Date civile `year`-`month`-`day`.
///
/// # Panics
///
/// Si la date n'existe pas : une faute de frappe d'un test.
pub(crate) fn d(year: u16, month: u8, day: u8) -> CivilDate {
    CivilDate::new(year, month, day).expect("valid civil date")
}

/// Réponse synthétique de `/consult/juri` : `JuriTextDto::default()` modifié par `edit`, sous la
/// clé `text`.
pub(crate) fn juri_with(edit: impl FnOnce(&mut JuriTextDto)) -> ConsultJuriResponseDto {
    let mut text = JuriTextDto::default();
    edit(&mut text);
    ConsultJuriResponseDto { text: Some(text) }
}

/// Entité produite par [`map_body`] : une variante par mapper.
#[derive(Debug)]
pub(crate) enum Mapped {
    /// Page de résultats de `/search`.
    Search(SearchPage),
    /// Loi ou décret de `/consult/lawDecree`.
    Act(Act),
    /// Convention collective de `/consult/kaliText`.
    Convention(Convention),
    /// Article de `/consult/getArticle`.
    Article(ArticleText),
    /// Décision de `/consult/juri`.
    Decision(Decision),
    /// Délibération de `/consult/cnil`.
    Deliberation(Deliberation),
    /// Accord d'entreprise de `/consult/acco`.
    Agreement(CompanyAgreement),
}

impl Mapped {
    /// Contrats de citation de l'entité : un par résultat d'une page de recherche, sinon un seul.
    pub(crate) fn contracts(&self) -> Vec<CitationContract> {
        match self {
            Self::Search(page) => page.hits.iter().map(Citable::contract).collect(),
            Self::Act(act) => vec![act.contract()],
            Self::Convention(convention) => vec![convention.contract()],
            Self::Article(article) => vec![article.contract()],
            Self::Decision(decision) => vec![decision.contract()],
            Self::Deliberation(deliberation) => vec![deliberation.contract()],
            Self::Agreement(agreement) => vec![agreement.contract()],
        }
    }

    /// Éléments reçus puis écartés par le mapping : résultats d'une page de recherche, entrées
    /// de la table des matières d'un texte ; zéro pour les autres entités, qui n'en écartent pas.
    pub(crate) fn dropped(&self) -> u32 {
        match self {
            Self::Search(page) => page.dropped,
            Self::Act(act) => act.body.toc.dropped(),
            Self::Convention(convention) => convention.body.toc.dropped(),
            Self::Article(_) | Self::Decision(_) | Self::Deliberation(_) | Self::Agreement(_) => 0,
        }
    }
}

/// Corps `text` d'une réponse de genre `kind`, lu comme le fait la production : `decode` à
/// l'endpoint du genre, puis le mapper, qui compte ses anomalies dans `diags`.
///
/// Une recherche est mappée pour la requête « golden » du fond, page 1, taille et mode par
/// défaut ; une consultation pour `requested`, lu dans le type d'identifiant du mapper (ignoré
/// pour une recherche).
///
/// # Errors
///
/// L'erreur de `decode` (corps illisible ou d'une autre forme), puis celle du mapper.
///
/// # Panics
///
/// Si `requested` n'est pas un identifiant du type attendu : une faute de frappe d'un test.
pub(crate) fn map_body(
    kind: Kind,
    text: &str,
    requested: &str,
    diags: &mut Diagnostics,
) -> Result<Mapped, CorpusError> {
    match kind {
        Kind::Search(fond) => decode(text, endpoint::SEARCH)
            .and_then(|dto| search_page(dto, &golden_query(fond), diags))
            .map(Mapped::Search),
        Kind::LawDecree => decode(text, endpoint::LAW_DECREE)
            .and_then(|dto| act(dto, &LegiTextId::parse(requested).expect("LEGITEXT"), diags))
            .map(Mapped::Act),
        Kind::KaliText => decode(text, endpoint::KALI_TEXT)
            .and_then(|dto| {
                convention(dto, &KaliTextId::parse(requested).expect("KALITEXT"), diags)
            })
            .map(Mapped::Convention),
        Kind::GetArticle => decode(text, endpoint::GET_ARTICLE)
            .and_then(|dto| article(dto, &ArticleId::parse(requested).expect("article id"), diags))
            .map(Mapped::Article),
        Kind::Juri => decode(text, endpoint::JURI)
            .and_then(|dto| {
                decision(dto, &DecisionId::parse(requested).expect("decision id"), diags)
            })
            .map(Mapped::Decision),
        Kind::Cnil => decode(text, endpoint::CNIL)
            .and_then(|dto| {
                deliberation(dto, &CnilTextId::parse(requested).expect("CNILTEXT"), diags)
            })
            .map(Mapped::Deliberation),
        Kind::Acco => decode(text, endpoint::ACCO)
            .and_then(|dto| agreement(dto, &AccoTextId::parse(requested).expect("ACCOTEXT"), diags))
            .map(Mapped::Agreement),
    }
}

/// Requête « golden » de `fond` : page 1, taille et mode par défaut, sans filtre.
///
/// # Panics
///
/// Jamais : la requête est valide pour tout fond (pas de `From<DomainError>` vers
/// `CorpusError`, d'où l'`expect`).
fn golden_query(fond: Fond) -> SearchQuery {
    QueryText::parse("golden")
        .and_then(|text| {
            SearchQuery::new(
                fond,
                text,
                NonZeroU32::MIN,
                PageSize::default(),
                MatchMode::default(),
                None,
                None,
            )
        })
        .expect("golden query")
}

/// Fixture `fixture` mappée comme le fait la production : [`map_body`] sur
/// `decoded(fixture, true)`, pour l'identifiant demandé à la capture.
///
/// # Errors
///
/// Voir [`map_body`].
pub(crate) fn map_fixture(
    fixture: &Fixture,
    diags: &mut Diagnostics,
) -> Result<Mapped, CorpusError> {
    map_body(fixture.kind, &decoded(fixture, true).text, fixture.requested, diags)
}

#[cfg(test)]
mod tests {
    use lf_domain::ecli::Ecli;

    use super::*;
    use crate::dto::acco::ConsultAccoResponseDto;
    use crate::dto::article::GetArticleResponseDto;
    use crate::dto::consult_text::{ConsultTextDto, TocSectionDto};
    use crate::dto::de::RawDate;
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
        assert_eq!(article.texte_html, None);
    }

    /// Valeurs affirmées sur ce groupe de fixtures.
    fn assert_decision_fixtures() {
        let juri = decode_as::<ConsultJuriResponseDto>("consult_juri").text.expect("text");
        assert_eq!(juri.num.as_deref(), Some("12500399"));
        assert_eq!(juri.numero_affaire, ["24-13.470"]);
        assert_eq!(juri.sommaire.len(), 1);
        assert_eq!(juri.ecli.as_deref(), Some("ECLI:FR:CCASS:2025:C100399"));

        let cetat = decode_as::<ConsultJuriResponseDto>("consult_juri_cetat").text.expect("text");
        assert_eq!(cetat.num.as_deref(), Some("506507"));
        assert_eq!(cetat.sommaire.len(), 6);

        let constit =
            decode_as::<ConsultJuriResponseDto>("consult_juri_constit").text.expect("text");
        assert_eq!(constit.num.as_deref(), Some("2025-1173"));
        assert_eq!(constit.sommaire.len(), 0);

        let cnil = decode_as::<ConsultJuriResponseDto>("consult_cnil").text.expect("text");
        assert_eq!(cnil.num.as_deref(), Some("SAN-2026-002"));
        assert_eq!(cnil.sommaire.len(), 0);
    }

    /// Valeurs affirmées sur ce groupe de fixtures.
    fn assert_agreement_fixture() {
        let acco = decode_as::<ConsultAccoResponseDto>("consult_acco").acco.expect("acco");
        assert_eq!(acco.code_idcc.as_deref(), Some("275"));
        assert_eq!(acco.themes.len(), 1);
        assert_eq!(acco.syndicats.len(), 3);
        assert_eq!(acco.date_depot, Some(RawDate::Millis(1_772_150_400_000)));
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

    /// Racine des URL de legifrance.gouv.fr.
    const LEGIFRANCE: &str = "https://www.legifrance.gouv.fr/";

    /// Contrats attendus de chaque entrée du golden : clé, nombre de contrats, chemin sous
    /// [`LEGIFRANCE`] par lequel commence chaque URL et ECLI natif de chaque contrat.
    const GOLDEN_CONTRACTS: [(&str, usize, &str, Option<&str>); 17] = [
        ("search_loda.json", 2, "loda/id/", None),
        ("search_code.json", 1, "codes/texte_lc/", None),
        ("search_juri.json", 2, "juri/id/", None),
        ("search_constit.json", 3, "cons/id/", None),
        ("search_cetat.json", 3, "ceta/id/", None),
        ("search_cnil.json", 2, "cnil/id/", None),
        ("search_kali.json", 2, "conv_coll/id/", None),
        ("search_acco.json", 2, "acco/id/", None),
        ("consult_loda.json", 1, "loda/id/LEGITEXT000033205014", None),
        ("consult_article.json", 1, "codes/article_lc/LEGIARTI000006419288", None),
        (
            "consult_juri.json",
            1,
            "juri/id/JURITEXT000051743650",
            Some("ECLI:FR:CCASS:2025:C100399"),
        ),
        (
            "consult_juri_constit.json",
            1,
            "cons/id/CONSTEXT000052555275",
            Some("ECLI:FR:CC:2025:2025.1173.QPC"),
        ),
        (
            "consult_juri_cetat.json",
            1,
            "ceta/id/CETATEXT000054178503",
            Some("ECLI:FR:CECHR:2026:506507.20260529"),
        ),
        ("consult_cnil.json", 1, "cnil/id/CNILTEXT000053352643", None),
        ("consult_kali.json", 1, "conv_coll/id/KALITEXT000049878950", None),
        ("consult_acco.json", 1, "acco/id/ACCOTEXT000053643828", None),
        (
            "consult_article.json[id=KALIARTI000049886059]",
            1,
            "codes/article_lc/LEGIARTI000006419288",
            None,
        ),
    ];

    /// Clé du golden ⇒ nom de la fixture et identifiant demandé par la variante :
    /// `consult_article.json[id=KALIARTI000049886059]` ⇒
    /// `("consult_article", Some("KALIARTI000049886059"))`, `search_code.json` ⇒
    /// `("search_code", None)`.
    fn golden_key(key: &str) -> (&str, Option<&str>) {
        let (file, variant) = key
            .strip_suffix(']')
            .and_then(|rest| rest.split_once("[id="))
            .map_or((key, None), |(file, id)| (file, Some(id)));
        (file.strip_suffix(".json").expect("golden key ends with .json"), variant)
    }

    /// D-30 (`citation_never_empty`, `ported_no_fabricated_eli`) : chaque entrée du golden se
    /// mappe et chacun de ses contrats a une citation, une URL du bon chemin, l'ECLI natif
    /// attendu et jamais d'ELI.
    #[test]
    fn every_normalized_fixture_has_contract() {
        let golden = golden();
        let normalizer = golden["normalizer"].as_object().expect("normalizer is an object");
        let mut keys: Vec<&str> = normalizer.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected: Vec<&str> = GOLDEN_CONTRACTS.iter().map(|(key, ..)| *key).collect();
        expected.sort_unstable();
        assert_eq!(keys.len(), 17);
        assert_eq!(keys, expected);

        let mut covered = Vec::new();
        let mut total = 0;
        for (key, count, path, ecli) in GOLDEN_CONTRACTS {
            let (name, variant) = golden_key(key);
            let fixture = fixture(name);
            covered.push(name);
            let requested = variant.unwrap_or(fixture.requested);
            let mut diags = Diagnostics::default();
            let mapped =
                map_body(fixture.kind, &decoded(fixture, true).text, requested, &mut diags)
                    .expect(key);
            assert_eq!(mapped.dropped(), 0, "{key}");
            let contracts = mapped.contracts();
            assert_eq!(contracts.len(), count, "{key}");
            let prefix = format!("{LEGIFRANCE}{path}");
            for contract in &contracts {
                let url = contract.source_url.as_str();
                assert!(!contract.citation.as_str().trim().is_empty(), "{key}");
                assert!(url.starts_with(&prefix), "{key}: {url}");
                assert!(contract.eli.is_none(), "{key}");
                assert_eq!(contract.ecli.as_ref().map(Ecli::as_str), ecli, "{key}");
            }
            if let Mapped::Search(page) = &mapped {
                assert_eq!(page.hits.len(), count, "{key}");
                for (hit, contract) in page.hits.iter().zip(&contracts) {
                    assert!(contract.source_url.as_str().ends_with(hit.id.as_str()), "{key}");
                }
            }
            total += contracts.len();
        }
        assert_eq!(total, 26);
        covered.sort_unstable();
        assert_eq!(covered.iter().filter(|name| **name == "consult_article").count(), 2);
        covered.dedup();
        assert_eq!(covered, NAMES);
    }

    #[test]
    fn fixtures_map_without_diagnostics() {
        for fixture in &ALL {
            let mut diags = Diagnostics::default();
            let mapped = map_fixture(fixture, &mut diags).expect(fixture.name);
            assert_eq!(diags, Diagnostics::default(), "{}", fixture.name);
            assert!(!mapped.contracts().is_empty(), "{}", fixture.name);
        }
    }

    #[test]
    fn kaliarti_variant_keeps_the_returned_id() {
        let text = decoded(fixture("consult_article"), true).text;
        let mut diags = Diagnostics::default();
        let variant = map_body(Kind::GetArticle, &text, "KALIARTI000049886059", &mut diags)
            .expect("variant maps");
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });
        let contracts = variant.contracts();
        assert_eq!(contracts.len(), 1);
        assert_eq!(
            contracts[0].source_url.as_str(),
            "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
        );
        let nominal = map_fixture(fixture("consult_article"), &mut Diagnostics::default())
            .expect("nominal maps");
        assert_eq!(format!("{variant:?}"), format!("{nominal:?}"));
    }

    #[test]
    fn map_body_rejects_a_body_of_the_wrong_shape() {
        let mut diags = Diagnostics::default();
        let shape = map_body(Kind::Acco, "null", "ACCOTEXT000053643828", &mut diags).unwrap_err();
        assert_eq!(
            shape,
            CorpusError::Decode {
                detail:
                    "/consult/acco: .: invalid type: null, expected struct ConsultAccoResponseDto"
                        .into()
            }
        );
        let missing = map_body(Kind::Acco, r#"{"acco":null}"#, "ACCOTEXT000053643828", &mut diags)
            .unwrap_err();
        assert_eq!(missing, CorpusError::NotFound { what: "/consult/acco".into() });
        assert_eq!(diags, Diagnostics::default());
    }

    /// Limite 2 de 04 §8.5 : un tableau est lu comme l'objet de ses premiers champs, dans l'ordre
    /// de déclaration (les DTO portent `#[serde(default)]`). Le mapping n'en tire donc rien
    /// qu'un objet ne donnerait pas.
    #[test]
    fn an_array_reads_as_the_object_of_its_first_fields() {
        let id = "LEGITEXT000006070721";
        let positional: ConsultTextDto =
            read(&format!(r#"["{id}","{id}","T"]"#), endpoint::LAW_DECREE);
        let object: ConsultTextDto =
            read(&format!(r#"{{"id":"{id}","cid":"{id}","title":"T"}}"#), endpoint::LAW_DECREE);
        assert_eq!(positional, object);

        let positional: GetArticleResponseDto =
            read(r#"{"article":["LEGIARTI000006419288"]}"#, endpoint::GET_ARTICLE);
        let object: GetArticleResponseDto =
            read(r#"{"article":{"id":"LEGIARTI000006419288"}}"#, endpoint::GET_ARTICLE);
        assert_eq!(positional, object);

        let positional: ConsultJuriResponseDto =
            read(r#"{"text":["JURITEXT000051743650","JURI"]}"#, endpoint::JURI);
        let object: ConsultJuriResponseDto =
            read(r#"{"text":{"id":"JURITEXT000051743650","origine":"JURI"}}"#, endpoint::JURI);
        assert_eq!(positional, object);

        let positional: ConsultAccoResponseDto =
            read(r#"{"acco":["ACCOTEXT000053643828"]}"#, endpoint::ACCO);
        let object: ConsultAccoResponseDto =
            read(r#"{"acco":{"id":"ACCOTEXT000053643828"}}"#, endpoint::ACCO);
        assert_eq!(positional, object);
    }

    /// Limite 2 de 04 §8.5 : un tableau vide est un objet vide, que `require` refuse en
    /// `NotFound` sans toucher aux compteurs ; un résultat de recherche vide est écarté et compté.
    #[test]
    fn an_empty_array_maps_to_not_found_or_a_dropped_hit() {
        let cases = [
            (Kind::LawDecree, "[]", "LEGITEXT000006070721", endpoint::LAW_DECREE),
            (Kind::KaliText, "[]", "KALITEXT000049878950", endpoint::KALI_TEXT),
            (Kind::GetArticle, r#"{"article":[]}"#, "LEGIARTI000006419288", endpoint::GET_ARTICLE),
            (Kind::Juri, r#"{"text":[]}"#, "JURITEXT000051743650", endpoint::JURI),
            (Kind::Cnil, r#"{"text":[]}"#, "CNILTEXT000053352643", endpoint::CNIL),
            (Kind::Acco, r#"{"acco":[]}"#, "ACCOTEXT000053643828", endpoint::ACCO),
        ];
        for (kind, body, requested, what) in cases {
            let mut diags = Diagnostics::default();
            let error = map_body(kind, body, requested, &mut diags).unwrap_err();
            assert_eq!(error, CorpusError::NotFound { what: what.into() }, "{body}");
            assert_eq!(diags, Diagnostics::default(), "{body}");
        }

        let mut diags = Diagnostics::default();
        let page = map_body(Kind::Search(Fond::Juri), r#"{"results":[[]]}"#, "", &mut diags);
        assert_eq!(page.map(|mapped| mapped.dropped()), Ok(1));
        assert_eq!(diags, Diagnostics { dropped_hits: 1, ..Diagnostics::default() });
    }
}
