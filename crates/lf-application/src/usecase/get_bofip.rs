//! Cas d'usage de `fr_get_bofip` (jalon MB) : la lecture d'une publication du BOFiP
//! (`GetBofipUseCase`, 05 §5.15).
//!
//! # Validation
//!
//! Dans l'ordre de la référence, avant tout appel au port : `id` (requis, puis sa forme),
//! `publication_id`, puis les paramètres de lecture communs (05 §5.0.2).
//!
//! # Réponse du port
//!
//! Le port rend toutes les publications qui portent l'identifiant : une, parfois deux. Le cas
//! d'usage ne s'en remet pas à l'adapter pour ce qui protège le modèle : une publication d'un
//! autre identifiant, ou plus de dix publications (`PUBLICATIONS_MAX`), est une réponse non conforme
//! (`upstream_error`) ; une liste vide est un `not_found`.
//!
//! # Cas (05 §5.15)
//!
//! - `publication_id` donné : la publication de ce numéro est lue ; si aucune ne le porte,
//!   `not_found`, et le message nomme les numéros qui existent ;
//! - sans `publication_id`, une seule publication : elle est lue ;
//! - sans `publication_id`, plusieurs publications : leur liste, par numéro croissant, sans
//!   aucun texte, avec une note ;
//! - publication sans texte dans le jeu : ses métadonnées, `text_length` à 0, et une note qui
//!   renvoie à son adresse.
//!
//! Le résumé d'une publication longue est son début (`lead`) : le BOFiP ne publie pas de sommaire
//! et n'a pas de dispositif.
//!
//! # Articles cités (ADR-0032)
//!
//! Une publication lue porte les articles de code que cite son texte **entier**
//! ([`cited_articles`]) : la liste est la même à chaque fenêtre, quels que soient `detail`, `focus`
//! et `offset`. Le cas d'usage ne la vérifie pas : l'appelant le fait avec `fr_verify_citations`
//! puis `fr_diff_article`, et aucune requête ne part vers Légifrance.

use lf_domain::{
    doctrine::{BoiId, CitedArticles, DoctrineDoc, PublicationId, cited_articles},
    text::DispositifRule,
};

use crate::{
    budget::{Reading, ReadingRequest, read},
    error::AppError,
    failure::doctrine_failure,
    input::GetBofipInput,
    messages,
    output::get_bofip::{BofipChoice, BofipPublication, GetBofipOutput, PublicationRef},
    ports::{DoctrineError, TaxDoctrine},
    tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_GET_BOFIP;

/// Nombre maximal de publications admises pour un identifiant. Le portail en sert deux au plus
/// (mesure du 2026-11-15) : la marge couvre une évolution du jeu, pas une réponse démesurée.
pub(crate) const PUBLICATIONS_MAX: usize = 10;

/// Note de la liste des publications d'un même identifiant.
pub(crate) const SEVERAL_PUBLICATIONS_NOTE: &str = "Several publications share this id: call \
     fr_get_bofip again with publication_id set to one of those listed in publications.";

/// Note d'une publication sans texte dont l'adresse est rendue.
pub(crate) const NO_TEXT_NOTE: &str = "This publication has no text in the BOFiP open data set \
     (an annex, a form, a letter or a map): read it on the BOFiP site at source_url.";

/// Note d'une publication sans texte et sans adresse sûre à rendre.
pub(crate) const NO_TEXT_NO_URL_NOTE: &str = "This publication has no text in the BOFiP open \
     data set (an annex, a form, a letter or a map): look it up by its id on the BOFiP site.";

/// Détail de journal d'une publication qui porte un autre identifiant que celui demandé.
const FOREIGN_ID: &str = "a publication carries another id than the one requested";

/// Détail de journal d'un identifiant qui porterait trop de publications.
const TOO_MANY_PUBLICATIONS: &str = "more than 10 publications for one id";

/// Lit une publication du BOFiP par son identifiant, selon le mode de lecture demandé.
pub(crate) struct GetBofipUseCase<'a, D: TaxDoctrine> {
    doctrine: &'a D,
}

impl<'a, D: TaxDoctrine> GetBofipUseCase<'a, D> {
    /// Cas d'usage qui lit `doctrine`.
    pub(crate) fn new(doctrine: &'a D) -> Self {
        Self { doctrine }
    }

