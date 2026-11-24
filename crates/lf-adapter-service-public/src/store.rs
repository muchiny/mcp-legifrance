//! Archive et index de chaque public, en mémoire (ADR-0033 décision 3 ; T-MS-23).
//!
//! - **Un téléchargement à la fois par public**, dans sa propre tâche (`tokio::spawn`), partagé par
//!   les appels qui l'attendent. Le délai du téléchargement est celui du client (300 s).
//! - **Attente d'un appel** : au plus l'échéance publiée ([`Deadline::current_or`]) moins
//!   [`WAIT_MARGIN`] ; ensuite l'appel rend [`GuidanceError::Downloading`], et le téléchargement
//!   continue. Sans la marge, la garde dure d'`Audited` répondrait avant l'adapter. Quand
//!   l'archive précédente se sert encore (revalidation d'une archive en sursis), l'attente est
//!   bornée à [`STALE_WAIT`] : sur une bonne liaison, l'archive du jour arrive avant ; sinon
//!   l'appel reçoit la précédente sans attendre la fin d'un serveur lent.
//! - **Fraîcheur** : une archive se sert sans requête pendant [`FRESH_FOR`] après son téléchargement
//!   ou sa dernière revalidation ; ensuite, l'appel suivant la revalide (`If-None-Match`, un `304`
//!   la garde). L'âge est le plus long des temps mesurés par l'horloge monotone et par l'horloge
//!   civile du port [`Clock`] : la monotone ne compte pas la mise en veille de la machine sous
//!   Linux et macOS, la civile peut reculer.
//! - **Échec** (archive refusée, revalidation échouée, refus 429) : l'archive précédente se sert
//!   encore, [`STALE_FOR`] de plus au plus ; aucune nouvelle tentative avant [`RETRY_AFTER_FAILURE`]
//!   (au-delà, pendant le repos du client après un 429, la tentative est refusée sans réseau). Une
//!   archive refusée par le lecteur a été téléchargée en entier : les refus qui se suivent espacent
//!   les tentatives ([`refusal_gap`] : une minute, un quart d'heure, une heure, puis six heures),
//!   jusqu'au premier succès. Sans archive servable, l'appel rend l'erreur de la dernière
//!   tentative ; un refus 429 y donne le repos qui reste.
//! - **Tâche du téléchargement** : le téléchargement et la lecture courent dans une tâche à part,
//!   que la tâche du public attend ; si elle panique, l'échec est enregistré comme un autre et le
//!   public n'en reste pas bloqué.
//! - **Index construit au chargement** sur un fil bloquant (`spawn_blocking`) : en-tête de chaque
//!   fiche (`F…`) et page de navigation (`N…`), lu à 64 Kio au plus ; ressources (`R…`) et
//!   fichiers nommés exclus ; chemin de chaque page tiré du menu du public (Q41). L'index n'est
//!   servi qu'entier : il remplace l'ancien d'un coup. Une page dont l'en-tête ne se lit pas est
//!   écartée et comptée ; une archive sans aucune page lisible, ou dont l'index dépasse son budget
//!   ([`GuidanceIndex::build`]), est refusée. Chaque chargement est journalisé (`info`, nombre de
//!   pages, écartées, ignorées), avec un avertissement si plus d'une page sur [`DROPPED_WARN_ONE_IN`]
//!   est écartée ; chaque échec aussi (`warn`, sans adresse).

use std::{
    fmt,
    future::Future,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use chrono::{DateTime, Utc};
use lf_application::{
    deadline::Deadline,
    ports::{Clock, GuidanceError},
};
use lf_domain::guidance::{Audience, GuidanceIndex, GuidanceMenu, SheetId};
use tokio::{
    sync::watch,
    time::{Instant, timeout},
};

use crate::{
    archive::{Archive, ENTRY_MAX_BYTES},
    client::{ArchiveClient, Download, Fetched},
    page, xml,
};

/// Durée pendant laquelle une archive se sert sans requête.
pub(crate) const FRESH_FOR: Duration = Duration::from_secs(24 * 3_600);

/// Durée supplémentaire pendant laquelle une archive qui n'a pas pu être revalidée se sert encore.
pub(crate) const STALE_FOR: Duration = Duration::from_secs(48 * 3_600);

/// Écart minimal entre une tentative échouée et la suivante.
pub(crate) const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);

