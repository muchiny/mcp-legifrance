//! Noms des outils MCP de la v0.1 (02 §3.2, `tools.rs`).
//!
//! Source unique des noms : l'audit les inscrit (`AuditRecord::tool`) et l'adapter MCP les
//! enregistre. [`ToolName::ALL`] sert au test de dérive entre les deux.
//!
//! Les outils des jalons suivants (M6 et M7) ont leur constante dès la vague 0 ; `ALL` ne les
//! compte qu'à l'intégration qui enregistre l'outil. La lecture d'une resource est auditée sous
//! `resource:<gabarit>` (`RESOURCE_*`, 05 §8.2) : ces noms ne sont pas des outils et ne figurent
//! jamais dans `ALL`.

/// Nom d'un outil MCP (préfixé `fr_`) ou d'une lecture de resource (préfixé `resource:`).
///
/// Le champ est privé : on ne fabrique un nom que par les constantes ci-dessous, ce qui
/// interdit qu'une ligne d'audit porte un nom inventé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolName(&'static str);

impl ToolName {
    /// `fr_search`.
    pub const FR_SEARCH: Self = Self("fr_search");
    /// `fr_get_act`.
    pub const FR_GET_ACT: Self = Self("fr_get_act");
    /// `fr_get_text`.
    pub const FR_GET_TEXT: Self = Self("fr_get_text");
    /// `fr_get_decision`.
    pub const FR_GET_DECISION: Self = Self("fr_get_decision");
    /// `fr_get_deliberation`.
    pub const FR_GET_DELIBERATION: Self = Self("fr_get_deliberation");
    /// `fr_get_convention`.
    pub const FR_GET_CONVENTION: Self = Self("fr_get_convention");
    /// `fr_get_company_agreement`.
    pub const FR_GET_COMPANY_AGREEMENT: Self = Self("fr_get_company_agreement");
    /// `fr_coverage`.
    pub const FR_COVERAGE: Self = Self("fr_coverage");
    /// `fr_verify_citations` (M6).
    pub const FR_VERIFY_CITATIONS: Self = Self("fr_verify_citations");
    /// `fr_resolve` (M6).
    pub const FR_RESOLVE: Self = Self("fr_resolve");
    /// `fr_get_related` (M7).
    pub const FR_GET_RELATED: Self = Self("fr_get_related");
    /// `fr_diff_article` (M7).
    pub const FR_DIFF_ARTICLE: Self = Self("fr_diff_article");
    /// `fr_search_all` (M7).
    pub const FR_SEARCH_ALL: Self = Self("fr_search_all");

    /// Lecture de la resource `legifrance://article/{article_id}` (M7), nom d'audit
    /// `resource:article`.
    pub const RESOURCE_ARTICLE: Self = Self("resource:article");
    /// Lecture de `legifrance://decision/{decision_id}` (M7), nom d'audit `resource:decision`.
    pub const RESOURCE_DECISION: Self = Self("resource:decision");
    /// Lecture de `legifrance://act/{text_id}` (M7), nom d'audit `resource:act`.
    pub const RESOURCE_ACT: Self = Self("resource:act");
    /// Lecture de `legifrance://code/{code}/article/{num}` (M7), nom d'audit
    /// `resource:code_article`.
    pub const RESOURCE_CODE_ARTICLE: Self = Self("resource:code_article");
    /// Lecture de `legifrance://coverage` (M7), nom d'audit `resource:coverage`.
    pub const RESOURCE_COVERAGE: Self = Self("resource:coverage");

    /// Les outils enregistrés, dans l'ordre du catalogue (05 §3) : les huit de la v0.1. Tout
    /// nouvel outil s'y ajoute à l'intégration qui l'enregistre.
    pub const ALL: [Self; 8] = [
        Self::FR_SEARCH,
        Self::FR_GET_ACT,
        Self::FR_GET_TEXT,
        Self::FR_GET_DECISION,
        Self::FR_GET_DELIBERATION,
        Self::FR_GET_CONVENTION,
        Self::FR_GET_COMPANY_AGREEMENT,
        Self::FR_COVERAGE,
    ];

    /// Le nom tel qu'il apparaît dans `tools/list` et dans le journal d'audit.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn tool_names_are_unique_and_prefixed() {
        let names: Vec<&str> = ToolName::ALL.iter().map(|tool| tool.as_str()).collect();
        assert_eq!(
            names,
            [
                "fr_search",
                "fr_get_act",
                "fr_get_text",
                "fr_get_decision",
                "fr_get_deliberation",
                "fr_get_convention",
                "fr_get_company_agreement",
                "fr_coverage",
            ]
        );
        assert!(names.iter().all(|name| name.starts_with("fr_")));
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len());
    }

    #[test]
    fn later_tools_and_resources_have_constants_outside_all() {
        let later = [
            (ToolName::FR_VERIFY_CITATIONS, "fr_verify_citations"),
            (ToolName::FR_RESOLVE, "fr_resolve"),
            (ToolName::FR_GET_RELATED, "fr_get_related"),
            (ToolName::FR_DIFF_ARTICLE, "fr_diff_article"),
            (ToolName::FR_SEARCH_ALL, "fr_search_all"),
            (ToolName::RESOURCE_ARTICLE, "resource:article"),
            (ToolName::RESOURCE_DECISION, "resource:decision"),
            (ToolName::RESOURCE_ACT, "resource:act"),
            (ToolName::RESOURCE_CODE_ARTICLE, "resource:code_article"),
            (ToolName::RESOURCE_COVERAGE, "resource:coverage"),
        ];
        for (tool, name) in later {
            assert_eq!(tool.as_str(), name);
            assert!(!ToolName::ALL.contains(&tool), "{name}");
        }
        let names: HashSet<&str> = later.iter().map(|(_, name)| *name).collect();
        assert_eq!(names.len(), later.len());
    }

    #[test]
    fn tool_name_all_matches_constants() {
        let constants = [
            ToolName::FR_SEARCH,
            ToolName::FR_GET_ACT,
            ToolName::FR_GET_TEXT,
            ToolName::FR_GET_DECISION,
            ToolName::FR_GET_DELIBERATION,
            ToolName::FR_GET_CONVENTION,
            ToolName::FR_GET_COMPANY_AGREEMENT,
            ToolName::FR_COVERAGE,
        ];
        assert_eq!(ToolName::ALL, constants);
        assert_eq!(ToolName::ALL.into_iter().collect::<HashSet<_>>().len(), constants.len());
    }
}
