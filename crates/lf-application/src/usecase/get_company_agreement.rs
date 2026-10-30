//! Cas d'usage de `fr_get_company_agreement` (v0.1) : la lecture des métadonnées d'un accord d'entreprise (`GetCompanyAgreementUseCase`).
//!
//! Ordre de validation (05 §5.7) : l'identifiant, requis puis de forme `ACCOTEXT` + 12 chiffres
//! (espaces et casse tolérés), avant tout appel au port. Les phrases `invalid_arg` viennent du
//! module `messages` ; une panne du corpus passe par `failure::port_failure`.

use lf_domain::id::AccoTextId;

use crate::{
    error::AppError, failure::port_failure, input::GetCompanyAgreementInput, messages,
    output::get_company_agreement::GetCompanyAgreementOutput, ports::LegalCorpus, tools::ToolName,
};

/// Lit les métadonnées d'un accord d'entreprise (ACCO) par son identifiant.
pub(crate) struct GetCompanyAgreementUseCase<'a, C: LegalCorpus> {
    corpus: &'a C,
}

impl<'a, C: LegalCorpus> GetCompanyAgreementUseCase<'a, C> {
    /// Cas d'usage qui lit `corpus`.
    pub(crate) fn new(corpus: &'a C) -> Self {
        Self { corpus }
    }

    /// Valide l'identifiant, puis lit l'accord.
    ///
    /// # Errors
    ///
    /// - `invalid_arg` si `agreement_id` est absent, blanc ou n'est pas un `ACCOTEXT` valide : aucun
    ///   appel n'est fait au port ;
    /// - `not_found` si le corpus ne connaît pas l'accord, et les autres erreurs du port
    ///   (`upstream_error`, `rate_limited`, `config_error`) traduites par `messages`.
    pub(crate) async fn execute(
        &self,
        input: GetCompanyAgreementInput,
    ) -> Result<GetCompanyAgreementOutput, AppError> {
        let tool = ToolName::FR_GET_COMPANY_AGREEMENT;
        let raw = input
            .agreement_id
            .filter(|raw| !raw.trim().is_empty())
            .ok_or_else(|| messages::required(messages::id_field(tool)))?;
        let id = AccoTextId::parse(&raw).map_err(|error| messages::domain(tool, &error))?;
        let agreement = self
            .corpus
            .get_company_agreement(&id)
            .await
            .map_err(|error| port_failure(tool, id.as_str(), &error))?;
        Ok(GetCompanyAgreementOutput { agreement })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::{ErrorCode, UpstreamCause},
        ports::CorpusError,
        testing::{FakeCorpus, PortRequest},
    };
    use lf_domain::entity::CompanyAgreement;
    use pretty_assertions::assert_eq;

    const ID: &str = "ACCOTEXT000053643828";

    fn agreement() -> CompanyAgreement {
        CompanyAgreement {
            agreement_id: AccoTextId::parse(ID).unwrap(),
            nature: Some("ACCORD".to_owned()),
            title: Some("Accord d'entreprise relatif au télétravail".to_owned()),
            raison_sociale: Some("CORSAIR (Télétravail)".to_owned()),
            siret: Some("32862158600143".to_owned()),
            idcc: None,
            code_ape: Some("5110Z".to_owned()),
            secteur: None,
            themes: vec!["Télétravail".to_owned()],
            syndicats: Vec::new(),
            date_texte: None,
            date_effet: None,
            date_depot: None,
        }
    }

    fn input(id: Option<&str>) -> GetCompanyAgreementInput {
        GetCompanyAgreementInput { agreement_id: id.map(str::to_owned) }
    }

    #[tokio::test]
    async fn reads_the_agreement_with_the_normalized_id() {
        let corpus = FakeCorpus::new();
        corpus.push_get_company_agreement(Ok(agreement()));
        // Espaces et casse tolérés : le port reçoit la forme normalisée.
        let output = GetCompanyAgreementUseCase::new(&corpus)
            .execute(input(Some("  accotext000053643828 ")))
            .await
            .unwrap();
        assert_eq!(output.agreement, agreement());
        assert_eq!(
            corpus.seen_requests(),
            [PortRequest::GetCompanyAgreement(AccoTextId::parse(ID).unwrap())]
        );
    }

    #[tokio::test]
    async fn a_missing_or_blank_id_is_required_without_network() {
        let corpus = FakeCorpus::new();
        let use_case = GetCompanyAgreementUseCase::new(&corpus);
        for raw in [None, Some(""), Some("   ")] {
            let error = use_case.execute(input(raw)).await.unwrap_err();
            assert_eq!(error, AppError::invalid_argument("agreement_id is required."), "{raw:?}");
        }
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn a_malformed_or_foreign_id_is_invalid_arg_without_network() {
        let corpus = FakeCorpus::new();
        let use_case = GetCompanyAgreementUseCase::new(&corpus);
        // Un `LEGITEXT` n'est pas un accord : la phrase renvoie à l'outil du bon préfixe.
        let foreign = use_case.execute(input(Some("LEGITEXT000006070721"))).await.unwrap_err();
        assert_eq!(foreign.code(), Some(ErrorCode::InvalidArg));
        assert!(foreign.message().starts_with("agreement_id='LEGITEXT000006070721'"));
        assert!(foreign.message().contains("fr_get_act"), "{}", foreign.message());
        let short = use_case.execute(input(Some("ACCOTEXT0001"))).await.unwrap_err();
        assert_eq!(short.code(), Some(ErrorCode::InvalidArg));
        assert!(short.message().starts_with("agreement_id='ACCOTEXT0001'"), "{}", short.message());
        assert_eq!(corpus.calls(), 0);
    }

    #[tokio::test]
    async fn an_unknown_agreement_is_not_found_naming_the_id() {
        let corpus = FakeCorpus::new();
        corpus.push_get_company_agreement(Err(CorpusError::NotFound {
            what: "/consult/acco".to_owned(),
        }));
        let error =
            GetCompanyAgreementUseCase::new(&corpus).execute(input(Some(ID))).await.unwrap_err();
        assert_eq!(
            error,
            AppError::not_found(format!(
                "No company agreement found for {ID}. Check the id, or search with fr_search."
            ))
        );
    }

    #[tokio::test]
    async fn port_failures_keep_their_code() {
        let corpus = FakeCorpus::new();
        corpus.push_get_company_agreement(Err(CorpusError::Timeout));
        corpus.push_get_company_agreement(Err(CorpusError::RateLimited { retry_after: None }));
        let use_case = GetCompanyAgreementUseCase::new(&corpus);
        let timeout = use_case.execute(input(Some(ID))).await.unwrap_err();
        assert_eq!(timeout.code(), Some(ErrorCode::UpstreamError));
        assert_eq!(timeout.cause(), Some(UpstreamCause::Timeout));
        let limited = use_case.execute(input(Some(ID))).await.unwrap_err();
        assert_eq!(limited.code(), Some(ErrorCode::RateLimited));
    }
}