/// Marge laissée avant l'échéance de l'appel.
pub(crate) const WAIT_MARGIN: Duration = Duration::from_secs(1);

/// Attente d'un appel quand l'archive précédente se sert encore.
pub(crate) const STALE_WAIT: Duration = Duration::from_secs(10);

/// Avertissement quand plus d'une page sur ce nombre est écartée au chargement.
const DROPPED_WARN_ONE_IN: usize = 20;

/// Nom du menu dans l'archive.
const MENU: &str = "menu.xml";

/// Ce qui télécharge l'archive d'un public : le client en production, un faux dans les tests.
pub(crate) trait Fetch: Send + Sync + 'static {
    /// Un téléchargement de l'archive de `audience`, conditionnel si `etag` est donné.
    fn fetch(
        &self,
        audience: Audience,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<Fetched, GuidanceError>> + Send;
}

impl Fetch for ArchiveClient {
    fn fetch(
        &self,
        audience: Audience,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<Fetched, GuidanceError>> + Send {
        Self::fetch(self, audience, etag)
    }
}

/// Une archive chargée : ses entrées, l'index de ses pages, son menu, ses en-têtes HTTP.
pub(crate) struct Loaded {
    pub(crate) archive: Archive,
    pub(crate) index: GuidanceIndex,
    pub(crate) menu: GuidanceMenu,
    pub(crate) etag: Option<String>,
    pub(crate) last_modified: Option<DateTime<Utc>>,
}

impl fmt::Debug for Loaded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Loaded")
            .field("bytes", &self.archive.byte_len())
            .field("entries", &self.archive.len())
            .field("ignored", &self.archive.ignored())
            .field("pages", &self.index.len())
            .field("menu_places", &self.menu.len())
            .field("etag", &self.etag)
            .field("last_modified", &self.last_modified)
            .finish()
    }
}

/// Charge une archive reçue : lecture du répertoire, du menu et des en-têtes des pages.
///
/// # Errors
///
/// [`GuidanceError::Decode`] si le lecteur refuse l'archive, ou si aucune page ne se lit.
pub(crate) fn load(audience: Audience, download: Download) -> Result<Loaded, GuidanceError> {
    let archive = Archive::open(download.body)
        .map_err(|error| decode(format!("archive refused: {error}")))?;
    let menu = read_menu(&archive);
    let mut pages = Vec::new();
    let mut candidates = 0_usize;
    for name in archive.names() {
        let Some(id) = page_id(name) else { continue };
        candidates = candidates.saturating_add(1);
        let page = archive
            .head(name)
            .ok()
            .flatten()
            .and_then(|head| xml::parse_head(&head).ok())
            .and_then(|root| page::header(&root, audience, &id, &menu).ok());
        pages.extend(page);
    }
    if pages.is_empty() {
        return Err(decode("archive has no readable page".to_owned()));
    }
    let index =
        GuidanceIndex::build(pages).map_err(|error| decode(format!("archive refused: {error}")))?;
    let dropped = candidates.saturating_sub(index.len());
    tracing::info!(
        audience = audience.name(),
        pages = index.len(),
        dropped,
        ignored = archive.ignored(),
        "Service-Public archive loaded"
    );
    if dropped.saturating_mul(DROPPED_WARN_ONE_IN) > candidates {
        tracing::warn!(
            audience = audience.name(),
            dropped,
            candidates,
            "Service-Public archive: many pages could not be read"
        );
    }
    Ok(Loaded { index, archive, menu, etag: download.etag, last_modified: download.last_modified })
}

/// Numéro d'une fiche ou d'une page de navigation nommée `{numéro}.xml`, écrit tel quel. Le
/// lecteur d'archive ne garde que des noms en `.xml`.
fn page_id(name: &str) -> Option<SheetId> {
    let stem = name.strip_suffix(".xml").unwrap_or_default();
    SheetId::parse(stem).ok().filter(|id| id.as_str() == stem)
}

