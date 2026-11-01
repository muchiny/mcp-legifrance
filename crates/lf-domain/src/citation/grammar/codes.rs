//! Grammaire des articles de codes (annexe B §4.2) et briques communes aux trois grammaires de
//! V2-10 (codes, textes, bloc de constitutionnalité).
//!
//! Chaque grammaire lit `Normalized::text` à la main : mots-clés comparés par `fold_char`
//! (casse et accents ignorés), jetons sensibles à la casse lus tels quels, aucune récursion
//! non bornée, aucune indexation hors des frontières de caractère. Les plages des candidats sont
//! des offsets du texte d'ORIGINE (`Normalized::orig_range`), celles des ancres des offsets de
//! `Normalized::text`.
//!
//! # Contrat avec l'arbitrage (`post::arbitrate`)
//!
//! - `CodeRef::Anaphora` est rendu tel quel quand le texte dit « du même code », « dudit
//!   code »… : l'arbitrage pose le code de l'antécédent et `code_via` (`code_via` vaut `None`
//!   en sortie de grammaire), ou `AnaphoreNonResolue`.
//! - Un article sans code est `CodeRef::Missing` avec `TexteManquant` (mot-clé d'article sans
//!   code ni texte) ou `CodeManquant` (numéro préfixé à tiret seul) : avec un contexte de
//!   document, l'arbitrage les remplace par `CodeImplicite` et `code_via = ContexteDocument`.
//! - Une énumération donne un candidat par numéro (plage = le numéro), un `enum_group` commun
//!   (offset d'origine du début du groupe) et un `group_extent` qui court du début du groupe à
//!   la fin de la référence de code ou de texte, qualificatif de version exclu.
//! - Les drapeaux qui dépendent de la date du jour (`AnneeFuture`, `AnneeImprobable`) sont à
//!   l'arbitrage : la grammaire ne rend que `DateInvalide`, `DateRepublicaine` et
//!   `AnneeNumeroDifferenteAnneeDate`.
//! - Le texte cité par un qualificatif de version (« dans sa rédaction issue de l'ordonnance… »)
//!   est contenu dans la plage du candidat `CodeArticle` (§4.15) : la grammaire des textes
//!   l'émet aussi comme candidat `Text` de valeur égale à celle du qualificatif, ce qui permet à
//!   l'arbitrage de reconnaître l'exception à la règle de non-chevauchement.

use std::ops::Range;
use std::sync::LazyLock;

use crate::citation::dict::codes::{CODES, CodeKey};
use crate::citation::normalize::{Normalized, fold_char};
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{
    Ambiguity, Anaphora, ArticleNum, ArticlePrefix, ArticleRef, CitationRef, CodeRef, Confidence,
    Flag, LatinAdverb, Sigle, SpecialNum, Subdivision, Suffixes, VersionQualifier,
};
use crate::date::CivilDate;

use super::{Candidate, textes};

/// Caractères relus avant une ancre (annexe B §4.16).
const WINDOW_BEFORE: usize = 80;
/// Caractères lus après une ancre.
const WINDOW_AFTER: usize = 400;
/// Caractères lus en plus pour ne pas couper un mot en deux à la fin de la fenêtre.
const WINDOW_SLACK: usize = 32;
/// Débuts de mot essayés avant une ancre.
const START_POSITIONS: usize = 12;
/// Mots d'un titre de code hors dictionnaire (`code_generic`).
const GENERIC_MAX_WORDS: usize = 8;

// ---------------------------------------------------------------------------------------------
// Briques lexicales (annexe B §4.0 et §4.1)
// ---------------------------------------------------------------------------------------------

/// Caractère de `text` à l'offset `at`.
pub(super) fn next_char(text: &str, at: usize) -> Option<char> {
    text.get(at..).and_then(|rest| rest.chars().next())
}

/// Caractère de `text` qui précède l'offset `at`.
pub(super) fn prev_char(text: &str, at: usize) -> Option<char> {
    text.get(..at).and_then(|head| head.chars().next_back())
}

/// Fin des espaces lues en `at` (zéro ou plus). Après la normalisation, il n'y a plus que des
/// U+0020.
pub(super) fn ws0(text: &str, at: usize) -> usize {
    at + text.get(at..).map_or(0, |rest| rest.len() - rest.trim_start_matches(' ').len())
}

/// Fin des espaces lues en `at` (une ou plus).
pub(super) fn ws(text: &str, at: usize) -> Option<usize> {
    let end = ws0(text, at);
    (end > at).then_some(end)
}

/// `B` de l'annexe B §4.0 : le caractère en `at` n'est ni une lettre ni un chiffre.
pub(super) fn boundary(text: &str, at: usize) -> bool {
    !next_char(text, at).is_some_and(char::is_alphanumeric)
}

/// Le caractère qui précède `at` n'est ni une lettre ni un chiffre.
pub(super) fn left_boundary(text: &str, at: usize) -> bool {
    !prev_char(text, at).is_some_and(char::is_alphanumeric)
}

/// Fin du littéral `lit` lu en `at`. Le littéral est écrit replié (minuscules, sans accent) ; une
/// espace du littéral vaut une ou plusieurs espaces du texte.
pub(super) fn kw(text: &str, at: usize, lit: &str) -> Option<usize> {
    let mut pos = at;
    for expected in lit.chars() {
        pos = if expected == ' ' {
            ws(text, pos)?
        } else {
            let c = next_char(text, pos).filter(|&c| fold_char(c) == expected)?;
            pos + c.len_utf8()
        };
    }
    Some(pos)
}

/// Comme [`kw`], suivi d'une frontière de mot.
pub(super) fn word(text: &str, at: usize, lit: &str) -> Option<usize> {
    kw(text, at, lit).filter(|&end| boundary(text, end))
}

/// Abréviation `stem` suivie de son point (inclus dans la lecture quand il est présent) ou d'une
/// frontière de mot (annexe B §4.0 : le point final d'une abréviation est optionnel).
pub(super) fn abbr(text: &str, at: usize, stem: &str) -> Option<usize> {
    let end = kw(text, at, stem)?;
    if next_char(text, end) == Some('.') {
        Some(end + 1)
    } else {
        boundary(text, end).then_some(end)
    }
}

/// Le plus long des littéraux `lits` lu en `at` comme mot : son rang et sa fin.
pub(super) fn longest(text: &str, at: usize, lits: &[&str]) -> Option<(usize, usize)> {
    lits.iter()
        .enumerate()
        .filter_map(|(i, lit)| word(text, at, lit).map(|end| (i, end)))
        .max_by_key(|&(_, end)| end)
}

/// Fin de `c` lu en `at`.
pub(super) fn consume(text: &str, at: usize, c: char) -> Option<usize> {
    (next_char(text, at) == Some(c)).then(|| at + c.len_utf8())
}

/// Entier décimal d'au plus neuf chiffres lu en `at` : sa valeur et sa fin.
#[expect(
    clippy::indexing_slicing,
    reason = "`digits` chiffres ASCII ont été comptés à partir de `at`"
)]
pub(super) fn uint(text: &str, at: usize) -> Option<(u32, usize)> {
    let digits =
        text.get(at..).map_or(0, |rest| rest.bytes().take_while(u8::is_ascii_digit).count());
    (1..=9).contains(&digits).then(|| {
        let value = text.as_bytes()[at..at + digits]
            .iter()
            .fold(0, |acc, byte| acc * 10 + u32::from(byte - b'0'));
        (value, at + digits)
    })
}

/// `SEP` : une ponctuation entre espaces, ou des espaces seuls.
pub(super) fn sep(text: &str, at: usize) -> Option<usize> {
    let pos = ws0(text, at);
    if matches!(next_char(text, pos), Some(',' | ';' | ':')) {
        Some(ws0(text, pos + 1))
    } else {
        ws(text, at)
    }
}

/// Liaison qui introduit un code ou un texte : « de l' », « de la », « des », « du », et « de »
/// seul si `bare_de`. Rend la fin, espaces suivantes comprises.
pub(super) fn link(text: &str, at: usize, bare_de: bool) -> Option<usize> {
    let words: &[&str] =
        if bare_de { &["de la", "des", "du", "de"] } else { &["de la", "des", "du"] };
    kw(text, at, "de l'").or_else(|| longest(text, at, words).map(|(_, end)| ws0(text, end)))
}

/// Entier de un ou deux chiffres compris entre 1 et `max`.
fn small(text: &str, at: usize, max: u8) -> Option<(u8, usize)> {
    let (value, end) = uint(text, at)?;
    let value = u8::try_from(value).ok().filter(|v| (1..=max).contains(v) && end - at <= 2)?;
    Some((value, end))
}

/// Entier de deux chiffres compris entre 1 et `max` (formes `JJ` et `MM` de la date ISO).
fn two_digits(text: &str, at: usize, max: u8) -> Option<(u8, usize)> {
    small(text, at, max).filter(|&(_, end)| end - at == 2)
}

/// `YEAR4` : quatre chiffres entre 1500 et 2099, suivis d'une frontière de mot.
fn year4(text: &str, at: usize) -> Option<(u16, usize)> {
    let (value, end) = uint(text, at)?;
    let year = u16::try_from(value).ok().filter(|y| (1500..=2099).contains(y))?;
    (end - at == 4 && boundary(text, end)).then_some((year, end))
}

/// Mois et leurs abréviations, repliés (annexe B §4.1).
const MONTHS: [(&str, u8); 22] = [
    ("janvier", 1),
    ("janv", 1),
    ("fevrier", 2),
    ("fevr", 2),
    ("fev", 2),
    ("mars", 3),
    ("avril", 4),
    ("avr", 4),
    ("mai", 5),
    ("juin", 6),
    ("juillet", 7),
    ("juill", 7),
    ("juil", 7),
    ("aout", 8),
    ("septembre", 9),
    ("sept", 9),
    ("octobre", 10),
    ("oct", 10),
    ("novembre", 11),
    ("nov", 11),
    ("decembre", 12),
    ("dec", 12),
];

/// Mois du calendrier républicain, repliés.
const REPUBLICAN_MONTHS: [&str; 12] = [
    "vendemiaire",
    "brumaire",
    "frimaire",
    "nivose",
    "pluviose",
    "ventose",
    "germinal",
    "floreal",
    "prairial",
    "messidor",
    "thermidor",
    "fructidor",
];

/// Une date lue, valide ou non.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DateHit {
    /// Date civile, `None` si invalide ou républicaine.
    pub date: Option<CivilDate>,
    /// Date telle qu'écrite quand elle est républicaine.
    pub raw: Option<String>,
    /// `DateInvalide` ou `DateRepublicaine`.
    pub flags: Vec<Flag>,
    /// Fin de la date.
    pub end: usize,
}

fn month_name(text: &str, at: usize) -> Option<(u8, usize)> {
    MONTHS
        .iter()
        .filter_map(|&(stem, number)| abbr(text, at, stem).map(|end| (number, end)))
        .max_by_key(|&(_, end)| end)
}

/// `DAY` : « 1er », « premier » ou un jour de 1 à 31 suivi d'une frontière de mot.
fn day(text: &str, at: usize) -> Option<(u8, usize)> {
    if let Some(end) = word(text, at, "1er").or_else(|| word(text, at, "premier")) {
        return Some((1, end));
    }
    small(text, at, 31).filter(|&(_, end)| boundary(text, end))
}

fn civil(year: u16, month: u8, day: u8, end: usize) -> DateHit {
    let date = CivilDate::new(year, month, day);
    let flags = date.map_or_else(|| vec![Flag::DateInvalide], |_| Vec::new());
    DateHit { date, raw: None, flags, end }
}

fn date_prose(text: &str, at: usize) -> Option<DateHit> {
    let (day, pos) = day(text, at)?;
    let (month, pos) = month_name(text, ws(text, pos)?)?;
    let (year, end) = year4(text, ws(text, pos)?)?;
    Some(civil(year, month, day, end))
}

fn dot_or_slash(text: &str, at: usize) -> Option<usize> {
    consume(text, at, '/').or_else(|| consume(text, at, '.'))
}

fn date_numeric(text: &str, at: usize) -> Option<DateHit> {
    let (day, pos) = small(text, at, 31)?;
    let (month, pos) = small(text, dot_or_slash(text, pos)?, 12)?;
    let (year, end) = year4(text, dot_or_slash(text, pos)?)?;
    Some(civil(year, month, day, end))
}

fn date_iso(text: &str, at: usize) -> Option<DateHit> {
    let (year, pos) = year4(text, at)?;
    let (month, pos) = two_digits(text, consume(text, pos, '-')?, 12)?;
    let (day, end) = two_digits(text, consume(text, pos, '-')?, 31)?;
    boundary(text, end).then(|| civil(year, month, day, end))
}

fn date_republican(text: &str, at: usize) -> Option<DateHit> {
    let (_, pos) = day(text, at)?;
    let (_, pos) = longest(text, ws(text, pos)?, &REPUBLICAN_MONTHS)?;
    let pos = word(text, ws(text, pos)?, "an")?;
    let (_, end) = roman(text, ws(text, pos)?)?;
    let raw = text.get(at..end).map(str::to_owned);
    Some(DateHit { date: None, raw, flags: vec![Flag::DateRepublicaine], end })
}

/// `DATE` : prose, numérique, ISO ou républicaine (annexe B §4.1).
pub(super) fn date(text: &str, at: usize) -> Option<DateHit> {
    date_prose(text, at)
        .or_else(|| date_numeric(text, at))
        .or_else(|| date_iso(text, at))
        .or_else(|| date_republican(text, at))
}

/// `DATE_LINK` : une date précédée de « du », « en date du », « le » ou « des ».
pub(super) fn date_link(text: &str, at: usize) -> Option<DateHit> {
    let start = longest(text, at, &["en date du", "du", "le", "des"])
        .and_then(|(_, end)| ws(text, end))
        .unwrap_or(at);
    date(text, start)
}

/// Rang d'un ordinal : n-ième, ou « dernier ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ordinal {
    /// 1, 2, 3…
    Nth(u16),
    /// « Dernier », « dernière ».
    Last,
}

