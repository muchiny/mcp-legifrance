//! Côté entrant : le seul contrat d'entrée de l'application est [`ToolCall`] (02 §5.4).

use std::{fmt, sync::Arc};

/// Un appel d'outil tel que reçu : l'adapter entrant a tenté le décodage, mais c'est
/// l'application qui audite le résultat. Une erreur de décodage devient donc un `invalid_arg`
/// audité (E-29, D-02).
///
/// Son `Debug` ne montre aucun argument (02 §11.3, F-4) : ni l'entrée décodée, ni le message de
/// décodage (qui les cite), seulement l'issue du décodage et la longueur du JSON canonique.
#[derive(Clone)]
pub struct ToolCall<I> {
    /// L'entrée décodée, ou la raison pour laquelle elle ne l'a pas été.
    pub input: Result<I, InputError>,
    /// Les arguments bruts en JSON canonique, pour l'audit (calcul de `input_hash`).
    pub canonical: CanonicalInput,
}

impl<I> fmt::Debug for ToolCall<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let input = match self.input {
            Ok(_) => "Ok(<decoded>)",
            Err(_) => "Err(<decoding error>)",
        };
        f.debug_struct("ToolCall")
            .field("input", &format_args!("{input}"))
            .field("canonical", &self.canonical)
            .finish()
    }
}

/// JSON canonique de l'objet `arguments` brut. Le clone ne copie pas le texte : l'appel, la
/// ligne d'audit et la trace d'un même appel le partagent.
///
/// Son `Debug` n'en donne que la longueur en octets, `CanonicalInput(<N bytes>)` : `CallMeta`,
/// `AuditRecord` et `ToolCall`, qui le portent, peuvent finir dans un journal (02 §11.3, F-4).
#[derive(Clone)]
pub struct CanonicalInput(Arc<str>);

impl fmt::Debug for CanonicalInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CanonicalInput(<{} bytes>)", self.0.len())
    }
}

impl CanonicalInput {
    /// Enveloppe un texte déjà canonique ; la canonicalisation est faite par l'adapter entrant.
    #[must_use]
    pub fn new(text: impl Into<Arc<str>>) -> Self {
        Self(text.into())
    }

    /// Le texte JSON.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Échec de décodage des arguments, ex. `page_size: invalid type: string "dix", expected u32`.
/// Le message est celui que le cas d'usage rend au modèle en `invalid_arg`.
#[derive(Debug, Clone)]
pub struct InputError {
    /// Message de décodage, sans valeur secrète : il cite les arguments du modèle, rien d'autre.
    pub message: String,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

// ---------------------------------------------------------------------------------------------
// Entrées décodées des outils (05 §3 à §5). Chaque champ reprend le nom du paramètre MCP, en
// `Option<String>` ou `Option<i64>` (entier tolérant de 05 §3.3) : l'adapter entrant a décodé la
// forme, c'est le cas d'usage qui valide les valeurs, dans l'ordre de la référence.
// ---------------------------------------------------------------------------------------------

/// Arguments de `fr_search` (05 §5.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchInput {
    /// Texte de la requête.
    pub query: Option<String>,
    /// Fond interrogé, `LODA` par défaut.
    pub fond: Option<String>,
    /// Mode de correspondance (`match` côté MCP) : `any`, `all` ou `exact`.
    pub match_mode: Option<String>,
    /// Numéro de page, à partir de 1.
    pub page: Option<i64>,
    /// Résultats par page, de 1 à 100.
    pub page_size: Option<i64>,
    /// Début de période, `AAAA-MM-JJ`.
    pub date_from: Option<String>,
    /// Fin de période, `AAAA-MM-JJ`.
    pub date_to: Option<String>,
    /// Date de version pour `LODA` et `CODE`, `AAAA-MM-JJ`.
    pub version_date: Option<String>,
}

/// Arguments de `fr_get_act` (05 §5.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetActInput {
    /// Identifiant `LEGITEXT`, suffixe de version admis.
    pub text_id: Option<String>,
    /// Date de version, `AAAA-MM-JJ`.
    pub date: Option<String>,
    /// Début de la fenêtre de table des matières.
    pub toc_offset: Option<i64>,
    /// Taille de la fenêtre de table des matières.
    pub toc_limit: Option<i64>,
}

