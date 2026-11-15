//! Nettoyage des textes du BOFiP (ADR-0029, décision 4).
//!
//! Le champ `contenu` du jeu `bofip-vigueur` est du texte brut, sans balise, où restent cinq
//! entités et elles seules (mesure du 2026-11-15 sur les 9 173 publications) : `&#13;` (70 038
//! fois), `&nbsp;` (14 532), `&gt;` (492), `&amp;` (328) et `&lt;` (114). S'y ajoutent des retours
//! chariot, des tabulations et des espaces insécables.
//!
//! Les cinq entités sont traitées en **une seule passe** : le résultat d'un décodage n'est jamais
//! relu, donc `&amp;lt;` reste `&lt;` (le texte parlait de l'entité, il n'écrivait pas un
//! chevron). Toute autre entité est laissée telle quelle et comptée : le compte est rendu à
//! l'appelant, qui peut le journaliser. Le nettoyage HTML des champs de Légifrance
//! ([`clean`](crate::text::clean)) ne sert pas ici : il est fait pour du balisage, et un `<` du
//! BOFiP est un chevron.

use crate::{
    error::DomainError,
    text::{LongText, flatten_whitespace, strip_invisible},
};

/// Longueur maximale, en caractères, d'un champ court rendu ([`clean_line`]) ; au-delà, le champ
/// est coupé et suivi de `…`. Le titre le plus long mesuré le 2026-11-15 fait 747 caractères.
pub const LINE_MAX_CHARS: usize = 1_000;

/// Les cinq entités traitées, sans leur `&` initial, et ce qu'elles deviennent.
const KNOWN_ENTITIES: [(&str, &str); 5] =
    [("#13;", ""), ("nbsp;", " "), ("lt;", "<"), ("gt;", ">"), ("amp;", "&")];

/// Longueur maximale du nom d'une entité comptée comme inconnue (`&` et `;` exclus).
const ENTITY_NAME_MAX: usize = 32;

/// Texte canonique du champ `contenu` et nombre d'entités inconnues qui y restent.
///
/// `&#13;` et le retour chariot sont retirés ; `&nbsp;`, l'espace insécable et la tabulation
/// deviennent une espace ; `&lt;`, `&gt;` et `&amp;` sont décodés. Le texte passe ensuite par
/// [`LongText::from_plain`] : un paragraphe par ligne non blanche, caractères invisibles
/// retirés, blancs repliés.
///
/// ```
/// use lf_domain::doctrine::clean_text;
///
/// let (text, unknown) = clean_text("1&#13;\nTaux&nbsp;: 5 % &amp; 10 %&#13;\n&eacute;")?;
/// assert_eq!(text.as_str(), "1\n\nTaux : 5 % & 10 %\n\n&eacute;");
/// assert_eq!(unknown, 1);
/// # Ok::<(), lf_domain::DomainError>(())
/// ```
///
/// # Errors
///
/// [`DomainError::TextTooLarge`] si le texte dépasse [`LongText::MAX_CHARS`] caractères.
pub fn clean_text(raw: &str) -> Result<(LongText, u32), DomainError> {
    let (decoded, unknown) = decode(raw);
    LongText::from_plain(&decoded).map(|text| (text, unknown))
}

/// Champ court (titre, division) sur une ligne : mêmes entités que [`clean_text`], caractères
/// invisibles retirés, blancs repliés, [`LINE_MAX_CHARS`] caractères au plus (`…` ensuite).
///
/// ```
/// use lf_domain::doctrine::clean_line;
///
/// assert_eq!(clean_line(" ENR -\u{a0}Mutations &amp; partages\r\n"), "ENR - Mutations & partages");
/// ```
pub fn clean_line(raw: &str) -> String {
    let (decoded, _) = decode(raw);
    let (visible, _) = strip_invisible(&decoded);
    clipped(&flatten_whitespace(&visible), LINE_MAX_CHARS)
}

