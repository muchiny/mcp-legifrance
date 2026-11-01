//! Sortie de `fr_get_deliberation` (v0.1) : `GetDeliberationOutput` et sa métrique d'audit.
//!
//! La sortie porte les métadonnées de la délibération et la lecture déjà faite de son texte
//! ([`Reading`] : résumé, fenêtre ou passages, selon le mode demandé, 05 §5.0.3). Le texte intégral
//! n'y reste pas : le cas d'usage le lit puis le lâche, la lecture en a gardé ce qu'il faut. La
//! citation et l'URL source viennent de `Citable::contract`, tirées par l'adapter MCP.

use lf_domain::entity::Deliberation;

use crate::{audited::AuditMetric, budget::Reading};

/// Ce que `GetDeliberationUseCase` rend : la délibération (texte vidé) et sa lecture.
#[derive(Debug, Clone, PartialEq)]
pub struct GetDeliberationOutput {
    /// Les métadonnées de la délibération. Son champ `text` est vidé : la lecture est dans
    /// [`GetDeliberationOutput::reading`].
    pub deliberation: Deliberation,
    /// La lecture du texte selon le mode demandé : résumé, texte plein ou passages.
    pub reading: Reading,
}

/// Métrique d'audit : les caractères renvoyés (05 §5.0.1), soit la somme des caractères de `text`,
/// de `summary` et des `passages[].text`, hors métadonnées.
impl AuditMetric for GetDeliberationOutput {
    fn output_count_or_size(&self) -> u64 {
        let reading = &self.reading;
        let summary = reading.summary.as_ref().map(|summary| summary.text.as_str());
        let passages = reading.passages.iter().flatten().map(|passage| passage.text.as_str());
        let returned: usize = summary
            .into_iter()
            .chain(reading.text.as_deref())
            .chain(passages)
            .map(|text| text.chars().count())
            .sum();
        u64::try_from(returned).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::{
        id::CnilTextId,
        text::{LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::budget::PassageOut;

    fn output(reading: Reading) -> GetDeliberationOutput {
        GetDeliberationOutput {
            deliberation: Deliberation {
                deliberation_id: CnilTextId::parse("CNILTEXT000053352643").unwrap(),
                num: None,
                nature: None,
                nature_delib: None,
                etat: None,
                date_texte: None,
                date_publication: None,
                title: None,
                title_long: None,
                text: LongText::default(),
            },
            reading,
        }
    }

    fn bare(text_length: u32) -> Reading {
        Reading {
            summary: None,
            text: None,
            text_length,
            next_offset: None,
            passages: None,
            note: None,
            summary_truncated: false,
        }
    }

    #[test]
    fn a_reading_without_content_returns_no_character() {
        assert_eq!(output(bare(0)).output_count_or_size(), 0);
        // La longueur du texte entier n'est pas ce qui est renvoyé.
        assert_eq!(output(bare(14_332)).output_count_or_size(), 0);
    }

    #[test]
    fn the_metric_counts_characters_not_bytes() {
        let reading =
            Reading { text: Some("Délibéré à Paris, œuvre 😀".to_owned()), ..bare(25) };
        assert_eq!(output(reading).output_count_or_size(), 25);
    }

    #[test]
    fn the_metric_adds_summary_text_and_passages() {
        let summary =
            Summary { source: SummarySource::Lead, text: "abcde".to_owned(), truncated: true };
        let reading = Reading {
            summary: Some(summary),
            text: Some("fghij".to_owned()),
            passages: Some(vec![
                PassageOut { offset: 0, text: "klm".to_owned(), score: 1.0 },
                PassageOut { offset: 9, text: "no".to_owned(), score: 1.0 },
            ]),
            note: Some("la remarque n'est pas comptée".to_owned()),
            next_offset: Some(5),
            ..bare(40)
        };
        assert_eq!(output(reading).output_count_or_size(), 5 + 5 + 3 + 2);
    }

    #[test]
    fn an_empty_passage_list_returns_no_character() {
        let reading = Reading { passages: Some(Vec::new()), ..bare(10) };
        assert_eq!(output(reading).output_count_or_size(), 0);
    }
}
