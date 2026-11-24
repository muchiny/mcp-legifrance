//! Tests de la source : le faux téléchargeur de `testing` pour chaque cas du 05 §5.17, puis la
//! source de production sur un faux serveur local, de bout en bout.

use std::num::NonZeroU32;

use chrono::{TimeZone as _, Utc};
use lf_domain::{
    guidance::{AudienceFilter, GuidanceContent, GuidanceKind, GuidanceTerms},
    search::PageSize,
};
use pretty_assertions::assert_eq;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::*;
use crate::{
    archive::tests::zip_of,
    client::{Download, Fetched},
    testing::{Fake, MIDDAY, PART, PRO, TestWall, call, fresh, server_error},
};

const WAIT: Duration = Duration::from_secs(300);
const P: Audience = Audience::Particuliers;
const E: Audience = Audience::Entreprendre;
const PART_PATH: &str = "/vdd/3.5/part/zip/vosdroits-latest.zip";
const PRO_PATH: &str = "/vdd/3.5/pro/zip/vosdroits-latest.zip";

fn source(fake: &Arc<Fake>) -> Source<Arc<Fake>> {
    Source::new(Arc::clone(fake), WAIT, Arc::new(TestWall::at(MIDDAY)))
}

fn query(words: &str, audience: AudienceFilter) -> GuidanceQuery {
    GuidanceQuery {
        terms: GuidanceTerms::parse(words).unwrap(),
        audience,
        page: NonZeroU32::MIN,
        page_size: PageSize::DEFAULT,
    }
}

fn id(raw: &str) -> SheetId {
    SheetId::parse(raw).unwrap()
}

fn hits(found: &GuidanceFound) -> Vec<(Audience, &str)> {
    found.page.hits.iter().map(|hit| (hit.audience, hit.id.as_str())).collect()
}

/// Une petite page : `<Publication>` au numéro `id`, avec `body` après son titre.
fn publication(id: &str, body: &str) -> Vec<u8> {
    format!(r#"<Publication ID="{id}"><dc:title>Page {id}</dc:title>{body}</Publication>"#)
        .into_bytes()
}

// ---- recherche ---------------------------------------------------------------------------------

#[tokio::test]
async fn a_search_reads_the_index_of_its_audience() {
    let modified = Utc.with_ymd_and_hms(2026, 11, 24, 14, 26, 9).unwrap();
    let download = Download { body: PART.to_vec(), etag: None, last_modified: Some(modified) };
    let fake = Fake::open([Ok(Fetched::Fresh(download))]);
    let found = source(&fake)
        .search(&query("solde bancaire insaisissable", AudienceFilter::Particuliers))
        .await
        .unwrap();
    assert_eq!(hits(&found)[0], (P, "F1437"));
    assert_eq!(
        found.page.hits[0].path,
        ["Justice", "Réparation du préjudice", "Saisies et recouvrements"]
    );
    assert_eq!(found.files, [ArchiveFile { audience: P, last_modified: Some(modified) }]);
    assert_eq!(fake.calls(), [call(P, None)]);
}

/// `all` : les deux téléchargements partent avant toute attente ; un numéro commun aux deux
/// archives n'est rendu qu'une fois, sous Particuliers.
#[tokio::test]
async fn a_search_on_all_loads_both_archives_together() {
    let fake = Fake::held([Ok(fresh(PART, "p")), Ok(fresh(PRO, "e"))]);
    let shared = Arc::new(source(&fake));
    let task = {
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            shared.search(&query("frais professionnels avantages", AudienceFilter::All)).await
        })
    };
    while fake.calls().len() < 2 {
        tokio::task::yield_now().await;
    }
    assert_eq!(fake.calls(), [call(P, None), call(E, None)]);
    fake.release();
    fake.release();
    let found = task.await.unwrap().unwrap();
    let common = hits(&found).into_iter().filter(|(_, id)| *id == "F2408").collect::<Vec<_>>();
    assert_eq!(common, [(P, "F2408")]);
    let audiences: Vec<Audience> = found.files.iter().map(|file| file.audience).collect();
    assert_eq!(audiences, [P, E]);
}

#[tokio::test]
async fn a_search_fails_with_its_archive() {
    let fake = Fake::open([Err(server_error())]);
    let refused = source(&fake).search(&query("loyer", AudienceFilter::Entreprendre)).await;
    assert_eq!(refused, Err(server_error()));
    assert_eq!(fake.calls(), [call(E, None)]);
}

// ---- lecture -----------------------------------------------------------------------------------

#[tokio::test]
async fn a_sheet_or_a_folder_is_read_from_the_archive_in_memory() {
    let fake = Fake::open([Ok(fresh(PART, "p"))]);
    let source = source(&fake);
    let read = source.get(P, &id("F1437")).await.unwrap();
    assert_eq!(read.doc.title, "Solde bancaire insaisissable (SBI)");
    assert_eq!(read.doc.path, ["Justice", "Réparation du préjudice", "Saisies et recouvrements"]);
    assert_eq!(read.doc.kind(), GuidanceKind::Sheet);
    assert_eq!(read.file, ArchiveFile { audience: P, last_modified: None });
    let folder = source.get(P, &id("N31675")).await.unwrap();
    assert!(
        matches!(folder.doc.content, GuidanceContent::Folder(ref entries) if entries.len() == 3)
    );
    // Une seule archive téléchargée pour tout cela.
    assert_eq!(fake.calls(), [call(P, None)]);
}

