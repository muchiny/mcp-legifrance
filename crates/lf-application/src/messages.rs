//! Textes d'erreur vus par le modèle : source unique de 05 §7.2 à §7.4 (02 §9.3).
//!
//! Chaque fonction est pure. `corpus_error` n'appelle jamais `Display` sur une `CorpusError` :
//! le message est rédigé à partir des seuls champs typés (variante, statut, `AuthOrigin`,
//! `ConfigKind`). Les cas d'usage ne l'appellent pas directement : ils passent par
//! `failure::port_failure`, qui journalise d'abord le `detail` pour l'opérateur.

use std::time::Duration;

use lf_domain::{DomainError, fond::Fond};

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
/// Il ne nomme que des outils exposés (`ToolName::ALL`, I-2) : `fr_get_text` lit un article,
/// `fr_search` sur le fond `CODE` les trouve.
const TOO_LARGE_FOR_GET_ACT: &str = "this text is too large for fr_get_act; read its articles \
                                     with fr_get_text, or find them with fr_search fond=CODE.";

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

/// Appel annulé par le client (ligne 21) : sans code, le message va à l'audit (le client a
/// renoncé à la réponse, et rmcp 3.5.1 ne l'envoie pas : spike 0A). Réservé à `Audited`, seul à
/// observer une annulation (m-4).
#[must_use]
pub(crate) fn cancelled() -> AppError {
    AppError::Upstream {
        message: "cancelled by client".to_owned(),
        cause: UpstreamCause::Cancelled,
    }
}

/// Échec d'écriture du journal en mode strict (ligne 22). Le paramètre `_error` n'est volontairement
/// pas lu : son `detail` peut citer un chemin local (00 §5, D-40) ; l'adapter d'audit le journalise
/// lui-même (plan 3c, span `audit_write`, 02 §11.1), `Audited` n'en journalise que le genre, et le
/// modèle ne le voit jamais. Le garder dans la signature interdit d'appeler cette fonction sans
/// échec réel.
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
        CorpusError::Upstream { status, retryable, detail: _ } => {
            upstream(tool, *status, *retryable)
        }
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
            "No text found for {subject}. Check the id, or search with fr_search fond=LODA or \
             CODE."
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

/// Suffixe d'un 500 ou 503 sur une recherche : le moteur les rend aussi pour une requête qu'il ne
/// sait pas lire (opérateur `AND` en fin de requête, `NOT` seul, guillemets), et la rejouer à
/// l'identique échoue de la même façon : on invite à reformuler, non à réessayer (L4-08, gap5-4).
const REPHRASE_SUFFIX: &str = "; if it repeats, rephrase the query or report it.";

/// `Upstream` (lignes 17 et 18). Sans statut, c'est une erreur réseau : la catégorie reste fixe,
/// `CorpusError::Upstream` ne la porte pas (05 §7.2 cite `TLS error` en exemple).
fn upstream(tool: ToolName, status: Option<u16>, retryable: bool) -> AppError {
    let is_search = matches!(tool, ToolName::FR_SEARCH | ToolName::FR_SEARCH_ALL);
    let suffix = if is_search && matches!(status, Some(500 | 503)) {
        REPHRASE_SUFFIX
    } else {
        retry_suffix(retryable)
    };
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

/// Longueur maximale, en caractères, d'une valeur de l'utilisateur recopiée dans un message
/// (E-27, 03 §2.7) : au-delà, la valeur est coupée et suivie de `…`.
const SHOWN_MAX_CHARS: usize = 64;

/// Plus grande borne d'un entier : une borne haute à `i64::MAX` s'écrit ouverte, `(1..)`.
const OPEN_ENDED: i64 = i64::MAX;

/// `value` telle que le modèle la lit entre apostrophes : coupée à 64 caractères (`…` ensuite),
/// barre oblique inverse, retours à la ligne, contrôles et séparateurs de ligne Unicode échappés.
fn shown(value: &str) -> String {
    let mut chars = value.chars();
    let mut out = String::new();
    for c in chars.by_ref().take(SHOWN_MAX_CHARS) {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') => {
                out.extend(c.escape_unicode());
            }
            c => out.push(c),
        }
    }
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// Argument absent (`text_id is required.`, parité P-012).
#[must_use]
pub fn required(field: &str) -> AppError {
    AppError::invalid_argument(format!("{field} is required."))
}

/// Entier hors de ses bornes incluses (`page_size=0 out of range (1..100).`, parité P-011).
/// Une borne haute à `i64::MAX` s'écrit ouverte : `page=0 out of range (1..).`
#[must_use]
pub fn out_of_range(field: &'static str, value: i64, min: i64, max: i64) -> AppError {
    let upper = if max == OPEN_ENDED { String::new() } else { max.to_string() };
    AppError::invalid_argument(format!("{field}={value} out of range ({min}..{upper})."))
}

/// Valeur hors d'une liste fermée (`match='or' must be one of ['all', 'any', 'exact'].`). La
/// valeur est recopiée par `shown` ; `allowed` s'écrit tel que donné, dans l'ordre reçu.
#[must_use]
pub fn not_one_of(field: &'static str, value: &str, allowed: &[&str]) -> AppError {
    let list = allowed.iter().map(|item| format!("'{item}'")).collect::<Vec<_>>().join(", ");
    AppError::invalid_argument(format!("{field}='{}' must be one of [{list}].", shown(value)))
}

