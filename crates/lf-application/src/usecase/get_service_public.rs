//! Cas d'usage de `fr_get_service_public` (jalon MS) : la lecture d'une fiche ou d'une page de
//! navigation de Service-Public (`GetServicePublicUseCase`, 05 §5.17).
//!
//! # Validation
//!
//! Dans l'ordre de la référence, avant tout appel au port : `id` (requis, puis sa forme),
//! `audience`, puis les paramètres de lecture communs (05 §5.0.2).
//!
//! # Réponse du port
//!
//! Une page d'un autre numéro ou d'un autre public que ceux demandés, ou venue de l'archive d'un
//! autre public, n'est pas conforme (`upstream_error`). Un numéro absent de l'archive est un
//! `not_found` qui nomme l'autre public (`failure::guidance_failure`).
//!
//! # Cas (05 §5.17)
//!
//! - fiche : son texte est lu selon le mode demandé, puis retiré de la page ; le résumé officiel
//!   est la description, puis la liste des chapitres ([`GuidanceDoc::official_summary`]) ; ses
//!   références légales restent dans la page ;
//! - dossier ou thème : ses entrées, sans lecture.
//!
//! # Plafond de réponse (05 §6.4)
//!
//! La vue raccourcit la fenêtre de texte pour tenir sous le plafond dur ; les métadonnées, elles,
//! doivent tenir seules sous `METADATA_TARGET_CHARS` (20 000). Mesure du 2026-11-24 sur les deux
//! archives : 13 279 caractères de références au plus (F39649, 50 références), 7 516 d'entrées
//! (N31901). Au-delà, les titres des dernières références sont raccourcis à 120 caractères, avec
//! une note, sans jamais retirer une référence ; une page qui ne tient toujours pas se lit sur le
//! site (`EntryTooLarge`, le message donne son adresse).

use lf_domain::{
    guidance::{Audience, GuidanceContent, GuidanceDoc, LegalReference, NavigationEntry, SheetId},
    text::DispositifRule,
};

use super::search::{DEGRADE_NOTE_RESERVE, JsonObject, join_notes};
use crate::{
    budget::{ReadingRequest, read, strip_from_end},
    error::AppError,
    failure::guidance_failure,
    input::GetServicePublicInput,
    messages,
    output::{
        get_service_public::GetServicePublicOutput,
        search_service_public::GUIDANCE_CONSTANTS_RESERVE_CHARS,
    },
    ports::{GuidanceError, GuidanceRead, PublicGuidance},
    tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_GET_SERVICE_PUBLIC;

/// Cible, en caractères de JSON compact, des métadonnées d'une page : tout sauf la lecture du
/// texte, champs constants de la vue compris. Le reste du plafond dur va à la fenêtre de texte.
pub(crate) const METADATA_TARGET_CHARS: usize = 20_000;

/// Longueur, en caractères, du titre d'une référence raccourci par le plafond, `…` compris.
pub(crate) const REFERENCE_TITLE_SHORT_CHARS: usize = 120;

/// Réserve pour la clé de la liste (`legal_references` ou `entries`), ses crochets et sa virgule.
const LIST_FIELD_RESERVE: usize = 24;

/// Longueur ISO d'une date rendue, `AAAA-MM-JJ`, guillemets compris.
const ISO_DATE_FIELD: usize = 12;

/// Détail de journal d'une page qui n'est pas celle demandée.
const FOREIGN_PAGE: &str = "the page read is not the one requested (id, audience or archive)";

/// Lit une fiche ou une page de navigation de Service-Public, selon le mode de lecture demandé.
pub(crate) struct GetServicePublicUseCase<'a, G: PublicGuidance> {
    guidance: &'a G,
}

impl<'a, G: PublicGuidance> GetServicePublicUseCase<'a, G> {
    /// Cas d'usage qui lit `guidance`.
    pub(crate) fn new(guidance: &'a G) -> Self {
        Self { guidance }
    }

