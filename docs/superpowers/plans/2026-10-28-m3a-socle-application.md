# M3a Socle de l'application — plan d'implémentation

> **Origine.** Plan 3a, écrit le 2026-10-28 après la clôture de la M2 (PR #23 mergée, PR documentaire #24 ouverte). Il ne repose sur aucun essai à blanc : les signatures viennent de `docs/plan/02-architecture.md` §5 à §9, de `05-outils-mcp.md` §7 et de `06-qualite.md` §2.2.
>
> **Découpage décidé le 2026-10-28 (Ruling du rédacteur, à confirmer ou corriger à la relecture).** M3 (`07-feuille-de-route.md`, T-M3-01 à T-M3-10) est trop grande pour une PR. Trois plans : **3a** (ce plan) = le socle sans cas d'usage : ports `AuditLog` et `Clock`, modèle d'erreurs et messages, décorateur `Audited`, doublures de test (T-M3-01 reste, T-M3-02, T-M3-03, plus la décision `TooLarge` reportée par le plan 2h) ; **3b** = cas d'usage v0.1, `ResponseBudget` minimal, façade `LegalService`, suite de contrat sur `FakeCorpus` et `PisteCorpus`, reports du domaine (T-M3-04, T-M3-05, T-M3-10) ; **3c** = adapters `lf-adapter-audit` (T-M3-06), `lf-adapter-cache` (T-M3-07), `SystemClock` (T-M3-08) et PR documentaire (T-M3-09, Q9). Sans identifiant PISTE : tout se teste sur doublures et wiremock.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** livrer dans `lf-application` ce dont tout cas d'usage a besoin avant d'exister : les ports d'audit et d'horloge, les erreurs applicatives avec leurs messages définitifs, le décorateur `Audited` qui journalise chaque issue sous une échéance dure, et les doublures de test.

**Architecture :** `lf-application` gagne `ports/audit.rs`, `ports/clock.rs`, `input.rs` (`ToolCall`, `CanonicalInput`, `InputError`), `tool.rs` (`ToolName`), `error.rs` (`ErrorCode`, `UpstreamCause`, `AppError`), `messages.rs` (la seule source des textes de 05 §7.2 à §7.4), `audited.rs` (`Audited<A>`, `CallMeta`, `AuditMetric`, `AuditPolicy`, `AuditMode`) et `testing.rs` (feature `test-support`). `CorpusError` se précise pour que les messages n'aient jamais à analyser un texte (`TooLarge`, statut de `Auth`, nature de `Config`) ; `lf-adapter-piste` suit. Aucun cas d'usage : `Audited::run` reçoit un futur de travail quelconque.

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain 1.99.0), `chrono` 0.4 (`std`), `tracing` 0.1 (`std`), `tokio` 1.53 (`time`, `macros`, `rt` ; dev : `test-util`), `thiserror` 2.0.21, `pretty_assertions`. Aucune dépendance nouvelle au workspace : `chrono` et `tracing` y sont déjà ; `futures` et `similar` (R2) ne servent pas ici.

**Spec :** `docs/plan/02-architecture.md` §3.2, §5.2 à §5.4, §7.1, §9 ; `docs/plan/05-outils-mcp.md` §7.1 à §7.4 ; `docs/plan/06-qualite.md` §2.2 ; `docs/plan/00-decisions.md` §3, §5, §7 ; `docs/plan/07-feuille-de-route.md` T-M3-01 à T-M3-03 ; `docs/plan/annexes/A-parite.md` §2.2 (S1 à S17) ; ADR-0008, ADR-0010, ADR-0012. Reports : PR #23 (`TooLarge`, « HTTP {status} »).

---

## Global Constraints

