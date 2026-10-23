//! Lexer HTML partagé : champs courts (`clean`, 03 §7.3) et texte long découpé en
//! paragraphes (03 §7.2). Écrit à la main, sans crate HTML (00 §2 et §4) : tolérant, en un
//! seul passage, linéaire, sans récursion ni panique. Aucun texte n'est perdu quand une
//! fermante manque, à une exception près : un ouvrant qui a vu un `=` (forme d'attribut) et
//! que rien ne ferme jusqu'à la fin de l'entrée est supprimé jusqu'à la fin (capture tronquée
//! dans un attribut, `consult_kali.json`).
//!
//! # Lexing (03 §7.2, étape 1)
//!
//! - Balise : `<`, `/` facultatif, nom `[A-Za-z][A-Za-z0-9_:-]*`, puis des attributs jusqu'au
//!   `>`. Un guillemet simple ou double qui ouvre une valeur (après `=` et d'éventuels blancs)
//!   court jusqu'au même guillemet, qui masque `<` et `>`. Un guillemet sans partenaire
//!   jusqu'à la fin de l'entrée est un octet ordinaire : il n'avale pas le reste du texte.
//! - Un `<` hors guillemets avant le `>` rend l'ouvrant littéral : son texte est gardé et la
//!   lecture reprend à ce second `<` (`x <a <b>y` donne `x <a y`).
//! - Tout autre `<` est du texte (`a < b`, `<3`, `</>`, `<!DOCTYPE …>`).
//! - Commentaire `<!-- … -->` : supprimé. S'il n'y a plus de `-->`, le `<!--` reste du texte
//!   et la lecture continue juste après lui.
//! - Ouvrant ni fermé par `>` ni interrompu par `<` jusqu'à la fin de l'entrée : supprimé
//!   jusqu'à la fin seulement s'il a vu un `=` (`<a href="x`, `<a title=Code d [...]`) ;
//!   sinon son `<` est littéral et la lecture reprend juste après (`si x <y alors` reste
//!   intact) : perdre la fin d'un texte juridique serait pire qu'y laisser une balise.
//!
//! # Classement (étape 2), insensible à la casse
//!
//! - Bloc, ouvrante ou fermante : `br`, `p`, `div`, `li`, `ul`, `ol`, `tr`, `table`,
//!   `blockquote`, `h1` à `h6` ([`Event::Break`]). La fermante `</br>` (52 paires `<BR></BR>`
//!   dans `consult_juri_constit.json`) est un bloc ordinaire ([`Block::Other`]) et non un
//!   second `<br>`.
//! - Cellule : `td`, `th` ([`Event::Cell`]).
//! - Supprimé avec son contenu : tout nom qui commence par `rech_`, plus `script` et `style`.
//!   L'ouvrant le plus extérieur prend la première fermante de son nom ; tant qu'elle n'est pas
//!   lue, les événements sont retenus. Sans fermante jusqu'à la fin, seul l'ouvrant est retiré
//!   et les événements retenus sont rendus : aucun texte perdu. Un ouvrant auto-fermant
//!   (`<RECH_X/>`) n'a pas de contenu. Une fermante cachée dans un commentaire ou une valeur
//!   citée ne compte pas.
//! - En ligne : tout le reste, retiré sans espace (`1<sup>er</sup>` donne `1er`).
//!
//! # Entités (étape 3) et caractères invisibles (03 §7.3, étape 3)
//!
//! [`decode_entities`] s'applique au texte, donc après le retrait des balises : un `&lt;`
//! décodé ne crée pas de balise. [`strip_invisible`] passe ensuite, de sorte qu'un caractère
//! invisible venu d'une entité (`&#173;`, `&#x202E;`) est retiré et compté comme un brut.
//!
//! # Linéarité
//!
//! - La boucle de [`scan`] cherche le prochain `<` par `find`, appelle `lex` et avance d'au
//!   moins un octet (`len.max(1)`, sans opérateur mutable) : elle termine toujours.
//! - `lex` lit vers l'avant, par un itérateur qui ne recule jamais. Balise : tout ce qu'il a
//!   lu est consommé. Ouvrant littéral : la lecture suivante reprend à l'octet où il s'est
//!   arrêté (le second `<`), donc deux lectures de `lex` ne se recouvrent pas.
//! - Ouvrant sans `=` que rien ne ferme jusqu'à la fin : son `<` est littéral et `lex` a lu
//!   tout le reste. Ce reste ne contient aucun `<` (le premier aurait arrêté `lex`, et aucune
//!   valeur citée n'a pu en masquer, faute de `=`) : le `find` suivant le parcourt une fois et
//!   la boucle s'arrête. Le reste est donc lu deux fois au plus, une seule fois par entrée.
//! - Guillemet d'une valeur : une copie de l'itérateur cherche le partenaire. Trouvé, la
//!   lecture saute après lui (octets lus une fois). Absent, la copie a lu jusqu'à la fin, mais
//!   cela n'arrive qu'une fois par sorte de guillemet : il n'y a plus de ce guillemet après
//!   lui, donc plus de recherche à faire.
//! - Commentaire : `find("-->")` réussit (texte consommé) ou échoue ; le premier échec pose
//!   `no_comment_end` et tout `<!--` suivant est littéral sans nouvelle recherche.
//! - Supprimés avec contenu : aucune recherche en avant. Chaque événement est retenu au plus
//!   une fois puis effacé ou rendu une fois ; chaque ouvrant est empilé puis dépilé une fois ;
//!   la table `outermost` donne l'ouvrant le plus extérieur d'un nom en temps constant.
//! - [`decode_entities`] lit au plus 12 octets par `&` ; [`strip_invisible`], un passage.

use std::collections::HashMap;

/// Événement du lexer, dans l'ordre de la source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event<'a> {
    /// Texte de la source, non vide, entités non décodées.
    Text(&'a str),
    /// Balise de bloc.
    Break(Block),
    /// Cellule de tableau (`td`, `th`).
    Cell,
}

