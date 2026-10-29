//! Méthode de façade `LegalService::get_related` pour `fr_get_related` (v0.3 (M7), 05 §5.11).
//!
//! Un `impl LegalService` à la forme gelée de `service/mod.rs`, sous `Audited::run` avec
//! `ToolName::FR_GET_RELATED` : le décodage des arguments, la validation, les appels au port et
//! l'analyse des liens forment l'exécution complète auditée.

use std::future::Future;

use super::LegalService;
use crate::{
    audited::CallMeta,
    error::AppError,
    input::{GetRelatedInput, ToolCall},
    output::get_related::GetRelatedOutput,
    ports::{AuditLog, Clock, LegalCorpus},
    tools::ToolName,
    usecase::get_related::GetRelatedUseCase,
};

impl<C: LegalCorpus, A: AuditLog, K: Clock> LegalService<C, A, K> {
    /// `fr_get_related` : les liens d'un article ou d'une décision, cent au plus (05 §5.11),
    /// audité sous `fr_get_related` (métrique : nombre de liens renvoyés). `cancel` se résout quand
    /// le client annule.
    ///
    /// # Errors
    ///
    /// `invalid_arg` (décodage, identifiant absent, mal formé ou d'une famille non lue),
    /// `not_found` (article ou décision inconnu), les erreurs communes du port, le dépassement de
    /// l'échéance globale, et `audit_error` en mode strict.
    pub async fn get_related(
        &self,
        call: ToolCall<GetRelatedInput>,
        cancel: impl Future<Output = ()> + Send,
    ) -> Result<GetRelatedOutput, AppError> {
        let meta = CallMeta::new(ToolName::FR_GET_RELATED, self.clock.now_utc(), call.canonical);
        let input = call.input;
        let use_case = GetRelatedUseCase::new(&self.corpus);
        self.audited.run(meta, cancel, async move { use_case.execute(input?).await }).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::get_act::fixtures::service;
    use crate::{
        error::ErrorCode,
        input::{CanonicalInput, GetRelatedInput, InputError, ToolCall},
        ports::{AuditStatus, CorpusError},
        testing::FakeCorpus,
        tools::ToolName,
        usecase::get_decision::fixtures::decision,
    };
    use lf_domain::{
        entity::{ArticleText, RelatedLink, RelatedLinks, RelatedNature},
        id::{ArticleId, DocumentId},
        text::LongText,
    };
    use pretty_assertions::assert_eq;
    use std::future::{pending, ready};

    const ARTI: &str = "LEGIARTI000006419288";
    const JURI: &str = "JURITEXT000051743650";

    fn call(arguments: &str, id: Option<&str>) -> ToolCall<GetRelatedInput> {
        ToolCall {
            input: Ok(GetRelatedInput { id: id.map(str::to_owned) }),
            canonical: CanonicalInput::new(arguments),
        }
    }

    fn article() -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(ARTI).unwrap(),
            num: Some("9".to_owned()),
            etat: None,
            code_title: Some("Code civil".to_owned()),
            parent: None,
            path: vec![],
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
            nota: None,
        }
    }

    #[tokio::test]
    async fn a_read_is_audited_with_the_number_of_links_returned() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Ok(article()));
        corpus.push_related_links(Ok(RelatedLinks {
            cites: vec![RelatedLink {
                target: DocumentId::parse("LEGIARTI000032041571").ok(),
                nature: RelatedNature::Code,
                title: Some("Code civil, art. 1240".to_owned()),
                date: None,
            }],
            cited_by: vec![],
        }));
        let (service, audit) = service(corpus);
        let arguments = format!(r#"{{"id":"{ARTI}"}}"#);

        let output = service.get_related(call(&arguments, Some(ARTI)), pending()).await.unwrap();

        assert_eq!(output.links.len(), 1);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool, ToolName::FR_GET_RELATED);
        assert_eq!(lines[0].status, AuditStatus::Ok);
        assert_eq!(lines[0].error_code, None);
        assert_eq!(lines[0].output_count_or_size, 1);
        assert_eq!(lines[0].input.as_str(), arguments);
    }

    #[tokio::test]
    async fn a_decision_without_references_is_audited_with_zero_links() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(JURI, "texte")));
        let (service, audit) = service(corpus);

        service.get_related(call("{}", Some(JURI)), pending()).await.unwrap();

        assert_eq!(audit.records()[0].output_count_or_size, 0);
    }

    #[tokio::test]
    async fn an_unknown_article_is_audited_as_not_found() {
        let corpus = FakeCorpus::new();
        corpus.push_get_article(Err(CorpusError::NotFound { what: "/consult/getArticle".into() }));
        corpus.push_related_links(Err(CorpusError::NotFound { what: "/consult/related".into() }));
        let (service, audit) = service(corpus);

        let error = service.get_related(call("{}", Some(ARTI)), pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::NotFound));
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

        let error = service.get_related(rejected, pending()).await.unwrap_err();

        assert_eq!(error.code(), Some(ErrorCode::InvalidArg));
        assert_eq!(error.message(), "foo: unknown field `foo`");
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tool, ToolName::FR_GET_RELATED);
        assert_eq!(lines[0].error_code, Some(ErrorCode::InvalidArg));
    }

    #[tokio::test]
    async fn a_missing_id_is_audited_as_invalid_arg() {
        let (service, audit) = service(FakeCorpus::new());
        let error = service.get_related(call("{}", None), pending()).await.unwrap_err();
        assert_eq!(error.message(), "id is required.");
        assert_eq!(audit.records()[0].error_code, Some(ErrorCode::InvalidArg));
    }

    #[tokio::test]
    async fn a_cancelled_call_is_audited_without_a_code() {
        let corpus = FakeCorpus::new();
        corpus.push_get_decision(Ok(decision(JURI, "texte")));
        let (service, audit) = service(corpus);

        let error = service.get_related(call("{}", Some(JURI)), ready(())).await.unwrap_err();

        assert_eq!(error.code(), None);
        let lines = audit.records();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].status, AuditStatus::Error);
        assert_eq!(lines[0].error_code, None);
    }
}
