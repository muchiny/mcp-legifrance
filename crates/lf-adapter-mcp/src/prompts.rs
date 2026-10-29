//! Prompts MCP (`prompts/list`, `prompts/get`), v0.3 (M7, 05 §8.5, 00 §7).
//!
//! Trois prompts, **en français** (Q14) : ce sont des messages insérés au nom de l'utilisateur
//! francophone. Les noms et les textes sont ceux de 05 §8.5, verrouillés avec le reste de la
//! surface (§9.1).
//!
//! - `analyser_decision(decision_id)` : l'identifiant, d'un seul jeton, est validé comme celui de
//!   `fr_get_decision` ; il n'entre dans le texte qu'une fois reconnu.
//! - `verifier_citations` et `etat_du_droit` n'ont **aucun** argument : Claude Code découpe les
//!   arguments de prompt sur les blancs (05 §1.3), un texte ou une question de plusieurs mots n'y
//!   passerait pas. Leur message demande ce texte ou cette question à l'utilisateur, au tour
//!   suivant.
//!
//! Les prompts ne sont pas des outils : ni cas d'usage ni ligne d'audit. Les erreurs sont des
//! erreurs de protocole (`-32602`, 00 §5) : nom inconnu, argument manquant, invalide ou inattendu.
//! `list_prompts` et `get_prompt` sont écrits à la main, sans `#[prompt_handler]` (05 §8.5) : il
//! poserait `ttlMs: 0` aux pairs modernes, et il n'y a pas de routeur à reconstruire pour trois
//! textes fixes.

use lf_application::messages;
use lf_domain::id::DecisionId;
use rmcp::{
    ErrorData,
    model::{
        GetPromptResult, JsonObject, ListPromptsResult, Prompt, PromptArgument, PromptMessage, Role,
    },
};

/// Nom du prompt d'analyse d'une décision.
pub(crate) const ANALYSER_DECISION: &str = "analyser_decision";

/// Nom du prompt de vérification de citations.
pub(crate) const VERIFIER_CITATIONS: &str = "verifier_citations";

/// Nom du prompt d'état du droit.
pub(crate) const ETAT_DU_DROIT: &str = "etat_du_droit";

/// Nom de l'unique argument de prompt.
const DECISION_ID: &str = "decision_id";

/// Message d'un nom de prompt inconnu. Le nom demandé n'est pas répété.
const UNKNOWN_PROMPT: &str = "unknown prompt";

/// Message d'un argument que le prompt ne définit pas. Son nom n'est pas répété.
const UNEXPECTED_ARGUMENT: &str = "unexpected argument";

const ANALYSER_DECISION_TITLE: &str = "Analyser une décision";

const ANALYSER_DECISION_DESCRIPTION: &str = "Analyser une décision de justice (Cour de cassation, Conseil d'État, Conseil constitutionnel) à partir de son identifiant Légifrance.";

const ANALYSER_DECISION_ARGUMENT: &str =
    "Identifiant Légifrance de la décision (JURITEXT…, CETATEXT… ou CONSTEXT…).";

const VERIFIER_CITATIONS_TITLE: &str = "Vérifier des citations";

const VERIFIER_CITATIONS_DESCRIPTION: &str = "Vérifier les citations juridiques d'un texte contre Légifrance (le texte est demandé au tour suivant).";

const ETAT_DU_DROIT_TITLE: &str = "État du droit";

const ETAT_DU_DROIT_DESCRIPTION: &str = "Établir l'état du droit français en vigueur sur une question, sources officielles à l'appui (la question est demandée au tour suivant).";

/// Message de `verifier_citations` (05 §8.5).
const VERIFIER_CITATIONS_TEXT: &str = "Je vais te donner un texte dont il faut vérifier les citations juridiques. Si je ne l'ai pas encore fourni, demande-le-moi et attends ma réponse.
Vérifie alors ses citations avec `fr_verify_citations`. Pour chaque citation, indique son statut et la référence officielle (`citation`, `source_url`). Pour une citation `not_found`, `invalid` ou `ambiguous`, propose une correction à l'aide de `fr_resolve`, sans rien inventer. Ne modifie pas le reste du texte.";

/// Message d'`etat_du_droit` (05 §8.5).
const ETAT_DU_DROIT_TEXT: &str = "Je vais te poser une question de droit français. Si je ne l'ai pas encore posée, demande-la-moi et attends ma réponse.
Établis ensuite l'état du droit français en vigueur aujourd'hui sur cette question.
Démarche :
1. Repère les textes et la jurisprudence avec `fr_search_all`, puis affine avec `fr_search` sur les fonds pertinents.
2. Lis les articles utiles avec `fr_get_text` et les décisions avec `fr_get_decision` ; vérifie que les versions sont en vigueur (dates de validité).
3. Si un point n'est pas couvert, appelle `fr_coverage` et dis-le explicitement.
Réponds en distinguant la loi et la jurisprudence, avec des citations exactes (`citation` et `source_url`), puis vérifie ta réponse avec `fr_verify_citations` avant de la donner.";

