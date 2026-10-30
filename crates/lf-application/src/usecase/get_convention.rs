//! Cas d'usage de `fr_get_convention` (v0.1) : la lecture d'une convention collective (`GetConventionUseCase`).
//!
//! Même table des matières que `fr_get_act` (`toc_offset`, `toc_limit`, `next_toc_offset`, 05
//! §5.0.4), mais aucune date : l'API ne versionne pas `/consult/kaliText` et l'outil n'a pas de
//! paramètre `date` (00 §7). Ordre de validation : `convention_id`, `toc_offset`, `toc_limit`.

use lf_domain::{contract::Nor, entity::Convention, id::KaliTextId};

use super::get_act::{TocRequest, text_chars};
use crate::{
    budget::estimate_chars, error::AppError, failure::port_failure, input::GetConventionInput,
    messages, output::get_convention::GetConventionOutput, ports::LegalCorpus, tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_GET_CONVENTION;

/// Champs de la réponse hors table des matières, pour l'estimation (`convention_id`, `idcc`,
/// `nor`, `citation`, `source_url`, `toc`, `toc_total`...).
const CONVENTION_FIELDS: usize = 14;

/// Caractères des champs de taille fixe de la réponse : identifiant, URL de la page, IDCC, NOR,
/// date et état.
const CONVENTION_FIXED_CHARS: usize = 200;

/// Cas d'usage de `fr_get_convention` : une convention collective et une page de sa table des
/// matières.
pub(crate) struct GetConventionUseCase<'a, C: LegalCorpus> {
    corpus: &'a C,
}

impl<'a, C: LegalCorpus> GetConventionUseCase<'a, C> {
    /// Cas d'usage sur `corpus`.
    pub(crate) fn new(corpus: &'a C) -> Self {
        Self { corpus }
    }

    /// Valide `input`, lit la convention et en rend une page de table des matières.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour un argument refusé (sans appel au port), l'erreur du port traduite par
    /// [`port_failure`] sinon, `invalid_arg` pour un `toc_offset` au-delà de la fin de la table.
    pub(crate) async fn execute(
        &self,
        input: GetConventionInput,
    ) -> Result<GetConventionOutput, AppError> {
        let id = convention_id(input.convention_id.as_deref())?;
        let request = TocRequest::parse(input.toc_offset, input.toc_limit)?;
        let mut convention = self
            .corpus
            .get_convention(&id)
            .await
            .map_err(|error| port_failure(TOOL, id.as_str(), &error))?;
        // La table est lue une fois, puis lâchée : la sortie ne porte que la page rendue.
        let toc = std::mem::take(&mut convention.body.toc);
        let window = request.window(TOOL, &toc, metadata_cost(&convention))?;
        Ok(GetConventionOutput {
            convention,
            toc: window.lines,
            toc_total: window.total,
            next_toc_offset: window.next_offset,
        })
    }
}

/// `convention_id` : absent ou blanc, `required` ; sinon un `KALITEXT`.
fn convention_id(raw: Option<&str>) -> Result<KaliTextId, AppError> {
    let raw = raw
        .filter(|raw| !raw.trim().is_empty())
        .ok_or_else(|| messages::required("convention_id"))?;
    KaliTextId::parse(raw).map_err(|error| messages::domain(TOOL, &error))
}

