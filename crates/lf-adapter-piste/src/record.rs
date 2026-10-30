//! Enregistrement de nouvelles fixtures (feature `record`, 04 §1.4, §13.4 ; 06 §4.4).
//!
//! API brute employée par `examples/record_fixture.rs` (`cargo xtask record-fixture`) et par
//! les tests live, jamais par le binaire :
//! [`PisteCorpus::exchange`](crate::PisteCorpus::exchange) envoie une requête par le vrai
//! client de l'adapter (User-Agent, OAuth, limiteur, borne de corps) et rend un [`Exchange`]
//! (statut, en-têtes utiles, octets reçus, sans classification, réessai ni réparation) ; ce
//! module en tire le fichier enregistré et son sidecar `.meta.toml`.
//!
//! Garanties, dans l'ordre de [`write()`] :
//! - **aucun identifiant ni jeton écrit** : en-têtes et jeton ne sont jamais écrits, et une
//!   réponse qui recopie la valeur exacte du jeton, du secret ou du `client_id`
//!   ([`Exchange::echoes_credentials`], calculé par l'adapter qui les détient) n'est pas écrite
//!   du tout : c'est l'expurgation **par valeur** (06 §4.4 point 5), en plus du contrôle de forme
//!   des tests d'intégrité ;
//! - un statut hors 2xx n'est écrit qu'avec `allow_error` (sondes « introuvable », 04 §11), et
//!   jamais un 401 ni un 429 ;
//! - un fichier existant n'est remplacé qu'avec `force`, jamais à travers un lien symbolique,
//!   et rien n'est créé hors de la racine ;
//! - les octets sont gardés tels quels, sauf les valeurs caviardées par pointeur JSON
//!   (`/acco/data` par défaut sur `/consult/acco`) : la valeur est remplacée **à sa place dans
//!   les octets** (aucun reformatage du reste), `sha256_raw` garde l'empreinte d'origine et
//!   `redactions` liste les pointeurs appliqués.
//!
//! Le chemin d'un fichier est `<racine>/<endpoint sans « / » initial>/<nom>.json` ; endpoint et
//! nom sont contrôlés ([`check_endpoint`], [`check_name`]) : ni `..`, ni chemin absolu, ni
//! caractère hors de `[A-Za-z0-9_.-]`.

use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lf_domain::coverage::PisteEnvironment;
use lf_domain::text::encoding::has_latin1_mojibake;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::sha256::{hex, sha256};

/// Champ `tool` du sidecar.
pub const TOOL: &str = concat!("xtask record-fixture ", env!("CARGO_PKG_VERSION"));

/// Champ `license` du sidecar (00 §12).
pub const LICENSE: &str = "Licence Ouverte 2.0 — source : Légifrance / DILA";

/// Endpoint dont la réponse est caviardée par défaut.
pub const ACCO_ENDPOINT: &str = crate::map::endpoint::ACCO;

/// Pointeur caviardé par défaut sur [`ACCO_ENDPOINT`] : base64 du `.docx` de l'accord.
pub const ACCO_DATA: &str = "/acco/data";

/// Taille au-delà de laquelle un fichier enregistré ne se versionne pas tel quel (06 §4.4 point 6).
pub const MAX_VERSIONED_BYTES: usize = 1 << 20;

/// Une sonde : une requête à enregistrer (`[[probe]]` d'un plan de lot, ou la ligne de commande).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// Gabarit de chemin de l'API (`/consult/juri`).
    pub endpoint: String,
    /// Corps JSON de la requête, tel qu'envoyé.
    pub body: String,
    /// Nom du fichier, sans extension (`juri/JURITEXT000051743650`).
    pub name: String,
    /// Écrire aussi une réponse hors 2xx.
    #[serde(default)]
    pub allow_error: bool,
    /// Pointeurs JSON à caviarder en plus du défaut.
    #[serde(default)]
    pub redact: Vec<String>,
}

/// Plan de lot (`crates/lf-adapter-piste/tests/fixtures/plan.toml`) : une liste de sondes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Sondes, dans l'ordre d'exécution.
    #[serde(default)]
    pub probe: Vec<Probe>,
}

/// Aide affichée avec un argument inconnu.
pub const USAGE: &str = "usage: record_fixture (--endpoint <path> --body <json> --name <name> [--allow-error] [--redact <pointer>]... | --plan <plan.toml> [--only <prefix>]) [--env sandbox|production] [--force] [--out <dir>]";

/// Ce qu'il faut enregistrer : une sonde de la ligne de commande, ou un lot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// `--endpoint --body --name [--allow-error] [--redact]…`.
    Single(Probe),
    /// `--plan <fichier> [--only <préfixe>]`.
    Plan {
        /// Fichier du lot.
        path: PathBuf,
        /// Préfixe de nom retenu ; `None` = toutes les sondes.
        only: Option<String>,
    },
}

/// Arguments de `record_fixture`, déjà contrôlés de façon cohérente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Sonde seule ou lot.
    pub mode: Mode,
    /// Environnement PISTE (sandbox par défaut).
    pub env: PisteEnvironment,
    /// Remplace les fichiers existants.
    pub force: bool,
    /// Racine d'écriture ; `None` = le dossier des fixtures enregistrées.
    pub out: Option<PathBuf>,
}

/// Lit et contrôle les arguments : aucun accès au système, aucun identifiant.
///
/// # Errors
///
/// [`RecordError::Usage`] : argument inconnu ou sans valeur, argument à valeur unique répété
/// (`--redact` seul peut se répéter), `--only` sans `--plan`, `--plan` mêlé à une option de
/// sonde seule, `--env` hors `sandbox` / `production`, `--plan` avec `--env production`.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, RecordError> {
    // Options qu'une sonde seule porte ; `--plan` ne s'y mêle pas.
    const PROBE_FLAGS: [&str; 5] = ["--endpoint", "--body", "--name", "--allow-error", "--redact"];
    let usage = |message: String| RecordError::Usage(message);
    let mut args = args.into_iter();
    // Chaque option lue, dans l'ordre : sert à refuser les répétitions et les mélanges.
    let mut used: Vec<String> = Vec::new();
    let mut probe = Probe::default();
    let (mut plan, mut only, mut env, mut out) = (None, None, None, None);
    let mut force = false;
    while let Some(flag) = args.next() {
        if flag != "--redact" && used.contains(&flag) {
            return Err(usage(format!("{flag} given twice")));
        }
        used.push(flag.clone());
        let mut value = || args.next().ok_or_else(|| usage(format!("{flag} needs a value")));
        match flag.as_str() {
            "--endpoint" => probe.endpoint = value()?,
            "--body" => probe.body = value()?,
            "--name" => probe.name = value()?,
            "--redact" => probe.redact.push(value()?),
            "--allow-error" => probe.allow_error = true,
            "--plan" => plan = Some(PathBuf::from(value()?)),
            "--only" => only = Some(value()?),
            "--env" => env = Some(value()?),
            "--force" => force = true,
            "--out" => out = Some(PathBuf::from(value()?)),
            _ => {
                // Un argument inconnu peut porter une valeur (`--jeton=…`) : on n'en montre que
                // le nom, jamais ce qui suit « = ».
                let name = flag.split_once('=').map_or(flag.as_str(), |(name, _)| name);
                return Err(usage(format!("unknown argument {name:?}\n{USAGE}")));
            }
        }
    }
    let mode = if let Some(path) = plan {
        if let Some(flag) = used.iter().find(|flag| PROBE_FLAGS.contains(&flag.as_str())) {
            return Err(usage(format!("{flag} cannot be combined with --plan")));
        }
        Mode::Plan { path, only }
    } else if only.is_some() {
        return Err(usage("--only needs --plan".to_owned()));
    } else {
        Mode::Single(probe)
    };
    let env = match env.as_deref().unwrap_or("sandbox") {
        "sandbox" => PisteEnvironment::Sandbox,
        "production" => PisteEnvironment::Production,
        other => return Err(usage(format!("--env must be sandbox or production, not {other:?}"))),
    };
    // Un lot est un balayage : il ne se joue jamais en production (Q3) ; une sonde seule le peut.
    if env == PisteEnvironment::Production && matches!(mode, Mode::Plan { .. }) {
        return Err(usage("--plan runs against the sandbox only".to_owned()));
    }
    Ok(Args { mode, env, force, out })
}