/// Menu du public ; vide s'il manque ou ne se lit pas (les chemins sont alors omis).
fn read_menu(archive: &Archive) -> GuidanceMenu {
    let entries = archive
        .read(MENU, ENTRY_MAX_BYTES)
        .ok()
        .flatten()
        .and_then(|bytes| xml::parse(&bytes).ok())
        .map(|root| page::menu(&root))
        .unwrap_or_default();
    GuidanceMenu::build(entries)
}

/// Erreur de lecture au libellé fixe.
fn decode(detail: String) -> GuidanceError {
    GuidanceError::Decode { detail }
}

/// Exécute `work` sur un fil bloquant. Une panique devient une erreur de lecture : le serveur
/// continue.
pub(crate) async fn off_thread<T, W>(work: W) -> Result<T, GuidanceError>
where
    T: Send + 'static,
    W: FnOnce() -> Result<T, GuidanceError> + Send + 'static,
{
    // Une instance par fermeture : aucune branche ici, que chaque instance devrait parcourir.
    tokio::task::spawn_blocking(work).await.map_err(stopped).and_then(std::convert::identity)
}

/// Erreur d'un travail à part (fil bloquant ou tâche) qui a paniqué ou a été annulé.
fn stopped(_: tokio::task::JoinError) -> GuidanceError {
    decode("background work stopped".to_owned())
}

/// Les archives des deux publics.
pub(crate) struct Store<F> {
    fetcher: Arc<F>,
    /// Horloge civile, pour l'âge d'une archive quand la machine a dormi.
    clock: Arc<dyn Clock>,
    /// Échéance d'un appel quand aucune n'est publiée (`LF_TOOL_DEADLINE_S`).
    wait_default: Duration,
    particuliers: Arc<Slot>,
    entreprendre: Arc<Slot>,
}

impl<F: Fetch> Store<F> {
    /// Magasin vide : rien n'est téléchargé avant le premier appel.
    pub(crate) fn new(fetcher: F, wait_default: Duration, clock: Arc<dyn Clock>) -> Self {
        Self {
            fetcher: Arc::new(fetcher),
            clock,
            wait_default,
            particuliers: Arc::new(Slot::new(Audience::Particuliers)),
            entreprendre: Arc::new(Slot::new(Audience::Entreprendre)),
        }
    }

    fn slot(&self, audience: Audience) -> &Arc<Slot> {
        match audience {
            Audience::Particuliers => &self.particuliers,
            Audience::Entreprendre => &self.entreprendre,
        }
    }

    /// Lance le téléchargement de `audience` s'il le faut, sans l'attendre : une recherche sur les
    /// deux publics charge leurs archives ensemble.
    pub(crate) fn prepare(&self, audience: Audience) {
        let slot = self.slot(audience);
        let mut state = slot.lock();
        if matches!(state.step(now(&*self.clock)), Step::Start) {
            self.start(slot, &mut state);
        }
    }

    /// L'archive servable de `audience`, téléchargée ou revalidée s'il le faut.
    ///
    /// # Errors
    ///
    /// [`GuidanceError::Downloading`] si le téléchargement dure au-delà de l'attente permise, ou
    /// l'erreur de la dernière tentative quand aucune archive ne se sert.
    pub(crate) async fn current(&self, audience: Audience) -> Result<Arc<Loaded>, GuidanceError> {
        let slot = self.slot(audience);
        // Lue dans la tâche de l'appel : une tâche lancée ne voit pas l'échéance publiée.
        let deadline = Deadline::current_or(self.wait_default).saturating_sub(WAIT_MARGIN);
        let (mut done, wait) = {
            let mut state = slot.lock();
            let now = now(&*self.clock);
            let done = match state.step(now) {
                Step::Serve(loaded) => return Ok(loaded),
                Step::Fail(error) => return Err(error),
                Step::Join(done) => done,
                Step::Start => self.start(slot, &mut state),
            };
            (done, state.patience(now, deadline))
        };
        // L'attente finie ou épuisée, l'état dit ce qui se sert.
        let _finished = timeout(wait, done.wait_for(|finished| *finished)).await;
        slot.lock().settle(now(&*self.clock))
    }