    /// Valide les arguments, lit la page, puis la met en forme.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` si `id` est absent ou mal formé, si `audience` ou un paramètre de lecture
    ///   est refusé (aucun appel n'est fait au port), ou si `offset` dépasse la fin du texte ;
    /// - `not_found` si le numéro n'est pas dans l'archive du public ; le message nomme l'autre ;
    /// - `upstream_error` pour une page non conforme ou trop grande, et les autres erreurs du
    ///   port (`rate_limited`, `config_error`), traduites par [`guidance_failure`].
    pub(crate) async fn execute(
        &self,
        input: GetServicePublicInput,
    ) -> Result<GetServicePublicOutput, AppError> {
        let raw = input.id.ok_or_else(|| messages::required(messages::id_field(TOOL)))?;
        let id = SheetId::parse(&raw).map_err(|error| messages::domain(TOOL, &error))?;
        let audience = audience_of(input.audience.as_deref())?;
        let request = ReadingRequest::parse(&input.reading)?;
        let found = self.guidance.get(audience, &id).await;
        let found = found.and_then(|found| conforming(&id, audience, found));
        let GuidanceRead { mut doc, file } =
            found.map_err(|error| guidance_failure(TOOL, Some((&id, audience)), &error))?;
        let note = fit_metadata(&mut doc)
            .map_err(|error| guidance_failure(TOOL, Some((&id, audience)), &error))?;
        let body = match &mut doc.content {
            GuidanceContent::Sheet { text, .. } => Some(std::mem::take(&mut text.text)),
            GuidanceContent::Folder(_) | GuidanceContent::Theme(_) => None,
        };
        let summary = doc.official_summary();
        let reading = body
            .map(|body| read(&body, Some(&summary), DispositifRule::None, &request))
            .transpose()?
            .map(|mut reading| {
                reading.note = join_notes([reading.note.take(), note]);
                reading
            });
        Ok(GetServicePublicOutput { doc, reading, file })
    }
}

/// `audience` : `particuliers` par défaut, insensible à la casse et aux espaces de bord ; `all`
/// n'a pas de sens pour une lecture.
fn audience_of(value: Option<&str>) -> Result<Audience, AppError> {
    let Some(value) = value else { return Ok(Audience::Particuliers) };
    Audience::parse(value).ok_or_else(|| messages::not_one_of("audience", value, &Audience::NAMES))
}

/// La page rendue par le port si c'est celle demandée : même numéro, même public, archive de ce
/// public.
fn conforming(
    id: &SheetId,
    audience: Audience,
    found: GuidanceRead,
) -> Result<GuidanceRead, GuidanceError> {
    if found.doc.id != *id || found.doc.audience != audience || found.file.audience != audience {
        return Err(GuidanceError::Decode { detail: FOREIGN_PAGE.to_owned() });
    }
    Ok(found)
}

/// Tient les métadonnées de `doc` sous [`METADATA_TARGET_CHARS`] : raccourcit les titres des
/// dernières références d'une fiche, puis refuse une page qui ne tient toujours pas. Rend la note
/// des titres raccourcis, s'il y en a.
fn fit_metadata(doc: &mut GuidanceDoc) -> Result<Option<String>, GuidanceError> {
    let fixed = metadata_cost(doc)
        .saturating_add(GUIDANCE_CONSTANTS_RESERVE_CHARS)
        .saturating_add(DEGRADE_NOTE_RESERVE)
        .saturating_add(LIST_FIELD_RESERVE);
    let over = |list: usize| fixed.saturating_add(list) > METADATA_TARGET_CHARS;
    let shortened = match &mut doc.content {
        GuidanceContent::Sheet { references, .. } => strip_from_end(
            references.references_mut(),
            |kept| over(list_cost(kept, reference_cost)),
            |reference| reference.shorten_title(REFERENCE_TITLE_SHORT_CHARS),
        ),
        GuidanceContent::Folder(_) | GuidanceContent::Theme(_) => 0,
    };
    let list = match &doc.content {
        GuidanceContent::Sheet { references, .. } => {
            list_cost(references.references(), reference_cost)
        }
        GuidanceContent::Folder(entries) | GuidanceContent::Theme(entries) => {
            list_cost(entries, entry_cost)
        }
    };
    if over(list) {
        return Err(GuidanceError::EntryTooLarge { source_url: doc.source_url.clone() });
    }
    Ok((shortened > 0).then(|| references_note(shortened)))
}

/// Note des titres de références raccourcis : leur nombre, et ce qui reste entier.
fn references_note(count: usize) -> String {
    let subject = if count == 1 {
        "1 long reference title was".to_owned()
    } else {
        format!("{count} long reference titles were")
    };
    format!(
        "{subject} shortened to keep the response under 45,000 characters; each reference keeps \
         its id and source_url."
    )
}

