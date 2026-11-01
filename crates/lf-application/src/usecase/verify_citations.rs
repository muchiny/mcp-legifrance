//! Cas d'usage de `fr_verify_citations` (v0.2 (M6)) : le contrôle des citations d'un texte
//! (`VerifyCitationsUseCase`, 05 §5.9, annexe B §7.7).
//!
//! # Étapes
//!
//! 1. Validation sans réseau : `text` de 1 à 20 000 caractères, `max_citations` de 1 à 100 (50 par
//!    défaut).
//! 2. Le contrôle lui-même est `verify_text` : analyse, tri hors ligne, dédoublonnage, résolution
//!    quatre à la fois sous le budget de 80 % du délai de l'outil, extrait cité.
//! 3. Plafond de réponse (05 §6.4) : le cas d'usage ne coupe pas par estimation (L2-13, gap1-5 :
//!    64 caractères par champ surestimaient d'environ 4 fois et coupaient à 69 ou 85 citations).
//!    `max_citations` est la seule borne ici ; la mesure exacte, sur le JSON, est faite par la vue
//!    de `lf-adapter-mcp`, qui raccourcit par la fin sous 45 000 caractères et le dit (`truncated`,
//!    `note`).
//!
//! Si toutes les citations soumises au corpus échouent, l'appel échoue avec l'erreur amont.

use std::time::Duration;

use lf_domain::date::VersionDate;

use crate::{
    budget::{ResponseBudget, bounded},
    citation::{
        Note,
        resolve::Proof,
        verify::{Run, verify_text},
    },
    deadline::Deadline,
    error::AppError,
    failure::port_failure,
    input::VerifyCitationsInput,
    messages,
    output::verify_citations::VerifyCitationsOutput,
    ports::{Clock, LegalCorpus},
    tools::ToolName,
};

const TOOL: ToolName = ToolName::FR_VERIFY_CITATIONS;

/// Longueur maximale de `text`, en caractères (05 §5.9).
pub(crate) const TEXT_MAX_CHARS: usize = 20_000;

/// Borne basse de `max_citations`.
const MAX_CITATIONS_MIN: usize = 1;

/// Cas d'usage `fr_verify_citations`.
pub(crate) struct VerifyCitationsUseCase<'a, C: LegalCorpus, K: Clock> {
    corpus: &'a C,
    clock: &'a K,
    proof: Proof,
    deadline: Duration,
}

impl<'a, C: LegalCorpus, K: Clock> VerifyCitationsUseCase<'a, C, K> {
    /// `proof` est ce que prouve l'absence dans la source interrogée (production, bac à sable ou
    /// instantané local), `deadline` l'échéance de l'outil, repli quand aucune échéance n'est
    /// publiée par [`Audited`](crate::audited::Audited).
    pub(crate) const fn new(corpus: &'a C, clock: &'a K, proof: Proof, deadline: Duration) -> Self {
        Self { corpus, clock, proof, deadline }
    }

