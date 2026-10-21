//! Délibération CNIL (`fr_get_deliberation`, `citations.py:363-395`).

use crate::{
    contract::{Citable, CitationContract, cnil_citation},
    date::CivilDate,
    id::CnilTextId,
    url::{ResourceKind, legifrance_url},
};

/// Délibération de la CNIL ; ECLI et ELI toujours nuls côté API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deliberation {
    /// Identifiant.
    pub deliberation_id: CnilTextId,
    /// Numéro, ex. `SAN-2026-002`.
    pub num: Option<String>,
    /// Nature.
    pub nature: Option<String>,
    /// Nature de délibération.
    pub nature_delib: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// Date de la délibération.
    pub date_texte: Option<CivilDate>,
    /// Date de publication.
    pub date_publication: Option<CivilDate>,
    /// Titre court.
    pub title: Option<String>,
    /// Titre long.
    pub title_long: Option<String>,
    /// Texte intégral.
    pub text: Option<String>,
}

impl Citable for Deliberation {
    fn contract(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Cnil, self.deliberation_id.as_str()).to_string(),
            cnil_citation(self.num.as_deref(), self.date_texte).or_else(|| self.title.clone()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn deliberation(num: Option<&str>) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse("CNILTEXT000053352643").unwrap(),
            num: num.map(Into::into),
            nature: None,
            nature_delib: None,
            etat: None,
            date_texte: CivilDate::new(2026, 1, 8),
            date_publication: None,
            title: Some("Délibération SAN-2026-002 du 8 janvier 2026".into()),
            title_long: None,
            text: None,
        }
    }

    #[test]
    fn cites_with_cnil_convention() {
        let c = deliberation(Some("SAN-2026-002")).contract();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/cnil/id/CNILTEXT000053352643");
        assert_eq!(
            c.human_readable_citation.as_deref(),
            Some("CNIL, délibération n° SAN-2026-002 du 8 janvier 2026")
        );
    }

    #[test]
    fn falls_back_to_title() {
        let c = deliberation(None).contract();
        assert_eq!(
            c.human_readable_citation.as_deref(),
            Some("Délibération SAN-2026-002 du 8 janvier 2026")
        );
    }
}
