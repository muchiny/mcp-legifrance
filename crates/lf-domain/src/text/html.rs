//! Lexer HTML partagé : champs courts (`clean`, 03 §7.3) et texte long découpé en
//! paragraphes (03 §7.2). Écrit à la main, sans crate HTML (00 §2 et §4) : tolérant, en deux
//! lectures de l'entrée, linéaire, en mémoire bornée, sans récursion ni panique. Aucun texte
//! n'est perdu quand une fermante manque, à une exception près : un ouvrant qui a vu un `=`
//! (forme d'attribut) et que rien ne ferme jusqu'à la fin de l'entrée est supprimé jusqu'à la
//! fin (capture tronquée dans un attribut, `consult_kali.json`).
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
//!   Un ouvrant n'ouvre une zone supprimée que si une fermante de son nom le suit (passe 1 de
//!   [`scan`]) ; la première fermante de ce nom la ferme, si bien que l'ouvrant le plus
//!   extérieur prend la première fermante de son nom. Sans fermante jusqu'à la fin, seul
//!   l'ouvrant est retiré : aucun texte perdu. Au-delà de 256 noms distincts de fermantes, les
//!   noms suivants sont traités comme sans fermante (contenu gardé). Un ouvrant auto-fermant
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
//! - La boucle de [`lex_all`] cherche le prochain `<` par `find`, appelle `lex` et avance d'au
//!   moins un octet (`len.max(1)`, sans opérateur mutable) : elle termine toujours. [`scan`]
//!   l'appelle deux fois (passes 1 et 2).
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
//! - Supprimés avec contenu : aucune recherche en avant ni événement retenu. La passe 1 note la
//!   fin de la dernière fermante de chaque nom (au plus 256 noms) ; la passe 2 y lit en temps
//!   constant si un ouvrant a une fermante après lui, et rend chaque événement au fil de la
//!   lecture (audit M-1 : le tampon d'événements de l'ancienne passe unique montait à 25 fois
//!   la taille de l'entrée).
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

/// Plus grand nombre de noms distincts d'éléments supprimés avec leur contenu dont la passe 1 de
/// [`scan`] retient la dernière fermante. Au-delà, un nom est traité comme sans fermante : son
/// contenu est gardé (aucun texte perdu), et la mémoire de `scan` reste bornée (audit M-1).
const MAX_DROP_NAMES: usize = 256;

/// Lit `html` et rend ses événements à `sink`, dans l'ordre, au fil de la lecture (grammaire et
/// classement : doc du module).
///
/// Deux passes du lexer (audit M-1) : la passe 1 note, pour chaque nom supprimé avec son
/// contenu (`RECH_*`, `script`, `style`), la fin de sa dernière fermante ([`drop_closings`]) ;
/// la passe 2 rend les événements, avec un seul état : le nom de l'élément en cours de
/// suppression. Un ouvrant n'ouvre une zone supprimée que si une fermante de son nom le suit ;
/// la zone se ferme à la première fermante de ce nom, et tout ce qu'elle contient part avec
/// elle. Un ouvrant sans fermante ne change rien : seule la balise est retirée.
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
/// - Au-delà de [`MAX_DROP_NAMES`] noms distincts de fermantes, dans l'ordre de la source, les
///   noms suivants sont traités comme sans fermante : leur contenu est gardé.
///
/// Mémoire : aucun événement n'est retenu ; en plus du nom de chaque balise (une à la fois),
/// `scan` ne garde que la table de la passe 1, d'au plus [`MAX_DROP_NAMES`] noms, et le nom de
/// l'élément en cours de suppression. Le tampon d'événements précédent montait à 25 fois la
/// taille de l'entrée (`<script>` suivi de `<` répétés, audit M-1).
pub(crate) fn scan<'a>(html: &'a str, mut sink: impl FnMut(Event<'a>)) {
    scan_into(html, false, &mut sink);
}

/// Variante de [`scan`] pour le texte long (L4-01) : les cellules d'un tableau portent leurs
/// blocs. [`Event::Cell`] n'est rendu qu'à l'ouvrante `td` ou `th` (elle ferme la cellule
/// précédente et en ouvre une) ; `</td>` rend une espace. Tant qu'une cellule est ouverte, une
/// coupure de bloc `br`, `p`, `div`, `li`, `ul`, `ol` ou `hN` (fermantes comprises) n'est pas
/// rendue comme une coupure mais comme l'espace [`Event::Text`] `" "` : le contenu reste dans
/// la cellule. `tr` et `table` (ouvrantes ou fermantes) restent des coupures, et ferment la
/// cellule. Hors cellule, tout est identique à [`scan`].
///
/// Limite connue (revue W4) : une cellule jamais fermée, sans `tr` ni `table` derrière, reste
/// ouverte jusqu'à la fin de l'entrée, et tous les blocs qui suivent valent des espaces (un seul
/// paragraphe, pagination dégradée, coût linéaire). `<td/>` ouvre une cellule comme `<td>`.
pub(crate) fn scan_rows<'a>(html: &'a str, mut sink: impl FnMut(Event<'a>)) {
    scan_into(html, true, &mut sink);
}