    /// Contrôle les citations de `input.text`.
    ///
    /// # Errors
    ///
    /// `invalid_arg` pour une entrée refusée ; l'erreur amont quand toutes les citations soumises
    /// au corpus échouent.
    pub(crate) async fn execute(
        &self,
        input: VerifyCitationsInput,
    ) -> Result<VerifyCitationsOutput, AppError> {
        let VerifyCitationsInput { text, max_citations } = input;
        let text = text
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| messages::required("text"))?;
        let length = text.chars().count();
        if length > TEXT_MAX_CHARS {
            return Err(messages::invalid_argument(format!(
                "text is too long ({length} characters, max {TEXT_MAX_CHARS}): split it into \
                 several calls."
            )));
        }
        let max_citations = bounded(
            "max_citations",
            max_citations,
            ResponseBudget::MAX_CITATIONS_DEFAULT,
            MAX_CITATIONS_MIN,
            ResponseBudget::MAX_CITATIONS_MAX,
        )?;
        let today = VersionDate::try_from(self.clock.today_paris())
            .map_err(|_| AppError::config("the clock is outside the supported date range."))?;
        let run = Run {
            proof: self.proof,
            today,
            remaining: Deadline::current_or(self.deadline),
            max_citations,
        };
        let verified = verify_text(self.corpus, run, &text)
            .await
            .map_err(|error| port_failure(TOOL, "the citations", &error))?;
        let found = verified.found;
        let citations = verified.checks;
        let note = (found == 0).then(|| Note::NoCitationRecognised.text());
        Ok(VerifyCitationsOutput {
            found,
            checked: citations.len(),
            truncated: found > citations.len(),
            citations,
            note,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audited::AuditMetric,
        citation::{CheckStatus, Note},
        error::ErrorCode,
        ports::{CorpusError, PisteEnvironment},
        testing::{FakeCorpus, FixedClock},
    };
    use chrono::{TimeZone, Utc};
    use lf_domain::{
        citation::Span,
        date::CivilDate,
        entity::{ArticleText, CodeListing, ParentText},
        id::{ArticleId, DocumentId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    fn clock() -> FixedClock {
        FixedClock::new(
            Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap(),
            CivilDate::new(2026, 10, 29).unwrap(),
        )
    }

    fn corpus_with_civil_code() -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
        corpus
    }

    fn article(n: u32) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(&format!("LEGIARTI{n:012}")).unwrap(),
            num: None,
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: CivilDate::new(2016, 10, 1),
            date_fin: None,
            text: LongText::from_plain("Texte.").unwrap(),
            nota: None,
        }
    }

    async fn run(
        corpus: &FakeCorpus,
        env: PisteEnvironment,
        text: Option<&str>,
        max_citations: Option<i64>,
    ) -> Result<VerifyCitationsOutput, AppError> {
        let clock = clock();
        let input = VerifyCitationsInput { text: text.map(str::to_owned), max_citations };
        VerifyCitationsUseCase::new(corpus, &clock, Proof::of(env, false), Duration::from_secs(60))
            .execute(input)
            .await
    }

    #[tokio::test]
    async fn the_resolved_citation_of_a_code_article_drops_the_sigle_of_its_title() {
        // L2-15 (verify_citations-16): « art. 777 CGI » donnait « Code général des impôts, CGI.,
        // art. 777 » ; la citation rendue n'a plus le sigle, celle d'un code sans sigle reste.
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse("LEGITEXT000006069577").unwrap(),
            title: "Code général des impôts".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
        let mut cgi = article(1);
        cgi.num = Some("777".to_owned());
        cgi.code_title = Some("Code général des impôts, CGI.".to_owned());
        cgi.parent = Some(ParentText::new(
            DocumentId::LegiText(LegiTextId::parse("LEGITEXT000006069577").unwrap()),
            Some("CODE"),
        ));
        corpus.push_find_article(Ok(cgi));
        let output =
            run(&corpus, PisteEnvironment::Production, Some("art. 777 CGI"), None).await.unwrap();
        let resolved = output.citations[0].resolved.as_ref().unwrap();
        assert_eq!(resolved.citation, "Code général des impôts, art. 777");
    }

