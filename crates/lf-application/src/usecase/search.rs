//! Cas d'usage de `fr_search` (v0.1) : la recherche plein texte (`SearchUseCase`, 05 §5.1).
//!
//! # Validation (02 §6.1, P-011)
//!
//! Sans réseau et dans l'ordre de la référence : `fond`, `query`, `page_size`, puis `page`,
//! `match`, `date_from`, `date_to` et `version_date` (leur forme), puis les combinaisons, dans
//! l'ordre du domaine : période inversée (`DateRange::new`), `version_date` hors LODA et CODE,
//! période sur CODE, page hors de la fenêtre atteignable (`SearchQuery::new`). Le premier échec
//! est rendu ; aucun appel au port ne part.
//!
//! # Dates par défaut (03 §6.4)
//!
//! `version_date` vaut le jour civil Europe/Paris de l'horloge pour LODA et CODE, jamais la date
//! UTC (D-19) : la requête du port, donc la clé de cache, change chaque jour à minuit à Paris.
//!
//! # Plafond de réponse (05 §6.4, 02 §6.1)
//!
//! La taille de la page est estimée (somme des longueurs en caractères plus 64 par champ ou
//! élément, cible 42 000, [`estimate_chars`]), puis dégradée dans cet ordre, sans jamais retirer
//! un résultat : (1) extraits des derniers résultats, avec une `note` ; (2) listes d'articles et
//! champs optionnels (`title`, `nature`, `etat`, `date`) des derniers résultats ; (3) si la page
//! ne tient toujours pas, `invalid_arg` qui propose la plus grande `page_size` qui tiendrait. La
//! mesure exacte, sur le JSON, est faite par la vue de `lf-adapter-mcp`.

use std::num::NonZeroU32;

use lf_domain::{
    contract::Citable,
    date::{DateRange, VersionDate},
    entity::{SearchHit, SearchPage, Snippet},
    fond::Fond,
    search::{MatchMode, PageSize, QueryText, SearchQuery, has_more},
    status::LegalStatus,
};

use crate::{
    budget::{ResponseBudget, estimate_chars, strip_from_end},
    error::AppError,
    failure::port_failure,
    input::SearchInput,
    messages,
    output::search::{HitOut, SearchOutput},
    ports::{Clock, LegalCorpus},
    tools::ToolName,
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_SEARCH;

/// Note de fin de fenêtre atteignable (FR-008, 05 §5.1).
const REACHABLE_NOTE: &str = "Results beyond the first 10,000 are not reachable; narrow the query \
                              with match or date_from/date_to.";

/// Longueur ISO d'une date rendue, `AAAA-MM-JJ`.
const ISO_DATE_CHARS: usize = 10;

/// Longueur maximale d'un IDCC rendu (1 à 9999).
const IDCC_CHARS: usize = 4;

/// Caractères réservés aux deux notes de dégradation (`snippets_note`, `details_note`) dans
/// l'estimation : les deux notes, avec trois chiffres de compteur, tiennent largement dedans.
const DEGRADE_NOTE_RESERVE_CHARS: usize = 400;

/// Coût estimé de la réservation des notes : le champ `note` et leur texte.
const DEGRADE_NOTE_RESERVE: usize = estimate_chars(1, DEGRADE_NOTE_RESERVE_CHARS);

/// Recherche plein texte : analyse des arguments, un appel au port, politique de réponse.
pub(crate) struct SearchUseCase<'a, C: LegalCorpus, K: Clock> {
    corpus: &'a C,
    clock: &'a K,
}

impl<'a, C: LegalCorpus, K: Clock> SearchUseCase<'a, C, K> {
    pub(crate) fn new(corpus: &'a C, clock: &'a K) -> Self {
        Self { corpus, clock }
    }

    /// Valide `input`, interroge le port et met la page en forme.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour tout argument refusé (sans appel au port) ou pour une page qui ne tient
    /// pas sous le plafond de réponse ; sinon l'erreur du port, traduite par
    /// [`port_failure`].
    pub(crate) async fn execute(&self, input: SearchInput) -> Result<SearchOutput, AppError> {
        let query = self.query_of(input)?;
        let page =
            self.corpus.search(&query).await.map_err(|error| port_failure(TOOL, "", &error))?;
        present(&query, page)
    }

    /// Requête validée du port, ou premier argument refusé.
    fn query_of(&self, input: SearchInput) -> Result<SearchQuery, AppError> {
        let SearchInput {
            query,
            fond,
            match_mode,
            page,
            page_size,
            date_from,
            date_to,
            version_date,
        } = input;
        let fond = fond_of(fond.as_deref())?;
        let text = text_of(query.as_deref())?;
        let page_size = page_size_of(page_size)?;
        let page = page_of(page)?;
        let mode = mode_of(match_mode.as_deref())?;
        let from = date_of("date_from", date_from.as_deref())?;
        let to = date_of("date_to", date_to.as_deref())?;
        let version = date_of("version_date", version_date.as_deref())?;
        let range = DateRange::new(from, to).map_err(|error| messages::domain(TOOL, &error))?;
        let version = version.or_else(|| fond.needs_version_date().then(|| self.today()));
        SearchQuery::new(fond, text, page, page_size, mode, range, version)
            .map_err(|error| messages::domain(TOOL, &error))
    }