/// Sorte de coupure de bloc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Block {
    /// `<br>` : chaque occurrence compte (deux `<br>` font une ligne vide dans `clean`).
    Br,
    /// `<p>` ou `</p>` : paragraphe.
    P,
    /// Autre bloc (`div`, `li`, `h1`…, et `</br>`).
    Other,
}

/// Classe d'un nom de balise (03 §7.2, étape 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Coupure de bloc.
    Break(Block),
    /// Cellule : une espace.
    Cell,
    /// Supprimé avec son contenu.
    Drop,
    /// En ligne : retiré sans espace.
    Inline,
}

/// Classe d'un nom de balise déjà mis en minuscules.
fn classify(name_lower: &str) -> Class {
    match name_lower {
        "br" => Class::Break(Block::Br),
        "p" => Class::Break(Block::P),
        "div" | "li" | "ul" | "ol" | "tr" | "table" | "blockquote" | "h1" | "h2" | "h3" | "h4"
        | "h5" | "h6" => Class::Break(Block::Other),
        "td" | "th" => Class::Cell,
        "script" | "style" => Class::Drop,
        _ if name_lower.starts_with("rech_") => Class::Drop,
        _ => Class::Inline,
    }
}

/// Lit `html` en un seul passage et rend ses événements à `sink`, dans l'ordre (grammaire et
/// classement : doc du module).
///
/// Cas limites des éléments supprimés avec leur contenu et des ouvrants littéraux :
///
/// - Ouvrants imbriqués du même nom (`RECH_*`, `script`, `style`) : la première fermante de ce
///   nom clôt l'ouvrant le plus extérieur, et tout ce qui les sépare part avec lui
///   (`<RECH_A>x<RECH_A>y</RECH_A>z</RECH_A>w` donne `zw`).
/// - Le contenu d'un `script` ou d'un `style` est lexé comme du HTML, pas comme du texte brut :
///   une valeur citée d'une pseudo-balise peut y masquer la fermante
///   (`<script>if (a<b && c="</script>") x</script>y` donne `y`).
/// - Un ouvrant rendu littéral (un `<` avant son `>`) rend tel quel le balisage de ses valeurs
///   citées, `RECH_*` compris : `<a t="<RECH_X>y</RECH_X>" <i>z` donne le texte
///   `<a t="<RECH_X>y</RECH_X>" ` puis `z`.
///
/// Mémoire : O(n), mais tant qu'un élément supprimé attend sa fermante, chaque événement est
/// retenu (24 octets, plus le nom de chaque ouvrant en attente). Le relecteur a mesuré un pic
/// d'environ 12,7 fois la taille de l'entrée sur un `<script>` non fermé suivi de `a<p>`
/// répétés (un événement tous les 2 octets), contre une fois la taille de l'entrée sans
/// élément en attente.
pub(crate) fn scan<'a>(html: &'a str, mut sink: impl FnMut(Event<'a>)) {
    scan_into(html, &mut sink);
}

/// Corps de [`scan`], non générique : `cargo-llvm-cov` résume une fonction générique par son
/// instance la mieux couverte, pas par l'union des instances (CONTRIBUTING.md).
fn scan_into<'a>(html: &'a str, sink: &mut dyn FnMut(Event<'a>)) {
    let mut out = Output { sink, pending: Vec::new(), outermost: HashMap::new(), held: Vec::new() };
    let mut no_comment_end = false;
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        let (text, tail) = rest.split_at(lt);
        out.text(text);
        let len = match lex(tail, &mut no_comment_end) {
            Lexeme::Literal(len) => {
                out.text(&tail[..len]);
                len
            }
            Lexeme::Skip(len) => len,
            Lexeme::Tag(tag) => {
                let len = tag.len;
                out.tag(tag);
                len
            }
        };
        // Au moins un octet consommé par tour, même si `lex` rendait 0 : la boucle termine.
        rest = &tail[len.max(1)..];
    }
    out.text(rest);
    out.finish();
}

/// Ce que `lex` lit en tête d'un reste qui commence par `<`.
enum Lexeme {
    /// `<` littéral : les `len` premiers octets sont du texte.
    Literal(usize),
    /// Commentaire, ou ouvrant à attributs non fermé jusqu'à la fin : `len` octets retirés.
    Skip(usize),
    /// Balise reconnue.
    Tag(Tag),
}

/// Balise reconnue par `lex`.
struct Tag {
    /// Nom en minuscules.
    name: String,
    /// `</nom …>`.
    closing: bool,
    /// Dernier octet avant le `>` : `/`.
    self_closing: bool,
    /// Longueur en octets, `>` compris.
    len: usize,
}

/// Lit le commentaire ou la balise en tête de `tail`, qui commence par `<`.
///
/// `no_comment_end` : plus aucun `-->` dans la suite de l'entrée (posé au premier échec).
fn lex(tail: &str, no_comment_end: &mut bool) -> Lexeme {
    if let Some(body) = tail.strip_prefix("<!--") {
        if !*no_comment_end {
            if let Some(end) = body.find("-->") {
                return Lexeme::Skip("<!--".len() + end + "-->".len());
            }
            *no_comment_end = true;
        }
        return Lexeme::Literal(1);
    }
    let mut bytes = tail.bytes().enumerate().skip(1).peekable();
    let closing = bytes.next_if(|&(_, b)| b == b'/').is_some();
    let Some((start, _)) = bytes.next_if(|&(_, b)| b.is_ascii_alphabetic()) else {
        return Lexeme::Literal(1);
    };
    while bytes.next_if(|&(_, b)| is_name_byte(b)).is_some() {}
    let end = bytes.peek().map_or(tail.len(), |&(i, _)| i);
    let name = tail[start..end].to_ascii_lowercase();
    // `value_next` : le dernier octet significatif est `=`, un guillemet ouvre donc une valeur.
    // `saw_eq` : un `=` a été vu, l'ouvrant a donc la forme d'une balise à attributs.
    let mut value_next = false;
    let mut saw_eq = false;
    let mut last = 0;
    while let Some((i, b)) = bytes.next() {
        match b {
            b'>' => {
                let tag = Tag { name, closing, self_closing: last == b'/', len: i + 1 };
                return Lexeme::Tag(tag);
            }
            b'<' => return Lexeme::Literal(i),
            b'"' | b'\'' if value_next => {
                // Partenaire cherché sur une copie : absent, le guillemet reste ordinaire.
                let mut ahead = bytes.clone();
                if ahead.any(|(_, c)| c == b) {
                    bytes = ahead;
                }
                value_next = false;
            }
            b'=' => {
                value_next = true;
                saw_eq = true;
            }
            _ => value_next &= b.is_ascii_whitespace(),
        }
        last = b;
    }
    // Fin de l'entrée sans `>` ni `<` : un ouvrant à attributs (capture tronquée) part jusqu'à
    // la fin ; tout autre `<` est littéral, et comme aucune valeur citée n'a été sautée (il en
    // faut un `=`), le reste ne contient aucun `<` : `scan` le parcourt une seule fois de plus.
    if saw_eq { Lexeme::Skip(tail.len()) } else { Lexeme::Literal(1) }
}

