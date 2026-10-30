//! `cargo xtask instructions-size` : taille des instructions du serveur (05 §4.3 règle 2, budget
//! D-33).
//!
//! xtask ne dépend d'aucun crate `lf-*` : la commande pilote le binaire `mcp-legifrance serve` en
//! boîte noire, sur stdio, dans l'ère moderne et sans état. Elle lui envoie `server/discover`
//! (les instructions) puis `tools/list` (les descripteurs), attend chaque réponse avant d'envoyer
//! la suivante, ferme stdin, puis :
//!
//! - échoue si les instructions dépassent 3 500 caractères Unicode ;
//! - affiche le compte, la marge, la plus longue ligne, et la taille de la liste des outils.
//!
//! Aucun réseau, aucun identifiant : le binaire n'appelle PISTE qu'à l'exécution d'un outil.
//! Le binaire doit exister : `launch` le construit puis le cherche à côté de xtask.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde_json::{Value, json};

/// Plafond des instructions, en caractères Unicode (00 §6).
pub const BUDGET_CHARS: usize = 3_500;

/// Attente maximale d'une réponse du serveur.
#[cfg(not(test))]
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// Ce que la commande mesure : les instructions de `server/discover` et la liste de `tools/list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measure {
    /// Instructions, en caractères Unicode.
    pub chars: usize,
    /// Instructions, en octets UTF-8.
    pub bytes: usize,
    /// Plus longue ligne des instructions, en caractères.
    pub longest_line: usize,
    /// Nombre d'outils de `tools/list`.
    pub tools: usize,
    /// Tableau `tools` de `tools/list`, en octets de JSON compact.
    pub descriptor_bytes: usize,
}

impl Measure {
    /// Caractères restant sous le plafond (0 s'il est dépassé).
    #[must_use]
    pub const fn margin(&self) -> usize {
        BUDGET_CHARS.saturating_sub(self.chars)
    }
}

/// Mesure les instructions du serveur et échoue au-delà de 3 500 caractères.
///
/// # Errors
///
/// Le message de l'échec : binaire introuvable ou muet, réponse illisible, ou instructions au-delà
/// du plafond.
pub fn run() -> Result<(), String> {
    run_with(&mut std::io::stdout().lock(), &launch)
}

/// Lanceur du serveur : les requêtes en entrée, les réponses (une ligne chacune) en sortie.
type Launcher<'a> = &'a dyn Fn(&[String]) -> Result<Vec<String>, String>;

/// Corps de [`run`], avec la sortie et le lancement du serveur injectés. Non générique : une seule
/// instanciation, donc une couverture par région sans trou propre à un type.
fn run_with(out: &mut dyn Write, launch: Launcher<'_>) -> Result<(), String> {
    let measure = measure(&launch(&requests())?)?;
    // Une sortie standard fermée ne change pas le verdict, qui est le code de sortie.
    drop(writeln!(out, "{}", report(&measure)));
    verdict(&measure)
}

/// Les deux requêtes, une par ligne, en ère moderne : `server/discover` (id 1) puis `tools/list`
/// (id 2), chacune avec le `_meta` que l'ère exige (05 §2.1).
#[must_use]
pub fn requests() -> Vec<String> {
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "xtask-instructions-size", "version": "0"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    ["server/discover", "tools/list"]
        .into_iter()
        .zip(1_u64..)
        .map(|(method, id)| {
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {"_meta": meta}})
                .to_string()
        })
        .collect()
}

/// Le `result` d'une réponse JSON-RPC d'identifiant `id`.
#[expect(
    clippy::indexing_slicing,
    reason = "indexation de `serde_json::Value` : un champ absent donne `Null`, sans panique"
)]
fn result_of(line: &str, id: u64) -> Result<Value, String> {
    let mut response: Value =
        serde_json::from_str(line).map_err(|e| format!("response {id} is not JSON: {e}"))?;
    if response["id"] != json!(id) {
        return Err(format!("response {id} carries another id"));
    }
    if let Some(error) = response.get("error") {
        return Err(format!("the server answered request {id} with an error: {error}"));
    }
    Ok(response["result"].take())
}

