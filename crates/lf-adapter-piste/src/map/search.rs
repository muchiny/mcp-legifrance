//! Page de résultats de `/search` vers `SearchPage` (04 §9.1 c, d, i ; §9.2 ; 03 §4.1).
//!
//! Chaque résultat est lu par son premier titre. Un résultat dont l'identifiant est absent,
//! invalide ou d'un autre fond que celui interrogé est écarté et compté (`dropped_hits`) : il
//! n'est lu ni plus loin ni propagé en erreur. Le suffixe de version `_JJ-MM-AAAA` d'un
//! `LEGITEXT` donne `version_start` ; celui d'un autre identifiant est retiré et compté. L'origine
//! (`origin`) est contrôlée sans jamais écarter un résultat. Les textes courts passent par
//! `clean_field`, qui retire balises `<mark>` et caractères invisibles sans les compter (D17).
//!
//! La date, l'extrait et les articles d'un résultat ne sont lus qu'après son rejet éventuel.
//! La date est la première non nulle et hors sentinelle de `date`, `datePublication` puis
//! `dateSignature` (E-26) ; une date illisible est la seule erreur d'un résultat (`Decode`, chemin
//! du champ). L'extrait est le texte du résultat, à défaut le premier passage du premier extrait
//! utilisable. Les articles (`CODE` et `KALI`) viennent des extraits des sections.
//!
//! Aucune valeur du corps n'est journalisée ni propagée : toute erreur du domaine devient `None`,
//! sauf une date illisible, dont seul le chemin du champ est nommé.

use lf_application::ports::CorpusError;
use lf_domain::contract::Idcc;
use lf_domain::date::CivilDate;
use lf_domain::entity::{CodeArticleRef, SearchHit, SearchPage, Snippet};
use lf_domain::fond::{Fond, PaginationMode};
use lf_domain::id::{ArticleId, DocumentId};
use lf_domain::search::SearchQuery;
use lf_domain::status::status_of;
use lf_domain::text::clean_field;

use super::dates::first_civil;
use super::rules::{check_origin, parse_counted, suffix_start};
use super::{Diagnostics, Field, endpoint};
use crate::dto::search::{SearchExtractDto, SearchResponseDto, SearchResultDto, SearchSectionDto};
use crate::memo::received_mode;

/// Page de résultats de `query`, sans le mode de pagination reçu.
///
/// Équivaut à [`search_page_with_mode`] dont on ignore le mode.
///
/// # Errors
///
/// Voir [`search_page_with_mode`].
pub(crate) fn search_page(
    dto: SearchResponseDto,
    query: &SearchQuery,
    diags: &mut Diagnostics,
) -> Result<SearchPage, CorpusError> {
    search_page_with_mode(dto, query, diags).map(|(page, _)| page)
}

/// Page de résultats de `query` et `typePagination` reconnu de la réponse (`None` s'il est
/// absent ou inconnu).
///
/// # Errors
///
/// [`CorpusError::Decode`] (`/search: results: more results than page size`) si la réponse porte
/// plus de résultats que la taille de page demandée ; les compteurs ne sont alors pas touchés.
/// Aussi `Decode` (`/search: results[].<champ>: unreadable date`) si la date d'un résultat
/// retenu est illisible.
pub(crate) fn search_page_with_mode(
    dto: SearchResponseDto,
    query: &SearchQuery,
    diags: &mut Diagnostics,
) -> Result<(SearchPage, Option<PaginationMode>), CorpusError> {
    dto.ensure_fits(endpoint::SEARCH, query.page_size())?;
    let fond = query.fond();
    let mut hits = Vec::with_capacity(dto.results.len());
    let mut dropped: u32 = 0;
    for result in dto.results {
        if let Some(hit) = map_hit(&result, fond, diags)? {
            hits.push(hit);
        } else {
            diags.note_dropped_hit();
            dropped = dropped.saturating_add(1);
        }
    }
    let received = received_mode(dto.type_pagination.as_deref());
    let page = SearchPage::new(
        fond,
        query.text().as_str(),
        dto.total_result_number,
        dto.total_article_result_number,
        hits,
        dropped,
        received.unwrap_or_default(),
    );
    Ok((page, received))
}

