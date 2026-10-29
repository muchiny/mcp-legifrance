//! Serveur MCP Légifrance : binaire `mcp-legifrance` (stdio) et interface en ligne de commande.
//!
//! Racine de composition (02 §3.7) : seul ce crate lit l'environnement, l'horloge et les
//! signaux, puis assemble les adapters. Les sept modules ci-dessous sont déclarés par la vague 0
//! et réduits à leur doc ; V1 et V2 les écrivent.
//!
//! STUB(V0) → V2-04 : `main` ne fait rien et rend le code 0 tant que la racine de composition
//! (`wiring::run`, codes de sortie) n'est pas écrite.

mod check;
mod cli;
mod clock;
mod config;
mod logging;
mod shutdown;
mod wiring;

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::SUCCESS
}
