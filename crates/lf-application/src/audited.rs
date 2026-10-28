//! Décorateur `Audited` : chaque exécution d'un outil est journalisée (02 §7.1 ; D-02, D-03).
//!
//! [`Audited::run`] enveloppe l'exécution COMPLÈTE d'un cas d'usage (décodage, validation, appels
//! au port) : il pose la seule garde dure du délai global (`tokio::time::timeout`, D-14), publie
//! l'échéance dans [`Deadline`] (00 §3), fait gagner l'annulation du client (`select!` biaisé),
//! mesure la durée sur l'horloge de `tokio` (suspendue en test, D-03), puis écrit une ligne
//! d'audit et attend son accusé (D-16) avant de rendre la main. Le mode ([`AuditMode`]) décide du
//! sort du résultat quand l'écriture échoue : retenu (`Strict`) ou rendu et compté (`BestEffort`).
//!
//! Journaux (02 §11) : un span `tool_call` par appel, dont les champs sont renseignés après coup,
//! et un événement de fin, en `info` pour un succès, une annulation et les erreurs attendues
//! (`invalid_arg`, `not_found`, D-08), en `warn` pour les autres. Aucune valeur d'argument ni
//! aucun `detail` n'y entre : `input_bytes` est une longueur, et l'échec d'écriture n'est nommé
//! que par son genre (02 §11.3).

use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use chrono::{DateTime, Utc};
use tokio::time::Instant;
use tracing::{Instrument, Span, field};

use crate::{
    deadline::Deadline,
    error::{AppError, ErrorCode},
    input::CanonicalInput,
    messages,
    ports::{AuditLog, AuditRecord, AuditStatus, PisteEnvironment},
    tools::ToolName,
};

/// Longueur maximale, en caractères, du champ `error` d'une ligne d'audit (00 §7, 02 §5.2).
const AUDIT_ERROR_MAX_CHARS: usize = 500;

/// Métrique d'audit d'une sortie d'outil (00 §7) : nombre de résultats, ou caractères renvoyés.
pub trait AuditMetric {
    /// Valeur inscrite dans `output_count_or_size` quand l'appel aboutit.
    fn output_count_or_size(&self) -> u64;
}

/// Ce qu'`Audited` sait d'un appel avant de l'exécuter : l'outil, l'heure d'entrée et les
/// arguments canoniques, recopiés tels quels dans la ligne d'audit.
#[derive(Debug, Clone)]
pub struct CallMeta {
    tool: ToolName,
    ts: DateTime<Utc>,
    canonical: CanonicalInput,
}

impl CallMeta {
    /// `ts` est `Clock::now_utc()` à l'entrée de la façade ; `canonical` vient du `ToolCall`.
    #[must_use]
    pub fn new(tool: ToolName, ts: DateTime<Utc>, canonical: CanonicalInput) -> Self {
        Self { tool, ts, canonical }
    }
}

/// Sort du résultat quand la ligne d'audit ne peut pas être écrite (02 §9.2, lignes 22 et 23).
///
/// Pas de `Default` : `lf-app` pose `Strict` (plan 3c), aucun défaut ne se cache ici.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditMode {
    /// Le résultat est retenu : l'appel rend `audit_error`, en citant le code d'origine.
    Strict,
    /// Le résultat est rendu ; l'échec est journalisé (`warn`) et compté.
    BestEffort,
}

/// Réglages d'[`Audited`], tous fournis par `lf-app`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditPolicy {
    /// Sort du résultat quand l'écriture échoue.
    pub mode: AuditMode,
    /// `LF_TOOL_DEADLINE_S`, garde dure de chaque appel. Au plus 600 s (borne posée par `lf-app`) :
    /// [`crate::deadline::Deadline::after`] ne déborde pas.
    pub deadline: Duration,
    /// Environnement PISTE, recopié dans chaque ligne.
    pub env: PisteEnvironment,
    /// Version du serveur, recopiée dans chaque ligne.
    pub server_version: &'static str,
}

/// Décorateur d'exécution : délai global, annulation et ligne d'audit pour chaque appel d'outil.
pub struct Audited<A: AuditLog> {
    log: A,
    policy: AuditPolicy,
    /// Échecs d'écriture du mode `BestEffort` (02 §9.2 ligne 23) : en mémoire, jamais exporté
    /// hors du processus.
    write_failures: AtomicU64,
}

impl<A: AuditLog> Audited<A> {
    /// Décorateur qui écrit dans `log` selon `policy`.
    #[must_use]
    pub fn new(log: A, policy: AuditPolicy) -> Self {
        Self { log, policy, write_failures: AtomicU64::new(0) }
    }

    /// Nombre d'écritures d'audit échouées en mode `BestEffort` depuis la création.
    #[must_use]
    pub fn write_failures(&self) -> u64 {
        self.write_failures.load(Ordering::Relaxed)
    }

    /// Exécute `work` sous le délai global, écrit la ligne d'audit de son issue et ne rend le
    /// résultat qu'une fois l'écriture accusée (D-16).
    ///
    /// `work` est l'exécution complète du cas d'usage, décodage et validation compris : un rejet
    /// local est audité comme un échec du port (D-02). Il ne lance aucune tâche (`tokio::spawn`) :
    /// l'échéance publiée ne la suivrait pas (02 §3.2, piège 5). `cancel` se résout quand le
    /// client annule (`ct.cancelled()` de rmcp) ; il l'emporte sur un résultat ou un délai échus
    /// au même instant, et abandonner `work` abandonne en cascade la requête en vol.
    ///
    /// Une panique de `work` traverse `run` sans ligne d'audit : aucune issue de 06 §2.2 ne la
    /// décrit, et la frontière de tâche (une tâche par requête, rmcp) la recueille.
    ///
    /// # Errors
    ///
    /// - L'erreur de `work`, telle quelle.
    /// - `upstream_error` ([`crate::error::UpstreamCause::Deadline`]) au-delà de `policy.deadline`.
    /// - `Upstream { cause: Cancelled }`, sans code, après une annulation : rmcp jette la réponse.
    /// - `audit_error` en mode `Strict` quand la ligne ne peut pas être écrite (hors annulation),
    ///   en citant le code d'origine s'il y en avait un : le résultat est retenu.
    pub async fn run<O, W, X>(&self, meta: CallMeta, cancel: X, work: W) -> Result<O, AppError>
    where
        O: AuditMetric,
        W: Future<Output = Result<O, AppError>> + Send,
        X: Future<Output = ()> + Send,
    {
        // Les champs de l'issue sont renseignés après coup (02 §11.1) ; `input_bytes` est une
        // longueur : la valeur des arguments n'est jamais journalisée (02 §11.3).
        let span = tracing::info_span!(
            "tool_call",
            tool = meta.tool.as_str(),
            input_bytes = meta.canonical.as_str().len(),
            status = field::Empty,
            error_code = field::Empty,
            duration_ms = field::Empty,
            output = field::Empty,
        );
        self.audit(meta, cancel, work).instrument(span).await
    }

