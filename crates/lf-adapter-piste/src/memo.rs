//! Mémo de `typePagination` (04 §6.6, §12.3).
//!
//! PISTE choisit le mode de pagination de `/search` (`typePagination`) et peut répondre un mode
//! différent de celui demandé (le fond `CODE_DATE` répond `ARTICLE` à `DEFAUT`). Pour que les
//! pages suivantes d'une même requête redemandent le mode que PISTE a réellement employé,
//! l'adapter retient la **dernière valeur reçue** pour la requête sans sa page.
//!
//! La clé (`MemoKey`) réunit le fond, le texte normalisé, le mode de correspondance, la
//! `version_date`, la plage de dates et la taille de page ; la page en est exclue (04 §6.6,
//! §12.3). Le mémo garde 1 024 entrées au plus, chacune une heure (`TTL`) ; au-delà, l'écriture la
//! plus ancienne est évincée.
//!
//! Le texte de la requête n'est jamais journalisé : ni la clé, ni l'entrée, ni le mémo
//! n'implémentent `Debug`. Le `Mutex` interne n'est jamais tenu à travers un `await` : toutes les
//! méthodes sont synchrones et rendent le verrou avant de retourner. Un verrou empoisonné est
//! repris tel quel, car l'état n'a aucun invariant en plusieurs étapes.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use lf_domain::{
    date::{DateRange, VersionDate},
    fond::{Fond, PaginationMode},
    search::{MatchMode, QueryText, SearchQuery},
};

use crate::time::MonoClock;

/// Durée de vie d'une entrée : une heure (04 §12.3).
const TTL: Duration = Duration::from_secs(3_600);

/// Nombre maximal d'entrées (04 §12.3).
const CAPACITY: usize = 1_024;

/// Clé du mémo : une requête sans sa page.
///
/// Sans `Debug` : elle contient le texte de la requête.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct MemoKey {
    fond: Fond,
    text: QueryText,
    mode: MatchMode,
    version_date: Option<VersionDate>,
    date_range: Option<DateRange>,
    page_size: u32,
}

impl MemoKey {
    /// Clé d'une requête : tous ses champs sauf la page.
    pub(crate) fn from_query(query: &SearchQuery) -> Self {
        Self {
            fond: query.fond(),
            text: query.text().clone(),
            mode: query.mode(),
            version_date: query.version_date(),
            date_range: query.date_range(),
            page_size: query.page_size().get(),
        }
    }
}

/// Une valeur mémorisée.
struct Entry {
    mode: PaginationMode,
    /// Lecture de l'horloge à la dernière écriture.
    stored_at: Duration,
    /// Rang d'écriture : l'éviction vise le plus petit, les horodatages pouvant être égaux.
    seq: u64,
}

/// Contenu protégé par le verrou.
#[derive(Default)]
struct State {
    entries: HashMap<MemoKey, Entry>,
    next_seq: u64,
}

/// Vrai tant que l'entrée a moins d'une heure : elle expire dès qu'elle atteint `TTL`.
fn is_fresh(stored_at: Duration, now: Duration) -> bool {
    now.saturating_sub(stored_at) < TTL
}

impl State {
    /// Écrit (ou réécrit) une entrée, la rajeunit, puis évince le surplus.
    ///
    /// Le nombre d'évictions est fixé avant la boucle : elle progresse même si sa borne était
    /// altérée.
    fn put(&mut self, key: MemoKey, mode: PaginationMode, now: Duration) {
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        self.entries.insert(key, Entry { mode, stored_at: now, seq });
        for _ in 0..self.entries.len().saturating_sub(CAPACITY) {
            self.evict_oldest();
        }
    }

    /// Retire l'écriture la plus ancienne ; sans effet sur un état vide.
    fn evict_oldest(&mut self) {
        let oldest =
            self.entries.iter().min_by_key(|(_, entry)| entry.seq).map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            self.entries.remove(&key);
        }
    }
}