/// Retient les sondes dont le nom commence par `only` (`None` = toutes).
///
/// # Errors
///
/// [`RecordError::NoProbe`] si la sélection est vide (faute de frappe dans le préfixe, plan
/// vide) : sans cela l'outil exigerait des identifiants pour ne rien enregistrer.
pub fn select(probes: Vec<Probe>, only: Option<&str>) -> Result<Vec<Probe>, RecordError> {
    let prefix = only.unwrap_or("");
    let selected: Vec<Probe> =
        probes.into_iter().filter(|probe| probe.name.starts_with(prefix)).collect();
    if selected.is_empty() { Err(RecordError::NoProbe(prefix.to_owned())) } else { Ok(selected) }
}

/// Échange brut avec l'API : ce que l'adapter a reçu, sans interprétation.
///
/// `Debug` écrit à la main : le corps n'est jamais affiché, seulement sa longueur.
#[derive(Clone, PartialEq, Eq)]
pub struct Exchange {
    /// Code de statut HTTP.
    pub status: u16,
    /// `Content-Type` reçu.
    pub content_type: Option<String>,
    /// `Content-Encoding` laissé par `reqwest` : absent pour une réponse gzip, qu'il décompresse
    /// en retirant l'en-tête ; la négociation gzip (V-70) ne se voit donc pas ici.
    pub content_encoding: Option<String>,
    /// `Retry-After` reçu.
    pub retry_after: Option<String>,
    /// Durée de l'envoi et de la lecture du corps (horloge de l'adapter).
    pub elapsed: Duration,
    /// Vrai si le corps contient la valeur exacte du jeton, du secret client ou du `client_id`.
    pub echoes_credentials: bool,
    /// Octets reçus, décompressés, non réparés.
    pub body: Vec<u8>,
}

impl fmt::Debug for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Exchange")
            .field("status", &self.status)
            .field("content_type", &self.content_type)
            .field("content_encoding", &self.content_encoding)
            .field("retry_after", &self.retry_after)
            .field("elapsed", &self.elapsed)
            .field("echoes_credentials", &self.echoes_credentials)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Refus d'enregistrer.
#[derive(Debug, Error)]
pub enum RecordError {
    /// Gabarit de chemin refusé.
    #[error("endpoint must be a path of [A-Za-z0-9_-] segments starting with /: {0:?}")]
    Endpoint(String),
    /// Nom de fixture refusé.
    #[error("fixture name must be /-separated segments of [A-Za-z0-9_.-], without . or ..: {0:?}")]
    Name(String),
    /// Corps de requête qui n'est pas du JSON.
    #[error("request body of {0:?} is not JSON")]
    Body(String),
    /// Réponse hors 2xx sans `allow_error`.
    #[error("HTTP {0} not recorded without --allow-error")]
    Status(u16),
    /// Statut 401 ou 429 : jeton refusé ou quota atteint, jamais une réponse de données.
    #[error("HTTP {0} is an authentication or rate-limit reply: never recorded")]
    Unrecordable(u16),
    /// Le fichier ou le sidecar visé est un lien symbolique : on n'écrit pas à travers.
    #[error("{0} is a symbolic link: refusing to write through it")]
    Symlink(PathBuf),
    /// Le dossier visé sort de la racine d'écriture (un composant est un lien symbolique).
    #[error("{0} resolves outside the output directory")]
    Outside(PathBuf),
    /// La réponse recopie un identifiant ou le jeton : rien n'est écrit.
    #[error("the response echoes a credential: nothing written")]
    Echo,
    /// Le fichier existe déjà et `force` n'est pas demandé.
    #[error("{0} exists: use --force to overwrite")]
    Exists(PathBuf),
    /// Pointeur de caviardage inapplicable.
    #[error("cannot redact {pointer}: {reason}")]
    Redact {
        /// Pointeur JSON demandé.
        pointer: String,
        /// Raison, en clair.
        reason: &'static str,
    },
    /// Arguments de ligne de commande incohérents ou inconnus (code de sortie 2).
    #[error("{0}")]
    Usage(String),
    /// `--only` (ou un plan vide) ne retient aucune sonde : rien à enregistrer.
    #[error("no probe selected (name prefix {0:?})")]
    NoProbe(String),
    /// Écriture impossible.
    #[error("cannot write {path}: {source}")]
    Io {
        /// Fichier visé.
        path: PathBuf,
        /// Erreur du système.
        source: io::Error,
    },
}

/// Ce que [`write()`] a écrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Fichier des octets enregistrés.
    pub json: PathBuf,
    /// Sidecar.
    pub meta: PathBuf,
    /// Taille du fichier écrit (après caviardage).
    pub bytes_file: usize,
    /// Pointeurs caviardés.
    pub redactions: Vec<String>,
}

/// Issue d'une sonde jouée, telle que la synthèse du lot la compte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Réponse reçue et fichier écrit.
    Recorded,
    /// Erreur d'échange, ou refus d'écrire ([`write()`]).
    NotRecorded,
}

/// Nombre de sondes qui n'ont pas été enregistrées.
#[must_use]
pub fn failures(outcomes: &[Outcome]) -> usize {
    outcomes.iter().filter(|outcome| **outcome == Outcome::NotRecorded).count()
}

/// Code de sortie de `record_fixture` (04 §13.4) : 0 si toutes les sondes sont enregistrées,
/// 1 si au moins une ne l'est pas (`Ok(n)` avec `n > 0`), 2 si le lot n'a pas commencé (`Err` :
/// usage, contrôle de sonde, plan illisible, identifiants absents).
#[must_use]
pub fn exit_code(result: &Result<usize, String>) -> u8 {
    match result {
        Ok(0) => 0,
        Ok(_) => 1,
        Err(_) => 2,
    }
}

/// Contrôle un gabarit de chemin : `/`, puis des segments non vides de `[A-Za-z0-9_-]`.
///
/// # Errors
///
/// [`RecordError::Endpoint`] sinon.
pub fn check_endpoint(endpoint: &str) -> Result<(), RecordError> {
    let valid = endpoint.strip_prefix('/').is_some_and(|rest| {
        rest.split('/').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    });
    if valid { Ok(()) } else { Err(RecordError::Endpoint(endpoint.to_owned())) }
}

/// Contrôle un nom de fixture : segments non vides de `[A-Za-z0-9_.-]` séparés par `/`, ni `.`
/// ni `..`, sans `/` initial ni final.
///
/// # Errors
///
/// [`RecordError::Name`] sinon.
pub fn check_name(name: &str) -> Result<(), RecordError> {
    let valid = name.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    });
    if valid { Ok(()) } else { Err(RecordError::Name(name.to_owned())) }
}

impl Probe {
    /// Contrôle l'endpoint, le nom, le corps (JSON) et les pointeurs (`/` initial).
    ///
    /// # Errors
    ///
    /// La première [`RecordError`] rencontrée, dans cet ordre.
    pub fn check(&self) -> Result<(), RecordError> {
        check_endpoint(&self.endpoint)?;
        check_name(&self.name)?;
        if serde_json::from_str::<Value>(&self.body).is_err() {
            return Err(RecordError::Body(self.name.clone()));
        }
        match self.redact.iter().find(|pointer| !pointer.starts_with('/')) {
            Some(pointer) => {
                Err(RecordError::Redact { pointer: pointer.clone(), reason: "not a JSON pointer" })
            }
            None => Ok(()),
        }
    }

    /// Pointeurs à caviarder : `/acco/data` d'abord sur un 2xx de [`ACCO_ENDPOINT`], puis ceux
    /// de la sonde, sans doublon.
    fn pointers(&self, status: u16) -> Vec<String> {
        let mut pointers = Vec::new();
        if self.endpoint == ACCO_ENDPOINT && is_success(status) {
            pointers.push(ACCO_DATA.to_owned());
        }
        for pointer in &self.redact {
            if !pointers.contains(pointer) {
                pointers.push(pointer.clone());
            }
        }
        pointers
    }
}

/// Vrai pour un statut 2xx.
fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Fichier enregistré et sidecar de `name` sous `root`, rangés par endpoint.
#[must_use]
pub fn fixture_paths(root: &Path, endpoint: &str, name: &str) -> (PathBuf, PathBuf) {
    let base = root.join(endpoint.trim_start_matches('/')).join(name);
    let mut json = base.clone().into_os_string();
    json.push(".json");
    let mut meta = base.into_os_string();
    meta.push(".meta.toml");
    (PathBuf::from(json), PathBuf::from(meta))
}

/// Détection du mojibake latin-1 (étape 1 de 04 §10.2) sur les octets bruts, relus en UTF-8
/// (avec remplacement : des octets invalides ne cachent pas un motif voisin).
#[must_use]
pub fn mojibake_detected(body: &[u8]) -> bool {
    has_latin1_mojibake(&String::from_utf8_lossy(body))
}