    /// Valide les arguments, lit les publications de l'identifiant, puis choisit et lit.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` si `id` est absent ou mal formé, si `publication_id` sort de ses bornes,
    ///   si un paramètre de lecture est refusé (aucun appel n'est fait au port), ou si `offset`
    ///   dépasse la fin du texte ;
    /// - `not_found` si aucune publication ne porte l'identifiant, ou aucune le numéro demandé ;
    /// - `upstream_error` pour une réponse non conforme, et les autres erreurs du port
    ///   (`rate_limited`, `config_error`), traduites par [`doctrine_failure`].
    pub(crate) async fn execute(&self, input: GetBofipInput) -> Result<GetBofipOutput, AppError> {
        let raw = input.id.ok_or_else(|| messages::required(messages::id_field(TOOL)))?;
        let id = BoiId::parse(&raw).map_err(|error| messages::domain(TOOL, &error))?;
        let wanted = publication_of(input.publication_id)?;
        let request = ReadingRequest::parse(&input.reading)?;
        let found = self.doctrine.get(&id).await;
        let docs = found.and_then(|docs| conforming(&id, docs));
        let docs = docs.map_err(|error| doctrine_failure(TOOL, id.as_str(), &error))?;
        select(id, docs, wanted, &request)
    }
}

/// `publication_id` : facultatif, de 1 à 9 999 999, `i64` hors de `u32` compris.
fn publication_of(value: Option<i64>) -> Result<Option<PublicationId>, AppError> {
    let Some(value) = value else { return Ok(None) };
    u32::try_from(value)
        .ok()
        .and_then(|number| PublicationId::new(number).ok())
        .map(Some)
        .ok_or_else(|| {
            messages::out_of_range(
                "publication_id",
                value,
                i64::from(PublicationId::MIN),
                i64::from(PublicationId::MAX),
            )
        })
}

/// Les publications rendues par le port, par numéro croissant, si la réponse est conforme : au
/// moins une publication, [`PUBLICATIONS_MAX`] au plus, toutes de l'identifiant demandé.
fn conforming(id: &BoiId, mut docs: Vec<DoctrineDoc>) -> Result<Vec<DoctrineDoc>, DoctrineError> {
    if docs.is_empty() {
        return Err(DoctrineError::NotFound);
    }
    if docs.len() > PUBLICATIONS_MAX {
        return Err(DoctrineError::Decode { detail: TOO_MANY_PUBLICATIONS.to_owned() });
    }
    if docs.iter().any(|doc| doc.id != *id) {
        return Err(DoctrineError::Decode { detail: FOREIGN_ID.to_owned() });
    }
    docs.sort_by_key(|doc| doc.publication_id);
    Ok(docs)
}

/// Choisit la publication à lire, ou rend la liste quand l'appel n'en désigne aucune.
fn select(
    id: BoiId,
    docs: Vec<DoctrineDoc>,
    wanted: Option<PublicationId>,
    request: &ReadingRequest,
) -> Result<GetBofipOutput, AppError> {
    let Some(number) = wanted else {
        return match <[DoctrineDoc; 1]>::try_from(docs) {
            Ok([doc]) => read_publication(doc, request),
            Err(docs) => Ok(GetBofipOutput::Choice(BofipChoice {
                id,
                publications: docs.into_iter().map(PublicationRef::from).collect(),
                note: SEVERAL_PUBLICATIONS_NOTE.to_owned(),
            })),
        };
    };
    let existing: Vec<PublicationId> = docs.iter().map(|doc| doc.publication_id).collect();
    let doc = docs
        .into_iter()
        .find(|doc| doc.publication_id == number)
        .ok_or_else(|| messages::doctrine_publication_not_found(&id, number, &existing))?;
    read_publication(doc, request)
}

/// Lit le texte de `doc` selon `request`, en tire les articles cités, puis le lâche : la sortie ne
/// porte que la lecture et la liste.
fn read_publication(
    mut doc: DoctrineDoc,
    request: &ReadingRequest,
) -> Result<GetBofipOutput, AppError> {
    let text = std::mem::take(&mut doc.text);
    let (reading, cited) = if text.is_empty() {
        (without_text(doc.source_url.is_some()), CitedArticles::default())
    } else {
        (read(&text, None, DispositifRule::None, request)?, cited_articles(text.as_str()))
    };
    Ok(GetBofipOutput::Publication(Box::new(BofipPublication { doc, reading, cited })))
}

