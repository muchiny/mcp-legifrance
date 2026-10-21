//! Port sortant unique vers le corpus juridique (02 §5.1, 00 §3) : PISTE en v0.x, DILA en M9.
//!
//! Les sept méthodes de la v0.1 ; `find_article`, `find_by_number` et `list_codes` arrivent en
//! M6, `article_versions` et `related_links` en M7.

use std::time::Duration;

use lf_domain::{
    date::VersionDate,
    entity::{Act, ArticleText, CompanyAgreement, Convention, Decision, Deliberation, SearchPage},
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::SearchQuery,
};
use thiserror::Error;

/// Port sortant vers le corpus juridique.
///
/// Aucune méthode par défaut : un décorateur ou un faux qui oublie une méthode ne compile
/// pas. Les futurs sont `+ Send`, car rmcp boxe les futurs d'outils en
/// `Pin<Box<dyn Future + Send>>` (02 §3.2, piège 2). Une implémentation écrit des `async fn`.
pub trait LegalCorpus: Send + Sync + 'static {
    /// Recherche (`/search`).
    fn search(
        &self,
        query: &SearchQuery,
    ) -> impl Future<Output = Result<SearchPage, CorpusError>> + Send;

    /// Texte LODA consolidé à une date (`/consult/lawDecree`).
    fn get_act(
        &self,
        id: &LegiTextId,
        date: VersionDate,
    ) -> impl Future<Output = Result<Act, CorpusError>> + Send;

    /// Article (`/consult/getArticle`).
    fn get_article(
        &self,
        id: &ArticleId,
    ) -> impl Future<Output = Result<ArticleText, CorpusError>> + Send;

    /// Décision de justice (`/consult/juri`).
    fn get_decision(
        &self,
        id: &DecisionId,
    ) -> impl Future<Output = Result<Decision, CorpusError>> + Send;

    /// Délibération CNIL (`/consult/cnil`).
    fn get_deliberation(
        &self,
        id: &CnilTextId,
    ) -> impl Future<Output = Result<Deliberation, CorpusError>> + Send;

    /// Convention collective (`/consult/kaliText`).
    fn get_convention(
        &self,
        id: &KaliTextId,
    ) -> impl Future<Output = Result<Convention, CorpusError>> + Send;

    /// Accord d'entreprise (`/consult/acco`).
    fn get_company_agreement(
        &self,
        id: &AccoTextId,
    ) -> impl Future<Output = Result<CompanyAgreement, CorpusError>> + Send;
}

