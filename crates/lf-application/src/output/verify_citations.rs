//! Sortie de `fr_verify_citations` (v0.2 (M6)) : `VerifyCitationsOutput` et sa métrique d'audit
//! (05 §5.9, annexe B §7.7).
//!
//! Une entrée par citation reconnue (au plus `max_citations`), dans l'ordre du texte. Le `span` de
//! chaque entrée est en **points de code Unicode** du texte reçu, et la note est une valeur
//! structurée que la vue de `lf-adapter-mcp` rend en anglais.

use crate::{audited::AuditMetric, citation::CitationCheck};

/// Réponse de `fr_verify_citations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyCitationsOutput {
    /// Citations reconnues dans le texte, y compris celles qui ne sont pas contrôlées.
    pub found: usize,
    /// Citations contrôlées et rendues, dans l'ordre du texte.
    pub checked: usize,
    /// Vrai si `found` dépasse `checked` : `max_citations` ou le plafond de réponse (05 §6.4).
    pub truncated: bool,
    /// Les contrôles, un par citation rendue.
    pub citations: Vec<CitationCheck>,
    /// Remarque exceptionnelle : liste raccourcie pour tenir sous le plafond de réponse.
    pub note: Option<String>,
}

impl AuditMetric for VerifyCitationsOutput {
    /// Nombre de citations contrôlées (`checked`, 05 §5.9).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.checked).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citation::{CheckStatus, Note};
    use lf_domain::citation::{CitationKind, Span};
    use pretty_assertions::assert_eq;

    fn check() -> CitationCheck {
        CitationCheck {
            span: Span { start: 0, end: 10 },
            raw: "article 9".to_owned(),
            kind: CitationKind::CodeArticle,
            status: CheckStatus::Unresolvable,
            resolved: None,
            quote: None,
            note: Some(Note::NoAgreement),
        }
    }

    #[test]
    fn the_audit_metric_is_the_number_checked() {
        let output = VerifyCitationsOutput {
            found: 7,
            checked: 2,
            truncated: true,
            citations: vec![check(), check()],
            note: None,
        };
        assert_eq!(output.output_count_or_size(), 2);
        let empty = VerifyCitationsOutput {
            found: 0,
            checked: 0,
            truncated: false,
            citations: vec![],
            note: None,
        };
        assert_eq!(empty.output_count_or_size(), 0);
    }
}
