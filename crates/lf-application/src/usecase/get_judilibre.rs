//! Cas d'usage de `fr_get_judilibre` (jalon MJ) : la lecture d'une décision de Judilibre
//! (`GetJudilibreUseCase`, 05 §5.19).
//!
//! Ordre de validation : `id` (requis, 24 caractères hexadécimaux), `part`, puis les cinq
//! paramètres de lecture ([`ReadingRequest::parse`]), le tout avant tout appel au port.
//!
//! La décision lue est découpée par le domaine ([`part_text`]) quand une partie est demandée ; si
//! les zones ou la partie manquent, le texte entier est lu, avec une `note`. Le texte passe ensuite
//! par [`read_with_dispositif`] : résumé par le sommaire de la Cour de cassation, sinon par le
//! dispositif que délimitent les zones (mis en forme comme le texte : caractères invisibles
//! retirés, blancs repliés ; découpé en mode résumé seulement), sinon par le début du texte.
//!
//! Une décision marquée `to_be_deleted` est rendue `not_found`, comme une décision inconnue ; une
//! décision publiée en partie (`partial`) porte une `note`.

use lf_domain::{
    entity::OfficialSummary,
    judilibre::{CaseLawDoc, DecisionPart, JudilibreId, Zones, part_text},
    text::LongText,
};

use crate::{
    budget::{Reading, ReadingRequest, read_with_dispositif},
    error::AppError,
    failure::case_law_failure,
    input::GetJudilibreInput,
    messages,
    output::get_judilibre::GetJudilibreOutput,
    ports::{CaseLawError, JudicialOpenData},
    tools::ToolName,
};

/// Outil que ce cas d'usage sert : nom des messages et de l'audit.
const TOOL: ToolName = ToolName::FR_GET_JUDILIBRE;

/// Remarque d'une décision sans texte.
pub(crate) const NO_TEXT_NOTE: &str = "This decision has no text in the Judilibre open data.";

/// Remarque d'une décision publiée en partie (`partial`).
pub(crate) const PARTIAL_NOTE: &str = "This decision is published in part only: some passages \
                                       were left out by the court.";

/// Détail de journal d'une décision rendue sous un autre identifiant que celui demandé.
const OTHER_ID: &str = "decision returned under another id";

/// Détail de journal d'un texte au-delà de la borne de `LongText`.
const TEXT_TOO_LARGE: &str = "decision text beyond the LongText bound";

/// Lit une décision de Judilibre par son identifiant.
pub(crate) struct GetJudilibreUseCase<'a, J: JudicialOpenData> {
    case_law: &'a J,
}

impl<'a, J: JudicialOpenData> GetJudilibreUseCase<'a, J> {
    /// Cas d'usage qui lit `case_law`.
    pub(crate) fn new(case_law: &'a J) -> Self {
        Self { case_law }
    }

    /// Valide les arguments, lit la décision, puis la rend selon la partie et le mode de lecture
    /// demandés.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` si `id` est absent ou invalide, si `part` est inconnue, si un paramètre de
    ///   lecture est hors bornes (sans appel au port), ou si `offset` dépasse la fin du texte ;
    /// - `not_found` pour une décision inconnue ou marquée `to_be_deleted` ;
    /// - `upstream_error` pour une décision rendue sous un autre identifiant ;
    /// - les autres erreurs du port, traduites par [`case_law_failure`].
    pub(crate) async fn execute(
        &self,
        input: GetJudilibreInput,
    ) -> Result<GetJudilibreOutput, AppError> {
        let raw = input.id.ok_or_else(|| messages::required(messages::id_field(TOOL)))?;
        let id = JudilibreId::parse(&raw).map_err(|error| messages::domain(TOOL, &error))?;
        let part = input
            .part
            .as_deref()
            .map(|value| {
                DecisionPart::parse(value)
                    .ok_or_else(|| messages::not_one_of("part", value, &DecisionPart::NAMES))
            })
            .transpose()?;
        let request = ReadingRequest::parse(&input.reading)?;
        let decision = self
            .case_law
            .get(&id)
            .await
            .map_err(|error| case_law_failure(TOOL, Some(&id), &error))?;
        if decision.to_be_deleted {
            return Err(case_law_failure(TOOL, Some(&id), &CaseLawError::NotFound));
        }
        if decision.id != id {
            let error = CaseLawError::Decode { detail: OTHER_ID.to_owned() };
            return Err(case_law_failure(TOOL, Some(&id), &error));
        }
        present(decision, part, &request)
    }
}