/// `prompts/list` : les trois prompts, `analyser_decision` seul portant un argument.
pub(crate) fn list() -> ListPromptsResult {
    let argument = PromptArgument::new(DECISION_ID)
        .with_description(ANALYSER_DECISION_ARGUMENT)
        .with_required(true);
    ListPromptsResult::with_all_items(vec![
        Prompt::new(ANALYSER_DECISION, Some(ANALYSER_DECISION_DESCRIPTION), Some(vec![argument]))
            .with_title(ANALYSER_DECISION_TITLE),
        Prompt::new(VERIFIER_CITATIONS, Some(VERIFIER_CITATIONS_DESCRIPTION), None)
            .with_title(VERIFIER_CITATIONS_TITLE),
        Prompt::new(ETAT_DU_DROIT, Some(ETAT_DU_DROIT_DESCRIPTION), None)
            .with_title(ETAT_DU_DROIT_TITLE),
    ])
}

/// `prompts/get` : le message du prompt `name`, après validation de ses arguments.
///
/// # Errors
///
/// `-32602` pour un nom inconnu, un argument requis manquant ou invalide, ou un argument que le
/// prompt ne définit pas.
pub(crate) fn get(
    name: &str,
    arguments: Option<&JsonObject>,
) -> Result<GetPromptResult, ErrorData> {
    let empty = JsonObject::new();
    let arguments = arguments.unwrap_or(&empty);
    let (description, text) = match name {
        ANALYSER_DECISION => {
            (ANALYSER_DECISION_DESCRIPTION, analyser_decision(&decision_id(arguments)?))
        }
        VERIFIER_CITATIONS => {
            no_arguments(arguments)?;
            (VERIFIER_CITATIONS_DESCRIPTION, VERIFIER_CITATIONS_TEXT.to_owned())
        }
        ETAT_DU_DROIT => {
            no_arguments(arguments)?;
            (ETAT_DU_DROIT_DESCRIPTION, ETAT_DU_DROIT_TEXT.to_owned())
        }
        _ => return Err(ErrorData::invalid_params(UNKNOWN_PROMPT, None)),
    };
    Ok(GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
        .with_description(description))
}

/// Un prompt sans argument ne reçoit rien.
fn no_arguments(arguments: &JsonObject) -> Result<(), ErrorData> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(UNEXPECTED_ARGUMENT, None))
    }
}

/// L'argument `decision_id` d'`analyser_decision` : une chaîne, requise, reconnue comme un
/// identifiant de décision (la validation de `fr_get_decision`, sans recopier la valeur). Rend la forme
/// normalisée, seule à entrer dans le texte du prompt.
fn decision_id(arguments: &JsonObject) -> Result<DecisionId, ErrorData> {
    if arguments.keys().any(|key| key != DECISION_ID) {
        return Err(ErrorData::invalid_params(UNEXPECTED_ARGUMENT, None));
    }
    let raw = match arguments.get(DECISION_ID) {
        None => return Err(invalid(&messages::required(DECISION_ID))),
        Some(value) => value.as_str().ok_or_else(|| {
            ErrorData::invalid_params(
                "decision_id must be a string (prompt arguments are strings).",
                None,
            )
        })?,
    };
    // Le message ne recopie pas la valeur : rmcp journalise toute erreur de handler, et un
    // argument de prompt est du texte du client.
    DecisionId::parse(raw).map_err(|_| ErrorData::invalid_params(NOT_A_DECISION_ID, None))
}

/// Refus d'un `decision_id` qui n'est pas un identifiant de décision, sans la valeur reçue.
const NOT_A_DECISION_ID: &str = "decision_id is not a decision identifier: it must start with one \
     of ('JURITEXT', 'CONSTEXT', 'CETATEXT') followed by digits.";

/// Erreur `-32602` portant le message rédigé par `lf_application::messages`.
fn invalid(error: &lf_application::error::AppError) -> ErrorData {
    ErrorData::invalid_params(error.message().to_owned(), None)
}

/// Message d'`analyser_decision` (05 §8.5) pour la décision `id`.
fn analyser_decision(id: &DecisionId) -> String {
    let id = id.as_str();
    format!(
        "Analyse la décision Légifrance {id}.
1. Appelle `fr_get_decision` avec decision_id=\"{id}\" pour obtenir ses métadonnées et son résumé ; lis ensuite les passages utiles avec `focus`, ou le texte avec detail=\"full\", si le résumé ne suffit pas.
2. Présente : juridiction et formation, date, numéros, solution, faits et procédure en bref, question de droit, motifs déterminants, portée.
3. Vérifie les textes et décisions que tu cites avec `fr_verify_citations`.
Cite la décision avec sa `citation`, son `ecli` s'il existe et son `source_url`. N'invente aucune référence."
    )
}

