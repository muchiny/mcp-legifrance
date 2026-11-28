//! Méthode de façade `LegalService::get_judilibre` pour `fr_get_judilibre` (jalon MJ).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_GET_JUDILIBRE` : le décodage des arguments, la validation, la lecture de la
//! décision et sa lecture progressive forment l'exécution complète auditée.

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetJudilibreInput, ToolCall},
    output::get_judilibre::GetJudilibreOutput,
    ports::{AuditLog, Clock, JudicialOpenData, LegalCorpus, PublicGuidance, TaxDoctrine},
    tools::ToolName,
    usecase::get_judilibre::GetJudilibreUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock, D: TaxDoctrine, G: PublicGuidance, J: JudicialOpenData>
    LegalService<C, A, K, D, G, J>
{
    /// `fr_get_judilibre` : lecture d'une décision de Judilibre, entière ou par partie (05
    /// §5.19). Métrique d'audit : les caractères renvoyés.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, arguments, `offset` au-delà du texte), `not_found` (décision
    /// inconnue ou retirée), `config_error` (source coupée, sans aucune E/S ; identifiants
    /// absents ; application non abonnée), `rate_limited`, `upstream_error`, le dépassement de
    /// l'échéance globale, et `audit_error` en mode strict.
    pub async fn get_judilibre(
        &self,
        call: ToolCall<GetJudilibreInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetJudilibreOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_GET_JUDILIBRE, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = GetJudilibreUseCase::new(&self.case_law);
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
        ports::{AuditStatus, CaseLawError},
        service::bofip_fixtures::{service_with_case_law, service_without_doctrine},
        testing::fixtures::judilibre,
    };

    fn call(id: Option<&str>) -> ToolCall<GetJudilibreInput> {
        ToolCall {
            input: Ok(GetJudilibreInput {
                id: id.map(str::to_owned),
                ..GetJudilibreInput::default()
            }),
            canonical: CanonicalInput::new(r#"{"id":"5fca624b7eeb384a40cafb85"}"#),
        }
    }

    #[tokio::test]
    async fn a_read_is_audited_with_the_returned_characters() {
        let (service, log) = service_with_case_law();
        service.case_law.push_get(Ok(judilibre::doc(judilibre::CC_ID)));
        let output = service.get_judilibre(call(Some(judilibre::CC_ID)), pending()).await.unwrap();
        let shown = output.reading.text.as_ref().unwrap().chars().count();
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool, ToolName::FR_GET_JUDILIBRE);
        assert_eq!(records[0].status, AuditStatus::Ok);
        assert_eq!(records[0].output_count_or_size, u64::try_from(shown).unwrap());
        assert_eq!((service.case_law.calls(), service.corpus.calls()), (1, 0));
    }

    #[tokio::test]
    async fn an_unknown_decision_is_audited_as_not_found() {
        let (service, log) = service_with_case_law();
        service.case_law.push_get(Err(CaseLawError::NotFound));
        let error =
            service.get_judilibre(call(Some(judilibre::CA_ID)), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::NotFound));
        assert_eq!(log.records()[0].error_code, Some(ErrorCode::NotFound));
    }

    /// Des arguments qui ne se décodent pas sont refusés et audités, sans appel à la source.
    #[tokio::test]
    async fn undecodable_arguments_are_audited_as_invalid_arg_without_network() {
        let (service, log) = service_with_case_law();
        let rejected = ToolCall {
            input: Err(InputError { message: "foo: unknown field `foo`".to_owned() }),
            canonical: CanonicalInput::new(r#"{"foo":1}"#),
        };
        let error = service.get_judilibre(rejected, pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("foo: unknown field `foo`"));
        assert_eq!(log.records()[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(service.case_law.calls(), 0);
    }

    #[tokio::test]
    async fn without_a_case_law_source_the_tool_is_a_config_error() {
        let (service, log) = service_without_doctrine();
        let error =
            service.get_judilibre(call(Some(judilibre::CC_ID)), pending()).await.unwrap_err();
        assert_eq!(error, AppError::config(messages::JUDILIBRE_DISABLED_MESSAGE));
        assert_eq!(log.records()[0].error_code, Some(ErrorCode::ConfigError));
        // Un argument refusé n'atteint pas la source, coupée ou non.
        let error = service.get_judilibre(call(None), pending()).await.unwrap_err();
        assert_eq!(error, AppError::invalid_argument("id is required."));
    }
}
