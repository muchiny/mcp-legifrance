//! Sortie de `fr_get_judilibre` (jalon MJ) : `GetJudilibreOutput` et sa métrique d'audit (05
//! §5.19).
//!
//! La décision lue (métadonnées, mentions, liens) et la [`Reading`] déjà calculée par le cas
//! d'usage, entière ou limitée à une partie. La citation se tire de la décision
//! ([`lf_domain::judilibre::CaseLawMention::citation`]) ; `attribution` et `case_law_note` sont
//! des constantes de la vue.

use lf_domain::judilibre::{CaseLawDoc, DecisionPart};

use crate::{audited::AuditMetric, budget::Reading, output::get_text::returned_chars};

/// Ce que `GetJudilibreUseCase` rend.
#[derive(Debug, Clone, PartialEq)]
pub struct GetJudilibreOutput {
    /// La décision. Son champ `text` est vidé et ses zones aussi : la lecture est dans
    /// [`GetJudilibreOutput::reading`].
    pub decision: CaseLawDoc,
    /// La partie lue, si elle était demandée **et** délimitée dans la décision ; `None` pour le
    /// texte entier.
    pub part: Option<DecisionPart>,
    /// Résumé, fenêtre de texte ou passages, selon les paramètres de lecture.
    pub reading: Reading,
}

impl AuditMetric for GetJudilibreOutput {
    /// Caractères renvoyés (05 §5.0.1).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(returned_chars(&self.reading)).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::text::{Summary, SummarySource};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::fixtures::judilibre;

    #[test]
    fn the_metric_counts_the_returned_characters() {
        let reading = Reading {
            summary: Some(Summary {
                source: SummarySource::Dispositif,
                text: "CASSE ET ANNULE".to_owned(),
                truncated: false,
            }),
            text: Some("été".to_owned()),
            text_length: 9_999,
            next_offset: None,
            passages: None,
            note: Some("Note.".to_owned()),
            summary_truncated: false,
        };
        let output =
            GetJudilibreOutput { decision: judilibre::doc(judilibre::CC_ID), part: None, reading };
        assert_eq!(output.output_count_or_size(), 18);
    }
}
