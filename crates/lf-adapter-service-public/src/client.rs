//! Transport : le téléchargement de l'archive du jour d'un public (ADR-0033 décisions 3, 4 et 11 ;
//! T-MS-21).
//!
//! - **Requête fermée** : les deux adresses de [`archive_url`], fixées dans le domaine et choisies
//!   par le public typé ; aucune variable ne les change. `https` seul, aucune redirection suivie,
//!   aucun `Referer`, aucune compression demandée (l'archive est déjà compressée). La requête ne
//!   porte ni mot cherché ni numéro de fiche : la fonction n'en reçoit pas.
//! - **Corps borné** à [`ARCHIVE_MAX_BYTES`] : refus immédiat si `Content-Length` annonce plus,
//!   sinon les morceaux sont comptés. Délai propre de [`DOWNLOAD_TIMEOUT`], connexion et corps
//!   compris.
//! - **Revalidation** : avec l'`ETag` de l'archive en mémoire, la requête porte `If-None-Match` ;
//!   un `304` la garde.
//! - **Statuts** : 200 lu, 304 gardé (s'il répond à une requête conditionnelle), 429 refusé puis
//!   repos de la source, 5xx refus qu'un appel suivant peut lever, tout autre statut refusé
//!   (redirection comprise). Aucun réessai.
//! - **Repos après un 429** : la durée annoncée par `Retry-After` en secondes, [`REST_MAX`] au plus,
//!   [`REST_DEFAULT`] si l'en-tête manque, est illisible ou annonce zéro ; pendant le repos, aucune
//!   requête ne part, pour aucun des deux publics (même serveur). Mêmes durées que le BOFiP
//!   (ADR-0029, décision 7).
//! - **Erreurs sans adresse** : toute erreur de `reqwest` passe par `without_url()`, puis est
//!   réduite à un libellé fixe.

use std::{
    fmt,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use lf_application::ports::GuidanceError;
use lf_domain::guidance::{Audience, archive_url};
use reqwest::{
    Client, Response,
    header::{ETAG, HeaderValue, IF_NONE_MATCH, LAST_MODIFIED, RETRY_AFTER},
    redirect,
};

/// Taille maximale de l'archive téléchargée : 64 Mio. Mesure du 2026-11-24 : 23,2 Mio pour
/// Particuliers, 10,5 Mio pour Entreprendre.
pub(crate) const ARCHIVE_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Délai d'un téléchargement, de la connexion à la fin du corps.
pub(crate) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Délai d'établissement de la connexion.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Repos le plus long après un refus 429, quoi qu'annonce le serveur.
pub(crate) const REST_MAX: Duration = Duration::from_secs(3_600);

/// Repos après un refus 429 sans `Retry-After` lisible, ou qui annonce zéro seconde.
pub(crate) const REST_DEFAULT: Duration = Duration::from_secs(900);

/// Longueur maximale d'un `ETag` gardé : au-delà, il n'est pas renvoyé (le serveur en donne de
/// 17 et 16 caractères).
const ETAG_MAX_BYTES: usize = 256;

/// Archive reçue, avec ce que ses en-têtes disent d'elle.
///
/// `Debug` rédigé à la main : le corps n'est jamais affiché, seulement sa longueur.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Download {
    /// Octets de l'archive, [`ARCHIVE_MAX_BYTES`] au plus.
    pub(crate) body: Vec<u8>,
    /// `ETag`, s'il est présent, en ASCII visible et de [`ETAG_MAX_BYTES`] au plus.
    pub(crate) etag: Option<String>,
    /// `Last-Modified`, s'il se lit comme une date HTTP.
    pub(crate) last_modified: Option<DateTime<Utc>>,
}

impl fmt::Debug for Download {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Download")
            .field("body_len", &self.body.len())
            .field("etag", &self.etag)
            .field("last_modified", &self.last_modified)
            .finish()
    }
}

/// Réponse à un téléchargement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fetched {
    /// Nouvelle archive.
    Fresh(Download),
    /// `304` : l'archive en mémoire reste la bonne.
    NotModified,
}

