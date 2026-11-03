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

/// `ce_num [(- | /) ce_num]` : un numéro de requête, ou deux numéros que relie un tiret ou une
/// barre sans espace (`194658-196116`, `194658/196116`, forme des requêtes jointes, U11 B1). Le
/// lien exige deux numéros de 4 à 6 chiffres hors 1500-2099 (ni une année, ni un numéro de texte
/// comme `2021-1234`) et aucun chiffre ni point après le second (`194658-196116-5` n'est rien).
fn requete_run(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    if let Some((e, n)) = requete(c, i) {
        return Some((e, vec![n]));
    }
    c.left_ok(i).then_some(())?;
    let first = c.digits(i, 4, 6)?;
    (c.is(first, '-') || c.is(first, '/')).then_some(())?;
    let start = first.saturating_add(1);
    let second = c.digits(start, 4, 6).filter(|&e| c.num_end_ok(e))?;
    (c.year4(i).is_none() && c.year4(start).is_none()).then_some(())?;
    Some((
        second,
        vec![CaseNumber::CeRequete(c.val(i, first)), CaseNumber::CeRequete(c.val(start, second))],
    ))
}

/// `ce_run { LIST_SEP ce_run }` : suite de numéros de requête.
fn request_list(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let (mut end, mut numbers) = requete_run(c, i)?;
    while let Some((e, next)) = c.list_sep(end).and_then(|p| requete_run(c, p)) {
        numbers.extend(next);
        end = e;
    }
    Some((end, numbers))
}

/// `ce_numbers` : `[req. | requête] (n° | nos) ce_num { LIST_SEP ce_num }` ou un numéro seul.
fn ce_numbers(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let p = c.abbr(i, "req").or_else(|| c.kwb(i, "requete")).and_then(|e| c.ws(e)).unwrap_or(i);
    if let Some((q, _)) = c.num_sign(p, true) {
        return request_list(c, q);
    }
    requete_run(c, i)
}

/// Longueur maximale, en caractères, de la zone « parties ».
const PARTIES_MAX: usize = 120;

/// Caractères de la fenêtre où chercher la date d'une référence d'une autre juridiction.
const FOREIGN_DATE_WINDOW: usize = 60;

/// Dernier caractère non blanc avant `k` : position et valeur.
fn previous(c: &Cur<'_>, k: usize) -> Option<(usize, char)> {
    (0..k).rev().find_map(|i| c.ch(i).filter(|&ch| ch != ' ').map(|ch| (i, ch)))
}

/// Le mot « et » finit juste avant `k` (blancs exceptés).
fn after_et(c: &Cur<'_>, k: usize) -> bool {
    previous(c, k).is_some_and(|(i, ch)| {
        ch == 't' && i.checked_sub(1).is_some_and(|e| c.is(e, 'e') && c.left_ok(e))
    })
}

/// En-tête d'une décision du Conseil d'État en `k` : juridiction, formation facultative, date.
fn header_at(c: &Cur<'_>, k: usize) -> bool {
    court(c, k)
        .is_some_and(|(e, long)| c.sep(e).and_then(|p| formation_and_date(c, p, long)).is_some())
}

/// Une référence à une autre juridiction commence en `k` : TA, CAA, CEDH, CJUE, Cass., Cons.
/// const., et, quand une date la suit de près, Cour, Tribunal, Trib. (un nom de partie comme
/// « Cour nationale du droit d'asile » ne ferme rien).
fn foreign_court_at(c: &Cur<'_>, k: usize) -> bool {
    let named = ["TA", "CAA", "CEDH", "CJUE"].iter().any(|w| c.litb(k, w).is_some())
        || c.abbr(k, "cass").is_some()
        || c.abbr(k, "cons").and_then(|e| c.abbr(c.ws(e)?, "const")).is_some();
    named
        || (c.ch(k).is_some_and(char::is_uppercase)
            && (c.kwb(k, "cour").or_else(|| c.kwb(k, "tribunal")).or_else(|| c.abbr(k, "trib")))
                .is_some()
            && (k..=k + FOREIGN_DATE_WINDOW).any(|m| c.left_ok(m) && c.date_link(m).is_some()))
}

