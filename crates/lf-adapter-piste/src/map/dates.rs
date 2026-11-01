//! Dates brutes vers dates civiles (04 §9.1 e, §8.2 ; 03 §6).
//!
//! PISTE rend une date en millisecondes depuis l'époque Unix ou en texte (`2024-03-15`,
//! `2024-03-15T00:00:00.000+0000`). Le mapping lit toujours le jour civil UTC : un instant qui ne
//! tombe pas à minuit UTC est lu quand même, et compté (`Diagnostics::non_midnight_timestamps`).
//! Une date à partir de 2222-01-01 est une sentinelle de l'API (`2999-01-01`) : elle devient
//! `None` (`open_ended`). Une date illisible est une erreur `Decode` qui nomme le champ, jamais
//! la valeur.

use lf_application::ports::CorpusError;
use lf_domain::date::{CivilDate, open_ended};

use super::{Diagnostics, Field};
use crate::dto::de::RawDate;

/// Millisecondes dans un jour.
const MILLIS_PER_DAY: i64 = 86_400_000;
/// Longueur de `AAAA-MM-JJ`.
const DATE_LEN: usize = 10;
/// Catégorie de l'erreur d'une date illisible.
const UNREADABLE: &str = "unreadable date";
/// Écritures du fuseau UTC en fin d'heure.
const UTC_ZONES: [&str; 3] = ["+0000", "+00:00", "Z"];

/// Date civile d'un champ : `None` si le champ est absent ou si la date est une sentinelle.
///
/// # Errors
///
/// [`CorpusError::Decode`] (`unreadable date`) si la date n'existe pas au calendrier ou si le
/// texte n'a pas la forme `AAAA-MM-JJ` suivie éventuellement de `T` et d'une heure.
pub(crate) fn civil(
    raw: Option<&RawDate>,
    field: Field,
    diags: &mut Diagnostics,
) -> Result<Option<CivilDate>, CorpusError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let date = match raw {
        RawDate::Millis(millis) => from_millis(*millis, diags),
        RawDate::Text(text) => from_text(text, diags),
    };
    date.ok_or_else(|| field.decode(UNREADABLE)).map(|date| open_ended(Some(date)))
}

/// Date civile d'un champ secondaire, lue sans jamais échouer : `None` si le champ est absent, si
/// la date est une sentinelle **ou si elle est illisible** (une date inconnue n'est pas une
/// erreur `Decode`). Aucun diagnostic n'est compté.
///
/// Sert aux bornes des sections d'un chemin (`context.titresTM[]`), où une date perdue ne doit
/// pas faire perdre l'article.
pub(crate) fn civil_or_unknown(raw: Option<&RawDate>) -> Option<CivilDate> {
    const SECTION: Field = Field { endpoint: super::endpoint::GET_ARTICLE, path: "section" };
    civil(raw, SECTION, &mut Diagnostics::default()).ok().flatten()
}

/// Première date présente et hors sentinelle des `candidates`, dans l'ordre.
///
/// Lecture paresseuse : les candidats suivant la première date ne sont pas lus.
///
/// # Errors
///
/// [`CorpusError::Decode`] du premier candidat lu qui est illisible.
pub(crate) fn first_civil(
    candidates: &[(Field, Option<&RawDate>)],
    diags: &mut Diagnostics,
) -> Result<Option<CivilDate>, CorpusError> {
    for &(field, raw) in candidates {
        if let Some(date) = civil(raw, field, diags)? {
            return Ok(Some(date));
        }
    }
    Ok(None)
}

/// Date `computed` si elle est présente (même si sa conversion donne `None`), sinon `plain`.
///
/// # Errors
///
/// [`CorpusError::Decode`] du champ choisi s'il est illisible.
pub(crate) fn prefer_computed(
    computed: (Option<&RawDate>, Field),
    plain: (Option<&RawDate>, Field),
    diags: &mut Diagnostics,
) -> Result<Option<CivilDate>, CorpusError> {
    let (raw, field) = if computed.0.is_some() { computed } else { plain };
    civil(raw, field, diags)
}

/// Jour UTC de `millis`, compté si l'instant n'est pas à minuit UTC.
fn from_millis(millis: i64, diags: &mut Diagnostics) -> Option<CivilDate> {
    let date = CivilDate::from_unix_millis(millis)?;
    if millis.rem_euclid(MILLIS_PER_DAY) != 0 {
        diags.note_non_midnight();
    }
    Some(date)
}

