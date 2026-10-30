//! Grammaire des textes et des articles de textes (annexe B §4.3).
//!
//! Deux formes : `text_cite` (nature, numéro ou date, anaphore, début du titre) et
//! `text_article` (article d'un texte, avec sa date de version). Les plages des candidats sont
//! des offsets du texte d'ORIGINE (`Normalized::orig_range`), celles des ancres des offsets de
//! `Normalized::text`.
//!
//! - Le début du titre (`title_hint`) est capturé dans la valeur mais reste **hors du span** et
//!   ne réserve pas sa plage : il s'arrête avant une citation complète qu'il contient.
//! - Un texte cité à l'intérieur d'un article de texte n'est pas émis seul (l'article l'englobe) ;
//!   un texte cité par un qualificatif de version d'un article de code l'est (§4.15), parce que
//!   la grammaire des codes ne le réserve pas.
//! - « Article N de la Convention » donne un article sans texte et l'ambiguïté
//!   `convention_non_identifiee` (N27). Les articles des quatre textes du bloc de
//!   constitutionnalité sont lus par `constit`.
//! - L'appellation d'usage (« loi Macron ») n'est lue qu'après le mot « loi » seul : après une
//!   abréviation (`D.`, `Ord.`) ou une autre nature, un nom propre est trop souvent une partie
//!   à l'instance.

use crate::citation::normalize::{Normalized, fold_char};
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{
    Ambiguity, CitationRef, Confidence, Flag, TextAnaphora, TextCite, TextNature, TextNumber,
};

use super::Candidate;
use super::codes::{
    self, DateHit, Found, Hit, Tail, abbr, boundary, consume, fold_str, kw, left_boundary, longest,
    next_char, uint, word, ws, ws0,
};

/// Caractères au plus d'un début de titre.
const HINT_MAX_CHARS: usize = 300;

/// Un texte lu, avec ce que la grammaire en dit.
#[derive(Clone, Debug)]
pub(super) struct TextHit {
    /// Texte cité.
    pub cite: TextCite,
    /// Fin du span (le début du titre n'en fait pas partie).
    pub end: usize,
    /// Confiance : basse pour une appellation.
    pub confidence: Confidence,
    /// Ambiguïté : `appellation`.
    pub ambiguity: Option<Ambiguity>,
    /// Drapeaux de la date et du numéro.
    pub flags: Vec<Flag>,
}

// ---------------------------------------------------------------------------------------------
// Numéro, émetteur, anaphore, appellation
// ---------------------------------------------------------------------------------------------

/// `NUM_SIGN` : « n° », « numéro », « num. », « no » (suivi d'un chiffre), espaces comprises.
fn num_sign(text: &str, at: usize) -> Option<usize> {
    let end = kw(text, at, "n°")
        .or_else(|| word(text, at, "numero"))
        .or_else(|| abbr(text, at, "num"))
        .or_else(|| {
            kw(text, at, "no")
                .filter(|&end| next_char(text, ws0(text, end)).is_some_and(|c| c.is_ascii_digit()))
        })?;
    Some(ws0(text, end))
}

/// `text_num` : `2016-1321`, `93-1419` (1900 + 93), `2020-1234-5`, ou un entier seul.
fn text_num(text: &str, at: usize) -> Option<(TextNumber, usize)> {
    let (first, first_end) = uint(text, at)?;
    let year = match first_end - at {
        4 if (1500..=2099).contains(&first) => u16::try_from(first).ok(),
        2 => u16::try_from(first).ok().map(|short| 1900 + short),
        _ => None,
    };
    let dashed = year.and_then(|year| {
        let (seq, pos) = consume(text, first_end, '-').and_then(|p| uint(text, p))?;
        let (extra, end) = match consume(text, pos, '-').and_then(|p| uint(text, p)) {
            Some((extra, end)) => (Some(extra), end),
            None => (None, pos),
        };
        let raw = text.get(at..end).unwrap_or_default().to_owned();
        Some((TextNumber { year: Some(year), seq, extra, raw }, end))
    });
    let (number, end) = dashed.unwrap_or_else(|| {
        let raw = text.get(at..first_end).unwrap_or_default().to_owned();
        (TextNumber { year: None, seq: first, extra: None, raw }, first_end)
    });
    // Ni lettre collée, ni tiret qui annonce un numéro tronqué.
    (boundary(text, end) && next_char(text, end) != Some('-')).then_some((number, end))
}

/// Émetteurs d'un texte : forme repliée et forme rendue.
const EMITTERS: [(&str, &str); 5] = [
    ("ministeriel", "ministériel"),
    ("interministeriel", "interministériel"),
    ("prefectoral", "préfectoral"),
    ("du conseil d'etat", "du Conseil d'État"),
    ("du roi", "du Roi"),
];

fn emitter_at(text: &str, at: usize) -> Option<(&'static str, usize)> {
    EMITTERS
        .iter()
        .filter_map(|&(lit, shown)| word(text, at, lit).map(|end| (shown, end)))
        .max_by_key(|&(_, end)| end)
}

/// `stem` suivi de « e » et « s » facultatifs, puis d'une frontière de mot.
fn ending(text: &str, at: usize, stem: &str) -> Option<usize> {
    let mut pos = kw(text, at, stem)?;
    for letter in ['e', 's'] {
        if next_char(text, pos).map(fold_char) == Some(letter) {
            pos += 1;
        }
    }
    boundary(text, pos).then_some(pos)
}

/// `text_anaphora` : « susvisée », « précité », « mentionnée ci-dessus », « ci-dessus
/// mentionné ».
fn text_anaphora(text: &str, at: usize) -> Option<(TextAnaphora, usize)> {
    ending(text, at, "susvise")
        .map(|end| (TextAnaphora::Susvisee, end))
        .or_else(|| ending(text, at, "precite").map(|end| (TextAnaphora::Precitee, end)))
        .or_else(|| {
            ending(text, at, "mentionne")
                .and_then(|after| ws(text, after))
                .and_then(|pos| word(text, pos, "ci-dessus"))
                .map(|end| (TextAnaphora::MentionneeCiDessus, end))
        })
        .or_else(|| {
            kw(text, at, "ci-dessus")
                .and_then(|after| ws(text, after))
                .and_then(|pos| ending(text, pos, "mentionne"))
                .map(|end| (TextAnaphora::MentionneeCiDessus, end))
        })
}