/// SHA-256 de `bytes`, en hexadécimal minuscule.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&sha256(bytes))
}

/// Instant `unix_secs` en RFC 3339 UTC à la seconde (`2026-11-02T10:15:00Z`) ; `unknown` hors
/// de la plage de `chrono`.
#[must_use]
pub fn rfc3339_utc(unix_secs: i64) -> String {
    chrono::DateTime::from_timestamp(unix_secs, 0)
        .map_or_else(|| "unknown".to_owned(), |at| at.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Caviarde `raw` aux pointeurs donnés, sans toucher au reste des octets ; rend le fichier et
/// les pointeurs appliqués (un pointeur absent du document est ignoré).
///
/// Une chaîne de `n` octets devient `"<redacted: n bytes base64>"` pour [`ACCO_DATA`],
/// `"<redacted: n bytes>"` ailleurs ; une autre valeur, `"<redacted: n bytes>"` où `n` est la
/// longueur de son texte JSON. La place de la valeur est retrouvée dans les octets
/// (`span`) puis vérifiée : ce qu'on y lit doit être la valeur que désigne le pointeur.
///
/// # Errors
///
/// [`RecordError::Redact`] si des pointeurs sont demandés sur un corps qui n'est pas du JSON, si
/// la place d'une valeur ne se retrouve pas, ou si une clé en double ferait survivre la valeur
/// (`duplicate key`).
pub fn redact(raw: &[u8], pointers: &[String]) -> Result<(Vec<u8>, Vec<String>), RecordError> {
    let mut file = raw.to_vec();
    let mut applied = Vec::new();
    for pointer in pointers {
        let failure = |reason| RecordError::Redact { pointer: pointer.clone(), reason };
        let document: Value =
            serde_json::from_slice(&file).map_err(|_| failure("body is not JSON"))?;
        let tokens = pointer_tokens(pointer);
        let Some(target) = document.pointer(pointer) else {
            // Absent pour `serde_json`, présent dans les octets : une clé en double, dont la
            // dernière occurrence (lue par `serde_json`) masque une première qu'on laisserait en
            // clair (L1). Absent des deux côtés, il n'y a rien à caviarder.
            if span(&file, &tokens).is_some() {
                return Err(failure("duplicate key"));
            }
            continue;
        };
        let place = span(&file, &tokens)
            .filter(|place| {
                file.get(place.clone())
                    .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
                    .as_ref()
                    == Some(target)
            })
            .ok_or_else(|| failure("value not found in the raw bytes"))?;
        let label = match target {
            Value::String(text) if pointer == ACCO_DATA => {
                format!("<redacted: {} bytes base64>", text.len())
            }
            Value::String(text) => format!("<redacted: {} bytes>", text.len()),
            _ => format!("<redacted: {} bytes>", place.len()),
        };
        let expected = Value::String(label);
        file.splice(place, expected.to_string().bytes());
        // Relecture : le pointeur doit désormais lire l'étiquette posée. Une clé en double dont
        // les deux occurrences portent la même valeur passerait le contrôle de `span` ; la
        // seconde, restée en clair, se voit ici. `.ok()` : un fichier devenu illisible échoue de
        // même, sans branche à part.
        let readback = serde_json::from_slice::<Value>(&file).ok();
        if readback.as_ref().and_then(|document| document.pointer(pointer)) != Some(&expected) {
            return Err(failure("duplicate key"));
        }
        applied.push(pointer.clone());
    }
    Ok((file, applied))
}

/// Segments d'un pointeur JSON (RFC 6901) : `~1` devient `/`, `~0` devient `~`.
fn pointer_tokens(pointer: &str) -> Vec<String> {
    pointer.split('/').skip(1).map(|token| token.replace("~1", "/").replace("~0", "~")).collect()
}

/// Place, dans `bytes` (JSON valide), de la valeur désignée par `tokens`.
fn span(bytes: &[u8], tokens: &[String]) -> Option<Range<usize>> {
    let mut start = skip_blank(bytes, 0);
    for token in tokens {
        start = child(bytes, start, token)?;
    }
    Some(start..value_end(bytes, start)?)
}

/// Premier octet non blanc à partir de `at`.
fn skip_blank(bytes: &[u8], at: usize) -> usize {
    let blank = bytes
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        .count();
    at + blank
}

/// Fin (exclue) de la chaîne JSON qui commence au `"` de `at`.
fn string_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut index = at + 1;
    loop {
        match bytes.get(index)? {
            b'\\' => index += 2,
            b'"' => return Some(index + 1),
            // Pas de `+=` ici : son mutant `*=` vaut `*= 1`, l'identité, et la boucle ne finirait
            // jamais (le test ne le verrait que par le délai). `saturating_add` n'a pas de mutant.
            _ => index = index.saturating_add(1),
        }
    }
}

/// Fin (exclue) de la valeur JSON qui commence à `at`.
#[expect(
    clippy::indexing_slicing,
    reason = "les positions viennent du balayage de ces mêmes octets (`skip_blank`, `string_end`) et ne dépassent pas leur longueur"
)]
fn value_end(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at)? {
        b'"' => string_end(bytes, at),
        b'{' | b'[' => {
            let mut depth: usize = 0;
            let mut index = at;
            loop {
                match bytes.get(index)? {
                    b'"' => {
                        index = string_end(bytes, index)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(index + 1);
                        }
                    }
                    _ => {}
                }
                // Voir `string_end` : `+= 1` aurait un mutant `*= 1` qui boucle sans fin.
                index = index.saturating_add(1);
            }
        }
        _ => {
            let rest = &bytes[at..];
            let length = rest
                .iter()
                .position(|byte| matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'))
                .unwrap_or(rest.len());
            Some(at + length)
        }
    }
}

/// Début de la valeur `token` du conteneur qui commence à `at` : membre d'un objet (clé
/// décodée) ou élément d'un tableau (indice décimal).
#[expect(
    clippy::indexing_slicing,
    reason = "les positions viennent du balayage de ces mêmes octets (`skip_blank`, `string_end`) et ne dépassent pas leur longueur"
)]
fn child(bytes: &[u8], at: usize, token: &str) -> Option<usize> {
    let open = *bytes.get(at)?;
    let wanted = match open {
        b'{' => None,
        b'[' => Some(token.parse::<usize>().ok()?),
        _ => return None,
    };
    let mut index = skip_blank(bytes, at + 1);
    let mut position = 0;
    while !matches!(bytes.get(index)?, b'}' | b']') {
        let value = if wanted.is_none() {
            let key_end = string_end(bytes, index)?;
            let key: String = serde_json::from_slice(&bytes[index..key_end]).ok()?;
            let value = skip_blank(bytes, skip_blank(bytes, key_end) + 1);
            if key == token {
                return Some(value);
            }
            value
        } else {
            if wanted == Some(position) {
                return Some(index);
            }
            index
        };
        index = skip_blank(bytes, value_end(bytes, value)?);
        if bytes.get(index) == Some(&b',') {
            index = skip_blank(bytes, index + 1);
        }
        position += 1;
    }
    None
}

/// Chaîne TOML de base (`"…"`) : `\` et `"` échappés, caractères de contrôle en `\uXXXX`.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Sidecar `.meta.toml` d'un enregistrement (04 §13.4), plus `elapsed_ms` (V-96).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    /// Gabarit de chemin.
    pub endpoint: String,
    /// Corps de la requête, tel qu'envoyé.
    pub request: String,
    /// Environnement PISTE.
    pub env: PisteEnvironment,
    /// Instant de la capture, RFC 3339 UTC.
    pub captured_at: String,
    /// Statut HTTP.
    pub http_status: u16,
    /// `Content-Type` reçu.
    pub content_type: Option<String>,
    /// `Content-Encoding` laissé par `reqwest` (absent pour du gzip).
    pub content_encoding: Option<String>,
    /// Durée de l'échange, en millisecondes.
    pub elapsed_ms: u128,
    /// Taille des octets reçus.
    pub bytes_raw: usize,
    /// Empreinte des octets reçus.
    pub sha256_raw: String,
    /// Empreinte du fichier écrit (différente seulement après caviardage).
    pub sha256_file: String,
    /// Mojibake détecté sur les octets reçus.
    pub mojibake_detected: bool,
    /// Pointeurs caviardés.
    pub redactions: Vec<String>,
}

