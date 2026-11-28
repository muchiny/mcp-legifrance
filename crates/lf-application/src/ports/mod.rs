//! Ports sortants (traits RPITIT à futur `+ Send`, dispatch statique, ADR-0012).

pub mod audit;
pub mod case_law;
pub mod clock;
pub mod corpus;
pub mod doctrine;
pub mod guidance;

pub use audit::{AuditError, AuditErrorKind, AuditLog, AuditRecord, AuditStatus};
pub use case_law::{CaseLawError, JudicialOpenData, NoCaseLaw};
pub use clock::Clock;
pub use corpus::{ArticleAt, AuthOrigin, ConfigKind, CorpusError, LegalCorpus};
pub use doctrine::{DoctrineError, NoDoctrine, PortalCode, TaxDoctrine};
pub use guidance::{
    ArchiveFile, GuidanceError, GuidanceFound, GuidanceRead, NoGuidance, PublicGuidance,
};
pub use lf_domain::coverage::PisteEnvironment;