/// Mesure les deux réponses du serveur (`server/discover`, puis `tools/list`).
///
/// # Errors
///
/// Une réponse manquante, illisible ou en erreur, sans `instructions` ou sans `tools`.
#[expect(
    clippy::indexing_slicing,
    reason = "indexation de `serde_json::Value` : un champ absent donne `Null`, sans panique"
)]
pub fn measure(responses: &[String]) -> Result<Measure, String> {
    let [discovered, listed] = responses else {
        return Err(format!("expected 2 responses, got {}", responses.len()));
    };
    let discovered = result_of(discovered, 1)?;
    let instructions =
        discovered["instructions"].as_str().ok_or("server/discover has no instructions")?;
    let listed = result_of(listed, 2)?;
    let tools = listed["tools"].as_array().ok_or("tools/list has no tools array")?;
    Ok(Measure {
        chars: instructions.chars().count(),
        bytes: instructions.len(),
        longest_line: instructions.lines().map(|line| line.chars().count()).max().unwrap_or(0),
        tools: tools.len(),
        descriptor_bytes: Value::Array(tools.clone()).to_string().len(),
    })
}

/// Les lignes que la commande affiche.
#[must_use]
pub fn report(measure: &Measure) -> String {
    format!(
        "instructions: {} characters ({} bytes), budget {BUDGET_CHARS}, margin {}, longest line {} \
         characters\ndescriptors: {} tools, {} bytes of JSON in tools/list",
        measure.chars,
        measure.bytes,
        measure.margin(),
        measure.longest_line,
        measure.tools,
        measure.descriptor_bytes,
    )
}

/// Échoue si les instructions dépassent le plafond.
///
/// # Errors
///
/// Le dépassement, en caractères.
pub fn verdict(measure: &Measure) -> Result<(), String> {
    if measure.chars > BUDGET_CHARS {
        return Err(format!(
            "instructions are {} characters, {} over the budget of {BUDGET_CHARS}",
            measure.chars,
            measure.chars - BUDGET_CHARS
        ));
    }
    Ok(())
}

/// Les tuyaux du processus : stdin à écrire, stdout à lire.
fn pipes(child: &mut Child) -> Result<(ChildStdin, ChildStdout), String> {
    match (child.stdin.take(), child.stdout.take()) {
        (Some(stdin), Some(stdout)) => Ok((stdin, stdout)),
        _ => Err("the server has no piped stdin and stdout".to_owned()),
    }
}

/// Lance `command` avec stdin et stdout en tuyaux, envoie chaque ligne de `requests` en attendant
/// sa réponse (au plus `timeout`), puis ferme stdin et arrête le processus. Rend les réponses.
///
/// Une requête n'est envoyée qu'après la réponse à la précédente : à l'EOF de stdin, le serveur
/// annule ses appels en vol (cycle de vie, 05 §2.1), donc une requête encore non traitée
/// n'aurait jamais de réponse.
fn drive(
    mut command: Command,
    requests: &[String],
    timeout: Duration,
) -> Result<Vec<String>, String> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|e| format!("cannot start the server: {e}"))?;
    let conversation = converse(&mut child, requests, timeout);
    // Dans tous les cas le serveur ne survit pas à la commande ; il peut déjà être sorti.
    drop(child.kill());
    drop(child.wait());
    conversation
}

/// L'échange de [`drive`], sur un processus déjà lancé.
fn converse(
    child: &mut Child,
    requests: &[String],
    timeout: Duration,
) -> Result<Vec<String>, String> {
    let (mut stdin, stdout) = pipes(child)?;
    // Un fil lit les lignes : `std` n'offre pas de lecture de tuyau avec délai. La lecture
    // s'arrête à la fin du flux (le processus est tué par `drive`) ou à la première ligne
    // illisible ; une réponse que plus personne n'attend est ignorée.
    let (lines, answers) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            drop(lines.send(line));
        }
    });
    let mut responses = Vec::with_capacity(requests.len());
    for request in requests {
        writeln!(stdin, "{request}")
            .and_then(|()| stdin.flush())
            .map_err(|e| format!("cannot write to the server: {e}"))?;
        match answers.recv_timeout(timeout) {
            Ok(line) => responses.push(line),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(format!("no answer from the server within {timeout:?}"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("the server closed its output before answering".to_owned());
            }
        }
    }
    Ok(responses)
}

