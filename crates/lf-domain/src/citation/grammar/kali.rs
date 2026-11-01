//! Grammaire des conventions collectives (KALI) et des accords d'entreprise (ACCO) (annexe B
//! §4.12).
//!
//! ```text
//! convention = conv_kind [ WS conv_title ] [ WS "du" WS DATE ] [ WS? "(" conv_ids ")" | WS conv_ids ]
//!            | conv_ids
//!            | "Avenant" WS NUM_SIGN INT [ WS "du" WS DATE ] [ WS "à la" WS convention ]
//! conv_ids   = idcc_part [ SEP brochure_part ] | brochure_part [ SEP idcc_part ]
//! ```
//!
//! Deux précisions sur le texte du spec. Un nom de nature seul (« convention collective »,
//! `CCN`, « Accord ») ne cite rien : il faut en plus un titre, une date ou des identifiants. Le
//! titre `conv_title` court jusqu'à « du » DATE, une parenthèse, une ponctuation forte, des
//! identifiants lisibles (« IDCC 1486 ») ou la fin du texte ; au-delà de 120 caractères sans rien
//! de tout cela, ce n'est plus un titre et il est abandonné. Le numéro ACCO (`T09426064936`) n'est pas reconnu en v0.2. Les plages des
//! candidats sont des offsets du texte d'ORIGINE (`Normalized::orig_range`), celles des ancres
//! des offsets de `Normalized::text`.

use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{Ambiguity, CitationRef, ConventionCite, ConventionKind};
use crate::contract::Idcc;

use super::Candidate;
use super::jorf::{DateHit, Hit, Win, scan_with};

/// Longueur maximale d'un titre, en caractères.
const TITLE_MAX: usize = 120;

/// Ponctuation forte, qui termine un titre. La virgule n'en est pas une : les intitulés de KALI
/// en contiennent.
const TITLE_STOPS: &[char] = &['.', ';', ':', '!', '?', '(', ')', '[', ']', '«', '»', '"'];

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    scan_with(n, anchors, &[AnchorClass::Family, AnchorClass::Sigle], &[convention])
}

/// Un identifiant lu : `idcc_part` ou `brochure_part`.
#[derive(Clone, Copy)]
enum IdPart {
    Idcc(Idcc),
    Brochure(u16),
}

/// Les identifiants d'une convention : au plus un IDCC et une brochure.
#[derive(Default)]
struct Ids {
    idcc: Option<Idcc>,
    brochure: Option<u16>,
}

impl Ids {
    fn set(&mut self, part: IdPart) {
        match part {
            IdPart::Idcc(idcc) => self.idcc = Some(idcc),
            IdPart::Brochure(brochure) => self.brochure = Some(brochure),
        }
    }

    fn has(&self, part: IdPart) -> bool {
        match part {
            IdPart::Idcc(_) => self.idcc.is_some(),
            IdPart::Brochure(_) => self.brochure.is_some(),
        }
    }
}

/// `idcc_part` : `IDCC 1486`, `IDCC n° 1486`.
fn idcc_part(w: &Win, p: usize) -> Option<(usize, IdPart)> {
    let q = w.ws0(w.cs(p, "IDCC")?);
    let (q, value) = w.digits(w.num_sign(q).unwrap_or(q), 1, 4)?;
    Some((q, IdPart::Idcc(Idcc::parse(&value.to_string())?)))
}

/// `brochure_part` : `brochure n° 3018`, `brochure JO 3018`.
fn brochure_part(w: &Win, p: usize) -> Option<(usize, IdPart)> {
    let q = w.kw(p, "brochure")?;
    let q = w.ws(q).and_then(|r| w.kw(r, "jo")).unwrap_or(q);
    let q = w.ws0(q);
    let (q, value) = w.digits(w.num_sign(q).unwrap_or(q), 4, 4)?;
    Some((q, IdPart::Brochure(u16::try_from(value).unwrap_or(u16::MAX))))
}

/// `conv_ids` : un identifiant, puis éventuellement l'autre.
fn ids(w: &Win, p: usize) -> Option<(usize, Ids)> {
    let id_part = |p| idcc_part(w, p).or_else(|| brochure_part(w, p));
    let (q, first) = id_part(p)?;
    let mut ids = Ids::default();
    ids.set(first);
    let second = w.sep(q).and_then(id_part).filter(|&(_, part)| !ids.has(part));
    let end = second.as_ref().map_or(q, |(end, _)| *end);
    if let Some((_, part)) = second {
        ids.set(part);
    }
    Some((end, ids))
}

