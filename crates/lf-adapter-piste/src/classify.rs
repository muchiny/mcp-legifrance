//! Classification d'une tentative d'envoi : succès, jeton refusé, échec passager ou définitif.
//!
//! Le statut HTTP, `Retry-After` et `Date` classent la réponse. Le corps n'est lu que dans quatre
//! cas (04 §11) : le message d'un 400 de consultation (marqueur d'un identifiant inconnu, qui
//! tranche « introuvable », et détail de l'erreur sinon), un 404 de consultation (une page HTML
//! n'est pas une preuve d'absence), un 403 (une page HTML est celle de la passerelle, pas un
//! refus d'accès) et un 2xx vide ; le corps d'un 400 de recherche ou du jeton
//! n'est jamais lu, et tout autre corps reste intact (`post_json` décode celui des 2xx). Le choix
//! de réessayer, d'attendre ou de renoncer appartient à `retry::run` ; ce module ne dort pas et
//! ne lit aucune horloge.

use std::time::Duration;

use lf_application::ports::{AuthOrigin, ConfigKind, CorpusError};
use reqwest::Client;

use crate::error_body;
use crate::http::{Attempt, RawResponse, TransportError, send_once};
use crate::retry::parse_retry_after;

/// Plus longue attente propagée dans [`CorpusError::RateLimited`].
///
/// Un `Retry-After` hostile (`u64::MAX` secondes, ou une date en l'an 9999) ne doit pas
/// atteindre l'appelant tel quel : un `Instant::now() + d` y paniquerait, et le modèle lirait
/// « réessayez dans 584 milliards d'années ». Seule la valeur rangée dans l'erreur est
/// écrêtée ; la décision d'attendre ou de refuser (`retry::run`, au plus 30 s) garde la valeur
/// annoncée.
const MAX_REPORTED_RETRY_AFTER: Duration = Duration::from_secs(3_600);

/// Budget de réessais que consomme un échec passager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Budget {
    /// Budget ordinaire : trois requêtes au plus.
    Standard,
    /// Budget réduit du 500 : deux requêtes au plus, ce statut étant le plus souvent durable.
    Reduced,
}

/// Issue classée d'une tentative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome<T> {
    /// Réponse 2xx : la valeur est celle que la tentative a produite.
    Success(T),
    /// Réponse 401 : porte l'erreur **finale** `Auth`, car seul `classify` connaît le gabarit
    /// d'endpoint. Le renouvellement du jeton est du ressort de l'appelant ; si l'issue atteint
    /// `retry::run`, elle y est définitive.
    Unauthorized(CorpusError),
    /// Échec qui mérite une nouvelle tentative, si le budget et le délai le permettent.
    Transient {
        /// Erreur à rendre si l'on renonce : déjà finale, la boucle ne la réécrit pas.
        error: CorpusError,
        /// Attente annoncée par le serveur (`Retry-After`), s'il y en a une d'exploitable.
        retry_after: Option<Duration>,
        /// Budget que cet échec consomme.
        budget: Budget,
    },
    /// Échec définitif : inutile de réessayer.
    Fatal(CorpusError),
}

impl<T> Outcome<T> {
    /// La même issue, où la valeur d'un succès passe par `f` ; toute autre issue est recopiée et
    /// `f` tombe sans être appelée (avec ce qu'elle a capturé).
    pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> Outcome<U> {
        match self {
            Self::Success(value) => Outcome::Success(f(value)),
            Self::Unauthorized(error) => Outcome::Unauthorized(error),
            Self::Transient { error, retry_after, budget } => {
                Outcome::Transient { error, retry_after, budget }
            }
            Self::Fatal(error) => Outcome::Fatal(error),
        }
    }
}

/// Ce que l'appelant interroge : il décide si une réponse peut signifier « introuvable ».
///
/// Le genre est passé par l'appelant et non déduit du gabarit d'endpoint : aucune comparaison de
/// chaîne ne peut mal classer un chemin nouveau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointKind {
    /// Recherche (`/search`) : zéro résultat est un succès, jamais [`CorpusError::NotFound`].
    Search,
    /// Consultation d'un texte, d'un article ou d'une version : seule à pouvoir être
    /// [`CorpusError::NotFound`].
    Consult,
    /// Endpoint du jeton : jamais [`CorpusError::NotFound`], son corps d'erreur est traité par
    /// `auth.rs`.
    Token,
}

impl EndpointKind {
    /// Origine d'un refus d'identifiants (401, 403 non HTML) reçu de cet endpoint. `auth.rs`
    /// traite ces statuts avant `classify` pour le jeton : le type reste juste s'ils l'atteignent.
    fn auth_origin(self) -> AuthOrigin {
        match self {
            Self::Token => AuthOrigin::Token,
            Self::Search | Self::Consult => AuthOrigin::Api,
        }
    }

    /// Nature d'un 404 ou 405 qui ne prouve pas l'absence : adresse du jeton (`LF_PISTE_OAUTH_URL`)
    /// ou de l'API (`LF_PISTE_BASE_URL`) erronée.
    fn endpoint_problem(self) -> ConfigKind {
        match self {
            Self::Token => ConfigKind::OAuthEndpoint,
            Self::Search | Self::Consult => ConfigKind::ApiEndpoint,
        }
    }
}

/// Contexte d'une classification : genre d'endpoint, hôte, réglage du mojibake et secrets à ne
/// jamais laisser dans un détail d'erreur.
///
/// Pas de `Debug` : `secrets` porte le jeton d'accès et le secret client, que ni un journal ni un
/// message ne doivent montrer. Ces valeurs ne servent qu'à être **retirées** d'un message du corps.
#[derive(Clone, Copy)]
pub(crate) struct ErrorContext<'a> {
    /// Ce que l'appelant interroge.
    pub kind: EndpointKind,
    /// Les deux adresses de l'adapter sont officielles : un 404 de consultation prouve alors
    /// l'absence.
    pub official_host: bool,
    /// Réparer le mojibake du message d'erreur lu dans le corps.
    pub repair_mojibake: bool,
    /// Valeurs exactes à retirer du détail d'un 400 (jeton d'accès, secret client).
    pub secrets: &'a [&'a str],
}

