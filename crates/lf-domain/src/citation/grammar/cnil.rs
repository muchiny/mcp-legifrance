//! Grammaire des délibérations de la CNIL (annexe B §4.11).
//!
//! ```text
//! cnil_cite = [ 'CNIL' SEP ] cnil_kw [ WS "de la formation restreinte" ] [ WS 'CNIL' ]
//!             WS? [ NUM_SIGN ] cnil_num [ WS "du" WS DATE ]
//! cnil_num  = [ cnil_series "-" ] YEAR4 "-" ( /[0-9]{3}/ | <série requise> /[1-9][0-9]{3}/ ) B
//! ```
//!
//! Confiance `High` si `CNIL` ou une série est écrite ; sinon `Low` avec `emetteur_inconnu`, car
//! les délibérations municipales sont aussi numérotées `AAAA-NNN` (N16). Un numéro incomplet
//! (`SAN-2026`) ne donne rien (N15), pas plus que quatre chiffres d'ordre sans série
//! (`2025-1173`) ou avec un zéro de tête (`SAN-2026-0002`).
//!
//! Deux lectures, fusionnées par [`scan`] : (a) les ancres `CNIL`, `délibération`, sigles lisent
//! toute série de deux à quatre majuscules ; (b) les ancres `décision` et `mise en demeure`
//! (`AnchorClass::CnilKeyword`) ne lisent que les séries de [`KNOWN_SERIES`], pour qu'une
//! `décision n° 2023-062` du Conseil constitutionnel, du Conseil d'État ou d'une commune, ou une
//! `décision PC-2023-045` d'urbanisme, ne devienne pas une citation de la CNIL. Les plages des candidats sont des offsets du texte
//! d'ORIGINE (`Normalized::orig_range`), celles des ancres des offsets de `Normalized::text`.

use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{Ambiguity, CitationRef, CnilCite, Confidence, cnil_order_fits};

use super::Candidate;
use super::jorf::{Hit, Win, scan_with};

/// `cnil_kw`, replié.
const KEYWORDS: [&str; 4] = ["deliberation", "delib.", "decision", "mise en demeure"];

/// Séries de la CNIL lues derrière `décision` ou `mise en demeure` seuls (B §4.11) : sanctions
/// (`SAN`), mises en demeure (`MED`, `MEDP`), décisions (`DF`, `DR`, `DT`) et habilitations (`HAB`).
pub(crate) const KNOWN_SERIES: [&str; 7] = ["SAN", "MED", "MEDP", "DF", "DR", "DT", "HAB"];

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires : la lecture par
/// `CNIL`, `délibération` et les sigles, puis la lecture par `décision` et `mise en demeure`
/// (séries connues seulement), triées par plage et sans doublon.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    let mut found = scan_with(n, anchors, &[AnchorClass::Family, AnchorClass::Sigle], &[cnil]);
    found.extend(scan_with(n, anchors, &[AnchorClass::CnilKeyword], &[cnil_known_series]));
    found.sort_by_key(|c| (c.range.start, c.range.end));
    found.dedup_by(|a, b| a.range == b.range && a.value == b.value);
    found
}

/// `cnil_cite` lue derrière `décision` ou `mise en demeure` : seulement avec une série connue.
fn cnil_known_series(w: &Win, s: usize) -> Option<Hit> {
    cnil(w, s).filter(|hit| {
        matches!(&hit.value, CitationRef::CnilDeliberation(cite)
            if cite.series.as_deref().is_some_and(|series| KNOWN_SERIES.contains(&series)))
    })
}

