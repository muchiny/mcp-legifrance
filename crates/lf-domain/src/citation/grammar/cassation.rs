//! Grammaire de la Cour de cassation (annexe B §4.5) et lexique commun des grammaires de
//! juridictions (§4.1) : [`lex`] porte les primitives (espaces, séparateurs, dates, ordinaux,
//! signe « numéro », fenêtres de lecture) que `fond`, `ce`, `ta_caa`, `cc` et `ecli` reprennent.
//!
//! Les grammaires sont écrites à la main sur une vue en caractères : le texte normalisé (casse
//! significative) et la vue clé (repliée, un caractère pour un caractère) partagent les mêmes
//! indices. Les plages des candidats sont des offsets du texte d'ORIGINE
//! (`Normalized::orig_range`), celles des ancres des offsets de `Normalized::text`.

#![allow(
    clippy::many_single_char_names,
    reason = "lecteur à curseur : `c` (fenêtre) et `i`, `s`, `p`, `e` (positions), comme dans la grammaire de l'annexe B"
)]

use crate::citation::dict::courts::CassChamber;
use crate::citation::normalize::Normalized;
use crate::citation::scan::{Anchor, AnchorClass};
use crate::citation::types::{
    Ambiguity, BulletinRef, CaseNumber, Confidence, Court, CourtVia, DecisionCite, DecisionKind,
    Flag, Pourvoi, Publication,
};

use self::lex::{Cur, Hit};
use super::Candidate;

/// Lexique commun des grammaires de juridictions (annexe B §4.0 et §4.1).
pub(crate) mod lex {
    use std::collections::BTreeSet;

    use crate::citation::grammar::{Candidate, Flags};
    use crate::citation::normalize::Normalized;
    use crate::citation::scan::{Anchor, AnchorClass};
    use crate::citation::types::{
        Ambiguity, CaseNumber, CitationRef, Confidence, Court, DecisionCite, Flag,
    };
    use crate::date::CivilDate;

    /// Caractères lus avant une ancre (§4.16).
    const BEFORE: usize = 80;
    /// Caractères lus à partir d'une ancre (§4.16).
    const AFTER: usize = 400;
    /// Débuts de mot essayés avant une ancre (§4.16).
    const MAX_STARTS: usize = 12;
    /// Mots d'un nom de lieu.
    const MAX_PLACE_WORDS: usize = 6;
    /// Caractères d'un mot d'un nom de lieu.
    const MAX_PLACE_CHARS: usize = 60;