/// `raw` sans retour chariot, ses cinq entités connues décodées en une passe, et le nombre
/// d'entités inconnues laissées telles quelles.
///
/// `raw` est découpé sur `&` : chaque morceau après le premier suit un `&`. S'il commence par
/// une entité connue, elle est remplacée ; sinon le `&` est remis. Rien de ce qui est écrit
/// n'est relu.
fn decode(raw: &str) -> (String, u32) {
    let mut pieces = raw.split('&');
    let mut out = String::with_capacity(raw.len());
    let mut unknown = 0_u32;
    push_without_cr(&mut out, pieces.next().unwrap_or_default());
    for tail in pieces {
        let known =
            KNOWN_ENTITIES.iter().find_map(|(name, to)| Some((*to, tail.strip_prefix(name)?)));
        let rest = if let Some((to, rest)) = known {
            out.push_str(to);
            rest
        } else {
            unknown = unknown.saturating_add(u32::from(is_entity(tail)));
            out.push('&');
            tail
        };
        push_without_cr(&mut out, rest);
    }
    (out, unknown)
}

/// Ajoute `text` à `out`, sans ses retours chariot.
fn push_without_cr(out: &mut String, text: &str) {
    out.extend(text.chars().filter(|c| *c != '\r'));
}

/// Vrai si `tail`, le texte qui suit un `&`, commence comme une entité : un `#` facultatif, de
/// 1 à [`ENTITY_NAME_MAX`] lettres ASCII ou chiffres, puis `;`. Un `&` suivi d'autre chose
/// (« R&D », « A & B ») n'est pas une entité.
fn is_entity(tail: &str) -> bool {
    let name = tail.strip_prefix('#').unwrap_or(tail);
    let len = name.bytes().take_while(u8::is_ascii_alphanumeric).count();
    (1..=ENTITY_NAME_MAX).contains(&len) && name.as_bytes().get(len) == Some(&b';')
}

