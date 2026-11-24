//! Archive et index de chaque public, en mémoire (ADR-0033 décision 3 ; T-MS-23).
//!
//! - **Un téléchargement à la fois par public**, dans sa propre tâche (`tokio::spawn`), partagé par
//!   les appels qui l'attendent. Le délai du téléchargement est celui du client (300 s).
//! - **Attente d'un appel** : au plus l'échéance publiée ([`Deadline::current_or`]) moins
//!   [`WAIT_MARGIN`] ; ensuite l'appel rend [`GuidanceError::Downloading`], et le téléchargement
//!   continue. Sans la marge, la garde dure d'`Audited` répondrait avant l'adapter. Quand
//!   l'archive précédente se sert encore (revalidation d'une archive en sursis), l'attente finit
//!   [`STALE_WAIT`] après le début du téléchargement, pour tous les appels qui l'attendent : sur
//!   une bonne liaison, l'archive du jour arrive avant ; sinon les appels reçoivent la précédente
//!   sans attendre la fin d'un serveur lent.
//! - **Fraîcheur** : une archive se sert sans requête pendant [`FRESH_FOR`] après son téléchargement
//!   ou sa dernière revalidation ; ensuite, l'appel suivant la revalide (`If-None-Match`, un `304`
//!   la garde). L'âge est le plus long des temps mesurés par l'horloge monotone et par l'horloge
//!   civile du port [`Clock`] : la monotone ne compte pas la mise en veille de la machine sous
//!   Linux et macOS, la civile peut reculer. Une horloge civile qui saute en avant vieillit
//!   l'archive d'autant : au-delà de 24 h, une revalidation ; au-delà de 72 h, l'archive ne se sert
//!   plus avant le succès suivant.
//! - **Échec** (archive refusée, revalidation échouée, refus 429) : l'archive précédente se sert
//!   encore, [`STALE_FOR`] de plus au plus ; aucune nouvelle tentative avant [`RETRY_AFTER_FAILURE`]
//!   (au-delà, pendant le repos du client après un 429, la tentative est refusée sans réseau). Les
//!   échecs qui ont coûté un téléchargement ([`Cost`]) espacent les tentatives quand ils se
//!   suivent ([`backoff`] : une minute, un quart d'heure, une heure, puis six heures ; une heure au
//!   plus pour un corps coupé ou un délai dépassé), jusqu'au premier succès ou `304`. Sans archive
//!   servable, l'appel rend l'erreur de la dernière tentative ; un refus 429 y donne le repos qui
//!   reste.
//! - **Tâche du téléchargement** : le téléchargement et la lecture courent dans une tâche à part,
//!   que la tâche du public attend ; si elle panique, l'échec est enregistré comme un autre et le
//!   public n'en reste pas bloqué.
//! - **Index construit au chargement** sur un fil bloquant (`spawn_blocking`) : en-tête de chaque
//!   fiche (`F…`) et page de navigation (`N…`), lu à 64 Kio au plus ; ressources (`R…`) et
//!   fichiers nommés exclus ; chemin de chaque page tiré du menu du public (Q41). L'index n'est
//!   servi qu'entier : il remplace l'ancien d'un coup. Une page dont l'en-tête ne se lit pas est
//!   écartée et comptée ; une archive sans aucune page lisible, ou dont l'index dépasse son budget
//!   ([`GuidanceIndex::build`]), est refusée : les pages sont lues une à une, et la lecture
//!   s'arrête au dépassement. Chaque chargement est journalisé (`info`, nombre de pages, écartées,
//!   ignorées), avec un avertissement si plus d'une page sur [`DROPPED_WARN_ONE_IN`] est écartée ;
//!   chaque échec aussi (`warn`, sans adresse), hors du verrou du public.

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
    client::{ArchiveClient, BODY_CUT, Download, Fetched},
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

/// Attente des appels quand l'archive précédente se sert encore, comptée depuis le début du
/// téléchargement.
pub(crate) const STALE_WAIT: Duration = Duration::from_secs(10);

