//! Tests du magasin : le faux téléchargeur de `testing`, sur le temps suspendu de `tokio` ; puis un
//! essai de bout en bout avec le vrai client contre un faux serveur local.

use pretty_assertions::assert_eq;
use tokio::time::advance;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

use super::*;
use crate::{
    archive::tests::zip_of,
    testing::{
        Fake, MIDDAY, PART, PRO, Reply, TestWall, call, fresh, journal, server_error, start_journal,
    },
};

const F1437: &[u8] = include_bytes!("../../tests/fixtures/xml/part/F1437.xml");
const MENU: &[u8] = include_bytes!("../../tests/fixtures/xml/part/menu.xml");
const WAIT: Duration = Duration::from_secs(300);
const P: Audience = Audience::Particuliers;
const E: Audience = Audience::Entreprendre;

fn store(fake: &Arc<Fake>) -> Store<Arc<Fake>> {
    walled(fake).0
}

/// Le magasin et son horloge civile, à régler par le test.
fn walled(fake: &Arc<Fake>) -> (Store<Arc<Fake>>, TestWall) {
    let wall = TestWall::at(MIDDAY);
    (Store::new(Arc::clone(fake), WAIT, Arc::new(wall.clone())), wall)
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
    start_journal();
    let loaded = load(P, download(PART.to_vec())).unwrap();
    // Aucune page écartée : le chargement est journalisé, sans avertissement.
    let logged = journal();
    assert!(logged.contains("pages=10 dropped=0"));
    assert!(!logged.contains("WARN"));
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
    start_journal();
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
    // Quatre pages candidates (`F1`, `F3`, `F4`, `N5`), deux écartées : chargement journalisé,
    // avec un avertissement.
    let logged = journal();
    assert!(logged.contains(" INFO ") && logged.contains("Service-Public archive loaded"));
    assert!(logged.contains("pages=2 dropped=2"));
    assert!(logged.contains(" WARN ") && logged.contains("many pages could not be read"));
    assert!(logged.contains("dropped=2 candidates=4"));
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

/// L'archive vient du réseau : un index au-delà de son budget la fait refuser.
#[test]
fn an_archive_over_the_index_budget_is_refused() {
    // 360 pages de 280 mots distincts de cinq lettres : 100 800 mots, au-delà de 100 000.
    const LETTERS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
    let mut next = 0_u32;
    let mut words = |count: usize| -> String {
        let mut line = Vec::with_capacity(count);
        for _ in 0..count {
            let mut number = next;
            next = next.saturating_add(1);
            let mut word = String::new();
            for _ in 0..5 {
                word.push(char::from(LETTERS[usize::try_from(number % 26).unwrap()]));
                number /= 26;
            }
            line.push(word);
        }
        line.join(" ")
    };
    let entries: Vec<(String, Vec<u8>)> = (1..=360)
        .map(|n| {
            let (title, summary) = (words(140), words(140));
            let xml = format!(
                r#"<Publication ID="F{n}"><dc:title>{title}</dc:title><dc:description>{summary}</dc:description></Publication>"#
            );
            (format!("F{n}.xml"), xml.into_bytes())
        })
        .collect();
    let refs: Vec<(&str, &[u8])> =
        entries.iter().map(|(name, xml)| (name.as_str(), xml.as_slice())).collect();
    let refused = load(P, download(zip_of(&refs))).unwrap_err();
    let detail =
        "archive refused: search index over budget (100000 words, 500000 postings at most)";
    assert_eq!(refused, GuidanceError::Decode { detail: detail.to_owned() });
    // Les 357 premières pages tiennent sous le budget.
    let fits = load(P, download(zip_of(&refs[..357]))).unwrap();
    assert_eq!(fits.index.len(), 357);
}

#[test]
fn a_loaded_archive_is_printed_without_its_bytes() {
    let loaded = load(E, Download { etag: Some("\"e\"".to_owned()), ..download(PRO.to_vec()) });
    assert_eq!(
        format!("{:?}", loaded.unwrap()),
        r#"Loaded { bytes: 7590, entries: 3, ignored: 0, pages: 2, menu_places: 100, etag: Some("\"e\""), last_modified: None }"#
    );
}

#[tokio::test]
#[expect(clippy::panic, reason = "une panique du travail est le cas testé")]
async fn blocking_work_runs_off_the_caller_thread_and_a_panic_is_an_error() {
    let caller = std::thread::current().id();
    let worker = off_thread(|| Ok(std::thread::current().id())).await.unwrap();
    assert_ne!(worker, caller);
    let stopped = off_thread::<(), _>(|| std::panic::panic_any("boom")).await;
    assert_eq!(
        stopped,
        Err(GuidanceError::Decode { detail: "background work stopped".to_owned() })
    );
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
    // Trente secondes plus tard, sans requête : le repos qui reste.
    advance(Duration::from_secs(30)).await;
    let left = GuidanceError::RateLimited { retry_after: Some(Duration::from_secs(870)) };
    assert_eq!(store.current(P).await.unwrap_err(), left);
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

#[test]
fn refusals_in_a_row_space_out_the_attempts() {
    let gaps = [0, 1, 2, 3, 4, 9].map(|refusals| refusal_gap(refusals).as_secs());
    assert_eq!(gaps, [60, 60, 900, 3_600, 21_600, 21_600]);
    let decode = GuidanceError::Decode { detail: "archive refused".to_owned() };
    let large = GuidanceError::TooLarge { detail: "archive over 64 bytes".to_owned() };
    assert!(refused(&decode) && refused(&large));
    let limited = GuidanceError::RateLimited { retry_after: None };
    for other in [server_error(), GuidanceError::Timeout, limited, GuidanceError::Downloading] {
        assert!(!refused(&other));
    }
}

/// Une archive refusée a coûté un téléchargement entier : les refus de suite espacent les
/// tentatives ; une autre panne ne compte pas ; un succès remet le compte à zéro.
#[tokio::test(start_paused = true)]
async fn refused_archives_space_out_the_attempts_until_a_success() {
    start_journal();
    let broken = || Ok(fresh(b"<html>maintenance</html>", "bad"));
    let fake = Fake::open([
        broken(),
        broken(),
        Err(server_error()),
        broken(),
        Ok(fresh(PART, "v1")),
        broken(),
        broken(),
    ]);
    let store = store(&fake);
    let second = Duration::from_secs(1);
    // Premier refus : une minute.
    assert!(store.current(P).await.is_err());
    assert!(journal().contains("Service-Public archive not refreshed"));
    advance(RETRY_AFTER_FAILURE).await;
    // Deuxième refus : un quart d'heure.
    assert!(store.current(P).await.is_err());
    assert!(journal().contains("retry_in_s=900"));
    advance(Duration::from_secs(900).saturating_sub(second)).await;
    assert!(store.current(P).await.is_err());
    assert_eq!(fake.calls().len(), 2);
    // Une panne du serveur n'allonge pas l'écart, et ne remet pas le compte à zéro.
    advance(second).await;
    assert_eq!(store.current(P).await.unwrap_err(), server_error());
    assert!(journal().contains("retry_in_s=60"));
    advance(RETRY_AFTER_FAILURE).await;
    // Troisième refus : une heure.
    assert!(store.current(P).await.is_err());
    advance(Duration::from_secs(3_600).saturating_sub(second)).await;
    assert!(store.current(P).await.is_err());
    assert_eq!(fake.calls().len(), 4);
    advance(second).await;
    let first = store.current(P).await.unwrap();
    // Après un succès, un refus coûte de nouveau une minute.
    advance(FRESH_FOR).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    advance(RETRY_AFTER_FAILURE.saturating_sub(second)).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls().len(), 6);
    advance(second).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls().len(), 7);
}

