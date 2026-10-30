//! `cargo xtask <tâche>` — voir `.cargo/config.toml`.

use std::{env, fs, process::Command, process::ExitCode};

use xtask::{
    arch::{inherits_workspace_lints, violations, workspace_packages},
    args::utf8_args,
    graph::{graph_violations, resolved_graph},
    instructions,
    lock_descriptors::{self, LockMode},
    record::example_command,
};

#[expect(
    clippy::disallowed_methods,
    clippy::print_stdout,
    reason = "CLI de développement : CARGO est fourni par cargo, le verdict va sur stdout (exception admise, xtask/src/main.rs)"
)]
fn check_arch() -> Result<(), String> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    // Graphe résolu complet : `--all-features` pour le pire cas, `--locked` pour le verrou.
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--all-features", "--locked"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata JSON: {e}"))?;
    let mut packages = workspace_packages(&metadata)?;
    for package in &mut packages {
        let manifest = fs::read_to_string(&package.manifest_path)
            .map_err(|e| format!("{}: {e}", package.manifest_path))?;
        package.lints_workspace = inherits_workspace_lints(&manifest)
            .map_err(|e| format!("{}: {e}", package.manifest_path))?;
    }
    let mut found = violations(&packages);
    found.extend(graph_violations(&resolved_graph(&metadata)?));
    if found.is_empty() {
        println!("check-arch: ok");
        Ok(())
    } else {
        Err(found.join("\n"))
    }
}

/// Lance l'exemple `record_fixture` de l'adapter avec `args`, environnement et sorties hérités.
/// Le code de sortie de l'exemple (0, 1 sonde non écrite, 2 usage ou identifiants absents) est
/// celui de `cargo xtask`.
#[expect(
    clippy::disallowed_methods,
    clippy::print_stderr,
    reason = "CLI de développement : CARGO est fourni par cargo, l'échec va sur stderr (exception admise, xtask/src/main.rs)"
)]
fn record_fixture(args: &[String]) -> ExitCode {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    match Command::new(cargo).args(example_command(args)).status() {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(e) => {
            eprintln!("cargo run: {e}");
            ExitCode::FAILURE
        }
    }
}

#[expect(
    clippy::print_stderr,
    reason = "CLI de développement : les messages d'usage et d'erreur vont sur stderr (exception admise, xtask/src/main.rs)"
)]
fn main() -> ExitCode {
    // `args_os` : `env::args()` paniquerait sur un argument qui n'est pas de l'UTF-8 (M8).
    let args = match utf8_args(env::args_os()) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let result = match args.get(1).map(String::as_str) {
        Some("record-fixture") => return record_fixture(args.get(2..).unwrap_or_default()),
        Some("check-arch") => check_arch(),
        Some("lock-descriptors") => {
            LockMode::from_args(args.get(2..).unwrap_or_default()).and_then(lock_descriptors::run)
        }
        Some("instructions-size") if args.len() == 2 => instructions::run(),
        _ => Err("usage: cargo xtask check-arch | record-fixture <arguments> | \
                  lock-descriptors [--check | --update | --compare <ref-git>] | \
                  instructions-size"
            .into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
