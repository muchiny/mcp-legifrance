//! Objets de valeur de `fr_search` : texte de la requête, taille de page, mode de
//! correspondance (03 §4.1, 00 §7).
//!
//! Validés sans réseau : une valeur construite est déjà conforme, l'adapter n'a plus qu'à la
//! traduire.

use std::num::NonZeroU32;

use crate::{
    date::{DateRange, VersionDate},
    error::DomainError,
    fond::Fond,
};

/// Longueur maximale, en caractères, d'une requête normalisée.
///
/// La borne de 500 est celle du contrat (05 §7.3) ; la limite réelle de l'API reste à
/// vérifier [V-81 · longueur-max-query].
pub const QUERY_MAX_CHARS: usize = 500;

/// Texte de recherche normalisé : sans caractère de contrôle, blancs repliés en une espace,
/// ni vide ni plus long que [`QUERY_MAX_CHARS`] caractères.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryText(String);

impl QueryText {
    /// Normalise `input` puis le valide.
    ///
    /// La normalisation retire les blancs de tête et de queue, replie chaque suite de blancs
    /// Unicode en une espace et supprime les caractères de contrôle. La longueur est mesurée
    /// après normalisation. Borne : [V-81 · longueur-max-query].
    ///
    /// ```
    /// use lf_domain::search::QueryText;
    ///
    /// let query = QueryText::parse("  droit \t de\u{a0}grève \u{0}").unwrap();
    /// assert_eq!(query.as_str(), "droit de grève");
    /// ```
    ///
    /// Une requête faite de blancs ou de contrôles est refusée :
    ///
    /// ```
    /// use lf_domain::{DomainError, search::QueryText};
    ///
    /// assert_eq!(QueryText::parse(" \t\u{1} "), Err(DomainError::EmptyQuery));
    /// ```
    ///
    /// # Errors
    ///
    /// [`DomainError::EmptyQuery`] si rien ne reste après normalisation,
    /// [`DomainError::QueryTooLong`] au-delà de [`QUERY_MAX_CHARS`] caractères.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let normalized = normalize(input);
        match normalized.chars().count() {
            0 => Err(DomainError::EmptyQuery),
            len if len > QUERY_MAX_CHARS => {
                Err(DomainError::QueryTooLong { len, max: QUERY_MAX_CHARS })
            }
            _ => Ok(Self(normalized)),
        }
    }

    /// Texte normalisé.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Découpe sur les blancs Unicode, retire les caractères de contrôle de chaque mot, écarte les
/// mots devenus vides et joint par une espace.
fn normalize(input: &str) -> String {
    input
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Nombre de résultats par page de `fr_search`, entre [`MIN`](Self::MIN) et
/// [`MAX`](Self::MAX) inclus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageSize(u32);

impl PageSize {
    /// Plus petite taille de page.
    pub const MIN: u32 = 1;
    /// Plus grande taille de page.
    pub const MAX: u32 = 100;
    /// Taille de page par défaut (parité avec la référence).
    pub const DEFAULT: Self = Self(10);

    /// Valide `value`.
    ///
    /// # Errors
    ///
    /// [`DomainError::OutOfRange`] hors de `MIN..=MAX`.
    pub fn new(value: u32) -> Result<Self, DomainError> {
        Self::checked(value)
    }

    /// Valeur numérique.
    pub fn get(self) -> u32 {
        self.0
    }

    fn checked(value: u32) -> Result<Self, DomainError> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::OutOfRange {
                field: "page_size",
                value: i64::from(value),
                min: i64::from(Self::MIN),
                max: i64::from(Self::MAX),
            })
        }
    }
}

impl Default for PageSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Mode de correspondance des mots de la requête (paramètre `match`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MatchMode {
    /// Au moins un des mots (`UN_DES_MOTS`) ; défaut, par parité avec la référence.
    #[default]
    Any,
    /// Tous les mots (`TOUS_LES_MOTS_DANS_UN_CHAMP`).
    All,
    /// Expression exacte (`EXACTE`).
    Exact,
}

/// Nombre de résultats au-delà duquel l'API ne livre plus rien : fenêtre supposée,
/// `[V-83 · pagination-profonde]`.
///
/// Une page est atteignable si `page * page_size <= REACHABLE_RESULTS`.
pub const REACHABLE_RESULTS: u64 = 10_000;

/// Requête de recherche validée de `fr_search` (03 §4.1, 02 §5.1).
///
/// Les champs sont privés : les invariants de [`new`](Self::new) tiennent pour toute valeur
/// construite (même choix que [`DateRange`]) :
///
/// - `version_date` n'est admise que pour un fond qui la consomme (`LODA`, `CODE`) ;
/// - `date_range` n'est admise que pour un fond à filtre de période (tous sauf `CODE`) ;
/// - la page demandée reste dans la fenêtre de [`REACHABLE_RESULTS`] résultats.
///
/// `Eq + Hash` : la valeur sert de clé de cache (00 §9). Elle ne porte pas de
/// `typePagination`, que l'adapter choisit seul (04 §6.6).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchQuery {
    fond: Fond,
    text: QueryText,
    page: NonZeroU32,
    page_size: PageSize,
    mode: MatchMode,
    date_range: Option<DateRange>,
    version_date: Option<VersionDate>,
}

