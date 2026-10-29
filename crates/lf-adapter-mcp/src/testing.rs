//! Harnais de test en processus (feature `test-support`, 06 §6.1) : un vrai client rmcp relié au
//! serveur par un tube mémoire (`tokio::io::duplex`), sans processus ni réseau.
//!
//! - [`connect`] sert n'importe quel `ServerHandler` derrière le chemin de production
//!   (`PreLifecycleFilter`, EOF, versions), et rend le client.
//! - [`Fixture`] monte un [`LegifranceServer`] sur les doubles de `lf-application`
//!   ([`FakeCorpus`], [`MemoryAuditLog`], [`FixedClock`]) tout en gardant des poignées dessus : un
//!   test d'outil programme le corpus, appelle l'outil par le client, puis relit les appels du port
//!   et les lignes d'audit.
//! - [`Transcript`] charge une transcription JSON-RPC brute (`tests/jsonrpc/<nom>.jsonl`, 06 §6.1)
//!   et [`replay_in_process`] la rejoue sur un tube mémoire, derrière le même chemin de production
//!   que [`connect`] mais sans client rmcp : le test écrit des lignes brutes et lit les lignes du
//!   serveur, avec les contrôles communs à toutes les transcriptions (C-03, C-26). Le même fichier
//!   est rejoué sur le binaire par `lf-app` (`replay_on_child`, V3-01), qui reprend
//!   [`Transcript`], [`Expect::check`] et [`protocol_violation`].
//!
//! Jamais activée en dépendance normale : `cargo xtask check-arch` (R10) n'admet la feature que
//! dans les `[dev-dependencies]` des autres membres.
#![expect(
    clippy::expect_used,
    reason = "harnais de test : un échec de montage fait tomber le test"
)]

use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use lf_application::{
    audited::{AuditMode, AuditPolicy},
    budget::ResponseBudget,
    ports::{
        ArticleAt, AuditError, AuditLog, AuditRecord, CorpusError, LegalCorpus, PisteEnvironment,
    },
    service::{ConfigurationState, LegalService, ServiceSettings},
    testing::{FakeCorpus, FixedClock, MemoryAuditLog},
};
use lf_domain::{
    date::{CivilDate, VersionDate},
    entity::{
        Act, ArticleText, ArticleVersion, CodeListing, CompanyAgreement, Convention, Decision,
        Deliberation, RelatedLinks, SearchHit, SearchPage,
    },
    id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId},
    search::{NumberQuery, SearchQuery},
};
use rmcp::{RoleClient, ServerHandler, ServiceExt, service::RunningService};
use serde_json::{Map, Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    LegifranceServer,
    lifecycle::serve_io,
    stdio::{ServeError, ServeOutcome},
};

/// Capacité du tube mémoire entre le client et le serveur, en octets.
const PIPE_BYTES: usize = 64 * 1024;

/// Capacité du tube mémoire d'une transcription : large, pour que le serveur n'attende jamais un
/// client qui ne lit qu'aux attentes (`tools/list` pèse plusieurs dizaines de kio).
const RAW_PIPE_BYTES: usize = 1 << 20;

/// Délai d'attente d'un message attendu, ou de l'arrêt du service : au-delà, il est déclaré
/// manquant.
const MESSAGE_WAIT: Duration = Duration::from_secs(5);

/// Durée de l'appel lent d'un amont simulé (`needs: slow_upstream`) : bien plus que n'importe
/// quelle transcription, qui l'annule ou ferme l'entrée avant la fin.
const SLOW_UPSTREAM: Duration = Duration::from_secs(30);

/// Sert `server` sur un tube mémoire, derrière le chemin de production (`serve_io`), et rend le
/// client rmcp connecté (poignée de main faite). Laisser tomber le client ferme le tube : le
/// serveur voit l'EOF et s'arrête.
///
/// # Panics
///
/// Si la poignée de main échoue.
pub async fn connect<S: ServerHandler>(server: S) -> RunningService<RoleClient, ()> {
    let (client_io, server_io) = tokio::io::duplex(PIPE_BYTES);
    let (read, write) = tokio::io::split(server_io);
    tokio::spawn(serve_io(server, read, write, CancellationToken::new()));
    ().serve(client_io).await.expect("the in-process client handshake")
}

/// Corpus partagé : le service possède son corpus, le test en garde une poignée.
pub struct SharedCorpus(pub Arc<FakeCorpus>);

impl LegalCorpus for SharedCorpus {
    async fn search(&self, query: &SearchQuery) -> Result<SearchPage, CorpusError> {
        self.0.search(query).await
    }

    async fn get_act(&self, id: &LegiTextId, date: VersionDate) -> Result<Act, CorpusError> {
        self.0.get_act(id, date).await
    }

    async fn get_article(&self, id: &ArticleId) -> Result<ArticleText, CorpusError> {
        self.0.get_article(id).await
    }

    async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> {
        self.0.get_decision(id).await
    }

    async fn get_deliberation(&self, id: &CnilTextId) -> Result<Deliberation, CorpusError> {
        self.0.get_deliberation(id).await
    }

    async fn get_convention(&self, id: &KaliTextId) -> Result<Convention, CorpusError> {
        self.0.get_convention(id).await
    }

    async fn get_company_agreement(
        &self,
        id: &AccoTextId,
    ) -> Result<CompanyAgreement, CorpusError> {
        self.0.get_company_agreement(id).await
    }

    async fn find_article(
        &self,
        text: &LegiTextId,
        num: &str,
        at: ArticleAt,
    ) -> Result<ArticleText, CorpusError> {
        self.0.find_article(text, num, at).await
    }

    async fn find_by_number(&self, query: &NumberQuery) -> Result<Vec<SearchHit>, CorpusError> {
        self.0.find_by_number(query).await
    }

    async fn list_codes(&self) -> Result<Vec<CodeListing>, CorpusError> {
        self.0.list_codes().await
    }

    async fn article_versions(&self, id: &ArticleId) -> Result<Vec<ArticleVersion>, CorpusError> {
        self.0.article_versions(id).await
    }

    async fn related_links(&self, id: &ArticleId) -> Result<RelatedLinks, CorpusError> {
        self.0.related_links(id).await
    }
}

/// Journal d'audit partagé : le service possède son journal, le test en garde une poignée.
pub struct SharedAudit(pub Arc<MemoryAuditLog>);

impl AuditLog for SharedAudit {
    async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        self.0.record(record).await
    }
}

/// Le serveur monté sur les doubles partagés d'une [`Fixture`].
pub type FixtureServer = LegifranceServer<SharedCorpus, SharedAudit, FixedClock>;

/// Un serveur de test et les poignées sur ce qu'il contient.
///
/// Les champs sont publics : un test change `settings` (mode d'audit, échéance) ou `today` avant
/// d'appeler [`Fixture::server`] ou [`Fixture::connect`].
pub struct Fixture {
    /// Corpus factice du service : programmer les réponses (`push_*`), relire les appels.
    pub corpus: Arc<FakeCorpus>,
    /// Journal d'audit en mémoire du service : relire les lignes (`records`).
    pub audit: Arc<MemoryAuditLog>,
    /// Réglages du service : audit strict, échéance de 60 s, bac à sable, identifiants configurés.
    pub settings: ServiceSettings,
    /// « Aujourd'hui » à Paris pour l'horloge figée : 2026-10-29.
    pub today: CivilDate,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            corpus: Arc::new(FakeCorpus::new()),
            audit: Arc::new(MemoryAuditLog::new()),
            settings: ServiceSettings {
                policy: AuditPolicy {
                    mode: AuditMode::Strict,
                    deadline: Duration::from_secs(60),
                    env: PisteEnvironment::Sandbox,
                    server_version: env!("CARGO_PKG_VERSION"),
                },
                configuration: ConfigurationState {
                    credentials_configured: true,
                    ..ConfigurationState::default()
                },
                budget: ResponseBudget,
            },
            today: CivilDate::new(2026, 10, 29).expect("a valid civil date"),
        }
    }
}

impl Fixture {
    /// Fixture dont chaque appel au corpus dure 30 s : l'amont lent que demande une transcription
    /// (`needs: slow_upstream`), pour qu'un appel reste en vol le temps de l'annuler ou de fermer
    /// l'entrée.
    #[must_use]
    pub fn slow_upstream() -> Self {
        Self { corpus: Arc::new(FakeCorpus::new().with_delay(SLOW_UPSTREAM)), ..Self::default() }
    }

    /// La fixture dont a besoin `transcript` : l'amont lent s'il le demande, sinon celle par défaut.
    #[must_use]
    pub fn for_transcript(transcript: &Transcript) -> Self {
        if transcript.needs(Need::SlowUpstream) { Self::slow_upstream() } else { Self::default() }
    }

    /// Un serveur neuf sur les doubles de la fixture (aucun routeur d'outil ne le remplit encore :
    /// les tranches y ajoutent leurs outils).
    #[must_use]
    pub fn server(&self) -> FixtureServer {
        #[expect(
            clippy::default_trait_access,
            reason = "chrono n'est pas une dépendance de ce crate : l'instant par défaut est l'époque Unix"
        )]
        let now = Default::default();
        let clock = FixedClock::new(now, self.today);
        let service = LegalService::new(
            SharedCorpus(Arc::clone(&self.corpus)),
            SharedAudit(Arc::clone(&self.audit)),
            clock,
            self.settings.clone(),
        );
        LegifranceServer::new(Arc::new(service))
    }

    /// [`Fixture::server`] servi par [`connect`] : le client rmcp connecté.
    pub async fn connect(&self) -> RunningService<RoleClient, ()> {
        connect(self.server()).await
    }
}