/// Mémo de `typePagination`, par requête sans page.
///
/// Sans `Debug` : voir la documentation du module.
pub(crate) struct PaginationMemo<K: MonoClock> {
    clock: K,
    state: Mutex<State>,
}

impl<K: MonoClock> PaginationMemo<K> {
    /// Crée un mémo vide lisant le temps sur `clock`.
    pub(crate) fn new(clock: K) -> Self {
        Self { clock, state: Mutex::new(State::default()) }
    }

    /// Prend le verrou ; un verrou empoisonné est repris.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Mode mémorisé pour `key`. Une entrée expirée est retirée et rend `None`.
    pub(crate) fn lookup(&self, key: &MemoKey) -> Option<PaginationMode> {
        let now = self.clock.now();
        let mut state = self.lock();
        let found =
            state.entries.get(key).map(|entry| (entry.mode, is_fresh(entry.stored_at, now)));
        match found {
            Some((mode, true)) => Some(mode),
            Some((_, false)) => {
                state.entries.remove(key);
                None
            }
            None => None,
        }
    }

    /// Mémorise `mode` pour `key`.
    pub(crate) fn record(&self, key: MemoKey, mode: PaginationMode) {
        let now = self.clock.now();
        self.lock().put(key, mode, now);
    }

    /// Mode à envoyer pour `query` (règles 2 à 4 de 04 §6.6).
    pub(crate) fn mode_for(&self, query: &SearchQuery) -> PaginationMode {
        let memoized = self.lookup(&MemoKey::from_query(query));
        choose_mode(query.page().get(), query.fond(), memoized)
    }

    /// Retient le mode que PISTE a répondu pour `query` (`received_mode` de la réponse).
    ///
    /// Sans mode reçu (`typePagination` absent ou inconnu), rien n'est écrit : une absence n'est
    /// pas une valeur reçue, et une page 2 de `CODE` doit retomber sur la règle 3 de 04 §6.6
    /// (`ARTICLE`) ; une entrée déjà présente reste telle quelle.
    pub(crate) fn remember(&self, query: &SearchQuery, received: Option<PaginationMode>) {
        if let Some(mode) = received {
            self.record(MemoKey::from_query(query), mode);
        }
    }
}

/// Décision pure du mode à envoyer (règles 2 à 4 de 04 §6.6).
///
/// La page 1 part toujours en `DEFAUT`. Une page suivante reprend le mode mémorisé ; sans mémo
/// (redémarrage, entrée expirée ou évincée), `CODE` repart en `ARTICLE` et les autres fonds en
/// `DEFAUT`.
pub(crate) fn choose_mode(
    page: u32,
    fond: Fond,
    memoized: Option<PaginationMode>,
) -> PaginationMode {
    if page <= 1 {
        return PaginationMode::Defaut;
    }
    memoized.unwrap_or(if fond == Fond::Code {
        PaginationMode::Article
    } else {
        PaginationMode::Defaut
    })
}