    /// Vue en caractères d'un texte normalisé.
    pub(crate) struct Doc<'a> {
        n: &'a Normalized,
        t: Vec<char>,
        k: Vec<char>,
        at: Vec<usize>,
    }

    /// Fenêtre de lecture : le texte et la vue clé, coupés à la fin de la fenêtre. Les indices
    /// restent ceux du document entier.
    #[derive(Clone, Copy)]
    pub(crate) struct Cur<'a> {
        t: &'a [char],
        k: &'a [char],
    }

    /// Une citation trouvée par une grammaire : indices de caractères, valeur, confiance,
    /// drapeaux et ambiguïté.
    #[derive(Debug)]
    pub(crate) struct Hit {
        pub(crate) start: usize,
        pub(crate) end: usize,
        pub(crate) value: CitationRef,
        pub(crate) confidence: Confidence,
        pub(crate) flags: Vec<Flag>,
        pub(crate) ambiguity: Option<Ambiguity>,
    }

    /// Une grammaire essayée à un début donné.
    pub(crate) type Grammar = fn(&Cur<'_>, usize) -> Option<Hit>;

    /// Prédicat de début propre à une famille, pour les citations sans ancre (N21, N28, P148).
    pub(crate) type Trigger = fn(&Cur<'_>, usize) -> bool;

    /// Une date lue.
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct DateHit {
        pub(crate) end: usize,
        pub(crate) date: Option<CivilDate>,
        pub(crate) flags: Vec<Flag>,
    }

    impl Hit {
        /// Citation de confiance haute, sans drapeau.
        pub(crate) fn new(start: usize, end: usize, value: CitationRef) -> Self {
            Self {
                start,
                end,
                value,
                confidence: Confidence::High,
                flags: Vec::new(),
                ambiguity: None,
            }
        }

        /// Décision de `start` à `end`.
        pub(crate) fn decision(start: usize, end: usize, decision: DecisionCite) -> Self {
            Self::new(start, end, CitationRef::Decision(decision))
        }

        /// Même citation, autre confiance.
        pub(crate) fn with_confidence(mut self, confidence: Confidence) -> Self {
            self.confidence = confidence;
            self
        }

        /// Même citation, avec ces drapeaux de plus.
        pub(crate) fn with_flags(mut self, flags: impl IntoIterator<Item = Flag>) -> Self {
            self.flags.extend(flags);
            self
        }

        /// Même citation, avec cette ambiguïté.
        pub(crate) fn with_ambiguity(mut self, ambiguity: Ambiguity) -> Self {
            self.ambiguity = Some(ambiguity);
            self
        }
    }

    /// Décision vide d'une juridiction.
    pub(crate) fn decision(court: Option<Court>) -> DecisionCite {
        DecisionCite {
            court,
            court_via: None,
            date: None,
            numbers: Vec::new(),
            kind: None,
            publication: None,
            solution: None,
            parties: None,
            subject: None,
            arret_num: None,
            formation_code: None,
            publication_codes: Vec::new(),
        }
    }

    /// Fonction qui lit des numéros d'affaire : fin de la lecture et numéros.
    pub(crate) type NumbersFn = fn(&Cur<'_>, usize) -> Option<(usize, Vec<CaseNumber>)>;

    /// Fonction qui rend les fins possibles d'une chambre, de la plus longue à la plus courte.
    pub(crate) type ChambersFn = fn(&Cur<'_>, usize) -> Vec<usize>;

    /// Liste de numéros `un { LIST_SEP un }` : fin de la lecture et numéros.
    pub(crate) fn number_list(
        c: &Cur<'_>,
        i: usize,
        one: fn(&Cur<'_>, usize) -> Option<(usize, CaseNumber)>,
    ) -> Option<(usize, Vec<CaseNumber>)> {
        let (mut end, first) = one(c, i)?;
        let mut numbers = vec![first];
        while let Some((e, next)) = c.list_sep(end).and_then(|p| one(c, p)) {
            numbers.push(next);
            end = e;
        }
        Some((end, numbers))
    }

    /// Une juridiction du fond lue après son nom : lieu, chambre, date et numéros.
    pub(crate) struct Located<S> {
        pub(crate) seat: S,
        pub(crate) seat_end: usize,
        pub(crate) chamber: Option<String>,
        pub(crate) end: usize,
        pub(crate) date: DateHit,
        pub(crate) numbers: Vec<CaseNumber>,
    }

    impl<S> Located<S> {
        /// Décision de `start` à la fin de la lecture, pour `court`.
        pub(crate) fn hit(self, start: usize, court: Court) -> Hit {
            let mut decision = decision(Some(court));
            decision.date = self.date.date;
            decision.numbers = self.numbers;
            Hit::decision(start, self.end, decision).with_flags(self.date.flags)
        }
    }

    /// Lieu reconnu par `seat_of` à partir de `p`, puis `SEP [chambre SEP] DATE_LINK SEP
    /// numéros`. Le lieu le plus long qui mène à une décision complète l'emporte.
    pub(crate) fn at_place<S>(
        c: &Cur<'_>,
        p: usize,
        chambers: ChambersFn,
        numbers: NumbersFn,
        seat_of: impl Fn(&str) -> Option<S>,
    ) -> Option<Located<S>> {
        let dated = |from: usize| {
            let date = c.date_link(from)?;
            let (end, numbers) = numbers(c, c.sep(date.end)?)?;
            Some((end, date, numbers))
        };
        c.place_ends(p).into_iter().find_map(|seat_end| {
            let seat = seat_of(&c.key_text(p, seat_end))?;
            let q = c.sep(seat_end)?;
            let with_chamber = chambers(c, q).into_iter().find_map(|e| {
                let tail = dated(c.sep(e)?)?;
                Some((Some(c.text(q, e)), tail))
            });
            let (chamber, (end, date, numbers)) =
                with_chamber.or_else(|| dated(q).map(|tail| (None, tail)))?;
            Some(Located { seat, seat_end, chamber, end, date, numbers })
        })
    }

    impl<'a> Doc<'a> {
        /// Vue en caractères de `n`.
        pub(crate) fn new(n: &'a Normalized) -> Self {
            let mut at: Vec<usize> = n.text.char_indices().map(|(i, _)| i).collect();
            at.push(n.text.len());
            Self { n, t: n.text.chars().collect(), k: n.key.chars().collect(), at }
        }

        /// Nombre de caractères du texte.
        pub(crate) fn len(&self) -> usize {
            self.t.len()
        }

        /// Indice de caractère de l'offset en octets `byte`, s'il tombe sur une frontière.
        pub(crate) fn char_at(&self, byte: usize) -> Option<usize> {
            self.at.binary_search(&byte).ok()
        }

        /// Fenêtre de lecture qui commence à `start` : jusqu'à `AFTER + BEFORE` caractères.
        pub(crate) fn cur(&self, start: usize) -> Cur<'_> {
            let end = self.t.len().min(start.saturating_add(AFTER + BEFORE));
            Cur {
                t: self.t.get(..end).unwrap_or_default(),
                k: self.k.get(..end).unwrap_or_default(),
            }
        }

        /// Début du mot qui contient ou précède `pos` et débuts de mot situés jusqu'à `BEFORE`
        /// caractères avant `pos` (au plus `MAX_STARTS`), plus `pos` lui-même.
        pub(crate) fn starts(&self, pos: usize) -> Vec<usize> {
            let word_start = |i: usize| {
                self.t.get(i).is_some_and(|c| c.is_alphanumeric())
                    && i.checked_sub(1)
                        .is_none_or(|p| !self.t.get(p).is_some_and(|c| c.is_alphanumeric()))
            };
            let mut out: Vec<usize> = (pos.saturating_sub(BEFORE)..pos)
                .rev()
                .filter(|&i| word_start(i))
                .take(MAX_STARTS)
                .collect();
            out.push(pos);
            out
        }

        /// Octets de la plage `start..end` du texte normalisé, convertis en plage d'origine.
        fn orig(&self, start: usize, end: usize) -> std::ops::Range<usize> {
            let at = |i: usize| self.at.get(i).copied().unwrap_or(self.n.text.len());
            self.n.orig_range(at(start)..at(end))
        }
    }

    /// Candidats d'une famille : chaque grammaire de `grammars` est essayée à l'ancre (classe
    /// de `classes`) et aux débuts de mot qui la précèdent, puis aux positions où `trigger` vaut
    /// vrai.
    /// Les citations contenues dans une autre de la même famille sont écartées.
    pub(crate) fn scan_family(
        n: &Normalized,
        anchors: &[Anchor],
        classes: &[AnchorClass],
        trigger: Option<Trigger>,
        grammars: &[Grammar],
    ) -> Vec<Candidate> {
        let relevant: Vec<&Anchor> =
            anchors.iter().filter(|a| classes.contains(&a.class)).collect();
        if relevant.is_empty() && trigger.is_none() {
            return Vec::new();
        }
        let doc = Doc::new(n);
        let mut positions: BTreeSet<usize> =
            relevant.iter().filter_map(|a| doc.char_at(a.range.start)).collect();
        if let Some(is_start) = trigger {
            positions.extend((0..doc.len()).filter(|&i| is_start(&doc.cur(i), i)));
        }
        let mut tried = BTreeSet::new();
        let mut hits = Vec::new();
        for start in positions.into_iter().flat_map(|pos| doc.starts(pos)) {
            if !tried.insert(start) {
                continue;
            }
            let cur = doc.cur(start);
            if cur.left_ok(start) {
                hits.extend(grammars.iter().filter_map(|grammar| grammar(&cur, start)));
            }
        }
        finish(&doc, hits)
    }

    /// Tri par début puis longueur décroissante, sans les citations contenues dans une autre.
    fn finish(doc: &Doc<'_>, mut hits: Vec<Hit>) -> Vec<Candidate> {
        hits.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let mut reach: Option<usize> = None;
        let mut out = Vec::new();
        for hit in hits {
            if reach.is_some_and(|r| hit.end <= r) {
                continue;
            }
            reach = Some(hit.end);
            let mut candidate = Candidate::new(doc.orig(hit.start, hit.end), hit.value);
            candidate.confidence = hit.confidence;
            candidate.flags = Flags {
                list: hit.flags,
                ambiguity: hit.ambiguity,
                enum_group: None,
                group_extent: None,
            };
            out.push(candidate);
        }
        out
    }

    /// Mois reconnus : mot replié, numéro, vrai si c'est un nom entier (sans point).
    const MONTHS: [(&str, u8, bool); 22] = [
        ("janvier", 1, true),
        ("janv", 1, false),
        ("fevrier", 2, true),
        ("fevr", 2, false),
        ("fev", 2, false),
        ("mars", 3, true),
        ("avril", 4, true),
        ("avr", 4, false),
        ("mai", 5, true),
        ("juin", 6, true),
        ("juillet", 7, true),
        ("juill", 7, false),
        ("juil", 7, false),
        ("aout", 8, true),
        ("septembre", 9, true),
        ("sept", 9, false),
        ("octobre", 10, true),
        ("oct", 10, false),
        ("novembre", 11, true),
        ("nov", 11, false),
        ("decembre", 12, true),
        ("dec", 12, false),
    ];

    /// Mois du calendrier républicain.
    const REPUBLICAN: [&str; 12] = [
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

    /// Mots d'ordinal en toutes lettres (sans `dernier`, qui n'est pas une chambre).
    const ORDINAL_WORDS: [(&str, u8); 13] = [
        ("deuxieme", 2),
        ("second", 2),
        ("seconde", 2),
        ("troisieme", 3),
        ("quatrieme", 4),
        ("cinquieme", 5),
        ("sixieme", 6),
        ("septieme", 7),
        ("huitieme", 8),
        ("neuvieme", 9),
        ("dixieme", 10),
        ("onzieme", 11),
        ("douzieme", 12),
    ];

    /// Mots de la vue clé qui, juste avant un pourvoi, annoncent une juridiction ou une chambre
    /// de la Cour de cassation non lue.
    const HEAD_WORDS: [&str; 21] = [
        "cass",
        "cassation",
        "crim",
        "civ",
        "soc",
        "com",
        "plen",
        "mixte",
        "reunies",
        "criminelle",
        "sociale",
        "commerciale",
        "civile",
        "req",
        "requetes",
        "1re",
        "1ere",
        "2e",
        "2eme",
        "3e",
        "3eme",
    ];

    impl Cur<'_> {
        /// Caractère du texte normalisé.
        pub(crate) fn ch(&self, i: usize) -> Option<char> {
            self.t.get(i).copied()
        }

        /// Caractère de la vue clé.
        fn key(&self, i: usize) -> Option<char> {
            self.k.get(i).copied()
        }

        /// Vrai si le texte porte `c` en `i`.
        pub(crate) fn is(&self, i: usize, c: char) -> bool {
            self.ch(i) == Some(c)
        }

        /// Vrai si un chiffre ASCII est en `i`.
        pub(crate) fn digit(&self, i: usize) -> bool {
            self.ch(i).is_some_and(|c| c.is_ascii_digit())
        }

        fn alnum(&self, i: usize) -> bool {
            self.ch(i).is_some_and(char::is_alphanumeric)
        }

        /// `B` : le caractère en `i` n'est ni une lettre ni un chiffre.
        pub(crate) fn bound(&self, i: usize) -> bool {
            !self.alnum(i)
        }

        /// Le caractère qui précède `i` n'est ni une lettre ni un chiffre.
        pub(crate) fn left_ok(&self, i: usize) -> bool {
            i.checked_sub(1).is_none_or(|p| !self.alnum(p))
        }

        /// Fin des espaces à partir de `i` (zéro ou plus).
        pub(crate) fn ws0(&self, mut i: usize) -> usize {
            while self.is(i, ' ') {
                i += 1;
            }
            i
        }

        /// `WS` : une ou plusieurs espaces.
        pub(crate) fn ws(&self, i: usize) -> Option<usize> {
            let end = self.ws0(i);
            (end > i).then_some(end)
        }

        /// `SEP` : `WS? ("," | ";" | ":") WS?` ou `WS`.
        pub(crate) fn sep(&self, i: usize) -> Option<usize> {
            let j = self.ws0(i);
            if matches!(self.ch(j), Some(',' | ';' | ':')) {
                Some(self.ws0(j + 1))
            } else {
                (j > i).then_some(j)
            }
        }

        /// `LIST_SEP` : virgule, `et` ou `ou`.
        pub(crate) fn list_sep(&self, i: usize) -> Option<usize> {
            let j = self.ws0(i);
            if self.is(j, ',') {
                let k = self.ws0(j + 1);
                return Some(self.kwb(k, "et").and_then(|e| self.ws(e)).unwrap_or(k));
            }
            if j == i {
                return None;
            }
            ["et", "ou"].iter().find_map(|word| self.kwb(j, word).and_then(|e| self.ws(e)))
        }

        /// Mot-clé replié : chaque espace du gabarit vaut une ou plusieurs espaces du texte.
        pub(crate) fn kw(&self, i: usize, needle: &str) -> Option<usize> {
            let mut p = i;
            for c in needle.chars() {
                if c == ' ' {
                    p = self.ws(p)?;
                } else {
                    (self.key(p)? == c).then_some(())?;
                    p += 1;
                }
            }
            Some(p)
        }

        /// [`Cur::kw`] suivi d'une frontière de mot.
        pub(crate) fn kwb(&self, i: usize, needle: &str) -> Option<usize> {
            self.kw(i, needle).filter(|&e| self.bound(e))
        }

        /// Point final d'une abréviation (inclus s'il est là), sinon frontière de mot.
        pub(crate) fn dotb(&self, e: usize) -> Option<usize> {
            if self.is(e, '.') { Some(e + 1) } else { self.bound(e).then_some(e) }
        }

        /// Abréviation repliée, point final facultatif (P017).
        pub(crate) fn abbr(&self, i: usize, needle: &str) -> Option<usize> {
            self.kw(i, needle).and_then(|e| self.dotb(e))
        }

        /// Point facultatif puis espaces, entre deux mots d'une abréviation (`ass. plén.`).
        pub(crate) fn dot_ws(&self, e: usize) -> usize {
            self.ws0(if self.is(e, '.') { e + 1 } else { e })
        }

        /// Littéral sensible à la casse, sur le texte normalisé.
        pub(crate) fn lit(&self, i: usize, needle: &str) -> Option<usize> {
            let mut p = i;
            for c in needle.chars() {
                (self.ch(p)? == c).then_some(())?;
                p += 1;
            }
            Some(p)
        }

        /// [`Cur::lit`] suivi d'une frontière de mot.
        pub(crate) fn litb(&self, i: usize, needle: &str) -> Option<usize> {
            self.lit(i, needle).filter(|&e| self.bound(e))
        }

        /// Texte normalisé de `i` à `j`.
        pub(crate) fn text(&self, i: usize, j: usize) -> String {
            self.t.get(i..j).unwrap_or_default().iter().collect()
        }

        /// Vue clé de `i` à `j`.
        pub(crate) fn key_text(&self, i: usize, j: usize) -> String {
            self.k.get(i..j).unwrap_or_default().iter().collect()
        }

        /// Entre `min` et `max` chiffres ASCII à partir de `i` : fin de la lecture.
        pub(crate) fn digits(&self, i: usize, min: usize, max: usize) -> Option<usize> {
            let n = (0..max).take_while(|&k| self.digit(i + k)).count();
            (n >= min).then_some(i + n)
        }

        /// Exactement `n` chiffres, non suivis d'un chiffre.
        pub(crate) fn exact(&self, i: usize, n: usize) -> Option<usize> {
            self.digits(i, n, n).filter(|&e| !self.digit(e))
        }

        /// Valeur des chiffres de `i` à `j`.
        pub(crate) fn val(&self, i: usize, j: usize) -> u32 {
            self.t.get(i..j).unwrap_or_default().iter().fold(0_u32, |acc, c| {
                acc.saturating_mul(10).saturating_add(c.to_digit(10).unwrap_or(0))
            })
        }

        /// Entier de `i` : fin et valeur.
        pub(crate) fn int(&self, i: usize) -> Option<(usize, u32)> {
            let e = self.digits(i, 1, 9)?;
            Some((e, self.val(i, e)))
        }

        /// Fin d'un numéro de dossier : frontière de mot, sans suite `-N`, `/N` ou `.N`.
        pub(crate) fn num_end_ok(&self, e: usize) -> bool {
            self.bound(e) && !(matches!(self.ch(e), Some('-' | '/' | '.')) && self.digit(e + 1))
        }

        /// `ROMAN` : majuscules romaines, frontière de mot.
        pub(crate) fn roman(&self, i: usize) -> Option<usize> {
            let n = (0..8)
                .take_while(|&k| self.ch(i + k).is_some_and(|c| "IVXLCDM".contains(c)))
                .count();
            (n > 0).then_some(i + n).filter(|&e| self.bound(e))
        }

        /// `YEAR4` : année de 1500 à 2099.
        pub(crate) fn year4(&self, i: usize) -> Option<(usize, u16)> {
            let e = self.exact(i, 4)?;
            let v = u16::try_from(self.val(i, e)).unwrap_or(0);
            (1500..=2099).contains(&v).then_some((e, v))
        }

        /// Ordinal : `1re`, `1ère`, `première`, `1er`, `premier`, `2e`, `2ème`, `deuxième`…
        pub(crate) fn ordinal(&self, i: usize) -> Option<(usize, u8)> {
            if !self.left_ok(i) {
                return None;
            }
            let first =
                ["premiere", "premier", "1ere", "1re", "1er"].iter().find_map(|w| self.kwb(i, w));
            if let Some(e) = first {
                return Some((e, 1));
            }
            let words = ORDINAL_WORDS.iter().find_map(|(w, n)| self.kwb(i, w).map(|e| (e, *n)));
            if words.is_some() {
                return words;
            }
            let d = self.digits(i, 1, 2)?;
            let n = u8::try_from(self.val(i, d)).unwrap_or(0);
            let e = self.kwb(d, "eme").or_else(|| self.kwb(d, "e"))?;
            (n >= 2).then_some((e, n))
        }

        /// `NUM_SIGN` (`n°`, `no`, `num.`, `numéro`) ou, si `plural`, `NUM_SIGN_PL`, suivi
        /// d'espaces facultatifs : fin de la lecture et vrai pour un pluriel. `no` et `nos`
        /// exigent un chiffre derrière.
        pub(crate) fn num_sign(&self, i: usize, plural: bool) -> Option<(usize, bool)> {
            if !self.left_ok(i) {
                return None;
            }
            let after = |e: usize| self.ws0(e);
            let pl = if plural {
                self.kw(i, "numeros")
                    .or_else(|| self.kw(i, "n°s"))
                    .or_else(|| self.kw(i, "nos").filter(|&e| self.digit(after(e))))
            } else {
                None
            };
            if let Some(e) = pl {
                return Some((after(e), true));
            }
            let one = self
                .kw(i, "numero")
                .or_else(|| self.kw(i, "num."))
                .or_else(|| self.kw(i, "n°"))
                .or_else(|| self.kw(i, "no").filter(|&e| self.digit(after(e))));
            one.map(|e| (after(e), false))
        }

        /// Un mot de la vue clé (lettres seules) à partir de `i`.
        fn word(&self, i: usize) -> (usize, String) {
            let n =
                (0..30).take_while(|&k| self.key(i + k).is_some_and(char::is_alphabetic)).count();
            (i + n, self.key_text(i, i + n))
        }

        /// `DAY` : `1er`, `premier` ou un jour de 1 à 31.
        fn day(&self, i: usize) -> Option<(usize, u8)> {
            if let Some(e) = self.kwb(i, "premier").or_else(|| self.kwb(i, "1er")) {
                return Some((e, 1));
            }
            let e = self.digits(i, 1, 2)?;
            let v = u8::try_from(self.val(i, e)).unwrap_or(0);
            ((1..=31).contains(&v) && self.bound(e)).then_some((e, v))
        }

        /// Mois et point final facultatif d'une abréviation.
        fn month(&self, i: usize) -> Option<(usize, u8)> {
            let (end, word) = self.word(i);
            let &(_, month, full) = MONTHS.iter().find(|(w, _, _)| *w == word)?;
            Some((if !full && self.is(end, '.') { end + 1 } else { end }, month))
        }

        /// Mois du calendrier républicain.
        fn republican_month(&self, i: usize) -> Option<usize> {
            let (end, word) = self.word(i);
            REPUBLICAN.contains(&word.as_str()).then_some(end)
        }

        /// Valeur d'une date lue : date civile ou `date_invalide`.
        fn dated(end: usize, y: u16, m: u8, d: u8) -> DateHit {
            let date = CivilDate::new(y, m, d);
            let flags = if date.is_none() { vec![Flag::DateInvalide] } else { Vec::new() };
            DateHit { end, date, flags }
        }

        /// `DATE_PROSE` : `7 octobre 2016`, `1er janv. 2018`.
        fn date_prose(&self, i: usize) -> Option<DateHit> {
            let (e, d) = self.day(i)?;
            let (e, m) = self.month(self.ws(e)?)?;
            let (e, y) = self.year4(self.ws(e)?)?;
            Some(Self::dated(e, y, m, d))
        }

        /// `DATE_NUM` : `29/05/2026`, `9.6.2026`, `29-05-2026`. Le tiret ne vaut qu'aux deux
        /// séparateurs à la fois : `12/05-2020` n'est pas une date.
        fn date_num(&self, i: usize) -> Option<DateHit> {
            let e = self.digits(i, 1, 2)?;
            let d = u8::try_from(self.val(i, e)).unwrap_or(0);
            let dashed = self.is(e, '-');
            (dashed || matches!(self.ch(e), Some('/' | '.'))).then_some(())?;
            (1..=31).contains(&d).then_some(())?;
            let ms = e + 1;
            let me = self.digits(ms, 1, 2)?;
            let m = u8::try_from(self.val(ms, me)).unwrap_or(0);
            let second =
                if dashed { self.is(me, '-') } else { matches!(self.ch(me), Some('/' | '.')) };
            (second && (1..=12).contains(&m)).then_some(())?;
            let (e, y) = self.year4(me + 1)?;
            Some(Self::dated(e, y, m, d))
        }

        /// `DATE_ISO` : `2026-04-30`.
        fn date_iso(&self, i: usize) -> Option<DateHit> {
            let (e, y) = self.year4(i)?;
            self.is(e, '-').then_some(())?;
            let me = self.digits(e + 1, 2, 2)?;
            let m = u8::try_from(self.val(e + 1, me)).unwrap_or(0);
            (self.is(me, '-') && (1..=12).contains(&m)).then_some(())?;
            let de = self.exact(me + 1, 2)?;
            let d = u8::try_from(self.val(me + 1, de)).unwrap_or(0);
            (1..=31).contains(&d).then_some(())?;
            Some(Self::dated(de, y, m, d))
        }

        /// `DATE_REPU` : gardée brute, `date_republicaine`.
        fn date_republican(&self, i: usize) -> Option<DateHit> {
            let (e, _) = self.day(i)?;
            let e = self.republican_month(self.ws(e)?)?;
            let e = self.kwb(self.ws(e)?, "an")?;
            let e = self.roman(self.ws(e)?)?;
            Some(DateHit { end: e, date: None, flags: vec![Flag::DateRepublicaine] })
        }

        /// `DATE` : ISO, numérique, en toutes lettres ou républicaine.
        pub(crate) fn date(&self, i: usize) -> Option<DateHit> {
            self.date_iso(i)
                .or_else(|| self.date_num(i))
                .or_else(|| self.date_prose(i))
                .or_else(|| self.date_republican(i))
        }

        /// `DATE_LINK` : `[du | en date du | le | des] DATE`.
        pub(crate) fn date_link(&self, i: usize) -> Option<DateHit> {
            ["en date du", "du", "le", "des"]
                .iter()
                .find_map(|w| self.kwb(i, w).and_then(|e| self.ws(e)).and_then(|p| self.date(p)))
                .or_else(|| self.date(i))
        }

        /// Fins possibles d'un nom de lieu lu à partir de `i` (mots, tirets et apostrophes,
        /// séparés par une espace), de la plus longue à la plus courte.
        pub(crate) fn place_ends(&self, i: usize) -> Vec<usize> {
            let mut ends = Vec::new();
            let mut p = i;
            for _ in 0..MAX_PLACE_WORDS {
                let n = (0..MAX_PLACE_CHARS)
                    .take_while(|&k| {
                        self.ch(p + k).is_some_and(|c| c.is_alphabetic() || c == '-' || c == '\'')
                    })
                    .count();
                if n == 0 {
                    break;
                }
                ends.push(p + n);
                match (self.ch(p + n), self.ch(p + n + 1)) {
                    (Some(' '), Some(c)) if c.is_alphabetic() => p += n + 1,
                    _ => break,
                }
            }
            ends.reverse();
            ends
        }

        /// Complément `de` ou `d'` après un nom qui finit en `e` (`Cour d'appel de`,
        /// `Tribunal administratif d'`) : fin de la lecture, espaces comprises.
        pub(crate) fn de(&self, e: usize) -> Option<usize> {
            let w = self.ws(e)?;
            self.kwb(w, "de").and_then(|x| self.ws(x)).or_else(|| self.kw(w, "d'"))
        }

        /// Caractères de la vue clé avant `end`, du plus proche au plus lointain.
        fn back(&self, end: usize) -> impl Iterator<Item = char> + '_ {
            self.k.get(..end).unwrap_or_default().iter().rev().copied()
        }

        /// Recule depuis `p` tant que le caractère de la vue clé précédent est un séparateur de
        /// tête (espace, ponctuation, parenthèse, crochet, tiret).
        fn back_over_separators(&self, p: usize) -> usize {
            let n = self
                .back(p)
                .take_while(|c| matches!(c, ' ' | ',' | ';' | ':' | '(' | ')' | '[' | ']' | '-'))
                .count();
            p.saturating_sub(n)
        }

        /// Mot (lettres et chiffres de la vue clé) qui finit en `end`, et son début.
        fn word_before(&self, end: usize) -> (usize, String) {
            let start =
                end.saturating_sub(self.back(end).take_while(|c| c.is_alphanumeric()).count());
            (start, self.key_text(start, end))
        }

        /// La séquence de chiffres et de séparateurs `/`, `.`, `-` qui finit en `end` est-elle une
        /// date : une année de quatre chiffres entre 1500 et 2099, ou `j/m/aa` ?
        fn date_run_before(&self, end: usize) -> bool {
            let n = self
                .back(end)
                .take_while(|c| c.is_ascii_digit() || matches!(c, '/' | '.' | '-'))
                .count();
            let run = self.key_text(end.saturating_sub(n), end);
            let groups: Vec<&str> =
                run.trim_start_matches(['/', '.', '-']).split(['/', '.', '-']).collect();
            match groups.as_slice() {
                [.., year] if year.len() == 4 => {
                    year.parse::<u16>().is_ok_and(|y| (1500..=2099).contains(&y))
                }
                [day, month, year] => {
                    year.len() == 2
                        && (1..=2).contains(&day.len())
                        && (1..=2).contains(&month.len())
                }
                _ => false,
            }
        }

        /// Vrai si le pourvoi qui commence en `s` est précédé, après les séparateurs et un mot
        /// `pourvoi` facultatif, d'une date (année de quatre chiffres, `j/m/aa`) ou d'un sigle de
        /// juridiction ou de chambre : la tête de la décision n'a pas été lue (U12, F1). Un point
        /// de fin de phrase derrière une date n'en fait pas une tête. Lecture linéaire : chaque
        /// retour en arrière s'arrête au premier caractère qui n'est pas de sa classe.
        pub(crate) fn head_before(&self, s: usize) -> bool {
            let mut p = self.back_over_separators(s);
            let (start, word) = self.word_before(p);
            if matches!(word.as_str(), "pourvoi" | "pourvois") {
                p = self.back_over_separators(start);
            }
            let dotted = self.back(p).next() == Some('.');
            let end = if dotted { p.saturating_sub(1) } else { p };
            match self.back(end).next() {
                Some(c) if c.is_ascii_digit() => !dotted && self.date_run_before(end),
                Some(_) => HEAD_WORDS.contains(&self.word_before(end).1.as_str()),
                None => false,
            }
        }

        /// `adm_chamber` : ordinal, `chambre` ou `ch.`, qualificatif facultatif de 40 caractères
        /// au plus. Fins possibles, de la plus longue à la plus courte.
        pub(crate) fn ordinal_chamber_ends(&self, i: usize) -> Vec<usize> {
            let Some((e, _)) = self.ordinal(i) else { return Vec::new() };
            let Some(keyword) =
                self.ws(e).and_then(|w| self.kwb(w, "chambre").or_else(|| self.abbr(w, "ch")))
            else {
                return Vec::new();
            };
            let mut ends = Vec::new();
            if let Some(w) = self.ws(keyword) {
                let n = (0..40)
                    .take_while(|&k| {
                        self.ch(w + k)
                            .is_some_and(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\''))
                    })
                    .count();
                let trimmed = (w..w + n).rev().find(|&p| !self.is(p, ' ')).map(|p| p + 1);
                ends.extend(trimmed);
            }
            ends.push(keyword);
            ends
        }
    }
}

/// Classes d'ancres où la Cour de cassation peut commencer.
const CLASSES: [AnchorClass; 4] =
    [AnchorClass::Court, AnchorClass::Sigle, AnchorClass::Pourvoi, AnchorClass::Sink];

/// Grammaires de la famille, essayées à chaque début.
const GRAMMARS: [lex::Grammar; 7] = [
    short_form,
    legifrance_title,
    long_chamber,
    parenthesised,
    header,
    bulletin_alone,
    pourvoi_alone,
];

/// Candidats de la famille, tirés des ancres compatibles avec ses grammaires.
pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    lex::scan_family(n, anchors, &CLASSES, None, &GRAMMARS)
}