/// Un mot capitalisé : `/[A-Z][a-zà-ÿ'-]+/`, avec au moins une minuscule.
fn capitalized(text: &str, at: usize) -> Option<usize> {
    let rest = text.get(at..).unwrap_or_default();
    let first = rest.chars().next().filter(|c| c.is_uppercase())?;
    let tail: usize = rest[first.len_utf8()..]
        .chars()
        .take_while(|c| c.is_lowercase() || matches!(c, '\'' | '-'))
        .map(char::len_utf8)
        .sum();
    let end = at + first.len_utf8() + tail;
    let has_lowercase = text.get(at..end).unwrap_or_default().chars().any(char::is_lowercase);
    (has_lowercase && boundary(text, end)).then_some(end)
}

/// `appellation` : un à trois mots capitalisés (« Macron », « Le Chapelier »).
fn appellation_at(text: &str, at: usize) -> Option<(String, usize)> {
    let (mut end, mut pos) = (at, at);
    for _ in 0..3 {
        let Some(word_end) = capitalized(text, pos) else { break };
        end = word_end;
        match ws(text, end) {
            Some(next) => pos = next,
            None => break,
        }
    }
    (end > at).then(|| (text.get(at..end).unwrap_or_default().to_owned(), end))
}

// ---------------------------------------------------------------------------------------------
// Natures
// ---------------------------------------------------------------------------------------------

/// Une nature lue.
struct NatureHit {
    nature: TextNature,
    end: usize,
    /// `L.` et `D.` ne désignent un texte que suivis d'un numéro (règle 1 du §4.2).
    needs_number: bool,
    /// Le mot « loi » seul : seule nature qui admet une appellation.
    bare_loi: bool,
}

/// Natures écrites en toutes lettres.
const WORD_NATURES: [(&str, TextNature); 9] = [
    ("ordonnance", TextNature::Ordonnance),
    ("decret-loi", TextNature::DecretLoi),
    ("decret", TextNature::Decret),
    ("arrete", TextNature::Arrete),
    ("reglement", TextNature::Reglement),
    ("circulaire", TextNature::Circulaire),
    ("instruction", TextNature::Instruction),
    ("deliberation", TextNature::Deliberation),
    ("decision", TextNature::Decision),
];

/// Natures abrégées, point final facultatif.
const ABBREVIATED_NATURES: [(&str, TextNature); 3] =
    [("ord", TextNature::Ordonnance), ("decr", TextNature::Decret), ("arr", TextNature::Arrete)];

/// `[ WS "pour" WS YEAR4 ]` : la fin de la nature d'une loi de finances.
fn pour_year(text: &str, at: usize) -> usize {
    ws(text, at)
        .and_then(|pos| word(text, pos, "pour"))
        .and_then(|pos| ws(text, pos))
        .and_then(|pos| uint(text, pos))
        .filter(|&(year, end)| (1500..=2099).contains(&year) && boundary(text, end))
        .map_or(at, |(_, end)| end)
}

/// Ce qui qualifie « loi » : « organique », « de finances », « du pays »…
fn loi_variant(text: &str, at: usize) -> Option<(TextNature, usize)> {
    if let Some(end) = word(text, at, "organique") {
        return Some((TextNature::LoiOrganique, end));
    }
    if let Some(end) = word(text, at, "constitutionnelle") {
        return Some((TextNature::LoiConstitutionnelle, end));
    }
    if let Some(end) = word(text, at, "de finances") {
        let rectified =
            ws(text, end).and_then(|pos| word(text, pos, "rectificative")).unwrap_or(end);
        return Some((TextNature::LoiDeFinances, pour_year(text, rectified)));
    }
    if let Some(end) = word(text, at, "de financement de la securite sociale") {
        return Some((TextNature::LoiDeFinancementSecu, pour_year(text, end)));
    }
    word(text, at, "du pays").map(|end| (TextNature::LoiDuPays, end))
}

fn loi_at(text: &str, at: usize) -> Option<NatureHit> {
    let after = word(text, at, "loi")?;
    let variant = ws(text, after).and_then(|pos| loi_variant(text, pos));
    let (nature, end) = variant.unwrap_or((TextNature::Loi, after));
    Some(NatureHit { nature, end, needs_number: false, bare_loi: variant.is_none() })
}

/// `L.` et `D.`, sensibles à la casse.
fn capital_abbreviation(text: &str, at: usize) -> Option<NatureHit> {
    let rest = text.get(at..).unwrap_or_default();
    let nature = if rest.starts_with("L.") {
        TextNature::Loi
    } else if rest.starts_with("D.") {
        TextNature::Decret
    } else {
        return None;
    };
    Some(NatureHit { nature, end: at + 2, needs_number: true, bare_loi: false })
}

fn nature(text: &str, at: usize) -> Option<NatureHit> {
    let written = WORD_NATURES
        .iter()
        .filter_map(|&(lit, nature)| word(text, at, lit).map(|end| (nature, end)));
    let abbreviated = ABBREVIATED_NATURES
        .iter()
        .filter_map(|&(lit, nature)| abbr(text, at, lit).map(|end| (nature, end)));
    let longest_other = written.chain(abbreviated).max_by_key(|&(_, end)| end);
    loi_at(text, at)
        .or_else(|| {
            longest_other.map(|(nature, end)| NatureHit {
                nature,
                end,
                needs_number: false,
                bare_loi: false,
            })
        })
        .or_else(|| capital_abbreviation(text, at))
}

// ---------------------------------------------------------------------------------------------
// text_cite
// ---------------------------------------------------------------------------------------------

/// Ce qui suit la nature : émetteur, numéro, date.
struct Body {
    number: Option<TextNumber>,
    date: Option<DateHit>,
    emitter: Option<&'static str>,
    end: usize,
}

