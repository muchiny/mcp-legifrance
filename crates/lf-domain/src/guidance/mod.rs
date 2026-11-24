//! Service-Public : les valeurs des fiches pratiques de Service-Public.gouv.fr (DILA),
//! l'information administrative rédigée pour le public (ADR-0033).
//!
//! Une fiche n'est pas un texte de Légifrance : elle n'a ni fonds, ni version à une date, ni
//! identifiant de la forme d'un [`Id`](crate::id::Id), et ce n'est pas de la doctrine. Elle a donc
//! ses propres types, tous validés sans réseau : une valeur construite est déjà conforme.
//!
//! - entrées : [`SheetId`], [`Audience`], [`AudienceFilter`], [`GuidanceTerms`] ;
//! - forme de comparaison d'un mot, commune à la requête et à l'index : [`fold_word`] ;
//! - textes de Légifrance cités par une fiche : [`legal_references`] (ADR-0033, décision 8).

mod audience;
mod id;
mod line;
mod references;
mod terms;

pub use audience::{Audience, AudienceFilter};
pub use id::{SHEET_ID_MAX_DIGITS, SheetId};
pub use line::LINE_MAX_CHARS;
pub use references::{
    LEGAL_REFERENCES_MAX, LegalReference, LegalReferences, legal_references, source_urls,
};
pub use terms::{GuidanceTerms, fold_word};
