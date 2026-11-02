//! Outil `fr_verify_citations` (05 §5.9) : le contrôle, en un appel, de toutes les citations
//! juridiques d'un texte contre le registre officiel.
//!
//! Motif de `tools/mod.rs` : arguments bruts (`JsonObject`), décodage par `to_call`, appel de la
//! façade avec le jeton d'annulation de la requête, vue de sortie à schéma explicite. La vue
//! ramène elle-même la réponse sous le plafond dur de 45 000 caractères (05 §6.4).

use lf_application::{
    input::VerifyCitationsInput,
    ports::{AuditLog, Clock, LegalCorpus},
};
use rmcp::{Json, model::JsonObject};
use rmcp_macros::{tool, tool_router};
use tokio_util::sync::CancellationToken;

use crate::{
    decode::to_call,
    failure::ToolFailure,
    params::verify_citations::VerifyCitationsParams,
    schema::{input_schema, output_schema},
    server::LegifranceServer,
    views::verify_citations::VerifyView,
};

/// Description de l'outil (987 caractères, 5 phrases ; 05 §5.9 ; D-12 : ce que le contrôle ne dit pas).
pub(crate) const DESCRIPTION: &str = "Check every legal citation found in a text of up to 20,000 characters, for example a draft answer, against the official Légifrance record. Recognises code articles (a bare number needs article/art.; L-, R- or D- numbers followed by the code name do not), laws and decrees, court decisions, ECLI, CNIL deliberations and collective agreements; found=0 does not prove that a text cites nothing. For each citation returns its `span`, a `status` (verified, not_found, ambiguous, unresolvable or invalid), the official record when found and a `note` explaining any mismatch; `verified` means the article exists, not that it says what the draft claims. A quoted excerpt is compared with the official text (`quote`) only when it is glued to the citation: no `quote` means NOT checked. It checks the first `max_citations` (default 50); if `truncated` is true, call again on the rest of the text. Use it before giving the user an answer that cites law; for a single citation fr_resolve is lighter.";

