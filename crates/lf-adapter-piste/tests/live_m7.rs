//! Tests live de M7 contre PISTE **sandbox** (04 §13.5, T-M7-02) : versions et liens d'un article.
//! Chaque test porte `#[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]` et son nom commence par
//! `live_` (profil nextest `live`). Sans identifiants, un test écrit un message sur stderr et
//! réussit.
//!
//! ```text
//! cargo nextest run -p lf-adapter-piste --test live_m7 --all-features --profile live --run-ignored only
//! ```
//!
//! Mêmes règles que `live.rs` : sandbox seulement (`LF_PISTE_ENV`, s'il est posé, doit valoir
//! `sandbox`), un corpus par test, une requête par seconde, rafale 1, une requête en vol et une
//! pause d'une seconde avant le premier appel.
//!
//! `live_versions_links_toc` du §13.5 ne couvre ici que les versions et les liens : la table des
//! matières (`tableMatieres`) n'est pas retenue en v0.1 (04 §7.1, option (b) de D-47) et n'a pas
//! de méthode de port M7. Les constats levés ici sont V-135 (formes de `canonicalArticleVersion`,
//! `getArticleByCid` et `relatedLinksArticle`), V-136 (forme de `relatedLinksArticle`) et, pour M7,
//! V-68 (réponse à un identifiant inexistant).
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
    use lf_domain::date::CivilDate;
    use lf_domain::id::ArticleId;
    use lf_domain::status::LegalStatus;
    use secrecy::SecretString;

    /// Article 9 du Code civil (« Chacun a droit au respect de sa vie privée »), dont les trois
    /// réponses ont été enregistrées le 2026-10-29 : trois versions, quatorze liens « cité par ».
    const ARTICLE: &str = "LEGIARTI000006419288";

    /// Identifiant bien formé et inexistant (04 §11.1).
    const UNKNOWN: &str = "LEGIARTI000000000000";

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

    /// V-135 : les versions de l'article 9 et ses liens, par les vraies réponses de la sandbox.
    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_versions_links_toc() {
        let Some(corpus) = sandbox("live_versions_links_toc").await else {
            return;
        };
        let id = ArticleId::parse(ARTICLE).unwrap();

        let versions = corpus.article_versions(&id).await.unwrap();
        assert!(versions.len() >= 3, "au moins trois versions de l'article 9");
        assert!(versions.iter().any(|version| version.id.as_str() == ARTICLE));
        let starts: Vec<Option<CivilDate>> =
            versions.iter().map(|version| version.date_debut).collect();
        assert!(starts.is_sorted(), "de la plus ancienne à la plus récente : {starts:?}");
        let last = versions.last().unwrap();
        assert_eq!(last.etat, Some(LegalStatus::Vigueur));
        assert_eq!(last.date_fin, None, "la version en vigueur n'a pas de fin");

        let links = corpus.related_links(&id).await.unwrap();
        assert!(!links.cited_by.is_empty(), "l'article 9 est cité par des textes");
        assert!(links.cited_by.iter().all(|link| link.title.is_some()));
    }

    /// V-68 pour M7 : ce que rend le port pour un article inexistant. Les versions : 200 sans
    /// aucun octet de `canonicalArticleVersion`, repli sur `getArticleByCid` (dont la réponse à un
    /// identifiant inconnu n'a pas été enregistrée) ; les liens : le 500 enregistré.
    #[tokio::test]
    #[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]
    async fn live_unknown_article_versions_and_links() {
        let Some(corpus) = sandbox("live_unknown_article_versions_and_links").await else {
            return;
        };
        let id = ArticleId::parse(UNKNOWN).unwrap();

        let versions = corpus.article_versions(&id).await;
        assert!(matches!(versions, Err(CorpusError::NotFound { .. })), "{versions:?}");

        let links = corpus.related_links(&id).await;
        assert!(matches!(links, Err(CorpusError::Upstream { status: Some(500), .. })), "{links:?}");
    }
}