/// Client des deux archives, avec le repos de la source.
pub(crate) struct ArchiveClient {
    http: Client,
    particuliers: String,
    entreprendre: String,
    /// Taille maximale du corps : [`ARCHIVE_MAX_BYTES`] en production.
    limit: u64,
    rest: Rest,
}

impl ArchiveClient {
    /// Client de production : les deux adresses de [`archive_url`], `https` seul, délai de
    /// [`DOWNLOAD_TIMEOUT`].
    ///
    /// # Errors
    ///
    /// [`GuidanceError::Upstream`], sans adresse, si `reqwest` ne peut pas construire le client
    /// (`User-Agent` invalide, TLS inutilisable).
    pub(crate) fn production(user_agent: &str) -> Result<Self, GuidanceError> {
        Ok(Self::with(
            build_client(user_agent, DOWNLOAD_TIMEOUT, false)?,
            archive_url(Audience::Particuliers).to_owned(),
            archive_url(Audience::Entreprendre).to_owned(),
            ARCHIVE_MAX_BYTES,
        ))
    }

    fn with(http: Client, particuliers: String, entreprendre: String, limit: u64) -> Self {
        Self { http, particuliers, entreprendre, limit, rest: Rest::default() }
    }

    /// Client d'un serveur de test : les adresses de production, dont l'origine est remplacée par
    /// `base` (`http://127.0.0.1:…`), en clair.
    #[cfg(test)]
    pub(crate) fn local(base: &str, timeout: Duration, limit: u64) -> Result<Self, GuidanceError> {
        let origin = "https://lecomarquage.service-public.gouv.fr";
        build_client("mcp-legifrance-tests/0", timeout, true).map(|http| {
            Self::with(
                http,
                archive_url(Audience::Particuliers).replacen(origin, base, 1),
                archive_url(Audience::Entreprendre).replacen(origin, base, 1),
                limit,
            )
        })
    }

    fn url(&self, audience: Audience) -> &str {
        match audience {
            Audience::Particuliers => &self.particuliers,
            Audience::Entreprendre => &self.entreprendre,
        }
    }

    /// Un seul `GET` de l'archive de `audience`, conditionnel si `etag` est donné. Pendant un
    /// repos, rien ne part.
    ///
    /// # Errors
    ///
    /// [`GuidanceError::RateLimited`] (429, ou repos en cours ; `retry_after` dit le repos qui
    /// reste) ; [`GuidanceError::Timeout`] ; [`GuidanceError::TooLarge`] pour une archive au-delà
    /// de la borne ; [`GuidanceError::Upstream`] pour une panne réseau (sans statut), un statut
    /// refusé ou un `304` sans requête conditionnelle. Aucune ne porte l'adresse.
    pub(crate) async fn fetch(
        &self,
        audience: Audience,
        etag: Option<&str>,
    ) -> Result<Fetched, GuidanceError> {
        if let Some(left) = self.rest.left(Instant::now()) {
            return Err(GuidanceError::RateLimited { retry_after: Some(left) });
        }
        let mut request = self.http.get(self.url(audience));
        if let Some(etag) = etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(classify)?;
        match response.status().as_u16() {
            200 => read_archive(response, self.limit).await.map(Fetched::Fresh),
            304 if etag.is_some() => Ok(Fetched::NotModified),
            429 => {
                let announced = retry_after(response.headers().get(RETRY_AFTER));
                let rest = self.rest.start(Instant::now(), announced);
                Err(GuidanceError::RateLimited { retry_after: Some(rest) })
            }
            status @ 500..=599 => Err(upstream(Some(status), true, "server error")),
            status => Err(upstream(Some(status), false, "unexpected status")),
        }
    }
}

/// Fin du repos de la source, s'il y en a un.
#[derive(Debug, Default)]
struct Rest(Mutex<Option<Instant>>);

impl Rest {
    /// Repos qui reste à `now`, s'il en reste.
    fn left(&self, now: Instant) -> Option<Duration> {
        let until = (*self.0.lock().unwrap_or_else(PoisonError::into_inner))?;
        let left = until.saturating_duration_since(now);
        (!left.is_zero()).then_some(left)
    }