    /// Jour civil Europe/Paris de l'horloge. Une horloge au-delà de la plage des dates de
    /// version (année 2222 ou après) est ramenée à sa borne plutôt que refusée : l'utilisateur
    /// n'a rien fourni de faux.
    fn today(&self) -> VersionDate {
        VersionDate::try_from(self.clock.today_paris()).unwrap_or(VersionDate::MAX)
    }
}

/// `fond` : `LODA` par défaut, sinon insensible à la casse et aux espaces (P-010).
fn fond_of(value: Option<&str>) -> Result<Fond, AppError> {
    value.map_or(Ok(Fond::Loda), Fond::parse).map_err(|error| messages::domain(TOOL, &error))
}

/// `query` : requise, normalisée et bornée par le domaine.
fn text_of(value: Option<&str>) -> Result<QueryText, AppError> {
    let value = value.ok_or_else(|| messages::required("query"))?;
    QueryText::parse(value).map_err(|error| messages::domain(TOOL, &error))
}

/// `page_size` : 10 par défaut, de 1 à 100 (P-011), `i64` hors de `u32` compris.
fn page_size_of(value: Option<i64>) -> Result<PageSize, AppError> {
    let Some(value) = value else { return Ok(PageSize::DEFAULT) };
    u32::try_from(value).ok().and_then(|size| PageSize::new(size).ok()).ok_or_else(|| {
        messages::out_of_range(
            "page_size",
            value,
            i64::from(PageSize::MIN),
            i64::from(PageSize::MAX),
        )
    })
}

/// `page` : 1 par défaut, au moins 1. Au-delà de `u32::MAX` la page est de toute façon hors de la
/// fenêtre atteignable : elle est saturée, et `SearchQuery::new` la refuse (le message cite alors
/// la page saturée).
fn page_of(value: Option<i64>) -> Result<NonZeroU32, AppError> {
    let Some(value) = value else { return Ok(NonZeroU32::MIN) };
    if value < 1 {
        return Err(messages::out_of_range("page", value, 1, i64::MAX));
    }
    Ok(u32::try_from(value).ok().and_then(NonZeroU32::new).unwrap_or(NonZeroU32::MAX))
}

/// `match` : `any` par défaut (Q10), insensible à la casse et aux espaces.
fn mode_of(value: Option<&str>) -> Result<MatchMode, AppError> {
    let Some(value) = value else { return Ok(MatchMode::Any) };
    match value.trim().to_ascii_lowercase().as_str() {
        "any" => Ok(MatchMode::Any),
        "all" => Ok(MatchMode::All),
        "exact" => Ok(MatchMode::Exact),
        _ => Err(messages::not_one_of("match", value, &["all", "any", "exact"])),
    }
}

/// Date `AAAA-MM-JJ` de l'argument `name` (D-22), sans rognage.
fn date_of(name: &'static str, value: Option<&str>) -> Result<Option<VersionDate>, AppError> {
    value
        .map(|raw| {
            VersionDate::parse(raw).map_err(|error| messages::domain_arg(TOOL, Some(name), &error))
        })
        .transpose()
}

/// Met la page du port en forme de réponse : `has_more`, note de fin de fenêtre, contrat de
/// citation de chaque résultat, puis plafond de réponse.
fn present(query: &SearchQuery, page: SearchPage) -> Result<SearchOutput, AppError> {
    let (number, size) = (query.page(), query.page_size());
    let more = has_more(number, size, page.total);
    // Il reste des résultats après cette page, mais la fenêtre atteignable s'arrête ici.
    let seen = u64::from(number.get()) * u64::from(size.get());
    let note = (!more && seen < page.total).then(|| REACHABLE_NOTE.to_owned());
    let mut output = SearchOutput {
        total: page.total,
        page: number.get(),
        has_more: more,
        total_articles: page.total_articles,
        hits: page.hits.into_iter().map(hit_out).collect(),
        note,
    };
    fit_under_target(&mut output, size)?;
    Ok(output)
}

/// Le résultat et son contrat de citation, calculé avant toute dégradation.
fn hit_out(hit: SearchHit) -> HitOut {
    let contract = hit.contract();
    HitOut {
        citation: contract.citation.as_str().to_owned(),
        source_url: contract.source_url.as_str().to_owned(),
        hit,
    }
}

// -------------------------------------------------------------------------------------------
// Plafond de réponse
// -------------------------------------------------------------------------------------------

/// Dégrade `output` jusqu'à ce que son estimation passe sous la cible (05 §6.4), ou refuse la page.
fn fit_under_target(output: &mut SearchOutput, requested: PageSize) -> Result<(), AppError> {
    let fixed = envelope_cost(output) + DEGRADE_NOTE_RESERVE;
    let over = |hits: &[HitOut]| fixed + page_cost(hits) > ResponseBudget::ESTIMATE_TARGET_CHARS;
    let snippets = strip_from_end(&mut output.hits, over, strip_snippets);
    let details = strip_from_end(&mut output.hits, over, strip_details);
    if over(&output.hits) {
        let largest = largest_page_size(&output.hits, fixed);
        return Err(messages::invalid_argument(format!(
            "page_size={} would exceed the 45,000-character response cap for this query; use \
             page_size <= {largest}.",
            requested.get()
        )));
    }
    let notes = [
        output.note.take(),
        (snippets > 0).then(|| snippets_note(snippets)),
        (details > 0).then(|| details_note(details)),
    ];
    let notes: Vec<String> = notes.into_iter().flatten().collect();
    output.note = (!notes.is_empty()).then(|| notes.join(" "));
    Ok(())
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
        "Snippets were removed from the last {hits} hits to keep the response under 45,000 \
         characters; use a smaller page_size to get them."
    )
}