/// Ordinaux écrits en lettres ou en « 1er » (annexe B §4.1 : `ORD1_F`, `ORD1_M`, `ORD_WORD`).
const ORDINALS: [(&str, Ordinal); 20] = [
    ("premier", Ordinal::Nth(1)),
    ("premiere", Ordinal::Nth(1)),
    ("1er", Ordinal::Nth(1)),
    ("1re", Ordinal::Nth(1)),
    ("1ere", Ordinal::Nth(1)),
    ("deuxieme", Ordinal::Nth(2)),
    ("second", Ordinal::Nth(2)),
    ("seconde", Ordinal::Nth(2)),
    ("troisieme", Ordinal::Nth(3)),
    ("quatrieme", Ordinal::Nth(4)),
    ("cinquieme", Ordinal::Nth(5)),
    ("sixieme", Ordinal::Nth(6)),
    ("septieme", Ordinal::Nth(7)),
    ("huitieme", Ordinal::Nth(8)),
    ("neuvieme", Ordinal::Nth(9)),
    ("dixieme", Ordinal::Nth(10)),
    ("onzieme", Ordinal::Nth(11)),
    ("douzieme", Ordinal::Nth(12)),
    ("dernier", Ordinal::Last),
    ("derniere", Ordinal::Last),
];

/// Ordinal en lettres, en « 1er » ou en « 2e », « 3ème »… (`ORDN`).
pub(super) fn ordinal(text: &str, at: usize) -> Option<(Ordinal, usize)> {
    let spelled = ORDINALS
        .iter()
        .filter_map(|&(lit, rank)| word(text, at, lit).map(|end| (rank, end)))
        .max_by_key(|&(_, end)| end);
    spelled.or_else(|| {
        let (value, pos) = uint(text, at)?;
        let end = kw(text, pos, "eme").or_else(|| kw(text, pos, "e"))?;
        let value = u16::try_from(value).ok().filter(|v| (2..=99).contains(v))?;
        boundary(text, end).then_some((Ordinal::Nth(value), end))
    })
}

/// Chiffres romains `I`, `V`, `X` de 1 à 39, sensibles à la casse, suivis d'une frontière.
pub(super) fn roman(text: &str, at: usize) -> Option<(String, usize)> {
    let rest = text.get(at..).unwrap_or_default();
    let len = rest.bytes().take_while(|b| matches!(b, b'I' | b'V' | b'X')).count();
    let digits = rest.get(..len).unwrap_or_default();
    let tail = digits.trim_start_matches('X');
    let valid = digits.len() - tail.len() <= 3
        && matches!(tail, "" | "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII" | "IX");
    (len > 0 && valid && boundary(text, at + len)).then(|| (digits.to_owned(), at + len))
}

// ---------------------------------------------------------------------------------------------
// Numéros d'articles et subdivisions (annexe B §4.2)
// ---------------------------------------------------------------------------------------------

/// Préfixes d'articles, du plus long au plus court pour une même lettre.
const PREFIXES: [(&str, ArticlePrefix); 9] = [
    ("LO", ArticlePrefix::Lo),
    ("R**", ArticlePrefix::RStarStar),
    ("R*", ArticlePrefix::RStar),
    ("R", ArticlePrefix::R),
    ("D**", ArticlePrefix::DStarStar),
    ("D*", ArticlePrefix::DStar),
    ("D", ArticlePrefix::D),
    ("A", ArticlePrefix::A),
    ("L", ArticlePrefix::L),
];

/// `prefix` suivi d'un chiffre : le préfixe (sensible à la casse) et la position du chiffre.
fn prefix(text: &str, at: usize) -> Option<(ArticlePrefix, usize)> {
    let rest = text.get(at..).unwrap_or_default();
    let &(spelled, found) = PREFIXES.iter().find(|(spelled, _)| rest.starts_with(spelled))?;
    let pos = at + spelled.len();
    let pos = ws0(text, consume(text, pos, '.').unwrap_or(pos));
    next_char(text, pos).is_some_and(|c| c.is_ascii_digit()).then_some((found, pos))
}

/// `base_num` : `1234-5`, `600-4-1`.
fn base_num(text: &str, at: usize) -> Option<(Vec<u32>, usize)> {
    let (first, mut end) = uint(text, at)?;
    let mut base = vec![first];
    while let Some((next, after)) = consume(text, end, '-').and_then(|pos| uint(text, pos)) {
        base.push(next);
        end = after;
    }
    Some((base, end))
}

/// Mot de lettres lu en `at`.
fn letters_word(text: &str, at: usize) -> &str {
    let rest = text.get(at..).unwrap_or_default();
    let len: usize = rest.chars().take_while(|c| c.is_alphabetic()).map(char::len_utf8).sum();
    rest.get(..len).unwrap_or_default()
}

/// Mots de deux lettres qui, en capitales, ne sont pas des lettres de suffixe : « ARTICLE 5 ET 6 »,
/// « ARTICLE L. 1234-5 DU CODE DU TRAVAIL ».
const FUNCTION_WORDS: [&str; 8] = ["de", "du", "la", "le", "et", "ou", "au", "en"];

/// Lettres de suffixe (`199 undecies B`) : une ou deux majuscules, qui ne sont ni un sigle
/// ambigu, ni un mot de liaison, ni le début d'un code (« 1240 C. civ. », « 121-3 CP »).
fn capital_letters(text: &str, at: usize) -> Option<(String, usize)> {
    let rest = text.get(at..).unwrap_or_default();
    let len = rest.bytes().take_while(u8::is_ascii_uppercase).count();
    let letters = rest.get(..len).unwrap_or_default();
    let refused = Sigle::parse(letters).is_some()
        || FUNCTION_WORDS.contains(&fold_str(letters).as_str())
        || dict_at(text, at).is_some();
    ((1..=2).contains(&len) && boundary(text, at + len) && !refused)
        .then(|| (letters.to_owned(), at + len))
}

/// Suffixes d'un numéro : `[adverbe [-entier]] [lettres] [adverbe]`, chacun au plus une fois
/// (`199 undecies B`, `150-0 B ter`, `199 terdecies-0 A`). `None` quand un adverbe ou une lettre
/// valide ne peut plus être lu après un adverbe (`5 bis ter`, `885-0 V bis A`) : le numéro serait
/// tronqué, donc aucune citation n'est émise.
fn suffixes(text: &str, at: usize) -> Option<(Suffixes, usize)> {
    let (mut found, mut end) = (Suffixes::default(), at);
    while let Some(start) = ws(text, end) {
        let word = letters_word(text, start);
        if let Some(adverb) = LatinAdverb::parse(word) {
            if !found.push_adverb(adverb) {
                return None;
            }
            end = start + word.len();
            if let Some((sub, after)) = consume(text, end, '-').and_then(|pos| uint(text, pos))
                && found.push_sub(sub)
            {
                end = after;
            }
        } else if let Some((letters, after)) = capital_letters(text, start) {
            if !found.push_letters(letters) {
                return if found.after_adverb() { None } else { Some((found, end)) };
            }
            end = after;
        } else {
            break;
        }
    }
    Some((found, end))
}

/// `num_end` : une frontière de mot, sans « ° » (un item) ni « - » (numéro tronqué) collé.
fn num_end(text: &str, end: usize) -> bool {
    boundary(text, end) && !matches!(next_char(text, end), Some('°' | '-'))
}

/// Numéros particuliers : `1er`, `unique`, `liminaire`, `préliminaire`.
const SPECIALS: [(&str, SpecialNum); 5] = [
    ("1er", SpecialNum::Premier),
    ("premier", SpecialNum::Premier),
    ("unique", SpecialNum::Unique),
    ("liminaire", SpecialNum::Liminaire),
    ("preliminaire", SpecialNum::Preliminaire),
];

/// `art_num` : le numéro d'article et sa fin.
pub(super) fn art_num(text: &str, at: usize) -> Option<(ArticleNum, usize)> {
    let special =
        SPECIALS.iter().find_map(|&(lit, special)| word(text, at, lit).map(|end| (special, end)));
    let (num, end) = if let Some((special, end)) = special {
        (
            ArticleNum {
                prefix: None,
                special: Some(special),
                base: Vec::new(),
                latin: None,
                latin_sub: None,
                letters: None,
                latin_after: None,
            },
            end,
        )
    } else {
        let (found, pos) = prefix(text, at).map_or((None, at), |(p, pos)| (Some(p), pos));
        let (base, pos) = base_num(text, pos)?;
        let (Suffixes { latin, latin_sub, letters, latin_after }, end) = suffixes(text, pos)?;
        (
            ArticleNum {
                prefix: found,
                special: None,
                base,
                latin,
                latin_sub,
                letters,
                latin_after,
            },
            end,
        )
    };
    num_end(text, end).then_some((num, end))
}

/// `sub` : une subdivision à la suite d'un numéro, avec sa fin.
fn sub(text: &str, at: usize) -> Option<(Subdivision, usize)> {
    let alinea = longest(text, at, &["alineas", "alinea"])
        .map(|(_, end)| end)
        .or_else(|| abbr(text, at, "al"))
        .and_then(|end| uint(text, ws0(text, end)))
        .and_then(|(n, end)| u16::try_from(n).ok().map(|n| (Subdivision::Alinea(n), end)));
    alinea
        .or_else(|| {
            let (n, end) = uint(text, at)?;
            let end = consume(text, end, '°')?;
            u16::try_from(n).ok().map(|n| (Subdivision::Item(n), end))
        })
        .or_else(|| roman(text, at).map(|(digits, end)| (Subdivision::Paragraphe(digits), end)))
        .or_else(|| {
            let (n, end) = uint(text, ws0(text, consume(text, at, '§')?))?;
            Some((Subdivision::Paragraphe(n.to_string()), end))
        })
        .or_else(|| {
            let letter = next_char(text, at).filter(char::is_ascii_lowercase)?;
            let end = consume(text, at + 1, ')')?;
            Some((Subdivision::Lettre(letter), end))
        })
}

/// `sub_list` : zéro ou plusieurs `SEP sub`.
fn sub_list(text: &str, at: usize) -> (Vec<Subdivision>, usize) {
    let (mut subs, mut end) = (Vec::new(), at);
    while let Some((found, after)) = sep(text, end).and_then(|pos| sub(text, pos)) {
        subs.push(found);
        end = after;
    }
    (subs, end)
}

/// Rang d'une subdivision, du plus grand niveau au plus petit (annexe B §4.2).
fn sub_rank(sub: &Subdivision) -> u8 {
    match sub {
        Subdivision::Paragraphe(_) | Subdivision::Section(_) => 0,
        Subdivision::Alinea(_) | Subdivision::DernierAlinea => 1,
        Subdivision::Item(_) => 2,
        Subdivision::Lettre(_) => 3,
        Subdivision::Phrase(_) | Subdivision::DernierePhrase => 4,
    }
}

/// `unit_lead` : « premier alinéa », « 3° », « I ».
fn unit_lead(text: &str, at: usize) -> Option<(Subdivision, usize)> {
    let alinea = ordinal(text, at).and_then(|(rank, end)| {
        let end = word(text, ws(text, end)?, "alinea")?;
        Some((rank_alinea(rank), end))
    });
    alinea
        .or_else(|| {
            let (n, end) = uint(text, at)?;
            let end = consume(text, end, '°')?;
            u16::try_from(n).ok().map(|n| (Subdivision::Item(n), end))
        })
        .or_else(|| roman(text, at).map(|(digits, end)| (Subdivision::Paragraphe(digits), end)))
}

fn rank_alinea(rank: Ordinal) -> Subdivision {
    match rank {
        Ordinal::Nth(n) => Subdivision::Alinea(n),
        Ordinal::Last => Subdivision::DernierAlinea,
    }
}

fn rank_phrase(rank: Ordinal) -> Subdivision {
    match rank {
        Ordinal::Nth(n) => Subdivision::Phrase(n),
        Ordinal::Last => Subdivision::DernierePhrase,
    }
}

/// `phrase_lead` : « première phrase du ».
fn phrase_lead(text: &str, at: usize) -> Option<(Subdivision, usize)> {
    let (rank, end) = ordinal(text, at)?;
    let end = word(text, ws(text, end)?, "phrase")?;
    let end = ws(text, word(text, ws(text, end)?, "du")?)?;
    Some((rank_phrase(rank), end))
}

/// `sub_lead` : les subdivisions citées avant le mot « article » (« la première phrase du
/// troisième alinéa de l' »), dans l'ordre de lecture, et la fin, liaison comprise.
pub(super) fn sub_lead(text: &str, at: usize) -> Option<(Vec<Subdivision>, usize)> {
    let mut subs = Vec::new();
    let mut pos = at;
    if let Some((phrase, end)) = phrase_lead(text, pos) {
        subs.push(phrase);
        pos = end;
    }
    let (first, mut end) = unit_lead(text, pos)?;
    subs.push(first);
    while let Some((unit, after)) = ws(text, end)
        .and_then(|p| {
            longest(text, p, &["de la", "du"]).map(|(_, e)| e).or_else(|| kw(text, p, "de l'"))
        })
        .and_then(|p| unit_lead(text, ws0(text, p)))
    {
        subs.push(unit);
        end = after;
    }
    let closing = ws(text, end).and_then(|p| link(text, p, false))?;
    Some((subs, closing))
}

