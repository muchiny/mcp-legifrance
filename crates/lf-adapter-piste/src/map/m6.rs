//! Mapping des réponses de M6 vers le domaine : `CodeListing`, article trouvé par numéro, hits de
//! recherche par numéro (04 §7.2, §9).
//!
//! Chaque fonction est pure et ne journalise rien : les anomalies sont comptées dans
//! [`Diagnostics`] (un code écarté compte comme un résultat écarté, `dropped_hits`), et aucune
//! valeur du corps ne sort d'ici, ni dans une erreur ni dans un compteur.
//!
//! - **Codes** : [`code_page`] garde les entrées qui ont un identifiant `LEGITEXT` lisible et un
//!   titre ; les autres sont écartées et comptées (03 §4.11).
//! - **Article** : [`found_article`] lit la réponse de `getArticleWithIdAndNum`, de même forme que
//!   celle de `getArticle`. L'identifiant demandé n'est pas connu d'avance (la requête porte le
//!   texte et le numéro) : c'est l'identifiant renvoyé qui fait foi, et le mapper de `getArticle`
//!   le reçoit comme identifiant demandé. [`article_of_text`] fait la même résolution dans les
//!   hits de la recherche exacte (04 §7.2 étape 3).
//! - **Hits** : [`number_page`] lit une page de `/search` par le mapper de la recherche. Les
//!   consultations (IDCC, NOR, ELI) rendent au plus un hit synthétisé : [`kali_cont_hits`],
//!   [`jorf_hits`], [`eli_article_hits`]. Un hit `LODA` porte un identifiant `LEGITEXT` ; un texte
//!   du Journal officiel sans version consolidée (`JORFTEXT…`) ne peut pas en être un tant que
//!   la surface JORF n'est pas ouverte (T-M7-01, 00 §4) : il ne donne aucun hit.

use lf_application::ports::CorpusError;
use lf_domain::contract::Idcc;
use lf_domain::entity::{ArticleText, CodeArticleRef, CodeListing, SearchHit, SearchPage};
use lf_domain::fond::Fond;
use lf_domain::id::{ArticleId, DocumentId, LegiTextId};
use lf_domain::search::SearchQuery;
use lf_domain::status::{LegalStatus, status_of};
use lf_domain::text::clean_field;

use super::article::article;
use super::body::non_blank;
use super::rules::{note_suffix, suffix_start};
use super::search::search_page_with_mode;
use super::{Diagnostics, Field, not_found};
use crate::dto::article::GetArticleResponseDto;
use crate::dto::m6::{
    CodeEntryDto, CodeListResponseDto, ConsultJorfDto, KaliContDto, KaliSectionDto,
};
use crate::dto::search::SearchResponseDto;

/// Une page de `/list/code`, mappée.
#[derive(Debug, PartialEq)]
pub(crate) struct CodePage {
    /// Codes retenus, dans l'ordre reçu.
    pub(crate) listings: Vec<CodeListing>,
    /// Nombre d'entrées reçues, écartées comprises : il décide s'il reste des pages.
    pub(crate) received: usize,
    /// Nombre total de codes annoncé par PISTE.
    pub(crate) total: Option<u64>,
}

/// Page de codes de `dto` ; les entrées écartées sont comptées dans `diags`.
pub(crate) fn code_page(dto: &CodeListResponseDto, diags: &mut Diagnostics) -> CodePage {
    let received = dto.results.len();
    let listings = dto
        .results
        .iter()
        .filter_map(|entry| {
            let listing = code_listing(entry);
            if listing.is_none() {
                diags.note_dropped_hit();
            }
            listing
        })
        .collect();
    CodePage { listings, received, total: dto.total_result_number }
}

/// Code d'une entrée : le premier de `id` et `cid` lisible comme `LEGITEXT`, un titre non vide
/// après nettoyage. L'état absent ou inconnu devient `None`.
fn code_listing(entry: &CodeEntryDto) -> Option<CodeListing> {
    let id = [entry.id.as_deref(), entry.cid.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|raw| LegiTextId::parse(raw).ok())?;
    let title = clean_field(entry.titre.as_deref())?;
    Some(CodeListing { id, title, etat: status_of(entry.etat.as_deref(), None) })
}

