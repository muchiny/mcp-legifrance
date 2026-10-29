//! Normalisation avec conservation des offsets (annexe B §3, 03 §8.2).
//!
//! Le texte d'origine passe par cinq étapes, dans cet ordre :
//!
//! 0. (option `strip_html`) balises retirées, entités décodées ;
//! 1. (option `repair_mojibake`) réparation Latin-1 vers UTF-8, tout ou rien ;
//! 2. suppression des caractères de largeur nulle, puis NFC ;
//! 3. repli caractère par caractère (table de l'annexe B §3.2) ;
//! 4. vue « clé » : `fold_char` appliqué à chaque caractère, un pour un.
//!
//! Chaque caractère produit porte la plage d'octets du texte d'origine dont il vient : la table
//! d'offsets ([`OffsetMap`]) se déduit de ces plages à la fin, une seule fois, ce qui revient à
//! composer les tables des étapes. Aucune entrée-sortie, aucune panique.
//!
//! Écart à l'ordre du §3.1, sans effet sur le repli lui-même : les caractères de largeur nulle
//! (U+200B, U+200C, U+200D, U+2060, U+FEFF, U+00AD) sont retirés **avant** la NFC et non après.
//! Sinon `e` + U+200B + U+0301 resterait décomposé en sortie, et la normalisation ne serait pas
//! idempotente (§3.5). Pour la même raison, la NFC est rejouée après la table : un `ʳ` suivi
//! d'une marque devient `r` + marque, qui se compose.

#![allow(dead_code, reason = "STUB(V0) → V2-10 : lu par les grammaires et par `post::arbitrate`")]

use std::borrow::Cow;
use std::ops::Range;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

use crate::text::encoding::repair_latin1_mojibake;

/// Garde-fou du domaine (annexe B §3.4) : au-delà, l'analyse s'arrête à la dernière frontière de
/// caractère avant la borne.
pub(crate) const MAX_INPUT_BYTES: usize = 1 << 20;

/// Longueur maximale, en caractères, d'une balise reconnue à l'étape 0.
const TAG_MAX_CHARS: usize = 80;

/// Longueur maximale, en octets, d'une entité reconnue à l'étape 0 (`&#x1F600;` en compte 9).
const ENTITY_MAX_BYTES: usize = 12;

/// `fold_char` sur le bloc Latin-1 Supplement et Latin Extended-A (U+00C0 à U+017F) : lettre de
/// base minuscule pour les lettres accentuées, minuscule pour les autres (`Æ` → `æ`, `Œ` → `œ`,
/// `Ð` → `ð`), identité pour `×`, `÷`, `ß`, `ĸ`, `ŉ`, `ſ`. Table explicite (annexe B §3.3).
const LATIN_FOLD: [char; 192] = [
    'a', 'a', 'a', 'a', 'a', 'a', '\u{e6}', 'c', 'e', 'e', 'e', 'e', 'i', 'i', 'i', 'i', '\u{f0}',
    'n', 'o', 'o', 'o', 'o', 'o', '\u{d7}', '\u{f8}', 'u', 'u', 'u', 'u', 'y', '\u{fe}', '\u{df}',
    'a', 'a', 'a', 'a', 'a', 'a', '\u{e6}', 'c', 'e', 'e', 'e', 'e', 'i', 'i', 'i', 'i', '\u{f0}',
    'n', 'o', 'o', 'o', 'o', 'o', '\u{f7}', '\u{f8}', 'u', 'u', 'u', 'u', 'y', '\u{fe}', 'y', 'a',
    'a', 'a', 'a', 'a', 'a', 'c', 'c', 'c', 'c', 'c', 'c', 'c', 'c', 'd', 'd', '\u{111}',
    '\u{111}', 'e', 'e', 'e', 'e', 'e', 'e', 'e', 'e', 'e', 'e', 'g', 'g', 'g', 'g', 'g', 'g', 'g',
    'g', 'h', 'h', '\u{127}', '\u{127}', 'i', 'i', 'i', 'i', 'i', 'i', 'i', 'i', 'i', '\u{131}',
    '\u{133}', '\u{133}', 'j', 'j', 'k', 'k', '\u{138}', 'l', 'l', 'l', 'l', 'l', 'l', '\u{140}',
    '\u{140}', '\u{142}', '\u{142}', 'n', 'n', 'n', 'n', 'n', 'n', '\u{149}', '\u{14b}', '\u{14b}',
    'o', 'o', 'o', 'o', 'o', 'o', '\u{153}', '\u{153}', 'r', 'r', 'r', 'r', 'r', 'r', 's', 's',
    's', 's', 's', 's', 's', 's', 't', 't', 't', 't', '\u{167}', '\u{167}', 'u', 'u', 'u', 'u',
    'u', 'u', 'u', 'u', 'u', 'u', 'u', 'u', 'w', 'w', 'y', 'y', 'y', 'z', 'z', 'z', 'z', 'z', 'z',
    '\u{17f}',
];

/// Premier point de code de [`LATIN_FOLD`].
const LATIN_FOLD_FIRST: u32 = 0xC0;

/// Étapes facultatives de la normalisation (`ParseOptions.strip_html` et `.repair_mojibake`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Steps {
    /// Étape 0 : retirer les balises et décoder les entités.
    pub strip_html: bool,
    /// Étape 1 : réparer le mojibake Latin-1 (tout ou rien).
    pub repair_mojibake: bool,
}

