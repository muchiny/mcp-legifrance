//! Requête de recherche validée de `fr_search_bofip` (05 §5.14), et la façon dont ses mots
//! doivent correspondre au texte ([`DoctrineMatch`], ADR-0031).

use std::num::NonZeroU32;

use crate::{
    error::DomainError,
    search::{PageSize, REACHABLE_RESULTS, within_reach},
};

use super::{id::Serie, kind::KindFilter, terms::DoctrineTerms};

/// Façon dont les mots d'une recherche doivent correspondre au texte d'une publication
/// (ADR-0031).
///
/// La recherche du portail est approchée : elle retrouve aussi les mots qui ressemblent au mot
/// cherché (« donation » trouve « fonction »). Une recherche demande donc d'abord les mots
/// exacts ; la recherche approchée ne sert qu'en secours, quand aucune publication ne les porte
/// tous. Qui décide du secours : le cas d'usage de `lf-application`, pas l'adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DoctrineMatch {
    /// Tous les mots, tels qu'ils sont écrits, accents et casse ignorés.
    #[default]
    Exact,
    /// Tous les mots, à une ou deux lettres près, le dernier en préfixe : la recherche du portail.
    Approximate,
}

/// Recherche dans le BOFiP : des mots, une série facultative, les natures voulues, une page, et
/// la façon dont les mots doivent correspondre.
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
    matching: DoctrineMatch,
}

impl DoctrineQuery {
    /// Assemble une requête en mots exacts et vérifie que sa page est atteignable.
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
        Self { terms, serie, kind, page, page_size, matching: DoctrineMatch::default() }.checked()
    }

    /// La même requête, mots approchants admis : mêmes mots, mêmes filtres, même page.
    #[must_use]
    pub fn approximate(self) -> Self {
        Self { matching: DoctrineMatch::Approximate, ..self }
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

    /// Façon dont les mots doivent correspondre : exacte pour une requête neuve.
    pub fn matching(&self) -> DoctrineMatch {
        self.matching
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

    /// Une requête neuve demande les mots exacts ; `approximate` ne change que cela.
    #[test]
    fn a_new_query_asks_for_exact_words_and_approximate_changes_nothing_else() {
        let exact = query(3, 25).unwrap();
        assert_eq!(exact.matching(), DoctrineMatch::Exact);
        assert_eq!(DoctrineMatch::default(), DoctrineMatch::Exact);
        let approximate = exact.clone().approximate();
        assert_eq!(approximate.matching(), DoctrineMatch::Approximate);
        assert_ne!(approximate, exact);
        assert_eq!(
            (approximate.terms(), approximate.serie(), approximate.kind()),
            (exact.terms(), exact.serie(), exact.kind())
        );
        assert_eq!(
            (approximate.page(), approximate.page_size(), approximate.offset()),
            (exact.page(), exact.page_size(), 50)
        );
        // Demandé deux fois : la même requête.
        assert_eq!(approximate.clone().approximate(), approximate);
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
