//! Corps de la requête `POST /search` (04 §6).
//!
//! `search_body` traduit une `SearchQuery` déjà validée par le domaine en octets JSON, prêts
//! pour `post_json`. Trois gardes recopient des règles du domaine (filtre de période sur CODE,
//! `version_date` hors LODA et CODE, page hors de portée) : une `SearchQuery` valide ne les
//! atteint pas. La quatrième, `version_date` absente sur LODA ou CODE, porte un contrat avec
//! l'application : le domaine ne l'exige pas (04 §6.2) et c'est `lf-application` qui doit remplir
//! la date du jour (00 §7) ; tant qu'elle ne le fait pas, une `SearchQuery` valide l'atteint. Toutes
//! rendent `InvalidRequest { status: None }`, sans appel réseau et avec un `warn!` (probable
//! bogue, 00 §5). `Params` détache les entrées de la requête pour que les gardes qui recopient le
//! domaine se testent.
//!
//! Le texte de la requête n'est jamais journalisé et aucune struct de ce module n'implémente
//! `Debug` : un message d'erreur ne cite que des noms de fonds et des nombres.
//!
//! `pageSize` 1 est envoyé en taille 2 (`wire_page_for`, L3-05) : PISTE ignore `pageNumber` à
//! `pageSize` 1. Les gardes de portée lisent la page demandée par l'appelant, pas la page envoyée.
//!
//! Le filtre `DATE_VERSION` de LODA et de CODE envoie le midi du jour à Paris en millisecondes
//! Unix (`paris_noon_ms`, 04 §6.3), calculé avec la base IANA embarquée par `chrono-tz`.

use chrono::{DateTime, Offset, TimeDelta, TimeZone};
use chrono_tz::Europe::Paris;
use lf_application::ports::CorpusError;
use lf_domain::{
    date::{DateRange, VersionDate},
    fond::{Fond, PaginationMode},
    search::{MatchMode, REACHABLE_RESULTS, SearchQuery},
};
use serde::Serialize;

/// Facette du filtre de version, réservée à `version_date` (04 §6.2).
const DATE_VERSION: &str = "DATE_VERSION";

/// Millisecondes par jour.
const DAY_MS: i64 = 86_400_000;

/// Millisecondes de midi.
const NOON_MS: i64 = 43_200_000;

/// Jours entre 0000-03-01 et 1970-01-01 dans l'algorithme de Hinnant.
const EPOCH_SHIFT_DAYS: i64 = 719_468;

/// Corps de `/search` (`SearchRequestDTO`).
#[derive(Serialize)]
struct SearchRequestDto<'a> {
    fond: &'static str,
    recherche: RechercheDto<'a>,
}

/// `RechercheSpecifiqueDTO` : un seul champ et un seul critère en v0.1.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RechercheDto<'a> {
    champs: Vec<ChampDto<'a>>,
    filtres: Vec<FiltreDto>,
    page_number: u32,
    page_size: u32,
    operateur: Operateur,
    sort: Sort,
    type_pagination: &'static str,
}

/// `ChampDTO`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChampDto<'a> {
    type_champ: TypeChamp,
    criteres: Vec<CritereDto<'a>>,
    operateur: Operateur,
}

/// `CritereDTO`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CritereDto<'a> {
    type_recherche: TypeRecherche,
    valeur: &'a str,
    operateur: Operateur,
}

/// `FiltreDTO` : `single_date` (millisecondes Unix) pour `DATE_VERSION`, `dates` pour une plage,
/// `valeurs` pour une facette à valeurs (`LEGAL_STATUS`, relevés c110/D08 : plusieurs valeurs
/// se combinent en OU).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FiltreDto {
    facette: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    single_date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dates: Option<DatePeriodDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    valeurs: Option<&'static [&'static str]>,
}

/// Période fermée, dates `AAAA-MM-JJ`.
#[derive(Serialize)]
struct DatePeriodDto {
    start: String,
    end: String,
}

/// Opérateur logique (seul `ET` en v0.1).
#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Operateur {
    Et,
}

/// Champ interrogé (seul `ALL` en v0.1).
#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum TypeChamp {
    All,
}

/// Tri (seule la pertinence en v0.1).
#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Sort {
    Pertinence,
}

/// Type de recherche d'un critère (04 §6.4).
#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum TypeRecherche {
    UnDesMots,
    TousLesMotsDansUnChamp,
    Exacte,
}

/// Les entrées du corps, détachées de `SearchQuery` : le domaine ne laisse pas construire une
/// requête invalide, et les gardes qui recopient ses règles ne se testeraient pas autrement.
struct Params<'a> {
    fond: Fond,
    text: &'a str,
    page: u32,
    page_size: u32,
    mode: MatchMode,
    date_range: Option<DateRange>,
    version_date: Option<VersionDate>,
    pagination: PaginationMode,
    in_force_only: bool,
}

