//! Port sortant vers les fiches pratiques de Service-Public (00 §3, ADR-0033) : l'information
//! administrative de la DILA, lue dans l'archive du jour de chaque public.
//!
//! Port distinct de [`LegalCorpus`](super::LegalCorpus) et de [`TaxDoctrine`](super::TaxDoctrine) :
//! une fiche n'a ni fonds, ni version à une date, et ce n'est pas de la doctrine. Deux méthodes :
//! [`PublicGuidance::search`] et [`PublicGuidance::get`]. Ses erreurs ([`GuidanceError`]) sont à
//! part, pour que le message vu par le modèle nomme Service-Public et jamais PISTE.
//!
//! La recherche et la lecture se font sur l'archive gardée en mémoire : seul son téléchargement
//! part sur le réseau, sans mot ni numéro (ADR-0033, décision 3). Aucune variante de
//! [`GuidanceError`] ne porte un mot de la recherche ni un extrait de l'archive ; les `detail`
//! sont des textes fixes de l'adapter, pour les journaux.

use std::{future::ready, time::Duration};

use chrono::{DateTime, Utc};
use lf_domain::guidance::{
    Audience, GuidanceDoc, GuidancePage, GuidanceQuery, ServicePublicUrl, SheetId, archive_url,
};
use thiserror::Error;

/// Archive du jour d'un public, dans les termes de la DILA (ADR-0033, décision 9) : son adresse
/// ([`ArchiveFile::url`]) et son nom ([`ARCHIVE_NAME`](lf_domain::guidance::ARCHIVE_NAME)) se
/// déduisent du public ; seule sa date vient du serveur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveFile {
    /// Public de l'archive.
    pub audience: Audience,
    /// En-tête `Last-Modified` reçu avec l'archive gardée en mémoire ; `None` s'il manquait ou ne
    /// se lisait pas.
    pub last_modified: Option<DateTime<Utc>>,
}

impl ArchiveFile {
    /// Adresse complète de l'archive, fixée dans le code.
    #[must_use]
    pub const fn url(&self) -> &'static str {
        archive_url(self.audience)
    }
}

/// Ce que rend une recherche : la page de résultats, et l'archive de chaque public cherché.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidanceFound {
    /// Résultats de la page demandée.
    pub page: GuidancePage,
    /// Les archives lues, une par public cherché. Le cas d'usage n'en prend que les dates des
    /// publics de la requête.
    pub files: Vec<ArchiveFile>,
}

/// Ce que rend une lecture : la page lue, et l'archive dont elle vient.
#[derive(Debug, Clone, PartialEq)]
pub struct GuidanceRead {
    /// La fiche ou la page de navigation.
    pub doc: GuidanceDoc,
    /// L'archive du public lu.
    pub file: ArchiveFile,
}

/// Port sortant vers les fiches pratiques de Service-Public.
///
/// Aucune méthode par défaut. Les futurs sont `+ Send`, comme ceux des autres ports ; le
/// dispatch est statique, jamais `dyn` (ADR-0012). Une implémentation écrit des `async fn`.
pub trait PublicGuidance: Send + Sync + 'static {
    /// Recherche dans les titres et résumés des pages des publics demandés ; jamais leur texte.
    fn search(
        &self,
        query: &GuidanceQuery,
    ) -> impl Future<Output = Result<GuidanceFound, GuidanceError>> + Send;

    /// La page `id` de l'archive de `audience` : une fiche avec son texte, ou un dossier avec ses
    /// entrées. Un numéro absent de cette archive est un [`GuidanceError::NotFound`] ; la page
    /// rendue porte le numéro et le public demandés.
    fn get(
        &self,
        audience: Audience,
        id: &SheetId,
    ) -> impl Future<Output = Result<GuidanceRead, GuidanceError>> + Send;
}

