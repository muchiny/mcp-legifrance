//! `JsonlAuditLog` : le journal d'audit JSONL, écrit par un fil dédié sous verrou `fs4`
//! (06 §11.5 ; 02 §3.5).
//!
//! Deux niveaux. [`Store`] fait l'E/S synchrone d'**une** ligne : répertoire, verrou, rotation,
//! ouverture en ajout, `write_all`, `sync_data`. [`JsonlAuditLog`] n'est que la file d'attente :
//! il remet les lignes à un fil qui possède le `Store`, et attend l'accusé.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

use fs4::TryLockError;
use lf_application::ports::{AuditError, AuditErrorKind, AuditLog, AuditRecord};
use tokio::sync::{mpsc, oneshot};
use tracing::field;

use crate::{
    hash, line,
    rotation::{self, ACTIVE_FILE, Active, PREFIX_LEN},
};

/// Fichier de verrou dédié (00 §7) : le journal lui-même est renommé par la rotation, il ne
/// peut donc pas porter le verrou.
const LOCK_FILE: &str = ".mcp-legifrance.lock";

/// Lignes en attente au plus (06 §11.5, 2) ; au-delà, `record` attend une place.
const CHANNEL_CAPACITY: usize = 1024;

/// Attente maximale du verrou : au-delà, l'écriture échoue plutôt que de se figer (contrat du
/// port `AuditLog`, point 2).
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// Pause entre deux tentatives non bloquantes du verrou.
const LOCK_POLL: Duration = Duration::from_millis(5);

/// Marge de l'attente de l'accusé au-delà de l'attente du verrou : écriture, `sync_data` et
/// rotation d'une ligne.
const ACK_MARGIN: Duration = Duration::from_secs(30);

/// Ce qu'une écriture réussie a fait, pour le span `audit_write`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Written {
    /// Octets de la ligne, saut de ligne final compris.
    pub(crate) bytes: usize,
    /// Le journal actif a été archivé juste avant cette ligne.
    pub(crate) rotated: bool,
}

/// L'E/S d'une ligne, sans file d'attente ni tâche : tout est synchrone.
pub(crate) struct Store {
    dir: PathBuf,
    rotate_bytes: u64,
    lock_timeout: Duration,
    /// Les droits du répertoire préexistant ont déjà été contrôlés (un seul `warn!`).
    dir_inspected: bool,
    /// Idem pour le journal actif préexistant.
    file_inspected: bool,
}

impl Store {
    pub(crate) fn new(dir: PathBuf, rotate_bytes: u64, lock_timeout: Duration) -> Self {
        Self { dir, rotate_bytes, lock_timeout, dir_inspected: false, file_inspected: false }
    }

    /// Ajoute `line` (qui se termine par `\n`, datée de `date` : `AAAA-MM-JJ`) au journal actif.
    pub(crate) fn append(&mut self, line: &str, date: &str) -> Result<Written, AuditError> {
        self.append_io(line, date).map_err(|error| failure(&self.dir, &error))
    }

    fn append_io(&mut self, line: &str, date: &str) -> io::Result<Written> {
        self.ensure_dir().and_then(|()| self.lock()).and_then(|lock| {
            // Le verrou se relâche à la fermeture de `lock`, après l'écriture et la rotation.
            let written = self.write_locked(line, date);
            drop(lock);
            written
        })
    }

    /// Crée le répertoire (`0700` sous Unix) s'il manque ; contrôle une fois ses droits s'il
    /// préexiste. Un répertoire lié (lien symbolique) est admis.
    fn ensure_dir(&mut self) -> io::Result<()> {
        match fs::metadata(&self.dir) {
            Ok(meta) if meta.is_dir() => {
                if !self.dir_inspected {
                    self.dir_inspected = true;
                    warn_if_broad(&self.dir, &meta, "directory");
                }
                Ok(())
            }
            Ok(_) => Err(io::Error::new(io::ErrorKind::NotADirectory, "not a directory")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.dir_inspected = true;
                create_private_dir(&self.dir)
            }
            Err(error) => Err(error),
        }
    }

    /// Prend le verrou consultatif sur le fichier dédié, avec une attente bornée.
    fn lock(&self) -> io::Result<File> {
        let file = open_private(&self.dir.join(LOCK_FILE), Mode::Lock)?;
        acquire_with(|| fs4::FileExt::try_lock(&file), self.lock_timeout)?;
        Ok(file)
    }

    /// Rotation éventuelle, puis ajout de la ligne et `sync_data`, le tout sous le verrou.
    ///
    /// Les étapes s'enchaînent par `and_then` plutôt que par `?` : chaque `?` est une région de
    /// couverture dont la branche d'erreur ne s'atteint pas sans défaillance du disque (écriture,
    /// `fsync`, renommage), que les tests ne savent pas provoquer. Le comportement est le même.
    fn write_locked(&mut self, line: &str, date: &str) -> io::Result<Written> {
        let path = self.dir.join(ACTIVE_FILE);
        refuse_non_regular(&path)
            .and_then(|()| open_private(&path, Mode::Journal))
            .and_then(|file| self.prepare(file, &path, line, date))
            .and_then(|ready| ready.append(line, &self.dir))
    }

    /// Relit l'état du journal ouvert et l'archive d'abord si la ligne l'exige.
    fn prepare(
        &mut self,
        mut file: File,
        path: &Path,
        line: &str,
        date: &str,
    ) -> io::Result<Ready> {
        self.inspect(&mut file, path).and_then(|active| {
            let line_len = u64::try_from(line.len()).unwrap_or(u64::MAX);
            if !rotation::must_rotate(&active, date, line_len, self.rotate_bytes) {
                return Ok(Ready { file, rotated: false, new_entry: active.len == 0 });
            }
            // Windows ne renomme pas un fichier ouvert : on referme avant.
            drop(file);
            rotation::rotate(&self.dir, active.date.as_deref().unwrap_or(date))
                .and_then(|_archive| open_private(path, Mode::Journal))
                .map(|file| Ready { file, rotated: true, new_entry: true })
        })
    }

