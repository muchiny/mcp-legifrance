//! Adapter PISTE du serveur MCP Légifrance (04, 02 §3.3).
//!
//! Rôle : `PisteCorpus`, seule implémentation du port `LegalCorpus` jusqu'en M9. Il parle
//! HTTP à l'API Légifrance via PISTE (OAuth 2.0 `client_credentials`), avec limiteur de
//! débit et réessais, et traduit les réponses en types du domaine.
//!
//! Livré au plan 2c : le squelette, les traits de temps, la configuration, l'authentification,
//! le client HTTP, la résilience et `PisteCorpus` (assemblage, mode dégradé, appel de l'API de
//! bout en bout). Livré au plan 2d : la configuration à champs privés et les réglages validés, le
//! décodage des corps JSON sans valeur du corps, le corps d'erreur de PISTE et la classification
//! « introuvable », les DTO de réponse de `/search` et de `/consult`, le corps de la requête
//! `/search` avec le midi Europe/Paris et le mémo de `typePagination`. Livré au plan 2f (budget de
//! décodage) : le budget de 200 000 éléments par document, les listes de DTO bornées, `results`
//! plafonné, `typePagination` tolérant et `num` lu en chaîne ou en nombre. Renvoyé : le mapping
//! (suite du plan 2f), les requêtes de consultation et `impl LegalCorpus for PisteCorpus` (plan 2g).
//!
//! Les modules sont privés : sont publics `PisteCorpus`, les types de configuration et
//! `StatsSnapshot`.
//!
//! Modules :
//! - `auth` : jeton d'accès OAuth 2.0 `client_credentials` (`TokenManager`) : un seul jeton en
//!   mémoire, renouvelé avant son expiration par une seule requête à la fois, avec réessais.
//! - `classify` : classification d'une tentative d'envoi (`Outcome`, `Budget`, `EndpointKind`,
//!   `ErrorContext`) d'après le statut HTTP et `Retry-After` ; le corps n'est lu que pour un 400
//!   de consultation, un 404 de consultation et un 2xx vide (« introuvable », 04 §11).
//! - `config` : configuration de l'adapter (`PisteConfig`, champs privés, réglages avancés
//!   validés `PisteTuning`) et validation des URL surchargées.
//! - `corpus` : `PisteCorpus`, l'assemblage de l'adapter : mode dégradé sans E/S, puis pour chaque
//!   tentative limiteur, jeton, envoi borné, classification et réessai, enfin décodage du corps.
//! - `deadline` : échéance d'un appel (`CallDeadline`), durée restante lue dans `Deadline`
//!   puis posée sur l'horloge monotone de l'adapter.
//! - `dto` : structures de désérialisation des réponses de PISTE (réponse et erreur de l'endpoint
//!   token, `PisteErrorDto` pour le corps d'erreur de l'API, réponses de `/search` et de
//!   `/consult/lawDecree|kaliText|getArticle|juri|cnil|acco`), avec des lecteurs serde tolérants
//!   (`bounded_list`, `capped_list`, `opt_raw_date`…), `budget` (200 000 éléments de liste par
//!   document, armé par `decode`) et `decode`, qui lit un corps JSON sous ce budget, avec le
//!   chemin de l'erreur et sans aucune valeur du corps.
//! - `encoding` : décodage UTF-8 strict du corps, détection et réparation du mojibake latin-1.
//! - `error_body` : message d'un corps d'erreur PISTE (UTF-8, mojibake, JSON facultatif), détail
//!   nettoyé, privé des secrets puis borné, marqueur « introuvable », détection d'une page HTML.
//! - `http` : client HTTP unique (`reqwest`, TLS rustls), tentative d'envoi (`send_once`) et
//!   lecture bornée du corps ; aucun réessai ni jugement de statut.
//! - `limit` : barrière de charge sortante (`RateGate`), sémaphore de concurrence puis limiteur
//!   de débit `governor` sur l'horloge de l'adapter ; toutes les attentes bornées par le délai.
//! - `map` : mapping des DTO vers le domaine (04 §9) ; socle commun livré : gabarits de chemin
//!   des endpoints (`endpoint`), anomalies d'un mapping comptées (`Diagnostics`, versées à
//!   `Stats::record_mapping`) et erreur d'un champ refusé (`Field`, `not_found`), sans valeur du
//!   corps.
//! - `memo` : mémo de `typePagination` (`PaginationMemo`) : dernière valeur reçue par requête sans
//!   page, 1 h, 1 024 entrées ; décision pure `choose_mode` et lecture tolérante `received_mode`
//!   (04 §6.6, §12.3).
//! - `request` : corps des requêtes envoyées à PISTE ; `request::search` construit celui de
//!   `/search` (`search_body`) et calcule le midi Europe/Paris du filtre `DATE_VERSION` (04 §6).
//! - `retry` : politique de réessai (`run`) : trois requêtes au plus, délai exponentiel avec gigue,
//!   `Retry-After`, bornée par le délai global ; boucle maison, sans `backon`.
//! - `stats` : compteurs en mémoire (`Stats` : mojibake et neuf diagnostics de mapping, ces
//!   derniers journalisés par `record_mapping`) et leur instantané public (`StatsSnapshot`).
//! - `testing` (tests seulement) : aides communes, `ClosedPort` (adresse locale qui refuse la
//!   connexion sur les trois systèmes de la CI).
//! - `time` : horloge monotone (`MonoClock`), attente (`Sleeper`) et course `race`, pour des
//!   tests déterministes sans geler l'horloge `tokio`.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "PisteCorpus::post_json et ce que lui seul emploie (try_send, \
                  RateGate::acquire, TokenManager::invalidate, Payload::Json, \
                  Stats::record_decode, les champs de TokenLease et d'état de PisteCorpus), \
                  request::search::search_body, memo::PaginationMemo, memo::received_mode, les \
                  DTO de /search et de /consult (lawDecree, kaliText, getArticle, juri, cnil, \
                  acco) et ce que seuls ils emploient (bounded_list, capped_list, null_default, \
                  opt_nonempty, opt_raw_date et RawDate de dto::de, dto::budget), les champs non \
                  lus de PisteErrorDto (status, error_code) et d'OAuthErrorDto \
                  (error_description) : consommés par impl LegalCorpus au plan 2g ; map (tous ses \
                  modules) et Stats::record_mapping : consommés par le mapping puis par impl \
                  LegalCorpus au plan 2g, avec add et log_mapping de stats, que seul \
                  record_mapping emploie"
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
mod map;
mod memo;
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
