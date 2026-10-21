//! Adapter PISTE du serveur MCP Légifrance (04, 02 §3.3).
//!
//! Rôle : `PisteCorpus`, seule implémentation du port `LegalCorpus` jusqu'en M9. Il parle
//! HTTP à l'API Légifrance via PISTE (OAuth 2.0 `client_credentials`), avec limiteur de
//! débit et réessais, et traduit les réponses en types du domaine.
//!
//! Livré au plan 2c : le squelette, les traits de temps, la configuration,
//! l'authentification, le client HTTP et la résilience. Renvoyé au plan 2d : les requêtes
//! `/search`, les DTO métier, le mapping et `impl LegalCorpus for PisteCorpus`.
//!
//! Les modules sont privés ; seuls la configuration et `StatsSnapshot` sont exportés tant
//! que `PisteCorpus` n'existe pas.
//!
//! Modules :
//! - `config` : configuration de l'adapter (`PisteConfig`) et validation des URL surchargées.
//! - `deadline` : échéance d'un appel (`CallDeadline`), durée restante lue dans `Deadline`
//!   puis posée sur l'horloge monotone de l'adapter.
//! - `encoding` : décodage UTF-8 strict du corps, détection et réparation du mojibake latin-1.
//! - `http` : client HTTP unique (`reqwest`, TLS rustls), tentative d'envoi (`send_once`) et
//!   lecture bornée du corps ; aucun réessai ni jugement de statut.
//! - `stats` : compteurs en mémoire (`Stats`) et leur instantané public (`StatsSnapshot`).
//! - `time` : horloge monotone (`MonoClock`) et attente (`Sleeper`), pour des tests
//!   déterministes sans geler l'horloge `tokio`.

#![cfg_attr(
    not(test),
    expect(dead_code, reason = "modules internes consommés par PisteCorpus au plan 2d")
)]

mod config;
mod deadline;
mod encoding;
mod http;
mod stats;
mod time;

pub use config::{
    ConfigProblem, ConfigWarning, PisteConfig, PisteCredentials, PisteEndpoints, RateQuota,
};
pub use stats::StatsSnapshot;
