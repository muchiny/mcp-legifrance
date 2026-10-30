//! Sérialisation d'une ligne d'audit (06 §11.2, 02 §3.5).
//!
//! Les clés sont écrites à la main, dans l'ordre normatif : `serde_json` ne garantit pas l'ordre
//! d'une table, et la ligne ne passe jamais par un type intermédiaire qui pourrait le changer.

use chrono::{DateTime, Datelike, Utc};
use lf_application::ports::{AuditError, AuditErrorKind, AuditRecord, AuditStatus};

/// Longueur maximale de `error`, en caractères Unicode (06 §11.2, ligne 8).
pub(crate) const ERROR_MAX_CHARS: usize = 500;

/// Date UTC `AAAA-MM-JJ` de l'horodatage : celle qui décide de la rotation (06 §11.5, 6).
pub(crate) fn utc_date(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d").to_string()
}

/// La ligne JSON compacte, terminée par `\n`, clés dans l'ordre normatif.
///
/// # Errors
///
/// `AuditErrorKind::Encode` si l'horodatage sort de `0000..=9999` : sa date ne tiendrait pas sur
/// les 10 octets que la rotation relit au début du fichier.
pub(crate) fn render(record: &AuditRecord, input_hash: &str) -> Result<String, AuditError> {
    let year = record.ts.year();
    if !(0..=9999).contains(&year) {
        return Err(AuditError {
            kind: AuditErrorKind::Encode,
            detail: format!("timestamp year {year} is outside 0000..=9999"),
        });
    }
    let mut line = String::with_capacity(320);
    line.push_str(r#"{"ts":"#);
    push_string(&mut line, &record.ts.format("%Y-%m-%dT%H:%M:%S%.3f+00:00").to_string());
    line.push_str(r#","tool":"#);
    push_string(&mut line, record.tool.as_str());
    line.push_str(r#","input_hash":"#);
    push_string(&mut line, input_hash);
    line.push_str(r#","output_count_or_size":"#);
    line.push_str(&record.output_count_or_size.to_string());
    line.push_str(r#","duration_ms":"#);
    line.push_str(&record.duration_ms.to_string());
    line.push_str(r#","status":"#);
    push_string(
        &mut line,
        match record.status {
            AuditStatus::Ok => "ok",
            AuditStatus::Error => "error",
        },
    );
    if let Some(code) = record.error_code {
        line.push_str(r#","error_code":"#);
        push_string(&mut line, code.as_str());
    }
    if let Some(error) = &record.error {
        line.push_str(r#","error":"#);
        push_string(&mut line, truncate_chars(error, ERROR_MAX_CHARS));
    }
    line.push_str(r#","env":"#);
    push_string(&mut line, record.env.name());
    line.push_str(r#","server_version":"#);
    push_string(&mut line, record.server_version);
    line.push_str("}\n");
    Ok(line)
}

/// Les `max` premiers caractères de `text` : la coupe tombe sur une frontière de caractère.
fn truncate_chars(text: &str, max: usize) -> &str {
    text.char_indices().nth(max).map_or(text, |(end, _)| &text[..end])
}

/// Ajoute `value` comme chaîne JSON : UTF-8 brut, `"`, `\\` et contrôles échappés par
/// `serde_json`, plus NEL, U+2028 et U+2029 (séparateurs de ligne pour certains lecteurs, que
/// `serde_json` laisse bruts).
fn push_string(line: &mut String, value: &str) {
    for ch in serde_json::Value::from(value).to_string().chars() {
        match ch {
            '\u{85}' => line.push_str("\\u0085"),
            '\u{2028}' => line.push_str("\\u2028"),
            '\u{2029}' => line.push_str("\\u2029"),
            other => line.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{record, record_at};
    use chrono::TimeZone;
    use lf_application::{
        error::ErrorCode,
        ports::{AuditErrorKind, AuditStatus, PisteEnvironment},
        tools::ToolName,
    };

    const HASH: &str = "26eab19dad0d8885c5c5d5ab0716f3eccfb7f14ca193c26e8378e111203fad48";

    /// L'ordre des clés est normatif (P-040, 06 §11.2) : `ts`, `tool`, `input_hash`,
    /// `output_count_or_size`, `duration_ms`, `status`, `error_code`, `error`, `env`,
    /// `server_version`. Les deux clés d'erreur n'existent qu'en erreur.
    #[test]
    fn line_fields_in_order() {
        let ok = AuditRecord {
            tool: ToolName::FR_GET_DECISION,
            output_count_or_size: 1147,
            duration_ms: 412,
            ..record()
        };
        assert_eq!(
            render(&ok, HASH).unwrap(),
            concat!(
                r#"{"ts":"2026-10-15T15:09:12.345+00:00","tool":"fr_get_decision","#,
                r#""input_hash":"26eab19dad0d8885c5c5d5ab0716f3eccfb7f14ca193c26e8378e111203fad48","#,
                r#""output_count_or_size":1147,"duration_ms":412,"status":"ok","#,
                r#""env":"sandbox","server_version":"0.1.0"}"#,
                "\n"
            )
        );
        let failed = AuditRecord {
            tool: ToolName::FR_GET_TEXT,
            status: AuditStatus::Error,
            error_code: Some(ErrorCode::InvalidArg),
            error: Some("article_id must be a LEGIARTI identifier.".to_owned()),
            env: PisteEnvironment::Production,
            ..record()
        };
        assert_eq!(
            render(&failed, HASH).unwrap(),
            concat!(
                r#"{"ts":"2026-10-15T15:09:12.345+00:00","tool":"fr_get_text","#,
                r#""input_hash":"26eab19dad0d8885c5c5d5ab0716f3eccfb7f14ca193c26e8378e111203fad48","#,
                r#""output_count_or_size":0,"duration_ms":3,"status":"error","#,
                r#""error_code":"invalid_arg","error":"article_id must be a LEGIARTI identifier.","#,
                r#""env":"production","server_version":"0.1.0"}"#,
                "\n"
            )
        );
    }

    /// Une annulation : `status: error`, sans `error_code`, avec `error` (00 §7).
    #[test]
    fn cancelled_call_has_error_but_no_error_code() {
        let cancelled = AuditRecord {
            status: AuditStatus::Error,
            error: Some("cancelled by client".to_owned()),
            ..record()
        };
        let line = render(&cancelled, HASH).unwrap();
        assert!(line.contains(r#""status":"error","error":"cancelled by client","env""#), "{line}");
        assert!(!line.contains("error_code"), "{line}");
    }

    /// `AAAA-MM-JJTHH:MM:SS.mmm+00:00` : suffixe `+00:00` et non `Z` (E-33), millisecondes
    /// tronquées et non arrondies, zéros de tête conservés.
    #[test]
    fn ts_uses_plus_00_00_and_truncated_millis() {
        let ts = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
            + chrono::TimeDelta::nanoseconds(999_999_999);
        let line = render(&record_at(ts), HASH).unwrap();
        assert!(line.starts_with(r#"{"ts":"2026-01-02T03:04:05.999+00:00","#), "{line}");
        assert_eq!(utc_date(ts), "2026-01-02");
    }

    /// Un instant hors de `0000..=9999` donnerait une date qui n'a pas 10 octets : refusé
    /// plutôt qu'écrit (la rotation relit ces 10 octets).
    #[test]
    fn ts_outside_four_digit_years_is_an_encode_error() {
        for ts in [DateTime::<Utc>::MAX_UTC, DateTime::<Utc>::MIN_UTC] {
            let error = render(&record_at(ts), HASH).unwrap_err();
            assert_eq!(error.kind, AuditErrorKind::Encode);
            assert!(error.detail.contains("year"), "{}", error.detail);
        }
        let edge = Utc.with_ymd_and_hms(9999, 12, 31, 23, 59, 59).unwrap();
        assert!(render(&record_at(edge), HASH).is_ok());
    }

    /// 500 caractères, pas 500 octets : on ne coupe jamais en plein UTF-8.
    #[test]
    fn error_truncated_to_500_chars() {
        let long =
            AuditRecord { status: AuditStatus::Error, error: Some("é".repeat(600)), ..record() };
        let line = render(&long, HASH).unwrap();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["error"].as_str().unwrap().chars().count(), ERROR_MAX_CHARS);

        let exact = AuditRecord { error: Some("é".repeat(ERROR_MAX_CHARS)), ..long };
        let line = render(&exact, HASH).unwrap();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["error"].as_str().unwrap().chars().count(), ERROR_MAX_CHARS);
    }

    /// UTF-8 écrit tel quel (`ensure_ascii=False` de la référence) ; seuls `"`, `\` et les
    /// contrôles sont échappés, comme `serde_json` le fait.
    #[test]
    fn line_is_compact_unescaped_utf8() {
        let failed = AuditRecord {
            status: AuditStatus::Error,
            error: Some("décision \"introuvable\" 😀\nligne 2".to_owned()),
            ..record()
        };
        let line = render(&failed, HASH).unwrap();
        assert!(line.contains(r#""error":"décision \"introuvable\" 😀\nligne 2""#), "{line}");
        assert_eq!(line.matches('\n').count(), 1, "une ligne, un seul saut de ligne final");
        assert!(line.ends_with("}\n"));
        assert!(!line.contains(": ") && !line.contains(", "), "{line}");
    }

    /// Une ligne du journal reste une ligne pour tout lecteur : NEL, U+2028 et U+2029, que
    /// `serde_json` laisse bruts et que `splitlines` de Python coupe, sont échappés
    /// (`Audited` les remplace déjà, F-1 : défense en profondeur).
    #[test]
    fn line_separators_are_escaped() {
        let failed = AuditRecord {
            status: AuditStatus::Error,
            error: Some("a\u{85}b\u{2028}c\u{2029}d".to_owned()),
            ..record()
        };
        let line = render(&failed, HASH).unwrap();
        assert!(line.contains(r#""error":"a\u0085b\u2028c\u2029d""#), "{line}");
        assert_eq!(line.lines().count(), 1);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["error"], "a\u{85}b\u{2028}c\u{2029}d");
    }

    /// Seule l'empreinte figure dans la ligne, jamais une valeur d'argument (D-17, 02 §11.3).
    #[test]
    fn audit_never_contains_argument_values() {
        let sentinel = "sentinel-argument-value";
        let secret = AuditRecord {
            input: lf_application::input::CanonicalInput::new(format!(
                r#"{{"query":"{sentinel}"}}"#
            )),
            ..record()
        };
        let line = render(&secret, HASH).unwrap();
        assert!(!line.contains(sentinel), "{line}");
        assert!(line.contains(HASH));
    }

    /// Schéma de 06 §11.3 : ligne valide, clés admises seulement, nom d'outil, empreinte et
    /// horodatage de la bonne forme.
    #[test]
    fn audit_line_validates_against_schema() {
        let allowed = [
            "ts",
            "tool",
            "input_hash",
            "output_count_or_size",
            "duration_ms",
            "status",
            "error_code",
            "error",
            "env",
            "server_version",
        ];
        let failed = AuditRecord {
            tool: ToolName::RESOURCE_ARTICLE,
            status: AuditStatus::Error,
            error_code: Some(ErrorCode::NotFound),
            error: Some("No article found.".to_owned()),
            ..record()
        };
        for sample in [record(), failed] {
            let line = render(&sample, HASH).unwrap();
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            let object = value.as_object().unwrap();
            assert!(object.keys().all(|key| allowed.contains(&key.as_str())), "{line}");
            let ts = object["ts"].as_str().unwrap();
            assert_eq!(ts.len(), 29, "{ts}");
            assert!(ts.ends_with("+00:00"), "{ts}");
            assert_eq!(object["input_hash"].as_str().unwrap().len(), 64);
            assert!(object["output_count_or_size"].is_u64() && object["duration_ms"].is_u64());
            let ok = object["status"] == "ok";
            assert_eq!(object.contains_key("error"), !ok, "{line}");
            assert!(ok || object.contains_key("error_code"), "{line}");
        }
    }
}