/// Texte normalisé, vue clé et tables d'offsets (annexe B §3.4).
#[derive(Clone, Debug)]
pub(crate) struct Normalized {
    /// Texte normalisé (étapes 0 à 3).
    pub text: String,
    /// Vue clé (étape 4), qui a exactement autant de `char` que `text`.
    pub key: String,
    /// Vrai si l'étape 1 a réparé le texte (drapeau `mojibake_repare`).
    pub mojibake_repaired: bool,
    /// Vrai si l'entrée dépassait [`MAX_INPUT_BYTES`] et a été coupée.
    pub truncated: bool,
    /// Table d'offsets : texte normalisé vers texte d'origine.
    offsets: OffsetMap,
    /// Par caractère : `(octet dans key, octet dans text)`.
    key_index: Vec<(u32, u32)>,
}

/// Nature d'un segment de la table d'offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegKind {
    /// Correspondance linéaire octet à octet.
    Copy,
    /// Tout offset intérieur renvoie le début (ou la fin) du segment d'origine.
    Replace,
}

/// Un segment : `norm_len` octets normalisés viennent de `orig_len` octets d'origine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seg {
    norm_start: u32,
    norm_len: u32,
    orig_start: u32,
    orig_len: u32,
    kind: SegKind,
}

/// Table d'offsets : segments triés par `norm_start`, contigus côté normalisé (annexe B §3.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OffsetMap {
    segs: Vec<Seg>,
}

/// Un caractère en cours de traitement et la plage d'origine dont il vient.
#[derive(Clone, Copy, Debug)]
struct Unit {
    ch: char,
    start: u32,
    end: u32,
    /// Vrai si `ch` est le caractère d'origine, inchangé, à sa place : segment `Copy`.
    kept: bool,
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Caractère de largeur nulle, supprimé (annexe B §3.2, classe « Largeur nulle »).
pub(crate) fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{00AD}')
}

/// Repli de l'étape 3 pour `c`, ou `None` s'il est conservé tel quel. `prev` est le caractère
/// produit juste avant : `º` ne devient `°` qu'après un chiffre ou un `n` (`nº`, `1º`).
pub(crate) fn replacement(c: char, prev: Option<char>) -> Option<&'static str> {
    match c {
        '\u{00A0}'
        | '\u{202F}'
        | '\u{2009}'
        | '\u{2007}'
        | '\u{2002}'..='\u{2006}'
        | '\u{2008}'
        | '\u{205F}'
        | '\u{3000}'
        | '\t'
        | '\r'
        | '\n' => Some(" "),
        '\u{2019}' | '\u{2018}' | '\u{02BC}' | '\u{0060}' | '\u{00B4}' | '\u{2032}' => Some("'"),
        '\u{2010}'..='\u{2014}' | '\u{2212}' | '\u{FE63}' | '\u{FF0D}' => Some("-"),
        '\u{2116}' => Some("n°"),
        '\u{02B3}' => Some("r"),
        '\u{1D49}' => Some("e"),
        '\u{1D48}' => Some("d"),
        '\u{02E2}' => Some("s"),
        '\u{00BA}' if prev.is_some_and(|p| p.is_ascii_digit() || matches!(p, 'n' | 'N')) => {
            Some("°")
        }
        _ => None,
    }
}

/// Caractère de la vue clé : minuscule, accents retirés, **un caractère pour un caractère**
/// (annexe B §3.3). Distinct de `text::fold`, qui développe `œ` en `oe` pour le BM25.
pub(crate) fn fold_char(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let table = usize::try_from(u32::from(c).wrapping_sub(LATIN_FOLD_FIRST))
        .ok()
        .and_then(|i| LATIN_FOLD.get(i));
    if let Some(base) = table {
        return *base;
    }
    // Hors du bloc, aucun caractère n'a une minuscule de plusieurs caractères (seul U+0130 en
    // a une, et il est dans la table) : le premier est le seul.
    c.to_lowercase().next().unwrap_or(c)
}

/// Étape 0 : balises et entités. Rend une unité par caractère conservé ou produit.
fn strip_html(input: &str) -> Vec<Unit> {
    let mut units = Vec::with_capacity(input.len());
    let mut pos = 0;
    while let Some(ch) = input[pos..].chars().next() {
        let tail = &input[pos..];
        let found = match ch {
            '<' => tag(tail).map(|(len, space)| (len, space.then_some(' '))),
            '&' => entity(tail).map(|(len, decoded)| (len, Some(decoded))),
            _ => None,
        };
        let end = pos + found.map_or(ch.len_utf8(), |(len, _)| len);
        match found {
            // Balise retirée : rien n'est produit, un trou reste côté origine.
            Some((_, None)) => {}
            // Balise de bloc ou entité : un caractère, qui couvre toute la plage reconnue.
            Some((_, Some(produced))) => {
                units.push(Unit {
                    ch: produced,
                    start: to_u32(pos),
                    end: to_u32(end),
                    kept: false,
                });
            }
            None => units.push(Unit { ch, start: to_u32(pos), end: to_u32(end), kept: true }),
        }
        pos = end;
    }
    units
}

