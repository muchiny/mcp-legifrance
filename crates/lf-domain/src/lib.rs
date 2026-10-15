//! Cœur pur du serveur MCP Légifrance.
//!
//! Aucune I/O, aucun async : identifiants typés, fonds, dates civiles, ECLI,
//! URL Légifrance, contrat de citation, entités et utilitaires texte.

pub mod date;
pub mod error;
pub mod fond;
pub mod id;

pub use error::DomainError;
