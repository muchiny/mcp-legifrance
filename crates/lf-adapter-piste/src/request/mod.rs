//! Corps des requêtes envoyées à PISTE.
//!
//! `search` construit celui de `/search` ; `consult` ceux des six consultations de la v0.1 ;
//! `m6` et `m7` ceux des opérations de M6 (recherche par numéro, liste des codes) et de M7
//! (versions et liens d'un article).

pub(crate) mod consult;
pub(crate) mod m6;
pub(crate) mod m7;
pub(crate) mod search;
