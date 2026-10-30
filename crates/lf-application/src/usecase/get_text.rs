//! Cas d'usage de `fr_get_text` (v0.1) : la lecture d'un article (`GetTextUseCase`).
//!
//! Ordre de validation (05 §5.0.2, §5.3) : l'identifiant (`article_id`, `LEGIARTI` ou `KALIARTI`),
//! puis les paramètres de lecture dans l'ordre de [`ReadingRequest::parse`] (`detail`, `offset`,
//! `max_chars`, `focus`, `passages`), tous avant l'appel au port. L'article lu passe ensuite par
//! [`read`] : le résumé d'un article est son début (`lead`), sans sommaire officiel ni dispositif.
//!
//! La lecture est raccourcie quand ses caractères, avec les métadonnées, dépasseraient la cible du
//! budget (05 §6.4, étape 4) : la fenêtre est relue avec un `max_chars` plus petit, donc coupée
//! comme toute fenêtre et avec un `next_offset` exact. L'adapter MCP mesure ensuite la taille
//! exacte, échappements JSON compris.

use lf_domain::{
    contract::Citable,
    entity::ArticleText,
    id::ArticleId,
    text::{DispositifRule, LongText, window},
};

use crate::{
    budget::{Reading, ReadingRequest, ResponseBudget, estimate_chars, read},
    error::AppError,
    failure::port_failure,
    input::{GetTextInput, ReadingInput},
    messages,
    output::get_text::{GetTextOutput, NOTA_MAX_CHARS, returned_chars},
    ports::LegalCorpus,
    tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_GET_TEXT;

/// Champs de la réponse hors lecture : `article_id`, `num`, `etat`, `code_title`, `parent_id`,
/// `path`, `date_debut`, `date_fin`, `nota`, `eli`, `citation`, `source_url`.
const ARTICLE_FIELDS: usize = 12;

/// Champs de la lecture : `summary`, `summary_source`, `text`, `text_length`, `next_offset`,
/// `passages` et `note`.
const READING_FIELDS: usize = 7;

/// Éléments comptés par passage : l'élément de la liste, `offset` et `text`.
const PASSAGE_FIELDS: usize = 3;

/// Caractères des champs de taille fixe de l'article : deux identifiants, deux dates, l'état et
/// l'URL de la page.
const ARTICLE_FIXED_CHARS: usize = 200;

/// Cas d'usage de `fr_get_text` : un article, en résumé, en fenêtre de texte ou en passages.
pub(crate) struct GetTextUseCase<'a, C: LegalCorpus> {
    corpus: &'a C,
}

impl<'a, C: LegalCorpus> GetTextUseCase<'a, C> {
    /// Cas d'usage qui lit `corpus`.
    pub(crate) fn new(corpus: &'a C) -> Self {
        Self { corpus }
    }

    /// Valide `input`, lit l'article et le lit selon le mode demandé.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` pour un argument refusé (identifiant, `detail`, `offset`, `max_chars`,
    ///   `focus` ou `passages`) : aucun appel n'est fait au port ;
    /// - l'erreur du port traduite par [`port_failure`] (`not_found`, `upstream_error`,
    ///   `rate_limited`, `config_error`) ;
    /// - `invalid_arg` pour un `offset` au-delà de la fin du texte, qu'on ne connaît qu'une fois
    ///   l'article lu.
    pub(crate) async fn execute(&self, input: GetTextInput) -> Result<GetTextOutput, AppError> {
        let id = article_id(input.article_id.as_deref())?;
        let request = ReadingRequest::parse(&input.reading)?;
        let mut article = self
            .corpus
            .get_article(&id)
            .await
            .map_err(|error| port_failure(TOOL, id.as_str(), &error))?;
        // Le texte et le `nota` sont lus une fois, puis lâchés : la sortie porte la lecture et le
        // `nota` borné, pas les textes entiers.
        let text = std::mem::take(&mut article.text);
        let nota = article.nota.take().map(|nota| capped_nota(&nota));
        let mut reading = read(&text, None, DispositifRule::None, &request)?;
        let room = room_for_reading(&article, nota.as_deref(), &reading);
        if returned_chars(&reading) > room {
            // Hors budget : la fenêtre est relue avec ce qui reste de la cible, sans descendre
            // sous le minimum de `max_chars`.
            let capped = ReadingInput {
                max_chars: Some(i64::try_from(room).unwrap_or(i64::MAX)),
                ..input.reading
            };
            // Mêmes paramètres, `max_chars` dans ses bornes, `offset` déjà vérifié : ni l'analyse
            // ni la lecture n'échouent. Dans le cas improbable où l'une échouerait, la lecture
            // entière est gardée et l'adapter la raccourcit à la mesure exacte.
            let shortened = ReadingRequest::parse(&capped)
                .ok()
                .and_then(|request| read(&text, None, DispositifRule::None, &request).ok());
            reading = shortened.unwrap_or(reading);
        }
        Ok(GetTextOutput { article, nota, reading })
    }
}

