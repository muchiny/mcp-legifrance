//! Ports sortants (traits RPITIT à futur `+ Send`, dispatch statique, ADR-0012).

pub mod corpus;

pub use corpus::{CorpusError, LegalCorpus};
