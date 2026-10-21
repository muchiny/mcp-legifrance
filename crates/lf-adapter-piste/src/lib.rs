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
//! Les modules sont privés et rien n'est exporté tant que `PisteCorpus` n'existe pas.
//!
//! Modules :
//! - `config` : configuration de l'adapter (`PisteConfig`) et validation des URL surchargées.
//! - `deadline` : échéance d'un appel (`CallDeadline`), durée restante lue dans `Deadline`
//!   puis posée sur l'horloge monotone de l'adapter.
//! - `time` : horloge monotone (`MonoClock`) et attente (`Sleeper`), pour des tests
//!   déterministes sans geler l'horloge `tokio`.

#![cfg_attr(
    not(test),
    expect(dead_code, reason = "modules internes consommés par PisteCorpus au plan 2d")
)]

mod config;
mod deadline;
mod time;

pub use config::{
    ConfigProblem, ConfigWarning, PisteConfig, PisteCredentials, PisteEndpoints, RateQuota,
};