- Rust edition 2024, MSRV 1.88 (`cargo +1.88 check --workspace --all-targets --locked`), toolchain 1.99.0. Identifiants anglais ASCII ; docs et commentaires en français ; messages d'erreur en anglais, comme ceux de 05 §7.
- `lf-application` : dépendances normales de la liste fermée de R2 (`thiserror`, `chrono` `std`, `tracing`, `futures`, `tokio` `time`/`macros`/`rt`, `similar`) ; `tokio` avec `test-util` en dev seulement ; aucune E/S, aucun `chrono-tz`, aucun `chrono::Utc::now`, aucune lecture d'environnement. Ports : pas d'`async_trait`, pas de `dyn`, aucune méthode par défaut (ADR-0012).
- Le crate ne sait rien de MCP, HTTP ni JSON : `CanonicalInput` est une chaîne opaque calculée ailleurs (`lf-adapter-mcp`), l'annulation arrive comme un futur générique.
- Lints : `clippy::pedantic`, `unwrap_used`, `expect_used`, `panic`, `unreachable`, `unimplemented`, `todo`, `dbg_macro` sous `-D warnings` ; `unwrap`/`expect` permis dans les tests ; aucun `#[allow]` nouveau ; pas d'`assert!(matches!(..))` (comparer des valeurs ou `to_string()`).
- Qualité (00 §10) : TDD (rouge constaté pour la bonne raison, puis vert) ; `make cov` à 100 % (lignes, régions, fonctions, `--all-features`) ; mutation **une fois en fin de plan** (tâche 8), un fichier à la fois par `make mutants FILE=<fichier>` après `make mem-check`, jamais en parallèle. Temps virtuel : `#[tokio::test(start_paused = true)]` partout où l'on mesure une durée.
- WSL (`~/.claude/rules/wsl-safety.md`) : `cargo nextest run -j 2`, jamais `-j 4`, un seul cargo à la fois, temporaires sous `/var/tmp`.
- Aucun secret, jeton ni identifiant dans un message, un test, un journal ou un fichier.
- Commits : un par tâche, message en français au format conventionnel. Rien n'est poussé par l'implémenteur.
- Les fichiers d'adapter touchés par la tâche 3 gardent leurs tests wiremock : aucun test existant n'est supprimé sans équivalent.

## Review Focus

1. **Un échec d'audit ne doit jamais fuiter ni masquer la cause.** `FailingAuditLog` en mode `strict` : le résultat est écarté, le message cite `Original error: [code].` et ne contient aucun chemin ; en `best-effort` le résultat d'origine passe. Tests : tâche 6, `audit_failure_returns_audit_error`, `audit_failure_does_not_mask_original_cause`, `best_effort_keeps_result`.
2. **Annulation et échéance simultanées.** `cancel` prêt au même instant que l'échéance : l'annulation gagne (`biased`), une ligne `error` sans code est écrite, jamais deux lignes. Tests : tâche 6, `cancel_wins_over_deadline_at_same_instant`, `cancelled_call_is_audited`.
3. **Le message d'erreur est borné et sans secret.** Détail de 10 000 caractères, détail contenant `Bearer`/`client_secret` : le champ `error` de l'audit fait au plus 500 caractères ; les `detail` des `CorpusError` ne sont jamais recopiés dans un message de modèle, sauf les cas listés (tâche 4). Tests : `audit_error_field_is_capped_at_500_chars`, `messages_never_echo_corpus_detail`.
4. **`retryable` ne ment pas.** Seuls 502, 503, 504, réseau et timeout invitent à réessayer ; 500, `Decode`, `TooLarge` ne le font jamais. Test : `retry_hint_only_for_retryable_failures`.
5. **Durée de l'erreur jamais nulle.** Une erreur après 200 ms virtuelles porte `duration_ms >= 200` ; une validation instantanée porte 0 sans panique. Tests : `error_duration_is_nonzero`, `duration_of_instant_failure_is_zero`.

---

## File Structure

| Fichier | Rôle |
|---|---|
| `crates/lf-application/Cargo.toml` | + `chrono`, `tracing`, feature `test-support`, `tokio` `macros` en normal |
| `src/ports/audit.rs`, `src/ports/clock.rs` | traits `AuditLog`, `Clock`, `AuditRecord`, `AuditStatus`, `AuditError`, `AuditErrorKind` |
| `src/input.rs`, `src/tool.rs` | `ToolCall`, `CanonicalInput`, `InputError` ; `ToolName` (7 outils v0.1 et `fr_coverage`) |
| `src/error.rs` | `ErrorCode`, `UpstreamCause`, `AppError` |
| `src/messages.rs` | gabarits de 05 §7.2 à §7.4, `corpus_error`, `deadline`, `audit`, `cancelled`, `invalid_argument` |
| `src/audited.rs` | `Audited<A>`, `CallMeta`, `AuditMetric`, `AuditPolicy`, `AuditMode` |
| `src/testing.rs` | `FakeCorpus`, `FixedClock`, `MemoryAuditLog`, `FailingAuditLog` (feature `test-support`) |
| `crates/lf-application/src/ports/corpus.rs` ; `crates/lf-adapter-piste/src/**` | précision de `CorpusError` (tâche 3) |
| `xtask/src/arch.rs` | `tokio` `macros` et `tracing` admis, `test-support` interdit hors dev |

---

### Task 1: Ports d'audit et d'horloge, entrée d'outil, nom d'outil