/// `WS? "(" conv_ids ")"`.
fn parenthesized(w: &Win, p: usize) -> Option<(usize, Ids)> {
    let (q, ids) = ids(w, w.cs(w.ws0(p), "(")?)?;
    Some((w.cs(q, ")")?, ids))
}

/// `WS conv_ids` : des identifiants nus, sans parenthèses (« convention collective IDCC 1486 »).
fn bare_ids(w: &Win, p: usize) -> Option<(usize, Ids)> {
    ids(w, w.ws(p)?)
}

/// `conv_title` : texte jusqu'à « du » DATE, une ponctuation forte, des identifiants lisibles
/// (`idcc_part` ou `brochure_part`) ou la fin du texte, d'au plus 120 caractères. `p` est la fin de la nature ; rend la fin du titre (sans espace finale) et son
/// texte.
fn title(w: &Win, p: usize) -> Option<(usize, String)> {
    let start = w.ws(p)?;
    let limit = (start + TITLE_MAX).min(w.len());
    let stops = |c: char, i: usize| {
        TITLE_STOPS.contains(&c)
            || (c == ' ' && (w.du_date(i).is_some() || bare_ids(w, i).is_some()))
    };
    let stop = (p..limit).find(|&i| w.at(i).is_some_and(|c| stops(c, i)));
    let mut end = stop.or((limit == w.len()).then_some(limit))?;
    while end > start && w.at(end - 1) == Some(' ') {
        end -= 1;
    }
    (end > start).then(|| (end, w.text_of(start, end)))
}

/// `conv_kind`.
fn kind(w: &Win, p: usize) -> Option<(usize, ConventionKind)> {
    let annexe = || w.kw(p, "annexe").and_then(|q| w.ws(q)).and_then(|q| w.roman(q));
    w.kw(p, "convention collective nationale")
        .map(|q| (q, ConventionKind::Ccn))
        .or_else(|| w.kw(p, "convention collective").map(|q| (q, ConventionKind::Convention)))
        .or_else(|| w.cs_word(p, "CCN").map(|q| (q, ConventionKind::Ccn)))
        .or_else(|| w.kw(p, "accord national interprofessionnel").map(|q| (q, ConventionKind::Ani)))
        .or_else(|| w.cs_word(p, "ANI").map(|q| (q, ConventionKind::Ani)))
        .or_else(|| w.kw(p, "accord").map(|q| (q, ConventionKind::Accord)))
        .or_else(|| annexe().map(|q| (q, ConventionKind::Annexe)))
}

/// Une convention lue, avant d'être mise en forme.
struct Parsed {
    end: usize,
    kind: Option<ConventionKind>,
    ids: Ids,
    title: Option<String>,
    date: Option<DateHit>,
}

/// Première et deuxième forme de `convention` : une nature, ou des identifiants seuls.
fn core(w: &Win, s: usize) -> Option<Parsed> {
    named(w, s).or_else(|| {
        let (end, ids) = ids(w, s)?;
        Some(Parsed { end, kind: None, ids, title: None, date: None })
    })
}

/// `conv_kind [ WS conv_title ] [ WS "du" WS DATE ] [ WS? "(" conv_ids ")" ]`.
fn named(w: &Win, s: usize) -> Option<Parsed> {
    let (p, kind) = kind(w, s)?;
    let title = title(w, p);
    let p = title.as_ref().map_or(p, |(end, _)| *end);
    let date = w.du_date(p);
    let p = date.map_or(p, |d| d.end);
    let parens = parenthesized(w, p).or_else(|| bare_ids(w, p));
    let end = parens.as_ref().map_or(p, |(end, _)| *end);
    let informative = title.is_some() || date.is_some() || parens.is_some();
    informative.then(|| Parsed {
        end,
        kind: Some(kind),
        ids: parens.map(|(_, ids)| ids).unwrap_or_default(),
        title: title.map(|(_, text)| text),
        date,
    })
}

/// Reconnaissance d'une convention de span `s..end`.
fn hit(s: usize, end: usize, cite: ConventionCite, date: Option<DateHit>) -> Hit {
    let mut hit = Hit::new(s, end, CitationRef::Convention(cite));
    hit.flags.extend(date.and_then(|d| d.flag()));
    hit
}