/// Balise reconnue en tête de `tail` (qui commence par `<`) : sa longueur en octets, et vrai si
/// elle vaut une espace (`br`, `p`, `div`, `li`), faux si elle est simplement retirée (`mark`,
/// `sup`, `sub`, `b`, `i`, `em`, `strong`, `u`, `span`, `a`). Toute autre balise, et tout `<`
/// sans `>` proche, restent du texte (`<lettre` d'un texte juridique, `a < b`).
fn tag(tail: &str) -> Option<(usize, bool)> {
    let end = tail
        .char_indices()
        .skip(1)
        .take(TAG_MAX_CHARS)
        .find(|&(_, c)| c == '>' || c == '<')
        .filter(|&(_, c)| c == '>')
        .map(|(i, _)| i)?;
    let inner = &tail[1..end];
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    let name_len = inner.bytes().take_while(u8::is_ascii_alphanumeric).count();
    let (name, after) = inner.split_at(name_len);
    if !(after.is_empty() || after.starts_with(|c: char| c.is_ascii_whitespace() || c == '/')) {
        return None;
    }
    match name.to_ascii_lowercase().as_str() {
        "mark" | "sup" | "sub" | "b" | "i" | "em" | "strong" | "u" | "span" | "a" => {
            Some((end + 1, false))
        }
        "br" | "p" | "div" | "li" => Some((end + 1, true)),
        _ => None,
    }
}

/// Entité reconnue en tête de `tail` (qui commence par `&`) : longueur en octets et caractère.
/// Noms : `nbsp`, `amp`, `apos`, `quot`, `lt`, `gt` ; références numériques `&#8217;` et
/// `&#x2019;`.
fn entity(tail: &str) -> Option<(usize, char)> {
    let semi = tail.bytes().take(ENTITY_MAX_BYTES).position(|b| b == b';')?;
    let name = &tail[1..semi];
    let decoded = match name {
        "nbsp" => Some('\u{00A0}'),
        "amp" => Some('&'),
        "apos" => Some('\''),
        "quot" => Some('"'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        _ => name.strip_prefix('#').and_then(numeric),
    };
    decoded.map(|c| (semi + 1, c))
}

/// Référence numérique d'une entité, sans le `#` : `8217` ou `x2019`.
fn numeric(digits: &str) -> Option<char> {
    let (radix, body) = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => (16, hex),
        None => (10, digits),
    };
    if body.is_empty() || !body.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u32::from_str_radix(body, radix).ok().and_then(char::from_u32)
}

/// Unités d'un texte laissé tel quel : chaque caractère couvre ses propres octets.
fn plain_units(input: &str) -> Vec<Unit> {
    input
        .char_indices()
        .map(|(i, ch)| Unit { ch, start: to_u32(i), end: to_u32(i + ch.len_utf8()), kept: true })
        .collect()
}

/// Étape 1 : réparation du mojibake, tout ou rien (`repair_latin1_mojibake`). Une fois réparé,
/// chaque caractère de `n` octets UTF-8 vient de `n` caractères Latin-1 consécutifs : il couvre
/// la plage d'origine de ces `n` unités.
fn repair(units: Vec<Unit>) -> (Vec<Unit>, bool) {
    let text: String = units.iter().map(|u| u.ch).collect();
    let Cow::Owned(fixed) = repair_latin1_mojibake(&text) else {
        return (units, false);
    };
    let mut pos = 0;
    let repaired = fixed
        .chars()
        .map(|ch| {
            let first = pos;
            pos += ch.len_utf8();
            Unit {
                ch,
                start: units.get(first).map_or(0, |u| u.start),
                end: units.get(pos - 1).map_or(0, |u| u.end),
                kept: false,
            }
        })
        .collect();
    (repaired, true)
}

/// Vrai si `next` reste dans le groupe `group` pour la composition NFC : une marque se colle à
/// son groupe ; un caractère de base ne s'y colle que s'il peut se composer avec lui (jamais
/// sous U+0300, ce qui couvre tout le français ; jamais en ASCII).
fn joins(group: &[Unit], next: char) -> bool {
    if canonical_combining_class(next) != 0 {
        return true;
    }
    if next < '\u{300}' {
        return false;
    }
    let joined: String = group.iter().map(|u| u.ch).chain(Some(next)).nfc().collect();
    let apart: String =
        group.iter().map(|u| u.ch).nfc().chain(Some(next).into_iter().nfc()).collect();
    joined != apart
}

/// Compose un groupe en NFC. Autant de caractères en entrée qu'en sortie : correspondance un
/// pour un ; sinon chaque caractère produit couvre toute la plage du groupe.
fn flush(group: &mut Vec<Unit>, out: &mut Vec<Unit>) {
    if let [only] = group.as_slice()
        && only.ch.is_ascii()
    {
        out.push(*only);
        group.clear();
        return;
    }
    let composed: Vec<char> = group.iter().map(|u| u.ch).nfc().collect();
    let start = group.first().map_or(0, |u| u.start);
    let end = group.last().map_or(0, |u| u.end);
    if composed.len() == group.len() {
        out.extend(group.iter().zip(composed).map(|(u, ch)| Unit {
            ch,
            start: u.start,
            end: u.end,
            kept: u.kept && ch == u.ch,
        }));
    } else {
        out.extend(composed.into_iter().map(|ch| Unit { ch, start, end, kept: false }));
    }
    group.clear();
}