/// Lit la partie demandée, ou le texte entier, et met la décision en forme ; le texte et les
/// zones quittent la décision rendue.
fn present(
    mut decision: CaseLawDoc,
    part: Option<DecisionPart>,
    request: &ReadingRequest,
) -> Result<GetJudilibreOutput, AppError> {
    let raw = std::mem::take(&mut decision.text);
    let zones = std::mem::take(&mut decision.zones);
    let chosen = part.map(|part| (part, part_text(&raw, &zones, part)));
    let missing = chosen.as_ref().and_then(|(part, body)| body.is_none().then_some(*part));
    let (body, read_part) = match &chosen {
        Some((part, Some(body))) => (body.as_str(), Some(*part)),
        Some((_, None)) | None => (raw.as_str(), None),
    };
    let text = long_text(&decision.id, body)?;
    let official = decision
        .summary
        .as_ref()
        .map(|summary| OfficialSummary::new([summary], Vec::<String>::new()));
    let dispositif = || dispositif_of(&raw, &zones, read_part, &text);
    let mut reading = read_with_dispositif(&text, official.as_ref(), dispositif, request)?;
    reading.note = notes(&reading, &text, missing, decision.partial);
    Ok(GetJudilibreOutput { decision, part: read_part, reading })
}

/// Le dispositif qui résume la lecture : celui des zones pour le texte entier, mis en forme
/// comme lui (caractères invisibles retirés, blancs repliés) ; le texte lu lui-même pour la
/// partie `dispositif` ; aucun pour une autre partie, que son propre début résume.
fn dispositif_of(
    raw: &str,
    zones: &Zones,
    read_part: Option<DecisionPart>,
    text: &LongText,
) -> Option<String> {
    match read_part {
        None => part_text(raw, zones, DecisionPart::Dispositif)
            .and_then(|dispositif| LongText::from_plain(&dispositif).ok())
            .map(|dispositif| dispositif.as_str().to_owned()),
        Some(DecisionPart::Dispositif) => Some(text.as_str().to_owned()),
        Some(_) => None,
    }
}

/// Texte canonique de `body`, sa mise en forme du domaine. Un texte au-delà de la borne du
/// domaine est une réponse trop grosse de Judilibre, jamais une faute de l'appelant.
fn long_text(id: &JudilibreId, body: &str) -> Result<LongText, AppError> {
    LongText::from_plain(body).map_err(|_| {
        let error = CaseLawError::TooLarge { detail: TEXT_TOO_LARGE.to_owned() };
        case_law_failure(TOOL, Some(id), &error)
    })
}

/// Remarques de la lecture, jointes par une espace : texte absent (à la place de celle de
/// Légifrance), partie non délimitée, décision publiée en partie.
fn notes(
    reading: &Reading,
    text: &LongText,
    missing: Option<DecisionPart>,
    partial: bool,
) -> Option<String> {
    let empty = text.is_empty().then(|| NO_TEXT_NOTE.to_owned());
    // Hors texte vide, la seule remarque du domaine est celle d'un `focus` sans passage.
    let reading_note = if text.is_empty() { None } else { reading.note.clone() };
    let undelimited = missing.map(|part| {
        format!(
            "Part {} is not delimited in this decision; the whole text is read instead.",
            part.name()
        )
    });
    let partial = partial.then(|| PARTIAL_NOTE.to_owned());
    let notes: Vec<String> =
        [empty, reading_note, undelimited, partial].into_iter().flatten().collect();
    (!notes.is_empty()).then(|| notes.join(" "))
}

