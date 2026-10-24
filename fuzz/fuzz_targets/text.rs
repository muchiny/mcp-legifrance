#![no_main]

use std::borrow::Cow;

use lf_domain::text::{
    clean, clean_with_report,
    encoding::{has_latin1_mojibake, repair_latin1_mojibake},
    passages, rank, window,
};
use libfuzzer_sys::fuzz_target;

/// Caractères que `clean` retire (03 §7.3), écrits ici indépendamment du domaine : contrôles
/// C0 et C1 (et U+007F) sauf `\t`, `\n` et `\r`, et les 17 plages de la propriété Unicode
/// `Default_Ignorable_Code_Point` (format invisible, bidi, sélecteurs de variante,
/// remplisseurs, tags Unicode).
fn invisible(c: char) -> bool {
    let control = c.is_control() && !matches!(c, '\t' | '\n' | '\r');
    let format = matches!(
        c,
        '\u{ad}'
            | '\u{34f}'
            | '\u{61c}'
            | '\u{115f}'..='\u{1160}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180b}'..='\u{180f}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2060}'..='\u{206f}'
            | '\u{3164}'
            | '\u{feff}'
            | '\u{ffa0}'
            | '\u{fff0}'..='\u{fff8}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
    );
    let bidi = matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let variation = matches!(c, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}');
    let tag = matches!(c, '\u{e0000}'..='\u{e0fff}');
    control || format || bidi || variation || tag
}

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
    let (text, report) = clean_with_report(&repaired);
    assert_eq!(clean(&repaired), text);
    // Aucun caractère invisible en sortie ; `\t` et `\r` sont repliés en espaces.
    assert!(text.chars().all(|c| !invisible(c) && c != '\t' && c != '\r'));
    // Chaque caractère retiré vient d'un caractère de la source ou d'une entité.
    let removed = usize::try_from(report.invisible_removed).unwrap_or(usize::MAX);
    assert!(removed <= repaired.chars().count());
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
