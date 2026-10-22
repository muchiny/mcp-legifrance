//! Décodage du corps des réponses PISTE : UTF-8 strict, puis détection et réparation du
//! mojibake latin-1 (04 §4.4, §10).
//!
//! L'en-tête `charset` n'est jamais consulté : tout corps de 2xx doit être de l'UTF-8
//! valide. Un corps qui ne l'est pas est une erreur de décodage, jamais une substitution.
//! La fonction est pure : ni compteur ni journal (c'est le rôle de `stats`).

use std::borrow::Cow;

use lf_application::ports::CorpusError;
use lf_domain::text::encoding::{has_latin1_mojibake, repair_latin1_mojibake};

/// Corps décodé, avec l'état de la détection du mojibake.
///
/// Trois états : sain `(false, false)`, réparé `(true, true)`, détecté mais gardé tel
/// quel `(true, false)` (réparation coupée ou refusée par le domaine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedBody {
    /// Texte du corps, réparé seulement si `repaired`.
    pub text: String,
    /// Un motif de mojibake latin-1 a été vu dans le corps reçu.
    pub detected: bool,
    /// Le texte a été réparé (jamais vrai sans `detected`).
    pub repaired: bool,
}

/// Décode `bytes` en UTF-8 strict, puis détecte et, si `repair`, répare le mojibake.
///
/// `endpoint` est un gabarit de chemin (jamais une URL ni un fragment du corps) : il sert
/// seulement à situer l'erreur. La détection ne dépend pas de `repair` : couper la
/// réparation ne coupe pas le constat (04 §10.5).
///
/// # Errors
///
/// [`CorpusError::Decode`] si `bytes` n'est pas de l'UTF-8 valide ; le détail ne cite que
/// la position de l'octet fautif.
pub(crate) fn decode_body(
    bytes: &[u8],
    endpoint: &str,
    repair: bool,
) -> Result<DecodedBody, CorpusError> {
    let text = std::str::from_utf8(bytes).map_err(|error| CorpusError::Decode {
        detail: format!("invalid UTF-8 at byte {} on {endpoint}", error.valid_up_to()),
    })?;
    // Toujours détecter, même quand la réparation est coupée (04 §10.5).
    let detected = has_latin1_mojibake(text);
    let fixed = if repair { repair_latin1_mojibake(text) } else { Cow::Borrowed(text) };
    // Le domaine ne rend `Owned` que pour une réparation acceptée, et elle change le texte.
    let repaired = matches!(fixed, Cow::Owned(_));
    Ok(DecodedBody { text: fixed.into_owned(), detected, repaired })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Corps JSON dont le texte est du mojibake synthétique (octets UTF-8 relus en latin-1).
    fn mojibake_json(texte: &str) -> String {
        let mojibake: String = texte.bytes().map(char::from).collect();
        format!(r#"{{"texte":"{mojibake}"}}"#)
    }

    #[test]
    fn invalid_utf8_body_maps_to_decode() {
        assert_eq!(
            decode_body(&[0xC3, 0x28], "/consult/juri", true),
            Err(CorpusError::Decode { detail: "invalid UTF-8 at byte 0 on /consult/juri".into() })
        );
    }

    #[test]
    fn utf8_error_position_is_reported() {
        let prefix = br#"{"a":"ok "#;
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(&[0xE9, b'"', b'}']);
        let at = prefix.len();
        assert_eq!(
            decode_body(&bytes, "/search", false),
            Err(CorpusError::Decode { detail: format!("invalid UTF-8 at byte {at} on /search") })
        );
    }

    #[test]
    fn decode_error_never_contains_body_text() {
        let mut bytes = b"SECRET-CORPS ".to_vec();
        bytes.push(0xFF);
        let message = decode_body(&bytes, "/consult/juri", true).unwrap_err().to_string();
        assert!(!message.contains("SECRET"), "{message}");
    }

    #[test]
    fn mojibake_body_is_repaired() {
        let body = mojibake_json("PREMIÈRE CHAMBRE CIVILE");
        assert_eq!(
            decode_body(body.as_bytes(), "/consult/juri", true),
            Ok(DecodedBody {
                text: r#"{"texte":"PREMIÈRE CHAMBRE CIVILE"}"#.into(),
                detected: true,
                repaired: true,
            })
        );
    }

    #[test]
    fn unrepairable_mojibake_is_detected_and_body_kept() {
        let body = "{\"texte\":\"R\u{c3}\u{a9}publique \u{20ac}\"}";
        assert_eq!(
            decode_body(body.as_bytes(), "/consult/juri", true),
            Ok(DecodedBody { text: body.into(), detected: true, repaired: false })
        );
    }

    #[test]
    fn nbsp_false_positive_is_detected_but_kept() {
        let body = "{\"texte\":\"constitutionnalit\u{e9}\u{a0};\"}";
        assert_eq!(
            decode_body(body.as_bytes(), "/consult/juri", true),
            Ok(DecodedBody { text: body.into(), detected: true, repaired: false })
        );
    }

    #[test]
    fn clean_body_is_untouched() {
        let body = r#"{"texte":"PREMIÈRE CHAMBRE CIVILE"}"#;
        assert_eq!(
            decode_body(body.as_bytes(), "/consult/juri", true),
            Ok(DecodedBody { text: body.into(), detected: false, repaired: false })
        );
    }

    #[test]
    fn repair_toggle_off_keeps_detection() {
        let body = mojibake_json("PREMIÈRE CHAMBRE CIVILE");
        assert_eq!(
            decode_body(body.as_bytes(), "/consult/juri", false),
            Ok(DecodedBody { text: body, detected: true, repaired: false })
        );
    }

    #[test]
    fn empty_body_decodes_to_empty_text() {
        assert_eq!(
            decode_body(b"", "/search", true),
            Ok(DecodedBody { text: String::new(), detected: false, repaired: false })
        );
    }
}
