//! Échec d'un outil rendu in-band (05 §3.2, §7).
//!
//! Une erreur d'outil n'est pas une erreur JSON-RPC : le client reçoit un résultat `isError: true`
//! dont l'unique bloc texte est `[code] message`, sans `structuredContent`. Le code et le message
//! viennent de l'`AppError` (`lf_application::messages` rédige les textes, 02 §9.3).
#![allow(dead_code, reason = "STUB(V0) → V1 : employé par les tranches d'outils (tools/*.rs)")]

use lf_application::error::AppError;
use rmcp::model::{ContentBlock, IntoContents};

/// L'`AppError` d'un appel d'outil, prête à être rendue : type d'erreur de
/// `Result<Json<V>, ToolFailure>`. rmcp met `isError: true` sur le résultat dès que le retour
/// est un `Err` d'`IntoContents`.
#[derive(Debug)]
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
    use super::*;
    use lf_application::error::{ErrorCode, UpstreamCause};
    use pretty_assertions::assert_eq;

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
}
