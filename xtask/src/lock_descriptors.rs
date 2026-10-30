//! `cargo xtask lock-descriptors` : verrou des descripteurs de la surface MCP (05 §9, 02 §3.8).
//!
//! La commande construit le binaire `mcp-legifrance`, le lance trois fois en stdio avec un
//! environnement vidé (ère moderne, `initialize` 2025-11-25 et 2025-06-18), vérifie que les trois
//! surfaces sont identiques, applique les contrôles de 05 §9.2 point 8, puis :
//!
//! - `--check` (défaut) compare la surface régénérée à `tests/tools.lock.json`, au manifeste MCPB
//!   et à la version de `server.json` ;
//! - `--update` réécrit ces trois fichiers ;
//! - `--compare <réf-git>` classe les différences avec le verrou de la référence (05 §9.5) et
//!   vérifie que la version du crate a assez avancé.
//!
//! Modules : `session` (pilote JSON-RPC), `process` (processus fils), `surface` (normalisation et
//! égalité des ères), `checks` (contrôles), `lock` (fichier de verrou), `diff` (rapport et classe
//! `SemVer`), `packaging` (manifeste MCPB, `server.json`), `json` et `sha256` (forme canonique et
//! empreintes). Tout est testé sur du JSON synthétique ; seul `RealHost` touche au système.

mod checks;
mod diff;
#[cfg(test)]
mod fixtures;
mod json;
mod lock;
mod packaging;
mod process;
mod session;
mod sha256;
mod surface;

use std::{
    ffi::OsString,
    fs,
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
};

use serde_json::Value;

use self::{
    packaging::{MANIFEST_PATH, SERVER_JSON_PATH},
    process::Launch,
    session::{SESSIONS, run_session},
    surface::{Capture, Surface},
};

/// Verrou des descripteurs, relatif à la racine du dépôt (05 §9.3).
const LOCK_PATH: &str = "tests/tools.lock.json";

/// Arguments de `cargo` qui construisent le binaire et annoncent son chemin en JSON.
const BUILD_ARGS: [&str; 7] = [
    "build",
    "--locked",
    "--package",
    "mcp-legifrance",
    "--bin",
    "mcp-legifrance",
    "--message-format=json-render-diagnostics",
];

/// Mode du verrou, choisi sur la ligne de commande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockMode {
    /// Régénère en mémoire et compare à `tests/tools.lock.json` (défaut, utilisé par la CI).
    Check,
    /// Réécrit `tests/tools.lock.json`.
    Update,
    /// Classe les différences avec le verrou de la référence git donnée (release).
    Compare(String),
}

impl LockMode {
    /// Lit les arguments qui suivent `lock-descriptors` : aucun (ou `--check`), `--update`, ou
    /// `--compare <réf-git>`.
    ///
    /// # Errors
    ///
    /// Le message d'usage si un argument est inconnu, si `--compare` n'a pas de référence, ou si
    /// plusieurs modes sont demandés.
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        const USAGE: &str =
            "usage: cargo xtask lock-descriptors [--check | --update | --compare <ref-git>]";
        match args {
            [] => Ok(Self::Check),
            [flag] if flag == "--check" => Ok(Self::Check),
            [flag] if flag == "--update" => Ok(Self::Update),
            [flag, reference] if flag == "--compare" && !reference.starts_with('-') => {
                Ok(Self::Compare(reference.clone()))
            }
            _ => Err(USAGE.to_owned()),
        }
    }
}

/// Le monde extérieur de la commande : serveur, fichiers, git et sorties. Les tests le remplacent
/// par un faux ; [`RealHost`] est la seule implémentation qui touche au système.
trait Host {
    /// Construit le binaire et joue les trois sessions.
    fn capture(&mut self) -> Result<Vec<Capture>, String>;
    /// Contenu d'un fichier du dépôt, `None` s'il n'existe pas.
    fn read(&mut self, path: &str) -> Result<Option<String>, String>;
    /// Écrit un fichier du dépôt (dossiers parents créés).
    fn write(&mut self, path: &str, text: &str) -> Result<(), String>;
    /// Contenu de `path` à la référence git `reference`.
    fn git_show(&mut self, reference: &str, path: &str) -> Result<String, String>;
    /// Une ligne sur stdout.
    fn say(&mut self, line: &str);
    /// Une ligne sur stderr.
    fn warn(&mut self, line: &str);
}

/// Exécute le verrou dans le `mode` donné.
///
/// # Errors
///
/// Le message de l'échec : binaire introuvable ou muet, ères différentes, contrôle en échec,
/// fichier absent ou périmé, saut de version insuffisant.
#[expect(
    clippy::needless_pass_by_value,
    reason = "signature gelée : xtask/src/main.rs la passe à `and_then`"
)]
pub fn run(mode: LockMode) -> Result<(), String> {
    execute(&mode, &mut RealHost::new())
}

/// Version de `[workspace.package]`, celle du crate (05 §9.4 : verrou, crate, `serverInfo` et
/// `server.json` ont la même).
fn workspace_version(cargo_toml: &str) -> Result<String, String> {
    let table =
        cargo_toml.parse::<toml::Table>().map_err(|error| format!("Cargo.toml: {error}"))?;
    table
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml: [workspace.package] version is missing".to_owned())
}

fn execute(mode: &LockMode, host: &mut dyn Host) -> Result<(), String> {
    let cargo_toml = host.read("Cargo.toml")?.ok_or("Cargo.toml is missing")?;
    let version = workspace_version(&cargo_toml)?;
    match mode {
        LockMode::Check => check(host, &version),
        LockMode::Update => update(host, &version),
        LockMode::Compare(reference) => compare(host, reference, &version),
    }
}

