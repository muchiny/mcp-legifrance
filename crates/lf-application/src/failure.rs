//! Échec du port, vu par l'opérateur puis par le modèle (I-1 de la revue finale du plan 3a).
//!
//! [`port_failure`] est le point d'entrée unique des cas d'usage (plan 3b) pour une
//! [`CorpusError`] : il journalise le `detail` que le modèle ne lit jamais, puis délègue la
//! rédaction du message à [`messages::corpus_error`], qui reste pur. Le module n'est pas pur
//! (il émet un événement `tracing`), d'où sa place hors de `messages`.

use crate::{error::AppError, messages, ports::CorpusError, tools::ToolName};

/// Traduit l'erreur du port en `AppError` pour `tool`, après l'avoir journalisée.
///
/// L'événement `corpus call failed` porte l'outil, le nom de la variante et le `detail` (le
/// `what` d'un `NotFound` ; rien pour `Timeout` ni `RateLimited`, qui n'en ont pas) : c'est un
/// journal d'opérateur, et l'adapter garantit qu'aucun `detail` ne porte de secret, de jeton ni
/// d'URL avec paramètres (02 §5.1). Niveau `info` pour un `NotFound`, issue attendue (D-08),
/// `warn` sinon. `subject` est l'identifiant demandé, cité par les messages `not_found` du
/// modèle mais jamais journalisé : c'est la valeur d'un argument (02 §11.3).
#[must_use]
pub fn port_failure(tool: ToolName, subject: &str, error: &CorpusError) -> AppError {
    let (variant, detail) = variant_and_detail(error);
    let tool_name = tool.as_str();
    if let CorpusError::NotFound { .. } = error {
        tracing::info!(tool = tool_name, variant, detail, "corpus call failed");
    } else {
        tracing::warn!(tool = tool_name, variant, detail, "corpus call failed");
    }
    messages::corpus_error(tool, subject, error)
}

/// Nom de la variante et texte de journal de l'erreur. Le `match` est exhaustif : une variante
/// ajoutée au port devra dire ce qu'elle journalise.
fn variant_and_detail(error: &CorpusError) -> (&'static str, Option<&str>) {
    match error {
        CorpusError::NotFound { what } => ("NotFound", Some(what)),
        CorpusError::InvalidRequest { status: _, detail } => ("InvalidRequest", Some(detail)),
        CorpusError::Upstream { status: _, retryable: _, detail } => ("Upstream", Some(detail)),
        CorpusError::Timeout => ("Timeout", None),
        CorpusError::RateLimited { retry_after: _ } => ("RateLimited", None),
        CorpusError::Auth { origin: _, status: _, detail } => ("Auth", Some(detail)),
        CorpusError::Config { kind: _, detail } => ("Config", Some(detail)),
        CorpusError::Decode { detail } => ("Decode", Some(detail)),
        CorpusError::TooLarge { detail } => ("TooLarge", Some(detail)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capture::Capture,
        ports::{AuthOrigin, ConfigKind},
    };
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};
    use tracing::Level;

    /// Identifiant demandé : un argument du modèle, jamais journalisé.
    const SUBJECT: &str = "LEGIARTI000006419288";

    /// Champs attendus de l'événement d'échec.
    fn fields(tool: &str, variant: &str, detail: Option<&str>) -> BTreeMap<&'static str, String> {
        let mut fields = BTreeMap::from([
            ("message", "corpus call failed".to_owned()),
            ("tool", tool.to_owned()),
            ("variant", variant.to_owned()),
        ]);
        if let Some(detail) = detail {
            fields.insert("detail", detail.to_owned());
        }
        fields
    }

    /// Les neuf variantes du port, avec le niveau, le nom et le `detail` attendus.
    fn rows() -> Vec<(CorpusError, Level, &'static str, Option<&'static str>)> {
        vec![
            (
                CorpusError::NotFound { what: "/consult/getArticle".to_owned() },
                Level::INFO,
                "NotFound",
                Some("/consult/getArticle"),
            ),
            (
                CorpusError::InvalidRequest { status: Some(400), detail: "bad facet".to_owned() },
                Level::WARN,
                "InvalidRequest",
                Some("bad facet"),
            ),
            (
                CorpusError::Upstream {
                    status: Some(503),
                    retryable: true,
                    detail: "HTTP 503 on /consult/getArticle".to_owned(),
                },
                Level::WARN,
                "Upstream",
                Some("HTTP 503 on /consult/getArticle"),
            ),
            (CorpusError::Timeout, Level::WARN, "Timeout", None),
            (
                CorpusError::RateLimited { retry_after: Some(Duration::from_secs(2)) },
                Level::WARN,
                "RateLimited",
                None,
            ),
            (
                CorpusError::Auth {
                    origin: AuthOrigin::Token,
                    status: Some(401),
                    detail: "OAuth token request rejected: HTTP 401".to_owned(),
                },
                Level::WARN,
                "Auth",
                Some("OAuth token request rejected: HTTP 401"),
            ),
            (
                CorpusError::Config {
                    kind: ConfigKind::ApiEndpoint,
                    detail: "HTTP 404 on /consult/getArticle".to_owned(),
                },
                Level::WARN,
                "Config",
                Some("HTTP 404 on /consult/getArticle"),
            ),
            (
                CorpusError::Decode { detail: "expected value at line 1 column 1".to_owned() },
                Level::WARN,
                "Decode",
                Some("expected value at line 1 column 1"),
            ),
            (
                CorpusError::TooLarge {
                    detail: "response too large (> 16 MiB) on /consult/lawDecree".to_owned(),
                },
                Level::WARN,
                "TooLarge",
                Some("response too large (> 16 MiB) on /consult/lawDecree"),
            ),
        ]
    }

    #[test]
    fn port_failure_logs_the_detail_then_returns_the_model_message() {
        for (error, level, variant, detail) in rows() {
            let (capture, _guard) = Capture::install();
            let got = port_failure(ToolName::FR_GET_TEXT, SUBJECT, &error);
            // Le message du modèle est celui de `messages`, inchangé.
            assert_eq!(got, messages::corpus_error(ToolName::FR_GET_TEXT, SUBJECT, &error));
            // Un seul événement, au niveau de D-08 : `info` pour l'introuvable, attendu.
            assert_eq!(
                capture.event_fields(),
                vec![(level, fields("fr_get_text", variant, detail))],
                "{variant}"
            );
            // L'identifiant demandé (argument du modèle) n'est jamais journalisé.
            assert!(!capture.all_values().iter().any(|v| v.contains(SUBJECT)), "{variant}");
        }
    }

    #[test]
    fn port_failure_names_the_tool_it_was_called_for() {
        let (capture, _guard) = Capture::install();
        let error = CorpusError::Timeout;
        let _ = port_failure(ToolName::FR_SEARCH, "", &error);
        let _ = port_failure(ToolName::FR_GET_DECISION, "JURITEXT000037999394", &error);
        let tools: Vec<String> =
            capture.event_fields().into_iter().map(|(_, fields)| fields["tool"].clone()).collect();
        assert_eq!(tools, ["fr_search", "fr_get_decision"]);
    }
}
