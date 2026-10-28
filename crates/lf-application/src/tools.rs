//! Noms des outils MCP de la v0.1 (02 §3.2, `tools.rs`).
//!
//! Source unique des noms : l'audit les inscrit (`AuditRecord::tool`) et l'adapter MCP les
//! enregistre. [`ToolName::ALL`] sert au test de dérive entre les deux.

/// Nom d'un outil MCP, toujours préfixé `fr_`.
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

    /// Les outils de la v0.1, dans l'ordre du catalogue (05 §3) ; tout nouvel outil s'y ajoute.
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
