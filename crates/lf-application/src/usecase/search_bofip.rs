//! Cas d'usage de `fr_search_bofip` (jalon MB) : la recherche dans le BOFiP
//! (`SearchBofipUseCase`, 05 §5.14).
//!
//! # Validation
//!
//! Sans réseau et dans l'ordre de la référence : `query` (requise, puis réduite à ses mots :
//! une requête sans lettre ni chiffre est refusée ici), `serie`, `kind`, `page_size`, `page`, puis
//! la fenêtre atteignable du portail (`DoctrineQuery::new` : `page × page_size` ≤ 10 000). Le
//! premier échec est rendu ; aucun appel au port ne part.
//!
//! # Mots exacts, puis recherche approchée en secours (ADR-0031)
//!
//! Un premier appel au port demande les mots exacts. S'il ne trouve **aucune** publication
//! (total nul), un second appel demande la même requête, mots approchants admis
//! ([`DoctrineQuery::approximate`] : mêmes mots, mêmes filtres, même page), et la réponse le dit
//! dans `note`. Jamais autrement : une page vide au-delà de la fin d'une recherche qui a des
//! résultats exacts ne déclenche rien, et une erreur du premier appel est rendue telle quelle.
//! Le second appel n'est pas un réessai : c'est une autre question, posée après une réponse.
//!
//! # Réponse du port
//!
//! Une page qui porte plus de résultats que `page_size` n'est pas conforme : `upstream_error`,
//! quel que soit l'adapter derrière le port (05 §5.14).
//!
//! # Plafond de réponse (05 §6.4)
//!
//! La taille de la page est calculée au caractère près, comme le JSON compact de la vue de
//! `lf-adapter-mcp` l'écrira, plus une réserve pour ses deux champs constants
//! ([`BOFIP_CONSTANTS_RESERVE_CHARS`]), puis comparée à la cible de 42 000 caractères. Un titre
//! peut faire 1 000 caractères : la page est dégradée sans jamais retirer un résultat, en
//! raccourcissant les titres des derniers résultats à 120 caractères (`TITLE_SHORT_CHARS`), avec une
//! `note` ; si elle ne tient toujours pas, `invalid_arg` propose la plus grande `page_size` qui
//! tiendrait.

use lf_domain::{
    doctrine::{
        BofipUrl, DoctrineHit, DoctrineMatch, DoctrinePage, DoctrineQuery, DoctrineTerms,
        KindFilter, Serie,
    },
    search::{PageSize, has_more},
};

use super::search::{
    DEGRADE_NOTE_RESERVE, JsonObject, digits, join_notes, largest_page_size, page_of, page_size_of,
};
use crate::{
    budget::{ResponseBudget, strip_from_end},
    error::AppError,
    failure::doctrine_failure,
    input::SearchBofipInput,
    messages,
    output::search_bofip::{BOFIP_CONSTANTS_RESERVE_CHARS, SearchBofipOutput},
    ports::{DoctrineError, TaxDoctrine},
    tools::ToolName,
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_SEARCH_BOFIP;

/// Note d'une recherche sans résultat, même approché : elle rappelle la règle (tous les mots),
/// ce que l'appelant peut changer, et que le vide ne prouve rien.
pub(crate) const NO_MATCH_NOTE: &str = "No match, even with approximate spelling: every word of \
     query must appear in a publication. Try fewer or other words, the singular or plural form, \
     remove serie, or set kind=\"all\"; an empty result does not mean the tax administration has \
     published nothing on the subject.";

/// Note d'une page servie par la recherche approchée (ADR-0031, décision 5) : aucune publication
/// ne porte tous les mots tels quels, et ceux de la page n'y correspondent qu'à peu près.
pub(crate) const APPROXIMATE_NOTE: &str = "No publication contains every word of query as \
     written; these results match only approximately (a word may differ by a letter or two, or \
     be the start of a longer word): check each title before relying on it, or try other words.";

/// Note de fin de fenêtre atteignable : le portail ne sert pas au-delà de 10 000 résultats.
const REACHABLE_NOTE: &str = "Results beyond the first 10,000 are not reachable; narrow the query \
                              with more words or with serie.";

/// Détail de journal d'une page plus longue que demandé.
const TOO_MANY_HITS: &str = "more hits than the requested page_size";

/// Longueur, en caractères, d'un titre raccourci par le plafond de réponse, `…` compris.
pub(crate) const TITLE_SHORT_CHARS: usize = 120;

/// Longueur ISO d'une date rendue, `AAAA-MM-JJ`.
const ISO_DATE_CHARS: usize = 10;

/// Recherche dans le BOFiP : analyse des arguments, un appel au port (deux si aucune publication
/// ne porte tous les mots tels quels), politique de réponse.
pub(crate) struct SearchBofipUseCase<'a, D: TaxDoctrine> {
    doctrine: &'a D,
}

impl<'a, D: TaxDoctrine> SearchBofipUseCase<'a, D> {
    /// Cas d'usage qui interroge `doctrine`.
    pub(crate) fn new(doctrine: &'a D) -> Self {
        Self { doctrine }
    }

