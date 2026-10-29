//! Grammaire du JORF, des NOR, des ELI et des identifiants Légifrance (annexe B §4.13), et
//! lexique commun des grammaires de V2-12 (CNIL, KALI et ACCO, puits).
//!
//! Le lexique (fenêtre de lecture, mots-clés, dates, numéros) vit ici parce que `grammar/mod.rs`
//! n'est pas à V2-12 : `cnil`, `kali` et `puits` l'importent d'ici. Il lit une fenêtre de
//! [`Normalized::text`] autour d'une ancre (annexe B §4.16 : 80 caractères avant, 400 après), sur
//! deux vues parallèles du même texte, à un caractère près : le texte, pour les littéraux
//! sensibles à la casse (`'CNIL'`), et la vue clé repliée, pour les mots-clés (`"délibération"`).
//! Les plages des candidats rendus sont des offsets du texte d'ORIGINE (`Normalized::orig_range`).

use crate::citation::normalize::{Normalized, fold_char};
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{
    Ambiguity, CitationRef, Confidence, Flag, IdPrefix, JorfCite, LegifranceIdRef,
};
use crate::contract::{Eli, Nor};
use crate::date::CivilDate;

use super::{Candidate, Flags};

/// Caractères lus avant l'ancre (annexe B §4.16).
const BEHIND: usize = 80;
/// Caractères lus après l'ancre.
const AHEAD: usize = 400;
/// Débuts de mots essayés avant l'ancre, au plus.
const MAX_STARTS: usize = 12;

/// Mois et abréviations de mois, repliés (point final facultatif quand il est écrit).
const MONTHS: [(&str, u8); 22] = [
    ("janvier", 1),
    ("janv.", 1),
    ("fevrier", 2),
    ("fevr.", 2),
    ("fev.", 2),
    ("mars", 3),
    ("avril", 4),
    ("avr.", 4),
    ("mai", 5),
    ("juin", 6),
    ("juillet", 7),
    ("juill.", 7),
    ("juil.", 7),
    ("aout", 8),
    ("septembre", 9),
    ("sept.", 9),
    ("octobre", 10),
    ("oct.", 10),
    ("novembre", 11),
    ("nov.", 11),
    ("decembre", 12),
    ("dec.", 12),
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

/// Une date lue (`DATE`, annexe B §4.1) : où elle finit et ce qu'elle vaut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DateHit {
    /// Fin de la date, en index de caractère de la fenêtre.
    pub(crate) end: usize,
    /// Date civile ; `None` si elle n'existe pas au calendrier (`31 février`) ou si elle est
    /// républicaine (gardée brute, sans conversion).
    pub(crate) date: Option<CivilDate>,
    /// Date du calendrier républicain.
    republican: bool,
}

impl DateHit {
    /// Date grégorienne : jour, mois et année lus, validés par `CivilDate::new`.
    fn civil(end: usize, year: u16, month: u8, day: u8) -> Self {
        Self { end, date: CivilDate::new(year, month, day), republican: false }
    }

    /// Drapeau que la date impose : `date_republicaine`, ou `date_invalide` si elle n'existe pas.
    pub(crate) fn flag(&self) -> Option<Flag> {
        if self.republican {
            Some(Flag::DateRepublicaine)
        } else {
            self.date.is_none().then_some(Flag::DateInvalide)
        }
    }
}

/// Un jour (ou un mois) lu en chiffres, `1..=max`.
fn ranged(value: u32, max: u8) -> Option<u8> {
    u8::try_from(value).ok().filter(|v| (1..=max).contains(v))
}

/// Ce qu'une règle de grammaire reconnaît, en index de caractère de la fenêtre.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Hit {
    /// Début du span.
    pub(crate) start: usize,
    /// Fin du span (exclue).
    pub(crate) end: usize,
    /// Valeur reconnue.
    pub(crate) value: CitationRef,
    /// Confiance de la grammaire.
    pub(crate) confidence: Confidence,
    /// Ambiguïté, une au plus.
    pub(crate) ambiguity: Option<Ambiguity>,
    /// Drapeaux de contrôle syntaxique.
    pub(crate) flags: Vec<Flag>,
}

impl Hit {
    /// Reconnaissance de confiance haute, sans ambiguïté ni drapeau.
    pub(crate) fn new(start: usize, end: usize, value: CitationRef) -> Self {
        Self { start, end, value, confidence: Confidence::High, ambiguity: None, flags: Vec::new() }
    }
}

/// Une règle : essaie la grammaire au caractère `start` de la fenêtre.
pub(crate) type Rule = fn(&Win, usize) -> Option<Hit>;

/// Fenêtre de lecture autour d'une ancre : les deux vues du texte, caractère par caractère.
pub(crate) struct Win {
    /// Texte normalisé.
    text: Vec<char>,
    /// Vue clé repliée, `fold_char` de chaque caractère de `text`.
    key: Vec<char>,
    /// Offset en octets dans `Normalized::text` de chaque caractère, plus la fin de la fenêtre.
    byte: Vec<usize>,
    /// Index du caractère où commence l'ancre.
    anchor: usize,
    /// Premier index qui peut servir de début de lecture (0, ou 1 quand la fenêtre commence par
    /// un caractère lu seulement pour savoir où commence un mot).
    floor: usize,
}

impl Win {
    /// Fenêtre `[ancre - 80, ancre + 400]` caractères ; `None` si la plage de l'ancre n'est pas
    /// une plage de `n.text` (hors du texte, ou hors d'une frontière de caractère).
    pub(crate) fn around(n: &Normalized, anchor: &Anchor) -> Option<Self> {
        let r = &anchor.range;
        n.text.get(r.clone())?;
        let behind: Vec<usize> =
            n.text[..r.start].char_indices().rev().take(BEHIND + 1).map(|(i, _)| i).collect();
        let lo = behind.last().copied().unwrap_or(r.start);
        let hi = n.text[r.end..].char_indices().nth(AHEAD).map_or(n.text.len(), |(i, _)| r.end + i);
        let mut text = Vec::new();
        let mut byte = Vec::new();
        for (i, c) in n.text[lo..hi].char_indices() {
            text.push(c);
            byte.push(lo + i);
        }
        byte.push(hi);
        let key = text.iter().copied().map(fold_char).collect();
        Some(Self {
            key,
            anchor: n.text[lo..r.start].chars().count(),
            floor: usize::from(behind.len() > BEHIND),
            text,
            byte,
        })
    }

    /// Nombre de caractères de la fenêtre.
    pub(crate) fn len(&self) -> usize {
        self.text.len()
    }

    /// Caractère d'index `p`.
    pub(crate) fn at(&self, p: usize) -> Option<char> {
        self.text.get(p).copied()
    }

    /// Texte des caractères `a..b`.
    pub(crate) fn text_of(&self, a: usize, b: usize) -> String {
        self.text[a..b].iter().collect()
    }

    /// Frontière de mot `B` : le caractère d'index `p` n'est ni une lettre ni un chiffre.
    pub(crate) fn bound(&self, p: usize) -> bool {
        self.at(p).is_none_or(|c| !c.is_alphanumeric())
    }