/// Corps de [`scan`] et de [`scan_rows`] (`rows`), non générique : `cargo-llvm-cov` résume une
/// fonction générique par son instance la mieux couverte, pas par l'union des instances
/// (CONTRIBUTING.md).
fn scan_into<'a>(html: &'a str, rows: bool, sink: &mut dyn FnMut(Event<'a>)) {
    let closings = drop_closings(html);
    // Nom de l'élément en cours de suppression : tout part jusqu'à sa première fermante.
    let mut discard: Option<String> = None;
    // Une cellule `td` ou `th` est ouverte (`rows` seulement) : seul état ajouté, un booléen.
    let mut in_cell = false;
    lex_all(html, &mut |piece| match piece {
        Piece::Text(text) => {
            if discard.is_none() {
                sink(Event::Text(text));
            }
        }
        Piece::Tag(at, tag) => {
            if discard.is_some() {
                if tag.closing && discard.as_deref() == Some(tag.name.as_str()) {
                    discard = None;
                }
                return;
            }
            let class = classify(&tag.name);
            if rows {
                match class {
                    Class::Cell => {
                        in_cell = !tag.closing;
                        // Ouvrante : nouvelle cellule. Fermante : une espace, le texte qui la
                        // suit n'est pas collé à celui de la cellule.
                        sink(if in_cell { Event::Cell } else { Event::Text(" ") });
                        return;
                    }
                    Class::Break(Block::Other) if matches!(tag.name.as_str(), "tr" | "table") => {
                        in_cell = false;
                    }
                    Class::Break(_) if in_cell => {
                        sink(Event::Text(" "));
                        return;
                    }
                    _ => {}
                }
            }
            match (class, tag.closing) {
                (Class::Drop, false) => {
                    // Fermante de ce nom après l'ouvrant : `end > at` (une fermante qui le
                    // précède, même accolée, finit au plus à `at`).
                    if !tag.self_closing && closings.get(&tag.name).is_some_and(|&end| end > at) {
                        discard = Some(tag.name);
                    }
                }
                (Class::Drop, true) | (Class::Inline, _) => {}
                (Class::Break(Block::Br), true) => sink(Event::Break(Block::Other)),
                (Class::Break(block), _) => sink(Event::Break(block)),
                (Class::Cell, _) => sink(Event::Cell),
            }
        }
    });
}

/// Passe 1 de [`scan`] : pour chaque nom supprimé avec son contenu, la position (en octets)
/// qui suit sa dernière fermante. Au plus [`MAX_DROP_NAMES`] noms, les premiers rencontrés ; la
/// fermante d'un nom déjà retenu met toujours sa position à jour.
fn drop_closings(html: &str) -> HashMap<String, usize> {
    let mut closings: HashMap<String, usize> = HashMap::new();
    lex_all(html, &mut |piece| {
        if let Piece::Tag(at, tag) = piece
            && tag.closing
            && classify(&tag.name) == Class::Drop
            && (closings.len() < MAX_DROP_NAMES || closings.contains_key(&tag.name))
        {
            closings.insert(tag.name, at.saturating_add(tag.len));
        }
    });
    closings
}

/// Morceau de la source lu par [`lex_all`].
enum Piece<'a> {
    /// Texte non vide, entités non décodées.
    Text(&'a str),
    /// Balise reconnue, et position (en octets) de son `<` dans l'entrée.
    Tag(usize, Tag),
}

/// Lit `html` par [`lex`] et rend chaque morceau à `visit`, dans l'ordre de la source : le
/// texte entre les balises, le texte d'un `<` littéral, chaque balise ; un commentaire et un
/// ouvrant tronqué en fin d'entrée ne rendent rien.
fn lex_all<'a>(html: &'a str, visit: &mut dyn FnMut(Piece<'a>)) {
    let mut no_comment_end = false;
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        let (text, tail) = rest.split_at(lt);
        visit_text(text, visit);
        let len = match lex(tail, &mut no_comment_end) {
            Lexeme::Literal(len) => {
                visit_text(&tail[..len], visit);
                len
            }
            Lexeme::Skip(len) => len,
            Lexeme::Tag(tag) => {
                let len = tag.len;
                visit(Piece::Tag(html.len().saturating_sub(tail.len()), tag));
                len
            }
        };
        // Au moins un octet consommé par tour, même si `lex` rendait 0 : la boucle termine.
        rest = &tail[len.max(1)..];
    }
    visit_text(rest, visit);
}

/// Rend `text` à `visit` s'il n'est pas vide.
fn visit_text<'a>(text: &'a str, visit: &mut dyn FnMut(Piece<'a>)) {
    if !text.is_empty() {
        visit(Piece::Text(text));
    }
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
                return Lexeme::Skip("<!--".len().saturating_add(end).saturating_add("-->".len()));
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
                let tag =
                    Tag { name, closing, self_closing: last == b'/', len: i.saturating_add(1) };
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
        None => ENTITIES
            .binary_search_by(|&(known, _)| known.cmp(name))
            .ok()
            .and_then(|i| ENTITIES.get(i))
            .map(|entry| entry.1)?,
    };
    Some((ch, end.saturating_add(1)))
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
/// Jeu retiré : les contrôles C0 et C1 (et U+007F) sauf `\t`, `\n` et `\r`, et tout point de
/// code `Default_Ignorable_Code_Point` (17 plages) : U+00AD, U+034F, U+061C, U+115F et U+1160,
/// U+17B4 et U+17B5, U+180B à U+180F, U+200B à U+200F, les contrôles bidirectionnels U+202A à
/// U+202E et U+2066 à U+2069, U+2060 à U+206F, U+3164, les sélecteurs de variante U+FE00 à
/// U+FE0F et U+E0100 à U+E01EF, U+FEFF, U+FFA0, U+FFF0 à U+FFF8, U+1BCA0 à U+1BCA3, U+1D173 à
/// U+1D17A, les tags U+E0000 à U+E0FFF. Un texte officiel français n'en a aucun usage
/// légitime, et ils peuvent masquer, réordonner ou porter en contrebande le texte montré
/// (MEN-06).
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

/// Caractère du jeu de [`strip_invisible`] : contrôle autre que `\t`, `\n` et `\r`, ou point de
/// code « ignorable par défaut ».
pub(crate) fn is_invisible(c: char) -> bool {
    let control = c.is_control() && !matches!(c, '\t' | '\n' | '\r');
    control || is_default_ignorable(c)
}

