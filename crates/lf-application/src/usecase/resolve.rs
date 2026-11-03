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
    citation::{
        CitationRef, Confidence, ParseOptions, Span, Spanned, format::canonical, parse_with,
        unread_article_mentions,
    },
    date::VersionDate,
};

use crate::{
    citation::{
        Candidate, CandidateKind, Note, ResolvedRecord,
        notes::reading_gaps,
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
            // Un numéro d'article écrit mais refusé (« 787 b ») n'est pas une citation absente.
            let note = if unread_article_mentions(&citation, &[]) {
                Note::ArticleNotRead
            } else {
                Note::NoCitationRecognised
            };
            return Ok(ResolveOutput {
                interpreted_as: None,
                candidates: Vec::new(),
                notes: vec![note],
            });
        };
        // Une mention d'article refusée (« art. 757 b ») à côté d'une citation lue : elle est dite
        // en tête, pour qu'une autre citation résolue ne la fasse pas oublier.
        let spans: Vec<Span> = parsed.iter().map(|cite| cite.span).collect();

        let outcome = self.resolve(first, today, date).await;
        if let Outcome::Failed(error) = &outcome {
            return Err(port_failure(TOOL, "the citation", error));
        }
        let mut notes: Vec<Note> = Vec::new();
        if unread_article_mentions(&citation, &spans) {
            notes.push(Note::ArticleNotRead);
        }
        notes.extend(note_of(&outcome, self.proof));
        if matches!(outcome, Outcome::Verified { .. }) {
            notes.extend(reading_gaps(first));
        }
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
        citation::Validity,
        error::ErrorCode,
        ports::{ArticleAt, CorpusError, PisteEnvironment},
        testing::{FakeCorpus, FixedClock, PortRequest},
    };
    use chrono::{TimeZone, Utc};
    use lf_domain::{
        citation::Sigle,
        date::CivilDate,
        entity::{Act, ArticleDraft, ArticleText, CodeListing, SearchHit, TextBody, Toc, TocDraft},
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
                validity: None,
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

    /// L2-07 (gap3-7) : une plage donne un candidat unique de confiance moyenne, et la note dit
    /// que seules les deux bornes ont été lues.
    #[tokio::test]
    async fn a_range_is_one_medium_candidate_with_the_range_note() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "1240")));
        corpus.push_find_article(Ok(article(2, "1244")));
        let output = resolve(&corpus, "articles 1240 à 1244 du code civil").await;
        assert_eq!(output.interpreted_as.as_deref(), Some("Code civil, art. 1240 à 1244"));
        assert_eq!(ids(&output.candidates), vec!["LEGIARTI000000000001".to_owned()]);
        assert_eq!(output.candidates[0].confidence, Confidence::Medium);
        assert_eq!(output.notes, [Note::RangeEndpointsOnly]);
        assert_eq!(corpus.calls_of("find_article"), 2);
    }

    #[tokio::test]
    async fn a_range_with_a_missing_last_article_has_no_candidate_and_says_why() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "1240")));
        corpus.push_find_article(Err(not_found()));
        let output = resolve(&corpus, "articles 1240 à 9999 du code civil").await;
        assert_eq!(output.candidates, []);
        assert_eq!(output.notes, [Note::RangeEndNotFound { num: "9999".to_owned() }]);
    }

    /// Jumeau : une liste « 1240 et 1241 » reste signalée par la note des autres citations, et la
    /// première est lue seule, de confiance haute.
    #[tokio::test]
    async fn a_list_is_not_a_range() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "1240")));
        let output = resolve(&corpus, "articles 1240 et 1241 du code civil").await;
        assert_eq!(output.candidates[0].confidence, Confidence::High);
        assert!(output.notes.contains(&Note::OtherCitations { count: 1 }), "{:?}", output.notes);
        assert_eq!(corpus.calls_of("find_article"), 1);
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

    fn corpus_with_cgi() -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse("LEGITEXT000006069577").unwrap(),
            title: "Code général des impôts".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
        corpus
    }

    /// Version de l'article 779 du CGI lue au 2011-06-01 : remplacée depuis (MODIFIE, fin
    /// 2011-06-12), comme la réponse réelle de journey_patrimoine-14.
    fn cgi_779(etat: LegalStatus, date_fin: Option<CivilDate>) -> ArticleText {
        ArticleText {
            etat: Some(etat),
            date_debut: CivilDate::new(2010, 5, 1),
            date_fin,
            code_title: Some("Code général des impôts, CGI.".to_owned()),
            ..article(22_201_685, "779")
        }
    }

    /// L2-12 (journey_patrimoine-14, resolve-5) : une version lue à une date et remplacée depuis le
    /// dit, en une note de 200 caractères au plus ; une version en vigueur, ou une date absente,
    /// ne dit rien de plus.
    #[tokio::test]
    async fn a_replaced_version_read_at_a_date_says_so_and_a_current_one_does_not() {
        let corpus = corpus_with_cgi();
        corpus.push_find_article(Ok(cgi_779(LegalStatus::Modifie, CivilDate::new(2011, 6, 12))));
        let output = run(
            &corpus,
            PisteEnvironment::Production,
            input("article 779 du CGI", Some("2011-06-01")),
        )
        .await
        .unwrap();
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.candidates[0].confidence, Confidence::High);
        assert_eq!(output.notes.len(), 1, "{:?}", output.notes);
        let text = output.notes[0].text();
        assert_eq!(
            text,
            "Version in force on 2011-06-01 (etat=MODIFIE, ended 2011-06-12); omit the date to \
             read the current text."
        );
        assert!(text.chars().count() <= 200);

        // Jumeau : la version lue à cette date est encore en vigueur, rien à signaler.
        let corpus = corpus_with_cgi();
        corpus.push_find_article(Ok(cgi_779(LegalStatus::Vigueur, None)));
        let output = run(
            &corpus,
            PisteEnvironment::Production,
            input("article 779 du CGI", Some("2011-06-01")),
        )
        .await
        .unwrap();
        assert_eq!(output.notes, []);

        // Sans date : la note de la version en vigueur, inchangée.
        let corpus = corpus_with_cgi();
        corpus.push_find_article(Ok(cgi_779(LegalStatus::Vigueur, None)));
        let output = run(&corpus, PisteEnvironment::Production, input("article 779 du CGI", None))
            .await
            .unwrap();
        assert_eq!(output.notes[0].text(), "Current version in force since 2010-05-01.");
    }

    /// D-03 (journey_patrimoine-14) : le candidat d'un article lu à une date porte la période de la
    /// version (valeurs de la réponse réelle, reconstituées : l'article 779 du CGI au 2011-06-01),
    /// aujourd'hui comme à une date passée ; une décision n'en porte pas.
    #[tokio::test]
    async fn the_candidate_of_an_article_carries_the_validity_of_its_version() {
        let corpus = corpus_with_cgi();
        corpus.push_find_article(Ok(cgi_779(LegalStatus::Modifie, CivilDate::new(2011, 6, 12))));
        let output = run(
            &corpus,
            PisteEnvironment::Production,
            input("article 779 du CGI", Some("2011-06-01")),
        )
        .await
        .unwrap();
        assert_eq!(
            output.candidates[0].record.validity,
            Some(Validity {
                etat: Some(LegalStatus::Modifie),
                date_debut: CivilDate::new(2010, 5, 1),
                date_fin: CivilDate::new(2011, 6, 12),
            })
        );

        // Aujourd'hui : en vigueur depuis le début de la version, sans fin.
        let corpus = corpus_with_cgi();
        corpus.push_find_article(Ok(cgi_779(LegalStatus::Vigueur, None)));
        let output = run(&corpus, PisteEnvironment::Production, input("article 779 du CGI", None))
            .await
            .unwrap();
        assert_eq!(
            output.candidates[0].record.validity,
            Some(Validity {
                etat: Some(LegalStatus::Vigueur),
                date_debut: CivilDate::new(2010, 5, 1),
                date_fin: None,
            })
        );
    }

    /// L2-17 (gap3-8) : un alinéa cité n'est pas contrôlé, et la réponse le dit ; sans alinéa, rien.
    #[tokio::test]
    async fn a_cited_subdivision_is_flagged_as_not_checked() {
        for citation in ["art. 9, al. 2 du code civil", "art. 9 alinéa 2 du code civil"] {
            let corpus = corpus_with_civil_code();
            corpus.push_find_article(Ok(article(1, "9")));
            let output = resolve(&corpus, citation).await;
            assert_eq!(output.candidates.len(), 1, "{citation}");
            assert_eq!(output.notes.len(), 1, "{citation}: {:?}", output.notes);
            assert_eq!(
                output.notes[0].text(),
                "Current version in force since 2016-10-01. The sub-division was not checked, \
                 only the article.",
                "{citation}"
            );
        }
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "9")));
        let output = resolve(&corpus, "art. 9 du code civil").await;
        assert_eq!(output.notes[0].text(), "Current version in force since 2016-10-01.");
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
            assert!(
                text.contains("an unknown or unattached abbreviation counts as missing"),
                "{text}"
            );
            assert!(text.ends_with("name the code next to the article, in full."), "{text}");
            // Aucun code en exemple : le sigle peut être connu mais non rattaché à l'article.
            assert!(!text.contains("code civil") && !text.contains("for example"), "{text}");
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

    /// U03 : un numéro refusé (« 757 b ») ne résout rien et la note dit qu'un article n'a pas été
    /// lu, au lieu d'accuser l'absence de code ou de citation ; aucun appel au corpus.
    #[tokio::test]
    async fn a_refused_lowercase_suffix_resolves_nothing_and_says_the_article_was_not_read_u03() {
        for citation in [
            "article 787 b du CGI",
            "art. 757 b du code général des impôts",
            "CGI, art. 757 b",
            "art 757 b code general des impots",
            "article 1240b du code civil",
            "art. 1240 b du code civil",
            "article L. 1234-5 b du code du travail",
        ] {
            let corpus = corpus_with_civil_code();
            let output = resolve(&corpus, citation).await;
            assert_eq!(output.interpreted_as, None, "{citation}");
            assert_eq!(output.candidates, [], "{citation}");
            assert_eq!(output.notes, [Note::ArticleNotRead], "{citation}");
            assert_eq!(corpus.calls(), 0, "{citation}");
        }
        // Jumeaux : sans article illisible, la note de l'absence de citation reste.
        let corpus = FakeCorpus::new();
        for citation in ["bonjour tout le monde", "1234-5 du code du travail"] {
            let output = resolve(&corpus, citation).await;
            assert_eq!(output.notes, [Note::NoCitationRecognised], "{citation}");
        }
        // Jumeau : la majuscule est lue et résolue.
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "787 B")));
        let output = resolve(&corpus, "art. 787 B du code civil").await;
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.interpreted_as.as_deref(), Some("Code civil, art. 787 B"));
    }

    /// U03, limite connue (CHANGELOG, ADR-0024 AM-85) : un numéro préfixé sans mot-clé
    /// (« L. 1234-5 b du code du travail ») est refusé comme les autres, mais reçoit la note
    /// générique « No citation recognised » et non celle de l'article non lu, car le filet de
    /// l'article non lu exige « article » ou « art. ». Figé tel quel ; aucun appel au corpus.
    #[tokio::test]
    async fn a_prefixed_number_without_keyword_and_a_lowercase_suffix_gets_the_generic_note_u03() {
        for citation in ["L. 1234-5 b du code du travail", "L1234-5 b du code du travail"] {
            let corpus = corpus_with_civil_code();
            let output = resolve(&corpus, citation).await;
            assert_eq!(output.interpreted_as, None, "{citation}");
            assert_eq!(output.candidates, [], "{citation}");
            assert_eq!(output.notes, [Note::NoCitationRecognised], "{citation}");
            assert_eq!(corpus.calls(), 0, "{citation}");
        }
    }

    /// U03 : une mention refusée placée avant une citation lue est signalée en tête des notes,
    /// même si l'autre citation est résolue en confiance haute.
    #[tokio::test]
    async fn a_refused_first_mention_is_flagged_even_when_a_later_citation_resolves_u03() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "1240")));
        let output = resolve(&corpus, "art. 757 b du CGI ; article 1240 du code civil").await;
        assert_eq!(output.interpreted_as.as_deref(), Some("Code civil, art. 1240"));
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.notes.first(), Some(&Note::ArticleNotRead), "{:?}", output.notes);
        // Jumeau : sans mention refusée, aucune note d'article illisible.
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1, "1240")));
        let output = resolve(&corpus, "article 1240 du code civil").await;
        assert!(!output.notes.contains(&Note::ArticleNotRead), "{:?}", output.notes);
    }

    /// U03 : un article refusé à côté d'un texte résolu ajoute la note de l'article illisible.
    #[tokio::test]
    async fn a_refused_article_next_to_a_resolved_text_adds_the_unread_note_u03() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(texts(1)));
        let output = resolve(&corpus, "art. 757 b du CGI ; loi n° 2016-1321").await;
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.notes.first(), Some(&Note::ArticleNotRead), "{:?}", output.notes);
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

    /// U12 : `fr_resolve` porte la même note de tête non lue que `fr_verify_citations`, et rien
    /// quand la tête est lue.
    #[tokio::test]
    async fn resolve_reports_an_unread_head_like_verify() {
        use lf_domain::{
            entity::{Decision, OfficialSummary},
            id::DecisionId,
        };
        let id = "JURITEXT000031539712";
        let corpus_for = || {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![
                SearchHit::new(Fond::Juri, DocumentId::parse(id).unwrap(), None).unwrap(),
            ]));
            corpus.push_get_decision(Ok(Decision {
                decision_id: DecisionId::parse(id).unwrap(),
                ecli: None,
                num: None,
                juridiction: None,
                formation: Some("CHAMBRE_SOCIALE".to_owned()),
                solution: None,
                numero_affaire: vec![],
                date_texte: CivilDate::new(2015, 11, 25),
                nature: Some("ARRET".to_owned()),
                title: None,
                text: LongText::from_plain("Texte de la décision.").unwrap(),
                official_summary: OfficialSummary::default(),
                cited_case_law: None,
                applied_texts: vec![],
            }));
            corpus
        };
        let unread = resolve(&corpus_for(), "Cass. soc., 25/11/15, n° 14-24.444").await;
        assert_eq!(unread.candidates.len(), 1);
        assert_eq!(unread.notes, [Note::HeadNotRead]);
        let right = resolve(&corpus_for(), "Cass. soc. 25-11-2015, n° 14-24.444").await;
        assert_eq!(right.candidates.len(), 1);
        assert_eq!(right.notes, []);
        let joined =
            resolve(&corpus_for(), "Cass. soc., 25 novembre 2015, n° 14-24.444 et 14-24.445").await;
        assert_eq!(joined.notes, [Note::OnlyFirstNumberChecked]);
    }

    #[tokio::test]
    async fn a_text_cited_without_a_number_points_to_fr_search() {
        for citation in
            ["loi du 1er juillet 1901", "article 34 de la Constitution du 4 octobre 1958"]
        {
            let corpus = FakeCorpus::new();
            let output = resolve(&corpus, citation).await;
            assert_eq!(output.candidates, [], "{citation}");
            assert_eq!(output.notes, [Note::NumberlessText], "{citation}");
            let text = output.notes[0].text();
            assert!(text.contains("fr_search (query: its title and date)"), "{text}");
            assert!(!text.contains("Not checkable in this version"), "{text}");
            assert_eq!(corpus.calls(), 0, "{citation}");
        }
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
            "Code civil has no article 9999 in force on 2026-10-29. The article may exist in an earlier or later version; retry with a date."
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

    fn caa_decision(juridiction: &str) -> lf_domain::entity::Decision {
        lf_domain::entity::Decision {
            decision_id: lf_domain::id::DecisionId::parse("CETATEXT000054955709").unwrap(),
            ecli: None,
            num: None,
            juridiction: Some(juridiction.to_owned()),
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: CivilDate::new(2026, 10, 2),
            nature: None,
            title: None,
            text: LongText::from_plain("Texte de la décision.").unwrap(),
            official_summary: lf_domain::entity::OfficialSummary::default(),
            cited_case_law: None,
            applied_texts: vec![],
        }
    }

    fn caa_hit() -> SearchHit {
        let mut found =
            SearchHit::new(Fond::Cetat, DocumentId::parse("CETATEXT000054955709").unwrap(), None)
                .unwrap();
        found.title = Some(
            "CAA de MARSEILLE, 5ème chambre, 02/10/2026, 25MA02308, Inédit au recueil Lebon"
                .to_owned(),
        );
        found
    }

    #[tokio::test]
    async fn a_caa_is_resolved_with_high_confidence_and_no_selection_note() {
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![caa_hit()]));
        corpus.push_get_decision(Ok(caa_decision("CAA de MARSEILLE")));
        let output = resolve(&corpus, "CAA Marseille, 2 octobre 2026, n° 25MA02308").await;
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.candidates[0].confidence, Confidence::High);
        assert_eq!(output.notes, []);
        // Un siège cité faux : un candidat `medium` et l'écart.
        let corpus = FakeCorpus::new();
        corpus.push_find_by_number(Ok(vec![caa_hit()]));
        corpus.push_get_decision(Ok(caa_decision("CAA de MARSEILLE")));
        let output = resolve(&corpus, "CAA Lyon, 2 octobre 2026, n° 25MA02308").await;
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

    /// U10 : l'article d'une loi qui figure au sommaire sans version en vigueur est un candidat de
    /// confiance moyenne, avec son identifiant et la note, aujourd'hui comme à une date passée.
    #[tokio::test]
    async fn resolve_returns_the_amending_article_as_a_medium_candidate() {
        let law = LegiTextId::parse("LEGITEXT000033202746").unwrap();
        let hit = || {
            let mut law_hit =
                SearchHit::new(Fond::Loda, DocumentId::LegiText(law.clone()), None).unwrap();
            law_hit.nature = Some("LOI".to_owned());
            law_hit.date = CivilDate::new(2016, 10, 9);
            law_hit.title = Some("LOI n° 2016-1321 du 7 octobre 2016".to_owned());
            law_hit
        };
        let act = || {
            let articles = vec![ArticleDraft {
                id: Some("LEGIARTI000000000001".to_owned()),
                num: Some("1".to_owned()),
                ..ArticleDraft::default()
            }];
            let toc = Toc::build(TocDraft { sections: vec![], articles }).unwrap();
            Act { text_id: law.clone(), body: TextBody { toc, ..TextBody::default() } }
        };
        let amending = ArticleText { etat: None, date_debut: None, ..article(1, "1") };
        for (date, day) in [(None, "2026-10-29"), (Some("2018-06-21"), "2018-06-21")] {
            let corpus = FakeCorpus::new();
            corpus.push_find_by_number(Ok(vec![hit()]));
            corpus.push_find_article(Err(not_found()));
            corpus.push_get_act(Ok(act()));
            corpus.push_get_article(Ok(amending.clone()));
            let citation = "article 1 de la loi n° 2016-1321 du 7 octobre 2016";
            let output =
                run(&corpus, PisteEnvironment::Production, input(citation, date)).await.unwrap();
            assert_eq!(output.candidates.len(), 1, "{date:?}");
            assert_eq!(output.candidates[0].confidence, Confidence::Medium, "{date:?}");
            assert_eq!(output.candidates[0].record.id.as_str(), "LEGIARTI000000000001");
            let notes: Vec<String> = output.notes.iter().map(Note::text).collect();
            assert_eq!(notes.len(), 1, "{notes:?}");
            assert!(
                notes[0].starts_with(&format!("No version in force on {day}: art. 1 of LOI")),
                "{notes:?}"
            );
        }
    }
}