/// Octet d'un nom de balise après la première lettre.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-')
}

/// Sortie de [`scan`] : retient les événements tant qu'un élément supprimé avec son contenu
/// attend sa fermante.
struct Output<'a, 's> {
    sink: &'s mut dyn FnMut(Event<'a>),
    /// Noms des éléments supprimés avec leur contenu, ouverts et pas encore fermés.
    pending: Vec<String>,
    /// Par nom de `pending` : position de son entrée la plus extérieure et longueur de `held`
    /// à son ouverture.
    outermost: HashMap<String, (usize, usize)>,
    /// Événements retenus tant que `pending` n'est pas vide.
    held: Vec<Event<'a>>,
}

impl<'a> Output<'a, '_> {
    fn emit(&mut self, event: Event<'a>) {
        if self.pending.is_empty() {
            (self.sink)(event);
        } else {
            self.held.push(event);
        }
    }

    fn text(&mut self, text: &'a str) {
        if !text.is_empty() {
            self.emit(Event::Text(text));
        }
    }

    fn tag(&mut self, tag: Tag) {
        match (classify(&tag.name), tag.closing) {
            (Class::Drop, false) => {
                if !tag.self_closing {
                    self.open(tag.name);
                }
            }
            (Class::Drop, true) => self.close(&tag.name),
            (Class::Break(Block::Br), true) => self.emit(Event::Break(Block::Other)),
            (Class::Break(block), _) => self.emit(Event::Break(block)),
            (Class::Cell, _) => self.emit(Event::Cell),
            (Class::Inline, _) => {}
        }
    }

    /// Ouvrant d'un élément supprimé avec son contenu.
    fn open(&mut self, name: String) {
        let first = (self.pending.len(), self.held.len());
        self.outermost.entry(name.clone()).or_insert(first);
        self.pending.push(name);
    }

    /// Fermante d'un élément supprimé avec son contenu : l'ouvrant le plus extérieur de ce nom
    /// est fermé ; son contenu et les ouvrants qu'il contient disparaissent. Sans ouvrant de
    /// ce nom, la fermante est seulement retirée.
    fn close(&mut self, name: &str) {
        if let Some(&(index, held)) = self.outermost.get(name) {
            self.held.truncate(held);
            for name in self.pending.drain(index..) {
                if self.outermost.get(&name).is_some_and(|&(first, _)| first >= index) {
                    self.outermost.remove(&name);
                }
            }
        }
    }

    /// Fin de l'entrée : les ouvrants encore en attente n'ont pas de fermante, seul l'ouvrant
    /// est retiré et les événements retenus sont rendus.
    fn finish(self) {
        let Self { sink, held, .. } = self;
        held.into_iter().for_each(sink);
    }
}

/// Décode les entités de `text` (03 §7.2, étape 3) ; une entité inconnue ou invalide reste
/// verbatim.
///
/// - Nommées : les 136 de [`ENTITIES`], sensibles à la casse, avec `;`.
/// - Numériques `&#NNN;` et `&#xHH;` (`X` permis) : tout scalaire Unicode sauf NUL
///   (`&#0;` reste littéral), U+0080 à U+009F remappés vers Windows-1252 comme en HTML
///   ([`WINDOWS_1252`] : `&#146;` donne `’`, `&#150;` donne `–`). Un non-scalaire
///   (surrogate, au-delà de U+10FFFF) reste littéral. Un caractère invisible ainsi décodé est
///   retiré ensuite par [`strip_invisible`].
pub(crate) fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        let (before, tail) = rest.split_at(amp);
        out.push_str(before);
        let len = if let Some((ch, len)) = entity(tail) {
            out.push(ch);
            len
        } else {
            out.push('&');
            1
        };
        // Au moins un octet consommé par tour : la boucle termine toujours.
        rest = &tail[len.max(1)..];
    }
    out.push_str(rest);
    out
}

/// Entité en tête de `tail` : (caractère, longueur avec `;`).
fn entity(tail: &str) -> Option<(char, usize)> {
    // Fenêtre bornée en octets : `;` est ASCII, donc `end` est une frontière de caractère.
    let end = tail.bytes().take(12).position(|b| b == b';')?;
    let name = &tail[1..end];
    let ch = match name.strip_prefix('#') {
        Some(number) => numeric(number)?,
        None => {
            ENTITIES.binary_search_by(|&(known, _)| known.cmp(name)).ok().map(|i| ENTITIES[i].1)?
        }
    };
    Some((ch, end + 1))
}

/// Entité numérique sans `&#` ni `;` : décimale, ou hexadécimale après `x` ou `X`.
fn numeric(number: &str) -> Option<char> {
    let (digits, radix) = match number.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16),
        None => (number, 10),
    };
    // `from_str_radix` accepterait un signe `+` : chiffres seuls.
    if !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let code = u32::from_str_radix(digits, radix).ok()?;
    char::from_u32(code).filter(|&c| c != '\0').map(windows_1252)
}

