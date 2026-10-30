//! Dates civiles (calendrier grégorien, sans fuseau).
//!
//! Les dates PISTE arrivent en millisecondes epoch à minuit UTC (vérifié sur
//! les fixtures : `consult_juri_constit.json` `dateTexte` = 2025-11-07T00:00Z),
//! d'où la conversion UTC de [`CivilDate::from_unix_millis`] (`citations.py:66-73`).
//!
//! # Trois niveaux de dates
//!
//! - [`CivilDate`] : toute date juridique, années 1 à 9999 ; une entité peut porter
//!   `2999-01-01` jusqu'au passage par [`open_ended`].
//! - [`open_ended`] : toute date à partir de [`SENTINEL_FLOOR`] (2222-01-01) est une sentinelle
//!   de l'API (`2999-01-01`, `2222-02-22`) et devient `None`.
//! - [`VersionDate`] : date de version demandée par l'utilisateur, lue strictement (ni espace,
//!   ni autre format) et bornée à `1500-01-01..=2221-12-31` (bornes provisoires, V-89), donc
//!   jamais sentinelle ; [`DateRange`] : période fermée de deux [`VersionDate`].
//!
//! La conversion epoch ms couvre les epochs négatifs (1804-03-21 = `-5231606400000`, C3).

use std::{fmt, str::FromStr};

use crate::error::{DomainError, echo};

/// Noms de mois français, accentués (décision Q16 : la référence les écrit
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDate {
    year: u16,
    month: u8,
    day: u8,
}

/// Première date de la zone sentinelle, 2222-01-01 : à partir de cette date, l'API rend `null`
/// pour un champ `…Computed` (00 §4).
pub const SENTINEL_FLOOR: CivilDate = CivilDate { year: 2222, month: 1, day: 1 };

/// Écarte les sentinelles de l'API : toute date à partir de [`SENTINEL_FLOOR`] devient `None`.
///
/// Le sens de `None` dépend du champ (03 §6.2) : date de fin, en vigueur sans date de fin ;
/// date de début, date d'entrée en vigueur non déterminée ; date de publication ou du texte,
/// inconnue. Le mapper l'applique à chaque champ de date d'entité.
pub fn open_ended(date: Option<CivilDate>) -> Option<CivilDate> {
    date.filter(|d| *d < SENTINEL_FLOOR)
}

/// Date de version d'un texte consolidé (`get_act`), demandée par l'utilisateur.
///
/// Invariant : `MIN` ≤ date ≤ `MAX`, donc jamais dans la zone sentinelle. Bornes provisoires
/// (03 §6.1, NON VÉRIFIÉ [V-89 · version-date-bornes]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionDate(CivilDate);

impl VersionDate {
    /// Première date acceptée, 1500-01-01.
    pub const MIN: Self = Self(CivilDate { year: 1500, month: 1, day: 1 });
    /// Dernière date acceptée, 2221-12-31 (la veille de [`SENTINEL_FLOOR`]).
    pub const MAX: Self = Self(CivilDate { year: 2221, month: 12, day: 31 });

    /// Lit exactement `AAAA-MM-JJ`, dans la plage `MIN..=MAX`. Corrige D-22 : « 07/10/2016 »
    /// ou « 2016-13-45 » n'atteignent plus l'API.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidDate`] si la forme, la date ou la plage est invalide (espaces
    /// compris : aucun rognage).
    ///
    /// ```
    /// use lf_domain::date::VersionDate;
    ///
    /// let date = VersionDate::parse("2016-10-07")?;
    /// assert_eq!(date.to_string(), "2016-10-07");
    /// // Autre format, date impossible, espaces, zone sentinelle : refusés.
    /// for bad in ["07/10/2016", "2016-13-45", " 2016-10-07", "2999-01-01"] {
    ///     assert_eq!(
    ///         VersionDate::parse(bad).map_err(|e| e.to_string()),
    ///         Err(format!("invalid date {bad:?}: expected YYYY-MM-DD"))
    ///     );
    /// }
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        CivilDate::parse_iso(input.as_bytes())
            .and_then(Self::within)
            .ok_or_else(|| DomainError::InvalidDate { input: echo(input) })
    }

    /// La même date, vue comme [`CivilDate`].
    pub fn as_civil(self) -> CivilDate {
        self.0
    }

    fn within(date: CivilDate) -> Option<Self> {
        (Self::MIN.0..=Self::MAX.0).contains(&date).then_some(Self(date))
    }
}

