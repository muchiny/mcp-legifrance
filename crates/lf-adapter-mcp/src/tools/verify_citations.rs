//! Outil `fr_verify_citations` (05 §5). Routeur vide jusqu'à la tranche V3-05, qui écrit ici son `#[tool]` selon le
//! motif de `tools/mod.rs`.

use lf_application::ports::{AuditLog, Clock, LegalCorpus};
use rmcp::tool_router;

use crate::server::LegifranceServer;

// STUB(V0) → V3-05 : bloc vide (`allow_empty`), combiné par `LegifranceServer::new`.
#[tool_router(router = verify_citations_router, vis = "pub(crate)", allow_empty)]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {}
