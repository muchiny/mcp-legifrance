//! Tests live de M6 contre PISTE **sandbox** (04 §13.5) : `find_article`, `find_by_number` et
//! `list_codes`. Chaque test porte `#[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]` et son nom
//! commence par `live_` (profil nextest `live`). Sans identifiants, un test écrit un message sur
//! stderr et réussit.
//!
//! ```text
//! cargo nextest run -p lf-adapter-piste --test live_m6 --all-features --profile live --run-ignored only
//! ```
//!
//! Mêmes règles que `live.rs` : sandbox seulement (`LF_PISTE_ENV`, s'il est posé, doit valoir
//! `sandbox`), un corpus par test, limiteur d'une requête par seconde et rafale 1, une requête en
//! vol, une pause d'une seconde avant le premier appel d'un test. Ces tests relisent les
//! scénarios enregistrés en fixtures (`tests/fixtures/recorded/**/m6`, 2026-10-29) : le Code civil
//! (`LEGITEXT000006070721`, article 9), la table des 108 codes, l'IDCC 2121, le NOR
//! `AGRX2502679L`, l'ECLI `ECLI:FR:CCASS:2025:C100399`. Les deux endpoints ELI ne sont pas
//! essayés : leur forme est **NON VÉRIFIÉE** [V-133 · eli-redirection-endpoints].
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
    use lf_application::ports::{ArticleAt, LegalCorpus};
    use lf_domain::coverage::PisteEnvironment;
    use lf_domain::date::VersionDate;
    use lf_domain::fond::Fond;
    use lf_domain::id::LegiTextId;
    use lf_domain::search::{NumberKind, NumberQuery, NumberValue};
    use lf_domain::status::LegalStatus;

    use secrecy::SecretString;

    /// Date des recherches par numéro et de `find_article`.
    const VERSION: &str = "2026-10-28";

    /// Code civil.
    const CIVIL_CODE: &str = "LEGITEXT000006070721";

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

    fn version() -> VersionDate {
        VersionDate::parse(VERSION).unwrap()
    }

    fn query(fond: Fond, kind: NumberKind, value: &str) -> NumberQuery {
        NumberQuery::new(fond, kind, NumberValue::parse(value).unwrap(), version()).unwrap()
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_list_codes_loads_one_table_with_repealed_codes() {
        let Some(corpus) = sandbox("live_list_codes_loads_one_table_with_repealed_codes").await
        else {
            return;
        };
        let codes = corpus.list_codes().await.unwrap();
        assert!(codes.len() >= 100, "{} codes", codes.len());
        let civil = codes.iter().find(|code| code.id.as_str() == CIVIL_CODE).unwrap();
        assert_eq!(civil.title, "Code civil");
        assert_eq!(civil.etat, Some(LegalStatus::Vigueur));
        assert!(codes.iter().any(|code| code.etat == Some(LegalStatus::Abroge)));
        // Le second appel sert la table : même contenu, sans nouvelle requête de liste.
        assert_eq!(corpus.list_codes().await.unwrap(), codes);
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_find_article_today_reads_the_current_version() {
        let Some(corpus) = sandbox("live_find_article_today_reads_the_current_version").await
        else {
            return;
        };
        let civil = LegiTextId::parse(CIVIL_CODE).unwrap();
        let article = corpus.find_article(&civil, "9", ArticleAt::Today(version())).await.unwrap();
        assert_eq!(article.num.as_deref(), Some("9"));
        assert_eq!(article.etat, Some(LegalStatus::Vigueur));
        assert!(article.text.as_str().starts_with("Chacun a droit au respect de sa vie privée."));
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_find_article_on_a_date_goes_through_the_exact_search() {
        let Some(corpus) =
            sandbox("live_find_article_on_a_date_goes_through_the_exact_search").await
        else {
            return;
        };
        let civil = LegiTextId::parse(CIVIL_CODE).unwrap();
        let at = VersionDate::parse("2020-06-01").unwrap();
        let article = corpus.find_article(&civil, "9", ArticleAt::On(at)).await.unwrap();
        assert_eq!(article.num.as_deref(), Some("9"));
        assert!(article.text.as_str().contains("vie privée"));
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_find_by_number_lists_the_codes_that_carry_an_article() {
        let Some(corpus) =
            sandbox("live_find_by_number_lists_the_codes_that_carry_an_article").await
        else {
            return;
        };
        let hits =
            corpus.find_by_number(&query(Fond::Code, NumberKind::Article, "9")).await.unwrap();
        assert!(hits.len() >= 2, "{} hits", hits.len());
        assert!(hits.iter().any(|hit| hit.id.as_str() == CIVIL_CODE));
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_find_by_number_case_decision_and_ecli() {
        let Some(corpus) = sandbox("live_find_by_number_case_decision_and_ecli").await else {
            return;
        };
        let case =
            corpus.find_by_number(&query(Fond::Juri, NumberKind::Case, "24-13.470")).await.unwrap();
        assert!(case.iter().any(|hit| hit.id.as_str() == "JURITEXT000051743650"));
        let ecli = corpus
            .find_by_number(&query(Fond::Juri, NumberKind::Ecli, "ECLI:FR:CCASS:2025:C100399"))
            .await
            .unwrap();
        assert_eq!(ecli.len(), 1);
        assert_eq!(ecli[0].id.as_str(), "JURITEXT000051743650");
        let unknown = corpus
            .find_by_number(&query(Fond::Juri, NumberKind::Ecli, "ECLI:FR:CCASS:1900:C000000"))
            .await
            .unwrap();
        assert_eq!(unknown, Vec::new());
    }

    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_find_by_number_idcc_nor_and_unknowns() {
        let Some(corpus) = sandbox("live_find_by_number_idcc_nor_and_unknowns").await else {
            return;
        };
        let idcc =
            corpus.find_by_number(&query(Fond::Kali, NumberKind::Idcc, "2121")).await.unwrap();
        assert_eq!(idcc.len(), 1);
        assert_eq!(idcc[0].id.as_str(), "KALITEXT000005654011");
        // IDCC inconnu : l'API répond 500 à la consultation, la recherche KALI prend le relais.
        let unknown =
            corpus.find_by_number(&query(Fond::Kali, NumberKind::Idcc, "99999")).await.unwrap();
        assert_eq!(unknown, Vec::new());
        // Un texte du seul Journal officiel n'a pas de version consolidée : aucun hit LODA.
        let nor = corpus
            .find_by_number(&query(Fond::Loda, NumberKind::Nor, "AGRX2502679L"))
            .await
            .unwrap();
        assert_eq!(nor, Vec::new());
        let unknown_nor = corpus
            .find_by_number(&query(Fond::Loda, NumberKind::Nor, "ZZZZ0000000Z"))
            .await
            .unwrap();
        assert_eq!(unknown_nor, Vec::new());
    }
}
