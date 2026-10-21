//! Contrat de citation et conventions de citation françaises.
//!
//! CONSTITUTION art. 4 de la référence : toute entité citable porte `eli_uri`,
//! `human_readable_citation` et `source_url`. PISTE renvoie un ELI nul pour la
//! législation : `eli_uri` porte donc l'URL Légifrance stable, jamais un ELI
//! fabriqué (`citations.py:8-23`).
//!
//! Le module s'appelle `contract`, pas `citation` : ce nom revient au parseur
//! de citations (M6, ADR-0009). Les règles de format vivent dans `rules`,
//! ré-exportées ici.
//!
//! Newtypes du contrat v2 (ADR-0009, décision C2) : [`CitationText`] (citation
//! sur une ligne, jamais vide, bornée), [`SourceUrl`] (page Légifrance
//! construite depuis un identifiant typé) et [`Eli`] (ELI natif validé, jamais
//! fabriqué).

mod eli;
mod rules;
mod text;

pub use crate::url::SourceUrl;
pub use eli::{ELI_MAX_CHARS, Eli};
pub use rules::{article_citation, cnil_citation, constit_citation, with_company};
pub use text::{CITATION_MAX_CHARS, CitationText};

/// Note jointe aux résultats de législation (`models.py:14-17`).
pub const ELI_NOTE: &str = "Legifrance lf-engine-app returns the ELI field null for legislation; \
'eli_uri' is the stable CID-keyed legifrance.gouv.fr resource URL, not a synthesized /eli/ identifier.";

/// Contrat de citation porté par toute entité citable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationContract {
    /// URL Légifrance stable (pas d'ELI fabriqué).
    pub eli_uri: String,
    /// Citation selon l'usage français, absente si la source ne la permet pas.
    pub human_readable_citation: Option<String>,
    /// Page legifrance.gouv.fr de la ressource.
    pub source_url: String,
}

impl CitationContract {
    /// `eli_uri` et `source_url` reçoivent tous deux `url`.
    pub fn from_url(url: String, human_readable_citation: Option<String>) -> Self {
        Self { eli_uri: url.clone(), human_readable_citation, source_url: url }
    }
}

/// Entité qui sait produire son contrat de citation.
pub trait Citable {
    /// Contrat de citation de l'entité.
    fn contract(&self) -> CitationContract;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn contract_uses_url_twice() {
        let c = CitationContract::from_url("https://u".into(), Some("Cit".into()));
        assert_eq!(c.eli_uri, "https://u");
        assert_eq!(c.source_url, "https://u");
        assert_eq!(c.human_readable_citation.as_deref(), Some("Cit"));
    }
}
