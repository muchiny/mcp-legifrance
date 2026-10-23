//! Aides communes aux tests de l'adapter (compilées sous `cfg(test)` seulement).
//!
//! `ClosedPort` : une adresse locale qui refuse la connexion, sur les trois systèmes de la CI.
//! `Capture` et `capture_at` : le journal `tracing` capté en mémoire.

use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

/// Adresse de boucle locale où personne n'écoute : toute connexion y est refusée
/// (`ECONNREFUSED`) sur Linux, macOS et Windows.
///
/// Le port est obtenu en liant `127.0.0.1:0` puis en fermant le socket. Réserver le port en le
/// gardant lié sans `listen` supprimerait la course avec un autre processus, mais macOS n'y
/// répond pas par un refus : la connexion attend alors son délai (CI macos-15, plan 2d). Reste une
/// course théorique, un autre processus devant lier exactement ce port entre la fermeture et la
/// connexion, et les noyaux évitent de réattribuer aussitôt un port éphémère.
pub(crate) struct ClosedPort {
    addr: SocketAddr,
}

impl ClosedPort {
    /// Relève un port libre de la boucle locale puis le libère.
    pub(crate) fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        Self { addr }
    }

    /// Adresse du port fermé.
    pub(crate) fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// URI de base `http://127.0.0.1:port`, sans barre finale.
    pub(crate) fn uri(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Journal `tracing` capté en mémoire.
#[derive(Clone, Default)]
pub(crate) struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl Capture {
    /// Tout ce qui a été journalisé jusqu'ici.
    pub(crate) fn text(&self) -> String {
        io::Write::flush(&mut self.clone()).unwrap();
        String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MakeWriter<'_> for Capture {
    type Writer = Capture;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

/// Abonné local au thread courant (le runtime des tests est mono-fil), à garder en vie pendant
/// tout l'appel testé.
pub(crate) fn capture_at(level: tracing::Level) -> (Capture, tracing::subscriber::DefaultGuard) {
    let capture = Capture::default();
    let subscriber =
        tracing_subscriber::fmt().with_writer(capture.clone()).with_max_level(level).finish();
    (capture, tracing::subscriber::set_default(subscriber))
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use tokio::net::TcpStream;

    use super::ClosedPort;

    #[tokio::test]
    async fn closed_port_refuses_connections() {
        let closed = ClosedPort::new();
        let error = TcpStream::connect(closed.addr()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ConnectionRefused);
    }

    #[test]
    fn uri_is_http_on_loopback_without_trailing_slash() {
        let closed = ClosedPort::new();
        assert_eq!(closed.uri(), format!("http://127.0.0.1:{}", closed.addr().port()));
    }
}
