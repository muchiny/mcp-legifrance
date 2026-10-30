//! Binaire du spike : serveur MCP sur stdin et stdout, code 0 à l'EOF (annexe D §4.3 point 3).

use std::{process::ExitCode, sync::Arc};

use lf_application::{
    audited::Audited,
    testing::{FakeCorpus, MemoryAuditLog},
};
use rmcp_stdio_spike::{Spike, serve_io, spike_policy};
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let audited = Arc::new(Audited::new(MemoryAuditLog::new(), spike_policy()));
    let spike = Spike::new(Arc::new(FakeCorpus::new()), audited);
    let shutdown = CancellationToken::new();
    match serve_io(spike, tokio::io::stdin(), tokio::io::stdout(), shutdown).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
