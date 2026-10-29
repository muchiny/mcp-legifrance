//! Suite de contrat du port `LegalCorpus` sur `PisteCorpus` (T-M3-05, 06 §2.3) : le vrai client
//! HTTP de l'adapter parle à un `wiremock` qui sert les seize réponses de référence
//! (`tests/fixtures/reference`) sur les mêmes cas que `FakeCorpus` et `CachedCorpus`.
//!
//! Les cas sont ceux de `ContractFixtures::reference` : les identifiants des fixtures. Une
//! consultation sert la fixture de son identifiant ; une recherche, celle de son fond ; tout autre
//! identifiant reçoit la réponse que PISTE donne à un identifiant inconnu (400, enregistrée en
//! sandbox), que l'adapter classe en `NotFound` (04 §11.2). Aucun réseau hors de la boucle locale.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod contract {
    use std::num::NonZeroU32;

    use lf_adapter_piste::{
        PisteConfig, PisteCorpus, PisteCredentials, PisteEndpoints, PisteTuning, RateQuota,
    };
    use lf_application::contract::{ContractFixtures, corpus_contract_m5};
    use lf_domain::coverage::PisteEnvironment;
    use lf_domain::fond::Fond;
    use lf_domain::id::DecisionId;
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

    /// Tout autre corps de `endpoint` reçoit le 400 d'un identifiant inconnu, moins prioritaire
    /// que les fixtures.
    async fn unknown(server: &MockServer, endpoint: &str, message: &'static str) {
        Mock::given(method("POST"))
            .and(path(format!("{API_BASE}{endpoint}")))
            .respond_with(json_reply(400, message.as_bytes()))
            .with_priority(10)
            .mount(server)
            .await;
    }

    /// Réponses enregistrées en sandbox le 2026-10-28 (V-68) : le message de PISTE pour un
    /// identifiant bien formé et inexistant. L'accord d'entreprise a le sien (message de comptage).
    const UNKNOWN: &str = r#"{"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null}"#;
    const UNKNOWN_ACCO: &str = r#"{"error":400,"status":"Bad Request","message":"Le nombre de résultat retourné par le moteur de recherche n'est pas égal à 1.","errorCode":null}"#;

    /// Serveur simulé : le jeton, les fixtures de `fx`, et le 400 pour tout le reste.
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
            unknown(&server, endpoint, UNKNOWN).await;
        }
        unknown(&server, "/consult/kaliText", UNKNOWN).await;
        unknown(&server, "/consult/acco", UNKNOWN_ACCO).await;
        server
    }

    /// `PisteCorpus` ordinaire (horloge et attente réelles) vers `server`, au débit maximal permis
    /// pour que les 38 appels de la suite ne s'attendent pas.
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
    async fn contract_on_piste_corpus_with_fixtures() {
        let fx = ContractFixtures::reference();
        let server = piste_server(&fx).await;
        let corpus = corpus_for(&server);
        corpus_contract_m5(&corpus, &fx).await;

        // Les fixtures servies sont celles des identifiants demandés : aucun identifiant renvoyé
        // ne diffère et aucun résultat n'est écarté. Une partie des fixtures porte du mojibake, que
        // l'adapter répare et compte : le réglage n'est pas coupé.
        let stats = corpus.stats();
        assert_eq!((stats.id_mismatch, stats.dropped_hits), (0, 0), "{stats:?}");
        assert!(stats.mojibake_repairs > 0, "the reference fixtures carry mojibake: {stats:?}");
        // Un seul jeton pour toute la suite, puis exactement les appels de la suite : 16
        // recherches, 7 décisions et 3 appels pour chacune des cinq autres consultations. Aucun
        // appel de plus (ni N+1, ni URL lue dans une réponse).
        let requests = server.received_requests().await.unwrap();
        let count = |route: &str| requests.iter().filter(|r| r.url.path() == route).count();
        assert_eq!(count(TOKEN_PATH), 1);
        assert_eq!(count(&format!("{API_BASE}/search")), 16);
        assert_eq!(count(&format!("{API_BASE}/consult/juri")), 7);
        for endpoint in ["lawDecree", "getArticle", "cnil", "kaliText", "acco"]
            .map(|name| format!("{API_BASE}/consult/{name}"))
        {
            assert_eq!(count(&endpoint), 3, "{endpoint}");
        }
        assert_eq!(requests.len(), 1 + 16 + 7 + 5 * 3);
    }
}
