//! Corps des requêtes de M6 envoyées à PISTE : `getArticleWithIdAndNum`, `/search` exacte sur
//! `NUM_ARTICLE` ou sur les autres numéros, `/list/code`, `getJoWithNor`, `kaliContIdcc` et les deux
//! endpoints ELI (04 §6.9, §7.1, §7.2 ; annexe B §6.6).
//!
//! Les identifiants et les numéros viennent du domaine, déjà validés ([`NumberValue`] : 1 à 128
//! caractères de `[A-Za-z0-9.*/:_ -]`) : ils sont envoyés tels quels, et `json!` construit des
//! corps sans échec possible. Seul le corps de `/search` peut être refusé, par des gardes
//! défensives qui recopient les règles du domaine (une [`NumberQuery`] aux champs publics peut être
//! assemblée à la main) : `InvalidRequest { status: None }`, sans appel réseau et avec un `warn!`
//! qui ne nomme jamais la valeur du numéro.
//!
//! Le filtre `DATE_VERSION` envoie le midi du jour à Paris en millisecondes Unix (04 §6.3) :
//! la conversion est celle de `request::search`, partagée (`paris_noon_ms`).

use std::num::NonZeroU32;

use lf_application::ports::CorpusError;
use lf_domain::{
    DomainError,
    date::{DateRange, VersionDate},
    fond::{Fond, PaginationMode},
    id::LegiTextId,
    search::{MatchMode, NumberKind, NumberQuery, NumberValue, PageSize, QueryText, SearchQuery},
};
use serde_json::{Value, json};

use super::search::paris_noon_ms;

/// `POST /consult/getArticleWithIdAndNum` : l'article en vigueur d'un texte, par son numéro.
pub(crate) const GET_ARTICLE_WITH_ID_AND_NUM: &str = "/consult/getArticleWithIdAndNum";

/// `POST /list/code` : la liste des codes.
pub(crate) const LIST_CODE: &str = "/list/code";

/// `POST /consult/getJoWithNor` : un texte du Journal officiel, par son NOR.
pub(crate) const GET_JO_WITH_NOR: &str = "/consult/getJoWithNor";

/// `POST /consult/kaliContIdcc` : le conteneur d'une convention collective, par son IDCC.
pub(crate) const KALI_CONT_IDCC: &str = "/consult/kaliContIdcc";

/// `POST /consult/getArticleWithIdEliOrAlias` : un article, par son ELI cité.
pub(crate) const GET_ARTICLE_WITH_ID_ELI_OR_ALIAS: &str = "/consult/getArticleWithIdEliOrAlias";

/// `POST /consult/eliAndAliasRedirectionTexte` : un texte, par son ELI cité.
pub(crate) const ELI_AND_ALIAS_REDIRECTION_TEXTE: &str = "/consult/eliAndAliasRedirectionTexte";

/// Taille des pages de `/list/code` (04 §7.1) : le maximum de l'API.
pub(crate) const CODE_PAGE_SIZE: u32 = 100;

/// Taille de page de la recherche par `NUM_ARTICLE` (04 §7.2 étape 3) : le maximum de l'API, pour
/// voir tous les codes qui portent le numéro.
pub(crate) const ARTICLE_PAGE_SIZE: u32 = 100;

/// Taille de page des autres recherches par numéro (04 §7.2).
pub(crate) const OTHER_PAGE_SIZE: u32 = 20;

/// Taille de page de la recherche par ECLI : la forme de la sonde V-128, qui n'attend qu'une
/// décision.
pub(crate) const ECLI_PAGE_SIZE: u32 = 10;

/// Facette du filtre de version (04 §6.2).
const DATE_VERSION: &str = "DATE_VERSION";

/// Facette du filtre par ECLI (04 §7.2), à valeur exacte.
const ECLI: &str = "ECLI";

/// Sérialise un corps déjà construit : `Value::to_string` ne peut pas échouer.
fn bytes(body: &Value) -> Vec<u8> {
    body.to_string().into_bytes()
}

/// Corps de `/list/code` pour la page `page` (à partir de 1), triée par titre.
pub(crate) fn list_code_body(page: u32) -> Vec<u8> {
    bytes(&json!({ "pageNumber": page, "pageSize": CODE_PAGE_SIZE, "sort": "TITLE_ASC" }))
}

