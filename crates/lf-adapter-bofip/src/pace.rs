//! Débit et quota vers le portail (ADR-0029, décision 7).
//!
//! Quatre règles, portées par l'instance du client (jamais par un `static`) :
//!
//! - **deux départs par seconde au plus** : deux départs sont espacés d'au moins
//!   [`START_INTERVAL`] ; une attente qui ne tient pas dans l'échéance de l'appel donne
//!   [`DoctrineError::Timeout`] ;
//! - **deux lectures en vol au plus** ([`IN_FLIGHT_MAX`]) : le [`Permit`] se garde jusqu'à la fin
//!   de la lecture du corps et de son décodage ;
//! - **[`DAILY_BUDGET`] requêtes par jour (UTC) et par processus** : au-delà,
//!   [`DoctrineError::RateLimited`] sans réseau, jusqu'à minuit UTC ;
//! - **repos après un refus 429** : la durée annoncée par `Retry-After`, [`REST_MAX`] au plus,
//!   [`REST_DEFAULT`] si l'en-tête manque, est illisible ou annonce zéro seconde (un refus qui
//!   invite à revenir aussitôt n'annonce rien d'utilisable) ; pendant le repos, tout nouvel appel
//!   est refusé sans réseau, et celui qui attendait son tour de départ ne part pas. Un appel déjà
//!   parti n'est pas rattrapé : avec deux lectures en vol, au plus une requête de plus.
//!
//! Aucun réessai, nulle part. Le calcul est une machine à états pure ([`Schedule`]) ; [`Pace`]
//! lui ajoute le verrou, le sémaphore et l'attente.

use std::{
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use lf_application::ports::{Clock, DoctrineError};
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::time::{Timer, within};

/// Écart minimal entre deux départs de requête : deux départs par seconde.
pub(crate) const START_INTERVAL: Duration = Duration::from_millis(500);

/// Lectures en vol au plus.
pub(crate) const IN_FLIGHT_MAX: usize = 2;

/// Requêtes par jour (UTC) et par processus. Le portail en accorde 50 000 par jour à chaque
/// adresse (en-tête `x-ratelimit-limit`, relevé le 2026-11-15).
pub(crate) const DAILY_BUDGET: u32 = 10_000;

/// Repos le plus long après un refus 429, quoi qu'annonce le portail.
pub(crate) const REST_MAX: Duration = Duration::from_secs(3_600);

/// Repos après un refus 429 sans `Retry-After` lisible, ou qui annonce zéro seconde.
pub(crate) const REST_DEFAULT: Duration = Duration::from_secs(900);

/// Secondes d'un jour.
const DAY_SECONDS: i64 = 86_400;

/// Ce que le budget du jour lit de l'horloge murale : le jour UTC et le temps qui reste avant le
/// suivant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UtcDay {
    /// Numéro du jour, compté depuis le 1970-01-01.
    number: i64,
    /// Temps restant avant minuit UTC : de 1 s à 24 h.
    until_next: Duration,
}

impl UtcDay {
    /// Jour de l'instant `timestamp`, en secondes depuis le 1970-01-01 à 00:00 UTC.
    pub(crate) fn at(timestamp: i64) -> Self {
        let elapsed = timestamp.rem_euclid(DAY_SECONDS);
        let left = DAY_SECONDS.saturating_sub(elapsed);
        Self {
            number: timestamp.div_euclid(DAY_SECONDS),
            until_next: Duration::from_secs(u64::try_from(left).unwrap_or(0)),
        }
    }
}

/// État du débit, sans horloge ni attente : chaque méthode reçoit l'instant.
#[derive(Debug, Default)]
struct Schedule {
    /// Premier instant où un départ est permis.
    next_start: Duration,
    /// Jour UTC auquel `used` se rapporte.
    day: i64,
    /// Requêtes parties ce jour-là.
    used: u32,
    /// Fin du repos qui suit un refus 429 ; passé, donc sans effet, tant qu'aucun refus n'est
    /// arrivé.
    rest_until: Duration,
}