    /// Valide `input`, interroge le port en mots exacts, puis en mots approchants si rien
    /// d'exact n'existe, et met la page en forme.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour tout argument refusé (sans appel au port) ou pour une page qui ne tient
    /// pas sous le plafond de réponse ; `upstream_error` pour une page plus longue que demandé ;
    /// sinon l'erreur du port, traduite par [`doctrine_failure`].
    pub(crate) async fn execute(
        &self,
        input: SearchBofipInput,
    ) -> Result<SearchBofipOutput, AppError> {
        let query = query_of(&input)?;
        let page = self.search(&query).await?;
        if !is_empty(&page) {
            return present(&query, page);
        }
        let query = query.approximate();
        let page = self.search(&query).await?;
        present(&query, page)
    }

    /// Un appel au port, son erreur traduite pour l'outil.
    async fn search(&self, query: &DoctrineQuery) -> Result<DoctrinePage, AppError> {
        self.doctrine.search(query).await.map_err(|error| doctrine_failure(TOOL, "", &error))
    }
}

/// Vrai si la recherche n'a trouvé aucune publication : total nul et page vide. Une page vide
/// au-delà de la fin d'une recherche qui a des résultats ne l'est pas.
fn is_empty(page: &DoctrinePage) -> bool {
    page.total == 0 && page.hits.is_empty()
}

/// Requête validée du port, ou premier argument refusé.
fn query_of(input: &SearchBofipInput) -> Result<DoctrineQuery, AppError> {
    let terms = terms_of(input.query.as_deref())?;
    let serie = input
        .serie
        .as_deref()
        .map(Serie::parse)
        .transpose()
        .map_err(|error| messages::domain(TOOL, &error))?;
    let kind = kind_of(input.kind.as_deref())?;
    let page_size = page_size_of(input.page_size)?;
    let page = page_of(input.page)?;
    DoctrineQuery::new(terms, serie, kind, page, page_size)
        .map_err(|error| messages::domain(TOOL, &error))
}

/// `query` : requise, puis réduite à ses mots par le domaine.
fn terms_of(value: Option<&str>) -> Result<DoctrineTerms, AppError> {
    let value = value.ok_or_else(|| messages::required("query"))?;
    DoctrineTerms::parse(value).map_err(|error| messages::domain(TOOL, &error))
}

/// `kind` : `doctrine` par défaut, insensible à la casse et aux espaces de bord.
fn kind_of(value: Option<&str>) -> Result<KindFilter, AppError> {
    let Some(value) = value else { return Ok(KindFilter::default()) };
    KindFilter::parse(value).ok_or_else(|| messages::not_one_of("kind", value, &KindFilter::NAMES))
}

/// Met la page du port en forme : suite, notes, plafond de réponse. La note dit d'abord comment
/// les mots ont correspondu (aucun résultat, ou résultats approchés), puis la fin de la fenêtre
/// atteignable.
fn present(query: &DoctrineQuery, page: DoctrinePage) -> Result<SearchBofipOutput, AppError> {
    let (number, size) = (query.page(), query.page_size());
    let asked = usize::try_from(size.get()).unwrap_or(usize::MAX);
    if page.hits.len() > asked {
        let error = DoctrineError::Decode { detail: TOO_MANY_HITS.to_owned() };
        return Err(doctrine_failure(TOOL, "", &error));
    }
    let more = has_more(number, size, page.total);
    // Il reste des résultats après cette page, mais la fenêtre atteignable s'arrête ici.
    let seen = u64::from(number.get()).saturating_mul(u64::from(size.get()));
    let unreachable = !more && seen < page.total;
    let matching = match (is_empty(&page), query.matching()) {
        (true, _) => Some(NO_MATCH_NOTE),
        (false, DoctrineMatch::Approximate) => Some(APPROXIMATE_NOTE),
        (false, DoctrineMatch::Exact) => None,
    };
    // « Aucun résultat » et la fin de fenêtre s'excluent : la seconde exige un total positif.
    let note =
        join_notes([matching.map(str::to_owned), unreachable.then(|| REACHABLE_NOTE.to_owned())]);
    let mut output = SearchBofipOutput {
        total: page.total,
        page: number.get(),
        has_more: more,
        hits: page.hits,
        note,
    };
    fit_under_target(&mut output, size)?;
    Ok(output)
}

/// Raccourcit des titres jusqu'à ce que la taille de `output` passe sous la cible (05 §6.4), ou
/// refuse la page.
fn fit_under_target(output: &mut SearchBofipOutput, requested: PageSize) -> Result<(), AppError> {
    let fixed = envelope_cost(output)
        .saturating_add(BOFIP_CONSTANTS_RESERVE_CHARS)
        .saturating_add(DEGRADE_NOTE_RESERVE);
    let over = |hits: &[DoctrineHit]| {
        fixed.saturating_add(page_cost(hits)) > ResponseBudget::ESTIMATE_TARGET_CHARS
    };
    let shortened = strip_from_end(&mut output.hits, over, shorten_title);
    if over(&output.hits) {
        let largest = largest_page_size(&output.hits, fixed, hit_cost);
        return Err(messages::invalid_argument(format!(
            "page_size={} would exceed the 45,000-character response cap for this query; use \
             page_size <= {largest}.",
            requested.get()
        )));
    }
    let titles = (shortened > 0).then(|| titles_note(shortened));
    output.note = join_notes([output.note.take(), titles]);
    Ok(())
}

