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