/// Chambre lue, avec son drapeau éventuel.
type Chamber = (usize, Option<CassChamber>, Option<Flag>);

/// Chambre civile numérotée : de 1 à 3, sinon `chambre_inexistante`.
fn civil(n: u8) -> (Option<CassChamber>, Option<Flag>) {
    match n {
        1 => (Some(CassChamber::Civ1), None),
        2 => (Some(CassChamber::Civ2), None),
        3 => (Some(CassChamber::Civ3), None),
        _ => (None, Some(Flag::ChambreInexistante(format!("civ{n}")))),
    }
}

/// `civ_n` : `1re civ.`, `2e ch. civ.`, `1re chambre civ.`, `1re ch. civile`, `civ. 1re`,
/// `civ.1re`, `civ. 4e`, `civ. 1`.
fn civ_number(c: &Cur<'_>, i: usize) -> Option<(usize, u8)> {
    let before = c.ordinal(i).and_then(|(e, n)| {
        let w = c.ws(e)?;
        let plain = c.abbr(w, "civ");
        let long = c.kwb(w, "chambre").and_then(|k| c.ws(k)).and_then(|k| c.abbr(k, "civ"));
        let short = c
            .abbr(w, "ch")
            .and_then(|k| c.ws(k))
            .and_then(|k| c.abbr(k, "civ").or_else(|| c.kwb(k, "civile")));
        plain.or(long).or(short).map(|end| (end, n))
    });
    let after = c.abbr(i, "civ").map(|e| c.ws0(e)).and_then(|w| {
        c.ordinal(w).or_else(|| {
            let e = c.exact(w, 1).filter(|&e| c.bound(e) && c.date(w).is_none())?;
            Some((e, u8::try_from(c.val(w, e)).unwrap_or(0)))
        })
    });
    before.or(after)
}

/// `cass_chamber` : `civ_n`, `civ.`, `com.`, `soc.`, `crim.`, `ass. plén.`, `ch. mixte`,
/// `ch. réunies`, `req.`, `AP`.
fn chamber_short(c: &Cur<'_>, i: usize) -> Option<Chamber> {
    if let Some((e, n)) = civ_number(c, i) {
        let (chamber, flag) = civil(n);
        return Some((e, chamber, flag));
    }
    let plain = [
        ("civ", CassChamber::CivUnique),
        ("com", CassChamber::Com),
        ("soc", CassChamber::Soc),
        ("crim", CassChamber::Crim),
        ("req", CassChamber::Requetes),
    ];
    let compound = [
        c.kw(i, "ass").and_then(|e| c.abbr(c.dot_ws(e), "plen")).map(|e| (e, CassChamber::AssPlen)),
        c.kw(i, "ch").and_then(|e| c.kwb(c.dot_ws(e), "mixte")).map(|e| (e, CassChamber::Mixte)),
        c.kw(i, "ch")
            .and_then(|e| c.kwb(c.dot_ws(e), "reunies"))
            .map(|e| (e, CassChamber::Reunies)),
        c.litb(i, "AP").map(|e| (e, CassChamber::AssPlen)),
    ];
    let found =
        plain.iter().map(|(w, ch)| c.abbr(i, w).map(|e| (e, *ch))).chain(compound).flatten().next();
    found.map(|(e, chamber)| (e, Some(chamber), None))
}

/// `cass_chamber_long` : `première chambre civile`, `chambre sociale`, `assemblée plénière`…
fn chamber_long(c: &Cur<'_>, i: usize) -> Option<Chamber> {
    let numbered = [("premiere", 1), ("deuxieme", 2), ("troisieme", 3)]
        .iter()
        .find_map(|(w, n)| {
            c.kwb(i, w).and_then(|e| c.kwb(c.ws(e)?, "chambre civile")).map(|e| (e, *n))
        })
        .or_else(|| {
            let (e, n) = c.ordinal(i).filter(|_| c.digit(i))?;
            c.kwb(c.ws(e)?, "chambre civile").map(|end| (end, n))
        });
    if let Some((e, n)) = numbered {
        let (chamber, flag) = civil(n);
        return Some((e, chamber, flag));
    }
    let table = [
        ("chambre commerciale", CassChamber::Com),
        ("chambre sociale", CassChamber::Soc),
        ("chambre criminelle", CassChamber::Crim),
        ("assemblee pleniere", CassChamber::AssPlen),
        ("chambre mixte", CassChamber::Mixte),
        ("chambres reunies", CassChamber::Reunies),
        ("ordonnance du premier president", CassChamber::PremierPresident),
    ];
    let (e, chamber) = table.iter().find_map(|(w, ch)| c.kwb(i, w).map(|e| (e, *ch)))?;
    let p = c.ws0(e);
    let p = if c.is(p, ',') { c.ws0(p + 1) } else { p };
    let extended = c
        .kwb(p, "economique et financiere")
        .or_else(|| c.kwb(p, "financiere et economique"))
        .filter(|_| chamber == CassChamber::Com);
    Some((extended.unwrap_or(e), Some(chamber), None))
}

/// `chamber_lf` : `Chambre civile 1`, `Chambre commerciale`, `Chambre sociale`.
fn chamber_lf(c: &Cur<'_>, i: usize) -> Option<Chamber> {
    let p = c.ws(c.kwb(i, "chambre")?)?;
    if let Some(w) = c.kwb(p, "civile").and_then(|e| c.ws(e)) {
        let e = c.exact(w, 1).filter(|&e| c.bound(e))?;
        let (chamber, flag) = civil(u8::try_from(c.val(w, e)).unwrap_or(0));
        return Some((e, chamber, flag));
    }
    let table = [
        ("commerciale", CassChamber::Com),
        ("sociale", CassChamber::Soc),
        ("criminelle", CassChamber::Crim),
    ];
    table.iter().find_map(|(w, ch)| c.kwb(p, w).map(|e| (e, Some(*ch), None)))
}

/// Chambre sous toutes ses écritures.
fn chamber_any(c: &Cur<'_>, i: usize) -> Option<Chamber> {
    chamber_short(c, i).or_else(|| chamber_long(c, i)).or_else(|| chamber_lf(c, i))
}

/// `cass_court` : `Cass.`, `Cour de cassation`, `C. cass.`.
fn court(c: &Cur<'_>, i: usize) -> Option<usize> {
    c.kwb(i, "cour de cassation")
        .or_else(|| c.kw(i, "c.").and_then(|e| c.abbr(c.ws0(e), "cass")))
        .or_else(|| c.abbr(i, "cass"))
}

/// `kind` : `avis`, `ord.`, `ordonnance`, `arrêt`.
fn kind_word(c: &Cur<'_>, i: usize) -> Option<(usize, DecisionKind)> {
    c.kwb(i, "avis")
        .map(|e| (e, DecisionKind::Avis))
        .or_else(|| c.kwb(i, "ordonnance").map(|e| (e, DecisionKind::Ordonnance)))
        .or_else(|| c.abbr(i, "ord").map(|e| (e, DecisionKind::Ordonnance)))
        .or_else(|| c.kwb(i, "arret").map(|e| (e, DecisionKind::Arret)))
}

/// `pourvoi` : `AA-NN.NNN` ou `AA-NNNNN`, sans lettre-clé.
fn pourvoi(c: &Cur<'_>, i: usize) -> Option<(usize, Pourvoi)> {
    if !c.left_ok(i) {
        return None;
    }
    let yy_end = c.digits(i, 2, 2).filter(|&e| c.is(e, '-'))?;
    let mid_end = c.digits(yy_end + 1, 2, 2)?;
    let (mid_start, seq_start, end) = if c.is(mid_end, '.') {
        (yy_end + 1, mid_end + 1, c.digits(mid_end + 1, 3, 3)?)
    } else {
        (yy_end + 1, yy_end + 3, c.digits(yy_end + 1, 5, 5)?)
    };
    c.num_end_ok(end).then_some(())?;
    Some((
        end,
        Pourvoi {
            yy: u8::try_from(c.val(i, yy_end)).unwrap_or(0),
            mid: u8::try_from(c.val(mid_start, mid_start + 2)).unwrap_or(0),
            seq: u16::try_from(c.val(seq_start, end)).unwrap_or(0),
            key: None,
        },
    ))
}