/// Coupe le titre du résultat à [`TITLE_SHORT_CHARS`] caractères, `…` compris ; vrai s'il était
/// plus long.
fn shorten_title(hit: &mut DoctrineHit) -> bool {
    let long = hit.title.chars().count() > TITLE_SHORT_CHARS;
    if long {
        let kept = messages::clip(&hit.title, TITLE_SHORT_CHARS.saturating_sub(1));
        hit.title = format!("{kept}…");
    }
    long
}

/// Note des titres raccourcis : leur nombre, et comment les lire en entier. Les titres courts ne
/// sont pas touchés : ce ne sont pas forcément ceux des derniers résultats.
fn titles_note(count: usize) -> String {
    let subject = if count == 1 {
        "1 long title was".to_owned()
    } else {
        format!("{count} long titles were")
    };
    format!(
        "{subject} shortened to keep the response under 45,000 characters; use a smaller \
         page_size, or fr_get_bofip, to get them whole."
    )
}

/// Longueur du JSON compact de l'enveloppe, `hits` vide et sans les deux champs constants de la
/// vue : `total`, `page`, `has_more`, `hits`, plus `note` quand elle est présente.
fn envelope_cost(output: &SearchBofipOutput) -> usize {
    let mut json = JsonObject::default();
    json.raw("total", digits(output.total));
    json.raw("page", digits(u64::from(output.page)));
    json.raw("has_more", if output.has_more { 4 } else { 5 });
    json.raw("hits", 2);
    json.optional_text("note", output.note.as_deref());
    json.total()
}

/// Longueur du JSON compact des résultats, sans les crochets de la liste (ceux de
/// [`envelope_cost`]) : leurs objets et les virgules qui les séparent.
fn page_cost(hits: &[DoctrineHit]) -> usize {
    hits.iter()
        .map(hit_cost)
        .fold(0, usize::saturating_add)
        .saturating_add(hits.len().saturating_sub(1))
}