/// Corps de `getArticleWithIdAndNum` : le texte `text` et le numéro `num`, tel que reçu.
pub(crate) fn article_by_number_body(text: &LegiTextId, num: &str) -> Vec<u8> {
    bytes(&json!({ "id": text.as_str(), "num": num }))
}

/// Corps de `getJoWithNor`.
pub(crate) fn nor_body(nor: &NumberValue) -> Vec<u8> {
    bytes(&json!({ "nor": nor.as_str() }))
}

/// Corps de `kaliContIdcc`.
pub(crate) fn idcc_body(idcc: &NumberValue) -> Vec<u8> {
    bytes(&json!({ "id": idcc.as_str() }))
}

/// Corps des endpoints ELI : le chemin cité, transmis tel quel (jamais un ELI fabriqué, 00 §4).
pub(crate) fn eli_body(eli: &NumberValue) -> Vec<u8> {
    bytes(&json!({ "idEliOrAlias": eli.as_str() }))
}

/// Ce que désigne un ELI cité, et donc l'endpoint qui le résout (04 §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EliTarget {
    /// Un article : le chemin finit par `/article_N`.
    Article,
    /// Un texte : le chemin finit par `/jo/texte`.
    Text,
}

impl EliTarget {
    /// Nature de l'ELI `eli` : un segment `/article_` en fait un ELI d'article, tout autre chemin
    /// est pris pour un ELI de texte.
    pub(crate) fn of(eli: &NumberValue) -> Self {
        if eli.as_str().contains("/article_") { Self::Article } else { Self::Text }
    }

    /// Gabarit d'endpoint qui résout cette nature d'ELI.
    pub(crate) const fn endpoint(self) -> &'static str {
        match self {
            Self::Article => GET_ARTICLE_WITH_ID_ELI_OR_ALIAS,
            Self::Text => ELI_AND_ALIAS_REDIRECTION_TEXTE,
        }
    }
}

/// Forme de la recherche d'une nature de numéro (04 §6.9, §7.2).
struct Shape {
    /// `typeChamp`.
    field: &'static str,
    /// `typeRecherche`.
    kind: &'static str,
    /// `typePagination` : `ARTICLE` pour `NUM_ARTICLE` seulement (04 §6.9).
    pagination: PaginationMode,
    /// `pageSize`.
    page_size: u32,
}

/// Recherche exacte (`EXACTE`) sur `field`.
const fn exact(field: &'static str, pagination: PaginationMode, page_size: u32) -> Shape {
    Shape { field, kind: "EXACTE", pagination, page_size }
}

/// Forme de la recherche de `kind`, ou `None` pour `Nor` et `Eli`, qui passent par une
/// consultation.
const fn shape(kind: NumberKind) -> Option<Shape> {
    match kind {
        NumberKind::Article => {
            Some(exact("NUM_ARTICLE", PaginationMode::Article, ARTICLE_PAGE_SIZE))
        }
        NumberKind::Text => Some(exact("NUM", PaginationMode::Defaut, OTHER_PAGE_SIZE)),
        NumberKind::Case => Some(exact("NUM_AFFAIRE", PaginationMode::Defaut, OTHER_PAGE_SIZE)),
        NumberKind::Decision => Some(exact("NUM_DEC", PaginationMode::Defaut, OTHER_PAGE_SIZE)),
        NumberKind::Deliberation => {
            Some(exact("NUM_DELIB", PaginationMode::Defaut, OTHER_PAGE_SIZE))
        }
        NumberKind::Idcc => Some(exact("IDCC", PaginationMode::Defaut, OTHER_PAGE_SIZE)),
        NumberKind::Ecli => Some(Shape {
            field: "ALL",
            kind: "UN_DES_MOTS",
            pagination: PaginationMode::Defaut,
            page_size: ECLI_PAGE_SIZE,
        }),
        NumberKind::Nor | NumberKind::Eli => None,
    }
}

/// Taille de page demandée pour `kind` : celle que le mapping prend pour borne d'une page.
pub(crate) const fn page_size(kind: NumberKind) -> u32 {
    match shape(kind) {
        Some(shape) => shape.page_size,
        None => OTHER_PAGE_SIZE,
    }
}

/// Refus d'un numéro d'article vide ou hors du jeu de caractères de [`NumberValue`], avant tout
/// appel réseau : texte fixe, jamais le numéro.
pub(crate) fn invalid_article_number() -> CorpusError {
    defensive("article number is empty, too long or has forbidden characters".to_owned())
}