impl Sidecar {
    /// Texte TOML du sidecar, clés dans l'ordre de 04 §13.4 ; `content_type` et
    /// `content_encoding` sont omis quand ils manquent.
    #[must_use]
    pub fn render(&self) -> String {
        let mut lines = vec![
            format!("endpoint = {}", toml_string(&self.endpoint)),
            format!("request = {}", toml_string(&self.request)),
            format!("env = {}", toml_string(self.env.name())),
            format!("captured_at = {}", toml_string(&self.captured_at)),
            format!("http_status = {}", self.http_status),
        ];
        if let Some(content_type) = &self.content_type {
            lines.push(format!("content_type = {}", toml_string(content_type)));
        }
        if let Some(content_encoding) = &self.content_encoding {
            lines.push(format!("content_encoding = {}", toml_string(content_encoding)));
        }
        let redactions: Vec<String> = self.redactions.iter().map(|p| toml_string(p)).collect();
        lines.extend([
            format!("elapsed_ms = {}", self.elapsed_ms),
            format!("bytes_raw = {}", self.bytes_raw),
            format!("sha256_raw = {}", toml_string(&self.sha256_raw)),
            format!("sha256_file = {}", toml_string(&self.sha256_file)),
            format!("mojibake_detected = {}", self.mojibake_detected),
            format!("redactions = [{}]", redactions.join(", ")),
            format!("tool = {}", toml_string(TOOL)),
            format!("license = {}", toml_string(LICENSE)),
        ]);
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// Écrit l'enregistrement de `probe` sous `root` : le fichier (octets reçus, caviardés au
/// besoin) puis son sidecar.
///
/// Refus, dans l'ordre : sonde invalide ([`Probe::check`]), écho d'un identifiant, statut 401
/// ou 429, statut hors 2xx sans `allow_error`, fichier existant sans `force` (un lien pendant
/// compte), fichier ou sidecar qui est un lien symbolique, caviardage inapplicable, dossier qui
/// mène hors de `root`. Aucun fichier n'est écrit avant ces contrôles (la racine peut avoir été
/// créée).
///
/// # Errors
///
/// La [`RecordError`] du refus, ou [`RecordError::Io`].
pub fn write(
    root: &Path,
    probe: &Probe,
    exchange: &Exchange,
    env: PisteEnvironment,
    captured_at: &str,
    force: bool,
) -> Result<Written, RecordError> {
    probe.check()?;
    if exchange.echoes_credentials {
        return Err(RecordError::Echo);
    }
    // 401 et 429 : jamais enregistrés, `allow_error` ou non (I1) ; avant le test du statut 2xx.
    if matches!(exchange.status, 401 | 429) {
        return Err(RecordError::Unrecordable(exchange.status));
    }
    if !is_success(exchange.status) && !probe.allow_error {
        return Err(RecordError::Status(exchange.status));
    }
    let (json, meta) = fixture_paths(root, &probe.endpoint, &probe.name);
    // `symlink_metadata` : un lien pendant existe (`Path::exists` le suivrait et dirait non).
    if !force && [&json, &meta].iter().any(|path| fs::symlink_metadata(path).is_ok()) {
        return Err(RecordError::Exists(json));
    }
    // Même avec `--force`, jamais à travers un lien symbolique (L2).
    if let Some(link) = [&json, &meta].into_iter().find(|path| is_symlink(path)) {
        return Err(RecordError::Symlink(link.clone()));
    }
    let (file, redactions) = redact(&exchange.body, &probe.pointers(exchange.status))?;
    let sidecar = Sidecar {
        endpoint: probe.endpoint.clone(),
        request: probe.body.clone(),
        env,
        captured_at: captured_at.to_owned(),
        http_status: exchange.status,
        content_type: exchange.content_type.clone(),
        content_encoding: exchange.content_encoding.clone(),
        elapsed_ms: exchange.elapsed.as_millis(),
        bytes_raw: exchange.body.len(),
        sha256_raw: sha256_hex(&exchange.body),
        sha256_file: sha256_hex(&file),
        mojibake_detected: mojibake_detected(&exchange.body),
        redactions: redactions.clone(),
    };
    // `fixture_paths` place toujours le fichier sous `root`.
    let parent = json.parent().unwrap_or(root);
    create_parent(root, parent)?;
    fs::write(&json, &file).map_err(|source| RecordError::Io { path: json.clone(), source })?;
    fs::write(&meta, sidecar.render())
        .map_err(|source| RecordError::Io { path: meta.clone(), source })?;
    Ok(Written { json, meta, bytes_file: file.len(), redactions })
}

/// Vrai si `path` est lui-même un lien symbolique (pendant ou non).
fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// Crée `parent` sous `root` sans jamais sortir de `root` (L2) : la racine est créée, puis le
/// plus proche ancêtre existant de `parent` doit, une fois les liens résolus, rester sous la
/// racine résolue. Le contrôle précède la création : rien n'est créé hors de la racine. Un
/// échec de résolution refuse aussi (échec fermé). Une racine qui est elle-même un lien reste
/// permise : les deux côtés sont résolus.
fn create_parent(root: &Path, parent: &Path) -> Result<(), RecordError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| RecordError::Io { path, source }
    };
    fs::create_dir_all(root).map_err(io(root))?;
    let nearest = parent.ancestors().find(|ancestor| ancestor.exists());
    let inside = fs::canonicalize(root)
        .ok()
        .zip(nearest.and_then(|ancestor| fs::canonicalize(ancestor).ok()))
        .is_some_and(|(root, nearest)| nearest.starts_with(root));
    if !inside {
        return Err(RecordError::Outside(parent.to_path_buf()));
    }
    fs::create_dir_all(parent).map_err(io(parent))
}

#[cfg(test)]
mod tests {
    use std::process;

    use super::*;

    fn probe(endpoint: &str, name: &str) -> Probe {
        Probe {
            endpoint: endpoint.to_owned(),
            body: r#"{"textId":"JURITEXT000051743650"}"#.to_owned(),
            name: name.to_owned(),
            allow_error: false,
            redact: Vec::new(),
        }
    }

    fn exchange(status: u16, body: &[u8]) -> Exchange {
        Exchange {
            status,
            content_type: Some("application/json".to_owned()),
            content_encoding: None,
            retry_after: None,
            elapsed: Duration::from_millis(1234),
            echoes_credentials: false,
            body: body.to_vec(),
        }
    }

    /// Répertoire de travail propre au test et au processus, vidé au départ.
    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lf-record-{test}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn pointers(list: &[&str]) -> Vec<String> {
        list.iter().map(|pointer| (*pointer).to_owned()).collect()
    }

    #[test]
    fn the_versioned_size_limit_is_one_mebibyte() {
        // Seuil de 06 §4.4 point 6 : le mutant `1 >> 20` le ramènerait à zéro sans autre signal
        // (seul l'exemple `record-fixture` le lit).
        assert_eq!(MAX_VERSIONED_BYTES, 1_048_576);
    }

