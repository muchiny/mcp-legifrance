//! Tests du magasin : un faux téléchargeur aux réponses écrites d'avance, retenu par une barrière
//! quand le test veut un téléchargement en cours, sur le temps suspendu de `tokio` ; puis un essai
//! de bout en bout avec le vrai client contre un faux serveur local.

use std::collections::VecDeque;

use pretty_assertions::assert_eq;
use tokio::{sync::Semaphore, time::advance};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

use super::*;
use crate::archive::tests::zip_of;

const PART: &[u8] = include_bytes!("../../tests/fixtures/archives/part-vosdroits-mini.zip");
const PRO: &[u8] = include_bytes!("../../tests/fixtures/archives/pro-vosdroits-mini.zip");
const F1437: &[u8] = include_bytes!("../../tests/fixtures/xml/part/F1437.xml");
const MENU: &[u8] = include_bytes!("../../tests/fixtures/xml/part/menu.xml");
const WAIT: Duration = Duration::from_secs(300);
const P: Audience = Audience::Particuliers;
const E: Audience = Audience::Entreprendre;

type Reply = Result<Fetched, GuidanceError>;

/// Faux téléchargeur : réponses dans l'ordre, appels notés, barrière avant chaque réponse.
struct Fake {
    replies: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<(Audience, Option<String>)>>,
    gate: Semaphore,
}

impl Fake {
    /// Répond sans attendre.
    fn open(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Self::with(replies, Semaphore::MAX_PERMITS)
    }

