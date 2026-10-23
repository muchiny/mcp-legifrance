//! Utilitaires texte purs : nettoyage HTML, réparation du mojibake, fenêtres, passages, classement BM25.

mod clean;
pub mod encoding;
pub(crate) mod fold;
mod html;
mod long_text;
mod passages;
mod rank;
mod window;

pub use clean::{CleanReport, clean, clean_field, clean_with_report};
pub use long_text::LongText;
pub use passages::{Passage, passages};
pub use rank::{B, K1, Scored, rank};
pub use window::{Window, window};