/// Lecture d'une publication sans texte, quel que soit le mode demandé : rien à lire, et une
/// note qui dit où la trouver.
fn without_text(has_url: bool) -> Reading {
    let note = if has_url { NO_TEXT_NOTE } else { NO_TEXT_NO_URL_NOTE };
    Reading {
        summary: None,
        text: None,
        text_length: 0,
        next_offset: None,
        passages: None,
        note: Some(note.to_owned()),
        summary_truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use lf_domain::text::SummarySource;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        error::{ErrorCode, UpstreamCause},
        input::ReadingInput,
        testing::{
            DoctrineRequest, FakeDoctrine,
            fixtures::bofip::{self, ID},
        },
    };

    /// Texte court : rendu entier en mode résumé.
    const SHORT: &str = "Les donations entre époux sont soumises aux droits de mutation.";

    fn input(id: &str) -> GetBofipInput {
        GetBofipInput { id: Some(id.to_owned()), ..GetBofipInput::default() }
    }

    fn numbered(number: i64) -> GetBofipInput {
        GetBofipInput { publication_id: Some(number), ..input(ID) }
    }

    /// Publication 3350 de `paragraphs` paragraphes distincts, bien au-delà du seuil du résumé,
    /// et son texte canonique (celui que la lecture découpe).
    fn long_doc(paragraphs: usize) -> (DoctrineDoc, String) {
        let raw = (1..=paragraphs)
            .map(|n| format!("Paragraphe {n} : la donation ouvre droit à un abattement."))
            .collect::<Vec<_>>()
            .join("\n");
        let doc = bofip::doc(3350, &raw);
        let text = doc.text.as_str().to_owned();
        (doc, text)
    }

    async fn run(
        input: GetBofipInput,
        response: Result<Vec<DoctrineDoc>, DoctrineError>,
    ) -> (Result<GetBofipOutput, AppError>, FakeDoctrine) {
        let doctrine = FakeDoctrine::new();
        doctrine.push_get(response);
        let result = GetBofipUseCase::new(&doctrine).execute(input).await;
        (result, doctrine)
    }

    /// Message d'un argument refusé ; vérifie qu'aucun appel n'atteint le port.
    async fn rejected(input: GetBofipInput) -> String {
        let (result, doctrine) = run(input, Ok(vec![bofip::doc(3350, SHORT)])).await;
        let error = result.unwrap_err();
        assert_eq!((error.code(), doctrine.calls()), (Some(ErrorCode::InvalidArg), 0));
        error.message().to_owned()
    }

    /// La publication lue d'une sortie qui n'est pas une liste.
    fn publication(output: &GetBofipOutput) -> BofipPublication {
        output.publication().cloned().unwrap()
    }

    fn unexpected() -> AppError {
        AppError::Upstream {
            message: "unexpected response from the BOFiP portal; report it.".to_owned(),
            cause: UpstreamCause::Decode,
        }
    }

    #[tokio::test]
    async fn one_publication_is_read_and_its_text_is_dropped() {
        let (result, doctrine) =
            run(input(" boi-enr-dmtg-10-50-20 "), Ok(vec![bofip::doc(3350, SHORT)])).await;
        let output = result.unwrap();
        assert_eq!(output.output_count_or_size(), 63);
        let read = publication(&output);
        // Les métadonnées sont celles du port ; le texte est dans la lecture, plus dans `doc`.
        assert_eq!(read.doc, bofip::doc(3350, ""));
        assert_eq!(read.doc.citation(), "BOFiP, BOI-ENR-DMTG-10-50-20, 24 mai 2023");
        assert_eq!(
            read.reading,
            Reading {
                summary: None,
                text: Some(SHORT.to_owned()),
                text_length: 63,
                next_offset: None,
                passages: None,
                note: None,
                summary_truncated: false,
            }
        );
        // Un texte qui ne cite aucun article de code : liste vide.
        assert_eq!(read.cited, CitedArticles::default());
        // L'identifiant part validé : rogné et en majuscules.
        assert_eq!(doctrine.seen_requests(), vec![DoctrineRequest::Get(bofip::id())]);
    }

    #[tokio::test]
    async fn a_long_publication_is_summarized_by_its_beginning() {
        let (doc, text) = long_doc(60);
        let (result, _) = run(input(ID), Ok(vec![doc])).await;
        let read = publication(&result.unwrap());
        let summary = read.reading.summary.unwrap();
        assert_eq!(summary.source, SummarySource::Lead);
        assert!(summary.text.starts_with("Paragraphe 1 : la donation ouvre droit"));
        assert!(summary.text.chars().count() < text.chars().count());
        assert_eq!(read.reading.text, None);
        assert_eq!(read.reading.text_length, u32::try_from(text.chars().count()).unwrap());
        assert!(read.reading.summary_truncated);
    }

    #[tokio::test]
    async fn the_cited_articles_come_from_the_whole_text_whatever_the_window() {
        // Le premier et le dernier paragraphe citent un article : aucune fenêtre courte ne voit
        // les deux, et la liste les porte toujours, dans l'ordre du texte.
        let filler: Vec<String> = (1..=60)
            .map(|n| format!("Paragraphe {n} : la donation ouvre droit à un abattement."))
            .collect();
        let raw = format!(
            "L'article 1135 bis du code général des impôts s'applique.\n{}\nVoir l'article 9 du \
             code civil, puis de nouveau l'article 1135 bis du code général des impôts.",
            filler.join("\n")
        );
        let doc = bofip::doc(3350, &raw);
        let expected = CitedArticles {
            articles: vec![
                "Code général des impôts, art. 1135 bis".to_owned(),
                "Code civil, art. 9".to_owned(),
            ],
            truncated: false,
        };
        let length = doc.text.as_str().chars().count();
        for reading in [
            ReadingInput::default(),
            ReadingInput {
                detail: Some("full".to_owned()),
                max_chars: Some(500),
                ..ReadingInput::default()
            },
            ReadingInput {
                detail: Some("full".to_owned()),
                offset: Some(i64::try_from(length.saturating_sub(100)).unwrap()),
                ..ReadingInput::default()
            },
            ReadingInput { focus: Some("abattement".to_owned()), ..ReadingInput::default() },
        ] {
            let call = GetBofipInput { reading: reading.clone(), ..input(ID) };
            let (result, _) = run(call, Ok(vec![doc.clone()])).await;
            assert_eq!(publication(&result.unwrap()).cited, expected, "{reading:?}");
        }
    }

    #[tokio::test]
    async fn reading_parameters_select_a_window_or_passages() {
        let (doc, text) = long_doc(60);
        let full = GetBofipInput {
            reading: ReadingInput {
                detail: Some("full".to_owned()),
                max_chars: Some(500),
                ..ReadingInput::default()
            },
            ..input(ID)
        };
        let (result, _) = run(full, Ok(vec![doc.clone()])).await;
        let read = publication(&result.unwrap());
        let window = read.reading.text.unwrap();
        assert!(text.starts_with(&window) && window.chars().count() <= 500);
        assert!(read.reading.next_offset.is_some());

        let focused = GetBofipInput {
            reading: ReadingInput {
                focus: Some("abattement".to_owned()),
                ..ReadingInput::default()
            },
            ..input(ID)
        };
        let (result, _) = run(focused, Ok(vec![doc])).await;
        let passages = publication(&result.unwrap()).reading.passages.unwrap();
        assert_ne!(passages.len(), 0);
        assert!(passages.iter().all(|passage| passage.text.contains("abattement")));
    }

    #[tokio::test]
    async fn an_offset_past_the_end_is_refused_after_the_read() {
        let past = GetBofipInput {
            reading: ReadingInput { offset: Some(64), ..ReadingInput::default() },
            ..input(ID)
        };
        let (result, doctrine) = run(past, Ok(vec![bofip::doc(3350, SHORT)])).await;
        let error = result.unwrap_err();
        assert_eq!((error.code(), doctrine.calls()), (Some(ErrorCode::InvalidArg), 1));
        assert_eq!(
            error.message(),
            "offset=64 is at or past the end of the text (text_length=63); restart from offset=0 \
             or omit offset."
        );
    }

    #[tokio::test]
    async fn arguments_are_validated_in_order_before_any_call() {
        // Tous les arguments sont fautifs : chaque ligne corrige le premier refusé.
        let mut call = GetBofipInput {
            id: None,
            publication_id: Some(0),
            reading: ReadingInput { max_chars: Some(1), ..ReadingInput::default() },
        };
        assert_eq!(rejected(call.clone()).await, "id is required.");
        call.id = Some("LEGIARTI 000006419288".to_owned());
        assert_eq!(
            rejected(call.clone()).await,
            "id='LEGIARTI 000006419288' is not a BOFiP identifier (letters, digits and hyphens, \
             e.g. BOI-ENR-DMTG-10-50-20); find one with fr_search_bofip."
        );
        call.id = Some(ID.to_owned());
        assert_eq!(rejected(call.clone()).await, "publication_id=0 out of range (1..9999999).");
        call.publication_id = Some(10_000_000);
        assert_eq!(
            rejected(call.clone()).await,
            "publication_id=10000000 out of range (1..9999999)."
        );
        call.publication_id = Some(i64::MAX);
        assert_eq!(
            rejected(call.clone()).await,
            format!("publication_id={} out of range (1..9999999).", i64::MAX)
        );
        call.publication_id = Some(-1);
        assert_eq!(rejected(call.clone()).await, "publication_id=-1 out of range (1..9999999).");
        call.publication_id = Some(9_999_999);
        assert_eq!(rejected(call.clone()).await, "max_chars=1 out of range (500..40000).");
    }

    #[tokio::test]
    async fn two_publications_without_selector_are_listed() {
        // Rendues dans le désordre par le port : listées par numéro croissant, sans aucun texte.
        let docs = vec![bofip::doc(12_001, "Texte de la seconde."), bofip::doc(3350, SHORT)];
        let (result, _) = run(input(ID), Ok(docs.clone())).await;
        let output = result.unwrap();
        assert_eq!(output.output_count_or_size(), 2);
        let listed = |number: u32| PublicationRef::from(bofip::doc(number, ""));
        assert_eq!(
            output,
            GetBofipOutput::Choice(BofipChoice {
                id: bofip::id(),
                publications: vec![listed(3350), listed(12_001)],
                note: SEVERAL_PUBLICATIONS_NOTE.to_owned(),
            })
        );
        // Le numéro de l'une d'elles la lit.
        let (result, _) = run(numbered(12_001), Ok(docs.clone())).await;
        let read = publication(&result.unwrap());
        assert_eq!(read.doc, bofip::doc(12_001, ""));
        assert_eq!(read.reading.text.as_deref(), Some("Texte de la seconde."));
        let (result, _) = run(numbered(3350), Ok(docs)).await;
        assert_eq!(publication(&result.unwrap()).reading.text.as_deref(), Some(SHORT));
    }

    #[tokio::test]
    async fn wrong_publication_id_is_not_found() {
        // Une publication : le message nomme le numéro qui existe.
        let (result, doctrine) = run(numbered(9), Ok(vec![bofip::doc(3350, SHORT)])).await;
        assert_eq!(
            result.unwrap_err(),
            AppError::not_found(
                "No BOFiP publication 9 for BOI-ENR-DMTG-10-50-20: publication_id must be 3350."
            )
        );
        assert_eq!(doctrine.calls(), 1);
        // Deux publications : les deux numéros, par ordre croissant.
        let docs = vec![bofip::doc(12_001, SHORT), bofip::doc(3350, SHORT)];
        let (result, _) = run(numbered(9), Ok(docs)).await;
        assert_eq!(
            result.unwrap_err(),
            AppError::not_found(
                "No BOFiP publication 9 for BOI-ENR-DMTG-10-50-20: publication_id must be 3350 or \
                 12001."
            )
        );
        // Le bon numéro d'une publication unique la lit.
        let (result, _) = run(numbered(3350), Ok(vec![bofip::doc(3350, SHORT)])).await;
        assert_eq!(publication(&result.unwrap()).doc.publication_id.get(), 3350);
    }

    #[tokio::test]
    async fn publication_without_text_points_to_source_url() {
        let (result, _) = run(input(ID), Ok(vec![bofip::doc(3350, "")])).await;
        let output = result.unwrap();
        assert_eq!(output.output_count_or_size(), 0);
        let read = publication(&output);
        assert_eq!(read.doc, bofip::doc(3350, ""));
        assert!(read.doc.source_url.is_some());
        assert_eq!(
            read.reading,
            Reading {
                summary: None,
                text: None,
                text_length: 0,
                next_offset: None,
                passages: None,
                note: Some(NO_TEXT_NOTE.to_owned()),
                summary_truncated: false,
            }
        );
        assert_eq!(read.cited, CitedArticles::default());
        // Sans adresse sûre à rendre, la note ne renvoie pas à un champ absent.
        let unlinked = DoctrineDoc { source_url: None, ..bofip::doc(3350, "") };
        let (result, _) = run(input(ID), Ok(vec![unlinked])).await;
        assert_eq!(
            publication(&result.unwrap()).reading.note.as_deref(),
            Some(NO_TEXT_NO_URL_NOTE)
        );
    }

    #[tokio::test]
    async fn a_publication_without_text_ignores_the_reading_mode() {
        // Ni `offset` ni `focus` ne font d'erreur : il n'y a rien à lire.
        for reading in [
            ReadingInput { offset: Some(500), ..ReadingInput::default() },
            ReadingInput { detail: Some("full".to_owned()), ..ReadingInput::default() },
            ReadingInput { focus: Some("abattement".to_owned()), ..ReadingInput::default() },
        ] {
            let call = GetBofipInput { reading, ..input(ID) };
            let (result, _) = run(call, Ok(vec![bofip::doc(3350, "")])).await;
            let read = publication(&result.unwrap());
            assert_eq!(read.reading.note.as_deref(), Some(NO_TEXT_NOTE));
            assert_eq!((read.reading.text_length, read.reading.passages), (0, None));
        }
    }

    #[tokio::test]
    async fn no_publication_is_not_found() {
        let expected = AppError::not_found(
            "No BOFiP publication found for BOI-ENR-DMTG-10-50-20. Check the id, or search with \
             fr_search_bofip.",
        );
        let (result, _) = run(input(ID), Err(DoctrineError::NotFound)).await;
        assert_eq!(result.unwrap_err(), expected);
        // Une liste vide, que le port ne devrait pas rendre, est le même `not_found`.
        let (result, _) = run(numbered(3350), Ok(Vec::new())).await;
        assert_eq!(result.unwrap_err(), expected);
    }

    #[tokio::test]
    async fn a_publication_of_another_id_is_an_upstream_error() {
        let foreign = DoctrineDoc {
            id: BoiId::parse("BOI-ENR-DMTG-10-50-30").unwrap(),
            ..bofip::doc(7, SHORT)
        };
        // Seule, ou mêlée à une publication du bon identifiant.
        for docs in [vec![foreign.clone()], vec![bofip::doc(3350, SHORT), foreign.clone()]] {
            let (result, _) = run(input(ID), Ok(docs)).await;
            assert_eq!(result.unwrap_err(), unexpected());
        }
        // Même en la désignant par son numéro : elle n'est jamais lue.
        let (result, _) = run(numbered(7), Ok(vec![foreign])).await;
        assert_eq!(result.unwrap_err(), unexpected());
    }

    #[tokio::test]
    async fn too_many_publications_are_an_upstream_error() {
        let docs = |count: u32| (1..=count).map(|n| bofip::doc(n, SHORT)).collect::<Vec<_>>();
        // Dix publications : listées.
        let (result, _) = run(input(ID), Ok(docs(10))).await;
        assert_eq!(result.unwrap().output_count_or_size(), 10);
        // Onze : réponse non conforme, même avec un numéro qui existe.
        let (result, _) = run(input(ID), Ok(docs(11))).await;
        assert_eq!(result.unwrap_err(), unexpected());
        let (result, _) = run(numbered(1), Ok(docs(11))).await;
        assert_eq!(result.unwrap_err(), unexpected());
    }

    #[tokio::test]
    async fn port_errors_are_translated_for_the_reading_tool() {
        let (result, _) = run(input(ID), Err(DoctrineError::Disabled)).await;
        let error = result.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::ConfigError));
        assert_eq!(error.message(), messages::DOCTRINE_DISABLED_MESSAGE);
        let (result, _) = run(input(ID), Err(DoctrineError::Timeout)).await;
        assert_eq!(
            result.unwrap_err(),
            AppError::Upstream {
                message: "the BOFiP portal did not answer in time; retry once, then report it."
                    .to_owned(),
                cause: UpstreamCause::Timeout,
            }
        );
    }
}
