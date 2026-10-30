//! Resources MCP (`resources/list`, `resources/templates/list`, `resources/read`), v0.3 (M7,
//! 05 §8.1 à §8.3).
//!
//! Une resource statique (`legifrance://coverage`) et quatre gabarits RFC 6570 de niveau 1
//! (`legifrance://article/{article_id}`, `…/decision/{decision_id}`, `…/act/{text_id}`,
//! `…/code/{code}/article/{num}`). La correspondance inverse, la validation de toute URI et la
//! lecture sont dans `lf-application` (`LegalService::read_resource`) ; ce module décrit les
//! resources, bâtit l'appel et rend la réponse ou l'erreur JSON-RPC.
//!
//! # Erreurs (05 §7.5, §8.3)
//!
//! - enregistrement ou URI introuvable : `ErrorData::resource_not_found` avec `data.uri` (`-32002`
//!   aux pairs legacy ; rmcp le réécrit en `-32602` aux pairs 2026-07-28), jamais `contents: []` ;
//! - URI hors grammaire ou identifiant invalide : `-32602` avec `data.uri`, dans les deux ères ;
//! - toute autre erreur : `-32603` dont le message reprend `[code] message` comme un outil.

use lf_application::{
    error::{AppError, ErrorCode},
    input::{ResourceInput, ToolCall},
    output::resource::{ResourceContent, ResourceKind, ResourceOutput},
    usecase::resource::is_resource_uri,
};
use rmcp::{
    ErrorData,
    model::{
        JsonObject, ListResourceTemplatesResult, ListResourcesResult, ReadResourceResult, Resource,
        ResourceContents, ResourceTemplate,
    },
};
use serde_json::{Value, json};

use crate::{canonical, views::coverage::CoverageView};

/// Description d'une resource ou d'un gabarit annoncé (05 §8.2).
struct Entry {
    /// URI de la resource, ou gabarit RFC 6570.
    uri: &'static str,
    /// Nom programmatique.
    name: &'static str,
    /// Titre lisible, en anglais (Q14).
    title: &'static str,
    /// À quoi sert la resource, pour l'hôte et le modèle.
    description: &'static str,
}

/// Ce qui décrit le gabarit `kind`.
const fn entry(kind: ResourceKind) -> Entry {
    match kind {
        ResourceKind::Coverage => Entry {
            uri: "legifrance://coverage",
            name: "coverage",
            title: "Coverage and known gaps",
            description: "What this server covers, how it is sourced and what it does NOT cover, \
                          with the PISTE environment and the state of the configuration (the \
                          same object as the fr_coverage tool).",
        },
        ResourceKind::Article => Entry {
            uri: "legifrance://article/{article_id}",
            name: "article",
            title: "Article (full text)",
            description: "The full text of one consolidated article, by its LEGIARTI identifier, \
                          as Markdown. At most 40,000 characters; a last line gives the offset to \
                          resume with fr_get_text.",
        },
        ResourceKind::Decision => Entry {
            uri: "legifrance://decision/{decision_id}",
            name: "decision",
            title: "Court decision (full text)",
            description: "The full text of one court decision, by its JURITEXT, CETATEXT or \
                          CONSTEXT identifier, as Markdown, with the official summary. At most \
                          40,000 characters; a last line gives the offset to resume with \
                          fr_get_decision.",
        },
        ResourceKind::Act => Entry {
            uri: "legifrance://act/{text_id}",
            name: "act",
            title: "Law, decree or code (table of contents)",
            description: "The metadata and the nested table of contents of one consolidated text, \
                          by its LEGITEXT identifier (with an optional _DD-MM-YYYY version \
                          suffix), as Markdown. At most 500 entries.",
        },
        ResourceKind::CodeArticle => Entry {
            uri: "legifrance://code/{code}/article/{num}",
            name: "code_article",
            title: "Code article by number",
            description: "The full text of one article of a code, by the code slug (code-civil, \
                          code-du-travail...) and the article number (L. 1234-5), in force today \
                          in Paris, as Markdown. Complete the code variable to list the slugs.",
        },
    }
}

