//! Extrait cité qui jouxte une citation (annexe B §7.9).
//!
//! Un extrait est un texte entre guillemets (`« … »`, `“ … ”`, `„ … “`, `"…"`) qui commence au
//! plus [`MAX_GAP_CHARS`] caractères après la fin de la citation, ou qui finit au plus autant de
//! caractères avant son début. Seuls les espaces (NBSP et fines comprises), `:` et `,` peuvent
//! s'intercaler. L'application compare ensuite l'extrait au texte officiel (`fold_char`).

use crate::citation::types::Span;

/// Longueur minimale de l'extrait, en caractères, blancs de bord exclus.
const MIN_QUOTE_CHARS: usize = 10;

/// Longueur maximale de l'extrait, en caractères, blancs de bord exclus.
const MAX_QUOTE_CHARS: usize = 2_000;

/// Écart maximal entre la citation et le guillemet, en caractères.
const MAX_GAP_CHARS: usize = 3;

/// Nombre de caractères parcourus au plus pour trouver l'autre guillemet : au-delà, l'extrait
/// dépasse de toute façon [`MAX_QUOTE_CHARS`] et la recherche reste bornée.
const SCAN_LIMIT_CHARS: usize = 4_096;

/// Guillemets ouvrants et fermants qui leur répondent. `„` (allemand) se ferme par `“` ou `”`.
const PAIRS: [(char, &[char]); 4] =
    [('«', &['»']), ('“', &['”']), ('„', &['“', '”']), ('"', &['"'])];

/// Span de l'extrait entre guillemets qui précède ou suit la citation de `span`, en octets de
/// `input`. `None` s'il n'y en a pas.
///
/// Le span rendu couvre l'extrait **sans** ses guillemets ni les blancs qui les bordent, puisque
/// c'est ce texte que l'application compare à l'article ou à la décision lus. L'extrait qui suit
/// la citation l'emporte sur celui qui la précède ; si le premier est refusé (trop court, trop
/// long, mal fermé), le second est essayé. Un `span` qui ne désigne pas un morceau de `input`
/// donne `None`.
pub fn adjacent_quote(input: &str, span: Span) -> Option<Span> {
    input.get(span.start..span.end)?;
    following(input, span.end).or_else(|| preceding(input, span.start))
}

/// Les guillemets qui ferment `open`, si c'en est un ouvrant.
fn closers_of(open: char) -> Option<&'static [char]> {
    PAIRS.iter().find(|(o, _)| *o == open).map(|(_, closers)| *closers)
}

/// `open` ouvre-t-il un extrait que `close` ferme ?
fn pairs_with(open: char, close: char) -> bool {
    closers_of(open).is_some_and(|closers| closers.contains(&close))
}

/// `c` ferme-t-il au moins une famille de guillemets ?
fn is_closer(c: char) -> bool {
    PAIRS.iter().any(|(_, closers)| closers.contains(&c))
}

/// Séparateur admis entre la citation et l'extrait : un blanc, `:` ou `,`.
fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, ':' | ',')
}

/// Le `"` à l'octet `at` est-il le premier d'une paire ? Les guillemets droits ouvrent et
/// ferment : on les apparie par rang (un `"` de rang pair ouvre, de rang impair ferme).
fn is_straight_opener(input: &str, at: usize) -> bool {
    input.get(..at).is_some_and(|head| head.matches('"').count() % 2 == 0)
}

/// Extrait qui commence au plus [`MAX_GAP_CHARS`] caractères après l'octet `from`, qui est une
/// frontière de caractère de `input` (le span a été validé par l'appelant).
fn following(input: &str, from: usize) -> Option<Span> {
    let rest = input.get(from..).unwrap_or_default();
    for (offset, c) in rest.char_indices().take(MAX_GAP_CHARS + 1) {
        if let Some(closers) = closers_of(c) {
            let at = from + offset;
            let opens = c != '"' || is_straight_opener(input, at);
            return if opens { quoted_after(input, at + c.len_utf8(), c, closers) } else { None };
        }
        if !is_separator(c) {
            return None;
        }
    }
    None
}