**Files:**
- Modify: `crates/lf-application/Cargo.toml`, `crates/lf-application/src/lib.rs`, `crates/lf-application/src/ports/mod.rs`, `xtask/src/arch.rs`
- Create: `src/ports/audit.rs`, `src/ports/clock.rs`, `src/input.rs`, `src/tool.rs`

**Interfaces:**
- Produces (types exacts de 02 §5.2 à §5.4) :
  `pub trait AuditLog: Send + Sync + 'static { fn record(&self, record: &AuditRecord) -> impl Future<Output = Result<(), AuditError>> + Send; }`
  `pub struct AuditRecord { pub ts: chrono::DateTime<chrono::Utc>, pub tool: ToolName, pub input: CanonicalInput, pub output_count_or_size: u64, pub duration_ms: u64, pub status: AuditStatus, pub error_code: Option<ErrorCode>, pub error: Option<String>, pub env: PisteEnvironment, pub server_version: &'static str }` (`Debug, Clone`) ; `AuditStatus { Ok, Error }` ; `AuditError { pub kind: AuditErrorKind, pub detail: String }` (`thiserror`, message `audit log unavailable ({kind:?})`) ; `AuditErrorKind { Io, DiskFull, PermissionDenied, Encode, WriterGone }` (`Debug, Clone, Copy, PartialEq, Eq`).
  `pub trait Clock: Send + Sync + 'static { fn now_utc(&self) -> DateTime<Utc>; fn today_paris(&self) -> CivilDate; }`
  `pub struct ToolCall<I> { pub input: Result<I, InputError>, pub canonical: CanonicalInput }` ; `CanonicalInput` (`Clone`, enveloppe `Arc<str>`, `new(impl Into<Arc<str>>)`, `as_str`) ; `InputError { pub message: String }` (`Display` = `message`).
  `ToolName` : newtype sur `&'static str`, `as_str()`, constantes `FR_SEARCH`, `FR_GET_ACT`, `FR_GET_TEXT`, `FR_GET_DECISION`, `FR_GET_DELIBERATION`, `FR_GET_CONVENTION`, `FR_GET_COMPANY_AGREEMENT`, `FR_COVERAGE` (`Copy, Eq, Hash`, `ALL`).
- `PisteEnvironment` se réexporte de `lf_domain::coverage` (`pub use`). `ErrorCode` vient de la tâche 2 : cette tâche déclare d'abord `ErrorCode` dans `error.rs` avec ses six variantes seulement (le reste de `error.rs` est la tâche 2).

- [ ] **Step 1: tests d'abord** — `audit_error_display_names_the_kind_only` (`kind: DiskFull`, `detail` contenant un chemin ⇒ `to_string()` = `audit log unavailable (DiskFull)`), `canonical_input_clones_share_the_text`, `input_error_displays_its_message`, `tool_names_are_unique_and_prefixed` (les 8 noms, tous `fr_`, sans doublon), `tool_name_all_matches_constants`, `port_traits_are_implementable_with_send_futures` (impl minimal + `poll_once` comme `ports/corpus.rs`).
- [ ] **Step 2: rouge** — `cargo nextest run -p lf-application --lib --all-features -j 2 --locked` : échec de compilation (types absents).
- [ ] **Step 3: implémenter** les fichiers ci-dessus ; `Cargo.toml` : `chrono.workspace = true`, `tracing.workspace = true`, `tokio` normal = `["rt", "time", "macros"]` ; feature `test-support = []` ; `xtask/src/arch.rs` : `APPLICATION_FEATURES` ajoute `tracing` → `["std"]` et ne change pas `tokio`/`chrono` ; un test de `check-arch` vérifie qu'un `[dependencies]` de `lf-application` qui active `test-support` d'un autre membre est refusé (même mécanisme que les features de R2).
- [ ] **Step 4: vert** — mêmes commandes ; `cargo xtask check-arch` ; `cargo clippy -p lf-application --all-features --all-targets --locked -- -D warnings`.
- [ ] **Step 5: commit** — `feat(application): ports AuditLog et Clock, ToolCall et ToolName (plan 3a, tâche 1)`.

---

### Task 2: `ErrorCode`, `UpstreamCause`, `AppError`

**Files:**
- Create/Modify: `crates/lf-application/src/error.rs`, `src/lib.rs`

