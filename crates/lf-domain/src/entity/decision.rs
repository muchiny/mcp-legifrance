//! Décision JURI / CONSTIT / CETAT (`fr_get_decision`, `citations.py:321-355`).
//!
//! La référence choisit l'URL d'après le champ `origine` ; ici le préfixe de
//! l'identifiant en tient lieu (`CONSTEXT` ⇔ origine `CONSTIT`, vérifié sur
//! `consult_juri_constit.json` et `consult_juri_cetat.json`).

use crate::{
    contract::{Citable, CitationContract, constit_citation},
    date::CivilDate,
    ecli::Ecli,
    id::DecisionId,
    url::{ResourceKind, legifrance_url},
};

/// Décision de justice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Identifiant de la décision.
    pub decision_id: DecisionId,
    /// ECLI natif, absent pour une partie des CAA/TA — jamais fabriqué.
    pub ecli: Option<Ecli>,
    /// Numéro (Conseil constitutionnel : `2025-1173`).
    pub num: Option<String>,
    /// Juridiction.
    pub juridiction: Option<String>,
    /// Formation.
    pub formation: Option<String>,
    /// Solution.
    pub solution: Option<String>,
    /// Numéros d'affaire, ex. `24-13.470`.
    pub numero_affaire: Vec<String>,
    /// Date de la décision.
    pub date_texte: Option<CivilDate>,
    /// Nature (Conseil constitutionnel : `QPC`, `DC`…).
    pub nature: Option<String>,
    /// Titre Légifrance.
    pub title: Option<String>,
    /// Texte intégral.
    pub text: Option<String>,
}

impl Citable for Decision {
    fn contract(&self) -> CitationContract {
        let (kind, citation) = match self.decision_id {
            DecisionId::Juri(_) => (ResourceKind::Juri, self.title.clone()),
            DecisionId::Cetat(_) => (ResourceKind::Cetat, self.title.clone()),
            DecisionId::Constit(_) => (
                ResourceKind::Constit,
                constit_citation(
                    self.num.as_deref(),
                    self.nature.as_deref(),
                    self.date_texte,
                    self.title.as_deref(),
                )
                .or_else(|| self.title.clone()),
            ),
        };
        CitationContract::from_url(legifrance_url(kind, self.decision_id.as_str()), citation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn decision(id: &str) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: Some("2025-1173".into()),
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: CivilDate::new(2025, 11, 7),
            nature: Some("QPC".into()),
            title: Some("Titre Légifrance [Objet]".into()),
            text: None,
        }
    }

    #[test]
    fn juri_cites_title() {
        let c = decision("JURITEXT000048465611").contract();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/juri/id/JURITEXT000048465611");
        assert_eq!(c.human_readable_citation.as_deref(), Some("Titre Légifrance [Objet]"));
    }

    #[test]
    fn cetat_cites_title() {
        let c = decision("CETATEXT000051655317").contract();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/ceta/id/CETATEXT000051655317");
        assert_eq!(c.human_readable_citation.as_deref(), Some("Titre Légifrance [Objet]"));
    }

    #[test]
    fn constit_uses_french_convention() {
        let c = decision("CONSTEXT000037434572").contract();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000037434572");
        assert_eq!(
            c.human_readable_citation.as_deref(),
            Some("Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 - Objet")
        );
    }

    #[test]
    fn constit_falls_back_to_title() {
        let mut d = decision("CONSTEXT000037434572");
        d.num = None;
        assert_eq!(
            d.contract().human_readable_citation.as_deref(),
            Some("Titre Légifrance [Objet]")
        );
    }
}
