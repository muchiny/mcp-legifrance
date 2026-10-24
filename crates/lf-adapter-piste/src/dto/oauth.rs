//! DTO de l'endpoint token OAuth 2.0 (04 §3.2).

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use super::de;

/// Réponse 200 de l'endpoint token. Tous les champs sont facultatifs : c'est `auth` qui juge
/// ce qui manque.
#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponseDto {
    /// Jeton d'accès ; son `Debug` est rédigé.
    #[serde(default)]
    pub access_token: Option<SecretString>,
    /// Type du jeton (`Bearer` attendu).
    #[serde(default)]
    pub token_type: Option<String>,
    /// Durée de vie en secondes, lue en nombre ou en chaîne de chiffres.
    #[serde(default, deserialize_with = "de::opt_u64_lenient")]
    pub expires_in: Option<u64>,
}

impl TokenResponseDto {
    /// Le jeton s'il est présent et non vide.
    ///
    /// Troisième point d'exposition d'un secret du crate (avec le formulaire OAuth et
    /// `bearer_auth`) : `expose_secret()` ne sert ici qu'à tester la longueur, la valeur n'est
    /// ni copiée ni journalisée. [`is_header_safe`] en est un quatrième, de même nature.
    pub(crate) fn usable_token(self) -> Option<SecretString> {
        self.access_token.filter(|token| !token.expose_secret().is_empty())
    }
}

/// Vrai si le jeton peut figurer dans l'en-tête `Authorization: Bearer …` : uniquement de
/// l'ASCII visible, l'espace et la tabulation. Un caractère de contrôle (retour ligne
/// compris) ou un octet hors ASCII ferait échouer `bearer_auth` à chaque envoi, sans requête
/// partie ; le jeton est alors refusé dès la réponse, avant toute mise en cache.
///
/// `expose_secret()` ne sert ici qu'à parcourir les octets : rien n'est copié ni journalisé.
pub(crate) fn is_header_safe(token: &SecretString) -> bool {
    token.expose_secret().bytes().all(|byte| byte == b'\t' || (b' '..=b'~').contains(&byte))
}

/// Corps d'erreur de l'endpoint token (RFC 6749 §5.2).
///
/// Seul `error` est déclaré : depuis le plan 2g, `error_description`, texte libre qui pourrait
/// citer la requête et que rien ne lit, est sauté sans construire de valeur. Accepte aussi un
/// tableau JSON `[]` (forme séquence de `serde`, sans champ requis).
#[derive(Debug, Deserialize)]
pub(crate) struct OAuthErrorDto {
    /// Code d'erreur (`invalid_client`…).
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn token(raw: &str) -> TokenResponseDto {
        serde_json::from_str(raw).unwrap()
    }

    fn exposed(dto: TokenResponseDto) -> Option<String> {
        dto.access_token.map(|secret| secret.expose_secret().to_owned())
    }

    #[test]
    fn a_complete_response_is_read() {
        let dto = token(
            r#"{"access_token":"tok","token_type":"Bearer","expires_in":3600,"scope":"openid"}"#,
        );
        assert_eq!(
            (dto.token_type.clone(), dto.expires_in, exposed(dto)),
            (Some("Bearer".to_owned()), Some(3600), Some("tok".to_owned()))
        );
    }

    #[test]
    fn every_field_is_optional() {
        let dto = token("{}");
        assert_eq!((dto.token_type.clone(), dto.expires_in, exposed(dto)), (None, None, None));
    }

    #[test]
    fn expires_in_is_lenient() {
        let read = |raw: &str| token(&format!(r#"{{"expires_in":{raw}}}"#)).expires_in;
        assert_eq!([r#""900""#, "null", r#""soon""#].map(read), [Some(900), None, None]);
    }

    #[test]
    fn usable_token_is_the_non_empty_token() {
        let usable =
            |raw: &str| token(raw).usable_token().map(|secret| secret.expose_secret().to_owned());
        assert_eq!(
            [
                usable(r#"{"access_token":"tok"}"#),
                usable(r#"{"access_token":""}"#),
                usable(r#"{"access_token":null}"#),
                usable("{}"),
            ],
            [Some("tok".to_owned()), None, None, None]
        );
    }

    #[test]
    fn header_safe_means_visible_ascii_space_or_tab() {
        let safe = |text: &str| is_header_safe(&SecretString::from(text));
        assert_eq!(
            [
                safe("tok"),
                safe("a b\tc~"),
                safe(""),
                safe("a\nb"),
                safe("a\rb"),
                safe("a\0b"),
                safe("a\x7fb"),
                safe("é"),
            ],
            [true, true, true, false, false, false, false, false]
        );
    }

    #[test]
    fn header_safe_agrees_with_the_header_value_rules_on_ascii() {
        for code in 0_u8..=127 {
            let text = char::from(code).to_string();
            let accepted = reqwest::header::HeaderValue::from_str(&text).is_ok();
            assert_eq!(is_header_safe(&SecretString::from(text)), accepted, "{code}");
        }
    }

    #[test]
    fn debug_of_the_response_hides_the_token() {
        let shown = format!("{:?}", token(r#"{"access_token":"tok-visible-if-leaked"}"#));
        assert_eq!(
            (shown.contains("tok-visible-if-leaked"), shown.contains("REDACTED")),
            (false, true),
            "{shown}"
        );
    }

    #[test]
    fn an_error_body_is_read_with_optional_fields() {
        let full: OAuthErrorDto =
            serde_json::from_str(r#"{"error":"invalid_client","error_description":"bad"}"#)
                .unwrap();
        let empty: OAuthErrorDto = serde_json::from_str("{}").unwrap();
        assert_eq!((full.error, empty.error), (Some("invalid_client".to_owned()), None));
    }

    #[test]
    fn unreadable_json_is_an_error() {
        assert_eq!(
            [
                serde_json::from_str::<TokenResponseDto>("<html>").is_err(),
                serde_json::from_str::<OAuthErrorDto>("<html>").is_err(),
                serde_json::from_str::<OAuthErrorDto>(r#"{"error":42}"#).is_err(),
            ],
            [true, true, true]
        );
    }
}
