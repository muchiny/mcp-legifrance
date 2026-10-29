//! Processus fils : le binaire en stdio (05 §9.2 point 1), `cargo build` et `git show`.
//!
//! Colle d'entrées-sorties réduite au minimum ; la logique des sessions est dans `session.rs`. Les
//! tests relancent le binaire de test lui-même comme faux serveur (voir `fixtures::peer_main`).

use std::{
    ffi::{OsStr, OsString},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::session::Transport;

/// Comment lancer le serveur.
#[derive(Debug, Clone)]
pub(crate) struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Variables d'environnement : l'environnement du fils est vidé, seules celles-ci passent.
    pub env: Vec<(OsString, OsString)>,
    /// Attente maximale d'un message du serveur.
    pub read_timeout: Duration,
    /// Délai accordé au serveur pour quitter après l'EOF de stdin (05 §2.2 : moins d'une seconde).
    pub exit_grace: Duration,
}

impl Launch {
    /// Lancement du binaire `program serve` : environnement vidé, 30 s par message, 5 s pour
    /// quitter.
    pub(crate) fn serve(program: PathBuf) -> Self {
        Self {
            program,
            args: vec![OsString::from("serve")],
            env: Vec::new(),
            read_timeout: Duration::from_secs(30),
            exit_grace: Duration::from_secs(5),
        }
    }
}

/// Transport sur le stdin et le stdout d'un processus. Une lecture dans un fil dédié permet
/// d'abandonner l'attente d'un serveur qui se tait.
struct ProcessTransport {
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    read_timeout: Duration,
    /// Lignes de stdout qui n'étaient pas un objet JSON.
    ignored: usize,
}

impl Transport for ProcessTransport {
    fn send(&mut self, message: &Value) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("the server's stdin is closed")?;
        writeln!(stdin, "{message}")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("cannot write to the server: {error}"))
    }

    fn receive(&mut self) -> Result<Value, String> {
        loop {
            let line = self.lines.recv_timeout(self.read_timeout).map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    format!(
                        "no message from the server within {} s",
                        self.read_timeout.as_secs_f32()
                    )
                }
                RecvTimeoutError::Disconnected => "the server closed its output".to_owned(),
            })?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(line) {
                Ok(message) if message.is_object() => return Ok(message),
                _ => self.ignored += 1,
            }
        }
    }
}