/// Remap des références numériques U+0080 à U+009F (WHATWG, HTML §13.2.5.80 « numeric
/// character reference end state ») : un texte HTML y désigne le caractère Windows-1252 du
/// même octet (`&#146;` donne `’`). Les cinq codes non attribués (0x81, 0x8D, 0x8F, 0x90,
/// 0x9D) restent des contrôles C1, que [`strip_invisible`] retire et compte.
const WINDOWS_1252: [char; 32] = [
    '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}', '\u{8f}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}', '\u{178}',
];

/// `c` passé par [`WINDOWS_1252`] s'il est entre U+0080 et U+009F, inchangé sinon.
fn windows_1252(c: char) -> char {
    // Sous U+0080, `wrapping_sub` déborde ; au-delà de U+009F, l'indice dépasse la table.
    let index = u32::from(c).wrapping_sub(0x80);
    usize::try_from(index).ok().and_then(|i| WINDOWS_1252.get(i)).copied().unwrap_or(c)
}

/// Entités nommées, triées par octets pour [`entity`] : les 5 de XML, les 96 latin-1 de HTML 4
/// (U+00A0 à U+00FF) et 35 caractères typographiques ou spéciaux de HTML 4.
const ENTITIES: &[(&str, char)] = &[
    ("AElig", 'Æ'),
    ("Aacute", 'Á'),
    ("Acirc", 'Â'),
    ("Agrave", 'À'),
    ("Aring", 'Å'),
    ("Atilde", 'Ã'),
    ("Auml", 'Ä'),
    ("Ccedil", 'Ç'),
    ("Dagger", '‡'),
    ("ETH", 'Ð'),
    ("Eacute", 'É'),
    ("Ecirc", 'Ê'),
    ("Egrave", 'È'),
    ("Euml", 'Ë'),
    ("Iacute", 'Í'),
    ("Icirc", 'Î'),
    ("Igrave", 'Ì'),
    ("Iuml", 'Ï'),
    ("Ntilde", 'Ñ'),
    ("OElig", 'Œ'),
    ("Oacute", 'Ó'),
    ("Ocirc", 'Ô'),
    ("Ograve", 'Ò'),
    ("Oslash", 'Ø'),
    ("Otilde", 'Õ'),
    ("Ouml", 'Ö'),
    ("Prime", '″'),
    ("Scaron", 'Š'),
    ("THORN", 'Þ'),
    ("Uacute", 'Ú'),
    ("Ucirc", 'Û'),
    ("Ugrave", 'Ù'),
    ("Uuml", 'Ü'),
    ("Yacute", 'Ý'),
    ("Yuml", 'Ÿ'),
    ("aacute", 'á'),
    ("acirc", 'â'),
    ("acute", '´'),
    ("aelig", 'æ'),
    ("agrave", 'à'),
    ("amp", '&'),
    ("apos", '\''),
    ("aring", 'å'),
    ("atilde", 'ã'),
    ("auml", 'ä'),
    ("bdquo", '„'),
    ("brvbar", '¦'),
    ("bull", '•'),
    ("ccedil", 'ç'),
    ("cedil", '¸'),
    ("cent", '¢'),
    ("circ", 'ˆ'),
    ("copy", '©'),
    ("curren", '¤'),
    ("dagger", '†'),
    ("deg", '°'),
    ("divide", '÷'),
    ("eacute", 'é'),
    ("ecirc", 'ê'),
    ("egrave", 'è'),
    ("emsp", '\u{2003}'),
    ("ensp", '\u{2002}'),
    ("eth", 'ð'),
    ("euml", 'ë'),
    ("euro", '€'),
    ("fnof", 'ƒ'),
    ("frac12", '½'),
    ("frac14", '¼'),
    ("frac34", '¾'),
    ("gt", '>'),
    ("hellip", '…'),
    ("iacute", 'í'),
    ("icirc", 'î'),
    ("iexcl", '¡'),
    ("igrave", 'ì'),
    ("iquest", '¿'),
    ("iuml", 'ï'),
    ("laquo", '«'),
    ("ldquo", '“'),
    ("lrm", '\u{200e}'),
    ("lsaquo", '‹'),
    ("lsquo", '‘'),
    ("lt", '<'),
    ("macr", '¯'),
    ("mdash", '—'),
    ("micro", 'µ'),
    ("middot", '·'),
    ("minus", '−'),
    ("nbsp", '\u{a0}'),
    ("ndash", '–'),
    ("not", '¬'),
    ("ntilde", 'ñ'),
    ("oacute", 'ó'),
    ("ocirc", 'ô'),
    ("oelig", 'œ'),
    ("ograve", 'ò'),
    ("ordf", 'ª'),
    ("ordm", 'º'),
    ("oslash", 'ø'),
    ("otilde", 'õ'),
    ("ouml", 'ö'),
    ("para", '¶'),
    ("permil", '‰'),
    ("plusmn", '±'),
    ("pound", '£'),
    ("prime", '′'),
    ("quot", '"'),
    ("raquo", '»'),
    ("rdquo", '”'),
    ("reg", '®'),
    ("rlm", '\u{200f}'),
    ("rsaquo", '›'),
    ("rsquo", '’'),
    ("sbquo", '‚'),
    ("scaron", 'š'),
    ("sect", '§'),
    ("shy", '\u{ad}'),
    ("sup1", '¹'),
    ("sup2", '²'),
    ("sup3", '³'),
    ("szlig", 'ß'),
    ("thinsp", '\u{2009}'),
    ("thorn", 'þ'),
    ("tilde", '˜'),
    ("times", '×'),
    ("trade", '™'),
    ("uacute", 'ú'),
    ("ucirc", 'û'),
    ("ugrave", 'ù'),
    ("uml", '¨'),
    ("uuml", 'ü'),
    ("yacute", 'ý'),
    ("yen", '¥'),
    ("yuml", 'ÿ'),
    ("zwj", '\u{200d}'),
    ("zwnj", '\u{200c}'),
];