    /// Corps de [`Audited::run`], exécuté dans le span `tool_call` (ses événements, comme ceux de
    /// `work`, y sont rattachés).
    async fn audit<O, W, X>(&self, meta: CallMeta, cancel: X, work: W) -> Result<O, AppError>
    where
        O: AuditMetric,
        W: Future<Output = Result<O, AppError>> + Send,
        X: Future<Output = ()> + Send,
    {
        // Horloge monotone de `tokio`, suspendue dans les tests (`start_paused`, 02 §3.2 piège 3).
        let started = Instant::now();
        let deadline = self.policy.deadline;
        // L'échéance publiée est celle de la garde dure : un adapter qui la lit comme durée
        // restante ne programme aucune tentative au-delà (00 §3).
        let work = Deadline::after(deadline).scope(work);
        let outcome = tokio::select! {
            // Biaisé : quand tout est prêt au même instant, l'annulation l'emporte (02 §7.1).
            biased;
            () = cancel => Outcome::Cancelled,
            done = tokio::time::timeout(deadline, work) => {
                Outcome::Done(done.unwrap_or_else(|_| Err(messages::deadline(deadline))))
            }
        };
        // Durée réelle, en erreur comme en succès (D-03), tronquée à la milliseconde et saturée.
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let record = self.record(meta, &outcome, duration_ms);
        trace_outcome(&record);
        let Err(error) = self.log.record(&record).await else {
            return outcome.into_result();
        };
        // Seul le genre de l'échec est journalisé : son `detail` peut citer un chemin local.
        match (outcome, self.policy.mode) {
            (Outcome::Done(result), AuditMode::Strict) => {
                tracing::error!(kind = ?error.kind, "audit write failed; result withheld");
                let original = result.as_ref().err().and_then(AppError::code);
                Err(messages::audit(&error, original))
            }
            // Mode `BestEffort`, ou annulation : personne ne lira la réponse d'un appel annulé, il
            // n'y a rien à retenir ; une seule tentative d'écriture, dont l'échec est signalé.
            (outcome, mode) => {
                tracing::warn!(kind = ?error.kind, "audit write failed");
                if mode == AuditMode::BestEffort {
                    self.write_failures.fetch_add(1, Ordering::Relaxed);
                }
                outcome.into_result()
            }
        }
    }

    /// Ligne d'audit de l'appel : en succès, la métrique de la sortie ; en erreur, 0, le code
    /// (absent pour une annulation) et le message final coupé à 500 caractères.
    fn record<O: AuditMetric>(
        &self,
        meta: CallMeta,
        outcome: &Outcome<O>,
        duration_ms: u64,
    ) -> AuditRecord {
        let failed = |error: &AppError| {
            let message = messages::clip(error.message(), AUDIT_ERROR_MAX_CHARS);
            (AuditStatus::Error, 0, error.code(), Some(message))
        };
        let (status, output_count_or_size, error_code, error) = match outcome {
            Outcome::Done(Ok(output)) => {
                (AuditStatus::Ok, output.output_count_or_size(), None, None)
            }
            Outcome::Done(Err(error)) => failed(error),
            Outcome::Cancelled => failed(&messages::cancelled()),
        };
        AuditRecord {
            ts: meta.ts,
            tool: meta.tool,
            input: meta.canonical,
            output_count_or_size,
            duration_ms,
            status,
            error_code,
            error,
            env: self.policy.env,
            server_version: self.policy.server_version,
        }
    }
}

/// Issue de l'exécution, avant l'écriture de la ligne.
enum Outcome<O> {
    /// `work` a rendu son résultat, ou le délai global l'a remplacé par `upstream_error`.
    Done(Result<O, AppError>),
    /// Le client a annulé : `work` a été abandonné.
    Cancelled,
}

impl<O> Outcome<O> {
    /// Ce que rend `run` quand l'écriture ne retient pas le résultat.
    fn into_result(self) -> Result<O, AppError> {
        match self {
            Self::Done(result) => result,
            Self::Cancelled => Err(messages::cancelled()),
        }
    }
}

/// Renseigne le span `tool_call` d'après la ligne, puis émet l'événement de fin : `info` pour un
/// succès, une annulation (choix du client) et les erreurs attendues, `warn` pour les autres
/// (D-08). Le `match` est exhaustif : un code ajouté devra choisir son niveau.
fn trace_outcome(record: &AuditRecord) {
    let span = Span::current();
    span.record("status", status_name(record.status));
    if let Some(code) = record.error_code {
        span.record("error_code", code.as_str());
    }
    span.record("duration_ms", record.duration_ms);
    span.record("output", record.output_count_or_size);
    match record.error_code {
        None | Some(ErrorCode::InvalidArg | ErrorCode::NotFound) => {
            tracing::info!("tool call finished");
        }
        Some(
            ErrorCode::UpstreamError
            | ErrorCode::RateLimited
            | ErrorCode::ConfigError
            | ErrorCode::AuditError,
        ) => tracing::warn!("tool call finished"),
    }
}