impl TryFrom<CivilDate> for VersionDate {
    type Error = DomainError;

    fn try_from(date: CivilDate) -> Result<Self, Self::Error> {
        Self::within(date).ok_or_else(|| DomainError::InvalidDate { input: date.to_string() })
    }
}

impl From<VersionDate> for CivilDate {
    fn from(date: VersionDate) -> Self {
        date.0
    }
}

impl fmt::Display for VersionDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for VersionDate {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Période fermée de deux [`VersionDate`] (invariant : `start() <= end()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateRange {
    from: VersionDate,
    to: VersionDate,
}

impl DateRange {
    /// Aucune borne : `Ok(None)`. Une seule borne : l'autre prend l'extrémité de la plage
    /// ([`VersionDate::MIN`] ou [`VersionDate::MAX`]), pour que l'adapter envoie toujours une
    /// période fermée et que la clé de cache soit unique (03 §6.1).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvertedDateRange`] si `from` est postérieure à `to`.
    pub fn new(
        from: Option<VersionDate>,
        to: Option<VersionDate>,
    ) -> Result<Option<Self>, DomainError> {
        Self::spanning(from, to)
    }

    /// Corps de [`DateRange::new`] : la décision vit ici, car cargo-mutants ne mute jamais
    /// une fonction nommée `new` (`CONTRIBUTING.md`).
    fn spanning(
        from: Option<VersionDate>,
        to: Option<VersionDate>,
    ) -> Result<Option<Self>, DomainError> {
        if from.is_none() && to.is_none() {
            return Ok(None);
        }
        Self::closed(from.unwrap_or(VersionDate::MIN), to.unwrap_or(VersionDate::MAX)).map(Some)
    }

    /// Le contrôle vit ici et non dans `new`, que cargo-mutants ne mute jamais.
    fn closed(from: VersionDate, to: VersionDate) -> Result<Self, DomainError> {
        if from > to {
            return Err(DomainError::InvertedDateRange { from, to });
        }
        Ok(Self { from, to })
    }

    /// Début de la période (inclus).
    pub fn start(self) -> VersionDate {
        self.from
    }