/// Date d'un texte `AAAA-MM-JJ` ou `AAAA-MM-JJT<heure>`, comptée si l'heure n'est pas minuit UTC.
fn from_text(text: &str, diags: &mut Diagnostics) -> Option<CivilDate> {
    let trimmed = text.trim();
    // Une coupure hors frontière de caractère (`2024-03-1é`) ou un texte trop court n'est pas une
    // date : `split_at_checked` rend `None` au lieu de paniquer.
    let (date, time) = trimmed.split_at_checked(DATE_LEN)?;
    let date = CivilDate::parse(date).ok()?;
    if !time.is_empty() && !is_utc_midnight(time.strip_prefix('T')?) {
        diags.note_non_midnight();
    }
    Some(date)
}

/// Vrai si `time` est minuit UTC, avec ou sans fractions et fuseau UTC.
fn is_utc_midnight(time: &str) -> bool {
    let bare = UTC_ZONES.iter().find_map(|zone| time.strip_suffix(zone)).unwrap_or(time);
    bare.starts_with("00:00:00") && bare.bytes().all(|byte| matches!(byte, b'0' | b':' | b'.'))
}

#[cfg(test)]
mod tests {
    use lf_domain::date::CivilDate;
    use serde_json::Value;

    use super::*;
    use crate::{
        dto::{de, decode::decode, juri::ConsultJuriResponseDto},
        map::{
            endpoint,
            fixtures::{self, d},
        },
    };

    /// Champ type des tests.
    const AT: Field = Field { endpoint: endpoint::JURI, path: "text.dateTexte" };
    /// Champ calculé de la décision.
    const COMPUTED: Field = Field { endpoint: endpoint::JURI, path: "text.dateTexteComputed" };
    /// Détail attendu d'une date illisible en [`AT`].
    const AT_DETAIL: &str = "/consult/juri: text.dateTexte: unreadable date";

    fn millis(value: i64) -> RawDate {
        RawDate::Millis(value)
    }

    fn text(value: &str) -> RawDate {
        RawDate::Text(value.to_owned())
    }

    /// Erreur attendue d'une date illisible en [`AT`].
    fn unreadable() -> CorpusError {
        CorpusError::Decode { detail: AT_DETAIL.to_owned() }
    }

    /// `civil` en [`AT`], avec les diagnostics qu'il laisse.
    fn run(raw: &RawDate) -> (Result<Option<CivilDate>, CorpusError>, Diagnostics) {
        let mut diags = Diagnostics::default();
        let result = civil(Some(raw), AT, &mut diags);
        (result, diags)
    }

    /// Diagnostics d'une seule horodatage hors minuit.
    fn one_non_midnight() -> Diagnostics {
        Diagnostics { non_midnight_timestamps: 1, ..Diagnostics::default() }
    }

    /// Date brute d'une fixture, à l'adresse JSON `pointer`.
    fn raw_at(fixture: &str, pointer: &str) -> RawDate {
        let root = fixtures::value(fixture);
        let leaf = root.pointer(pointer).unwrap();
        de::opt_raw_date(leaf.clone()).unwrap().unwrap()
    }

    /// Vérifie que chaque entrée donne la date attendue, sans diagnostic.
    fn assert_dates(cases: &[(RawDate, Option<CivilDate>)]) {
        for (raw, expected) in cases {
            let (result, diags) = run(raw);
            assert_eq!(result, Ok(*expected), "{raw:?}");
            assert_eq!(diags, Diagnostics::default(), "{raw:?}");
        }
    }

    /// Clé d'un champ de date dans les réponses de PISTE.
    fn is_date_key(key: &str) -> bool {
        key.starts_with("date") || key == "debut" || key == "fin"
    }

