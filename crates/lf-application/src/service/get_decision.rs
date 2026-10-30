//! Méthode de façade `LegalService::get_decision` pour `fr_get_decision` (v0.1).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_GET_DECISION` : le décodage des arguments, la validation, l'appel au port et la
//! lecture forment l'exécution complète auditée.

use std::future::Future;

use super::LegalService;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetDecisionInput, ToolCall},
    output::get_decision::GetDecisionOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::get_decision::GetDecisionUseCase,
};

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_get_decision` : une décision de justice, son résumé, une fenêtre de son texte ou ses
    /// passages (05 §5.4). Métrique d'audit : les caractères renvoyés. `cancel` se résout quand le
    /// client annule.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, identifiant, paramètres de lecture), `not_found`, les erreurs
    /// communes du port, le dépassement de l'échéance globale, et `audit_error` en mode strict.
    pub async fn get_decision(
        &self,
        call: ToolCall<GetDecisionInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetDecisionOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_GET_DECISION, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = GetDecisionUseCase::new(&self.corpus);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::get_act::fixtures::service;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, GetDecisionInput, InputError, ReadingInput, ToolCall},
        ports::{AuditStatus, CorpusError},
        testing::FakeCorpus,
        tools::ToolName,
        usecase::get_decision::fixtures::{constit_text, decision},
    };
    use pretty_assertions::assert_eq;
    use std::future::{pending, ready};

    const CONSTIT: &str = "CONSTEXT000052555275";

    fn call(arguments: &str, id: Option<&str>) -> ToolCall<GetDecisionInput> {
        ToolCall {
            input: Ok(GetDecisionInput {
                decision_id: id.map(str::to_owned),
                reading: ReadingInput::default(),
            }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    #[tokio::test]
    async fn a_read_is_audited_with_the_returned_characters() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CONSTIT, &constit_text())));
        let (service, audit) = service(corpus);
        let arguments = format!(r#"{{"decision_id":"{CONSTIT}"}}"#);

        let output =
            service.get_decision(call(&arguments, Some(CONSTIT)), pending()).await.unwrap();

        let summary = output.reading.summary.as_ref().expect("the default summary").text.clone();
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool, ToolName::FR_GET_DECISION);
        assert_eq!(lines[0].status, AuditStatus::Ok);
        assert_eq!(lines[0].error_code, None);
        assert_eq!(
            lines[0].output_count_or_size,
            u64::try_from(summary.chars().count()).unwrap(),
            "la métrique est le nombre de caractères du résumé rendu"
        );
        assert_eq!(lines[0].input.as_str(), arguments);
    }

    #[tokio::test]
    async fn an_unknown_decision_is_audited_as_not_found() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Err(CorpusError::NotFound { what: "/consult/juri".to_owned() }));
        let (service, audit) = service(corpus);

        let error = service.get_decision(call("{}", Some(CONSTIT)), pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        let message = error.message();
        assert!(message.starts_with("No CONSTIT decision found for"), "{message}");
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].status, AuditStatus::Error);
        assert_eq!(lines[0].error_code, Some(ErrorCode::NotFound));
        assert_eq!(lines[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_network() {
        let corpus = FakeCorpus::new();
        let (service, audit) = service(corpus);
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };

        let error = service.get_decision(rejected, pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error.message(), "foo: unknown field `foo`");
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(lines[0].input.as_str(), r#"{"foo":1}"#);
    }

    #[tokio::test]
    async fn a_missing_id_is_audited_as_invalid_arg() {
        let (service, audit) = service(FakeCorpus::new());

        let error = service.get_decision(call("{}", None), pending()).await.unwrap_err();

        assert_eq!(error.message(), "decision_id is required.");
        assert_eq!(audit.records().len(), 1);
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(CONSTIT, &constit_text())));
        let (service, audit) = service(corpus);

        let error = service.get_decision(call("{}", Some(CONSTIT)), ready(())).await.unwrap_err();

        assert_eq!(error.code(), None);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].status, AuditStatus::Error);
        assert_eq!(lines[0].error_code, None);
    }
}