/// `[key_letter] pourvoi` : la lettre-clé (`N 24-13.470`) est gardée.
fn keyed_pourvoi(c: &Cur<'_>, i: usize) -> Option<(usize, CaseNumber)> {
    let key = c.ch(i).filter(char::is_ascii_uppercase).and_then(|k| c.ws(i + 1).map(|w| (w, k)));
    let (start, key) = match key {
        Some((w, k)) => (w, Some(k)),
        None => (i, None),
    };
    pourvoi(c, start).map(|(e, p)| (e, CaseNumber::Pourvoi(Pourvoi { key, ..p })))
}

/// `pourvoi { LIST_SEP pourvoi }` avec lettres-clés.
fn pourvoi_list(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let (mut end, first) = keyed_pourvoi(c, i)?;
    let mut numbers = vec![first];
    while let Some((e, next)) = c.list_sep(end).and_then(|p| keyed_pourvoi(c, p)) {
        numbers.push(next);
        end = e;
    }
    Some((end, numbers))
}

/// `cass_numbers` : `[pourvoi] [n°] liste`.
fn cass_numbers(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
    let word = c.kwb(i, "pourvois").or_else(|| c.kwb(i, "pourvoi")).and_then(|e| c.ws(e));
    let p = word.unwrap_or(i);
    let p = c.num_sign(p, true).map_or(p, |(e, _)| e);
    pourvoi_list(c, p)
}

/// `bulletin` : `Bull. civ., I, n° 270`, `Bull. 2007, I, n° 159`. Texte entier de la référence.
fn bulletin(c: &Cur<'_>, i: usize) -> Option<(usize, String)> {
    let mut p = c.abbr(i, "bull")?;
    let kind = c.ws(p).and_then(|w| {
        c.abbr(w, "civ")
            .or_else(|| c.abbr(w, "crim"))
            .or_else(|| c.kw(w, "ass").and_then(|e| c.abbr(c.dot_ws(e), "plen")))
            .or_else(|| c.kw(w, "ch").and_then(|e| c.kwb(c.dot_ws(e), "mixte")))
    });
    p = kind.unwrap_or(p);
    let after_year = c.year4(c.sep(p).unwrap_or(p)).map(|(e, _)| e);
    p = after_year.unwrap_or(p);
    p = c.roman(c.sep(p).unwrap_or(p)).unwrap_or(p);
    let (q, _) = c.num_sign(c.sep(p)?, false)?;
    let (end, _) = c.int(q)?;
    Some((end, c.text(i, end)))
}

/// Une mention de publication.
enum Mention {
    Bulletin(BulletinRef),
    Rapport,
}

/// `publication` : `Publié au bulletin`, `Inédit`, `Bull. …`, `Rapport`.
fn publication(c: &Cur<'_>, i: usize) -> Option<(usize, Mention)> {
    c.kwb(i, "publie au bulletin")
        .map(|e| (e, Mention::Bulletin(BulletinRef::Publie)))
        .or_else(|| c.kwb(i, "inedit").map(|e| (e, Mention::Bulletin(BulletinRef::Inedit))))
        .or_else(|| c.kwb(i, "rapport").map(|e| (e, Mention::Rapport)))
        .or_else(|| {
            bulletin(c, i).map(|(e, raw)| (e, Mention::Bulletin(BulletinRef::Reference(raw))))
        })
}

/// `[WS? "(" solution ")"]` : minuscules, 3 à 40 caractères.
fn solution(c: &Cur<'_>, i: usize) -> Option<(usize, String)> {
    let open = c.ws0(i);
    c.is(open, '(').then_some(())?;
    let n = (1..=41)
        .take_while(|&k| {
            c.ch(open + k).is_some_and(|ch| ch.is_lowercase() || matches!(ch, '\'' | ' ' | '-'))
        })
        .count();
    ((3..=40).contains(&n) && c.is(open + 1 + n, ')'))
        .then(|| (open + n + 2, c.text(open + 1, open + 1 + n)))
}

/// `form_pub` : `FS-B`, `FP-B+R`. Formation et lettres de publication.
fn form_pub(c: &Cur<'_>, i: usize) -> Option<(usize, String, Vec<char>)> {
    let (e, form) =
        ["FRH", "FRR", "FP", "FS", "FM", "F"].iter().find_map(|w| c.lit(i, w).map(|e| (e, *w)))?;
    let mut codes = Vec::new();
    let mut end = e;
    if c.is(e, '-') {
        let mut p = e + 1;
        while let Some(letter) = c.ch(p).filter(|l| "PBRIDL".contains(*l)) {
            codes.push(letter);
            p += 1;
            if c.is(p, '+') && c.ch(p + 1).is_some_and(|l| "PBRIDL".contains(l)) {
                p += 1;
            } else {
                break;
            }
        }
        if !codes.is_empty() {
            end = p;
        }
    }
    let clean = !codes.is_empty() || (!c.is(e, '.') && !c.is(e, '\''));
    (c.bound(end) && clean).then(|| (end, form.to_owned(), codes))
}

/// Ce qui suit la date d'une décision.
struct Tail {
    end: usize,
    numbers: Vec<CaseNumber>,
    publication: Option<Publication>,
    solution: Option<String>,
    formation: Option<(String, Vec<char>)>,
}

/// Jonction de la date (ou de la chambre) au numéro : `SEP`, puis une ouverture facultative `(` ou
/// `[` suivie d'espaces. Rend la position où chercher le numéro et le fermant attendu. Si aucun
/// numéro ne suit l'ouverture, `tail` ne lit rien, comme en 1.1.0 où l'ouverture bloquait le numéro.
fn number_link(c: &Cur<'_>, i: usize) -> Option<(usize, Option<char>)> {
    let p = c.sep(i)?;
    Some(match c.ch(p) {
        Some('(') => (c.ws0(p + 1), Some(')')),
        Some('[') => (c.ws0(p + 1), Some(']')),
        _ => (p, None),
    })
}

/// `SEP ["(" | "["] cass_numbers { SEP publication } [")" | "]"] [solution] [SEP form_pub]`.
fn tail(c: &Cur<'_>, i: usize) -> Tail {
    let mut out =
        Tail { end: i, numbers: Vec::new(), publication: None, solution: None, formation: None };
    let Some((closer, mut end, numbers)) = number_link(c, i)
        .and_then(|(p, closer)| cass_numbers(c, p).map(|(end, numbers)| (closer, end, numbers)))
    else {
        return out;
    };
    out.numbers = numbers;
    while let Some((e, mention)) = c.sep(end).and_then(|p| publication(c, p)) {
        if let Mention::Bulletin(b) = mention {
            out.publication = Some(Publication { bulletin: Some(b), lebon: None });
        }
        end = e;
    }
    if let Some(close) = closer {
        let q = c.ws0(end);
        if c.is(q, close) {
            end = q + 1;
        }
    }
    if let Some((e, text)) = solution(c, end) {
        out.solution = Some(text);
        end = e;
    }
    if let Some((e, form, codes)) = c.sep(end).and_then(|p| form_pub(c, p)) {
        out.formation = Some((form, codes));
        end = e;
    }
    out.end = end;
    out
}

/// Vrai si l'année à deux chiffres du pourvoi tombe 1 à 50 ans après celle de la décision.
fn posterior(yy: u8, year: u16) -> bool {
    (1..=50).contains(&((u16::from(yy) + 100 - year % 100) % 100))
}

/// Applique la fin d'une décision, ses drapeaux et ses ambiguïtés, puis rend la citation.
fn finish(
    start: usize,
    mut dec: DecisionCite,
    date: lex::DateHit,
    tail: Tail,
    mut flags: Vec<Flag>,
) -> Hit {
    dec.date = date.date;
    flags.extend(date.flags);
    let late = dec.date.is_some_and(|d| {
        tail.numbers
            .iter()
            .any(|n| matches!(n, CaseNumber::Pourvoi(p) if posterior(p.yy, d.year())))
    });
    if late {
        flags.push(Flag::AnneePourvoiPosterieure);
    }
    let missing = tail.numbers.is_empty() && dec.arret_num.is_none();
    dec.numbers = tail.numbers;
    dec.publication = tail.publication;
    dec.solution = tail.solution;
    if let Some((form, codes)) = tail.formation {
        dec.formation_code = Some(form);
        dec.publication_codes = codes;
    }
    let hit = Hit::decision(start, tail.end, dec).with_flags(flags);
    if missing { hit.with_ambiguity(Ambiguity::NumeroManquant) } else { hit }
}

/// `cass_court [SEP chambre] SEP [kind WS] DATE_LINK …` et `cass_short SEP DATE …`. Avec le mot
/// `Cass.` et une chambre, la date peut manquer : `Cass. crim., n° X` (AM-84, confiance moyenne).
fn short_form(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (p, chamber, chamber_end) = if let Some(e) = court(c, s) {
        let p = c.sep(e)?;
        match chamber_any(c, p).and_then(|ch| c.sep(ch.0).map(|r| (r, ch))) {
            Some((r, (end, chamber, flag))) => (r, Some((chamber, flag)), Some(end)),
            None => (p, None, None),
        }
    } else {
        let (e, chamber, flag) = chamber_short(c, s)?;
        (c.sep(e)?, Some((chamber, flag)), None)
    };
    let (p, kind) = match kind_word(c, p).and_then(|(e, k)| c.ws(e).map(|w| (w, k))) {
        Some((w, k)) => (w, Some(k)),
        None => (p, None),
    };
    let Some(date) = c.date_link(p) else {
        return dateless_form(c, s, chamber?, chamber_end?);
    };
    let (chamber, flag) = chamber.unwrap_or((None, None));
    let mut dec = lex::decision(Some(Court::CourDeCassation { chamber }));
    dec.kind = kind;
    let tail = tail(c, date.end);
    Some(finish(s, dec, date, tail, flag.into_iter().collect()))
}

/// `cass_court SEP chambre SEP cass_numbers` : décision sans date, chambre lue (AM-84).
fn dateless_form(
    c: &Cur<'_>,
    s: usize,
    chamber: (Option<CassChamber>, Option<Flag>),
    chamber_end: usize,
) -> Option<Hit> {
    let tail = tail(c, chamber_end);
    (!tail.numbers.is_empty()).then_some(())?;
    let (chamber, flag) = chamber;
    let dec = lex::decision(Some(Court::CourDeCassation { chamber }));
    let date = lex::DateHit { end: chamber_end, date: None, flags: Vec::new() };
    Some(finish(s, dec, date, tail, flag.into_iter().collect()).with_confidence(Confidence::Medium))
}

/// `lf_title_cass` : `Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, …`.
fn legifrance_title(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let p = c.sep(c.kwb(s, "cour de cassation")?)?;
    let nature = c.kwb(p, "civile").or_else(|| c.kwb(p, "criminelle")).and_then(|e| c.sep(e));
    let (e, chamber, flag) = chamber_lf(c, nature.unwrap_or(p))?;
    let date = c.date_link(c.sep(e)?)?;
    let (mut end, numbers) = c.sep(date.end).and_then(|q| cass_numbers(c, q))?;
    let mut publication = None;
    if let Some((e, b)) = c.sep(end).and_then(|q| {
        c.kwb(q, "publie au bulletin")
            .map(|e| (e, BulletinRef::Publie))
            .or_else(|| c.kwb(q, "inedit").map(|e| (e, BulletinRef::Inedit)))
    }) {
        publication = Some(Publication { bulletin: Some(b), lebon: None });
        end = e;
    }
    let tail = Tail { end, numbers, publication, solution: None, formation: None };
    let dec = lex::decision(Some(Court::CourDeCassation { chamber }));
    Some(finish(s, dec, date, tail, flag.into_iter().collect()))
}

/// `cass_chamber_long ["de la Cour de cassation"] SEP DATE_LINK SEP pourvoi` : la juridiction se
/// déduit de la chambre.
fn long_chamber(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (e, chamber, flag) = chamber_long(c, s)?;
    let e = c.ws(e).and_then(|w| c.kwb(w, "de la cour de cassation")).unwrap_or(e);
    let date = c.date_link(c.sep(e)?)?;
    let tail = tail(c, date.end);
    (!tail.numbers.is_empty()).then_some(())?;
    let mut dec = lex::decision(Some(Court::CourDeCassation { chamber }));
    dec.court_via = Some(CourtVia::Chambre);
    Some(finish(s, dec, date, tail, flag.into_iter().collect()))
}

/// `cass_court "(" chambre SEP "arrêt" NUM_SIGN INT "du" DATE ")"`.
fn parenthesised(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let open = c.ws0(court(c, s)?);
    c.is(open, '(').then_some(())?;
    let (e, chamber, flag) = chamber_long(c, open + 1)?;
    let (q, _) = c.num_sign(c.ws(c.kwb(c.sep(e)?, "arret")?)?, false)?;
    let (e, number) = c.int(q)?;
    let date = c.date(c.ws(c.kwb(c.ws(e)?, "du")?)?)?;
    c.is(date.end, ')').then_some(())?;
    let mut dec = lex::decision(Some(Court::CourDeCassation { chamber }));
    dec.arret_num = Some(number);
    dec.kind = Some(DecisionKind::Arret);
    let tail = Tail {
        end: date.end + 1,
        numbers: vec![],
        publication: None,
        solution: None,
        formation: None,
    };
    Some(finish(s, dec, date, tail, flag.into_iter().collect()))
}

/// `"Arrêt" NUM_SIGN INT [form_letter] "Pourvoi" NUM_SIGN [key] pourvoi` : en-tête d'arrêt.
fn header(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (q, _) = c.num_sign(c.ws(c.kwb(s, "arret")?)?, false)?;
    let (mut p, number) = c.int(q)?;
    let mut formation = None;
    if let Some((e, form, codes)) = c.ws(p).and_then(|w| form_pub(c, w)) {
        formation = Some((form, codes));
        p = e;
    }
    let w = c.ws(p)?;
    c.kwb(w, "pourvoi")?;
    let (end, numbers) = cass_numbers(c, w)?;
    let mut dec = lex::decision(Some(Court::CourDeCassation { chamber: None }));
    dec.arret_num = Some(number);
    dec.kind = Some(DecisionKind::Arret);
    dec.formation_code = formation.map(|(form, _)| form);
    dec.numbers = numbers;
    Some(Hit::decision(s, end, dec))
}

