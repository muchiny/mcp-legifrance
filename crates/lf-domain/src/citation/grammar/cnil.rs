//! Grammaire des délibérations de la CNIL (annexe B §4.11).
//!
//! ```text
//! cnil_cite = [ 'CNIL' SEP ] cnil_kw [ WS "de la formation restreinte" ] [ WS 'CNIL' ]
//!             WS? [ NUM_SIGN ] cnil_num [ WS "du" WS DATE ]
//! cnil_num  = [ cnil_series "-" ] YEAR4 "-" /[0-9]{3}/ B
//! ```
//!
//! Confiance `High` si `CNIL` ou une série est écrite ; sinon `Low` avec `emetteur_inconnu`, car
//! les délibérations municipales sont aussi numérotées `AAAA-NNN` (N16). Un numéro incomplet
//! (`SAN-2026`) ne donne rien (N15). Les plages des candidats sont des offsets du texte
//! d'ORIGINE (`Normalized::orig_range`), celles des ancres des offsets de `Normalized::text`.

use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{Ambiguity, CitationRef, CnilCite, Confidence};

use super::Candidate;
use super::jorf::{Hit, Win, scan_with};

/// `cnil_kw`, replié.
const KEYWORDS: [&str; 4] = ["deliberation", "delib.", "decision", "mise en demeure"];

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    scan_with(n, anchors, &[AnchorClass::Family, AnchorClass::Sigle], &[cnil])
}

/// `cnil_num` : série, année, numéro d'ordre, et la fin du numéro.
fn number(w: &Win, p: usize) -> Option<(usize, Option<String>, u16, u16)> {
    let mut e = p;
    while matches!(w.at(e), Some('A'..='Z')) {
        e += 1;
    }
    let series = ((2..=4).contains(&(e - p)) && w.at(e) == Some('-')).then(|| w.text_of(p, e));
    let (q, year) = w.year4(if series.is_some() { e + 1 } else { p })?;
    let (q, seq) = w.digits(w.cs(q, "-")?, 3, 3)?;
    Some((q, series, year, u16::try_from(seq).unwrap_or(u16::MAX)))
}