/// `resources/list` : la resource statique, `legifrance://coverage`.
pub(crate) fn list() -> ListResourcesResult {
    let coverage = entry(ResourceKind::Coverage);
    let resource = Resource::new(coverage.uri, coverage.name)
        .with_title(coverage.title)
        .with_description(coverage.description)
        .with_mime_type(ResourceKind::Coverage.mime_type());
    ListResourcesResult::with_all_items(vec![resource])
}

/// `resources/templates/list` : les quatre gabarits, dans l'ordre de 05 §8.2.
pub(crate) fn list_templates() -> ListResourceTemplatesResult {
    let templates = ResourceKind::ALL
        .into_iter()
        .filter(|kind| *kind != ResourceKind::Coverage)
        .map(|kind| {
            let template = entry(kind);
            ResourceTemplate::new(template.uri, template.name)
                .with_title(template.title)
                .with_description(template.description)
                .with_mime_type(kind.mime_type())
        })
        .collect();
    ListResourceTemplatesResult::with_all_items(templates)
}

/// L'appel de lecture de `uri` : l'entrée (l'URI telle que reçue) et son JSON canonique, qui
/// donne l'empreinte de la ligne d'audit.
pub(crate) fn call(uri: &str) -> ToolCall<ResourceInput> {
    let mut arguments = JsonObject::new();
    arguments.insert("uri".to_owned(), Value::String(uri.to_owned()));
    ToolCall {
        canonical: canonical::of(&arguments),
        input: Ok(ResourceInput { uri: Some(uri.to_owned()) }),
    }
}

/// Le résultat de `resources/read` : un seul contenu, de l'URI demandée. Sans indications de
/// cache : le serveur les pose, aux seuls pairs modernes.
pub(crate) fn contents(uri: &str, output: ResourceOutput) -> ReadResourceResult {
    let mime_type = output.kind.mime_type();
    let text = match output.content {
        ResourceContent::Markdown(markdown) => markdown,
        // La vue ne contient que des chaînes, des booléens et des listes : sa sérialisation ne
        // peut pas échouer.
        ResourceContent::Coverage(coverage) => {
            serde_json::to_string(&CoverageView::from(coverage)).unwrap_or_default()
        }
    };
    ReadResourceResult::new(vec![ResourceContents::text(text, uri).with_mime_type(mime_type)])
}

/// L'erreur JSON-RPC d'une lecture refusée (05 §8.3). `data.uri` ne recopie qu'une URI de la
/// grammaire : rmcp journalise l'erreur entière, et une URI hors grammaire est du texte libre du
/// client (jamais de texte de requête dans un journal).
pub(crate) fn error(uri: &str, failure: &AppError) -> ErrorData {
    let data = is_resource_uri(uri).then(|| json!({ "uri": uri }));
    match failure.code() {
        Some(ErrorCode::NotFound) => {
            ErrorData::resource_not_found(failure.message().to_owned(), data)
        }
        Some(ErrorCode::InvalidArg) => {
            ErrorData::invalid_params(failure.message().to_owned(), data)
        }
        Some(code) => {
            ErrorData::internal_error(format!("[{}] {}", code.as_str(), failure.message()), None)
        }
        // Un appel annulé : personne ne lira la réponse.
        None => ErrorData::internal_error(failure.message().to_owned(), None),
    }
}

#[cfg(test)]
mod tests {
    use lf_application::{
        error::UpstreamCause, input::InputError, messages, ports::CorpusError, tools::ToolName,
    };
    use pretty_assertions::assert_eq;
    use rmcp::model::ErrorCode as RpcCode;

    use super::*;

    fn markdown(text: &str) -> ResourceOutput {
        ResourceOutput {
            kind: ResourceKind::Article,
            content: ResourceContent::Markdown(text.into()),
        }
    }

    #[test]
    fn the_static_resource_is_the_coverage() {
        let listed = list();
        assert_eq!(listed.resources.len(), 1);
        let coverage = &listed.resources[0];
        assert_eq!(coverage.uri, "legifrance://coverage");
        assert_eq!(coverage.name, "coverage");
        assert_eq!(coverage.title.as_deref(), Some("Coverage and known gaps"));
        assert_eq!(coverage.mime_type.as_deref(), Some("application/json"));
        assert!(coverage.description.as_deref().unwrap().contains("fr_coverage"));
        // Une seule page.
        assert_eq!(listed.next_cursor, None);
    }