/// `bulletin` seul : `Bull. civ. I, n° 270`.
fn bulletin_alone(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (end, raw) = bulletin(c, s)?;
    let mut dec = lex::decision(Some(Court::CourDeCassation { chamber: None }));
    dec.publication =
        Some(Publication { bulletin: Some(BulletinRef::Reference(raw)), lebon: None });
    Some(
        Hit::decision(s, end, dec)
            .with_confidence(Confidence::Low)
            .with_ambiguity(Ambiguity::ReferenceDePublicationSeule),
    )
}

/// `NUM_SIGN pourvoi` : juridiction inférée de la forme du numéro (N29).
fn pourvoi_alone(c: &Cur<'_>, s: usize) -> Option<Hit> {
    let (p, _) = c.num_sign(s, false)?;
    let (end, numbers) = pourvoi_list(c, p)?;
    let mut dec = lex::decision(Some(Court::CourDeCassation { chamber: None }));
    dec.court_via = Some(CourtVia::FormeDuNumero);
    dec.numbers = numbers;
    let unread = c.head_before(s).then_some(Flag::TeteDecisionNonLue);
    Some(Hit::decision(s, end, dec).with_confidence(Confidence::Medium).with_flags(unread))
}

/// Aides de test partagées par les six grammaires de juridictions.
#[cfg(test)]
pub(crate) mod testing {
    use crate::citation::grammar::Candidate;
    use crate::citation::normalize::{Normalized, Steps, normalize};
    use crate::citation::scan::{Anchor, AnchorClass};
    use crate::citation::types::{CitationRef, Court, DecisionCite};
    use crate::date::CivilDate;

    /// Signature d'un `scan` de famille.
    pub(crate) type ScanFn = fn(&Normalized, &[Anchor]) -> Vec<Candidate>;

    /// Ancre construite à la main : première occurrence de `needle` dans le texte normalisé.
    pub(crate) fn anchor(n: &Normalized, class: AnchorClass, needle: &str) -> Anchor {
        assert!(n.text.contains(needle), "{needle:?} absent de {:?}", n.text);
        let start = n.text.find(needle).unwrap_or_default();
        Anchor { class, range: start..start + needle.len() }
    }

    /// Candidats de `scan` sur `input`, avec les ancres `(classe, extrait)` données.
    pub(crate) fn run(
        scan: ScanFn,
        input: &str,
        anchors: &[(AnchorClass, &str)],
    ) -> Vec<Candidate> {
        let n = normalize(input, Steps::default());
        let anchors: Vec<Anchor> = anchors.iter().map(|(c, s)| anchor(&n, *c, s)).collect();
        scan(&n, &anchors)
    }

