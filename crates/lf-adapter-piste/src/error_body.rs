//! Lecture du corps d'erreur de PISTE (400 et 404) : message, marqueur « introuvable », détail
//! montrable et détection d'une page HTML (04 §11.2, §11.3).
//!
//! Un détail d'erreur PISTE est du **texte du corps** : long, avec des caractères de contrôle,
//! et susceptible de recopier la requête, donc un jeton ou des données personnelles. Aucune
//! fonction de ce module ne journalise. [`message`] rend le texte **brut**, réservé à
//! [`is_not_found_marker`] et à [`clean_detail`] ; seul [`clean_detail`] produit un texte qu'on
//! peut montrer ou journaliser.

use crate::dto::decode::decode;
use crate::dto::error::PisteErrorDto;
use crate::encoding::decode_body;

/// Plus gros corps d'erreur lu (64 Kio exactement compris) : un message d'erreur tient en
/// quelques centaines d'octets, le plafond général de la réponse (16 Mio) serait démesuré. Au-delà,
/// le corps n'est pas lu et le statut seul classe la réponse.
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Longueur maximale, en caractères, d'un détail montrable.
const MAX_DETAIL_CHARS: usize = 200;

/// Remplaçant d'un secret retiré du détail.
const REDACTED: &str = "[redacted]";

/// Fragments (en minuscules) qui désignent un identifiant inconnu dans le message d'un 400.
///
/// Un seul élément : `valider est fausse` couvre « à valider est fausse » comme « a valider est
/// fausse ». Le tableau permet d'en ajouter un sans changer la fonction.
const NOT_FOUND_MARKERS: [&str; 1] = ["valider est fausse"];

/// Texte **brut** du champ `message` d'un corps d'erreur PISTE.
///
/// Rend `None` pour un corps trop gros (plus de 64 Kio), qui n'est pas de l'UTF-8, qui n'est pas
/// un objet JSON, ou dont `message` manque, est `null` ou n'est pas une chaîne : la lecture est
/// facultative et le statut seul classe alors la réponse. Le mojibake est réparé si `repair`.
///
/// Le texte rendu n'est ni nettoyé ni expurgé : il ne sert qu'à [`is_not_found_marker`] et à
/// [`clean_detail`], et ne se journalise ni ne se montre avant ce dernier. L'erreur de décodage
/// du DTO est écartée sans être lue : son détail ne vaut rien de plus que `None`.
pub(crate) fn message(body: &[u8], endpoint: &str, repair: bool) -> Option<String> {
    if body.len() > MAX_ERROR_BODY_BYTES {
        return None;
    }
    let decoded = decode_body(body, endpoint, repair).ok()?;
    decode::<PisteErrorDto>(&decoded.text, endpoint).ok()?.message
}

/// Vrai si le message brut désigne un identifiant inconnu (« L'expression à valider est
/// fausse. »), sans tenir compte de la casse. Le test porte sur le texte complet, avant tout
/// nettoyage.
pub(crate) fn is_not_found_marker(message: &str) -> bool {
    let lowered = message.to_lowercase();
    NOT_FOUND_MARKERS.iter().any(|marker| lowered.contains(marker))
}