// ---- transcriptions JSON-RPC brutes (06 §6.1) -------------------------------------------------

/// Dossier des transcriptions versionnées de ce crate (`tests/jsonrpc/`, un cas par fichier
/// `<nom>.jsonl`, sans préfixe).
#[must_use]
pub fn transcripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("jsonrpc")
}

/// Codes d'erreur JSON-RPC que le serveur peut émettre (annexe D §3.5, C-26) : les codes standard,
/// `-32022` (version non supportée) et `-32002` (ressource introuvable, pairs legacy seulement :
/// M7). Aucun code maison, jamais `-32042`.
const ERROR_CODES: [i64; 7] = [-32700, -32600, -32601, -32602, -32603, -32022, -32002];

/// Mots-clés d'une ligne qui accompagnent `expect` et ne sont pas une instruction.
const EXPECT_COMPANIONS: [&str; 4] = ["mode", "absent", "snapshot", "redact"];

/// Ce qu'une transcription exige de son environnement, que chaque exécuteur satisfait à sa façon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Un appel à l'amont (PISTE) qui dure bien plus que la transcription : en processus, un
    /// `FakeCorpus` à 30 s ([`Fixture::slow_upstream`]) ; sur le binaire, un faux PISTE lent.
    SlowUpstream,
}

/// Comparaison d'un message attendu au message reçu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Chaque clé attendue existe, avec la même valeur ; les tableaux gardent leur longueur.
    Subset,
    /// Égalité complète.
    Exact,
}

/// Un message du serveur attendu (instruction `expect`).
#[derive(Debug, Clone, PartialEq)]
pub struct Expect {
    /// Le message attendu, tout ou partie selon [`Expect::mode`]. Quand il porte un `id`, il est
    /// apparié par `id` aux messages du serveur ; sinon, c'est le prochain message.
    pub message: Value,
    /// Comparaison du message attendu au message reçu.
    pub mode: Mode,
    /// Pointeurs JSON qui ne doivent pas exister dans le message reçu.
    pub absent: Vec<String>,
    /// Nom d'un instantané `insta` du message reçu (voir [`Replay::snapshots`]).
    pub snapshot: Option<String>,
    /// Pointeurs JSON dont la valeur est remplacée par `"[redacted]"` dans l'instantané.
    pub redact: Vec<String>,
}

impl Expect {
    /// L'`id` du message attendu, quand il en porte un.
    #[must_use]
    pub fn id(&self) -> Option<&Value> {
        self.message.get("id")
    }

    /// Compare `actual` au message attendu, puis vérifie les pointeurs `absent`.
    ///
    /// # Errors
    ///
    /// Le premier écart, avec son pointeur JSON.
    pub fn check(&self, actual: &Value) -> Result<(), String> {
        match self.mode {
            Mode::Subset => subset("", &self.message, actual)?,
            Mode::Exact if self.message == *actual => {}
            Mode::Exact => return Err(format!("expected exactly {}, got {actual}", self.message)),
        }
        match self.absent.iter().find(|pointer| actual.pointer(pointer).is_some()) {
            Some(pointer) => Err(format!("{pointer} must be absent, got {actual}")),
            None => Ok(()),
        }
    }

    /// L'instantané de `actual` à comparer, si l'instruction en demande un : version du serveur et
    /// pointeurs `redact` masqués.
    #[must_use]
    pub fn capture(&self, actual: &Value) -> Option<Capture> {
        let name = self.snapshot.clone()?;
        let mut message = actual.clone();
        for pointer in &self.redact {
            if let Some(slot) = message.pointer_mut(pointer) {
                *slot = Value::from("[redacted]");
            }
        }
        redact_server_version(&mut message);
        Some(Capture { name, message })
    }
}

/// Instantané demandé par une instruction `expect`.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    /// Nom de l'instantané.
    pub name: String,
    /// Le message reçu, version du serveur et pointeurs `redact` masqués.
    pub message: Value,
}

/// Une instruction d'une transcription.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// `{"comment": "…"}` : une phrase pour le lecteur.
    Comment(String),
    /// `{"needs": "slow_upstream"}` : ce que l'exécuteur doit fournir.
    Needs(Need),
    /// `{"binary_only": "raison"}` : la transcription n'est rejouée que sur le binaire.
    BinaryOnly(String),
    /// `{"send": {…}}` : écrit l'objet sur une ligne.
    Send(Value),
    /// `{"send_raw": "…"}` : écrit la chaîne telle quelle (ligne non JSON, tableau).
    SendRaw(String),
    /// `{"expect": {…}, "mode": …, "absent": […], "snapshot": …, "redact": […]}`.
    Expect(Expect),
    /// `{"expect_silence_ms": 500}` : aucune ligne pendant la durée donnée.
    ExpectSilence(Duration),
    /// `{"expect_alive": true}` : `server/discover` reçoit une réponse, le service n'est pas mort.
    ExpectAlive,
    /// `{"close_stdin": true}` : ferme l'entrée du serveur (EOF).
    CloseStdin,
    /// `{"expect_exit": {"code": 0, "within_ms": 1000}}` : code et délai de sortie du service.
    ExpectExit {
        /// Code de sortie du processus : 0 pour une fin normale, 1 pour un échec.
        code: i32,
        /// Délai maximal entre la demande et la sortie.
        within: Duration,
    },
}

/// Une instruction et sa ligne dans le fichier.
#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    /// Numéro de ligne, à partir de 1.
    pub line: usize,
    /// L'instruction.
    pub step: Step,
}

/// Échec du chargement d'une transcription.
#[derive(Debug, Error)]
pub enum TranscriptError {
    /// Fichier ou dossier illisible.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// Chemin lu.
        path: PathBuf,
        /// Cause.
        source: io::Error,
    },
    /// Ligne qui n'est pas une instruction valide.
    #[error("{name}.jsonl line {line}: {reason}")]
    Line {
        /// Nom de la transcription.
        name: String,
        /// Numéro de ligne, à partir de 1.
        line: usize,
        /// Ce qui est invalide.
        reason: String,
    },
}

/// Une transcription JSON-RPC brute : une instruction par ligne, un cas par fichier (06 §6.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    name: String,
    steps: Vec<Instruction>,
}

impl Transcript {
    /// Charge `tests/jsonrpc/<nom>.jsonl` ; la transcription porte le nom du fichier, sans
    /// extension.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Io`] si le fichier est illisible, [`TranscriptError::Line`] si une ligne
    /// n'est pas une instruction valide.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, TranscriptError> {
        Self::read(path.as_ref())
    }

    fn read(path: &Path) -> Result<Self, TranscriptError> {
        let text = fs::read_to_string(path)
            .map_err(|source| TranscriptError::Io { path: path.to_path_buf(), source })?;
        let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
        Self::parse(&name.unwrap_or_default(), &text)
    }

    /// Charge toutes les transcriptions `*.jsonl` de `dir`, triées par nom.
    ///
    /// # Errors
    ///
    /// Comme [`Transcript::load`], plus [`TranscriptError::Io`] si le dossier est illisible.
    pub fn load_all(dir: impl AsRef<Path>) -> Result<Vec<Self>, TranscriptError> {
        let dir = dir.as_ref();
        let entries = fs::read_dir(dir)
            .map_err(|source| TranscriptError::Io { path: dir.to_path_buf(), source })?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
            .collect();
        paths.sort();
        paths.iter().map(|path| Self::read(path)).collect()
    }

    /// Analyse le texte d'une transcription nommée `name`. Les lignes vides sont ignorées.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Line`] si une ligne n'est pas une instruction valide.
    pub fn parse(name: &str, text: &str) -> Result<Self, TranscriptError> {
        let mut steps = Vec::new();
        for (index, text) in text.lines().enumerate().filter(|(_, text)| !text.trim().is_empty()) {
            let step = serde_json::from_str::<Value>(text)
                .map_err(|error| error.to_string())
                .and_then(|line| parse_step(&line))
                .map_err(|reason| TranscriptError::Line {
                    name: name.to_owned(),
                    line: index + 1,
                    reason,
                })?;
            steps.push(Instruction { line: index + 1, step });
        }
        Ok(Self { name: name.to_owned(), steps })
    }

    /// Le nom du cas, celui du fichier sans extension.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Les instructions, dans l'ordre du fichier.
    #[must_use]
    pub fn steps(&self) -> &[Instruction] {
        &self.steps
    }

    /// La raison pour laquelle la transcription n'est rejouée que sur le binaire, s'il y en a une.
    #[must_use]
    pub fn binary_only(&self) -> Option<&str> {
        self.steps.iter().find_map(|instruction| match &instruction.step {
            Step::BinaryOnly(reason) => Some(reason.as_str()),
            _ => None,
        })
    }

    /// Vrai si la transcription exige `need` de son environnement.
    #[must_use]
    pub fn needs(&self, need: Need) -> bool {
        self.steps.iter().any(|instruction| instruction.step == Step::Needs(need))
    }
}

