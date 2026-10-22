//! Limitation de la charge sortante vers PISTE : concurrence puis débit.
//!
//! [`RateGate`] impose deux bornes avant chaque tentative d'envoi. D'abord un sémaphore plafonne
//! les requêtes simultanées ; le permis est rendu à la chute du [`Permit`], que l'appelant garde
//! jusqu'après la lecture du corps. Ensuite un limiteur `governor` (GCRA direct) espace les
//! débuts de tentative selon le quota.
//!
//! Toutes les attentes sont bornées par le reste du délai de l'appel et passent par le
//! [`Sleeper`] injecté : **jamais** `RateLimiter::until_ready`, dont l'attente passe par
//! `futures_timer`, hors de l'horloge de test et hors du délai global. Le limiteur ne lit que
//! la [`MonoClock`] de l'adapter, par [`MonoLimiterClock`] : même origine que les
//! [`CallDeadline`].

use std::future::Future;
use std::num::NonZeroU16;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::time::Duration;

use governor::clock::Clock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use lf_application::ports::CorpusError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::config::RateQuota;
use crate::deadline::CallDeadline;
use crate::time::{MonoClock, Sleeper, race};

/// Garde-fou de progression de [`RateGate::take_credit`].
///
/// Sans contention, une attente suffit ; avec au plus 16 permis (`LF_PISTE_CONCURRENCY` ≤ 16),
/// un appel peut perdre plusieurs tours, pas des dizaines. Atteint, il rend `RateLimited` au
/// lieu de boucler (jamais en production). Aucun mutant `TIMEOUT` n'est possible.
const MAX_CHECKS: u32 = 64;

/// Horloge du limiteur : la [`MonoClock`] de l'adapter, en production comme en test.
///
/// L'instant de `governor` est la durée écoulée depuis l'origine de cette horloge.
#[derive(Debug, Clone)]
pub(crate) struct MonoLimiterClock<K>(K);

impl<K: MonoClock> Clock for MonoLimiterClock<K> {
    type Instant = Duration;

    fn now(&self) -> Duration {
        self.0.now()
    }
}

/// Limiteur direct (sans clé), à état en mémoire, sur l'horloge de l'adapter.
type DirectLimiter<K> =
    RateLimiter<NotKeyed, InMemoryState, MonoLimiterClock<K>, NoOpMiddleware<Duration>>;

/// Quota `governor` : `per_second` requêtes par seconde (regarnissage toutes les
/// `1 / per_second` s), rafale `burst`.
fn quota_of(quota: RateQuota) -> Quota {
    Quota::per_second(quota.per_second()).allow_burst(quota.burst())
}

/// Prend un permis du sémaphore, ou renonce quand `timeout` finit le premier.
///
/// Non générique : une seule instance de code. Un sémaphore fermé, jamais exposé, se confond
/// avec un délai épuisé, ce qui évite une branche morte.
async fn race_permit(
    semaphore: Arc<Semaphore>,
    timeout: Pin<&mut (dyn Future<Output = ()> + Send)>,
) -> Result<OwnedSemaphorePermit, CorpusError> {
    race(semaphore.acquire_owned(), timeout).await.and_then(Result::ok).ok_or(CorpusError::Timeout)
}

/// Permis de concurrence : la place est rendue à la chute. Aucun accesseur.
#[derive(Debug)]
pub(crate) struct Permit {
    _inner: OwnedSemaphorePermit,
}

/// Barrière de charge sortante : au plus N requêtes simultanées, au plus le quota de débit.
///
/// `K: Clone` : la barrière garde un exemplaire de l'horloge (reste du délai, instant du
/// limiteur) et en donne un autre au limiteur. `TokioClock` est `Copy` ; les clones de
/// `ManualClock` partagent leur compteur, donc la [`CallDeadline`] de l'appelant vaut pour les
/// deux. Passer un clone de l'horloge qui a créé les [`CallDeadline`].
pub(crate) struct RateGate<K: MonoClock + Clone, S: Sleeper> {
    clock: K,
    sleeper: S,
    semaphore: Arc<Semaphore>,
    limiter: DirectLimiter<K>,
}