    /// Retient chaque réponse jusqu'à [`Fake::release`].
    fn held(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
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
    fn release(&self) {
        self.gate.add_permits(1);
    }

    fn calls(&self) -> Vec<(Audience, Option<String>)> {
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

fn fresh(body: &[u8], etag: &str) -> Fetched {
    Fetched::Fresh(Download {
        body: body.to_vec(),
        etag: Some(etag.to_owned()),
        last_modified: None,
    })
}

fn store(fake: &Arc<Fake>) -> Store<Arc<Fake>> {
    Store::new(Arc::clone(fake), WAIT)
}

fn server_error() -> GuidanceError {
    GuidanceError::Upstream {
        status: Some(500),
        retryable: true,
        detail: "server error".to_owned(),
    }
}

fn call(audience: Audience, etag: Option<&str>) -> (Audience, Option<String>) {
    (audience, etag.map(str::to_owned))
}

fn download(body: Vec<u8>) -> Download {
    Download { body, etag: None, last_modified: None }
}

/// Une petite page : `<Publication>` au numéro `id`.
fn publication(id: &str) -> Vec<u8> {
    format!(r#"<Publication ID="{id}"><dc:title>Page {id}</dc:title></Publication>"#).into_bytes()
}

fn ids(loaded: &Loaded) -> Vec<&str> {
    loaded.index.pages().iter().map(|page| page.id.as_str()).collect()
}

// ---- chargement --------------------------------------------------------------------------------

#[test]
fn the_index_holds_every_page_of_the_archive_with_its_path() {
    let loaded = load(P, download(PART.to_vec())).unwrap();
    // Dans l'ordre des noms (répertoire de l'archive trié) ; le classement n'en dépend pas.
    assert_eq!(
        ids(&loaded),
        [
            "F10041", "F1239", "F1437", "F1903", "F2408", "F31613", "F34680", "F468", "N19804",
            "N31675"
        ]
    );
    assert_eq!(loaded.archive.len(), 11);
    let sbi = loaded.index.pages().iter().find(|page| page.id.as_str() == "F1437").unwrap();
    assert_eq!(sbi.path, ["Justice", "Réparation du préjudice", "Saisies et recouvrements"]);
    assert!(!loaded.menu.is_empty());
    let pro = load(E, download(PRO.to_vec())).unwrap();
    assert_eq!(ids(&pro), ["F2408", "F37954"]);
    assert_eq!(
        pro.index.pages()[0].source_url.as_str(),
        "https://entreprendre.service-public.gouv.fr/vosdroits/F2408"
    );
}

#[test]
fn resources_named_files_and_unreadable_pages_are_left_out() {
    let named = publication("F1");
    let resource = publication("R1");
    let lower = publication("F2");
    let elsewhere = publication("F5");
    let navigation = publication("N5");
    let bytes = zip_of(&[
        ("F1.xml", &named),
        ("R1.xml", &resource),
        ("accueil.xml", &named),
        ("f2.xml", &lower),
        ("F3.xml", b"<pas du XML"),
        ("F4.xml", &elsewhere),
        ("N5.xml", &navigation),
    ]);
    let loaded = load(P, download(bytes)).unwrap();
    assert_eq!(ids(&loaded), ["F1", "N5"]);
    // Sans menu, les chemins sont omis.
    assert!(loaded.menu.is_empty());
    assert_eq!(loaded.index.pages()[0].path, Vec::<String>::new());
    // Une entrée dont les données ne se décompressent pas est écartée : octet faussé dans les
    // données de `F6.xml`, après son en-tête local (30 octets) et son nom (6 octets).
    let mut bytes = zip_of(&[("F6.xml", &publication("F6")), ("N5.xml", &navigation)]);
    bytes[40] ^= 0xFF;
    assert_eq!(ids(&load(P, download(bytes)).unwrap()), ["N5"]);
    // Un menu illisible vaut un menu absent.
    let bytes = zip_of(&[("menu.xml", b"<Menu><ItemMenu"), ("F1437.xml", F1437)]);
    let loaded = load(P, download(bytes)).unwrap();
    assert!(loaded.menu.is_empty());
}

#[test]
fn an_archive_without_a_readable_page_is_refused() {
    let refused = load(P, download(zip_of(&[("menu.xml", MENU)]))).unwrap_err();
    assert_eq!(
        refused,
        GuidanceError::Decode { detail: "archive has no readable page".to_owned() }
    );
    let refused = load(P, download(b"<html>maintenance</html>".to_vec())).unwrap_err();
    let detail = "archive refused: no end of central directory".to_owned();
    assert_eq!(refused, GuidanceError::Decode { detail });
}

#[test]
fn a_loaded_archive_is_printed_without_its_bytes() {
    let loaded = load(E, Download { etag: Some("\"e\"".to_owned()), ..download(PRO.to_vec()) });
    assert_eq!(
        format!("{:?}", loaded.unwrap()),
        r#"Loaded { entries: 3, pages: 2, menu_places: 100, etag: Some("\"e\""), last_modified: None }"#
    );
}

#[tokio::test]
#[expect(clippy::panic, reason = "une panique du travail est le cas testé")]
async fn blocking_work_runs_off_the_caller_thread_and_a_panic_is_an_error() {
    let caller = std::thread::current().id();
    let worker = off_thread(|| Ok(std::thread::current().id())).await.unwrap();
    assert_ne!(worker, caller);
    let stopped = off_thread::<(), _>(|| std::panic::panic_any("boom")).await;
    assert_eq!(stopped, Err(GuidanceError::Decode { detail: "blocking work stopped".to_owned() }));
}

// ---- téléchargement et fraîcheur ---------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn the_archive_is_fetched_once_per_day() {
    let fake = Fake::open([Ok(fresh(PART, "v1")), Ok(Fetched::NotModified), Ok(fresh(PART, "v2"))]);
    let store = store(&fake);
    let first = store.current(P).await.unwrap();
    assert_eq!(first.index.len(), 10);
    advance(FRESH_FOR.saturating_sub(Duration::from_secs(1))).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls(), [call(P, None)]);
    // Au bout de 24 h, une requête conditionnelle ; le `304` garde l'archive et relance les 24 h.
    advance(Duration::from_secs(1)).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    advance(FRESH_FOR.saturating_sub(Duration::from_secs(1))).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls(), [call(P, None), call(P, Some("v1"))]);
    // Une archive nouvelle remplace l'ancienne.
    advance(Duration::from_secs(1)).await;
    let second = store.current(P).await.unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(second.etag.as_deref(), Some("v2"));
    assert_eq!(fake.calls(), [call(P, None), call(P, Some("v1")), call(P, Some("v1"))]);
}

#[tokio::test(start_paused = true)]
async fn concurrent_calls_share_one_download() {
    let fake = Fake::held([Ok(fresh(PART, "v1")), Ok(fresh(PRO, "p1"))]);
    let store = Arc::new(store(&fake));
    let waiting: Vec<_> = (0..3)
        .map(|_| {
            let store = Arc::clone(&store);
            tokio::spawn(async move { store.current(P).await })
        })
        .collect();
    while fake.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    fake.release();
    let mut served = Vec::new();
    for task in waiting {
        served.push(task.await.unwrap().unwrap());
    }
    assert!(served.iter().all(|loaded| Arc::ptr_eq(loaded, &served[0])));
    assert_eq!(fake.calls(), [call(P, None)]);
    // Chaque public a son archive et son téléchargement.
    fake.release();
    assert_eq!(ids(&store.current(E).await.unwrap()), ["F2408", "F37954"]);
    assert_eq!(fake.calls(), [call(P, None), call(E, None)]);
}

#[tokio::test(start_paused = true)]
async fn a_call_stops_waiting_one_second_before_its_deadline() {
    let fake = Fake::held([Ok(fresh(PART, "v1"))]);
    let store = store(&fake);
    let start = Instant::now();
    let reply = Deadline::after(Duration::from_secs(10)).scope(store.current(P)).await;
    assert_eq!(reply.unwrap_err(), GuidanceError::Downloading);
    let waited = start.elapsed();
    assert!(
        waited >= Duration::from_secs(9) && waited < Duration::from_millis(9_010),
        "{waited:?}"
    );
    // Sans échéance publiée : l'échéance par défaut, moins une seconde.
    let start = Instant::now();
    assert_eq!(store.current(P).await.unwrap_err(), GuidanceError::Downloading);
    let waited = start.elapsed();
    assert!(waited >= Duration::from_secs(299) && waited < Duration::from_millis(299_010));
    assert_eq!(fake.calls(), [call(P, None)]);
}

#[tokio::test(start_paused = true)]
async fn a_call_at_its_deadline_leaves_the_download_running() {
    let fake = Fake::held([Ok(fresh(PART, "v1"))]);
    let store = store(&fake);
    let reply = Deadline::after(Duration::from_millis(500)).scope(store.current(P)).await;
    assert_eq!(reply.unwrap_err(), GuidanceError::Downloading);
    // Le téléchargement est lancé : sa tâche part dès que l'appel rend la main.
    while fake.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    fake.release();
    // L'appel suivant trouve le même téléchargement, fini : aucune requête de plus.
    assert_eq!(store.current(P).await.unwrap().index.len(), 10);
    assert_eq!(fake.calls(), [call(P, None)]);
}

// ---- échecs ------------------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn a_refused_archive_keeps_the_previous_one() {
    let fake = Fake::open([
        Ok(fresh(PART, "v1")),
        Ok(fresh(b"<html>maintenance</html>", "v2")),
        Err(server_error()),
        Ok(Fetched::NotModified),
    ]);
    let store = store(&fake);
    let first = store.current(P).await.unwrap();
    advance(FRESH_FOR).await;
    // Archive refusée par le lecteur : l'ancienne se sert.
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    // Pas de nouvelle tentative avant une minute.
    advance(RETRY_AFTER_FAILURE.saturating_sub(Duration::from_secs(1))).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls().len(), 2);
    // Revalidation échouée : l'ancienne se sert encore.
    advance(Duration::from_secs(1)).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    // Un `304` la revalide pour 24 h.
    advance(RETRY_AFTER_FAILURE).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    advance(FRESH_FOR.saturating_sub(Duration::from_secs(1))).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(
        fake.calls(),
        [call(P, None), call(P, Some("v1")), call(P, Some("v1")), call(P, Some("v1"))]
    );
}

