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

/// `FiltreDTO` : `single_date` (millisecondes Unix) pour `DATE_VERSION`, `dates` pour une plage.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FiltreDto {
    facette: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    single_date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dates: Option<DatePeriodDto>,
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
            page_number: params.page,
            page_size: params.page_size,
            operateur: Operateur::Et,
            sort: Sort::Pertinence,
            type_pagination: params.pagination.api_name(),
        },
    })
}

/// Garde de profondeur : l'API n'atteint que `REACHABLE_RESULTS` résultats (04 §6.7). Le
/// produit se fait en `u64` : `u32::MAX` au carré y tient.
fn check_reach(page: u32, page_size: u32) -> Result<(), CorpusError> {
    if u64::from(page) * u64::from(page_size) > REACHABLE_RESULTS {
        return Err(defensive(format!(
            "page {page} x page_size {page_size} exceeds {REACHABLE_RESULTS} reachable results"
        )));
    }
    Ok(())
}

/// Filtres du corps, toujours dans cet ordre : version puis période.
fn filtres(params: &Params<'_>) -> Result<Vec<FiltreDto>, CorpusError> {
    let mut filtres = Vec::new();
    filtres.extend(version_filter(params.fond, params.version_date)?);
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
        })),
        (true, None) => Err(defensive(format!("version_date missing for fond {}", fond.name()))),
        (false, Some(_)) => {
            Err(defensive(format!("version_date not applicable to fond {}", fond.name())))
        }
        (false, None) => Ok(None),
    }
}

/// Filtre de période sur la facette de date du fond (04 §6.5).
fn range_filter(fond: Fond, range: DateRange) -> Result<FiltreDto, CorpusError> {
    let facette = date_facet(fond)
        .ok_or_else(|| defensive(format!("date range not applicable to fond {}", fond.name())))?;
    Ok(FiltreDto {
        facette,
        single_date: None,
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
    fn filtres_order_is_deterministic() {
        let with_range =
            Params { date_range: range("2016-01-01", "2016-12-31"), ..params(Fond::Loda) };
        let first = encode(&request_dto(&with_range).unwrap()).unwrap();
        let second = encode(&request_dto(&with_range).unwrap()).unwrap();
        assert_eq!(first, second);
        let want = r#"{"fond":"LODA_DATE","recherche":{"champs":[{"typeChamp":"ALL","criteres":[{"typeRecherche":"UN_DES_MOTS","valeur":"république numérique","operateur":"ET"}],"operateur":"ET"}],"filtres":[{"facette":"DATE_VERSION","singleDate":1792058400000},{"facette":"DATE_SIGNATURE","dates":{"start":"2016-01-01","end":"2016-12-31"}}],"pageNumber":1,"pageSize":10,"operateur":"ET","sort":"PERTINENCE","typePagination":"DEFAUT"}}"#;
        assert_eq!(String::from_utf8(first).unwrap(), want);
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
