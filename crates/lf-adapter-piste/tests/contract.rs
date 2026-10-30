//! Suite de contrat du port `LegalCorpus` sur `PisteCorpus` (T-M3-05, 06 §2.3) : le vrai client
//! HTTP de l'adapter parle à un `wiremock` qui sert les réponses de référence
//! (`tests/fixtures/reference`) et les fixtures enregistrées de M6 et M7
//! (`tests/fixtures/recorded`) sur les mêmes cas que `FakeCorpus` et `CachedCorpus`, pour les
//! douze opérations du port.
//!
//! Les cas sont ceux de `ContractFixtures::reference` : les identifiants des fixtures. Une
//! consultation sert la fixture de son identifiant ; une recherche, celle de son fond ou de son
//! numéro ; tout autre identifiant reçoit la réponse que PISTE donne à un identifiant inconnu,
//! enregistrée en sandbox : 400 pour les consultations (que l'adapter classe en `NotFound`, 04
//! §11.2), 200 sans octet pour `canonicalArticleVersion`, 200 à enveloppe nulle pour
//! `getArticleWithIdAndNum`, et 500 pour `relatedLinksArticle` (qui reste une erreur amont, 04
//! §11.1). Aucun réseau hors de la boucle locale.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod contract {
    use std::num::NonZeroU32;

    use lf_adapter_piste::{
        PisteConfig, PisteCorpus, PisteCredentials, PisteEndpoints, PisteTuning, RateQuota,
    };
    use lf_application::contract::{ContractFixtures, corpus_contract};
    use lf_domain::coverage::PisteEnvironment;
    use lf_domain::fond::Fond;
    use lf_domain::id::DecisionId;
    use lf_domain::search::{NumberKind, NumberQuery};
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const API_BASE: &str = "/dila/legifrance/lf-engine-app";
    const TOKEN_PATH: &str = "/oauth/token";

    const CONSULT_LODA: &[u8] = include_bytes!("fixtures/reference/consult_loda.json");
    const CONSULT_ARTICLE: &[u8] = include_bytes!("fixtures/reference/consult_article.json");
    const CONSULT_JURI: &[u8] = include_bytes!("fixtures/reference/consult_juri.json");
    const CONSULT_CONSTIT: &[u8] = include_bytes!("fixtures/reference/consult_juri_constit.json");
    const CONSULT_CETAT: &[u8] = include_bytes!("fixtures/reference/consult_juri_cetat.json");
    const CONSULT_CNIL: &[u8] = include_bytes!("fixtures/reference/consult_cnil.json");
    const CONSULT_KALI: &[u8] = include_bytes!("fixtures/reference/consult_kali.json");
    const CONSULT_ACCO: &[u8] = include_bytes!("fixtures/reference/consult_acco.json");

    // M6 : fixtures enregistrées en sandbox (étape I-R).
    const BY_NUMBER: &[u8] = include_bytes!(
        "fixtures/recorded/consult/getArticleWithIdAndNum/m6/getArticleWithIdAndNum-code-civil-9.json"
    );
    const BY_NUMBER_UNKNOWN: &[u8] = include_bytes!(
        "fixtures/recorded/consult/getArticleWithIdAndNum/m6/getArticleWithIdAndNum-numero-inconnu.json"
    );
    const EXACT_ARTICLE_9: &[u8] =
        include_bytes!("fixtures/recorded/search/m6/search-exacte-num-article-9.json");
    const ECLI_JURI: &[u8] = include_bytes!(
        "fixtures/recorded/search/m6/search-ecli-JURI-ECLI-FR-CCASS-2025-C100399.json"
    );
    const ECLI_UNKNOWN: &[u8] =
        include_bytes!("fixtures/recorded/search/m6/search-ecli-JURI-inconnu.json");
    const IDCC_2121: &[u8] =
        include_bytes!("fixtures/recorded/consult/kaliContIdcc/m6/kaliContIdcc-2121.json");
    const LIST_CODE_1: &[u8] =
        include_bytes!("fixtures/recorded/list/code/m6/list-code-page1.json");
    const LIST_CODE_2: &[u8] =
        include_bytes!("fixtures/recorded/list/code/m6/list-code-page2.json");

    // M7 : idem.
    const CANONICAL: &[u8] = include_bytes!(
        "fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-LEGIARTI000006419288.json"
    );
    const CANONICAL_UNKNOWN: &[u8] = include_bytes!(
        "fixtures/recorded/search/canonicalArticleVersion/m7/canonicalArticleVersion-inconnu.json"
    );
    const RELATED: &[u8] = include_bytes!(
        "fixtures/recorded/consult/relatedLinksArticle/m7/relatedLinksArticle-LEGIARTI000006419288.json"
    );
    const RELATED_UNKNOWN: &[u8] = include_bytes!(
        "fixtures/recorded/consult/relatedLinksArticle/m7/relatedLinksArticle-inconnu.json"
    );

    /// L'ECLI dont la recherche est enregistrée ; tout autre ECLI du contrat reçoit la page vide.
    const RECORDED_ECLI: &str = "ECLI:FR:CCASS:2025:C100399";

    /// Une page de `/search` sans résultat, pour un numéro d'article qu'aucun code ne porte.
    const NO_ARTICLE: &[u8] =
        br#"{"executionTime":0,"results":[],"totalResultNumber":0,"typePagination":"ARTICLE"}"#;

    /// Réponse de `/search` du fond.
    fn search_fixture(fond: Fond) -> &'static [u8] {
        match fond {
            Fond::Loda => include_bytes!("fixtures/reference/search_loda.json"),
            Fond::Code => include_bytes!("fixtures/reference/search_code.json"),
            Fond::Juri => include_bytes!("fixtures/reference/search_juri.json"),
            Fond::Constit => include_bytes!("fixtures/reference/search_constit.json"),
            Fond::Cetat => include_bytes!("fixtures/reference/search_cetat.json"),
            Fond::Cnil => include_bytes!("fixtures/reference/search_cnil.json"),
            Fond::Kali => include_bytes!("fixtures/reference/search_kali.json"),
            Fond::Acco => include_bytes!("fixtures/reference/search_acco.json"),
        }
    }

    /// Réponse de `/consult/juri` pour la décision `id`, d'après son préfixe.
    fn decision_fixture(id: &DecisionId) -> &'static [u8] {
        match id {
            DecisionId::Juri(_) => CONSULT_JURI,
            DecisionId::Constit(_) => CONSULT_CONSTIT,
            DecisionId::Cetat(_) => CONSULT_CETAT,
        }
    }

    fn json_reply(status: u16, body: &'static [u8]) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_raw(body, "application/json")
    }

    /// Le corps de `endpoint` pour `request` (sous-ensemble du JSON envoyé) est servi tel quel.
    async fn serve(server: &MockServer, endpoint: &str, request: Value, body: &'static [u8]) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .and(body_partial_json(request))
            .respond_with(json_reply(200, body))
            .mount(server)
            .await;
    }

    /// Comme [`serve`], mais plus prioritaire que la fixture générale du fond : une recherche par
    /// numéro ou par ECLI a le même `fond` qu'une recherche en texte libre.
    async fn serve_first(server: &MockServer, request: Value, body: &'static [u8]) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}/search")))
            .and(body_partial_json(request))
            .respond_with(json_reply(200, body))
            .with_priority(1)
            .mount(server)
            .await;
    }

    /// Tout autre corps de `endpoint` reçoit `status` et `body`, moins prioritaire que les
    /// fixtures : la réponse de PISTE à un identifiant inconnu.
    async fn fallback(server: &MockServer, endpoint: &str, status: u16, body: &'static [u8]) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .respond_with(json_reply(status, body))
            .with_priority(10)
            .mount(server)
            .await;
    }

    /// Réponses enregistrées en sandbox le 2026-10-28 (V-68) : le message de PISTE pour un
    /// identifiant bien formé et inexistant. L'accord d'entreprise a le sien (message de comptage).
    const UNKNOWN: &str = r#"{"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null}"#;
    const UNKNOWN_ACCO: &str = r#"{"error":400,"status":"Bad Request","message":"Le nombre de résultat retourné par le moteur de recherche n'est pas égal à 1.","errorCode":null}"#;

    /// Requête de recherche exacte de l'article `num` (`find_article` à une date, recherche par
    /// numéro d'article) : la forme se distingue d'une recherche en texte libre par son champ.
    fn exact_article_request(num: &str) -> Value {
        json!({
            "fond": "CODE_DATE",
            "recherche": {"champs": [{
                "typeChamp": "NUM_ARTICLE",
                "criteres": [{"typeRecherche": "EXACTE", "valeur": num}],
            }]},
        })
    }

    /// Requête de recherche d'un ECLI : la facette `ECLI`, à valeur exacte.
    fn ecli_request(query: &NumberQuery) -> Value {
        json!({
            "fond": query.fond.api_name(),
            "recherche": {"filtres": [{"facette": "ECLI", "valeurs": [query.value.as_str()]}]},
        })
    }

    /// Les réponses d'une recherche par numéro (`find_by_number`) : celles de l'article partagent
    /// la recherche exacte de `find_article`, montée à part ; un NOR inconnu reçoit le 400 général.
    async fn serve_number(server: &MockServer, query: &NumberQuery) {
        match query.kind {
            NumberKind::Ecli => {
                let page =
                    if query.value.as_str() == RECORDED_ECLI { ECLI_JURI } else { ECLI_UNKNOWN };
                serve_first(server, ecli_request(query), page).await;
            }
            NumberKind::Idcc => {
                let request = json!({"id": query.value.as_str()});
                serve(server, "/consult/kaliContIdcc", request, IDCC_2121).await;
            }
            // Aucune autre nature n'a de cas dans le contrat ; un cas ajouté sans fixture reçoit
            // la réponse d'un inconnu, et la suite le dit.
            _ => {}
        }
    }

    /// Serveur simulé : le jeton, les fixtures de `fx`, et la réponse de PISTE à un inconnu pour
    /// tout le reste.
    async fn piste_server(fx: &ContractFixtures) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(json_reply(
                200,
                br#"{"access_token":"contract-token","token_type":"Bearer","expires_in":3600}"#,
            ))
            .mount(&server)
            .await;
        for query in &fx.searches {
            let fond = query.fond();
            let request = json!({"fond": fond.api_name()});
            serve(&server, "/search", request, search_fixture(fond)).await;
        }
        let act = json!({"textId": fx.act.as_str()});
        serve(&server, "/consult/lawDecree", act, CONSULT_LODA).await;
        let article = json!({"id": fx.article.as_str()});
        serve(&server, "/consult/getArticle", article, CONSULT_ARTICLE).await;
        for id in &fx.decisions {
            let request = json!({"textId": id.as_str()});
            serve(&server, "/consult/juri", request, decision_fixture(id)).await;
        }
        let deliberation = json!({"textId": fx.deliberation.as_str()});
        serve(&server, "/consult/cnil", deliberation, CONSULT_CNIL).await;
        let convention = json!({"id": fx.convention.as_str()});
        serve(&server, "/consult/kaliText", convention, CONSULT_KALI).await;
        let agreement = json!({"id": fx.agreement.as_str()});
        serve(&server, "/consult/acco", agreement, CONSULT_ACCO).await;
        for endpoint in
            ["/consult/lawDecree", "/consult/getArticle", "/consult/juri", "/consult/cnil"]
        {
            fallback(&server, endpoint, 400, UNKNOWN.as_bytes()).await;
        }
        fallback(&server, "/consult/kaliText", 400, UNKNOWN.as_bytes()).await;
        fallback(&server, "/consult/acco", 400, UNKNOWN_ACCO.as_bytes()).await;
        serve_m6_m7(&server, fx).await;
        server
    }

    /// Les réponses de M6 et M7 : `find_article` (par numéro, puis par recherche exacte),
    /// `find_by_number`, la liste des codes en deux pages, les versions et les liens.
    async fn serve_m6_m7(server: &MockServer, fx: &ContractFixtures) {
        let by_number = json!({"id": fx.code.as_str(), "num": fx.article_num});
        serve(server, "/consult/getArticleWithIdAndNum", by_number, BY_NUMBER).await;
        serve_first(server, exact_article_request(&fx.article_num), EXACT_ARTICLE_9).await;
        serve_first(server, exact_article_request(&fx.absent_article_num), NO_ARTICLE).await;
        fallback(server, "/consult/getArticleWithIdAndNum", 200, BY_NUMBER_UNKNOWN).await;
        for query in fx.numbers.iter().chain(&fx.absent_numbers) {
            serve_number(server, query).await;
        }
        fallback(server, "/consult/getJoWithNor", 400, UNKNOWN.as_bytes()).await;
        serve(server, "/list/code", json!({"pageNumber": 1}), LIST_CODE_1).await;
        serve(server, "/list/code", json!({"pageNumber": 2}), LIST_CODE_2).await;
        let versions = json!({"id": fx.article.as_str()});
        serve(server, "/search/canonicalArticleVersion", versions, CANONICAL).await;
        fallback(server, "/search/canonicalArticleVersion", 200, CANONICAL_UNKNOWN).await;
        fallback(server, "/consult/getArticleByCid", 400, UNKNOWN.as_bytes()).await;
        let related = json!({"articleId": fx.article.as_str()});
        serve(server, "/consult/relatedLinksArticle", related, RELATED).await;
        fallback(server, "/consult/relatedLinksArticle", 500, RELATED_UNKNOWN).await;
    }

    /// `PisteCorpus` ordinaire (horloge et attente réelles) vers `server`, au débit maximal permis
    /// pour que les 66 appels de la suite ne s'attendent pas.
    fn corpus_for(server: &MockServer) -> PisteCorpus {
        let uri = server.uri();
        let (endpoints, _warnings) = PisteEndpoints::with_overrides(
            PisteEnvironment::Sandbox,
            Some(&format!("{uri}{TOKEN_PATH}")),
            Some(&format!("{uri}{API_BASE}")),
        )
        .unwrap();
        let credentials = PisteCredentials::new(
            "contract-client".to_owned(),
            SecretString::from("contract-secret"),
        );
        let hundred = NonZeroU32::new(100).unwrap();
        let tuning =
            PisteTuning { rate: RateQuota::new(hundred, hundred), ..PisteTuning::default() };
        let config = PisteConfig::new(
            PisteEnvironment::Sandbox,
            Ok(endpoints),
            Some(credentials),
            "lf-contract-tests/0".to_owned(),
        )
        .with_tuning(tuning)
        .unwrap();
        PisteCorpus::new(config)
    }

    #[tokio::test]
    async fn contract_on_piste_corpus() {
        let fx = ContractFixtures::reference();
        let server = piste_server(&fx).await;
        let corpus = corpus_for(&server);
        corpus_contract(&corpus, &fx).await;

        // Les fixtures servies sont celles des identifiants demandés : aucun identifiant renvoyé
        // ne diffère et aucun résultat n'est écarté. Une partie des fixtures porte du mojibake, que
        // l'adapter répare et compte : le réglage n'est pas coupé.
        let stats = corpus.stats();
        assert_eq!((stats.id_mismatch, stats.dropped_hits), (0, 0), "{stats:?}");
        assert!(stats.mojibake_repairs > 0, "the reference fixtures carry mojibake: {stats:?}");
        // Un seul jeton pour toute la suite, puis exactement les appels de la suite, route par
        // route. Aucun appel de plus (ni N+1, ni URL lue dans une réponse).
        let requests = server.received_requests().await.unwrap();
        let count = |route: &str| requests.iter().filter(|r| r.url.path() == route).count();
        assert_eq!(count(TOKEN_PATH), 1);
        let routes = [
            // 16 recherches ; `find_article` à une date : 2 lectures et 1 numéro absent, plus le
            // numéro absent d'« aujourd'hui », qui retombe sur la recherche ; `find_by_number` :
            // l'ECLI présent (2), l'article 9 (2) et l'ECLI absent (1).
            ("/search", 16 + 4 + 5),
            ("/consult/juri", 7),
            // 3 pour la v0.1, plus 2 lectures de `find_article` à une date.
            ("/consult/getArticle", 3 + 2),
            ("/consult/lawDecree", 3),
            ("/consult/cnil", 3),
            ("/consult/kaliText", 3),
            ("/consult/acco", 3),
            // « Aujourd'hui » : 2 lectures et 1 numéro absent (200 à enveloppe nulle).
            ("/consult/getArticleWithIdAndNum", 3),
            // IDCC présent (2) ; NOR absent (1, 400 classé en liste vide).
            ("/consult/kaliContIdcc", 2),
            ("/consult/getJoWithNor", 1),
            // La table des codes se charge en deux pages, une seule fois pour les deux lectures.
            ("/list/code", 2),
            // 2 lectures, plus l'article absent, qui retombe sur `getArticleByCid` (400).
            ("/search/canonicalArticleVersion", 3),
            ("/consult/getArticleByCid", 1),
            // 2 lectures, plus l'article absent : 500, deux tentatives (04 §5.1).
            ("/consult/relatedLinksArticle", 2 + 2),
        ];
        for (endpoint, expected) in routes {
            assert_eq!(count(&format!("{API_BASE}{endpoint}")), expected, "{endpoint}");
        }
        let total = 1 + routes.iter().map(|(_, expected)| expected).sum::<usize>();
        assert_eq!(requests.len(), total);
    }
}