#[tool_router(router = verify_citations_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
    #[tool(
        name = "fr_verify_citations",
        title = "Verify legal citations",
        description = DESCRIPTION,
        input_schema = input_schema::<VerifyCitationsParams>(),
        output_schema = output_schema::<VerifyView>(),
        annotations(
            title = "Verify legal citations",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_verify_citations(
        &self,
        args: JsonObject,
        ct: CancellationToken,
    ) -> Result<Json<VerifyView>, ToolFailure> {
        let call = to_call::<VerifyCitationsParams, VerifyCitationsInput>(args);
        let output = self.service.verify_citations(call, ct.cancelled()).await?;
        Ok(Json(VerifyView::from(output)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_description_is_the_documented_text() {
        assert_eq!(DESCRIPTION.chars().count(), 987);
        assert!(DESCRIPTION.starts_with("Check every legal citation found in a text"));
        assert!(DESCRIPTION.contains("`max_citations` (default 50)"));
        assert!(DESCRIPTION.ends_with("for a single citation fr_resolve is lighter."));
        // Les outils cités le sont par leur nom d'enregistrement (05 §4.3).
        assert!(DESCRIPTION.contains("fr_resolve"));
    }

    /// D-12 (verify_citations-4 et -8, journey_patrimoine-3) : ce que le contrôle ne dit pas.
    #[test]
    fn the_description_says_what_is_checked_and_what_is_not() {
        // Seul un extrait collé à la citation est contrôlé ; sans `quote`, rien n'a été contrôlé.
        assert!(DESCRIPTION.contains("only when it is glued to the citation"), "{DESCRIPTION}");
        assert!(DESCRIPTION.contains("no `quote` means NOT checked"), "{DESCRIPTION}");
        // `found` à 0 ne prouve pas l'absence de citation ; un article de code se lit après
        // « article » ou « art. ».
        assert!(DESCRIPTION.contains("found=0 does not prove"), "{DESCRIPTION}");
        // Revue finale de la PR 2 : seul un numéro nu exige « article » ou « art. » ; un numéro
        // L., R. ou D. suivi du nom du code est lu sans.
        assert!(
            DESCRIPTION.contains(
                "a bare number needs article/art.; L-, R- or D- numbers followed by the code name do not"
            ),
            "{DESCRIPTION}"
        );
        assert!(!DESCRIPTION.contains("only when introduced"), "{DESCRIPTION}");
        // `verified` ne dit pas que l'article dit ce que le brouillon prétend.
        assert!(DESCRIPTION.contains("`verified` means the article exists"), "{DESCRIPTION}");
        assert!(DESCRIPTION.chars().count() <= 1_000);
    }

    #[cfg(feature = "test-support")]
    mod over_the_wire {
        use lf_application::{
            error::ErrorCode,
            ports::{AuditStatus, CorpusError},
            tools::ToolName,
        };
        use lf_domain::{
            date::CivilDate,
            entity::{ArticleText, CodeListing},
            id::{ArticleId, LegiTextId},
            status::LegalStatus,
            text::LongText,
        };
        use pretty_assertions::assert_eq;
        use rmcp::model::CallToolRequestParams;
        use serde_json::{Value, json};

        use crate::testing::Fixture;

        async fn call(fixture: &Fixture, arguments: &Value) -> rmcp::model::CallToolResult {
            let client = fixture.connect().await;
            let mut params = CallToolRequestParams::new("fr_verify_citations");
            params.arguments = arguments.as_object().cloned();
            client.call_tool(params).await.unwrap()
        }

        fn civil_code_listed(fixture: &Fixture) {
            fixture.corpus.push_list_codes(Ok(vec![CodeListing {
                id: LegiTextId::parse("LEGITEXT000006070721").unwrap(),
                title: "Code civil".to_owned(),
                etat: Some(LegalStatus::Vigueur),
            }]));
        }

        #[tokio::test]
        async fn a_missing_article_comes_back_with_its_span_status_and_note() {
            let fixture = Fixture::default();
            civil_code_listed(&fixture);
            fixture.corpus.push_find_article(Err(CorpusError::NotFound { what: "x".to_owned() }));
            let text = "Voir l'article 9999 du code civil.";

            let result = call(&fixture, &json!({"text": text})).await;

            assert_eq!(result.is_error, Some(false));
            let content = result.structured_content.clone().unwrap();
            assert_eq!(
                (content["found"].clone(), content["checked"].clone()),
                (json!(1), json!(1))
            );
            assert!(content.get("truncated").is_none());
            let citation = &content["citations"][0];
            assert_eq!(citation["kind"], "code_article");
            assert_eq!(citation["raw"], "article 9999 du code civil");
            assert_eq!(citation["span"], json!({"start": 7, "end": 33}));
            // L'environnement de la fixture décide : jamais `not_found` en bac à sable.
            let status = citation["status"].as_str().unwrap();
            assert!(["not_found", "unresolvable"].contains(&status), "{status}");
            assert!(citation["note"].as_str().unwrap().contains("9999"));
            // Le bloc texte porte le même JSON que `structuredContent`.
            let text_block = &result.content[0].as_text().unwrap().text;
            assert_eq!(serde_json::from_str::<Value>(text_block).unwrap(), content);

            let records = fixture.audit.records();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].tool, ToolName::FR_VERIFY_CITATIONS);
            assert_eq!(records[0].status, AuditStatus::Ok);
            assert_eq!(records[0].output_count_or_size, 1, "le nombre de citations contrôlées");
        }

        /// D-03-suite : un article cité par son identifiant LEGIARTI porte `etat`, `date_debut`
        /// et `date_fin` sur le fil ; la date de fin sentinelle 2999 est omise ; pas de `note`
        /// de version sur ce chemin (asymétrie assumée avec le chemin par numéro).
        #[tokio::test]
        async fn an_article_cited_by_its_identifier_carries_its_validity_on_the_wire() {
            let article = |date_fin| ArticleText {
                article_id: ArticleId::parse("LEGIARTI000006419288").unwrap(),
                num: Some("9".to_owned()),
                etat: LegalStatus::parse("VIGUEUR"),
                code_title: Some("Code civil".to_owned()),
                parent: None,
                path: Vec::new(),
                full_sections_titre: None,
                date_debut: CivilDate::new(1970, 7, 19),
                date_fin,
                text: LongText::from_plain("Chacun a droit au respect de sa vie privée.").unwrap(),
                nota: None,
            };
            let fixture = Fixture::default();
            fixture.corpus.push_get_article(Ok(article(CivilDate::new(2999, 1, 1))));
            let result =
                call(&fixture, &json!({"text": "Voir LEGIARTI000006419288 pour la règle."})).await;
            assert_eq!(result.is_error, Some(false));
            let content = result.structured_content.unwrap();
            let citation = &content["citations"][0];
            assert_eq!(citation["status"], "verified", "{content}");
            let resolved = &citation["resolved"];
            assert_eq!(resolved["etat"], "VIGUEUR", "{content}");
            assert_eq!(resolved["date_debut"], "1970-07-19", "{content}");
            assert!(resolved.get("date_fin").is_none(), "sentinelle omise : {content}");
            assert!(citation.get("note").is_none(), "{content}");

            // Jumeau : une vraie date de fin est rendue.
            let fixture = Fixture::default();
            fixture.corpus.push_get_article(Ok(article(CivilDate::new(2011, 6, 12))));
            let result =
                call(&fixture, &json!({"text": "Voir LEGIARTI000006419288 pour la règle."})).await;
            let content = result.structured_content.unwrap();
            assert_eq!(content["citations"][0]["resolved"]["date_fin"], "2011-06-12", "{content}");
        }

        #[tokio::test]
        async fn invalid_arguments_are_audited_as_invalid_arg() {
            let fixture = Fixture::default();
            let result =
                call(&fixture, &json!({"text": "article 9 du code civil", "max_citations": 0}))
                    .await;
            assert_eq!(result.is_error, Some(true));
            let message = &result.content[0].as_text().unwrap().text;
            assert_eq!(message, "[invalid_arg] max_citations=0 out of range (1..100).");
            assert_eq!(fixture.audit.records()[0].error_code, Some(ErrorCode::InvalidArg));

            let result = call(&fixture, &json!({})).await;
            assert_eq!(
                result.content[0].as_text().unwrap().text,
                "[invalid_arg] text is required."
            );
            assert_eq!(fixture.corpus.calls(), 0);
        }

        #[tokio::test]
        async fn every_citation_failing_upstream_fails_the_call() {
            let fixture = Fixture::default();
            civil_code_listed(&fixture);
            fixture.corpus.push_find_article(Err(CorpusError::Timeout));
            let result = call(&fixture, &json!({"text": "article 9 du code civil"})).await;
            assert_eq!(result.is_error, Some(true));
            let message = &result.content[0].as_text().unwrap().text;
            assert!(message.starts_with("[upstream_error] "), "{message}");
            assert_eq!(fixture.audit.records()[0].error_code, Some(ErrorCode::UpstreamError));
        }
    }
}