    /// Texte d'origine couvert par un candidat.
    pub(crate) fn matched<'a>(input: &'a str, c: &Candidate) -> &'a str {
        &input[c.range.clone()]
    }

    /// Le candidat unique d'une liste.
    pub(crate) fn only(mut candidates: Vec<Candidate>) -> Candidate {
        assert_eq!(candidates.len(), 1, "{candidates:#?}");
        candidates.remove(0)
    }

    /// La décision d'un candidat.
    #[expect(clippy::panic, reason = "aide d'échec de test : le message est ce que le test dit")]
    pub(crate) fn decision(c: &Candidate) -> &DecisionCite {
        match &c.value {
            CitationRef::Decision(d) => d,
            other => panic!("pas une décision : {other:?}"),
        }
    }

    /// Fenêtre de lecture ouverte au début de `text` (normalisé), passée à `f`.
    pub(crate) fn read<T>(text: &str, f: impl Fn(&super::lex::Cur<'_>) -> T) -> T {
        let n = normalize(text, Steps::default());
        let doc = super::lex::Doc::new(&n);
        f(&doc.cur(0))
    }

    /// Décision vide, sans juridiction.
    pub(crate) fn blank() -> DecisionCite {
        DecisionCite {
            court: None,
            court_via: None,
            date: None,
            numbers: Vec::new(),
            kind: None,
            publication: None,
            solution: None,
            parties: None,
            subject: None,
            arret_num: None,
            formation_code: None,
            publication_codes: Vec::new(),
        }
    }

    /// Décision vide d'une juridiction.
    pub(crate) fn with_court(court: Court) -> DecisionCite {
        DecisionCite { court: Some(court), ..blank() }
    }

    /// Date civile valide.
    pub(crate) fn date(y: u16, m: u8, d: u8) -> Option<CivilDate> {
        CivilDate::new(y, m, d)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::testing::{date, decision, matched, only, read, run, with_court};
    use super::*;
    use crate::citation::dict::courts::CassChamber;
    use crate::citation::grammar::Candidate;
    use crate::citation::scan::AnchorClass::{Court as CourtA, Pourvoi as PourvoiA, Sink};
    use crate::citation::types::{
        Ambiguity, BulletinRef, CaseNumber, CitationRef, Confidence, Court, CourtVia, DecisionKind,
        Flag, Pourvoi, Publication, UnsupportedCite, UnsupportedFamily,
    };

    fn cass(chamber: Option<CassChamber>) -> crate::citation::types::DecisionCite {
        with_court(Court::CourDeCassation { chamber })
    }

    fn pv(yy: u8, mid: u8, seq: u16, key: Option<char>) -> CaseNumber {
        CaseNumber::Pourvoi(Pourvoi { yy, mid, seq, key })
    }

    fn bulletin(raw: &str) -> Publication {
        Publication { bulletin: Some(BulletinRef::Reference(raw.into())), lebon: None }
    }

    /// Un cas : entrée, ancres, extrait attendu, décision attendue.
    struct Case {
        input: &'static str,
        anchors: &'static [(crate::citation::scan::AnchorClass, &'static str)],
        span: &'static str,
        want: crate::citation::types::DecisionCite,
    }

    fn check(case: &Case) -> crate::citation::grammar::Candidate {
        let got = only(run(scan, case.input, case.anchors));
        assert_eq!(matched(case.input, &got), case.span, "{}", case.input);
        assert_eq!(*decision(&got), case.want, "{}", case.input);
        got
    }

    #[test]
    fn citation_jp_forms_p090_p093_p094() {
        check(&Case {
            input: "à rapprocher : 1re Civ., 25 mai 2023, pourvoi n° 21-23.174 (cassation partielle) et l'arrêt cité.",
            anchors: &[(CourtA, "Civ."), (PourvoiA, "21-23.174")],
            span: "1re Civ., 25 mai 2023, pourvoi n° 21-23.174 (cassation partielle)",
            want: crate::citation::types::DecisionCite {
                date: date(2023, 5, 25),
                numbers: vec![pv(21, 23, 174, None)],
                solution: Some("cassation partielle".into()),
                ..cass(Some(CassChamber::Civ1))
            },
        });
        check(&Case {
            input: "Crim., 28 mars 2018, pourvoi n° 17-82.138, Bull. crim. 2018, n° 59 (cassation).",
            anchors: &[(CourtA, "Crim."), (PourvoiA, "17-82.138"), (Sink, "Bull.")],
            span: "Crim., 28 mars 2018, pourvoi n° 17-82.138, Bull. crim. 2018, n° 59 (cassation)",
            want: crate::citation::types::DecisionCite {
                date: date(2018, 3, 28),
                numbers: vec![pv(17, 82, 138, None)],
                publication: Some(bulletin("Bull. crim. 2018, n° 59")),
                solution: Some("cassation".into()),
                ..cass(Some(CassChamber::Crim))
            },
        });
        check(&Case {
            input: "le droit à une nationalité (1re Civ., 25 avril 2007, pourvoi n° 04-17.632, Bull. 2007, I, n° 159 (rejet)).",
            anchors: &[(CourtA, "Civ."), (PourvoiA, "04-17.632")],
            span: "1re Civ., 25 avril 2007, pourvoi n° 04-17.632, Bull. 2007, I, n° 159 (rejet)",
            want: crate::citation::types::DecisionCite {
                date: date(2007, 4, 25),
                numbers: vec![pv(4, 17, 632, None)],
                publication: Some(bulletin("Bull. 2007, I, n° 159")),
                solution: Some("rejet".into()),
                ..cass(Some(CassChamber::Civ1))
            },
        });
    }

    #[test]
    fn cass_court_forms_p095_to_p099_p102_to_p104_p106_p108() {
        let cases = [
            Case {
                input: "Cass. Civ. 1ère, 17 déc. 2010, n°10-10.906, Bull. civ., I, n°270",
                anchors: &[(CourtA, "Cass."), (PourvoiA, "10-10.906")],
                span: "Cass. Civ. 1ère, 17 déc. 2010, n°10-10.906, Bull. civ., I, n°270",
                want: crate::citation::types::DecisionCite {
                    date: date(2010, 12, 17),
                    numbers: vec![pv(10, 10, 906, None)],
                    publication: Some(bulletin("Bull. civ., I, n°270")),
                    ..cass(Some(CassChamber::Civ1))
                },
            },
            Case {
                input: "Cass. 1re civ., 2 juill. 2014, n° 13-21.929",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass. 1re civ., 2 juill. 2014, n° 13-21.929",
                want: crate::citation::types::DecisionCite {
                    date: date(2014, 7, 2),
                    numbers: vec![pv(13, 21, 929, None)],
                    ..cass(Some(CassChamber::Civ1))
                },
            },
            Case {
                input: "Cass. 2e civ., 26 septembre 2019, n° 18-14.708",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass. 2e civ., 26 septembre 2019, n° 18-14.708",
                want: crate::citation::types::DecisionCite {
                    date: date(2019, 9, 26),
                    numbers: vec![pv(18, 14, 708, None)],
                    ..cass(Some(CassChamber::Civ2))
                },
            },
            Case {
                input: "Cass. 3e civ., 2 juillet 2003, n° 02-70.047, Bull. 2003, III, n° 140",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass. 3e civ., 2 juillet 2003, n° 02-70.047, Bull. 2003, III, n° 140",
                want: crate::citation::types::DecisionCite {
                    date: date(2003, 7, 2),
                    numbers: vec![pv(2, 70, 47, None)],
                    publication: Some(bulletin("Bull. 2003, III, n° 140")),
                    ..cass(Some(CassChamber::Civ3))
                },
            },
            Case {
                input: "Cass. 2e civ., AVIS du 27 juin 2019, n° 19-70.006",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass. 2e civ., AVIS du 27 juin 2019, n° 19-70.006",
                want: crate::citation::types::DecisionCite {
                    date: date(2019, 6, 27),
                    kind: Some(DecisionKind::Avis),
                    numbers: vec![pv(19, 70, 6, None)],
                    ..cass(Some(CassChamber::Civ2))
                },
            },
        ];
        for case in &cases {
            check(case);
        }
    }

    #[test]
    fn cass_court_forms_with_chambers_and_formations() {
        let cases = [
            Case {
                input: "Cass. soc., 12 févr. 2025, n° 23-12.345, FS-B",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass. soc., 12 févr. 2025, n° 23-12.345, FS-B",
                want: crate::citation::types::DecisionCite {
                    date: date(2025, 2, 12),
                    numbers: vec![pv(23, 12, 345, None)],
                    formation_code: Some("FS".into()),
                    publication_codes: vec!['B'],
                    ..cass(Some(CassChamber::Soc))
                },
            },
            Case {
                input: "Cass. ass. plén., 17 nov. 2000, n° 99-13.701",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass. ass. plén., 17 nov. 2000, n° 99-13.701",
                want: crate::citation::types::DecisionCite {
                    date: date(2000, 11, 17),
                    numbers: vec![pv(99, 13, 701, None)],
                    ..cass(Some(CassChamber::AssPlen))
                },
            },
            Case {
                input: "Cass., ch. mixte, 6 sept. 2002, n° 98-22.981",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass., ch. mixte, 6 sept. 2002, n° 98-22.981",
                want: crate::citation::types::DecisionCite {
                    date: date(2002, 9, 6),
                    numbers: vec![pv(98, 22, 981, None)],
                    ..cass(Some(CassChamber::Mixte))
                },
            },
            Case {
                input: "Cass., 1\u{2b3}\u{1d49} civ., 4 juin 2025, pourvoi no 24\u{2011}13.470",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass., 1\u{2b3}\u{1d49} civ., 4 juin 2025, pourvoi no 24\u{2011}13.470",
                want: crate::citation::types::DecisionCite {
                    date: date(2025, 6, 4),
                    numbers: vec![pv(24, 13, 470, None)],
                    ..cass(Some(CassChamber::Civ1))
                },
            },
            Case {
                input: "Cass. civ. 1re, 4 juin 2025, n° 24-13.470",
                anchors: &[(CourtA, "Cass.")],
                span: "Cass. civ. 1re, 4 juin 2025, n° 24-13.470",
                want: crate::citation::types::DecisionCite {
                    date: date(2025, 6, 4),
                    numbers: vec![pv(24, 13, 470, None)],
                    ..cass(Some(CassChamber::Civ1))
                },
            },
        ];
        for case in &cases {
            check(case);
        }
    }

    #[test]
    fn legifrance_titles_p091_p092_p101() {
        let cases = [
            Case {
                input: "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin",
                anchors: &[(CourtA, "Cour de cassation")],
                span: "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin",
                want: crate::citation::types::DecisionCite {
                    date: date(2025, 6, 4),
                    numbers: vec![pv(24, 13, 470, None)],
                    publication: Some(Publication {
                        bulletin: Some(BulletinRef::Publie),
                        lebon: None,
                    }),
                    ..cass(Some(CassChamber::Civ1))
                },
            },
            Case {
                input: "Cour de cassation, civile, Chambre commerciale, 22 novembre 2023, 22-17.843, Publié au bulletin",
                anchors: &[(CourtA, "Cour de cassation")],
                span: "Cour de cassation, civile, Chambre commerciale, 22 novembre 2023, 22-17.843, Publié au bulletin",
                want: crate::citation::types::DecisionCite {
                    date: date(2023, 11, 22),
                    numbers: vec![pv(22, 17, 843, None)],
                    publication: Some(Publication {
                        bulletin: Some(BulletinRef::Publie),
                        lebon: None,
                    }),
                    ..cass(Some(CassChamber::Com))
                },
            },
            Case {
                input: "Cour de Cassation, Chambre civile 2, du 24 février 2005, 04-10.362, Publié au bulletin",
                anchors: &[(CourtA, "Cour de Cassation")],
                span: "Cour de Cassation, Chambre civile 2, du 24 février 2005, 04-10.362, Publié au bulletin",
                want: crate::citation::types::DecisionCite {
                    date: date(2005, 2, 24),
                    numbers: vec![pv(4, 10, 362, None)],
                    publication: Some(Publication {
                        bulletin: Some(BulletinRef::Publie),
                        lebon: None,
                    }),
                    ..cass(Some(CassChamber::Civ2))
                },
            },
        ];
        for case in &cases {
            check(case);
        }
    }

    #[test]
    fn header_and_parenthesised_forms_p100_p105() {
        let got = check(&Case {
            input: "Arrêt n° 399 F Pourvoi n° N 24-13.470 R É P U B L I Q U E",
            anchors: &[(CourtA, "Arrêt"), (PourvoiA, "24-13.470")],
            span: "Arrêt n° 399 F Pourvoi n° N 24-13.470",
            want: crate::citation::types::DecisionCite {
                kind: Some(DecisionKind::Arret),
                arret_num: Some(399),
                formation_code: Some("F".into()),
                numbers: vec![pv(24, 13, 470, Some('N'))],
                ..cass(None)
            },
        });
        assert_eq!(got.confidence, Confidence::High);
        check(&Case {
            input: "SAISI le 8 août 2025 par la Cour de cassation (chambre criminelle, arrêt n° 1133 du 6 août 2025), dans les conditions",
            anchors: &[(CourtA, "Cour de cassation")],
            span: "Cour de cassation (chambre criminelle, arrêt n° 1133 du 6 août 2025)",
            want: crate::citation::types::DecisionCite {
                date: date(2025, 8, 6),
                kind: Some(DecisionKind::Arret),
                arret_num: Some(1133),
                ..cass(Some(CassChamber::Crim))
            },
        });
    }

    #[test]
    fn chamber_alone_p107_and_pourvoi_alone_n29() {
        let got = check(&Case {
            input: "Première chambre civile, 4 juin 2025, 24-13.470",
            anchors: &[(CourtA, "Première chambre civile"), (PourvoiA, "24-13.470")],
            span: "Première chambre civile, 4 juin 2025, 24-13.470",
            want: crate::citation::types::DecisionCite {
                court_via: Some(CourtVia::Chambre),
                date: date(2025, 6, 4),
                numbers: vec![pv(24, 13, 470, None)],
                ..cass(Some(CassChamber::Civ1))
            },
        });
        assert_eq!(got.confidence, Confidence::High);
        let got = check(&Case {
            input: "n° 21-23.174",
            anchors: &[(PourvoiA, "21-23.174")],
            span: "n° 21-23.174",
            want: crate::citation::types::DecisionCite {
                court_via: Some(CourtVia::FormeDuNumero),
                numbers: vec![pv(21, 23, 174, None)],
                ..cass(None)
            },
        });
        assert_eq!(got.confidence, Confidence::Medium);
    }

    #[test]
    fn missing_number_n20_and_bare_bulletin_n31() {
        let got = check(&Case {
            input: "Crim., 28 mars 2018",
            anchors: &[(CourtA, "Crim.")],
            span: "Crim., 28 mars 2018",
            want: crate::citation::types::DecisionCite {
                date: date(2018, 3, 28),
                ..cass(Some(CassChamber::Crim))
            },
        });
        assert_eq!(got.flags.ambiguity, Some(Ambiguity::NumeroManquant));
        let got = check(&Case {
            input: "Bull. civ. I, n° 270",
            anchors: &[(Sink, "Bull.")],
            span: "Bull. civ. I, n° 270",
            want: crate::citation::types::DecisionCite {
                publication: Some(bulletin("Bull. civ. I, n° 270")),
                ..cass(None)
            },
        });
        assert_eq!(got.flags.ambiguity, Some(Ambiguity::ReferenceDePublicationSeule));
        assert_eq!(got.confidence, Confidence::Low);
    }

    #[test]
    fn nonexistent_civil_chamber_n08_flags_it() {
        let got = check(&Case {
            input: "Cass. civ. 4e, 3 mars 2020, n° 18-12.345",
            anchors: &[(CourtA, "Cass.")],
            span: "Cass. civ. 4e, 3 mars 2020, n° 18-12.345",
            want: crate::citation::types::DecisionCite {
                date: date(2020, 3, 3),
                numbers: vec![pv(18, 12, 345, None)],
                ..cass(None)
            },
        });
        assert_eq!(got.flags.list, [Flag::ChambreInexistante("civ4".into())]);
    }

    #[test]
    fn things_that_are_not_cassation_decisions_n18_n19_n39() {
        for (input, anchors) in [
            ("la Soc. Générale a interjeté appel", &[(CourtA, "Soc.")][..]),
            ("Com. de communes du Pays Haut", &[(CourtA, "Com.")][..]),
            ("pourvoi n° 21-23.1745", &[(PourvoiA, "21-23.1745")][..]),
        ] {
            assert_eq!(run(scan, input, anchors), [], "{input}");
        }
    }

    #[test]
    fn rare_pourvoi_and_formation_spellings() {
        let tail = |input: &str| decision(&only(run(scan, input, &[(CourtA, "Cass.")]))).clone();
        let numbered = tail("Cass. civ. 3, 4 juin 2025, n° 21-12.345");
        assert_eq!(
            numbered.court,
            Some(Court::CourDeCassation { chamber: Some(CassChamber::Civ3) })
        );
        assert_eq!(tail("Cass. soc., 4 juin 2025, n° 24-13470").numbers, [pv(24, 13, 470, None)]);
        assert_eq!(
            tail("Cass. soc., 4 juin 2025, n° 21-23.174 et 21-23.175").numbers,
            [pv(21, 23, 174, None), pv(21, 23, 175, None)]
        );
        let formation = tail("Cass. soc., 12 févr. 2025, n° 23-12.345, FP-B+R");
        assert_eq!(formation.formation_code.as_deref(), Some("FP"));
        assert_eq!(formation.publication_codes, ['B', 'R']);
        // « no » collé au numéro : le pourvoi n'a pas de frontière à gauche.
        let glued = only(run(scan, "Cass. soc., 4 juin 2025, no21-23.174", &[(CourtA, "Cass.")]));
        assert_eq!(decision(&glued).numbers, []);
        assert_eq!(glued.flags.ambiguity, Some(Ambiguity::NumeroManquant));
    }

    #[test]
    fn lexer_primitives() {
        let republican = read("3 brumaire an IV", |c| c.date(0)).map(|d| (d.date, d.flags));
        assert_eq!(republican, Some((None, vec![Flag::DateRepublicaine])));
        assert_eq!(read("1er janvier 2020", |c| c.date(0)).map(|d| d.date), Some(date(2020, 1, 1)));
        assert_eq!(read("deuxième", |c| c.ordinal(0)), Some((8, 2)));
        assert_eq!(read("x2e", |c| c.ordinal(1)), None);
        assert_eq!(read("chambre", |c| c.ordinal_chamber_ends(0)), Vec::<usize>::new());
        assert_eq!(read("12", |c| c.place_ends(0)), Vec::<usize>::new());
        assert_eq!(read("Saint-Denis de La Réunion, x", |c| c.place_ends(0)), [25, 17, 14, 11]);
    }

    #[test]
    fn invalid_date_and_posterior_pourvoi_are_flagged() {
        let got =
            only(run(scan, "Cass. soc., 31 février 2020, n° 18-12.345", &[(CourtA, "Cass.")]));
        assert_eq!(decision(&got).date, None);
        assert_eq!(got.flags.list, [Flag::DateInvalide]);
        let got = only(run(scan, "Cass. soc., 3 mars 2019, n° 20-12.345", &[(CourtA, "Cass.")]));
        assert_eq!(got.flags.list, [Flag::AnneePourvoiPosterieure]);
        assert_eq!(decision(&got).date, date(2019, 3, 3));
        // 99 pour 2000 : pas postérieur.
        let got = only(run(scan, "Cass. soc., 3 mars 2000, n° 99-12.345", &[(CourtA, "Cass.")]));
        assert_eq!(got.flags.list, []);
    }

    #[test]
    fn dates_cut_short_are_not_dates() {
        for text in [
            "7 octobre2016",
            "7 octobre 99",
            "12/ab/2020",
            "12/05-2020",
            "12/13/2020",
            "12/05/20",
            "2026/04/30",
            "2026-4-30",
            "2026-04-3",
            "2026-04-32",
            "2026-13-01",
            "3 janvier an IV",
            "3 brumaire",
            "3 brumaire en IV",
            "3 brumaire an",
            "3 brumaire an iv",
        ] {
            assert_eq!(read(text, |c| c.date(0)), None, "{text}");
        }
    }

    #[test]
    fn lexer_readers_stop_at_the_first_missing_piece() {
        // Une espace du gabarit exige au moins une espace du texte.
        assert_eq!(read("cour-de", |c| c.kw(0, "cour de")), None);
        assert_eq!(read("cour   de", |c| c.kw(0, "cour de")), Some(9));
        // Un ordinal sans le mot « chambre » n'ouvre aucune chambre.
        assert_eq!(read("2e civ", |c| c.ordinal_chamber_ends(0)), Vec::<usize>::new());
        // Un littéral que le texte interrompt n'est pas lu.
        assert_eq!(read("ab", |c| c.lit(0, "abc")), None);
        assert_eq!(read("abc", |c| c.lit(0, "abc")), Some(3));
        // Une virgule suivie de « et » : l'espace qui suit est avalée.
        assert_eq!(read("x, et 5", |c| c.list_sep(1)), Some(6));
        assert_eq!(read("x, 5", |c| c.list_sep(1)), Some(3));
        for text in ["21-2.345", "21-23.17", "21-234"] {
            assert!(read(text, |c| pourvoi(c, 0)).is_none(), "{text}");
        }
        for text in ["Bull. civ.", "Bull. civ., xyz", "Bull. civ., n° x"] {
            assert_eq!(read(text, |c| super::bulletin(c, 0)), None, "{text}");
        }
    }

    #[test]
    fn bulletin_names_the_plenary_assembly_and_the_mixed_chamber() {
        for text in ["Bull. ass. plén., n° 5", "Bull. ch. mixte, n° 5"] {
            assert_eq!(
                read(text, |c| super::bulletin(c, 0)),
                Some((text.chars().count(), text.to_owned())),
                "{text}"
            );
        }
    }

    #[test]
    fn chambers_in_their_less_common_spellings() {
        assert_eq!(
            read("ch. réunies", |c| chamber_short(c, 0)),
            Some((11, Some(CassChamber::Reunies), None))
        );
        // Sans virgule derrière : la lecture s'arrête à la chambre.
        assert_eq!(
            read("chambre sociale", |c| chamber_long(c, 0)),
            Some((15, Some(CassChamber::Soc), None))
        );
        // Chambre commerciale « économique et financière », dans les deux ordres.
        for text in [
            "chambre commerciale, économique et financière",
            "chambre commerciale, financière et économique",
        ] {
            assert_eq!(
                read(text, |c| chamber_long(c, 0)),
                Some((text.chars().count(), Some(CassChamber::Com), None)),
                "{text}"
            );
        }
        // La précision ne vaut que pour la chambre commerciale.
        assert_eq!(
            read("chambre sociale, économique et financière", |c| chamber_long(c, 0)),
            Some((15, Some(CassChamber::Soc), None))
        );
        assert_eq!(
            read("AP", |c| chamber_short(c, 0)),
            Some((2, Some(CassChamber::AssPlen), None))
        );
        // « civ. » suivi d'un chiffre seul : numéro de chambre, sauf si c'est un jour de date.
        assert_eq!(read("civ. 3", |c| civ_number(c, 0)), Some((6, 3)));
        for text in ["civ. x", "civ. 12", "civ. 4 juin 2020"] {
            assert_eq!(read(text, |c| civ_number(c, 0)), None, "{text}");
        }
        assert_eq!(read("première-chambre civile", |c| chamber_long(c, 0)), None);
        for text in ["chambre", "Chambre civile x", "Chambre civile 12"] {
            assert_eq!(read(text, |c| chamber_lf(c, 0)), None, "{text}");
        }
    }

    #[test]
    fn decision_kinds_ordonnance_and_arret() {
        assert_eq!(read("ordonnance", |c| kind_word(c, 0)), Some((10, DecisionKind::Ordonnance)));
        assert_eq!(read("ord.", |c| kind_word(c, 0)), Some((4, DecisionKind::Ordonnance)));
        assert_eq!(read("arrêt", |c| kind_word(c, 0)), Some((5, DecisionKind::Arret)));
    }

    #[test]
    fn publication_tail_inedit_rapport_and_formation_without_letters() {
        let input = "Cass. soc., 4 juin 2025, n° 24-13.470, Rapport";
        let got = only(run(scan, input, &[(CourtA, "Cass.")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(decision(&got).publication, None);
        let input = "Cass. soc., 4 juin 2025, n° 24-13.470, Inédit";
        let got = only(run(scan, input, &[(CourtA, "Cass.")]));
        assert_eq!(matched(input, &got), input);
        assert_eq!(
            decision(&got).publication,
            Some(Publication { bulletin: Some(BulletinRef::Inedit), lebon: None })
        );
        // « FS- » sans lettre de publication : seule la formation est lue.
        let input = "Cass. soc., 4 juin 2025, n° 24-13.470, FS-Z";
        let got = only(run(scan, input, &[(CourtA, "Cass.")]));
        assert_eq!(matched(input, &got), "Cass. soc., 4 juin 2025, n° 24-13.470, FS");
        assert_eq!(decision(&got).formation_code.as_deref(), Some("FS"));
        assert_eq!(decision(&got).publication_codes, Vec::<char>::new());
    }

    #[test]
    fn legifrance_title_without_or_with_inedit_publication() {
        check(&Case {
            input: "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Inédit",
            anchors: &[(CourtA, "Cour de cassation")],
            span: "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Inédit",
            want: crate::citation::types::DecisionCite {
                date: date(2025, 6, 4),
                numbers: vec![pv(24, 13, 470, None)],
                publication: Some(Publication { bulletin: Some(BulletinRef::Inedit), lebon: None }),
                ..cass(Some(CassChamber::Civ1))
            },
        });
        check(&Case {
            input: "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, texte",
            anchors: &[(CourtA, "Cour de cassation")],
            span: "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470",
            want: crate::citation::types::DecisionCite {
                date: date(2025, 6, 4),
                numbers: vec![pv(24, 13, 470, None)],
                ..cass(Some(CassChamber::Civ1))
            },
        });
    }

    #[test]
    fn header_without_formation_letter() {
        check(&Case {
            input: "Arrêt n° 399 Pourvoi n° 24-13.470",
            anchors: &[(CourtA, "Arrêt"), (PourvoiA, "24-13.470")],
            span: "Arrêt n° 399 Pourvoi n° 24-13.470",
            want: crate::citation::types::DecisionCite {
                kind: Some(DecisionKind::Arret),
                arret_num: Some(399),
                numbers: vec![pv(24, 13, 470, None)],
                ..cass(None)
            },
        });
    }

    #[test]
    fn incomplete_grammar_forms_give_nothing() {
        type Grammar = fn(&Cur<'_>, usize) -> Option<Hit>;
        let cases: [(&str, Grammar, &[&str]); 5] = [
            ("short_form", short_form, &["Cass.4 juin 2025, n° 24-13.470"]),
            (
                "legifrance_title",
                legifrance_title,
                &[
                    "Cour de cassation",
                    "Cour de cassation, Chambre sociale",
                    "Cour de cassation, Chambre sociale, texte",
                    "Cour de cassation, Chambre sociale, 4 juin 2025",
                ],
            ),
            (
                "long_chamber",
                long_chamber,
                &[
                    "première chambre civile",
                    "première chambre civile, texte",
                    "première chambre civile, 4 juin 2025",
                ],
            ),
            (
                "parenthesised",
                parenthesised,
                &[
                    "Cour de cassation (xyz",
                    "Cour de cassation (chambre criminelle",
                    "Cour de cassation (chambre criminelle, xyz",
                    "Cour de cassation (chambre criminelle, arrêt",
                    "Cour de cassation (chambre criminelle, arrêt xyz",
                    "Cour de cassation (chambre criminelle, arrêt n° x",
                    "Cour de cassation (chambre criminelle, arrêt n° 1133",
                    "Cour de cassation (chambre criminelle, arrêt n° 1133 xx",
                    "Cour de cassation (chambre criminelle, arrêt n° 1133 du",
                    "Cour de cassation (chambre criminelle, arrêt n° 1133 du xyz",
                    "Cour de cassation (chambre criminelle, arrêt n° 1133 du 6 août 2025 x",
                ],
            ),
            (
                "header",
                header,
                &[
                    "Arrêt",
                    "Arrêt xyz",
                    "Arrêt n° x",
                    "Arrêt n° 399",
                    "Arrêt n° 399 xyz",
                    "Arrêt n° 399 Pourvoi n° x",
                ],
            ),
        ];
        for (name, grammar, inputs) in cases {
            for text in inputs {
                assert!(read(text, |c| grammar(c, 0)).is_none(), "{name} : {text}");
            }
        }
    }

    #[test]
    fn a_place_needs_a_dated_decision_after_its_chamber() {
        fn glued(c: &Cur<'_>, i: usize) -> Vec<usize> {
            c.int(i).map(|(e, _)| e).into_iter().collect()
        }
        fn one_number(c: &Cur<'_>, i: usize) -> Option<(usize, Vec<CaseNumber>)> {
            c.int(i).map(|(e, n)| (e, vec![pv(0, 0, u16::try_from(n).unwrap_or(0), None)]))
        }
        let seat = |key: &str| (key != "nowhere").then(|| key.to_owned());
        let at = |text: &str| read(text, |c| lex::at_place(c, 0, glued, one_number, seat));
        // Lieu inconnu, ou lieu collé à ce qui suit : rien n'est lu.
        assert!(at("Nowhere, 12, 4 juin 2025, 7").is_none());
        assert!(at("Paris12, 12, 4 juin 2025, 7").is_none());
        // Date sans séparateur derrière, ou séparateur sans numéro.
        assert!(at("Paris, 12, 4 juin 2025").is_none());
        assert!(at("Paris, 12, 4 juin 2025, x").is_none());
        // Sans chambre lisible, la date suit directement le lieu.
        assert_eq!(at("Paris, 4 juin 2025, 7").map(|f| f.chamber), Some(None));
        // La chambre « 12 » est collée à « x » : pas de séparateur, pas de décision.
        assert!(
            read("Paris, 12x, 4 juin 2025, 7", |c| lex::at_place(c, 0, glued, one_number, seat))
                .is_none()
        );
        // La chambre est lue, mais la date manque derrière.
        assert!(
            read("Paris, 12, texte", |c| lex::at_place(c, 0, glued, one_number, seat)).is_none()
        );
        let found = read("Paris, 12, 4 juin 2025, 7", |c| {
            lex::at_place(c, 0, glued, one_number, seat)
                .map(|f| (f.seat, f.chamber, f.date.date, f.numbers))
        });
        assert_eq!(
            found,
            Some((
                "paris".to_owned(),
                Some("12".to_owned()),
                date(2025, 6, 4),
                vec![pv(0, 0, 7, None)]
            ))
        );
    }

    /// Citations de `parse` : texte d'origine couvert, décision.
    fn parsed(input: &str) -> Vec<(String, crate::citation::types::DecisionCite)> {
        crate::citation::parse(input)
            .into_iter()
            .filter_map(|c| match c.value {
                CitationRef::Decision(d) => {
                    Some((input.get(c.span.start..c.span.end).unwrap_or_default().to_owned(), d))
                }
                _ => None,
            })
            .collect()
    }

    /// Une citation qui n'est pas une décision n'est pas comptée par `parsed`.
    #[test]
    fn parsed_keeps_only_decisions() {
        assert_eq!(parsed("article 1240 du code civil").len(), 0);
        assert_eq!(parsed("Cass. soc., 4 juin 2025, n° 24-13.470").len(), 1);
    }

    /// Drapeaux et ambiguïtés des citations de `parse`.
    fn parsed_flags(input: &str) -> Vec<(Vec<Flag>, Option<Ambiguity>)> {
        crate::citation::parse(input).into_iter().map(|c| (c.flags, c.ambiguity)).collect()
    }

    /// U12 G1 : un jour, un mois et une année séparés par deux tirets sont une date.
    #[test]
    fn dates_with_dashes_are_dates() {
        for (text, want) in [
            ("25-11-2015", date(2015, 11, 25)),
            ("1-1-2000", date(2000, 1, 1)),
            ("01-01-2000", date(2000, 1, 1)),
            ("31-12-1999", date(1999, 12, 31)),
        ] {
            let hit = read(text, |c| c.date(0));
            assert_eq!(hit.map(|h| (h.end, h.date)), Some((text.chars().count(), want)), "{text}");
        }
        // Une date à tirets impossible est lue, avec son drapeau (comme `31/02/2020`).
        let hit = read("31-02-2020", |c| c.date(0)).map(|h| (h.date, h.flags));
        assert_eq!(hit, Some((None, vec![Flag::DateInvalide])));
    }

    /// U12 G1 (garde) : le tiret ne se lit que des deux côtés, et jamais pour un numéro d'affaire.
    #[test]
    fn dashed_dates_stay_strict() {
        for text in [
            "25-11-15",
            "25/11-2015",
            "25-11/2015",
            "25.11-2015",
            "25-11.2015",
            "12-13-2020",
            "32-01-2020",
            "00-01-2020",
            "25-11-20155",
            "14-24.444",
            "10-12.345",
            "20-12345",
            "71-44 DC",
            "n° 14-12-345",
        ] {
            assert_eq!(read(text, |c| c.date(0)), None, "{text}");
        }
        // Les formes à barre et à point gardent leur lecture.
        assert!(read("25/11/2015", |c| c.date(0)).is_some());
        assert!(read("25.11.2015", |c| c.date(0)).is_some());
        assert!(read("2015-11-25", |c| c.date(0)).is_some());
    }

    /// U12 G2 : `civ.` collé à son ordinal ou à son chiffre.
    #[test]
    fn glued_civ_chamber() {
        for (text, want) in [("civ.1re", (7, 1)), ("civ.2e", (6, 2)), ("civ.3", (5, 3))] {
            assert_eq!(read(text, |c| civ_number(c, 0)), Some(want), "{text}");
        }
        assert_eq!(
            read("civ.1re", |c| chamber_short(c, 0)),
            Some((7, Some(CassChamber::Civ1), None))
        );
        assert_eq!(
            read("civ.2e", |c| chamber_short(c, 0)),
            Some((6, Some(CassChamber::Civ2), None))
        );
    }

    /// U12 G2 (garde) : collé ou non, `civ.` ne lit ni une date, ni un nombre, ni un mot.
    #[test]
    fn a_glued_civ_does_not_read_a_date_a_big_number_or_a_word() {
        for text in ["civ.12", "civ.4 juin 2020", "civ.x", "civ.", "civ. 12", "civ. 4 juin 2020"] {
            assert_eq!(read(text, |c| civ_number(c, 0)), None, "{text}");
        }
        assert_eq!(read("civ. 3", |c| civ_number(c, 0)), Some((6, 3)));
        assert_eq!(read("civ. 1re", |c| civ_number(c, 0)), Some((8, 1)));
        assert_eq!(read("1re civ.", |c| civ_number(c, 0)), Some((8, 1)));
    }

    /// U12 G2bis : `ch.` ou `chambre` entre l'ordinal et `civ.`, ordinal chiffré devant `chambre
    /// civile`.
    #[test]
    fn ordinal_chamber_with_ch_or_chambre_is_read() {
        for (text, want) in [
            ("2e ch. civ.", CassChamber::Civ2),
            ("1re chambre civ.", CassChamber::Civ1),
            ("3e ch civ", CassChamber::Civ3),
            ("1ère chambre civile", CassChamber::Civ1),
            ("2ème chambre civile", CassChamber::Civ2),
            ("1re ch. civile", CassChamber::Civ1),
        ] {
            let got = read(text, |c| chamber_any(c, 0));
            assert_eq!(got, Some((text.chars().count(), Some(want), None)), "{text}");
        }
        // Garde : les formes déjà lues le restent.
        assert_eq!(
            read("première chambre civile", |c| chamber_any(c, 0)),
            Some((23, Some(CassChamber::Civ1), None))
        );
        assert_eq!(
            read("1re civ.", |c| chamber_any(c, 0)),
            Some((8, Some(CassChamber::Civ1), None))
        );
        // Un ordinal sans « civ. » ni « chambre civile » n'est pas une chambre.
        for text in ["2e ch. soc.", "1re chambre sociale", "2e ch.", "2e chambre"] {
            assert_eq!(read(text, |c| chamber_any(c, 0)), None, "{text}");
        }
    }

    /// U12 G3 : une parenthèse, un crochet ouvrant le numéro après la date ne coupent plus la
    /// décision en deux.
    #[test]
    fn tail_links_with_opener_or_dash() {
        for input in [
            "Cass. com., 12 mars 1999 (n° 14-24.444)",
            "Cass. com., 12 mars 1999 [n° 14-24.444]",
            "Cass. com., 12 mars 1999 (pourvoi n° 14-24.444)",
            "Cass. com., 12 mars 1999 (14-24.444)",
            "Cass. com., 12 mars 1999, (n° 14-24.444)",
            "Cass. com., 12 mars 1999 ( n° 14-24.444 )",
            "Cass. com., 12 mars 1999 (n° 14-24.444 )",
            "Cass. com., 12 mars 1999 [ n° 14-24.444]",
        ] {
            let got = parsed(input);
            assert_eq!(got.len(), 1, "{input} : {got:?}");
            assert_eq!(got[0].0, input, "{input}");
            assert_eq!(got[0].1.numbers, vec![pv(14, 24, 444, None)], "{input}");
            assert_eq!(got[0].1.date, date(1999, 3, 12), "{input}");
            assert_eq!(
                got[0].1.court,
                Some(Court::CourDeCassation { chamber: Some(CassChamber::Com) })
            );
            assert_eq!(parsed_flags(input)[0].1, None, "{input}");
        }
        // La mention de 15 caractères avant le texte et le point qui suit ne sont pas avalés.
        let got = parsed("Voir Cass. soc., 25 novembre 2015 (n° 14-24.444). Suite.");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "Cass. soc., 25 novembre 2015 (n° 14-24.444)");
    }

    /// U12 G3 : un fermant manquant, ou une parenthèse sans numéro, ne change rien.
    #[test]
    fn tail_without_a_closer_or_a_number_after_the_opener() {
        // Sans fermant : la lecture s'arrête au numéro.
        let got = parsed("Cass. com., 12 mars 1999 (n° 14-24.444 et la suite");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "Cass. com., 12 mars 1999 (n° 14-24.444");
        // Un fermant d'une autre espèce n'est pas le fermant.
        let got = parsed("Cass. com., 12 mars 1999 (n° 14-24.444] suite");
        assert_eq!(got[0].0, "Cass. com., 12 mars 1999 (n° 14-24.444");
        let got = parsed("Cass. com., 12 mars 1999 [n° 14-24.444) suite");
        assert_eq!(got[0].0, "Cass. com., 12 mars 1999 [n° 14-24.444");
    }

    /// U12 G3 (garde) : une parenthèse qui ne contient pas de numéro ne lie rien.
    #[test]
    fn a_parenthesis_without_a_number_changes_nothing() {
        for input in
            ["Cass. com., 12 mars 1999 (rejet)", "Cass. com., 12 mars 1999 (voir n° 14-24.444)"]
        {
            let got = parsed(input);
            assert_eq!(got[0].0, "Cass. com., 12 mars 1999", "{input}");
            assert_eq!(got[0].1.numbers, vec![], "{input}");
            assert_eq!(parsed_flags(input)[0].1, Some(Ambiguity::NumeroManquant), "{input}");
        }
    }

    /// U12 G3 (garde) : le tiret ne lie pas la date au numéro (V12) ; le pourvoi lu seul porte le
    /// drapeau de tête non lue (F1) sans être modifié.
    #[test]
    fn a_dash_between_a_date_and_a_number_is_not_linked() {
        for input in [
            "Cass. crim., 25 novembre 2015 - n° 14-24.444",
            "Cass. crim., 25 novembre 2015 – n° 14-24.444",
            "Cass. crim., 25 novembre 2015\n- n° 14-24.444",
        ] {
            let got = parsed(input);
            assert_eq!(got.len(), 2, "{input} : {got:?}");
            assert_eq!(got[0].0, "Cass. crim., 25 novembre 2015", "{input}");
            assert_eq!(got[1].0, "n° 14-24.444", "{input}");
            assert_eq!(got[1].1.court_via, Some(CourtVia::FormeDuNumero), "{input}");
        }
    }

    /// U12 G3 : le fermant se lit après les numéros et leurs publications, avant la solution et
    /// la formation.
    #[test]
    fn closing_bracket_order_with_tail() {
        let got = parsed("Cass. soc., 25 novembre 2015 (n° 14-24.444, Inédit) (rejet) FS-D");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "Cass. soc., 25 novembre 2015 (n° 14-24.444, Inédit) (rejet) FS-D");
        let dec = &got[0].1;
        assert_eq!(
            dec.publication,
            Some(Publication { bulletin: Some(BulletinRef::Inedit), lebon: None })
        );
        assert_eq!(dec.solution.as_deref(), Some("rejet"));
        assert_eq!(dec.formation_code.as_deref(), Some("FS"));
        // Publication entre le numéro et le fermant, sans fermant : la lecture s'arrête à la publication.
        let got = parsed("Cass. soc., 25 novembre 2015 (n° 14-24.444, Publié au bulletin");
        assert_eq!(got[0].0, "Cass. soc., 25 novembre 2015 (n° 14-24.444, Publié au bulletin");
        // Garde : la forme à virgule garde sa lecture et son span.
        let got = parsed("Cass. soc., 25 novembre 2015, n° 14-24.444 (rejet) FS-B suite");
        assert_eq!(got[0].0, "Cass. soc., 25 novembre 2015, n° 14-24.444 (rejet) FS-B");
    }

    /// U12 : `Cass. crim., n° X` : la chambre est lue sans date, confiance moyenne (AM-84).
    #[test]
    fn a_chamber_without_a_date_is_read() {
        let input = "Cass. crim., n° 14-24.444";
        let got = parsed(input);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, input);
        assert_eq!(
            got[0].1.court,
            Some(Court::CourDeCassation { chamber: Some(CassChamber::Crim) })
        );
        assert_eq!(got[0].1.date, None);
        assert_eq!(got[0].1.court_via, None);
        assert_eq!(got[0].1.numbers, vec![pv(14, 24, 444, None)]);
        let found = crate::citation::parse(input);
        assert_eq!(found[0].confidence, Confidence::Medium);
        assert!(!found[0].flags.contains(&Flag::TeteDecisionNonLue));
        for other in [
            "Cass. civ. 1re, pourvoi n° 14-24.444",
            "Cass. soc. n° 14-24.444",
            "Cass. com. 14-24.444",
        ] {
            let got = parsed(other);
            assert_eq!(got.len(), 1, "{other} : {got:?}");
            assert_eq!(got[0].0, other);
            assert_eq!(got[0].1.date, None, "{other}");
        }
    }

    /// U12 AM-84 : une chambre sans date ni numéro derrière n'est pas une décision.
    #[test]
    fn a_chamber_without_a_date_or_a_number_is_not_a_citation() {
        for input in ["Cass. crim., voir ci-dessus", "Cass. soc., pourvoi", "Cass. com. x 12"] {
            assert_eq!(parsed(input), vec![], "{input}");
            assert_eq!(read(input, |c| short_form(c, 0)).map(|hit| hit.end), None, "{input}");
        }
    }

    /// U12 (garde) : sans chambre lue, le numéro reste lu seul (N29).
    #[test]
    fn a_court_without_a_chamber_or_a_date_leaves_the_number_alone() {
        for (input, span) in [
            ("Cass., n° 14-24.444", "n° 14-24.444"),
            ("Cour de cassation, n° 14-24.444", "n° 14-24.444"),
            ("Voir n° 14-24.444", "n° 14-24.444"),
            ("n° 14-24.444", "n° 14-24.444"),
        ] {
            let got = parsed(input);
            assert_eq!(got.len(), 1, "{input} : {got:?}");
            assert_eq!(got[0].0, span, "{input}");
            assert_eq!(got[0].1.court_via, Some(CourtVia::FormeDuNumero), "{input}");
            assert_eq!(got[0].1.court, Some(Court::CourDeCassation { chamber: None }), "{input}");
        }
    }

    /// U12 G4 : la chambre écrite devant « de la Cour de cassation » est lue.
    #[test]
    fn chamber_before_the_court_name_is_read() {
        let input = "Chambre sociale de la Cour de cassation, 25 novembre 2015, n° 14-24.444";
        let got = parsed(input);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, input);
        assert_eq!(
            got[0].1.court,
            Some(Court::CourDeCassation { chamber: Some(CassChamber::Soc) })
        );
        assert_eq!(got[0].1.date, date(2015, 11, 25));
        assert_eq!(got[0].1.court_via, Some(CourtVia::Chambre));
        // Garde : sans le nom de la Cour, la chambre longue garde sa lecture.
        let got = parsed("Chambre sociale, 25 novembre 2015, n° 14-24.444");
        assert_eq!(got[0].0, "Chambre sociale, 25 novembre 2015, n° 14-24.444");
        // « de la Cour » sans « de cassation » n'est pas le nom de la Cour.
        assert!(
            read("chambre sociale de la cour d'appel, 4 juin 2025, n° 24-13.470", |c| {
                long_chamber(c, 0)
            })
            .is_none()
        );
    }

    /// U12 : la forme complète à date à tirets couvre tout le texte (garde de la 3e forme du rapport).
    #[test]
    fn a_right_dashed_head_is_one_citation_covering_the_whole_text_u12() {
        let input = "Cass. soc. 25-11-2015, n° 14-24.444";
        let got = parsed(input);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, input);
        assert_eq!(got[0].1.date, date(2015, 11, 25));
        assert_eq!(
            got[0].1.court,
            Some(Court::CourDeCassation { chamber: Some(CassChamber::Soc) })
        );
        let flags = parsed_flags(input);
        assert!(!flags[0].0.contains(&Flag::TeteDecisionNonLue));
        assert_eq!(crate::citation::parse(input)[0].confidence, Confidence::High);
    }

    /// U12 F1 : le drapeau de tête non lue ne se pose que sur un pourvoi précédé d'une date, d'un
    /// sigle ou d'une chambre.
    #[test]
    fn head_before_flags_only_a_date_or_a_court_just_before_the_number() {
        for text in [
            "Cass. soc., pourvoi n° 24-13.470",
            "Cass. soc., 25/11/15, n° 14-24.444",
            "Cass. soc., 25.11.15, n° 14-24.444",
            "Cass. soc., 25-11-15, n° 14-24.444",
            "Cass. crim., nov. 2015, n° 14-24.444",
            "Cass. crim., req. n° 14-24.444",
            "arrêt, civ. n° 14-24.444",
            "arrêt Cass.soc. n° 14-24.444",
            "arrêt (Cass. 1re civ.) n° 14-24.444",
            "arrêt de la chambre sociale n° 14-24.444",
            "Cour de cassation, n° 14-24.444",
            "Cass. n° 14-24.444",
            "de la Cour de cassation : n° 14-24.444",
            "arrêt du 25 novembre 2015 [n° 14-24.444]",
            "arrêt du 25 novembre 2015 - n° 14-24.444",
            "arrêt du 25 novembre 2015, pourvois n° 14-24.444",
            "arrêt ass. plén. n° 14-24.444",
            "arrêt ch. mixte n° 14-24.444",
            "arrêt 2e civ. n° 14-24.444",
            "arrêt 1ere n° 14-24.444",
            "soc. n° 14-24.444",
            "2015 n° 14-24.444",
            "25/11/15 n° 14-24.444",
            "commerciale n° 14-24.444",
            "arrêt 1re n° 14-24.444",
            "arrêt 1500 n° 14-24.444",
            "arrêt 2099 n° 14-24.444",
        ] {
            let at = text.rfind("n°").map_or(0, |b| text[..b].chars().count());
            assert!(read(text, |c| c.head_before(at)), "{text}");
        }
    }

    /// U12 F1 (garde) : rien n'est signalé quand ce qui précède n'est ni une date ni un sigle.
    #[test]
    fn head_before_ignores_what_is_not_a_head() {
        for text in [
            "n° 14-24.444",
            "voir n° 14-24.444",
            "arrêt et n° 14-24.444",
            "2015. N° 14-24.444",
            "Dans l'arrêt n° 14-24.444",
            "page 12 n° 14-24.444",
            "le 14 n° 14-24.444",
            "arrêt n° 14-24.123 et n° 14-24.444",
            "arrêt 14-24.123, n° 14-24.444",
            "Cass. soc. 25 novembre 2015. N° 14-24.444",
            "civil. n° 14-24.444",
            "commun n° 14-24.444",
            "ancien socle n° 14-24.444",
            "arrêt 1499 n° 14-24.444",
            "arrêt 2100 n° 14-24.444",
            "arrêt 25/11/145 n° 14-24.444",
            "arrêt 5/1 n° 14-24.444",
            "arrêt 123/11/15 n° 14-24.444",
            "arrêt 12/123/15 n° 14-24.444",
            "arrêt xsoc n° 14-24.444",
        ] {
            let at = text
                .rfind("n°")
                .or_else(|| text.rfind("N°"))
                .map_or(0, |b| text[..b].chars().count());
            assert!(!read(text, |c| c.head_before(at)), "{text}");
        }
        // Début de texte : rien avant.
        assert!(!read("n° 14-24.444", |c| c.head_before(0)));
        assert!(!read("  n° 14-24.444", |c| c.head_before(2)));
    }

    /// U12 F1 : le drapeau est posé sur le pourvoi lu seul (à travers `parse`), pas sur la forme
    /// nue (N29) ni sur la forme complète.
    #[test]
    fn the_flag_is_set_on_the_lone_pourvoi_with_an_unread_head() {
        let flagged = |input: &str| {
            crate::citation::parse(input)
                .iter()
                .map(|c| c.flags.contains(&Flag::TeteDecisionNonLue))
                .collect::<Vec<_>>()
        };
        assert_eq!(flagged("Cass. soc., pourvoi n° 24-13.470"), [false]);
        assert_eq!(flagged("Cass.soc. n° 14-24.444"), [true]);
        assert_eq!(flagged("Cass. soc., 25/11/15, n° 14-24.444"), [true]);
        assert_eq!(flagged("Cass. crim., nov. 2015, n° 14-24.444"), [true]);
        assert_eq!(flagged("Cass. soc., req. n° 14-24.444"), [true]);
        assert_eq!(flagged("n° 14-24.444"), [false]);
        assert_eq!(flagged("Cass. soc., 25 novembre 2015, n° 14-24.444"), [false]);
        assert_eq!(flagged("Cass. soc., 25-11-2015, n° 14-24.444"), [false]);
        assert_eq!(flagged("Cass. com., 12 mars 1999 (n° 14-24.444)"), [false]);
    }

    /// U12 (garde) : aucune forme complète du jeu de tests ne porte le drapeau de tête non lue.
    #[test]
    fn complete_forms_get_no_new_flag() {
        for input in [
            "Cass. soc., 25 novembre 2015, n° 14-24.444",
            "Cass. civ. 1re, 4 juin 2025, n° 24-13.470",
            "Cass. 1re civ., 4 juin 2025, n° 24-13.470",
            "Civ. 1re, 4 juin 2025, 24-13.470",
            "Cass. soc., 25 novembre 2015, pourvoi n° 14-24.444 (cassation)",
            "Cass. crim. 25/11/2015, n° 14-24.444",
            "Cass. crim. 25.11.2015 n° 14-24.444",
            "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin",
            "Première chambre civile, 4 juin 2025, 24-13.470",
            "1re Civ., 25 mai 2023, pourvoi n° 21-23.174 (cassation partielle)",
            "Cass. ass. plén., 6 octobre 2006, n° 05-13.255",
        ] {
            for flags in parsed_flags(input) {
                assert!(!flags.0.contains(&Flag::TeteDecisionNonLue), "{input} : {flags:?}");
            }
        }
    }

    /// U12 garantie 14 n° 7 : « N° de pourvoi : » et « n°: » ne sont pas lus, comme en 1.1.0
    /// (aucune citation) ; la forme sans deux-points garde sa lecture.
    #[test]
    fn a_pourvoi_label_with_a_colon_keeps_its_1_1_0_reading_u12() {
        for input in ["N° de pourvoi : 14-24.444", "n°: 14-24.444", "N° de pourvoi: 14-24.444"] {
            assert_eq!(crate::citation::parse(input), vec![], "{input}");
        }
        for (input, span) in
            [("pourvoi n° 14-24.444", "n° 14-24.444"), ("Pourvoi N° 14-24.444", "N° 14-24.444")]
        {
            let got = parsed(input);
            assert_eq!(got.len(), 1, "{input} : {got:?}");
            assert_eq!(got[0].0, span, "{input}");
        }
    }

    /// U12 : `head_before` est borné et linéaire (20 000 caractères).
    #[test]
    fn head_before_is_bounded_and_linear() {
        let input = "Cass. soc., pourvoi n° 24-13.470 ; 25/11/15, ".repeat(450);
        assert!(input.chars().count() > 18_000);
        let started = std::time::Instant::now();
        let n = crate::citation::parse(&input).len();
        assert!(n > 100);
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        for run in [
            format!("{}n° 14-24.444", "9".repeat(20_000)),
            format!("{} n° 14-24.444", "9/".repeat(10_000)),
        ] {
            let n = crate::citation::normalize::normalize(
                &run,
                crate::citation::normalize::Steps::default(),
            );
            let doc = lex::Doc::new(&n);
            let at = run.chars().count() - 12;
            let started = std::time::Instant::now();
            let _ = doc.cur(at).head_before(at);
            assert!(started.elapsed() < std::time::Duration::from_secs(1));
        }
    }

    #[test]
    #[should_panic(expected = "pas une décision")]
    fn the_decision_helper_rejects_other_citations() {
        let other = Candidate::new(
            0..1,
            CitationRef::Unsupported(UnsupportedCite { family: UnsupportedFamily::Ue, key: None }),
        );
        let _ = decision(&other);
    }
}