/// Vérifie que la nature du numéro existe pour le fond (annexe B §6.6) : une [`NumberQuery`] aux
/// champs publics peut être assemblée à la main hors des couples autorisés.
///
/// # Errors
///
/// `CorpusError::InvalidRequest { status: None, .. }` pour un couple (fond, nature) hors de
/// l'annexe, sans appel réseau.
pub(crate) fn ensure_applicable(query: &NumberQuery) -> Result<(), CorpusError> {
    if query.kind.applies_to(query.fond) {
        Ok(())
    } else {
        Err(defensive(format!(
            "number kind {} does not apply to fond {}",
            query.kind.as_str(),
            query.fond.name()
        )))
    }
}

/// Requête de cadrage des résultats d'une recherche par numéro : le fond, la taille de page et la
/// période de `query`, la valeur pour texte, et la correspondance exacte. Le mapping de
/// `/search` la lit pour borner la page (`ensure_fits`) et rattacher chaque hit à son fond ; elle
/// ne part jamais vers l'API.
///
/// # Errors
///
/// `CorpusError::InvalidRequest { status: None, .. }` si la valeur est faite de blancs (rien à
/// chercher) ou si le domaine refuse l'assemblage (`version_date` ou période incohérentes).
pub(crate) fn result_frame(query: &NumberQuery) -> Result<SearchQuery, CorpusError> {
    let text = QueryText::parse(query.value.as_str()).map_err(domain_refusal)?;
    SearchQuery::new(
        query.fond,
        text,
        NonZeroU32::MIN,
        PageSize::new(page_size(query.kind)).unwrap_or_default(),
        MatchMode::Exact,
        query.within,
        query.version_date,
    )
    .map_err(domain_refusal)
}

/// Refus du domaine devenu garde défensive : texte fixe, jamais la valeur refusée.
fn domain_refusal(_error: DomainError) -> CorpusError {
    defensive("number query refused: blank value or inconsistent dates".to_owned())
}

/// Corps de `/search` pour la page `page` d'une recherche par numéro.
///
/// # Errors
///
/// `CorpusError::InvalidRequest { status: None, .. }` si une garde défensive refuse la requête :
/// nature qui n'existe pas pour le fond ([`ensure_applicable`]) ou qui ne passe pas par `/search`
/// (`Nor`, `Eli`), `version_date` absente (CODE, LODA) ou superflue, période sur un fond sans
/// facette de date. Aucun appel réseau n'a alors eu lieu.
pub(crate) fn number_search_body(query: &NumberQuery, page: u32) -> Result<Vec<u8>, CorpusError> {
    ensure_applicable(query)?;
    let fond = query.fond;
    let Some(shape) = shape(query.kind) else {
        return Err(defensive(format!(
            "number kind {} is not searchable, it is a consultation",
            query.kind.as_str()
        )));
    };
    let mut filtres: Vec<Value> = version_filter(fond, query.version_date)?.into_iter().collect();
    // L'ECLI est la valeur exacte de sa facette ; la requête, `*`, ne sert qu'à tout accepter.
    let value = if query.kind == NumberKind::Ecli {
        filtres.push(json!({ "facette": ECLI, "valeurs": [query.value.as_str()] }));
        "*"
    } else {
        query.value.as_str()
    };
    if let Some(range) = query.within {
        filtres.push(range_filter(fond, range)?);
    }
    Ok(bytes(&json!({
        "fond": fond.api_name(),
        "recherche": {
            "champs": [{
                "typeChamp": shape.field,
                "criteres": [{ "typeRecherche": shape.kind, "valeur": value, "operateur": "ET" }],
                "operateur": "ET",
            }],
            "filtres": filtres,
            "pageNumber": page,
            "pageSize": shape.page_size,
            "operateur": "ET",
            "sort": "PERTINENCE",
            "typePagination": shape.pagination.api_name(),
        },
    })))
}

/// Filtre `DATE_VERSION` : exigé pour LODA et CODE, refusé ailleurs (04 §6.2).
fn version_filter(
    fond: Fond,
    version_date: Option<VersionDate>,
) -> Result<Option<Value>, CorpusError> {
    match (fond.needs_version_date(), version_date) {
        (true, Some(date)) => {
            Ok(Some(json!({ "facette": DATE_VERSION, "singleDate": paris_noon_ms(date) })))
        }
        (true, None) => Err(defensive(format!("version_date missing for fond {}", fond.name()))),
        (false, Some(_)) => {
            Err(defensive(format!("version_date not applicable to fond {}", fond.name())))
        }
        (false, None) => Ok(None),
    }
}