/// Surface observée, contrôlée, et le verrou qui en découle (valeur et texte du fichier).
fn regenerate(host: &mut dyn Host, version: &str) -> Result<(Value, String), String> {
    let captures = host.capture()?;
    let surface = Surface::from_captures(&captures)?;
    // Construire le verrou d'abord : sans instructions ni flottants, les contrôles ont un sens.
    let lock = lock::build(&surface)?;
    let findings = checks::run(&surface, version);
    for warning in &findings.warnings {
        host.warn(&format!("warning: {warning}"));
    }
    if !findings.errors.is_empty() {
        return Err(format!("descriptor controls failed:\n- {}", findings.errors.join("\n- ")));
    }
    let text = json::pretty(&lock);
    Ok((lock, text))
}

type Generator = fn(&Value, &Value) -> Result<Value, String>;

/// Fichiers dérivés du verrou : manifeste MCPB et `server.json`.
const GENERATED: [(&str, Generator); 2] =
    [(MANIFEST_PATH, packaging::manifest), (SERVER_JSON_PATH, packaging::server_json)];

fn read_json(host: &mut dyn Host, path: &str) -> Result<Value, String> {
    let text = host.read(path)?.ok_or_else(|| format!("{path} is missing"))?;
    serde_json::from_str(&text).map_err(|error| format!("{path} is not valid JSON: {error}"))
}

fn update(host: &mut dyn Host, version: &str) -> Result<(), String> {
    let (lock, text) = regenerate(host, version)?;
    // Lire les fichiers dérivés avant toute écriture : un manifeste absent n'écrit rien.
    let mut pending = Vec::new();
    for (path, generate) in GENERATED {
        let current = read_json(host, path)?;
        pending.push((path, generate(&current, &lock)?, current));
    }
    write_if_changed(host, LOCK_PATH, &text)?;
    for (path, wanted, current) in pending {
        if wanted == current {
            host.say(&format!("{path}: unchanged"));
        } else {
            host.write(path, &json::pretty(&wanted))?;
            host.say(&format!("{path}: updated"));
        }
    }
    Ok(())
}

fn write_if_changed(host: &mut dyn Host, path: &str, text: &str) -> Result<(), String> {
    if host.read(path)?.as_deref() == Some(text) {
        host.say(&format!("{path}: unchanged"));
        return Ok(());
    }
    host.write(path, text)?;
    host.say(&format!("{path}: updated"));
    Ok(())
}

const UPDATE_HINT: &str = "run `cargo xtask lock-descriptors --update`";

fn check(host: &mut dyn Host, version: &str) -> Result<(), String> {
    let (lock, text) = regenerate(host, version)?;
    let mut problems = Vec::new();
    let file = host.read(LOCK_PATH)?;
    match lock::baseline(file.as_deref(), LOCK_PATH) {
        Err(error) => problems.push(error),
        Ok(old) if old == lock => {
            if file.as_deref() != Some(text.as_str()) {
                problems.push(format!("{LOCK_PATH} is not in canonical form"));
            }
        }
        Ok(old) => {
            problems.push(format!("{LOCK_PATH} differs from the descriptors the binary serves:"));
            problems.extend(diff::report(&old, &lock).into_iter().map(|line| format!("  {line}")));
        }
    }
    for (path, generate) in GENERATED {
        match read_json(host, path).and_then(|current| {
            let wanted = generate(&current, &lock)?;
            Ok((json::changed_paths(&current, &wanted), current == wanted))
        }) {
            Err(error) => problems.push(error),
            Ok((_, true)) => {}
            Ok((paths, false)) => {
                problems.push(format!("{path} is out of date at {}", paths.join(", ")));
            }
        }
    }
    if problems.is_empty() {
        host.say("lock-descriptors: ok");
        Ok(())
    } else {
        Err(format!("{}\n{UPDATE_HINT}", problems.join("\n")))
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "indexation de `serde_json::Value` : un champ absent donne `Null`, sans panique"
)]
fn compare(host: &mut dyn Host, reference: &str, version: &str) -> Result<(), String> {
    let old_text = host.git_show(reference, LOCK_PATH)?;
    let old = lock::baseline(Some(&old_text), &format!("{LOCK_PATH} at {reference}"))?;
    let new_text = host.read(LOCK_PATH)?;
    let new = lock::baseline(new_text.as_deref(), LOCK_PATH)?;
    if new["server_version"] != version {
        return Err(format!(
            "{LOCK_PATH} is for version {}, the crate is {version}: {UPDATE_HINT}",
            new["server_version"]
        ));
    }
    let changes = diff::classify(&old, &new);
    for change in &changes {
        host.say(&format!("[{}] {}: {}", change.class.label(), change.path, change.what));
    }
    let class = diff::overall(&changes);
    let old_version = old["server_version"].as_str().unwrap_or_default();
    let jump = diff::check_version_jump(old_version, version, class)?;
    let summary = class.map_or("no surface change", diff::Class::label);
    host.say(&format!("{summary}: {old_version} -> {version} ({jump:?} bump) is enough"));
    Ok(())
}

/// Programme `cargo` : celui qui lance xtask (`CARGO`), à défaut `cargo`.
fn cargo_or_default(from_env: Option<OsString>) -> OsString {
    from_env.unwrap_or_else(|| OsString::from("cargo"))
}

#[expect(
    clippy::disallowed_methods,
    reason = "CLI de développement : CARGO est fourni par cargo (exception admise pour xtask)"
)]
fn cargo_program() -> OsString {
    cargo_or_default(std::env::var_os("CARGO"))
}

