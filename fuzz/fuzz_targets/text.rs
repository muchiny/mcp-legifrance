#![no_main]

use std::borrow::Cow;

use lf_domain::text::{
    clean,
    encoding::{has_latin1_mojibake, repair_latin1_mojibake},
    passages, rank, window,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (&str, &str, u16, u16)| {
    let (html, query, offset, max) = input;
    let max = usize::from(max).max(1);
    // Réparation avant nettoyage (03 §7.1) : une réparation acceptée ne laisse
    // aucun motif, et un second passage rend toujours le texte emprunté, tel quel.
    let repaired = repair_latin1_mojibake(html);
    if let Cow::Owned(fixed) = &repaired {
        assert!(!has_latin1_mojibake(fixed));
    }
    let again = repair_latin1_mojibake(&repaired);
    assert_eq!((matches!(again, Cow::Owned(_)), again.as_ref()), (false, repaired.as_ref()));
    let text = clean(&repaired);
    let w = window(&text, usize::from(offset), max);
    assert!(w.text.chars().count() <= max);
    // Les passages recouvrent exactement le texte nettoyé.
    let parts = passages(&text, max);
    let rebuilt: String = parts.iter().map(|p| p.text).collect();
    assert_eq!(rebuilt, text);
    let texts: Vec<&str> = parts.iter().map(|p| p.text).collect();
    for scored in rank(&texts, query, 5) {
        assert!(scored.score > 0.0 && scored.index < texts.len());
    }
});