/// Un téléchargement qui panique est un échec comme un autre : le public n'en reste pas bloqué.
#[tokio::test(start_paused = true)]
async fn a_download_that_panics_frees_its_public() {
    // Sans réponse écrite d'avance, le faux téléchargeur panique.
    let fake = Fake::open(Vec::<Reply>::new());
    let store = store(&fake);
    let stopped = GuidanceError::Decode { detail: "background work stopped".to_owned() };
    assert_eq!(store.current(P).await.unwrap_err(), stopped);
    advance(RETRY_AFTER_FAILURE).await;
    assert_eq!(store.current(P).await.unwrap_err(), stopped);
    assert_eq!(fake.calls(), [call(P, None), call(P, None)]);
}

/// La machine a dormi : l'horloge monotone n'a pas bougé, l'horloge civile si. L'archive est
/// revalidée au réveil. Une horloge civile qui recule ne change rien.
#[tokio::test(start_paused = true)]
async fn an_archive_is_revalidated_after_the_machine_slept() {
    let fake =
        Fake::open([Ok(fresh(PART, "v1")), Ok(Fetched::NotModified), Ok(Fetched::NotModified)]);
    let (store, wall) = walled(&fake);
    let first = store.current(P).await.unwrap();
    wall.shift(23 * 3_600);
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls(), [call(P, None)]);
    wall.shift(3_600);
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls(), [call(P, None), call(P, Some("v1"))]);
    // L'horloge civile recule de dix jours : l'horloge monotone compte seule.
    wall.shift(-10 * 24 * 3_600);
    advance(FRESH_FOR.saturating_sub(Duration::from_secs(1))).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls().len(), 2);
    advance(Duration::from_secs(1)).await;
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    assert_eq!(fake.calls().len(), 3);
}

/// Une archive en sursis se sert après une attente courte quand sa revalidation tarde ; la
/// revalidation finie, l'archive du jour se sert sans attente.
#[tokio::test(start_paused = true)]
async fn a_stale_archive_waits_briefly_for_its_revalidation() {
    let fake = Fake::held([Ok(fresh(PART, "v1")), Ok(fresh(PART, "v2"))]);
    fake.release();
    let store = store(&fake);
    let first = store.current(P).await.unwrap();
    advance(FRESH_FOR).await;
    let start = Instant::now();
    assert!(Arc::ptr_eq(&first, &store.current(P).await.unwrap()));
    let waited = start.elapsed();
    assert!(waited >= STALE_WAIT && waited < STALE_WAIT.saturating_add(Duration::from_millis(10)));
    // Une échéance plus courte reste la borne.
    let start = Instant::now();
    let reply = Deadline::after(Duration::from_secs(3)).scope(store.current(P)).await;
    assert!(Arc::ptr_eq(&first, &reply.unwrap()));
    assert!(start.elapsed() < Duration::from_millis(2_010));
    fake.release();
    let second = store.current(P).await.unwrap();
    assert_eq!(second.etag.as_deref(), Some("v2"));
    assert_eq!(fake.calls(), [call(P, None), call(P, Some("v1"))]);
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
    let client = ArchiveClient::local(&server.uri(), WAIT, 1 << 20);
    let store = Store::new(client, WAIT, Arc::new(TestWall::at(MIDDAY)));
    let loaded = store.current(P).await.unwrap();
    assert_eq!((loaded.index.len(), loaded.etag.as_deref()), (10, Some("\"6b059e81-1734757\"")));
    assert!(Arc::ptr_eq(&loaded, &store.current(P).await.unwrap()));
}
