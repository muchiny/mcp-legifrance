//! Méthode de façade `LegalService::get_deliberation` pour `fr_get_deliberation` (v0.1).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_GET_DELIBERATION` : le décodage des arguments, la validation, l'appel au port et
//! la lecture du texte forment l'exécution complète auditée (D-02).

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetDeliberationInput, ToolCall},
    output::get_deliberation::GetDeliberationOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::get_deliberation::GetDeliberationUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_get_deliberation` : lecture d'une délibération de la CNIL, en résumé, en fenêtre de texte
    /// ou en passages (05 §5.5). Métrique d'audit : les caractères renvoyés.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, identifiant, paramètres de lecture, `offset` au-delà de la fin),
    /// `not_found`, les erreurs communes du port, le dépassement de l'échéance globale, et
    /// `audit_error` en mode strict.
    pub async fn get_deliberation(
        &self,
        call: ToolCall<GetDeliberationInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetDeliberationOutput, AppError> {
        let meta =
            CallMeta::new(ToolName::FR_GET_DELIBERATION, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = GetDeliberationUseCase::new(&self.corpus);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc, time::Duration};

    use chrono::DateTime;
    use lf_domain::{
        date::CivilDate,
        entity::Deliberation,
        id::CnilTextId,
        text::{LongText, SummarySource},
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        audited::{AuditMode, AuditPolicy},
        budget::ResponseBudget,
        error::ErrorCode,
        input::{CanonicalInput, InputError, ReadingInput},
        ports::{AuditError, AuditRecord, AuditStatus, CorpusError, PisteEnvironment},
        service::{ConfigurationState, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };

    const ID: &str = "CNILTEXT000053352643";

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

    fn call(arguments: &str, id: Option<&str>) -> ToolCall<GetDeliberationInput> {
        ToolCall {
            input: Ok(GetDeliberationInput {
                deliberation_id: id.map(str::to_owned),
                reading: ReadingInput::default(),
            }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    fn deliberation(text: &str) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse(ID).unwrap(),
            num: Some("SAN-2026-002".to_owned()),
            nature: None,
            nature_delib: None,
            etat: None,
            date_texte: CivilDate::new(2026, 1, 8),
            date_publication: None,
            title: None,
            title_long: None,
            text: LongText::from_plain(text).unwrap(),
        }
    }

    #[tokio::test]
    async fn a_read_is_audited_with_the_returned_characters() {
        let (service, log) = service();
        service.corpus.push_get_deliberation(Ok(deliberation("La formation restreinte délibère.")));
        let arguments = format!(r#"{{"deliberation_id":"{ID}"}}"#);
        let output = service.get_deliberation(call(&arguments, Some(ID)), pending()).await.unwrap();
        assert_eq!(output.reading.text.as_deref(), Some("La formation restreinte délibère."));
        let records = log.records();
        assert_eq!(records.len(), 1);
        let line = &records[0];
        assert_eq!(line.tool, ToolName::FR_GET_DELIBERATION);
        assert_eq!(line.status, AuditStatus::Ok);
        assert_eq!(line.output_count_or_size, 33);
        assert_eq!(line.error_code, None);
        assert_eq!(line.input.as_str(), arguments);
        assert_eq!(line.ts.timestamp(), 1_790_000_000);
        assert_eq!(line.env, PisteEnvironment::Sandbox);
    }

    #[tokio::test]
    async fn a_long_deliberation_is_summarised_by_its_lead_and_audited_by_the_summary() {
        let (service, log) = service();
        let sentence = "Considérant que la société n’a pas respecté ses obligations. ";
        service.corpus.push_get_deliberation(Ok(deliberation(&sentence.repeat(60))));
        let output = service.get_deliberation(call("{}", Some(ID)), pending()).await.unwrap();
        let summary = output.reading.summary.expect("un texte long donne un résumé");
        assert_eq!(summary.source, SummarySource::Lead);
        let records = log.records();
        assert_eq!(
            records[0].output_count_or_size,
            u64::try_from(summary.text.chars().count()).unwrap()
        );
    }

    #[tokio::test]
    async fn an_unknown_deliberation_is_audited_as_not_found() {
        let (service, log) = service();
        service
            .corpus
            .push_get_deliberation(Err(CorpusError::NotFound { what: "cnil".to_owned() }));
        let error = service.get_deliberation(call("{}", Some(ID)), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn an_offset_past_the_end_is_audited_as_invalid_arg() {
        let (service, log) = service();
        service.corpus.push_get_deliberation(Ok(deliberation("Texte court.")));
        let rejected = ToolCall {
            input: Ok(GetDeliberationInput {
                deliberation_id: Some(ID.to_owned()),
                reading: ReadingInput { offset: Some(50), ..ReadingInput::default() },
            }),
            canonical: CanonicalInput::new(r#"{"offset":50}"#),
        };
        let error = service.get_deliberation(rejected, pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_network() {
        let (service, log) = service();
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.get_deliberation(rejected, pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("foo: unknown field `foo`"));
        assert_eq!(service.corpus.calls(), 0);
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].input.as_str(), r#"{"foo":1}"#);
    }

    #[tokio::test]
    async fn a_missing_id_is_audited_as_invalid_arg_without_network() {
        let (service, log) = service();
        let error = service.get_deliberation(call("{}", None), pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("deliberation_id is required."));
        assert_eq!(service.corpus.calls(), 0);
        assert_eq!(log.records().len(), 1);
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let (service, log) = service();
        let error = service
            .get_deliberation(call("{}", Some(ID)), std::future::ready(()))
            .await
            .unwrap_err();
        assert_eq!(error.code(), None);
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, None);
    }
}
