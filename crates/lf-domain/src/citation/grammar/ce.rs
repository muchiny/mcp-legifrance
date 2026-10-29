//! Grammaire du Conseil d'État (annexe B §4.7).
//!
//! Décision complète (`ce_decision`, titres Légifrance compris), en-tête Ariane `Conseil d'État
//! N° 99522` (P131) et numéro seul précédé d'un indice d'enregistrement (N28, sans ancre : la
//! famille se déclenche d'elle-même). Les plages des candidats sont des offsets du texte
//! d'ORIGINE (`Normalized::orig_range`), celles des ancres des offsets de `Normalized::text`.

#![allow(
    clippy::many_single_char_names,
    reason = "lecteur à curseur : `c` (fenêtre) et `i`, `s`, `p`, `e` (positions), comme dans la grammaire de l'annexe B"
)]

use crate::citation::dict::courts::CeFormation;
use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{Ambiguity, CaseNumber, Confidence, Court, LebonRef, Publication};

use super::Candidate;
use super::cassation::lex::{self, Cur, DateHit, Hit};

/// Classes d'ancres où une décision du Conseil d'État peut commencer.
const CLASSES: [AnchorClass; 2] = [AnchorClass::Court, AnchorClass::Sigle];

/// Grammaires de la famille, essayées à chaque début.
const GRAMMARS: [lex::Grammar; 3] = [decision, header, orphan_number];

/// Indices d'enregistrement qui autorisent un numéro seul (§4.17), en vue clé.
const REGISTRATION: [&str; 6] = ["sous le", "enregistre", "requete", "req.", "pourvoi", "affaire"];

/// Caractères lus avant un numéro seul pour y chercher un indice d'enregistrement.
const REGISTRATION_WINDOW: usize = 30;

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    lex::scan_family(n, anchors, &CLASSES, Some(is_orphan_number), &GRAMMARS)
}

/// Formation lue : fin, formation et chambres citées.
type Formation = (usize, CeFormation, Vec<u8>);

/// Formation et chambres, ou rien.
type Chambers = Option<(CeFormation, Vec<u8>)>;

/// `ce_court` : fin de la lecture et vrai pour « Conseil d'État » en toutes lettres.
fn court(c: &Cur<'_>, i: usize) -> Option<(usize, bool)> {
    if let Some(e) = c.kwb(i, "conseil d'etat") {
        return Some((e, true));
    }
    c.litb(i, "CE")
        .or_else(|| c.lit(i, "C.E."))
        .or_else(|| c.abbr(i, "cons").and_then(|e| c.kwb(c.ws(e)?, "d'etat")))
        .map(|e| (e, false))
}

/// Entier d'un ou deux chiffres : fin et valeur.
fn small(c: &Cur<'_>, i: usize) -> Option<(usize, u8)> {
    let e = c.digits(i, 1, 2)?;
    Some((e, u8::try_from(c.val(i, e)).unwrap_or(0)))
}

/// `WS? ( "et" | "-" | "/" | "," ) WS?`.
fn joiner(c: &Cur<'_>, i: usize) -> Option<usize> {
    let p = c.ws0(i);
    let e = if matches!(c.ch(p), Some('-' | '/' | ',')) { p + 1 } else { c.kwb(p, "et")? };
    Some(c.ws0(e))
}

/// `ORDN { joiner ORDN }` : fin et ordinaux.
fn ordinals(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<u8>)> {
    let (mut end, first) = c.ordinal(i)?;
    let mut all = vec![first];
    while let Some((e, n)) = joiner(c, end).and_then(|p| c.ordinal(p)) {
        all.push(n);
        end = e;
    }
    Some((end, all))
}

/// `ch_reunies` : `2ème - 7ème chambres réunies`, `5e et 6e ch. réunies`, `CHR`.
fn chambers_together(c: &Cur<'_>, i: usize) -> Option<Formation> {
    if let Some(e) = c.litb(i, "CHR") {
        return Some((e, CeFormation::ChambresReunies, Vec::new()));
    }
    let (e, chambers) = ordinals(c, i)?;
    let w = c.ws(e)?;
    let keyword = c.abbr(w, "ch").or_else(|| c.kwb(w, "chambres"))?;
    let end = c.kwb(c.ws(keyword)?, "reunies")?;
    Some((end, CeFormation::ChambresReunies, chambers))
}

