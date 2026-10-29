//! Sondes live de M5 contre PISTE **sandbox** (T-M5-06, 06 §5.3) : `#[ignore = "live: needs
//! LF_PISTE_CLIENT_ID/SECRET"]`, nom préfixé `live_` (profil nextest `live`). Sans identifiants, un
//! test écrit un message sur stderr et réussit. Les sondes **ne sont pas lancées par les agents** :
//! les identifiants sont réservés à l'intégrateur (F2).
//!
//! ```text
//! cargo nextest run -p lf-adapter-piste --test live_m5 --all-features --profile live --run-ignored only
//! ```
//!
//! Trois étiquettes de 09 §2 :
//! - **V-82** `live_type_pagination_page2_code` : la page 2 de `CODE_DATE` ne rejoue pas la page 1
//!   quand le mémo de `typePagination` suffit (04 §6.6) ;
//! - **V-83** `live_deep_pagination_limit` : la fenêtre atteignable de `/search` est de
//!   `REACHABLE_RESULTS` (10 000) résultats, et au-delà l'API ne rend pas de résultats ;
//! - **V-75** `live_response_sizes` : les réponses des textes de référence tiennent sous la borne
//!   de corps de 16 Mio (04 §4.3). Les codes entiers (Code du travail, 31,4 Mio) sont au-delà et
//!   restent couverts par `live_lawdecree_on_code_size` (`live.rs`).
//!
//! Sandbox seulement : `LF_PISTE_ENV`, s'il est posé, doit valoir `sandbox`. Un corpus par test,
//! limiteur d'une requête par seconde, rafale 1, une requête en vol, et une pause d'une seconde
//! avant le premier appel d'un test : au plus un appel par seconde sur toute la suite, aucun 429
//! provoqué. Les mesures sur les octets bruts (`PisteCorpus::exchange`) n'existent qu'avec la
//! feature `record` ; les conclusions sont écrites sur stderr, sans valeur d'identifiant.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod live_m5 {
    use std::num::{NonZeroU16, NonZeroU32};
    use std::time::Duration;

    use lf_adapter_piste::{
        PisteConfig, PisteCorpus, PisteCredentials, PisteEndpoints, PisteTuning, RateQuota,
    };
    use lf_application::ports::LegalCorpus;
    use lf_domain::coverage::PisteEnvironment;
    use lf_domain::date::VersionDate;
    use lf_domain::entity::SearchPage;
    use lf_domain::fond::Fond;
    use lf_domain::search::{MatchMode, PageSize, QueryText, REACHABLE_RESULTS, SearchQuery};
    use secrecy::SecretString;

    /// Date de version des recherches CODE.
    const VERSION: &str = "2026-10-28";

    /// Identifiants de la sandbox lus dans l'environnement, ou `None` avec un message sur stderr.
    /// L'environnement est vérifié d'abord : hors sandbox, aucune valeur d'identifiant n'est même
    /// lue, et aucune valeur n'est jamais affichée (les messages ne portent que le nom du test).
    #[expect(
        clippy::disallowed_methods,
        clippy::print_stderr,
        reason = "test live : identifiants lus dans l'environnement et diagnostic sur stderr, sans jamais afficher de valeur"
    )]
    fn credentials(test: &str) -> Option<PisteCredentials> {
        if std::env::var("LF_PISTE_ENV").is_ok_and(|env| env != "sandbox") {
            eprintln!("{test}: skipped, live tests run against the sandbox only");
            return None;
        }
        let id = std::env::var("LF_PISTE_CLIENT_ID").ok();
        let secret = std::env::var("LF_PISTE_CLIENT_SECRET").ok();
        let (Some(id), Some(secret)) = (id, secret) else {
            eprintln!(
                "{test}: skipped, set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (sandbox)"
            );
            return None;
        };
        Some(PisteCredentials::new(id, SecretString::from(secret)))
    }

    /// Corpus sandbox d'un test (une requête par seconde), après une pause d'une seconde.
    async fn sandbox(test: &str) -> Option<PisteCorpus> {
        let credentials = credentials(test)?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let env = PisteEnvironment::Sandbox;
        let user_agent = format!("mcp-legifrance/{} live-tests", env!("CARGO_PKG_VERSION"));
        let tuning = PisteTuning {
            rate: RateQuota::new(NonZeroU32::MIN, NonZeroU32::MIN),
            max_concurrency: NonZeroU16::MIN,
            request_timeout: Duration::from_secs(120),
            tool_deadline: Duration::from_secs(180),
            ..PisteTuning::default()
        };
        let config =
            PisteConfig::new(env, Ok(PisteEndpoints::official(env)), Some(credentials), user_agent)
                .with_tuning(tuning)
                .ok()?;
        Some(PisteCorpus::new(config))
    }

    /// Borne de corps par défaut de l'adapter (16 Mio, 04 §4.3), lue sur `PisteTuning` plutôt que
    /// recopiée : les sondes suivent la valeur livrée.
    fn body_limit() -> u64 {
        PisteTuning::default().max_body_bytes
    }

    /// Identifiants des résultats d'une page, dans l'ordre.
    fn ids(page: &SearchPage) -> Vec<String> {
        page.hits.iter().map(|hit| hit.id.as_str().to_owned()).collect()
    }

    /// V-82 : `S{CODE_DATE; "contrat de travail"; [DV(J)]; p=1}` puis `p=2`, par le port, donc avec
    /// le mémo de `typePagination` : la page 2 repart du mode que la page 1 a reçu. Sandbox,
    /// 2026-10-28 : la page 1 répond `DEFAUT`, la page 2 en `DEFAUT` donne dix autres textes, alors
    /// qu'une page 2 en `ARTICLE` rejoue la page 1 (tâche C5 du plan 2h). Le mémo suffit donc si
    /// la page 2 du port ne rejoue pas la page 1 ; sinon il faut le paramètre opaque `pagination`
    /// (classe « ajout », 05 §13.1 ligne 7).
    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    #[expect(
        clippy::print_stderr,
        reason = "test live : conclusion de V-82 sur stderr, sans valeur d'identifiant"
    )]
    async fn live_type_pagination_page2_code() {
        let Some(corpus) = sandbox("live_type_pagination_page2_code").await else {
            return;
        };
        let version = VersionDate::parse(VERSION).unwrap();
        let code_page = |page: u32| {
            SearchQuery::new(
                Fond::Code,
                QueryText::parse("contrat de travail").unwrap(),
                NonZeroU32::new(page).unwrap(),
                PageSize::default(),
                MatchMode::Any,
                None,
                Some(version),
            )
            .unwrap()
        };
        let first = corpus.search(&code_page(1)).await.unwrap();
        let second = corpus.search(&code_page(2)).await.unwrap();
        let (first_ids, second_ids) = (ids(&first), ids(&second));
        eprintln!(
            "V-82: page 1 answered typePagination {:?} ({} hits of {}), page 2 {:?} ({} hits)",
            first.pagination,
            first_ids.len(),
            first.total,
            second.pagination,
            second_ids.len()
        );
        assert!(first.total > 10, "the query has a second page: total {}", first.total);
        assert!(!first_ids.is_empty() && !second_ids.is_empty(), "both pages have hits");
        assert_eq!(first.total, second.total, "the total does not depend on the page");
        assert!(
            second_ids.iter().all(|id| !first_ids.contains(id)),
            "page 2 replays page 1: the memo is not enough, V-82 needs the opaque `pagination` parameter"
        );
        #[cfg(feature = "record")]
        raw::page_2_by_mode(&corpus, &first_ids).await;
    }

    /// V-83 : `S{JURI; "responsabilite"; n=100; p=100}` (positions 9 901 à 10 000) est atteignable.
    /// Sandbox, 2026-10-28 : 200 avec 100 résultats ; `p=101` (positions 10 001 et au-delà) : 503.
    /// `REACHABLE_RESULTS` est confirmé si la dernière page atteignable a des résultats et si la
    /// suivante n'en rend plus, par un statut d'erreur ou par une page vide.
    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    #[expect(
        clippy::print_stderr,
        reason = "test live : conclusion de V-83 sur stderr, sans valeur d'identifiant"
    )]
    async fn live_deep_pagination_limit() {
        let Some(corpus) = sandbox("live_deep_pagination_limit").await else {
            return;
        };
        let size = PageSize::MAX;
        let last_page = u32::try_from(REACHABLE_RESULTS / u64::from(size)).unwrap();
        let query = SearchQuery::new(
            Fond::Juri,
            QueryText::parse("responsabilite").unwrap(),
            NonZeroU32::new(last_page).unwrap(),
            PageSize::new(size).unwrap(),
            MatchMode::Any,
            None,
            None,
        )
        .unwrap();
        let page = corpus.search(&query).await.unwrap();
        eprintln!(
            "V-83: page {last_page} of {size} answered {} hits of {}",
            page.hits.len(),
            page.total
        );
        assert!(page.total >= REACHABLE_RESULTS, "the query reaches the window: {}", page.total);
        assert_eq!(
            page.hits.len(),
            usize::try_from(size).unwrap(),
            "the last reachable page is full"
        );
        #[cfg(feature = "record")]
        raw::page_beyond_the_window(&corpus, last_page + 1, size).await;
    }

    /// Mesures sur les octets bruts (`PisteCorpus::exchange`, feature `record`).
    #[cfg(feature = "record")]
    mod raw {
        use chrono::{NaiveDate, TimeZone};
        use chrono_tz::Europe::Paris;
        use serde_json::{Value, json};

        use super::*;

        /// Réponse brute : statut, corps JSON (ou `null` s'il n'en est pas) et taille en octets.
        async fn post(corpus: &PisteCorpus, endpoint: &str, body: &Value) -> (u16, Value, usize) {
            let reply = corpus.exchange(endpoint, body.to_string().as_bytes()).await.unwrap();
            let value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
            (reply.status, value, reply.body.len())
        }

        /// Identifiants des résultats d'une réponse `/search` brute.
        fn hit_ids(value: &Value) -> Vec<String> {
            value["results"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|hit| hit["titles"][0]["id"].as_str().map(str::to_owned))
                .collect()
        }

        /// Midi Europe/Paris de `VERSION`, en millisecondes (le `singleDate` de 04 §6.3).
        fn noon_paris() -> i64 {
            let day = NaiveDate::parse_from_str(VERSION, "%Y-%m-%d").unwrap();
            Paris
                .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
                .unwrap()
                .timestamp_millis()
        }

        /// Corps `/search` de 04 §6.1 pour `CODE_DATE`, à la page `page` et au mode `mode`.
        fn code_search(page: u32, mode: &str) -> Value {
            json!({"fond": "CODE_DATE", "recherche": {
                "champs": [{"typeChamp": "ALL", "criteres": [
                    {"typeRecherche": "UN_DES_MOTS", "valeur": "contrat de travail",
                     "operateur": "ET"}], "operateur": "ET"}],
                "filtres": [{"facette": "DATE_VERSION", "singleDate": noon_paris()}],
                "pageNumber": page, "pageSize": 10, "operateur": "ET", "sort": "PERTINENCE",
                "typePagination": mode}})
        }

        /// V-82, suite : la page 2 envoyée en `DEFAUT` puis en `ARTICLE`, comparée à la page 1
        /// (`first_ids`, lue par le port). `DEFAUT` ne rejoue pas la page 1 ; `ARTICLE` est consigné
        /// sur stderr sans assertion, car son effet est le défaut de l'API que le mémo évite.
        #[expect(
            clippy::print_stderr,
            reason = "test live : conclusion de V-82 sur stderr, sans valeur d'identifiant"
        )]
        pub(super) async fn page_2_by_mode(corpus: &PisteCorpus, first_ids: &[String]) {
            for mode in ["DEFAUT", "ARTICLE"] {
                let (status, body, _) = post(corpus, "/search", &code_search(2, mode)).await;
                let replayed = hit_ids(&body).iter().any(|id| first_ids.contains(id));
                eprintln!(
                    "V-82: page 2 sent as {mode}: status {status}, total {}, {} articles, replays page 1: {replayed}",
                    body["totalResultNumber"], body["totalArticleResultNumber"]
                );
                assert_eq!(status, 200, "page 2 sent as {mode}");
                assert!(mode != "DEFAUT" || !replayed, "page 2 sent as DEFAUT replays page 1");
            }
        }

        /// V-83, suite : la page qui suit la dernière atteignable ne rend aucun résultat, par un
        /// statut d'erreur (503 constaté) ou par une page vide.
        #[expect(
            clippy::print_stderr,
            reason = "test live : conclusion de V-83 sur stderr, sans valeur d'identifiant"
        )]
        pub(super) async fn page_beyond_the_window(corpus: &PisteCorpus, page: u32, size: u32) {
            let body = json!({"fond": "JURI", "recherche": {
                "champs": [{"typeChamp": "ALL", "criteres": [
                    {"typeRecherche": "UN_DES_MOTS", "valeur": "responsabilite",
                     "operateur": "ET"}], "operateur": "ET"}],
                "filtres": [],
                "pageNumber": page, "pageSize": size, "operateur": "ET", "sort": "PERTINENCE",
                "typePagination": "DEFAUT"}});
            let (status, value, bytes) = post(corpus, "/search", &body).await;
            let hits = hit_ids(&value).len();
            eprintln!(
                "V-83: page {page} of {size} answered status {status}, {hits} hits, {bytes} bytes"
            );
            assert!(
                status != 200 || hits == 0,
                "page {page} of {size} is beyond {REACHABLE_RESULTS} results yet answers {hits} hits: \
                 REACHABLE_RESULTS must be raised"
            );
        }

        /// V-75 : taille brute (décompressée) des réponses de référence face à la borne de corps
        /// de 16 Mio. Sandbox, 2026-10-28 : Code civil 6,2 Mio, accord d'entreprise 0,9 Mio,
        /// convention KALI 0,8 Mio, recherche à 100 résultats 0,5 Mio. Les codes entiers plus
        /// gros (Code du travail, 31,4 Mio) sont refusés nets par `get_act` : ils ne sont pas ici.
        #[tokio::test]
        #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
        #[expect(
            clippy::print_stderr,
            reason = "test live : tailles mesurées sur stderr, sans valeur d'identifiant"
        )]
        async fn live_response_sizes() {
            let Some(corpus) = sandbox("live_response_sizes").await else {
                return;
            };
            let search_100 = json!({"fond": "JURI", "recherche": {
                "champs": [{"typeChamp": "ALL", "criteres": [
                    {"typeRecherche": "UN_DES_MOTS", "valeur": "responsabilite",
                     "operateur": "ET"}], "operateur": "ET"}],
                "filtres": [],
                "pageNumber": 1, "pageSize": 100, "operateur": "ET", "sort": "PERTINENCE",
                "typePagination": "DEFAUT"}});
            let probes = [
                (
                    "lawDecree, Code civil",
                    "/consult/lawDecree",
                    json!({"textId": "LEGITEXT000006070721", "date": VERSION}),
                ),
                (
                    "lawDecree, loi pour une République numérique",
                    "/consult/lawDecree",
                    json!({"textId": "LEGITEXT000033205014", "date": VERSION}),
                ),
                ("getArticle", "/consult/getArticle", json!({"id": "LEGIARTI000006419288"})),
                (
                    "juri, Cour de cassation",
                    "/consult/juri",
                    json!({"textId": "JURITEXT000051743650"}),
                ),
                (
                    "juri, Conseil constitutionnel",
                    "/consult/juri",
                    json!({"textId": "CONSTEXT000052555275"}),
                ),
                (
                    "juri, Conseil d'État",
                    "/consult/juri",
                    json!({"textId": "CETATEXT000054178503"}),
                ),
                ("kaliText", "/consult/kaliText", json!({"id": "KALITEXT000049878950"})),
                ("acco", "/consult/acco", json!({"id": "ACCOTEXT000053643828"})),
                ("search, 100 résultats", "/search", search_100),
            ];
            let limit = body_limit();
            let mut largest = 0;
            for (label, endpoint, body) in probes {
                let (status, _, bytes) = post(&corpus, endpoint, &body).await;
                eprintln!("V-75: {label}: status {status}, {bytes} bytes of {limit}");
                // `exchange` rend tout statut sans erreur : un refus a un petit corps et passerait
                // la borne sans rien prouver.
                assert_eq!(status, 200, "{label}: HTTP status of {endpoint}");
                assert!(
                    bytes as u64 <= limit,
                    "{label}: {bytes} bytes exceed the body limit {limit}"
                );
                largest = largest.max(bytes);
            }
            eprintln!("V-75: largest reference response {largest} bytes, limit {limit} bytes");
        }
    }
}
