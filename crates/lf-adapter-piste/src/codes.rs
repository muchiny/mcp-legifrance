//! Table des codes de l'adapter (M6, 04 §7.1, §12.3) : le résultat de `/list/code`, mis en mémoire
//! pour 7 jours.
//!
//! La table est **unique** pour le processus (`list_codes` et, en M7, les complétions de
//! `legifrance://code/{code}/article/{num}` la lisent) et **paresseuse** : rien n'est chargé à la
//! construction, ni au démarrage (00 §6) ; le premier appel qui la trouve absente ou périmée la
//! charge. Le chargement est **mono-vol** : un `Mutex` asynchrone est tenu pendant toute la requête,
//! les appels concurrents attendent donc le résultat du premier au lieu d'envoyer chacun la leur.
//!
//! - **Fraîche** : moins de 7 jours (`TTL`, borne stricte comme celle du mémo de `typePagination`).
//! - **Échec d'un rafraîchissement** : la table précédente est gardée sept jours de plus
//!   (`STALE_GRACE`), un `warn!` sans détail le signale ; au-delà, l'erreur du chargement sort telle
//!   quelle. Sans table précédente, un échec ne laisse rien en mémoire : l'appel suivant recharge.
//! - **Attente du verrou** : bornée par l'échéance de l'appelant, comme celle du jeton
//!   (`auth::TokenManager`) : un appelant qui attend derrière un chargement trop long reçoit
//!   [`CorpusError::Timeout`] sans rien modifier.
//!
//! Le verrou n'est jamais tenu hors de `get_or_load`. Aucune valeur d'une réponse n'est journalisée.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lf_application::ports::CorpusError;
use lf_domain::entity::CodeListing;
use tokio::sync::Mutex;

use crate::time::{MonoClock, race};

/// Durée de vie de la table : 7 jours (04 §12.3), expirée dès qu'elle l'atteint.
const TTL: Duration = Duration::from_secs(7 * 86_400);

/// Sursis après l'expiration : la table périmée sert encore tant qu'un rafraîchissement échoue,
/// jusqu'à 7 jours de plus (04 §12.3).
const STALE_GRACE: Duration = Duration::from_secs(7 * 86_400);

/// Table chargée et l'instant de son chargement.
struct Loaded {
    listings: Arc<[CodeListing]>,
    /// Lecture de l'horloge juste avant le chargement : les attentes de réessai ne repoussent pas
    /// l'expiration.
    loaded_at: Duration,
}

/// Table des codes, chargée à la demande.
///
/// Sans `Debug` : elle porte des titres issus d'une réponse de l'API.
pub(crate) struct CodeTable<K: MonoClock> {
    clock: K,
    slot: Mutex<Option<Loaded>>,
}

impl<K: MonoClock> CodeTable<K> {
    /// Crée une table vide lisant le temps sur `clock` ; aucune E/S.
    pub(crate) fn new(clock: K) -> Self {
        Self { clock, slot: Mutex::new(None) }
    }

    /// Table des codes, chargée par `load` si elle est absente ou périmée.
    ///
    /// `wait` borne l'attente du verrou (`sleeper.sleep(deadline.remaining(..))`) ; le verrou est
    /// interrogé en premier : un verrou libre est pris même si l'échéance est passée. `load` ne
    /// s'exécute qu'une fois à la fois, sous le verrou.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Timeout`] si le verrou n'est pas pris avant l'échéance (rien n'est
    /// modifié) ; sinon l'erreur de `load`, quand aucune table de moins de 14 jours ne peut la
    /// remplacer.
    pub(crate) async fn get_or_load<F, Fut>(
        &self,
        wait: Pin<&mut (dyn Future<Output = ()> + Send)>,
        load: F,
    ) -> Result<Arc<[CodeListing]>, CorpusError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<CodeListing>, CorpusError>>,
    {
        let mut slot = race(self.slot.lock(), wait).await.ok_or(CorpusError::Timeout)?;
        let now = self.clock.now();
        if let Some(loaded) = slot.as_ref()
            && age(loaded, now) < TTL
        {
            return Ok(Arc::clone(&loaded.listings));
        }
        match load().await {
            Ok(listings) => {
                let listings: Arc<[CodeListing]> = listings.into();
                *slot = Some(Loaded { listings: Arc::clone(&listings), loaded_at: now });
                Ok(listings)
            }
            Err(error) => match slot.as_ref() {
                Some(previous) if age(previous, now) < TTL.saturating_add(STALE_GRACE) => {
                    tracing::warn!(
                        age_days = age(previous, now).as_secs() / 86_400,
                        "code table refresh failed: serving the previous table"
                    );
                    Ok(Arc::clone(&previous.listings))
                }
                _ => Err(error),
            },
        }
    }
}

