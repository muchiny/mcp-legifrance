//! Adapter PISTE du serveur MCP Légifrance (04, 02 §3.3).
//!
//! Rôle : `PisteCorpus`, seule implémentation du port `LegalCorpus` jusqu'en M9. Il parle
//! HTTP à l'API Légifrance via PISTE (OAuth 2.0 `client_credentials`), avec limiteur de
//! débit et réessais, et traduit les réponses en types du domaine.
//!
//! Livré au plan 2c : le squelette, les traits de temps, la configuration, l'authentification,
//! le client HTTP, la résilience et `PisteCorpus` (assemblage, mode dégradé, appel de l'API de
//! bout en bout). Livré au plan 2d (première moitié) : la configuration à champs privés et les
//! réglages validés, le décodage des corps JSON sans valeur du corps, le corps d'erreur de PISTE
//! et la classification « introuvable ». Renvoyé aux plans suivants : les requêtes de consultation,
//! les DTO métier, le mapping et `impl LegalCorpus for PisteCorpus`.
//!
//! Les modules sont privés : sont publics `PisteCorpus`, les types de configuration et
//! `StatsSnapshot`.
//!
//! Modules :
//! - `auth` : jeton d'accès OAuth 2.0 `client_credentials` (`TokenManager`) : un seul jeton en
//!   mémoire, renouvelé avant son expiration par une seule requête à la fois, avec réessais.
//! - `classify` : classification d'une tentative d'envoi (`Outcome`, `Budget`, `EndpointKind`,
//!   `ErrorContext`) d'après le statut HTTP et `Retry-After` ; le corps n'est lu que pour un 400,
//!   un 404 de consultation et un 2xx vide (« introuvable », 04 §11).
//! - `config` : configuration de l'adapter (`PisteConfig`, champs privés, réglages avancés
//!   validés `PisteTuning`) et validation des URL surchargées.
//! - `corpus` : `PisteCorpus`, l'assemblage de l'adapter : mode dégradé sans E/S, puis pour chaque
//!   tentative limiteur, jeton, envoi borné, classification et réessai, enfin décodage du corps.
//! - `deadline` : échéance d'un appel (`CallDeadline`), durée restante lue dans `Deadline`
//!   puis posée sur l'horloge monotone de l'adapter.
//! - `dto` : structures de désérialisation des réponses de PISTE (réponse et erreur de l'endpoint
//!   token, `PisteErrorDto` pour le corps d'erreur de l'API, réponses de `/search` et de
//!   `/consult/lawDecree|kaliText|getArticle|juri|cnil|acco`), avec des lecteurs serde tolérants
//!   (`null_default`, `opt_raw_date`…) et `decode`, qui lit un corps JSON avec le chemin de
//!   l'erreur et sans aucune valeur du corps.
//! - `encoding` : décodage UTF-8 strict du corps, détection et réparation du mojibake latin-1.
//! - `error_body` : message d'un corps d'erreur PISTE (UTF-8, mojibake, JSON facultatif), détail
//!   nettoyé, privé des secrets puis borné, marqueur « introuvable », détection d'une page HTML.
//! - `http` : client HTTP unique (`reqwest`, TLS rustls), tentative d'envoi (`send_once`) et
//!   lecture bornée du corps ; aucun réessai ni jugement de statut.
//! - `limit` : barrière de charge sortante (`RateGate`), sémaphore de concurrence puis limiteur
//!   de débit `governor` sur l'horloge de l'adapter ; toutes les attentes bornées par le délai.
//! - `request` : corps des requêtes envoyées à PISTE ; `request::search` construit celui de
//!   `/search` (`search_body`) et calcule le midi Europe/Paris du filtre `DATE_VERSION` (04 §6).
//! - `retry` : politique de réessai (`run`) : trois requêtes au plus, délai exponentiel avec gigue,
//!   `Retry-After`, bornée par le délai global ; boucle maison, sans `backon`.
//! - `stats` : compteurs en mémoire (`Stats`) et leur instantané public (`StatsSnapshot`).
//! - `testing` (tests seulement) : aides communes, `ClosedPort` (adresse locale qui refuse la
//!   connexion de façon déterministe).
//! - `time` : horloge monotone (`MonoClock`), attente (`Sleeper`) et course `race`, pour des
//!   tests déterministes sans geler l'horloge `tokio`.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "PisteCorpus::post_json et ce que lui seul emploie (try_send, RateGate::acquire, \
                  TokenManager::invalidate, Payload::Json, Stats::record_decode, les champs de \
                  TokenLease et d'état de PisteCorpus), dto::decode::decode, PisteErrorDto, \
                  request::search::search_body, OAuthErrorDto, les DTO de /search et de \
                  /consult (lawDecree, kaliText, getArticle, juri, cnil, acco) et ce que seuls \
                  ils emploient (lecteurs serde de dto::de, champs non lus) : consommés par \
                  impl LegalCorpus au plan 2e"
    )
)]

mod auth;
mod classify;
mod config;
mod corpus;
mod deadline;
mod dto;
mod encoding;
mod error_body;
mod http;
mod limit;
mod request;
mod retry;
mod stats;
#[cfg(test)]
mod testing;
mod time;

pub use config::{
    ConfigProblem, ConfigWarning, PisteConfig, PisteCredentials, PisteEndpoints, PisteTuning,
    RateQuota, TuningError,
};
pub use corpus::PisteCorpus;
pub use stats::StatsSnapshot;
