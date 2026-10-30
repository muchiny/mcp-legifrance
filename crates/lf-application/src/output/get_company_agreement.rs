//! Sortie de `fr_get_company_agreement` (v0.1) : `GetCompanyAgreementOutput` et sa métrique d'audit.
//!
//! Métadonnées seules (05 §5.7) : le texte d'un accord d'entreprise n'existe qu'en pièce jointe
//! `.docx`, que le domaine ne porte pas (`CompanyAgreement` n'a aucun champ `text`). L'adapter MCP
//! tire la citation et l'URL source de l'accord par `Citable::contract`.

use lf_domain::entity::CompanyAgreement;

use crate::audited::AuditMetric;

/// Ce que `GetCompanyAgreementUseCase` rend : l'accord, tel que le corpus l'a lu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetCompanyAgreementOutput {
    /// Les métadonnées de l'accord d'entreprise.
    pub agreement: CompanyAgreement,
}

/// Métrique d'audit : 1 (un accord lu, 05 §5.0.1).
impl AuditMetric for GetCompanyAgreementOutput {
    fn output_count_or_size(&self) -> u64 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::id::AccoTextId;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_metric_is_one_agreement() {
        let output = GetCompanyAgreementOutput {
            agreement: CompanyAgreement {
                agreement_id: AccoTextId::parse("ACCOTEXT000053643828").unwrap(),
                nature: None,
                title: None,
                raison_sociale: None,
                siret: None,
                idcc: None,
                code_ape: None,
                secteur: None,
                themes: Vec::new(),
                syndicats: Vec::new(),
                date_texte: None,
                date_effet: None,
                date_depot: None,
            },
        };
        assert_eq!(output.output_count_or_size(), 1);
    }
}