/// Âge de la table `loaded` à l'instant `now`.
fn age(loaded: &Loaded, now: Duration) -> Duration {
    now.saturating_sub(loaded.loaded_at)
}

#[cfg(test)]
mod tests {
    use std::future::{pending, ready};
    use std::pin::pin;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use lf_domain::id::LegiTextId;
    use tokio::sync::Notify;
    use tracing::Level;

    use super::*;
    use crate::testing::capture_at;
    use crate::time::ManualClock;

    const JUST_UNDER_A_WEEK: Duration = Duration::new(7 * 86_400 - 1, 999_999_999);

    fn listing(n: u64) -> CodeListing {
        CodeListing {
            id: LegiTextId::parse(&format!("LEGITEXT{n:012}")).unwrap(),
            title: format!("Code {n}"),
            etat: None,
        }
    }

    fn table() -> (CodeTable<ManualClock>, ManualClock) {
        let clock = ManualClock::new();
        (CodeTable::new(clock.clone()), clock)
    }

    fn upstream() -> CorpusError {
        CorpusError::Upstream { status: Some(503), retryable: true, detail: "down".to_owned() }
    }

    /// Une lecture de la table dont le chargement rend `outcome` (`n` entrées ou une erreur) et
    /// est compté dans `loads`. Toutes les lectures de ces tests passent par ce chargeur (un seul
    /// type de fermeture) : la couverture du générique `get_or_load` se mesure par instance.
    async fn read_with(
        table: &CodeTable<ManualClock>,
        loads: &AtomicUsize,
        wait: Pin<&mut (dyn Future<Output = ()> + Send)>,
        outcome: Result<u64, CorpusError>,
    ) -> Result<Arc<[CodeListing]>, CorpusError> {
        table
            .get_or_load(wait, || async {
                loads.fetch_add(1, Ordering::SeqCst);
                outcome.map(|n| (1..=n).map(listing).collect())
            })
            .await
    }

    /// Une lecture sans limite d'attente qui charge `n` entrées.
    async fn read(
        table: &CodeTable<ManualClock>,
        loads: &AtomicUsize,
        n: u64,
    ) -> Result<Arc<[CodeListing]>, CorpusError> {
        let mut wait = pin!(pending::<()>());
        read_with(table, loads, wait.as_mut(), Ok(n)).await
    }

    /// Une lecture dont le chargement échoue.
    async fn read_failing(
        table: &CodeTable<ManualClock>,
        loads: &AtomicUsize,
    ) -> Result<Arc<[CodeListing]>, CorpusError> {
        let mut wait = pin!(pending::<()>());
        read_with(table, loads, wait.as_mut(), Err(upstream())).await
    }

    #[tokio::test]
    async fn nothing_is_loaded_before_the_first_read() {
        let (table, _clock) = table();
        assert!(table.slot.lock().await.is_none());
    }

