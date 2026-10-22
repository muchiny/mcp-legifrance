//! Politique de réessai : boucle de tentatives, délai exponentiel avec gigue et `Retry-After`.
//!
//! Ni `backon` ni `backoff` : le spike V-06 a montré que `backon` 1.6.0 tient la politique
//! mais n'apporte rien ici. Son `Sleeper` est `'static` (il faudrait un `Arc` du `Sleeper` du
//! crate), le budget propre au 500 passerait par un `when` à effet de bord lié à son ordre
//! d'appel interne, l'erreur devrait être le `E` d'un `Result` (donc `Outcome` deviendrait un
//! `Result<_, Transient>`), le 401 et le permis de débit restent dans la fermeture de tentative,
//! et il ajoute une dépendance à âge-vérifier. Une boucle de vingt-cinq lignes (`run`) et un
//! calcul de délai de cinq (`backoff`) suffisent ; l'aléa de la gigue s'injecte par le trait
//! [`Jitter`], ce que `backon` ne permet que par une graine.
//!
//! Toute attente passe par le [`Sleeper`] du crate et tout instant par la [`MonoClock`] : aucune
//! lecture d'horloge murale. `chrono` ne sert qu'à lire les dates HTTP de `Retry-After` et de
//! `Date`.

use std::future::Future;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::time::Duration;

use chrono::DateTime;
use lf_application::ports::CorpusError;

use crate::classify::{Budget, Outcome};
use crate::deadline::CallDeadline;
use crate::time::{MonoClock, Sleeper};

/// Requêtes au plus par appel, la première comprise.
const MAX_ATTEMPTS: u32 = 3;
/// Requêtes au plus quand des 500 sont vus : le deuxième 500 arrête la boucle.
const MAX_REDUCED: u32 = 2;
/// Attente de base avant la première reprise ; elle double à chaque reprise.
const BASE_WAIT: Duration = Duration::from_millis(500);
/// Plus longue `Retry-After` que l'on accepte de dormir.
///
/// Décision provisoire (la spec dit « plafonnée à 30 s », ambiguë) : au-delà, **refus immédiat,
/// pas d'écrêtage**. L'erreur classée est rendue telle quelle, donc `RateLimited` porte
/// l'attente annoncée pour un 429 (écrêtée à une heure par `classify`, pour la seule valeur
/// propagée). Écrêter ferait réessayer un serveur qui a dit « pas avant
/// 2 min » : quota gaspillé et nouvel échec presque certain. Exactement 30 s se dort, 30,001 s
/// est refusé.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

/// Attente annoncée par `Retry-After`, en secondes entières ou en date HTTP.
///
/// Un nombre de secondes trop grand pour un `u64` **sature** au lieu d'être ignoré : la boucle
/// le refusera. Une date HTTP se lit contre le `Date` de la même réponse, jamais contre une
/// horloge locale : une date égale donne zéro, une date passée ou illisible donne `None`.
pub(crate) fn parse_retry_after(retry_after: Option<&str>, date: Option<&str>) -> Option<Duration> {
    let text = retry_after?.trim();
    if text.bytes().all(|byte| byte.is_ascii_digit()) {
        if text.is_empty() {
            return None;
        }
        Some(Duration::from_secs(text.parse().unwrap_or(u64::MAX)))
    } else {
        http_date_delay(text, date?.trim())
    }
}

/// Écart entre une date HTTP cible et la date d'envoi de la réponse.
///
/// `chrono` accepte `GMT` (IMF-fixdate) et vérifie la concordance du jour de semaine ;
/// `to_std` refuse une différence négative, ce qui écarte une date passée.
fn http_date_delay(target: &str, sent: &str) -> Option<Duration> {
    let target = DateTime::parse_from_rfc2822(target).ok()?;
    let sent = DateTime::parse_from_rfc2822(sent).ok()?;
    target.signed_duration_since(sent).to_std().ok()
}

/// Source d'aléa de la gigue.
pub(crate) trait Jitter: Send + Sync + 'static {
    /// Durée uniforme dans `[0, bound)`.
    fn below(&self, bound: Duration) -> Duration;
}

/// Gigue de production.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemJitter;

impl Jitter for SystemJitter {
    /// Tire dans `[0, bound)` avec les clés `SipHash` qu'`RandomState` reçoit de l'OS : le condensé
    /// d'une entrée vide est un `u64` imprévisible. Pas de la cryptographie.
    fn below(&self, bound: Duration) -> Duration {
        scale(bound, RandomState::new().build_hasher().finish())
    }
}