/// Corps JSON de `/search` pour une requête validée.
///
/// `pagination` est fourni par l'appelant : le choix `DEFAUT` ou `ARTICLE` n'est pas de ce
/// module.
///
/// # Errors
///
/// `CorpusError::InvalidRequest { status: None, .. }` si une garde défensive refuse la requête ;
/// aucun appel réseau n'a alors eu lieu. Depuis une `SearchQuery` valide, seule la garde
/// `version_date` absente sur LODA ou CODE est atteignable (le domaine ne l'exige pas, 04 §6.2) ;
/// les autres recopient des règles du domaine.
pub(crate) fn search_body(
    query: &SearchQuery,
    pagination: PaginationMode,
) -> Result<Vec<u8>, CorpusError> {
    let params = Params {
        fond: query.fond(),
        text: query.text().as_str(),
        page: query.page().get(),
        page_size: query.page_size().get(),
        mode: query.mode(),
        date_range: query.date_range(),
        version_date: query.version_date(),
        pagination,
        in_force_only: query.in_force_only(),
    };
    request_dto(&params).and_then(|dto| encode(&dto))
}

/// Sérialise un corps en JSON ; l'erreur de `serde_json` n'est pas recopiée (elle pourrait
/// citer une valeur).
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CorpusError> {
    serde_json::to_vec(value).map_err(|_| defensive("request body serialization failed".to_owned()))
}

/// Le DTO de requête, après les gardes.
fn request_dto<'a>(params: &Params<'a>) -> Result<SearchRequestDto<'a>, CorpusError> {
    check_reach(params.page, params.page_size)?;
    let wire = wire_page_for(params.fond, params.pagination, params.page, params.page_size);
    Ok(SearchRequestDto {
        fond: params.fond.api_name(),
        recherche: RechercheDto {
            champs: vec![ChampDto {
                type_champ: TypeChamp::All,
                criteres: vec![CritereDto {
                    type_recherche: type_recherche(params.mode),
                    valeur: params.text,
                    operateur: Operateur::Et,
                }],
                operateur: Operateur::Et,
            }],
            filtres: filtres(params)?,
            page_number: wire.page_number,
            page_size: wire.page_size,
            operateur: Operateur::Et,
            sort: Sort::Pertinence,
            type_pagination: params.pagination.api_name(),
        },
    })
}

/// Page réellement demandée à PISTE pour une page de l'appelant (L3-05, `search-1`).
///
/// PISTE ignore `pageNumber` quand `pageSize` vaut 1 : les pages 1, 2 et 3 rendent toutes le rang 1
/// (relevés C9 de la sandbox, JURI, LODA, CNIL et CODE). Une page de taille 1 est donc servie par
/// une page de taille 2, dont on ne garde que l'élément `skip`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct WirePage {
    /// `pageNumber` envoyé.
    pub(crate) page_number: u32,
    /// `pageSize` envoyé.
    pub(crate) page_size: u32,
    /// Rang, dans la page reçue, du premier résultat à garder.
    pub(crate) skip: usize,
}

/// Page de taille 2 qui porte la page `page` de taille 1 ; sinon la page telle quelle.
///
/// `page` vaut au moins 1 (`NonZeroU32` côté domaine) : le rang gardé est `(page - 1) % 2`.
fn wire_page(page: u32, page_size: u32) -> WirePage {
    if page_size == 1 {
        WirePage {
            page_number: page.div_ceil(2),
            page_size: 2,
            skip: usize::from(page.is_multiple_of(2)),
        }
    } else {
        WirePage { page_number: page, page_size, skip: 0 }
    }
}

/// Page à envoyer pour `fond` en mode `pagination`.
///
/// `CODE` en mode `ARTICLE` est exclu : `pageSize` y compte des articles et non des hits, un hit en
/// porte plusieurs, et couper au niveau des articles n'est pas planifié. Le mode `DEFAUT` de
/// `CODE` a le défaut (C9-code-taille1-page3 rend le hit de la page 1) et est corrigé.
pub(crate) fn wire_page_for(
    fond: Fond,
    pagination: PaginationMode,
    page: u32,
    page_size: u32,
) -> WirePage {
    if fond == Fond::Code && pagination == PaginationMode::Article {
        WirePage { page_number: page, page_size, skip: 0 }
    } else {
        wire_page(page, page_size)
    }
}

/// Garde de profondeur : l'API n'atteint que `REACHABLE_RESULTS` résultats (04 §6.7). Le
/// produit se fait en `u64` : `u32::MAX` au carré y tient.
fn check_reach(page: u32, page_size: u32) -> Result<(), CorpusError> {
    if u64::from(page).saturating_mul(u64::from(page_size)) > REACHABLE_RESULTS {
        return Err(defensive(format!(
            "page {page} x page_size {page_size} exceeds {REACHABLE_RESULTS} reachable results"
        )));
    }
    Ok(())
}

