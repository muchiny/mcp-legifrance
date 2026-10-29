//! Complétions MCP (`completion/complete`), v0.3 (M7, 05 §8.4, 00 §7).
//!
//! Seule la variable `code` du gabarit `legifrance://code/{code}/article/{num}` est complétée :
//! les slugs de codes, `list_codes()` en source (cache de 7 jours côté adapter de cache), 100
//! valeurs au plus avec `total` et `hasMore`. Tout le reste donne une liste vide : les autres
//! variables de gabarit (identifiants non énumérables), tout prompt et toute URI inconnue.
//!
//! Le débit est limité à 10 complétions par seconde et par processus (MUST, `completion.mdx:207`),
//! par `LegalService::complete`. Une erreur amont ou un débit dépassé donnent aussi la liste vide,
//! jamais une erreur JSON-RPC (02 §9.2 ligne 27). Les complétions ne sont pas auditées (00 §7).

use lf_application::{input::CompleteInput, output::complete::CompleteOutput};
use rmcp::model::{CompleteRequestParams, CompleteResult, CompletionInfo, Reference};

/// Le gabarit dont la variable `code` est complétée.
pub(crate) const CODE_TEMPLATE: &str = "legifrance://code/{code}/article/{num}";

/// La demande de complétion qui concerne le service : `None` pour tout ce qui n'est pas une
/// variable d'un gabarit de resource, ou une variable d'un autre gabarit que celui des codes
/// (liste vide, sans passer par le service ni son limiteur).
pub(crate) fn input(request: &CompleteRequestParams) -> Option<CompleteInput> {
    match &request.r#ref {
        Reference::Resource(template) if template.uri == CODE_TEMPLATE => Some(CompleteInput {
            argument: Some(request.argument.name.clone()),
            value: Some(request.argument.value.clone()),
        }),
        // Un autre gabarit, un prompt, ou une référence qu'une version de rmcp ajouterait.
        _ => None,
    }
}

/// Le résultat de `completion/complete` : au plus 100 valeurs (`CompletionInfo::MAX_VALUES`) ;
/// `total` et `hasMore` seulement quand la liste est une vraie réponse. Une liste vide sans
/// réponse (variable inconnue, débit dépassé, erreur amont) n'annonce ni l'un ni l'autre.
pub(crate) fn result(output: CompleteOutput) -> CompleteResult {
    let mut values = output.values;
    values.truncate(CompletionInfo::MAX_VALUES);
    // `new` ne refuse que plus de 100 valeurs, que `truncate` vient d'exclure.
    let mut completion = CompletionInfo::new(values).unwrap_or_default();
    completion.total = output.total.map(|total| u32::try_from(total).unwrap_or(u32::MAX));
    completion.has_more = output.total.map(|_| output.has_more);
    CompleteResult::new(completion)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rmcp::model::ArgumentInfo;

    use super::*;

    fn request(reference: Reference, name: &str, value: &str) -> CompleteRequestParams {
        CompleteRequestParams::new(reference, ArgumentInfo::new(name, value))
    }

    #[test]
    fn completion_only_for_code_variable() {
        // Le gabarit des codes : la variable et la saisie passent telles quelles au service ;
        // c'est lui qui ne complète que `code`.
        let code = request(Reference::for_resource(CODE_TEMPLATE), "code", "code-c");
        assert_eq!(
            input(&code),
            Some(CompleteInput {
                argument: Some("code".to_owned()),
                value: Some("code-c".to_owned())
            })
        );
        let num = request(Reference::for_resource(CODE_TEMPLATE), "num", "9");
        assert_eq!(input(&num).unwrap().argument.as_deref(), Some("num"));

        // Les autres gabarits, une URI inconnue et tout prompt : jamais de complétion.
        for uri in [
            "legifrance://article/{article_id}",
            "legifrance://decision/{decision_id}",
            "legifrance://act/{text_id}",
            "legifrance://coverage",
            "legifrance://code/code-civil/article/9",
            "legifrance://code/{code}/article/{num}/",
            "file:///etc/passwd",
            "",
        ] {
            assert_eq!(input(&request(Reference::for_resource(uri), "code", "c")), None, "{uri}");
        }
        for prompt in ["analyser_decision", "verifier_citations", "etat_du_droit", "code"] {
            assert_eq!(
                input(&request(Reference::for_prompt(prompt), "code", "c")),
                None,
                "{prompt}"
            );
        }
    }

    #[test]
    fn the_result_carries_the_values_total_and_more() {
        let output = CompleteOutput {
            values: vec!["code-civil".to_owned(), "code-du-travail".to_owned()],
            total: Some(250),
            has_more: true,
        };

        let completed = result(output).completion;

        assert_eq!(completed.values, ["code-civil", "code-du-travail"]);
        assert_eq!(completed.total, Some(250));
        assert_eq!(completed.has_more, Some(true));
    }

    #[test]
    fn an_empty_answer_announces_nothing() {
        let completed = result(CompleteOutput::empty()).completion;
        assert_eq!(completed, CompletionInfo::default());
        assert_eq!((completed.total, completed.has_more), (None, None));

        // Une vraie réponse sans correspondance : zéro, et rien d'autre.
        let none = CompleteOutput { values: Vec::new(), total: Some(0), has_more: false };
        let completed = result(none).completion;
        assert_eq!((completed.total, completed.has_more), (Some(0), Some(false)));
    }

    #[test]
    fn at_most_one_hundred_values_ever_leave_the_adapter() {
        let many = CompleteOutput {
            values: (0..150).map(|n| format!("code-{n}")).collect(),
            total: Some(150),
            has_more: true,
        };

        let completed = result(many).completion;

        assert_eq!(completed.values.len(), CompletionInfo::MAX_VALUES);
        assert_eq!(completed.values[99], "code-99");
        completed.validate().unwrap();
    }

    #[test]
    fn a_total_beyond_u32_saturates() {
        let huge = CompleteOutput { values: Vec::new(), total: Some(usize::MAX), has_more: true };
        let max = u32::MAX;
        assert_eq!(result(huge).completion.total, Some(max));
    }
}
