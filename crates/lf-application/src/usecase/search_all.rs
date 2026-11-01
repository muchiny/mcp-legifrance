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
//! La taille de toute la réponse est estimée (somme des longueurs plus 64 par champ ou élément,
//! cible 42 000), puis dégradée en partant du dernier résultat du dernier fond, sans jamais retirer
//! un résultat : (1) extraits ; (2) listes d'articles et champs optionnels. Une `note` le dit. La
//! mesure exacte, sur le JSON, est faite par la vue de `lf-adapter-mcp`.
//!
//! # Progression
//!
//! `progress(faits, total)` est appelé à la fin de chaque fond, dans l'ordre d'arrivée. Le cas
//! d'usage ne sait rien du transport : la façade et l'adaptateur décident de ce qu'ils en font.

use futures::{StreamExt, stream};
use lf_domain::{
    entity::Snippet,
    fond::Fond,
    search::{QueryText, check_quotes},
    status::LegalStatus,
};

use crate::{
    budget::{ResponseBudget, bounded, estimate_chars, strip_from_end, the_last_hits},
    error::AppError,
    input::{SearchAllInput, SearchInput},
    messages,
    output::{
        search::HitOut,
        search_all::{FondHits, GroupOut, SearchAllOutput},
    },
    ports::{Clock, LegalCorpus},
    tools::ToolName,
    usecase::search::SearchUseCase,
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

/// Longueur ISO d'une date rendue, `AAAA-MM-JJ`.
const ISO_DATE_CHARS: usize = 10;

/// Longueur maximale d'un IDCC rendu (1 à 9999).
const IDCC_CHARS: usize = 4;

/// Caractères d'un nom de fond (`CONSTIT`), le plus long.
const FOND_NAME_CHARS: usize = 7;

/// Caractères du préfixe `[upstream_error] ` d'une entrée d'erreur, le plus long.
const ERROR_PREFIX_CHARS: usize = 17;

/// Caractères réservés aux deux notes de dégradation : elles tiennent largement dedans.
const DEGRADE_NOTE_RESERVE_CHARS: usize = 400;

/// Coût estimé de la réservation des notes : le champ `note` et leur texte.
const DEGRADE_NOTE_RESERVE: usize = estimate_chars(1, DEGRADE_NOTE_RESERVE_CHARS);

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

/// Dégrade les résultats, du dernier au premier, jusqu'à ce que l'estimation de la réponse passe
/// sous la cible (05 §6.4). Rend la `note` qui le dit, s'il y a eu dégradation.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn fit_under_target(groups: &mut [GroupOut]) -> Option<String> {
    let fixed = estimate_chars(1, 0)
        + DEGRADE_NOTE_RESERVE
        + groups.iter().map(group_cost).fold(0, usize::saturating_add);
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
    let details = strip_from_end(&mut hits, over, |out| strip_details(out));
    let notes = [
        (snippets > 0).then(|| snippets_note(snippets)),
        (details > 0).then(|| details_note(details)),
    ];
    let notes: Vec<String> = notes.into_iter().flatten().collect();
    (!notes.is_empty()).then(|| notes.join(" "))
}

/// Coût estimé d'un groupe hors résultats : `fond`, `total`, `has_more` et `hits`, ou `fond` et
/// `error`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn group_cost(group: &GroupOut) -> usize {
    match &group.result {
        Ok(_) => estimate_chars(4, FOND_NAME_CHARS),
        Err(error) => {
            estimate_chars(2, FOND_NAME_CHARS + ERROR_PREFIX_CHARS + chars(error.message()))
        }
    }
}

/// Retire l'extrait du résultat et ceux de ses articles ; vrai s'il y en avait un.
fn strip_snippets(out: &mut HitOut) -> bool {
    let mut stripped = out.hit.snippet.take().is_some();
    for article in &mut out.hit.articles {
        stripped |= article.snippet.take().is_some();
    }
    stripped
}