/// Retire de `text` les caractères invisibles et rend leur nombre (03 §7.3, étape 3).
///
/// Jeu retiré : U+00AD, U+061C, U+200B à U+200F, U+2060, U+FEFF, les contrôles C0 et C1 (et
/// U+007F) sauf `\t`, `\n` et `\r`, les contrôles bidirectionnels U+202A à U+202E et U+2066 à
/// U+2069, les tags U+E0000 à U+E007F. Un texte officiel français n'en a aucun usage légitime,
/// et ils peuvent masquer ou réordonner le texte montré (MEN-06).
///
/// Exception : VT (U+000B), FF (U+000C) et NEL (U+0085), contrôles qui sont aussi des blancs,
/// deviennent une espace au lieu de disparaître, pour ne pas coller les deux mots qu'ils
/// séparent ; ils sont comptés comme les autres. (Une référence `&#133;` n'en produit pas :
/// [`decode_entities`] la remappe vers `…`.)
pub(crate) fn strip_invisible(text: &str) -> (String, u32) {
    let mut removed = 0_u32;
    let mut visible = String::with_capacity(text.len());
    for c in text.chars() {
        if is_invisible(c) {
            removed = removed.saturating_add(1);
            if is_blank_control(c) {
                visible.push(' ');
            }
        } else {
            visible.push(c);
        }
    }
    (visible, removed)
}

/// Contrôle du jeu de [`strip_invisible`] qui est aussi un blanc : VT, FF, NEL.
fn is_blank_control(c: char) -> bool {
    matches!(c, '\u{b}' | '\u{c}' | '\u{85}')
}

