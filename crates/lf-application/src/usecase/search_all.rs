//! Cas d'usage de `fr_search_all` (v0.3, M7) : le premier repérage multi-fonds
//! (`SearchAllUseCase`, 05 §5.13).
//!
//! # Étapes
//!
//! 1. Validation sans réseau, dans l'ordre du tableau d'entrée : `query` (comme `fr_search`),
//!    `fonds` (1 à 8 fonds uniques, insensibles à la casse et aux espaces ; cinq par défaut), puis
//!    `per_fond` (1 à 10, 3 par défaut).
//! 2. Une recherche par fond, **page 1, `match` = `any`**, sans filtre de date et avec
//!    `version_date` au jour de Paris pour LODA et CODE : c'est `SearchUseCase`, donc la même
//!    mise en forme et la même citation que `fr_search`. Quatre recherches au plus sont en vol à
//!    la fois (`CONCURRENCY`, 05 §5.13 : le limiteur régule le reste). Les groupes sont rendus
//!    dans l'ordre de `fonds`, quel que soit l'ordre d'arrivée.
//! 3. Un fond qui échoue donne une entrée d'erreur sans faire échouer les autres. Si **tous**
//!    échouent avec le même code (cas typique : `config_error`), l'appel échoue avec ce code.
//!
//! # Plafond de réponse (05 §6.4)
//!
//! La taille de toute la réponse est la longueur exacte de son JSON compact, comme pour `fr_search`
//! (clés, guillemets, séparateurs et échappements : `usecase::search::hit_cost`, plus l'enveloppe
//! et les groupes de ce module). Elle est dégradée seulement si cette longueur dépasse la cible de
//! 42 000 moins la réserve des notes, en partant du dernier résultat du dernier fond, sans jamais
//! retirer un résultat : (1) extraits ; (2) listes d'articles et champs optionnels. Une `note` le
//! dit. La vue de `lf-adapter-mcp` mesure encore le JSON (plafond de 45 000) : un filet, qui ne
//! se déclenche plus que pour des cas pathologiques.
//!
//! # Progression
//!
//! `progress(faits, total)` est appelé à la fin de chaque fond, dans l'ordre d'arrivée. Le cas
//! d'usage ne sait rien du transport : la façade et l'adaptateur décident de ce qu'ils en font.

use futures::{StreamExt, stream};
use lf_domain::{
    fond::Fond,
    search::{QueryText, check_quotes},
};

use crate::{
    budget::{ResponseBudget, bounded, strip_from_end, the_last_hits},
    error::AppError,
    input::{SearchAllInput, SearchInput},
    messages,
    output::{
        search::HitOut,
        search_all::{FondHits, GroupOut, SearchAllOutput},
    },
    ports::{Clock, LegalCorpus},
    tools::ToolName,
    usecase::search::{
        DEGRADE_NOTE_RESERVE, JsonObject, SearchUseCase, digits, hit_cost, strip_details,
        strip_snippets,
    },
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_SEARCH_ALL;

/// Fonds interrogés quand `fonds` est absent (05 §5.13).
const DEFAULT_FONDS: [Fond; 5] = [Fond::Loda, Fond::Code, Fond::Juri, Fond::Cetat, Fond::Constit];

/// Résultats par fond quand `per_fond` est absent, et ses bornes (05 §5.13).
const PER_FOND_DEFAULT: usize = 3;
const PER_FOND_MIN: usize = 1;
const PER_FOND_MAX: usize = 10;

/// Recherches simultanément en vol.
const CONCURRENCY: usize = 4;

/// Note quand tous les fonds ont répondu sans résultat (D-10, 05 §5.13) : l'appel réussit, mais
/// rien ne rappelait que le vide ne prouve pas le silence de la loi. Pas de filtre de date ici :
/// la note ne cite que ce que `fr_search_all` offre.
const NO_MATCH_NOTE: &str = "No match in any fond: try other words or other fonds, or call \
                             fr_coverage; an empty result does not mean the law is silent.";

/// L'enveloppe sans groupe ni note : `{"groups":[]}`.
const ENVELOPE_CHARS: usize = r#"{"groups":[]}"#.len();

/// Recherche multi-fonds : analyse des arguments, une recherche par fond, politique de réponse.
pub(crate) struct SearchAllUseCase<'a, C: LegalCorpus, K: Clock> {
    corpus: &'a C,
    clock: &'a K,
}

impl<'a, C: LegalCorpus, K: Clock> SearchAllUseCase<'a, C, K> {
    pub(crate) fn new(corpus: &'a C, clock: &'a K) -> Self {
        Self { corpus, clock }
    }

    /// Valide `input`, interroge les fonds et met les groupes en forme. `progress(faits, total)`
    /// est appelé après chaque fond terminé.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour tout argument refusé (sans appel au port) ; l'erreur commune quand tous
    /// les fonds échouent avec le même code.
    pub(crate) async fn execute(
        &self,
        input: SearchAllInput,
        mut progress: impl FnMut(u32, u32) + Send,
    ) -> Result<SearchAllOutput, AppError> {
        let SearchAllInput { query, fonds, per_fond } = input;
        let query = query.ok_or_else(|| messages::required("query"))?;
        let text = QueryText::parse(&query).map_err(|error| messages::domain(TOOL, &error))?;
        check_quotes(&text).map_err(|error| messages::domain(TOOL, &error))?;
        let fonds = fonds_of(fonds)?;
        let per_fond = bounded("per_fond", per_fond, PER_FOND_DEFAULT, PER_FOND_MIN, PER_FOND_MAX)?;

        let total = u32::try_from(fonds.len()).unwrap_or(u32::MAX);
        let query = &query;
        let mut arrivals = stream::iter(fonds.into_iter().enumerate())
            .map(|(index, fond)| async move {
                (index, GroupOut { fond, result: self.search_fond(fond, query, per_fond).await })
            })
            .buffer_unordered(CONCURRENCY);
        let mut groups = Vec::new();
        while let Some(group) = arrivals.next().await {
            groups.push(group);
            progress(u32::try_from(groups.len()).unwrap_or(u32::MAX), total);
        }
        groups.sort_by_key(|(index, _)| *index);
        let mut groups: Vec<GroupOut> = groups.into_iter().map(|(_, group)| group).collect();

        if let Some(error) = shared_failure(&groups) {
            return Err(error);
        }
        let note = fit_under_target(&mut groups).or_else(|| all_empty(&groups));
        Ok(SearchAllOutput { groups, note })
    }

    /// La première page de `fond`, comme `fr_search` la rendrait.
    async fn search_fond(
        &self,
        fond: Fond,
        query: &str,
        per_fond: usize,
    ) -> Result<FondHits, AppError> {
        let input = SearchInput {
            query: Some(query.to_owned()),
            fond: Some(fond.name().to_owned()),
            page_size: Some(i64::try_from(per_fond).unwrap_or(i64::MAX)),
            ..SearchInput::default()
        };
        let page = SearchUseCase::new(self.corpus, self.clock).execute(input).await?;
        Ok(FondHits { total: page.total, has_more: page.has_more, hits: page.hits })
    }
}

/// Fonds demandés : cinq par défaut ; sinon au moins un, chacun connu et au plus une fois.
fn fonds_of(value: Option<Vec<String>>) -> Result<Vec<Fond>, AppError> {
    let Some(names) = value else { return Ok(DEFAULT_FONDS.to_vec()) };
    if names.is_empty() {
        return Err(messages::invalid_argument(
            "fonds is empty; list 1 to 8 fonds or omit it for the default fonds.",
        ));
    }
    let mut fonds: Vec<Fond> = Vec::with_capacity(names.len());
    for name in &names {
        let fond = Fond::parse(name).map_err(|error| messages::domain(TOOL, &error))?;
        if fonds.contains(&fond) {
            return Err(messages::invalid_argument(format!(
                "fonds lists {} more than once; each fond must appear once.",
                fond.name()
            )));
        }
        fonds.push(fond);
    }
    Ok(fonds)
}

