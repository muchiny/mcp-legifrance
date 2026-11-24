//! Cas d'usage de `fr_search_service_public` (jalon MS) : la recherche dans les fiches de
//! Service-Public (`SearchServicePublicUseCase`, 05 §5.16).
//!
//! # Validation
//!
//! Sans réseau et dans l'ordre de la référence : `query` (requise, puis réduite à ses mots : une
//! requête sans lettre ni chiffre est refusée ici), `audience`, `page_size` (1 à 50), `page`. Le
//! premier échec est rendu ; aucun appel au port ne part.
//!
//! # Réponse du port
//!
//! Le cas d'usage ne s'en remet pas à l'adapter pour ce qui protège le modèle : une page qui porte
//! plus de résultats que `page_size`, ou un résultat d'un public qui n'a pas été cherché, n'est
//! pas conforme (`upstream_error`). Les archives rendues (`source_files`) sont celles des publics
//! cherchés, dans leur ordre ; le port n'en donne que la date.
//!
//! # Plafond de réponse (05 §6.4)
//!
//! Comme pour le BOFiP : la taille de la page est calculée au caractère près, comme le JSON
//! compact de la vue l'écrira, plus une réserve pour ses champs constants
//! ([`GUIDANCE_CONSTANTS_RESERVE_CHARS`]), puis comparée à la cible de 42 000 caractères. Un titre
//! peut faire 1 000 caractères : les titres des derniers résultats sont raccourcis à 120
//! caractères, avec une `note`, sans jamais retirer un résultat. À 50 résultats au plus, la page
//! raccourcie tient toujours (test `the_worst_page_fits_once_its_titles_are_shortened`).

use std::num::NonZeroU32;

use lf_domain::{
    guidance::{AudienceFilter, GuidanceHit, GuidanceQuery, GuidanceTerms},
    search::{PageSize, has_more},
};

use super::search::{DEGRADE_NOTE_RESERVE, JsonObject, digits, join_notes, page_of};
use crate::{
    budget::{ResponseBudget, strip_from_end},
    error::AppError,
    failure::guidance_failure,
    input::SearchServicePublicInput,
    messages,
    output::search_service_public::{GUIDANCE_CONSTANTS_RESERVE_CHARS, SearchServicePublicOutput},
    ports::{ArchiveFile, GuidanceError, GuidanceFound, PublicGuidance},
    tools::ToolName,
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_SEARCH_SERVICE_PUBLIC;

/// `page_size` maximal (05 §5.16).
pub(crate) const PAGE_SIZE_MAX: u32 = 50;

/// Note d'une recherche sans résultat : ce que la recherche lit, ce que l'appelant peut changer,
/// et que le vide ne prouve rien.
pub(crate) const NO_MATCH_NOTE: &str = "No sheet or folder has these words in its title or \
     summary (the search does not read the full text). Try fewer or other words, \
     audience=\"entreprendre\" or \"all\", or browse a folder (N... id) with \
     fr_get_service_public; an empty result does not mean nothing is published on the subject.";

/// Détail de journal d'une page plus longue que demandé.
const TOO_MANY_HITS: &str = "more hits than the requested page_size";

/// Détail de journal d'un résultat venu d'un public qui n'a pas été cherché.
const FOREIGN_AUDIENCE: &str = "a hit comes from an audience that was not searched";

/// Longueur, en caractères, d'un titre raccourci par le plafond de réponse, `…` compris.
pub(crate) const TITLE_SHORT_CHARS: usize = 120;

/// Longueur ISO d'une date rendue, `AAAA-MM-JJ`.
const ISO_DATE_CHARS: usize = 10;

/// Recherche dans les fiches de Service-Public : analyse des arguments, un appel au port,
/// politique de réponse.
pub(crate) struct SearchServicePublicUseCase<'a, G: PublicGuidance> {
    guidance: &'a G,
}

impl<'a, G: PublicGuidance> SearchServicePublicUseCase<'a, G> {
    /// Cas d'usage qui interroge `guidance`.
    pub(crate) fn new(guidance: &'a G) -> Self {
        Self { guidance }
    }

    /// Valide `input`, interroge le port et met la page en forme.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour tout argument refusé (sans appel au port) ; `upstream_error` pour une
    /// page non conforme ; sinon l'erreur du port, traduite par [`guidance_failure`].
    pub(crate) async fn execute(
        &self,
        input: SearchServicePublicInput,
    ) -> Result<SearchServicePublicOutput, AppError> {
        let query = query_of(&input)?;
        let found = self.guidance.search(&query).await;
        let found = found.and_then(|found| conforming(&query, found));
        let found = found.map_err(|error| guidance_failure(TOOL, None, &error))?;
        Ok(present(&query, found))
    }
}

