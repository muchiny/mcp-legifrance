//! Champ court d'une fiche de Service-Public (titre, résumé, intitulé) rendu sûr sur une ligne.
//!
//! Le lecteur XML de l'adapter a déjà décodé les entités : rien n'est décodé une seconde fois
//! (une fiche qui écrit `&amp;amp;` affiche `&amp;`).

use crate::text::{flatten_whitespace, strip_invisible};

/// Longueur maximale d'un champ court, en caractères. Relevé du 2026-11-23 : 516 caractères au
/// plus pour un titre de référence.
pub const LINE_MAX_CHARS: usize = 1_000;

/// `raw` sans caractère invisible, blancs repliés en une espace et bords retirés,
/// [`LINE_MAX_CHARS`] caractères au plus (`…` ensuite).
pub(crate) fn clean_line(raw: &str) -> String {
    let (visible, _) = strip_invisible(raw);
    let flat = flatten_whitespace(&visible);
    let mut chars = flat.chars();
    let head: String = chars.by_ref().take(LINE_MAX_CHARS).collect();
    if chars.next().is_some() { head + "…" } else { head }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_line_is_flat_visible_and_bounded() {
        assert_eq!(
            clean_line("  Code du\ttravail :\n article\u{a0}L3121-1 "),
            "Code du travail : article L3121-1"
        );
        assert_eq!(clean_line("Co\u{200b}de\u{202e}"), "Code");
        assert_eq!(clean_line("&amp;"), "&amp;");
        assert_eq!(clean_line(" \u{200b} "), "");
        let long = "é".repeat(LINE_MAX_CHARS);
        assert_eq!(clean_line(&long), long);
        assert_eq!(clean_line(&format!("{long}x")), format!("{long}…"));
    }
}
