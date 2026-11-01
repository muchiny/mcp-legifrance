//! Grammaire du Conseil constitutionnel (annexe B §4.9).
//!
//! Quatre formes : décision titrée (`Cons. const., décision n° 2025-1173 QPC du 7 novembre
//! 2025`, `Décision 2025-1173 QPC - 07 novembre 2025 - …`), décision datée en tête (`Cons.
//! const., 30 juill. 2010, n° 2010-14/22 QPC`), nature suivie du numéro (`QPC 2025-1173`) et
//! numéro sans type ni juridiction (`Décision 2025-1173`, N21). Les deux dernières n'ont pas
//! d'ancre : la famille se déclenche d'elle-même. La queue d'un titre (parties, sujet,
//! solution, P141) est captée hors de la plage. Les plages des candidats sont des offsets du
//! texte d'ORIGINE (`Normalized::orig_range`), celles des ancres des offsets de
//! `Normalized::text`.

#![allow(
    clippy::many_single_char_names,
    reason = "lecteur à curseur : `c` (fenêtre) et `i`, `s`, `p`, `e` (positions), comme dans la grammaire de l'annexe B"
)]

use crate::citation::dict::courts::CcType;
use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{Ambiguity, CaseNumber, CcNumber, Court, DecisionCite};

use super::Candidate;
use super::cassation::lex::{self, Cur, DateHit, Hit};

/// Classes d'ancres où une décision du Conseil constitutionnel peut commencer.
const CLASSES: [AnchorClass; 3] = [AnchorClass::Court, AnchorClass::Sigle, AnchorClass::CcNumber];

/// Grammaires de la famille, essayées à chaque début.
const GRAMMARS: [lex::Grammar; 4] = [titled, dated, type_first, untyped];

/// Caractères lus pour les parties d'une queue de titre.
const MAX_PARTIES: usize = 120;

/// Caractères lus pour le sujet entre crochets.
const MAX_SUBJECT: usize = 400;

/// Caractères lus pour la solution.
const MAX_SOLUTION: usize = 100;

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    lex::scan_family(n, anchors, &CLASSES, Some(is_unanchored), &GRAMMARS)
}

/// Numéro sans son type : année, rang, numéros joints et rectification.
struct Num {
    year: u16,
    seq: u32,
    joined: Vec<u32>,
    rectif: bool,
}

/// `cc_court` : fin de la lecture et vrai pour le sigle `CC`.
fn court(c: &Cur<'_>, i: usize) -> Option<(usize, bool)> {
    c.kwb(i, "conseil constitutionnel")
        .or_else(|| c.abbr(i, "cons").and_then(|e| c.abbr(c.ws0(e), "const")))
        .or_else(|| c.kw(i, "c.").and_then(|e| c.abbr(c.ws0(e), "const")))
        .map(|e| (e, false))
        .or_else(|| c.litb(i, "CC").map(|e| (e, true)))
}

/// `("déc." | "décision") WS?` : fin de la lecture, espaces comprises.
fn decision_word(c: &Cur<'_>, i: usize) -> Option<usize> {
    c.kwb(i, "decision").or_else(|| c.abbr(i, "dec")).map(|e| c.ws0(e))
}

/// Année sur deux chiffres de 59 à 99 (1959-1999), forme de `NUM_DEC` : `71` pour 1971. Elle ne
/// doit pas être la fin d'un nombre plus long (`2016-71-44`, `12.71-44`, `12/71-44`).
fn short_year(c: &Cur<'_>, i: usize) -> Option<(usize, u16)> {
    let alone =
        i.checked_sub(1).is_none_or(|p| !c.ch(p).is_some_and(|ch| matches!(ch, '-' | '.' | '/')));
    let e = c.exact(i, 2).filter(|_| alone && c.left_ok(i))?;
    let year = 1900_u16.saturating_add(u16::try_from(c.val(i, e)).unwrap_or(0));
    (1959..=1999).contains(&year).then_some((e, year))
}

