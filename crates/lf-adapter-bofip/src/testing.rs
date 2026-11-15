//! Outils des tests du crate : horloge murale réglable, réponses réelles du portail, journal
//! capté. Compilé sous `#[cfg(test)]` seulement.

use std::{
    cell::RefCell,
    fs,
    io::{self, Write as _},
    path::PathBuf,
    sync::{
        Arc, Once,
        atomic::{AtomicI64, Ordering},
    },
};

use chrono::{DateTime, Utc};
use flate2::{Compression, write::GzEncoder};
use lf_application::ports::Clock;
use lf_domain::date::CivilDate;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};
use tracing_subscriber::fmt::MakeWriter;

/// Le 2026-11-15 à 12:00:00 UTC, en secondes depuis le 1970-01-01 : le jour 20 772.
pub(crate) const MIDDAY: i64 = 1_794_744_000;

/// Horloge murale réglable ; les clones partagent le même instant.
#[derive(Debug, Clone)]
pub(crate) struct TestWall(Arc<AtomicI64>);

impl TestWall {
    /// Horloge arrêtée à `timestamp` (secondes depuis le 1970-01-01, UTC).
    pub(crate) fn at(timestamp: i64) -> Self {
        Self(Arc::new(AtomicI64::new(timestamp)))
    }

    /// Règle l'horloge sur `timestamp`.
    pub(crate) fn set(&self, timestamp: i64) {
        self.0.store(timestamp, Ordering::SeqCst);
    }
}

impl Clock for TestWall {
    fn now_utc(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.0.load(Ordering::SeqCst), 0).unwrap()
    }

    /// Jamais lue par l'adapter : une date fixe.
    fn today_paris(&self) -> CivilDate {
        CivilDate::new(2026, 11, 15).unwrap()
    }
}

/// Dossier des réponses réelles du portail (`tests/fixtures`).
pub(crate) fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// Octets de la réponse réelle `name`.
pub(crate) fn fixture(name: &str) -> Vec<u8> {
    fs::read(fixtures_dir().join(name)).unwrap()
}

/// `bytes` compressés en gzip.
pub(crate) fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// Base `http://127.0.0.1:<port>` d'un serveur d'une seule connexion, qui répond `response` tel
/// quel puis ferme : de quoi couper un corps en route, ou répondre en clair à du TLS.
pub(crate) async fn raw_server(response: &[u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let response = response.to_vec();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).await.unwrap();
        stream.write_all(&response).await.unwrap();
        // Le client peut avoir déjà fermé : la fermeture n'a pas à réussir.
        let _ = stream.shutdown().await;
    });
    format!("http://{address}")
}

/// Base `http://127.0.0.1:<port>` d'un port où personne n'écoute.
pub(crate) async fn closed_port() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    format!("http://{}", listener.local_addr().unwrap())
}

thread_local! {
    /// Journal du fil courant : chaque test tourne sur son propre fil.
    static JOURNAL: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Écrit dans le journal du fil courant : `io::Write` et `MakeWriter` à la fois.
#[derive(Clone, Copy)]
struct ThreadLog;

impl io::Write for ThreadLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        JOURNAL.with_borrow_mut(|journal| journal.extend_from_slice(buf));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MakeWriter<'_> for ThreadLog {
    type Writer = Self;

    fn make_writer(&self) -> Self::Writer {
        *self
    }
}

/// Vide le journal du fil courant et s'assure que tous les événements `tracing`, jusqu'au
/// niveau `TRACE`, y sont écrits. À appeler au début d'un test qui lit [`journal`].
///
/// L'abonné est global et installé une seule fois : un abonné local changerait l'intérêt mis en
/// cache par chaque point d'instrumentation, et un test voisin pourrait le figer sur « jamais ».
pub(crate) fn start_journal() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(ThreadLog)
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::set_global_default(subscriber).unwrap();
    });
    JOURNAL.with_borrow_mut(Vec::clear);
}

/// Le journal écrit par ce fil depuis [`start_journal`], vidé par la lecture.
pub(crate) fn journal() -> String {
    String::from_utf8(JOURNAL.with_borrow_mut(std::mem::take)).unwrap()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_test_wall_is_set_by_hand_and_shared_by_its_clones() {
        let wall = TestWall::at(MIDDAY);
        let other = wall.clone();
        assert_eq!(wall.now_utc().to_rfc3339(), "2026-11-15T12:00:00+00:00");
        other.set(0);
        assert_eq!(wall.now_utc(), DateTime::UNIX_EPOCH);
        assert_eq!(wall.today_paris(), CivilDate::new(2026, 11, 15).unwrap());
    }

    #[test]
    fn the_journal_holds_the_events_of_this_thread_until_it_is_read() {
        start_journal();
        tracing::warn!(count = 3, "événement de test");
        let text = journal();
        assert!(
            text.contains("WARN") && text.contains("événement de test") && text.contains("count=3")
        );
        assert_eq!(journal(), "");
        tracing::debug!("un autre");
        start_journal();
        assert_eq!(journal(), "");
        // L'écrivain n'a rien à vider.
        assert!(io::Write::flush(&mut ThreadLog).is_ok());
    }

    #[test]
    fn fixtures_are_read_from_the_tests_directory() {
        assert!(fixtures_dir().ends_with("tests/fixtures"));
        assert_eq!(fixture("search-empty.json"), br#"{"total_count": 0, "results": []}"#);
    }

    /// Le `README` voisin des réponses enregistrées dit d'où elles viennent (adresse, date,
    /// éditeur, licence) et cite chaque fichier avec sa taille : ni fichier oublié, ni fichier
    /// disparu, ni réponse retouchée sans que la liste suive.
    #[test]
    fn fixtures_are_listed_with_their_source() {
        let readme = String::from_utf8(fixture("README.md")).unwrap();
        let source = [crate::doctrine::RECORDS_URL, "2026-11-15", "DGFiP", "Licence Ouverte 2.0"];
        assert_eq!(source.map(|needle| readme.contains(needle)), [true; 4]);

        let mut present: Vec<String> = fs::read_dir(fixtures_dir())
            .unwrap()
            .map(|entry| entry.unwrap())
            .map(|entry| {
                (entry.file_name().into_string().unwrap(), entry.metadata().unwrap().len())
            })
            .filter(|(name, _)| name != "README.md")
            .map(|(name, size)| format!("- `{name}` ({size} octets)"))
            .collect();
        present.sort();
        let listed: Vec<&str> = readme
            .lines()
            .filter(|line| line.starts_with("- `"))
            .map(|line| line.split(" : ").next().unwrap())
            .collect();
        assert_eq!(listed, present);
        assert_eq!(listed.len(), 14);
    }
}