    /// Le caractère d'index `p` commence un mot : le précédent n'est ni une lettre ni un chiffre.
    pub(crate) fn word_start(&self, p: usize) -> bool {
        p.checked_sub(1).and_then(|i| self.at(i)).is_none_or(|c| !c.is_alphanumeric())
    }

    /// `WS?` : saute zéro espace ou plus.
    pub(crate) fn ws0(&self, mut p: usize) -> usize {
        while self.at(p) == Some(' ') {
            p += 1;
        }
        p
    }

    /// `WS` : saute une espace ou plus.
    pub(crate) fn ws(&self, p: usize) -> Option<usize> {
        let q = self.ws0(p);
        (q > p).then_some(q)
    }

    /// `SEP` : `WS? ("," | ";" | ":") WS?` ou `WS`.
    pub(crate) fn sep(&self, p: usize) -> Option<usize> {
        let q = self.ws0(p);
        if matches!(self.at(q), Some(',' | ';' | ':')) {
            Some(self.ws0(q + 1))
        } else {
            (q > p).then_some(q)
        }
    }

    /// Littéral sensible à la casse (`'CNIL'`), lu sur le texte. Pas de frontière.
    pub(crate) fn cs(&self, p: usize, lit: &str) -> Option<usize> {
        let end = p + lit.chars().count();
        self.text.get(p..end).filter(|s| s.iter().copied().eq(lit.chars())).map(|_| end)
    }

    /// Littéral sensible à la casse suivi d'une frontière de mot (un sigle).
    pub(crate) fn cs_word(&self, p: usize, lit: &str) -> Option<usize> {
        self.cs(p, lit).filter(|&q| self.bound(q))
    }

    /// Mot-clé (`"délibération"`), comparé sur la vue clé : `lit` est écrit replié (minuscules,
    /// sans accent). Une espace de `lit` vaut une espace ou plus. Un point final de `lit` est
    /// facultatif dans le texte et, s'il est lu, il est inclus. Sans lui, un mot-clé qui finit par
    /// une lettre ou un chiffre veut une frontière de mot.
    pub(crate) fn kw(&self, p: usize, lit: &str) -> Option<usize> {
        let (core, dotted) = lit.strip_suffix('.').map_or((lit, false), |core| (core, true));
        let mut q = p;
        for c in core.chars() {
            if c == ' ' {
                q = self.ws(q)?;
            } else if self.key.get(q) == Some(&c) {
                q += 1;
            } else {
                return None;
            }
        }
        if dotted && self.at(q) == Some('.') {
            return Some(q + 1);
        }
        let ends_on_word = core.chars().next_back().is_some_and(char::is_alphanumeric);
        (!ends_on_word || self.bound(q)).then_some(q)
    }

    /// Fin de la suite de chiffres ASCII qui commence en `p` (`p` si elle est vide).
    fn digit_run(&self, mut p: usize) -> usize {
        while matches!(self.at(p), Some('0'..='9')) {
            p += 1;
        }
        p
    }

    /// Valeur des chiffres `a..b` (saturée au-delà de 19 chiffres).
    fn value(&self, a: usize, b: usize) -> u64 {
        self.text[a..b].iter().fold(0_u64, |acc, c| {
            acc.saturating_mul(10).saturating_add(u64::from(c.to_digit(10).unwrap_or(0)))
        })
    }

    /// `/[0-9]{min,max}/ B` : fin et valeur. `max` ne dépasse pas 9 (la valeur tient en `u32`).
    pub(crate) fn digits(&self, p: usize, min: usize, max: usize) -> Option<(usize, u32)> {
        let e = self.digit_run(p);
        let value = u32::try_from(self.value(p, e)).unwrap_or(u32::MAX);
        ((min..=max).contains(&(e - p)) && self.bound(e)).then_some((e, value))
    }

    /// `/[0-9]{len}/ B` : fin et valeur, pour les nombres qui ne tiennent pas en `u32`.
    fn exact(&self, p: usize, len: usize) -> Option<(usize, u64)> {
        let e = self.digit_run(p);
        (e - p == len && self.bound(e)).then(|| (e, self.value(p, e)))
    }

    /// `YEAR4` : quatre chiffres de 1500 à 2099, puis une frontière.
    pub(crate) fn year4(&self, p: usize) -> Option<(usize, u16)> {
        let (e, value) = self.digits(p, 4, 4)?;
        let year = u16::try_from(value).ok().filter(|y| (1500..=2099).contains(y))?;
        Some((e, year))
    }

    /// `NUM_SIGN` : `n°`, `no` (devant un chiffre seulement), `num.` ou `numéro`, puis `WS?`.
    pub(crate) fn num_sign(&self, p: usize) -> Option<usize> {
        let digit_next = |q: usize| matches!(self.at(self.ws0(q)), Some('0'..='9'));
        let q = self
            .kw(p, "n°")
            .or_else(|| self.kw(p, "num."))
            .or_else(|| self.kw(p, "numero"))
            .or_else(|| self.kw(p, "no").filter(|&q| digit_next(q)))?;
        Some(self.ws0(q))
    }

    /// `ROMAN` : une suite non vide de chiffres romains majuscules, puis une frontière.
    pub(crate) fn roman(&self, mut p: usize) -> Option<usize> {
        let start = p;
        while matches!(self.at(p), Some('M' | 'D' | 'C' | 'L' | 'X' | 'V' | 'I')) {
            p += 1;
        }
        (p > start && self.bound(p)).then_some(p)
    }

    /// `DAY` : `1er`, `premier` ou un nombre de 1 à 31.
    fn day(&self, p: usize) -> Option<(usize, u8)> {
        self.kw(p, "1er")
            .or_else(|| self.kw(p, "premier"))
            .map(|q| (q, 1))
            .or_else(|| self.digits(p, 1, 2).and_then(|(q, v)| ranged(v, 31).map(|d| (q, d))))
    }

    /// `MONTH`, en toutes lettres ou abrégé.
    fn month(&self, p: usize) -> Option<(usize, u8)> {
        MONTHS.iter().find_map(|&(lit, month)| self.kw(p, lit).map(|q| (q, month)))
    }

    /// Séparateur de `DATE_NUM` : `/` ou `.`.
    fn date_sep(&self, p: usize) -> Option<usize> {
        matches!(self.at(p), Some('/' | '.')).then_some(p + 1)
    }

    /// `DATE_PROSE` : `7 octobre 2016`, `1er févr. 2024`.
    fn date_prose(&self, p: usize) -> Option<DateHit> {
        let (q, day) = self.day(p)?;
        let (q, month) = self.month(self.ws(q)?)?;
        let (q, year) = self.year4(self.ws(q)?)?;
        Some(DateHit::civil(q, year, month, day))
    }

    /// `DATE_NUM` : `29/05/2026`, `29.5.2026`.
    fn date_num(&self, p: usize) -> Option<DateHit> {
        let (q, day) = self.digits(p, 1, 2)?;
        let (q, month) = self.digits(self.date_sep(q)?, 1, 2)?;
        let (q, year) = self.year4(self.date_sep(q)?)?;
        Some(DateHit::civil(q, year, ranged(month, 12)?, ranged(day, 31)?))
    }

