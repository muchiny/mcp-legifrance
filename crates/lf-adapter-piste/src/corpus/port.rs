//! `impl LegalCorpus for PisteCorpus` : les sept opérations du port (04 §1.3, §7.1, §12.3).
//!
//! L'identifiant d'une consultation va dans le **corps** de la requête (`request::consult`), jamais
//! dans le chemin : le gabarit d'endpoint est constant, ne dépend que de l'opération, et aucune
//! URL lue dans une réponse (`lienJo`, `urlCC`, `url`, `attachementUrl`, `href`) n'est appelée. Le
//! décodage sous budget et le mapping s'exécutent dans `PisteCorpus::fetch`, donc sous le permis de
//! concurrence.
//!
//! `search` lit le mémo de `typePagination` **avant** l'envoi (`PaginationMemo::mode_for`, qui
//! alimente `search_body` et sa garde défensive, avant toute requête, jeton compris) et ne retient
//! le mode reçu qu'**après** le mapping : sur `Err`, rien n'est retenu (R12).

use lf_application::ports::{CorpusError, LegalCorpus};
use lf_domain::date::VersionDate;
use lf_domain::entity::{
    Act, ArticleText, CompanyAgreement, Convention, Decision, Deliberation, SearchPage,
};
use lf_domain::id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId};
use lf_domain::search::SearchQuery;
use serde::de::DeserializeOwned;

use super::PisteCorpus;
use crate::classify::EndpointKind;
use crate::map::endpoint;
use crate::map::search::search_page_with_mode;
use crate::map::{Diagnostics, act, agreement, article, convention, decision, deliberation};
use crate::request::consult::Consultation;
use crate::request::search::search_body;
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

impl<K: MonoClock + Clone, S: Sleeper + Clone> LegalCorpus for PisteCorpus<K, S> {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        let memo = &self.inner.memo;
        let body = search_body(query, memo.mode_for(query))?;
        let (page, received) = self
            .fetch(EndpointKind::Search, endpoint::SEARCH, &body, |dto, diags| {
                search_page_with_mode(dto, query, diags)
            })
            .await?;
        memo.remember(query, received);
        Ok(page)
    }

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
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::time::Duration;

    use lf_application::ports::{CorpusError, LegalCorpus};
    use lf_domain::date::{DateRange, VersionDate};
    use lf_domain::fond::{Fond, PaginationMode};
    use lf_domain::id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId};
    use lf_domain::search::{MatchMode, PageSize, QueryText, SearchQuery};
    use serde_json::{Value, json};
    use wiremock::matchers::{body_json, header, method, path, path_regex};
    use wiremock::{Mock, ResponseTemplate};

    use crate::corpus::tests::{API_BASE, Rig, TOKEN_PATH, never, requests_to, token_mock};
    use crate::map::Diagnostics;
    use crate::map::endpoint;
    use crate::map::fixtures::{Mapped, decoded, fixture, map_body, text as fixture_text};
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
    async fn code_page_2_without_memo_sends_article() {
        let rig = Rig::new().await;
        search_replies(&rig, EMPTY_PAGE).await;
        rig.corpus.search(&golden_page(Fond::Code, 2, None)).await.unwrap();
        assert_eq!(sent_modes(&rig).await, [json!("ARTICLE")]);
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
            assert_eq!(sent_modes(&rig).await, [json!("DEFAUT"), json!("ARTICLE")], "{body}");
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
    async fn a_body_of_more_than_16_mib_maps_to_decode() {
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
            Err(CorpusError::Decode {
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
