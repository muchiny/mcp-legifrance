//! Dates civiles (calendrier grégorien, sans fuseau).
//!
//! Les dates PISTE arrivent en millisecondes epoch à minuit UTC (vérifié sur
//! les fixtures : `consult_juri_constit.json` `dateTexte` = 2025-11-07T00:00Z),
//! d'où la conversion UTC de [`CivilDate::from_unix_millis`] (`citations.py:66-73`).

use std::{fmt, str::FromStr};

use crate::error::{DomainError, echo};

/// Noms de mois français, accentués (décision Q9 : la référence les écrit
/// sans accents, `citations.py:283-286`).
const FR_MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

const MILLIS_PER_DAY: i64 = 86_400_000;

/// Date civile validée, années 1 à 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CivilDate {
    year: u16,
    month: u8,
    day: u8,
}

/// Date de version d'un texte consolidé (`get_act`).
pub type VersionDate = CivilDate;

fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn digit(b: u8) -> Option<u8> {
    b.is_ascii_digit().then(|| b - b'0')
}

impl CivilDate {
    /// Construit une date si elle existe dans le calendrier.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let valid = (1..=9999).contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day);
        valid.then_some(Self { year, month, day })
    }

    /// Lit une date `AAAA-MM-JJ` (espaces autour tolérés).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidDate`] si la forme ou la date est invalide.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        Self::parse_iso(input.trim().as_bytes())
            .ok_or_else(|| DomainError::InvalidDate { input: echo(input) })
    }

    fn parse_iso(bytes: &[u8]) -> Option<Self> {
        let &[y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2] = bytes else {
            return None;
        };
        let year = u16::from(digit(y1)?) * 1000
            + u16::from(digit(y2)?) * 100
            + u16::from(digit(y3)?) * 10
            + u16::from(digit(y4)?);
        let month = digit(m1)? * 10 + digit(m2)?;
        let day = digit(d1)? * 10 + digit(d2)?;
        Self::new(year, month, day)
    }

    /// Jour UTC d'un instant exprimé en millisecondes epoch.
    pub fn from_unix_millis(millis: i64) -> Option<Self> {
        civil_from_days(millis.div_euclid(MILLIS_PER_DAY))
    }

    /// Année.
    pub fn year(self) -> u16 {
        self.year
    }

    /// Mois, 1 à 12.
    pub fn month(self) -> u8 {
        self.month
    }

    /// Jour, 1 à 31.
    pub fn day(self) -> u8 {
        self.day
    }

    /// Date en prose française : `1er janvier 2026`, `7 novembre 2025`.
    ///
    /// Divergence assumée : `1er` pour le premier du mois et mois accentués
    /// (décision Q9, `docs/plan/00-decisions.md` §8) ; la référence écrit
    /// `1 janvier` (`_fr_date`, `citations.py:289-295`).
    pub fn to_french(self) -> String {
        let month = FR_MONTHS[usize::from(self.month - 1)];
        if self.day == 1 {
            format!("1er {month} {}", self.year)
        } else {
            format!("{} {month} {}", self.day, self.year)
        }
    }
}

/// Jours depuis 1970-01-01 vers date civile (algorithme `civil_from_days`
/// de H. Hinnant, <https://howardhinnant.github.io/date_algorithms.html>).
fn civil_from_days(days: i64) -> Option<CivilDate> {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Hinnant : month ∈ [1, 12] et day ∈ [1, 31], les conversions n'échouent jamais.
    let ((year, month), day) =
        u16::try_from(year).ok().zip(u8::try_from(month).ok()).zip(u8::try_from(day).ok())?;
    CivilDate::new(year, month, day)
}

impl fmt::Display for CivilDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for CivilDate {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn d(y: u16, m: u8, day: u8) -> CivilDate {
        CivilDate::new(y, m, day).unwrap()
    }