    /// Valeurs des champs de date de `value`, à toute profondeur.
    fn date_leaves<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if is_date_key(key) {
                        out.push(child);
                    } else {
                        date_leaves(child, out);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| date_leaves(item, out)),
            _ => {}
        }
    }

    /// Sentinelle `2999-01-01` d'après son littéral brut.
    fn is_sentinel(raw: &RawDate) -> bool {
        match raw {
            RawDate::Millis(value) => *value == 32_472_144_000_000,
            RawDate::Text(text) => text.starts_with("2999-01-01"),
        }
    }

    #[test]
    fn sentinel_occurrences_in_fixtures_become_none() {
        let mut leaves = 0;
        let mut absent = 0;
        let mut sentinels = [0_u32; 3];
        let mut dates = 0;
        let mut diags = Diagnostics::default();
        for name in fixtures::NAMES {
            let root = fixtures::value(name);
            let mut found = Vec::new();
            date_leaves(&root, &mut found);
            for leaf in found {
                leaves += 1;
                let Some(raw) = de::opt_raw_date(leaf.clone()).unwrap() else {
                    absent += 1;
                    continue;
                };
                let result = civil(Some(&raw), AT, &mut diags).unwrap();
                if is_sentinel(&raw) {
                    assert_eq!(result, None, "{name}: {raw:?}");
                    match raw {
                        RawDate::Millis(_) => sentinels[0] += 1,
                        RawDate::Text(ref value) if value.len() == DATE_LEN => sentinels[1] += 1,
                        RawDate::Text(_) => sentinels[2] += 1,
                    }
                } else {
                    assert!(result.is_some(), "{name}: {raw:?}");
                    dates += 1;
                }
            }
        }
        assert_eq!(leaves, 235);
        assert_eq!(absent, 154);
        assert_eq!(sentinels, [11, 6, 5]);
        assert_eq!(dates, 59);
        assert_eq!(diags.non_midnight_timestamps, 2);
    }

    #[test]
    fn non_midnight_in_fixtures_are_the_two_kali_section_versions() {
        for index in [0, 1] {
            let pointer = format!("/results/{index}/sections/0/dateVersion");
            let mut diags = Diagnostics::default();
            civil(Some(&raw_at("search_kali", &pointer)), AT, &mut diags).unwrap();
            assert_eq!(diags, one_non_midnight(), "{pointer}");
        }
    }

    #[test]
    fn sentinel_2999_and_2222_are_none() {
        assert_dates(&[
            (millis(32_472_144_000_000), None),
            (millis(7_952_342_400_000), None),
            (text("2999-01-01"), None),
            (text("2222-01-01"), None),
            (text("2999-01-01T00:00:00.000+0000"), None),
            (millis(7_952_256_000_000), Some(d(2221, 12, 31))),
            (text("2221-12-31"), Some(d(2221, 12, 31))),
        ]);
    }

    #[test]
    fn unknown_section_dates_are_none_never_an_error() {
        let known =
            [(text("2016-10-01"), d(2016, 10, 1)), (millis(1_475_280_000_000), d(2016, 10, 1))];
        for (raw, expected) in known {
            assert_eq!(civil_or_unknown(Some(&raw)), Some(expected));
        }
        // Absente, sentinelle, calendrier impossible, forme inconnue : inconnue.
        for raw in [text("2999-01-01"), text("2016-13-45"), text("hier"), millis(i64::MAX)] {
            assert_eq!(civil_or_unknown(Some(&raw)), None, "{raw:?}");
        }
        assert_eq!(civil_or_unknown(None), None);
    }

    #[test]
    fn iso_plus0000_string_parses() {
        assert_dates(&[
            (raw_at("search_loda", "/results/0/date"), Some(d(2016, 10, 9))),
            (raw_at("search_loda", "/results/0/datePublication"), Some(d(2016, 10, 8))),
            (text("2016-10-09T00:00:00+0000"), Some(d(2016, 10, 9))),
            (text("2016-10-09T00:00:00Z"), Some(d(2016, 10, 9))),
            (text("2016-10-09T00:00:00.000+00:00"), Some(d(2016, 10, 9))),
            (text("2016-10-09T00:00:00"), Some(d(2016, 10, 9))),
        ]);
    }

    #[test]
    fn negative_ms_1804() {
        let expected = Some(d(1804, 3, 21));
        assert_dates(&[
            (millis(-5_231_606_400_000), expected),
            (raw_at("consult_article", "/article/textTitles/0/dateDebut"), expected),
            (raw_at("consult_article", "/article/context/titresTM/0/debut"), expected),
            (raw_at("search_code", "/results/0/date"), expected),
        ]);
    }

    #[test]
    fn millis_vectors() {
        assert_dates(&[
            (millis(17_193_600_000), Some(d(1970, 7, 19))),
            (millis(1_475_884_800_000), Some(d(2016, 10, 8))),
            (millis(1_710_460_800_000), Some(d(2024, 3, 15))),
        ]);
    }

    #[test]
    fn calendar_range_edges_are_decode_errors() {
        assert_dates(&[
            (millis(-62_135_596_800_000), Some(d(1, 1, 1))),
            (millis(253_402_214_400_000), None),
        ]);
        for edge in [-62_135_596_800_001, 253_402_300_800_000, i64::MIN, i64::MAX] {
            let (result, _) = run(&millis(edge));
            assert_eq!(result, Err(unreadable()), "{edge}");
        }
    }

    #[test]
    fn text_that_is_not_a_date_is_a_decode_error() {
        for input in [
            "15/03/2024",
            "2024-13-45",
            "2024-02-30",
            "2024-03-15 00:00:00",
            "2024-03-15t00:00:00",
            "2024-03-1é",
            "é2024-03-15T00:00:00",
            "T00:00:00",
            "2024-13-45T00:00:00",
            "2024-03-15é",
        ] {
            let (result, diags) = run(&text(input));
            let error = result.unwrap_err();
            assert_eq!(error, unreadable(), "{input}");
            assert!(!format!("{error:?} {error}").contains("2024"), "{input}");
            assert_eq!(diags, Diagnostics::default(), "{input}");
        }
    }

    #[test]
    fn non_midnight_is_counted_not_refused() {
        let cases = [
            (millis(1_710_460_800_000 + 3_600_000), Some(d(2024, 3, 15))),
            (millis(-1), Some(d(1969, 12, 31))),
            (text("2026-07-08T07:47:16.102+0000"), Some(d(2026, 7, 8))),
            (text("2024-03-15T"), Some(d(2024, 3, 15))),
            (text("2999-01-01T07:47:16.102+0000"), None),
        ];
        for (raw, expected) in cases {
            let (result, diags) = run(&raw);
            assert_eq!(result, Ok(expected), "{raw:?}");
            assert_eq!(diags, one_non_midnight(), "{raw:?}");
        }
    }

    #[test]
    fn yyyymmdd_digits_read_as_1970_with_a_diagnostic() {
        let (result, diags) = run(&millis(20_240_315));
        assert_eq!(result, Ok(Some(d(1970, 1, 1))));
        assert_eq!(diags, one_non_midnight());
    }

    #[test]
    fn is_utc_midnight_table() {
        for time in [
            "00:00:00",
            "00:00:00.000",
            "00:00:00.000+0000",
            "00:00:00+0000",
            "00:00:00Z",
            "00:00:00.000+00:00",
        ] {
            assert!(is_utc_midnight(time), "{time}");
        }
        for time in [
            "00:00:01.000+0000",
            "00:00:00.001+0000",
            "00:00:00.000+0200",
            "07:47:16.102+0000",
            "00:00",
            "",
            "01:00:00",
        ] {
            assert!(!is_utc_midnight(time), "{time}");
        }
    }

    #[test]
    fn civil_of_none_is_none() {
        let mut diags = Diagnostics::default();
        assert_eq!(civil(None, AT, &mut diags), Ok(None));
        assert_eq!(diags, Diagnostics::default());
    }

    #[test]
    fn text_dates_tolerate_surrounding_blanks() {
        assert_dates(&[(text(" 2024-03-15 "), Some(d(2024, 3, 15)))]);
    }

    /// Candidats `[(calculée, brute)]` de la décision de la fixture `name`.
    fn decision_date(name: &str, endpoint: &'static str) -> (Option<CivilDate>, Diagnostics) {
        let dto = decode::<ConsultJuriResponseDto>(fixtures::text(name), endpoint).unwrap();
        let text = dto.text.unwrap();
        let mut diags = Diagnostics::default();
        let raw = Field { endpoint, path: "text.dateTexte" };
        let computed = Field { endpoint, path: "text.dateTexteComputed" };
        let date = first_civil(
            &[(computed, text.date_texte_computed.as_ref()), (raw, text.date_texte.as_ref())],
            &mut diags,
        )
        .unwrap();
        (date, diags)
    }

    #[test]
    fn first_civil_reads_the_computed_date_first() {
        let cases = [
            ("consult_cnil", endpoint::CNIL, d(2026, 1, 8)),
            ("consult_juri", endpoint::JURI, d(2025, 6, 4)),
            ("consult_juri_cetat", endpoint::JURI, d(2026, 5, 29)),
            ("consult_juri_constit", endpoint::JURI, d(2025, 11, 7)),
        ];
        for (name, endpoint, expected) in cases {
            assert_eq!(decision_date(name, endpoint).0, Some(expected), "{name}");
        }
        let dto = decode::<ConsultJuriResponseDto>(fixtures::text("consult_cnil"), endpoint::CNIL)
            .unwrap();
        let publication = dto.text.unwrap().date_publi_computed;
        let mut diags = Diagnostics::default();
        assert_eq!(civil(publication.as_ref(), AT, &mut diags), Ok(Some(d(2026, 1, 14))));
    }

    #[test]
    fn first_civil_skips_absent_and_sentinel_candidates() {
        let mut diags = Diagnostics::default();
        let result = fixtures::value("search_kali");
        let hit = &result["results"][0];
        let read = |key: &str| de::opt_raw_date(hit[key].clone()).unwrap();
        let (date, published) = (read("date"), read("datePublication"));
        let (signed, broadcast) = (read("dateSignature"), read("dateDiffusion"));
        assert_eq!((&date, &signed, &broadcast), (&None, &None, &None));
        assert_eq!(published, Some(text("2999-01-01T00:00:00.000+0000")));
        let candidates = [
            (AT, date.as_ref()),
            (AT, published.as_ref()),
            (AT, signed.as_ref()),
            (AT, broadcast.as_ref()),
        ];
        assert_eq!(first_civil(&candidates, &mut diags), Ok(None));

        let sentinel = text("2999-01-01");
        let good = text("2016-12-28");
        let candidates = [(AT, Some(&sentinel)), (COMPUTED, None), (AT, Some(&good))];
        assert_eq!(first_civil(&candidates, &mut diags), Ok(Some(d(2016, 12, 28))));
    }

    #[test]
    fn first_civil_stops_at_the_first_date() {
        let mut diags = Diagnostics::default();
        let good = text("2024-03-15");
        let junk = text("n'importe quoi");
        let candidates = [(AT, Some(&good)), (COMPUTED, Some(&junk))];
        assert_eq!(first_civil(&candidates, &mut diags), Ok(Some(d(2024, 3, 15))));
    }

    #[test]
    fn first_civil_reports_the_field_of_the_unreadable_candidate() {
        let mut diags = Diagnostics::default();
        let junk = text("sekret-42");
        let good = text("2024-03-15");
        let candidates = [(COMPUTED, None), (COMPUTED, Some(&junk)), (AT, Some(&good))];
        let error = first_civil(&candidates, &mut diags).unwrap_err();
        assert_eq!(
            error,
            CorpusError::Decode {
                detail: "/consult/juri: text.dateTexteComputed: unreadable date".to_owned()
            }
        );
        assert!(!format!("{error:?}").contains("sekret"));
    }

    #[test]
    fn first_civil_of_nothing_is_none() {
        let mut diags = Diagnostics::default();
        assert_eq!(first_civil(&[], &mut diags), Ok(None));
        assert_eq!(first_civil(&[(AT, None), (COMPUTED, None)], &mut diags), Ok(None));
        assert_eq!(diags, Diagnostics::default());
    }

    /// `prefer_computed` entre [`COMPUTED`] et [`AT`] ; le champ brut s'appelle `PLAIN` dans la fiche.
    fn prefer(
        computed: Option<&RawDate>,
        plain: Option<&RawDate>,
    ) -> Result<Option<CivilDate>, CorpusError> {
        prefer_computed((computed, COMPUTED), (plain, AT), &mut Diagnostics::default())
    }

    #[test]
    fn prefer_computed_takes_the_computed_date() {
        let computed = millis(1_748_995_200_000);
        let plain = millis(1_762_473_600_000);
        assert_eq!(prefer(Some(&computed), Some(&plain)), Ok(Some(d(2025, 6, 4))));
    }

    #[test]
    fn prefer_computed_falls_back_to_the_plain_date() {
        let plain = millis(1_762_473_600_000);
        assert_eq!(prefer(None, Some(&plain)), Ok(Some(d(2025, 11, 7))));
        assert_eq!(prefer(None, Some(&text("2025-11-07"))), Ok(Some(d(2025, 11, 7))));
        assert_eq!(prefer(None, None), Ok(None));
    }

    #[test]
    fn prefer_computed_sentinel_is_none() {
        let computed = millis(32_472_144_000_000);
        let plain = millis(1_762_473_600_000);
        assert_eq!(prefer(Some(&computed), Some(&plain)), Ok(None));
    }

    #[test]
    fn prefer_computed_unreadable_names_the_chosen_field() {
        let junk = text("sekret-42");
        let plain = millis(1_762_473_600_000);
        let from_computed = prefer(Some(&junk), Some(&plain)).unwrap_err();
        assert_eq!(
            from_computed,
            CorpusError::Decode {
                detail: "/consult/juri: text.dateTexteComputed: unreadable date".to_owned()
            }
        );
        let from_plain = prefer(None, Some(&junk)).unwrap_err();
        assert_eq!(from_plain, unreadable());
        for error in [from_computed, from_plain] {
            assert!(!format!("{error:?}").contains("sekret"));
        }
    }
}
