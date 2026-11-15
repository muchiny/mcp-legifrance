//! Requête de recherche validée de `fr_search_bofip` (05 §5.14).

use std::num::NonZeroU32;

use crate::{
    error::DomainError,
    search::{PageSize, REACHABLE_RESULTS, within_reach},
};

use super::{id::Serie, kind::KindFilter, terms::DoctrineTerms};

/// Recherche dans le BOFiP : des mots, une série facultative, les natures voulues, une page.
///
/// Les champs sont privés : l'invariant de [`new`](Self::new) tient pour toute valeur construite.
/// Le portail refuse `offset + limit > 10 000` (mesure du 2026-11-15), soit
/// `page × page_size >` [`REACHABLE_RESULTS`] : la même fenêtre que `fr_search`, contrôlée ici
/// avant tout appel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DoctrineQuery {
    terms: DoctrineTerms,
    serie: Option<Serie>,
    kind: KindFilter,
    page: NonZeroU32,
    page_size: PageSize,
}

impl DoctrineQuery {
    /// Assemble une requête et vérifie que sa page est atteignable.
    ///
    /// # Errors
    ///
    /// [`DomainError::PageBeyondReach`] si `page × page_size` dépasse [`REACHABLE_RESULTS`].
    pub fn new(
        terms: DoctrineTerms,
        serie: Option<Serie>,
        kind: KindFilter,
        page: NonZeroU32,
        page_size: PageSize,
    ) -> Result<Self, DomainError> {
        Self { terms, serie, kind, page, page_size }.checked()
    }

    /// Mots cherchés.
    pub fn terms(&self) -> &DoctrineTerms {
        &self.terms
    }

    /// Série demandée, le cas échéant.
    pub fn serie(&self) -> Option<&Serie> {
        self.serie.as_ref()
    }

    /// Natures demandées.
    pub fn kind(&self) -> KindFilter {
        self.kind
    }

    /// Numéro de page, à partir de 1.
    pub fn page(&self) -> NonZeroU32 {
        self.page
    }

    /// Nombre de résultats par page.
    pub fn page_size(&self) -> PageSize {
        self.page_size
    }

    /// Rang du premier résultat de la page, à partir de 0 : `(page − 1) × page_size`. Au plus
    /// 9 999 pour une requête construite.
    pub fn offset(&self) -> u32 {
        self.page.get().saturating_sub(1).saturating_mul(self.page_size.get())
    }

    fn checked(self) -> Result<Self, DomainError> {
        if !within_reach(u64::from(self.page.get()), u64::from(self.page_size.get())) {
            return Err(DomainError::PageBeyondReach {
                page: self.page.get(),
                page_size: self.page_size.get(),
                max: REACHABLE_RESULTS,
            });
        }
        Ok(self)
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn query(page: u32, size: u32) -> Result<DoctrineQuery, DomainError> {
        DoctrineQuery::new(
            DoctrineTerms::parse("donation abattement").unwrap(),
            Some(Serie::parse("ENR").unwrap()),
            KindFilter::News,
            NonZeroU32::new(page).unwrap(),
            PageSize::new(size).unwrap(),
        )
    }

    #[test]
    fn a_query_keeps_what_it_was_given() {
        let query = query(3, 25).unwrap();
        assert_eq!(query.terms().as_str(), "donation abattement");
        assert_eq!(query.serie().map(Serie::as_str), Some("ENR"));
        assert_eq!(query.kind(), KindFilter::News);
        assert_eq!((query.page().get(), query.page_size().get()), (3, 25));
    }

    #[test]
    fn a_query_without_serie_has_none() {
        let query = DoctrineQuery::new(
            DoctrineTerms::parse("usufruit").unwrap(),
            None,
            KindFilter::default(),
            NonZeroU32::MIN,
            PageSize::DEFAULT,
        )
        .unwrap();
        assert_eq!((query.serie(), query.kind(), query.offset()), (None, KindFilter::Doctrine, 0));
    }

    #[test]
    fn offset_is_the_rank_of_the_first_hit() {
        for (page, size, offset) in
            [(1, 10, 0), (2, 10, 10), (3, 25, 50), (100, 100, 9_900), (10_000, 1, 9_999)]
        {
            assert_eq!(query(page, size).unwrap().offset(), offset);
        }
    }

    #[test]
    fn a_page_beyond_the_window_is_refused() {
        assert_eq!(query(100, 100).unwrap().page().get(), 100);
        assert_eq!(query(1_000, 10).unwrap().offset(), 9_990);
        for (page, size) in [(101, 100), (1_001, 10), (10_001, 1), (u32::MAX, 100)] {
            assert_eq!(
                query(page, size),
                Err(DomainError::PageBeyondReach { page, page_size: size, max: 10_000 })
            );
        }
    }

    proptest! {
        /// Une requête construite reste dans la fenêtre du portail : `offset + limit ≤ 10 000`.
        #[test]
        fn accepted_queries_fit_the_portal_window(page in 1_u32..=12_000, size in 1_u32..=100) {
            let reachable = u64::from(page) * u64::from(size) <= REACHABLE_RESULTS;
            let offset = query(page, size).ok().map(|query| query.offset());
            prop_assert_eq!(offset, reachable.then_some((page - 1) * size));
        }
    }
}