/// Filtre de période sur la facette de date du fond (04 §6.5).
fn range_filter(fond: Fond, range: DateRange) -> Result<Value, CorpusError> {
    let facette = date_facet(fond)
        .ok_or_else(|| defensive(format!("date range not applicable to fond {}", fond.name())))?;
    Ok(json!({
        "facette": facette,
        "dates": { "start": range.start().to_string(), "end": range.end().to_string() },
    }))
}

/// Facette de date du fond ; `None` pour CODE, qui n'a pas de filtre de période.
const fn date_facet(fond: Fond) -> Option<&'static str> {
    match fond {
        Fond::Loda | Fond::Kali | Fond::Acco => Some("DATE_SIGNATURE"),
        Fond::Juri | Fond::Constit | Fond::Cetat => Some("DATE_DECISION"),
        Fond::Cnil => Some("DATE_DELIB"),
        Fond::Code => None,
    }
}

/// Garde défensive franchie : probable bogue de l'appelant, journalisé sans la valeur du numéro,
/// et erreur sans statut HTTP (02 §5.1).
fn defensive(detail: String) -> CorpusError {
    tracing::warn!(%detail, "number search request refused before sending: probable bug");
    CorpusError::InvalidRequest { status: None, detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn value(raw: &str) -> NumberValue {
        NumberValue::parse(raw).unwrap()
    }

    fn body(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap()
    }

    /// Requête recopiée dans le sidecar `.meta.toml` d'une fixture enregistrée.
    fn recorded(meta: &str) -> Value {
        let table: toml::Table = toml::from_str(meta).unwrap();
        serde_json::from_str(table["request"].as_str().unwrap()).unwrap()
    }

    fn query(fond: Fond, kind: NumberKind, raw: &str, at: &str) -> NumberQuery {
        NumberQuery::new(fond, kind, value(raw), day(at)).unwrap()
    }

    fn sent(query: &NumberQuery) -> Value {
        body(&number_search_body(query, 1).unwrap())
    }

    /// Garde défensive attendue : `InvalidRequest` sans statut, de détail `detail`.
    fn invalid(detail: &str) -> CorpusError {
        CorpusError::InvalidRequest { status: None, detail: detail.to_owned() }
    }

    fn refused(query: &NumberQuery) -> CorpusError {
        number_search_body(query, 1).unwrap_err()
    }

    // ---- les corps enregistrés en sandbox (I-R) --------------------------------------------

    #[test]
    fn list_code_bodies_match_the_recorded_requests() {
        const PAGE_1: &str =
            include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page1.meta.toml");
        const PAGE_2: &str =
            include_str!("../../tests/fixtures/recorded/list/code/m6/list-code-page2.meta.toml");
        assert_eq!(body(&list_code_body(1)), recorded(PAGE_1));
        assert_eq!(body(&list_code_body(2)), recorded(PAGE_2));
    }

    #[test]
    fn consultation_bodies_match_the_recorded_requests() {
        const BY_NUMBER: &str = include_str!(
            "../../tests/fixtures/recorded/consult/getArticleWithIdAndNum/m6/getArticleWithIdAndNum-code-civil-9.meta.toml"
        );
        const NOR: &str = include_str!(
            "../../tests/fixtures/recorded/consult/getJoWithNor/m6/getJoWithNor-AGRX2502679L.meta.toml"
        );
        const IDCC: &str = include_str!(
            "../../tests/fixtures/recorded/consult/kaliContIdcc/m6/kaliContIdcc-2121.meta.toml"
        );
        let civil = LegiTextId::parse("LEGITEXT000006070721").unwrap();
        assert_eq!(body(&article_by_number_body(&civil, "9")), recorded(BY_NUMBER));
        assert_eq!(body(&nor_body(&value("AGRX2502679L"))), recorded(NOR));
        assert_eq!(body(&idcc_body(&value("2121"))), recorded(IDCC));
    }

    #[test]
    fn the_exact_article_search_matches_the_recorded_request() {
        const META: &str = include_str!(
            "../../tests/fixtures/recorded/search/m6/search-exacte-num-article-9.meta.toml"
        );
        let query = query(Fond::Code, NumberKind::Article, "9", "2026-10-28");
        assert_eq!(sent(&query), recorded(META));
    }

    #[test]
    fn the_ecli_searches_match_the_recorded_requests() {
        const CASSATION: &str = include_str!(
            "../../tests/fixtures/recorded/search/m6/search-ecli-JURI-ECLI-FR-CCASS-2025-C100399.meta.toml"
        );
        const STATE: &str = include_str!(
            "../../tests/fixtures/recorded/search/m6/search-ecli-CETAT-ECLI-FR-CECHR-2026-506507.meta.toml"
        );
        const UNKNOWN: &str = include_str!(
            "../../tests/fixtures/recorded/search/m6/search-ecli-JURI-inconnu.meta.toml"
        );
        for (fond, ecli, meta) in [
            (Fond::Juri, "ECLI:FR:CCASS:2025:C100399", CASSATION),
            (Fond::Cetat, "ECLI:FR:CECHR:2026:506507.20260529", STATE),
            (Fond::Juri, "ECLI:FR:CCASS:1900:C000000", UNKNOWN),
        ] {
            assert_eq!(sent(&query(fond, NumberKind::Ecli, ecli, "2026-10-28")), recorded(meta));
        }
    }

    // ---- /search : un corps par couple (04 §6.9) -------------------------------------------

    /// Corps attendu d'une recherche exacte : filtres, pagination et taille donnés.
    fn exact(fond: &str, field: &str, text: &str, filtres: &Value, pagination: &str) -> Value {
        let size = if pagination == "ARTICLE" { 100 } else { 20 };
        json!({
            "fond": fond,
            "recherche": {
                "champs": [{
                    "typeChamp": field,
                    "criteres": [{"typeRecherche": "EXACTE", "valeur": text, "operateur": "ET"}],
                    "operateur": "ET",
                }],
                "filtres": filtres,
                "pageNumber": 1,
                "pageSize": size,
                "operateur": "ET",
                "sort": "PERTINENCE",
                "typePagination": pagination,
            },
        })
    }

    #[test]
    fn each_searchable_pair_has_its_field_and_pagination() {
        let version = json!([{"facette": "DATE_VERSION", "singleDate": 1_793_185_200_000_i64}]);
        let none = json!([]);
        let table = [
            (
                Fond::Code,
                NumberKind::Article,
                "L1242-2",
                "CODE_DATE",
                "NUM_ARTICLE",
                &version,
                "ARTICLE",
            ),
            (Fond::Loda, NumberKind::Article, "9", "LODA_DATE", "NUM_ARTICLE", &version, "ARTICLE"),
            (Fond::Loda, NumberKind::Text, "2016-1321", "LODA_DATE", "NUM", &version, "DEFAUT"),
            (Fond::Juri, NumberKind::Case, "24-13.470", "JURI", "NUM_AFFAIRE", &none, "DEFAUT"),
            (
                Fond::Constit,
                NumberKind::Decision,
                "2025-1173 QPC",
                "CONSTIT",
                "NUM_DEC",
                &none,
                "DEFAUT",
            ),
            (Fond::Cetat, NumberKind::Decision, "506507", "CETAT", "NUM_DEC", &none, "DEFAUT"),
            (
                Fond::Cnil,
                NumberKind::Deliberation,
                "SAN-2026-002",
                "CNIL",
                "NUM_DELIB",
                &none,
                "DEFAUT",
            ),
            (Fond::Kali, NumberKind::Idcc, "2121", "KALI", "IDCC", &none, "DEFAUT"),
            (Fond::Acco, NumberKind::Idcc, "2121", "ACCO", "IDCC", &none, "DEFAUT"),
        ];
        for (fond, kind, text, api, field, filtres, pagination) in table {
            let query = query(fond, kind, text, "2026-10-28");
            assert_eq!(
                sent(&query),
                exact(api, field, text, filtres, pagination),
                "{fond} {kind:?}"
            );
        }
    }

    #[test]
    fn the_page_number_is_sent() {
        let query = query(Fond::Code, NumberKind::Article, "9", "2026-10-28");
        let page_2 = body(&number_search_body(&query, 2).unwrap());
        assert_eq!(page_2["recherche"]["pageNumber"], 2);
    }

    #[test]
    fn a_period_uses_the_date_facet_of_the_fond_after_the_version_filter() {
        let range =
            DateRange::new(Some(day("2020-01-01")), Some(day("2020-12-31"))).unwrap().unwrap();
        let facets = [
            (Fond::Juri, NumberKind::Case, "DATE_DECISION"),
            (Fond::Constit, NumberKind::Decision, "DATE_DECISION"),
            (Fond::Cetat, NumberKind::Decision, "DATE_DECISION"),
            (Fond::Cnil, NumberKind::Deliberation, "DATE_DELIB"),
            (Fond::Kali, NumberKind::Idcc, "DATE_SIGNATURE"),
            (Fond::Acco, NumberKind::Idcc, "DATE_SIGNATURE"),
        ];
        for (fond, kind, facet) in facets {
            let query = query_in(fond, kind, range);
            assert_eq!(
                sent(&query)["recherche"]["filtres"],
                json!([{"facette": facet, "dates": {"start": "2020-01-01", "end": "2020-12-31"}}]),
                "{fond}"
            );
        }
        // LODA : version d'abord, puis la période.
        let loda = query_in(Fond::Loda, NumberKind::Text, range);
        assert_eq!(
            sent(&loda)["recherche"]["filtres"],
            json!([
                {"facette": "DATE_VERSION", "singleDate": 1_793_185_200_000_i64},
                {"facette": "DATE_SIGNATURE", "dates": {"start": "2020-01-01", "end": "2020-12-31"}},
            ])
        );
        // ECLI : version (aucune ici), puis l'ECLI, puis la période.
        let ecli = query_in(Fond::Juri, NumberKind::Ecli, range);
        assert_eq!(
            sent(&ecli)["recherche"]["filtres"],
            json!([
                {"facette": "ECLI", "valeurs": ["ECLI:FR:X"]},
                {"facette": "DATE_DECISION", "dates": {"start": "2020-01-01", "end": "2020-12-31"}},
            ])
        );
    }

    fn query_in(fond: Fond, kind: NumberKind, range: DateRange) -> NumberQuery {
        let raw = if kind == NumberKind::Ecli { "ECLI:FR:X" } else { "275" };
        query(fond, kind, raw, "2026-10-28").with_within(range)
    }

    #[test]
    fn the_ecli_value_is_the_exact_facet_and_the_query_is_a_star() {
        let sent =
            sent(&query(Fond::Cetat, NumberKind::Ecli, "ECLI:FR:CECHR:2026:1", "2026-10-28"));
        let search = &sent["recherche"];
        assert_eq!(sent["fond"], "CETAT");
        assert_eq!(
            search["champs"][0],
            json!({
                "typeChamp": "ALL",
                "criteres": [{"typeRecherche": "UN_DES_MOTS", "valeur": "*", "operateur": "ET"}],
                "operateur": "ET",
            })
        );
        assert_eq!(
            search["filtres"],
            json!([{"facette": "ECLI", "valeurs": ["ECLI:FR:CECHR:2026:1"]}])
        );
        assert_eq!(
            (&search["pageSize"], &search["typePagination"]),
            (&json!(10), &json!("DEFAUT"))
        );
    }

    #[test]
    fn page_size_follows_the_search_shape() {
        let sizes = NumberKind::ALL.map(page_size);
        assert_eq!(sizes, [100, 20, 20, 20, 20, 20, 20, 10, 20]);
    }

    // ---- gardes défensives ---------------------------------------------------------------

    #[test]
    fn kinds_that_are_consultations_are_not_searches() {
        for (fond, kind, raw) in [
            (Fond::Loda, NumberKind::Nor, "AGRX2502679L"),
            (Fond::Loda, NumberKind::Eli, "/eli/loi/2016/10/7/ECFI1524250L/jo/texte"),
        ] {
            assert_eq!(
                refused(&query(fond, kind, raw, "2026-10-28")),
                invalid(&format!(
                    "number kind {} is not searchable, it is a consultation",
                    kind.as_str()
                ))
            );
        }
    }

    #[test]
    fn a_hand_built_pair_outside_annex_b_is_refused() {
        let mut query = query(Fond::Juri, NumberKind::Case, "24-13.470", "2026-10-28");
        assert!(ensure_applicable(&query).is_ok());
        query.fond = Fond::Cnil;
        let expected = invalid("number kind case does not apply to fond CNIL");
        assert_eq!(refused(&query), expected);
        assert_eq!(ensure_applicable(&query).unwrap_err(), expected);
    }

    // ---- cadrage des résultats ---------------------------------------------------------------

    #[test]
    fn the_result_frame_carries_the_fond_size_dates_and_exact_matching() {
        let range = DateRange::new(Some(day("2020-01-01")), None).unwrap().unwrap();
        let article = query(Fond::Loda, NumberKind::Article, "9", "2026-10-28").with_within(range);
        let frame = result_frame(&article).unwrap();
        assert_eq!(
            (frame.fond(), frame.text().as_str(), frame.page().get(), frame.page_size().get()),
            (Fond::Loda, "9", 1, 100)
        );
        assert_eq!(
            (frame.mode(), frame.date_range(), frame.version_date()),
            (MatchMode::Exact, Some(range), Some(day("2026-10-28")))
        );
        let sizes: Vec<u32> = [
            (Fond::Juri, NumberKind::Case, "24-13.470"),
            (Fond::Juri, NumberKind::Ecli, "ECLI:FR:CCASS:2025:C100399"),
        ]
        .into_iter()
        .map(|(fond, kind, raw)| {
            result_frame(&query(fond, kind, raw, "2026-10-28")).unwrap().page_size().get()
        })
        .collect();
        assert_eq!(sizes, [20, 10]);
    }

    #[test]
    fn a_blank_value_has_no_frame_and_the_message_hides_it() {
        let blank = query(Fond::Juri, NumberKind::Case, "   ", "2026-10-28");
        assert_eq!(
            result_frame(&blank).unwrap_err(),
            invalid("number query refused: blank value or inconsistent dates")
        );
    }

    #[test]
    fn an_invalid_article_number_is_a_fixed_refusal() {
        assert_eq!(
            invalid_article_number(),
            CorpusError::InvalidRequest {
                status: None,
                detail: "article number is empty, too long or has forbidden characters".to_owned()
            }
        );
    }

    #[test]
    fn a_frame_refuses_dates_the_fond_cannot_filter_on() {
        let mut extra = query(Fond::Juri, NumberKind::Case, "24-13.470", "2026-10-28");
        extra.version_date = Some(day("2026-10-28"));
        assert!(result_frame(&extra).is_err());
        let range = DateRange::new(Some(day("2020-01-01")), None).unwrap().unwrap();
        let on_code = query(Fond::Code, NumberKind::Article, "9", "2026-10-28").with_within(range);
        assert!(result_frame(&on_code).is_err());
    }

    #[test]
    fn the_version_date_guards_mirror_the_search_body() {
        let mut missing = query(Fond::Code, NumberKind::Article, "9", "2026-10-28");
        missing.version_date = None;
        assert_eq!(refused(&missing), invalid("version_date missing for fond CODE"));
        let mut extra = query(Fond::Juri, NumberKind::Case, "24-13.470", "2026-10-28");
        extra.version_date = Some(day("2026-10-28"));
        assert_eq!(refused(&extra), invalid("version_date not applicable to fond JURI"));
    }

    #[test]
    fn a_period_on_code_is_refused_without_a_date_facet() {
        let range = DateRange::new(Some(day("2020-01-01")), None).unwrap().unwrap();
        let query = query(Fond::Code, NumberKind::Article, "9", "2026-10-28").with_within(range);
        assert_eq!(refused(&query), invalid("date range not applicable to fond CODE"));
    }

    #[test]
    fn a_refusal_never_names_the_number() {
        let mut query = query(Fond::Code, NumberKind::Article, "SECRET-NUMBER-1", "2026-10-28");
        query.version_date = None;
        assert!(!format!("{:?}", refused(&query)).contains("SECRET"));
    }

    // ---- ELI ---------------------------------------------------------------------------------

    #[test]
    fn an_eli_path_with_an_article_segment_is_an_article_eli() {
        let article = value("/eli/loi/2016/10/7/ECFI1524250L/jo/article_1");
        let text = value("/eli/loi/2016/10/7/ECFI1524250L/jo/texte");
        assert_eq!(EliTarget::of(&article), EliTarget::Article);
        assert_eq!(EliTarget::of(&text), EliTarget::Text);
        assert_eq!(EliTarget::Article.endpoint(), "/consult/getArticleWithIdEliOrAlias");
        assert_eq!(EliTarget::Text.endpoint(), "/consult/eliAndAliasRedirectionTexte");
        assert_eq!(
            body(&eli_body(&article)),
            json!({"idEliOrAlias": "/eli/loi/2016/10/7/ECFI1524250L/jo/article_1"})
        );
    }
}
