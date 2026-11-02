//! `impl LegalCorpus for PisteCorpus` : les douze opérations du port (04 §1.3, §7.1, §12.3). Les
//! cinq de M6 et M7 délèguent à `corpus::m6` et `corpus::m7`.
//!
//! L'identifiant d'une consultation va dans le **corps** de la requête (`request::consult`), jamais
//! dans le chemin : le gabarit d'endpoint est constant, ne dépend que de l'opération, et aucune
//! URL lue dans une réponse (`lienJo`, `urlCC`, `url`, `attachementUrl`, `href`) n'est appelée. Le
//! décodage sous budget et le mapping s'exécutent dans `PisteCorpus::fetch`, donc sous le permis de
//! concurrence.
//!
//! `search` lit le mémo de `typePagination` **avant** l'envoi (`PaginationMemo::mode_for`, qui
//! alimente `search_body` et sa garde défensive, avant toute requête, jeton compris) et ne retient
//! le mode reçu qu'**après** le mapping : sur `Err`, rien n'est retenu (R12). Une page de taille 1
//! est demandée en taille 2 (PISTE ignore `pageNumber` à `pageSize` 1, L3-05) : la réponse est
//! coupée à son créneau (`SearchResponseDto::keep_window`) avant le mapping. Un CODE répondu en
//! `ARTICLE` à une page demandée en `DEFAUT` est relu une fois en `ARTICLE` (`reread_in_article`).

use lf_application::ports::{ArticleAt, CorpusError, LegalCorpus};
use lf_domain::date::VersionDate;
use lf_domain::entity::{
    Act, ArticleText, ArticleVersion, CodeListing, CompanyAgreement, Convention, Decision,
    Deliberation, RelatedLinks, SearchHit, SearchPage,
};
use lf_domain::fond::{Fond, PaginationMode};
use lf_domain::id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId};
use lf_domain::search::{NumberQuery, SearchQuery};
use serde::de::DeserializeOwned;

use super::PisteCorpus;
use crate::classify::EndpointKind;
use crate::dto::search::SearchResponseDto;
use crate::map::endpoint;
use crate::map::search::search_page_with_mode;
use crate::map::{Diagnostics, act, agreement, article, convention, decision, deliberation};
use crate::request::consult::Consultation;
use crate::request::search::{search_body, wire_page_for};
use crate::time::{MonoClock, Sleeper};

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Une consultation : `fetch(EndpointKind::Consult, consultation.endpoint(),
    /// &consultation.body(), map)`.
    async fn consult<D: DeserializeOwned, T>(
        &self,
        consultation: Consultation<'_>,
        map: impl FnOnce(D, &mut Diagnostics) -> Result<T, CorpusError>,
    ) -> Result<T, CorpusError> {
        self.fetch(EndpointKind::Consult, consultation.endpoint(), &consultation.body(), map).await
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// Une recherche envoyée en `mode`, et le mode reçu en réponse.
    async fn search_in(
        &self,
        query: &SearchQuery,
        mode: PaginationMode,
    ) -> Result<(SearchPage, Option<PaginationMode>), CorpusError> {
        let body = search_body(query, mode)?;
        // Une page de taille 1 est demandée en taille 2 (L3-05) : on ne garde que son créneau.
        let wire = wire_page_for(query.fond(), mode, query.page().get(), query.page_size().get());
        self.fetch(
            EndpointKind::Search,
            endpoint::SEARCH,
            &body,
            |mut dto: SearchResponseDto, diags| {
                dto.keep_window(
                    endpoint::SEARCH,
                    wire.page_size,
                    wire.skip,
                    query.page_size().get(),
                )?;
                search_page_with_mode(dto, query, diags)
            },
        )
        .await
    }
}

impl<K: MonoClock + Clone, S: Sleeper + Clone> LegalCorpus for PisteCorpus<K, S> {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        let memo = &self.inner.memo;
        let mode = memo.mode_for(query);
        let (page, received) = self.search_in(query, mode).await?;
        memo.remember(query, received);
        // CODE, mode non mémorisé, envoyé en `DEFAUT` mais répondu en `ARTICLE` (PISTE pagine par
        // article : un seul code, beaucoup d'articles). Deux cas, relus une fois en `ARTICLE` :
        // - `page_size` 1 : la requête a été doublée (L3-05) ; la page reçue (deux articles, coupée
        //   à un hit) n'est pas celle qui est demandée et la page suivante redonnerait un article
        //   déjà rendu ; la relecture est rendue telle quelle ;
        // - page > 1 vide alors que l'amont annonce des résultats (gap3-1 : le mémo est vide, la
        //   page de textes demandée en `DEFAUT` n'existe pas) ; la relecture n'est rendue que si
        //   elle porte un article, sinon la page hors de la fin des articles reste vide.
        let doubled =
            wire_page_for(query.fond(), mode, query.page().get(), query.page_size().get())
                .page_size
                != query.page_size().get();
        if reread_in_article(
            query.fond(),
            mode,
            received,
            query.page().get(),
            doubled,
            page.hits.is_empty(),
            page.total,
        ) {
            let (again, again_received) = self.search_in(query, PaginationMode::Article).await?;
            memo.remember(query, again_received);
            if doubled || again.hits.iter().any(|hit| !hit.articles.is_empty()) {
                return Ok(again);
            }
        }
        Ok(page)
    }

    /// Un code entier (Code du travail : 31,4 Mio contre une borne de 16 Mio, V-96, 04 §7.3) ne
    /// tient pas dans `get_act` en v0.1 : la lecture bornée rend [`CorpusError::TooLarge`],
    /// détecté par la conséquence, sans reconnaître les codes à l'avance. La consigne vers
    /// `get_article` ou la recherche est rédigée par `lf-application::messages`, d'après la
    /// variante.
    async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
        self.consult(Consultation::LawDecree(id, date), |dto, diags| act::act(dto, id, diags)).await
    }

    async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
        self.consult(Consultation::Article(id), |dto, diags| article::article(dto, id, diags)).await
    }

    async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
        self.consult(Consultation::Decision(id), |dto, diags| decision::decision(dto, id, diags))
            .await
    }

    async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
        self.consult(Consultation::Deliberation(id), |dto, diags| {
            deliberation::deliberation(dto, id, diags)
        })
        .await
    }

    async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
        self.consult(Consultation::Convention(id), |dto, diags| {
            convention::convention(dto, id, diags)
        })
        .await
    }

    async fn get_company_agreement(
        &self,
        id: &AccoTextId,
    ) -> Result<CompanyAgreement, CorpusError> {
        self.consult(Consultation::Agreement(id), |dto, diags| agreement::agreement(dto, id, diags))
            .await
    }

    async fn find_article(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        self.find_article_impl(text, num, at).await
    }

    async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        self.find_by_number_impl(query).await
    }

    async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        self.list_codes_impl().await
    }

    async fn article_versions(&self, id: &ArticleId) -> Result<Vec<ArticleVersion>, CorpusError> {
        self.article_versions_impl(id).await
    }

    async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
        self.related_links_impl(id).await
    }
}