/// Retire la liste d'articles et les champs optionnels `title`, `nature`, `etat` et `date` ; vrai
/// s'il y avait quelque chose à retirer. L'`idcc` reste : il identifie la convention.
fn strip_details(out: &mut HitOut) -> bool {
    let hit = &mut out.hit;
    let had_articles = !hit.articles.is_empty();
    hit.articles.clear();
    let had_fields = [
        hit.title.take().is_some(),
        hit.nature.take().is_some(),
        hit.etat.take().is_some(),
        hit.date.take().is_some(),
    ];
    had_articles || had_fields.contains(&true)
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

/// Coût estimé d'un résultat : 64 par champ ou élément, plus la longueur de ses textes. `kind`,
/// `id`, `citation` et `source_url` sont toujours rendus ; le reste seulement s'il est présent.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn hit_cost(out: &HitOut) -> usize {
    let hit = &out.hit;
    let articles = hit.articles.iter().flat_map(|article| {
        [
            Some(article.article_id.as_str()),
            article.num.as_deref(),
            article.snippet.as_ref().map(Snippet::as_str),
        ]
    });
    let texts = [
        Some(hit.fond.name()),
        Some(hit.id.as_str()),
        Some(out.citation.as_str()),
        Some(out.source_url.as_str()),
        hit.title.as_deref(),
        hit.nature.as_deref(),
        hit.etat.as_ref().map(LegalStatus::as_api_code),
        hit.snippet.as_ref().map(Snippet::as_str),
    ]
    .into_iter()
    .chain(articles)
    .flatten();
    let (fields, text) =
        texts.fold((0, 0), |(fields, text), value| (fields + 1, text + chars(value)));
    let dated = usize::from(hit.date.is_some());
    let numbered = usize::from(hit.idcc.is_some());
    // Les dates et l'IDCC ont une longueur connue ; la liste d'articles et chacun de ses éléments
    // sont des champs de plus.
    let structure = dated + numbered + usize::from(!hit.articles.is_empty()) + hit.articles.len();
    estimate_chars(fields + structure, text + dated * ISO_DATE_CHARS + numbered * IDCC_CHARS)
}