/// Longueur du JSON compact des métadonnées de `doc`, sans sa liste ni la lecture : `id`,
/// `audience`, `kind`, `title`, `citation` et `source_url` toujours ; `sheet_type`,
/// `description`, `theme`, `path`, `updated` et `legal_references_truncated` s'ils sont présents.
fn metadata_cost(doc: &GuidanceDoc) -> usize {
    let mut json = JsonObject::default();
    json.text("id", doc.id.as_str());
    json.text("audience", doc.audience.name());
    json.text("kind", doc.kind().as_str());
    json.optional_text("sheet_type", doc.sheet_type.as_deref());
    json.text("title", &doc.title);
    json.optional_text("description", doc.description.as_deref());
    json.optional_text("theme", doc.theme.as_deref());
    json.optional_texts("path", &doc.path);
    if doc.updated.is_some() {
        json.raw("updated", ISO_DATE_FIELD);
    }
    json.text("citation", &doc.citation());
    json.text("source_url", doc.source_url.as_str());
    if matches!(&doc.content, GuidanceContent::Sheet { references, .. } if references.truncated()) {
        json.raw("legal_references_truncated", 4);
    }
    json.total()
}

/// Longueur du JSON compact d'une liste, sans ses crochets : ses objets et leurs virgules.
fn list_cost<T>(items: &[T], cost: fn(&T) -> usize) -> usize {
    items
        .iter()
        .map(cost)
        .fold(0, usize::saturating_add)
        .saturating_add(items.len().saturating_sub(1))
}

/// Longueur du JSON compact d'une référence : `source_url` toujours, `id` et `title` s'ils sont
/// présents.
fn reference_cost(reference: &LegalReference) -> usize {
    let mut json = JsonObject::default();
    json.optional_text("id", reference.id());
    json.optional_text("title", reference.title());
    json.text("source_url", reference.source_url());
    json.total()
}

/// Longueur du JSON compact d'une entrée : `id`, `kind` et `title` toujours, `group` s'il est
/// présent.
fn entry_cost(entry: &NavigationEntry) -> usize {
    let mut json = JsonObject::default();
    json.text("id", entry.id.as_str());
    json.text("kind", entry.kind.as_str());
    json.text("title", &entry.title);
    json.optional_text("group", entry.group.as_deref());
    json.total()
}

