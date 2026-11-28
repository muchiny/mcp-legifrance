//! Méthode de façade `LegalService::search_judilibre` pour `fr_search_judilibre` (jalon MJ).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_SEARCH_JUDILIBRE` : le décodage des arguments, la validation, l'appel à la
//! source Judilibre et la mise en forme de la page forment l'exécution complète auditée.

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{SearchJudilibreInput, ToolCall},
    output::search_judilibre::SearchJudilibreOutput,
    ports::{AuditLog, Clock, JudicialOpenData, LegalCorpus, PublicGuidance, TaxDoctrine},
    tools::ToolName,
    usecase::search_judilibre::SearchJudilibreUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance, J: JudicialOpenData>
    LegalService<C, A, K, D, G, J>
{
    /// `fr_search_judilibre` : recherche dans Judilibre, la jurisprudence judiciaire en open data
    /// de la Cour de cassation (05 §5.18). Métrique d'audit : le nombre de résultats renvoyés.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, arguments, page hors de la fenêtre atteignable, réponse qui ne
    /// tient pas sous le plafond), `config_error` (source coupée, sans aucune E/S ; identifiants
    /// absents ; application non abonnée), `rate_limited`, `upstream_error`, le dépassement de
    /// l'échéance globale, et `audit_error` en mode strict.
    pub async fn search_judilibre(
        &self,
        call: ToolCall<SearchJudilibreInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<SearchJudilibreOutput, AppError> {
        let meta =
            CallMeta::new(ToolName::FR_SEARCH_JUDILIBRE, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = SearchJudilibreUseCase::new(&self.case_law, &self.clock);
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
        input::CanonicalInput,
        messages,
        ports::{AuditStatus, CaseLawError},
        service::bofip_fixtures::{service_with_case_law, service_without_doctrine},
        testing::fixtures::judilibre,
    };

    fn call(arguments: &str, query: Option<&str>) -> ToolCall<SearchJudilibreInput> {
        ToolCall {
            input: Ok(SearchJudilibreInput {
                query: query.map(str::to_owned),
                ..SearchJudilibreInput::default()
            }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    #[tokio::test]
    async fn a_search_is_audited_with_the_number_of_hits() {
        let (service, log) = service_with_case_law();
        let hits = vec![judilibre::hit(judilibre::CC_ID), judilibre::hit(judilibre::CA_ID)];
        service.case_law.push_search(Ok(judilibre::page(1_234, hits.clone())));
        let arguments = r#"{"query":"donation"}"#;
        let output = service.search_judilibre(call(arguments, Some("donation")), pending()).await;
        let output = output.unwrap();
        assert_eq!((output.total, output.hits, output.has_more), (1_234, hits, true));
        // Le jour de la consultation est celui de l'horloge, Europe/Paris.
        assert_eq!(output.consulted_on.to_string(), "2026-10-29");
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_SEARCH_JUDILIBRE);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, 2);
        assert_eq!(records[0].input.as_str(), arguments);
        // Judilibre ne passe pas par le corpus de Légifrance.
        assert_eq!((service.case_law.calls(), service.corpus.calls()), (1, 0));
    }

    #[tokio::test]
    async fn a_port_failure_is_audited_with_its_code() {
        let (service, log) = service_with_case_law();
        service.case_law.push_search(Err(CaseLawError::NotSubscribed));
        let error =
            service.search_judilibre(call("{}", Some("bail")), pending()).await.unwrap_err();
        assert_eq!(error, AppError::config(messages::JUDILIBRE_NOT_SUBSCRIBED_MESSAGE));
        let records = log.records();
        assert_eq!(records[0].error_code, Some(ErrorCode::ConfigError));
    }

    /// Une façade assemblée par `LegalService::new` n'a pas de source Judilibre : l'outil répond
    /// `config_error`, l'appel est audité, et rien n'est demandé au corpus.
    #[tokio::test]
    async fn without_a_case_law_source_the_tool_is_a_config_error() {
        let (service, log) = service_without_doctrine();
        let error =
            service.search_judilibre(call("{}", Some("bail")), pending()).await.unwrap_err();
        assert_eq!(error, AppError::config(messages::JUDILIBRE_DISABLED_MESSAGE));
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_SEARCH_JUDILIBRE);
        assert_eq!(records[0].error_code, Some(ErrorCode::ConfigError));
        assert_eq!(service.corpus.calls(), 0);
    }
}