/// Faut-il relire la page en `ARTICLE` (04 §6.6) ? Seul un CODE envoyé en `DEFAUT` et répondu en
/// `ARTICLE` y prétend : soit la requête a été doublée (L3-05, `page_size` 1), soit c'est une page
/// > 1 vide alors que l'amont annonce des résultats.
fn reread_in_article(
    fond: Fond,
    sent: PaginationMode,
    received: Option<PaginationMode>,
    page_number: u32,
    doubled: bool,
    hits_empty: bool,
    total: u64,
) -> bool {
    fond == Fond::Code
        && sent == PaginationMode::Defaut
        && received == Some(PaginationMode::Article)
        && (doubled || (page_number > 1 && hits_empty && total > 0))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::time::Duration;

    use lf_application::ports::{CorpusError, LegalCorpus};
    use lf_domain::date::{DateRange, VersionDate};
    use lf_domain::entity::SearchPage;
    use lf_domain::fond::{Fond, PaginationMode};
    use lf_domain::id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId};
    use lf_domain::search::{MatchMode, PageSize, QueryText, SearchQuery};
    use serde_json::{Value, json};
    use wiremock::matchers::{body_json, body_partial_json, header, method, path, path_regex};
    use wiremock::{Mock, ResponseTemplate};

    use crate::corpus::tests::{API_BASE, Rig, TOKEN_PATH, never, requests_to, token_mock};
    use crate::map::Diagnostics;
    use crate::map::endpoint;
    use crate::map::fixtures::{Mapped, decoded, fixture, map_body, text as fixture_text};
    use crate::memo::MemoKey;
    use crate::request::search::search_body;
    use crate::stats::StatsSnapshot;
    use crate::testing::capture_at;
    use crate::view::view;

    /// Midi Europe/Paris du 15 octobre 2026, en millisecondes (UTC+2 : 10 h UTC).
    const NOON_2026_10_15: i64 = 1_792_058_400_000;

    fn version_date() -> VersionDate {
        VersionDate::parse("2026-10-15").unwrap()
    }

    /// La fixture `name`, servie telle quelle en 200.
    fn fixture_reply(name: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(fixture(name).bytes(), "application/json")
    }

    /// Le jeton une fois, et la fixture `name` une fois, à `endpoint` pour le seul corps `request`.
    async fn serve(rig: &Rig, endpoint: &str, request: Value, name: &str) {
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .and(body_json(request))
            .respond_with(fixture_reply(name))
            .up_to_n_times(1)
            .expect(1)
            .mount(&rig.server)
            .await;
    }

    /// Un `Rig` ordinaire qui sert la fixture `name` pour `request`.
    async fn served(endpoint: &str, request: Value, name: &str) -> Rig {
        let rig = Rig::new().await;
        serve(&rig, endpoint, request, name).await;
        rig
    }

    /// Vue attendue de la fixture `name` demandée sous `requested`, par le chemin de production.
    fn expected(name: &str, requested: &str) -> Value {
        let fixture = fixture(name);
        let mapped = map_body(
            fixture.kind,
            &decoded(fixture, true).text,
            requested,
            &mut Diagnostics::default(),
        )
        .unwrap();
        view(&mapped)
    }

    /// Compteurs attendus après la fixture `name` : le mojibake du corps, `id_mismatch` donné.
    fn stats_after(name: &str, id_mismatch: u64) -> StatsSnapshot {
        let body = decoded(fixture(name), true);
        StatsSnapshot {
            mojibake_repairs: u64::from(body.repaired),
            mojibake_unrepaired: u64::from(body.detected && !body.repaired),
            id_mismatch,
            ..StatsSnapshot::default()
        }
    }

    /// Requête « golden » : texte `golden`, page 1, taille et mode par défaut, sans plage ;
    /// `version_date` pour LODA et CODE seulement.
    fn golden(fond: Fond) -> SearchQuery {
        let version = matches!(fond, Fond::Loda | Fond::Code).then(version_date);
        SearchQuery::new(
            fond,
            QueryText::parse("golden").unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            version,
        )
        .unwrap()
    }

    /// Corps de la première requête reçue sur `endpoint`.
    async fn sent_body(rig: &Rig, endpoint: &str) -> Value {
        let route = format!("{API_BASE}{endpoint}");
        let requests = rig.server.received_requests().await.unwrap();
        let first = requests.iter().find(|request| request.url.path() == route).unwrap();
        serde_json::from_slice(&first.body).unwrap()
    }

    /// Une recherche « golden » sur `fond` contre la fixture `name`.
    async fn check_search(name: &str, fond: Fond, api: &str, filtres: Value) {
        let query = golden(fond);
        let request: Value =
            serde_json::from_slice(&search_body(&query, PaginationMode::Defaut).unwrap()).unwrap();
        let rig = served(endpoint::SEARCH, request, name).await;
        let page = rig.corpus.search(&query).await.unwrap();
        assert_eq!(view(&Mapped::Search(page)), expected(name, ""));
        let sent = sent_body(&rig, endpoint::SEARCH).await;
        let recherche = &sent["recherche"];
        let criteria = &recherche["champs"][0]["criteres"][0];
        assert_eq!(sent["fond"], api);
        assert_eq!(criteria["typeRecherche"], "UN_DES_MOTS");
        assert_eq!(criteria["valeur"], "golden");
        assert_eq!(recherche["filtres"], filtres);
        assert_eq!(
            (&recherche["pageNumber"], &recherche["pageSize"], &recherche["typePagination"]),
            (&json!(1), &json!(10), &json!("DEFAUT"))
        );
        assert_eq!(rig.corpus.stats(), stats_after(name, 0));
    }

    fn version_filter() -> Value {
        json!([{"facette": "DATE_VERSION", "singleDate": NOON_2026_10_15}])
    }

    // ---- /search : une fixture par fond ----------------------------------------------------

    #[tokio::test]
    async fn search_loda_fixture_maps() {
        check_search("search_loda", Fond::Loda, "LODA_DATE", version_filter()).await;
    }

    #[tokio::test]
    async fn search_code_fixture_maps() {
        check_search("search_code", Fond::Code, "CODE_DATE", version_filter()).await;
    }

    #[tokio::test]
    async fn search_juri_fixture_maps() {
        check_search("search_juri", Fond::Juri, "JURI", json!([])).await;
    }

    #[tokio::test]
    async fn search_constit_fixture_maps() {
        check_search("search_constit", Fond::Constit, "CONSTIT", json!([])).await;
    }

    #[tokio::test]
    async fn search_cetat_fixture_maps() {
        check_search("search_cetat", Fond::Cetat, "CETAT", json!([])).await;
    }

    #[tokio::test]
    async fn search_cnil_fixture_maps() {
        check_search("search_cnil", Fond::Cnil, "CNIL", json!([])).await;
    }

    #[tokio::test]
    async fn search_kali_fixture_maps() {
        check_search("search_kali", Fond::Kali, "KALI", json!([])).await;
    }

    #[tokio::test]
    async fn search_acco_fixture_maps() {
        check_search("search_acco", Fond::Acco, "ACCO", json!([])).await;
    }

    // ---- /consult : une fixture par route -------------------------------------------------

    #[tokio::test]
    async fn consult_law_decree_fixture_maps() {
        let name = "consult_loda";
        let requested = fixture(name).requested;
        let request = json!({"textId": "LEGITEXT000033205014", "date": "2026-10-15"});
        let rig = served(endpoint::LAW_DECREE, request, name).await;
        let id = LegiTextId::parse(requested).unwrap();
        let act = rig.corpus.get_act(&id, version_date()).await.unwrap();
        assert_eq!(view(&Mapped::Act(act)), expected(name, requested));
        assert_eq!(rig.corpus.stats(), stats_after(name, 0));
    }

    #[tokio::test]
    async fn consult_get_article_fixture_maps() {
        let name = "consult_article";
        for (requested, mismatches) in [(fixture(name).requested, 0), ("KALIARTI000049886059", 1)] {
            let rig = served(endpoint::GET_ARTICLE, json!({"id": requested}), name).await;
            let id = ArticleId::parse(requested).unwrap();
            let article = rig.corpus.get_article(&id).await.unwrap();
            assert_eq!(view(&Mapped::Article(article)), expected(name, requested));
            assert_eq!(rig.corpus.stats(), stats_after(name, mismatches));
        }
    }

    #[tokio::test]
    async fn consult_juri_fixture_maps() {
        for name in ["consult_juri", "consult_juri_constit", "consult_juri_cetat"] {
            let requested = fixture(name).requested;
            let rig = served(endpoint::JURI, json!({"textId": requested}), name).await;
            let id = DecisionId::parse(requested).unwrap();
            let decision = rig.corpus.get_decision(&id).await.unwrap();
            assert_eq!(view(&Mapped::Decision(decision)), expected(name, requested), "{name}");
            assert_eq!(rig.corpus.stats(), stats_after(name, 0), "{name}");
        }
    }

    #[tokio::test]
    async fn consult_cnil_fixture_maps() {
        let name = "consult_cnil";
        let requested = fixture(name).requested;
        let rig = served(endpoint::CNIL, json!({"textId": requested}), name).await;
        let id = CnilTextId::parse(requested).unwrap();
        let deliberation = rig.corpus.get_deliberation(&id).await.unwrap();
        assert_eq!(view(&Mapped::Deliberation(deliberation)), expected(name, requested));
        assert_eq!(rig.corpus.stats(), stats_after(name, 0));
    }

    #[tokio::test]
    async fn consult_kali_text_fixture_maps() {
        let name = "consult_kali";
        let requested = fixture(name).requested;
        let rig = served(endpoint::KALI_TEXT, json!({"id": requested}), name).await;
        let id = KaliTextId::parse(requested).unwrap();
        let convention = rig.corpus.get_convention(&id).await.unwrap();
        assert_eq!(view(&Mapped::Convention(convention)), expected(name, requested));
        assert_eq!(rig.corpus.stats(), stats_after(name, 0));
    }

    #[tokio::test]
    async fn consult_acco_fixture_maps() {
        let name = "consult_acco";
        let requested = fixture(name).requested;
        let rig = served(endpoint::ACCO, json!({"id": requested}), name).await;
        let id = AccoTextId::parse(requested).unwrap();
        let agreement = rig.corpus.get_company_agreement(&id).await.unwrap();
        assert_eq!(view(&Mapped::Agreement(agreement)), expected(name, requested));
        assert_eq!(rig.corpus.stats(), stats_after(name, 0));
    }

    // ---- /search par le port : pagination, mémo (R12, R13), facettes, gardes, page pleine ------

    /// Page vide, sans `typePagination`.
    const EMPTY_PAGE: &str = r#"{"results":[],"totalResultNumber":0}"#;

    /// Requête « golden » du fond, à la page `page` et avec la plage `range`.
    fn golden_page(fond: Fond, page: u32, range: Option<DateRange>) -> SearchQuery {
        let version = matches!(fond, Fond::Loda | Fond::Code).then(version_date);
        SearchQuery::new(
            fond,
            QueryText::parse("golden").unwrap(),
            NonZeroU32::new(page).unwrap(),
            PageSize::default(),
            MatchMode::default(),
            range,
            version,
        )
        .unwrap()
    }

    /// Le jeton une fois ; `/search` répond `body` (200) à volonté.
    async fn search_replies(rig: &Rig, body: &str) {
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::SEARCH)))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json"),
            )
            .mount(&rig.server)
            .await;
    }

    /// Corps de chaque requête `/search` reçue, dans l'ordre.
    async fn search_bodies(rig: &Rig) -> Vec<Value> {
        let route = format!("{API_BASE}{}", endpoint::SEARCH);
        let requests = rig.server.received_requests().await.unwrap();
        requests
            .iter()
            .filter(|request| request.url.path() == route)
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }

    /// `recherche.typePagination` de chaque requête `/search`, dans l'ordre.
    async fn sent_modes(rig: &Rig) -> Vec<Value> {
        search_bodies(rig)
            .await
            .iter()
            .map(|body| body["recherche"]["typePagination"].clone())
            .collect()
    }

    /// `count` résultats `JURITEXT{n:012}` titrés `t`, `totalResultNumber = count`.
    fn juri_results(count: usize) -> String {
        let results: Vec<Value> = (1..=count)
            .map(|n| json!({"titles": [{"id": format!("JURITEXT{n:012}"), "title": "t"}]}))
            .collect();
        json!({"results": results, "totalResultNumber": count}).to_string()
    }

    // ---- L3-05 (search-1) : `pageSize` 1 ignoré par PISTE, relevés C9 ----------------------

    /// Relevés C9 : `bail commercial`, pages de taille 2 (la page 1 est la 1 et la 2 du rang) ; les
    /// pages de taille 1 des mêmes relevés rendent toutes le rang 1 (le défaut amont).
    const JURI_PAGE_1: &str =
        include_str!("../../tests/fixtures/recorded/search/c101/C9-juri-taille2-page1.json");
    const JURI_PAGE_2: &str =
        include_str!("../../tests/fixtures/recorded/search/c101/C9-juri-taille2-page2.json");
    const CODE_PAGE_1: &str =
        include_str!("../../tests/fixtures/recorded/search/c101/C9-code-taille2-page1.json");
    const CODE_PAGE_2: &str =
        include_str!("../../tests/fixtures/recorded/search/c101/C9-code-taille2-page2.json");

    /// Requête « golden » du fond à la page `page`, de taille `size`.
    fn sized(fond: Fond, page: u32, size: u32) -> SearchQuery {
        let version = matches!(fond, Fond::Loda | Fond::Code).then(version_date);
        SearchQuery::new(
            fond,
            QueryText::parse("bail commercial").unwrap(),
            NonZeroU32::new(page).unwrap(),
            PageSize::new(size).unwrap(),
            MatchMode::default(),
            None,
            version,
        )
        .unwrap()
    }

    /// Le jeton une fois ; la page `wire_page` de `/search` répond `body` (200) à volonté.
    async fn page_replies(rig: &Rig, wire_page: u32, body: &str) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::SEARCH)))
            .and(body_partial_json(json!({"recherche": {"pageNumber": wire_page}})))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json"),
            )
            .mount(&rig.server)
            .await;
    }

    /// Identifiants des hits d'une page, dans l'ordre.
    fn ids(page: &SearchPage) -> Vec<&str> {
        page.hits.iter().map(|hit| hit.id.as_str()).collect()
    }

    /// `(pageNumber, pageSize)` de chaque requête `/search`, dans l'ordre.
    async fn sent_pages(rig: &Rig) -> Vec<(Value, Value)> {
        search_bodies(rig)
            .await
            .iter()
            .map(|body| {
                (body["recherche"]["pageNumber"].clone(), body["recherche"]["pageSize"].clone())
            })
            .collect()
    }

    #[tokio::test]
    async fn page_size_1_is_served_by_pages_of_two() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        page_replies(&rig, 1, JURI_PAGE_1).await;
        page_replies(&rig, 2, JURI_PAGE_2).await;
        let mut served = Vec::new();
        for page in 1..=4 {
            let result = rig.corpus.search(&sized(Fond::Juri, page, 1)).await.unwrap();
            assert_eq!(result.total, 152_062, "page {page}");
            served.push(ids(&result).join(","));
        }
        // Les rangs 1 à 4 des relevés de taille 2 : un hit par page, et pas deux fois le rang 1.
        assert_eq!(
            served,
            [
                "JURITEXT000041975710",
                "JURITEXT000054167256",
                "JURITEXT000041490554",
                "JURITEXT000035574682"
            ]
        );
        let pages = [1, 1, 2, 2].map(|number| (json!(number), json!(2)));
        assert_eq!(sent_pages(&rig).await, pages);
    }

    /// Dernière page impaire : la page de taille 2 ne porte qu'un résultat, la page de rang pair
    /// est vide, sans panique ; une page de taille 2 vide l'est aussi.
    #[tokio::test]
    async fn the_odd_last_page_of_size_1_leaves_the_second_slot_empty() {
        let lone = {
            let mut value: Value = serde_json::from_str(JURI_PAGE_2).unwrap();
            value["results"].as_array_mut().unwrap().truncate(1);
            value.to_string()
        };
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        page_replies(&rig, 2, &lone).await;
        let odd = rig.corpus.search(&sized(Fond::Juri, 3, 1)).await.unwrap();
        let even = rig.corpus.search(&sized(Fond::Juri, 4, 1)).await.unwrap();
        assert_eq!(ids(&odd), ["JURITEXT000041490554"]);
        assert_eq!(ids(&even), Vec::<&str>::new());
        assert_eq!(even.total, 152_062);
    }

    #[tokio::test]
    async fn a_page_of_size_1_that_overflows_the_wire_page_is_a_decode_error() {
        let three = {
            let mut value: Value = serde_json::from_str(JURI_PAGE_1).unwrap();
            let first = value["results"][0].clone();
            value["results"].as_array_mut().unwrap().push(first);
            value.to_string()
        };
        let rig = Rig::new().await;
        search_replies(&rig, &three).await;
        let outcome = rig.corpus.search(&sized(Fond::Juri, 1, 1)).await.map(drop);
        assert_eq!(
            outcome,
            Err(CorpusError::Decode {
                detail: "/search: results: more results than page size".to_owned()
            })
        );
    }

    /// Jumeau : les tailles 2 et 3 partent telles quelles et rendent la page entière.
    #[tokio::test]
    async fn page_sizes_2_and_3_are_sent_and_read_unchanged() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        page_replies(&rig, 2, JURI_PAGE_2).await;
        let two = rig.corpus.search(&sized(Fond::Juri, 2, 2)).await.unwrap();
        assert_eq!(ids(&two), ["JURITEXT000041490554", "JURITEXT000035574682"]);
        let three = rig.corpus.search(&sized(Fond::Juri, 2, 3)).await.unwrap();
        assert_eq!(ids(&three).len(), 2);
        assert_eq!(sent_pages(&rig).await, [(json!(2), json!(2)), (json!(2), json!(3))]);
    }

    /// CODE en `DEFAUT` : même correction (relevés C9-code-taille1 : la page 3 rend le hit de la
    /// page 1).
    #[tokio::test]
    async fn code_page_size_1_is_served_by_pages_of_two_in_defaut_mode() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        page_replies(&rig, 1, CODE_PAGE_1).await;
        page_replies(&rig, 2, CODE_PAGE_2).await;
        let mut served = Vec::new();
        for page in 1..=4 {
            let result = rig.corpus.search(&sized(Fond::Code, page, 1)).await.unwrap();
            served.push(ids(&result).join(","));
        }
        assert_eq!(
            served,
            [
                "LEGITEXT000005634379",
                "LEGITEXT000006074075",
                "LEGITEXT000006070299",
                "LEGITEXT000006072665"
            ]
        );
    }

    /// Réponse `body` à une requête de la page `wire_page` et de la taille `wire_size`.
    async fn sized_replies(rig: &Rig, wire_page: u32, wire_size: u32, body: &str) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::SEARCH)))
            .and(body_partial_json(
                json!({"recherche": {"pageNumber": wire_page, "pageSize": wire_size}}),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json"),
            )
            .mount(&rig.server)
            .await;
    }

    /// `body` d'une page CODE à un seul hit, répondue en mode `ARTICLE`.
    fn lone_article_page(body: &str) -> String {
        let mut value: Value = serde_json::from_str(body).unwrap();
        value["results"].as_array_mut().unwrap().truncate(1);
        value["typePagination"] = json!("ARTICLE");
        value.to_string()
    }

    /// CODE en mode `ARTICLE` (déjà mémorisé) : `pageSize` compte des articles ; la requête et la
    /// page lue restent inchangées, sans troncature.
    #[tokio::test]
    async fn code_page_size_1_in_article_mode_is_left_alone() {
        let lone = lone_article_page(CODE_PAGE_2);
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        sized_replies(&rig, 2, 1, &lone).await;
        rig.corpus.inner.memo.remember(&sized(Fond::Code, 1, 1), Some(PaginationMode::Article));
        // Requête inchangée (page 2, taille 1) ; doublée, elle aurait demandé la page 1 de
        // taille 2 et rendu un autre hit.
        let second = rig.corpus.search(&sized(Fond::Code, 2, 1)).await.unwrap();
        assert_eq!(ids(&second), ["LEGITEXT000006070299"]);
        assert_eq!(sent_pages(&rig).await, [(json!(2), json!(1))]);
        assert_eq!(sent_modes(&rig).await, [json!("ARTICLE")]);
    }

    /// CODE `page_size` 1, première requête (mode inconnu) doublée en `DEFAUT` mais répondue en
    /// `ARTICLE` : la page de deux articles n'est pas celle qui est demandée. Elle est relue en
    /// `ARTICLE`, non doublée, et ne rend qu'un article ; la page 2 le suit sans doublon.
    #[tokio::test]
    async fn code_page_size_1_first_page_answered_by_article_is_read_again() {
        let by_article = CODE_PAGE_1.replacen(
            r#""typePagination":"DEFAUT""#,
            r#""typePagination":"ARTICLE""#,
            1,
        );
        assert_ne!(by_article, CODE_PAGE_1);
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        sized_replies(&rig, 1, 2, &by_article).await;
        sized_replies(&rig, 1, 1, &lone_article_page(CODE_PAGE_1)).await;
        sized_replies(&rig, 2, 1, &lone_article_page(CODE_PAGE_2)).await;
        let first = rig.corpus.search(&sized(Fond::Code, 1, 1)).await.unwrap();
        let second = rig.corpus.search(&sized(Fond::Code, 2, 1)).await.unwrap();
        assert_eq!(ids(&first), ["LEGITEXT000005634379"]);
        assert_eq!(ids(&second), ["LEGITEXT000006070299"]);
        assert_eq!(
            sent_pages(&rig).await,
            [(json!(1), json!(2)), (json!(1), json!(1)), (json!(2), json!(1))]
        );
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("ARTICLE"), json!("ARTICLE")]);
    }

    /// L'échec de la relecture est rendu tel quel, sans page tronquée.
    #[tokio::test]
    async fn a_failing_second_read_of_a_code_page_is_an_error() {
        let by_article = CODE_PAGE_1.replacen(
            r#""typePagination":"DEFAUT""#,
            r#""typePagination":"ARTICLE""#,
            1,
        );
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        sized_replies(&rig, 1, 2, &by_article).await;
        assert!(rig.corpus.search(&sized(Fond::Code, 1, 1)).await.is_err());
        assert_eq!(sent_pages(&rig).await.first(), Some(&(json!(1), json!(2))));
    }

    /// Jumeau : une première page CODE `page_size` 1 répondue en `DEFAUT` n'est pas relue.
    #[tokio::test]
    async fn code_page_size_1_first_page_answered_by_defaut_is_not_read_again() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        sized_replies(&rig, 1, 2, CODE_PAGE_1).await;
        let first = rig.corpus.search(&sized(Fond::Code, 1, 1)).await.unwrap();
        assert_eq!(ids(&first), ["LEGITEXT000005634379"]);
        assert_eq!(sent_pages(&rig).await, [(json!(1), json!(2))]);
    }

    // ---- U09 (gap3-1) : page > 1 d'un CODE demandée à froid, relevés C12 (sandbox, 2026-11-01) --

    /// Relevés C12 : `émoluments notaire donation`, CODE, `TOUS_LES_MOTS_DANS_UN_CHAMP`, pages de
    /// taille 5. PISTE répond toujours `typePagination` `ARTICLE` (1 code, 16 articles) ; une page
    /// > 1 demandée en `DEFAUT` revient vide, la même page demandée en `ARTICLE` porte les articles.
    const C12_P1: &str = include_str!(
        "../../tests/fixtures/recorded/search/c101/C12-code-emoluments-page1-defaut.json"
    );
    const C12_P2_DEFAUT: &str = include_str!(
        "../../tests/fixtures/recorded/search/c101/C12-code-emoluments-page2-defaut.json"
    );
    const C12_P2_ARTICLE: &str = include_str!(
        "../../tests/fixtures/recorded/search/c101/C12-code-emoluments-page2-article.json"
    );
    const C12_P3_DEFAUT: &str = include_str!(
        "../../tests/fixtures/recorded/search/c101/C12-code-emoluments-page3-defaut.json"
    );
    const C12_P3_ARTICLE: &str = include_str!(
        "../../tests/fixtures/recorded/search/c101/C12-code-emoluments-page3-article.json"
    );
    const C12_P4_DEFAUT: &str = include_str!(
        "../../tests/fixtures/recorded/search/c101/C12-code-emoluments-page4-defaut.json"
    );
    const C12_P4_ARTICLE: &str = include_str!(
        "../../tests/fixtures/recorded/search/c101/C12-code-emoluments-page4-article.json"
    );

    const C12_P1_IDS: [&str; 5] = [
        "LEGIARTI000033254415",
        "LEGIARTI000041684292",
        "LEGIARTI000041684268",
        "LEGIARTI000041684289",
        "LEGIARTI000032132066",
    ];
    const C12_P2_IDS: [&str; 5] = [
        "LEGIARTI000041684274",
        "LEGIARTI000041684286",
        "LEGIARTI000041684283",
        "LEGIARTI000041684271",
        "LEGIARTI000041684265",
    ];
    /// Les deux extraits de la page 3 sans identifiant sont écartés par le mapping.
    const C12_P3_IDS: [&str; 5] = [
        "LEGIARTI000041684262",
        "LEGIARTI000041684280",
        "LEGIARTI000041684277",
        "LEGIARTI000041684614",
        "LEGIARTI000041684332",
    ];
    const C12_P4_IDS: [&str; 1] = ["LEGIARTI000048153997"];

    /// La requête de C12 à la page `page`, en pages de `size`.
    fn c12(page: u32, size: u32) -> SearchQuery {
        SearchQuery::new(
            Fond::Code,
            QueryText::parse("émoluments notaire donation").unwrap(),
            NonZeroU32::new(page).unwrap(),
            PageSize::new(size).unwrap(),
            MatchMode::All,
            None,
            Some(version_date()),
        )
        .unwrap()
    }

    /// Réponse `body` à la requête de la page `wire_page` envoyée en `mode`.
    async fn mode_replies(rig: &Rig, wire_page: u32, mode: &str, body: &str) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::SEARCH)))
            .and(body_partial_json(
                json!({"recherche": {"pageNumber": wire_page, "typePagination": mode}}),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json"),
            )
            .mount(&rig.server)
            .await;
    }

    /// Un `Rig` qui répond comme la sandbox aux relevés C12 : `DEFAUT` vide à partir de la page 2,
    /// `ARTICLE` porteur des articles.
    async fn c12_rig() -> Rig {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        mode_replies(&rig, 1, "DEFAUT", C12_P1).await;
        mode_replies(&rig, 2, "DEFAUT", C12_P2_DEFAUT).await;
        mode_replies(&rig, 2, "ARTICLE", C12_P2_ARTICLE).await;
        mode_replies(&rig, 3, "DEFAUT", C12_P3_DEFAUT).await;
        mode_replies(&rig, 3, "ARTICLE", C12_P3_ARTICLE).await;
        mode_replies(&rig, 4, "DEFAUT", C12_P4_DEFAUT).await;
        mode_replies(&rig, 4, "ARTICLE", C12_P4_ARTICLE).await;
        rig
    }

    /// Identifiants des articles du premier hit.
    fn article_ids(page: &SearchPage) -> Vec<&str> {
        page.hits
            .first()
            .map(|hit| hit.articles.iter().map(|article| article.article_id.as_str()).collect())
            .unwrap_or_default()
    }

    /// Une réponse `ARTICLE` d'une page hors de la fin des articles : le hit du code revient sans
    /// aucun extrait (C12 page 4, sections vidées), `totalArticleResultNumber` à `articles`.
    fn article_page_past_the_end(articles: u64) -> String {
        let mut value: Value = serde_json::from_str(C12_P4_ARTICLE).unwrap();
        value["results"][0]["sections"] = json!([]);
        value["totalArticleResultNumber"] = json!(articles);
        value.to_string()
    }

    /// V1 : page 2 en premier appel d'un processus neuf. PISTE répond à `DEFAUT` une page de textes
    /// vide ; la page est relue une fois en `ARTICLE`.
    #[tokio::test]
    async fn cold_code_page_2_answered_by_article_is_read_again_in_article() {
        let rig = c12_rig().await;
        let page = rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(article_ids(&page), C12_P2_IDS);
        assert_eq!((page.total, page.total_articles), (1, Some(16)));
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("ARTICLE")]);
        assert_eq!(sent_pages(&rig).await, [(json!(2), json!(5)), (json!(2), json!(5))]);
    }

    /// Suite de V1 : le mémo a appris `ARTICLE`, les pages suivantes partent directement ainsi, et
    /// les pages 1 à 4 réunissent les 16 articles du relevé.
    #[tokio::test]
    async fn after_the_reread_the_next_page_goes_straight_to_article() {
        let rig = c12_rig().await;
        let mut union: Vec<String> = Vec::new();
        let mut keep =
            |page: &SearchPage| union.extend(article_ids(page).into_iter().map(str::to_owned));
        let second = rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(article_ids(&second), C12_P2_IDS);
        keep(&second);
        let third = rig.corpus.search(&c12(3, 5)).await.unwrap();
        assert_eq!(article_ids(&third), C12_P3_IDS);
        keep(&third);
        let fourth = rig.corpus.search(&c12(4, 5)).await.unwrap();
        assert_eq!(article_ids(&fourth), C12_P4_IDS);
        keep(&fourth);
        let first = rig.corpus.search(&c12(1, 5)).await.unwrap();
        assert_eq!(article_ids(&first), C12_P1_IDS);
        keep(&first);
        assert_eq!(
            sent_modes(&rig).await,
            [
                json!("DEFAUT"),
                json!("ARTICLE"),
                json!("ARTICLE"),
                json!("ARTICLE"),
                json!("DEFAUT")
            ]
        );
        union.sort();
        union.dedup();
        assert_eq!(union.len(), 16);
    }

    /// V2 : la page 3 en premier appel rend les mêmes articles que la même page après un parcours
    /// à chaud.
    #[tokio::test]
    async fn a_cold_traversal_starting_at_page_3_yields_the_same_articles_as_the_warm_one() {
        let cold_rig = c12_rig().await;
        let cold = cold_rig.corpus.search(&c12(3, 5)).await.unwrap();
        let warm_rig = c12_rig().await;
        for page in [1, 2] {
            warm_rig.corpus.search(&c12(page, 5)).await.unwrap();
        }
        let warm = warm_rig.corpus.search(&c12(3, 5)).await.unwrap();
        assert_eq!(article_ids(&cold), C12_P3_IDS);
        assert_eq!(article_ids(&cold), article_ids(&warm));
        assert_eq!(cold.total_articles, warm.total_articles);
    }

    /// V3 : `page_size` est dans la clé du mémo ; chaque taille nouvelle repart à froid et relit
    /// une fois, puis la même clé part directement en `ARTICLE`.
    #[tokio::test]
    async fn a_page_size_change_between_pages_rereads_once_per_key() {
        let rig = c12_rig().await;
        rig.corpus.search(&c12(1, 10)).await.unwrap();
        let second = rig.corpus.search(&c12(2, 5)).await.unwrap();
        let third = rig.corpus.search(&c12(3, 20)).await.unwrap();
        assert_eq!(article_ids(&second), C12_P2_IDS);
        assert_eq!(article_ids(&third), C12_P3_IDS);
        assert_eq!(
            sent_modes(&rig).await,
            [json!("DEFAUT"), json!("DEFAUT"), json!("ARTICLE"), json!("DEFAUT"), json!("ARTICLE")]
        );
        // La clé (page_size 5) est apprise : la même page repart directement en ARTICLE.
        rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(sent_modes(&rig).await.last(), Some(&json!("ARTICLE")));
        assert_eq!(sent_modes(&rig).await.len(), 6);
    }

    /// V6 : un autre champ de la clé entre la page 1 et la page 2 (correspondance, date de version) donne une clé neuve : la page 2 est à froid et relue une fois.
    #[tokio::test]
    async fn a_cold_code_page_changes_nothing_when_another_key_field_differs() {
        let other_match = |mode| {
            SearchQuery::new(
                Fond::Code,
                QueryText::parse("émoluments notaire donation").unwrap(),
                NonZeroU32::new(2).unwrap(),
                PageSize::new(5).unwrap(),
                mode,
                None,
                Some(version_date()),
            )
            .unwrap()
        };
        let other_day = SearchQuery::new(
            Fond::Code,
            QueryText::parse("émoluments notaire donation").unwrap(),
            NonZeroU32::new(2).unwrap(),
            PageSize::new(5).unwrap(),
            MatchMode::All,
            None,
            Some(VersionDate::parse("2026-10-16").unwrap()),
        )
        .unwrap();
        for second in [other_match(MatchMode::Any), other_day] {
            let rig = c12_rig().await;
            rig.corpus.search(&c12(1, 5)).await.unwrap();
            let page = rig.corpus.search(&second).await.unwrap();
            assert_eq!(article_ids(&page), C12_P2_IDS);
            assert_eq!(
                sent_modes(&rig).await,
                [json!("DEFAUT"), json!("DEFAUT"), json!("ARTICLE")]
            );
        }
    }

    /// Table pure de la décision de relire en `ARTICLE`.
    #[test]
    fn reread_in_article_table() {
        use PaginationMode::{Article, Defaut};
        // (fond, envoyé, reçu, page, doublé, hits vides, total) -> relire ?
        let table = [
            // Page > 1 vide d'un CODE répondu en ARTICLE avec des résultats : relire.
            (Fond::Code, Defaut, Some(Article), 2, false, true, 1, true),
            (Fond::Code, Defaut, Some(Article), 7, false, true, 40, true),
            // Requête doublée (L3-05) : relire, quel que soit le contenu.
            (Fond::Code, Defaut, Some(Article), 1, true, false, 1, true),
            (Fond::Code, Defaut, Some(Article), 2, true, true, 0, true),
            // Page 1 : jamais, sauf doublée.
            (Fond::Code, Defaut, Some(Article), 1, false, true, 1, false),
            // Page non vide, ou sans résultat annoncé.
            (Fond::Code, Defaut, Some(Article), 2, false, false, 1, false),
            (Fond::Code, Defaut, Some(Article), 2, false, true, 0, false),
            // Mode reçu DEFAUT ou absent.
            (Fond::Code, Defaut, Some(Defaut), 2, false, true, 1, false),
            (Fond::Code, Defaut, None, 2, false, true, 1, false),
            // Autres fonds.
            (Fond::Juri, Defaut, Some(Article), 2, false, true, 1, false),
            (Fond::Loda, Defaut, Some(Article), 2, false, true, 1, false),
            (Fond::Kali, Defaut, Some(Article), 2, true, true, 1, false),
            // Déjà envoyé en ARTICLE : pas de boucle.
            (Fond::Code, Article, Some(Article), 2, false, true, 1, false),
            (Fond::Code, Article, Some(Article), 1, true, true, 1, false),
        ];
        for (fond, sent, received, page, doubled, empty, total, expected) in table {
            assert_eq!(
                super::reread_in_article(fond, sent, received, page, doubled, empty, total),
                expected,
                "{fond:?} {sent:?} {received:?} page {page} doubled {doubled} empty {empty} \
                 total {total}"
            );
        }
    }

    /// Plusieurs hits à la relecture, un seul porte des articles : elle est rendue (tue `any` vers
    /// `all`).
    #[tokio::test]
    async fn the_reread_is_kept_when_at_least_one_hit_carries_an_article() {
        let mixed = {
            let mut value: Value = serde_json::from_str(C12_P2_ARTICLE).unwrap();
            let mut bare = value["results"][0].clone();
            bare["titles"][0]["id"] = json!("LEGITEXT000006070299");
            bare["sections"] = json!([]);
            value["results"].as_array_mut().unwrap().insert(0, bare);
            value["totalResultNumber"] = json!(2);
            value.to_string()
        };
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        mode_replies(
            &rig,
            2,
            "DEFAUT",
            &C12_P2_DEFAUT.replacen(r#""totalResultNumber":1"#, r#""totalResultNumber":2"#, 1),
        )
        .await;
        mode_replies(&rig, 2, "ARTICLE", &mixed).await;
        let page = rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(page.hits.len(), 2);
        assert_eq!(page.hits[1].articles.len(), 5);
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("ARTICLE")]);
    }

    /// La relecture ne change que `typePagination` (même page, même taille, mêmes filtres).
    #[tokio::test]
    async fn the_reread_differs_from_the_first_request_only_by_the_type_pagination() {
        let rig = c12_rig().await;
        rig.corpus.search(&c12(2, 5)).await.unwrap();
        let mut bodies = search_bodies(&rig).await;
        assert_eq!(bodies.len(), 2);
        for body in &mut bodies {
            body["recherche"].as_object_mut().unwrap().remove("typePagination");
        }
        assert_eq!(bodies[0], bodies[1]);
    }

    /// L'échec de la relecture est rendu tel quel (avant : une page vide muette) ; le mémo a déjà
    /// appris `ARTICLE`, le nouvel essai part directement en `ARTICLE` et réussit.
    #[tokio::test]
    async fn a_failing_reread_of_a_cold_empty_code_page_is_an_error() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        mode_replies(&rig, 2, "DEFAUT", C12_P2_DEFAUT).await;
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::SEARCH)))
            .and(body_partial_json(json!({"recherche": {"typePagination": "ARTICLE"}})))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(3)
            .mount(&rig.server)
            .await;
        mode_replies(&rig, 2, "ARTICLE", C12_P2_ARTICLE).await;
        assert!(rig.corpus.search(&c12(2, 5)).await.is_err());
        let retried = rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(article_ids(&retried), C12_P2_IDS);
        let modes = sent_modes(&rig).await;
        assert_eq!(modes.first(), Some(&json!("DEFAUT")));
        assert_eq!(modes.iter().filter(|mode| **mode == json!("DEFAUT")).count(), 1);
        assert_eq!(modes.last(), Some(&json!("ARTICLE")));
    }

    /// V8 : page hors de la fin des articles, à froid. La relecture ne porte aucun article : elle
    /// n'est pas rendue, la page vide de la première réponse l'est (comme en 1.1.0), et la page a
    /// été relue exactement une fois en `ARTICLE`.
    #[tokio::test]
    async fn cold_out_of_range_code_page_is_read_again_once_in_article() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        mode_replies(&rig, 5, "DEFAUT", C12_P3_DEFAUT).await;
        mode_replies(&rig, 5, "ARTICLE", &article_page_past_the_end(16)).await;
        let page = rig.corpus.search(&c12(5, 5)).await.unwrap();
        assert_eq!(ids(&page), Vec::<&str>::new());
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("ARTICLE")]);
    }

    /// Garde de la même page hors fin : vide et mémo appris, sans compter les requêtes.
    #[tokio::test]
    async fn cold_out_of_range_code_page_stays_empty() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        mode_replies(&rig, 5, "DEFAUT", C12_P3_DEFAUT).await;
        mode_replies(&rig, 5, "ARTICLE", &article_page_past_the_end(16)).await;
        let query = c12(5, 5);
        let page = rig.corpus.search(&query).await.unwrap();
        assert_eq!(ids(&page), Vec::<&str>::new());
        assert_eq!((page.total, page.total_articles), (1, None));
        let learned = rig.corpus.inner.memo.lookup(&MemoKey::from_query(&query));
        assert_eq!(learned, Some(PaginationMode::Article));
    }

    /// Production du 2026-11-02 (« notoriété acquisitive tarif », page 3 de taille 2, 3 articles) :
    /// à froid, une page au-delà des articles reste vide, `total` 1, sans `total_articles`.
    #[tokio::test]
    async fn cold_code_page_beyond_the_articles_matches_production_notoriete() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        mode_replies(&rig, 3, "DEFAUT", C12_P3_DEFAUT).await;
        mode_replies(&rig, 3, "ARTICLE", &article_page_past_the_end(3)).await;
        let page = rig.corpus.search(&c12(3, 2)).await.unwrap();
        assert_eq!(ids(&page), Vec::<&str>::new());
        assert_eq!((page.total, page.total_articles), (1, None));
    }

    /// V-82 : un CODE répondu en `DEFAUT` (page vide, résultats annoncés) n'est pas relu.
    #[tokio::test]
    async fn a_cold_empty_code_page_answered_by_defaut_is_not_read_again() {
        let by_defaut = C12_P2_DEFAUT.replacen(
            r#""typePagination":"ARTICLE""#,
            r#""typePagination":"DEFAUT""#,
            1,
        );
        assert_ne!(by_defaut, C12_P2_DEFAUT);
        let rig = Rig::new().await;
        search_replies(&rig, &by_defaut).await;
        let page = rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(ids(&page), Vec::<&str>::new());
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// Hors CODE, `ARTICLE` n'existe pas : une page vide répondue ainsi n'est pas relue.
    #[tokio::test]
    async fn a_cold_empty_non_code_page_answered_by_article_is_not_read_again() {
        let rig = Rig::new().await;
        search_replies(&rig, r#"{"results":[],"totalResultNumber":1,"typePagination":"ARTICLE"}"#)
            .await;
        rig.corpus.search(&golden_page(Fond::Juri, 2, None)).await.unwrap();
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// Mémo déjà `ARTICLE` : une page vide répondue en `ARTICLE` n'est pas relue (pas de boucle).
    #[tokio::test]
    async fn a_memoized_article_empty_page_is_not_read_again() {
        let rig = Rig::new().await;
        search_replies(&rig, C12_P3_DEFAUT).await;
        rig.corpus.inner.memo.remember(&c12(3, 5), Some(PaginationMode::Article));
        let page = rig.corpus.search(&c12(3, 5)).await.unwrap();
        assert_eq!(ids(&page), Vec::<&str>::new());
        assert_eq!(sent_modes(&rig).await, [json!("ARTICLE")]);
    }

    /// Sans résultat annoncé (`totalResultNumber` 0), aucun article n'existe : pas de relecture.
    #[tokio::test]
    async fn a_cold_empty_code_page_with_a_total_of_zero_answered_by_article_is_not_read_again() {
        let none =
            C12_P2_DEFAUT.replacen(r#""totalResultNumber":1"#, r#""totalResultNumber":0"#, 1);
        assert_ne!(none, C12_P2_DEFAUT);
        let rig = Rig::new().await;
        search_replies(&rig, &none).await;
        rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// Sans `typePagination` dans la réponse, rien n'est appris et rien n'est relu.
    #[tokio::test]
    async fn a_cold_empty_code_page_without_type_pagination_is_not_read_again() {
        let bare = C12_P2_DEFAUT.replacen(r#""typePagination":"ARTICLE","#, "", 1);
        assert_ne!(bare, C12_P2_DEFAUT);
        let rig = Rig::new().await;
        search_replies(&rig, &bare).await;
        rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// Page 1 vide d'un CODE répondue en `ARTICLE` : jamais relue (la condition est `page > 1`).
    #[tokio::test]
    async fn a_cold_empty_code_page_one_is_never_read_again() {
        let rig = Rig::new().await;
        search_replies(&rig, C12_P2_DEFAUT).await;
        let page = rig.corpus.search(&c12(1, 5)).await.unwrap();
        assert_eq!(ids(&page), Vec::<&str>::new());
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// Une page > 1 non vide à froid, même répondue en `ARTICLE`, n'est pas relue.
    #[tokio::test]
    async fn a_cold_non_empty_code_page_answered_by_article_is_not_read_again() {
        let rig = Rig::new().await;
        search_replies(&rig, C12_P2_ARTICLE).await;
        let page = rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(article_ids(&page), C12_P2_IDS);
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// Plusieurs codes (mode `DEFAUT` reçu) : la page 2 vide ou non est servie en une requête.
    #[tokio::test]
    async fn a_cold_code_page_two_with_several_codes_sends_one_request() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        page_replies(&rig, 2, CODE_PAGE_2).await;
        let page = rig.corpus.search(&sized(Fond::Code, 2, 2)).await.unwrap();
        assert_eq!(ids(&page), ["LEGITEXT000006070299", "LEGITEXT000006072665"]);
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// La relecture encore vide (page hors fin des deux côtés) est rendue sans troisième appel.
    #[tokio::test]
    async fn the_reread_that_is_still_empty_is_returned_without_a_third_call() {
        let rig = Rig::new().await;
        search_replies(&rig, C12_P2_DEFAUT).await;
        let page = rig.corpus.search(&c12(2, 5)).await.unwrap();
        assert_eq!(ids(&page), Vec::<&str>::new());
        assert!(sent_modes(&rig).await.len() <= 2);
    }

    /// `page_size` 1 à froid, page 2 : le doublement puis la relecture, jamais une troisième
    /// requête.
    #[tokio::test]
    async fn cold_code_page_size_1_is_read_again_once_only() {
        let by_article = CODE_PAGE_1.replacen(
            r#""typePagination":"DEFAUT""#,
            r#""typePagination":"ARTICLE""#,
            1,
        );
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        sized_replies(&rig, 1, 2, &by_article).await;
        sized_replies(&rig, 2, 1, &lone_article_page(CODE_PAGE_2)).await;
        let page = rig.corpus.search(&sized(Fond::Code, 2, 1)).await.unwrap();
        assert_eq!(ids(&page), ["LEGITEXT000006070299"]);
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("ARTICLE")]);
    }

    /// Branche « doublée » inchangée : la relecture est rendue même sans article.
    #[tokio::test]
    async fn the_doubled_branch_still_returns_a_reread_without_article() {
        let by_article = CODE_PAGE_1.replacen(
            r#""typePagination":"DEFAUT""#,
            r#""typePagination":"ARTICLE""#,
            1,
        );
        let bare = {
            let mut value: Value = serde_json::from_str(&lone_article_page(CODE_PAGE_1)).unwrap();
            value["results"][0]["sections"] = json!([]);
            value.to_string()
        };
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        sized_replies(&rig, 1, 2, &by_article).await;
        sized_replies(&rig, 1, 1, &bare).await;
        let page = rig.corpus.search(&sized(Fond::Code, 1, 1)).await.unwrap();
        assert_eq!(ids(&page), ["LEGITEXT000005634379"]);
        assert_eq!(article_ids(&page), Vec::<&str>::new());
    }

    /// Limite connue n° 1 (garantie 14, N-1) : `page_size` 1 en mode `ARTICLE` part comme en 1.1.0
    /// (la page demandée telle quelle, sans doublement ni troncature), à chaud comme à froid.
    #[tokio::test]
    async fn code_article_page_of_one_keeps_the_1_1_0_requests() {
        let lone = lone_article_page(CODE_PAGE_2);
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        page_replies(&rig, 1, &lone).await;
        page_replies(&rig, 2, &lone).await;
        page_replies(&rig, 3, &lone).await;
        rig.corpus.inner.memo.remember(&sized(Fond::Code, 1, 1), Some(PaginationMode::Article));
        for page in 2..=3 {
            rig.corpus.search(&sized(Fond::Code, page, 1)).await.unwrap();
        }
        assert_eq!(sent_pages(&rig).await, [(json!(2), json!(1)), (json!(3), json!(1))]);
        assert_eq!(sent_modes(&rig).await, [json!("ARTICLE"), json!("ARTICLE")]);
    }

    /// Limite connue n° 2 (garantie 14, gap3-3) : à chaud (mémo `ARTICLE`), une page au-delà des
    /// articles rend le hit du code sans article, comme en 1.1.0.
    #[tokio::test]
    async fn a_warm_code_page_past_the_end_keeps_the_1_1_0_answer() {
        let rig = Rig::new().await;
        search_replies(&rig, &article_page_past_the_end(3)).await;
        rig.corpus.inner.memo.remember(&c12(5, 5), Some(PaginationMode::Article));
        let page = rig.corpus.search(&c12(5, 5)).await.unwrap();
        assert_eq!(ids(&page), ["LEGITEXT000005634379"]);
        assert_eq!(article_ids(&page), Vec::<&str>::new());
        assert_eq!(page.total_articles, Some(3));
    }

    #[tokio::test]
    async fn search_page_2_sends_page_number_2() {
        let rig = Rig::new().await;
        search_replies(&rig, EMPTY_PAGE).await;
        rig.corpus.search(&golden_page(Fond::Loda, 2, None)).await.unwrap();
        let sent = sent_body(&rig, endpoint::SEARCH).await;
        assert_eq!(
            (&sent["recherche"]["pageNumber"], &sent["recherche"]["typePagination"]),
            (&json!(2), &json!("DEFAUT"))
        );
    }

    #[tokio::test]
    async fn type_pagination_is_echoed_on_next_page() {
        let rig = Rig::new().await;
        search_replies(&rig, r#"{"results":[],"totalResultNumber":0,"typePagination":"ARTICLE"}"#)
            .await;
        for page in [1, 2] {
            rig.corpus.search(&golden_page(Fond::Juri, page, None)).await.unwrap();
        }
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("ARTICLE")]);
    }

    #[tokio::test]
    async fn code_page_2_without_memo_sends_defaut() {
        let rig = Rig::new().await;
        search_replies(&rig, EMPTY_PAGE).await;
        rig.corpus.search(&golden_page(Fond::Code, 2, None)).await.unwrap();
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT")]);
    }

    /// Documente V-82 (ce n'est pas le rouge de C5, qui passait déjà avant : le repli sans mémo
    /// est testé par `code_page_2_without_memo_sends_defaut`) : la page 1 de `CODE_DATE`
    /// enregistrée en sandbox répond `typePagination` `DEFAUT` ; la page 2 repart donc en
    /// `DEFAUT`, et non en `ARTICLE` qui rejouerait la page 1.
    #[tokio::test]
    async fn code_page_2_follows_the_defaut_answered_by_the_sandbox() {
        const PAGE_1: &str =
            include_str!("../../tests/fixtures/recorded/search/pagination/code-page1-defaut.json");
        let rig = Rig::new().await;
        search_replies(&rig, PAGE_1).await;
        let first = rig.corpus.search(&golden_page(Fond::Code, 1, None)).await.unwrap();
        rig.corpus.search(&golden_page(Fond::Code, 2, None)).await.unwrap();
        assert_eq!(first.pagination, PaginationMode::Defaut);
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("DEFAUT")]);
    }

    #[tokio::test]
    async fn a_code_page_without_a_known_type_pagination_leaves_no_memo() {
        for body in [
            r#"{"results":[],"totalResultNumber":0,"typePagination":"PAR_PAGE"}"#,
            r#"{"results":[],"totalResultNumber":0,"typePagination":7}"#,
        ] {
            let rig = Rig::new().await;
            search_replies(&rig, body).await;
            let first = rig.corpus.search(&golden_page(Fond::Code, 1, None)).await.unwrap();
            rig.corpus.search(&golden_page(Fond::Code, 2, None)).await.unwrap();
            assert_eq!(first.pagination, PaginationMode::Defaut, "{body}");
            assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("DEFAUT")], "{body}");
        }
    }

    #[tokio::test]
    async fn a_search_that_fails_to_map_remembers_no_type_pagination() {
        let rig = Rig::new().await;
        // Le mode ARTICLE est reçu, mais la date du résultat est illisible : le mapping échoue.
        search_replies(
            &rig,
            r#"{"results":[{"titles":[{"id":"JURITEXT000000000001","title":"t"}],"date":"hier"}],
                "totalResultNumber":1,"typePagination":"ARTICLE"}"#,
        )
        .await;
        for page in [1, 2] {
            let outcome = rig.corpus.search(&golden_page(Fond::Juri, page, None)).await.map(drop);
            assert_eq!(
                outcome,
                Err(CorpusError::Decode {
                    detail: "/search: results[].date: unreadable date".to_owned()
                }),
                "page {page}"
            );
        }
        // Sans mémo, la page 2 d'un fond de jurisprudence repart de DEFAUT.
        assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("DEFAUT")]);
    }

    #[tokio::test]
    async fn date_range_maps_to_fond_facet() {
        let range = DateRange::new(
            Some(VersionDate::parse("2016-01-01").unwrap()),
            Some(VersionDate::parse("2016-12-31").unwrap()),
        )
        .unwrap();
        for (fond, facet) in [
            (Fond::Loda, "DATE_SIGNATURE"),
            (Fond::Juri, "DATE_DECISION"),
            (Fond::Constit, "DATE_DECISION"),
            (Fond::Cetat, "DATE_DECISION"),
            (Fond::Cnil, "DATE_DELIB"),
            (Fond::Kali, "DATE_SIGNATURE"),
            (Fond::Acco, "DATE_SIGNATURE"),
        ] {
            let rig = Rig::new().await;
            search_replies(&rig, EMPTY_PAGE).await;
            rig.corpus.search(&golden_page(fond, 1, range)).await.unwrap();
            let sent = sent_body(&rig, endpoint::SEARCH).await;
            assert_eq!(
                sent["recherche"]["filtres"].as_array().unwrap().last(),
                Some(
                    &json!({"facette": facet, "dates": {"start": "2016-01-01", "end": "2016-12-31"}})
                ),
                "{fond:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_loda_or_code_query_without_version_date_never_reaches_the_network() {
        let rig = Rig::new().await;
        never(&rig.server, TOKEN_PATH).await;
        never(&rig.server, &format!("{API_BASE}{}", endpoint::SEARCH)).await;
        for fond in [Fond::Loda, Fond::Code] {
            let query = SearchQuery::new(
                fond,
                QueryText::parse("golden").unwrap(),
                NonZeroU32::MIN,
                PageSize::default(),
                MatchMode::default(),
                None,
                None,
            )
            .unwrap();
            assert_eq!(
                rig.corpus.search(&query).await,
                Err(CorpusError::InvalidRequest {
                    status: None,
                    detail: format!("version_date missing for fond {}", fond.name()),
                })
            );
        }
    }

    #[tokio::test]
    async fn a_full_page_of_100_results_passes_and_more_is_refused() {
        for (page_size, count, expected) in [
            (100, 100, Ok(100)),
            (
                100,
                101,
                Err(CorpusError::Decode {
                    detail: "/search: results[100]: too many elements".to_owned(),
                }),
            ),
            (
                10,
                11,
                Err(CorpusError::Decode {
                    detail: "/search: results: more results than page size".to_owned(),
                }),
            ),
        ] {
            let rig = Rig::new().await;
            search_replies(&rig, &juri_results(count)).await;
            let query = SearchQuery::new(
                Fond::Juri,
                QueryText::parse("golden").unwrap(),
                NonZeroU32::MIN,
                PageSize::new(page_size).unwrap(),
                MatchMode::default(),
                None,
                None,
            )
            .unwrap();
            let outcome = rig.corpus.search(&query).await.map(|page| page.hits.len());
            assert_eq!(outcome, expected, "{page_size} / {count}");
            assert_eq!(rig.corpus.stats(), StatsSnapshot::default(), "{page_size} / {count}");
        }
    }

    // ---- sécurité par le port : identifiants dans le corps, aucune URL suivie, aucun N+1 ------

    /// Le jeton une fois ; toute route de l'API répond `{}` (200) à volonté.
    async fn every_endpoint_replies_empty(rig: &Rig) {
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path_regex(format!("^{API_BASE}/")))
            .respond_with(ResponseTemplate::new(200).set_body_raw("{}", "application/json"))
            .mount(&rig.server)
            .await;
    }

    /// La fixture `name` lue en `Value`, puis chaque `"objet/clé"` de `edits` posé à `value` par
    /// insertion dans l'objet parent (la clé est créée si elle manque), rendue en texte.
    fn edited(name: &str, edits: &[&str], value: &Value) -> String {
        let mut document: Value = serde_json::from_str(fixture_text(name)).unwrap();
        for edit in edits {
            let (parent, key) = edit.rsplit_once('/').unwrap();
            document
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.to_owned(), value.clone());
        }
        document.to_string()
    }

    /// Le jeton une fois ; `endpoint` répond `body` (200) à volonté.
    async fn reply_on(rig: &Rig, endpoint: &str, body: String) {
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&rig.server)
            .await;
    }

    #[tokio::test]
    async fn lawdecree_too_large_is_typed() {
        // Le Code du travail dépasse la borne de corps de 16 Mio (31,4 Mio, V-96, C2) : l'erreur
        // est typée, sans nouvelle tentative ; `lf-application::messages` oriente alors vers
        // `get_article` ou la recherche, d'après la variante et sans lire le `detail`.
        let rig = Rig::new().await;
        reply_on(&rig, endpoint::LAW_DECREE, " ".repeat(16 * 1024 * 1024 + 1)).await;
        let law = LegiTextId::parse(fixture("consult_loda").requested).unwrap();
        assert_eq!(
            rig.corpus.get_act(&law, version_date()).await.map(drop),
            Err(CorpusError::TooLarge {
                detail: "response too large (> 16 MiB) on /consult/lawDecree".to_owned()
            })
        );
        assert_eq!(api_requests(&rig).await.len(), 1);
    }

    #[tokio::test]
    async fn non_lawdecree_too_large_is_typed_too() {
        // Même variante pour toute consultation et pour la recherche, à la borne configurée :
        // seul l'outil appelant décide du message (la consigne `get_article` est celle de
        // `get_act`).
        let tuned = || Rig::tuned(|setup| setup.tuning.max_body_bytes = 1_000);
        let rig = tuned().await;
        reply_on(&rig, endpoint::JURI, "a".repeat(2_000)).await;
        let id = DecisionId::parse(fixture("consult_juri").requested).unwrap();
        assert_eq!(
            rig.corpus.get_decision(&id).await.map(drop),
            Err(CorpusError::TooLarge {
                detail: "response too large (> 1000 bytes) on /consult/juri".to_owned()
            })
        );
        let rig = tuned().await;
        reply_on(&rig, endpoint::SEARCH, "a".repeat(2_000)).await;
        assert_eq!(
            rig.corpus.search(&golden(Fond::Juri)).await.map(drop),
            Err(CorpusError::TooLarge {
                detail: "response too large (> 1000 bytes) on /search".to_owned()
            })
        );
        assert_eq!(api_requests(&rig).await.len(), 1);
    }

    #[tokio::test]
    async fn a_too_large_token_response_keeps_the_generic_decode_error() {
        // La borne de corps vaut aussi pour le jeton OAuth : un jeton énorme n'est pas « un texte
        // trop gros pour `get_act` », il reste un `Decode`, jamais un `TooLarge`, et l'API n'est
        // pas appelée.
        let rig = Rig::tuned(|setup| setup.tuning.max_body_bytes = 1_000).await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_raw("a".repeat(2_000), "text/plain"))
            .expect(1)
            .mount(&rig.server)
            .await;
        let route = format!("{API_BASE}{}", endpoint::LAW_DECREE);
        never(&rig.server, &route).await;
        let law = LegiTextId::parse(fixture("consult_loda").requested).unwrap();
        assert_eq!(
            rig.corpus.get_act(&law, version_date()).await.map(drop),
            Err(CorpusError::Decode {
                detail: "response too large (> 1000 bytes) on PISTE OAuth".to_owned()
            })
        );
    }

    #[tokio::test]
    async fn get_act_under_the_body_limit_is_decoded_as_before() {
        // La fixture `consult_loda` tient sous une borne de 1 Mio : la borne seule ne change rien.
        let name = "consult_loda";
        let request = json!({"textId": "LEGITEXT000033205014", "date": "2026-10-15"});
        let rig = Rig::tuned(|setup| setup.tuning.max_body_bytes = 1 << 20).await;
        serve(&rig, endpoint::LAW_DECREE, request, name).await;
        let law = LegiTextId::parse(fixture(name).requested).unwrap();
        let act = rig.corpus.get_act(&law, version_date()).await.unwrap();
        assert_eq!(view(&Mapped::Act(act)), expected(name, fixture(name).requested));
    }

    /// Chemin, query et corps de chaque requête reçue hors jeton, dans l'ordre.
    async fn api_requests(rig: &Rig) -> Vec<(String, Option<String>, Value)> {
        let requests = rig.server.received_requests().await.unwrap();
        requests
            .iter()
            .filter(|request| request.url.path() != TOKEN_PATH)
            .map(|request| {
                (
                    request.url.path().to_owned(),
                    request.url.query().map(str::to_owned),
                    serde_json::from_slice(&request.body).unwrap(),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn ids_go_in_body_never_in_path() {
        let rig = Rig::new().await;
        every_endpoint_replies_empty(&rig).await;
        let hostile = "../../oauth/token?grant_type=x#frag";
        let query = SearchQuery::new(
            Fond::Juri,
            QueryText::parse(hostile).unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            None,
        )
        .unwrap();
        let _ignored = rig.corpus.search(&query).await;
        let law = LegiTextId::parse(" legitext000033205014_01-01-2020 ").unwrap();
        let _ignored = rig.corpus.get_act(&law, version_date()).await;
        let article = ArticleId::parse(" kaliarti000049886059 ").unwrap();
        let _ignored = rig.corpus.get_article(&article).await;
        let decision = DecisionId::parse("constext000052555275").unwrap();
        let _ignored = rig.corpus.get_decision(&decision).await;
        let deliberation = CnilTextId::parse("CNILTEXT000053352643 ").unwrap();
        let _ignored = rig.corpus.get_deliberation(&deliberation).await;
        let convention = KaliTextId::parse(" KALITEXT000049878950").unwrap();
        let _ignored = rig.corpus.get_convention(&convention).await;
        let agreement = AccoTextId::parse("accotext000053643828").unwrap();
        let _ignored = rig.corpus.get_company_agreement(&agreement).await;

        let sent = api_requests(&rig).await;
        let (search, consultations) = sent.split_first().unwrap();
        assert_eq!(search.0, format!("{API_BASE}{}", endpoint::SEARCH));
        assert_eq!(search.1, None);
        assert_eq!(search.2["recherche"]["champs"][0]["criteres"][0]["valeur"], hostile);
        let expected_consultations = [
            (endpoint::LAW_DECREE, json!({"textId": "LEGITEXT000033205014", "date": "2026-10-15"})),
            (endpoint::GET_ARTICLE, json!({"id": "KALIARTI000049886059"})),
            (endpoint::JURI, json!({"textId": "CONSTEXT000052555275"})),
            (endpoint::CNIL, json!({"textId": "CNILTEXT000053352643"})),
            (endpoint::KALI_TEXT, json!({"id": "KALITEXT000049878950"})),
            (endpoint::ACCO, json!({"id": "ACCOTEXT000053643828"})),
        ]
        .map(|(template, body)| (format!("{API_BASE}{template}"), None, body));
        assert_eq!(consultations, expected_consultations);
        assert_eq!(sent.len(), 7);
    }

    #[tokio::test]
    async fn urls_in_responses_are_never_fetched() {
        let rig = Rig::new().await;
        let trap = format!("{}/trap", rig.server.uri());
        Mock::given(method("POST"))
            .and(path_regex("^/trap"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&rig.server)
            .await;
        token_mock(&rig.server, "tok-1", 1).await;
        let link = Value::String(trap.clone());
        for (route, name, edits) in [
            (endpoint::JURI, "consult_juri_constit", &["/text/lienJo", "/text/urlCC"][..]),
            (endpoint::CNIL, "consult_cnil", &["/text/lienJo", "/text/urlCC"][..]),
            (endpoint::ACCO, "consult_acco", &["/acco/url", "/acco/attachementUrl"][..]),
        ] {
            let body = edited(name, edits, &link);
            Mock::given(method("POST"))
                .and(path(format!("{API_BASE}{route}")))
                .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
                .mount(&rig.server)
                .await;
        }
        let original = fixture_text("consult_kali");
        let kali = original.replace("/affichCodeArticle.do", &format!("{trap}/kali"));
        assert_ne!(kali, original);
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::KALI_TEXT)))
            .respond_with(ResponseTemplate::new(200).set_body_raw(kali, "application/json"))
            .mount(&rig.server)
            .await;

        let decision = DecisionId::parse(fixture("consult_juri_constit").requested).unwrap();
        rig.corpus.get_decision(&decision).await.unwrap();
        let deliberation = CnilTextId::parse(fixture("consult_cnil").requested).unwrap();
        rig.corpus.get_deliberation(&deliberation).await.unwrap();
        let agreement = AccoTextId::parse(fixture("consult_acco").requested).unwrap();
        rig.corpus.get_company_agreement(&agreement).await.unwrap();
        let convention = KaliTextId::parse(fixture("consult_kali").requested).unwrap();
        rig.corpus.get_convention(&convention).await.unwrap();
        assert_eq!(requests_to(&rig.server, "/trap").await, 0);
        assert_eq!(requests_to(&rig.server, "/trap/kali").await, 0);
    }

    #[tokio::test]
    async fn kali_inline_content_is_available_without_extra_call() {
        let name = "consult_kali";
        let requested = fixture(name).requested;
        let rig = Rig::new().await;
        serve(&rig, endpoint::KALI_TEXT, json!({"id": requested}), name).await;
        never(&rig.server, &format!("{API_BASE}{}", endpoint::GET_ARTICLE)).await;
        let id = KaliTextId::parse(requested).unwrap();
        let convention = rig.corpus.get_convention(&id).await.unwrap();
        let projection = view(&Mapped::Convention(convention));
        let article = &projection["toc"][0]["section"]["children"][0]["article"];
        assert_eq!(article["article_id"], "KALIARTI000049886059");
        let inline = article["inline_text"]["text"].as_str().unwrap();
        assert!(inline.starts_with("Les parties conviennent de préciser"), "{inline}");
    }

    #[tokio::test]
    async fn acco_large_data_field_is_skipped() {
        let name = "consult_acco";
        let requested = fixture(name).requested;
        let rig = Rig::new().await;
        let big = Value::String("QUJD".repeat(5 * 1024 * 1024 / 4));
        let body = edited(name, &["/acco/data"], &big);
        drop(big);
        reply_on(&rig, endpoint::ACCO, body).await;
        let id = AccoTextId::parse(requested).unwrap();
        let agreement = rig.corpus.get_company_agreement(&id).await.unwrap();
        assert_eq!(view(&Mapped::Agreement(agreement)), expected(name, requested));
    }

    #[tokio::test]
    async fn a_port_error_never_carries_a_body_value() {
        let rig = Rig::new().await;
        let body = r#"{"text":{"titre":"t","numeroAffaire":"sekret-42-Ω"}}"#;
        reply_on(&rig, endpoint::JURI, body.to_owned()).await;
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        let id = DecisionId::parse("JURITEXT000007024230").unwrap();
        let error = rig.corpus.get_decision(&id).await.unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode {
                detail:
                    "/consult/juri: text.numeroAffaire: invalid type: string, expected a sequence"
                        .to_owned(),
            }
        );
        for rendered in [error.to_string(), format!("{error:?}"), capture.text()] {
            assert!(!rendered.contains("sekret"), "{rendered}");
        }
    }

    // ---- erreurs par le port (04 §5.1, §4.3, §8.5, §9.3, §11.2) ------------------------------

    /// Les six consultations, dans l'ordre lawDecree, getArticle, juri, cnil, kaliText, acco,
    /// sur les identifiants des fixtures ; seule l'issue compte.
    async fn each_consultation(rig: &Rig) -> [Result<(), CorpusError>; 6] {
        let corpus = &rig.corpus;
        let law = LegiTextId::parse(fixture("consult_loda").requested).unwrap();
        let article = ArticleId::parse(fixture("consult_article").requested).unwrap();
        let decision = DecisionId::parse(fixture("consult_juri").requested).unwrap();
        let deliberation = CnilTextId::parse(fixture("consult_cnil").requested).unwrap();
        let convention = KaliTextId::parse(fixture("consult_kali").requested).unwrap();
        let agreement = AccoTextId::parse(fixture("consult_acco").requested).unwrap();
        [
            corpus.get_act(&law, version_date()).await.map(drop),
            corpus.get_article(&article).await.map(drop),
            corpus.get_decision(&decision).await.map(drop),
            corpus.get_deliberation(&deliberation).await.map(drop),
            corpus.get_convention(&convention).await.map(drop),
            corpus.get_company_agreement(&agreement).await.map(drop),
        ]
    }

    /// `NotFound` du gabarit d'endpoint `endpoint`.
    fn not_found(endpoint: &str) -> Result<(), CorpusError> {
        Err(CorpusError::NotFound { what: endpoint.to_owned() })
    }

    /// Les six `NotFound` attendus, dans l'ordre de `each_consultation`.
    fn six_not_found() -> [Result<(), CorpusError>; 6] {
        [
            endpoint::LAW_DECREE,
            endpoint::GET_ARTICLE,
            endpoint::JURI,
            endpoint::CNIL,
            endpoint::KALI_TEXT,
            endpoint::ACCO,
        ]
        .map(not_found)
    }

    /// `endpoint` répond `body` (200) à volonté, sans toucher au jeton.
    async fn route_replies(rig: &Rig, endpoint: &str, body: &str) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json"),
            )
            .mount(&rig.server)
            .await;
    }

    #[tokio::test]
    async fn consult_empty_envelope_maps_to_not_found() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        for (route, body) in [
            (endpoint::LAW_DECREE, "{}"),
            (endpoint::GET_ARTICLE, r#"{"article":null}"#),
            (endpoint::JURI, r#"{"text":null}"#),
            (endpoint::CNIL, r#"{"text":null}"#),
            (endpoint::KALI_TEXT, "{}"),
            (endpoint::ACCO, r#"{"acco":null}"#),
        ] {
            route_replies(&rig, route, body).await;
        }
        assert_eq!(each_consultation(&rig).await, six_not_found());
        // Un `NotFound` ne compte rien (04 §9.3).
        assert_eq!(rig.corpus.stats(), StatsSnapshot::default());
    }

    #[tokio::test]
    async fn unknown_ids_400_map_to_not_found_on_every_consultation() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let refused = ResponseTemplate::new(400).set_body_raw(
            r#"{"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null}"#,
            "application/json",
        );
        Mock::given(method("POST"))
            .and(path_regex(format!("^{API_BASE}/consult/")))
            .respond_with(refused)
            .mount(&rig.server)
            .await;
        assert_eq!(each_consultation(&rig).await, six_not_found());
    }

    #[tokio::test]
    async fn the_recorded_acco_count_400_maps_to_not_found() {
        // Le 400 réel d'un identifiant ACCO inexistant (V-68), servi tel qu'enregistré : le
        // message de comptage, et non « valider est fausse », dit « introuvable » sur ACCO.
        let recorded = include_str!(
            "../../tests/fixtures/recorded/consult/acco/inconnu/ACCOTEXT000000000000.json"
        );
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::ACCO)))
            .respond_with(ResponseTemplate::new(400).set_body_raw(recorded, "application/json"))
            .expect(1)
            .mount(&rig.server)
            .await;
        let id = AccoTextId::parse("ACCOTEXT000000000000").unwrap();
        assert_eq!(
            rig.corpus.get_company_agreement(&id).await.map(drop),
            Err(CorpusError::NotFound { what: "/consult/acco".to_owned() })
        );
    }

    #[tokio::test]
    async fn the_400_without_marker_is_an_invalid_request_with_a_generic_detail_on_search() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let refused = ResponseTemplate::new(400)
            .set_body_raw(r#"{"message":"Champ golden invalide"}"#, "application/json");
        Mock::given(method("POST"))
            .and(path_regex(format!("^{API_BASE}/")))
            .respond_with(refused)
            .mount(&rig.server)
            .await;
        let id = DecisionId::parse(fixture("consult_juri").requested).unwrap();
        assert_eq!(
            rig.corpus.get_decision(&id).await.map(drop),
            Err(CorpusError::InvalidRequest {
                status: Some(400),
                detail: "Champ golden invalide".to_owned(),
            })
        );
        assert_eq!(
            rig.corpus.search(&golden(Fond::Juri)).await.map(drop),
            Err(CorpusError::InvalidRequest {
                status: Some(400),
                detail: "HTTP 400 on /search".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn api_401_refreshes_once_then_maps() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        token_mock(&rig.server, "tok-2", 1).await;
        let route = format!("{API_BASE}{}", endpoint::JURI);
        Mock::given(method("POST"))
            .and(path(route.clone()))
            .and(header("authorization", "Bearer tok-1"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&rig.server)
            .await;
        Mock::given(method("POST"))
            .and(path(route))
            .and(header("authorization", "Bearer tok-2"))
            .respond_with(fixture_reply("consult_juri"))
            .expect(1)
            .mount(&rig.server)
            .await;
        let requested = fixture("consult_juri").requested;
        let id = DecisionId::parse(requested).unwrap();
        let decision = rig.corpus.get_decision(&id).await.unwrap();
        assert_eq!(view(&Mapped::Decision(decision)), expected("consult_juri", requested));
    }

    #[tokio::test]
    async fn the_429_and_the_503_are_retried_then_classified() {
        for (template, error) in [
            (
                ResponseTemplate::new(429).insert_header("retry-after", "1"),
                CorpusError::RateLimited { retry_after: Some(Duration::from_secs(1)) },
            ),
            (
                ResponseTemplate::new(503),
                CorpusError::Upstream {
                    status: Some(503),
                    retryable: true,
                    detail: "HTTP 503 on /consult/acco".to_owned(),
                },
            ),
        ] {
            let rig = Rig::new().await;
            token_mock(&rig.server, "tok-1", 1).await;
            Mock::given(method("POST"))
                .and(path(format!("{API_BASE}{}", endpoint::ACCO)))
                .respond_with(template)
                .expect(3)
                .mount(&rig.server)
                .await;
            let id = AccoTextId::parse(fixture("consult_acco").requested).unwrap();
            assert_eq!(rig.corpus.get_company_agreement(&id).await.map(drop), Err(error));
            assert_eq!(rig.sleeper.waits().len(), 2);
        }
    }

    #[tokio::test]
    async fn a_body_of_more_than_16_mib_maps_to_too_large() {
        let rig = Rig::new().await;
        token_mock(&rig.server, "tok-1", 1).await;
        let body = vec![b' '; 16 * 1024 * 1024 + 1];
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{}", endpoint::ACCO)))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .expect(1)
            .mount(&rig.server)
            .await;
        let id = AccoTextId::parse(fixture("consult_acco").requested).unwrap();
        assert_eq!(
            rig.corpus.get_company_agreement(&id).await.map(drop),
            Err(CorpusError::TooLarge {
                detail: "response too large (> 16 MiB) on /consult/acco".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn a_body_beyond_the_element_budget_maps_to_decode_with_its_path() {
        let rig = Rig::new().await;
        let articles = vec!["{}"; 200_001].join(",");
        reply_on(&rig, endpoint::LAW_DECREE, format!(r#"{{"articles":[{articles}]}}"#)).await;
        let id = LegiTextId::parse(fixture("consult_loda").requested).unwrap();
        assert_eq!(
            rig.corpus.get_act(&id, version_date()).await.map(drop),
            Err(CorpusError::Decode {
                detail: "/consult/lawDecree: articles[200000]: too many elements".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn a_mapping_error_keeps_the_diagnostics_counted_before_it() {
        // Le premier résultat est écarté et compté ; le second, gardé, a une date illisible :
        // la page entière est un `Decode`, mais le compte du premier est versé.
        let rig = Rig::new().await;
        let page = json!({
            "totalResultNumber": 2,
            "results": [
                {"titles": [{"id": "not-an-id"}]},
                {"titles": [{"id": "JURITEXT000051743650", "title": "t"}], "date": "sekret"},
            ],
        });
        reply_on(&rig, endpoint::SEARCH, page.to_string()).await;
        assert_eq!(
            rig.corpus.search(&golden(Fond::Juri)).await.map(drop),
            Err(CorpusError::Decode {
                detail: "/search: results[].date: unreadable date".to_owned()
            })
        );
        assert_eq!(
            rig.corpus.stats(),
            StatsSnapshot { dropped_hits: 1, ..StatsSnapshot::default() }
        );
    }
}
