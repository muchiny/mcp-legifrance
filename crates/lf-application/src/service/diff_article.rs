//! Méthode de façade `LegalService::diff_article` pour `fr_diff_article` (v0.3, M7, 05 §5.12).
//!
//! La méthode suit le motif gelé de `service/mod.rs` : l'identité de l'appel (outil, heure,
//! arguments canoniques) est posée avant tout, puis le décodage, la validation et le cas d'usage
//! s'exécutent ensemble sous [`Audited::run`](crate::audited::Audited::run). Un rejet local ou un
//! échec de décodage laisse donc, comme tout le reste, exactement une ligne d'audit (D-02).

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{DiffArticleInput, ToolCall},
    output::diff_article::DiffArticleOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::diff_article::DiffArticleUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_diff_article` : compare les versions d'un article en vigueur à deux dates (05 §5.12),
    /// auditée sous `fr_diff_article` (métrique : nombre de hunks).
    ///
    /// `cancel` se résout quand le client annule l'appel ; l'appel est alors abandonné et audité
    /// comme annulé.
    ///
    /// # Errors
    ///
    /// L'erreur du cas d'usage (`invalid_arg` pour un décodage ou une validation refusés,
    /// `not_found` quand aucune version n'est en vigueur à une date, erreurs du corpus
    /// traduites), `upstream_error` au-delà de l'échéance, `audit_error` en mode strict quand la
    /// ligne ne peut pas être écrite.
    pub async fn diff_article(
        &self,
        call: ToolCall<DiffArticleInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<DiffArticleOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_DIFF_ARTICLE, self.clock.now_utc(), call.canonical);
        let input = call.input;
        self.audited
            .run(meta, cancel, async {
                let input = input.map_err(AppError::from)?;
                DiffArticleUseCase::new(&self.corpus, &self.clock).execute(input).await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, future::ready, sync::Arc, time::Duration};

    use chrono::{TimeZone, Utc};
    use lf_domain::{
        date::CivilDate,
        entity::{ArticleText, ArticleVersion, ParentText},
        id::{ArticleId, DocumentId, LegiArtiId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        error::{ErrorCode, UpstreamCause},
        input::{CanonicalInput, InputError},
        ports::{AuditError, AuditRecord, AuditStatus, CorpusError, PisteEnvironment},
        service::{ConfigurationState, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };

    /// Journal partagé : le service possède le sien, le test garde une poignée.
    struct SharedLog(Arc<MemoryAuditLog>);

    impl AuditLog for SharedLog {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    fn arti(n: u32) -> LegiArtiId {
        LegiArtiId::parse(&format!("LEGIARTI{n:012}")).unwrap()
    }

    fn day(year: u16, month: u8, daynum: u8) -> CivilDate {
        CivilDate::new(year, month, daynum).unwrap()
    }

    /// Un service sur `corpus`, et le journal d'audit qu'il alimente.
    fn rig(
        corpus: FakeCorpus,
    ) -> (LegalService<FakeCorpus, SharedLog, FixedClock>, Arc<MemoryAuditLog>) {
        let audit = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "9.9.9",
            },
            configuration: ConfigurationState::default(),
            budget: ResponseBudget,
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap();
        let clock = FixedClock::new(now, day(2026, 10, 29));
        let service = LegalService::new(corpus, SharedLog(Arc::clone(&audit)), clock, settings);
        (service, audit)
    }

    fn version(n: u32, from: (u16, u8, u8), to: Option<(u16, u8, u8)>) -> ArticleVersion {
        ArticleVersion {
            id: arti(n),
            num: Some("9".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            date_debut: Some(day(from.0, from.1, from.2)),
            date_fin: to.map(|(y, m, d)| day(y, m, d)),
        }
    }

    fn article(n: u32, text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::Legi(arti(n)),
            num: Some("9".to_owned()),
            etat: None,
            code_title: Some("Code civil".to_owned()),
            parent: Some(ParentText::new(
                DocumentId::parse("LEGITEXT000006070721").unwrap(),
                Some("CODE"),
            )),
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    /// Un corpus à deux versions, dont les textes diffèrent par deux mots.
    fn corpus() -> FakeCorpus {
        let corpus = FakeCorpus::new();
        corpus.push_article_versions(Ok(vec![
            version(1, (2016, 10, 9), Some((2023, 9, 7))),
            version(2, (2023, 9, 8), None),
        ]));
        corpus.push_get_article(Ok(article(1, "est puni de un an")));
        corpus.push_get_article(Ok(article(2, "est puni de deux ans")));
        corpus
    }

    fn call(date_a: Option<&str>) -> ToolCall<DiffArticleInput> {
        let input = DiffArticleInput {
            article_id: Some(arti(2).as_str().to_owned()),
            date_a: date_a.map(str::to_owned),
            date_b: None,
        };
        ToolCall { input: Ok(input), canonical: CanonicalInput::new("{\"date_a\":\"2020-01-01\"}") }
    }

    #[tokio::test]
    async fn diff_article_is_audited_under_its_name_with_the_number_of_hunks() {
        let (service, audit) = rig(corpus());
        let output = service.diff_article(call(Some("2020-01-01")), pending()).await.unwrap();
        assert_eq!(output.hunks.len(), 1);

        let records = audit.records();
        assert_eq!(records.len(), 1, "une seule ligne par appel");
        let record = &records[0];
        assert_eq!(record.tool, ToolName::FR_DIFF_ARTICLE);
        assert_eq!(record.status, AuditStatus::Ok);
        assert_eq!(record.output_count_or_size, 1, "les hunks");
        assert_eq!(record.input.as_str(), "{\"date_a\":\"2020-01-01\"}");
        assert_eq!(record.ts, Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap());
        assert_eq!((record.error_code, record.error.as_deref()), (None, None));
        assert_eq!(record.server_version, "9.9.9");
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_calling_the_port() {
        let (service, audit) = rig(FakeCorpus::new());
        let failed = ToolCall::<DiffArticleInput> {
            input: Err(InputError { message: "date_a: invalid type: integer `7`".to_owned() }),
            canonical: CanonicalInput::new("{\"date_a\":7}"),
        };
        let error = service.diff_article(failed, pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error.message(), "date_a: invalid type: integer `7`");
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_local_rejection_and_a_missing_version_are_audited_with_their_codes() {
        let (service, audit) = rig(FakeCorpus::new());
        let error = service.diff_article(call(None), pending()).await.unwrap_err();
        assert_eq!(error.message(), "date_a is required.");

        let missing = FakeCorpus::new();
        missing.push_article_versions(Err(CorpusError::NotFound { what: "x".to_owned() }));
        let (service, audit_missing) = rig(missing);
        let error = service.diff_article(call(Some("2020-01-01")), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));

        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(audit_missing.records()[0].error_code, Some(ErrorCode::NotFound));
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let (service, audit) = rig(corpus());
        let error = service.diff_article(call(Some("2020-01-01")), ready(())).await.unwrap_err();
        assert_eq!(error.code(), None);
        assert_eq!(error.cause(), Some(UpstreamCause::Cancelled));
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, None);
    }
}