    #[test]
    fn the_four_templates_are_listed_in_the_documented_order() {
        let listed = list_templates();
        let seen: Vec<(&str, &str, &str, &str)> = listed
            .resource_templates
            .iter()
            .map(|t| {
                (
                    t.uri_template.as_str(),
                    t.name.as_str(),
                    t.title.as_deref().unwrap(),
                    t.mime_type.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "legifrance://article/{article_id}",
                    "article",
                    "Article (full text)",
                    "text/markdown"
                ),
                (
                    "legifrance://decision/{decision_id}",
                    "decision",
                    "Court decision (full text)",
                    "text/markdown"
                ),
                (
                    "legifrance://act/{text_id}",
                    "act",
                    "Law, decree or code (table of contents)",
                    "text/markdown"
                ),
                (
                    "legifrance://code/{code}/article/{num}",
                    "code_article",
                    "Code article by number",
                    "text/markdown"
                ),
            ]
        );
        assert!(listed.resource_templates.iter().all(|t| t.description.is_some()));
        assert_eq!(listed.next_cursor, None);
    }

    #[test]
    fn templates_are_level_one_and_their_names_follow_the_audit_names() {
        for kind in ResourceKind::ALL {
            let described = entry(kind);
            // Niveau 1 de RFC 6570 : `{var}` seulement, ni opérateur ni liste.
            assert!(!described.uri.contains("{+") && !described.uri.contains("{/"));
            assert!(described.uri.starts_with("legifrance://"));
            // `resource:<nom du gabarit>`, le nom de la ligne d'audit (00 §7).
            assert_eq!(kind.audit_name().as_str(), format!("resource:{}", described.name));
            assert!(!described.title.is_empty() && !described.description.is_empty());
        }
    }

    #[test]
    fn the_call_carries_the_uri_and_its_canonical_json() {
        let call = call("legifrance://article/LEGIARTI000006419288");
        assert_eq!(
            call.input.unwrap(),
            ResourceInput { uri: Some("legifrance://article/LEGIARTI000006419288".to_owned()) }
        );
        assert_eq!(
            call.canonical.as_str(),
            "{\"uri\":\"legifrance://article/LEGIARTI000006419288\"}"
        );
        // Une URI hostile garde la forme canonique : ASCII seulement, échappée.
        let hostile = super::call("legifrance://x/\"\u{202e}\n");
        assert_eq!(hostile.canonical.as_str(), "{\"uri\":\"legifrance://x/\\\"\\u202e\\n\"}");
    }

    #[test]
    fn a_markdown_read_has_one_content_of_the_requested_uri() {
        let result = contents("legifrance://article/LEGIARTI000006419288", markdown("# Titre"));

        assert_eq!(result.contents.len(), 1);
        assert_eq!(
            result.contents[0],
            ResourceContents::TextResourceContents {
                uri: "legifrance://article/LEGIARTI000006419288".to_owned(),
                mime_type: Some("text/markdown".to_owned()),
                text: "# Titre".to_owned(),
                meta: None,
            }
        );
        // Les indications de cache sont posées plus haut, selon l'ère.
        assert_eq!((result.ttl_ms, result.cache_scope), (None, None));
    }