/// Fraction `random / 2^64` de `bound`, strictement inférieure à `bound`.
///
/// Calcul entier sur 128 bits, arrondi vers le bas : un flottant pourrait arrondir à `bound`.
/// La borne est limitée à un `u64` de nanosecondes (environ 584 ans).
fn scale(bound: Duration, random: u64) -> Duration {
    let nanos = u64::try_from(bound.as_nanos()).unwrap_or(u64::MAX);
    let scaled = (u128::from(nanos) * u128::from(random)) >> 64;
    Duration::from_nanos(u64::try_from(scaled).unwrap_or(u64::MAX))
}

/// Attente avant la reprise numéro `retry` (à partir de 1) : base doublée plus gigue.
///
/// 500 ms puis 1 s, plus une gigue dans `[0, base)` : elle s'ajoute, ne retranche jamais.
fn backoff(retry: u32, jitter: &impl Jitter) -> Duration {
    let base = BASE_WAIT.saturating_mul(2_u32.saturating_pow(retry.saturating_sub(1)));
    base.saturating_add(jitter.below(base))
}

/// Attente avant la reprise numéro `retry`, ou `None` s'il faut renoncer.
///
/// L'attente est le plus grand du délai exponentiel et de `Retry-After`. On renonce quand
/// `Retry-After` dépasse [`MAX_RETRY_AFTER`] ou quand l'attente ne tient pas dans le délai
/// global (l'égalité tient).
fn wait_before_retry<K: MonoClock, S, J: Jitter>(
    ctx: &RetryContext<'_, K, S, J>,
    retry: u32,
    retry_after: Option<Duration>,
) -> Option<Duration> {
    let announced = retry_after.unwrap_or_default();
    let wait = backoff(retry, ctx.jitter).max(announced);
    if announced > MAX_RETRY_AFTER || !ctx.deadline.fits(ctx.clock, wait) {
        None
    } else {
        Some(wait)
    }
}

/// Vrai quand plus aucune requête n'est permise : trois faites, ou un deuxième 500 vu.
fn out_of_attempts(attempts_made: u32, reduced_seen: u32) -> bool {
    attempts_made >= MAX_ATTEMPTS || reduced_seen >= MAX_REDUCED
}

/// Ce que `run` emprunte à son appelant.
#[derive(Debug)]
pub(crate) struct RetryContext<'a, K, S, J> {
    /// Horloge monotone du crate, la même que celle de `deadline`.
    pub clock: &'a K,
    /// Seule porte d'entrée des attentes.
    pub sleeper: &'a S,
    /// Échéance globale de l'appel.
    pub deadline: &'a CallDeadline,
    /// Source de la gigue.
    pub jitter: &'a J,
}