/// Point de code de la propriété Unicode `Default_Ignorable_Code_Point` (audit L-1) : rendu
/// invisible, il peut porter une charge que le lecteur ne voit pas (les sélecteurs de variante
/// U+FE00 à U+FE0F et U+E0100 à U+E01EF codent un octet chacun). Les 17 plages sont recopiées
/// de `is_invisible` de `crates/lf-adapter-piste/src/error_body.rs` (le domaine ne dépend pas
/// de l'adapter) : U+00AD, U+061C, U+200B à U+200F (U+200C et U+200D compris), les bidi
/// U+202A à U+202E et U+2066 à U+2069, U+2060 et U+FEFF y figurent déjà.
fn is_default_ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// Séparateur de deux cellules d'une ligne de tableau dans le texte long (L4-01, Q4) : ajouté
/// par le serveur, il ne vient pas de la source.
const CELL_SEPARATOR: &str = " | ";

/// Découpe `html` en lignes (03 §7.2, étape 4) et rend le texte visible de chacune à `visible`,
/// au fil de la lecture ; rend le nombre de caractères invisibles retirés.
///
/// Une ligne est le texte compris entre deux coupures de bloc ([`Event::Break`], de n'importe
/// quelle sorte : `<BR></BR>` coupe deux fois, ce qui est sans effet puisque l'appelant ignore
/// les segments blancs). Elle est lue par [`scan_rows`] : une ligne de tableau (`tr`) est une
/// seule ligne, dont les cellules ([`Event::Cell`]) sont séparées par [`CELL_SEPARATOR`] ; les
/// blocs d'une cellule (`<td><p>…</p></td>`) y valent une espace. Une cellule vide (blancs,
/// balises vides, invisibles seuls) entre deux cellules garde son séparateur, pour que les
/// colonnes ne se perdent pas (`Mode | Initial | | Différé`) ; les cellules vides en tête et en
/// fin de ligne s'effacent, et la ligne ne commence ni ne finit par un séparateur. Chaque
/// cellule passe par [`decode_entities`], puis [`strip_invisible`]. Les blancs ne sont pas
/// repliés ici : `LongText` les replie directement dans son texte canonique, sans tableau
/// intermédiaire de paragraphes ni de mots (audit M-2). Sans balise de bloc, il n'y a qu'une
/// ligne.
///
/// Le nombre rendu est la somme, sur toutes les cellules, des caractères invisibles retirés (y
/// compris ceux des cellules qui n'ont rien d'autre et disparaissent).
pub(crate) fn paragraphs(html: &str, visible: &mut dyn FnMut(&str)) -> u32 {
    let mut invisible = 0_u32;
    let mut segment = String::new();
    let mut row = Row::default();
    scan_rows(html, |event| match event {
        Event::Text(text) => segment.push_str(text),
        Event::Cell => invisible = invisible.saturating_add(row.push_cell(&mut segment)),
        Event::Break(_) => {
            invisible = invisible.saturating_add(row.push_cell(&mut segment));
            row.finish(visible);
        }
    });
    invisible = invisible.saturating_add(row.push_cell(&mut segment));
    row.finish(visible);
    invisible
}

/// Ligne en cours de [`paragraphs`] : texte visible des cellules déjà reçues.
#[derive(Default)]
struct Row {
    /// Texte de la ligne, séparateurs compris.
    text: String,
    /// Une cellule non vide a déjà été ajoutée.
    has_text: bool,
    /// Cellules vides vues depuis la dernière cellule non vide : leur séparateur n'est écrit que
    /// si une cellule non vide les suit (jamais en fin de ligne).
    pending: usize,
}

impl Row {
    /// Termine la cellule `segment` (vidé) : décodée, sans invisibles. Rend le nombre
    /// d'invisibles retirés.
    fn push_cell(&mut self, segment: &mut String) -> u32 {
        let decoded = decode_entities(segment);
        segment.clear();
        let (text, removed) = strip_invisible(&decoded);
        if text.split_whitespace().next().is_some() {
            if self.has_text {
                // Un séparateur par cellule vide d'écart, plus celui de la cellule elle-même.
                for _ in 0..=self.pending {
                    self.text.push_str(CELL_SEPARATOR);
                }
            }
            self.text.push_str(&text);
            self.has_text = true;
            self.pending = 0;
        } else if self.has_text {
            self.pending = self.pending.saturating_add(1);
        } else {
            // Blancs de tête : gardés tels quels, c'est le texte d'une ligne sans cellule.
            self.text.push_str(&text);
        }
        removed
    }

