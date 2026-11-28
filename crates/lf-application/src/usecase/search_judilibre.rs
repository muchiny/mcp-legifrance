//! Cas d'usage de `fr_search_judilibre` (jalon MJ) : la recherche dans Judilibre
//! (`SearchJudilibreUseCase`, 05 §5.18).
//!
//! # Validation
//!
//! Sans réseau et dans l'ordre de 05 §5.18 : `query` (requise, 1 à 450 caractères), `match`,
//! `courts` (1 à 5 valeurs distinctes), `part`, `date_from` puis `date_to`, `sort`, `page_size`
//! (1 à 50), `page`, puis l'ordre des dates et la fenêtre atteignable (`CaseLawQuery::new` :
//! `page × page_size` ≤ 10 000, au-delà l'API rend `416`). Le premier échec est rendu ; aucun
//! appel au port ne part.
//!
//! # Réponse du port
//!
//! Une page qui porte plus de résultats que `page_size` n'est pas conforme : `upstream_error`.
//! Une page vide malgré un `total` positif porte une `note` qui dit pourquoi ; aucun résultat du
//! tout, une `note` qui propose d'élargir la recherche (ADR-0035, décision 4).
//!
//! # Plafond de réponse (05 §6.4)
//!
//! Comme pour `fr_search_bofip` : la taille de la page est calculée au caractère près, comme le
//! JSON compact de la vue l'écrira, citation comprise, plus une réserve pour ses champs
//! constants ([`CASE_LAW_CONSTANTS_RESERVE_CHARS`]). Un sommaire de la Cour de cassation peut
//! être long : la page est dégradée sans jamais retirer un résultat, en raccourcissant les
//! sommaires des derniers résultats à [`EXCERPT_MAX_CHARS`] caractères, avec une `note` ; si elle
//! ne tient toujours pas, `invalid_arg` propose la plus grande `page_size` qui tiendrait.

use std::num::NonZeroU32;

use lf_domain::{
    date::{CivilDate, VersionDate},
    judilibre::{
        CASE_LAW_PAGE_SIZE_MAX, CaseLawFilters, CaseLawHit, CaseLawPage, CaseLawQuery, CaseLawSort,
        Court, CourtSet, EXCERPT_MAX_CHARS, SearchPart,
    },
    search::{MatchMode, PageSize, QueryText, has_more},
};

use super::search::{
    DEGRADE_NOTE_RESERVE, JsonObject, digits, join_notes, largest_page_size, page_of,
};
use crate::{
    budget::{ResponseBudget, strip_from_end},
    error::AppError,
    failure::case_law_failure,
    input::SearchJudilibreInput,
    messages,
    output::search_judilibre::{CASE_LAW_CONSTANTS_RESERVE_CHARS, SearchJudilibreOutput},
    ports::{CaseLawError, Clock, JudicialOpenData},
    tools::ToolName,
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_SEARCH_JUDILIBRE;

/// Note d'une recherche sans résultat : ce que l'appelant peut changer, et que le vide ne prouve
/// rien.
pub(crate) const NO_MATCH_NOTE: &str = "No decision matches: try match=\"any\", fewer or other \
     words, or other courts; an empty result does not mean that no court has ruled on the \
     subject.";

/// Note de fin de fenêtre atteignable : Judilibre ne sert pas au-delà de 10 000 résultats.
const REACHABLE_NOTE: &str = "Results beyond the first 10,000 are not reachable; narrow the query \
                              with match, courts, part or date_from/date_to.";

/// Détail de journal d'une page plus longue que demandé.
const TOO_MANY_HITS: &str = "more hits than the requested page_size";

/// Valeurs admises de `match`, dans l'ordre des messages.
const MATCH_NAMES: [&str; 3] = ["all", "any", "exact"];

/// Recherche dans Judilibre : analyse des arguments, un appel au port, politique de réponse.
pub(crate) struct SearchJudilibreUseCase<'a, J: JudicialOpenData, K: Clock> {
    case_law: &'a J,
    clock: &'a K,
}

impl<'a, J: JudicialOpenData, K: Clock> SearchJudilibreUseCase<'a, J, K> {
    /// Cas d'usage qui interroge `case_law` ; `clock` date la consultation.
    pub(crate) fn new(case_law: &'a J, clock: &'a K) -> Self {
        Self { case_law, clock }
    }

