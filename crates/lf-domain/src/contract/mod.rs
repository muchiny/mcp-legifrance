//! Contrat de citation et conventions de citation françaises.
//!
//! Toute entité citable expose son [`CitationContract`], calculé à la demande
//! par [`Citable::contract`] : `citation` jamais vide (repli `Légifrance {ID}`),
//! `source_url` toujours présente, `ecli` et `eli` seulement s'ils sont natifs,
//! jamais fabriqués. Le champ `eli_uri` de la référence (CONSTITUTION art. 4,
//! `citations.py:8-23`), toujours égal à `source_url`, n'est pas reproduit ; la
//! note [`ELI_NOTE`] le remplace, une seule fois, dans les instructions du
//! serveur et dans `fr_coverage`.
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

use crate::ecli::Ecli;

pub use crate::url::SourceUrl;
pub use eli::{ELI_MAX_CHARS, Eli};
pub use rules::{article_citation, cnil_citation, constit_citation, with_company};
pub use text::{CITATION_MAX_CHARS, CitationText};

/// Note ELI, déclarée une fois dans les instructions du serveur et dans
/// `fr_coverage` (03 §5.3) : valeur `eli_note` de `fr_coverage`
/// (`docs/plan/05-outils-mcp.md` §5.8). Remplace la note de la référence
/// (`models.py:14-17`), qui renvoyait à `eli_uri`.
pub const ELI_NOTE: &str = "The API returns no native ELI for legislation: source_url is the stable Légifrance \
URL, not an ELI. An eli field appears only when the API provides one; never build an /eli/ path.";

/// Contrat de citation porté par toute entité citable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationContract {
    /// Citation selon l'usage français : une ligne, jamais vide, bornée.
    pub citation: CitationText,
    /// Page legifrance.gouv.fr de la ressource.
    pub source_url: SourceUrl,
    /// ECLI natif seulement, jamais fabriqué.
    pub ecli: Option<Ecli>,
    /// ELI natif seulement, jamais fabriqué ; aujourd'hui toujours `None`.
    pub eli: Option<Eli>,
}

impl CitationContract {
    /// Contrat sans ECLI ni ELI : `citation` nettoyée, ou le repli
    /// `Légifrance {id}` si elle est absente ou sans caractère visible.
    pub fn from_url(source_url: SourceUrl, citation: Option<&str>, id: &str) -> Self {
        let citation =
            citation.and_then(CitationText::new).unwrap_or_else(|| CitationText::fallback(id));
        Self { citation, source_url, ecli: None, eli: None }
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
    use crate::url::{ResourceKind, legifrance_url};
    use pretty_assertions::assert_eq;

    const ID: &str = "CETATEXT000054178503";

    fn url() -> SourceUrl {
        legifrance_url(ResourceKind::Cetat, ID)
    }

    #[test]
    fn from_url_cleans_the_citation() {
        let c = CitationContract::from_url(url(), Some(" Conseil d'État,\n2ème chambre "), ID);
        assert_eq!(
            c,
            CitationContract {
                citation: CitationText::new("Conseil d'État, 2ème chambre").unwrap(),
                source_url: url(),
                ecli: None,
                eli: None,
            }
        );
    }

    #[test]
    fn from_url_falls_back_to_the_id() {
        for citation in [None, Some(""), Some(" \n\t\u{0}")] {
            let c = CitationContract::from_url(url(), citation, ID);
            assert_eq!(
                (c.citation.as_str(), c.citation.is_truncated()),
                ("Légifrance CETATEXT000054178503", false),
                "{citation:?}"
            );
        }
    }

    #[test]
    fn eli_note_is_the_coverage_text() {
        // Valeur `eli_note` de `fr_coverage` (05 §5.8), mot pour mot.
        assert_eq!(
            ELI_NOTE,
            "The API returns no native ELI for legislation: source_url is the stable Légifrance \
             URL, not an ELI. An eli field appears only when the API provides one; never build \
             an /eli/ path."
        );
        assert_eq!(ELI_NOTE.chars().count(), 179);
    }
}
