//! Vues de sortie des outils (05 §3.4, §5.0.3, §5.0.4).
//!
//! Une struct `XView` par outil (`Serialize` + `JsonSchema`), construite depuis le `XOutput` de
//! l'application. Les briques communes sont réelles : [`common`] (règles de rendu) et [`fit`]
//! (garde du plafond dur). `reading` et `toc` sont les vues partagées des lectures longues et des
//! tables des matières ; chaque fichier d'outil est réduit à sa doc jusqu'à sa tranche.

mod common;
mod fit;
mod reading;
mod toc;

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