/// Extrait qui finit au plus [`MAX_GAP_CHARS`] caractères avant l'octet `until`, qui est une
/// frontière de caractère de `input` (le span a été validé par l'appelant).
fn preceding(input: &str, until: usize) -> Option<Span> {
    let head = input.get(..until).unwrap_or_default();
    for (offset, c) in head.char_indices().rev().take(MAX_GAP_CHARS + 1) {
        if is_closer(c) {
            let closes = c != '"' || !is_straight_opener(input, offset);
            return if closes { quoted_before(input, offset, c) } else { None };
        }
        if !is_separator(c) {
            return None;
        }
    }
    None
}

/// Extrait qui commence à l'octet `start` (juste après `opener`), jusqu'au premier des `closers`.
fn quoted_after(input: &str, start: usize, opener: char, closers: &[char]) -> Option<Span> {
    let rest = input.get(start..).unwrap_or_default();
    let mut end = None;
    for (offset, c) in rest.char_indices().take(SCAN_LIMIT_CHARS) {
        if closers.contains(&c) {
            end = Some(start + offset);
            break;
        }
        if c == opener {
            return None;
        }
    }
    inner_span(input, start, end?)
}

/// Extrait fermé par `closer` à l'octet `at`, depuis le premier guillemet ouvrant qui le précède.
fn quoted_before(input: &str, at: usize, closer: char) -> Option<Span> {
    let head = input.get(..at).unwrap_or_default();
    let mut start = None;
    for (offset, c) in head.char_indices().rev().take(SCAN_LIMIT_CHARS) {
        if pairs_with(c, closer) {
            start = Some(offset + c.len_utf8());
            break;
        }
        if c == closer {
            return None;
        }
    }
    inner_span(input, start?, at)
}