    #[tokio::test]
    async fn the_first_read_loads_and_the_next_ones_reuse_the_table() {
        let (table, _clock) = table();
        let loads = AtomicUsize::new(0);
        let first = read(&table, &loads, 3).await.unwrap();
        let second = read(&table, &loads, 99).await.unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(first.len(), 3);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[tokio::test]
    async fn the_table_expires_at_exactly_seven_days() {
        assert_eq!(TTL, Duration::from_secs(604_800));
        let (table, clock) = table();
        let loads = AtomicUsize::new(0);
        read(&table, &loads, 1).await.unwrap();
        clock.advance(JUST_UNDER_A_WEEK);
        read(&table, &loads, 1).await.unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        clock.advance(Duration::from_nanos(1));
        let renewed = read(&table, &loads, 2).await.unwrap();
        assert_eq!((loads.load(Ordering::SeqCst), renewed.len()), (2, 2));
        // L'âge repart de zéro au rechargement.
        clock.advance(JUST_UNDER_A_WEEK);
        read(&table, &loads, 2).await.unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_first_reads_share_a_single_load() {
        let (table, _clock) = table();
        let loads = AtomicUsize::new(0);
        let slow = |n: u64| {
            let (table, loads) = (&table, &loads);
            async move {
                let mut wait = pin!(pending::<()>());
                table
                    .get_or_load(wait.as_mut(), || async move {
                        loads.fetch_add(1, Ordering::SeqCst);
                        tokio::task::yield_now().await;
                        Ok((1..=n).map(listing).collect())
                    })
                    .await
            }
        };
        let (a, b, c) = tokio::join!(slow(2), slow(5), slow(7));
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        let sizes = [a.unwrap().len(), b.unwrap().len(), c.unwrap().len()];
        assert_eq!(sizes, [2, 2, 2]);
    }

    #[tokio::test]
    async fn a_failed_first_load_leaves_nothing_and_the_next_read_loads_again() {
        let (table, _clock) = table();
        let loads = AtomicUsize::new(0);
        assert_eq!(read_failing(&table, &loads).await.unwrap_err(), upstream());
        assert!(table.slot.lock().await.is_none());
        let loaded = read(&table, &loads, 4).await.unwrap();
        assert_eq!((loads.load(Ordering::SeqCst), loaded.len()), (2, 4));
    }

    #[tokio::test]
    async fn a_failed_refresh_serves_the_previous_table_for_seven_more_days() {
        let (capture, _guard) = capture_at(Level::WARN);
        let (table, clock) = table();
        let loads = AtomicUsize::new(0);
        let first = read(&table, &loads, 3).await.unwrap();
        clock.advance(TTL);
        let stale = read_failing(&table, &loads).await.unwrap();
        assert!(Arc::ptr_eq(&first, &stale));
        // 14 jours moins un instant : encore servie.
        clock.advance(JUST_UNDER_A_WEEK);
        let still = read_failing(&table, &loads).await.unwrap();
        assert!(Arc::ptr_eq(&first, &still));
        assert_eq!(loads.load(Ordering::SeqCst), 3);
        let log = capture.text();
        assert!(log.contains("code table refresh failed"), "{log}");
        assert!(!log.contains("down"), "{log}");
        // 14 jours : l'erreur sort.
        clock.advance(Duration::from_nanos(1));
        assert_eq!(read_failing(&table, &loads).await.unwrap_err(), upstream());
        // La table périmée est gardée : un chargement réussi la remplace.
        let renewed = read(&table, &loads, 1).await.unwrap();
        assert_eq!(renewed.len(), 1);
    }

    #[tokio::test]
    async fn a_waiter_gives_up_at_its_deadline_without_touching_the_table() {
        let (table, _clock) = table();
        let table = Arc::new(table);
        let release = Arc::new(Notify::new());
        let holder = {
            let (table, release) = (Arc::clone(&table), Arc::clone(&release));
            tokio::spawn(async move {
                let mut wait = pin!(pending::<()>());
                table
                    .get_or_load(wait.as_mut(), || async move {
                        release.notified().await;
                        Ok(vec![listing(1)])
                    })
                    .await
            })
        };
        // Laisse le premier appel prendre le verrou et attendre son chargement.
        tokio::task::yield_now().await;
        let mut expired = pin!(ready(()));
        let waiter_loads = AtomicUsize::new(0);
        let waiter = read_with(&table, &waiter_loads, expired.as_mut(), Ok(9)).await;
        assert_eq!(waiter.unwrap_err(), CorpusError::Timeout);
        assert_eq!(waiter_loads.load(Ordering::SeqCst), 0);
        // Le chargement en cours n'est pas touché : il aboutit et la table le garde.
        release.notify_one();
        assert_eq!(holder.await.unwrap().unwrap().len(), 1);
        assert_eq!(table.slot.lock().await.as_ref().map(|loaded| loaded.listings.len()), Some(1));
    }

    #[tokio::test]
    async fn a_free_lock_is_taken_even_when_the_deadline_has_passed() {
        let (table, _clock) = table();
        let loads = AtomicUsize::new(0);
        let mut expired = pin!(ready(()));
        let loaded = read_with(&table, &loads, expired.as_mut(), Ok(2)).await.unwrap();
        assert_eq!((loads.load(Ordering::SeqCst), loaded.len()), (1, 2));
    }
}
