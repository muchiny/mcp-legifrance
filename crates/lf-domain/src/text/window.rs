//! Fenêtres de lecture paginées (`offset` / `max_chars`, jalon M5).
//!
//! Règle nouvelle, absente de la référence, qui ne coupe que `snippet[:280]`
//! (`citations.py:162`) : la fenêtre découle de l'économie de contexte (00 §7 / ADR-0006).
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
//! Un énumérateur n'est pas une fin de phrase (L4-02) : un point qui suit une ligne
//! faite seulement de jetons d'énumérateur (entiers, chiffres romains de I à XX, lettres
//! seules, `bis`, `ter`…, séparés par blancs, tirets ou points) ne coupe pas, sinon une
//! page finirait par « I bis. – 1. » et son contenu commencerait la suivante. La ligne
//! se mesure depuis le dernier saut de ligne de la tête de fenêtre (ou son début : une
//! fenêtre qui démarre en milieu de ligne ignore ce qui précède). « article 5. Suite »
//! reste une fin de phrase.
//!
//! Tient lieu de `text::truncate` (§4 de docs/plan/00-decisions.md, Domaine) :
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
///
/// ```
/// use lf_domain::text::{Window, window};
///
/// let text = "En application de l'article L. 1234-5 du code du travail, \
///             le salarié a droit à une indemnité.";
/// // Le point de « L. » ne termine pas la phrase : coupe au dernier espace.
/// let first = window(text, 0, 40);
/// assert_eq!(
///     first,
///     Window { text: "En application de l'article L. 1234-5 ", next_offset: Some(38) }
/// );
/// // La fenêtre suivante reprend à `next_offset` ; les fenêtres partitionnent le texte.
/// let second = window(text, 38, 100);
/// assert_eq!(second.next_offset, None);
/// assert_eq!(format!("{}{}", first.text, second.text), text);
/// ```
pub fn window(text: &str, offset: usize, max_chars: usize) -> Window<'_> {
    let rest = &text[byte_at(text, offset)..];
    let hard = byte_at(rest, max_chars.max(1));
    if hard == rest.len() {
        return Window { text: rest, next_offset: None };
    }
    let head = &rest[..hard];
    let cut = soft_cut(head).unwrap_or(hard);
    let chunk = &rest[..cut];
    Window { text: chunk, next_offset: Some(offset.saturating_add(chunk.chars().count())) }
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
    let before = &head[..j];
    let word = before
        .trim_end_matches('.')
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default();
    if !word.is_empty() && is_abbreviation(word) {
        return false;
    }
    !header_line(before).is_some_and(is_enumerator_line)
}

/// Longueur maximale, en caractères, d'une ligne d'en-tête d'énumérateur : « XVIII quaterdecies. –
/// 12. – a » en compte une trentaine. Au-delà, la ligne est du texte. La borne fixe aussi le coût
/// de chaque point examiné par [`soft_cut`], sans quoi une ligne amont « 1. 1. 1. … » sans saut de
/// ligne coûterait un temps quadratique dans la taille de la fenêtre.
const HEADER_LINE_MAX: usize = 64;

/// Ligne courante de `before` (depuis son dernier saut de ligne), ou `None` si elle dépasse
/// [`HEADER_LINE_MAX`] caractères : une ligne si longue n'est pas un en-tête.
fn header_line(before: &str) -> Option<&str> {
    for (count, (i, c)) in before.char_indices().rev().enumerate() {
        if c == '\n' {
            return before.get(i.saturating_add(1)..);
        }
        if count >= HEADER_LINE_MAX {
            return None;
        }
    }
    Some(before)
}

/// Chiffres romains de II à XX (en majuscules) ; « I », « V » et « X » sont des lettres seules.
const ROMAN: [&str; 17] = [
    "II", "III", "IV", "VI", "VII", "VIII", "IX", "XI", "XII", "XIII", "XIV", "XV", "XVI", "XVII",
    "XVIII", "XIX", "XX",
];

/// Suffixes d'ordinal des articles (en minuscules) : « 12 bis », « 3 quater ».
const ORDINALS: [&str; 12] = [
    "bis",
    "ter",
    "quater",
    "quinquies",
    "sexies",
    "septies",
    "octies",
    "nonies",
    "decies",
    "undecies",
    "duodecies",
    "terdecies",
];

/// Vrai pour un entier, une lettre seule, un chiffre romain de [`ROMAN`] ou un mot de [`ORDINALS`].
fn is_enumerator_token(token: &str) -> bool {
    let mut letters = token.chars();
    let single_letter = letters.next().is_some_and(char::is_alphabetic) && letters.next().is_none();
    single_letter
        || token.bytes().all(|b| b.is_ascii_digit())
        || ROMAN.contains(&token)
        || ORDINALS.contains(&token.to_lowercase().as_str())
}

