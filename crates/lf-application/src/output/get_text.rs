//! Sortie de `fr_get_text` (v0.1) : `GetTextOutput` et sa métrique d'audit.
//!
//! Les métadonnées de l'article et la lecture (`Reading`, commune aux trois lectures longues)
//! voyagent ensemble ; l'adapter MCP tire la citation et l'URL source de l'article par
//! `Citable::contract`.

use lf_domain::entity::ArticleText;

use crate::{audited::AuditMetric, budget::Reading};

/// Longueur maximale, en caractères, du `nota` rendu (05 §5.3). Au-delà, il est coupé comme une
/// fenêtre de lecture : en fin de phrase ou de mot.
pub const NOTA_MAX_CHARS: usize = 1_200;

/// Ce que `fr_get_text` rend à l'adapter : l'article, son `nota` borné et la lecture demandée.
#[derive(Debug, Clone, PartialEq)]
pub struct GetTextOutput {
    /// L'article lu. Son texte et son `nota` sont vidés : la lecture est dans `reading` et le
    /// `nota` borné dans `nota`.
    pub article: ArticleText,
    /// Le `nota` de l'article, d'au plus [`NOTA_MAX_CHARS`] caractères ; absent s'il n'y en a pas.
    pub nota: Option<String>,
    /// La lecture : résumé, fenêtre de texte ou passages, avec `text_length` et `next_offset`.
    pub reading: Reading,
}

/// Caractères renvoyés par une lecture : la somme des caractères de `text`, de `summary` et des
/// `passages[].text`, hors métadonnées (05 §5.0.1).
pub(crate) fn returned_chars(reading: &Reading) -> usize {
    let summary = reading.summary.as_ref().map_or(0, |summary| summary.text.chars().count());
    let text = reading.text.as_ref().map_or(0, |text| text.chars().count());
    let passages = reading.passages.iter().flatten().map(|passage| passage.text.chars().count());
    summary.saturating_add(text).saturating_add(passages.sum::<usize>())
}

impl AuditMetric for GetTextOutput {
    /// Caractères renvoyés (05 §5.0.1).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(returned_chars(&self.reading)).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::PassageOut;
    use lf_domain::{
        id::ArticleId,
        text::{LongText, Summary, SummarySource},
    };
    use pretty_assertions::assert_eq;

    fn reading() -> Reading {
        Reading {
            summary: None,
            text: None,
            text_length: 9_999,
            next_offset: None,
            passages: None,
            note: Some("a note that is not counted".to_owned()),
            summary_truncated: false,
        }
    }

    fn output(reading: Reading) -> GetTextOutput {
        GetTextOutput {
            article: ArticleText {
                article_id: ArticleId::parse("LEGIARTI000006419288").unwrap(),
                num: None,
                etat: None,
                code_title: None,
                parent: None,
                path: Vec::new(),
                full_sections_titre: None,
                date_debut: None,
                date_fin: None,
                text: LongText::default(),
                nota: None,
            },
            nota: Some("a nota that is not counted".to_owned()),
            reading,
        }
    }

    #[test]
    fn the_metric_counts_the_characters_of_text_summary_and_passages_only() {
        assert_eq!(output(reading()).output_count_or_size(), 0);
        // Les caractères, pas les octets : « é » et « œ » en comptent un chacun.
        let text = Reading { text: Some("Pièce œuvre".to_owned()), ..reading() };
        assert_eq!(output(text).output_count_or_size(), 11);
        let summary = Reading {
            summary: Some(Summary {
                source: SummarySource::Lead,
                text: "Début".to_owned(),
                truncated: true,
            }),
            ..reading()
        };
        assert_eq!(output(summary).output_count_or_size(), 5);
        let passages = Reading {
            passages: Some(vec![
                PassageOut { offset: 0, text: "un".to_owned(), score: 1.0 },
                PassageOut { offset: 40, text: "trois".to_owned(), score: 1.0 },
            ]),
            ..reading()
        };
        assert_eq!(output(passages).output_count_or_size(), 7);
    }

    #[test]
    fn text_and_summary_add_up_when_both_are_present() {
        // Un texte court accompagné d'un sommaire officiel : les deux sont rendus.
        let both = Reading {
            summary: Some(Summary {
                source: SummarySource::Official,
                text: "abc".to_owned(),
                truncated: false,
            }),
            text: Some("defgh".to_owned()),
            ..reading()
        };
        assert_eq!(returned_chars(&both), 8);
    }
}
