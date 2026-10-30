//! Méthode de façade `LegalService::get_text` pour `fr_get_text` (v0.1).
//!
//! Le motif gelé de `service/mod.rs` : l'exécution complète (décodage, validation, appel au port)
//! tourne sous `Audited::run`, avec `ToolName::FR_GET_TEXT`.

use std::future::Future;

use super::LegalService;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetTextInput, ToolCall},
    output::get_text::GetTextOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::get_text::GetTextUseCase,
};

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_get_text` : un article en résumé, en fenêtre de texte ou en passages. `cancel` se
    /// résout quand le client annule.
    ///
    /// # Errors
    ///
    /// Une [`AppError`] par échec de l'appel, dont `invalid_arg` pour un décodage ou une
    /// validation refusés ; l'issue est auditée dans tous les cas.
    pub async fn get_text(
        &self,
        call: ToolCall<GetTextInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetTextOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_GET_TEXT, self.clock.now_utc(), call.canonical);
        self.audited
            .run(meta, cancel, async {
                let input = call.input?;
                GetTextUseCase::new(&self.corpus).execute(input).await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc, time::Duration};

    use chrono::DateTime;
    use lf_domain::{date::CivilDate, entity::ArticleText, id::ArticleId, text::LongText};
    use pretty_assertions::assert_eq;

    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        error::ErrorCode,
        input::{CanonicalInput, GetTextInput, InputError, ReadingInput, ToolCall},
        ports::{AuditError, AuditLog, AuditRecord, AuditStatus, CorpusError, PisteEnvironment},
        service::{ConfigurationState, LegalService, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
        tools::ToolName,
    };

    const ID: &str = "LEGIARTI000006419288";
    const TEXT: &str = "Chacun a droit au respect de sa vie privée.";

    /// Journal partagé : le service possède le sien, le test en garde une poignée.
    struct SharedAudit(Arc<MemoryAuditLog>);

    impl AuditLog for SharedAudit {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            self.0.record(record).await
        }
    }

    fn service() -> (LegalService<FakeCorpus, SharedAudit, FixedClock>, Arc<MemoryAuditLog>) {
        let log = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env: PisteEnvironment::Sandbox,
                server_version: "0.0.0",
            },
            configuration: ConfigurationState::default(),
            budget: ResponseBudget,
        };
        let clock = FixedClock::new(
            DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
            CivilDate::new(2026, 10, 29).unwrap(),
        );
        let service =
            LegalService::new(FakeCorpus::new(), SharedAudit(Arc::clone(&log)), clock, settings);
        (service, log)
    }

    fn call(arguments: &str, input: GetTextInput) -> ToolCall<GetTextInput> {
        ToolCall { input: Ok(input), canonical: CanonicalInput::new(arguments) }
    }

    fn article(text: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ID).unwrap(),
            num: Some("9".to_owned()),
            etat: None,
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: Vec::new(),
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain(text).unwrap(),
            nota: None,
        }
    }

    fn wanted() -> GetTextInput {
        GetTextInput { article_id: Some(ID.to_owned()), reading: ReadingInput::default() }
    }

    #[tokio::test]
    async fn get_text_writes_one_audit_line_with_the_characters_returned() {
        let (service, audit) = service();
        service.corpus.push_get_article(Ok(article(TEXT)));

        let output =
            service.get_text(call("{\"article_id\":\"x\"}", wanted()), pending()).await.unwrap();

        assert_eq!(output.reading.text.as_deref(), Some(TEXT));
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool, ToolName::FR_GET_TEXT);
        assert_eq!(lines[0].status, AuditStatus::Ok);
        assert_eq!(lines[0].output_count_or_size, u64::try_from(TEXT.chars().count()).unwrap());
        assert_eq!(lines[0].input.as_str(), "{\"article_id\":\"x\"}");
    }

    #[tokio::test]
    async fn get_text_decoding_failure_is_audited_as_invalid_arg_without_network() {
        let (service, audit) = service();
        let rejected = ToolCall {
            input: Err(InputError {
                message: "offset: invalid type: string \"x\", expected an integer".to_owned(),
            }),
            canonical: CanonicalInput::new("{\"offset\":\"x\"}"),
        };

        let error = service.get_text(rejected, pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error.message(), "offset: invalid type: string \"x\", expected an integer");
        assert_eq!(service.corpus.calls(), 0);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].status, AuditStatus::Error);
        assert_eq!(lines[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(lines[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn get_text_not_found_is_audited_with_the_tool_message() {
        let (service, audit) = service();
        service.corpus.push_get_article(Err(CorpusError::NotFound { what: "x".to_owned() }));

        let error = service.get_text(call("{}", wanted()), pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::NotFound));
        assert_eq!(audit.records()[0].output_count_or_size, 0);
    }
}
