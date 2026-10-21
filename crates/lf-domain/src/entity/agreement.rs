//! Accord d'entreprise, métadonnées seules (`fr_get_company_agreement`,
//! `citations.py:444-486`) : le texte n'existe qu'en pièce jointe `.docx`.

use crate::{
    contract::{Citable, CitationContract, with_company},
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
    /// Convention collective de rattachement (IDCC).
    pub idcc: Option<String>,
    /// Code APE.
    pub code_ape: Option<String>,
    /// Secteur.
    pub secteur: Option<String>,
    /// Thèmes.
    pub themes: Vec<String>,
    /// Syndicats signataires.
    pub syndicats: Vec<String>,
    /// Date du texte.
    pub date_texte: Option<CivilDate>,
    /// Date d'effet.
    pub date_effet: Option<CivilDate>,
    /// Date de dépôt.
    pub date_depot: Option<CivilDate>,
}

impl Citable for CompanyAgreement {
    fn contract(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Acco, self.agreement_id.as_str()).to_string(),
            with_company(self.title.as_deref(), self.raison_sociale.as_deref()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn cites_title_and_company() {
        let agreement = CompanyAgreement {
            agreement_id: AccoTextId::parse("ACCOTEXT000051888026").unwrap(),
            nature: None,
            title: Some("Télétravail".into()),
            raison_sociale: Some("CORSAIR".into()),
            siret: Some("32862158600143".into()),
            idcc: Some("275".into()),
            code_ape: None,
            secteur: None,
            themes: vec![],
            syndicats: vec![],
            date_texte: None,
            date_effet: None,
            date_depot: None,
        };
        assert_eq!(
            agreement.contract(),
            CitationContract::from_url(
                "https://www.legifrance.gouv.fr/acco/id/ACCOTEXT000051888026".into(),
                Some("Télétravail (CORSAIR)".into()),
            )
        );
    }
}
