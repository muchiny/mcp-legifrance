//! Cas d'usage de `fr_get_decision` (v0.1) : la lecture d'une décision de justice (`GetDecisionUseCase`).
//!
//! Ordre de validation (05 §5.0.2, §5.4) : l'identifiant, requis puis de forme `JURITEXT`,
//! `CONSTEXT` ou `CETATEXT` + 12 chiffres, puis les cinq paramètres de lecture
//! ([`ReadingRequest::parse`]), le tout avant tout appel au port. La décision lue est ensuite
//! résumée, fenêtrée ou classée par [`read`], avec la règle de dispositif que le préfixe de
//! l'identifiant désigne.

use lf_domain::{id::DecisionId, text::DispositifRule};

use crate::{
    budget::{ReadingRequest, read},
    error::AppError,
    failure::port_failure,
    input::GetDecisionInput,
    messages,
    output::get_decision::GetDecisionOutput,
    ports::LegalCorpus,
    tools::ToolName,
};

/// Règle de repérage du dispositif d'après le préfixe de l'identifiant (03 §7.9) : « décide » à
/// « juge par » pour le Conseil constitutionnel (`CONSTEXT`), « décide » à « délibéré » pour le
/// Conseil d'État et la justice administrative (`CETATEXT`), aucune pour la Cour de cassation et
/// les cours d'appel (`JURITEXT`, pas d'heuristique « PAR CES MOTIFS » en v0.1, 00 §4).
pub(crate) fn dispositif_rule(id: &DecisionId) -> DispositifRule {
    match id {
        DecisionId::Constit(_) => DispositifRule::ConseilConstitutionnel,
        DecisionId::Cetat(_) => DispositifRule::ConseilEtat,
        DecisionId::Juri(_) => DispositifRule::None,
    }
}

/// Lit une décision de justice par son identifiant.
pub(crate) struct GetDecisionUseCase<'a, C: LegalCorpus> {
    corpus: &'a C,
}

impl<'a, C: LegalCorpus> GetDecisionUseCase<'a, C> {
    /// Cas d'usage qui lit `corpus`.
    pub(crate) fn new(corpus: &'a C) -> Self {
        Self { corpus }
    }