**Interfaces:**
- Produces :
  `pub enum ErrorCode { InvalidArg, NotFound, UpstreamError, RateLimited, ConfigError, AuditError }` ; `ErrorCode::ALL: [ErrorCode; 6]` ; `as_str(self) -> &'static str` rendant exactement `invalid_arg`, `not_found`, `upstream_error`, `rate_limited`, `config_error`, `audit_error` (source unique, 02 §9.1).
  `pub enum UpstreamCause { Status(u16), Network, Timeout, Deadline, Decode, Cancelled }` (`Debug, Clone, Copy, PartialEq, Eq`).
  `pub enum AppError { InvalidArgument { message: String }, NotFound { message: String }, Upstream { message: String, cause: UpstreamCause }, RateLimited { message: String, retry_after: Option<Duration> }, Config { message: String }, Audit { message: String, original: Option<ErrorCode> } }` (`Debug, Clone, PartialEq, Eq`, `Display` = message, `std::error::Error`).
  Méthodes : `code(&self) -> Option<ErrorCode>` (`None` seulement pour `Upstream { cause: Cancelled }`), `message(&self) -> &str`, `cause(&self) -> Option<UpstreamCause>`.
  Constructeurs `AppError::{invalid_argument, not_found, config}(impl Into<String>)` ; `From<InputError> for AppError` ⇒ `InvalidArgument { message }` (le message de décodage tel quel, 02 §9.2 ligne 1).

- [ ] **Step 1: tests d'abord** — `error_codes_are_the_six_documented_strings` (compare `ErrorCode::ALL.map(as_str)` à la liste de 02 §9.1), `error_code_all_has_no_duplicates`, `app_error_code_per_variant` (table : variante → code attendu, y compris `Upstream` ⇒ `UpstreamError`, `Audit` ⇒ `AuditError`, `Cancelled` ⇒ `None`), `input_error_becomes_invalid_argument_with_same_text`, `display_is_the_final_message`.
- [ ] **Step 2: rouge** ; **Step 3: implémenter** ; **Step 4: vert** (commande de la tâche 1 + `make cov` sur `error.rs`) ; **Step 5: commit** — `feat(application): ErrorCode, UpstreamCause et AppError (plan 3a, tâche 2)`.

---

### Task 3: Préciser `CorpusError` (décision `TooLarge`, statut d'`Auth`, nature de `Config`)

**Décision (Ruling, plan 3a).** Le plan 2h reportait ici le choix entre une variante typée `TooLarge` et un test de préfixe de `detail`, plus le formatage « HTTP {status} ». Les messages de 05 §7.2 distinguent des cas que `Auth { detail }` et `Config { detail }` ne portent qu'en texte libre (« HTTP 403 on … », message de configuration, 404 de routage, 404 OAuth). Analyser ces textes couplerait `messages` à l'adapter ; on les type. Coût si le propriétaire préfère les textes : retour arrière de cette seule tâche, `messages` gardant alors un seul gabarit d'accès.

**Files:**
- Modify: `crates/lf-application/src/ports/corpus.rs`, `crates/lf-adapter-piste/src/{http,classify,corpus,corpus/port}.rs` (et tout fichier que `cargo check --all-features` désigne)

**Interfaces:**
- Produces :
  `CorpusError::TooLarge { detail: String }` — `#[error("response too large")]`, jamais réessayé ; remplace le `Decode` à préfixe `response too large` de `act_too_large`. L'adapter le rend pour **tout** corps au-dessus de la borne (pas seulement `lawDecree`) ; le cas d'usage seul décide du message (tâche 4).
  `CorpusError::Auth { status: Option<u16>, detail: String }` — `Some(401)` après un second 401 ou un 400/401 OAuth, `Some(403)` pour un 403 de l'API.
  `CorpusError::Config { kind: ConfigKind, detail: String }` avec `pub enum ConfigKind { Settings, ApiEndpoint, OAuthEndpoint }` (`Debug, Clone, Copy, PartialEq, Eq`) : `Settings` = `detail` est déjà un message final sans secret (identifiants absents, `LF_PISTE_ENV` invalide, `EnvMismatch`, client HTTP non constructible, OAuth 200 sans `access_token`) ; `ApiEndpoint` = 404/405 de routage sur l'API ; `OAuthEndpoint` = 404 de l'endpoint token.
- Supprime : `TOO_LARGE_PREFIX` comme critère de classification (la constante peut rester pour le texte de `detail`), `act_too_large` et ses tests de préfixe/suffixe.

