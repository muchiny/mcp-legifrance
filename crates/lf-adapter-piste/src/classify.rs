//! Classification d'une tentative d'envoi : succès, jeton refusé, échec passager ou définitif.
//!
//! Classification de transport seulement : le corps n'est jamais lu. Seuls le statut HTTP,
//! `Retry-After` et `Date` comptent ; seul `post_json` décode le corps des 2xx, et un corps
//! d'erreur n'est jamais lu au plan 2c (T-M2-08). Le choix de réessayer, d'attendre ou de
//! renoncer appartient à `retry::run` ; ce module ne dort pas et ne lit aucune horloge.

use std::time::Duration;

use lf_application::ports::CorpusError;
use reqwest::Client;

use crate::http::{Attempt, RawResponse, TransportError, send_once};
use crate::retry::parse_retry_after;

/// Plus longue attente propagée dans [`CorpusError::RateLimited`].
///
/// Un `Retry-After` hostile (`u64::MAX` secondes, ou une date en l'an 9999) ne doit pas
/// atteindre l'appelant tel quel : un `Instant::now() + d` y paniquerait, et le modèle lirait
/// « réessayez dans 584 milliards d'années ». Seule la valeur rangée dans l'erreur est
/// écrêtée ; la décision d'attendre ou de refuser (`retry::run`, au plus 30 s) garde la valeur
/// annoncée.
const MAX_REPORTED_RETRY_AFTER: Duration = Duration::from_secs(3_600);

/// Budget de réessais que consomme un échec passager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Budget {
    /// Budget ordinaire : trois requêtes au plus.
    Standard,
    /// Budget réduit du 500 : deux requêtes au plus, ce statut étant le plus souvent durable.
    Reduced,
}

/// Issue classée d'une tentative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome<T> {
    /// Réponse 2xx : la valeur est celle que la tentative a produite.
    Success(T),
    /// Réponse 401 : porte l'erreur **finale** `Auth`, car seul `classify` connaît le gabarit
    /// d'endpoint. Le renouvellement du jeton est du ressort de l'appelant ; si l'issue atteint
    /// `retry::run`, elle y est définitive.
    Unauthorized(CorpusError),
    /// Échec qui mérite une nouvelle tentative, si le budget et le délai le permettent.
    Transient {
        /// Erreur à rendre si l'on renonce : déjà finale, la boucle ne la réécrit pas.
        error: CorpusError,
        /// Attente annoncée par le serveur (`Retry-After`), s'il y en a une d'exploitable.
        retry_after: Option<Duration>,
        /// Budget que cet échec consomme.
        budget: Budget,
    },
    /// Échec définitif : inutile de réessayer.
    Fatal(CorpusError),
}

/// Classe le résultat d'une tentative d'envoi.
///
/// `endpoint` est le gabarit de chemin (`/consult/juri`), seule trace de la route dans les
/// `detail` des erreurs : jamais d'URL, de jeton ni de corps.
pub(crate) fn classify(
    result: Result<RawResponse, TransportError>,
    endpoint: &str,
) -> Outcome<RawResponse> {
    match result {
        Ok(response) => classify_status(response, endpoint),
        Err(error) => classify_transport(error),
    }
}

/// Classe une réponse HTTP d'après son seul statut ; `Retry-After` n'est lu que pour 429, 502,
/// 503 et 504.
fn classify_status(response: RawResponse, endpoint: &str) -> Outcome<RawResponse> {
    let status = response.status;
    let detail = || format!("HTTP {status} on {endpoint}");
    let retry_after =
        || parse_retry_after(response.retry_after.as_deref(), response.date.as_deref());
    match status {
        200..=299 => Outcome::Success(response),
        401 => Outcome::Unauthorized(CorpusError::Auth { detail: detail() }),
        403 => Outcome::Fatal(CorpusError::Auth { detail: detail() }),
        429 => {
            let retry_after = retry_after();
            Outcome::Transient {
                error: CorpusError::RateLimited {
                    retry_after: retry_after.map(|wait| wait.min(MAX_REPORTED_RETRY_AFTER)),
                },
                retry_after,
                budget: Budget::Standard,
            }
        }
        // L'erreur est déjà finale (`retryable: false`) : la boucle ne réécrit rien.
        500 => Outcome::Transient {
            error: CorpusError::Upstream {
                status: Some(status),
                retryable: false,
                detail: detail(),
            },
            retry_after: None,
            budget: Budget::Reduced,
        },
        502..=504 => Outcome::Transient {
            error: CorpusError::Upstream {
                status: Some(status),
                retryable: true,
                detail: detail(),
            },
            retry_after: retry_after(),
            budget: Budget::Standard,
        },
        400 => {
            Outcome::Fatal(CorpusError::InvalidRequest { status: Some(status), detail: detail() })
        }
        // Choix provisoire : jamais `NotFound` ici, la distinction d'une consultation
        // introuvable est T-M2-08 (04 §11.2).
        404 | 405 => Outcome::Fatal(CorpusError::Config { detail: detail() }),
        _ => Outcome::Fatal(CorpusError::Upstream {
            status: Some(status),
            retryable: false,
            detail: detail(),
        }),
    }
}

