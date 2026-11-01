//! Cas d'usage de `fr_resolve` (v0.2 (M6)) : la résolution d'une citation libre
//! (`ResolveUseCase`, 05 §5.10, annexe B §7.8).
//!
//! # Étapes
//!
//! 1. Validation sans réseau : `citation` de 1 à 1 000 caractères, puis `date` (`AAAA-MM-JJ`).
//! 2. Analyse : la **première** citation reconnue est résolue ; les suivantes ne sont que comptées
//!    (une note renvoie à `fr_verify_citations`). Aucune citation reconnue : `candidates: []`.
//! 3. `interpreted_as` : la forme canonique de ce que le parseur a lu.
//! 4. Résolution par le moteur partagé avec `fr_verify_citations` (`Resolver::resolve_ref`) ; la
//!    `date` de l'appel est la date de version des articles, à défaut d'une date citée. Sans
//!    `date`, c'est la version en vigueur aujourd'hui à Paris (`ArticleAt::Today`, D5).
//! 5. Les candidats sont rangés du plus sûr au moins sûr et limités à cinq.
//!
//! Rien de trouvé n'est pas une erreur (pas de `not_found`) : la liste est vide et la note dit
//! pourquoi. Seul un échec du corpus fait échouer l'appel, puisqu'il n'y a qu'une citation.
//!
//! Limite de cette version : l'ordre des candidats de même confiance est celui de l'API ; l'égalité
//! avec la date citée (annexe B §7.8) départage déjà les décisions dans le moteur de résolution.

use std::cmp::Reverse;

use lf_domain::{
    citation::{CitationRef, Confidence, ParseOptions, Spanned, format::canonical, parse_with},
    date::VersionDate,
};

use crate::{
    citation::{
        Candidate, CandidateKind, Note, ResolvedRecord,
        policy::MAX_CANDIDATES,
        resolve::{CodeTable, Outcome, Proof, Resolver, needs_codes},
    },
    error::AppError,
    failure::port_failure,
    input::ResolveInput,
    messages,
    output::resolve::ResolveOutput,
    ports::{Clock, LegalCorpus},
    tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_RESOLVE;

/// Longueur maximale de `citation`, en caractères (05 §5.10, 03 §8.8).
pub(crate) const CITATION_MAX_CHARS: usize = 1_000;

/// Cas d'usage `fr_resolve`.
pub(crate) struct ResolveUseCase<'a, C: LegalCorpus, K: Clock> {
    corpus: &'a C,
    clock: &'a K,
    proof: Proof,
}

impl<'a, C: LegalCorpus, K: Clock> ResolveUseCase<'a, C, K> {
    /// `proof` est ce que prouve l'absence dans la source interrogée : dans le bac à sable de PISTE
    /// ou dans l'instantané local, l'absence d'un enregistrement a la note de cette source.
    pub(crate) const fn new(corpus: &'a C, clock: &'a K, proof: Proof) -> Self {
        Self { corpus, clock, proof }
    }

    /// Résout la première citation de `input.citation`.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour une entrée refusée (sans appel au corpus) ; l'erreur amont quand le
    /// corpus échoue sur la citation.
    pub(crate) async fn execute(&self, input: ResolveInput) -> Result<ResolveOutput, AppError> {
        let ResolveInput { citation, date } = input;
        let citation = citation
            .filter(|citation| !citation.trim().is_empty())
            .ok_or_else(|| messages::required("citation"))?;
        let length = citation.chars().count();
        if length > CITATION_MAX_CHARS {
            return Err(messages::invalid_argument(format!(
                "citation is too long ({length} characters, max {CITATION_MAX_CHARS}): pass one \
                 citation, or use fr_verify_citations for a whole text."
            )));
        }
        let date = date
            .as_deref()
            .map(|raw| VersionDate::parse(raw).map_err(|error| messages::domain(TOOL, &error)))
            .transpose()?;
        let today = VersionDate::try_from(self.clock.today_paris())
            .map_err(|_| AppError::config("the clock is outside the supported date range."))?;

        let options = ParseOptions { today: Some(today.as_civil()), ..ParseOptions::default() };
        let parsed = parse_with(&citation, &options);
        let Some((first, others)) = parsed.split_first() else {
            return Ok(ResolveOutput {
                interpreted_as: None,
                candidates: Vec::new(),
                notes: vec![Note::NoCitationRecognised],
            });
        };

        let outcome = self.resolve(first, today, date).await;
        if let Outcome::Failed(error) = &outcome {
            return Err(port_failure(TOOL, "the citation", error));
        }
        let mut notes: Vec<Note> = note_of(&outcome, self.proof).into_iter().collect();
        if !others.is_empty() {
            notes.push(Note::OtherCitations { count: others.len() });
        }
        Ok(ResolveOutput {
            interpreted_as: canonical(&first.value),
            candidates: best_first(candidates_of(outcome)),
            notes,
        })
    }

