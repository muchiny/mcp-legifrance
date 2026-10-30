//! Couche application du serveur MCP Légifrance (02 §3.2).
//!
//! Ports, cas d'usage et façade `LegalService` ; le crate ignore MCP, HTTP, JSON et le
//! système de fichiers. Le plan 2b livre le port sortant [`ports::LegalCorpus`] et ses
//! erreurs [`ports::CorpusError`] ; le plan 3a pose le socle de M3 : les ports
//! [`ports::AuditLog`] et [`ports::Clock`], l'entrée d'outil [`input::ToolCall`], les noms
//! d'outils [`tools::ToolName`], les codes d'erreur [`error::ErrorCode`], les causes amont
//! [`error::UpstreamCause`] et l'erreur d'application [`error::AppError`] ; le module
//! [`messages`] rédige les textes d'erreur vus par le modèle, et [`failure::port_failure`]
//! journalise l'échec du port avant de lui déléguer le message ; le module `testing` (feature
//! `test-support`) fournit les doublures `FakeCorpus`, `FixedClock`, `MemoryAuditLog` et
//! `FailingAuditLog` ; le décorateur [`audited::Audited`] enveloppe chaque exécution d'outil
//! (délai global, annulation, ligne d'audit). La vague 0 du plan « fin du projet » gèle les
//! interfaces des cas d'usage : le port complet à douze méthodes, les entrées décodées des outils
//! ([`input`]), le budget de réponse ([`budget`]) avec la politique de lecture, la façade
//! [`service::LegalService`] et ses réglages, et les fichiers vides des cas d'usage ([`usecase`]),
//! des sorties ([`output`]) et des citations ([`citation`]) que les vagues suivantes remplissent.
//!
//! Dépendances : liste fermée de 02 §3.2 (R2), vérifiée par `cargo xtask check-arch`.

pub mod audited;
pub mod budget;
pub mod citation;
#[cfg(feature = "test-support")]
pub mod contract;
pub mod deadline;
pub mod error;
pub mod failure;
pub mod input;
pub mod messages;
pub mod output;
pub mod ports;
pub mod service;
pub mod tools;
pub mod usecase;

#[cfg(test)]
mod capture;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
