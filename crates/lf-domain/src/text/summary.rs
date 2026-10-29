//! Résumé d'un texte long (`detail="summary"`, 03 §7.9) : sommaire officiel, sinon dispositif,
//! sinon début du texte.
//!
//! STUB(V0) → V1-04 : les types, les constantes et la signature de [`summary`] sont gelés par la
//! vague 0 (V1-06 s'y appuie pour la politique de lecture). Le corps est provisoire : il rend
//! toujours la source [`SummarySource::Lead`], coupée aux `max_chars` premiers caractères.

use std::fmt;

use super::LongText;
use crate::entity::OfficialSummary;

/// Plafond, en caractères, d'un résumé : le résumé rendu compte au plus
/// `min(max_chars, SUMMARY_CAP_CHARS)` caractères (05 §6.1).
pub const SUMMARY_CAP_CHARS: usize = 1_200;

/// Longueur minimale, en caractères, du début de texte rendu comme résumé quand ni sommaire
/// officiel ni dispositif n'existent.
pub const LEAD_MIN_CHARS: usize = 600;

/// D'où vient le résumé (`summary_source` de la sortie, 05 §5.0.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarySource {
    /// Sommaire officiel de la décision (`analyses`, puis `titrages`).
    Official,
    /// Dispositif repéré dans le texte, selon la règle de la juridiction.
    Dispositif,
    /// Début du texte, faute de mieux.
    Lead,
}

impl SummarySource {
    /// Valeur de `summary_source` sur le fil : `"official"`, `"dispositif"` ou `"lead"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Dispositif => "dispositif",
            Self::Lead => "lead",
        }
    }
}

/// Règle de repérage du dispositif, choisie par l'appelant selon la juridiction de la décision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositifRule {
    /// Décision du Conseil constitutionnel : entre « décide » et « juge par ».
    ConseilConstitutionnel,
    /// Décision du Conseil d'État : après les considérants, paragraphes décoratifs écartés.
    ConseilEtat,
    /// Pas de règle de dispositif (autres fonds).
    None,
}

/// Résumé d'un texte long.
///
/// Son `Debug` n'écrit jamais le texte (extrait d'un document), seulement la source, la longueur
/// en caractères et le drapeau `truncated`.
#[derive(Clone, PartialEq, Eq)]
pub struct Summary {
    /// Origine du résumé.
    pub source: SummarySource,
    /// Texte du résumé, d'au plus `max_chars` caractères.
    pub text: String,
    /// Le texte d'origine a été coupé pour tenir dans la borne.
    pub truncated: bool,
}

impl fmt::Debug for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Summary {{ source: {:?}, chars: {}, truncated: {} }}",
            self.source,
            self.text.chars().count(),
            self.truncated
        )
    }
}

/// Résumé de `text`, d'au plus `max_chars` caractères.
///
/// STUB(V0) → V1-04 : le sommaire officiel et le dispositif sont ignorés, le résumé est le début
/// du texte (source [`SummarySource::Lead`]), coupé à `max_chars` caractères.
#[must_use]
pub fn summary(
    text: &LongText,
    _official: Option<&OfficialSummary>,
    _rule: DispositifRule,
    max_chars: usize,
) -> Summary {
    // STUB(V0) → V1-04
    let lead: String = text.as_str().chars().take(max_chars).collect();
    let truncated = lead.chars().count() < text.as_str().chars().count();
    Summary { source: SummarySource::Lead, text: lead, truncated }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn source_names_are_the_wire_values() {
        assert_eq!(SummarySource::Official.as_str(), "official");
        assert_eq!(SummarySource::Dispositif.as_str(), "dispositif");
        assert_eq!(SummarySource::Lead.as_str(), "lead");
    }

    #[test]
    fn caps_are_the_documented_values() {
        assert_eq!((SUMMARY_CAP_CHARS, LEAD_MIN_CHARS), (1_200, 600));
    }

    #[test]
    fn debug_never_writes_the_text() {
        let summary = Summary {
            source: SummarySource::Official,
            text: "contenu confidentiel".to_owned(),
            truncated: true,
        };
        let shown = format!("{summary:?}");
        assert_eq!(shown, "Summary { source: Official, chars: 20, truncated: true }");
        assert!(!shown.contains("confidentiel"));
    }

    #[test]
    fn stub_cuts_the_lead_at_max_chars() {
        // STUB(V0) → V1-04 : seule la forme est gelée ; l'algorithme est éprouvé là-bas.
        let long = LongText::from_plain("Premier paragraphe.\n\nSecond paragraphe.").unwrap();
        let cut = summary(&long, None, DispositifRule::None, 10);
        assert_eq!(
            cut,
            Summary { source: SummarySource::Lead, text: "Premier pa".to_owned(), truncated: true }
        );
        let whole = summary(&long, None, DispositifRule::ConseilEtat, 1_000);
        assert_eq!((whole.source, whole.truncated), (SummarySource::Lead, false));
        assert_eq!(whole.text, long.as_str());
    }
}