impl ErrorContext<'static> {
    /// Contexte de l'endpoint du jeton : aucun corps d'erreur n'est lu par `classify`.
    pub(crate) const TOKEN: Self = Self {
        kind: EndpointKind::Token,
        official_host: false,
        repair_mojibake: false,
        secrets: &[],
    };
}

/// Classe le résultat d'une tentative d'envoi.
///
/// `endpoint` est le gabarit de chemin (`/consult/juri`), seule trace de la route dans les
/// `detail` des erreurs : jamais d'URL, de jeton ni de corps.
pub(crate) fn classify(
    result: Result<RawResponse, TransportError>,
    endpoint: &str,
    context: &ErrorContext<'_>,
) -> Outcome<RawResponse> {
    match result {
        Ok(response) => classify_status(response, endpoint, context),
        Err(error) => classify_transport(error, context.kind),
    }
}

/// Classe une réponse HTTP d'après son statut ; `Retry-After` n'est lu que pour 429, 502, 503 et
/// 504. Le corps n'est lu que pour un 2xx (vide ?), un 400 de consultation, un 403 (page HTML de
/// la passerelle ou non) et un 404.
fn classify_status(
    response: RawResponse,
    endpoint: &str,
    context: &ErrorContext<'_>,
) -> Outcome<RawResponse> {
    let status = response.status;
    let detail = || format!("HTTP {status} on {endpoint}");
    let retry_after =
        || parse_retry_after(response.retry_after.as_deref(), response.date.as_deref());
    match status {
        200..=299 => classify_success(response, endpoint, context.kind),
        401 => Outcome::Unauthorized(CorpusError::Auth {
            origin: context.kind.auth_origin(),
            status: Some(status),
            detail: detail(),
        }),
        // Une page HTML est celle de la passerelle (Apache, 199 octets observés sur `/search`
        // avec un mot de 100 caractères ou plus), pas un refus d'accès à l'API. Le seul 403 HTML
        // enregistré est cette page ; le 403 d'une application non souscrite n'a jamais été
        // enregistré (V-65) : s'il est lui aussi une page HTML, il sera classé `InvalidRequest`.
        // Un 403 JSON ou vide reste `Auth`.
        403 if error_body::looks_like_html(&response.body) => {
            Outcome::Fatal(CorpusError::InvalidRequest {
                status: Some(403),
                detail: "request rejected by the PISTE gateway".to_owned(),
            })
        }
        403 => Outcome::Fatal(CorpusError::Auth {
            origin: context.kind.auth_origin(),
            status: Some(status),
            detail: detail(),
        }),
        429 => {
            let retry_after = retry_after();
            Outcome::Transient {
                error: CorpusError::RateLimited {
                    retry_after: retry_after.map(|wait| wait.min(MAX_REPORTED_RETRY_AFTER)),
                },
                retry_after,
                budget: Budget::Standard,
            }
        }
        // L'erreur est déjà finale (`retryable: false`) : la boucle ne réécrit rien.
        500 => Outcome::Transient {
            error: CorpusError::Upstream {
                status: Some(status),
                retryable: false,
                detail: detail(),
            },
            retry_after: None,
            budget: Budget::Reduced,
        },
        502..=504 => Outcome::Transient {
            error: CorpusError::Upstream {
                status: Some(status),
                retryable: true,
                detail: detail(),
            },
            retry_after: retry_after(),
            budget: Budget::Standard,
        },
        400 => Outcome::Fatal(bad_request(&response, endpoint, context)),
        // Le 404 d'une consultation ne prouve l'absence que d'un hôte officiel et hors page HTML.
        404 if proves_absence(&response, context) => {
            Outcome::Fatal(CorpusError::NotFound { what: endpoint.to_owned() })
        }
        404 | 405 => Outcome::Fatal(CorpusError::Config {
            kind: context.kind.endpoint_problem(),
            detail: detail(),
        }),
        _ => Outcome::Fatal(CorpusError::Upstream {
            status: Some(status),
            retryable: false,
            detail: detail(),
        }),
    }
}

/// Classe un 2xx : un corps vide est « introuvable » sur une consultation (sans réessai), une
/// erreur de décodage sur une recherche, et reste un succès pour le jeton, que `auth.rs` lit.
///
/// Tout corps non vide, même `null` ou `{}`, est laissé au décodeur : une enveloppe vide se juge
/// avec les DTO métier (04 §9.1 a).
fn classify_success(
    response: RawResponse,
    endpoint: &str,
    kind: EndpointKind,
) -> Outcome<RawResponse> {
    match (kind, response.body.trim_ascii().is_empty()) {
        (EndpointKind::Consult, true) => {
            Outcome::Fatal(CorpusError::NotFound { what: endpoint.to_owned() })
        }
        (EndpointKind::Search, true) => Outcome::Fatal(CorpusError::Decode {
            detail: format!("empty response body on {endpoint}"),
        }),
        _ => Outcome::Success(response),
    }
}