/// Article de la réponse `dto` de `endpoint` (`getArticleWithIdAndNum`, ou l'endpoint ELI
/// d'article, de même forme que `getArticle`).
///
/// L'identifiant demandé n'existe pas : le mapper de `getArticle` reçoit comme tel celui que la
/// réponse porte (`id`, à défaut `cid`), ce qui ne compte aucune discordance d'identifiant tant que
/// la réponse est cohérente.
///
/// # Errors
///
/// [`CorpusError::NotFound`] (`endpoint`) si l'article est absent, `null` ou sans `id` ni `cid`.
/// Ensuite `Decode` (`<endpoint>: article.id: invalid identifier`) pour un identifiant illisible,
/// puis les erreurs de [`article`].
pub(crate) fn found_article(
    dto: GetArticleResponseDto,
    endpoint: &'static str,
    diags: &mut Diagnostics,
) -> Result<ArticleText, CorpusError> {
    let raw = dto
        .article
        .as_ref()
        .and_then(|found| {
            non_blank(found.id.as_deref()).or_else(|| non_blank(found.cid.as_deref()))
        })
        .map(str::to_owned);
    let Some(raw) = raw else {
        return Err(not_found(endpoint));
    };
    let field = Field { endpoint, path: "article.id" };
    let requested = raw.parse::<ArticleId>().map_err(|error| field.domain(&error))?;
    article(dto, &requested, diags)
}

/// Page de `/search` lue pour une recherche par numéro ; `frame` est la requête de cadrage
/// (`request::m6::result_frame`), qui porte le fond et la taille de page demandée.
///
/// # Errors
///
/// Celles de [`search_page_with_mode`] : `Decode` si la page porte plus de résultats que la taille
/// demandée ou si la date d'un résultat est illisible.
pub(crate) fn number_page(
    dto: SearchResponseDto,
    frame: &SearchQuery,
    diags: &mut Diagnostics,
) -> Result<SearchPage, CorpusError> {
    search_page_with_mode(dto, frame, diags).map(|(page, _received)| page)
}

/// Identifiant de l'article numéro `num` du texte `text` dans `page`, une recherche exacte sur
/// `NUM_ARTICLE` (04 §7.2 étape 3) : le hit dont l'identifiant chronologique (`cid`) est le
/// `LEGITEXT` du code, puis son extrait dont le numéro est celui demandé, sans égard à la casse.
pub(crate) fn article_of_text(
    page: &SearchPage,
    text: &LegiTextId,
    num: &str,
) -> Option<ArticleId> {
    page.hits
        .iter()
        .filter(|hit| {
            hit.cid.as_ref().is_some_and(|cid| cid.as_str() == text.as_str())
                || hit.id.as_str() == text.as_str()
        })
        .flat_map(|hit| &hit.articles)
        .find(|found| {
            found.num.as_deref().is_some_and(|n| n.trim().eq_ignore_ascii_case(num.trim()))
        })
        .map(|found| found.article_id.clone())
}

/// Hit `fond` d'identifiant `raw`, lu par le domaine, sans compter le suffixe de version. `None` si
/// l'identifiant est illisible ou d'une autre famille que le fond.
fn readable_hit(fond: Fond, raw: &str) -> Option<SearchHit> {
    let id = DocumentId::parse(raw).ok()?;
    SearchHit::new(fond, id, suffix_start(raw)).ok()
}

/// Hit `fond` d'identifiant `raw` : lu par le domaine puis rapproché du fond, suffixe de version
/// retiré et compté. `None` si l'identifiant est illisible ou d'une autre famille que le fond.
fn start_hit(fond: Fond, raw: &str, diags: &mut Diagnostics) -> Option<SearchHit> {
    let hit = readable_hit(fond, raw)?;
    note_suffix(raw, hit.id.as_str(), diags);
    Some(hit)
}

/// Préfixe de l'intitulé d'une section de texte de base.
const BASE_TEXT_PREFIX: &str = "Texte de base :";

/// Intitulé de `section` sans le préfixe `Texte de base : `, `None` s'il n'en reste rien.
fn section_title(section: &KaliSectionDto) -> Option<String> {
    let title = clean_field(section.title.as_deref())?;
    let bare = title.strip_prefix(BASE_TEXT_PREFIX).unwrap_or(&title).trim();
    (!bare.is_empty()).then(|| bare.to_owned())
}

/// Le texte est en vigueur : `VIGUEUR`, `VIGUEUR_ETEN` ou `VIGUEUR_NON_ETEN` (`VIGUEUR_DIFF` n'est
/// pas encore applicable).
fn is_in_force(status: Option<&LegalStatus>) -> bool {
    matches!(
        status,
        Some(LegalStatus::Vigueur | LegalStatus::VigueurEtendue | LegalStatus::VigueurNonEtendue)
    )
}

/// Section de `dto` du texte de base `id` et son état.
fn base_section<'a>(
    dto: &'a KaliContDto,
    id: &str,
) -> Option<(&'a KaliSectionDto, Option<LegalStatus>)> {
    dto.sections
        .iter()
        .find(|section| section.id.as_deref().is_some_and(|found| found.trim() == id))
        .map(|section| (section, status_of(section.etat.as_deref(), None)))
}