#[tokio::test(start_paused = true)]
async fn a_stale_archive_expires_after_48_hours() {
    let fake = Fake::open(
        std::iter::once(Ok(fresh(PART, "v1")))
            .chain(std::iter::repeat_with(|| Err(server_error())).take(10)),
    );
    let store = store(&fake);
    let first = store.current(P).await.unwrap();
    // Dernière seconde du sursis : la tentative échoue, l'archive se sert encore.
    advance(FRESH_FOR.saturating_add(STALE_FOR).saturating_sub(Duration::from_secs(1))).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    // Sursis épuisé : l'erreur de la dernière tentative, sans nouvelle requête (moins d'une minute).
    advance(Duration::from_secs(1)).await;
    assert_eq!(store.current(P).await.unwrap_err(), server_error());
    assert_eq!(fake.calls().len(), 2);
    // Une minute plus tard, une nouvelle tentative ; elle échoue, et rien ne se sert.
    advance(RETRY_AFTER_FAILURE).await;
    assert_eq!(store.current(P).await.unwrap_err(), server_error());
    assert_eq!(fake.calls().len(), 3);
}

#[tokio::test(start_paused = true)]
async fn without_an_archive_the_failure_is_returned_until_the_next_attempt() {
    let limited = GuidanceError::RateLimited { retry_after: Some(Duration::from_secs(900)) };
    let fake = Fake::open([Err(limited.clone()), Ok(fresh(b"PK", "v0")), Ok(fresh(PART, "v1"))]);
    let store = store(&fake);
    assert_eq!(store.current(P).await.unwrap_err(), limited);
    advance(Duration::from_secs(30)).await;
    assert_eq!(store.current(P).await.unwrap_err(), limited);
    assert_eq!(fake.calls().len(), 1);
    advance(Duration::from_secs(30)).await;
    let refused = store.current(P).await.unwrap_err();
    assert_eq!(
        refused,
        GuidanceError::Decode { detail: "archive refused: no end of central directory".to_owned() }
    );
    advance(RETRY_AFTER_FAILURE).await;
    assert_eq!(store.current(P).await.unwrap().index.len(), 10);
    assert_eq!(fake.calls(), [call(P, None), call(P, None), call(P, None)]);
}

// ---- bout en bout ------------------------------------------------------------------------------

/// Le vrai client derrière le magasin : un téléchargement, puis des lectures sans requête.
#[tokio::test]
async fn the_client_feeds_the_store() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/vdd/3.5/part/zip/vosdroits-latest.zip"))
        .and(header("user-agent", "mcp-legifrance-tests/0"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"6b059e81-1734757\"")
                .set_body_bytes(PART.to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = ArchiveClient::local(&server.uri(), WAIT, 1 << 20).unwrap();
    let store = Store::new(client, WAIT);
    let loaded = store.current(P).await.unwrap();
    assert_eq!((loaded.index.len(), loaded.etag.as_deref()), (10, Some("\"6b059e81-1734757\"")));
    assert!(Arc::ptr_eq(&loaded, &store.current(P).await.unwrap()));
}
