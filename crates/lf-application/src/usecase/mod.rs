//! Cas d'usage, un fichier par outil (02 §6.2) : analyse des entrées vers le domaine, appels au
//! port, politique de réponse. Les fichiers sont déclarés en vague 0 ; chaque unité remplit les siens.

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
