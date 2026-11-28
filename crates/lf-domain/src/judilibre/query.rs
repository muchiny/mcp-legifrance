//! Requête de `fr_search_judilibre` (ADR-0035, décisions 3 et 4 ; 05 §5.18).
//!
//! Validée sans réseau : une valeur construite est déjà conforme, et l'adapter n'a plus qu'à la
//! traduire en paramètres de la liste fermée.

use std::num::NonZeroU32;

use super::{CourtSet, SearchPart};
use crate::{
    date::{DateRange, VersionDate},
    error::DomainError,
    search::{MatchMode, PageSize, QueryText, REACHABLE_RESULTS, within_reach},
};

/// Plus grande taille de page de Judilibre (spécification 1.2.5 de l'API : `page_size` ≤ 50).
pub const CASE_LAW_PAGE_SIZE_MAX: u32 = 50;

/// Ordre des résultats (`sort` de `fr_search_judilibre`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CaseLawSort {
    /// Pertinence (`sort=scorepub`), le défaut : le score, relevé par la publication.
    #[default]
    Relevance,
    /// Date de la décision, la plus récente d'abord (`sort=date&order=desc`).
    Date,
}

impl CaseLawSort {
    /// Les deux ordres.
    pub const ALL: [Self; 2] = [Self::Relevance, Self::Date];

    /// Valeurs admises de `sort`.
    pub const NAMES: [&'static str; 2] = ["relevance", "date"];

    /// Nom de l'ordre.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Date => "date",
        }
    }

    /// Ordre d'un nom, insensible à la casse et aux espaces de bord.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|sort| sort.name().eq_ignore_ascii_case(raw))
    }
}

/// Filtres d'une recherche : tout ce qui n'est ni les mots ni la page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CaseLawFilters {
    /// Correspondance des mots (`operator`).
    pub mode: MatchMode,
    /// Juridictions cherchées (`jurisdiction`, toujours envoyé).
    pub courts: CourtSet,
    /// Partie où chercher (`field`) ; tout le texte si absente.
    pub part: Option<SearchPart>,
    /// Date de décision la plus ancienne (`date_start`).
    pub date_from: Option<VersionDate>,
    /// Date de décision la plus récente (`date_end`).
    pub date_to: Option<VersionDate>,
    /// Ordre des résultats.
    pub sort: CaseLawSort,
}

/// Requête validée de `fr_search_judilibre`.
///
/// Invariants de toute valeur construite : `date_from` ≤ `date_to` ; `page_size` ≤
/// [`CASE_LAW_PAGE_SIZE_MAX`] ; `page × page_size` ≤ [`REACHABLE_RESULTS`] (au-delà, l'API rend
/// `416` : mesuré le 2026-11-27).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CaseLawQuery {
    text: QueryText,
    filters: CaseLawFilters,
    page: NonZeroU32,
    page_size: PageSize,
}

impl CaseLawQuery {
    /// Assemble une requête et vérifie ses invariants.
    ///
    /// # Errors
    ///
    /// Dans cet ordre : [`DomainError::InvertedDateRange`], [`DomainError::OutOfRange`] pour une
    /// `page_size` au-delà de [`CASE_LAW_PAGE_SIZE_MAX`], [`DomainError::PageBeyondReach`].
    pub fn new(
        text: QueryText,
        filters: CaseLawFilters,
        page: NonZeroU32,
        page_size: PageSize,
    ) -> Result<Self, DomainError> {
        Self { text, filters, page, page_size }.checked()
    }

    /// Le contrôle vit ici et non dans `new`, que cargo-mutants ne mute jamais.
    fn checked(self) -> Result<Self, DomainError> {
        DateRange::new(self.filters.date_from, self.filters.date_to)?;
        let size = self.page_size.get();
        if size > CASE_LAW_PAGE_SIZE_MAX {
            return Err(DomainError::OutOfRange {
                field: "page_size",
                value: i64::from(size),
                min: i64::from(PageSize::MIN),
                max: i64::from(CASE_LAW_PAGE_SIZE_MAX),
            });
        }
        if !within_reach(u64::from(self.page.get()), u64::from(size)) {
            return Err(DomainError::PageBeyondReach {
                page: self.page.get(),
                page_size: size,
                max: REACHABLE_RESULTS,
            });
        }
        Ok(self)
    }

