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
use crate::map::endpoint::ACCO;

/// Plus gros corps d'erreur **décodé** (64 Kio exactement compris) : un message d'erreur tient en
/// quelques centaines d'octets, le plafond général de la réponse (16 Mio) serait démesuré. Au-delà,
/// le corps n'est pas décodé et le statut seul classe la réponse. La limite ne borne pas la
/// lecture réseau : `http::read_body` a déjà lu le corps, d'au plus `max_body_bytes` octets
/// (16 Mio par défaut), comme pour toute réponse.
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

/// Fragment (en minuscules) du 400 d'un identifiant ACCO inexistant : « Le nombre de résultat
/// retourné par le moteur de recherche n'est pas égal à 1. » (sandbox, 2026-10-28, V-68).
///
/// Le fragment est sans accent, pour survivre à un mojibake non réparé comme `valider est
/// fausse`. « Pas égal à 1 » peut dire zéro comme plusieurs ; pour une recherche par identifiant
/// il ne peut dire que zéro, les identifiants étant uniques. Ce marqueur n'est donc admis que sur
/// `/consult/acco` (`crate::map::endpoint::ACCO`) : ailleurs, un résultat multiple resterait
/// possible et le message ne prouverait pas l'absence.
const ACCO_COUNT_MARKER: &str = "moteur de recherche n'est pas";

/// Queue distinctive du même message, cherchée **après** [`ACCO_COUNT_MARKER`] : « égal à 1 »
/// réduit à `gal` et `1`, sans accent pour survivre à un mojibake non réparé. Elle écarte un
/// message voisin (« le moteur de recherche n'est pas disponible ») qui ne dit rien de l'absence.
const ACCO_COUNT_TAIL: [&str; 2] = ["gal", "1"];

/// Texte **brut** du champ `message` d'un corps d'erreur PISTE.
///
/// Rend `None` pour un corps trop gros pour être décodé (plus de 64 Kio), qui n'est pas de
/// l'UTF-8, qui n'est pas un objet JSON, ou dont `message` manque, est `null` ou n'est pas une
/// chaîne : la lecture est facultative et le statut seul classe alors la réponse. Le mojibake est
/// réparé si `repair`.
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

/// Vrai si le message brut d'un 400 de **consultation** désigne un identifiant inconnu : le
/// marqueur général ([`is_not_found_marker`]) sur tout endpoint, et en plus le message de
/// comptage d'ACCO sur `/consult/acco` seulement.
///
/// L'appelant ne passe un message que pour une consultation ; `/search` n'est jamais lue, donc
/// jamais « introuvable » (04 §11.2 règle 2, décision R7).
pub(crate) fn is_not_found(endpoint: &str, message: &str) -> bool {
    is_not_found_marker(message)
        || (endpoint == ACCO && is_acco_count(message))
}

