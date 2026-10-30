//! Adapter DILA : ingestion des dumps du fonds LEGI (M9, tâche T-M9-02), index tantivy et port
//! [`DilaCorpus`] (T-M9-03).
//!
//! Ce crate n'est compilé dans les binaires publiés que derrière la feature `dila` (D9). Il lit
//! les archives `tar.gz` de la DILA **en flux, sans rien extraire sur le disque**
//! (`flate2` → `tar` → `quick-xml`, [`ingest`]), tient un registre des archives appliquées
//! ([`registry`]) et refait le corpus quand un nouveau global arrive ([`sync`]). Les documents
//! lus ([`model`]) vont dans un [`DocStore`] : [`IndexStore`] les range dans l'index tantivy
//! ([`index`]), [`MemoryStore`] en mémoire (tests, petits dumps). [`DilaCorpus`] ([`corpus`])
//! lit cet index et implémente `LegalCorpus` : les opérations que les dumps LEGI ne couvrent pas
//! (décisions, CNIL, KALI, ACCO) rendent une erreur de configuration, décision 00 §5.
//!
//! # Parcours
//!
//! 1. [`sync_directory`] liste le répertoire d'archives, consulte le [`Registry`] et en tire un
//!    [`Plan`] : le global d'abord (qui vide le magasin), puis les incréments par ordre
//!    chronologique ;
//! 2. [`ingest_archive`] lit chaque archive : le contenu est rangé membre par membre, les
//!    suppressions des listes `.dat` sont appliquées **après** tout le contenu, par chemin ;
//! 3. un membre défectueux est compté dans l'[`IngestReport`] sans arrêter l'archive ; une
//!    archive illisible arrête la synchronisation et laisse le registre dans l'état exact de ce
//!    qui est appliqué.
//!
//! # Sécurité
//!
//! Rien n'est écrit hors du magasin et du registre. Les chemins sont validés (pas de `..`, pas
//! de chemin absolu, 512 octets au plus), les liens ne sont jamais suivis, les membres sont
//! bornés à 16 Mio, `DOCTYPE` et `ENTITY` sont refusés. Aucune erreur ne cite le contenu d'un
//! membre.

pub mod archive;
mod codec;
pub mod corpus;
pub mod error;
pub mod index;
pub mod ingest;
mod map;
pub mod model;
pub mod path;
pub mod registry;
pub mod store;
pub mod suppression;
pub mod sync;
pub mod xml;

#[cfg(test)]
mod testing;

pub use archive::{ArchiveKind, ArchiveName, ArchiveStamp};
pub use corpus::DilaCorpus;
pub use error::{IngestError, StoreError};
pub use index::{Engine, IndexStore, french_analyzer};
pub use ingest::{IngestLimits, IngestReport, Rejection, ingest_archive};
pub use model::Document;
pub use registry::{ArchiveStatus, Plan, RebuildReason, Registry};
pub use store::{DocStore, MemoryStore, StoredDoc};
pub use sync::{AppliedArchive, SyncReport, sync_directory};