/// Erreur du domaine rendue au modèle (03 §9, 05 §7.3), pour l'argument par défaut de l'outil :
/// l'identifiant de [`id_field`] et la date `date`. Un cas d'usage qui connaît l'argument fautif
/// (`date_from`, `version_date`...) appelle [`domain_arg`].
#[must_use]
pub fn domain(tool: ToolName, error: &DomainError) -> AppError {
    domain_arg(tool, None, error)
}

/// Comme [`domain`], en nommant l'argument fautif quand l'erreur ne le porte pas (`InvalidDate`,
/// `InvalidEcli`). Pour un identifiant, `arg` remplace le nom de [`id_field`].
///
/// La famille « entrée » de 03 §9 donne `invalid_arg`, avec la phrase d'aide qui cite l'outil à
/// employer. La famille « données amont » (`HitKindMismatch`, `TocTooDeep`, `TextTooLarge`) et
/// l'invariant de construction `EmptyCoverageGaps` ne sont pas une faute de l'utilisateur : ils
/// rendent le message de `Decode` (`upstream_error`). Le `match` est exhaustif : une variante
/// ajoutée au domaine casse la compilation.
#[must_use]
pub fn domain_arg(tool: ToolName, arg: Option<&'static str>, error: &DomainError) -> AppError {
    let id = arg.unwrap_or_else(|| id_field(tool));
    match error {
        DomainError::InvalidId { input, expected, .. } => {
            invalid_argument(invalid_id(id, input, expected))
        }
        DomainError::BadSuffix { input, .. } => invalid_argument(format!(
            "{id}='{}': invalid version suffix (expected _DD-MM-YYYY, e.g. _08-09-2023).",
            shown(input)
        )),
        DomainError::InvalidDate { input, .. } => invalid_argument(format!(
            "{}='{}' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15).",
            arg.unwrap_or("date"),
            shown(input)
        )),
        DomainError::InvalidEcli { input, .. } => invalid_argument(format!(
            "{}='{}' is not a valid ECLI (expected ECLI:FR:<court>:<year>:<ordinal>).",
            arg.unwrap_or("ecli"),
            shown(input)
        )),
        DomainError::UnknownFond { input, .. } => {
            let mut names = Fond::ALL.map(Fond::name);
            names.sort_unstable();
            not_one_of("fond", input, &names)
        }
        DomainError::EmptyQuery => required("query"),
        DomainError::QueryTooLong { len, max } => invalid_argument(format!(
            "query is too long ({len} characters, max {max}); shorten it or use match=\"all\"."
        )),
        DomainError::UnbalancedQuotes => invalid_argument(
            "query has an unbalanced double quote (\"): close it or remove it.".to_owned(),
        ),
        DomainError::OutOfRange { field, value, min, max } => {
            out_of_range(field, *value, *min, *max)
        }
        DomainError::InvertedDateRange { from, to } => {
            invalid_argument(format!("date_from={from} is after date_to={to}."))
        }
        DomainError::VersionDateNotApplicable { fond: _ } => invalid_argument(
            "version_date only applies to fond LODA or CODE; remove it, or use \
             date_from/date_to."
                .to_owned(),
        ),
        DomainError::DateFilterNotApplicable { fond } => {
            let hint = if fond.needs_version_date() { "; use version_date" } else { "" };
            invalid_argument(format!("date_from/date_to are not supported for fond={fond}{hint}."))
        }
        DomainError::PageBeyondReach { page, page_size, max } => invalid_argument(format!(
            "page={page} with page_size={page_size} goes beyond the {max} reachable results; \
             narrow the query with match or date_from/date_to."
        )),
        DomainError::OffsetPastEnd { offset, text_length } => invalid_argument(format!(
            "offset={offset} is past the end of the text (text_length={text_length}); restart \
             from offset=0 or omit offset."
        )),
        DomainError::FocusWithoutTerms => invalid_argument(
            "focus contains no searchable word (only stop words or punctuation); give a few \
             meaningful words or omit focus."
                .to_owned(),
        ),
        DomainError::TocOffsetPastEnd { offset, total } => invalid_argument(format!(
            "toc_offset={offset} is past the end of the table of contents (toc_total={total}); \
             restart from toc_offset=0."
        )),
        DomainError::NumberKindNotApplicable { fond, kind } => {
            invalid_argument(format!("{kind} is not a number kind of fond {fond}."))
        }
        DomainError::HitKindMismatch { .. }
        | DomainError::TocTooDeep { .. }
        | DomainError::TextTooLarge { .. }
        | DomainError::EmptyCoverageGaps => decode(),
    }
}