/// `cnil_cite`, lue au caractère `s` de la fenêtre.
fn cnil(w: &Win, s: usize) -> Option<Hit> {
    let prefix = w.cs_word(s, "CNIL").and_then(|q| w.sep(q));
    let kw_start = prefix.unwrap_or(s);
    let p = KEYWORDS.iter().find_map(|kw| w.kw(kw_start, kw))?;
    let restricted = w.ws(p).and_then(|q| w.kw(q, "de la formation restreinte"));
    let p = restricted.unwrap_or(p);
    let named = w.ws(p).and_then(|q| w.cs_word(q, "CNIL"));
    let p = w.ws0(named.unwrap_or(p));
    let (q, series, year, seq) = number(w, w.num_sign(p).unwrap_or(p))?;
    let date = w.du_date(q);
    let known_sender = prefix.is_some() || named.is_some() || series.is_some();
    let cite = CnilCite {
        series,
        year,
        seq,
        date: date.and_then(|d| d.date),
        formation_restreinte: restricted.is_some(),
    };
    let mut hit = Hit::new(s, date.map_or(q, |d| d.end), CitationRef::CnilDeliberation(cite));
    if !known_sender {
        hit.confidence = Confidence::Low;
        hit.ambiguity = Some(Ambiguity::EmetteurInconnu);
    }
    hit.flags.extend(date.and_then(|d| d.flag()));
    Some(hit)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::grammar::jorf::testkit::{Found, find};
    use crate::citation::normalize::{Steps, normalize};
    use crate::citation::scan::AnchorClass;
    use crate::citation::types::{Ambiguity, CitationRef, CnilCite, Confidence, Flag};
    use crate::date::CivilDate;

    fn found(input: &str, needles: &[&str]) -> Vec<Found> {
        find(scan, input, AnchorClass::Family, needles)
    }

    fn cite(
        series: Option<&str>,
        year: u16,
        seq: u16,
        date: Option<(u16, u8, u8)>,
        formation_restreinte: bool,
    ) -> CitationRef {
        CitationRef::CnilDeliberation(CnilCite {
            series: series.map(str::to_owned),
            year,
            seq,
            date: date.and_then(|(y, m, d)| CivilDate::new(y, m, d)),
            formation_restreinte,
        })
    }

    fn one(input: &str, needles: &[&str]) -> Found {
        let mut all = found(input, needles);
        assert_eq!(all.len(), 1, "{input:?} : {all:?}");
        all.remove(0)
    }

    #[test]
    fn p170_deliberation_with_series_and_date() {
        let f = one("Délibération SAN-2026-002 du 8 janvier 2026", &["Délibération"]);
        assert_eq!(f.text, "Délibération SAN-2026-002 du 8 janvier 2026");
        assert_eq!(f.value, cite(Some("SAN"), 2026, 2, Some((2026, 1, 8)), false));
        assert_eq!((f.confidence, f.ambiguity, f.flags), (Confidence::High, None, vec![]));
    }

    #[test]
    fn p171_formation_restreinte_and_trailing_words_excluded() {
        let input = "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 \
                     prononçant une sanction pécuniaire à l'encontre de la société FREE";
        let f = one(input, &["Délibération"]);
        assert_eq!(
            f.text,
            "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026"
        );
        assert_eq!(f.value, cite(Some("SAN"), 2026, 2, Some((2026, 1, 8)), true));
    }

    #[test]
    fn p172_en_dash_in_the_number_is_folded() {
        let input = "Délibération de la formation restreinte n° SAN–2026-003 du 22 janvier 2026";
        let f = one(input, &["Délibération"]);
        assert_eq!(f.text, input);
        assert_eq!(f.value, cite(Some("SAN"), 2026, 3, Some((2026, 1, 22)), true));
    }

    #[test]
    fn p173_cnil_prefix_and_unaccented_keyword() {
        let input = "CNIL, deliberation n° SAN-2026-002 du 8 janvier 2026";
        // Que l'ancre soit « CNIL » ou le mot-clé, le même candidat, le plus long, sort.
        for needle in ["CNIL", "deliberation"] {
            let f = one(input, &[needle]);
            assert_eq!(f.text, input);
            assert_eq!(f.value, cite(Some("SAN"), 2026, 2, Some((2026, 1, 8)), false));
            assert_eq!(f.confidence, Confidence::High);
        }
        assert_eq!(found(input, &["CNIL", "deliberation"]).len(), 1);
    }

    #[test]
    fn p174_number_without_series_is_high_with_cnil() {
        let f = one("CNIL, délibération n°2023-062 du 13 avril 2023", &["CNIL"]);
        assert_eq!(f.text, "CNIL, délibération n°2023-062 du 13 avril 2023");
        assert_eq!(f.value, cite(None, 2023, 62, Some((2023, 4, 13)), false));
        assert_eq!((f.confidence, f.ambiguity), (Confidence::High, None));
    }

    #[test]
    fn p175_decision_keyword_without_date() {
        let f = one("CNIL, décision n° MED-2018-024", &["CNIL"]);
        assert_eq!(f.text, "CNIL, décision n° MED-2018-024");
        assert_eq!(f.value, cite(Some("MED"), 2018, 24, None, false));
    }

    #[test]
    fn p176_abbreviated_keyword_then_cnil_and_abbreviated_month() {
        let f = one("délib. CNIL n° 2024-012 du 1er févr. 2024", &["délib."]);
        assert_eq!(f.text, "délib. CNIL n° 2024-012 du 1er févr. 2024");
        assert_eq!(f.value, cite(None, 2024, 12, Some((2024, 2, 1)), false));
        assert_eq!(f.confidence, Confidence::High);
    }

    #[test]
    fn n16_municipal_deliberation_has_unknown_sender_and_low_confidence() {
        let input = "Délibération n° 2023-062 du 13 avril 2023 du conseil municipal";
        let f = one(input, &["Délibération"]);
        assert_eq!(f.text, "Délibération n° 2023-062 du 13 avril 2023");
        assert_eq!(f.confidence, Confidence::Low);
        assert_eq!(f.ambiguity, Some(Ambiguity::EmetteurInconnu));
    }

    #[test]
    fn a_series_alone_is_enough_for_high_confidence() {
        let f = one("Décision MED-2018-024", &["Décision"]);
        assert_eq!((f.confidence, f.ambiguity), (Confidence::High, None));
        assert_eq!(f.value, cite(Some("MED"), 2018, 24, None, false));
        // Toute série de deux à quatre majuscules compte.
        assert_eq!(one("délibération AB-2020-001", &["délibération"]).confidence, Confidence::High);
        assert_eq!(
            one("délibération ABCD-2020-001", &["délibération"]).confidence,
            Confidence::High
        );
    }

    #[test]
    fn n15_incomplete_number_is_not_a_citation() {
        assert_eq!(found("délibération SAN-2026", &["délibération"]), []);
        assert_eq!(found("délibération n° 2026", &["délibération"]), []);
        // Quatre chiffres d'ordre, ou une série trop longue ou en minuscules.
        assert_eq!(found("Délibération n° 2025-1173", &["Délibération"]), []);
        assert_eq!(found("Délibération ABCDE-2025-117", &["Délibération"]), []);
        assert_eq!(found("Délibération san-2025-117", &["Délibération"]), []);
        assert_eq!(found("Délibération S-2025-117", &["Délibération"]), []);
        // Année hors de 1500 à 2099.
        assert_eq!(found("Délibération n° 1499-117", &["Délibération"]), []);
        assert_eq!(found("Délibération n° 2100-117", &["Délibération"]), []);
    }

    #[test]
    fn a_number_sign_is_optional_and_has_four_spellings() {
        for input in [
            "délibération 2023-062",
            "délibération no 2023-062",
            "délibération num. 2023-062",
            "délibération numéro 2023-062",
            "délibération n°2023-062",
            "délibération № 2023-062",
            "délib.2023-062",
        ] {
            let f = one(input, &["délib"]);
            assert_eq!(f.value, cite(None, 2023, 62, None, false), "{input}");
            assert_eq!(f.text, input, "{input}");
        }
        // « no » n'est un signe de numéro que devant un chiffre.
        assert_eq!(found("délibération no SAN-2026-002", &["délibération"]), []);
    }

    #[test]
    fn the_other_keywords_and_the_cnil_prefix_separators() {
        for input in [
            "CNIL délibération 2023-062",
            "CNIL: délibération 2023-062",
            "CNIL ; délibération 2023-062",
            "CNIL , délibération 2023-062",
        ] {
            let f = one(input, &["CNIL"]);
            assert_eq!(f.text, input, "{input}");
        }
        let f = one("CNIL, mise en demeure n° MED-2018-024", &["mise en demeure"]);
        assert_eq!(f.text, "CNIL, mise en demeure n° MED-2018-024");
        // Pas de mot-clé collé au numéro, ni de « CNIL » suivi de rien d'utile.
        assert_eq!(found("délibération2023-062", &["délibération"]), []);
        assert_eq!(found("CNIL, avis 2023-062", &["CNIL"]), []);
        assert_eq!(found("CNIL", &["CNIL"]), []);
    }

    #[test]
    fn a_du_without_a_date_stops_before_it() {
        let f = one("Délibération SAN-2026-002 du conseil", &["Délibération"]);
        assert_eq!(f.text, "Délibération SAN-2026-002");
        assert_eq!(f.value, cite(Some("SAN"), 2026, 2, None, false));
    }

    #[test]
    fn an_impossible_date_is_kept_raw_with_a_flag() {
        let f = one("Délibération SAN-2026-002 du 31 février 2026", &["Délibération"]);
        assert_eq!(f.text, "Délibération SAN-2026-002 du 31 février 2026");
        assert_eq!(f.value, cite(Some("SAN"), 2026, 2, None, false));
        assert_eq!(f.flags, vec![Flag::DateInvalide]);
        let g = one("Délibération SAN-2026-002 du 12 thermidor an XII", &["Délibération"]);
        assert_eq!(g.flags, vec![Flag::DateRepublicaine]);
        assert_eq!(g.text, "Délibération SAN-2026-002 du 12 thermidor an XII");
    }

    #[test]
    fn only_family_and_sigle_anchors_are_read_and_bad_anchors_are_ignored() {
        let input = "Délibération SAN-2026-002";
        assert_eq!(find(scan, input, AnchorClass::Court, &["Délibération"]), []);
        assert_eq!(find(scan, input, AnchorClass::Sigle, &["Délibération"]).len(), 1);
        assert_eq!(find(scan, input, AnchorClass::Family, &["absent"]), []);
        assert_eq!(scan(&normalize("é", Steps::default()), &[]), []);
    }
}