/// Paramètres de lecture communs aux trois lectures de texte long (05 §5.0).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadingInput {
    /// Niveau de détail : `summary` ou `full`.
    pub detail: Option<String>,
    /// Position de départ, en caractères, pour `detail="full"`.
    pub offset: Option<i64>,
    /// Plafond de caractères du texte rendu.
    pub max_chars: Option<i64>,
    /// Mots à retrouver dans le texte (mode passages).
    pub focus: Option<String>,
    /// Nombre de passages rendus en mode passages.
    pub passages: Option<i64>,
}

/// Arguments de `fr_get_text` (05 §5.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetTextInput {
    /// Identifiant `LEGIARTI` ou `KALIARTI`.
    pub article_id: Option<String>,
    /// Paramètres de lecture.
    pub reading: ReadingInput,
}

/// Arguments de `fr_get_decision` (05 §5.4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetDecisionInput {
    /// Identifiant `JURITEXT`, `CONSTEXT` ou `CETATEXT`.
    pub decision_id: Option<String>,
    /// Paramètres de lecture.
    pub reading: ReadingInput,
}

/// Arguments de `fr_get_deliberation` (05 §5.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetDeliberationInput {
    /// Identifiant `CNILTEXT`.
    pub deliberation_id: Option<String>,
    /// Paramètres de lecture.
    pub reading: ReadingInput,
}

/// Arguments de `fr_get_convention` (05 §5.6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetConventionInput {
    /// Identifiant `KALITEXT`.
    pub convention_id: Option<String>,
    /// Début de la fenêtre de table des matières.
    pub toc_offset: Option<i64>,
    /// Taille de la fenêtre de table des matières.
    pub toc_limit: Option<i64>,
}

/// Arguments de `fr_get_company_agreement` (05 §5.7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetCompanyAgreementInput {
    /// Identifiant `ACCOTEXT`.
    pub agreement_id: Option<String>,
}

/// Arguments de `fr_coverage` : aucun (05 §5.8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageInput;

/// Arguments de `fr_verify_citations` (05 §5.9).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifyCitationsInput {
    /// Texte dont on contrôle les citations.
    pub text: Option<String>,
    /// Nombre maximal de citations contrôlées, de 1 à 100.
    pub max_citations: Option<i64>,
}

/// Arguments de `fr_resolve` (05 §5.10).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolveInput {
    /// Citation libre à résoudre.
    pub citation: Option<String>,
    /// Date de la version voulue, `AAAA-MM-JJ`.
    pub date: Option<String>,
}

/// Arguments de `fr_get_related` (05 §5.11).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GetRelatedInput {
    /// Identifiant d'un article ou d'une décision.
    pub id: Option<String>,
    /// Sens des seuls liens demandés : `cites`, `cited_by`, `cites_case_law` ou `applies_text`
    /// (insensible à la casse) ; tous les liens quand il est absent.
    pub relation: Option<String>,
}

/// Arguments de `fr_diff_article` (05 §5.12).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffArticleInput {
    /// Identifiant de l'article.
    pub article_id: Option<String>,
    /// Première date, `AAAA-MM-JJ`.
    pub date_a: Option<String>,
    /// Seconde date, `AAAA-MM-JJ`, aujourd'hui (Paris) par défaut.
    pub date_b: Option<String>,
}

/// Arguments de `fr_search_all` (05 §5.13).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchAllInput {
    /// Texte de la requête.
    pub query: Option<String>,
    /// Fonds interrogés ; cinq fonds par défaut.
    pub fonds: Option<Vec<String>>,
    /// Résultats par fond, de 1 à 10.
    pub per_fond: Option<i64>,
}

/// Lecture d'une resource MCP (05 §8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceInput {
    /// URI demandée, `legifrance://...`.
    pub uri: Option<String>,
}

