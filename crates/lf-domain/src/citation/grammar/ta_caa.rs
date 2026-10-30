//! Grammaire des cours administratives d'appel et des tribunaux administratifs (annexe B §4.8).
//!
//! Forme directe (`CAA de Marseille, 4ème chambre, 09/06/2026, 24MA02429`) et forme inverse du
//! corps d'une décision (`arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de
//! Nancy`, `jugement nos 2005064 du 30 juin 2022, le tribunal administratif`). Un code de
//! numéro incohérent avec le siège cité lève `caa_code_siege_incoherent`. Les plages des
//! candidats sont des offsets du texte d'ORIGINE (`Normalized::orig_range`), celles des ancres
//! des offsets de `Normalized::text`.

#![allow(
    clippy::many_single_char_names,
    reason = "lecteur à curseur : `c` (fenêtre) et `i`, `s`, `p`, `e` (positions), comme dans la grammaire de l'annexe B"
)]

use crate::citation::dict::courts::{CaaCode, CaaSeat, TaSeat};
use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{CaaNumber, CaseNumber, Court, DecisionKind, Flag, TaNumber};

use super::Candidate;
use super::cassation::lex::{self, Cur, Hit};

/// Classes d'ancres où une décision administrative peut commencer.
const CLASSES: [AnchorClass; 3] = [AnchorClass::Court, AnchorClass::Sigle, AnchorClass::CaaNumber];

/// Grammaires de la famille, essayées à chaque début.
const GRAMMARS: [lex::Grammar; 4] = [caa, caa_reverse, ta, ta_reverse];

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    lex::scan_family(n, anchors, &CLASSES, None, &GRAMMARS)
}

/// `de` ou rien, puis les espaces : `CAA de Lyon`, `TA de Lyon`.
fn optional_de(c: &Cur<'_>, p: usize) -> usize {
    c.kwb(p, "de").and_then(|x| c.ws(x)).unwrap_or(p)
}

/// `adm_chamber` : fins possibles, de la plus longue à la plus courte.
fn adm_chambers(c: &Cur<'_>, i: usize) -> Vec<usize> {
    c.ordinal_chamber_ends(i)
}

/// Le plus long siège reconnu par `seat_of` à partir de `p` : fin et siège.
fn seat_at<S>(c: &Cur<'_>, p: usize, seat_of: impl Fn(&str) -> Option<S>) -> Option<(usize, S)> {
    c.place_ends(p).into_iter().find_map(|e| seat_of(&c.key_text(p, e)).map(|seat| (e, seat)))
}

/// `caa_num` : `24MA02429`, code de cour connu. Fin de la lecture et numéro.
fn caa_number(c: &Cur<'_>, i: usize) -> Option<(usize, CaaNumber)> {
    c.left_ok(i).then_some(())?;
    let yy = c.digits(i, 2, 2)?;
    let court = CaaCode::parse(&c.text(yy, yy + 2))?;
    let end = c.digits(yy + 2, 5, 5).filter(|&e| c.num_end_ok(e))?;
    Some((
        end,
        CaaNumber { yy: u8::try_from(c.val(i, yy)).unwrap_or(0), court, seq: c.val(yy + 2, end) },
    ))
}

/// `[NUM_SIGN] caa_num`.
fn caa_numbers(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let p = c.num_sign(i, false).map_or(i, |(e, _)| e);
    caa_number(c, p).map(|(e, n)| (e, vec![CaseNumber::Caa(n)]))
}

/// `ta_num` : sept chiffres.
fn ta_one(c: &Cur<'_>, i: usize) -> Option<(usize, CaseNumber)> {
    c.left_ok(i).then_some(())?;
    let end = c.digits(i, 7, 7).filter(|&e| c.num_end_ok(e))?;
    Some((end, CaseNumber::Ta(TaNumber(c.val(i, end)))))
}

/// `[NUM_SIGN | NUM_SIGN_PL] ta_num { LIST_SEP ta_num }`.
fn ta_numbers(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let p = c.num_sign(i, true).map_or(i, |(e, _)| e);
    lex::number_list(c, p, ta_one)
}

/// Drapeau d'un code de requête qui n'est pas celui du siège cité.
fn code_flags(seat: CaaSeat, numbers: &[CaseNumber]) -> Option<Flag> {
    numbers
        .iter()
        .any(|n| matches!(n, CaseNumber::Caa(n) if seat.code() != Some(n.court)))
        .then_some(Flag::CaaCodeSiegeIncoherent)
}