/// Nom de l'argument identifiant de l'outil (`text_id` pour `fr_get_act`...), `id` par défaut.
/// Les resources reprennent le nom de la variable de leur gabarit (05 §8.2).
#[must_use]
pub fn id_field(tool: ToolName) -> &'static str {
    match tool {
        ToolName::FR_GET_ACT | ToolName::RESOURCE_ACT => "text_id",
        ToolName::FR_GET_TEXT
        | ToolName::FR_DIFF_ARTICLE
        | ToolName::RESOURCE_ARTICLE
        | ToolName::RESOURCE_CODE_ARTICLE => "article_id",
        ToolName::FR_GET_DECISION | ToolName::RESOURCE_DECISION => "decision_id",
        ToolName::FR_GET_DELIBERATION => "deliberation_id",
        ToolName::FR_GET_CONVENTION => "convention_id",
        ToolName::FR_GET_COMPANY_AGREEMENT => "agreement_id",
        _ => "id",
    }
}

/// Ce que désigne un préfixe d'identifiant et l'outil qui le lit (table d'aide de 03 §2.7).
/// `JORFTEXT` a sa propre phrase (`jorf_hint`) : aucun outil ne le lit.
const PREFIX_HELP: [(&str, &str, &str); 9] = [
    ("LEGITEXT", "a whole text", ToolName::FR_GET_ACT.as_str()),
    ("LEGIARTI", "an article", ToolName::FR_GET_TEXT.as_str()),
    ("KALIARTI", "an article", ToolName::FR_GET_TEXT.as_str()),
    ("JURITEXT", "a court decision", ToolName::FR_GET_DECISION.as_str()),
    ("CONSTEXT", "a court decision", ToolName::FR_GET_DECISION.as_str()),
    ("CETATEXT", "a court decision", ToolName::FR_GET_DECISION.as_str()),
    ("CNILTEXT", "a CNIL deliberation", ToolName::FR_GET_DELIBERATION.as_str()),
    ("KALITEXT", "a collective agreement", ToolName::FR_GET_CONVENTION.as_str()),
    ("ACCOTEXT", "a company agreement", ToolName::FR_GET_COMPANY_AGREEMENT.as_str()),
];

/// Phrase d'aide pour un `JORFTEXT...` : texte d'origine du Journal officiel, que rien ne lit.
const JORF_HINT: &str = " JORFTEXT... is the original Journal officiel text: search it with \
                         fr_search fond=LODA and use the LEGITEXT id of the hit.";

/// Identifiant bien formé d'un exemple, par préfixe (`e.g.` des messages de chiffres).
const EXAMPLE_IDS: [(&str, &str); 7] = [
    ("LEGITEXT", "LEGITEXT000006070721"),
    ("LEGIARTI", "LEGIARTI000006419288"),
    ("JURITEXT", "JURITEXT000037999394"),
    ("CNILTEXT", "CNILTEXT000053352643"),
    ("KALITEXT", "KALITEXT000005652329"),
    ("ACCOTEXT", "ACCOTEXT000037731479"),
    ("KALIARTI", "KALIARTI000049886059"),
];

/// Préfixes admis lus dans `expected` (`LEGITEXT`, `LEGIARTI or KALIARTI`, `JURITEXT, CONSTEXT or
/// CETATEXT`) ; vide quand `expected` n'en liste pas (`8 letters`).
fn expected_prefixes(expected: &str) -> Vec<&str> {
    expected
        .split([',', ' '])
        .filter(|word| word.len() == 8 && word.bytes().all(|b| b.is_ascii_uppercase()))
        .collect()
}

