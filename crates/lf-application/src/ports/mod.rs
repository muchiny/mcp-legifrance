//! Ports sortants (traits RPITIT à futur `+ Send`, dispatch statique, ADR-0012).

pub mod audit;
pub mod clock;
pub mod corpus;

pub use audit::{AuditError, AuditErrorKind, AuditLog, AuditRecord, AuditStatus};
pub use clock::Clock;
pub use corpus::{ArticleAt, AuthOrigin, ConfigKind, CorpusError, LegalCorpus};
pub use lf_domain::coverage::PisteEnvironment;
