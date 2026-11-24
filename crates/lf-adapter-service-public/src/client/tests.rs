//! Tests du client : faux serveur local (`wiremock`) ou serveur brut d'une connexion, jamais le
//! serveur de la DILA.

use chrono::TimeZone as _;
use pretty_assertions::assert_eq;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

use super::*;

const AGENT: &str = "mcp-legifrance-tests/0";
const SHORT: Duration = Duration::from_millis(200);
const LONG: Duration = Duration::from_secs(20);
const PART: &str = "/vdd/3.5/part/zip/vosdroits-latest.zip";
const PRO: &str = "/vdd/3.5/pro/zip/vosdroits-latest.zip";
const ORIGIN: &str = "https://lecomarquage.service-public.gouv.fr";
/// En-têtes réels de l'archive de Particuliers (relevé du 2026-11-24).
const REAL_ETAG: &str = "\"6b059e81-1734757\"";
const REAL_LAST_MODIFIED: &str = "Tue, 24 Nov 2026 14:26:09 GMT";

fn local(base: &str, timeout: Duration, limit: u64) -> ArchiveClient {
    ArchiveClient::local(base, timeout, limit)
}

fn client(server: &MockServer) -> ArchiveClient {
    local(&server.uri(), LONG, 1_000)
}

async fn server(template: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(template).mount(&server).await;
    server
}

fn upstream_error(status: Option<u16>, retryable: bool, detail: &str) -> GuidanceError {
    GuidanceError::Upstream { status, retryable, detail: detail.to_owned() }
}

fn network(kind: &str) -> GuidanceError {
    upstream_error(None, true, &format!("network error ({kind})"))
}

