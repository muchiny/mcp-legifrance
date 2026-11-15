//! Port sortant vers la doctrine fiscale (00 §3, ADR-0028) : le BOFiP, lu sur le portail ouvert
//! de la DGFiP.
//!
//! Port distinct de [`LegalCorpus`](super::LegalCorpus) : le BOFiP n'est pas un fond de
//! Légifrance, n'a pas d'identifiants PISTE et se lit sans compte. Deux méthodes :
//! [`TaxDoctrine::search`] et [`TaxDoctrine::get`]. Ses erreurs ([`DoctrineError`]) sont à part
//! de `CorpusError`, pour que le message vu par le modèle nomme le BOFiP et jamais PISTE.
//!
//! Aucune variante de [`DoctrineError`] ne porte l'adresse de la requête, ses mots, un message
//! du portail ni un extrait de corps : en `GET`, l'adresse contient les mots cherchés
//! (ADR-0029, décision 6). Les `detail` sont des textes fixes de l'adapter, pour les journaux.

use std::{future::ready, time::Duration};

use lf_domain::doctrine::{BoiId, DoctrineDoc, DoctrinePage, DoctrineQuery};
use thiserror::Error;

/// Longueur maximale d'un code d'erreur du portail ([`PortalCode`]).
pub const PORTAL_CODE_MAX_BYTES: usize = 40;

/// Port sortant vers la doctrine fiscale.
///
/// Aucune méthode par défaut. Les futurs sont `+ Send`, comme ceux de
/// [`LegalCorpus`](super::LegalCorpus) ; le dispatch est statique, jamais `dyn` (ADR-0012). Une
/// implémentation écrit des `async fn`.
pub trait TaxDoctrine: Send + Sync + 'static {
    /// Recherche de publications ; jamais leur texte.
    fn search(
        &self,
        query: &DoctrineQuery,
    ) -> impl Future<Output = Result<DoctrinePage, DoctrineError>> + Send;

    /// Publications qui portent l'identifiant `id`, avec leur texte : une, parfois deux (23
    /// identifiants le 2026-11-15). Aucune publication est un [`DoctrineError::NotFound`] ;
    /// chaque publication rendue porte l'identifiant demandé.
    fn get(
        &self,
        id: &BoiId,
    ) -> impl Future<Output = Result<Vec<DoctrineDoc>, DoctrineError>> + Send;
}

/// Code d'erreur lu dans un refus du portail (`ODSQLSyntaxError`…) : de 1 à
/// [`PORTAL_CODE_MAX_BYTES`] lettres ASCII, rien d'autre.
///
/// Le type garde la seule chose qu'on retient d'un corps d'erreur : jamais son message, qui
/// recopie la requête (ADR-0029, décision 6). Sa forme le rend sûr à montrer au modèle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalCode(String);

impl PortalCode {
    /// Garde `raw` s'il a la forme attendue ; `None` sinon.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let valid = (1..=PORTAL_CODE_MAX_BYTES).contains(&raw.len())
            && raw.bytes().all(|byte| byte.is_ascii_alphabetic());
        valid.then(|| Self(raw.to_owned()))
    }

    /// Le code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Erreurs du port.
///
/// Le message vu par le modèle est rédigé par `messages::doctrine_error` à partir des seuls
/// champs typés (variante, statut, [`PortalCode`]) ; les `detail` ne vont qu'aux journaux
/// d'opérateur, par `failure::doctrine_failure`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DoctrineError {
    /// Source coupée (`LF_BOFIP=off`, valeur invalide, mode hors ligne) : aucune E/S n'est faite.
    #[error("BOFiP source is turned off")]
    Disabled,
    /// Aucune publication ne porte l'identifiant demandé.
    #[error("not found")]
    NotFound,
    /// Quota : refus 429 du portail, repos de la source après un refus, ou budget du jour du
    /// processus épuisé. Jamais réessayé.
    #[error("rate limited")]
    RateLimited {
        /// Attente annoncée par le portail (`Retry-After`) ou par le limiteur local.
        retry_after: Option<Duration>,
    },
    /// Requête refusée par le portail (400). Jamais réessayée.
    #[error("request rejected by the BOFiP portal")]
    InvalidRequest {
        /// Statut HTTP reçu.
        status: u16,
        /// Code d'erreur du portail, s'il a la forme attendue.
        code: Option<PortalCode>,
    },
    /// Erreur amont ; `status` vaut `None` pour une erreur réseau. `retryable` ne sert qu'au
    /// message : l'adapter ne réessaie jamais.
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
    /// Réponse illisible ou non conforme : JSON invalide, identifiant rendu différent de celui
    /// demandé, plus de résultats que demandé, permalien sans numéro.
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

/// Source de doctrine coupée : un [`TaxDoctrine`] qui rend toujours [`DoctrineError::Disabled`],
/// sans aucune E/S. C'est la source d'une façade assemblée sans BOFiP, et celle du binaire quand
/// `LF_BOFIP` la coupe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoDoctrine;

impl TaxDoctrine for NoDoctrine {
    fn search(
        &self,
        _query: &DoctrineQuery,
    ) -> impl Future<Output = Result<DoctrinePage, DoctrineError>> + Send {
        ready(Err(DoctrineError::Disabled))
    }

    fn get(
        &self,
        _id: &BoiId,
    ) -> impl Future<Output = Result<Vec<DoctrineDoc>, DoctrineError>> + Send {
        ready(Err(DoctrineError::Disabled))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::{
        doctrine::{DoctrineTerms, KindFilter},
        search::PageSize,
    };
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn a_source_turned_off_refuses_every_call() {
        let query = DoctrineQuery::new(
            DoctrineTerms::parse("donation").unwrap(),
            None,
            KindFilter::default(),
            NonZeroU32::MIN,
            PageSize::DEFAULT,
        )
        .unwrap();
        assert_eq!(NoDoctrine.search(&query).await, Err(DoctrineError::Disabled));
        let id = BoiId::parse("BOI-ENR-DMTG-10-50-20").unwrap();
        assert_eq!(NoDoctrine.get(&id).await, Err(DoctrineError::Disabled));
    }

    #[test]
    fn a_portal_code_is_letters_only() {
        for raw in ["ODSQLSyntaxError", "InvalidRESTParametersError", "a", &"Z".repeat(40)] {
            assert_eq!(
                PortalCode::parse(raw).map(|code| code.as_str().to_owned()),
                Some(raw.to_owned())
            );
        }
        for raw in [
            "",
            &"Z".repeat(41),
            "ODSQL SyntaxError",
            "Error1",
            "error_code",
            "Erreur.",
            "É",
            "search(contenu, \"donation\")",
            "a\nb",
        ] {
            assert_eq!(PortalCode::parse(raw), None);
        }
    }

    #[test]
    fn errors_display_without_any_request_detail() {
        let cases = [
            (DoctrineError::Disabled, "BOFiP source is turned off"),
            (DoctrineError::NotFound, "not found"),
            (DoctrineError::RateLimited { retry_after: None }, "rate limited"),
            (
                DoctrineError::InvalidRequest { status: 400, code: PortalCode::parse("X") },
                "request rejected by the BOFiP portal",
            ),
            (
                DoctrineError::Upstream { status: Some(503), retryable: true, detail: "d".into() },
                "upstream error",
            ),
            (DoctrineError::Timeout, "upstream timeout"),
            (
                DoctrineError::Decode { detail: "records: id".into() },
                "undecodable upstream response: records: id",
            ),
            (DoctrineError::TooLarge { detail: "4 MiB".into() }, "response too large"),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }
}