    /// Valide `input`, interroge le port et met la page en forme.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour tout argument refusé (sans appel au port) ou pour une page qui ne tient
    /// pas sous le plafond de réponse ; `upstream_error` pour une page plus longue que demandé ;
    /// sinon l'erreur du port, traduite par [`case_law_failure`].
    pub(crate) async fn execute(
        &self,
        input: SearchJudilibreInput,
    ) -> Result<SearchJudilibreOutput, AppError> {
        let query = query_of(&input)?;
        let page = self
            .case_law
            .search(&query)
            .await
            .map_err(|error| case_law_failure(TOOL, None, &error))?;
        present(&query, page, self.clock.today_paris())
    }
}

/// Requête validée du port, ou premier argument refusé.
fn query_of(input: &SearchJudilibreInput) -> Result<CaseLawQuery, AppError> {
    let text = text_of(input.query.as_deref())?;
    let mode = mode_of(input.match_mode.as_deref())?;
    let courts = courts_of(input.courts.as_deref())?;
    let part = part_of(input.part.as_deref())?;
    let date_from = date_of("date_from", input.date_from.as_deref())?;
    let date_to = date_of("date_to", input.date_to.as_deref())?;
    let sort = sort_of(input.sort.as_deref())?;
    let page_size = page_size_of(input.page_size)?;
    let page = page_of(input.page)?;
    let filters = CaseLawFilters { mode, courts, part, date_from, date_to, sort };
    CaseLawQuery::new(text, filters, page, page_size)
        .map_err(|error| messages::domain(TOOL, &error))
}

/// `query` : requise, normalisée et bornée par le domaine.
fn text_of(value: Option<&str>) -> Result<QueryText, AppError> {
    let value = value.ok_or_else(|| messages::required("query"))?;
    QueryText::parse(value).map_err(|error| messages::domain(TOOL, &error))
}

/// `match` : `all` par défaut (ADR-0035, décision 4), insensible à la casse et aux espaces.
fn mode_of(value: Option<&str>) -> Result<MatchMode, AppError> {
    let Some(value) = value else { return Ok(MatchMode::All) };
    match value.trim().to_ascii_lowercase().as_str() {
        "any" => Ok(MatchMode::Any),
        "all" => Ok(MatchMode::All),
        "exact" => Ok(MatchMode::Exact),
        _ => Err(messages::not_one_of("match", value, &MATCH_NAMES)),
    }
}

/// `courts` : les cinq par défaut ; sinon de 1 à 5 valeurs connues et distinctes.
fn courts_of(values: Option<&[String]>) -> Result<CourtSet, AppError> {
    let Some(values) = values else { return Ok(CourtSet::ALL) };
    let mut set: Option<CourtSet> = None;
    for value in values {
        let court = Court::parse(value)
            .ok_or_else(|| messages::not_one_of("courts", value, &Court::CODES))?;
        let Some(known) = set.as_mut() else {
            set = CourtSet::of([court]);
            continue;
        };
        if !known.insert(court) {
            return Err(messages::invalid_argument(format!(
                "courts lists '{}' twice; give each court once.",
                court.code()
            )));
        }
    }
    set.ok_or_else(|| {
        messages::invalid_argument(
            "courts is empty; give 1 to 5 of 'cc', 'ca', 'tj', 'tcom', 'cph', or omit it to \
             search all of them.",
        )
    })
}

/// `part` : absente par défaut (tout le texte).
fn part_of(value: Option<&str>) -> Result<Option<SearchPart>, AppError> {
    value
        .map(|value| {
            SearchPart::parse(value)
                .ok_or_else(|| messages::not_one_of("part", value, &SearchPart::NAMES))
        })
        .transpose()
}

/// Date `AAAA-MM-JJ` de l'argument `name`, sans rognage.
fn date_of(name: &'static str, value: Option<&str>) -> Result<Option<VersionDate>, AppError> {
    value
        .map(|raw| {
            VersionDate::parse(raw).map_err(|error| messages::domain_arg(TOOL, Some(name), &error))
        })
        .transpose()
}

/// `sort` : `relevance` par défaut.
fn sort_of(value: Option<&str>) -> Result<CaseLawSort, AppError> {
    let Some(value) = value else { return Ok(CaseLawSort::default()) };
    CaseLawSort::parse(value)
        .ok_or_else(|| messages::not_one_of("sort", value, &CaseLawSort::NAMES))
}

