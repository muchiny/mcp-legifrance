#![no_main]

use lf_domain::text::LongText;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // Jamais de panique, et jamais d'erreur à cette taille (la limite est de 16 000 000).
    let text = LongText::from_html(data).unwrap();
    let paragraphs: Vec<&str> = text.paragraphs().map(|(_, paragraph)| paragraph).collect();
    // Le texte canonique est exactement les paragraphes joints par une ligne vide.
    assert_eq!(text.as_str(), paragraphs.join("\n\n"));
    assert_eq!(usize::try_from(text.char_len()).unwrap(), text.as_str().chars().count());
    for paragraph in paragraphs {
        assert!(!paragraph.is_empty() && !paragraph.contains('\n'));
    }
});