/// Filtres du corps, toujours dans cet ordre : version, état puis période.
fn filtres(params: &Params<'_>) -> Result<Vec<FiltreDto>, CorpusError> {
    let mut filtres = Vec::new();
    filtres.extend(version_filter(params.fond, params.version_date)?);
    if params.in_force_only {
        filtres.push(in_force_filter(params.fond)?);
    }
    if let Some(range) = params.date_range {
        filtres.push(range_filter(params.fond, range)?);
    }
    Ok(filtres)
}

/// Filtre `DATE_VERSION` : exigé pour LODA et CODE, refusé ailleurs (04 §6.2).
fn version_filter(
    fond: Fond,
    version_date: Option<VersionDate>,
) -> Result<Option<FiltreDto>, CorpusError> {
    match (fond.needs_version_date(), version_date) {
        (true, Some(date)) => Ok(Some(FiltreDto {
            facette: DATE_VERSION,
            single_date: Some(paris_noon_ms(date)),
            dates: None,
            valeurs: None,
        })),
        (true, None) => Err(defensive(format!("version_date missing for fond {}", fond.name()))),
        (false, Some(_)) => {
            Err(defensive(format!("version_date not applicable to fond {}", fond.name())))
        }
        (false, None) => Ok(None),
    }
}

/// États `LEGAL_STATUS` gardés par `in_force_only` (D-08). Les autres états de KALI (`ABROGE`,
/// `DENONCE`, `MODIFIE`, `PERIME`, `REMPLACE`) sont exclus.
const IN_FORCE_STATUSES: &[&str] = &["VIGUEUR", "VIGUEUR_ETEN", "VIGUEUR_NON_ETEN"];

/// Filtre `LEGAL_STATUS` de `in_force_only` : relevé et validé sur KALI seulement.
fn in_force_filter(fond: Fond) -> Result<FiltreDto, CorpusError> {
    if fond != Fond::Kali {
        return Err(defensive(format!("in_force_only not applicable to fond {}", fond.name())));
    }
    Ok(FiltreDto {
        facette: "LEGAL_STATUS",
        single_date: None,
        dates: None,
        valeurs: Some(IN_FORCE_STATUSES),
    })
}

/// Filtre de période sur la facette de date du fond (04 §6.5).
fn range_filter(fond: Fond, range: DateRange) -> Result<FiltreDto, CorpusError> {
    let facette = date_facet(fond)
        .ok_or_else(|| defensive(format!("date range not applicable to fond {}", fond.name())))?;
    Ok(FiltreDto {
        facette,
        single_date: None,
        valeurs: None,
        dates: Some(DatePeriodDto {
            start: range.start().to_string(),
            end: range.end().to_string(),
        }),
    })
}

/// Facette de date du fond ; `None` pour CODE, qui n'a pas de filtre de période.
fn date_facet(fond: Fond) -> Option<&'static str> {
    match fond {
        Fond::Loda | Fond::Kali | Fond::Acco => Some("DATE_SIGNATURE"),
        Fond::Juri | Fond::Constit | Fond::Cetat => Some("DATE_DECISION"),
        Fond::Cnil => Some("DATE_DELIB"),
        Fond::Code => None,
    }
}

/// `typeRecherche` d'un critère d'après le mode de correspondance (04 §6.4).
fn type_recherche(mode: MatchMode) -> TypeRecherche {
    match mode {
        MatchMode::Any => TypeRecherche::UnDesMots,
        MatchMode::All => TypeRecherche::TousLesMotsDansUnChamp,
        MatchMode::Exact => TypeRecherche::Exacte,
    }
}

/// Garde défensive franchie : probable bogue de l'appelant, journalisé sans le texte de la
/// requête, et erreur sans statut HTTP (02 §5.1).
fn defensive(detail: String) -> CorpusError {
    tracing::warn!(%detail, "search request refused before sending: probable bug");
    CorpusError::InvalidRequest { status: None, detail }
}

/// Midi du jour à Paris, en millisecondes Unix (04 §6.3) ; partagée avec `request::m6`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`VersionDate` est borné à 1500..=2221 : jours et millisecondes tiennent dans un `i64`"
)]
pub(super) fn paris_noon_ms(date: VersionDate) -> i64 {
    let civil = date.as_civil();
    let days =
        days_from_civil(i64::from(civil.year()), i64::from(civil.month()), i64::from(civil.day()));
    let utc_noon_ms = days * DAY_MS + NOON_MS;
    // Pas de panique possible : `VersionDate` est borné à 1500..=2221, loin des limites de chrono.
    let utc = DateTime::UNIX_EPOCH.naive_utc() + TimeDelta::milliseconds(utc_noon_ms);
    let offset = Paris.offset_from_utc_datetime(&utc).fix().local_minus_utc();
    utc_noon_ms - i64::from(offset) * 1_000
}

