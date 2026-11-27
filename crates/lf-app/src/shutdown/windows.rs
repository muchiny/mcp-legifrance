//! Signaux d'arrêt sous Windows : Ctrl+C, Ctrl+Break, fermeture de la console, arrêt du système.
//!
//! NON VÉRIFIÉ à l'exécution [V-168 · arret-windows] ; la compilation l'est en CI. Dans un fichier
//! à part : le balayage de mutation tourne sous Linux, où ce code n'est pas compilé, et l'écarte
//! (`exclude_globs` de `.cargo/mutants.toml`).

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
