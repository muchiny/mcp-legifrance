//! Cas d'usage de `fr_search` (v0.1) : la recherche plein texte (`SearchUseCase`, 05 §5.1).
//!
//! # Validation (02 §6.1, P-011)
//!
//! Sans réseau et dans l'ordre de la référence : `fond`, `query`, `page_size`, puis `page`,
//! `match`, `date_from`, `date_to` et `version_date` (leur forme), puis les combinaisons, dans
//! l'ordre du domaine : période inversée (`DateRange::new`), `version_date` hors LODA et CODE,
//! période sur CODE, page hors de la fenêtre atteignable (`SearchQuery::new`), puis `in_force_only`
//! hors de KALI (`SearchQuery::with_in_force_only`, D-08). Le premier échec
//! est rendu ; aucun appel au port ne part.
//!
//! # Dates par défaut (03 §6.4)
//!
//! `version_date` vaut le jour civil Europe/Paris de l'horloge pour LODA et CODE, jamais la date
//! UTC (D-19) : la requête du port, donc la clé de cache, change chaque jour à minuit à Paris.
//!
//! # Plafond de réponse (05 §6.4, 02 §6.1)
//!
//! La taille de la page est calculée au caractère près, comme le JSON compact de la vue de
//! `lf-adapter-mcp` l'écrira (clés, guillemets, séparateurs et échappements compris, et seulement
//! les champs que la vue rend : le `title` contenu dans la citation et une date sentinelle sont
//! omis), puis comparée à la cible de 42 000 caractères ; la page est dégradée dans cet ordre,
//! sans jamais retirer un résultat : (1) extraits des derniers résultats, avec une `note` ; (2)
//! listes d'articles et champs optionnels (`title`, `nature`, `etat`, `date`) des derniers
//! résultats ; (3) si la page ne tient toujours pas, `invalid_arg` qui propose la plus grande
//! `page_size` qui tiendrait. La mesure exacte, sur le JSON, est faite par la vue de
//! `lf-adapter-mcp`.

use std::num::NonZeroU32;

use lf_domain::{
    contract::Citable,
    date::{DateRange, VersionDate, open_ended},
    entity::{CodeArticleRef, SearchHit, SearchPage, Snippet},
    fond::Fond,
    search::{MatchMode, PageSize, QueryText, SearchQuery, check_quotes, has_more},
    status::LegalStatus,
    text::title_unless_in_citation,
};