    #[test]
    fn the_coverage_is_served_as_the_json_of_the_tool_view() {
        let coverage = lf_application::output::coverage::CoverageOutput {
            coverage: lf_domain::coverage::coverage(
                lf_application::ports::PisteEnvironment::Sandbox,
                &lf_domain::coverage::ConfigFacts {
                    credentials_configured: true,
                    custom_host: false,
                    has_problems: false,
                    local_snapshot: false,
                },
                &lf_domain::fond::Fond::ALL,
            ),
            environment: lf_application::ports::PisteEnvironment::Sandbox,
            configuration: lf_application::output::coverage::CoverageConfiguration {
                credentials_configured: true,
                audit_mode: lf_application::audited::AuditMode::Strict,
                problems: Vec::new(),
                local_snapshot: false,
            },
            consulted_on: lf_domain::date::CivilDate::new(2026, 10, 29).unwrap(),
        };
        let view = CoverageView::from(coverage.clone());

        let result = contents(
            "legifrance://coverage",
            ResourceOutput {
                kind: ResourceKind::Coverage,
                content: ResourceContent::Coverage(coverage),
            },
        );

        let text = serde_json::to_string(&view).unwrap();
        assert_eq!(
            result.contents,
            [ResourceContents::text(text.clone(), "legifrance://coverage")
                .with_mime_type("application/json")]
        );
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["consulted_on"], json!("2026-10-29"));
        assert_eq!(parsed, serde_json::to_value(&view).unwrap());
    }

    // ---- erreurs ----------------------------------------------------------------------------

    #[test]
    fn not_found_is_resource_not_found_with_the_uri() {
        let failure = messages::corpus_error(
            ToolName::FR_GET_TEXT,
            "LEGIARTI000006419999",
            &CorpusError::NotFound { what: "/consult/getArticle".to_owned() },
        );

        let error = error("legifrance://article/LEGIARTI000006419999", &failure);

        assert_eq!(error.code, RpcCode::RESOURCE_NOT_FOUND);
        assert_eq!(error.message, failure.message());
        assert_eq!(error.data, Some(json!({"uri": "legifrance://article/LEGIARTI000006419999"})));
    }

    #[test]
    fn an_invalid_identifier_is_invalid_params_with_the_uri() {
        let failure = messages::required("uri");

        let error = error("legifrance://article/x", &failure);

        assert_eq!(error.code, RpcCode::INVALID_PARAMS);
        assert_eq!(error.message, "uri is required.");
        assert_eq!(error.data, Some(json!({"uri": "legifrance://article/x"})));
    }

    #[test]
    fn a_huge_uri_is_not_echoed() {
        let uri = format!("legifrance://article/{}", "x".repeat(600));
        let failure = messages::required("uri");

        assert_eq!(error(&uri, &failure).data, None);
        // 512 octets exactement : la limite de la grammaire, encore recopiée.
        let edge = format!("legifrance://article/{}", "x".repeat(512 - 21));
        assert_eq!(edge.len(), 512);
        assert_eq!(error(&edge, &failure).data, Some(json!({"uri": edge})));
    }

    #[test]
    fn a_uri_outside_the_grammar_is_never_echoed() {
        let failure = messages::required("uri");

        for uri in ["texte libre de la requete", "https://example.org/x", "legifrance://article/é"]
        {
            let error = error(uri, &failure);
            assert_eq!(error.data, None, "{uri}");
            assert!(!format!("{error:?}").contains("libre"), "{uri}");
        }
        // De la grammaire, même inconnue : recopiée (05 §8.3).
        assert_eq!(
            error("legifrance://inconnue/x", &failure).data,
            Some(json!({"uri": "legifrance://inconnue/x"}))
        );
    }

    #[test]
    fn other_errors_are_internal_errors_with_the_tool_text() {
        let upstream = messages::corpus_error(
            ToolName::FR_GET_TEXT,
            "",
            &CorpusError::Upstream { status: Some(503), retryable: true, detail: "secret".into() },
        );
        let rate_limited = messages::corpus_error(
            ToolName::FR_GET_TEXT,
            "",
            &CorpusError::RateLimited { retry_after: None },
        );

        for failure in [upstream, rate_limited] {
            let code = failure.code().unwrap().as_str();
            let error = error("legifrance://coverage", &failure);
            assert_eq!(error.code, RpcCode::INTERNAL_ERROR);
            assert_eq!(error.message, format!("[{code}] {}", failure.message()));
            assert_eq!(error.data, None);
            assert!(!error.message.contains("secret"));
        }
    }

    #[test]
    fn a_cancelled_read_has_no_code_and_gives_the_bare_message() {
        let cancelled = AppError::Upstream {
            message: "The call was cancelled.".to_owned(),
            cause: UpstreamCause::Cancelled,
        };
        assert_eq!(cancelled.code(), None);

        let error = error("legifrance://coverage", &cancelled);

        assert_eq!(error.code, RpcCode::INTERNAL_ERROR);
        assert_eq!(error.message, cancelled.message());
    }

    #[test]
    fn a_decoding_failure_converts_to_invalid_params() {
        let failure = AppError::from(InputError { message: "uri: bad".to_owned() });
        assert_eq!(error("legifrance://x", &failure).code, RpcCode::INVALID_PARAMS);
    }
}
