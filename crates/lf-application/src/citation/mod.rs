//! Résolution des citations juridiques (M6 et M7, annexe B §6.6) : contrôle, résolution, liens,
//! politique de confiance et notes.
//!
//! Les cinq fichiers sont déclarés en vague 0 ; les unités V3-05 (`verify`, `resolve`, `policy`,
//! `notes`) et V3-06 (`related`) les remplissent. Le parseur de citations lui-même vit dans
//! `lf_domain::citation`.

pub mod notes;
pub mod policy;
pub mod related;
pub mod resolve;
pub mod verify;
