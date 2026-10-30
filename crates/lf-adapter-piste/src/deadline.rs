//! Échéance d'un appel d'outil, exprimée dans l'horloge monotone de l'adapter.
//!
//! `lf_application::deadline::Deadline` vit sur l'horloge `tokio` et se publie en variable
//! locale de tâche. L'adapter n'en lit que la durée **restante**, qu'il pose aussitôt sur
//! [`MonoClock`] : aucun instant absolu ne passe d'une horloge à l'autre (04 §5.3). C'est ce
//! qui garde cohérents les tests qui pilotent un `ManualClock` d'origine arbitraire.
//!
//! Toutes les méthodes sont pures : elles reçoivent l'horloge en paramètre et n'attendent
//! jamais.

use std::time::Duration;

use lf_application::deadline::Deadline;

use crate::time::MonoClock;

/// Échéance d'un appel, comptée en temps écoulé de l'horloge qui l'a créée.
///
/// Non générique : la mélanger avec une autre horloge n'a pas de sens, et les types qui la
/// consomment n'ont ainsi aucun paramètre d'horloge à propager. Ni `new` ni `Default` : une
/// échéance « par défaut » n'a pas de sens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CallDeadline {
    at: Duration,
}

impl CallDeadline {
    /// Échéance située `remaining` après l'instant courant de `clock` (saturée).
    pub(crate) fn start(clock: &impl MonoClock, remaining: Duration) -> Self {
        Self { at: clock.now().saturating_add(remaining) }
    }

    /// Échéance lue dans la variable locale de tâche, ou `tool_deadline` hors de tout `scope`.
    pub(crate) fn from_task_local(clock: &impl MonoClock, tool_deadline: Duration) -> Self {
        Self::start(clock, Deadline::current_or(tool_deadline))
    }

    /// Durée restante avant l'échéance, nulle une fois celle-ci passée.
    pub(crate) fn remaining(&self, clock: &impl MonoClock) -> Duration {
        self.at.saturating_sub(clock.now())
    }

    /// Vrai quand il ne reste rien : l'instant exact de l'échéance compte comme expiré.
    ///
    /// Un reste nul suffit, sans comparer deux instants : la saturation de
    /// [`Self::remaining`] rend le cas « dépassée de longtemps » identique à l'égalité.
    pub(crate) fn is_expired(&self, clock: &impl MonoClock) -> bool {
        self.remaining(clock).is_zero()
    }

    /// Délai alloué à une tentative : `min(attempt_timeout, reste)` (04 §5.3).
    pub(crate) fn cap(&self, clock: &impl MonoClock, attempt_timeout: Duration) -> Duration {
        attempt_timeout.min(self.remaining(clock))
    }