    /// Inverse indépendant (`days_from_civil`, même source) servant d'oracle.
    fn days_from_civil(date: CivilDate) -> i64 {
        let (m, dd) = (i64::from(date.month()), i64::from(date.day()));
        let y = i64::from(date.year()) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + dd - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    #[test]
    fn new_validates_calendar() {
        assert!(CivilDate::new(2024, 2, 29).is_some());
        assert!(CivilDate::new(2023, 2, 29).is_none());
        assert!(CivilDate::new(2000, 2, 29).is_some());
        assert!(CivilDate::new(1900, 2, 29).is_none());
        assert!(CivilDate::new(2023, 2, 28).is_some());
        assert!(CivilDate::new(2023, 4, 31).is_none());
        assert!(CivilDate::new(2023, 4, 30).is_some());
        for month in [6, 9, 11] {
            assert!(CivilDate::new(2023, month, 31).is_none());
            assert!(CivilDate::new(2023, month, 30).is_some());
        }
        assert!(CivilDate::new(2023, 12, 31).is_some());
        assert!(CivilDate::new(2023, 1, 0).is_none());
        assert!(CivilDate::new(2023, 0, 1).is_none());
        assert!(CivilDate::new(2023, 13, 1).is_none());
        assert!(CivilDate::new(0, 1, 1).is_none());
        assert!(CivilDate::new(1, 1, 1).is_some());
        assert!(CivilDate::new(9999, 12, 31).is_some());
        assert!(CivilDate::new(10_000, 1, 1).is_none());
    }

    #[test]
    fn parse_reads_iso_dates() {
        assert_eq!(CivilDate::parse("2023-09-08").unwrap(), d(2023, 9, 8));
        assert_eq!(" 2999-01-01 ".parse::<CivilDate>().unwrap(), d(2999, 1, 1));
    }

    #[test]
    fn parse_rejects_other_shapes() {
        for bad in [
            "",
            "2023-9-08",
            "2023-09-8",
            "20230908",
            "2023/09/08",
            "08-09-2023",
            "2023-02-30",
            "2023-13-01",
            "0000-01-01",
            "2023-0a-01",
            "2023-01-0a",
            "a023-01-01",
            "2a23-01-01",
            "20a3-01-01",
            "202a-01-01",
            "2023-a1-01",
            "2023-01-a1",
            "+2023-01-01",
            "2023-01-01T00:00",
        ] {
            assert_eq!(
                CivilDate::parse(bad).unwrap_err(),
                DomainError::InvalidDate { input: bad.into() },
                "{bad:?}"
            );
        }
    }

    #[test]
    fn from_unix_millis_matches_fixtures() {
        // consult_juri_constit.json, consult_loda.json, consult_article.json
        assert_eq!(CivilDate::from_unix_millis(1_762_473_600_000), Some(d(2025, 11, 7)));
        assert_eq!(CivilDate::from_unix_millis(1_475_884_800_000), Some(d(2016, 10, 8)));
        assert_eq!(CivilDate::from_unix_millis(17_193_600_000), Some(d(1970, 7, 19)));
        assert_eq!(CivilDate::from_unix_millis(32_472_144_000_000), Some(d(2999, 1, 1)));
    }

    #[test]
    fn from_unix_millis_edges() {
        assert_eq!(CivilDate::from_unix_millis(0), Some(d(1970, 1, 1)));
        assert_eq!(CivilDate::from_unix_millis(-1), Some(d(1969, 12, 31)));
        assert_eq!(CivilDate::from_unix_millis(MILLIS_PER_DAY - 1), Some(d(1970, 1, 1)));
        assert_eq!(CivilDate::from_unix_millis(951_782_400_000), Some(d(2000, 2, 29)));
        assert_eq!(CivilDate::from_unix_millis(951_868_800_000), Some(d(2000, 3, 1)));
        assert_eq!(CivilDate::from_unix_millis(i64::MIN), None);
        assert_eq!(CivilDate::from_unix_millis(i64::MAX), None);
        assert_eq!(CivilDate::from_unix_millis(-62_135_596_800_001), None); // 0000-12-31
        assert_eq!(CivilDate::from_unix_millis(-62_135_596_800_000), Some(d(1, 1, 1)));
        assert_eq!(CivilDate::from_unix_millis(253_402_300_799_999), Some(d(9999, 12, 31)));
        assert_eq!(CivilDate::from_unix_millis(253_402_300_800_000), None); // 10000-01-01
    }

    #[test]
    fn getters_and_display() {
        let date = d(2025, 3, 7);
        assert_eq!((date.year(), date.month(), date.day()), (2025, 3, 7));
        assert_eq!(date.to_string(), "2025-03-07");
        assert_eq!(d(812, 1, 2).to_string(), "0812-01-02");
    }

    #[test]
    fn french_prose() {
        assert_eq!(d(2025, 11, 7).to_french(), "7 novembre 2025");
        assert_eq!(d(2026, 1, 1).to_french(), "1er janvier 2026");
        assert_eq!(d(2026, 2, 2).to_french(), "2 février 2026");
        assert_eq!(d(2026, 8, 15).to_french(), "15 août 2026");
        assert_eq!(d(2026, 12, 31).to_french(), "31 décembre 2026");
    }

    #[test]
    fn french_prose_covers_all_twelve_months() {
        let prose: Vec<String> = (1..=12).map(|m| d(2026, m, 15).to_french()).collect();
        assert_eq!(
            prose,
            [
                "15 janvier 2026",
                "15 février 2026",
                "15 mars 2026",
                "15 avril 2026",
                "15 mai 2026",
                "15 juin 2026",
                "15 juillet 2026",
                "15 août 2026",
                "15 septembre 2026",
                "15 octobre 2026",
                "15 novembre 2026",
                "15 décembre 2026",
            ]
        );
        assert_eq!(d(2026, 3, 1).to_french(), "1er mars 2026");
    }

    proptest! {
        #[test]
        fn ordering_is_chronological(
            (ya, ma, da) in (1u16..=9999, 1u8..=12, 1u8..=28),
            (yb, mb, db) in (1u16..=9999, 1u8..=12, 1u8..=28),
        ) {
            let (a, b) = (d(ya, ma, da), d(yb, mb, db));
            prop_assert_eq!(a.cmp(&b), days_from_civil(a).cmp(&days_from_civil(b)));
        }

        #[test]
        fn display_parse_roundtrip(y in 1u16..=9999, m in 1u8..=12, day in 1u8..=28) {
            let date = d(y, m, day);
            prop_assert_eq!(CivilDate::parse(&date.to_string()), Ok(date));
        }

        #[test]
        fn millis_in_range_always_give_a_date(
            millis in -62_135_596_800_000_i64..253_402_300_800_000_i64,
        ) {
            let date = CivilDate::from_unix_millis(millis).map(days_from_civil);
            let day = millis.div_euclid(MILLIS_PER_DAY);
            prop_assert_eq!(date, Some(day));
        }

        #[test]
        fn civil_roundtrips_through_oracle(y in 1u16..=9999, m in 1u8..=12, day in 1u8..=31) {
            if let Some(date) = CivilDate::new(y, m, day) {
                let days = days_from_civil(date);
                prop_assert_eq!(civil_from_days(days), Some(date));
                prop_assert_eq!(CivilDate::from_unix_millis(days * MILLIS_PER_DAY + 3_600_000), Some(date));
                prop_assert_eq!(CivilDate::parse(&date.to_string()).unwrap(), date);
            }
        }

        #[test]
        fn parse_never_panics(s in "\\PC*") {
            let _ = CivilDate::parse(&s);
        }
    }
}
