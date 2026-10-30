//! Résolution des citations juridiques (M6 et M7, annexe B §6.6) : contrôle, résolution, liens,
//! politique de confiance et notes.
//!
//! Le parseur de citations lui-même vit dans `lf_domain::citation`. Ce module range les types que
//! le contrôle ([`verify`]) et le moteur de résolution ([`resolve`]) partagent avec la sortie de
//! `fr_verify_citations` et, à terme, de `fr_resolve` : statuts, enregistrement résolu, candidats.
//! L'unité V3-05 remplit `verify`, `resolve`, `policy` et `notes` ; V3-06 remplit `related`.

pub mod notes;
pub mod policy;
pub mod related;
pub mod resolve;
pub mod verify;

use lf_domain::{
    citation::{CitationKind, Confidence, Span},
    contract::{Citable, CitationContract},
    ecli::Ecli,
    id::DocumentId,
};

pub use notes::{Field, NotCheckedReason, Note};

/// Verdict sur une citation contrôlée (00 §7, annexe B §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckStatus {
    /// Enregistrement trouvé, champs cités cohérents.
    Verified,
    /// Citation bien formée, aucun enregistrement, preuve d'absence réunie (production seulement).
    NotFound,
    /// Plusieurs lectures ou plusieurs enregistrements possibles.
    Ambiguous,
    /// Contrôle impossible : hors périmètre, hors couverture, erreur amont, budget, bac à sable.
    Unresolvable,
    /// Syntaxe impossible, ou contradiction avec l'enregistrement officiel.
    Invalid,
}

impl CheckStatus {
    /// Valeur sur le fil.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::NotFound => "not_found",
            Self::Ambiguous => "ambiguous",
            Self::Unresolvable => "unresolvable",
            Self::Invalid => "invalid",
        }
    }
}

/// Verdict sur l'extrait cité qui jouxte une citation (annexe B §7.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuoteCheck {
    /// L'extrait figure dans le texte officiel.
    Match,
    /// L'extrait ne figure pas dans le texte officiel.
    QuoteMismatch,
}

impl QuoteCheck {
    /// Valeur sur le fil.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Match => "match",
            Self::QuoteMismatch => "quote_mismatch",
        }
    }
}

/// Enregistrement officiel trouvé pour une citation : son identifiant vient d'une réponse de
/// l'API, sa citation et son URL du contrat de citation du document (annexe B §7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRecord {
    /// Identifiant lu dans la réponse, jamais construit.
    pub id: DocumentId,
    /// Citation selon l'usage français, calculée par le contrat du document.
    pub citation: String,
    /// Page Légifrance du document.
    pub source_url: String,
    /// ECLI natif, s'il y en a un.
    pub ecli: Option<Ecli>,
}

impl ResolvedRecord {
    /// Enregistrement de `entity`, d'identifiant `id`.
    pub(crate) fn of<T: Citable>(id: DocumentId, entity: &T) -> Self {
        let CitationContract { citation, source_url, ecli, eli: _ } = entity.contract();
        Self {
            id,
            citation: citation.as_str().to_owned(),
            source_url: source_url.as_str().to_owned(),
            ecli,
        }
    }
}

/// Famille de document d'un candidat (`kind` de `fr_resolve`, 05 §5.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    /// Article de code, de loi ou de convention.
    Article,
    /// Texte LODA (loi, décret, ordonnance).
    Loda,
    /// Code.
    Code,
    /// Décision de l'ordre judiciaire.
    Juri,
    /// Décision du Conseil constitutionnel.
    Constit,
    /// Décision administrative.
    Cetat,
    /// Délibération de la CNIL.
    Cnil,
    /// Convention collective.
    Kali,
    /// Accord d'entreprise.
    Acco,
}

impl CandidateKind {
    /// Valeur sur le fil.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Loda => "loda",
            Self::Code => "code",
            Self::Juri => "juri",
            Self::Constit => "constit",
            Self::Cetat => "cetat",
            Self::Cnil => "cnil",
            Self::Kali => "kali",
            Self::Acco => "acco",
        }
    }

    /// Famille du document d'identifiant `id`, d'après son préfixe. Un texte du Journal officiel
    /// n'a pas de famille propre en v0.2 : il se range avec les textes LODA.
    #[must_use]
    pub const fn of(id: &DocumentId) -> Self {
        match id {
            DocumentId::LegiArti(_) | DocumentId::KaliArti(_) => Self::Article,
            DocumentId::LegiText(_) | DocumentId::JorfText(_) => Self::Loda,
            DocumentId::KaliText(_) => Self::Kali,
            DocumentId::JuriText(_) => Self::Juri,
            DocumentId::ConsText(_) => Self::Constit,
            DocumentId::CetaText(_) => Self::Cetat,
            DocumentId::CnilText(_) => Self::Cnil,
            DocumentId::AccoText(_) => Self::Acco,
        }
    }
}

