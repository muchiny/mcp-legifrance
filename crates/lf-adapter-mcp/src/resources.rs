//! Resources MCP (`resources/list`, `resources/templates/list`, `resources/read`).
//!
//! STUB(V0) → V3-07 : stubs fonctionnels. Le serveur n'annonce aucune resource en v0.1 ; V3-07
//! écrit les cinq gabarits de 05 §8 (`legifrance://article/{article_id}`…), leur lecture
//! auditée sous `resource:<gabarit>` et les codes d'erreur des deux ères.

use rmcp::{
    ErrorData,
    model::{ListResourceTemplatesResult, ListResourcesResult},
};

/// `resources/list` : aucune resource concrète.
pub(crate) fn list() -> ListResourcesResult {
    ListResourcesResult::default()
}

/// `resources/templates/list` : aucun gabarit.
pub(crate) fn list_templates() -> ListResourceTemplatesResult {
    ListResourceTemplatesResult::default()
}

/// `resources/read` : aucune resource, donc toute URI est inconnue (`-32002`). L'URI demandée
/// n'est pas répétée dans le message.
pub(crate) fn read() -> ErrorData {
    ErrorData::resource_not_found("resource not found", None)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rmcp::model::ErrorCode;

    use super::*;

    #[test]
    fn no_resource_and_no_template_are_listed() {
        assert_eq!(list().resources.len(), 0);
        assert_eq!(list_templates().resource_templates.len(), 0);
    }

    #[test]
    fn reading_any_resource_is_not_found() {
        let error = read();
        assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
        assert_eq!(error.message, "resource not found");
    }
}