    /// `DATE_ISO` : `2026-04-30`.
    fn date_iso(&self, p: usize) -> Option<DateHit> {
        let (q, year) = self.year4(p)?;
        let (q, month) = self.digits(self.cs(q, "-")?, 2, 2)?;
        let (q, day) = self.digits(self.cs(q, "-")?, 2, 2)?;
        Some(DateHit::civil(q, year, ranged(month, 12)?, ranged(day, 31)?))
    }

    /// `DATE_REPU` : `12 thermidor an XII`, gardée brute.
    fn date_republican(&self, p: usize) -> Option<DateHit> {
        let (q, _) = self.day(p)?;
        let q = self.ws(q)?;
        let q = REPUBLICAN_MONTHS.iter().find_map(|m| self.kw(q, m))?;
        let q = self.kw(self.ws(q)?, "an")?;
        let q = self.roman(self.ws(q)?)?;
        Some(DateHit { end: q, date: None, republican: true })
    }

    /// `DATE` : prose, numérique, ISO ou républicaine.
    pub(crate) fn date(&self, p: usize) -> Option<DateHit> {
        self.date_prose(p)
            .or_else(|| self.date_num(p))
            .or_else(|| self.date_iso(p))
            .or_else(|| self.date_republican(p))
    }

    /// `WS "du" WS DATE`.
    pub(crate) fn du_date(&self, p: usize) -> Option<DateHit> {
        let q = self.kw(self.ws(p)?, "du")?;
        self.date(self.ws(q)?)
    }

    /// Débuts de lecture, du plus à gauche au plus à droite : les débuts de mots situés avant
    /// l'ancre (les 12 plus proches) et le début de l'ancre elle-même.
    fn starts(&self) -> Vec<usize> {
        let mut starts: Vec<usize> = (self.floor..self.anchor)
            .rev()
            .filter(|&i| self.at(i).is_some_and(char::is_alphanumeric) && self.word_start(i))
            .take(MAX_STARTS)
            .collect();
        starts.reverse();
        starts.push(self.anchor);
        starts
    }

    /// Candidats de la fenêtre : chaque début de lecture est essayé avec chaque règle, de gauche à
    /// droite ; ce qui tombe dans une reconnaissance déjà faite est sauté.
    fn read(&self, n: &Normalized, rules: &[Rule]) -> Vec<Candidate> {
        let mut found = Vec::new();
        let mut covered = 0;
        for s in self.starts() {
            let inside_word = self.at(s).is_some_and(char::is_alphanumeric) && !self.word_start(s);
            if s < covered || inside_word {
                continue;
            }
            if let Some(hit) = rules.iter().find_map(|rule| rule(self, s)) {
                covered = hit.end;
                found.push(self.candidate(n, hit));
            }
        }
        found
    }

    /// Candidat d'une reconnaissance, avec une plage d'offsets du texte d'origine.
    fn candidate(&self, n: &Normalized, hit: Hit) -> Candidate {
        let range = n.orig_range(self.byte[hit.start]..self.byte[hit.end]);
        let mut candidate = Candidate::new(range, hit.value);
        candidate.confidence = hit.confidence;
        candidate.flags = Flags { list: hit.flags, ambiguity: hit.ambiguity, ..Flags::default() };
        candidate
    }
}

/// Candidats d'une famille : les règles `rules` essayées aux ancres des classes `classes`, sans
/// doublon (deux ancres d'une même citation donnent le même candidat).
pub(crate) fn scan_with(
    n: &Normalized,
    anchors: &[Anchor],
    classes: &[AnchorClass],
    rules: &[Rule],
) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = anchors
        .iter()
        .filter(|a| classes.contains(&a.class))
        .filter_map(|a| Win::around(n, a))
        .flat_map(|w| w.read(n, rules))
        .collect();
    found.sort_by_key(|c| (c.range.start, c.range.end));
    found.dedup_by(|a, b| a.range == b.range && a.value == b.value);
    found
}

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    let classes =
        [AnchorClass::Family, AnchorClass::Sigle, AnchorClass::Nor, AnchorClass::LegifranceId];
    scan_with(n, anchors, &classes, &[journal, nor, eli, legifrance_id])
}

/// Un pas d'une grammaire en suite : la fin de ce qu'il lit à partir de `p`.
type Step = fn(&Win, usize) -> Option<usize>;

/// Enchaîne les pas : la fin du dernier, ou `None` dès qu'un pas échoue.
fn run_steps(w: &Win, p: usize, steps: &[Step]) -> Option<usize> {
    steps.iter().try_fold(p, |q, step| step(w, q))
}

/// Suite non vide de caractères qui vérifient `pred`, à partir de `p` : sa fin.
fn run(w: &Win, p: usize, pred: fn(char) -> bool) -> Option<usize> {
    let mut q = p;
    while w.at(q).is_some_and(pred) {
        q += 1;
    }
    (q > p).then_some(q)
}

// ---- JORF ---------------------------------------------------------------------------------

/// `'JORF' | 'JO' | "J.O." | "Journal officiel" [ WS "de la République française" ]` : la fin de
/// l'en-tête.
fn journal_head(w: &Win, s: usize) -> Option<usize> {
    w.cs_word(s, "JORF").or_else(|| w.cs_word(s, "JO")).or_else(|| w.kw(s, "j.o.")).or_else(|| {
        w.kw(s, "journal officiel")
            .map(|q| w.ws(q).and_then(|r| w.kw(r, "de la republique francaise")).unwrap_or(q))
    })
}

/// `jorf` : `JORF n°0263 du 8 novembre 2025, texte n° 152, p. 4`.
fn journal(w: &Win, s: usize) -> Option<Hit> {
    let head = journal_head(w, s)?;
    let number = w.num_sign(w.ws0(head)).and_then(|q| w.digits(q, 1, 4));
    let date = w.du_date(number.map_or(head, |(q, _)| q))?;
    let texte = w
        .sep(date.end)
        .and_then(|q| w.kw(q, "texte"))
        .and_then(|q| w.ws(q))
        .and_then(|q| w.num_sign(q))
        .and_then(|q| w.digits(q, 1, 9));
    let after_texte = texte.map_or(date.end, |(q, _)| q);
    let page = w
        .sep(after_texte)
        .and_then(|q| w.kw(q, "p.").or_else(|| w.kw(q, "page")))
        .and_then(|q| w.ws(q))
        .and_then(|q| w.digits(q, 1, 9));
    let cite = JorfCite {
        number: number.map(|(_, v)| u16::try_from(v).unwrap_or(u16::MAX)),
        date: date.date,
        texte: texte.map(|(_, v)| v),
        page: page.map(|(_, v)| v),
    };
    let mut hit = Hit::new(s, page.map_or(after_texte, |(q, _)| q), CitationRef::Jorf(cite));
    hit.flags.extend(date.flag());
    Some(hit)
}

// ---- NOR ----------------------------------------------------------------------------------

