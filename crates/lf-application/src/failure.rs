//! Échec du port, vu par l'opérateur puis par le modèle (I-1 de la revue finale du plan 3a).
//!
//! [`port_failure`] est le point d'entrée unique des cas d'usage (plan 3b) pour une
//! [`CorpusError`] : il journalise le `detail` de l'erreur (le modèle n'en lit que deux,
//! coupés : celui d'`InvalidRequest` et celui d'un `Config { kind: Settings }`, 05 §7.2 ; tous
//! les autres ne vont qu'au journal), puis délègue la rédaction du message à
//! [`messages::corpus_error`], qui reste pur. Le module n'est pas pur (il émet un événement
//! `tracing`), d'où sa place hors de `messages`.
//!
//! [`doctrine_failure`] fait de même pour une [`DoctrineError`] du port de doctrine (BOFiP), et
//! [`guidance_failure`] pour une [`GuidanceError`] du port de Service-Public.

use lf_domain::guidance::{Audience, SheetId};

use crate::{
    error::AppError,
    messages,
    ports::{CorpusError, DoctrineError, GuidanceError, PortalCode},
    tools::ToolName,
};

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
pub(crate) fn variant_and_detail(error: &CorpusError) -> (&'static str, Option<&str>) {
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

/// Traduit l'erreur du port de doctrine en `AppError` pour `tool`, après l'avoir journalisée.
///
/// L'événement `doctrine call failed` porte l'outil, le nom de la variante et son détail : le
/// `detail` fixe de l'adapter, ou le code d'erreur du portail pour un refus. L'adapter garantit
/// qu'aucun détail ne porte l'adresse de la requête, ses mots ni un message du portail (ADR-0029,
/// décision 6). Niveau `info` pour une publication introuvable et pour une source coupée, issues
/// attendues ; `warn` sinon. `subject` est l'identifiant demandé, cité par le message
/// `not_found` du modèle mais jamais journalisé : c'est la valeur d'un argument (02 §11.3).
#[must_use]
pub fn doctrine_failure(tool: ToolName, subject: &str, error: &DoctrineError) -> AppError {
    let (variant, detail) = doctrine_variant_and_detail(error);
    let tool_name = tool.as_str();
    if matches!(error, DoctrineError::NotFound | DoctrineError::Disabled) {
        tracing::info!(tool = tool_name, variant, detail, "doctrine call failed");
    } else {
        tracing::warn!(tool = tool_name, variant, detail, "doctrine call failed");
    }
    messages::doctrine_error(tool, subject, error)
}

/// Nom de la variante et texte de journal de l'erreur du port de doctrine. Le `match` est
/// exhaustif : une variante ajoutée au port devra dire ce qu'elle journalise.
fn doctrine_variant_and_detail(error: &DoctrineError) -> (&'static str, Option<&str>) {
    match error {
        DoctrineError::Disabled => ("Disabled", None),
        DoctrineError::NotFound => ("NotFound", None),
        DoctrineError::RateLimited { retry_after: _ } => ("RateLimited", None),
        DoctrineError::InvalidRequest { status: _, code } => {
            ("InvalidRequest", code.as_ref().map(PortalCode::as_str))
        }
        DoctrineError::Upstream { status: _, retryable: _, detail } => ("Upstream", Some(detail)),
        DoctrineError::Timeout => ("Timeout", None),
        DoctrineError::Decode { detail } => ("Decode", Some(detail)),
        DoctrineError::TooLarge { detail } => ("TooLarge", Some(detail)),
    }
}

/// Traduit l'erreur du port de Service-Public en `AppError` pour `tool`, après l'avoir
/// journalisée.
///
/// L'événement `guidance call failed` porte l'outil, le nom de la variante et le `detail` fixe
/// de l'adapter ; jamais un mot de la recherche, ni le numéro ou l'adresse de la page demandée,
/// valeurs d'arguments (02 §11.3). Niveau `info` pour une page absente, une source coupée et une
/// archive en cours de téléchargement, issues attendues ; `warn` sinon. `missing` est la page
/// demandée par une lecture, que le message `not_found` cite.
#[must_use]
pub fn guidance_failure(
    tool: ToolName,
    missing: Option<(&SheetId, Audience)>,
    error: &GuidanceError,
) -> AppError {
    let (variant, detail) = guidance_variant_and_detail(error);
    let tool_name = tool.as_str();
    if matches!(
        error,
        GuidanceError::NotFound | GuidanceError::Disabled | GuidanceError::Downloading
    ) {
        tracing::info!(tool = tool_name, variant, detail, "guidance call failed");
    } else {
        tracing::warn!(tool = tool_name, variant, detail, "guidance call failed");
    }
    messages::guidance_error(missing, error)
}

/// Nom de la variante et texte de journal de l'erreur du port de Service-Public. Le `match` est
/// exhaustif : une variante ajoutée au port devra dire ce qu'elle journalise.
fn guidance_variant_and_detail(error: &GuidanceError) -> (&'static str, Option<&str>) {
    match error {
        GuidanceError::Disabled => ("Disabled", None),
        GuidanceError::NotFound => ("NotFound", None),
        GuidanceError::RateLimited { retry_after: _ } => ("RateLimited", None),
        GuidanceError::Downloading => ("Downloading", None),
        // L'adresse nomme la page demandée : un argument, jamais journalisé.
        GuidanceError::EntryTooLarge { source_url: _ } => ("EntryTooLarge", None),
        GuidanceError::Upstream { status: _, retryable: _, detail } => ("Upstream", Some(detail)),
        GuidanceError::Timeout => ("Timeout", None),
        GuidanceError::Decode { detail } => ("Decode", Some(detail)),
        GuidanceError::TooLarge { detail } => ("TooLarge", Some(detail)),
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
    /// Champs attendus de l'événement d'échec du port de doctrine.
    fn doctrine_fields(
        tool: &str,
        variant: &str,
        detail: Option<&str>,
    ) -> BTreeMap<&'static str, String> {
        let mut fields = fields(tool, variant, detail);
        fields.insert("message", "doctrine call failed".to_owned());
        fields
    }

    /// Les huit variantes du port de doctrine, avec le niveau, le nom et le détail attendus.
    fn doctrine_rows() -> Vec<(DoctrineError, Level, &'static str, Option<&'static str>)> {
        vec![
            (DoctrineError::Disabled, Level::INFO, "Disabled", None),
            (DoctrineError::NotFound, Level::INFO, "NotFound", None),
            (
                DoctrineError::RateLimited { retry_after: Some(Duration::from_secs(900)) },
                Level::WARN,
                "RateLimited",
                None,
            ),
            (
                DoctrineError::InvalidRequest {
                    status: 400,
                    code: PortalCode::parse("ODSQLSyntaxError"),
                },
                Level::WARN,
                "InvalidRequest",
                Some("ODSQLSyntaxError"),
            ),
            (
                DoctrineError::InvalidRequest { status: 400, code: None },
                Level::WARN,
                "InvalidRequest",
                None,
            ),
            (
                DoctrineError::Upstream {
                    status: Some(503),
                    retryable: true,
                    detail: "HTTP 503 on records".to_owned(),
                },
                Level::WARN,
                "Upstream",
                Some("HTTP 503 on records"),
            ),
            (DoctrineError::Timeout, Level::WARN, "Timeout", None),
            (
                DoctrineError::Decode { detail: "records: unexpected id".to_owned() },
                Level::WARN,
                "Decode",
                Some("records: unexpected id"),
            ),
            (
                DoctrineError::TooLarge { detail: "response too large (> 4 MiB)".to_owned() },
                Level::WARN,
                "TooLarge",
                Some("response too large (> 4 MiB)"),
            ),
        ]
    }

    #[test]
    fn doctrine_failure_logs_the_detail_then_returns_the_model_message() {
        const BOI: &str = "BOI-ENR-DMTG-10-50-20";
        for (error, level, variant, detail) in doctrine_rows() {
            let (capture, _guard) = Capture::install();
            let got = doctrine_failure(ToolName::FR_GET_BOFIP, BOI, &error);
            assert_eq!(got, messages::doctrine_error(ToolName::FR_GET_BOFIP, BOI, &error));
            assert_eq!(
                capture.event_fields(),
                vec![(level, doctrine_fields("fr_get_bofip", variant, detail))],
                "{variant}"
            );
            // L'identifiant demandé (argument du modèle) n'est jamais journalisé.
            assert!(!capture.all_values().iter().any(|v| v.contains(BOI)), "{variant}");
        }
    }

    #[test]
    fn doctrine_failure_names_the_tool_it_was_called_for() {
        let (capture, _guard) = Capture::install();
        let _ = doctrine_failure(ToolName::FR_SEARCH_BOFIP, "", &DoctrineError::Timeout);
        assert_eq!(
            capture.event_fields(),
            vec![(Level::WARN, doctrine_fields("fr_search_bofip", "Timeout", None))]
        );
    }

    /// Champs attendus de l'événement d'échec du port de Service-Public.
    fn guidance_fields(
        tool: &str,
        variant: &str,
        detail: Option<&str>,
    ) -> BTreeMap<&'static str, String> {
        let mut fields = fields(tool, variant, detail);
        fields.insert("message", "guidance call failed".to_owned());
        fields
    }

    /// Les neuf variantes du port de Service-Public, avec le niveau, le nom et le détail attendus.
    fn guidance_rows() -> Vec<(GuidanceError, Level, &'static str, Option<&'static str>)> {
        let id = SheetId::parse("F1437").unwrap();
        let url = lf_domain::guidance::ServicePublicUrl::read(None, Audience::Particuliers, &id);
        vec![
            (GuidanceError::Disabled, Level::INFO, "Disabled", None),
            (GuidanceError::NotFound, Level::INFO, "NotFound", None),
            (GuidanceError::Downloading, Level::INFO, "Downloading", None),
            (
                GuidanceError::RateLimited { retry_after: Some(Duration::from_secs(60)) },
                Level::WARN,
                "RateLimited",
                None,
            ),
            (GuidanceError::EntryTooLarge { source_url: url }, Level::WARN, "EntryTooLarge", None),
            (
                GuidanceError::Upstream {
                    status: Some(502),
                    retryable: true,
                    detail: "archive download: HTTP 502".to_owned(),
                },
                Level::WARN,
                "Upstream",
                Some("archive download: HTTP 502"),
            ),
            (GuidanceError::Timeout, Level::WARN, "Timeout", None),
            (
                GuidanceError::Decode { detail: "zip: data descriptor".to_owned() },
                Level::WARN,
                "Decode",
                Some("zip: data descriptor"),
            ),
            (
                GuidanceError::TooLarge { detail: "archive over 64 MiB".to_owned() },
                Level::WARN,
                "TooLarge",
                Some("archive over 64 MiB"),
            ),
        ]
    }

    #[test]
    fn guidance_failure_logs_the_detail_then_returns_the_model_message() {
        let id = SheetId::parse("F1437").unwrap();
        let missing = Some((&id, Audience::Particuliers));
        for (error, level, variant, detail) in guidance_rows() {
            let (capture, _guard) = Capture::install();
            let got = guidance_failure(ToolName::FR_GET_SERVICE_PUBLIC, missing, &error);
            assert_eq!(got, messages::guidance_error(missing, &error));
            assert_eq!(
                capture.event_fields(),
                vec![(level, guidance_fields("fr_get_service_public", variant, detail))],
                "{variant}"
            );
            // Le numéro demandé (argument du modèle) n'est jamais journalisé, même dans l'adresse.
            assert!(!capture.all_values().iter().any(|v| v.contains("1437")), "{variant}");
        }
    }

    #[test]
    fn guidance_failure_names_the_tool_it_was_called_for() {
        let (capture, _guard) = Capture::install();
        let _ = guidance_failure(ToolName::FR_SEARCH_SERVICE_PUBLIC, None, &GuidanceError::Timeout);
        assert_eq!(
            capture.event_fields(),
            vec![(Level::WARN, guidance_fields("fr_search_service_public", "Timeout", None))]
        );
    }
}
