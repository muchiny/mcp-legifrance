//! Méthode de façade `LegalService::search_service_public` pour `fr_search_service_public`
//! (jalon MS).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_SEARCH_SERVICE_PUBLIC` : le décodage des arguments, la validation, l'appel à la
//! source de Service-Public et la mise en forme de la page forment l'exécution complète auditée.

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{SearchServicePublicInput, ToolCall},
    output::search_service_public::SearchServicePublicOutput,
    ports::{AuditLog, Clock, LegalCorpus, PublicGuidance, TaxDoctrine},
    tools::ToolName,
    usecase::search_service_public::SearchServicePublicUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance>
    LegalService<C, A, K, D, G>
{
    /// `fr_search_service_public` : recherche dans les fiches de Service-Public (05 §5.16).
    /// Métrique d'audit : le nombre de résultats renvoyés.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, arguments), `config_error` (source coupée, sans aucune E/S),
    /// `rate_limited`, `upstream_error`, le dépassement de l'échéance globale, et `audit_error`
    /// en mode strict.
    pub async fn search_service_public(
        &self,
        call: ToolCall<SearchServicePublicInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<SearchServicePublicOutput, AppError> {
        let meta =
            CallMeta::new(ToolName::FR_SEARCH_SERVICE_PUBLIC, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = SearchServicePublicUseCase::new(&self.guidance);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use lf_domain::guidance::{Audience, AudienceFilter};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, InputError},
        messages,
        ports::{AuditStatus, GuidanceError},
        service::bofip_fixtures::{service_with_guidance, service_without_doctrine},
        testing::fixtures::service_public::{found, hit},
    };

    fn call(arguments: &str, query: Option<&str>) -> ToolCall<SearchServicePublicInput> {
        ToolCall {
            input: Ok(SearchServicePublicInput {
                query: query.map(str::to_owned),
                ..SearchServicePublicInput::default()
            }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    #[tokio::test]
    async fn a_search_is_audited_with_the_number_of_hits() {
        let (service, log) = service_with_guidance();
        let hits = vec![hit(Audience::Particuliers, "F1437", "Solde bancaire insaisissable")];
        service.guidance.push_search(Ok(found(AudienceFilter::Particuliers, 12, hits.clone())));
        let arguments = r#"{"query":"solde bancaire"}"#;
        let output =
            service.search_service_public(call(arguments, Some("solde bancaire")), pending()).await;
        let output = output.unwrap();
        assert_eq!((output.total, output.hits, output.has_more), (12, hits, true));
        let records = log.records();
        assert_eq!(records.len(), 1);
        let line = &records[0];
        assert_eq!(line.tool, ToolName::FR_SEARCH_SERVICE_PUBLIC);
        assert_eq!((line.status, line.output_count_or_size), (AuditStatus::Ok, 1));
        assert_eq!(line.input.as_str(), arguments);
        // Service-Public ne passe ni par le corpus de Légifrance, ni par la doctrine.
        assert_eq!((service.guidance.calls(), service.corpus.calls()), (1, 0));
    }

    #[tokio::test]
    async fn a_refused_argument_or_a_port_failure_is_audited_with_its_code() {
        let (service, log) = service_with_guidance();
        let error = service.search_service_public(call("{}", None), pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("query is required."));
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.search_service_public(rejected, pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("foo: unknown field `foo`"));
        service.guidance.push_search(Err(GuidanceError::RateLimited { retry_after: None }));
        let error =
            service.search_service_public(call("{}", Some("aide")), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::RateLimited));
        let codes: Vec<_> = log.records().iter().map(|line| line.error_code).collect();
        assert_eq!(
            codes,
            [
                Some(ErrorCode::InvalidArg),
                Some(ErrorCode::InvalidArg),
                Some(ErrorCode::RateLimited)
            ]
        );
        assert_eq!(service.guidance.calls(), 1);
    }

    /// Une façade assemblée par `LegalService::new` n'a pas de source de Service-Public : l'outil
    /// répond `config_error`, l'appel est audité, et rien n'est demandé au corpus.
    #[tokio::test]
    async fn without_a_guidance_source_the_tool_is_a_config_error() {
        let (service, log) = service_without_doctrine();
        let error =
            service.search_service_public(call("{}", Some("aide")), pending()).await.unwrap_err();
        assert_eq!(error, AppError::config(messages::GUIDANCE_DISABLED_MESSAGE));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_SEARCH_SERVICE_PUBLIC);
        assert_eq!(records[0].error_code, Some(ErrorCode::ConfigError));
        assert_eq!(service.corpus.calls(), 0);
    }
}
