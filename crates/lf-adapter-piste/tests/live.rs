//! Tests live contre PISTE **sandbox** (04 §13.5, T-M2-12) : chaque test porte
//! `#[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]` et son nom commence par `live_` (profil
//! nextest `live`). Sans identifiants, un test écrit un message sur stderr et réussit.
//!
//! ```text
//! cargo nextest run -p lf-adapter-piste --test live --all-features --profile live --run-ignored only
//! ```
//!
//! Sandbox seulement : `LF_PISTE_ENV`, s'il est posé, doit valoir `sandbox`. Un corpus par test,
//! limiteur d'une requête par seconde, rafale 1, une requête en vol, et une pause d'une seconde
//! avant le premier appel d'un test (un corpus neuf a un limiteur neuf) : au plus un appel par
//! seconde sur toute la suite, aucun 429 provoqué. Les tests qui lisent les octets bruts
//! (`PisteCorpus::exchange`) n'existent qu'avec la feature `record`.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests
//! (`unwrap` permis, `clippy.toml`).

#[cfg(test)]
mod live {
    use std::num::{NonZeroU16, NonZeroU32};
    use std::time::Duration;

    use lf_adapter_piste::{
        PisteConfig, PisteCorpus, PisteCredentials, PisteEndpoints, PisteTuning, RateQuota,
    };
    use lf_application::ports::{CorpusError, LegalCorpus};
    use lf_domain::coverage::PisteEnvironment;
    use lf_domain::date::{DateRange, VersionDate};
    use lf_domain::fond::Fond;
    use lf_domain::id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId};
    use lf_domain::search::{MatchMode, PageSize, QueryText, SearchQuery};
    use secrecy::SecretString;

    /// Date de version des recherches LODA et CODE et de `get_act`.
    const VERSION: &str = "2026-10-28";

    /// Identifiants de la sandbox lus dans l'environnement, ou `None` avec un message sur stderr.
    /// L'environnement est vérifié d'abord : hors sandbox, aucune valeur d'identifiant n'est même
    /// lue, et aucune valeur n'est jamais affichée (les messages ne portent que le nom du test).
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
    async fn sandbox(test: &str, max_body_bytes: u64) -> Option<PisteCorpus> {
        let credentials = credentials(test)?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let env = PisteEnvironment::Sandbox;
        let user_agent = format!("mcp-legifrance/{} live-tests", env!("CARGO_PKG_VERSION"));
        let tuning = PisteTuning {
            rate: RateQuota::new(NonZeroU32::MIN, NonZeroU32::MIN),
            max_concurrency: NonZeroU16::MIN,
            max_body_bytes,
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

    /// Borne de corps par défaut de l'adapter (16 Mio, 04 §4.3).
    const DEFAULT_BODY: u64 = 16 << 20;

    fn query(fond: Fond, text: &str, mode: MatchMode, range: Option<DateRange>) -> SearchQuery {
        let version =
            matches!(fond, Fond::Loda | Fond::Code).then(|| VersionDate::parse(VERSION).unwrap());
        SearchQuery::new(
            fond,
            QueryText::parse(text).unwrap(),
            NonZeroU32::MIN,
            PageSize::new(3).unwrap(),
            mode,
            range,
            version,
        )
        .unwrap()
    }

    /// Les huit recherches des scénarios de la référence (recherche 01 §11.7).
    const SCENARIOS: [(Fond, &str); 8] = [
        (Fond::Loda, "republique numerique"),
        (Fond::Code, "vie privee"),
        (Fond::Juri, "responsabilite"),
        (Fond::Constit, "liberte"),
        (Fond::Cetat, "urbanisme"),
        (Fond::Cnil, "sanction"),
        (Fond::Kali, "convention collective"),
        (Fond::Acco, "teletravail"),
    ];

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_reference_scenarios() {
        let Some(corpus) = sandbox("live_reference_scenarios", DEFAULT_BODY).await else { return };
        for (fond, text) in SCENARIOS {
            let page = corpus.search(&query(fond, text, MatchMode::Any, None)).await.unwrap();
            assert!(page.total >= 1 && !page.hits.is_empty(), "{fond:?} {text}");
        }
        let version = VersionDate::parse(VERSION).unwrap();
        let act = LegiTextId::parse("LEGITEXT000033205014").unwrap();
        corpus.get_act(&act, version).await.unwrap();
        corpus.get_article(&ArticleId::parse("LEGIARTI000006419288").unwrap()).await.unwrap();
        corpus.get_decision(&DecisionId::parse("JURITEXT000051743650").unwrap()).await.unwrap();
        corpus.get_deliberation(&CnilTextId::parse("CNILTEXT000053352643").unwrap()).await.unwrap();
        corpus.get_convention(&KaliTextId::parse("KALITEXT000049878950").unwrap()).await.unwrap();
        let acco = AccoTextId::parse("ACCOTEXT000053643828").unwrap();
        corpus.get_company_agreement(&acco).await.unwrap();
    }

    /// Ce que rend le port pour chaque consultation v0.1 d'un identifiant bien formé et inexistant
    /// (04 §11) : le nom de la variante de `CorpusError`, ou `Ok`.
    async fn unknown(corpus: &PisteCorpus, digits: &str) -> Vec<(String, String)> {
        let name = |result: Result<(), CorpusError>| match result {
            Ok(()) => "Ok".to_owned(),
            Err(error) => {
                format!("{error:?}").split([' ', '{', '(']).next().unwrap_or("").to_owned()
            }
        };
        let version = VersionDate::parse(VERSION).unwrap();
        let mut seen = Vec::new();
        for prefix in ["JURITEXT", "CETATEXT", "CONSTEXT"] {
            let id = DecisionId::parse(&format!("{prefix}{digits}")).unwrap();
            seen.push((prefix.to_owned(), name(corpus.get_decision(&id).await.map(drop))));
        }
        let id = CnilTextId::parse(&format!("CNILTEXT{digits}")).unwrap();
        seen.push(("CNILTEXT".to_owned(), name(corpus.get_deliberation(&id).await.map(drop))));
        let id = LegiTextId::parse(&format!("LEGITEXT{digits}")).unwrap();
        seen.push(("LEGITEXT".to_owned(), name(corpus.get_act(&id, version).await.map(drop))));
        let id = ArticleId::parse(&format!("LEGIARTI{digits}")).unwrap();
        seen.push(("LEGIARTI".to_owned(), name(corpus.get_article(&id).await.map(drop))));
        let id = KaliTextId::parse(&format!("KALITEXT{digits}")).unwrap();
        seen.push(("KALITEXT".to_owned(), name(corpus.get_convention(&id).await.map(drop))));
        let id = AccoTextId::parse(&format!("ACCOTEXT{digits}")).unwrap();
        seen.push(("ACCOTEXT".to_owned(), name(corpus.get_company_agreement(&id).await.map(drop))));
        seen
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_unknown_ids_classification() {
        let Some(corpus) = sandbox("live_unknown_ids_classification", DEFAULT_BODY).await else {
            return;
        };
        let seen = unknown(&corpus, "000000000000").await;
        let expected: Vec<(String, String)> =
            seen.iter().map(|(prefix, _)| (prefix.clone(), "NotFound".to_owned())).collect();
        assert_eq!(seen, expected);
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_date_facets_reduce_total() {
        let Some(corpus) = sandbox("live_date_facets_reduce_total", DEFAULT_BODY).await else {
            return;
        };
        let year = DateRange::new(
            Some(VersionDate::parse("2020-01-01").unwrap()),
            Some(VersionDate::parse("2020-12-31").unwrap()),
        )
        .unwrap();
        for (fond, text) in SCENARIOS.into_iter().filter(|(fond, _)| *fond != Fond::Code) {
            let all = corpus.search(&query(fond, text, MatchMode::Any, None)).await.unwrap();
            let some = corpus.search(&query(fond, text, MatchMode::Any, year)).await.unwrap();
            assert!(some.total < all.total, "{fond:?}: {} !< {}", some.total, all.total);
        }
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_match_modes_totals() {
        let Some(corpus) = sandbox("live_match_modes_totals", DEFAULT_BODY).await else { return };
        let mut totals = Vec::new();
        for mode in [MatchMode::Any, MatchMode::All, MatchMode::Exact] {
            let page = corpus.search(&query(Fond::Juri, "contrat de travail", mode, None)).await;
            totals.push(page.unwrap().total);
        }
        assert!(totals[0] >= totals[1] && totals[1] >= totals[2], "{totals:?}");
    }

    /// Tests sur les octets bruts (`PisteCorpus::exchange`, feature `record`).
    #[cfg(feature = "record")]
    mod raw {
        use chrono::{NaiveDate, TimeZone};
        use chrono_tz::Europe::Paris;
        use lf_adapter_piste::record::mojibake_detected;
        use serde_json::{Value, json};

        use super::*;

        async fn post(corpus: &PisteCorpus, endpoint: &str, body: &Value) -> (u16, Value, usize) {
            let reply = corpus.exchange(endpoint, body.to_string().as_bytes()).await.unwrap();
            let value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
            (reply.status, value, reply.body.len())
        }

        /// Corps `/search` de 04 §6.1 pour CODE, `singleDate` donné en epoch ms.
        fn code_search(text: &str, single_date: i64, size: u32) -> Value {
            json!({"fond": "CODE_DATE", "recherche": {
                "champs": [{"typeChamp": "ALL", "criteres": [
                    {"typeRecherche": "UN_DES_MOTS", "valeur": text, "operateur": "ET"}], "operateur": "ET"}],
                "filtres": [{"facette": "DATE_VERSION", "singleDate": single_date}],
                "pageNumber": 1, "pageSize": size, "operateur": "ET", "sort": "PERTINENCE",
                "typePagination": "DEFAUT"}})
        }

        fn at(date: NaiveDate, hour: u32) -> i64 {
            Paris
                .from_local_datetime(&date.and_hms_opt(hour, 0, 0).unwrap())
                .unwrap()
                .timestamp_millis()
        }

        /// Identifiants des hits d'une réponse `/search` brute.
        fn hit_ids(value: &Value) -> Vec<String> {
            value["results"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|hit| hit["titles"][0]["id"].as_str().map(str::to_owned))
                .collect()
        }

        #[tokio::test]
        #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
        async fn live_no_mojibake_on_reference_ids() {
            let Some(corpus) = sandbox("live_no_mojibake_on_reference_ids", DEFAULT_BODY).await
            else {
                return;
            };
            let requests = [
                ("/consult/getArticle", json!({"id": "LEGIARTI000006419288"})),
                ("/consult/juri", json!({"textId": "JURITEXT000051743650"})),
                ("/consult/lawDecree", json!({"textId": "LEGITEXT000033205014", "date": VERSION})),
            ];
            for (endpoint, body) in requests {
                let reply = corpus.exchange(endpoint, body.to_string().as_bytes()).await.unwrap();
                assert_eq!(
                    (reply.status, mojibake_detected(&reply.body)),
                    (200, false),
                    "{endpoint}"
                );
                assert_eq!(reply.content_type.as_deref(), Some("application/json"));
            }
        }

        #[tokio::test]
        #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
        async fn live_singledate_noon_paris_semantics() {
            let Some(corpus) = sandbox("live_singledate_noon_paris_semantics", DEFAULT_BODY).await
            else {
                return;
            };
            // Un code dont la version courante a commencé le jour D, lu dans le suffixe du hit.
            let today = NaiveDate::parse_from_str(VERSION, "%Y-%m-%d").unwrap();
            let (_, now, _) =
                post(&corpus, "/search", &code_search("contrat", at(today, 12), 20)).await;
            let (text, start) = hit_ids(&now)
                .iter()
                .filter_map(|id| id.split_once('_'))
                .filter_map(|(text, suffix)| {
                    Some((text.to_owned(), NaiveDate::parse_from_str(suffix, "%d-%m-%Y").ok()?))
                })
                .filter(|(_, start)| *start <= today)
                .max_by_key(|(_, start)| *start)
                .unwrap();
            let mut suffixes = Vec::new();
            for instant in [at(start.pred_opt().unwrap(), 12), at(start, 0), at(start, 12)] {
                let (_, page, _) =
                    post(&corpus, "/search", &code_search("contrat", instant, 20)).await;
                let id = hit_ids(&page).into_iter().find(|id| id.starts_with(&text)).unwrap();
                suffixes.push(id == format!("{text}_{}", start.format("%d-%m-%Y")));
            }
            // Veille à midi et minuit de Paris : version précédente ; midi de Paris : version de D.
            assert_eq!(suffixes, [false, false, true], "{text} version of {start}");
        }

        /// Taille et durée de `/consult/lawDecree` sur deux codes entiers (seuils de 04 §7.3, V-96).
        ///
        /// ROUGE à l'essai contre la vraie sandbox, par construction : les seuils (4 Mio, 10 s)
        /// sont ceux du plan et ne sont pas assouplis ici. Le sort de ce test est celui de la
        /// tâche conditionnelle C2 du plan 2h, décidée en PR 2 d'après les mesures ; d'ici là, un
        /// échec de ce seul test (sur la sandbox) est attendu et ne doit pas faire croire à une
        /// régression. La borne de corps (64 Mio) n'est que le filet qui laisse la mesure aboutir.
        #[tokio::test]
        #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
        async fn live_lawdecree_on_code_size() {
            let Some(corpus) = sandbox("live_lawdecree_on_code_size", 64 << 20).await else {
                return;
            };
            let mut measures = Vec::new();
            for code in ["LEGITEXT000006070721", "LEGITEXT000006072050"] {
                let body = json!({"textId": code, "date": VERSION}).to_string();
                let reply = corpus.exchange("/consult/lawDecree", body.as_bytes()).await.unwrap();
                eprintln!("{code}: {} bytes in {} ms", reply.body.len(), reply.elapsed.as_millis());
                measures.push((
                    code,
                    reply.body.len() <= 4 << 20,
                    reply.elapsed <= Duration::from_secs(10),
                ));
            }
            // Seuils de 04 §7.3 : 4 Mio et 10 s (V-96).
            assert_eq!(
                measures,
                [("LEGITEXT000006070721", true, true), ("LEGITEXT000006072050", true, true)]
            );
        }

        #[tokio::test]
        #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
        async fn live_texte_html_presence() {
            let Some(corpus) = sandbox("live_texte_html_presence", DEFAULT_BODY).await else {
                return;
            };
            let (_, article, _) =
                post(&corpus, "/consult/getArticle", &json!({"id": "LEGIARTI000006419288"})).await;
            let (_, decision, _) =
                post(&corpus, "/consult/juri", &json!({"textId": "JURITEXT000051743650"})).await;
            assert!(article["article"]["texteHtml"].is_string());
            assert!(decision["text"]["texteHtml"].is_string());
        }

        #[tokio::test]
        #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
        async fn live_dates_are_midnight_utc_and_eli_fields_are_null() {
            let Some(corpus) =
                sandbox("live_dates_are_midnight_utc_and_eli_fields_are_null", DEFAULT_BODY).await
            else {
                return;
            };
            let (_, value, _) =
                post(&corpus, "/consult/getArticle", &json!({"id": "LEGIARTI000006419288"})).await;
            let article = &value["article"];
            for key in ["dateDebut", "dateFin"] {
                assert_eq!(article[key].as_i64().map(|ms| ms % 86_400_000), Some(0), "{key}");
            }
            assert_eq!((&article["idEli"], &article["idEliAlias"]), (&Value::Null, &Value::Null));
        }
    }
}
