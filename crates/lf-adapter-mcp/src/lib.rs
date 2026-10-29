//! Composant interne de mcp-legifrance : adapter MCP (rmcp) du service applicatif.
//!
//! Seul ce crate connaît rmcp (02 §3.6). La vague 0 pose l'infrastructure, que les tranches
//! d'outils (V1 à V3) complètent :
//!
//! - [`serve_stdio`] sert un serveur MCP sur stdin et stdout, derrière un filtre de cycle de vie
//!   (`lifecycle`) qui écarte les messages reçus avant toute ère, et se termine avec le code 0
//!   à l'EOF en moins d'une seconde ([`ServeOutcome`], [`ServeError`]) ;
//! - les schémas (`schema`), le décodage tolérant (`lenient`, `canonical`, `decode`), les échecs
//!   in-band (`failure`) et les versions de protocole (`versions`) sont les briques des outils ;
//! - [`Surface`] et [`SURFACE`] disent quelle surface d'outils (v0.1, v0.2 ou v0.3) le serveur
//!   annonce ; les textes sont dans `text`.
//!
//! Les modules sont privés : l'API publique n'expose aucun type de domaine d'outil (02 §3.6).

mod canonical;
mod decode;
mod failure;
mod lenient;
mod lifecycle;
mod schema;
mod stdio;
mod text;
mod versions;

pub use stdio::{ServeError, ServeOutcome, serve_stdio};
pub use text::{SURFACE, Surface};
