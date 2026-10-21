//! Cœur pur du serveur MCP Légifrance.
//!
//! Aucune I/O, aucun async : identifiants typés, fonds, dates civiles, ECLI,
//! URL Légifrance, contrat de citation, entités et utilitaires texte.

pub mod contract;
pub mod coverage;
pub mod date;
pub mod ecli;
pub mod entity;
pub mod error;
pub mod fond;
pub mod id;
pub mod text;
pub mod url;

pub use error::DomainError;