/// La note « aucun résultat » quand chaque fond a répondu, sans total ni résultat. Un fond en
/// erreur n'est pas un fond vide : sa seule entrée le dit. La dégradation ne retire que des
/// extraits et des champs de résultats : elle ne coïncide jamais avec cette note.
fn all_empty(groups: &[GroupOut]) -> Option<String> {
    groups
        .iter()
        .all(|group| {
            group.result.as_ref().is_ok_and(|found| found.total == 0 && found.hits.is_empty())
        })
        .then(|| NO_MATCH_NOTE.to_owned())
}

/// L'erreur commune quand tous les fonds ont échoué avec le même code ; `None` dès qu'un fond a
/// répondu ou que les codes diffèrent. C'est l'erreur du premier fond qui est rendue.
fn shared_failure(groups: &[GroupOut]) -> Option<AppError> {
    let mut errors = groups.iter().map(|group| group.result.as_ref().err());
    let first = errors.next().flatten()?;
    errors
        .all(|error| error.is_some_and(|error| error.code() == first.code()))
        .then(|| first.clone())
}

// -------------------------------------------------------------------------------------------
// Plafond de réponse
// -------------------------------------------------------------------------------------------

/// Dégrade les résultats, du dernier au premier, jusqu'à ce que la longueur exacte de la réponse
/// passe sous la cible, réserve des notes comprise (05 §6.4). Rend la `note` qui le dit, s'il y a
/// eu dégradation.
fn fit_under_target(groups: &mut [GroupOut]) -> Option<String> {
    let fixed = envelope_cost(groups).saturating_add(DEGRADE_NOTE_RESERVE);
    let mut hits: Vec<&mut HitOut> = groups
        .iter_mut()
        .filter_map(|group| group.result.as_mut().ok())
        .flat_map(|found| found.hits.iter_mut())
        .collect();
    let over = |all: &[&mut HitOut]| {
        let cost = all.iter().map(|out| hit_cost(out)).fold(0, usize::saturating_add);
        fixed.saturating_add(cost) > ResponseBudget::ESTIMATE_TARGET_CHARS
    };
    let snippets = strip_from_end(&mut hits, over, |out| strip_snippets(out));
    let details = strip_from_end(&mut hits, over, |out| strip_details(out, false));
    let notes = [
        (snippets > 0).then(|| snippets_note(snippets)),
        (details > 0).then(|| details_note(details)),
    ];
    let notes: Vec<String> = notes.into_iter().flatten().collect();
    (!notes.is_empty()).then(|| notes.join(" "))
}

/// Longueur du JSON compact de l'enveloppe et de ses groupes, hors objets des résultats (ceux de
/// [`hit_cost`]) : `{"groups":[`, les groupes séparés par des virgules, `]}`. Le champ `note`
/// n'y est pas : [`DEGRADE_NOTE_RESERVE`] le réserve.
fn envelope_cost(groups: &[GroupOut]) -> usize {
    groups
        .iter()
        .map(group_cost)
        .fold(ENVELOPE_CHARS, usize::saturating_add)
        .saturating_add(groups.len().saturating_sub(1))
}

/// Longueur du JSON compact d'un groupe, hors objets des résultats : `fond`, puis `total`,
/// `has_more` et `hits` (`[`, les virgules entre résultats, `]`), ou `error`.
fn group_cost(group: &GroupOut) -> usize {
    let mut json = JsonObject::default();
    json.raw("fond", group.fond.name().len().saturating_add(2));
    match &group.result {
        Ok(found) => {
            json.raw("total", digits(found.total));
            json.raw("has_more", if found.has_more { 4 } else { 5 });
            json.raw("hits", found.hits.len().saturating_sub(1).saturating_add(2));
        }
        Err(error) => json.text("error", &error_text(error)),
    }
    json.total()
}

/// `[code] message`, comme la vue l'écrit ; un appel annulé n'a pas de code : le message seul.
fn error_text(error: &AppError) -> String {
    match error.code() {
        Some(code) => format!("[{}] {}", code.as_str(), error.message()),
        None => error.message().to_owned(),
    }
}

/// Note des extraits retirés (05 §6.4).
fn snippets_note(hits: usize) -> String {
    format!(
        "Snippets were removed from {} to keep the response under 45,000 characters; use \
         fr_search on one fond to get them.",
        the_last_hits(hits)
    )
}