/// Étape 2 : NFC, par groupes (un caractère de base et les marques qui le suivent).
fn compose_nfc(units: Vec<Unit>) -> Vec<Unit> {
    let mut out = Vec::with_capacity(units.len());
    let mut group: Vec<Unit> = Vec::new();
    for unit in units {
        if !group.is_empty() && !joins(&group, unit.ch) {
            flush(&mut group, &mut out);
        }
        group.push(unit);
    }
    flush(&mut group, &mut out);
    out
}

/// Étape 3 : table de repli (annexe B §3.2).
fn fold_table(units: Vec<Unit>) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::with_capacity(units.len());
    for unit in units {
        let prev = out.last().map(|u| u.ch);
        match replacement(unit.ch, prev) {
            Some(text) => {
                out.extend(text.chars().map(|ch| Unit { ch, kept: false, ..unit }));
            }
            None => out.push(unit),
        }
    }
    out
}

/// Coupe `input` à [`MAX_INPUT_BYTES`], sur une frontière de caractère.
fn truncate(input: &str) -> (&str, bool) {
    if input.len() <= MAX_INPUT_BYTES {
        return (input, false);
    }
    let mut end = MAX_INPUT_BYTES;
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    (&input[..end], true)
}

/// Normalise `input` : les cinq étapes de l'annexe B §3.1.
pub(crate) fn normalize(input: &str, steps: Steps) -> Normalized {
    let (input, truncated) = truncate(input);
    let units = if steps.strip_html { strip_html(input) } else { plain_units(input) };
    let (units, mojibake_repaired) =
        if steps.repair_mojibake { repair(units) } else { (units, false) };
    let units: Vec<Unit> = units.into_iter().filter(|u| !is_zero_width(u.ch)).collect();
    // Seconde composition après la table : `ʳ` + U+0301 donne `r` + U+0301, qui se compose en `ŕ`.
    let units = compose_nfc(fold_table(compose_nfc(units)));
    Normalized::from_units(&units, truncated, mojibake_repaired)
}

impl Normalized {
    /// Construit le texte, la clé et les tables à partir des unités finales.
    fn from_units(units: &[Unit], truncated: bool, mojibake_repaired: bool) -> Self {
        let mut text = String::with_capacity(units.len());
        let mut key = String::with_capacity(units.len());
        let mut key_index = Vec::with_capacity(units.len());
        let mut segs: Vec<Seg> = Vec::new();
        for unit in units {
            let norm_start = to_u32(text.len());
            key_index.push((to_u32(key.len()), norm_start));
            text.push(unit.ch);
            key.push(fold_char(unit.ch));
            let norm_len = to_u32(unit.ch.len_utf8());
            let orig_len = unit.end - unit.start;
            let kind = if unit.kept { SegKind::Copy } else { SegKind::Replace };
            if let Some(prev) = segs.last_mut().filter(|p| p.extends(kind, unit)) {
                prev.norm_len += norm_len;
                if kind == SegKind::Copy {
                    prev.orig_len += orig_len;
                }
            } else {
                segs.push(Seg { norm_start, norm_len, orig_start: unit.start, orig_len, kind });
            }
        }
        Self { text, key, mojibake_repaired, truncated, offsets: OffsetMap { segs }, key_index }
    }

    /// Plage d'origine d'une plage de `text` (frontières de caractère de `text`) : le début
    /// du caractère d'origine qui contient le premier octet, la fin de celui qui contient le
    /// dernier. Une plage vide rend une plage vide.
    pub(crate) fn orig_range(&self, norm: Range<usize>) -> Range<usize> {
        let start = self.offsets.to_orig(to_u32(norm.start));
        if norm.start >= norm.end {
            return start as usize..start as usize;
        }
        start as usize..self.offsets.to_orig_end(to_u32(norm.end)) as usize
    }

    /// Offset dans `text` du caractère qui commence à l'offset `key_offset` de `key`.
    pub(crate) fn key_to_text(&self, key_offset: usize) -> Option<usize> {
        if key_offset == self.key.len() {
            return Some(self.text.len());
        }
        let wanted = to_u32(key_offset);
        let i = self.key_index.binary_search_by_key(&wanted, |&(k, _)| k).ok()?;
        self.key_index.get(i).map(|&(_, t)| t as usize)
    }

    /// Offset dans `key` du caractère qui commence à l'offset `text_offset` de `text`.
    pub(crate) fn text_to_key(&self, text_offset: usize) -> Option<usize> {
        if text_offset == self.text.len() {
            return Some(self.key.len());
        }
        let wanted = to_u32(text_offset);
        let i = self.key_index.binary_search_by_key(&wanted, |&(_, t)| t).ok()?;
        self.key_index.get(i).map(|&(k, _)| k as usize)
    }
}

impl Seg {
    /// Vrai si l'unité suivante, de nature `kind`, prolonge ce segment.
    fn extends(&self, kind: SegKind, unit: &Unit) -> bool {
        self.kind == kind
            && match kind {
                SegKind::Copy => self.orig_start + self.orig_len == unit.start,
                // Expansion (`№` donne `n°`) : plusieurs caractères pour la même plage.
                SegKind::Replace => {
                    self.orig_start == unit.start && self.orig_start + self.orig_len == unit.end
                }
            }
    }
}