/// `cc_num` : `2025-1173`, `2010-14/22`, `2015-491R`, et `71-44` pour 1971.
fn cc_num(c: &Cur<'_>, i: usize) -> Option<(usize, Num)> {
    let (e, year) = c.year4(i).or_else(|| short_year(c, i))?;
    c.is(e, '-').then_some(())?;
    let (mut end, seq) = c.int(e + 1)?;
    let mut joined = Vec::new();
    while let Some((next, n)) = c.is(end, '/').then(|| c.int(end + 1)).flatten() {
        joined.push(n);
        end = next;
    }
    let rectif = c.is(end, 'R');
    Some((end + usize::from(rectif), Num { year, seq, joined, rectif }))
}

/// `cc_type` : sigle en majuscules, frontière de mot. Fin de la lecture et type.
fn cc_type(c: &Cur<'_>, i: usize) -> Option<(usize, CcType)> {
    let len = (0..8)
        .take_while(|&k| {
            c.ch(i + k).is_some_and(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        })
        .count();
    let kind = CcType::parse(&c.text(i, i + len))?;
    c.bound(i + len).then_some((i + len, kind))
}

/// Numéro d'affaire complet.
fn case_number(num: Num, kind: CcType) -> CaseNumber {
    CaseNumber::Cc(CcNumber {
        year: num.year,
        seq: num.seq,
        joined: num.joined,
        rectif: num.rectif,
        kind,
    })
}

/// Queue d'un titre Légifrance : parties, sujet entre crochets et solution.
#[derive(Default)]
struct Tail {
    parties: Option<String>,
    subject: Option<String>,
    solution: Option<String>,
}

/// Texte de `i` à `j` sans les espaces des bords, ou rien s'il est vide.
fn trimmed(c: &Cur<'_>, i: usize, j: usize) -> Option<String> {
    let text = c.text(i, j);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// `cc_tail` : `WS "-" WS [parties] [WS "[" sujet "]"] [WS "-" WS solution]`.
fn tail(c: &Cur<'_>, i: usize) -> Tail {
    let dash = |from: usize| Some(c.ws0(from)).filter(|&w| c.is(w, '-')).and_then(|w| c.ws(w + 1));
    let Some(start) = c.ws(i).filter(|&w| c.is(w, '-')).and_then(|w| c.ws(w + 1)) else {
        return Tail::default();
    };
    let parties_end = (start..start + MAX_PARTIES)
        .find(|&j| {
            c.ch(j).is_none_or(|ch| ch == '[' || ch == '\n')
                || (c.is(j, '-') && c.is(j - 1, ' ') && c.is(j + 1, ' '))
        })
        .unwrap_or(start + MAX_PARTIES);
    let bracket = c.ws0(parties_end);
    let subject_end = c
        .is(bracket, '[')
        .then(|| (bracket + 1..bracket + MAX_SUBJECT).find(|&j| c.is(j, ']')))
        .flatten();
    let after_subject = subject_end.map_or(parties_end, |j| j + 1);
    let solution = dash(after_subject).and_then(|s| {
        let end = (s..s + MAX_SOLUTION)
            .find(|&j| c.ch(j).is_none_or(|ch| matches!(ch, '.' | ';' | '\n' | '[')))
            .unwrap_or(s + MAX_SOLUTION);
        trimmed(c, s, end)
    });
    Tail {
        parties: trimmed(c, start, parties_end),
        subject: subject_end.and_then(|j| trimmed(c, bracket + 1, j)),
        solution,
    }
}

/// `[WS ("du" WS DATE | "-" WS DATE)]`.
fn title_date(c: &Cur<'_>, i: usize) -> Option<DateHit> {
    let w = c.ws(i)?;
    let p = c
        .kwb(w, "du")
        .and_then(|e| c.ws(e))
        .or_else(|| c.is(w, '-').then(|| c.ws(w + 1)).flatten())?;
    c.date(p)
}

/// Décision du Conseil constitutionnel, sans date.
fn cc_decision(num: Num, kind: CcType) -> DecisionCite {
    let mut decision = lex::decision(Some(Court::ConseilConstitutionnel));
    decision.numbers = vec![case_number(num, kind)];
    decision
}

/// `[cc_court SEP] [déc.] [NUM_SIGN] cc_num WS cc_type [date] [queue]`.
fn titled(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (p, sigle) = match court(c, s) {
        Some((e, sigle)) => (c.sep(e)?, Some(sigle)),
        None => (s, None),
    };
    let word = decision_word(c, p);
    let signed = c.num_sign(word.unwrap_or(p), false).map(|(e, _)| e);
    // « CC » n'est la juridiction que suivi de « déc. », d'une date ou de « n° » (P146).
    if sigle == Some(true) && word.is_none() && signed.is_none() {
        return None;
    }
    let (e, num) = cc_num(c, signed.or(word).unwrap_or(p))?;
    let (end, kind) = cc_type(c, c.ws(e)?)?;
    // Les types d'une lettre (`D`, `I`, `L`) ne comptent qu'avec un mot de contexte.
    if kind.as_str().len() == 1 && sigle.is_none() && word.is_none() {
        return None;
    }
    let mut decision = cc_decision(num, kind);
    let Some(date) = title_date(c, end) else {
        return Some(Hit::decision(s, end, decision));
    };
    decision.date = date.date;
    let queue = tail(c, date.end);
    decision.parties = queue.parties;
    decision.subject = queue.subject;
    decision.solution = queue.solution;
    Some(Hit::decision(s, date.end, decision).with_flags(date.flags))
}

/// `cc_court SEP DATE SEP NUM_SIGN cc_num WS cc_type` (P145, P149).
fn dated(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (e, _) = court(c, s)?;
    let date = c.date_link(c.sep(e)?)?;
    let (q, _) = c.num_sign(c.sep(date.end)?, false)?;
    let (e, num) = cc_num(c, q)?;
    let (end, kind) = cc_type(c, c.ws(e)?)?;
    let mut decision = cc_decision(num, kind);
    decision.date = date.date;
    Some(Hit::decision(s, end, decision).with_flags(date.flags))
}

/// `cc_type WS cc_num` : `QPC 2025-1173` (P148). Les types d'une lettre n'y sont pas admis.
fn type_first(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (e, kind) = cc_type(c, s)?;
    (kind.as_str().len() > 1).then_some(())?;
    let (end, num) = cc_num(c, c.ws(e)?)?;
    Some(Hit::decision(s, end, cc_decision(num, kind)))
}

/// `("décision" | "déc.") WS? [NUM_SIGN] AAAA-N` : ni juridiction ni type (N21).
fn untyped(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let w = decision_word(c, s)?;
    let p = c.num_sign(w, false).map_or(w, |(e, _)| e);
    let year = c.digits(p, 4, 4).filter(|&e| c.is(e, '-'))?;
    let (end, _) = c.int(year + 1).filter(|&(e, _)| c.bound(e))?;
    let mut decision = lex::decision(None);
    decision.numbers = vec![CaseNumber::Unknown(c.text(p, end))];
    Some(Hit::decision(s, end, decision).with_ambiguity(Ambiguity::JuridictionEtTypeManquants))
}

/// Début d'une citation sans ancre : nature suivie du numéro, ou « décision » et numéro.
fn is_unanchored(c: &Cur<'_>, i: usize) -> bool {
    c.left_ok(i) && (type_first(c, i).is_some() || untyped(c, i).is_some())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::super::cassation::testing::{blank, date, decision, matched, only, run, with_court};
    use super::*;
    use crate::citation::dict::courts::CcType;
    use crate::citation::scan::AnchorClass::{CcNumber as CcNumberA, Court as CourtA, Sigle};
    use crate::citation::types::{
        Ambiguity, CaseNumber, CcNumber, Confidence, Court, DecisionCite, Flag,
    };

    fn cc() -> DecisionCite {
        with_court(Court::ConseilConstitutionnel)
    }

    fn num(year: u16, seq: u32, joined: &[u32], rectif: bool, kind: CcType) -> Vec<CaseNumber> {
        vec![CaseNumber::Cc(CcNumber { year, seq, joined: joined.to_vec(), rectif, kind })]
    }

    #[test]
    fn corpus_forms_p140_to_p149() {
        let cases = [
            (
                "Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025 - Absence de publicité des débats",
                vec![(CourtA, "Cons. const.")],
                "Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025",
                DecisionCite {
                    date: date(2025, 11, 7),
                    numbers: num(2025, 1173, &[], false, CcType::Qpc),
                    parties: Some("Absence de publicité des débats".into()),
                    ..cc()
                },
            ),
            (
                "Elle a été enregistrée au secrétariat général du Conseil constitutionnel sous le n° 2025-1173 QPC.",
                vec![(CcNumberA, "2025-1173 QPC")],
                "n° 2025-1173 QPC",
                DecisionCite { numbers: num(2025, 1173, &[], false, CcType::Qpc), ..cc() },
            ),
            (
                "Cons. const., 30 juill. 2010, n° 2010-14/22 QPC",
                vec![(CourtA, "Cons. const."), (CcNumberA, "2010-14/22 QPC")],
                "Cons. const., 30 juill. 2010, n° 2010-14/22 QPC",
                DecisionCite {
                    date: date(2010, 7, 30),
                    numbers: num(2010, 14, &[22], false, CcType::Qpc),
                    ..cc()
                },
            ),
            (
                "CC, déc. n° 2009-580 DC du 10 juin 2009",
                vec![(Sigle, "CC")],
                "CC, déc. n° 2009-580 DC du 10 juin 2009",
                DecisionCite {
                    date: date(2009, 6, 10),
                    numbers: num(2009, 580, &[], false, CcType::Dc),
                    ..cc()
                },
            ),
            (
                "tendant à la rectification pour erreur matérielle de la décision n° 2015-491 QPC du 14 octobre 2015 par laquelle",
                vec![(CcNumberA, "2015-491 QPC")],
                "décision n° 2015-491 QPC du 14 octobre 2015",
                DecisionCite {
                    date: date(2015, 10, 14),
                    numbers: num(2015, 491, &[], false, CcType::Qpc),
                    ..cc()
                },
            ),
            (
                "QPC 2025-1173",
                vec![],
                "QPC 2025-1173",
                DecisionCite { numbers: num(2025, 1173, &[], false, CcType::Qpc), ..cc() },
            ),
            (
                "Cons. const., 18 janv. 2024, n° 2023-6130 AN",
                vec![(CourtA, "Cons. const.")],
                "Cons. const., 18 janv. 2024, n° 2023-6130 AN",
                DecisionCite {
                    date: date(2024, 1, 18),
                    numbers: num(2023, 6130, &[], false, CcType::An),
                    ..cc()
                },
            ),
        ];
        for (input, anchors, span, want) in cases {
            let got = only(run(scan, input, &anchors));
            assert_eq!(matched(input, &got), span, "{input}");
            assert_eq!(*decision(&got), want, "{input}");
            assert_eq!(got.confidence, Confidence::High, "{input}");
        }
    }

    /// L1-04 : année sur deux chiffres (1959-1999), lue comme 19AA.
    #[test]
    fn two_digit_year_forms_l1_04() {
        let cases = [
            (
                "Conseil constitutionnel, décision n° 71-44 DC du 16 juillet 1971",
                vec![(CourtA, "Conseil constitutionnel")],
                "Conseil constitutionnel, décision n° 71-44 DC du 16 juillet 1971",
                DecisionCite {
                    date: date(1971, 7, 16),
                    numbers: num(1971, 44, &[], false, CcType::Dc),
                    ..cc()
                },
            ),
            (
                "Décision n° 82-132 DC du 16 janvier 1982",
                vec![(CcNumberA, "82-132 DC")],
                "Décision n° 82-132 DC du 16 janvier 1982",
                DecisionCite {
                    date: date(1982, 1, 16),
                    numbers: num(1982, 132, &[], false, CcType::Dc),
                    ..cc()
                },
            ),
            (
                "DC 71-44",
                vec![],
                "DC 71-44",
                DecisionCite { numbers: num(1971, 44, &[], false, CcType::Dc), ..cc() },
            ),
            (
                "Cons. const., décision n° 94-343/344 DC du 27 juillet 1994",
                vec![(CourtA, "Cons. const.")],
                "Cons. const., décision n° 94-343/344 DC du 27 juillet 1994",
                DecisionCite {
                    date: date(1994, 7, 27),
                    numbers: num(1994, 343, &[344], false, CcType::Dc),
                    ..cc()
                },
            ),
            // Numéro en tout début de texte (aucun caractère à gauche).
            (
                "71-44 DC",
                vec![(CcNumberA, "71-44 DC")],
                "71-44 DC",
                DecisionCite { numbers: num(1971, 44, &[], false, CcType::Dc), ..cc() },
            ),
            // La forme à quatre chiffres reste lue pour les années 1959-1999.
            (
                "Cons. const., décision n° 1971-44 DC du 16 juillet 1971",
                vec![(CourtA, "Cons. const.")],
                "Cons. const., décision n° 1971-44 DC du 16 juillet 1971",
                DecisionCite {
                    date: date(1971, 7, 16),
                    numbers: num(1971, 44, &[], false, CcType::Dc),
                    ..cc()
                },
            ),
            // Bornes de l'intervalle 59-99.
            (
                "Décision n° 59-1 DC du 1 janvier 1959",
                vec![(CcNumberA, "59-1 DC")],
                "Décision n° 59-1 DC du 1 janvier 1959",
                DecisionCite {
                    date: date(1959, 1, 1),
                    numbers: num(1959, 1, &[], false, CcType::Dc),
                    ..cc()
                },
            ),
            (
                "Décision n° 99-410 DC du 15 mars 1999",
                vec![(CcNumberA, "99-410 DC")],
                "Décision n° 99-410 DC du 15 mars 1999",
                DecisionCite {
                    date: date(1999, 3, 15),
                    numbers: num(1999, 410, &[], false, CcType::Dc),
                    ..cc()
                },
            ),
        ];
        for (input, anchors, span, want) in cases {
            let got = only(run(scan, input, &anchors));
            assert_eq!(matched(input, &got), span, "{input}");
            assert_eq!(*decision(&got), want, "{input}");
            assert_eq!(got.confidence, Confidence::High, "{input}");
        }
    }

    #[test]
    fn title_tails_are_captured_outside_the_span_p141_p142() {
        let input = "Décision 2025-1173 QPC - 07 novembre 2025 - M.\u{a0}Amine G. [Absence de publicité des débats devant le juge] - Conformité";
        let got = only(run(scan, input, &[(CcNumberA, "2025-1173 QPC")]));
        assert_eq!(matched(input, &got), "Décision 2025-1173 QPC - 07 novembre 2025");
        assert_eq!(
            *decision(&got),
            DecisionCite {
                date: date(2025, 11, 7),
                numbers: num(2025, 1173, &[], false, CcType::Qpc),
                parties: Some("M. Amine G.".into()),
                subject: Some("Absence de publicité des débats devant le juge".into()),
                solution: Some("Conformité".into()),
                ..cc()
            }
        );
        let input = "Décision 2021-817 DC - 20 mai 2021 - Loi pour une sécurité globale préservant les libertés  - Non conformité partielle - réserve";
        let got = only(run(scan, input, &[(CcNumberA, "2021-817 DC")]));
        assert_eq!(matched(input, &got), "Décision 2021-817 DC - 20 mai 2021");
        assert_eq!(
            decision(&got).parties.as_deref(),
            Some("Loi pour une sécurité globale préservant les libertés")
        );
        assert_eq!(decision(&got).solution.as_deref(), Some("Non conformité partielle - réserve"));
        // Sans date, la queue n'est pas lue.
        let input = "Décision 2021-817 DC - voir";
        let got = only(run(scan, input, &[(CcNumberA, "2021-817 DC")]));
        assert_eq!(matched(input, &got), "Décision 2021-817 DC");
        assert_eq!(decision(&got).parties, None);
    }

    #[test]
    fn number_variants_and_dates() {
        let input = "Cons. const., décision n° 2015-491R QPC du 31 juin 2015";
        let got = only(run(scan, input, &[(CourtA, "Cons. const.")]));
        assert_eq!(decision(&got).numbers, num(2015, 491, &[], true, CcType::Qpc));
        assert_eq!(decision(&got).date, None);
        assert_eq!(got.flags.list, [Flag::DateInvalide]);
        let input = "Conseil constitutionnel, 2010-14/22/23 QPC du 30 juillet 2010";
        let got = only(run(scan, input, &[(CourtA, "Conseil constitutionnel")]));
        assert_eq!(decision(&got).numbers, num(2010, 14, &[22, 23], false, CcType::Qpc));
        for (text, kind) in [("AR16", CcType::Ar16), ("ELEC", CcType::Elec), ("LOM", CcType::Lom)] {
            let input = format!("C. const., déc. 2020-5 {text}");
            let got = only(run(scan, &input, &[(CourtA, "C. const.")]));
            assert_eq!(decision(&got).numbers, num(2020, 5, &[], false, kind), "{input}");
        }
    }

    #[test]
    fn bare_decision_number_n21() {
        let input = "Décision 2025-1173";
        let got = only(run(scan, input, &[]));
        assert_eq!(matched(input, &got), "Décision 2025-1173");
        assert_eq!(
            *decision(&got),
            DecisionCite { numbers: vec![CaseNumber::Unknown("2025-1173".into())], ..blank() }
        );
        assert_eq!(got.flags.ambiguity, Some(Ambiguity::JuridictionEtTypeManquants));
        let input = "voir déc. n° 2025-1173, puis";
        let got = only(run(scan, input, &[]));
        assert_eq!(matched(input, &got), "déc. n° 2025-1173");
    }

    #[test]
    fn single_letter_types_need_a_context_and_near_misses_give_nothing() {
        let input = "la loi n° 2013-1117 I. - Le code";
        assert_eq!(run(scan, input, &[(CcNumberA, "2013-1117 I")]), []);
        // « CC » sans « déc. », date ou « n° » n'est pas la juridiction : seul le numéro reste.
        let input = "CC, 2009-580 DC";
        let got = only(run(scan, input, &[(Sigle, "CC"), (CcNumberA, "2009-580 DC")]));
        assert_eq!(matched(input, &got), "2009-580 DC");
        let input = "CC, déc. n° 2010-14 L du 3 mars 2010";
        let got = only(run(scan, input, &[(Sigle, "CC")]));
        assert_eq!(decision(&got).numbers, num(2010, 14, &[], false, CcType::L));
        for (input, anchors) in [
            ("pour la période 2016-1321", vec![]),
            ("art. 1240 CC, puis", vec![(Sigle, "CC")]),
            ("n° 2016-1321 XYZ", vec![(CcNumberA, "2016-1321")]),
            ("n° 2016-1321 DCX", vec![(CcNumberA, "2016-1321")]),
            ("QPC 2025", vec![]),
            ("décision 25-1173", vec![]),
            ("Cons. const., 30 juill. 2010, 2010-14 QPC", vec![(CourtA, "Cons. const.")]),
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
    }

    /// L1-04 : l'année sur deux chiffres ne capte ni les numéros de texte, ni les années hors
    /// 1959-1999, ni un nombre qui n'est pas un début de numéro (N44).
    #[test]
    fn two_digit_years_are_limited_to_1959_1999_n44() {
        for (input, anchors) in [
            // Décret, loi, pourvoi : numéros de texte ou de dossier, pas du Conseil constitutionnel.
            ("décret n° 93-1419 L du 29 décembre 1993", vec![(CcNumberA, "93-1419 L")]),
            ("loi n° 58-1067 du 28 octobre 1958", vec![]),
            ("pourvoi n° 93-12.345", vec![]),
            // Année deux chiffres hors de 59-99, ou sur trois chiffres.
            ("Décision n° 58-1 DC du 1 janvier 1958", vec![]),
            ("Décision n° 25-1173 QPC du 7 novembre 2025", vec![]),
            ("Décision n° 00-12 DC", vec![]),
            ("Décision n° 971-44 DC du 16 juillet 1971", vec![]),
            // Le nombre fait partie d'un numéro plus long.
            ("Selon 2016-71-44 DC", vec![(CcNumberA, "71-44 DC")]),
            ("Selon 12.71-44 DC", vec![(CcNumberA, "71-44 DC")]),
            ("Selon 12/71-44 DC", vec![(CcNumberA, "71-44 DC")]),
            ("Selon x71-44 DC", vec![(CcNumberA, "71-44 DC")]),
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
        // Jumeau : le même décret avec un contexte « Cons. const. » est bien lu.
        let input = "Cons. const., déc. n° 93-1419 L du 29 décembre 1993";
        let got = only(run(scan, input, &[(CourtA, "Cons. const.")]));
        assert_eq!(decision(&got).numbers, num(1993, 1419, &[], false, CcType::L));
        // « décision n° 71-44 » sans type reste un numéro de texte : rien ici.
        assert_eq!(run(scan, "décision n° 71-44", &[]), []);
    }

    /// Un titre sans « du » ni tiret après le type n'a pas de date : la décision s'arrête au type.
    #[test]
    fn title_without_date_stops_at_the_type() {
        let input = "Décision 2025-1173 QPC voir ci-dessous";
        let got = only(run(scan, input, &[(CcNumberA, "2025-1173 QPC")]));
        assert_eq!(matched(input, &got), "Décision 2025-1173 QPC");
        assert_eq!(decision(&got).date, None);
        assert_eq!(decision(&got).parties, None);
        // « du » sans espace ni date derrière : pas de date non plus.
        let input = "Décision 2025-1173 QPC du";
        let got = only(run(scan, input, &[(CcNumberA, "2025-1173 QPC")]));
        assert_eq!(matched(input, &got), "Décision 2025-1173 QPC");
        assert_eq!(decision(&got).date, None);
    }

    /// Chaque morceau de la forme datée et de la forme titrée est exigé ; sinon rien n'est cité.
    #[test]
    fn dated_and_titled_forms_need_every_part() {
        for (input, anchors) in [
            // Rien après la juridiction qui la sépare de la suite.
            ("Cons. const.2025-1173 QPC", vec![(CourtA, "Cons. const.")]),
            // La date est collée à ce qui la suit.
            ("Cons. const., 30 juill. 2010x 2010-14 QPC", vec![(CourtA, "Cons. const.")]),
            // Le numéro n'a pas de rang.
            ("Décision 2025-abc QPC", vec![(CcNumberA, "QPC")]),
            ("Cons. const., 30 juill. 2010, n° abc QPC", vec![(CourtA, "Cons. const.")]),
            // Le type est collé au numéro ou n'en est pas un.
            ("Cons. const., 30 juill. 2010, n° 2010-14QPC", vec![(CourtA, "Cons. const.")]),
            ("Cons. const., 30 juill. 2010, n° 2010-14 XYZ", vec![(CourtA, "Cons. const.")]),
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
    }
}
