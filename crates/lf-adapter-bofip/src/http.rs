//! Transport : un `GET`, une réponse bornée, des erreurs sans adresse (ADR-0029, décision 6).
//!
//! En `GET`, l'adresse porte les mots cherchés. Rien de ce qui sort de ce module ne la contient :
//! toute erreur de `reqwest` passe par `without_url()`, puis est réduite à l'un de quatre
//! libellés fixes. Le `message` d'un refus du portail recopie la requête : il n'est jamais lu,
//! seul son code l'est, et seulement s'il a la forme d'un [`PortalCode`].
//!
//! Aucun réessai, aucune redirection suivie. Le corps est compté **après** décompression : une
//! réponse gzip démesurée s'arrête à la borne.

use std::{fmt, time::Duration};

use lf_application::ports::{DoctrineError, PortalCode};
use reqwest::{
    Client, Response, Url,
    header::{ACCEPT, HeaderMap, HeaderValue, RETRY_AFTER},
    redirect,
};

use crate::dto::Refusal;

/// Taille maximale du corps d'une recherche, après décompression : 1 Mio. La plus grosse page
/// mesurée (100 résultats) fait moins de 60 Kio.
pub(crate) const SEARCH_MAX_BYTES: u64 = 1024 * 1024;

/// Taille maximale du corps d'une lecture, après décompression : 4 Mio. La plus longue
/// publication mesurée fait 182 Kio.
pub(crate) const GET_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Taille au-delà de laquelle le corps d'un refus n'est pas lu : 64 Kio.
const REFUSAL_MAX_BYTES: u64 = 64 * 1024;

/// Délai d'établissement de la connexion.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Réponse brute : statut, `Retry-After` et corps, sans interprétation.
///
/// `Debug` rédigé à la main : le corps n'est jamais affiché, seulement sa longueur.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Reply {
    /// Code de statut HTTP.
    pub(crate) status: u16,
    /// `Retry-After`, s'il est présent et en ASCII.
    pub(crate) retry_after: Option<String>,
    /// Corps décompressé. Pour un statut 200, entier et d'au plus la borne demandée ; pour un
    /// autre statut, vide s'il dépassait [`REFUSAL_MAX_BYTES`] ou n'a pas pu être lu.
    pub(crate) body: Vec<u8>,
}

impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reply")
            .field("status", &self.status)
            .field("retry_after", &self.retry_after)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Construit le client HTTP. `plain_http` n'est vrai que pour une base de test : le client de
/// production refuse toute adresse qui n'est pas en `https`.
///
/// # Errors
///
/// [`DoctrineError::Upstream`], sans adresse, si `reqwest` ne peut pas construire le client
/// (`User-Agent` invalide, TLS inutilisable).
pub(crate) fn build_client(
    user_agent: &str,
    request_timeout: Duration,
    plain_http: bool,
) -> Result<Client, DoctrineError> {
    let mut defaults = HeaderMap::new();
    defaults.insert(ACCEPT, HeaderValue::from_static("application/json"));
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        // Simple filet : le délai effectif est celui que `send_once` reçoit.
        .timeout(request_timeout)
        .user_agent(user_agent)
        .default_headers(defaults)
        .gzip(true)
        .redirect(redirect::Policy::none())
        .referer(false)
        .https_only(!plain_http)
        .build()
        .map_err(|_| DoctrineError::Upstream {
            status: None,
            retryable: false,
            detail: "could not build the HTTP client".to_owned(),
        })
}

/// Un seul `GET` de `url`, sans réessai ni jugement du statut. `timeout` remplace le délai du
/// client ; `limit` borne le corps d'une réponse 200.
///
/// # Errors
///
/// [`DoctrineError::Timeout`], [`DoctrineError::Upstream`] sans statut (panne réseau ou TLS,
/// adresse refusée par le client), [`DoctrineError::TooLarge`] si le corps d'une réponse 200
/// dépasse `limit`. Aucune ne porte l'adresse.
pub(crate) async fn send_once(
    client: &Client,
    url: Url,
    timeout: Duration,
    limit: u64,
) -> Result<Reply, DoctrineError> {
    let response = client.get(url).timeout(timeout).send().await.map_err(classify)?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = if status == 200 {
        read_body(response, limit).await?
    } else {
        // Le corps d'un refus ne sert qu'à lire son code : trop gros ou coupé, on s'en passe.
        read_body(response, REFUSAL_MAX_BYTES).await.unwrap_or_default()
    };
    Ok(Reply { status, retry_after, body })
}