impl OffsetMap {
    /// Segment qui contient l'octet normalisé `norm`.
    fn seg_at(&self, norm: u32) -> Option<&Seg> {
        let i = self.segs.partition_point(|s| s.norm_start + s.norm_len <= norm);
        self.segs.get(i)
    }

    /// Fin, côté origine, du dernier segment (0 pour un texte vide).
    fn orig_end(&self) -> u32 {
        self.segs.last().map_or(0, |s| s.orig_start + s.orig_len)
    }

    /// Offset d'origine du caractère qui commence en `norm` (début de span). Un offset au bord
    /// du texte, ou au-delà, rend la fin du dernier segment.
    pub(crate) fn to_orig(&self, norm: u32) -> u32 {
        match self.seg_at(norm) {
            Some(s) if s.kind == SegKind::Copy => s.orig_start + (norm - s.norm_start),
            Some(s) => s.orig_start,
            None => self.orig_end(),
        }
    }

    /// Fin, dans l'origine, du caractère d'origine qui couvre `norm_end - 1` (fin de span). Un
    /// segment `Replace` rend toute sa plage ; une suppression finale reste hors du span.
    pub(crate) fn to_orig_end(&self, norm_end: u32) -> u32 {
        match self.seg_at(norm_end.saturating_sub(1)) {
            Some(s) if s.kind == SegKind::Copy => s.orig_start + (norm_end - s.norm_start),
            Some(s) => s.orig_start + s.orig_len,
            None => self.orig_end(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    const PLAIN: Steps = Steps { strip_html: false, repair_mojibake: false };
    const HTML: Steps = Steps { strip_html: true, repair_mojibake: false };
    const MOJIBAKE: Steps = Steps { strip_html: false, repair_mojibake: true };
    const BOTH: Steps = Steps { strip_html: true, repair_mojibake: true };

    /// Entrée, étapes, plage normalisée, texte normalisé attendu, plage d'origine attendue.
    type Case<'a> = (&'a str, Steps, Range<usize>, &'a str, Range<usize>);

    /// Caractères qui exercent la table de repli, la NFC et l'étape 0.
    const SPECIALS: [char; 28] = [
        'a', 'n', 'N', 'e', 'r', '1', ' ', '.', '\u{e9}', '\u{301}', '\u{323}', '\u{a0}',
        '\u{202f}', '\u{2019}', '\u{2013}', '\u{2116}', '\u{ba}', '\u{200b}', '\u{ad}', '\u{2b3}',
        '\u{ab}', '<', '>', '&', ';', '#', '/', '\u{c3}',
    ];

    fn soup() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                3 => any::<char>(),
                7 => proptest::sample::select(SPECIALS.to_vec()),
            ],
            0..48,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn markup() -> impl Strategy<Value = String> {
        "[a-z<>&;#/ x0-9]{0,40}"
    }

    fn all_steps() -> impl Strategy<Value = Steps> {
        (any::<bool>(), any::<bool>())
            .prop_map(|(strip_html, repair_mojibake)| Steps { strip_html, repair_mojibake })
    }

    fn boundaries(s: &str) -> Vec<usize> {
        s.char_indices().map(|(i, _)| i).chain([s.len()]).collect()
    }

    /// Les 11 classes de l'annexe B §3.2, recopiées du tableau : (entrées, sortie).
    fn annex_table() -> Vec<(Vec<char>, &'static str)> {
        vec![
            (
                vec![
                    '\u{a0}', '\u{202f}', '\u{2009}', '\u{2007}', '\u{2002}', '\u{2003}',
                    '\u{2004}', '\u{2005}', '\u{2006}', '\u{2008}', '\u{205f}', '\u{3000}', '\t',
                    '\r', '\n',
                ],
                " ",
            ),
            (vec!['\u{2019}', '\u{2018}', '\u{2bc}', '\u{60}', '\u{b4}', '\u{2032}'], "'"),
            (
                vec![
                    '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2212}',
                    '\u{fe63}', '\u{ff0d}',
                ],
                "-",
            ),
            (vec!['\u{2116}'], "n°"),
            (vec!['\u{2b3}'], "r"),
            (vec!['\u{1d49}'], "e"),
            (vec!['\u{1d48}'], "d"),
            (vec!['\u{2e2}'], "s"),
        ]
    }

    const ZERO_WIDTH: [char; 6] =
        ['\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{ad}'];

    #[test]
    fn fold_table_matches_annex_b_3_2() {
        let table = annex_table();
        for (inputs, output) in &table {
            for &c in inputs {
                let code = u32::from(c);
                assert_eq!(replacement(c, None), Some(*output), "U+{code:04X}");
                assert!(!is_zero_width(c));
            }
        }
        for c in ZERO_WIDTH {
            let code = u32::from(c);
            assert!(is_zero_width(c), "U+{code:04X}");
            assert_eq!(replacement(c, None), None);
        }
        // º (U+00BA) : degré seulement après un chiffre ou un n, sinon conservé.
        for prev in ['0', '7', 'n', 'N'] {
            assert_eq!(replacement('\u{ba}', Some(prev)), Some("°"));
        }
        assert_eq!(replacement('\u{ba}', None), None);
        assert_eq!(replacement('\u{ba}', Some('x')), None);
        // Tout le reste du plan Unicode est inchangé (guillemets et ligatures compris).
        let listed: Vec<char> = table.iter().flat_map(|(inputs, _)| inputs.clone()).collect();
        for cp in 0..=0x0010_FFFF_u32 {
            let Some(c) = char::from_u32(cp) else { continue };
            if listed.contains(&c) || ZERO_WIDTH.contains(&c) {
                continue;
            }
            if c == '\u{ba}' {
                continue;
            }
            assert_eq!(replacement(c, Some('1')), None, "U+{cp:04X} ne figure pas dans la table");
            assert!(!is_zero_width(c), "U+{cp:04X}");
        }
        for kept in ['«', '»', '“', '”', '„', 'œ', 'æ'] {
            assert_eq!(normalize(&kept.to_string(), PLAIN).text, kept.to_string());
        }
    }

    #[test]
    fn fold_table_is_applied_end_to_end() {
        let input = "a\u{a0}b\u{2019}c\u{2013}d\u{2116}e 1\u{ba} n\u{ba} x\u{ba}f\u{2b3}\u{1d49}\u{ad}\u{200b}";
        assert_eq!(normalize(input, PLAIN).text, "a b'c-dn°e 1° n° xºfre");
        assert_eq!(normalize("1\u{1d48}\u{2e2}", PLAIN).text, "1ds");
    }

    #[test]
    fn fold_table_output_is_composed_again() {
        // `ʳ` devient `r`, qui se compose avec la marque qui suit : la sortie reste en NFC.
        let once = normalize("\u{2b3}\u{301}", PLAIN);
        assert_eq!(once.text, "\u{155}");
        assert_eq!(normalize(&once.text, PLAIN).text, once.text);
        assert_eq!(once.orig_range(0..2), 0..4);
    }

    #[test]
    fn identity_on_plain_ascii_is_a_single_copy_segment() {
        let n = normalize("Art. L. 761-1 du C. civ.", PLAIN);
        assert_eq!(n.text, "Art. L. 761-1 du C. civ.");
        assert_eq!(n.key, "art. l. 761-1 du c. civ.");
        assert_eq!(n.offsets.segs.len(), 1);
        assert_eq!(n.orig_range(5..11), 5..11);
        assert!(!n.mojibake_repaired && !n.truncated);
    }

    #[test]
    fn offset_map_roundtrip() {
        // (entrée, étapes, plage normalisée, texte normalisé attendu, plage d'origine attendue)
        let cases: [Case<'_>; 13] = [
            ("x\u{a0}y", PLAIN, 1..2, " ", 1..3),
            ("x\u{a0}y", PLAIN, 0..3, "x y", 0..4),
            ("\u{2116} 12", PLAIN, 0..1, "n", 0..3),
            ("\u{2116} 12", PLAIN, 1..3, "°", 0..3),
            ("e\u{301}x", PLAIN, 0..2, "é", 0..3),
            ("e\u{301}x", PLAIN, 2..3, "x", 3..4),
            ("a\u{200b}b", PLAIN, 0..2, "ab", 0..5),
            ("a\u{200b}b", PLAIN, 1..2, "b", 4..5),
            ("ab\u{200b}", PLAIN, 0..2, "ab", 0..2),
            ("\u{200b}ab", PLAIN, 0..1, "a", 3..4),
            ("1<sup>re</sup> civ.", HTML, 0..3, "1re", 0..8),
            ("l&#8217;article", HTML, 1..2, "'", 1..8),
            ("pr\u{c3}\u{a9}sident", MOJIBAKE, 2..4, "é", 2..6),
        ];
        for (input, steps, norm, expected_text, orig) in cases {
            let n = normalize(input, steps);
            assert_eq!(n.text.get(norm.clone()), Some(expected_text), "{input:?}");
            assert_eq!(n.orig_range(norm), orig, "{input:?}");
        }
    }

    #[test]
    fn copy_segments_map_byte_for_byte() {
        let input = "é\u{a0}Cass. civ. 1re, 4 juin 2025 \u{2013} n° 24-13.470";
        let n = normalize(input, PLAIN);
        for needle in ["Cass.", "civ.", "4 juin 2025", "24-13.470"] {
            let start = n.text.find(needle).expect("présent dans le texte normalisé");
            let orig = n.orig_range(start..start + needle.len());
            assert_eq!(&input[orig], needle);
        }
    }

    #[test]
    fn empty_ranges_and_empty_text() {
        let n = normalize("", PLAIN);
        assert_eq!(n.text, "");
        assert_eq!(n.orig_range(0..0), 0..0);
        assert_eq!(n.orig_range(7..9), 0..0);
        assert_eq!(n.offsets.to_orig(0), 0);
        let n = normalize("\u{200b}ab\u{200b}", PLAIN);
        assert_eq!(n.orig_range(0..0), 3..3);
        assert_eq!(n.orig_range(2..2), 5..5);
        assert_eq!(n.offsets.to_orig_end(0), 3);
        assert_eq!(n.offsets.to_orig(99), 5);
        assert_eq!(n.offsets.to_orig_end(99), 5);
    }

    #[test]
    fn key_view_is_one_to_one() {
        let n = normalize("Éé Œuvre Ça \u{c0}", PLAIN);
        assert_eq!(n.key, "ee œuvre ca a");
        assert_eq!(n.key.chars().count(), n.text.chars().count());
        assert_ne!(n.key.len(), n.text.len());
        // L'octet 4 est au milieu de « œ » dans la clé.
        assert_eq!(n.key_to_text(4), None);
        // Aller-retour des offsets : chaque caractère de la clé retrouve son caractère de texte.
        for ((k, t), (kc, tc)) in n.key_index.iter().zip(n.key.chars().zip(n.text.chars())) {
            assert_eq!(n.key[*k as usize..].chars().next(), Some(kc));
            assert_eq!(n.text[*t as usize..].chars().next(), Some(tc));
            assert_eq!(n.key_to_text(*k as usize), Some(*t as usize));
            assert_eq!(n.text_to_key(*t as usize), Some(*k as usize));
        }
        assert_eq!(n.key_to_text(n.key.len()), Some(n.text.len()));
        assert_eq!(n.text_to_key(n.text.len()), Some(n.key.len()));
        // « é » fait deux octets dans le texte et un dans la clé : l'octet 1 n'est pas une frontière.
        assert_eq!(n.text_to_key(1), None);
        assert_eq!(n.key_to_text(1), Some(2));
    }

    proptest! {
        #[test]
        fn key_view_is_one_to_one_for_any_input(input in soup(), steps in all_steps()) {
            let n = normalize(&input, steps);
            prop_assert_eq!(n.key.chars().count(), n.text.chars().count());
            prop_assert_eq!(n.key_index.len(), n.text.chars().count());
            let mut text_chars = n.text.chars();
            let mut key_chars = n.key.chars();
            for &(k, t) in &n.key_index {
                prop_assert!(n.key.is_char_boundary(k as usize));
                prop_assert!(n.text.is_char_boundary(t as usize));
                let (tc, kc) = (text_chars.next(), key_chars.next());
                prop_assert_eq!(kc, tc.map(fold_char));
                prop_assert_eq!(n.key_to_text(k as usize), Some(t as usize));
                prop_assert_eq!(n.text_to_key(t as usize), Some(k as usize));
            }
        }

        #[test]
        fn offsets_are_boundaries_and_monotone(input in soup(), steps in all_steps()) {
            let n = normalize(&input, steps);
            let cuts: Vec<usize> = boundaries(&n.text).into_iter().take(24).collect();
            for &a in &cuts {
                let mut last_end = None;
                for &b in cuts.iter().filter(|&&b| b >= a) {
                    let r = n.orig_range(a..b);
                    prop_assert!(r.start <= r.end);
                    prop_assert!(input.get(r.clone()).is_some(), "{input:?} {a}..{b} -> {r:?}");
                    if b > a {
                        prop_assert!(last_end.is_none_or(|e| e <= r.end));
                        last_end = Some(r.end);
                    }
                }
            }
            let starts: Vec<usize> = cuts.iter().map(|&a| n.orig_range(a..a).start).collect();
            prop_assert!(starts.windows(2).all(|w| w[0] <= w[1]));
        }

        #[test]
        fn normalization_is_idempotent_without_steps_0_and_1(input in soup()) {
            let once = normalize(&input, PLAIN);
            let twice = normalize(&once.text, PLAIN);
            prop_assert_eq!(&twice.text, &once.text);
            prop_assert_eq!(&twice.key, &once.key);
        }

        #[test]
        fn nfc_step_equals_str_nfc(input in soup()) {
            let composed: String = compose_nfc(plain_units(&input)).iter().map(|u| u.ch).collect();
            prop_assert_eq!(composed, input.nfc().collect::<String>());
        }

        #[test]
        fn markup_never_panics_and_stays_in_bounds(input in markup()) {
            let n = normalize(&input, HTML);
            for a in boundaries(&n.text) {
                prop_assert!(input.get(n.orig_range(a..a)).is_some());
            }
        }
    }

    #[test]
    fn nfc_composes_by_group() {
        let units = |s: &str| compose_nfc(plain_units(s));
        let text = |s: &str| units(s).iter().map(|u| u.ch).collect::<String>();
        assert_eq!(text("e\u{301}"), "é");
        assert_eq!(text("e\u{323}\u{302}"), "\u{1ec7}");
        // Hangul : L + V + T se composent par paires successives.
        assert_eq!(text("\u{1100}\u{1161}\u{11a8}"), "\u{ac01}");
        // Réordonnancement canonique des marques, sans composition : autant de caractères.
        assert_eq!(text("x\u{301}\u{323}"), "x\u{323}\u{301}");
        assert_eq!(text("x\u{301}\u{302}"), "x\u{301}\u{302}");
        // Composition partielle : moins de caractères que d'unités, plage du groupe entier.
        assert_eq!(text("a\u{301}\u{323}"), "\u{1ea1}\u{301}");
        // Singleton canonique : U+212A devient K.
        assert_eq!(text("\u{212a}"), "K");
        // Un caractère de base ne se colle pas à un autre qui ne se compose pas avec lui.
        assert_eq!(text("e\u{301}\u{0e01}"), "é\u{0e01}");
        assert_eq!(units("e\u{301}").len(), 1);
    }

    #[test]
    fn html_removes_tags_and_decodes_entities() {
        let t = |s: &str| normalize(s, HTML).text;
        assert_eq!(t("pour une <mark>République</mark>"), "pour une République");
        assert_eq!(t("1<sup>re</sup> civ."), "1re civ.");
        assert_eq!(t("a<br/>b<br>c<br />d"), "a b c d");
        assert_eq!(t("<P CLASS=\"x\">un</p><div>deux</div><li>trois"), " un  deux  trois");
        assert_eq!(
            t("<b>g</b><I>i</I><em>e</em><strong>s</strong><u>u</u><span a>s</span><a>a</a>"),
            "giesusa"
        );
        assert_eq!(t("&nbsp;&amp;&apos;&quot;&lt;&gt;"), " &'\"<>");
        assert_eq!(t("l&#8217;art. l&#x2019;art. l&#X2019;art."), "l'art. l'art. l'art.");
        // Balises inconnues, `<` isolé, entités invalides : texte tel quel.
        assert_eq!(t("<lettre>"), "<lettre>");
        assert_eq!(t("a < b <mark>c</mark>"), "a < b c");
        assert_eq!(t("a <mark"), "a <mark");
        assert_eq!(t("<markx>"), "<markx>");
        assert_eq!(t("<mark/>x<>"), "x<>");
        assert_eq!(
            t("&unknown; &#; &#x; &#xZZ; &#1114112; &amp"),
            "&unknown; &#; &#x; &#xZZ; &#1114112; &amp"
        );
        assert_eq!(t("&"), "&");
        // Sans l'option, tout reste.
        assert_eq!(normalize("<mark>x</mark>&amp;", PLAIN).text, "<mark>x</mark>&amp;");
    }

    #[test]
    fn mojibake_is_repaired_only_when_asked_and_reported() {
        let broken = "pr\u{c3}\u{a9}sident n\u{c2}\u{b0} 5";
        let fixed = normalize(broken, MOJIBAKE);
        assert!(fixed.mojibake_repaired);
        assert_eq!(fixed.text, "président n° 5");
        // Non demandé : le texte reste, tel quel (après le repli des seuls caractères de la table).
        let raw = normalize(broken, PLAIN);
        assert!(!raw.mojibake_repaired);
        assert_eq!(raw.text.chars().count(), broken.chars().count());
        // Demandé, mais rien à réparer ou réparation refusée : pas de drapeau.
        assert!(!normalize("président", MOJIBAKE).mojibake_repaired);
        assert!(!normalize("\u{c3}\u{41}", MOJIBAKE).mojibake_repaired);
        assert!(!normalize("\u{c3}\u{a9}\u{20ac}", MOJIBAKE).mojibake_repaired);
        // L'espace insécable de `à` (Ã + U+00A0) est réparée avant le repli des espaces.
        assert_eq!(normalize("\u{c3}\u{a0} la", MOJIBAKE).text, "à la");
        // Avec les balises : la plage d'origine suit les octets de l'entrée.
        let n = normalize("<mark>pr\u{c3}\u{a9}</mark>", BOTH);
        assert_eq!(n.text, "pré");
        assert_eq!(n.orig_range(2..4), 8..12);
    }

    #[test]
    fn long_input_is_cut_at_a_character_boundary() {
        let mut input = "a".repeat(MAX_INPUT_BYTES - 1);
        input.push('é');
        let n = normalize(&input, PLAIN);
        assert!(n.truncated);
        assert_eq!(n.text.len(), MAX_INPUT_BYTES - 1);
        let exact = "b".repeat(MAX_INPUT_BYTES);
        assert!(!normalize(&exact, PLAIN).truncated);
        assert_eq!(normalize(&exact, PLAIN).text.len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn fold_char_table_and_fallbacks() {
        assert_eq!(fold_char('A'), 'a');
        assert_eq!(fold_char('z'), 'z');
        assert_eq!(fold_char('1'), '1');
        for (from, to) in [('é', 'e'), ('È', 'e'), ('Ë', 'e'), ('ç', 'c'), ('Ç', 'c'), ('ÿ', 'y')]
        {
            assert_eq!(fold_char(from), to, "{from}");
        }
        assert_eq!(fold_char('œ'), 'œ');
        assert_eq!(fold_char('Œ'), 'œ');
        assert_eq!(fold_char('æ'), 'æ');
        assert_eq!(fold_char('Æ'), 'æ');
        assert_eq!(fold_char('İ'), 'i');
        assert_eq!(fold_char('Σ'), 'σ');
        assert_eq!(fold_char('\u{2013}'), '\u{2013}');
        assert_eq!(fold_char('\u{ba}'), '\u{ba}');
    }

    #[test]
    fn fold_char_table_agrees_with_canonical_decomposition() {
        for cp in 0xC0..0x180_u32 {
            let c = char::from_u32(cp).unwrap_or('?');
            let base = c.to_string().nfd().next().unwrap_or(c);
            let expected = if base.is_ascii_alphabetic() {
                base.to_ascii_lowercase()
            } else {
                c.to_lowercase().next().unwrap_or(c)
            };
            assert_eq!(fold_char(c), expected, "U+{cp:04X}");
        }
    }

    #[test]
    fn only_the_table_range_has_multi_char_lowercase() {
        for cp in 0..=0x0010_FFFF_u32 {
            let Some(c) = char::from_u32(cp) else { continue };
            if c.to_lowercase().count() != 1 {
                assert_eq!(c, '\u{130}', "U+{cp:04X}");
            }
        }
    }

    proptest! {
        #[test]
        fn fold_char_is_idempotent(c in any::<char>()) {
            let folded = fold_char(c);
            prop_assert_eq!(fold_char(folded), folded);
        }
    }
}
