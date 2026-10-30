//! Grammaires par famille de citations (annexe B §4.2 à §4.14) et type commun des candidats.
//!
//! Chaque famille expose `pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate>` :
//! elle essaie ses grammaires aux ancres qui lui conviennent et rend des candidats. Les plages
//! d'un [`Candidate`] sont des offsets du texte **d'origine** (la famille convertit une fois, par
//! `Normalized::orig_range`), parce que `post::arbitrate` ne reçoit pas le texte normalisé. Les
//! plages des ancres, elles, sont des offsets de `Normalized::text`, celui que lisent les
//! grammaires.

pub(crate) mod cassation;
pub(crate) mod cc;
pub(crate) mod ce;
pub(crate) mod cnil;
pub(crate) mod codes;
pub(crate) mod constit;
pub(crate) mod ecli;
pub(crate) mod fond;
pub(crate) mod jorf;
pub(crate) mod kali;
pub(crate) mod puits;
pub(crate) mod ta_caa;
pub(crate) mod textes;

use std::ops::Range;

use crate::citation::normalize::Normalized;
use crate::citation::scan::Anchor;
use crate::citation::types::{Ambiguity, CitationKind, CitationRef, Confidence, Flag};

/// Ce qu'un candidat porte en plus de sa valeur : de quoi remplir `Spanned` à l'arbitrage.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Flags {
    /// Drapeaux de contrôle syntaxique.
    pub list: Vec<Flag>,
    /// Ambiguïté, une au plus.
    pub ambiguity: Option<Ambiguity>,
    /// Numéro du groupe d'une énumération.
    pub enum_group: Option<u32>,
    /// Étendue du groupe d'une énumération, jusqu'à la fin de la référence de code ou de texte :
    /// l'arbitrage évalue les chevauchements sur elle (§4.16, règle 4), les spans émis restant
    /// ceux des numéros.
    pub group_extent: Option<Range<usize>>,
}

/// Une citation proposée par une grammaire, avant arbitrage.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Candidate {
    /// Plage en octets du texte d'origine.
    pub range: Range<usize>,
    /// Valeur reconnue.
    pub value: CitationRef,
    /// Priorité à longueur égale (voir [`priority_of`]) : la plus grande l'emporte.
    pub priority: u8,
    /// Confiance de la grammaire.
    pub confidence: Confidence,
    /// Drapeaux, ambiguïté et énumération.
    pub flags: Flags,
}

/// Priorité de départage à longueur de span égale (annexe B §4.16) : `Ecli` > `LegifranceId` >
/// `Eli` > `Nor` > `Decision` > `CnilDeliberation` > `Convention` > `Jorf` > `TextArticle` >
/// `Text` > `CodeArticle` > `Unsupported`. Plus la valeur est grande, plus la famille l'emporte.
pub(crate) const fn priority_of(kind: CitationKind) -> u8 {
    match kind {
        CitationKind::Ecli => 12,
        CitationKind::LegifranceId => 11,
        CitationKind::Eli => 10,
        CitationKind::Nor => 9,
        CitationKind::Decision => 8,
        CitationKind::CnilDeliberation => 7,
        CitationKind::Convention => 6,
        CitationKind::Jorf => 5,
        CitationKind::TextArticle => 4,
        CitationKind::Text => 3,
        CitationKind::CodeArticle => 2,
        CitationKind::Unsupported => 1,
    }
}

impl Candidate {
    /// Candidat de confiance haute, sans drapeau, à la priorité de sa famille.
    pub(crate) fn new(range: Range<usize>, value: CitationRef) -> Self {
        Self {
            range,
            priority: priority_of(value.kind()),
            value,
            confidence: Confidence::High,
            flags: Flags::default(),
        }
    }
}

/// Candidats de toutes les familles, dans l'ordre des unités de grammaire.
pub(crate) fn scan_all(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate> {
    [
        codes::scan,
        textes::scan,
        constit::scan,
        cassation::scan,
        fond::scan,
        ce::scan,
        ta_caa::scan,
        cc::scan,
        ecli::scan,
        cnil::scan,
        kali::scan,
        jorf::scan,
        puits::scan,
    ]
    .into_iter()
    .flat_map(|family| family(n, anchors))
    .collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::citation::normalize::{Steps, normalize};
    use crate::citation::types::{TextCite, TextNature, UnsupportedCite, UnsupportedFamily};

    #[test]
    fn priorities_follow_annex_b_4_16() {
        let order = [
            CitationKind::Ecli,
            CitationKind::LegifranceId,
            CitationKind::Eli,
            CitationKind::Nor,
            CitationKind::Decision,
            CitationKind::CnilDeliberation,
            CitationKind::Convention,
            CitationKind::Jorf,
            CitationKind::TextArticle,
            CitationKind::Text,
            CitationKind::CodeArticle,
            CitationKind::Unsupported,
        ];
        let priorities = order.map(priority_of);
        assert!(priorities.windows(2).all(|w| w[0] > w[1]), "{priorities:?}");
        assert_eq!(CitationKind::ALL.map(priority_of).iter().copied().max(), Some(12));
        assert_eq!(CitationKind::ALL.len(), order.len());
    }

    #[test]
    fn a_new_candidate_takes_the_priority_of_its_family() {
        let text = TextCite {
            nature: TextNature::Loi,
            number: None,
            date: None,
            date_raw: None,
            title_hint: None,
            anaphora: None,
            appellation: None,
            emitter: None,
        };
        let candidate = Candidate::new(3..9, CitationRef::Text(text));
        assert_eq!(candidate.range, 3..9);
        assert_eq!(candidate.priority, priority_of(CitationKind::Text));
        assert_eq!(candidate.confidence, Confidence::High);
        assert_eq!(candidate.flags, Flags::default());
        let sink = Candidate::new(
            0..1,
            CitationRef::Unsupported(UnsupportedCite { family: UnsupportedFamily::Ue, key: None }),
        );
        assert!(sink.priority < candidate.priority);
    }

    #[test]
    fn scan_all_runs_every_family_without_panicking() {
        let n = normalize("article 1240 du code civil", Steps::default());
        let candidates = scan_all(&n, &[]);
        assert!(candidates.len() <= n.text.len());
    }
}
