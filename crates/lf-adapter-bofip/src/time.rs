//! Temps de l'adapter : une horloge monotone et une attente, réunies dans [`Timer`].
//!
//! Ce n'est **pas un port** : le trait n'existe que pour rendre déterministes les tests du débit
//! (départs espacés, repos après un refus, échéance) sans attendre pour de vrai. L'horloge murale,
//! elle, est le port `Clock` de l'application : elle ne sert qu'au jour du budget.
//!
//! Visibilité : [`Timer`] et [`TokioTimer`] sont `pub` dans ce module privé parce que la
//! signature publique de `BofipDoctrine<W, T>` les nomme ; innommables hors du crate, donc
//! scellés.

use std::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
    time::Duration,
};

/// Temps monotone et attente de l'adapter.
pub trait Timer: Send + Sync + 'static {
    /// Temps écoulé depuis l'origine de ce minuteur. Deux minuteurs ne se comparent pas.
    fn now(&self) -> Duration;

    /// Attend `duration`. Le futur rendu ne compte que lorsqu'il est attendu ; celui de
    /// [`TokioTimer`] fixe son échéance dès l'appel.
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// Minuteur de production, fondé sur `tokio::time`.
#[derive(Debug, Clone, Copy)]
pub struct TokioTimer {
    origin: tokio::time::Instant,
}

impl TokioTimer {
    /// Minuteur dont l'origine est l'instant présent.
    pub(crate) fn start() -> Self {
        Self { origin: tokio::time::Instant::now() }
    }
}

impl Timer for TokioTimer {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(duration)
    }
}

/// Course entre un travail et une limite : `Some(résultat)` si le travail finit le premier,
/// `None` si la limite tombe avant lui.
///
/// Le travail est interrogé **d'abord** : quand les deux sont prêts, son résultat l'emporte, et
/// la limite n'est même pas interrogée. Écrite sans `select!` (donc sans la feature `macros` de
/// tokio) pour que la limite passe par le [`Timer`] injecté.
pub(crate) async fn within<R>(
    work: impl Future<Output = R>,
    limit: impl Future<Output = ()>,
) -> Option<R> {
    let mut work = pin!(work);
    let mut limit = pin!(limit);
    poll_fn(|cx| match work.as_mut().poll(cx) {
        Poll::Ready(value) => Poll::Ready(Some(value)),
        Poll::Pending => limit.as_mut().poll(cx).map(|()| None),
    })
    .await
}

#[cfg(test)]
pub(crate) use doubles::ManualTimer;

#[cfg(test)]
mod doubles {
    //! Minuteur de test : le temps n'avance que sur demande, et une attente le fait avancer
    //! d'autant sans attendre.

    use std::{
        future::{Future, pending},
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    use super::Timer;

    /// Minuteur manuel ; les clones partagent le même temps et le même relevé d'attentes.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct ManualTimer {
        nanos: Arc<AtomicU64>,
        sleeps: Arc<Mutex<Vec<Duration>>>,
        /// Durée à partir de laquelle une attente ne finit jamais.
        hold_from: Option<Duration>,
        /// Vrai si chaque attente rend la main une fois avant de finir.
        yields: bool,
    }

    impl ManualTimer {
        /// Minuteur dont les attentes de `from` ou plus restent en suspens, sans rien noter ni
        /// avancer : une limite lointaine qui ne tombe pas pendant le test.
        pub(crate) fn holding(from: Duration) -> Self {
            Self { hold_from: Some(from), ..Self::default() }
        }

        /// Minuteur dont chaque attente rend la main une fois avant de finir : le temps qu'un
        /// autre appel, interrogé entre-temps, reçoive son refus.
        pub(crate) fn yielding() -> Self {
            Self { yields: true, ..Self::default() }
        }

        /// Avance le temps de `by`.
        pub(crate) fn advance(&self, by: Duration) {
            self.nanos.fetch_add(u64::try_from(by.as_nanos()).unwrap(), Ordering::SeqCst);
        }

        /// Les attentes demandées **et** attendues, dans l'ordre.
        pub(crate) fn sleeps(&self) -> Vec<Duration> {
            self.sleeps.lock().unwrap().clone()
        }
    }

    impl Timer for ManualTimer {
        fn now(&self) -> Duration {
            Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
        }

        /// Paresseuse : rien n'est noté ni avancé tant que le futur n'est pas interrogé. Une
        /// limite jamais atteinte ne laisse donc aucune trace.
        fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
            let timer = self.clone();
            let held = self.hold_from.is_some_and(|from| duration >= from);
            let yields = self.yields;
            async move {
                if held {
                    pending::<()>().await;
                }
                if yields {
                    tokio::task::yield_now().await;
                }
                timer.sleeps.lock().unwrap().push(duration);
                timer.advance(duration);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, ready};

    use pretty_assertions::assert_eq;

    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[tokio::test]
    async fn the_work_wins_when_both_are_ready_and_the_limit_is_not_even_polled() {
        let timer = ManualTimer::default();
        assert_eq!(within(ready(7), timer.sleep(ms(30))).await, Some(7));
        assert_eq!((timer.now(), timer.sleeps()), (ms(0), Vec::new()));
    }

    #[tokio::test]
    async fn the_limit_ends_a_work_that_is_not_ready() {
        let timer = ManualTimer::default();
        assert_eq!(within(pending::<u8>(), timer.sleep(ms(30))).await, None);
        assert_eq!((timer.now(), timer.sleeps()), (ms(30), vec![ms(30)]));
    }

    #[tokio::test]
    async fn the_manual_timer_only_moves_on_demand_and_clones_share_it() {
        let timer = ManualTimer::default();
        let other = timer.clone();
        assert_eq!(timer.now(), ms(0));
        timer.advance(ms(1_500));
        other.sleep(ms(500)).await;
        assert_eq!(
            (timer.now(), other.now(), timer.sleeps()),
            (ms(2_000), ms(2_000), vec![ms(500)])
        );
    }

    #[tokio::test]
    async fn a_holding_timer_never_ends_a_long_wait() {
        let timer = ManualTimer::holding(ms(1_000));
        assert_eq!(within(timer.sleep(ms(1_000)), timer.sleep(ms(999))).await, None);
        assert_eq!((timer.now(), timer.sleeps()), (ms(999), vec![ms(999)]));
    }

    #[tokio::test(start_paused = true)]
    async fn the_tokio_timer_measures_elapsed_time_from_its_start() {
        let timer = TokioTimer::start();
        assert_eq!(timer.now(), ms(0));
        timer.sleep(ms(250)).await;
        assert_eq!(timer.now(), ms(250));
        assert_eq!(within(pending::<u8>(), timer.sleep(ms(750))).await, None);
        assert_eq!(timer.now(), ms(1_000));
    }
}