    /// Commence un repos à `now` pour la durée annoncée, bornée ; rend sa longueur.
    fn start(&self, now: Instant, announced: Option<Duration>) -> Duration {
        let length = rest_length(announced);
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = now.checked_add(length);
        length
    }
}

/// Longueur d'un repos : l'annonce bornée à [`REST_MAX`], [`REST_DEFAULT`] sans annonce utile.
fn rest_length(announced: Option<Duration>) -> Duration {
    announced.filter(|announced| !announced.is_zero()).map_or(REST_DEFAULT, |d| d.min(REST_MAX))
}

/// Durée annoncée par un en-tête `Retry-After` en secondes entières. Une date HTTP, ou tout autre
/// texte, est traitée comme un en-tête absent.
fn retry_after(value: Option<&HeaderValue>) -> Option<Duration> {
    value?.to_str().ok()?.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Construit le client HTTP. `plain_http` n'est vrai que pour un serveur de test : le client de
/// production refuse toute adresse qui n'est pas en `https`.
fn build_client(
    user_agent: &str,
    timeout: Duration,
    plain_http: bool,
) -> Result<Client, GuidanceError> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(timeout)
        .user_agent(user_agent)
        .no_gzip()
        .redirect(redirect::Policy::none())
        .referer(false)
        .https_only(!plain_http)
        .build()
        .map_err(|_| upstream(None, false, "could not build the HTTP client"))
}

/// Lit l'archive et ses en-têtes, en refusant de dépasser `limit` octets.
async fn read_archive(mut response: Response, limit: u64) -> Result<Download, GuidanceError> {
    let headers = response.headers();
    let etag = headers
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .filter(|etag| !etag.is_empty() && etag.len() <= ETAG_MAX_BYTES)
        .map(str::to_owned);
    let last_modified = headers.get(LAST_MODIFIED).and_then(http_date);
    if response.content_length().is_some_and(|length| length > limit) {
        return Err(too_large(limit));
    }
    let mut body = Vec::new();
    let mut received: u64 = 0;
    while let Some(chunk) = response.chunk().await.map_err(classify)? {
        received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if received > limit {
            return Err(too_large(limit));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Download { body, etag, last_modified })
}

/// Date HTTP de la forme de la RFC 2822 (`Tue, 24 Nov 2026 14:26:09 GMT`) ; les formes
/// obsolètes de la RFC 9110 ne sont pas lues.
fn http_date(value: &HeaderValue) -> Option<DateTime<Utc>> {
    let raw = value.to_str().ok()?;
    DateTime::parse_from_rfc2822(raw.trim()).ok().map(|date| date.with_timezone(&Utc))
}

/// Erreur « archive trop grosse », au libellé de la borne.
fn too_large(limit: u64) -> GuidanceError {
    GuidanceError::TooLarge { detail: format!("archive over {limit} bytes") }
}

/// Erreur amont au libellé fixe.
fn upstream(status: Option<u16>, retryable: bool, detail: &str) -> GuidanceError {
    GuidanceError::Upstream { status, retryable, detail: detail.to_owned() }
}

/// Range une erreur de `reqwest` : délai, ou panne réseau avec l'un de trois libellés fixes.
/// L'adresse est retirée d'abord ; le texte de l'erreur n'est jamais recopié.
fn classify(error: reqwest::Error) -> GuidanceError {
    let error = error.without_url();
    classify_parts(error.is_timeout(), error.is_connect(), error.is_body() || error.is_decode())
}

/// Cœur de [`classify`], sur les constats de `reqwest` : `timeout`, panne de `connect` (phase de
/// connexion, TLS compris) et panne de `body` (corps interrompu). Le délai d'abord ; le reste
/// n'est qu'un libellé pour les journaux.
fn classify_parts(timeout: bool, connect: bool, body: bool) -> GuidanceError {
    if timeout {
        return GuidanceError::Timeout;
    }
    let kind = if connect {
        "network error (connect)"
    } else if body {
        "network error (body)"
    } else {
        "network error (request)"
    };
    upstream(None, true, kind)
}

#[cfg(test)]
mod tests;
