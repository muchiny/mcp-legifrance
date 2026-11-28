//! Port sortant vers la jurisprudence judiciaire en open data (00 §3, ADR-0035) : Judilibre, la
//! base de la Cour de cassation, lue par l'API PISTE.
//!
//! Port distinct de [`LegalCorpus`](super::LegalCorpus) : identifiant, juridictions, mentions et
//! règles de réutilisation propres. Deux méthodes : [`JudicialOpenData::search`] et
//! [`JudicialOpenData::get`]. Ses erreurs ([`CaseLawError`]) sont à part de `CorpusError`, pour
//! que le message vu par le modèle nomme Judilibre et jamais Légifrance.
//!
//! Aucune variante de [`CaseLawError`] ne porte l'adresse de la requête, ses mots ni un extrait
//! de corps : en `GET`, l'adresse contient les mots cherchés (ADR-0035, décision 9). Les `detail`
//! sont des textes fixes de l'adapter, pour les journaux.

use std::{future::ready, time::Duration};

use lf_domain::judilibre::{CaseLawDoc, CaseLawPage, CaseLawQuery, JudilibreId};
use thiserror::Error;

use super::{AuthOrigin, ConfigKind};

/// Port sortant vers Judilibre.
///
/// Aucune méthode par défaut. Les futurs sont `+ Send`, comme ceux de
/// [`LegalCorpus`](super::LegalCorpus) ; le dispatch est statique, jamais `dyn` (ADR-0012). Une
/// implémentation écrit des `async fn`.
pub trait JudicialOpenData: Send + Sync + 'static {
    /// Recherche de décisions ; jamais leur texte. Les juridictions de la requête sont toujours
    /// envoyées : vide, la liste ne viserait que la Cour de cassation.
    fn search(
        &self,
        query: &CaseLawQuery,
    ) -> impl Future<Output = Result<CaseLawPage, CaseLawError>> + Send;

    /// Une décision, avec son texte entier et ses zones. Une décision inconnue est un
    /// [`CaseLawError::NotFound`] ; la décision rendue porte l'identifiant demandé.
    fn get(
        &self,
        id: &JudilibreId,
    ) -> impl Future<Output = Result<CaseLawDoc, CaseLawError>> + Send;
}

/// Erreurs du port.
///
/// Le message vu par le modèle est rédigé par `messages::case_law_error` à partir des seuls
/// champs typés (variante, statut, nature) ; les `detail` ne vont qu'aux journaux d'opérateur,
/// par `failure::case_law_failure`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CaseLawError {
    /// Source coupée (`LF_JUDILIBRE=off`, valeur invalide, mode hors ligne) : aucune E/S n'est
    /// faite.
    #[error("Judilibre source is turned off")]
    Disabled,
    /// Refus 403 de l'API : l'application PISTE n'est pas abonnée à Judilibre. L'adapter le
    /// retient 10 minutes, sans requête pendant ce temps (ADR-0035, décision 9).
    #[error("PISTE application not subscribed to Judilibre")]
    NotSubscribed,
    /// Aucune décision ne porte l'identifiant demandé (404).
    #[error("not found")]
    NotFound,
    /// Quota PISTE : refus 429, ou limiteur local. Quota partagé avec Légifrance.
    #[error("rate limited")]
    RateLimited {
        /// Attente annoncée par PISTE (`Retry-After`) ou par le limiteur local.
        retry_after: Option<Duration>,
    },
    /// Requête refusée par Judilibre (400, 416, 423) : jamais réessayée. `status` vaut `None`
    /// quand la requête n'a pas pu partir (garde locale de l'adapter, bug probable).
    #[error("request rejected by Judilibre")]
    Rejected {
        /// Statut HTTP reçu.
        status: Option<u16>,
    },
    /// Erreur amont après les réessais ; `status` vaut `None` pour une erreur réseau.
    #[error("upstream error")]
    Upstream {
        /// Statut HTTP, s'il y en a un.
        status: Option<u16>,
        /// Vrai si une nouvelle tentative du modèle a des chances d'aboutir.
        retryable: bool,
        /// Détail pour les journaux, jamais vu par le modèle : texte fixe, sans adresse.
        detail: String,
    },
    /// Délai de la requête dépassé, ou échéance de l'appel atteinte en attendant son tour.
    #[error("upstream timeout")]
    Timeout,
    /// Identifiants PISTE refusés (jeton, ou 401 répété de l'API). Jamais un 403 de l'API, qui
    /// est [`CaseLawError::NotSubscribed`].
    #[error("credentials rejected")]
    Auth {
        /// Endpoint qui a refusé.
        origin: AuthOrigin,
        /// Statut HTTP, s'il y en a un.
        status: Option<u16>,
        /// Détail pour les journaux, jamais vu par le modèle.
        detail: String,
    },
    /// Configuration inutilisable : identifiants absents, adresse refusée, endpoint introuvable.
    #[error("configuration error")]
    Config {
        /// Nature du problème.
        kind: ConfigKind,
        /// Détail pour les journaux ; seul celui de `Settings` est montré, coupé (05 §7.2).
        detail: String,
    },
    /// Réponse illisible ou non conforme : JSON invalide, identifiant rendu différent de celui
    /// demandé, champ obligatoire absent.
    #[error("undecodable upstream response: {detail}")]
    Decode {
        /// Détail pour les journaux, jamais vu par le modèle : texte fixe, sans valeur du corps.
        detail: String,
    },
    /// Corps de réponse au-delà de la borne, mesurée après décompression.
    #[error("response too large")]
    TooLarge {
        /// Borne dépassée, pour les journaux.
        detail: String,
    },
}