/// Quatre lettres, sept chiffres, une lettre, puis une frontière : la fin du NOR. Les lettres
/// sont des majuscules, ou de toute casse après le mot `NOR` (`any_case`).
fn nor_value(w: &Win, p: usize, any_case: bool) -> Option<usize> {
    let letter = |i: usize| {
        w.at(i).is_some_and(
            |c| if any_case { c.is_ascii_alphabetic() } else { c.is_ascii_uppercase() },
        )
    };
    let digit = |i: usize| matches!(w.at(i), Some('0'..='9'));
    let shaped = (p..p + 4).all(letter) && (p + 4..p + 11).all(digit) && letter(p + 11);
    (shaped && w.bound(p + 12)).then_some(p + 12)
}

/// `nor` : `NOR : ECFI1524250L` (le span est la valeur seule) ou un NOR en majuscules.
fn nor(w: &Win, s: usize) -> Option<Hit> {
    let after_keyword = w
        .cs_word(s, "NOR")
        .map(|q| w.ws0(q))
        .map(|q| if w.at(q) == Some(':') { w.ws0(q + 1) } else { q });
    let (start, end) = after_keyword
        .and_then(|p| nor_value(w, p, true).map(|e| (p, e)))
        .or_else(|| nor_value(w, s, false).map(|e| (s, e)))?;
    Nor::parse(&w.text_of(start, end)).map(|nor| Hit::new(start, end, CitationRef::Nor(nor)))
}

// ---- ELI ----------------------------------------------------------------------------------

/// `"/"`.
fn slash(w: &Win, p: usize) -> Option<usize> {
    w.cs(p, "/")
}

/// `YEAR4`.
fn eli_year(w: &Win, p: usize) -> Option<usize> {
    w.year4(p).map(|(e, _)| e)
}

/// `/[0-9]{1,2}/` : un mois ou un jour.
fn eli_part(w: &Win, p: usize) -> Option<usize> {
    w.digits(p, 1, 2).map(|(e, _)| e)
}

/// `eli_kind` : `/[a-z_]+/`.
fn eli_kind(w: &Win, p: usize) -> Option<usize> {
    run(w, p, |c| c.is_ascii_lowercase() || c == '_')
}

/// `eli_key` : un NOR en majuscules, ou `/[0-9]{4}-[0-9]+/`.
fn eli_key(w: &Win, p: usize) -> Option<usize> {
    nor_value(w, p, false).or_else(|| {
        w.digits(p, 4, 4)
            .and_then(|(e, _)| w.cs(e, "-"))
            .and_then(|e| w.digits(e, 1, 9))
            .map(|(e, _)| e)
    })
}

/// Version de l'ELI : `"jo"` ou `/[a-z]+/`.
fn eli_version(w: &Win, p: usize) -> Option<usize> {
    run(w, p, |c| c.is_ascii_lowercase())
}

