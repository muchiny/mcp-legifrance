//! Sortie de `fr_get_bofip` (jalon MB) : `GetBofipOutput` et sa métrique d'audit (05 §5.15).
//!
//! Deux formes : une publication lue ([`BofipPublication`] : ses métadonnées et la lecture déjà
//! faite de son texte), ou la liste des publications qui portent le même identifiant
//! ([`BofipChoice`]), sans aucun texte, quand l'appel ne dit pas laquelle lire. La citation se
//! tire de la publication ([`lf_domain::doctrine::DoctrineDoc::citation`]) ; `attribution` et
//! `doctrine_note` sont des constantes de la vue.

use lf_domain::{
    date::CivilDate,
    doctrine::{BofipUrl, BoiId, DoctrineDoc, PublicationId},
};

use crate::{audited::AuditMetric, budget::Reading, output::get_text::returned_chars};

/// Ce que `GetBofipUseCase` rend.
#[derive(Debug, Clone, PartialEq)]
pub enum GetBofipOutput {
    /// Une publication, lue selon le mode demandé.
    Publication(Box<BofipPublication>),
    /// Plusieurs publications portent l'identifiant : leur liste, sans texte.
    Choice(BofipChoice),
}

impl GetBofipOutput {
    /// La publication lue ; `None` pour une liste.
    #[must_use]
    pub fn publication(&self) -> Option<&BofipPublication> {
        match self {
            Self::Publication(publication) => Some(publication),
            Self::Choice(_) => None,
        }
    }
}

/// Une publication et sa lecture.
#[derive(Debug, Clone, PartialEq)]
pub struct BofipPublication {
    /// Les métadonnées de la publication. Son champ `text` est vidé : la lecture est dans
    /// [`BofipPublication::reading`].
    pub doc: DoctrineDoc,
    /// La lecture du texte selon le mode demandé : résumé, texte plein ou passages. Pour une
    /// publication sans texte : `text_length` à 0 et une remarque.
    pub reading: Reading,
}

/// Les publications qui portent un même identifiant, quand l'appel n'en choisit aucune.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BofipChoice {
    /// L'identifiant demandé.
    pub id: BoiId,
    /// Les publications, par numéro croissant.
    pub publications: Vec<PublicationRef>,
    /// Remarque : comment en lire une.
    pub note: String,
}

/// Une publication dans la liste de [`BofipChoice`] : de quoi la choisir, jamais son texte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationRef {
    /// Numéro de la publication : la valeur à repasser dans `publication_id`.
    pub publication_id: PublicationId,
    /// Titre, sur une ligne.
    pub title: String,
    /// Début de validité, s'il est connu.
    pub valid_from: Option<CivilDate>,
    /// Adresse sur le site du BOFiP, si elle est sûre à rendre.
    pub source_url: Option<BofipUrl>,
}

impl From<DoctrineDoc> for PublicationRef {
    /// Garde de la publication ce qui sert à la choisir ; son texte est lâché.
    fn from(doc: DoctrineDoc) -> Self {
        Self {
            publication_id: doc.publication_id,
            title: doc.title,
            valid_from: doc.valid_from,
            source_url: doc.source_url,
        }
    }
}

/// Métrique d'audit (05 §5.15) : les caractères renvoyés pour une publication lue (`summary`,
/// `text` et `passages[].text`, hors métadonnées), le nombre de publications pour une liste.
impl AuditMetric for GetBofipOutput {
    fn output_count_or_size(&self) -> u64 {
        let size = match self {
            Self::Publication(publication) => returned_chars(&publication.reading),
            Self::Choice(choice) => choice.publications.len(),
        };
        u64::try_from(size).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::text::{Summary, SummarySource};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{budget::PassageOut, testing::fixtures::bofip};

    fn reading(text_length: u32) -> Reading {
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

    fn publication(reading: Reading) -> GetBofipOutput {
        GetBofipOutput::Publication(Box::new(BofipPublication {
            doc: bofip::doc(3350, ""),
            reading,
        }))
    }

    #[test]
    fn a_publication_counts_the_characters_returned() {
        assert_eq!(publication(reading(167_535)).output_count_or_size(), 0);
        let summary =
            Summary { source: SummarySource::Lead, text: "abcde".to_owned(), truncated: true };
        let full = Reading {
            summary: Some(summary),
            text: Some("Donation œuvre".to_owned()),
            passages: Some(vec![PassageOut { offset: 0, text: "klm".to_owned(), score: 1.0 }]),
            note: Some("la remarque n'est pas comptée".to_owned()),
            ..reading(40)
        };
        assert_eq!(publication(full).output_count_or_size(), 5 + 14 + 3);
    }

    #[test]
    fn a_choice_counts_the_publications_listed() {
        let publications: Vec<PublicationRef> =
            [3350, 12_001].map(|number| bofip::doc(number, "texte").into()).into();
        let choice = BofipChoice { id: bofip::id(), publications, note: "note".to_owned() };
        assert_eq!(GetBofipOutput::Choice(choice).output_count_or_size(), 2);
    }

    #[test]
    fn only_a_reading_has_a_publication() {
        let read = BofipPublication { doc: bofip::doc(3350, ""), reading: reading(12) };
        let output = GetBofipOutput::Publication(Box::new(read.clone()));
        assert_eq!(output.publication(), Some(&read));
        let choice = BofipChoice { id: bofip::id(), publications: Vec::new(), note: String::new() };
        assert_eq!(GetBofipOutput::Choice(choice).publication(), None);
    }

    #[test]
    fn a_listed_publication_keeps_what_identifies_it_and_drops_the_text() {
        let doc = bofip::doc(3350, "Un texte que la liste ne rend pas.");
        let listed = PublicationRef::from(doc.clone());
        assert_eq!(
            listed,
            PublicationRef {
                publication_id: doc.publication_id,
                title: doc.title,
                valid_from: doc.valid_from,
                source_url: doc.source_url,
            }
        );
        assert_eq!(listed.publication_id.get(), 3350);
        assert!(listed.source_url.is_some() && listed.valid_from.is_some());
    }
}
