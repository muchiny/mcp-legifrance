//! Sorties des outils, un fichier par outil : l'objet que le cas d'usage rend à l'adapter MCP, avec
//! sa métrique d'audit (`AuditMetric`). Les fichiers sont déclarés en vague 0 ; chaque unité remplit
//! les siens.

pub mod complete;
pub mod coverage;
pub mod diff_article;
pub mod get_act;
pub mod get_company_agreement;
pub mod get_convention;
pub mod get_decision;
pub mod get_deliberation;
pub mod get_related;
pub mod get_text;
pub mod resolve;
pub mod resource;
pub mod search;
pub mod search_all;
pub mod verify_citations;