#[tokio::test]
async fn a_page_absent_from_the_archive_of_its_audience_is_not_found() {
    let fake = Fake::open([Ok(fresh(PART, "p")), Ok(fresh(PRO, "e"))]);
    let source = source(&fake);
    assert_eq!(source.get(P, &id("F9999")).await, Err(GuidanceError::NotFound));
    // F1437 n'est que dans l'archive de Particuliers.
    assert_eq!(source.get(E, &id("F1437")).await, Err(GuidanceError::NotFound));
}

#[tokio::test]
async fn a_page_over_two_mebibytes_is_refused_with_its_address() {
    let huge = publication(
        "F1",
        &format!("<Texte><Paragraphe>{}</Paragraphe></Texte>", "a".repeat(2 << 20)),
    );
    let small = publication("N5", "");
    let fake = Fake::open([Ok(fresh(&zip_of(&[("F1.xml", &huge), ("N5.xml", &small)]), "p"))]);
    let refused = source(&fake).get(E, &id("F1")).await;
    let source_url = ServicePublicUrl::read(None, E, &id("F1"));
    assert_eq!(source_url.as_str(), "https://entreprendre.service-public.gouv.fr/vosdroits/F1");
    assert_eq!(refused, Err(GuidanceError::EntryTooLarge { source_url }));
}

#[tokio::test]
async fn a_corrupt_entry_or_a_refused_page_is_a_decode_error() {
    let navigation = publication("N5", "");
    let elsewhere = publication("F5", "");
    // Octet faussé dans les données de `F6.xml`, après son en-tête local et son nom (36 octets).
    let mut bytes = zip_of(&[
        ("F6.xml", &publication("F6", "")),
        ("F3.xml", b"<pas du XML"),
        ("F4.xml", &elsewhere),
        ("N5.xml", &navigation),
    ]);
    bytes[40] ^= 0xFF;
    let fake = Fake::open([Ok(fresh(&bytes, "p"))]);
    let source = source(&fake);
    let decode = |detail: &str| Err(GuidanceError::Decode { detail: detail.to_owned() });
    assert_eq!(source.get(P, &id("F6")).await, decode("archive entry is corrupt"));
    assert_eq!(
        source.get(P, &id("F3")).await,
        decode("page refused: entry is not well-formed XML")
    );
    assert_eq!(
        source.get(P, &id("F4")).await,
        decode("page refused: publication id is missing, invalid or not the entry's")
    );
    assert_eq!(source.get(P, &id("N5")).await.map(|read| read.doc.title), Ok("Page N5".to_owned()));
}

#[tokio::test]
async fn a_read_fails_with_its_archive() {
    let fake = Fake::open([Err(server_error())]);
    assert_eq!(source(&fake).get(P, &id("F1437")).await, Err(server_error()));
}

// ---- bout en bout ------------------------------------------------------------------------------

async fn dila() -> MockServer {
    let server = MockServer::start().await;
    for (route, body) in [(PART_PATH, PART), (PRO_PATH, PRO)] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&server)
            .await;
    }
    server
}

/// G10 : les requêtes ne portent que les deux adresses fixes et trois en-têtes ; aucun mot de la
/// recherche, aucun numéro lu.
#[tokio::test]
async fn no_request_carries_a_search_word() {
    let server = dila().await;
    let guidance = ServicePublicGuidance::local(&server.uri(), WAIT);
    let found =
        guidance.search(&query("solde bancaire insaisissable", AudienceFilter::All)).await.unwrap();
    assert_eq!(hits(&found)[0], (P, "F1437"));
    assert!(guidance.get(E, &id("F37954")).await.is_ok());
    let requests = server.received_requests().await.unwrap();
    let mut seen: Vec<(&str, Option<&str>)> =
        requests.iter().map(|request| (request.url.path(), request.url.query())).collect();
    seen.sort_unstable();
    assert_eq!(seen, [(PART_PATH, None), (PRO_PATH, None)]);
    for request in &requests {
        let mut names: Vec<&str> =
            request.headers.keys().map(reqwest::header::HeaderName::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["accept", "host", "user-agent"]);
        assert_eq!(request.body, Vec::<u8>::new());
    }
}

#[tokio::test]
async fn a_read_sends_no_request_once_the_archive_is_loaded() {
    let server = dila().await;
    let guidance = ServicePublicGuidance::local(&server.uri(), WAIT);
    for raw in ["F1437", "F1239", "N31675", "F9999"] {
        let _ = guidance.get(P, &id(raw)).await;
    }
    assert!(guidance.search(&query("logement", AudienceFilter::Particuliers)).await.is_ok());
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), PART_PATH);
}

/// La source de production se construit sans E/S ni échec.
#[test]
fn the_production_source_is_built_without_io() {
    let config = ServicePublicConfig::new("mcp-legifrance/1.5.0".to_owned(), WAIT);
    let _guidance = ServicePublicGuidance::new(&config, TestWall::at(MIDDAY));
}
