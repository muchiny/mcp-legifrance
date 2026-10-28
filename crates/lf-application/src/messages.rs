//! Textes d'erreur vus par le modèle : source unique de 05 §7.2 à §7.4 (02 §9.3).
//!
//! Chaque fonction est pure. `corpus_error` n'appelle jamais `Display` sur une `CorpusError` :
//! le message est rédigé à partir des seuls champs typés (variante, statut, `AuthOrigin`,
//! `ConfigKind`).

use std::time::Duration;

use crate::{
    error::{AppError, ErrorCode, UpstreamCause},
    ports::{
        audit::AuditError,
        corpus::{AuthOrigin, ConfigKind, CorpusError},
    },
    tools::ToolName,
};

/// Longueur maximale, en caractères, du détail d'une `InvalidRequest` recopié dans le message
/// (05 §7.2 : le message PISTE nettoyé par l'adapter, 200 caractères au plus).
const INVALID_REQUEST_DETAIL_MAX: usize = 200;

/// Longueur maximale, en caractères, du détail d'un `ConfigKind::Settings` (texte du modèle,
/// 500 caractères comme le champ `error` de l'audit, 00 §7).
const SETTINGS_DETAIL_MAX: usize = 500;

/// Message du `Decode`, repris pour un `TooLarge` hors `fr_get_act` et pour un `NotFound`
/// qu'un outil de recherche ne devrait jamais recevoir (E-21).
const DECODE_MESSAGE: &str = "unexpected response from Legifrance; report it.";

/// Message d'un `TooLarge` sur `fr_get_act` (C2, 04 §7.3) : un code entier, à lire par article.
const TOO_LARGE_FOR_GET_ACT: &str =
    "this text is too large for get_act; use get_article or search instead.";

/// Message d'un refus d'identifiants (sans le statut, que `credentials_rejected` ajoute).
const CREDENTIALS_REJECTED_HINT: &str =
    ": check LF_PISTE_CLIENT_ID, LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV.";

/// Coupe `message` à `max_chars` caractères (points de code), sans jamais scinder un caractère
/// multi-octets. Pas de points de suspension : la coupe est une borne de sûreté, pas un résumé.
#[must_use]
pub fn clip(message: &str, max_chars: usize) -> String {
    message.chars().take(max_chars).collect()
}

/// Secondes arrondies au supérieur : une attente annoncée ne doit jamais être trop courte.
fn ceil_secs(duration: Duration) -> u64 {
    duration.as_secs().saturating_add(u64::from(duration.subsec_nanos() > 0))
}

/// Erreur d'argument : le message est déjà celui du modèle (domaine, 03 §2.7, ou décodage).
#[must_use]
pub fn invalid_argument(message: impl Into<String>) -> AppError {
    AppError::invalid_argument(message)
}

/// Échéance globale dépassée (02 §9.2 ligne 20) ; `budget` est `LF_TOOL_DEADLINE_S`.
#[must_use]
pub fn deadline(budget: Duration) -> AppError {
    AppError::Upstream {
        message: format!(
            "tool deadline of {} s exceeded; retry, or narrow the request (smaller page_size, \
             fewer fonds).",
            ceil_secs(budget)
        ),
        cause: UpstreamCause::Deadline,
    }
}

/// Appel annulé par le client (ligne 21) : le message ne va qu'à l'audit, rmcp jette la réponse.
#[must_use]
pub fn cancelled() -> AppError {
    AppError::Upstream {
        message: "cancelled by client".to_owned(),
        cause: UpstreamCause::Cancelled,
    }
}

/// Échec d'écriture du journal en mode strict (ligne 22). Le paramètre `_error` n'est volontairement
/// pas lu : son `detail` peut citer un chemin local (00 §5, D-40) et va aux journaux de l'appelant,
/// jamais au modèle. Le garder dans la signature interdit d'appeler cette fonction sans échec réel.
#[must_use]
pub fn audit(_error: &AuditError, original: Option<ErrorCode>) -> AppError {
    let mut message = String::from(
        "audit log unavailable; the call was not recorded and its result is withheld (see \
         server logs).",
    );
    if let Some(code) = original {
        message.push_str(" Original error: [");
        message.push_str(code.as_str());
        message.push_str("].");
    }
    AppError::Audit { message, original }
}