/// Lit le corps en refusant de dépasser `limit` octets.
///
/// Le refus est immédiat quand `Content-Length` annonce plus que la borne (corps non compressé
/// seulement : `reqwest` n'expose pas la longueur d'une réponse gzip, qu'il décompresse en
/// flux). Sinon les morceaux, déjà décompressés, sont comptés.
async fn read_body(mut response: Response, limit: u64) -> Result<Vec<u8>, DoctrineError> {
    if response.content_length().is_some_and(|length| length > limit) {
        return Err(too_large(limit));
    }
    let mut body = Vec::new();
    let mut received: u64 = 0;
    while let Some(chunk) = response.chunk().await.map_err(classify)? {
        received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if received > limit {
            return Err(too_large(limit));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Erreur « corps trop gros », au libellé de la borne.
fn too_large(limit: u64) -> DoctrineError {
    DoctrineError::TooLarge { detail: format!("response body over {limit} bytes") }
}

/// Range une erreur de `reqwest` : délai, ou panne réseau avec l'un de trois libellés fixes.
/// L'adresse est retirée d'abord ; le texte de l'erreur n'est jamais recopié.
fn classify(error: reqwest::Error) -> DoctrineError {
    let error = error.without_url();
    classify_parts(error.is_timeout(), error.is_connect(), error.is_body() || error.is_decode())
}

/// Cœur de [`classify`], sur les constats de `reqwest` : `timeout`, panne de `connect` (phase de
/// connexion, TLS compris) et panne de `body` (corps interrompu ou gzip corrompu). Le délai
/// d'abord ; le reste n'est qu'un libellé pour les journaux.
fn classify_parts(timeout: bool, connect: bool, body: bool) -> DoctrineError {
    if timeout {
        return DoctrineError::Timeout;
    }
    let kind = if connect {
        "connect"
    } else if body {
        "body"
    } else {
        "request"
    };
    DoctrineError::Upstream {
        status: None,
        retryable: true,
        detail: format!("network error ({kind})"),
    }
}

/// Code d'un refus du portail, lu dans son corps : gardé seulement s'il a la forme d'un
/// [`PortalCode`]. Le `message` voisin n'est jamais lu.
pub(crate) fn refusal_code(body: &[u8]) -> Option<PortalCode> {
    let refusal: Refusal = serde_json::from_slice(body).ok()?;
    PortalCode::parse(refusal.error_code.as_deref()?)
}

/// Durée annoncée par un en-tête `Retry-After` en secondes entières. Une date HTTP, ou tout
/// autre texte, est traitée comme un en-tête absent.
pub(crate) fn retry_after(raw: Option<&str>) -> Option<Duration> {
    raw?.trim().parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;
    use crate::testing::{closed_port, fixture, gzip, raw_server};

    const AGENT: &str = "mcp-legifrance-tests/0";
    const SHORT: Duration = Duration::from_millis(200);
    const LONG: Duration = Duration::from_secs(20);

    fn test_client() -> Client {
        build_client(AGENT, LONG, true).unwrap()
    }

    fn url(base: &str, tail: &str) -> Url {
        Url::parse(&format!("{base}{tail}")).unwrap()
    }

    fn network(kind: &str) -> DoctrineError {
        DoctrineError::Upstream {
            status: None,
            retryable: true,
            detail: format!("network error ({kind})"),
        }
    }

    async fn server(template: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/records"))
            .respond_with(template)
            .mount(&server)
            .await;
        server
    }

    // ---- requête ---------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_get_carries_the_agent_and_asks_for_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/records"))
            .and(header("user-agent", AGENT))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("search-empty.json")))
            .expect(1)
            .mount(&server)
            .await;
        let reply =
            send_once(&test_client(), url(&server.uri(), "/records?limit=1"), LONG, 100).await;
        assert_eq!(
            reply,
            Ok(Reply { status: 200, retry_after: None, body: fixture("search-empty.json") })
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests[0].url.query(), Some("limit=1"));
        assert_eq!(requests[0].headers.get("referer"), None);
        // Tout ce qui part avec une requête : quatre en-têtes, sans cookie ni identifiant.
        let mut names: Vec<&str> =
            requests[0].headers.keys().map(reqwest::header::HeaderName::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["accept", "accept-encoding", "host", "user-agent"]);
        assert_eq!(requests[0].headers.get("accept-encoding").unwrap(), "gzip");
    }

    #[test]
    fn a_reply_is_printed_without_its_body() {
        let reply =
            Reply { status: 429, retry_after: Some("120".to_owned()), body: b"secret".to_vec() };
        assert_eq!(
            format!("{reply:?}"),
            r#"Reply { status: 429, retry_after: Some("120"), body_len: 6 }"#
        );
    }

    #[test]
    fn an_agent_that_is_not_a_header_value_builds_no_client() {
        let expected = DoctrineError::Upstream {
            status: None,
            retryable: false,
            detail: "could not build the HTTP client".to_owned(),
        };
        assert_eq!(build_client("agent\nsur deux lignes", LONG, false).err(), Some(expected));
    }

    // ---- requête fermée --------------------------------------------------------------------------

    /// Le client de production refuse une adresse `http` avant toute connexion : seule une base
    /// de test, construite avec `plain_http`, peut viser un serveur local.
    #[tokio::test]
    async fn production_client_refuses_plain_http() {
        let server = server(ResponseTemplate::new(200)).await;
        let production = build_client(AGENT, LONG, false).unwrap();
        let refused = send_once(&production, url(&server.uri(), "/records"), LONG, 100).await;
        assert_eq!(refused, Err(network("request")));
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
        // Le même appel passe avec le client de test : c'est bien le schéma qui est refusé.
        let allowed = send_once(&test_client(), url(&server.uri(), "/records"), LONG, 100).await;
        assert_eq!(allowed.map(|reply| reply.status), Ok(200));
    }

    #[tokio::test]
    async fn redirect_not_followed() {
        let target = server(ResponseTemplate::new(200)).await;
        let location = format!("{}/records", target.uri());
        for status in [301, 302, 303, 307, 308] {
            let template =
                ResponseTemplate::new(status).insert_header("location", location.as_str());
            let origin = server(template).await;
            let reply = send_once(&test_client(), url(&origin.uri(), "/records"), LONG, 100).await;
            assert_eq!(reply, Ok(Reply { status, retry_after: None, body: Vec::new() }));
        }
        assert_eq!(target.received_requests().await.unwrap().len(), 0);
    }

    // ---- corps borné ------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_body_at_the_limit_passes_and_one_byte_more_is_too_large() {
        let server = server(ResponseTemplate::new(200).set_body_bytes(vec![b'a'; 1000])).await;
        let client = test_client();
        let call = |limit| send_once(&client, url(&server.uri(), "/records"), LONG, limit);
        assert_eq!(call(1000).await.map(|reply| reply.body.len()), Ok(1000));
        assert_eq!(
            call(999).await,
            Err(DoctrineError::TooLarge { detail: "response body over 999 bytes".to_owned() })
        );
    }

    /// Une réponse gzip minuscule qui se déploie au-delà de la borne s'arrête à la borne : la
    /// taille est comptée après décompression.
    #[tokio::test]
    async fn gzip_bomb_capped_after_decompression() {
        let bomb = gzip(&vec![0_u8; 8 * 1024 * 1024]);
        assert!(bomb.len() < 16 * 1024);
        let template = ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .set_body_bytes(bomb);
        let server = server(template).await;
        let reply =
            send_once(&test_client(), url(&server.uri(), "/records"), LONG, SEARCH_MAX_BYTES).await;
        assert_eq!(
            reply,
            Err(DoctrineError::TooLarge { detail: "response body over 1048576 bytes".to_owned() })
        );
    }

    #[tokio::test]
    async fn a_gzip_body_under_the_limit_is_decompressed() {
        let plain = fixture("search-donation-page1.json");
        let template = ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .set_body_bytes(gzip(&plain));
        let server = server(template).await;
        let reply =
            send_once(&test_client(), url(&server.uri(), "/records"), LONG, SEARCH_MAX_BYTES).await;
        assert_eq!(reply.map(|reply| reply.body), Ok(plain));
    }

    #[test]
    fn the_two_limits_are_one_and_four_mebibytes() {
        assert_eq!((SEARCH_MAX_BYTES, GET_MAX_BYTES), (1_048_576, 4_194_304));
        assert_eq!(REFUSAL_MAX_BYTES, 65_536);
    }

    // ---- refus ------------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_refusal_keeps_its_status_its_retry_after_and_its_body() {
        let template = ResponseTemplate::new(429)
            .insert_header("retry-after", "120")
            .set_body_bytes(fixture("error-400-syntax.json"));
        let server = server(template).await;
        // La borne des réponses 200 ne s'applique pas au corps d'un refus.
        let reply = send_once(&test_client(), url(&server.uri(), "/records"), LONG, 1).await;
        assert_eq!(
            reply,
            Ok(Reply {
                status: 429,
                retry_after: Some("120".to_owned()),
                body: fixture("error-400-syntax.json"),
            })
        );
    }

    #[tokio::test]
    async fn a_refusal_body_over_64_kib_is_not_kept() {
        for (size, kept) in [(65_536, 65_536), (65_537, 0)] {
            let server = server(ResponseTemplate::new(500).set_body_bytes(vec![b'x'; size])).await;
            let reply = send_once(&test_client(), url(&server.uri(), "/records"), LONG, 1).await;
            assert_eq!(reply.map(|reply| (reply.status, reply.body.len())), Ok((500, kept)));
        }
    }

    #[tokio::test]
    async fn a_retry_after_that_is_not_ascii_is_dropped() {
        let template =
            ResponseTemplate::new(429).insert_header("retry-after", "demain à midi".as_bytes());
        let server = server(template).await;
        let reply = send_once(&test_client(), url(&server.uri(), "/records"), LONG, 1).await;
        assert_eq!(reply, Ok(Reply { status: 429, retry_after: None, body: Vec::new() }));
    }

    #[test]
    fn error_code_validated_message_dropped() {
        // Les deux refus réels du portail : le code est gardé, le message jamais lu.
        let rows = [
            ("error-400-syntax.json", "ODSQLSyntaxError"),
            ("error-400-limit.json", "InvalidRESTParameterError"),
        ];
        for (name, code) in rows {
            let body = fixture(name);
            assert!(String::from_utf8_lossy(&body).contains("\"message\""));
            assert_eq!(refusal_code(&body), PortalCode::parse(code));
            assert_eq!(
                refusal_code(&body).map(|code| code.as_str().to_owned()),
                Some(code.to_owned())
            );
        }
        // Un code qui n'a pas la forme attendue n'est pas gardé : il pourrait recopier la requête.
        let hostile: [&[u8]; 7] = [
            br#"{"error_code": "search(contenu, \"donation\")", "message": "m"}"#,
            br#"{"error_code": "Error 1"}"#,
            br#"{"error_code": ""}"#,
            br#"{"error_code": null, "message": "ODSQLSyntaxError"}"#,
            br#"{"message": "ODSQLSyntaxError"}"#,
            br#"{"error_code": 400}"#,
            b"<html>ODSQLSyntaxError</html>",
        ];
        for body in hostile {
            assert_eq!(refusal_code(body), None);
        }
        assert_eq!(refusal_code(b""), None);
    }

    #[test]
    fn retry_after_is_read_in_whole_seconds_only() {
        let rows = [
            (Some("120"), Some(120)),
            (Some(" 5 "), Some(5)),
            (Some("0"), Some(0)),
            (Some("86400"), Some(86_400)),
            (Some("Wed, 21 Oct 2026 07:28:00 GMT"), None),
            (Some("1.5"), None),
            (Some("-3"), None),
            (Some(""), None),
            (None, None),
        ];
        for (raw, seconds) in rows {
            assert_eq!(retry_after(raw), seconds.map(Duration::from_secs), "{raw:?}");
        }
    }

    // ---- pannes -----------------------------------------------------------------------------------

    #[test]
    fn a_failure_is_a_timeout_first_then_one_of_three_labels() {
        let rows = [
            ((true, false, false), DoctrineError::Timeout),
            ((true, true, true), DoctrineError::Timeout),
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
        let base = closed_port().await;
        let reply = send_once(&test_client(), url(&base, "/records"), LONG, 100).await;
        assert_eq!(reply, Err(network("connect")));
    }

    #[tokio::test]
    async fn a_slow_answer_is_a_timeout() {
        let server = server(ResponseTemplate::new(200).set_delay(Duration::from_secs(30))).await;
        let reply = send_once(&test_client(), url(&server.uri(), "/records"), SHORT, 100).await;
        assert_eq!(reply, Err(DoctrineError::Timeout));
    }

    #[tokio::test]
    async fn a_body_cut_short_is_a_body_failure() {
        let cut = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 500\r\n\r\n{\"total_count\"";
        let base = raw_server(cut).await;
        let reply = send_once(&test_client(), url(&base, "/records"), LONG, 1000).await;
        assert_eq!(reply, Err(network("body")));
    }

    #[tokio::test]
    async fn a_false_gzip_body_is_a_body_failure() {
        let template = ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .set_body_bytes(b"ceci n'est pas du gzip".to_vec());
        let server = server(template).await;
        let reply = send_once(&test_client(), url(&server.uri(), "/records"), LONG, 1000).await;
        assert_eq!(reply, Err(network("body")));
    }

    /// Le corps d'un refus coupé en route ne change pas le refus : le statut suffit.
    #[tokio::test]
    async fn a_refusal_whose_body_is_cut_short_keeps_its_status() {
        let cut = b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 500\r\n\r\n{\"error_code\"";
        let base = raw_server(cut).await;
        let reply = send_once(&test_client(), url(&base, "/records"), LONG, 1000).await;
        assert_eq!(reply, Ok(Reply { status: 503, retry_after: None, body: Vec::new() }));
    }

    /// Une poignée de main TLS qui échoue (ici : un serveur qui ne parle pas TLS) est une panne
    /// de connexion.
    #[tokio::test]
    async fn a_failed_tls_handshake_is_a_connect_failure() {
        let base = raw_server(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        let secure = base.replacen("http://", "https://", 1);
        let reply = send_once(&test_client(), url(&secure, "/records"), LONG, 100).await;
        assert_eq!(reply, Err(network("connect")));
    }
}
