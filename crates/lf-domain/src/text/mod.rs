//! Utilitaires texte purs : nettoyage HTML des champs courts (`clean`), texte long en
//! paragraphes ([`LongText`]), réparation du mojibake, fenêtres, passages, classement BM25.
//!
//! `clean` et `LongText::from_html` partagent un lexer HTML écrit à la main (`html`, privé au
//! crate : linéaire, en mémoire bornée, sans perte de texte quand une fermante manque, 03 §7.2).

mod clean;
pub mod encoding;
mod focus;
pub(crate) mod fold;
mod html;
mod long_text;
mod passages;
mod rank;
mod stopwords;
mod summary;
mod window;

pub use clean::{
    CleanReport, clean, clean_field, clean_with_report, flatten_whitespace,
    title_unless_in_citation,
};
pub use focus::{FOCUS_MAX_CHARS, Focus};
pub use fold::fold;
pub use html::is_invisible;
pub use long_text::LongText;
pub use passages::{Passage, passages};
pub use rank::{B, K1, Scored, rank};
pub use summary::{
    DispositifRule, LEAD_MIN_CHARS, SUMMARY_CAP_CHARS, Summary, SummarySource, summary,
};
pub use window::{Window, window};