    /// Fin de la période (incluse).
    pub fn end(self) -> VersionDate {
        self.to
    }
}

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
        Self::checked(year, month, day)
    }

    /// Corps de [`CivilDate::new`] : la validation vit ici, car cargo-mutants ne mute jamais
    /// une fonction nommée `new` (`CONTRIBUTING.md`).
    fn checked(year: u16, month: u8, day: u8) -> Option<Self> {
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

    /// Lit exactement `AAAA-MM-JJ`, sans rognage. Sert aussi au suffixe de version
    /// des identifiants (`JJ-MM-AAAA` réordonné, `id.rs`).
    pub(crate) fn parse_iso(bytes: &[u8]) -> Option<Self> {
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
    /// (décision Q16, `docs/plan/00-decisions.md` §4 et ADR-0009) ; la référence écrit
    /// `1 janvier` (`_fr_date`, `citations.py:289-295`).
    ///
    /// ```
    /// use lf_domain::date::CivilDate;
    ///
    /// let date: CivilDate = "2026-01-01".parse()?;
    /// assert_eq!(date.to_french(), "1er janvier 2026");
    /// let date: CivilDate = "2025-11-07".parse()?;
    /// assert_eq!(date.to_french(), "7 novembre 2025");
    /// # Ok::<(), lf_domain::DomainError>(())
    /// ```
    #[expect(
        clippy::indexing_slicing,
        reason = "le mois d'une `CivilDate` est validé entre 1 et 12 à la construction"
    )]
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
    use std::collections::HashSet;

    fn d(y: u16, m: u8, day: u8) -> CivilDate {
        CivilDate::new(y, m, day).unwrap()
    }

    #[test]
    fn hash_agrees_with_eq() {
        // Clés de cache (M3) : deux lectures d'une même date, une seule clé.
        let civil = [d(2016, 10, 8), " 2016-10-08 ".parse().unwrap(), d(2016, 10, 9)];
        assert_eq!(HashSet::from(civil), HashSet::from([d(2016, 10, 8), d(2016, 10, 9)]));
        let day = VersionDate::parse("2016-10-08").unwrap();
        let versions = [day, VersionDate::try_from(d(2016, 10, 8)).unwrap(), VersionDate::MAX];
        assert_eq!(HashSet::from(versions), HashSet::from([day, VersionDate::MAX]));
        let since = DateRange::new(Some(day), None).unwrap().unwrap();
        let closed = DateRange::new(Some(day), Some(VersionDate::MAX)).unwrap().unwrap();
        let until = DateRange::new(None, Some(day)).unwrap().unwrap();
        assert_eq!(HashSet::from([since, closed, until]), HashSet::from([since, until]));
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
    fn negative_ms_1804() {
        // Code civil, `textTitles[].dateDebut` de consult_article.json (03 §6.3).
        assert_eq!(CivilDate::from_unix_millis(-5_231_606_400_000), Some(d(1804, 3, 21)));
    }

    #[test]
    fn open_ended_maps_sentinels_to_none() {
        assert_eq!(SENTINEL_FLOOR, d(2222, 1, 1));
        for sentinel in [d(2999, 1, 1), d(2222, 2, 22), d(2222, 1, 1)] {
            assert_eq!(open_ended(Some(sentinel)), None, "{sentinel}");
        }
        for kept in [d(2221, 12, 31), d(1804, 3, 21)] {
            assert_eq!(open_ended(Some(kept)), Some(kept), "{kept}");
        }
        assert_eq!(open_ended(None), None);
    }

    #[test]
    fn epoch_ms_bounds_feed_open_ended_and_version_date() {
        // Chaîne du mapper (03 §6.2-§6.3) : millisecondes epoch, puis `open_ended`.
        let at = |millis| CivilDate::from_unix_millis(millis).unwrap();
        assert_eq!(at(-14_831_769_600_000), VersionDate::MIN.as_civil());
        assert_eq!(at(7_952_256_000_000), VersionDate::MAX.as_civil());
        // `MAX` est la veille de `SENTINEL_FLOOR`.
        assert_eq!(at(7_952_256_000_000 + MILLIS_PER_DAY), SENTINEL_FLOOR);
        assert_eq!(open_ended(Some(at(7_952_256_000_000))), Some(VersionDate::MAX.as_civil()));
        for sentinel in [7_952_342_400_000, 7_956_835_200_000, 32_472_144_000_000] {
            assert_eq!(open_ended(CivilDate::from_unix_millis(sentinel)), None, "{sentinel}");
        }
    }

    #[test]
    fn version_date_reads_its_range() {
        assert_eq!(VersionDate::parse("1500-01-01"), Ok(VersionDate::MIN));
        assert_eq!(VersionDate::parse("2221-12-31"), Ok(VersionDate::MAX));
        let date: VersionDate = "2026-10-15".parse().unwrap();
        assert_eq!(date.as_civil(), d(2026, 10, 15));
        assert_eq!(CivilDate::from(date), d(2026, 10, 15));
        assert_eq!(date.to_string(), "2026-10-15");
        assert_eq!(VersionDate::MIN.to_string(), "1500-01-01");
        assert_eq!(VersionDate::MAX.to_string(), "2221-12-31");
        let mut sorted = [VersionDate::MAX, date, VersionDate::MIN];
        sorted.sort();
        assert_eq!(sorted, [VersionDate::MIN, date, VersionDate::MAX]);
    }

    #[test]
    fn version_date_rejects_bad_formats() {
        // D-22 : ni format étranger, ni date impossible, ni sentinelle, ni
        // espaces (parse strict, 03 §6.1).
        for bad in [
            "07/10/2016",
            "2016-13-45",
            "hier",
            "2026-02-29",
            "1499-12-31",
            "2222-01-01",
            "2999-01-01",
            " 2026-10-15",
            "2026-10-15 ",
            " 2026-10-15 ",
            "2026-10-15\n",
            "",
        ] {
            let refused = Err(DomainError::InvalidDate { input: bad.into() });
            assert_eq!(VersionDate::parse(bad), refused, "{bad:?}");
            assert_eq!(bad.parse::<VersionDate>(), refused, "{bad:?}");
        }
    }

    #[test]
    fn version_date_from_civil_date_checks_the_range() {
        assert_eq!(VersionDate::try_from(d(2026, 10, 15)), VersionDate::parse("2026-10-15"));
        assert_eq!(VersionDate::try_from(d(1500, 1, 1)), Ok(VersionDate::MIN));
        assert_eq!(VersionDate::try_from(d(2221, 12, 31)), Ok(VersionDate::MAX));
        for out in [d(1499, 12, 31), d(2222, 1, 1), d(2999, 1, 1)] {
            assert_eq!(
                VersionDate::try_from(out),
                Err(DomainError::InvalidDate { input: out.to_string() })
            );
        }
    }

    #[test]
    fn date_range_with_one_bound_uses_range_limit() {
        let day = VersionDate::parse("2016-10-08").unwrap();
        let since = DateRange::new(Some(day), None).unwrap().unwrap();
        assert_eq!((since.start(), since.end()), (day, VersionDate::MAX));
        let until = DateRange::new(None, Some(day)).unwrap().unwrap();
        assert_eq!((until.start(), until.end()), (VersionDate::MIN, day));
        // Aucune borne : pas de période.
        assert_eq!(DateRange::new(None, None), Ok(None));
    }

    #[test]
    fn inverted_date_range_is_rejected() {
        let early = VersionDate::parse("2025-01-01").unwrap();
        let late = VersionDate::parse("2026-01-01").unwrap();
        assert_eq!(
            DateRange::new(Some(late), Some(early)),
            Err(DomainError::InvertedDateRange { from: late, to: early })
        );
        let range = DateRange::new(Some(early), Some(late)).unwrap().unwrap();
        assert_eq!((range.start(), range.end()), (early, late));
        // Bornes égales : une période d'un jour.
        let single = DateRange::new(Some(early), Some(early)).unwrap().unwrap();
        assert_eq!((single.start(), single.end()), (early, early));
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
            let _ = VersionDate::parse(&s);
        }

        #[test]
        fn version_date_roundtrip(y in 1500u16..=2221, m in 1u8..=12, day in 1u8..=28) {
            let civil = d(y, m, day);
            let date = VersionDate::try_from(civil).unwrap();
            prop_assert_eq!(date.as_civil(), civil);
            prop_assert_eq!(VersionDate::parse(&date.to_string()), Ok(date));
            // Hors format (espace, ordre JJ/MM/AAAA) : refusée, sans panique.
            for bad in [format!(" {date}"), format!("{day:02}/{m:02}/{y:04}")] {
                prop_assert_eq!(VersionDate::parse(&bad), Err(DomainError::InvalidDate { input: bad.clone() }));
            }
        }

        #[test]
        fn version_date_refuses_years_out_of_range(
            y in prop_oneof![1u16..1500, 2222u16..=9999],
            m in 1u8..=12,
            day in 1u8..=28,
        ) {
            let civil = d(y, m, day);
            let refused = Err(DomainError::InvalidDate { input: civil.to_string() });
            prop_assert_eq!(VersionDate::try_from(civil), refused.clone());
            prop_assert_eq!(VersionDate::parse(&civil.to_string()), refused);
        }
    }
}