/// Erreurs du port.
///
/// `Clone` est requis : moka renvoie `Arc<E>` aux appels coalescés (02 §7.2). Les `detail`
/// ne contiennent jamais de secret, de jeton ni d'URL avec paramètres ; ils servent aux
/// journaux, le message vu par le modèle étant rédigé par l'application (02 §9.3).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CorpusError {
    /// Ressource introuvable (classification de 04 §11.2).
    #[error("not found: {what}")]
    NotFound {
        /// Ce qui manque, ex. l'identifiant.
        what: String,
    },
    /// Requête refusée : `Some(400)` pour un 400 PISTE sans marqueur « introuvable » ;
    /// `None` pour la garde défensive de l'adapter, sans appel réseau. Jamais réessayée.
    #[error("request rejected by Legifrance")]
    InvalidRequest {
        /// Statut HTTP, s'il y en a un.
        status: Option<u16>,
        /// Détail pour les journaux.
        detail: String,
    },
    /// Erreur amont ; `status` vaut `None` pour une erreur réseau. `retryable` ne sert qu'au
    /// message (00 §5).
    #[error("upstream error")]
    Upstream {
        /// Statut HTTP, s'il y en a un.
        status: Option<u16>,
        /// Vrai si une nouvelle tentative a des chances d'aboutir.
        retryable: bool,
        /// Détail pour les journaux.
        detail: String,
    },
    /// Délai de requête dépassé après réessais, ou délai restant épuisé.
    #[error("upstream timeout")]
    Timeout,
    /// Quota atteint.
    #[error("rate limited")]
    RateLimited {
        /// Attente annoncée par `Retry-After` ou par le limiteur.
        retry_after: Option<Duration>,
    },
    /// Identifiants refusés (OAuth 400/401, second 401 de l'API, 403 de l'API).
    #[error("credentials rejected")]
    Auth {
        /// Détail pour les journaux.
        detail: String,
    },
    /// Configuration invalide ou absente (identifiants, OAuth 404, route inconnue).
    #[error("configuration error: {detail}")]
    Config {
        /// Détail pour les journaux.
        detail: String,
    },
    /// Réponse illisible (JSON invalide, corps trop gros, forme inattendue).
    #[error("undecodable upstream response: {detail}")]
    Decode {
        /// Détail pour les journaux.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::{
        fond::Fond,
        search::{MatchMode, PageSize, QueryText},
    };
    use pretty_assertions::assert_eq;
    use std::{
        future::ready,
        num::NonZeroU32,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    /// Faux corpus minimal : chaque méthode échoue en nommant la méthode appelée. Il prouve
    /// que le port s'implémente par des `async fn` dont les futurs sont `Send`.
    struct Named;

    fn named(method: &str) -> CorpusError {
        CorpusError::NotFound { what: method.to_owned() }
    }

    impl LegalCorpus for Named {
        async fn search(&self, _: &SearchQuery) -> Result<SearchPage, CorpusError> {
            ready(Err(named("search"))).await
        }
        async fn get_act(&self, _: &LegiTextId, _: VersionDate) -> Result<Act, CorpusError> {
            ready(Err(named("get_act"))).await
        }
        async fn get_article(&self, _: &ArticleId) -> Result<ArticleText, CorpusError> {
            ready(Err(named("get_article"))).await
        }
        async fn get_decision(&self, _: &DecisionId) -> Result<Decision, CorpusError> {
            ready(Err(named("get_decision"))).await
        }
        async fn get_deliberation(&self, _: &CnilTextId) -> Result<Deliberation, CorpusError> {
            ready(Err(named("get_deliberation"))).await
        }
        async fn get_convention(&self, _: &KaliTextId) -> Result<Convention, CorpusError> {
            ready(Err(named("get_convention"))).await
        }
        async fn get_company_agreement(
            &self,
            _: &AccoTextId,
        ) -> Result<CompanyAgreement, CorpusError> {
            ready(Err(named("get_company_agreement"))).await
        }
    }

    /// Exige un futur `Send` et le sonde une fois : le faux corpus n'attend rien, il est donc
    /// prêt au premier `poll`.
    fn poll_once<F: Future + Send>(future: F) -> Poll<F::Output> {
        pin!(future).poll(&mut Context::from_waker(Waker::noop()))
    }

    async fn every_method<C: LegalCorpus>(corpus: &C) -> Vec<CorpusError> {
        let query = SearchQuery::new(
            Fond::Juri,
            QueryText::parse("bail").unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            None,
        )
        .unwrap();
        vec![
            corpus.search(&query).await.unwrap_err(),
            corpus
                .get_act(&"LEGITEXT000006070721".parse().unwrap(), VersionDate::MIN)
                .await
                .unwrap_err(),
            corpus.get_article(&"LEGIARTI000006419288".parse().unwrap()).await.unwrap_err(),
            corpus.get_decision(&"JURITEXT000037999394".parse().unwrap()).await.unwrap_err(),
            corpus.get_deliberation(&"CNILTEXT000053352643".parse().unwrap()).await.unwrap_err(),
            corpus.get_convention(&"KALITEXT000005652329".parse().unwrap()).await.unwrap_err(),
            corpus
                .get_company_agreement(&"ACCOTEXT000037731479".parse().unwrap())
                .await
                .unwrap_err(),
        ]
    }

    #[test]
    fn port_is_implemented_by_async_fns_with_send_futures() {
        let errors = poll_once(every_method(&Named));
        let names = [
            "search",
            "get_act",
            "get_article",
            "get_decision",
            "get_deliberation",
            "get_convention",
            "get_company_agreement",
        ];
        assert_eq!(errors, Poll::Ready(names.map(named).to_vec()));
    }

    #[test]
    fn messages_carry_no_secret_and_name_the_failure() {
        let cases = [
            (
                CorpusError::NotFound { what: "JURITEXT000037999394".into() },
                "not found: JURITEXT000037999394",
            ),
            (
                CorpusError::InvalidRequest { status: Some(400), detail: "x".into() },
                "request rejected by Legifrance",
            ),
            (
                CorpusError::Upstream { status: None, retryable: true, detail: "x".into() },
                "upstream error",
            ),
            (CorpusError::Timeout, "upstream timeout"),
            (
                CorpusError::RateLimited { retry_after: Some(Duration::from_secs(12)) },
                "rate limited",
            ),
            (CorpusError::Auth { detail: "x".into() }, "credentials rejected"),
            (
                CorpusError::Config { detail: "credentials missing".into() },
                "configuration error: credentials missing",
            ),
            (
                CorpusError::Decode { detail: "invalid UTF-8 at byte 3".into() },
                "undecodable upstream response: invalid UTF-8 at byte 3",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }
}
