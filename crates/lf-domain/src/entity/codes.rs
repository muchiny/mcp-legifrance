//! Entrée de la liste des codes (`/list/code`, M6, 03 §4.11).

use crate::{id::LegiTextId, status::LegalStatus};

/// Une entrée de `list_codes()` : un code, abrogé ou non.
///
/// Le nom `CodeEntry` est déjà pris par l'entrée statique du dictionnaire des codes du parseur de
/// citations (champs `&'static`) ; celle-ci vient de l'API. Une entrée sans identifiant `LEGITEXT`
/// valide ou sans titre est écartée (et comptée) par le mapper, jamais construite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeListing {
    /// Identifiant du code.
    pub id: LegiTextId,
    /// Titre officiel, ex. `Code civil`.
    pub title: String,
    /// État juridique ; `None` : absent ou non reconnu.
    pub etat: Option<LegalStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn listing_compares_by_value() {
        let listing = CodeListing {
            id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
            title: "Code civil".into(),
            etat: Some(LegalStatus::Vigueur),
        };
        assert_eq!(listing.clone(), listing);
        assert_ne!(CodeListing { etat: None, ..listing.clone() }, listing);
        assert_eq!(listing.id.as_str(), "LEGITEXT000006070721");
    }
}