/// Erreurs du port.
///
/// Le message vu par le modèle est rédigé par `messages::guidance_error` à partir des seuls
/// champs typés ; les `detail` ne vont qu'aux journaux d'opérateur, par
/// `failure::guidance_failure`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GuidanceError {
    /// Source coupée (`LF_SERVICE_PUBLIC=off`, valeur invalide, mode hors ligne) : aucune E/S
    /// n'est faite.
    #[error("Service-Public source is turned off")]
    Disabled,
    /// Le numéro n'est pas dans l'archive du public demandé.
    #[error("not found")]
    NotFound,
    /// Refus 429 du serveur de la DILA sans archive en mémoire, ou repos de la source après un
    /// refus. Jamais réessayé.
    #[error("rate limited")]
    RateLimited {
        /// Attente annoncée par le serveur (`Retry-After`) ou par le repos de la source.
        retry_after: Option<Duration>,
    },
    /// L'archive est encore en cours de téléchargement quand l'appel atteint son échéance
    /// moins une seconde ; le téléchargement continue (ADR-0033, décision 3).
    #[error("archive still downloading")]
    Downloading,
    /// Page trop grande pour être rendue ici : entrée de l'archive au-delà de 2 Mio une fois
    /// décompressée (adapter), ou métadonnées qui ne tiennent pas sous le plafond de réponse (cas
    /// d'usage). La page se lit sur le site.
    #[error("entry too large")]
    EntryTooLarge {
        /// Adresse publique de la page, que le message donne.
        source_url: ServicePublicUrl,
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
    /// Délai du téléchargement dépassé (300 s).
    #[error("upstream timeout")]
    Timeout,
    /// Archive ou page refusée par le lecteur : format de l'archive, somme CRC-32, `DOCTYPE`,
    /// bornes du XML.
    #[error("undecodable upstream response: {detail}")]
    Decode {
        /// Détail pour les journaux, jamais vu par le modèle : texte fixe, sans contenu lu.
        detail: String,
    },
    /// Archive au-delà de 64 Mio.
    #[error("response too large")]
    TooLarge {
        /// Borne dépassée, pour les journaux.
        detail: String,
    },
}

/// Source de Service-Public coupée : un [`PublicGuidance`] qui rend toujours
/// [`GuidanceError::Disabled`], sans aucune E/S. C'est la source d'une façade assemblée sans
/// Service-Public, et celle du binaire quand `LF_SERVICE_PUBLIC` la coupe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoGuidance;

impl PublicGuidance for NoGuidance {
    fn search(
        &self,
        _query: &GuidanceQuery,
    ) -> impl Future<Output = Result<GuidanceFound, GuidanceError>> + Send {
        ready(Err(GuidanceError::Disabled))
    }

    fn get(
        &self,
        _audience: Audience,
        _id: &SheetId,
    ) -> impl Future<Output = Result<GuidanceRead, GuidanceError>> + Send {
        ready(Err(GuidanceError::Disabled))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::{
        guidance::{AudienceFilter, GuidanceTerms},
        search::PageSize,
    };
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn no_guidance_answers_config_error_without_io() {
        let query = GuidanceQuery {
            terms: GuidanceTerms::parse("allocation logement").unwrap(),
            audience: AudienceFilter::All,
            page: NonZeroU32::MIN,
            page_size: PageSize::DEFAULT,
        };
        assert_eq!(NoGuidance.search(&query).await, Err(GuidanceError::Disabled));
        let id = SheetId::parse("F1437").unwrap();
        for audience in Audience::ALL {
            assert_eq!(NoGuidance.get(audience, &id).await, Err(GuidanceError::Disabled));
        }
    }

    #[test]
    fn an_archive_file_names_the_archive_of_its_audience() {
        let part = ArchiveFile { audience: Audience::Particuliers, last_modified: None };
        let pro = ArchiveFile { audience: Audience::Entreprendre, ..part };
        assert_eq!(
            [part.url(), pro.url()],
            [
                "https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/zip/vosdroits-latest.zip",
                "https://lecomarquage.service-public.gouv.fr/vdd/3.5/pro/zip/vosdroits-latest.zip",
            ]
        );
    }

    #[test]
    fn errors_display_without_any_request_detail() {
        let id = SheetId::parse("F37096").unwrap();
        let cases = [
            (GuidanceError::Disabled, "Service-Public source is turned off"),
            (GuidanceError::NotFound, "not found"),
            (GuidanceError::RateLimited { retry_after: None }, "rate limited"),
            (GuidanceError::Downloading, "archive still downloading"),
            (
                GuidanceError::EntryTooLarge {
                    source_url: ServicePublicUrl::read(None, Audience::Particuliers, &id),
                },
                "entry too large",
            ),
            (
                GuidanceError::Upstream { status: Some(503), retryable: true, detail: "d".into() },
                "upstream error",
            ),
            (GuidanceError::Timeout, "upstream timeout"),
            (
                GuidanceError::Decode { detail: "zip: data descriptor".into() },
                "undecodable upstream response: zip: data descriptor",
            ),
            (GuidanceError::TooLarge { detail: "64 MiB".into() }, "response too large"),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }
}