#[cfg(test)]
mod tests {
    use lf_application::tools::ToolName;
    use pretty_assertions::assert_eq;
    use rmcp::model::ErrorCode;
    use serde_json::{Value, json};

    use super::*;

    const ID: &str = "JURITEXT000037999394";

    fn arguments(value: &Value) -> JsonObject {
        value.as_object().cloned().unwrap()
    }

    /// Le texte de l'unique message d'un prompt.
    fn text_of(result: &GetPromptResult) -> &str {
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, Role::User);
        &result.messages[0].content.as_text().unwrap().text
    }

    /// Tous les textes visibles du serveur pour les trois prompts : descriptions, titres,
    /// arguments et messages.
    fn every_text() -> Vec<String> {
        let mut texts = Vec::new();
        for prompt in list().prompts {
            texts.extend(prompt.title);
            texts.extend(prompt.description);
            for argument in prompt.arguments.into_iter().flatten() {
                texts.extend(argument.description);
            }
        }
        for name in [ANALYSER_DECISION, VERIFIER_CITATIONS, ETAT_DU_DROIT] {
            let given = json!({"decision_id": ID});
            let result = get(name, (name == ANALYSER_DECISION).then(|| arguments(&given)).as_ref());
            texts.push(text_of(&result.unwrap()).to_owned());
        }
        texts
    }

    #[test]
    fn prompts_are_in_french() {
        let names: Vec<String> = list().prompts.into_iter().map(|prompt| prompt.name).collect();
        assert_eq!(names, ["analyser_decision", "verifier_citations", "etat_du_droit"]);

        let french =
            [" le ", " la ", " les ", " des ", " du ", " de ", " un ", " une ", " à ", " et "];
        let english = [" the ", " of ", " and ", " with ", " for ", " to ", " is ", " are "];
        for text in every_text() {
            let padded = format!(" {} ", text.replace(['\n', ',', '.', ':', ';', '(', ')'], " "));
            let hits = french.iter().filter(|word| padded.contains(**word)).count();
            assert!(hits >= 1, "pas de français dans : {text}");
            for word in english {
                assert!(!padded.contains(word), "mot anglais {word:?} dans : {text}");
            }
        }
        // Les consignes d'étapes parlent à l'utilisateur et au modèle, à l'impératif.
        let given = json!({"decision_id": ID});
        let analysis = get(ANALYSER_DECISION, Some(&arguments(&given))).unwrap();
        assert!(
            text_of(&analysis).starts_with("Analyse la décision Légifrance JURITEXT000037999394.")
        );
        assert!(
            text_of(&get(VERIFIER_CITATIONS, None).unwrap())
                .starts_with("Je vais te donner un texte")
        );
        assert!(
            text_of(&get(ETAT_DU_DROIT, None).unwrap())
                .starts_with("Je vais te poser une question")
        );
    }

    #[test]
    fn prompts_without_free_text_arguments() {
        let listed = list().prompts;
        let described: Vec<(&str, Vec<&str>)> = listed
            .iter()
            .map(|prompt| {
                let arguments = prompt.arguments.iter().flatten();
                (prompt.name.as_str(), arguments.map(|a| a.name.as_str()).collect())
            })
            .collect();
        assert_eq!(
            described,
            [
                ("analyser_decision", vec!["decision_id"]),
                ("verifier_citations", vec![]),
                ("etat_du_droit", vec![]),
            ]
        );
        // `decision_id` est requis, et sa description dit qu'il tient en un seul jeton.
        let argument = &listed[0].arguments.as_ref().unwrap()[0];
        assert_eq!(argument.required, Some(true));
        assert!(argument.description.as_deref().unwrap().contains("Identifiant Légifrance"));
        // Les deux autres n'annoncent pas même une liste vide.
        assert!(listed[1].arguments.is_none() && listed[2].arguments.is_none());
    }

    #[test]
    fn the_listed_texts_are_the_documented_ones() {
        let listed = list();
        assert_eq!(listed.next_cursor, None);
        let titles: Vec<&str> =
            listed.prompts.iter().map(|prompt| prompt.title.as_deref().unwrap()).collect();
        assert_eq!(titles, ["Analyser une décision", "Vérifier des citations", "État du droit"]);
        assert_eq!(
            listed.prompts[0].description.as_deref(),
            Some(
                "Analyser une décision de justice (Cour de cassation, Conseil d'État, Conseil constitutionnel) à partir de son identifiant Légifrance."
            )
        );
        assert_eq!(
            listed.prompts[1].description.as_deref(),
            Some(
                "Vérifier les citations juridiques d'un texte contre Légifrance (le texte est demandé au tour suivant)."
            )
        );
        assert_eq!(
            listed.prompts[2].description.as_deref(),
            Some(
                "Établir l'état du droit français en vigueur sur une question, sources officielles à l'appui (la question est demandée au tour suivant)."
            )
        );
    }

    #[test]
    fn analyser_decision_puts_the_validated_identifier_in_the_text() {
        let given = json!({"decision_id": ID});

        let result = get(ANALYSER_DECISION, Some(&arguments(&given))).unwrap();

        let text = text_of(&result);
        assert_eq!(text.matches(ID).count(), 2, "{text}");
        assert!(text.contains(&format!("decision_id=\"{ID}\"")), "{text}");
        assert!(text.ends_with("N'invente aucune référence."), "{text}");
        assert_eq!(text.lines().count(), 5, "{text}");
        assert_eq!(result.description.as_deref(), list().prompts[0].description.as_deref());
    }

    #[test]
    fn the_other_two_prompts_ask_for_their_input_on_the_next_turn() {
        for name in [VERIFIER_CITATIONS, ETAT_DU_DROIT] {
            for given in [None, Some(JsonObject::new())] {
                let result = get(name, given.as_ref()).unwrap();
                let text = text_of(&result);
                assert!(text.contains("demande-"), "{text}");
                assert!(text.contains("attends ma réponse"), "{text}");
            }
        }
        let etat = get(ETAT_DU_DROIT, None).unwrap();
        assert_eq!(text_of(&etat).lines().count(), 7);
        let verifier = get(VERIFIER_CITATIONS, None).unwrap();
        assert_eq!(text_of(&verifier).lines().count(), 2);
    }

    #[test]
    fn every_tool_quoted_in_a_prompt_exists() {
        // Test de dérive (05 §4.3) : un outil renommé ou retiré casse le prompt qui le cite.
        // Les treize outils de la surface complète.
        let catalog: Vec<&str> = ToolName::ALL.iter().map(|tool| tool.as_str()).collect();
        let mut quoted = 0;
        for text in every_text() {
            for word in text.split('`').skip(1).step_by(2) {
                if word.starts_with("fr_") {
                    assert!(catalog.contains(&word), "{word} n'est pas un outil du catalogue");
                    quoted += 1;
                }
            }
        }
        assert!(quoted >= 10, "{quoted}");
    }

    #[test]
    fn an_unknown_prompt_is_invalid_params_without_echoing_its_name() {
        for name in ["", "anything", "ANALYSER_DECISION", "analyser_decision ", "fr_search"] {
            let error = get(name, None).unwrap_err();
            assert_eq!(error.code, ErrorCode::INVALID_PARAMS, "{name:?}");
            assert_eq!(error.message, "unknown prompt");
            assert_eq!(error.data, None);
        }
    }

    #[test]
    fn analyser_decision_requires_a_valid_decision_id() {
        let invalid_cases = [
            (None, "decision_id is required."),
            (Some(json!({})), "decision_id is required."),
            (Some(json!({"decision_id": 7})), "decision_id must be a string"),
            (Some(json!({"decision_id": null})), "decision_id must be a string"),
            (Some(json!({"decision_id": "not-an-id"})), "decision_id is not a decision identifier"),
            (
                Some(json!({"decision_id": "LEGIARTI000006419288"})),
                "decision_id is not a decision identifier",
            ),
            (Some(json!({"decision_id": ""})), "decision_id is not a decision identifier"),
            (Some(json!({"decision_id": ID, "extra": "x"})), "unexpected argument"),
            (Some(json!({"other": ID})), "unexpected argument"),
        ];
        for (given, expected) in invalid_cases {
            let object = given.as_ref().map(arguments);
            let error = get(ANALYSER_DECISION, object.as_ref()).unwrap_err();
            assert_eq!(error.code, ErrorCode::INVALID_PARAMS, "{given:?}");
            assert!(error.message.contains(expected), "{given:?}: {}", error.message);
            assert_eq!(error.data, None);
            // Jamais la valeur reçue : rmcp journalise l'erreur.
            if let Some(Value::String(value)) = given.as_ref().and_then(|v| v.get("decision_id")) {
                assert!(value.is_empty() || !error.message.contains(value.as_str()), "{given:?}");
            }
        }
    }

    #[test]
    fn a_prompt_without_arguments_refuses_any() {
        for name in [VERIFIER_CITATIONS, ETAT_DU_DROIT] {
            for given in [json!({"text": "x"}), json!({"decision_id": ID})] {
                let error = get(name, Some(&arguments(&given))).unwrap_err();
                assert_eq!(error.code, ErrorCode::INVALID_PARAMS, "{name} {given}");
                assert_eq!(error.message, "unexpected argument");
            }
        }
    }
}