/// Joue les trois sessions sur `launch`, avec le nombre de lignes non JSON ignorées au total.
fn sessions(launch: &Launch) -> Result<(Vec<Capture>, usize), String> {
    let mut captures = Vec::new();
    let mut ignored = 0;
    for kind in SESSIONS {
        let mut capture = None;
        ignored += process::with_server(launch, &mut |transport| {
            capture = Some(run_session(transport, kind)?);
            Ok(())
        })?;
        captures.extend(capture);
    }
    Ok((captures, ignored))
}

/// Transmet à `warn` l'avertissement des lignes ignorées, s'il y en a.
fn report_ignored(ignored: usize, warn: &mut impl FnMut(&str)) {
    if let Some(warning) = ignored_warning(ignored) {
        warn(&warning);
    }
}

/// Avertissement sur les lignes de la sortie du serveur qui n'étaient pas du JSON-RPC, s'il y en a.
fn ignored_warning(ignored: usize) -> Option<String> {
    (ignored > 0).then(|| {
        format!(
            "warning: {ignored} line(s) of the server's stdout were not JSON-RPC and were ignored"
        )
    })
}

/// Le système réel : racine du dépôt, processus, fichiers, git et consoles.
struct RealHost {
    root: PathBuf,
    /// Programme qui construit le binaire, avec ses arguments.
    cargo: OsString,
    build_args: Vec<OsString>,
    /// Lancement du serveur ; `program` est remplacé par l'exécutable que la construction annonce.
    server: Launch,
    /// Console : les verdicts vont sur la sortie standard, les avertissements sur l'erreur
    /// standard. Des écrivains, pour que les tests lisent ce qui est dit.
    stdout: Box<dyn Write>,
    stderr: Box<dyn Write>,
}

impl RealHost {
    /// Racine du dépôt : le parent du dossier de xtask ; `cargo build` du binaire ; serveur lancé
    /// avec `serve` et un environnement vide.
    fn new() -> Self {
        Self {
            root: Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
            cargo: cargo_program(),
            build_args: BUILD_ARGS.map(OsString::from).to_vec(),
            server: Launch::serve(PathBuf::new()),
            stdout: Box::new(io::stdout()),
            stderr: Box::new(io::stderr()),
        }
    }
}

impl Host for RealHost {
    fn capture(&mut self) -> Result<Vec<Capture>, String> {
        let output = process::run_capture(&self.cargo, &self.build_args, true)?;
        let mut server = self.server.clone();
        server.program = process::built_executable(&output, "mcp-legifrance")?;
        let (captures, ignored) = sessions(&server)?;
        report_ignored(ignored, &mut |line| self.warn(line));
        Ok(captures)
    }

