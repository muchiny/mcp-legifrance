//! Aides communes aux tests de l'adapter (compilées sous `cfg(test)` seulement).
//!
//! `ClosedPort` : une adresse locale qui refuse la connexion de façon déterministe.

use std::net::SocketAddr;

use tokio::net::TcpSocket;

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