    /// Lance le téléchargement de `slot`, sous son verrou ; rend le signal de sa fin.
    fn start(&self, slot: &Arc<Slot>, state: &mut State) -> watch::Receiver<bool> {
        let (finish, done) = watch::channel(false);
        let etag = state.loaded.as_ref().and_then(|(loaded, _)| loaded.etag.clone());
        state.running = Some(done.clone());
        state.failed = None;
        let fetcher = Arc::clone(&self.fetcher);
        let clock = Arc::clone(&self.clock);
        let slot = Arc::clone(slot);
        tokio::spawn(async move {
            let audience = slot.audience;
            // Dans sa propre tâche : une panique y devient un échec, et le public reste libre.
            let work = tokio::spawn(async move { refresh(&*fetcher, audience, etag).await });
            let outcome = work.await.map_err(stopped).and_then(std::convert::identity);
            slot.lock().finish(audience, outcome, now(&*clock));
            finish.send_replace(true);
        });
        done
    }
}

/// L'instant présent, sur les deux horloges.
fn now(clock: &dyn Clock) -> Moment {
    Moment { mono: Instant::now(), wall: clock.now_utc() }
}

/// Un instant sur l'horloge monotone de `tokio` et sur l'horloge civile du port.
#[derive(Debug, Clone, Copy)]
struct Moment {
    mono: Instant,
    wall: DateTime<Utc>,
}

impl Moment {
    /// Temps écoulé depuis `earlier` : le plus long des deux. La monotone ne compte pas la mise en
    /// veille sous Linux et macOS ; la civile peut reculer, et ne compte alors pour rien.
    fn since(&self, earlier: &Self) -> Duration {
        let wall = self.wall.signed_duration_since(earlier.wall).to_std().unwrap_or_default();
        self.mono.saturating_duration_since(earlier.mono).max(wall)
    }
}

/// Écart avant la tentative qui suit `refusals` archives refusées de suite.
fn refusal_gap(refusals: u32) -> Duration {
    match refusals {
        0 | 1 => RETRY_AFTER_FAILURE,
        2 => Duration::from_secs(15 * 60),
        3 => Duration::from_secs(3_600),
        _ => Duration::from_secs(6 * 3_600),
    }
}

/// Vrai si l'archive a été reçue puis refusée : elle a coûté un téléchargement entier.
fn refused(error: &GuidanceError) -> bool {
    matches!(error, GuidanceError::Decode { .. } | GuidanceError::TooLarge { .. })
}

/// Téléchargement, puis chargement d'une archive nouvelle ; `None` pour un `304`.
async fn refresh<F: Fetch>(
    fetcher: &F,
    audience: Audience,
    etag: Option<String>,
) -> Result<Option<Loaded>, GuidanceError> {
    match fetcher.fetch(audience, etag.as_deref()).await? {
        Fetched::NotModified => Ok(None),
        Fetched::Fresh(download) => off_thread(move || load(audience, download)).await.map(Some),
    }
}

/// L'archive d'un public et son téléchargement.
struct Slot {
    audience: Audience,
    state: Mutex<State>,
}

impl Slot {
    fn new(audience: Audience) -> Self {
        Self { audience, state: Mutex::new(State::default()) }
    }