/// Rejoue `attempt` selon la politique : trois requêtes au plus, deux si un 500 revient.
///
/// `Success` rend la valeur ; `Unauthorized` et `Fatal` rendent leur erreur sans réessai (le
/// renouvellement du jeton sur 401 se greffe dans la fermeture de l'appelant, donc un
/// `Unauthorized` qui arrive ici est définitif). Un `Transient` dort `wait_before_retry` puis
/// recommence, sauf si le budget est épuisé ou si l'attente est refusée.
///
/// # Errors
///
/// L'erreur de la dernière tentative ; `Timeout` si aucune n'a eu lieu (jamais en pratique : la
/// boucle fait au moins une tentative).
pub(crate) async fn run<T, K, S, J, F, Fut>(
    ctx: &RetryContext<'_, K, S, J>,
    mut attempt: F,
) -> Result<T, CorpusError>
where
    K: MonoClock,
    S: Sleeper,
    J: Jitter,
    F: FnMut() -> Fut,
    Fut: Future<Output = Outcome<T>>,
{
    let mut last = CorpusError::Timeout;
    let mut reduced_seen = 0;
    // Borne dure : la boucle ne peut pas dépasser `MAX_ATTEMPTS` tours, même mutée.
    for number in 1..=MAX_ATTEMPTS {
        match attempt().await {
            Outcome::Success(value) => return Ok(value),
            Outcome::Unauthorized(error) | Outcome::Fatal(error) => return Err(error),
            Outcome::Transient { error, retry_after, budget } => {
                last = error;
                if budget == Budget::Reduced {
                    reduced_seen += 1;
                }
                if out_of_attempts(number, reduced_seen) {
                    break;
                }
                let Some(wait) = wait_before_retry(ctx, number, retry_after) else {
                    break;
                };
                ctx.sleeper.sleep(wait).await;
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::ready;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    use lf_domain::coverage::PisteEnvironment;
    use reqwest::{Client, Url};
    use wiremock::matchers::{any, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::classify::{EndpointKind, ErrorContext, try_send};
    use crate::config::{PisteConfig, PisteEndpoints};
    use crate::http::{Attempt, Payload, RawResponse, build_client};
    use crate::testing::ClosedPort;
    use crate::time::{ManualClock, RecordingSleeper};

    const DATE: &str = "Sun, 06 Nov 1994 08:49:37 GMT";
    const ENDPOINT: &str = "/consult/juri";
    const CONSULT: ErrorContext<'static> = ErrorContext {
        kind: EndpointKind::Consult,
        official_host: false,
        repair_mojibake: true,
        secrets: &[],
    };

    const OFFICIAL_CONSULT: ErrorContext<'static> = ErrorContext { official_host: true, ..CONSULT };

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    const fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    // ---- Analyse de Retry-After ------------------------------------------------------

    #[test]
    fn retry_after_in_whole_seconds() {
        assert_eq!(parse_retry_after(Some("2"), None), Some(secs(2)));
        assert_eq!(parse_retry_after(Some("0"), None), Some(Duration::ZERO));
        assert_eq!(parse_retry_after(Some("120"), Some(DATE)), Some(secs(120)));
    }

    #[test]
    fn retry_after_ignores_surrounding_spaces() {
        assert_eq!(parse_retry_after(Some("  7 \t"), None), Some(secs(7)));
    }

    #[test]
    fn absent_empty_or_garbled_retry_after_is_none() {
        for text in ["", " ", "-5", "+5", "1.5", "1e3", "abc", "5 s", "5,5", "0x10", "٣"] {
            assert_eq!(parse_retry_after(Some(text), Some(DATE)), None, "text {text:?}");
            assert_eq!(parse_retry_after(Some(text), None), None, "text {text:?}");
        }
        assert_eq!(parse_retry_after(None, Some(DATE)), None);
        assert_eq!(parse_retry_after(None, None), None);
    }

    #[test]
    fn giant_retry_after_saturates_instead_of_being_dropped() {
        for text in ["18446744073709551615", "18446744073709551616", "99999999999999999999999999"] {
            assert_eq!(
                parse_retry_after(Some(text), None),
                Some(Duration::from_secs(u64::MAX)),
                "text {text:?}"
            );
        }
    }

    #[test]
    fn http_date_is_the_difference_with_the_date_of_the_same_response() {
        assert_eq!(
            parse_retry_after(Some("Sun, 06 Nov 1994 08:49:40 GMT"), Some(DATE)),
            Some(secs(3))
        );
        assert_eq!(
            parse_retry_after(Some("Sun, 06 Nov 1994 09:49:37 GMT"), Some(DATE)),
            Some(secs(3600))
        );
    }

    #[test]
    fn http_date_equal_to_the_date_header_is_zero() {
        assert_eq!(parse_retry_after(Some(DATE), Some(DATE)), Some(Duration::ZERO));
    }

    #[test]
    fn http_date_without_a_date_header_is_ignored() {
        assert_eq!(parse_retry_after(Some("Sun, 06 Nov 1994 08:49:40 GMT"), None), None);
    }

    #[test]
    fn http_date_in_the_past_is_ignored() {
        assert_eq!(parse_retry_after(Some("Sun, 06 Nov 1994 08:49:34 GMT"), Some(DATE)), None);
    }

    #[test]
    fn malformed_http_date_or_date_header_is_ignored() {
        let cases = [
            ("Sun, 99 Nov 1994 08:49:40 GMT", DATE),
            ("Sun, 06 Nov 1994 08:49:40 GMT", "yesterday"),
            ("Sun, 06 Nov 1994 08:49:40 GMT", ""),
            ("Mon, 06 Nov 1994 08:49:40 GMT", DATE),
            ("Sun, 06 Nov 1994 08:49:40 GMT", "Mon, 06 Nov 1994 08:49:37 GMT"),
        ];
        for (retry_after, date) in cases {
            assert_eq!(parse_retry_after(Some(retry_after), Some(date)), None, "{retry_after:?}");
        }
    }

    #[test]
    fn far_future_http_date_is_a_large_wait() {
        let delay = parse_retry_after(Some("Fri, 31 Dec 9999 23:59:59 GMT"), Some(DATE));
        assert!(delay > Some(secs(30)), "{delay:?}");
    }

    // ---- Gigue et délai --------------------------------------------------------------

    /// Gigue déterministe : la fraction `self.0 / 2^64` de la borne.
    struct FractionJitter(u64);

    impl Jitter for FractionJitter {
        fn below(&self, bound: Duration) -> Duration {
            scale(bound, self.0)
        }
    }

    #[test]
    fn scale_takes_a_fraction_of_the_bound() {
        assert_eq!(scale(secs(1), 0), Duration::ZERO);
        assert_eq!(scale(secs(1), 1 << 63), ms(500));
        assert_eq!(scale(secs(1), 1 << 62), ms(250));
        assert_eq!(scale(secs(4), 1 << 63), secs(2));
    }

    #[test]
    fn scale_is_strictly_below_the_bound() {
        assert_eq!(scale(secs(1), u64::MAX), Duration::from_nanos(999_999_999));
        assert_eq!(scale(Duration::from_nanos(1), u64::MAX), Duration::ZERO);
        assert_eq!(scale(Duration::ZERO, u64::MAX), Duration::ZERO);
    }

    #[test]
    fn system_jitter_stays_below_the_bound_and_varies() {
        let draws: Vec<Duration> = (0..64).map(|_| SystemJitter.below(secs(1))).collect();
        assert_eq!(draws.iter().filter(|draw| **draw >= secs(1)).count(), 0);
        assert!(draws.iter().any(|draw| *draw != draws[0]), "{draws:?}");
        assert_eq!(SystemJitter.below(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn backoff_without_jitter_is_500_ms_then_one_second() {
        assert_eq!(backoff(1, &FractionJitter(0)), ms(500));
        assert_eq!(backoff(2, &FractionJitter(0)), secs(1));
    }

    #[test]
    fn backoff_adds_a_jitter_below_the_base_and_never_subtracts() {
        assert_eq!(backoff(1, &FractionJitter(u64::MAX)), Duration::from_nanos(999_999_999));
        assert_eq!(backoff(2, &FractionJitter(u64::MAX)), Duration::from_nanos(1_999_999_999));
        assert_eq!(backoff(1, &FractionJitter(1 << 63)), ms(750));
        assert_eq!(backoff(2, &FractionJitter(1 << 63)), ms(1500));
    }

    // ---- Boucle ----------------------------------------------------------------------

    struct Run {
        result: Result<u32, CorpusError>,
        calls: usize,
        waits: Vec<Duration>,
    }

    fn assert_send<T: Send>(value: T) -> T {
        value
    }

    /// Joue `script` dans `run` : seul point d'appel de `run` des tests de la boucle (une
    /// instance générique par fermeture, et `llvm-cov` résume par l'instance la plus couverte).
    async fn drive(script: Vec<Outcome<u32>>, remaining: Duration, random: u64) -> Run {
        let clock = ManualClock::new();
        let sleeper = RecordingSleeper::new(clock.clone());
        let deadline = CallDeadline::start(&clock, remaining);
        let jitter = FractionJitter(random);
        let ctx =
            RetryContext { clock: &clock, sleeper: &sleeper, deadline: &deadline, jitter: &jitter };
        let mut script = VecDeque::from(script);
        let mut calls = 0;
        let result = assert_send(run(&ctx, || {
            calls += 1;
            ready(script.pop_front().expect("script long enough"))
        }))
        .await;
        Run { result, calls, waits: sleeper.waits() }
    }

    async fn drive_plain(script: Vec<Outcome<u32>>) -> Run {
        drive(script, secs(60), 0).await
    }

    fn upstream(status: u16, retryable: bool) -> CorpusError {
        CorpusError::Upstream {
            status: Some(status),
            retryable,
            detail: format!("HTTP {status} on {ENDPOINT}"),
        }
    }

    fn transient(error: CorpusError, retry_after: Option<Duration>) -> Outcome<u32> {
        Outcome::Transient { error, retry_after, budget: Budget::Standard }
    }

    fn t503() -> Outcome<u32> {
        transient(upstream(503, true), None)
    }

    fn t500() -> Outcome<u32> {
        Outcome::Transient {
            error: upstream(500, false),
            retry_after: None,
            budget: Budget::Reduced,
        }
    }

    fn rate_limited(retry_after: Duration) -> Outcome<u32> {
        transient(CorpusError::RateLimited { retry_after: Some(retry_after) }, Some(retry_after))
    }

    #[tokio::test]
    async fn first_success_does_not_wait() {
        let run = drive_plain(vec![Outcome::Success(7)]).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 1, vec![]));
    }

    #[tokio::test]
    async fn transient_then_success_waits_the_first_backoff() {
        let run = drive_plain(vec![t503(), Outcome::Success(7)]).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 2, vec![ms(500)]));
    }

    #[tokio::test]
    async fn two_transients_then_success_waits_both_backoffs() {
        let run = drive_plain(vec![t503(), t503(), Outcome::Success(7)]).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 3, vec![ms(500), secs(1)]));
    }

    #[tokio::test]
    async fn three_transients_stop_at_three_attempts_with_the_last_error() {
        let run = drive_plain(vec![t503(), t503(), t503()]).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(upstream(503, true)), 3, vec![ms(500), secs(1)])
        );
    }

    #[tokio::test]
    async fn jitter_is_added_to_each_wait() {
        let run = drive(vec![t503(), t503(), t503()], secs(60), u64::MAX).await;
        assert_eq!(
            run.waits,
            vec![Duration::from_nanos(999_999_999), Duration::from_nanos(1_999_999_999)]
        );
    }

    #[tokio::test]
    async fn a_second_500_stops_the_loop_after_two_requests() {
        let run = drive_plain(vec![t500(), t500()]).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(upstream(500, false)), 2, vec![ms(500)])
        );
    }

    #[tokio::test]
    async fn a_first_500_is_retried_once_and_may_succeed() {
        let run = drive_plain(vec![t500(), Outcome::Success(7)]).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 2, vec![ms(500)]));
    }

    #[tokio::test]
    async fn the_loop_stops_at_the_second_500_even_after_another_transient() {
        let run = drive_plain(vec![t503(), t500(), t500()]).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(upstream(500, false)), 3, vec![ms(500), secs(1)])
        );
    }

    #[tokio::test]
    async fn a_single_500_does_not_shorten_the_budget_of_other_transients() {
        let run = drive_plain(vec![t500(), t503(), t503()]).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(upstream(503, true)), 3, vec![ms(500), secs(1)])
        );
    }

    #[tokio::test]
    async fn fatal_is_returned_without_retry() {
        let run = drive_plain(vec![Outcome::Fatal(upstream(404, false))]).await;
        assert_eq!((run.result, run.calls, run.waits), (Err(upstream(404, false)), 1, vec![]));
    }

    #[tokio::test]
    async fn an_unauthorized_reaching_the_loop_is_final() {
        let auth = CorpusError::Auth { detail: "HTTP 401 on /consult/juri".to_owned() };
        let run = drive_plain(vec![Outcome::Unauthorized(auth.clone())]).await;
        assert_eq!((run.result, run.calls, run.waits), (Err(auth), 1, vec![]));
    }

    #[tokio::test]
    async fn the_error_of_the_last_attempt_is_the_one_returned() {
        let run = drive_plain(vec![
            transient(CorpusError::RateLimited { retry_after: None }, None),
            t503(),
            transient(CorpusError::Timeout, None),
        ])
        .await;
        assert_eq!((run.result, run.calls), (Err(CorpusError::Timeout), 3));
    }

    // ---- Retry-After et délai global -------------------------------------------------

    #[tokio::test]
    async fn retry_after_longer_than_the_backoff_is_what_is_waited() {
        let run = drive_plain(vec![rate_limited(secs(2)), Outcome::Success(7)]).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 2, vec![secs(2)]));
    }

    #[tokio::test]
    async fn backoff_longer_than_the_retry_after_is_what_is_waited() {
        let run = drive_plain(vec![rate_limited(ms(100)), Outcome::Success(7)]).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 2, vec![ms(500)]));
    }

    #[tokio::test]
    async fn each_retry_compares_its_own_retry_after_with_its_own_backoff() {
        let run =
            drive_plain(vec![rate_limited(secs(3)), rate_limited(ms(100)), Outcome::Success(7)])
                .await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 3, vec![secs(3), secs(1)]));
    }

    #[tokio::test]
    async fn retry_after_of_exactly_thirty_seconds_is_waited() {
        let run = drive_plain(vec![rate_limited(secs(30)), Outcome::Success(7)]).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 2, vec![secs(30)]));
    }

    #[tokio::test]
    async fn retry_after_beyond_thirty_seconds_fails_fast_even_with_time_to_spare() {
        let beyond = ms(30_001);
        let run = drive(vec![rate_limited(beyond), Outcome::Success(7)], secs(3600), 0).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(CorpusError::RateLimited { retry_after: Some(beyond) }), 1, vec![])
        );
    }

    #[tokio::test]
    async fn a_giant_retry_after_fails_fast_with_its_value() {
        let giant = Duration::from_secs(u64::MAX);
        let run = drive_plain(vec![rate_limited(giant), Outcome::Success(7)]).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(CorpusError::RateLimited { retry_after: Some(giant) }), 1, vec![])
        );
    }

    #[tokio::test]
    async fn a_wait_that_exactly_fits_is_slept() {
        let run = drive(vec![t503(), Outcome::Success(7)], ms(500), 0).await;
        assert_eq!((run.result, run.calls, run.waits), (Ok(7), 2, vec![ms(500)]));
    }

    #[tokio::test]
    async fn a_wait_one_nanosecond_too_long_fails_fast_with_the_last_error() {
        let run =
            drive(vec![t503(), Outcome::Success(7)], Duration::from_nanos(499_999_999), 0).await;
        assert_eq!((run.result, run.calls, run.waits), (Err(upstream(503, true)), 1, vec![]));
    }

    #[tokio::test]
    async fn the_deadline_stops_the_second_retry_but_not_the_first() {
        let run = drive(vec![t503(), t503(), t503()], ms(1400), 0).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(upstream(503, true)), 2, vec![ms(500)])
        );
    }

    #[tokio::test]
    async fn a_rate_limited_error_refused_by_the_deadline_keeps_its_retry_after() {
        let run = drive(vec![rate_limited(secs(2)), Outcome::Success(7)], secs(1), 0).await;
        assert_eq!(
            (run.result, run.calls, run.waits),
            (Err(CorpusError::RateLimited { retry_after: Some(secs(2)) }), 1, vec![])
        );
    }

    // ---- Bout en bout contre wiremock -------------------------------------------------

    fn user_agent() -> String {
        format!(
            "mcp-legifrance/{} (+https://github.com/muchiny/mcp-legifrance)",
            env!("CARGO_PKG_VERSION")
        )
    }

    /// Configuration dont les deux URL pointent vers `uri` (boucle locale, en `http`).
    fn config_for(uri: &str) -> (PisteConfig, PisteEndpoints) {
        let (endpoints, _) = PisteEndpoints::with_overrides(
            PisteEnvironment::Sandbox,
            Some(&format!("{uri}/oauth/token")),
            Some(&format!("{uri}/dila/legifrance/lf-engine-app")),
        )
        .unwrap();
        let config =
            PisteConfig::new(PisteEnvironment::Sandbox, Ok(endpoints.clone()), None, user_agent());
        (config, endpoints)
    }

    struct Rig {
        client: Client,
        url: Url,
        clock: ManualClock,
        sleeper: RecordingSleeper,
        deadline: CallDeadline,
        request_timeout: Duration,
    }

    impl Rig {
        fn new(uri: &str) -> Self {
            let (config, endpoints) = config_for(uri);
            let clock = ManualClock::new();
            Self {
                client: build_client(&config, &endpoints).unwrap(),
                url: Url::parse(&format!("{uri}{ENDPOINT}")).unwrap(),
                sleeper: RecordingSleeper::new(clock.clone()),
                deadline: CallDeadline::start(&clock, secs(60)),
                clock,
                request_timeout: secs(5),
            }
        }

        fn with_remaining(mut self, remaining: Duration) -> Self {
            self.deadline = CallDeadline::start(&self.clock, remaining);
            self
        }

        fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
            self.request_timeout = request_timeout;
            self
        }
    }

    /// Une consultation sur un hôte non officiel.
    async fn call(rig: &Rig) -> Result<RawResponse, CorpusError> {
        call_with(rig, &CONSULT).await
    }

    /// Seul point d'appel de `run` des tests de bout en bout.
    async fn call_with(rig: &Rig, context: &ErrorContext<'_>) -> Result<RawResponse, CorpusError> {
        let ctx = RetryContext {
            clock: &rig.clock,
            sleeper: &rig.sleeper,
            deadline: &rig.deadline,
            jitter: &SystemJitter,
        };
        let attempt = Attempt {
            url: &rig.url,
            endpoint: ENDPOINT,
            payload: Payload::Json(b"{}"),
            bearer: None,
            timeout: rig.deadline.cap(&rig.clock, rig.request_timeout),
            max_body_bytes: 16 * 1024 * 1024,
        };
        run(&ctx, || async { try_send(&rig.client, &attempt, context).await }).await
    }

    /// `times` réponses `template` à `POST /consult/juri`, exactement.
    async fn reply(server: &MockServer, template: ResponseTemplate, times: u64) {
        Mock::given(method("POST"))
            .and(path(ENDPOINT))
            .respond_with(template)
            .up_to_n_times(times)
            .expect(times)
            .mount(server)
            .await;
    }

    /// Une réponse qui ne doit jamais être servie.
    async fn never(server: &MockServer, template: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(ENDPOINT))
            .respond_with(template)
            .expect(0)
            .mount(server)
            .await;
    }

    async fn requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    /// Réponse de `code` au corps `{}` : un 2xx sans corps est « introuvable » sur une
    /// consultation, ce que ces tests de réessai ne visent pas.
    fn status(code: u16) -> ResponseTemplate {
        ResponseTemplate::new(code).set_body_string("{}")
    }

    fn upstream_error(code: u16, retryable: bool) -> CorpusError {
        CorpusError::Upstream {
            status: Some(code),
            retryable,
            detail: format!("HTTP {code} on {ENDPOINT}"),
        }
    }

    fn assert_within(wait: Duration, low: Duration, high: Duration) {
        assert!(low <= wait, "{wait:?} below {low:?}");
        assert!(wait < high, "{wait:?} not below {high:?}");
    }

    #[tokio::test]
    async fn success_is_returned_with_the_raw_response() {
        let server = MockServer::start().await;
        reply(&server, status(200).set_body_string(r#"{"ok":true}"#), 1).await;
        let rig = Rig::new(&server.uri());
        let response = call(&rig).await.unwrap();
        assert_eq!((response.status, response.body), (200, br#"{"ok":true}"#.to_vec()));
        assert_eq!(rig.sleeper.waits(), vec![]);
    }

    #[tokio::test]
    async fn the_429_retry_after_is_honoured() {
        let server = MockServer::start().await;
        reply(&server, status(429).insert_header("Retry-After", "2"), 1).await;
        reply(&server, status(200), 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await.unwrap().status, 200);
        assert_eq!(rig.sleeper.waits(), vec![secs(2)]);
    }

    #[tokio::test]
    async fn the_429_retry_after_http_date_is_honoured() {
        let server = MockServer::start().await;
        let limited = status(429)
            .insert_header("Date", DATE)
            .insert_header("Retry-After", "Sun, 06 Nov 1994 08:49:40 GMT");
        reply(&server, limited, 1).await;
        reply(&server, status(200), 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await.unwrap().status, 200);
        assert_eq!(rig.sleeper.waits(), vec![secs(3)]);
    }

    #[tokio::test]
    async fn the_429_retry_after_beyond_the_deadline_fails_fast() {
        let server = MockServer::start().await;
        reply(&server, status(429).insert_header("Retry-After", "120"), 1).await;
        never(&server, status(200)).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call(&rig).await,
            Err(CorpusError::RateLimited { retry_after: Some(secs(120)) })
        );
        assert_eq!(rig.sleeper.waits(), vec![]);
        assert_eq!(rig.clock.now(), Duration::ZERO);
    }

    #[tokio::test]
    async fn the_429_three_times_maps_to_rate_limited() {
        let server = MockServer::start().await;
        reply(&server, status(429), 3).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await, Err(CorpusError::RateLimited { retry_after: None }));
        assert_eq!(requests(&server).await, 3);
        assert_eq!(rig.sleeper.waits().len(), 2);
    }

    /// Serveur TCP brut : une réponse par connexion, dans l'ordre. Wiremock ajoute lui-même
    /// un `Date`, ce qui interdit de tester son absence avec lui.
    fn raw_sequence_server(responses: Vec<&'static str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                for line in reader.by_ref().lines() {
                    if line.unwrap().is_empty() {
                        break;
                    }
                }
                reader.read_exact(&mut [0_u8; 2]).unwrap();
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        uri
    }

    #[tokio::test]
    async fn the_429_retry_after_http_date_without_date_header_uses_the_backoff() {
        let uri = raw_sequence_server(vec![
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: Sun, 06 Nov 1994 08:49:40 GMT\r\n\
             Content-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        ]);
        let rig = Rig::new(&uri);
        assert_eq!(call(&rig).await.unwrap().status, 200);
        let waits = rig.sleeper.waits();
        assert_eq!(waits.len(), 1);
        assert_within(waits[0], ms(500), secs(1));
    }

    #[tokio::test]
    async fn the_500_then_200_succeeds_after_backoff() {
        let server = MockServer::start().await;
        reply(&server, status(500), 1).await;
        reply(&server, status(200), 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await.unwrap().status, 200);
        let waits = rig.sleeper.waits();
        assert_eq!(waits.len(), 1);
        assert_within(waits[0], ms(500), secs(1));
    }

    #[tokio::test]
    async fn the_500_twice_maps_to_upstream_not_retryable() {
        let server = MockServer::start().await;
        reply(&server, status(500), 2).await;
        never(&server, status(200)).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await, Err(upstream_error(500, false)));
        assert_eq!(requests(&server).await, 2);
    }

    #[tokio::test]
    async fn the_503_then_500_then_500_stops_at_the_second_500() {
        let server = MockServer::start().await;
        reply(&server, status(503), 1).await;
        reply(&server, status(500), 2).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await, Err(upstream_error(500, false)));
        assert_eq!(requests(&server).await, 3);
    }

    #[tokio::test]
    async fn the_503_three_times_maps_to_upstream_retryable() {
        let server = MockServer::start().await;
        reply(&server, status(503), 3).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await, Err(upstream_error(503, true)));
        let waits = rig.sleeper.waits();
        assert_eq!(waits.len(), 2);
        assert_within(waits[0], ms(500), secs(1));
        assert_within(waits[1], secs(1), secs(2));
    }

    #[tokio::test]
    async fn the_400_is_not_retried() {
        let server = MockServer::start().await;
        reply(&server, status(400), 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call(&rig).await,
            Err(CorpusError::InvalidRequest {
                status: Some(400),
                detail: "HTTP 400 on /consult/juri".to_owned(),
            })
        );
        assert_eq!(rig.sleeper.waits(), vec![]);
    }

    #[tokio::test]
    async fn the_403_maps_to_auth() {
        let server = MockServer::start().await;
        reply(&server, status(403), 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call(&rig).await,
            Err(CorpusError::Auth { detail: "HTTP 403 on /consult/juri".to_owned() })
        );
    }

    #[tokio::test]
    async fn the_401_is_final_for_the_loop() {
        let server = MockServer::start().await;
        reply(&server, status(401), 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call(&rig).await,
            Err(CorpusError::Auth { detail: "HTTP 401 on /consult/juri".to_owned() })
        );
        assert_eq!(requests(&server).await, 1);
    }

    #[tokio::test]
    async fn the_404_is_config_not_not_found() {
        let server = MockServer::start().await;
        reply(&server, status(404), 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call(&rig).await,
            Err(CorpusError::Config { detail: "HTTP 404 on /consult/juri".to_owned() })
        );
    }

    #[tokio::test]
    async fn the_404_on_consult_official_host_is_not_found() {
        let server = MockServer::start().await;
        reply(&server, status(404).set_body_string(r#"{"status":404,"error":"Not Found"}"#), 1)
            .await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call_with(&rig, &OFFICIAL_CONSULT).await,
            Err(CorpusError::NotFound { what: "/consult/juri".to_owned() })
        );
        assert_eq!((requests(&server).await, rig.sleeper.waits()), (1, vec![]));
    }

    #[tokio::test]
    async fn the_404_html_is_config() {
        let server = MockServer::start().await;
        let page = ResponseTemplate::new(404).set_body_raw("<html>404</html>", "text/html");
        reply(&server, page, 1).await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call_with(&rig, &OFFICIAL_CONSULT).await,
            Err(CorpusError::Config { detail: "HTTP 404 on /consult/juri".to_owned() })
        );
    }

    #[tokio::test]
    async fn the_404_on_search_is_config_not_not_found() {
        let server = MockServer::start().await;
        reply(&server, status(404), 1).await;
        let rig = Rig::new(&server.uri());
        let search = ErrorContext { kind: EndpointKind::Search, ..OFFICIAL_CONSULT };
        assert_eq!(
            call_with(&rig, &search).await,
            Err(CorpusError::Config { detail: "HTTP 404 on /consult/juri".to_owned() })
        );
    }

    #[tokio::test]
    async fn the_400_with_a_message_is_not_retried_and_carries_it() {
        let server = MockServer::start().await;
        reply(&server, status(400).set_body_string(r#"{"error":400,"message":"Bad\nfacet"}"#), 1)
            .await;
        let rig = Rig::new(&server.uri());
        assert_eq!(
            call(&rig).await,
            Err(CorpusError::InvalidRequest { status: Some(400), detail: "Bad facet".to_owned() })
        );
        assert_eq!((requests(&server).await, rig.sleeper.waits()), (1, vec![]));
    }

    #[tokio::test]
    async fn redirect_is_not_followed_end_to_end() {
        let server = MockServer::start().await;
        reply(&server, status(302).insert_header("Location", "/elsewhere"), 1).await;
        Mock::given(any())
            .and(path("/elsewhere"))
            .respond_with(status(200))
            .expect(0)
            .mount(&server)
            .await;
        let rig = Rig::new(&server.uri());
        assert_eq!(call(&rig).await, Err(upstream_error(302, false)));
    }

    #[tokio::test]
    async fn network_error_is_retried() {
        let closed = ClosedPort::new();
        let rig = Rig::new(&closed.uri());
        assert_eq!(
            call(&rig).await,
            Err(CorpusError::Upstream {
                status: None,
                retryable: true,
                detail: "network error (connect) on /consult/juri".to_owned(),
            })
        );
        assert_eq!(rig.sleeper.waits().len(), 2);
    }

    #[tokio::test]
    async fn request_timeout_maps_to_timeout() {
        let server = MockServer::start().await;
        reply(&server, status(200).set_delay(secs(1)), 3).await;
        let rig = Rig::new(&server.uri()).with_request_timeout(ms(100));
        let started = Instant::now();
        assert_eq!(call(&rig).await, Err(CorpusError::Timeout));
        let elapsed = started.elapsed();
        assert!(elapsed < ms(2500), "{elapsed:?}");
        assert_eq!(requests(&server).await, 3);
        assert_eq!(rig.sleeper.waits().len(), 2);
    }

    #[tokio::test]
    async fn tool_deadline_caps_retries() {
        let server = MockServer::start().await;
        reply(&server, status(503), 2).await;
        let rig = Rig::new(&server.uri()).with_remaining(secs(1));
        assert_eq!(call(&rig).await, Err(upstream_error(503, true)));
        assert_eq!(requests(&server).await, 2);
        assert_eq!(rig.sleeper.waits().len(), 1);
        let now = rig.clock.now();
        assert!(now < secs(1), "{now:?}");
    }
}
