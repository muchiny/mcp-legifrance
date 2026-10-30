//! Cœur pur du serveur MCP Légifrance.
//!
//! Aucune I/O, aucun async : identifiants typés, fonds, dates civiles, ECLI,
//! URL Légifrance, contrat de citation, entités et utilitaires texte.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "déploiement par crate de `arithmetic_side_effects` : dérogation provisoire, à retirer avec la PR de ce crate"
)]

pub mod citation;
pub mod contract;
pub mod coverage;
pub mod date;
pub mod ecli;
pub mod entity;
pub mod error;
pub mod fond;
pub mod id;
pub mod search;
pub mod status;
pub mod text;
pub mod url;

pub use error::DomainError;