impl SearchQuery {
    /// Assemble une requête et vérifie ses invariants.
    ///
    /// Une période inversée est déjà refusée à la construction de [`DateRange`].
    ///
    /// # Errors
    ///
    /// Dans cet ordre : [`DomainError::VersionDateNotApplicable`],
    /// [`DomainError::DateFilterNotApplicable`], [`DomainError::PageBeyondReach`].
    pub fn new(
        fond: Fond,
        text: QueryText,
        page: NonZeroU32,
        page_size: PageSize,
        mode: MatchMode,
        date_range: Option<DateRange>,
        version_date: Option<VersionDate>,
    ) -> Result<Self, DomainError> {
        Self { fond, text, page, page_size, mode, date_range, version_date }.checked()
    }

    /// Fond interrogé.
    pub fn fond(&self) -> Fond {
        self.fond
    }

    /// Texte normalisé de la requête.
    pub fn text(&self) -> &QueryText {
        &self.text
    }

    /// Numéro de page, à partir de 1.
    pub fn page(&self) -> NonZeroU32 {
        self.page
    }

    /// Nombre de résultats par page.
    pub fn page_size(&self) -> PageSize {
        self.page_size
    }

    /// Mode de correspondance des mots.
    pub fn mode(&self) -> MatchMode {
        self.mode
    }

    /// Période demandée, le cas échéant.
    pub fn date_range(&self) -> Option<DateRange> {
        self.date_range
    }

    /// Date de version demandée, le cas échéant.
    pub fn version_date(&self) -> Option<VersionDate> {
        self.version_date
    }

