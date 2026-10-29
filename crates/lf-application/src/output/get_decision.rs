//! Sortie de `fr_get_decision` (v0.1) : `GetDecisionOutput` et sa métrique d'audit.
//!
//! La sortie porte la décision lue (métadonnées, citation, ECLI natif) et la [`Reading`] déjà
//! calculée par le cas d'usage : l'adapter MCP n'a plus qu'à la mettre en forme. La décision garde
//! son texte intégral ; l'adapter n'en lit que les métadonnées, le texte rendu est `reading.text`.

use lf_domain::entity::Decision;

use crate::{audited::AuditMetric, budget::Reading};

/// Ce que `GetDecisionUseCase` rend : la décision lue et ce qu'on en montre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetDecisionOutput {
    /// La décision, telle que le corpus l'a lue.
    pub decision: Decision,
    /// Résumé, fenêtre de texte ou passages, selon les paramètres de lecture.
    pub reading: Reading,
}

/// Métrique d'audit : les caractères renvoyés (05 §5.0.1), soit la somme des caractères de
/// `summary`, de `text` et des `passages[].text`, hors métadonnées.
impl AuditMetric for GetDecisionOutput {
    fn output_count_or_size(&self) -> u64 {
        let reading = &self.reading;
        let summary = reading.summary.as_ref().map(|summary| summary.text.as_str());
        let passages = reading.passages.iter().flatten().map(|passage| passage.text.as_str());
        let chars = summary
            .into_iter()
            .chain(reading.text.as_deref())
            .chain(passages)
            .map(|text| text.chars().count())
            .fold(0_usize, usize::saturating_add);
        u64::try_from(chars).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::PassageOut;
    use lf_domain::{
        entity::OfficialSummary,
        id::DecisionId,
        text::{LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;

    fn output(reading: Reading) -> GetDecisionOutput {
        GetDecisionOutput {
            decision: Decision {
                decision_id: DecisionId::parse("CONSTEXT000052555275").unwrap(),
                ecli: None,
                num: None,
                juridiction: None,
                formation: None,
                solution: None,
                numero_affaire: Vec::new(),
                date_texte: None,
                nature: None,
                title: None,
                text: LongText::default(),
                official_summary: OfficialSummary::default(),
            },
            reading,
        }
    }

    fn bare() -> Reading {
        Reading {
            summary: None,
            text: None,
            text_length: 14_332,
            next_offset: None,
            passages: None,
            note: None,
        }
    }

    fn passage(text: &str) -> PassageOut {
        PassageOut { offset: 0, text: text.to_owned() }
    }

    #[test]
    fn the_metric_counts_each_returned_text_in_characters() {
        // Un rendu sans texte vaut 0, quelle que soit la longueur de la décision.
        assert_eq!(output(bare()).output_count_or_size(), 0);
        let summary = Summary {
            source: SummarySource::Dispositif,
            text: "Article 1er.".to_owned(),
            truncated: false,
        };
        let summarized = Reading { summary: Some(summary), ..bare() };
        assert_eq!(output(summarized).output_count_or_size(), 12);
        let windowed = Reading { text: Some("Première page. ".to_owned()), ..bare() };
        assert_eq!(output(windowed).output_count_or_size(), 15);
        let passages =
            Reading { passages: Some(vec![passage("Dès lors,"), passage("")]), ..bare() };
        assert_eq!(output(passages).output_count_or_size(), 9);
    }

    #[test]
    fn the_metric_counts_characters_not_bytes_and_ignores_the_note() {
        // « é » et l'emoji hors BMP valent un caractère chacun.
        let reading = Reading {
            text: Some("été😀".to_owned()),
            passages: Some(vec![passage("œ")]),
            note: Some("No passage matches focus; read the text with detail=\"full\".".to_owned()),
            ..bare()
        };
        assert_eq!(output(reading).output_count_or_size(), 5);
    }

    #[test]
    fn every_returned_text_adds_up() {
        let reading = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "abc".to_owned(),
                truncated: true,
            }),
            text: Some("defg".to_owned()),
            passages: Some(vec![passage("hi"), passage("jkl")]),
            ..bare()
        };
        assert_eq!(output(reading).output_count_or_size(), 12);
    }
}