    /// Rend la ligne à `visible` et repart d'une ligne vide.
    fn finish(&mut self, visible: &mut dyn FnMut(&str)) {
        visible(&self.text);
        *self = Self::default();
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
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
        // sans `=` : son `<` est littéral (règle bornée), le texte est gardé.
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
    fn closing_right_before_an_opener_does_not_close_it() {
        // La fermante d'un nom, juste avant un ouvrant du même nom, ne le ferme pas : l'ouvrant
        // reste sans fermante et son contenu est gardé.
        assert_eq!(rendered("texte</script><script>suite"), "textesuite");
        assert_eq!(rendered("</RECH_A><RECH_A>x<p>y"), "x[p]y");
        assert_eq!(rendered("abc</style><style>d</style>e"), "abce");
    }

    #[test]
    fn drop_names_are_capped_and_their_content_is_then_kept() {
        // Passe 1 (audit M-1) : au plus 256 noms de fermantes retenus ; au-delà, un élément est
        // traité comme sans fermante, son contenu est GARDÉ (jamais perdu).
        let mut html = String::new();
        for i in 0..256 {
            write!(html, "<RECH_{i}>x{i}</RECH_{i}>").unwrap();
        }
        html.push_str("<RECH_PLUS>gardé</RECH_PLUS>");
        // Un nom déjà retenu suit toujours sa dernière fermante, après le plafond.
        html.push_str("<RECH_0>parti</RECH_0>fin");
        assert_eq!(rendered(&html), "gardéfin");
    }

    #[test]
    fn scan_memory_does_not_grow_with_the_input() {
        // Audit M-1 : aucun événement n'est retenu ; hors lexer, la seule mémoire de `scan` qui
        // dépend de l'entrée est la table de la passe 1, plafonnée à MAX_DROP_NAMES noms.
        let closings = |n: usize| {
            (0..n).fold(String::new(), |mut html, i| {
                write!(html, "</rech_{i}>").unwrap();
                html
            })
        };
        let small = drop_closings(&closings(MAX_DROP_NAMES + 1));
        let large = drop_closings(&closings(100_000));
        assert_eq!((small.len(), large.len()), (MAX_DROP_NAMES, MAX_DROP_NAMES));
        assert_eq!(small.capacity(), large.capacity());
        // Un nom répété : une seule entrée, la fin de sa dernière fermante.
        let repeated = format!("a{}", "</SCRIPT>".repeat(10_000));
        assert_eq!(
            drop_closings(&repeated),
            HashMap::from([("script".to_owned(), repeated.len())])
        );
        // Sans fermante, la table reste vide quelle que soit la taille de l'entrée.
        for size in [1, MIB] {
            assert_eq!(drop_closings(&format!("<script>{}", "<".repeat(size))).capacity(), 0);
        }
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
            '\u{fefe}',
            '\u{dffff}',
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

    /// Les 17 plages de la propriété Unicode `Default_Ignorable_Code_Point`, (premier, dernier),
    /// recopiées de `crates/lf-adapter-piste/src/error_body.rs` (tests de `is_invisible`).
    const DEFAULT_IGNORABLE: [(char, char); 17] = [
        ('\u{00AD}', '\u{00AD}'),
        ('\u{034F}', '\u{034F}'),
        ('\u{061C}', '\u{061C}'),
        ('\u{115F}', '\u{1160}'),
        ('\u{17B4}', '\u{17B5}'),
        ('\u{180B}', '\u{180F}'),
        ('\u{200B}', '\u{200F}'),
        ('\u{202A}', '\u{202E}'),
        ('\u{2060}', '\u{206F}'),
        ('\u{3164}', '\u{3164}'),
        ('\u{FE00}', '\u{FE0F}'),
        ('\u{FEFF}', '\u{FEFF}'),
        ('\u{FFA0}', '\u{FFA0}'),
        ('\u{FFF0}', '\u{FFF8}'),
        ('\u{1BCA0}', '\u{1BCA3}'),
        ('\u{1D173}', '\u{1D17A}'),
        ('\u{E0000}', '\u{E0FFF}'),
    ];

    /// Caractère de code `code`.
    fn ch(code: u32) -> char {
        char::from_u32(code).unwrap()
    }

    #[test]
    fn strip_invisible_removes_every_default_ignorable_range() {
        // Audit L-1 : premier, milieu et dernier point de chaque plage, retirés et comptés une
        // fois (U+200C et U+200D, déjà du jeu, ne comptent pas double) ; le voisin de chaque
        // bord est gardé.
        for (first, last) in DEFAULT_IGNORABLE {
            let middle = ch(u32::midpoint(u32::from(first), u32::from(last)));
            for c in [first, middle, last] {
                assert_eq!(strip_invisible(&format!("a{c}b")), ("ab".to_owned(), 1), "{c:?}");
            }
            for c in [ch(u32::from(first) - 1), ch(u32::from(last) + 1)] {
                assert_eq!(strip_invisible(&format!("a{c}b")), (format!("a{c}b"), 0), "{c:?}");
            }
        }
        // Points relevés par l'audit (sélecteurs de variante, opérateurs invisibles, remplisseurs).
        for c in ['\u{180e}', '\u{2061}', '\u{2064}', '\u{3164}', '\u{fe0f}', '\u{e0101}'] {
            assert_eq!(strip_invisible(&format!("a{c}b")), ("ab".to_owned(), 1), "{c:?}");
        }
        assert_eq!(strip_invisible("\u{200c}\u{200d}"), (String::new(), 2));
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
        // Ouvrant supprimable sans fermante, puis des blocs : rien n'est retenu, tout est rendu.
        finishes_within_budget(format!("<script>{}", "a<br>".repeat(MIB / 5)), scan_all);
        // Zone supprimée qui court jusqu'à une fermante finale.
        finishes_within_budget(format!("<script>{}</script>", "a<br>".repeat(MIB / 5)), scan_all);
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

        /// Les deux passes de `scan` contre un oracle naïf (quadratique) écrit d'après la règle :
        /// à chaque ouvrant supprimable, chercher en avant la première fermante de son nom.
        #[test]
        fn scan_matches_the_forward_search_oracle(parts in proptest::collection::vec(
            prop_oneof![
                Just("<script>"), Just("</script>"), Just("<style t='</style>'>"),
                Just("</style >"), Just("<RECH_A>"), Just("</RECH_A>"), Just("<rech_b x=1>"),
                Just("</rech_b>"), Just("<RECH_A/>"), Just("</RECH_A/>"), Just("<!--"),
                Just("-->"), Just("x=\""), Just("\""), Just("'"), Just("="), Just("<"),
                Just(">"), Just("a"), Just("é"), Just("<p>"), Just("</br>"), Just("<td>"),
                Just("<b>"), Just("<i "),
            ],
            0..64,
        )) {
            let html = parts.concat();
            prop_assert_eq!(events(&html), forward_search(&html));
        }

        /// Familles hostiles de l'audit M-1 : sortie exacte, sans perte ni rétention.
        #[test]
        fn hostile_families_keep_their_exact_output(n in 0_usize..2_000) {
            // `<script>` suivi de `<` répétés : chaque `<` est un texte d'un octet, rendu.
            let lts = format!("<script>{}", "<".repeat(n));
            prop_assert_eq!(events(&lts), vec![Text("<"); n]);
            // `a<p>` répétés après un ouvrant supprimable sans fermante.
            prop_assert_eq!(rendered(&format!("<RECH_A>{}", "a<p>".repeat(n))), "a[p]".repeat(n));
            // Fermante manquante : le texte reste ; présente, il part avec l'élément.
            let words = "x ".repeat(n);
            prop_assert_eq!(rendered(&format!("<style>{words}")), words.clone());
            prop_assert_eq!(rendered(&format!("<style>{words}</style>y")), "y");
        }

        /// N noms `RECH_` distincts : les MAX_DROP_NAMES premiers partent avec leur contenu, les
        /// suivants le gardent.
        #[test]
        fn drop_names_past_the_cap_keep_their_content(n in 0_usize..600) {
            let html = (0..n).fold(String::new(), |mut html, i| {
                write!(html, "<RECH_{i}>x{i}</RECH_{i}>").unwrap();
                html
            });
            let kept = (MAX_DROP_NAMES..n).fold(String::new(), |mut kept, i| {
                write!(kept, "x{i}").unwrap();
                kept
            });
            prop_assert_eq!(rendered(&html), kept);
        }
    }

    /// Oracle de `scan` : les morceaux du lexer, puis, à chaque ouvrant supprimable non
    /// auto-fermant, recherche en avant de la première fermante de son nom ; trouvée, tout part
    /// jusqu'à elle, sinon seul l'ouvrant part. Sans plafond de noms (entrées de peu de noms).
    fn forward_search(html: &str) -> Vec<Event<'_>> {
        let mut pieces = Vec::new();
        lex_all(html, &mut |piece| pieces.push(piece));
        let closes = |piece: &Piece<'_>, name: &str| match piece {
            Piece::Tag(_, tag) => tag.closing && tag.name == name,
            Piece::Text(_) => false,
        };
        let mut out = Vec::new();
        let mut i = 0;
        while i < pieces.len() {
            match &pieces[i] {
                Piece::Text(text) => out.push(Text(text)),
                Piece::Tag(_, tag) => match (classify(&tag.name), tag.closing) {
                    (Class::Drop, false) => {
                        let close = pieces[i + 1..].iter().position(|p| closes(p, &tag.name));
                        if let Some(skipped) = close.filter(|_| !tag.self_closing) {
                            i += skipped + 1;
                        }
                    }
                    (Class::Drop, true) | (Class::Inline, _) => {}
                    (Class::Break(Br), true) => out.push(Break(Other)),
                    (Class::Break(block), _) => out.push(Break(block)),
                    (Class::Cell, _) => out.push(Cell),
                },
            }
            i += 1;
        }
        out
    }