/// `article_id` : absent ou blanc, `required` ; sinon un `LEGIARTI` ou un `KALIARTI`.
fn article_id(raw: Option<&str>) -> Result<ArticleId, AppError> {
    let raw = raw
        .filter(|raw| !raw.trim().is_empty())
        .ok_or_else(|| messages::required(messages::id_field(TOOL)))?;
    ArticleId::parse(raw).map_err(|error| messages::domain(TOOL, &error))
}

/// Le `nota` coupé à [`NOTA_MAX_CHARS`] caractères, comme une fenêtre de lecture (en fin de phrase
/// ou de mot), blancs de coupe retirés.
fn capped_nota(nota: &LongText) -> String {
    window(nota.as_str(), 0, NOTA_MAX_CHARS).text.trim_end().to_owned()
}

/// Caractères de texte que la lecture peut encore porter sous la cible du budget, une fois
/// comptés les métadonnées de l'article (citation comprise), le `nota` et les champs de la
/// lecture, au moins [`ResponseBudget::MAX_CHARS_MIN`] (le minimum d'un `max_chars`).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "estimation de taille : sommes de longueurs de textes et de compteurs déjà en mémoire, donc sous `usize::MAX`"
)]
fn room_for_reading(article: &ArticleText, nota: Option<&str>, reading: &Reading) -> usize {
    let contract = article.contract();
    let texts = [article.num.as_deref(), article.code_title.as_deref(), nota]
        .into_iter()
        .flatten()
        .map(|text| text.chars().count());
    let path = article.path.iter().map(|title| title.chars().count());
    let chars = ARTICLE_FIXED_CHARS
        + contract.citation.as_str().chars().count()
        + texts.chain(path).sum::<usize>();
    let passages = reading.passages.as_ref().map_or(0, Vec::len);
    let fields = ARTICLE_FIELDS + article.path.len() + READING_FIELDS + PASSAGE_FIELDS * passages;
    ResponseBudget::ESTIMATE_TARGET_CHARS
        .saturating_sub(estimate_chars(fields, chars))
        .max(ResponseBudget::MAX_CHARS_MIN)
}

#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
mod tests {
    use super::*;
    use crate::{
        error::{ErrorCode, UpstreamCause},
        input::ReadingInput,
        ports::CorpusError,
        testing::{FakeCorpus, PortRequest},
    };
    use lf_domain::{
        date::CivilDate, entity::ParentText, id::DocumentId, status::LegalStatus,
        text::SummarySource,
    };
    use pretty_assertions::assert_eq;

    const ID: &str = "LEGIARTI000006419288";
    const KALI_ID: &str = "KALIARTI000005805295";

    const RENT: &str = "Le locataire doit payer le loyer au bailleur à la date convenue.";
    const NOTICE: &str = "Le préavis de départ est de trois mois, sauf en zone tendue.";
    const DEPOSIT: &str = "Le dépôt de garantie est restitué dans un délai de deux mois.";