/// `[ WS emitter ] ( WS NUM_SIGN text_num [ WS DATE_LINK ] | WS DATE_LINK )` à partir de `at`.
fn body_after(
    text: &str,
    at: usize,
    emitter: Option<&'static str>,
    needs_number: bool,
) -> Option<Body> {
    let numbered = ws(text, at)
        .and_then(|pos| num_sign(text, pos))
        .and_then(|pos| text_num(text, pos))
        .map(|(number, end)| {
            let date = ws(text, end).and_then(|pos| codes::date_link(text, pos));
            Body { number: Some(number), end: date.as_ref().map_or(end, |d| d.end), date, emitter }
        });
    numbered.or_else(|| {
        let date = ws(text, at).and_then(|pos| codes::date_link(text, pos))?;
        (!needs_number).then_some(Body { number: None, end: date.end, date: Some(date), emitter })
    })
}

fn body(text: &str, nature: &NatureHit) -> Option<Body> {
    ws(text, nature.end)
        .and_then(|pos| emitter_at(text, pos))
        .and_then(|(emitter, after)| body_after(text, after, Some(emitter), nature.needs_number))
        .or_else(|| body_after(text, nature.end, None, nature.needs_number))
}

fn appellation_hit(text: &str, nature: &NatureHit) -> Option<TextHit> {
    if !nature.bare_loi {
        return None;
    }
    let (name, end) = ws(text, nature.end).and_then(|pos| appellation_at(text, pos))?;
    let cite = TextCite {
        nature: nature.nature,
        number: None,
        date: None,
        date_raw: None,
        title_hint: None,
        anaphora: None,
        appellation: Some(name),
        emitter: None,
    };
    Some(TextHit {
        cite,
        end,
        confidence: Confidence::Low,
        ambiguity: Some(Ambiguity::Appellation),
        flags: Vec::new(),
    })
}

/// Un texte sans début de titre.
fn text_core(text: &str, at: usize) -> Option<TextHit> {
    let found = nature(text, at)?;
    let Some(body) = body(text, &found) else {
        return appellation_hit(text, &found);
    };
    let anaphora = ws(text, body.end).and_then(|pos| text_anaphora(text, pos));
    let end = anaphora.map_or(body.end, |(_, end)| end);
    let mut flags = body.date.as_ref().map_or_else(Vec::new, |d| d.flags.clone());
    let date = body.date.as_ref().and_then(|d| d.date);
    let number_year = body.number.as_ref().and_then(|n| n.year);
    if number_year.zip(date).is_some_and(|(year, date)| year != date.year()) {
        flags.push(Flag::AnneeNumeroDifferenteAnneeDate);
    }
    let cite = TextCite {
        nature: found.nature,
        number: body.number,
        date,
        date_raw: body.date.and_then(|d| d.raw),
        title_hint: None,
        anaphora: anaphora.map(|(anaphora, _)| anaphora),
        appellation: None,
        emitter: body.emitter.map(str::to_owned),
    };
    Some(TextHit { cite, end, confidence: Confidence::High, ambiguity: None, flags })
}

/// Débuts d'un titre de texte.
const TITLE_STARTS: [&str; 15] = [
    "portant",
    "relatif",
    "relative",
    "relatifs",
    "relatives",
    "pour",
    "sur",
    "de",
    "tendant",
    "visant",
    "modifiant",
    "pris",
    "prise",
    "transposant",
    "habilitant",
];

/// Une citation complète commence ici : un texte, ou un article suivi de son numéro.
fn embedded_citation(text: &str, at: usize) -> bool {
    text_core(text, at).is_some()
        || codes::art_kw(text, at).and_then(|pos| codes::art_item(text, pos)).is_some()
}

/// Retire les mots de liaison qui terminent un titre coupé avant une citation (« de l' »).
fn trim_connectors(mut hint: &str) -> &str {
    const CONNECTORS: [&str; 10] = ["de", "du", "des", "la", "le", "les", "l'", "d'", "a", "en"];
    while let Some((head, last)) = hint.rsplit_once(' ') {
        if !CONNECTORS.contains(&fold_str(last).as_str()) {
            break;
        }
        hint = head.trim_end();
    }
    hint
}

/// `title_hint` : `WS title_start` puis le texte jusqu'à « ; », « . », « ( », « [ », une
/// citation complète ou 300 caractères.
fn title_hint(text: &str, at: usize) -> Option<String> {
    let start = ws(text, at)?;
    (kw(text, start, "d'").is_some() || longest(text, start, &TITLE_STARTS).is_some())
        .then_some(())?;
    let mut limit = start;
    for (i, c) in text.get(start..).unwrap_or_default().char_indices().take(HINT_MAX_CHARS) {
        let here = start + i;
        let nested = i > 0 && c.is_alphanumeric() && left_boundary(text, here);
        if matches!(c, ';' | '.' | '(' | '[') || (nested && embedded_citation(text, here)) {
            break;
        }
        limit = here + c.len_utf8();
    }
    let hint = trim_connectors(text.get(start..limit).unwrap_or_default().trim_end());
    (!hint.is_empty()).then(|| hint.to_owned())
}

/// `text_cite` : un texte et le début de son titre. Sert à `text_article`, aux qualificatifs de
/// version de `codes` et à la grammaire des textes.
pub(super) fn text_cite(text: &str, at: usize) -> Option<TextHit> {
    let mut hit = text_core(text, at)?;
    if hit.cite.appellation.is_none() {
        hit.cite.title_hint = title_hint(text, hit.end);
    }
    Some(hit)
}

// ---------------------------------------------------------------------------------------------
// Candidats
// ---------------------------------------------------------------------------------------------

/// `text_article` : `[sub_lead] art_kw art_list [ WS "au" WS DATE ] WS link text_target`.
fn text_article_at(text: &str, start: usize) -> Option<Hit> {
    let (lead, pos) = codes::sub_lead(text, start).unwrap_or_else(|| (Vec::new(), start));
    let list = codes::art_list(text, codes::art_kw(text, pos)?)?;
    let mut pos = ws(text, list.end)?;
    let dated = word(text, pos, "au")
        .and_then(|end| ws(text, end))
        .and_then(|date_start| codes::date(text, date_start));
    let at_date = dated.as_ref().and_then(|d| d.date);
    let mut flags = dated.as_ref().map_or_else(Vec::new, |d| d.flags.clone());
    if let Some(dated) = &dated {
        pos = ws(text, dated.end)?;
    }
    let target = codes::link(text, pos, true)?;
    let (cite, end, confidence, ambiguity) = if let Some(hit) = text_cite(text, target) {
        flags.extend(hit.flags);
        (Some(hit.cite), hit.end, hit.confidence, hit.ambiguity)
    } else {
        let end = word(text, target, "convention")?;
        (None, end, Confidence::High, Some(Ambiguity::ConventionNonIdentifiee))
    };
    let tail = Tail { span_end: end, group_end: end, confidence, ambiguity, flags, version: None };
    let make = |article, _| CitationRef::TextArticle { text: cite.clone(), article, at_date };
    Some(Hit { end, found: codes::article_founds(start, &lead, &list, &tail, &make) })
}