/// Un enregistrement qui pourrait être celui que la citation désigne.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// L'enregistrement.
    pub record: ResolvedRecord,
    /// Sa famille.
    pub kind: CandidateKind,
    /// Confiance dans la correspondance (annexe B §7.8).
    pub confidence: Confidence,
}

/// Résultat du contrôle d'une citation, dans l'ordre du texte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationCheck {
    /// Position dans le texte reçu, en **points de code Unicode**, intervalle semi-ouvert.
    pub span: Span,
    /// Extrait d'origine.
    pub raw: String,
    /// Famille de la citation reconnue.
    pub kind: CitationKind,
    /// Verdict.
    pub status: CheckStatus,
    /// Enregistrement officiel trouvé, y compris quand il contredit la citation.
    pub resolved: Option<ResolvedRecord>,
    /// Verdict sur l'extrait cité adjacent, quand le texte officiel a été lu.
    pub quote: Option<QuoteCheck>,
    /// Explication actionnable.
    pub note: Option<Note>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_domain::{
        entity::{ArticleText, SearchHit},
        fond::Fond,
        id::ArticleId,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn wire_names_are_snake_case() {
        let statuses = [
            (CheckStatus::Verified, "verified"),
            (CheckStatus::NotFound, "not_found"),
            (CheckStatus::Ambiguous, "ambiguous"),
            (CheckStatus::Unresolvable, "unresolvable"),
            (CheckStatus::Invalid, "invalid"),
        ];
        for (status, name) in statuses {
            assert_eq!(status.as_str(), name);
        }
        assert_eq!(QuoteCheck::Match.as_str(), "match");
        assert_eq!(QuoteCheck::QuoteMismatch.as_str(), "quote_mismatch");
        let kinds = [
            (CandidateKind::Article, "article"),
            (CandidateKind::Loda, "loda"),
            (CandidateKind::Code, "code"),
            (CandidateKind::Juri, "juri"),
            (CandidateKind::Constit, "constit"),
            (CandidateKind::Cetat, "cetat"),
            (CandidateKind::Cnil, "cnil"),
            (CandidateKind::Kali, "kali"),
            (CandidateKind::Acco, "acco"),
        ];
        for (kind, name) in kinds {
            assert_eq!(kind.as_str(), name);
        }
    }

    #[test]
    fn candidate_kind_follows_the_identifier_prefix() {
        let expected = [
            ("LEGIARTI000006419288", CandidateKind::Article),
            ("KALIARTI000005635096", CandidateKind::Article),
            ("LEGITEXT000006070721", CandidateKind::Loda),
            ("JORFTEXT000033202746", CandidateKind::Loda),
            ("KALITEXT000005635096", CandidateKind::Kali),
            ("JURITEXT000051743650", CandidateKind::Juri),
            ("CONSTEXT000052555275", CandidateKind::Constit),
            ("CETATEXT000054178503", CandidateKind::Cetat),
            ("CNILTEXT000053352643", CandidateKind::Cnil),
            ("ACCOTEXT000045000001", CandidateKind::Acco),
        ];
        for (id, kind) in expected {
            assert_eq!(CandidateKind::of(&DocumentId::parse(id).unwrap()), kind, "{id}");
        }
    }

    #[test]
    fn a_record_carries_the_contract_of_its_document() {
        let article = ArticleText {
            article_id: ArticleId::parse("LEGIARTI000006419288").unwrap(),
            num: Some("9".to_owned()),
            etat: None,
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::default(),
            nota: None,
        };
        let id = DocumentId::parse("LEGIARTI000006419288").unwrap();
        let record = ResolvedRecord::of(id.clone(), &article);
        assert_eq!(record.id, id);
        assert_eq!(record.citation, "Code civil, art. 9");
        assert_eq!(
            record.source_url,
            "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288"
        );
        assert_eq!(record.ecli, None);
        // Un résultat de recherche se cite aussi.
        let hit =
            SearchHit::new(Fond::Cnil, DocumentId::parse("CNILTEXT000053352643").unwrap(), None)
                .unwrap();
        let record = ResolvedRecord::of(hit.id.clone(), &hit);
        assert_eq!(record.citation, "Légifrance CNILTEXT000053352643");
    }
}