/// Troisième forme : `"Avenant" WS NUM_SIGN INT [ WS "du" WS DATE ] [ WS "à la" WS convention ]`.
fn avenant(w: &Win, s: usize) -> Option<Hit> {
    let p = w.num_sign(w.ws(w.kw(s, "avenant")?)?)?;
    let (q, number) = w.digits(p, 1, 9)?;
    let date = w.du_date(q);
    let q = date.map_or(q, |d| d.end);
    let nested =
        w.ws(q).and_then(|r| w.kw(r, "a la")).and_then(|r| w.ws(r)).and_then(|r| core(w, r));
    let known = nested.is_some();
    let (end, ids, title) = nested.map_or((q, Ids::default(), None), |c| (c.end, c.ids, c.title));
    let cite = ConventionCite {
        kind: Some(ConventionKind::Avenant),
        idcc: ids.idcc,
        brochure: ids.brochure,
        avenant: Some(number),
        title_hint: title,
        date: date.and_then(|d| d.date),
    };
    let mut hit = hit(s, end, cite, date);
    hit.ambiguity = (!known).then_some(Ambiguity::ConventionInconnue);
    Some(hit)
}

/// `convention`, lue au caractère `s` de la fenêtre.
fn convention(w: &Win, s: usize) -> Option<Hit> {
    core(w, s)
        .map(|c| {
            let cite = ConventionCite {
                kind: c.kind,
                idcc: c.ids.idcc,
                brochure: c.ids.brochure,
                avenant: None,
                title_hint: c.title,
                date: c.date.and_then(|d| d.date),
            };
            hit(s, c.end, cite, c.date)
        })
        .or_else(|| avenant(w, s))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::grammar::jorf::testkit::{Found, find};
    use crate::citation::scan::AnchorClass;
    use crate::citation::types::{Ambiguity, CitationRef, ConventionCite, ConventionKind, Flag};
    use crate::contract::Idcc;
    use crate::date::CivilDate;

    fn found(input: &str, needles: &[&str]) -> Vec<Found> {
        find(scan, input, AnchorClass::Family, needles)
    }

    fn one(input: &str, needles: &[&str]) -> Found {
        let mut all = found(input, needles);
        assert_eq!(all.len(), 1, "{input:?} : {all:?}");
        all.remove(0)
    }

    #[derive(Clone, Copy, Default)]
    struct Want {
        kind: Option<ConventionKind>,
        idcc: Option<u16>,
        brochure: Option<u16>,
        avenant: Option<u32>,
        title: Option<&'static str>,
        date: Option<(u16, u8, u8)>,
    }

    fn convention(want: Want) -> CitationRef {
        CitationRef::Convention(ConventionCite {
            kind: want.kind,
            idcc: want.idcc.map(|n| Idcc::parse(&n.to_string()).expect("IDCC valide")),
            brochure: want.brochure,
            avenant: want.avenant,
            title_hint: want.title.map(str::to_owned),
            date: want.date.and_then(|(y, m, d)| CivilDate::new(y, m, d)),
        })
    }

    #[test]
    fn p180_ccn_title_stops_before_the_date_and_the_newline_is_excluded() {
        let f = one(
            "Convention collective nationale de l'édition du 14 janvier 2000\n",
            &["Convention collective"],
        );
        assert_eq!(f.text, "Convention collective nationale de l'édition du 14 janvier 2000");
        let want = Want {
            kind: Some(ConventionKind::Ccn),
            title: Some("de l'édition"),
            date: Some((2000, 1, 14)),
            ..Want::default()
        };
        assert_eq!(f.value, convention(want));
        assert_eq!((f.ambiguity, f.flags), (None, vec![]));
    }

    #[test]
    fn p181_avenant_without_convention_is_flagged_unknown() {
        let input = "Avenant n° 18 du 19 juillet 2002 relatif à la médaille d'honneur du travail";
        let f = one(input, &["Avenant"]);
        assert_eq!(f.text, "Avenant n° 18 du 19 juillet 2002");
        let want = Want {
            kind: Some(ConventionKind::Avenant),
            avenant: Some(18),
            date: Some((2002, 7, 19)),
            ..Want::default()
        };
        assert_eq!(f.value, convention(want));
        assert_eq!(f.ambiguity, Some(Ambiguity::ConventionInconnue));
    }

    #[test]
    fn p182_ccn_with_title_and_both_identifiers_in_parentheses() {
        let f = one("CCN Syntec (IDCC 1486, brochure n° 3018)", &["IDCC"]);
        assert_eq!(f.text, "CCN Syntec (IDCC 1486, brochure n° 3018)");
        let want = Want {
            kind: Some(ConventionKind::Ccn),
            idcc: Some(1486),
            brochure: Some(3018),
            title: Some("Syntec"),
            ..Want::default()
        };
        assert_eq!(f.value, convention(want));
    }

    #[test]
    fn p183_and_p185_idcc_alone() {
        let f = one("IDCC n° 1486", &["IDCC"]);
        assert_eq!(f.text, "IDCC n° 1486");
        assert_eq!(f.value, convention(Want { idcc: Some(1486), ..Want::default() }));
        let g = one("IDCC 275", &["IDCC"]);
        assert_eq!(g.value, convention(Want { idcc: Some(275), ..Want::default() }));
        assert_eq!(one("IDCC1486", &["IDCC"]).text, "IDCC1486");
    }

    #[test]
    fn p184_title_stops_at_the_parenthesis_and_the_determiner_is_excluded() {
        let f = one("la convention collective nationale de l'édition (IDCC 2121)", &["IDCC"]);
        assert_eq!(f.text, "convention collective nationale de l'édition (IDCC 2121)");
        let want = Want {
            kind: Some(ConventionKind::Ccn),
            idcc: Some(2121),
            title: Some("de l'édition"),
            ..Want::default()
        };
        assert_eq!(f.value, convention(want));
    }

    #[test]
    fn n17_avenant_alone_is_ambiguous() {
        let f = one("l'avenant n° 18", &["avenant"]);
        assert_eq!(f.text, "avenant n° 18");
        assert_eq!(f.ambiguity, Some(Ambiguity::ConventionInconnue));
        let want =
            Want { kind: Some(ConventionKind::Avenant), avenant: Some(18), ..Want::default() };
        assert_eq!(f.value, convention(want));
    }

    #[test]
    fn an_avenant_takes_its_convention_after_a_la() {
        let input = "Avenant n° 3 du 5 mars 2019 à la convention collective nationale de \
                     l'édition (IDCC 2121), applicable";
        let f = one(input, &["Avenant"]);
        assert_eq!(
            f.text,
            "Avenant n° 3 du 5 mars 2019 à la convention collective nationale de l'édition \
             (IDCC 2121)"
        );
        let want = Want {
            kind: Some(ConventionKind::Avenant),
            idcc: Some(2121),
            avenant: Some(3),
            title: Some("de l'édition"),
            date: Some((2019, 3, 5)),
            ..Want::default()
        };
        assert_eq!(f.value, convention(want));
        assert_eq!(f.ambiguity, None);
        // Une convention illisible après « à la » laisse l'avenant sans convention.
        let g = one("Avenant n° 3 à la société", &["Avenant"]);
        assert_eq!(g.text, "Avenant n° 3");
        assert_eq!(g.ambiguity, Some(Ambiguity::ConventionInconnue));
    }

    #[test]
    fn an_avenant_needs_a_number_sign_and_a_clean_number() {
        assert_eq!(found("Avenant 18", &["Avenant"]), []);
        assert_eq!(found("Avenant n° 18bis", &["Avenant"]), []);
        assert_eq!(found("Avenant n° x", &["Avenant"]), []);
        assert_eq!(found("Avenant n°", &["Avenant"]), []);
        assert_eq!(found("Avenant", &["Avenant"]), []);
        assert_eq!(found("Avenants n° 3", &["Avenants"]), []);
    }

    #[test]
    fn identifiers_come_in_either_order_without_repeating() {
        let both = one("brochure JO n° 3018, IDCC 1486", &["IDCC"]);
        assert_eq!(both.text, "brochure JO n° 3018, IDCC 1486");
        let want = Want { idcc: Some(1486), brochure: Some(3018), ..Want::default() };
        assert_eq!(both.value, convention(want));
        let brochure = one("brochure 3018", &["brochure"]);
        assert_eq!(brochure.value, convention(Want { brochure: Some(3018), ..Want::default() }));
        // Un second IDCC n'est pas lu : un seul de chaque, séparés par SEP.
        let twice = one("IDCC 1486, IDCC 1487", &["IDCC"]);
        assert_eq!(twice.text, "IDCC 1486");
        let plain = one("IDCC 1486 brochure 3018 et suite", &["IDCC"]);
        assert_eq!(plain.text, "IDCC 1486 brochure 3018");
        let glued = one("IDCC 1486 ; brochure JO3018", &["IDCC"]);
        assert_eq!(glued.text, "IDCC 1486");
    }

    #[test]
    fn malformed_identifiers_are_not_citations() {
        for input in [
            "IDCC 0000",
            "IDCC 12345",
            "IDCC n°",
            "IDCC",
            "IDCC abc",
            "brochure n° 301",
            "brochure n° 30189",
            "brochure",
            "T09426064936",
        ] {
            assert_eq!(found(input, &["IDCC", "brochure", "T09"]), [], "{input}");
        }
    }

    #[test]
    fn the_other_kinds_need_something_besides_their_name() {
        let long = one("Accord national interprofessionnel du 11 janvier 2013", &["Accord"]);
        let want =
            Want { kind: Some(ConventionKind::Ani), date: Some((2013, 1, 11)), ..Want::default() };
        assert_eq!(long.value, convention(want));
        let short = one("ANI du 11 janvier 2013", &["ANI"]);
        assert_eq!(short.text, "ANI du 11 janvier 2013");
        assert_eq!(
            short.value,
            convention(Want {
                kind: Some(ConventionKind::Ani),
                date: Some((2013, 1, 11)),
                ..Want::default()
            })
        );
        let branch = one("Accord de branche du 5 mai 2020", &["Accord"]);
        assert_eq!(
            branch.value,
            convention(Want {
                kind: Some(ConventionKind::Accord),
                title: Some("de branche"),
                date: Some((2020, 5, 5)),
                ..Want::default()
            })
        );
        let annex = one("Annexe II du 3 mars 2010", &["Annexe"]);
        assert_eq!(annex.text, "Annexe II du 3 mars 2010");
        assert_eq!(
            annex.value,
            convention(Want {
                kind: Some(ConventionKind::Annexe),
                date: Some((2010, 3, 3)),
                ..Want::default()
            })
        );
        let plain = one("convention collective du 14 janvier 2000", &["convention"]);
        assert_eq!(
            plain.value,
            convention(Want {
                kind: Some(ConventionKind::Convention),
                date: Some((2000, 1, 14)),
                ..Want::default()
            })
        );
        // Le nom seul ne cite rien.
        for input in [
            "convention collective",
            "convention collective nationale",
            "Accord",
            "ANI",
            "CCN",
            "Annexe II",
            "Annexe 3 du 3 mars 2010",
            "ccn Syntec",
        ] {
            assert_eq!(found(input, &["onvention", "Accord", "ANI", "CCN", "Annexe", "ccn"]), []);
        }
    }

    #[test]
    fn a_lowercase_ccn_is_not_the_sigle() {
        let f = one("ccn Syntec (IDCC 1486)", &["IDCC"]);
        assert_eq!(f.text, "IDCC 1486");
    }

    #[test]
    fn a_title_ends_at_strong_punctuation_or_the_end_of_the_text() {
        let f = one("Convention collective nationale des commerces. Le salarié", &["Convention"]);
        assert_eq!(f.text, "Convention collective nationale des commerces");
        let g = one("Convention collective nationale des commerces, de gros", &["Convention"]);
        assert_eq!(g.text, "Convention collective nationale des commerces, de gros");
        // Quarante mots sans ponctuation : au-delà de 120 caractères, ce n'est plus un titre.
        let long = format!("Convention collective nationale {}", "mot ".repeat(40));
        assert_eq!(found(&long, &["Convention"]), []);
    }

    #[test]
    fn an_impossible_date_is_kept_raw_with_a_flag() {
        let f = one("convention collective nationale du 31 février 2000", &["convention"]);
        assert_eq!(f.flags, vec![Flag::DateInvalide]);
        assert_eq!(f.text, "convention collective nationale du 31 février 2000");
        let g = one("Avenant n° 2 du 12 thermidor an XII", &["Avenant"]);
        assert_eq!(g.flags, vec![Flag::DateRepublicaine]);
    }

    #[test]
    fn an_unclosed_parenthesis_keeps_the_title_and_the_identifier_apart() {
        let all = found("CCN Syntec (IDCC 1486", &["IDCC"]);
        let texts: Vec<&str> = all.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(texts, ["CCN Syntec", "IDCC 1486"]);
        // Une parenthèse qui ne contient pas d'identifiants n'est pas absorbée.
        let all = found("CCN Syntec (art. 3) IDCC 1486", &["IDCC"]);
        let texts: Vec<&str> = all.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(texts, ["CCN Syntec", "IDCC 1486"]);
    }

    #[test]
    fn l1_05_bare_identifiers_after_the_kind_are_read_not_swallowed_by_the_title() {
        // Cas réel de l'E2E (resolve-7) : sans parenthèses, « IDCC 1486 » était avalé comme titre.
        let f = one("convention collective IDCC 1486", &["IDCC"]);
        assert_eq!(f.text, "convention collective IDCC 1486");
        let want =
            Want { kind: Some(ConventionKind::Convention), idcc: Some(1486), ..Want::default() };
        assert_eq!(f.value, convention(want));
        let g = one("CCN IDCC 1486", &["IDCC"]);
        assert_eq!(g.text, "CCN IDCC 1486");
        assert_eq!(
            g.value,
            convention(Want {
                kind: Some(ConventionKind::Ccn),
                idcc: Some(1486),
                ..Want::default()
            })
        );
        let h = one("convention collective IDCC 1486, brochure n° 3018", &["IDCC"]);
        assert_eq!(h.text, "convention collective IDCC 1486, brochure n° 3018");
        let want = Want {
            kind: Some(ConventionKind::Convention),
            idcc: Some(1486),
            brochure: Some(3018),
            ..Want::default()
        };
        assert_eq!(h.value, convention(want));
        let i = one("convention collective brochure n° 3018", &["brochure"]);
        let want = Want {
            kind: Some(ConventionKind::Convention),
            brochure: Some(3018),
            ..Want::default()
        };
        assert_eq!(i.value, convention(want));
    }

    #[test]
    fn l1_05_a_title_is_kept_when_the_identifiers_follow_it() {
        let input = "convention collective nationale des bureaux d'études techniques IDCC 1486";
        let f = one(input, &["IDCC"]);
        assert_eq!(f.text, input);
        let want = Want {
            kind: Some(ConventionKind::Ccn),
            idcc: Some(1486),
            title: Some("des bureaux d'études techniques"),
            ..Want::default()
        };
        assert_eq!(f.value, convention(want));
        // Titre, date, puis identifiants nus.
        let g = one("convention collective de l'édition du 14 janvier 2000 IDCC 2121", &["IDCC"]);
        let want = Want {
            kind: Some(ConventionKind::Convention),
            idcc: Some(2121),
            title: Some("de l'édition"),
            date: Some((2000, 1, 14)),
            ..Want::default()
        };
        assert_eq!(g.value, convention(want));
    }

    #[test]
    fn l1_05_neighbours_are_unchanged() {
        // Une parenthèse reste prioritaire (p182, p184).
        let f = one("CCN Syntec (IDCC 1486)", &["IDCC"]);
        let want = Want {
            kind: Some(ConventionKind::Ccn),
            idcc: Some(1486),
            title: Some("Syntec"),
            ..Want::default()
        };
        assert_eq!(f.value, convention(want));
        // Un IDCC mal formé ou un mot « IDCC » sans nombre reste dans le titre.
        let g = one("convention collective de l'IDCC en vigueur", &["convention"]);
        let want = Want {
            kind: Some(ConventionKind::Convention),
            title: Some("de l'IDCC en vigueur"),
            ..Want::default()
        };
        assert_eq!(g.value, convention(want));
        let h = one("convention collective IDCC 12345", &["onvention"]);
        let want = Want {
            kind: Some(ConventionKind::Convention),
            title: Some("IDCC 12345"),
            ..Want::default()
        };
        assert_eq!(h.value, convention(want));
    }

    #[test]
    fn l1_05_the_sentence_gives_a_single_citation() {
        let input = "Selon la convention collective IDCC 1486, les salariés bénéficient du préavis";
        let f = one(input, &["onvention"]);
        assert_eq!(f.text, "convention collective IDCC 1486");
    }

    #[test]
    fn only_family_and_sigle_anchors_are_read() {
        let input = "IDCC 275";
        assert_eq!(find(scan, input, AnchorClass::Court, &["IDCC"]), []);
        assert_eq!(find(scan, input, AnchorClass::Sigle, &["IDCC"]).len(), 1);
    }
}
