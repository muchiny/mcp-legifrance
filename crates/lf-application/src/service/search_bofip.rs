//! Méthode de façade `LegalService::search_bofip` pour `fr_search_bofip` (jalon MB).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_SEARCH_BOFIP` : le décodage des arguments, la validation, l'appel à la source de
//! doctrine et la mise en forme de la page forment l'exécution complète auditée.

use std::future::Future;

use crate::ports::JudicialOpenData;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{SearchBofipInput, ToolCall},
    output::search_bofip::SearchBofipOutput,
    ports::{AuditLog, Clock, LegalCorpus, PublicGuidance, TaxDoctrine},
    tools::ToolName,
    usecase::search_bofip::SearchBofipUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance, J: JudicialOpenData>
    LegalService<C, A, K, D, G, J>
{
    /// `fr_search_bofip` : recherche dans le BOFiP, la doctrine publiée de l'administration
    /// fiscale (05 §5.14). Métrique d'audit : le nombre de résultats renvoyés.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, arguments, page hors de la fenêtre atteignable, réponse qui ne
    /// tient pas sous le plafond), `config_error` (source coupée, sans aucune E/S),
    /// `rate_limited`, `upstream_error`, le dépassement de l'échéance globale, et `audit_error` en
    /// mode strict.
    pub async fn search_bofip(
        &self,
        call: ToolCall<SearchBofipInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<SearchBofipOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_SEARCH_BOFIP, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = SearchBofipUseCase::new(&self.doctrine);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use lf_domain::doctrine::DoctrinePage;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, InputError},
        messages,
        ports::{AuditStatus, DoctrineError, PisteEnvironment},
        service::bofip_fixtures::{service_with_doctrine, service_without_doctrine},
        testing::fixtures::bofip,
    };

    fn call(arguments: &str, query: Option<&str>) -> ToolCall<SearchBofipInput> {
        ToolCall {
            input: Ok(SearchBofipInput {
                query: query.map(str::to_owned),
                ..SearchBofipInput::default()
            }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    #[tokio::test]
    async fn a_search_is_audited_with_the_number_of_hits() {
        let (service, log) = service_with_doctrine();
        let hits = vec![bofip::hit(3350, "Donations"), bofip::hit(3351, "Successions")];
        service.doctrine.push_search(Ok(DoctrinePage { total: 357, hits: hits.clone() }));
        let arguments = r#"{"query":"donation"}"#;
        let output = service.search_bofip(call(arguments, Some("donation")), pending()).await;
        let output = output.unwrap();
        assert_eq!((output.total, output.hits, output.has_more), (357, hits, true));
        let records = log.records();
        assert_eq!(records.len(), 1);
        let line = &records[0];
        assert_eq!(line.tool, ToolName::FR_SEARCH_BOFIP);
        assert_eq!(line.status, AuditStatus::Ok);
        assert_eq!(line.output_count_or_size, 2);
        assert_eq!(line.error_code, None);
        assert_eq!(line.input.as_str(), arguments);
        assert_eq!(line.ts.timestamp(), 1_790_000_000);
        assert_eq!(line.env, PisteEnvironment::Sandbox);
        // La doctrine ne passe pas par le corpus de Légifrance.
        assert_eq!((service.doctrine.calls(), service.corpus.calls()), (1, 0));
    }

    #[tokio::test]
    async fn a_refused_argument_is_audited_as_invalid_arg_without_network() {
        let (service, log) = service_with_doctrine();
        let error = service.search_bofip(call("{}", None), pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("query is required."));
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.search_bofip(rejected, pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("foo: unknown field `foo`"));
        let codes: Vec<_> = log.records().iter().map(|line| line.error_code).collect();
        assert_eq!(codes, [Some(ErrorCode::InvalidArg), Some(ErrorCode::InvalidArg)]);
        assert_eq!(service.doctrine.calls(), 0);
    }

    #[tokio::test]
    async fn a_port_failure_is_audited_with_its_code() {
        let (service, log) = service_with_doctrine();
        service.doctrine.push_search(Err(DoctrineError::RateLimited { retry_after: None }));
        let error =
            service.search_bofip(call("{}", Some("donation")), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::RateLimited));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
        assert_eq!(records[0].error_code, Some(ErrorCode::RateLimited));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    /// Une façade assemblée par `LegalService::new` n'a pas de source de doctrine : l'outil
    /// répond `config_error`, l'appel est audité, et rien n'est demandé au corpus.
    #[tokio::test]
    async fn without_a_doctrine_source_the_tool_is_a_config_error() {
        let (service, log) = service_without_doctrine();
        let error =
            service.search_bofip(call("{}", Some("donation")), pending()).await.unwrap_err();
        assert_eq!(error, AppError::config(messages::DOCTRINE_DISABLED_MESSAGE));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_SEARCH_BOFIP);
        assert_eq!(records[0].error_code, Some(ErrorCode::ConfigError));
        assert_eq!(service.corpus.calls(), 0);
    }
}
