//! `SystemClock` : implémentation du port `Clock` sur l'heure murale. Seul lieu où l'horloge
//! système est lue (`clippy.toml`, `disallowed-methods`).

#![allow(
    dead_code,
    reason = "consommé par la racine de composition (V2-04, `wiring::run`) ; retirer à ce moment"
)]

use chrono::{DateTime, Datelike, Offset, TimeZone, Utc};
use lf_application::ports::Clock;
use lf_domain::date::{CivilDate, VersionDate};

/// Horloge de production : instant UTC de la machine, date civile lue à Paris.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    #[expect(
        clippy::disallowed_methods,
        reason = "seul lieu où l'horloge murale est lue ; tout le reste passe par le port `Clock` (clippy.toml, T-M3-10)"
    )]
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn today_paris(&self) -> CivilDate {
        paris_date(self.now_utc())
    }
}

/// Date civile Europe/Paris de l'instant `at` (D-19).
///
/// Elle vient de la base IANA de `chrono-tz`, jamais de `TZ` ni du fuseau de la machine : la
/// date par défaut de `date` et de `version_date` ne dépend pas de l'environnement (PT7).
///
/// Hors des années 1 à 9999 que connaît le domaine (jamais avec une horloge réelle), la date est
/// ramenée à la borne de [`VersionDate`] la plus proche plutôt que de paniquer.
pub(crate) fn paris_date(at: DateTime<Utc>) -> CivilDate {
    // `DateTime::with_timezone(..).date_naive()` panique aux bornes de chrono (`MAX_UTC` plus un
    // décalage) : on ajoute le décalage de Paris à la main, en retombant sur l'instant UTC.
    let utc = at.naive_utc();
    let offset = chrono_tz::Europe::Paris.offset_from_utc_datetime(&utc).fix();
    let day = utc.checked_add_offset(offset).unwrap_or(utc).date();
    let exact = u16::try_from(day.year()).ok().and_then(|year| {
        let month = u8::try_from(day.month()).ok()?;
        let day = u8::try_from(day.day()).ok()?;
        CivilDate::new(year, month, day)
    });
    exact.unwrap_or_else(|| {
        if day.year() < 1 { VersionDate::MIN.as_civil() } else { VersionDate::MAX.as_civil() }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn civil(y: u16, m: u8, d: u8) -> CivilDate {
        CivilDate::new(y, m, d).unwrap()
    }

    /// 22:30 UTC en été = 00:30 le lendemain à Paris (UTC+2) : la date UTC ne convient pas (D-19).
    #[test]
    fn today_is_paris_date_at_2230_utc_in_summer() {
        assert_eq!(paris_date(utc(2026, 7, 14, 22, 30)), civil(2026, 7, 15));
    }

    /// 23:30 UTC en hiver = 00:30 le lendemain à Paris (UTC+1).
    #[test]
    fn today_paris_in_winter() {
        assert_eq!(paris_date(utc(2026, 1, 14, 23, 30)), civil(2026, 1, 15));
        assert_eq!(paris_date(utc(2026, 1, 14, 22, 30)), civil(2026, 1, 14));
    }

    /// Jours de bascule : passage à l'heure d'été le 2026-03-29 (01:00Z), retour à l'heure
    /// d'hiver le 2026-10-25 (01:00Z).
    #[test]
    fn dst_transition_days() {
        assert_eq!(paris_date(utc(2026, 3, 29, 21, 30)), civil(2026, 3, 29));
        assert_eq!(paris_date(utc(2026, 3, 29, 22, 30)), civil(2026, 3, 30));
        assert_eq!(paris_date(utc(2026, 10, 25, 21, 30)), civil(2026, 10, 25));
        assert_eq!(paris_date(utc(2026, 10, 25, 22, 30)), civil(2026, 10, 25));
        assert_eq!(paris_date(utc(2026, 10, 25, 23, 30)), civil(2026, 10, 26));
    }

    /// Hors des années 1 à 9999 du domaine (jamais avec une horloge réelle), la date est ramenée
    /// à la borne de `VersionDate` la plus proche, sans panique.
    #[test]
    fn paris_date_saturates_outside_the_calendar() {
        assert_eq!(paris_date(DateTime::<Utc>::MIN_UTC), VersionDate::MIN.as_civil());
        assert_eq!(paris_date(DateTime::<Utc>::MAX_UTC), VersionDate::MAX.as_civil());
    }

    fn paris_wall_date(at: DateTime<Utc>) -> String {
        at.with_timezone(&chrono_tz::Europe::Paris).format("%Y-%m-%d").to_string()
    }

    /// `today_paris` est la date Paris de l'instant de `now_utc` (encadrée pour tolérer minuit).
    #[test]
    fn today_paris_matches_paris_wall_date() {
        let clock = SystemClock;
        let before = paris_wall_date(clock.now_utc());
        let got = clock.today_paris().to_string();
        let after = paris_wall_date(clock.now_utc());
        assert!(got == before || got == after, "{got} hors de [{before}, {after}]");
    }

    #[test]
    fn now_utc_reads_the_wall_clock() {
        assert!(SystemClock.now_utc() > utc(2026, 1, 1, 0, 0));
    }

    /// PT7 : `TZ=Pacific/Kiritimati` (UTC+14) est sans effet sur la date par défaut. Le test
    /// relance ce binaire de test avec `TZ`, car modifier l'environnement du processus courant
    /// est `unsafe` (édition 2024) et `unsafe_code` est interdit.
    ///
    /// Kiritimati ne date autrement qu'à Paris entre 10 h et 23 h UTC environ. `Pacific/Pago_Pago`
    /// (UTC-11) couvre le reste de la journée : à toute heure, au moins un des deux fuseaux donne
    /// une date locale différente de celle de Paris, et une horloge qui lirait `TZ` serait prise.
    #[test]
    fn tz_env_does_not_change_today() {
        let exe = std::env::current_exe().unwrap();
        for tz in ["Pacific/Kiritimati", "Pacific/Pago_Pago"] {
            let out = Command::new(&exe)
                .args(["--exact", "clock::tests::today_paris_matches_paris_wall_date"])
                .env("TZ", tz)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "TZ={tz}\n{stdout}\n{stderr}");
            assert!(
                stdout.contains("1 passed"),
                "TZ={tz} : le test fils n'a pas tourné : {stdout}"
            );
        }
    }
}