/// Valeur du champ `status`, celle de la ligne d'audit.
const fn status_name(status: AuditStatus) -> &'static str {
    match status {
        AuditStatus::Ok => "ok",
        AuditStatus::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::UpstreamCause,
        input::{InputError, ToolCall},
        messages::clip,
        ports::{AuditError, AuditErrorKind, AuthOrigin, ConfigKind, CorpusError, LegalCorpus},
        testing::{FailingAuditLog, FakeCorpus, MemoryAuditLog, NO_DEADLINE},
    };
    use lf_domain::{
        entity::{Decision, OfficialSummary, SearchPage},
        fond::{Fond, PaginationMode},
        id::DecisionId,
        search::{MatchMode, PageSize, QueryText, SearchQuery},
        text::LongText,
    };
    use pretty_assertions::assert_eq;
    use std::{
        collections::BTreeMap,
        fmt,
        future::{pending, ready},
        hint::black_box,
        num::NonZeroU32,
        pin::pin,
        sync::{Arc, Mutex},
    };
    use tokio::time::{sleep, timeout};
    use tracing::{
        Level, Subscriber,
        field::{Field, Visit},
        span::{Attributes, Id, Record},
        subscriber::DefaultGuard,
    };
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

    /// `LF_TOOL_DEADLINE_S` par défaut (00 §1).
    const DEADLINE: Duration = Duration::from_secs(60);
    const DECISION_ID: &str = "JURITEXT000037999394";
    const CANONICAL: &str = r#"{"decision_id":"JURITEXT000037999394"}"#;
    const DECISION_TEXT: &str = "La Cour rejette le pourvoi.";
    /// Délai d'un port qui ne répond jamais dans le temps d'un test (un jour virtuel).
    const NEVER: Duration = Duration::from_secs(86_400);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// Sortie de test : sa métrique est la valeur qu'elle porte.
    #[derive(Debug, PartialEq, Eq)]
    struct Out(u64);

    impl AuditMetric for Out {
        fn output_count_or_size(&self) -> u64 {
            self.0
        }
    }

    fn ts() -> DateTime<Utc> {
        DateTime::from_timestamp(1_782_000_000, 0).unwrap()
    }

    fn policy(mode: AuditMode, deadline: Duration) -> AuditPolicy {
        AuditPolicy { mode, deadline, env: PisteEnvironment::Sandbox, server_version: "0.0.0-test" }
    }

    fn meta() -> CallMeta {
        CallMeta::new(ToolName::FR_GET_DECISION, ts(), CanonicalInput::new(CANONICAL))
    }

    fn memory(mode: AuditMode, deadline: Duration) -> Audited<MemoryAuditLog> {
        Audited::new(MemoryAuditLog::new(), policy(mode, deadline))
    }

    fn failing(kind: AuditErrorKind, mode: AuditMode) -> Audited<FailingAuditLog> {
        Audited::new(FailingAuditLog::new(kind), policy(mode, DEADLINE))
    }

    /// Un échec d'écriture quelconque : `messages::audit` n'en lit jamais le contenu.
    fn write_error() -> AuditError {
        AuditError { kind: AuditErrorKind::DiskFull, detail: String::new() }
    }

    fn decision() -> Decision {
        Decision {
            decision_id: DECISION_ID.parse().unwrap(),
            ecli: None,
            num: None,
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: None,
            nature: None,
            title: None,
            text: LongText::from_plain(DECISION_TEXT).unwrap(),
            official_summary: OfficialSummary::default(),
        }
    }

    /// Caractères renvoyés par `fr_get_decision` pour [`decision`] (métrique de 00 §7).
    fn decision_chars() -> u64 {
        u64::from(decision().text.char_len())
    }

    fn query() -> SearchQuery {
        SearchQuery::new(
            Fond::Juri,
            QueryText::parse("bail").unwrap(),
            NonZeroU32::MIN,
            PageSize::default(),
            MatchMode::default(),
            None,
            None,
        )
        .unwrap()
    }

    fn empty_page() -> SearchPage {
        SearchPage::new(Fond::Juri, "bail", Some(0), None, vec![], 0, PaginationMode::Defaut)
    }

    /// Appel de `fr_get_decision` : `Ok` porte l'identifiant brut, `Err` le message de décodage.
    fn call(raw: Result<&str, &str>) -> ToolCall<String> {
        ToolCall {
            input: raw
                .map(str::to_owned)
                .map_err(|message| InputError { message: message.to_owned() }),
            canonical: CanonicalInput::new(CANONICAL),
        }
    }

    /// Cas d'usage de test façon `fr_get_decision` (le vrai arrive au plan 3b) : décodage,
    /// validation par le domaine, appel au port, message de l'outil ; la métrique est le nombre de
    /// caractères du texte.
    async fn get_decision(corpus: &FakeCorpus, call: ToolCall<String>) -> Result<Out, AppError> {
        let raw = call.input?;
        let id: DecisionId =
            raw.parse().map_err(|error| messages::invalid_argument(format!("{error}")))?;
        let decision = corpus
            .get_decision(&id)
            .await
            .map_err(|error| messages::corpus_error(ToolName::FR_GET_DECISION, &raw, &error))?;
        Ok(Out(u64::from(decision.text.char_len())))
    }

    /// Cas d'usage de test façon `fr_search` : la métrique est le nombre de résultats rendus. La
    /// table ne lui programme qu'un succès (ligne 12) ; les erreurs du port passent par
    /// [`get_decision`].
    async fn search(corpus: &FakeCorpus) -> Result<Out, AppError> {
        let page = corpus.search(&query()).await.expect("the table programs a page");
        Ok(Out(u64::try_from(page.hits.len()).unwrap()))
    }

    /// Futur `cancel` : résolu après `after`, jamais sans valeur.
    async fn cancel_after(after: Option<Duration>) {
        match after {
            Some(delay) => sleep(delay).await,
            None => pending().await,
        }
    }

    // ----- Table de 06 §2.2 -----

    /// Journal de la table : en mémoire, ou toujours en échec (`DiskFull`), sous un seul type.
    enum TableLog {
        Memory(MemoryAuditLog),
        Failing(FailingAuditLog),
    }

    impl AuditLog for TableLog {
        async fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
            match self {
                Self::Memory(log) => log.record(record).await,
                Self::Failing(log) => log.record(record).await,
            }
        }
    }

    impl TableLog {
        fn records(&self) -> Vec<AuditRecord> {
            match self {
                Self::Memory(log) => log.records(),
                Self::Failing(_) => Vec::new(),
            }
        }
    }

    /// Outil appelé par une ligne de la table.
    enum Input {
        /// `fr_get_decision` : identifiant brut, ou message de décodage.
        Decision(Result<&'static str, &'static str>),
        /// `fr_search` sur une requête valide.
        Search,
    }

    /// Réponse programmée dans le faux corpus (une `Decision` est grosse : en boîte).
    enum Answer {
        Decision(Box<Result<Decision, CorpusError>>),
        Search(Result<SearchPage, CorpusError>),
        Nothing,
    }

    /// Une issue de 06 §2.2 : mise en place, puis ce qu'on attend de la ligne et du retour.
    struct Case {
        name: &'static str,
        mode: AuditMode,
        failing_log: bool,
        deadline: Duration,
        input: Input,
        answer: Answer,
        /// Délai du faux corpus.
        delay: Option<Duration>,
        /// Instant d'annulation par le client.
        cancel: Option<Duration>,
        /// `(status, error_code, output_count_or_size)` de la ligne ; `None` : aucune ligne.
        line: Option<(AuditStatus, Option<ErrorCode>, u64)>,
        /// Bornes de `duration_ms`.
        duration_ms: (u64, u64),
        /// Métrique de la sortie, ou code de l'erreur rendue.
        returned: Result<u64, Option<ErrorCode>>,
        cause: Option<UpstreamCause>,
        /// Message de l'erreur rendue.
        message: Option<String>,
        port_calls: usize,
        write_failures: u64,
    }

    /// Ligne ordinaire : journal en mémoire, mode `Strict`, délai par défaut, pas d'annulation.
    fn case(name: &'static str, input: Input, answer: Answer, delay_ms: Option<u64>) -> Case {
        Case {
            name,
            mode: AuditMode::Strict,
            failing_log: false,
            deadline: DEADLINE,
            input,
            answer,
            delay: delay_ms.map(ms),
            cancel: None,
            line: None,
            duration_ms: (0, 0),
            returned: Ok(0),
            cause: None,
            message: None,
            port_calls: 1,
            write_failures: 0,
        }
    }

    /// Ligne ordinaire de `fr_get_decision` sur [`DECISION_ID`], avec la réponse du port.
    fn get_decision_case(
        name: &'static str,
        answer: Result<Decision, CorpusError>,
        delay_ms: Option<u64>,
    ) -> Case {
        let answer = Answer::Decision(Box::new(answer));
        case(name, Input::Decision(Ok(DECISION_ID)), answer, delay_ms)
    }

    /// Les 15 issues de 06 §2.2, dans l'ordre (la ligne 4 a deux variantes de `what`), puis les
    /// deux croisements de l'annulation (ligne 11) avec un journal en échec (lignes 13 et 15).
    fn outcomes() -> Vec<Case> {
        let mut cases = entry_outcomes();
        cases.extend(port_outcomes());
        cases.extend(time_outcomes());
        cases.extend(audit_outcomes());
        cases.extend(cancelled_with_failed_write());
        cases
    }

    /// Lignes 1 à 3 : succès, décodage impossible, validation du domaine (0 appel au port).
    fn entry_outcomes() -> Vec<Case> {
        let undecodable = "decision_id: invalid type: integer `42`, expected a string";
        let wrong_prefix = "CNILTEXT000053352643";
        let invalid_id = wrong_prefix.parse::<DecisionId>().unwrap_err().to_string();
        let invalid = |name, input, message| Case {
            line: Some((AuditStatus::Error, Some(ErrorCode::InvalidArg), 0)),
            returned: Err(Some(ErrorCode::InvalidArg)),
            message: Some(message),
            port_calls: 0,
            ..case(name, input, Answer::Nothing, None)
        };
        vec![
            Case {
                line: Some((AuditStatus::Ok, None, decision_chars())),
                duration_ms: (150, 160),
                returned: Ok(decision_chars()),
                ..get_decision_case("1 success", Ok(decision()), Some(150))
            },
            invalid("2 undecodable", Input::Decision(Err(undecodable)), undecodable.to_owned()),
            invalid("3 domain validation", Input::Decision(Ok(wrong_prefix)), invalid_id),
        ]
    }

    /// Lignes 4 à 9 : erreurs du port, traduites par le message de l'outil.
    fn port_outcomes() -> Vec<Case> {
        let failed = |name, error: CorpusError, delay_ms: u64, code, cause| {
            let expected = messages::corpus_error(ToolName::FR_GET_DECISION, DECISION_ID, &error);
            Case {
                line: Some((AuditStatus::Error, Some(code), 0)),
                duration_ms: (delay_ms, delay_ms + 10),
                returned: Err(Some(code)),
                cause,
                message: Some(expected.message().to_owned()),
                ..get_decision_case(name, Err(error), Some(delay_ms))
            }
        };
        let not_found = |what: &str| CorpusError::NotFound { what: what.to_owned() };
        vec![
            failed("4 not found (404)", not_found("HTTP 404"), 40, ErrorCode::NotFound, None),
            failed(
                "4 not found (empty 200)",
                not_found("empty body"),
                40,
                ErrorCode::NotFound,
                None,
            ),
            failed(
                "5 upstream 503",
                CorpusError::Upstream {
                    status: Some(503),
                    retryable: true,
                    detail: "HTTP 503".to_owned(),
                },
                200,
                ErrorCode::UpstreamError,
                Some(UpstreamCause::Status(503)),
            ),
            failed(
                "6 rate limited",
                CorpusError::RateLimited { retry_after: Some(secs(12)) },
                50,
                ErrorCode::RateLimited,
                None,
            ),
            failed(
                "7 credentials missing",
                CorpusError::Config {
                    kind: ConfigKind::CredentialsMissing,
                    detail: "LF_PISTE_CLIENT_ID is not set".to_owned(),
                },
                0,
                ErrorCode::ConfigError,
                None,
            ),
            failed(
                "8 credentials rejected",
                CorpusError::Auth {
                    origin: AuthOrigin::Token,
                    status: Some(401),
                    detail: "invalid_client".to_owned(),
                },
                60,
                ErrorCode::ConfigError,
                None,
            ),
            failed(
                "9 undecodable response",
                CorpusError::Decode { detail: "expected value at line 1 column 1".to_owned() },
                70,
                ErrorCode::UpstreamError,
                Some(UpstreamCause::Decode),
            ),
        ]
    }

    /// Lignes 10 à 12 : délai global, annulation, recherche vide.
    fn time_outcomes() -> Vec<Case> {
        vec![
            Case {
                deadline: secs(1),
                delay: Some(NEVER),
                line: Some((AuditStatus::Error, Some(ErrorCode::UpstreamError), 0)),
                duration_ms: (1_000, 1_010),
                returned: Err(Some(ErrorCode::UpstreamError)),
                cause: Some(UpstreamCause::Deadline),
                message: Some(messages::deadline(secs(1)).message().to_owned()),
                ..get_decision_case("10 deadline", Ok(decision()), None)
            },
            Case {
                delay: Some(NEVER),
                cancel: Some(ms(100)),
                line: Some((AuditStatus::Error, None, 0)),
                duration_ms: (100, 110),
                returned: Err(None),
                cause: Some(UpstreamCause::Cancelled),
                message: Some("cancelled by client".to_owned()),
                ..get_decision_case("11 cancelled", Ok(decision()), None)
            },
            Case {
                line: Some((AuditStatus::Ok, None, 0)),
                duration_ms: (30, 40),
                returned: Ok(0),
                ..case("12 empty search", Input::Search, Answer::Search(Ok(empty_page())), Some(30))
            },
        ]
    }

    /// Lignes 13 à 15 : journal en échec (`DiskFull`), aucune ligne possible.
    fn audit_outcomes() -> Vec<Case> {
        let withheld = |original| messages::audit(&write_error(), original).message().to_owned();
        let not_found = CorpusError::NotFound { what: "HTTP 404".to_owned() };
        vec![
            Case {
                failing_log: true,
                returned: Err(Some(ErrorCode::AuditError)),
                message: Some(withheld(None)),
                ..get_decision_case("13 strict, success", Ok(decision()), None)
            },
            Case {
                failing_log: true,
                returned: Err(Some(ErrorCode::AuditError)),
                message: Some(withheld(Some(ErrorCode::NotFound))),
                ..get_decision_case("14 strict, original error", Err(not_found), None)
            },
            Case {
                mode: AuditMode::BestEffort,
                failing_log: true,
                returned: Ok(decision_chars()),
                write_failures: 1,
                ..get_decision_case("15 best effort", Ok(decision()), None)
            },
        ]
    }

    /// Annulation et journal en échec : personne ne lit la réponse, rien n'est donc retenu, quel
    /// que soit le mode ; le mode ne décide que du compteur. Ces deux lignes complètent la matrice
    /// de décision d'`Audited` dans la même instanciation que la table.
    fn cancelled_with_failed_write() -> Vec<Case> {
        let crossed = |name, mode, write_failures| Case {
            mode,
            failing_log: true,
            delay: Some(NEVER),
            cancel: Some(ms(100)),
            returned: Err(None),
            cause: Some(UpstreamCause::Cancelled),
            message: Some("cancelled by client".to_owned()),
            write_failures,
            ..get_decision_case(name, Ok(decision()), None)
        };
        vec![
            crossed("11 x 13 cancelled, strict, write failure", AuditMode::Strict, 0),
            crossed("11 x 15 cancelled, best effort, write failure", AuditMode::BestEffort, 1),
        ]
    }

    #[tokio::test(start_paused = true)]
    async fn audit_written_for_every_outcome() {
        let cases = outcomes();
        assert_eq!(cases.len(), 18, "15 issues (la 4 en deux variantes) et 2 croisements");
        for case in cases {
            let name = case.name;
            let corpus =
                case.delay.map_or_else(FakeCorpus::new, |d| FakeCorpus::new().with_delay(d));
            match case.answer {
                Answer::Decision(answer) => corpus.push_get_decision(*answer),
                Answer::Search(answer) => corpus.push_search(answer),
                Answer::Nothing => {}
            }
            let log = if case.failing_log {
                TableLog::Failing(FailingAuditLog::new(AuditErrorKind::DiskFull))
            } else {
                TableLog::Memory(MemoryAuditLog::new())
            };
            let audited = Audited::new(log, policy(case.mode, case.deadline));
            let cancel = cancel_after(case.cancel);
            let got = match case.input {
                Input::Decision(raw) => {
                    audited.run(meta(), cancel, get_decision(&corpus, call(raw))).await
                }
                Input::Search => audited.run(meta(), cancel, search(&corpus)).await,
            };

            let lines = audited.log.records();
            assert_eq!(lines.len(), usize::from(case.line.is_some()), "{name}");
            if let (Some(line), Some(expected)) = (lines.first(), case.line) {
                assert_eq!(
                    (line.status, line.error_code, line.output_count_or_size),
                    expected,
                    "{name}"
                );
                let (min, max) = case.duration_ms;
                assert!((min..=max).contains(&line.duration_ms), "{name}: {} ms", line.duration_ms);
                // Le champ `error` est le message rendu, sans préfixe ni ajout.
                assert_eq!(
                    line.error,
                    got.as_ref().err().map(|e| clip(e.message(), 500)),
                    "{name}"
                );
            }
            assert_eq!(
                got.as_ref().map(AuditMetric::output_count_or_size).map_err(AppError::code),
                case.returned,
                "{name}"
            );
            assert_eq!(got.as_ref().err().and_then(AppError::cause), case.cause, "{name}");
            assert_eq!(
                got.as_ref().err().map(AppError::message),
                case.message.as_deref(),
                "{name}"
            );
            assert_eq!(corpus.calls(), case.port_calls, "{name}");
            assert_eq!(audited.write_failures(), case.write_failures, "{name}");
        }
    }

    // ----- Durée et délai global -----

    #[tokio::test(start_paused = true)]
    async fn error_duration_is_nonzero() {
        let audited = memory(AuditMode::Strict, DEADLINE);
        let work = async {
            sleep(ms(200)).await;
            Err::<Out, _>(AppError::not_found("No JURI decision found."))
        };
        let got = audited.run(meta(), pending(), work).await;
        assert_eq!(got.map_err(|e| e.code()), Err(Some(ErrorCode::NotFound)));
        let lines = audited.log.records();
        assert_eq!((lines[0].status, lines[0].duration_ms), (AuditStatus::Error, 200));
    }

    #[tokio::test(start_paused = true)]
    async fn duration_of_instant_failure_is_zero() {
        let audited = memory(AuditMode::Strict, DEADLINE);
        let got =
            audited.run(meta(), pending(), ready(Err::<Out, _>(AppError::invalid_argument("bad"))));
        assert_eq!(got.await, Err(AppError::invalid_argument("bad")));
        assert_eq!(audited.log.records()[0].duration_ms, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_maps_to_upstream_error_before_deadline_plus_epsilon() {
        let audited = memory(AuditMode::Strict, secs(1));
        let start = Instant::now();
        // Un cas d'usage qui ne rend jamais la main.
        let got = audited.run(meta(), pending(), pending::<Result<Out, AppError>>()).await;
        let elapsed = start.elapsed();
        assert!((ms(1_000)..=ms(1_010)).contains(&elapsed), "{elapsed:?}");
        let error = got.unwrap_err();
        assert_eq!(error, messages::deadline(secs(1)));
        assert_eq!(
            (error.code(), error.cause()),
            (Some(ErrorCode::UpstreamError), Some(UpstreamCause::Deadline))
        );
        let line = &audited.log.records()[0];
        assert_eq!(
            (line.status, line.error_code, line.error.as_deref()),
            (AuditStatus::Error, Some(ErrorCode::UpstreamError), Some(error.message()))
        );
        assert!((1_000..=1_010).contains(&line.duration_ms), "{} ms", line.duration_ms);
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_is_published_to_the_port() {
        let corpus = FakeCorpus::new().with_delay(secs(2));
        corpus.push_get_decision(Ok(decision()));
        corpus.push_get_decision(Ok(decision()));
        corpus.push_get_decision(Ok(decision()));
        let id: DecisionId = DECISION_ID.parse().unwrap();
        let audited = memory(AuditMode::Strict, DEADLINE);
        let work = async {
            sleep(secs(3)).await;
            let first = corpus.get_decision(&id).await;
            let second = corpus.get_decision(&id).await;
            assert!(first.is_ok() && second.is_ok());
            Ok(Out(2))
        };
        assert_eq!(audited.run(meta(), pending(), work).await, Ok(Out(2)));
        // 60 s moins 3 s, puis moins 3 + 2 s : la durée restante, y compris au second appel.
        assert_eq!(corpus.seen_deadlines(), vec![secs(57), secs(55)]);
        // Hors de `run`, aucune échéance n'est publiée : l'adapter retombe sur son défaut.
        assert!(corpus.get_decision(&id).await.is_ok());
        assert_eq!(corpus.seen_deadlines()[2], NO_DEADLINE);
        assert_eq!(Deadline::current(), None);
    }

    // ----- Annulation -----

    #[tokio::test(start_paused = true)]
    async fn cancelled_call_is_audited() {
        let corpus = FakeCorpus::new().with_delay(NEVER);
        corpus.push_get_decision(Ok(decision()));
        let audited = memory(AuditMode::Strict, DEADLINE);
        let work = get_decision(&corpus, call(Ok(DECISION_ID)));
        let got = audited.run(meta(), sleep(ms(100)), work).await;
        let error = got.unwrap_err();
        assert_eq!(error, messages::cancelled());
        assert_eq!((error.code(), error.cause()), (None, Some(UpstreamCause::Cancelled)));
        let lines = audited.log.records();
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(
            (line.status, line.error_code, line.error.as_deref()),
            (AuditStatus::Error, None, Some("cancelled by client"))
        );
        assert_eq!(line.output_count_or_size, 0);
        assert!((100..=110).contains(&line.duration_ms), "{} ms", line.duration_ms);
        // L'appel au port, abandonné, a été fait une fois.
        assert_eq!((corpus.calls(), corpus.max_concurrency()), (1, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_wins_over_deadline_at_same_instant() {
        let audited = memory(AuditMode::Strict, secs(1));
        let got = audited.run(meta(), sleep(secs(1)), pending::<Result<Out, AppError>>()).await;
        assert_eq!(got, Err(messages::cancelled()));
        // Même à l'instant zéro, contre un résultat déjà prêt.
        let got = audited.run(meta(), ready(()), ready(Ok(Out(7)))).await;
        assert_eq!(got, Err(messages::cancelled()));
        let lines = audited.log.records();
        let seen: Vec<_> =
            lines.iter().map(|l| (l.error_code, l.error.as_deref(), l.duration_ms)).collect();
        assert_eq!(
            seen,
            vec![
                (None, Some("cancelled by client"), 1_000),
                (None, Some("cancelled by client"), 0)
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_call_with_failed_write_stays_cancelled() {
        for (mode, failures) in [(AuditMode::Strict, 0), (AuditMode::BestEffort, 1)] {
            let (capture, _guard) = Capture::install();
            let audited = failing(AuditErrorKind::WriterGone, mode);
            let got = audited.run(meta(), ready(()), pending::<Result<Out, AppError>>()).await;
            assert_eq!(got, Err(messages::cancelled()), "{mode:?}");
            assert_eq!(audited.write_failures(), failures, "{mode:?}");
            assert_eq!(
                capture.events(),
                vec![
                    (Level::INFO, "tool call finished".to_owned(), None),
                    (Level::WARN, "audit write failed".to_owned(), Some("WriterGone".to_owned())),
                ],
                "{mode:?}"
            );
        }
    }

    // ----- Échec d'écriture -----

    #[tokio::test(start_paused = true)]
    async fn audit_failure_returns_audit_error() {
        let audited = failing(AuditErrorKind::DiskFull, AuditMode::Strict);
        let got = audited.run(meta(), pending(), ready(Ok(Out(3)))).await;
        let error = got.unwrap_err();
        assert_eq!(error, messages::audit(&write_error(), None));
        assert_eq!(error.code(), Some(ErrorCode::AuditError));
        // Le `detail` de l'échec (chemin, texte du journal) n'atteint jamais le modèle.
        assert!(!error.message().contains("FailingAuditLog"), "{error:?}");
        assert_eq!(audited.write_failures(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn audit_failure_does_not_mask_original_cause() {
        let audited = failing(AuditErrorKind::DiskFull, AuditMode::Strict);
        let work = ready(Err::<Out, _>(AppError::not_found("No JURI decision found.")));
        let error = audited.run(meta(), pending(), work).await.unwrap_err();
        assert_eq!(error, messages::audit(&write_error(), Some(ErrorCode::NotFound)));
        assert!(error.message().ends_with(" Original error: [not_found]."), "{error:?}");
        assert_eq!(audited.write_failures(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn best_effort_keeps_result() {
        let (capture, _guard) = Capture::install();
        let audited = failing(AuditErrorKind::Io, AuditMode::BestEffort);
        assert_eq!(audited.run(meta(), pending(), ready(Ok(Out(5)))).await, Ok(Out(5)));
        assert_eq!(audited.write_failures(), 1);
        // Une erreur d'origine est rendue telle quelle, et comptée de même.
        let work = ready(Err::<Out, _>(AppError::not_found("gone")));
        assert_eq!(audited.run(meta(), pending(), work).await, Err(AppError::not_found("gone")));
        assert_eq!(audited.write_failures(), 2);
        let warned = (Level::WARN, "audit write failed".to_owned(), Some("Io".to_owned()));
        let events = capture.events();
        assert_eq!(events.iter().filter(|event| **event == warned).count(), 2);
        assert!(events.iter().all(|(level, ..)| *level != Level::ERROR), "{events:?}");

        // Écriture réussie : le compteur ne bouge pas.
        let written = memory(AuditMode::BestEffort, DEADLINE);
        assert_eq!(written.run(meta(), pending(), ready(Ok(Out(5)))).await, Ok(Out(5)));
        assert_eq!((written.write_failures(), written.log.records().len()), (0, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn strict_write_failure_logs_an_error_event() {
        let (capture, _guard) = Capture::install();
        let audited = failing(AuditErrorKind::PermissionDenied, AuditMode::Strict);
        let got = audited.run(meta(), pending(), ready(Ok(Out(1)))).await;
        assert_eq!(got.map_err(|e| e.code()), Err(Some(ErrorCode::AuditError)));
        assert_eq!(
            capture.events(),
            vec![
                (Level::INFO, "tool call finished".to_owned(), None),
                (
                    Level::ERROR,
                    "audit write failed; result withheld".to_owned(),
                    Some("PermissionDenied".to_owned())
                ),
            ]
        );
        // Ni le `detail` de l'échec ni les arguments ne sont journalisés.
        let all = capture.all_values();
        assert!(!all.iter().any(|v| v.contains("FailingAuditLog") || v.contains(DECISION_ID)));
    }

    #[tokio::test(start_paused = true)]
    async fn audit_write_is_acknowledged_before_tool_returns() {
        let audited =
            Audited::new(MemoryAuditLog::with_gate(), policy(AuditMode::Strict, DEADLINE));
        let mut call = pin!(audited.run(meta(), pending(), ready(Ok(Out(1)))));
        // Une heure virtuelle : l'appel attend toujours l'accusé d'écriture.
        assert!(timeout(secs(3_600), call.as_mut()).await.is_err());
        assert!(audited.log.records().is_empty());
        audited.log.open_gate();
        assert_eq!(call.await, Ok(Out(1)));
        assert_eq!(audited.log.records().len(), 1);
    }

    // ----- Contenu de la ligne -----

    #[tokio::test(start_paused = true)]
    async fn audit_metric_is_taken_from_the_output() {
        let audited = memory(AuditMode::Strict, DEADLINE);
        assert_eq!(audited.run(meta(), pending(), ready(Ok(Out(4_242)))).await, Ok(Out(4_242)));
        let line = &audited.log.records()[0];
        assert_eq!((line.status, line.output_count_or_size), (AuditStatus::Ok, 4_242));
        assert_eq!((line.error_code, line.error.as_deref()), (None, None));
    }

    #[tokio::test(start_paused = true)]
    async fn audit_error_field_is_capped_at_500_chars() {
        let audited = memory(AuditMode::Strict, DEADLINE);
        let long = "é".repeat(600);
        let work = ready(Err::<Out, _>(AppError::invalid_argument(long.clone())));
        // Le modèle lit le message entier ; seule la ligne d'audit est coupée.
        assert_eq!(
            audited.run(meta(), pending(), work).await,
            Err(AppError::invalid_argument(long))
        );
        let line = &audited.log.records()[0];
        assert_eq!(line.error, Some("é".repeat(500)));
    }

    #[tokio::test(start_paused = true)]
    async fn record_carries_env_version_and_timestamp() {
        let policy = AuditPolicy {
            mode: AuditMode::Strict,
            deadline: DEADLINE,
            env: PisteEnvironment::Production,
            server_version: "1.2.3-test",
        };
        let audited = Audited::new(MemoryAuditLog::new(), policy);
        let meta = CallMeta::new(ToolName::FR_SEARCH, ts(), CanonicalInput::new(r#"{"q":"bail"}"#));
        assert_eq!(audited.run(meta, pending(), ready(Ok(Out(2)))).await, Ok(Out(2)));
        let line = &audited.log.records()[0];
        assert_eq!(
            (line.ts, line.tool, line.input.as_str(), line.env, line.server_version),
            (
                ts(),
                ToolName::FR_SEARCH,
                r#"{"q":"bail"}"#,
                PisteEnvironment::Production,
                "1.2.3-test"
            )
        );
    }

    // ----- Journaux (`tracing`) -----

    #[tokio::test(start_paused = true)]
    async fn invalid_arg_is_not_logged_as_error() {
        let (capture, _guard) = Capture::install();
        let audited = memory(AuditMode::Strict, DEADLINE);
        let work = ready(Err::<Out, _>(AppError::invalid_argument("page_size: invalid type")));
        assert!(audited.run(meta(), pending(), work).await.is_err());
        assert_eq!(capture.levels(), vec![Level::INFO]);
    }

    #[tokio::test(start_paused = true)]
    async fn tool_call_event_level_follows_the_outcome() {
        let rate_limited = AppError::RateLimited { message: "m".to_owned(), retry_after: None };
        let cases = [
            ("success", Ok(Out(1)), Level::INFO),
            ("not_found", Err(AppError::not_found("m")), Level::INFO),
            ("upstream_error", Err(messages::deadline(DEADLINE)), Level::WARN),
            ("rate_limited", Err(rate_limited), Level::WARN),
            ("config_error", Err(AppError::config("m")), Level::WARN),
        ];
        for (name, result, level) in cases {
            let (capture, _guard) = Capture::install();
            let audited = memory(AuditMode::Strict, DEADLINE);
            let _ = audited.run(meta(), pending(), ready(result)).await;
            assert_eq!(capture.levels(), vec![level], "{name}");
        }
        // Une annulation est un choix du client, pas une panne : `info`.
        let (capture, _guard) = Capture::install();
        let audited = memory(AuditMode::Strict, DEADLINE);
        let _ = audited.run(meta(), ready(()), pending::<Result<Out, AppError>>()).await;
        assert_eq!(capture.levels(), vec![Level::INFO]);
    }

    #[tokio::test(start_paused = true)]
    async fn tool_call_span_carries_outcome_fields() {
        let (capture, _guard) = Capture::install();
        let audited = memory(AuditMode::Strict, DEADLINE);
        let work = async {
            sleep(ms(30)).await;
            Err::<Out, _>(AppError::not_found("No JURI decision found."))
        };
        let _ = audited.run(meta(), pending(), work).await;
        let input_bytes = CANONICAL.len().to_string();
        assert_eq!(
            capture.span("tool_call"),
            BTreeMap::from([
                ("tool", "fr_get_decision".to_owned()),
                ("input_bytes", input_bytes.clone()),
                ("status", "error".to_owned()),
                ("error_code", "not_found".to_owned()),
                ("duration_ms", "30".to_owned()),
                ("output", "0".to_owned()),
            ])
        );
        // Succès : pas de code, la métrique de la sortie.
        let _ = audited.run(meta(), pending(), ready(Ok(Out(9)))).await;
        assert_eq!(
            capture.span("tool_call"),
            BTreeMap::from([
                ("tool", "fr_get_decision".to_owned()),
                ("input_bytes", input_bytes),
                ("status", "ok".to_owned()),
                ("duration_ms", "0".to_owned()),
                ("output", "9".to_owned()),
            ])
        );
        // Les arguments eux-mêmes ne sont jamais journalisés (02 §11.3).
        assert!(!capture.all_values().iter().any(|v| v.contains(DECISION_ID)));
    }

    // ----- Propriétés de `run` -----

    fn assert_send<F: Future + Send>(_: &F) {}

    #[test]
    fn run_future_is_send() {
        // rmcp boxe les futurs d'outils en `dyn Future + Send` (02 §3.2).
        let audited = memory(AuditMode::Strict, DEADLINE);
        let future = audited.run(meta(), pending(), ready(Ok(Out(1))));
        assert_send(&future);
    }

    #[tokio::test(start_paused = true)]
    async fn work_panic_propagates_without_audit_line() {
        // Une panique n'est pas une issue de 06 §2.2 : aucun code ne la décrit. Elle traverse `run`
        // jusqu'à la frontière de tâche (rmcp, une tâche par requête) ; aucune ligne n'est écrite.
        let audited = Arc::new(memory(AuditMode::Strict, DEADLINE));
        let task = {
            let audited = Arc::clone(&audited);
            tokio::spawn(async move {
                let work =
                    async { Ok(black_box(None::<Out>).expect("simulated bug in a use case")) };
                audited.run(meta(), pending(), work).await
            })
        };
        assert!(task.await.is_err_and(|error| error.is_panic()));
        assert!(audited.log.records().is_empty());
    }

    // ----- Capture des journaux -----

    /// Événement capturé : niveau, message et champ `kind` s'il existe.
    type Event = (Level, String, Option<String>);

    #[derive(Default)]
    struct Captured {
        events: Vec<(Level, BTreeMap<&'static str, String>)>,
        spans: Vec<(Id, &'static str, BTreeMap<&'static str, String>)>,
    }

    /// Couche `tracing` qui garde les événements et les champs des spans.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Captured>>);

    impl Capture {
        /// Installe la capture comme abonné du fil courant (runtime `current_thread` des tests).
        fn install() -> (Self, DefaultGuard) {
            let capture = Self::default();
            let subscriber = tracing_subscriber::registry().with(capture.clone());
            (capture, tracing::subscriber::set_default(subscriber))
        }

        fn events(&self) -> Vec<Event> {
            let captured = self.0.lock().unwrap();
            captured
                .events
                .iter()
                .map(|(level, fields)| {
                    (*level, fields["message"].clone(), fields.get("kind").cloned())
                })
                .collect()
        }

        fn levels(&self) -> Vec<Level> {
            self.0.lock().unwrap().events.iter().map(|(level, _)| *level).collect()
        }

        /// Champs du dernier span nommé `name`.
        fn span(&self, name: &str) -> BTreeMap<&'static str, String> {
            let captured = self.0.lock().unwrap();
            captured.spans.iter().rev().find(|(_, n, _)| *n == name).unwrap().2.clone()
        }

        /// Toutes les valeurs journalisées, événements et spans confondus.
        fn all_values(&self) -> Vec<String> {
            let captured = self.0.lock().unwrap();
            let events = captured.events.iter().flat_map(|(_, fields)| fields.values());
            let spans = captured.spans.iter().flat_map(|(_, _, fields)| fields.values());
            events.chain(spans).cloned().collect()
        }
    }

    struct Fields<'a>(&'a mut BTreeMap<&'static str, String>);

    impl Visit for Fields<'_> {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name(), value.to_owned());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.0.insert(field.name(), format!("{value:?}"));
        }
    }

    impl<S: Subscriber> Layer<S> for Capture {
        fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _: Context<'_, S>) {
            let mut fields = BTreeMap::new();
            attrs.record(&mut Fields(&mut fields));
            self.0.lock().unwrap().spans.push((id.clone(), attrs.metadata().name(), fields));
        }

        fn on_record(&self, id: &Id, values: &Record<'_>, _: Context<'_, S>) {
            let mut captured = self.0.lock().unwrap();
            let span = captured.spans.iter_mut().rev().find(|(known, ..)| known == id).unwrap();
            values.record(&mut Fields(&mut span.2));
        }

        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            let mut fields = BTreeMap::new();
            event.record(&mut Fields(&mut fields));
            self.0.lock().unwrap().events.push((*event.metadata().level(), fields));
        }
    }
}
