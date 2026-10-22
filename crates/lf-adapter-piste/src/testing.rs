//! Aides communes aux tests de l'adapter (compilées sous `cfg(test)` seulement).
//!
//! `ClosedPort` : une adresse locale qui refuse la connexion de façon déterministe.
//! `Capture` et `capture_at` : le journal `tracing` capté en mémoire.

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::net::TcpSocket;
use tracing_subscriber::fmt::MakeWriter;

/// Adresse de boucle locale dont le port est réservé mais jamais écouté : toute connexion y est
/// refusée (`ECONNREFUSED` sous Linux) et aucun autre processus ne peut prendre le port.
///
/// À garder en vie pendant tout l'appel testé : la fermer libérerait le port et rouvrirait la
/// course que cette aide supprime.
pub(crate) struct ClosedPort {
    /// Retenu pour sa seule durée de vie : sa fermeture libérerait le port.
    _socket: TcpSocket,
    addr: SocketAddr,
}

impl ClosedPort {
    /// Réserve un port libre de la boucle locale, sans jamais appeler `listen`. Synchrone : le
    /// socket n'est enregistré auprès du runtime qu'au `connect` ou au `listen`.
    pub(crate) fn new() -> Self {
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = socket.local_addr().unwrap();
        Self { _socket: socket, addr }
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

    use tokio::net::{TcpListener, TcpStream};

    use super::ClosedPort;

    #[tokio::test]
    async fn closed_port_refuses_connections() {
        let closed = ClosedPort::new();
        let error = TcpStream::connect(closed.addr()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ConnectionRefused);
    }

    #[tokio::test]
    async fn closed_port_cannot_be_taken_by_another_listener() {
        let closed = ClosedPort::new();
        let error = TcpListener::bind(closed.addr()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AddrInUse);
    }

    #[test]
    fn uri_is_http_on_loopback_without_trailing_slash() {
        let closed = ClosedPort::new();
        assert_eq!(closed.uri(), format!("http://127.0.0.1:{}", closed.addr().port()));
    }
}
