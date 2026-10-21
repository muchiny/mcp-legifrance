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
    /// ni copiée ni journalisée.
    pub(crate) fn usable_token(self) -> Option<SecretString> {
        self.access_token.filter(|token| !token.expose_secret().is_empty())
    }
}

/// Corps d'erreur de l'endpoint token (RFC 6749 §5.2).
///
/// Accepte aussi un tableau JSON `[]` (forme séquence de `serde`, sans champ requis).
#[derive(Debug, Deserialize)]
pub(crate) struct OAuthErrorDto {
    /// Code d'erreur (`invalid_client`…).
    #[serde(default)]
    pub error: Option<String>,
    /// Description libre : jamais recopiée dans un message, elle pourrait citer la requête.
    #[serde(default)]
    pub error_description: Option<String>,
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
        assert_eq!(
            (full.error, full.error_description, empty.error, empty.error_description),
            (Some("invalid_client".to_owned()), Some("bad".to_owned()), None, None)
        );
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