/// `caa_court WS` : `CAA [de]`, `C.A.A. [de]`, `Cour administrative d'appel de|d'`.
fn caa_court(c: &Cur<'_>, i: usize) -> Option<usize> {
    c.litb(i, "CAA")
        .or_else(|| c.lit(i, "C.A.A."))
        .and_then(|e| c.ws(e))
        .map(|p| optional_de(c, p))
        .or_else(|| c.kw(i, "cour administrative d'appel").and_then(|e| c.de(e)))
}

/// `caa_decision` directe : `CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429`.
fn caa(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let found =
        lex::at_place(c, caa_court(c, s)?, adm_chambers, caa_numbers, CaaSeat::parse_folded)?;
    let court = Court::Caa { seat: Some(found.seat), chamber: found.chamber.clone() };
    let flag = code_flags(found.seat, &found.numbers);
    Some(found.hit(s, court).with_flags(flag))
}

/// `arrêt|ordonnance NUM_SIGN caa_num du DATE [,] [la] cour administrative d'appel de|d' siège`.
fn caa_reverse(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (e, kind) = c
        .kwb(s, "arret")
        .map(|e| (e, DecisionKind::Arret))
        .or_else(|| c.kwb(s, "ordonnance").map(|e| (e, DecisionKind::Ordonnance)))?;
    let (q, _) = c.num_sign(c.ws(e)?, false)?;
    let (e, number) = caa_number(c, q)?;
    let date = c.date(c.ws(c.kwb(c.ws(e)?, "du")?)?)?;
    let after = c.ws0(date.end);
    let after = if c.is(after, ',') { c.ws0(after + 1) } else { after };
    let after = c.kwb(after, "la").and_then(|x| c.ws(x)).unwrap_or(after);
    let p = c.de(c.kw(after, "cour administrative d'appel")?)?;
    let (end, seat) = seat_at(c, p, CaaSeat::parse_folded)?;
    let mut decision = lex::decision(Some(Court::Caa { seat: Some(seat), chamber: None }));
    decision.date = date.date;
    decision.kind = Some(kind);
    decision.numbers = vec![CaseNumber::Caa(number)];
    let flag = code_flags(seat, &decision.numbers);
    Some(Hit::decision(s, end, decision).with_flags(date.flags).with_flags(flag))
}

/// `ta_court WS` : `TA [de]`, `T.A. [de]`, `trib. adm.`, `Tribunal administratif de|d'`.
fn ta_court(c: &Cur<'_>, i: usize) -> Option<usize> {
    c.litb(i, "TA")
        .or_else(|| c.lit(i, "T.A."))
        .or_else(|| c.abbr(i, "trib").and_then(|e| c.abbr(c.ws(e)?, "adm")))
        .and_then(|e| c.ws(e))
        .map(|p| optional_de(c, p))
        .or_else(|| c.kw(i, "tribunal administratif").and_then(|e| c.de(e)))
}

/// `ta_decision` directe : `TA Strasbourg, 30 juin 2022, n° 2005064`.
fn ta(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let found = lex::at_place(c, ta_court(c, s)?, adm_chambers, ta_numbers, TaSeat::parse_folded)?;
    let court = Court::Ta { seat: Some(found.seat), chamber: found.chamber.clone() };
    Some(found.hit(s, court))
}