/// Une autre référence commence en `k` (profondeur nulle) : la zone « parties » s'arrête là.
/// R1' « ; » suivi d'une juridiction ou d'une date ; R2 en-tête complet ; R2' juridiction et
/// séparateur après « . » ou « et » ; R3' formation et date après « , », « . » ou « ; » ; R4 page
/// du Lebon ; R5 autre juridiction. Une date nue après une virgule ou « du » + date ne ferment rien.
fn closes_zone(c: &Cur<'_>, k: usize) -> bool {
    if c.is(k, ';') {
        let next = c.ws0(k + 1);
        return court(c, next).is_some() || c.date(next).is_some();
    }
    if !c.left_ok(k) {
        return false;
    }
    let stop = previous(c, k).map(|(_, ch)| ch);
    header_at(c, k)
        || (matches!(stop, Some(',' | '.' | ';'))
            && formation_and_date(c, k, false).is_some_and(|(formation, _)| formation.is_some()))
        || ((stop == Some('.') || after_et(c, k))
            && court(c, k).is_some_and(|(e, _)| c.sep(e).is_some()))
        || page(c, k).is_some()
        || foreign_court_at(c, k)
}

/// Virgules où la zone « parties » qui commence en `p` peut finir : à profondeur nulle (hors
/// `(…)`, `[…]`, `«…»`), dans les 120 premiers caractères, avant la première autre référence.
/// Un fermant sans ouvrant arrête la zone.
fn parties_commas(c: &Cur<'_>, p: usize) -> Vec<usize> {
    let mut depth = 0usize;
    let mut found = Vec::new();
    for k in p..=p + PARTIES_MAX {
        let Some(ch) = c.ch(k) else { break };
        if depth == 0 && closes_zone(c, k) {
            break;
        }
        match ch {
            '(' | '[' | '«' => depth = depth.saturating_add(1),
            ')' | ']' | '»' => match depth.checked_sub(1) {
                Some(outer) => depth = outer,
                None => break,
            },
            ',' if depth == 0 && k > p => found.push(k),
            _ => {}
        }
    }
    found
}