/// Résultat de `result` pour `fond`, ou `None` s'il est écarté.
///
/// L'identifiant est lu d'abord : un résultat écarté n'est jamais lu plus loin, pas même sa date.
/// Les textes courts n'ont pas d'erreur possible (`clean_field`, `status_of` et `Idcc::parse`
/// rendent `None`) ; seule une date illisible est une erreur.
///
/// # Errors
///
/// [`CorpusError::Decode`] si une date lue du résultat retenu est illisible.
fn map_hit(
    result: &SearchResultDto,
    fond: Fond,
    diags: &mut Diagnostics,
) -> Result<Option<SearchHit>, CorpusError> {
    let title = result.titles.first();
    let Some(mut hit) = start_hit(fond, title.and_then(|title| title.id.as_deref()), diags) else {
        return Ok(None);
    };
    check_origin(result.origin.as_deref(), hit.id.as_str(), diags);
    hit.cid = hit_cid(title.and_then(|title| title.cid.as_deref()));
    hit.title = clean_field(title.and_then(|title| title.title.as_deref()));
    hit.nature = clean_field(result.nature.as_deref());
    hit.etat = status_of(result.etat.as_deref(), None);
    hit.idcc = result.idcc.as_deref().and_then(Idcc::parse);
    hit.raison_sociale = clean_field(result.raison_sociale.as_deref());
    hit.date = hit_date(result, diags)?;
    hit.snippet = hit_snippet(result);
    hit.articles = hit_articles(fond, &result.sections);
    Ok(Some(hit))
}

/// Date `date` du résultat.
const DATE: Field = Field { endpoint: endpoint::SEARCH, path: "results[].date" };
/// Date de publication du résultat.
const DATE_PUBLICATION: Field =
    Field { endpoint: endpoint::SEARCH, path: "results[].datePublication" };
/// Date de signature du résultat.
const DATE_SIGNATURE: Field = Field { endpoint: endpoint::SEARCH, path: "results[].dateSignature" };

/// Première date civile du résultat : `date`, `datePublication`, puis `dateSignature`.
///
/// Un champ absent ou à sentinelle est sauté ; les champs suivant la première date ne sont pas lus.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`unreadable date`) du premier champ lu qui est illisible.
fn hit_date(
    result: &SearchResultDto,
    diags: &mut Diagnostics,
) -> Result<Option<CivilDate>, CorpusError> {
    first_civil(
        &[
            (DATE, result.date.as_ref()),
            (DATE_PUBLICATION, result.date_publication.as_ref()),
            (DATE_SIGNATURE, result.date_signature.as_ref()),
        ],
        diags,
    )
}

/// Extrait du résultat : son texte, à défaut le premier passage d'un extrait de section.
///
/// Un texte vide après nettoyage laisse la main aux extraits.
fn hit_snippet(result: &SearchResultDto) -> Option<Snippet> {
    result.text.as_deref().and_then(Snippet::new).or_else(|| extract_snippet(&result.sections))
}

/// Premier extrait utilisable des `sections` : le premier passage (`values`) de chaque extrait
/// est essayé dans l'ordre, les suivants ne sont jamais lus.
fn extract_snippet(sections: &[SearchSectionDto]) -> Option<Snippet> {
    sections
        .iter()
        .flat_map(|section| &section.extracts)
        .filter_map(|extract| extract.values.first())
        .find_map(|value| Snippet::new(value))
}

/// Articles trouvés dans les `sections`, dans l'ordre : seuls ceux du bon couple fond et famille
/// d'identifiant sont gardés.
fn hit_articles(fond: Fond, sections: &[SearchSectionDto]) -> Vec<CodeArticleRef> {
    sections
        .iter()
        .flat_map(|section| &section.extracts)
        .filter_map(|extract| article_ref(fond, extract))
        .collect()
}

/// Référence d'article d'un extrait, ou `None` si l'extrait n'en désigne pas un pour `fond`.
///
/// Un `CODE` ne garde que les `LEGIARTI` et leur numéro nettoyé ; un `KALI` que les `KALIARTI`,
/// sans numéro (texte libre non fiable). Tout autre fond n'a pas d'articles. L'extrait de
/// l'article est son premier passage, à défaut vide.
fn article_ref(fond: Fond, extract: &SearchExtractDto) -> Option<CodeArticleRef> {
    let article_id = ArticleId::parse(extract.id.as_deref()?).ok()?;
    let num = match (fond, &article_id) {
        (Fond::Code, ArticleId::Legi(_)) => clean_field(extract.num.as_deref()),
        (Fond::Kali, ArticleId::Kali(_)) => None,
        _ => return None,
    };
    let raw = extract.values.first().map_or("", String::as_str);
    Some(CodeArticleRef::new(article_id, num, raw))
}

