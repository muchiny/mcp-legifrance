//! Cas d'usage de `fr_get_deliberation` (v0.1) : la lecture d'une délibération de la CNIL (`GetDeliberationUseCase`).
//!
//! Ordre de validation (05 §5.0.2, §5.5) : l'identifiant (requis, puis `CNILTEXT` + 12 chiffres,
//! espaces et casse tolérés), puis les paramètres de lecture (`detail`, `offset`, `max_chars`,
//! `focus`, `passages`), tous avant l'appel au port. Le texte lu est ensuite confié à
//! [`read`], sans sommaire officiel (la CNIL n'en publie pas) et avec la règle de dispositif
//! [`DispositifRule::Cnil`] (D-09) : le résumé d'une sanction ou d'une mise en demeure longue est
//! son dispositif (`dispositif`), celui de toute autre délibération, avis compris, son début
//! (`lead`, 05 §6.2).
//! Un `offset` au-delà de la fin du texte ne se voit qu'une fois le texte lu : il est refusé après
//! l'appel au port. Les phrases `invalid_arg` viennent du module `messages` ; une panne du corpus
//! passe par `failure::port_failure`.

use lf_domain::{id::CnilTextId, text::DispositifRule};

use crate::{
    budget::{ReadingRequest, read},
    error::AppError,
    failure::port_failure,
    input::GetDeliberationInput,
    messages,
    output::get_deliberation::GetDeliberationOutput,
    ports::LegalCorpus,
    tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_GET_DELIBERATION;

/// Lit une délibération de la CNIL par son identifiant, selon le mode de lecture demandé.
pub(crate) struct GetDeliberationUseCase<'a, C: LegalCorpus> {
    corpus: &'a C,
}

impl<'a, C: LegalCorpus> GetDeliberationUseCase<'a, C> {
    /// Cas d'usage qui lit `corpus`.
    pub(crate) fn new(corpus: &'a C) -> Self {
        Self { corpus }
    }

    /// Valide l'identifiant et les paramètres de lecture, lit la délibération, puis son texte.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` si `deliberation_id` est absent ou n'est pas un `CNILTEXT` valide, si un
    ///   paramètre de lecture est refusé (aucun appel n'est fait au port), ou si `offset` dépasse
    ///   la fin du texte ;
    /// - `not_found` si le corpus ne connaît pas la délibération, et les autres erreurs du port
    ///   (`upstream_error`, `rate_limited`, `config_error`) traduites par `messages`.
    pub(crate) async fn execute(
        &self,
        input: GetDeliberationInput,
    ) -> Result<GetDeliberationOutput, AppError> {
        let raw =
            input.deliberation_id.ok_or_else(|| messages::required(messages::id_field(TOOL)))?;
        let id = CnilTextId::parse(&raw).map_err(|error| messages::domain(TOOL, &error))?;
        let request = ReadingRequest::parse(&input.reading)?;
        let mut deliberation = self
            .corpus
            .get_deliberation(&id)
            .await
            .map_err(|error| port_failure(TOOL, id.as_str(), &error))?;
        // Le texte est lu une fois, puis lâché : la sortie ne porte que la lecture.
        let text = std::mem::take(&mut deliberation.text);
        let reading = read(&text, None, DispositifRule::Cnil, &request)?;
        Ok(GetDeliberationOutput { deliberation, reading })
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tests : valeurs bornées par les données du test"
)]
#[cfg(test)]
#[expect(
    clippy::as_conversions,
    reason = "tests : tailles et indices bornés par les données du test"
)]
mod tests {
    use lf_domain::{
        date::CivilDate,
        entity::Deliberation,
        text::{LongText, SUMMARY_CAP_CHARS, SummarySource},
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        error::{ErrorCode, UpstreamCause},
        input::ReadingInput,
        ports::CorpusError,
        testing::{FakeCorpus, PortRequest},
    };

    const ID: &str = "CNILTEXT000053352643";

    /// Une retouche des paramètres de lecture, pour parcourir plusieurs cas.
    type Edit = fn(&mut ReadingInput);

    /// Les trois paragraphes (593 caractères) de la fixture `consult_cnil.json`, texte tronqué.
    const FIXTURE_PARAGRAPHS: [&str; 3] = [
        "- Les développements de la délibération comportant des données à caractère personnel ou des secrets protégés par la loi sont remplacés par le signe […] -",
        "La Commission nationale de l’informatique et des libertés, réunie en sa formation restreinte composée de M. Philippe-Pierre CABOURDIN, président, M. Vincent LESCLOUS, vice-président, Mmes Laurence FRANCESCHINI et Isabelle LATOURNARIE-WILLEMS, MM. KLING, membres,",
        "Vu le règlement (UE) 2016/679 du Parlement européen et du Conseil du 27 avril 2016 relatif à la protection des personnes physiques à l’égard du traitement des données à [...]",
    ];

