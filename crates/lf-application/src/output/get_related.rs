//! Sortie de `fr_get_related` (v0.3, M7) : `GetRelatedOutput` et sa métrique d'audit (05 §5.11).
//!
//! L'application rend la liste déjà bornée : au plus 100 liens, et une estimation de la réponse
//! sous la cible (05 §6.4). Les règles de rendu en JSON (`kind` en `snake_case`, `truncated`
//! seulement quand il est vrai) sont celles de la vue, dans `lf-adapter-mcp`.

use lf_domain::citation::CitationKind;

use crate::{audited::AuditMetric, citation::Note};

/// Sens d'un lien par rapport à la page demandée (05 §5.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    /// Article : un texte que l'article cite (`liensCite`).
    Cites,
    /// Article : un texte qui cite l'article (`liensCitePar`).
    CitedBy,
    /// Décision : une décision citée par `citationJp`.
    CitesCaseLaw,
    /// Décision : un texte appliqué (autre citation de `citationJp`, ou titre de `liens`).
    AppliesText,
}

impl Relation {
    /// Valeur `relation` de la réponse (`cites`, `cited_by`, `cites_case_law`, `applies_text`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cites => "cites",
            Self::CitedBy => "cited_by",
            Self::CitesCaseLaw => "cites_case_law",
            Self::AppliesText => "applies_text",
        }
    }
}

/// Un lien rendu.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinkOut {
    /// Sens du lien.
    pub relation: Relation,
    /// Famille de la cible, dans le vocabulaire de `fr_verify_citations`.
    pub kind: CitationKind,
    /// Forme canonique, ou titre de l'API nettoyé : jamais vide.
    pub citation: String,
    /// Identifiant Légifrance de la cible, seulement s'il est typé et qu'un outil servi sait le
    /// lire (`LEGIARTI`, `LEGITEXT`, `KALIARTI`, `KALITEXT`) : jamais fabriqué.
    pub id: Option<String>,
    /// Page Légifrance de la cible, quand `id` est présent.
    pub source_url: Option<String>,
}

/// Réponse de `fr_get_related`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetRelatedOutput {
    /// Identifiant demandé, sous sa forme normalisée.
    pub id: String,
    /// Citation de la page demandée (article ou décision).
    pub citation: String,
    /// Page Légifrance de l'identifiant demandé.
    pub source_url: String,
    /// Nombre de liens avant troncature.
    pub total: u32,
    /// Les liens rendus : cent au plus.
    pub links: Vec<LinkOut>,
    /// Vrai quand des liens ont été retirés (au-delà de cent, ou pour tenir sous le plafond).
    pub truncated: bool,
    /// Présente quand l'API ne donne aucune référence pour la décision demandée : la liste vide
    /// ne dit alors pas que la décision ne cite rien (V4-05).
    pub note: Option<Note>,
}

impl AuditMetric for GetRelatedOutput {
    /// Nombre de liens renvoyés (05 §5.0.1).
    fn output_count_or_size(&self) -> u64 {
        u64::try_from(self.links.len()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn link(citation: &str) -> LinkOut {
        LinkOut {
            relation: Relation::Cites,
            kind: CitationKind::Text,
            citation: citation.to_owned(),
            id: None,
            source_url: None,
        }
    }

    fn output(links: Vec<LinkOut>, total: u32) -> GetRelatedOutput {
        GetRelatedOutput {
            id: "LEGIARTI000006419288".to_owned(),
            citation: "Code civil, art. 9".to_owned(),
            source_url: "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
                .to_owned(),
            total,
            links,
            truncated: false,
            note: None,
        }
    }

    #[test]
    fn relations_are_rendered_with_the_documented_values() {
        let rendered =
            [Relation::Cites, Relation::CitedBy, Relation::CitesCaseLaw, Relation::AppliesText]
                .map(Relation::as_str);
        assert_eq!(rendered, ["cites", "cited_by", "cites_case_law", "applies_text"]);
    }

    #[test]
    fn the_metric_is_the_number_of_links_returned_not_the_total() {
        assert_eq!(output(vec![], 0).output_count_or_size(), 0);
        // 250 liens avant troncature, 2 rendus : la métrique compte les rendus.
        assert_eq!(output(vec![link("a"), link("b")], 250).output_count_or_size(), 2);
    }

    #[test]
    fn links_compare_and_hash_by_value() {
        let one = link("Loi n° 2016-1321");
        assert_eq!(one.clone(), one);
        assert_ne!(one, LinkOut { relation: Relation::CitedBy, ..one.clone() });
        let mut seen = std::collections::HashSet::new();
        assert!(seen.insert(one.clone()));
        assert!(!seen.insert(one));
    }
}
