//! Signaux du processus (SIGINT, SIGTERM) convertis en `CancellationToken` d'arrêt.

use tokio_util::sync::CancellationToken;

/// Jeton annulé à la réception d'un signal d'arrêt : SIGINT ou SIGTERM sous Unix, Ctrl+C,
/// Ctrl+Break, fermeture de la console ou arrêt du système sous Windows.
///
/// Claude Code envoie SIGINT, puis SIGTERM 100 ms plus tard, puis SIGKILL 400 ms après : le
/// jeton est annulé dès le premier signal, et le serveur sort sans attendre les suivants
/// (02 §8.8). Les écouteurs sont installés avant le retour : un signal reçu juste après n'est
/// pas perdu, et son action par défaut (tuer le processus) est remplacée pour de bon.
///
/// À appeler depuis le runtime tokio (`block_on`), qui porte la tâche d'attente. Si les
/// écouteurs ne s'installent pas, un avertissement est journalisé et le jeton ne sera annulé que
/// par l'appelant : le serveur s'arrête alors à l'EOF de stdin.
pub(crate) fn shutdown_token() -> CancellationToken {
    let token = CancellationToken::new();
    match platform::Signals::register() {
        Ok(mut signals) => {
            let trigger = token.clone();
            tokio::spawn(async move {
                let name = signals.recv().await;
                tracing::info!(signal = name, "shutdown signal received");
                trigger.cancel();
            });
        }
        Err(error) => tracing::warn!(%error, "cannot listen for shutdown signals"),
    }
    token
}

#[cfg(unix)]
mod platform {
    use std::io;
    use tokio::signal::unix::{Signal, SignalKind, signal};

    pub(super) struct Signals {
        interrupt: Signal,
        terminate: Signal,
    }

    impl Signals {
        /// Installe les écouteurs tout de suite (le handler du processus est posé ici).
        pub(super) fn register() -> io::Result<Self> {
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }

        /// Attend le premier signal et rend son nom.
        pub(super) async fn recv(&mut self) -> &'static str {
            tokio::select! {
                _ = self.interrupt.recv() => "SIGINT",
                _ = self.terminate.recv() => "SIGTERM",
            }
        }
    }
}

/// Windows : NON VÉRIFIÉ à l'exécution [V-168 · arret-windows] ; la compilation l'est en CI.
#[cfg(windows)]
mod platform {
    use std::io;
    use tokio::signal::windows::{
        CtrlBreak, CtrlC, CtrlClose, CtrlShutdown, ctrl_break, ctrl_c, ctrl_close, ctrl_shutdown,
    };

    pub(super) struct Signals {
        ctrl_c: CtrlC,
        ctrl_break: CtrlBreak,
        ctrl_close: CtrlClose,
        ctrl_shutdown: CtrlShutdown,
    }

    impl Signals {
        /// Installe les écouteurs tout de suite.
        pub(super) fn register() -> io::Result<Self> {
            Ok(Self {
                ctrl_c: ctrl_c()?,
                ctrl_break: ctrl_break()?,
                ctrl_close: ctrl_close()?,
                ctrl_shutdown: ctrl_shutdown()?,
            })
        }

        /// Attend le premier événement et rend son nom.
        pub(super) async fn recv(&mut self) -> &'static str {
            tokio::select! {
                _ = self.ctrl_c.recv() => "CTRL_C",
                _ = self.ctrl_break.recv() => "CTRL_BREAK",
                _ = self.ctrl_close.recv() => "CTRL_CLOSE",
                _ = self.ctrl_shutdown.recv() => "CTRL_SHUTDOWN",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Les signaux Unix s'envoient sans `unsafe` par `sh` ; Windows n'a pas d'équivalent ici
    // (V-168).
    #[cfg(unix)]
    use super::shutdown_token;
    #[cfg(unix)]
    use std::{process::Command, time::Duration};

    /// Envoie `signal` au processus de test, par `sh` (`libc::kill` serait `unsafe`).
    #[cfg(unix)]
    fn send(signal: &str) {
        let status = Command::new("sh")
            .args(["-c", &format!("kill -{signal} {}", std::process::id())])
            .status()
            .expect("sh doit se lancer");
        assert!(status.success(), "kill -{signal} a échoué");
    }

    #[cfg(unix)]
    async fn assert_cancelled_by(signal: &str) {
        let token = shutdown_token();
        assert!(!token.is_cancelled());
        send(signal);
        tokio::time::timeout(Duration::from_secs(5), token.cancelled())
            .await
            .expect("le signal doit annuler le jeton en 5 s");
    }

    /// 02 §8.8 : SIGTERM déclenche l'arrêt (Claude Code l'envoie après SIGINT).
    #[cfg(unix)]
    #[tokio::test]
    async fn sigterm_cancels_token() {
        assert_cancelled_by("TERM").await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sigint_cancels_token() {
        assert_cancelled_by("INT").await;
    }
}