impl<K: MonoClock + Clone, S: Sleeper> RateGate<K, S> {
    /// Assemble la barrière ; ne décide de rien.
    pub(crate) fn new(clock: K, sleeper: S, quota: RateQuota, concurrency: NonZeroU16) -> Self {
        let limiter =
            RateLimiter::direct_with_clock(quota_of(quota), MonoLimiterClock(clock.clone()));
        Self {
            clock,
            sleeper,
            semaphore: Arc::new(Semaphore::new(usize::from(concurrency.get()))),
            limiter,
        }
    }

    /// Obtient la permission de lancer une tentative : un permis de concurrence, puis un
    /// crédit de débit. Le permis est gardé par l'appelant jusqu'après la lecture du corps.
    ///
    /// Le limiteur n'est consulté qu'après le permis, avant chaque tentative (réessais
    /// compris), et jamais pour la requête de jeton (affaire de l'appelant).
    ///
    /// # Errors
    ///
    /// - [`CorpusError::Timeout`] si le délai est déjà écoulé, ou si aucun permis ne se libère
    ///   avant l'échéance.
    /// - [`CorpusError::RateLimited`] avec `retry_after: Some(attente)` si le prochain crédit
    ///   tombe après l'échéance (l'attente qui dépasserait l'échéance n'est pas dormie ; des
    ///   attentes plus courtes, prises avant, ont pu l'être), ou si `MAX_CHECKS` est atteint.
    pub(crate) async fn acquire(&self, deadline: &CallDeadline) -> Result<Permit, CorpusError> {
        if deadline.is_expired(&self.clock) {
            return Err(CorpusError::Timeout);
        }
        // variable locale : un échec du limiteur rend aussitôt le permis
        let inner = self.take_slot(deadline).await?;
        self.take_credit(deadline).await?;
        Ok(Permit { _inner: inner })
    }

    /// Attend un permis du sémaphore, au plus le reste du délai.
    async fn take_slot(
        &self,
        deadline: &CallDeadline,
    ) -> Result<OwnedSemaphorePermit, CorpusError> {
        let timeout = pin!(self.sleeper.sleep(deadline.remaining(&self.clock)));
        race_permit(Arc::clone(&self.semaphore), timeout).await
    }