    #[tokio::test]
    async fn text_and_max_citations_are_validated_before_any_call() {
        let corpus = FakeCorpus::new();
        let env = PisteEnvironment::Production;
        let missing = run(&corpus, env, None, None).await.unwrap_err();
        assert_eq!(missing.message(), "text is required.");
        let empty = run(&corpus, env, Some(""), None).await.unwrap_err();
        assert_eq!(empty.message(), "text is required.");
        // Le blanc seul vaut le vide (errors-14, verify_citations-16) : espace insécable, tabulation,
        // saut de ligne.
        for blank in ["\u{a0}\t", "   \n "] {
            let error = run(&corpus, env, Some(blank), None).await.unwrap_err();
            assert_eq!(error.message(), "text is required.", "{blank:?}");
            assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        }
        let long = "a".repeat(TEXT_MAX_CHARS + 1);
        let too_long = run(&corpus, env, Some(&long), None).await.unwrap_err();
        assert_eq!(
            too_long.message(),
            "text is too long (20001 characters, max 20000): split it into several calls."
        );
        assert_eq!(too_long.code(), Some(ErrorCode::InvalidArg));
        for bad in [0, 101, -1] {
            let error =
                run(&corpus, env, Some("article 9 du code civil"), Some(bad)).await.unwrap_err();
            assert_eq!(error.message(), format!("max_citations={bad} out of range (1..100)."));
        }
        assert_eq!(corpus.calls(), 0);
        // Les bornes sont admises : 20 000 caractères, 1 et 100 citations.
        let limit = "a".repeat(TEXT_MAX_CHARS);
        assert!(run(&corpus, env, Some(&limit), Some(1)).await.is_ok());
        assert!(run(&corpus, env, Some(&limit), Some(100)).await.is_ok());
    }