    #[test]
    fn check_endpoint_accepts_api_paths_and_refuses_the_rest() {
        for good in ["/search", "/consult/juri", "/consult/legi/tableMatieres", "/list/code_x-1"] {
            assert!(check_endpoint(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "/",
            "search",
            "//x",
            "/a//b",
            "/a/../b",
            "/a b",
            "/consult//juri",
            "/consult/juri/",
            "/consult/../oauth",
            "/consult/ju ri",
            "/consult/juri?x=1",
            "/consult/é",
            "/consult/./juri",
        ] {
            assert_eq!(
                check_endpoint(bad).map_err(|error| error.to_string()),
                Err(format!(
                    "endpoint must be a path of [A-Za-z0-9_-] segments starting with /: {bad:?}"
                ))
            );
        }
    }

    #[test]
    fn check_name_refuses_traversal_and_odd_characters() {
        for good in ["juri/JURITEXT000051743650", "a.b-c_d", "x/y/z"] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in ["", "/abs", "a//b", "a/", "..", "a/../b", ".", "a b", "a\\b", "é"] {
            assert_eq!(
                check_name(bad).map_err(|error| error.to_string()),
                Err(format!(
                    "fixture name must be /-separated segments of [A-Za-z0-9_.-], without . or ..: {bad:?}"
                ))
            );
        }
    }

    #[test]
    fn a_probe_check_reports_the_first_problem() {
        assert!(probe("/consult/juri", "ok").check().is_ok());
        let mut bad = probe("/consult/juri", "ok");
        bad.body = "{".to_owned();
        assert_eq!(bad.check().unwrap_err().to_string(), r#"request body of "ok" is not JSON"#);
        let mut bad = probe("/consult/juri", "ok");
        bad.redact = pointers(&["/fine", "acco/data"]);
        assert_eq!(
            bad.check().unwrap_err().to_string(),
            "cannot redact acco/data: not a JSON pointer"
        );
        assert_eq!(
            probe("juri", "ok").check().unwrap_err().to_string(),
            r#"endpoint must be a path of [A-Za-z0-9_-] segments starting with /: "juri""#
        );
        assert_eq!(
            probe("/juri", "..").check().unwrap_err().to_string(),
            r#"fixture name must be /-separated segments of [A-Za-z0-9_.-], without . or ..: "..""#
        );
    }

    #[test]
    fn acco_data_is_redacted_by_default_on_a_successful_acco_reply_only() {
        let mut acco = probe(ACCO_ENDPOINT, "a");
        assert_eq!(acco.pointers(200), [ACCO_DATA]);
        assert_eq!(acco.pointers(299), [ACCO_DATA]);
        assert_eq!(acco.pointers(400), Vec::<String>::new());
        assert_eq!(acco.pointers(199), Vec::<String>::new());
        acco.redact = pointers(&["/acco/data", "/acco/attachment"]);
        assert_eq!(acco.pointers(200), ["/acco/data", "/acco/attachment"]);
        let mut juri = probe("/consult/juri", "j");
        assert_eq!(juri.pointers(200), Vec::<String>::new());
        juri.redact = pointers(&["/text/x"]);
        assert_eq!(juri.pointers(500), ["/text/x"]);
    }

    #[test]
    fn fixture_paths_nest_by_endpoint() {
        let (json, meta) = fixture_paths(Path::new("rec"), "/consult/juri", "juri/JURITEXT1");
        assert_eq!(json, Path::new("rec/consult/juri/juri/JURITEXT1.json"));
        assert_eq!(meta, Path::new("rec/consult/juri/juri/JURITEXT1.meta.toml"));
    }

    #[test]
    fn mojibake_is_detected_on_the_raw_bytes() {
        assert!(mojibake_detected("PREMI\u{c3}\u{88}RE".as_bytes()));
        assert!(mojibake_detected("R\u{c3}\u{a9}publique".as_bytes()));
        assert!(!mojibake_detected("PREMIÈRE République".as_bytes()));
        assert!(!mojibake_detected(b"\xff caf\xc3\xa9"));
        assert!(mojibake_detected(b"\xff R\xc3\x83\xc2\xa9publique"));
    }

    #[test]
    fn sha256_hex_is_the_fips_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn rfc3339_utc_formats_seconds_and_marks_out_of_range() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_792_058_400), "2026-10-15T10:00:00Z");
        assert_eq!(rfc3339_utc(1_793_182_500), "2026-10-28T10:15:00Z");
        assert_eq!(rfc3339_utc(i64::MAX), "unknown");
    }