/// Base `http://127.0.0.1:<port>` d'un serveur d'une seule connexion, qui répond `response` tel
/// quel puis ferme.
async fn raw_server(response: &[u8]) -> String {
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

/// Réponse 200 en morceaux (`chunked`, sans `Content-Length`) de `sizes` octets chacun.
fn chunked(sizes: &[usize]) -> Vec<u8> {
    let mut response = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n".to_vec();
    for size in sizes {
        response.extend_from_slice(format!("{size:x}\r\n").as_bytes());
        response.extend(std::iter::repeat_n(b'a', *size));
        response.extend_from_slice(b"\r\n");
    }
    response.extend_from_slice(b"0\r\n\r\n");
    response
}

// ---- requête fermée ----------------------------------------------------------------------------

/// G10 : la requête ne porte que l'adresse fixe du public et trois en-têtes ; ni mot cherché, ni
/// numéro de fiche, ni cookie, ni compression demandée.
#[tokio::test]
async fn no_request_carries_a_search_word() {
    let server = server(ResponseTemplate::new(200).set_body_bytes(b"PK".to_vec())).await;
    let archive = client(&server);
    for audience in [Audience::Particuliers, Audience::Entreprendre] {
        assert!(archive.fetch(audience, None).await.is_ok());
    }
    let requests = server.received_requests().await.unwrap();
    let paths: Vec<(&str, Option<&str>)> =
        requests.iter().map(|request| (request.url.path(), request.url.query())).collect();
    assert_eq!(paths, [(PART, None), (PRO, None)]);
    for request in &requests {
        let mut names: Vec<&str> =
            request.headers.keys().map(reqwest::header::HeaderName::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["accept", "host", "user-agent"]);
        assert_eq!(request.headers.get("user-agent").unwrap(), AGENT);
    }
}

#[test]
fn production_targets_the_two_fixed_addresses_with_its_bounds() {
    let production = ArchiveClient::production(AGENT);
    assert_eq!(production.url(Audience::Particuliers), format!("{ORIGIN}{PART}"));
    assert_eq!(production.url(Audience::Entreprendre), format!("{ORIGIN}{PRO}"));
    assert_eq!(production.limit, 67_108_864);
    assert_eq!(DOWNLOAD_TIMEOUT, Duration::from_secs(300));
    assert_eq!((REST_DEFAULT, REST_MAX), (Duration::from_secs(900), Duration::from_secs(3_600)));
}

/// Un client qui ne se construit pas refuse chaque appel, sans réseau ; la construction n'est
/// tentée qu'une fois.
#[tokio::test]
async fn an_agent_that_is_not_a_header_value_builds_no_client() {
    let refused = ArchiveClient::production("agent\nsur deux lignes");
    let expected = upstream_error(None, false, "could not build the HTTP client");
    for audience in [Audience::Particuliers, Audience::Entreprendre] {
        assert_eq!(refused.fetch(audience, None).await, Err(expected.clone()));
    }
}

/// Le client de production refuse une adresse `http` avant toute connexion.
#[tokio::test]
async fn production_client_refuses_plain_http() {
    let server = server(ResponseTemplate::new(200)).await;
    let production = ArchiveClient::with(
        Target { user_agent: AGENT, timeout: LONG, plain_http: false },
        format!("{}{PART}", server.uri()),
        format!("{}{PRO}", server.uri()),
        1_000,
    );
    let refused = production.fetch(Audience::Particuliers, None).await;
    assert_eq!(refused, Err(network("request")));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn no_redirect_is_followed() {
    let target = server(ResponseTemplate::new(200)).await;
    let location = format!("{}{PART}", target.uri());
    for status in [301, 302, 303, 307, 308] {
        let origin =
            server(ResponseTemplate::new(status).insert_header("location", location.as_str()))
                .await;
        let reply = client(&origin).fetch(Audience::Particuliers, None).await;
        assert_eq!(reply, Err(upstream_error(Some(status), false, "unexpected status")));
    }
    assert_eq!(target.received_requests().await.unwrap().len(), 0);
}

// ---- archive reçue -----------------------------------------------------------------------------

#[tokio::test]
async fn an_archive_comes_with_its_etag_and_date() {
    let template = ResponseTemplate::new(200)
        .insert_header("etag", REAL_ETAG)
        .insert_header("last-modified", REAL_LAST_MODIFIED)
        .set_body_bytes(vec![b'z'; 1_000]);
    let reply = client(&server(template).await).fetch(Audience::Entreprendre, None).await;
    let expected = Download {
        body: vec![b'z'; 1_000],
        etag: Some(REAL_ETAG.to_owned()),
        last_modified: Some(Utc.with_ymd_and_hms(2026, 11, 24, 14, 26, 9).unwrap()),
    };
    assert_eq!(reply, Ok(Fetched::Fresh(expected)));
}

#[tokio::test]
async fn odd_validators_are_dropped() {
    let long = "x".repeat(ETAG_MAX_BYTES + 1);
    let rows: [(&[u8], &[u8]); 4] = [
        (b"", b"hier"),
        (long.as_bytes(), "mardi 24 novembre".as_bytes()),
        ("\"é\"".as_bytes(), "Tue, 24 Nov 2026 14:26:09 GMT à Paris".as_bytes()),
        (b"\"ok\"", b"Tuesday, 24-Nov-26 14:26:09 GMT"),
    ];
    for (etag, date) in rows {
        let template = ResponseTemplate::new(200)
            .insert_header("etag", etag)
            .insert_header("last-modified", date);
        let reply = client(&server(template).await).fetch(Audience::Particuliers, None).await;
        let kept = (etag == b"\"ok\"").then(|| "\"ok\"".to_owned());
        let expected = Download { body: Vec::new(), etag: kept, last_modified: None };
        assert_eq!(reply, Ok(Fetched::Fresh(expected)));
    }
    // Un décalage horaire est ramené à UTC ; un `ETag` de la longueur maximale est gardé.
    let at_bound = "y".repeat(ETAG_MAX_BYTES);
    let template = ResponseTemplate::new(200)
        .insert_header("etag", at_bound.as_str())
        .insert_header("last-modified", "Tue, 24 Nov 2026 15:26:09 +0100");
    let reply = client(&server(template).await).fetch(Audience::Particuliers, None).await;
    let expected = Download {
        body: Vec::new(),
        etag: Some(at_bound),
        last_modified: Some(Utc.with_ymd_and_hms(2026, 11, 24, 14, 26, 9).unwrap()),
    };
    assert_eq!(reply, Ok(Fetched::Fresh(expected)));
}

#[test]
fn a_download_is_printed_without_its_body() {
    let download = Download { body: b"secret".to_vec(), etag: None, last_modified: None };
    assert_eq!(
        format!("{download:?}"),
        "Download { body_len: 6, etag: None, last_modified: None }"
    );
}

// ---- corps borné -------------------------------------------------------------------------------

#[tokio::test]
async fn a_body_over_the_cap_is_refused() {
    let over = GuidanceError::TooLarge { detail: "archive over 1000 bytes".to_owned() };
    // Annoncé par `Content-Length` : refusé avant lecture.
    let template = ResponseTemplate::new(200).set_body_bytes(vec![b'a'; 1_001]);
    let reply = client(&server(template).await).fetch(Audience::Particuliers, None).await;
    assert_eq!(reply, Err(over.clone()));
    // Sans longueur annoncée : les morceaux sont comptés, la borne tient à l'octet près.
    let base = raw_server(&chunked(&[600, 400])).await;
    let reply = local(&base, LONG, 1_000).fetch(Audience::Particuliers, None).await;
    let whole = Download { body: vec![b'a'; 1_000], etag: None, last_modified: None };
    assert_eq!(reply, Ok(Fetched::Fresh(whole)));
    let base = raw_server(&chunked(&[600, 401])).await;
    let reply = local(&base, LONG, 1_000).fetch(Audience::Particuliers, None).await;
    assert_eq!(reply, Err(over));
}

// ---- revalidation ------------------------------------------------------------------------------

#[tokio::test]
async fn a_304_keeps_the_archive() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PART))
        .and(header("if-none-match", REAL_ETAG))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    let reply = client(&server).fetch(Audience::Particuliers, Some(REAL_ETAG)).await;
    assert_eq!(reply, Ok(Fetched::NotModified));
}