/// `page_size` : 10 par défaut, de 1 à [`CASE_LAW_PAGE_SIZE_MAX`].
fn page_size_of(value: Option<i64>) -> Result<PageSize, AppError> {
    let Some(value) = value else { return Ok(PageSize::DEFAULT) };
    u32::try_from(value)
        .ok()
        .filter(|size| *size <= CASE_LAW_PAGE_SIZE_MAX)
        .and_then(|size| PageSize::new(size).ok())
        .ok_or_else(|| {
            messages::out_of_range(
                "page_size",
                value,
                i64::from(PageSize::MIN),
                i64::from(CASE_LAW_PAGE_SIZE_MAX),
            )
        })
}

/// Met la page du port en forme : suite, notes, plafond de réponse.
fn present(
    query: &CaseLawQuery,
    page: CaseLawPage,
    consulted_on: CivilDate,
) -> Result<SearchJudilibreOutput, AppError> {
    let (number, size) = (query.page(), query.page_size());
    let asked = usize::try_from(size.get()).unwrap_or(usize::MAX);
    if page.hits.len() > asked {
        let error = CaseLawError::Decode { detail: TOO_MANY_HITS.to_owned() };
        return Err(case_law_failure(TOOL, None, &error));
    }
    let more = has_more(number, size, page.total);
    let note = page_note(number, size, &page);
    let mut output = SearchJudilibreOutput {
        total: page.total,
        page: number.get(),
        has_more: more,
        hits: page.hits,
        consulted_on,
        note,
    };
    fit_under_target(&mut output, size)?;
    Ok(output)
}

/// Note de la page servie : aucun résultat, page vide malgré un `total` positif (anomalie de
/// l'API, ou résultats tous illisibles, donc écartés par l'adapter), page au-delà de la dernière,
/// ou fin de la fenêtre atteignable ; `None` pour une page ordinaire. Les quatre cas s'excluent.
fn page_note(number: NonZeroU32, size: PageSize, page: &CaseLawPage) -> Option<String> {
    if page.total == 0 && page.hits.is_empty() {
        return Some(NO_MATCH_NOTE.to_owned());
    }
    let per_page = u64::from(size.get());
    let before = u64::from(number.get()).saturating_sub(1).saturating_mul(per_page);
    if page.hits.is_empty() {
        // `page × page_size` ne dépasse jamais 10 000 (refusé avant l'appel) : une page en deçà
        // du total est toujours dans la fenêtre atteignable, et une page au-delà du total suit
        // un total de moins de 10 000 résultats.
        if before < page.total {
            return Some(format!(
                "Judilibre returned no readable decision on this page although it reports {} \
                 results; retry once, or try another page.",
                page.total
            ));
        }
        return Some(format!(
            "This page is past the last one: {} results in all, so the last page is {} with \
             page_size={per_page}.",
            page.total,
            page.total.div_ceil(per_page)
        ));
    }
    // Il reste des résultats après cette page, mais la fenêtre atteignable s'arrête ici.
    let seen = before.saturating_add(per_page);
    (!has_more(number, size, page.total) && seen < page.total).then(|| REACHABLE_NOTE.to_owned())
}

/// Raccourcit des sommaires jusqu'à ce que la taille de `output` passe sous la cible (05 §6.4),
/// ou refuse la page.
fn fit_under_target(
    output: &mut SearchJudilibreOutput,
    requested: PageSize,
) -> Result<(), AppError> {
    let fixed = envelope_cost(output)
        .saturating_add(CASE_LAW_CONSTANTS_RESERVE_CHARS)
        .saturating_add(DEGRADE_NOTE_RESERVE);
    let over = |hits: &[CaseLawHit]| {
        fixed.saturating_add(page_cost(hits)) > ResponseBudget::ESTIMATE_TARGET_CHARS
    };
    let shortened = strip_from_end(&mut output.hits, over, shorten_summary);
    if over(&output.hits) {
        let largest = largest_page_size(&output.hits, fixed, hit_cost);
        return Err(messages::invalid_argument(format!(
            "page_size={} would exceed the 45,000-character response cap for this query; use \
             page_size <= {largest}.",
            requested.get()
        )));
    }
    let summaries = (shortened > 0).then(|| summaries_note(shortened));
    output.note = join_notes([output.note.take(), summaries]);
    Ok(())
}

/// Coupe le sommaire du résultat à [`EXCERPT_MAX_CHARS`] caractères, `…` compris ; vrai s'il
/// était plus long.
fn shorten_summary(hit: &mut CaseLawHit) -> bool {
    let Some(summary) = hit.summary.as_mut() else { return false };
    let long = summary.chars().count() > EXCERPT_MAX_CHARS;
    if long {
        let kept = messages::clip(summary, EXCERPT_MAX_CHARS.saturating_sub(1));
        *summary = format!("{kept}…");
    }
    long
}