/// Attend la fin de `child` jusqu'à `grace`, puis le tue.
fn wait_for_exit(child: &mut Child, grace: Duration) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + grace;
    loop {
        // Une erreur de `try_wait` vaut « pas encore parti » : le délai tranche.
        if let Some(status) = child.try_wait().ok().flatten() {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            // Échecs de `kill` et de `wait` ignorés : le processus est déjà parti ou le sera.
            child.kill().ok();
            child.wait().ok();
            return Err(format!(
                "the server did not exit within {} s of the end of its input",
                grace.as_secs_f32()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Un serveur lancé : le processus, son canal de lignes et le délai de sortie qu'on lui accorde.
struct Running {
    child: Child,
    transport: ProcessTransport,
    exit_grace: Duration,
}

impl Running {
    /// Lance `launch` : stdin et stdout en tubes, environnement vidé, stderr hérité.
    fn start(launch: &Launch) -> Result<Self, String> {
        let mut child = Command::new(&launch.program)
            .args(&launch.args)
            .env_clear()
            .envs(launch.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", launch.program.display()))?;
        let (sender, lines) = mpsc::channel();
        // Lecteur détaché : il s'arrête quand le fils ferme (ou perd) son stdout.
        let _reader = child.stdout.take().map(|stdout| {
            thread::spawn(move || {
                BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                    .try_for_each(|line| sender.send(line))
                    .ok();
            })
        });
        let transport = ProcessTransport {
            stdin: child.stdin.take(),
            lines,
            read_timeout: launch.read_timeout,
            ignored: 0,
        };
        Ok(Self { child, transport, exit_grace: launch.exit_grace })
    }

    /// Ferme stdin. Après l'échec de la session, tue le serveur ; sinon exige qu'il quitte avec le
    /// code 0 dans le délai. Rend le nombre de lignes de stdout ignorées (non JSON).
    fn stop(mut self, session_failed: bool) -> Result<usize, String> {
        self.transport.stdin = None;
        if session_failed {
            self.child.kill().ok();
            self.child.wait().ok();
            return Ok(self.transport.ignored);
        }
        let status = wait_for_exit(&mut self.child, self.exit_grace)?;
        if status.success() {
            Ok(self.transport.ignored)
        } else {
            Err(format!(
                "the server exited with {status} after the end of its input, expected success"
            ))
        }
    }
}

/// Lance le serveur, joue `session` dessus, ferme son stdin et exige une sortie propre (code 0).
/// Rend le nombre de lignes de stdout ignorées (non JSON). `session` garde ce qu'elle lit dans ses
/// propres variables : une seule instanciation à couvrir, sans type générique.
///
/// # Errors
///
/// Le processus ne démarre pas, `session` échoue (le processus est alors tué), ou le serveur ne
/// quitte pas avec le code 0 dans le délai après l'EOF.
pub(crate) fn with_server(
    launch: &Launch,
    session: &mut dyn FnMut(&mut dyn Transport) -> Result<(), String>,
) -> Result<usize, String> {
    let mut running = Running::start(launch)?;
    let outcome = session(&mut running.transport);
    let stopped = running.stop(outcome.is_err());
    // L'échec de la session prime sur celui de l'arrêt.
    outcome?;
    stopped
}

/// Exécute `program args` et rend son stdout. `show_stderr` laisse passer le stderr du fils (la
/// progression de `cargo build`) ; sinon il est repris dans le message d'échec.
///
/// # Errors
///
/// Le processus ne démarre pas, ou sort avec un code non nul.
pub(crate) fn run_capture(
    program: &OsStr,
    args: &[OsString],
    show_stderr: bool,
) -> Result<String, String> {
    let stderr = if show_stderr { Stdio::inherit() } else { Stdio::piped() };
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(stderr)
        .output()
        .map_err(|error| format!("cannot run {}: {error}", Path::new(program).display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        let words: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        Err(format!(
            "{} {} failed ({}): {}",
            Path::new(program).display(),
            words.join(" "),
            output.status,
            detail.trim()
        ))
    }
}

/// Exécute `git -C root args…` et rend son stdout ; le stderr de git entre dans l'erreur.
///
/// # Errors
///
/// Git ne démarre pas, ou sort avec un code non nul.
pub(crate) fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut words = vec![OsString::from("-C"), root.as_os_str().to_owned()];
    words.extend(args.iter().map(OsString::from));
    run_capture(OsStr::new("git"), &words, false)
}

/// Exécutable que `cargo build --message-format=json` a produit pour la cible binaire `name`.
///
/// # Errors
///
/// Aucun message `compiler-artifact` n'annonce d'exécutable pour cette cible.
pub(crate) fn built_executable(cargo_output: &str, name: &str) -> Result<PathBuf, String> {
    cargo_output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["target"]["name"] == name
        })
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from))
        .next_back()
        .ok_or_else(|| format!("cargo did not report an executable for `{name}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_descriptors::{
        fixtures::{peer, peer_args},
        session::{SESSIONS, SessionKind, run_session},
        surface::Capture,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::env;

    /// Session qui ne fait rien : le serveur reçoit un EOF immédiat.
    #[expect(clippy::unnecessary_wraps, reason = "doit avoir la signature d'une session")]
    fn nothing(_: &mut dyn Transport) -> Result<(), String> {
        Ok(())
    }

    /// Session qui envoie une requête sans attendre de réponse.
    fn one_request(transport: &mut dyn Transport) -> Result<(), String> {
        transport.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
    }

    /// Joue `kind` sur un vrai processus : la capture et le nombre de lignes ignorées.
    fn play(launch: &Launch, kind: SessionKind) -> Result<(Capture, usize), String> {
        let mut capture = None;
        let ignored = with_server(launch, &mut |transport| {
            capture = Some(run_session(transport, kind)?);
            Ok(())
        })?;
        Ok((capture.unwrap(), ignored))
    }

    #[test]
    fn the_three_sessions_run_against_real_processes_that_exit_cleanly_at_eof() {
        let launch = peer("server");
        let mut labels = Vec::new();
        for kind in SESSIONS {
            let (capture, ignored) = play(&launch, kind).unwrap();
            // « running 1 test » de libtest passe par stdout : ignoré, pas fatal.
            assert!(ignored >= 1, "{ignored}");
            assert_eq!(capture.tools.len(), 2);
            labels.push(capture.label);
        }
        assert_eq!(
            labels,
            ["discover 2026-07-28", "initialize 2025-11-25", "initialize 2025-06-18"]
        );
    }

    #[test]
    fn lines_that_are_not_json_objects_are_counted_and_skipped() {
        let (capture, ignored) = play(&peer("noisy"), SessionKind::Modern).unwrap();
        assert_eq!(capture.tools.len(), 2);
        assert!(ignored >= 2, "la bannière et l'en-tête de libtest : {ignored}");
    }

    #[test]
    fn a_silent_server_times_out_and_is_killed() {
        let mut launch = peer("silent");
        launch.read_timeout = Duration::from_millis(1500);
        let error = play(&launch, SessionKind::Modern).unwrap_err();
        assert_eq!(
            error,
            "discover 2026-07-28: server/discover: no message from the server within 1.5 s"
        );
    }

    #[test]
    fn a_server_that_ignores_what_it_reads_still_exits_cleanly_at_eof() {
        assert_eq!(with_server(&peer("silent"), &mut one_request), Ok(0));
    }

    #[test]
    fn a_server_that_exits_late_but_within_the_delay_is_accepted() {
        assert_eq!(with_server(&peer("sleep=300"), &mut nothing).map(|_| ()), Ok(()));
    }

    #[test]
    fn a_server_that_does_not_exit_at_eof_is_killed_and_reported() {
        let mut launch = peer("sleep=30000");
        launch.exit_grace = Duration::from_millis(300);
        let error = with_server(&launch, &mut nothing).unwrap_err();
        assert_eq!(error, "the server did not exit within 0.3 s of the end of its input");
    }

    #[test]
    fn a_server_that_exits_with_an_error_code_is_reported() {
        let error = with_server(&peer("fail"), &mut nothing).unwrap_err();
        assert!(
            error.starts_with("the server exited with exit status: 3 after the end of its input"),
            "{error}"
        );
    }

    #[test]
    fn a_failing_session_kills_the_server_and_keeps_its_own_error() {
        let error = with_server(&peer("sleep=30000"), &mut |_| Err("session failed".to_owned()))
            .unwrap_err();
        assert_eq!(error, "session failed");
    }

    #[test]
    fn a_missing_program_is_reported() {
        let missing = Launch::serve(PathBuf::from("/nonexistent/mcp-legifrance"));
        let error = with_server(&missing, &mut nothing).unwrap_err();
        assert!(error.starts_with("cannot start /nonexistent/mcp-legifrance: "), "{error}");
    }

    #[test]
    fn a_closed_stdin_and_a_closed_output_are_reported() {
        let (sender, lines) = mpsc::channel();
        let mut closed = ProcessTransport {
            stdin: None,
            lines,
            read_timeout: Duration::from_secs(1),
            ignored: 0,
        };
        assert_eq!(closed.send(&json!({})).unwrap_err(), "the server's stdin is closed");
        drop(sender);
        assert_eq!(closed.receive().unwrap_err(), "the server closed its output");
    }

    #[test]
    fn writing_to_a_server_that_already_left_is_reported() {
        // Le pair `cargo` quitte sans rien lire : une écriture finit par échouer (tube cassé).
        let error = with_server(&peer("cargo"), &mut |transport| loop {
            transport.send(&json!({"jsonrpc": "2.0", "method": "x"}))?;
            thread::sleep(Duration::from_millis(50));
        })
        .unwrap_err();
        assert!(error.starts_with("cannot write to the server: "), "{error}");
    }

    #[test]
    fn run_capture_returns_stdout_and_reports_failures() {
        let exe = env::current_exe().unwrap();
        let stdout = run_capture(exe.as_os_str(), &peer_args("cargo"), true).unwrap();
        assert_eq!(built_executable(&stdout, "mcp-legifrance").unwrap(), exe);
        let flag = [OsString::from("--no-such-flag")];
        let error = run_capture(exe.as_os_str(), &flag, false).unwrap_err();
        assert!(error.contains("--no-such-flag failed ("), "{error}");
        let error = run_capture(OsStr::new("/nonexistent/tool"), &[], false).unwrap_err();
        assert!(error.starts_with("cannot run /nonexistent/tool: "), "{error}");
    }

    #[test]
    fn built_executable_reads_the_cargo_artifact_of_the_binary_target() {
        let artifact = |name: &str, kind: &str, executable: Value| {
            json!({"reason": "compiler-artifact", "target": {"name": name, "kind": [kind]}, "executable": executable}).to_string()
        };
        let output = [
            "not json".to_owned(),
            json!({"reason": "build-finished", "success": true}).to_string(),
            artifact("mcp-legifrance", "lib", json!("/t/libmcp.rlib")),
            artifact("mcp-legifrance", "bin", json!(null)),
            artifact("other", "bin", json!("/t/other")),
            artifact("mcp-legifrance", "bin", json!("/t/first")),
            artifact("mcp-legifrance", "bin", json!("/t/last")),
        ]
        .join("\n");
        assert_eq!(built_executable(&output, "mcp-legifrance").unwrap(), PathBuf::from("/t/last"));
        assert_eq!(
            built_executable("", "mcp-legifrance").unwrap_err(),
            "cargo did not report an executable for `mcp-legifrance`"
        );
        assert!(
            built_executable(&artifact("mcp-legifrance", "lib", json!("/x")), "mcp-legifrance")
                .is_err()
        );
    }
}
