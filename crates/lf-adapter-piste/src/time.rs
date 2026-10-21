//! Temps de l'adapter : horloge monotone et attente.
//!
//! Ce ne sont **pas des ports** : ces traits n'existent que pour rendre déterministes les
//! tests du jeton, du backoff, de `Retry-After`, du débit et du délai restant, sans geler
//! l'horloge `tokio` (l'avance automatique d'une horloge gelée peut déclencher à tort les
//! délais de `reqwest`).

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::task::Poll;
use std::time::Duration;

/// Horloge monotone : temps écoulé depuis une origine propre à chaque horloge.
///
/// Deux horloges ne se comparent pas entre elles. C'est l'« instant » que consomme le
/// limiteur de débit.
pub(crate) trait MonoClock: Send + Sync + 'static {
    /// Temps monotone écoulé depuis l'origine de cette horloge.
    fn now(&self) -> Duration;
}

/// Attente asynchrone : seule porte d'entrée des attentes de l'adapter.
pub(crate) trait Sleeper: Send + Sync + 'static {
    /// Attend `duration`, sans effet tant que le futur n'est pas attendu.
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// Horloge de production, fondée sur `tokio::time::Instant`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TokioClock {
    origin: tokio::time::Instant,
}

impl TokioClock {
    /// Crée une horloge dont l'origine est l'instant présent.
    pub(crate) fn new() -> Self {
        Self { origin: tokio::time::Instant::now() }
    }
}

impl MonoClock for TokioClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// Attente de production, fondée sur `tokio::time::sleep`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(duration)
    }
}

/// Course entre un travail et une attente : `Some(résultat)` si le travail finit le premier,
/// `None` si l'attente finit avant lui.
///
/// Le travail est interrogé **d'abord** : quand les deux sont prêts en même temps, le résultat
/// du travail l'emporte. Écrite sans `select!` (donc sans la feature `macros` de tokio) et sans
/// `tokio::time::timeout`, pour que l'attente passe par le [`Sleeper`] injecté, déterministe en
/// test. L'attente arrive en objet-trait (sans allocation) : un seul code de comparaison sert
/// tous les `Sleeper`.
pub(crate) async fn race<T>(
    work: impl Future<Output = T>,
    mut timeout: Pin<&mut (dyn Future<Output = ()> + Send)>,
) -> Option<T> {
    let mut work = pin!(work);
    poll_fn(|cx| match work.as_mut().poll(cx) {
        Poll::Ready(value) => Poll::Ready(Some(value)),
        Poll::Pending => timeout.as_mut().poll(cx).map(|()| None),
    })
    .await
}

#[cfg(test)]
mod doubles {
    //! Doublures déterministes pour les tests : horloge manuelle et attente enregistrée.

    use super::{MonoClock, Sleeper};
    use std::future::Future;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Horloge qui n'avance que sur demande ; les clones partagent le même temps.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct ManualClock {
        nanos: Arc<AtomicU64>,
    }

    impl ManualClock {
        /// Crée une horloge à zéro.
        pub(crate) fn new() -> Self {
            Self::default()
        }

        /// Avance l'horloge de `by`.
        pub(crate) fn advance(&self, by: Duration) {
            let nanos = u64::try_from(by.as_nanos()).expect("advance fits in u64 nanoseconds");
            self.nanos.fetch_add(nanos, Ordering::SeqCst);
        }
    }

    impl MonoClock for ManualClock {
        fn now(&self) -> Duration {
            Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
        }
    }

    /// Attente qui note chaque durée puis avance l'horloge manuelle, sans attendre.
    #[derive(Debug, Clone)]
    pub(crate) struct RecordingSleeper {
        clock: ManualClock,
        waits: Arc<Mutex<Vec<Duration>>>,
    }

    impl RecordingSleeper {
        /// Crée une attente liée à `clock`.
        pub(crate) fn new(clock: ManualClock) -> Self {
            Self { clock, waits: Arc::new(Mutex::new(Vec::new())) }
        }

        /// Durées demandées, dans l'ordre.
        pub(crate) fn waits(&self) -> Vec<Duration> {
            self.waits.lock().expect("waits lock not poisoned").clone()
        }
    }

    impl Sleeper for RecordingSleeper {
        fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
            let clock = self.clock.clone();
            let waits = Arc::clone(&self.waits);
            async move {
                waits.lock().expect("waits lock not poisoned").push(duration);
                clock.advance(duration);
            }
        }
    }
}

