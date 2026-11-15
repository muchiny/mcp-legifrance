//! Méthode de façade `LegalService::get_bofip` pour `fr_get_bofip` (jalon MB).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_GET_BOFIP` : le décodage des arguments, la validation, l'appel à la source de
//! doctrine, le choix de la publication et la lecture de son texte forment l'exécution complète
//! auditée.

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetBofipInput, ToolCall},
    output::get_bofip::GetBofipOutput,
    ports::{AuditLog, Clock, LegalCorpus, TaxDoctrine},
    tools::ToolName,
    usecase::get_bofip::GetBofipUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine> LegalService<C, A, K, D> {
    /// `fr_get_bofip` : lecture d'une publication du BOFiP, en résumé, en fenêtre de texte ou en
    /// passages, ou liste des publications qui portent l'identifiant (05 §5.15). Métrique
    /// d'audit : les caractères renvoyés, ou le nombre de publications listées.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, identifiant, `publication_id`, paramètres de lecture, `offset`
    /// au-delà de la fin), `not_found`, `config_error` (source coupée, sans aucune E/S),
    /// `rate_limited`, `upstream_error`, le dépassement de l'échéance globale, et `audit_error` en
    /// mode strict.
    pub async fn get_bofip(
        &self,
        call: ToolCall<GetBofipInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetBofipOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_GET_BOFIP, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = GetBofipUseCase::new(&self.doctrine);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, InputError},
        messages,
        ports::{AuditStatus, DoctrineError, PisteEnvironment},
        service::bofip_fixtures::{service_with_doctrine, service_without_doctrine},
        testing::fixtures::bofip::{self, ID},
    };

    fn call(arguments: &str, id: Option<&str>) -> ToolCall<GetBofipInput> {
        ToolCall {
            input: Ok(GetBofipInput { id: id.map(str::to_owned), ..GetBofipInput::default() }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    #[tokio::test]
    async fn a_read_is_audited_with_the_returned_characters() {
        let (service, log) = service_with_doctrine();
        service.doctrine.push_get(Ok(vec![bofip::doc(3350, "Les donations sont taxées.")]));
        let arguments = format!(r#"{{"id":"{ID}"}}"#);
        let output = service.get_bofip(call(&arguments, Some(ID)), pending()).await.unwrap();
        let read = output.publication().unwrap();
        assert_eq!(read.reading.text.as_deref(), Some("Les donations sont taxées."));
        let records = log.records();
        assert_eq!(records.len(), 1);
        let line = &records[0];
        assert_eq!(line.tool, ToolName::FR_GET_BOFIP);
        assert_eq!(line.status, AuditStatus::Ok);
        assert_eq!(line.output_count_or_size, 26);
        assert_eq!(line.error_code, None);
        assert_eq!(line.input.as_str(), arguments);
        assert_eq!(line.ts.timestamp(), 1_790_000_000);
        assert_eq!(line.env, PisteEnvironment::Sandbox);
        // La doctrine ne passe pas par le corpus de Légifrance.
        assert_eq!((service.doctrine.calls(), service.corpus.calls()), (1, 0));
    }

    #[tokio::test]
    async fn a_list_of_publications_is_audited_with_their_number() {
        let (service, log) = service_with_doctrine();
        service.doctrine.push_get(Ok(vec![bofip::doc(3350, "Un."), bofip::doc(12_001, "Deux.")]));
        let output = service.get_bofip(call("{}", Some(ID)), pending()).await.unwrap();
        assert_eq!(output.publication(), None);
        assert_eq!(log.records()[0].output_count_or_size, 2);
    }

    #[tokio::test]
    async fn an_unknown_publication_is_audited_as_not_found() {
        let (service, log) = service_with_doctrine();
        service.doctrine.push_get(Err(DoctrineError::NotFound));
        let error = service.get_bofip(call("{}", Some(ID)), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, Some(ErrorCode::NotFound));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn a_refused_argument_is_audited_as_invalid_arg_without_network() {
        let (service, log) = service_with_doctrine();
        let error = service.get_bofip(call("{}", None), pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("id is required."));
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.get_bofip(rejected, pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("foo: unknown field `foo`"));
        let codes: Vec<_> = log.records().iter().map(|line| line.error_code).collect();
        assert_eq!(codes, [Some(ErrorCode::InvalidArg), Some(ErrorCode::InvalidArg)]);
        assert_eq!(service.doctrine.calls(), 0);
    }

    /// Une façade assemblée par `LegalService::new` n'a pas de source de doctrine : l'outil
    /// répond `config_error`, l'appel est audité, et rien n'est demandé au corpus.
    #[tokio::test]
    async fn without_a_doctrine_source_the_tool_is_a_config_error() {
        let (service, log) = service_without_doctrine();
        let error = service.get_bofip(call("{}", Some(ID)), pending()).await.unwrap_err();
        assert_eq!(error, AppError::config(messages::DOCTRINE_DISABLED_MESSAGE));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_BOFIP);
        assert_eq!(records[0].error_code, Some(ErrorCode::ConfigError));
        assert_eq!(service.corpus.calls(), 0);
    }
}