/// `parties SEP ce_numbers` : texte sans `n°` ni chiffre de tête, 120 caractères au plus, jusqu'à
/// la première virgule suivie de numéros.
fn parties_then_numbers(c: &Cur<'_>, p: usize) -> Option<(String, usize, Vec<CaseNumber>)> {
    (!c.digit(p)).then_some(())?;
    parties_commas(c, p).into_iter().find_map(|j| {
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

    use super::super::cassation::testing::{
        blank, date, decision, matched, only, read, run, with_court,
    };
    use super::*;
    use crate::citation::scan::AnchorClass::{Court as CourtA, Sigle};
    use crate::citation::types::{DecisionCite, Flag};
    use crate::date::CivilDate;

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

    #[test]
    fn formations_cut_short_are_not_formations() {
        for text in ["2e", "2e ch.", "2e chambres", "2e ch. xyz"] {
            assert_eq!(read(text, |c| chambers_together(c, 0)), None, "{text}");
        }
        assert_eq!(read("2e", |c| chamber_alone(c, 0)), None);
        for text in
            ["7 / x", "7 / 5", "7 / 5 xyz", "4e", "4e et", "4e et xyz", "4e et 5e", "4e et 5e xyz"]
        {
            assert_eq!(read(text, |c| subsections_together(c, 0)), None, "{text}");
        }
        for text in ["6e", "6e xyz"] {
            assert_eq!(read(text, |c| subsection_alone(c, 0)), None, "{text}");
        }
        // Une formation que rien ne sépare de la date n'est pas lue, la date non plus.
        assert!(read("ord.3 mars 2020", |c| formation_and_date(c, 0, false)).is_none());
        // Titre Légifrance dont la formation libre n'est pas suivie d'une date.
        assert!(read("texte libre, xyz", |c| free_formation(c, 0)).is_none());
    }

    #[test]
    fn court_spellings_and_registration_numbers_cut_short() {
        assert_eq!(read("Cons.d'Etat", |c| court(c, 0)), None);
        assert_eq!(read("Cons. d'Etat", |c| court(c, 0)), Some((12, false)));
        assert_eq!(read("a123456", |c| requete_end(c, 1)), None);
        assert_eq!(read("123456", |c| requete_end(c, 0)), Some(6));
        assert!(read("CE xyz", |c| header(c, 0)).is_none());
        assert!(read("CE n° 12", |c| header(c, 0)).is_none());
    }

    /// Décisions lues par `scan` : texte couvert, date et numéros de requête.
    fn read_all(
        input: &str,
        anchors: &[(AnchorClass, &str)],
    ) -> Vec<(String, Option<CivilDate>, Vec<CaseNumber>)> {
        run(scan, input, anchors)
            .iter()
            .map(|c| {
                let d = decision(c);
                (matched(input, c).to_owned(), d.date, d.numbers.clone())
            })
            .collect()
    }

    /// Virgules de la zone « parties » de `text` lue depuis 0 (positions en caractères).
    fn commas(text: &str) -> Vec<usize> {
        read(text, |c| parties_commas(c, 0))
    }

    /// Virgules de `text` situées entre 1 et `PARTIES_MAX` et avant `marker`, ou toutes sans.
    fn commas_before(text: &str, marker: &str) -> Vec<usize> {
        let limit = text.find(marker).map_or(usize::MAX, |byte| text[..byte].chars().count());
        text.chars()
            .enumerate()
            .filter(|&(i, c)| c == ',' && (1..=PARTIES_MAX).contains(&i) && i < limit)
            .map(|(i, _)| i)
            .collect()
    }

    /// Revue vague 2 (U08), limite connue : la zone « parties » suit les parenthèses et les
    /// guillemets. Une `(` ou une `«` sans fermant masque les virgules qui suivent, et un `)`
    /// isolé arrête la zone : la référence que la 1.1.0 lisait n'est plus lue (l'écriture
    /// équilibrée l'est toujours).
    #[test]
    fn an_unbalanced_opener_or_closer_hides_the_commas_after_it() {
        let balanced = "Société X (en liquidation), n° 123456";
        assert_eq!(commas(balanced), [26]);
        for text in [
            "Société X (en liquidation, n° 123456",
            "Société X « en liquidation, n° 123456",
            "Société X en liquidation), n° 123456",
        ] {
            assert_eq!(commas(text), Vec::<usize>::new(), "{text}");
        }
    }

    /// U08 (a) : une seconde référence qui porte l'ancre « CE » est lue seule, avec sa date et
    /// son numéro ; la date de la première ne lui est jamais prêtée.
    #[test]
    fn two_references_are_never_one_decision() {
        for (input, anchors, wanted) in [
            (
                "CE, Ass., 30 mars 1966, Compagnie générale d'énergie radio-électrique, p. 257 ; CE, 23 juillet 2010, Soc. X, n° 328757.",
                [(Sigle, "CE, Ass."), (Sigle, "CE, 23")],
                "CE, 23 juillet 2010, Soc. X, n° 328757",
            ),
            (
                "CE, Ass., 30 mars 1966, p. 257 ; CE, 23 juillet 2010, n° 328757.",
                [(Sigle, "CE, Ass."), (Sigle, "CE, 23")],
                "CE, 23 juillet 2010, n° 328757",
            ),
            (
                "CE, Ass., 30 mars 1966, Cie X, p. 257, et CE, 23 juillet 2010, n° 328757.",
                [(Sigle, "CE, Ass."), (Sigle, "CE, 23")],
                "CE, 23 juillet 2010, n° 328757",
            ),
            (
                "CE, Ass., 30 mars 1966, Compagnie générale, Leb. p. 257. CE, 23 juillet 2010, Société Y, n° 328757, Leb.",
                [(Sigle, "CE, Ass."), (Sigle, "CE, 23")],
                "CE, 23 juillet 2010, Société Y, n° 328757, Leb.",
            ),
            (
                "CE, Ass., 30 mars 1966, Cie X puis CE, 23 juillet 2010, n° 328757.",
                [(Sigle, "CE, Ass."), (Sigle, "CE, 23")],
                "CE, 23 juillet 2010, n° 328757",
            ),
            (
                "CE, Ass., 30 mars 1966, Cie X (voir CE, 23 juillet 2010, n° 328757).",
                [(Sigle, "CE, Ass."), (Sigle, "CE, 23")],
                "CE, 23 juillet 2010, n° 328757",
            ),
            (
                "Conseil d'État, Assemblée, 30 mars 1966, Cie X ; Conseil d'État, 23 juillet 2010, Y, n° 328757",
                [(CourtA, "Conseil d'État, Ass"), (CourtA, "Conseil d'État, 23")],
                "Conseil d'État, 23 juillet 2010, Y, n° 328757",
            ),
        ] {
            assert_eq!(
                read_all(input, &anchors),
                [(wanted.to_owned(), date(2010, 7, 23), req(328_757))],
                "{input}"
            );
        }
    }

    /// U08 (b) : sans ancre « CE » (forme du Lebon), sans date ou sans numéro, la seconde référence
    /// n'est pas lue et ne prête rien à la première.
    #[test]
    fn second_reference_without_an_anchor_is_not_read() {
        for input in [
            "CE, Ass., 30 mars 1966, Cie X, p. 257 ; Section, 2010-07-23, Y, n° 328757.",
            "CE, Ass., 30 mars 1966, Cie X, p. 257. Section, 23 juillet 2010, Y, n° 328757.",
            "CE, Ass., 30 mars 1966, Cie X ; 23 juillet 2010, Y, n° 328757.",
            "CE, Ass., 30 mars 1966, Cie X, Sect., 23 juillet 2010, Y, n° 328757.",
        ] {
            assert!(read_all(input, &[(Sigle, "CE, Ass.")]).is_empty(), "{input}");
        }
    }

    #[test]
    fn a_second_reference_without_a_date_never_lends_its_number() {
        for input in [
            "CE, Ass., 30 mars 1966, Cie X ; CE, Section, Soc. Y, n° 328757.",
            "CE, Ass., 30 mars 1966, Cie X. CE, Section, Soc. Y, n° 328757.",
            "CE, Ass., 30 mars 1966, Cie X, et CE, Section, Soc. Y, n° 328757.",
            "CE, Ass., 30 mars 1966, Cie X et CE, Section, Soc. Y, n° 328757.",
        ] {
            assert!(
                read_all(input, &[(Sigle, "CE, Ass."), (Sigle, "CE, Sec")]).is_empty(),
                "{input}"
            );
        }
    }

    /// Chaque règle de fermeture de la zone, seule : les virgules gardées s'arrêtent au repère.
    #[test]
    fn parties_stop_at_the_next_reference() {
        for (text, marker) in [
            // R1' : « ; » suivi d'une juridiction, ou d'une date.
            ("Cie X, Y ; CE xyz, 123456", ";"),
            ("Cie X, Y ; 23 juillet 2010, Z, n° 328757", ";"),
            // R2 : en-tête complet, avec ou sans formation, en toutes lettres.
            ("Cie X, Y puis CE, 23 juillet 2010, n° 328757", "CE,"),
            ("Cie X, Y puis CE, Ass., 23 juillet 2010, n° 328757", "CE,"),
            ("Cie X, Y puis Conseil d'État, 23 juillet 2010, n° 328757", "Conseil"),
            // R2' : juridiction suivie d'un séparateur, après « . » ou « et ».
            ("Cie X, Y. CE, Section, Soc. Z, n° 328757", "CE,"),
            ("Cie X, Y, et CE, Section, Soc. Z, n° 328757", "CE,"),
            ("Cie X, Y et CE, Section, Soc. Z, n° 328757", "CE,"),
            // R3' : formation et date, après « , », « . » ou « ; ».
            ("Cie X, Y, Section, 2010-07-23, Z, n° 328757", "Section"),
            ("Cie X, Y. Section, 2010-07-23, Z, n° 328757", "Section"),
            ("Cie X, Y ; Section, 2010-07-23, Z, n° 328757", "Section"),
            // R4 : page du Lebon.
            ("Cie X, p. 257, n° 328757", "p."),
            ("Cie X, Y p. 1, n° 328757", "p."),
            // R5 : autre juridiction.
            ("Cie X, TA Lyon, n° 123456", "TA"),
            ("Cie X, CAA Paris, n° 123456", "CAA"),
            ("Cie X, Cass. crim., n° 123456", "Cass."),
            ("Cie X, Cons. const., n° 123456", "Cons."),
            ("Cie X, CEDH, n° 123456", "CEDH"),
            ("Cie X, CJUE, n° 123456", "CJUE"),
            ("Cie X, Cour de cassation du 4 mai 2000, Y, n° 123456", "Cour"),
            ("Cie X, Tribunal de Lyon du 4 mai 2000, Y, n° 123456", "Tribunal"),
            ("Cie X, Trib. adm. de Lyon, 4 mai 2000, Y, n° 123456", "Trib."),
        ] {
            let want = commas_before(text, marker);
            assert_eq!(commas(text), want, "{text}");
            assert!(want.len() < text.matches(',').count(), "le repère doit couper : {text}");
        }
    }

    /// Les mêmes mots, quand ils ne commencent pas une autre référence, ne ferment rien.
    #[test]
    fn parties_words_that_start_no_reference_close_nothing() {
        for text in [
            "Nicolo ; Boisdet, n° 108243",
            "Comité d'entreprise CE Bretagne, n° 328757",
            "Cie X, CE Bretagne, 12 mai 2018, n° 123456",
            "Cie X, CE, Bretagne, n° 123456",
            "Rouquette, 12 mai 2018, n° 194658",
            "Rouquette: 12 mai 2018, n° 194658",
            "Commune de Lyon (arrêté du 5 mai 2018), n° 123456",
            "Commune, arrêté du 5 mai 2018, n° 123456",
            "Cie X, p. x, n° 123456",
            "Cie X, p., n° 123456",
            "Cie Xp. 257, n° 123456",
            "Cie X, ta Lyon, n° 123456",
            "OFPRA c/ Cour nationale du droit d'asile, n° 123456",
            "Société Tribunal, n° 123456",
            "contre la cour de Lyon du 4 mai 2000, n° 123456",
            "Cie X, Ass. générale, n° 123456",
            // « et » seul ferme après « . » ou « et » : un mot qui finit par « t » ou « et » non.
            "Cie Dupont CE, Section, Soc. Z, n° 328757",
            "Cie Dupuiset CE, Section, Soc. Z, n° 328757",
            "Cie e, CE, Section, Soc. Z, n° 328757",
            "Cie X, Cons.xyz, n° 123456",
            // Une date trop loin derrière « Cour » (plus de 60 caractères) ne la fait pas fermer.
            "Cie X, Cour nationale du droit d'asile et ses services, représentée par son président, n° 123456 du 4 mai 2000",
        ] {
            assert_eq!(commas(text), commas_before(text, "\u{0}"), "{text}");
        }
    }

    /// Un groupe parenthésé n'ouvre ni ne ferme rien : ses virgules et ses références sont les
    /// siennes. Un fermant sans ouvrant, ou un groupe jamais fermé, ne laisse aucune virgule.
    #[test]
    fn parties_groups_hide_their_commas_and_references() {
        let text = "Commune (CE, 5 mai 2018, n° 111111), n° 123456";
        assert_eq!(commas(text), [35]);
        for text in [
            "Commune [CE, 5 mai 2018 ; CE, 5 mai 2018, x], n° 123456",
            "Commune «CE, 5 mai 2018, p. 1, TA Lyon», n° 123456",
            "Commune (a (b, c), d), n° 123456",
        ] {
            let last = text.chars().count() - ", n° 123456".chars().count();
            assert_eq!(commas(text), [last], "{text}");
        }
        assert_eq!(commas("Commune X), n° 123456"), Vec::<usize>::new());
        assert_eq!(commas("Commune (X, n° 123456"), Vec::<usize>::new());
        assert_eq!(commas("Commune ]X, n° 123456"), Vec::<usize>::new());
        assert_eq!(commas("Commune »X, n° 123456"), Vec::<usize>::new());
    }

    /// Bornes de la zone : une virgule en 0 ne compte pas, de 1 à 120 oui, en 121 non.
    #[test]
    fn parties_zone_is_one_hundred_and_twenty_characters() {
        for (position, kept) in [(0, false), (1, true), (119, true), (120, true), (121, false)] {
            let text = format!("{}, 123456", "x".repeat(position));
            assert_eq!(commas(&text).contains(&position), kept, "{position}");
            let parties = read(&text, |c| parties_then_numbers(c, 0));
            assert_eq!(parties.is_some(), kept, "{position}");
        }
    }

    /// Garde : des parties qui portent « du » + date restent des parties.
    #[test]
    fn parties_with_a_date_after_du_stay_parties() {
        let input = "CE, 3 mars 2020, Commune de Lyon (arrêté du 5 mai 2018), n° 123456";
        let got = only(run(scan, input, &[(Sigle, "CE")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(
            decision(&got).parties.as_deref(),
            Some("Commune de Lyon (arrêté du 5 mai 2018)")
        );
        assert_eq!(decision(&got).numbers, req(123_456));
    }

    /// Garde : un « ; » entre deux noms de parties ne coupe pas la décision.
    #[test]
    fn parties_with_a_semicolon_between_names_stay_one_decision() {
        let input = "CE, 3 mars 2020, Nicolo ; Boisdet, n° 108243";
        let got = only(run(scan, input, &[(Sigle, "CE")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(decision(&got).parties.as_deref(), Some("Nicolo ; Boisdet"));
        assert_eq!(decision(&got).numbers, req(108_243));
    }

    /// Garde : une date nue après une virgule reste dans les parties.
    #[test]
    fn a_bare_date_after_a_comma_in_parties_stays_in_parties() {
        let input = "CE, 5 mars 1999, Rouquette, 12 mai 2018, n° 194658";
        let got = only(run(scan, input, &[(Sigle, "CE")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(decision(&got).date, date(1999, 3, 5));
        assert_eq!(decision(&got).parties.as_deref(), Some("Rouquette, 12 mai 2018"));
    }

    /// Les cas limites des parties, décidés explicitement (R1' à R5 et leurs exceptions).
    #[test]
    fn legit_parties_with_semicolon_or_comma_date_are_decided_explicitly() {
        for (input, parties) in [
            (
                "CE, 3 mars 2020, Ministre de l'intérieur ; Préfet du Rhône, n° 123456",
                Some("Ministre de l'intérieur ; Préfet du Rhône"),
            ),
            (
                "CE, 3 mars 2020, OFPRA c/ Cour nationale du droit d'asile, n° 123456",
                Some("OFPRA c/ Cour nationale du droit d'asile"),
            ),
            (
                "CE, 3 mars 2020, Comité d'entreprise CE Bretagne, n° 123456",
                Some("Comité d'entreprise CE Bretagne"),
            ),
        ] {
            let got = only(run(scan, input, &[(Sigle, "CE")]));
            assert_eq!(decision(&got).parties.as_deref(), parties, "{input}");
        }
    }

    /// Une page du Lebon avant le numéro marque une référence sans numéro : rien n'est lu.
    #[test]
    fn a_lebon_page_closes_the_parties_zone() {
        let input = "CE, 3 mars 2020, Cie X, p. 257, n° 123456";
        assert_eq!(run(scan, input, &[(Sigle, "CE")]), []);
    }

    /// Une référence étrangère à date « du » après une virgule n'est pas avalée.
    #[test]
    fn a_foreign_reference_with_a_du_date_after_a_comma_is_not_swallowed() {
        for input in [
            "CE, 3 mars 2020, Cie X, TA Lyon du 4 mai 2000, Y, n° 123456",
            "CE, 3 mars 2020, Cie X, CAA Paris du 4 mai 2000, Y, n° 123456",
            "CE, 3 mars 2020, Cie X, Cass. civ. 1re du 4 mai 2000, Y, n° 123456",
            "CE, 3 mars 2020, Cie X, CEDH du 4 mai 2000, Y, n° 123456",
        ] {
            assert!(run(scan, input, &[(Sigle, "CE")]).is_empty(), "{input}");
        }
    }

    /// Une référence entre parenthèses ne vole jamais le numéro de la décision qui la contient.
    #[test]
    fn a_reference_inside_parentheses_never_steals_the_outer_number() {
        let input = "CE, 3 mars 2020, Commune (CE, 5 mai 2018, n° 111111), n° 123456";
        assert_eq!(
            read_all(input, &[(Sigle, "CE, 3"), (Sigle, "CE, 5")]),
            [("CE, 5 mai 2018, n° 111111".to_owned(), date(2018, 5, 5), req(111_111))]
        );
    }

    #[test]
    fn parties_and_publication_cut_short() {
        for text in ["12 x, 123456", "Société n° 5, 123456", "Société X, abc"] {
            assert_eq!(read(text, |c| parties_then_numbers(c, 0)), None, "{text}");
        }
        for text in ["p.", "p. x"] {
            assert_eq!(read(text, |c| page(c, 0)), None, "{text}");
        }
        assert_eq!(read("xyz", |c| lebon(c, 0)), None);
    }

    /// Ce que la grammaire lit de `input` : texte repris et numéros de chaque décision.
    fn readings(input: &str) -> Vec<(String, Vec<CaseNumber>)> {
        run(scan, input, &[(Sigle, "CE")])
            .into_iter()
            .map(|got| (matched(input, &got).to_owned(), decision(&got).numbers.clone()))
            .collect()
    }

    /// U11 B1 (AM-83) : le tiret et la barre, sans espace, relient deux numéros de requête ; la
    /// citation est lue en entier, avec ses deux numéros.
    #[test]
    fn joined_request_numbers() {
        let two = vec![CaseNumber::CeRequete(194_658), CaseNumber::CeRequete(196_116)];
        for input in [
            "CE, Ass., 5 mars 1999, n° 194658-196116",
            "CE, Ass., 5 mars 1999, n° 194658/196116",
            "CE, Ass., 5 mars 1999, nos 194658-196116",
            "CE, Ass., 5 mars 1999, Rouquette, n° 194658/196116",
            "CE, Ass., 5 mars 1999, 194658-196116",
        ] {
            assert_eq!(readings(input), [(input.to_owned(), two.clone())], "{input}");
        }
        // Le lien se mêle aux séparateurs de liste, trois numéros au plus lus ici comme au-delà.
        let input = "CE, 5 mars 1999, n° 194658-196116 et 196117";
        let want = vec![
            CaseNumber::CeRequete(194_658),
            CaseNumber::CeRequete(196_116),
            CaseNumber::CeRequete(196_117),
        ];
        assert_eq!(readings(input), [(input.to_owned(), want)]);
        let input = "CE, 5 mars 1999, n° 194658, 196116/196117";
        let want = vec![
            CaseNumber::CeRequete(194_658),
            CaseNumber::CeRequete(196_116),
            CaseNumber::CeRequete(196_117),
        ];
        assert_eq!(readings(input), [(input.to_owned(), want)]);
        // Une mention du Lebon après les deux numéros reste reprise.
        let input = "CE, Ass., 5 mars 1999, n° 194658-196116, publié au recueil Lebon";
        let got = readings(input);
        assert_eq!(got[0].0, input);
        assert_eq!(got[0].1, two);
    }

    /// U11, garde : les listes déjà lues en 1.1.0 le restent.
    #[test]
    fn already_read_lists_of_request_numbers_keep_their_reading() {
        let two = vec![CaseNumber::CeRequete(194_658), CaseNumber::CeRequete(196_116)];
        for input in [
            "CE, Ass., 5 mars 1999, nos 194658, 196116",
            "CE, Ass., 5 mars 1999, n°s 194658 et 196116",
            "CE, Ass., 5 mars 1999, n° 194658 ou 196116",
        ] {
            assert_eq!(readings(input), [(input.to_owned(), two.clone())], "{input}");
        }
    }

    /// U11, garde (garantie 14 : l'espace entre deux numéros reste hors périmètre) : un nombre qui
    /// suit un numéro de requête après une espace, un montant ou une année n'est pas un second
    /// numéro, et les formes à tiret ou barre qui ne sont pas deux numéros ne sont pas liées.
    #[test]
    fn a_number_followed_by_an_amount_or_a_year_keeps_its_1_1_0_span() {
        let one = |n: u32| vec![CaseNumber::CeRequete(n)];
        for (input, text, want) in [
            (
                "CE, 28 mars 2018, n° 458590 15000 euros",
                "CE, 28 mars 2018, n° 458590",
                one(458_590),
            ),
            ("CE, 28 mars 2018, n° 158592 1998", "CE, 28 mars 2018, n° 158592", one(158_592)),
            ("CE, 28 mars 2018, n° 410000 2018", "CE, 28 mars 2018, n° 410000", one(410_000)),
            // Tiret ou barre entourés d'espaces : pas un lien.
            ("CE, 28 mars 2018, n° 410000 - 196116", "CE, 28 mars 2018, n° 410000", one(410_000)),
            ("CE, 28 mars 2018, n° 410000 / 196116", "CE, 28 mars 2018, n° 410000", one(410_000)),
        ] {
            assert_eq!(readings(input), [(text.to_owned(), want)], "{input}");
        }
    }

    /// U11 : la lecture d'un numéro lié s'arrête net hors de ses conditions (début au milieu d'un
    /// mot, chiffres suivis d'une lettre ou d'un autre signe, rien après le lien).
    #[test]
    fn a_joined_request_number_is_cut_short_outside_its_forms() {
        assert_eq!(read("x194658-196116", |c| requete_run(c, 1)), None);
        for text in ["194658x", "194658-196", "194658/196116x"] {
            assert_eq!(read(text, |c| requete_run(c, 0)), None, "{text}");
        }
        assert_eq!(
            read("194658-196116", |c| requete_run(c, 0)),
            Some((13, vec![CaseNumber::CeRequete(194_658), CaseNumber::CeRequete(196_116)]))
        );
    }

    /// U11, négatifs : un numéro suivi d'un tiret ou d'une barre qui n'en fait pas deux numéros
    /// n'est pas une liste, et les numéros de textes ne sont jamais lus comme des requêtes jointes.
    #[test]
    fn ce_joined_numbers_negatives() {
        for input in [
            "CE, 28 mars 2018, n° 410000-5",
            "CE, 28 mars 2018, n° 410000-2018",
            "CE, 28 mars 2018, n° 410000/2018",
            "CE, 28 mars 2018, n° 2021-1234",
            "CE, 28 mars 2018, n° 194658-196116-5",
            "CE, 28 mars 2018, n° 194658-196116.5",
            "CE, 28 mars 2018, n° 194658-1961167",
            "CE, 28 mars 2018, n° 2010-14/22 QPC",
            // Groupement de milliers : limite figée (D16), la citation n'est pas lue.
            "CE, 28 mars 2018, n° 194 658",
            "CE, 28 mars 2018, décret n° 2021-1234",
            "CE, 28 mars 2018, loi n° 58-1067",
        ] {
            // Comme en 1.1.0 : aucune décision n'est lue.
            assert_eq!(readings(input), [], "{input}");
        }
    }
}
