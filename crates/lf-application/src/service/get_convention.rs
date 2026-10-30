//! Méthode de façade `LegalService::get_convention` pour `fr_get_convention` (v0.1).
//!
//! Le motif gelé de `service/mod.rs` : l'exécution complète (décodage, validation, appel au port)
//! tourne sous `Audited::run`, avec `ToolName::FR_GET_CONVENTION`.

use std::future::Future;

use super::LegalService;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetConventionInput, ToolCall},
    output::get_convention::GetConventionOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::get_convention::GetConventionUseCase,
};

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_get_convention` : une convention collective et une page de sa table des matières.
    /// `cancel` se résout quand le client annule.
    ///
    /// # Errors
    ///
    /// Une [`AppError`] par échec de l'appel, dont `invalid_arg` pour un décodage ou une
    /// validation refusés ; l'issue est auditée dans tous les cas.
    pub async fn get_convention(
        &self,
        call: ToolCall<GetConventionInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetConventionOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_GET_CONVENTION, self.clock.now_utc(), call.canonical);
        self.audited
            .run(meta, cancel, async {
                let input = call.input?;
                GetConventionUseCase::new(&self.corpus).execute(input).await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::get_act::fixtures::service;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, GetConventionInput, InputError, ToolCall},
        ports::AuditStatus,
        testing::FakeCorpus,
        tools::ToolName,
        usecase::get_act::fixtures::toc,
    };
    use lf_domain::{
        entity::{Convention, TextBody},
        id::KaliTextId,
    };
    use pretty_assertions::assert_eq;
    use std::future::pending;

    fn convention() -> Convention {
        Convention {
            text_id: KaliTextId::parse("KALITEXT000049878950").unwrap(),
            body: TextBody { toc: toc(1, 2, "Chapitre"), ..TextBody::default() },
            idcc: None,
            container_title: None,
            container_id: None,
            bulletin: None,
        }
    }

    #[tokio::test]
    async fn get_convention_writes_one_audit_line_with_the_toc_count() {
        let corpus = FakeCorpus::new();
        corpus.push_get_convention(Ok(convention()));
        let (service, audit) = service(corpus);
        let call = ToolCall {
            input: Ok(GetConventionInput {
                convention_id: Some("KALITEXT000049878950".to_owned()),
                ..GetConventionInput::default()
            }),
            canonical: CanonicalInput::new("{\"convention_id\":\"x\"}"),
        };

        let output = service.get_convention(call, pending()).await.unwrap();

        assert_eq!(output.toc.len(), 3);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool, ToolName::FR_GET_CONVENTION);
        assert_eq!(lines[0].status, AuditStatus::Ok);
        assert_eq!(lines[0].output_count_or_size, 3);
    }

    #[tokio::test]
    async fn get_convention_decoding_failure_is_audited_as_invalid_arg() {
        let (service, audit) = service(FakeCorpus::new());
        let rejected = ToolCall {
            input: Err(InputError {
                message: "date: unknown field `date`, expected one of `convention_id`, \
                          `toc_offset`, `toc_limit`"
                    .to_owned(),
            }),
            canonical: CanonicalInput::new("{\"date\":\"2020-01-01\"}"),
        };

        let error = service.get_convention(rejected, pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool, ToolName::FR_GET_CONVENTION);
        assert_eq!(lines[0].error_code, Some(ErrorCode::InvalidArg));
    }
}
