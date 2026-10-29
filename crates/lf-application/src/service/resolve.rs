//! Méthode de façade `LegalService::resolve` pour `fr_resolve` (v0.2 (M6), 05 §5.10).
//!
//! Elle suit le motif gelé de `service/mod.rs` : l'identité de l'appel (outil, heure, arguments
//! canoniques) est posée avant tout, puis le décodage, la validation et le cas d'usage s'exécutent
//! ensemble sous [`Audited::run`](crate::audited::Audited::run).

use std::future::Future;

use crate::{
    audited::CallMeta,
    error::AppError,
    input::{ResolveInput, ToolCall},
    output::resolve::ResolveOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::resolve::ResolveUseCase,
};

use super::LegalService;

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_resolve` : transforme une citation libre en identifiants Légifrance (05 §5.10), auditée
    /// sous `fr_resolve` (métrique : 1).
    ///
    /// `cancel` se résout quand le client annule l'appel ; la résolution en vol est alors
    /// abandonnée et l'appel est audité comme annulé.
    ///
    /// # Errors
    ///
    /// L'erreur du cas d'usage (`invalid_arg` pour un décodage ou une validation refusés, l'erreur
    /// amont quand le corpus échoue), `upstream_error` au-delà de l'échéance, `audit_error` en mode
    /// strict quand la ligne ne peut pas être écrite.
    pub async fn resolve(
        &self,
        call: ToolCall<ResolveInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<ResolveOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_RESOLVE, self.clock.now_utc(), call.canonical);
        let input = call.input;
        self.audited
            .run(meta, cancel, async {
                let input = input.map_err(AppError::from)?;
                ResolveUseCase::new(&self.corpus, &self.clock, self.settings.proof())
                    .execute(input)
                    .await
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
        entity::{ArticleText, CodeListing},
        id::{ArticleId, LegiTextId},
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

    /// Un service sur `corpus`, et le journal d'audit qu'il alimente.
    fn rig(
        corpus: FakeCorpus,
        env: PisteEnvironment,
    ) -> (LegalService<FakeCorpus, SharedLog, FixedClock>, Arc<MemoryAuditLog>) {
        rig_with(corpus, env, ConfigurationState::default())
    }

    /// Idem, avec l'état de configuration `configuration`.
    fn rig_with(
        corpus: FakeCorpus,
        env: PisteEnvironment,
        configuration: ConfigurationState,
    ) -> (LegalService<FakeCorpus, SharedLog, FixedClock>, Arc<MemoryAuditLog>) {
        let audit = Arc::new(MemoryAuditLog::new());
        let settings = ServiceSettings {
            policy: AuditPolicy {
                mode: AuditMode::Strict,
                deadline: Duration::from_secs(60),
                env,
                server_version: "9.9.9",
            },
            configuration,
            budget: ResponseBudget,
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 29, 8, 30, 0).unwrap();
        let clock = FixedClock::new(now, CivilDate::new(2026, 10, 29).unwrap());
        let service = LegalService::new(corpus, SharedLog(Arc::clone(&audit)), clock, settings);
        (service, audit)
    }

    fn call(citation: &str) -> ToolCall<ResolveInput> {
        let input = ResolveInput { citation: Some(citation.to_owned()), date: None };
        ToolCall { input: Ok(input), canonical: CanonicalInput::new("{\"citation\":\"…\"}") }
    }

    fn civil_code_with_article_9(corpus: &FakeCorpus) {
        corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
        corpus.push_find_article(Ok(ArticleText {
            article_id: ArticleId::parse("LEGIARTI000006419288").unwrap(),
            num: Some("9".to_owned()),
            etat: Some(LegalStatus::Vigueur),
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
            nota: None,
        }));
    }

    #[tokio::test]
    async fn resolve_is_audited_under_its_name_with_a_metric_of_one() {
        let corpus = FakeCorpus::new();
        civil_code_with_article_9(&corpus);
        let (service, audit) = rig(corpus, PisteEnvironment::Production);

        let output = service.resolve(call("art. 9 C. civ."), pending()).await.unwrap();
        assert_eq!(output.candidates.len(), 1);

        let records = audit.records();
        assert_eq!(records.len(), 1, "une seule ligne par appel");
        let record = &records[0];
        assert_eq!(record.tool, ToolName::FR_RESOLVE);
        assert_eq!(record.status, AuditStatus::Ok);
        assert_eq!(record.output_count_or_size, 1);
        assert_eq!(record.server_version, "9.9.9");
    }

    /// Le drapeau d'instantané local arrive jusqu'au moteur : la note d'une absence parle de
    /// l'instantané, bien que l'environnement de l'audit soit `production`.
    #[tokio::test]
    async fn the_local_snapshot_flag_reaches_the_note_of_an_absence() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Ok(vec![CodeListing {
            id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]));
        corpus.push_find_article(Err(CorpusError::NotFound { what: "x".to_owned() }));
        let configuration = ConfigurationState { local_snapshot: true, ..Default::default() };
        let (service, audit) = rig_with(corpus, PisteEnvironment::Production, configuration);
        let output = service.resolve(call("article 9999 du code civil"), pending()).await.unwrap();
        assert_eq!(output.candidates, []);
        assert_eq!(
            output.notes[0].text(),
            "No article record matches Code civil, art. 9999 on 2026-10-29 in the local DILA \
             snapshot, which may be out of date."
        );
        assert_eq!(audit.records()[0].env, PisteEnvironment::Production);
    }

    #[tokio::test]
    async fn an_empty_answer_is_still_a_success_audited_with_a_metric_of_one() {
        let (service, audit) = rig(FakeCorpus::new(), PisteEnvironment::Production);
        let output = service.resolve(call("bonjour"), pending()).await.unwrap();
        assert_eq!(output.candidates, []);
        let records = audit.records();
        assert_eq!((records[0].status, records[0].output_count_or_size), (AuditStatus::Ok, 1));
    }

    #[tokio::test]
    async fn a_decoding_failure_is_audited_as_invalid_arg_without_calling_the_port() {
        let (service, audit) = rig(FakeCorpus::new(), PisteEnvironment::Production);
        let failed = ToolCall::<ResolveInput> {
            input: Err(InputError { message: "date: invalid type: integer".to_owned() }),
            canonical: CanonicalInput::new("{\"date\":20261029}"),
        };
        let error = service.resolve(failed, pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error_code, Some(ErrorCode::InvalidArg));
        assert_eq!(records[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn an_upstream_failure_is_audited_as_upstream_error() {
        let corpus = FakeCorpus::new();
        corpus.push_list_codes(Err(CorpusError::Timeout));
        let (service, audit) = rig(corpus, PisteEnvironment::Production);
        let error = service.resolve(call("art. 9 C. civ."), pending()).await.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::UpstreamError));
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let (service, audit) = rig(FakeCorpus::new(), PisteEnvironment::Production);
        let error = service.resolve(call("art. 9 C. civ."), ready(())).await.unwrap_err();
        assert_eq!(error.code(), None);
        assert_eq!(error.cause(), Some(UpstreamCause::Cancelled));
        let records = audit.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, AuditStatus::Error);
    }
}
