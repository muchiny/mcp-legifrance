//! Doctrine fiscale : les valeurs du BOFiP (Bulletin officiel des finances publiques, impôts),
//! la doctrine publiée de l'administration fiscale (ADR-0028, ADR-0029, ADR-0030).
//!
//! Le BOFiP n'est pas un fond de Légifrance : ses identifiants (`BOI-…`, `ACTU-…`) n'ont ni la
//! forme ni le sens d'un [`Id`](crate::id::Id), ses adresses ne sont pas sous
//! [`WEB_BASE`](crate::url::WEB_BASE), et sa recherche n'a ni opérateur ni mode de correspondance.
//! Il a donc ses propres types, tous validés sans réseau : une valeur construite est déjà
//! conforme, et l'adapter n'écrit dans la requête amont que des valeurs typées ici (ADR-0029,
//! décision 3).
//!
//! - entrées : [`BoiId`], [`PublicationId`], [`Serie`], [`KindFilter`], [`DoctrineTerms`],
//!   réunies dans [`DoctrineQuery`] pour une recherche ;
//! - sorties : [`DoctrineHit`], [`DoctrinePage`], [`DoctrineDoc`], de nature [`DoctrineKind`] ;
//! - données du portail rendues sûres : [`publication_text`] pour le texte d'une publication
//!   (son champ HTML, et [`clean_text`] pour son champ brut, en secours), [`clean_line`] pour un
//!   champ court,
//!   [`Permalink`] et [`BofipUrl`] pour l'adresse, [`doctrine_citation`] pour la citation.

mod entity;
mod id;
mod kind;
mod query;
mod terms;
mod text;
mod url;

pub use entity::{DoctrineDoc, DoctrineHit, DoctrinePage, doctrine_citation};
pub use id::{BOI_ID_MAX_BYTES, BoiId, PublicationId, SERIE_MAX_BYTES, Serie};
pub use kind::{DoctrineKind, KindFilter};
pub use query::DoctrineQuery;
pub use terms::DoctrineTerms;
pub use text::{LINE_MAX_CHARS, PublicationText, clean_line, clean_text, publication_text};
pub use url::{BOFIP_PAGE_BASE, BOFIP_URL_MAX_BYTES, BofipUrl, Permalink};