    /// Attend un crédit du limiteur : dort `wait` tant qu'il tient dans le reste du délai.
    ///
    /// Défaut connu et accepté : le dernier tour dort puis sort sans revérifier.
    async fn take_credit(&self, deadline: &CallDeadline) -> Result<(), CorpusError> {
        let mut last_wait = Duration::ZERO;
        for _ in 0..MAX_CHECKS {
            let Err(not_until) = self.limiter.check() else {
                return Ok(());
            };
            last_wait = not_until.wait_time_from(self.clock.now());
            if !deadline.fits(&self.clock, last_wait) {
                return Err(CorpusError::RateLimited { retry_after: Some(last_wait) });
            }
            self.sleeper.sleep(last_wait).await;
        }
        Err(CorpusError::RateLimited { retry_after: Some(last_wait) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{ManualClock, RecordingSleeper, TokioClock, TokioSleeper};
    use std::num::NonZeroU32;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn quota(per_second: u32, burst: u32) -> RateQuota {
        RateQuota::new(NonZeroU32::new(per_second).unwrap(), NonZeroU32::new(burst).unwrap())
    }

    fn permits(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    /// Barrière sur une horloge manuelle et une attente enregistrée liée à cette horloge.
    fn gate(
        per_second: u32,
        burst: u32,
        concurrency: u16,
    ) -> (ManualClock, RecordingSleeper, RateGate<ManualClock, RecordingSleeper>) {
        let clock = ManualClock::new();
        let sleeper = RecordingSleeper::new(clock.clone());
        let gate = RateGate::new(
            clock.clone(),
            sleeper.clone(),
            quota(per_second, burst),
            permits(concurrency),
        );
        (clock, sleeper, gate)
    }

    fn within(clock: &ManualClock, remaining: Duration) -> CallDeadline {
        CallDeadline::start(clock, remaining)
    }

    #[tokio::test]
    async fn rate_limiter_spaces_requests() {
        let (clock, sleeper, gate) = gate(2, 1, 4);
        let deadline = within(&clock, Duration::from_secs(60));
        for _ in 0..10 {
            drop(gate.acquire(&deadline).await.unwrap());
        }
        let waits = sleeper.waits();
        assert_eq!(waits, vec![ms(500); 9]);
        assert!(waits.iter().sum::<Duration>() >= ms(4500));
        assert_eq!(clock.now(), ms(4500));
    }

    #[tokio::test]
    async fn burst_is_available_immediately() {
        let (clock, sleeper, gate) = gate(5, 10, 16);
        let deadline = within(&clock, Duration::from_secs(60));
        let mut held = Vec::new();
        for _ in 0..10 {
            held.push(gate.acquire(&deadline).await.unwrap());
        }
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
        held.push(gate.acquire(&deadline).await.unwrap());
        assert_eq!(sleeper.waits(), vec![ms(200)]);
    }

    #[tokio::test]
    async fn rate_limit_wait_beyond_deadline_fails_fast() {
        let (clock, sleeper, gate) = gate(1, 1, 1);
        drop(gate.acquire(&within(&clock, Duration::from_secs(60))).await.unwrap());
        let failure = gate.acquire(&within(&clock, ms(500))).await.unwrap_err();
        assert_eq!(failure, CorpusError::RateLimited { retry_after: Some(Duration::from_secs(1)) });
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
        assert_eq!(clock.now(), Duration::ZERO);
        // le permis pris avant le limiteur a été rendu : le suivant n'est pas bloqué
        clock.advance(Duration::from_secs(1));
        assert!(gate.acquire(&within(&clock, ms(500))).await.is_ok());
    }

    #[tokio::test]
    async fn wait_that_exactly_fits_is_slept() {
        let (clock, sleeper, gate) = gate(1, 1, 1);
        drop(gate.acquire(&within(&clock, Duration::from_secs(60))).await.unwrap());
        let outcome = gate.acquire(&within(&clock, Duration::from_secs(1))).await;
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(sleeper.waits(), vec![Duration::from_secs(1)]);
    }

    #[tokio::test]
    async fn wait_one_nanosecond_too_long_fails_fast() {
        let (clock, sleeper, gate) = gate(1, 1, 1);
        drop(gate.acquire(&within(&clock, Duration::from_secs(60))).await.unwrap());
        let deadline = within(&clock, Duration::from_secs(1) - Duration::from_nanos(1));
        let failure = gate.acquire(&deadline).await.unwrap_err();
        assert_eq!(failure, CorpusError::RateLimited { retry_after: Some(Duration::from_secs(1)) });
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
    }

    #[tokio::test]
    async fn expired_deadline_times_out_before_taking_anything() {
        let (clock, sleeper, gate) = gate(5, 10, 1);
        let failure = gate.acquire(&within(&clock, Duration::ZERO)).await.unwrap_err();
        assert_eq!(failure, CorpusError::Timeout);
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
        // ni permis ni crédit consommés
        assert!(gate.acquire(&within(&clock, Duration::from_secs(1))).await.is_ok());
    }

    #[tokio::test]
    async fn permit_is_released_on_drop() {
        let (clock, _sleeper, gate) = gate(5, 10, 1);
        let held = gate.acquire(&within(&clock, Duration::from_secs(1))).await.unwrap();
        let failure = gate.acquire(&within(&clock, Duration::from_secs(1))).await.unwrap_err();
        assert_eq!(failure, CorpusError::Timeout);
        drop(held);
        assert!(gate.acquire(&within(&clock, Duration::from_secs(1))).await.is_ok());
    }

    #[tokio::test]
    async fn semaphore_wait_is_bounded_by_the_deadline() {
        let (clock, sleeper, gate) = gate(5, 10, 1);
        let _held = gate.acquire(&within(&clock, Duration::from_secs(60))).await.unwrap();
        let failure = gate.acquire(&within(&clock, Duration::from_secs(2))).await.unwrap_err();
        assert_eq!(failure, CorpusError::Timeout);
        assert_eq!(sleeper.waits(), vec![Duration::from_secs(2)]);
        assert_eq!(clock.now(), Duration::from_secs(2));
    }

    #[tokio::test]
    async fn free_permit_is_taken_without_arming_a_wait() {
        let (clock, sleeper, gate) = gate(5, 10, 2);
        let deadline = within(&clock, Duration::from_secs(60));
        let _first = gate.acquire(&deadline).await.unwrap();
        let _second = gate.acquire(&deadline).await.unwrap();
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
    }

    #[tokio::test]
    async fn limiter_clock_is_the_mono_clock() {
        let clock = ManualClock::new();
        let limiter_clock = MonoLimiterClock(clock.clone());
        assert_eq!(governor::clock::Clock::now(&limiter_clock), Duration::ZERO);
        clock.advance(ms(1234));
        assert_eq!(governor::clock::Clock::now(&limiter_clock), ms(1234));

        let (clock, sleeper, gate) = gate(1, 1, 1);
        let deadline = within(&clock, Duration::from_secs(60));
        drop(gate.acquire(&deadline).await.unwrap());
        // le temps réel ne rend aucun crédit : seule l'horloge injectée compte
        std::thread::sleep(ms(20));
        clock.advance(Duration::from_secs(1));
        drop(gate.acquire(&deadline).await.unwrap());
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
    }

    #[tokio::test]
    async fn quota_replenishes_with_the_clock() {
        let (clock, sleeper, gate) = gate(1, 3, 8);
        let deadline = within(&clock, Duration::from_secs(60));
        for _ in 0..3 {
            drop(gate.acquire(&deadline).await.unwrap());
        }
        clock.advance(Duration::from_secs(2));
        drop(gate.acquire(&deadline).await.unwrap());
        drop(gate.acquire(&deadline).await.unwrap());
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
        drop(gate.acquire(&deadline).await.unwrap());
        assert_eq!(sleeper.waits(), vec![Duration::from_secs(1)]);
    }

    #[tokio::test]
    async fn exhausted_check_budget_reports_rate_limited() {
        let clock = ManualClock::new();
        // l'attente avance une autre horloge : le crédit du limiteur ne revient jamais
        let sleeper = RecordingSleeper::new(ManualClock::new());
        let gate = RateGate::new(clock.clone(), sleeper.clone(), quota(1, 1), permits(1));
        let deadline = within(&clock, Duration::from_secs(600));
        drop(gate.acquire(&deadline).await.unwrap());
        let failure = gate.acquire(&deadline).await.unwrap_err();
        assert_eq!(failure, CorpusError::RateLimited { retry_after: Some(Duration::from_secs(1)) });
        let checks = usize::try_from(MAX_CHECKS).unwrap();
        assert_eq!(sleeper.waits(), vec![Duration::from_secs(1); checks]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrency_cap_is_respected() {
        // horloge et attente réelles : un RecordingSleeper, prêt tout de suite, ferait
        // échouer les tâches en attente par Timeout
        let clock = TokioClock::new();
        let gate = Arc::new(RateGate::new(clock, TokioSleeper, quota(1000, 1000), permits(4)));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let tasks: Vec<_> = (0..10)
            .map(|_| {
                let gate = Arc::clone(&gate);
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                tokio::spawn(async move {
                    let deadline = CallDeadline::start(&clock, Duration::from_secs(30));
                    let permit = gate.acquire(&deadline).await.unwrap();
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(ms(20)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    drop(permit);
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(active.load(Ordering::SeqCst), 0);
        let peak = peak.load(Ordering::SeqCst);
        assert!((2..=4).contains(&peak), "peak = {peak}");
    }
}