impl Schedule {
    /// Réserve un départ à l'instant `now`, pour un appel à qui il reste `remaining` : rend
    /// l'attente à observer avant de partir.
    ///
    /// Un refus ne réserve rien et ne consomme rien du budget.
    ///
    /// # Errors
    ///
    /// [`DoctrineError::RateLimited`] pendant un repos (avec le temps qu'il lui reste) ou quand
    /// le budget du jour est épuisé (avec le temps avant minuit UTC) ;
    /// [`DoctrineError::Timeout`] si l'attente ne laisse rien à l'appel.
    fn plan(
        &mut self,
        now: Duration,
        today: UtcDay,
        remaining: Duration,
    ) -> Result<Duration, DoctrineError> {
        self.rested(now)?;
        if today.number != self.day {
            self.day = today.number;
            self.used = 0;
        }
        if self.used >= DAILY_BUDGET {
            return Err(DoctrineError::RateLimited { retry_after: Some(today.until_next) });
        }
        let wait = self.next_start.saturating_sub(now);
        if wait >= remaining {
            return Err(DoctrineError::Timeout);
        }
        self.next_start = now.saturating_add(wait).saturating_add(START_INTERVAL);
        self.used = self.used.saturating_add(1);
        Ok(wait)
    }

    /// Vérifie qu'aucun repos n'est en cours à l'instant `now`.
    ///
    /// # Errors
    ///
    /// [`DoctrineError::RateLimited`] pendant un repos, avec le temps qu'il lui reste.
    fn rested(&self, now: Duration) -> Result<(), DoctrineError> {
        if now < self.rest_until {
            let left = self.rest_until.saturating_sub(now);
            return Err(DoctrineError::RateLimited { retry_after: Some(left) });
        }
        Ok(())
    }

    /// Ouvre un repos à l'instant `now`, de la durée `announced` par le portail, bornée ; rend
    /// la durée retenue. Zéro seconde est lu comme une durée absente : le repos par défaut.
    fn rest(&mut self, now: Duration, announced: Option<Duration>) -> Duration {
        let rest = announced.filter(|rest| !rest.is_zero()).unwrap_or(REST_DEFAULT).min(REST_MAX);
        self.rest_until = now.saturating_add(rest);
        rest
    }
}

/// Droit d'envoyer une requête et d'en lire la réponse ; la place en vol est rendue quand il
/// tombe.
#[derive(Debug)]
pub(crate) struct Permit<'a> {
    _in_flight: SemaphorePermit<'a>,
}

/// Le débit d'une instance du client.
#[derive(Debug)]
pub(crate) struct Pace<W: Clock, T: Timer> {
    /// Horloge murale : le jour UTC du budget.
    wall: W,
    /// Temps monotone et attente.
    timer: T,
    schedule: Mutex<Schedule>,
    in_flight: Semaphore,
}

impl<W: Clock, T: Timer> Pace<W, T> {
    /// Débit neuf : aucun départ pris, aucun repos.
    pub(crate) fn new(wall: W, timer: T) -> Self {
        Self {
            wall,
            timer,
            schedule: Mutex::new(Schedule::default()),
            in_flight: Semaphore::new(IN_FLIGHT_MAX),
        }
    }

    /// Le minuteur, pour que l'appelant compte son échéance sur la même horloge.
    pub(crate) fn timer(&self) -> &T {
        &self.timer
    }