/// Au plus un hit KALI : parmi les textes de base (`texteBaseId`) lisibles comme `KALITEXT`, le
/// premier dont la section est en vigueur, sinon le premier lisible ; avec l'IDCC du conteneur et
/// l'état de sa section. Le titre est celui du conteneur (la version à jour), sauf pour un texte
/// dont la section dit qu'il n'est pas en vigueur : le conteneur le présenterait sous la citation
/// d'un autre texte, le titre est alors celui de sa section, sans `Texte de base : `. Des textes
/// de base tous illisibles sont comptés comme un résultat écarté.
pub(crate) fn kali_cont_hits(dto: &KaliContDto, diags: &mut Diagnostics) -> Vec<SearchHit> {
    let readable: Vec<(&str, SearchHit)> = dto
        .texte_base_id
        .iter()
        .filter_map(|raw| readable_hit(Fond::Kali, raw).map(|hit| (raw.as_str(), hit)))
        .collect();
    let in_force = readable.iter().position(|(_, hit)| {
        is_in_force(base_section(dto, hit.id.as_str()).and_then(|s| s.1).as_ref())
    });
    let Some((raw, mut hit)) = readable.into_iter().nth(in_force.unwrap_or(0)) else {
        if !dto.texte_base_id.is_empty() {
            diags.note_dropped_hit();
        }
        return Vec::new();
    };
    note_suffix(raw, hit.id.as_str(), diags);
    hit.title = clean_field(dto.titre.as_deref());
    hit.idcc = dto.num.as_deref().and_then(Idcc::parse);
    if let Some((section, status)) = base_section(dto, hit.id.as_str()) {
        if status.is_some() && !is_in_force(status.as_ref()) {
            hit.title = section_title(section);
        }
        hit.etat = status;
    }
    vec![hit]
}

/// Au plus un hit LODA, pour un texte du Journal officiel (NOR, ELI de texte) : l'identifiant ou le
/// `cid` lisible comme `LEGITEXT`, avec le titre et la nature. Un texte sans version consolidée
/// (`JORFTEXT…`) ne donne aucun hit (voir la documentation du module).
pub(crate) fn jorf_hits(dto: &ConsultJorfDto, diags: &mut Diagnostics) -> Vec<SearchHit> {
    let found = [dto.id.as_deref(), dto.cid.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|raw| start_hit(Fond::Loda, raw, diags));
    found
        .map(|mut hit| {
            hit.title = clean_field(dto.title.as_deref());
            hit.nature = clean_field(dto.nature.as_deref());
            hit
        })
        .into_iter()
        .collect()
}

