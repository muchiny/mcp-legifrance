//! Requêtes `GET` envoyées à Judilibre (ADR-0035, décision 3) : deux chemins, une liste fermée de
//! paramètres tirés de valeurs typées du domaine, `jurisdiction` toujours présent.
//!
//! L'hôte est celui de l'API PISTE de la configuration (`LF_PISTE_BASE_URL` compris) ; le chemin
//! remplace celui de Légifrance. L'adresse porte les mots cherchés : elle n'est jamais journalisée
//! ni recopiée dans une erreur (`http::Attempt` la tait, `classify` ne cite que le gabarit).
//!
//! Jamais envoyés : `chamber`, `formation`, `location`, `theme`, `withFileOfType` ; aucun filtre ne
//! permet d'étudier un juge (ADR-0035, décision 7). `resolve_references=true` accompagne chaque
//! requête depuis V-241 (2026-11-28) : l'API rend alors en clair la juridiction, le siège, la
//! chambre, la formation, la publication et la solution, sans rien changer d'autre.

use lf_domain::judilibre::{CaseLawQuery, CaseLawSort, JudilibreId};
use lf_domain::search::MatchMode;
use reqwest::Url;

/// Gabarit de la recherche de décisions.
pub(crate) const SEARCH: &str = "/cassation/judilibre/v1.0/search";

/// Gabarit de la lecture d'une décision.
pub(crate) const DECISION: &str = "/cassation/judilibre/v1.0/decision";

/// Noms des paramètres d'une recherche, seuls admis sur [`SEARCH`].
#[cfg(test)]
pub(crate) const SEARCH_PARAMS: [&str; 11] = [
    "query",
    "operator",
    "field",
    "jurisdiction",
    "date_start",
    "date_end",
    "sort",
    "order",
    "page",
    "page_size",
    "resolve_references",
];

/// Noms des paramètres d'une lecture, seuls admis sur [`DECISION`].
#[cfg(test)]
pub(crate) const DECISION_PARAMS: [&str; 2] = ["id", "resolve_references"];

/// Un paramètre de l'adresse : nom fixe, valeur tirée d'une valeur typée.
pub(crate) type Param = (&'static str, String);

/// Paramètres d'une recherche, dans un ordre fixe. `jurisdiction` est répété pour chaque
/// juridiction et jamais omis : vide, l'API ne viserait que la Cour de cassation. `order=desc`
/// n'accompagne que le tri par date (05 §5.18). `page` compte à partir de 0 côté API, à partir de
/// 1 côté outil.
pub(crate) fn search_params(query: &CaseLawQuery) -> Vec<Param> {
    let filters = query.filters();
    let mut params = vec![
        ("query", query.text().as_str().to_owned()),
        ("operator", operator(filters.mode).to_owned()),
    ];
    if let Some(part) = filters.part {
        params.push(("field", part.name().to_owned()));
    }
    params.extend(filters.courts.iter().map(|court| ("jurisdiction", court.code().to_owned())));
    if let Some(date) = filters.date_from {
        params.push(("date_start", date.to_string()));
    }
    if let Some(date) = filters.date_to {
        params.push(("date_end", date.to_string()));
    }
    params.push(("sort", sort(filters.sort).to_owned()));
    if filters.sort == CaseLawSort::Date {
        params.push(("order", "desc".to_owned()));
    }
    params.push(("page", query.page().get().saturating_sub(1).to_string()));
    params.push(("page_size", query.page_size().get().to_string()));
    params.push(resolve_references());
    params
}

/// Paramètres de la lecture d'une décision : son identifiant, et les intitulés en clair.
pub(crate) fn decision_params(id: &JudilibreId) -> Vec<Param> {
    vec![("id", id.as_str().to_owned()), resolve_references()]
}

/// `resolve_references=true`, dernier paramètre de chaque requête : les intitulés en clair
/// (V-241).
fn resolve_references() -> Param {
    ("resolve_references", "true".to_owned())
}

/// Valeur de `operator` : `or` (un des mots), `and` (tous) ou `exact` (expression exacte).
const fn operator(mode: MatchMode) -> &'static str {
    match mode {
        MatchMode::Any => "or",
        MatchMode::All => "and",
        MatchMode::Exact => "exact",
    }
}

/// Valeur de `sort` : `scorepub` (pertinence, puis niveau de publication) ou `date`.
const fn sort(sort: CaseLawSort) -> &'static str {
    match sort {
        CaseLawSort::Relevance => "scorepub",
        CaseLawSort::Date => "date",
    }
}