/// Écart le plus long après un corps coupé ou un délai dépassé : une panne passagère du réseau ne
/// doit pas priver le serveur de l'archive pendant des heures.
pub(crate) const CUT_GAP_MAX: Duration = Duration::from_secs(3_600);

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
    let mut candidates = 0_usize;
    // Lues une à une par l'index : aucune page n'est lue au-delà de son budget.
    let pages = archive
        .names()
        .filter_map(|name| page_id(name).map(|id| (name, id)))
        .inspect(|_| candidates = candidates.saturating_add(1))
        .filter_map(|(name, id)| {
            archive
                .head(name)
                .ok()
                .flatten()
                .and_then(|head| xml::parse_head(&head).ok())
                .and_then(|root| page::header(&root, audience, &id, &menu).ok())
        });
    let index =
        GuidanceIndex::build(pages).map_err(|error| decode(format!("archive refused: {error}")))?;
    if index.is_empty() {
        return Err(decode("archive has no readable page".to_owned()));
    }
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
pub(crate) struct Store<F, K> {
    fetcher: Arc<F>,
    /// Horloge civile, pour l'âge d'une archive quand la machine a dormi.
    clock: Arc<K>,
    /// Échéance d'un appel quand aucune n'est publiée (`LF_TOOL_DEADLINE_S`).
    wait_default: Duration,
    particuliers: Arc<Slot>,
    entreprendre: Arc<Slot>,
}