/// Requête validée du port, ou premier argument refusé.
fn query_of(input: &SearchServicePublicInput) -> Result<GuidanceQuery, AppError> {
    let raw = input.query.as_deref().ok_or_else(|| messages::required("query"))?;
    let terms = GuidanceTerms::parse(raw).map_err(|error| messages::domain(TOOL, &error))?;
    let audience = audience_of(input.audience.as_deref())?;
    let page_size = page_size_of(input.page_size)?;
    let page: NonZeroU32 = page_of(input.page)?;
    Ok(GuidanceQuery { terms, audience, page, page_size })
}

/// `audience` : `particuliers` par défaut, insensible à la casse et aux espaces de bord.
fn audience_of(value: Option<&str>) -> Result<AudienceFilter, AppError> {
    let Some(value) = value else { return Ok(AudienceFilter::default()) };
    AudienceFilter::parse(value)
        .ok_or_else(|| messages::not_one_of("audience", value, &AudienceFilter::NAMES))
}

/// `page_size` : 10 par défaut, de 1 à [`PAGE_SIZE_MAX`].
fn page_size_of(value: Option<i64>) -> Result<PageSize, AppError> {
    let Some(value) = value else { return Ok(PageSize::DEFAULT) };
    u32::try_from(value)
        .ok()
        .filter(|size| *size <= PAGE_SIZE_MAX)
        .and_then(|size| PageSize::new(size).ok())
        .ok_or_else(|| {
            messages::out_of_range(
                "page_size",
                value,
                i64::from(PageSize::MIN),
                i64::from(PAGE_SIZE_MAX),
            )
        })
}

/// La réponse du port si elle est conforme : pas plus de résultats que demandé, tous d'un public
/// cherché.
fn conforming(query: &GuidanceQuery, found: GuidanceFound) -> Result<GuidanceFound, GuidanceError> {
    let asked = usize::try_from(query.page_size.get()).unwrap_or(usize::MAX);
    if found.page.hits.len() > asked {
        return Err(GuidanceError::Decode { detail: TOO_MANY_HITS.to_owned() });
    }
    let searched = query.audience.audiences();
    if found.page.hits.iter().any(|hit| !searched.contains(&hit.audience)) {
        return Err(GuidanceError::Decode { detail: FOREIGN_AUDIENCE.to_owned() });
    }
    Ok(found)
}

/// Met la page du port en forme : suite, archives, note, plafond de réponse.
fn present(query: &GuidanceQuery, found: GuidanceFound) -> SearchServicePublicOutput {
    let GuidanceFound { page, files: dated } = found;
    let date_of = |audience| {
        dated.iter().find(|file| file.audience == audience).and_then(|file| file.last_modified)
    };
    let files = query
        .audience
        .audiences()
        .iter()
        .map(|&audience| ArchiveFile { audience, last_modified: date_of(audience) })
        .collect();
    let empty = page.total == 0 && page.hits.is_empty();
    let mut output = SearchServicePublicOutput {
        audience: query.audience,
        total: page.total,
        page: query.page.get(),
        has_more: has_more(query.page, query.page_size, page.total),
        hits: page.hits,
        files,
        note: empty.then(|| NO_MATCH_NOTE.to_owned()),
    };
    fit_under_target(&mut output);
    output
}

/// Raccourcit des titres, en partant du dernier résultat, jusqu'à ce que la taille de `output`
/// passe sous la cible (05 §6.4).
fn fit_under_target(output: &mut SearchServicePublicOutput) {
    let fixed = envelope_cost(output)
        .saturating_add(GUIDANCE_CONSTANTS_RESERVE_CHARS)
        .saturating_add(DEGRADE_NOTE_RESERVE);
    let over = |hits: &[GuidanceHit]| {
        fixed.saturating_add(page_cost(hits)) > ResponseBudget::ESTIMATE_TARGET_CHARS
    };
    let shortened = strip_from_end(&mut output.hits, over, shorten_title);
    let titles = (shortened > 0).then(|| titles_note(shortened));
    output.note = join_notes([output.note.take(), titles]);
}

/// Coupe le titre du résultat à [`TITLE_SHORT_CHARS`] caractères, `…` compris ; vrai s'il était
/// plus long.
fn shorten_title(hit: &mut GuidanceHit) -> bool {
    let long = hit.title.chars().count() > TITLE_SHORT_CHARS;
    if long {
        let kept = messages::clip(&hit.title, TITLE_SHORT_CHARS.saturating_sub(1));
        hit.title = format!("{kept}…");
    }
    long
}