/// Nombre de jours depuis 1970-01-01 pour une date civile grégorienne (algorithme de Hinnant).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "algorithme de Hinnant sur des années bornées à 1500..=2221 : tous les termes tiennent dans un `i64`"
)]
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - EPOCH_SHIFT_DAYS
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::search::{PageSize, QueryText};
    use serde::{Serializer, ser::Error as _};
    use serde_json::{Value, json};

    use super::*;
    use crate::testing::capture_at;

    fn day(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn range(from: &str, to: &str) -> Option<DateRange> {
        DateRange::new(Some(day(from)), Some(day(to))).unwrap()
    }

    fn params(fond: Fond) -> Params<'static> {
        Params {
            fond,
            text: "république numérique",
            page: 1,
            page_size: 10,
            mode: MatchMode::Any,
            date_range: None,
            version_date: fond.needs_version_date().then(|| day("2026-10-15")),
            pagination: PaginationMode::Defaut,
            in_force_only: false,
        }
    }

    fn body_of(params: &Params<'_>) -> Value {
        let bytes = encode(&request_dto(params).unwrap()).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Le payload de référence de 04 §6.1, à `filtres` près.
    fn expected(fond: &str, filtres: &Value) -> Value {
        json!({
            "fond": fond,
            "recherche": {
                "champs": [{
                    "typeChamp": "ALL",
                    "criteres": [{
                        "typeRecherche": "UN_DES_MOTS",
                        "valeur": "république numérique",
                        "operateur": "ET"
                    }],
                    "operateur": "ET"
                }],
                "filtres": filtres,
                "pageNumber": 1,
                "pageSize": 10,
                "operateur": "ET",
                "sort": "PERTINENCE",
                "typePagination": "DEFAUT"
            }
        })
    }

    fn invalid(detail: &str) -> CorpusError {
        CorpusError::InvalidRequest { status: None, detail: detail.to_owned() }
    }

    #[test]
    fn search_request_matches_reference_json() {
        for fond in Fond::ALL {
            let filtres = if fond.needs_version_date() {
                json!([{"facette": "DATE_VERSION", "singleDate": 1_792_058_400_000_i64}])
            } else {
                json!([])
            };
            assert_eq!(body_of(&params(fond)), expected(fond.api_name(), &filtres), "{fond:?}");
        }
    }

    #[test]
    fn search_request_with_date_range_uses_the_fond_facet() {
        let cases = [
            (Fond::Loda, "DATE_SIGNATURE"),
            (Fond::Juri, "DATE_DECISION"),
            (Fond::Constit, "DATE_DECISION"),
            (Fond::Cetat, "DATE_DECISION"),
            (Fond::Cnil, "DATE_DELIB"),
            (Fond::Kali, "DATE_SIGNATURE"),
            (Fond::Acco, "DATE_SIGNATURE"),
        ];
        for (fond, facette) in cases {
            let with_range =
                Params { date_range: range("2016-01-01", "2016-12-31"), ..params(fond) };
            let mut filtres = Vec::new();
            if fond.needs_version_date() {
                filtres
                    .push(json!({"facette": "DATE_VERSION", "singleDate": 1_792_058_400_000_i64}));
            }
            filtres.push(json!({
                "facette": facette,
                "dates": {"start": "2016-01-01", "end": "2016-12-31"}
            }));
            assert_eq!(
                body_of(&with_range),
                expected(fond.api_name(), &Value::Array(filtres)),
                "{fond:?}"
            );
        }
    }

    #[test]
    fn search_body_reads_every_field_of_the_query() {
        let query = SearchQuery::new(
            Fond::Loda,
            QueryText::parse("  république   numérique ").unwrap(),
            NonZeroU32::new(2).unwrap(),
            PageSize::new(25).unwrap(),
            MatchMode::All,
            range("2016-01-01", "2016-12-31"),
            Some(day("2026-10-15")),
        )
        .unwrap();
        let bytes = search_body(&query, PaginationMode::Article).unwrap();
        let mut want = expected(
            "LODA_DATE",
            &json!([
                {"facette": "DATE_VERSION", "singleDate": 1_792_058_400_000_i64},
                {"facette": "DATE_SIGNATURE", "dates": {"start": "2016-01-01", "end": "2016-12-31"}}
            ]),
        );
        want["recherche"]["champs"][0]["criteres"][0]["typeRecherche"] =
            json!("TOUS_LES_MOTS_DANS_UN_CHAMP");
        want["recherche"]["pageNumber"] = json!(2);
        want["recherche"]["pageSize"] = json!(25);
        want["recherche"]["typePagination"] = json!("ARTICLE");
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), want);
    }

    #[test]
    fn query_text_is_sent_verbatim() {
        let text = "\"article 1\" * \\ {\"a\": 1} é😀";
        let body = body_of(&Params { text, ..params(Fond::Juri) });
        assert_eq!(body["recherche"]["champs"][0]["criteres"][0]["valeur"], json!(text));
    }

    #[test]
    fn type_recherche_follows_match() {
        let cases = [
            (MatchMode::Any, "UN_DES_MOTS"),
            (MatchMode::All, "TOUS_LES_MOTS_DANS_UN_CHAMP"),
            (MatchMode::Exact, "EXACTE"),
        ];
        for (mode, want) in cases {
            let body = body_of(&Params { mode, ..params(Fond::Juri) });
            assert_eq!(body["recherche"]["champs"][0]["criteres"][0]["typeRecherche"], json!(want));
        }
    }

    #[test]
    fn pagination_mode_is_sent_as_given() {
        for (pagination, want) in
            [(PaginationMode::Defaut, "DEFAUT"), (PaginationMode::Article, "ARTICLE")]
        {
            let body = body_of(&Params { pagination, ..params(Fond::Code) });
            assert_eq!(body["recherche"]["typePagination"], json!(want));
        }
    }

    #[test]
    fn wire_page_table() {
        let table = [
            ((1, 1), (1, 2, 0)),
            ((2, 1), (1, 2, 1)),
            ((3, 1), (2, 2, 0)),
            ((4, 1), (2, 2, 1)),
            ((9_999, 1), (5_000, 2, 0)),
            ((10_000, 1), (5_000, 2, 1)),
            ((u32::MAX, 1), (2_147_483_648, 2, 0)),
            ((1, 2), (1, 2, 0)),
            ((7, 2), (7, 2, 0)),
            ((5, 3), (5, 3, 0)),
            ((2, 100), (2, 100, 0)),
        ];
        for ((page, size), (page_number, page_size, skip)) in table {
            assert_eq!(
                wire_page(page, size),
                WirePage { page_number, page_size, skip },
                "page {page} size {size}"
            );
        }
    }

    /// `(pageNumber, pageSize)` du corps pour la page `page` de taille `page_size`.
    fn wire_of(params: &Params<'_>) -> (Value, Value) {
        let body = body_of(params);
        (body["recherche"]["pageNumber"].clone(), body["recherche"]["pageSize"].clone())
    }

    /// L3-05 (search-1) : PISTE ignore `pageNumber` quand `pageSize` vaut 1 (relevés C9, fonds
    /// JURI, LODA, CNIL, CODE) ; la page `p` de taille 1 est servie par la page `(p+1)/2` de
    /// taille 2.
    #[test]
    fn page_size_1_is_sent_as_pages_of_two() {
        let wanted = [(1, 1), (2, 1), (3, 2), (4, 2), (9_999, 5_000), (10_000, 5_000)];
        for fond in [Fond::Juri, Fond::Loda, Fond::Cnil, Fond::Constit, Fond::Cetat, Fond::Kali] {
            for (page, number) in wanted {
                let sent = wire_of(&Params { page, page_size: 1, ..params(fond) });
                assert_eq!(sent, (json!(number), json!(2)), "{fond:?} page {page}");
            }
        }
    }

    /// Jumeau de `page_size_1_is_sent_as_pages_of_two` : les tailles 2, 3 et 10 sont inchangées.
    #[test]
    fn other_page_sizes_are_sent_unchanged() {
        for page_size in [2, 3, 10, 100] {
            let sent = wire_of(&Params { page: 3, page_size, ..params(Fond::Juri) });
            assert_eq!(sent, (json!(3), json!(page_size)), "size {page_size}");
        }
    }

    /// CODE en mode `DEFAUT` a le même défaut (C9-code-taille1-page3 rend le hit de la page 1) ;
    /// en mode `ARTICLE`, `pageSize` compte des articles, un hit en porte plusieurs et la
    /// troncature au niveau des articles n'est pas planifiée : la requête reste inchangée.
    #[test]
    fn code_page_size_1_is_doubled_in_defaut_mode_only() {
        let code = Params { page: 3, page_size: 1, ..params(Fond::Code) };
        assert_eq!(wire_of(&code), (json!(2), json!(2)));
        let by_article = Params { pagination: PaginationMode::Article, ..code };
        assert_eq!(wire_of(&by_article), (json!(3), json!(1)));
    }

    #[test]
    fn the_reach_guard_still_reads_the_requested_page() {
        let at_the_edge = Params { page: 10_000, page_size: 1, ..params(Fond::Juri) };
        assert!(request_dto(&at_the_edge).is_ok());
        let beyond = Params { page: 10_001, page_size: 1, ..params(Fond::Juri) };
        assert_eq!(
            request_dto(&beyond).err(),
            Some(invalid("page 10001 x page_size 1 exceeds 10000 reachable results"))
        );
    }

    #[test]
    fn filtres_order_is_deterministic() {
        let with_range =
            Params { date_range: range("2016-01-01", "2016-12-31"), ..params(Fond::Loda) };
        let first = encode(&request_dto(&with_range).unwrap()).unwrap();
        let second = encode(&request_dto(&with_range).unwrap()).unwrap();
        assert_eq!(first, second);
        let want = r#"{"fond":"LODA_DATE","recherche":{"champs":[{"typeChamp":"ALL","criteres":[{"typeRecherche":"UN_DES_MOTS","valeur":"république numérique","operateur":"ET"}],"operateur":"ET"}],"filtres":[{"facette":"DATE_VERSION","singleDate":1792058400000},{"facette":"DATE_SIGNATURE","dates":{"start":"2016-01-01","end":"2016-12-31"}}],"pageNumber":1,"pageSize":10,"operateur":"ET","sort":"PERTINENCE","typePagination":"DEFAUT"}}"#;
        assert_eq!(String::from_utf8(first).unwrap(), want);
    }

    /// Les trois états « en vigueur » de la facette `LEGAL_STATUS` (relevés c110/D08, sandbox).
    const IN_FORCE: [&str; 3] = ["VIGUEUR", "VIGUEUR_ETEN", "VIGUEUR_NON_ETEN"];

    /// D-08 : la requête de la sonde `c110/D08-kali-cc-vigueur-3`, au caractère près.
    #[test]
    fn in_force_only_sends_the_legal_status_filter_on_kali() {
        let with_status = Params { in_force_only: true, ..params(Fond::Kali) };
        let want = r#"{"fond":"KALI","recherche":{"champs":[{"typeChamp":"ALL","criteres":[{"typeRecherche":"UN_DES_MOTS","valeur":"république numérique","operateur":"ET"}],"operateur":"ET"}],"filtres":[{"facette":"LEGAL_STATUS","valeurs":["VIGUEUR","VIGUEUR_ETEN","VIGUEUR_NON_ETEN"]}],"pageNumber":1,"pageSize":10,"operateur":"ET","sort":"PERTINENCE","typePagination":"DEFAUT"}}"#;
        assert_eq!(
            String::from_utf8(encode(&request_dto(&with_status).unwrap()).unwrap()).unwrap(),
            want
        );
        assert_eq!(
            body_of(&with_status),
            expected("KALI", &json!([{"facette": "LEGAL_STATUS", "valeurs": IN_FORCE}]))
        );
    }

    /// D-08, relevé `c110/D08-kali-cc-vigueur-3-date-signature-2020` : état puis période.
    #[test]
    fn the_status_filter_comes_before_the_date_range() {
        let both = Params {
            in_force_only: true,
            date_range: range("2020-01-01", "2020-12-31"),
            ..params(Fond::Kali)
        };
        assert_eq!(
            body_of(&both),
            expected(
                "KALI",
                &json!([
                    {"facette": "LEGAL_STATUS", "valeurs": IN_FORCE},
                    {"facette": "DATE_SIGNATURE", "dates": {"start": "2020-01-01", "end": "2020-12-31"}}
                ])
            )
        );
    }

    /// D-08 : `false` (ou absent) ne change pas un octet de la requête de la 1.0.1.
    #[test]
    fn without_in_force_only_the_kali_body_is_the_one_of_1_0_1() {
        let plain = r#"{"fond":"KALI","recherche":{"champs":[{"typeChamp":"ALL","criteres":[{"typeRecherche":"UN_DES_MOTS","valeur":"république numérique","operateur":"ET"}],"operateur":"ET"}],"filtres":[],"pageNumber":1,"pageSize":10,"operateur":"ET","sort":"PERTINENCE","typePagination":"DEFAUT"}}"#;
        let off = encode(&request_dto(&params(Fond::Kali)).unwrap()).unwrap();
        assert_eq!(String::from_utf8(off).unwrap(), plain);
        let dated = Params { date_range: range("2016-01-01", "2016-12-31"), ..params(Fond::Kali) };
        let want = r#""filtres":[{"facette":"DATE_SIGNATURE","dates":{"start":"2016-01-01","end":"2016-12-31"}}]"#;
        assert!(
            String::from_utf8(encode(&request_dto(&dated).unwrap()).unwrap())
                .unwrap()
                .contains(want)
        );
    }

    #[test]
    fn in_force_only_on_another_fond_is_defensive_invalid_request_without_network() {
        for fond in Fond::ALL.into_iter().filter(|fond| *fond != Fond::Kali) {
            let refused = Params { in_force_only: true, ..params(fond) };
            assert_eq!(
                request_dto(&refused).err(),
                Some(invalid(&format!("in_force_only not applicable to fond {}", fond.name()))),
                "{fond:?}"
            );
        }
    }

    #[test]
    fn search_body_reads_in_force_only_from_the_query() {
        let build = |on: bool| {
            let query = SearchQuery::new(
                Fond::Kali,
                QueryText::parse("convention collective").unwrap(),
                NonZeroU32::MIN,
                PageSize::DEFAULT,
                MatchMode::Any,
                None,
                None,
            )
            .and_then(|query| query.with_in_force_only(on))
            .unwrap();
            serde_json::from_slice::<Value>(&search_body(&query, PaginationMode::Defaut).unwrap())
                .unwrap()["recherche"]["filtres"]
                .clone()
        };
        assert_eq!(build(true), json!([{"facette": "LEGAL_STATUS", "valeurs": IN_FORCE}]));
        assert_eq!(build(false), json!([]));
    }

    #[test]
    fn date_facet_table_matches_doc() {
        let table = [
            (Fond::Loda, Some("DATE_SIGNATURE")),
            (Fond::Code, None),
            (Fond::Juri, Some("DATE_DECISION")),
            (Fond::Constit, Some("DATE_DECISION")),
            (Fond::Cetat, Some("DATE_DECISION")),
            (Fond::Cnil, Some("DATE_DELIB")),
            (Fond::Kali, Some("DATE_SIGNATURE")),
            (Fond::Acco, Some("DATE_SIGNATURE")),
        ];
        assert_eq!(table.len(), Fond::ALL.len());
        for (fond, want) in table {
            assert_eq!(date_facet(fond), want, "{fond:?}");
        }
        for fond in Fond::ALL {
            assert_eq!(date_facet(fond).is_some(), fond.has_date_filter(), "{fond:?}");
        }
    }

    #[test]
    fn code_date_range_is_defensive_invalid_request_without_network() {
        let refused =
            Params { date_range: range("2016-01-01", "2016-12-31"), ..params(Fond::Code) };
        assert_eq!(
            request_dto(&refused).err(),
            Some(invalid("date range not applicable to fond CODE"))
        );
    }

    #[test]
    fn a_refused_request_is_logged_as_a_probable_bug_without_the_query_text() {
        let (capture, _guard) = capture_at(tracing::Level::WARN);
        let refused = Params {
            text: "texte-secret-42",
            date_range: range("2016-01-01", "2016-12-31"),
            ..params(Fond::Code)
        };
        let result = request_dto(&refused);
        let log = capture.text();
        assert!(result.is_err());
        assert!(log.contains("WARN"), "{log}");
        assert!(log.contains("probable bug"), "{log}");
        assert!(log.contains("date range not applicable to fond CODE"), "{log}");
        assert!(!log.contains("texte-secret-42"), "{log}");
    }

    #[test]
    fn version_date_is_required_for_loda_and_code_only() {
        for fond in [Fond::Loda, Fond::Code] {
            let missing = Params { version_date: None, ..params(fond) };
            assert_eq!(
                request_dto(&missing).err(),
                Some(invalid(&format!("version_date missing for fond {}", fond.name())))
            );
        }
        for fond in Fond::ALL.into_iter().filter(|fond| !fond.needs_version_date()) {
            let extra = Params { version_date: Some(day("2026-10-15")), ..params(fond) };
            assert_eq!(
                request_dto(&extra).err(),
                Some(invalid(&format!("version_date not applicable to fond {}", fond.name())))
            );
        }
    }

    /// Contrat avec l'application : le domaine n'exige pas `version_date` sur LODA et CODE
    /// (04 §6.2), `search_body` est donc le dernier rempart avant le réseau.
    #[test]
    fn a_valid_loda_or_code_query_without_version_date_is_refused_without_network() {
        for fond in [Fond::Loda, Fond::Code] {
            let (capture, _guard) = capture_at(tracing::Level::WARN);
            let query = SearchQuery::new(
                fond,
                QueryText::parse("texte-secret-42").unwrap(),
                NonZeroU32::new(1).unwrap(),
                PageSize::DEFAULT,
                MatchMode::Any,
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                search_body(&query, PaginationMode::Defaut),
                Err(invalid(&format!("version_date missing for fond {}", fond.name()))),
                "{fond:?}"
            );
            let log = capture.text();
            assert!(log.contains("WARN"), "{log}");
            assert!(log.contains("probable bug"), "{log}");
            assert!(log.contains("version_date missing"), "{log}");
            assert!(!log.contains("texte-secret-42"), "{log}");
        }
    }

    #[test]
    fn deep_pagination_is_defensive_invalid_request_without_network() {
        let reach = |page: u32, page_size: u32| {
            request_dto(&Params { page, page_size, ..params(Fond::Juri) }).err()
        };
        assert_eq!(reach(100, 100), None);
        assert_eq!(reach(10_000, 1), None);
        assert_eq!(reach(1, 100), None);
        assert_eq!(
            reach(101, 100),
            Some(invalid("page 101 x page_size 100 exceeds 10000 reachable results"))
        );
        assert_eq!(
            reach(10_001, 1),
            Some(invalid("page 10001 x page_size 1 exceeds 10000 reachable results"))
        );
        assert_eq!(
            reach(u32::MAX, 1),
            Some(invalid("page 4294967295 x page_size 1 exceeds 10000 reachable results"))
        );
    }

    #[test]
    fn encode_failure_is_reported_without_serde_detail() {
        struct Failing;
        impl Serialize for Failing {
            fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(S::Error::custom("secret text 12345"))
            }
        }
        assert_eq!(encode(&Failing), Err(invalid("request body serialization failed")));
    }

    #[test]
    fn paris_noon_vectors() {
        // Le dernier vecteur est midi à Paris en temps moyen local (+0:09:21), pas minuit UTC
        // (-5_231_606_400_000, vecteur de `CivilDate::from_unix_millis`).
        let vectors = [
            ("2026-10-15", 1_792_058_400_000),
            ("2026-01-15", 1_768_474_800_000),
            ("2026-03-29", 1_774_778_400_000),
            ("2026-10-25", 1_792_926_000_000),
            ("1804-03-21", -5_231_563_761_000),
        ];
        for (date, want) in vectors {
            assert_eq!(paris_noon_ms(day(date)), want, "{date}");
        }
    }

    #[test]
    fn paris_noon_covers_calendar_edges_and_historic_offsets() {
        let vectors = [
            ("1970-01-01", 39_600_000),
            ("2000-02-29", 951_822_000_000),
            ("2024-02-29", 1_709_204_400_000),
            ("1976-03-28", 196_855_200_000),
            ("1944-08-25", -800_028_000_000),
            ("1911-03-11", -1_855_915_200_000),
            ("1900-03-01", -2_203_848_561_000),
            ("1500-01-01", -14_831_726_961_000),
            ("2221-12-31", 7_952_295_600_000),
        ];
        for (date, want) in vectors {
            assert_eq!(paris_noon_ms(day(date)), want, "{date}");
        }
    }

    #[test]
    fn paris_noon_keeps_summer_time_until_2099() {
        let vectors = [
            ("2037-07-01", 2_130_055_200_000),
            ("2050-07-01", 2_540_282_400_000),
            ("2099-07-01", 4_086_583_200_000),
        ];
        for (date, want) in vectors {
            assert_eq!(paris_noon_ms(day(date)), want, "{date}");
        }
    }

    #[test]
    fn paris_noon_uses_standard_time_from_2100() {
        // Limite de `chrono-tz` 0.10.4 : les règles de Paris ne sont tabulées que jusqu'en 2099 ;
        // ensuite l'heure d'hiver vaut toute l'année (11:00Z au lieu de 10:00Z en été, alors que
        // `zoneinfo` garde 10:00Z). L'écart d'une heure est très en deçà de la marge de 10 h du
        // §6.3 : ce test épingle le comportement, il ne le corrige pas.
        assert_eq!(paris_noon_ms(day("2100-07-01")), 4_118_122_800_000);
        assert_eq!(paris_noon_ms(day("2221-07-01")), 7_936_484_400_000);
    }

    #[test]
    fn iana_database_version_is_pinned() {
        // Une montée de `chrono-tz` casse ce test : relire alors les vecteurs ci-dessus.
        assert_eq!(chrono_tz::IANA_TZDB_VERSION, "2025b");
    }

    #[test]
    fn enum_serialization_screaming_snake_case() {
        assert_eq!(serde_json::to_value(Operateur::Et).unwrap(), json!("ET"));
        assert_eq!(serde_json::to_value(TypeChamp::All).unwrap(), json!("ALL"));
        assert_eq!(serde_json::to_value(Sort::Pertinence).unwrap(), json!("PERTINENCE"));
        assert_eq!(serde_json::to_value(TypeRecherche::UnDesMots).unwrap(), json!("UN_DES_MOTS"));
        assert_eq!(
            serde_json::to_value(TypeRecherche::TousLesMotsDansUnChamp).unwrap(),
            json!("TOUS_LES_MOTS_DANS_UN_CHAMP")
        );
        assert_eq!(serde_json::to_value(TypeRecherche::Exacte).unwrap(), json!("EXACTE"));
    }
}