    #[test]
    fn redact_replaces_the_value_in_place_and_keeps_every_other_byte() {
        let raw = br#"{ "executionTime" : 3,
  "acco": {"id":"ACCOTEXT1", "data" : "QUJD\/RA==","x":[1, {"data":"keep"}]} }"#;
        let (file, applied) = redact(raw, &pointers(&[ACCO_DATA])).unwrap();
        assert_eq!(
            String::from_utf8(file).unwrap(),
            r#"{ "executionTime" : 3,
  "acco": {"id":"ACCOTEXT1", "data" : "<redacted: 9 bytes base64>","x":[1, {"data":"keep"}]} }"#
        );
        assert_eq!(applied, [ACCO_DATA]);
    }

    #[test]
    fn redact_reaches_arrays_escaped_keys_and_any_value() {
        let raw = br#"{"a/b": [ 0 , {"k":"v\"q"} ], "n": 12, "t~": true, "s":"\u00e9t\u00e9"}"#;
        let (file, applied) =
            redact(raw, &pointers(&["/a~1b/1", "/n", "/t~0", "/s", "/a~1b/7", "/none"])).unwrap();
        assert_eq!(
            String::from_utf8(file).unwrap(),
            r#"{"a/b": [ 0 , "<redacted: 12 bytes>" ], "n": "<redacted: 2 bytes>", "t~": "<redacted: 4 bytes>", "s":"<redacted: 5 bytes>"}"#
        );
        assert_eq!(applied, ["/a~1b/1", "/n", "/t~0", "/s"]);
        let (file, applied) = redact(b"[ \"x\" , [1,2] ]", &pointers(&["/1/0"])).unwrap();
        assert_eq!(
            (file, applied),
            (b"[ \"x\" , [\"<redacted: 1 bytes>\",2] ]".to_vec(), pointers(&["/1/0"]))
        );
    }

    #[test]
    fn redact_without_pointer_keeps_the_bytes_even_when_they_are_not_json() {
        assert_eq!(redact(b"<html>", &[]).unwrap(), (b"<html>".to_vec(), Vec::new()));
        assert_eq!(
            redact(b"<html>", &pointers(&["/x"])).unwrap_err().to_string(),
            "cannot redact /x: body is not JSON"
        );
    }

    #[test]
    fn redact_refuses_a_duplicate_key_instead_of_leaving_a_copy() {
        let at = pointers(&["/acco/data"]);
        // Le pointeur lit la dernière occurrence ; l'épissure traiterait la première et la
        // valeur resterait en clair dans l'autre (L1) : échec fermé.
        for raw in [
            r#"{"acco":{"data":"X"},"acco":{"data":"X"}}"#,
            r#"{"acco":{"data":"X"},"acco":{"id":1}}"#,
        ] {
            assert_eq!(
                redact(raw.as_bytes(), &at).unwrap_err().to_string(),
                "cannot redact /acco/data: duplicate key",
                "{raw}"
            );
        }
        // Deux valeurs différentes : déjà refusé par le contrôle de place (la première n'est pas
        // celle que lit le pointeur).
        assert_eq!(
            redact(br#"{"acco":{"data":"X"},"acco":{"data":"Y"}}"#, &at).unwrap_err().to_string(),
            "cannot redact /acco/data: value not found in the raw bytes"
        );
        // Sans doublon, rien ne change : le caviardage reste accepté.
        let (file, applied) =
            redact(br#"{"acco":{"data":"X"},"autre":{"data":"X"}}"#, &at).unwrap();
        assert_eq!(file, br#"{"acco":{"data":"<redacted: 1 bytes base64>"},"autre":{"data":"X"}}"#);
        assert_eq!(applied, ["/acco/data"]);
    }

    #[test]
    fn redact_refuses_a_value_it_cannot_locate_in_the_bytes() {
        // serde_json garde la dernière clé en double, la recherche dans les octets la première.
        assert_eq!(
            redact(br#"{"d":"first","d":"second"}"#, &pointers(&["/d"])).unwrap_err().to_string(),
            "cannot redact /d: value not found in the raw bytes"
        );
    }

    #[test]
    fn the_scanner_stops_on_truncated_or_foreign_input() {
        assert_eq!(string_end(b"\"abc", 0), None);
        assert_eq!(string_end(b"\"a\\\"b\"", 0), Some(6));
        assert_eq!(value_end(b"[1,[2]", 0), None);
        assert_eq!(value_end(b"{\"a\":\"}\"}", 0), Some(9));
        assert_eq!(value_end(b"", 0), None);
        assert_eq!(value_end(b"123", 0), Some(3));
        assert_eq!(child(b"3", 0, "x"), None);
        assert_eq!(child(b"[1]", 0, "x"), None);
        assert_eq!(child(b"{\"a\":1", 0, "b"), None);
        assert_eq!(child(b"", 0, "b"), None);
        assert_eq!(skip_blank(b"  ", 5), 5);
        // Branches qu'un JSON valide n'atteint pas : `redact` ne les appelle que sur un corps lu.
        assert_eq!(span(b"{\"a\":1}", &["b".to_owned()]), None);
        assert_eq!(span(b"[", &[]), None);
        assert_eq!(value_end(b"[\"ab", 0), None);
        assert_eq!(child(b"{\"ab", 0, "x"), None);
        assert_eq!(child(b"{\"\\u12\":1}", 0, "x"), None);
        assert_eq!(child(b"{\"a\":[", 0, "x"), None);
    }

    #[test]
    fn a_sidecar_renders_toml_that_reads_back() {
        let sidecar = Sidecar {
            endpoint: "/consult/juri".to_owned(),
            request: "{\"textId\":\"J\\\\1\"}\u{1}".to_owned(),
            env: PisteEnvironment::Sandbox,
            captured_at: "2026-10-28T10:15:00Z".to_owned(),
            http_status: 400,
            content_type: Some("application/json".to_owned()),
            content_encoding: Some("br".to_owned()),
            elapsed_ms: 1234,
            bytes_raw: 87,
            sha256_raw: "aa".to_owned(),
            sha256_file: "bb".to_owned(),
            mojibake_detected: true,
            redactions: pointers(&["/acco/data", "/q\"x"]),
        };
        let text = sidecar.render();
        let table: toml::Table = toml::from_str(&text).unwrap();
        let keys: Vec<&str> = table.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "bytes_raw",
                "captured_at",
                "content_encoding",
                "content_type",
                "elapsed_ms",
                "endpoint",
                "env",
                "http_status",
                "license",
                "mojibake_detected",
                "redactions",
                "request",
                "sha256_file",
                "sha256_raw",
                "tool",
            ]
        );
        let text_of = |key: &str| table[key].as_str().unwrap().to_owned();
        assert_eq!(
            [
                text_of("captured_at"),
                text_of("content_encoding"),
                text_of("env"),
                text_of("license"),
                text_of("tool")
            ],
            ["2026-10-28T10:15:00Z", "br", "sandbox", LICENSE, TOOL]
        );
        assert_eq!(
            (
                table["http_status"].as_integer(),
                table["elapsed_ms"].as_integer(),
                table["bytes_raw"].as_integer()
            ),
            (Some(400), Some(1234), Some(87))
        );
        assert_eq!(table["mojibake_detected"].as_bool(), Some(true));
        let redactions: Vec<&str> = table["redactions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(toml::Value::as_str)
            .collect();
        assert_eq!(redactions, ["/acco/data", "/q\"x"]);
        assert_eq!(table["request"].as_str(), Some(sidecar.request.as_str()));
        assert!(text.starts_with("endpoint = \"/consult/juri\"\nrequest = "), "{text}");
        assert!(text.ends_with(&format!("license = {}\n", toml_string(LICENSE))), "{text}");
        let bare = Sidecar { content_type: None, content_encoding: None, ..sidecar };
        let table: toml::Table = toml::from_str(&bare.render()).unwrap();
        assert_eq!((table.get("content_type"), table.get("content_encoding")), (None, None));
    }

    #[test]
    fn write_records_the_bytes_and_the_sidecar_and_refuses_to_overwrite() {
        let root = scratch("write");
        let probe = probe("/consult/juri", "juri/J1");
        let reply = exchange(200, b"{\"text\":{\"id\":\"J1\"}}");
        let written =
            write(&root, &probe, &reply, PisteEnvironment::Sandbox, "2026-10-28T10:15:00Z", false)
                .unwrap();
        assert_eq!(written.json, root.join("consult/juri/juri/J1.json"));
        assert_eq!((written.bytes_file, written.redactions.len()), (reply.body.len(), 0));
        assert_eq!(fs::read(&written.json).unwrap(), reply.body);
        let meta: toml::Table =
            toml::from_str(&fs::read_to_string(&written.meta).unwrap()).unwrap();
        assert_eq!(meta["sha256_raw"].as_str(), Some(sha256_hex(&reply.body).as_str()));
        assert_eq!(meta["sha256_file"], meta["sha256_raw"]);
        assert_eq!(meta["elapsed_ms"].as_integer(), Some(1234));
        assert_eq!(meta["mojibake_detected"].as_bool(), Some(false));
        let again = write(&root, &probe, &reply, PisteEnvironment::Sandbox, "later", false);
        assert_eq!(
            again.unwrap_err().to_string(),
            format!("{} exists: use --force to overwrite", written.json.display())
        );
        fs::remove_file(&written.json).unwrap();
        // Le sidecar seul suffit à refuser.
        assert_eq!(
            write(&root, &probe, &reply, PisteEnvironment::Sandbox, "later", false)
                .unwrap_err()
                .to_string(),
            format!("{} exists: use --force to overwrite", written.json.display())
        );
        let forced =
            write(&root, &probe, &reply, PisteEnvironment::Production, "later", true).unwrap();
        let meta: toml::Table = toml::from_str(&fs::read_to_string(&forced.meta).unwrap()).unwrap();
        assert_eq!(
            (meta["env"].as_str(), meta["captured_at"].as_str()),
            (Some("production"), Some("later"))
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn write_refuses_an_echo_an_error_status_and_a_bad_probe_without_writing() {
        let root = scratch("refuse");
        let good = probe("/consult/juri", "j");
        let mut echo = exchange(200, b"{}");
        echo.echoes_credentials = true;
        let refusals = [
            write(&root, &good, &echo, PisteEnvironment::Sandbox, "t", true),
            write(&root, &good, &exchange(404, b"{}"), PisteEnvironment::Sandbox, "t", true),
            write(
                &root,
                &probe("x", "j"),
                &exchange(200, b"{}"),
                PisteEnvironment::Sandbox,
                "t",
                true,
            ),
        ];
        let shown: Vec<String> =
            refusals.into_iter().map(|result| result.unwrap_err().to_string()).collect();
        assert_eq!(
            shown,
            [
                "the response echoes a credential: nothing written".to_owned(),
                "HTTP 404 not recorded without --allow-error".to_owned(),
                r#"endpoint must be a path of [A-Za-z0-9_-] segments starting with /: "x""#
                    .to_owned(),
            ]
        );
        assert!(!root.exists());
        let allowed = Probe { allow_error: true, ..good };
        let written =
            write(&root, &allowed, &exchange(404, b""), PisteEnvironment::Sandbox, "t", false)
                .unwrap();
        assert_eq!(fs::read(&written.json).unwrap(), b"");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn write_never_records_authentication_or_rate_limit_replies() {
        // 401 et 429 ne sont pas des réponses de données (jeton refusé, quota atteint) : même avec
        // `allow_error`, rien n'est écrit (I1).
        let root = scratch("never");
        let allowed = Probe { allow_error: true, ..probe("/consult/juri", "j") };
        for status in [401, 429] {
            let error = write(
                &root,
                &allowed,
                &exchange(status, b"{}"),
                PisteEnvironment::Sandbox,
                "t",
                true,
            )
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("HTTP {status} is an authentication or rate-limit reply: never recorded")
            );
        }
        assert!(!root.exists());
        // Les autres refus du serveur restent enregistrables avec `allow_error`.
        for status in [400, 403, 404, 500, 503] {
            let name = format!("s{status}");
            let probe = Probe { allow_error: true, ..probe("/consult/juri", &name) };
            let written = write(
                &root,
                &probe,
                &exchange(status, b"{}"),
                PisteEnvironment::Sandbox,
                "t",
                false,
            )
            .unwrap();
            assert_eq!(fs::read(&written.json).unwrap(), b"{}", "{status}");
        }
        // Sans `allow_error`, 401 est refusé aussi, par le même message.
        let refused = write(
            &root,
            &probe("/consult/juri", "plain"),
            &exchange(401, b"{}"),
            PisteEnvironment::Sandbox,
            "t",
            false,
        );
        assert_eq!(
            refused.unwrap_err().to_string(),
            "HTTP 401 is an authentication or rate-limit reply: never recorded"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// Dossier extérieur à la racine, vide, pour les tests de liens symboliques.
    #[cfg(unix)]
    fn outside(test: &str) -> PathBuf {
        let dir = scratch(&format!("outside-{test}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn write_refuses_a_symbolic_link_at_the_file_or_the_sidecar_even_with_force() {
        use std::os::unix::fs::symlink;
        let reply = exchange(200, b"{}");
        let sandbox = PisteEnvironment::Sandbox;
        // (lien porté par le sidecar ?, cible déjà existante ?, force ?)
        for (on_meta, existing, force) in
            [(false, false, true), (false, true, true), (true, false, true), (true, true, true)]
        {
            let tag = format!("link-{on_meta}-{existing}-{force}");
            let (root, away) = (scratch(&tag), outside(&tag));
            let (json, meta) = fixture_paths(&root, "/consult/juri", "j");
            fs::create_dir_all(json.parent().unwrap()).unwrap();
            let target = away.join("target");
            if existing {
                fs::write(&target, b"keep").unwrap();
            }
            let link = if on_meta { &meta } else { &json };
            symlink(&target, link).unwrap();
            let error = write(&root, &probe("/consult/juri", "j"), &reply, sandbox, "t", force)
                .unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("{} is a symbolic link: refusing to write through it", link.display()),
                "{tag}"
            );
            // Rien n'a été écrit, ni par le lien, ni à côté.
            assert_eq!(target.exists(), existing, "{tag}");
            if existing {
                assert_eq!(fs::read(&target).unwrap(), b"keep", "{tag}");
            }
            let other = if on_meta { &json } else { &meta };
            assert!(!other.exists(), "{tag}");
            fs::remove_dir_all(&root).unwrap();
            fs::remove_dir_all(&away).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_counts_a_dangling_link_as_an_existing_file() {
        use std::os::unix::fs::symlink;
        // `Path::exists` suit le lien et dirait « absent » : sans `--force`, le lien pendant doit
        // être vu par `symlink_metadata` et refusé comme un fichier existant. Avec `--force`, le
        // refus de lien (test précédent) prend le relais.
        let (root, away) = (scratch("dangling"), outside("dangling"));
        let (json, meta) = fixture_paths(&root, "/consult/juri", "j");
        fs::create_dir_all(json.parent().unwrap()).unwrap();
        symlink(away.join("nowhere"), &meta).unwrap();
        let error = write(
            &root,
            &probe("/consult/juri", "j"),
            &exchange(200, b"{}"),
            PisteEnvironment::Sandbox,
            "t",
            false,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{} exists: use --force to overwrite", json.display())
        );
        assert!(!away.join("nowhere").exists());
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&away).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_refuses_a_directory_that_leads_outside_the_root() {
        use std::os::unix::fs::symlink;
        // `<racine>/consult` remplacé par un lien vers l'extérieur : rien n'est créé là-bas.
        let (root, away) = (scratch("escape"), outside("escape"));
        fs::create_dir_all(&root).unwrap();
        symlink(&away, root.join("consult")).unwrap();
        let error = write(
            &root,
            &probe("/consult/juri", "juri/J1"),
            &exchange(200, b"{}"),
            PisteEnvironment::Sandbox,
            "t",
            true,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "{} resolves outside the output directory",
                root.join("consult/juri/juri").display()
            )
        );
        assert_eq!(fs::read_dir(&away).unwrap().count(), 0);
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&away).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_accepts_a_root_that_is_itself_a_link_to_a_directory() {
        use std::os::unix::fs::symlink;
        // Un dossier de sortie donné par un lien reste permis : c'est le choix de l'appelant ;
        // le refus ne vise que ce qui s'échappe de la racine.
        let (real, home) = (outside("rootlink"), scratch("rootlink"));
        symlink(&real, &home).unwrap();
        let written = write(
            &home,
            &probe("/consult/juri", "j"),
            &exchange(200, b"{}"),
            PisteEnvironment::Sandbox,
            "t",
            false,
        )
        .unwrap();
        assert!(real.join("consult/juri/j.json").is_file());
        assert_eq!(written.json, home.join("consult/juri/j.json"));
        fs::remove_file(&home).unwrap();
        fs::remove_dir_all(&real).unwrap();
    }

    #[test]
    fn write_reports_a_root_it_cannot_create() {
        let root = scratch("rootfile");
        fs::write(&root, b"file, not a directory").unwrap();
        let error = write(
            &root,
            &probe("/consult/juri", "j"),
            &exchange(200, b"{}"),
            PisteEnvironment::Sandbox,
            "t",
            false,
        )
        .unwrap_err();
        assert!(
            error.to_string().starts_with(&format!("cannot write {}", root.display())),
            "{error}"
        );
        fs::remove_file(&root).unwrap();
    }

    #[test]
    fn failures_count_the_probes_that_were_not_recorded() {
        use Outcome::{NotRecorded, Recorded};
        assert_eq!(failures(&[]), 0);
        assert_eq!(failures(&[Recorded, Recorded]), 0);
        assert_eq!(failures(&[Recorded, NotRecorded, Recorded, NotRecorded]), 2);
        assert_eq!(failures(&[NotRecorded]), 1);
    }

    #[test]
    fn the_exit_code_follows_the_documented_contract() {
        // 04 §13.4 : 0 tout est enregistré, 1 au moins une sonde non écrite, 2 usage, contrôle,
        // plan illisible ou identifiants absents (le lot n'a pas commencé).
        assert_eq!(exit_code(&Ok(0)), 0);
        assert_eq!(exit_code(&Ok(1)), 1);
        assert_eq!(exit_code(&Ok(40)), 1);
        assert_eq!(exit_code(&Err("set LF_PISTE_CLIENT_ID".to_owned())), 2);
    }

    #[test]
    fn write_redacts_acco_data_and_keeps_the_raw_digest() {
        let root = scratch("acco");
        let raw = br#"{"acco":{"id":"ACCOTEXT1","data":"UEsDBA=="}}"#;
        let written = write(
            &root,
            &probe(ACCO_ENDPOINT, "acco/A1"),
            &exchange(200, raw),
            PisteEnvironment::Sandbox,
            "t",
            false,
        )
        .unwrap();
        let file = fs::read(&written.json).unwrap();
        assert_eq!(file, br#"{"acco":{"id":"ACCOTEXT1","data":"<redacted: 8 bytes base64>"}}"#);
        let meta: toml::Table =
            toml::from_str(&fs::read_to_string(&written.meta).unwrap()).unwrap();
        assert_eq!(meta["sha256_raw"].as_str(), Some(sha256_hex(raw).as_str()));
        assert_eq!(meta["sha256_file"].as_str(), Some(sha256_hex(&file).as_str()));
        assert_eq!(meta["bytes_raw"].as_integer(), Some(45));
        assert_eq!(meta["redactions"].as_array().unwrap().len(), 1);
        let refused = write(
            &root,
            &Probe { redact: pointers(&["/x"]), ..probe("/consult/juri", "html") },
            &exchange(200, b"<html>"),
            PisteEnvironment::Sandbox,
            "t",
            false,
        );
        assert_eq!(refused.unwrap_err().to_string(), "cannot redact /x: body is not JSON");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn write_reports_the_file_it_cannot_write() {
        // Un répertoire à la place du fichier : `--force` passe, l'écriture échoue.
        for (blocked, written) in [("j.json", false), ("j.meta.toml", true)] {
            let root = scratch(blocked);
            fs::create_dir_all(root.join("consult/juri").join(blocked)).unwrap();
            let error = write(
                &root,
                &probe("/consult/juri", "j"),
                &exchange(200, b"{}"),
                PisteEnvironment::Sandbox,
                "t",
                true,
            )
            .unwrap_err();
            let path = root.join("consult/juri").join(blocked);
            assert!(error.to_string().starts_with(&format!("cannot write {}", path.display())));
            assert_eq!(root.join("consult/juri/j.json").is_file(), written, "{blocked}");
            fs::remove_dir_all(&root).unwrap();
        }
    }

    #[test]
    fn write_reports_the_path_it_cannot_create() {
        let root = scratch("io");
        fs::create_dir_all(&root).unwrap();
        let blocker = root.join("consult");
        fs::write(&blocker, b"file, not a directory").unwrap();
        let error = write(
            &root,
            &probe("/consult/juri", "j"),
            &exchange(200, b"{}"),
            PisteEnvironment::Sandbox,
            "t",
            false,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with(&format!("cannot write {}", root.join("consult/juri").display())),
            "{error}"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_plan_reads_its_probes_and_refuses_unknown_keys() {
        let plan: Plan = toml::from_str(
            r#"
            [[probe]]
            endpoint = "/consult/juri"
            body = '{"textId":"JURITEXT000000000000"}'
            name = "inconnu/JURITEXT000000000000"
            allow_error = true

            [[probe]]
            endpoint = "/consult/acco"
            body = '{"id":"ACCOTEXT000053643828"}'
            name = "acco/ACCOTEXT000053643828"
            redact = ["/acco/attachment"]
            "#,
        )
        .unwrap();
        assert_eq!(plan.probe.len(), 2);
        assert!(plan.probe[0].allow_error && plan.probe[0].redact.is_empty());
        assert_eq!(plan.probe[1].redact, ["/acco/attachment"]);
        assert!(
            toml::from_str::<Plan>("[[probe]]\nendpoint='/a'\nbody='{}'\nname='n'\nforce=true")
                .is_err()
        );
        assert_eq!(toml::from_str::<Plan>("").unwrap().probe, Vec::new());
    }

    #[test]
    fn the_shipped_plan_parses_and_every_probe_passes_the_check() {
        // Le lot livré est lu tel quel (CRLF toléré) : aucune sonde ne doit être refusée par
        // `Probe::check` avant le réseau, et les noms doivent rester uniques (un nom = un fichier).
        let text = include_str!("../tests/fixtures/plan.toml").replace("\r\n", "\n");
        let plan: Plan = toml::from_str(&text).unwrap();
        let refused: Vec<&str> = plan
            .probe
            .iter()
            .filter_map(|probe| probe.check().is_err().then_some(probe.name.as_str()))
            .collect();
        assert_eq!(refused, Vec::<&str>::new());
        let names: std::collections::BTreeSet<&str> =
            plan.probe.iter().map(|probe| probe.name.as_str()).collect();
        assert_eq!(names.len(), plan.probe.len(), "duplicate probe names");
        assert_ne!(plan.probe.len(), 0);
    }

    fn parsed(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|arg| (*arg).to_owned())).map_err(|error| error.to_string())
    }

    fn refused(args: &[&str]) -> String {
        parsed(args).unwrap_err()
    }

    #[test]
    fn a_single_probe_is_read_with_its_options() {
        let args = parsed(&[
            "--endpoint",
            "/consult/juri",
            "--body",
            "{}",
            "--name",
            "juri/x",
            "--allow-error",
            "--redact",
            "/a",
            "--redact",
            "/b",
            "--env",
            "production",
            "--force",
            "--out",
            "o",
        ])
        .unwrap();
        let expected = Probe {
            endpoint: "/consult/juri".to_owned(),
            body: "{}".to_owned(),
            name: "juri/x".to_owned(),
            allow_error: true,
            redact: vec!["/a".to_owned(), "/b".to_owned()],
        };
        assert_eq!(args.mode, Mode::Single(expected));
        assert_eq!(args.env, PisteEnvironment::Production);
        assert!(args.force);
        assert_eq!(args.out, Some(PathBuf::from("o")));
    }

    #[test]
    fn a_plan_is_read_with_its_prefix_and_defaults_to_the_sandbox() {
        let args = parsed(&["--plan", "p.toml", "--only", "inconnu/"]).unwrap();
        assert_eq!(
            args.mode,
            Mode::Plan { path: PathBuf::from("p.toml"), only: Some("inconnu/".to_owned()) }
        );
        assert_eq!(args.env, PisteEnvironment::Sandbox);
        assert!(!args.force);
        assert_eq!(args.out, None);
        let whole = parsed(&["--plan", "p.toml", "--env", "sandbox"]).unwrap();
        assert_eq!(whole.mode, Mode::Plan { path: PathBuf::from("p.toml"), only: None });
        assert_eq!(whole.env, PisteEnvironment::Sandbox);
    }

    #[test]
    fn an_unknown_argument_or_a_missing_value_is_refused() {
        assert_eq!(refused(&["--bogus"]), format!("unknown argument \"--bogus\"\n{USAGE}"));
        for flag in
            ["--endpoint", "--body", "--name", "--redact", "--plan", "--only", "--env", "--out"]
        {
            assert_eq!(refused(&[flag]), format!("{flag} needs a value"));
        }
    }

    #[test]
    fn a_repeated_single_valued_argument_is_refused() {
        for (args, flag) in [
            (&["--endpoint", "/a", "--endpoint", "/b"][..], "--endpoint"),
            (&["--body", "{}", "--body", "{}"], "--body"),
            (&["--name", "a", "--name", "b"], "--name"),
            (&["--allow-error", "--allow-error"], "--allow-error"),
            (&["--plan", "a", "--plan", "b"], "--plan"),
            (&["--plan", "a", "--only", "x", "--only", "y"], "--only"),
            (&["--env", "sandbox", "--env", "sandbox"], "--env"),
            (&["--force", "--force"], "--force"),
            (&["--out", "a", "--out", "b"], "--out"),
        ] {
            assert_eq!(refused(args), format!("{flag} given twice"));
        }
        assert!(parsed(&["--redact", "/a", "--redact", "/a"]).is_ok());
    }

    #[test]
    fn only_without_a_plan_is_refused() {
        assert_eq!(
            refused(&["--endpoint", "/consult/juri", "--only", "inconnu/"]),
            "--only needs --plan"
        );
    }

    #[test]
    fn a_plan_mixed_with_single_probe_options_is_refused() {
        for flag in ["--endpoint", "--body", "--name", "--redact"] {
            assert_eq!(
                refused(&[flag, "/x", "--plan", "p.toml"]),
                format!("{flag} cannot be combined with --plan")
            );
        }
        assert_eq!(
            refused(&["--plan", "p.toml", "--allow-error"]),
            "--allow-error cannot be combined with --plan"
        );
    }

    #[test]
    fn an_environment_other_than_sandbox_or_production_is_refused() {
        assert_eq!(
            refused(&["--plan", "p.toml", "--env", "prod"]),
            "--env must be sandbox or production, not \"prod\""
        );
        // Une sonde seule peut viser la production (choix explicite du mainteneur, Q3).
        assert_eq!(
            parsed(&["--endpoint", "/consult/juri", "--env", "production"]).unwrap().env,
            PisteEnvironment::Production
        );
    }

    #[test]
    fn a_plan_never_runs_against_production() {
        // Le lot entier ne se joue qu'en sandbox (L3) ; `--only` n'y change rien.
        for args in [
            &["--plan", "p.toml", "--env", "production"][..],
            &["--plan", "p.toml", "--only", "inconnu/", "--env", "production"],
        ] {
            assert_eq!(refused(args), "--plan runs against the sandbox only");
        }
        assert_eq!(
            parsed(&["--plan", "p.toml", "--env", "sandbox"]).unwrap().env,
            PisteEnvironment::Sandbox
        );
    }

    #[test]
    fn an_unknown_argument_is_echoed_without_a_possible_value() {
        // `--jeton=valeur` : seule la partie avant « = » revient à l'écran, dans l'erreur
        // d'argument inconnu comme dans celle d'argument répété.
        assert_eq!(refused(&["--jeton=abc"]), format!("unknown argument \"--jeton\"\n{USAGE}"));
        assert_eq!(
            refused(&["--jeton=abc", "--jeton=abc"]),
            format!("unknown argument \"--jeton\"\n{USAGE}")
        );
        assert_eq!(refused(&["--force=abc"]), format!("unknown argument \"--force\"\n{USAGE}"));
    }

    #[test]
    fn select_keeps_the_probes_whose_name_starts_with_the_prefix() {
        let probes = vec![
            probe("/consult/juri", "inconnu/A"),
            probe("/consult/juri", "dates/B"),
            probe("/consult/juri", "inconnu/C"),
        ];
        let names = |selected: Vec<Probe>| -> Vec<String> {
            selected.into_iter().map(|probe| probe.name).collect()
        };
        assert_eq!(
            names(select(probes.clone(), Some("inconnu/")).unwrap()),
            ["inconnu/A", "inconnu/C"]
        );
        assert_eq!(
            names(select(probes.clone(), None).unwrap()),
            ["inconnu/A", "dates/B", "inconnu/C"]
        );
        // le préfixe vide retient tout ; le préfixe est un début de nom, pas une sous-chaîne
        assert_eq!(select(probes.clone(), Some("")).unwrap(), probes);
        assert_eq!(
            select(probes, Some("connu/")).unwrap_err().to_string(),
            "no probe selected (name prefix \"connu/\")"
        );
    }

    #[test]
    fn select_refuses_an_empty_selection_before_any_network() {
        assert_eq!(
            select(Vec::new(), None).unwrap_err().to_string(),
            "no probe selected (name prefix \"\")"
        );
        let plan: Plan =
            toml::from_str(&include_str!("../tests/fixtures/plan.toml").replace("\r\n", "\n"))
                .unwrap();
        let typo = select(plan.probe.clone(), Some("inconu/")).unwrap_err();
        assert_eq!(typo.to_string(), "no probe selected (name prefix \"inconu/\")");
        let selected = select(plan.probe.clone(), Some("inconnu/")).unwrap();
        let expected: Vec<Probe> =
            plan.probe.iter().filter(|probe| probe.name.starts_with("inconnu/")).cloned().collect();
        assert_eq!(selected, expected);
        assert_ne!(selected.len(), 0);
    }

    #[test]
    fn debug_of_an_exchange_shows_no_body() {
        let reply = exchange(200, br#"{"secret":"tok-visible"}"#);
        let shown = format!("{reply:?}");
        assert_eq!(
            shown,
            "Exchange { status: 200, content_type: Some(\"application/json\"), content_encoding: None, retry_after: None, elapsed: 1.234s, echoes_credentials: false, body_len: 24 }"
        );
    }
}