    /// Attend une place en vol, puis son tour de départ, pour un appel à qui il reste
    /// `remaining`.
    ///
    /// Le repos est relu après l'attente du tour : un refus 429 reçu par un autre appel pendant
    /// cette attente arrête aussi celui-ci, qui garde le départ compté au budget.
    ///
    /// # Errors
    ///
    /// [`DoctrineError::Timeout`] si la place ou le tour ne viennent pas dans `remaining` ;
    /// [`DoctrineError::RateLimited`] pendant un repos ou quand le budget du jour est épuisé.
    /// Aucune de ces erreurs n'a touché le réseau.
    pub(crate) async fn acquire(&self, remaining: Duration) -> Result<Permit<'_>, DoctrineError> {
        let asked = self.timer.now();
        let in_flight = within(self.in_flight.acquire(), self.timer.sleep(remaining))
            .await
            .and_then(Result::ok)
            .ok_or(DoctrineError::Timeout)?;
        let now = self.timer.now();
        let left = remaining.saturating_sub(now.saturating_sub(asked));
        let today = UtcDay::at(self.wall.now_utc().timestamp());
        let wait = self.schedule().plan(now, today, left)?;
        self.timer.sleep(wait).await;
        self.schedule().rested(self.timer.now())?;
        Ok(Permit { _in_flight: in_flight })
    }

    /// Ouvre le repos qui suit un refus 429 ; rend sa durée.
    pub(crate) fn rest(&self, announced: Option<Duration>) -> Duration {
        let now = self.timer.now();
        self.schedule().rest(now, announced)
    }

    /// L'état, même si un autre fil a paniqué en le tenant : il ne porte que des compteurs.
    fn schedule(&self) -> MutexGuard<'_, Schedule> {
        self.schedule.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        testing::{MIDDAY, TestWall},
        time::ManualTimer,
    };

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    const fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// Un jour quelconque, à midi.
    fn noon() -> UtcDay {
        UtcDay::at(MIDDAY)
    }

    const LONG: Duration = Duration::from_secs(60);

    fn limited(retry_after: Duration) -> Result<Duration, DoctrineError> {
        Err(DoctrineError::RateLimited { retry_after: Some(retry_after) })
    }

    #[test]
    fn a_utc_day_is_numbered_from_the_epoch_and_knows_the_time_left() {
        let rows = [
            (0, 0, 86_400),
            (1, 0, 86_399),
            (86_399, 0, 1),
            (86_400, 1, 86_400),
            (MIDDAY, 20_772, 43_200),
            (-1, -1, 1),
            (-86_400, -1, 86_400),
            (-86_401, -2, 1),
        ];
        for (timestamp, number, left) in rows {
            assert_eq!(
                UtcDay::at(timestamp),
                UtcDay { number, until_next: s(left) },
                "{timestamp}"
            );
        }
    }

    #[test]
    fn starts_are_spaced_by_half_a_second() {
        let mut schedule = Schedule::default();
        // Le premier part aussitôt ; les suivants, demandés au même instant, se rangent derrière.
        assert_eq!(schedule.plan(s(10), noon(), LONG), Ok(ms(0)));
        assert_eq!(schedule.plan(s(10), noon(), LONG), Ok(ms(500)));
        assert_eq!(schedule.plan(s(10), noon(), LONG), Ok(ms(1_000)));
        // Un départ demandé plus tard n'attend que ce qui reste de l'écart.
        assert_eq!(schedule.plan(ms(11_400), noon(), LONG), Ok(ms(100)));
        // Passé l'écart, plus d'attente ; l'écart suivant part de ce départ-là.
        assert_eq!(schedule.plan(s(30), noon(), LONG), Ok(ms(0)));
        assert_eq!(schedule.plan(ms(30_499), noon(), LONG), Ok(ms(1)));
        assert_eq!(schedule.plan(ms(31_000), noon(), LONG), Ok(ms(0)));
        assert_eq!(schedule.used, 7);
    }

    #[test]
    fn a_wait_that_leaves_nothing_to_the_call_is_a_timeout_and_reserves_nothing() {
        let mut schedule = Schedule::default();
        assert_eq!(schedule.plan(s(10), noon(), LONG), Ok(ms(0)));
        // 500 ms d'attente : il faut qu'il reste plus que cela à l'appel.
        assert_eq!(schedule.plan(s(10), noon(), ms(499)), Err(DoctrineError::Timeout));
        assert_eq!(schedule.plan(s(10), noon(), ms(500)), Err(DoctrineError::Timeout));
        assert_eq!((schedule.used, schedule.next_start), (1, ms(10_500)));
        assert_eq!(schedule.plan(s(10), noon(), ms(501)), Ok(ms(500)));
        // Un appel sans temps restant ne part pas, même sans attente.
        assert_eq!(schedule.plan(s(60), noon(), ms(0)), Err(DoctrineError::Timeout));
        assert_eq!(schedule.plan(s(60), noon(), ms(1)), Ok(ms(0)));
        assert_eq!(schedule.used, 3);
    }

    #[test]
    fn retry_after_capped_at_one_hour() {
        let rows = [
            (Some(s(120)), s(120)),
            (Some(s(1)), s(1)),
            (Some(ms(1)), ms(1)),
            (Some(s(3_600)), s(3_600)),
            (Some(s(3_601)), s(3_600)),
            (Some(s(86_400)), s(3_600)),
            (Some(Duration::MAX), s(3_600)),
            // Sans en-tête lisible, ou avec zéro seconde : un quart d'heure.
            (None, s(900)),
            (Some(s(0)), s(900)),
        ];
        for (announced, rest) in rows {
            let mut schedule = Schedule::default();
            assert_eq!(schedule.rest(s(50), announced), rest, "{announced:?}");
            assert_eq!(schedule.rest_until, s(50) + rest, "{announced:?}");
        }
    }

    #[test]
    fn during_a_rest_every_start_is_refused_with_the_time_left() {
        let mut schedule = Schedule::default();
        assert_eq!(schedule.plan(s(10), noon(), LONG), Ok(ms(0)));
        assert_eq!(schedule.rest(s(11), Some(s(120))), s(120));
        assert_eq!(schedule.plan(s(11), noon(), LONG), limited(s(120)));
        assert_eq!(schedule.plan(s(100), noon(), LONG), limited(s(31)));
        assert_eq!(schedule.plan(ms(130_999), noon(), LONG), limited(ms(1)));
        // Un refus pendant le repos ne consomme ni départ ni budget.
        assert_eq!((schedule.used, schedule.next_start), (1, ms(10_500)));
        // À la fin du repos, exactement, les départs reprennent.
        assert_eq!(schedule.plan(s(131), noon(), LONG), Ok(ms(0)));
        assert_eq!(schedule.used, 2);
    }

    #[test]
    fn the_daily_budget_is_per_utc_day() {
        let mut schedule =
            Schedule { day: noon().number, used: DAILY_BUDGET - 1, ..Schedule::default() };
        assert_eq!(schedule.plan(s(10), noon(), LONG), Ok(ms(0)));
        assert_eq!(schedule.used, DAILY_BUDGET);
        // Épuisé : refus jusqu'à minuit UTC, sans rien réserver.
        assert_eq!(schedule.plan(s(20), noon(), LONG), limited(s(43_200)));
        let evening = UtcDay::at(MIDDAY + 43_199);
        assert_eq!(schedule.plan(s(30), evening, LONG), limited(s(1)));
        assert_eq!((schedule.used, schedule.next_start), (DAILY_BUDGET, ms(10_500)));
        // Le jour suivant, le compteur repart de zéro.
        let tomorrow = UtcDay::at(MIDDAY + 43_200);
        assert_eq!(schedule.plan(s(40), tomorrow, LONG), Ok(ms(0)));
        assert_eq!((schedule.day, schedule.used), (noon().number + 1, 1));
        // Un jour différent, même antérieur (horloge reculée), remet aussi le compteur à zéro.
        schedule.used = DAILY_BUDGET;
        assert_eq!(schedule.plan(s(50), noon(), LONG), Ok(ms(0)));
        assert_eq!((schedule.day, schedule.used), (noon().number, 1));
    }

    /// Le repos se juge avant le budget : pendant un repos, c'est son reste qui est annoncé.
    #[test]
    fn a_rest_is_announced_before_an_exhausted_budget() {
        let mut schedule =
            Schedule { day: noon().number, used: DAILY_BUDGET, ..Schedule::default() };
        schedule.rest(s(10), Some(s(60)));
        assert_eq!(schedule.plan(s(20), noon(), LONG), limited(s(50)));
        assert_eq!(schedule.plan(s(70), noon(), LONG), limited(s(43_200)));
    }

    fn pace() -> (Pace<TestWall, ManualTimer>, ManualTimer, TestWall) {
        let timer = ManualTimer::default();
        let wall = TestWall::at(MIDDAY);
        (Pace::new(wall.clone(), timer.clone()), timer, wall)
    }

    #[tokio::test]
    async fn acquire_waits_for_its_turn_on_the_injected_timer() {
        let (pace, timer, _wall) = pace();
        drop(pace.acquire(LONG).await.unwrap());
        drop(pace.acquire(LONG).await.unwrap());
        drop(pace.acquire(LONG).await.unwrap());
        // Trois départs : le premier sans attente, les deux autres à 500 ms d'écart.
        assert_eq!(timer.sleeps(), [ms(0), ms(500), ms(500)]);
        assert_eq!(timer.now(), s(1));
        assert_eq!(pace.timer().now(), s(1));
    }

    #[tokio::test]
    async fn at_most_two_reads_are_in_flight_and_a_third_waits_until_its_deadline() {
        let (pace, timer, _wall) = pace();
        let first = pace.acquire(LONG).await.unwrap();
        let second = pace.acquire(LONG).await.unwrap();
        assert_eq!(pace.in_flight.available_permits(), 0);
        // Aucune place : l'appel attend toute son échéance, puis renonce sans rien réserver.
        assert_eq!(pace.acquire(s(7)).await.err(), Some(DoctrineError::Timeout));
        assert_eq!(timer.sleeps(), [ms(0), ms(500), s(7)]);
        assert_eq!(pace.schedule().used, 2);
        // Une place rendue : l'appel suivant passe.
        drop(first);
        assert_eq!(pace.in_flight.available_permits(), 1);
        let fourth = pace.acquire(LONG).await.unwrap();
        assert_eq!(pace.schedule().used, 3);
        drop((second, fourth));
        assert_eq!(pace.in_flight.available_permits(), IN_FLIGHT_MAX);
    }

    #[tokio::test]
    async fn a_turn_that_leaves_nothing_to_the_call_is_a_timeout_and_frees_its_slot() {
        let (pace, timer, _wall) = pace();
        drop(pace.acquire(LONG).await.unwrap());
        // Prochain départ dans 500 ms. Avec 500 ms de reste, l'attente ne laisse rien à l'appel.
        assert_eq!(pace.acquire(ms(500)).await.err(), Some(DoctrineError::Timeout));
        assert_eq!(pace.acquire(ms(501)).await.err(), None);
        assert_eq!(timer.sleeps(), [ms(0), ms(500)]);
        // Un refus rend sa place en vol.
        assert_eq!(pace.in_flight.available_permits(), IN_FLIGHT_MAX);
    }

    /// Le temps passé à attendre une place est retiré de ce qui reste à l'appel : une place
    /// rendue juste avant l'échéance laisse partir l'appel, une place rendue à l'échéance non.
    #[tokio::test]
    async fn the_time_spent_waiting_for_a_slot_counts_against_the_deadline() {
        for (waited, refusal) in [(s(7_199), None), (s(7_200), Some(DoctrineError::Timeout))] {
            // Les attentes d'une heure ou plus ne finissent jamais : la limite de l'appel reste
            // en suspens, c'est la place rendue qui le réveille.
            let timer = ManualTimer::holding(s(3_600));
            let pace = Pace::new(TestWall::at(MIDDAY), timer.clone());
            let first = pace.acquire(LONG).await.unwrap();
            let _second = pace.acquire(LONG).await.unwrap();
            let waiter = pace.acquire(s(7_200));
            let release = async {
                timer.advance(waited);
                drop(first);
            };
            let (third, ()) = tokio::join!(waiter, release);
            assert_eq!(third.err(), refusal, "{waited:?}");
        }
    }

    #[tokio::test]
    async fn a_rest_refuses_acquisitions_without_waiting() {
        let (pace, timer, _wall) = pace();
        assert_eq!(pace.rest(Some(s(120))), s(120));
        let refused = pace.acquire(LONG).await.err();
        assert_eq!(refused, Some(DoctrineError::RateLimited { retry_after: Some(s(120)) }));
        timer.advance(s(119));
        let refused = pace.acquire(LONG).await.err();
        assert_eq!(refused, Some(DoctrineError::RateLimited { retry_after: Some(s(1)) }));
        assert_eq!(timer.sleeps(), Vec::new());
        timer.advance(s(1));
        assert_eq!(pace.acquire(LONG).await.err(), None);
        // Sans durée annoncée : un quart d'heure, compté depuis l'instant du refus.
        assert_eq!(pace.rest(None), s(900));
        assert_eq!(pace.schedule().rest_until, s(120) + s(900));
    }

    /// Un refus 429 reçu pendant qu'un autre appel attend son tour de départ arrête aussi cet
    /// appel : il ne part pas pendant le repos.
    #[tokio::test]
    async fn a_rest_opened_while_a_call_waits_for_its_turn_stops_that_call() {
        // Chaque attente rend la main une fois : le refus a le temps d'arriver.
        let pace = Pace::new(TestWall::at(MIDDAY), ManualTimer::yielding());
        let first = pace.acquire(LONG).await.unwrap();
        // Le second appel attend 500 ms ; le refus du premier arrive pendant cette attente.
        let waiting = pace.acquire(LONG);
        let refusal = async { pace.rest(Some(s(120))) };
        let (second, rest) = tokio::join!(waiting, refusal);
        assert_eq!(rest, s(120));
        // Repos ouvert à l'instant 0, attente finie à 500 ms : il en reste 119,5 s.
        let left = Some(ms(119_500));
        assert_eq!(second.err(), Some(DoctrineError::RateLimited { retry_after: left }));
        // Le départ réservé reste compté au budget ; la place en vol est rendue.
        assert_eq!(pace.schedule().used, 2);
        drop(first);
        assert_eq!(pace.in_flight.available_permits(), IN_FLIGHT_MAX);
    }

    #[tokio::test]
    async fn daily_budget_exhausted_is_local() {
        let (pace, timer, wall) = pace();
        for _ in 0..DAILY_BUDGET {
            drop(pace.acquire(LONG).await.unwrap());
            // Rend la main : sans cela, tokio met en attente un futur qui n'a jamais cédé, et le
            // minuteur de test, toujours prêt, y verrait une échéance dépassée.
            tokio::task::yield_now().await;
        }
        // Dix mille départs : la suite est refusée sans attendre, jusqu'à minuit UTC.
        let sleeps = timer.sleeps().len();
        let refused = pace.acquire(LONG).await.err();
        assert_eq!(refused, Some(DoctrineError::RateLimited { retry_after: Some(s(43_200)) }));
        assert_eq!(timer.sleeps().len(), sleeps);
        // L'horloge murale passe minuit : le budget du jour suivant est entier.
        wall.set(MIDDAY + 43_200);
        assert_eq!(pace.acquire(LONG).await.err(), None);
        assert_eq!(pace.schedule().used, 1);
    }
}