    /// Taille et date de la première ligne du journal ; contrôle ses droits une fois s'il
    /// préexiste.
    fn inspect(&mut self, file: &mut File, path: &Path) -> io::Result<Active> {
        file.metadata().and_then(|meta| {
            if meta.len() > 0 && !self.file_inspected {
                self.file_inspected = true;
                warn_if_broad(path, &meta, "journal");
            }
            first_date_of(file, meta.len()).map(|date| Active { len: meta.len(), date })
        })
    }
}

/// Le journal ouvert, prêt à recevoir sa ligne.
struct Ready {
    file: File,
    /// Le journal précédent vient d'être archivé.
    rotated: bool,
    /// Le fichier est neuf : son entrée de répertoire doit survivre à un arrêt brutal.
    new_entry: bool,
}

impl Ready {
    /// `write_all` puis `sync_data` : la ligne est durable quand cette fonction rend `Ok`.
    fn append(mut self, line: &str, dir: &Path) -> io::Result<Written> {
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.sync_data())
            .and_then(|()| if self.new_entry { rotation::sync_dir(dir) } else { Ok(()) })
            .map(|()| Written { bytes: line.len(), rotated: self.rotated })
    }
}

/// Les droits d'un fichier ou répertoire privé.
#[derive(Clone, Copy)]
enum Mode {
    /// Le fichier de verrou : lu et écrit, jamais tronqué.
    Lock,
    /// Le journal : ajout seul, relu au début pour la date de sa première ligne.
    Journal,
}

/// Ouvre (en le créant au besoin, en `0600` sous Unix) le fichier de verrou ou le journal.
fn open_private(path: &Path, mode: Mode) -> io::Result<File> {
    let mut options = OpenOptions::new();
    match mode {
        Mode::Lock => options.read(true).write(true).truncate(false),
        Mode::Journal => options.read(true).append(true),
    };
    options.create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Crée `dir` et ses parents manquants, en `0700` sous Unix.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Refuse un journal qui est un lien symbolique ou pas un fichier ordinaire (06 §11.6). Une
/// course entre ce contrôle et l'ouverture est acceptée : elle suppose un attaquant du même
/// compte (§10.2, MEN-18). Un fichier absent ou illisible passe : l'ouverture qui suit le dit.
fn refuse_non_regular(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() => Err(io::Error::other(
            "the audit journal is a symbolic link or not a regular file; refusing to write to it",
        )),
        _ => Ok(()),
    }
}

/// La date `AAAA-MM-JJ` de la première ligne d'un fichier de `len` octets, relue dans ses
/// 17 premiers octets.
fn first_date_of(file: &mut File, len: u64) -> io::Result<Option<String>> {
    if len == 0 {
        return Ok(None);
    }
    let mut prefix = Vec::with_capacity(PREFIX_LEN);
    file.take(u64::try_from(PREFIX_LEN).unwrap_or(u64::MAX))
        .read_to_end(&mut prefix)
        .map(|_| rotation::date_of_prefix(&prefix).map(str::to_owned))
}

/// `warn!` si `meta` donne à d'autres comptes un accès que le défaut (`0700` / `0600`) refuse.
/// Le répertoire ou le fichier n'est pas modifié : il peut appartenir à la personne (06 §11.6).
#[cfg(unix)]
fn warn_if_broad(path: &Path, meta: &fs::Metadata, what: &str) {
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777;
    if mode & 0o077 != 0 {
        tracing::warn!(
            path = %path.display(),
            mode = %format_args!("{mode:03o}"),
            "audit {what} is accessible to other accounts; its permissions are left unchanged"
        );
    }
}

/// Hors Unix, les droits ne se lisent pas en bits (ACL : V-165).
#[cfg(not(unix))]
fn warn_if_broad(_: &Path, _: &fs::Metadata, _: &str) {}

/// Ce que le fil d'écriture exécute pour chaque ligne reçue : `(ligne, date)`.
type Handler = Box<dyn FnMut(&str, &str) -> Result<Written, AuditError> + Send>;

/// Une ligne remise au fil d'écriture, avec de quoi répondre.
struct Job {
    line: String,
    date: String,
    ack: oneshot::Sender<Result<Written, AuditError>>,
}

/// Journal d'audit JSONL : une ligne par exécution d'outil, en ajout seul (06 §11).
///
/// Implémente [`AuditLog`]. Le crate documente le format, le verrou et la rotation. Plusieurs
/// instances, dans un ou plusieurs processus, peuvent écrire le même répertoire.
#[derive(Debug)]
pub struct JsonlAuditLog {
    dir: PathBuf,
    rotate_bytes: u64,
    lock_timeout: Duration,
    /// Crée le fil d'écriture ; remplaçable par les tests pour simuler un échec de création.
    spawn: fn(Store) -> io::Result<mpsc::Sender<Job>>,
    /// Le fil d'écriture, créé au premier `record()`.
    writer: Mutex<Option<mpsc::Sender<Job>>>,
}

impl JsonlAuditLog {
    /// Taille du journal actif à partir de laquelle il est archivé : 16 Mio (02 §3.5).
    pub const DEFAULT_ROTATE_BYTES: u64 = 16 * 1024 * 1024;

    /// Prépare un journal dans `dir`, archivé quand il dépasserait `rotate_bytes`.
    ///
    /// **Aucune E/S** : ni le répertoire, ni le fil d'écriture, ni les fichiers n'existent avant
    /// le premier `record()` (00 §7). Une erreur d'accès au répertoire se lit donc à ce moment,
    /// dans `record()`.
    ///
    /// # Errors
    ///
    /// `AuditErrorKind::Io` si `dir` est vide ou si `rotate_bytes` vaut 0 : une configuration
    /// qui ne pourrait jamais écrire.
    pub fn open(dir: &Path, rotate_bytes: u64) -> Result<Self, AuditError> {
        if dir.as_os_str().is_empty() {
            return Err(unusable_setting("the audit directory is empty"));
        }
        if rotate_bytes == 0 {
            return Err(unusable_setting("the rotation size must be at least one byte"));
        }
        Ok(Self {
            dir: dir.to_owned(),
            rotate_bytes,
            lock_timeout: LOCK_TIMEOUT,
            spawn: spawn_store,
            writer: Mutex::new(None),
        })
    }

