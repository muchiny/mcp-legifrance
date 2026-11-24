//! Sortie de `fr_get_service_public` (jalon MS) : `GetServicePublicOutput` et sa métrique
//! d'audit (05 §5.17).
//!
//! Une page de Service-Public lue : une fiche, avec la lecture déjà faite de son texte, ou une
//! page de navigation (dossier, thème), avec ses entrées et sans texte. La citation et le résumé
//! officiel se tirent de la page ([`GuidanceDoc::citation`]) ; `attribution`, `source_file` et
//! `guidance_note` sont écrits par la vue à partir de la page et de `file`.

use lf_domain::guidance::{GuidanceContent, GuidanceDoc};

use crate::{
    audited::AuditMetric, budget::Reading, output::get_text::returned_chars, ports::ArchiveFile,
};

/// Ce que `GetServicePublicUseCase` rend.
#[derive(Debug, Clone, PartialEq)]
pub struct GetServicePublicOutput {
    /// La page lue. Le texte d'une fiche en est retiré : la lecture est dans
    /// [`GetServicePublicOutput::reading`] ; ses références légales y restent.
    pub doc: GuidanceDoc,
    /// La lecture du texte d'une fiche selon le mode demandé ; `None` pour une page de
    /// navigation.
    pub reading: Option<Reading>,
    /// L'archive du public lu.
    pub file: ArchiveFile,
}

impl GetServicePublicOutput {
    /// Nom de l'entrée lue dans l'archive, dans les termes de la DILA (`F1234.xml`).
    #[must_use]
    pub fn entry(&self) -> String {
        format!("{}.xml", self.doc.id)
    }
}

/// Métrique d'audit (05 §5.17) : les caractères renvoyés pour une fiche (`summary`, `text` et
/// `passages[].text`, hors métadonnées), le nombre d'entrées pour une page de navigation.
impl AuditMetric for GetServicePublicOutput {
    fn output_count_or_size(&self) -> u64 {
        let size = match (&self.reading, &self.doc.content) {
            (Some(reading), _) => returned_chars(reading),
            (None, GuidanceContent::Folder(entries) | GuidanceContent::Theme(entries)) => {
                entries.len()
            }
            (None, GuidanceContent::Sheet { .. }) => 0,
        };
        u64::try_from(size).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::guidance::Audience;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::fixtures::service_public::{entry, file, folder, sheet};

    fn reading(text: &str) -> Reading {
        Reading {
            summary: None,
            text: Some(text.to_owned()),
            text_length: 40,
            next_offset: None,
            passages: None,
            note: Some("la remarque n'est pas comptée".to_owned()),
            summary_truncated: false,
        }
    }

    #[test]
    fn a_sheet_counts_the_characters_returned_and_names_its_entry() {
        let output = GetServicePublicOutput {
            doc: sheet(Audience::Particuliers, "F1437", ""),
            reading: Some(reading("Œuvre d'art")),
            file: file(Audience::Particuliers),
        };
        assert_eq!(output.output_count_or_size(), 11);
        assert_eq!(output.entry(), "F1437.xml");
    }

    #[test]
    fn a_folder_counts_its_entries() {
        let entries = vec![entry("F1437", "Compte"), entry("F2408", "Activité")];
        let output = GetServicePublicOutput {
            doc: folder(Audience::Entreprendre, "N31901", entries),
            reading: None,
            file: file(Audience::Entreprendre),
        };
        assert_eq!(output.output_count_or_size(), 2);
        assert_eq!(output.entry(), "N31901.xml");
        // Un thème aussi.
        let theme = GetServicePublicOutput {
            doc: GuidanceDoc {
                content: GuidanceContent::Theme(vec![entry("N19803", "Argent")]),
                ..output.doc.clone()
            },
            ..output
        };
        assert_eq!(theme.output_count_or_size(), 1);
        // Une fiche sans lecture (forme que le cas d'usage ne rend pas) ne compte rien.
        let unread = GetServicePublicOutput {
            doc: sheet(Audience::Particuliers, "F1437", "Texte."),
            reading: None,
            file: file(Audience::Particuliers),
        };
        assert_eq!(unread.output_count_or_size(), 0);
    }
}