    #[tokio::test]
    async fn a_text_without_citation_says_so_and_calls_nothing() {
        let corpus = corpus_with_civil_code();
        let output =
            run(&corpus, PisteEnvironment::Production, Some("Bonjour, ceci ne cite rien."), None)
                .await
                .unwrap();
        assert_eq!((output.found, output.checked, output.truncated), (0, 0, false));
        assert_eq!(output.citations, []);
        let note = output.note.expect("no citation recognised carries a note");
        assert_eq!(note, Note::NoCitationRecognised.text());
        assert_eq!(
            note,
            "No citation recognised. Only citations introduced by 'article' or 'art.' are read; \
             an empty result does not prove the text cites nothing."
        );
        assert!(note.chars().count() <= 200);
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_text_with_a_citation_carries_no_such_note() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Ok(article(1)));
        let output = run(
            &corpus,
            PisteEnvironment::Production,
            Some("Voir l'article 1 du code civil."),
            None,
        )
        .await
        .unwrap();
        assert_eq!((output.found, output.checked), (1, 1));
        assert_eq!(output.note, None);
    }

    #[tokio::test]
    async fn statuses_and_spans_in_code_points() {
        let corpus = corpus_with_civil_code();
        corpus.push_find_article(Err(CorpusError::NotFound { what: "x".to_owned() }));
        // Trois caractères hors du latin de base (2, 3 et 4 octets) avant la citation : les offsets
        // en octets et en points de code diffèrent.
        let text = "é€😀 Voir l'article 9999 du code civil.";
        let output = run(&corpus, PisteEnvironment::Production, Some(text), None).await.unwrap();
        assert_eq!((output.found, output.checked, output.truncated), (1, 1, false));
        let check = &output.citations[0];
        assert_eq!(check.status, CheckStatus::NotFound);
        let byte_start = text.find(&check.raw).unwrap();
        let start = text[..byte_start].chars().count();
        assert_ne!(start, byte_start);
        assert_eq!(check.span, Span { start, end: start + check.raw.chars().count() });
        assert_eq!(
            text.chars()
                .skip(check.span.start)
                .take(check.span.end - check.span.start)
                .collect::<String>(),
            check.raw
        );
        assert_eq!(output.output_count_or_size(), 1);
    }

    #[tokio::test]
    async fn a_clock_outside_the_supported_range_is_a_config_error() {
        let corpus = corpus_with_civil_code();
        let clock = FixedClock::new(
            Utc.with_ymd_and_hms(1400, 1, 1, 8, 30, 0).unwrap(),
            CivilDate::new(1400, 1, 1).unwrap(),
        );
        let input = VerifyCitationsInput {
            text: Some("article 9 du code civil".to_owned()),
            max_citations: None,
        };
        let proof = Proof::of(PisteEnvironment::Production, false);
        let error = VerifyCitationsUseCase::new(&corpus, &clock, proof, Duration::from_secs(60))
            .execute(input)
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::ConfigError));
        assert_eq!(error.message(), "the clock is outside the supported date range.");
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn all_upstream_failures_fail_the_call_with_upstream_error() {
        let corpus = corpus_with_civil_code();
        let failure =
            CorpusError::Upstream { status: Some(503), retryable: true, detail: "down".to_owned() };
        corpus.push_find_article(Err(failure.clone()));
        corpus.push_find_article(Err(failure));
        let text = "article 9 du code civil. Voir aussi l'article 10 du code civil.";
        let error = run(&corpus, PisteEnvironment::Production, Some(text), None).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
    }

    #[tokio::test]
    async fn max_citations_100_is_reached_with_quotes_gap1_5() {
        // Cas réel de gap1-5 : cent citations de code, dont trente suivies d'un extrait collé. L'ancienne
        // estimation (64 caractères par champ) coupait à 69 ; la mesure exacte est celle de la vue.
        let corpus = corpus_with_civil_code();
        for n in 1..=100 {
            let mut quoted = article(n);
            quoted.text =
                LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap();
            corpus.push_find_article(Ok(quoted));
        }
        let text = (1..=100)
            .map(|n| {
                if n <= 30 {
                    format!(
                        "article {n} du code civil « Chacun a droit au respect de sa vie privée »"
                    )
                } else {
                    format!("article {n} du code civil")
                }
            })
            .collect::<Vec<_>>()
            .join(" ; ");
        let output =
            run(&corpus, PisteEnvironment::Production, Some(&text), Some(100)).await.unwrap();
        assert_eq!((output.found, output.checked, output.truncated), (100, 100, false));
        assert_eq!(output.citations.len(), 100);
        assert_eq!(output.note, None);
        assert!(output.citations[0].quote.is_some());
        // Les premières citations sont gardées, dans l'ordre.
        assert!(output.citations[0].raw.contains("article 1 "));
        assert!(output.citations[99].raw.contains("article 100 "));
    }

    #[tokio::test]
    async fn the_application_does_not_cut_by_estimate_only_max_citations_bounds_the_list() {
        // Des enregistrements très lourds : la coupe sur la taille mesurée est celle de la vue, le
        // cas d'usage rend toutes les citations demandées, sans note de raccourcissement.
        let corpus = corpus_with_civil_code();
        for n in 1..=100 {
            let mut heavy = article(n);
            heavy.code_title = Some("Code civil ".repeat(100));
            corpus.push_find_article(Ok(heavy));
        }
        let text =
            (1..=100).map(|n| format!("article {n} du code civil")).collect::<Vec<_>>().join(" ; ");
        let output =
            run(&corpus, PisteEnvironment::Production, Some(&text), Some(100)).await.unwrap();
        assert_eq!((output.found, output.checked, output.truncated), (100, 100, false));
        assert_eq!(output.note, None);
        // `max_citations` borne toujours : 100 trouvées, 40 demandées, 40 rendues, dans l'ordre.
        let corpus = corpus_with_civil_code();
        for n in 1..=100 {
            corpus.push_find_article(Ok(article(n)));
        }
        let output =
            run(&corpus, PisteEnvironment::Production, Some(&text), Some(40)).await.unwrap();
        assert_eq!((output.found, output.checked, output.truncated), (100, 40, true));
        assert!(output.citations[39].raw.contains("article 40 "));
        // Sous le plafond, la liste n'est ni raccourcie ni annotée (défaut de 50 inchangé).
        let corpus = corpus_with_civil_code();
        for n in 1..=3 {
            corpus.push_find_article(Ok(article(n)));
        }
        let small = "article 1 du code civil ; article 2 du code civil ; article 3 du code civil";
        let output = run(&corpus, PisteEnvironment::Production, Some(small), None).await.unwrap();
        assert_eq!((output.found, output.checked, output.truncated), (3, 3, false));
        assert_eq!(output.note, None);
    }
}