    /// Valide les arguments, lit la décision, puis la rend selon le mode de lecture demandé.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` si `decision_id` est absent ou invalide, si un paramètre de lecture est
    ///   hors bornes, ou si `offset` dépasse la fin du texte ; les cas hors `offset` n'appellent
    ///   pas le port ;
    /// - `not_found` si le corpus ne connaît pas la décision (le fond nommé suit le préfixe,
    ///   E-15), et les autres erreurs du port (`upstream_error`, `rate_limited`, `config_error`)
    ///   traduites par `messages`.
    pub(crate) async fn execute(
        &self,
        input: GetDecisionInput,
    ) -> Result<GetDecisionOutput, AppError> {
        let tool = ToolName::FR_GET_DECISION;
        let raw = input.decision_id.ok_or_else(|| messages::required(messages::id_field(tool)))?;
        let id = DecisionId::parse(&raw).map_err(|error| messages::domain(tool, &error))?;
        let request = ReadingRequest::parse(&input.reading)?;
        let decision = self
            .corpus
            .get_decision(&id)
            .await
            .map_err(|error| port_failure(tool, id.as_str(), &error))?;
        // Le fond de la décision rendue, non celui de la demande : c'est lui qui règle le dispositif.
        let rule = dispositif_rule(&decision.decision_id);
        let reading = read(&decision.text, Some(&decision.official_summary), rule, &request)?;
        Ok(GetDecisionOutput { decision, reading })
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Décisions de test partagées avec les tests de la façade.

    use lf_domain::{entity::Decision, entity::OfficialSummary, id::DecisionId, text::LongText};

    /// Une décision sans métadonnée, de texte `text` (paragraphes séparés par une ligne vide).
    pub(crate) fn decision(id: &str, text: &str) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: Vec::new(),
            date_texte: None,
            nature: None,
            title: None,
            text: LongText::from_plain(text).unwrap(),
            official_summary: OfficialSummary::default(),
            cited_case_law: None,
            applied_texts: vec![],
        }
    }

    /// Un paragraphe de remplissage d'environ 140 caractères.
    pub(crate) fn filler(n: usize) -> String {
        format!(
            "{n}. Considérant que la règle posée par le législateur s'impose à tous les \
             justiciables, sans que puisse y faire obstacle aucune disposition contraire."
        )
    }

    /// `count` paragraphes de remplissage, séparés par une ligne vide.
    pub(crate) fn fillers(count: usize) -> String {
        (1..=count).map(filler).collect::<Vec<_>>().join("\n\n")
    }

    /// Une décision du Conseil constitutionnel : visas, dispositif entre « décide » et « jugé
    /// par », puis une clôture ; assez longue pour que le mode résumé ne la rende pas entière.
    pub(crate) fn constit_text() -> String {
        format!(
            "{}\n\nLE CONSEIL CONSTITUTIONNEL DÉCIDE :\n\nArticle 1er. - La disposition est conforme à \
             la Constitution.\n\nArticle 2. - Cette décision sera publiée au Journal officiel.\n\n\
             Jugé par le Conseil constitutionnel dans sa séance du 6 novembre 2025.",
            fillers(12)
        )
    }

    /// Une décision du Conseil d'État : considérants, « D E C I D E : », articles, « Délibéré ».
    pub(crate) fn cetat_text() -> String {
        format!(
            "{}\n\nD E C I D E :\n\n--------------\n\nArticle 1er : La requête est rejetée.\n\n\
             Délibéré à l'issue de la séance du 12 mai 2026.",
            fillers(12)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{cetat_text, constit_text, decision, filler, fillers};
    use super::*;
    use crate::{
        error::{ErrorCode, UpstreamCause},
        input::ReadingInput,
        ports::CorpusError,
        testing::{FakeCorpus, PortRequest},
    };
    use lf_domain::{
        entity::OfficialSummary,
        text::{SUMMARY_CAP_CHARS, SummarySource},
    };
    use pretty_assertions::assert_eq;

    const JURI: &str = "JURITEXT000048465611";
    const CONSTIT: &str = "CONSTEXT000052555275";
    const CETAT: &str = "CETATEXT000051655317";

    fn input(id: Option<&str>) -> GetDecisionInput {
        GetDecisionInput { decision_id: id.map(str::to_owned), reading: ReadingInput::default() }
    }

    fn reading(edit: impl FnOnce(&mut ReadingInput)) -> ReadingInput {
        let mut reading = ReadingInput::default();
        edit(&mut reading);
        reading
    }

    fn id(raw: &str) -> DecisionId {
        DecisionId::parse(raw).unwrap()
    }

    fn source(output: &GetDecisionOutput) -> Option<SummarySource> {
        output.reading.summary.as_ref().map(|summary| summary.source)
    }

    #[test]
    fn dispositif_rule_from_prefix() {
        assert_eq!(dispositif_rule(&id(CONSTIT)), DispositifRule::ConseilConstitutionnel);
        assert_eq!(dispositif_rule(&id(CETAT)), DispositifRule::ConseilEtat);
        // Pas d'heuristique « PAR CES MOTIFS » pour la justice judiciaire en v0.1 (00 §4).
        assert_eq!(dispositif_rule(&id(JURI)), DispositifRule::None);
    }

    #[tokio::test]
    async fn the_prefix_picks_the_dispositif_rule_of_the_summary() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CONSTIT, &constit_text())));
        corpus.push_get_decision(Ok(decision(CETAT, &cetat_text())));
        corpus.push_get_decision(Ok(decision(JURI, &constit_text())));
        let use_case = GetDecisionUseCase::new(&corpus);

        let constit = use_case.execute(input(Some(CONSTIT))).await.unwrap();
        assert_eq!(source(&constit), Some(SummarySource::Dispositif));
        let summary = constit.reading.summary.unwrap().text;
        assert!(summary.starts_with("Article 1er. - La disposition est conforme"), "{summary}");
        assert!(!summary.contains("DÉCIDE") && !summary.contains("Jugé par"), "{summary}");

        let cetat = use_case.execute(input(Some(CETAT))).await.unwrap();
        assert_eq!(source(&cetat), Some(SummarySource::Dispositif));
        let summary = cetat.reading.summary.unwrap().text;
        assert_eq!(summary, "Article 1er : La requête est rejetée.");

        // Le même texte sous un préfixe JURI n'a pas de règle : le début du texte fait le résumé.
        let juri = use_case.execute(input(Some(JURI))).await.unwrap();
        assert_eq!(source(&juri), Some(SummarySource::Lead));
        assert!(juri.reading.summary.unwrap().text.starts_with("1. Considérant"));
    }

    #[tokio::test]
    async fn the_rule_follows_the_decision_the_port_returned() {
        // Le fond de l'identifiant rendu règle le dispositif (04 §9.2, D28) : une demande
        // `CONSTEXT` qui reçoit une décision `CETATEXT` la résume avec la règle du Conseil d'État.
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CETAT, &cetat_text())));

        let output = GetDecisionUseCase::new(&corpus).execute(input(Some(CONSTIT))).await.unwrap();

        assert_eq!(source(&output), Some(SummarySource::Dispositif));
        assert_eq!(output.reading.summary.unwrap().text, "Article 1er : La requête est rejetée.");
    }

    #[tokio::test]
    async fn an_official_summary_wins_over_the_dispositif() {
        let corpus = FakeCorpus::new();
        let mut given = decision(CONSTIT, &constit_text());
        given.official_summary = OfficialSummary::new(["Sommaire officiel."], [""; 0]);
        corpus.push_get_decision(Ok(given));

        let output = GetDecisionUseCase::new(&corpus).execute(input(Some(CONSTIT))).await.unwrap();

        assert_eq!(source(&output), Some(SummarySource::Official));
        assert_eq!(output.reading.summary.unwrap().text, "Sommaire officiel.");
    }

    #[tokio::test]
    async fn the_default_summary_stays_under_its_cap() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(JURI, &fillers(60))));

        let output = GetDecisionUseCase::new(&corpus).execute(input(Some(JURI))).await.unwrap();

        let summary = output.reading.summary.unwrap();
        assert!(summary.text.chars().count() <= SUMMARY_CAP_CHARS);
        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.text_length, output.decision.text.char_len());
    }

    #[tokio::test]
    async fn a_short_decision_comes_back_whole_without_a_summary() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(JURI, "Rejet du pourvoi.")));

        let output = GetDecisionUseCase::new(&corpus).execute(input(Some(JURI))).await.unwrap();

        assert_eq!(output.reading.text.as_deref(), Some("Rejet du pourvoi."));
        assert_eq!(output.reading.summary, None);
        assert_eq!(output.reading.next_offset, None);
    }

    #[tokio::test]
    async fn the_full_mode_pages_the_text_with_next_offset() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CONSTIT, &fillers(40))));
        corpus.push_get_decision(Ok(decision(CONSTIT, &fillers(40))));
        let use_case = GetDecisionUseCase::new(&corpus);
        let full = |offset| GetDecisionInput {
            decision_id: Some(CONSTIT.to_owned()),
            reading: reading(|reading| {
                reading.detail = Some("FULL".to_owned());
                reading.offset = offset;
                reading.max_chars = Some(500);
            }),
        };

        let first = use_case.execute(full(None)).await.unwrap();
        let first_text = first.reading.text.unwrap();
        assert!(first_text.chars().count() <= 500 && first_text.starts_with("1. Considérant"));
        let next = first.reading.next_offset.expect("a window that does not reach the end");

        let second = use_case.execute(full(Some(i64::from(next)))).await.unwrap();
        // La seconde page reprend le texte canonique là où `next_offset` le dit.
        let rest: String = fillers(40).chars().skip(usize::try_from(next).unwrap()).collect();
        assert!(rest.starts_with(&second.reading.text.unwrap()));
        assert_eq!(second.reading.text_length, first.reading.text_length);
    }

    #[tokio::test]
    async fn focus_returns_the_matching_passages() {
        let corpus = FakeCorpus::new();
        let text = format!(
            "{}\n\nLe principe de publicité des débats est méconnu.\n\n{}",
            fillers(5),
            filler(9)
        );
        corpus.push_get_decision(Ok(decision(CONSTIT, &text)));
        let wanted = GetDecisionInput {
            decision_id: Some(CONSTIT.to_owned()),
            reading: reading(|reading| reading.focus = Some("publicité des débats".to_owned())),
        };

        let output = GetDecisionUseCase::new(&corpus).execute(wanted).await.unwrap();

        let passages = output.reading.passages.expect("the passages mode");
        assert_eq!(passages[0].text, "Le principe de publicité des débats est méconnu.");
        assert_eq!(output.reading.summary, None);
    }

    #[tokio::test]
    async fn the_requested_id_is_normalized_for_the_port() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CONSTIT, "Conforme.")));

        GetDecisionUseCase::new(&corpus)
            .execute(input(Some("  constext000052555275 ")))
            .await
            .unwrap();

        assert_eq!(corpus.seen_requests(), [PortRequest::GetDecision(id(CONSTIT))]);
    }

    #[tokio::test]
    async fn the_metadata_is_the_ports_own_and_an_ecli_is_never_invented() {
        // Le Conseil constitutionnel a un numéro, une nature et une date, dont un ECLI pourrait se
        // déduire : sans ECLI natif, la sortie n'en porte pas.
        let corpus = FakeCorpus::new();
        let mut given = decision(CONSTIT, "Conforme.");
        given.num = Some("2025-1173".to_owned());
        given.nature = Some("QPC".to_owned());
        given.date_texte = lf_domain::date::CivilDate::new(2025, 11, 7);
        corpus.push_get_decision(Ok(given.clone()));

        let output = GetDecisionUseCase::new(&corpus).execute(input(Some(CONSTIT))).await.unwrap();

        assert_eq!(output.decision, given);
        assert_eq!(output.decision.ecli, None);
    }

    #[tokio::test]
    async fn a_missing_id_is_invalid_arg_without_network() {
        let corpus = FakeCorpus::new();
        let error = GetDecisionUseCase::new(&corpus).execute(input(None)).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("decision_id is required."));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_malformed_or_foreign_id_is_invalid_arg_without_network() {
        let corpus = FakeCorpus::new();
        let use_case = GetDecisionUseCase::new(&corpus);
        // Une délibération de la CNIL se lit avec un autre outil : la phrase le dit.
        let cnil = use_case.execute(input(Some("CNILTEXT000053352643"))).await.unwrap_err();
        assert_eq!(cnil.code(), Some(ErrorCode::InvalidArg));
        let message = cnil.message();
        assert!(message.starts_with("decision_id='CNILTEXT000053352643'"));
        assert!(message.contains("fr_get_deliberation"), "{message}");
        let short = use_case.execute(input(Some("CETATEXT12"))).await.unwrap_err();
        let message = short.message();
        assert!(message.starts_with("decision_id='CETATEXT12'"), "{message}");
        let blank = use_case.execute(input(Some("   "))).await.unwrap_err();
        assert_eq!(blank.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn reading_arguments_are_validated_after_the_id_and_before_the_port() {
        let corpus = FakeCorpus::new();
        let use_case = GetDecisionUseCase::new(&corpus);
        let with = |id: Option<&str>, edit: fn(&mut ReadingInput)| GetDecisionInput {
            decision_id: id.map(str::to_owned),
            reading: reading(edit),
        };

        let summary_offset = with(Some(JURI), |reading| {
            reading.detail = Some("summary".to_owned());
            reading.offset = Some(10);
        });
        assert_eq!(
            use_case.execute(summary_offset).await.unwrap_err(),
            AppError::invalid_argument("offset requires detail=\"full\" (or omit detail).")
        );
        let too_small = with(Some(JURI), |reading| reading.max_chars = Some(100));
        assert_eq!(
            use_case.execute(too_small).await.unwrap_err(),
            AppError::invalid_argument("max_chars=100 out of range (500..40000).")
        );
        let stop_words = with(Some(JURI), |reading| reading.focus = Some("le de la".to_owned()));
        assert_eq!(
            use_case.execute(stop_words).await.unwrap_err().code(),
            Some(ErrorCode::InvalidArg)
        );
        // L'identifiant d'abord : un identifiant absent l'emporte sur un `max_chars` invalide.
        let both = with(None, |reading| reading.max_chars = Some(100));
        assert_eq!(
            use_case.execute(both).await.unwrap_err(),
            AppError::invalid_argument("decision_id is required.")
        );
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_offset_past_the_end_is_invalid_arg() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(JURI, "Rejet du pourvoi.")));
        let wanted = GetDecisionInput {
            decision_id: Some(JURI.to_owned()),
            reading: reading(|reading| reading.offset = Some(20_000)),
        };

        let error = GetDecisionUseCase::new(&corpus).execute(wanted).await.unwrap_err();

        assert_eq!(
            error,
            AppError::invalid_argument(
                "offset=20000 is at or past the end of the text (text_length=17); restart from \
                 offset=0 or omit offset."
            )
        );
    }

    #[tokio::test]
    async fn an_empty_decision_text_gives_the_note() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(JURI, "")));

        let output = GetDecisionUseCase::new(&corpus).execute(input(Some(JURI))).await.unwrap();

        assert_eq!(output.reading.text, None);
        assert_eq!(output.reading.text_length, 0);
        assert_eq!(
            output.reading.note.as_deref(),
            Some("No text is available from Légifrance for this record.")
        );
    }

    #[tokio::test]
    async fn not_found_message_by_prefix() {
        // E-15 : le fond nommé suit le préfixe de l'identifiant (00 §5).
        let corpus = FakeCorpus::new();
        for _ in 0..3 {
            corpus
                .push_get_decision(Err(CorpusError::NotFound { what: "/consult/juri".to_owned() }));
        }
        let use_case = GetDecisionUseCase::new(&corpus);
        for (requested, fond) in [(JURI, "JURI"), (CONSTIT, "CONSTIT"), (CETAT, "CETAT")] {
            let error = use_case.execute(input(Some(requested))).await.unwrap_err();
            assert_eq!(
                error,
                AppError::not_found(format!(
                    "No {fond} decision found for {requested}. Check the id, or search with \
                     fr_search."
                ))
            );
        }
    }

    #[tokio::test]
    async fn port_failures_keep_their_code() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Err(CorpusError::Timeout));
        corpus.push_get_decision(Err(CorpusError::RateLimited { retry_after: None }));
        let use_case = GetDecisionUseCase::new(&corpus);

        let timeout = use_case.execute(input(Some(JURI))).await.unwrap_err();
        assert_eq!(timeout.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(timeout.cause(), Some(UpstreamCause::Timeout));
        let limited = use_case.execute(input(Some(JURI))).await.unwrap_err();
        assert_eq!(limited.code(), Some(ErrorCode::RateLimited));
    }
}