/// Traduit une erreur du corpus en `AppError` (02 §9.2, 05 §7.2 à §7.4).
///
/// `subject` est l'identifiant demandé, cité tel quel par les messages `not_found` (vide pour
/// `fr_search`). Les `match` sont exhaustifs, sans bras `_` sur `CorpusError`, `AuthOrigin` ni
/// `ConfigKind` : une variante ajoutée casse la compilation. Le `detail` d'une `CorpusError` n'est
/// recopié que pour `InvalidRequest` (déjà nettoyé par l'adapter, coupé) et `Config { Settings }`
/// (texte rédigé pour être lu) ; aucun `Display` de `CorpusError` n'est appelé (celui de `Config`
/// recopierait le détail, donc l'URL du jeton).
#[must_use]
pub fn corpus_error(tool: ToolName, subject: &str, error: &CorpusError) -> AppError {
    match error {
        CorpusError::NotFound { what: _ } => not_found(tool, subject),
        CorpusError::InvalidRequest { status, detail } => invalid_request(*status, detail),
        CorpusError::Upstream { status, retryable, detail: _ } => upstream(*status, *retryable),
        CorpusError::Timeout => AppError::Upstream {
            message: format!("Legifrance/PISTE did not answer in time{}", retry_suffix(true)),
            cause: UpstreamCause::Timeout,
        },
        CorpusError::RateLimited { retry_after } => rate_limited(*retry_after),
        CorpusError::Auth { origin, status, detail: _ } => auth(*origin, *status),
        CorpusError::Config { kind, detail } => config(*kind, detail),
        CorpusError::Decode { detail: _ } => decode(),
        CorpusError::TooLarge { detail: _ } => match tool {
            ToolName::FR_GET_ACT => AppError::Upstream {
                message: TOO_LARGE_FOR_GET_ACT.to_owned(),
                cause: UpstreamCause::Decode,
            },
            _ => decode(),
        },
    }
}

/// `Decode` (ligne 19) : non réessayable, donc sans invitation à réessayer (00 §8).
fn decode() -> AppError {
    AppError::Upstream { message: DECODE_MESSAGE.to_owned(), cause: UpstreamCause::Decode }
}

/// Message `not_found` de l'outil (05 §7.4). `fr_search` et `fr_coverage` ne peuvent pas le
/// recevoir (04 §11.2) : repli défensif sur le message de `Decode`, jamais une panique.
fn not_found(tool: ToolName, subject: &str) -> AppError {
    let message = match tool {
        ToolName::FR_GET_ACT => format!(
            "No LODA text found for {subject}. Check the id, or search with fr_search fond=LODA \
             or CODE."
        ),
        ToolName::FR_GET_TEXT => format!(
            "No article found for {subject}. Check the id, or find the article with fr_get_act or \
             fr_search."
        ),
        ToolName::FR_GET_DECISION => format!(
            "No {} decision found for {subject}. Check the id, or search with fr_search.",
            decision_fond(subject)
        ),
        ToolName::FR_GET_DELIBERATION => format!(
            "No CNIL deliberation found for {subject}. Check the id, or search with fr_search."
        ),
        ToolName::FR_GET_CONVENTION => format!(
            "No collective agreement text found for {subject}. Check the id, or search with \
             fr_search."
        ),
        ToolName::FR_GET_COMPANY_AGREEMENT => format!(
            "No company agreement found for {subject}. Check the id, or search with fr_search."
        ),
        _ => return decode(),
    };
    AppError::not_found(message)
}

/// Fond nommé d'après le préfixe de l'identifiant (E-15) ; tout autre préfixe, y compris vide,
/// retombe sur `JURI` (identifiant déjà validé par le domaine : ce repli reste défensif).
fn decision_fond(subject: &str) -> &'static str {
    if subject.starts_with("CONSTEXT") {
        "CONSTIT"
    } else if subject.starts_with("CETATEXT") {
        "CETAT"
    } else {
        "JURI"
    }
}

/// `InvalidRequest` (ligne 15) : `Some(s)` cite le statut reçu (400, ou 403 de la passerelle) ;
/// `None` est une garde locale, aucune requête n'est partie, donc pas de « HTTP » (07 T-M2-09).
fn invalid_request(status: Option<u16>, detail: &str) -> AppError {
    let detail = clip(detail, INVALID_REQUEST_DETAIL_MAX);
    match status {
        Some(status) => AppError::Upstream {
            message: format!("Legifrance rejected the request (HTTP {status}): {detail}"),
            cause: UpstreamCause::Status(status),
        },
        None => AppError::Upstream {
            message: format!("the request was not sent to Legifrance ({detail}); report it."),
            cause: UpstreamCause::Decode,
        },
    }
}

/// Suffixe d'un échec amont : l'invitation à réessayer n'apparaît que si `retryable` (00 §8).
const fn retry_suffix(retryable: bool) -> &'static str {
    if retryable { "; retry once, then report it." } else { "; report it." }
}

/// `Upstream` (lignes 17 et 18). Sans statut, c'est une erreur réseau : la catégorie reste fixe,
/// `CorpusError::Upstream` ne la porte pas (05 §7.2 cite `TLS error` en exemple).
fn upstream(status: Option<u16>, retryable: bool) -> AppError {
    let suffix = retry_suffix(retryable);
    match status {
        Some(status) => {
            let reason = match status {
                500 => " (internal server error)",
                502 => " (bad gateway)",
                503 => " (service unavailable)",
                504 => " (gateway timeout)",
                _ => "",
            };
            AppError::Upstream {
                message: format!("Legifrance/PISTE HTTP {status}{reason}{suffix}"),
                cause: UpstreamCause::Status(status),
            }
        }
        None => AppError::Upstream {
            message: format!("could not reach Legifrance/PISTE (connection failed){suffix}"),
            cause: UpstreamCause::Network,
        },
    }
}