/// Estimation des champs de la réponse hors table des matières (05 §6.4).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn metadata_cost(convention: &Convention) -> usize {
    let body = &convention.body;
    let texts = [
        body.title.as_deref(),
        body.nor.as_ref().map(Nor::as_str),
        convention.container_title.as_deref(),
        convention.bulletin.as_deref(),
    ];
    estimate_chars(CONVENTION_FIELDS, CONVENTION_FIXED_CHARS + text_chars(texts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audited::AuditMetric,
        budget::{ResponseBudget, estimate_chars},
        error::ErrorCode,
        input::GetConventionInput,
        ports::CorpusError,
        testing::{FakeCorpus, PortRequest},
        usecase::get_act::fixtures::toc,
    };
    use lf_domain::{
        contract::{Idcc, Nor},
        entity::{Convention, TextBody, Toc},
        id::KaliTextId,
        status::LegalStatus,
    };
    use pretty_assertions::assert_eq;

    const ID: &str = "KALITEXT000049878950";

    fn convention(toc: Toc) -> Convention {
        Convention {
            text_id: KaliTextId::parse(ID).unwrap(),
            body: TextBody {
                title: Some("Annexe IX - Stipulations spécifiques".into()),
                nor: Nor::parse("ASET2450455M"),
                etat: LegalStatus::parse("VIGUEUR_ETEN"),
                toc,
                ..TextBody::default()
            },
            idcc: Idcc::parse("2121"),
            container_title: Some("Convention collective nationale de l'édition".into()),
            container_id: Some("KALICONT000005635096".into()),
            bulletin: Some("BO n°2024-20".into()),
        }
    }

    fn input(convention_id: &str) -> GetConventionInput {
        GetConventionInput { convention_id: Some(convention_id.to_owned()), ..Default::default() }
    }

    fn corpus_with(convention: Convention) -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_get_convention(Ok(convention));
        corpus
    }

    async fn run(
        corpus: &FakeCorpus,
        input: GetConventionInput,
    ) -> Result<crate::output::get_convention::GetConventionOutput, crate::error::AppError> {
        GetConventionUseCase::new(corpus).execute(input).await
    }

    #[tokio::test]
    async fn convention_is_read_by_its_normalized_identifier() {
        let corpus = corpus_with(convention(toc(1, 1, "Chapitre")));

        let output = run(&corpus, input(" kalitext000049878950 ")).await.unwrap();

        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetConvention(KaliTextId::parse(ID).unwrap())]
        );
        assert_eq!(output.convention.text_id.as_str(), ID);
        assert_eq!(output.convention.idcc, Idcc::parse("2121"));
        assert_eq!((output.toc.len(), output.toc_total, output.next_toc_offset), (2, 2, None));
    }

    #[tokio::test]
    async fn convention_rejects_bad_arguments_in_order_without_network() {
        let corpus = FakeCorpus::new();
        let cases: [(GetConventionInput, &str); 5] = [
            (GetConventionInput::default(), "convention_id is required."),
            (input(" "), "convention_id is required."),
            (
                input("LEGITEXT000033205014"),
                "convention_id='LEGITEXT000033205014' must be a Legifrance KALITEXT... \
                 identifier. LEGITEXT... identifies a whole text: use fr_get_act.",
            ),
            (
                GetConventionInput { toc_offset: Some(-1), ..input(ID) },
                "toc_offset=-1 out of range (0..4294967295).",
            ),
            (
                GetConventionInput { toc_limit: Some(501), ..input(ID) },
                "toc_limit=501 out of range (1..500).",
            ),
        ];
        for (wanted, message) in cases {
            let error = run(&corpus, wanted.clone()).await.unwrap_err();
            assert_eq!(
                (error.code(), error.message()),
                (Some(ErrorCode::InvalidArg), message),
                "{wanted:?}"
            );
        }
        // L'identifiant parle avant les bornes de la table.
        let both = GetConventionInput { toc_limit: Some(0), ..input("LEGITEXT000033205014") };
        let error = run(&corpus, both).await.unwrap_err();
        assert_eq!(error.message().split('=').next(), Some("convention_id"));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn toc_page_bounds() {
        let all = || corpus_with(convention(toc(4, 21, "Titre")));
        let window = |offset: Option<i64>, limit: Option<i64>| GetConventionInput {
            toc_offset: offset,
            toc_limit: limit,
            ..input(ID)
        };

        let output = run(&all(), window(None, None)).await.unwrap();
        assert_eq!((output.toc.len(), output.toc_total, output.next_toc_offset), (88, 88, None));

        let output = run(&all(), window(Some(10), Some(20))).await.unwrap();
        assert_eq!(
            (output.toc.len(), output.toc_total, output.next_toc_offset),
            (20, 88, Some(30))
        );

        let output = run(&all(), window(Some(68), Some(20))).await.unwrap();
        assert_eq!((output.toc.len(), output.next_toc_offset), (20, None));

        let error = run(&all(), window(Some(300), None)).await.unwrap_err();
        assert_eq!(
            (error.code(), error.message()),
            (
                Some(ErrorCode::InvalidArg),
                "toc_offset=300 is past the end of the table of contents (toc_total=88); \
                 restart from toc_offset=0."
            )
        );
    }

    #[tokio::test]
    async fn toc_shrunk_under_estimate_with_next_offset() {
        let title = "T".repeat(100);
        let corpus = corpus_with(convention(toc(500, 0, &title)));
        let wanted = GetConventionInput { toc_limit: Some(500), ..input(ID) };

        let output = run(&corpus, wanted).await.unwrap();

        let kept = output.toc.len();
        assert!(0 < kept && kept < 500, "{kept}");
        assert_eq!(output.toc_total, 500);
        assert_eq!(output.next_toc_offset, Some(u32::try_from(kept).unwrap()));
        let per_entry = estimate_chars(2, 100 + 4);
        assert!(kept * per_entry <= ResponseBudget::ESTIMATE_TARGET_CHARS, "{kept}");
    }

    #[tokio::test]
    async fn output_metric_counts_the_returned_toc_entries() {
        let corpus = corpus_with(convention(toc(2, 3, "Titre")));
        let wanted = GetConventionInput { toc_limit: Some(5), ..input(ID) };

        let output = run(&corpus, wanted).await.unwrap();

        assert_eq!(output.output_count_or_size(), 5);
    }

    #[tokio::test]
    async fn port_failures_become_the_get_convention_messages() {
        let corpus = FakeCorpus::new();
        corpus.push_get_convention(Err(CorpusError::NotFound { what: "x".to_owned() }));

        let error = run(&corpus, input(ID)).await.unwrap_err();

        assert_eq!(
            (error.code(), error.message()),
            (
                Some(ErrorCode::NotFound),
                "No collective agreement text found for KALITEXT000049878950. Check the id, or \
                 search with fr_search."
            )
        );
    }
}
