//! Fenêtres de lecture paginées (`offset` / `max_chars`, jalon M5).
//!
//! Règle nouvelle, absente de la référence, qui ne coupe que `snippet[:280]`
//! (`citations.py:162`) : la fenêtre découle de l'économie de contexte (§6).
//!
//! Tient lieu de `text::truncate` (§4 de docs/plan/00-decisions.md) :
//! `window(t, 0, n).text` est la troncature en fin de phrase.
//!
//! Positions en caractères (pas en octets) : sûres en UTF-8 et compréhensibles
//! par un modèle. Les fenêtres successives partitionnent exactement le texte.
//! La « seconde moitié » de la coupe douce est mesurée en octets
//! (`head.len() / 2`).

/// Fenêtre de texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window<'a> {
    /// Contenu de la fenêtre.
    pub text: &'a str,
    /// Position de la fenêtre suivante, absente en fin de texte.
    pub next_offset: Option<usize>,
}

/// Fenêtre d'au plus `max_chars` caractères (au moins 1) à partir de `offset`,
/// coupée de préférence après une fin de phrase, sinon après un espace, dans la
/// seconde moitié de la fenêtre.
pub fn window(text: &str, offset: usize, max_chars: usize) -> Window<'_> {
    let rest = &text[byte_at(text, offset)..];
    let hard = byte_at(rest, max_chars.max(1));
    if hard == rest.len() {
        return Window { text: rest, next_offset: None };
    }
    let head = &rest[..hard];
    let cut = soft_cut(head).unwrap_or(hard);
    let chunk = &rest[..cut];
    Window { text: chunk, next_offset: Some(offset + chunk.chars().count()) }
}

/// Indice en octets du caractère n° `chars`, ou longueur de `s`.
fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(i, _)| i)
}

/// Coupe juste après un espace, dans la seconde moitié de `head` : d'abord un
/// espace précédé d'une fin de phrase ou un saut de ligne, sinon tout espace.
fn soft_cut(head: &str) -> Option<usize> {
    let min = head.len() / 2;
    let mut fallback = None;
    let mut chars = head.char_indices().rev().peekable();
    while let Some((i, c)) = chars.next() {
        if i < min {
            break;
        }
        if !c.is_whitespace() {
            continue;
        }
        let end = i + c.len_utf8();
        let after_sentence =
            chars.peek().is_some_and(|&(_, p)| matches!(p, '.' | '!' | '?' | '…' | ';'));
        if c == '\n' || after_sentence {
            return Some(end);
        }
        fallback = fallback.or(Some(end));
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn whole_text_fits() {
        assert_eq!(window("abc", 0, 10), Window { text: "abc", next_offset: None });
        assert_eq!(window("abc", 0, 3), Window { text: "abc", next_offset: None });
        assert_eq!(window("", 0, 10), Window { text: "", next_offset: None });
    }

    #[test]
    fn offset_past_end_is_empty() {
        assert_eq!(window("abc", 99, 10), Window { text: "", next_offset: None });
    }

    #[test]
    fn prefers_sentence_end() {
        let text = "Une phrase. Puis une autre phrase longue";
        assert_eq!(window(text, 0, 20), Window { text: "Une phrase. ", next_offset: Some(12) });
    }

    #[test]
    fn prefers_newline() {
        let text = "Ligne une\nligne deux suite";
        assert_eq!(window(text, 0, 14), Window { text: "Ligne une\n", next_offset: Some(10) });
    }

    #[test]
    fn falls_back_to_last_space() {
        let text = "mot mot mot mot mot";
        assert_eq!(window(text, 0, 10), Window { text: "mot mot ", next_offset: Some(8) });
    }

    #[test]
    fn ignores_boundaries_in_first_half() {
        // Seule fin de phrase avant la moitié : on prend le dernier espace.
        let text = "A. bbbbbbb cccccccccccccc";
        assert_eq!(window(text, 0, 12), Window { text: "A. bbbbbbb ", next_offset: Some(11) });
        // Aucun espace dans la seconde moitié : coupe dure.
        let text = "A. bbbbbbbbbbbbbbbbbbbbbb";
        assert_eq!(window(text, 0, 10), Window { text: "A. bbbbbbb", next_offset: Some(10) });
    }

    #[test]
    fn boundary_exactly_at_half_is_accepted() {
        // head = "abc defg" (8 octets), min = 4 ; l'espace est à 3 < 4 : refusé.
        assert_eq!(window("abc defgh", 0, 8), Window { text: "abc defg", next_offset: Some(8) });
        // head = "abcd efg" ; l'espace est à 4 = min : accepté.
        assert_eq!(window("abcd efgh", 0, 8), Window { text: "abcd ", next_offset: Some(5) });
    }

    #[test]
    fn offsets_are_in_chars() {
        let text = "été été été";
        assert_eq!(window(text, 4, 4), Window { text: "été ", next_offset: Some(8) });
    }

    #[test]
    fn zero_max_chars_still_progresses() {
        assert_eq!(window("ab", 0, 0), Window { text: "a", next_offset: Some(1) });
    }

    proptest! {
        #[test]
        fn windows_partition_the_text(text in "\\PC{0,300}", max in 1usize..60) {
            let len = text.chars().count();
            let mut rebuilt = String::new();
            let mut offset = 0;
            loop {
                let w = window(&text, offset, max);
                prop_assert!(w.text.chars().count() <= max);
                rebuilt.push_str(w.text);
                match w.next_offset {
                    Some(next) => {
                        prop_assert!(next > offset);
                        prop_assert!(next < len);
                        offset = next;
                    }
                    None => break,
                }
            }
            prop_assert_eq!(rebuilt, text);
        }
    }
}