/// Note des sommaires raccourcis : leur nombre, et comment les lire en entier.
fn summaries_note(count: usize) -> String {
    let subject = if count == 1 {
        "1 long summary was".to_owned()
    } else {
        format!("{count} long summaries were")
    };
    format!(
        "{subject} shortened to keep the response under 45,000 characters; use a smaller \
         page_size, or fr_get_judilibre, to get them whole."
    )
}

/// Longueur du JSON compact de l'enveloppe, `hits` vide et sans les champs constants de la vue :
/// `total`, `page`, `has_more`, `hits`, `consulted_on`, plus `note` quand elle est présente.
fn envelope_cost(output: &SearchJudilibreOutput) -> usize {
    let mut json = JsonObject::default();
    json.raw("total", digits(output.total));
    json.raw("page", digits(u64::from(output.page)));
    json.raw("has_more", if output.has_more { 4 } else { 5 });
    json.raw("hits", 2);
    json.text("consulted_on", &output.consulted_on.to_string());
    json.optional_text("note", output.note.as_deref());
    json.total()
}

/// Longueur du JSON compact des résultats, sans les crochets de la liste (ceux de
/// [`envelope_cost`]) : leurs objets et les virgules qui les séparent.
fn page_cost(hits: &[CaseLawHit]) -> usize {
    hits.iter()
        .map(hit_cost)
        .fold(0, usize::saturating_add)
        .saturating_add(hits.len().saturating_sub(1))
}

