//! Judilibre : les décisions de l'ordre judiciaire publiées en open data par la Cour de
//! cassation (ADR-0035).
//!
//! Une décision de Judilibre n'est pas un texte de Légifrance : elle a son propre identifiant
//! (24 caractères hexadécimaux), ses juridictions (Cour de cassation, cours d'appel, tribunaux
//! judiciaires, tribunaux de commerce, conseils de prud'hommes) et ses mentions obligatoires.
//! Ses types sont validés sans réseau : une valeur construite est déjà conforme.
//!
//! - entrées : [`JudilibreId`], [`Court`], [`CourtSet`], [`SearchPart`], [`DecisionPart`],
//!   [`CaseLawQuery`] ;
//! - décisions : [`CaseLawHit`], [`CaseLawPage`], [`CaseLawDoc`], leurs mentions
//!   ([`CaseLawMention`]) et leur citation ;
//! - découpage par les zones : [`part_text`] ;
//! - intitulés des codes : [`court_label`], [`chamber_label`], [`formation_label`]…
//!
//! Aucun type ne désigne une personne, et aucune fonction n'extrait un nom (ADR-0035,
//! décision 7 : profilage des magistrats et des greffiers interdit).

mod court;
mod doc;
mod id;
mod labels;
mod part;
mod query;
mod zones;

pub use court::{Court, CourtSet};
pub use doc::{
    CITATION_YEAR_MIN, CaseLawDoc, CaseLawHit, CaseLawMention, CaseLawPage, ContestedDecision,
    EXCERPT_MAX_CHARS, HIT_THEMES_MAX, RelatedDecision, excerpt, strip_markup,
};
pub use id::{JUDILIBRE_ID_CHARS, JudilibreId};
pub use labels::{chamber_label, court_label, formation_label, publication_label, solution_label};
pub use part::{DecisionPart, SearchPart};
pub use query::{CASE_LAW_PAGE_SIZE_MAX, CaseLawFilters, CaseLawQuery, CaseLawSort};
pub use zones::{ZONE_SPANS_MAX, ZoneSpan, Zones, part_text};

/// Adresse publique d'une décision sur le site de la Cour de cassation, jamais appelée par le
/// serveur : `https://www.courdecassation.fr/decision/{id}`.
///
/// Mesure du 2026-11-27 : l'adresse répond 200 derrière une protection anti-robot ; son contenu
/// n'a pas été lu (**NON VÉRIFIÉ** [V-239 · judilibre-adresse-publique]).
#[must_use]
pub fn public_url(id: &JudilibreId) -> String {
    format!("https://www.courdecassation.fr/decision/{id}")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_public_address_has_a_strict_grammar() {
        let id = JudilibreId::parse("5FCA624B7EEB384A40CAFB85").unwrap();
        assert_eq!(public_url(&id), "https://www.courdecassation.fr/decision/5fca624b7eeb384a40cafb85");
    }
}