    /// Résultat de la résolution de `item` ; la liste des codes n'est lue que si la citation en a
    /// besoin.
    async fn resolve(
        &self,
        item: &Spanned<CitationRef>,
        today: VersionDate,
        date: Option<VersionDate>,
    ) -> Outcome {
        let codes = if needs_codes(item) {
            CodeTable::from_result(self.corpus.list_codes().await)
        } else {
            CodeTable::empty()
        };
        Resolver::new(self.corpus, today, &codes).resolve_ref(item, date).await.outcome
    }
}

/// Note de l'issue. Une liste de candidats non vide parle d'elle-même : la note « N records match;
/// call `fr_resolve` » du moteur renverrait ici à l'outil en cours.
fn note_of(outcome: &Outcome, proof: Proof) -> Option<Note> {
    match outcome {
        Outcome::Candidates { items, .. } if !items.is_empty() => None,
        other => other.note(proof),
    }
}

/// Les enregistrements que l'issue propose. Le document réel d'une citation en écart est un
/// candidat de confiance moyenne (annexe B §7.6, §7.8).
fn candidates_of(outcome: Outcome) -> Vec<Candidate> {
    let single = |record: ResolvedRecord, confidence| {
        vec![Candidate { kind: CandidateKind::of(&record.id), record, confidence }]
    };
    match outcome {
        Outcome::Verified { record, confidence, .. } => single(record, confidence),
        Outcome::Mismatch { record, .. } => single(record, Confidence::Medium),
        Outcome::Candidates { items, .. } => items,
        Outcome::Invalid { .. }
        | Outcome::Absent { .. }
        | Outcome::Unresolvable { .. }
        | Outcome::Failed(_) => Vec::new(),
    }
}