/// Source Judilibre coupée : un [`JudicialOpenData`] qui rend toujours
/// [`CaseLawError::Disabled`], sans aucune E/S. C'est la source d'une façade assemblée sans
/// Judilibre, et celle du binaire quand `LF_JUDILIBRE` la coupe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoCaseLaw;

impl JudicialOpenData for NoCaseLaw {
    fn search(
        &self,
        _query: &CaseLawQuery,
    ) -> impl Future<Output = Result<CaseLawPage, CaseLawError>> + Send {
        ready(Err(CaseLawError::Disabled))
    }

    fn get(
        &self,
        _id: &JudilibreId,
    ) -> impl Future<Output = Result<CaseLawDoc, CaseLawError>> + Send {
        ready(Err(CaseLawError::Disabled))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::{
        judilibre::{CaseLawFilters, CaseLawSort, CourtSet},
        search::{MatchMode, PageSize, QueryText},
    };
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn a_source_turned_off_refuses_every_call() {
        let filters = CaseLawFilters {
            mode: MatchMode::All,
            courts: CourtSet::ALL,
            part: None,
            date_from: None,
            date_to: None,
            sort: CaseLawSort::Relevance,
        };
        let query = CaseLawQuery::new(
            QueryText::parse("bail commercial").unwrap(),
            filters,
            NonZeroU32::MIN,
            PageSize::DEFAULT,
        )
        .unwrap();
        assert_eq!(NoCaseLaw.search(&query).await, Err(CaseLawError::Disabled));
        let id = JudilibreId::parse("5fca624b7eeb384a40cafb85").unwrap();
        assert_eq!(NoCaseLaw.get(&id).await, Err(CaseLawError::Disabled));
    }

    #[test]
    fn errors_display_without_any_request_detail() {
        let detail = || "HTTP 503 on /cassation/judilibre/v1.0/search".to_owned();
        let cases = [
            (CaseLawError::Disabled, "Judilibre source is turned off"),
            (CaseLawError::NotSubscribed, "PISTE application not subscribed to Judilibre"),
            (CaseLawError::NotFound, "not found"),
            (CaseLawError::RateLimited { retry_after: None }, "rate limited"),
            (CaseLawError::Rejected { status: Some(416) }, "request rejected by Judilibre"),
            (
                CaseLawError::Upstream { status: Some(503), retryable: true, detail: detail() },
                "upstream error",
            ),
            (CaseLawError::Timeout, "upstream timeout"),
            (
                CaseLawError::Auth { origin: AuthOrigin::Token, status: None, detail: detail() },
                "credentials rejected",
            ),
            (
                CaseLawError::Config { kind: ConfigKind::Settings, detail: detail() },
                "configuration error",
            ),
            (CaseLawError::TooLarge { detail: detail() }, "response too large"),
            (
                CaseLawError::Decode { detail: "missing field `text`".to_owned() },
                "undecodable upstream response: missing field `text`",
            ),
        ];
        for (error, shown) in cases {
            assert_eq!(error.to_string(), shown);
        }
    }
}