#[cfg(test)]
mod tests {
    use lf_domain::{guidance::legal_references, text::SummarySource};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        error::{ErrorCode, UpstreamCause},
        input::ReadingInput,
        ports::ArchiveFile,
        testing::{
            FakeGuidance, GuidanceRequest,
            fixtures::service_public::{
                ARTICLE_URL, DESCRIPTION, entry, file, folder, id, read as found, sheet,
                sheet_citing,
            },
        },
    };

    fn asking(raw: &str) -> GetServicePublicInput {
        GetServicePublicInput { id: Some(raw.to_owned()), ..GetServicePublicInput::default() }
    }

    async fn run(
        input: GetServicePublicInput,
        response: Result<GuidanceRead, GuidanceError>,
    ) -> (Result<GetServicePublicOutput, AppError>, FakeGuidance) {
        let guidance = FakeGuidance::new();
        guidance.push_get(response);
        let result = GetServicePublicUseCase::new(&guidance).execute(input).await;
        (result, guidance)
    }

    /// Une fiche de `count` références, aux titres de `title_chars` caractères.
    fn with_references(count: u64, title_chars: usize) -> GuidanceDoc {
        let title = "t".repeat(title_chars);
        let urls: Vec<String> = (0..count)
            .map(|n| format!("https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI{n:012}"))
            .collect();
        let references =
            legal_references(urls.iter().map(|url| (Some(title.as_str()), url.as_str())));
        sheet_citing(Audience::Particuliers, "F1437", "Texte.", references)
    }

    /// Les références de la fiche `doc`.
    fn references_of(doc: &GuidanceDoc) -> &[LegalReference] {
        doc.legal_references().unwrap().references()
    }

    #[tokio::test]
    async fn a_sheet_is_read_with_its_official_summary_and_keeps_its_references() {
        let doc = sheet(Audience::Particuliers, "F1437", "Le solde est versé.");
        let input = GetServicePublicInput {
            audience: Some(" Particuliers ".to_owned()),
            ..asking("f1437")
        };
        let (output, guidance) = run(input, Ok(found(doc.clone()))).await;
        let output = output.unwrap();
        // Un texte court est rendu entier, avec le résumé officiel : la description, puis les
        // chapitres.
        let reading = output.reading.clone().unwrap();
        assert_eq!(reading.text.as_deref(), Some("Bénéficiaires\n\nLe solde est versé."));
        let summary = reading.summary.clone().unwrap();
        assert_eq!(summary.source, SummarySource::Official);
        assert_eq!(summary.text, format!("{DESCRIPTION}\n\nChapitres : Bénéficiaires"));
        assert_eq!(reading.note, None);
        // Le texte est retiré de la page ; le reste est celui du port.
        assert_eq!(output.doc.text().map(lf_domain::text::LongText::as_str), Some(""));
        assert_eq!((&output.doc.title, output.doc.citation()), (&doc.title, doc.citation()));
        assert_eq!(references_of(&output.doc)[0].source_url(), ARTICLE_URL);
        assert_eq!(output.file, file(Audience::Particuliers));
        let returned = 34 + summary.text.chars().count();
        assert_eq!(output.output_count_or_size(), u64::try_from(returned).unwrap());
        assert_eq!(
            guidance.seen_requests(),
            [GuidanceRequest::Get(Audience::Particuliers, id("F1437"))]
        );
    }

    /// Le résumé d'une fiche longue est le résumé officiel : la description, puis les chapitres.
    #[tokio::test]
    async fn a_long_sheet_is_summarised_by_its_description_and_chapters() {
        let doc = sheet(Audience::Entreprendre, "F2408", &"Un paragraphe assez long. ".repeat(400));
        let input =
            GetServicePublicInput { audience: Some("entreprendre".to_owned()), ..asking("F2408") };
        let (output, _) = run(input, Ok(found(doc))).await;
        let summary = output.unwrap().reading.unwrap().summary.unwrap();
        assert_eq!(summary.source, SummarySource::Official);
        assert_eq!(summary.text, format!("{DESCRIPTION}\n\nChapitres : Bénéficiaires"));
    }

    #[tokio::test]
    async fn a_folder_lists_its_entries_without_reading() {
        let entries = vec![entry("F1437", "Compte"), entry("F1903", "Temps de travail")];
        let doc = folder(Audience::Particuliers, "N31675", entries.clone());
        let input = GetServicePublicInput {
            reading: ReadingInput { detail: Some("full".to_owned()), ..ReadingInput::default() },
            ..asking("N31675")
        };
        let (output, _) = run(input, Ok(found(doc))).await;
        let output = output.unwrap();
        assert_eq!(output.reading, None);
        assert_eq!(output.doc.content, GuidanceContent::Folder(entries));
        assert_eq!((output.output_count_or_size(), output.entry()), (2, "N31675.xml".to_owned()));
    }

    #[tokio::test]
    async fn a_missing_number_names_the_other_audience() {
        for (audience, raw, other) in [
            (Audience::Particuliers, "F31901", "entreprendre"),
            (Audience::Entreprendre, "F1437", "particuliers"),
        ] {
            let input =
                GetServicePublicInput { audience: Some(audience.name().to_owned()), ..asking(raw) };
            let (error, _) = run(input, Err(GuidanceError::NotFound)).await;
            let error = error.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::NotFound));
            assert_eq!(
                error.message(),
                format!(
                    "No Service-Public sheet or folder {raw} for {}. Try audience {other}, or \
                     search with fr_search_service_public.",
                    audience.name()
                )
            );
        }
    }

    #[tokio::test]
    async fn an_entry_too_large_gives_the_public_address() {
        let source_url = sheet(Audience::Particuliers, "F37096", "").source_url;
        let (error, _) =
            run(asking("F37096"), Err(GuidanceError::EntryTooLarge { source_url })).await;
        let error = error.unwrap_err();
        assert_eq!(
            (error.code(), error.cause()),
            (Some(ErrorCode::UpstreamError), Some(UpstreamCause::Decode))
        );
        assert_eq!(
            error.message(),
            "this Service-Public page is too large to read here; read it at \
             https://www.service-public.gouv.fr/particuliers/vosdroits/F37096."
        );
    }

    #[tokio::test]
    async fn arguments_are_checked_in_order_before_any_call() {
        let malformed = messages::domain(TOOL, &SheetId::parse("X12").unwrap_err());
        let rows = [
            (GetServicePublicInput::default(), "id is required."),
            (
                GetServicePublicInput { audience: Some("all".to_owned()), ..asking("X12") },
                malformed.message(),
            ),
            (
                GetServicePublicInput {
                    audience: Some("all".to_owned()),
                    reading: ReadingInput { max_chars: Some(1), ..ReadingInput::default() },
                    ..asking("F1437")
                },
                "audience='all' must be one of ['entreprendre', 'particuliers'].",
            ),
            (
                GetServicePublicInput {
                    reading: ReadingInput { max_chars: Some(1), ..ReadingInput::default() },
                    ..asking("F1437")
                },
                "max_chars=1 out of range (500..40000).",
            ),
        ];
        for (input, message) in rows {
            let guidance = FakeGuidance::new();
            let error = GetServicePublicUseCase::new(&guidance).execute(input).await.unwrap_err();
            assert_eq!((error.code(), error.message()), (Some(ErrorCode::InvalidArg), message));
            assert_eq!(guidance.calls(), 0);
        }
    }

    #[tokio::test]
    async fn an_offset_beyond_the_text_is_refused_after_the_read() {
        let input = GetServicePublicInput {
            reading: ReadingInput { offset: Some(9_999), ..ReadingInput::default() },
            ..asking("F1437")
        };
        let (error, guidance) =
            run(input, Ok(found(sheet(Audience::Particuliers, "F1437", "Court.")))).await;
        assert_eq!(error.unwrap_err().code(), Some(ErrorCode::InvalidArg));
        assert_eq!(guidance.calls(), 1);
    }

    /// Une page d'un autre numéro, d'un autre public, ou venue de l'archive de l'autre public,
    /// n'est pas conforme.
    #[tokio::test]
    async fn a_page_that_is_not_the_one_requested_is_refused() {
        let wrong_id = found(sheet(Audience::Particuliers, "F1438", ""));
        let wrong_audience = found(sheet(Audience::Entreprendre, "F1437", ""));
        let wrong_archive = GuidanceRead {
            file: ArchiveFile { audience: Audience::Entreprendre, last_modified: None },
            ..found(sheet(Audience::Particuliers, "F1437", ""))
        };
        for response in [wrong_id, wrong_audience, wrong_archive] {
            let (error, _) = run(asking("F1437"), Ok(response)).await;
            let error = error.unwrap_err();
            assert_eq!(error.cause(), Some(UpstreamCause::Decode));
            assert_eq!(error.message(), "the Service-Public archive could not be read; report it.");
        }
    }

    #[tokio::test]
    async fn real_sized_references_are_kept_whole() {
        // 50 références aux titres de 150 caractères : environ 13 700 caractères de JSON, un
        // peu plus que la plus grosse fiche mesurée (13 279, F39649).
        let doc = with_references(50, 150);
        let size = list_cost(references_of(&doc), reference_cost);
        assert!(size > 13_279, "{size}");
        let (output, _) = run(asking("F1437"), Ok(found(doc))).await;
        let output = output.unwrap();
        let references = references_of(&output.doc);
        assert!(references.iter().all(|r| r.title().unwrap().chars().count() == 150));
        assert_eq!(output.reading.unwrap().note, None);
    }

    /// Cinquante références aux titres de 1 000 caractères : les derniers titres sont
    /// raccourcis, jamais retirés, et la page tient sous la cible.
    #[tokio::test]
    async fn long_reference_titles_are_shortened_from_the_end() {
        let (output, _) = run(asking("F1437"), Ok(found(with_references(50, 1_000)))).await;
        let output = output.unwrap();
        let kept = references_of(&output.doc);
        assert_eq!(kept.len(), 50);
        let shortened = kept.iter().filter(|r| r.title().unwrap().ends_with('…')).count();
        assert!(shortened > 1);
        for (index, reference) in kept.iter().enumerate() {
            let short = index >= 50 - shortened;
            let wanted = if short { REFERENCE_TITLE_SHORT_CHARS } else { 1_000 };
            assert_eq!(reference.title().unwrap().chars().count(), wanted);
            assert!(reference.id().is_some());
        }
        let list = list_cost(kept, reference_cost);
        let fixed = metadata_cost(&output.doc)
            + GUIDANCE_CONSTANTS_RESERVE_CHARS
            + DEGRADE_NOTE_RESERVE
            + LIST_FIELD_RESERVE;
        assert!(fixed + list <= METADATA_TARGET_CHARS);
        assert_eq!(output.reading.unwrap().note, Some(references_note(shortened)));
    }

    #[test]
    fn one_shortened_reference_title_is_named_in_the_singular() {
        assert_eq!(
            references_note(1),
            "1 long reference title was shortened to keep the response under 45,000 characters; \
             each reference keeps its id and source_url."
        );
        assert!(references_note(3).starts_with("3 long reference titles were"));
    }

    /// Des métadonnées qui ne tiennent pas, même titres raccourcis, ou des entrées trop
    /// nombreuses : la page se lit sur le site.
    #[tokio::test]
    async fn a_page_whose_metadata_cannot_fit_is_read_on_the_site() {
        let mut long = sheet(Audience::Particuliers, "F1437", "Texte.");
        long.description = Some("d".repeat(METADATA_TARGET_CHARS));
        let many: Vec<NavigationEntry> =
            (0..2_000).map(|n| entry(&format!("F{n}"), "Une fiche")).collect();
        let crowded = folder(Audience::Particuliers, "N1437", many.clone());
        let theme = GuidanceDoc {
            content: GuidanceContent::Theme(many),
            ..folder(Audience::Particuliers, "N19803", Vec::new())
        };
        for (raw, doc) in [("F1437", long), ("N1437", crowded), ("N19803", theme)] {
            let (error, _) = run(asking(raw), Ok(found(doc))).await;
            let error = error.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
            assert_eq!(
                error.message(),
                format!(
                    "this Service-Public page is too large to read here; read it at \
                     https://www.service-public.gouv.fr/particuliers/vosdroits/{raw}."
                )
            );
        }
    }

    /// Les tailles calculées sont celles du JSON compact que la vue écrira.
    #[test]
    fn references_and_entries_cost_their_compact_json() {
        let found = legal_references([
            (Some("Code \"civil\""), ARTICLE_URL),
            (None, "https://www.legifrance.gouv.fr/jorf/id/JORFTEXT000000886460"),
        ]);
        let (with_id, without) = (&found.references()[0], &found.references()[1]);
        let json = format!(
            r#"{{"id":"LEGIARTI000032041571","title":"Code \"civil\"","source_url":"{ARTICLE_URL}"}}"#
        );
        assert_eq!(reference_cost(with_id), json.chars().count());
        assert_eq!(
            reference_cost(without),
            r#"{"source_url":"https://www.legifrance.gouv.fr/jorf/id/JORFTEXT000000886460"}"#.len()
        );
        assert_eq!(
            list_cost(found.references(), reference_cost),
            reference_cost(with_id) + 1 + reference_cost(without)
        );
        let grouped =
            NavigationEntry { group: Some("Banque".to_owned()), ..entry("F1437", "Compte") };
        assert_eq!(
            entry_cost(&grouped),
            r#"{"id":"F1437","kind":"sheet","title":"Compte","group":"Banque"}"#.len()
        );
        assert_eq!(list_cost::<NavigationEntry>(&[], entry_cost), 0);
    }

    /// Les métadonnées comptent chaque champ présent, et le drapeau des références coupées.
    #[test]
    fn metadata_cost_follows_the_fields_present() {
        let full = sheet(Audience::Particuliers, "F1437", "Texte.");
        let bare = GuidanceDoc {
            sheet_type: None,
            description: None,
            theme: None,
            path: Vec::new(),
            updated: None,
            ..full.clone()
        };
        let extra = format!(
            r#","sheet_type":"Fiche d'information","description":"{DESCRIPTION}","theme":"Argent","path":["Argent","Banque"],"updated":"2026-04-01""#
        );
        let citation_gap = full.citation().chars().count() - bare.citation().chars().count();
        assert_eq!(
            metadata_cost(&full) - metadata_cost(&bare),
            extra.chars().count() + citation_gap
        );
        let truncated = with_references(51, 10);
        let fifty = with_references(50, 10);
        assert_eq!(
            metadata_cost(&truncated) - metadata_cost(&fifty),
            r#","legal_references_truncated":true"#.len()
        );
    }
}