#[cfg(test)]
pub(crate) use doubles::{ManualClock, RecordingSleeper};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn manual_clock_starts_at_zero_and_only_moves_on_advance() {
        let clock = ManualClock::new();
        assert_eq!(clock.now(), Duration::ZERO);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(clock.now(), Duration::ZERO);
        clock.advance(Duration::from_millis(1500));
        assert_eq!(clock.now(), Duration::from_millis(1500));
        clock.advance(Duration::from_nanos(7));
        assert_eq!(clock.now(), Duration::from_millis(1500) + Duration::from_nanos(7));
    }

    #[test]
    fn manual_clock_clones_share_the_same_time() {
        let clock = ManualClock::new();
        let twin = clock.clone();
        twin.advance(Duration::from_secs(3));
        assert_eq!(clock.now(), Duration::from_secs(3));
    }

    #[tokio::test]
    async fn recording_sleeper_advances_the_clock_and_notes_waits_in_order() {
        let clock = ManualClock::new();
        let sleeper = RecordingSleeper::new(clock.clone());
        assert_eq!(sleeper.waits(), Vec::<Duration>::new());
        sleeper.sleep(Duration::from_millis(500)).await;
        assert_eq!(clock.now(), Duration::from_millis(500));
        sleeper.sleep(Duration::from_secs(2)).await;
        assert_eq!(clock.now(), Duration::from_millis(2500));
        assert_eq!(sleeper.waits(), vec![Duration::from_millis(500), Duration::from_secs(2)]);
    }

    #[tokio::test]
    async fn recording_sleeper_clones_share_the_waits() {
        let sleeper = RecordingSleeper::new(ManualClock::new());
        let twin = sleeper.clone();
        twin.sleep(Duration::from_millis(1)).await;
        assert_eq!(sleeper.waits(), vec![Duration::from_millis(1)]);
    }

    #[test]
    fn tokio_clock_is_monotone_and_follows_real_time() {
        let clock = TokioClock::new();
        let first = clock.now();
        std::thread::sleep(Duration::from_millis(10));
        let second = clock.now();
        assert!(second > first, "{first:?} then {second:?}");
        assert!(second >= Duration::from_millis(10), "{second:?}");
    }

    #[tokio::test]
    async fn tokio_sleeper_really_waits() {
        let started = Instant::now();
        TokioSleeper.sleep(Duration::from_millis(20)).await;
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(20), "{elapsed:?}");
    }

    #[tokio::test]
    async fn race_returns_the_work_when_it_is_ready() {
        let outcome = race(std::future::ready(7_u8), pin!(std::future::ready(()))).await;
        assert_eq!(outcome, Some(7));
    }

    #[tokio::test]
    async fn race_polls_the_work_before_the_timeout() {
        let outcome = race(std::future::ready("work"), pin!(std::future::ready(()))).await;
        assert_eq!(outcome, Some("work"));
    }

    #[tokio::test]
    async fn race_gives_up_when_the_timeout_ends_first() {
        let outcome = race(std::future::pending::<u8>(), pin!(std::future::ready(()))).await;
        assert_eq!(outcome, None);
    }

    #[tokio::test]
    async fn race_waits_for_pending_work_while_the_timeout_is_pending_too() {
        let mut polls = 0_u8;
        let work = poll_fn(|cx| {
            polls += 1;
            if polls == 1 {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(polls)
            }
        });
        let outcome = race(work, pin!(std::future::pending::<()>())).await;
        assert_eq!(outcome, Some(2));
    }
}