/// Longueur du JSON compact d'un résultat, telle que la vue l'écrit : `id`, `publication_id`,
/// `title` et `kind` toujours ; `serie`, `valid_from` et `source_url` s'ils sont présents.
fn hit_cost(hit: &DoctrineHit) -> usize {
    let mut json = JsonObject::default();
    json.text("id", hit.id.as_str());
    json.raw("publication_id", digits(u64::from(hit.publication_id.get())));
    json.text("title", &hit.title);
    json.raw("kind", hit.kind.name().len().saturating_add(2));
    json.optional_text("serie", hit.serie.as_ref().map(Serie::as_str));
    if hit.valid_from.is_some() {
        json.raw("valid_from", ISO_DATE_CHARS.saturating_add(2));
    }
    json.optional_text("source_url", hit.source_url.as_ref().map(BofipUrl::as_str));
    json.total()
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use lf_domain::doctrine::DoctrineKind;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        error::{ErrorCode, UpstreamCause},
        testing::{DoctrineRequest, FakeDoctrine, fixtures::bofip},
    };

    fn text(query: &str) -> SearchBofipInput {
        SearchBofipInput { query: Some(query.to_owned()), ..SearchBofipInput::default() }
    }

    fn page(total: u64, hits: Vec<DoctrineHit>) -> DoctrinePage {
        DoctrinePage { total, hits }
    }

    /// `count` résultats aux numéros de publication 1, 2…, tous de titre `title`.
    fn hits(count: u32, title: &str) -> Vec<DoctrineHit> {
        (1..=count).map(|number| bofip::hit(number, title)).collect()
    }

    async fn run(
        input: SearchBofipInput,
        response: Result<DoctrinePage, DoctrineError>,
    ) -> (Result<SearchBofipOutput, AppError>, FakeDoctrine) {
        run_all(input, vec![response]).await
    }

    /// Comme [`run`], avec une réponse programmée par appel au port, dans l'ordre.
    async fn run_all(
        input: SearchBofipInput,
        responses: Vec<Result<DoctrinePage, DoctrineError>>,
    ) -> (Result<SearchBofipOutput, AppError>, FakeDoctrine) {
        let doctrine = FakeDoctrine::new();
        for response in responses {
            doctrine.push_search(response);
        }
        let result = SearchBofipUseCase::new(&doctrine).execute(input).await;
        (result, doctrine)
    }

    /// Recherche de `words` en mots exacts, avec les valeurs par défaut de l'outil.
    fn exact(words: &str) -> DoctrineQuery {
        DoctrineQuery::new(
            DoctrineTerms::parse(words).unwrap(),
            None,
            KindFilter::Doctrine,
            NonZeroU32::MIN,
            PageSize::DEFAULT,
        )
        .unwrap()
    }

    /// Les deux appels du secours : `words` en mots exacts, puis les mêmes en mots approchants.
    fn both_calls(words: &str) -> Vec<DoctrineRequest> {
        vec![
            DoctrineRequest::Search(exact(words)),
            DoctrineRequest::Search(exact(words).approximate()),
        ]
    }

    /// Message d'un argument refusé ; vérifie qu'aucun appel n'atteint le port.
    async fn rejected(input: SearchBofipInput) -> String {
        let (result, doctrine) = run(input, Ok(page(0, Vec::new()))).await;
        let error = result.unwrap_err();
        assert_eq!((error.code(), doctrine.calls()), (Some(ErrorCode::InvalidArg), 0));
        error.message().to_owned()
    }

    #[tokio::test]
    async fn a_search_sends_the_validated_query_and_returns_the_page() {
        let input = SearchBofipInput {
            query: Some("plus-value immobilière".to_owned()),
            serie: Some(" rfpi ".to_owned()),
            kind: Some("ALL".to_owned()),
            page: Some(3),
            page_size: Some(2),
        };
        let found = vec![bofip::hit(3350, "Premier"), bofip::hit(3351, "Second")];
        let (result, doctrine) = run(input, Ok(page(357, found.clone()))).await;
        let output = result.unwrap();
        assert_eq!(
            output,
            SearchBofipOutput { total: 357, page: 3, has_more: true, hits: found, note: None }
        );
        assert_eq!(output.output_count_or_size(), 2);
        let expected = DoctrineQuery::new(
            DoctrineTerms::parse("plus value immobilière").unwrap(),
            Some(Serie::parse("RFPI").unwrap()),
            KindFilter::All,
            NonZeroU32::new(3).unwrap(),
            PageSize::new(2).unwrap(),
        )
        .unwrap();
        assert_eq!(doctrine.seen_requests(), vec![DoctrineRequest::Search(expected)]);
    }

    #[tokio::test]
    async fn defaults_are_doctrine_only_first_page_of_ten() {
        let (result, doctrine) = run(text("donation"), Ok(page(1, hits(1, "Donations")))).await;
        assert_eq!(result.unwrap().page, 1);
        assert_eq!(doctrine.seen_requests(), vec![DoctrineRequest::Search(bofip::query())]);
        assert_eq!(bofip::query().kind(), KindFilter::Doctrine);
        assert_eq!(bofip::query().page_size(), PageSize::DEFAULT);
    }

    #[tokio::test]
    async fn arguments_are_validated_in_order_before_any_call() {
        // Tous les arguments sont fautifs : chaque ligne corrige le premier refusé.
        let mut input = SearchBofipInput {
            query: None,
            serie: Some("EN R".to_owned()),
            kind: Some("actualité".to_owned()),
            page: Some(0),
            page_size: Some(0),
        };
        assert_eq!(rejected(input.clone()).await, "query is required.");
        input.query = Some("   ".to_owned());
        assert_eq!(rejected(input.clone()).await, "query is required.");
        input.query = Some("« … » ?".to_owned());
        assert_eq!(
            rejected(input.clone()).await,
            "query contains no searchable word (only punctuation or symbols); give a few plain \
             words."
        );
        input.query = Some("donation".to_owned());
        assert_eq!(
            rejected(input.clone()).await,
            "serie='EN R' is not a BOFiP serie code (1 to 16 letters or digits, e.g. ENR, IR, \
             RFPI); omit it to search every serie."
        );
        input.serie = Some("ENR".to_owned());
        assert_eq!(
            rejected(input.clone()).await,
            "kind='actualité' must be one of ['all', 'doctrine', 'news']."
        );
        input.kind = Some("news".to_owned());
        assert_eq!(rejected(input.clone()).await, "page_size=0 out of range (1..100).");
        input.page_size = Some(100);
        assert_eq!(rejected(input.clone()).await, "page=0 out of range (1..).");
        input.page = Some(101);
        assert_eq!(
            rejected(input.clone()).await,
            "page=101 with page_size=100 goes beyond the 10000 reachable results; narrow the \
             query with more words or with serie."
        );
        // Dernière page atteignable : l'appel part.
        input.page = Some(100);
        let (result, doctrine) = run(input, Ok(page(10_000, hits(1, "Donations")))).await;
        assert_eq!((result.is_ok(), doctrine.calls()), (true, 1));
    }

    #[tokio::test]
    async fn a_query_too_long_is_refused_like_fr_search() {
        assert_eq!(
            rejected(text(&"a".repeat(451))).await,
            "query is too long (451 characters, max 450); shorten it."
        );
    }

    /// Des résultats exacts : un seul appel, en mots exacts, et aucune note (ADR-0031).
    #[tokio::test]
    async fn exact_results_are_served_after_a_single_exact_call() {
        let found = hits(2, "Donations");
        let (result, doctrine) = run(text("donation"), Ok(page(58, found.clone()))).await;
        assert_eq!(
            result.unwrap(),
            SearchBofipOutput { total: 58, page: 1, has_more: true, hits: found, note: None }
        );
        assert_eq!(doctrine.seen_requests(), vec![DoctrineRequest::Search(exact("donation"))]);
        assert_eq!(exact("donation").matching(), DoctrineMatch::Exact);
    }

    /// Rien d'exact : la même requête part en mots approchants (mêmes mots, mêmes filtres, même
    /// page), et la réponse le dit.
    #[tokio::test]
    async fn without_exact_results_the_same_query_is_sent_approximately_and_said_so() {
        let input = SearchBofipInput {
            serie: Some("ENR".to_owned()),
            kind: Some("all".to_owned()),
            page: Some(2),
            page_size: Some(5),
            ..text("donnation abattement")
        };
        let found = hits(5, "Donations");
        let (result, doctrine) =
            run_all(input, vec![Ok(page(0, Vec::new())), Ok(page(122, found.clone()))]).await;
        assert_eq!(
            result.unwrap(),
            SearchBofipOutput {
                total: 122,
                page: 2,
                has_more: true,
                hits: found,
                note: Some(APPROXIMATE_NOTE.to_owned()),
            }
        );
        let exact = DoctrineQuery::new(
            DoctrineTerms::parse("donnation abattement").unwrap(),
            Some(Serie::parse("ENR").unwrap()),
            KindFilter::All,
            NonZeroU32::new(2).unwrap(),
            PageSize::new(5).unwrap(),
        )
        .unwrap();
        assert_eq!(
            doctrine.seen_requests(),
            vec![
                DoctrineRequest::Search(exact.clone()),
                DoctrineRequest::Search(exact.approximate())
            ]
        );
    }

    /// Rien, même approché : deux appels, et la note dit la règle et quoi changer.
    #[tokio::test]
    async fn an_empty_result_explains_the_search_rule() {
        let (result, doctrine) =
            run_all(text("zzzz"), vec![Ok(page(0, Vec::new())), Ok(page(0, Vec::new()))]).await;
        assert_eq!(
            result.unwrap(),
            SearchBofipOutput {
                total: 0,
                page: 1,
                has_more: false,
                hits: Vec::new(),
                note: Some(NO_MATCH_NOTE.to_owned()),
            }
        );
        assert_eq!(doctrine.seen_requests(), both_calls("zzzz"));
        // Une page vide au-delà de la fin d'une recherche qui a des résultats exacts n'est pas
        // « aucun résultat » : un seul appel, aucune note.
        let input = SearchBofipInput { page: Some(9), ..text("donation") };
        let (result, doctrine) = run(input, Ok(page(12, Vec::new()))).await;
        assert_eq!((result.unwrap().note, doctrine.calls()), (None, 1));
        // Un total nul avec des résultats (réponse incohérente) n'est pas « aucun résultat » non
        // plus : la page est rendue, sans second appel.
        let (result, doctrine) = run(text("donation"), Ok(page(0, hits(1, "Donations")))).await;
        assert_eq!((result.unwrap().note, doctrine.calls()), (None, 1));
        // De même pour la recherche approchée : des résultats, ou une page au-delà de sa fin,
        // gardent la note des résultats approchés.
        for second in [page(3, Vec::new()), page(0, hits(1, "Donations"))] {
            let (result, _) =
                run_all(text("donation"), vec![Ok(page(0, Vec::new())), Ok(second)]).await;
            assert_eq!(result.unwrap().note.as_deref(), Some(APPROXIMATE_NOTE));
        }
    }

    /// Une erreur du premier appel est rendue sans second appel ; une erreur du second est
    /// rendue elle aussi.
    #[tokio::test]
    async fn an_error_of_either_call_is_returned() {
        let (result, doctrine) = run(text("donation"), Err(DoctrineError::Timeout)).await;
        let error = result.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_without_query_words(&error, "donation");
        assert_eq!(doctrine.seen_requests(), vec![DoctrineRequest::Search(exact("donation"))]);
        let limited = DoctrineError::RateLimited { retry_after: None };
        let (result, doctrine) =
            run_all(text("donation"), vec![Ok(page(0, Vec::new())), Err(limited)]).await;
        let error = result.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::RateLimited));
        assert_without_query_words(&error, "donation");
        assert_eq!(doctrine.seen_requests(), both_calls("donation"));
    }

    /// Une erreur rendue ne cite pas les mots de la requête, ni dans son texte, ni dans sa forme
    /// de débogage (ADR-0029, étendu par ADR-0031 à la seconde requête).
    fn assert_without_query_words(error: &AppError, words: &str) {
        let shown = format!("{error} | {error:?}");
        assert!(!shown.contains(words), "{shown}");
    }

    /// Une page approchée garde ses deux notes, dans l'ordre : comment les mots ont correspondu,
    /// puis la fin de la fenêtre atteignable ; une page approchée trop longue est refusée.
    #[tokio::test]
    async fn an_approximate_page_keeps_both_notes_and_the_page_size_rule() {
        let input = SearchBofipInput { page: Some(100), page_size: Some(100), ..text("impot") };
        let (result, _) =
            run_all(input, vec![Ok(page(0, Vec::new())), Ok(page(10_001, hits(100, "Impôt")))])
                .await;
        assert_eq!(result.unwrap().note, Some(format!("{APPROXIMATE_NOTE} {REACHABLE_NOTE}")));
        let input = SearchBofipInput { page_size: Some(2), ..text("donation") };
        let (result, _) =
            run_all(input, vec![Ok(page(0, Vec::new())), Ok(page(3, hits(3, "Donations")))]).await;
        assert_eq!(result.unwrap_err().code(), Some(ErrorCode::UpstreamError));
    }

    /// Les notes de ce cas d'usage sont des constantes, sans gabarit à remplir : elles ne peuvent
    /// pas citer les mots de la requête. Elles comptent dans l'enveloppe de la réponse ; le
    /// budget avec la plus longue combinaison de notes est tenu par le test suivant.
    #[test]
    fn the_notes_are_fixed_texts() {
        for note in [NO_MATCH_NOTE, APPROXIMATE_NOTE, REACHABLE_NOTE] {
            assert!(!note.contains('{') && !note.is_empty(), "{note}");
        }
        assert!(APPROXIMATE_NOTE.contains("approximately"));
        assert!(NO_MATCH_NOTE.contains("even with approximate spelling"));
    }

    /// Des titres raccourcis dans une page approchée : la note des résultats approchés vient
    /// d'abord, celle des titres ensuite, et la réponse tient sous la cible.
    #[tokio::test]
    async fn an_approximate_page_with_shortened_titles_keeps_its_note_first() {
        let input = SearchBofipInput { page_size: Some(100), ..text("donation") };
        let list = hits(100, &"t".repeat(400));
        let (result, _) = run_all(input, vec![Ok(page(0, Vec::new())), Ok(page(100, list))]).await;
        let output = result.unwrap();
        let note = output.note.clone().unwrap();
        assert!(note.starts_with(&format!("{APPROXIMATE_NOTE} ")), "{note}");
        assert!(note.ends_with("use a smaller page_size, or fr_get_bofip, to get them whole."));
        assert!(
            envelope_cost(&output) + BOFIP_CONSTANTS_RESERVE_CHARS + page_cost(&output.hits)
                <= ResponseBudget::ESTIMATE_TARGET_CHARS
        );
    }

    #[tokio::test]
    async fn has_more_follows_the_total_and_the_reachable_window() {
        let rows = [
            // (page, page_size, total, has_more, note de fin de fenêtre)
            (1, 10, 10, false, false),
            (1, 10, 11, true, false),
            (2, 10, 11, false, false),
            (99, 100, 20_000, true, false),
            (100, 100, 10_000, false, false),
            (100, 100, 10_001, false, true),
        ];
        for (number, size, total, more, note) in rows {
            let input =
                SearchBofipInput { page: Some(number), page_size: Some(size), ..text("donation") };
            let (result, _) = run(input, Ok(page(total, hits(1, "Donations")))).await;
            let output = result.unwrap();
            assert_eq!(
                (output.has_more, output.note),
                (more, note.then(|| REACHABLE_NOTE.to_owned())),
                "page {number}, page_size {size}, total {total}"
            );
        }
    }

    #[tokio::test]
    async fn a_page_longer_than_asked_is_an_upstream_error() {
        let input = SearchBofipInput { page_size: Some(2), ..text("donation") };
        let (result, _) = run(input.clone(), Ok(page(3, hits(3, "Donations")))).await;
        assert_eq!(
            result.unwrap_err(),
            AppError::Upstream {
                message: "unexpected response from the BOFiP portal; report it.".to_owned(),
                cause: UpstreamCause::Decode,
            }
        );
        // Autant que demandé : la page est rendue.
        let (result, _) = run(input, Ok(page(3, hits(2, "Donations")))).await;
        assert_eq!(result.unwrap().hits.len(), 2);
    }

    #[tokio::test]
    async fn port_errors_are_translated_for_the_search_tool() {
        let (result, _) = run(text("donation"), Err(DoctrineError::Disabled)).await;
        let error = result.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::ConfigError));
        assert_eq!(error.message(), messages::DOCTRINE_DISABLED_MESSAGE);
        let (result, _) =
            run(text("donation"), Err(DoctrineError::RateLimited { retry_after: None })).await;
        assert_eq!(
            result.unwrap_err(),
            AppError::RateLimited {
                message: "BOFiP portal quota reached; retry later.".to_owned(),
                retry_after: None,
            }
        );
        // Une recherche ne reçoit jamais `NotFound` : repli sur une réponse inattendue.
        let (result, _) = run(text("donation"), Err(DoctrineError::NotFound)).await;
        assert_eq!(result.unwrap_err().code(), Some(ErrorCode::UpstreamError));
    }

    /// Le coût d'un résultat est la longueur exacte de son JSON compact, champ par champ, dans
    /// l'ordre de la vue.
    #[test]
    fn hit_cost_is_the_length_of_the_compact_json_of_the_view() {
        let full = bofip::hit(3350, "Donations \"entre\" époux\\");
        let json = r#"{"id":"BOI-ENR-DMTG-10-50-20","publication_id":3350,"title":"Donations \"entre\" époux\\","kind":"doctrine","serie":"ENR","valid_from":"2023-05-24","source_url":"https://bofip.impots.gouv.fr/bofip/3350-PGP.html/identifiant=BOI-ENR-DMTG-10-50-20-20230524"}"#;
        assert_eq!(hit_cost(&full), json.chars().count());

        let bare = DoctrineHit {
            kind: DoctrineKind::News,
            serie: None,
            valid_from: None,
            source_url: None,
            ..bofip::hit(12, "a")
        };
        let json =
            r#"{"id":"BOI-ENR-DMTG-10-50-20","publication_id":12,"title":"a","kind":"news"}"#;
        assert_eq!(hit_cost(&bare), json.chars().count());

        // Chaque champ facultatif pèse son propre poids.
        let serie = DoctrineHit { serie: Some(Serie::parse("TVA").unwrap()), ..bare.clone() };
        assert_eq!(hit_cost(&serie), hit_cost(&bare) + r#","serie":"TVA""#.len());
        let dated = DoctrineHit { valid_from: full.valid_from, ..bare.clone() };
        assert_eq!(hit_cost(&dated), hit_cost(&bare) + r#","valid_from":"2023-05-24""#.len());
        let linked = DoctrineHit { source_url: full.source_url.clone(), ..bare.clone() };
        let url = full.source_url.unwrap();
        assert_eq!(
            hit_cost(&linked),
            hit_cost(&bare) + r#","source_url":"""#.len() + url.as_str().len()
        );
    }

    #[test]
    fn the_page_cost_counts_one_comma_between_hits() {
        let list = hits(3, "Donations");
        let costs: usize = list.iter().map(hit_cost).sum();
        assert_eq!(page_cost(&list), costs + 2);
        assert_eq!(page_cost(&list[..1]), hit_cost(&list[0]));
        assert_eq!(page_cost(&[]), 0);
    }

    #[test]
    fn the_envelope_cost_is_the_length_of_its_compact_json_without_hits() {
        let mut output = SearchBofipOutput {
            total: 357,
            page: 12,
            has_more: true,
            hits: Vec::new(),
            note: None,
        };
        let json = r#"{"total":357,"page":12,"has_more":true,"hits":[]}"#;
        assert_eq!(envelope_cost(&output), json.len());
        output.has_more = false;
        output.note = Some("fin \"de\" fenêtre".to_owned());
        let json =
            r#"{"total":357,"page":12,"has_more":false,"hits":[],"note":"fin \"de\" fenêtre"}"#;
        assert_eq!(envelope_cost(&output), json.chars().count());
    }

    #[test]
    fn a_title_is_shortened_to_120_characters_with_an_ellipsis() {
        let mut long = bofip::hit(1, &"é".repeat(121));
        assert!(shorten_title(&mut long));
        assert_eq!(long.title, format!("{}…", "é".repeat(119)));
        assert_eq!(long.title.chars().count(), TITLE_SHORT_CHARS);
        // Déjà raccourci, ou juste à la borne : rien à faire.
        assert!(!shorten_title(&mut long));
        let mut exact = bofip::hit(1, &"é".repeat(120));
        assert!(!shorten_title(&mut exact));
        assert_eq!(exact.title, "é".repeat(120));
    }

    /// Taille fixe d'une réponse de `count` résultats sans note : enveloppe et réserves.
    fn fixed(total: u64, more: bool) -> usize {
        let output =
            SearchBofipOutput { total, page: 1, has_more: more, hits: Vec::new(), note: None };
        envelope_cost(&output) + BOFIP_CONSTANTS_RESERVE_CHARS + DEGRADE_NOTE_RESERVE
    }

    async fn page_of_100(list: Vec<DoctrineHit>) -> Result<SearchBofipOutput, AppError> {
        let input = SearchBofipInput { page_size: Some(100), ..text("donation") };
        let total = u64::try_from(list.len()).unwrap();
        run(input, Ok(page(total, list))).await.0
    }

    #[tokio::test]
    async fn a_page_under_the_target_is_returned_untouched() {
        // Cent résultats aux titres courts : la page mesurée tient.
        let list = hits(100, "ENR - Mutations à titre gratuit - Donations");
        assert!(fixed(100, false) + page_cost(&list) <= ResponseBudget::ESTIMATE_TARGET_CHARS);
        let output = page_of_100(list.clone()).await.unwrap();
        assert_eq!((output.hits, output.note), (list, None));
    }

    #[tokio::test]
    async fn long_titles_are_shortened_from_the_last_hit_until_the_page_fits() {
        // Cent titres de 400 caractères : la page dépasse la cible, les derniers titres sont
        // raccourcis, aucun résultat n'est retiré.
        let list = hits(100, &"t".repeat(400));
        assert!(fixed(100, false) + page_cost(&list) > ResponseBudget::ESTIMATE_TARGET_CHARS);
        let output = page_of_100(list.clone()).await.unwrap();
        assert_eq!(output.hits.len(), 100);
        let shortened =
            output.hits.iter().filter(|hit| hit.title.chars().count() == TITLE_SHORT_CHARS).count();
        let whole = output.hits.iter().filter(|hit| hit.title.chars().count() == 400).count();
        assert_eq!(shortened + whole, 100);
        // Les titres raccourcis sont les derniers, et il n'y en a pas un de trop : avec un titre
        // raccourci de moins, la page dépasserait.
        assert!(output.hits[..whole].iter().all(|hit| hit.title.chars().count() == 400));
        assert!(
            fixed(100, false) + page_cost(&output.hits) <= ResponseBudget::ESTIMATE_TARGET_CHARS
        );
        let mut one_less = output.hits.clone();
        one_less[whole] = list[whole].clone();
        assert!(fixed(100, false) + page_cost(&one_less) > ResponseBudget::ESTIMATE_TARGET_CHARS);
        assert!(shortened > 1);
        assert_eq!(
            output.note,
            Some(format!(
                "{shortened} long titles were shortened to keep the response under 45,000 \
                 characters; use a smaller page_size, or fr_get_bofip, to get them whole."
            ))
        );
        // Les identifiants, numéros et adresses sont intacts.
        let kept: Vec<_> = output.hits.iter().map(|hit| hit.publication_id).collect();
        assert_eq!(kept, list.iter().map(|hit| hit.publication_id).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn a_single_shortened_title_is_counted_in_the_singular_after_the_other_note() {
        // Total au-delà de la fenêtre : la note de fin de fenêtre précède celle des titres.
        let mut list = hits(100, "court");
        let room = ResponseBudget::ESTIMATE_TARGET_CHARS - fixed_with_note() - page_cost(&list);
        // Un seul titre long, juste assez pour dépasser : le raccourcir suffit.
        list[99].title = "t".repeat(5 + room + 1);
        let input = SearchBofipInput { page: Some(100), page_size: Some(100), ..text("donation") };
        let (result, _) = run(input, Ok(page(10_001, list))).await;
        assert_eq!(
            result.unwrap().note,
            Some(format!(
                "{REACHABLE_NOTE} 1 long title was shortened to keep the response under 45,000 \
                 characters; use a smaller page_size, or fr_get_bofip, to get them whole."
            ))
        );
    }

    /// Taille fixe de la page 100 d'une recherche de 10 001 résultats, note de fin de fenêtre
    /// comprise.
    fn fixed_with_note() -> usize {
        let output = SearchBofipOutput {
            total: 10_001,
            page: 100,
            has_more: false,
            hits: Vec::new(),
            note: Some(REACHABLE_NOTE.to_owned()),
        };
        envelope_cost(&output) + BOFIP_CONSTANTS_RESERVE_CHARS + DEGRADE_NOTE_RESERVE
    }

    #[tokio::test]
    async fn the_target_is_inclusive() {
        // Page mesurée à la cible exacte : rendue telle quelle. Un caractère de plus : raccourcie.
        let mut list = hits(100, "court");
        let room = ResponseBudget::ESTIMATE_TARGET_CHARS - fixed(100, false) - page_cost(&list);
        list[0].title = "t".repeat(5 + room);
        assert_eq!(fixed(100, false) + page_cost(&list), ResponseBudget::ESTIMATE_TARGET_CHARS);
        assert_eq!(page_of_100(list.clone()).await.unwrap().note, None);
        list[0].title.push('t');
        let output = page_of_100(list).await.unwrap();
        assert_eq!(output.hits[0].title.chars().count(), TITLE_SHORT_CHARS);
        assert!(output.note.unwrap().starts_with("1 long title was shortened "));
    }

    #[tokio::test]
    async fn a_page_that_cannot_fit_proposes_the_largest_page_size_that_does() {
        // Cent résultats dont les adresses et les titres restent trop lourds une fois raccourcis.
        let list = hits(100, &"\"".repeat(400));
        let mut shortest = list.clone();
        assert!(shortest.iter_mut().all(shorten_title));
        assert!(fixed(100, false) + page_cost(&shortest) > ResponseBudget::ESTIMATE_TARGET_CHARS);
        let largest = largest_page_size(&shortest, fixed(100, false), hit_cost);
        assert!((1..100).contains(&largest));
        let error = page_of_100(list.clone()).await.unwrap_err();
        assert_eq!(
            error,
            AppError::invalid_argument(format!(
                "page_size=100 would exceed the 45,000-character response cap for this query; \
                 use page_size <= {largest}."
            ))
        );
        // Règle rejouable : avec cette taille de page, la réponse tient.
        let input = SearchBofipInput {
            page_size: Some(i64::try_from(largest).unwrap()),
            ..text("donation")
        };
        let (result, _) = run(input, Ok(page(100, list[..largest].to_vec()))).await;
        assert_eq!(result.unwrap().hits.len(), largest);
    }
}
