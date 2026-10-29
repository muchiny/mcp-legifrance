//! Outil `fr_search_all` (05 §5). Routeur vide jusqu'à la tranche V2-07, qui écrit ici son `#[tool]` selon le
//! motif de `tools/mod.rs`.

use lf_application::ports::{AuditLog, Clock, LegalCorpus};
use rmcp::tool_router;

use crate::server::LegifranceServer;

// STUB(V0) → V2-07 : bloc vide (`allow_empty`), combiné par `LegifranceServer::new`.
#[tool_router(router = search_all_router, vis = "pub(crate)", allow_empty)]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {}
