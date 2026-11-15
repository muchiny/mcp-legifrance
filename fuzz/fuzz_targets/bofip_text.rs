#![no_main]

use lf_domain::{
    doctrine::{clean_text, publication_text},
    text::LongText,
};
use libfuzzer_sys::fuzz_target;

/// Lettres et chiffres de `text`, comptés paragraphe par paragraphe. Écrit ici indépendamment
/// du domaine, pour contrôler sa garde (ADR-0030, décision 4).
fn letters_and_digits(text: &LongText) -> usize {
    text.paragraphs()
        .map(|(_, paragraph)| paragraph.chars().filter(|c| c.is_alphabetic() || c.is_numeric()).count())
        .sum()
}

// Entrée : le champ HTML d'une publication du BOFiP, un octet nul, puis son champ brut (sans
// octet nul : tout est le champ HTML). Les graines de fuzz/seeds/bofip_text ont cette forme.
fuzz_target!(|data: &str| {
    let (html, plain) = data.split_once('\0').unwrap_or((data, ""));
    // Jamais de panique, et jamais d'erreur à cette taille (la limite est de 16 000 000).
    let read = publication_text(Some(html), Some(plain)).unwrap();
    let (plain_text, unknown) = clean_text(plain).unwrap();

    // Le texte servi est un texte canonique : paragraphes non vides, joints par une ligne vide.
    let paragraphs: Vec<&str> = read.text.paragraphs().map(|(_, paragraph)| paragraph).collect();
    assert_eq!(read.text.as_str(), paragraphs.join("\n\n"));
    assert_eq!(usize::try_from(read.text.char_len()).unwrap(), read.text.as_str().chars().count());
    for paragraph in &paragraphs {
        assert!(!paragraph.is_empty() && !paragraph.contains('\n'));
    }

    // C'est l'une des deux lectures. Le champ brut n'est servi que sans champ HTML, ou sur un
    // repli signalé ; le champ HTML n'est servi que s'il tient la garde des neuf dixièmes.
    if read.fell_back || html.trim().is_empty() {
        assert_eq!((&read.text, read.unknown_entities), (&plain_text, unknown));
        assert_eq!(read.fell_back, !html.trim().is_empty());
    } else {
        assert_eq!(read.unknown_entities, 0);
        assert!(letters_and_digits(&read.text) * 10 >= letters_and_digits(&plain_text) * 9);
    }

    // Les guillemets doublés par le portail : doubler ceux d'un champ puis le lire rend le texte
    // du champ d'origine. (Sans balise `html` : la coupe des restes de document est mise à
    // part. Le texte seul est comparé : un champ blanc n'est pas lu, ses caractères invisibles
    // ne sont donc pas comptés.)
    if !html.to_ascii_lowercase().contains("<html") {
        let doubled = html.replace('"', "\"\"");
        let read = publication_text(Some(&doubled), None).unwrap();
        assert_eq!(read.text.as_str(), LongText::from_html(html).unwrap().as_str());
        assert!(!read.fell_back);
    }
});
