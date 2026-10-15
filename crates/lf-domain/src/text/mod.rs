//! Utilitaires texte purs : nettoyage HTML, fenêtres, passages, classement BM25.

mod clean;
mod passages;
mod window;

pub use clean::clean;
pub use passages::{Passage, passages};
pub use window::{Window, window};