/// `RateLimited` (ligne 16) : secondes arrondies au supérieur, ou « a few seconds » sans
/// `Retry-After` (V-63).
fn rate_limited(retry_after: Option<Duration>) -> AppError {
    let message = match retry_after {
        Some(delay) => format!("PISTE quota reached; retry in {} s.", ceil_secs(delay)),
        None => "PISTE quota reached; retry in a few seconds.".to_owned(),
    };
    AppError::RateLimited { message, retry_after }
}

/// `Auth` (lignes 7, 11, 12) : seul un 403 de l'API dit « accès refusé » (application non
/// abonnée) ; tout autre refus, jeton compris, dit « identifiants refusés ».
fn auth(origin: AuthOrigin, status: Option<u16>) -> AppError {
    let message = match (origin, status) {
        (AuthOrigin::Api, Some(403)) => "PISTE denied access (HTTP 403): check that the PISTE \
                                         application is subscribed to the Legifrance API."
            .to_owned(),
        (AuthOrigin::Token | AuthOrigin::Api, Some(status)) => {
            format!("PISTE rejected the credentials (HTTP {status}){CREDENTIALS_REJECTED_HINT}")
        }
        (AuthOrigin::Token | AuthOrigin::Api, None) => {
            format!("PISTE rejected the credentials{CREDENTIALS_REJECTED_HINT}")
        }
    };
    AppError::config(message)
}