/// `jugement NUM_SIGN ta_num … du DATE [SEP] [le] tribunal administratif [de|d' siège]`.
fn ta_reverse(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let e = c.kwb(s, "jugement")?;
    let (q, _) = c.num_sign(c.ws(e)?, true)?;
    let (e, numbers) = lex::number_list(c, q, ta_one)?;
    let date = c.date(c.ws(c.kwb(c.ws(e)?, "du")?)?)?;
    let after = c.sep(date.end).unwrap_or(date.end);
    let after = c.kwb(after, "le").and_then(|x| c.ws(x)).unwrap_or(after);
    let court_end = c.kwb(after, "tribunal administratif")?;
    let seated = c.de(court_end).and_then(|p| seat_at(c, p, TaSeat::parse_folded));
    let end = seated.map_or(court_end, |(e, _)| e);
    let mut decision =
        lex::decision(Some(Court::Ta { seat: seated.map(|(_, seat)| seat), chamber: None }));
    decision.date = date.date;
    decision.kind = Some(DecisionKind::Jugement);
    decision.numbers = numbers;
    Some(Hit::decision(s, end, decision).with_flags(date.flags))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::super::cassation::testing::{date, decision, matched, only, run, with_court};
    use super::*;
    use crate::citation::dict::courts::{CaaCode, CaaSeat, TaSeat};
    use crate::citation::scan::AnchorClass::{CaaNumber as CaaNumberA, Court as CourtA, Sigle};
    use crate::citation::types::{
        CaaNumber, CaseNumber, Confidence, Court, DecisionCite, DecisionKind, Flag, TaNumber,
    };

    fn caa_court(seat: &str, chamber: Option<&str>) -> DecisionCite {
        with_court(Court::Caa {
            seat: CaaSeat::parse_folded(seat),
            chamber: chamber.map(str::to_owned),
        })
    }

    fn ta_court(seat: Option<&str>, chamber: Option<&str>) -> DecisionCite {
        with_court(Court::Ta {
            seat: seat.and_then(TaSeat::parse_folded),
            chamber: chamber.map(str::to_owned),
        })
    }

    fn caa_no(yy: u8, court: CaaCode, seq: u32) -> CaseNumber {
        CaseNumber::Caa(CaaNumber { yy, court, seq })
    }

    fn ta_no(n: u32) -> CaseNumber {
        CaseNumber::Ta(TaNumber(n))
    }

    #[test]
    fn caa_forward_forms_p121_p122_p133() {
        let cases = [
            (
                "CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429",
                (Sigle, "CAA"),
                DecisionCite {
                    date: date(2026, 6, 9),
                    numbers: vec![caa_no(24, CaaCode::Ma, 2429)],
                    ..caa_court("marseille", Some("4ème chambre"))
                },
            ),
            (
                "CAA de NANCY, 3ème chambre, 22/05/2025, 22NC02259",
                (Sigle, "CAA"),
                DecisionCite {
                    date: date(2025, 5, 22),
                    numbers: vec![caa_no(22, CaaCode::Nc, 2259)],
                    ..caa_court("nancy", Some("3ème chambre"))
                },
            ),
            (
                "CAA Nancy, 3e ch., 22 mai 2025, n° 22NC02259",
                (Sigle, "CAA"),
                DecisionCite {
                    date: date(2025, 5, 22),
                    numbers: vec![caa_no(22, CaaCode::Nc, 2259)],
                    ..caa_court("nancy", Some("3e ch."))
                },
            ),
            (
                "C.A.A. de Lyon, 1re chambre - formation à trois, 4 juin 2020, 20LY00123",
                (CourtA, "C.A.A."),
                DecisionCite {
                    date: date(2020, 6, 4),
                    numbers: vec![caa_no(20, CaaCode::Ly, 123)],
                    ..caa_court("lyon", Some("1re chambre - formation à trois"))
                },
            ),
            (
                "Cour administrative d'appel de Paris, 4 juin 2020, n° 20PA00123",
                (CourtA, "Cour administrative d'appel"),
                DecisionCite {
                    date: date(2020, 6, 4),
                    numbers: vec![caa_no(20, CaaCode::Pa, 123)],
                    ..caa_court("paris", None)
                },
            ),
        ];
        for (input, (class, needle), want) in cases {
            let got = only(run(scan, input, &[(class, needle)]));
            assert_eq!(matched(input, &got), input);
            assert_eq!(*decision(&got), want, "{input}");
            assert_eq!(got.confidence, Confidence::High, "{input}");
        }
    }

    #[test]
    fn caa_code_and_seat_must_agree() {
        let input = "CAA de Lyon, 4 juin 2024, n° 24MA02429";
        let got = only(run(scan, input, &[(Sigle, "CAA")]));
        assert_eq!(got.flags.list, [Flag::CaaCodeSiegeIncoherent]);
        assert_eq!(decision(&got).numbers, [caa_no(24, CaaCode::Ma, 2429)]);
        let input = "CAA de Lyon, 4 juin 2024, n° 24LY02429";
        assert_eq!(only(run(scan, input, &[(Sigle, "CAA")])).flags.list, []);
    }

    #[test]
    fn caa_reverse_form_p123() {
        let input = "Par un arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy a, sur appel";
        let got = only(run(scan, input, &[(CaaNumberA, "22NC02259")]));
        assert_eq!(
            matched(input, &got),
            "arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy"
        );
        assert_eq!(
            *decision(&got),
            DecisionCite {
                date: date(2025, 5, 22),
                kind: Some(DecisionKind::Arret),
                numbers: vec![caa_no(22, CaaCode::Nc, 2259)],
                ..caa_court("nancy", None)
            }
        );
        let input =
            "une ordonnance n° 22PA01234 du 31 juin 2025, cour administrative d'appel de Lyon";
        let got = only(run(scan, input, &[(CaaNumberA, "22PA01234")]));
        assert_eq!(decision(&got).kind, Some(DecisionKind::Ordonnance));
        assert_eq!(decision(&got).date, None);
        assert_eq!(got.flags.list, [Flag::DateInvalide, Flag::CaaCodeSiegeIncoherent]);
    }

    #[test]
    fn ta_forward_forms_p132() {
        let cases = [
            (
                "TA Strasbourg, 30 juin 2022, n° 2005064",
                (Sigle, "TA"),
                DecisionCite {
                    date: date(2022, 6, 30),
                    numbers: vec![ta_no(2_005_064)],
                    ..ta_court(Some("strasbourg"), None)
                },
            ),
            (
                "T.A. de Lyon, 2e chambre, 30 juin 2022, nos 2005064, 2006015 et 2006016",
                (CourtA, "T.A."),
                DecisionCite {
                    date: date(2022, 6, 30),
                    numbers: vec![ta_no(2_005_064), ta_no(2_006_015), ta_no(2_006_016)],
                    ..ta_court(Some("lyon"), Some("2e chambre"))
                },
            ),
            (
                "Tribunal administratif de Nîmes, 30/06/2022, 2005064",
                (CourtA, "Tribunal administratif"),
                DecisionCite {
                    date: date(2022, 6, 30),
                    numbers: vec![ta_no(2_005_064)],
                    ..ta_court(Some("nimes"), None)
                },
            ),
            (
                "Tribunal administratif d'Orléans, du 30 juin 2022, n° 2005064",
                (CourtA, "Tribunal administratif"),
                DecisionCite {
                    date: date(2022, 6, 30),
                    numbers: vec![ta_no(2_005_064)],
                    ..ta_court(Some("orleans"), None)
                },
            ),
            (
                "trib. adm. Paris, 30 juin 2022, 2005064",
                (CourtA, "trib. adm."),
                DecisionCite {
                    date: date(2022, 6, 30),
                    numbers: vec![ta_no(2_005_064)],
                    ..ta_court(Some("paris"), None)
                },
            ),
        ];
        for (input, (class, needle), want) in cases {
            let got = only(run(scan, input, &[(class, needle)]));
            assert_eq!(matched(input, &got), input);
            assert_eq!(*decision(&got), want, "{input}");
        }
    }

    #[test]
    fn ta_reverse_form_p124_has_no_seat_unless_written() {
        let input = "Par un jugement nos 2005064, 2006015, 2006016 du 30 juin 2022, le tribunal administratif a rejeté sa demande.";
        let got = only(run(scan, input, &[(CourtA, "tribunal administratif")]));
        assert_eq!(
            matched(input, &got),
            "jugement nos 2005064, 2006015, 2006016 du 30 juin 2022, le tribunal administratif"
        );
        assert_eq!(
            *decision(&got),
            DecisionCite {
                date: date(2022, 6, 30),
                kind: Some(DecisionKind::Jugement),
                numbers: vec![ta_no(2_005_064), ta_no(2_006_015), ta_no(2_006_016)],
                ..ta_court(None, None)
            }
        );
        let input = "un jugement n° 2005064 du 31 juin 2022 tribunal administratif de Lyon, puis";
        let got = only(run(scan, input, &[(CourtA, "tribunal administratif")]));
        assert_eq!(
            matched(input, &got),
            "jugement n° 2005064 du 31 juin 2022 tribunal administratif de Lyon"
        );
        assert_eq!(decision(&got).court, ta_court(Some("lyon"), None).court);
        assert_eq!(got.flags.list, [Flag::DateInvalide]);
        // « de » suivi d'un lieu inconnu : le siège reste vide et « de » n'est pas repris.
        let input = "jugement n° 2005064 du 30 juin 2022, le tribunal administratif de Atlantis";
        let got = only(run(scan, input, &[(CourtA, "tribunal administratif")]));
        assert_eq!(
            matched(input, &got),
            "jugement n° 2005064 du 30 juin 2022, le tribunal administratif"
        );
    }

    #[test]
    fn mentions_without_date_or_number_are_ignored_n30() {
        for (input, anchors) in [
            ("le TA Paris a été saisi", vec![(Sigle, "TA")]),
            ("TA Paris, 30 juin 2022", vec![(Sigle, "TA")]),
            ("TA Atlantis, 30 juin 2022, n° 2005064", vec![(Sigle, "TA")]),
            ("TA Paris, 30 juin 2022, n° 20050645", vec![(Sigle, "TA")]),
            ("CAA Lyon, 4 juin 2020, n° 20XX00123", vec![(Sigle, "CAA")]),
            ("CAA Atlantis, 4 juin 2020, n° 20LY00123", vec![(Sigle, "CAA")]),
            ("CAA, 4 juin 2020, n° 20LY00123", vec![(Sigle, "CAA")]),
            ("arrêt n° 22NC02259 du 22 mai 2025", vec![(CaaNumberA, "22NC02259")]),
            ("jugement n° 2005064 du 30 juin 2022", vec![(CourtA, "jugement")]),
            ("TATA Paris, 30 juin 2022, n° 2005064", vec![(Sigle, "TATA")]),
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
    }

    /// Un numéro CAA ou TA collé au signe « no » (sans espace) n'est pas lu, ni un numéro CAA
    /// sans code de cour, trop court ou trop long.
    #[test]
    fn malformed_case_numbers_give_no_citation() {
        for (input, anchors) in [
            ("CAA Lyon, 4 juin 2020, no20LY00123", vec![(Sigle, "CAA")]),
            ("CAA Lyon, 4 juin 2020, n° LY00123", vec![(Sigle, "CAA")]),
            ("CAA Lyon, 4 juin 2020, n° 20LY0012", vec![(Sigle, "CAA")]),
            ("CAA Lyon, 4 juin 2020, n° 20LY001234", vec![(Sigle, "CAA")]),
            ("TA Paris, 30 juin 2022, no2005064", vec![(Sigle, "TA")]),
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
    }

    /// La forme inverse d'un arrêt exige chaque morceau, dans l'ordre : mot, signe de numéro,
    /// numéro, « du », date, puis la cour et un siège connu.
    #[test]
    fn caa_reverse_form_needs_every_part() {
        let anchors = [(CaaNumberA, "22NC02259")];
        for input in [
            "arrêt, n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy",
            "arrêt 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy",
            "arrêt n° 22NC02259, du 22 mai 2025 la cour administrative d'appel de Nancy",
            "arrêt n° 22NC02259 le 22 mai 2025 la cour administrative d'appel de Nancy",
            "arrêt n° 22NC02259 du-22 mai 2025 la cour administrative d'appel de Nancy",
            "arrêt n° 22NC02259 du demain la cour administrative d'appel de Nancy",
            "arrêt n° 22NC02259 du 22 mai 2025 la juridiction de Nancy",
            "arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel Nancy",
            "arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Atlantis",
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
        // Un code de cour inconnu n'est pas un numéro CAA.
        let input = "arrêt n° 22XX02259 du 22 mai 2025 la cour administrative d'appel de Nancy";
        assert_eq!(run(scan, input, &[(CaaNumberA, "22XX02259")]), []);
    }

    /// Même exigence pour la forme inverse d'un jugement.
    #[test]
    fn ta_reverse_form_needs_every_part() {
        let anchors = [(CourtA, "tribunal administratif")];
        for input in [
            "jugement, n° 2005064 du 30 juin 2022, le tribunal administratif",
            "jugement 2005064 du 30 juin 2022, le tribunal administratif",
            "jugement n° 20050 du 30 juin 2022, le tribunal administratif",
            "jugement n° 2005064, du 30 juin 2022, le tribunal administratif",
            "jugement n° 2005064 le 30 juin 2022, le tribunal administratif",
            "jugement n° 2005064 du-30 juin 2022, le tribunal administratif",
            "jugement n° 2005064 du demain, le tribunal administratif",
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
    }

    /// `trib.adm.` sans espace après `trib.` n'est pas l'abréviation du tribunal.
    #[test]
    fn trib_adm_needs_a_space_after_trib() {
        let input = "trib.adm. Paris, 30 juin 2022, 2005064";
        assert_eq!(run(scan, input, &[(CourtA, "trib")]), []);
    }
}
