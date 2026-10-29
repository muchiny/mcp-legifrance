//! Paramètres d'entrée des outils (05 §3.3, §5) : une struct `XParams` par outil (`Deserialize` +
//! `JsonSchema`), décodée par [`crate::decode::to_call`] et convertie en entrée applicative.
//!
//! [`reading`] est complet : les cinq paramètres de lecture communs aux trois lectures longues.
//! Chaque fichier d'outil est réduit à sa doc jusqu'à sa tranche.

mod reading;

mod coverage;
mod diff_article;
mod get_act;
mod get_company_agreement;
mod get_convention;
mod get_decision;
mod get_deliberation;
mod get_related;
mod get_text;
mod resolve;
mod search;
mod search_all;
mod verify_citations;
