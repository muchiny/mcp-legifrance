//! Publications du BOFiP telles que le port `TaxDoctrine` les rend (ADR-0028, ADR-0029).
//!
//! Ces valeurs n'implémentent pas [`Citable`](crate::contract::Citable) : le contrat de citation
//! exige une adresse sous legifrance.gouv.fr, qu'une publication du BOFiP n'a pas, et son
//! adresse propre peut manquer ([`Permalink`](super::Permalink)). La citation est une méthode à
//! part ([`DoctrineDoc::citation`]).

use crate::{date::CivilDate, text::LongText};

use super::{
    id::{BoiId, PublicationId, Serie},
    kind::DoctrineKind,
    url::BofipUrl,
};

/// Résultat d'une recherche : les métadonnées d'une publication, jamais son texte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctrineHit {
    /// Identifiant juridique.
    pub id: BoiId,
    /// Numéro de la publication, unique.
    pub publication_id: PublicationId,
    /// Titre, sur une ligne ([`clean_line`](super::clean_line)).
    pub title: String,
    /// Nature : doctrine ou actualité.
    pub kind: DoctrineKind,
    /// Série ; `None` : la publication n'en porte pas (6 publications le 2026-11-15).
    pub serie: Option<Serie>,
    /// Début de validité ; `None` : absent ou illisible.
    pub valid_from: Option<CivilDate>,
    /// Adresse sur le site du BOFiP ; `None` : le permalien sort de la grammaire stricte.
    pub source_url: Option<BofipUrl>,
}

/// Page de résultats d'une recherche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctrinePage {
    /// Nombre total de publications trouvées, toutes pages confondues.
    pub total: u64,
    /// Résultats de la page demandée, dans l'ordre du portail.
    pub hits: Vec<DoctrineHit>,
}

/// Publication lue, avec son texte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctrineDoc {
    /// Identifiant juridique.
    pub id: BoiId,
    /// Numéro de la publication, unique : il départage deux publications de même identifiant.
    pub publication_id: PublicationId,
    /// Titre, sur une ligne ([`clean_line`](super::clean_line)).
    pub title: String,
    /// Nature : doctrine ou actualité.
    pub kind: DoctrineKind,
    /// Série ; `None` : la publication n'en porte pas.
    pub serie: Option<Serie>,
    /// Division dans la série, sur une ligne ; `None` : absente.
    pub division: Option<String>,
    /// Début de validité ; `None` : absent ou illisible.
    pub valid_from: Option<CivilDate>,
    /// Adresse sur le site du BOFiP ; `None` : le permalien sort de la grammaire stricte.
    pub source_url: Option<BofipUrl>,
    /// Texte lu ([`publication_text`](super::publication_text)) ; vide pour une publication
    /// sans texte dans le jeu (annexe, formulaire, lettre, carte : 50 publications le 2026-11-15).
    pub text: LongText,
}

impl DoctrineDoc {
    /// Citation de la publication ([`doctrine_citation`]).
    pub fn citation(&self) -> String {
        doctrine_citation(&self.id, self.valid_from)
    }
}

/// Citation d'une publication : « BOFiP, {identifiant}, {début de validité en toutes lettres} »,
/// ou « BOFiP, {identifiant} » sans date (05 §5.15).
///
/// ```
/// use lf_domain::{date::CivilDate, doctrine::{BoiId, doctrine_citation}};
///
/// let id = BoiId::parse("BOI-ENR-DMTG-10-50-20")?;
/// assert_eq!(
///     doctrine_citation(&id, CivilDate::new(2023, 5, 24)),
///     "BOFiP, BOI-ENR-DMTG-10-50-20, 24 mai 2023"
/// );
/// assert_eq!(doctrine_citation(&id, None), "BOFiP, BOI-ENR-DMTG-10-50-20");
/// # Ok::<(), lf_domain::DomainError>(())
/// ```
pub fn doctrine_citation(id: &BoiId, valid_from: Option<CivilDate>) -> String {
    match valid_from {
        Some(date) => format!("BOFiP, {id}, {}", date.to_french()),
        None => format!("BOFiP, {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctrine::{Permalink, clean_text};
    use pretty_assertions::assert_eq;

    fn doc(valid_from: Option<CivilDate>) -> DoctrineDoc {
        let link = Permalink::parse("https://bofip.impots.gouv.fr/bofip/3350-PGP.html").unwrap();
        DoctrineDoc {
            id: BoiId::parse("BOI-ENR-DMTG-10-50-20").unwrap(),
            publication_id: link.publication_id,
            title: "ENR - Mutations à titre gratuit".to_owned(),
            kind: DoctrineKind::Doctrine,
            serie: Some(Serie::parse("ENR").unwrap()),
            division: Some("DMTG".to_owned()),
            valid_from,
            source_url: link.url,
            text: clean_text("1&#13;\nTexte.").unwrap().0,
        }
    }

    #[test]
    fn citation_names_the_id_and_the_start_of_validity() {
        assert_eq!(
            doc(CivilDate::new(2023, 5, 24)).citation(),
            "BOFiP, BOI-ENR-DMTG-10-50-20, 24 mai 2023"
        );
        assert_eq!(
            doc(CivilDate::new(2026, 1, 1)).citation(),
            "BOFiP, BOI-ENR-DMTG-10-50-20, 1er janvier 2026"
        );
    }

    #[test]
    fn citation_without_a_date_names_the_id_alone() {
        assert_eq!(doc(None).citation(), "BOFiP, BOI-ENR-DMTG-10-50-20");
    }

    #[test]
    fn a_page_keeps_its_hits_and_total() {
        let source = doc(CivilDate::new(2023, 5, 24));
        let hit = DoctrineHit {
            id: source.id.clone(),
            publication_id: source.publication_id,
            title: source.title.clone(),
            kind: DoctrineKind::News,
            serie: None,
            valid_from: None,
            source_url: None,
        };
        let page = DoctrinePage { total: 75, hits: vec![hit.clone()] };
        assert_eq!(page.clone(), DoctrinePage { total: 75, hits: vec![hit] });
        assert_eq!(source.text.as_str(), "1\n\nTexte.");
    }
}