/// Vrai si `line` (la ligne courante, sans le point qui suit) n'est faite que de jetons
/// d'énumérateur séparés par des blancs, des tirets ou des points : « I bis. – 1 » ou « II ».
/// Un point qui la suit ne termine pas une phrase, mais un en-tête (« article 5 » n'en est pas un).
fn is_enumerator_line(line: &str) -> bool {
    let mut tokens = line
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | '.'))
        .filter(|t| !t.is_empty())
        .peekable();
    tokens.peek().is_some() && tokens.all(is_enumerator_token)
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
        let end = i.saturating_add(c.len_utf8());
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
    fn the_header_line_stops_at_the_newline_and_at_its_length_cap() {
        assert_eq!(header_line("Alinéa.\nI bis"), Some("I bis"));
        assert_eq!(header_line("II"), Some("II"));
        assert_eq!(header_line(""), Some(""));
        assert_eq!(header_line("ligne\n"), Some(""));
        let at_cap = "–".repeat(HEADER_LINE_MAX);
        assert_eq!(header_line(&at_cap), Some(at_cap.as_str()));
        assert_eq!(header_line(&format!("x\n{at_cap}")), Some(at_cap.as_str()));
        let past_cap = "1".repeat(HEADER_LINE_MAX + 1);
        assert_eq!(header_line(&past_cap), None);
        assert_eq!(header_line(&format!("x\n{past_cap}")), None);
    }

    #[test]
    fn a_line_of_enumerators_longer_than_a_header_ends_a_sentence() {
        let short = "1. ".repeat(10);
        let short = short.trim_end();
        assert!(!ends_sentence(short, short.len() - 1, '.'), "{short:?}");
        let long = "1. ".repeat(20_000);
        let long = long.trim_end();
        assert!(ends_sentence(long, long.len() - 1, '.'));
        let cut = window(long, 0, 1_000);
        assert!(cut.text.ends_with(". "), "{:?}", cut.text);
        assert_eq!(cut.next_offset, Some(cut.text.chars().count()));
    }

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

    /// Texte de la première fenêtre d'au plus `max` caractères.
    fn cut_of(text: &str, max: usize) -> String {
        window(text, 0, max).text.to_owned()
    }

    #[test]
    fn an_enumerator_alone_on_its_line_is_not_a_sentence_end() {
        // Sans la règle, la page se terminait par « I bis. – 1. », le contenu à la page suivante.
        let text = "Le montant est fixé comme suit.\n\nI bis. – 1. Le prélèvement est dû";
        assert_eq!(cut_of(text, 45), "Le montant est fixé comme suit.\n\n");
        // Jumeau : le même texte sans énumérateur se coupe bien après la phrase.
        let text = "Le montant est fixé comme suit.\n\nLe bis. – Le prélèvement est dû";
        assert_eq!(cut_of(text, 44), "Le montant est fixé comme suit.\n\nLe bis. ");
        // Jumeau : une phrase qui se termine par un nombre reste une fin de phrase.
        let text = "Voir article 5. Suite des dispositions applicables";
        assert_eq!(cut_of(text, 30), "Voir article 5. ");
    }

    #[test]
    fn a_roman_numeral_header_is_kept_with_its_paragraph() {
        let text = "Le montant est fixé comme suit.\n\nII. Le prélèvement prévu au I est dû";
        assert_eq!(cut_of(text, 37), "Le montant est fixé comme suit.\n\n");
        // Un « II. » placé après une vraie phrase, sur la même ligne, reste une fin de phrase :
        // la ligne n'est pas faite que d'énumérateurs.
        let text = "Le montant est fixé. II. Le prélèvement prévu au I est dû";
        assert_eq!(cut_of(text, 26), "Le montant est fixé. II. ");
    }

    #[test]
    fn enumerator_rules() {
        for line in [
            "1",
            "12",
            "I",
            "II",
            "XIV",
            "XX",
            "a",
            "Z",
            "bis",
            "Ter",
            "quater",
            "quinquies",
            "decies",
            "I bis",
            "I bis – 1",
            "I — 1",
            "I\u{a0}bis",
            "1 - 2",
            "I – 1 bis",
            "2.3",
            "A.1",
            "1\t2",
        ] {
            assert!(is_enumerator_line(line), "{line:?}");
        }
        for line in [
            "",
            " ",
            "–",
            "...",
            "article 5",
            "article",
            "II.A. Suite",
            "XXI",
            "IIII",
            "IL",
            "CIVIL",
            "ii",
            "bise",
            "terre",
            "1a",
            "Le bis",
            "5 €",
            "1°",
        ] {
            assert!(!is_enumerator_line(line), "{line:?}");
        }
    }

    #[test]
    fn a_line_is_measured_from_the_last_newline() {
        // L'énumérateur est seul sur sa ligne, même si la ligne précédente porte du texte.
        let text = "Texte avant\n1. Suite du paragraphe numéroté et plus";
        assert_eq!(cut_of(text, 20), "Texte avant\n");
        // Une ligne qui porte du texte avant « 1. » : fin de phrase conservée.
        let text = "Texte avant 1. Suite du paragraphe numéroté et plus";
        assert_eq!(cut_of(text, 20), "Texte avant 1. ");
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