    /// Pour les tests : raccourcit l'attente du verrou.
    #[cfg(test)]
    fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// Pour les tests : remplace la création du fil d'écriture.
    #[cfg(test)]
    fn with_spawn(mut self, spawn: fn(Store) -> io::Result<mpsc::Sender<Job>>) -> Self {
        self.spawn = spawn;
        self
    }

    /// Pour les tests : remplace le fil d'écriture (fil simulé, mort ou figé).
    #[cfg(test)]
    fn with_sender(self, sender: mpsc::Sender<Job>) -> Self {
        *self.writer.lock().unwrap() = Some(sender);
        self
    }

    /// Le fil d'écriture, créé au premier appel (ou recréé après une panne).
    fn sender(&self) -> Result<mpsc::Sender<Job>, AuditError> {
        let mut slot = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(sender) = slot.as_ref() {
            return Ok(sender.clone());
        }
        let store = Store::new(self.dir.clone(), self.rotate_bytes, self.lock_timeout);
        let sender = (self.spawn)(store).map_err(spawn_failure)?;
        *slot = Some(sender.clone());
        Ok(sender)
    }

    /// Oublie `sender` s'il est toujours le fil courant : le prochain `record()` en crée un.
    fn forget(&self, sender: &mpsc::Sender<Job>) {
        let mut slot = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(|current| current.same_channel(sender)) {
            *slot = None;
        }
    }

    async fn write(&self, record: &AuditRecord, input_hash: &str) -> Result<Written, AuditError> {
        let line = line::render(record, input_hash)?;
        let date = line::utc_date(record.ts);
        let sender = self.sender()?;
        let (ack, receipt) = oneshot::channel();
        if sender.send(Job { line, date, ack }).await.is_err() {
            self.forget(&sender);
            return Err(writer_gone("the audit writer thread has stopped"));
        }
        match tokio::time::timeout(self.lock_timeout.saturating_add(ACK_MARGIN), receipt).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => {
                self.forget(&sender);
                Err(writer_gone("the audit writer thread stopped before acknowledging"))
            }
            Err(_) => {
                self.forget(&sender);
                Err(writer_gone("the audit writer did not acknowledge in time"))
            }
        }
    }
}

impl AuditLog for JsonlAuditLog {
    async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        let input_hash = hash::input_hash(&record.input);
        let span = tracing::debug_span!(
            "audit_write",
            input_hash = &input_hash[..12],
            bytes = field::Empty,
            rotated = field::Empty
        );
        match self.write(record, &input_hash).await {
            Ok(written) => {
                span.record("bytes", written.bytes);
                span.record("rotated", written.rotated);
                Ok(())
            }
            Err(error) => {
                tracing::error!(
                    parent: &span,
                    kind = ?error.kind,
                    detail = %error.detail,
                    "audit write failed"
                );
                Err(error)
            }
        }
    }
}

/// Lance le fil d'écriture de production : il possède `store`.
fn spawn_store(mut store: Store) -> io::Result<mpsc::Sender<Job>> {
    spawn_writer(move |line, date| store.append(line, date))
}

/// Lance le fil qui exécute `handle` pour chaque ligne reçue, dans l'ordre, puis répond.
///
/// Une ligne remise est écrite en entier même si l'appelant a abandonné son attente (contrat du
/// port) : la réponse est envoyée sans condition, et son échec est ignoré. Le fil s'arrête quand
/// le dernier émetteur disparaît.
fn spawn_writer(
    handle: impl FnMut(&str, &str) -> Result<Written, AuditError> + Send + 'static,
) -> io::Result<mpsc::Sender<Job>> {
    spawn_boxed(Box::new(handle))
}

/// Corps non générique de [`spawn_writer`] : une seule instance de code, quel que soit le
/// gestionnaire.
fn spawn_boxed(mut handle: Handler) -> io::Result<mpsc::Sender<Job>> {
    let (sender, mut jobs) = mpsc::channel::<Job>(CHANNEL_CAPACITY);
    thread::Builder::new()
        .name("lf-audit-writer".to_owned())
        .spawn(move || {
            while let Some(job) = jobs.blocking_recv() {
                let outcome = handle(&job.line, &job.date);
                let _ = job.ack.send(outcome);
            }
        })
        .map(|_detached| sender)
}

/// Tente le verrou jusqu'à ce qu'il soit pris ou que `timeout` soit écoulé (non bloquant, pour
/// que l'attente soit bornée : contrat du port `AuditLog`, point 2).
///
/// # Errors
///
/// `TimedOut` si le verrou reste tenu ; sinon l'erreur d'E/S de la tentative.
fn acquire_with(
    mut try_lock: impl FnMut() -> Result<(), TryLockError>,
    timeout: Duration,
) -> io::Result<()> {
    acquire(&mut try_lock, timeout)
}

/// Corps non générique de [`acquire_with`].
fn acquire(
    try_lock: &mut dyn FnMut() -> Result<(), TryLockError>,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(error)) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the audit lock stayed held by another writer",
            ));
        }
        thread::sleep(LOCK_POLL);
    }
}

/// Le genre d'`AuditError` d'une erreur d'E/S (02 §5.2, 06 §11.7). Un délai dépassé est un
/// écrivain qui ne répond plus : `WriterGone`, comme l'exige le contrat du port.
pub(crate) fn kind_of(error: &io::Error) -> AuditErrorKind {
    match error.kind() {
        io::ErrorKind::StorageFull => AuditErrorKind::DiskFull,
        io::ErrorKind::PermissionDenied => AuditErrorKind::PermissionDenied,
        io::ErrorKind::TimedOut => AuditErrorKind::WriterGone,
        _ => AuditErrorKind::Io,
    }
}