    /// Mots cherchés, normalisés.
    pub fn text(&self) -> &QueryText {
        &self.text
    }

    /// Filtres.
    pub fn filters(&self) -> &CaseLawFilters {
        &self.filters
    }

    /// Page demandée, à partir de 1 (l'API compte à partir de 0).
    pub fn page(&self) -> NonZeroU32 {
        self.page
    }

    /// Nombre de résultats par page.
    pub fn page_size(&self) -> PageSize {
        self.page_size
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::judilibre::Court;

    fn filters(from: Option<&str>, to: Option<&str>) -> CaseLawFilters {
        CaseLawFilters {
            mode: MatchMode::All,
            courts: CourtSet::ALL,
            part: None,
            date_from: from.map(|date| VersionDate::parse(date).unwrap()),
            date_to: to.map(|date| VersionDate::parse(date).unwrap()),
            sort: CaseLawSort::Relevance,
        }
    }

    fn query(page: u32, size: u32, filters: CaseLawFilters) -> Result<CaseLawQuery, DomainError> {
        CaseLawQuery::new(
            QueryText::parse("donation nue-propriété").unwrap(),
            filters,
            NonZeroU32::new(page).unwrap(),
            PageSize::new(size).unwrap(),
        )
    }

    #[test]
    fn page_window_over_10000_refused() {
        let ok = query(200, 50, filters(None, None)).unwrap();
        assert_eq!((ok.page().get(), ok.page_size().get()), (200, 50));
        assert_eq!(
            query(201, 50, filters(None, None)),
            Err(DomainError::PageBeyondReach { page: 201, page_size: 50, max: 10_000 })
        );
        assert_eq!(
            query(u32::MAX, 1, filters(None, None)),
            Err(DomainError::PageBeyondReach { page: u32::MAX, page_size: 1, max: 10_000 })
        );
        assert!(query(10_000, 1, filters(None, None)).is_ok());
    }

    #[test]
    fn page_size_is_capped_at_50() {
        assert!(query(1, 50, filters(None, None)).is_ok());
        assert_eq!(
            query(1, 51, filters(None, None)),
            Err(DomainError::OutOfRange { field: "page_size", value: 51, min: 1, max: 50 })
        );
    }

    #[test]
    fn dates_must_be_in_order() {
        assert!(query(1, 10, filters(Some("2020-01-01"), Some("2020-01-01"))).is_ok());
        assert!(query(1, 10, filters(Some("2020-01-01"), None)).is_ok());
        assert!(query(1, 10, filters(None, Some("2020-01-01"))).is_ok());
        let inverted = Err(DomainError::InvertedDateRange {
            from: VersionDate::parse("2021-01-01").unwrap(),
            to: VersionDate::parse("2020-12-31").unwrap(),
        });
        assert_eq!(query(1, 10, filters(Some("2021-01-01"), Some("2020-12-31"))), inverted);
        // La date passe avant la taille de page et la fenêtre.
        assert_eq!(query(400, 51, filters(Some("2021-01-01"), Some("2020-12-31"))), inverted);
        assert_eq!(
            query(400, 51, filters(None, None)),
            Err(DomainError::OutOfRange { field: "page_size", value: 51, min: 1, max: 50 })
        );
    }

    #[test]
    fn accessors_return_what_was_given() {
        let mut given = filters(Some("2019-01-01"), Some("2024-12-31"));
        given.courts = CourtSet::of([Court::Appeal]).unwrap();
        given.part = Some(SearchPart::Motivations);
        given.sort = CaseLawSort::Date;
        given.mode = MatchMode::Exact;
        let query = query(3, 20, given).unwrap();
        assert_eq!(query.text().as_str(), "donation nue-propriété");
        assert_eq!(query.filters(), &given);
        assert_eq!((query.page().get(), query.page_size().get()), (3, 20));
    }

    #[test]
    fn sort_names_round_trip() {
        assert_eq!(CaseLawSort::ALL.map(CaseLawSort::name), CaseLawSort::NAMES);
        for sort in CaseLawSort::ALL {
            assert_eq!(
                CaseLawSort::parse(&format!(" {} ", sort.name().to_uppercase())),
                Some(sort)
            );
        }
        assert_eq!(CaseLawSort::parse("scorepub"), None);
        assert_eq!(CaseLawSort::default(), CaseLawSort::Relevance);
    }
}