/// Note des titres raccourcis : leur nombre, et comment les lire en entier.
fn titles_note(count: usize) -> String {
    let subject = if count == 1 {
        "1 long title was".to_owned()
    } else {
        format!("{count} long titles were")
    };
    format!(
        "{subject} shortened to keep the response under 45,000 characters; use a smaller \
         page_size, or fr_get_service_public, to get them whole."
    )
}

/// Longueur du JSON compact de l'enveloppe, `hits` vide et sans les champs constants de la vue :
/// `total`, `page`, `has_more`, `hits`, plus `note` quand elle est présente.
fn envelope_cost(output: &SearchServicePublicOutput) -> usize {
    let mut json = JsonObject::default();
    json.raw("total", digits(output.total));
    json.raw("page", digits(u64::from(output.page)));
    json.raw("has_more", if output.has_more { 4 } else { 5 });
    json.raw("hits", 2);
    json.optional_text("note", output.note.as_deref());
    json.total()
}

/// Longueur du JSON compact des résultats, sans les crochets de la liste : leurs objets et les
/// virgules qui les séparent.
fn page_cost(hits: &[GuidanceHit]) -> usize {
    hits.iter()
        .map(hit_cost)
        .fold(0, usize::saturating_add)
        .saturating_add(hits.len().saturating_sub(1))
}

/// Longueur du JSON compact d'un résultat, telle que la vue l'écrit : `id`, `audience`, `kind`,
/// `title` et `source_url` toujours ; `updated` s'il est connu.
fn hit_cost(hit: &GuidanceHit) -> usize {
    let mut json = JsonObject::default();
    json.text("id", hit.id.as_str());
    json.text("audience", hit.audience.name());
    json.text("kind", hit.kind.as_str());
    json.text("title", &hit.title);
    if hit.updated.is_some() {
        json.raw("updated", ISO_DATE_CHARS.saturating_add(2));
    }
    json.text("source_url", hit.source_url.as_str());
    json.total()
}