- [ ] **Step 1: tests d'abord, dans l'adapter** — conserver chaque test existant en changeant l'attendu : `lawdecree_too_large_is_typed` (corps > 16 Mio sur `/consult/lawDecree` ⇒ `TooLarge`), `non_lawdecree_too_large_is_typed_too` (même corps sur `/consult/juri`), `second_401_is_auth_with_status_401`, `api_403_json_is_auth_with_status_403`, `oauth_404_is_config_oauth_endpoint`, `api_404_routing_is_config_api_endpoint`, `missing_credentials_is_config_settings`, `oauth_without_access_token_is_config_settings`. Côté port : `messages_carry_no_secret_and_name_the_failure` étendu aux nouvelles formes (`TooLarge` ⇒ `response too large` ; `Auth` ⇒ `credentials rejected` ; `Config` ⇒ `configuration error: {detail}`).
- [ ] **Step 2: rouge** (échec de compilation puis d'assertions) ; **Step 3: implémenter** — types dans `corpus.rs` du port ; l'adapter renseigne `status`/`kind` là où il construit ces variantes (liste : `grep -rn "CorpusError::\(Auth\|Config\)" crates/lf-adapter-piste/src`, une quinzaine de sites de production, dont `classify.rs:149,161,196` et `corpus.rs:374,376,419`) ; le critère de `TooLarge` est le chemin de `too_large()` de `http.rs`, plus de comparaison de texte. **Step 4: vert** — `cargo nextest run --workspace --all-features -j 2 --locked` ; `make cov` ; `make arch`. **Step 5: commit** — `refactor(application,adapter): CorpusError précise TooLarge, le statut d'Auth et la nature de Config (plan 3a, tâche 3)`.

---

### Task 4: Module `messages`

**Files:**
- Create: `crates/lf-application/src/messages.rs`, `src/lib.rs` (mod)

**Interfaces:**
- Consumes : `CorpusError` (tâche 3), `AppError`, `UpstreamCause`, `ToolName`, ids du domaine.
- Produces :
  `pub fn corpus_error(tool: ToolName, subject: &str, error: &CorpusError) -> AppError` — `subject` est l'identifiant demandé tel qu'il sera cité (ou vide pour `fr_search`).
  `pub fn deadline(budget: Duration) -> AppError` ; `pub fn cancelled() -> AppError` ; `pub fn audit(error: &AuditError, original: Option<ErrorCode>) -> AppError` ; `pub fn invalid_argument(message: impl Into<String>) -> AppError` ; `pub fn clip(message: &str, max_chars: usize) -> String` (coupe à `max_chars` caractères, sans scinder un point de code).
- Gabarits (texte exact de 05 §7.2 à §7.4, `{n}` = secondes arrondies au supérieur) :
  - `NotFound` par outil (§7.4) : `fr_get_act` ⇒ `No LODA text found for {id}. Check the id, or search with fr_search fond=LODA or CODE.` ; `fr_get_text` ⇒ `No article found for {id}. Check the id, or find the article with fr_get_act or fr_search.` ; `fr_get_decision` ⇒ `No {JURI|CONSTIT|CETAT} decision found for {id}. Check the id, or search with fr_search.` (fond selon le préfixe du sujet : `JURITEXT`, `CONSTEXT`, `CETATEXT` ; autre préfixe ⇒ `JURI`, ne jamais paniquer) ; `fr_get_deliberation` ⇒ `No CNIL deliberation found for {id}. Check the id, or search with fr_search.` ; `fr_get_convention` ⇒ `No collective agreement text found for {id}. Check the id, or search with fr_search.` ; `fr_get_company_agreement` ⇒ `No company agreement found for {id}. Check the id, or search with fr_search.`. `fr_search` et `fr_coverage` ne peuvent pas recevoir `NotFound` (04 §11.2) : défensivement `Upstream { cause: Decode }` « unexpected response from Legifrance; report it. » (Ruling : la phrase d'aide des trois derniers outils, que 05 §7.4 écrit « … », est celle de `fr_get_decision`).
  - `RateLimited` ⇒ `PISTE quota reached; retry in {n} s.` ou `PISTE quota reached; retry in a few seconds.` sans `retry_after`.
  - `Upstream { status: Some(s) }` ⇒ `Legifrance/PISTE HTTP {s} ({raison})` avec raison `internal server error` (500), `bad gateway` (502), `service unavailable` (503), `gateway timeout` (504), parenthèse omise pour tout autre statut ; suffixe `; retry once, then report it.` si `retryable`, sinon `; report it.`. `status: None` ⇒ `could not reach Legifrance/PISTE (connection failed)` + même suffixe. Cause : `Status(s)` ou `Network`.
  - `Timeout` ⇒ `Legifrance/PISTE did not answer in time; retry once, then report it.` (Ruling : `CorpusError::Timeout` ne porte pas de durée ; le gabarit de 05 §7.2 avec `{n} s` n'est pas rédigeable sans la changer), cause `Timeout`.
  - `InvalidRequest { status: Some(s), detail }` ⇒ `Legifrance rejected the request (HTTP {s}): {detail}` ; `status: None` ⇒ `the request was not sent to Legifrance ({detail}); report it.` ; `detail` coupé à 200 caractères ; cause `Status(s)` ou `Decode` pour `None`.
  - `Decode` ⇒ `unexpected response from Legifrance; report it.` ; `TooLarge`, outil `fr_get_act` ⇒ `this text is too large for get_act; use get_article or search instead.` ; `TooLarge`, autre outil ⇒ le message de `Decode`. Cause `Decode` dans les deux cas.
  - `Auth { status }` ⇒ `Config` : `Some(403)` ⇒ `PISTE denied access (HTTP 403): check that the PISTE application is subscribed to the Legifrance API.` ; `Some(s)` autre ⇒ `PISTE rejected the credentials (HTTP {s}): check LF_PISTE_CLIENT_ID, LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV.` ; `None` ⇒ même phrase sans `(HTTP {s})`.
  - `Config { kind }` : `Settings` ⇒ `detail` tel quel, coupé à 500 ; `ApiEndpoint` ⇒ `Legifrance endpoint not found: check LF_PISTE_BASE_URL.` ; `OAuthEndpoint` ⇒ `PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.`.
  - `deadline` ⇒ `tool deadline of {n} s exceeded; retry, or narrow the request (smaller page_size, fewer fonds).`, cause `Deadline`.
  - `cancelled` ⇒ message `cancelled by client`, cause `Cancelled` (jamais montré au modèle).
  - `audit` ⇒ `audit log unavailable; the call was not recorded and its result is withheld (see server logs).` suivi, si `original` est `Some(c)`, de ` Original error: [{c}].` ; jamais le `detail` de `AuditError`.

- [ ] **Step 1: tests d'abord** — `corpus_error_maps_to_app_error_table` (toutes les variantes de `CorpusError` × code et cause de 02 §9.2 lignes 5 à 19), `not_found_messages_match_reference` (6 outils × `what` d'un 404 de ressource, d'un 200 vide et d'un 400 « valider est fausse » : 3 origines, même message ; première phrase de la référence ; CONSTEXT et CETATEXT ⇒ `CONSTIT`/`CETAT`, E-15), `decision_fond_follows_prefix_and_never_panics` (préfixe inconnu, chaîne vide), `error_matrix_s1_to_s17` (les lignes de `annexes/A-parite.md` §2.2 qui relèvent des erreurs de corpus et d'audit : S6 à S15, S17 ; S1 à S5 appartiennent à 3b), `retry_hint_only_for_retryable_failures`, `messages_never_echo_corpus_detail` (détail contenant `Bearer abc` et `client_secret=x` pour chaque variante hors `Config::Settings` et `InvalidRequest` : absent du message), `invalid_request_detail_is_clipped_to_200_chars`, `config_settings_detail_is_clipped_to_500_chars`, `audit_message_names_no_path` (`detail` = chemin ⇒ absent), `audit_message_cites_original_code`, `deadline_rounds_up_seconds` (60 s, 59,2 s ⇒ 60), `clip_does_not_split_code_points` (accents et émoji).
- [ ] **Step 2: rouge** ; **Step 3: implémenter** (fonctions pures, `match` exhaustifs sans `_` pour qu'une variante ajoutée casse la compilation) ; **Step 4: vert** + `make cov` sur `messages.rs` ; **Step 5: commit** — `feat(application): messages des erreurs, source unique de 05 §7.2 à §7.4 (plan 3a, tâche 4)`.

---

### Task 5: Doublures de test (`testing`, feature `test-support`)

**Files:**
- Create: `crates/lf-application/src/testing.rs`

**Interfaces:**
- Produces (06 §2.2) :
  `FakeCorpus` : file `VecDeque<Result<T, CorpusError>>` programmable par méthode (`push_search`, `push_act`, …, un `push_*` par méthode du port), délai optionnel `with_delay(Duration)` par `tokio::time::sleep`, `calls() -> usize` total et `calls_of(&str)` par nom de méthode, `max_concurrency() -> usize`, `seen_deadlines() -> Vec<Duration>` (la durée restante de `Deadline::current_or(DEFAULT)` lue à chaque appel). File vide ⇒ `Err(CorpusError::Config { kind: Settings, detail: "FakeCorpus: no response programmed for {method}" })`.
  `FixedClock::new(now_utc: DateTime<Utc>, today_paris: CivilDate)` ; `MemoryAuditLog` (`records() -> Vec<AuditRecord>`, `with_gate()` : `record` ne rend la main qu'après `open_gate()`) ; `FailingAuditLog::new(kind: AuditErrorKind)`.
- Le module est `#[cfg(any(test, feature = "test-support"))]`.

- [ ] **Step 1: tests d'abord** — `fake_corpus_returns_programmed_responses_in_order`, `fake_corpus_counts_calls_per_method`, `fake_corpus_delay_uses_virtual_time` (200 ms virtuelles), `fake_corpus_tracks_max_concurrency` (3 appels `join!`), `fake_corpus_reports_remaining_deadline`, `empty_queue_is_a_config_error_naming_the_method`, `fixed_clock_returns_both_values`, `memory_audit_log_keeps_records_in_order`, `gated_audit_log_blocks_until_opened`, `failing_audit_log_fails_with_each_kind`.
- [ ] **Step 2: rouge** ; **Step 3: implémenter** (`std::sync::Mutex` sans garde tenue à travers un `.await`) ; **Step 4: vert** ; `cargo nextest run -p lf-application --all-features` ; **Step 5: commit** — `test(application): FakeCorpus, FixedClock, MemoryAuditLog et FailingAuditLog (plan 3a, tâche 5)`.

---

### Task 6: `Audited`

**Files:**
- Create: `crates/lf-application/src/audited.rs` ; Modify: `src/lib.rs`

**Interfaces:**
- Consumes : tâches 1 à 5.
- Produces :
  `pub trait AuditMetric { fn output_count_or_size(&self) -> u64; }`
  `pub struct CallMeta { /* tool, ts, canonical */ }` avec `CallMeta::new(tool: ToolName, ts: DateTime<Utc>, canonical: CanonicalInput)`.
  `pub enum AuditMode { Strict, BestEffort }` (`Default` interdit : le défaut `Strict` est posé par `lf-app`, 3c).
  `pub struct AuditPolicy { pub mode: AuditMode, pub deadline: Duration, pub env: PisteEnvironment, pub server_version: &'static str }`.
  `pub struct Audited<A: AuditLog> { /* log, policy */ }` ; `Audited::new(log: A, policy: AuditPolicy) -> Self` (aucune décision dans `new`) ; `pub async fn run<O, W, X>(&self, meta: CallMeta, cancel: X, work: W) -> Result<O, AppError> where O: AuditMetric, W: Future<Output = Result<O, AppError>> + Send, X: Future<Output = ()> + Send`.
- Algorithme (02 §7.1, avec l'API livrée de `Deadline`) : `started = tokio::time::Instant::now()` ; `work` enveloppé par `Deadline::after(policy.deadline).scope(work)` ; `tokio::select! { biased; () = cancel => Cancelled, r = tokio::time::timeout(policy.deadline, work) => Done(r.unwrap_or_else(|_| Err(messages::deadline(policy.deadline)))) }` ; `duration_ms = started.elapsed().as_millis()` en `u64` saturé ; `AuditRecord` : succès ⇒ `Ok`, métrique de `O`, pas de code ; échec ⇒ `Error`, métrique 0, `error_code = e.code()` (absent pour l'annulation), `error = clip(e.message(), 500)` ; écriture par `log.record(&record).await` ; mode `Strict` + échec d'écriture ⇒ `messages::audit(&error, original_code)` en écartant le résultat ; mode `BestEffort` ⇒ `tracing::warn!(kind = ?kind, "audit write failed")` et résultat d'origine ; annulation ⇒ ligne écrite (une tentative, échec ignoré avec `warn!`), retour `messages::cancelled()`. Span `tool_call` avec `tool`, `status`, `error_code`, `duration_ms`, `output` renseignés après coup ; `invalid_arg` et `not_found` journalisés en `info`, les autres erreurs en `warn` (D-08).

- [ ] **Step 1: tests d'abord, dans `audited.rs`** (table pilotée `audit_written_for_every_outcome`, une entrée par ligne de 06 §2.2 ; les lignes 2 et 3 sont rendues par un `work` qui échoue en `invalid_argument` sans appeler le port — le chemin par `LegalService` est retesté en 3b) : les 15 issues, avec `lines`, `status`, `error_code`, `output_count_or_size`, `duration_ms >= min`, code retourné et nombre d'appels au port ; puis `error_duration_is_nonzero` (échec après 200 ms virtuelles), `duration_of_instant_failure_is_zero`, `deadline_maps_to_upstream_error_before_deadline_plus_epsilon` (porte jamais résolue, échéance 1 s : retour entre 1 000 et 1 010 ms virtuelles, cause `Deadline`), `deadline_is_published_to_the_port` (`FakeCorpus::seen_deadlines` : `LF_TOOL_DEADLINE_S` moins le temps écoulé, y compris au second appel ; hors `run` ⇒ le défaut), `cancelled_call_is_audited` (une ligne `error`, `error_code` absent, `error = "cancelled by client"`, durée ≈ 100 ms), `cancel_wins_over_deadline_at_same_instant`, `audit_failure_returns_audit_error`, `audit_failure_does_not_mask_original_cause`, `best_effort_keeps_result`, `audit_write_is_acknowledged_before_tool_returns` (`MemoryAuditLog::with_gate` : `run` reste pendant tant que la porte est fermée), `audit_metric_is_taken_from_the_output` (valeur renvoyée par l'implémentation de test d'`AuditMetric`), `audit_error_field_is_capped_at_500_chars`, `invalid_arg_is_not_logged_as_error` (subscriber de capture : `INFO`, pas `ERROR` ni `WARN`), `record_carries_env_version_and_timestamp` (champs de `CallMeta` et de la politique recopiés).
- [ ] **Step 2: rouge** ; **Step 3: implémenter** ; **Step 4: vert** + `make cov` sur `audited.rs` ; **Step 5: commit** — `feat(application): Audited journalise chaque issue sous une échéance dure (plan 3a, tâche 6)`.

---

### Task 7: Arborescence, documents et lints

**Files:** `docs/plan/02-architecture.md` (§5.2, §5.3, §9.1 : « non compilé » ⇒ « livré, plan 3a », `CorpusError` précisé), `docs/plan/04-adapter-piste.md` (§7.3 : `TooLarge` typé, plus d'appariement de texte), `docs/plan/05-outils-mcp.md` (§7.2 : lignes `Upstream (Decode)`, `Auth`, `Config`, `Timeout` ; les Ruling de la tâche 4), `docs/plan/07-feuille-de-route.md` (T-M3-01 ⇒ livrée en partie ; T-M3-02, T-M3-03 ⇒ livrées), `CHANGELOG.md`, `docs/plan/09-risques-et-questions.md` (D-03, D-08, D-14, D-16 : liens vers les tests ; R-20 et R-21 : état).

- [ ] **Step 1:** pour chaque modification, vérifier d'abord le texte actuel (`grep`), ne changer que ce que la livraison rend faux, garder les étiquettes `[V-xx · tag]`.
- [ ] **Step 2: `make fmt-check lint`, `make test`, `make doc`, `make arch`, `make -k ci`** (seul `deny` peut rester rouge si une alerte neuve tombe hors plan ; le dire).
- [ ] **Step 3: commit** — `docs: socle de l'application livré, TooLarge typé (plan 3a, tâche 7)`.

---

### Task 8: Mutation, couverture et revue de fin

- [ ] **Step 1:** `make mem-check`, puis `make mutants FILE=crates/lf-application/src/<f>.rs` pour `error.rs`, `messages.rs`, `audited.rs`, `ports/audit.rs`, `input.rs`, `tool.rs`, `testing.rs`, un fichier à la fois ; côté adapter, `make mutants-diff` sur la tâche 3. Cible : 0 `MISSED`, 0 `TIMEOUT`. Les mutants de `new` et de `Default` ne sont pas mutés : aucune décision n'y vit.
- [ ] **Step 2:** `make cov` à 100 % ; `cargo +1.88 check --workspace --all-targets --locked`.
- [ ] **Step 3:** revue Rust (`code-reviewer`) et audit de sécurité (`security-auditor` : le crate écrit le journal et rédige les messages vus par le modèle) ; un tour de correction ; commit des corrections.

---

## Hors plan, à reporter dans 3b et 3c

- **3b** : cas d'usage et `ResponseBudget` minimal ; `LegalService` ; `error_matrix_s1_to_s17` pour S1 à S5 ; `audit_written_for_every_outcome` rejoué par la façade ; suite de contrat `contract::corpus_contract` sur `FakeCorpus` et `PisteCorpus` ; T-M3-10 (`Ord` sur les identifiants, forme de `Coverage`, features de `tokio` dans `check-arch`, `disallowed-methods`).
- **3c** : `JsonlAuditLog` (V-09, V-164 à V-167), `CachedCorpus` (V-08), `SystemClock` (V-17), valeur par défaut `Strict` posée par `lf-app`, T-M3-09 (Q9, 09 §3.16).
