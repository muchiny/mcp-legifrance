//! Entités renvoyées par le corpus. Les entités consultables et chaque
//! `SearchHit` implémentent [`Citable`](crate::citation::Citable) ;
//! `TocEntry` et `TextBody` non.
//!
//! Les champs publics en français (`etat`, `juridiction`, `raison_sociale`…)
//! reprennent les clés JSON de sortie de la référence (`citations.py:213-227`,
//! `264-275`, `341-354`, `381-394`, `465-485`) : §0, noms de champs JSON tels
//! quels, parité de migration.

mod act;
mod agreement;
mod article;
mod decision;
mod deliberation;
mod search;

pub use act::{Act, Convention, TextBody, TocEntry};
pub use agreement::{ATTACHMENT_NOTE, CompanyAgreement};
pub use article::ArticleText;
pub use decision::Decision;
pub use deliberation::Deliberation;
pub use search::{CodeArticleRef, SNIPPET_MAX_CHARS, SearchHit, SearchPage};
