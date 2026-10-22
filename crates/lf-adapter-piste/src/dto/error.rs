//! Corps d'erreur de l'API Légifrance (04 §11.3).

use std::fmt;

use serde::Deserialize;

use super::de;

/// Corps JSON d'une réponse d'erreur de PISTE (400, 404, 500…). Tous les champs sont
/// facultatifs et les champs inconnus (`path`, `timestamp`…) sont ignorés.
///
/// La lecture reste facultative : le classificateur ignore toute erreur de décodage. Le `Debug`
/// est écrit à la main : `message` et `status` viennent du corps et peuvent citer la requête.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PisteErrorDto {
    /// Code numérique de l'erreur (en nombre ou en chaîne de chiffres ; sinon `None`).
    #[serde(default, deserialize_with = "de::opt_u64_lenient")]
    pub error: Option<u64>,
    /// Statut, de forme libre (texte, nombre, objet).
    #[serde(default)]
    pub status: Option<serde_json::Value>,
    /// Message libre de PISTE.
    #[serde(default)]
    pub message: Option<String>,
    /// Code d'erreur applicatif.
    #[serde(default)]
    pub error_code: Option<String>,
}

impl fmt::Debug for PisteErrorDto {
    /// Seul le code numérique est écrit : le reste vient du corps.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("PisteErrorDto").field("error", &self.error).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(raw: &str) -> PisteErrorDto {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn the_known_400_shape_is_read() {
        let dto = read(
            r#"{"error":400,"status":"Bad Request","message":"Bad value","errorCode":"E42","path":"/consult/juri"}"#,
        );
        assert_eq!(
            (dto.error, dto.status, dto.message.as_deref(), dto.error_code.as_deref()),
            (Some(400), Some(json!("Bad Request")), Some("Bad value"), Some("E42"))
        );
    }

    #[test]
    fn every_field_is_optional() {
        let dto = read("{}");
        assert_eq!((dto.error, dto.status, dto.message, dto.error_code), (None, None, None, None));
    }

    #[test]
    fn error_is_lenient_and_status_keeps_any_shape() {
        let errors = [r#""404""#, "404", r#""Not Found""#, "null", "{}", "-1"];
        assert_eq!(
            errors.map(|raw| read(&format!(r#"{{"error":{raw}}}"#)).error),
            [Some(404), Some(404), None, None, None, None]
        );
        let statuses = [r#""Bad Request""#, "400", r#"{"code":400}"#, "null"];
        assert_eq!(
            statuses.map(|raw| read(&format!(r#"{{"status":{raw}}}"#)).status),
            [Some(json!("Bad Request")), Some(json!(400)), Some(json!({"code": 400})), None]
        );
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
