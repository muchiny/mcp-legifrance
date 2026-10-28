//! Couche application du serveur MCP Légifrance (02 §3.2).
//!
//! Ports, cas d'usage et façade `LegalService` ; le crate ignore MCP, HTTP, JSON et le
//! système de fichiers. Le plan 2b livre le port sortant [`ports::LegalCorpus`] et ses
//! erreurs [`ports::CorpusError`] ; le plan 3a pose le socle de M3 : les ports
//! [`ports::AuditLog`] et [`ports::Clock`], l'entrée d'outil [`input::ToolCall`], les noms
//! d'outils [`tools::ToolName`], les codes d'erreur [`error::ErrorCode`], les causes amont
//! [`error::UpstreamCause`] et l'erreur d'application [`error::AppError`] ; le module
//! [`messages`] rédige les textes d'erreur vus par le modèle ; le module `testing` (feature
//! `test-support`) fournit les doublures `FakeCorpus`, `FixedClock`, `MemoryAuditLog` et
//! `FailingAuditLog`. Les cas d'usage et `Audited` suivent dans les tâches suivantes du plan.
//!
//! Dépendances : liste fermée de 02 §3.2 (R2), vérifiée par `cargo xtask check-arch`.

pub mod deadline;
pub mod error;
pub mod input;
pub mod messages;
pub mod ports;
pub mod tools;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;
