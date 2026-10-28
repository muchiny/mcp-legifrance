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

/// Endpoint qui a refusé les identifiants ([`CorpusError::Auth`]).
///
/// Le message vu par le modèle en dépend (02 §9.2 lignes 7, 11 et 12 ; 05 §7.2) : un 403 de
/// l'API dit « accès refusé » (application non abonnée), tout autre refus « identifiants
/// refusés ». Typé pour que `lf-application::messages` n'ait jamais à lire le `detail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthOrigin {
    /// Endpoint du jeton OAuth : 400, 401 ou 403.
    Token,
    /// API Légifrance : second 401 après le renouvellement du jeton, ou 403 dont le corps
    /// n'est pas une page HTML.
    Api,
}

/// Nature d'une erreur de configuration ([`CorpusError::Config`]).
///
/// Chaque nature a son message pour le modèle (05 §7.2), que `lf-application::messages`
/// rédige seul : le `detail` est un texte de journal (sans point final, parfois avec une URL
/// sans paramètres, parfois plus court que 05 §7.2), jamais recopié ni analysé, sauf pour
/// [`ConfigKind::Settings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKind {
    /// Identifiants absents, sans E/S (02 §9.2 ligne 5).
    CredentialsMissing,
    /// Réponse OAuth 200 inexploitable : sans `access_token` (ou vide), jeton qui n'est pas une
    /// valeur d'en-tête, ou JSON illisible ; un seul message pour le modèle (ligne 9).
    OAuthResponse,
    /// 404 ou 405 de routage sur l'API : `/search`, page HTML, hôte non officiel (ligne 14).
    ApiEndpoint,
    /// 404 ou 405 de l'endpoint du jeton (ligne 8).
    OAuthEndpoint,
    /// Valeur de configuration invalide, mode dégradé (ligne 6) : `LF_PISTE_ENV` invalide, URL
    /// refusée dont `EnvMismatch`, client HTTP non constructible. Le `detail` est vu par le
    /// modèle, coupé à 500 caractères par `messages` : l'adapter le rédige pour être lu et en
    /// répond (aucun secret, aucune valeur brute : `ConfigProblem::invalid_env` ne garde pas la
    /// valeur de `LF_PISTE_ENV`). C'est le `Display` d'un `ConfigProblem` (aligné sur 05 §7.2)
    /// ou, pour un client HTTP non constructible, la chaîne de l'erreur de construction (écart
    /// connu : ce texte-là n'est pas rédigé pour le modèle, mais l'adapter ne l'a jamais
    /// distingué).
    Settings,
}

