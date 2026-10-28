//! Échéance d'un appel d'outil, publiée en variable locale de tâche.
//!
//! `Audited` (M3) pose la garde dure : il publie `Deadline::after(LF_TOOL_DEADLINE_S)` par
//! [`Deadline::scope`] puis borne l'appel par `tokio::time::timeout`. Ce module ne pose
//! aucun délai lui-même. Un adapter lit l'échéance comme une durée restante
//! ([`Deadline::current_or`]) et la convertit dans sa propre horloge (04 §5.3).
//!
//! La valeur n'est visible que dans la tâche qui porte le `scope` et ses sous-futures
//! (`join!`, `select!`) : une tâche lancée par `tokio::spawn` ne la voit pas (02 §3.2,
//! piège 5) et retombe sur la valeur par défaut. La feature `rt` de `tokio` ne sert qu'à
//! `task_local!` (R2) ; `time` fournit `Instant`, suspendu dans les tests.

use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

tokio::task_local! {
    static CURRENT: Deadline;
}

/// Repli d'une échéance démesurée : environ 30 ans, la valeur de `tokio::time::timeout` quand
/// `Instant + durée` déborde (`Instant::far_future`, privé à `tokio` 1.53).
const FAR_FUTURE: Duration = Duration::from_secs(86_400 * 365 * 30);

/// Instant limite d'un appel d'outil, sur l'horloge de `tokio` (suspendue en test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline(Instant);

impl Deadline {
    /// Échéance située à `budget` d'ici. `LF_TOOL_DEADLINE_S` vaut au plus 600 s (borne de
    /// `lf-app`), mais un budget démesuré ne panique pas : l'échéance sature à un instant
    /// lointain, comme la garde de `tokio::time::timeout` qui l'accompagne (I-1).
    #[must_use]
    pub fn after(budget: Duration) -> Self {
        let now = Instant::now();
        // `now + FAR_FUTURE` ne déborde pas : l'horloge monotone part du démarrage de la machine.
        Self(now.checked_add(budget).unwrap_or_else(|| now + FAR_FUTURE))
    }

    /// Temps restant avant l'échéance, zéro une fois celle-ci passée.
    #[must_use]
    pub fn remaining(self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }

    /// Exécute `future` avec cette échéance publiée comme [`Deadline::current`].
    pub async fn scope<F: Future>(self, future: F) -> F::Output {
        CURRENT.scope(self, future).await
    }

    /// Échéance publiée par le `scope` englobant, ou `None` hors de tout `scope`.
    #[must_use]
    pub fn current() -> Option<Self> {
        CURRENT.try_with(|deadline| *deadline).ok()
    }

    /// Temps restant de l'échéance publiée, ou `default` hors de tout `scope`.
    #[must_use]
    pub fn current_or(default: Duration) -> Duration {
        Self::current().map_or(default, Self::remaining)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::advance;

    use super::Deadline;

    const DEFAULT: Duration = Duration::from_secs(60);

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[tokio::test(start_paused = true)]
    async fn published_deadline_reads_as_remaining_time() {
        let deadline = Deadline::after(secs(10));
        let seen = deadline
            .scope(async {
                let first = Deadline::current_or(DEFAULT);
                advance(secs(3)).await;
                (first, Deadline::current_or(DEFAULT), Deadline::current())
            })
            .await;
        assert_eq!(seen, (secs(10), secs(7), Some(deadline)));
    }

    #[tokio::test(start_paused = true)]
    async fn outside_any_scope_the_default_applies() {
        assert_eq!((Deadline::current(), Deadline::current_or(DEFAULT)), (None, DEFAULT));
    }

    #[tokio::test(start_paused = true)]
    async fn past_deadline_leaves_nothing() {
        let deadline = Deadline::after(secs(1));
        advance(secs(2)).await;
        assert_eq!(deadline.remaining(), Duration::ZERO);
        assert_eq!(deadline.scope(async { Deadline::current_or(DEFAULT) }).await, Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn joined_sub_futures_share_the_deadline() {
        let read = || async { Deadline::current_or(DEFAULT) };
        let both = Deadline::after(secs(5)).scope(async { tokio::join!(read(), read()) }).await;
        assert_eq!(both, (secs(5), secs(5)));
    }

    #[tokio::test(start_paused = true)]
    async fn an_oversized_budget_saturates_instead_of_panicking() {
        // `Instant + Duration::MAX` déborderait : l'échéance retombe sur un instant lointain
        // (environ 30 ans), comme la garde de `tokio::time::timeout`.
        let far = Duration::from_secs(86_400 * 365 * 30);
        for budget in [Duration::MAX, Duration::from_secs(u64::MAX / 2)] {
            let deadline = Deadline::after(budget);
            assert_eq!(deadline.remaining(), far, "{budget:?}");
            assert_eq!(deadline.scope(async { Deadline::current_or(DEFAULT) }).await, far);
        }
        // Un budget ordinaire n'est pas touché, même au-delà de la borne de 600 s de `lf-app`.
        assert_eq!(Deadline::after(secs(3_600)).remaining(), secs(3_600));
    }

    #[tokio::test(start_paused = true)]
    async fn spawned_task_does_not_see_the_deadline() {
        let spawned = Deadline::after(secs(5))
            .scope(async { tokio::spawn(async { Deadline::current_or(DEFAULT) }).await })
            .await;
        assert_eq!(spawned.unwrap(), DEFAULT);
    }
}
