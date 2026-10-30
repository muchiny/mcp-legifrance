//! Tâches de développement, testables hors du binaire.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "déploiement par crate de `arithmetic_side_effects` : dérogation provisoire, à retirer avec la PR de ce crate"
)]

pub mod arch;
pub mod args;
pub mod graph;
pub mod instructions;
pub mod lock_descriptors;
pub mod record;
