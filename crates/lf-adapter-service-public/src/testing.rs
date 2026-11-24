//! Outils des tests du crate : mini-archives réelles, faux téléchargeur aux réponses écrites
//! d'avance, retenu par une barrière quand le test veut un téléchargement en cours. Compilé sous
//! `#[cfg(test)]` seulement.

use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    io,
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicI64, Ordering},
    },
};

use chrono::{DateTime, Utc};
use lf_application::ports::{Clock, GuidanceError};
use lf_domain::{date::CivilDate, guidance::Audience};
use tokio::sync::Semaphore;
use tracing_subscriber::fmt::MakeWriter;

use crate::{
    client::{Download, Fetched},
    store::Fetch,
};

/// Mini-archive de Particuliers (T-MS-02) : dix pages et le menu.
pub(crate) const PART: &[u8] = include_bytes!("../tests/fixtures/archives/part-vosdroits-mini.zip");

/// Mini-archive d'Entreprendre (T-MS-02) : deux pages et le menu.
pub(crate) const PRO: &[u8] = include_bytes!("../tests/fixtures/archives/pro-vosdroits-mini.zip");

pub(crate) type Reply = Result<Fetched, GuidanceError>;

/// Faux téléchargeur : réponses dans l'ordre, appels notés, barrière avant chaque réponse.
pub(crate) struct Fake {
    replies: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<(Audience, Option<String>)>>,
    gate: Semaphore,
}

impl Fake {
    /// Répond sans attendre.
    pub(crate) fn open(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Self::with(replies, Semaphore::MAX_PERMITS)
    }

    /// Retient chaque réponse jusqu'à [`Fake::release`].
    pub(crate) fn held(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Self::with(replies, 0)
    }

    fn with(replies: impl IntoIterator<Item = Reply>, permits: usize) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
            gate: Semaphore::new(permits),
        })
    }

    /// Laisse partir une réponse retenue.
    pub(crate) fn release(&self) {
        self.gate.add_permits(1);
    }

    pub(crate) fn calls(&self) -> Vec<(Audience, Option<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Fetch for Arc<Fake> {
    fn fetch(
        &self,
        audience: Audience,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<Fetched, GuidanceError>> + Send {
        let fake = Arc::clone(self);
        fake.calls.lock().unwrap().push((audience, etag.map(str::to_owned)));
        async move {
            fake.gate.acquire().await.unwrap().forget();
            fake.replies.lock().unwrap().pop_front().unwrap()
        }
    }
}

pub(crate) fn fresh(body: &[u8], etag: &str) -> Fetched {
    Fetched::Fresh(Download {
        body: body.to_vec(),
        etag: Some(etag.to_owned()),
        last_modified: None,
    })
}

/// Panne 500 du serveur, telle que le client la rend.
pub(crate) fn server_error() -> GuidanceError {
    GuidanceError::Upstream {
        status: Some(500),
        retryable: true,
        detail: "server error".to_owned(),
    }
}

/// Un appel noté par le faux téléchargeur.
pub(crate) fn call(audience: Audience, etag: Option<&str>) -> (Audience, Option<String>) {
    (audience, etag.map(str::to_owned))
}

/// 2026-11-24T12:00:00Z, en secondes depuis le 1970-01-01.
pub(crate) const MIDDAY: i64 = 1_795_521_600;

/// Horloge civile de test, réglée à la main et partagée par ses clones.
#[derive(Debug, Clone)]
pub(crate) struct TestWall(Arc<AtomicI64>);

impl TestWall {
    /// Horloge arrêtée à `timestamp` (secondes depuis le 1970-01-01, UTC).
    pub(crate) fn at(timestamp: i64) -> Self {
        Self(Arc::new(AtomicI64::new(timestamp)))
    }

    /// Avance (ou recule) l'horloge de `seconds`.
    pub(crate) fn shift(&self, seconds: i64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for TestWall {
    fn now_utc(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.0.load(Ordering::SeqCst), 0).unwrap()
    }

    /// Jamais lue par l'adapter : une date fixe.
    fn today_paris(&self) -> CivilDate {
        CivilDate::new(2026, 11, 24).unwrap()
    }
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
        assert_eq!(wall.now_utc().to_rfc3339(), "2026-11-24T12:00:00+00:00");
        other.shift(-3_600);
        assert_eq!(wall.now_utc().to_rfc3339(), "2026-11-24T11:00:00+00:00");
        assert_eq!(wall.today_paris(), CivilDate::new(2026, 11, 24).unwrap());
    }

    #[test]
    fn the_journal_holds_the_events_of_this_thread_until_it_is_read() {
        start_journal();
        tracing::warn!(count = 3, "événement de test");
        let text = journal();
        assert!(text.contains(" WARN ") && text.contains("événement de test count=3"));
        assert_eq!(journal(), "");
        // L'écrivain n'a rien à vider.
        assert!(io::Write::flush(&mut ThreadLog).is_ok());
    }
}
