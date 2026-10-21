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
                constit_citation(self.num.as_deref(), self.nature.as_deref(), self.date_texte)
                    .or_else(|| self.title.clone()),
            ),
        };
        let id = self.decision_id.as_str();
        CitationContract {
            ecli: self.ecli.clone(),
            ..CitationContract::from_url(legifrance_url(kind, id), citation.as_deref(), id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const IDS: [&str; 3] = ["JURITEXT000048465611", "CONSTEXT000052555275", "CETATEXT000051655317"];

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
    fn juri_and_cetat_citation_is_clean_title() {
        // Titres de consult_juri.json et de consult_juri_cetat.json, cités tels
        // quels sur une ligne (03 §5.4), même avec `num`, `nature` et date.
        let cases = [
            (
                IDS[0],
                "Cour de cassation, civile, Chambre civile 1,\n4 juin 2025, 24-13.470, Publié au bulletin",
                "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin",
                "https://www.legifrance.gouv.fr/juri/id/JURITEXT000048465611",
            ),
            (
                IDS[2],
                "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507",
                "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507",
                "https://www.legifrance.gouv.fr/ceta/id/CETATEXT000051655317",
            ),
        ];
        for (id, title, citation, url) in cases {
            let mut d = decision(id);
            d.title = Some(title.into());
            let c = d.contract();
            assert_eq!((c.citation.as_str(), c.source_url.as_str()), (citation, url));
        }
    }

    #[test]
    fn constit_citation_ignores_bracketed_subject() {
        // consult_juri_constit.json : l'objet entre crochets reste lisible dans
        // `title`, pas dans la citation (décision C2, 00 §4).
        let mut d = decision(IDS[1]);
        d.title = Some(
            "Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de publicité des débats]"
                .into(),
        );
        let c = d.contract();
        assert_eq!(
            (c.citation.as_str(), c.source_url.as_str()),
            (
                "Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025",
                "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000052555275"
            )
        );
    }

    #[test]
    fn constit_falls_back_to_title() {
        let mut d = decision(IDS[1]);
        d.num = None;
        assert_eq!(d.contract().citation.as_str(), "Titre Légifrance [Objet]");
    }

    #[test]
    fn native_ecli_is_propagated() {
        // ECLI natifs de consult_juri.json, consult_juri_constit.json et
        // consult_juri_cetat.json.
        let eclis = [
            "ECLI:FR:CCASS:2025:C100399",
            "ECLI:FR:CC:2025:2025.1173.QPC",
            "ECLI:FR:CECHR:2026:506507.20260529",
        ];
        for (id, ecli) in IDS.into_iter().zip(eclis) {
            let mut d = decision(id);
            d.ecli = Some(Ecli::parse(ecli).unwrap());
            let c = d.contract();
            assert_eq!((c.ecli.as_ref().map(Ecli::as_str), c.eli), (Some(ecli), None), "{id}");
        }
    }

    #[test]
    fn ecli_is_never_fabricated() {
        // `num`, `nature` et date renseignés, sans ECLI natif : aucun ECLI dérivé.
        for id in IDS {
            let c = decision(id).contract();
            assert_eq!((c.ecli, c.eli), (None, None), "{id}");
        }
    }
}
