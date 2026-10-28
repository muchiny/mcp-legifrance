//! Port sortant de l'horloge (02 §5.3).

use chrono::{DateTime, Utc};
use lf_domain::date::CivilDate;

/// Horloge du serveur. Synchrone : aucune E/S.
///
/// Implémentations : `lf-app::clock::SystemClock` (avec `chrono-tz`) et `FixedClock` (doublure
/// de test, feature `test-support`) qui reçoit ses deux valeurs, pour que `lf-application`
/// n'ait pas besoin de `chrono-tz` (R2).
pub trait Clock: Send + Sync + 'static {
    /// Instant courant en UTC.
    fn now_utc(&self) -> DateTime<Utc>;

    /// Date civile Europe/Paris : le défaut de `date` et de `version_date` (D-19, E-05).
    fn today_paris(&self) -> CivilDate;
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::date::CivilDate;
    use pretty_assertions::assert_eq;

    struct Frozen;

    impl Clock for Frozen {
        fn now_utc(&self) -> DateTime<Utc> {
            DateTime::UNIX_EPOCH
        }
        fn today_paris(&self) -> CivilDate {
            CivilDate::new(1970, 1, 1).unwrap()
        }
    }

    /// Le port est synchrone et sert de `&C` partagé entre tâches : `Send + Sync + 'static`.
    fn assert_shareable<T: Clock>(_: &T) {}

    #[test]
    fn clock_is_implementable_and_synchronous() {
        assert_shareable(&Frozen);
        assert_eq!(Frozen.now_utc().timestamp(), 0);
        assert_eq!(Frozen.today_paris().to_string(), "1970-01-01");
    }
}