/// Longueur en caractères (points de code) : l'unité du plafond.
fn chars(text: &str) -> usize {
    text.chars().count()
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::DateTime;
    use lf_domain::{
        contract::{Citable, Idcc},
        date::{CivilDate, VersionDate},
        entity::{CodeArticleRef, SearchHit, SearchPage},
        fond::PaginationMode,
        id::{ArticleId, DocumentId, LegiArtiId},
        search::MatchMode,
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
        let fixed = estimate_chars(1, 0)
            + DEGRADE_NOTE_RESERVE
            + groups.iter().map(group_cost).fold(0, usize::saturating_add);
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
        // Quatre fonds de dix résultats chargés : un peu au-delà de 42 000 caractères estimés, et
        // les extraits suffisent à repasser dessous.
        let fonds = [Fond::Loda, Fond::Code, Fond::Kali, Fond::Juri];
        let corpus = FakeCorpus::new();
        let mut untrimmed = 0;
        for fond in fonds {
            let page = loaded_page(fond, 10);
            untrimmed += page.hits.iter().map(|hit| hit_cost(&hit_out_of(hit))).sum::<usize>();
            corpus.push_search(Ok(page));
        }
        assert!(untrimmed > ResponseBudget::ESTIMATE_TARGET_CHARS, "{untrimmed}");
        let names: Vec<&str> = fonds.iter().map(|fond| fond.name()).collect();

        let output = run(&corpus, all_of(Some("bail"), Some(&names), Some(10))).await.unwrap();

        // Aucun résultat n'est retiré ; l'estimation passe sous la cible.
        let hits = all_hits(&output);
        assert_eq!(hits.len(), 40);
        assert!(estimate(&output) <= ResponseBudget::ESTIMATE_TARGET_CHARS);
        // Les extraits partent d'abord, en partant de la fin : le premier résultat garde le sien.
        assert!(hits[0].hit.snippet.is_some());
        assert!(hits[39].hit.snippet.is_none());
        assert!(hits[39].hit.title.is_some(), "les titres ne partent qu'ensuite");
        let note = output.note.as_deref().expect("the response was degraded");
        assert!(note.starts_with("Snippets were removed from the last "), "{note}");
        assert!(note.contains("use fr_search on one fond"), "{note}");
        assert!(!note.contains("Article lists"), "{note}");
        // Les extraits d'articles partent avec ceux des résultats.
        let code_hit = &output.groups[1].result.as_ref().unwrap().hits;
        assert!(code_hit[0].hit.articles[0].snippet.is_some());
        assert!(code_hit[9].hit.articles[0].snippet.is_none());
        assert!(code_hit[9].hit.articles[0].num.is_some(), "l'article reste, sans son extrait");
        // Les citations, calculées avant la dégradation, restent entières.
        assert!(hits[39].citation.starts_with("JURI 10 titre"));
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
                    nature: Some("n".repeat(200)),
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

    #[test]
    fn strip_helpers_report_whether_they_removed_something() {
        let mut full = hit_out_of(&loaded(Fond::Code, 1));
        assert!(strip_snippets(&mut full));
        assert!(!strip_snippets(&mut full), "déjà retirés");
        assert!(strip_details(&mut full));
        assert!(!strip_details(&mut full), "déjà retirés");
        assert_eq!(
            (full.hit.title, full.hit.nature, full.hit.etat, full.hit.date),
            (None, None, None, None)
        );
        assert_eq!(full.hit.articles, Vec::<CodeArticleRef>::new());
        // Un article sans extrait n'est pas un extrait retiré.
        let mut only_article = hit_out_of(&SearchHit {
            articles: vec![CodeArticleRef::new(
                ArticleId::Legi(LegiArtiId::parse("LEGIARTI000000000001").unwrap()),
                None,
                "",
            )],
            ..bare(Fond::Code, 1)
        });
        assert!(!strip_snippets(&mut only_article));
        assert!(strip_details(&mut only_article), "la liste d'articles part");
        // L'IDCC reste.
        let mut kali = hit_out_of(&loaded(Fond::Kali, 1));
        strip_details(&mut kali);
        assert!(kali.hit.idcc.is_some());
    }

    #[test]
    fn the_cost_of_a_hit_counts_its_fields_and_its_texts() {
        let bare_hit = hit_out_of(&bare(Fond::Juri, 1));
        let base = hit_cost(&bare_hit);
        // Quatre champs toujours rendus.
        let texts = "JURI".len()
            + bare_hit.hit.id.as_str().len()
            + bare_hit.citation.chars().count()
            + bare_hit.source_url.chars().count();
        assert_eq!(base, estimate_chars(4, texts));
        // Une date et un IDCC : un champ chacun, de longueur connue.
        let dated = SearchHit {
            date: CivilDate::new(2016, 10, 7),
            idcc: Idcc::parse("1486"),
            ..bare(Fond::Juri, 1)
        };
        assert_eq!(hit_cost(&hit_out_of(&dated)), base + 2 * 64 + ISO_DATE_CHARS + IDCC_CHARS);
        // Une liste d'articles : le champ, l'élément, l'identifiant, le numéro, l'extrait.
        let with_article = SearchHit {
            articles: vec![CodeArticleRef::new(
                ArticleId::Legi(LegiArtiId::parse("LEGIARTI000000000001").unwrap()),
                Some("9".to_owned()),
                "abc",
            )],
            ..bare(Fond::Juri, 1)
        };
        assert_eq!(hit_cost(&hit_out_of(&with_article)), base + 5 * 64 + 20 + 1 + 3);
    }

    #[test]
    fn the_cost_of_an_error_group_counts_its_message() {
        let failed = GroupOut { fond: Fond::Cnil, result: Err(AppError::config("no credentials")) };
        assert_eq!(
            group_cost(&failed),
            estimate_chars(2, FOND_NAME_CHARS + ERROR_PREFIX_CHARS + 14)
        );
        let found = GroupOut {
            fond: Fond::Cnil,
            result: Ok(FondHits { total: 0, has_more: false, hits: vec![] }),
        };
        assert_eq!(group_cost(&found), estimate_chars(4, FOND_NAME_CHARS));
    }
}
