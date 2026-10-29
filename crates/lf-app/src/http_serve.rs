//! Commande `serve-http` (M8) : sert la façade de production en Streamable HTTP sur la boucle
//! locale (02 §8.1 ; annexe D §5.2, §6.2).
//!
//! Le prototype `LegifranceServer` est construit une fois ; la fabrique sans état de rmcp le clone
//! à chaque requête. Les journaux vont sur stderr comme pour `serve`, et stdout ne porte rien. Le
//! code de sortie est 0 à l'arrêt demandé (SIGINT, SIGTERM) et une erreur (code 1 dans `main`) si
//! l'adresse ne peut pas être écoutée ou si le service s'interrompt.

use std::{process::ExitCode, sync::Arc};

use anyhow::Context as _;
use lf_adapter_mcp::{HttpOptions, LegifranceServer, serve_http};
use tokio_util::sync::CancellationToken;

use crate::wiring::AppService;

/// Sert `service` en HTTP selon `opts` jusqu'à l'annulation de `shutdown`.
///
/// # Errors
///
/// L'échec du transport HTTP : adresse prise ou refusée, service interrompu par une erreur
/// d'entrée-sortie. `main` le journalise et sort avec le code 1.
pub(crate) async fn run(
    service: AppService,
    opts: HttpOptions,
    shutdown: CancellationToken,
) -> anyhow::Result<ExitCode> {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        listen = %opts.listen(),
        "starting (HTTP, local profile)"
    );
    let prototype = LegifranceServer::new(Arc::new(service));
    let outcome = serve_http(move || prototype.clone(), opts, shutdown)
        .await
        .context("the MCP HTTP server failed")?;
    tracing::info!(?outcome, "stopped");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        net::{SocketAddr, TcpStream},
        time::Duration,
    };

    use lf_adapter_mcp::HttpToken;

    use super::*;
    use crate::{
        config::{load, test_env::MapEnv},
        wiring::build,
    };

    const TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";

    /// Façade de production sans identifiants (mode dégradé) et sans journal d'audit utilisable :
    /// aucun appel d'outil n'est fait ici, seul le transport l'est.
    fn service() -> AppService {
        build(&load(&MapEnv::new(&[])).config)
    }

    /// Un port libre de la boucle locale, relâché juste avant que le serveur ne le prenne.
    fn free_loopback_address() -> SocketAddr {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// Un GET authentifié sur `/mcp` : 405 si le jeton est bon et que le transport tourne, 401 sinon.
    fn status_of(addr: SocketAddr, authorization: &str) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let request = format!(
            "GET /mcp HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\n{authorization}\
             Connection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply.lines().next().unwrap().to_owned()
    }

    /// Le transport sert la façade, exige le jeton, puis sort avec le code 0 à l'arrêt demandé.
    #[tokio::test]
    async fn run_serves_the_production_service_until_shutdown() {
        let addr = free_loopback_address();
        let opts = HttpOptions::local(addr).unwrap().with_token(HttpToken::new(TOKEN).unwrap());
        let shutdown = CancellationToken::new();
        let server = tokio::spawn(run(service(), opts, shutdown.clone()));

        let authorization = format!("Authorization: Bearer {TOKEN}\r\n");
        let (without, with) = tokio::task::spawn_blocking(move || {
            // Le serveur se lie dans sa propre tâche : attendre qu'il écoute.
            for _ in 0..250 {
                if TcpStream::connect(addr).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            (status_of(addr, ""), status_of(addr, &authorization))
        })
        .await
        .unwrap();
        assert!(without.contains("401"), "{without}");
        assert!(with.contains("405"), "{with}");

        shutdown.cancel();
        let code = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("the server stops")
            .unwrap()
            .unwrap();
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    /// Une adresse prise est un échec du transport, avec le contexte pour le journal.
    #[tokio::test]
    async fn run_fails_when_the_address_is_taken() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let opts = HttpOptions::local(taken.local_addr().unwrap()).unwrap();
        let error = run(service(), opts, CancellationToken::new()).await.unwrap_err();
        assert_eq!(error.to_string(), "the MCP HTTP server failed");
    }
}