/// `ch_seule` : `9ème chambre jugeant seule`, `1re ch.`, `CHS`.
fn chamber_alone(c: &Cur<'_>, i: usize) -> Option<Formation> {
    if let Some(e) = c.litb(i, "CHS") {
        return Some((e, CeFormation::ChambreSeule, Vec::new()));
    }
    let (e, n) = c.ordinal(i)?;
    let w = c.ws(e)?;
    let keyword = c.kwb(w, "chambre").or_else(|| c.abbr(w, "ch"))?;
    let end = c.ws(keyword).and_then(|x| c.kwb(x, "jugeant seule")).unwrap_or(keyword);
    Some((end, CeFormation::ChambreSeule, vec![n]))
}

/// `ssr` : `7 / 5 SSR`, `5e et 6e sous-sections réunies`, `SSR`.
fn subsections_together(c: &Cur<'_>, i: usize) -> Option<Formation> {
    let found = CeFormation::SousSectionsReunies;
    if let Some(e) = c.litb(i, "SSR") {
        return Some((e, found, Vec::new()));
    }
    if let Some((e, a)) = small(c, i) {
        let slash = c.ws0(e);
        if c.is(slash, '/') {
            let (e, b) = small(c, c.ws0(slash + 1))?;
            return Some((c.litb(c.ws(e)?, "SSR")?, found, vec![a, b]));
        }
    }
    let (e, a) = c.ordinal(i)?;
    let (e, b) = c.ordinal(c.ws(c.kwb(c.ws(e)?, "et")?)?)?;
    let end = c.kwb(c.ws(e)?, "sous-sections reunies")?;
    Some((end, found, vec![a, b]))
}

/// `ssjs` : `6ème sous-section jugeant seule`, `SSJS`.
fn subsection_alone(c: &Cur<'_>, i: usize) -> Option<Formation> {
    if let Some(e) = c.litb(i, "SSJS") {
        return Some((e, CeFormation::SousSectionSeule, Vec::new()));
    }
    let (e, n) = c.ordinal(i)?;
    let keyword = c.kwb(c.ws(e)?, "sous-section")?;
    let end = c.ws(keyword).and_then(|x| c.kwb(x, "jugeant seule")).unwrap_or(keyword);
    Some((end, CeFormation::SousSectionSeule, vec![n]))
}

/// Formations écrites en un mot ou une expression, sans chambre.
fn named_formation(c: &Cur<'_>, i: usize) -> Option<(usize, CeFormation)> {
    let with_dispute = |e: usize| c.ws(e).and_then(|w| c.kwb(w, "du contentieux")).unwrap_or(e);
    c.abbr(i, "ass")
        .map(|e| (e, CeFormation::Assemblee))
        .or_else(|| c.kwb(i, "assemblee").map(|e| (with_dispute(e), CeFormation::Assemblee)))
        .or_else(|| c.abbr(i, "sect").map(|e| (e, CeFormation::Section)))
        .or_else(|| c.kwb(i, "section").map(|e| (with_dispute(e), CeFormation::Section)))
        .or_else(|| c.abbr(i, "ord").map(|e| (e, CeFormation::Ordonnance)))
        .or_else(|| c.abbr(i, "ref").map(|e| (e, CeFormation::Ordonnance)))
        .or_else(|| c.kwb(i, "juge des referes").map(|e| (e, CeFormation::Ordonnance)))
        .or_else(|| c.kwb(i, "avis").map(|e| (e, CeFormation::Avis)))
}

/// `ce_formation`.
fn formation(c: &Cur<'_>, i: usize) -> Option<Formation> {
    chambers_together(c, i)
        .or_else(|| chamber_alone(c, i))
        .or_else(|| subsections_together(c, i))
        .or_else(|| subsection_alone(c, i))
        .or_else(|| named_formation(c, i).map(|(e, f)| (e, f, Vec::new())))
}

/// `[ce_formation SEP] DATE_LINK`. Avec « Conseil d'État » en toutes lettres, un titre Légifrance
/// porte une formation libre (60 caractères au plus, sans virgule) que `ce_formation` relit.
fn formation_and_date(c: &Cur<'_>, p: usize, long: bool) -> Option<(Chambers, DateHit)> {
    let strict = formation(c, p).and_then(|(e, f, chambers)| {
        let date = c.date_link(c.sep(e)?)?;
        Some((Some((f, chambers)), date))
    });
    strict
        .or_else(|| long.then(|| free_formation(c, p)).flatten())
        .or_else(|| c.date_link(p).map(|date| (None, date)))
}