    fn checked(self) -> Result<Self, DomainError> {
        let fond = self.fond;
        if self.version_date.is_some() && !fond.needs_version_date() {
            return Err(DomainError::VersionDateNotApplicable { fond });
        }
        if self.date_range.is_some() && !fond.has_date_filter() {
            return Err(DomainError::DateFilterNotApplicable { fond });
        }
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

/// Vrai s'il existe une page suivante atteignable après `page`, sur `total` résultats.
///
/// La page suivante doit contenir au moins un résultat (`page * page_size < total`) et rester
/// dans la fenêtre de [`REACHABLE_RESULTS`] résultats `[V-83 · pagination-profonde]`.
///
/// ```
/// use std::num::NonZeroU32;
/// use lf_domain::search::{PageSize, has_more};
///
/// let page = |n| NonZeroU32::new(n).unwrap();
/// assert!(has_more(page(1), PageSize::DEFAULT, 11));
/// assert!(!has_more(page(333), PageSize::new(30).unwrap(), 50_000));
/// ```
pub fn has_more(page: NonZeroU32, page_size: PageSize, total: u64) -> bool {
    let (page, size) = (u64::from(page.get()), u64::from(page_size.get()));
    page * size < total && within_reach(page + 1, size)
}

/// Fenêtre unique partagée par le rejet d'une page et par [`has_more`].
fn within_reach(page: u64, page_size: u64) -> bool {
    page * page_size <= REACHABLE_RESULTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::HashSet;

    fn text(s: &str) -> QueryText {
        QueryText::parse(s).unwrap()
    }

    fn page(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).unwrap()
    }

    fn size(n: u32) -> PageSize {
        PageSize::new(n).unwrap()
    }

    fn date(s: &str) -> VersionDate {
        VersionDate::parse(s).unwrap()
    }

    fn range() -> Option<DateRange> {
        DateRange::new(Some(date("2020-01-01")), None).unwrap()
    }

    fn query(
        fond: Fond,
        page_n: u32,
        size_n: u32,
        date_range: Option<DateRange>,
        version_date: Option<VersionDate>,
    ) -> Result<SearchQuery, DomainError> {
        SearchQuery::new(
            fond,
            text("bail"),
            page(page_n),
            size(size_n),
            MatchMode::default(),
            date_range,
            version_date,
        )
    }

    #[test]
    fn query_is_trimmed_and_folded() {
        assert_eq!(text("  droit \t\r\n de\u{a0}\u{a0}grève  ").as_str(), "droit de grève");
    }

    #[test]
    fn query_control_chars_are_stripped() {
        assert_eq!(text("a\u{0}b \u{1} c\u{7f}\u{9b}").as_str(), "ab c");
    }

    #[test]
    fn blank_query_is_empty() {
        for blank in ["", "   ", "\t\n", "\u{0}\u{1}", " \u{7} \u{1b} "] {
            assert_eq!(QueryText::parse(blank), Err(DomainError::EmptyQuery), "{blank:?}");
        }
    }

    #[test]
    fn query_too_long_is_invalid_arg() {
        let at_max = "é".repeat(QUERY_MAX_CHARS);
        assert_eq!(text(&at_max).as_str(), at_max);
        assert_eq!(
            QueryText::parse(&"é".repeat(QUERY_MAX_CHARS + 1)),
            Err(DomainError::QueryTooLong { len: 501, max: 500 })
        );
    }

    #[test]
    fn query_length_is_counted_after_normalization() {
        let padded = format!("  {}  \u{0}", "a".repeat(QUERY_MAX_CHARS));
        assert_eq!(text(&padded).as_str().len(), QUERY_MAX_CHARS);
    }

    #[test]
    fn page_size_bounds() {
        let out = |value| DomainError::OutOfRange { field: "page_size", value, min: 1, max: 100 };
        assert_eq!(PageSize::new(0), Err(out(0)));
        assert_eq!(PageSize::new(1).map(PageSize::get), Ok(1));
        assert_eq!(PageSize::new(100).map(PageSize::get), Ok(100));
        assert_eq!(PageSize::new(101), Err(out(101)));
    }

    #[test]
    fn defaults_match_reference() {
        assert_eq!(PageSize::default().get(), 10);
        assert_eq!(MatchMode::default(), MatchMode::Any);
    }

    #[test]
    fn version_date_outside_loda_code_is_rejected() {
        let v = Some(date("2024-01-01"));
        for fond in Fond::ALL {
            let got = query(fond, 1, 10, None, v);
            if fond.needs_version_date() {
                assert_eq!(got.map(|q| q.version_date()), Ok(v), "{fond}");
            } else {
                assert_eq!(got, Err(DomainError::VersionDateNotApplicable { fond }), "{fond}");
            }
        }
    }

    #[test]
    fn date_range_on_code_is_rejected() {
        for fond in Fond::ALL {
            let got = query(fond, 1, 10, range(), None);
            if fond == Fond::Code {
                assert_eq!(got, Err(DomainError::DateFilterNotApplicable { fond }));
            } else {
                assert_eq!(got.map(|q| q.date_range()), Ok(range()), "{fond}");
            }
        }
    }

    #[test]
    fn version_date_is_checked_before_date_range() {
        assert_eq!(
            query(Fond::Juri, 1, 10, range(), Some(date("2024-01-01"))),
            Err(DomainError::VersionDateNotApplicable { fond: Fond::Juri })
        );
    }

    #[test]
    fn page_beyond_reachable_window_is_rejected() {
        assert_eq!(query(Fond::Juri, 100, 100, None, None).map(|q| q.page()), Ok(page(100)));
        assert_eq!(
            query(Fond::Juri, 101, 100, None, None),
            Err(DomainError::PageBeyondReach { page: 101, page_size: 100, max: 10_000 })
        );
        assert_eq!(
            query(Fond::Juri, u32::MAX, 100, None, None),
            Err(DomainError::PageBeyondReach { page: u32::MAX, page_size: 100, max: 10_000 })
        );
    }

    #[test]
    fn accessors_return_the_validated_fields() {
        let q = SearchQuery::new(
            Fond::Loda,
            text(" bail "),
            page(3),
            size(25),
            MatchMode::Exact,
            range(),
            Some(date("2024-01-01")),
        )
        .unwrap();
        assert_eq!(
            (q.fond(), q.text().as_str(), q.page(), q.page_size(), q.mode()),
            (Fond::Loda, "bail", page(3), size(25), MatchMode::Exact)
        );
        assert_eq!((q.date_range(), q.version_date()), (range(), Some(date("2024-01-01"))));
    }

    #[test]
    fn normalized_queries_share_a_cache_key() {
        let a = query(Fond::Juri, 1, 10, None, None).unwrap();
        let b = SearchQuery::new(
            Fond::Juri,
            text("  bail\t"),
            page(1),
            PageSize::default(),
            MatchMode::Any,
            None,
            None,
        )
        .unwrap();
        assert_eq!(HashSet::from([a.clone(), b]).len(), 1);
        assert_ne!(a, query(Fond::Juri, 2, 10, None, None).unwrap());
    }

    #[test]
    fn has_more_false_at_reachable_window_end() {
        assert!(has_more(page(1), size(10), 11));
        assert!(!has_more(page(1), size(10), 10));
        assert!(has_more(page(99), size(100), 20_000));
        assert!(!has_more(page(100), size(100), 20_000));
        assert!(has_more(page(332), size(30), 50_000));
        assert!(!has_more(page(333), size(30), 50_000));
        assert!(!has_more(page(u32::MAX), size(100), u64::MAX));
    }

    proptest! {
        #[test]
        fn accepted_pages_stay_within_reach(p in 1u32..=10_000, n in 1u32..=100) {
            let reachable = u64::from(p) * u64::from(n) <= REACHABLE_RESULTS;
            prop_assert_eq!(query(Fond::Juri, p, n, None, None).is_ok(), reachable);
        }

        #[test]
        fn parse_never_panics_and_output_is_normal(s in "\\PC*|[\\x00-\\x20a ]*") {
            if let Ok(q) = QueryText::parse(&s) {
                let t = q.as_str();
                prop_assert!(!t.chars().any(char::is_control));
                prop_assert!(!t.contains("  ") && t.trim() == t && !t.is_empty());
                prop_assert!(t.chars().count() <= QUERY_MAX_CHARS);
                prop_assert_eq!(QueryText::parse(t), Ok(q.clone()));
            }
        }
    }
}
