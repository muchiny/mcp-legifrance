//! Méthode de façade `LegalService::read_resource` (v0.3, M7).
//!
//! Exception au motif des outils : la signature est
//! `read_resource(&self, call: ToolCall<ResourceInput>, cancel)`, et l'appel est audité sous
//! `resource:<nom du gabarit>` (`ToolName::RESOURCE_*`, jamais dans `ToolName::ALL`), avec pour
//! métrique les caractères renvoyés.
//!
//! Le gabarit n'est connu qu'une fois l'URI analysée : l'analyse se fait donc avant `Audited::run`.
//! Une URI absente, hors grammaire ou sans gabarit n'a pas de nom de ligne à porter et n'écrit pas
//! de ligne d'audit, comme un outil inconnu (02 §9.2). Toute lecture d'un gabarit reconnu l'est,
//! succès comme échec.

use std::future::Future;

use super::LegalService;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{ResourceInput, ToolCall},
    messages,
    output::resource::ResourceOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    usecase::resource::{ResourceRef, ResourceUseCase},
};

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// Lit la resource `legifrance://...` désignée par `call`. `cancel` se résout quand le client
    /// annule.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` pour une URI absente ou hors grammaire, ou un identifiant invalide ;
    /// - `not_found` pour une URI sans gabarit ou un enregistrement introuvable (l'adapter MCP en
    ///   fait `-32002` ou `-32602` selon l'ère du pair) ;
    /// - les autres erreurs du port, comme celles des outils.
    ///
    /// L'issue d'un gabarit reconnu est auditée dans tous les cas.
    pub async fn read_resource(
        &self,
        call: ToolCall<ResourceInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<ResourceOutput, AppError> {
        let input = call.input?;
        let uri = input.uri.ok_or_else(|| messages::required("uri"))?;
        let target = ResourceRef::parse(&uri)?;
        let meta = CallMeta::new(target.kind().audit_name(), self.clock.now_utc(), call.canonical);
        self.audited
            .run(meta, cancel, async {
                ResourceUseCase::new(&self.corpus, &self.clock, &self.settings)
                    .execute(target)
                    .await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Arc, time::Duration};

    use chrono::DateTime;
    use lf_domain::{
        date::CivilDate,
        entity::{Act, ArticleText, CodeListing, Decision, OfficialSummary, TextBody},
        id::{ArticleId, DecisionId, LegiTextId},
        status::LegalStatus,
        text::LongText,
    };
    use pretty_assertions::assert_eq;

    use crate::{
        audited::{AuditMetric, AuditMode, AuditPolicy},
        budget::ResponseBudget,
        error::ErrorCode,
        input::{CanonicalInput, InputError, ResourceInput, ToolCall},
        output::resource::ResourceKind,
        ports::{AuditError, AuditLog, AuditRecord, AuditStatus, CorpusError, PisteEnvironment},
        service::{ConfigurationState, LegalService, ServiceSettings},
        testing::{FakeCorpus, FixedClock, MemoryAuditLog},
    };

    const ARTICLE_ID: &str = "LEGIARTI000006419288";
    const DECISION_ID: &str = "JURITEXT000037999394";
    const TEXT_ID: &str = "LEGITEXT000006070721";
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

    fn call(uri: &str) -> ToolCall<ResourceInput> {
        ToolCall {
            input: Ok(ResourceInput { uri: Some(uri.to_owned()) }),
            canonical: CanonicalInput::new(format!("{{\"uri\":\"{uri}\"}}")),
        }
    }

    fn article() -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTICLE_ID).unwrap(),
            num: Some("9".to_owned()),
            etat: None,
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: Vec::new(),
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain(TEXT).unwrap(),
            nota: None,
        }
    }

    fn decision() -> Decision {
        Decision {
            decision_id: DecisionId::parse(DECISION_ID).unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: Vec::new(),
            date_texte: None,
            nature: None,
            title: Some("Cass. civ. 1re, 5 juin 2019".to_owned()),
            text: LongText::from_plain("La Cour rejette.").unwrap(),
            official_summary: OfficialSummary::default(),
        }
    }

    fn act() -> Act {
        Act {
            text_id: LegiTextId::parse(TEXT_ID).unwrap(),
            body: TextBody { title: Some("Code civil".to_owned()), ..TextBody::default() },
        }
    }

    fn codes() -> Vec<CodeListing> {
        vec![CodeListing {
            id: LegiTextId::parse(TEXT_ID).unwrap(),
            title: "Code civil".to_owned(),
            etat: Some(LegalStatus::Vigueur),
        }]
    }

    #[tokio::test]
    async fn resource_read_audited_as_resource_name() {
        let (service, audit) = service();
        service.corpus.push_get_article(Ok(article()));

        let output = service
            .read_resource(call("legifrance://article/LEGIARTI000006419288"), pending())
            .await;
        let output = output.unwrap();

        assert_eq!(output.kind, ResourceKind::Article);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool.as_str(), "resource:article");
        assert_eq!(lines[0].status, AuditStatus::Ok);
        assert_eq!(lines[0].output_count_or_size, output.output_count_or_size());
        assert!(lines[0].output_count_or_size > u64::try_from(TEXT.chars().count()).unwrap());
        assert_eq!(
            lines[0].input.as_str(),
            "{\"uri\":\"legifrance://article/LEGIARTI000006419288\"}"
        );
    }

    #[tokio::test]
    async fn every_template_is_audited_under_its_own_resource_name() {
        let (service, audit) = service();
        service.corpus.push_get_article(Ok(article()));
        service.corpus.push_get_decision(Ok(decision()));
        service.corpus.push_get_act(Ok(act()));
        service.corpus.push_list_codes(Ok(codes()));
        service.corpus.push_find_article(Ok(article()));
        let uris = [
            "legifrance://coverage",
            "legifrance://article/LEGIARTI000006419288",
            "legifrance://decision/JURITEXT000037999394",
            "legifrance://act/LEGITEXT000006070721",
            "legifrance://code/code-civil/article/9",
        ];

        for uri in uris {
            service.read_resource(call(uri), pending()).await.unwrap();
        }

        let names: Vec<String> =
            audit.records().iter().map(|line| line.tool.as_str().to_owned()).collect();
        assert_eq!(
            names,
            [
                "resource:coverage",
                "resource:article",
                "resource:decision",
                "resource:act",
                "resource:code_article"
            ]
        );
        assert!(audit.records().iter().all(|line| line.status == AuditStatus::Ok));
        // Une couverture compte ses lacunes, un Markdown ses caractères : jamais zéro.
        assert!(audit.records().iter().all(|line| line.output_count_or_size > 0));
    }

    #[tokio::test]
    async fn a_missing_record_is_audited_as_not_found_under_the_resource_name() {
        let (service, audit) = service();
        service.corpus.push_get_article(Err(CorpusError::NotFound { what: "x".to_owned() }));
        service.corpus.push_list_codes(Ok(codes()));

        let by_id = service
            .read_resource(call("legifrance://article/LEGIARTI000006419288"), pending())
            .await
            .unwrap_err();
        let by_code = service
            .read_resource(call("legifrance://code/code-inconnu/article/9"), pending())
            .await
            .unwrap_err();

        assert_eq!(by_id.code(), Some(ErrorCode::NotFound));
        assert_eq!(by_code.code(), Some(ErrorCode::NotFound));
        let lines = audit.records();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].tool.as_str(), "resource:article");
        assert_eq!(lines[1].tool.as_str(), "resource:code_article");
        for line in &lines {
            assert_eq!(line.status, AuditStatus::Error);
            assert_eq!(line.error_code, Some(ErrorCode::NotFound));
            assert_eq!(line.output_count_or_size, 0);
        }
    }

    #[tokio::test]
    async fn an_invalid_identifier_of_a_known_template_is_audited_as_invalid_arg() {
        let (service, audit) = service();

        let error = service
            .read_resource(call("legifrance://decision/not-an-id"), pending())
            .await
            .unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(service.corpus.calls(), 0);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool.as_str(), "resource:decision");
        assert_eq!(lines[0].error_code, Some(ErrorCode::InvalidArg));
    }

    #[tokio::test]
    async fn an_unrecognised_or_malformed_uri_is_refused_without_audit_or_network() {
        let (service, audit) = service();

        let unknown =
            service.read_resource(call("legifrance://unknown/x"), pending()).await.unwrap_err();
        let malformed =
            service.read_resource(call("file:///etc/passwd"), pending()).await.unwrap_err();
        let traversal =
            service.read_resource(call("legifrance://article/.."), pending()).await.unwrap_err();

        assert_eq!(unknown.code(), Some(ErrorCode::NotFound));
        assert_eq!(malformed.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(traversal.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(service.corpus.calls(), 0);
        assert_eq!(audit.records().len(), 0);
    }

    #[tokio::test]
    async fn a_missing_uri_or_a_decoding_failure_is_invalid_arg_without_audit() {
        let (service, audit) = service();
        let missing = ToolCall {
            input: Ok(ResourceInput { uri: None }),
            canonical: CanonicalInput::new("{}"),
        };
        let undecodable = ToolCall {
            input: Err(InputError { message: "uri: invalid type: integer `1`".to_owned() }),
            canonical: CanonicalInput::new("{\"uri\":1}"),
        };

        let missing = service.read_resource(missing, pending()).await.unwrap_err();
        let undecodable = service.read_resource(undecodable, pending()).await.unwrap_err();

        assert_eq!(missing.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(missing.message(), "uri is required.");
        assert_eq!(undecodable.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(undecodable.message(), "uri: invalid type: integer `1`");
        assert_eq!(audit.records().len(), 0);
    }
}