/// Détail montrable d'un message brut : nettoyer, **puis** retirer les secrets, **puis** couper.
///
/// L'ordre est imposé : (1) supprimer les caractères invisibles de mise en forme, (2) changer les
/// caractères de contrôle en espaces, (3) replier les espaces, (4) `redact` des secrets, (5)
/// couper à 200 caractères, (6) retirer l'espace finale laissée par la coupure.
///
/// - Le retrait suit le nettoyage : un jeton coupé par un caractère invisible (`tok-01\u{200B}23`)
///   n'a aucune occurrence exacte avant le nettoyage, qui le reconstitue.
/// - Le retrait précède la coupure : un secret n'est jamais coupé à cheval sur 200 caractères, ce
///   qui en laisserait un morceau.
///
/// Limite : seules les occurrences **exactes après nettoyage** sont retirées. Une valeur dérivée
/// (encodée en URL ou en base64, tronquée, d'une autre casse) ou coupée par un caractère de
/// contrôle n'est pas reconnue ; et un secret qui contient lui-même une espace ou un caractère de
/// contrôle n'est plus retrouvé, puisque le texte a été replié.
pub(crate) fn clean_detail(message: &str, secrets: &[&str]) -> String {
    let visible: String = message
        .chars()
        .filter(|&c| !is_invisible(c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let folded = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    let redacted = redact(&folded, secrets);
    let bounded: String = redacted.chars().take(MAX_DETAIL_CHARS).collect();
    bounded.trim_end().to_owned()
}

/// Remplace chaque occurrence exacte de chaque secret non vide par `[redacted]`.
fn redact(text: &str, secrets: &[&str]) -> String {
    secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .fold(text.to_owned(), |text, secret| text.replace(secret, REDACTED))
}

/// Caractère invisible de mise en forme (catégorie Cf) que `char::is_control` ignore : marques
/// et surcharges de sens d'écriture, espaces de largeur nulle, séparateurs invisibles, BOM.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Vrai si le premier octet non blanc du corps est `<` : balisage, `<?xml` ou `<!DOCTYPE`.
///
/// Plus large que « contient `<html` » : dans le doute, une page est du balisage et non la
/// réponse JSON de PISTE. Un BOM devant le balisage n'est pas reconnu.
pub(crate) fn looks_like_html(body: &[u8]) -> bool {
    body.trim_ascii_start().first() == Some(&b'<')
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "/consult/juri";
    const MARKER_BODY: &str = r#"{"timestamp":1,"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null,"exceptionDetails":null}"#;

    fn read(body: &str) -> Option<String> {
        message(body.as_bytes(), ENDPOINT, true)
    }

    /// Le texte lu comme du latin-1 : le mojibake que PISTE a déjà servi.
    fn mojibake(text: &str) -> String {
        text.bytes().map(char::from).collect()
    }

    fn cleaned(message: &str) -> String {
        clean_detail(message, &[])
    }

    #[test]
    fn the_known_400_message_is_read() {
        assert_eq!(read(MARKER_BODY).as_deref(), Some("L'expression à valider est fausse."));
    }

    #[test]
    fn a_mojibake_message_is_repaired_when_asked_and_left_otherwise() {
        let body = mojibake(MARKER_BODY);
        assert_eq!(
            message(body.as_bytes(), ENDPOINT, true).as_deref(),
            Some("L'expression à valider est fausse.")
        );
        assert_eq!(
            message(body.as_bytes(), ENDPOINT, false),
            Some(mojibake("L'expression à valider est fausse."))
        );
    }

    #[test]
    fn a_body_without_a_usable_message_gives_none() {
        for body in [
            "",
            "   ",
            "<html><body>Not Found</body></html>",
            "plain text",
            "null",
            "[]",
            "{}",
            r#"{"error":400}"#,
            r#"{"message":null}"#,
            r#"{"message":12}"#,
            r#"{"message":["a"]}"#,
            r#"{"message":"cut"#,
            r#"{"message":"ok"} trailing"#,
        ] {
            assert_eq!(read(body), None, "{body}");
        }
    }

    #[test]
    fn an_invalid_utf8_body_gives_none() {
        let mut body = br#"{"message":"a"#.to_vec();
        body.push(0xff);
        body.extend_from_slice(br#"b"}"#);
        assert_eq!(message(&body, ENDPOINT, true), None);
    }

    #[test]
    fn a_body_over_64_kib_is_not_read_but_one_of_exactly_64_kib_is() {
        let framing = r#"{"message":""}"#.len();
        let filler = "a".repeat(64 * 1024 - framing);
        let exact = format!(r#"{{"message":"{filler}"}}"#);
        assert_eq!(exact.len(), 64 * 1024);
        assert_eq!(read(&exact), Some(filler.clone()));
        let over = format!(r#"{{"message":"{filler}a"}}"#);
        assert_eq!(over.len(), 64 * 1024 + 1);
        assert_eq!(read(&over), None);
    }

    #[test]
    fn the_marker_is_found_whatever_the_case_and_the_accent() {
        for text in [
            "L'expression à valider est fausse.",
            "L'EXPRESSION À VALIDER EST FAUSSE.",
            "expression a valider est fausse",
            "Valider Est Fausse",
            "Ã\u{a0} valider est fausse",
        ] {
            assert!(is_not_found_marker(text), "{text}");
        }
    }

    #[test]
    fn other_messages_are_not_the_marker() {
        for text in [
            "",
            "Bad Request",
            "valider",
            "invalid id",
            "à valider",
            "a valider",
            "il faut a valider ceci",
            "est fausse",
        ] {
            assert!(!is_not_found_marker(text), "{text}");
        }
    }

    #[test]
    fn plain_text_is_kept() {
        assert_eq!(cleaned("Unknown facet DATE_X"), "Unknown facet DATE_X");
    }

    #[test]
    fn control_characters_become_spaces_and_spaces_are_folded() {
        assert_eq!(
            cleaned("a\nb\r\nc\td\u{0}e\u{1b}[31mf\u{7f}g\u{85}h\u{9f}i"),
            "a b c d e [31mf g h i"
        );
        assert_eq!(cleaned("  a \u{a0}\u{a0} b  "), "a b");
    }

    #[test]
    fn invisible_formatting_characters_are_dropped() {
        for c in [
            '\u{200b}', '\u{200f}', '\u{202a}', '\u{202e}', '\u{2060}', '\u{2064}', '\u{2066}',
            '\u{2069}', '\u{feff}',
        ] {
            assert_eq!(cleaned(&format!("a{c}b")), "ab", "{c:?}");
        }
        // Les bornes des plages : juste en dehors, le caractère reste (ou, pour une espace,
        // est replié).
        for (c, expected) in [
            ('\u{200a}', "a b"),
            ('\u{2010}', "a\u{2010}b"),
            ('\u{2029}', "a b"),
            ('\u{205f}', "a b"),
            ('\u{2065}', "a\u{2065}b"),
            ('\u{206a}', "a\u{206a}b"),
        ] {
            assert_eq!(cleaned(&format!("a{c}b")), expected, "{c:?}");
        }
    }

    #[test]
    fn the_detail_is_cut_at_200_characters_on_a_character_boundary() {
        for (count, kept) in [(199, 199), (200, 200), (201, 200), (5_000, 200)] {
            let detail = cleaned(&"é".repeat(count));
            assert_eq!((detail.chars().count(), detail.len()), (kept, kept * 2), "{count}");
        }
    }

    #[test]
    fn a_cut_that_ends_on_a_space_is_trimmed() {
        let message = format!("{} tail", "a".repeat(199));
        assert_eq!(cleaned(&message), "a".repeat(199));
    }

    #[test]
    fn known_secrets_are_replaced_everywhere_in_the_detail() {
        assert_eq!(
            clean_detail("bad Bearer tok-123 for sec-456, again tok-123", &["tok-123", "sec-456"]),
            "bad Bearer [redacted] for [redacted], again [redacted]"
        );
    }

    #[test]
    fn an_empty_secret_redacts_nothing() {
        assert_eq!(clean_detail("abc", &["", "b"]), "a[redacted]c");
        assert_eq!(clean_detail("abc", &[""]), "abc");
    }

    #[test]
    fn a_token_split_by_a_zero_width_space_is_still_redacted() {
        assert_eq!(
            clean_detail("Bearer t\u{2060}ok-01\u{200B}2345\u{FEFF}6789", &["tok-0123456789"]),
            "Bearer [redacted]"
        );
    }

    #[test]
    fn nothing_visible_gives_an_empty_detail() {
        assert_eq!(cleaned(""), "");
        assert_eq!(cleaned(" \n\t\u{0}\u{200B} "), "");
    }

    #[test]
    fn markup_is_html_and_the_rest_is_not() {
        for body in
            ["<!DOCTYPE html><html></html>", "  \r\n\t<html>", "<?xml version=\"1.0\"?>", "<"]
        {
            assert!(looks_like_html(body.as_bytes()), "{body:?}");
        }
        for body in ["", "   ", r#"{"message":"<b>"}"#, "Not Found <html>", "\u{feff}<html>"] {
            assert!(!looks_like_html(body.as_bytes()), "{body:?}");
        }
    }
}