/// Analyse une ligne (déjà décodée) en instruction : exactement un mot-clé d'instruction, plus les
/// mots-clés qui accompagnent `expect`.
fn parse_step(line: &Value) -> Result<Step, String> {
    let object = line.as_object().ok_or("a line must be a JSON object")?;
    let mut keys = object.keys().filter(|key| !EXPECT_COMPANIONS.contains(&key.as_str()));
    let (Some(key), None) = (keys.next(), keys.next()) else {
        return Err("a line holds exactly one instruction".to_owned());
    };
    let body = &object[key];
    if key != "expect"
        && let Some(extra) = EXPECT_COMPANIONS.iter().find(|word| object.contains_key(**word))
    {
        return Err(format!("`{extra}` only goes with `expect`"));
    }
    let text = || body.as_str().map(str::to_owned).ok_or_else(|| format!("`{key}` takes a string"));
    let flag = |step: Step| {
        (*body == Value::Bool(true)).then_some(step).ok_or_else(|| format!("`{key}` must be true"))
    };
    match key.as_str() {
        "comment" => text().map(Step::Comment),
        "binary_only" => text().map(Step::BinaryOnly),
        "send_raw" => text().map(Step::SendRaw),
        "needs" => match body.as_str() {
            Some("slow_upstream") => Ok(Step::Needs(Need::SlowUpstream)),
            _ => Err(format!("unknown need {body}")),
        },
        "send" if body.is_object() => Ok(Step::Send(body.clone())),
        "send" => Err("`send` takes a JSON object".to_owned()),
        "expect" => parse_expect(object, body),
        "expect_silence_ms" => body
            .as_u64()
            .map(|ms| Step::ExpectSilence(Duration::from_millis(ms)))
            .ok_or_else(|| "`expect_silence_ms` takes a number of milliseconds".to_owned()),
        "expect_alive" => flag(Step::ExpectAlive),
        "close_stdin" => flag(Step::CloseStdin),
        "expect_exit" => {
            let code = body.get("code").and_then(Value::as_i64).and_then(|c| i32::try_from(c).ok());
            let within = body.get("within_ms").and_then(Value::as_u64).map(Duration::from_millis);
            code.zip(within)
                .map(|(code, within)| Step::ExpectExit { code, within })
                .ok_or_else(|| r#"`expect_exit` takes {"code": 0, "within_ms": 1000}"#.to_owned())
        }
        other => Err(format!("unknown instruction `{other}`")),
    }
}

/// Analyse une instruction `expect` et ses mots-clés `mode`, `absent`, `snapshot` et `redact`.
fn parse_expect(object: &Map<String, Value>, body: &Value) -> Result<Step, String> {
    if !body.is_object() {
        return Err("`expect` takes a JSON object".to_owned());
    }
    let mode = match object.get("mode").map(Value::as_str) {
        None | Some(Some("subset")) => Mode::Subset,
        Some(Some("exact")) => Mode::Exact,
        Some(_) => return Err(r#"`mode` is "subset" or "exact""#.to_owned()),
    };
    let strings = |word: &str| -> Result<Vec<String>, String> {
        object.get(word).map_or(Ok(Vec::new()), |value| {
            let items = value.as_array().map(|items| items.iter().map(Value::as_str));
            items
                .and_then(|items| items.map(|item| item.map(str::to_owned)).collect())
                .ok_or_else(|| format!("`{word}` takes an array of strings"))
        })
    };
    let snapshot = match object.get("snapshot").map(Value::as_str) {
        None => None,
        Some(Some(name)) => Some(name.to_owned()),
        Some(None) => return Err("`snapshot` takes a string".to_owned()),
    };
    Ok(Step::Expect(Expect {
        message: body.clone(),
        mode,
        absent: strings("absent")?,
        snapshot,
        redact: strings("redact")?,
    }))
}

/// Vérifie que chaque clé de `expected` existe dans `actual` avec la même valeur ; les tableaux
/// gardent leur longueur. L'erreur porte le pointeur JSON de l'écart.
fn subset(path: &str, expected: &Value, actual: &Value) -> Result<(), String> {
    match (expected, actual) {
        (Value::Object(want), Value::Object(have)) => want.iter().try_for_each(|(key, want)| {
            let here = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
            let have = have.get(key).ok_or_else(|| format!("{here}: missing in {actual}"))?;
            subset(&here, want, have)
        }),
        (Value::Array(want), Value::Array(have)) if want.len() == have.len() => {
            want.iter().zip(have).enumerate().try_for_each(|(index, (want, have))| {
                subset(&format!("{path}/{index}"), want, have)
            })
        }
        _ if expected == actual => Ok(()),
        _ => Err(format!("{path}: expected {expected}, got {actual}")),
    }
}

/// Masque la version du serveur (`mcp-legifrance` et sa `version`, où qu'elle figure) : un
/// instantané ne change pas à chaque version du crate.
pub fn redact_server_version(message: &mut Value) {
    match message {
        Value::Object(map) => {
            if map.get("name") == Some(&Value::from("mcp-legifrance"))
                && let Some(version) = map.get_mut("version")
            {
                *version = Value::from("[version]");
            }
            map.values_mut().for_each(redact_server_version);
        }
        Value::Array(items) => items.iter_mut().for_each(redact_server_version),
        _ => {}
    }
}

/// Les contrôles communs à toutes les transcriptions sur un message du serveur : un message
/// JSON-RPC 2.0 (C-01), jamais une requête du serveur au client, c'est-à-dire ni `method` avec
/// `id` (C-03), aucun code d'erreur maison et une réponse qui reprend l'`id` d'une requête envoyée
/// (C-26). `sent_ids` sont les `id` des requêtes envoyées jusque-là. Rend la violation, s'il y en
/// a une.
#[must_use]
pub fn protocol_violation(message: &Value, sent_ids: &[Value]) -> Option<String> {
    let Some(object) = message.as_object().filter(|o| o.get("jsonrpc") == Some(&json!("2.0")))
    else {
        return Some(format!("not a JSON-RPC 2.0 message: {message}"));
    };
    let id = object.get("id");
    if id.is_some() && object.contains_key("method") {
        return Some(format!("a server request is forbidden (C-03): {message}"));
    }
    if let Some(error) = object.get("error") {
        let code = error.get("code").and_then(Value::as_i64);
        if !code.is_some_and(|code| ERROR_CODES.contains(&code)) {
            return Some(format!("an error code of our own is forbidden (C-26): {message}"));
        }
    }
    id.filter(|id| !id.is_null() && !sent_ids.contains(id))
        .map(|_| format!("the response does not carry the id of a request (C-26): {message}"))
}

/// Ce que `replay_in_process` rend d'une transcription rejouée sans écart.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Replay {
    /// Les instantanés demandés par les instructions `expect`, dans l'ordre : à comparer par
    /// l'appelant (`insta`, dev-dépendance qui n'est pas celle de ce module).
    pub snapshots: Vec<Capture>,
    /// Tous les messages reçus du serveur, dans l'ordre d'arrivée.
    pub received: Vec<Value>,
}

/// Écart entre une transcription et le serveur, ou transcription non rejouable ici.
#[derive(Debug, Error)]
#[error("{transcript}.jsonl line {line}: {message}")]
pub struct ReplayError {
    /// Nom de la transcription.
    pub transcript: String,
    /// Ligne de l'instruction en échec ; 0 hors de toute instruction.
    pub line: usize,
    /// L'écart.
    pub message: String,
}

impl ReplayError {
    fn at(transcript: &Transcript, line: usize, message: String) -> Self {
        Self { transcript: transcript.name.clone(), line, message }
    }
}

/// Code de sortie qu'aurait le processus pour cette fin de service : 0 pour une fin normale (EOF
/// ou arrêt), 1 pour un échec (annexe D §4.3).
fn exit_code(joined: &Result<Result<ServeOutcome, ServeError>, tokio::task::JoinError>) -> i32 {
    i32::from(!matches!(joined, Ok(Ok(_))))
}

/// Ce que rend une lecture de la sortie du serveur.
enum Read {
    /// Une ligne, décodée (une chaîne si elle n'est pas du JSON).
    Message(Value),
    /// Rien n'est arrivé pendant la durée donnée.
    Silence,
    /// La sortie du serveur est fermée.
    Eof,
}

/// Une transcription en cours : le client brut d'un serveur servi derrière le chemin de production.
struct Run {
    write: Option<WriteHalf<DuplexStream>>,
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    server: Option<JoinHandle<Result<ServeOutcome, ServeError>>>,
    /// Messages lus en cherchant un `id`, pas encore attendus.
    pending: VecDeque<Value>,
    /// `id` des requêtes envoyées.
    sent_ids: Vec<Value>,
    /// Nombre de sondes `expect_alive` envoyées.
    probes: u32,
    replay: Replay,
}

impl Run {
    fn start<S: ServerHandler>(server: S) -> Self {
        let (client, server_io) = tokio::io::duplex(RAW_PIPE_BYTES);
        let (server_read, server_write) = tokio::io::split(server_io);
        let server =
            tokio::spawn(serve_io(server, server_read, server_write, CancellationToken::new()));
        let (read, write) = tokio::io::split(client);
        Self {
            write: Some(write),
            lines: BufReader::new(read).lines(),
            server: Some(server),
            pending: VecDeque::new(),
            sent_ids: Vec::new(),
            probes: 0,
            replay: Replay::default(),
        }
    }

    /// Écrit une ligne. Un échec d'écriture est la fermeture de l'entrée ou la fin du service :
    /// l'attente suivante le signale.
    async fn write_line(&mut self, text: &str) -> Result<(), String> {
        let write = self.write.as_mut().ok_or("stdin is closed")?;
        let line = format!("{text}\n");
        write.write_all(line.as_bytes()).await.ok();
        write.flush().await.ok();
        Ok(())
    }

    async fn send(&mut self, message: &Value) -> Result<(), String> {
        if message.get("method").is_some() {
            self.sent_ids.extend(message.get("id").cloned());
        }
        self.write_line(&message.to_string()).await
    }

    /// Prochaine ligne du serveur, ou le silence ou l'EOF. Une erreur de lecture vaut EOF.
    async fn read(&mut self, wait: Duration) -> Result<Read, String> {
        let line = match tokio::time::timeout(wait, self.lines.next_line()).await {
            Err(_) => return Ok(Read::Silence),
            Ok(line) => line.ok().flatten(),
        };
        let Some(line) = line else { return Ok(Read::Eof) };
        let message = serde_json::from_str(&line).unwrap_or(Value::String(line));
        if let Some(violation) = protocol_violation(&message, &self.sent_ids) {
            return Err(violation);
        }
        self.replay.received.push(message.clone());
        Ok(Read::Message(message))
    }

    /// Un message du serveur, au plus [`MESSAGE_WAIT`] d'attente.
    async fn read_message(&mut self) -> Result<Value, String> {
        match self.read(MESSAGE_WAIT).await? {
            Read::Message(message) => Ok(message),
            Read::Silence => Err("no message from the server within 5 s".to_owned()),
            Read::Eof => Err("the server closed its output".to_owned()),
        }
    }

    /// Le prochain message : un message passé en cherchant un `id`, sinon le suivant du serveur.
    async fn next_message(&mut self) -> Result<Value, String> {
        match self.pending.pop_front() {
            Some(message) => Ok(message),
            None => self.read_message().await,
        }
    }

    /// Le message qui porte `id` : parmi ceux déjà lus, sinon en lisant. Les autres restent en
    /// attente pour les instructions suivantes.
    async fn message_with_id(&mut self, id: &Value) -> Result<Value, String> {
        loop {
            let found = self.pending.iter().position(|message| message.get("id") == Some(id));
            if let Some(message) = found.and_then(|position| self.pending.remove(position)) {
                return Ok(message);
            }
            let message =
                self.read_message().await.map_err(|error| format!("{error} (id {id})"))?;
            self.pending.push_back(message);
        }
    }

    /// Attend la fin du service ; rend son code de sortie.
    async fn exit(&mut self, within: Duration) -> Result<i32, String> {
        let server = self.server.take().ok_or("the service already exited")?;
        let joined = tokio::time::timeout(within, server)
            .await
            .map_err(|_| format!("the service did not exit within {within:?}"))?;
        Ok(exit_code(&joined))
    }

    async fn apply(&mut self, step: &Step) -> Result<(), String> {
        match step {
            Step::Comment(_) | Step::Needs(_) | Step::BinaryOnly(_) => Ok(()),
            Step::Send(message) => self.send(message).await,
            Step::SendRaw(text) => {
                if let Ok(message) = serde_json::from_str::<Value>(text) {
                    self.sent_ids.extend(message.get("id").cloned());
                }
                self.write_line(text).await
            }
            Step::Expect(expect) => {
                let message = match expect.id() {
                    Some(id) => self.message_with_id(id).await?,
                    None => self.next_message().await?,
                };
                expect.check(&message)?;
                self.replay.snapshots.extend(expect.capture(&message));
                Ok(())
            }
            Step::ExpectSilence(wait) => {
                if let Some(stray) = self.pending.front() {
                    return Err(format!("a message arrived before the silence: {stray}"));
                }
                match self.read(*wait).await? {
                    Read::Silence => Ok(()),
                    Read::Message(message) => {
                        Err(format!("a message broke the silence: {message}"))
                    }
                    Read::Eof => Err("the server closed its output during the silence".to_owned()),
                }
            }
            Step::ExpectAlive => {
                self.probes += 1;
                let id = json!(format!("alive-{}", self.probes));
                let probe = json!({"jsonrpc": "2.0", "id": id, "method": "server/discover",
                "params": {"_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {}
                }}});
                self.send(&probe).await?;
                let reply = self.message_with_id(&id).await?;
                if reply.get("result").is_some() {
                    Ok(())
                } else {
                    Err(format!("server/discover failed, the service is not alive: {reply}"))
                }
            }
            Step::CloseStdin => {
                if let Some(mut write) = self.write.take() {
                    write.shutdown().await.ok();
                }
                Ok(())
            }
            Step::ExpectExit { code, within } => {
                let actual = self.exit(*within).await?;
                if actual == *code {
                    Ok(())
                } else {
                    Err(format!("the service exited with code {actual}, expected {code}"))
                }
            }
        }
    }

    /// Fin de la transcription : ferme l'entrée, attend la fin du service (code 0), puis exige que
    /// le serveur n'ait rien dit de plus que ce qui a été attendu.
    async fn finish(&mut self) -> Result<(), String> {
        self.apply(&Step::CloseStdin).await?;
        if self.server.is_some() {
            self.apply(&Step::ExpectExit { code: 0, within: MESSAGE_WAIT }).await?;
        }
        match self.pending.pop_front() {
            Some(stray) => Err(format!("the server said more than expected: {stray}")),
            None => match self.read(MESSAGE_WAIT).await? {
                Read::Message(stray) => Err(format!("the server said more than expected: {stray}")),
                Read::Silence | Read::Eof => Ok(()),
            },
        }
    }
}

