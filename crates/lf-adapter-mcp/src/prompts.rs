//! Prompts MCP (`prompts/list`, `prompts/get`).
//!
//! STUB(V0) → V3-07 : stubs fonctionnels. Le serveur n'annonce aucun prompt en v0.1 ; V3-07
//! écrit les prompts, en français (Q14).

use rmcp::{ErrorData, model::ListPromptsResult};

/// `prompts/list` : aucun prompt.
pub(crate) fn list() -> ListPromptsResult {
    ListPromptsResult::default()
}

/// `prompts/get` : aucun prompt, donc tout nom est inconnu (`-32602`, 05 §8). Le nom demandé n'est
/// pas répété dans le message.
pub(crate) fn get() -> ErrorData {
    ErrorData::invalid_params("unknown prompt", None)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rmcp::model::ErrorCode;

    use super::*;

    #[test]
    fn no_prompt_is_listed() {
        assert_eq!(list().prompts.len(), 0);
    }

    #[test]
    fn getting_any_prompt_is_invalid_params() {
        let error = get();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "unknown prompt");
    }
}