    /// Vrai si une attente de `wait` tient dans le reste.
    ///
    /// Frontière arrêtée : l'égalité tient, seul un dépassement strict est refusé (04 §5.4 :
    /// « si `wait` dépasse le reste »). À l'échéance, `fits(0)` est donc vrai et
    /// `fits(1 ms)` faux ; l'appelant constate l'expiration par [`Self::is_expired`].
    pub(crate) fn fits(&self, clock: &impl MonoClock, wait: Duration) -> bool {
        wait <= self.remaining(clock)
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::ManualClock;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Horloge d'origine non nulle (environ 11,6 jours) : toute confusion avec un temps
    /// relatif ou avec l'horloge `tokio` fait échouer le test.
    fn clock_at_arbitrary_origin() -> ManualClock {
        let clock = ManualClock::new();
        clock.advance(Duration::from_secs(1_000_000) + ms(123));
        clock
    }

    // Exception assumée à « pas de start_paused » (04 §13.1) : `Deadline` vit sur l'horloge
    // tokio, qu'il faut figer pour lire « exactement 1 s » sans attendre. Le test ne fait
    // aucune E/S ; seul `Deadline` voit le temps tokio, l'adapter ne lit que `ManualClock`.
    #[tokio::test(start_paused = true)]
    async fn deadline_from_task_local_is_used() {
        let clock = clock_at_arbitrary_origin();
        let origin = clock.now();
        let seen = Deadline::after(Duration::from_secs(1))
            .scope(async {
                let deadline = CallDeadline::from_task_local(&clock, Duration::from_secs(60));
                clock.advance(ms(400)); // le temps de l'adapter passe sans toucher à l'horloge tokio
                (deadline, deadline.remaining(&clock))
            })
            .await;
        assert_eq!(
            seen,
            (CallDeadline { at: origin + Duration::from_secs(1) }, ms(600)),
            "l'échéance est exprimée dans ManualClock, pas dans l'horloge tokio"
        );
    }

    #[test]
    fn no_deadline_published_uses_tool_deadline() {
        let clock = clock_at_arbitrary_origin();
        let deadline = CallDeadline::from_task_local(&clock, Duration::from_secs(60));
        assert_eq!(deadline.remaining(&clock), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn spawned_task_does_not_see_the_deadline() {
        let clock = clock_at_arbitrary_origin();
        let seen = Deadline::after(Duration::from_secs(1))
            .scope(async {
                tokio::spawn(async move {
                    CallDeadline::from_task_local(&clock, Duration::from_secs(60)).remaining(&clock)
                })
                .await
            })
            .await
            .unwrap();
        assert_eq!(
            seen,
            Duration::from_secs(60),
            "tokio::spawn ne propage pas la variable locale de tâche"
        );
    }

    #[test]
    fn remaining_saturates_at_zero_after_expiry() {
        let clock = clock_at_arbitrary_origin();
        let deadline = CallDeadline::start(&clock, ms(100));
        let state = |c: &ManualClock| (deadline.remaining(c), deadline.is_expired(c));
        let mut seen = vec![state(&clock)];
        clock.advance(ms(99));
        seen.push(state(&clock));
        clock.advance(ms(1));
        seen.push(state(&clock));
        clock.advance(Duration::from_secs(3600));
        seen.push(state(&clock));
        assert_eq!(
            seen,
            vec![(ms(100), false), (ms(1), false), (Duration::ZERO, true), (Duration::ZERO, true),]
        );
    }

    #[test]
    fn cap_is_the_minimum_of_timeout_and_remaining() {
        let clock = clock_at_arbitrary_origin();
        let deadline = CallDeadline::start(&clock, ms(1000));
        let before = [ms(400), ms(1000), ms(5000)].map(|t| deadline.cap(&clock, t));
        assert_eq!(before, [ms(400), ms(1000), ms(1000)]);
        clock.advance(ms(1200));
        assert_eq!(deadline.cap(&clock, ms(400)), Duration::ZERO);
    }

    #[test]
    fn fits_is_exact_at_the_boundary() {
        let clock = clock_at_arbitrary_origin();
        let deadline = CallDeadline::start(&clock, ms(1000));
        let before = [0, 999, 1000, 1001].map(|n| deadline.fits(&clock, ms(n)));
        assert_eq!(before, [true, true, true, false], "l'égalité tient, le dépassement non");
        clock.advance(ms(1000));
        let at_deadline = [0, 1].map(|n| deadline.fits(&clock, ms(n)));
        assert_eq!(at_deadline, [true, false], "à l'échéance, seule l'attente nulle tient");
    }

    #[test]
    fn start_does_not_overflow_with_huge_remaining() {
        let clock = clock_at_arbitrary_origin();
        let deadline = CallDeadline::start(&clock, Duration::MAX);
        assert_eq!(
            (
                deadline.remaining(&clock),
                deadline.cap(&clock, ms(40)),
                deadline.fits(&clock, ms(40)),
                deadline.is_expired(&clock),
            ),
            (Duration::MAX.saturating_sub(clock.now()), ms(40), true, false)
        );
    }
}
