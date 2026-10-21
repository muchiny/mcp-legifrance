//! Couche application du serveur MCP Légifrance (02 §3.2).
//!
//! Ports, cas d'usage et façade `LegalService` ; le crate ignore MCP, HTTP, JSON et le
//! système de fichiers. Le plan 2b livre le port sortant [`ports::LegalCorpus`] et ses
//! erreurs [`ports::CorpusError`] ; les cas d'usage, `Audited`, `AuditLog`, `Clock` et les
//! doublures de test (`FakeCorpus`, feature `test-support`) arrivent en M3.
//!
//! Dépendances : liste fermée de 02 §3.2 (R2), vérifiée par `cargo xtask check-arch`.

pub mod ports;
