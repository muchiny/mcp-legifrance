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

    use lf_application::ports::{CorpusError, LegalCorpus};
    use lf_domain::date::{DateRange, VersionDate};
    use lf_domain::fond::{Fond, PaginationMode};
    use lf_domain::id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId};
    use lf_domain::search::{MatchMode, PageSize, QueryText, SearchQuery};
    use serde_json::{Value, json};
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, ResponseTemplate};

    use crate::corpus::tests::{API_BASE, Rig, TOKEN_PATH, never, token_mock};
    use crate::map::Diagnostics;
    use crate::map::endpoint;
    use crate::map::fixtures::{Mapped, decoded, fixture, map_body};
    use crate::request::search::search_body;
    use crate::stats::StatsSnapshot;
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
}