/// Construit le binaire `mcp-legifrance`, puis l'interroge sur stdio, environnement vidé.
#[cfg(not(test))]
fn launch(requests: &[String]) -> Result<Vec<String>, String> {
    let built = Command::new("cargo")
        .args(["build", "--quiet", "--locked", "--package", "mcp-legifrance"])
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !built.success() {
        return Err("cargo build --package mcp-legifrance failed".to_owned());
    }
    // `cargo xtask` lance `<cible>/<profil>/xtask` : le serveur est construit dans le même dossier.
    let xtask = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    let server = xtask.with_file_name(format!("mcp-legifrance{}", std::env::consts::EXE_SUFFIX));
    let mut command = Command::new(server);
    command.arg("serve").env_clear();
    drive(command, requests, ANSWER_TIMEOUT)
}

/// Remplaçant de test : deux réponses fixes, sans construire ni lancer le binaire.
#[cfg(test)]
#[expect(clippy::unnecessary_wraps, reason = "même signature que le lanceur réel")]
fn launch(requests: &[String]) -> Result<Vec<String>, String> {
    assert_eq!(requests.len(), 2);
    Ok(tests::canned(&"x".repeat(100), 3))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// Les deux réponses d'un serveur dont les instructions valent `instructions`, avec
    /// `tools` outils.
    pub(super) fn canned(instructions: &str, tools: usize) -> Vec<String> {
        let tools: Vec<Value> = (0..tools).map(|n| json!({"name": format!("fr_{n}")})).collect();
        vec![
            json!({"jsonrpc": "2.0", "id": 1, "result": {"instructions": instructions}})
                .to_string(),
            json!({"jsonrpc": "2.0", "id": 2, "result": {"tools": tools}}).to_string(),
        ]
    }

    #[test]
    fn the_requests_are_modern_discover_then_tools_list() {
        let requests = requests();
        assert_eq!(requests.len(), 2);
        let parsed: Vec<Value> =
            requests.iter().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(parsed[0]["method"], "server/discover");
        assert_eq!(parsed[0]["id"], 1);
        assert_eq!(parsed[1]["method"], "tools/list");
        assert_eq!(parsed[1]["id"], 2);
        for request in &parsed {
            assert_eq!(request["jsonrpc"], "2.0");
            let meta = &request["params"]["_meta"];
            assert_eq!(meta["io.modelcontextprotocol/protocolVersion"], "2026-07-28");
            assert!(meta["io.modelcontextprotocol/clientCapabilities"].is_object());
            assert!(meta["io.modelcontextprotocol/clientInfo"]["name"].is_string());
        }
        // Une requête par ligne : aucun saut de ligne dans le JSON.
        assert!(requests.iter().all(|line| !line.contains('\n')));
    }

    #[test]
    fn the_measure_counts_characters_not_bytes() {
        // « é » et « — » pèsent 2 et 3 octets : 5 caractères, 8 octets (le second « \n » sépare).
        let measured = measure(&canned("é—a\nbb", 2)).unwrap();
        assert_eq!(measured.chars, 6);
        assert_eq!(measured.bytes, 9);
        assert_eq!(measured.longest_line, 3);
        assert_eq!(measured.tools, 2);
        assert_eq!(measured.descriptor_bytes, r#"[{"name":"fr_0"},{"name":"fr_1"}]"#.len());
        assert_eq!(measured.margin(), 3_494);
    }

    #[test]
    fn empty_instructions_have_no_line_and_no_margin_loss() {
        let measured = measure(&canned("", 0)).unwrap();
        assert_eq!((measured.chars, measured.longest_line, measured.tools), (0, 0, 0));
        assert_eq!(measured.descriptor_bytes, 2);
        assert_eq!(measured.margin(), BUDGET_CHARS);
    }

    #[test]
    fn the_margin_is_zero_over_the_budget() {
        let measured = measure(&canned(&"a".repeat(BUDGET_CHARS + 5), 1)).unwrap();
        assert_eq!(measured.margin(), 0);
    }

    #[test]
    fn the_verdict_fails_only_above_the_budget() {
        let at_budget = measure(&canned(&"a".repeat(BUDGET_CHARS), 1)).unwrap();
        assert_eq!(verdict(&at_budget), Ok(()));
        let over = measure(&canned(&"a".repeat(BUDGET_CHARS + 1), 1)).unwrap();
        assert_eq!(
            verdict(&over).unwrap_err(),
            "instructions are 3501 characters, 1 over the budget of 3500"
        );
    }

    #[test]
    fn the_report_gives_count_margin_longest_line_and_descriptor_size() {
        let measured = measure(&canned("ab\ncde", 1)).unwrap();
        assert_eq!(
            report(&measured),
            "instructions: 6 characters (6 bytes), budget 3500, margin 3494, longest line 3 \
             characters\ndescriptors: 1 tools, 17 bytes of JSON in tools/list"
        );
    }

    #[test]
    fn malformed_answers_are_refused_with_a_reason() {
        let ok = canned("text", 1);
        assert_eq!(measure(&ok[..1]).unwrap_err(), "expected 2 responses, got 1");
        assert!(
            measure(&["no".into(), ok[1].clone()])
                .unwrap_err()
                .starts_with("response 1 is not JSON")
        );
        assert!(
            measure(&[ok[0].clone(), "{".into()])
                .unwrap_err()
                .starts_with("response 2 is not JSON")
        );
        // Les deux réponses dans le mauvais ordre : l'identifiant ne correspond pas.
        assert_eq!(
            measure(&[ok[1].clone(), ok[0].clone()]).unwrap_err(),
            "response 1 carries another id"
        );
        let error = json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32602, "message": "bad"}});
        assert!(
            measure(&[error.to_string(), ok[1].clone()])
                .unwrap_err()
                .starts_with("the server answered request 1 with an error: ")
        );
        let bare = json!({"jsonrpc": "2.0", "id": 1, "result": {}}).to_string();
        assert_eq!(
            measure(&[bare, ok[1].clone()]).unwrap_err(),
            "server/discover has no instructions"
        );
        let no_tools = json!({"jsonrpc": "2.0", "id": 2, "result": {}}).to_string();
        assert_eq!(
            measure(&[ok[0].clone(), no_tools]).unwrap_err(),
            "tools/list has no tools array"
        );
    }

    #[test]
    fn run_prints_the_report_and_passes_under_the_budget() {
        let mut out = Vec::new();
        run_with(&mut out, &|requests| {
            assert_eq!(requests, requests_for_test());
            Ok(canned(&"a".repeat(2_879), 8))
        })
        .unwrap();
        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed
                .starts_with("instructions: 2879 characters (2879 bytes), budget 3500, margin 621")
        );
        assert!(
            printed.ends_with("descriptors: 8 tools, 129 bytes of JSON in tools/list\n"),
            "{printed}"
        );
    }

    fn requests_for_test() -> Vec<String> {
        requests()
    }

    #[test]
    fn run_prints_then_fails_over_the_budget() {
        let mut out = Vec::new();
        let error = run_with(&mut out, &|_| Ok(canned(&"a".repeat(3_600), 1))).unwrap_err();
        assert_eq!(error, "instructions are 3600 characters, 100 over the budget of 3500");
        assert!(String::from_utf8(out).unwrap().contains("margin 0"));
    }

    #[test]
    fn run_passes_an_unreadable_answer_through_without_printing() {
        let mut out = Vec::new();
        let error =
            run_with(&mut out, &|_| Ok(vec!["not json".to_owned(), String::new()])).unwrap_err();
        assert!(error.starts_with("response 1 is not JSON"), "{error}");
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn run_passes_a_launch_failure_through_without_printing() {
        let mut out = Vec::new();
        let error = run_with(&mut out, &|_| Err("no binary".to_owned())).unwrap_err();
        assert_eq!(error, "no binary");
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn a_closed_standard_output_does_not_change_the_verdict() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut closed = Closed;
        closed.flush().unwrap();
        assert_eq!(run_with(&mut closed, &|_| Ok(canned("short", 1))), Ok(()));
    }

    #[test]
    fn the_public_run_uses_the_launcher() {
        // Sous test, le lanceur est un remplaçant (deux réponses fixes, instructions de 100
        // caractères) : `run` va au bout sans construire ni lancer le binaire.
        assert_eq!(run(), Ok(()));
    }

    // ---- le pilote de processus, sur des commandes `sh` --------------------------------------

    #[cfg(unix)]
    mod driving {
        use pretty_assertions::assert_eq;

        use super::*;

        fn sh(script: &str) -> Command {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        }

        fn lines(requests: &[&str]) -> Vec<String> {
            requests.iter().map(|&line| line.to_owned()).collect()
        }

        const SECOND: Duration = Duration::from_secs(1);

        #[test]
        fn each_request_gets_its_answer_in_order() {
            let command = sh(r#"while read -r line; do echo "answer to $line"; done"#);
            let answers = drive(command, &lines(&["one", "two"]), SECOND).unwrap();
            assert_eq!(answers, ["answer to one", "answer to two"]);
        }

        #[test]
        fn a_request_is_sent_only_after_the_previous_answer() {
            // Après « one », le script attend 0,3 s une ligne qui ne doit pas venir : si le
            // pilote envoyait « two » sans attendre la réponse, la première réponse serait « early ».
            let script = "read -r one; if read -r -t 0.3 early; then echo early; else echo first; \
                          fi; read -r two; echo second";
            let mut command = Command::new("bash");
            command.args(["-c", script]);
            let answers = drive(command, &lines(&["one", "two"]), SECOND).unwrap();
            assert_eq!(answers, ["first", "second"]);
        }

        #[test]
        fn a_server_that_exits_after_reading_without_answering_is_an_error() {
            let error = drive(sh("read -r line"), &lines(&["one"]), SECOND).unwrap_err();
            assert_eq!(error, "the server closed its output before answering");
        }

        #[test]
        fn a_server_that_is_gone_before_reading_makes_the_write_fail() {
            // 1 Mio dépasse le tampon du tuyau : l'écriture ne peut finir que si le serveur lit, et
            // échoue (tuyau fermé) dès qu'il est sorti, sans dépendre d'une course.
            let huge = "x".repeat(1 << 20);
            let error = drive(sh("exit 0"), &[huge], SECOND).unwrap_err();
            assert!(error.starts_with("cannot write to the server: "), "{error}");
        }

        #[test]
        fn a_silent_server_times_out_and_is_stopped() {
            let started = std::time::Instant::now();
            let error =
                drive(sh("read -r line; sleep 30"), &lines(&["one"]), Duration::from_millis(200))
                    .unwrap_err();
            assert_eq!(error, "no answer from the server within 200ms");
            assert!(started.elapsed() < Duration::from_secs(10));
        }

        #[test]
        fn a_missing_program_is_reported() {
            let error =
                drive(Command::new("/nonexistent/server"), &lines(&["one"]), SECOND).unwrap_err();
            assert!(error.starts_with("cannot start the server: "), "{error}");
        }

        #[test]
        fn a_process_without_pipes_is_refused() {
            let mut child = Command::new("true").spawn().unwrap();
            let error = converse(&mut child, &[], SECOND).unwrap_err();
            assert_eq!(error, "the server has no piped stdin and stdout");
            child.wait().unwrap();
        }

        #[test]
        fn no_request_means_no_answer_and_no_wait() {
            let answers = drive(sh("sleep 30"), &[], SECOND).unwrap();
            assert_eq!(answers.len(), 0);
        }
    }
}