/// Classe un échec de transport : délai et réseau sont passagers, TLS et corps trop gros non.
fn classify_transport(error: TransportError) -> Outcome<RawResponse> {
    match error {
        TransportError::Timeout | TransportError::Network { .. } => {
            Outcome::Transient { error: error.into(), retry_after: None, budget: Budget::Standard }
        }
        TransportError::Tls { .. } | TransportError::TooLarge { .. } => {
            Outcome::Fatal(error.into())
        }
    }
}

/// Une tentative d'envoi puis sa classification.
pub(crate) async fn try_send(client: &Client, attempt: &Attempt<'_>) -> Outcome<RawResponse> {
    classify(send_once(client, attempt).await, attempt.endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "/consult/juri";
    const DATE: &str = "Sun, 06 Nov 1994 08:49:37 GMT";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn response(status: u16) -> RawResponse {
        RawResponse { status, retry_after: None, date: None, body: b"{}".to_vec() }
    }

    fn with_headers(status: u16, retry_after: Option<&str>, date: Option<&str>) -> RawResponse {
        RawResponse {
            retry_after: retry_after.map(str::to_owned),
            date: date.map(str::to_owned),
            ..response(status)
        }
    }

    fn classified(status: u16) -> Outcome<RawResponse> {
        classify(Ok(response(status)), ENDPOINT)
    }

    fn detail(status: u16) -> String {
        format!("HTTP {status} on {ENDPOINT}")
    }

    fn upstream(status: u16, retryable: bool) -> CorpusError {
        CorpusError::Upstream { status: Some(status), retryable, detail: detail(status) }
    }

    fn transient(error: CorpusError, retry_after: Option<Duration>) -> Outcome<RawResponse> {
        Outcome::Transient { error, retry_after, budget: Budget::Standard }
    }

    #[test]
    fn every_2xx_is_a_success_carrying_the_response() {
        for status in [200, 201, 202, 204, 206, 299] {
            assert_eq!(classified(status), Outcome::Success(response(status)), "status {status}");
        }
    }

    #[test]
    fn the_status_just_outside_2xx_is_not_a_success() {
        for status in [199, 300] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    #[test]
    fn redirects_are_final_upstream_errors() {
        for status in [301, 302, 303, 304, 307, 308, 399] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    #[test]
    fn status_400_is_invalid_request_with_a_generic_detail() {
        assert_eq!(
            classified(400),
            Outcome::Fatal(CorpusError::InvalidRequest {
                status: Some(400),
                detail: "HTTP 400 on /consult/juri".to_owned(),
            })
        );
    }

    #[test]
    fn the_body_of_a_400_is_never_read() {
        let mut secret = response(400);
        secret.body = br#"{"message":"secret-ish"}"#.to_vec();
        assert_eq!(
            classify(Ok(secret), ENDPOINT),
            Outcome::Fatal(CorpusError::InvalidRequest {
                status: Some(400),
                detail: "HTTP 400 on /consult/juri".to_owned(),
            })
        );
    }

    #[test]
    fn status_401_is_unauthorized_with_the_final_auth_error() {
        assert_eq!(
            classified(401),
            Outcome::Unauthorized(CorpusError::Auth {
                detail: "HTTP 401 on /consult/juri".to_owned()
            })
        );
    }

    #[test]
    fn status_403_is_a_final_auth_error() {
        assert_eq!(
            classified(403),
            Outcome::Fatal(CorpusError::Auth { detail: "HTTP 403 on /consult/juri".to_owned() })
        );
    }

    #[test]
    fn statuses_404_and_405_are_config_never_not_found() {
        for status in [404, 405] {
            assert_eq!(
                classified(status),
                Outcome::Fatal(CorpusError::Config { detail: detail(status) })
            );
        }
    }

    #[test]
    fn other_4xx_are_final_upstream_errors() {
        for status in [402, 406, 408, 409, 410, 413, 422, 451, 499] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    #[test]
    fn status_429_is_transient_rate_limited_without_retry_after() {
        assert_eq!(
            classified(429),
            transient(CorpusError::RateLimited { retry_after: None }, None)
        );
    }

    #[test]
    fn status_429_carries_the_retry_after_in_both_places() {
        assert_eq!(
            classify(Ok(with_headers(429, Some("7"), None)), ENDPOINT),
            transient(CorpusError::RateLimited { retry_after: Some(secs(7)) }, Some(secs(7)))
        );
    }

    #[test]
    fn the_retry_after_kept_in_the_429_error_is_capped_at_one_hour() {
        // (secondes annoncées, secondes rangées dans l'erreur)
        for (announced, kept) in [
            ("18446744073709551615", 3_600),
            ("99999999999999999999", 3_600),
            ("120", 120),
            ("3600", 3_600),
            ("3601", 3_600),
        ] {
            let raw = Some(Duration::from_secs(announced.parse().unwrap_or(u64::MAX)));
            assert_eq!(
                classify(Ok(with_headers(429, Some(announced), None)), ENDPOINT),
                // La boucle de réessai garde la valeur annoncée pour décider d'attendre ou de
                // refuser ; seule la valeur propagée à l'appelant est écrêtée.
                transient(CorpusError::RateLimited { retry_after: Some(secs(kept)) }, raw),
                "{announced}"
            );
        }
    }

    #[test]
    fn status_500_is_transient_on_the_reduced_budget_with_its_final_error() {
        assert_eq!(
            classified(500),
            Outcome::Transient {
                error: upstream(500, false),
                retry_after: None,
                budget: Budget::Reduced,
            }
        );
    }

    #[test]
    fn retry_after_is_not_read_on_a_500() {
        assert_eq!(
            classify(Ok(with_headers(500, Some("7"), None)), ENDPOINT),
            Outcome::Transient {
                error: upstream(500, false),
                retry_after: None,
                budget: Budget::Reduced,
            }
        );
    }

    #[test]
    fn statuses_502_503_504_are_transient_and_retryable() {
        for status in [502, 503, 504] {
            assert_eq!(classified(status), transient(upstream(status, true), None));
        }
    }

    #[test]
    fn statuses_502_503_504_honour_retry_after() {
        for status in [502, 503, 504] {
            assert_eq!(
                classify(Ok(with_headers(status, Some("12"), None)), ENDPOINT),
                transient(upstream(status, true), Some(secs(12)))
            );
        }
    }

    #[test]
    fn retry_after_given_as_an_http_date_uses_the_date_header_of_the_same_response() {
        let reply = with_headers(429, Some("Sun, 06 Nov 1994 08:49:40 GMT"), Some(DATE));
        assert_eq!(
            classify(Ok(reply), ENDPOINT),
            transient(CorpusError::RateLimited { retry_after: Some(secs(3)) }, Some(secs(3)))
        );
    }

    #[test]
    fn unusable_retry_after_is_ignored() {
        assert_eq!(
            classify(Ok(with_headers(503, Some("soon"), Some(DATE))), ENDPOINT),
            transient(upstream(503, true), None)
        );
    }

    #[test]
    fn other_5xx_are_final_upstream_errors() {
        for status in [501, 505, 507, 511, 599, 600, 999] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    fn kind(outcome: &Outcome<RawResponse>) -> &'static str {
        match outcome {
            Outcome::Success(_) => "success",
            Outcome::Unauthorized(_) => "unauthorized",
            Outcome::Transient { .. } => "transient",
            Outcome::Fatal(_) => "fatal",
        }
    }

    #[test]
    fn every_status_has_exactly_the_class_of_the_table() {
        for status in 100..=999 {
            let expected = match status {
                200..=299 => "success",
                401 => "unauthorized",
                429 | 500 | 502..=504 => "transient",
                _ => "fatal",
            };
            assert_eq!(kind(&classified(status)), expected, "status {status}");
        }
    }

    #[test]
    fn transport_timeout_is_transient_timeout() {
        assert_eq!(
            classify(Err(TransportError::Timeout), ENDPOINT),
            transient(CorpusError::Timeout, None)
        );
    }

    #[test]
    fn network_error_is_transient_and_retryable() {
        let detail = "network error (connect) on /consult/juri".to_owned();
        assert_eq!(
            classify(Err(TransportError::Network { detail: detail.clone() }), ENDPOINT),
            transient(CorpusError::Upstream { status: None, retryable: true, detail }, None)
        );
    }

    #[test]
    fn tls_failure_is_final() {
        let detail = "TLS handshake failed on /consult/juri".to_owned();
        assert_eq!(
            classify(Err(TransportError::Tls { detail: detail.clone() }), ENDPOINT),
            Outcome::Fatal(CorpusError::Upstream { status: None, retryable: false, detail })
        );
    }

    #[test]
    fn oversized_body_is_a_final_decode_error() {
        let detail = "response too large (> 16 MiB) on /consult/juri".to_owned();
        assert_eq!(
            classify(Err(TransportError::TooLarge { detail: detail.clone() }), ENDPOINT),
            Outcome::Fatal(CorpusError::Decode { detail })
        );
    }
}