/// Résultat vide de `fond` pour l'identifiant brut `raw_id`, ou `None` s'il est écarté.
///
/// L'identifiant est lu par `parse_counted` (casse, blancs et suffixe tolérés, suffixe retiré
/// compté) puis rapproché du fond par `SearchHit::new` ; la date du suffixe d'un `LEGITEXT` devient
/// `version_start`. Toute erreur du domaine devient `None` : sa valeur n'est ni gardée ni
/// journalisée.
fn start_hit(fond: Fond, raw_id: Option<&str>, diags: &mut Diagnostics) -> Option<SearchHit> {
    let raw = raw_id?;
    let id = parse_counted::<DocumentId>(raw, diags).ok()?;
    SearchHit::new(fond, id, suffix_start(raw)).ok()
}

/// Identifiant chronologique d'un résultat, sans contrôle de préfixe.
///
/// Un `cid` illisible laisse le résultat sans `cid` : il ne l'écarte pas.
fn hit_cid(raw: Option<&str>) -> Option<DocumentId> {
    DocumentId::parse(raw?).ok()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::date::CivilDate;
    use lf_domain::entity::{CodeArticleRef, Snippet};
    use lf_domain::search::{MatchMode, PageSize, QueryText};
    use lf_domain::status::LegalStatus;
    use serde_json::{Value, json};

    use super::*;
    use crate::dto::de::RawDate;
    use crate::dto::decode::decode;
    use crate::dto::search::SearchSectionDto;

    /// Requête de `fond` sur « responsabilité civile », page 1, de taille `page_size`.
    fn query(fond: Fond, page_size: u32) -> SearchQuery {
        query_of(fond, "responsabilité civile", page_size)
    }

    /// Requête de `fond` sur `text`, page 1, de taille `page_size`.
    fn query_of(fond: Fond, text: &str, page_size: u32) -> SearchQuery {
        SearchQuery::new(
            fond,
            QueryText::parse(text).unwrap(),
            NonZeroU32::new(1).unwrap(),
            PageSize::new(page_size).unwrap(),
            MatchMode::Any,
            None,
            None,
        )
        .unwrap()
    }

    /// `json` lu comme le fait la production.
    fn dto(json: &str) -> SearchResponseDto {
        decode(json, "/search").unwrap()
    }

    /// Page de `json` pour `fond` (taille de page 100) et les compteurs relevés.
    fn page_of(fond: Fond, json: &str) -> (SearchPage, Diagnostics) {
        let mut diags = Diagnostics::default();
        let page = search_page(dto(json), &query(fond, 100), &mut diags).unwrap();
        (page, diags)
    }

    /// Page d'un seul résultat `result` en LODA.
    fn loda_hit(result: &Value) -> SearchHit {
        let json = json!({ "results": [result] }).to_string();
        let (mut page, _) = page_of(Fond::Loda, &json);
        assert_eq!(page.hits.len(), 1, "{json}");
        page.hits.remove(0)
    }

    /// Résultat LODA valide de titre `titles[0]` complété par `extra`.
    fn loda_result(extra: &Value) -> Value {
        let mut result = json!({ "titles": [{ "id": "LEGITEXT000006070721" }] });
        for (key, value) in extra.as_object().unwrap() {
            result[key] = value.clone();
        }
        result
    }

    fn day(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap()
    }

    #[test]
    fn results_beyond_the_page_size_are_a_decode_error() {
        let two = r#"{"results":[{},{}]}"#;
        let three = r#"{"results":[{},{},{}]}"#;
        let mut diags = Diagnostics::default();
        assert!(search_page(dto(two), &query(Fond::Juri, 2), &mut diags).is_ok());
        let mut diags = Diagnostics::default();
        assert_eq!(
            search_page(dto(three), &query(Fond::Juri, 2), &mut diags),
            Err(CorpusError::Decode {
                detail: "/search: results: more results than page size".into()
            })
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn invalid_hit_is_dropped_and_counted() {
        let json = json!({
            "totalResultNumber": 50,
            "results": [
                { "titles": [{ "id": "JURITEXT000051743650", "title": "Un" }] },
                { "titles": [{ "id": "CNILTEXT000053352643" }] },
                { "titles": [] },
                { "titles": [{ "id": "JURITEXT12" }] },
                { "titles": [{
                    "id": " juritext000048465611_01-01-2000 ",
                    "title": "Suffixe toléré"
                }] },
                { "titles": [{ "id": "JURITEXT000048465611_31-02-2000" }] },
                { "titles": [{ "title": "Sans id" }] },
            ]
        })
        .to_string();
        let (page, diags) = page_of(Fond::Juri, &json);
        let kept: Vec<_> = page
            .hits
            .iter()
            .map(|hit| (hit.id.as_str(), hit.title.as_deref(), hit.version_start))
            .collect();
        assert_eq!(
            kept,
            [
                ("JURITEXT000051743650", Some("Un"), None),
                ("JURITEXT000048465611", Some("Suffixe toléré"), None),
            ]
        );
        assert_eq!(page.dropped, 5);
        assert_eq!(page.total, 50);
        assert_eq!(
            diags,
            Diagnostics { dropped_hits: 5, upstream_suffix_stripped: 1, ..Diagnostics::default() }
        );
    }

    #[test]
    fn a_dropped_hit_is_not_read_further() {
        let json = r#"{"results":[{"titles":[{"id":"CNILTEXT000053352643"}],"date":"hier",
            "origin":"JURI","idcc":"12a"}]}"#;
        let (page, diags) = page_of(Fond::Juri, json);
        assert_eq!((page.hits.len(), page.dropped), (0, 1));
        assert_eq!(diags, Diagnostics { dropped_hits: 1, ..Diagnostics::default() });
    }

    #[test]
    fn dropped_hits_accumulate_in_the_diagnostics() {
        let json = r#"{"results":[{"titles":[{"id":"x"}]},{}]}"#;
        let mut diags = Diagnostics { dropped_hits: 5, ..Diagnostics::default() };
        let page = search_page(dto(json), &query(Fond::Juri, 10), &mut diags).unwrap();
        assert_eq!(page.dropped, 2);
        assert_eq!(diags.dropped_hits, 7);
    }

    #[test]
    fn degenerate_responses_map_without_panic() {
        for json in ["{}", r#"{"results":null}"#, r#"{"results":[]}"#] {
            let (page, diags) = page_of(Fond::Loda, json);
            assert_eq!(page.hits, [], "{json}");
            assert_eq!((page.total, page.total_articles, page.dropped), (0, None, 0), "{json}");
            assert_eq!(page.pagination, PaginationMode::Defaut, "{json}");
            assert_eq!(diags, Diagnostics::default(), "{json}");
        }
        for json in [
            r#"{"results":[{}]}"#,
            r#"{"results":[{"titles":[{}]}]}"#,
            r#"{"results":[{"titles":[{"id":""}]}]}"#,
            r#"{"results":[{"titles":[{"id":"   "}]}]}"#,
        ] {
            let (page, diags) = page_of(Fond::Loda, json);
            assert_eq!((page.hits.len(), page.dropped, diags.dropped_hits), (0, 1, 1), "{json}");
        }
    }

    #[test]
    fn start_hit_checks_the_family_of_the_fond() {
        for fond in Fond::ALL {
            let mut diags = Diagnostics::default();
            let own = format!("{}000012345678", fond.hit_prefix());
            let hit = start_hit(fond, Some(&own), &mut diags).unwrap();
            assert_eq!((hit.fond, hit.id.as_str()), (fond, own.as_str()));
            let other = Fond::ALL.iter().find(|f| f.hit_prefix() != fond.hit_prefix()).unwrap();
            let foreign = format!("{}000012345678", other.hit_prefix());
            assert_eq!(start_hit(fond, Some(&foreign), &mut diags), None, "{fond:?}");
            assert_eq!(diags, Diagnostics::default());
        }
        let mut diags = Diagnostics::default();
        assert_eq!(start_hit(Fond::Loda, Some("LEGIARTI000006419288"), &mut diags), None);
        assert_eq!(start_hit(Fond::Loda, None, &mut diags), None);
        for fond in [Fond::Code, Fond::Loda] {
            let hit = start_hit(fond, Some("LEGITEXT000033205014_08-09-2023"), &mut diags).unwrap();
            assert_eq!(hit.id.as_str(), "LEGITEXT000033205014");
            assert_eq!(hit.version_start, Some(day(2023, 9, 8)));
            assert_eq!(start_hit(fond, Some("LEGITEXT000033205014_31-02-2023"), &mut diags), None);
        }
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn hit_origin_is_checked_and_never_drops() {
        let result = |origin: &str| {
            json!({ "results": [{
                "titles": [{ "id": "JURITEXT000051743650" }],
                "origin": origin
            }] })
            .to_string()
        };
        let (page, diags) = page_of(Fond::Juri, &result("CETAT"));
        assert_eq!(page.hits.len(), 1);
        assert_eq!(diags, Diagnostics { origin_mismatch: 1, ..Diagnostics::default() });
        let (page, diags) = page_of(Fond::Juri, &result("JURI"));
        assert_eq!(page.hits.len(), 1);
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn hit_cid_is_parsed_without_a_prefix_check() {
        assert_eq!(
            hit_cid(Some("JORFTEXT000033202746")).map(|id| id.as_str().to_owned()),
            Some("JORFTEXT000033202746".to_owned())
        );
        assert_eq!(
            hit_cid(Some("  legitext000006070721 ")).map(|id| id.as_str().to_owned()),
            Some("LEGITEXT000006070721".to_owned())
        );
        for raw in [Some("xx"), Some(""), None] {
            assert_eq!(hit_cid(raw), None, "{raw:?}");
        }
        let hit = loda_hit(&json!({ "titles": [{ "id": "LEGITEXT000006070721", "cid": "xx" }] }));
        assert_eq!(hit.cid, None);
        let hit = loda_hit(&json!({
            "titles": [{ "id": "LEGITEXT000006070721", "cid": "JORFTEXT000033202746" }]
        }));
        assert_eq!(hit.cid, hit_cid(Some("JORFTEXT000033202746")));
    }

    #[test]
    fn hit_text_fields_are_cleaned_and_typed() {
        let hit = loda_hit(&json!({
            "titles": [{ "id": "LEGITEXT000006070721", "title": "LOI <mark>numérique</mark>  (1)" }],
            "nature": " <mark>LOI</mark> ",
            "etat": " vigueur_eten ",
            "raisonSociale": " ACME  <mark>SA</mark> ",
        }));
        assert_eq!(hit.title.as_deref(), Some("LOI numérique (1)"));
        assert_eq!(hit.nature.as_deref(), Some("LOI"));
        assert_eq!(hit.etat, Some(LegalStatus::VigueurEtendue));
        assert_eq!(hit.raison_sociale.as_deref(), Some("ACME SA"));

        let hit = loda_hit(&json!({
            "titles": [{ "id": "LEGITEXT000006070721", "title": "" }],
            "nature": "  ",
            "etat": "",
            "raisonSociale": "<mark> </mark>",
        }));
        assert_eq!((hit.title, hit.nature, hit.etat, hit.raison_sociale), (None, None, None, None));

        let hit = loda_hit(&loda_result(&json!({ "etat": "FOO_BAR" })));
        assert_eq!(hit.etat, Some(LegalStatus::Other("FOO_BAR".into())));
    }

    #[test]
    fn idcc_accepts_string_and_number_in_a_hit() {
        let cases = [
            (json!("275"), Some(275)),
            (json!(1486), Some(1486)),
            (json!("0000000000002121"), Some(2121)),
            (json!("10000"), None),
            (json!("12a"), None),
            (json!(0), None),
            (json!(2.5), None),
            (Value::Null, None),
        ];
        for (idcc, expected) in cases {
            let hit = loda_hit(&loda_result(&json!({ "idcc": idcc })));
            assert_eq!(hit.idcc.map(Idcc::get), expected, "{idcc}");
        }
        assert_eq!(loda_hit(&loda_result(&json!({}))).idcc, None);
    }

    #[test]
    fn invisible_characters_are_removed_from_hit_texts() {
        let (page, diags) = page_of(
            Fond::Loda,
            &json!({ "results": [{
                "titles": [{ "id": "LEGITEXT000006070721", "title": "A\u{202e}B\u{200b}C" }],
                "nature": "L\u{202e}OI",
                "raisonSociale": "X\u{ad}Y",
            }] })
            .to_string(),
        );
        let hit = &page.hits[0];
        assert_eq!(hit.title.as_deref(), Some("ABC"));
        assert_eq!(hit.nature.as_deref(), Some("LOI"));
        assert_eq!(hit.raison_sociale.as_deref(), Some("XY"));
        assert_eq!(diags.invisible_chars_removed, 0);
    }

    #[test]
    fn total_comes_from_the_response_or_the_kept_hits() {
        let one = r#"{"titles":[{"id":"JURITEXT000051743650"}]}"#;
        let json = |total: &str, results: &str| format!(r#"{{{total}"results":[{results}]}}"#);
        let cases = [
            (json(r#""totalResultNumber":7,"#, ""), 7),
            (json(r#""totalResultNumber":0,"#, ""), 0),
            (json("", one), 1),
            (json("", &format!("{one},{{}}")), 1),
        ];
        for (body, expected) in cases {
            assert_eq!(page_of(Fond::Juri, &body).0.total, expected, "{body}");
        }
    }

    #[test]
    fn total_articles_keeps_only_positive_counts() {
        let cases = [
            (r#"{"totalArticleResultNumber":3}"#, Some(3)),
            (r#"{"totalArticleResultNumber":"3"}"#, Some(3)),
            (r#"{"totalArticleResultNumber":0}"#, None),
            ("{}", None),
        ];
        for (body, expected) in cases {
            assert_eq!(page_of(Fond::Code, body).0.total_articles, expected, "{body}");
        }
    }

    #[test]
    fn search_page_with_mode_returns_the_received_mode() {
        let cases = [
            (
                r#"{"typePagination":"DEFAUT"}"#,
                PaginationMode::Defaut,
                Some(PaginationMode::Defaut),
            ),
            (
                r#"{"typePagination":"ARTICLE"}"#,
                PaginationMode::Article,
                Some(PaginationMode::Article),
            ),
            (
                r#"{"typePagination":" article "}"#,
                PaginationMode::Article,
                Some(PaginationMode::Article),
            ),
            (r#"{"typePagination":"WEIRD"}"#, PaginationMode::Defaut, None),
            ("{}", PaginationMode::Defaut, None),
            (r#"{"typePagination":null}"#, PaginationMode::Defaut, None),
            (r#"{"typePagination":7}"#, PaginationMode::Defaut, None),
        ];
        for (body, pagination, received) in cases {
            let mut diags = Diagnostics::default();
            let (page, mode) =
                search_page_with_mode(dto(body), &query(Fond::Code, 10), &mut diags).unwrap();
            assert_eq!((page.pagination, mode), (pagination, received), "{body}");
        }
    }

    #[test]
    fn search_page_is_search_page_with_mode_without_the_mode() {
        let body = r#"{"typePagination":"ARTICLE","totalResultNumber":9,
            "results":[{"titles":[{"id":"LEGITEXT000006070721"}]},{}]}"#;
        let mut with_mode_diags = Diagnostics::default();
        let (with_mode, _) =
            search_page_with_mode(dto(body), &query(Fond::Code, 10), &mut with_mode_diags).unwrap();
        let mut diags = Diagnostics::default();
        let page = search_page(dto(body), &query(Fond::Code, 10), &mut diags).unwrap();
        assert_eq!(page, with_mode);
        assert_eq!(diags, with_mode_diags);
        assert_eq!(diags.dropped_hits, 1);
    }

    #[test]
    fn the_page_carries_the_fond_and_the_query() {
        let mut diags = Diagnostics::default();
        let page = search_page(
            dto("{}"),
            &query_of(Fond::Juri, "  responsabilité   civile ", 10),
            &mut diags,
        )
        .unwrap();
        assert_eq!((page.fond, page.query.as_str()), (Fond::Juri, "responsabilité civile"));
    }

    /// Date lue de `result` par `hit_date`, avec ses compteurs.
    fn date_of(result: &SearchResultDto) -> Result<Option<CivilDate>, CorpusError> {
        hit_date(result, &mut Diagnostics::default())
    }

    /// Résultat dont les trois dates sont `date`, `publication` et `signature`.
    fn dated(
        date: Option<RawDate>,
        publication: Option<RawDate>,
        signature: Option<RawDate>,
    ) -> SearchResultDto {
        SearchResultDto {
            date,
            date_publication: publication,
            date_signature: signature,
            ..Default::default()
        }
    }

    /// Date en texte, à minuit UTC : une macro et non une fonction, que
    /// `clippy::unnecessary_wraps` refuserait.
    macro_rules! at {
        ($day:expr) => {
            Some(RawDate::Text(format!("{}T00:00:00.000+0000", $day)))
        };
    }

    #[test]
    fn hit_date_takes_the_first_date_out_of_the_sentinel_zone() {
        let cases = [
            (dated(at!("2016-10-09"), at!("2016-10-08"), None), Some(day(2016, 10, 9))),
            (dated(None, at!("2026-01-14"), None), Some(day(2026, 1, 14))),
            (dated(None, None, at!("2025-12-19")), Some(day(2025, 12, 19))),
            (dated(at!("2999-01-01"), at!("2016-10-08"), None), Some(day(2016, 10, 8))),
            (dated(None, at!("2999-01-01"), None), None),
            (
                dated(
                    Some(RawDate::Millis(32_503_680_000_000)),
                    None,
                    Some(RawDate::Millis(1_475_884_800_000)),
                ),
                Some(day(2016, 10, 8)),
            ),
            (dated(Some(RawDate::Millis(-5_231_606_400_000)), None, None), Some(day(1804, 3, 21))),
            (dated(None, None, None), None),
        ];
        for (result, expected) in cases {
            assert_eq!(date_of(&result), Ok(expected));
        }
    }

    #[test]
    fn an_unreadable_date_is_a_decode_error_with_its_path() {
        for text in ["hier", "2024-13-45"] {
            let bad = || Some(RawDate::Text(text.to_owned()));
            let cases = [
                (dated(bad(), None, None), "date"),
                (dated(None, bad(), None), "datePublication"),
                (dated(None, None, bad()), "dateSignature"),
            ];
            for (result, field) in cases {
                assert_eq!(
                    date_of(&result),
                    Err(CorpusError::Decode {
                        detail: format!("/search: results[].{field}: unreadable date")
                    }),
                    "{text} {field}"
                );
            }
        }
        let result = dated(Some(RawDate::Text("hier".into())), at!("2016-10-08"), None);
        assert!(date_of(&result).is_err());
    }

    #[test]
    fn no_date_text_appears_in_the_decode_error() {
        let result = dated(Some(RawDate::Text("SECRET-DATE-VALUE".into())), None, None);
        let error = date_of(&result).unwrap_err();
        let shown = format!("{error:?} {error}");
        assert!(!shown.contains("SECRET"), "{shown}");
    }

    #[test]
    fn later_dates_are_not_read_once_one_is_found() {
        let result = dated(at!("2016-10-09"), None, Some(RawDate::Text("hier".into())));
        assert_eq!(date_of(&result), Ok(Some(day(2016, 10, 9))));
    }

    #[test]
    fn an_unreadable_date_fails_the_page_but_only_for_a_kept_hit() {
        let hit = |date: &str| {
            format!(
                r#"{{"results":[{{"titles":[{{"id":"JURITEXT000051743650"}}],"date":"{date}"}}]}}"#
            )
        };
        let mut diags = Diagnostics::default();
        assert_eq!(
            search_page(dto(&hit("hier")), &query(Fond::Juri, 10), &mut diags),
            Err(CorpusError::Decode { detail: "/search: results[].date: unreadable date".into() })
        );
        let (page, _) = page_of(Fond::Juri, &hit("2024-03-15"));
        assert_eq!(page.hits[0].date, Some(day(2024, 3, 15)));
    }

    /// Résultat de `json`, lu comme la production.
    fn result_of(json: &Value) -> SearchResultDto {
        decode(&json.to_string(), "/search").unwrap()
    }

    /// Texte d'un extrait, ou `None`.
    fn text_of(snippet: Option<Snippet>) -> Option<String> {
        snippet.map(|snippet| snippet.as_str().to_owned())
    }

    #[test]
    fn snippet_prefers_the_text() {
        let result = result_of(&json!({
            "text": "a <mark>b</mark> c",
            "sections": [{ "extracts": [{ "values": ["zzz"] }] }],
        }));
        assert_eq!(text_of(hit_snippet(&result)).as_deref(), Some("a b c"));
    }

    #[test]
    fn snippet_falls_back_to_the_first_extract_value() {
        let result = result_of(&json!({ "sections": [
            { "extracts": [] },
            { "extracts": [
                { "values": [] },
                { "values": ["", "x"] },
                { "values": ["<mark>y</mark>", "z"] },
            ] },
        ] }));
        assert_eq!(text_of(hit_snippet(&result)).as_deref(), Some("y"));
    }

    #[test]
    fn blank_text_falls_back_to_the_extracts() {
        let result = result_of(&json!({
            "text": " <mark> </mark> ",
            "sections": [{ "extracts": [{ "values": ["u"] }] }],
        }));
        assert_eq!(text_of(hit_snippet(&result)).as_deref(), Some("u"));
    }

    #[test]
    fn snippet_is_none_without_text_or_usable_extract() {
        for result in [
            json!({}),
            json!({ "text": "" }),
            json!({ "text": "<mark></mark>", "sections": [] }),
            json!({ "sections": [{ "extracts": [{ "values": ["", "x"] }, { "values": [] }, {}] }] }),
        ] {
            assert_eq!(hit_snippet(&result_of(&result)), None, "{result}");
        }
    }

    #[test]
    fn snippet_is_truncated_to_280_chars() {
        let count = |n: usize| {
            let result = result_of(&json!({ "text": "a".repeat(n) }));
            text_of(hit_snippet(&result)).unwrap()
        };
        assert_eq!(count(280), "a".repeat(280));
        assert_eq!(count(281), format!("{}…", "a".repeat(279)));
    }

    /// Sections de `json`, lues comme la production.
    fn sections_of(json: &Value) -> Vec<SearchSectionDto> {
        result_of(&json!({ "sections": json })).sections
    }

    /// `(identifiant, numéro, extrait)` des références de `articles`.
    fn flat(articles: &[CodeArticleRef]) -> Vec<(String, Option<String>, Option<String>)> {
        articles
            .iter()
            .map(|article| {
                (
                    article.article_id.as_str().to_owned(),
                    article.num.clone(),
                    text_of(article.snippet.clone()),
                )
            })
            .collect()
    }

    /// `Some(texte.to_owned())` : une macro, pour la même raison que `at!`.
    macro_rules! some {
        ($text:expr) => {
            Some($text.to_owned())
        };
    }

    #[test]
    fn code_article_refs_come_from_the_extracts() {
        let sections = sections_of(&json!([
            { "extracts": [
                { "id": "LEGIARTI000006419288", "num": " 9 <mark>bis</mark> ",
                  "values": ["[...]Chacun <mark>a</mark> droit[...]", "second"] },
                { "num": "7", "values": ["sans id"] },
                { "id": "KALIARTI000005805295", "num": "1", "values": ["kali"] },
                { "id": "LEGIARTI12", "num": "2", "values": ["court"] },
                { "id": "LEGIARTI000006419289" },
            ] },
            { "extracts": [
                { "id": "LEGIARTI000006419290_01-01-2020", "num": "4", "values": ["<mark> </mark>"] },
            ] },
        ]));
        let articles = hit_articles(Fond::Code, &sections);
        assert_eq!(
            flat(&articles),
            [
                (
                    "LEGIARTI000006419288".to_owned(),
                    some!("9 bis"),
                    some!("[...]Chacun a droit[...]")
                ),
                ("LEGIARTI000006419289".to_owned(), None, None),
                ("LEGIARTI000006419290".to_owned(), some!("4"), None),
            ]
        );
        let result = result_of(&json!({ "sections": [
            { "extracts": [{ "id": "LEGIARTI000006419288", "values": ["premier", "second"] }] },
        ] }));
        assert_eq!(text_of(hit_snippet(&result)).as_deref(), Some("premier"));
    }

    #[test]
    fn kali_article_refs_have_no_num() {
        let sections = sections_of(&json!([{ "extracts": [
            { "id": "KALIARTI000005805295", "num": "8.4 de la convention)", "values": ["v1"] },
            { "id": "LEGIARTI000006419288", "num": "1", "values": ["legi"] },
            { "id": "KALIARTI000005805297", "num": "", "values": [] },
        ] }]));
        assert_eq!(
            flat(&hit_articles(Fond::Kali, &sections)),
            [
                ("KALIARTI000005805295".to_owned(), None, some!("v1")),
                ("KALIARTI000005805297".to_owned(), None, None),
            ]
        );
    }

    #[test]
    fn other_fonds_have_no_article_refs() {
        let both = sections_of(&json!([{ "extracts": [
            { "id": "LEGIARTI000006419288", "num": "1", "values": ["a"] },
            { "id": "KALIARTI000005805295", "num": "2", "values": ["b"] },
        ] }]));
        for fond in [Fond::Juri, Fond::Constit, Fond::Cetat, Fond::Cnil, Fond::Acco, Fond::Loda] {
            assert_eq!(hit_articles(fond, &both), [], "{fond:?}");
        }
        let kali_only = sections_of(&json!([{ "extracts": [{ "id": "KALIARTI000005805295" }] }]));
        assert_eq!(hit_articles(Fond::Code, &kali_only), []);
        let legi_only = sections_of(&json!([{ "extracts": [{ "id": "LEGIARTI000006419288" }] }]));
        assert_eq!(hit_articles(Fond::Kali, &legi_only), []);
        assert_eq!(hit_articles(Fond::Code, &[]), []);
    }

    #[test]
    fn a_kept_hit_carries_its_date_snippet_and_articles() {
        let json = json!({ "results": [{
            "titles": [{ "id": "LEGITEXT000006070721" }],
            "date": "2016-10-09T00:00:00.000+0000",
            "sections": [{ "extracts": [
                { "id": "LEGIARTI000006419288", "num": "3", "values": ["<mark>x</mark>"] },
            ] }],
        }] })
        .to_string();
        let (page, _) = page_of(Fond::Code, &json);
        let hit = &page.hits[0];
        assert_eq!(hit.date, Some(day(2016, 10, 9)));
        assert_eq!(text_of(hit.snippet.clone()).as_deref(), Some("x"));
        assert_eq!(
            flat(&hit.articles),
            [("LEGIARTI000006419288".to_owned(), some!("3"), some!("x"))]
        );
    }
}