/// Du plus sûr au moins sûr (le tri est stable : à confiance égale, l'ordre d'arrivée), cinq au
/// plus.
fn best_first(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by_key(|candidate| Reverse(candidate.confidence));
    candidates.truncate(MAX_CANDIDATES);
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::ErrorCode,
        ports::{ArticleAt, CorpusError, PisteEnvironment},
        testing::{FakeCorpus, FixedClock, PortRequest},
    };
    use chrono::{TimeZone, Utc};
    use lf_domain::{
        citation::Sigle,
        date::CivilDate,
        entity::{ArticleText, CodeListing, SearchHit},
        fond::Fond,
        id::{ArticleId, DocumentId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    const CIVIL: &str = "LEGITEXT000006070721";

    /// Le 29 octobre 2026 à Paris, alors qu'il est encore le 28 en UTC : « aujourd'hui » ne se lit
    /// pas sur l'heure UTC.
    fn clock() -> FixedClock {
        FixedClock::new(
            Utc.with_ymd_and_hms(2026, 10, 28, 23, 30, 0).unwrap(),
            CivilDate::new(2026, 10, 29).unwrap(),
        )
    }

    fn today() -> VersionDate {
        VersionDate::parse("2026-10-29").unwrap()
    }

    fn corpus_with_civil_code() -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse(CIVIL).unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
        corpus
    }

    fn article(n: u32, num: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
            num: Some(num.to_owned()),
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: CivilDate::new(2016, 10, 1),
            date_fin: None,
            text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
            nota: None,
        }
    }

    fn not_found() -> CorpusError {
        CorpusError::NotFound { what: "/consult".to_owned() }
    }

    fn input(citation: &str, date: Option<&str>) -> ResolveInput {
        ResolveInput { citation: Some(citation.to_owned()), date: date.map(str::to_owned) }
    }

    async fn run(
        corpus: &FakeCorpus,
        env: PisteEnvironment,
        input: ResolveInput,
    ) -> Result<ResolveOutput, AppError> {
        run_with(corpus, Proof::of(env, false), input).await
    }

    async fn run_with(
        corpus: &FakeCorpus,
        proof: Proof,
        input: ResolveInput,
    ) -> Result<ResolveOutput, AppError> {
        let clock = clock();
        ResolveUseCase::new(corpus, &clock, proof).execute(input).await
    }

    async fn resolve(corpus: &FakeCorpus, citation: &str) -> ResolveOutput {
        run(corpus, PisteEnvironment::Production, input(citation, None)).await.unwrap()
    }

    fn candidate(n: u32, confidence: Confidence) -> Candidate {
        let id = DocumentId::parse(&format!("LEGITEXT{n:012}")).unwrap();
        Candidate {
            kind: CandidateKind::of(&id),
            record: ResolvedRecord {
                citation: format!("Texte {n}"),
                source_url: format!("https://example.test/{n}"),
                id,
                ecli: None,
            },
            confidence,
        }
    }

    fn ids(candidates: &[Candidate]) -> Vec<String> {
        candidates.iter().map(|candidate| candidate.record.id.as_str().to_owned()).collect()
    }

    fn texts(n: u32) -> Vec<SearchHit> {
        (1..=n)
            .map(|n| {
                let id = DocumentId::parse(&format!("LEGITEXT{n:012}")).unwrap();
                SearchHit::new(Fond::Loda, id, None).unwrap()
            })
            .collect()
    }

    #[test]
    fn the_candidates_are_sorted_by_confidence_then_arrival_and_cut_at_five() {
        use Confidence::{High, Low, Medium};
        let mixed = [Low, High, Medium, Low, High, Medium, Low];
        let candidates: Vec<Candidate> =
            (1..).zip(mixed).map(|(n, confidence)| candidate(n, confidence)).collect();

        let kept = best_first(candidates);

        let confidences: Vec<Confidence> = kept.iter().map(|c| c.confidence).collect();
        assert_eq!(confidences, [High, High, Medium, Medium, Low]);
        // Stable : à confiance égale, l'ordre d'arrivée (2 avant 5, 3 avant 6).
        assert_eq!(
            ids(&kept),
            ["LEGITEXT000000000002", "LEGITEXT000000000005", "LEGITEXT000000000003"]
                .into_iter()
                .chain(["LEGITEXT000000000006", "LEGITEXT000000000001"])
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
        assert_eq!(best_first(Vec::new()), Vec::new());
    }

    #[tokio::test]
    async fn at_most_5_candidates_best_first() {
        // Huit textes portent ce numéro : cinq candidats de confiance basse, sans note (la liste
        // parle d'elle-même), dans l'ordre de l'API.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(texts(8)));
        let output = resolve(&corpus, "loi n° 2016-1321").await;
        assert_eq!(output.candidates.len(), 5);
        assert!(output.candidates.iter().all(|c| c.confidence == Confidence::Low));
        assert_eq!(
            ids(&output.candidates),
            (1..=5).map(|n| format!("LEGITEXT{n:012}")).collect::<Vec<_>>()
        );
        assert!(output.candidates.iter().all(|c| c.kind == CandidateKind::Loda));
        assert_eq!(output.notes, []);

        // Un seul texte : un candidat, de confiance moyenne (aucune date citée pour confirmer).
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(texts(1)));
        let output = resolve(&corpus, "loi n° 2016-1321").await;
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.candidates[0].confidence, Confidence::Medium);
    }

    #[tokio::test]
    async fn interpreted_as_present() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(6_419_288, "9")));
        let output = resolve(&corpus, "art. 9 C. civ.").await;
        assert_eq!(output.interpreted_as.as_deref(), Some("Code civil, art. 9"));
        assert_eq!(output.candidates.len(), 1);
        let found = &output.candidates[0];
        assert_eq!(found.record.id.as_str(), "LEGIARTI000006419288");
        assert_eq!(found.record.citation, "Code civil, art. 9");
        assert_eq!(found.kind, CandidateKind::Article);
        assert_eq!(found.confidence, Confidence::High);
        assert_eq!(
            output.notes,
            [Note::VersionSince(CivilDate::new(2016, 10, 1).unwrap())],
            "la version en vigueur dit depuis quand"
        );

        // Sans forme sûre (ni code ni texte), rien n'est interprété et rien n'est cherché.
        let corpus = FakeCorpus::new();
        let output = resolve(&corpus, "article 9").await;
        assert_eq!(output.interpreted_as, None);
        assert_eq!(output.candidates, []);
        assert_eq!(output.notes, [Note::MissingTarget { num: "9".to_owned() }]);
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn date_defaults_today_paris() {
        // Sans `date` : la version en vigueur aujourd'hui à Paris (le 29, alors qu'il est le 28 en
        // UTC), et la date de l'appel qui vaut ce jour donne le même signal (D5).
        for date in [None, Some("2026-10-29")] {
            let corpus = corpus_with_civil_code();
            corpus.push_find_article(Ok(article(1, "9")));
            let output = run(&corpus, PisteEnvironment::Production, input("art. 9 c. civ.", date))
                .await
                .unwrap();
            assert_eq!(output.candidates.len(), 1);
            assert_eq!(
                corpus.seen_requests(),
                vec![
                    PortRequest::ListCodes,
                    PortRequest::FindArticle(
                        LegiTextId::parse(CIVIL).unwrap(),
                        "9".to_owned(),
                        ArticleAt::Today(today())
                    ),
                ],
                "{date:?}"
            );
        }

        // Une autre date : cette version-là, et la note de la version en vigueur disparaît.
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "9")));
        let output =
            run(&corpus, PisteEnvironment::Production, input("art. 9 c. civ.", Some("2016-09-30")))
                .await
                .unwrap();
        assert_eq!(output.notes, []);
        assert_eq!(
            corpus.seen_requests().last(),
            Some(&PortRequest::FindArticle(
                LegiTextId::parse(CIVIL).unwrap(),
                "9".to_owned(),
                ArticleAt::On(VersionDate::parse("2016-09-30").unwrap())
            ))
        );
    }

    #[tokio::test]
    async fn citation_and_date_are_validated_before_any_call() {
        let corpus = FakeCorpus::new();
        let env = PisteEnvironment::Production;
        for missing in [None, Some(""), Some("  \n ")] {
            let input = ResolveInput { citation: missing.map(str::to_owned), date: None };
            let error = run(&corpus, env, input).await.unwrap_err();
            assert_eq!(error.message(), "citation is required.", "{missing:?}");
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        }
        let long = "a".repeat(CITATION_MAX_CHARS + 1);
        let error = run(&corpus, env, input(&long, None)).await.unwrap_err();
        assert_eq!(
            error.message(),
            "citation is too long (1001 characters, max 1000): pass one citation, or use \
             fr_verify_citations for a whole text."
        );
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        let error = run(&corpus, env, input("art. 9 c. civ.", Some("2016-13-45"))).await;
        assert_eq!(
            error.unwrap_err().message(),
            "date='2016-13-45' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15)."
        );
        assert_eq!(corpus.calls(), 0);
        // La borne est admise.
        let limit = "a".repeat(CITATION_MAX_CHARS);
        assert!(run(&corpus, env, input(&limit, None)).await.is_ok());
    }

    #[tokio::test]
    async fn a_clock_outside_the_supported_range_is_a_configuration_error() {
        // Une horloge réglée avant 1500 : aucune version n'en découle, rien n'est demandé au corpus.
        let corpus = FakeCorpus::new();
        let clock = FixedClock::new(
            Utc.with_ymd_and_hms(1400, 1, 1, 0, 0, 0).unwrap(),
            CivilDate::new(1400, 1, 1).unwrap(),
        );
        let usecase = ResolveUseCase::new(&corpus, &clock, Proof::Production);
        let error = usecase.execute(input("art. 9 c. civ.", None)).await.unwrap_err();
        assert_eq!(error.message(), "the clock is outside the supported date range.");
        assert_eq!(error.code(), Some(ErrorCode::ConfigError));
        assert_eq!(corpus.calls(), 0);
    }

    /// L2-09 (resolve-4, gap3-3) : la note d'un article sans cible ne renvoie plus l'outil à
    /// lui-même et ne dit pas qu'aucun code n'a été donné quand un sigle inconnu l'a été.
    #[tokio::test]
    async fn a_missing_or_unknown_target_note_names_no_circular_tool() {
        for (citation, num) in [
            ("article 1", "1"),
            ("RGPD article 6", "6"),
            ("art. L. 111-1 CGFP", "L111-1"),
            ("art. 1240 code civ.", "1240"),
        ] {
            let corpus = FakeCorpus::new();
            let output = resolve(&corpus, citation).await;
            assert_eq!(output.candidates, [], "{citation}");
            assert_eq!(output.notes, [Note::MissingTarget { num: num.to_owned() }], "{citation}");
            let text = output.notes[0].text();
            assert!(text.starts_with("No recognised code or text for article "), "{text}");
            assert!(text.contains("an unknown abbreviation counts as missing"), "{text}");
            assert!(text.contains(&format!("\"article {num} du code civil\"")), "{text}");
            assert!(!text.contains("fr_resolve") && !text.contains("No code or text given"));
            assert_eq!(corpus.calls(), 0, "{citation}");
        }
    }

    /// Jumeau de `a_missing_or_unknown_target_note_names_no_circular_tool` : un sigle ou une forme
    /// connus résolvent toujours.
    #[tokio::test]
    async fn a_known_abbreviation_still_resolves() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "1240")));
        let output = resolve(&corpus, "art. 1240 C. civ.").await;
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.interpreted_as.as_deref(), Some("Code civil, art. 1240"));
        assert!(
            output.notes.iter().all(|note| !matches!(note, Note::MissingTarget { .. })),
            "{:?}",
            output.notes
        );
    }

    /// L2-09 (gap3-11) : la note du CPCE nomme ses deux codes.
    #[tokio::test]
    async fn the_cpce_note_names_both_codes() {
        let corpus = FakeCorpus::new();
        let output = resolve(&corpus, "art. L. 32 CPCE").await;
        assert_eq!(output.candidates, []);
        assert_eq!(output.notes, [Note::AmbiguousSigle { sigle: Sigle::Cpce }]);
        let text = output.notes[0].text();
        assert!(text.contains("Code des postes et des communications électroniques"), "{text}");
        assert!(text.contains("Code des procédures civiles d'exécution"), "{text}");
        assert!(!text.contains("two codes"), "{text}");
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn no_citation_recognised_is_a_success_with_a_note() {
        let corpus = FakeCorpus::new();
        let output = resolve(&corpus, "bonjour tout le monde").await;
        assert_eq!(output.interpreted_as, None);
        assert_eq!(output.candidates, []);
        assert_eq!(output.notes, [Note::NoCitationRecognised]);
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn only_the_first_citation_is_resolved_and_the_others_are_counted() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "9")));
        let output =
            resolve(&corpus, "article 9 du code civil, puis l'article 1240 du code civil").await;
        assert_eq!(output.interpreted_as.as_deref(), Some("Code civil, art. 9"));
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(
            output.notes,
            [
                Note::VersionSince(CivilDate::new(2016, 10, 1).unwrap()),
                Note::OtherCitations { count: 1 }
            ]
        );
        assert_eq!(corpus.calls_of("find_article"), 1, "une seule citation est cherchée");
    }

    #[tokio::test]
    async fn nothing_found_is_an_empty_list_with_the_reason_never_not_found() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Err(not_found()));
        let output = resolve(&corpus, "article 9999 du code civil").await;
        assert_eq!(output.candidates, []);
        assert_eq!(output.interpreted_as.as_deref(), Some("Code civil, art. 9999"));
        assert_eq!(
            output.notes[0].text(),
            "Code civil has no article 9999 in force on 2026-10-29. The article may exist in earlier versions; retry with a date."
        );

        // En bac à sable, la note dit que ses données diffèrent de la production.
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Err(not_found()));
        let output =
            run(&corpus, PisteEnvironment::Sandbox, input("article 9999 du code civil", None))
                .await
                .unwrap();
        assert_eq!(output.candidates, []);
        assert!(output.notes[0].text().contains("sandbox data differ from production"));

        // Dans l'instantané local de l'index DILA, la note dit qu'il peut être périmé, sans parler
        // de bac à sable : l'absence ne prouve rien.
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Err(not_found()));
        let output = run_with(&corpus, Proof::Snapshot, input("article 9999 du code civil", None))
            .await
            .unwrap();
        assert_eq!(output.candidates, []);
        assert_eq!(
            output.notes[0].text(),
            "No article record matches Code civil, art. 9999 on 2026-10-29 in the local DILA \
             snapshot, which may be out of date."
        );
    }

    #[tokio::test]
    async fn a_record_that_contradicts_the_citation_is_a_medium_candidate_with_the_gap() {
        let corpus = corpus_with_civil_code();
        // L'API rend l'article « 10 » quand on a demandé le 9.
        corpus.push_find_article(Ok(article(1, "10")));
        let output = resolve(&corpus, "article 9 du code civil").await;
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.candidates[0].confidence, Confidence::Medium);
        assert!(matches!(output.notes.as_slice(), [Note::Mismatch { .. }]), "{:?}", output.notes);
    }

    #[tokio::test]
    async fn a_corpus_failure_fails_the_call() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Err(CorpusError::Timeout));
        let error =
            run(&corpus, PisteEnvironment::Production, input("article 9 du code civil", None))
                .await
                .unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
    }
}