/// Erreur d'un 400 : pour une consultation, `NotFound` si le message désigne un identifiant
/// inconnu, sinon `InvalidRequest` dont le détail est le message PISTE nettoyé, privé des
/// secrets et borné, ou le gabarit `HTTP 400 on {endpoint}` si le corps n'en livre aucun. Pour
/// une recherche, le détail est **toujours** ce gabarit.
///
/// Le message d'un 400 de recherche peut citer le texte de la requête (04 §12.4, §12.5), et le
/// `detail` va aux journaux et à l'audit de l'application : il n'est donc ni lu ni recopié. Le
/// `detail` d'une consultation est le message seul : 05 §7.2 le recopie tel quel après
/// « Legifrance rejected the request (HTTP 400): ». Un 400 est un bug probable (`warn`), sauf
/// l'identifiant inconnu, qui est un cas normal. Le corps n'est jamais journalisé tel quel ; le
/// `warn` d'une recherche ne porte ni message ni détail. Le corps du jeton n'est jamais lu.
fn bad_request(response: &RawResponse, endpoint: &str, context: &ErrorContext<'_>) -> CorpusError {
    let message = match context.kind {
        EndpointKind::Consult => {
            error_body::message(&response.body, endpoint, context.repair_mojibake)
        }
        EndpointKind::Search | EndpointKind::Token => None,
    };
    // Le marqueur se cherche dans le texte brut complet, avant nettoyage, retrait et coupure.
    if message.as_deref().is_some_and(|raw| error_body::is_not_found(endpoint, raw)) {
        return CorpusError::NotFound { what: endpoint.to_owned() };
    }
    let detail = message
        .map(|raw| error_body::clean_detail(&raw, context.secrets))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| format!("HTTP 400 on {endpoint}"));
    if context.kind == EndpointKind::Search {
        // 04 §12.5 : le texte de la requête n'est journalisé à aucun niveau, et le message PISTE
        // d'un 400 de recherche peut le citer ; le journal n'en porte rien.
        tracing::warn!(
            endpoint,
            status = 400,
            "PISTE rejected a search request as invalid (HTTP 400): probable bug"
        );
    } else {
        tracing::warn!(
            endpoint,
            %detail,
            "PISTE rejected a request as invalid (HTTP 400): probable bug"
        );
    }
    CorpusError::InvalidRequest { status: Some(400), detail }
}

/// Un 404 prouve l'absence d'une ressource seulement pour une consultation, sur l'hôte officiel
/// et avec un corps qui n'est pas une page HTML (passerelle, proxy) : sinon c'est une erreur de
/// configuration.
fn proves_absence(response: &RawResponse, context: &ErrorContext<'_>) -> bool {
    context.kind == EndpointKind::Consult
        && context.official_host
        && !error_body::looks_like_html(&response.body)
}

/// Classe un échec de transport : délai et réseau sont passagers, TLS et corps trop gros non.
///
/// Le corps trop gros est un [`CorpusError::TooLarge`] pour `/search` et les consultations, mais
/// un [`CorpusError::Decode`] pour l'endpoint du jeton : un jeton énorme n'est pas « un texte trop
/// gros pour `get_act` » (plan 3a). Le genre vient de l'appelant, jamais du texte du `detail`.
fn classify_transport(error: TransportError, kind: EndpointKind) -> Outcome<RawResponse> {
    match error {
        TransportError::Timeout | TransportError::Network { .. } => {
            Outcome::Transient { error: error.into(), retry_after: None, budget: Budget::Standard }
        }
        TransportError::TooLarge { detail } if kind == EndpointKind::Token => {
            Outcome::Fatal(CorpusError::Decode { detail })
        }
        TransportError::Tls { .. } | TransportError::TooLarge { .. } => {
            Outcome::Fatal(error.into())
        }
    }
}