    // ---- paragraphs (03 §7.2, étape 4) ----

    #[test]
    fn paragraphs_hands_over_each_visible_segment() {
        // Segments rendus tels quels (décodés, sans invisibles, blancs non repliés), blancs et
        // vides compris : le repli et l'écart des segments blancs reviennent à `LongText`.
        let mut segments = Vec::new();
        let invisible = paragraphs(" a&amp;\u{200b} b <p><BR></BR>\t", &mut |visible| {
            segments.push(visible.to_owned());
        });
        assert_eq!(segments, [" a& b ", "", "", "\t"]);
        assert_eq!(invisible, 1);
    }

    /// Paragraphes de `html` tels que `LongText` les forme (segments visibles, blancs repliés,
    /// segments blancs écartés), et compteur d'invisibles.
    fn split(html: &str) -> (Vec<String>, u32) {
        let mut out = Vec::new();
        let invisible = paragraphs(html, &mut |visible| {
            out.extend(Some(fold_ws(visible)).filter(|folded| !folded.is_empty()));
        });
        (out, invisible)
    }

    /// Paragraphes seuls, sans le compteur d'invisibles.
    fn texts(html: &str) -> Vec<String> {
        split(html).0
    }

    /// Suite de blancs repliée en une espace, bords rognés.
    fn fold_ws(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Champ texte d'une fixture, par son chemin dans le JSON. Une fixture connue en mojibake
    /// (`known_issues` de son `.meta.toml`) est réparée par l'appelant, sur le texte entier,
    /// avant d'arriver ici (03 §7.1) : aucune des fixtures de ce module n'en a.
    fn text_field(fixture_json: &str, path: &[&str]) -> String {
        let mut value: serde_json::Value = serde_json::from_str(fixture_json).unwrap();
        for key in path {
            value = value[key].take();
        }
        value.as_str().unwrap().to_owned()
    }

    const CONSTIT: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/reference/consult_juri_constit.json"
    );
    const CETAT: &str =
        include_str!("../../../lf-adapter-piste/tests/fixtures/reference/consult_juri_cetat.json");
    const CNIL: &str =
        include_str!("../../../lf-adapter-piste/tests/fixtures/reference/consult_cnil.json");