    /// Verrou de l'état ; jamais gardé au-delà d'un `await`.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// État d'un public.
#[derive(Default)]
struct State {
    /// L'archive chargée, et l'instant de son téléchargement ou de sa dernière revalidation.
    loaded: Option<(Arc<Loaded>, Moment)>,
    /// Signal de fin du téléchargement en cours.
    running: Option<watch::Receiver<bool>>,
    /// La dernière tentative, si elle a échoué.
    failed: Option<Failure>,
    /// Archives refusées de suite, depuis le dernier succès.
    refusals: u32,
}

/// Une tentative échouée.
struct Failure {
    /// Fin de la tentative.
    at: Moment,
    /// Écart avant la suivante.
    gap: Duration,
    error: GuidanceError,
}

impl Failure {
    /// L'erreur à rendre à `now` : pour un refus 429, l'attente qui reste avant la tentative
    /// suivante, la plus longue du repos annoncé et de l'écart.
    fn aged(&self, now: Moment) -> GuidanceError {
        match &self.error {
            GuidanceError::RateLimited { retry_after: Some(wait) } => GuidanceError::RateLimited {
                retry_after: Some((*wait).max(self.gap).saturating_sub(now.since(&self.at))),
            },
            error => error.clone(),
        }
    }
}

/// Ce qu'un appel fait de l'état.
enum Step {
    /// Servir cette archive.
    Serve(Arc<Loaded>),
    /// Rendre cette erreur.
    Fail(GuidanceError),
    /// Attendre le téléchargement en cours.
    Join(watch::Receiver<bool>),
    /// Lancer un téléchargement, puis l'attendre.
    Start,
}

impl State {
    /// Décision d'un appel à `now`.
    fn step(&self, now: Moment) -> Step {
        if let Some(loaded) = self.servable(now, FRESH_FOR) {
            return Step::Serve(loaded);
        }
        if let Some(done) = &self.running {
            return Step::Join(done.clone());
        }
        match &self.failed {
            Some(failure) if now.since(&failure.at) < failure.gap => {
                self.settled(now, failure.aged(now)).map_or_else(Step::Fail, Step::Serve)
            }
            _ => Step::Start,
        }
    }

    /// Attente permise à un appel qui attend un téléchargement : `deadline`, bornée à
    /// [`STALE_WAIT`] quand l'archive précédente se sert encore.
    fn patience(&self, now: Moment, deadline: Duration) -> Duration {
        if self.servable(now, FRESH_FOR.saturating_add(STALE_FOR)).is_some() {
            deadline.min(STALE_WAIT)
        } else {
            deadline
        }
    }

    /// Réponse après l'attente : l'archive si elle se sert encore, sinon l'erreur de la tentative,
    /// sinon « en cours de téléchargement ».
    fn settle(&self, now: Moment) -> Result<Arc<Loaded>, GuidanceError> {
        let error = self.failed.as_ref().map_or(GuidanceError::Downloading, |f| f.aged(now));
        self.settled(now, error)
    }

    /// L'archive si elle se sert encore, fraîche ou en sursis ; sinon `error`.
    fn settled(&self, now: Moment, error: GuidanceError) -> Result<Arc<Loaded>, GuidanceError> {
        self.servable(now, FRESH_FOR.saturating_add(STALE_FOR)).ok_or(error)
    }

    /// L'archive, si elle a été validée il y a moins de `age`.
    fn servable(&self, now: Moment, age: Duration) -> Option<Arc<Loaded>> {
        let (loaded, at) = self.loaded.as_ref()?;
        (now.since(at) < age).then(|| Arc::clone(loaded))
    }

    /// Fin d'un téléchargement de `audience` à `now` : nouvelle archive, archive revalidée, ou
    /// échec.
    fn finish(
        &mut self,
        audience: Audience,
        outcome: Result<Option<Loaded>, GuidanceError>,
        now: Moment,
    ) {
        self.running = None;
        match outcome {
            Ok(Some(loaded)) => {
                self.refusals = 0;
                self.loaded = Some((Arc::new(loaded), now));
            }
            Ok(None) => {
                self.refusals = 0;
                self.loaded = self.loaded.take().map(|(loaded, _)| (loaded, now));
            }
            Err(error) => {
                let gap = if refused(&error) {
                    self.refusals = self.refusals.saturating_add(1);
                    refusal_gap(self.refusals)
                } else {
                    RETRY_AFTER_FAILURE
                };
                tracing::warn!(
                    audience = audience.name(),
                    error = %error,
                    retry_in_s = gap.as_secs(),
                    "Service-Public archive not refreshed"
                );
                self.failed = Some(Failure { at: now, gap, error });
            }
        }
    }
}

#[cfg(test)]
mod tests;
