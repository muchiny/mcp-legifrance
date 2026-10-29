//! Outil `fr_get_convention` (05 §5). Routeur vide jusqu'à la tranche V1-02, qui écrit ici son `#[tool]` selon le
//! motif de `tools/mod.rs`.

use lf_application::ports::{AuditLog, Clock, LegalCorpus};
use rmcp::tool_router;

use crate::server::LegifranceServer;

// STUB(V0) → V1-02 : bloc vide (`allow_empty`), combiné par `LegifranceServer::new`.
#[tool_router(router = get_convention_router, vis = "pub(crate)", allow_empty)]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {}