/// Au plus un hit LODA, pour un article cité par son ELI : le texte parent de l'article (`LEGITEXT`),
/// qui porte l'article. Un article sans texte parent consolidé (autre famille, parent absent) ne
/// donne aucun hit.
pub(crate) fn eli_article_hits(found: ArticleText) -> Vec<SearchHit> {
    let ArticleText { article_id, num, code_title, parent, .. } = found;
    let hit = parent.and_then(|parent| SearchHit::new(Fond::Loda, parent.id, None).ok());
    hit.map(|mut hit| {
        hit.title = code_title;
        hit.articles = vec![CodeArticleRef::new(article_id, num, "")];
        hit
    })
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::decode::decode;
    use crate::map::fixtures::read;
    use crate::request::m6::{GET_ARTICLE_WITH_ID_AND_NUM, result_frame};
    use lf_domain::entity::ParentText;
    use lf_domain::search::{NumberKind, NumberQuery, NumberValue};

    macro_rules! text {
        ($value:expr) => {
            Some($value.to_owned())
        };
    }

    const PAGE_1: &str =
        include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page1.json");
    const PAGE_2: &str =
        include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page2.json");
    const CIVIL_9: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getArticleWithIdAndNum/m6/getArticleWithIdAndNum-code-civil-9.json"
    );
    const UNKNOWN_NUMBER: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getArticleWithIdAndNum/m6/getArticleWithIdAndNum-numero-inconnu.json"
    );
    const EXACT_9: &str =
        include_str!("../../tests/fixtures/recorded/search/m6/search-exacte-num-article-9.json");
    const ECLI_JURI: &str = include_str!(
        "../../tests/fixtures/recorded/search/m6/search-ecli-JURI-ECLI-FR-CCASS-2025-C100399.json"
    );
    const ECLI_UNKNOWN: &str =
        include_str!("../../tests/fixtures/recorded/search/m6/search-ecli-JURI-inconnu.json");
    const IDCC_2121: &str = include_str!(
        "../../tests/fixtures/recorded/consult/kaliContIdcc/m6/kaliContIdcc-2121.json"
    );
    const NOR: &str = include_str!(
        "../../tests/fixtures/recorded/consult/getJoWithNor/m6/getJoWithNor-AGRX2502679L.json"
    );

    fn civil_code() -> LegiTextId {
        LegiTextId::parse("LEGITEXT000006070721").unwrap()
    }

    fn at() -> lf_domain::date::VersionDate {
        lf_domain::date::VersionDate::parse("2026-10-28").unwrap()
    }

    fn code_query() -> NumberQuery {
        NumberQuery::new(Fond::Code, NumberKind::Article, NumberValue::parse("9").unwrap(), at())
            .unwrap()
    }

    fn page_of(raw: &str, query: &NumberQuery) -> SearchPage {
        let frame = result_frame(query).unwrap();
        number_page(read(raw, "/search"), &frame, &mut Diagnostics::default()).unwrap()
    }

    // ---- codes ---------------------------------------------------------------------------

    #[test]
    fn the_recorded_pages_give_108_codes_with_their_state() {
        let mut diags = Diagnostics::default();
        let first = code_page(&read(PAGE_1, "/list/code"), &mut diags);
        let second = code_page(&read(PAGE_2, "/list/code"), &mut diags);
        assert_eq!((first.listings.len(), first.received, first.total), (100, 100, Some(108)));
        assert_eq!((second.listings.len(), second.received, second.total), (8, 8, Some(108)));
        assert_eq!(diags, Diagnostics::default());
        let all: Vec<&CodeListing> = first.listings.iter().chain(&second.listings).collect();
        let in_force = all.iter().filter(|code| code.etat == Some(LegalStatus::Vigueur)).count();
        let repealed = all.iter().filter(|code| code.etat == Some(LegalStatus::Abroge)).count();
        assert_eq!((in_force, repealed), (76, 32));
        assert_eq!(
            first.listings[0],
            CodeListing {
                id: LegiTextId::parse("LEGITEXT000006074232").unwrap(),
                title: "Code de déontologie des architectes".to_owned(),
                etat: Some(LegalStatus::Vigueur),
            }
        );
        let civil = all.iter().find(|code| code.id == civil_code()).unwrap();
        assert_eq!(civil.title, "Code civil");
    }

    #[test]
    fn a_code_without_a_readable_identifier_or_a_title_is_dropped_and_counted() {
        let dto: CodeListResponseDto = read(
            r#"{"totalResultNumber":5,"results":[
                {"id":"LEGITEXT000006070721","titre":"Code civil","etat":"VIGUEUR"},
                {"id":"","cid":"LEGITEXT000006069577_01-01-2020","titre":"Code de commerce"},
                {"id":"JORFTEXT000033202746","cid":"LEGITEXT000023501962","titre":" Code minier ","etat":"  "},
                {"id":"LEGITEXT000006070716","titre":"  "},
                {"id":"JORFTEXT000033202746","titre":"Pas un code"},
                {"titre":"Sans identifiant"}]}"#,
            "/list/code",
        );
        let mut diags = Diagnostics::default();
        let page = code_page(&dto, &mut diags);
        let seen: Vec<(&str, &str, Option<LegalStatus>)> = page
            .listings
            .iter()
            .map(|code| (code.id.as_str(), code.title.as_str(), code.etat.clone()))
            .collect();
        assert_eq!(
            seen,
            [
                ("LEGITEXT000006070721", "Code civil", Some(LegalStatus::Vigueur)),
                ("LEGITEXT000006069577", "Code de commerce", None),
                ("LEGITEXT000023501962", "Code minier", None),
            ]
        );
        assert_eq!((page.received, page.total), (6, Some(5)));
        assert_eq!(diags, Diagnostics { dropped_hits: 3, ..Diagnostics::default() });
    }

    #[test]
    fn an_empty_code_page_is_empty() {
        let page = code_page(&read("{}", "/list/code"), &mut Diagnostics::default());
        assert_eq!(page, CodePage { listings: Vec::new(), received: 0, total: None });
    }

    // ---- article par numéro ------------------------------------------------------------

    #[test]
    fn the_recorded_article_maps_without_an_identifier_mismatch() {
        let mut diags = Diagnostics::default();
        let found = found_article(
            read(CIVIL_9, "/consult/getArticleWithIdAndNum"),
            GET_ARTICLE_WITH_ID_AND_NUM,
            &mut diags,
        );
        let found = found.unwrap();
        assert_eq!(found.article_id.as_str(), "LEGIARTI000006419288");
        assert_eq!(found.num.as_deref(), Some("9"));
        assert_eq!(found.code_title.as_deref(), Some("Code civil"));
        assert_eq!(
            found.parent.as_ref().map(|parent| parent.id.as_str()),
            Some("LEGITEXT000006070721")
        );
        assert!(found.text.as_str().starts_with("Chacun a droit au respect de sa vie privée."));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn an_empty_envelope_or_an_article_without_identity_is_not_found() {
        for raw in [
            UNKNOWN_NUMBER,
            r#"{"article":{"num":"9","texte":"x"}}"#,
            "{}",
            r#"{"article":{"id":" ","cid":""}}"#,
        ] {
            let mut diags = Diagnostics::default();
            let result = found_article(
                read(raw, "/consult/getArticleWithIdAndNum"),
                GET_ARTICLE_WITH_ID_AND_NUM,
                &mut diags,
            );
            assert_eq!(
                result.unwrap_err(),
                CorpusError::NotFound { what: "/consult/getArticleWithIdAndNum".to_owned() },
                "{raw}"
            );
            assert_eq!(diags, Diagnostics::default());
        }
    }

    #[test]
    fn the_chronological_identifier_stands_in_for_a_missing_id() {
        let raw = r#"{"article":{"cid":"LEGIARTI000006419288","num":"9","texte":"Texte.","etat":"VIGUEUR"}}"#;
        let mut diags = Diagnostics::default();
        let found = found_article(
            read(raw, "/consult/getArticleWithIdAndNum"),
            GET_ARTICLE_WITH_ID_AND_NUM,
            &mut diags,
        )
        .unwrap();
        assert_eq!(found.article_id.as_str(), "LEGIARTI000006419288");
        // L'identifiant renvoyé manque : la discordance est comptée, par le mapper d'article.
        assert_eq!(diags, Diagnostics { id_mismatch: 1, ..Diagnostics::default() });
    }

    #[test]
    fn an_unreadable_identifier_is_a_decode_error_without_the_value() {
        let raw = r#"{"article":{"id":"SECRET-ID-0000","num":"9","texte":"x"}}"#;
        let error = found_article(
            read(raw, "/consult/getArticleWithIdAndNum"),
            GET_ARTICLE_WITH_ID_AND_NUM,
            &mut Diagnostics::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode {
                detail: "/consult/getArticleWithIdAndNum: article.id: invalid identifier"
                    .to_owned()
            }
        );
    }

    // ---- recherche par numéro ----------------------------------------------------------

    #[test]
    fn the_exact_article_search_lists_the_codes_that_carry_the_number() {
        let page = page_of(EXACT_9, &code_query());
        assert_eq!((page.hits.len(), page.total, page.dropped), (7, 7, 0));
        let civil = page
            .hits
            .iter()
            .find(|hit| hit.id == DocumentId::parse("LEGITEXT000006070721").unwrap());
        let civil = civil.unwrap();
        assert_eq!(civil.fond, Fond::Code);
        assert_eq!(civil.cid.as_ref().map(DocumentId::as_str), Some("LEGITEXT000006070721"));
        let article = &civil.articles[0];
        assert_eq!(
            (article.article_id.as_str(), article.num.as_deref()),
            ("LEGIARTI000006419288", Some("9"))
        );
    }

    #[test]
    fn the_article_of_a_code_is_found_by_cid_and_number() {
        let page = page_of(EXACT_9, &code_query());
        let found = article_of_text(&page, &civil_code(), "9");
        assert_eq!(found.as_ref().map(ArticleId::as_str), Some("LEGIARTI000006419288"));
        let procedure = LegiTextId::parse("LEGITEXT000006070716").unwrap();
        assert_eq!(
            article_of_text(&page, &procedure, " 9 ").as_ref().map(ArticleId::as_str),
            Some("LEGIARTI000006410102")
        );
        // Autre numéro, autre texte : rien.
        assert_eq!(article_of_text(&page, &civil_code(), "10"), None);
        let unknown = LegiTextId::parse("LEGITEXT999999999999").unwrap();
        assert_eq!(article_of_text(&page, &unknown, "9"), None);
    }

    #[test]
    fn the_number_is_compared_without_regard_to_case() {
        let mut page = page_of(EXACT_9, &code_query());
        page.hits.retain(|hit| hit.id.as_str() == "LEGITEXT000006070721");
        page.hits[0].articles[0].num = text!("L1242-2");
        let civil = civil_code();
        assert!(article_of_text(&page, &civil, "l1242-2").is_some());
        assert!(article_of_text(&page, &civil, "L1242-3").is_none());
        // Un extrait sans numéro ne correspond à aucun numéro.
        page.hits[0].articles[0].num = None;
        assert!(article_of_text(&page, &civil, "").is_none());
    }

    #[test]
    fn a_hit_is_matched_by_its_identifier_when_it_has_no_cid() {
        let mut page = page_of(EXACT_9, &code_query());
        page.hits.retain(|hit| hit.id.as_str() == "LEGITEXT000006070721");
        page.hits[0].cid = None;
        assert!(article_of_text(&page, &civil_code(), "9").is_some());
    }

    #[test]
    fn the_ecli_search_gives_the_decision_or_nothing() {
        let query = ecli_query(Fond::Juri, "ECLI:FR:CCASS:2025:C100399");
        let page = page_of(ECLI_JURI, &query);
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].id.as_str(), "JURITEXT000051743650");
        assert_eq!(page.hits[0].fond, Fond::Juri);
        let none = page_of(ECLI_UNKNOWN, &ecli_query(Fond::Juri, "ECLI:FR:CCASS:1900:C000000"));
        assert_eq!((none.hits.len(), none.total), (0, 0));
    }

    fn ecli_query(fond: Fond, ecli: &str) -> NumberQuery {
        NumberQuery::new(fond, NumberKind::Ecli, NumberValue::parse(ecli).unwrap(), at()).unwrap()
    }

    #[test]
    fn a_page_with_more_results_than_asked_is_a_decode_error() {
        // La taille de page de l'ECLI est 10 : onze résultats dépassent.
        let results = [r#"{"titles":[{"id":"JURITEXT000051743650"}]}"#; 11].join(",");
        let raw = format!(r#"{{"results":[{results}]}}"#);
        let frame = result_frame(&ecli_query(Fond::Juri, "ECLI:FR:CCASS:2025:C100399")).unwrap();
        let error =
            number_page(read(&raw, "/search"), &frame, &mut Diagnostics::default()).unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode {
                detail: "/search: results: more results than page size".to_owned()
            }
        );
    }

    // ---- consultations : IDCC, NOR, ELI --------------------------------------------------

    #[test]
    fn the_recorded_container_gives_the_base_text_with_its_idcc() {
        let mut diags = Diagnostics::default();
        let hits = kali_cont_hits(&read(IDCC_2121, "/consult/kaliContIdcc"), &mut diags);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!((hit.fond, hit.id.as_str()), (Fond::Kali, "KALITEXT000005654011"));
        assert_eq!(
            hit.title.as_deref(),
            Some("Convention collective nationale de l'édition du 14 janvier 2000")
        );
        assert_eq!(hit.idcc.map(Idcc::get), Some(2121));
        assert_eq!(hit.etat, Some(LegalStatus::VigueurEtendue));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn the_first_readable_base_text_wins_and_unreadable_ones_are_counted_only_when_all_are() {
        let mut diags = Diagnostics::default();
        let dto: KaliContDto = read(
            r#"{"titre":"Convention","num":"x","texteBaseId":["nope","LEGITEXT000006070721","KALITEXT000005654011","KALITEXT000005654012"]}"#,
            "/consult/kaliContIdcc",
        );
        let hits = kali_cont_hits(&dto, &mut diags);
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            ["KALITEXT000005654011"]
        );
        assert_eq!(hits[0].idcc, None);
        assert_eq!(diags, Diagnostics::default());

        let mut diags = Diagnostics::default();
        let dto: KaliContDto =
            read(r#"{"texteBaseId":["nope","LEGITEXT000006070721"]}"#, "/consult/kaliContIdcc");
        assert_eq!(kali_cont_hits(&dto, &mut diags), Vec::new());
        assert_eq!(diags, Diagnostics { dropped_hits: 1, ..Diagnostics::default() });

        let mut diags = Diagnostics::default();
        assert_eq!(kali_cont_hits(&KaliContDto::default(), &mut diags), Vec::new());
        assert_eq!(diags, Diagnostics::default());
    }

    const IDCC_1486: &str =
        include_str!("../../tests/fixtures/derived/kaliContIdcc-1486-reduit.json");

    /// Conteneur d'un IDCC avec `texteBaseId` dans l'ordre donné et une section `(id, etat, titre)`
    /// par entrée de `sections`.
    fn container(bases: &[&str], sections: &[(&str, &str, &str)]) -> KaliContDto {
        let sections: Vec<serde_json::Value> = sections
            .iter()
            .map(|(id, etat, title)| serde_json::json!({ "id": id, "etat": etat, "title": title }))
            .collect();
        let raw = serde_json::json!({
            "titre": "Convention du conteneur, mise à jour par avenant n° 46 ",
            "num": "1486",
            "texteBaseId": bases,
            "sections": sections,
        });
        read(&raw.to_string(), "/consult/kaliContIdcc")
    }

    const OLD: &str = "KALITEXT000005679895";
    const CURRENT: &str = "KALITEXT000044253019";

    #[test]
    fn kali_cont_hits_prefers_the_text_in_force() {
        let sections = [
            (OLD, "ABROGE", "Texte de base : Convention de 1987. \n"),
            (CURRENT, "VIGUEUR_ETEN", "Texte de base : Convention du conteneur, version 2021"),
        ];
        for bases in [[OLD, CURRENT], [CURRENT, OLD]] {
            let mut diags = Diagnostics::default();
            let hits = kali_cont_hits(&container(&bases, &sections), &mut diags);
            assert_eq!(hits.len(), 1, "{bases:?}");
            assert_eq!(hits[0].id.as_str(), CURRENT, "{bases:?}");
            assert_eq!(hits[0].etat, Some(LegalStatus::VigueurEtendue), "{bases:?}");
            assert_eq!(
                hits[0].title.as_deref(),
                Some("Convention du conteneur, mise à jour par avenant n° 46"),
                "{bases:?}"
            );
            assert_eq!(hits[0].idcc.map(Idcc::get), Some(1486), "{bases:?}");
            assert_eq!(diags, Diagnostics::default());
        }
    }

    #[test]
    fn the_first_text_is_kept_when_it_is_the_one_in_force() {
        let sections = [(CURRENT, "VIGUEUR", "x"), (OLD, "ABROGE", "y")];
        let hits =
            kali_cont_hits(&container(&[CURRENT, OLD], &sections), &mut Diagnostics::default());
        assert_eq!(
            (hits[0].id.as_str(), hits[0].etat.clone()),
            (CURRENT, Some(LegalStatus::Vigueur))
        );
    }

    #[test]
    fn a_single_repealed_text_keeps_its_own_title_and_its_state() {
        let sections = [(OLD, "ABROGE", "Texte de base : Convention de 1987. \n")];
        let hits = kali_cont_hits(&container(&[OLD], &sections), &mut Diagnostics::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.as_str(), OLD);
        assert_eq!(hits[0].etat, Some(LegalStatus::Abroge));
        assert_eq!(hits[0].title.as_deref(), Some("Convention de 1987."));
    }

    #[test]
    fn several_repealed_texts_give_the_first_one_with_its_title() {
        let sections =
            [(OLD, "ABROGE", "Texte de base : Premier"), (CURRENT, "REMPLACE", "Second")];
        let hits =
            kali_cont_hits(&container(&[OLD, CURRENT], &sections), &mut Diagnostics::default());
        assert_eq!(
            (hits[0].id.as_str(), hits[0].etat.clone(), hits[0].title.as_deref()),
            (OLD, Some(LegalStatus::Abroge), Some("Premier"))
        );
    }

    #[test]
    fn a_deferred_or_unknown_state_is_not_in_force() {
        for etat in ["VIGUEUR_DIFF", "MODIFIE", "", "FOO"] {
            let sections = [(OLD, etat, "Texte de base : Ancien"), (CURRENT, "ABROGE", "Autre")];
            let hits =
                kali_cont_hits(&container(&[OLD, CURRENT], &sections), &mut Diagnostics::default());
            assert_eq!(hits[0].id.as_str(), OLD, "{etat}");
        }
        for etat in ["VIGUEUR", "VIGUEUR_ETEN", "VIGUEUR_NON_ETEN"] {
            let sections = [(OLD, "ABROGE", "Ancien"), (CURRENT, etat, "Courant")];
            let hits =
                kali_cont_hits(&container(&[OLD, CURRENT], &sections), &mut Diagnostics::default());
            assert_eq!(hits[0].id.as_str(), CURRENT, "{etat}");
        }
    }

    #[test]
    fn without_a_section_for_the_text_the_container_title_stays() {
        let hits = kali_cont_hits(
            &container(&[OLD, CURRENT], &[("KALITEXT000000000001", "VIGUEUR", "Autre")]),
            &mut Diagnostics::default(),
        );
        assert_eq!(hits[0].id.as_str(), OLD);
        assert_eq!(hits[0].etat, None);
        assert_eq!(
            hits[0].title.as_deref(),
            Some("Convention du conteneur, mise à jour par avenant n° 46")
        );
    }

    #[test]
    fn a_repealed_section_title_without_the_prefix_is_kept_whole() {
        for (title, expected) in [
            ("Convention de 1987", Some("Convention de 1987")),
            ("Texte de base :", None),
            ("Texte de base :  \n", None),
        ] {
            let hits = kali_cont_hits(
                &container(&[OLD], &[(OLD, "ABROGE", title)]),
                &mut Diagnostics::default(),
            );
            assert_eq!(hits[0].title.as_deref(), expected, "{title:?}");
        }
    }

    #[test]
    fn a_repealed_section_without_a_title_gives_no_title() {
        let hits = kali_cont_hits(
            &container(&[OLD], &[(OLD, "ABROGE", "  ")]),
            &mut Diagnostics::default(),
        );
        assert_eq!(
            (hits[0].etat.clone(), hits[0].title.clone()),
            (Some(LegalStatus::Abroge), None)
        );
    }

    #[test]
    fn an_unreadable_text_is_skipped_before_the_state_choice() {
        let sections = [(CURRENT, "VIGUEUR_ETEN", "Courant")];
        let hits = kali_cont_hits(
            &container(&["nope", "LEGITEXT000006070721", CURRENT], &sections),
            &mut Diagnostics::default(),
        );
        assert_eq!(hits[0].id.as_str(), CURRENT);
    }

    #[test]
    fn the_recorded_container_of_the_idcc_1486_gives_the_text_in_force() {
        let mut diags = Diagnostics::default();
        let hits = kali_cont_hits(&read(IDCC_1486, "/consult/kaliContIdcc"), &mut diags);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!((hit.fond, hit.id.as_str()), (Fond::Kali, CURRENT));
        assert_eq!(hit.etat, Some(LegalStatus::VigueurEtendue));
        assert_eq!(hit.idcc.map(Idcc::get), Some(1486));
        assert!(
            hit.title.as_deref().is_some_and(|t| t.ends_with("avenant n° 46 du 16 juillet 2021"))
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn a_journal_only_text_gives_no_hit() {
        let mut diags = Diagnostics::default();
        let jorf: ConsultJorfDto = read(NOR, "/consult/getJoWithNor");
        assert_eq!(jorf_hits(&jorf, &mut diags), Vec::new());
        assert_eq!(jorf_hits(&ConsultJorfDto::default(), &mut diags), Vec::new());
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn a_consolidated_text_gives_a_loda_hit() {
        let jorf: ConsultJorfDto = read(
            r#"{"id":"JORFTEXT000033202746","cid":"LEGITEXT000033205014_08-09-2023",
                "title":"LOI pour une <mark>République</mark> numérique","nature":"LOI"}"#,
            "/consult/getJoWithNor",
        );
        let mut diags = Diagnostics::default();
        let hits = jorf_hits(&jorf, &mut diags);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].fond, hits[0].id.as_str()), (Fond::Loda, "LEGITEXT000033205014"));
        assert_eq!(hits[0].title.as_deref(), Some("LOI pour une République numérique"));
        assert_eq!(hits[0].nature.as_deref(), Some("LOI"));
        assert_eq!(
            hits[0].version_start.map(|date| date.to_string()),
            Some("2023-09-08".to_owned())
        );
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn an_eli_article_gives_its_parent_text_carrying_the_article() {
        let found = found_article(
            read(CIVIL_9, "/consult/getArticleWithIdAndNum"),
            GET_ARTICLE_WITH_ID_AND_NUM,
            &mut Diagnostics::default(),
        )
        .unwrap();
        let hits = eli_article_hits(found.clone());
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!((hit.fond, hit.id.as_str()), (Fond::Loda, "LEGITEXT000006070721"));
        assert_eq!(hit.title.as_deref(), Some("Code civil"));
        assert_eq!(hit.articles.len(), 1);
        assert_eq!(hit.articles[0].article_id.as_str(), "LEGIARTI000006419288");
        assert_eq!(hit.articles[0].num.as_deref(), Some("9"));
        assert_eq!(hit.articles[0].snippet, None);

        // Sans parent, ou avec un parent d'une autre famille : aucun hit.
        let orphan = ArticleText { parent: None, ..found.clone() };
        assert_eq!(eli_article_hits(orphan), Vec::new());
        let kali = ParentText::new(DocumentId::parse("KALITEXT000005654011").unwrap(), None);
        let foreign = ArticleText { parent: Some(kali), ..found };
        assert_eq!(eli_article_hits(foreign), Vec::new());
    }

    #[test]
    fn decoding_goes_through_the_production_reader() {
        // Garde-fou : `read` des tests est bien `decode` de la production.
        let direct: CodeListResponseDto = decode(PAGE_2, "/list/code").unwrap();
        assert_eq!(direct, read(PAGE_2, "/list/code"));
    }
}