/// `text` coupé à `max` caractères, suivi de `…` s'il était plus long.
fn clipped(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() { head + "…" } else { head }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn cleaned(raw: &str) -> (String, u32) {
        let (text, unknown) = clean_text(raw).unwrap();
        (text.as_str().to_owned(), unknown)
    }

    #[test]
    fn the_five_measured_entities_are_handled() {
        assert_eq!(cleaned("a&#13;b"), ("ab".to_owned(), 0));
        assert_eq!(cleaned("a&nbsp;b"), ("a b".to_owned(), 0));
        assert_eq!(cleaned("a &lt; b &gt; c"), ("a < b > c".to_owned(), 0));
        assert_eq!(cleaned("R&amp;D"), ("R&D".to_owned(), 0));
        assert_eq!(cleaned("&lt;&gt;&amp;&nbsp;&#13;x"), ("<>& x".to_owned(), 0));
    }

    /// Le résultat d'un décodage n'est jamais relu : une entité écrite en clair dans le texte
    /// (`&amp;lt;`) reste une entité, elle ne devient pas le caractère.
    #[test]
    fn entities_decoded_in_one_pass() {
        for (raw, expected) in [
            ("&amp;lt;", "&lt;"),
            ("&amp;gt;", "&gt;"),
            ("&amp;amp;", "&amp;"),
            ("&amp;#13;", "&#13;"),
            ("&amp;nbsp;", "&nbsp;"),
            ("&amp;amp;lt;", "&amp;lt;"),
            ("&amp;&amp;lt;", "&&lt;"),
            ("&&amp;lt;", "&&lt;"),
        ] {
            assert_eq!(decode(raw), (expected.to_owned(), 0));
            assert_eq!(cleaned(raw), (expected.to_owned(), 0));
        }
        // Un retour chariot retiré ne recolle pas une entité : rien n'est relu.
        assert_eq!(decode("&l\rt;"), ("&lt;".to_owned(), 0));
        assert_eq!(decode("&&#13;lt;"), ("&lt;".to_owned(), 0));
    }

    #[test]
    fn carriage_returns_are_removed_and_lines_become_paragraphs() {
        assert_eq!(cleaned("un\r\ndeux&#13;\ntrois"), ("un\n\ndeux\n\ntrois".to_owned(), 0));
        assert_eq!(cleaned("a\rb"), ("ab".to_owned(), 0));
        assert_eq!(decode("\r\ra\r\n\r"), ("a\n".to_owned(), 0));
    }

    #[test]
    fn blanks_become_one_space() {
        assert_eq!(cleaned("a\u{a0}b\tc &nbsp;\u{a0}\t d"), ("a b c d".to_owned(), 0));
        assert_eq!(cleaned("  a  \n\n\n  b  "), ("a\n\nb".to_owned(), 0));
    }

    #[test]
    fn unknown_entities_are_kept_and_counted() {
        assert_eq!(cleaned("&eacute;t&eacute;"), ("&eacute;t&eacute;".to_owned(), 2));
        assert_eq!(cleaned("&#233; &#xE9; &#013;"), ("&#233; &#xE9; &#013;".to_owned(), 3));
        assert_eq!(cleaned("&NBSP; &Lt; &AMP;"), ("&NBSP; &Lt; &AMP;".to_owned(), 3));
        assert_eq!(cleaned("&amp;&copy;"), ("&&copy;".to_owned(), 1));
    }

    #[test]
    fn a_lone_ampersand_is_not_an_entity() {
        for raw in ["R&D", "A & B", "a&", "&", "&;", "&#;", "& lt;", "&l t;", "&é;", "&lt", "&##1;"]
        {
            assert_eq!(cleaned(raw), (raw.to_owned(), 0));
        }
    }

    #[test]
    fn entity_names_are_bounded() {
        let longest = format!("&{};", "a".repeat(ENTITY_NAME_MAX));
        assert_eq!(decode(&longest), (longest.clone(), 1));
        let too_long = format!("&{};", "a".repeat(ENTITY_NAME_MAX + 1));
        assert_eq!(decode(&too_long), (too_long.clone(), 0));
    }

    #[test]
    fn empty_text_is_empty() {
        let (text, unknown) = clean_text("").unwrap();
        assert!(text.is_empty());
        assert_eq!((text.char_len(), unknown), (0, 0));
        assert!(clean_text("&#13;\r\n&nbsp;\t").unwrap().0.is_empty());
    }

    #[test]
    fn invisible_characters_are_removed_from_the_text() {
        let (text, _) = clean_text("plus\u{200b}-value\u{202e}").unwrap();
        assert_eq!((text.as_str(), text.invisible_removed()), ("plus-value", 2));
    }

    #[test]
    fn text_over_the_limit_is_refused() {
        let raw = "é".repeat(16_000_001);
        assert_eq!(clean_text(&raw), Err(DomainError::TextTooLarge { chars: 16_000_001 }));
    }

    #[test]
    fn a_line_is_decoded_flattened_and_stripped() {
        assert_eq!(
            clean_line(" ENR -\u{a0}Mutations &amp; partages\r\n"),
            "ENR - Mutations & partages"
        );
        assert_eq!(clean_line("a\n\nb\tc&nbsp;&nbsp;d"), "a b c d");
        assert_eq!(clean_line("IR\u{200b} - \u{202e}Base&#13;"), "IR - Base");
        assert_eq!(clean_line("&amp;lt; &eacute;"), "&lt; &eacute;");
        assert_eq!(clean_line(" \r\n\t&nbsp;"), "");
    }

    #[test]
    fn a_line_is_clipped_with_an_ellipsis() {
        let longest = "é".repeat(LINE_MAX_CHARS);
        assert_eq!(clean_line(&longest), longest);
        let clipped = clean_line(&"é".repeat(LINE_MAX_CHARS + 1));
        assert_eq!(clipped, format!("{longest}…"));
        assert_eq!(super::clipped("abc", 2), "ab…");
        assert_eq!(super::clipped("abc", 3), "abc");
    }

    proptest! {
        /// Sans `&` ni retour chariot, le décodage ne change rien et ne compte rien.
        #[test]
        fn text_without_ampersand_is_unchanged(raw in "[^&\r]{0,200}") {
            prop_assert_eq!(decode(&raw), (raw.clone(), 0));
        }

        /// Le décodage ne laisse aucun retour chariot et ne rallonge jamais le texte.
        #[test]
        fn decoding_never_grows_the_text(raw in "(&(amp|lt|gt|nbsp|#13|x);|[a&;#\r ]){0,60}") {
            let (out, unknown) = decode(&raw);
            prop_assert!(!out.contains('\r'));
            prop_assert!(out.len() <= raw.len());
            prop_assert!(usize::try_from(unknown).unwrap() <= raw.matches('&').count());
        }
    }
}