/// Note des listes d'articles et champs optionnels retirés.
fn details_note(hits: usize) -> String {
    format!(
        "Article lists and the optional fields title, nature, etat and date were removed from the \
         last {hits} hits to keep the response under 45,000 characters; use a smaller page_size \
         to get them."
    )
}

/// Plus grande `page_size` dont les premiers résultats, ainsi dégradés, tiennent sous la cible :
/// déterministe, donc rejouable. Au moins 1, pour ne jamais proposer une page vide.
fn largest_page_size(hits: &[HitOut], fixed: usize) -> usize {
    hits.iter()
        .scan(fixed, |used, out| {
            *used = used.saturating_add(hit_cost(out));
            Some(*used)
        })
        .take_while(|&used| used <= ResponseBudget::ESTIMATE_TARGET_CHARS)
        .count()
        .max(1)
}

/// Coût estimé de l'enveloppe : `total`, `page`, `has_more`, `hits`, plus `total_articles` et
/// `note` quand ils sont présents.
fn envelope_cost(output: &SearchOutput) -> usize {
    let optional =
        usize::from(output.total_articles.is_some()) + usize::from(output.note.is_some());
    estimate_chars(4 + optional, output.note.as_deref().map_or(0, chars))
}

/// Coût estimé des résultats.
fn page_cost(hits: &[HitOut]) -> usize {
    hits.iter().map(hit_cost).fold(0, usize::saturating_add)
}

/// Coût estimé d'un résultat : 64 par champ ou élément, plus la longueur de ses textes. `kind`,
/// `id`, `citation` et `source_url` sont toujours rendus ; le reste seulement s'il est présent.
fn hit_cost(out: &HitOut) -> usize {
    let hit = &out.hit;
    let mut fields = 4;
    let mut text = hit.fond.name().len()
        + hit.id.as_str().len()
        + chars(&out.citation)
        + chars(&out.source_url);
    let strings = [
        hit.title.as_deref(),
        hit.nature.as_deref(),
        hit.etat.as_ref().map(LegalStatus::as_api_code),
        hit.snippet.as_ref().map(Snippet::as_str),
    ];
    for value in strings.into_iter().flatten() {
        fields += 1;
        text += chars(value);
    }
    if hit.date.is_some() {
        fields += 1;
        text += ISO_DATE_CHARS;
    }
    if hit.idcc.is_some() {
        fields += 1;
        text += IDCC_CHARS;
    }
    if !hit.articles.is_empty() {
        fields += 1;
    }
    for article in &hit.articles {
        fields += 2;
        text += article.article_id.as_str().len();
        for value in [article.num.as_deref(), article.snippet.as_ref().map(Snippet::as_str)]
            .into_iter()
            .flatten()
        {
            fields += 1;
            text += chars(value);
        }
    }
    estimate_chars(fields, text)
}

