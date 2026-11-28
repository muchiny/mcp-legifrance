//! Test en direct de Judilibre (T-MJ-23, ADR-0035) : une recherche puis la lecture de la première
//! décision trouvée, contre l'API PISTE de **production**, où l'application du propriétaire est
//! abonnée à Judilibre. Hors CI : `live.yml` ne lance pas ce fichier.
//!
//! Lancé par le propriétaire seulement, identifiants dans l'environnement, jamais affichés :
//!
//! ```text
//! LF_PISTE_ENV=production cargo nextest run -p lf-adapter-piste --test live_judilibre \
//!     --all-features --profile live --run-ignored only
//! ```
//!
//! Sans `LF_PISTE_ENV=production` ou sans identifiants, le test écrit un message sur stderr et
//! réussit. Coût : un jeton et deux requêtes, à une requête par seconde.
//!
//! Tout le fichier est sous `cfg(test)` : les aides partagent les règles de lint des tests.

#[cfg(test)]
mod live {
    use std::num::{NonZeroU16, NonZeroU32};
    use std::time::Duration;

    use lf_adapter_piste::{
        PisteConfig, PisteCorpus, PisteCredentials, PisteEndpoints, PisteJudilibre, PisteTuning,
        RateQuota,
    };
    use lf_application::ports::JudicialOpenData;
    use lf_domain::coverage::PisteEnvironment;
    use lf_domain::judilibre::{CaseLawFilters, CaseLawQuery, CaseLawSort, CourtSet};
    use lf_domain::search::{MatchMode, PageSize, QueryText};
    use secrecy::SecretString;

    /// Identifiants de production lus dans l'environnement, ou `None` avec un message sur stderr.
    /// L'environnement est vérifié d'abord : sans `LF_PISTE_ENV=production`, aucune valeur
    /// d'identifiant n'est lue, et aucune valeur n'est jamais affichée.
    #[expect(
        clippy::disallowed_methods,
        clippy::print_stderr,
        reason = "test en direct : identifiants lus dans l'environnement et diagnostic sur stderr, sans jamais afficher de valeur"
    )]
    fn credentials(test: &str) -> Option<PisteCredentials> {
        if std::env::var("LF_PISTE_ENV").ok().as_deref() != Some("production") {
            eprintln!("{test}: skipped, set LF_PISTE_ENV=production (owner only)");
            return None;
        }
        let id = std::env::var("LF_PISTE_CLIENT_ID").ok();
        let secret = std::env::var("LF_PISTE_CLIENT_SECRET").ok();
        let (Some(id), Some(secret)) = (id, secret) else {
            eprintln!("{test}: skipped, set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET");
            return None;
        };
        Some(PisteCredentials::new(id, SecretString::from(secret)))
    }

    /// Adapter de production, une requête par seconde, une seule en vol.
    fn production(test: &str) -> Option<PisteJudilibre> {
        let credentials = credentials(test)?;
        let env = PisteEnvironment::Production;
        let user_agent = format!("mcp-legifrance/{} live-tests", env!("CARGO_PKG_VERSION"));
        let tuning = PisteTuning {
            rate: RateQuota::new(NonZeroU32::MIN, NonZeroU32::MIN),
            max_concurrency: NonZeroU16::MIN,
            request_timeout: Duration::from_secs(60),
            tool_deadline: Duration::from_secs(120),
            ..PisteTuning::default()
        };
        let config =
            PisteConfig::new(env, Ok(PisteEndpoints::official(env)), Some(credentials), user_agent)
                .with_tuning(tuning)
                .ok()?;
        Some(PisteCorpus::new(config).judilibre())
    }

    #[tokio::test]
    #[ignore = "live: production PISTE application subscribed to Judilibre"]
    async fn live_judilibre_search_then_read() {
        let Some(judilibre) = production("live_judilibre_search_then_read") else {
            return;
        };
        let filters = CaseLawFilters {
            mode: MatchMode::All,
            courts: CourtSet::ALL,
            part: None,
            date_from: None,
            date_to: None,
            sort: CaseLawSort::Relevance,
        };
        let query = CaseLawQuery::new(
            QueryText::parse("donation indirecte assurance-vie").unwrap(),
            filters,
            NonZeroU32::MIN,
            PageSize::new(3).unwrap(),
        )
        .unwrap();
        let page = judilibre.search(&query).await.unwrap();
        assert!(page.total >= 1 && !page.hits.is_empty(), "total {}", page.total);
        let first = &page.hits[0];
        assert!(!first.mention.court.is_empty() && !first.mention.date.is_empty());
        // V-241 : chaque siège arrive en clair, jamais en code entre parenthèses.
        for hit in &page.hits {
            let court = &hit.mention.court;
            assert!(!court.contains(" ("), "{court}");
        }
        let doc = judilibre.get(&first.id).await.unwrap();
        assert_eq!(doc.id, first.id);
        assert_eq!(doc.mention.court, first.mention.court);
        assert_ne!(doc.text, "");
    }
}