#[cfg(test)]
mod tests {
    use lf_domain::{judilibre::ZoneSpan, text::SummarySource};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::AuditMetric,
        error::{ErrorCode, UpstreamCause},
        input::ReadingInput,
        testing::{CaseLawRequest, FakeCaseLaw, fixtures::judilibre},
    };

    fn input(id: &str) -> GetJudilibreInput {
        GetJudilibreInput { id: Some(id.to_owned()), ..GetJudilibreInput::default() }
    }

    fn with_part(part: &str) -> GetJudilibreInput {
        GetJudilibreInput { part: Some(part.to_owned()), ..input(judilibre::CC_ID) }
    }

    async fn run(
        input: GetJudilibreInput,
        response: Result<CaseLawDoc, CaseLawError>,
    ) -> (Result<GetJudilibreOutput, AppError>, FakeCaseLaw) {
        let case_law = FakeCaseLaw::new();
        case_law.push_get(response);
        let result = GetJudilibreUseCase::new(&case_law).execute(input).await;
        (result, case_law)
    }

    async fn read(input: GetJudilibreInput) -> GetJudilibreOutput {
        run(input, Ok(judilibre::doc(judilibre::CC_ID))).await.0.unwrap()
    }

    #[tokio::test]
    async fn a_short_decision_is_returned_whole_with_its_metadata() {
        let (result, case_law) =
            run(input(" 5FCA624B7EEB384A40CAFB85 "), Ok(judilibre::doc(judilibre::CC_ID))).await;
        let output = result.unwrap();
        assert_eq!(
            case_law.seen_requests(),
            vec![CaseLawRequest::Get(judilibre::id(judilibre::CC_ID))]
        );
        let mut expected = judilibre::doc(judilibre::CC_ID);
        expected.text = String::new();
        expected.zones = Zones::default();
        assert_eq!(output.decision, expected);
        assert_eq!(output.part, None);
        // Le texte tient dans le résumé : il est rendu entier, sans résumé tiré de lui-même.
        let whole = LongText::from_plain(judilibre::TEXT).unwrap();
        assert_eq!(output.reading.text.as_deref(), Some(whole.as_str()));
        assert_eq!(output.reading.summary, None);
        assert_eq!(output.reading.note, None);
        assert_eq!(output.output_count_or_size(), u64::from(whole.char_len()));
    }

    #[tokio::test]
    async fn a_long_decision_is_summarised_by_its_dispositif() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        // Un exposé plus long que le plafond du résumé : la décision n'est plus rendue entière.
        let filler = "Motif développé. ".repeat(100);
        doc.text =
            judilibre::TEXT.replace("Faits et procédure", &format!("Faits et procédure {filler}"));
        doc.zones = shifted(&doc.text);
        let (result, _) = run(input(judilibre::CC_ID), Ok(doc)).await;
        let reading = result.unwrap().reading;
        let summary = reading.summary.unwrap();
        assert_eq!(summary.source, SummarySource::Dispositif);
        assert!(summary.text.starts_with("PAR CES MOTIFS"), "{}", summary.text);
        assert_eq!(reading.text, None);
    }

    #[tokio::test]
    async fn the_dispositif_summary_loses_invisible_characters() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        let filler = "Motif développé. ".repeat(100);
        // Un contrôle bidirectionnel, un tag Unicode et une espace de largeur nulle dans le
        // dispositif, que le texte lu perd aussi.
        doc.text = judilibre::TEXT
            .replace("Faits et procédure", &format!("Faits et procédure {filler}"))
            .replace("PAR CES MOTIFS", "PAR CES\u{202E} MOTIFS\u{E0041}\u{200B}");
        doc.zones = shifted(&doc.text);
        let (result, _) = run(input(judilibre::CC_ID), Ok(doc)).await;
        let summary = result.unwrap().reading.summary.unwrap();
        assert_eq!(summary.source, SummarySource::Dispositif);
        assert!(summary.text.starts_with("PAR CES MOTIFS"), "{}", summary.text);
        assert!(
            !summary.text.contains(['\u{202E}', '\u{E0041}', '\u{200B}']),
            "{:?}",
            summary.text
        );
    }

    /// Zones de `text`, recalculées sur les mêmes débuts de partie que [`judilibre::zones`].
    fn shifted(text: &str) -> Zones {
        let at = |needle: &str| {
            u32::try_from(text.get(..text.find(needle).unwrap()).unwrap().encode_utf16().count())
                .unwrap()
        };
        let end = u32::try_from(text.encode_utf16().count()).unwrap().saturating_add(1);
        Zones::of([
            (DecisionPart::Introduction, ZoneSpan { start: 0, end: at("Faits et procédure") }),
            (DecisionPart::Expose, ZoneSpan { start: at("Faits et procédure"), end: at("Examen") }),
            (DecisionPart::Moyens, ZoneSpan { start: at("Examen"), end: at("Réponse") }),
            (DecisionPart::Motivations, ZoneSpan { start: at("Réponse"), end: at("PAR CES") }),
            (DecisionPart::Dispositif, ZoneSpan { start: at("PAR CES"), end }),
        ])
    }

    #[tokio::test]
    async fn the_official_summary_comes_first() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        doc.summary = Some("Une donation entre vifs est irrévocable.".to_owned());
        let (result, _) = run(input(judilibre::CC_ID), Ok(doc)).await;
        let summary = result.unwrap().reading.summary.unwrap();
        assert_eq!(
            (summary.source, summary.text.as_str()),
            (SummarySource::Official, "Une donation entre vifs est irrévocable.")
        );
    }

    #[tokio::test]
    async fn a_part_reads_only_its_fragments() {
        let output = read(with_part("motivations")).await;
        assert_eq!(output.part, Some(DecisionPart::Motivations));
        assert_eq!(
            output.reading.text.as_deref(),
            Some(
                "Réponse de la Cour\n\n3. Vu l'article 894 du code civil, la donation est un acte \
                 irrévocable."
            )
        );
        assert_eq!(output.reading.text_length, 91);
        assert_eq!(output.reading.note, None);
        let ruling = read(with_part(" DISPOSITIF ")).await;
        assert_eq!(ruling.part, Some(DecisionPart::Dispositif));
        assert!(ruling.reading.text.unwrap().starts_with("PAR CES MOTIFS, la Cour :"));
    }

    #[tokio::test]
    async fn missing_zones_fall_back_to_full_text() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        doc.zones = Zones::default();
        let (result, _) = run(with_part("motivations"), Ok(doc)).await;
        let output = result.unwrap();
        assert_eq!(output.part, None);
        let whole = LongText::from_plain(judilibre::TEXT).unwrap();
        assert_eq!(output.reading.text.as_deref(), Some(whole.as_str()));
        assert_eq!(
            output.reading.note.as_deref(),
            Some(
                "Part motivations is not delimited in this decision; the whole text is read \
                  instead."
            )
        );
        // Une partie absente des zones d'une décision zonée : de même.
        let output = read(with_part("annexes")).await;
        assert_eq!(output.part, None);
        assert!(output.reading.note.unwrap().starts_with("Part annexes is not delimited"));
    }

    #[tokio::test]
    async fn deleted_decision_is_not_found() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        doc.to_be_deleted = true;
        let (result, _) = run(input(judilibre::CC_ID), Ok(doc)).await;
        assert_eq!(
            result.unwrap_err(),
            AppError::not_found(
                "No Judilibre decision 5fca624b7eeb384a40cafb85. Search with fr_search_judilibre."
            )
        );
        let (result, _) = run(input(judilibre::CC_ID), Err(CaseLawError::NotFound)).await;
        assert_eq!(result.unwrap_err().code(), Some(ErrorCode::NotFound));
    }

    #[tokio::test]
    async fn a_decision_under_another_id_is_an_upstream_error() {
        let (result, _) = run(input(judilibre::CC_ID), Ok(judilibre::doc(judilibre::CA_ID))).await;
        let error = result.unwrap_err();
        assert_eq!(
            (error.code(), error.cause()),
            (Some(ErrorCode::UpstreamError), Some(UpstreamCause::Decode))
        );
    }

    #[tokio::test]
    async fn a_partial_decision_says_so() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        doc.partial = true;
        let (result, _) = run(input(judilibre::CC_ID), Ok(doc.clone())).await;
        assert_eq!(result.unwrap().reading.note.as_deref(), Some(PARTIAL_NOTE));
        doc.zones = Zones::default();
        let (result, _) = run(with_part("expose"), Ok(doc)).await;
        let note = result.unwrap().reading.note.unwrap();
        assert!(note.starts_with("Part expose is not delimited"), "{note}");
        assert!(note.ends_with(PARTIAL_NOTE), "{note}");
    }

    #[tokio::test]
    async fn an_empty_decision_names_judilibre() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        doc.text = String::new();
        doc.zones = Zones::default();
        let (result, _) = run(input(judilibre::CC_ID), Ok(doc)).await;
        let reading = result.unwrap().reading;
        assert_eq!(reading.note.as_deref(), Some(NO_TEXT_NOTE));
        assert_eq!(reading.text_length, 0);
    }

    #[tokio::test]
    async fn a_text_beyond_the_domain_bound_is_too_large() {
        let mut doc = judilibre::doc(judilibre::CC_ID);
        doc.text = "a".repeat(usize::try_from(LongText::MAX_CHARS).unwrap() + 1);
        doc.zones = Zones::default();
        let (result, _) = run(input(judilibre::CC_ID), Ok(doc)).await;
        assert_eq!(result.unwrap_err().message(), "the Judilibre answer is too large; report it.");
    }

    #[tokio::test]
    async fn a_focus_without_passage_keeps_the_reading_note() {
        let focused = GetJudilibreInput {
            reading: ReadingInput {
                focus: Some("hypothèque".to_owned()),
                ..ReadingInput::default()
            },
            ..input(judilibre::CC_ID)
        };
        let reading = read(focused).await.reading;
        assert_eq!(reading.passages, Some(Vec::new()));
        assert_eq!(
            reading.note.as_deref(),
            Some("No passage matches focus; read the text with detail=\"full\".")
        );
    }

    #[tokio::test]
    async fn invalid_args_never_reach_the_port() {
        let cases = [
            (GetJudilibreInput::default(), "id is required."),
            (
                input("JURITEXT000045653019"),
                "id='JURITEXT000045653019' is not a Judilibre id (24 hexadecimal characters, e.g. \
                 5fca624b7eeb384a40cafb85); find one with fr_search_judilibre.",
            ),
            (
                with_part("summary"),
                "part='summary' must be one of ['introduction', 'expose', 'moyens', \
                 'motivations', 'dispositif', 'annexes'].",
            ),
            (
                GetJudilibreInput {
                    reading: ReadingInput { max_chars: Some(10), ..ReadingInput::default() },
                    ..input(judilibre::CC_ID)
                },
                "max_chars=10 out of range (500..40000).",
            ),
        ];
        for (input, message) in cases {
            let (result, case_law) = run(input, Ok(judilibre::doc(judilibre::CC_ID))).await;
            assert_eq!(case_law.calls(), 0, "{message}");
            assert_eq!(result.unwrap_err(), AppError::invalid_argument(message));
        }
    }

    #[tokio::test]
    async fn an_offset_past_the_part_is_refused_after_the_read() {
        let past = GetJudilibreInput {
            reading: ReadingInput {
                detail: Some("full".to_owned()),
                offset: Some(5_000),
                ..ReadingInput::default()
            },
            ..with_part("motivations")
        };
        let (result, case_law) = run(past, Ok(judilibre::doc(judilibre::CC_ID))).await;
        assert_eq!(case_law.calls(), 1);
        assert_eq!(
            result.unwrap_err().message(),
            "offset=5000 is at or past the end of the text (text_length=91); restart from \
             offset=0 or omit offset."
        );
    }

    #[tokio::test]
    async fn port_errors_name_the_decision_asked_for() {
        let (result, _) = run(input(judilibre::CA_ID), Err(CaseLawError::Disabled)).await;
        assert_eq!(result.unwrap_err().code(), Some(ErrorCode::ConfigError));
        let (result, _) = run(input(judilibre::CA_ID), Err(CaseLawError::NotFound)).await;
        assert_eq!(
            result.unwrap_err().message(),
            "No Judilibre decision 63c8eefddc5b777c90992fd8. Search with fr_search_judilibre."
        );
    }

    #[test]
    fn only_the_whole_text_or_the_dispositif_is_summarised_by_the_dispositif() {
        let zones = judilibre::zones();
        let whole = LongText::from_plain(judilibre::TEXT).unwrap();
        let ruling = dispositif_of(judilibre::TEXT, &zones, None, &whole).unwrap();
        assert!(ruling.starts_with("PAR CES MOTIFS"));
        // La partie `dispositif` lue est son propre résumé : le texte lu, déjà mis en forme.
        let read = part_text(judilibre::TEXT, &zones, DecisionPart::Dispositif).unwrap();
        let read = LongText::from_plain(&read).unwrap();
        assert_eq!(
            dispositif_of(judilibre::TEXT, &zones, Some(DecisionPart::Dispositif), &read),
            Some(ruling)
        );
        let moyens = Some(DecisionPart::Moyens);
        assert_eq!(dispositif_of(judilibre::TEXT, &zones, moyens, &whole), None);
    }
}
