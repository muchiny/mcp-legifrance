//! Colle `serve_stdio` : le serveur MCP sur les vrais stdin et stdout du processus.
//!
//! Ce fichier est la seule exclusion de la couverture `--lib` (D6, `COV_IGNORE` du Makefile) :
//! il se réduit à l'appel de [`crate::lifecycle::serve_io`] avec `tokio::io::stdin()` et
//! `tokio::io::stdout()`, que l'e2e du binaire exerce. Toute la logique du cycle de vie (filtre,
//! EOF, arrêt, traduction des échecs) vit dans `lifecycle.rs`, qui est mesuré.

use rmcp::{ServerHandler, service::ServerInitializeError};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::lifecycle::serve_io;

/// Fin normale du service : le code de sortie du processus est 0 dans les deux cas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeOutcome {
    /// L'entrée s'est fermée (EOF), avant ou après l'ouverture d'une ère.
    Eof,
    /// Arrêt demandé par le jeton `shutdown` (signal du processus).
    Shutdown,
}

/// Échec du service : le code de sortie du processus est 1.
#[derive(Debug, Error)]
pub enum ServeError {
    /// Initialisation impossible, hors EOF et arrêt. La variante est boîtée : une
    /// `ServerInitializeError` pèse 368 octets (`clippy::large_enum_variant`, `result_large_err`).
    #[error("MCP initialization failed: {0}")]
    Initialize(Box<ServerInitializeError>),
    /// La tâche de service a paniqué ou a été interrompue.
    #[error("MCP service task failed: {0}")]
    Join(tokio::task::JoinError),
    /// Écoute impossible (adresse prise ou refusée) ou service HTTP interrompu par une erreur
    /// d'entrée-sortie (`serve_http`, M8).
    #[cfg(feature = "http")]
    #[error("MCP HTTP transport failed: {0}")]
    Http(std::io::Error),
}

/// Sert `server` en JSON-RPC sur stdin et stdout jusqu'à l'EOF de stdin ou jusqu'à l'annulation de
/// `shutdown`. stdout ne porte que du JSON-RPC (Q9) ; les journaux vont sur stderr.
///
/// # Errors
///
/// [`ServeError`] si l'initialisation échoue autrement que par EOF ou arrêt, ou si la tâche de
/// service échoue.
pub async fn serve_stdio<S: ServerHandler>(
    server: S,
    shutdown: CancellationToken,
) -> Result<ServeOutcome, ServeError> {
    serve_io(server, tokio::io::stdin(), tokio::io::stdout(), shutdown).await
}
