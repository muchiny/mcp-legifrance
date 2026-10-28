# M3a Socle de l'application — plan d'implémentation

> **Origine.** Plan 3a, écrit le 2026-10-28 après la clôture de la M2 (PR #23 mergée, PR documentaire #24 ouverte). Il ne repose sur aucun essai à blanc : les signatures viennent de `docs/plan/02-architecture.md` §5 à §9, de `05-outils-mcp.md` §7 et de `06-qualite.md` §2.2.
>
> **Découpage décidé le 2026-10-28 (Ruling du rédacteur, à confirmer ou corriger à la relecture).** M3 (`07-feuille-de-route.md`, T-M3-01 à T-M3-10) est trop grande pour une PR. Trois plans : **3a** (ce plan) = le socle sans cas d'usage : ports `AuditLog` et `Clock`, modèle d'erreurs et messages, décorateur `Audited`, doublures de test (T-M3-01 reste, T-M3-02, T-M3-03, plus la décision `TooLarge` reportée par le plan 2h) ; **3b** = cas d'usage v0.1, `ResponseBudget` minimal, façade `LegalService`, suite de contrat sur `FakeCorpus` et `PisteCorpus`, reports du domaine (T-M3-04, T-M3-05, T-M3-10) ; **3c** = adapters `lf-adapter-audit` (T-M3-06), `lf-adapter-cache` (T-M3-07), `SystemClock` (T-M3-08) et PR documentaire (T-M3-09, Q9). Sans identifiant PISTE : tout se teste sur doublures et wiremock.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** livrer dans `lf-application` ce dont tout cas d'usage a besoin avant d'exister : les ports d'audit et d'horloge, les erreurs applicatives avec leurs messages définitifs, le décorateur `Audited` qui journalise chaque issue sous une échéance dure, et les doublures de test.

**Architecture :** `lf-application` gagne `ports/audit.rs`, `ports/clock.rs`, `input.rs` (`ToolCall`, `CanonicalInput`, `InputError`), `tools.rs` (`ToolName`, nom de 02 §2.1), `error.rs` (`ErrorCode`, `UpstreamCause`, `AppError`), `messages.rs` (la seule source des textes de 05 §7.2 à §7.4), `audited.rs` (`Audited<A>`, `CallMeta`, `AuditMetric`, `AuditPolicy`, `AuditMode`, compteur d'échecs d'écriture ; `AuditMetric` y vit tant qu'aucune sortie d'outil n'existe, 3b la déplacera avec `output.rs` si besoin) et `testing.rs` (feature `test-support`). `CorpusError` se précise pour que les messages n'aient jamais à analyser un texte (`TooLarge`, origine et statut d'`Auth`, nature de `Config`) ; `lf-adapter-piste` suit. Aucun cas d'usage : `Audited::run` reçoit un futur de travail quelconque.

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain 1.99.0), `chrono` 0.4 (`std`), `tracing` 0.1 (`std`), `tokio` 1.53 (`time`, `macros`, `rt` ; dev : `test-util`), `thiserror` 2.0.21, `pretty_assertions`. Aucune dépendance nouvelle au workspace : `chrono` et `tracing` y sont déjà ; `futures` et `similar` (R2) ne servent pas ici.

**Spec :** `docs/plan/02-architecture.md` §3.2, §5.2 à §5.4, §7.1, §9 ; `docs/plan/05-outils-mcp.md` §7.1 à §7.4 ; `docs/plan/06-qualite.md` §2.2 ; `docs/plan/00-decisions.md` §3, §5, §7 ; `docs/plan/07-feuille-de-route.md` T-M3-01 à T-M3-03 ; `docs/plan/annexes/A-parite.md` §2.2 (S1 à S17) ; ADR-0008, ADR-0010, ADR-0012. Reports : PR #23 (`TooLarge`, « HTTP {status} »).

---

## Global Constraints

- Rust edition 2024, MSRV 1.88 (`cargo +1.88 check --workspace --all-targets --locked`), toolchain 1.99.0. Identifiants anglais ASCII ; docs et commentaires en français ; messages d'erreur en anglais, comme ceux de 05 §7.
- `lf-application` : dépendances normales de la liste fermée de R2 (`thiserror`, `chrono` `std`, `tracing`, `futures`, `tokio` `time`/`macros`/`rt`, `similar`) ; `tokio` avec `test-util` et `tracing-subscriber` en dev seulement (R6 ne confine que les dépendances normales) ; aucune E/S, aucun `chrono-tz`, aucun `chrono::Utc::now`, aucune lecture d'environnement. Ports : pas d'`async_trait`, pas de `dyn`, aucune méthode par défaut (ADR-0012).
- Le crate ne sait rien de MCP, HTTP ni JSON : `CanonicalInput` est une chaîne opaque calculée ailleurs (`lf-adapter-mcp`), l'annulation arrive comme un futur générique.
- Lints : `clippy::pedantic`, `unwrap_used`, `expect_used`, `panic`, `unreachable`, `unimplemented`, `todo`, `dbg_macro` sous `-D warnings` ; `unwrap`/`expect` permis dans les tests ; aucun `#[allow]` nouveau ; pas d'`assert!(matches!(..))` (comparer des valeurs ou `to_string()`).
- Qualité (00 §10) : TDD (rouge constaté pour la bonne raison, puis vert) ; `make cov` à 100 % (lignes, régions, fonctions, `--all-features`) ; mutation **une fois en fin de plan** (tâche 8), un fichier à la fois par `make mutants FILE=<fichier>` après `make mem-check`, jamais en parallèle. Temps virtuel : `#[tokio::test(start_paused = true)]` partout où l'on mesure une durée.
- WSL (`~/.claude/rules/wsl-safety.md`) : `cargo nextest run -j 2`, jamais `-j 4`, un seul cargo à la fois, temporaires sous `/var/tmp`.
- Aucun secret, jeton ni identifiant dans un message, un test, un journal ou un fichier.
- Commits : un par tâche, message en français au format conventionnel. Rien n'est poussé par l'implémenteur.
- Les fichiers d'adapter touchés par la tâche 3 gardent leurs tests wiremock : aucun test existant n'est supprimé sans équivalent.

## Review Focus

1. **Un échec d'audit ne doit jamais fuiter ni masquer la cause.** `FailingAuditLog` en mode `strict` : le résultat est écarté, le message cite `Original error: [code].` et ne contient aucun chemin ; en `best-effort` le résultat d'origine passe. Tests : tâche 6, `audit_failure_returns_audit_error`, `audit_failure_does_not_mask_original_cause`, `best_effort_keeps_result` (qui lit aussi `Audited::write_failures()`).
2. **Annulation et échéance simultanées.** `cancel` prêt au même instant que l'échéance : l'annulation gagne (`biased`), une ligne `error` sans code est écrite, jamais deux lignes. Tests : tâche 6, `cancel_wins_over_deadline_at_same_instant`, `cancelled_call_is_audited`.
3. **Le message d'erreur est borné et sans secret.** Détail de 10 000 caractères, détail contenant `Bearer`/`client_secret` : le champ `error` de l'audit fait au plus 500 caractères ; les `detail` des `CorpusError` ne sont jamais recopiés dans un message de modèle, sauf les cas listés (tâche 4). Tests : `audit_error_field_is_capped_at_500_chars`, `messages_never_echo_corpus_detail`.
4. **`retryable` ne ment pas.** Seuls 502, 503, 504, réseau et timeout invitent à réessayer ; 500, `Decode`, `TooLarge` ne le font jamais. Test : `retry_hint_only_for_retryable_failures`.
5. **Durée de l'erreur jamais nulle.** Une erreur après 200 ms virtuelles porte `duration_ms >= 200` ; une validation instantanée porte 0 sans panique. Tests : `error_duration_is_nonzero`, `duration_of_instant_failure_is_zero`.

---

## File Structure

| Fichier | Rôle |
|---|---|
| `crates/lf-application/Cargo.toml`, `Cargo.lock` | + `chrono`, `tracing`, feature `test-support`, `tokio` `macros` en normal ; dev : `tracing-subscriber` |
| `src/ports/audit.rs`, `src/ports/clock.rs` | traits `AuditLog`, `Clock`, `AuditRecord`, `AuditStatus`, `AuditError`, `AuditErrorKind` |
| `src/input.rs`, `src/tools.rs` | `ToolCall`, `CanonicalInput`, `InputError` ; `ToolName` (7 outils v0.1 et `fr_coverage`) |
| `src/error.rs` | `ErrorCode`, `UpstreamCause`, `AppError` |
| `src/messages.rs` | gabarits de 05 §7.2 à §7.4, `corpus_error`, `deadline`, `audit`, `cancelled`, `invalid_argument` |
| `src/audited.rs` | `Audited<A>`, `CallMeta`, `AuditMetric`, `AuditPolicy`, `AuditMode` |
| `src/testing.rs` | `FakeCorpus`, `FixedClock`, `MemoryAuditLog`, `FailingAuditLog` (feature `test-support`) |
| `crates/lf-application/src/ports/corpus.rs` ; `crates/lf-adapter-piste/src/**`, `tests/live.rs` | précision de `CorpusError` (tâche 3) |
| `xtask/src/arch.rs` | `tracing` ajouté à `APPLICATION_FEATURES` (tableau de 2 à 3 entrées) ; nouvelle règle : aucun membre n'active `lf-application/test-support` en dépendance normale |

---

### Task 1: Ports d'audit et d'horloge, entrée d'outil, nom d'outil

**Files:**
- Modify: `crates/lf-application/Cargo.toml`, `crates/lf-application/src/lib.rs`, `crates/lf-application/src/ports/mod.rs`, `xtask/src/arch.rs`
- Create: `src/ports/audit.rs`, `src/ports/clock.rs`, `src/input.rs`, `src/tools.rs`

**Interfaces:**
- Produces (types exacts de 02 §5.2 à §5.4) :
  `pub trait AuditLog: Send + Sync + 'static { fn record(&self, record: &AuditRecord) -> impl Future<Output = Result<(), AuditError>> + Send; }`
  `pub struct AuditRecord { pub ts: chrono::DateTime<chrono::Utc>, pub tool: ToolName, pub input: CanonicalInput, pub output_count_or_size: u64, pub duration_ms: u64, pub status: AuditStatus, pub error_code: Option<ErrorCode>, pub error: Option<String>, pub env: PisteEnvironment, pub server_version: &'static str }` (`Debug, Clone`) ; `AuditStatus { Ok, Error }` ; `AuditError { pub kind: AuditErrorKind, pub detail: String }` (`thiserror`, message `audit log unavailable ({kind:?})`) ; `AuditErrorKind { Io, DiskFull, PermissionDenied, Encode, WriterGone }` (`Debug, Clone, Copy, PartialEq, Eq`).
  `pub trait Clock: Send + Sync + 'static { fn now_utc(&self) -> DateTime<Utc>; fn today_paris(&self) -> CivilDate; }`
  `pub struct ToolCall<I> { pub input: Result<I, InputError>, pub canonical: CanonicalInput }` ; `CanonicalInput` (`Clone`, enveloppe `Arc<str>`, `new(impl Into<Arc<str>>)`, `as_str`) ; `InputError { pub message: String }` (`Display` = `message`).
  `ToolName` (fichier `tools.rs`) : newtype sur `&'static str`, `as_str()`, constantes `FR_SEARCH`, `FR_GET_ACT`, `FR_GET_TEXT`, `FR_GET_DECISION`, `FR_GET_DELIBERATION`, `FR_GET_CONVENTION`, `FR_GET_COMPANY_AGREEMENT`, `FR_COVERAGE` (`Copy, Eq, Hash`, `ALL`).
- `PisteEnvironment` se réexporte de `lf_domain::coverage` (`pub use`). `ErrorCode` vient de la tâche 2 : cette tâche déclare d'abord `ErrorCode` dans `error.rs` avec ses six variantes seulement (le reste de `error.rs` est la tâche 2).

- [ ] **Step 1: tests d'abord** — `audit_error_display_names_the_kind_only` (`kind: DiskFull`, `detail` contenant un chemin ⇒ `to_string()` = `audit log unavailable (DiskFull)`), `canonical_input_clones_share_the_text`, `input_error_displays_its_message`, `tool_names_are_unique_and_prefixed` (les 8 noms, tous `fr_`, sans doublon), `tool_name_all_matches_constants`, `port_traits_are_implementable_with_send_futures` (impl minimal + `poll_once` comme `ports/corpus.rs`).
- [ ] **Step 2: rouge** — `cargo nextest run -p lf-application --lib --all-features -j 2 --locked` : échec de compilation (types absents).
- [ ] **Step 3: implémenter** les fichiers ci-dessus ; `Cargo.toml` : `chrono.workspace = true`, `tracing.workspace = true`, `tokio` normal = `["rt", "time", "macros"]` ; feature `test-support = []` ; dev : `tracing-subscriber.workspace = true`, `tokio` avec `test-util`. `Cargo.lock` : le régénérer sans `--locked` (`cargo update -w` ou `cargo check`), aucun crate externe nouveau n'y entre (vérifier au `git diff`), et l'inclure dans le commit. `xtask/src/arch.rs` : `APPLICATION_FEATURES` passe de 2 à 3 entrées (`tracing` ⇒ `["std"]`, test d'`arch.rs` autour des lignes 1254-1257 à étendre) ; **nouvelle règle**, avec son test sur un manifeste de fixture : pour tout membre autre que `lf-application`, la dépendance normale `lf-application` n'active jamais la feature `test-support` (elle n'est permise qu'en `[dev-dependencies]`) ; le test échoue d'abord sur un manifeste qui l'active en normal.
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

### Task 3: Préciser `CorpusError` (décision `TooLarge`, origine d'`Auth`, nature de `Config`)

**Décision (Ruling, plan 3a).** Le plan 2h reportait ici le choix entre une variante typée `TooLarge` et un test de préfixe de `detail`, plus le formatage « HTTP {status} ». Les `detail` de l'adapter sont des textes de **journal** (04 §3.5, §11.3), pas des messages pour le modèle : sans point final, parfois avec une URL, parfois plus courts que 05 §7.2 (`CREDENTIALS_MISSING` dans `corpus.rs`, `PISTE OAuth returned no access_token` et `…response is not valid JSON at {url}` dans `auth.rs`, `Display` de `ConfigProblem`). `messages` ne doit ni les recopier tels quels ni les analyser : on type ce qu'il doit distinguer et `messages` rédige les textes de 05 §7.2 seul. Coût si le propriétaire préfère les textes : retour arrière de cette seule tâche.

**Files:**
- Modify: `crates/lf-application/src/ports/corpus.rs` ; `crates/lf-adapter-piste/src/{auth,classify,corpus,http,retry}.rs`, `src/corpus/{port,raw}.rs`, `tests/live.rs` (attendu `TooLarge` de `live_lawdecree_on_code_size`) et tout fichier que `cargo check --workspace --all-targets --all-features` désigne ; les chaînes `Debug` attendues par des tests (`auth.rs` autour des lignes 1334-1336) suivent.

**Interfaces:**
- Produces :
  `CorpusError::TooLarge { detail: String }` — `#[error("response too large")]`, jamais réessayé. Rendu **seulement** pour un corps de `/search` ou de `/consult/*` au-dessus de la borne ; le corps de l'endpoint token reste un `Decode` (un jeton énorme n'est pas « un texte trop gros pour `get_act` »). Remplace le `Decode` à préfixe `response too large` de `act_too_large`.
  `CorpusError::Auth { origin: AuthOrigin, status: Option<u16>, detail: String }` avec `pub enum AuthOrigin { Token, Api }` (`Debug, Clone, Copy, PartialEq, Eq`) : `status` est le statut réellement reçu (400, 401 ou 403 de l'endpoint token ; 401 après un second 401 de l'API ; 403 de l'API), `None` si inconnu.
  `CorpusError::Config { kind: ConfigKind, detail: String }` avec `pub enum ConfigKind { CredentialsMissing, OAuthResponse, ApiEndpoint, OAuthEndpoint, Settings }` (`Debug, Clone, Copy, PartialEq, Eq`) : `CredentialsMissing` (identifiants absents, aux deux sites de `corpus.rs` et `auth.rs`) ; `OAuthResponse` (OAuth 200 sans `access_token`, jeton inutilisable, JSON OAuth illisible : un seul message pour le modèle) ; `ApiEndpoint` (404/405 de routage sur l'API) ; `OAuthEndpoint` (404 de l'endpoint token) ; `Settings` (valeur de configuration invalide, dont `LF_PISTE_ENV` invalide et `EnvMismatch`, client HTTP non constructible : `detail` est le `Display` de `ConfigProblem`).
  **Alignement des textes `Settings`** : l'implémenteur lit les `Display` de `ConfigProblem` (`config.rs`) et les aligne sur 05 §7.2 (`LF_PISTE_ENV='prod' is invalid: expected sandbox or production.` ; `LF_PISTE_BASE_URL points to the PISTE production host but LF_PISTE_ENV is sandbox: set LF_PISTE_ENV=production, or fix the URL.`), avec un test par texte exact ; un `Display` qui contient une URL ou un secret est un défaut à corriger ici.
- Supprime : `TOO_LARGE_PREFIX` comme critère de classification, `act_too_large` et ses tests de préfixe/suffixe.

- [ ] **Step 1: tests d'abord, dans l'adapter** — conserver chaque test existant en changeant l'attendu : `lawdecree_too_large_is_typed` (corps > 16 Mio sur `/consult/lawDecree` ⇒ `TooLarge`), `non_lawdecree_too_large_is_typed_too` (`/consult/juri`, `/search`), `a_too_large_token_response_keeps_the_generic_decode_error` (le cas `/oauth/token` de `corpus/port.rs` : `Decode`, jamais `TooLarge`), `second_401_is_auth_api_with_status_401`, `api_403_json_is_auth_api_with_status_403`, `oauth_400_is_auth_token_with_status_400`, `oauth_403_is_auth_token_with_status_403`, `oauth_404_is_config_oauth_endpoint`, `api_404_routing_is_config_api_endpoint`, `missing_credentials_is_config_credentials_missing` (aux deux sites), `oauth_without_access_token_is_config_oauth_response`, `oauth_invalid_json_is_config_oauth_response` (le `detail` ne contient plus l'URL dans le message vu par le modèle : il reste un détail de journal), `env_invalid_and_env_mismatch_display_match_05_7_2`. Côté port : `messages_carry_no_secret_and_name_the_failure` étendu aux nouvelles formes.
- [ ] **Step 2: rouge** (échec de compilation puis d'assertions) ; **Step 3: implémenter** — types dans `corpus.rs` du port ; l'adapter renseigne `origin`/`status`/`kind` là où il construit ces variantes (liste : `grep -rn "CorpusError::\(Auth\|Config\)" crates/lf-adapter-piste/src crates/lf-adapter-piste/tests`, une quinzaine de sites de production dont `classify.rs:149,161,196`, `corpus.rs:374,376,419`, `auth.rs:233,334-347` ; le recensement exact fait foi, pas ces numéros) ; `TooLarge` sort de `too_large()` de `http.rs` pour les endpoints non token, plus de comparaison de texte. **Step 4: vert** — `cargo nextest run --workspace --all-features -j 2 --locked` ; `make cov` ; `make arch`. **Step 5: commit** — `refactor(application,adapter): CorpusError précise TooLarge, l'origine d'Auth et la nature de Config (plan 3a, tâche 3)`.

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
  - Erreur réseau : la catégorie reste fixe `connection failed` (Ruling : `CorpusError::Upstream` ne porte pas la catégorie ; 05 §7.2 cite `TLS error` en exemple, à ajouter si l'adapter la type un jour).
  - `Timeout` ⇒ `Legifrance/PISTE did not answer in time; retry once, then report it.` (Ruling : `CorpusError::Timeout` ne porte pas de durée ; le gabarit de 05 §7.2 avec `{n} s` n'est pas rédigeable sans la changer), cause `Timeout`.
  - `InvalidRequest { status: Some(s), detail }` ⇒ `Legifrance rejected the request (HTTP {s}): {detail}` ; `status: None` ⇒ `the request was not sent to Legifrance ({detail}); report it.` ; `detail` coupé à 200 caractères ; cause `Status(s)` ou `Decode` pour `None`.
  - `Decode` ⇒ `unexpected response from Legifrance; report it.` ; `TooLarge`, outil `fr_get_act` (jamais pour un jeton, qui est un `Decode`) ⇒ `this text is too large for get_act; use get_article or search instead.` ; `TooLarge`, autre outil ⇒ le message de `Decode`. Cause `Decode` dans les deux cas.
  - `Auth { origin, status }` ⇒ `Config` : `origin = Api` et `status = Some(403)` ⇒ `PISTE denied access (HTTP 403): check that the PISTE application is subscribed to the Legifrance API.` ; tout autre cas (jeton 400/401/403, API 401) ⇒ `PISTE rejected the credentials (HTTP {s}): check LF_PISTE_CLIENT_ID, LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV.`, sans `(HTTP {s})` quand `status` est `None`.
  - `Config { kind }` : `CredentialsMissing` ⇒ `PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials).` ; `OAuthResponse` ⇒ `PISTE OAuth returned no access_token.` ; `ApiEndpoint` ⇒ `Legifrance endpoint not found: check LF_PISTE_BASE_URL.` ; `OAuthEndpoint` ⇒ `PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.` ; `Settings` ⇒ `detail` tel quel (c'est le `Display` aligné de la tâche 3), coupé à 500.
  - `deadline` ⇒ `tool deadline of {n} s exceeded; retry, or narrow the request (smaller page_size, fewer fonds).`, cause `Deadline`.
  - `cancelled` ⇒ message `cancelled by client`, cause `Cancelled` (jamais montré au modèle).
  - `audit` ⇒ `audit log unavailable; the call was not recorded and its result is withheld (see server logs).` suivi, si `original` est `Some(c)`, de ` Original error: [{c}].` ; jamais le `detail` de `AuditError`.

- [ ] **Step 1: tests d'abord** — `corpus_error_maps_to_app_error_table` (toutes les variantes de `CorpusError` et de `ConfigKind`/`AuthOrigin` × code et cause de 02 §9.2 lignes 5 à 19), `not_found_messages_match_reference` (6 outils × `what` d'un 404 de ressource, d'un 200 vide et d'un 400 « valider est fausse » : 3 origines, même message ; première phrase de la référence ; CONSTEXT et CETATEXT ⇒ `CONSTIT`/`CETAT`, E-15), `decision_fond_follows_prefix_and_never_panics` (préfixe inconnu, chaîne vide), `error_matrix_s1_to_s17` (les lignes de `annexes/A-parite.md` §2.2 qui relèvent des erreurs de corpus et d'audit : S6 à S15 et S17, avec le **texte exact** attendu pour chacune, point final compris ; S16 est la ligne 12 de `audited.rs` ; S1 à S5 appartiennent à 3b), `retry_hint_only_for_retryable_failures`, `messages_never_echo_corpus_detail` (détail contenant `Bearer abc` et `client_secret=x` pour chaque variante hors `Config::Settings` et `InvalidRequest` : absent du message), `invalid_request_detail_is_clipped_to_200_chars`, `config_settings_detail_is_clipped_to_500_chars`, `audit_message_names_no_path` (`detail` = chemin ⇒ absent), `audit_message_cites_original_code`, `deadline_rounds_up_seconds` (60 s, 59,2 s ⇒ 60), `clip_does_not_split_code_points` (accents et émoji).
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
  `pub struct Audited<A: AuditLog> { /* log, policy, compteur */ }` ; `Audited::new(log: A, policy: AuditPolicy) -> Self` (aucune décision dans `new`) ; `pub fn write_failures(&self) -> u64` (compteur `AtomicU64` en mémoire des échecs d'écriture du mode `BestEffort`, 02 §9.2 ligne 23, jamais exporté hors processus) ; `pub async fn run<O, W, X>(&self, meta: CallMeta, cancel: X, work: W) -> Result<O, AppError> where O: AuditMetric, W: Future<Output = Result<O, AppError>> + Send, X: Future<Output = ()> + Send`.
- Algorithme (02 §7.1, avec l'API livrée de `Deadline`) : `started = tokio::time::Instant::now()` ; `work` enveloppé par `Deadline::after(policy.deadline).scope(work)` ; `tokio::select! { biased; () = cancel => Cancelled, r = tokio::time::timeout(policy.deadline, work) => Done(r.unwrap_or_else(|_| Err(messages::deadline(policy.deadline)))) }` ; `duration_ms = started.elapsed().as_millis()` en `u64` saturé ; `AuditRecord` : succès ⇒ `Ok`, métrique de `O`, pas de code ; échec ⇒ `Error`, métrique 0, `error_code = e.code()` (absent pour l'annulation), `error = clip(e.message(), 500)` ; écriture par `log.record(&record).await` ; mode `Strict` + échec d'écriture ⇒ `messages::audit(&error, original_code)` en écartant le résultat ; mode `BestEffort` ⇒ `tracing::warn!(kind = ?kind, "audit write failed")`, compteur incrémenté, résultat d'origine ; mode `Strict` ⇒ en plus `tracing::error!(kind = ?kind, "audit write failed; result withheld")` ; annulation ⇒ ligne écrite (une tentative, échec ignoré avec `warn!`), retour `messages::cancelled()`. Span `tool_call` avec `tool`, `input_bytes` (longueur de `CanonicalInput::as_str()`, 02 §11.1), `status`, `error_code`, `duration_ms`, `output` renseignés après coup ; `invalid_arg` et `not_found` journalisés en `info`, les autres erreurs en `warn` (D-08).

- [ ] **Step 1: tests d'abord, dans `audited.rs`** (table pilotée `audit_written_for_every_outcome`, une entrée par ligne de 06 §2.2 ; les lignes 2 et 3 sont rendues par un `work` qui échoue en `invalid_argument` sans appeler le port — le chemin par `LegalService` est retesté en 3b) : les 15 issues, avec `lines`, `status`, `error_code`, `output_count_or_size`, `duration_ms >= min`, code retourné et nombre d'appels au port ; puis `error_duration_is_nonzero` (échec après 200 ms virtuelles), `duration_of_instant_failure_is_zero`, `deadline_maps_to_upstream_error_before_deadline_plus_epsilon` (porte jamais résolue, échéance 1 s : retour entre 1 000 et 1 010 ms virtuelles, cause `Deadline`), `deadline_is_published_to_the_port` (`FakeCorpus::seen_deadlines` : `LF_TOOL_DEADLINE_S` moins le temps écoulé, y compris au second appel ; hors `run` ⇒ le défaut), `cancelled_call_is_audited` (une ligne `error`, `error_code` absent, `error = "cancelled by client"`, durée ≈ 100 ms), `cancel_wins_over_deadline_at_same_instant`, `audit_failure_returns_audit_error`, `audit_failure_does_not_mask_original_cause`, `best_effort_keeps_result` (assertion aussi sur `write_failures() == 1` ; `write_failures() == 0` après un succès d'écriture), `strict_write_failure_logs_an_error_event` (subscriber de capture), `audit_write_is_acknowledged_before_tool_returns` (`MemoryAuditLog::with_gate` : `run` reste pendant tant que la porte est fermée), `audit_metric_is_taken_from_the_output` (valeur renvoyée par l'implémentation de test d'`AuditMetric`), `audit_error_field_is_capped_at_500_chars`, `invalid_arg_is_not_logged_as_error` (subscriber de capture construit avec `tracing-subscriber` en dev : `INFO`, pas `ERROR` ni `WARN`), `record_carries_env_version_and_timestamp` (champs de `CallMeta` et de la politique recopiés).
- [ ] **Step 2: rouge** ; **Step 3: implémenter** ; **Step 4: vert** + `make cov` sur `audited.rs` ; **Step 5: commit** — `feat(application): Audited journalise chaque issue sous une échéance dure (plan 3a, tâche 6)`.

---

### Task 7: Arborescence, documents et lints

**Files:** `docs/plan/02-architecture.md` (§2.1 et §3.2 : `tools.rs`, `AuditMetric` dans `audited.rs` ; §5.1 : bloc `CorpusError` précisé (`TooLarge`, `AuthOrigin`, `ConfigKind`) ; §5.2, §5.3, §9.1 : « non compilé » ⇒ « livré, plan 3a » ; §9.2 lignes 5 à 12 et 19 ; §11.1 : `input_bytes`), `docs/plan/04-adapter-piste.md` (§7.3 : `TooLarge` typé, plus d'appariement de texte), `docs/plan/05-outils-mcp.md` (§7.2 : lignes `Upstream (Decode)`, `Auth`, `Config`, `Timeout` ; les Ruling de la tâche 4), `docs/plan/07-feuille-de-route.md` (T-M3-01, T-M3-02 et T-M3-03 ⇒ « livrées en partie (plan 3a) » avec la liste de ce qui reste pour 3b et 3c), `CHANGELOG.md`, `docs/plan/09-risques-et-questions.md` (D-03, D-08, D-14, D-16 : liens vers les tests ; R-20 et R-21 : état).

- [ ] **Step 1:** pour chaque modification, vérifier d'abord le texte actuel (`grep`), ne changer que ce que la livraison rend faux, garder les étiquettes `[V-xx · tag]`.
- [ ] **Step 2: `make fmt-check lint`, `make test`, `make doc`, `make arch`, `make -k ci`** (seul `deny` peut rester rouge si une alerte neuve tombe hors plan ; le dire).
- [ ] **Step 3: commit** — `docs: socle de l'application livré, TooLarge typé (plan 3a, tâche 7)`.

---

### Task 8: Mutation, couverture et revue de fin

- [ ] **Step 1:** `make mem-check`, puis `make mutants FILE=crates/lf-application/src/<f>.rs` pour `error.rs`, `messages.rs`, `audited.rs`, `ports/audit.rs`, `input.rs`, `tools.rs`, `testing.rs`, un fichier à la fois ; côté adapter, `make mutants FILE=…` pour chaque fichier modifié par la tâche 3 (`auth.rs`, `classify.rs`, `corpus.rs`, `corpus/port.rs`, `corpus/raw.rs`, `http.rs`, `retry.rs` si touché), un à la fois ; la CI refera `mutants-diff` (plafond 60 min). Cible : 0 `MISSED`, 0 `TIMEOUT`. Les mutants de `new` et de `Default` ne sont pas mutés : aucune décision n'y vit.
- [ ] **Step 2:** `make cov` à 100 % ; `cargo +1.88 check --workspace --all-targets --locked`.
- [ ] **Step 3:** revue Rust (`code-reviewer`) et audit de sécurité (`security-auditor` : le crate écrit le journal et rédige les messages vus par le modèle) ; un tour de correction ; commit des corrections.

---

## Hors plan, à reporter dans 3b et 3c

- **3b** : cas d'usage et `ResponseBudget` minimal ; `LegalService` ; `error_matrix_s1_to_s17` pour S1 à S5 (S16 : recherche vide, ligne `ok`/0, dès 3a et rejouée par la façade) ; `audit_written_for_every_outcome` rejoué par la façade (lignes 2 et 3 par la validation réelle) ; `audit_metric_per_tool`, `duration_includes_normalization`, `input_hash_of_invalid_arguments_is_computed`, `invalid_arg_is_not_logged_as_error` par la façade ; suite de contrat `contract::corpus_contract` sur `FakeCorpus` et `PisteCorpus` ; T-M3-10 (`Ord` sur les identifiants, forme de `Coverage`, features de `tokio` dans `check-arch`, `disallowed-methods`).
- **3c** : `JsonlAuditLog` (V-09, V-164 à V-167), `CachedCorpus` (V-08), `SystemClock` (V-17), valeur par défaut `Strict` posée par `lf-app`, T-M3-09 (Q9, 09 §3.16).
