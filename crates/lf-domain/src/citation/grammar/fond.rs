//! Grammaire des juridictions judiciaires du fond (CA, TJ, autres) (annexe B §4.6).
//!
//! Cour d'appel (`ca_decision`), tribunal judiciaire (`tj_decision`), tribunal de commerce et
//! conseil de prud'hommes (`autre_judic`) suivent le même fil : juridiction, lieu, chambre
//! facultative (cour d'appel seulement), date, numéro. L'en-tête `N° RG … - N° Portalis …`
//! (P114) n'a pas de juridiction. Les plages des candidats sont des offsets du texte d'ORIGINE
//! (`Normalized::orig_range`), celles des ancres des offsets de `Normalized::text`.

#![allow(
    clippy::many_single_char_names,
    reason = "lecteur à curseur : `c` (fenêtre) et `i`, `s`, `p`, `e` (positions), comme dans la grammaire de l'annexe B"
)]

use crate::citation::dict::courts::CaSeat;
use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{Ambiguity, CaseNumber, Confidence, Court, RgNumber};

use super::Candidate;
use super::cassation::lex::{self, Cur, Hit};

/// Classes d'ancres où une juridiction du fond peut commencer.
const CLASSES: [AnchorClass; 3] = [AnchorClass::Court, AnchorClass::Sigle, AnchorClass::Rg];

/// Grammaires de la famille, essayées à chaque début.
const GRAMMARS: [lex::Grammar; 5] = [appeal, tribunal, commerce, prud_hommes, registry_header];

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    lex::scan_family(n, anchors, &CLASSES, None, &GRAMMARS)
}

/// `rg` : `AA/NNNN` ou `AA/NNNNN`.
fn rg(c: &Cur<'_>, i: usize) -> Option<(usize, CaseNumber)> {
    if !c.left_ok(i) {
        return None;
    }
    let yy = c.digits(i, 2, 2).filter(|&e| c.is(e, '/'))?;
    let end = c.digits(yy + 1, 4, 5).filter(|&e| c.num_end_ok(e))?;
    Some((
        end,
        CaseNumber::Rg(RgNumber {
            yy: u8::try_from(c.val(i, yy)).unwrap_or(0),
            seq: c.val(yy + 1, end),
        }),
    ))
}

/// `rg_part` : `[NUM_SIGN] ['RG' WS] rg`.
fn rg_part(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let p = c.num_sign(i, false).map_or(i, |(e, _)| e);
    rg(c, c.litb(p, "RG").and_then(|e| c.ws(e)).unwrap_or(p)).map(|(e, n)| (e, vec![n]))
}

/// Numéro d'un tribunal de commerce : `/[0-9]{4}[A-Z]?[0-9]{5,6}/`, gardé brut.
fn commerce_number(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let p = c.num_sign(i, false).map_or(i, |(e, _)| e);
    if !c.left_ok(p) {
        return None;
    }
    let year = c.digits(p, 4, 4)?;
    let letter = c.ch(year).is_some_and(|ch| ch.is_ascii_uppercase());
    let end = c.digits(year + usize::from(letter), 5, 6).filter(|&e| c.num_end_ok(e))?;
    Some((end, vec![CaseNumber::Unknown(c.text(p, end))]))
}

/// `portalis` : `35L7-V-B7H-CIDHB`, en majuscules et chiffres.
fn portalis(c: &Cur<'_>, i: usize) -> Option<usize> {
    let segment = |p: usize, n: usize, digits: bool| {
        (0..n)
            .all(|k| {
                c.ch(p + k)
                    .is_some_and(|ch| ch.is_ascii_uppercase() || (digits && ch.is_ascii_digit()))
            })
            .then_some(p + n)
    };
    let dash = |p: usize| c.is(p, '-').then_some(p + 1);
    let a = segment(i, 4, true)?;
    let b = segment(dash(a)?, 1, false)?;
    let d = segment(dash(b)?, 3, true)?;
    segment(dash(d)?, 5, true).filter(|&e| c.bound(e))
}

/// Pas de chambre (tribunaux judiciaires et autres juridictions).
fn no_chambers(_c: &Cur<'_>, _i: usize) -> Vec<usize> {
    Vec::new()
}

