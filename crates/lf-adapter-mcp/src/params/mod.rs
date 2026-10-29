//! Paramètres d'entrée des outils (05 §3.3, §5) : une struct `XParams` par outil (`Deserialize` +
//! `JsonSchema`), décodée par [`crate::decode::to_call`] et convertie en entrée applicative.
//!
//! [`reading`] est complet : les cinq paramètres de lecture communs aux trois lectures longues.
//! Chaque fichier d'outil est réduit à sa doc jusqu'à sa tranche.

mod reading;

pub(crate) mod coverage;
pub(crate) mod diff_article;
pub(crate) mod get_act;
pub(crate) mod get_company_agreement;
pub(crate) mod get_convention;
pub(crate) mod get_decision;
pub(crate) mod get_deliberation;
pub(crate) mod get_related;
pub(crate) mod get_text;
pub(crate) mod resolve;
pub(crate) mod search;
pub(crate) mod search_all;
pub(crate) mod verify_citations;