/// Note des listes d'articles et champs optionnels retirés.
fn details_note(hits: usize) -> String {
    format!(
        "Article lists and the optional fields title, nature, etat and date were removed from \
         {} to keep the response under 45,000 characters; use fr_search on one fond to get \
         them.",
        the_last_hits(hits)
    )
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, time::Duration};

    use chrono::DateTime;
    use lf_domain::{
        contract::{Citable, Idcc},
        date::{CivilDate, VersionDate},
        entity::{CodeArticleRef, SearchHit, SearchPage, Snippet},
        fond::PaginationMode,
        id::{ArticleId, DocumentId, LegiArtiId},
        search::MatchMode,
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        error::{ErrorCode, UpstreamCause},
        ports::CorpusError,
        testing::{FakeCorpus, FixedClock, PortRequest},
    };

    fn clock() -> FixedClock {
        FixedClock::new(DateTime::UNIX_EPOCH, CivilDate::new(2026, 10, 29).unwrap())
    }

    /// Résultat sans extrait ni titre d'un fond.
    fn bare(fond: Fond, n: u32) -> SearchHit {
        SearchHit::new(
            fond,
            DocumentId::parse(&format!("{}{n:012}", fond.hit_prefix())).unwrap(),
            None,
        )
        .unwrap()
    }

    fn page_of(fond: Fond, total: u64, hits: Vec<SearchHit>) -> SearchPage {
        SearchPage::new(fond, "bail", Some(total), None, hits, 0, PaginationMode::Defaut)
    }

    /// Une page de `n` résultats nus.
    fn bare_page(fond: Fond, total: u64, n: u32) -> SearchPage {
        page_of(fond, total, (1..=n).map(|index| bare(fond, index)).collect())
    }

    fn all_of(
        query: Option<&str>,
        fonds: Option<&[&str]>,
        per_fond: Option<i64>,
    ) -> SearchAllInput {
        SearchAllInput {
            query: query.map(str::to_owned),
            fonds: fonds.map(|names| names.iter().map(|name| (*name).to_owned()).collect()),
            per_fond,
        }
    }

    fn named(fonds: &[&str]) -> SearchAllInput {
        all_of(Some("bail"), Some(fonds), None)
    }

    async fn run(corpus: &FakeCorpus, input: SearchAllInput) -> Result<SearchAllOutput, AppError> {
        SearchAllUseCase::new(corpus, &clock()).execute(input, |_, _| {}).await
    }

    /// Message d'un `invalid_arg`, après avoir vérifié qu'aucune requête n'est partie.
    async fn rejected(input: SearchAllInput) -> String {
        let corpus = FakeCorpus::new();
        let error = run(&corpus, input).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(corpus.calls(), 0, "un rejet local n'appelle pas le port");
        error.message().to_owned()
    }

    /// Les requêtes de recherche que le port a reçues.
    fn sent(corpus: &FakeCorpus) -> Vec<lf_domain::search::SearchQuery> {
        corpus
            .seen_requests()
            .into_iter()
            .filter_map(|request| match request {
                PortRequest::Search(query) => Some(query),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn sent_keeps_only_the_searches() {
        let corpus = FakeCorpus::new();
        let article = ArticleId::Legi(LegiArtiId::parse("LEGIARTI000000000001").unwrap());
        let _ = corpus.get_article(&article).await;
        assert_eq!(corpus.calls(), 1);
        assert_eq!(sent(&corpus), []);
    }

    fn fond_names(output: &SearchAllOutput) -> Vec<&'static str> {
        output.groups.iter().map(|group| group.fond.name()).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn search_all_respects_concurrency_cap() {
        let corpus = FakeCorpus::new().with_delay(Duration::from_millis(100));
        for fond in Fond::ALL {
            corpus.push_search(Ok(bare_page(fond, 10, 1)));
        }
        let names: Vec<&str> = Fond::ALL.iter().map(|fond| fond.name()).collect();

        let output = run(&corpus, named(&names)).await.unwrap();

        assert_eq!(corpus.calls(), 8, "un appel par fond");
        assert_eq!(corpus.max_concurrency(), CONCURRENCY, "jamais plus de quatre en vol");
        assert_eq!(CONCURRENCY, 4);
        // Les groupes suivent l'ordre demandé, quel que soit l'ordre d'arrivée.
        assert_eq!(fond_names(&output), names);
    }

    #[tokio::test(start_paused = true)]
    async fn groups_follow_the_requested_order_even_when_the_slow_fonds_finish_last() {
        // Les réponses suivent l'ordre des appels : la page de chaque fond porte son nom.
        let corpus = FakeCorpus::new().with_delay(Duration::from_millis(50));
        corpus.push_search(Ok(bare_page(Fond::Kali, 1, 1)));
        corpus.push_search(Ok(bare_page(Fond::Cnil, 2, 1)));
        let output = run(&corpus, named(&["KALI", "CNIL"])).await.unwrap();
        let totals: Vec<u64> = output
            .groups
            .iter()
            .map(|group| group.result.as_ref().map(|found| found.total).unwrap())
            .collect();
        assert_eq!((fond_names(&output), totals), (vec!["KALI", "CNIL"], vec![1, 2]));
    }

    /// D-10 : tous les fonds répondent sans résultat (cas réel `search_all-9`, requête
    /// « zzqxwv pfrtgh » : cinq groupes à `total` 0, sans note) : une note le dit.
    #[tokio::test]
    async fn a_note_says_so_when_every_fond_is_empty() {
        let corpus = FakeCorpus::new();
        for fond in DEFAULT_FONDS {
            corpus.push_search(Ok(page_of(fond, 0, vec![])));
        }
        let output = run(&corpus, all_of(Some("zzqxwv pfrtgh"), None, None)).await.unwrap();
        assert_eq!(output.groups.len(), 5);
        assert_eq!(output.note.as_deref(), Some(NO_MATCH_NOTE));
        assert!(!NO_MATCH_NOTE.contains("date"), "fr_search_all n'a pas de filtre de date");
    }

    /// D-10, faux positif évité : un seul groupe avec un résultat, ou avec un total annoncé, et
    /// il n'y a pas de note.
    #[tokio::test]
    async fn no_note_when_one_fond_has_results() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page_of(Fond::Loda, 0, vec![])));
        corpus.push_search(Ok(bare_page(Fond::Code, 4, 1)));
        let output = run(&corpus, named(&["LODA", "CODE"])).await.unwrap();
        assert_eq!(output.note, None);

        // Un total positif sans résultat listé n'est pas un fond vide.
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page_of(Fond::Loda, 0, vec![])));
        corpus.push_search(Ok(page_of(Fond::Code, 7, vec![])));
        let output = run(&corpus, named(&["LODA", "CODE"])).await.unwrap();
        assert_eq!(output.note, None);
    }

    /// D-10, faux positif évité : un fond en erreur n'est pas un fond vide, donc pas de note
    /// « No match » (l'entrée d'erreur dit déjà quoi faire). Vrai positif conservé : un seul fond
    /// demandé et vide a bien sa note.
    #[tokio::test]
    async fn a_failed_fond_is_not_an_empty_one() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page_of(Fond::Loda, 0, vec![])));
        corpus.push_search(Err(CorpusError::Timeout));
        let output = run(&corpus, named(&["LODA", "JURI"])).await.unwrap();
        assert_eq!(output.note, None);

        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page_of(Fond::Juri, 0, vec![])));
        let output = run(&corpus, named(&["JURI"])).await.unwrap();
        assert_eq!(output.note.as_deref(), Some(NO_MATCH_NOTE));
    }

    #[tokio::test]
    async fn search_all_error_per_group() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(bare_page(Fond::Loda, 40, 2)));
        corpus.push_search(Err(CorpusError::Timeout));
        corpus.push_search(Ok(bare_page(Fond::Code, 1, 1)));

        let output = run(&corpus, named(&["LODA", "JURI", "CODE"])).await.unwrap();

        assert_eq!(fond_names(&output), ["LODA", "JURI", "CODE"]);
        let loda = output.groups[0].result.as_ref().unwrap();
        assert_eq!((loda.total, loda.has_more, loda.hits.len()), (40, true, 2));
        let juri = output.groups[1].result.as_ref().unwrap_err();
        assert_eq!(juri.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(juri.cause(), Some(UpstreamCause::Timeout));
        let code = output.groups[2].result.as_ref().unwrap();
        assert_eq!((code.total, code.has_more, code.hits.len()), (1, false, 1));
        assert_eq!(output.note, None);
    }

    #[tokio::test]
    async fn when_every_fond_fails_with_the_same_code_the_call_fails_with_it() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Err(CorpusError::Timeout));
        corpus.push_search(Err(CorpusError::Timeout));
        let error = run(&corpus, named(&["LODA", "JURI"])).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(error.cause(), Some(UpstreamCause::Timeout));

        // Un seul fond, qui échoue : c'est aussi « tous ».
        let corpus = FakeCorpus::new();
        corpus.push_search(Err(CorpusError::RateLimited { retry_after: None }));
        let error = run(&corpus, named(&["CNIL"])).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::RateLimited));
    }

    #[tokio::test]
    async fn when_the_failures_differ_or_one_fond_answers_the_groups_carry_the_errors() {
        // Deux codes différents : pas d'erreur commune.
        let corpus = FakeCorpus::new();
        corpus.push_search(Err(CorpusError::Timeout));
        corpus.push_search(Err(CorpusError::RateLimited { retry_after: None }));
        let output = run(&corpus, named(&["LODA", "JURI"])).await.unwrap();
        let codes: Vec<Option<ErrorCode>> = output
            .groups
            .iter()
            .map(|group| group.result.as_ref().err().and_then(AppError::code))
            .collect();
        assert_eq!(codes, [Some(ErrorCode::UpstreamError), Some(ErrorCode::RateLimited)]);

        // Un fond répond, l'autre non, dans l'autre ordre : le premier échec ne suffit pas.
        let corpus = FakeCorpus::new();
        corpus.push_search(Err(CorpusError::Timeout));
        corpus.push_search(Ok(bare_page(Fond::Juri, 1, 1)));
        let output = run(&corpus, named(&["LODA", "JURI"])).await.unwrap();
        assert!(output.groups[0].result.is_err() && output.groups[1].result.is_ok());
    }

    #[tokio::test]
    async fn the_default_request_is_page_one_any_three_per_fond_on_five_fonds() {
        let corpus = FakeCorpus::new();
        for fond in DEFAULT_FONDS {
            corpus.push_search(Ok(bare_page(fond, 1, 1)));
        }

        let output = run(&corpus, all_of(Some("  bail commercial "), None, None)).await.unwrap();

        assert_eq!(fond_names(&output), ["LODA", "CODE", "JURI", "CETAT", "CONSTIT"]);
        let today = VersionDate::parse("2026-10-29").unwrap();
        for (query, fond) in sent(&corpus).iter().zip(DEFAULT_FONDS) {
            assert_eq!(query.fond(), fond);
            assert_eq!(query.page().get(), 1);
            assert_eq!(query.page_size().get(), 3);
            assert_eq!(query.mode(), MatchMode::Any);
            assert_eq!(query.date_range(), None, "pas de filtre de période");
            // Le jour de Paris pour LODA et CODE seulement.
            assert_eq!(query.version_date(), fond.needs_version_date().then_some(today));
            assert_eq!(query.text().as_str(), "bail commercial");
        }
    }

    #[tokio::test]
    async fn fonds_are_case_insensitive_and_per_fond_is_forwarded_as_the_page_size() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(bare_page(Fond::Kali, 1, 1)));
        corpus.push_search(Ok(bare_page(Fond::Cnil, 1, 1)));

        let output =
            run(&corpus, all_of(Some("bail"), Some(&[" kali ", "Cnil"]), Some(10))).await.unwrap();

        assert_eq!(fond_names(&output), ["KALI", "CNIL"]);
        let seen = sent(&corpus);
        assert_eq!(seen.iter().map(|query| query.page_size().get()).collect::<Vec<_>>(), [10, 10]);
        assert_eq!(seen[0].version_date(), None, "KALI ne se consulte pas à une date");
    }

    #[tokio::test]
    async fn validation_follows_the_input_table_and_never_calls_the_port() {
        let mut input = all_of(None, Some(&[]), Some(0));
        assert_eq!(rejected(input.clone()).await, "query is required.");
        input.query = Some("   ".to_owned());
        assert_eq!(rejected(input.clone()).await, "query is required.");
        input.query = Some("bail".to_owned());
        assert_eq!(
            rejected(input.clone()).await,
            "fonds is empty; list 1 to 8 fonds or omit it for the default fonds."
        );
        input.fonds = Some(vec!["LODA".to_owned(), "XYZ".to_owned()]);
        assert_eq!(
            rejected(input.clone()).await,
            "fond='XYZ' must be one of ['ACCO', 'CETAT', 'CNIL', 'CODE', 'CONSTIT', 'JURI', \
             'KALI', 'LODA']."
        );
        input.fonds = Some(vec!["LODA".to_owned(), "code".to_owned(), " loda".to_owned()]);
        assert_eq!(
            rejected(input.clone()).await,
            "fonds lists LODA more than once; each fond must appear once."
        );
        input.fonds = None;
        assert_eq!(rejected(input.clone()).await, "per_fond=0 out of range (1..10).");
        for bad in [-1, 11, i64::MAX] {
            input.per_fond = Some(bad);
            assert_eq!(
                rejected(input.clone()).await,
                format!("per_fond={bad} out of range (1..10).")
            );
        }
        // Tout est corrigé : les cinq fonds par défaut sont interrogés.
        input.per_fond = Some(1);
        let corpus = FakeCorpus::new();
        for fond in DEFAULT_FONDS {
            corpus.push_search(Ok(bare_page(fond, 0, 0)));
        }
        run(&corpus, input).await.unwrap();
        assert_eq!(corpus.calls(), 5);
    }

    #[tokio::test]
    async fn a_too_long_query_is_rejected_once_before_any_fond_is_searched() {
        let message = rejected(all_of(Some(&"a".repeat(612)), None, None)).await;
        assert!(message.starts_with("query is too long (612 characters, max 450)"), "{message}");
    }

    #[tokio::test]
    async fn an_unbalanced_double_quote_is_one_invalid_arg_and_no_fond_is_searched() {
        // E2E search_all-1 : six fonds échouaient tous en 503 ; une seule erreur locale désormais.
        for odd in ["\"licenciement", "licenciement\""] {
            let corpus = FakeCorpus::new();
            let error = run(&corpus, all_of(Some(odd), Some(&["LODA", "JURI", "CODE"]), Some(1)))
                .await
                .unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{odd:?}");
            assert_eq!(
                error.message(),
                "query has an unbalanced double quote (\"): close it or remove it.",
                "{odd:?}"
            );
            assert_eq!(corpus.calls(), 0, "{odd:?}");
        }
    }

    #[tokio::test]
    async fn balanced_quotes_still_search_every_fond() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(bare_page(Fond::Loda, 0, 0)));
        corpus.push_search(Ok(bare_page(Fond::Code, 0, 0)));
        run(&corpus, all_of(Some("\"licenciement économique\""), Some(&["LODA", "CODE"]), Some(1)))
            .await
            .unwrap();
        assert_eq!(corpus.calls(), 2);
    }

    #[tokio::test]
    async fn progress_counts_each_finished_fond() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(bare_page(Fond::Loda, 1, 1)));
        corpus.push_search(Err(CorpusError::Timeout));
        corpus.push_search(Ok(bare_page(Fond::Code, 1, 1)));
        let mut seen = Vec::new();

        SearchAllUseCase::new(&corpus, &clock())
            .execute(named(&["LODA", "JURI", "CODE"]), |done, total| seen.push((done, total)))
            .await
            .unwrap();

        assert_eq!(seen, [(1, 3), (2, 3), (3, 3)]);
    }

    // ---------------------------------------------------------------------------------------
    // Plafond de réponse
    // ---------------------------------------------------------------------------------------

    /// Résultat chargé : titre, nature, date et extrait ; sur CODE, un article ; sur KALI, un IDCC.
    fn loaded(fond: Fond, n: u32) -> SearchHit {
        let article = LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap();
        SearchHit {
            title: Some(format!("{} {n} ", fond.name()) + &"titre ".repeat(15)),
            nature: Some("Texte".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            date: CivilDate::new(2016, 10, 7),
            snippet: Snippet::new(&"extrait ".repeat(40)),
            idcc: (fond == Fond::Kali).then(|| Idcc::parse("1486").unwrap()),
            articles: if fond == Fond::Code {
                vec![CodeArticleRef::new(
                    ArticleId::Legi(article),
                    Some("L. 145-1".to_owned()),
                    &"article ".repeat(20),
                )]
            } else {
                vec![]
            },
            ..bare(fond, n)
        }
    }

    fn loaded_page(fond: Fond, n: u32) -> SearchPage {
        page_of(fond, 500, (1..=n).map(|index| loaded(fond, index)).collect())
    }

    /// Estimation de toute la réponse, comme `fit_under_target` la calcule.
    fn estimate(output: &SearchAllOutput) -> usize {
        let mut groups = output.groups.clone();
        let fixed = envelope_cost(&groups) + DEGRADE_NOTE_RESERVE;
        let hits: usize = groups
            .iter_mut()
            .filter_map(|group| group.result.as_mut().ok())
            .flat_map(|found| found.hits.iter())
            .map(hit_cost)
            .sum();
        fixed + hits
    }

    fn all_hits(output: &SearchAllOutput) -> Vec<&HitOut> {
        output
            .groups
            .iter()
            .filter_map(|group| group.result.as_ref().ok())
            .flat_map(|found| found.hits.iter())
            .collect()
    }

    #[tokio::test]
    async fn search_all_trimmed_under_estimate() {
        // Huit fonds de dix résultats chargés : au-delà de 42 000 caractères de JSON exact, et les
        // extraits suffisent à repasser dessous (JURI en dernier : son citation porte le titre).
        let fonds = [
            Fond::Loda,
            Fond::Kali,
            Fond::Cetat,
            Fond::Constit,
            Fond::Cnil,
            Fond::Acco,
            Fond::Code,
            Fond::Juri,
        ];
        let corpus = FakeCorpus::new();
        let mut untrimmed = Vec::new();
        for fond in fonds {
            let page = loaded_page(fond, 10);
            untrimmed.push(group_of(fond, page.hits.iter().map(hit_out_of).collect()));
            corpus.push_search(Ok(page));
        }
        let untrimmed = exact_len(&untrimmed, None);
        assert!(untrimmed > ResponseBudget::ESTIMATE_TARGET_CHARS, "{untrimmed}");
        let names: Vec<&str> = fonds.iter().map(|fond| fond.name()).collect();

        let output = run(&corpus, all_of(Some("bail"), Some(&names), Some(10))).await.unwrap();

        // Aucun résultat n'est retiré ; l'estimation passe sous la cible.
        let hits = all_hits(&output);
        assert_eq!(hits.len(), 80);
        assert!(estimate(&output) <= ResponseBudget::ESTIMATE_TARGET_CHARS);
        // Les extraits partent d'abord, en partant de la fin : le premier résultat garde le sien.
        assert!(hits[0].hit.snippet.is_some());
        assert!(hits[79].hit.snippet.is_none());
        assert!(hits[79].hit.title.is_some(), "les titres ne partent qu'ensuite");
        let note = output.note.as_deref().expect("the response was degraded");
        assert!(note.starts_with("Snippets were removed from the last "), "{note}");
        assert!(note.contains("use fr_search on one fond"), "{note}");
        assert!(!note.contains("Article lists"), "{note}");
        // Les extraits d'articles partent avec ceux des résultats.
        let code_hit = &output.groups[6].result.as_ref().unwrap().hits;
        for out in code_hit {
            assert_eq!(out.hit.snippet.is_none(), out.hit.articles[0].snippet.is_none());
        }
        assert!(code_hit[9].hit.articles[0].snippet.is_none());
        assert!(code_hit[9].hit.articles[0].num.is_some(), "l'article reste, sans son extrait");
        // Les citations, calculées avant la dégradation, restent entières.
        assert!(hits[79].citation.starts_with("JURI 10 titre"));
    }

    #[test]
    fn notes_use_the_singular_for_one_hit() {
        assert_eq!(
            snippets_note(1),
            "Snippets were removed from the last hit to keep the response under 45,000 \
             characters; use fr_search on one fond to get them."
        );
        assert_eq!(
            details_note(1),
            "Article lists and the optional fields title, nature, etat and date were removed from \
             the last hit to keep the response under 45,000 characters; use fr_search on one fond \
             to get them."
        );
        assert!(snippets_note(5).contains("from the last 5 hits to"));
        assert!(details_note(5).contains("from the last 5 hits to"));
    }

    /// Le résultat et sa citation, comme `fr_search` les rend.
    fn hit_out_of(hit: &SearchHit) -> HitOut {
        let contract = hit.contract();
        HitOut {
            citation: contract.citation.as_str().to_owned(),
            source_url: contract.source_url.as_str().to_owned(),
            hit: hit.clone(),
        }
    }

    #[tokio::test]
    async fn titles_and_article_lists_go_too_when_the_snippets_are_not_enough() {
        // Beaucoup de gros titres : retirer les extraits ne suffit pas.
        let corpus = FakeCorpus::new();
        for fond in Fond::ALL {
            let hits = (1..=10)
                .map(|n| SearchHit {
                    title: Some("t".repeat(100)),
                    nature: Some("n".repeat(600)),
                    ..bare(fond, n)
                })
                .collect();
            corpus.push_search(Ok(page_of(fond, 500, hits)));
        }
        let names: Vec<&str> = Fond::ALL.iter().map(|fond| fond.name()).collect();

        let output = run(&corpus, all_of(Some("bail"), Some(&names), Some(10))).await.unwrap();

        let hits = all_hits(&output);
        assert_eq!(hits.len(), 80);
        assert!(estimate(&output) <= ResponseBudget::ESTIMATE_TARGET_CHARS);
        let note = output.note.as_deref().unwrap();
        assert!(
            note.starts_with("Article lists and the optional fields title, nature, etat"),
            "{note}"
        );
        assert!(hits[0].hit.title.is_some() && hits[79].hit.title.is_none());
        assert!(!note.contains("Snippets"), "aucun extrait n'était à retirer : {note}");
    }

    #[tokio::test]
    async fn a_response_under_the_target_is_not_touched() {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(loaded_page(Fond::Code, 3)));
        corpus.push_search(Ok(loaded_page(Fond::Kali, 3)));

        let output = run(&corpus, named(&["CODE", "KALI"])).await.unwrap();

        assert_eq!(output.note, None);
        assert!(all_hits(&output).iter().all(|out| out.hit.snippet.is_some()));
        assert!(all_hits(&output)[0].hit.articles[0].snippet.is_some());
    }

    // ---------------------------------------------------------------------------------------
    // U16 : estimation exacte de la taille (search_all-2)
    // ---------------------------------------------------------------------------------------

    /// Caractères réservés aux notes dans l'estimation de `fr_search` (411 en dur : un changement
    /// de la réserve se voit ici).
    const NOTE_RESERVE: usize = 411;

    const TARGET: usize = ResponseBudget::ESTIMATE_TARGET_CHARS;

    // ---- l'oracle : la longueur exacte du JSON compact de la vue, écrite à part ----------------

    /// Une chaîne JSON, échappée comme `serde_json` l'écrit.
    fn q(text: &str) -> String {
        let mut out = String::from("\"");
        for c in text.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                c if c < ' ' => {
                    let _ = write!(out, "\\u{:04x}", u32::from(c));
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    /// Le JSON compact d'un résultat, dans l'ordre des champs de la vue.
    fn json_hit(out: &HitOut) -> String {
        let hit = &out.hit;
        let mut fields = vec![
            format!("\"kind\":{}", q(&hit.fond.name().to_ascii_lowercase())),
            format!("\"id\":{}", q(hit.id.as_str())),
        ];
        if let Some(title) =
            lf_domain::text::title_unless_in_citation(hit.title.as_deref(), &out.citation)
        {
            fields.push(format!("\"title\":{}", q(title)));
        }
        if let Some(date) = lf_domain::date::open_ended(hit.date) {
            fields.push(format!("\"date\":\"{date}\""));
        }
        if let Some(nature) = &hit.nature {
            fields.push(format!("\"nature\":{}", q(nature)));
        }
        if let Some(etat) = &hit.etat {
            fields.push(format!("\"etat\":{}", q(etat.as_api_code())));
        }
        if let Some(idcc) = &hit.idcc {
            fields.push(format!("\"idcc\":{}", q(&idcc.to_string())));
        }
        if let Some(snippet) = &hit.snippet {
            fields.push(format!("\"snippet\":{}", q(snippet.as_str())));
        }
        if !hit.articles.is_empty() {
            let articles: Vec<String> = hit
                .articles
                .iter()
                .map(|article| {
                    let mut parts =
                        vec![format!("\"article_id\":{}", q(article.article_id.as_str()))];
                    if let Some(num) = &article.num {
                        parts.push(format!("\"num\":{}", q(num)));
                    }
                    if let Some(snippet) = &article.snippet {
                        parts.push(format!("\"snippet\":{}", q(snippet.as_str())));
                    }
                    format!("{{{}}}", parts.join(","))
                })
                .collect();
            fields.push(format!("\"articles\":[{}]", articles.join(",")));
        }
        fields.push(format!("\"citation\":{}", q(&out.citation)));
        fields.push(format!("\"source_url\":{}", q(&out.source_url)));
        format!("{{{}}}", fields.join(","))
    }

    /// Le JSON compact d'un groupe : ses résultats, ou son erreur.
    fn json_group(group: &GroupOut) -> String {
        let name = q(group.fond.name());
        match &group.result {
            Ok(found) => {
                let hits: Vec<String> = found.hits.iter().map(json_hit).collect();
                format!(
                    "{{\"fond\":{name},\"total\":{},\"has_more\":{},\"hits\":[{}]}}",
                    found.total,
                    found.has_more,
                    hits.join(",")
                )
            }
            Err(error) => {
                let text = match error.code() {
                    Some(code) => format!("[{}] {}", code.as_str(), error.message()),
                    None => error.message().to_owned(),
                };
                format!("{{\"fond\":{name},\"error\":{}}}", q(&text))
            }
        }
    }

    /// Le JSON compact de la réponse, `note` comprise.
    fn json_of(groups: &[GroupOut], note: Option<&str>) -> String {
        let groups: Vec<String> = groups.iter().map(json_group).collect();
        let note = note.map(|note| format!(",\"note\":{}", q(note))).unwrap_or_default();
        format!("{{\"groups\":[{}]{note}}}", groups.join(","))
    }

    /// Longueur exacte, en caractères, de la réponse.
    fn exact_len(groups: &[GroupOut], note: Option<&str>) -> usize {
        json_of(groups, note).chars().count()
    }

    /// Ce que l'application compte pour ces groupes, sans la réserve des notes.
    fn counted(groups: &[GroupOut]) -> usize {
        let hits: usize = groups
            .iter()
            .filter_map(|group| group.result.as_ref().ok())
            .flat_map(|found| found.hits.iter())
            .map(hit_cost)
            .sum();
        envelope_cost(groups) + hits
    }

    // ---- les données : aux longueurs du rejeu de production, sans texte de Légifrance -----------

    /// Un résultat aux longueurs relevées en production : citation de 60 à 150 caractères, extrait
    /// de 280, date, nature et `etat` ; CODE : trois articles à extrait de 272 caractères, sans
    /// extrait propre ; KALI et ACCO : un IDCC. Le titre est celui de la citation (non rendu).
    fn shaped(fond: Fond, n: u32) -> HitOut {
        let base = bare(fond, n);
        let citation = format!(
            "{} n° {n} {}",
            fond.name(),
            "c".repeat(60 + 30 * usize::try_from(n % 4).unwrap())
        );
        let articles = if fond == Fond::Code {
            (1..=3)
                .map(|a| {
                    CodeArticleRef::new(
                        ArticleId::Legi(
                            LegiArtiId::parse(&format!("LEGIARTI{:012}", n * 10 + a)).unwrap(),
                        ),
                        Some("L. 145-1".to_owned()),
                        &"article ".repeat(34),
                    )
                })
                .collect()
        } else {
            vec![]
        };
        let hit = SearchHit {
            title: Some(citation.clone()),
            date: CivilDate::new(2016, 10, 7),
            nature: Some("ARRET".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            snippet: (fond != Fond::Code).then(|| Snippet::new(&"extrait ".repeat(35)).unwrap()),
            idcc: matches!(fond, Fond::Kali | Fond::Acco).then(|| Idcc::parse("1486").unwrap()),
            articles,
            ..base
        };
        let source_url =
            format!("https://www.legifrance.gouv.fr/{}/{}", fond.name(), hit.id.as_str());
        HitOut { hit, citation, source_url }
    }

    fn group_of(fond: Fond, hits: Vec<HitOut>) -> GroupOut {
        GroupOut { fond, result: Ok(FondHits { total: 500, has_more: true, hits }) }
    }

    /// Un groupe par fond, `per_fond` résultats chacun.
    fn shaped_groups(fonds: &[Fond], per_fond: u32) -> Vec<GroupOut> {
        fonds
            .iter()
            .map(|fond| group_of(*fond, (1..=per_fond).map(|n| shaped(*fond, n)).collect()))
            .collect()
    }

    const FIVE: [Fond; 5] = DEFAULT_FONDS;

    /// Règle la `nature` du dernier résultat pour que la réponse sans note fasse `wanted`
    /// caractères exactement.
    fn padded_to(mut groups: Vec<GroupOut>, wanted: usize) -> Vec<GroupOut> {
        let set = |groups: &mut Vec<GroupOut>, pad: usize| {
            let last = groups.last_mut().unwrap().result.as_mut().unwrap().hits.last_mut().unwrap();
            last.hit.nature = Some("n".repeat(pad));
        };
        set(&mut groups, 1);
        let base = exact_len(&groups, None);
        assert!(base <= wanted, "{base} > {wanted}");
        set(&mut groups, 1 + wanted - base);
        assert_eq!(exact_len(&groups, None), wanted);
        groups
    }

    fn snippets_kept(groups: &[GroupOut]) -> usize {
        groups
            .iter()
            .filter_map(|group| group.result.as_ref().ok())
            .flat_map(|found| found.hits.iter())
            .filter(|out| {
                out.hit.snippet.is_some() || out.hit.articles.iter().any(|a| a.snippet.is_some())
            })
            .count()
    }

    fn every_hit_keeps_its_details(groups: &[GroupOut]) -> bool {
        groups
            .iter()
            .filter_map(|group| group.result.as_ref().ok())
            .flat_map(|found| found.hits.iter())
            .all(|out| out.hit.etat.is_some() && out.hit.date.is_some() && out.hit.nature.is_some())
    }

    /// V1 : 5 fonds x 10 résultats aux longueurs de production, JSON exact d'environ 36 000
    /// caractères : sous la cible, rien n'est retiré (la 1.1.0 retirait tous les extraits).
    #[test]
    fn a_production_shaped_response_under_the_cap_is_not_degraded() {
        let mut groups = shaped_groups(&FIVE, 10);
        let before = groups.clone();
        let size = exact_len(&groups, None);
        assert!((33_000..=40_000).contains(&size), "{size}");

        let note = fit_under_target(&mut groups);

        assert_eq!(note, None);
        assert_eq!(groups, before);
    }

    /// V2 : 8 fonds x 10, JSON exact au-dessus de la cible mais sous elle sans les extraits : seuls
    /// des extraits partent, en partant de la fin ; tous les résultats gardent `etat`, `date` et
    /// `nature` (la 1.1.0 les retirait dès le 4e résultat).
    #[test]
    fn etat_date_and_nature_survive_when_removing_snippets_is_enough() {
        let mut groups = shaped_groups(&Fond::ALL, 10);
        assert!(exact_len(&groups, None) > TARGET);
        let mut without = groups.clone();
        for out in without
            .iter_mut()
            .filter_map(|g| g.result.as_mut().ok())
            .flat_map(|f| f.hits.iter_mut())
        {
            strip_snippets(out);
        }
        assert!(exact_len(&without, None) < TARGET - NOTE_RESERVE);

        let note = fit_under_target(&mut groups).expect("the response was degraded");

        assert!(note.starts_with("Snippets were removed from the last "), "{note}");
        assert!(!note.contains("Article lists"), "{note}");
        assert!(every_hit_keeps_its_details(&groups));
        assert!(snippets_kept(&groups) > 0, "les premiers extraits restent");
        assert!(exact_len(&groups, Some(&note)) <= TARGET);
    }

    /// Le bord : la réponse qui tient pile à `cible - réserve` est conservée ; un caractère de plus
    /// et un extrait, le dernier, part.
    #[test]
    fn a_response_exactly_at_the_target_is_kept_and_one_character_more_strips_one_snippet() {
        let edge = TARGET - NOTE_RESERVE;
        let mut at_target = padded_to(shaped_groups(&FIVE, 10), edge);
        let before = at_target.clone();
        assert_eq!(fit_under_target(&mut at_target), None);
        assert_eq!(at_target, before);

        let mut over = padded_to(shaped_groups(&FIVE, 10), edge + 1);
        let before = over.clone();
        let note = fit_under_target(&mut over);
        assert_eq!(note, Some(snippets_note(1)));
        let mut expected = before;
        let last = expected.last_mut().unwrap().result.as_mut().unwrap().hits.last_mut().unwrap();
        last.hit.snippet = None;
        assert_eq!(over, expected);
    }

    /// Minimalité : la dégradation s'arrête dès que la réponse tient ; rendre le dernier extrait
    /// retiré la ferait repasser au-dessus de la limite.
    #[test]
    fn degradation_stops_as_soon_as_the_response_fits() {
        let original = shaped_groups(&Fond::ALL, 10);
        let mut groups = original.clone();
        let note = fit_under_target(&mut groups).expect("degraded");
        assert!(exact_len(&groups, Some(&note)) <= TARGET);

        // Le dernier résultat dont l'extrait a été retiré en dernier : le plus petit indice.
        let flat = |groups: &[GroupOut]| -> Vec<HitOut> {
            groups
                .iter()
                .filter_map(|g| g.result.as_ref().ok())
                .flat_map(|f| f.hits.iter().cloned())
                .collect()
        };
        let (before, after) = (flat(&original), flat(&groups));
        let first_stripped = before
            .iter()
            .zip(&after)
            .position(|(old, new)| old.hit.snippet.is_some() && new.hit.snippet.is_none())
            .or_else(|| {
                before
                    .iter()
                    .zip(&after)
                    .position(|(old, new)| old.hit.articles != new.hit.articles)
            })
            .expect("something was stripped");
        // On rend l'extrait de ce résultat.
        let mut restored = groups.clone();
        let target = restored
            .iter_mut()
            .filter_map(|g| g.result.as_mut().ok())
            .flat_map(|f| f.hits.iter_mut())
            .nth(first_stripped)
            .unwrap();
        *target = before[first_stripped].clone();
        assert!(
            exact_len(&restored, None) + NOTE_RESERVE > TARGET,
            "un extrait de moins aurait suffi : {}",
            exact_len(&restored, None)
        );
    }

    /// Les coûts d'un groupe et de l'enveloppe sont la longueur du JSON compact (littéraux).
    #[test]
    fn group_and_envelope_costs_are_the_length_of_the_compact_json() {
        let json_len = |json: &str| json.chars().count();
        // Un groupe plein, deux résultats : leurs objets sont comptés à part, la virgule et les
        // crochets de la liste sont du groupe.
        let full = group_of(Fond::Loda, vec![shaped(Fond::Loda, 1), shaped(Fond::Loda, 2)]);
        assert_eq!(
            group_cost(&full),
            json_len(r#"{"fond":"LODA","total":500,"has_more":true,"hits":[,]}"#)
        );
        // Un groupe vide.
        let empty = GroupOut {
            fond: Fond::Cnil,
            result: Ok(FondHits { total: 0, has_more: false, hits: vec![] }),
        };
        let empty_json = r#"{"fond":"CNIL","total":0,"has_more":false,"hits":[]}"#;
        assert_eq!(group_cost(&empty), json_len(empty_json));
        assert_eq!(json_group(&empty), empty_json);
        // Un groupe en erreur.
        let failed = GroupOut { fond: Fond::Cnil, result: Err(AppError::config("no credentials")) };
        let failed_json = r#"{"fond":"CNIL","error":"[config_error] no credentials"}"#;
        assert_eq!(group_cost(&failed), json_len(failed_json));
        assert_eq!(json_group(&failed), failed_json);
        // L'enveloppe : `{"groups":[` + groupes séparés par des virgules + `]}`.
        assert_eq!(envelope_cost(&[]), json_len(r#"{"groups":[]}"#));
        assert_eq!(
            envelope_cost(std::slice::from_ref(&empty)),
            json_len(&format!(r#"{{"groups":[{empty_json}]}}"#))
        );
        assert_eq!(
            envelope_cost(&[empty.clone(), failed.clone(), empty.clone()]),
            json_len(&format!(r#"{{"groups":[{empty_json},{failed_json},{empty_json}]}}"#))
        );
        // Un total de plusieurs chiffres, et le `has_more` vrai (4 caractères) ou faux (5).
        let big = GroupOut {
            fond: Fond::Loda,
            result: Ok(FondHits { total: 1_234_567, has_more: true, hits: vec![] }),
        };
        assert_eq!(
            group_cost(&big),
            json_len(r#"{"fond":"LODA","total":1234567,"has_more":true,"hits":[]}"#)
        );
        // Le plein, résultats compris, égale l'oracle.
        assert_eq!(
            counted(std::slice::from_ref(&full)),
            exact_len(std::slice::from_ref(&full), None)
        );
    }

    /// Un groupe vide et un groupe en erreur gardent leur forme exacte dans l'enveloppe.
    #[test]
    fn an_empty_group_and_a_failed_group_keep_their_exact_shape_in_the_envelope() {
        let groups = vec![
            group_of(Fond::Loda, vec![shaped(Fond::Loda, 1)]),
            GroupOut {
                fond: Fond::Cnil,
                result: Ok(FondHits { total: 0, has_more: false, hits: vec![] }),
            },
            GroupOut { fond: Fond::Juri, result: Err(AppError::config("boom")) },
            group_of(Fond::Kali, vec![shaped(Fond::Kali, 1), shaped(Fond::Kali, 2)]),
        ];
        assert_eq!(counted(&groups), exact_len(&groups, None));
    }

    /// Une erreur est comptée `[code] message` ; un appel annulé n'a pas de code : le message seul.
    #[test]
    fn an_error_group_is_counted_with_its_code_and_without_it_when_cancelled() {
        let with_code =
            GroupOut { fond: Fond::Juri, result: Err(AppError::config("no credentials")) };
        assert_eq!(
            group_cost(&with_code),
            r#"{"fond":"JURI","error":"[config_error] no credentials"}"#.chars().count()
        );
        let cancelled = GroupOut {
            fond: Fond::Juri,
            result: Err(AppError::Upstream {
                message: "cancelled".to_owned(),
                cause: UpstreamCause::Cancelled,
            }),
        };
        assert_eq!(cancelled.result.as_ref().unwrap_err().code(), None);
        assert_eq!(
            group_cost(&cancelled),
            r#"{"fond":"JURI","error":"cancelled"}"#.chars().count()
        );
    }

    /// Un groupe en erreur au milieu est compté et ne décale pas l'ordre de dégradation : les
    /// extraits partent en partant du dernier résultat du dernier fond.
    #[test]
    fn an_error_group_inside_the_list_is_counted_and_does_not_shift_the_strip_order() {
        let mut groups = shaped_groups(&Fond::ALL, 10);
        groups[3] = GroupOut { fond: Fond::Constit, result: Err(AppError::config("down")) };
        groups[5] = GroupOut { fond: Fond::Cnil, result: Err(AppError::config("down")) };
        assert_eq!(counted(&groups), exact_len(&groups, None));
        let original = groups.clone();

        let note = fit_under_target(&mut groups);

        assert!(note.is_some_and(|note| note.starts_with("Snippets were removed")));
        assert_eq!(groups[3], original[3], "l'erreur reste telle quelle");
        let hits =
            |group: &GroupOut| group.result.as_ref().map(|f| f.hits.clone()).unwrap_or_default();
        // Le premier résultat du premier fond garde son extrait ; le dernier du dernier le perd.
        assert!(hits(&groups[0])[0].hit.snippet.is_some());
        assert!(hits(&groups[7]).last().unwrap().hit.snippet.is_none());
    }

    /// Les échappements JSON sont comptés : `"`, `\` (deux) et U+0001 (six) dans la citation, dans
    /// `source_url` et dans le message d'une erreur.
    #[test]
    fn json_escapes_in_citations_and_errors_are_counted() {
        let rich = "a\"b\\c\u{1}d";
        let mut hit = shaped(Fond::Juri, 1);
        hit.citation = rich.repeat(5);
        hit.source_url = format!("https://x/{rich}");
        let groups = vec![
            group_of(Fond::Juri, vec![hit]),
            GroupOut { fond: Fond::Cnil, result: Err(AppError::config(rich)) },
        ];
        assert_eq!(counted(&groups), exact_len(&groups, None));
        // Le littéral : trois caractères riches coûtent 1 + 2 + 1 + 2 + 1 + 6 + 1 = 14 de plus.
        let json = json_of(&groups, None);
        assert!(json.contains(r#"a\"b\\c\u0001d"#), "{json}");
    }

    /// KALI : un IDCC et une date sentinelle (2222-01-01, non rendue) ; le coût est exactement le
    /// JSON rendu.
    #[test]
    fn a_kali_hit_with_idcc_and_a_sentinel_date_costs_exactly_its_rendered_json() {
        let hit = SearchHit {
            date: CivilDate::new(2222, 1, 1),
            idcc: Idcc::parse("1486"),
            nature: Some("CONVENTION".to_owned()),
            ..bare(Fond::Kali, 1)
        };
        let out =
            HitOut { citation: "Convention".to_owned(), source_url: "https://u".to_owned(), hit };
        let expected = format!(
            concat!(
                r#"{{"kind":"kali","id":"{}","nature":"CONVENTION","#,
                r#""idcc":"1486","citation":"Convention","source_url":"https://u"}}"#
            ),
            out.hit.id.as_str()
        );
        assert_eq!(json_hit(&out), expected);
        assert_eq!(hit_cost(&out), expected.chars().count());
    }

    /// Un titre contenu dans la citation n'est pas rendu : le retirer ne compte pas comme un détail
    /// retiré, et la note des détails ne le compte pas.
    #[test]
    fn a_hit_whose_title_is_in_its_citation_does_not_count_as_stripped_details() {
        // 80 résultats dont seul le dernier a un `nature` (rendu) : la note compte un résultat,
        // pas les 79 dont seul le titre, déjà dans la citation, a été retiré.
        let mut groups: Vec<GroupOut> = Fond::ALL
            .iter()
            .map(|fond| {
                let hits = (1..=10)
                    .map(|n| {
                        let mut out = shaped(*fond, n);
                        out.hit.snippet = None;
                        out.hit.articles.clear();
                        out.hit.etat = None;
                        out.hit.date = None;
                        out.hit.nature = Some("n".repeat(700));
                        out
                    })
                    .collect();
                group_of(*fond, hits)
            })
            .collect();
        assert!(exact_len(&groups, None) > TARGET);

        let note = fit_under_target(&mut groups).expect("degraded");

        // Chaque résultat retiré avait une nature rendue : le compte est le nombre de natures
        // effectivement retirées.
        let removed = groups
            .iter()
            .filter_map(|g| g.result.as_ref().ok())
            .flat_map(|f| f.hits.iter())
            .filter(|out| out.hit.nature.is_none())
            .count();
        assert!(removed > 0);
        assert_eq!(note, details_note(removed));

        // Seul le titre (dans la citation) à retirer : rien n'est compté, aucune note.
        let mut titles_only: Vec<GroupOut> = Fond::ALL
            .iter()
            .map(|fond| {
                let hits = (1..=10)
                    .map(|n| {
                        let mut out = shaped(*fond, n);
                        out.hit.snippet = None;
                        out.hit.articles.clear();
                        out.hit.etat = None;
                        out.hit.date = None;
                        out.hit.nature = None;
                        out.citation = format!("{} {}", out.citation, "x".repeat(600));
                        out.hit.title = Some(out.citation.clone());
                        out
                    })
                    .collect();
                group_of(*fond, hits)
            })
            .collect();
        assert!(exact_len(&titles_only, None) > TARGET);
        assert_eq!(fit_under_target(&mut titles_only), None, "rien de rendu à retirer");
    }

    /// Un résultat sans extrait entouré de résultats dégradés : « the last N hits » compte les
    /// résultats dont l'extrait a été retiré, comme en 1.1.0 (limite connue, garantie 14 n° 9,
    /// 1.1.2).
    #[test]
    fn the_note_counts_the_suffix_walked_as_in_1_1_0() {
        let mut groups = shaped_groups(&Fond::ALL, 10);
        // L'avant-dernier résultat n'a pas d'extrait.
        let hits = &mut groups[7].result.as_mut().unwrap().hits;
        hits[8].hit.snippet = None;
        let original = groups.clone();

        let note = fit_under_target(&mut groups).expect("degraded");

        let lost = |before: &[GroupOut], after: &[GroupOut]| -> usize {
            before
                .iter()
                .zip(after)
                .map(|(b, a)| {
                    let (b, a) = (b.result.as_ref().unwrap(), a.result.as_ref().unwrap());
                    b.hits
                        .iter()
                        .zip(&a.hits)
                        .filter(|(b, a)| {
                            b.hit.snippet.is_some() && a.hit.snippet.is_none()
                                || b.hit.articles != a.hit.articles
                                    && b.hit.articles.iter().any(|x| x.snippet.is_some())
                        })
                        .count()
                })
                .sum()
        };
        let removed = lost(&original, &groups);
        assert!(removed >= 3, "{removed}");
        assert!(
            note.starts_with(&format!("Snippets were removed from the last {removed} hits ")),
            "{note}"
        );
        assert!(note.contains("the last"), "{note}");
    }

    /// Les deux notes, pour 80 résultats, tiennent dans la réserve de 411 caractères (le champ
    /// `note` et l'espace qui les joint en prennent 11).
    #[test]
    fn the_degrade_notes_fit_in_their_reserve() {
        let both = snippets_note(80).chars().count() + 1 + details_note(80).chars().count();
        assert!(both <= NOTE_RESERVE - 11, "{both}");
    }

    /// L'estimation ne sous-compte jamais, sur V1 (5 x 10), V2 (8 x 10) et V8 (CNIL, CODE, KALI
    /// x 10).
    #[test]
    fn the_estimate_never_undercounts_on_any_surface() {
        for (fonds, per_fond) in [
            (FIVE.to_vec(), 10),
            (Fond::ALL.to_vec(), 10),
            (vec![Fond::Cnil, Fond::Code, Fond::Kali], 10),
            (FIVE.to_vec(), 5),
        ] {
            let groups = shaped_groups(&fonds, per_fond);
            let measured = exact_len(&groups, None);
            assert!(counted(&groups) >= measured, "{fonds:?} x {per_fond}");
        }
    }

    /// V8 : CNIL, CODE et KALI x 10 tiennent sous la cible : rien n'est retiré.
    #[test]
    fn cnil_code_and_kali_times_ten_are_not_degraded() {
        let mut groups = shaped_groups(&[Fond::Cnil, Fond::Code, Fond::Kali], 10);
        assert!(exact_len(&groups, None) < TARGET - NOTE_RESERVE);
        let before = groups.clone();
        assert_eq!(fit_under_target(&mut groups), None);
        assert_eq!(groups, before);
    }

    /// Un total de recherche 5 x 5 (17 800 caractères, le relevé c1030) est inchangé : l'ancienne et
    /// la nouvelle estimation s'accordent à ne rien retirer.
    #[test]
    fn five_by_five_is_untouched() {
        let mut groups = shaped_groups(&FIVE, 5);
        assert!(exact_len(&groups, None) < 25_000);
        let before = groups.clone();
        assert_eq!(fit_under_target(&mut groups), None);
        assert_eq!(groups, before);
    }
}
