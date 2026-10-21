#![no_main]

use lf_domain::{
    DomainError,
    date::{CivilDate, DateRange, SENTINEL_FLOOR, VersionDate, open_ended},
};
use libfuzzer_sys::fuzz_target;

/// Date civile tirée de trois entiers (`None` hors calendrier) et sa date de
/// version (`None` aussi hors de `MIN..=MAX`) : la lecture du texte et la
/// conversion depuis `CivilDate` s'accordent.
fn draw((year, month, day): (u16, u8, u8)) -> (Option<CivilDate>, Option<VersionDate>) {
    let civil = CivilDate::new(year, month, day);
    let version = civil.and_then(|date| VersionDate::try_from(date).ok());
    if let Some(date) = civil {
        assert_eq!(VersionDate::parse(&date.to_string()).ok(), version);
    }
    (civil, version)
}

fuzz_target!(|input: ((u16, u8, u8), (u16, u8, u8), i64, &str)| {
    let (a, b, millis, text) = input;

    // Lecture stricte : toute date acceptée s'écrit comme l'entrée, se relit à
    // l'identique et reste dans MIN..=MAX.
    if let Ok(date) = VersionDate::parse(text) {
        assert_eq!(date.to_string(), text);
        assert_eq!(VersionDate::parse(&date.to_string()), Ok(date));
        assert!(VersionDate::MIN <= date && date <= VersionDate::MAX);
    }

    // DateRange::new ne panique jamais : sans borne, `None` ; une borne absente
    // prend MIN ou MAX et la période rendue est ordonnée ; seule une période
    // inversée est refusée.
    let ((civil_a, from), (civil_b, to)) = (draw(a), draw(b));
    match DateRange::new(from, to) {
        Ok(None) => assert_eq!((from, to), (None, None)),
        Ok(Some(range)) => {
            assert!(range.start() <= range.end());
            assert_eq!(
                (range.start(), range.end()),
                (from.unwrap_or(VersionDate::MIN), to.unwrap_or(VersionDate::MAX))
            );
        }
        Err(error) => assert_eq!(
            Some(error),
            from.zip(to)
                .filter(|(from, to)| from > to)
                .map(|(from, to)| DomainError::InvertedDateRange { from, to })
        ),
    }

    // open_ended : `None` à partir de SENTINEL_FLOOR, date gardée sinon ; une
    // date de version n'est jamais une sentinelle.
    for date in [civil_a, civil_b, CivilDate::from_unix_millis(millis)] {
        assert_eq!(open_ended(date), date.filter(|date| *date < SENTINEL_FLOOR));
    }
    for date in [from, to].into_iter().flatten() {
        assert_eq!(open_ended(Some(date.as_civil())), Some(date.as_civil()));
    }
});