/// Lecture tolérante de `typePagination` dans une réponse : le mode reçu, s'il y en a un.
///
/// Une valeur absente rend `None` sans journal. Une valeur inconnue rend `None` avec un `warn!`
/// qui ne cite que sa longueur : elle vient du corps de la réponse et n'est jamais journalisée.
/// Dans les deux cas le mémo n'enregistre rien (`PaginationMemo::remember`) ; la page rendue à
/// l'appelant, elle, vaut alors `PaginationMode::default()` (`unwrap_or_default`).
pub(crate) fn received_mode(raw: Option<&str>) -> Option<PaginationMode> {
    let value = raw?;
    let mode = PaginationMode::from_api(value);
    if mode.is_none() {
        tracing::warn!(len = value.len(), "unknown typePagination in PISTE response");
    }
    mode
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

    use lf_domain::search::PageSize;
    use tracing::Level;

    use super::*;
    use crate::testing::capture_at;
    use crate::time::ManualClock;

    const ONE_HOUR: Duration = Duration::from_secs(3_600);
    const JUST_UNDER_AN_HOUR: Duration = Duration::new(3_599, 999_999_999);

    impl PaginationMemo<ManualClock> {
        fn len(&self) -> usize {
            self.lock().entries.len()
        }
    }

    fn day(text: &str) -> VersionDate {
        VersionDate::parse(text).unwrap()
    }

    fn query(fond: Fond, text: &str, page: u32) -> SearchQuery {
        SearchQuery::new(
            fond,
            QueryText::parse(text).unwrap(),
            NonZeroU32::new(page).unwrap(),
            PageSize::DEFAULT,
            MatchMode::Any,
            None,
            None,
        )
        .unwrap()
    }

    fn memo() -> (PaginationMemo<ManualClock>, ManualClock) {
        let clock = ManualClock::new();
        (PaginationMemo::new(clock.clone()), clock)
    }

    fn numbered(i: usize) -> MemoKey {
        MemoKey::from_query(&query(Fond::Loda, &format!("requete {i}"), 1))
    }

    #[test]
    fn page_one_is_always_defaut() {
        for fond in Fond::ALL {
            for memoized in [None, Some(PaginationMode::Article), Some(PaginationMode::Defaut)] {
                assert_eq!(choose_mode(1, fond, memoized), PaginationMode::Defaut, "{fond:?}");
                assert_eq!(choose_mode(0, fond, memoized), PaginationMode::Defaut, "{fond:?}");
            }
        }
    }

    #[test]
    fn later_pages_follow_the_memo() {
        for fond in Fond::ALL {
            assert_eq!(
                choose_mode(2, fond, Some(PaginationMode::Article)),
                PaginationMode::Article,
                "{fond:?}"
            );
            assert_eq!(
                choose_mode(7, fond, Some(PaginationMode::Defaut)),
                PaginationMode::Defaut,
                "{fond:?}"
            );
        }
    }

    #[test]
    fn code_page_two_without_memo_is_article_and_other_fonds_defaut() {
        assert_eq!(choose_mode(2, Fond::Code, None), PaginationMode::Article);
        for fond in Fond::ALL.into_iter().filter(|fond| *fond != Fond::Code) {
            assert_eq!(choose_mode(2, fond, None), PaginationMode::Defaut, "{fond:?}");
        }
    }

    #[test]
    fn received_mode_reads_known_values_and_nothing_when_absent() {
        let (capture, _guard) = capture_at(Level::WARN);
        assert_eq!(received_mode(Some("ARTICLE")), Some(PaginationMode::Article));
        assert_eq!(received_mode(Some("DEFAUT")), Some(PaginationMode::Defaut));
        assert_eq!(received_mode(None), None);
        assert_eq!(capture.text(), "");
    }

    #[test]
    fn unknown_received_mode_is_ignored_with_a_warning_that_omits_the_value() {
        let (capture, _guard) = capture_at(Level::WARN);
        assert_eq!(received_mode(Some("SECRET_PAGINATION")), None);
        let log = capture.text();
        assert!(log.contains("WARN"), "{log}");
        assert!(log.contains("unknown typePagination"), "{log}");
        assert!(log.contains("len=17"), "{log}");
        assert!(!log.contains("SECRET"), "{log}");
    }

    /// Règle 3 de 04 §6.6 : une page 1 de CODE sans `typePagination` ne laisse aucun mémo, la
    /// page 2 repart donc en `ARTICLE` et non en `DEFAUT`.
    #[test]
    fn a_code_response_without_type_pagination_leaves_no_memo() {
        let (memo, _clock) = memo();
        let first = query(Fond::Code, "bail", 1);
        memo.remember(&first, received_mode(None));
        assert_eq!(memo.len(), 0);
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 2)), PaginationMode::Article);
    }

    #[test]
    fn an_unknown_type_pagination_leaves_no_memo() {
        let (capture, _guard) = capture_at(Level::WARN);
        let (memo, _clock) = memo();
        memo.remember(&query(Fond::Code, "bail", 1), received_mode(Some("INCONNU")));
        assert_eq!(memo.len(), 0);
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 2)), PaginationMode::Article);
        assert!(capture.text().contains("unknown typePagination"));
    }

    /// R13 : `typePagination` numérique est lu comme chaîne (`"3"`), puis refusé comme mode
    /// inconnu, sans mémo et sans citer la valeur.
    #[test]
    fn a_numeric_type_pagination_is_unknown_without_a_memo() {
        let (capture, _guard) = capture_at(Level::WARN);
        let (memo, _clock) = memo();
        let mode = received_mode(Some("3"));
        assert_eq!(mode, None);
        memo.remember(&query(Fond::Loda, "bail", 1), mode);
        assert_eq!(memo.len(), 0);
        let log = capture.text();
        assert!(log.contains("unknown typePagination"), "{log}");
        assert!(log.contains("len=1"), "{log}");
        assert!(!log.contains("\"3\""), "{log}");
    }

    #[test]
    fn a_known_type_pagination_is_memoized() {
        let (memo, _clock) = memo();
        memo.remember(&query(Fond::Loda, "bail", 1), received_mode(Some("ARTICLE")));
        assert_eq!(memo.len(), 1);
        assert_eq!(memo.mode_for(&query(Fond::Loda, "bail", 2)), PaginationMode::Article);
        memo.remember(&query(Fond::Code, "bail", 1), received_mode(Some("DEFAUT")));
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 2)), PaginationMode::Defaut);
    }

    #[test]
    fn a_response_without_type_pagination_keeps_the_last_value_received() {
        let (memo, _clock) = memo();
        let first = query(Fond::Loda, "bail", 1);
        memo.remember(&first, received_mode(Some("ARTICLE")));
        memo.remember(&first, received_mode(None));
        assert_eq!(memo.len(), 1);
        assert_eq!(memo.mode_for(&query(Fond::Loda, "bail", 2)), PaginationMode::Article);
    }

    /// Une entrée expirée est oubliée : `CODE` retombe sur la règle 3 (`ARTICLE`), les autres
    /// fonds sur `DEFAUT`, même si le mode mémorisé était l'inverse.
    #[test]
    fn mode_for_falls_back_to_the_default_rule_after_expiry() {
        for (fond, stored, fallback) in [
            (Fond::Code, PaginationMode::Defaut, PaginationMode::Article),
            (Fond::Loda, PaginationMode::Article, PaginationMode::Defaut),
        ] {
            let (memo, clock) = memo();
            memo.remember(&query(fond, "bail", 1), Some(stored));
            clock.advance(JUST_UNDER_AN_HOUR);
            assert_eq!(memo.mode_for(&query(fond, "bail", 2)), stored, "{fond:?}");
            clock.advance(Duration::from_nanos(1));
            assert_eq!(memo.mode_for(&query(fond, "bail", 2)), fallback, "{fond:?}");
            assert_eq!(memo.len(), 0, "{fond:?}");
        }
    }

    #[test]
    fn queries_that_differ_only_by_page_share_the_key() {
        let first = MemoKey::from_query(&query(Fond::Code, "contrat de travail", 1));
        let ninth = MemoKey::from_query(&query(Fond::Code, "contrat de travail", 9));
        assert!(first == ninth);
    }

    #[test]
    fn every_other_field_changes_the_key() {
        let build = |fond: Fond,
                     text: &str,
                     size: u32,
                     mode: MatchMode,
                     range: Option<DateRange>,
                     version: Option<VersionDate>| {
            MemoKey::from_query(
                &SearchQuery::new(
                    fond,
                    QueryText::parse(text).unwrap(),
                    NonZeroU32::new(1).unwrap(),
                    PageSize::new(size).unwrap(),
                    mode,
                    range,
                    version,
                )
                .unwrap(),
            )
        };
        let range = |from: &str, to: &str| DateRange::new(Some(day(from)), Some(day(to))).unwrap();
        let full = range("2016-01-01", "2016-12-31");
        let base = build(Fond::Loda, "grève", 10, MatchMode::Any, full, Some(day("2026-10-15")));
        assert!(base == base.clone());
        let variants = [
            build(Fond::Kali, "grève", 10, MatchMode::Any, full, None),
            build(Fond::Loda, "grèves", 10, MatchMode::Any, full, Some(day("2026-10-15"))),
            build(Fond::Loda, "grève", 11, MatchMode::Any, full, Some(day("2026-10-15"))),
            build(Fond::Loda, "grève", 10, MatchMode::All, full, Some(day("2026-10-15"))),
            build(Fond::Loda, "grève", 10, MatchMode::Any, None, Some(day("2026-10-15"))),
            build(
                Fond::Loda,
                "grève",
                10,
                MatchMode::Any,
                range("2016-01-01", "2016-12-30"),
                Some(day("2026-10-15")),
            ),
            build(Fond::Loda, "grève", 10, MatchMode::Any, full, Some(day("2026-10-16"))),
            build(Fond::Loda, "grève", 10, MatchMode::Any, full, None),
        ];
        for (index, variant) in variants.iter().enumerate() {
            assert!(base != *variant, "variant {index}");
        }
    }

    /// Deux requêtes qui ne diffèrent que par le fond : leurs clés diffèrent et un mode
    /// mémorisé pour l'une n'est pas retrouvé pour l'autre.
    #[test]
    fn the_fond_alone_changes_the_key() {
        let build = |fond: Fond, range: Option<DateRange>, version: Option<VersionDate>| {
            SearchQuery::new(
                fond,
                QueryText::parse("grève").unwrap(),
                NonZeroU32::new(1).unwrap(),
                PageSize::DEFAULT,
                MatchMode::Any,
                range,
                version,
            )
            .unwrap()
        };
        let range = DateRange::new(Some(day("2016-01-01")), Some(day("2016-12-31"))).unwrap();
        let pairs = [
            (build(Fond::Kali, range, None), build(Fond::Juri, range, None)),
            (
                build(Fond::Loda, None, Some(day("2026-10-15"))),
                build(Fond::Code, None, Some(day("2026-10-15"))),
            ),
        ];
        for (index, (first, second)) in pairs.iter().enumerate() {
            assert!(MemoKey::from_query(first) != MemoKey::from_query(second), "pair {index}");
            let (memo, _clock) = memo();
            memo.remember(first, Some(PaginationMode::Article));
            assert_eq!(
                memo.lookup(&MemoKey::from_query(first)),
                Some(PaginationMode::Article),
                "pair {index}"
            );
            assert_eq!(memo.lookup(&MemoKey::from_query(second)), None, "pair {index}");
        }
    }

    #[test]
    fn type_pagination_is_echoed_on_next_page() {
        let (memo, _clock) = memo();
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 1)), PaginationMode::Defaut);
        memo.remember(&query(Fond::Code, "bail", 1), Some(PaginationMode::Article));
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 2)), PaginationMode::Article);
        memo.remember(&query(Fond::Code, "bail", 1), Some(PaginationMode::Defaut));
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 2)), PaginationMode::Defaut);
        memo.remember(&query(Fond::Code, "bail", 1), Some(PaginationMode::Article));
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 1)), PaginationMode::Defaut);
    }

    #[test]
    fn code_page_two_without_memo_sends_article() {
        let (memo, _clock) = memo();
        assert_eq!(memo.mode_for(&query(Fond::Code, "bail", 2)), PaginationMode::Article);
    }

    #[test]
    fn loda_page_two_without_memo_sends_defaut() {
        let (memo, _clock) = memo();
        assert_eq!(memo.mode_for(&query(Fond::Loda, "bail", 2)), PaginationMode::Defaut);
    }

    #[test]
    fn memo_is_scoped_by_query() {
        let (memo, _clock) = memo();
        memo.remember(&query(Fond::Loda, "alpha", 1), Some(PaginationMode::Article));
        assert_eq!(memo.mode_for(&query(Fond::Loda, "beta", 2)), PaginationMode::Defaut);
        assert_eq!(memo.mode_for(&query(Fond::Loda, "alpha", 2)), PaginationMode::Article);
    }

    #[test]
    fn entry_expires_exactly_at_one_hour() {
        assert_eq!(TTL, ONE_HOUR);
        let (memo, clock) = memo();
        clock.advance(Duration::from_secs(10));
        memo.record(numbered(0), PaginationMode::Article);
        clock.advance(JUST_UNDER_AN_HOUR);
        assert_eq!(memo.lookup(&numbered(0)), Some(PaginationMode::Article));
        assert_eq!(memo.len(), 1);
        clock.advance(Duration::from_nanos(1));
        assert_eq!(memo.lookup(&numbered(0)), None);
        assert_eq!(memo.len(), 0);
    }

    #[test]
    fn rewriting_an_entry_restarts_its_ttl() {
        let (memo, clock) = memo();
        memo.record(numbered(0), PaginationMode::Article);
        clock.advance(Duration::from_secs(3_599));
        memo.record(numbered(0), PaginationMode::Article);
        clock.advance(Duration::from_secs(30));
        assert_eq!(memo.lookup(&numbered(0)), Some(PaginationMode::Article));
        assert_eq!(memo.len(), 1);
    }

    #[test]
    fn capacity_is_reached_without_eviction() {
        let (memo, _clock) = memo();
        for i in 0..CAPACITY {
            memo.record(numbered(i), PaginationMode::Article);
        }
        assert_eq!(memo.len(), 1_024);
        assert_eq!(memo.lookup(&numbered(0)), Some(PaginationMode::Article));
    }

    #[test]
    fn the_1025th_entry_evicts_the_oldest_one() {
        let (memo, _clock) = memo();
        for i in 0..=CAPACITY {
            memo.record(numbered(i), PaginationMode::Article);
        }
        assert_eq!(memo.len(), 1_024);
        assert_eq!(memo.lookup(&numbered(0)), None);
        assert_eq!(memo.lookup(&numbered(1)), Some(PaginationMode::Article));
        assert_eq!(memo.lookup(&numbered(CAPACITY)), Some(PaginationMode::Article));
    }

    #[test]
    fn rewriting_a_key_makes_it_the_newest_and_never_evicts() {
        let (memo, _clock) = memo();
        for i in 0..CAPACITY {
            memo.record(numbered(i), PaginationMode::Defaut);
        }
        memo.record(numbered(0), PaginationMode::Article);
        assert_eq!(memo.len(), 1_024);
        memo.record(numbered(CAPACITY), PaginationMode::Defaut);
        assert_eq!(memo.len(), 1_024);
        assert_eq!(memo.lookup(&numbered(1)), None);
        assert_eq!(memo.lookup(&numbered(2)), Some(PaginationMode::Defaut));
        assert_eq!(memo.lookup(&numbered(0)), Some(PaginationMode::Article));
    }

    #[test]
    fn evicting_from_an_empty_memo_does_nothing() {
        let (memo, _clock) = memo();
        memo.lock().evict_oldest();
        assert_eq!(memo.len(), 0);
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let (memo, _clock) = memo();
        memo.record(numbered(0), PaginationMode::Article);
        let poisoned = catch_unwind(AssertUnwindSafe(|| {
            let _guard = memo.state.lock().unwrap();
            resume_unwind(Box::new("poison"));
        }));
        assert!(poisoned.is_err());
        assert!(memo.state.is_poisoned());
        assert_eq!(memo.lookup(&numbered(0)), Some(PaginationMode::Article));
        memo.record(numbered(1), PaginationMode::Defaut);
        assert_eq!(memo.len(), 2);
    }
}