    const CGI_777: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/getArticle/c101/C11-getArticle-cgi-777-LEGIARTI000030061736.json"
    );
    const CGI_669: &str = include_str!(
        "../../../lf-adapter-piste/tests/fixtures/recorded/consult/getArticle/c101/C11-getArticle-cgi-669-LEGIARTI000006310173.json"
    );

    /// `texteHtml` d'une fixture.
    fn text_html(fixture_json: &str) -> String {
        text_field(fixture_json, &["text", "texteHtml"])
    }

    /// Somme des longueurs en caractères des paragraphes.
    fn chars_sum(paragraphs: &[String]) -> usize {
        paragraphs.iter().map(|p| p.chars().count()).sum()
    }

    #[test]
    fn from_html_block_tags_split_paragraphs() {
        assert_eq!(texts("<p>a</p><p>b</p>"), ["a", "b"]);
        assert_eq!(texts("a<br/>b"), ["a", "b"]);
        assert_eq!(texts("a<BR>b"), ["a", "b"]);
        assert_eq!(texts("<div>a</div><li>b<h1>c"), ["a", "b", "c"]);
    }

    #[test]
    fn from_html_closing_br_is_a_block_too() {
        // `<BR></BR>` coupe deux fois : sans effet, le segment vide entre les deux est supprimé.
        assert_eq!(texts("a<BR></BR>b"), ["a", "b"]);
        assert_eq!(texts("a</br>b"), ["a", "b"]);
    }

    #[test]
    fn from_html_table_cells_are_separated_by_a_bar() {
        // Une ligne de tableau est un paragraphe ; les cellules y sont séparées par ` | ` (L4-01).
        assert_eq!(texts("<tr><td>a</td><td>b</td></tr>"), ["a | b"]);
        assert_eq!(texts("a<th>b"), ["a | b"]);
        assert_eq!(
            texts("<tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr>"),
            ["a | b", "c | d"]
        );
        // Une cellule seule n'a pas de séparateur.
        assert_eq!(texts("<tr><td>a</td></tr>"), ["a"]);
    }

    #[test]
    fn from_html_cells_with_paragraphs_stay_on_the_row() {
        // Forme de Légifrance (art. 777 CGI) : `<td><p>…</p></td>` et `<td><br/>10 <br/></td>`.
        let table = "<table><tbody>\
            <tr><td><p align=\"left\">Comprise entre 8 072 € et 12 109 € </p></td>\
            <td align=\"center\"><br/>10 <br/></td></tr>\
            <tr><td><p>Comprise entre 12 109 € et 15 932 € </p></td><td><br/>15 <br/></td></tr>\
            </tbody></table>";
        assert_eq!(
            texts(table),
            ["Comprise entre 8 072 € et 12 109 € | 10", "Comprise entre 12 109 € et 15 932 € | 15"]
        );
        // Vrai positif conservé : hors cellule, `<p>` et `<br>` coupent toujours.
        assert_eq!(texts("<p>a</p><td>b</td><p>c</p>"), ["a", "b", "c"]);
        assert_eq!(texts("a<br/>b<table><tr><td>c</td></tr></table>d"), ["a", "b", "c", "d"]);
    }

    #[test]
    fn from_html_breaks_inside_a_cell_are_a_space_not_a_column() {
        // `<br>`, `<p>`, `<li>`… dans une cellule : le contenu reste dans la même colonne.
        assert_eq!(
            texts("<tr><td><p>TARIF </p><p>applicable (%) </p></td><td>x</td></tr>"),
            ["TARIF applicable (%) | x"]
        );
        assert_eq!(
            texts(
                "<tr><td>Paiement <br/>(payable)</td><td><ul><li>un</li><li>deux</li></ul></td></tr>"
            ),
            ["Paiement (payable) | un deux"]
        );
        assert_eq!(
            texts("<tr><td>a</br>b<div>c</div><h2>d</h2><ol><li>e</td><td>f"),
            ["a b c d e | f"]
        );
    }

    #[test]
    fn from_html_a_row_or_a_table_ends_the_cell() {
        // `tr`, `table` et leurs fermantes coupent la ligne, même dans une cellule ouverte ;
        // après elles, un bloc coupe de nouveau.
        assert_eq!(texts("<td><p>a</p></td></tr><p>b</p><p>c</p>"), ["a", "b", "c"]);
        assert_eq!(texts("<td>a<tr><td>b</td></tr>"), ["a", "b"]);
        assert_eq!(
            texts("<table><tr><td>a<table><tr><td>b</table>c<p>d</p>"),
            ["a", "b", "c", "d"]
        );
        assert_eq!(texts("<td>a</td></table><p>b</p>"), ["a", "b"]);
    }

    #[test]
    fn from_html_an_unclosed_cell_ends_at_the_next_cell() {
        assert_eq!(texts("<tr><td>a<td>b<td>c"), ["a | b | c"]);
        assert_eq!(texts("<tr><th>a<th><p>b</p><tr><td>c<td>d"), ["a | b", "c | d"]);
    }

    /// Revue W4 (L4-01) : comportement connu, voulu. Une cellule jamais fermée, sans `tr` ni
    /// `table` fermant derrière, reste ouverte jusqu'à la fin : les blocs qui suivent valent des
    /// espaces et tiennent en un seul paragraphe. Le coût reste linéaire ; seule la pagination
    /// s'en ressent. Une `<td/>` est une ouvrante comme une autre. Un `tr` ou un `table`, ouvrant
    /// ou fermant, rend les paragraphes.
    #[test]
    fn from_html_an_orphan_cell_swallows_the_blocks_that_follow() {
        assert_eq!(texts("<td>a<p>b</p><p>c</p>d"), ["a b c d"]);
        assert_eq!(texts("<td/>a<p>b</p><p>c</p>"), ["a b c"]);
        assert_eq!(texts("x<p>y</p><td>a<h2>b</h2><p>c</p>"), ["x", "y", "a b c"]);
        assert_eq!(texts("<td>a<p>b</p></table><p>c</p><p>d</p>"), ["a b", "c", "d"]);
        assert_eq!(texts("<td>a<p>b</p><tr><p>c</p>"), ["a b", "c"]);
    }

    #[test]
    fn from_html_an_empty_cell_inside_a_row_keeps_its_bar() {
        // Fixture KALITEXT000049878950 : `<th></th>` au milieu d'une ligne.
        assert_eq!(
            texts("<tr><th>Mode </th><th>Initial </th><th></th><th>Différé </th></tr>"),
            ["Mode | Initial | | Différé"]
        );
        assert_eq!(texts("<tr><td>a</td><td></td><td></td><td>d</td></tr>"), ["a | | | d"]);
        // Une cellule qui n'a que des blancs, des balises vides ou des invisibles est vide.
        assert_eq!(
            texts("<tr><td>a</td><td> <p></p><br/>&nbsp;&#8203;</td><td>c</td></tr>"),
            ["a | | c"]
        );
        // Jamais de séparateur en tête ni en fin de ligne.
        assert_eq!(texts("<tr><td></td><td>a</td><td>b</td><td></td><td></td></tr>"), ["a | b"]);
        assert_eq!(
            texts("<tr><td><p align=\"center\"></p></td><td></td></tr>"),
            Vec::<String>::new()
        );
        assert_eq!(
            texts("<tr><td>Moins de :</td><td><p></p></td><td><p></p></td></tr>"),
            ["Moins de :"]
        );
        // Les invisibles des cellules vides sont comptés quand même.
        assert_eq!(
            split("<tr><td>a</td><td>\u{200b}</td><td>b</td></tr>"),
            (vec!["a | | b".to_owned()], 1)
        );
    }

    #[test]
    fn from_html_text_around_cells_is_kept() {
        // Du texte avant la première cellule ou entre `</td>` et la suivante n'est pas perdu.
        assert_eq!(
            texts("<tr>avant<td>a</td>entre<td>b</td>après</tr>"),
            ["avant | a entre | b après"]
        );
    }

    #[test]
    fn clean_keeps_a_cell_as_a_plain_space() {
        use crate::text::clean::clean;
        // Les champs courts ne changent pas : ni séparateur, ni cellule de paragraphe.
        assert_eq!(clean("<tr><td>a</td><td>b</td></tr>"), "a b");
        assert_eq!(clean("<td><p>a</p></td><td>b</td>"), "a\n\nb");
    }

    /// `texteHtml` de l'article d'une fixture C11 (relevé de `getArticle`).
    fn article_html(fixture_json: &str) -> String {
        text_field(fixture_json, &["article", "texteHtml"])
    }

    #[test]
    fn article_777_cgi_rows_are_one_paragraph_each() {
        // Relevé C11 (art. 777 CGI) : trois tableaux dont une cellule d'en-tête à deux `<p>`,
        // des cellules `<br/>10 <br/>` et, au tableau III, des lignes à cellule de droite vide.
        let rows = texts(&article_html(CGI_777));
        assert_eq!(rows[1], "Tableau I");
        assert_eq!(rows[3], "FRACTION DE PART NETTE TAXABLE | TARIF applicable (%)");
        assert_eq!(rows[4], "N'excédant pas 8 072 € | 5");
        assert_eq!(rows[5], "Comprise entre 8 072 € et 12 109 € | 10");
        assert_eq!(rows[10], "Au-delà de 1 805 677 € | 45");
        assert_eq!(rows[11], "Tableau II");
        // Une ligne de titre de sous-tableau, cellule de tarif vide : pas de barre finale.
        assert!(rows.contains(&"Entre frères et sœurs vivants ou représentés :".to_owned()));
        assert!(rows.contains(&"Supérieure à 24 430 € | 45".to_owned()));
        // Aucune ligne ne commence ni ne finit par une barre, aucune cellule n'est seule.
        assert!(rows.iter().all(|row| !row.starts_with('|') && !row.ends_with('|')));
        assert!(!rows.contains(&"5".to_owned()) && !rows.contains(&"10".to_owned()));
    }

    #[test]
    fn article_669_cgi_rows_are_one_paragraph_each() {
        let rows = texts(&article_html(CGI_669));
        assert_eq!(
            rows[1],
            "AGE de l'usufruitier | VALEUR de l'usufruit | VALEUR de la nue-propriété"
        );
        // Cellules de droite vides : pas de barre finale.
        assert_eq!(rows[2], "Moins de :");
        assert_eq!(rows[3], "21 ans révolus | 90 % | 10 %");
        assert_eq!(rows[11], "Plus de 91 ans révolus | 10 % | 90 %");
        assert!(rows[12].starts_with("Pour déterminer la valeur de la nue-propriété"));
        assert_eq!(rows.len(), 14);
    }

    #[test]
    fn from_html_inline_tags_vanish_without_space() {
        assert_eq!(texts("1<sup>er</sup> <mark>mai</mark>"), ["1er mai"]);
    }

    #[test]
    fn from_html_drops_rech_and_script_with_content() {
        assert_eq!(
            texts("a<RECH_ECLI>ECLI:FR:X</RECH_ECLI><p>b<script>x()</script>c"),
            ["a", "bc"]
        );
    }

    #[test]
    fn from_html_unclosed_tag_at_end() {
        assert_eq!(texts("a<p>b <a href=\"x"), ["a", "b"]);
        assert_eq!(texts("a<p>b <i"), ["a", "b <i"]);
        assert_eq!(texts("a<p>b<"), ["a", "b<"]);
    }

    #[test]
    fn from_html_literal_less_than() {
        assert_eq!(texts("a < b"), ["a < b"]);
    }

    #[test]
    fn from_html_decodes_entities_after_tags() {
        assert_eq!(texts("A &amp; B <p>&lt;b&gt; &eacute;t&#233;"), ["A & B", "<b> été"]);
    }

    #[test]
    fn from_html_invalid_numeric_entity_kept() {
        assert_eq!(
            texts("a &#xZZ; &#1114112; &#; &unknown;"),
            ["a &#xZZ; &#1114112; &#; &unknown;"]
        );
    }

    #[test]
    fn from_html_no_block_tag_gives_one_paragraph() {
        assert_eq!(texts("un seul <b>bloc</b> de texte"), ["un seul bloc de texte"]);
    }

    #[test]
    fn from_html_empty_paragraphs_are_dropped() {
        assert_eq!(texts("<br/><br/>"), Vec::<String>::new());
        assert_eq!(texts(" <p> \n\t </p>&nbsp;<p>&#32;</p>"), Vec::<String>::new());
        assert_eq!(texts("<p>a</p><p></p><p>b</p>"), ["a", "b"]);
    }

    #[test]
    fn from_html_empty_input_gives_no_paragraph() {
        assert_eq!(split(""), (Vec::new(), 0));
        assert_eq!(split("<p></p><script>x</script>"), (Vec::new(), 0));
    }

    #[test]
    fn from_html_folds_inner_newlines() {
        assert_eq!(texts("a\n   b"), ["a b"]);
        assert_eq!(texts("  a\r\n\t b\u{a0}c&#10;d  <p>\n e \n"), ["a b c d", "e"]);
    }

    #[test]
    fn from_html_counts_invisible_chars_over_all_paragraphs() {
        // Brut ; brut puis décodé d'une entité ; VT (devient une espace, compté) : 1 + 2 + 1 + 2.
        let html = "a\u{200b}b<p>\u{202e}c&#173;d<p>e\u{b}f<p>\u{200b}\u{200b}";
        assert_eq!(split(html), (vec!["ab".to_owned(), "cd".into(), "e f".into()], 6));
    }

    #[test]
    fn from_html_without_invisible_chars_counts_zero() {
        assert_eq!(split("a<p>b"), (vec!["a".to_owned(), "b".into()], 0));
    }

    #[test]
    fn constit_fixture_yields_49_paragraphs() {
        assert_eq!(texts(&text_html(CONSTIT)).len(), 49);
    }

    #[test]
    fn cetat_fixture_yields_multiple_paragraphs() {
        assert_eq!(texts(&text_html(CETAT)).len(), 43);
    }

    #[test]
    fn cnil_fixture_yields_3_paragraphs() {
        assert_eq!(texts(&text_html(CNIL)).len(), 3);
    }

    #[test]
    fn constit_paragraph_chars_sum_to_14236() {
        assert_eq!(chars_sum(&texts(&text_html(CONSTIT))), 14236);
    }

    #[test]
    fn cetat_paragraph_chars_sum_to_14524() {
        assert_eq!(chars_sum(&texts(&text_html(CETAT))), 14524);
    }

    #[test]
    fn cnil_paragraph_chars_sum_to_589() {
        assert_eq!(chars_sum(&texts(&text_html(CNIL))), 589);
    }

    #[test]
    fn fixtures_paragraphs_are_clean() {
        for fixture in [CONSTIT, CETAT, CNIL] {
            let (paragraphs, invisible) = split(&text_html(fixture));
            assert_eq!(invisible, 0);
            for paragraph in &paragraphs {
                assert_ne!(paragraph, "");
                assert_eq!(fold_ws(paragraph), *paragraph);
            }
        }
    }

    #[test]
    fn constit_text_is_html_plus_trailing_ecli() {
        // 03 §10 : `texte` est le HTML sans balises, suivi de l'ECLI que `RECH_ECLI` supprime.
        let text = text_field(CONSTIT, &["text", "texte"]);
        let ecli = text_field(CONSTIT, &["text", "ecli"]);
        let joined = texts(&text_html(CONSTIT)).join(" ");
        assert_eq!(fold_ws(&text), format!("{} {ecli}", fold_ws(&joined)));
    }

    proptest! {
        #[test]
        fn rows_lose_no_word_and_add_only_bars(
            parts in proptest::collection::vec(
                prop_oneof!["<td>", "</td>", "<th>", "<tr>", "</tr>", "<table>", "</table>", "<p>",
                    "</p>", "<br/>", "<li>", "<script>", "</script>", "a", "bc", " ", "\n"],
                0..60,
            ),
        ) {
            let html = parts.concat();
            // Mots de la lecture à plat (`scan`), qui n'ajoute aucun séparateur.
            let mut flat = String::new();
            scan(&html, |event| match event {
                Event::Text(text) => flat.push_str(text),
                Event::Break(_) | Event::Cell => flat.push(' '),
            });
            let flat: Vec<&str> = flat.split_whitespace().collect();
            let rows = texts(&html);
            // Les lignes n'ajoutent que des barres, ne commencent ni ne finissent par une barre,
            // et gardent tous les mots dans l'ordre.
            prop_assert!(rows.iter().all(|row| !row.starts_with('|') && !row.ends_with('|')));
            let kept: Vec<&str> =
                rows.iter().flat_map(|row| row.split_whitespace()).filter(|w| *w != "|").collect();
            prop_assert_eq!(kept, flat);
        }

        #[test]
        fn from_html_never_panics_and_preserves_text(
            parts in proptest::collection::vec(
                prop_oneof!["<", ">", "/", "\"", "=", " ", "\n", "!--", "-->", "&", "#", "x", ";",
                    "br", "p", "td", "script", "RECH_E", "é", "\u{a0}", "\u{200b}", "&shy;",
                    any::<char>().prop_map(String::from)],
                0..40,
            ),
            plain in "[a-zA-Z0-9éè .,;:\n\t]{0,80}",
        ) {
            // Chaque segment rendu est sans invisible ; repliés, les paragraphes sont non vides.
            let html = parts.concat();
            let mut segments = Vec::new();
            paragraphs(&html, &mut |visible| segments.push(visible.to_owned()));
            prop_assert!(segments.iter().all(|segment| !segment.chars().any(is_invisible)));
            for paragraph in &split(&html).0 {
                prop_assert_ne!(paragraph, "");
                prop_assert_eq!(&fold_ws(paragraph), paragraph);
            }
            // Sans `<` ni `&`, le texte est conservé, blancs repliés.
            let (kept, invisible) = split(&plain);
            prop_assert_eq!(invisible, 0);
            prop_assert_eq!(kept.join(" "), fold_ws(&plain));
            prop_assert_eq!(kept.len(), usize::from(!fold_ws(&plain).is_empty()));
        }
    }
}
