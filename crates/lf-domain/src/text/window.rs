//! Fenêtres de lecture paginées (`offset` / `max_chars`, jalon M5).
//!
//! Règle nouvelle, absente de la référence, qui ne coupe que `snippet[:280]`
//! (`citations.py:162`) : la fenêtre découle de l'économie de contexte (§6).
//!
//! Coupe douce : un point d'abréviation juridique (`L.`, `art.`, `M.`, `G.`) ne
//! compte pas comme fin de phrase, sinon la fenêtre trancherait « l'article L. |
//! 1234-5 » et séparerait une citation de son numéro. Règle nouvelle, absente
//! de la référence : un point est une fin de phrase si le mot qui le précède
//! est vide ou n'est pas une abréviation (une lettre seule, ou un mot de
//! [`ABBREVIATIONS`]) ; sinon ce blanc est un espace ordinaire (repli). `! ? …
//! ;` et le saut de ligne restent des fins de phrase. Limite : on n'exige pas
//! de majuscule après le point.
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

/// Abréviations juridiques courantes, en minuscules, dont le point final ne
/// termine pas la phrase (en plus des lettres seules : `L.`, `M.`, `G.`).
const ABBREVIATIONS: [&str; 18] = [
    "al", "art", "cass", "cf", "chap", "civ", "com", "cons", "const", "crim", "ex", "mm", "pén",
    "préc", "réf", "soc", "trav", "vol",
];

/// Vrai pour une lettre seule ou un mot de [`ABBREVIATIONS`] (casse ignorée).
/// Un chiffre seul n'en est pas une : « article 5. Suite » se coupe après « 5. ».
fn is_abbreviation(word: &str) -> bool {
    let mut letters = word.chars();
    let single_letter = letters.next().is_some_and(char::is_alphabetic) && letters.next().is_none();
    single_letter || ABBREVIATIONS.contains(&word.to_lowercase().as_str())
}

/// Vrai si le caractère `p`, trouvé à l'octet `j` de `head`, termine une phrase.
fn ends_sentence(head: &str, j: usize, p: char) -> bool {
    if p != '.' {
        return matches!(p, '!' | '?' | '…' | ';');
    }
    let word = head[..j]
        .trim_end_matches('.')
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default();
    word.is_empty() || !is_abbreviation(word)
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
        let after_sentence = chars.peek().is_some_and(|&(j, p)| ends_sentence(head, j, p));
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

    const ARTICLE: &str = "En application de l'article L. 1234-5 du code du travail, le salarié a droit à une indemnité.";

    #[test]
    fn legal_abbreviation_dot_is_not_a_sentence_end() {
        // Sans la règle, la coupe tombait après « L. » : « l'article L. | 1234-5 ».
        assert_eq!(
            window(ARTICLE, 0, 40),
            Window { text: "En application de l'article L. 1234-5 ", next_offset: Some(38) }
        );
        // Mot de la liste.
        let text = "Voir les dispositions de l'art. 9 du code civil";
        assert_eq!(
            window(text, 0, 36),
            Window { text: "Voir les dispositions de l'art. 9 ", next_offset: Some(34) }
        );
        // Lettres seules : « M. Amine G. ».
        // Un espace ordinaire plus tard : sans la règle, la coupe tomberait à 26.
        let text = "Le contrat de M. Amine G. porte sur un bail";
        assert_eq!(
            window(text, 0, 34),
            Window { text: "Le contrat de M. Amine G. porte ", next_offset: Some(32) }
        );
    }

    #[test]
    fn digit_dot_and_ellipsis_still_end_a_sentence() {
        let text = "Voir article 5. Suite des dispositions";
        assert_eq!(window(text, 0, 30), Window { text: "Voir article 5. ", next_offset: Some(16) });
        let text = "Il hésite à conclure... Suite du texte";
        assert_eq!(
            window(text, 0, 30),
            Window { text: "Il hésite à conclure... ", next_offset: Some(24) }
        );
        // Point sans mot devant : seule une abréviation bloque, c'est une fin de phrase.
        let text = "a bbbbbbbbbbbbbbbbbbbb . cc dd ee ff gg";
        assert_eq!(
            window(text, 0, 34),
            Window { text: "a bbbbbbbbbbbbbbbbbbbb . ", next_offset: Some(25) }
        );
    }

    #[test]
    fn dot_after_punctuation_or_symbol_ends_a_sentence() {
        // Guillemet, parenthèse, apostrophe, symbole : le mot devant le point est vide.
        for closing in ["».", ").", "\".", "’.", "€.", "%."] {
            let text = format!("bail résilié{closing} Suite du texte long");
            let expected = format!("bail résilié{closing} ");
            assert_eq!(
                window(&text, 0, 26),
                Window { text: &expected, next_offset: Some(15) },
                "{closing}"
            );
        }
    }

    #[test]
    fn abbreviation_rules() {
        assert!(is_abbreviation("L"));
        assert!(is_abbreviation("é"));
        assert!(is_abbreviation("Art"));
        assert!(is_abbreviation("PÉN"));
        assert!(is_abbreviation("vol"));
        assert!(!is_abbreviation("5"));
        assert!(!is_abbreviation(""));
        assert!(!is_abbreviation("article"));
        assert!(!is_abbreviation("LL"));
    }

    #[test]
    fn soft_cut_recognizes_each_terminator() {
        for t in ['!', '?', '…', ';'] {
            let text = format!("abcdef{t} gh ijklmno");
            assert_eq!(
                window(&text, 0, 12),
                Window { text: &format!("abcdef{t} "), next_offset: Some(8) }
            );
        }
        let text = "abcdefx gh ijklmno";
        assert_eq!(window(text, 0, 12), Window { text: "abcdefx gh ", next_offset: Some(11) });
    }

    #[test]
    fn extreme_bounds_do_not_overflow() {
        assert_eq!(window("abc", usize::MAX, usize::MAX), Window { text: "", next_offset: None });
        assert_eq!(window("abc", 0, usize::MAX), Window { text: "abc", next_offset: None });
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

        /// Alphabet de terminateurs, de points et de blancs : les coupes douces
        /// (phrase, saut de ligne, abréviation, repli) sont exercées à chaque tirage.
        #[test]
        fn windows_partition_with_soft_boundaries(
            text in "[a-zéà .!?…;\n]{0,300}",
            max in 1usize..40,
        ) {
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