/// Formation libre d'un titre Légifrance, jusqu'à la première virgule.
fn free_formation(c: &Cur<'_>, p: usize) -> Option<(Chambers, DateHit)> {
    let comma = (p + 1..p + 60).find(|&j| matches!(c.ch(j), Some(',' | ';')))?;
    let date = c.date_link(c.ws0(comma + 1))?;
    Some((formation(c, p).map(|(_, f, chambers)| (f, chambers)), date))
}

/// `ce_num` : de 4 à 6 chiffres. Fin de la lecture.
fn requete_end(c: &Cur<'_>, i: usize) -> Option<usize> {
    c.left_ok(i).then_some(())?;
    c.digits(i, 4, 6).filter(|&e| c.num_end_ok(e))
}

/// `ce_num` comme numéro d'affaire.
fn requete(c: &Cur<'_>, i: usize) -> Option<(usize, CaseNumber)> {
    requete_end(c, i).map(|e| (e, CaseNumber::CeRequete(c.val(i, e))))
}

/// `ce_numbers` : `[req. | requête] (n° | nos) ce_num { LIST_SEP ce_num }` ou un numéro seul.
fn ce_numbers(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let p = c.abbr(i, "req").or_else(|| c.kwb(i, "requete")).and_then(|e| c.ws(e)).unwrap_or(i);
    if let Some((q, _)) = c.num_sign(p, true) {
        return lex::number_list(c, q, requete);
    }
    requete(c, i).map(|(e, n)| (e, vec![n]))
}

/// `parties SEP ce_numbers` : texte sans `n°` ni chiffre de tête, 120 caractères au plus, jusqu'à
/// la première virgule suivie de numéros.
fn parties_then_numbers(c: &Cur<'_>, p: usize) -> Option<(String, usize, Vec<CaseNumber>)> {
    (!c.digit(p)).then_some(())?;
    (p + 1..=p + 120).filter(|&j| c.is(j, ',')).find_map(|j| {
        (!c.key_text(p, j).contains("n°")).then_some(())?;
        let (end, numbers) = ce_numbers(c, c.ws0(j + 1))?;
        Some((c.text(p, j).trim_end().to_owned(), end, numbers))
    })
}

/// Page du Lebon : `p. 541`.
fn page(c: &Cur<'_>, i: usize) -> Option<(usize, String)> {
    let (end, _) = c.int(c.ws(c.kw(i, "p.")?)?)?;
    Some((end, c.text(i, end)))
}

/// `lebon` : `Publié au recueil Lebon`, `Leb., p. 541`, `Rec. Lebon T.`.
fn lebon(c: &Cur<'_>, i: usize) -> Option<(usize, LebonRef)> {
    let long = [
        ("publie au recueil lebon", LebonRef::Publie),
        ("mentionne aux tables du recueil lebon", LebonRef::Tables),
        ("inedit au recueil lebon", LebonRef::Inedit),
    ];
    if let Some((e, kind)) = long.into_iter().find_map(|(w, k)| c.kwb(i, w).map(|e| (e, k))) {
        return Some((e, kind));
    }
    let head = c
        .kwb(i, "recueil lebon")
        .or_else(|| c.abbr(i, "rec").map(|e| c.ws(e).and_then(|w| c.kwb(w, "lebon")).unwrap_or(e)))
        .or_else(|| c.abbr(i, "leb"))
        .or_else(|| c.kwb(i, "lebon"))?;
    let tables = c.ws(head).and_then(|w| c.kw(w, "t.").or_else(|| c.kwb(w, "tables")));
    let after = tables.unwrap_or(head);
    Some(match c.sep(after).and_then(|s| page(c, s)) {
        Some((end, text)) => (end, LebonRef::Page(text)),
        None if tables.is_some() => (after, LebonRef::Tables),
        None => (after, LebonRef::Publie),
    })
}