#[cfg(test)]
mod tests {
    use lf_domain::guidance::Audience;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        error::{ErrorCode, UpstreamCause},
        testing::{
            FakeGuidance, GuidanceRequest,
            fixtures::service_public::{archive_date, file, found, hit, query},
        },
    };

    fn words(query: &str) -> SearchServicePublicInput {
        SearchServicePublicInput { query: Some(query.to_owned()), ..Default::default() }
    }

    async fn run(
        input: SearchServicePublicInput,
        response: Result<GuidanceFound, GuidanceError>,
    ) -> (Result<SearchServicePublicOutput, AppError>, FakeGuidance) {
        let guidance = FakeGuidance::new();
        guidance.push_search(response);
        let result = SearchServicePublicUseCase::new(&guidance).execute(input).await;
        (result, guidance)
    }

    /// `count` résultats Particuliers, de numéros F1, F2…, tous de titre `title`.
    fn hits(count: u32, title: &str) -> Vec<GuidanceHit> {
        (1..=count).map(|n| hit(Audience::Particuliers, &format!("F{n}"), title)).collect()
    }

    #[tokio::test]
    async fn a_search_returns_the_page_with_the_archive_of_each_audience() {
        let all = SearchServicePublicInput {
            audience: Some(" ALL ".to_owned()),
            page_size: Some(2),
            ..words("compte bancaire")
        };
        let page = vec![
            hit(Audience::Particuliers, "F1437", "Compte bancaire"),
            hit(Audience::Entreprendre, "F31901", "Compte professionnel"),
        ];
        // Le port rend les archives dans un autre ordre, et une de trop : seules comptent les
        // dates des publics cherchés.
        let mut response = found(AudienceFilter::All, 3, page.clone());
        response.files.reverse();
        let (output, guidance) = run(all, Ok(response)).await;
        let output = output.unwrap();
        assert_eq!(
            output,
            SearchServicePublicOutput {
                audience: AudienceFilter::All,
                total: 3,
                page: 1,
                has_more: true,
                hits: page,
                files: vec![file(Audience::Particuliers), file(Audience::Entreprendre)],
                note: None,
            }
        );
        assert_eq!(output.output_count_or_size(), 2);
        let sent = GuidanceQuery {
            page_size: PageSize::new(2).unwrap(),
            ..query("compte bancaire", AudienceFilter::All)
        };
        assert_eq!(guidance.seen_requests(), [GuidanceRequest::Search(sent)]);
    }

    #[tokio::test]
    async fn particuliers_is_the_default_audience_and_a_missing_date_stays_unknown() {
        let mut response = found(AudienceFilter::All, 0, Vec::new());
        response.files.retain(|archive| archive.audience == Audience::Entreprendre);
        let (output, guidance) = run(words("logement"), Ok(response)).await;
        let output = output.unwrap();
        assert_eq!(output.audience, AudienceFilter::Particuliers);
        assert_eq!(
            output.files,
            [ArchiveFile { audience: Audience::Particuliers, last_modified: None }]
        );
        assert_eq!(
            guidance.seen_requests(),
            [GuidanceRequest::Search(query("logement", AudienceFilter::Particuliers))]
        );
        assert_eq!(archive_date().to_rfc3339(), "2026-11-24T14:26:09+00:00");
    }

    #[tokio::test]
    async fn no_match_says_what_the_search_reads_and_what_to_try() {
        let (output, _) =
            run(words("zzz"), Ok(found(AudienceFilter::Particuliers, 0, Vec::new()))).await;
        let output = output.unwrap();
        assert_eq!((output.total, output.has_more), (0, false));
        assert_eq!(output.note.as_deref(), Some(NO_MATCH_NOTE));
        // Une page vide au-delà de la fin d'une recherche qui a des résultats n'a pas de note.
        let beyond = SearchServicePublicInput { page: Some(9), ..words("compte") };
        let (output, guidance) =
            run(beyond, Ok(found(AudienceFilter::Particuliers, 4, Vec::new()))).await;
        let output = output.unwrap();
        assert_eq!((output.total, output.page, output.has_more, output.note), (4, 9, false, None));
        assert_eq!(guidance.calls(), 1);
    }

    #[tokio::test]
    async fn arguments_are_checked_in_order_before_any_call() {
        let without_words = messages::domain(TOOL, &GuidanceTerms::parse("?!").unwrap_err());
        let rows = [
            (SearchServicePublicInput::default(), "query is required."),
            (words(" ?! "), without_words.message()),
            (
                SearchServicePublicInput {
                    audience: Some("associations".to_owned()),
                    page_size: Some(0),
                    ..words("aide")
                },
                "audience='associations' must be one of ['all', 'entreprendre', 'particuliers'].",
            ),
            (
                SearchServicePublicInput { page_size: Some(51), page: Some(0), ..words("aide") },
                "page_size=51 out of range (1..50).",
            ),
            (
                SearchServicePublicInput { page_size: Some(0), ..words("aide") },
                "page_size=0 out of range (1..50).",
            ),
            (
                SearchServicePublicInput { page_size: Some(-3), ..words("aide") },
                "page_size=-3 out of range (1..50).",
            ),
            (
                SearchServicePublicInput { page: Some(0), ..words("aide") },
                "page=0 out of range (1..).",
            ),
        ];
        for (input, message) in rows {
            let guidance = FakeGuidance::new();
            let error =
                SearchServicePublicUseCase::new(&guidance).execute(input).await.unwrap_err();
            assert_eq!((error.code(), error.message()), (Some(ErrorCode::InvalidArg), message));
            assert_eq!(guidance.calls(), 0);
        }
    }

    #[tokio::test]
    async fn the_page_size_bounds_are_accepted() {
        for size in [1, 50] {
            let input = SearchServicePublicInput { page_size: Some(size), ..words("aide") };
            let (output, guidance) =
                run(input, Ok(found(AudienceFilter::Particuliers, 0, Vec::new()))).await;
            assert!(output.is_ok());
            let page_size = PageSize::new(u32::try_from(size).unwrap()).unwrap();
            let sent = GuidanceQuery { page_size, ..query("aide", AudienceFilter::Particuliers) };
            assert_eq!(guidance.seen_requests(), [GuidanceRequest::Search(sent)]);
        }
    }

    #[tokio::test]
    async fn a_page_longer_than_asked_or_from_another_audience_is_refused() {
        let long = SearchServicePublicInput { page_size: Some(1), ..words("compte") };
        let (error, _) =
            run(long, Ok(found(AudienceFilter::Particuliers, 2, hits(2, "Compte")))).await;
        let foreign = vec![hit(Audience::Entreprendre, "F31901", "Compte")];
        let (other, _) =
            run(words("compte"), Ok(found(AudienceFilter::Particuliers, 1, foreign))).await;
        for error in [error.unwrap_err(), other.unwrap_err()] {
            assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
            assert_eq!(error.cause(), Some(UpstreamCause::Decode));
            assert_eq!(error.message(), "the Service-Public archive could not be read; report it.");
        }
    }

    #[tokio::test]
    async fn port_errors_are_translated_without_the_search_words() {
        let (error, _) = run(words("allocation logement"), Err(GuidanceError::Downloading)).await;
        let error = error.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(
            error.message(),
            "Service-Public archive is still downloading; retry in a minute."
        );
        assert!(!error.message().contains("logement"));
        let (error, _) = run(words("aide"), Err(GuidanceError::Disabled)).await;
        assert_eq!(error.unwrap_err().code(), Some(ErrorCode::ConfigError));
    }

    /// Le pire cas : 50 résultats aux titres de 1 000 guillemets (2 000 caractères une fois
    /// échappés). Les derniers titres sont raccourcis jusqu'à tenir sous la cible ; aucun résultat
    /// n'est retiré.
    #[tokio::test]
    async fn the_worst_page_fits_once_its_titles_are_shortened() {
        let title = "\"".repeat(1_000);
        let input = SearchServicePublicInput { page_size: Some(50), ..words("aide") };
        let response = found(AudienceFilter::Particuliers, 9_999, hits(50, &title));
        let (output, _) = run(input, Ok(response)).await;
        let output = output.unwrap();
        assert_eq!(output.hits.len(), 50);
        let shortened = output.hits.iter().filter(|hit| hit.title.ends_with('…')).count();
        assert!(shortened > 0);
        // Les titres raccourcis sont les derniers, à la longueur fixée.
        for (index, hit) in output.hits.iter().enumerate() {
            let short = index >= 50 - shortened;
            assert_eq!(hit.title.chars().count(), if short { TITLE_SHORT_CHARS } else { 1_000 });
        }
        let size =
            envelope_cost(&output) + page_cost(&output.hits) + GUIDANCE_CONSTANTS_RESERVE_CHARS;
        assert!(size <= ResponseBudget::ESTIMATE_TARGET_CHARS, "{size}");
        assert_eq!(output.note, Some(titles_note(shortened)));
        assert!(output.note.unwrap().starts_with(&format!("{shortened} long titles were")));
    }

    /// Seuls les derniers titres sont raccourcis, et un titre court n'est pas compté.
    #[tokio::test]
    async fn only_the_last_long_titles_are_shortened() {
        let long = "a".repeat(1_000);
        let mut page = hits(40, &long);
        page.push(hit(Audience::Particuliers, "F99", "Court"));
        let input = SearchServicePublicInput { page_size: Some(50), ..words("aide") };
        let (output, _) = run(input, Ok(found(AudienceFilter::Particuliers, 41, page))).await;
        let output = output.unwrap();
        let shortened = output.hits.iter().filter(|hit| hit.title.ends_with('…')).count();
        assert!(shortened > 1 && shortened < 40, "{shortened}");
        assert_eq!(output.hits[0].title, long);
        assert_eq!(output.hits[40].title, "Court");
        assert_eq!(
            output.note,
            Some(format!(
                "{shortened} long titles were shortened to keep the response under 45,000 \
                 characters; use a smaller page_size, or fr_get_service_public, to get them whole."
            ))
        );
        let size =
            envelope_cost(&output) + page_cost(&output.hits) + GUIDANCE_CONSTANTS_RESERVE_CHARS;
        assert!(size <= ResponseBudget::ESTIMATE_TARGET_CHARS, "{size}");
    }

    #[test]
    fn one_shortened_title_is_named_in_the_singular() {
        assert_eq!(
            titles_note(1),
            "1 long title was shortened to keep the response under 45,000 characters; use a \
             smaller page_size, or fr_get_service_public, to get them whole."
        );
    }

    /// La taille calculée est celle du JSON compact que la vue écrira pour un résultat.
    #[test]
    fn a_hit_costs_its_compact_json() {
        let dated = hit(Audience::Entreprendre, "F31901", "Compte \"pro\"");
        let json = format!(
            r#"{{"id":"F31901","audience":"entreprendre","kind":"sheet","title":"Compte \"pro\"","updated":"2026-04-01","source_url":"{}"}}"#,
            dated.source_url
        );
        assert_eq!(hit_cost(&dated), json.chars().count());
        let undated = GuidanceHit { updated: None, ..dated };
        assert_eq!(hit_cost(&undated), json.chars().count() - r#","updated":"2026-04-01""#.len());
        assert_eq!(page_cost(&[]), 0);
    }
}