/// Erreurs du port.
///
/// `Clone` est requis : moka renvoie `Arc<E>` aux appels coalescés (02 §7.2). Les `detail`
/// ne contiennent jamais de secret, de jeton ni d'URL avec paramètres. Le message vu par le
/// modèle est rédigé par l'application (02 §9.3) à partir des seuls champs typés (variante,
/// statut, [`AuthOrigin`], [`ConfigKind`]), à deux exceptions près : le `detail`
/// d'[`CorpusError::InvalidRequest`] (coupé à 200 caractères) et celui d'un
/// [`ConfigKind::Settings`] (coupé à 500) sont recopiés dans ce message ; l'adapter doit donc les
/// nettoyer et les expurger (jeton et secret client retirés) avant de les rendre. Tous les autres
/// `detail` ne servent qu'aux journaux d'opérateur : les cas d'usage les y écrivent par
/// `failure::port_failure`, jamais vers le modèle.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CorpusError {
    /// Ressource introuvable (classification de 04 §11.2).
    #[error("not found: {what}")]
    NotFound {
        /// Ce qui manque, ex. l'identifiant.
        what: String,
    },
    /// Requête refusée : `Some(400)` pour un 400 PISTE sans marqueur « introuvable » ;
    /// `Some(403)` pour une page HTML de la passerelle (pas un refus d'accès à l'API) ;
    /// `None` pour la garde défensive de l'adapter, sans appel réseau. Jamais réessayée.
    #[error("request rejected by Legifrance")]
    InvalidRequest {
        /// Statut HTTP, s'il y en a un.
        status: Option<u16>,
        /// Vu par le modèle (coupé à 200 caractères par `messages`) et journalisé : message PISTE
        /// nettoyé et expurgé par l'adapter (jeton et secret client retirés), ou texte fixe.
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
        /// Détail pour les journaux (`failure::port_failure`), jamais vu par le modèle.
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
    /// Identifiants refusés (OAuth 400, 401 ou 403 ; second 401 de l'API) ou accès refusé (403
    /// de l'API dont le corps n'est pas une page HTML ; la page HTML de la passerelle est une
    /// `InvalidRequest`).
    #[error("credentials rejected")]
    Auth {
        /// Endpoint qui a refusé.
        origin: AuthOrigin,
        /// Statut HTTP réellement reçu, `None` s'il est inconnu.
        status: Option<u16>,
        /// Détail pour les journaux (`failure::port_failure`), jamais vu par le modèle.
        detail: String,
    },
    /// Configuration invalide ou absente ; `kind` dit laquelle.
    #[error("configuration error: {detail}")]
    Config {
        /// Nature de l'erreur, seule lue par le message du modèle (hors `Settings`).
        kind: ConfigKind,
        /// Détail pour les journaux (`failure::port_failure`) ; pour [`ConfigKind::Settings`],
        /// vu aussi par le modèle (coupé à 500 caractères), donc rédigé et expurgé par l'adapter.
        detail: String,
    },
    /// Réponse illisible (JSON invalide, forme inattendue, corps de l'endpoint du jeton
    /// au-delà de la borne).
    #[error("undecodable upstream response: {detail}")]
    Decode {
        /// Détail pour les journaux (`failure::port_failure`), jamais vu par le modèle.
        detail: String,
    },
    /// Corps d'une réponse de `/search` ou d'une consultation au-delà de la borne de corps
    /// (04 §4.3), jamais réessayé. Typé pour `get_act` : sur `fr_get_act`, un texte trop gros (un
    /// code entier, 04 §7.3) oriente vers `fr_get_text` ou `fr_search`, sans lire le `detail`.
    /// Le corps trop gros de l'endpoint du jeton reste un [`CorpusError::Decode`] : un jeton
    /// énorme n'est pas un texte trop gros pour `get_act`.
    #[error("response too large")]
    TooLarge {
        /// Borne et gabarit d'endpoint, pour les journaux (`failure::port_failure`).
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixtures::{poll_once, query};
    use pretty_assertions::assert_eq;
    use std::{future::ready, task::Poll};

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

    async fn every_method<C: LegalCorpus>(corpus: &C) -> Vec<CorpusError> {
        vec![
            corpus.search(&query()).await.unwrap_err(),
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
            (
                CorpusError::Auth {
                    origin: AuthOrigin::Token,
                    status: Some(401),
                    detail: "OAuth token request rejected: HTTP 401, environment sandbox".into(),
                },
                "credentials rejected",
            ),
            (
                CorpusError::Auth {
                    origin: AuthOrigin::Api,
                    status: Some(403),
                    detail: "HTTP 403 on /consult/juri".into(),
                },
                "credentials rejected",
            ),
            (
                CorpusError::Auth { origin: AuthOrigin::Api, status: None, detail: "x".into() },
                "credentials rejected",
            ),
            (
                CorpusError::Config {
                    kind: ConfigKind::CredentialsMissing,
                    detail: "credentials missing".into(),
                },
                "configuration error: credentials missing",
            ),
            (
                CorpusError::Config {
                    kind: ConfigKind::OAuthResponse,
                    detail: "PISTE OAuth returned no access_token".into(),
                },
                "configuration error: PISTE OAuth returned no access_token",
            ),
            (
                CorpusError::Config {
                    kind: ConfigKind::ApiEndpoint,
                    detail: "HTTP 404 on /search".into(),
                },
                "configuration error: HTTP 404 on /search",
            ),
            (
                CorpusError::Config {
                    kind: ConfigKind::OAuthEndpoint,
                    detail: "PISTE OAuth endpoint answered HTTP 404".into(),
                },
                "configuration error: PISTE OAuth endpoint answered HTTP 404",
            ),
            (
                CorpusError::Config { kind: ConfigKind::Settings, detail: "bad setting".into() },
                "configuration error: bad setting",
            ),
            (
                CorpusError::Decode { detail: "invalid UTF-8 at byte 3".into() },
                "undecodable upstream response: invalid UTF-8 at byte 3",
            ),
            // Le message ne recopie pas le `detail` (borne et endpoint) : il nomme l'échec.
            (
                CorpusError::TooLarge {
                    detail: "response too large (> 16 MiB) on /consult/lawDecree".into(),
                },
                "response too large",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn origin_and_kind_are_plain_copyable_values() {
        let origin = AuthOrigin::Api;
        let kind = ConfigKind::Settings;
        let (copied_origin, copied_kind) = (origin, kind);
        assert_eq!((copied_origin, copied_kind), (origin, kind));
        assert_eq!(format!("{origin:?} {kind:?}"), "Api Settings");
    }
}