/// `ce_court SEP [ce_formation SEP] DATE_LINK SEP [parties SEP] ce_numbers [SEP lebon]`.
fn decision(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (e, long) = court(c, s)?;
    let (chambers, date) = formation_and_date(c, c.sep(e)?, long)?;
    let q = c.sep(date.end)?;
    let (parties, end, numbers) = if let Some((end, numbers)) = ce_numbers(c, q) {
        (None, end, numbers)
    } else {
        let (parties, end, numbers) = parties_then_numbers(c, q)?;
        (Some(parties), end, numbers)
    };
    let (formation, chambers) = chambers.unzip();
    let mut dec = lex::decision(Some(Court::ConseilEtat {
        formation,
        chambers: chambers.unwrap_or_default(),
    }));
    dec.date = date.date;
    dec.numbers = numbers;
    dec.parties = parties;
    let mention = c.sep(end).and_then(|p| lebon(c, p));
    let end = mention.as_ref().map_or(end, |(e, _)| *e);
    dec.publication = mention.map(|(_, lebon)| Publication { bulletin: None, lebon: Some(lebon) });
    Some(Hit::decision(s, end, dec).with_flags(date.flags))
}

/// `ce_court WS NUM_SIGN ce_num` : en-tête Ariane (P131), sans date.
fn header(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (e, _) = court(c, s)?;
    let (q, _) = c.num_sign(c.ws(e)?, false)?;
    let end = requete_end(c, q)?;
    let mut dec = lex::decision(Some(Court::ConseilEtat { formation: None, chambers: Vec::new() }));
    dec.numbers = vec![CaseNumber::CeRequete(c.val(q, end))];
    Some(Hit::decision(s, end, dec))
}

/// `NUM_SIGN ce_num` précédé d'un indice d'enregistrement (N28) : juridiction manquante.
fn orphan_number(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (p, _) = c.num_sign(s, false)?;
    let end = requete_end(c, p)?;
    let context = c.key_text(s.saturating_sub(REGISTRATION_WINDOW), s);
    REGISTRATION.iter().any(|word| context.contains(word)).then_some(())?;
    let mut dec = lex::decision(None);
    dec.numbers = vec![CaseNumber::Unknown(c.text(p, end))];
    Some(
        Hit::decision(s, end, dec)
            .with_confidence(Confidence::Low)
            .with_ambiguity(Ambiguity::JuridictionManquante),
    )
}

