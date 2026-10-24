//! Corps d'erreur de l'API Légifrance (04 §11.3).

use std::fmt;

use serde::Deserialize;

use super::de;

/// Corps JSON d'une réponse d'erreur de PISTE (400, 404, 500…). Les deux champs sont
/// facultatifs ; seuls `error` et `message` sont déclarés : les autres (`status` et `errorCode`
/// depuis le plan 2g, `path`, `timestamp`…) sont sautés sans construire de valeur, et aucun type
/// inattendu ne peut faire perdre le message ni le marqueur « introuvable » (V-67).
///
/// La lecture reste facultative : le classificateur ignore toute erreur de décodage. Le `Debug`
/// est écrit à la main : `message` vient du corps et peut citer la requête.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PisteErrorDto {
    /// Code numérique de l'erreur (en nombre ou en chaîne de chiffres ; sinon `None`).
    #[serde(default, deserialize_with = "de::opt_u64_lenient")]
    pub error: Option<u64>,
    /// Message libre de PISTE.
    #[serde(default)]
    pub message: Option<String>,
}

impl fmt::Debug for PisteErrorDto {
    /// Seul le code numérique est écrit : le reste vient du corps.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("PisteErrorDto").field("error", &self.error).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(raw: &str) -> PisteErrorDto {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn the_known_400_shape_is_read() {
        let dto = read(
            r#"{"error":400,"status":"Bad Request","message":"Bad value","errorCode":"E42","path":"/consult/juri"}"#,
        );
        assert_eq!((dto.error, dto.message.as_deref()), (Some(400), Some("Bad value")));
    }

    #[test]
    fn every_field_is_optional() {
        let dto = read("{}");
        assert_eq!((dto.error, dto.message), (None, None));
    }

    #[test]
    fn error_is_lenient() {
        let errors = [r#""404""#, "404", r#""Not Found""#, "null", "{}", "-1"];
        assert_eq!(
            errors.map(|raw| read(&format!(r#"{{"error":{raw}}}"#)).error),
            [Some(404), Some(404), None, None, None, None]
        );
    }

    #[test]
    fn status_and_error_code_of_any_shape_never_cost_the_message() {
        // `status` et `errorCode` ne sont plus déclarés (plan 2g) : sautés sans valeur construite.
        for raw in [r#""E42""#, "1234", "1.5", "null", r#"{"code":1}"#, r#"["E42"]"#, "true"] {
            let dto = read(&format!(r#"{{"message":"kept","errorCode":{raw},"status":{raw}}}"#));
            assert_eq!(dto.message.as_deref(), Some("kept"), "{raw}");
        }
    }

    #[test]
    fn unreadable_json_is_an_error() {
        let raws = ["<html>", "", r#"{"message":42}"#];
        assert_eq!(raws.map(|raw| serde_json::from_str::<PisteErrorDto>(raw).is_err()), [true; 3]);
    }

    #[test]
    fn debug_shows_only_the_numeric_code() {
        let dto = read(r#"{"error":400,"message":"secret-in-body","status":"S"}"#);
        assert_eq!(format!("{dto:?}"), "PisteErrorDto { error: Some(400), .. }");
    }
}
