//! Accord d'entreprise, métadonnées seules (`fr_get_company_agreement`,
//! `citations.py:444-486`) : le texte n'existe qu'en pièce jointe `.docx`.

use crate::{
    contract::{Citable, CitationContract, Idcc, with_company},
    date::CivilDate,
    id::AccoTextId,
    url::{ResourceKind, legifrance_url},
};

/// Note jointe à chaque accord (`citations.py:479-482`).
pub const ATTACHMENT_NOTE: &str = "Full text is only distributed by Legifrance as a .docx \
attachment; consult source_url for the human-readable page. This tool returns metadata only.";

/// Accord d'entreprise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyAgreement {
    /// Identifiant.
    pub agreement_id: AccoTextId,
    /// Nature.
    pub nature: Option<String>,
    /// Titre.
    pub title: Option<String>,
    /// Raison sociale de l'entreprise.
    pub raison_sociale: Option<String>,
    /// SIRET.
    pub siret: Option<String>,
    /// Convention collective de rattachement ; `None` : absent ou illisible.
    pub idcc: Option<Idcc>,
    /// Code APE.
    pub code_ape: Option<String>,
    /// Secteur.
    pub secteur: Option<String>,
    /// Thèmes.
    pub themes: Vec<String>,
    /// Syndicats signataires.
    pub syndicats: Vec<String>,
    /// Date du texte ; `None` : inconnue.
    pub date_texte: Option<CivilDate>,
    /// Date d'effet, une date de début ; `None` : date d'effet non déterminée.
    pub date_effet: Option<CivilDate>,
    /// Date de dépôt ; `None` : inconnue.
    pub date_depot: Option<CivilDate>,
}

impl Citable for CompanyAgreement {
    fn contract(&self) -> CitationContract {
        let id = self.agreement_id.as_str();
        CitationContract::from_url(
            legifrance_url(ResourceKind::Acco, id),
            with_company(self.title.as_deref(), self.raison_sociale.as_deref()).as_deref(),
            id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn agreement(idcc: Option<Idcc>) -> CompanyAgreement {
        CompanyAgreement {
            agreement_id: AccoTextId::parse("ACCOTEXT000051888026").unwrap(),
            nature: None,
            title: Some("Télétravail".into()),
            raison_sociale: Some("CORSAIR".into()),
            siret: Some("32862158600143".into()),
            idcc,
            code_ape: None,
            secteur: None,
            themes: vec![],
            syndicats: vec![],
            date_texte: None,
            date_effet: None,
            date_depot: None,
        }
    }

    #[test]
    fn cites_title_and_company() {
        let c = agreement(Idcc::parse("275")).contract();
        assert_eq!(
            (c.citation.as_str(), c.source_url.as_str()),
            (
                "Télétravail (CORSAIR)",
                "https://www.legifrance.gouv.fr/acco/id/ACCOTEXT000051888026"
            )
        );
    }

    #[test]
    fn agreement_idcc_is_typed() {
        // L'IDCC brut de l'API est zéro-complété sur seize chiffres ; il est typé, sans zéros
        // de tête, et absent quand il est illisible. La citation n'en dépend pas.
        let padded = agreement(Idcc::parse("0000000000000275"));
        let unreadable = agreement(Idcc::parse("0000"));
        assert_eq!(
            (
                padded.idcc.map(Idcc::get),
                padded.idcc.map(|i| i.to_string()),
                unreadable.idcc,
                padded.contract().citation == unreadable.contract().citation,
            ),
            (Some(275), Some("275".to_owned()), None, true)
        );
    }
}