/// Début possible d'un numéro seul.
fn is_orphan_number(c: &Cur<'_>, i: usize) -> bool {
    orphan_number(c, i).is_some()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::super::cassation::testing::{blank, date, decision, matched, only, run, with_court};
    use super::*;
    use crate::citation::scan::AnchorClass::{Court as CourtA, Sigle};
    use crate::citation::types::{DecisionCite, Flag};

    fn ce(formation: Option<CeFormation>, chambers: &[u8]) -> DecisionCite {
        with_court(Court::ConseilEtat { formation, chambers: chambers.to_vec() })
    }

    fn req(n: u32) -> Vec<CaseNumber> {
        vec![CaseNumber::CeRequete(n)]
    }

    fn lebon_of(l: LebonRef) -> Publication {
        Publication { bulletin: None, lebon: Some(l) }
    }

    #[test]
    fn corpus_forms_p120_to_p130() {
        let cases = [
            (
                "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507",
                CourtA,
                "Conseil d'État",
                DecisionCite {
                    date: date(2026, 5, 29),
                    numbers: req(506_507),
                    ..ce(Some(CeFormation::ChambresReunies), &[2, 7])
                },
            ),
            (
                "Conseil d'Etat, 7 / 5 SSR, du 28 décembre 2001, 216642, mentionné aux tables du recueil Lebon",
                CourtA,
                "Conseil d'Etat",
                DecisionCite {
                    date: date(2001, 12, 28),
                    numbers: req(216_642),
                    publication: Some(lebon_of(LebonRef::Tables)),
                    ..ce(Some(CeFormation::SousSectionsReunies), &[7, 5])
                },
            ),
            (
                "CE, 14 avril 2023, n° 462797, mentionné aux tables du Recueil Lebon",
                Sigle,
                "CE",
                DecisionCite {
                    date: date(2023, 4, 14),
                    numbers: req(462_797),
                    publication: Some(lebon_of(LebonRef::Tables)),
                    ..ce(None, &[])
                },
            ),
            (
                "CE, Sect., 22 février 2007, req. n° 264541, Leb., p. 541",
                Sigle,
                "CE",
                DecisionCite {
                    date: date(2007, 2, 22),
                    numbers: req(264_541),
                    publication: Some(lebon_of(LebonRef::Page("p. 541".into()))),
                    ..ce(Some(CeFormation::Section), &[])
                },
            ),
            (
                "CE, ass., 3 févr. 1989, Compagnie Alitalia, n° 74052, Rec. p. 44",
                Sigle,
                "CE",
                DecisionCite {
                    date: date(1989, 2, 3),
                    parties: Some("Compagnie Alitalia".into()),
                    numbers: req(74_052),
                    publication: Some(lebon_of(LebonRef::Page("p. 44".into()))),
                    ..ce(Some(CeFormation::Assemblee), &[])
                },
            ),
            (
                "CE, 5e et 6e ch. réunies, 10 mars 2025, n° 489701",
                Sigle,
                "CE",
                DecisionCite {
                    date: date(2025, 3, 10),
                    numbers: req(489_701),
                    ..ce(Some(CeFormation::ChambresReunies), &[5, 6])
                },
            ),
            (
                "CE, ord., 27 juillet 2021, n° 454852",
                Sigle,
                "CE",
                DecisionCite {
                    date: date(2021, 7, 27),
                    numbers: req(454_852),
                    ..ce(Some(CeFormation::Ordonnance), &[])
                },
            ),
        ];
        for (input, class, needle, want) in cases {
            let got = only(run(scan, input, &[(class, needle)]));
            assert_eq!(matched(input, &got), input);
            assert_eq!(*decision(&got), want, "{input}");
            assert_eq!(got.confidence, Confidence::High, "{input}");
        }
    }

    #[test]
    fn header_p131_and_other_court_spellings() {
        let input = "Conseil d'État N° 99522 ECLI:FR:CESSR:1976:99522.19761215 Publié au recueil Lebon Section du Contentieux";
        let got = only(run(scan, input, &[(CourtA, "Conseil d'État")]));
        assert_eq!(matched(input, &got), "Conseil d'État N° 99522");
        assert_eq!(*decision(&got), DecisionCite { numbers: req(99_522), ..ce(None, &[]) });
        for (input, class, needle) in [
            ("C.E., 3 mars 2020, n° 12345", Sigle, "C.E."),
            ("Cons. d'État, 3 mars 2020, n° 12345", CourtA, "Cons."),
        ] {
            let got = only(run(scan, input, &[(class, needle)]));
            assert_eq!(matched(input, &got), input);
            assert_eq!(decision(&got).numbers, req(12_345));
        }
    }

    #[test]
    fn formation_spellings() {
        let cases: [(&str, CeFormation, &[u8]); 15] = [
            ("assemblée du contentieux", CeFormation::Assemblee, &[]),
            ("sect.", CeFormation::Section, &[]),
            ("section", CeFormation::Section, &[]),
            ("réf.", CeFormation::Ordonnance, &[]),
            ("juge des référés", CeFormation::Ordonnance, &[]),
            ("avis", CeFormation::Avis, &[]),
            ("CHR", CeFormation::ChambresReunies, &[]),
            ("CHS", CeFormation::ChambreSeule, &[]),
            ("SSR", CeFormation::SousSectionsReunies, &[]),
            ("SSJS", CeFormation::SousSectionSeule, &[]),
            ("4ème et 5ème sous-sections réunies", CeFormation::SousSectionsReunies, &[4, 5]),
            ("6ème sous-section jugeant seule", CeFormation::SousSectionSeule, &[6]),
            ("1re chambre", CeFormation::ChambreSeule, &[1]),
            ("9ème chambre jugeant seule", CeFormation::ChambreSeule, &[9]),
            ("2e, 3e / 4e chambres réunies", CeFormation::ChambresReunies, &[2, 3, 4]),
        ];
        for (text, formation, chambers) in cases {
            let input = format!("CE, {text}, 3 mars 2020, n° 410000");
            let got = only(run(scan, &input, &[(Sigle, "CE")]));
            assert_eq!(matched(&input, &got), input);
            assert_eq!(
                decision(&got).court,
                Some(Court::ConseilEtat {
                    formation: Some(formation),
                    chambers: chambers.to_vec()
                }),
                "{text}"
            );
        }
        let input = "CE, 7/5 SSR, 3 mars 2020, n° 410000";
        let got = only(run(scan, input, &[(Sigle, "CE")]));
        assert_eq!(
            decision(&got).court,
            Some(Court::ConseilEtat {
                formation: Some(CeFormation::SousSectionsReunies),
                chambers: vec![7, 5]
            })
        );
    }

    #[test]
    fn publication_forms_and_number_lists() {
        for (tail, want) in [
            (", Publié au recueil Lebon", LebonRef::Publie),
            (", Inédit au recueil Lebon", LebonRef::Inedit),
            (", Rec. Lebon T. p. 650", LebonRef::Page("p. 650".into())),
            (", Lebon", LebonRef::Publie),
            (", Rec. Lebon T.", LebonRef::Tables),
            (", recueil Lebon tables", LebonRef::Tables),
            (", Rec. Lebon", LebonRef::Publie),
            (", Leb.", LebonRef::Publie),
        ] {
            let input = format!("CE, 28 mars 2018, n° 410000{tail}");
            let got = only(run(scan, &input, &[(Sigle, "CE")]));
            assert_eq!(matched(&input, &got), input, "{input}");
            assert_eq!(decision(&got).publication, Some(lebon_of(want)), "{input}");
        }
        let input = "CE, 28 mars 2018, nos 410000, 410001 et 410002";
        let got = only(run(scan, input, &[(Sigle, "CE")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(
            decision(&got).numbers,
            [
                CaseNumber::CeRequete(410_000),
                CaseNumber::CeRequete(410_001),
                CaseNumber::CeRequete(410_002)
            ]
        );
        // « Leb. » sans page : la virgule qui suit n'est pas reprise.
        let input = "CE, 28 mars 2018, n° 410000, Leb., puis";
        let got = only(run(scan, input, &[(Sigle, "CE")]));
        assert_eq!(matched(input, &got), "CE, 28 mars 2018, n° 410000, Leb.");
    }

    #[test]
    fn free_formation_of_a_legifrance_title() {
        let input = "Conseil d'Etat, Section du Contentieux Avis, 20/11/2010, 331234";
        let got = only(run(scan, input, &[(CourtA, "Conseil d'Etat")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(
            *decision(&got),
            DecisionCite {
                date: date(2010, 11, 20),
                numbers: req(331_234),
                ..ce(Some(CeFormation::Section), &[])
            }
        );
        let input = "Conseil d'Etat, texte libre, 20/11/2010, 331234";
        let got = only(run(scan, input, &[(CourtA, "Conseil d'Etat")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(
            decision(&got).court,
            Some(Court::ConseilEtat { formation: None, chambers: vec![] })
        );
    }

    #[test]
    fn invalid_date_n09_and_orphan_number_n28() {
        let input = "CE, 31 février 2020, n° 123456";
        let got = only(run(scan, input, &[(Sigle, "CE")]));
        assert_eq!(decision(&got).date, None);
        assert_eq!(got.flags.list, [Flag::DateInvalide]);
        let input = "sous le n° 506507";
        let got = only(run(scan, input, &[]));
        assert_eq!(matched(input, &got), "n° 506507");
        assert_eq!(
            *decision(&got),
            DecisionCite { numbers: vec![CaseNumber::Unknown("506507".into())], ..blank() }
        );
        assert_eq!(got.confidence, Confidence::Low);
        assert_eq!(got.flags.ambiguity, Some(Ambiguity::JuridictionManquante));
    }

    #[test]
    fn near_misses_give_nothing() {
        for (input, anchors) in [
            ("art. L. 52-8 CE", vec![(Sigle, "CE")]),
            ("CEDH, 4 juin 2020, n° 12345", vec![(Sigle, "CEDH")]),
            ("facture n° 123456", vec![]),
            ("sous le n° 12", vec![]),
            ("sous le n° 1234567", vec![]),
            ("CE, 3 mars 2020", vec![(Sigle, "CE")]),
            ("CE, 3 mars 2020, n° 12", vec![(Sigle, "CE")]),
            ("Conseil d'État, formation", vec![(CourtA, "Conseil d'État")]),
        ] {
            assert_eq!(run(scan, input, &anchors), [], "{input}");
        }
    }
}