/// Message d'un identifiant refusé (03 §2.7) : mauvaise famille, préfixe inconnu ou chiffres.
fn invalid_id(field: &str, input: &str, expected: &str) -> String {
    let prefixes = expected_prefixes(expected);
    let value = shown(input);
    let head: String = input.trim_start().chars().take(8).collect::<String>().to_ascii_uppercase();
    // Le bon préfixe suivi d'un reste qui n'est pas 12 chiffres.
    if prefixes.contains(&head.as_str()) {
        let example = prefixes.first().map_or_else(String::new, |prefix| {
            EXAMPLE_IDS.iter().find(|(known, _)| known == prefix).map_or_else(
                || format!("{prefix}000000000001"),
                |(_, example)| (*example).to_owned(),
            )
        });
        return format!(
            "{field}='{value}' must be {expected} followed by exactly 12 digits (e.g. {example})."
        );
    }
    let wanted = match prefixes.as_slice() {
        [] => {
            return format!(
                "{field}='{value}' must be a Legifrance identifier ({expected} followed by 12 \
                 digits)."
            );
        }
        [_, _, _, ..] => format!(
            "start with one of ({})",
            prefixes.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", ")
        ),
        [one] => format!("be a Legifrance {one}... identifier"),
        [first, second] => format!("be a Legifrance {first}... or {second}... identifier"),
    };
    let help = if head == "JORFTEXT" {
        JORF_HINT.to_owned()
    } else {
        PREFIX_HELP
            .iter()
            .find(|(prefix, ..)| *prefix == head)
            .map(|(_, what, tool)| {
                let then = if head == "LEGITEXT" && field == "article_id" {
                    ", then pick an article_id from its table of contents"
                } else {
                    ""
                };
                format!(" {head}... identifies {what}: use {tool}{then}.")
            })
            .unwrap_or_default()
    };
    format!("{field}='{value}' must {wanted}.{help}")
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
            let app = corpus_error(tool, ID, &error);
            assert_eq!(view(&app), (code, cause, message), "{error:?}");
        }
        // TooLarge sur fr_get_act : la consigne propre à C2 (04 §7.3), qui ne nomme que des
        // outils exposés (I-2).
        let too_large = CorpusError::TooLarge { detail: SENTINEL.to_owned() };
        let app = corpus_error(ToolName::FR_GET_ACT, ID, &too_large);
        assert_eq!(
            view(&app),
            (
                Some(UpstreamError),
                Some(UpstreamCause::Decode),
                "this text is too large for fr_get_act; read its articles with fr_get_text, or \
                 find them with fr_search fond=CODE."
            )
        );
    }

    #[test]
    fn too_large_hint_names_only_exposed_tools() {
        // Tout mot `fr_…` de la consigne est un outil de `ToolName::ALL` : le modèle ne doit
        // jamais être orienté vers un outil qui n'existe pas.
        let too_large = CorpusError::TooLarge { detail: String::new() };
        let app = corpus_error(ToolName::FR_GET_ACT, ID, &too_large);
        let named: Vec<&str> = app
            .message()
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| word.starts_with("fr_"))
            .collect();
        assert_eq!(named, ["fr_get_act", "fr_get_text", "fr_search"]);
        let exposed: Vec<&str> = ToolName::ALL.iter().map(|tool| tool.as_str()).collect();
        assert!(named.iter().all(|word| exposed.contains(word)), "{named:?}");
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
            let app = corpus_error(ToolName::FR_SEARCH, "", &auth(origin, status));
            assert_eq!(view(&app), (Some(ErrorCode::ConfigError), None, message.as_str()));
        }
    }

    #[test]
    fn not_found_messages_match_reference() {
        let rows = [
            (
                ToolName::FR_GET_ACT,
                "LEGITEXT000006070721",
                "No text found for LEGITEXT000006070721. Check the id, or search with fr_search \
                 fond=LODA or CODE.",
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
                let app = corpus_error(tool, id, &error);
                assert_eq!(view(&app), (Some(ErrorCode::NotFound), None, message), "{tool:?}");
            }
        }
        // La table couvre les six outils de lecture de la v0.1. Les sept autres n'y ont pas de
        // ligne : `fr_search`, `fr_coverage` et `fr_search_all` ne reçoivent jamais `NotFound`
        // (04 §11.2, test à part), et `fr_verify_citations`, `fr_resolve`, `fr_get_related` et
        // `fr_diff_article` écrivent leur propre `not_found` dans leur cas d'usage (05 §7.4).
        let covered: Vec<ToolName> = rows.iter().map(|row| row.0).collect();
        let elsewhere = [
            ToolName::FR_SEARCH,
            ToolName::FR_COVERAGE,
            ToolName::FR_SEARCH_ALL,
            ToolName::FR_VERIFY_CITATIONS,
            ToolName::FR_RESOLVE,
            ToolName::FR_GET_RELATED,
            ToolName::FR_DIFF_ARTICLE,
        ];
        for tool in ToolName::ALL {
            assert!(
                covered.contains(&tool) || elsewhere.contains(&tool),
                "{tool:?} sans message not_found"
            );
        }
    }

    #[test]
    fn search_and_coverage_never_receive_not_found_and_fall_back_to_decode() {
        for tool in [ToolName::FR_SEARCH, ToolName::FR_COVERAGE] {
            let error = CorpusError::NotFound { what: "x".to_owned() };
            let app = corpus_error(tool, "", &error);
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
            let app = corpus_error(ToolName::FR_GET_DECISION, subject, &error);
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
            corpus_error(act, uri, &config(ConfigKind::CredentialsMissing)),
            config_error(
                "PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET \
                 (from your piste.gouv.fr application, section OAuth Credentials)."
            )
        );
        // S7 : OAuth 200 sans access_token.
        assert_eq!(
            corpus_error(act, uri, &config(ConfigKind::OAuthResponse)),
            config_error("PISTE OAuth returned no access_token.")
        );
        // S8 : OAuth 4xx, puis OAuth 404.
        assert_eq!(
            corpus_error(act, uri, &auth(AuthOrigin::Token, Some(401))),
            AppError::config(rejected(401))
        );
        assert_eq!(
            corpus_error(act, uri, &config(ConfigKind::OAuthEndpoint)),
            config_error("PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.")
        );
        // S9 : API 404 de ressource, puis 404 de routage.
        assert_eq!(
            corpus_error(act, uri, &CorpusError::NotFound { what: uri.to_owned() }),
            AppError::not_found(
                "No text found for LEGITEXT000006070721. Check the id, or search with fr_search \
                 fond=LODA or CODE."
            )
        );
        assert_eq!(
            corpus_error(act, uri, &config(ConfigKind::ApiEndpoint)),
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
            corpus_error(ToolName::FR_SEARCH, "", &search_400),
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
            corpus_error(act, uri, &consult_400),
            expected_upstream(
                Status(400),
                "Legifrance rejected the request (HTTP 400): HTTP 400 on /consult/lawDecree"
            )
        );
        assert_eq!(
            corpus_error(act, uri, &auth(AuthOrigin::Api, Some(403))),
            config_error(
                "PISTE denied access (HTTP 403): check that the PISTE application is subscribed \
                 to the Legifrance API."
            )
        );
        // S11 : 502/503/504 puis 500 puis 429.
        assert_eq!(
            corpus_error(act, uri, &upstream(Some(503), true)),
            expected_upstream(
                Status(503),
                "Legifrance/PISTE HTTP 503 (service unavailable); retry once, then report it."
            )
        );
        assert_eq!(
            corpus_error(act, uri, &upstream(Some(500), false)),
            expected_upstream(
                Status(500),
                "Legifrance/PISTE HTTP 500 (internal server error); report it."
            )
        );
        assert_eq!(
            corpus_error(
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
            corpus_error(act, uri, &auth(AuthOrigin::Api, Some(401))),
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
            corpus_error(act, uri, &upstream(None, true)),
            expected_upstream(
                Network,
                "could not reach Legifrance/PISTE (connection failed); retry once, then report it."
            )
        );
        assert_eq!(
            corpus_error(act, uri, &CorpusError::Timeout),
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
        // S14 : 200 à corps non JSON, puis corps au-delà de la borne sur fr_get_act (C2).
        assert_eq!(
            corpus_error(act, uri, &CorpusError::Decode { detail: "expected value".into() }),
            expected_upstream(Decode, "unexpected response from Legifrance; report it.")
        );
        assert_eq!(
            corpus_error(act, uri, &CorpusError::TooLarge { detail: "> 16 MiB".into() }),
            expected_upstream(
                Decode,
                "this text is too large for fr_get_act; read its articles with fr_get_text, or \
                 find them with fr_search fond=CODE."
            )
        );
        // S15 : 200 à charge vide ⇒ première phrase de la référence, puis l'aide.
        let empty = CorpusError::NotFound { what: "empty payload".into() };
        assert_eq!(
            corpus_error(act, uri, &empty),
            AppError::not_found(
                "No text found for LEGITEXT000006070721. Check the id, or search with fr_search \
                 fond=LODA or CODE."
            )
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
            let app = corpus_error(ToolName::FR_GET_TEXT, ID, &error);
            assert_eq!(app.message().contains(hint), retryable, "{error:?}");
        }
    }

    #[test]
    fn a_500_or_503_on_a_search_asks_to_rephrase_instead_of_retrying() {
        // E2E search_all-1 et gap5-4 : « travail AND », « NOT », un guillemet orphelin.
        for tool in [ToolName::FR_SEARCH, ToolName::FR_SEARCH_ALL] {
            let app = corpus_error(tool, "", &upstream(Some(503), true));
            assert_eq!(
                app,
                expected_upstream(
                    UpstreamCause::Status(503),
                    "Legifrance/PISTE HTTP 503 (service unavailable); if it repeats, rephrase \
                     the query or report it."
                )
            );
            let app = corpus_error(tool, "", &upstream(Some(500), false));
            assert_eq!(
                app,
                expected_upstream(
                    UpstreamCause::Status(500),
                    "Legifrance/PISTE HTTP 500 (internal server error); if it repeats, \
                     rephrase the query or report it."
                )
            );
        }
    }

    #[test]
    fn other_search_failures_and_other_tools_keep_their_usual_suffix() {
        // Jumeau : seuls 500 et 503 d'une recherche changent.
        for (status, message) in [
            (502, "Legifrance/PISTE HTTP 502 (bad gateway); retry once, then report it."),
            (504, "Legifrance/PISTE HTTP 504 (gateway timeout); retry once, then report it."),
        ] {
            let app = corpus_error(ToolName::FR_SEARCH, "", &upstream(Some(status), true));
            assert_eq!(app.message(), message);
        }
        let timeout = corpus_error(ToolName::FR_SEARCH_ALL, "", &CorpusError::Timeout);
        assert_eq!(
            timeout.message(),
            "Legifrance/PISTE did not answer in time; retry once, then report it."
        );
        let network = corpus_error(ToolName::FR_SEARCH, "", &upstream(None, true));
        assert!(network.message().ends_with("; retry once, then report it."));
        let consult = corpus_error(ToolName::FR_GET_TEXT, ID, &upstream(Some(503), true));
        assert_eq!(
            consult.message(),
            "Legifrance/PISTE HTTP 503 (service unavailable); retry once, then report it."
        );
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
            let app = corpus_error(ToolName::FR_GET_ACT, ID, &upstream(Some(status), false));
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
            let app =
                corpus_error(ToolName::FR_SEARCH, "", &CorpusError::RateLimited { retry_after });
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
        errors.push(CorpusError::NotFound { what: SENTINEL.to_owned() });
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
                let app = corpus_error(tool, ID, error);
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
            let app = corpus_error(ToolName::FR_GET_ACT, ID, &error);
            let kept = "é".repeat(200);
            assert!(app.message().contains(&kept), "{status:?}");
            assert!(!app.message().contains(&"é".repeat(201)), "{status:?}");
        }
        // Un détail de 200 caractères pile passe entier.
        let exact = "x".repeat(200);
        let error = CorpusError::InvalidRequest { status: Some(400), detail: exact.clone() };
        let app = corpus_error(ToolName::FR_GET_ACT, ID, &error);
        assert_eq!(app.message(), format!("Legifrance rejected the request (HTTP 400): {exact}"));
    }

    #[test]
    fn config_settings_detail_is_clipped_to_500_chars() {
        let long = "é".repeat(600);
        let error = CorpusError::Config { kind: ConfigKind::Settings, detail: long };
        let app = corpus_error(ToolName::FR_GET_ACT, ID, &error);
        assert_eq!(app, AppError::config("é".repeat(500)));
        let exact = "x".repeat(500);
        let error = CorpusError::Config { kind: ConfigKind::Settings, detail: exact.clone() };
        assert_eq!(corpus_error(ToolName::FR_GET_ACT, ID, &error), AppError::config(exact));
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

    // ---- aides d'argument et erreurs du domaine (vague 0, unité 0C) ----

    use lf_domain::{
        date::VersionDate,
        ecli::Ecli,
        entity::SearchHit,
        id::{AnyId, ArticleId, DecisionId, DocumentId, KaliTextId, LegiTextId},
    };

    fn invalid(message: &str) -> AppError {
        AppError::invalid_argument(message)
    }

    /// Message rendu par `domain` pour l'outil `tool`.
    fn domain_text(tool: ToolName, error: &DomainError) -> String {
        let app = domain(tool, error);
        assert_eq!(app.code(), Some(ErrorCode::InvalidArg), "{error}");
        app.message().to_owned()
    }

    #[test]
    fn required_names_the_argument() {
        assert_eq!(required("text_id"), invalid("text_id is required."));
    }

    #[test]
    fn out_of_range_matches_the_reference_wording() {
        assert_eq!(
            out_of_range("page_size", 0, 1, 100),
            invalid("page_size=0 out of range (1..100).")
        );
        assert_eq!(
            out_of_range("max_chars", -5, 500, 40_000),
            invalid("max_chars=-5 out of range (500..40000).")
        );
        // Borne haute ouverte : `(1..)`.
        assert_eq!(out_of_range("page", 0, 1, i64::MAX), invalid("page=0 out of range (1..)."));
    }

    #[test]
    fn not_one_of_lists_the_allowed_values_in_the_given_order() {
        assert_eq!(
            not_one_of("match", "or", &["all", "any", "exact"]),
            invalid("match='or' must be one of ['all', 'any', 'exact'].")
        );
        assert_eq!(
            not_one_of("detail", "long", &["full", "summary"]).message(),
            "detail='long' must be one of ['full', 'summary']."
        );
    }

    #[test]
    fn shown_escapes_controls_and_cuts_at_64_characters() {
        assert_eq!(shown("a\nb\tc\rd"), r"a\nb\tc\rd");
        assert_eq!(shown("a\\b"), r"a\\b");
        assert_eq!(shown("a\u{1b}\u{0}b"), r"a\u{1b}\u{0}b");
        // Séparateurs de ligne et de paragraphe Unicode : échappés aussi.
        assert_eq!(shown("a\u{2028}b\u{2029}"), r"a\u{2028}b\u{2029}");
        // Accents et emoji passent tels quels.
        assert_eq!(shown("é😀"), "é😀");
        // 64 caractères : intacts ; 65 : coupés et suivis de `…`.
        assert_eq!(shown(&"x".repeat(64)), "x".repeat(64));
        assert_eq!(shown(&"x".repeat(65)), format!("{}…", "x".repeat(64)));
        assert_eq!(shown(""), "");
        // La coupe précède l'échappement : 64 caractères d'entrée, quel que soit le rendu.
        assert_eq!(shown(&"\n".repeat(65)), format!("{}…", r"\n".repeat(64)));
        assert!(!not_one_of("fond", "x\ny", &["A"]).message().contains('\n'));
    }

    #[test]
    fn id_errors_follow_the_table_of_03_section_2_7() {
        let act = ToolName::FR_GET_ACT;
        // Mauvaise famille, avec l'outil à employer.
        let wrong_family = LegiTextId::parse("JURITEXT000051743650").unwrap_err();
        assert_eq!(
            domain_text(act, &wrong_family),
            "text_id='JURITEXT000051743650' must be a Legifrance LEGITEXT... identifier. \
             JURITEXT... identifies a court decision: use fr_get_decision."
        );
        let whole_text = ArticleId::parse("LEGITEXT000006070721").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_GET_TEXT, &whole_text),
            "article_id='LEGITEXT000006070721' must be a Legifrance LEGIARTI... or KALIARTI... \
             identifier. LEGITEXT... identifies a whole text: use fr_get_act, then pick an \
             article_id from its table of contents."
        );
        // Le renvoi vers la table des matières ne vaut que pour `article_id`.
        assert_eq!(
            domain_text(ToolName::FR_GET_RELATED, &AnyId::parse("x").unwrap_err()),
            "id='x' must be a Legifrance identifier (8 letters followed by 12 digits)."
        );
        let deliberation = DecisionId::parse("CNILTEXT000053352643").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_GET_DECISION, &deliberation),
            "decision_id='CNILTEXT000053352643' must start with one of ('JURITEXT', \
             'CONSTEXT', 'CETATEXT'). CNILTEXT... identifies a CNIL deliberation: use \
             fr_get_deliberation."
        );
        let convention = KaliTextId::parse("LEGITEXT000006070721").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_GET_CONVENTION, &convention),
            "convention_id='LEGITEXT000006070721' must be a Legifrance KALITEXT... identifier. \
             LEGITEXT... identifies a whole text: use fr_get_act."
        );
        // Préfixe inconnu : pas de phrase d'aide.
        let unknown = KaliTextId::parse("KALICONT000005635096").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_GET_CONVENTION, &unknown),
            "convention_id='KALICONT000005635096' must be a Legifrance KALITEXT... identifier."
        );
        // Journal officiel : aucun outil ne le lit.
        let jorf = LegiTextId::parse("JORFTEXT000033202746").unwrap_err();
        assert_eq!(
            domain_text(act, &jorf),
            "text_id='JORFTEXT000033202746' must be a Legifrance LEGITEXT... identifier. \
             JORFTEXT... is the original Journal officiel text: search it with fr_search \
             fond=LODA and use the LEGITEXT id of the hit."
        );
    }

    #[test]
    fn id_errors_with_the_right_prefix_ask_for_twelve_digits() {
        let cases = [
            (
                ToolName::FR_GET_ACT,
                LegiTextId::parse("LEGITEXT0001").unwrap_err(),
                "text_id='LEGITEXT0001' must be LEGITEXT followed by exactly 12 digits (e.g. \
              LEGITEXT000006070721).",
            ),
            (
                ToolName::FR_GET_TEXT,
                ArticleId::parse("legiarti0001").unwrap_err(),
                "article_id='legiarti0001' must be LEGIARTI or KALIARTI followed by exactly 12 \
              digits (e.g. LEGIARTI000006419288).",
            ),
            (
                ToolName::FR_GET_DECISION,
                DecisionId::parse("CETATEXT12").unwrap_err(),
                "decision_id='CETATEXT12' must be JURITEXT, CONSTEXT or CETATEXT followed by exactly \
              12 digits (e.g. JURITEXT000037999394).",
            ),
            (
                ToolName::FR_GET_COMPANY_AGREEMENT,
                lf_domain::id::AccoTextId::parse("ACCOTEXT1").unwrap_err(),
                "agreement_id='ACCOTEXT1' must be ACCOTEXT followed by exactly 12 digits (e.g. \
              ACCOTEXT000037731479).",
            ),
        ];
        for (tool, error, expected) in cases {
            assert_eq!(domain_text(tool, &error), expected);
        }
    }

    #[test]
    fn long_prefix_lists_and_unknown_letters_are_rendered_without_help() {
        let foo = DocumentId::parse("FOOTEXT000000000001").unwrap_err();
        let message = domain_text(ToolName::FR_GET_RELATED, &foo);
        assert!(
            message.starts_with(
                "id='FOOTEXT000000000001' must start with one of ('LEGITEXT', 'LEGIARTI', "
            ),
            "{message}"
        );
        assert!(message.ends_with("')."), "{message}");
        // Une liste de deux préfixes sans exemple connu : l'exemple est fabriqué à partir du premier.
        let scta = lf_domain::id::SectionId::parse("LEGISCTA1").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_GET_ACT, &scta),
            "text_id='LEGISCTA1' must be LEGISCTA or KALISCTA followed by exactly 12 digits (e.g. \
             LEGISCTA000000000001)."
        );
    }

    #[test]
    fn bad_suffix_cites_the_whole_identifier() {
        let error = LegiTextId::parse("LEGITEXT000033205014_31-02-2023").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_GET_ACT, &error),
            "text_id='LEGITEXT000033205014_31-02-2023': invalid version suffix (expected \
             _DD-MM-YYYY, e.g. _08-09-2023)."
        );
    }

    #[test]
    fn argument_names_follow_the_tool_or_the_caller() {
        let date = VersionDate::parse("2016-13-45").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_GET_ACT, &date),
            "date='2016-13-45' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15)."
        );
        let named = domain_arg(ToolName::FR_SEARCH, Some("date_from"), &date);
        assert_eq!(
            named.message(),
            "date_from='2016-13-45' is not a valid date (expected YYYY-MM-DD, e.g. 2026-10-15)."
        );
        let ecli = Ecli::parse("x").unwrap_err();
        assert_eq!(
            domain_text(ToolName::FR_RESOLVE, &ecli),
            "ecli='x' is not a valid ECLI (expected ECLI:FR:<court>:<year>:<ordinal>)."
        );
        assert_eq!(
            domain_arg(ToolName::FR_RESOLVE, Some("citation"), &ecli).message(),
            "citation='x' is not a valid ECLI (expected ECLI:FR:<court>:<year>:<ordinal>)."
        );
        // Pour un identifiant, l'argument de l'appelant remplace celui de l'outil.
        let id = LegiTextId::parse("LEGITEXT0001").unwrap_err();
        assert!(
            domain_arg(ToolName::FR_DIFF_ARTICLE, Some("id"), &id).message().starts_with("id='")
        );
        assert!(domain(ToolName::FR_DIFF_ARTICLE, &id).message().starts_with("article_id='"));
    }

    #[test]
    fn id_field_names_the_identifier_argument_of_each_tool() {
        let rows = [
            (ToolName::FR_GET_ACT, "text_id"),
            (ToolName::FR_GET_TEXT, "article_id"),
            (ToolName::FR_GET_DECISION, "decision_id"),
            (ToolName::FR_GET_DELIBERATION, "deliberation_id"),
            (ToolName::FR_GET_CONVENTION, "convention_id"),
            (ToolName::FR_GET_COMPANY_AGREEMENT, "agreement_id"),
            (ToolName::FR_GET_RELATED, "id"),
            (ToolName::FR_DIFF_ARTICLE, "article_id"),
            (ToolName::RESOURCE_ARTICLE, "article_id"),
            (ToolName::RESOURCE_CODE_ARTICLE, "article_id"),
            (ToolName::RESOURCE_DECISION, "decision_id"),
            (ToolName::RESOURCE_ACT, "text_id"),
            (ToolName::FR_SEARCH, "id"),
            (ToolName::FR_COVERAGE, "id"),
            (ToolName::RESOURCE_COVERAGE, "id"),
        ];
        for (tool, field) in rows {
            assert_eq!((tool.as_str(), id_field(tool)), (tool.as_str(), field));
        }
    }

    #[test]
    fn input_family_domain_errors_have_the_texts_of_05_section_7_3() {
        let tool = ToolName::FR_SEARCH;
        let from = VersionDate::parse("2026-01-01").unwrap();
        let to = VersionDate::parse("2025-01-01").unwrap();
        let rows = [
            (
                Fond::parse("XYZ").unwrap_err(),
                "fond='XYZ' must be one of ['ACCO', 'CETAT', 'CNIL', 'CODE', 'CONSTIT', 'JURI', \
                 'KALI', 'LODA'].",
            ),
            (DomainError::EmptyQuery, "query is required."),
            (
                DomainError::UnbalancedQuotes,
                "query has an unbalanced double quote (\"): close it or remove it.",
            ),
            (
                DomainError::QueryTooLong { len: 612, max: 450 },
                "query is too long (612 characters, max 450); shorten it or use match=\"all\".",
            ),
            (
                DomainError::OutOfRange { field: "page_size", value: 0, min: 1, max: 100 },
                "page_size=0 out of range (1..100).",
            ),
            (
                DomainError::InvertedDateRange { from, to },
                "date_from=2026-01-01 is after date_to=2025-01-01.",
            ),
            (
                DomainError::VersionDateNotApplicable { fond: Fond::Juri },
                "version_date only applies to fond LODA or CODE; remove it, or use \
                 date_from/date_to.",
            ),
            (
                DomainError::DateFilterNotApplicable { fond: Fond::Code },
                "date_from/date_to are not supported for fond=CODE; use version_date.",
            ),
            (
                DomainError::DateFilterNotApplicable { fond: Fond::Juri },
                "date_from/date_to are not supported for fond=JURI.",
            ),
            (
                DomainError::PageBeyondReach { page: 200, page_size: 100, max: 10_000 },
                "page=200 with page_size=100 goes beyond the 10000 reachable results; narrow the \
                 query with match or date_from/date_to.",
            ),
            (
                DomainError::OffsetPastEnd { offset: 20_000, text_length: 14_332 },
                "offset=20000 is past the end of the text (text_length=14332); restart from \
                 offset=0 or omit offset.",
            ),
            (
                DomainError::FocusWithoutTerms,
                "focus contains no searchable word (only stop words or punctuation); give a few \
                 meaningful words or omit focus.",
            ),
            (
                DomainError::TocOffsetPastEnd { offset: 300, total: 88 },
                "toc_offset=300 is past the end of the table of contents (toc_total=88); \
                 restart from toc_offset=0.",
            ),
            (
                DomainError::NumberKindNotApplicable { fond: Fond::Cnil, kind: "ecli" },
                "ecli is not a number kind of fond CNIL.",
            ),
        ];
        for (error, expected) in rows {
            assert_eq!(domain_text(tool, &error), expected);
        }
    }

    #[test]
    fn upstream_family_domain_errors_are_decode_failures() {
        // `HitKindMismatch` ne se construit pas hors du domaine : on le provoque.
        let mismatch =
            SearchHit::new(Fond::Juri, DocumentId::parse("LEGITEXT000006070721").unwrap(), None)
                .unwrap_err();
        assert!(
            mismatch
                .to_string()
                .starts_with("hit identifier \"LEGITEXT000006070721\" does not belong")
        );
        for error in [
            mismatch,
            DomainError::TocTooDeep { max: 64 },
            DomainError::TextTooLarge { chars: 16_000_001 },
            DomainError::EmptyCoverageGaps,
        ] {
            let app = domain(ToolName::FR_GET_ACT, &error);
            assert_eq!(
                view(&app),
                (
                    Some(ErrorCode::UpstreamError),
                    Some(UpstreamCause::Decode),
                    "unexpected response from Legifrance; report it."
                ),
                "{error}"
            );
        }
    }
}