/// Longueur en caractères (points de code) : l'unité du plafond.
fn chars(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use chrono::{DateTime, TimeZone, Utc};
    use lf_domain::{
        contract::{Citable, Idcc},
        date::{CivilDate, VersionDate},
        entity::{CodeArticleRef, SearchHit, SearchPage, Snippet},
        fond::{Fond, PaginationMode},
        id::{ArticleId, DocumentId},
        search::{MatchMode, PageSize},
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        budget::{ResponseBudget, estimate_chars},
        error::ErrorCode,
        ports::CorpusError,
        testing::{FakeCorpus, FixedClock, PortRequest},
    };

    const REACHABLE_NOTE: &str = "Results beyond the first 10,000 are not reachable; narrow the \
                                  query with match or date_from/date_to.";

    fn day(year: u16, month: u8, daynum: u8) -> CivilDate {
        CivilDate::new(year, month, daynum).unwrap()
    }

    fn clock() -> FixedClock {
        FixedClock::new(DateTime::UNIX_EPOCH, day(2026, 10, 29))
    }

    fn text(query: &str) -> SearchInput {
        SearchInput { query: Some(query.to_owned()), ..SearchInput::default() }
    }

    fn doc(id: &str) -> DocumentId {
        DocumentId::parse(id).unwrap()
    }

    /// Résultat vide du fond, d'identifiant bâti sur `n`.
    fn bare(fond: Fond, n: u32) -> SearchHit {
        SearchHit::new(fond, doc(&format!("{}{n:012}", fond.hit_prefix())), None).unwrap()
    }

    /// Résultat JURI titré, sans extrait.
    fn titled(n: u32, title: &str) -> SearchHit {
        SearchHit { title: Some(title.to_owned()), ..bare(Fond::Juri, n) }
    }

    fn page_of(fond: Fond, total: u64, hits: Vec<SearchHit>) -> SearchPage {
        SearchPage::new(fond, "bail", Some(total), None, hits, 0, PaginationMode::Defaut)
    }

    /// Un corpus qui servira `page`.
    fn serving(page: SearchPage) -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_search(Ok(page));
        corpus
    }

    async fn run(
        corpus: &FakeCorpus,
        clock: &FixedClock,
        input: SearchInput,
    ) -> Result<SearchOutput, AppError> {
        SearchUseCase::new(corpus, clock).execute(input).await
    }

    /// Message d'un `invalid_arg`, après avoir vérifié qu'aucune requête n'est partie.
    async fn rejected(input: SearchInput) -> String {
        let corpus = FakeCorpus::new();
        let error = run(&corpus, &clock(), input).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(corpus.calls(), 0, "un rejet local n'appelle pas le port");
        error.message().to_owned()
    }

    /// La requête que le port a reçue.
    fn sent(corpus: &FakeCorpus) -> lf_domain::search::SearchQuery {
        let query = match corpus.seen_requests().remove(0) {
            PortRequest::Search(query) => Some(query),
            _ => None,
        };
        query.expect("the only request is a search")
    }

    const NOT_A_DATE: &str = "is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15).";

    #[tokio::test]
    async fn search_validation_order_matches_reference() {
        let mut input = SearchInput {
            query: Some(String::new()),
            fond: Some("XYZ".to_owned()),
            match_mode: Some("or".to_owned()),
            page: Some(0),
            page_size: Some(0),
            date_from: Some("2016-13-45".to_owned()),
            date_to: Some("nope".to_owned()),
            version_date: Some("later".to_owned()),
        };
        // Chaque étape corrige l'argument qui vient d'être refusé : le suivant parle alors.
        assert_eq!(
            rejected(input.clone()).await,
            "fond='XYZ' must be one of ['ACCO', 'CETAT', 'CNIL', 'CODE', 'CONSTIT', 'JURI', \
             'KALI', 'LODA']."
        );
        input.fond = None;
        assert_eq!(rejected(input.clone()).await, "query is required.");
        input.query = Some("bail".to_owned());
        assert_eq!(rejected(input.clone()).await, "page_size=0 out of range (1..100).");
        input.page_size = Some(10);
        assert_eq!(rejected(input.clone()).await, "page=0 out of range (1..).");
        input.page = Some(1);
        assert_eq!(
            rejected(input.clone()).await,
            "match='or' must be one of ['all', 'any', 'exact']."
        );
        input.match_mode = None;
        assert_eq!(rejected(input.clone()).await, format!("date_from='2016-13-45' {NOT_A_DATE}"));
        input.date_from = Some("2016-01-01".to_owned());
        assert_eq!(rejected(input.clone()).await, format!("date_to='nope' {NOT_A_DATE}"));
        input.date_to = Some("2016-12-31".to_owned());
        assert_eq!(rejected(input.clone()).await, format!("version_date='later' {NOT_A_DATE}"));
        input.version_date = Some("2016-06-01".to_owned());

        // Tout est corrigé : la requête part.
        let corpus = serving(page_of(Fond::Loda, 0, vec![]));
        run(&corpus, &clock(), input).await.unwrap();
        assert_eq!(corpus.calls(), 1);
    }

    #[tokio::test]
    async fn an_invalid_fond_is_reported_before_everything_else_even_when_blank() {
        for fond in ["", " ", "loda!", "JORF"] {
            let input = SearchInput { fond: Some(fond.to_owned()), ..SearchInput::default() };
            assert!(rejected(input).await.starts_with(&format!("fond='{fond}' must be one of [")));
        }
    }

    #[tokio::test]
    async fn query_errors_follow_the_domain_messages() {
        for blank in ["", "   ", "\u{1}\t\n"] {
            assert_eq!(rejected(text(blank)).await, "query is required.");
        }
        assert_eq!(
            rejected(text(&"a".repeat(612))).await,
            "query is too long (612 characters, max 450); shorten it or use match=\"all\"."
        );
    }

    #[tokio::test]
    async fn page_size_and_page_must_be_in_range() {
        for bad in [0, -1, 101, i64::from(u32::MAX) + 1, i64::MIN, i64::MAX] {
            let input = SearchInput { page_size: Some(bad), ..text("bail") };
            assert_eq!(rejected(input).await, format!("page_size={bad} out of range (1..100)."));
        }
        for bad in [0, -1, i64::MIN] {
            let input = SearchInput { page: Some(bad), ..text("bail") };
            assert_eq!(rejected(input).await, format!("page={bad} out of range (1..)."));
        }
    }

    #[tokio::test]
    async fn page_beyond_reachable_window_is_invalid_arg_without_network() {
        let beyond = "goes beyond the 10000 reachable results; narrow the query with match or \
                      date_from/date_to.";
        for (page, page_size) in [(200, 100), (101, 100), (1001, 10), (3334, 3)] {
            let input =
                SearchInput { page: Some(page), page_size: Some(page_size), ..text("bail") };
            assert_eq!(
                rejected(input).await,
                format!("page={page} with page_size={page_size} {beyond}")
            );
        }
        // Une page démesurée est saturée à `u32::MAX` : elle reste refusée, sans panique.
        let input = SearchInput { page: Some(i64::MAX), ..text("bail") };
        assert_eq!(rejected(input).await, format!("page=4294967295 with page_size=10 {beyond}"));

        // La dernière page atteignable part, elle.
        let corpus = serving(page_of(Fond::Loda, 50_000, vec![]));
        let input = SearchInput { page: Some(100), page_size: Some(100), ..text("bail") };
        run(&corpus, &clock(), input).await.unwrap();
        assert_eq!(corpus.calls(), 1);
    }

    #[tokio::test]
    async fn invalid_date_is_rejected_locally() {
        let bad = ["2016-13-45", "07/10/2016", "", " 2016-10-07", "2999-01-01", "1499-12-31"];
        for value in bad {
            let dates = [
                ("date_from", SearchInput { date_from: Some(value.to_owned()), ..text("bail") }),
                ("date_to", SearchInput { date_to: Some(value.to_owned()), ..text("bail") }),
                (
                    "version_date",
                    SearchInput { version_date: Some(value.to_owned()), ..text("bail") },
                ),
            ];
            for (name, input) in dates {
                assert_eq!(rejected(input).await, format!("{name}='{value}' {NOT_A_DATE}"));
            }
        }
    }

    #[tokio::test]
    async fn version_date_rejected_outside_loda_code() {
        let message = "version_date only applies to fond LODA or CODE; remove it, or use \
                       date_from/date_to.";
        for fond in [Fond::Juri, Fond::Constit, Fond::Cetat, Fond::Cnil, Fond::Kali, Fond::Acco] {
            let input = SearchInput {
                fond: Some(fond.name().to_owned()),
                version_date: Some("2026-01-01".to_owned()),
                ..text("bail")
            };
            assert_eq!(rejected(input).await, message, "{fond}");
        }
        // LODA et CODE la consomment.
        for fond in [Fond::Loda, Fond::Code] {
            let corpus = serving(page_of(fond, 0, vec![]));
            let input = SearchInput {
                fond: Some(fond.name().to_owned()),
                version_date: Some("2026-01-01".to_owned()),
                ..text("bail")
            };
            run(&corpus, &clock(), input).await.unwrap();
            assert_eq!(
                sent(&corpus).version_date(),
                Some(VersionDate::parse("2026-01-01").unwrap())
            );
        }
    }

    #[tokio::test]
    async fn date_filters_are_rejected_on_code_and_checked_for_order() {
        for (from, to) in [(Some("2020-01-01"), None), (None, Some("2020-01-01"))] {
            let input = SearchInput {
                fond: Some("code".to_owned()),
                date_from: from.map(str::to_owned),
                date_to: to.map(str::to_owned),
                ..text("bail")
            };
            assert_eq!(
                rejected(input).await,
                "date_from/date_to are not supported for fond=CODE; use version_date."
            );
        }
        let inverted = SearchInput {
            date_from: Some("2026-01-01".to_owned()),
            date_to: Some("2025-01-01".to_owned()),
            ..text("bail")
        };
        assert_eq!(rejected(inverted).await, "date_from=2026-01-01 is after date_to=2025-01-01.");
        // L'ordre des bornes se juge avant la compatibilité avec le fond.
        let both = SearchInput {
            fond: Some("JURI".to_owned()),
            version_date: Some("2026-01-01".to_owned()),
            date_from: Some("2026-01-01".to_owned()),
            date_to: Some("2025-01-01".to_owned()),
            ..text("bail")
        };
        assert_eq!(rejected(both).await, "date_from=2026-01-01 is after date_to=2025-01-01.");
    }

    #[tokio::test]
    async fn a_single_date_bound_is_closed_by_the_domain() {
        let corpus = serving(page_of(Fond::Juri, 0, vec![]));
        let input = SearchInput {
            fond: Some("JURI".to_owned()),
            date_from: Some("2020-01-01".to_owned()),
            ..text("bail")
        };
        run(&corpus, &clock(), input).await.unwrap();
        let range = sent(&corpus).date_range().unwrap();
        assert_eq!(range.start().to_string(), "2020-01-01");
        assert_eq!(range.end(), VersionDate::MAX);
    }

    #[tokio::test]
    async fn defaults_reach_the_port() {
        let corpus = serving(page_of(Fond::Loda, 0, vec![]));
        run(&corpus, &clock(), text("  droit \t de grève ")).await.unwrap();
        let query = sent(&corpus);
        assert_eq!(query.fond(), Fond::Loda);
        assert_eq!(query.text().as_str(), "droit de grève");
        assert_eq!(query.page(), NonZeroU32::MIN);
        assert_eq!(query.page_size(), PageSize::DEFAULT);
        assert_eq!(query.mode(), MatchMode::Any);
        assert_eq!(query.date_range(), None);
        assert_eq!(query.version_date(), Some(VersionDate::parse("2026-10-29").unwrap()));
    }

    #[tokio::test]
    async fn fond_and_match_are_read_leniently() {
        for (raw, mode) in
            [("any", MatchMode::Any), ("ALL", MatchMode::All), (" Exact ", MatchMode::Exact)]
        {
            let corpus = serving(page_of(Fond::Juri, 0, vec![]));
            let input = SearchInput {
                fond: Some(" juri ".to_owned()),
                match_mode: Some(raw.to_owned()),
                page: Some(3),
                page_size: Some(25),
                ..text("bail")
            };
            run(&corpus, &clock(), input).await.unwrap();
            let query = sent(&corpus);
            assert_eq!(query.fond(), Fond::Juri);
            assert_eq!(query.mode(), mode);
            assert_eq!(query.page().get(), 3);
            assert_eq!(query.page_size().get(), 25);
            assert_eq!(query.version_date(), None, "pas de version_date hors LODA et CODE");
        }
    }

    #[tokio::test]
    async fn port_query_version_date_follows_paris_day() {
        // 22 h 30 UTC un jour d'été : il est déjà le lendemain à Paris.
        let late = Utc.with_ymd_and_hms(2026, 7, 14, 22, 30, 0).unwrap();
        let paris = FixedClock::new(late, day(2026, 7, 15));
        for fond in ["LODA", "CODE"] {
            let corpus = serving(page_of(Fond::Loda, 0, vec![]));
            let input = SearchInput { fond: Some(fond.to_owned()), ..text("bail") };
            run(&corpus, &paris, input).await.unwrap();
            let sent_on = sent(&corpus).version_date().unwrap().to_string();
            assert_eq!(sent_on, "2026-07-15", "{fond}");
        }
        // Le lendemain, la requête change : c'est ce qui fait tourner la clé de cache chaque jour.
        let corpus = serving(page_of(Fond::Loda, 0, vec![]));
        let next = FixedClock::new(late, day(2026, 7, 16));
        run(&corpus, &next, text("bail")).await.unwrap();
        assert_eq!(sent(&corpus).version_date().unwrap().to_string(), "2026-07-16");
        // Une date explicite l'emporte sur l'horloge.
        let corpus = serving(page_of(Fond::Loda, 0, vec![]));
        let input = SearchInput { version_date: Some("2020-02-29".to_owned()), ..text("bail") };
        run(&corpus, &paris, input).await.unwrap();
        assert_eq!(sent(&corpus).version_date().unwrap().to_string(), "2020-02-29");
    }

    #[tokio::test]
    async fn a_clock_past_the_version_range_is_clamped_not_rejected() {
        let corpus = serving(page_of(Fond::Loda, 0, vec![]));
        let far = FixedClock::new(DateTime::UNIX_EPOCH, day(2300, 1, 1));
        run(&corpus, &far, text("bail")).await.unwrap();
        assert_eq!(sent(&corpus).version_date(), Some(VersionDate::MAX));
    }

    /// Réponse à `total` résultats servie à la page `page` de taille `size`.
    async fn paged(page: i64, size: i64, total: u64) -> SearchOutput {
        let corpus = serving(page_of(Fond::Loda, total, vec![bare(Fond::Loda, 1)]));
        let input = SearchInput { page: Some(page), page_size: Some(size), ..text("bail") };
        run(&corpus, &clock(), input).await.unwrap()
    }

    #[tokio::test]
    async fn has_more_false_at_reachable_end_with_note() {
        // Fin de la fenêtre atteignable : 333 x 30 = 9 990, la page 334 sortirait des 10 000.
        let end = paged(333, 30, 50_000).await;
        assert!(!end.has_more);
        assert_eq!(end.note.as_deref(), Some(REACHABLE_NOTE));
        // Pile 10 000, et il en reste : même fin de fenêtre.
        let exact = paged(100, 100, 10_001).await;
        assert!(!exact.has_more);
        assert_eq!(exact.note.as_deref(), Some(REACHABLE_NOTE));
        // Une page avant la fin de la fenêtre : la suite existe et se lit.
        let before = paged(99, 100, 50_000).await;
        assert!(before.has_more);
        assert_eq!(before.note, None);
        // Dernière page naturelle : rien après, donc ni suite ni note.
        let last = paged(1, 10, 10).await;
        assert!(!last.has_more);
        assert_eq!(last.note, None);
        let short = paged(1, 10, 11).await;
        assert!(short.has_more);
        // Au-delà des résultats réels : ni suite ni note.
        let past = paged(5, 10, 30).await;
        assert!(!past.has_more);
        assert_eq!(past.note, None);
        assert_eq!(end.page, 333);
        assert_eq!(end.total, 50_000);
    }

    #[tokio::test]
    async fn an_empty_search_is_a_success() {
        let corpus = serving(page_of(Fond::Loda, 0, vec![]));
        let output = run(&corpus, &clock(), text("zzzz")).await.unwrap();
        assert_eq!(
            output,
            SearchOutput {
                total: 0,
                page: 1,
                has_more: false,
                total_articles: None,
                hits: vec![],
                note: None
            }
        );
        assert_eq!(output.output_count_or_size(), 0);
    }

    #[tokio::test]
    async fn hits_keep_the_order_of_the_api_and_carry_their_citation_contract() {
        let first = titled(1, "Cour de cassation, chambre civile 1, 2024");
        let second = SearchHit {
            snippet: Snippet::new("[...] bail [...]"),
            etat: Some(LegalStatus::Vigueur),
            idcc: Idcc::parse("275"),
            ..bare(Fond::Juri, 2)
        };
        let page = SearchPage::new(
            Fond::Juri,
            "bail",
            Some(57),
            Some(12),
            vec![first.clone(), second.clone()],
            1,
            PaginationMode::Article,
        );
        let corpus = serving(page);
        let input = SearchInput { fond: Some("JURI".to_owned()), ..text("bail") };
        let output = run(&corpus, &clock(), input).await.unwrap();

        assert_eq!((output.total, output.total_articles, output.page), (57, Some(12), 1));
        assert_eq!(output.output_count_or_size(), 2);
        let ids: Vec<_> = output.hits.iter().map(|h| h.hit.id.as_str().to_owned()).collect();
        assert_eq!(ids, ["JURITEXT000000000001", "JURITEXT000000000002"]);
        for (out, expected) in output.hits.iter().zip([first, second]) {
            let contract = expected.contract();
            assert_eq!(out.citation, contract.citation.as_str());
            assert_eq!(out.source_url, contract.source_url.as_str());
            assert_eq!(out.hit, expected, "un résultat sous le plafond n'est pas modifié");
        }
        assert_eq!(output.hits[0].citation, "Cour de cassation, chambre civile 1, 2024");
        assert_eq!(output.hits[1].citation, "Légifrance JURITEXT000000000002");
        assert_eq!(output.note, None);
    }

    #[tokio::test]
    async fn port_failures_are_translated_by_the_message_table() {
        let cases = [
            (
                CorpusError::Timeout,
                ErrorCode::UpstreamError,
                "Legifrance/PISTE did not answer in time; retry once, then report it.",
            ),
            (
                CorpusError::NotFound { what: "/search".to_owned() },
                ErrorCode::UpstreamError,
                "unexpected response from Legifrance; report it.",
            ),
            (
                CorpusError::RateLimited { retry_after: None },
                ErrorCode::RateLimited,
                "PISTE quota reached; retry in a few seconds.",
            ),
        ];
        for (failure, code, message) in cases {
            let corpus = FakeCorpus::new();
            corpus.push_search(Err(failure));
            let error = run(&corpus, &clock(), text("bail")).await.unwrap_err();
            assert_eq!(error.code(), Some(code));
            assert_eq!(error.message(), message);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Plafond de réponse (05 §6.4)
    // ---------------------------------------------------------------------------------------

    const TARGET: usize = ResponseBudget::ESTIMATE_TARGET_CHARS;

    fn snippet(n: usize) -> Snippet {
        Snippet::new(&format!("[...] extrait {n} {}", "mot ".repeat(60))).unwrap()
    }

    /// `count` résultats LODA à extrait de 280 caractères et à titre court.
    fn with_snippets(count: u32) -> Vec<SearchHit> {
        (1..=count)
            .map(|n| SearchHit {
                title: Some(format!("Loi n° {n}")),
                snippet: Some(snippet(n as usize)),
                ..bare(Fond::Loda, n)
            })
            .collect()
    }

    async fn big_page(
        hits: Vec<SearchHit>,
        fond: Fond,
        size: i64,
    ) -> Result<SearchOutput, AppError> {
        let corpus = serving(page_of(fond, 5_000, hits));
        let input = SearchInput {
            fond: Some(fond.name().to_owned()),
            page_size: Some(size),
            ..text("bail")
        };
        run(&corpus, &clock(), input).await
    }

    #[test]
    fn hit_cost_counts_fields_and_characters() {
        let hit = |hit| {
            let contract = Citable::contract(&hit);
            HitOut {
                citation: contract.citation.as_str().to_owned(),
                source_url: contract.source_url.as_str().to_owned(),
                hit,
            }
        };
        // Nu : kind (4), id (20), citation « Légifrance JURITEXT… » (31), URL (59), 4 champs.
        let bare_hit = hit(bare(Fond::Juri, 7));
        let fixed =
            "juri".len() + 20 + bare_hit.citation.chars().count() + bare_hit.source_url.len();
        assert_eq!(fixed, 4 + 20 + 31 + 59);
        assert_eq!(hit_cost(&bare_hit), estimate_chars(4, fixed));

        // Tous les champs optionnels, un snippet de 6 caractères et deux articles.
        let full = hit(SearchHit {
            title: Some("Titre".to_owned()),
            date: CivilDate::new(2024, 1, 2),
            nature: Some("ARRET".to_owned()),
            etat: Some(LegalStatus::Abroge),
            snippet: Snippet::new("abcdef"),
            idcc: Idcc::parse("275"),
            articles: vec![
                CodeArticleRef::new(
                    ArticleId::parse("LEGIARTI000006419288").unwrap(),
                    Some("9".to_owned()),
                    "xyz",
                ),
                CodeArticleRef::new(ArticleId::parse("LEGIARTI000006419289").unwrap(), None, ""),
            ],
            ..bare(Fond::Juri, 7)
        });
        // La citation d'un résultat titré est son titre : elle est comptée à part, en plus de `title`.
        let titled_fixed = "juri".len() + 20 + "Titre".len() + 59;
        let optional_text = "Titre".len() + 10 + "ARRET".len() + "ABROGE".len() + 6 + 4;
        let articles_text = (20 + 1 + 3) + 20;
        // 4 champs fixes, 6 optionnels, la liste (1), 2 éléments et leurs identifiants (4), `num`
        // et `snippet` du premier (2).
        assert_eq!(
            hit_cost(&full),
            estimate_chars(4 + 6 + 1 + 4 + 2, titled_fixed + optional_text + articles_text)
        );
    }

    #[test]
    fn degrade_notes_fit_in_their_reserve() {
        let both = format!("{} {}", snippets_note(100), details_note(100));
        assert!(both.chars().count() <= DEGRADE_NOTE_RESERVE_CHARS, "{both}");
        assert_eq!(
            snippets_note(7),
            "Snippets were removed from the last 7 hits to keep the response under 45,000 \
             characters; use a smaller page_size to get them."
        );
    }

    #[tokio::test]
    async fn snippets_are_removed_from_the_last_hits_when_over_the_estimate() {
        let output = big_page(with_snippets(90), Fond::Loda, 90).await.unwrap();
        // Aucun résultat n'est retiré.
        assert_eq!(output.hits.len(), 90);
        let removed = output.hits.iter().filter(|h| h.hit.snippet.is_none()).count();
        assert!(removed > 0 && removed < 90, "{removed}");
        // Ils sont retirés en partant du dernier, sans trou.
        let first_removed = output.hits.len() - removed;
        assert!(output.hits[..first_removed].iter().all(|h| h.hit.snippet.is_some()));
        assert!(output.hits[first_removed..].iter().all(|h| h.hit.snippet.is_none()));
        assert_eq!(output.note, Some(snippets_note(removed)));
        // Le titre, la citation et les champs d'identité restent.
        assert!(output.hits.iter().all(|h| h.hit.title.is_some() && !h.citation.is_empty()));
        // L'estimation passe sous la cible, et rendre son extrait au premier résultat dégradé la
        // ferait dépasser : on n'en retire pas plus que nécessaire.
        let fixed = estimate_chars(4, 0) + DEGRADE_NOTE_RESERVE;
        assert!(fixed + page_cost(&output.hits) <= TARGET);
        let mut restored = output.hits.clone();
        restored[first_removed].hit.snippet = with_snippets(90)[first_removed].snippet.clone();
        assert!(fixed + page_cost(&restored) > TARGET, "retrait minimal");
    }

    #[tokio::test]
    async fn nothing_is_removed_when_the_page_fits() {
        let output = big_page(with_snippets(20), Fond::Loda, 20).await.unwrap();
        assert!(output.hits.iter().all(|h| h.hit.snippet.is_some()));
        assert_eq!(output.note, None);
    }

    #[tokio::test]
    async fn article_lists_and_optional_fields_go_after_the_snippets() {
        // 40 résultats CODE : titre de 400 caractères, trois articles à extrait.
        let long = "mot ".repeat(100);
        let hits: Vec<SearchHit> = (1..=40)
            .map(|n| SearchHit {
                title: Some(long.clone()),
                nature: Some("CODE".to_owned()),
                etat: Some(LegalStatus::Vigueur),
                date: CivilDate::new(2024, 1, 2),
                snippet: Snippet::new(&"abc ".repeat(70)),
                articles: (0..3)
                    .map(|a| {
                        CodeArticleRef::new(
                            ArticleId::parse(&format!("LEGIARTI{:012}", n * 10 + a)).unwrap(),
                            Some(a.to_string()),
                            &"def ".repeat(70),
                        )
                    })
                    .collect(),
                ..bare(Fond::Code, n)
            })
            .collect();
        let output = big_page(hits, Fond::Code, 40).await.unwrap();

        assert_eq!(output.hits.len(), 40);
        let stripped = output.hits.iter().filter(|h| h.hit.snippet.is_none()).count();
        let trimmed = output.hits.iter().filter(|h| h.hit.articles.is_empty()).count();
        assert_eq!(stripped, 40, "tous les extraits sont partis avant qu'on touche aux champs");
        assert!(trimmed > 0 && trimmed < 40, "{trimmed}");
        // Retraits en partant de la fin, sans trou.
        let first = 40 - trimmed;
        assert!(output.hits[..first].iter().all(|h| h.hit.title.is_some()));
        assert!(output.hits[first..].iter().all(|h| {
            h.hit.title.is_none()
                && h.hit.nature.is_none()
                && h.hit.etat.is_none()
                && h.hit.date.is_none()
        }));
        // La citation, calculée avant, survit au retrait du titre.
        assert!(output.hits[39].citation.starts_with("mot mot"));
        assert_eq!(output.note, Some(format!("{} {}", snippets_note(40), details_note(trimmed))));
    }

    #[tokio::test]
    async fn a_page_that_cannot_fit_even_stripped_is_invalid_arg_with_a_usable_page_size() {
        // Citation de 899 caractères : un résultat nu coûte 1 245 (4 champs, 4 + 20 + 899 + 59
        // caractères), et l'enveloppe plus la réserve des notes 720 : (42 000 - 720) / 1 245 = 33.
        let long = format!("{}fin", "mot ".repeat(224));
        let hits: Vec<SearchHit> = (1..=100)
            .map(|n| SearchHit { snippet: Snippet::new("extrait"), ..titled(n, &long) })
            .collect();
        let error = big_page(hits.clone(), Fond::Juri, 100).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(
            error.message(),
            "page_size=100 would exceed the 45,000-character response cap for this query; use \
             page_size <= 33."
        );
        // Règle rejouable : avec cette taille de page, la réponse tient.
        let replay = big_page(hits[..33].to_vec(), Fond::Juri, 33).await.unwrap();
        assert_eq!(replay.hits.len(), 33);
        // Et la taille suivante ne tient plus.
        let error = big_page(hits[..34].to_vec(), Fond::Juri, 34).await.unwrap_err();
        assert!(error.message().ends_with("use page_size <= 33."), "{}", error.message());
    }

    #[tokio::test]
    async fn one_hit_is_always_proposed_as_the_last_resort_page_size() {
        // Un seul résultat démesuré (hors bornes réelles) : la taille proposée ne tombe pas à 0.
        let huge = SearchHit { title: Some("x".repeat(60_000)), ..bare(Fond::Juri, 1) };
        let hit = HitOut { citation: "c".to_owned(), source_url: "u".to_owned(), hit: huge };
        assert_eq!(largest_page_size(&[hit], 0), 1);
    }

    #[tokio::test]
    async fn the_audit_metric_counts_returned_hits() {
        let corpus =
            serving(page_of(Fond::Juri, 9_999, vec![bare(Fond::Juri, 1), bare(Fond::Juri, 2)]));
        let input = SearchInput { fond: Some("JURI".to_owned()), ..text("bail") };
        let output = run(&corpus, &clock(), input).await.unwrap();
        assert_eq!(output.output_count_or_size(), 2);
    }
}