/// `ca_chamber` : fins possibles, de la plus longue à la plus courte.
fn chamber_ends(c: &Cur<'_>, i: usize) -> Vec<usize> {
    let pole = c.kwb(i, "pole").and_then(|e| {
        let (e, _) = c.int(c.ws(e)?)?;
        let dash = c.ws0(e);
        if c.is(dash, '-') {
            let q = c.ws(c.kwb(c.ws0(dash + 1), "chambre")?)?;
            return c.int(q).map(|(end, _)| end);
        }
        let q = c.sep(e)?;
        let k = c.kwb(q, "chambre").or_else(|| c.abbr(q, "ch"))?;
        c.int(c.ws(k)?).map(|(end, _)| end)
    });
    let sectioned = c.kwb(i, "chambre").and_then(|e| {
        let (e, _) = c.int(c.ws(e)?)?;
        let (e, _) = c.int(c.ws(c.kwb(c.ws(e)?, "section")?)?)?;
        Some(e)
    });
    pole.or(sectioned).map_or_else(|| c.ordinal_chamber_ends(i), |end| vec![end])
}

/// `ca_decision` : `Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215`.
fn appeal(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let p = c
        .litb(s, "CA")
        .and_then(|e| c.ws(e))
        .or_else(|| c.kw(s, "c.a.").map(|e| c.ws0(e)))
        .or_else(|| c.kw(s, "cour d'appel").and_then(|e| c.de(e)))?;
    let found = lex::at_place(c, p, chamber_ends, rg_part, CaSeat::parse_folded)?;
    let court = Court::CourAppel { seat: Some(found.seat), chamber: found.chamber.clone() };
    Some(found.hit(s, court))
}

/// `tj_decision` : `Tribunal judiciaire de Lille, 2026-04-30, n° 25/00497`.
fn tribunal(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let p = c
        .litb(s, "TJ")
        .and_then(|e| c.ws(e))
        .or_else(|| c.kw(s, "tribunal judiciaire").and_then(|e| c.de(e)))?;
    let found = lex::at_place(c, p, no_chambers, rg_part, |key| Some(key.to_uppercase()))?;
    let court = Court::TribunalJudiciaire { seat: Some(found.seat.clone()) };
    Some(found.hit(s, court))
}

/// Autre juridiction du fond : libellé gardé tel qu'écrit, jusqu'au lieu compris.
fn other_court(c: &Cur<'_>, s: usize, p: usize, number: lex::NumbersFn) -> Option<Hit> {
    let found = lex::at_place(c, p, no_chambers, number, |key| Some(key.to_owned()))?;
    let court = Court::AutreJudiciaire { label: c.text(s, found.seat_end) };
    Some(found.hit(s, court))
}

/// `autre_judic` : tribunal de commerce.
fn commerce(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let p = c
        .kw(s, "t")
        .and_then(|e| c.abbr(c.dot_ws(e), "com"))
        .and_then(|e| c.ws(e))
        .or_else(|| c.kw(s, "tribunal de commerce").and_then(|e| c.de(e)))?;
    other_court(c, s, p, commerce_number)
}

/// `autre_judic` : conseil de prud'hommes.
fn prud_hommes(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let p = c
        .litb(s, "CPH")
        .and_then(|e| c.ws(e))
        .or_else(|| c.kw(s, "conseil de prud'hommes").and_then(|e| c.de(e)))?;
    other_court(c, s, p, rg_part)
}