use crate::{
    budget::{ResponseBudget, escaped_chars, strip_from_end, the_last_hits},
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

/// Note d'une recherche sans résultat (D-10, 05 §5.1) : `hits` vide et aucun total annoncé. La
/// recherche réussit, mais un modèle lit surtout la réponse : elle lui rappelle que le vide ne
/// prouve pas le silence de la loi. Les autres notes (articles à lire, fin de fenêtre) supposent un
/// total positif : elles ne coïncident jamais avec celle-ci.
pub(crate) const NO_MATCH_NOTE: &str = "No match: try other words, remove the date filters or \
                                        call fr_coverage; an empty result does not mean the law \
                                        is silent.";

/// Note de défense en profondeur de `in_force_only` : un résultat rendu a un `etat` que le filtre
/// aurait dû écarter. Le texte nomme les trois états gardés (D-08) ; il ne dit pas que la source
/// a tort, seulement que le filtre n'est peut-être pas appliqué.
pub(crate) const FILTER_NOT_APPLIED_NOTE: &str = "in_force_only was set, but a hit is not VIGUEUR, \
     VIGUEUR_ETEN or VIGUEUR_NON_ETEN: the source may not have applied the filter. Check each etat.";

/// Longueur ISO d'une date rendue, `AAAA-MM-JJ`.
const ISO_DATE_CHARS: usize = 10;

/// Caractères réservés aux deux notes de dégradation (`snippets_note`, `details_note`) dans
/// l'estimation : les deux notes, avec trois chiffres de compteur, tiennent largement dedans.
const DEGRADE_NOTE_RESERVE_CHARS: usize = 400;

/// Réservation des notes : leur texte, plus le champ `note` s'il n'existe pas encore (`,"note":""`,
/// 10 caractères) ou l'espace qui joint la nouvelle note à l'ancienne.
const DEGRADE_NOTE_RESERVE: usize = DEGRADE_NOTE_RESERVE_CHARS + 11;

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
            in_force_only,
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
            .and_then(|query| query.with_in_force_only(in_force_only.unwrap_or(false)))
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
    let text = QueryText::parse(value).map_err(|error| messages::domain(TOOL, &error))?;
    check_quotes(&text).map_err(|error| messages::domain(TOOL, &error))?;
    Ok(text)
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

/// Met la page du port en forme de réponse : `has_more`, note (fin de fenêtre, articles à lire ou
/// aucun résultat), contrat de citation de chaque résultat, puis plafond de réponse.
fn present(query: &SearchQuery, page: SearchPage) -> Result<SearchOutput, AppError> {
    let (number, size) = (query.page(), query.page_size());
    // CODE : `page_size` compte des articles quand l'amont pagine par article, et `total` des
    // textes. Le mode reçu ne décide pas (il ne suit pas le mode demandé) : seul compte
    // `total_articles > page × page_size`, dans la fenêtre atteignable (L4-07).
    // Seul CODE compte des articles : un autre fond qui renseignerait `total_articles` n'ajoute
    // ni suite ni note.
    let articles = page.total_articles.filter(|_| page.fond == Fond::Code);
    let more_articles = articles.filter(|&count| has_more(number, size, count));
    let more = more_articles.is_some() || has_more(number, size, page.total);
    // Il reste des résultats après cette page, mais la fenêtre atteignable s'arrête ici.
    let seen = u64::from(number.get()).saturating_mul(u64::from(size.get()));
    let largest = page.total.max(articles.unwrap_or(0));
    // Aucun résultat du tout (D-10) : ni total, ni article annoncé, ni résultat listé. Un seul
    // chemin produit une note : les deux autres exigent un total positif.
    let no_match = largest == 0 && page.hits.is_empty();
    let note = more_articles
        .map(|count| articles_note(count, &page.hits, number))
        .or_else(|| (!more && seen < largest).then(|| REACHABLE_NOTE.to_owned()))
        .or_else(|| no_match.then(|| NO_MATCH_NOTE.to_owned()));
    // Défense en profondeur : `in_force_only` demandé, mais un résultat d'un autre état. La note
    // s'ajoute aux autres.
    let unfiltered = query.in_force_only() && page.hits.iter().any(escapes_in_force_filter);
    let note = join_notes([note, unfiltered.then(|| FILTER_NOT_APPLIED_NOTE.to_owned())]);
    let mut output = SearchOutput {
        total: page.total,
        page: number.get(),
        has_more: more,
        total_articles: page.total_articles,
        hits: page.hits.into_iter().map(hit_out).collect(),
        note,
    };
    // La note de défense renvoie à l'`etat` de chaque résultat : la dégradation le garde.
    fit_under_target(&mut output, size, unfiltered)?;
    Ok(output)
}

/// Le résultat a-t-il un `etat` hors des trois que `in_force_only` garde (`VIGUEUR`,
/// `VIGUEUR_ETEN`, `VIGUEUR_NON_ETEN`) ? Un résultat sans `etat` n'est pas signalé : rien ne dit
/// qu'il échappe au filtre.
fn escapes_in_force_filter(hit: &SearchHit) -> bool {
    hit.etat.as_ref().is_some_and(|etat| {
        !matches!(
            etat,
            LegalStatus::Vigueur | LegalStatus::VigueurEtendue | LegalStatus::VigueurNonEtendue
        )
    })
}

/// Les notes présentes, jointes par une espace ; `None` s'il n'y en a aucune.
fn join_notes<const N: usize>(notes: [Option<String>; N]) -> Option<String> {
    let notes: Vec<String> = notes.into_iter().flatten().collect();
    (!notes.is_empty()).then(|| notes.join(" "))
}

/// Note des articles encore à lire sur les pages suivantes (CODE, 05 §5.1) : combien en
/// correspondent, combien cette page en liste, et la page à demander.
fn articles_note(count: u64, hits: &[SearchHit], page: NonZeroU32) -> String {
    let shown: usize = hits.iter().map(|hit| hit.articles.len()).sum();
    format!(
        "{count} matching articles, {shown} shown (page {page}); request page={} for the next \
         articles of this code.",
        page.saturating_add(1)
    )
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
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn fit_under_target(
    output: &mut SearchOutput,
    requested: PageSize,
    keep_etat: bool,
) -> Result<(), AppError> {
    let fixed = envelope_cost(output) + DEGRADE_NOTE_RESERVE;
    let over = |hits: &[HitOut]| fixed + page_cost(hits) > ResponseBudget::ESTIMATE_TARGET_CHARS;
    let snippets = strip_from_end(&mut output.hits, over, strip_snippets);
    let details = strip_from_end(&mut output.hits, over, |out| strip_details(out, keep_etat));
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
        (details > 0).then(|| details_note(details, keep_etat)),
    ];
    output.note = join_notes(notes);
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
/// s'il y avait quelque chose de rendu à retirer (un titre contenu dans la citation et une date
/// sentinelle ne le sont pas). L'`idcc` reste : il identifie la convention. Avec `keep_etat`
/// (note de défense de `in_force_only` posée : « Check each etat »), l'`etat` reste aussi.
fn strip_details(out: &mut HitOut, keep_etat: bool) -> bool {
    let citation = out.citation.as_str();
    let hit = &mut out.hit;
    let had_articles = !hit.articles.is_empty();
    hit.articles.clear();
    let had_fields = [
        hit.title
            .take()
            .is_some_and(|title| title_unless_in_citation(Some(&title), citation).is_some()),
        hit.nature.take().is_some(),
        !keep_etat && hit.etat.take().is_some(),
        hit.date.take().is_some_and(|date| open_ended(Some(date)).is_some()),
    ];
    had_articles || had_fields.contains(&true)
}

/// Note des extraits retirés (05 §6.4).
fn snippets_note(hits: usize) -> String {
    format!(
        "Snippets were removed from {} to keep the response under 45,000 characters; use a \
         smaller page_size to get them.",
        the_last_hits(hits)
    )
}

/// Note des listes d'articles et champs optionnels retirés ; `etat` n'en fait pas partie quand
/// `keep_etat` (note de défense de `in_force_only` posée).
fn details_note(hits: usize, keep_etat: bool) -> String {
    let fields = if keep_etat { "title, nature and date" } else { "title, nature, etat and date" };
    format!(
        "Article lists and the optional fields {fields} were removed from {} to keep the response \
         under 45,000 characters; use a smaller page_size to get them.",
        the_last_hits(hits)
    )
}

/// Plus grande `page_size` dont les premiers résultats, ainsi dégradés, tiennent sous la cible :
/// déterministe, donc rejouable. Au moins 1, pour ne jamais proposer une page vide.
fn largest_page_size(hits: &[HitOut], fixed: usize) -> usize {
    hits.iter()
        .enumerate()
        .scan(fixed, |used, (index, out)| {
            // Une virgule sépare chaque résultat du précédent.
            *used = used.saturating_add(hit_cost(out)).saturating_add(usize::from(index > 0));
            Some(*used)
        })
        .take_while(|&used| used <= ResponseBudget::ESTIMATE_TARGET_CHARS)
        .count()
        .max(1)
}

/// Longueur, en caractères, du JSON compact d'un objet en construction : `{`, ses champs séparés
/// par des virgules, `}`. Les clés sont de l'ASCII.
#[derive(Default)]
struct JsonObject {
    len: usize,
    fields: usize,
}

impl JsonObject {
    /// Champ `"clé":valeur` dont la valeur occupe `value` caractères, guillemets compris.
    fn raw(&mut self, key: &str, value: usize) {
        // `"` + clé + `"` + `:` : trois caractères autour de la clé.
        self.len = self.len.saturating_add(key.len()).saturating_add(3).saturating_add(value);
        self.fields = self.fields.saturating_add(1);
    }

    /// Champ texte : la valeur entre guillemets, échappée comme `serde_json` l'écrit.
    fn text(&mut self, key: &str, value: &str) {
        self.raw(key, escaped_chars(value).saturating_add(2));
    }

    /// Champ texte présent seulement s'il y a une valeur.
    fn optional_text(&mut self, key: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.text(key, value);
        }
    }

    fn total(&self) -> usize {
        self.len.saturating_add(self.fields.saturating_sub(1)).saturating_add(2)
    }
}

/// Nombre de chiffres d'un entier écrit en décimal.
fn digits(number: u64) -> usize {
    number.to_string().len()
}

/// Longueur du JSON compact de l'enveloppe, `hits` vide : `total`, `page`, `has_more`, `hits`, plus
/// `total_articles` et `note` quand ils sont présents. Les résultats s'y ajoutent par
/// [`page_cost`].
fn envelope_cost(output: &SearchOutput) -> usize {
    let mut json = JsonObject::default();
    json.raw("total", digits(output.total));
    json.raw("page", digits(u64::from(output.page)));
    json.raw("has_more", if output.has_more { 4 } else { 5 });
    if let Some(articles) = output.total_articles {
        json.raw("total_articles", digits(articles));
    }
    json.raw("hits", 2);
    json.optional_text("note", output.note.as_deref());
    json.total()
}

/// Longueur du JSON compact des résultats, sans les crochets de la liste (ceux de
/// [`envelope_cost`]) : leurs objets et les virgules qui les séparent.
fn page_cost(hits: &[HitOut]) -> usize {
    hits.iter()
        .map(hit_cost)
        .fold(0, usize::saturating_add)
        .saturating_add(hits.len().saturating_sub(1))
}

/// Longueur du JSON compact d'un résultat, telle que la vue l'écrit (`views/search.rs`) : mêmes
/// champs, dans les mêmes conditions. `kind`, `id`, `citation` et `source_url` sont toujours
/// rendus ; `title` seulement s'il n'est pas dans la citation, `date` hors de la zone sentinelle,
/// le reste s'il est présent.
fn hit_cost(out: &HitOut) -> usize {
    let hit = &out.hit;
    let mut json = JsonObject::default();
    json.raw("kind", hit.fond.name().len().saturating_add(2));
    json.text("id", hit.id.as_str());
    json.optional_text("title", title_unless_in_citation(hit.title.as_deref(), &out.citation));
    if open_ended(hit.date).is_some() {
        json.raw("date", ISO_DATE_CHARS.saturating_add(2));
    }
    json.optional_text("nature", hit.nature.as_deref());
    json.optional_text("etat", hit.etat.as_ref().map(LegalStatus::as_api_code));
    if let Some(idcc) = &hit.idcc {
        json.text("idcc", &idcc.to_string());
    }
    json.optional_text("snippet", hit.snippet.as_ref().map(Snippet::as_str));
    if !hit.articles.is_empty() {
        json.raw("articles", articles_cost(&hit.articles));
    }
    json.text("citation", &out.citation);
    json.text("source_url", &out.source_url);
    json.total()
}

/// Longueur du JSON compact de la liste d'articles d'un résultat : `[`, ses objets séparés par des
/// virgules, `]`.
fn articles_cost(articles: &[CodeArticleRef]) -> usize {
    articles
        .iter()
        .map(|article| {
            let mut json = JsonObject::default();
            json.text("article_id", article.article_id.as_str());
            json.optional_text("num", article.num.as_deref());
            json.optional_text("snippet", article.snippet.as_ref().map(Snippet::as_str));
            json.total()
        })
        .fold(0, usize::saturating_add)
        .saturating_add(articles.len().saturating_sub(1))
        .saturating_add(2)
}

#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
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
        budget::ResponseBudget,
        error::ErrorCode,
        ports::CorpusError,
        testing::{FakeCorpus, FixedClock},
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
        corpus.seen_requests().remove(0).into_search().expect("the only request is a search")
    }

    const NOT_A_DATE: &str = "is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15).";
    const OUT_OF_RANGE: &str = "is outside the supported range (1500-01-01..2221-12-31).";

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
            in_force_only: None,
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
            "query is too long (612 characters, max 450); shorten it."
        );
    }

    const UNBALANCED: &str = "query has an unbalanced double quote (\"): close it or remove it.";

    #[tokio::test]
    async fn an_unbalanced_double_quote_is_refused_before_the_port_is_called() {
        // E2E search_all-1 : PISTE répond 503 (500 sur CODE) à ces formes.
        for odd in ["\"licenciement", "licenciement\"", "\"", "\"a\" \"b", "a \" b"] {
            for fond in ["LODA", "CODE", "JURI"] {
                let input = SearchInput { fond: Some(fond.to_owned()), ..text(odd) };
                let corpus = FakeCorpus::new();
                let error = run(&corpus, &clock(), input).await.unwrap_err();
                assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{odd:?} {fond}");
                assert_eq!(error.message(), UNBALANCED, "{odd:?} {fond}");
                assert_eq!(corpus.calls(), 0, "{odd:?} {fond}");
            }
        }
    }

    #[tokio::test]
    async fn the_unbalanced_quote_message_never_echoes_the_query() {
        let secret = "\"confidentiel-9f3a";
        let message = rejected(text(secret)).await;
        assert!(!message.contains("confidentiel"), "{message}");
    }

    #[tokio::test]
    async fn balanced_quotes_apostrophes_and_open_parentheses_still_search() {
        for fine in ["\"licenciement économique\"", "l'article (1240", "«licenciement", "bail"] {
            let corpus = serving(page_of(Fond::Loda, 0, vec![]));
            run(&corpus, &clock(), text(fine)).await.unwrap();
            assert_eq!(corpus.calls(), 1, "{fine:?}");
        }
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
        let bad = [
            ("2016-13-45", NOT_A_DATE),
            ("07/10/2016", NOT_A_DATE),
            ("", NOT_A_DATE),
            (" 2016-10-07", NOT_A_DATE),
            // Bien formées mais hors plage : le message le dit (L4-09, protocol-6).
            ("2999-01-01", OUT_OF_RANGE),
            ("1499-12-31", OUT_OF_RANGE),
        ];
        for (value, reason) in bad {
            let dates = [
                ("date_from", SearchInput { date_from: Some(value.to_owned()), ..text("bail") }),
                ("date_to", SearchInput { date_to: Some(value.to_owned()), ..text("bail") }),
                (
                    "version_date",
                    SearchInput { version_date: Some(value.to_owned()), ..text("bail") },
                ),
            ];
            for (name, input) in dates {
                assert_eq!(rejected(input).await, format!("{name}='{value}' {reason}"));
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

    /// D-08 : le vrai positif, `in_force_only` sur KALI arrive au port.
    #[tokio::test]
    async fn in_force_only_on_kali_reaches_the_port() {
        let corpus = serving(page_of(Fond::Kali, 0, vec![]));
        let input = SearchInput {
            fond: Some("kali".to_owned()),
            in_force_only: Some(true),
            ..text("convention collective")
        };
        run(&corpus, &clock(), input).await.unwrap();
        let query = sent(&corpus);
        assert_eq!((query.fond(), query.in_force_only()), (Fond::Kali, true));
        assert_eq!(query.version_date(), None);
    }

    /// Résultat KALI d'état `etat` (code d'API).
    fn kali_hit(n: u32, etat: &str) -> SearchHit {
        SearchHit { etat: LegalStatus::parse(etat), ..bare(Fond::Kali, n) }
    }

    /// `in_force_only` demandé sur KALI, la source ayant répondu `hits`.
    async fn in_force_search(flag: Option<bool>, hits: Vec<SearchHit>) -> SearchOutput {
        let total = u64::try_from(hits.len()).unwrap();
        let corpus = serving(page_of(Fond::Kali, total, hits));
        let input = SearchInput {
            fond: Some("KALI".to_owned()),
            in_force_only: flag,
            ..text("convention collective")
        };
        run(&corpus, &clock(), input).await.unwrap()
    }

    /// Défense en profondeur (revue finale de la PR 2) : la production ne peut pas être testée, et
    /// une source qui ignorerait la facette `LEGAL_STATUS` rendrait des textes qui ne sont pas en
    /// vigueur sans que rien ne le dise. Vrai positif : chaque état hors des trois gardés déclenche
    /// la note, même parmi des états gardés.
    #[tokio::test]
    async fn in_force_only_flags_a_hit_whose_etat_is_not_a_kept_one() {
        for etat in [
            "DENONCE",
            "ABROGE",
            "MODIFIE",
            "PERIME",
            "REMPLACE",
            "VIGUEUR_DIFF",
            "ABROGE_DIFF",
            "INITIALE",
            "SANS_ETAT",
            "SURPRISE",
        ] {
            let hits = vec![kali_hit(1, "VIGUEUR"), kali_hit(2, etat)];
            let output = in_force_search(Some(true), hits).await;
            assert_eq!(output.note.as_deref(), Some(FILTER_NOT_APPLIED_NOTE), "{etat}");
        }
    }

    /// Jumeaux sans note : les trois états gardés, un résultat sans état (rien ne dit qu'il est
    /// hors du filtre), un filtre non demandé, et un résultat hors filtre sur un filtre absent.
    #[tokio::test]
    async fn in_force_only_adds_no_note_when_every_known_etat_is_kept() {
        let kept = || {
            vec![
                kali_hit(1, "VIGUEUR"),
                kali_hit(2, "VIGUEUR_ETEN"),
                kali_hit(3, "VIGUEUR_NON_ETEN"),
                bare(Fond::Kali, 4),
            ]
        };
        assert_eq!(in_force_search(Some(true), kept()).await.note, None);
        let mixed = || vec![kali_hit(1, "VIGUEUR"), kali_hit(2, "DENONCE")];
        for flag in [None, Some(false)] {
            assert_eq!(in_force_search(flag, mixed()).await.note, None, "{flag:?}");
        }
    }

    /// Second tour de la revue finale : la note de défense dit « Check each etat » ; une page
    /// dégradée pour tenir sous la cible n'en retire donc pas l'`etat` (elle retire le reste des
    /// champs optionnels, en partant de la fin).
    #[tokio::test]
    async fn a_degraded_page_keeps_the_etat_of_every_hit_when_the_filter_note_is_set() {
        let hits = || -> Vec<SearchHit> {
            (1..=60)
                .map(|n| SearchHit {
                    nature: Some("x".repeat(800)),
                    ..kali_hit(n, if n == 60 { "DENONCE" } else { "VIGUEUR" })
                })
                .collect()
        };
        let search = |flag: Option<bool>| {
            let corpus = serving(page_of(Fond::Kali, 60, hits()));
            let input = SearchInput {
                fond: Some("KALI".to_owned()),
                in_force_only: flag,
                page_size: Some(60),
                ..text("convention collective")
            };
            async move { run(&corpus, &clock(), input).await.unwrap() }
        };

        let output = search(Some(true)).await;
        let trimmed = output.hits.iter().filter(|h| h.hit.nature.is_none()).count();
        assert!(trimmed > 0 && trimmed < 60, "{trimmed}");
        assert!(output.hits.iter().all(|h| h.hit.etat.is_some()));
        assert_eq!(output.hits[59].hit.etat, LegalStatus::parse("DENONCE"));
        let note = output.note.as_deref().unwrap();
        assert!(note.starts_with(FILTER_NOT_APPLIED_NOTE), "{note}");
        assert!(note.ends_with(&details_note(trimmed, true)), "{note}");

        // Jumeau sans filtre : la dégradation retire aussi l'`etat` des derniers résultats.
        let output = search(None).await;
        assert!(output.hits[59].hit.etat.is_none());
        assert!(!output.note.as_deref().unwrap().contains("in_force_only"));
    }

    /// La note de défense s'ajoute aux autres notes au lieu de les remplacer.
    #[tokio::test]
    async fn the_filter_note_is_joined_with_the_other_notes() {
        let corpus = serving(SearchPage::new(
            Fond::Kali,
            "bail",
            Some(10_001),
            None,
            vec![kali_hit(1, "DENONCE")],
            0,
            PaginationMode::Defaut,
        ));
        let input = SearchInput {
            fond: Some("KALI".to_owned()),
            in_force_only: Some(true),
            page: Some(1_000),
            page_size: Some(10),
            ..text("convention collective")
        };
        let output = run(&corpus, &clock(), input).await.unwrap();
        assert_eq!(
            output.note.as_deref(),
            Some(format!("{REACHABLE_NOTE} {FILTER_NOT_APPLIED_NOTE}").as_str())
        );
    }

    /// D-08 : `false` ou absent donne la requête de la 1.0.1, sur tous les fonds.
    #[tokio::test]
    async fn in_force_only_false_or_absent_changes_nothing() {
        for fond in Fond::ALL {
            for flag in [None, Some(false)] {
                let corpus = serving(page_of(fond, 0, vec![]));
                let input = SearchInput {
                    fond: Some(fond.name().to_owned()),
                    in_force_only: flag,
                    ..text("bail")
                };
                run(&corpus, &clock(), input).await.unwrap();
                assert!(!sent(&corpus).in_force_only(), "{fond} {flag:?}");
            }
        }
    }

    /// D-08 : tout autre fond, et le fond absent (LODA), refuse `in_force_only=true` en le disant.
    #[tokio::test]
    async fn in_force_only_is_rejected_outside_kali() {
        for fond in Fond::ALL.into_iter().filter(|fond| *fond != Fond::Kali) {
            let input = SearchInput {
                fond: Some(fond.name().to_owned()),
                in_force_only: Some(true),
                ..text("bail")
            };
            assert_eq!(
                rejected(input).await,
                format!(
                    "in_force_only only applies to fond KALI, not {fond}; set fond=KALI or remove it."
                ),
                "{fond}"
            );
        }
        let no_fond = SearchInput { in_force_only: Some(true), ..text("bail") };
        assert_eq!(
            rejected(no_fond).await,
            "in_force_only only applies to fond KALI, not LODA; set fond=KALI or remove it."
        );
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

    /// Page CODE d'un seul texte, `total_articles` articles trouvés dont `listed` sont listés,
    /// reçue dans le mode `mode`.
    fn code_page(total_articles: Option<u64>, listed: u32, mode: PaginationMode) -> SearchPage {
        let articles = (1..=listed)
            .map(|n| {
                CodeArticleRef::new(
                    ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
                    Some(n.to_string()),
                    "extrait",
                )
            })
            .collect();
        let hit = SearchHit { articles, ..bare(Fond::Code, 1) };
        SearchPage::new(Fond::Code, "bail", Some(1), total_articles, vec![hit], 0, mode)
    }

    /// Page `page` de taille `size` d'une recherche CODE servie par `served`.
    async fn code_paged(page: i64, size: i64, served: SearchPage) -> SearchOutput {
        let input = SearchInput {
            page: Some(page),
            page_size: Some(size),
            fond: Some("CODE".to_owned()),
            ..text("bail")
        };
        run(&serving(served), &clock(), input).await.unwrap()
    }

    /// L4-07 (journey_patrimoine-11, C12) : 1 texte, 16 articles, 5 listés par page.
    #[tokio::test]
    async fn a_code_hit_listing_fewer_articles_than_total_articles_has_more_in_article_mode() {
        let output = code_paged(1, 5, code_page(Some(16), 5, PaginationMode::Article)).await;
        assert!(output.has_more);
        assert_eq!(output.total, 1);
        assert_eq!(
            output.note.as_deref(),
            Some(
                "16 matching articles, 5 shown (page 1); request page=2 for the next articles \
                 of this code."
            )
        );
        let middle = code_paged(3, 5, code_page(Some(16), 5, PaginationMode::Article)).await;
        assert!(middle.has_more);
        assert!(middle.note.as_deref().is_some_and(|note| {
            note.ends_with("request page=4 for the next articles of this code.")
        }));
    }

    /// Le mode reçu ne compte pas : la page 1 de C12 demandée en `DEFAUT` répond `ARTICLE`, et
    /// les fixtures CODE enregistrées ne s'accordent pas sur le mode.
    #[tokio::test]
    async fn the_received_mode_does_not_decide_has_more_for_articles() {
        let output = code_paged(1, 5, code_page(Some(16), 5, PaginationMode::Defaut)).await;
        assert!(output.has_more);
        assert!(
            output
                .note
                .as_deref()
                .is_some_and(|note| note.starts_with("16 matching articles, 5 shown (page 1);"))
        );
    }

    /// Jumeau : sans `total_articles` (mode `DEFAUT` ordinaire) rien ne change, ni `has_more`
    /// ni note.
    /// `total_articles` n'a de sens que pour CODE : un autre fond qui le renseignerait au-delà de
    /// `page_size` n'obtient ni `has_more` ni note.
    #[tokio::test]
    async fn total_articles_of_a_fond_other_than_code_adds_no_more_and_no_note() {
        for fond in [Fond::Juri, Fond::Loda] {
            let page = SearchPage::new(
                fond,
                "bail",
                Some(2),
                Some(500),
                vec![bare(fond, 1), bare(fond, 2)],
                0,
                PaginationMode::Article,
            );
            let corpus = serving(page);
            let input = SearchInput { fond: Some(fond.name().to_owned()), ..text("bail") };
            let output = run(&corpus, &clock(), input).await.unwrap();
            assert!(!output.has_more, "{fond:?}");
            assert_eq!(output.note, None, "{fond:?}");
        }
    }

    #[tokio::test]
    async fn defaut_mode_ignores_total_articles() {
        let output = code_paged(1, 5, code_page(None, 5, PaginationMode::Defaut)).await;
        assert!(!output.has_more);
        assert_eq!(output.note, None);
    }

    /// Dernière page : 16 articles à 5 par page, la page 4 en porte un et n'a pas de suite ; une
    /// page pile à la fin non plus.
    #[tokio::test]
    async fn the_last_page_of_articles_has_no_follow_up() {
        let last = code_paged(4, 5, code_page(Some(16), 1, PaginationMode::Article)).await;
        assert!(!last.has_more);
        assert_eq!(last.note, None);
        let exact = code_paged(2, 5, code_page(Some(10), 5, PaginationMode::Article)).await;
        assert!(!exact.has_more);
        assert_eq!(exact.note, None);
    }

    /// La fenêtre de 10 000 s'applique aux articles comme aux textes.
    #[tokio::test]
    async fn the_reachable_window_applies_to_articles_too() {
        let before =
            code_paged(99, 100, code_page(Some(20_000), 100, PaginationMode::Article)).await;
        assert!(before.has_more);
        let end = code_paged(100, 100, code_page(Some(10_001), 100, PaginationMode::Article)).await;
        assert!(!end.has_more);
        assert_eq!(end.note.as_deref(), Some(REACHABLE_NOTE));
    }

    /// La note des articles s'ajoute aux notes de dégradation au lieu de les remplacer.
    #[tokio::test]
    async fn the_articles_note_is_joined_with_the_degradation_notes() {
        let mut page = code_page(Some(400), 5, PaginationMode::Article);
        page.hits.extend(
            (2..=100)
                .map(|n| SearchHit { snippet: Some(snippet(n as usize)), ..bare(Fond::Code, n) }),
        );
        let output = code_paged(1, 100, page).await;
        let note = output.note.unwrap();
        assert!(note.starts_with("400 matching articles, 5 shown (page 1);"), "{note}");
        assert!(note.contains("Snippets were removed"), "{note}");
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
                note: Some(NO_MATCH_NOTE.to_owned())
            }
        );
        assert_eq!(output.output_count_or_size(), 0);
    }

    /// D-10 : la note d'une recherche sans résultat reprend le texte de la fiche et tient très
    /// en deçà de la réserve des notes de dégradation.
    #[test]
    fn the_no_match_note_names_the_ways_out() {
        assert_eq!(
            NO_MATCH_NOTE,
            "No match: try other words, remove the date filters or call fr_coverage; an empty \
             result does not mean the law is silent."
        );
        assert!(NO_MATCH_NOTE.len() < DEGRADE_NOTE_RESERVE_CHARS / 2);
    }

    /// D-10, faux positif évité : une page vide dont le total est positif (page au-delà des
    /// résultats, fin de fenêtre) ne dit jamais « No match » : la loi n'est pas muette, la page
    /// est seulement vide. La note de fin de fenêtre reste la seule note.
    #[tokio::test]
    async fn an_empty_page_of_a_non_empty_search_gets_no_no_match_note() {
        let past_the_results = serving(page_of(Fond::Loda, 30, vec![]));
        let input = SearchInput { page: Some(5), ..text("bail") };
        let output = run(&past_the_results, &clock(), input).await.unwrap();
        assert_eq!((output.total, output.hits.len(), output.note), (30, 0, None));

        let past_the_window = serving(page_of(Fond::Loda, 50_000, vec![]));
        let input = SearchInput { page: Some(333), page_size: Some(30), ..text("bail") };
        let output = run(&past_the_window, &clock(), input).await.unwrap();
        assert_eq!(output.note.as_deref(), Some(REACHABLE_NOTE));
    }

    /// D-10, faux positif évité : des articles annoncés (CODE) sans texte listé ne sont pas
    /// « aucun résultat ».
    #[tokio::test]
    async fn an_empty_code_page_with_articles_announced_gets_no_no_match_note() {
        let page = SearchPage::new(
            Fond::Code,
            "bail",
            Some(0),
            Some(5),
            vec![],
            0,
            PaginationMode::Article,
        );
        let output = run(&serving(page), &clock(), text("bail")).await.unwrap();
        assert_eq!((output.total, output.total_articles, output.note), (0, Some(5), None));
    }

    /// D-10, faux positif évité : un résultat présent, même si la source annonce un total nul,
    /// n'est pas une recherche sans résultat.
    #[tokio::test]
    async fn a_listed_hit_gets_no_no_match_note_even_with_a_zero_total() {
        let output = run(
            &serving(page_of(Fond::Loda, 0, vec![bare(Fond::Loda, 1)])),
            &clock(),
            text("bail"),
        )
        .await
        .unwrap();
        assert_eq!((output.hits.len(), output.note), (1, None));
    }

    /// D-10, vrai positif conservé : la note vaut pour tout fond, et pour une page 2 vide d'une
    /// recherche qui n'a rien trouvé.
    #[tokio::test]
    async fn the_no_match_note_applies_to_every_fond_and_page() {
        for fond in ["LODA", "CODE", "JURI", "CNIL", "KALI", "ACCO"] {
            let parsed = Fond::parse(fond).unwrap();
            let input = SearchInput { fond: Some(fond.to_owned()), ..text("zzzz") };
            let output = run(&serving(page_of(parsed, 0, vec![])), &clock(), input).await.unwrap();
            assert_eq!(output.note.as_deref(), Some(NO_MATCH_NOTE), "{fond}");
        }
        let input = SearchInput { page: Some(2), ..text("zzzz") };
        let output = run(&serving(page_of(Fond::Loda, 0, vec![])), &clock(), input).await.unwrap();
        assert_eq!(output.note.as_deref(), Some(NO_MATCH_NOTE));
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
            Some(8),
            vec![first.clone(), second.clone()],
            1,
            PaginationMode::Article,
        );
        let corpus = serving(page);
        let input = SearchInput { fond: Some("JURI".to_owned()), ..text("bail") };
        let output = run(&corpus, &clock(), input).await.unwrap();

        assert_eq!((output.total, output.total_articles, output.page), (57, Some(8), 1));
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
                title: Some(format!("Loi n° {n} relative à l'organisation du service public")),
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

    /// Le coût d'un résultat est la longueur exacte de son JSON compact, champ par champ, dans
    /// l'ordre et aux conditions de la vue.
    #[test]
    fn hit_cost_is_the_length_of_the_compact_json_of_the_view() {
        let json_len = |json: &str| json.chars().count();
        // Nu : `kind`, `id`, `citation`, `source_url`.
        let bare_hit = plain(bare(Fond::Juri, 7));
        assert_eq!(
            hit_cost(&bare_hit),
            json_len(
                r#"{"kind":"juri","id":"JURITEXT000000000007","citation":"c","source_url":"u"}"#
            )
        );

        // Tous les champs optionnels, un extrait et deux articles (le second sans `num` ni
        // extrait). Le titre n'est pas dans la citation `c` : il est rendu.
        let full = plain(SearchHit {
            title: Some(" Titre ".to_owned()),
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
        assert_eq!(
            hit_cost(&full),
            json_len(concat!(
                r#"{"kind":"juri","id":"JURITEXT000000000007","title":"Titre","#,
                r#""date":"2024-01-02","nature":"ARRET","etat":"ABROGE","idcc":"275","#,
                r#""snippet":"abcdef","articles":[{"article_id":"LEGIARTI000006419288","#,
                r#""num":"9","snippet":"xyz"},{"article_id":"LEGIARTI000006419289"}],"#,
                r#""citation":"c","source_url":"u"}"#
            ))
        );
    }

    /// Ce que la vue omet n'est pas compté : un titre contenu dans la citation (blanc replié) et
    /// une date sentinelle. Un titre qui diffère de la citation, lui, l'est.
    #[test]
    fn hit_cost_ignores_what_the_view_omits() {
        let cost = |title: Option<&str>, citation: &str, date: Option<CivilDate>| {
            let hit = SearchHit { title: title.map(str::to_owned), date, ..bare(Fond::Juri, 1) };
            hit_cost(&HitOut { citation: citation.to_owned(), source_url: "u".to_owned(), hit })
        };
        let without = cost(None, "Loi X, art. 1", None);
        assert_eq!(cost(Some("Loi X"), "Loi X, art. 1", None), without);
        assert_eq!(cost(Some("Loi\nX"), "Loi X, art. 1", None), without);
        assert_eq!(cost(Some(" "), "Loi X, art. 1", None), without);
        // `,"title":"Loi Y"` : 16 caractères.
        assert_eq!(cost(Some("Loi Y"), "Loi X, art. 1", None), without + 16);
        // Une date sentinelle (2999-01-01) est omise, une date ordinaire coûte `,"date":"2024-01-02"`, 20 caractères.
        assert_eq!(cost(None, "Loi X, art. 1", CivilDate::new(2999, 1, 1)), without);
        assert_eq!(cost(None, "Loi X, art. 1", CivilDate::new(2024, 1, 2)), without + 20);
    }

    #[test]
    fn hit_cost_counts_json_escapes_in_every_text() {
        let plain_hit = |title: &str, snippet: &str, article: &str| {
            plain(SearchHit {
                title: Some(title.to_owned()),
                snippet: Snippet::new(snippet),
                articles: vec![CodeArticleRef::new(
                    ArticleId::parse("LEGIARTI000006419288").unwrap(),
                    Some(article.to_owned()),
                    "",
                )],
                ..bare(Fond::Code, 1)
            })
        };
        let flat = hit_cost(&plain_hit("aa", "bb", "cc"));
        // Un guillemet ou un antislash pèse deux caractères, un contrôle six : titre, extrait,
        // numéro d'article.
        assert_eq!(hit_cost(&plain_hit("a\"", "bb", "cc")), flat + 1);
        assert_eq!(hit_cost(&plain_hit("aa", "b\\", "cc")), flat + 1);
        assert_eq!(hit_cost(&plain_hit("aa", "bb", "\"\u{1}")), flat + 1 + 5);
        // Citation et URL (rendues telles que le contrat les donne).
        let quoted = HitOut { citation: "\"c\"".to_owned(), ..plain(bare(Fond::Juri, 2)) };
        let unquoted = HitOut { citation: "xcx".to_owned(), ..plain(bare(Fond::Juri, 2)) };
        assert_eq!(hit_cost(&quoted), hit_cost(&unquoted) + 2);
        let url = HitOut { source_url: "u\\".to_owned(), ..plain(bare(Fond::Juri, 2)) };
        let flat_url = HitOut { source_url: "ux".to_owned(), ..plain(bare(Fond::Juri, 2)) };
        assert_eq!(hit_cost(&url), hit_cost(&flat_url) + 1);
    }

    /// Cent résultats JURI tels que Légifrance les donne : titre-citation d'une soixantaine de
    /// caractères, date, nature, extrait de 280 caractères.
    fn typical_juri_hits(count: u32) -> Vec<SearchHit> {
        (1..=count)
            .map(|n| SearchHit {
                date: CivilDate::new(2020, 3, 12),
                nature: Some("ARRET".to_owned()),
                snippet: Some(snippet(n as usize)),
                ..titled(
                    n,
                    &format!("Cour de cassation, civile, Chambre civile 1, 12 mars 2020, {n}"),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn a_hundred_juri_hits_with_typical_citations_are_not_refused() {
        let output = big_page(typical_juri_hits(100), Fond::Juri, 100).await.unwrap();
        assert_eq!(output.hits.len(), 100, "aucun résultat n'est retiré");
        // Les extraits ne partent que des derniers résultats, et les premiers gardent le leur :
        // l'estimation à 64 par champ les retirait tous, puis refusait la page.
        let kept = output.hits.iter().filter(|h| h.hit.snippet.is_some()).count();
        assert!((1..100).contains(&kept), "{kept}");
        assert!(output.hits[..kept].iter().all(|h| h.hit.snippet.is_some()));
        assert!(output.hits[kept..].iter().all(|h| h.hit.snippet.is_none()));
        assert!(output.hits.iter().all(|h| h.hit.title.is_some()), "les détails restent");
        assert_eq!(output.note, Some(snippets_note(100 - kept)));
    }

    #[tokio::test]
    async fn a_hundred_pathological_hits_are_still_refused_with_a_page_size_that_fits() {
        // Jumeau « vrai positif conservé » : la page qui ne tient vraiment pas est refusée.
        let hits: Vec<SearchHit> = (1..=100).map(|n| titled(n, &"x".repeat(700))).collect();
        let error = big_page(hits, Fond::Juri, 100).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert!(error.message().starts_with("page_size=100 would exceed"));
    }

    #[test]
    fn notes_use_the_singular_for_one_hit() {
        assert_eq!(
            snippets_note(1),
            "Snippets were removed from the last hit to keep the response under 45,000 \
             characters; use a smaller page_size to get them."
        );
        assert_eq!(
            details_note(1, false),
            "Article lists and the optional fields title, nature, etat and date were removed from \
             the last hit to keep the response under 45,000 characters; use a smaller page_size \
             to get them."
        );
        // Le pluriel reste pour deux et plus.
        assert!(snippets_note(2).contains("from the last 2 hits to"));
        assert!(details_note(2, false).contains("from the last 2 hits to"));
        assert!(details_note(2, true).contains("fields title, nature and date were removed"));
    }

    #[test]
    fn only_the_last_hit_loses_its_snippet_and_the_note_says_the_last_hit() {
        let pair = |length: usize| {
            output_of(vec![
                plain(SearchHit { title: Some("x".repeat(length)), ..bare(Fond::Juri, 1) }),
                plain(bare(Fond::Juri, 2)),
            ])
        };
        // Sans extrait, la page tient pile à la cible ; l'extrait du second résultat la dépasse.
        // Le titre est rendu dès un caractère : la mesure part d'un titre d'un caractère.
        let one = pair(1);
        let fixed = envelope_cost(&one) + DEGRADE_NOTE_RESERVE;
        let mut output = pair(TARGET - fixed - page_cost(&one.hits) + 1);
        assert_eq!(fixed + page_cost(&output.hits), TARGET);
        output.hits[1].hit.snippet = Snippet::new("last");
        // Le premier n'en a pas : seul le dernier est touché.
        fit_under_target(&mut output, PageSize::new(2).unwrap(), false).unwrap();
        assert!(output.hits[1].hit.snippet.is_none());
        assert_eq!(output.note, Some(snippets_note(1)));
        assert!(output.note.as_deref().unwrap().contains("from the last hit to"));
    }

    #[test]
    fn degrade_notes_fit_in_their_reserve() {
        let both = format!("{} {}", snippets_note(100), details_note(100, false));
        assert!(both.chars().count() <= DEGRADE_NOTE_RESERVE_CHARS, "{both}");
        assert_eq!(
            snippets_note(7),
            "Snippets were removed from the last 7 hits to keep the response under 45,000 \
             characters; use a smaller page_size to get them."
        );
    }

    #[tokio::test]
    async fn snippets_are_removed_from_the_last_hits_when_over_the_estimate() {
        let output = big_page(with_snippets(100), Fond::Loda, 100).await.unwrap();
        // Aucun résultat n'est retiré.
        assert_eq!(output.hits.len(), 100);
        let removed = output.hits.iter().filter(|h| h.hit.snippet.is_none()).count();
        assert!(removed > 0 && removed < 100, "{removed}");
        // Ils sont retirés en partant du dernier, sans trou.
        let first_removed = output.hits.len() - removed;
        assert!(output.hits[..first_removed].iter().all(|h| h.hit.snippet.is_some()));
        assert!(output.hits[first_removed..].iter().all(|h| h.hit.snippet.is_none()));
        assert_eq!(output.note, Some(snippets_note(removed)));
        // Le titre, la citation et les champs d'identité restent.
        assert!(output.hits.iter().all(|h| h.hit.title.is_some() && !h.citation.is_empty()));
        // L'estimation passe sous la cible, et rendre son extrait au premier résultat dégradé la
        // ferait dépasser : on n'en retire pas plus que nécessaire.
        let shell = SearchOutput { hits: vec![], note: None, ..output.clone() };
        let fixed = envelope_cost(&shell) + DEGRADE_NOTE_RESERVE;
        assert!(fixed + page_cost(&output.hits) <= TARGET);
        let mut restored = output.hits.clone();
        restored[first_removed].hit.snippet = with_snippets(100)[first_removed].snippet.clone();
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
        // 40 résultats CODE : titre (donc citation) de 800 caractères, trois articles à extrait.
        let long = "mot ".repeat(200);
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
        assert_eq!(
            output.note,
            Some(format!("{} {}", snippets_note(40), details_note(trimmed, false)))
        );
    }

    #[tokio::test]
    async fn a_page_that_cannot_fit_even_stripped_is_invalid_arg_with_a_usable_page_size() {
        // Citation de 899 caractères : un résultat nu coûte 1 031 caractères de JSON (`kind` 13,
        // `id` 27, `citation` 912, `source_url` 74, trois virgules et deux accolades), plus une
        // virgule entre deux résultats ; l'enveloppe (49) et la réserve des notes (411) en prennent
        // 460 : 460 + 40 × 1 031 + 39 = 41 739 tient, 41 résultats font 42 771.
        let long = format!("{}fin", "mot ".repeat(224));
        let hits: Vec<SearchHit> = (1..=100)
            .map(|n| SearchHit { snippet: Snippet::new("extrait"), ..titled(n, &long) })
            .collect();
        let error = big_page(hits.clone(), Fond::Juri, 100).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(
            error.message(),
            "page_size=100 would exceed the 45,000-character response cap for this query; use \
             page_size <= 40."
        );
        // Règle rejouable : avec cette taille de page, la réponse tient.
        let replay = big_page(hits[..40].to_vec(), Fond::Juri, 40).await.unwrap();
        assert_eq!(replay.hits.len(), 40);
        // Et la taille suivante ne tient plus.
        let error = big_page(hits[..41].to_vec(), Fond::Juri, 41).await.unwrap_err();
        assert!(error.message().ends_with("use page_size <= 40."));
    }

    #[tokio::test]
    async fn one_hit_is_always_proposed_as_the_last_resort_page_size() {
        // Un seul résultat démesuré (hors bornes réelles) : la taille proposée ne tombe pas à 0.
        let huge = SearchHit { title: Some("x".repeat(60_000)), ..bare(Fond::Juri, 1) };
        let hit = HitOut { citation: "c".to_owned(), source_url: "u".to_owned(), hit: huge };
        assert_eq!(largest_page_size(&[hit], 0), 1);
    }

    /// Les virgules entre résultats comptent, une par résultat après le premier : trois résultats
    /// qui tiennent tout juste avec leurs deux virgules donnent 3 ; un caractère de plus, 2.
    #[test]
    fn the_largest_page_size_counts_one_comma_between_hits() {
        let hits: Vec<HitOut> = (1..=3).map(|n| plain(bare(Fond::Juri, n))).collect();
        let costs: usize = hits.iter().map(hit_cost).sum();
        let exact = ResponseBudget::ESTIMATE_TARGET_CHARS - costs - 2;
        assert_eq!(largest_page_size(&hits, exact), 3);
        assert_eq!(largest_page_size(&hits, exact + 1), 2);
    }

    /// La réserve des notes de dégradation vaut 411 caractères : le test de dérive de
    /// `lf-adapter-mcp` (`tests/budgets.rs`) épingle la même valeur de bout en bout. Elle couvre les
    /// deux notes au compteur le plus long, l'espace qui les joint et le champ `,"note":""`.
    #[test]
    fn the_degrade_note_reserve_covers_both_notes_and_the_note_field() {
        assert_eq!(DEGRADE_NOTE_RESERVE, 411);
        let notes = snippets_note(100).len() + details_note(100, false).len();
        assert!(notes + 1 + r#","note":"""#.len() <= DEGRADE_NOTE_RESERVE);
    }

    /// Un résultat rendu tel quel, à citation et URL courtes, pour mesurer les coûts.
    fn plain(hit: SearchHit) -> HitOut {
        HitOut { citation: "c".to_owned(), source_url: "u".to_owned(), hit }
    }

    fn output_of(hits: Vec<HitOut>) -> SearchOutput {
        SearchOutput { total: 1, page: 1, has_more: false, total_articles: None, hits, note: None }
    }

    fn article_ref(n: u32, snippet: &str) -> CodeArticleRef {
        CodeArticleRef::new(ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(), None, snippet)
    }

    /// La page qui tient pile à la cible est conservée telle quelle ; un caractère de plus et les
    /// champs optionnels sont retirés.
    #[test]
    fn a_page_exactly_at_the_target_is_not_degraded() {
        let with_title = |length: usize| {
            output_of(vec![plain(SearchHit {
                title: Some("x".repeat(length)),
                ..bare(Fond::Juri, 1)
            })])
        };
        // Un titre vide n'est pas rendu : la mesure part d'un titre d'un caractère.
        let one = with_title(1);
        let fixed = envelope_cost(&one) + DEGRADE_NOTE_RESERVE;
        let exact = TARGET - fixed - page_cost(&one.hits) + 1;

        let mut at_target = with_title(exact);
        assert_eq!(fixed + page_cost(&at_target.hits), TARGET);
        fit_under_target(&mut at_target, PageSize::new(1).unwrap(), false).unwrap();
        assert_eq!(at_target, with_title(exact));

        let mut over_target = with_title(exact + 1);
        fit_under_target(&mut over_target, PageSize::new(1).unwrap(), false).unwrap();
        assert_eq!(over_target.hits[0].hit.title, None);
        assert_eq!(over_target.note, Some(details_note(1, false)));
    }

    #[test]
    fn strip_snippets_reports_a_snippet_of_the_hit_or_of_one_of_its_articles() {
        let own = SearchHit { snippet: Snippet::new("abc"), ..bare(Fond::Code, 1) };
        let in_article = SearchHit {
            articles: vec![article_ref(1, ""), article_ref(2, "def")],
            ..bare(Fond::Code, 2)
        };
        let both = SearchHit {
            snippet: Snippet::new("abc"),
            articles: vec![article_ref(3, "def")],
            ..bare(Fond::Code, 3)
        };
        let own_with_bare_articles = SearchHit {
            snippet: Snippet::new("abc"),
            articles: vec![article_ref(4, "")],
            ..bare(Fond::Code, 4)
        };
        let nothing = SearchHit { articles: vec![article_ref(5, "")], ..bare(Fond::Code, 5) };
        for (hit, expected) in [
            (own, true),
            (in_article, true),
            (both, true),
            (own_with_bare_articles, true),
            (nothing, false),
        ] {
            let mut out = plain(hit);
            assert_eq!(strip_snippets(&mut out), expected);
            assert!(out.hit.snippet.is_none());
            assert!(out.hit.articles.iter().all(|article| article.snippet.is_none()));
        }
    }

    #[test]
    fn strip_details_reports_articles_or_optional_fields() {
        let fields = SearchHit { nature: Some("ARRET".to_owned()), ..bare(Fond::Juri, 1) };
        let articles = SearchHit { articles: vec![article_ref(1, "")], ..bare(Fond::Code, 2) };
        let both = SearchHit {
            date: CivilDate::new(2024, 1, 2),
            articles: vec![article_ref(2, "")],
            ..bare(Fond::Code, 3)
        };
        for (hit, expected) in
            [(fields, true), (articles, true), (both, true), (bare(Fond::Juri, 4), false)]
        {
            let mut out = plain(hit);
            assert_eq!(strip_details(&mut out, false), expected);
            assert_eq!(out.hit.articles, Vec::new());
            assert!(out.hit.title.is_none() && out.hit.nature.is_none());
            assert!(out.hit.etat.is_none() && out.hit.date.is_none());
        }
        // Un titre contenu dans la citation et une date sentinelle ne sont pas rendus : les retirer
        // ne gagne rien, le résultat n'est pas compté comme dégradé.
        let unseen = SearchHit {
            title: Some("c".to_owned()),
            date: CivilDate::new(2222, 1, 1),
            ..bare(Fond::Juri, 6)
        };
        let mut out = plain(unseen);
        assert!(!strip_details(&mut out, false));
        assert!(out.hit.title.is_none() && out.hit.date.is_none());
        // L'`idcc` identifie la convention : il reste.
        let mut kept = plain(SearchHit { idcc: Idcc::parse("275"), ..bare(Fond::Kali, 5) });
        assert!(!strip_details(&mut kept, false));
        assert!(kept.hit.idcc.is_some());
    }

    #[test]
    fn the_details_note_names_the_count_and_the_fields() {
        assert_eq!(
            details_note(7, false),
            "Article lists and the optional fields title, nature, etat and date were removed from \
             the last 7 hits to keep the response under 45,000 characters; use a smaller \
             page_size to get them."
        );
    }

    #[test]
    fn the_envelope_cost_is_the_length_of_its_compact_json_without_hits() {
        let cost =
            |total: u64, page: u32, more: bool, articles: Option<u64>, note: Option<&str>| {
                let output = SearchOutput {
                    total,
                    page,
                    has_more: more,
                    total_articles: articles,
                    hits: vec![],
                    note: note.map(str::to_owned),
                };
                envelope_cost(&output)
            };
        let len = |json: &str| json.chars().count();
        assert_eq!(
            cost(1, 1, false, None, None),
            len(r#"{"total":1,"page":1,"has_more":false,"hits":[]}"#)
        );
        assert_eq!(
            cost(12_345, 120, true, None, None),
            len(r#"{"total":12345,"page":120,"has_more":true,"hits":[]}"#)
        );
        assert_eq!(
            cost(0, 1, false, Some(1_000), None),
            len(r#"{"total":0,"page":1,"has_more":false,"total_articles":1000,"hits":[]}"#)
        );
        // La note est comptée échappée.
        assert_eq!(
            cost(1, 1, false, None, Some("a\"b")),
            len(r#"{"total":1,"page":1,"has_more":false,"hits":[],"note":"a\"b"}"#)
        );
        assert_eq!(
            cost(1, 1, true, Some(12), Some("abc")),
            len(
                r#"{"total":1,"page":1,"has_more":true,"total_articles":12,"hits":[],"note":"abc"}"#
            )
        );
    }

    #[test]
    fn the_page_cost_adds_a_comma_between_hits() {
        let one = plain(bare(Fond::Juri, 1));
        let alone = hit_cost(&one);
        assert_eq!(page_cost(&[]), 0);
        assert_eq!(page_cost(std::slice::from_ref(&one)), alone);
        assert_eq!(page_cost(&[one.clone(), one.clone(), one]), 3 * alone + 2);
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