/// Longueur du JSON compact d'un résultat, telle que la vue l'écrit : `id`, `court`, `date` et
/// `citation` toujours ; `chamber`, `formation`, `number`, `ecli`, `solution`, `publication`,
/// `summary`, `excerpt` et `themes` s'ils sont présents.
fn hit_cost(hit: &CaseLawHit) -> usize {
    let mention = &hit.mention;
    let mut json = JsonObject::default();
    json.text("id", hit.id.as_str());
    json.text("court", &mention.court);
    json.optional_text("chamber", mention.chamber.as_deref());
    json.optional_text("formation", mention.formation.as_deref());
    json.text("date", &mention.date);
    json.optional_text("number", hit.number.as_deref());
    json.optional_text("ecli", hit.ecli.as_deref());
    json.optional_text("solution", hit.solution.as_deref());
    json.optional_text("publication", hit.publication.as_deref());
    json.optional_text("summary", hit.summary.as_deref());
    json.optional_text("excerpt", hit.excerpt.as_deref());
    json.optional_texts("themes", &hit.themes);
    json.text("citation", &mention.citation(hit.number.as_deref()));
    json.total()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        error::{ErrorCode, UpstreamCause},
        testing::{CaseLawRequest, FakeCaseLaw, FixedClock, fixtures::judilibre},
    };

    fn clock() -> FixedClock {
        FixedClock::new(chrono::DateTime::UNIX_EPOCH, CivilDate::new(2026, 11, 27).unwrap())
    }

    fn text(query: &str) -> SearchJudilibreInput {
        SearchJudilibreInput { query: Some(query.to_owned()), ..SearchJudilibreInput::default() }
    }

    async fn run(
        input: SearchJudilibreInput,
        response: Result<CaseLawPage, CaseLawError>,
    ) -> (Result<SearchJudilibreOutput, AppError>, FakeCaseLaw) {
        let case_law = FakeCaseLaw::new();
        case_law.push_search(response);
        let clock = clock();
        let result = SearchJudilibreUseCase::new(&case_law, &clock).execute(input).await;
        (result, case_law)
    }

    /// Le message `invalid_arg` de `input`, sans appel au port.
    async fn refused(input: SearchJudilibreInput) -> String {
        let (result, case_law) = run(input, Ok(judilibre::page(0, Vec::new()))).await;
        assert_eq!(case_law.calls(), 0);
        let error = result.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{error:?}");
        error.message().to_owned()
    }

    #[tokio::test]
    async fn a_valid_search_sends_the_typed_query_and_dates_the_consultation() {
        let input = SearchJudilibreInput {
            query: Some("  donation   nue-propriété ".to_owned()),
            match_mode: Some(" EXACT ".to_owned()),
            courts: Some(vec!["ca".to_owned(), "CC".to_owned()]),
            part: Some("motivations".to_owned()),
            date_from: Some("2019-01-01".to_owned()),
            date_to: Some("2024-12-31".to_owned()),
            sort: Some("date".to_owned()),
            page: Some(3),
            page_size: Some(50),
        };
        let hits = vec![judilibre::hit(judilibre::CC_ID)];
        let (result, case_law) = run(input, Ok(judilibre::page(400, hits.clone()))).await;
        let output = result.unwrap();
        assert_eq!(
            output,
            SearchJudilibreOutput {
                total: 400,
                page: 3,
                has_more: true,
                hits,
                consulted_on: CivilDate::new(2026, 11, 27).unwrap(),
                note: None,
            }
        );
        assert_eq!(output.output_count_or_size(), 1);
        let expected = CaseLawQuery::new(
            QueryText::parse("donation nue-propriété").unwrap(),
            CaseLawFilters {
                mode: MatchMode::Exact,
                courts: CourtSet::of([Court::Appeal, Court::Cassation]).unwrap(),
                part: Some(SearchPart::Motivations),
                date_from: Some(VersionDate::parse("2019-01-01").unwrap()),
                date_to: Some(VersionDate::parse("2024-12-31").unwrap()),
                sort: CaseLawSort::Date,
            },
            NonZeroU32::new(3).unwrap(),
            PageSize::new(50).unwrap(),
        )
        .unwrap();
        assert_eq!(case_law.seen_requests(), vec![CaseLawRequest::Search(expected)]);
    }

    #[tokio::test]
    async fn defaults_are_all_words_every_court_by_relevance() {
        let (result, case_law) = run(text("bail"), Ok(judilibre::page(0, Vec::new()))).await;
        assert!(result.is_ok());
        assert_eq!(
            case_law.seen_requests(),
            vec![CaseLawRequest::Search(judilibre::query("bail"))]
        );
    }

    #[tokio::test]
    async fn invalid_args_never_reach_the_port() {
        let with = |edit: fn(&mut SearchJudilibreInput)| {
            let mut input = text("bail");
            edit(&mut input);
            input
        };
        let cases: Vec<(SearchJudilibreInput, &str)> = vec![
            (SearchJudilibreInput::default(), "query is required."),
            (text("   "), "query is required."),
            (
                with(|input| input.match_mode = Some("or".to_owned())),
                "match='or' must be one of ['all', 'any', 'exact'].",
            ),
            (
                with(|input| input.courts = Some(vec!["ce".to_owned()])),
                "courts='ce' must be one of ['cc', 'ca', 'tj', 'tcom', 'cph'].",
            ),
            (
                with(|input| input.courts = Some(Vec::new())),
                "courts is empty; give 1 to 5 of 'cc', 'ca', 'tj', 'tcom', 'cph', or omit it to \
                 search all of them.",
            ),
            (
                with(|input| input.courts = Some(vec!["tj".to_owned(), " TJ".to_owned()])),
                "courts lists 'tj' twice; give each court once.",
            ),
            (
                with(|input| input.part = Some("annexes".to_owned())),
                "part='annexes' must be one of ['motivations', 'dispositif', 'expose', 'moyens', \
                 'summary', 'visa'].",
            ),
            (
                with(|input| input.date_from = Some("2020-13-01".to_owned())),
                "date_from='2020-13-01' is not a valid date (expected YYYY-MM-DD, e.g. \
                 2026-10-15).",
            ),
            (
                with(|input| input.date_to = Some("hier".to_owned())),
                "date_to='hier' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15).",
            ),
            (
                with(|input| input.sort = Some("scorepub".to_owned())),
                "sort='scorepub' must be one of ['relevance', 'date'].",
            ),
            (with(|input| input.page_size = Some(51)), "page_size=51 out of range (1..50)."),
            (with(|input| input.page_size = Some(0)), "page_size=0 out of range (1..50)."),
            (
                with(|input| input.page_size = Some(i64::MAX)),
                "page_size=9223372036854775807 out of range (1..50).",
            ),
            (with(|input| input.page = Some(0)), "page=0 out of range (1..)."),
            (
                with(|input| {
                    input.date_from = Some("2021-01-01".to_owned());
                    input.date_to = Some("2020-12-31".to_owned());
                }),
                "date_from=2021-01-01 is after date_to=2020-12-31.",
            ),
            (
                with(|input| {
                    input.page = Some(201);
                    input.page_size = Some(50);
                }),
                "page=201 with page_size=50 goes beyond the 10000 reachable results; narrow the \
                 query with match, courts, part or date_from/date_to.",
            ),
        ];
        for (input, message) in cases {
            assert_eq!(refused(input).await, message);
        }
        // Le premier argument fautif l'emporte : `match` avant `courts`, `sort` avant `page_size`.
        let mut both = with(|input| input.courts = Some(vec!["xx".to_owned()]));
        both.match_mode = Some("or".to_owned());
        assert!(refused(both).await.starts_with("match="));
        let mut both = with(|input| input.page_size = Some(99));
        both.sort = Some("x".to_owned());
        assert!(refused(both).await.starts_with("sort="));
    }

    #[tokio::test]
    async fn a_search_without_any_result_invites_to_widen_it() {
        let (result, _) = run(text("bail"), Ok(judilibre::page(0, Vec::new()))).await;
        let output = result.unwrap();
        assert_eq!((output.total, output.has_more), (0, false));
        assert_eq!(output.note.as_deref(), Some(NO_MATCH_NOTE));
    }

    #[tokio::test]
    async fn empty_page_with_positive_total_has_a_note() {
        // Au-delà de la dernière page : 25 résultats, pages de 10, page 4.
        let past = SearchJudilibreInput { page: Some(4), ..text("bail") };
        let (result, _) = run(past, Ok(judilibre::page(25, Vec::new()))).await;
        assert_eq!(
            result.unwrap().note.as_deref(),
            Some(
                "This page is past the last one: 25 results in all, so the last page is 3 with \
                  page_size=10."
            )
        );
        // Page 1 000 de 10 résultats : la dernière de la fenêtre atteignable, en deçà d'un
        // total de 50 000. Vide, c'est une anomalie, jamais une page au-delà de la dernière.
        let far = SearchJudilibreInput { page: Some(1_000), ..text("bail") };
        let (result, _) = run(far, Ok(judilibre::page(50_000, Vec::new()))).await;
        assert_eq!(
            result.unwrap().note.as_deref(),
            Some(
                "Judilibre returned no readable decision on this page although it reports 50000 \
                  results; retry once, or try another page."
            )
        );
        // Page vide en deçà du total : anomalie de l'API, ou résultats tous illisibles.
        let (result, _) = run(text("bail"), Ok(judilibre::page(7, Vec::new()))).await;
        let output = result.unwrap();
        assert_eq!(
            output.note.as_deref(),
            Some(
                "Judilibre returned no readable decision on this page although it reports 7 \
                  results; retry once, or try another page."
            )
        );
        assert!(!output.has_more);
        // Juste après la dernière page pleine : 20 résultats, page 3.
        let next = SearchJudilibreInput { page: Some(3), ..text("bail") };
        let (result, _) = run(next, Ok(judilibre::page(20, Vec::new()))).await;
        assert!(result.unwrap().note.unwrap().contains("the last page is 2 "));
    }

    #[tokio::test]
    async fn the_last_reachable_page_says_so() {
        let last = SearchJudilibreInput { page: Some(200), page_size: Some(50), ..text("bail") };
        let hits = vec![judilibre::hit(judilibre::CA_ID); 50];
        let (result, _) = run(last, Ok(judilibre::page(12_000, hits.clone()))).await;
        let output = result.unwrap();
        assert!(!output.has_more);
        assert_eq!(output.note.as_deref(), Some(REACHABLE_NOTE));
        // Exactement 10 000 résultats : rien au-delà, aucune note.
        let exact = SearchJudilibreInput { page: Some(200), page_size: Some(50), ..text("bail") };
        let (result, _) = run(exact, Ok(judilibre::page(10_000, hits.clone()))).await;
        assert_eq!(result.unwrap().note, None);
        // Une page ordinaire avant la fin n'a pas de note.
        let early = SearchJudilibreInput { page: Some(2), page_size: Some(50), ..text("bail") };
        let (result, _) = run(early, Ok(judilibre::page(12_000, hits))).await;
        let output = result.unwrap();
        assert!(output.has_more);
        assert_eq!(output.note, None);
    }

    #[tokio::test]
    async fn a_page_longer_than_asked_is_an_upstream_error() {
        let input = SearchJudilibreInput { page_size: Some(1), ..text("bail") };
        let hits = vec![judilibre::hit(judilibre::CC_ID), judilibre::hit(judilibre::CA_ID)];
        let (result, _) = run(input, Ok(judilibre::page(2, hits))).await;
        let error = result.unwrap_err();
        assert_eq!(
            (error.code(), error.cause()),
            (Some(ErrorCode::UpstreamError), Some(UpstreamCause::Decode))
        );
        // Une page pleine, à la taille demandée, passe.
        let input = SearchJudilibreInput { page_size: Some(1), ..text("bail") };
        let (result, _) =
            run(input, Ok(judilibre::page(2, vec![judilibre::hit(judilibre::CC_ID)]))).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn port_errors_are_translated_without_the_query() {
        let (result, _) = run(text("secret de famille"), Err(CaseLawError::NotSubscribed)).await;
        let error = result.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::ConfigError));
        assert!(!error.message().contains("secret"));
        // Une recherche ne reçoit jamais `NotFound` : repli sur le message de `Decode`.
        let (result, _) = run(text("bail"), Err(CaseLawError::NotFound)).await;
        assert_eq!(result.unwrap_err().message(), "unexpected response from Judilibre; report it.");
    }

    /// Résultat de la Cour de cassation au sommaire de `chars` caractères.
    fn with_summary(chars: usize) -> CaseLawHit {
        CaseLawHit { summary: Some("s".repeat(chars)), ..judilibre::hit(judilibre::CC_ID) }
    }

    #[tokio::test]
    async fn long_summaries_are_shortened_from_the_end_to_fit_the_cap() {
        let input = SearchJudilibreInput { page_size: Some(50), ..text("bail") };
        let hits = vec![with_summary(1_500); 50];
        let (result, _) = run(input, Ok(judilibre::page(50, hits))).await;
        let output = result.unwrap();
        assert_eq!(output.hits.len(), 50);
        let shortened = output
            .hits
            .iter()
            .filter(|hit| hit.summary.as_ref().is_some_and(|s| s.chars().count() == 300))
            .count();
        assert!(shortened > 0 && shortened < 50, "{shortened}");
        // Les derniers sont raccourcis, les premiers gardés entiers.
        assert_eq!(output.hits[0].summary.as_ref().unwrap().chars().count(), 1_500);
        assert!(output.hits[49].summary.as_ref().unwrap().ends_with('…'));
        assert_eq!(
            output.note.unwrap(),
            format!(
                "{shortened} long summaries were shortened to keep the response under 45,000 \
                 characters; use a smaller page_size, or fr_get_judilibre, to get them whole."
            )
        );
        let size = output.hits.iter().map(hit_cost).sum::<usize>();
        assert!(size < ResponseBudget::ESTIMATE_TARGET_CHARS, "{size}");
    }

    #[test]
    fn a_single_shortened_summary_is_named_in_the_singular() {
        assert!(summaries_note(1).starts_with("1 long summary was shortened"));
        let mut short = with_summary(300);
        assert!(!shorten_summary(&mut short));
        assert_eq!(short.summary.unwrap().chars().count(), 300);
        let mut none = judilibre::hit(judilibre::CA_ID);
        assert!(!shorten_summary(&mut none));
        let mut long = with_summary(301);
        assert!(shorten_summary(&mut long));
        assert_eq!(long.summary.unwrap().chars().count(), 300);
    }

    /// Une page juste à la cible se rend entière ; un caractère de plus raccourcit le dernier
    /// sommaire.
    #[test]
    fn a_page_exactly_at_the_target_is_kept_whole() {
        let page_with = |last: usize| {
            let mut hits = vec![with_summary(1_000); 20];
            hits.push(with_summary(last));
            SearchJudilibreOutput {
                total: 21,
                page: 1,
                has_more: false,
                hits,
                consulted_on: CivilDate::new(2026, 11, 27).unwrap(),
                note: None,
            }
        };
        let probe = page_with(400);
        let fixed = envelope_cost(&probe)
            .saturating_add(CASE_LAW_CONSTANTS_RESERVE_CHARS)
            .saturating_add(DEGRADE_NOTE_RESERVE);
        let last = ResponseBudget::ESTIMATE_TARGET_CHARS
            .saturating_add(400)
            .saturating_sub(fixed)
            .saturating_sub(page_cost(&probe.hits));
        assert!(last > 400, "{last}");
        let mut exact = page_with(last);
        fit_under_target(&mut exact, PageSize::new(50).unwrap()).unwrap();
        assert_eq!(exact.note, None);
        assert_eq!(exact.hits[20].summary.as_ref().unwrap().chars().count(), last);
        let mut over = page_with(last.saturating_add(1));
        fit_under_target(&mut over, PageSize::new(50).unwrap()).unwrap();
        assert_eq!(over.note, Some(summaries_note(1)));
        assert_eq!(over.hits[20].summary.as_ref().unwrap().chars().count(), 300);
    }

    /// `match` connaît ses trois noms, quelles que soient la casse et les espaces autour.
    #[test]
    fn the_match_mode_accepts_its_three_names_whatever_the_case() {
        assert_eq!(mode_of(None).unwrap(), MatchMode::All);
        assert_eq!(mode_of(Some(" ANY ")).unwrap(), MatchMode::Any);
        assert_eq!(mode_of(Some("All")).unwrap(), MatchMode::All);
        assert_eq!(mode_of(Some("exact")).unwrap(), MatchMode::Exact);
        assert!(mode_of(Some("some")).is_err());
    }

    #[tokio::test]
    async fn a_page_that_cannot_fit_names_the_largest_page_size() {
        // Des thèmes immenses, que le plafond ne raccourcit pas : la page est refusée.
        let huge =
            CaseLawHit { themes: vec!["t".repeat(2_000); 3], ..judilibre::hit(judilibre::CA_ID) };
        let input = SearchJudilibreInput { page_size: Some(50), ..text("bail") };
        let (result, _) = run(input, Ok(judilibre::page(50, vec![huge; 50]))).await;
        let message = result.unwrap_err().message().to_owned();
        assert!(
            message.starts_with(
                "page_size=50 would exceed the 45,000-character response cap for this query; use \
                 page_size <= "
            ),
            "{message}"
        );
    }

    #[test]
    fn the_hit_cost_is_the_compact_json_length_of_the_view() {
        let hit = judilibre::hit(judilibre::CC_ID);
        let json = format!(
            r#"{{"id":"5fca624b7eeb384a40cafb85","court":"Cour de cassation","chamber":"Première chambre civile","formation":"Formation de section","date":"2020-12-02","number":"19-20.000","ecli":"ECLI:FR:CCASS:2020:C100001","solution":"Cassation","publication":"Publié au Bulletin","summary":"Une donation entre vifs est irrévocable.","themes":["Donation"],"citation":"{}"}}"#,
            hit.mention.citation(hit.number.as_deref())
        );
        assert_eq!(hit_cost(&hit), json.chars().count());
        let appeal = judilibre::hit(judilibre::CA_ID);
        let json = format!(
            r#"{{"id":"63c8eefddc5b777c90992fd8","court":"Cour d'appel de Paris","chamber":"Pôle 3 - Chambre 1","date":"2023-01-18","number":"20/17369","solution":"Infirmation","excerpt":"la donation consentie entre époux","themes":["Donation"],"citation":"{}"}}"#,
            appeal.mention.citation(appeal.number.as_deref())
        );
        assert_eq!(hit_cost(&appeal), json.chars().count());
        // Deux thèmes : une virgule de plus que leurs textes entre guillemets.
        let themes =
            CaseLawHit { themes: vec!["a".to_owned(), "b\"".to_owned()], ..appeal.clone() };
        assert_eq!(
            hit_cost(&themes),
            hit_cost(&appeal) - r#""Donation""#.len() + r#""a","b\"""#.len()
        );
    }

    #[test]
    fn the_envelope_cost_is_the_compact_json_length_without_the_constants() {
        let mut output = SearchJudilibreOutput {
            total: 1_234,
            page: 12,
            has_more: true,
            hits: Vec::new(),
            consulted_on: CivilDate::new(2026, 11, 27).unwrap(),
            note: None,
        };
        let json =
            r#"{"total":1234,"page":12,"has_more":true,"hits":[],"consulted_on":"2026-11-27"}"#;
        assert_eq!(envelope_cost(&output), json.chars().count());
        output.has_more = false;
        output.note = Some("Note « é ».".to_owned());
        let json = r#"{"total":1234,"page":12,"has_more":false,"hits":[],"consulted_on":"2026-11-27","note":"Note « é »."}"#;
        assert_eq!(envelope_cost(&output), json.chars().count());
        let hits = vec![judilibre::hit(judilibre::CC_ID), judilibre::hit(judilibre::CA_ID)];
        assert_eq!(page_cost(&hits), hit_cost(&hits[0]) + hit_cost(&hits[1]) + 1);
        assert_eq!(page_cost(&[]), 0);
    }
}