/// Demande de complétion d'une variable de gabarit (05 §8.4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompleteInput {
    /// Nom de la variable à compléter (`code`).
    pub argument: Option<String>,
    /// Début de valeur déjà saisi.
    pub value: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    #[test]
    fn canonical_input_clones_share_the_text() {
        let input = CanonicalInput::new(r#"{"query":"bail"}"#);
        let clone = input.clone();
        assert_eq!(clone.as_str(), r#"{"query":"bail"}"#);
        assert!(Arc::ptr_eq(&input.0, &clone.0));
        let shared: Arc<str> = Arc::from("{}");
        assert!(Arc::ptr_eq(&CanonicalInput::new(Arc::clone(&shared)).0, &shared));
    }

    #[test]
    fn input_error_displays_its_message() {
        let error = InputError { message: "page_size: invalid type".to_owned() };
        assert_eq!(error.to_string(), "page_size: invalid type");
    }

    /// Argument témoin : il ne doit apparaître dans aucun `Debug` (journaux, F-4).
    const SENTINEL: &str = "sentinel-argument-value";

    #[test]
    fn canonical_input_debug_shows_only_its_length_in_bytes() {
        // 26 octets pour 24 caractères : la longueur est celle du texte JSON, en octets.
        let input = CanonicalInput::new(r#"{"query":"bail résilié"}"#);
        assert_eq!(format!("{input:?}"), "CanonicalInput(<26 bytes>)");
        let input = CanonicalInput::new(format!(r#"{{"query":"{SENTINEL}"}}"#));
        assert_eq!(format!("{input:?}"), "CanonicalInput(<35 bytes>)");
        assert_eq!(format!("{:?}", CanonicalInput::new("")), "CanonicalInput(<0 bytes>)");
    }

    #[test]
    fn tool_call_debug_never_shows_the_arguments() {
        let canonical = CanonicalInput::new(format!(r#"{{"query":"{SENTINEL}"}}"#));
        let decoded: ToolCall<String> =
            ToolCall { input: Ok(SENTINEL.to_owned()), canonical: canonical.clone() };
        let rejected: ToolCall<String> = ToolCall {
            input: Err(InputError { message: format!("query: invalid value '{SENTINEL}'") }),
            canonical,
        };
        assert_eq!(
            format!("{decoded:?}"),
            "ToolCall { input: Ok(<decoded>), canonical: CanonicalInput(<35 bytes>) }"
        );
        assert_eq!(
            format!("{rejected:?}"),
            "ToolCall { input: Err(<decoding error>), canonical: CanonicalInput(<35 bytes>) }"
        );
        for rendered in [format!("{decoded:?}"), format!("{rejected:?}"), format!("{decoded:#?}")] {
            assert!(!rendered.contains(SENTINEL), "{rendered}");
        }
    }

    #[test]
    fn tool_call_carries_the_decoding_result_and_the_canonical_text() {
        let call: ToolCall<u32> = ToolCall {
            input: Err(InputError { message: "bad".to_owned() }),
            canonical: CanonicalInput::new("{}"),
        };
        assert_eq!(call.input.unwrap_err().message, "bad");
        assert_eq!(call.canonical.as_str(), "{}");
    }

    #[test]
    fn tool_inputs_default_to_absent_arguments() {
        // Un objet `arguments` vide : tout est absent, la validation est celle du cas d'usage.
        assert_eq!(SearchInput::default().query, None);
        assert_eq!(GetActInput::default().toc_limit, None);
        assert_eq!(ReadingInput::default(), ReadingInput { detail: None, ..Default::default() });
        let text = GetTextInput {
            article_id: Some("LEGIARTI000006419288".to_owned()),
            ..Default::default()
        };
        assert_eq!(text.reading, ReadingInput::default());
        assert_eq!(format!("{CoverageInput:?}"), "CoverageInput");
        assert_eq!(SearchAllInput::default().fonds, None);
        assert_eq!(CompleteInput::default(), CompleteInput { argument: None, value: None });
        // Clone et égalité : un test de cas d'usage compare ce qu'il a passé à ce qu'il a reçu.
        let input = ResolveInput { citation: Some("art. 9".to_owned()), date: None };
        assert_eq!(input.clone(), input);
        assert_ne!(input, ResolveInput::default());
    }
}
