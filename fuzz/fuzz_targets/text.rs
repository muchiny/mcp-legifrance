#![no_main]

use lf_domain::text::{clean, passages, rank, window};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (&str, &str, u16, u16)| {
    let (html, query, offset, max) = input;
    let max = usize::from(max).max(1);
    let text = clean(html);
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
