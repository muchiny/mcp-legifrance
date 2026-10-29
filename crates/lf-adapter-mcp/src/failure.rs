//! Échec d'un outil rendu in-band (05 §3.2, §7).
//!
//! Une erreur d'outil n'est pas une erreur JSON-RPC : le client reçoit un résultat `isError: true`
//! dont l'unique bloc texte est `[code] message`, sans `structuredContent`. Le code et le message
//! viennent de l'`AppError` (`lf_application::messages` rédige les textes, 02 §9.3).

use lf_application::error::AppError;
use rmcp::model::{ContentBlock, IntoContents};

/// L'`AppError` d'un appel d'outil, prête à être rendue : type d'erreur de
/// `Result<Json<V>, ToolFailure>`. rmcp met `isError: true` sur le résultat dès que le retour
/// est un `Err` d'`IntoContents`.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "construit par les outils des tranches (tools/*.rs), pas encore écrits"
)]
pub(crate) struct ToolFailure(pub(crate) AppError);

impl From<AppError> for ToolFailure {
    fn from(error: AppError) -> Self {
        Self(error)
    }
}

impl IntoContents for ToolFailure {
    /// Un bloc texte `[code] message`. Un appel annulé n'a pas de code (`AppError::code` rend
    /// `None`) et personne ne lira sa réponse : le message seul suffit.
    fn into_contents(self) -> Vec<ContentBlock> {
        let text = match self.0.code() {
            Some(code) => format!("[{}] {}", code.as_str(), self.0.message()),
            None => self.0.message().to_owned(),
        };
        vec![ContentBlock::text(text)]
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use lf_application::{
        error::{ErrorCode, UpstreamCause},
        messages,
        ports::{AuditError, AuditErrorKind, ConfigKind, CorpusError},
        tools::ToolName,
    };
    use pretty_assertions::assert_eq;

    /// Une erreur réelle par code, rédigée par `lf_application::messages` comme en production :
    /// les textes que le modèle lit (05 §7.2 à §7.4).
    fn sample(code: ErrorCode) -> AppError {
        match code {
            ErrorCode::InvalidArg => messages::required("query"),
            ErrorCode::NotFound => messages::corpus_error(
                ToolName::FR_GET_TEXT,
                "LEGIARTI000006419999",
                &CorpusError::NotFound { what: "/consult/getArticle".to_owned() },
            ),
            ErrorCode::UpstreamError => messages::corpus_error(
                ToolName::FR_SEARCH,
                "",
                &CorpusError::Upstream {
                    status: Some(503),
                    retryable: true,
                    detail: "upstream-detail".to_owned(),
                },
            ),
            ErrorCode::RateLimited => messages::corpus_error(
                ToolName::FR_SEARCH,
                "",
                &CorpusError::RateLimited { retry_after: Some(Duration::from_secs(12)) },
            ),
            ErrorCode::ConfigError => messages::corpus_error(
                ToolName::FR_SEARCH,
                "",
                &CorpusError::Config {
                    kind: ConfigKind::CredentialsMissing,
                    detail: "config-detail".to_owned(),
                },
            ),
            ErrorCode::AuditError => messages::audit(
                &AuditError {
                    kind: AuditErrorKind::DiskFull,
                    detail: "/home/user/audit".to_owned(),
                },
                Some(ErrorCode::NotFound),
            ),
        }
    }

    /// Le seul bloc texte rendu pour `error`.
    fn text_of(error: AppError) -> String {
        let mut blocks = ToolFailure(error).into_contents();
        assert_eq!(blocks.len(), 1);
        blocks.remove(0).as_text().unwrap().text.clone()
    }

    #[test]
    fn error_text_snapshot_per_code() {
        // Un instantané par code : le texte exact que lit le modèle, préfixe compris. Une
        // reformulation dans `lf_application::messages` doit être relue ici (05 §7.2).
        for code in ErrorCode::ALL {
            let text = text_of(sample(code));
            assert!(text.starts_with(&format!("[{}] ", code.as_str())), "{text}");
            insta::assert_snapshot!(code.as_str(), text);
        }
    }

    #[test]
    fn no_sample_message_shows_a_detail_meant_for_the_logs() {
        // Ni chemin local (le `detail` de l'audit), ni détail amont ou de configuration : ces
        // textes vont aux journaux, jamais au modèle (D-40).
        for code in ErrorCode::ALL {
            let text = text_of(sample(code));
            for hidden in ["/home/user", "upstream-detail", "config-detail"] {
                assert!(!text.contains(hidden), "{text}");
            }
        }
    }

    fn rendered(error: AppError) -> Vec<String> {
        ToolFailure(error)
            .into_contents()
            .iter()
            .map(|block| block.as_text().unwrap().text.clone())
            .collect()
    }

    #[test]
    fn the_text_is_the_code_in_brackets_then_the_message() {
        assert_eq!(
            rendered(AppError::invalid_argument("query is required.")),
            ["[invalid_arg] query is required."]
        );
        assert_eq!(rendered(AppError::not_found("no such text.")), ["[not_found] no such text."]);
        assert_eq!(
            rendered(AppError::config("bad credentials.")),
            ["[config_error] bad credentials."]
        );
    }

    #[test]
    fn every_code_has_its_wire_name_as_prefix() {
        let errors = [
            AppError::invalid_argument("m"),
            AppError::not_found("m"),
            AppError::Upstream { message: "m".into(), cause: UpstreamCause::Network },
            AppError::RateLimited { message: "m".into(), retry_after: None },
            AppError::config("m"),
            AppError::Audit { message: "m".into(), original: None },
        ];
        let prefixes: Vec<_> =
            ErrorCode::ALL.iter().map(|code| format!("[{}] m", code.as_str())).collect();
        let texts: Vec<_> = errors.into_iter().map(|error| rendered(error).remove(0)).collect();
        assert_eq!(texts, prefixes);
    }

    #[test]
    fn a_cancelled_call_has_no_code_so_only_the_message_is_rendered() {
        let cancelled = AppError::Upstream {
            message: "cancelled by client".into(),
            cause: UpstreamCause::Cancelled,
        };
        assert_eq!(rendered(cancelled), ["cancelled by client"]);
    }

    #[test]
    fn there_is_exactly_one_text_block() {
        let blocks = ToolFailure::from(AppError::invalid_argument("m")).into_contents();
        assert_eq!(blocks.len(), 1);
    }

    /// Les erreurs rendues par un vrai routeur rmcp, sur le fil : `isError: true`, un seul bloc
    /// texte, aucun `structuredContent`.
    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use rmcp::{
            Json,
            handler::server::{common::schema_for_empty_input, router::tool::ToolRoute},
            model::{CallToolRequestParams, JsonObject, Tool},
        };
        use schemars::JsonSchema;
        use serde::Serialize;
        use serde_json::json;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::testing::{Fixture, FixtureServer};

        /// Sortie de la route de test quand aucun code n'est demandé.
        #[derive(Serialize, JsonSchema)]
        struct Ack {
            ok: bool,
        }

        /// Route de test : échoue avec l'exemple du code demandé dans `code`, réussit sans.
        async fn fail(args: JsonObject) -> Result<Json<Ack>, ToolFailure> {
            let requested = args.get("code").and_then(serde_json::Value::as_str);
            match ErrorCode::ALL.into_iter().find(|code| Some(code.as_str()) == requested) {
                Some(code) => Err(ToolFailure(sample(code))),
                None => Ok(Json(Ack { ok: true })),
            }
        }

        fn server() -> FixtureServer {
            let mut server = Fixture::default().server();
            server.router_mut().add_route(ToolRoute::new(
                Tool::new("fail", "fails with the requested code", schema_for_empty_input()),
                fail,
            ));
            server
        }

        #[tokio::test]
        async fn error_results_are_text_with_code_prefix() {
            let client = crate::testing::connect(server()).await;
            for code in ErrorCode::ALL {
                let mut params = CallToolRequestParams::new("fail");
                params.arguments = json!({"code": code.as_str()}).as_object().cloned();
                let result = client.call_tool(params).await.unwrap();
                assert_eq!(result.is_error, Some(true), "{code:?}");
                assert!(result.structured_content.is_none(), "{code:?}");
                assert_eq!(result.content.len(), 1, "{code:?}");
                let text = &result.content[0].as_text().unwrap().text;
                assert!(text.starts_with(&format!("[{}] ", code.as_str())), "{text}");
                assert_eq!(*text, text_of(sample(code)));
            }
        }

        #[tokio::test]
        async fn a_success_keeps_its_structured_content_and_no_error_flag() {
            // Contraste : sans code demandé, la même route rend un succès structuré.
            let client = crate::testing::connect(server()).await;
            let result = client.call_tool(CallToolRequestParams::new("fail")).await.unwrap();
            assert_eq!(result.is_error, Some(false));
            assert_eq!(result.structured_content, Some(json!({"ok": true})));
        }
    }
}
