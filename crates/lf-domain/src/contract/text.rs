//! `CitationText` : citation lisible, sur une ligne et bornée (ADR-0009, décision C2).
//!
//! Règle nouvelle, absente de la référence, où `human_readable_citation` pouvait
//! être `null` ou multiligne ; la coupe est partagée avec le `Snippet` (03 §7.3), par
//! `cut_at_word`, dont la borne est un paramètre.
//! Invariants garantis par [`CitationText::new`] (03 §1.4) :
//!
//! - jamais vide ;
//! - une seule ligne : chaque blanc Unicode (`char::is_whitespace`) devient
//!   U+0020, les suites sont repliées en une seule et le texte est rogné ;
//! - aucun caractère de contrôle (`char::is_control`, retiré sans laisser
//!   d'espace ; NEL, à la fois blanc et contrôle, devient une espace) ;
//! - au plus [`CITATION_MAX_CHARS`] caractères (`char`), `…` compris.
//!
//! Au-delà, les 999 premiers caractères sont gardés, coupés à la dernière
//! espace située dans leurs 40 derniers (`CUT_WINDOW_CHARS`), sinon coupe
//! franche, puis `…` est ajouté et [`CitationText::is_truncated`] vaut `true`
//! (la vue rend aussi `title`, 05 §5.7).

use std::fmt;

/// Nombre maximal de caractères d'une citation, `…` compris.
pub const CITATION_MAX_CHARS: usize = 1_000;

/// Largeur de la fenêtre où chercher une espace de coupe.
const CUT_WINDOW_CHARS: usize = 40;

/// Citation lisible validée : une ligne, non vide, bornée.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CitationText {
    text: String,
    truncated: bool,
}

impl CitationText {
    /// Normalise `raw` ; `None` s'il ne reste aucun caractère visible.
    ///
    /// ```
    /// use lf_domain::contract::CitationText;
    ///
    /// let citation = CitationText::new("  Code civil,\n\tarticle 1240\u{7} ");
    /// assert_eq!(citation.as_ref().map(CitationText::as_str), Some("Code civil, article 1240"));
    /// assert_eq!(CitationText::new(" \n\t "), None);
    /// // 1 199 caractères : 995 gardés, coupés en fin de mot, puis `…`.
    /// let long = CitationText::new(&"mot ".repeat(300));
    /// assert_eq!(
    ///     long.as_ref().map(|c| (c.as_str().chars().count(), c.is_truncated())),
    ///     Some((996, true))
    /// );
    /// assert_eq!(long.as_ref().map(|c| c.as_str().ends_with(" mot…")), Some(true));
    /// ```
    ///
    /// La logique vit dans `normalized`, `fold_blanks` et `cut_at_word` : cargo-mutants ne
    /// mute pas les fonctions nommées `new` (`CONTRIBUTING.md`).
    pub fn new(raw: &str) -> Option<Self> {
        Self::normalized(raw)
    }

    fn normalized(raw: &str) -> Option<Self> {
        let text = fold_blanks(raw);
        (!text.is_empty()).then(|| match cut_at_word(&text, CITATION_MAX_CHARS) {
            Some(cut) => Self { text: cut, truncated: true },
            None => Self { text, truncated: false },
        })
    }

    /// Repli `Légifrance {id}` ; `id` est un identifiant validé (`[A-Z0-9]`).
    pub(crate) fn fallback(id: &str) -> Self {
        Self { text: format!("Légifrance {id}"), truncated: false }
    }

    /// Le texte.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Vrai si le texte d'origine a été coupé.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }
}

impl fmt::Display for CitationText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Replie les blancs en une espace, retire les contrôles et rogne, en une passe.
fn fold_blanks(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            pending_space = true;
        } else if !c.is_control() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(c);
        }
    }
    out
}