/// Caractère du jeu de [`strip_invisible`].
fn is_invisible(c: char) -> bool {
    let control = c.is_control() && !matches!(c, '\t' | '\n' | '\r');
    control
        || matches!(
            c,
            '\u{ad}'
                | '\u{61c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2060}'
                | '\u{feff}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{e0000}'..='\u{e007f}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::{fmt::Write as _, sync::mpsc, thread, time::Duration};

    use Block::{Br, Other, P};
    use Event::{Break, Cell, Text};

    /// Événements de `scan`, dans l'ordre.
    fn events(html: &str) -> Vec<Event<'_>> {
        let mut out = Vec::new();
        scan(html, |event| out.push(event));
        out
    }

    /// Rendu lisible des événements : le texte tel quel, chaque coupure par un repère.
    fn rendered(html: &str) -> String {
        let mut out = String::new();
        scan(html, |event| match event {
            Text(text) => out.push_str(text),
            Break(Br) => out.push_str("[br]"),
            Break(P) => out.push_str("[p]"),
            Break(Other) => out.push_str("[/]"),
            Cell => out.push_str("[td]"),
        });
        out
    }

    /// Texte d'un événement, `None` pour une coupure.
    fn text_slice(event: Event<'_>) -> Option<&str> {
        match event {
            Text(text) => Some(text),
            Break(_) | Cell => None,
        }
    }

    #[test]
    fn text_slice_keeps_only_text() {
        let texts: Vec<&str> = events("a<br>b<td>c").into_iter().filter_map(text_slice).collect();
        assert_eq!(texts, ["a", "b", "c"]);
    }

    #[test]
    fn classify_table() {
        let cases = [
            ("br", Class::Break(Br)),
            ("p", Class::Break(P)),
            ("div", Class::Break(Other)),
            ("li", Class::Break(Other)),
            ("ul", Class::Break(Other)),
            ("ol", Class::Break(Other)),
            ("tr", Class::Break(Other)),
            ("table", Class::Break(Other)),
            ("blockquote", Class::Break(Other)),
            ("h1", Class::Break(Other)),
            ("h2", Class::Break(Other)),
            ("h3", Class::Break(Other)),
            ("h4", Class::Break(Other)),
            ("h5", Class::Break(Other)),
            ("h6", Class::Break(Other)),
            ("td", Class::Cell),
            ("th", Class::Cell),
            ("script", Class::Drop),
            ("style", Class::Drop),
            ("rech_ecli", Class::Drop),
            ("rech_", Class::Drop),
            // En ligne : tout le reste, dont les noms voisins des précédents.
            ("rech", Class::Inline),
            ("xrech_a", Class::Inline),
            ("scripts", Class::Inline),
            ("h7", Class::Inline),
            ("mark", Class::Inline),
            ("a", Class::Inline),
            ("sup", Class::Inline),
            ("span", Class::Inline),
        ];
        for (name, class) in cases {
            assert_eq!(classify(name), class, "{name}");
        }
    }

    #[test]
    fn plain_text_is_one_event() {
        assert_eq!(events("abc é"), [Text("abc é")]);
        assert_eq!(events(""), Vec::new());
    }

    #[test]
    fn inline_tags_vanish_and_blocks_break() {
        assert_eq!(events("1<sup>er</sup>"), [Text("1"), Text("er")]);
        assert_eq!(
            events("a<br>b<br/>c<BR />d"),
            [Text("a"), Break(Br), Text("b"), Break(Br), Text("c"), Break(Br), Text("d")]
        );
        assert_eq!(events("<p align=\"left\">a</P>"), [Break(P), Text("a"), Break(P)]);
        assert_eq!(events("<li>a</LI>"), [Break(Other), Text("a"), Break(Other)]);
        assert_eq!(events("<td>a</TH>"), [Cell, Text("a"), Cell]);
        assert_eq!(rendered("<p>a</p><li>b<td>c<br>"), "[p]a[p][/]b[td]c[br]");
    }

    #[test]
    fn closing_br_is_a_plain_block_break() {
        // 52 paires `<BR></BR>` (consult_juri_constit.json) : la fermante n'est pas un
        // second `<br>`, `clean` n'y ajoute donc pas de ligne vide.
        assert_eq!(events("a<BR></BR>b"), [Text("a"), Break(Br), Break(Other), Text("b")]);
    }

    #[test]
    fn literal_less_than_is_text() {
        assert_eq!(rendered("a < b"), "a < b");
        assert_eq!(rendered("<3 <> </> </ a> <!x> <é> fin <"), "<3 <> </> </ a> <!x> <é> fin <");
        // Un `<` avant le `>` : l'ouvrant est littéral, le lexer reprend au second `<`.
        assert_eq!(rendered("x <a <b>y"), "x <a y");
        assert_eq!(rendered("a<b<c>d"), "a<bd");
        // Les guillemets d'un ouvrant littéral restent du texte, sans relecture.
        assert_eq!(rendered("<a t=\"<b>\" <c>d"), "<a t=\"<b>\" d");
    }

    #[test]
    fn tag_names_and_attributes() {
        assert_eq!(rendered("a<x1 z>b<a.b>c<Aé>d"), "abcd");
        // `_`, `:`, `-` et les chiffres font partie du nom : la fermante doit le citer en entier.
        for name in ["rech_a_b", "rech_a:b", "rech_a-b", "rech_a1", "RECH_AB"] {
            let html = format!("<{name}>x</rech_a>y</{name}>z");
            assert_eq!(rendered(&html), "z", "{name}");
        }
    }

    #[test]
    fn quoted_attribute_values_hide_angle_brackets() {
        assert_eq!(rendered("<a title=\"a>b\">x</a>"), "x");
        assert_eq!(rendered("<a title='a>b<c'>x</a>"), "x");
        assert_eq!(rendered("<a title = \"a>b\">x</a>"), "x");
        // Un guillemet qui n'ouvre pas une valeur (pas après `=`) est un octet ordinaire.
        assert_eq!(rendered("<a l'x>y'>z"), "y'>z");
        assert_eq!(rendered("<a x=1 'a>b'>y"), "b'>y");
        // Guillemet sans partenaire jusqu'à la fin : octet ordinaire, la balise finit au `>`.
        assert_eq!(rendered("<a x=\"y>z"), "z");
        assert_eq!(rendered("<a x=\"1\" y=\"2>z"), "z");
    }

    #[test]
    fn unclosed_tag_at_end_is_dropped_only_with_an_attribute() {
        // Un `=` vu : forme d'attribut, capture tronquée, supprimée jusqu'à la fin.
        assert_eq!(rendered("abc <a href=\"x"), "abc ");
        assert_eq!(rendered("abc <a href=x"), "abc ");
        assert_eq!(rendered("abc <a title = 'x y"), "abc ");
        // consult_kali.json : capture tronquée dans la valeur de `title`.
        assert_eq!(rendered("voir <a href=\"/x?a=1&b=2\" title=\"Code d [...]"), "voir ");
        // Sans `=`, le `<` est littéral et rien n'est perdu.
        assert_eq!(rendered("abc <br"), "abc <br");
        assert_eq!(rendered("abc </p"), "abc </p");
        assert_eq!(rendered("abc <a b c"), "abc <a b c");
        assert_eq!(
            rendered("si x <y alors la peine est doublée"),
            "si x <y alors la peine est doublée"
        );
        // Après le `<` littéral, la lecture reprend : une balise plus loin reste lue.
        assert_eq!(rendered("x <y z<br>w"), "x <y z[br]w");
    }

    #[test]
    fn comments_are_dropped_unless_unclosed() {
        assert_eq!(rendered("a<!-- b <p> -->c"), "ac");
        assert_eq!(rendered("a<!---->c<!-- x --->d"), "acd");
        assert_eq!(rendered("a<!-- b"), "a<!-- b");
        // Plus aucun `-->` : chaque `<!--` suivant reste littéral.
        assert_eq!(rendered("a<!-- b<!-- c<br>d"), "a<!-- b<!-- c[br]d");
        assert_eq!(rendered("a-->b"), "a-->b");
    }

    #[test]
    fn rech_script_and_style_are_dropped_with_content() {
        assert_eq!(rendered("a<RECH_ECLI>ECLI:FR:CC:2025:2025.1173.QPC</RECH_ECLI>b"), "ab");
        assert_eq!(rendered("a<rech_x>b<p>c</RECH_X >d"), "ad");
        assert_eq!(rendered("<script type=\"x\">if (a < b) {}</script>y<style>p{}</style>z"), "yz");
    }

    #[test]
    fn dropped_element_without_closing_tag_keeps_its_text() {
        assert_eq!(rendered("<RECH_ECLI>texte sans fermante"), "texte sans fermante");
        assert_eq!(rendered("<script>a<br>b"), "a[br]b");
        // Fermante seule, fermante d'un autre nom : rien n'est retiré de plus. Fermante tronquée
        // sans `=` : son `<` est littéral (règle bornée), le texte retenu est rendu.
        assert_eq!(rendered("a</script>b"), "ab");
        assert_eq!(rendered("<RECH_A>x</RECH_B>y"), "xy");
        assert_eq!(rendered("<RECH_A>x</RECH_A"), "x</RECH_A");
        // Ouvrant auto-fermant : aucun contenu à retirer.
        assert_eq!(rendered("<RECH_A/>b</RECH_A>c"), "bc");
    }

    #[test]
    fn nested_dropped_elements() {
        // L'ouvrant le plus extérieur prend la première fermante de son nom.
        assert_eq!(rendered("<script>a<script>b</script>c</script>d"), "cd");
        // Fermante d'un élément intérieur : seul son contenu part.
        assert_eq!(rendered("<style>x<script>y</script>z"), "xz");
        assert_eq!(rendered("<script>a<style>b<script>c</style>d"), "ad");
        assert_eq!(rendered("x<script>a</script>b<style>c<script>d</script>e"), "xbce");
        // Fermante d'un élément extérieur : l'intérieur part avec lui, puis est oublié.
        assert_eq!(rendered("<script>a<style>b</script>c<style>d</style>e"), "ce");
        // Fermante cachée dans un commentaire ou une valeur citée : ignorée.
        assert_eq!(rendered("<script>a<!-- </script> -->b</script>c"), "c");
        assert_eq!(rendered("<RECH_A>a<b t=\"</RECH_A>\">b</RECH_A>c"), "c");
        // Doc de `scan` : imbrication du même nom (M-1), script lexé comme du HTML (M-2),
        // ouvrant littéral qui rend ses valeurs citées telles quelles (M-3).
        assert_eq!(rendered("<RECH_A>x<RECH_A>y</RECH_A>z</RECH_A>w"), "zw");
        assert_eq!(rendered("<script>if (a<b && c=\"</script>\") x</script>y"), "y");
        assert_eq!(rendered("<a t=\"<RECH_X>y</RECH_X>\" <i>z"), "<a t=\"<RECH_X>y</RECH_X>\" z");
    }

    #[test]
    fn named_entities_table_is_sorted_and_complete() {
        // 5 entités XML, 96 latin-1 (U+00A0 à U+00FF), 35 typographiques ou spéciales de HTML 4.
        assert_eq!(ENTITIES.len(), 136);
        assert!(ENTITIES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for &(name, ch) in ENTITIES {
            assert_eq!(decode_entities(&format!("a&{name};b")), format!("a{ch}b"), "{name}");
        }
    }

    #[test]
    fn named_entities_are_case_sensitive() {
        assert_eq!(decode_entities("&Eacute;&eacute;&EACUTE;"), "Éé&EACUTE;");
        assert_eq!(decode_entities("&unknown; & seul &; &amp"), "&unknown; & seul &; &amp");
        assert_eq!(decode_entities("&amp;lt; &laquo;x&raquo; l&rsquo;"), "&lt; «x» l’");
    }

    #[test]
    fn numeric_entities() {
        assert_eq!(decode_entities("&#233;&#xE9;&#XE9;&#x0e9;&#00233;"), "ééééé");
        assert_eq!(decode_entities("&#1;&#x202E;&#xE0041;"), "\u{1}\u{202e}\u{e0041}");
        // NUL, non-scalaires, signes et chiffres invalides : verbatim.
        let kept =
            "&#0;&#x0;&#xD800;&#57343;&#x110000;&#1114112;&#+65;&#x+41;&#x;&#X;&#;&#1a;&#xg;";
        assert_eq!(decode_entities(kept), kept);
    }

    #[test]
    fn numeric_c1_references_are_windows_1252() {
        // Attendu tiré du codec `cp1252` de Python ; les cinq octets non attribués restent C1.
        let all = (0x80..=0x9f).fold(String::new(), |mut all, code| {
            write!(all, "&#{code};").unwrap();
            all
        });
        assert_eq!(
            decode_entities(&all),
            "\u{20ac}\u{81}\u{201a}\u{192}\u{201e}\u{2026}\u{2020}\u{2021}\u{2c6}\u{2030}\u{160}\
             \u{2039}\u{152}\u{8d}\u{17d}\u{8f}\u{90}\u{2018}\u{2019}\u{201c}\u{201d}\u{2022}\
             \u{2013}\u{2014}\u{2dc}\u{2122}\u{161}\u{203a}\u{153}\u{9d}\u{17e}\u{178}"
        );
        assert_eq!(decode_entities("&#146;&#x92;&#X96;&#128;"), "’’–€");
        // Bornes voisines : inchangées.
        assert_eq!(decode_entities("&#127;&#160;&#x7F;&#xA0;"), "\u{7f}\u{a0}\u{7f}\u{a0}");
        assert_eq!(windows_1252('a'), 'a');
        assert_eq!(windows_1252('\u{10ffff}'), '\u{10ffff}');
    }

    #[test]
    fn entity_window_is_twelve_bytes() {
        assert_eq!(decode_entities("&#x0010FFFF;"), "\u{10ffff}");
        assert_eq!(decode_entities("&#x00010FFFF;"), "&#x00010FFFF;");
        // Octet 12 au milieu d'un caractère multi-octet : texte conservé, sans panique.
        assert_eq!(decode_entities("&ammmmmmmmmmé;"), "&ammmmmmmmmmé;");
    }

    #[test]
    fn strip_invisible_set_and_neighbours() {
        let removed = [
            '\u{0}',
            '\u{7}',
            '\u{8}',
            '\u{e}',
            '\u{1f}',
            '\u{7f}',
            '\u{80}',
            '\u{84}',
            '\u{86}',
            '\u{9f}',
            '\u{ad}',
            '\u{61c}',
            '\u{200b}',
            '\u{200c}',
            '\u{200d}',
            '\u{200e}',
            '\u{200f}',
            '\u{202a}',
            '\u{202e}',
            '\u{2060}',
            '\u{2066}',
            '\u{2069}',
            '\u{feff}',
            '\u{e0000}',
            '\u{e0041}',
            '\u{e007f}',
        ];
        for c in removed {
            assert_eq!(strip_invisible(&format!("a{c}b")), ("ab".to_owned(), 1), "{c:?}");
        }
        let kept = [
            '\t',
            '\n',
            '\r',
            ' ',
            '\u{a0}',
            '\u{ac}',
            '\u{ae}',
            '\u{61b}',
            '\u{61d}',
            '\u{200a}',
            '\u{2010}',
            '\u{2029}',
            '\u{202f}',
            '\u{205f}',
            '\u{2061}',
            '\u{2065}',
            '\u{206a}',
            '\u{fefe}',
            '\u{dffff}',
            '\u{e0080}',
            'é',
            '’',
        ];
        for c in kept {
            assert_eq!(strip_invisible(&format!("a{c}b")), (format!("a{c}b"), 0), "{c:?}");
        }
        assert_eq!(strip_invisible("\u{200b}\u{200b}x\u{feff}"), ("x".to_owned(), 3));
        // VT, FF, NEL : contrôles qui sont aussi des blancs, remplacés par une espace et comptés.
        for c in ['\u{b}', '\u{c}', '\u{85}'] {
            assert_eq!(strip_invisible(&format!("a{c}b")), ("a b".to_owned(), 1), "{c:?}");
        }
    }

    /// Plafond de chaque entrée de 1 Mio. Un passage linéaire prend quelques dizaines de
    /// millisecondes, même instrumenté ; un passage quadratique, des minutes. Le travail
    /// tourne dans un fil et le test n'attend pas sa fin : un mutant quadratique échoue au
    /// bout de 2 s au lieu de dépasser le délai de cargo-mutants (TIMEOUT).
    const BUDGET: Duration = Duration::from_secs(2);
    const MIB: usize = 1 << 20;

    fn finishes_within_budget(input: String, work: fn(&str)) {
        let (done, wait) = mpsc::channel();
        thread::spawn(move || {
            work(&input);
            let _ = done.send(());
        });
        assert_eq!(wait.recv_timeout(BUDGET), Ok(()));
    }

    fn scan_all(html: &str) {
        let mut count = 0_usize;
        scan(html, |_| count += 1);
        assert!(count <= html.len());
    }

    #[test]
    fn scan_is_linear() {
        let units = [
            "<",
            "<a ",
            "<a <b ",
            "<a x=\"",
            "<a x='",
            "<!--",
            "<!-- -->",
            "<script>",
            "</script>",
            "<RECH_A>",
            "<p>",
            "a<b>",
            "<a x=\"y\" ",
        ];
        for unit in units {
            finishes_within_budget(unit.repeat(MIB / unit.len()), scan_all);
        }
        // Noms distincts sans fermante, puis des fermantes d'autres noms.
        let mut distinct = String::new();
        for i in 0..40_000 {
            write!(distinct, "<RECH_{i}>").unwrap();
        }
        for i in 0..40_000 {
            write!(distinct, "</RECH_X{i}>").unwrap();
        }
        finishes_within_budget(distinct, scan_all);
        // Fermantes en attente : 1 Mio d'événements retenus puis relâchés.
        finishes_within_budget(format!("<script>{}", "a<br>".repeat(MIB / 5)), scan_all);
        // Ouvrant sans `=` que rien ne ferme : `<` littéral, le reste relu une fois.
        finishes_within_budget(format!("x <y {}", "z ".repeat(MIB / 2)), scan_all);
    }

    #[test]
    fn decoding_and_stripping_are_linear() {
        for unit in ["&", "&#", "&#x1", "&amp", "&amp;", "&#173;", "\u{200b}"] {
            finishes_within_budget(unit.repeat(MIB / unit.len()), |text| {
                let (visible, _) = strip_invisible(&decode_entities(text));
                assert!(visible.len() <= text.len());
            });
        }
    }

    proptest! {
        #[test]
        fn scan_text_comes_from_the_input(parts in proptest::collection::vec(
            prop_oneof![
                "<", ">", "/", "\"", "'", "=", " ", "!--", "-->", "a", "br", "p", "li", "td",
                "script", "RECH_E", "é", "&amp;",
            ],
            0..40,
        )) {
            let html = parts.concat();
            let mut texts = Vec::new();
            scan(&html, |event| texts.extend(text_slice(event)));
            // Chaque texte est une tranche non vide de la source, dans l'ordre : rien n'est fabriqué.
            let mut rest = html.as_str();
            for text in texts {
                let after = rest.find(text).map(|at| at + text.len());
                prop_assert!(!text.is_empty() && after.is_some());
                rest = &rest[after.unwrap_or_default()..];
            }
        }

        /// Conservation (et non seulement provenance) : du texte sans `<` ni `&` entre des
        /// balises construites (en ligne, blocs, cellules, commentaires, valeurs citées qui
        /// masquent `<` et `>`, ouvrants supprimés avec contenu dont aucune fermante n'existe,
        /// ouvrant auto-fermant, ouvrant à attributs tronqué en fin d'entrée) : la concaténation
        /// des `Text` est exactement l'entrée moins ses balises.
        #[test]
        fn scan_keeps_all_text_between_tags(
            parts in proptest::collection::vec(
                prop_oneof![
                    "[^<&]{0,8}".prop_map(|text| (text.clone(), text)),
                    prop_oneof![
                        Just("<b>"),
                        Just("</b>"),
                        Just("<br/>"),
                        Just("</BR>"),
                        Just("<p align=\"left\">"),
                        Just("</p>"),
                        Just("<li>"),
                        Just("<td>"),
                        Just("<a href=\"x>y\" title='<z>'>"),
                        Just("</a>"),
                        Just("<!-- c<b> -->"),
                        Just("<script>"),
                        Just("<RECH_ECLI>"),
                        Just("<style type=x>"),
                        Just("<RECH_A/>"),
                    ]
                    .prop_map(|tag| (tag.to_owned(), String::new())),
                ],
                0..30,
            ),
            // Fin d'entrée : rien, ouvrant à attributs tronqué (retiré), ou ouvrant sans `=`
            // que rien ne ferme (texte gardé, règle bornée).
            end in prop_oneof![
                Just(("", "")),
                Just(("<a href=\"x", "")),
                Just(("<a title=Code d [...]", "")),
                Just((" si x <y alors", " si x <y alors")),
            ],
        ) {
            let (end_piece, end_kept) = end;
            let html: String =
                parts.iter().map(|(piece, _)| piece.as_str()).chain([end_piece]).collect();
            let expected: String =
                parts.iter().map(|(_, kept)| kept.as_str()).chain([end_kept]).collect();
            let mut texts = String::new();
            scan(&html, |event| texts.extend(text_slice(event)));
            prop_assert_eq!(texts, expected);
        }

        #[test]
        fn input_without_markup_is_one_text(text in "[^<]{1,80}") {
            prop_assert_eq!(events(&text), vec![Text(&text)]);
        }
    }
}