/// Un texte seul.
fn text_only_at(text: &str, start: usize) -> Option<Hit> {
    let hit = text_cite(text, start)?;
    let found = Found {
        range: start..hit.end,
        value: CitationRef::Text(hit.cite),
        confidence: hit.confidence,
        ambiguity: hit.ambiguity,
        flags: hit.flags,
        group: None,
    };
    Some(Hit { end: hit.end, found: vec![found] })
}

fn text_at(text: &str, start: usize) -> Option<Hit> {
    text_article_at(text, start).or_else(|| text_only_at(text, start))
}

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    codes::drive(n, anchors, &[AnchorClass::TextNature, AnchorClass::ArticleKeyword], text_at)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use pretty_assertions::assert_eq;

    use super::super::codes::test_support::{Row, anchors, check_rows, foreign, norm, tail};
    use super::*;
    use crate::citation::scan::AnchorClass::{self, ArticleKeyword, TextNature};

    const LOI: (AnchorClass, &str) = (TextNature, "loi");
    const ORD: (AnchorClass, &str) = (TextNature, "ordonnance");
    const DECRET: (AnchorClass, &str) = (TextNature, "décret");
    const ARRETE: (AnchorClass, &str) = (TextNature, "arrêté");
    const ART: (AnchorClass, &str) = (ArticleKeyword, "article");

    fn cite_label(t: &TextCite) -> String {
        let number = t.number.as_ref().map_or("-", |n| n.raw.as_str());
        let date = t.date.map_or_else(|| "-".to_owned(), |d| d.to_string());
        let mut s = format!("{} {number} {date}", t.nature.code());
        if let Some(raw) = &t.date_raw {
            let _ = write!(s, " raw={raw}");
        }
        if let Some(emitter) = &t.emitter {
            let _ = write!(s, " emit={emitter}");
        }
        if let Some(anaphora) = t.anaphora {
            let _ = write!(s, " anaphora={anaphora:?}");
        }
        if let Some(appellation) = &t.appellation {
            let _ = write!(s, " app={appellation}");
        }
        if let Some(hint) = &t.title_hint {
            let _ = write!(s, " hint={hint:?}");
        }
        s
    }

    fn show(c: &Candidate) -> String {
        let head = match &c.value {
            CitationRef::Text(t) => format!("Text {}", cite_label(t)),
            CitationRef::TextArticle { text, article, at_date } => {
                let mut s = format!("Art {}", article.num.num_key());
                if !article.sub.is_empty() {
                    let _ = write!(s, " sub={:?}", article.sub);
                }
                if let Some(to) = &article.range_to {
                    let _ = write!(s, " to={}", to.num_key());
                }
                if let Some(d) = at_date {
                    let _ = write!(s, " at={d}");
                }
                s + &format!(" of {}", text.as_ref().map_or_else(|| "none".to_owned(), cite_label))
            }
            other => format!("autre : {other:?}"),
        };
        head + &tail(c)
    }

    fn check(rows: &[Row<'_>]) {
        check_rows(scan, show, rows);
    }

    #[test]
    fn texts_with_a_number_and_a_date_p050_to_p057() {
        check(&[
            (
                "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)",
                &[LOI],
                &[(
                    "LOI n° 2016-1321 du 7 octobre 2016",
                    "Text LOI 2016-1321 2016-10-07 hint=\"pour une République numérique\"",
                )],
            ),
            (
                "Décret n° 2016-1849 du 23 décembre 2016 relatif à la carte mobilité inclusion",
                &[DECRET],
                &[(
                    "Décret n° 2016-1849 du 23 décembre 2016",
                    "Text DECRET 2016-1849 2016-12-23 hint=\"relatif à la carte mobilité inclusion\"",
                )],
            ),
            (
                "la loi n° 98-389 du 19 mai 1998 transposant cette directive",
                &[LOI],
                &[(
                    "loi n° 98-389 du 19 mai 1998",
                    "Text LOI 98-389 1998-05-19 hint=\"transposant cette directive\"",
                )],
            ),
            (
                "- l\u{2019}ordonnance n° 58-1067 du 7 novembre 1958 portant loi organique sur le Conseil constitutionnel ;",
                &[ORD, LOI],
                &[(
                    "ordonnance n° 58-1067 du 7 novembre 1958",
                    "Text ORDONNANCE 58-1067 1958-11-07 hint=\"portant loi organique sur le Conseil constitutionnel\"",
                )],
            ),
            (
                "- la loi n° 2019-222 du 23 mars 2019 de programmation 2018-2022 et de réforme pour la justice ;",
                &[LOI],
                &[(
                    "loi n° 2019-222 du 23 mars 2019",
                    "Text LOI 2019-222 2019-03-23 hint=\"de programmation 2018-2022 et de réforme pour la justice\"",
                )],
            ),
            (
                "- la loi n° 2023-1059 du 20 novembre 2023 d\u{2019}orientation et de programmation du ministère de la justice 2023-2027 ;",
                &[LOI],
                &[(
                    "loi n° 2023-1059 du 20 novembre 2023",
                    "Text LOI 2023-1059 2023-11-20 hint=\"d'orientation et de programmation du ministère de la justice 2023-2027\"",
                )],
            ),
        ]);
    }

    #[test]
    fn texts_with_a_date_only_or_an_anaphora_p058_to_p064() {
        check(&[
            (
                "dans sa rédaction résultant de la loi du 20 novembre 2023 mentionnée ci-dessus",
                &[LOI],
                &[(
                    "loi du 20 novembre 2023 mentionnée ci-dessus",
                    "Text LOI - 2023-11-20 anaphora=MentionneeCiDessus",
                )],
            ),
            (
                "- le règlement du 4 février 2010 sur la procédure suivie devant le Conseil constitutionnel pour les questions prioritaires de constitutionnalité ;",
                &[(TextNature, "règlement")],
                &[(
                    "règlement du 4 février 2010",
                    "Text REGLEMENT - 2010-02-04 hint=\"sur la procédure suivie devant le Conseil constitutionnel pour les questions prioritaires de constitutionnalité\"",
                )],
            ),
            (
                "la loi n°2019-290 en vigueur à la date d'aujourd'hui",
                &[LOI],
                &[("loi n°2019-290", "Text LOI 2019-290 -")],
            ),
            (
                "l'arrêté du 12 février 2024 relatif aux modalités",
                &[ARRETE],
                &[(
                    "arrêté du 12 février 2024",
                    "Text ARRETE - 2024-02-12 hint=\"relatif aux modalités\"",
                )],
            ),
            (
                "le décret-loi du 30 octobre 1935",
                &[DECRET],
                &[("décret-loi du 30 octobre 1935", "Text DECRET_LOI - 1935-10-30")],
            ),
            (
                "la loi du 29 juillet 1881 sur la liberté de la presse",
                &[LOI],
                &[(
                    "loi du 29 juillet 1881",
                    "Text LOI - 1881-07-29 hint=\"sur la liberté de la presse\"",
                )],
            ),
            (
                "loi n° 78-17 du 6 janvier 1978 relative à l'informatique, aux fichiers et aux libertés",
                &[LOI],
                &[(
                    "loi n° 78-17 du 6 janvier 1978",
                    "Text LOI 78-17 1978-01-06 hint=\"relative à l'informatique, aux fichiers et aux libertés\"",
                )],
            ),
        ]);
    }

    #[test]
    fn natures_and_dates_p065_to_p078() {
        check(&[
            (
                "loi organique n° 2009-1523 du 10 décembre 2009",
                &[LOI],
                &[(
                    "loi organique n° 2009-1523 du 10 décembre 2009",
                    "Text LOI_ORGANIQUE 2009-1523 2009-12-10",
                )],
            ),
            (
                "la loi constitutionnelle n° 2008-724 du 23 juillet 2008",
                &[LOI],
                &[(
                    "loi constitutionnelle n° 2008-724 du 23 juillet 2008",
                    "Text LOI_CONSTIT 2008-724 2008-07-23",
                )],
            ),
            (
                "le décret n° 2019-1333 du 11 décembre 2019",
                &[DECRET],
                &[("décret n° 2019-1333 du 11 décembre 2019", "Text DECRET 2019-1333 2019-12-11")],
            ),
            (
                "D. n° 2019-1333 du 11 déc. 2019",
                &[(TextNature, "d. n")],
                &[("D. n° 2019-1333 du 11 déc. 2019", "Text DECRET 2019-1333 2019-12-11")],
            ),
            (
                "L. n° 2016-1321 du 7 oct. 2016",
                &[(TextNature, "l. n")],
                &[("L. n° 2016-1321 du 7 oct. 2016", "Text LOI 2016-1321 2016-10-07")],
            ),
            (
                "Ord. n° 2016-131 du 10 févr. 2016",
                &[(TextNature, "ord.")],
                &[("Ord. n° 2016-131 du 10 févr. 2016", "Text ORDONNANCE 2016-131 2016-02-10")],
            ),
            (
                "loi no 2016-1321 du 7 octobre 2016",
                &[LOI],
                &[("loi no 2016-1321 du 7 octobre 2016", "Text LOI 2016-1321 2016-10-07")],
            ),
            (
                "loi n°2016\u{2011}1321 du 07/10/2016",
                &[LOI],
                &[("loi n°2016\u{2011}1321 du 07/10/2016", "Text LOI 2016-1321 2016-10-07")],
            ),
            (
                "décret nº\u{202f}2016-1849 du 23\u{a0}décembre 2016",
                &[DECRET],
                &[(
                    "décret nº\u{202f}2016-1849 du 23\u{a0}décembre 2016",
                    "Text DECRET 2016-1849 2016-12-23",
                )],
            ),
        ]);
    }

    #[test]
    fn number_signs_and_the_other_natures_p065_to_p078() {
        check(&[
            ("loi numéro 2016-1321", &[LOI], &[("loi numéro 2016-1321", "Text LOI 2016-1321 -")]),
            ("loi num. 2016-1321", &[LOI], &[("loi num. 2016-1321", "Text LOI 2016-1321 -")]),
            (
                "loi n° 12 du 3 mars 1950",
                &[LOI],
                &[("loi n° 12 du 3 mars 1950", "Text LOI 12 1950-03-03")],
            ),
            (
                "loi n° 2020-1234-5 du 3 mars 2020",
                &[LOI],
                &[("loi n° 2020-1234-5 du 3 mars 2020", "Text LOI 2020-1234-5 2020-03-03")],
            ),
            (
                "loi de finances pour 2020 n° 2019-1479 du 28 décembre 2019",
                &[LOI],
                &[(
                    "loi de finances pour 2020 n° 2019-1479 du 28 décembre 2019",
                    "Text LOI_FINANCES 2019-1479 2019-12-28",
                )],
            ),
            (
                "loi de finances rectificative pour 2020 n° 2020-935 du 30 juillet 2020",
                &[LOI],
                &[(
                    "loi de finances rectificative pour 2020 n° 2020-935 du 30 juillet 2020",
                    "Text LOI_FINANCES 2020-935 2020-07-30",
                )],
            ),
            (
                "loi de finances n° 2019-1479 du 28 décembre 2019",
                &[LOI],
                &[(
                    "loi de finances n° 2019-1479 du 28 décembre 2019",
                    "Text LOI_FINANCES 2019-1479 2019-12-28",
                )],
            ),
            (
                "loi de financement de la sécurité sociale pour 2021 n° 2020-1576 du 14 décembre 2020",
                &[LOI],
                &[(
                    "loi de financement de la sécurité sociale pour 2021 n° 2020-1576 du 14 décembre 2020",
                    "Text LOI_FSS 2020-1576 2020-12-14",
                )],
            ),
            (
                "loi du pays n° 2020-12 du 3 mars 2020",
                &[LOI],
                &[("loi du pays n° 2020-12 du 3 mars 2020", "Text LOI_DU_PAYS 2020-12 2020-03-03")],
            ),
            (
                "circulaire n° 5 du 3 mars 2020",
                &[(TextNature, "circulaire")],
                &[("circulaire n° 5 du 3 mars 2020", "Text CIRCULAIRE 5 2020-03-03")],
            ),
            (
                "instruction n° 12",
                &[(TextNature, "instruction")],
                &[("instruction n° 12", "Text INSTRUCTION 12 -")],
            ),
            (
                "délibération n° 12 du 3 mars 2020",
                &[(TextNature, "délibération")],
                &[("délibération n° 12 du 3 mars 2020", "Text DELIBERATION 12 2020-03-03")],
            ),
            (
                "décision du 3 mars 2020",
                &[(TextNature, "décision")],
                &[("décision du 3 mars 2020", "Text DECISION - 2020-03-03")],
            ),
            (
                "arr. du 3 mars 2020",
                &[(TextNature, "arr.")],
                &[("arr. du 3 mars 2020", "Text ARRETE - 2020-03-03")],
            ),
            (
                "décr. n° 2020-1",
                &[(TextNature, "décr.")],
                &[("décr. n° 2020-1", "Text DECRET 2020-1 -")],
            ),
        ]);
    }

    #[test]
    fn emitters_and_anaphoras() {
        check(&[
            (
                "arrêté ministériel du 12 mai 2020",
                &[ARRETE],
                &[(
                    "arrêté ministériel du 12 mai 2020",
                    "Text ARRETE - 2020-05-12 emit=ministériel",
                )],
            ),
            (
                "arrêté interministériel n° 5 du 12 mai 2020",
                &[ARRETE],
                &[(
                    "arrêté interministériel n° 5 du 12 mai 2020",
                    "Text ARRETE 5 2020-05-12 emit=interministériel",
                )],
            ),
            (
                "arrêté préfectoral n° 2020-12 du 3 mars 2020",
                &[ARRETE],
                &[(
                    "arrêté préfectoral n° 2020-12 du 3 mars 2020",
                    "Text ARRETE 2020-12 2020-03-03 emit=préfectoral",
                )],
            ),
            (
                "décret du Conseil d'État du 5 mai 1990",
                &[DECRET],
                &[(
                    "décret du Conseil d'État du 5 mai 1990",
                    "Text DECRET - 1990-05-05 emit=du Conseil d'État",
                )],
            ),
            (
                "ordonnance du Roi n° 5",
                &[ORD],
                &[("ordonnance du Roi n° 5", "Text ORDONNANCE 5 - emit=du Roi")],
            ),
            (
                "l'ordonnance du 7 novembre 1958 susvisée",
                &[ORD],
                &[(
                    "ordonnance du 7 novembre 1958 susvisée",
                    "Text ORDONNANCE - 1958-11-07 anaphora=Susvisee",
                )],
            ),
            (
                "la loi n° 78-17 du 6 janvier 1978 précitée",
                &[LOI],
                &[(
                    "loi n° 78-17 du 6 janvier 1978 précitée",
                    "Text LOI 78-17 1978-01-06 anaphora=Precitee",
                )],
            ),
            ("les lois susvisées n° 78-17 ci-dessus mentionnées", &[LOI], &[]),
            (
                "le décret n° 2020-1 ci-dessus mentionné",
                &[DECRET],
                &[(
                    "décret n° 2020-1 ci-dessus mentionné",
                    "Text DECRET 2020-1 - anaphora=MentionneeCiDessus",
                )],
            ),
            (
                "le décret n° 2020-1 mentionné ci-dessus",
                &[DECRET],
                &[(
                    "décret n° 2020-1 mentionné ci-dessus",
                    "Text DECRET 2020-1 - anaphora=MentionneeCiDessus",
                )],
            ),
        ]);
    }

    #[test]
    fn suspicious_dates_and_numbers_n07() {
        check(&[
            (
                "loi n° 2016-1321 du 7 octobre 2015",
                &[LOI],
                &[(
                    "loi n° 2016-1321 du 7 octobre 2015",
                    "Text LOI 2016-1321 2015-10-07 flags=[\"annee_numero_differente_annee_date\"]",
                )],
            ),
            (
                "loi n° 93-1419 du 31 décembre 1993",
                &[LOI],
                &[("loi n° 93-1419 du 31 décembre 1993", "Text LOI 93-1419 1993-12-31")],
            ),
            (
                "loi n° 2016-1321 du 31 février 2016",
                &[LOI],
                &[(
                    "loi n° 2016-1321 du 31 février 2016",
                    "Text LOI 2016-1321 - flags=[\"date_invalide\"]",
                )],
            ),
            (
                "loi du 4 messidor an II",
                &[LOI],
                &[(
                    "loi du 4 messidor an II",
                    "Text LOI - - raw=4 messidor an II flags=[\"date_republicaine\"]",
                )],
            ),
        ]);
    }

    #[test]
    fn appellations_are_low_confidence_n35() {
        check(&[
            (
                "la loi Macron",
                &[LOI],
                &[("loi Macron", "Text LOI - - app=Macron conf=Low amb=appellation")],
            ),
            (
                "selon la loi Le Chapelier, les",
                &[LOI],
                &[("loi Le Chapelier", "Text LOI - - app=Le Chapelier conf=Low amb=appellation")],
            ),
            (
                "la loi Informatique et libertés",
                &[LOI],
                &[("loi Informatique", "Text LOI - - app=Informatique conf=Low amb=appellation")],
            ),
            (
                "la loi Aa Bb Cc Dd",
                &[LOI],
                &[("loi Aa Bb Cc", "Text LOI - - app=Aa Bb Cc conf=Low amb=appellation")],
            ),
            (
                "article 3 de la loi Macron",
                &[LOI, ART],
                &[(
                    "article 3 de la loi Macron",
                    "Art 3 of LOI - - app=Macron conf=Low amb=appellation",
                )],
            ),
        ]);
    }

    #[test]
    fn not_a_text() {
        check(&[
            ("pour la période 2016-1321", &[LOI], &[]),
            ("la loi", &[LOI], &[]),
            ("la loi n° ", &[LOI], &[]),
            ("la loi n° abc", &[LOI], &[]),
            ("la loi du 31", &[LOI], &[]),
            ("la loi du ", &[LOI], &[]),
            ("la loi n° 2016-", &[LOI], &[]),
            ("la loi n° 3000-12", &[LOI], &[]),
            ("la loi n° 2016-1321-", &[LOI], &[]),
            ("la loi no abc", &[LOI], &[]),
            ("nous avons voté la loi nous", &[LOI], &[]),
            ("la loi organique Macron", &[LOI], &[]),
            ("la loi macron", &[LOI], &[]),
            ("la loi MACRON", &[LOI], &[]),
            ("la loi M", &[LOI], &[]),
            ("la loi 'Macron", &[LOI], &[]),
            ("D. Dupont", &[(TextNature, "d. d")], &[]),
            ("L. 1234-5", &[(TextNature, "l. 1")], &[]),
            ("ord. Dupont", &[(TextNature, "ord.")], &[]),
            ("Décision 2025-1173", &[(TextNature, "décision")], &[]),
            ("", &[LOI], &[]),
        ]);
        let n = norm("la loi n° 2016-1321");
        assert_eq!(scan(&n, &[]), Vec::new());
        assert_eq!(scan(&n, &anchors(&n, &[(AnchorClass::Court, "loi")])), Vec::new());
    }

    #[test]
    fn title_hints_stop_before_embedded_citations() {
        check(&[
            (
                "décret n° 2016-1849 du 23 décembre 2016 relatif à la carte mobilité inclusion pris en application de l'article 107 de la loi n° 2016-1321 du 7 octobre 2016 pour une République numérique",
                &[DECRET, ART, LOI],
                &[
                    (
                        "décret n° 2016-1849 du 23 décembre 2016",
                        "Text DECRET 2016-1849 2016-12-23 hint=\"relatif à la carte mobilité inclusion pris en application\"",
                    ),
                    (
                        "article 107 de la loi n° 2016-1321 du 7 octobre 2016",
                        "Art 107 of LOI 2016-1321 2016-10-07 hint=\"pour une République numérique\"",
                    ),
                ],
            ),
            (
                "loi du 3 mars 2020 portant diverses mesures ; autres",
                &[LOI],
                &[(
                    "loi du 3 mars 2020",
                    "Text LOI - 2020-03-03 hint=\"portant diverses mesures\"",
                )],
            ),
            (
                "loi du 3 mars 2020 portant mesures [1] autres",
                &[LOI],
                &[("loi du 3 mars 2020", "Text LOI - 2020-03-03 hint=\"portant mesures\"")],
            ),
            (
                "loi du 3 mars 2020 portant mesures. Autres",
                &[LOI],
                &[("loi du 3 mars 2020", "Text LOI - 2020-03-03 hint=\"portant mesures\"")],
            ),
            (
                "loi du 3 mars 2020 portant",
                &[LOI],
                &[("loi du 3 mars 2020", "Text LOI - 2020-03-03 hint=\"portant\"")],
            ),
            (
                "loi du 3 mars 2020 autre chose",
                &[LOI],
                &[("loi du 3 mars 2020", "Text LOI - 2020-03-03")],
            ),
            ("loi du 3 mars 2020 ", &[LOI], &[("loi du 3 mars 2020", "Text LOI - 2020-03-03")]),
        ]);
        // Trois cents caractères au plus.
        let long = format!("loi du 3 mars 2020 portant {}", "x ".repeat(300));
        let n = norm(&long);
        let found = scan(&n, &anchors(&n, &[LOI]));
        let hint = format!("portant {}x", "x ".repeat(145));
        assert_eq!(hint.chars().count(), 299);
        assert_eq!(found.len(), 1);
        assert_eq!(show(&found[0]), format!("Text LOI - 2020-03-03 hint={hint:?}"));
    }

    #[test]
    fn other_families_are_reported_as_such() {
        assert!(show(&foreign()).starts_with("autre : Unsupported"));
    }

    #[test]
    fn articles_without_a_text_to_follow() {
        check(&[
            ("article de la loi du 3 mars 2020", &[ART], &[]),
            ("article 5", &[ART], &[]),
            ("article 5 au 1er janvier 2018", &[ART], &[]),
            ("article 5 au 1er janvier 2018 sans suite", &[ART], &[]),
            ("article 5 de la Constitution", &[ART], &[]),
        ]);
    }

    #[test]
    fn articles_of_texts_p052_p053_p059_p061_p062_p064() {
        check(&[
            (
                "pris en application de l'article 107 de la loi n° 2016-1321 du 7 octobre 2016 pour une République numérique",
                &[ART, LOI],
                &[(
                    "article 107 de la loi n° 2016-1321 du 7 octobre 2016",
                    "Art 107 of LOI 2016-1321 2016-10-07 hint=\"pour une République numérique\"",
                )],
            ),
            (
                "et en application de l'article 2 de la loi n° 93-1419 du 31 décembre 1993 relative à l'Imprimerie nationale",
                &[ART, LOI],
                &[(
                    "article 2 de la loi n° 93-1419 du 31 décembre 1993",
                    "Art 2 of LOI 93-1419 1993-12-31 hint=\"relative à l'Imprimerie nationale\"",
                )],
            ),
            (
                "dans les conditions prévues à l\u{2019}article 23-11 de l\u{2019}ordonnance du 7 novembre 1958 susvisée.",
                &[ART, ORD],
                &[(
                    "article 23-11 de l\u{2019}ordonnance du 7 novembre 1958 susvisée",
                    "Art 23-11 of ORDONNANCE - 1958-11-07 anaphora=Susvisee",
                )],
            ),
            (
                "l\u{2019}article 6 nonies au 1er janvier 2018 de l\u{2019}ordonnance n°58-1100.",
                &[ART, ORD],
                &[(
                    "article 6 nonies au 1er janvier 2018 de l\u{2019}ordonnance n°58-1100",
                    "Art 6 nonies at=2018-01-01 of ORDONNANCE 58-1100 -",
                )],
            ),
            (
                "l\u{2019}article 3-1 au 1er janvier 2018 de la loi n° 86-1067",
                &[ART, LOI],
                &[(
                    "article 3-1 au 1er janvier 2018 de la loi n° 86-1067",
                    "Art 3-1 at=2018-01-01 of LOI 86-1067 -",
                )],
            ),
            (
                "article 57 de la loi du 17 juillet 1978 en vigueur aujourd'hui",
                &[ART, LOI],
                &[("article 57 de la loi du 17 juillet 1978", "Art 57 of LOI - 1978-07-17")],
            ),
            (
                "articles 2 et 3 de la loi n° 78-17 du 6 janvier 1978",
                &[ART, LOI],
                &[
                    ("2", "Art 2 of LOI 78-17 1978-01-06 grp"),
                    ("3", "Art 3 of LOI 78-17 1978-01-06 grp"),
                ],
            ),
            (
                "le premier alinéa de l'article 5 de la loi n° 78-17 du 6 janvier 1978",
                &[ART, LOI],
                &[(
                    "premier alinéa de l'article 5 de la loi n° 78-17 du 6 janvier 1978",
                    "Art 5 sub=[Alinea(1)] of LOI 78-17 1978-01-06",
                )],
            ),
            (
                "art. 5 de loi n° 78-17",
                &[(ArticleKeyword, "art."), LOI],
                &[("art. 5 de loi n° 78-17", "Art 5 of LOI 78-17 -")],
            ),
            (
                "article 5 au 31 février 2018 de la loi n° 86-1067",
                &[ART, LOI],
                &[(
                    "article 5 au 31 février 2018 de la loi n° 86-1067",
                    "Art 5 of LOI 86-1067 - flags=[\"date_invalide\"]",
                )],
            ),
            (
                "articles 1100 à 1101 de la loi n° 78-17",
                &[ART, LOI],
                &[("articles 1100 à 1101 de la loi n° 78-17", "Art 1100 to=1101 of LOI 78-17 -")],
            ),
        ]);
    }

    #[test]
    fn the_convention_alone_is_an_unidentified_text_n27() {
        check(&[
            (
                "l'article 6 de la Convention",
                &[ART],
                &[("article 6 de la Convention", "Art 6 of none amb=convention_non_identifiee")],
            ),
            (
                "les articles 6 et 8 de la Convention.",
                &[ART],
                &[
                    ("6", "Art 6 of none amb=convention_non_identifiee grp"),
                    ("8", "Art 8 of none amb=convention_non_identifiee grp"),
                ],
            ),
            ("l'article 6 de la Conventionnelle", &[ART], &[]),
            ("l'article 6 de la loi", &[ART, LOI], &[]),
            ("l'article 6 du code civil", &[ART], &[]),
        ]);
    }

    #[test]
    fn a_version_text_is_found_on_its_own() {
        check(&[(
            "Il résulte des dispositions des articles 1244 et 1253 du code civil, dans leur rédaction antérieure à celle issue de l'ordonnance n° 2016-131 du 10 février 2016, que",
            &[ART, ORD],
            &[("ordonnance n° 2016-131 du 10 février 2016", "Text ORDONNANCE 2016-131 2016-02-10")],
        )]);
    }

    /// Le texte lu en tête de `text` : sa description et sa fin.
    fn cite_at(text: &str) -> Option<(String, usize)> {
        text_cite(text, 0).map(|hit| (cite_label(&hit.cite), hit.end))
    }

    #[test]
    fn a_text_cite_reports_its_span_without_the_title() {
        let text = "loi n° 78-17 du 6 janvier 1978 relative à l'informatique";
        let (label, end) = cite_at(text).expect("texte");
        assert_eq!(&text[..end], "loi n° 78-17 du 6 janvier 1978");
        assert_eq!(label, "LOI 78-17 1978-01-06 hint=\"relative à l'informatique\"");
        assert_eq!(cite_at("la loi"), None);
    }

    /// Année, séquence, complément, texte du numéro et fin.
    type Number = (Option<u16>, u32, Option<u32>, String, usize);

    fn number_of(text: &str) -> Option<Number> {
        text_num(text, 0).map(|(n, end)| (n.year, n.seq, n.extra, n.raw, end))
    }

    #[test]
    fn text_numbers_follow_annex_b_4_3() {
        assert_eq!(
            number_of("2016-1321 du"),
            Some((Some(2016), 1321, None, "2016-1321".to_owned(), 9))
        );
        assert_eq!(number_of("93-1419"), Some((Some(1993), 1419, None, "93-1419".to_owned(), 7)));
        assert_eq!(number_of("58-1100."), Some((Some(1958), 1100, None, "58-1100".to_owned(), 7)));
        assert_eq!(
            number_of("2020-1234-5 x"),
            Some((Some(2020), 1234, Some(5), "2020-1234-5".to_owned(), 11))
        );
        assert_eq!(number_of("506507,"), Some((None, 506_507, None, "506507".to_owned(), 6)));
        assert_eq!(number_of("12"), Some((None, 12, None, "12".to_owned(), 2)));
        for not_a_number in
            ["", "abc", "2016-", "2016-1321-", "3000-12", "123-45", "12a", "2016-1321a", "2016--5"]
        {
            assert_eq!(number_of(not_a_number), None, "{not_a_number}");
        }
    }

    #[test]
    fn number_signs() {
        for (text, end) in [
            ("n° 12", 4),
            ("n°12", 3),
            ("no 12", 3),
            ("numéro 12", "numéro ".len()),
            ("num. 12", 5),
            ("num 12", 4),
        ] {
            assert_eq!(num_sign(text, 0), Some(end), "{text}");
        }
        for not_a_sign in ["no abc", "nous 12", "n 12", "number 12", ""] {
            assert_eq!(num_sign(not_a_sign, 0), None, "{not_a_sign}");
        }
    }

    #[test]
    fn connectors_are_trimmed_from_the_end_of_a_hint() {
        assert_eq!(
            trim_connectors("relatif à la carte pris en application de l'"),
            "relatif à la carte pris en application"
        );
        assert_eq!(trim_connectors("relatif à la carte"), "relatif à la carte");
        assert_eq!(
            trim_connectors("pour une République numérique"),
            "pour une République numérique"
        );
        assert_eq!(trim_connectors("portant"), "portant");
        assert_eq!(trim_connectors("sur la"), "sur");
        assert_eq!(trim_connectors(""), "");
    }
}