/// `art_kw` suivi de ses espaces : « articles », « article », « arts », « art. », « art ».
pub(super) fn art_kw(text: &str, at: usize) -> Option<usize> {
    longest(text, at, &["articles", "article", "arts"]).and_then(|(_, end)| ws(text, end)).or_else(
        || {
            let end = kw(text, at, "art")?;
            match consume(text, end, '.') {
                Some(after) => Some(ws0(text, after)),
                None => boundary(text, end).then(|| ws(text, end)).flatten(),
            }
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Articles, listes et plages
// ---------------------------------------------------------------------------------------------

/// Un article de la liste, avec ses subdivisions et son qualificatif de version.
#[derive(Clone, Debug)]
pub(super) struct Item {
    /// Numéro.
    pub num: ArticleNum,
    /// Fin d'une plage (« 1100 à 1100-2 »).
    pub range_to: Option<ArticleNum>,
    /// Plage du numéro seul, qualificatifs et subdivisions exclus.
    pub span: Range<usize>,
    /// Subdivisions citées après le numéro.
    pub subs: Vec<Subdivision>,
    /// « Ancien » ou « nouveau » collé au numéro.
    pub version: Option<VersionQualifier>,
    /// Fin de l'article, subdivisions et version comprises.
    pub end: usize,
}

/// `version_inline` : « ancien », « (ancien) », « nouveau ».
fn inline_version(text: &str, at: usize) -> Option<(VersionQualifier, usize)> {
    let start = ws(text, at)?;
    word(text, start, "ancien")
        .or_else(|| kw(text, start, "(ancien)"))
        .map(|end| (VersionQualifier::Ancien, end))
        .or_else(|| word(text, start, "nouveau").map(|end| (VersionQualifier::Nouveau, end)))
}

/// `art_item` : un numéro, son qualificatif de version collé et ses subdivisions.
pub(super) fn art_item(text: &str, at: usize) -> Option<Item> {
    let (num, num_end) = art_num(text, at)?;
    let (version, pos) =
        inline_version(text, num_end).map_or((None, num_end), |(v, end)| (Some(v), end));
    let (subs, end) = sub_list(text, pos);
    Some(Item { num, range_to: None, span: at..num_end, subs, version, end })
}

/// Les articles d'une liste ou d'une plage.
#[derive(Clone, Debug)]
pub(super) struct ArtList {
    /// Un élément par numéro, un seul pour une plage.
    pub items: Vec<Item>,
    /// Fin de la liste.
    pub end: usize,
}

/// `LIST_SEP` : « , », « , et », « et », « ou ».
fn list_sep(text: &str, at: usize) -> Option<usize> {
    let pos = ws0(text, at);
    if let Some(after) = consume(text, pos, ',') {
        let after = ws0(text, after);
        return Some(word(text, after, "et").and_then(|end| ws(text, end)).unwrap_or(after));
    }
    let start = ws(text, at)?;
    let end = word(text, start, "et").or_else(|| word(text, start, "ou"))?;
    ws(text, end)
}

/// Seconde borne d'une plage : « à 1100-2 ». Le « à » est lu avec son accent : « a » est aussi
/// le verbe avoir.
fn range_end(text: &str, at: usize) -> Option<Item> {
    let start = ws(text, at)?;
    let after = consume(text, start, 'à').or_else(|| consume(text, start, 'À'))?;
    art_item(text, ws(text, after)?)
}

/// `art_list` : des articles séparés par `LIST_SEP`, ou une plage « 1100 à 1100-2 ».
pub(super) fn art_list(text: &str, at: usize) -> Option<ArtList> {
    let first = art_item(text, at)?;
    if let Some(last) = range_end(text, first.end) {
        let mut subs = first.subs;
        subs.extend(last.subs);
        let item = Item {
            num: first.num,
            range_to: Some(last.num),
            span: first.span.start..last.span.end,
            subs,
            version: first.version.or(last.version),
            end: last.end,
        };
        return Some(ArtList { end: item.end, items: vec![item] });
    }
    let mut end = first.end;
    let mut items = vec![first];
    while let Some(next) = list_sep(text, end).and_then(|pos| art_item(text, pos)) {
        end = next.end;
        items.push(next);
    }
    Some(ArtList { items, end })
}

// ---------------------------------------------------------------------------------------------
// Codes (annexe B §4.2, §5)
// ---------------------------------------------------------------------------------------------

/// Un code reconnu et ce qui s'y attache.
#[derive(Clone, Debug)]
pub(super) struct CodeHit {
    /// Code cité.
    pub code: CodeRef,
    /// Confiance.
    pub confidence: Confidence,
    /// Ambiguïté, une au plus.
    pub ambiguity: Option<Ambiguity>,
    /// Fin de la référence.
    pub end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PatKind {
    /// Titre officiel, comparé sans casse ni accents.
    Title,
    /// Abréviation à points, lue par jetons.
    Abbr,
    /// Sigle en majuscules, sensible à la casse.
    Sigle,
}

/// Motif du dictionnaire : un titre, une abréviation ou un sigle d'un code.
struct Pat {
    kind: PatKind,
    /// Titre replié, ou sigle tel quel.
    lit: String,
    /// Jetons repliés d'une abréviation.
    tokens: Vec<String>,
    /// L'abréviation se termine par un point.
    dot: bool,
    key: CodeKey,
    confidence: Confidence,
    ambiguity: Option<Ambiguity>,
    /// Premier caractère replié.
    first: Option<char>,
}

pub(super) fn fold_str(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

fn title_pat(
    title: &str,
    key: CodeKey,
    confidence: Confidence,
    ambiguity: Option<Ambiguity>,
) -> Pat {
    let lit = fold_str(title);
    let first = lit.chars().next();
    Pat {
        kind: PatKind::Title,
        lit,
        tokens: Vec::new(),
        dot: false,
        key,
        confidence,
        ambiguity,
        first,
    }
}

fn abbr_pat(abbreviation: &str, key: CodeKey, confidence: Confidence) -> Pat {
    let sigle = abbreviation.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let tokens: Vec<String> = abbreviation
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(fold_str)
        .collect();
    let first = abbreviation.chars().next().map(fold_char);
    Pat {
        kind: if sigle { PatKind::Sigle } else { PatKind::Abbr },
        lit: abbreviation.to_owned(),
        tokens,
        dot: abbreviation.ends_with('.'),
        key,
        confidence,
        ambiguity: None,
        first,
    }
}

/// Titres, alias sans « (nouveau) » ni « (ancien) », abréviations et sigles des 77 codes.
fn patterns() -> Vec<Pat> {
    let mut out = Vec::new();
    for entry in &CODES {
        out.push(title_pat(entry.title, entry.key, Confidence::High, None));
        let generic =
            entry.title.strip_suffix(" (nouveau)").map(|stem| (stem, entry.key)).or_else(|| {
                let stem = entry.title.strip_suffix(" (ancien)")?;
                Some((stem, entry.variant_of.unwrap_or(entry.key)))
            });
        if let Some((stem, key)) = generic {
            // « code minier » vise la version nouvelle ; l'autre version est essayée ensuite.
            let (confidence, ambiguity) = if entry.variant_of.is_some() {
                (Confidence::Medium, Some(Ambiguity::CodeAVariantes))
            } else {
                (Confidence::High, None)
            };
            out.push(title_pat(stem, key, confidence, ambiguity));
        }
        for &(abbreviation, confidence) in entry.abbreviations {
            out.push(abbr_pat(abbreviation, entry.key, confidence));
        }
    }
    out
}

static PATTERNS: LazyLock<Vec<Pat>> = LazyLock::new(patterns);

/// Entre deux jetons d'une abréviation : un point ou une virgule facultatifs, puis des espaces,
/// pour au moins un caractère.
fn token_gap(text: &str, at: usize) -> Option<usize> {
    let mark = at + usize::from(matches!(next_char(text, at), Some('.' | ',')));
    let end = ws0(text, mark);
    (end > at).then_some(end)
}

fn pat_end(text: &str, at: usize, pat: &Pat) -> Option<usize> {
    match pat.kind {
        PatKind::Title => word(text, at, &pat.lit),
        PatKind::Sigle => text
            .get(at..)
            .filter(|rest| rest.starts_with(pat.lit.as_str()))
            .map(|_| at + pat.lit.len())
            .filter(|&end| boundary(text, end)),
        PatKind::Abbr => {
            let mut pos = at;
            for (i, token) in pat.tokens.iter().enumerate() {
                if i > 0 {
                    pos = token_gap(text, pos)?;
                }
                pos = kw(text, pos, token)?;
            }
            match consume(text, pos, '.').filter(|_| pat.dot) {
                Some(after) => Some(after),
                None => boundary(text, pos).then_some(pos),
            }
        }
    }
}

/// Titre ou abréviation du dictionnaire lu en `at`, la plus longue correspondance.
pub(super) fn dict_at(text: &str, at: usize) -> Option<CodeHit> {
    let initial = next_char(text, at).map(fold_char);
    PATTERNS
        .iter()
        .filter(|pat| pat.first == initial)
        .filter_map(|pat| pat_end(text, at, pat).map(|end| (pat, end)))
        .max_by_key(|&(_, end)| end)
        .map(|(pat, end)| CodeHit {
            code: CodeRef::Known(pat.key),
            confidence: pat.confidence,
            ambiguity: pat.ambiguity.clone(),
            end,
        })
}

/// Sigle ambigu (annexe B §5.3) : jamais résolu sans réserve, confiance basse. `CE` et `CPCE`
/// n'ont pas de code.
fn sigle_at(text: &str, at: usize) -> Option<CodeHit> {
    let rest = text.get(at..).unwrap_or_default();
    let len = rest.bytes().take_while(u8::is_ascii_uppercase).count();
    let sigle = rest.get(..len).and_then(Sigle::parse)?;
    let keys: Vec<CodeKey> = CODES
        .iter()
        .filter(|entry| entry.ambiguous_sigles.contains(&sigle.as_str()))
        .map(|entry| entry.key)
        .collect();
    let code = match keys.as_slice() {
        [key] if sigle != Sigle::Ce => CodeRef::Known(*key),
        _ => CodeRef::Missing,
    };
    boundary(text, at + len).then_some(CodeHit {
        code,
        confidence: Confidence::Low,
        ambiguity: Some(Ambiguity::SigleAmbigu(sigle)),
        end: at + len,
    })
}

/// Formules qui renvoient au dernier code cité.
const ANAPHORAS: [(&str, Anaphora); 5] = [
    ("meme code", Anaphora::MemeCode),
    ("dudit code", Anaphora::Dudit),
    ("ledit code", Anaphora::Dudit),
    ("du present code", Anaphora::PresentCode),
    ("de ce code", Anaphora::CeCode),
];

fn anaphora_at(text: &str, at: usize) -> Option<CodeHit> {
    let (anaphora, end) =
        ANAPHORAS.iter().find_map(|&(lit, a)| word(text, at, lit).map(|end| (a, end)))?;
    Some(CodeHit {
        code: CodeRef::Anaphora(anaphora),
        confidence: Confidence::High,
        ambiguity: None,
        end,
    })
}

/// Mots qui arrêtent un titre de code hors dictionnaire.
const GENERIC_STOPS: [&str; 4] = ["dans", "en", "tel", "applicable"];

/// `code_generic` : « code de la voirie communale » hors dictionnaire, huit mots au plus.
fn generic_at(text: &str, at: usize) -> Option<CodeHit> {
    let start = word(text, at, "code").and_then(|end| ws(text, end))?;
    let mut pos = link(text, start, true)?;
    let (mut end, mut count) = (pos, 0);
    while count < GENERIC_MAX_WORDS {
        let spelled = text
            .get(pos..)
            .map(|rest| rest.chars().take_while(|c| c.is_alphabetic() || matches!(c, '\'' | '-')))
            .map_or(0, |run| run.map(char::len_utf8).sum::<usize>());
        let folded = fold_str(text.get(pos..pos + spelled).unwrap_or_default());
        if spelled == 0 || GENERIC_STOPS.contains(&folded.as_str()) {
            break;
        }
        end = pos + spelled;
        count += 1;
        match ws(text, end) {
            Some(next) => pos = next,
            None => break,
        }
    }
    (count > 0).then(|| CodeHit {
        code: CodeRef::Unknown(text.get(at..end).unwrap_or_default().to_owned()),
        confidence: Confidence::Medium,
        ambiguity: None,
        end,
    })
}

/// `code_ref` : titre ou abréviation, sigle ambigu, anaphore, titre hors dictionnaire.
fn code_core(text: &str, at: usize) -> Option<CodeHit> {
    dict_at(text, at)
        .or_else(|| sigle_at(text, at))
        .or_else(|| anaphora_at(text, at))
        .or_else(|| generic_at(text, at))
}

/// `code_link code_ref` : la liaison est facultative.
fn code_part(text: &str, at: usize) -> Option<CodeHit> {
    link(text, at, false).and_then(|pos| code_core(text, pos)).or_else(|| code_core(text, at))
}

/// Intervalle entre un article et son code : des espaces, une virgule facultative.
fn gap(text: &str, at: usize) -> Option<usize> {
    let pos = ws0(text, at);
    let end = consume(text, pos, ',').map_or(pos, |after| ws0(text, after));
    (end > at).then_some(end)
}

// ---------------------------------------------------------------------------------------------
// Qualificatifs de version (annexe B §4.2, `version_q`)
// ---------------------------------------------------------------------------------------------

/// Article ou déterminant qui précède un texte cité : « l' », « la », « le », « les ».
fn determiner(text: &str, at: usize) -> usize {
    let pos = ws0(text, at);
    kw(text, pos, "l'")
        .or_else(|| longest(text, pos, &["la", "le", "les"]).map(|(_, end)| ws0(text, end)))
        .unwrap_or(pos)
}

fn en_vigueur_au(text: &str, at: usize) -> Option<(VersionQualifier, usize)> {
    let start = kw(text, at, "en vigueur au").and_then(|end| ws(text, end))?;
    let hit = date(text, start)?;
    Some((VersionQualifier::EnVigueurAu(hit.date?), hit.end))
}

fn applicable(text: &str, at: usize) -> Option<(VersionQualifier, usize)> {
    longest(text, at, &["applicable au litige", "applicable a la date des faits"]).map(
        |(i, end)| {
            let version = if i == 0 {
                VersionQualifier::RedactionApplicableAuLitige
            } else {
                VersionQualifier::RedactionApplicableDateDesFaits
            };
            (version, end)
        },
    )
}

fn en_vigueur(text: &str, at: usize) -> Option<(VersionQualifier, usize)> {
    let end = word(text, at, "en vigueur")?;
    let dated = ws(text, end).and_then(|pos| date_link(text, pos));
    let version = VersionQualifier::RedactionEnVigueur(dated.as_ref().and_then(|d| d.date));
    Some((version, dated.map_or(end, |d| d.end)))
}

fn issue_de(text: &str, at: usize) -> Option<(VersionQualifier, usize)> {
    // La contraction « du » est admise en plus des formes de l'annexe : « issue du décret ».
    let (i, end) = longest(
        text,
        at,
        &[
            "anterieure a celle issue de",
            "anterieure a celle issue du",
            "anterieure a",
            "anterieure au",
            "issue de",
            "issue du",
            "resultant de",
            "resultant du",
        ],
    )?;
    let hit = textes::text_cite(text, determiner(text, end))?;
    let cite = Box::new(hit.cite);
    let version = if i < 4 {
        VersionQualifier::RedactionAnterieureA(cite)
    } else {
        VersionQualifier::RedactionIssueDe(cite)
    };
    Some((version, hit.end))
}

/// `version_q` : « dans sa rédaction applicable au litige », « en vigueur au 1er janvier 2018 »…
fn version_q(text: &str, at: usize) -> Option<(VersionQualifier, usize)> {
    let pos = sep(text, at)?;
    en_vigueur_au(text, pos).or_else(|| {
        let after = longest(text, pos, &["dans sa redaction", "dans leur redaction"])
            .and_then(|(_, end)| ws(text, end))?;
        applicable(text, after)
            .or_else(|| en_vigueur(text, after))
            .or_else(|| issue_de(text, after))
    })
}

// ---------------------------------------------------------------------------------------------
// Candidats
// ---------------------------------------------------------------------------------------------

/// Une citation reconnue, en offsets de `Normalized::text`.
#[derive(Clone, Debug)]
pub(super) struct Found {
    /// Plage du span.
    pub range: Range<usize>,
    /// Valeur.
    pub value: CitationRef,
    /// Confiance.
    pub confidence: Confidence,
    /// Ambiguïté.
    pub ambiguity: Option<Ambiguity>,
    /// Drapeaux.
    pub flags: Vec<Flag>,
    /// Étendue du groupe, pour un membre d'énumération.
    pub group: Option<Range<usize>>,
}

/// Ce qu'une grammaire reconnaît à partir d'un point : ses citations et la fin de la lecture.
#[derive(Clone, Debug)]
pub(super) struct Hit {
    /// Fin de la lecture, qualificatifs compris.
    pub end: usize,
    /// Une citation, ou une par numéro d'une énumération.
    pub found: Vec<Found>,
}

/// Fin et attributs communs aux citations d'une liste d'articles.
pub(super) struct Tail {
    /// Fin du span d'une citation seule.
    pub span_end: usize,
    /// Fin du groupe d'une énumération.
    pub group_end: usize,
    /// Confiance.
    pub confidence: Confidence,
    /// Ambiguïté.
    pub ambiguity: Option<Ambiguity>,
    /// Drapeaux.
    pub flags: Vec<Flag>,
    /// Qualificatif de version, distribué aux articles qui n'en portent pas.
    pub version: Option<VersionQualifier>,
}

/// Une citation par article de la liste ; `make` bâtit la valeur d'un article (subdivisions de
/// tête comprises, rangées du plus grand niveau au plus petit).
pub(super) fn article_founds(
    start: usize,
    lead: &[Subdivision],
    list: &ArtList,
    tail: &Tail,
    make: &dyn Fn(ArticleRef, Option<VersionQualifier>) -> CitationRef,
) -> Vec<Found> {
    let grouped = list.items.len() > 1;
    list.items
        .iter()
        .map(|item| {
            let mut sub = lead.to_vec();
            sub.extend(item.subs.iter().cloned());
            sub.sort_by_key(sub_rank);
            let article =
                ArticleRef { num: item.num.clone(), sub, range_to: item.range_to.clone() };
            Found {
                range: if grouped { item.span.clone() } else { start..tail.span_end },
                value: make(article, item.version.clone().or_else(|| tail.version.clone())),
                confidence: tail.confidence,
                ambiguity: tail.ambiguity.clone(),
                flags: tail.flags.clone(),
                group: grouped.then_some(start..tail.group_end),
            }
        })
        .collect()
}

/// Candidat d'arbitrage d'une citation reconnue.
fn candidate(n: &Normalized, found: Found) -> Candidate {
    let mut candidate = Candidate::new(n.orig_range(found.range), found.value);
    candidate.confidence = found.confidence;
    candidate.flags.list = found.flags;
    candidate.flags.ambiguity = found.ambiguity;
    if let Some(group) = found.group {
        let extent = n.orig_range(group);
        candidate.flags.enum_group = Some(u32::try_from(extent.start).unwrap_or(u32::MAX));
        candidate.flags.group_extent = Some(extent);
    }
    candidate
}

/// Débuts de mot essayés pour une ancre : au plus 12 débuts de mot situés dans les 80
/// caractères qui précèdent (le plus ancien d'abord), puis l'ancre elle-même.
fn start_positions(text: &str, anchor: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = text
        .get(..anchor)
        .unwrap_or_default()
        .char_indices()
        .rev()
        .take(WINDOW_BEFORE)
        .filter(|&(i, c)| c.is_alphanumeric() && left_boundary(text, i))
        .map(|(i, _)| i)
        .take(START_POSITIONS)
        .collect();
    starts.reverse();
    starts.push(anchor);
    starts
}

/// Fin de la fenêtre de lecture d'une ancre : 400 caractères après elle, prolongés jusqu'à la fin
/// du mot ou du numéro coupé (au plus 32 caractères) : un numéro lu à la frontière de la fenêtre
/// ne doit pas être tronqué.
fn window_end(text: &str, anchor: usize) -> usize {
    let cut = text
        .get(anchor..)
        .and_then(|rest| rest.char_indices().nth(WINDOW_AFTER))
        .map_or(text.len(), |(i, _)| anchor + i);
    let slack: usize = text
        .get(cut..)
        .unwrap_or_default()
        .chars()
        .take(WINDOW_SLACK)
        .take_while(|c| c.is_alphanumeric() || *c == '-')
        .map(char::len_utf8)
        .sum();
    cut + slack
}

/// Parcourt les ancres de `classes` de gauche à droite : pour chacune, `parse` est essayé aux
/// débuts de mot de la fenêtre, le plus ancien d'abord, et la première lecture réussie est
/// gardée. Une ancre déjà couverte par une lecture de la famille est ignorée.
pub(super) fn drive(
    n: &Normalized,
    anchors: &[Anchor],
    classes: &[AnchorClass],
    parse: fn(&str, usize) -> Option<Hit>,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut covered = 0;
    for anchor in anchors.iter().filter(|a| classes.contains(&a.class)) {
        if anchor.range.start < covered {
            continue;
        }
        let window = n.text.get(..window_end(&n.text, anchor.range.start)).unwrap_or(&n.text);
        let hit = start_positions(window, anchor.range.start)
            .into_iter()
            .filter(|&start| start >= covered && left_boundary(window, start))
            .find_map(|start| parse(window, start));
        if let Some(hit) = hit {
            covered = hit.end;
            out.extend(hit.found.into_iter().map(|found| candidate(n, found)));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Formes de l'annexe B §4.2
// ---------------------------------------------------------------------------------------------

/// Un article d'un code, avec ce que le code apporte.
fn coded(code: &CodeHit) -> impl Fn(ArticleRef, Option<VersionQualifier>) -> CitationRef + '_ {
    move |article, version| CitationRef::CodeArticle {
        code: code.code.clone(),
        article,
        version,
        code_via: None,
    }
}

/// `code_ref SEP art_kw WS art_item` : « Code civil, art. 9 », « C. civ., art. 1240 ».
fn code_first(text: &str, start: usize) -> Option<Hit> {
    let code = dict_at(text, start)?;
    let item = sep(text, code.end)
        .and_then(|pos| art_kw(text, pos))
        .and_then(|pos| art_item(text, pos))?;
    let end = item.end;
    let tail = Tail {
        span_end: end,
        group_end: end,
        confidence: code.confidence,
        ambiguity: code.ambiguity.clone(),
        flags: Vec::new(),
        version: None,
    };
    let list = ArtList { items: vec![item], end };
    Some(Hit { end, found: article_founds(start, &[], &list, &tail, &coded(&code)) })
}

/// `[sub_lead] art_kw WS art_list [code_link code_ref [version_q]]` : formes 1, 3, 4 et 6.
fn article_first(text: &str, start: usize) -> Option<Hit> {
    let (lead, pos) = sub_lead(text, start).unwrap_or_else(|| (Vec::new(), start));
    let list = art_list(text, art_kw(text, pos)?)?;
    let Some(code) = gap(text, list.end).and_then(|pos| code_part(text, pos)) else {
        // Sans code ni texte : « l'article 12 ».
        let tail = Tail {
            span_end: list.end,
            group_end: list.end,
            confidence: Confidence::High,
            ambiguity: Some(Ambiguity::TexteManquant),
            flags: Vec::new(),
            version: None,
        };
        let missing = CodeHit {
            code: CodeRef::Missing,
            confidence: Confidence::High,
            ambiguity: None,
            end: list.end,
        };
        return Some(Hit {
            end: list.end,
            found: article_founds(start, &lead, &list, &tail, &coded(&missing)),
        });
    };
    let version = version_q(text, code.end);
    let end = version.as_ref().map_or(code.end, |&(_, end)| end);
    let tail = Tail {
        span_end: end,
        group_end: code.end,
        confidence: code.confidence,
        ambiguity: code.ambiguity.clone(),
        flags: Vec::new(),
        version: version.map(|(version, _)| version),
    };
    Some(Hit { end, found: article_founds(start, &lead, &list, &tail, &coded(&code)) })
}

/// `prefixed_num …` : formes 5 et 7, un numéro préfixé avec ou sans code.
fn prefixed(text: &str, start: usize) -> Option<Hit> {
    let item = art_item(text, start).filter(|item| item.num.prefix.is_some())?;
    let (num_span, item_end, dashed) = (item.span.clone(), item.end, item.num.base.len() > 1);
    let list = ArtList { items: vec![item], end: item_end };
    if let Some(code) = gap(text, item_end).and_then(|pos| code_part(text, pos)) {
        let version = version_q(text, code.end);
        let end = version.as_ref().map_or(code.end, |&(_, end)| end);
        let tail = Tail {
            span_end: end,
            group_end: code.end,
            confidence: code.confidence,
            ambiguity: code.ambiguity.clone(),
            flags: Vec::new(),
            version: version.map(|(version, _)| version),
        };
        return Some(Hit { end, found: article_founds(start, &[], &list, &tail, &coded(&code)) });
    }
    // Sans code, seul un numéro à tiret est une citation (annexe B §4.2, règle 3).
    dashed.then(|| {
        let tail = Tail {
            span_end: num_span.end,
            group_end: num_span.end,
            confidence: Confidence::High,
            ambiguity: Some(Ambiguity::CodeManquant),
            flags: Vec::new(),
            version: None,
        };
        let missing = CodeHit {
            code: CodeRef::Missing,
            confidence: Confidence::High,
            ambiguity: None,
            end: num_span.end,
        };
        Hit { end: num_span.end, found: article_founds(start, &[], &list, &tail, &coded(&missing)) }
    })
}

/// Les sept formes de l'annexe B §4.2, essayées à `start`.
fn code_article_at(text: &str, start: usize) -> Option<Hit> {
    code_first(text, start).or_else(|| article_first(text, start)).or_else(|| prefixed(text, start))
}

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    drive(
        n,
        anchors,
        &[
            AnchorClass::ArticleKeyword,
            AnchorClass::Code,
            AnchorClass::ArticlePrefix,
            AnchorClass::Sigle,
        ],
        code_article_at,
    )
}

/// Outils de test partagés par les trois grammaires de V2-10 : ancres construites à la main.
#[cfg(test)]
pub(in crate::citation::grammar) mod test_support {
    use std::fmt::Write as _;

    use crate::citation::normalize::{Normalized, Steps, normalize};
    use crate::citation::scan::{Anchor, AnchorClass};
    use crate::citation::types::{CitationRef, Confidence, UnsupportedCite, UnsupportedFamily};

    use super::super::Candidate;

    /// Texte normalisé sans étape facultative.
    pub(in crate::citation::grammar) fn norm(input: &str) -> Normalized {
        normalize(input, Steps::default())
    }

    /// Un candidat d'une autre famille, pour exercer le repli des `show` des tests.
    pub(in crate::citation::grammar) fn foreign() -> Candidate {
        Candidate::new(
            0..1,
            CitationRef::Unsupported(UnsupportedCite { family: UnsupportedFamily::Ue, key: None }),
        )
    }

    /// Ancres écrites à la main : une par occurrence de chaque aiguille (casse ignorée), triées
    /// par début, avec la classe donnée.
    pub(in crate::citation::grammar) fn anchors(
        n: &Normalized,
        spec: &[(AnchorClass, &str)],
    ) -> Vec<Anchor> {
        let lower = n.text.to_ascii_lowercase();
        let mut found: Vec<Anchor> = spec
            .iter()
            .flat_map(|&(class, needle)| {
                lower
                    .match_indices(&needle.to_ascii_lowercase())
                    .map(move |(i, m)| Anchor { class, range: i..i + m.len() })
                    .collect::<Vec<_>>()
            })
            .collect();
        found.sort_by_key(|a| a.range.start);
        found
    }

    /// Textes d'origine couverts par les candidats.
    pub(in crate::citation::grammar) fn spans<'a>(
        input: &'a str,
        found: &[Candidate],
    ) -> Vec<&'a str> {
        found.iter().map(|c| &input[c.range.clone()]).collect()
    }

    /// Ce que tout candidat porte en plus de sa valeur : confiance non haute, ambiguïté,
    /// drapeaux et appartenance à une énumération, chacun précédé d'une espace.
    pub(in crate::citation::grammar) fn tail(c: &Candidate) -> String {
        let mut s = String::new();
        if c.confidence != Confidence::High {
            let _ = write!(s, " conf={:?}", c.confidence);
        }
        if let Some(a) = &c.flags.ambiguity {
            let _ = write!(s, " amb={}", a.code());
        }
        if !c.flags.list.is_empty() {
            let codes: Vec<_> = c.flags.list.iter().map(|f| f.code().into_owned()).collect();
            let _ = write!(s, " flags={codes:?}");
        }
        if c.flags.enum_group.is_some() {
            s += " grp";
        }
        s
    }

    /// Une ligne d'un tableau de cas : entrée, ancres, citations attendues (texte d'origine
    /// couvert, description).
    pub(in crate::citation::grammar) type Row<'a> =
        (&'a str, &'a [(AnchorClass, &'a str)], &'a [(&'a str, &'a str)]);

    /// Famille testée : sa fonction `scan`.
    pub(in crate::citation::grammar) type Scan = fn(&Normalized, &[Anchor]) -> Vec<Candidate>;

    /// Vérifie chaque ligne : les candidats de `scan` doivent couvrir les textes attendus et
    /// être décrits par `show`.
    pub(in crate::citation::grammar) fn check_rows(
        scan: Scan,
        show: fn(&Candidate) -> String,
        rows: &[Row<'_>],
    ) {
        for &(input, spec, expected) in rows {
            let n = norm(input);
            let found = scan(&n, &anchors(&n, spec));
            let got: Vec<(String, String)> = spans(input, &found)
                .into_iter()
                .map(str::to_owned)
                .zip(found.iter().map(show))
                .collect();
            let want: Vec<(String, String)> =
                expected.iter().map(|&(a, b)| (a.to_owned(), b.to_owned())).collect();
            pretty_assertions::assert_eq!(got, want, "entrée : {input:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::test_support::{anchors, foreign, norm, spans, tail};
    use super::*;
    use crate::citation::scan::AnchorClass::{self, ArticleKeyword, ArticlePrefix, Code};
    use std::fmt::Write as _;

    const ART: (AnchorClass, &str) = (ArticleKeyword, "article");
    const ART_DOT: (AnchorClass, &str) = (ArticleKeyword, "art.");
    const CODE: (AnchorClass, &str) = (Code, "code ");

    fn code_label(code: &CodeRef) -> String {
        match code {
            CodeRef::Known(key) => key.title().to_owned(),
            CodeRef::Unknown(title) => format!("?{title}"),
            CodeRef::Anaphora(a) => format!("~{a:?}"),
            CodeRef::Missing => "-".to_owned(),
        }
    }

    fn version_label(version: &VersionQualifier) -> String {
        match version {
            VersionQualifier::EnVigueurAu(d) => format!("EnVigueurAu({d})"),
            VersionQualifier::RedactionEnVigueur(d) => {
                format!(
                    "RedactionEnVigueur({})",
                    d.map_or_else(|| "-".to_owned(), |d| d.to_string())
                )
            }
            VersionQualifier::RedactionIssueDe(t) => {
                format!("RedactionIssueDe({} {})", t.nature.code(), number_raw(t))
            }
            VersionQualifier::RedactionAnterieureA(t) => {
                format!("RedactionAnterieureA({} {})", t.nature.code(), number_raw(t))
            }
            other => format!("{other:?}"),
        }
    }

    fn number_raw(t: &crate::citation::types::TextCite) -> String {
        t.number.as_ref().map_or_else(String::new, |n| n.raw.clone())
    }

    /// Une ligne par candidat : `code|numéro`, puis subdivisions, fin de plage, version,
    /// confiance, ambiguïté, drapeaux et appartenance à une énumération.
    fn show(c: &Candidate) -> String {
        let CitationRef::CodeArticle { code, article, version, code_via } = &c.value else {
            return format!("autre : {:?}", c.value);
        };
        let mut s = format!("{}|{}", code_label(code), article.num.num_key());
        if !article.sub.is_empty() {
            let _ = write!(s, " sub={:?}", article.sub);
        }
        if let Some(to) = &article.range_to {
            let _ = write!(s, " to={}", to.num_key());
        }
        if let Some(v) = version {
            let _ = write!(s, " ver={}", version_label(v));
        }
        assert_eq!(*code_via, None, "le code_via est posé par l'arbitrage");
        s + &tail(c)
    }

    fn run(input: &str, spec: &[(AnchorClass, &str)]) -> Vec<(String, String)> {
        let n = norm(input);
        let found = scan(&n, &anchors(&n, spec));
        spans(input, &found).into_iter().map(str::to_owned).zip(found.iter().map(show)).collect()
    }

    /// Chaque ligne : entrée, ancres, citations attendues (texte d'origine couvert, description).
    type Row<'a> = (&'a str, &'a [(AnchorClass, &'a str)], &'a [(&'a str, &'a str)]);

    fn check(rows: &[Row<'_>]) {
        for &(input, spec, expected) in rows {
            let want: Vec<(String, String)> =
                expected.iter().map(|&(a, b)| (a.to_owned(), b.to_owned())).collect();
            assert_eq!(run(input, spec), want, "entrée : {input:?}");
        }
    }

    #[test]
    fn articles_with_a_code_title_p001_to_p014() {
        check(&[
            (
                "au titre de l'article L. 761-1 du code de justice administrative.",
                &[ART],
                &[(
                    "article L. 761-1 du code de justice administrative",
                    "Code de justice administrative|L761-1",
                )],
            ),
            (
                "en méconnaissance des dispositions de l'article R. 741-7 du code de justice administrative, manque en fait.",
                &[ART, CODE],
                &[(
                    "article R. 741-7 du code de justice administrative",
                    "Code de justice administrative|R741-7",
                )],
            ),
            (
                "4. Aux termes de l'article L. 600-4-1 du code de l'urbanisme : \" Lorsqu'elle annule",
                &[ART],
                &[("article L. 600-4-1 du code de l'urbanisme", "Code de l'urbanisme|L600-4-1")],
            ),
            (
                "[...]et R. 104-26 du code de l\u{2019}urbanisme, selon lesquelles",
                &[(ArticlePrefix, "R. 1")],
                &[("R. 104-26 du code de l\u{2019}urbanisme", "Code de l'urbanisme|R104-26")],
            ),
            (
                "aux termes de l'article L. 104-7 du code de l'urbanisme : \" Les documents \" Aux termes de l'article R. 104-26 du même code : \" Lorsqu'un document",
                &[ART, CODE],
                &[
                    ("article L. 104-7 du code de l'urbanisme", "Code de l'urbanisme|L104-7"),
                    ("article R. 104-26 du même code", "~MemeCode|R104-26"),
                ],
            ),
            (
                "du premier alinéa de l\u{2019}article 396 du code de procédure pénale et de l\u{2019}article 397-1-1 du même code.",
                &[ART, CODE],
                &[
                    (
                        "premier alinéa de l\u{2019}article 396 du code de procédure pénale",
                        "Code de procédure pénale|396 sub=[Alinea(1)]",
                    ),
                    ("article 397-1-1 du même code", "~MemeCode|397-1-1"),
                ],
            ),
            (
                "le renvoi opéré par la première phrase du troisième alinéa de l\u{2019}article 397-1-1 du code de procédure pénale aux mots",
                &[ART],
                &[(
                    "première phrase du troisième alinéa de l\u{2019}article 397-1-1 du code de procédure pénale",
                    "Code de procédure pénale|397-1-1 sub=[Alinea(3), Phrase(1)]",
                )],
            ),
            (
                "les dispositions du deuxième alinéa de l\u{2019}article 141-2 et de l\u{2019}article 141-4 sont applicables",
                &[ART],
                &[
                    (
                        "deuxième alinéa de l\u{2019}article 141-2",
                        "-|141-2 sub=[Alinea(2)] amb=texte_manquant",
                    ),
                    ("article 141-4", "-|141-4 amb=texte_manquant"),
                ],
            ),
            (
                "qu'aux termes de l'article 2270-1 ancien du code civil, les actions",
                &[ART],
                &[("article 2270-1 ancien du code civil", "Code civil|2270-1 ver=Ancien")],
            ),
            (
                "La limitation de responsabilité du manutentionnaire prévue à l'article L. 5422-23 du code des transports ne s'applique",
                &[ART],
                &[("article L. 5422-23 du code des transports", "Code des transports|L5422-23")],
            ),
            (
                "Il résulte des articles L. 5422-13 et L. 5422-23 du code des transports",
                &[ART],
                &[
                    ("L. 5422-13", "Code des transports|L5422-13 grp"),
                    ("L. 5422-23", "Code des transports|L5422-23 grp"),
                ],
            ),
            (
                "aux termes de l'article L. 123-6 du code de l'urbanisme dans sa rédaction applicable au litige :",
                &[ART],
                &[(
                    "article L. 123-6 du code de l'urbanisme dans sa rédaction applicable au litige",
                    "Code de l'urbanisme|L123-6 ver=RedactionApplicableAuLitige",
                )],
            ),
            (
                "l'article L. 36-11 du code des postes et des communications électroniques",
                &[ART],
                &[(
                    "article L. 36-11 du code des postes et des communications électroniques",
                    "Code des postes et des communications électroniques|L36-11",
                )],
            ),
        ]);
    }

    #[test]
    fn code_first_and_abbreviated_forms_p015_to_p019_p037_p039_to_p048() {
        check(&[
            ("Code civil, art. 9", &[CODE, ART_DOT], &[("Code civil, art. 9", "Code civil|9")]),
            ("C. civ., art. 1240", &[ART_DOT], &[("C. civ., art. 1240", "Code civil|1240")]),
            ("(art. 1240 C. civ.)", &[ART_DOT], &[("art. 1240 C. civ.", "Code civil|1240")]),
            (
                "Selon l'art. 1240 du Code civil, tout fait",
                &[ART_DOT],
                &[("art. 1240 du Code civil", "Code civil|1240")],
            ),
            (
                "art. L.1234-5 C. trav.",
                &[ART_DOT],
                &[("art. L.1234-5 C. trav.", "Code du travail|L1234-5")],
            ),
            (
                "en application de L. 1234-5 du code du travail",
                &[(ArticlePrefix, "L. 1")],
                &[("L. 1234-5 du code du travail", "Code du travail|L1234-5")],
            ),
            (
                "Article L1234-5 du Code du travail",
                &[ART],
                &[("Article L1234-5 du Code du travail", "Code du travail|L1234-5")],
            ),
            (
                "au titre de l'article 700 du CPC",
                &[ART],
                &[("article 700 du CPC", "Code de procédure civile|700")],
            ),
            (
                "C. pr. pén., art. 63-4",
                &[ART_DOT],
                &[("C. pr. pén., art. 63-4", "Code de procédure pénale|63-4")],
            ),
            ("art. 1240 C.civ.", &[ART_DOT], &[("art. 1240 C.civ.", "Code civil|1240")]),
            (
                "C. consom., art. L. 121-1",
                &[ART_DOT],
                &[("C. consom., art. L. 121-1", "Code de la consommation|L121-1")],
            ),
            (
                "ARTICLE L. 1234-5 DU CODE DU TRAVAIL",
                &[ART],
                &[("ARTICLE L. 1234-5 DU CODE DU TRAVAIL", "Code du travail|L1234-5")],
            ),
            (
                "article L. 242-1 du code de la sécurité sociale",
                &[ART],
                &[(
                    "article L. 242-1 du code de la sécurité sociale",
                    "Code de la sécurité sociale|L242-1",
                )],
            ),
            (
                "art. L. 511-1 CMF",
                &[ART_DOT],
                &[("art. L. 511-1 CMF", "Code monétaire et financier|L511-1 conf=Medium")],
            ),
            (
                "article 5 du Code Napoléon",
                &[ART],
                &[("article 5 du Code Napoléon", "Code civil|5 conf=Medium")],
            ),
        ]);
    }

    #[test]
    fn suffixed_numbers_keep_their_adverb_and_their_code() {
        check(&[
            (
                "article 150-0 B ter du code général des impôts",
                &[ART],
                &[(
                    "article 150-0 B ter du code général des impôts",
                    "Code général des impôts|150-0 B ter",
                )],
            ),
            (
                "CGI, art. 150-0 B ter.",
                &[ART_DOT],
                &[("CGI, art. 150-0 B ter", "Code général des impôts|150-0 B ter")],
            ),
            (
                "art. 199 terdecies-0 A du code général des impôts",
                &[ART_DOT],
                &[(
                    "art. 199 terdecies-0 A du code général des impôts",
                    "Code général des impôts|199 terdecies-0 A",
                )],
            ),
        ]);
    }

    #[test]
    fn prefixes_and_numbers_p022_to_p029_p036_p042() {
        check(&[
            (
                "l'article R. 4127-1 du code de la santé publique",
                &[ART],
                &[(
                    "article R. 4127-1 du code de la santé publique",
                    "Code de la santé publique|R4127-1",
                )],
            ),
            (
                "article R* 111-1 du code de l'urbanisme",
                &[ART],
                &[("article R* 111-1 du code de l'urbanisme", "Code de l'urbanisme|R*111-1")],
            ),
            (
                "article R** 5 du code de l'urbanisme",
                &[ART],
                &[("article R** 5 du code de l'urbanisme", "Code de l'urbanisme|R**5")],
            ),
            (
                "art. D. 1234-1 C. trav.",
                &[ART_DOT],
                &[("art. D. 1234-1 C. trav.", "Code du travail|D1234-1")],
            ),
            (
                "article D* 12 du code du travail",
                &[ART],
                &[("article D* 12 du code du travail", "Code du travail|D*12")],
            ),
            (
                "article D** 12 du code du travail",
                &[ART],
                &[("article D** 12 du code du travail", "Code du travail|D**12")],
            ),
            (
                "article A. 422-1 du code des assurances",
                &[ART],
                &[("article A. 422-1 du code des assurances", "Code des assurances|A422-1")],
            ),
            (
                "l'article LO. 141 du code électoral",
                &[ART],
                &[("article LO. 141 du code électoral", "Code électoral|LO141")],
            ),
            (
                "c'est le cas, par exemple, de l'article 39 octies du code général des impôts.",
                &[ART],
                &[(
                    "article 39 octies du code général des impôts",
                    "Code général des impôts|39 octies",
                )],
            ),
            (
                "art. 199 undecies B du CGI",
                &[ART_DOT],
                &[("art. 199 undecies B du CGI", "Code général des impôts|199 undecies B")],
            ),
            (
                "article 238 bis CGI",
                &[ART],
                &[("article 238 bis CGI", "Code général des impôts|238 bis")],
            ),
            (
                "l'article 1er du code civil",
                &[ART],
                &[("article 1er du code civil", "Code civil|1er")],
            ),
            (
                "article L.\u{a0}1234\u{2011}5 du code du travail",
                &[ART],
                &[("article L.\u{a0}1234\u{2011}5 du code du travail", "Code du travail|L1234-5")],
            ),
            (
                "article unique du code civil",
                &[ART],
                &[("article unique du code civil", "Code civil|unique")],
            ),
            (
                "article 5 B du code civil",
                &[ART],
                &[("article 5 B du code civil", "Code civil|5 B")],
            ),
        ]);
    }

    #[test]
    fn subdivisions_p031_p038_p040() {
        check(&[
            (
                "Article L. 721-3, 2°, du code de commerce.",
                &[ART],
                &[(
                    "Article L. 721-3, 2°, du code de commerce",
                    "Code de commerce|L721-3 sub=[Item(2)]",
                )],
            ),
            (
                "article 6, al. 2, du code civil",
                &[ART],
                &[("article 6, al. 2, du code civil", "Code civil|6 sub=[Alinea(2)]")],
            ),
            (
                "le 3° du I de l'article L. 1111-2 du CGCT",
                &[ART],
                &[(
                    "3° du I de l'article L. 1111-2 du CGCT",
                    "Code général des collectivités territoriales|L1111-2 sub=[Paragraphe(\"I\"), Item(3)]",
                )],
            ),
            (
                "article 6 alinéa 2 du code civil",
                &[ART],
                &[("article 6 alinéa 2 du code civil", "Code civil|6 sub=[Alinea(2)]")],
            ),
            (
                "article 6, alinéas 2 du code civil",
                &[ART],
                &[("article 6, alinéas 2 du code civil", "Code civil|6 sub=[Alinea(2)]")],
            ),
            (
                "article 6, II, a), du code civil",
                &[ART],
                &[(
                    "article 6, II, a), du code civil",
                    "Code civil|6 sub=[Paragraphe(\"II\"), Lettre('a')]",
                )],
            ),
            (
                "article 6, § 3 du code civil",
                &[ART],
                &[("article 6, § 3 du code civil", "Code civil|6 sub=[Paragraphe(\"3\")]")],
            ),
            (
                "le dernier alinéa de l'article 6 du code civil",
                &[ART],
                &[(
                    "dernier alinéa de l'article 6 du code civil",
                    "Code civil|6 sub=[DernierAlinea]",
                )],
            ),
            (
                "la dernière phrase du dernier alinéa de l'article 6 du code civil",
                &[ART],
                &[(
                    "dernière phrase du dernier alinéa de l'article 6 du code civil",
                    "Code civil|6 sub=[DernierAlinea, DernierePhrase]",
                )],
            ),
            (
                "du 2° de l'article 6 du code civil",
                &[ART],
                &[("2° de l'article 6 du code civil", "Code civil|6 sub=[Item(2)]")],
            ),
        ]);
    }

    #[test]
    fn enumerations_and_ranges_p033_p034_p035() {
        check(&[
            (
                "au sens des articles 56 et 57 du code de procédure pénale, l'inventaire sommaire réalisé par un agent de police judiciaire, en application de l'article 20 dudit code,",
                &[ART, CODE],
                &[
                    ("56", "Code de procédure pénale|56 grp"),
                    ("57", "Code de procédure pénale|57 grp"),
                    ("article 20 dudit code", "~Dudit|20"),
                ],
            ),
            (
                "Vu les articles L. 122-4, L. 122-14 et L. 122-14-3 du Code du travail ;",
                &[ART],
                &[
                    ("L. 122-4", "Code du travail|L122-4 grp"),
                    ("L. 122-14", "Code du travail|L122-14 grp"),
                    ("L. 122-14-3", "Code du travail|L122-14-3 grp"),
                ],
            ),
            (
                "articles 1100 à 1100-2 du code civil",
                &[ART],
                &[("articles 1100 à 1100-2 du code civil", "Code civil|1100 to=1100-2")],
            ),
            (
                "articles 5 ou 6 du code civil",
                &[ART],
                &[("5", "Code civil|5 grp"), ("6", "Code civil|6 grp")],
            ),
            (
                "articles 5, 6, et 7 du code civil",
                &[ART],
                &[("5", "Code civil|5 grp"), ("6", "Code civil|6 grp"), ("7", "Code civil|7 grp")],
            ),
            (
                "articles 5, al. 2 et 6 du code civil",
                &[ART],
                &[("5", "Code civil|5 sub=[Alinea(2)] grp"), ("6", "Code civil|6 grp")],
            ),
            (
                "articles 5 et 6 du même code",
                &[ART],
                &[("5", "~MemeCode|5 grp"), ("6", "~MemeCode|6 grp")],
            ),
            (
                "le 2° de l'article 5 et de l'article 6 du code civil",
                &[ART],
                &[
                    ("2° de l'article 5", "-|5 sub=[Item(2)] amb=texte_manquant"),
                    ("article 6 du code civil", "Code civil|6"),
                ],
            ),
        ]);
    }

    /// Étendue et identifiant du groupe de chaque candidat, en texte d'origine.
    fn extents(input: &str, spec: &[(AnchorClass, &str)]) -> Vec<(Option<String>, Option<u32>)> {
        let n = norm(input);
        scan(&n, &anchors(&n, spec))
            .iter()
            .map(|c| {
                (c.flags.group_extent.clone().map(|e| input[e].to_owned()), c.flags.enum_group)
            })
            .collect()
    }

    #[test]
    fn an_enumeration_shares_one_group_that_ends_with_the_code() {
        let group = "articles L. 122-4, L. 122-14 et L. 122-14-3 du Code du travail".to_owned();
        assert_eq!(
            extents(
                "Vu les articles L. 122-4, L. 122-14 et L. 122-14-3 du Code du travail ;",
                &[ART]
            ),
            vec![(Some(group), Some(7)); 3]
        );
        // L'identifiant est l'offset d'origine du début du groupe, pas celui du texte normalisé.
        assert_eq!(
            extents("\u{a0}articles 5 et 6 du code civil", &[ART]),
            vec![(Some("articles 5 et 6 du code civil".to_owned()), Some(2)); 2]
        );
        // Deux groupes ont deux identifiants ; une citation seule n'en a pas.
        assert_eq!(
            extents("articles 5 et 6 du code civil puis articles 7 et 8 du code civil", &[ART]),
            vec![
                (Some("articles 5 et 6 du code civil".to_owned()), Some(0)),
                (Some("articles 5 et 6 du code civil".to_owned()), Some(0)),
                (Some("articles 7 et 8 du code civil".to_owned()), Some(35)),
                (Some("articles 7 et 8 du code civil".to_owned()), Some(35)),
            ]
        );
        assert_eq!(extents("article 5 du code civil", &[ART]), vec![(None, None)]);
    }

    #[test]
    fn the_group_extent_excludes_the_version_qualifier() {
        let input = "articles 5 et 6 du code civil, dans leur rédaction applicable au litige";
        assert_eq!(
            extents(input, &[ART]),
            vec![(Some("articles 5 et 6 du code civil".to_owned()), Some(0)); 2]
        );
        check(&[(
            input,
            &[ART],
            &[
                ("5", "Code civil|5 ver=RedactionApplicableAuLitige grp"),
                ("6", "Code civil|6 ver=RedactionApplicableAuLitige grp"),
            ],
        )]);
    }

    #[test]
    fn anaphoras_name_the_last_code_without_resolving_it() {
        check(&[
            (
                "article 5 du présent code",
                &[ART],
                &[("article 5 du présent code", "~PresentCode|5")],
            ),
            ("article 5 de ce code", &[ART], &[("article 5 de ce code", "~CeCode|5")]),
            ("article 5 ledit code", &[ART], &[("article 5 ledit code", "~Dudit|5")]),
            ("article 5 dudit code", &[ART], &[("article 5 dudit code", "~Dudit|5")]),
            (
                "l'article L. 821-5-2 du code de commerce est devenu l'article L. 821-5-3 du même code",
                &[ART, CODE],
                &[
                    ("article L. 821-5-2 du code de commerce", "Code de commerce|L821-5-2"),
                    ("article L. 821-5-3 du même code", "~MemeCode|L821-5-3"),
                ],
            ),
        ]);
    }

    #[test]
    fn version_qualifiers_stay_in_the_span() {
        check(&[
            (
                "article 5 du code civil, en vigueur au 1er janvier 2018 ;",
                &[ART],
                &[(
                    "article 5 du code civil, en vigueur au 1er janvier 2018",
                    "Code civil|5 ver=EnVigueurAu(2018-01-01)",
                )],
            ),
            (
                "article 5 du code civil dans sa rédaction applicable à la date des faits",
                &[ART],
                &[(
                    "article 5 du code civil dans sa rédaction applicable à la date des faits",
                    "Code civil|5 ver=RedactionApplicableDateDesFaits",
                )],
            ),
            (
                "article 5 du code civil, dans sa rédaction en vigueur.",
                &[ART],
                &[(
                    "article 5 du code civil, dans sa rédaction en vigueur",
                    "Code civil|5 ver=RedactionEnVigueur(-)",
                )],
            ),
            (
                "article 5 du code civil dans sa rédaction en vigueur le 1er mars 2020",
                &[ART],
                &[(
                    "article 5 du code civil dans sa rédaction en vigueur le 1er mars 2020",
                    "Code civil|5 ver=RedactionEnVigueur(2020-03-01)",
                )],
            ),
            (
                "article 5 nouveau du code civil",
                &[ART],
                &[("article 5 nouveau du code civil", "Code civil|5 ver=Nouveau")],
            ),
            (
                "article 5 (ancien) du code civil",
                &[ART],
                &[("article 5 (ancien) du code civil", "Code civil|5 ver=Ancien")],
            ),
            // Une date impossible ou un texte absent n'est pas un qualificatif.
            (
                "article 5 du code civil, en vigueur au 31 février 2020",
                &[ART],
                &[("article 5 du code civil", "Code civil|5")],
            ),
            (
                "article 5 du code civil dans sa rédaction issue de la loi",
                &[ART],
                &[("article 5 du code civil", "Code civil|5")],
            ),
            (
                "article 5 du code civil, en vigueur au demain",
                &[ART],
                &[("article 5 du code civil", "Code civil|5")],
            ),
            (
                "article 5 du code civil dans sa rédaction",
                &[ART],
                &[("article 5 du code civil", "Code civil|5")],
            ),
        ]);
    }

    #[test]
    fn other_families_are_reported_as_such() {
        assert!(show(&foreign()).starts_with("autre : Unsupported"));
    }

    #[test]
    fn version_qualifiers_citing_a_text_p045() {
        check(&[
            (
                "Il résulte des dispositions des articles 1244 et 1253 du code civil, dans leur rédaction antérieure à celle issue de l'ordonnance n° 2016-131 du 10 février 2016, que",
                &[ART],
                &[
                    ("1244", "Code civil|1244 ver=RedactionAnterieureA(ORDONNANCE 2016-131) grp"),
                    ("1253", "Code civil|1253 ver=RedactionAnterieureA(ORDONNANCE 2016-131) grp"),
                ],
            ),
            (
                "article 5 du code civil dans sa rédaction issue de l'ordonnance n° 2016-131 du 10 février 2016, que",
                &[ART],
                &[(
                    "article 5 du code civil dans sa rédaction issue de l'ordonnance n° 2016-131 du 10 février 2016",
                    "Code civil|5 ver=RedactionIssueDe(ORDONNANCE 2016-131)",
                )],
            ),
            (
                "article 5 du code civil, dans sa rédaction résultant de la loi n° 2019-222 du 23 mars 2019 de programmation",
                &[ART],
                &[(
                    "article 5 du code civil, dans sa rédaction résultant de la loi n° 2019-222 du 23 mars 2019",
                    "Code civil|5 ver=RedactionIssueDe(LOI 2019-222)",
                )],
            ),
            (
                "article 5 du code civil dans sa rédaction issue du décret n° 2020-1",
                &[ART],
                &[(
                    "article 5 du code civil dans sa rédaction issue du décret n° 2020-1",
                    "Code civil|5 ver=RedactionIssueDe(DECRET 2020-1)",
                )],
            ),
            (
                "article 5 du code civil dans sa rédaction antérieure au décret n° 2020-1",
                &[ART],
                &[(
                    "article 5 du code civil dans sa rédaction antérieure au décret n° 2020-1",
                    "Code civil|5 ver=RedactionAnterieureA(DECRET 2020-1)",
                )],
            ),
            (
                "article 5 du code civil, dans sa rédaction actuelle",
                &[ART],
                &[("article 5 du code civil", "Code civil|5")],
            ),
            (
                "article 5 du code civil dans sa rédaction antérieure à la loi Macron",
                &[ART],
                &[(
                    "article 5 du code civil dans sa rédaction antérieure à la loi Macron",
                    "Code civil|5 ver=RedactionAnterieureA(LOI )",
                )],
            ),
        ]);
    }

    #[test]
    fn missing_ambiguous_and_implicit_codes_n02_n03_n04_n05_n26_n41() {
        check(&[
            (
                "Le préavis est fixé par L. 1234-5.",
                &[(ArticlePrefix, "L. 1")],
                &[("L. 1234-5", "-|L1234-5 amb=code_manquant")],
            ),
            ("conformément à l'article 12", &[ART], &[("article 12", "-|12 amb=texte_manquant")]),
            (
                "art. L. 52-8 CE",
                &[ART_DOT],
                &[("art. L. 52-8 CE", "-|L52-8 conf=Low amb=sigle_ambigu:CE")],
            ),
            (
                "article 121-3 CP",
                &[ART],
                &[("article 121-3 CP", "Code pénal|121-3 conf=Low amb=sigle_ambigu:CP")],
            ),
            ("l'art. 1er", &[ART_DOT], &[("art. 1er", "-|1er amb=texte_manquant")]),
            (
                "aux consultations prévues à l'article R. 132-5.",
                &[ART],
                &[("article R. 132-5", "-|R132-5 amb=texte_manquant")],
            ),
            // Un paragraphe « § » sans numéro n'est pas une subdivision.
            ("article 6, § x du code civil", &[ART], &[("article 6", "-|6 amb=texte_manquant")]),
            // Un numéro préfixé sans tiret n'est une citation qu'avec un code ou un mot-clé.
            ("par L. 12 seulement", &[(ArticlePrefix, "L. 1")], &[]),
            (
                "Le préavis est fixé par R. 1234-5-1 ",
                &[(ArticlePrefix, "R. 1")],
                &[("R. 1234-5-1", "-|R1234-5-1 amb=code_manquant")],
            ),
        ]);
    }

    #[test]
    fn ambiguous_sigles_follow_annex_b_5_3() {
        check(&[
            (
                "article 5 CC",
                &[ART],
                &[("article 5 CC", "Code civil|5 conf=Low amb=sigle_ambigu:CC")],
            ),
            (
                "article 5 CT",
                &[ART],
                &[("article 5 CT", "Code du travail|5 conf=Low amb=sigle_ambigu:CT")],
            ),
            (
                "article 5 CU",
                &[ART],
                &[("article 5 CU", "Code de l'urbanisme|5 conf=Low amb=sigle_ambigu:CU")],
            ),
            (
                "article 5 CPI",
                &[ART],
                &[(
                    "article 5 CPI",
                    "Code de la propriété intellectuelle|5 conf=Low amb=sigle_ambigu:CPI",
                )],
            ),
            (
                "article 5 CCP",
                &[ART],
                &[(
                    "article 5 CCP",
                    "Code de la commande publique|5 conf=Low amb=sigle_ambigu:CCP",
                )],
            ),
            ("article 5 CPCE", &[ART], &[("article 5 CPCE", "-|5 conf=Low amb=sigle_ambigu:CPCE")]),
            // Un sigle en minuscules, ou plus long, n'est pas un sigle ambigu.
            ("article 5 cp", &[ART], &[("article 5", "-|5 amb=texte_manquant")]),
            ("article 5 CPX", &[ART], &[("article 5", "-|5 amb=texte_manquant")]),
        ]);
    }

    #[test]
    fn code_titles_outside_the_dictionary_and_code_variants() {
        check(&[
            (
                "l'article 3 du code de la voirie communale.",
                &[ART],
                &[(
                    "article 3 du code de la voirie communale",
                    "?code de la voirie communale|3 conf=Medium",
                )],
            ),
            (
                "article 3 du code des marchés publics dans sa rédaction applicable au litige",
                &[ART],
                &[(
                    "article 3 du code des marchés publics dans sa rédaction applicable au litige",
                    "?code des marchés publics|3 ver=RedactionApplicableAuLitige conf=Medium",
                )],
            ),
            (
                "article 3 du code de l'ancien régime, applicable",
                &[ART],
                &[(
                    "article 3 du code de l'ancien régime",
                    "?code de l'ancien régime|3 conf=Medium",
                )],
            ),
            (
                "article 3 du code de a b c d e f g h i j",
                &[ART],
                &[(
                    "article 3 du code de a b c d e f g h",
                    "?code de a b c d e f g h|3 conf=Medium",
                )],
            ),
            ("article 3 du code de", &[ART], &[("article 3", "-|3 amb=texte_manquant")]),
            ("article 3 du code civique", &[ART], &[("article 3", "-|3 amb=texte_manquant")]),
            (
                "article L. 111-1 du code rural",
                &[ART],
                &[(
                    "article L. 111-1 du code rural",
                    "Code rural et de la pêche maritime|L111-1 conf=Medium amb=code_a_variantes",
                )],
            ),
            (
                "article L. 111-1 du code minier",
                &[ART],
                &[(
                    "article L. 111-1 du code minier",
                    "Code minier (nouveau)|L111-1 conf=Medium amb=code_a_variantes",
                )],
            ),
            (
                "article L. 111-1 du code minier (ancien)",
                &[ART],
                &[("article L. 111-1 du code minier (ancien)", "Code minier (ancien)|L111-1")],
            ),
            (
                "article L. 111-1 du code forestier",
                &[ART],
                &[("article L. 111-1 du code forestier", "Code forestier (nouveau)|L111-1")],
            ),
        ]);
    }

    #[test]
    fn rejected_inputs_n01_n33_n36_n40() {
        check(&[
            ("1 du code civil.", &[CODE], &[]),
            ("note sous D. 2016, p. 1234", &[(ArticlePrefix, "D. 2")], &[]),
            ("art. L. 1234-5a C. trav.", &[ART_DOT], &[]),
            ("art. 2270- du code civil", &[ART_DOT], &[]),
            ("art. 2° du code civil", &[ART_DOT], &[]),
            ("classement en zone 1AU du secteur", &[(ArticlePrefix, "AU")], &[]),
            ("article du code civil", &[ART], &[]),
            ("article", &[ART], &[]),
            ("", &[ART], &[]),
        ]);
        // Sans ancre de ses classes, la famille ne lit rien.
        let n = norm("article 5 du code civil");
        assert_eq!(scan(&n, &[]), Vec::new());
        assert_eq!(scan(&n, &anchors(&n, &[(AnchorClass::Court, "article")])), Vec::new());
    }

    #[test]
    fn a_reading_covers_the_anchors_it_contains() {
        // Les ancres « article » et « code » appartiennent à la même lecture : un seul candidat.
        assert_eq!(
            run("article 5 du code civil", &[ART, CODE, (Code, "civil")]),
            vec![("article 5 du code civil".to_owned(), "Code civil|5".to_owned())]
        );
    }

    #[test]
    fn read_positions_are_word_starts_of_a_bounded_window() {
        let text = "aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp X";
        let anchor = text.len() - 1;
        let starts = start_positions(text, anchor);
        assert_eq!(starts.len(), 13);
        assert_eq!(text.get(starts[0]..starts[0] + 2), Some("ee"));
        assert_eq!(starts.last(), Some(&anchor));
        // Quatre-vingts caractères avant l'ancre, pas plus.
        let wide = format!("{}Z", format!("{} ", "a".repeat(19)).repeat(6));
        assert_eq!(start_positions(&wide, 120), vec![40, 60, 80, 100, 120]);
        // Aucun mot avant l'ancre.
        assert_eq!(start_positions("ancre", 0), vec![0]);
        // Quatre cents caractères après l'ancre, sans couper un mot en deux.
        assert_eq!(window_end(&"a ".repeat(300), 11), 411);
        assert_eq!(window_end("court", 2), 5);
        assert_eq!(window_end(&"é ".repeat(300), 2), 602);
        let digits = format!("{}{}", " ".repeat(399), "1234 5");
        assert_eq!(window_end(&digits, 0), 403);
        // La fin du mot coupé est lue, dans la limite de 32 caractères.
        assert_eq!(window_end(&"a".repeat(1000), 0), 432);
    }

    #[test]
    fn char_helpers_respect_boundaries() {
        assert_eq!(next_char("aé", 1), Some('é'));
        assert_eq!(next_char("aé", 2), None);
        assert_eq!(next_char("a", 1), None);
        assert_eq!(prev_char("aé", 3), Some('é'));
        assert_eq!(prev_char("aé", 2), None);
        assert_eq!(prev_char("a", 0), None);
        assert_eq!(consume("a(", 1, '('), Some(2));
        assert_eq!(consume("a(", 0, '('), None);
        assert!(boundary("ab", 2));
        assert!(!boundary("ab", 1));
        assert!(boundary("a-", 1));
        assert!(!boundary("a1", 1));
        assert!(left_boundary("a b", 2));
        assert!(!left_boundary("ab", 1));
        assert!(left_boundary("a", 0));
    }

    #[test]
    fn whitespace_and_keyword_helpers() {
        assert_eq!(ws0("a  b", 1), 3);
        assert_eq!(ws0("a", 9), 9);
        assert_eq!(ws("a  b", 1), Some(3));
        assert_eq!(ws("ab", 1), None);
        assert_eq!(kw("Article  12", 0, "article "), Some(9));
        assert_eq!(kw("ÉTÉ", 0, "ete"), Some("ÉTÉ".len()));
        assert_eq!(kw("Etat", 0, "ete"), None);
        assert_eq!(word("loi n", 0, "loi"), Some(3));
        assert_eq!(word("loix", 0, "loi"), None);
        assert_eq!(abbr("art. 5", 0, "art"), Some(4));
        assert_eq!(abbr("art 5", 0, "art"), Some(3));
        assert_eq!(abbr("arte", 0, "art"), None);
        assert_eq!(longest("articles 5", 0, &["article", "articles", "arts"]), Some((1, 8)));
        assert_eq!(longest("autre", 0, &["article"]), None);
    }

    #[test]
    fn integers_are_bounded() {
        assert_eq!(uint("12ab", 0), Some((12, 2)));
        assert_eq!(uint("123456789", 0), Some((123_456_789, 9)));
        assert_eq!(uint("1234567890", 0), None);
        assert_eq!(uint("ab", 0), None);
        assert_eq!(uint("a7", 1), Some((7, 2)));
        assert_eq!(uint("", 0), None);
        assert_eq!(small("12", 0, 31), Some((12, 2)));
        assert_eq!(small("05", 0, 31), Some((5, 2)));
        assert_eq!(small("012", 0, 31), None);
        assert_eq!(small("0", 0, 31), None);
        assert_eq!(small("32", 0, 31), None);
        assert_eq!(two_digits("05", 0, 12), Some((5, 2)));
        assert_eq!(two_digits("5", 0, 12), None);
        assert_eq!(year4("2018 ", 0), Some((2018, 4)));
        assert_eq!(year4("1499", 0), None);
        assert_eq!(year4("2100", 0), None);
        assert_eq!(year4("20180", 0), None);
        assert_eq!(year4("201", 0), None);
    }

    #[test]
    fn separators_and_links() {
        assert_eq!(sep("a , b", 1), Some(4));
        assert_eq!(sep("a;b", 1), Some(2));
        assert_eq!(sep("a:b", 1), Some(2));
        assert_eq!(sep("a b", 1), Some(2));
        assert_eq!(sep("ab", 1), None);
        assert_eq!(link("de l'article", 0, false), Some(5));
        assert_eq!(link("de la loi", 0, false), Some(6));
        assert_eq!(link("du code", 0, false), Some(3));
        assert_eq!(link("des codes", 0, false), Some(4));
        assert_eq!(link("de code", 0, false), None);
        assert_eq!(link("de code", 0, true), Some(3));
        assert_eq!(link("dudit", 0, false), None);
    }

    /// Date lue en tête de `text` : ISO, drapeaux, fin.
    fn date_of(text: &str) -> Option<(Option<String>, Vec<String>, usize)> {
        date(text, 0).map(|hit| {
            (
                hit.date.map(|d| d.to_string()),
                hit.flags.iter().map(|f| f.code().into_owned()).collect(),
                hit.end,
            )
        })
    }

    fn valid(iso: &str, end: usize) -> (Option<String>, Vec<String>, usize) {
        (Some(iso.to_owned()), Vec::new(), end)
    }

    #[test]
    fn dates_in_every_written_form() {
        assert_eq!(date_of("1er janvier 2018"), Some(valid("2018-01-01", 16)));
        assert_eq!(date_of("premier mars 2020 x"), Some(valid("2020-03-01", 17)));
        assert_eq!(date_of("7 octobre 2016"), Some(valid("2016-10-07", 14)));
        assert_eq!(date_of("07 novembre 2025"), Some(valid("2025-11-07", 16)));
        assert_eq!(date_of("4 JUIN 2025"), Some(valid("2025-06-04", 11)));
        assert_eq!(date_of("7 nov. 2025"), Some(valid("2025-11-07", 11)));
        assert_eq!(date_of("3 sept 2020"), Some(valid("2020-09-03", 11)));
        assert_eq!(date_of("3 juill. 2020"), Some(valid("2020-07-03", 13)));
        assert_eq!(date_of("3 juil. 2020"), Some(valid("2020-07-03", 12)));
        assert_eq!(date_of("3 févr. 2020"), Some(valid("2020-02-03", "3 févr. 2020".len())));
        assert_eq!(date_of("3 août 2020"), Some(valid("2020-08-03", "3 août 2020".len())));
        assert_eq!(date_of("29/05/2026"), Some(valid("2026-05-29", 10)));
        assert_eq!(date_of("29.05.2026"), Some(valid("2026-05-29", 10)));
        assert_eq!(date_of("1/2/2026"), Some(valid("2026-02-01", 8)));
        assert_eq!(date_of("2026-04-30"), Some(valid("2026-04-30", 10)));
        let impossible = |end| Some((None, vec!["date_invalide".to_owned()], end));
        assert_eq!(date_of("31 février 2020"), impossible("31 février 2020".len()));
        assert_eq!(date_of("2026-04-31"), impossible(10));
        assert_eq!(date_of("31/06/2024"), impossible(10));
        let republican = date("4 messidor an II", 0).expect("date républicaine");
        assert_eq!(republican.date, None);
        assert_eq!(republican.raw.as_deref(), Some("4 messidor an II"));
        assert_eq!(republican.flags, vec![Flag::DateRepublicaine]);
        assert_eq!(republican.end, 16);
        for not_a_date in [
            "15 mai 3000",
            "00/05/2026",
            "7 octobre",
            "7 xyz 2016",
            "32 mai 2016",
            "2026-4-30",
            "2026-04-3",
            "2026-04-300",
            "4 messidor an",
            "4 messidor II",
            "29/13/2026",
            "29/05 2026",
            "29/05/99",
            "2026",
            "2026-04",
            "2026-04 30",
            "4",
            "4 messidor",
            "4 messidor an xyz",
        ] {
            assert_eq!(date_of(not_a_date), None, "{not_a_date}");
        }
    }

    #[test]
    fn dates_with_a_link_word() {
        for (text, end) in [
            ("du 7 octobre 2016", 17),
            ("en date du 7 octobre 2016", 25),
            ("le 7 octobre 2016", 17),
            ("des 7 octobre 2016", 18),
            ("7 octobre 2016", 14),
        ] {
            let hit = date_link(text, 0).expect(text);
            assert_eq!(
                (hit.date.map(|d| d.to_string()), hit.end),
                (Some("2016-10-07".to_owned()), end)
            );
        }
        assert!(date_link("du abc", 0).is_none());
        assert!(date_link("", 0).is_none());
    }

    #[test]
    fn ordinals_romans_and_special_numbers() {
        for (text, rank) in [
            ("premier", Ordinal::Nth(1)),
            ("première", Ordinal::Nth(1)),
            ("1ère", Ordinal::Nth(1)),
            ("1re", Ordinal::Nth(1)),
            ("1er", Ordinal::Nth(1)),
            ("seconde", Ordinal::Nth(2)),
            ("troisième", Ordinal::Nth(3)),
            ("douzième", Ordinal::Nth(12)),
            ("dernier", Ordinal::Last),
            ("dernière", Ordinal::Last),
            ("2e", Ordinal::Nth(2)),
            ("3ème", Ordinal::Nth(3)),
            ("10e", Ordinal::Nth(10)),
        ] {
            assert_eq!(ordinal(text, 0), Some((rank, text.len())), "{text}");
        }
        for not_an_ordinal in ["1e", "2ex", "100e", "alinéa", "e"] {
            assert_eq!(ordinal(not_an_ordinal, 0), None, "{not_an_ordinal}");
        }
        assert_eq!(roman("XIV x", 0), Some(("XIV".to_owned(), 3)));
        assert_eq!(roman("V", 0), Some(("V".to_owned(), 1)));
        assert_eq!(roman("XXXIX.", 0), Some(("XXXIX".to_owned(), 5)));
        for not_a_roman in ["XXXX", "IIII", "XL", "Ix", "", "VV", "ivx"] {
            assert_eq!(roman(not_a_roman, 0), None, "{not_a_roman}");
        }
    }

    fn num_of(text: &str) -> Option<(String, usize)> {
        art_num(text, 0).map(|(num, end)| (num.num_key(), end))
    }

    fn key(key: &str, end: usize) -> (String, usize) {
        (key.to_owned(), end)
    }

    #[test]
    fn article_numbers() {
        assert_eq!(num_of("L. 761-1 du"), Some(key("L761-1", 8)));
        assert_eq!(num_of("L.761-1"), Some(key("L761-1", 7)));
        assert_eq!(num_of("R*111-1"), Some(key("R*111-1", 7)));
        assert_eq!(num_of("LO 141,"), Some(key("LO141", 6)));
        assert_eq!(num_of("1er"), Some(key("1er", 3)));
        assert_eq!(num_of("premier"), Some(key("1er", 7)));
        assert_eq!(num_of("unique"), Some(key("unique", 6)));
        assert_eq!(num_of("liminaire"), Some(key("liminaire", 9)));
        assert_eq!(num_of("préliminaire"), Some(key("préliminaire", "préliminaire".len())));
        assert_eq!(num_of("39 octies du"), Some(key("39 octies", 9)));
        assert_eq!(num_of("199 undecies B du"), Some(key("199 undecies B", 14)));
        assert_eq!(num_of("5 B bis"), Some(key("5 B bis", 7)));
        assert_eq!(num_of("5 B C"), Some(key("5 B", 3)));
        assert_eq!(num_of("121-3 CP"), Some(key("121-3", 5)));
        assert_eq!(num_of("5 DU CODE"), Some(key("5", 1)));
        assert_eq!(num_of("5 ET 6"), Some(key("5", 1)));
        assert_eq!(num_of("5 ABC"), Some(key("5", 1)));
        assert_eq!(num_of("5 a"), Some(key("5", 1)));
        assert_eq!(num_of("5 C. civ."), Some(key("5", 1)));
        for not_a_number in ["5 bis ter", "1234-5a", "2270-", "2°", "L", "L. x", "", "uniques"] {
            assert_eq!(num_of(not_a_number), None, "{not_a_number}");
        }
    }

    #[test]
    fn article_numbers_with_a_latin_adverb_after_the_letters_or_glued_to_a_dash() {
        // L1-01 : les formes réelles du CGI (E2E resolve-3, gap3-1, gap3-6).
        let kept = [
            ("150-0 B ter du", "150-0 B ter", 11),
            ("150-0 D ter", "150-0 D ter", 11),
            ("199 terdecies-0 A du", "199 terdecies-0 A", 17),
            ("199 terdecies-0 AA", "199 terdecies-0 AA", 18),
            ("199 terdecies-0 A ter", "199 terdecies-0 A ter", 21),
            ("199 terdecies-0 A bis,", "199 terdecies-0 A bis", 21),
            ("50 terdecies-0", "50 terdecies-0", 14),
            ("383-0 ter", "383-0 ter", 9),
            ("46-0 B ter", "46-0 B ter", 10),
            ("1649 quater B quinquies", "1649 quater B quinquies", 23),
            ("1594 F quinquies", "1594 F quinquies", 16),
            ("199 undecies B", "199 undecies B", 14),
            ("5 bis A", "5 bis A", 7),
            ("5 bis-3", "5 bis-3", 7),
            ("5 bis DU CODE", "5 bis", 5),
            ("5 B E", "5 B", 3),
            ("199 undecies B E", "199 undecies B", 14),
            ("5 B bis DU CODE", "5 B bis", 7),
        ];
        for (input, expected, end) in kept {
            assert_eq!(num_of(input), Some(key(expected, end)), "{input}");
        }
        // Jamais un numéro tronqué : un adverbe ou une lettre qui ne peut plus être lu après un
        // adverbe fait renvoyer `None` (filet N36), « 885-0 V bis A » comprise.
        let refused = [
            "5 bis ter",
            "5 B bis ter",
            "5 B bis A",
            "885-0 V bis A",
            "1594 F quinquies A",
            "199 terdecies-0 ter",
            "5 bis-3-4",
            "5 B bis-3",
            "199 undecies B bis-3",
        ];
        for input in refused {
            assert_eq!(num_of(input), None, "{input}");
        }
    }

    /// Limite documentée (B §4.2, 09) : un paragraphe romain collé après un adverbe se lit comme
    /// une lettre de suffixe. Après « lettre adverbe », une seconde lettre fait refuser tout le
    /// numéro (filet N36) ; après « adverbe » seul, la lettre est prise dans la clé. Dans les deux
    /// cas rien n'est tronqué en silence : au pire la clé ne se résout pas (`not_found`).
    #[test]
    fn a_roman_paragraph_glued_after_an_adverb_is_a_documented_limit() {
        // Refusé en entier : « 150-0 B ter » seul se lit, « 150-0 B ter I » non.
        assert_eq!(num_of("150-0 B ter"), Some(key("150-0 B ter", 11)));
        assert_eq!(num_of("150-0 B ter I"), None);
        assert_eq!(num_of("150-0 B ter I du"), None);
        // Lu comme une lettre de suffixe : la clé « 200 bis I » ne se résoudra pas.
        assert_eq!(num_of("200 bis I"), Some(key("200 bis I", 9)));
        assert_eq!(num_of("5 bis A l'égard de"), Some(key("5 bis A", 7)));
        assert_eq!(num_of("5 bis A du"), Some(key("5 bis A", 7)));
    }

    #[test]
    fn article_keywords() {
        assert_eq!(art_kw("articles 5", 0), Some(9));
        assert_eq!(art_kw("article 5", 0), Some(8));
        assert_eq!(art_kw("ARTICLE  5", 0), Some(9));
        assert_eq!(art_kw("arts 5", 0), Some(5));
        assert_eq!(art_kw("art. 5", 0), Some(5));
        assert_eq!(art_kw("art.5", 0), Some(4));
        assert_eq!(art_kw("art 5", 0), Some(4));
        for not_a_keyword in ["art5", "artisan 5", "article", "articles", "arts", "art"] {
            assert_eq!(art_kw(not_a_keyword, 0), None, "{not_a_keyword}");
        }
    }

    /// Un article de liste : clé, fin de plage, texte du numéro, nombre de subdivisions.
    type Entry = (String, Option<String>, String, usize);

    /// Les articles d'une liste et la fin de la liste.
    fn list_of(text: &str) -> Option<(Vec<Entry>, usize)> {
        let list = art_list(text, 0)?;
        let items = list
            .items
            .iter()
            .map(|item| {
                (
                    item.num.num_key(),
                    item.range_to.as_ref().map(ArticleNum::num_key),
                    text[item.span.clone()].to_owned(),
                    item.subs.len(),
                )
            })
            .collect();
        Some((items, list.end))
    }

    fn entry(key: &str, to: Option<&str>, span: &str, subs: usize) -> Entry {
        (key.to_owned(), to.map(str::to_owned), span.to_owned(), subs)
    }

    #[test]
    fn article_lists_and_ranges() {
        assert_eq!(
            list_of("1100 à 1100-2 du"),
            Some((vec![entry("1100", Some("1100-2"), "1100 à 1100-2", 0)], 14))
        );
        assert_eq!(
            list_of("1100 À 1101 du"),
            Some((vec![entry("1100", Some("1101"), "1100 À 1101", 0)], 12))
        );
        assert_eq!(
            list_of("5, 6 et 7 du"),
            Some((
                vec![entry("5", None, "5", 0), entry("6", None, "6", 0), entry("7", None, "7", 0)],
                9
            ))
        );
        assert_eq!(
            list_of("5 ou 6"),
            Some((vec![entry("5", None, "5", 0), entry("6", None, "6", 0)], 6))
        );
        assert_eq!(
            list_of("5, et 6"),
            Some((vec![entry("5", None, "5", 0), entry("6", None, "6", 0)], 7))
        );
        assert_eq!(list_of("5 et du"), Some((vec![entry("5", None, "5", 0)], 1)));
        assert_eq!(list_of("5 à du"), Some((vec![entry("5", None, "5", 0)], 1)));
        assert_eq!(list_of("5 a 6"), Some((vec![entry("5", None, "5", 0)], 1)));
        assert_eq!(
            list_of("5, 2° et 6, al. 3"),
            Some((
                vec![entry("5", None, "5", 1), entry("6", None, "6", 1)],
                "5, 2° et 6, al. 3".len()
            ))
        );
        assert_eq!(list_of("5 à"), Some((vec![entry("5", None, "5", 0)], 1)));
        assert_eq!(list_of("du"), None);
    }

    #[test]
    fn inline_version_qualifiers() {
        let version = |text: &str| art_item(text, 0).map(|item| (item.version, item.end));
        assert_eq!(version("5 ancien du"), Some((Some(VersionQualifier::Ancien), 8)));
        assert_eq!(version("5 (ancien) du"), Some((Some(VersionQualifier::Ancien), 10)));
        assert_eq!(version("5 nouveau du"), Some((Some(VersionQualifier::Nouveau), 9)));
        assert_eq!(version("5 ancienne"), Some((None, 1)));
        assert_eq!(version("5 du"), Some((None, 1)));
    }

    #[test]
    fn subdivision_leads() {
        let lead = |text: &str| sub_lead(text, 0).map(|(subs, end)| (subs, text[end..].to_owned()));
        assert_eq!(
            lead("première phrase du troisième alinéa de l'article 5"),
            Some((vec![Subdivision::Phrase(1), Subdivision::Alinea(3)], "article 5".to_owned()))
        );
        assert_eq!(
            lead("3° du I de la suite"),
            Some((
                vec![Subdivision::Item(3), Subdivision::Paragraphe("I".to_owned())],
                "suite".to_owned()
            ))
        );
        assert_eq!(
            lead("dernier alinéa des articles 5"),
            Some((vec![Subdivision::DernierAlinea], "articles 5".to_owned()))
        );
        assert_eq!(lead("article 5"), None);
        assert_eq!(lead("premier alinéa article"), None);
        assert_eq!(lead("dernière phrase du article"), None);
        assert_eq!(lead("première phrase de l'article"), None);
        for incomplete in [
            "premier",
            "première",
            "première phrase",
            "première phrase du",
            "première partie du",
            "premier mot de l'article",
        ] {
            assert_eq!(lead(incomplete), None, "{incomplete}");
        }
    }

    #[test]
    fn dictionary_titles_abbreviations_and_sigles() {
        let hit = |text: &str| dict_at(text, 0).map(|h| (code_label(&h.code), h.confidence, h.end));
        assert_eq!(hit("Code civil."), Some(("Code civil".to_owned(), Confidence::High, 10)));
        assert_eq!(hit("C. civ. 5"), Some(("Code civil".to_owned(), Confidence::High, 7)));
        assert_eq!(hit("C. civ 5"), Some(("Code civil".to_owned(), Confidence::High, 6)));
        let annexe = "code général des impôts, annexe II";
        assert_eq!(
            hit(annexe),
            Some(("Code général des impôts, annexe II".to_owned(), Confidence::High, annexe.len()))
        );
        let annexe_iv = "code général des impôts, annexe IV";
        assert_eq!(
            hit(&format!("{annexe_iv} bis")),
            Some((
                "Code général des impôts, annexe IV".to_owned(),
                Confidence::High,
                annexe_iv.len()
            ))
        );
        assert_eq!(hit("CGI"), Some(("Code général des impôts".to_owned(), Confidence::High, 3)));
        assert_eq!(
            hit("CMF"),
            Some(("Code monétaire et financier".to_owned(), Confidence::Medium, 3))
        );
        for not_a_code in ["cgi", "CGIX", "codex", "C. xyz.", "", "CP", "CE"] {
            assert!(dict_at(not_a_code, 0).is_none(), "{not_a_code}");
        }
    }

    /// Briques lexicales, vérifiées directement : une valeur de retour fausse ne se voit pas
    /// toujours dans la grammaire (elle y fait boucler l'analyse au lieu d'échouer vite).
    #[test]
    fn lexical_bricks_return_the_end_of_what_they_read() {
        // `abbr` : le point facultatif est inclus, sinon une frontière de mot est exigée.
        assert_eq!(abbr("art. 5", 0, "art"), Some(4));
        assert_eq!(abbr("art 5", 0, "art"), Some(3));
        assert_eq!(abbr("x art 5", 2, "art"), Some(5));
        assert_eq!(abbr("artx", 0, "art"), None);
        assert_eq!(abbr("ar 5", 0, "art"), None);
        // `consume` : la fin compte les octets du caractère lu.
        assert_eq!(consume("a", 0, 'a'), Some(1));
        assert_eq!(consume("xa", 1, 'a'), Some(2));
        assert_eq!(consume("éa", 0, 'é'), Some(2));
        assert_eq!(consume("b", 0, 'a'), None);
        // `list_sep` : « , », « , et », « et », « ou », suivis de leurs espaces.
        assert_eq!(list_sep("1, 2", 1), Some(3));
        assert_eq!(list_sep("1 , 2", 1), Some(4));
        assert_eq!(list_sep("1, et 2", 1), Some(6));
        assert_eq!(list_sep("1 et 2", 1), Some(5));
        assert_eq!(list_sep("1 ou 2", 1), Some(5));
        assert_eq!(list_sep("1x", 1), None);
        assert_eq!(list_sep("1 etc", 1), None);
    }

    #[test]
    fn gaps_need_at_least_one_character() {
        // Ni espace, ni virgule : pas d'intervalle (`end > at`, pas `end >= at`).
        assert_eq!(gap("ab", 0), None);
        assert_eq!(gap("ab", 2), None);
        assert_eq!(gap("a b", 1), Some(2));
        assert_eq!(gap("a, b", 1), Some(3));
        assert_eq!(gap("a,b", 1), Some(2));
        assert_eq!(token_gap("ab", 0), None);
        assert_eq!(token_gap("ab", 2), None);
        assert_eq!(token_gap("a b", 1), Some(2));
        assert_eq!(token_gap("a. b", 1), Some(3));
        assert_eq!(token_gap("a.b", 1), Some(2));
        assert_eq!(token_gap("a,b", 1), Some(2));
    }

    #[test]
    fn year4_wants_exactly_four_digits_and_a_word_boundary() {
        assert_eq!(year4("2000", 0), Some((2000, 4)));
        assert_eq!(year4("x 1999 y", 2), Some((1999, 6)));
        // Quatre chiffres collés à une lettre : pas de frontière de mot.
        assert_eq!(year4("2000x", 0), None);
        // Zéros de tête : la valeur est dans la plage, mais pas la forme à quatre chiffres.
        assert_eq!(year4("02000", 0), None);
        assert_eq!(year4("02000 ", 0), None);
        // Hors de 1500 à 2099.
        assert_eq!(year4("1499", 0), None);
        assert_eq!(year4("2100", 0), None);
    }

    #[test]
    fn capital_letters_stop_at_a_word_boundary_after_the_letters() {
        assert_eq!(capital_letters("199 B", 4), Some(("B".to_owned(), 5)));
        assert_eq!(capital_letters("199 AB x", 4), Some(("AB".to_owned(), 6)));
        // Une majuscule suivie d'une minuscule débute un mot : pas une lettre de suffixe.
        assert_eq!(capital_letters("1 Ab", 2), None);
        assert_eq!(capital_letters("199 Bx", 4), None);
        // Trois majuscules ou plus, mots de liaison et sigles ambigus.
        assert_eq!(capital_letters("199 ABC", 4), None);
        assert_eq!(capital_letters("199 DE", 4), None);
        assert_eq!(capital_letters("199 CE", 4), None);
    }

    #[test]
    fn an_ambiguous_sigle_ends_where_its_capitals_end() {
        let end = |text: &str, at: usize| sigle_at(text, at).map(|hit| hit.end);
        assert_eq!(end("xx CE ", 3), Some(5));
        assert_eq!(end("xx CPI", 3), Some(6));
        assert_eq!(end("CE", 0), Some(2));
        // Collé à une lettre : pas de frontière de mot (en fin de texte, `at * len` en aurait vu une).
        assert_eq!(end("xx CEz", 3), None);
        assert_eq!(end("xx ce ", 3), None);
    }
}