/// Un `304` qui ne répond à aucune requête conditionnelle n'a rien à garder.
#[tokio::test]
async fn a_304_without_a_condition_is_refused() {
    let server = server(ResponseTemplate::new(304)).await;
    let reply = client(&server).fetch(Audience::Particuliers, None).await;
    assert_eq!(reply, Err(upstream_error(Some(304), false, "unexpected status")));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("if-none-match"), None);
}

// ---- statuts -----------------------------------------------------------------------------------

#[tokio::test]
async fn each_refused_status_gives_its_error() {
    let rows = [
        (500, upstream_error(Some(500), true, "server error")),
        (503, upstream_error(Some(503), true, "server error")),
        (599, upstream_error(Some(599), true, "server error")),
        (204, upstream_error(Some(204), false, "unexpected status")),
        (403, upstream_error(Some(403), false, "unexpected status")),
        (404, upstream_error(Some(404), false, "unexpected status")),
        (499, upstream_error(Some(499), false, "unexpected status")),
    ];
    for (status, expected) in rows {
        let reply = client(&server(ResponseTemplate::new(status)).await)
            .fetch(Audience::Particuliers, None)
            .await;
        assert_eq!(reply, Err(expected));
    }
}

#[tokio::test]
async fn a_429_rests_the_source_without_retry() {
    let server = server(ResponseTemplate::new(429).insert_header("retry-after", "120")).await;
    let archive = client(&server);
    let first = archive.fetch(Audience::Particuliers, None).await;
    assert_eq!(
        first,
        Err(GuidanceError::RateLimited { retry_after: Some(Duration::from_secs(120)) })
    );
    // Pendant le repos, rien ne part, pour aucun des deux publics.
    for audience in [Audience::Particuliers, Audience::Entreprendre] {
        let reply = archive.fetch(audience, Some(REAL_ETAG)).await;
        let resting = |left: Duration| left <= Duration::from_secs(120) && left.as_secs() >= 100;
        assert!(
            matches!(reply, Err(GuidanceError::RateLimited { retry_after: Some(left) }) if resting(left)),
            "{reply:?}"
        );
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    // Sans `Retry-After`, le repos par défaut.
    let server = server_429_without_header().await;
    let reply = client(&server).fetch(Audience::Entreprendre, None).await;
    assert_eq!(reply, Err(GuidanceError::RateLimited { retry_after: Some(REST_DEFAULT) }));
}

async fn server_429_without_header() -> MockServer {
    server(ResponseTemplate::new(429)).await
}

#[test]
fn a_rest_ends_at_its_length() {
    let rest = Rest::default();
    let start = Instant::now();
    let at = |seconds: u64| start.checked_add(Duration::from_secs(seconds)).unwrap();
    assert_eq!(rest.left(start), None);
    assert_eq!(rest.start(start, Some(Duration::from_secs(120))), Duration::from_secs(120));
    assert_eq!(rest.left(start), Some(Duration::from_secs(120)));
    assert_eq!(rest.left(at(119)), Some(Duration::from_secs(1)));
    assert_eq!(rest.left(at(120)), None);
    assert_eq!(rest.left(at(500)), None);
}

#[test]
fn a_rest_is_the_announced_length_bounded() {
    let rows = [
        (None, REST_DEFAULT),
        (Some(0), REST_DEFAULT),
        (Some(1), Duration::from_secs(1)),
        (Some(120), Duration::from_secs(120)),
        (Some(3_600), REST_MAX),
        (Some(86_400), REST_MAX),
    ];
    for (announced, length) in rows {
        assert_eq!(rest_length(announced.map(Duration::from_secs)), length, "{announced:?}");
    }
}

#[test]
fn retry_after_is_read_in_whole_seconds_only() {
    let rows: [(&[u8], Option<u64>); 7] = [
        (b"120", Some(120)),
        (b" 5 ", Some(5)),
        (b"0", Some(0)),
        (b"Wed, 21 Oct 2026 07:28:00 GMT", None),
        (b"1.5", None),
        (b"-3", None),
        ("demain à midi".as_bytes(), None),
    ];
    for (raw, seconds) in rows {
        let value = HeaderValue::from_bytes(raw).unwrap();
        assert_eq!(retry_after(Some(&value)), seconds.map(Duration::from_secs), "{raw:?}");
    }
    assert_eq!(retry_after(None), None);
}

// ---- pannes ------------------------------------------------------------------------------------

#[test]
fn a_failure_is_a_timeout_first_then_one_of_three_labels() {
    let rows = [
        ((true, false, false), GuidanceError::Timeout),
        ((true, true, true), GuidanceError::Timeout),
        ((false, true, false), network("connect")),
        ((false, true, true), network("connect")),
        ((false, false, true), network("body")),
        ((false, false, false), network("request")),
    ];
    for ((timeout, connect, body), expected) in rows {
        assert_eq!(classify_parts(timeout, connect, body), expected);
    }
}

#[tokio::test]
async fn a_closed_port_is_a_connect_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let reply = local(&base, LONG, 1_000).fetch(Audience::Particuliers, None).await;
    assert_eq!(reply, Err(network("connect")));
}

#[tokio::test]
async fn a_slow_answer_is_a_timeout() {
    let server = server(ResponseTemplate::new(200).set_delay(Duration::from_secs(30))).await;
    let reply = local(&server.uri(), SHORT, 1_000).fetch(Audience::Particuliers, None).await;
    assert_eq!(reply, Err(GuidanceError::Timeout));
}

#[tokio::test]
async fn a_body_cut_short_is_a_body_failure() {
    let base = raw_server(b"HTTP/1.1 200 OK\r\ncontent-length: 500\r\n\r\nPK\x03\x04").await;
    let reply = local(&base, LONG, 1_000).fetch(Audience::Particuliers, None).await;
    assert_eq!(reply, Err(network("body")));
}

/// Une poignée de main TLS qui échoue (un serveur qui ne parle pas TLS) est une panne de
/// connexion.
#[tokio::test]
async fn a_failed_tls_handshake_is_a_connect_failure() {
    let base = raw_server(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
    let secure = base.replacen("http://", "https://", 1);
    let reply = local(&secure, LONG, 1_000).fetch(Audience::Particuliers, None).await;
    assert_eq!(reply, Err(network("connect")));
}
