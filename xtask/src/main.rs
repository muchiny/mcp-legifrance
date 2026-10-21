//! `cargo xtask <tâche>` — voir `.cargo/config.toml`.

use std::{env, fs, process::Command, process::ExitCode};

use xtask::arch::{inherits_workspace_lints, violations, workspace_packages};

fn check_arch() -> Result<(), String> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
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
    let found = violations(&packages);
    if found.is_empty() {
        println!("check-arch: ok");
        Ok(())
    } else {
        Err(found.join("\n"))
    }
}

fn main() -> ExitCode {
    let result = match env::args().nth(1).as_deref() {
        Some("check-arch") => check_arch(),
        _ => Err("usage: cargo xtask check-arch".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