/// `Config` (lignes 5, 6, 8, 9, 14) : un message fixe par nature. Seul `Settings` recopie son
/// `detail`, texte rédigé pour être lu (`ConfigProblem`, ou erreur de construction du client).
fn config(kind: ConfigKind, detail: &str) -> AppError {
    let message = match kind {
        ConfigKind::CredentialsMissing => {
            "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (from \
             your piste.gouv.fr application, section OAuth Credentials)."
                .to_owned()
        }
        ConfigKind::OAuthResponse => "PISTE OAuth returned no access_token.".to_owned(),
        ConfigKind::ApiEndpoint => {
            "Legifrance endpoint not found: check LF_PISTE_BASE_URL.".to_owned()
        }
        ConfigKind::OAuthEndpoint => {
            "PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.".to_owned()
        }
        ConfigKind::Settings => clip(detail, SETTINGS_DETAIL_MAX),
    };
    AppError::config(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::audit::AuditErrorKind;
    use pretty_assertions::assert_eq;

    const ID: &str = "LEGITEXT000006070721";
    /// Détail factice portant une URL, un en-tête et un secret client : aucun ne doit atteindre
    /// le modèle (hors `ConfigKind::Settings` et `InvalidRequest`, dont le détail est rédigé
    /// pour être lu).
    const SENTINEL: &str = "GET https://example.invalid/oauth/token Bearer abc client_secret=x";

    fn corpus(tool: ToolName, subject: &str, error: &CorpusError) -> AppError {
        corpus_error(tool, subject, error)
    }

    /// Code, cause et message d'une `AppError` : la forme de la table de 02 §9.2.
    fn view(error: &AppError) -> (Option<ErrorCode>, Option<UpstreamCause>, &str) {
        (error.code(), error.cause(), error.message())
    }

    fn upstream(status: Option<u16>, retryable: bool) -> CorpusError {
        CorpusError::Upstream { status, retryable, detail: SENTINEL.to_owned() }
    }

    fn auth(origin: AuthOrigin, status: Option<u16>) -> CorpusError {
        CorpusError::Auth { origin, status, detail: SENTINEL.to_owned() }
    }

    fn config(kind: ConfigKind) -> CorpusError {
        CorpusError::Config { kind, detail: SENTINEL.to_owned() }
    }

    fn expected_upstream(cause: UpstreamCause, message: &str) -> AppError {
        AppError::Upstream { cause, message: message.to_owned() }
    }

    /// Une ligne de la table 02 §9.2 : erreur du corpus, code, cause, message.
    type Row = (CorpusError, Option<ErrorCode>, Option<UpstreamCause>, &'static str);

    /// Lignes 13, 13 bis et 15 : ressource absente et requête refusée.
    fn rows_request() -> Vec<Row> {
        use ErrorCode::{NotFound, UpstreamError};
        vec![
            // 02 §9.2 ligne 13 et 13 bis.
            (
                CorpusError::NotFound { what: ID.to_owned() },
                Some(NotFound),
                None,
                "No article found for LEGITEXT000006070721. Check the id, or find the article \
                 with fr_get_act or fr_search.",
            ),
            // Ligne 15 : 400 puis page de la passerelle (403), puis garde locale.
            (
                CorpusError::InvalidRequest { status: Some(400), detail: "bad facet".to_owned() },
                Some(UpstreamError),
                Some(UpstreamCause::Status(400)),
                "Legifrance rejected the request (HTTP 400): bad facet",
            ),
            (
                CorpusError::InvalidRequest { status: Some(403), detail: "gateway".to_owned() },
                Some(UpstreamError),
                Some(UpstreamCause::Status(403)),
                "Legifrance rejected the request (HTTP 403): gateway",
            ),
            (
                CorpusError::InvalidRequest { status: None, detail: "empty text".to_owned() },
                Some(UpstreamError),
                Some(UpstreamCause::Decode),
                "the request was not sent to Legifrance (empty text); report it.",
            ),
        ]
    }

    /// Lignes 16, 17 et 18 : quota, statuts HTTP, réseau et délai de requête.
    fn rows_upstream() -> Vec<Row> {
        use ErrorCode::{RateLimited, UpstreamError};
        vec![
            // Ligne 17 : réessayable puis non réessayable ; ligne 18 : réseau.
            (
                upstream(Some(503), true),
                Some(UpstreamError),
                Some(UpstreamCause::Status(503)),
                "Legifrance/PISTE HTTP 503 (service unavailable); retry once, then report it.",
            ),
            (
                upstream(Some(500), false),
                Some(UpstreamError),
                Some(UpstreamCause::Status(500)),
                "Legifrance/PISTE HTTP 500 (internal server error); report it.",
            ),
            (
                upstream(None, true),
                Some(UpstreamError),
                Some(UpstreamCause::Network),
                "could not reach Legifrance/PISTE (connection failed); retry once, then report it.",
            ),
            // Ligne 18 : délai de requête.
            (
                CorpusError::Timeout,
                Some(UpstreamError),
                Some(UpstreamCause::Timeout),
                "Legifrance/PISTE did not answer in time; retry once, then report it.",
            ),
            // Ligne 16.
            (
                CorpusError::RateLimited { retry_after: Some(Duration::from_secs(12)) },
                Some(RateLimited),
                None,
                "PISTE quota reached; retry in 12 s.",
            ),
        ]
    }

    /// Lignes 5 à 9, 11, 12, 14 et 19 : identifiants, configuration et corps illisible.
    fn rows_config() -> Vec<Row> {
        use ErrorCode::{ConfigError, UpstreamError};
        let rejected = "PISTE rejected the credentials (HTTP 401): check LF_PISTE_CLIENT_ID, \
                        LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV.";
        vec![
            // Lignes 7, 11 et 12.
            (auth(AuthOrigin::Token, Some(401)), Some(ConfigError), None, rejected),
            (
                auth(AuthOrigin::Api, Some(403)),
                Some(ConfigError),
                None,
                "PISTE denied access (HTTP 403): check that the PISTE application is subscribed \
                 to the Legifrance API.",
            ),
            // Lignes 5, 6, 8, 9 et 14.
            (
                config(ConfigKind::CredentialsMissing),
                Some(ConfigError),
                None,
                "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET \
                 (from your piste.gouv.fr application, section OAuth Credentials).",
            ),
            (
                config(ConfigKind::OAuthResponse),
                Some(ConfigError),
                None,
                "PISTE OAuth returned no access_token.",
            ),
            (
                config(ConfigKind::ApiEndpoint),
                Some(ConfigError),
                None,
                "Legifrance endpoint not found: check LF_PISTE_BASE_URL.",
            ),
            (
                config(ConfigKind::OAuthEndpoint),
                Some(ConfigError),
                None,
                "PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.",
            ),
            (
                CorpusError::Config {
                    kind: ConfigKind::Settings,
                    detail: "LF_PISTE_ENV='prod' is invalid: expected sandbox or production."
                        .to_owned(),
                },
                Some(ConfigError),
                None,
                "LF_PISTE_ENV='prod' is invalid: expected sandbox or production.",
            ),
            // Ligne 19.
            (
                CorpusError::Decode { detail: SENTINEL.to_owned() },
                Some(UpstreamError),
                Some(UpstreamCause::Decode),
                "unexpected response from Legifrance; report it.",
            ),
            (
                CorpusError::TooLarge { detail: SENTINEL.to_owned() },
                Some(UpstreamError),
                Some(UpstreamCause::Decode),
                "unexpected response from Legifrance; report it.",
            ),
        ]
    }

    #[test]
    fn corpus_error_maps_to_app_error_table() {
        use ErrorCode::UpstreamError;
        let tool = ToolName::FR_GET_TEXT;
        let rows = [rows_request(), rows_upstream(), rows_config()].concat();
        for (error, code, cause, message) in rows {
            let app = corpus(tool, ID, &error);
            assert_eq!(view(&app), (code, cause, message), "{error:?}");
        }
        // TooLarge sur get_act : la consigne propre à C2 (04 §7.3).
        let too_large = CorpusError::TooLarge { detail: SENTINEL.to_owned() };
        let app = corpus(ToolName::FR_GET_ACT, ID, &too_large);
        assert_eq!(
            view(&app),
            (
                Some(UpstreamError),
                Some(UpstreamCause::Decode),
                "this text is too large for get_act; use get_article or search instead."
            )
        );
    }

    #[test]
    fn auth_message_depends_on_origin_and_status_only() {
        let rejected = |status: &str| {
            format!(
                "PISTE rejected the credentials{status}: check LF_PISTE_CLIENT_ID, \
                 LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV."
            )
        };
        let denied = "PISTE denied access (HTTP 403): check that the PISTE application is \
                      subscribed to the Legifrance API.";
        let rows = [
            (AuthOrigin::Token, Some(400), rejected(" (HTTP 400)")),
            (AuthOrigin::Token, Some(401), rejected(" (HTTP 401)")),
            // Un 403 de l'endpoint du jeton est un refus d'identifiants, pas d'abonnement.
            (AuthOrigin::Token, Some(403), rejected(" (HTTP 403)")),
            (AuthOrigin::Token, None, rejected("")),
            (AuthOrigin::Api, Some(401), rejected(" (HTTP 401)")),
            (AuthOrigin::Api, Some(403), denied.to_owned()),
            (AuthOrigin::Api, None, rejected("")),
        ];
        for (origin, status, message) in rows {
            let app = corpus(ToolName::FR_SEARCH, "", &auth(origin, status));
            assert_eq!(view(&app), (Some(ErrorCode::ConfigError), None, message.as_str()));
        }
    }

    #[test]
    fn not_found_messages_match_reference() {
        let rows = [
            (
                ToolName::FR_GET_ACT,
                "LEGITEXT000006070721",
                "No LODA text found for LEGITEXT000006070721. Check the id, or search with \
                 fr_search fond=LODA or CODE.",
            ),
            (
                ToolName::FR_GET_TEXT,
                "LEGIARTI000006419999",
                "No article found for LEGIARTI000006419999. Check the id, or find the article \
                 with fr_get_act or fr_search.",
            ),
            (
                ToolName::FR_GET_DECISION,
                "JURITEXT000007000001",
                "No JURI decision found for JURITEXT000007000001. Check the id, or search with \
                 fr_search.",
            ),
            (
                ToolName::FR_GET_DECISION,
                "CONSTEXT000017665001",
                "No CONSTIT decision found for CONSTEXT000017665001. Check the id, or search \
                 with fr_search.",
            ),
            (
                ToolName::FR_GET_DECISION,
                "CETATEXT000008000001",
                "No CETAT decision found for CETATEXT000008000001. Check the id, or search with \
                 fr_search.",
            ),
            (
                ToolName::FR_GET_DELIBERATION,
                "CNILTEXT000017653001",
                "No CNIL deliberation found for CNILTEXT000017653001. Check the id, or search \
                 with fr_search.",
            ),
            (
                ToolName::FR_GET_CONVENTION,
                "KALITEXT000005635001",
                "No collective agreement text found for KALITEXT000005635001. Check the id, or \
                 search with fr_search.",
            ),
            (
                ToolName::FR_GET_COMPANY_AGREEMENT,
                "ACCOTEXT000030000001",
                "No company agreement found for ACCOTEXT000030000001. Check the id, or search \
                 with fr_search.",
            ),
        ];
        for (tool, id, message) in rows {
            // Les trois origines de l'adapter (404 de ressource, 200 vide, 400 « valider est
            // fausse ») arrivent en `NotFound` : le `what` varie, le message non.
            for what in [id, "empty response", "valider est fausse"] {
                let error = CorpusError::NotFound { what: what.to_owned() };
                let app = corpus(tool, id, &error);
                assert_eq!(view(&app), (Some(ErrorCode::NotFound), None, message), "{tool:?}");
            }
        }
        // La table couvre tous les outils qui lisent une ressource ; les deux autres ne
        // reçoivent jamais `NotFound` (04 §11.2) et ont un test à part.
        let covered: Vec<ToolName> = rows.iter().map(|row| row.0).collect();
        for tool in ToolName::ALL {
            assert!(
                covered.contains(&tool)
                    || tool == ToolName::FR_SEARCH
                    || tool == ToolName::FR_COVERAGE,
                "{tool:?} sans message not_found"
            );
        }
    }

    #[test]
    fn search_and_coverage_never_receive_not_found_and_fall_back_to_decode() {
        for tool in [ToolName::FR_SEARCH, ToolName::FR_COVERAGE] {
            let error = CorpusError::NotFound { what: "x".to_owned() };
            let app = corpus(tool, "", &error);
            assert_eq!(
                app,
                expected_upstream(
                    UpstreamCause::Decode,
                    "unexpected response from Legifrance; report it."
                )
            );
        }
    }

    #[test]
    fn decision_fond_follows_prefix_and_never_panics() {
        let rows = [
            ("JURITEXT000007000001", "JURI"),
            ("CONSTEXT000017665001", "CONSTIT"),
            ("CETATEXT000008000001", "CETAT"),
            // Préfixe inconnu, vide, ou sensible à la casse : JURI, sans panique.
            ("CNILTEXT000017653001", "JURI"),
            ("", "JURI"),
            ("juritext", "JURI"),
            ("é", "JURI"),
        ];
        for (subject, fond) in rows {
            let error = CorpusError::NotFound { what: String::new() };
            let app = corpus(ToolName::FR_GET_DECISION, subject, &error);
            let expected = format!(
                "No {fond} decision found for {subject}. Check the id, or search with fr_search."
            );
            assert_eq!(app, AppError::not_found(expected));
        }
    }

    /// Texte d'un refus d'identifiants (S8, S12).
    fn rejected(status: u16) -> String {
        format!(
            "PISTE rejected the credentials (HTTP {status}): check LF_PISTE_CLIENT_ID, \
             LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV."
        )
    }

    fn config_error(message: &str) -> AppError {
        AppError::config(message)
    }

    /// S6 à S9 : identifiants, OAuth, 404.
    fn matrix_s6_to_s9() {
        let act = ToolName::FR_GET_ACT;
        let uri = ID;
        // S6 : identifiants absents.
        assert_eq!(
            corpus(act, uri, &config(ConfigKind::CredentialsMissing)),
            config_error(
                "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET \
                 (from your piste.gouv.fr application, section OAuth Credentials)."
            )
        );
        // S7 : OAuth 200 sans access_token.
        assert_eq!(
            corpus(act, uri, &config(ConfigKind::OAuthResponse)),
            config_error("PISTE OAuth returned no access_token.")
        );
        // S8 : OAuth 4xx, puis OAuth 404.
        assert_eq!(
            corpus(act, uri, &auth(AuthOrigin::Token, Some(401))),
            AppError::config(rejected(401))
        );
        assert_eq!(
            corpus(act, uri, &config(ConfigKind::OAuthEndpoint)),
            config_error("PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.")
        );
        // S9 : API 404 de ressource, puis 404 de routage.
        assert_eq!(
            corpus(act, uri, &CorpusError::NotFound { what: uri.to_owned() }),
            AppError::not_found(
                "No LODA text found for LEGITEXT000006070721. Check the id, or search with \
                 fr_search fond=LODA or CODE."
            )
        );
        assert_eq!(
            corpus(act, uri, &config(ConfigKind::ApiEndpoint)),
            config_error("Legifrance endpoint not found: check LF_PISTE_BASE_URL.")
        );
    }

    /// S10 à S12 : 400, 403, 429, 5xx, 401.
    fn matrix_s10_to_s12() {
        use UpstreamCause::Status;
        let act = ToolName::FR_GET_ACT;
        let uri = ID;
        // S10 : 400 d'une recherche, 400 d'une consultation, 403 d'accès.
        let search_400 =
            CorpusError::InvalidRequest { status: Some(400), detail: "HTTP 400 on /search".into() };
        assert_eq!(
            corpus(ToolName::FR_SEARCH, "", &search_400),
            expected_upstream(
                Status(400),
                "Legifrance rejected the request (HTTP 400): HTTP 400 on /search"
            )
        );
        let consult_400 = CorpusError::InvalidRequest {
            status: Some(400),
            detail: "HTTP 400 on /consult/lawDecree".into(),
        };
        assert_eq!(
            corpus(act, uri, &consult_400),
            expected_upstream(
                Status(400),
                "Legifrance rejected the request (HTTP 400): HTTP 400 on /consult/lawDecree"
            )
        );
        assert_eq!(
            corpus(act, uri, &auth(AuthOrigin::Api, Some(403))),
            config_error(
                "PISTE denied access (HTTP 403): check that the PISTE application is subscribed \
                 to the Legifrance API."
            )
        );
        // S11 : 502/503/504 puis 500 puis 429.
        assert_eq!(
            corpus(act, uri, &upstream(Some(503), true)),
            expected_upstream(
                Status(503),
                "Legifrance/PISTE HTTP 503 (service unavailable); retry once, then report it."
            )
        );
        assert_eq!(
            corpus(act, uri, &upstream(Some(500), false)),
            expected_upstream(
                Status(500),
                "Legifrance/PISTE HTTP 500 (internal server error); report it."
            )
        );
        assert_eq!(
            corpus(
                act,
                uri,
                &CorpusError::RateLimited { retry_after: Some(Duration::from_secs(12)) }
            ),
            AppError::RateLimited {
                message: "PISTE quota reached; retry in 12 s.".to_owned(),
                retry_after: Some(Duration::from_secs(12)),
            }
        );
        // S12 : API 401 deux fois.
        assert_eq!(
            corpus(act, uri, &auth(AuthOrigin::Api, Some(401))),
            AppError::config(rejected(401))
        );
    }

    /// S13 à S15 et S17 : réseau, délai, corps illisible, charge vide, audit.
    fn matrix_s13_to_s17() {
        use UpstreamCause::{Decode, Network};
        let act = ToolName::FR_GET_ACT;
        let uri = ID;
        // S13 : réseau, délai de requête, délai global.
        assert_eq!(
            corpus(act, uri, &upstream(None, true)),
            expected_upstream(
                Network,
                "could not reach Legifrance/PISTE (connection failed); retry once, then report it."
            )
        );
        assert_eq!(
            corpus(act, uri, &CorpusError::Timeout),
            expected_upstream(
                UpstreamCause::Timeout,
                "Legifrance/PISTE did not answer in time; retry once, then report it."
            )
        );
        assert_eq!(
            deadline(Duration::from_secs(60)),
            expected_upstream(
                UpstreamCause::Deadline,
                "tool deadline of 60 s exceeded; retry, or narrow the request (smaller \
                 page_size, fewer fonds)."
            )
        );
        // S14 : 200 à corps non JSON.
        assert_eq!(
            corpus(act, uri, &CorpusError::Decode { detail: "expected value".into() }),
            expected_upstream(Decode, "unexpected response from Legifrance; report it.")
        );
        // S15 : 200 à charge vide ⇒ première phrase de la référence, puis l'aide.
        let empty = CorpusError::NotFound { what: "empty payload".into() };
        assert_eq!(
            corpus(act, uri, &empty).message(),
            "No LODA text found for LEGITEXT000006070721. Check the id, or search with fr_search \
             fond=LODA or CODE."
        );
        // S17 : audit, sans puis avec code d'origine.
        let failure = AuditError { kind: AuditErrorKind::PermissionDenied, detail: "/x".into() };
        assert_eq!(
            audit(&failure, None),
            AppError::Audit {
                message: "audit log unavailable; the call was not recorded and its result is \
                          withheld (see server logs)."
                    .to_owned(),
                original: None,
            }
        );
        assert_eq!(
            audit(&failure, Some(ErrorCode::UpstreamError)),
            AppError::Audit {
                message: "audit log unavailable; the call was not recorded and its result is \
                          withheld (see server logs). Original error: [upstream_error]."
                    .to_owned(),
                original: Some(ErrorCode::UpstreamError),
            }
        );
    }

    #[test]
    fn error_matrix_s1_to_s17() {
        // S1 à S5 (validation) appartiennent à 3b ; S16 est la ligne 12 de `audited.rs`.
        matrix_s6_to_s9();
        matrix_s10_to_s12();
        matrix_s13_to_s17();
    }

    #[test]
    fn retry_hint_only_for_retryable_failures() {
        let hint = "retry once, then report it.";
        let cases = [
            (upstream(Some(502), true), true),
            (upstream(Some(504), true), true),
            (upstream(None, true), true),
            (CorpusError::Timeout, true),
            (upstream(Some(500), false), false),
            (upstream(Some(418), false), false),
            (upstream(None, false), false),
            (CorpusError::Decode { detail: String::new() }, false),
            (CorpusError::TooLarge { detail: String::new() }, false),
            (CorpusError::InvalidRequest { status: Some(400), detail: String::new() }, false),
            (CorpusError::InvalidRequest { status: None, detail: String::new() }, false),
        ];
        for (error, retryable) in cases {
            let app = corpus(ToolName::FR_GET_TEXT, ID, &error);
            assert_eq!(app.message().contains(hint), retryable, "{error:?}");
        }
    }

    #[test]
    fn upstream_reason_phrases_cover_the_documented_statuses_only() {
        let rows = [
            (500, "Legifrance/PISTE HTTP 500 (internal server error); report it."),
            (502, "Legifrance/PISTE HTTP 502 (bad gateway); report it."),
            (503, "Legifrance/PISTE HTTP 503 (service unavailable); report it."),
            (504, "Legifrance/PISTE HTTP 504 (gateway timeout); report it."),
            (418, "Legifrance/PISTE HTTP 418; report it."),
        ];
        for (status, message) in rows {
            let app = corpus(ToolName::FR_GET_ACT, ID, &upstream(Some(status), false));
            assert_eq!(app.message(), message);
            assert_eq!(app.cause(), Some(UpstreamCause::Status(status)));
        }
    }

    #[test]
    fn rate_limited_message_with_and_without_retry_after() {
        let rows = [
            (Some(Duration::from_secs(12)), "PISTE quota reached; retry in 12 s."),
            // Arrondi au supérieur : 1,2 s se lit « 2 s », jamais une attente trop courte.
            (Some(Duration::from_millis(1200)), "PISTE quota reached; retry in 2 s."),
            (Some(Duration::from_millis(1)), "PISTE quota reached; retry in 1 s."),
            (Some(Duration::ZERO), "PISTE quota reached; retry in 0 s."),
            (None, "PISTE quota reached; retry in a few seconds."),
        ];
        for (retry_after, message) in rows {
            let app = corpus(ToolName::FR_SEARCH, "", &CorpusError::RateLimited { retry_after });
            assert_eq!(app, AppError::RateLimited { message: message.to_owned(), retry_after });
        }
    }

    #[test]
    fn messages_never_echo_corpus_detail() {
        let sentinels = ["Bearer abc", "client_secret=x", "example.invalid", "oauth/token"];
        let kinds = [
            ConfigKind::CredentialsMissing,
            ConfigKind::OAuthResponse,
            ConfigKind::ApiEndpoint,
            ConfigKind::OAuthEndpoint,
        ];
        let mut errors: Vec<CorpusError> = kinds.into_iter().map(config).collect();
        for origin in [AuthOrigin::Token, AuthOrigin::Api] {
            for status in [None, Some(400), Some(401), Some(403)] {
                errors.push(auth(origin, status));
            }
        }
        errors.push(CorpusError::TooLarge { detail: SENTINEL.to_owned() });
        errors.push(CorpusError::Decode { detail: SENTINEL.to_owned() });
        for status in [None, Some(500), Some(503)] {
            for retryable in [false, true] {
                errors.push(upstream(status, retryable));
            }
        }
        // `InvalidRequest` est exclu à dessein : son détail est le message PISTE déjà nettoyé
        // par l'adapter (05 §7.2), recopié, coupé à 200 caractères.
        for tool in ToolName::ALL {
            for error in &errors {
                let app = corpus(tool, ID, error);
                let message = app.message();
                for sentinel in sentinels {
                    assert!(
                        !message.contains(sentinel),
                        "{tool:?} {error:?} echoes {sentinel}: {message}"
                    );
                }
            }
        }
    }

    #[test]
    fn invalid_request_detail_is_clipped_to_200_chars() {
        let long = "é".repeat(250);
        for status in [Some(400), None] {
            let error = CorpusError::InvalidRequest { status, detail: long.clone() };
            let app = corpus(ToolName::FR_GET_ACT, ID, &error);
            let kept = "é".repeat(200);
            assert!(app.message().contains(&kept), "{status:?}");
            assert!(!app.message().contains(&"é".repeat(201)), "{status:?}");
        }
        // Un détail de 200 caractères pile passe entier.
        let exact = "x".repeat(200);
        let error = CorpusError::InvalidRequest { status: Some(400), detail: exact.clone() };
        let app = corpus(ToolName::FR_GET_ACT, ID, &error);
        assert_eq!(app.message(), format!("Legifrance rejected the request (HTTP 400): {exact}"));
    }

    #[test]
    fn config_settings_detail_is_clipped_to_500_chars() {
        let long = "é".repeat(600);
        let error = CorpusError::Config { kind: ConfigKind::Settings, detail: long };
        let app = corpus(ToolName::FR_GET_ACT, ID, &error);
        assert_eq!(app, AppError::config("é".repeat(500)));
        let exact = "x".repeat(500);
        let error = CorpusError::Config { kind: ConfigKind::Settings, detail: exact.clone() };
        assert_eq!(corpus(ToolName::FR_GET_ACT, ID, &error), AppError::config(exact));
    }

    #[test]
    fn audit_message_names_no_path() {
        let failure = AuditError {
            kind: AuditErrorKind::DiskFull,
            detail: "write /var/log/lf/audit.jsonl: no space left on device".to_owned(),
        };
        for original in [None, Some(ErrorCode::NotFound)] {
            let app = audit(&failure, original);
            for leaked in ["/var/log", "audit.jsonl", "no space", "DiskFull"] {
                let message = app.message();
                assert!(!message.contains(leaked), "{leaked} in {message}");
            }
        }
    }

    #[test]
    fn audit_message_cites_original_code() {
        let failure = AuditError { kind: AuditErrorKind::Io, detail: String::new() };
        for code in ErrorCode::ALL {
            let app = audit(&failure, Some(code));
            let expected = format!(
                "audit log unavailable; the call was not recorded and its result is withheld \
                 (see server logs). Original error: [{}].",
                code.as_str()
            );
            assert_eq!(app.message(), expected);
            assert_eq!(app.code(), Some(ErrorCode::AuditError));
        }
    }

    #[test]
    fn deadline_rounds_up_seconds() {
        let rows = [
            (Duration::from_secs(60), 60),
            (Duration::from_millis(59_200), 60),
            (Duration::from_millis(59_001), 60),
            (Duration::from_millis(1), 1),
            (Duration::from_secs(600), 600),
        ];
        for (budget, seconds) in rows {
            let expected = format!(
                "tool deadline of {seconds} s exceeded; retry, or narrow the request (smaller \
                 page_size, fewer fonds)."
            );
            assert_eq!(deadline(budget), expected_upstream(UpstreamCause::Deadline, &expected));
        }
    }

    #[test]
    fn cancelled_has_no_code_and_a_fixed_message() {
        let app = cancelled();
        assert_eq!(app, expected_upstream(UpstreamCause::Cancelled, "cancelled by client"));
        assert_eq!(app.code(), None);
    }

    #[test]
    fn invalid_argument_keeps_the_given_text() {
        let app = invalid_argument("page=0 out of range (1..).");
        assert_eq!(app, AppError::invalid_argument("page=0 out of range (1..)."));
        assert_eq!(invalid_argument(String::from("a")).message(), "a");
    }

    #[test]
    fn clip_does_not_split_code_points() {
        assert_eq!(clip("abcdef", 3), "abc");
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abc", 10), "abc");
        assert_eq!(clip("", 3), "");
        assert_eq!(clip("abc", 0), "");
        // Accents (2 octets), emoji (4 octets), caractère combinant : on compte des points de code.
        assert_eq!(clip("éàü", 2), "éà");
        assert_eq!(clip("a😀b😀", 3), "a😀b");
        assert_eq!(clip("e\u{301}x", 2), "e\u{301}");
    }
}