/// Adresse de `endpoint` sur l'hôte de `base` : chemin remplacé, paramètres encodés, sans
/// fragment. Le chemin de `base` (celui de Légifrance, ou d'une surcharge) ne sert pas.
pub(crate) fn url(base: &Url, endpoint: &str, params: &[Param]) -> Url {
    let mut url = base.clone();
    url.set_path(endpoint);
    url.set_fragment(None);
    url.set_query(None);
    url.query_pairs_mut().extend_pairs(params.iter().map(|(name, value)| (*name, value.as_str())));
    url
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::date::VersionDate;
    use lf_domain::judilibre::{CaseLawFilters, Court, CourtSet, SearchPart};
    use lf_domain::search::{PageSize, QueryText};

    use super::*;

    fn query(filters: CaseLawFilters, page: u32, size: u32) -> CaseLawQuery {
        CaseLawQuery::new(
            QueryText::parse("donation & usufruit").unwrap(),
            filters,
            NonZeroU32::new(page).unwrap(),
            PageSize::new(size).unwrap(),
        )
        .unwrap()
    }

    fn defaults() -> CaseLawFilters {
        CaseLawFilters {
            mode: MatchMode::All,
            courts: CourtSet::ALL,
            part: None,
            date_from: None,
            date_to: None,
            sort: CaseLawSort::Relevance,
        }
    }

    fn shown(params: &[Param]) -> Vec<(&str, &str)> {
        params.iter().map(|(name, value)| (*name, value.as_str())).collect()
    }

    #[test]
    fn a_default_search_names_every_court_and_counts_pages_from_zero() {
        let params = search_params(&query(defaults(), 1, 10));
        assert_eq!(
            shown(&params),
            [
                ("query", "donation & usufruit"),
                ("operator", "and"),
                ("jurisdiction", "cc"),
                ("jurisdiction", "ca"),
                ("jurisdiction", "tj"),
                ("jurisdiction", "tcom"),
                ("jurisdiction", "cph"),
                ("sort", "scorepub"),
                ("page", "0"),
                ("page_size", "10"),
                ("resolve_references", "true"),
            ]
        );
    }

    #[test]
    fn every_filter_becomes_its_parameter() {
        let filters = CaseLawFilters {
            mode: MatchMode::Exact,
            courts: CourtSet::of([Court::Appeal]).unwrap(),
            part: Some(SearchPart::Motivations),
            date_from: Some("2020-01-01".parse::<VersionDate>().unwrap()),
            date_to: Some("2023-12-31".parse::<VersionDate>().unwrap()),
            sort: CaseLawSort::Date,
        };
        let params = search_params(&query(filters, 3, 50));
        assert_eq!(
            shown(&params),
            [
                ("query", "donation & usufruit"),
                ("operator", "exact"),
                ("field", "motivations"),
                ("jurisdiction", "ca"),
                ("date_start", "2020-01-01"),
                ("date_end", "2023-12-31"),
                ("sort", "date"),
                ("order", "desc"),
                ("page", "2"),
                ("page_size", "50"),
                ("resolve_references", "true"),
            ]
        );
        let any =
            search_params(&query(CaseLawFilters { mode: MatchMode::Any, ..defaults() }, 1, 1));
        assert_eq!(any[1], ("operator", "or".to_owned()));
    }

    #[test]
    fn only_listed_parameters_are_ever_sent() {
        let filters = CaseLawFilters {
            part: Some(SearchPart::Visa),
            date_from: Some("2020-01-01".parse::<VersionDate>().unwrap()),
            date_to: Some("2020-01-02".parse::<VersionDate>().unwrap()),
            ..defaults()
        };
        for (name, _) in search_params(&query(filters, 2, 5)) {
            assert!(SEARCH_PARAMS.contains(&name), "{name}");
        }
        let id = JudilibreId::parse("5fca624b7eeb384a40cafb85").unwrap();
        let params = decision_params(&id);
        assert_eq!(
            shown(&params),
            [("id", "5fca624b7eeb384a40cafb85"), ("resolve_references", "true")]
        );
        assert!(params.iter().all(|(name, _)| DECISION_PARAMS.contains(name)));
    }

    #[test]
    fn the_address_keeps_the_host_and_replaces_path_query_and_fragment() {
        let base =
            Url::parse("https://api.piste.gouv.fr/dila/legifrance/lf-engine-app?x=1#top").unwrap();
        let params = [("query", "nue-propriété & usufruit".to_owned()), ("page", "0".to_owned())];
        assert_eq!(
            url(&base, SEARCH, &params).as_str(),
            "https://api.piste.gouv.fr/cassation/judilibre/v1.0/search?query=nue-propri%C3%A9t%C3%A9+%26+usufruit&page=0"
        );
        let local = Url::parse("http://127.0.0.1:8080/dila/legifrance/lf-engine-app").unwrap();
        let id = [("id", "5fca624b7eeb384a40cafb85".to_owned())];
        assert_eq!(
            url(&local, DECISION, &id).as_str(),
            "http://127.0.0.1:8080/cassation/judilibre/v1.0/decision?id=5fca624b7eeb384a40cafb85"
        );
    }
}