/// En-tête `N° RG 23/13690 [- N° Portalis …]` : la juridiction n'est pas écrite (P114).
fn registry_header(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (p, _) = c.num_sign(s, false)?;
    let (mut end, first) = rg(c, c.ws(c.litb(p, "RG")?)?)?;
    let mut numbers = vec![first];
    let portalis_part = c.ws(end).filter(|&w| c.is(w, '-')).and_then(|w| {
        let (q, _) = c.num_sign(c.ws0(w + 1), false)?;
        let q = c.ws(c.kwb(q, "portalis")?)?;
        portalis(c, q).map(|e| (e, c.text(q, e)))
    });
    if let Some((e, code)) = portalis_part {
        numbers.push(CaseNumber::Portalis(code));
        end = e;
    }
    let mut decision = lex::decision(None);
    decision.numbers = numbers;
    Some(
        Hit::decision(s, end, decision)
            .with_confidence(Confidence::Medium)
            .with_ambiguity(Ambiguity::JuridictionManquante),
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::super::cassation::testing::{blank, date, decision, matched, only, run, with_court};
    use super::*;
    use crate::citation::dict::courts::CaSeat;
    use crate::citation::scan::AnchorClass::{Court as CourtA, Rg, Sigle};
    use crate::citation::types::{
        Ambiguity, CaseNumber, Confidence, Court, DecisionCite, Flag, RgNumber,
    };

    fn rg(yy: u8, seq: u32) -> CaseNumber {
        CaseNumber::Rg(RgNumber { yy, seq })
    }

    fn ca(seat: &str, chamber: Option<&str>) -> DecisionCite {
        with_court(Court::CourAppel {
            seat: CaSeat::parse_folded(seat),
            chamber: chamber.map(str::to_owned),
        })
    }

    #[test]
    fn appeal_court_forms_p110_to_p113() {
        let cases = [
            (
                "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215",
                vec![(CourtA, "Cour d'appel")],
                DecisionCite {
                    date: date(2025, 6, 23),
                    numbers: vec![rg(24, 15215)],
                    ..ca("paris", Some("Pôle 1 - Chambre 12"))
                },
            ),
            (
                "Cour d'appel de Versailles, 6e Chambre, 4 juin 2020, n°17/04940",
                vec![(CourtA, "Cour d'appel")],
                DecisionCite {
                    date: date(2020, 6, 4),
                    numbers: vec![rg(17, 4940)],
                    ..ca("versailles", Some("6e Chambre"))
                },
            ),
            (
                "Cour d'appel de Limoges, 21 mai 2015, 14/00554",
                vec![(CourtA, "Cour d'appel"), (Rg, "14/00554")],
                DecisionCite {
                    date: date(2015, 5, 21),
                    numbers: vec![rg(14, 554)],
                    ..ca("limoges", None)
                },
            ),
            (
                "CA Paris, pôle 5, ch. 9, 4 avr. 2024, n° 23/13690",
                vec![(Sigle, "CA")],
                DecisionCite {
                    date: date(2024, 4, 4),
                    numbers: vec![rg(23, 13690)],
                    ..ca("paris", Some("pôle 5, ch. 9"))
                },
            ),
            (
                "C.A. Aix-en-Provence, 1re chambre civile, 2 mars 2021, RG 20/00012",
                vec![(CourtA, "C.A.")],
                DecisionCite {
                    date: date(2021, 3, 2),
                    numbers: vec![rg(20, 12)],
                    ..ca("aix-en-provence", Some("1re chambre civile"))
                },
            ),
            (
                "Cour d'appel d'Agen, 3 mai 2019, n° RG 18/00321",
                vec![(CourtA, "Cour d'appel")],
                DecisionCite {
                    date: date(2019, 5, 3),
                    numbers: vec![rg(18, 321)],
                    ..ca("agen", None)
                },
            ),
        ];
        for (input, anchors, want) in cases {
            let got = only(run(scan, input, &anchors));
            assert_eq!(matched(input, &got), input);
            assert_eq!(*decision(&got), want, "{input}");
            assert_eq!(got.confidence, Confidence::High, "{input}");
        }
        let input = "Cour d'appel de Rennes, chambre 1 section 2, 4 juin 2020, 14/00554";
        let got = only(run(scan, input, &[(CourtA, "Cour d'appel")]));
        assert_eq!(
            *decision(&got),
            DecisionCite {
                date: date(2020, 6, 4),
                numbers: vec![rg(14, 554)],
                ..ca("rennes", Some("chambre 1 section 2"))
            }
        );
        let input = "CA Saint-Denis de La Réunion, 4 juin 2020, 17/04940";
        let got = only(run(scan, input, &[(Sigle, "CA")]));
        assert_eq!(decision(&got).court, ca("saint-denis de la reunion", None).court);
        assert_eq!(
            CaSeat::parse_folded("saint-denis de la reunion").map(CaSeat::as_str),
            Some("ST-DENIS-REUNION")
        );
    }

    #[test]
    fn registry_header_p114_keeps_the_court_unknown() {
        let input = "Numéro d'inscription au répertoire général : N° RG 23/13690 - N° Portalis 35L7-V-B7H-CIDHB";
        let got = only(run(scan, input, &[(Rg, "23/13690")]));
        assert_eq!(matched(input, &got), "N° RG 23/13690 - N° Portalis 35L7-V-B7H-CIDHB");
        assert_eq!(
            *decision(&got),
            DecisionCite {
                numbers: vec![rg(23, 13690), CaseNumber::Portalis("35L7-V-B7H-CIDHB".into())],
                ..blank()
            }
        );
        assert_eq!(got.flags.ambiguity, Some(Ambiguity::JuridictionManquante));
        assert_eq!(got.confidence, Confidence::Medium);
        // Sans Portalis : le RG seul suffit.
        let short = "N° RG 23/13690, puis";
        let got = only(run(scan, short, &[(Rg, "23/13690")]));
        assert_eq!(matched(short, &got), "N° RG 23/13690");
        assert_eq!(decision(&got).numbers, [rg(23, 13690)]);
    }

    #[test]
    fn judicial_tribunal_and_other_courts() {
        let input = "Tribunal judiciaire de Lille, 2026-04-30, n° 25/00497";
        let got = only(run(scan, input, &[(CourtA, "Tribunal judiciaire")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(
            *decision(&got),
            DecisionCite {
                date: date(2026, 4, 30),
                numbers: vec![rg(25, 497)],
                ..with_court(Court::TribunalJudiciaire { seat: Some("LILLE".into()) })
            }
        );
        let input = "TJ Saint-Étienne, 4 juin 2025, RG 24/01234";
        let got = only(run(scan, input, &[(Sigle, "TJ")]));
        assert_eq!(
            decision(&got).court,
            Some(Court::TribunalJudiciaire { seat: Some("SAINT-ETIENNE".into()) })
        );
        let input = "T. com. Marseille, 3 mars 2019, n° 2019J00123";
        let got = only(run(scan, input, &[(CourtA, "T. com.")]));
        assert_eq!(
            *decision(&got),
            DecisionCite {
                date: date(2019, 3, 3),
                numbers: vec![CaseNumber::Unknown("2019J00123".into())],
                ..with_court(Court::AutreJudiciaire { label: "T. com. Marseille".into() })
            }
        );
        let input = "Tribunal de commerce de Nanterre, 3 mars 2019, 201900123";
        let got = only(run(scan, input, &[(CourtA, "Tribunal de commerce")]));
        assert_eq!(
            decision(&got).court,
            Some(Court::AutreJudiciaire { label: "Tribunal de commerce de Nanterre".into() })
        );
        let input = "CPH Lyon, 3 mars 2019, n° 18/00321";
        let got = only(run(scan, input, &[(Sigle, "CPH")]));
        assert_eq!(decision(&got).numbers, [rg(18, 321)]);
        assert_eq!(decision(&got).court, Some(Court::AutreJudiciaire { label: "CPH Lyon".into() }));
        let input = "Conseil de prud'hommes d'Angers, 3 mars 2019, 18/00321";
        let got = only(run(scan, input, &[(CourtA, "Conseil de prud'hommes")]));
        assert_eq!(
            decision(&got).court,
            Some(Court::AutreJudiciaire { label: "Conseil de prud'hommes d'Angers".into() })
        );
    }

    #[test]
    fn invalid_date_n37_is_flagged_and_incomplete_mentions_are_ignored_n30() {
        let input = "CA Paris, 31/06/2024, n° 23/13690";
        let got = only(run(scan, input, &[(Sigle, "CA")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(decision(&got).date, None);
        assert_eq!(got.flags.list, [Flag::DateInvalide]);
        for (input, anchors) in [
            ("le TJ Paris a été saisi", vec![(Sigle, "TJ")]),
            ("la CA Paris a statué", vec![(Sigle, "CA")]),
            ("Cour d'appel de Atlantis, 4 juin 2020, 14/00554", vec![(CourtA, "Cour d'appel")]),
            ("Cour d'appel de Paris, 4 juin 2020", vec![(CourtA, "Cour d'appel")]),
            ("CA Paris, 4 juin 2020, no24/15215", vec![(Sigle, "CA")]),
            ("T. com. Marseille, 3 mars 2019, no2019J00123", vec![(CourtA, "T. com.")]),
            ("CAEN, 4 juin 2020, 14/00554", vec![(Sigle, "CAEN")]),
            ("N° RG 23/136", vec![(Rg, "23/136")]),
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
    }
}