/// Une tentative d'envoi puis sa classification.
pub(crate) async fn try_send(
    client: &Client,
    attempt: &Attempt<'_>,
    context: &ErrorContext<'_>,
) -> Outcome<RawResponse> {
    classify(send_once(client, attempt).await, attempt.endpoint, context)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::testing::capture_at;

    const ENDPOINT: &str = "/consult/juri";
    const DATE: &str = "Sun, 06 Nov 1994 08:49:37 GMT";
    const CONSULT: ErrorContext<'static> = ErrorContext {
        kind: EndpointKind::Consult,
        official_host: false,
        repair_mojibake: true,
        secrets: &[],
    };

    const SEARCH_ENDPOINT: &str = "/search";
    const OFFICIAL: ErrorContext<'static> = ErrorContext { official_host: true, ..CONSULT };
    const SEARCH: ErrorContext<'static> = ErrorContext { kind: EndpointKind::Search, ..OFFICIAL };
    const MARKER_MESSAGE: &str = "L'expression à valider est fausse.";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn response(status: u16) -> RawResponse {
        RawResponse {
            status,
            retry_after: None,
            date: None,
            content_type: None,
            content_encoding: None,
            body: b"{}".to_vec(),
        }
    }

    fn with_body(status: u16, body: &str) -> RawResponse {
        RawResponse { body: body.as_bytes().to_vec(), ..response(status) }
    }

    /// Réponse d'erreur de PISTE, avec la forme du 400 de référence.
    fn piste_error(status: u16, message: &str) -> RawResponse {
        let body = serde_json::json!({
            "timestamp": 1,
            "error": status,
            "status": "Bad Request",
            "message": message,
            "errorCode": null,
        });
        with_body(status, &body.to_string())
    }

    /// Le texte lu comme du latin-1 : le mojibake que PISTE a déjà servi.
    fn mojibake(text: &str) -> String {
        text.bytes().map(char::from).collect()
    }

    fn fatal(error: CorpusError) -> Outcome<RawResponse> {
        Outcome::Fatal(error)
    }

    fn not_found() -> CorpusError {
        CorpusError::NotFound { what: ENDPOINT.to_owned() }
    }

    fn invalid(detail: &str) -> CorpusError {
        CorpusError::InvalidRequest { status: Some(400), detail: detail.to_owned() }
    }

    fn with_headers(status: u16, retry_after: Option<&str>, date: Option<&str>) -> RawResponse {
        RawResponse {
            retry_after: retry_after.map(str::to_owned),
            date: date.map(str::to_owned),
            ..response(status)
        }
    }

    fn classified(status: u16) -> Outcome<RawResponse> {
        classify(Ok(response(status)), ENDPOINT, &CONSULT)
    }

    fn detail(status: u16) -> String {
        format!("HTTP {status} on {ENDPOINT}")
    }

    fn upstream(status: u16, retryable: bool) -> CorpusError {
        CorpusError::Upstream { status: Some(status), retryable, detail: detail(status) }
    }

    /// Refus de l'API, au statut reçu.
    fn api_auth(status: u16) -> CorpusError {
        CorpusError::Auth { origin: AuthOrigin::Api, status: Some(status), detail: detail(status) }
    }

    /// 404 ou 405 de routage sur l'API, au détail donné.
    fn api_endpoint(detail: String) -> CorpusError {
        CorpusError::Config { kind: ConfigKind::ApiEndpoint, detail }
    }

    fn transient(error: CorpusError, retry_after: Option<Duration>) -> Outcome<RawResponse> {
        Outcome::Transient { error, retry_after, budget: Budget::Standard }
    }

    #[test]
    fn every_2xx_is_a_success_carrying_the_response() {
        for status in [200, 201, 202, 204, 206, 299] {
            assert_eq!(classified(status), Outcome::Success(response(status)), "status {status}");
        }
    }

    #[test]
    fn the_status_just_outside_2xx_is_not_a_success() {
        for status in [199, 300] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    #[test]
    fn redirects_are_final_upstream_errors() {
        for status in [301, 302, 303, 304, 307, 308, 399] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    #[test]
    fn status_400_without_a_usable_message_keeps_the_generic_detail() {
        for body in ["{}", "", "<html>Bad Request</html>", "plain", r#"{"message":null}"#, "null"] {
            for context in [CONSULT, SEARCH] {
                assert_eq!(
                    classify(Ok(with_body(400, body)), ENDPOINT, &context),
                    fatal(invalid("HTTP 400 on /consult/juri")),
                    "{body:?}"
                );
            }
        }
    }

    #[test]
    fn the_400_marker_on_a_consultation_is_not_found_on_any_host() {
        for context in [CONSULT, OFFICIAL] {
            assert_eq!(
                classify(Ok(piste_error(400, MARKER_MESSAGE)), ENDPOINT, &context),
                fatal(not_found())
            );
        }
    }

    #[test]
    fn the_400_marker_survives_any_shape_of_error_code() {
        for code in ["1234", r#""E42""#, "null", r#"{"code":1}"#, r#"["E42"]"#, "true", "1.5"] {
            let body =
                format!(r#"{{"error":400,"message":"{MARKER_MESSAGE}","errorCode":{code}}}"#);
            assert_eq!(
                classify(Ok(with_body(400, &body)), ENDPOINT, &CONSULT),
                fatal(not_found()),
                "{code}"
            );
        }
    }

    #[test]
    fn the_400_marker_is_searched_in_the_whole_raw_message() {
        // Le marqueur est au-delà des 200 caractères que garde le détail : il se cherche dans le
        // texte brut complet, avant la coupure.
        let message = format!("{} {MARKER_MESSAGE}", "x".repeat(250));
        assert_eq!(
            classify(Ok(piste_error(400, &message)), ENDPOINT, &CONSULT),
            fatal(not_found())
        );
    }

    #[test]
    fn the_400_marker_ignores_case_and_accent() {
        for message in [
            "L'EXPRESSION À VALIDER EST FAUSSE.",
            "expression a valider est fausse",
            "VALIDER EST FAUSSE",
        ] {
            assert_eq!(
                classify(Ok(piste_error(400, message)), ENDPOINT, &CONSULT),
                fatal(not_found()),
                "{message}"
            );
        }
    }

    #[test]
    fn a_400_mentioning_a_valider_alone_is_not_not_found() {
        for context in [CONSULT, OFFICIAL] {
            for message in ["il faut a valider ceci", "À valider", "a valider"] {
                assert_eq!(
                    classify(Ok(piste_error(400, message)), ENDPOINT, &context),
                    fatal(invalid(message)),
                    "{message}"
                );
            }
        }
    }

    #[test]
    fn the_400_marker_survives_mojibake_with_or_without_repair() {
        let garbled = mojibake(MARKER_MESSAGE);
        for repair_mojibake in [true, false] {
            let context = ErrorContext { repair_mojibake, ..CONSULT };
            assert_eq!(
                classify(Ok(piste_error(400, &garbled)), ENDPOINT, &context),
                fatal(not_found()),
                "repair {repair_mojibake}"
            );
        }
    }

    /// Corps enregistré en sandbox le 2026-10-28 (lot `inconnu/`, plan 2h) : 400 d'un identifiant
    /// ACCO inexistant, d'un message qui n'est pas le marqueur « valider est fausse ».
    const ACCO_UNKNOWN: [&str; 2] = [
        include_str!("../tests/fixtures/recorded/consult/acco/inconnu/ACCOTEXT000000000000.json"),
        include_str!("../tests/fixtures/recorded/consult/acco/inconnu/ACCOTEXT999999999999.json"),
    ];
    const ACCO_ENDPOINT: &str = crate::map::endpoint::ACCO;
    const ACCO_COUNT_MESSAGE: &str =
        "Le nombre de résultat retourné par le moteur de recherche n'est pas égal à 1.";

    #[test]
    fn the_acco_count_message_on_a_consultation_is_not_found() {
        for context in [CONSULT, OFFICIAL] {
            for body in ACCO_UNKNOWN {
                assert_eq!(
                    classify(Ok(with_body(400, body)), ACCO_ENDPOINT, &context),
                    fatal(CorpusError::NotFound { what: ACCO_ENDPOINT.to_owned() }),
                    "{body}"
                );
            }
        }
    }

    #[test]
    fn the_acco_count_message_is_not_found_only_on_the_acco_consultation() {
        // R7 : le message de comptage ne dit « introuvable » que de `/consult/acco`. Sur une
        // autre consultation il reste un 400 rejeté (détail = le message), et sur `/search`,
        // où le corps n'est pas lu, un 400 au détail générique.
        for body in ACCO_UNKNOWN {
            assert_eq!(
                classify(Ok(with_body(400, body)), ENDPOINT, &OFFICIAL),
                fatal(invalid(ACCO_COUNT_MESSAGE))
            );
            assert_eq!(
                classify(Ok(with_body(400, body)), SEARCH_ENDPOINT, &SEARCH),
                fatal(invalid("HTTP 400 on /search"))
            );
        }
    }

    #[test]
    fn another_400_message_on_the_acco_consultation_is_a_rejected_request() {
        for message in ["Bad Request", "Une erreur est survenue dans le moteur de recherche"] {
            assert_eq!(
                classify(Ok(piste_error(400, message)), ACCO_ENDPOINT, &OFFICIAL),
                fatal(invalid(message)),
                "{message}"
            );
        }
    }

    #[test]
    fn the_400_marker_on_search_is_only_a_rejected_request_with_a_generic_detail() {
        assert_eq!(
            classify(Ok(piste_error(400, MARKER_MESSAGE)), SEARCH_ENDPOINT, &SEARCH),
            fatal(invalid("HTTP 400 on /search"))
        );
    }

    #[test]
    fn another_400_message_becomes_the_detail_of_a_consultation() {
        assert_eq!(
            classify(Ok(piste_error(400, "Unknown facet DATE_X")), ENDPOINT, &CONSULT),
            fatal(invalid("Unknown facet DATE_X"))
        );
    }

    #[test]
    fn the_pista_message_of_a_search_400_never_reaches_the_detail() {
        // Le message d'un 400 de recherche peut citer le texte de la requête (04 §12.5) : le
        // détail, que le port destine aux journaux et à l'audit, ne le recopie jamais.
        let secret_context = ErrorContext { secrets: &["tok-0123456789"], ..SEARCH };
        for message in [
            "Unknown value texte-secret-42 in query",
            "Unknown facet DATE_X",
            "Bearer tok-0123456789 refused",
            "x",
            MARKER_MESSAGE,
        ] {
            for context in [SEARCH, secret_context] {
                assert_eq!(
                    classify(Ok(piste_error(400, message)), SEARCH_ENDPOINT, &context),
                    fatal(invalid("HTTP 400 on /search")),
                    "{message}"
                );
            }
        }
    }

    #[test]
    fn the_400_detail_is_repaired_cleaned_and_bounded() {
        let garbled = mojibake("Facette inconnue : é");
        assert_eq!(
            classify(Ok(piste_error(400, &garbled)), ENDPOINT, &CONSULT),
            fatal(invalid("Facette inconnue : é"))
        );
        let unrepaired = ErrorContext { repair_mojibake: false, ..CONSULT };
        assert_eq!(
            classify(Ok(piste_error(400, &garbled)), ENDPOINT, &unrepaired),
            fatal(invalid(&garbled))
        );
        assert_eq!(
            classify(Ok(piste_error(400, "line1\nline2\u{1b}[31m\u{202e}x")), ENDPOINT, &CONSULT),
            fatal(invalid("line1 line2 [31mx"))
        );
        assert_eq!(
            classify(Ok(piste_error(400, &"x".repeat(5_000))), ENDPOINT, &CONSULT),
            fatal(invalid(&"x".repeat(200)))
        );
    }

    #[test]
    fn a_400_message_with_nothing_visible_falls_back_to_the_generic_detail() {
        assert_eq!(
            classify(Ok(piste_error(400, " \n\u{200b}\u{1} ")), ENDPOINT, &CONSULT),
            fatal(invalid("HTTP 400 on /consult/juri"))
        );
    }

    #[test]
    fn the_token_endpoint_400_does_not_read_the_body() {
        assert_eq!(
            classify(Ok(piste_error(400, MARKER_MESSAGE)), "/oauth/token", &ErrorContext::TOKEN),
            fatal(invalid("HTTP 400 on /oauth/token"))
        );
    }

    #[test]
    fn the_400_detail_never_carries_a_known_secret() {
        let context = ErrorContext { secrets: &["tok-0123456789", "client-secret-xyz"], ..CONSULT };
        let detail_of = |message: &str| classify(Ok(piste_error(400, message)), ENDPOINT, &context);
        assert_eq!(
            detail_of("Bad Bearer tok-0123456789 / client-secret-xyz in tok-0123456789"),
            fatal(invalid("Bad Bearer [redacted] / [redacted] in [redacted]"))
        );
        // Le secret est retiré avant la coupure à 200 caractères, jamais à cheval sur elle.
        assert_eq!(
            detail_of(&format!("{}tok-0123456789", "y".repeat(195))),
            fatal(invalid(&format!("{}[reda", "y".repeat(195))))
        );
        // Un jeton coupé par un caractère invisible : l'expurgation suit le nettoyage.
        assert_eq!(detail_of("Bearer tok-01\u{200B}23456789"), fatal(invalid("Bearer [redacted]")));
    }

    #[test]
    fn a_rejected_400_is_logged_as_a_probable_bug_without_the_raw_body() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        let body = r#"{"message":"Facet\u0007 refused","extra":"raw-body-field"}"#;
        assert_eq!(
            classify(Ok(with_body(400, body)), ENDPOINT, &CONSULT),
            fatal(invalid("Facet refused"))
        );
        let log = capture.text();
        for expected in
            ["WARN", "probable bug", r#"endpoint="/consult/juri""#, "detail=Facet refused"]
        {
            assert!(log.contains(expected), "{expected} missing from {log:?}");
        }
        assert!(!log.contains("raw-body-field"), "{log:?}");
        assert!(!log.contains('\u{7}'), "{log:?}");
    }

    #[test]
    fn an_unknown_identifier_is_not_a_probable_bug() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        assert_eq!(
            classify(Ok(piste_error(400, MARKER_MESSAGE)), ENDPOINT, &CONSULT),
            fatal(not_found())
        );
        assert_eq!(capture.text(), "");
    }

    #[test]
    fn a_rejected_search_400_is_logged_without_the_pista_message() {
        let (capture, _guard) = capture_at(tracing::Level::TRACE);
        assert_eq!(
            classify(
                Ok(piste_error(400, "Unknown value texte-secret-42 in query")),
                SEARCH_ENDPOINT,
                &SEARCH
            ),
            fatal(invalid("HTTP 400 on /search"))
        );
        let log = capture.text();
        for expected in ["WARN", "probable bug", r#"endpoint="/search""#, "status=400"] {
            assert!(log.contains(expected), "{expected} missing from {log:?}");
        }
        assert!(!log.contains("texte-secret-42"), "{log:?}");
        assert!(!log.contains("detail"), "{log:?}");
    }

    #[test]
    fn a_404_on_a_consultation_of_the_official_host_with_a_non_html_body_is_not_found() {
        for body in [r#"{"status":404,"error":"Not Found"}"#, "", "Not Found", "{}"] {
            assert_eq!(
                classify(Ok(with_body(404, body)), ENDPOINT, &OFFICIAL),
                fatal(not_found()),
                "{body:?}"
            );
        }
    }

    #[test]
    fn a_404_html_page_is_config_even_on_the_official_host() {
        for body in [
            "<!DOCTYPE html><html>404</html>",
            "\n  <html>",
            r#"<?xml version="1.0"?>"#,
            "\u{feff}<html>",
            "\u{feff}  \r\n<!DOCTYPE html>",
        ] {
            assert_eq!(
                classify(Ok(with_body(404, body)), ENDPOINT, &OFFICIAL),
                fatal(api_endpoint(detail(404))),
                "{body:?}"
            );
        }
    }

    #[test]
    fn a_404_on_a_custom_host_is_config_whatever_the_body() {
        for body in ["", "{}", r#"{"status":404}"#, "<html></html>"] {
            assert_eq!(
                classify(Ok(with_body(404, body)), ENDPOINT, &CONSULT),
                fatal(api_endpoint(detail(404))),
                "{body:?}"
            );
        }
    }

    #[test]
    fn a_404_on_search_or_on_the_token_endpoint_is_config_even_on_the_official_host() {
        // La nature suit le genre d'endpoint passé par l'appelant, jamais le texte du chemin.
        let token = ErrorContext { official_host: true, ..ErrorContext::TOKEN };
        for (endpoint, context, kind) in [
            (SEARCH_ENDPOINT, SEARCH, ConfigKind::ApiEndpoint),
            ("/oauth/token", token, ConfigKind::OAuthEndpoint),
        ] {
            assert_eq!(
                classify(Ok(with_body(404, "{}")), endpoint, &context),
                fatal(CorpusError::Config { kind, detail: format!("HTTP 404 on {endpoint}") }),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn a_refusal_in_the_token_context_is_typed_as_coming_from_the_token_endpoint() {
        // `auth.rs` traite ces statuts avant `classify` ; le type reste juste s'ils l'atteignent.
        let endpoint = "PISTE OAuth";
        let token =
            |status: u16| classify(Ok(with_body(status, "{}")), endpoint, &ErrorContext::TOKEN);
        let refused = |status: u16| CorpusError::Auth {
            origin: AuthOrigin::Token,
            status: Some(status),
            detail: format!("HTTP {status} on {endpoint}"),
        };
        assert_eq!(token(401), Outcome::Unauthorized(refused(401)));
        assert_eq!(token(403), fatal(refused(403)));
        assert_eq!(
            token(405),
            fatal(CorpusError::Config {
                kind: ConfigKind::OAuthEndpoint,
                detail: format!("HTTP 405 on {endpoint}"),
            })
        );
    }

    #[test]
    fn the_other_statuses_are_never_not_found_even_with_the_marker() {
        let classified =
            |status| classify(Ok(piste_error(status, MARKER_MESSAGE)), ENDPOINT, &OFFICIAL);
        assert_eq!(classified(401), Outcome::Unauthorized(api_auth(401)));
        assert_eq!(classified(403), fatal(api_auth(403)));
        assert_eq!(classified(405), fatal(api_endpoint(detail(405))));
        for status in [410, 422] {
            assert_eq!(classified(status), fatal(upstream(status, false)), "{status}");
        }
        // Le 500 n'est jamais « introuvable » (règle 5).
        assert_eq!(
            classified(500),
            Outcome::Transient {
                error: upstream(500, false),
                retry_after: None,
                budget: Budget::Reduced,
            }
        );
        assert_eq!(classified(502), transient(upstream(502, true), None));
    }

    #[test]
    fn an_empty_2xx_body_on_a_consultation_is_not_found() {
        for status in [200, 201, 204] {
            for body in ["", "  \r\n\t"] {
                assert_eq!(
                    classify(Ok(with_body(status, body)), ENDPOINT, &CONSULT),
                    fatal(not_found()),
                    "{status} {body:?}"
                );
            }
        }
    }

    #[test]
    fn an_empty_2xx_body_on_search_is_a_decode_error() {
        for status in [200, 204] {
            assert_eq!(
                classify(Ok(with_body(status, "")), SEARCH_ENDPOINT, &SEARCH),
                fatal(CorpusError::Decode { detail: "empty response body on /search".to_owned() }),
                "{status}"
            );
        }
    }

    #[test]
    fn an_empty_2xx_body_on_the_token_endpoint_is_left_to_the_token_reader() {
        assert_eq!(
            classify(Ok(with_body(200, "")), "/oauth/token", &ErrorContext::TOKEN),
            Outcome::Success(with_body(200, ""))
        );
    }

    #[test]
    fn a_2xx_with_any_body_is_left_to_the_decoder() {
        for body in ["null", "{}", r#"{"text":null}"#, "x", "<html>"] {
            for context in [CONSULT, SEARCH] {
                assert_eq!(
                    classify(Ok(with_body(200, body)), ENDPOINT, &context),
                    Outcome::Success(with_body(200, body)),
                    "{body:?}"
                );
            }
        }
    }

    #[test]
    fn status_401_is_unauthorized_with_the_final_auth_error() {
        assert_eq!(
            classified(401),
            Outcome::Unauthorized(CorpusError::Auth {
                origin: AuthOrigin::Api,
                status: Some(401),
                detail: "HTTP 401 on /consult/juri".to_owned()
            })
        );
    }

    #[test]
    fn status_403_is_a_final_auth_error() {
        assert_eq!(
            classified(403),
            Outcome::Fatal(CorpusError::Auth {
                origin: AuthOrigin::Api,
                status: Some(403),
                detail: "HTTP 403 on /consult/juri".to_owned()
            })
        );
    }

    /// Page 403 de la passerelle Apache de PISTE, recopiée octet pour octet (199 octets) de la
    /// réponse enregistrée en sandbox le 2026-10-28 (`search/longueur/mot-unique-100`, `/search`
    /// avec un mot unique de 100 caractères, `Content-Type: text/html; charset=iso-8859-1`) :
    /// la page standard du serveur, sans donnée sensible.
    const GATEWAY_403_BODY: &str = "<!DOCTYPE HTML PUBLIC \"-//IETF//DTD HTML 2.0//EN\">\n\
        <html><head>\n\
        <title>403 Forbidden</title>\n\
        </head><body>\n\
        <h1>Forbidden</h1>\n\
        <p>You don't have permission to access this resource.</p>\n\
        </body></html>\n";

    /// 403 JSON d'une route inexistante, lu du fichier enregistré le 2026-10-28.
    const ROUTE_403_BODY: &str = include_str!(
        "../tests/fixtures/recorded/consult/inexistant/inconnu/route-inexistante.json"
    );

    fn gateway_403() -> RawResponse {
        RawResponse {
            content_type: Some("text/html; charset=iso-8859-1".to_owned()),
            ..with_body(403, GATEWAY_403_BODY)
        }
    }

    #[test]
    fn a_gateway_html_403_is_not_an_auth_error() {
        assert_eq!(GATEWAY_403_BODY.len(), 199);
        let rejected = CorpusError::InvalidRequest {
            status: Some(403),
            detail: "request rejected by the PISTE gateway".to_owned(),
        };
        for (endpoint, context) in
            [(SEARCH_ENDPOINT, &SEARCH), (ENDPOINT, &CONSULT), (ENDPOINT, &OFFICIAL)]
        {
            assert_eq!(
                classify(Ok(gateway_403()), endpoint, context),
                Outcome::Fatal(rejected.clone()),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn api_403_json_is_auth_api_with_status_403() {
        let auth = |endpoint: &str| {
            Outcome::Fatal(CorpusError::Auth {
                origin: AuthOrigin::Api,
                status: Some(403),
                detail: format!("HTTP 403 on {endpoint}"),
            })
        };
        // Le 403 d'une route inexistante, enregistré en sandbox (33 octets) : le corps de la
        // requête recopié (04 §11.1) ; puis un corps vide.
        for body in [ROUTE_403_BODY, ""] {
            let response = RawResponse {
                content_type: Some("application/json".to_owned()),
                ..with_body(403, body)
            };
            assert_eq!(classify(Ok(response), ENDPOINT, &CONSULT), auth(ENDPOINT), "{body:?}");
        }
    }

    #[test]
    fn statuses_404_and_405_are_config_never_not_found() {
        for status in [404, 405] {
            assert_eq!(classified(status), Outcome::Fatal(api_endpoint(detail(status))));
        }
    }

    #[test]
    fn other_4xx_are_final_upstream_errors() {
        for status in [402, 406, 408, 409, 410, 413, 422, 451, 499] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    #[test]
    fn status_429_is_transient_rate_limited_without_retry_after() {
        assert_eq!(
            classified(429),
            transient(CorpusError::RateLimited { retry_after: None }, None)
        );
    }

    #[test]
    fn status_429_carries_the_retry_after_in_both_places() {
        assert_eq!(
            classify(Ok(with_headers(429, Some("7"), None)), ENDPOINT, &CONSULT),
            transient(CorpusError::RateLimited { retry_after: Some(secs(7)) }, Some(secs(7)))
        );
    }

    #[test]
    fn the_retry_after_kept_in_the_429_error_is_capped_at_one_hour() {
        // (secondes annoncées, secondes rangées dans l'erreur)
        for (announced, kept) in [
            ("18446744073709551615", 3_600),
            ("99999999999999999999", 3_600),
            ("120", 120),
            ("3600", 3_600),
            ("3601", 3_600),
        ] {
            let raw = Some(Duration::from_secs(announced.parse().unwrap_or(u64::MAX)));
            assert_eq!(
                classify(Ok(with_headers(429, Some(announced), None)), ENDPOINT, &CONSULT),
                // La boucle de réessai garde la valeur annoncée pour décider d'attendre ou de
                // refuser ; seule la valeur propagée à l'appelant est écrêtée.
                transient(CorpusError::RateLimited { retry_after: Some(secs(kept)) }, raw),
                "{announced}"
            );
        }
    }

    #[test]
    fn status_500_is_transient_on_the_reduced_budget_with_its_final_error() {
        assert_eq!(
            classified(500),
            Outcome::Transient {
                error: upstream(500, false),
                retry_after: None,
                budget: Budget::Reduced,
            }
        );
    }

    #[test]
    fn retry_after_is_not_read_on_a_500() {
        assert_eq!(
            classify(Ok(with_headers(500, Some("7"), None)), ENDPOINT, &CONSULT),
            Outcome::Transient {
                error: upstream(500, false),
                retry_after: None,
                budget: Budget::Reduced,
            }
        );
    }

    #[test]
    fn statuses_502_503_504_are_transient_and_retryable() {
        for status in [502, 503, 504] {
            assert_eq!(classified(status), transient(upstream(status, true), None));
        }
    }

    #[test]
    fn statuses_502_503_504_honour_retry_after() {
        for status in [502, 503, 504] {
            assert_eq!(
                classify(Ok(with_headers(status, Some("12"), None)), ENDPOINT, &CONSULT),
                transient(upstream(status, true), Some(secs(12)))
            );
        }
    }

    #[test]
    fn retry_after_given_as_an_http_date_uses_the_date_header_of_the_same_response() {
        let reply = with_headers(429, Some("Sun, 06 Nov 1994 08:49:40 GMT"), Some(DATE));
        assert_eq!(
            classify(Ok(reply), ENDPOINT, &CONSULT),
            transient(CorpusError::RateLimited { retry_after: Some(secs(3)) }, Some(secs(3)))
        );
    }

    #[test]
    fn unusable_retry_after_is_ignored() {
        assert_eq!(
            classify(Ok(with_headers(503, Some("soon"), Some(DATE))), ENDPOINT, &CONSULT),
            transient(upstream(503, true), None)
        );
    }

    #[test]
    fn other_5xx_are_final_upstream_errors() {
        for status in [501, 505, 507, 511, 599, 600, 999] {
            assert_eq!(classified(status), Outcome::Fatal(upstream(status, false)));
        }
    }

    fn kind(outcome: &Outcome<RawResponse>) -> &'static str {
        match outcome {
            Outcome::Success(_) => "success",
            Outcome::Unauthorized(_) => "unauthorized",
            Outcome::Transient { .. } => "transient",
            Outcome::Fatal(_) => "fatal",
        }
    }

    #[test]
    fn every_status_has_exactly_the_class_of_the_table() {
        for status in 100..=999 {
            let expected = match status {
                200..=299 => "success",
                401 => "unauthorized",
                429 | 500 | 502..=504 => "transient",
                _ => "fatal",
            };
            assert_eq!(kind(&classified(status)), expected, "status {status}");
        }
    }

    #[test]
    fn transport_timeout_is_transient_timeout() {
        assert_eq!(
            classify(Err(TransportError::Timeout), ENDPOINT, &CONSULT),
            transient(CorpusError::Timeout, None)
        );
    }

    #[test]
    fn network_error_is_transient_and_retryable() {
        let detail = "network error (connect) on /consult/juri".to_owned();
        assert_eq!(
            classify(Err(TransportError::Network { detail: detail.clone() }), ENDPOINT, &CONSULT),
            transient(CorpusError::Upstream { status: None, retryable: true, detail }, None)
        );
    }

    #[test]
    fn tls_failure_is_final() {
        let detail = "TLS handshake failed on /consult/juri".to_owned();
        assert_eq!(
            classify(Err(TransportError::Tls { detail: detail.clone() }), ENDPOINT, &CONSULT),
            Outcome::Fatal(CorpusError::Upstream { status: None, retryable: false, detail })
        );
    }

    #[test]
    fn an_oversized_body_is_final_too_large_on_the_api_and_decode_on_the_token() {
        let too_large = |endpoint: &str| TransportError::TooLarge {
            detail: format!("response too large (> 16 MiB) on {endpoint}"),
        };
        let detail = |endpoint: &str| format!("response too large (> 16 MiB) on {endpoint}");
        for (endpoint, context) in [(ENDPOINT, CONSULT), (SEARCH_ENDPOINT, SEARCH)] {
            assert_eq!(
                classify(Err(too_large(endpoint)), endpoint, &context),
                Outcome::Fatal(CorpusError::TooLarge { detail: detail(endpoint) }),
                "{endpoint}"
            );
        }
        // Le corps du jeton au-delà de la borne n'est pas « un texte trop gros » : `Decode`.
        assert_eq!(
            classify(Err(too_large("PISTE OAuth")), "PISTE OAuth", &ErrorContext::TOKEN),
            Outcome::Fatal(CorpusError::Decode { detail: detail("PISTE OAuth") })
        );
    }

    // ---- Outcome::map --------------------------------------------------------------------

    fn plus_one(n: u8) -> u8 {
        n + 1
    }

    /// Fermeture qui garde `held` : elle tombe avec lui si `map` ne l'appelle pas.
    fn holding(held: Arc<()>) -> impl FnOnce(u8) -> (u8, Arc<()>) {
        move |n| (n, held)
    }

    #[test]
    fn map_transforms_a_success_and_copies_every_other_outcome() {
        assert_eq!(Outcome::Success(20).map(plus_one), Outcome::Success(21));
        assert_eq!(
            Outcome::Unauthorized(CorpusError::Timeout).map(plus_one),
            Outcome::Unauthorized(CorpusError::Timeout)
        );
        assert_eq!(
            Outcome::Transient {
                error: CorpusError::Timeout,
                retry_after: Some(secs(2)),
                budget: Budget::Reduced
            }
            .map(plus_one),
            Outcome::Transient {
                error: CorpusError::Timeout,
                retry_after: Some(secs(2)),
                budget: Budget::Reduced
            }
        );
        assert_eq!(
            Outcome::Fatal(CorpusError::Timeout).map(plus_one),
            Outcome::Fatal(CorpusError::Timeout)
        );
    }

    #[test]
    fn map_drops_its_closure_unused_outside_a_success() {
        let held = Arc::new(());
        let failed = Outcome::Fatal(CorpusError::Timeout).map(holding(Arc::clone(&held)));
        assert_eq!(Arc::strong_count(&held), 1);
        assert_eq!(failed, Outcome::Fatal(CorpusError::Timeout));
        let kept = Outcome::Success(7).map(holding(Arc::clone(&held)));
        assert_eq!(Arc::strong_count(&held), 2);
        assert_eq!(kept, Outcome::Success((7, held)));
    }
}
