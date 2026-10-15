//! Utilitaires texte purs : nettoyage HTML, fenêtres, passages, classement BM25.

mod clean;
mod fold;
mod passages;
mod rank;
mod window;

pub use clean::clean;
pub use passages::{Passage, passages};
pub use rank::{B, K1, Scored, rank};
pub use window::{Window, window};