/// `cnil_num` : série, année, numéro d'ordre, et la fin du numéro.
fn number(w: &Win, p: usize) -> Option<(usize, Option<String>, u16, u16)> {
    let mut e = p;
    while matches!(w.at(e), Some('A'..='Z')) {
        e += 1;
    }
    let series = ((2..=4).contains(&(e - p)) && w.at(e) == Some('-')).then(|| w.text_of(p, e));
    let (q, year) = w.year4(if series.is_some() { e + 1 } else { p })?;
    let order = w.cs(q, "-")?;
    let (q, seq) = w.digits(order, 3, 4)?;
    cnil_order_fits(q.saturating_sub(order), seq, series.is_some())
        .then(|| (q, series, year, u16::try_from(seq).unwrap_or(u16::MAX)))
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
    use crate::citation::grammar::jorf::testkit::{Found, find, find_pairs};
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

    // ---- U07 : mots-clés `décision` et `mise en demeure`, numéros d'ordre à quatre chiffres ------

    /// Candidats lus avec une ancre de la classe `CnilKeyword` posée sur `needles`.
    fn keyword(input: &str, needles: &[&str]) -> Vec<Found> {
        find(scan, input, AnchorClass::CnilKeyword, needles)
    }

    /// Texte d'origine et valeur de chaque candidat, avec les VRAIES ancres du texte.
    fn real(input: &str) -> Vec<(String, CitationRef)> {
        let n = normalize(input, Steps::default());
        scan(&n, &crate::citation::scan::find_anchors(&n))
            .into_iter()
            .map(|c| (input.get(c.range).unwrap_or_default().to_owned(), c.value))
            .collect()
    }

    #[test]
    fn decision_and_mise_en_demeure_need_a_series_without_cnil() {
        let f = &keyword("Décision n° MED-2024-150 du 15 novembre 2024", &["Décision"]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].text, "Décision n° MED-2024-150 du 15 novembre 2024");
        assert_eq!(f[0].value, cite(Some("MED"), 2024, 150, Some((2024, 11, 15)), false));
        assert_eq!((f[0].confidence, f[0].ambiguity.clone()), (Confidence::High, None));

        let f = &keyword("mise en demeure MED-2024-150", &["mise en demeure"]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].text, "mise en demeure MED-2024-150");
        assert_eq!(f[0].value, cite(Some("MED"), 2024, 150, None, false));

        let f = &keyword("mise en demeure n° MED-2022-034 du 25 avril 2022", &["mise en demeure"]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].value, cite(Some("MED"), 2022, 34, Some((2022, 4, 25)), false));

        let f = &keyword("décision DR-2024-196 du 26 juillet 2024", &["décision"]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].value, cite(Some("DR"), 2024, 196, Some((2024, 7, 26)), false));

        // Les sept séries connues (B §4.11) sont lues sans « CNIL » ni « délibération ».
        for series in ["SAN", "MED", "MEDP", "DF", "DR", "DT", "HAB"] {
            let input = format!("décision {series}-2019-003");
            let f = keyword(&input, &["décision"]);
            assert_eq!(f.len(), 1, "{input:?} : {f:?}");
            assert_eq!(f[0].value, cite(Some(series), 2019, 3, None, false), "{input}");
        }

        // Sans série, aucun numéro n'est lu : la voie mot-clé ne lit pas les décisions
        // numérotées `AAAA-NNN` (CC, CE, communes).
        for (input, word) in [
            ("Décision n° 2023-062", "Décision"),
            ("décision n° 2023-062 du 13 avril 2023", "décision"),
            ("mise en demeure n° 2023-062", "mise en demeure"),
            ("Décision 2025-1173", "Décision"),
            ("décision n° 2020-092", "décision"),
        ] {
            assert_eq!(keyword(input, &[word]), [], "{input:?}");
        }
    }

    #[test]
    fn four_digit_orders_need_a_series() {
        for (input, series, year, seq) in [
            ("Délibération DF-2017-1448", "DF", 2017, 1448),
            ("Délibération DF-2017-1061", "DF", 2017, 1061),
            ("Délibération DF-2017-2671", "DF", 2017, 2671),
            ("Délibération n° SAN-2026-1000", "SAN", 2026, 1000),
        ] {
            let f = one(input, &["Délibération"]);
            assert_eq!(f.value, cite(Some(series), year, seq, None, false), "{input}");
            assert_eq!(f.confidence, Confidence::High, "{input}");
        }
        let f = one("Délibération n° SAN-2026-1000 du 8 janvier 2026", &["Délibération"]);
        assert_eq!(f.value, cite(Some("SAN"), 2026, 1000, Some((2026, 1, 8)), false));
        let f = &keyword("décision DF-2017-1448", &["décision"]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].value, cite(Some("DF"), 2017, 1448, None, false));
        // N15 : sans série, quatre chiffres d'ordre ne sont pas une citation ; ni cinq, ni un zéro
        // de tête.
        assert_eq!(found("Délibération n° 2025-1173", &["Délibération"]), []);
        assert_eq!(found("CNIL, délibération n° 2025-1173", &["CNIL"]), []);
        assert_eq!(found("Délibération SAN-2026-0002", &["Délibération"]), []);
        assert_eq!(found("Délibération SAN-2026-12345", &["Délibération"]), []);
        assert_eq!(keyword("décision n° 2025-1173", &["décision"]), []);
    }

    #[test]
    fn four_digit_bounds_and_leading_zero() {
        // Trois chiffres : lus. Quatre chiffres de 1000 à 9999 avec série : lus.
        for (order, seq) in [("999", 999), ("1000", 1000), ("1001", 1001), ("9999", 9999)] {
            let input = format!("Délibération SAN-2026-{order}");
            let f = one(&input, &["Délibération"]);
            assert_eq!(f.value, cite(Some("SAN"), 2026, seq, None, false), "{input}");
        }
        // Un zéro de tête ou cinq chiffres : rien, sans repli sur trois chiffres.
        for order in ["0999", "0001", "0000", "0100", "10000", "99999", "00999"] {
            let input = format!("Délibération SAN-2026-{order}");
            assert_eq!(found(&input, &["Délibération"]), [], "{input}");
        }
        // Sans série : trois chiffres exactement.
        assert_eq!(
            one("Délibération 2025-117", &["Délibération"]).value,
            cite(None, 2025, 117, None, false)
        );
        for order in ["1000", "1173", "9999", "0117"] {
            let input = format!("Délibération 2025-{order}");
            assert_eq!(found(&input, &["Délibération"]), [], "{input}");
        }
    }

    #[test]
    fn keyword_only_path_ignores_unknown_series() {
        for (input, word) in [
            ("décision MLD-2016-253", "décision"),
            ("décision PC-2023-045 du 5 mai 2023", "décision"),
            ("Décision AG-2024-001", "Décision"),
            ("mise en demeure PC-2023-045", "mise en demeure"),
            ("décision DFX-2017-100", "décision"),
            ("décision D-2017-100", "décision"),
            ("décision ABCD-2017-100", "décision"),
            ("décision san-2017-100", "décision"),
        ] {
            assert_eq!(keyword(input, &[word]), [], "{input:?}");
        }
        // La voie existante reste ouverte : « CNIL » ou « délibération » écrits, toute série
        // de deux à quatre majuscules est lue, comme en 1.1.0.
        let f = one("CNIL, décision MLD-2016-253", &["CNIL"]);
        assert_eq!((f.confidence, f.ambiguity), (Confidence::High, None));
        assert_eq!(f.value, cite(Some("MLD"), 2016, 253, None, false));
        assert_eq!(one("délibération PC-2023-045", &["délibération"]).confidence, Confidence::High);
        // La même citation, avec les vraies ancres : lue derrière « CNIL », silencieuse sans.
        assert_eq!(real("CNIL, décision MLD-2016-253").len(), 1);
        assert_eq!(real("décision MLD-2016-253"), []);
    }

    #[test]
    fn the_keyword_anchor_reads_only_the_series_path() {
        // Même avec un mot-clé quelconque sous l'ancre, un numéro sans série n'est pas lu.
        assert_eq!(keyword("Délibération 2023-062", &["Délibération"]), []);
        assert_eq!(keyword("Délibération SAN-2026-002", &["Délibération"]).len(), 1);
        // Une ancre qui ne désigne rien, ou aucune ancre, ne lit rien.
        assert_eq!(keyword("décision MED-2024-150", &["absent"]), []);
        assert_eq!(scan(&normalize("é", Steps::default()), &[]), []);
        // Les ancres `Family` et `Sigle` lisent toujours les numéros sans série (voie existante).
        assert_eq!(found("décision n° 2023-062 de la CNIL", &["décision"]).len(), 1);
    }

    #[test]
    fn one_candidate_when_both_reads_see_the_same_citation() {
        let input = "CNIL, décision n° MED-2018-024";
        let both = find_pairs(
            scan,
            input,
            &[(AnchorClass::Family, "CNIL"), (AnchorClass::CnilKeyword, "décision")],
        );
        assert_eq!(both.len(), 1, "{both:?}");
        assert_eq!(both[0].text, input);
        assert_eq!(both[0].value, cite(Some("MED"), 2018, 24, None, false));
        // Avec les vraies ancres (`CNIL`, `décision` en nature de texte et en mot-clé).
        assert_eq!(real(input).len(), 1);
        assert_eq!(real("Délibération SAN-2026-002 du 8 janvier 2026").len(), 1);
    }

    #[test]
    fn both_reads_are_merged_in_text_order() {
        // La voie mot-clé lit la première citation, la voie existante la seconde, plus de quatre-
        // vingts caractères plus loin : l'ordre du texte est conservé.
        let filler =
            "Le texte qui suit rappelle longuement le contexte de cette affaire pour séparer. ";
        let input = format!(
            "décision DR-2024-196 du 26 juillet 2024. {filler}Puis CNIL, délibération n° 2023-062 du 13 avril 2023."
        );
        let read = real(&input);
        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read[0].0, "décision DR-2024-196 du 26 juillet 2024");
        assert_eq!(read[0].1, cite(Some("DR"), 2024, 196, Some((2024, 7, 26)), false));
        assert_eq!(read[1].0, "CNIL, délibération n° 2023-062 du 13 avril 2023");
        assert_eq!(read[1].1, cite(None, 2023, 62, Some((2023, 4, 13)), false));
        // Deux mots-clés distants, deux citations, sans doublon.
        let two = format!("décision DR-2024-196. {filler}Mise en demeure MED-2022-034.");
        assert_eq!(real(&two).len(), 2);
        // La même citation écrite deux fois reste deux citations (le doublon est de plage ET de
        // valeur, pas de valeur seule).
        let twice = format!("décision DR-2024-196. {filler}Puis la décision DR-2024-196.");
        assert_eq!(real(&twice).len(), 2, "{:?}", real(&twice));
    }
}
