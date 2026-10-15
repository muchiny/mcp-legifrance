//! Découpe d'un texte long en passages adressables par `offset`.

use super::window::window;

/// Passage de texte et sa position (en caractères) dans le texte complet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Passage<'a> {
    /// Position du premier caractère.
    pub offset: usize,
    /// Contenu.
    pub text: &'a str,
}

/// Fenêtres successives d'au plus `max_chars` caractères couvrant tout `text`.
pub fn passages(text: &str, max_chars: usize) -> Vec<Passage<'_>> {
    let mut out = Vec::new();
    let mut offset = Some(0);
    // Chaque fenêtre avance d'au moins un caractère : `len + 1` tours suffisent,
    // et la borne garantit la terminaison quoi qu'il arrive.
    for _ in 0..=text.len() {
        let Some(start) = offset else { break };
        let w = window(text, start, max_chars);
        if !w.text.is_empty() {
            out.push(Passage { offset: start, text: w.text });
        }
        offset = w.next_offset;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn splits_on_sentences_with_offsets() {
        let text = "Première phrase. Deuxième phrase. Troisième.";
        assert_eq!(
            passages(text, 20),
            [
                Passage { offset: 0, text: "Première phrase. " },
                Passage { offset: 17, text: "Deuxième phrase. " },
                Passage { offset: 34, text: "Troisième." },
            ]
        );
    }

    #[test]
    fn empty_text_has_no_passage() {
        assert_eq!(passages("", 10), []);
    }

    proptest! {
        #[test]
        fn passages_partition_the_text(text in "\\PC{1,300}", max in 0usize..60) {
            let mut seen = 0;
            let mut rebuilt = String::new();
            for p in passages(&text, max) {
                prop_assert_eq!(p.offset, seen);
                prop_assert!(!p.text.is_empty());
                prop_assert!(p.text.chars().count() <= max.max(1));
                seen += p.text.chars().count();
                rebuilt.push_str(p.text);
            }
            prop_assert_eq!(rebuilt, text);
        }
    }
}