impl<F: Fetch, K: Clock> Store<F, K> {
    /// Magasin vide : rien n'est téléchargé avant le premier appel.
    pub(crate) fn new(fetcher: F, wait_default: Duration, clock: K) -> Self {
        Self {
            fetcher: Arc::new(fetcher),
            clock: Arc::new(clock),
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
        let now = now(&*self.clock);
        if matches!(state.step(now), Step::Start) {
            self.start(slot, &mut state, now);
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
        let (mut running, wait) = {
            let mut state = slot.lock();
            let now = now(&*self.clock);
            let running = match state.step(now) {
                Step::Serve(loaded) => return Ok(loaded),
                Step::Fail(error) => return Err(error),
                Step::Join(running) => running,
                Step::Start => self.start(slot, &mut state, now),
            };
            let wait = state.patience(now, deadline, &running);
            (running, wait)
        };
        // L'attente finie ou épuisée, l'état dit ce qui se sert.
        let _finished = timeout(wait, running.done.wait_for(|finished| *finished)).await;
        slot.lock().settle(now(&*self.clock))
    }

    /// Lance à `at` le téléchargement de `slot`, sous son verrou ; rend le téléchargement en cours.
    fn start(&self, slot: &Arc<Slot>, state: &mut State, at: Moment) -> Running {
        let (finish, done) = watch::channel(false);
        let etag = state.loaded.as_ref().and_then(|(loaded, _)| loaded.etag.clone());
        let running = Running { done, since: at };
        state.running = Some(running.clone());
        state.failed = None;
        let fetcher = Arc::clone(&self.fetcher);
        let clock = Arc::clone(&self.clock);
        let slot = Arc::clone(slot);
        tokio::spawn(async move {
            let audience = slot.audience;
            // Dans sa propre tâche : une panique y devient un échec, et le public reste libre.
            let work = tokio::spawn(async move { refresh(&*fetcher, audience, etag).await });
            let outcome = work.await.map_err(stopped).and_then(std::convert::identity);
            let failed = slot.lock().finish(outcome, now(&*clock));
            // Journal hors du verrou : une sortie d'erreur lente ne bloque pas les appels.
            if let Some((error, gap)) = failed {
                tracing::warn!(
                    audience = audience.name(),
                    error = %error,
                    retry_in_s = gap.as_secs(),
                    "Service-Public archive not refreshed"
                );
            }
            finish.send_replace(true);
        });
        running
    }
}

/// L'instant présent, sur les deux horloges.
fn now(clock: &impl Clock) -> Moment {
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

/// Écart avant la tentative qui suit `costly` échecs coûteux de suite.
fn backoff(costly: u32) -> Duration {
    match costly {
        0 | 1 => RETRY_AFTER_FAILURE,
        2 => Duration::from_secs(15 * 60),
        3 => Duration::from_secs(3_600),
        _ => Duration::from_secs(6 * 3_600),
    }
}

/// Ce qu'un échec a coûté.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cost {
    /// Pas de corps lu : connexion refusée, statut d'erreur, refus 429.
    Small,
    /// Un corps lu en partie : coupé, ou arrêté par le délai (un délai dépassé à la connexion
    /// compte ici aussi : le client ne les distingue pas).
    Cut,
    /// Une archive reçue, puis refusée.
    Refused,
}

/// Ce que `error` a coûté.
fn cost(error: &GuidanceError) -> Cost {
    match error {
        GuidanceError::Decode { .. } | GuidanceError::TooLarge { .. } => Cost::Refused,
        GuidanceError::Timeout => Cost::Cut,
        GuidanceError::Upstream { detail, .. } if detail == BODY_CUT => Cost::Cut,
        _ => Cost::Small,
    }
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

/// Un téléchargement en cours : le signal de sa fin, et l'instant de son début.
#[derive(Clone)]
struct Running {
    done: watch::Receiver<bool>,
    since: Moment,
}

/// État d'un public.
#[derive(Default)]
struct State {
    /// L'archive chargée, et l'instant de son téléchargement ou de sa dernière revalidation.
    loaded: Option<(Arc<Loaded>, Moment)>,
    /// Le téléchargement en cours.
    running: Option<Running>,
    /// La dernière tentative, si elle a échoué.
    failed: Option<Failure>,
    /// Échecs coûteux de suite ([`Cost`]), depuis le dernier succès ou `304`.
    costly: u32,
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
    Join(Running),
    /// Lancer un téléchargement, puis l'attendre.
    Start,
}

impl State {
    /// Décision d'un appel à `now`.
    fn step(&self, now: Moment) -> Step {
        if let Some(loaded) = self.servable(now, FRESH_FOR) {
            return Step::Serve(loaded);
        }
        if let Some(running) = &self.running {
            return Step::Join(running.clone());
        }
        match &self.failed {
            Some(failure) if now.since(&failure.at) < failure.gap => {
                self.settled(now, failure.aged(now)).map_or_else(Step::Fail, Step::Serve)
            }
            _ => Step::Start,
        }
    }

    /// Attente permise à un appel qui attend `running` : `deadline`, bornée par la fin des
    /// [`STALE_WAIT`] qui suivent son début quand l'archive précédente se sert encore.
    fn patience(&self, now: Moment, deadline: Duration, running: &Running) -> Duration {
        if self.servable(now, FRESH_FOR.saturating_add(STALE_FOR)).is_some() {
            deadline.min(STALE_WAIT.saturating_sub(now.since(&running.since)))
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

    /// Fin d'un téléchargement à `now` : nouvelle archive, archive revalidée, ou échec ; rend
    /// l'erreur et l'écart d'un échec, pour le journal.
    fn finish(
        &mut self,
        outcome: Result<Option<Loaded>, GuidanceError>,
        now: Moment,
    ) -> Option<(GuidanceError, Duration)> {
        self.running = None;
        match outcome {
            Ok(Some(loaded)) => {
                self.costly = 0;
                self.loaded = Some((Arc::new(loaded), now));
                None
            }
            Ok(None) => {
                self.costly = 0;
                self.loaded = self.loaded.take().map(|(loaded, _)| (loaded, now));
                None
            }
            Err(error) => {
                let gap = match cost(&error) {
                    Cost::Small => RETRY_AFTER_FAILURE,
                    Cost::Cut => {
                        self.costly = self.costly.saturating_add(1);
                        backoff(self.costly).min(CUT_GAP_MAX)
                    }
                    Cost::Refused => {
                        self.costly = self.costly.saturating_add(1);
                        backoff(self.costly)
                    }
                };
                self.failed = Some(Failure { at: now, gap, error: error.clone() });
                Some((error, gap))
            }
        }
    }
}

#[cfg(test)]
mod tests;