/// Rejoue `transcript` sur `server`, servi sur un tube mémoire derrière le chemin de production
/// (`PreLifecycleFilter`, EOF), et rend les instantanés demandés et les messages reçus.
///
/// Chaque message du serveur passe les contrôles communs ([`protocol_violation`]). À la fin, le
/// serveur n'a rien dit de plus que ce que la transcription attendait, et s'arrête sur l'EOF.
///
/// # Errors
///
/// [`ReplayError`] au premier écart, avec la ligne de l'instruction ; ou si la transcription n'est
/// rejouée que sur le binaire ([`Transcript::binary_only`]).
pub async fn replay_in_process<S: ServerHandler>(
    server: S,
    transcript: &Transcript,
) -> Result<Replay, ReplayError> {
    if let Some(reason) = transcript.binary_only() {
        let message = format!("replayed on the binary only: {reason}");
        return Err(ReplayError::at(transcript, 0, message));
    }
    replay(Run::start(server), transcript).await
}

/// Le corps de [`replay_in_process`], hors de toute généricité : un seul exemplaire à couvrir.
async fn replay(mut run: Run, transcript: &Transcript) -> Result<Replay, ReplayError> {
    for Instruction { line, step } in &transcript.steps {
        run.apply(step).await.map_err(|message| ReplayError::at(transcript, *line, message))?;
    }
    let last = transcript.steps.last().map_or(0, |instruction| instruction.line);
    run.finish().await.map_err(|message| ReplayError::at(transcript, last, message))?;
    Ok(run.replay)
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, num::NonZeroU32};

    use lf_application::{
        input::CanonicalInput, ports::AuditStatus, testing::PortRequest, tools::ToolName,
    };
    use lf_domain::{
        fond::Fond,
        search::{MatchMode, NumberKind, NumberValue, PageSize, QueryText},
    };
    use pretty_assertions::assert_eq;
    use rmcp::{
        ErrorData, RoleServer,
        model::{
            CallToolRequestParams, DiscoverResult, ErrorCode, Implementation, ProtocolVersion,
            ServerCapabilities, ServerConfig, SubscriptionFilter,
        },
        service::RequestContext,
    };

    use super::*;

    // ---- transcriptions : lecture du format --------------------------------------------------

    fn parse_error(text: &str) -> String {
        Transcript::parse("inline", text).unwrap_err().to_string()
    }

    #[test]
    fn every_instruction_form_parses_with_its_line_number() {
        let text = [
            r#"{"comment":"une phrase"}"#,
            "",
            r#"{"needs":"slow_upstream"}"#,
            r#"{"binary_only":"V-35"}"#,
            r#"{"send":{"jsonrpc":"2.0","id":1,"method":"ping"}}"#,
            r#"{"send_raw":"[]"}"#,
            r#"{"expect":{"id":1},"mode":"exact","absent":["/result"],"snapshot":"s","redact":["/a"]}"#,
            r#"{"expect":{"id":2}}"#,
            r#"{"expect_silence_ms":500}"#,
            r#"{"expect_alive":true}"#,
            r#"{"close_stdin":true}"#,
            r#"{"expect_exit":{"code":0,"within_ms":1000}}"#,
        ]
        .join("\n");
        let transcript = Transcript::parse("forms", &text).unwrap();
        assert_eq!(transcript.name(), "forms");
        let lines: Vec<usize> = transcript.steps().iter().map(|i| i.line).collect();
        // La ligne vide compte dans la numérotation, sans produire d'instruction.
        assert_eq!(lines, [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let steps: Vec<&Step> = transcript.steps().iter().map(|i| &i.step).collect();
        assert_eq!(steps[0], &Step::Comment("une phrase".to_owned()));
        assert_eq!(steps[1], &Step::Needs(Need::SlowUpstream));
        assert_eq!(steps[2], &Step::BinaryOnly("V-35".to_owned()));
        assert_eq!(steps[3], &Step::Send(json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})));
        assert_eq!(steps[4], &Step::SendRaw("[]".to_owned()));
        assert_eq!(
            steps[5],
            &Step::Expect(Expect {
                message: json!({"id": 1}),
                mode: Mode::Exact,
                absent: vec!["/result".to_owned()],
                snapshot: Some("s".to_owned()),
                redact: vec!["/a".to_owned()],
            })
        );
        assert_eq!(
            steps[6],
            &Step::Expect(Expect {
                message: json!({"id": 2}),
                mode: Mode::Subset,
                absent: vec![],
                snapshot: None,
                redact: vec![],
            })
        );
        assert_eq!(steps[7], &Step::ExpectSilence(Duration::from_millis(500)));
        assert_eq!(steps[8], &Step::ExpectAlive);
        assert_eq!(steps[9], &Step::CloseStdin);
        assert_eq!(steps[10], &Step::ExpectExit { code: 0, within: Duration::from_millis(1000) });
        assert_eq!(transcript.binary_only(), Some("V-35"));
        assert!(transcript.needs(Need::SlowUpstream));
        let plain = Transcript::parse("plain", r#"{"close_stdin":true}"#).unwrap();
        assert_eq!(plain.binary_only(), None);
        assert!(!plain.needs(Need::SlowUpstream));
    }

    #[test]
    fn an_invalid_line_is_rejected_with_the_name_the_line_and_the_reason() {
        let cases = [
            ("not json", "expected ident"),
            ("[]", "a line must be a JSON object"),
            ("{}", "exactly one instruction"),
            (r#"{"comment":"a","send":{}}"#, "exactly one instruction"),
            (r#"{"comment":"a","mode":"exact"}"#, "`mode` only goes with `expect`"),
            (r#"{"comment":5}"#, "`comment` takes a string"),
            (r#"{"needs":"unicorns"}"#, r#"unknown need "unicorns""#),
            (r#"{"send":"x"}"#, "`send` takes a JSON object"),
            (r#"{"expect":[]}"#, "`expect` takes a JSON object"),
            (r#"{"expect":{},"mode":"fuzzy"}"#, r#"`mode` is "subset" or "exact""#),
            (r#"{"expect":{},"mode":5}"#, r#"`mode` is "subset" or "exact""#),
            (r#"{"expect":{},"absent":"x"}"#, "`absent` takes an array of strings"),
            (r#"{"expect":{},"redact":[1]}"#, "`redact` takes an array of strings"),
            (r#"{"expect":{},"snapshot":5}"#, "`snapshot` takes a string"),
            (r#"{"expect_silence_ms":"x"}"#, "`expect_silence_ms` takes a number"),
            (r#"{"expect_alive":false}"#, "`expect_alive` must be true"),
            (r#"{"expect_exit":{"code":0}}"#, "`expect_exit` takes"),
            (r#"{"expect_exit":{"code":"0","within_ms":1}}"#, "`expect_exit` takes"),
            (r#"{"frobnicate":1}"#, "unknown instruction `frobnicate`"),
        ];
        for (line, reason) in cases {
            let error = parse_error(&format!("{{\"comment\":\"ok\"}}\n\n{line}"));
            assert!(error.starts_with("inline.jsonl line 3: "), "{line}: {error}");
            assert!(error.contains(reason), "{line}: {error}");
        }
    }

    #[test]
    fn a_transcript_loads_from_a_file_and_takes_the_name_of_the_file() {
        let transcript =
            Transcript::load(transcripts_dir().join("eof_after_discover.jsonl")).unwrap();
        assert_eq!(transcript.name(), "eof_after_discover");
        assert!(transcript.steps().len() > 3);
        let missing = Transcript::load(transcripts_dir().join("absent.jsonl")).unwrap_err();
        assert!(matches!(missing, TranscriptError::Io { .. }), "{missing}");
        assert!(missing.to_string().starts_with("cannot read "), "{missing}");
        let unreadable_dir = Transcript::load_all(transcripts_dir().join("absent")).unwrap_err();
        assert!(matches!(unreadable_dir, TranscriptError::Io { .. }), "{unreadable_dir}");
    }

    // ---- transcriptions : comparaison, instantanés et contrôles communs ------------------------

    fn expect(message: &Value, mode: Mode, absent: &[&str]) -> Expect {
        Expect {
            message: message.clone(),
            mode,
            absent: absent.iter().map(|pointer| (*pointer).to_owned()).collect(),
            snapshot: None,
            redact: vec![],
        }
    }

    #[test]
    fn subset_compares_keys_and_whole_arrays_and_exact_compares_everything() {
        let actual = json!({"id": 1, "result": {"list": [1, {"a": 1, "b": 2}], "extra": true}});
        let subset_of = |message: Value| expect(&message, Mode::Subset, &[]).check(&actual);
        assert_eq!(subset_of(json!({"id": 1})), Ok(()));
        assert_eq!(subset_of(json!({"result": {"list": [1, {"b": 2}]}})), Ok(()));
        assert_eq!(
            subset_of(json!({"result": {"list": [1]}})),
            Err(r#"/result/list: expected [1], got [1,{"a":1,"b":2}]"#.to_owned())
        );
        assert_eq!(
            subset_of(json!({"result": {"list": [1, {"b": 3}]}})),
            Err("/result/list/1/b: expected 3, got 2".to_owned())
        );
        let missing = subset_of(json!({"result": {"gone": 1}})).unwrap_err();
        assert!(missing.starts_with("/result/gone: missing in "), "{missing}");
        // Un `/` dans une clé s'écrit `~1` dans le pointeur (RFC 6901).
        let slash = expect(&json!({"a/b": 2}), Mode::Subset, &[]).check(&json!({"a/b": 1}));
        assert_eq!(slash, Err("/a~1b: expected 2, got 1".to_owned()));
        assert_eq!(expect(&actual, Mode::Exact, &[]).check(&actual), Ok(()));
        let exact = expect(&json!({"id": 1}), Mode::Exact, &[]).check(&actual).unwrap_err();
        assert!(exact.starts_with(r#"expected exactly {"id":1}, got "#), "{exact}");
    }

    #[test]
    fn absent_pointers_must_not_exist_in_the_message() {
        let actual = json!({"result": {"ttlMs": 0, "list": []}});
        let check = |pointers: &[&str]| expect(&json!({}), Mode::Subset, pointers).check(&actual);
        assert_eq!(check(&["/result/cacheScope", "/nope/deeper"]), Ok(()));
        let error = check(&["/result/cacheScope", "/result/ttlMs"]).unwrap_err();
        assert!(error.starts_with("/result/ttlMs must be absent, got "), "{error}");
    }

    #[test]
    fn a_snapshot_is_the_message_with_the_redacted_pointers_and_the_server_version_masked() {
        let actual = json!({"id": 1, "result": {
            "instructions": "long text",
            "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "mcp-legifrance", "version": "1.2.3"}},
            "tools": [{"name": "fr_search", "version": "kept"}],
        }});
        let mut wanted = expect(&json!({}), Mode::Subset, &[]);
        assert_eq!(wanted.capture(&actual), None);
        wanted.snapshot = Some("shape".to_owned());
        wanted.redact = vec!["/result/instructions".to_owned(), "/result/not/there".to_owned()];
        let capture = wanted.capture(&actual).unwrap();
        assert_eq!(capture.name, "shape");
        assert_eq!(capture.message["result"]["instructions"], "[redacted]");
        assert_eq!(
            capture.message["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["version"],
            "[version]"
        );
        // Un autre `name` garde sa `version`.
        assert_eq!(capture.message["result"]["tools"][0]["version"], "kept");
        assert_eq!(wanted.id(), None);
        assert_eq!(expect(&json!({"id": "x"}), Mode::Subset, &[]).id(), Some(&json!("x")));
    }

    #[test]
    fn the_common_controls_reject_what_a_server_must_never_say() {
        let sent = [json!(1), json!("a")];
        let violation = |message: Value| protocol_violation(&message, &sent);
        // Messages permis : réponse à une requête envoyée, notification, erreur sans id.
        assert_eq!(violation(json!({"jsonrpc": "2.0", "id": 1, "result": {}})), None);
        assert_eq!(
            violation(json!({"jsonrpc": "2.0", "id": "a", "error": {"code": -32601}})),
            None
        );
        assert_eq!(violation(json!({"jsonrpc": "2.0", "method": "notifications/progress"})), None);
        assert_eq!(violation(json!({"jsonrpc": "2.0", "error": {"code": -32600}})), None);
        assert_eq!(
            violation(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700}})),
            None
        );
        for code in ERROR_CODES {
            assert_eq!(
                violation(json!({"jsonrpc": "2.0", "id": 1, "error": {"code": code}})),
                None
            );
        }
        // Messages refusés.
        for (message, reason) in [
            (json!("not json"), "not a JSON-RPC 2.0 message"),
            (json!({"id": 1, "result": {}}), "not a JSON-RPC 2.0 message"),
            (json!({"jsonrpc": "1.0", "id": 1, "result": {}}), "not a JSON-RPC 2.0 message"),
            (
                json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}),
                "a server request is forbidden (C-03)",
            ),
            (json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32042}}), "(C-26)"),
            (json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32020}}), "(C-26)"),
            (json!({"jsonrpc": "2.0", "id": 1, "error": {"message": "no code"}}), "(C-26)"),
            (
                json!({"jsonrpc": "2.0", "id": 2, "result": {}}),
                "does not carry the id of a request",
            ),
        ] {
            let found = violation(message.clone()).unwrap();
            assert!(found.contains(reason), "{message}: {found}");
        }
    }

    #[tokio::test]
    async fn the_exit_code_is_zero_for_a_normal_end_and_one_for_a_failure() {
        let ended = |outcome| exit_code(&Ok(outcome));
        assert_eq!(ended(Ok(ServeOutcome::Eof)), 0);
        assert_eq!(ended(Ok(ServeOutcome::Shutdown)), 0);
        let aborted = || async {
            let handle = tokio::spawn(std::future::pending::<Result<ServeOutcome, ServeError>>());
            handle.abort();
            handle.await
        };
        let join_error = aborted().await.unwrap_err();
        assert_eq!(ended(Err(ServeError::Join(join_error))), 1);
        // Une tâche de service interrompue ou paniquée est un échec.
        assert_eq!(exit_code(&aborted().await), 1);
    }

    // ---- transcriptions : rejeu, écarts et fin de service ---------------------------------------

    /// Serveur minimal sur le chemin de production, pour les cas que `LegifranceServer` ne peut pas
    /// produire (un `server/discover` en échec, un refus de `subscriptions/listen`).
    struct Stub {
        /// `subscriptions/listen` est accepté (annexe D §5.3) ; sinon `-32601`.
        listens: bool,
        /// Code de l'erreur que rend `server/discover`, ou un résultat si `None`.
        discover_error: Option<i32>,
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "la signature de `discover` est celle du trait, sans `.await` dans ce double"
    )]
    impl ServerHandler for Stub {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
                .with_server_info(Implementation::new("mcp-legifrance", "0.0.0"))
        }

        fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
            Cow::Borrowed(crate::versions::SUPPORTED)
        }

        async fn discover(
            &self,
            _context: RequestContext<RoleServer>,
        ) -> Result<DiscoverResult, ErrorData> {
            match self.discover_error {
                Some(code) => Err(ErrorData::new(ErrorCode(code), "stub", None)),
                None => Ok(DiscoverResult::from_server_info(
                    crate::versions::SUPPORTED.to_vec(),
                    self.get_info(),
                )),
            }
        }

        fn accepted_subscription_filter(
            &self,
            _requested: &SubscriptionFilter,
        ) -> Option<SubscriptionFilter> {
            self.listens.then(SubscriptionFilter::new)
        }
    }

    const LISTENING: Stub = Stub { listens: true, discover_error: None };

    /// Le message d'écart du rejeu de `text` sur un serveur de la fixture par défaut.
    async fn replay_error(text: &str) -> String {
        let transcript = Transcript::parse("inline", text).unwrap();
        let fixture = Fixture::for_transcript(&transcript);
        replay_in_process(fixture.server(), &transcript).await.unwrap_err().to_string()
    }

    const PING: &str = r#"{"send":{"jsonrpc":"2.0","id":1,"method":"ping"}}"#;

    #[tokio::test]
    async fn a_mismatch_names_the_line_and_the_json_pointer() {
        let error =
            replay_error(&format!("{PING}\n{}", r#"{"expect":{"id":1,"result":{"x":1}}}"#)).await;
        assert!(error.starts_with("inline.jsonl line 2: /result/x: missing in "), "{error}");
        let error = replay_error(&format!(
            "{PING}\n{}",
            r#"{"expect":{"id":1,"result":{"a":1}},"mode":"exact"}"#
        ))
        .await;
        assert!(error.starts_with("inline.jsonl line 2: expected exactly "), "{error}");
        let error =
            replay_error(&format!("{PING}\n{}", r#"{"expect":{"id":1},"absent":["/result"]}"#))
                .await;
        assert!(error.starts_with("inline.jsonl line 2: /result must be absent"), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_message_that_never_comes_is_reported_missing_after_five_seconds() {
        let by_id = replay_error(r#"{"expect":{"id":5}}"#).await;
        assert_eq!(by_id, "inline.jsonl line 1: no message from the server within 5 s (id 5)");
        let next = replay_error(r#"{"expect":{"jsonrpc":"2.0"}}"#).await;
        assert_eq!(next, "inline.jsonl line 1: no message from the server within 5 s");
    }

    #[tokio::test]
    async fn the_server_closing_its_output_is_reported() {
        let closed = [
            r#"{"close_stdin":true}"#,
            r#"{"expect_exit":{"code":0,"within_ms":1000}}"#,
            r#"{"expect":{"jsonrpc":"2.0"}}"#,
        ];
        let error = replay_error(&closed.join("\n")).await;
        assert_eq!(error, "inline.jsonl line 3: the server closed its output");
        let error = replay_error(&format!(
            "{}\n{}\n{}",
            closed[0], closed[1], r#"{"expect_silence_ms":50}"#
        ))
        .await;
        assert_eq!(error, "inline.jsonl line 3: the server closed its output during the silence");
    }

    #[tokio::test]
    async fn a_message_during_a_silence_breaks_it() {
        let error = replay_error(&format!("{PING}\n{}", r#"{"expect_silence_ms":2000}"#)).await;
        assert!(error.starts_with("inline.jsonl line 2: a message broke the silence: "), "{error}");
    }

    #[tokio::test]
    async fn a_message_read_but_not_expected_blocks_a_silence_and_the_end() {
        let fixture = Fixture::default();
        let mut run = Run::start(fixture.server());
        run.pending.push_back(json!({"jsonrpc": "2.0", "id": 1, "result": {}}));
        let error = run.apply(&Step::ExpectSilence(Duration::from_millis(10))).await.unwrap_err();
        assert!(error.starts_with("a message arrived before the silence: "), "{error}");
        let error = run.finish().await.unwrap_err();
        assert!(error.starts_with("the server said more than expected: "), "{error}");
    }

    #[tokio::test]
    async fn an_expectation_without_id_takes_the_message_read_in_passing() {
        let ping = |id: u32| format!(r#"{{"send":{{"jsonrpc":"2.0","id":{id},"method":"ping"}}}}"#);
        // Attendre l'id 2 lit la réponse 1 au passage ; l'attente sans id la retrouve.
        let text = [
            ping(1),
            ping(2),
            r#"{"expect":{"id":2}}"#.to_owned(),
            r#"{"expect":{"jsonrpc":"2.0","result":{}}}"#.to_owned(),
        ]
        .join("\n");
        let transcript = Transcript::parse("passing", &text).unwrap();
        let replay = replay_in_process(Fixture::default().server(), &transcript).await.unwrap();
        assert_eq!(replay.received.len(), 2);
    }

    #[tokio::test]
    async fn a_message_nobody_expected_fails_the_end_of_the_transcript() {
        let error = replay_error(PING).await;
        assert!(
            error.starts_with("inline.jsonl line 1: the server said more than expected: "),
            "{error}"
        );
    }

    #[tokio::test]
    async fn the_exit_is_checked_for_code_delay_and_single_use() {
        let close = r#"{"close_stdin":true}"#;
        let exit = |code: i32| format!(r#"{{"expect_exit":{{"code":{code},"within_ms":1000}}}}"#);
        let error = replay_error(&format!("{close}\n{}", exit(1))).await;
        assert_eq!(error, "inline.jsonl line 2: the service exited with code 0, expected 1");
        let error = replay_error(&format!("{close}\n{}\n{}", exit(0), exit(0))).await;
        assert_eq!(error, "inline.jsonl line 3: the service already exited");
    }

    #[tokio::test]
    async fn writing_after_the_end_of_the_input_is_an_error() {
        let error = replay_error(&format!("{}\n{PING}", r#"{"close_stdin":true}"#)).await;
        assert_eq!(error, "inline.jsonl line 2: stdin is closed");
    }

    #[tokio::test(start_paused = true)]
    async fn an_exit_that_does_not_come_is_reported_after_the_delay() {
        let error = replay_error(r#"{"expect_exit":{"code":0,"within_ms":100}}"#).await;
        assert_eq!(error, "inline.jsonl line 1: the service did not exit within 100ms");
    }

    #[tokio::test]
    async fn a_binary_only_transcript_is_refused_in_process() {
        let transcript =
            Transcript::load(transcripts_dir().join("eof_with_open_listen.jsonl")).unwrap();
        let error = replay_in_process(Fixture::default().server(), &transcript).await.unwrap_err();
        assert_eq!(error.line, 0);
        assert_eq!(error.transcript, "eof_with_open_listen");
        assert!(error.message.starts_with("replayed on the binary only: V-35"), "{error}");
    }

    #[tokio::test]
    async fn expect_alive_needs_a_server_that_answers_discover() {
        let transcript = Transcript::parse("alive", r#"{"expect_alive":true}"#).unwrap();
        let ok = Stub { listens: false, discover_error: None };
        replay_in_process(ok, &transcript).await.unwrap();
        let failing = Stub { listens: false, discover_error: Some(-32603) };
        let error = replay_in_process(failing, &transcript).await.unwrap_err();
        assert!(
            error.message.starts_with("server/discover failed, the service is not alive: "),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_server_that_invents_an_error_code_is_caught_by_the_common_controls() {
        let transcript = Transcript::parse("alive", r#"{"expect_alive":true}"#).unwrap();
        let invented = Stub { listens: false, discover_error: Some(-32050) };
        let error = replay_in_process(invented, &transcript).await.unwrap_err();
        assert!(
            error.message.starts_with("an error code of our own is forbidden (C-26)"),
            "{error}"
        );
    }

    // ---- les 22 transcriptions de la liste minimale (06 §6.1) -------------------------------------

    fn transcript(name: &str) -> Transcript {
        Transcript::load(transcripts_dir().join(format!("{name}.jsonl"))).unwrap()
    }

    /// Compare les instantanés d'un rejeu à ceux de `src/snapshots/` (`insta`, cartes triées).
    fn assert_snapshots(replay: &Replay) {
        for capture in &replay.snapshots {
            insta::with_settings!({ sort_maps => true, omit_expression => true }, {
                insta::assert_json_snapshot!(capture.name.as_str(), capture.message);
            });
        }
    }

    /// Rejoue `name` sur la fixture qu'elle demande, et compare ses instantanés.
    async fn replay_green(name: &str) -> Replay {
        let transcript = transcript(name);
        let fixture = Fixture::for_transcript(&transcript);
        // `ReplayError` (Debug) porte la transcription, la ligne et l'écart.
        let replay = replay_in_process(fixture.server(), &transcript).await.unwrap();
        assert_snapshots(&replay);
        replay
    }

    /// Une transcription rejouée par test, du nom de la transcription ; `REPLAYED` en garde la liste.
    macro_rules! replayed_transcripts {
        ($($name:ident),* $(,)?) => {
            const REPLAYED: &[&str] = &[$(stringify!($name)),*];
            $(
                #[tokio::test]
                async fn $name() {
                    replay_green(stringify!($name)).await;
                }
            )*
        };
    }

    replayed_transcripts! {
        modern_discover,
        modern_tools_list,
        modern_tools_call,
        modern_listen,
        modern_meta_incomplete,
        modern_unsupported_version,
        modern_envelope_legacy_version,
        modern_removed_methods,
        legacy_2025_11_25,
        legacy_2025_06_18,
        legacy_then_modern,
        discover_after_initialize,
        initialize_negotiation,
        unknown_method_lifecycle,
        premature_notification,
        garbage_and_batch,
        cancel_then_silence,
        eof_after_discover,
        interleaved_two_conversations,
        mrtr_fields_tolerated,
        results_carry_server_info,
    }

    /// Rejouée sur le binaire seulement (T-M4-08).
    const BINARY_ONLY: &[&str] = &["eof_with_open_listen"];

    #[test]
    fn the_minimal_list_of_twenty_two_transcriptions_is_versioned() {
        let found: Vec<String> = Transcript::load_all(transcripts_dir())
            .unwrap()
            .iter()
            .map(|transcript| transcript.name().to_owned())
            .collect();
        let mut expected: Vec<&str> = [REPLAYED, BINARY_ONLY].concat();
        assert_eq!(expected.len(), 22);
        expected.sort_unstable();
        assert_eq!(found, expected);
        // Seule `eof_with_open_listen` est réservée au binaire ; seules deux demandent un amont lent.
        let binary: Vec<String> = Transcript::load_all(transcripts_dir())
            .unwrap()
            .iter()
            .filter(|transcript| transcript.binary_only().is_some())
            .map(|transcript| transcript.name().to_owned())
            .collect();
        assert_eq!(binary, BINARY_ONLY);
        let slow: Vec<&str> = REPLAYED
            .iter()
            .copied()
            .filter(|name| transcript(name).needs(Need::SlowUpstream))
            .collect();
        assert_eq!(slow, ["cancel_then_silence", "interleaved_two_conversations"]);
    }

    /// `subscriptions/listen` (C-24, annexe D §5.3) : l'accusé minimal, puis le silence jusqu'à
    /// l'annulation. La transcription est éprouvée sur un serveur qui accepte le filtre.
    #[tokio::test]
    async fn modern_listen_is_acknowledged_by_a_server_that_accepts_the_subscription_filter() {
        let transcript = transcript("modern_listen");
        replay_in_process(LISTENING, &transcript).await.unwrap();
    }

    /// V-35 en processus, sur le chemin de production : un EOF avec un `listen` ouvert termine le
    /// service avec le code 0 en moins d'une seconde. La transcription reste réservée au binaire
    /// (V3-01), seul à mesurer la sortie du processus.
    #[tokio::test]
    async fn v35_eof_with_an_open_listen_exits_zero_within_a_second() {
        let binary_only = transcript("eof_with_open_listen");
        let steps = binary_only
            .steps
            .iter()
            .filter(|instruction| !matches!(instruction.step, Step::BinaryOnly(_)))
            .cloned()
            .collect();
        let in_process = Transcript { name: binary_only.name.clone(), steps };
        replay_in_process(Fixture::default().server(), &in_process).await.unwrap();
    }

    // ---- constats de rmcp 3.5.1 (annexe D §9, 09 V-33 à V-37, V-168) ------------------------------

    fn result_of(replay: &Replay, id: &Value) -> Value {
        replay.received.iter().find(|message| message["id"] == *id).unwrap()["result"].clone()
    }

    /// V-33 : `ttlMs` et `cacheScope` ne sont posés qu'aux pairs modernes, rmcp ne retire aux pairs
    /// legacy que `resultType`.
    #[tokio::test]
    async fn v33_cache_hints_are_posed_for_modern_peers_only() {
        let modern = replay_green("modern_tools_list").await;
        let listed = result_of(&modern, &json!(1));
        assert_eq!(listed["ttlMs"], json!(3_600_000));
        assert_eq!(listed["cacheScope"], json!("public"));
        assert_eq!(listed["resultType"], json!("complete"));
        for legacy in ["legacy_2025_11_25", "legacy_2025_06_18"] {
            let replay = replay_green(legacy).await;
            let listed = result_of(&replay, &json!(2));
            let keys: Vec<&String> = listed.as_object().unwrap().keys().collect();
            assert_eq!(keys, ["tools"], "{legacy}");
        }
    }

    /// V-34 : une requête à enveloppe moderne qui déclare une version legacy est servie, sans
    /// `resultType` ni indices de cache. Le repli de 00 §6 (`-32022` par `PreLifecycleFilter`) n'est
    /// pas appliqué ; s'il l'est, ce test et `modern_envelope_legacy_version` s'inversent.
    #[tokio::test]
    async fn v34_a_modern_envelope_with_a_legacy_version_is_served_in_the_legacy_form() {
        let replay = replay_green("modern_envelope_legacy_version").await;
        let listed = result_of(&replay, &json!(1));
        assert!(listed["tools"].is_array(), "{listed}");
        let keys: Vec<&String> = listed.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["tools"]);
        assert!(result_of(&replay, &json!(2)).get("resultType").is_none());
    }

    /// V-37 : le client rmcp 3.5.1 de `connect` (`().serve`) ouvre la poignée de main legacy
    /// `initialize` ; avec le repli explicite du serveur, il négocie 2025-11-25. Les tests d'outils
    /// qui passent par ce client voient donc l'ère legacy (sans `resultType`) ; l'ère moderne se
    /// teste par les transcriptions, qui forcent l'ère dans chaque `_meta`.
    #[tokio::test]
    async fn v37_the_in_process_client_negotiates_the_legacy_era() {
        let client = connect(Fixture::default().server()).await;
        let negotiated = client.peer_info().expect("the server introduced itself");
        assert_eq!(negotiated.protocol_version, ProtocolVersion::V_2025_11_25);
        assert!(negotiated.protocol_version < ProtocolVersion::V_2026_07_28);
    }

    /// V-168, partie testable sans console : l'EOF d'une session ouverte termine le service avec le
    /// code 0 en moins d'une seconde, quel que soit le système (`duplex` n'a pas de système). Les
    /// jobs Windows de la CI le rejouent ; l'événement Ctrl+C et le binaire relèvent de V3-01.
    #[tokio::test]
    async fn v168_eof_in_a_session_exits_zero_within_a_second() {
        let session = [
            r#"{"send":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#,
            r#"{"expect":{"id":1,"result":{"protocolVersion":"2025-11-25"}}}"#,
            r#"{"send":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
            r#"{"send":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#,
            r#"{"expect":{"id":2}}"#,
            r#"{"close_stdin":true}"#,
            r#"{"expect_exit":{"code":0,"within_ms":1000}}"#,
        ]
        .join("\n");
        let transcript = Transcript::parse("eof_in_session", &session).unwrap();
        let server = Fixture::default().server();
        replay_in_process(server, &transcript).await.unwrap();
    }

    /// Review Focus 5 : EOF pendant un appel en vol. Le serveur se termine avec le code 0 en moins
    /// d'une seconde, sans attendre les 30 s de l'amont ni les 5 s que rmcp accorde sinon aux
    /// réponses en vol (`service.rs:1762-1766`), et l'appel garde sa ligne d'audit. rmcp écrit
    /// encore la réponse de l'appel annulé avant de s'arrêter (`isError`, « cancelled by client »),
    /// une fois le gestionnaire revenu, donc après sa ligne d'audit.
    #[tokio::test]
    async fn eof_with_inflight_call_exits_zero_within_1s() {
        let fixture = Fixture::slow_upstream();
        let mut run = Run::start(fixture.server());
        let call = json!({"jsonrpc": "2.0", "id": "call-1", "method": "tools/call",
            "params": {"name": "fr_search", "arguments": {"query": "bail"}, "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {}}}});
        for step in [
            Step::Send(call),
            // Rien ne répond : l'appel est en vol, au milieu des 30 s de l'amont.
            Step::ExpectSilence(Duration::from_millis(200)),
            Step::CloseStdin,
            Step::ExpectExit { code: 0, within: Duration::from_secs(1) },
        ] {
            run.apply(&step).await.unwrap();
        }
        assert_eq!(fixture.corpus.calls(), 1, "the call reached the port before the EOF");
        // La réponse de l'appel annulé dit que le client l'a annulé.
        let late = run.read_message().await.unwrap();
        assert_eq!(late["id"], json!("call-1"));
        assert_eq!(late["result"]["isError"], json!(true));
        assert_eq!(late["result"]["content"][0]["text"], json!("cancelled by client"));
        // Une ligne d'audit par appel, même annulé par l'EOF (Q9), écrite avant cette réponse.
        let records = fixture.audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error.as_deref(), Some("cancelled by client"));
    }

    /// Bug rmcp #1347 (annexe D §9.1, test documentaire) : une méthode connue dont les `params` ne
    /// se désérialisent pas reçoit `-32601` (la requête est lue comme une méthode inconnue) au lieu
    /// de `-32602`, avant tout code de l'outil et donc sans ligne d'audit. Tant que l'issue est
    /// ouverte, ce test attend `-32601` ; à sa correction, l'inverser en `-32602`.
    #[tokio::test]
    async fn raw_issue_1347_known_method_with_undecodable_params_is_method_not_found() {
        let meta = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;
        let mut lines = Vec::new();
        for (id, params) in [
            (1, format!(r#"{{"name":5,{meta}}}"#)),
            (2, format!(r#"{{"name":"fr_coverage","arguments":"bail",{meta}}}"#)),
        ] {
            lines.push(format!(
                r#"{{"send":{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{params}}}}}"#
            ));
            lines.push(format!(r#"{{"expect":{{"id":{id},"error":{{"code":-32601}}}}}}"#));
        }
        let transcript = Transcript::parse("issue_1347", &lines.join("\n")).unwrap();
        let fixture = Fixture::default();
        replay_in_process(fixture.server(), &transcript).await.unwrap();
        assert_eq!(fixture.audit.records().len(), 0);
        assert_eq!(fixture.corpus.calls(), 0);
    }

    /// Bug rmcp #1337 (C-21) : `outputSchema` et `structuredContent` ont une racine objet pour les
    /// pairs legacy, qui l'exigent. Le client en processus est legacy (V-37).
    #[tokio::test]
    async fn legacy_tools_call_structured_content_is_object() {
        let client = Fixture::default().connect().await;
        for tool in client.list_all_tools().await.unwrap() {
            let schema = tool.output_schema.expect("every tool declares an output schema");
            assert_eq!(schema["type"], json!("object"), "{}", tool.name);
            assert_eq!(tool.input_schema["type"], json!("object"), "{}", tool.name);
        }
        let result = client.call_tool(CallToolRequestParams::new("fr_coverage")).await.unwrap();
        assert!(result.structured_content.is_some_and(|content| content.is_object()));
    }

    /// Bug rmcp #1348 (C-22, C-53) : aucun descripteur ne porte `execution`, `icons` ni
    /// `x-mcp-header`. Le verrou des descripteurs (V3-02) le contrôle sur la surface complète.
    #[tokio::test]
    async fn raw_issue_1348_descriptors_carry_no_execution_icons_or_header_hints() {
        let client = Fixture::default().connect().await;
        let tools = serde_json::to_string(&client.list_all_tools().await.unwrap()).unwrap();
        for forbidden in ["\"execution\"", "\"icons\"", "x-mcp-header"] {
            assert!(!tools.contains(forbidden), "{forbidden} in {tools}");
        }
    }

    #[tokio::test]
    async fn connect_lists_the_tools_of_the_router() {
        let fixture = Fixture::default();
        let mut server = fixture.server();
        let registered = server.router_mut().list_all().len();
        let client = connect(server).await;
        assert_eq!(client.list_all_tools().await.unwrap().len(), registered);
        let info = client.peer_info().expect("the server introduced itself");
        assert_eq!(info.server_info.as_ref().unwrap().name, "mcp-legifrance");
    }

    #[tokio::test]
    async fn the_fixture_connects_its_own_server() {
        let client = Fixture::default().connect().await;
        let names: Vec<_> =
            client.list_all_tools().await.unwrap().into_iter().map(|tool| tool.name).collect();
        assert!(names.iter().any(|name| name == "fr_coverage"), "{names:?}");
    }

    #[test]
    fn the_default_fixture_is_strict_with_credentials_on_a_fixed_day() {
        let fixture = Fixture::default();
        assert_eq!(fixture.settings.policy.mode, AuditMode::Strict);
        assert_eq!(fixture.settings.policy.deadline, Duration::from_secs(60));
        assert!(fixture.settings.configuration.credentials_configured);
        assert_eq!(fixture.today.to_string(), "2026-10-29");
        assert_eq!(fixture.audit.records().len(), 0);
        assert_eq!(fixture.corpus.calls(), 0);
    }

    #[test]
    fn the_server_reads_the_settings_of_the_fixture() {
        let mut fixture = Fixture::default();
        fixture.settings.policy.deadline = Duration::from_secs(7);
        assert_eq!(fixture.server().service.settings().policy.deadline, Duration::from_secs(7));
    }

    #[tokio::test]
    async fn the_shared_audit_handle_sees_what_the_service_records() {
        let fixture = Fixture::default();
        let shared = SharedAudit(Arc::clone(&fixture.audit));
        #[expect(
            clippy::default_trait_access,
            reason = "chrono n'est pas une dépendance de ce crate : l'instant par défaut est l'époque Unix"
        )]
        let ts = Default::default();
        let record = AuditRecord {
            ts,
            tool: ToolName::FR_COVERAGE,
            input: CanonicalInput::new("{}".to_owned()),
            output_count_or_size: 0,
            duration_ms: 0,
            status: AuditStatus::Ok,
            error_code: None,
            error: None,
            env: PisteEnvironment::Sandbox,
            server_version: "0.0.0",
        };
        shared.record(&record).await.unwrap();
        assert_eq!(fixture.audit.records().len(), 1);
        assert_eq!(fixture.audit.records()[0].tool, ToolName::FR_COVERAGE);
    }

    #[tokio::test]
    async fn the_shared_corpus_forwards_the_twelve_operations_to_the_fake() {
        let corpus = Arc::new(FakeCorpus::new());
        let shared = SharedCorpus(Arc::clone(&corpus));
        let query = SearchQuery::new(
            Fond::Juri,
            QueryText::parse("bail").unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            None,
        )
        .unwrap();
        let number = NumberQuery::new(
            Fond::Juri,
            NumberKind::Case,
            NumberValue::parse("24-13.470").unwrap(),
            VersionDate::MIN,
        )
        .unwrap();
        let text: LegiTextId = "LEGITEXT000006070721".parse().unwrap();
        let article: ArticleId = "LEGIARTI000006419288".parse().unwrap();
        let date = VersionDate::parse("2026-10-29").unwrap();

        // Réponses non programmées : seul le transfert des appels au faux est éprouvé.
        let _ = shared.search(&query).await;
        let _ = shared.get_act(&text, date).await;
        let _ = shared.get_article(&article).await;
        let _ = shared.get_decision(&"JURITEXT000037999394".parse().unwrap()).await;
        let _ = shared.get_deliberation(&"CNILTEXT000053352643".parse().unwrap()).await;
        let _ = shared.get_convention(&"KALITEXT000005652329".parse().unwrap()).await;
        let _ = shared.get_company_agreement(&"ACCOTEXT000037731479".parse().unwrap()).await;
        let _ = shared.find_article(&text, "9", ArticleAt::Today(date)).await;
        let _ = shared.find_by_number(&number).await;
        let _ = shared.list_codes().await;
        let _ = shared.article_versions(&article).await;
        let _ = shared.related_links(&article).await;

        let methods: Vec<&str> = corpus.seen_requests().iter().map(PortRequest::method).collect();
        assert_eq!(
            methods,
            [
                "search",
                "get_act",
                "get_article",
                "get_decision",
                "get_deliberation",
                "get_convention",
                "get_company_agreement",
                "find_article",
                "find_by_number",
                "list_codes",
                "article_versions",
                "related_links",
            ]
        );
    }
}