    fn deliberation_with(text: LongText) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse(ID).unwrap(),
            num: Some("SAN-2026-002".to_owned()),
            nature: Some("Délibération".to_owned()),
            nature_delib: Some("Sanction".to_owned()),
            etat: None,
            date_texte: CivilDate::new(2026, 1, 8),
            date_publication: CivilDate::new(2026, 1, 14),
            title: Some("Délibération SAN-2026-002 du 8 janvier 2026".to_owned()),
            title_long: None,
            text,
        }
    }

    fn fixture_text() -> LongText {
        LongText::from_plain(&FIXTURE_PARAGRAPHS.join("\n")).unwrap()
    }

    /// Une phrase de 140 caractères environ.
    fn sentence(i: usize) -> String {
        format!(
            "Considérant que le manquement n° {i} à l’article 32 du règlement est caractérisé, \
             la formation restreinte prononce une sanction. "
        )
    }

    /// Un texte de `count` paragraphes de cinq phrases, comme une longue délibération.
    fn long_text(count: usize) -> LongText {
        let paragraphs: Vec<String> = (0..count)
            .map(|p| (0..5).map(|s| sentence(p * 10 + s)).collect::<String>().trim_end().to_owned())
            .collect();
        LongText::from_plain(&paragraphs.join("\n")).unwrap()
    }

    fn input(id: Option<&str>, edit: impl FnOnce(&mut ReadingInput)) -> GetDeliberationInput {
        let mut reading = ReadingInput::default();
        edit(&mut reading);
        GetDeliberationInput { deliberation_id: id.map(str::to_owned), reading }
    }

    fn chars(text: &str) -> usize {
        text.chars().count()
    }

    #[tokio::test]
    async fn summary_source_is_lead_for_cnil() {
        let text = long_text(40);
        assert!(text.char_len() > 4_000);
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(text.clone())));
        let output =
            GetDeliberationUseCase::new(&corpus).execute(input(Some(ID), |_| {})).await.unwrap();
        // La CNIL ne publie pas de sommaire et ce texte n'a pas de dispositif repérable (ni
        // « PAR CES MOTIFS » ni mesures correctrices, comme un avis) : le résumé est le début.
        let summary = output.reading.summary.as_ref().expect("un texte long donne un résumé");
        assert_eq!(summary.source, SummarySource::Lead);
        assert_eq!(summary.source.as_str(), "lead");
        assert!(text.as_str().starts_with(&summary.text));
        let size = chars(&summary.text);
        assert!(
            (lf_domain::text::LEAD_MIN_CHARS..=SUMMARY_CAP_CHARS).contains(&size),
            "{size} caractères"
        );
        // Le texte n'est pas rendu, mais sa longueur l'est, pour décider de la suite de la lecture.
        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.text_length, text.char_len());
        assert_eq!(output.reading.next_offset, None);
        assert_eq!(output.reading.passages, None);
        assert_eq!(output.reading.note, None);
        // Les métadonnées restent, le texte entier est lâché.
        assert_eq!(output.deliberation, deliberation_with(LongText::default()));
        assert_eq!(output.output_count_or_size(), u64::try_from(size).unwrap());
    }

    /// Une sanction longue : motifs, puis « PAR CES MOTIFS », le dispositif et la signature.
    fn sanction_text() -> LongText {
        let mut paragraphs: Vec<String> =
            (0..30).map(|p| format!("{p}. {}", sentence(p).trim_end())).collect();
        paragraphs.push("PAR CES MOTIFS".to_owned());
        paragraphs.push(
            "La formation restreinte de la CNIL, après en avoir délibéré, décide de :".to_owned(),
        );
        paragraphs.push("- prononcer à l’encontre de la société X une amende administrative d’un montant de 50 000 euros ;".to_owned());
        paragraphs.push("Le Président".to_owned());
        paragraphs.push(
            "Cette décision peut faire l’objet d’un recours devant le Conseil d’État.".to_owned(),
        );
        LongText::from_plain(&paragraphs.join("\n")).unwrap()
    }

    #[tokio::test]
    async fn the_summary_of_a_sanction_is_its_operative_part() {
        // D-09 : « PAR CES MOTIFS » ... « Le Président » donne le résumé, pas le début du texte.
        let text = sanction_text();
        assert!(chars(text.as_str()) > SUMMARY_CAP_CHARS);
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(text)));
        let output =
            GetDeliberationUseCase::new(&corpus).execute(input(Some(ID), |_| {})).await.unwrap();
        let summary = output.reading.summary.as_ref().expect("un texte long donne un résumé");
        assert_eq!(summary.source, SummarySource::Dispositif);
        assert_eq!(summary.source.as_str(), "dispositif");
        assert_eq!(
            summary.text,
            "La formation restreinte de la CNIL, après en avoir délibéré, décide de :\n\n\
             - prononcer à l’encontre de la société X une amende administrative d’un montant de 50 000 euros ;"
        );
        assert!(!summary.truncated);
        assert_eq!(output.reading.text, None);
    }

    #[tokio::test]
    async fn the_summary_follows_max_chars_down_to_its_floor() {
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(long_text(40))));
        let output = GetDeliberationUseCase::new(&corpus)
            .execute(input(Some(ID), |i| i.max_chars = Some(500)))
            .await
            .unwrap();
        let summary = output.reading.summary.unwrap();
        assert_eq!(summary.source, SummarySource::Lead);
        let size = chars(&summary.text);
        assert!(size <= 500, "{size} caractères");
    }

    #[tokio::test]
    async fn a_short_deliberation_comes_back_whole_without_a_summary() {
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(fixture_text())));
        let output =
            GetDeliberationUseCase::new(&corpus).execute(input(Some(ID), |_| {})).await.unwrap();
        // 593 caractères : sous min(max_chars, 1 200), le texte est rendu entier (05 §5.0.3).
        assert_eq!(output.reading.text.as_deref(), Some(FIXTURE_PARAGRAPHS.join("\n\n").as_str()));
        assert_eq!(output.reading.text_length, 593);
        assert_eq!(output.reading.summary, None);
        assert_eq!(output.reading.next_offset, None);
        assert_eq!(output.output_count_or_size(), 593);
    }

    /// Un paragraphe de `size` caractères exactement, sans blanc de fin (L4-12, cnil_kali_acco-15).
    fn paragraph_of(size: usize) -> String {
        let mut paragraph: String = "Vu la loi n° 78-17 du 6 janvier 1978 modifiée, "
            .chars()
            .cycle()
            .take(size.checked_sub(1).unwrap())
            .collect();
        paragraph.push('.');
        paragraph
    }

    /// Un texte de 3 852 caractères comme CNILTEXT000019796121 : 18 paragraphes de 200, un de 216.
    fn cnil_3852_chars() -> LongText {
        let mut paragraphs: Vec<String> = (0..18).map(|_| paragraph_of(200)).collect();
        paragraphs.push(paragraph_of(216));
        let text = LongText::from_plain(&paragraphs.join("\n")).unwrap();
        assert_eq!(text.char_len(), 3_852);
        text
    }

    #[tokio::test]
    async fn a_3852_character_deliberation_is_summarised_although_it_fits_max_chars() {
        // CNILTEXT000019796121 : 3 852 caractères < max_chars par défaut (4 000), mais le texte
        // entier n'est rendu que sous min(max_chars, 1 200) (05 §5.0.3) : le seuil est 1 200.
        let text = cnil_3852_chars();
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(text.clone())));
        let output =
            GetDeliberationUseCase::new(&corpus).execute(input(Some(ID), |_| {})).await.unwrap();
        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.text_length, 3_852);
        let summary = output.reading.summary.as_ref().expect("au-dessus de 1 200 : un résumé");
        assert_eq!(summary.source, SummarySource::Lead);
        assert!(chars(&summary.text) <= SUMMARY_CAP_CHARS);
        assert!(text.as_str().starts_with(&summary.text));
        assert_eq!(output.reading.next_offset, None);
    }

    #[tokio::test]
    async fn a_3852_character_deliberation_comes_back_whole_with_detail_full() {
        // Jumeau : le texte entier reste à un appel de là, sans fenêtre ni `next_offset`.
        let text = cnil_3852_chars();
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(text.clone())));
        let output = GetDeliberationUseCase::new(&corpus)
            .execute(input(Some(ID), |i| i.detail = Some("full".to_owned())))
            .await
            .unwrap();
        assert_eq!(output.reading.text.as_deref(), Some(text.as_str()));
        assert_eq!(output.reading.text_length, 3_852);
        assert_eq!(output.reading.summary, None);
        assert_eq!(output.reading.next_offset, None);
    }

    #[tokio::test]
    async fn the_port_receives_the_normalized_id() {
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(fixture_text())));
        // Espaces et casse tolérés : le port reçoit la forme normalisée.
        GetDeliberationUseCase::new(&corpus)
            .execute(input(Some("  cniltext000053352643 "), |_| {}))
            .await
            .unwrap();
        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetDeliberation(CnilTextId::parse(ID).unwrap())]
        );
    }

    #[tokio::test]
    async fn full_pages_rebuild_the_text_and_follow_next_offset() {
        let text = long_text(12);
        let corpus = FakeCorpus::new();
        let use_case = GetDeliberationUseCase::new(&corpus);
        let mut rebuilt = String::new();
        let mut offset = 0_u32;
        let mut pages = 0;
        loop {
            corpus.push_get_deliberation(Ok(deliberation_with(text.clone())));
            let output = use_case
                .execute(input(Some(ID), |i| {
                    i.detail = Some("full".to_owned());
                    i.offset = Some(i64::from(offset));
                    i.max_chars = Some(1_000);
                }))
                .await
                .unwrap();
            let page = output.reading.text.clone().expect("une page a du texte");
            assert!(chars(&page) <= 1_000);
            assert_eq!(output.reading.text_length, text.char_len());
            assert_eq!(output.reading.summary, None);
            assert_eq!(output.output_count_or_size(), u64::try_from(chars(&page)).unwrap());
            rebuilt.push_str(&page);
            pages += 1;
            assert!(pages < 100, "pas de progression");
            match output.reading.next_offset {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert!(pages > 3, "{pages} pages");
        assert_eq!(rebuilt, text.as_str());
    }

    #[tokio::test]
    async fn an_offset_without_detail_reads_a_window() {
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(long_text(12))));
        let output = GetDeliberationUseCase::new(&corpus)
            .execute(input(Some(ID), |i| i.offset = Some(300)))
            .await
            .unwrap();
        // `detail` vaut `full` dès que `offset > 0` est fourni seul (05 §5.0.2).
        assert!(output.reading.text.is_some());
        assert_eq!(output.reading.summary, None);
    }

    #[tokio::test]
    async fn focus_returns_ranked_passages_with_offsets() {
        let text = LongText::from_plain(
            "Premier paragraphe sur la procédure.\n\nLe responsable de traitement n’a pas notifié \
             la violation de données dans les délais.\n\nTroisième paragraphe sur les mesures \
             correctrices.",
        )
        .unwrap();
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(text.clone())));
        let output = GetDeliberationUseCase::new(&corpus)
            .execute(input(Some(ID), |i| i.focus = Some("notifié violation".to_owned())))
            .await
            .unwrap();
        let passages = output.reading.passages.as_ref().expect("le mode focus rend des passages");
        let best = &passages[0];
        assert!(best.text.starts_with("Le responsable de traitement"), "{}", best.text);
        // L'offset du passage est utilisable comme `offset` de fenêtre.
        let start = usize::try_from(best.offset).unwrap();
        let from_offset: String = text.as_str().chars().skip(start).collect();
        assert!(from_offset.starts_with(&best.text));
        assert_eq!(output.reading.summary, None);
        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.text_length, text.char_len());
    }

    #[tokio::test]
    async fn focus_without_a_match_says_so() {
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(fixture_text())));
        let output = GetDeliberationUseCase::new(&corpus)
            .execute(input(Some(ID), |i| i.focus = Some("zibeline".to_owned())))
            .await
            .unwrap();
        assert_eq!(output.reading.passages, Some(Vec::new()));
        assert_eq!(
            output.reading.note.as_deref(),
            Some("No passage matches focus; read the text with detail=\"full\".")
        );
        assert_eq!(output.output_count_or_size(), 0);
    }

    #[tokio::test]
    async fn an_empty_text_gives_a_note_in_every_mode() {
        let note = "No text is available from Légifrance for this record.";
        for edit in [
            (|_: &mut ReadingInput| {}) as Edit,
            |i| i.detail = Some("full".to_owned()),
            |i| i.focus = Some("sanction".to_owned()),
        ] {
            let corpus = FakeCorpus::new();
            corpus.push_get_deliberation(Ok(deliberation_with(LongText::default())));
            let output =
                GetDeliberationUseCase::new(&corpus).execute(input(Some(ID), edit)).await.unwrap();
            assert_eq!(output.reading.note.as_deref(), Some(note));
            assert_eq!(output.reading.text_length, 0);
            assert_eq!(output.reading.text, None);
            assert_eq!(output.output_count_or_size(), 0);
        }
    }

    #[tokio::test]
    async fn an_offset_past_the_end_is_invalid_arg_after_the_read() {
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Ok(deliberation_with(fixture_text())));
        let error = GetDeliberationUseCase::new(&corpus)
            .execute(input(Some(ID), |i| i.offset = Some(593)))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            AppError::invalid_argument(
                "offset=593 is at or past the end of the text (text_length=593); restart from offset=0 \
                 or omit offset."
            )
        );
        // Le texte a dû être lu pour connaître sa longueur.
        assert_eq!(corpus.calls(), 1);
    }

    #[tokio::test]
    async fn a_missing_id_is_invalid_arg_without_network() {
        let corpus = FakeCorpus::new();
        let error =
            GetDeliberationUseCase::new(&corpus).execute(input(None, |_| {})).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("deliberation_id is required."));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_malformed_or_foreign_id_is_invalid_arg_without_network() {
        let corpus = FakeCorpus::new();
        let use_case = GetDeliberationUseCase::new(&corpus);
        // Un `JURITEXT` n'est pas une délibération : la phrase renvoie à l'outil du bon préfixe.
        let foreign =
            use_case.execute(input(Some("JURITEXT000037999394"), |_| {})).await.unwrap_err();
        assert_eq!(foreign.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(
            foreign.message(),
            "deliberation_id='JURITEXT000037999394' must be a Legifrance CNILTEXT... identifier. \
             JURITEXT... identifies a court decision: use fr_get_decision."
        );
        let short = use_case.execute(input(Some("CNILTEXT0001"), |_| {})).await.unwrap_err();
        assert_eq!(short.code(), Some(ErrorCode::InvalidArg));
        assert!(short.message().starts_with("deliberation_id='CNILTEXT0001'"));
        let blank = use_case.execute(input(Some("   "), |_| {})).await.unwrap_err();
        assert_eq!(blank.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn reading_parameters_are_refused_without_network_and_after_the_id() {
        let corpus = FakeCorpus::new();
        let use_case = GetDeliberationUseCase::new(&corpus);
        let refusals: [(Edit, &str); 5] = [
            (
                |i| i.detail = Some("long".to_owned()),
                "detail='long' must be one of ['full', 'summary'].",
            ),
            (|i| i.max_chars = Some(100), "max_chars=100 out of range (500..40000)."),
            (
                |i| {
                    i.detail = Some("summary".to_owned());
                    i.offset = Some(10);
                },
                "offset requires detail=\"full\" (or omit detail).",
            ),
            (|i| i.offset = Some(-1), "offset=-1 out of range (0..4294967295)."),
            (
                |i| {
                    i.focus = Some("le de la".to_owned());
                },
                "focus contains no searchable word (only stop words or punctuation); give a few \
                 meaningful words or omit focus.",
            ),
        ];
        for (edit, message) in refusals {
            let error = use_case.execute(input(Some(ID), edit)).await.unwrap_err();
            assert_eq!(error, AppError::invalid_argument(message));
        }
        // `passages` hors bornes : refusé seulement avec `focus`, ignoré sans lui.
        let passages = use_case
            .execute(input(Some(ID), |i| {
                i.focus = Some("sanction".to_owned());
                i.passages = Some(11);
            }))
            .await
            .unwrap_err();
        assert_eq!(passages, AppError::invalid_argument("passages=11 out of range (1..10)."));
        // L'identifiant se valide avant les paramètres de lecture.
        let both = use_case
            .execute(input(Some("CNILTEXT0001"), |i| i.max_chars = Some(100)))
            .await
            .unwrap_err();
        assert!(both.message().starts_with("deliberation_id='CNILTEXT0001'"));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_unknown_deliberation_is_not_found_naming_the_id() {
        let corpus = FakeCorpus::new();
        corpus
            .push_get_deliberation(Err(CorpusError::NotFound { what: "/consult/cnil".to_owned() }));
        let error = GetDeliberationUseCase::new(&corpus)
            .execute(input(Some(ID), |_| {}))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            AppError::not_found(format!(
                "No CNIL deliberation found for {ID}. Check the id, or search with fr_search."
            ))
        );
    }

    #[tokio::test]
    async fn port_failures_keep_their_code() {
        let corpus = FakeCorpus::new();
        corpus.push_get_deliberation(Err(CorpusError::Timeout));
        corpus.push_get_deliberation(Err(CorpusError::RateLimited { retry_after: None }));
        let use_case = GetDeliberationUseCase::new(&corpus);
        let timeout = use_case.execute(input(Some(ID), |_| {})).await.unwrap_err();
        assert_eq!(timeout.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(timeout.cause(), Some(UpstreamCause::Timeout));
        let limited = use_case.execute(input(Some(ID), |_| {})).await.unwrap_err();
        assert_eq!(limited.code(), Some(ErrorCode::RateLimited));
    }
}