/// Span de `input[start..end]` sans ses blancs de bord, si l'extrait a la bonne longueur.
fn inner_span(input: &str, start: usize, end: usize) -> Option<Span> {
    let inner = input.get(start..end).unwrap_or_default();
    let trimmed = inner.trim();
    if !(MIN_QUOTE_CHARS..=MAX_QUOTE_CHARS).contains(&trimmed.chars().count()) {
        return None;
    }
    let lead = inner.len() - inner.trim_start().len();
    Some(Span { start: start + lead, end: start + lead + trimmed.len() })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    use super::*;

    /// Span de la première occurrence de `needle` dans `input`.
    fn span_of(input: &str, needle: &str) -> Span {
        let start = input.find(needle).expect("aiguille présente");
        Span { start, end: start + needle.len() }
    }

    /// Texte de l'extrait détecté autour de `cite`, ou `None`.
    fn quote_of<'a>(input: &'a str, cite: &str) -> Option<&'a str> {
        adjacent_quote(input, span_of(input, cite)).and_then(|s| input.get(s.start..s.end))
    }

    const PRIVACY: &str = "Chacun a droit au respect de sa vie privée";

    #[test]
    fn adjacent_quote_detects_guillemets() {
        // Exemple de 05 §5.9 : l'extrait suit la citation, séparé par « , ».
        let text =
            "Selon l'article 9 du code civil, « Chacun a droit au respect de sa vie privée »";
        assert_eq!(quote_of(text, "article 9 du code civil"), Some(PRIVACY));

        // Séparateurs admis : espaces, « : » et « , » (3 caractères au plus).
        for gap in [" ", ": ", " : ", ",", ", ", "", "\u{a0}:\u{a0}", "\u{202f}"] {
            let text = format!("article 9 du code civil{gap}« {PRIVACY} » fin");
            assert_eq!(quote_of(&text, "article 9 du code civil"), Some(PRIVACY), "{gap:?}");
        }

        // L'extrait précède la citation : il finit au plus 3 caractères avant son début.
        for gap in [" ", ", ", " : ", ""] {
            let text = format!("« {PRIVACY} »{gap}article 9 du code civil");
            assert_eq!(quote_of(&text, "article 9 du code civil"), Some(PRIVACY), "{gap:?}");
        }
    }

    #[test]
    fn the_span_excludes_guillemets_and_inner_blanks() {
        let text = "art. 9 C. civ. :\u{a0}«\u{a0}Chacun a droit au respect\u{202f}» fin";
        let span = adjacent_quote(text, span_of(text, "art. 9 C. civ.")).expect("extrait");
        assert_eq!(text.get(span.start..span.end), Some("Chacun a droit au respect"));
        // Les offsets sont des octets de `input` : ils tombent sur des frontières de caractère.
        assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
    }

    #[test]
    fn every_quote_style_is_recognised() {
        let styles = [('«', '»'), ('“', '”'), ('„', '“'), ('„', '”'), ('"', '"')];
        for (open, close) in styles {
            let after = format!("article 9 du code civil : {open}{PRIVACY}{close}");
            assert_eq!(quote_of(&after, "article 9 du code civil"), Some(PRIVACY), "{open}{close}");
            let before = format!("{open}{PRIVACY}{close}, article 9 du code civil");
            assert_eq!(
                quote_of(&before, "article 9 du code civil"),
                Some(PRIVACY),
                "{open}{close}"
            );
        }
    }

    #[test]
    fn quote_length_is_between_10_and_2000_characters() {
        let cite = "article 9 du code civil";
        for (len, expected) in [(9, false), (10, true), (2_000, true), (2_001, false)] {
            let body = "é".repeat(len);
            let text = format!("{cite} : « {body} »");
            assert_eq!(quote_of(&text, cite).is_some(), expected, "{len} caractères");
            let text = format!("« {body} » : {cite}");
            assert_eq!(quote_of(&text, cite).is_some(), expected, "{len} caractères, avant");
        }
        // La longueur est celle de l'extrait, blancs de bord exclus.
        let text = format!("{cite} : «          x          »");
        assert_eq!(quote_of(&text, cite), None);
    }

    #[test]
    fn the_quote_must_be_within_three_characters_of_the_citation() {
        let cite = "article 9 du code civil";
        // Quatre caractères d'écart : trop loin, avant comme après.
        assert_eq!(quote_of(&format!("{cite}  :  « {PRIVACY} »"), cite), None);
        assert_eq!(quote_of(&format!("« {PRIVACY} »  :  {cite}"), cite), None);
        // Un caractère qui n'est pas un séparateur coupe le lien.
        assert_eq!(quote_of(&format!("{cite}. « {PRIVACY} »"), cite), None);
        assert_eq!(quote_of(&format!("{cite} et « {PRIVACY} »"), cite), None);
        assert_eq!(quote_of(&format!("« {PRIVACY} » et {cite}"), cite), None);
        assert_eq!(quote_of(&format!("{cite} dispose « {PRIVACY} »"), cite), None);
    }

    #[test]
    fn unbalanced_or_mismatched_guillemets_are_not_a_quote() {
        let cite = "article 9 du code civil";
        // Pas de guillemet fermant.
        assert_eq!(quote_of(&format!("{cite} : « {PRIVACY}"), cite), None);
        assert_eq!(quote_of(&format!("{PRIVACY} » : {cite}"), cite), None);
        // Fermant d'une autre famille.
        assert_eq!(quote_of(&format!("{cite} : « {PRIVACY} ”"), cite), None);
        assert_eq!(quote_of(&format!("“ {PRIVACY} » : {cite}"), cite), None);
        // Ouvrant qui revient avant le fermant (imbriqué ou mal fermé).
        assert_eq!(quote_of(&format!("{cite} : « {PRIVACY} « autre » ensuite »"), cite), None);
        assert_eq!(quote_of(&format!("« autre » {PRIVACY} » : {cite}"), cite), None);
        // Un fermant isolé après la citation n'ouvre rien.
        assert_eq!(quote_of(&format!("{cite} » {PRIVACY} »"), cite), None);
        // Un ouvrant isolé avant la citation ne ferme rien.
        assert_eq!(quote_of(&format!("« {PRIVACY} « : {cite}"), cite), None);
    }

    #[test]
    fn ascii_quotes_are_paired_by_position() {
        let cite = "article 9 du code civil";
        // La citation est elle-même entre guillemets : le `"` qui la suit la ferme, il n'ouvre rien.
        let inside = format!("il écrit \"voir {cite}\" puis \"{PRIVACY}\"");
        assert_eq!(quote_of(&inside, cite), None);
        // Même chose avant la citation : le `"` qui la précède l'ouvre, il ne ferme rien.
        let inside = format!("il écrit \"{cite} fin de phrase\"");
        assert_eq!(quote_of(&inside, cite), None);
        let inside = format!("\"{PRIVACY} {cite} fin de phrase\"");
        assert_eq!(quote_of(&inside, cite), None);
        // Un extrait ASCII complet qui suit la citation reste reconnu après d'autres guillemets.
        let text = format!("\"a\" et \"b\" {cite} : \"{PRIVACY}\"");
        assert_eq!(quote_of(&text, cite), Some(PRIVACY));
        let text = format!("\"{PRIVACY}\" {cite} et \"b\"");
        assert_eq!(quote_of(&text, cite), Some(PRIVACY));
    }

    #[test]
    fn the_following_quote_wins_and_the_preceding_one_is_the_fallback() {
        let cite = "article 9 du code civil";
        let both = format!("« premier extrait assez long » {cite} : « {PRIVACY} »");
        assert_eq!(quote_of(&both, cite), Some(PRIVACY));
        // Extrait suivant trop court : on se rabat sur l'extrait qui précède.
        let short = format!("« premier extrait assez long » {cite} : « court »");
        assert_eq!(quote_of(&short, cite), Some("premier extrait assez long"));
        // Aucun des deux.
        assert_eq!(quote_of(&format!("« a » {cite} : « b »"), cite), None);
        assert_eq!(quote_of(cite, cite), None);
    }

    #[test]
    fn the_closing_guillemet_must_be_found_within_the_scan_limit() {
        let cite = "article 9 du code civil";
        let far = format!("{cite} : « {} »", "x".repeat(5_000));
        assert_eq!(quote_of(&far, cite), None);
        let far = format!("« {} » : {cite}", "x".repeat(5_000));
        assert_eq!(quote_of(&far, cite), None);
    }

    #[test]
    fn an_invalid_span_gives_no_quote() {
        let text = "article 9 : « Chacun a droit au respect de sa vie privée »";
        let valid = span_of(text, "article 9");
        assert!(adjacent_quote(text, valid).is_some());
        // Hors du texte, inversé, hors frontière de caractère.
        assert_eq!(adjacent_quote(text, Span { start: 0, end: text.len() + 1 }), None);
        assert_eq!(adjacent_quote(text, Span { start: text.len() + 5, end: text.len() + 9 }), None);
        assert_eq!(adjacent_quote(text, Span { start: 5, end: 2 }), None);
        let accents = "é « Chacun a droit au respect »";
        assert_eq!(adjacent_quote(accents, Span { start: 1, end: 2 }), None);
        assert_eq!(adjacent_quote("", Span { start: 0, end: 0 }), None);
    }

    proptest! {
        /// Aucune entrée ne fait paniquer la fonction, et un extrait rendu est un morceau valide de
        /// `input`, hors de la citation, de 10 à 2 000 caractères.
        #[test]
        fn adjacent_quote_never_panics_and_stays_in_bounds(
            input in "[a-zé« »“”„\" :,.]{0,80}",
            a in 0usize..120,
            b in 0usize..120,
        ) {
            let span = Span { start: a.min(b), end: a.max(b) };
            if let Some(quote) = adjacent_quote(&input, span) {
                let text = input.get(quote.start..quote.end).expect("frontières valides");
                prop_assert!((10..=2_000).contains(&text.chars().count()));
                prop_assert_eq!(text.trim(), text);
                prop_assert!(quote.end <= span.start || quote.start >= span.end);
            }
        }

        /// Un extrait de 10 caractères au moins, collé à la citation par un séparateur admis, est
        /// toujours retrouvé tel quel, avant comme après.
        #[test]
        fn a_well_formed_quote_is_always_found(
            body in "[a-zA-Z][a-zA-Z ,.]{8,60}[a-zA-Z]",
            gap in prop::sample::select(vec!["", " ", ": ", ", ", " : "]),
            before in any::<bool>(),
        ) {
            let cite = "art. 9 C. civ.";
            let text = if before {
                format!("« {body} »{gap}{cite} fin")
            } else {
                format!("début {cite}{gap}« {body} »")
            };
            prop_assert_eq!(quote_of(&text, cite), Some(body.as_str()));
        }
    }
}
