//! Cœur pur du serveur MCP Légifrance.
//!
//! Aucune I/O, aucun async : identifiants typés, fonds, dates civiles, ECLI,
//! URL Légifrance, contrat de citation, entités et utilitaires texte.

pub mod error;

pub use error::DomainError;