/// Texte coupé à `max_chars` caractères (`…` compris) en fin de mot, ou `None` s'il tient déjà.
///
/// Les `max_chars - 1` premiers caractères sont gardés, coupés à la dernière espace de leurs
/// [`CUT_WINDOW_CHARS`] derniers, sinon coupe franche, puis `…` est ajouté. `max_chars` vaut au
/// moins 1 : les appelants passent une constante.
pub(crate) fn cut_at_word(text: &str, max_chars: usize) -> Option<String> {
    let mut rest = text.char_indices().skip(max_chars - 1);
    let (head_end, _) = rest.next()?;
    rest.next()?;
    let head = &text[..head_end];
    let cut = head
        .char_indices()
        .rev()
        .take(CUT_WINDOW_CHARS)
        .find(|&(_, c)| c == ' ')
        .map_or(head.len(), |(i, _)| i);
    Some(format!("{}…", &head[..cut]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// `n` fois le caractère `c`.
    fn run(c: char, n: usize) -> String {
        std::iter::repeat_n(c, n).collect()
    }

    /// Texte et drapeau de coupe, pour comparer d'un bloc.
    fn parts(raw: &str) -> Option<(String, bool)> {
        CitationText::new(raw).map(|t| (t.as_str().to_owned(), t.is_truncated()))
    }

    #[test]
    fn blank_or_control_only_is_none() {
        let blanks = ["", " ", " \n\t\r ", "\u{a0}\u{202f}\u{2028}\u{3000}"];
        let controls = ["\u{0}\u{7}\u{1b}\u{7f}", "\u{0} \u{9f}"];
        for raw in blanks.into_iter().chain(controls) {
            assert_eq!(CitationText::new(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn blanks_fold_to_one_space_and_controls_vanish() {
        let cases = [
            ("Code civil, art. 9", "Code civil, art. 9"),
            ("  Code\tcivil,\n\nart.\u{a0}9  ", "Code civil, art. 9"),
            ("n°\u{202f}9", "n° 9"),
            // NEL est un blanc et un contrôle : il devient une espace.
            ("a\u{85}b", "a b"),
            // Contrôle hors blanc : retiré, sans laisser d'espace.
            ("a\u{0}b\u{7f}c", "abc"),
            // Contrôle entre deux blancs : la suite reste une seule espace.
            ("a \u{0} \u{1b}\tb", "a b"),
            ("\u{0} a", "a"),
            ("a \u{0}", "a"),
        ];
        for (raw, expected) in cases {
            assert_eq!(parts(raw), Some((expected.to_owned(), false)), "{raw:?}");
        }
    }

    #[test]
    fn display_is_the_text() {
        assert_eq!(CitationText::new(" Code civil ").unwrap().to_string(), "Code civil");
    }

    #[test]
    fn up_to_1000_chars_is_kept_whole() {
        let exact = run('é', CITATION_MAX_CHARS);
        assert_eq!(parts(&exact), Some((exact.clone(), false)));
    }

    #[test]
    fn beyond_1000_chars_without_space_is_cut_hard() {
        let expected = format!("{}…", run('é', 999));
        assert_eq!(expected.chars().count(), CITATION_MAX_CHARS);
        assert_eq!(parts(&run('é', CITATION_MAX_CHARS + 1)), Some((expected, true)));
    }

    #[test]
    fn cut_at_last_space_within_last_40_chars() {
        // `index` fois `a`, une espace à cet indice, puis 1 100 `b`. Les 999
        // premiers caractères sont gardés ; la fenêtre couvre les indices 959 à 998.
        let cases = [
            (959, format!("{}…", run('a', 959))),
            (998, format!("{}…", run('a', 998))),
            (958, format!("{} {}…", run('a', 958), run('b', 40))),
            (999, format!("{}…", run('a', 999))),
        ];
        for (index, expected) in cases {
            let raw = format!("{} {}", run('a', index), run('b', 1_100));
            assert_eq!(parts(&raw), Some((expected, true)), "{index}");
        }
        // Deux espaces dans la fenêtre : la coupe se fait à la dernière.
        let raw = format!("{} {} {}", run('a', 970), run('b', 10), run('c', 100));
        let expected = format!("{} {}…", run('a', 970), run('b', 10));
        assert_eq!(parts(&raw), Some((expected, true)));
    }

    #[test]
    fn cut_at_word_respects_the_bound_it_is_given() {
        // Borne 10 : 9 caractères gardés, coupés à la dernière espace, `…` compris dans 10.
        assert_eq!(cut_at_word("aaaa bbbb cccc", 10), Some("aaaa…".to_owned()));
        assert_eq!(cut_at_word("aaaaaaaaaaaa", 10), Some(format!("{}…", run('a', 9))));
        assert_eq!(cut_at_word(&run('a', 10), 10), None);
        assert_eq!(cut_at_word(&run('a', 11), 10), Some(format!("{}…", run('a', 9))));
        // Borne 280 : 279 caractères gardés.
        assert_eq!(cut_at_word(&run('é', 280), 280), None);
        assert_eq!(cut_at_word(&run('é', 281), 280), Some(format!("{}…", run('é', 279))));
        // Borne 1 : plus rien de gardé que `…`.
        assert_eq!(cut_at_word("ab", 1), Some("…".to_owned()));
    }

    #[test]
    fn fallback_names_the_identifier() {
        let t = CitationText::fallback("CETATEXT000054178503");
        assert_eq!((t.as_str(), t.is_truncated()), ("Légifrance CETATEXT000054178503", false));
    }

    /// Invariants violés par `CitationText::new(raw)` (liste vide si aucun).
    /// Oracle indépendant : contrôles hors blancs retirés, puis blancs repliés
    /// par `split_whitespace` ; sans coupe, le texte lui est égal.
    fn broken_invariants(raw: &str) -> Vec<&'static str> {
        let visible: String =
            raw.chars().filter(|c| c.is_whitespace() || !c.is_control()).collect();
        let folded = visible.split_whitespace().collect::<Vec<_>>().join(" ");
        let checks = match CitationText::new(raw) {
            None => vec![("None only without content", folded.is_empty())],
            Some(t) => {
                let s = t.as_str();
                let again = CitationText::new(s);
                let cut = folded.chars().count() > CITATION_MAX_CHARS;
                vec![
                    ("never empty", !s.is_empty()),
                    ("at most 1000 chars", s.chars().count() <= CITATION_MAX_CHARS),
                    (
                        "no control, U+0020 only",
                        s.chars().all(|c| !c.is_control() && (c == ' ' || !c.is_whitespace())),
                    ),
                    ("trimmed, single spaces", !s.contains("  ") && s.trim() == s),
                    ("same text again", again.as_ref().map(CitationText::as_str) == Some(s)),
                    ("fixed point unless cut", t.is_truncated() || again == Some(t.clone())),
                    ("cut beyond 1000 only", t.is_truncated() == cut),
                    ("whole text unless cut", cut || s == folded),
                    (
                        "prefix and ellipsis if cut",
                        !cut || s.strip_suffix('…').is_some_and(|head| folded.starts_with(head)),
                    ),
                ]
            }
        };
        checks.into_iter().filter_map(|(name, ok)| (!ok).then_some(name)).collect()
    }

    #[test]
    fn invariants_hold_on_examples() {
        let (hard, soft) = (run('é', 1_100), format!("{} {}", run('a', 980), run('b', 50)));
        for raw in ["", " \u{0}\n", "a", "  Code\tcivil  ", hard.as_str(), soft.as_str()] {
            assert_eq!(broken_invariants(raw), Vec::<&str>::new(), "{raw:?}");
        }
    }

    proptest! {
        #[test]
        fn any_text_keeps_the_invariants(raw in "(?s).{0,1200}") {
            prop_assert_eq!(broken_invariants(&raw), Vec::<&str>::new());
        }

        #[test]
        fn words_and_blanks_keep_the_invariants(
            raw in "([a-zé0-9]{1,30}[ \\t\\n\\x{0}\\x{7}\\x{85}\\x{a0}\\x{202f}]{1,3}){0,100}",
        ) {
            prop_assert_eq!(broken_invariants(&raw), Vec::<&str>::new());
        }
    }
}