/// Erreur d'écriture dans `dir` : le chemin et la cause vont dans `detail` (journaux seulement).
fn failure(dir: &Path, error: &io::Error) -> AuditError {
    AuditError {
        kind: kind_of(error),
        detail: format!("audit directory {}: {error}", dir.display()),
    }
}

#[expect(clippy::needless_pass_by_value, reason = "passée telle quelle à `map_err`")]
fn spawn_failure(error: io::Error) -> AuditError {
    AuditError { kind: kind_of(&error), detail: format!("audit writer thread: {error}") }
}

fn writer_gone(detail: &str) -> AuditError {
    AuditError { kind: AuditErrorKind::WriterGone, detail: detail.to_owned() }
}

fn unusable_setting(detail: &str) -> AuditError {
    AuditError { kind: AuditErrorKind::Io, detail: detail.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{record, record_at};
    use chrono::TimeZone;
    use lf_application::ports::AuditStatus;
    use std::{
        collections::BTreeMap,
        sync::{Arc, mpsc as std_mpsc},
    };
    use tracing::{
        Event, Level, Metadata, Subscriber,
        field::{Field, Visit},
        span::{Attributes, Id, Record},
        subscriber::Interest,
    };

    fn journal(dir: &Path) -> JsonlAuditLog {
        JsonlAuditLog::open(dir, JsonlAuditLog::DEFAULT_ROTATE_BYTES).unwrap()
    }

    /// Capteur propre à un test. `tracing` mémorise l'intérêt d'un site d'appel à sa première
    /// émission, pour tout le processus : un test sans capteur qui l'atteint le premier le fige à
    /// « personne n'écoute », et un capteur local installé ensuite ne reçoit plus rien. Sous
    /// `cargo test` (couverture), où tous les tests partagent un processus, cela rend les tests de
    /// traces aléatoires. Un capteur global qui ne répond jamais « jamais » (`sometimes`) évite ce gel.
    fn local_capture() -> (Capture, tracing::subscriber::DefaultGuard) {
        static INTEREST: std::sync::Once = std::sync::Once::new();
        INTEREST.call_once(|| {
            tracing::subscriber::set_global_default(Capture::default()).unwrap();
        });
        let capture = Capture::default();
        let guard = tracing::subscriber::set_default(capture.clone());
        (capture, guard)
    }

    /// Les lignes d'un fichier, sans le saut de ligne final (qui doit exister).
    fn lines_of(path: &Path) -> Vec<String> {
        let text = fs::read_to_string(path).unwrap();
        assert!(text.is_empty() || text.ends_with('\n'), "ligne finale coupée : {path:?}");
        text.lines().map(str::to_owned).collect()
    }

    /// Les fichiers `mcp-legifrance*.jsonl` d'un répertoire, archives comprises, triés.
    fn journals_in(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy();
                name.starts_with("mcp-legifrance") && name.ends_with(".jsonl")
            })
            .collect();
        found.sort();
        found
    }

    fn at_day(day: u32) -> AuditRecord {
        record_at(chrono::Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap())
    }

    // ----- configuration -----------------------------------------------------------------

    #[test]
    fn open_rejects_an_empty_dir_and_a_zero_limit() {
        for (dir, limit) in [("", 1024), ("/tmp/x", 0)] {
            let error = JsonlAuditLog::open(Path::new(dir), limit).unwrap_err();
            assert_eq!(error.kind, AuditErrorKind::Io);
            assert_ne!(error.detail, "");
        }
    }

    #[tokio::test]
    async fn open_does_no_io_until_the_first_record() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("deep/audit");
        let log = journal(&dir);
        assert!(!dir.exists(), "open ne crée rien");
        assert!(!base.path().join("deep").exists());
        log.record(&record()).await.unwrap();
        assert_eq!(lines_of(&dir.join(ACTIVE_FILE)).len(), 1);
    }

    // ----- écriture ----------------------------------------------------------------------

    #[tokio::test]
    async fn record_appends_one_whole_line_per_call_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let log = journal(dir.path());
        for n in 0..3 {
            log.record(&AuditRecord { output_count_or_size: n, ..record() }).await.unwrap();
        }
        let lines = lines_of(&dir.path().join(ACTIVE_FILE));
        assert_eq!(lines.len(), 3);
        for (n, text) in lines.iter().enumerate() {
            let value: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(value["output_count_or_size"], n);
            assert_eq!(value["input_hash"], hash::input_hash(&record().input));
        }
        // Le verrou dédié existe et reste en place entre deux lignes.
        assert!(dir.path().join(LOCK_FILE).is_file());
    }

    #[tokio::test]
    async fn a_second_instance_appends_to_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        journal(dir.path()).record(&record()).await.unwrap();
        journal(dir.path()).record(&record()).await.unwrap();
        assert_eq!(lines_of(&dir.path().join(ACTIVE_FILE)).len(), 2);
    }

    #[tokio::test]
    async fn an_unencodable_record_is_an_encode_error_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let log = journal(dir.path());
        let error = log.record(&record_at(chrono::DateTime::<chrono::Utc>::MAX_UTC)).await;
        assert_eq!(error.unwrap_err().kind, AuditErrorKind::Encode);
        assert!(!dir.path().join(ACTIVE_FILE).exists());
    }

    // ----- rotation ----------------------------------------------------------------------

    #[tokio::test]
    async fn size_limit_rotates_into_a_dated_archive() {
        let dir = tempfile::tempdir().unwrap();
        let line_len = line::render(&record(), &hash::input_hash(&record().input)).unwrap().len();
        // Une ligne tient, deux dépassent.
        let log = JsonlAuditLog::open(dir.path(), u64::try_from(line_len).unwrap() + 10).unwrap();
        log.record(&record()).await.unwrap();
        log.record(&record()).await.unwrap();
        log.record(&record()).await.unwrap();
        let journals = journals_in(dir.path());
        let names: Vec<_> = journals
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "mcp-legifrance-2026-10-15-1.jsonl",
                "mcp-legifrance-2026-10-15.jsonl",
                "mcp-legifrance.jsonl"
            ]
        );
        for path in &journals {
            assert_eq!(lines_of(path).len(), 1, "{path:?}");
        }
    }

    #[tokio::test]
    async fn a_new_utc_day_archives_the_file_under_the_date_of_its_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let log = journal(dir.path());
        log.record(&at_day(15)).await.unwrap();
        log.record(&at_day(15)).await.unwrap();
        log.record(&at_day(16)).await.unwrap();
        // Une ligne en retard (jour antérieur) s'ajoute au fichier courant, sans rotation.
        log.record(&at_day(15)).await.unwrap();
        let archive = dir.path().join("mcp-legifrance-2026-10-15.jsonl");
        assert_eq!(lines_of(&archive).len(), 2);
        assert_eq!(lines_of(&dir.path().join(ACTIVE_FILE)).len(), 2);
        assert_eq!(journals_in(dir.path()).len(), 2);
    }

    #[tokio::test]
    async fn a_foreign_active_file_rotates_by_size_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ACTIVE_FILE), "not json at all\n").unwrap();
        let log = JsonlAuditLog::open(dir.path(), 1000).unwrap();
        log.record(&record()).await.unwrap();
        assert_eq!(lines_of(&dir.path().join(ACTIVE_FILE)).len(), 2, "sa date est illisible");
        let log = JsonlAuditLog::open(dir.path(), 100).unwrap();
        log.record(&record()).await.unwrap();
        // Archive nommée d'après la ligne entrante, faute de date lisible dans le fichier.
        let archive = dir.path().join("mcp-legifrance-2026-10-15.jsonl");
        assert_eq!(lines_of(&archive).len(), 2);
        assert_eq!(lines_of(&dir.path().join(ACTIVE_FILE)).len(), 1);
    }

    /// La rotation se fait sous le même verrou que l'écriture : tant qu'un autre écrivain le
    /// tient, rien n'est renommé ni écrit ; une fois libéré, la rotation a lieu.
    #[tokio::test]
    async fn rotation_under_same_lock() {
        let dir = tempfile::tempdir().unwrap();
        let line_len = line::render(&record(), &hash::input_hash(&record().input)).unwrap().len();
        let log = JsonlAuditLog::open(dir.path(), u64::try_from(line_len).unwrap() + 10)
            .unwrap()
            .with_lock_timeout(Duration::from_millis(100));
        log.record(&record()).await.unwrap();
        let first = fs::read_to_string(dir.path().join(ACTIVE_FILE)).unwrap();

        let other = File::open(dir.path().join(LOCK_FILE)).unwrap();
        fs4::FileExt::lock(&other).unwrap();
        let error = log.record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::WriterGone, "{}", error.detail);
        assert_eq!(journals_in(dir.path()), [dir.path().join(ACTIVE_FILE)], "aucun renommage");
        assert_eq!(fs::read_to_string(dir.path().join(ACTIVE_FILE)).unwrap(), first);

        fs4::FileExt::unlock(&other).unwrap();
        log.record(&record()).await.unwrap();
        assert_eq!(
            journals_in(dir.path()).len(),
            2,
            "la rotation a eu lieu une fois le verrou pris"
        );
    }

    // ----- concurrence -------------------------------------------------------------------

    /// Deux instances (donc deux descriptions de fichier et deux verrous, comme deux
    /// processus) et plusieurs tâches écrivent de longues lignes avec une rotation fréquente :
    /// chaque ligne, dans le fichier actif comme dans les archives, est un JSON complet.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn no_torn_lines_under_concurrent_writers() {
        const WRITERS: u64 = 2;
        const TASKS: u64 = 3;
        const LINES: u64 = 15;
        let dir = tempfile::tempdir().unwrap();
        let limit = 40 * 1024;
        let logs: Vec<_> = (0..WRITERS)
            .map(|_| Arc::new(JsonlAuditLog::open(dir.path(), limit).unwrap()))
            .collect();
        let mut tasks = Vec::new();
        for (writer, log) in (0..WRITERS).zip(&logs) {
            for task in 0..TASKS {
                let log = Arc::clone(log);
                tasks.push(tokio::spawn(async move {
                    for n in 0..LINES {
                        let id = writer * 1_000_000 + task * 1_000 + n;
                        let call = AuditRecord {
                            output_count_or_size: id,
                            status: AuditStatus::Error,
                            error: Some("é😀\"x\"".repeat(1500)),
                            ..record()
                        };
                        log.record(&call).await.unwrap();
                    }
                }));
            }
        }
        for task in tasks {
            task.await.unwrap();
        }

        let mut seen = Vec::new();
        for path in journals_in(dir.path()) {
            assert!(fs::metadata(&path).unwrap().len() <= limit, "{path:?} dépasse la limite");
            for text in lines_of(&path) {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert!(value["error"].as_str().unwrap().ends_with("\"x\""), "ligne coupée");
                seen.push(value["output_count_or_size"].as_u64().unwrap());
            }
        }
        seen.sort_unstable();
        let mut expected: Vec<u64> = (0..WRITERS)
            .flat_map(|w| {
                (0..TASKS).flat_map(move |t| (0..LINES).map(move |n| w * 1_000_000 + t * 1_000 + n))
            })
            .collect();
        expected.sort_unstable();
        assert_eq!(seen, expected, "chaque ligne présente une fois, aucune perdue");
        assert!(journals_in(dir.path()).len() > 2, "la rotation a bien eu lieu");
    }

    // ----- pannes ------------------------------------------------------------------------

    #[tokio::test]
    async fn unwritable_dir_is_an_error_not_a_panic() {
        let base = tempfile::tempdir().unwrap();

        // Un fichier à la place d'un parent : le répertoire ne peut pas être créé.
        let blocker = base.path().join("blocker");
        fs::write(&blocker, "x").unwrap();
        let log = journal(&blocker.join("audit"));
        let error = log.record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::Io);
        assert!(
            error.detail.contains("blocker"),
            "le chemin est dans le détail : {}",
            error.detail
        );
        assert!(!error.to_string().contains("blocker"), "mais pas dans le message");

        // Le chemin du journal est un répertoire.
        let dir = base.path().join("clash");
        fs::create_dir_all(dir.join(ACTIVE_FILE)).unwrap();
        let error = journal(&dir).record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::Io, "{}", error.detail);

        // Le chemin du répertoire est un fichier.
        let error = journal(&blocker).record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::Io, "{}", error.detail);
    }

    /// Sous root, les droits ne s'appliquent pas (06 §2.5) : le test ne vérifie alors rien.
    #[cfg(unix)]
    #[tokio::test]
    async fn read_only_dir_maps_to_permission_denied_then_recovers() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let locked = base.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        // Une seule passe, ou aucune sous root : une boucle plutôt qu'un `if`, pour que la
        // couverture ne compte pas une branche « droits ignorés » qu'un compte ordinaire ne prend
        // jamais.
        let rounds = usize::from(fs::File::create(locked.join("probe")).is_err());
        for _ in 0..rounds {
            // Sous un répertoire en lecture seule, le répertoire d'audit ne peut pas être créé.
            let nested = journal(&locked.join("audit"));
            let error = nested.record(&record()).await.unwrap_err();
            assert_eq!(error.kind, AuditErrorKind::PermissionDenied, "{}", error.detail);
            // Dans le répertoire lui-même, c'est le fichier de verrou qui ne peut pas être créé.
            let log = journal(&locked);
            let error = log.record(&record()).await.unwrap_err();
            assert_eq!(error.kind, AuditErrorKind::PermissionDenied, "{}", error.detail);

            // Un journal existant mais illisible : même genre d'erreur.
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
            let active = locked.join(ACTIVE_FILE);
            fs::write(&active, "").unwrap();
            fs::set_permissions(&active, fs::Permissions::from_mode(0o000)).unwrap();
            let error = log.record(&record()).await.unwrap_err();
            assert_eq!(error.kind, AuditErrorKind::PermissionDenied, "{}", error.detail);

            // Les droits reviennent : le même journal écrit, sans être reconstruit.
            fs::set_permissions(&active, fs::Permissions::from_mode(0o600)).unwrap();
            log.record(&record()).await.unwrap();
            assert_eq!(lines_of(&active).len(), 1);
        }
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[tokio::test]
    async fn a_panicking_writer_is_writer_gone_and_the_next_call_gets_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let log = journal(dir.path()).with_sender(
            spawn_writer(|_, _| std::panic::resume_unwind(Box::new("writer crashed"))).unwrap(),
        );
        let error = log.record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::WriterGone);
        log.record(&record()).await.unwrap();
        assert_eq!(lines_of(&dir.path().join(ACTIVE_FILE)).len(), 1);
    }

    #[tokio::test]
    async fn a_stopped_writer_is_writer_gone_at_send_time() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, jobs) = mpsc::channel(1);
        drop(jobs);
        let log = journal(dir.path()).with_sender(sender);
        let error = log.record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::WriterGone);
        // Le fil mort est oublié : l'appel suivant en crée un.
        log.record(&record()).await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_writer_creation_is_reported_then_retried() {
        let dir = tempfile::tempdir().unwrap();
        let log = journal(dir.path())
            .with_spawn(|_| Err(io::Error::from(io::ErrorKind::PermissionDenied)));
        let error = log.record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::PermissionDenied);
        assert!(error.detail.contains("writer thread"), "{}", error.detail);
        assert!(!dir.path().join(ACTIVE_FILE).exists(), "rien n'a été écrit");

        // Le même journal, avec un fil qui se crée, écrit : l'échec n'est pas mémorisé.
        let log = log.with_spawn(spawn_store);
        log.record(&record()).await.unwrap();
        assert_eq!(lines_of(&dir.path().join(ACTIVE_FILE)).len(), 1);
    }

    #[tokio::test]
    async fn forgetting_a_stale_writer_keeps_the_current_one() {
        let dir = tempfile::tempdir().unwrap();
        let log = journal(dir.path());
        log.record(&record()).await.unwrap();
        let (stale, _jobs) = mpsc::channel(1);
        log.forget(&stale);
        assert!(log.writer.lock().unwrap().is_some(), "un autre fil a pris le relais");
        log.record(&record()).await.unwrap();
        assert_eq!(lines_of(&dir.path().join(ACTIVE_FILE)).len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_frozen_writer_times_out_as_writer_gone() {
        let dir = tempfile::tempdir().unwrap();
        let (release, gate) = std_mpsc::channel::<()>();
        let log = journal(dir.path()).with_sender(
            spawn_writer(move |_, _| {
                let _ = gate.recv();
                Ok(Written { bytes: 0, rotated: false })
            })
            .unwrap(),
        );
        let error = log.record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::WriterGone);
        assert!(error.detail.contains("in time"), "{}", error.detail);
        drop(release);
    }

    /// Contrat du port : une ligne remise au fil est écrite en entier même si le futur de
    /// `record` est abandonné ensuite.
    #[tokio::test]
    async fn a_dropped_record_future_still_gets_its_line_written() {
        let dir = tempfile::tempdir().unwrap();
        let (release, gate) = std_mpsc::channel::<()>();
        let (done, finished) = std_mpsc::channel::<()>();
        let mut store = Store::new(dir.path().to_owned(), 1 << 20, Duration::from_secs(5));
        let log = journal(dir.path()).with_sender(
            spawn_writer(move |line, date| {
                let _ = gate.recv();
                let written = store.append(line, date);
                let _ = done.send(());
                written
            })
            .unwrap(),
        );
        let abandoned = tokio::time::timeout(Duration::ZERO, log.record(&record())).await;
        assert!(abandoned.is_err(), "l'appelant a renoncé avant l'accusé");
        assert!(!dir.path().join(ACTIVE_FILE).exists(), "le fil attend encore");
        // Le fil d'écriture prévient quand la ligne est écrite : pas de sondage, donc pas de course
        // entre le test et le fil (qui ferait varier la couverture selon la vitesse de la machine).
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        let active = dir.path().join(ACTIVE_FILE);
        assert_eq!(lines_of(&active).len(), 1);
    }

    // ----- permissions et liens ----------------------------------------------------------

    #[cfg(unix)]
    #[tokio::test]
    async fn file_mode_0600_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("audit");
        journal(&dir).record(&record()).await.unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(ACTIVE_FILE)), 0o600);
        assert_eq!(mode(&dir.join(LOCK_FILE)), 0o600);
        // L'archive de la rotation garde les droits du journal : c'est le même fichier renommé.
        let rotating = JsonlAuditLog::open(&dir, 10).unwrap();
        rotating.record(&record()).await.unwrap();
        assert_eq!(mode(&dir.join("mcp-legifrance-2026-10-15.jsonl")), 0o600);
        assert_eq!(mode(&dir.join(ACTIVE_FILE)), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn existing_broad_permissions_are_kept_and_warned_once() {
        use std::os::unix::fs::PermissionsExt;
        let (capture, _guard) = local_capture();
        let dir = tempfile::tempdir().unwrap();
        let active = dir.path().join(ACTIVE_FILE);
        fs::write(&active, "{\"ts\":\"2026-10-15T00:00:00.000+00:00\"}\n").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&active, fs::Permissions::from_mode(0o644)).unwrap();

        let mut store = Store::new(dir.path().to_owned(), 1 << 20, Duration::from_secs(1));
        store.append("{}\n", "2026-10-15").unwrap();
        let warnings = capture.events_at(Level::WARN);
        assert_eq!(warnings.len(), 2, "répertoire puis fichier : {warnings:?}");
        assert!(warnings[0]["message"].contains("directory"), "{warnings:?}");
        assert!(warnings[1]["message"].contains("journal"), "{warnings:?}");
        assert_eq!(warnings[0]["mode"], "755");
        assert_eq!(warnings[1]["mode"], "644");

        store.append("{}\n", "2026-10-15").unwrap();
        assert_eq!(capture.events_at(Level::WARN).len(), 2, "un seul avertissement par processus");
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(dir.path()), 0o755, "les droits préexistants ne sont pas modifiés");
        assert_eq!(mode(&active), 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn private_permissions_do_not_warn() {
        let (capture, _guard) = local_capture();
        let dir = tempfile::tempdir().unwrap();
        // `tempdir` suit l'umask : on fixe les droits que l'application elle-même poserait.
        fs::set_permissions(dir.path(), std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        let mut store = Store::new(dir.path().to_owned(), 1 << 20, Duration::from_secs(1));
        store.append("{}\n", "2026-10-15").unwrap();
        store.append("{}\n", "2026-10-15").unwrap();
        assert_eq!(capture.events_at(Level::WARN).len(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn audit_file_symlink_is_refused() {
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("elsewhere.jsonl");
        fs::write(&target, "").unwrap();
        let dir = base.path().join("audit");
        fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&target, dir.join(ACTIVE_FILE)).unwrap();
        let error = journal(&dir).record(&record()).await.unwrap_err();
        assert_eq!(error.kind, AuditErrorKind::Io);
        assert!(error.detail.contains("symbolic link"), "{}", error.detail);
        assert_eq!(fs::read_to_string(&target).unwrap(), "", "rien n'est écrit à travers le lien");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlinked_directory_is_accepted() {
        let base = tempfile::tempdir().unwrap();
        let real = base.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        journal(&link).record(&record()).await.unwrap();
        assert_eq!(lines_of(&real.join(ACTIVE_FILE)).len(), 1);
    }

    // ----- verrou et classement des erreurs ----------------------------------------------

    #[test]
    fn acquire_retries_while_busy_then_times_out() {
        let mut attempts = 0;
        acquire_with(
            || {
                attempts += 1;
                if attempts < 3 { Err(TryLockError::WouldBlock) } else { Ok(()) }
            },
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(attempts, 3);

        let started = Instant::now();
        let error =
            acquire_with(|| Err(TryLockError::WouldBlock), Duration::from_millis(30)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn acquire_reports_a_real_lock_error_at_once() {
        let mut attempts = 0;
        let error = acquire_with(
            || {
                attempts += 1;
                Err(TryLockError::Error(io::Error::from(io::ErrorKind::Unsupported)))
            },
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!((attempts, error.kind()), (1, io::ErrorKind::Unsupported));
    }

    #[test]
    fn io_error_kind_maps_to_audit_error_kind() {
        let cases = [
            (io::ErrorKind::StorageFull, AuditErrorKind::DiskFull),
            (io::ErrorKind::PermissionDenied, AuditErrorKind::PermissionDenied),
            (io::ErrorKind::TimedOut, AuditErrorKind::WriterGone),
            (io::ErrorKind::NotFound, AuditErrorKind::Io),
            (io::ErrorKind::Other, AuditErrorKind::Io),
        ];
        for (io_kind, expected) in cases {
            let error = io::Error::from(io_kind);
            assert_eq!(kind_of(&error), expected, "{io_kind:?}");
            assert_eq!(spawn_failure(error).kind, expected);
        }
        let error = failure(Path::new("/var/audit"), &io::Error::from(io::ErrorKind::StorageFull));
        assert_eq!(error.kind, AuditErrorKind::DiskFull);
        assert!(error.detail.contains("/var/audit"));
        assert_eq!(error.to_string(), "audit log unavailable (DiskFull)");
    }

    // ----- traces ------------------------------------------------------------------------

    #[tokio::test]
    async fn a_write_emits_the_audit_write_span_and_a_failure_emits_an_error() {
        let (capture, _guard) = local_capture();
        let dir = tempfile::tempdir().unwrap();
        let log = JsonlAuditLog::open(dir.path(), 100).unwrap();
        log.record(&record()).await.unwrap();
        log.record(&record()).await.unwrap();
        // Le souscripteur de test n'a pas de notion de portée : ces trois appels n'ont aucun effet.
        let scope = Id::from_u64(1);
        capture.enter(&scope);
        capture.exit(&scope);
        capture.record_follows_from(&scope, &scope);
        let spans = capture.spans("audit_write");
        assert_eq!(spans.len(), 2);
        let expected_hash = hash::input_hash(&record().input);
        assert_eq!(spans[0]["input_hash"], expected_hash[..12]);
        assert_eq!(spans[0]["rotated"], "false");
        assert_eq!(spans[1]["rotated"], "true");
        assert!(spans[0]["bytes"].parse::<usize>().unwrap() > 100);
        assert_eq!(capture.events_at(Level::ERROR).len(), 0);

        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "x").unwrap();
        journal(&blocker).record(&record()).await.unwrap_err();
        let errors = capture.events_at(Level::ERROR);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["kind"], "Io");
        assert!(errors[0]["detail"].contains("blocker"), "{errors:?}");
        assert_eq!(errors[0]["message"], "audit write failed");
    }

    /// Souscripteur minimal : `tracing-subscriber` est confiné à `lf-app` (R6), ce crate ne
    /// l'emploie donc pas en développement.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Seen>>);

    type Fields = BTreeMap<&'static str, String>;

    #[derive(Default)]
    struct Seen {
        events: Vec<(Level, Fields)>,
        spans: Vec<(&'static str, Fields)>,
    }

    struct Collect<'a>(&'a mut Fields);

    impl Visit for Collect<'_> {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name(), value.to_owned());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name(), format!("{value:?}"));
        }
    }

    impl Capture {
        fn events_at(&self, level: Level) -> Vec<Fields> {
            let seen = self.0.lock().unwrap();
            seen.events.iter().filter(|(l, _)| *l == level).map(|(_, f)| f.clone()).collect()
        }

        fn spans(&self, name: &str) -> Vec<Fields> {
            let seen = self.0.lock().unwrap();
            seen.spans.iter().filter(|(n, _)| *n == name).map(|(_, f)| f.clone()).collect()
        }
    }

    impl Subscriber for Capture {
        fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
            Interest::sometimes()
        }

        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, attributes: &Attributes<'_>) -> Id {
            let mut fields = Fields::new();
            attributes.record(&mut Collect(&mut fields));
            let mut seen = self.0.lock().unwrap();
            seen.spans.push((attributes.metadata().name(), fields));
            Id::from_u64(u64::try_from(seen.spans.len()).unwrap())
        }

        fn record(&self, span: &Id, values: &Record<'_>) {
            let mut seen = self.0.lock().unwrap();
            let index = usize::try_from(span.into_u64()).unwrap() - 1;
            values.record(&mut Collect(&mut seen.spans[index].1));
        }

        fn record_follows_from(&self, _: &Id, _: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut fields = Fields::new();
            event.record(&mut Collect(&mut fields));
            self.0.lock().unwrap().events.push((*event.metadata().level(), fields));
        }

        fn enter(&self, _: &Id) {}

        fn exit(&self, _: &Id) {}
    }

    // ----- mesures `#[ignore]` : V-165 (D22) et V-166 ------------------------------------

    fn acl_listing(path: &Path) -> String {
        #[cfg(windows)]
        {
            match std::process::Command::new("icacls").arg(path).output() {
                Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
                Err(error) => format!("icacls unavailable: {error}"),
            }
        }
        #[cfg(not(windows))]
        {
            format!("not Windows: no ACL to report for {}", path.display())
        }
    }

    /// V-165 (D22) : relève les ACL réelles du journal, du verrou et du dossier d'audit sous
    /// Windows (`icacls`), pour décider si les ACL héritées suffisent. Mesure, pas assertion :
    /// à lancer à la main avec `--run-ignored`. Sous `cargo llvm-cov` (cfg `coverage`), elle
    /// tourne pour que le code de mesure reste couvert ; hors Windows, elle ne relève rien.
    #[tokio::test]
    #[cfg_attr(not(coverage), ignore = "mesure V-165 (D22) : ACL Windows, à lancer à la main")]
    #[expect(clippy::print_stderr, reason = "rapport de mesure destiné à la personne qui le lance")]
    async fn windows_acl_report() {
        let dir = tempfile::tempdir().unwrap();
        journal(dir.path()).record(&record()).await.unwrap();
        for path in
            [dir.path().to_owned(), dir.path().join(ACTIVE_FILE), dir.path().join(LOCK_FILE)]
        {
            eprintln!("--- {}\n{}", path.display(), acl_listing(&path));
        }
    }

    /// V-166 : coût de `sync_data` par ligne sur le système de fichiers de `TMPDIR` (SSD,
    /// disque réseau, Windows). Mesure, pas assertion.
    #[tokio::test]
    #[cfg_attr(not(coverage), ignore = "mesure V-166 : coût de sync_data, à lancer à la main")]
    #[expect(clippy::print_stderr, reason = "rapport de mesure destiné à la personne qui le lance")]
    async fn sync_data_cost_report() {
        const LINES: usize = 20;
        let dir = tempfile::tempdir().unwrap();
        let log = journal(dir.path());
        let mut costs = Vec::new();
        for n in 0..LINES {
            let started = Instant::now();
            let call = AuditRecord { output_count_or_size: u64::try_from(n).unwrap(), ..record() };
            log.record(&call).await.unwrap();
            costs.push(started.elapsed());
        }
        costs.sort_unstable();
        eprintln!(
            "sync_data par ligne ({LINES} lignes, {}) : min {:?}, médiane {:?}, max {:?}",
            dir.path().display(),
            costs[0],
            costs[LINES / 2],
            costs[LINES - 1]
        );
    }
}