    fn read(&mut self, path: &str) -> Result<Option<String>, String> {
        match fs::read_to_string(self.root.join(path)) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{path}: {error}")),
        }
    }

    fn write(&mut self, path: &str, text: &str) -> Result<(), String> {
        let file = self.root.join(path);
        let folder = file.parent().unwrap_or(&self.root);
        fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        fs::write(&file, text).map_err(|error| format!("{path}: {error}"))
    }

    fn git_show(&mut self, reference: &str, path: &str) -> Result<String, String> {
        process::run_git(&self.root, &["show", &format!("{reference}:{path}")])
    }

    fn say(&mut self, line: &str) {
        // Une console fermée (tube rompu) ne change pas le verdict : l'erreur est ignorée.
        let _ = writeln!(self.stdout, "{line}");
    }

    fn warn(&mut self, line: &str) {
        let _ = writeln!(self.stderr, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        env, process as os,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use super::*;
    use crate::lock_descriptors::fixtures::{VERSION, VERSIONS, capture, peer, peer_args, tool};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn parse(args: &[&str]) -> Result<LockMode, String> {
        LockMode::from_args(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn check_is_the_default_mode() {
        assert_eq!(parse(&[]), Ok(LockMode::Check));
        assert_eq!(parse(&["--check"]), Ok(LockMode::Check));
    }

    #[test]
    fn update_and_compare_are_explicit() {
        assert_eq!(parse(&["--update"]), Ok(LockMode::Update));
        assert_eq!(parse(&["--compare", "v0.1.0"]), Ok(LockMode::Compare("v0.1.0".to_owned())));
    }

    #[test]
    fn unknown_missing_or_combined_arguments_give_the_usage() {
        for args in [
            &["--bogus"][..],
            &["--compare"],
            &["--compare", "--update"],
            &["--check", "--update"],
            &["--update", "--compare", "v0.1.0"],
            &["v0.1.0"],
        ] {
            let error = parse(args).unwrap_err();
            assert!(error.starts_with("usage: cargo xtask lock-descriptors"), "{args:?} : {error}");
        }
    }

    /// Les trois sessions d'un serveur synthétique qui sert `tools`.
    fn sessions_of(tools: &[Value]) -> Vec<Capture> {
        let mut modern = capture("discover 2026-07-28", tools);
        modern.supported_versions = Some(VERSIONS.map(str::to_owned).to_vec());
        vec![
            modern,
            capture("initialize 2025-11-25", tools),
            capture("initialize 2025-06-18", tools),
        ]
    }

    /// Hôte en mémoire : fichiers, références git, sessions préparées, sorties enregistrées.
    struct FakeHost {
        files: BTreeMap<String, String>,
        refs: BTreeMap<String, String>,
        captures: Result<Vec<Capture>, String>,
        said: Vec<String>,
        warned: Vec<String>,
        written: Vec<String>,
        /// Chemins dont la lecture échoue (erreur d'entrée-sortie).
        unreadable: Vec<&'static str>,
        /// Chemins dont l'écriture échoue.
        unwritable: Vec<&'static str>,
    }

    impl FakeHost {
        fn new(tools: &[Value]) -> Self {
            let files = [
                ("Cargo.toml", format!("[workspace.package]\nversion = \"{VERSION}\"\n")),
                (
                    MANIFEST_PATH,
                    json!({"name": "mcp-legifrance", "version": "0.0.0", "tools": [{"name": "stale", "description": "x"}]})
                        .to_string(),
                ),
                (
                    SERVER_JSON_PATH,
                    json!({"name": "io.github.muchiny/mcp-legifrance", "version": "0.0.0",
                           "packages": [{"registryType": "mcpb", "version": "0.0.0"}]})
                    .to_string(),
                ),
            ];
            Self {
                files: files.into_iter().map(|(path, text)| (path.to_owned(), text)).collect(),
                refs: BTreeMap::new(),
                captures: Ok(sessions_of(tools)),
                said: Vec::new(),
                warned: Vec::new(),
                written: Vec::new(),
                unreadable: Vec::new(),
                unwritable: Vec::new(),
            }
        }

        fn standard() -> Self {
            Self::new(&[tool("fr_search"), tool("fr_coverage")])
        }

        fn file(&self, path: &str) -> Value {
            serde_json::from_str(&self.files[path]).unwrap()
        }
    }

    impl Host for FakeHost {
        fn capture(&mut self) -> Result<Vec<Capture>, String> {
            self.captures.clone()
        }

        fn read(&mut self, path: &str) -> Result<Option<String>, String> {
            if self.unreadable.contains(&path) {
                return Err(format!("{path}: input/output error"));
            }
            Ok(self.files.get(path).cloned())
        }

        fn write(&mut self, path: &str, text: &str) -> Result<(), String> {
            if self.unwritable.contains(&path) {
                return Err(format!("{path}: read-only file system"));
            }
            self.written.push(path.to_owned());
            self.files.insert(path.to_owned(), text.to_owned());
            Ok(())
        }

        fn git_show(&mut self, reference: &str, path: &str) -> Result<String, String> {
            self.refs
                .get(&format!("{reference}:{path}"))
                .cloned()
                .ok_or_else(|| format!("git: unknown revision {reference}"))
        }

        fn say(&mut self, line: &str) {
            self.said.push(line.to_owned());
        }

        fn warn(&mut self, line: &str) {
            self.warned.push(line.to_owned());
        }
    }

    fn run_mode(mode: &LockMode, host: &mut FakeHost) -> Result<(), String> {
        execute(mode, host)
    }

    #[test]
    fn lock_generates_mcpb_manifest_and_server_json_version() {
        let mut host = FakeHost::standard();
        run_mode(&LockMode::Update, &mut host).unwrap();
        assert_eq!(host.written, [LOCK_PATH, MANIFEST_PATH, SERVER_JSON_PATH]);
        let manifest = host.file(MANIFEST_PATH);
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["name"], "mcp-legifrance");
        let names: Vec<&str> = manifest["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(names, ["fr_coverage", "fr_search"]);
        assert_eq!(manifest["tools"][1]["description"], tool("fr_search")["description"]);
        let server_json = host.file(SERVER_JSON_PATH);
        assert_eq!(server_json["version"], "0.1.0");
        assert_eq!(server_json["packages"][0]["version"], "0.1.0");
        assert_eq!(
            host.said,
            [
                "tests/tools.lock.json: updated",
                "packaging/mcpb/manifest.json: updated",
                "server.json: updated",
            ]
        );

        // `--check` accepte ce que `--update` a écrit, sans rien écrire.
        host.written.clear();
        run_mode(&LockMode::Check, &mut host).unwrap();
        assert_eq!(host.said.last().map(String::as_str), Some("lock-descriptors: ok"));
        assert_eq!(host.written, Vec::<String>::new());

        // Une version de `server.json` ou du manifeste qui diverge, ou un outil qui change, échoue.
        let drifts = [
            (SERVER_JSON_PATH, "/version", json!("0.2.0")),
            (MANIFEST_PATH, "/version", json!("0.2.0")),
            (MANIFEST_PATH, "/tools", json!([])),
        ];
        for (path, pointer, value) in drifts {
            let mut drifted = FakeHost::standard();
            run_mode(&LockMode::Update, &mut drifted).unwrap();
            let mut content = drifted.file(path);
            *content.pointer_mut(pointer).unwrap() = value;
            drifted.files.insert(path.to_owned(), content.to_string());
            let error = run_mode(&LockMode::Check, &mut drifted).unwrap_err();
            assert_eq!(
                error,
                format!("{path} is out of date at {pointer}\n{UPDATE_HINT}"),
                "{path}"
            );
        }
    }

    #[test]
    fn update_leaves_matching_files_untouched_and_keeps_the_manifest_key_order() {
        let mut host = FakeHost::standard();
        run_mode(&LockMode::Update, &mut host).unwrap();
        // Un manifeste équivalent mais écrit à la main (autre ordre des clés) n'est pas réécrit.
        let tools = host.file(MANIFEST_PATH)["tools"].clone();
        let reordered =
            format!("{{\"version\":\"0.1.0\",\"name\":\"mcp-legifrance\",\"tools\":{tools}}}");
        host.files.insert(MANIFEST_PATH.to_owned(), reordered.clone());
        host.written.clear();
        host.said.clear();
        run_mode(&LockMode::Update, &mut host).unwrap();
        assert_eq!(host.written, Vec::<String>::new());
        assert_eq!(host.files[MANIFEST_PATH], reordered);
        assert_eq!(
            host.said,
            [
                "tests/tools.lock.json: unchanged",
                "packaging/mcpb/manifest.json: unchanged",
                "server.json: unchanged",
            ]
        );
    }

    #[test]
    fn update_writes_nothing_when_a_control_fails_or_a_packaging_file_is_missing() {
        let mut bad = tool("fr_search");
        bad["inputSchema"]["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
        let mut host = FakeHost::new(&[bad]);
        let error = run_mode(&LockMode::Update, &mut host).unwrap_err();
        assert_eq!(
            error,
            "descriptor controls failed:\n- tool `fr_search`: inputSchema must not carry a $schema key (05 §2.5)"
        );
        assert_eq!(host.written, Vec::<String>::new());

        let mut missing = FakeHost::standard();
        missing.files.remove(MANIFEST_PATH);
        assert_eq!(
            run_mode(&LockMode::Update, &mut missing).unwrap_err(),
            "packaging/mcpb/manifest.json is missing"
        );
        assert_eq!(missing.written, Vec::<String>::new());
        let mut garbled = FakeHost::standard();
        garbled.files.insert(SERVER_JSON_PATH.to_owned(), "{".to_owned());
        assert!(
            run_mode(&LockMode::Update, &mut garbled)
                .unwrap_err()
                .starts_with("server.json is not valid JSON: ")
        );
        let mut shapeless = FakeHost::standard();
        shapeless.files.insert(SERVER_JSON_PATH.to_owned(), "[]".to_owned());
        assert_eq!(
            run_mode(&LockMode::Update, &mut shapeless).unwrap_err(),
            "server.json is not a JSON object"
        );
    }

    #[test]
    fn check_reports_the_drift_of_the_lock_with_json_paths() {
        let mut host = FakeHost::standard();
        run_mode(&LockMode::Update, &mut host).unwrap();
        // Le binaire sert maintenant une autre description et un outil de plus.
        let mut reworded = tool("fr_search");
        reworded["description"] = json!("Reworded. Still three sentences. Really.");
        host.captures = Ok(sessions_of(&[reworded, tool("fr_coverage"), tool("fr_extra")]));
        let error = run_mode(&LockMode::Check, &mut host).unwrap_err();
        assert_eq!(
            error,
            "tests/tools.lock.json differs from the descriptors the binary serves:\n  \
             tools added: fr_extra\n  \
             tools changed: fr_search (/descriptor/description, /sha256)\n  \
             other fields changed: /sizes/tools_list_bytes, /surface_sha256, /toolset_sha256\n\
             packaging/mcpb/manifest.json is out of date at /tools\n\
             run `cargo xtask lock-descriptors --update`"
        );
    }

    #[test]
    fn check_refuses_a_missing_empty_or_hand_formatted_lock() {
        let mut host = FakeHost::standard();
        run_mode(&LockMode::Update, &mut host).unwrap();
        let lock_text = host.files[LOCK_PATH].clone();

        host.files.remove(LOCK_PATH);
        let error = run_mode(&LockMode::Check, &mut host).unwrap_err();
        assert!(
            error.starts_with(
                "tests/tools.lock.json is missing: an absent baseline is never accepted\n"
            ),
            "{error}"
        );
        for empty in ["", "{}"] {
            host.files.insert(LOCK_PATH.to_owned(), empty.to_owned());
            let error = run_mode(&LockMode::Check, &mut host).unwrap_err();
            assert!(error.contains("tests/tools.lock.json: lock file"), "{empty:?} : {error}");
        }
        let compact =
            serde_json::to_string(&serde_json::from_str::<Value>(&lock_text).unwrap()).unwrap();
        host.files.insert(LOCK_PATH.to_owned(), compact);
        let error = run_mode(&LockMode::Check, &mut host).unwrap_err();
        assert_eq!(error, format!("tests/tools.lock.json is not in canonical form\n{UPDATE_HINT}"));
    }

    #[test]
    fn controls_warnings_and_failures_of_the_binary_surface_are_surfaced() {
        let mut terse = tool("fr_search");
        terse["description"] = json!("Too short.");
        let mut host = FakeHost::new(&[terse]);
        run_mode(&LockMode::Update, &mut host).unwrap();
        assert_eq!(
            host.warned,
            ["warning: tool `fr_search`: description has 1 sentences, 3 to 6 expected (00 §6)"]
        );
        let mut stale = FakeHost::standard();
        stale.captures = Ok(sessions_of(&[tool("fr_search")])
            .into_iter()
            .map(|mut c| {
                c.server_version = "9.9.9".into();
                c
            })
            .collect());
        let error = run_mode(&LockMode::Check, &mut stale).unwrap_err();
        assert!(
            error.contains("serverInfo.version is `9.9.9`, the crate version is `0.1.0`"),
            "{error}"
        );
        let mut broken = FakeHost::standard();
        broken.captures = Err("cannot start the server".into());
        assert_eq!(run_mode(&LockMode::Check, &mut broken).unwrap_err(), "cannot start the server");
        let mut eras = FakeHost::standard();
        let mut captures = sessions_of(&[tool("fr_search")]);
        captures[2].instructions = Some("other".into());
        eras.captures = Ok(captures);
        assert!(
            run_mode(&LockMode::Check, &mut eras)
                .unwrap_err()
                .starts_with("eras differ: `initialize 2025-06-18`")
        );
    }

    #[test]
    fn the_crate_version_is_read_from_the_workspace_manifest() {
        assert_eq!(
            workspace_version("[workspace.package]\nversion = \"1.2.3\"\n").unwrap(),
            "1.2.3"
        );
        assert_eq!(
            workspace_version("[workspace]\n").unwrap_err(),
            "Cargo.toml: [workspace.package] version is missing"
        );
        assert!(workspace_version("[workspace.package\n").unwrap_err().starts_with("Cargo.toml: "));
        let mut host = FakeHost::standard();
        host.files.remove("Cargo.toml");
        assert_eq!(run_mode(&LockMode::Check, &mut host).unwrap_err(), "Cargo.toml is missing");
    }

    /// Hôte qui a déjà écrit son verrou, avec le verrou d'une version précédente à `v0.1.0`.
    fn compare_host(
        previous: &[Value],
        previous_version: &str,
        current: &[Value],
        version: &str,
    ) -> FakeHost {
        let mut previous_host = FakeHost::new(previous);
        previous_host.files.insert(
            "Cargo.toml".into(),
            format!("[workspace.package]\nversion = \"{previous_version}\"\n"),
        );
        previous_host.captures = Ok(sessions_of(previous)
            .into_iter()
            .map(|mut c| {
                c.server_version = previous_version.into();
                c
            })
            .collect());
        run_mode(&LockMode::Update, &mut previous_host).unwrap();
        let mut host = FakeHost::new(current);
        host.files
            .insert("Cargo.toml".into(), format!("[workspace.package]\nversion = \"{version}\"\n"));
        host.captures = Ok(sessions_of(current)
            .into_iter()
            .map(|mut c| {
                c.server_version = version.into();
                c
            })
            .collect());
        run_mode(&LockMode::Update, &mut host).unwrap();
        host.refs.insert(
            format!("v{previous_version}:{LOCK_PATH}"),
            previous_host.files[LOCK_PATH].clone(),
        );
        // Seules les sorties de `--compare` comptent pour les tests.
        host.said.clear();
        host.written.clear();
        host
    }

    #[test]
    fn compare_classifies_the_changes_and_demands_enough_version() {
        let compare = |previous: &[Value], current: &[Value], version: &str| {
            let mut host = compare_host(previous, "0.1.0", current, version);
            let outcome = run_mode(&LockMode::Compare("v0.1.0".into()), &mut host);
            (outcome, host.said)
        };
        let base = [tool("fr_search")];
        let (outcome, said) = compare(&base, &base, "0.1.1");
        assert_eq!(outcome, Ok(()));
        assert_eq!(said, ["no surface change: 0.1.0 -> 0.1.1 (Patch bump) is enough"]);
        let (outcome, said) = compare(&base, &[tool("fr_search"), tool("fr_coverage")], "0.2.0");
        assert_eq!(outcome, Ok(()));
        assert_eq!(
            said,
            [
                "[addition] /tools/fr_coverage: added",
                "addition: 0.1.0 -> 0.2.0 (Minor bump) is enough"
            ]
        );
        let (outcome, said) = compare(&base, &[tool("fr_search"), tool("fr_coverage")], "0.1.1");
        assert_eq!(
            outcome.unwrap_err(),
            "addition changes need at least a Minor version bump from 0.1.0, but 0.1.1 is a Patch bump"
        );
        assert_eq!(said, ["[addition] /tools/fr_coverage: added"]);
        let (outcome, _) = compare(&[tool("fr_search"), tool("fr_coverage")], &base, "0.2.0");
        assert_eq!(outcome, Ok(()));
        let mut reworded = tool("fr_search");
        reworded["description"] = json!("Reworded. Still three sentences. Really.");
        let (outcome, said) = compare(&base, &[reworded], "0.1.1");
        assert_eq!(outcome, Ok(()));
        assert_eq!(said[0], "[fix] /tools/fr_search/description: text reworded");
        let (outcome, _) = compare(&base, &base, "0.0.9");
        assert_eq!(outcome.unwrap_err(), "version 0.0.9 is lower than the reference version 0.1.0");
    }

    #[test]
    fn compare_refuses_a_missing_reference_an_empty_baseline_or_a_stale_lock() {
        let base = [tool("fr_search")];
        let mut host = compare_host(&base, "0.1.0", &base, "0.1.1");
        assert_eq!(
            run_mode(&LockMode::Compare("v9.9.9".into()), &mut host).unwrap_err(),
            "git: unknown revision v9.9.9"
        );
        host.refs.insert(format!("v0.0.1:{LOCK_PATH}"), "{}".into());
        let error = run_mode(&LockMode::Compare("v0.0.1".into()), &mut host).unwrap_err();
        assert_eq!(
            error,
            "tests/tools.lock.json at v0.0.1: lock file is empty: an empty baseline is never accepted"
        );
        // Le verrou courant est celui d'une autre version que le crate.
        host.files.insert("Cargo.toml".into(), "[workspace.package]\nversion = \"0.1.5\"\n".into());
        let error = run_mode(&LockMode::Compare("v0.1.0".into()), &mut host).unwrap_err();
        assert_eq!(
            error,
            format!(
                "tests/tools.lock.json is for version \"0.1.1\", the crate is 0.1.5: {UPDATE_HINT}"
            )
        );
        host.files.remove(LOCK_PATH);
        let error = run_mode(&LockMode::Compare("v0.1.0".into()), &mut host).unwrap_err();
        assert_eq!(error, "tests/tools.lock.json is missing: an absent baseline is never accepted");
    }

    #[test]
    fn a_surface_that_cannot_be_locked_stops_before_the_controls() {
        let mut mute = FakeHost::standard();
        mute.captures = Ok(sessions_of(&[tool("fr_search")])
            .into_iter()
            .map(|mut session| {
                session.instructions = None;
                session
            })
            .collect());
        assert_eq!(
            run_mode(&LockMode::Check, &mut mute).unwrap_err(),
            "the surface has no instructions"
        );
        let mut floaty = tool("fr_search");
        floaty["inputSchema"]["properties"]["query"]["maxLength"] = json!(2.5);
        let mut host = FakeHost::new(&[floaty]);
        let error = run_mode(&LockMode::Update, &mut host).unwrap_err();
        assert_eq!(
            error,
            "/tools/fr_search/inputSchema/properties/query/maxLength: floating-point number 2.5 (no float is allowed in a descriptor)"
        );
        assert_eq!(host.written, Vec::<String>::new());
    }

    #[test]
    fn input_output_failures_are_reported_by_every_mode() {
        let cases = [
            (LockMode::Check, "Cargo.toml", false, "Cargo.toml: input/output error"),
            (LockMode::Check, LOCK_PATH, false, "tests/tools.lock.json: input/output error"),
            (
                LockMode::Update,
                MANIFEST_PATH,
                false,
                "packaging/mcpb/manifest.json: input/output error",
            ),
            (LockMode::Update, LOCK_PATH, false, "tests/tools.lock.json: input/output error"),
            (LockMode::Update, LOCK_PATH, true, "tests/tools.lock.json: read-only file system"),
            (
                LockMode::Update,
                MANIFEST_PATH,
                true,
                "packaging/mcpb/manifest.json: read-only file system",
            ),
        ];
        for (mode, path, writing, expected) in cases {
            let mut host = FakeHost::standard();
            if writing {
                host.unwritable.push(path);
            } else {
                host.unreadable.push(path);
            }
            assert_eq!(run_mode(&mode, &mut host).unwrap_err(), expected, "{mode:?} {path}");
        }
        // Un manifeste illisible est un problème de plus de `--check`, pas une interruption.
        let mut checked = FakeHost::standard();
        run_mode(&LockMode::Update, &mut checked).unwrap();
        checked.unreadable.push(MANIFEST_PATH);
        assert_eq!(
            run_mode(&LockMode::Check, &mut checked).unwrap_err(),
            format!("packaging/mcpb/manifest.json: input/output error\n{UPDATE_HINT}")
        );
        let base = [tool("fr_search")];
        let mut comparing = compare_host(&base, "0.1.0", &base, "0.1.1");
        comparing.unreadable.push(LOCK_PATH);
        assert_eq!(
            run_mode(&LockMode::Compare("v0.1.0".into()), &mut comparing).unwrap_err(),
            "tests/tools.lock.json: input/output error"
        );
    }

    #[test]
    fn cargo_is_the_program_that_runs_xtask_or_plain_cargo() {
        assert_eq!(cargo_or_default(Some(OsString::from("/opt/cargo"))), "/opt/cargo");
        assert_eq!(cargo_or_default(None), "cargo");
    }

    /// Dossier de travail neuf et propre à ce test.
    fn work_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("lock-descriptors-{}-{name}", os::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Hôte réel dont « cargo » et le serveur sont le binaire de test relancé comme pair.
    fn peer_host(root: &Path, mode: &str) -> RealHost {
        RealHost {
            root: root.to_path_buf(),
            cargo: env::current_exe().unwrap().into_os_string(),
            build_args: peer_args("cargo"),
            server: peer(mode),
            stdout: Box::new(io::sink()),
            stderr: Box::new(io::sink()),
        }
    }

    /// Écrivain qui garde ce qu'on lui écrit, lisible après coup.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Shared {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn cargo_program_is_never_empty() {
        assert!(!cargo_program().is_empty());
    }

    /// Ce que `report_ignored` transmet pour `ignored` lignes.
    fn reported(ignored: usize) -> Vec<String> {
        let mut said = Vec::new();
        report_ignored(ignored, &mut |line| said.push(line.to_owned()));
        said
    }

    #[test]
    fn ignored_lines_are_reported_once_and_only_when_there_are_some() {
        assert_eq!(reported(0), Vec::<String>::new());
        assert_eq!(
            reported(3),
            ["warning: 3 line(s) of the server's stdout were not JSON-RPC and were ignored"]
        );
    }

    #[test]
    fn the_warning_counts_the_ignored_lines_and_only_exists_for_some() {
        assert_eq!(ignored_warning(0), None);
        assert_eq!(
            ignored_warning(1).as_deref(),
            Some("warning: 1 line(s) of the server's stdout were not JSON-RPC and were ignored")
        );
        assert_eq!(
            ignored_warning(7).as_deref(),
            Some("warning: 7 line(s) of the server's stdout were not JSON-RPC and were ignored")
        );
    }

    #[test]
    fn the_three_sessions_add_up_the_lines_they_ignored() {
        // Chaque session ignore au moins la bannière du pair et l'en-tête de libtest.
        let (captures, ignored) = sessions(&peer("noisy")).unwrap();
        assert_eq!(captures.len(), SESSIONS.len());
        assert!(ignored >= 2 * SESSIONS.len(), "{ignored}");
    }

    #[test]
    fn the_real_host_says_on_stdout_and_warns_on_stderr() {
        let root = work_dir("console");
        let (out, err) = (Shared::default(), Shared::default());
        let mut host = peer_host(&root, "noisy");
        host.stdout = Box::new(out.clone());
        host.stderr = Box::new(err.clone());
        host.say("lock-descriptors: ok");
        host.warn("warning: careful");
        host.stdout.flush().unwrap();
        host.stderr.flush().unwrap();
        assert_eq!(out.text(), "lock-descriptors: ok\n");
        assert_eq!(err.text(), "warning: careful\n");
        // La construction d'un serveur bruyant avertit, sur l'erreur standard seulement.
        let captures = host.capture().unwrap();
        assert_eq!(captures.len(), SESSIONS.len());
        let warned = err.text();
        let warning = warned.lines().nth(1).unwrap();
        assert!(
            warning.starts_with("warning: ")
                && warning.ends_with(
                    " line(s) of the server's stdout were not JSON-RPC and were ignored"
                ),
            "{warned}"
        );
        assert_eq!(warned.lines().count(), 2, "{warned}");
        assert_eq!(out.text(), "lock-descriptors: ok\n");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn update_then_check_work_end_to_end_on_real_files_and_processes() {
        let root = work_dir("end-to-end");
        fs::write(root.join("Cargo.toml"), "[workspace.package]\nversion = \"0.1.0\"\n").unwrap();
        fs::create_dir_all(root.join("packaging/mcpb")).unwrap();
        fs::write(root.join(MANIFEST_PATH), r#"{"name": "mcp-legifrance", "version": "0.0.0"}"#)
            .unwrap();
        fs::write(root.join(SERVER_JSON_PATH), r#"{"version": "0.0.0"}"#).unwrap();
        execute(&LockMode::Update, &mut peer_host(&root, "server")).unwrap();
        let lock = fs::read_to_string(root.join(LOCK_PATH)).unwrap();
        assert!(lock.starts_with("{\n  \"capabilities\": {"), "{lock}");
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(root.join(MANIFEST_PATH)).unwrap()).unwrap();
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["tools"][0]["name"], "fr_coverage");
        execute(&LockMode::Check, &mut peer_host(&root, "server")).unwrap();
        fs::write(root.join(SERVER_JSON_PATH), r#"{"version": "9.9.9"}"#).unwrap();
        let error = execute(&LockMode::Check, &mut peer_host(&root, "server")).unwrap_err();
        assert_eq!(error, format!("server.json is out of date at /version\n{UPDATE_HINT}"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_real_host_reports_builds_and_servers_that_fail() {
        let root = work_dir("failures");
        let mut no_cargo = peer_host(&root, "server");
        no_cargo.cargo = OsString::from("/nonexistent/cargo");
        assert!(no_cargo.capture().unwrap_err().starts_with("cannot run /nonexistent/cargo: "));
        // Une construction qui réussit sans annoncer d'exécutable.
        let mut no_artifact = peer_host(&root, "server");
        no_artifact.build_args = ["--exact", "no_such_test"].map(OsString::from).to_vec();
        assert_eq!(
            no_artifact.capture().unwrap_err(),
            "cargo did not report an executable for `mcp-legifrance`"
        );
        let error = peer_host(&root, "fail").capture().unwrap_err();
        assert!(error.starts_with("the server exited with "), "{error}");
        // Un serveur qui se tait : la session échoue au bout du délai de lecture.
        let mut mute = peer_host(&root, "silent");
        mute.server.read_timeout = Duration::from_millis(1500);
        assert_eq!(
            mute.capture().unwrap_err(),
            "discover 2026-07-28: server/discover: no message from the server within 1.5 s"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_real_host_reads_and_writes_files_under_its_root() {
        let root = work_dir("files");
        let mut host = peer_host(&root, "server");
        assert_eq!(host.read("a/b.json"), Ok(None));
        host.write("a/b.json", "{}\n").unwrap();
        assert_eq!(host.read("a/b.json"), Ok(Some("{}\n".to_owned())));
        // Un dossier ne se lit ni ne s'écrase ; un fichier ne peut pas être un dossier parent.
        assert!(host.read("a").unwrap_err().starts_with("a: "));
        assert!(host.write("a", "x").unwrap_err().starts_with("a: "));
        assert!(host.write("a/b.json/c.json", "x").unwrap_err().contains("a/b.json"));
        host.say("lock-descriptors: console test");
        host.warn("warning: console test");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_real_host_reads_a_file_from_git() {
        let root = work_dir("git");
        process::run_git(&root, &["init", "-q"]).unwrap();
        fs::write(root.join("a.json"), "{\"staged\": true}\n").unwrap();
        process::run_git(&root, &["add", "a.json"]).unwrap();
        let mut host = peer_host(&root, "server");
        // Sans référence, `git show :a.json` lit la version de l'index.
        assert_eq!(host.git_show("", "a.json").unwrap(), "{\"staged\": true}\n");
        let error = host.git_show("no-such-ref", "a.json").unwrap_err();
        assert!(error.contains("show no-such-ref:a.json failed ("), "{error}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn run_reads_the_real_repository_and_reports_an_unknown_reference() {
        let error = run(LockMode::Compare("no-such-ref-for-the-test".to_owned())).unwrap_err();
        assert!(
            error.contains("show no-such-ref-for-the-test:tests/tools.lock.json failed ("),
            "{error}"
        );
    }

    #[test]
    fn a_malformed_workspace_manifest_or_packaging_file_is_reported() {
        let mut host = FakeHost::standard();
        host.files.insert("Cargo.toml".into(), "[".into());
        assert!(run_mode(&LockMode::Check, &mut host).unwrap_err().starts_with("Cargo.toml: "));
        let mut shapeless = FakeHost::standard();
        run_mode(&LockMode::Update, &mut shapeless).unwrap();
        shapeless.files.insert(MANIFEST_PATH.into(), "[]".into());
        assert_eq!(
            run_mode(&LockMode::Check, &mut shapeless).unwrap_err(),
            format!("the MCPB manifest is not a JSON object\n{UPDATE_HINT}")
        );
    }
}