/// Fin de l'ELI : `"texte"` ou `"article_" /[0-9A-Za-z_-]+/`.
fn eli_last(w: &Win, p: usize) -> Option<usize> {
    w.kw(p, "texte").or_else(|| {
        w.kw(p, "article_")
            .and_then(|q| run(w, q, |c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')))
    })
}

/// `"jo"`.
fn eli_jo(w: &Win, p: usize) -> Option<usize> {
    w.kw(p, "jo")
}

/// Après `/eli/` : `eli_kind "/" YEAR4 "/" mois "/" jour "/" eli_key "/" version "/" fin`.
const ELI_TEXT: [Step; 13] = [
    eli_kind,
    slash,
    eli_year,
    slash,
    eli_part,
    slash,
    eli_part,
    slash,
    eli_key,
    slash,
    eli_version,
    slash,
    eli_last,
];

/// Après `/eli/` : `"jo" "/" YEAR4 "/" mois "/" jour "/" /[0-9]{4}/`.
const ELI_ISSUE: [Step; 9] =
    [eli_jo, slash, eli_year, slash, eli_part, slash, eli_part, slash, four_digits];

/// `eli` : un ELI cité, avec ou sans l'hôte de Légifrance.
fn eli(w: &Win, s: usize) -> Option<Hit> {
    let host = w.kw(s, "https://www.legifrance.gouv.fr");
    let p = w.kw(host.unwrap_or(s), "/eli/")?;
    let end = run_steps(w, p, &ELI_TEXT).or_else(|| run_steps(w, p, &ELI_ISSUE))?;
    Eli::parse(&w.text_of(s, end)).map(|eli| Hit::new(s, end, CitationRef::Eli(eli)))
}

// ---- Identifiants Légifrance ----------------------------------------------------------------

/// `"_"`.
fn underscore(w: &Win, p: usize) -> Option<usize> {
    w.cs(p, "_")
}

/// `"-"`.
fn dash(w: &Win, p: usize) -> Option<usize> {
    w.cs(p, "-")
}

/// Deux chiffres.
fn two_digits(w: &Win, p: usize) -> Option<usize> {
    w.digits(p, 2, 2).map(|(e, _)| e)
}

/// Quatre chiffres.
fn four_digits(w: &Win, p: usize) -> Option<usize> {
    w.digits(p, 4, 4).map(|(e, _)| e)
}

/// `"_" JJ "-" MM "-" AAAA`.
const VERSION_SUFFIX: [Step; 6] = [underscore, two_digits, dash, two_digits, dash, four_digits];

/// Suffixe de version `_JJ-MM-AAAA` qui commence en `p` : la date, validée par `CivilDate`.
fn version_suffix(w: &Win, p: usize) -> Option<DateHit> {
    let end = run_steps(w, p, &VERSION_SUFFIX)?;
    let part = |a: usize, b: usize| w.value(a, b);
    let year = u16::try_from(part(p + 7, p + 11)).unwrap_or(0);
    let month = u8::try_from(part(p + 4, p + 6)).unwrap_or(0);
    let day = u8::try_from(part(p + 1, p + 3)).unwrap_or(0);
    Some(DateHit::civil(end, year, month, day))
}

/// Segment d'URL qui précède l'identifiant en `s` : `juri` dans `/juri/id/JURITEXT…`, ou le
/// segment qui touche l'identifiant (`article_lc` dans `/codes/article_lc/LEGIARTI…`).
fn url_kind(w: &Win, s: usize) -> Option<String> {
    let slash = s.checked_sub(1).filter(|&i| w.at(i) == Some('/'))?;
    let seg_end = slash.checked_sub(3).filter(|&i| w.cs(i, "/id") == Some(slash)).unwrap_or(slash);
    let mut seg_start = seg_end;
    while seg_start > 0 && w.at(seg_start - 1).is_some_and(|c| c.is_ascii_lowercase() || c == '_') {
        seg_start -= 1;
    }
    let after_slash = seg_start.checked_sub(1).and_then(|i| w.at(i)) == Some('/');
    (seg_start < seg_end && after_slash).then(|| w.text_of(seg_start, seg_end))
}

/// `lf_id` : un des seize préfixes, douze chiffres, un suffixe de version éventuel.
fn legifrance_id(w: &Win, s: usize) -> Option<Hit> {
    let prefix = (s..s + 8)
        .map(|i| w.at(i))
        .collect::<Option<String>>()
        .and_then(|word| IdPrefix::parse(&word))?;
    let (q, digits) = w.exact(s + 8, 12)?;
    let version = version_suffix(w, q);
    let cite = LegifranceIdRef {
        prefix,
        digits,
        version_start: version.and_then(|d| d.date),
        url_kind: url_kind(w, s),
    };
    let mut hit = Hit::new(s, version.map_or(q, |d| d.end), CitationRef::LegifranceId(cite));
    hit.flags.extend(version.and_then(|d| d.flag()));
    Some(hit)
}

/// Outillage des tests de grammaire : ancres construites à la main, lecture des candidats.
#[cfg(test)]
pub(crate) mod testkit {
    use crate::citation::normalize::{Normalized, Steps, normalize};
    use crate::citation::scan::{Anchor, AnchorClass};
    use crate::citation::types::{Ambiguity, CitationRef, Confidence, Flag};

    use super::Candidate;

    /// Signature commune des `scan` de famille.
    pub(crate) type Scan = fn(&Normalized, &[Anchor]) -> Vec<Candidate>;

    /// Un candidat lu : le texte d'origine qu'il couvre et ce qu'il porte.
    #[derive(Debug, PartialEq)]
    pub(crate) struct Found {
        pub(crate) text: String,
        pub(crate) value: CitationRef,
        pub(crate) confidence: Confidence,
        pub(crate) ambiguity: Option<Ambiguity>,
        pub(crate) flags: Vec<Flag>,
    }

    /// Ancres posées sur la première occurrence, dans le texte normalisé, de chaque `needle` (un
    /// `needle` absent ne pose rien), avec la classe qui l'accompagne.
    pub(crate) fn anchors(n: &Normalized, pairs: &[(AnchorClass, &str)]) -> Vec<Anchor> {
        pairs
            .iter()
            .filter_map(|&(class, needle)| {
                n.text
                    .find(needle)
                    .map(|start| Anchor { class, range: start..start + needle.len() })
            })
            .collect()
    }

    /// Candidats de `scan` sur `input`, avec les ancres de `pairs`.
    pub(crate) fn find_pairs(scan: Scan, input: &str, pairs: &[(AnchorClass, &str)]) -> Vec<Found> {
        let n = normalize(input, Steps::default());
        let candidates = scan(&n, &anchors(&n, pairs));
        candidates
            .into_iter()
            .map(|c| Found {
                text: input[c.range.clone()].to_owned(),
                value: c.value,
                confidence: c.confidence,
                ambiguity: c.flags.ambiguity,
                flags: c.flags.list,
            })
            .collect()
    }

    /// Candidats de `scan` sur `input`, avec des ancres de classe `class` sur `needles`.
    pub(crate) fn find(
        scan: Scan,
        input: &str,
        class: AnchorClass,
        needles: &[&str],
    ) -> Vec<Found> {
        let pairs: Vec<(AnchorClass, &str)> = needles.iter().map(|&n| (class, n)).collect();
        find_pairs(scan, input, &pairs)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::testkit::{Found, find, find_pairs};
    use super::*;
    use crate::citation::normalize::{Steps, normalize};
    use crate::citation::types::{IdPrefix, JorfCite, LegifranceIdRef};
    use crate::contract::{Eli, Nor};

    fn date(y: u16, m: u8, d: u8) -> Option<CivilDate> {
        CivilDate::new(y, m, d)
    }

    fn found(input: &str, needles: &[&str]) -> Vec<Found> {
        find(scan, input, AnchorClass::Family, needles)
    }

    fn one(input: &str, needles: &[&str]) -> Found {
        let mut all = found(input, needles);
        assert_eq!(all.len(), 1, "{input:?} : {all:?}");
        all.remove(0)
    }

    fn jorf(
        number: Option<u16>,
        day: Option<(u16, u8, u8)>,
        texte: Option<u32>,
        page: Option<u32>,
    ) -> CitationRef {
        CitationRef::Jorf(JorfCite {
            number,
            date: day.and_then(|(y, m, d)| date(y, m, d)),
            texte,
            page,
        })
    }

    fn nor(value: &str) -> CitationRef {
        CitationRef::Nor(Nor::parse(value).expect("NOR valide"))
    }

    fn eli(value: &str) -> CitationRef {
        CitationRef::Eli(Eli::parse(value).expect("ELI valide"))
    }

    fn id(
        prefix: IdPrefix,
        digits: u64,
        version: Option<(u16, u8, u8)>,
        url: Option<&str>,
    ) -> CitationRef {
        CitationRef::LegifranceId(LegifranceIdRef {
            prefix,
            digits,
            version_start: version.and_then(|(y, m, d)| date(y, m, d)),
            url_kind: url.map(str::to_owned),
        })
    }

    fn by_id(input: &str, needle: &str) -> Vec<Found> {
        find(scan, input, AnchorClass::LegifranceId, &[needle])
    }

    // ---- JORF ------------------------------------------------------------------------------

    #[test]
    fn p190_to_p192_jorf_with_number_date_and_text() {
        let f = one("JORF n°0263 du 8 novembre 2025, texte n° 152", &["JORF"]);
        assert_eq!(f.text, "JORF n°0263 du 8 novembre 2025, texte n° 152");
        assert_eq!(f.value, jorf(Some(263), Some((2025, 11, 8)), Some(152), None));
        assert_eq!((f.confidence, f.ambiguity, f.flags), (Confidence::High, None, vec![]));
        let g = one("JORF n°0032 du 7 février 2026, texte n° 66", &["JORF"]);
        assert_eq!(g.value, jorf(Some(32), Some((2026, 2, 7)), Some(66), None));
        let h = one("JORF n° 0235 du 8 octobre 2016", &["JORF"]);
        assert_eq!(h.text, "JORF n° 0235 du 8 octobre 2016");
        assert_eq!(h.value, jorf(Some(235), Some((2016, 10, 8)), None, None));
    }

    #[test]
    fn the_ways_to_write_the_journal() {
        let fam = AnchorClass::Family;
        let sig = AnchorClass::Sigle;
        let table = [
            ("JO du 8 octobre 2016", "JO", sig, None),
            ("J.O. n° 235 du 8 octobre 2016", "J.O.", sig, Some(235)),
            ("J.O n° 235 du 8 octobre 2016", "J.O", sig, Some(235)),
            ("Journal officiel n° 235 du 8 octobre 2016", "Journal officiel", fam, Some(235)),
            (
                "Journal officiel de la République française n° 235 du 8 octobre 2016",
                "Journal officiel",
                fam,
                Some(235),
            ),
            ("journal officiel du 8 octobre 2016", "journal officiel", fam, None),
        ];
        for (input, needle, class, number) in table {
            let all = find(scan, input, class, &[needle]);
            assert_eq!(all.len(), 1, "{input}");
            assert_eq!(all[0].text, input, "{input}");
            assert_eq!(all[0].value, jorf(number, Some((2016, 10, 8)), None, None), "{input}");
        }
    }

    #[test]
    fn jorf_text_and_page_parts_are_optional_and_ordered() {
        let f = one("JORF du 8 novembre 2025 texte n° 152", &["JORF"]);
        assert_eq!(f.value, jorf(None, Some((2025, 11, 8)), Some(152), None));
        let g = one("JORF du 8 novembre 2025, page 12", &["JORF"]);
        assert_eq!(g.text, "JORF du 8 novembre 2025, page 12");
        assert_eq!(g.value, jorf(None, Some((2025, 11, 8)), None, Some(12)));
        let h = one("JORF n°0263 du 8 novembre 2025, texte n° 152, p. 4", &["JORF"]);
        assert_eq!(h.value, jorf(Some(263), Some((2025, 11, 8)), Some(152), Some(4)));
        // Sans signe de numéro, avec un numéro trop gros ou mal écrit : la partie n'est pas lue.
        for tail in [", texte 152", ", texte n° 9999999999", ", p.12", ", p. x", ", page"] {
            let input = format!("JORF du 8 novembre 2025{tail}");
            assert_eq!(one(&input, &["JORF"]).text, "JORF du 8 novembre 2025", "{input}");
        }
    }

    #[test]
    fn jorf_needs_a_date_and_a_clean_number() {
        for (input, needle) in [
            ("JORF n° 0235", "JORF"),
            ("JORF n° 12345 du 8 octobre 2016", "JORF"),
            ("JORFX du 8 octobre 2016", "JORFX"),
            ("JORF du 8 octobrex 2016", "JORF"),
            ("JORF 8 octobre 2016", "JORF"),
            ("Journal du 8 octobre 2016", "Journal"),
            ("Journal officiel de la Républiquex du 8 octobre 2016", "Journal"),
            ("JORF 0235 du 8 octobre 2016", "JORF"),
        ] {
            assert_eq!(found(input, &[needle]), [], "{input}");
        }
        // Date impossible : gardée brute avec un drapeau.
        let f = one("JORF du 31 février 2016", &["JORF"]);
        assert_eq!((f.value, f.flags), (jorf(None, None, None, None), vec![Flag::DateInvalide]));
    }

    // ---- NOR -------------------------------------------------------------------------------

    #[test]
    fn p193_and_p194_nor_after_the_keyword_span_the_value_only() {
        let f = one("NOR : ECFI1524250L", &["NOR"]);
        assert_eq!(f.text, "ECFI1524250L");
        assert_eq!(f.value, nor("ECFI1524250L"));
        let g = one("NOR: cscx2531180s", &["NOR"]);
        assert_eq!(g.text, "cscx2531180s");
        assert_eq!(g.value, nor("CSCX2531180S"));
        assert_eq!(one("NOR ECFI1524250L", &["NOR"]).text, "ECFI1524250L");
        assert_eq!(one("NOR:ECFI1524250L", &["NOR"]).text, "ECFI1524250L");
    }

    #[test]
    fn a_bare_nor_is_uppercase_and_stands_alone() {
        let f = find(scan, "le décret AFSA1629022D du", AnchorClass::Nor, &["AFSA1629022D"]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].text, "AFSA1629022D");
        assert_eq!(f[0].value, nor("AFSA1629022D"));
        for (input, token) in [
            ("afsa1629022d", "afsa1629022d"),
            ("NOR ECFI152425L", "ECFI152425L"),
            ("XAFSA1629022D", "AFSA1629022D"),
            ("AFSA1629022DX", "AFSA1629022D"),
            ("AFSA16290222D", "AFSA16290222D"),
            ("NORECFI1524250L", "ECFI1524250L"),
            ("NOR : ecfi152425", "ecfi152425"),
            ("NOR", "NOR"),
        ] {
            let pairs = [(AnchorClass::Nor, token), (AnchorClass::Family, "NOR")];
            assert_eq!(find_pairs(scan, input, &pairs), [], "{input}");
        }
        // Le même NOR vu par deux ancres ne sort qu'une fois.
        let both = [(AnchorClass::Family, "NOR"), (AnchorClass::Nor, "ECFI1524250L")];
        assert_eq!(find_pairs(scan, "NOR : ECFI1524250L", &both).len(), 1);
    }

    // ---- ELI -------------------------------------------------------------------------------

    #[test]
    fn p195_and_p196_eli_with_and_without_the_host() {
        let f = one("/eli/decret/2021/7/13/PRMD2117108D/jo/article_1", &["/eli/"]);
        assert_eq!(f.text, "/eli/decret/2021/7/13/PRMD2117108D/jo/article_1");
        assert_eq!(f.value, eli("/eli/decret/2021/7/13/PRMD2117108D/jo/article_1"));
        let url = "https://www.legifrance.gouv.fr/eli/decret/2018/2/13/JUSC1732516D/jo/texte";
        let g = one(url, &["/eli/"]);
        assert_eq!(g.text, url);
        assert_eq!(g.value, eli(url));
    }

    #[test]
    fn eli_other_shapes_and_boundaries() {
        let f = one("voir (https://www.legifrance.gouv.fr/eli/jo/2019/1/25/0021).", &["/eli/"]);
        assert_eq!(f.text, "https://www.legifrance.gouv.fr/eli/jo/2019/1/25/0021");
        let g = one("/eli/loi/2016/10/7/2016-1321/jo/texte.", &["/eli/"]);
        assert_eq!(g.text, "/eli/loi/2016/10/7/2016-1321/jo/texte");
        let h = one("/eli/arrete/2020/12/31/ABCD2012345E/oj/article_L-1_bis, suite", &["/eli/"]);
        assert_eq!(h.text, "/eli/arrete/2020/12/31/ABCD2012345E/oj/article_L-1_bis");
        // Un hôte en capitales n'est pas celui de Légifrance : seul le chemin est cité.
        let i = one("HTTPS://WWW.LEGIFRANCE.GOUV.FR/eli/jo/2019/1/25/0021", &["/eli/"]);
        assert_eq!(i.text, "/eli/jo/2019/1/25/0021");
        // Au-delà de 256 caractères, ce n'est plus un ELI.
        let long = format!("/eli/loi/2016/10/7/2016-1321/jo/article_{}", "a".repeat(300));
        assert_eq!(found(&long, &["/eli/"]), []);
    }

    #[test]
    fn malformed_elis_are_not_citations() {
        for input in [
            "/eli/decret/2021/7/13/PRMD2117108/jo/article_1",
            "/eli/decret/2021/7/13/PRMD2117108D/jo",
            "/eli/decret/21/7/13/PRMD2117108D/jo/texte",
            "/eli/decret/2021/007/13/PRMD2117108D/jo/texte",
            "/eli/decret/2021/7/013/PRMD2117108D/jo/texte",
            "/eli/jo/2019/1/25/002",
            "/eli/Decret/2021/7/13/PRMD2117108D/jo/texte",
            "/eli/decret/2021/7/13/PRMD2117108D/Jo/texte",
            "/eli/decret/2021/7/13/PRMD2117108D/jo/article_",
            "/eli/decret/2021/7/13/PRMD2117108D/jo/textes",
            "/eli/decret/2021/7/13/PRMD2117108D//texte",
            "/eli/decret/2021/7/13/PRMD2117108D",
            "/eli/decret/2021/7/13/2016/jo/texte",
            "/eli/decret/2021/7/13/20161321/jo/texte",
            "/eli/decret/2021/7/13/2016-/jo/texte",
            "/eli//2021/7/13/PRMD2117108D/jo/texte",
            "/eli/",
        ] {
            assert_eq!(found(input, &["/eli/"]), [], "{input}");
        }
    }

    // ---- Identifiants Légifrance ---------------------------------------------------------------

    #[test]
    fn p197_to_p200_legifrance_identifiers() {
        let f = by_id("LEGIARTI000006419288", "LEGIARTI");
        assert_eq!(f[0].value, id(IdPrefix::LegiArti, 6_419_288, None, None));
        let g = by_id("LEGITEXT000033205014_08-09-2023", "LEGITEXT");
        assert_eq!(g[0].text, "LEGITEXT000033205014_08-09-2023");
        assert_eq!(g[0].value, id(IdPrefix::LegiText, 33_205_014, Some((2023, 9, 8)), None));
        let h = by_id("https://www.legifrance.gouv.fr/juri/id/JURITEXT000036780011", "JURITEXT");
        assert_eq!(h[0].text, "JURITEXT000036780011");
        assert_eq!(h[0].value, id(IdPrefix::JuriText, 36_780_011, None, Some("juri")));
        let i = by_id("KALICONT000005635096", "KALICONT");
        assert_eq!(i[0].value, id(IdPrefix::KaliCont, 5_635_096, None, None));
    }

    #[test]
    fn identifiers_url_kind_and_suffix_edge_cases() {
        let url = "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288";
        let a = by_id(url, "LEGIARTI");
        assert_eq!(a[0].value, id(IdPrefix::LegiArti, 6_419_288, None, Some("article_lc")));
        // Pas de segment, ou un segment qui ne commence pas après une barre : pas de `url_kind`.
        for input in
            ["/LEGIARTI000006419288", "juri/id/LEGIARTI000006419288", "x/LEGIARTI000006419288"]
        {
            let f = by_id(input, "LEGIARTI");
            assert_eq!(f[0].value, id(IdPrefix::LegiArti, 6_419_288, None, None), "{input}");
        }
        // Date de version impossible : gardée brute avec un drapeau ; suffixe mal formé ignoré.
        let b = by_id("LEGITEXT000033205014_31-02-2023", "LEGITEXT");
        assert_eq!(b[0].flags, vec![Flag::DateInvalide]);
        assert_eq!(b[0].value, id(IdPrefix::LegiText, 33_205_014, None, None));
        assert_eq!(b[0].text, "LEGITEXT000033205014_31-02-2023");
        let c = by_id("LEGITEXT000033205014_8-9-2023", "LEGITEXT");
        assert_eq!(c[0].text, "LEGITEXT000033205014");
        let d = by_id("LEGITEXT000033205014_08-09-20234", "LEGITEXT");
        assert_eq!(d[0].text, "LEGITEXT000033205014");
    }

    #[test]
    fn n23_short_long_or_misplaced_identifiers_are_not_citations() {
        for (input, needle) in [
            ("LEGIARTI00000641928", "LEGIARTI"),
            ("LEGIARTI0000064192888", "LEGIARTI"),
            ("JURINOME000036780011", "JURINOME"),
            ("legiarti000006419288", "legiarti"),
            ("XLEGIARTI000006419288", "LEGIARTI"),
            ("LEGIARTI000006419288X", "LEGIARTI"),
            ("LEGIARTI", "LEGIARTI"),
            ("LEGIARTIABCDEFGHIJKL", "LEGIARTI"),
        ] {
            assert_eq!(by_id(input, needle), [], "{input}");
        }
    }

    #[test]
    fn classes_are_filtered_and_the_four_rules_share_one_scan() {
        let input = "JORF du 8 octobre 2016, NOR : ECFI1524250L, LEGIARTI000006419288";
        let pairs = [
            (AnchorClass::Family, "JORF"),
            (AnchorClass::Family, "NOR"),
            (AnchorClass::LegifranceId, "LEGIARTI"),
        ];
        let all = find_pairs(scan, input, &pairs);
        let texts: Vec<&str> = all.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(texts, ["JORF du 8 octobre 2016", "ECFI1524250L", "LEGIARTI000006419288"]);
        assert_eq!(find(scan, input, AnchorClass::Court, &["JORF"]), []);
    }

    #[test]
    fn a_mid_word_anchor_is_skipped() {
        let n = normalize("xJORF du 8 octobre 2016", Steps::default());
        let anchor = Anchor { class: AnchorClass::Family, range: 1..5 };
        assert_eq!(scan(&n, &[anchor]), []);
    }

    // ---- Lexique commun ------------------------------------------------------------------------

    fn win(text: &str, anchor: &str) -> Win {
        let n = normalize(text, Steps::default());
        let start = n.text.find(anchor).expect("ancre présente");
        let anchor = Anchor { class: AnchorClass::Family, range: start..start + anchor.len() };
        Win::around(&n, &anchor).expect("fenêtre")
    }

    /// Fenêtre dont l'ancre est vide, au début du texte.
    fn win0(text: &str) -> Win {
        let n = normalize(text, Steps::default());
        Win::around(&n, &Anchor { class: AnchorClass::Family, range: 0..0 }).expect("fenêtre")
    }

    #[test]
    fn the_window_reads_80_characters_before_and_400_after() {
        let text = format!("{}CNIL{}", "a".repeat(200), "b".repeat(500));
        let w = win(&text, "CNIL");
        assert_eq!((w.len(), w.anchor, w.floor), (81 + 4 + 400, 81, 1));
        assert_eq!(w.byte.len(), w.len() + 1);
        let short = win("ab CNIL", "CNIL");
        assert_eq!((short.len(), short.anchor, short.floor), (7, 3, 0));
        // 81 caractères exactement avant l'ancre : le premier ne sert qu'à voir si un mot commence.
        let edge = win(&format!("{}CNIL", "a".repeat(81)), "CNIL");
        assert_eq!((edge.anchor, edge.floor), (81, 1));
        let tight = win(&format!("{}CNIL", "a".repeat(80)), "CNIL");
        assert_eq!((tight.anchor, tight.floor), (80, 0));
    }

    #[test]
    fn a_bad_anchor_gives_no_window() {
        let n = normalize("é et plus", Steps::default());
        for range in [1..2, 5..100] {
            let anchor = Anchor { class: AnchorClass::Family, range };
            assert!(Win::around(&n, &anchor).is_none());
            assert_eq!(scan_with(&n, &[anchor], &[AnchorClass::Family], &[]), []);
        }
    }

    #[test]
    fn at_most_twelve_word_starts_before_the_anchor_are_tried() {
        let w = win("a b c d e f g h i j k l m n o p CNIL", "CNIL");
        let starts = w.starts();
        assert_eq!(starts.len(), 13);
        assert_eq!(starts.last().copied(), Some(w.anchor));
        assert_eq!(starts.first().copied(), Some(w.anchor - 24));
        // La fenêtre coupée au milieu d'un mot ne propose pas le début de ce mot.
        let cut = win(&format!("{}x CNIL", "a".repeat(100)), "CNIL");
        assert_eq!(cut.starts(), vec![cut.anchor]);
    }

    #[test]
    fn keywords_fold_accents_and_case_and_handle_dots_and_spaces() {
        let w = win("DÉLIB. n°", "DÉLIB");
        assert_eq!(w.kw(0, "delib."), Some(6));
        assert_eq!(w.kw(0, "deli"), None);
        assert_eq!(w.kw(7, "n°"), Some(9));
        assert_eq!(win("n°2", "n").kw(0, "n°"), Some(2));
        assert_eq!(win("de  la formation", "de").kw(0, "de la formation"), Some(16));
        let nodot = win("délib n°", "délib");
        assert_eq!(nodot.kw(0, "delib."), Some(5));
        assert_eq!(nodot.kw(0, "delib"), Some(5));
        let glued = win("délibérationx", "délib");
        assert_eq!(glued.kw(0, "delib."), None);
        assert_eq!(glued.kw(0, "deliberation"), None);
        assert_eq!(win("abc", "abc").kw(0, "abd"), None);
        assert_eq!(win("ab", "ab").kw(0, "abc"), None);
    }

    #[test]
    fn case_sensitive_literals_and_boundaries() {
        let w = win("CNIL cnil CNILS", "CNIL");
        assert_eq!(w.cs(0, "CNIL"), Some(4));
        assert_eq!(w.cs(5, "CNIL"), None);
        assert_eq!(w.cs_word(0, "CNIL"), Some(4));
        assert_eq!(w.cs_word(10, "CNIL"), None);
        assert_eq!(w.cs(12, "CNILS"), None);
        assert!(w.bound(4) && !w.bound(10) && w.bound(w.len()));
        assert!(w.word_start(0) && w.word_start(5) && !w.word_start(1));
    }

    #[test]
    fn spaces_and_separators() {
        let w = win("a ,  b ; c :d e", "a");
        assert_eq!((w.ws0(1), w.ws(1), w.ws(2)), (2, Some(2), None));
        assert_eq!(w.sep(1), Some(5));
        assert_eq!(w.sep(6), Some(9));
        assert_eq!(w.sep(10), Some(12));
        assert_eq!(w.sep(13), Some(14));
        assert_eq!(w.sep(14), None);
    }

    #[test]
    fn numbers_years_and_number_signs() {
        let w = win("123 12a 1234 000012345", "123");
        assert_eq!(w.digits(0, 1, 4), Some((3, 123)));
        assert_eq!(w.digits(0, 4, 4), None);
        assert_eq!(w.digits(4, 1, 4), None);
        assert_eq!(w.digits(8, 4, 4), Some((12, 1234)));
        assert_eq!(w.digits(13, 1, 9), Some((22, 12_345)));
        assert_eq!(w.digits(13, 1, 8), None);
        for (text, expected) in [
            ("1500", Some((4, 1500))),
            ("2099", Some((4, 2099))),
            ("1499", None),
            ("2100", None),
            ("12345", None),
            ("abcd", None),
        ] {
            assert_eq!(win(text, text).year4(0), expected, "{text}");
        }
        for (text, expected) in [
            ("no 7", Some(3)),
            ("no x", None),
            ("numéro 3", Some(7)),
            ("num. 4", Some(5)),
            ("n°5", Some(2)),
            ("n° 5", Some(3)),
            ("n°", Some(2)),
            ("x", None),
        ] {
            assert_eq!(win(text, text).num_sign(0), expected, "{text}");
        }
    }

    #[test]
    fn every_date_form_of_annex_b_4_1() {
        let at = |text: &str| {
            let w = win0(text);
            w.date(0).map(|d| (d.end == w.len(), d.date, d.flag()))
        };
        let ok = |y, m, d| (true, date(y, m, d), None);
        assert_eq!(at("7 octobre 2016"), Some(ok(2016, 10, 7)));
        assert_eq!(at("1er févr. 2024"), Some(ok(2024, 2, 1)));
        assert_eq!(at("premier mars 2024"), Some(ok(2024, 3, 1)));
        assert_eq!(at("1 janvier 2018"), Some(ok(2018, 1, 1)));
        assert_eq!(at("07 novembre 2025"), Some(ok(2025, 11, 7)));
        assert_eq!(at("4 JUIN 2025"), Some(ok(2025, 6, 4)));
        assert_eq!(at("12 sept 1999"), Some(ok(1999, 9, 12)));
        assert_eq!(at("3 DÉC. 2001"), Some(ok(2001, 12, 3)));
        assert_eq!(at("29/05/2026"), Some(ok(2026, 5, 29)));
        assert_eq!(at("9.5.2026"), Some(ok(2026, 5, 9)));
        assert_eq!(at("2026-04-30"), Some(ok(2026, 4, 30)));
        let invalid = Some(Flag::DateInvalide);
        assert_eq!(at("31 février 2020"), Some((true, None, invalid.clone())));
        assert_eq!(at("31/02/2020"), Some((true, None, invalid.clone())));
        assert_eq!(at("2020-02-30"), Some((true, None, invalid)));
        assert_eq!(at("12 thermidor an XII"), Some((true, None, Some(Flag::DateRepublicaine))));
        for bad in [
            "32 janvier 2020",
            "0 janvier 2020",
            "7 octobrex 2016",
            "7 octobre 1499",
            "7 octobre 2100",
            "7 octobre 20166",
            "7octobre 2016",
            "32/01/2020",
            "01/13/2020",
            "01-01-2020",
            "01/01x2020",
            "2020-13-01",
            "2020-12-32",
            "2020-1-01",
            "2020-12-1",
            "2020/12/01",
            "7 oct.2016",
            "29/05",
            "29/05/20",
            "2020-12",
            "12 thermidor, an XII",
            "12 thermidor an",
            "12 thermidor an xii",
            "12 thermidor XII",
            "12 thermidorx an XII",
            "12 foo an XII",
        ] {
            assert_eq!(at(bad), None, "{bad}");
        }
    }

    #[test]
    fn du_date_wants_spaces_and_the_word_du() {
        for text in [" du 8 janvier 2026", " DU 3 mai 2020", " du  5/5/2020"] {
            let w = win0(text);
            assert_eq!(w.du_date(0).map(|d| d.end), Some(w.len()), "{text}");
        }
        for text in [
            "du 8 janvier 2026",
            " du8 janvier 2026",
            " du-8 janvier 2026",
            " dux 8 janvier 2026",
            " du 8 foo",
        ] {
            assert_eq!(win0(text).du_date(0), None, "{text}");
        }
    }

    #[test]
    fn roman_numerals_are_uppercase_and_non_empty() {
        assert_eq!(win("XII x", "XII").roman(0), Some(3));
        assert_eq!(win("XII x", "XII").roman(4), None);
        assert_eq!(win("XIIa", "XII").roman(0), None);
        assert_eq!(win("IV.", "IV").roman(0), Some(2));
    }
}