/// Vrai si le message brut est le comptage d'ACCO : le marqueur, puis sa queue distinctive.
fn is_acco_count(message: &str) -> bool {
    let lowered = message.to_lowercase();
    lowered
        .split_once(ACCO_COUNT_MARKER)
        .is_some_and(|(_, tail)| ACCO_COUNT_TAIL.iter().all(|part| tail.contains(part)))
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
///
/// Les secrets sont retirés du plus long au plus court : un secret contenu dans un autre (un
/// jeton qui est une sous-chaîne du secret client, ou l'inverse) n'en laisse ainsi aucun morceau.
pub(crate) fn redact(text: &str, secrets: &[&str]) -> String {
    let mut secrets: Vec<&str> =
        secrets.iter().copied().filter(|secret| !secret.is_empty()).collect();
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    secrets.into_iter().fold(text.to_owned(), |text, secret| text.replace(secret, REDACTED))
}

/// Point de code « ignorable par défaut » d'Unicode (propriété `Default_Ignorable_Code_Point`)
/// : rendu invisible, mais que `char::is_control` et les blancs ignorent. Y figurent les marques
/// et surcharges de sens d'écriture, les espaces de largeur nulle, le trait d'union conditionnel,
/// les remplisseurs, les sélecteurs de variante, le BOM et les étiquettes U+E0000 à U+E0FFF (qui
/// recopient l'ASCII sans qu'un humain les voie).
///
/// Les plages sont écrites en clair, sans crate : le jeu de la propriété est stable.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// Octets du BOM UTF-8.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Vrai si le premier octet non blanc du corps est `<` : balisage, `<?xml` ou `<!DOCTYPE`.
///
/// Plus large que « contient `<html` » : dans le doute, une page est du balisage et non la
/// réponse JSON de PISTE. Tous les BOM UTF-8 de tête sont écartés avant le test (une page de
/// passerelle peut en porter un ou plusieurs) ; un BOM seul, ou suivi de JSON, n'est pas du
/// balisage.
pub(crate) fn looks_like_html(body: &[u8]) -> bool {
    let mut body = body;
    while let Some(rest) = body.strip_prefix(UTF8_BOM) {
        body = rest;
    }
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

    const ACCO_COUNT: &str =
        "Le nombre de résultat retourné par le moteur de recherche n'est pas égal à 1.";

    #[test]
    fn the_general_marker_is_not_found_on_every_consultation() {
        for endpoint in [ENDPOINT, ACCO, "/consult/cnil"] {
            assert!(is_not_found(endpoint, "L'expression à valider est fausse."), "{endpoint}");
        }
    }

    #[test]
    fn the_acco_count_message_is_not_found_on_the_acco_endpoint_only() {
        assert!(is_not_found(ACCO, ACCO_COUNT));
        assert!(is_not_found(ACCO, &ACCO_COUNT.to_uppercase()));
        assert!(is_not_found(ACCO, &mojibake(ACCO_COUNT)));
        for endpoint in [ENDPOINT, "/consult/cnil", "/search", "/consult/acco/", ""] {
            assert!(!is_not_found(endpoint, ACCO_COUNT), "{endpoint}");
        }
    }

    #[test]
    fn the_acco_count_marker_needs_its_distinctive_tail() {
        // Un message voisin du marqueur mais qui ne dit pas « égal à 1 » n'est pas une absence.
        for message in [
            "Le moteur de recherche n'est pas disponible",
            "Le moteur de recherche n'est pas égal",
            "Le moteur de recherche n'est pas à 1",
            "égal à 1, le moteur de recherche n'est pas",
        ] {
            assert!(!is_not_found(ACCO, message), "{message}");
        }
    }

    #[test]
    fn the_recorded_acco_messages_are_not_found() {
        // Les deux 400 enregistrés en sandbox (V-68), lus du fichier committé.
        for body in [
            include_str!("../tests/fixtures/recorded/consult/acco/inconnu/ACCOTEXT000000000000.json"),
            include_str!("../tests/fixtures/recorded/consult/acco/inconnu/ACCOTEXT999999999999.json"),
        ] {
            let text = message(body.as_bytes(), ACCO, true).unwrap();
            assert!(is_not_found(ACCO, &text), "{text}");
            let broken = message(body.as_bytes(), ACCO, false).unwrap();
            assert!(is_not_found(ACCO, &broken), "{broken}");
        }
    }

    #[test]
    fn other_messages_are_not_found_nowhere() {
        for message in [
            "",
            "Bad Request",
            "Une erreur est survenue dans le moteur de recherche",
            "recherche n'est pas égal à 1",
        ] {
            assert!(!is_not_found(ACCO, message), "{message}");
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

    /// Plages de la propriété Unicode `Default_Ignorable_Code_Point` : (premier, dernier).
    const IGNORABLE_RANGES: [(char, char); 17] = [
        ('\u{00AD}', '\u{00AD}'),
        ('\u{034F}', '\u{034F}'),
        ('\u{061C}', '\u{061C}'),
        ('\u{115F}', '\u{1160}'),
        ('\u{17B4}', '\u{17B5}'),
        ('\u{180B}', '\u{180F}'),
        ('\u{200B}', '\u{200F}'),
        ('\u{202A}', '\u{202E}'),
        ('\u{2060}', '\u{206F}'),
        ('\u{3164}', '\u{3164}'),
        ('\u{FE00}', '\u{FE0F}'),
        ('\u{FEFF}', '\u{FEFF}'),
        ('\u{FFA0}', '\u{FFA0}'),
        ('\u{FFF0}', '\u{FFF8}'),
        ('\u{1BCA0}', '\u{1BCA3}'),
        ('\u{1D173}', '\u{1D17A}'),
        ('\u{E0000}', '\u{E0FFF}'),
    ];

    fn before(c: char) -> char {
        char::from_u32(c as u32 - 1).unwrap()
    }

    fn after(c: char) -> char {
        char::from_u32(c as u32 + 1).unwrap()
    }

    #[test]
    fn invisible_formatting_characters_are_dropped() {
        // Chaque borne de chaque plage, et un point intérieur.
        for (first, last) in IGNORABLE_RANGES {
            let middle = char::from_u32(u32::midpoint(first as u32, last as u32)).unwrap();
            for c in [first, middle, last] {
                assert_eq!(cleaned(&format!("a{c}b")), "ab", "{c:?} in {first:?}..={last:?}");
            }
        }
    }

    #[test]
    fn the_characters_next_to_an_invisible_range_are_kept() {
        // Juste en dehors de chaque plage, le caractère reste (ou, pour une espace, est replié).
        for (first, last) in IGNORABLE_RANGES {
            for c in [before(first), after(last)] {
                let expected = if c.is_whitespace() { "a b".to_owned() } else { format!("a{c}b") };
                assert_eq!(cleaned(&format!("a{c}b")), expected, "{c:?} next to {first:?}");
            }
        }
    }

    #[test]
    fn invisible_characters_that_were_once_missed_are_dropped() {
        // Le trait d'union conditionnel, la marque de lettre arabe, U+206A..206F (dépréciés),
        // les sélecteurs de variante, le remplisseur hangul et une étiquette ASCII.
        for c in [
            '\u{00AD}',
            '\u{061C}',
            '\u{206A}',
            '\u{206F}',
            '\u{FE0F}',
            '\u{E0101}',
            '\u{3164}',
            '\u{E0041}',
        ] {
            assert_eq!(cleaned(&format!("a{c}b")), "ab", "{c:?}");
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
    fn a_token_split_by_any_default_ignorable_character_is_still_redacted() {
        for c in ['\u{00AD}', '\u{FE0F}', '\u{E0041}', '\u{3164}', '\u{206A}'] {
            let message = format!("Bearer tok-01{c}23{c}456789 refused");
            assert_eq!(
                clean_detail(&message, &["tok-0123456789"]),
                "Bearer [redacted] refused",
                "{c:?}"
            );
        }
    }

    #[test]
    fn a_secret_containing_another_is_fully_redacted() {
        // Le jeton est une sous-chaîne stricte du secret client, et inversement : quel que soit
        // l'ordre de la liste, il ne reste aucun morceau du plus long.
        for secrets in [["tok", "client-tok-secret"], ["client-tok-secret", "tok"]] {
            assert_eq!(
                clean_detail("bad client-tok-secret and tok, then client-tok-secret", &secrets),
                "bad [redacted] and [redacted], then [redacted]",
                "{secrets:?}"
            );
        }
        assert_eq!(clean_detail("a tok b", &["tok", "tok"]), "a [redacted] b");
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
        for body in ["", "   ", r#"{"message":"<b>"}"#, "Not Found <html>"] {
            assert!(!looks_like_html(body.as_bytes()), "{body:?}");
        }
    }

    #[test]
    fn markup_behind_a_utf8_bom_is_still_html() {
        for body in [
            "\u{feff}<html>",
            "\u{feff}  <html>",
            "\u{feff}\r\n<!DOCTYPE html>",
            "\u{feff}<",
            "\u{feff}\u{feff}<html>",
            "\u{feff}\u{feff}\u{feff} <",
        ] {
            assert!(looks_like_html(body.as_bytes()), "{body:?}");
        }
        // Un ou plusieurs BOM seuls, ou suivis de JSON, ne sont pas du balisage.
        for body in ["\u{feff}", "\u{feff}\u{feff}", "\u{feff}{}", "\u{feff}\u{feff}{}"] {
            assert!(!looks_like_html(body.as_bytes()), "{body:?}");
        }
    }
}