    fn chars(text: &str) -> usize {
        text.chars().count()
    }

    /// Article du Code civil, art. 9, avec le texte donné.
    fn article_with(id: &str, text: LongText) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(id).unwrap(),
            num: Some("9".to_owned()),
            etat: LegalStatus::parse("VIGUEUR"),
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec!["Livre Ier : Des personnes".to_owned()],
            full_sections_titre: None,
            date_debut: CivilDate::new(1970, 7, 19),
            date_fin: None,
            text,
            nota: None,
        }
    }

    fn article(text: &str) -> ArticleText {
        article_with(ID, LongText::from_plain(text).unwrap())
    }

    /// Trois paragraphes de locations : le texte canonique les joint par une ligne vide.
    fn lease() -> String {
        format!("{RENT}\n{NOTICE}\n{DEPOSIT}")
    }

    /// Un texte de `sentences` phrases de 74 caractères, en un seul paragraphe.
    fn prose(sentences: usize) -> String {
        "Considérant que l'article L. 1-1 du code civil prévoit une règle précise. "
            .repeat(sentences)
    }

    fn input(id: &str) -> GetTextInput {
        GetTextInput { article_id: Some(id.to_owned()), reading: ReadingInput::default() }
    }

    fn reading(f: impl FnOnce(&mut ReadingInput)) -> GetTextInput {
        let mut reading = ReadingInput::default();
        f(&mut reading);
        GetTextInput { article_id: Some(ID.to_owned()), reading }
    }

    async fn read_with(
        corpus: &FakeCorpus,
        input: GetTextInput,
    ) -> Result<GetTextOutput, AppError> {
        GetTextUseCase::new(corpus).execute(input).await
    }

    fn served(text: &str) -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article(text)));
        corpus
    }

    #[tokio::test]
    async fn a_short_article_is_returned_whole_with_the_normalized_id() {
        let corpus = served(RENT);
        // Espaces et casse tolérés : le port reçoit la forme normalisée.
        let output = read_with(&corpus, input("  legiarti000006419288 ")).await.unwrap();

        assert_eq!(output.reading.text.as_deref(), Some(RENT));
        assert_eq!(output.reading.summary, None);
        assert_eq!(output.reading.text_length, u32::try_from(chars(RENT)).unwrap());
        assert_eq!(output.reading.next_offset, None);
        assert_eq!(output.reading.passages, None);
        assert_eq!(output.reading.note, None);
        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetArticle(ArticleId::parse(ID).unwrap())]
        );
        // Le texte est dans la lecture : l'article rendu n'en porte plus.
        assert!(output.article.text.is_empty());
        assert_eq!(output.article.num.as_deref(), Some("9"));
        assert_eq!(output.nota, None);
    }

    #[tokio::test]
    async fn summary_mode_uses_lead_for_articles() {
        let long = prose(40);
        let corpus = served(&long);

        let output = read_with(&corpus, input(ID)).await.unwrap();

        let summary = output.reading.summary.expect("an article over 1,200 characters is cut");
        assert_eq!(summary.source, SummarySource::Lead);
        let size = chars(&summary.text);
        assert!((600..=1_200).contains(&size), "{size}");
        assert!(long.starts_with(summary.text.trim_end()));
        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.next_offset, None);
        assert_eq!(output.reading.text_length, u32::try_from(chars(long.trim_end())).unwrap());
    }

    #[tokio::test]
    async fn summary_is_bounded_by_a_small_max_chars() {
        let corpus = served(&prose(40));

        let output = read_with(&corpus, reading(|r| r.max_chars = Some(500))).await.unwrap();

        let summary = output.reading.summary.unwrap();
        assert_eq!(summary.source, SummarySource::Lead);
        let size = chars(&summary.text);
        assert!(size <= 500, "{size}");
    }

    #[tokio::test]
    async fn focus_returns_passages_with_offsets() {
        let corpus = served(&lease());

        let output =
            read_with(&corpus, reading(|r| r.focus = Some("préavis".to_owned()))).await.unwrap();

        let passages = output.reading.passages.expect("focus gives passages");
        let best = &passages[0];
        assert_eq!(best.text, NOTICE);
        // Les paragraphes sont joints par une ligne vide : le second commence après le premier et
        // ses deux sauts de ligne.
        assert_eq!(best.offset, u32::try_from(chars(RENT) + 2).unwrap());
        // L'`offset` d'un passage est utilisable tel quel comme `offset` de fenêtre.
        let canonical = format!("{RENT}\n\n{NOTICE}\n\n{DEPOSIT}");
        let from_offset: String = canonical.chars().skip(best.offset as usize).collect();
        assert!(from_offset.starts_with(NOTICE));
        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.summary, None);
        assert_eq!(output.reading.text_length, u32::try_from(chars(&canonical)).unwrap());
    }

    #[tokio::test]
    async fn focus_gives_at_most_passages_in_score_order() {
        let corpus = served(&lease());

        let output = read_with(
            &corpus,
            reading(|r| {
                r.focus = Some("mois".to_owned());
                r.passages = Some(1);
            }),
        )
        .await
        .unwrap();

        // « mois » figure dans le préavis et dans le dépôt : un seul passage est demandé.
        assert_eq!(output.reading.passages.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_focus_that_matches_nothing_gives_no_passage_and_a_note() {
        let corpus = served(&lease());

        let output =
            read_with(&corpus, reading(|r| r.focus = Some("hypothèque".to_owned()))).await.unwrap();

        assert_eq!(output.reading.passages, Some(Vec::new()));
        assert_eq!(
            output.reading.note.as_deref(),
            Some("No passage matches focus; read the text with detail=\"full\".")
        );
    }

    #[tokio::test]
    async fn focus_without_terms_message() {
        let corpus = FakeCorpus::new();

        for focus in ["le de la", "   ", "... ,"] {
            let error = read_with(&corpus, reading(|r| r.focus = Some(focus.to_owned())))
                .await
                .unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg), "{focus:?}");
            assert_eq!(
                error.message(),
                "focus contains no searchable word (only stop words or punctuation); give a few \
                 meaningful words or omit focus.",
                "{focus:?}"
            );
        }
        // Refusé avant l'appel au port.
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn kaliarti_accepted() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article_with(KALI_ID, LongText::from_plain(RENT).unwrap())));

        let output = read_with(&corpus, input(KALI_ID)).await.unwrap();

        assert_eq!(output.article.article_id, ArticleId::parse(KALI_ID).unwrap());
        assert_eq!(output.reading.text.as_deref(), Some(RENT));
        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetArticle(ArticleId::parse(KALI_ID).unwrap())]
        );
    }

    #[tokio::test]
    async fn a_missing_or_blank_id_is_required_without_network() {
        let corpus = FakeCorpus::new();

        for id in [None, Some(""), Some("  \t")] {
            let wanted =
                GetTextInput { article_id: id.map(str::to_owned), ..GetTextInput::default() };
            let error = read_with(&corpus, wanted).await.unwrap_err();
            assert_eq!(error, AppError::invalid_argument("article_id is required."), "{id:?}");
        }
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_whole_text_or_foreign_id_is_invalid_arg_pointing_to_the_right_tool() {
        let corpus = FakeCorpus::new();

        let whole = read_with(&corpus, input("LEGITEXT000006070721")).await.unwrap_err();
        assert_eq!(whole.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(
            whole.message(),
            "article_id='LEGITEXT000006070721' must be a Legifrance LEGIARTI... or KALIARTI... \
             identifier. LEGITEXT... identifies a whole text: use fr_get_act, then pick an \
             article_id from its table of contents."
        );
        let decision = read_with(&corpus, input("JURITEXT000007042960")).await.unwrap_err();
        assert!(decision.message().contains("fr_get_decision"));
        let short = read_with(&corpus, input("LEGIARTI0001")).await.unwrap_err();
        assert!(short.message().starts_with("article_id='LEGIARTI0001'"));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn validation_follows_the_reference_order_and_precedes_the_port() {
        let corpus = FakeCorpus::new();
        let bad_everything = GetTextInput {
            article_id: Some("nope".to_owned()),
            reading: ReadingInput {
                detail: Some("long".to_owned()),
                offset: Some(-1),
                max_chars: Some(100),
                focus: Some("le".to_owned()),
                passages: Some(0),
            },
        };

        // L'identifiant d'abord, puis detail, offset, max_chars, focus, passages.
        let mut wanted = bad_everything;
        let id = read_with(&corpus, wanted.clone()).await.unwrap_err();
        assert!(id.message().starts_with("article_id='nope'"));
        wanted.article_id = Some(ID.to_owned());
        let messages = [
            "detail='long' must be one of ['full', 'summary'].",
            // `focus` l'emporte sur `offset`, qui n'est alors pas jugé : max_chars vient ensuite.
            "max_chars=100 out of range (500..40000).",
            "focus contains no searchable word (only stop words or punctuation); give a few \
             meaningful words or omit focus.",
            "passages=0 out of range (1..10).",
        ];
        for (index, expected) in messages.into_iter().enumerate() {
            let error = read_with(&corpus, wanted.clone()).await.unwrap_err();
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
            assert_eq!(error.message(), expected);
            match index {
                0 => wanted.reading.detail = None,
                1 => wanted.reading.max_chars = None,
                2 => wanted.reading.focus = Some("loyer".to_owned()),
                _ => {}
            }
        }
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_offset_with_an_explicit_summary_is_refused_and_a_negative_one_too() {
        let corpus = FakeCorpus::new();

        let summary = read_with(
            &corpus,
            reading(|r| {
                r.detail = Some("Summary".to_owned());
                r.offset = Some(10);
            }),
        )
        .await
        .unwrap_err();
        let negative = read_with(&corpus, reading(|r| r.offset = Some(-5))).await.unwrap_err();

        assert_eq!(summary.message(), "offset requires detail=\"full\" (or omit detail).");
        assert_eq!(negative.message(), "offset=-5 out of range (0..).");
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_offset_past_the_end_is_invalid_arg_once_the_article_is_read() {
        let corpus = served(RENT);
        let length = chars(RENT);

        let error =
            read_with(&corpus, reading(|r| r.offset = Some(i64::try_from(length).unwrap())))
                .await
                .unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(
            error.message(),
            format!(
                "offset={length} is past the end of the text (text_length={length}); restart from \
                 offset=0 or omit offset."
            )
        );
    }

    #[tokio::test]
    async fn an_empty_article_gives_the_no_text_note() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article("")));

        let output = read_with(&corpus, input(ID)).await.unwrap();

        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.text_length, 0);
        assert_eq!(
            output.reading.note.as_deref(),
            Some("No text is available from Légifrance for this record.")
        );
    }

    #[tokio::test]
    async fn pages_follow_next_offset_and_rebuild_the_text() {
        let long = prose(120);
        let canonical = long.trim_end().to_owned();
        let corpus = FakeCorpus::new();
        let mut rebuilt = String::new();
        let mut offset = 0_i64;
        for _ in 0..40 {
            corpus.push_get_article(Ok(article(&long)));
            let page = read_with(
                &corpus,
                reading(|r| {
                    r.detail = Some("full".to_owned());
                    r.offset = Some(offset);
                    r.max_chars = Some(1_000);
                }),
            )
            .await
            .unwrap();
            rebuilt.push_str(page.reading.text.as_deref().unwrap());
            match page.reading.next_offset {
                Some(next) => offset = i64::from(next),
                None => break,
            }
        }

        // Les pages se partitionnent exactement : leur concaténation est le texte canonique.
        assert_eq!(rebuilt, canonical);
    }

    #[tokio::test]
    async fn the_nota_is_capped_and_moved_out_of_the_article() {
        let corpus = FakeCorpus::new();
        let mut with_nota = article(RENT);
        with_nota.nota = Some(LongText::from_plain(&prose(40)).unwrap());
        corpus.push_get_article(Ok(with_nota));

        let output = read_with(&corpus, input(ID)).await.unwrap();

        let nota = output.nota.expect("the nota is kept");
        let size = chars(&nota);
        assert!(size <= NOTA_MAX_CHARS && size > NOTA_MAX_CHARS / 2, "{size}");
        assert!(prose(40).starts_with(&nota));
        assert_eq!(output.article.nota, None);
    }

    #[tokio::test]
    async fn a_short_nota_is_kept_whole_and_a_missing_one_stays_missing() {
        let corpus = FakeCorpus::new();
        let mut with_nota = article(RENT);
        with_nota.nota =
            Some(LongText::from_plain("Version issue de la loi n° 2016-1321.").unwrap());
        corpus.push_get_article(Ok(with_nota));
        corpus.push_get_article(Ok(article(RENT)));

        let short = read_with(&corpus, input(ID)).await.unwrap();
        let none = read_with(&corpus, input(ID)).await.unwrap();

        assert_eq!(short.nota.as_deref(), Some("Version issue de la loi n° 2016-1321."));
        assert_eq!(none.nota, None);
    }

    #[tokio::test]
    async fn the_window_leaves_room_for_the_metadata_under_the_estimate_target() {
        // Dix niveaux de plan de 400 caractères : les métadonnées pèsent environ 4 000 caractères,
        // une fenêtre de 40 000 ferait dépasser la cible de 42 000.
        let long = prose(2_000);
        let heavy = |text: &str| {
            let mut heavy = article(text);
            heavy.path = (0..10).map(|n| format!("{n} {}", "x".repeat(400))).collect();
            heavy
        };
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(heavy(&long)));
        corpus.push_get_article(Ok(article(&long)));
        let wide = |r: &mut ReadingInput| {
            r.detail = Some("full".to_owned());
            r.max_chars = Some(40_000);
        };

        let shortened = read_with(&corpus, reading(wide)).await.unwrap();
        let untouched = read_with(&corpus, reading(wide)).await.unwrap();

        let kept = shortened.reading.text.unwrap();
        // 4 000 caractères de plan, 29 champs ou éléments à 64 : il reste au plus 36 000 environ.
        let size = chars(&kept);
        assert!(size < 36_500 && size > 18_000, "{size}");
        // `next_offset` reste exact : la fenêtre raccourcie finit où la suivante commence.
        assert_eq!(shortened.reading.next_offset, Some(u32::try_from(chars(&kept)).unwrap()));
        // Sans métadonnées lourdes, la fenêtre demandée est servie telle quelle.
        let whole = untouched.reading.text.unwrap();
        let size = chars(&whole);
        assert!(size > 38_000 && size <= 40_000, "{size}");
    }

    #[tokio::test]
    async fn an_unknown_article_is_not_found_naming_the_id() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Err(CorpusError::NotFound {
            what: "/consult/getArticle".to_owned(),
        }));

        let error = read_with(&corpus, input(ID)).await.unwrap_err();

        assert_eq!(
            error,
            AppError::not_found(format!(
                "No article found for {ID}. Check the id, or find the article with fr_get_act or \
                 fr_search."
            ))
        );
    }

    #[tokio::test]
    async fn port_failures_keep_their_code() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Err(CorpusError::Timeout));
        corpus.push_get_article(Err(CorpusError::RateLimited { retry_after: None }));

        let timeout = read_with(&corpus, input(ID)).await.unwrap_err();
        let limited = read_with(&corpus, input(ID)).await.unwrap_err();

        assert_eq!(timeout.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(timeout.cause(), Some(UpstreamCause::Timeout));
        assert_eq!(limited.code(), Some(ErrorCode::RateLimited));
    }
}
