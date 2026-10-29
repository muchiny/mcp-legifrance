//! Composant interne de mcp-legifrance : adapter MCP (rmcp) du service applicatif.
//!
//! Seul ce crate connaît rmcp (02 §3.6). La vague 0 pose l'infrastructure, que les tranches
//! d'outils (V1 à V3) complètent :
//!
//! - [`LegifranceServer`] est le serveur : il additionne les routeurs des 13 outils (`tools`),
//!   sert les resources, les prompts et les complétions (`resources`, `prompts`, `completions`,
//!   vides jusqu'à M7) ; les paramètres (`params`) et les vues (`views`) des outils sont décrits
//!   par `schemars` ;
//! - [`serve_stdio`] sert un serveur MCP sur stdin et stdout, derrière un filtre de cycle de vie
//!   (`lifecycle`) qui écarte les messages reçus avant toute ère, et se termine avec le code 0
//!   à l'EOF en moins d'une seconde ([`ServeOutcome`], [`ServeError`]) ;
//! - les schémas (`schema`), le décodage tolérant (`lenient`, `canonical`, `decode`), les échecs
//!   in-band (`failure`) et les versions de protocole (`versions`) sont les briques des outils ;
//! - [`Surface`] et [`SURFACE`] disent quelle surface d'outils (v0.1, v0.2 ou v0.3) le serveur
//!   annonce ; les textes sont dans `text` ;
//! - `testing` (feature `test-support`) relie un vrai client rmcp au serveur par un tube mémoire
//!   et monte un serveur sur les doubles de `lf-application`.
//!
//! Les modules sont privés, sauf `testing` : l'API publique n'expose aucun type de domaine d'outil (02 §3.6).

mod canonical;
mod completions;
mod decode;
mod failure;
mod lenient;
mod lifecycle;
mod params;
mod prompts;
mod resources;
mod schema;
mod server;
mod stdio;
mod text;
mod tools;
mod versions;
mod views;

#[cfg(feature = "test-support")]
pub mod testing;

pub use server::LegifranceServer;
pub use stdio::{ServeError, ServeOutcome, serve_stdio};
pub use text::{SURFACE, Surface};
