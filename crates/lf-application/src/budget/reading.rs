//! Politique de lecture des textes longs (05 §6, 02 §6.1) : trois modes (résumé, texte plein,
//! passages), `offset`, `max_chars` et `next_offset`.
//!
//! STUB(V0) → V1-06 : les types et les signatures sont gelés par la vague 0 (les tranches de lecture
//! V2-01 à V2-03 les consomment). Les corps de [`ReadingRequest::parse`] et de [`read`] sont
//! `todo!()` ; la forme interne de [`ReadingRequest`] est laissée à V1-06.

use lf_domain::{
    entity::OfficialSummary,
    text::{DispositifRule, LongText, Summary},
};

use crate::{error::AppError, input::ReadingInput};

/// Lecture demandée, déjà validée : mode, `offset`, `max_chars`, `focus` et `passages`.
///
/// La forme interne appartient à V1-06 ; elle ne se construit que par [`ReadingRequest::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingRequest(());

impl ReadingRequest {
    /// Valide les paramètres de lecture dans l'ordre de la référence (`detail`, `offset`,
    /// `max_chars`, `focus`, `passages`) et en déduit le mode.
    ///
    /// STUB(V0) → V1-06.
    ///
    /// # Errors
    ///
    /// [`AppError::InvalidArgument`] pour une valeur hors bornes, un `detail` inconnu, un
    /// `offset` combiné à `detail="summary"` ou un `focus` sans mot recherchable.
    #[expect(clippy::todo, reason = "STUB(V0) → V1-06")]
    pub fn parse(_input: &ReadingInput) -> Result<Self, AppError> {
        todo!("STUB(V0) → V1-06")
    }
}

/// Un passage rendu en mode `focus` : sa position et son texte (05 §5.0.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassageOut {
    /// Position du premier caractère du passage dans le texte canonique, en caractères.
    pub offset: u32,
    /// Texte du passage.
    pub text: String,
}

/// Résultat d'une lecture : les champs communs aux trois lectures longues (05 §5.0.3). Chaque
/// mode ne remplit que les siens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// Résumé, en mode résumé.
    pub summary: Option<Summary>,
    /// Texte, en mode plein texte ou pour un texte court.
    pub text: Option<String>,
    /// Longueur du texte canonique entier, en caractères.
    pub text_length: u32,
    /// Position de la fenêtre suivante, absente en fin de texte.
    pub next_offset: Option<u32>,
    /// Passages, en mode `focus`.
    pub passages: Option<Vec<PassageOut>>,
    /// Remarque exceptionnelle (texte vide, par exemple).
    pub note: Option<String>,
}

/// Lit `text` selon `req`.
///
/// STUB(V0) → V1-06.
///
/// # Errors
///
/// [`AppError::InvalidArgument`] quand `offset` dépasse la fin du texte.
#[expect(clippy::todo, reason = "STUB(V0) → V1-06")]
pub fn read(
    _text: &LongText,
    _official: Option<&OfficialSummary>,
    _rule: DispositifRule,
    _req: &ReadingRequest,
) -> Result<Reading, AppError> {
    todo!("STUB(V0) → V1-06")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn reading_compares_by_value() {
        let reading = Reading {
            summary: None,
            text: Some("texte".to_owned()),
            text_length: 5,
            next_offset: None,
            passages: Some(vec![PassageOut { offset: 0, text: "texte".to_owned() }]),
            note: None,
        };
        assert_eq!(reading.clone(), reading);
        assert_ne!(Reading { note: Some("vide".to_owned()), ..reading.clone() }, reading);
    }
}
