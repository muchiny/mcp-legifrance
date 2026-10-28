# M2h `record-fixture`, tests live et sondes sandbox — plan d'implémentation (T-M2-11, T-M2-12, T-M2-14 à T-M2-22)

> **Origine.** Plan 2h, écrit le 2026-10-28 sur `plan/m2h-outils-sondes` (worktree `/home/muchini/mcp-ligitfrance-m2h`), base `main` @ `f549680` (plan 2g fusionné, PR #18). Il est écrit **depuis un essai à blanc** : l'outillage a été implémenté dans le worktree jetable `/var/tmp/m2h-dry` (branche `dryrun/m2h`, même base), en huit commits `dry: …` (`6f8b8b0` à `1b94045`), vérifiés par `cargo clippy -p lf-adapter-piste --all-targets --all-features -D warnings` et `cargo nextest run -p lf-adapter-piste --lib --all-features -j 2`, puis `make cov` et `make -k ci`. La campagne de sondes a ensuite tourné **en sandbox**, avec l'outil de l'essai : 173 appels (159 à l'API, 14 jetons), une requête par seconde au plus, aucun 429. Les enregistrements sont restés dans `/var/tmp/m2h-dry` (jamais versionnés) ; aucun identifiant, secret ni jeton n'apparaît dans ce plan. **Le test Rust reste l'oracle** ; une réponse de PISTE différente de celle notée ici est consignée, jamais forcée.
>
> **Préalables (hors plan).** (1) La branche `docs/identifiants-piste` (`12682b9`, recette PISTE et premiers constats du 2026-10-28) touche 09 §2.2, 09 §2.20 et `CONTRIBUTING.md` : elle est fusionnée (ou rebasée sur `main`) **avant** la PR 2 de ce plan, qui écrit dans les mêmes sections. (2) `deny` est rouge sur `main` (RUSTSEC-2026-0386, `tokio` 1.53.2) : montée 1.53.3 par une PR `security-fix` séparée, hors plan. (3) L'environnement GitHub `piste-sandbox` porte `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` (propriétaire), sinon `live.yml` saute tous ses tests. `$WT` désigne le worktree d'exécution ; ledger : `$WT/.superpowers/sdd/2026-10-28-m2h-outils-et-sondes/progress.md` (git-ignoré).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** livrer l'outil d'enregistrement de fixtures (`cargo xtask record-fixture`, feature `record`), la suite live sandbox (`tests/live.rs`, `live.yml`) et les deux reliquats de 2g (I-5, intégrité des fixtures enregistrées), puis mener la campagne de sondes de M2 (T-M2-14 à T-M2-22) et appliquer ce qu'elle impose à l'adapter.

**Architecture :** la logique d'enregistrement vit **dans la bibliothèque** de l'adapter, derrière la feature `record` (`src/record.rs` : contrôle des sondes, chemins, caviardage en place, sidecar, écriture), pour être testée et couverte par `make cov` (`--lib`). L'API brute `PisteCorpus::exchange` (`src/corpus/raw.rs`) passe par le vrai chemin d'une tentative (client, barrière, jeton, délai, borne de corps) sans classer, réessayer ni réparer. L'exemple `examples/record_fixture.rs` n'est que de la colle d'E/S ; `xtask` le lance en boîte noire (00 §2). Les tests live sont des tests d'intégration `#[ignore]` ; ceux qui lisent les octets bruts exigent `record`.

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain 1.99.0) ; aucune dépendance nouvelle (`chrono`, `chrono-tz`, `serde`, `serde_json`, `secrecy`, `thiserror` déjà normales ; `toml`, `tokio` `macros`, `wiremock` déjà en dev) ; `cargo nextest` 0.9.148 (profil `live`), `cargo llvm-cov`, `cargo-mutants` 27.1.0 ; Python 3 (`python3 -I`) pour l'analyse des enregistrements.

**Spec :** `docs/plan/07-feuille-de-route.md` T-M2-11, T-M2-12, T-M2-14 à T-M2-22 (T-M2-13 est en M4 ; T-M2-23 clôt M2) ; `docs/plan/04-adapter-piste.md` §1.4, §6.3 à §6.8, §7.3, §10.2, §10.7, §11, §13.4, §13.5 ; `docs/plan/00-decisions.md` §2, §8, §10, §11.1 ; `docs/plan/02-architecture.md` §3.8 ; `docs/plan/06-qualite.md` §5, §7.5 ; `docs/plan/09-risques-et-questions.md` §2.2, §2.18, §2.20, §4 ; reliquats de 2g (I-5 ; détecteur de secrets des fixtures).

---

## Global Constraints

- Rust edition 2024, `rust-version = "1.88"` : `cargo +1.88 check --workspace --all-targets --all-features --locked` vert en fin de chaque PR ; toolchain `1.99.0`.
- Lints : `clippy::pedantic`, `unwrap_used`, `expect_used`, `panic` sous `-D warnings` (`make lint` = `--all-targets --all-features`) ; `unwrap`/`expect` permis dans le code `cfg(test)` seulement (`clippy.toml`) ; aucun `allow` nouveau, aucun `assert!(matches!(..))` (une branche de `matches!` non prise est une région non couverte). Docs et commentaires en français, identifiants et messages d'erreur en anglais, sans point final.
- Couverture : `make cov` = 100 % des lignes, régions et fonctions, **`--all-features` compris** (D2).
- Architecture : `cargo xtask check-arch` vert. `xtask` ne dépend d'aucun crate `lf-*` (R5) ; la feature `record` n'ajoute aucune dépendance (R6 ne bouge pas) ; un `[[example]]` n'est pas une cible `bin`/`lib` pour `check-arch`. `lf-domain` n'est modifié que par la tâche conditionnelle C3.
- Dépendances : aucune nouvelle. Toute dépendance qu'une tâche conditionnelle voudrait ajouter passe par context7, `make min-age` (`--precise`), `deny.toml` et une justification au ledger.
- Identifiants PISTE : `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` dans l'environnement seulement, jamais en argument, dans un fichier du dépôt, un journal, un rapport ou une fixture ; en local, chargés dans **le même appel shell** que la commande (`set -a; . ~/.config/mcp-legifrance/piste.env; set +a; …`) ; jamais `cat`, `echo`, `env`, `printenv`, `set -x`, `curl -v`. Sandbox seulement (Q3) ; une requête par seconde au plus, séquentiel, aucun 429 provoqué.
- Avant tout commit d'enregistrement : `python3 -I .superpowers/sdd/…/secret_count.py <répertoires>` (Task 10) rend **0** pour le Client ID, le secret, `Bearer ` et `access_token` ; chaque fichier versionné est relu (V-178).
- Aucune décision dans une `fn new` ni dans un `impl Default`. Tests qui lisent une source ou un sidecar : tolérants au CRLF (`replace("\r\n", "\n")`).
- WSL : jamais `cargo test -j 4` ni plus (`cargo nextest run -j 2`) ; temporaires sous `/var/tmp` ; **aucune mutation avant les Tasks 9 et 20**, puis `make mutants FILE=<fichier>` un fichier à la fois, après `make mem-check`.
- `make -k ci` à chaque tâche : seul `deny` peut rester rouge, pour RUSTSEC-2026-0386 seulement.
- Commits : un par tâche, message conventionnel en français, sans trailer ajouté par l'implémenteur (le contrôleur ajoute `Co-Authored-By`) ; rien n'est poussé ; le propriétaire ouvre et fusionne les PR.

## Review Focus

1. **Un identifiant qui finit dans une fixture** : une réponse qui recopie le jeton, le secret ou le Client ID (l'essai a observé qu'une route inconnue répond 403 **en recopiant le corps de la requête**) n'est jamais écrite. Épinglé par `a_body_that_echoes_a_credential_is_flagged` (Task 4) et `write_refuses_an_echo_an_error_status_and_a_bad_probe_without_writing` (Task 2), puis par le comptage par valeur de la Task 10.
2. **Écriture hors de `tests/fixtures/recorded/`** par `--name` ou `--endpoint` (`../x`, `/abs`, `a/./b`, segment vide, caractère hors `[A-Za-z0-9_.-]`). Épinglé par `check_name_refuses_traversal_and_odd_characters` et `check_endpoint_accepts_api_paths_and_refuses_the_rest` (Task 2).
3. **Fixture versionnée écrasée en silence** : sans `--force`, rien n'est écrit si le `.json` **ou** le `.meta.toml` existe. Épinglé par `write_records_the_bytes_and_the_sidecar_and_refuses_to_overwrite` (Task 2).
4. **Caviardage qui touche d'autres octets** (clé en double, échappement `~0`/`~1`, valeur non chaîne) : le reste du fichier reste octet pour octet celui reçu, `sha256_raw` garde l'empreinte d'origine. Épinglé par `redact_replaces_the_value_in_place_and_keeps_every_other_byte`, `redact_reaches_arrays_escaped_keys_and_any_value` et `redact_refuses_a_value_it_cannot_locate_in_the_bytes` (Task 2).
5. **Suite live lancée en production ou en rafale** : `LF_PISTE_ENV` posé à autre chose que `sandbox` fait sauter chaque test ; un corpus par test, limiteur 1/s, pause d'une seconde, profil nextest à un fil. Épinglé par l'étape 3 de la Task 7 (exécution sans identifiants, puis avec `LF_PISTE_ENV=production` : tout saute, aucun appel).

---

## Décisions de cadrage

**[P]** = engage le propriétaire ; les autres lignes sont des rulings du plan, annulables.

| # | Décision | Source | Si elle est fausse |
|---|---|---|---|
| D1 **[P]** | La logique d'enregistrement vit dans `src/record.rs` (`pub mod record`, `#[cfg(feature = "record")]`), testée en unitaire et couverte par `make cov` ; l'exemple n'est que de la colle (arguments, réseau, stdout), exclu de la mutation. La feuille de route nommait `xtask/src/record_fixture.rs` et `tests/record_fixture.rs` : un test d'intégration n'est pas mesuré par `make cov` (`--lib`) | 04 §13.4, 07 T-M2-11 | écrivain dans l'exemple, testé par `tests/record_fixture.rs` hors couverture |
| D2 **[P]** | `make cov` passe en `--all-features` (`COV_FLAGS`) et `.cargo/mutants.toml` gagne `all_features = true` : `record` est mesuré et muté comme le reste ; `exclude_globs` gagne `crates/lf-adapter-piste/examples/**` | 06 §7.1 | cible `cov-record` séparée et job CI de plus |
| D3 | `PisteCorpus::exchange(&self, endpoint: &str, body: &[u8]) -> Result<Exchange, CorpusError>` (`src/corpus/raw.rs`, `#[cfg(feature = "record")] mod raw;`) : `ready` → `api_url` → client → barrière → jeton → `send_once` ; aucun classement (un statut n'est jamais une erreur), aucun réessai, aucun renouvellement sur 401, aucune réparation ; `echoes_credentials` compare **par valeur** le corps au jeton, au secret et au Client ID | 04 §1.4, §13.4 | — |
| D4 **[P]** | SHA-256 déplacé de `integrity.rs` vers `src/sha256.rs` (`#[cfg(any(test, feature = "record"))]`), partagé par l'intégrité et l'écrivain, sans `sha2` (D9 du plan 2g). Coût mesuré : 63 mutants de code déplacé dans `mutants-diff` | plan 2g D9 | `sha2` en dépendance optionnelle de `record` |
| D5 **[P]** | Sidecar : clés de 04 §13.4 dans cet ordre, **plus `elapsed_ms`** (durée mesurée, utile à V-96) ; `content_type` et `content_encoding` omises si l'en-tête manque ; étiquette `"<redacted: N bytes base64>"` pour `/acco/data` (2xx de `/consult/acco` seulement), `"<redacted: N bytes>"` pour un pointeur de `--redact` (N = octets de la chaîne, ou du texte JSON d'une valeur non chaîne) | 04 §13.4 | `elapsed_ms` retiré, durée seulement sur stdout |
| D6 **[P]** | Réglages de l'exemple : 1 requête/s, rafale 1, une requête en vol, borne de corps **64 Mio** (pour mesurer les codes, V-75, V-96), délai de requête 120 s, échéance d'appel 180 s ; User-Agent `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-legifrance) record-fixture` | 04 §13.4, D4 du plan 2f (hors plan) | borne de 16 Mio : le Code du travail (31,4 Mio) n'est plus mesurable |
| D7 | `cargo xtask record-fixture <args>` : `xtask/src/record.rs::example_command(&[String]) -> Vec<String>` (pur, testé) et la colle dans `main.rs` (déjà exclue de la mutation) | 00 §2, 02 §3.8 | — |
| D8 **[P]** | `tests/live.rs` est enveloppé dans `#[cfg(test)] mod live { … }` (les aides hors `#[test]` suivent ainsi les règles de lint des tests, sans `allow`) ; le filtre du profil nextest `live` devient `test(/(^\|::)live_/)` ; `live.yml` lance `cargo nextest run -p lf-adapter-piste --test live --all-features --locked --profile live --run-ignored only --no-fail-fast` (06 §7.5 disait `--workspace`, sans features) | 04 §13.5, 06 §7.5 | aides sans `unwrap` et `Result` partout |
| D9 | I-5 : `Reply` porte aussi le `TokenLease` ; `post_json` passe toute `CorpusError::Decode` rendue par le lecteur dans `without_secrets`, qui retire **par valeur** le jeton et le secret client (`error_body::redact`, devenu `pub(crate)`) | reliquat I-5 de 2g | — |
| D10 **[P]** | Intégrité des fixtures enregistrées (`integrity.rs`, `cfg(test)`, sans feature) : mêmes détecteurs que les fixtures de référence, contrôle du sidecar, nombre épinglé `RECORDED` ; le détecteur base64 exempte les feuilles de clé **`path`** et **`url`** (15 002 faux positifs mesurés sur 108 enregistrements, Task 6) | D-35, audit 2g | exemptions par pointeur, une par fixture |
| D11 **[P]** | Fixtures versionnées : les enregistrements de **1 Mio au plus** (106 sur 108 à l'essai, 4,6 Mio au total) ; les deux codes (6,2 et 31,4 Mio) ne sont pas versionnés, leurs mesures vont dans 09 §2.20 | 04 §13.4 (« dérivée réduite ») | dérivée réduite de chaque code (`--redact` des `content`) |
| D12 **[P]** | **Deux PR basées sur `main`, jamais empilées** : PR 1 = outillage (Tasks 1 à 9) ; PR 2 = campagne et suites (Tasks 10 à 20), branche tirée de `main` **après** la fusion de la PR 1 et de `docs/identifiants-piste` | leçon M2d/M2f | — |

### Écarts de l'essai (spec contre réalité, mesurés)

| # | Constat | Conséquence |
|---|---|---|
| E1 | `reqwest` 0.13 avec `gzip` décompresse et **retire** `Content-Encoding` et `Content-Length` (vérifié avec context7) : aucune des 108 réponses n'expose l'en-tête | `live_gzip_negotiated` (V-70) est impossible par le vrai client : décision 3 |
| E2 | `expires_in` et `scope` ne sortent ni du port ni d'`exchange` | `live_token_reuse_and_expires_in` (V-60) non écrit ; V-60 vient de la recette manuelle (200, `expires_in` 3600, `openid resource.READ`) : décision 4 |
| E3 | Le filtre nextest `^live_` ne voit pas `live::live_x` | D8 |
| E4 | Les aides d'un test d'intégration hors `#[test]` déclenchent `unwrap_used` | D8 |
| E5 | `make doc` : lien vers un élément privé (`span`) et `[`write`]` ambigu (fonction et macro) | liens en code simple, `[`write()`]` |
| E6 | Branches du scanner de caviardage inatteignables sur un JSON valide (`?` de `string_end`, `value_end`, `child`) ; branche `else` de `json.parent()` morte | témoins directs dans `the_scanner_stops_on_truncated_or_foreign_input` ; `json.parent().unwrap_or(root)` |
| E7 | `mojibake_detected` vaut `true` pour 6 enregistrements sur 108, **tous faux positifs** : lettre accentuée + NBSP (4 fichiers) et contrôles C1 présents dans la source (2 fichiers, 6 occurrences) | V-72 et R-05 : constat consigné (Task 14) ; aucun changement du détecteur dans ce plan |
| E8 | Une route inconnue (`/consult/inexistant`) répond **403** avec le corps de la requête recopié (JSON), la même signature qu'une application non souscrite ; un mot unique de 100 caractères répond 403 **HTML** (`text/html; charset=iso-8859-1`, pare-feu de la passerelle) | tâche conditionnelle C4 |
| E9 | `cargo mutants --list --in-diff` sur le diff de l'essai : **204 mutants** (`record.rs` 125, `sha256.rs` 63 déplacés, `corpus.rs` 6, `corpus/raw.rs` 5, `xtask/src/record.rs` 3, `http.rs` 2) ; `corpus/raw.rs` muté en 73 s (4 caught, 1 unviable, base de 44 s) | PR 1 seule sous le plafond de 60 min (D12) ; la Task 9 refait la mesure |
| E10 | Flake préexistant, hors 2h : `retry::tests::network_error_is_retried` et `the_429_retry_after_http_date_without_date_header_uses_the_backoff` échouent ensemble une fois sur trois passes de `make cov` (le port fermé de `ClosedPort` est repris par un serveur wiremock d'un test voisin) ; vert à la passe suivante | signalé au ledger s'il se reproduit ; correction hors plan (plan 2b, avec le flake `auth.rs`) |

### Mesures de l'essai

- Tests : `make test` 1 391 tests verts (9 live ignorés) ; `lf-adapter-piste --lib --all-features` 918 tests (908 avant les témoins de couverture).
- `make cov` (`--all-features`) : **100 %** lignes (46 029), régions (25 637), fonctions (3 631).
- `make -k ci` : tout vert sauf `deny` (RUSTSEC-2026-0386, connu) — `make doc` vert après E5 ; une passe de `cov` rouge par le flake E10, verte à la suivante.
- Sandbox : 173 appels (159 API, 14 jetons) ; `lawDecree` Code civil 2, Code du travail 2 ; 0 429, 0 401 ; comptage par valeur des identifiants sur 232 fichiers : 0.

### Correspondance des tâches et des commits de l'essai

| Task | Commit `dry:` |
|---|---|
| 1, 2 | `6f8b8b0` (feature, `sha256.rs`, en-têtes, `record.rs`) ; `8211091` (cov et mutants `--all-features`, témoins de couverture) ; `1b94045` (liens de doc) |
| 3 | `2b35827` |
| 4 | `de640c1` (+ témoins de `8211091`) |
| 5 | `3249607` ; `plan.toml` dans `314e0eb` |
| 6 | `e3e4cbf` (exemption par clé : **non faite** à l'essai, mesurée seulement) |
| 7 | `314e0eb` |
| 10 à 19 | campagne de l'essai (logs `target/probes/run1` à `run7`), aucun commit |

## Structure des fichiers

| Fichier | Rôle | Task |
|---|---|---|
| `crates/lf-adapter-piste/Cargo.toml` | `[features] record = []` ; `[[example]] record_fixture`, `required-features = ["record"]` | 1, 5 |
| `src/sha256.rs` (créé) | SHA-256 FIPS 180-4 et `hex`, sous `cfg(any(test, feature = "record"))` | 1 |
| `src/http.rs` | `RawResponse.content_type`, `content_encoding` | 1 |
| `src/record.rs` (créé) | `Probe`, `Plan`, `Exchange`, `RecordError`, `Written`, contrôles, chemins, caviardage, sidecar, `write` | 2 |
| `src/corpus.rs`, `src/error_body.rs` | I-5 (`Reply.lease`, `without_secrets`) | 3 |
| `src/corpus/raw.rs` (créé) | `PisteCorpus::exchange` | 4 |
| `examples/record_fixture.rs`, `xtask/src/record.rs` (créés), `xtask/src/{lib,main}.rs` | outil | 5 |
| `tests/fixtures/plan.toml` (créé) | lot de sondes par défaut (91 sondes) | 5 |
| `src/integrity.rs` | fixtures enregistrées, exemption par clé | 6 |
| `tests/live.rs`, `.github/workflows/live.yml` (créés), `.config/nextest.toml` | suite live | 7 |
| `Makefile`, `.cargo/mutants.toml` | D2 | 1 |
| `tests/fixtures/recorded/**` (créé, PR 2) | environ 104 enregistrements et sidecars (essai : 104) | 10 |
| `src/classify.rs`, `src/error_body.rs`, `src/memo.rs`, `lf-domain/src/search.rs` | tâches conditionnelles C1 à C5 | 12, 15, 17, 18 |

## Découpage en PR

- **PR 1 `feat/m2h-outillage` → `main`** : Tasks 1 à 9. Aucune requête réelle n'est nécessaire (wiremock) ; la Task 7 se vérifie sans identifiants. 204 mutants dans `mutants-diff` à l'essai (E9).
- **PR 2 `feat/m2h-sondes` → `main`**, tirée de `main` **après** la fusion de la PR 1 et de `docs/identifiants-piste` : Tasks 10 à 20 (enregistrements, consignation, tâches conditionnelles déclenchées, documents). Les mutants n'y viennent que des tâches conditionnelles.

Commande de test commune : `cargo nextest run -p lf-adapter-piste --lib --all-features -j 2 --locked <filtre>` (rouge, puis vert) ; puis `make fmt-check lint`, `make cov`, `make -k ci`.

---

## PR 1 — outillage

### Task 1 : feature `record`, SHA-256 partagé, en-têtes de la réponse brute, cov et mutants `--all-features`

04 §1.4, §13.4 ; D2, D4.

**Files:**
- Create: `crates/lf-adapter-piste/src/sha256.rs`
- Modify: `crates/lf-adapter-piste/Cargo.toml` (`[features]` avant `[dependencies]`, commentaire : « enregistrement de fixtures, jamais activée par le binaire ») ; `src/lib.rs` (`#[cfg(any(test, feature = "record"))] mod sha256;`) ; `src/integrity.rs` (retire `H0`, `K`, `sha256`, `compress`, `hex` et leur test ; `use crate::sha256::{hex, sha256};`) ; `src/golden.rs` (import des tests) ; `src/http.rs` ; `src/classify.rs`, `src/auth.rs` (littéraux de `RawResponse`) ; `Makefile` (`COV_FLAGS` gagne `--all-features`, commentaires l. 8-9 et de `cov`) ; `.cargo/mutants.toml` (`all_features = true` commenté ; `exclude_globs` : `crates/lf-adapter-piste/examples/**`, « glue d'E/S de record-fixture ; la logique est dans src/record.rs »)

**Interfaces:**
- Produces : `crate::sha256::{sha256(&[u8]) -> [u8; 32], hex(&[u8]) -> String}` (`hex` sans `expect` : table de 16 chiffres) ; `RawResponse { …, content_type: Option<String>, content_encoding: Option<String>, … }` (en-tête non ASCII ⇒ `None`, comme `retry_after`).

- [ ] **Step 1 : tests** (rouges). `sha256_matches_the_fips_180_vectors` (déplacé tel quel) ; `hex_writes_two_lowercase_digits_per_byte` : `hex(&[0x00, 0x0f, 0xa5, 0xff]) == "000fa5ff"`. `status_and_two_headers_are_exposed` (http.rs) : la réponse gagne `Content-Encoding: br` et `set_body_raw(.., "text/plain; charset=utf-8")` ⇒ `content_type == Some("text/plain; charset=utf-8")`, `content_encoding == Some("br")`. `debug_of_a_raw_response_shows_no_body` attend `RawResponse { status: 200, retry_after: Some("30"), date: None, content_type: Some("application/json"), content_encoding: None, body_len: 40 }`.
- [ ] **Step 2 : implémenter.** Le `Debug` manuel de `RawResponse` affiche les deux champs. `make fmt-check lint` ; tests verts ; `make cov` à 100 % (le module `sha256` est compilé en test sans feature, et sous `record`).
- [ ] **Step 3 : commit** `feat(adapter): feature record, SHA-256 partagé, Content-Type et Content-Encoding de la réponse brute`.

### Task 2 : `src/record.rs`, l'écrivain de fixtures

04 §13.4 ; D1, D5. Review Focus 1 à 4.

**Files:**
- Create: `crates/lf-adapter-piste/src/record.rs`
- Modify: `src/lib.rs` (`#[cfg(feature = "record")] pub mod record;` après `mod memo;`)

**Interfaces:**
- Consumes : `crate::sha256` ; `lf_domain::text::encoding::has_latin1_mojibake` ; `lf_domain::coverage::PisteEnvironment::name()`.
- Produces (tout `pub`) :

```rust
pub const TOOL: &str = concat!("xtask record-fixture ", env!("CARGO_PKG_VERSION"));
pub const LICENSE: &str = "Licence Ouverte 2.0 — source : Légifrance / DILA";
pub const ACCO_ENDPOINT: &str = "/consult/acco";
pub const ACCO_DATA: &str = "/acco/data";
pub const MAX_VERSIONED_BYTES: usize = 1 << 20;
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)] #[serde(deny_unknown_fields)]
pub struct Probe { pub endpoint: String, pub body: String, pub name: String,
    #[serde(default)] pub allow_error: bool, #[serde(default)] pub redact: Vec<String> }
#[derive(Debug, Deserialize)] #[serde(deny_unknown_fields)]
pub struct Plan { #[serde(default)] pub probe: Vec<Probe> }
pub struct Exchange { pub status: u16, pub content_type: Option<String>, pub content_encoding: Option<String>,
    pub retry_after: Option<String>, pub elapsed: Duration, pub echoes_credentials: bool, pub body: Vec<u8> }
    // Debug manuel : body_len au lieu du corps
pub enum RecordError { Endpoint(String), Name(String), Body(String), Status(u16), Echo, Exists(PathBuf),
    Redact { pointer: String, reason: &'static str }, Io { path: PathBuf, source: io::Error } }
pub struct Written { pub json: PathBuf, pub meta: PathBuf, pub bytes_file: usize, pub redactions: Vec<String> }
pub fn check_endpoint(&str) -> Result<(), RecordError>;   // "/" puis segments [A-Za-z0-9_-]+
pub fn check_name(&str) -> Result<(), RecordError>;       // segments [A-Za-z0-9_.-]+, ni "." ni ".."
impl Probe { pub fn check(&self) -> Result<(), RecordError>; }  // endpoint, nom, corps JSON, pointeurs "/…", dans cet ordre
pub fn fixture_paths(root: &Path, endpoint: &str, name: &str) -> (PathBuf, PathBuf); // root/<endpoint sans "/">/<name>.json et .meta.toml
pub fn mojibake_detected(body: &[u8]) -> bool;            // has_latin1_mojibake(&String::from_utf8_lossy(body))
pub fn sha256_hex(bytes: &[u8]) -> String;
pub fn rfc3339_utc(unix_secs: i64) -> String;             // DateTime::from_timestamp ; "unknown" hors plage (chrono sans `clock`)
pub fn redact(raw: &[u8], pointers: &[String]) -> Result<(Vec<u8>, Vec<String>), RecordError>;
pub fn write(root: &Path, probe: &Probe, exchange: &Exchange, env: PisteEnvironment,
             captured_at: &str, force: bool) -> Result<Written, RecordError>;
```

Messages (`thiserror`) : `endpoint must be a path of [A-Za-z0-9_-] segments starting with /: {0:?}` ; `fixture name must be /-separated segments of [A-Za-z0-9_.-], without . or ..: {0:?}` ; `request body of {0:?} is not JSON` ; `HTTP {0} not recorded without --allow-error` ; `the response echoes a credential: nothing written` ; `{0} exists: use --force to overwrite` ; `cannot redact {pointer}: {reason}` (raisons `body is not JSON`, `value not found in the raw bytes`) ; `cannot write {path}: {source}`.

`redact` : pour chaque pointeur (dédoublonné, `/acco/data` d'abord sur un 2xx de `/consult/acco`), un pointeur absent est sauté ; la place de la valeur est retrouvée dans les octets par un petit scanner (`span`, `skip_blank`, `string_end`, `value_end`, `child`, `pointer_tokens` qui défait `~1` puis `~0`), **vérifiée** (ce qu'on y relit vaut la valeur du pointeur ; une clé en double échoue), puis remplacée par l'étiquette de D5 ; les autres octets ne bougent pas. `write` refuse, dans cet ordre et sans rien écrire : sonde invalide, écho, statut hors 2xx sans `allow_error`, `.json` ou `.meta.toml` existant sans `force`, échec de caviardage ; puis `create_dir_all(json.parent().unwrap_or(root))`, le `.json`, le `.meta.toml`. Sidecar rendu à la main (chaînes TOML de base : `\`, `"` échappés, contrôles en `\uXXXX`), clés dans l'ordre de 04 §13.4 avec `elapsed_ms` après `content_encoding`.

- [ ] **Step 1 : tests** (rouges ; aides `probe(endpoint, name)`, `exchange(status, body)`, `scratch(test)` = `std::env::temp_dir()/lf-record-<test>-<pid>`, retiré en fin de test) :
  - `check_endpoint_accepts_api_paths_and_refuses_the_rest` (`/consult/juri`, `/search` acceptés ; `""`, `juri`, `/`, `//x`, `/a//b`, `/a/../b`, `/a b`, `/é` refusés) ; `check_name_refuses_traversal_and_odd_characters` (`juri/JURITEXT000051743650`, `a.b-c_d` acceptés ; `..`, `.`, `a/../b`, `/a`, `a/`, `a b`, `a\b` refusés) ; `a_probe_check_reports_the_first_problem` (messages exacts, pointeur sans `/` en dernier).
  - `acco_data_is_redacted_by_default_on_a_successful_acco_reply_only` ; `fixture_paths_nest_by_endpoint` ; `mojibake_is_detected_on_the_raw_bytes` ; `sha256_hex_is_the_fips_digest` (`abc`) ; `rfc3339_utc_formats_seconds_and_marks_out_of_range` (`1793182500` ⇒ `2026-10-28T10:15:00Z` ; `i64::MAX` ⇒ `unknown`).
  - `redact_replaces_the_value_in_place_and_keeps_every_other_byte` ; `redact_reaches_arrays_escaped_keys_and_any_value` : `{"a/b": [ 0 , {"k":"v\"q"} ], "n": 12, "t~": true, "s":"\u00e9t\u00e9"}` avec `/a~1b/1`, `/n`, `/t~0`, `/s` ⇒ `"<redacted: 12 bytes>"`, `"<redacted: 2 bytes>"`, `"<redacted: 4 bytes>"`, `"<redacted: 5 bytes>"` ; `redact_without_pointer_keeps_the_bytes_even_when_they_are_not_json` ; `redact_refuses_a_value_it_cannot_locate_in_the_bytes` (`{"d":"first","d":"second"}`) ; `the_scanner_stops_on_truncated_or_foreign_input` (dont `span(b"{\"a\":1}", ["b"]) == None`, `span(b"[", []) == None`, `value_end(b"[\"ab", 0) == None`, `child(b"{\"\\u12\":1}", 0, "x") == None`, `child(b"{\"a\":[", 0, "x") == None`).
  - `a_sidecar_renders_toml_that_reads_back` (relu par `toml::Table` : 15 clés, `content_encoding = "br"`, `elapsed_ms = 1234`, `redactions = ["/acco/data", "/q\"x"]`).
  - `write_records_the_bytes_and_the_sidecar_and_refuses_to_overwrite` ; `write_refuses_an_echo_an_error_status_and_a_bad_probe_without_writing` ; `write_redacts_acco_data_and_keeps_the_raw_digest` (`bytes_raw = 45`, `sha256_raw` ≠ `sha256_file`) ; `write_reports_the_file_it_cannot_write` (un répertoire à la place du `.json`, puis du `.meta.toml`, avec `force`) ; `write_reports_the_path_it_cannot_create` ; `a_plan_reads_its_probes_and_refuses_unknown_keys` ; `debug_of_an_exchange_shows_no_body`.
  - Aucune assertion `assert!(matches!(..))` : comparer `to_string()` de l'erreur.
- [ ] **Step 2 : implémenter.** Liens de doc : jamais vers un élément privé, `[`write()`]` (E5). `cargo clippy -p lf-adapter-piste --all-targets --all-features --locked -- -D warnings` ; tests verts ; `make cov` 100 % ; `make doc`.
- [ ] **Step 3 : commit** `feat(adapter): module record, sonde, caviardage en place, sidecar et écrivain de fixtures`.

### Task 3 : I-5, jeton et secret retirés par valeur du détail d'un `Decode`

Reliquat I-5 du plan 2g ; D9.

**Files:**
- Modify: `crates/lf-adapter-piste/src/corpus.rs` (`Reply { response, permit, lease: TokenLease }` ; `attempt` garde `lease.generation` avant de déplacer le bail ; `post_json` : `read(&decoded.text).map_err(|e| without_secrets(e, &lease, &inner.config))` ; `fn without_secrets(CorpusError, &TokenLease, &PisteConfig) -> CorpusError`) ; `src/error_body.rs` (`pub(crate) fn redact`)

- [ ] **Step 1 : tests** (rouges) : `a_decode_detail_never_carries_the_token_or_the_secret_as_a_path_key` — pour une clé `tok-1`, `my-client-secret` puis `free-key`, le corps `{"items":[1],"<clé>":[1,}` ⇒ `Decode { detail: "/consult/juri: <montré>: expected value at line 1 column <len>" }` avec `[redacted]`, `[redacted]`, `free-key` ; `without_secrets_leaves_other_errors_alone` (`NotFound` inchangé ; `"x: tok-1: y"` ⇒ `"x: [redacted]: y"`).
- [ ] **Step 2 : implémenter**, commentaire de `without_secrets` : `decode` garde dans le chemin une clé de forme identifiant (`dto::decode::safe_key`), une réponse qui recopierait un secret en clé le ferait sortir. Tests verts ; 908 tests de la lib verts.
- [ ] **Step 3 : commit** `fix(adapter): jeton et secret client retirés par valeur du détail d'une erreur de lecture (I-5)`.

### Task 4 : `PisteCorpus::exchange` (`src/corpus/raw.rs`)

04 §1.4, §13.4 ; D3. Review Focus 1.

**Files:**
- Create: `crates/lf-adapter-piste/src/corpus/raw.rs`
- Modify: `src/corpus.rs` (`#[cfg(feature = "record")] mod raw;` après `mod port;` ; `Setup.user_agent` passe `pub(super)` pour les tests voisins)

**Interfaces:**
- Consumes : `ready`, `api_url`, `CallDeadline::from_task_local`, `SharedClient::get`, `gate.acquire`, `tokens.token`, `http::{Attempt, Payload::Json, send_once}`, `record::Exchange`.
- Produces : `pub async fn exchange(&self, endpoint: &str, body: &[u8]) -> Result<Exchange, CorpusError>` ; `fn echoes(body: &[u8], secrets: &[&str]) -> bool` (valeurs vides ignorées). Durée sur `inner.clock` autour de `send_once` ; permis relâché à la fin de la fonction.

- [ ] **Step 1 : tests** (rouges ; route `/dila/legifrance/lf-engine-app/consult/juri`, corps `{"textId":"JURITEXT000000000000"}`, aides de `corpus::tests` : `Rig`, `token_mock`, `json_reply`, `requests_to`, `rate`, `ms`, `SECRET`, `TOKEN_PATH`) : `echoes_finds_any_non_empty_secret_byte_for_byte` ; `an_error_status_is_returned_raw_without_classification` (400 avec le marqueur et `Retry-After: 7` : statut, octets, `content_type`, `retry_after` tels quels, `elapsed == 0` sous l'horloge de test) ; `a_503_is_not_retried_and_mojibake_is_not_repaired` (une seule requête, octets `R\u{c3}\u{a9}publique` intacts, `stats()` nul) ; `a_body_that_echoes_a_credential_is_flagged` (`tok-1`, secret, `my-client-id` ⇒ vrai ; `tok-2` ⇒ faux) ; `the_degraded_mode_and_a_bad_template_make_no_request` ; `an_unbuildable_client_or_an_exhausted_quota_is_an_error` (User-Agent `bad\nagent` ⇒ `Config` ; quota 1/1 et échéance 500 ms ⇒ second appel `RateLimited { retry_after: Some(1 s) }`) ; `a_token_failure_or_an_oversized_body_is_an_error` (`credentials rejected` ; borne 100 octets ⇒ `Decode { "response too large (> 100 bytes) on /consult/juri" }`).
- [ ] **Step 2 : implémenter.** Tests verts ; `make cov` 100 % (sans les deux derniers tests, `?` du client et de la barrière restent non couverts : mesuré).
- [ ] **Step 3 : commit** `feat(adapter): PisteCorpus::exchange, échange brut par le vrai client (feature record)`.

### Task 5 : exemple `record_fixture`, `cargo xtask record-fixture`, lot `plan.toml`

04 §13.4 ; 00 §2 ; 02 §3.8 ; D6, D7.

**Files:**
- Create: `crates/lf-adapter-piste/examples/record_fixture.rs`, `xtask/src/record.rs`, `crates/lf-adapter-piste/tests/fixtures/plan.toml`
- Modify: `crates/lf-adapter-piste/Cargo.toml` (`[[example]] name = "record_fixture"`, `required-features = ["record"]`, commentaire) ; `xtask/src/lib.rs` (`pub mod record;`) ; `xtask/src/main.rs` (`record_fixture(&args[2..])` : `Command::new($CARGO).args(example_command(args)).status()` ; usage `cargo xtask check-arch | record-fixture <arguments>`)

**Interfaces:**
- Produces : `xtask::record::example_command(args: &[String]) -> Vec<String>` = `run --quiet --locked --package lf-adapter-piste --example record_fixture --features record -- <args…>`.
- Exemple : arguments `--endpoint --body --name [--allow-error] [--redact <p>]… | --plan <toml> [--only <préfixe>]`, plus `[--env sandbox|production] [--force] [--out <dir>]` ; défauts `sandbox` et `crates/lf-adapter-piste/tests/fixtures/recorded` ; toutes les sondes passent `Probe::check` **avant** la lecture des identifiants et tout réseau ; une ligne par sonde sur stdout, `nom\tstatut\tN bytes\tN ms\tmojibake=…\twritten` (ou `NOT WRITTEN: <erreur>`, ou `written (> 1 MiB: version a reduced derivative)`), jamais un identifiant ; code 0, 1 (sonde non écrite) ou 2 (usage, identifiants absents) ; `captured_at` = `rfc3339_utc(SystemTime)`.
- `plan.toml` : 91 sondes, familles `inconnu/` (19), `mojibake/` (6), `complet/` (5), `contenu/` (2), `identifiants/` (8, `pageSize` 100), `lawdecree/` (2), `dates/` (19), `erreurs500/` (6), `modes/` (6), `syntaxe/` (5), `longueur/` (4), `pagination/` (6) ; corps JSON en chaîne TOML ; `allow_error = true` sur les sondes d'erreur. Le fichier de l'essai est la référence (`/var/tmp/m2h-dry/crates/lf-adapter-piste/tests/fixtures/plan.toml`, généré par script ; à recopier tel quel, en ajoutant les sondes `longueur/{100,200,300,400,450,499,100-mots-200,mot-unique-100,mot-unique-500,250-mots-500}` et `dates/v78-bornes-JURI-*` jouées en phases 2 à 4 de l'essai, Tasks 13 et 16).

- [ ] **Step 1 : test** (rouge) `record_fixture_forwards_to_example` : `example_command(["--plan", "plan.toml", "--only", "inconnu/"]).join(" ")` vaut la ligne ci-dessus ; `example_command(&[])` finit par `--`.
- [ ] **Step 2 : implémenter.** `cargo xtask check-arch` : `ok`. Sans identifiants : `env -u LF_PISTE_CLIENT_ID -u LF_PISTE_CLIENT_SECRET cargo xtask record-fixture --plan crates/lf-adapter-piste/tests/fixtures/plan.toml` ⇒ les 91 sondes passent le contrôle, puis `set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET in the environment`, code 2 ; `--bogus` ⇒ usage, code 2.
- [ ] **Step 3 : commit** `feat(xtask): cargo xtask record-fixture et exemple record_fixture, lot de sondes plan.toml`.

### Task 6 : intégrité des fixtures enregistrées

D-35 ; reliquat « détecteur de secrets » de 2g ; D10, D11.

**Files:**
- Modify: `crates/lf-adapter-piste/src/integrity.rs` (doc de module ; tests)

**Interfaces:**
- Produces (dans `tests`) : `const RECORDED: usize = 0;` (la Task 10 le porte au nombre versionné) ; `const RECORDED_KEYS: [&str; 13]` (clés de D5 hors `content_type`/`content_encoding`) ; `fn files_under(&Path) -> Vec<PathBuf>` (vide si absent) ; `fn recorded_problems(root, json) -> Vec<String>` ; `fn recorded_tree(root) -> (Vec<String>, usize)` ; `const BASE64_KEYS: [&str; 2] = ["path", "url"]`.

Contrôles par fixture : sidecar présent et TOML ; clés de `RECORDED_KEYS` ; `sha256_file` recalculé ; sans caviardage, `sha256_raw` = `sha256_file` et `mojibake_detected` = détection sur le fichier ; `env` ∈ {`sandbox`, `production`} ; chemin qui commence par l'endpoint du sidecar ; licence exacte ; 1 Mio au plus ; `leaks(..)`. Arbre : tout fichier est `.json` ou `.meta.toml`, un sidecar orphelin est signalé. **Exemption par clé** (mesurée à l'essai sur 108 enregistrements : 15 001 `base64-like … .path` dans 6 fichiers — chemins `/LEGISCTA…/LEGIARTI…` de la TOC —, 1 `acco.url`, 1 `content` du Code du travail — lien `download/pdf?id=…` de 43 caractères, non versionné par D11 ; aucun UUID, hex, JWT ni mot de secret) : `leaks` ne lance pas `has_base64_run` sur une feuille dont la dernière clé est dans `BASE64_KEYS`, ce qui remplace `BASE64_LOOKALIKES` (deux exemptions par pointeur de 2g).

- [ ] **Step 1 : tests** (rouges) : `recorded_fixtures_pass_integrity` (`recorded_tree(tests/fixtures/recorded) == (vec![], RECORDED)`) ; `recorded_tree_flags_each_problem` (arbre temporaire : une fixture saine ⇒ `(vec![], 1)` ; puis `bad.json` de plus de 1 Mio avec une suite hex de 32, sidecar sans `tool`, `env = "prod"`, endpoint `/consult/kali`, licence altérée ; `lone.meta.toml`, `notes.txt`, `nometa.json`, `broken.json` + sidecar `= x` ⇒ les 12 messages exacts de l'essai, 4 fixtures) ; `base64_lookalikes_are_exempt_by_key_only` (un `path` et un `url` de 60 caractères base64 passent, la même valeur sous `texte` est signalée).
- [ ] **Step 2 : implémenter**, puis vérifier que `fixtures_contain_no_token_like_strings` reste vert sans `BASE64_LOOKALIKES`. `make cov` 100 %.
- [ ] **Step 3 : commit** `test(adapter): intégrité des fixtures enregistrées, exemption base64 par clé (path, url)`.

### Task 7 : `tests/live.rs`, profil nextest et `live.yml`

04 §13.5 ; 06 §5, §7.5 ; T-M2-12 ; D8. Review Focus 5.

**Files:**
- Create: `crates/lf-adapter-piste/tests/live.rs`, `.github/workflows/live.yml`
- Modify: `.config/nextest.toml` (`default-filter = "test(/(^|::)live_/)"`, commentaire)

**Interfaces:**
- Aides (dans `#[cfg(test)] mod live`) : `credentials(test) -> Option<PisteCredentials>` (message `"{test}: skipped, set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (sandbox)"` ; `LF_PISTE_ENV` posé et ≠ `sandbox` ⇒ `"{test}: skipped, live tests run against the sandbox only"`) ; `sandbox(test, max_body_bytes: u64) -> Option<PisteCorpus>` (pause d'1 s, `RateQuota::new(1, 1)`, une requête en vol, délais 120 s / 180 s, User-Agent `mcp-legifrance/<version> live-tests`) ; `const VERSION: &str = "2026-10-28"` ; `SCENARIOS` (les huit recherches de recherche 01 §11.7).
- Tests par le port : `live_reference_scenarios` (8 recherches, `total ≥ 1`, puis `get_act(LEGITEXT000033205014)`, `get_article(LEGIARTI000006419288)`, `get_decision(JURITEXT000051743650)`, `get_deliberation(CNILTEXT000053352643)`, `get_convention(KALITEXT000049878950)`, `get_company_agreement(ACCOTEXT000053643828)` : 14 appels) ; `live_unknown_ids_classification` (les huit consultations sur `…000000000000` ⇒ `NotFound` partout : 8 appels) ; `live_date_facets_reduce_total` (7 fonds, plage 2020 ⇒ total strictement plus petit : 14) ; `live_match_modes_totals` (JURI « contrat de travail » : `any ≥ all ≥ exact` : 3).
- Tests sur octets bruts, `#[cfg(feature = "record")] mod raw` : `live_no_mojibake_on_reference_ids` (3 consultations : 200, `mojibake_detected` faux, `application/json`) ; `live_singledate_noon_paris_semantics` (D lu dans le suffixe le plus récent ≤ `VERSION` des hits CODE « contrat » ; veille à midi, minuit de Paris, midi de Paris ⇒ `[false, false, true]` : 4) ; `live_lawdecree_on_code_size` (borne 64 Mio ; seuils 4 Mio et 10 s : **rouge à l'essai**, voir C2) ; `live_texte_html_presence` (2) ; `live_dates_are_midnight_utc_and_eli_fields_are_null` (1). Non écrits : `live_token_reuse_and_expires_in` (E2), `live_gzip_negotiated` (E1), M5 et M6.

- [ ] **Step 1 : écrire les tests** (tous `#[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]`).
- [ ] **Step 2 : lint.** `cargo clippy -p lf-adapter-piste --all-targets --all-features --locked -- -D warnings`, puis sans `--all-features`.
- [ ] **Step 3 : exécution sans réseau.** `env -u LF_PISTE_CLIENT_ID -u LF_PISTE_CLIENT_SECRET cargo nextest run -p lf-adapter-piste --test live --all-features --locked --profile live --run-ignored only --no-capture` ⇒ `9 tests run: 9 passed`, neuf messages `skipped`. Avec `LF_PISTE_ENV=production` et des identifiants factices (`LF_PISTE_CLIENT_ID=x LF_PISTE_CLIENT_SECRET=y`) ⇒ neuf `skipped … sandbox only`, aucune requête.
- [ ] **Step 4 : `live.yml`** (esquisse de 06 §7.5 avec les actions épinglées de `ci.yml` : `checkout` v7.0.1, `rust-toolchain` 1.99.0, `rust-cache` v2.9.2, `install-action` `cargo-nextest@0.9.148`) : `workflow_dispatch` et `cron: "41 5 * * 1"`, `permissions: contents: read`, `concurrency: live` sans annulation, job unique `sandbox` (`if: github.repository == 'muchiny/mcp-legifrance'`, `environment: piste-sandbox`, `timeout-minutes: 30`, `LF_PISTE_ENV: sandbox`, secrets en `env`), commande de D8. `actionlint` et `zizmor` (job `workflow-lint`) muets — non installés sur le poste de l'essai, à faire passer par la CI.
- [ ] **Step 5 : commit** `test(adapter): tests live sandbox (tests/live.rs) et workflow live.yml`.

### Task 8 : documentation de l'outillage

Aucune ligne de Rust.

**Files:**
- Modify: `docs/plan/04-adapter-piste.md` (§1.4 : feature `record` livrée, aucune dépendance ; §1.5 : `record.rs`, `corpus/raw.rs`, `sha256.rs` ; §13.4 : chemins réels (D1, D7), `elapsed_ms` et étiquettes (D5), borne de 64 Mio (D6), `content_encoding` jamais vu (E1) ; §13.5 : commande nextest, `cfg(test) mod live`, tests écrits et non écrits (E1, E2)) ; `docs/plan/06-qualite.md` (§7.1 `cov --all-features` ; §7.5 commande réelle de `live.yml`) ; `docs/plan/07-feuille-de-route.md` (T-M2-11 et T-M2-12 : « Livrée au plan 2h », Où réels) ; `docs/plan/02-architecture.md` §3.8 ; `docs/plan/00-decisions.md` §11.1 (I-5 résorbé)

- [ ] **Step 1 : rédiger** ; `grep -n "record_fixture.rs\|tests/record_fixture" docs/plan/*.md` ne rend plus que l'historique.
- [ ] **Step 2 : commit** `docs: record-fixture, tests live et I-5 (plan 2h, PR 1)`.

### Task 9 : mutation de la PR 1, revue de branche

- [ ] **Step 1 : préalables.** `make mem-check` ; aucune autre compilation ; `make cov` vert.
- [ ] **Step 2 : mutation, un fichier à la fois** : `make mutants FILE=crates/lf-adapter-piste/src/record.rs` (125 mutants listés à l'essai), puis `src/corpus/raw.rs` (5 ; essai : 4 caught, 1 unviable, 73 s), `src/corpus.rs`, `src/http.rs`, `xtask/src/record.rs` (3). `sha256.rs` est du code déplacé déjà muté en 2g : le muter quand même (63) si le temps le permet, sinon le noter au ledger. Attendu : 0 MISSED, 0 TIMEOUT ; survivant ⇒ test dans la tâche propriétaire, commit `test(adapter): mutants survivants du plan 2h (PR 1)`.
- [ ] **Step 3 : budget `mutants-diff`.** `git diff main...HEAD > /var/tmp/m2h.diff && TMPDIR=/var/tmp cargo mutants --list --in-diff /var/tmp/m2h.diff | wc -l` (essai : 204). Au-delà de 300, sortir `sha256.rs` de la PR 1 (garder la copie de 2g dans `integrity.rs` et en dupliquer l'usage dans `record.rs` derrière `cfg`), décision au ledger.
- [ ] **Step 4 : revue.** `code-reviewer` (Sonnet) puis `security-auditor` (Opus) sur la branche : Review Focus 1 à 5, I-5, aucune valeur d'identifiant dans un message. Une vague de clôture.
- [ ] **Step 5 : clôture.** `make -k ci` (seul `deny`) ; `cargo +1.88 check --workspace --all-targets --all-features --locked` ; `make arch`. Rien n'est poussé : PR 1 remise au propriétaire.

---

## PR 2 — campagne de sondes en sandbox

Chaque tâche de sonde donne : la **commande** (lot ou test live), l'**observable** attendu, la **règle de décision**, la **consignation** — une ligne par V dans 09 §2.20 (`| Date (UTC) | V | Statut | Résultat (une phrase) | Preuve | Environnement | Documents mis à jour |`) et une ligne par exécution dans 06 §5.4 (`| Date (UTC) | Version / commit | env | Tests exécutés | Résultat | Étiquettes levées (V-xx) | Remarques |`). Les constats de l'essai (2026-10-28, sandbox) sont donnés comme **attendu** ; une réponse différente est consignée telle quelle et la règle de décision s'applique à ce qui est observé.

### Task 10 : exécution de la campagne, comptage des identifiants, sélection des fixtures

V-178 ; D11.

**Files:**
- Create: `crates/lf-adapter-piste/tests/fixtures/recorded/**` ; `$WT/.superpowers/sdd/2026-10-28-m2h-outils-et-sondes/secret_count.py` (git-ignoré)
- Modify: `crates/lf-adapter-piste/src/integrity.rs` (`RECORDED`)

- [ ] **Step 1 : lot.** En tête, sans identifiants : `cargo build --locked -p xtask && cargo build --locked -p lf-adapter-piste --example record_fixture --features record` (la compilation ne voit jamais `piste.env`). Puis `set -a; . ~/.config/mcp-legifrance/piste.env; set +a; LF_PISTE_ENV=sandbox cargo xtask record-fixture --plan crates/lf-adapter-piste/tests/fixtures/plan.toml --env sandbox > /var/tmp/m2h-run.log` (un seul processus : un seul jeton). Attendu : environ 100 lignes `written`, durée < 3 min, aucune ligne `error:`.
- [ ] **Step 2 : suite live.** En tête, sans identifiants : `cargo nextest run -p lf-adapter-piste --test live --all-features --locked --profile live --no-run`. Puis, même chargement, `cargo nextest run -p lf-adapter-piste --test live --all-features --locked --profile live --run-ignored only --no-fail-fast --no-capture`. Attendu à l'essai : 7 verts, `live_unknown_ids_classification` rouge (ACCO, C1), `live_lawdecree_on_code_size` rouge (C2).
- [ ] **Step 3 : comptage par valeur.** `secret_count.py` lit `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` dans l'environnement (chargé dans le même appel), compte sans les imprimer leurs occurrences et celles de `Bearer ` et `access_token` dans `tests/fixtures/recorded` et les journaux, n'affiche que des noms et des nombres, sort en 1 si un total est non nul. Attendu : 0 partout (essai : 232 fichiers, 0).
- [ ] **Step 4 : sélection.** Retirer du répertoire les enregistrements de plus de 1 Mio (essai : les deux codes) après en avoir noté taille et durée au ledger ; retirer aussi les réponses non JSON (403 HTML de la passerelle, E8, famille `longueur/`) : le test d'intégrité les refuse (`not JSON`, échec fermé) ; leurs octets et en-têtes vont au ledger, C4 recopie le corps dans son test ; retirer aussi le `.meta.toml` d'un enregistrement retiré (sinon sidecar orphelin) ; relire **chaque** fichier restant (V-178 : données pseudonymisées de JURI, occultées de la CNIL, aucun nom de personne physique en clair hors décisions publiques ; `acco.data` caviardé). `RECORDED` = nombre de `.json` restants (essai : 104) ; `cargo nextest run -p lf-adapter-piste --lib -j 2 integrity` vert.
- [ ] **Step 4 bis : contrôle des caviardages.** Avant la première fixture commitée, consigner au ledger le contrôle : chaque pointeur de `redactions` d'un sidecar se lit `<redacted: …>` dans le fichier correspondant.
- [ ] **Step 5 : commit** `test(adapter): fixtures enregistrées en sandbox (lot du plan 2h), intégrité épinglée`.

### Task 11 : T-M2-14, « introuvable » (V-66, V-67, V-68)

- **Commande** : familles `inconnu/` du lot ; `live_unknown_ids_classification`.
- **Observable (essai)** : `juri` (JURI, CETAT, CONSTIT), `cnil`, `lawDecree` et un identifiant mal formé ⇒ 400 `{"timestamp","error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null,"exceptionDetails":null}` pour `…000000000000` comme pour `…999999999999` ; `getArticle` ⇒ 200 `{"executionTime":0,"dereferenced":false,"article":null}` ; `kaliText` ⇒ 200, enveloppe dont tous les champs sont `null` ou `[]` ; `acco` ⇒ 400 « Le nombre de résultat retourné par le moteur de recherche n'est pas égal à 1. » ; corps `{}` ⇒ 500 ; route inconnue ⇒ 403 qui recopie le corps de la requête ; **aucun 404**. Par le port : `NotFound` partout sauf ACCO (`InvalidRequest`).
- **Règle** : si une consultation rend autre chose que `NotFound` par le port alors tâche conditionnelle C1 pour elle ; si un 404 apparaît alors 04 §5.1 et §11 décrivent sa forme (V-66) et `classify` est réaligné par C1 ; sinon V-66 est consignée « aucun 404 observé, routage = 403 ».
- **Consignation** : 09 §2.20 V-66, V-67, V-68 ; table de 04 §11 définitive (une ligne par endpoint, statut et corps observés) ; 06 §5.4.
- [ ] **Step 1** : relire les 19 enregistrements `inconnu/` ; **Step 2** : écrire les lignes ; **Step 3** : commit avec la Task 12 si C1 n'est pas déclenchée.

### Task 12 (C1, conditionnelle, déclenchée à l'essai) : ACCO introuvable ⇒ `NotFound`

**Seulement si** la Task 11 observe un message ACCO distinct du marqueur.

**Files:** Modify `crates/lf-adapter-piste/src/error_body.rs` (`is_not_found_marker` ou un second marqueur limité à `/consult/acco`, au choix de la revue), `src/classify.rs` (tests).

- [ ] **Step 1 : test** (rouge) `the_acco_count_message_on_a_consultation_is_not_found` : corps **recopié de l'enregistrement** `consult/acco/inconnu/ACCOTEXT000000000000.json` ⇒ `NotFound` ; le même message sur `/search` ⇒ pas `NotFound` (R7).
- [ ] **Step 2 : implémenter** ; `live_unknown_ids_classification` vert en sandbox (8 appels).
- [ ] **Step 3 : commit** `fix(adapter): un identifiant ACCO inexistant est introuvable (message de comptage, V-68)`.

### Task 13 : T-M2-15, dates (V-76, V-77, V-78, V-89, V-100, V-101)

- **Commande** : `live_singledate_noon_paris_semantics`, `live_date_facets_reduce_total`, `live_dates_are_midnight_utc_and_eli_fields_are_null` ; familles `dates/` ; bornes : `dates/v78-bornes-JURI-{2025-06-04, 2025-06-04-05, 2025-06-05}` (« responsabilite », `DATE_DECISION`).
- **Observable (essai)** : `LEGITEXT000006072050` en version du 2026-10-08 : veille à midi et **minuit de Paris** ⇒ `_01-10-2026` ; minuit UTC et midi de Paris ⇒ `_08-10-2026`. `singleDate` en chaîne `"2026-10-28"` accepté (même total que l'epoch) ; `dates` en `AAAA-MM-JJTHH:MM:SS` et en epoch ms acceptés (total 3 188 comme `AAAA-MM-JJ`). Les sept facettes réduisent le total (LODA 117 170 ⇒ 4 474 ; JURI 188 032 ⇒ 3 188 ; CONSTIT 1 846 ⇒ 56 ; CETAT 51 985 ⇒ 1 943 ; CNIL 1 035 ⇒ 46 ; KALI 27 567 ⇒ 760 ; ACCO 56 475 ⇒ 8 114) ; bornes incluses (34 + 12 = 46). CODE au 1500-01-01 ⇒ 0 résultat ; au 2221-12-31 ⇒ versions futures (`_01-01-2029`) ; `lawDecree` du petit texte au 1500-01-01 et au 1804-03-21 ⇒ 400 marqueur (donc `NotFound` par le port), au 2221-12-31 ⇒ 200. 30 322 horodatages lus : tous à minuit UTC sauf un ; sentinelle `2999-01-01` (15 133), `2222-02-22` jamais vue.
- **Règle** : si midi de Paris donne la version de D et minuit de Paris la précédente alors V-76 levée, règle de 00 §8 confirmée ; sinon amendement de 00 §8 avec ADR (tâche ajoutée). Si une facette ne réduit pas le total alors 04 §6.5 change de facette pour ce fond. Si une date antérieure au texte rend le marqueur alors 04 §7.3 et 05 notent que `get_act` d'une date trop ancienne répond `not_found` (message à prévoir en M3, pas de code ici). L'horodatage non minuit est identifié (fichier, champ) avant de consigner V-100.
- **Consignation** : 09 §2.20 V-76 (levée), V-77, V-78 (levée), V-89, V-100, V-101 (non observée) ; 04 §6.3, §6.5 ; 06 §5.4.
- [ ] **Step 1** : relire ; **Step 2** : lignes ; **Step 3** : commit `docs: sondes de dates en sandbox (V-76, V-77, V-78, V-89, V-100, V-101)`.

### Task 14 : T-M2-16 et T-M2-17, `lawDecree` sur un code (V-96, V-75, V-95, V-97 à V-99) et mojibake (V-71, V-72, V-73)

- **Commande** : `live_lawdecree_on_code_size` (2 appels au plus par code et par campagne) ; familles `lawdecree/`, `mojibake/` ; `live_no_mojibake_on_reference_ids`.
- **Observable (essai)** : Code civil 6 476 648 octets en 1,2 à 1,9 s ; Code du travail 32 918 561 octets en 6,3 à 9,0 s : **seuil de 4 Mio dépassé deux fois**, et le Code du travail dépasse aussi la borne de 16 Mio (`get_act` y échoue aujourd'hui en `Decode`). Code civil : `intOrdre` sur les 2 906 articles et les 622 sections, 7 identifiants d'article en double dans la TOC, `content` non nul sur chaque article. Les six requêtes en mojibake reviennent en UTF-8 propre ; 106 réponses sur 108 en `application/json` sans charset (les 2 autres : 403 HTML, E8) ; aucun échappement `\u00XX` ; 6 détections, toutes fausses (E7).
- **Règle** : si un code dépasse 4 Mio ou 10 s alors C2 (décision du propriétaire, 04 §16 question 13) ; si `intOrdre` manque alors V-97 rouvre le tri de la TOC (04 §9). Si les octets sont propres alors V-72 levée (hypothèse 5 de 09 §4 confirmée : le mojibake des fixtures vient de la capture de la référence) et R-05 réévalué en T-M2-23 ; si du mojibake réel apparaît alors réparation par champ (00 §8).
- **Consignation** : 09 §2.20 V-96, V-75, V-95, V-97, V-98, V-99, V-71, V-72, V-73 ; 04 §7.3, §10.7 ; 06 §5.4.
- [ ] **Step 1** : lignes ; **Step 2** : commit `docs: lawDecree sur deux codes et origine du mojibake (sandbox)`.

### Task 15 (C2, conditionnelle, déclenchée à l'essai) : `get_act` sur un code

**Seulement après** la décision du propriétaire (décision 5). Le plan n'en spécule pas le code : la tâche est rédigée au ledger une fois l'option choisie, sur le modèle des Tasks 2 à 4 (test wiremock d'abord, corps recopié d'un enregistrement), avec une sonde de l'endpoint retenu (au plus 2 appels par code) avant tout code. Options : (a) `/consult/legi/tableMatieres` pour un `LEGITEXT` de code, et reconnaissance d'un code avant l'appel (avancer `/list/code` de M6 amende 00 §8) ; (b) refuser `get_act` sur un code avec un message qui oriente vers `get_article` ; (c) relever la borne de corps de `lawDecree` (D4 du plan 2f, hors périmètre de ce plan).

### Task 16 : T-M2-18 et T-M2-20, erreurs 500 (V-69), modes et requête (V-79, V-80, V-81, V-87, V-88)

- **Commande** : familles `erreurs500/`, `modes/`, `syntaxe/`, `longueur/` ; `live_match_modes_totals`.
- **Observable (essai)** : facette inconnue ⇒ 500 deux fois ; `fond = ALL` avec filtre ⇒ 500 deux fois ; champ racine inconnu ⇒ 200 (ignoré) deux fois ; corps `{"timestamp","error":500,…,"message":"Une exception non gérée est survenue, veuillez contacter l'administrateur",…}`. JURI « contrat de travail » : `UN_DES_MOTS` 606 333 ≥ `TOUS_LES_MOTS_DANS_UN_CHAMP` 119 868 ≥ `EXACTE` 86 213 ; LODA « protection des donnees » : 125 907 ≥ 4 633 ≥ 554. `*` ⇒ 606 350 ; `"contrat de travail"` entre guillemets = `EXACTE` (86 213) ; `contra*` ⇒ 357 711 (joker actif) ; `ET`/`OU` lus comme des mots. Requête de 400 et 450 caractères, et de 100 mots (200 caractères) ⇒ 200 ; 499 et 500 caractères ⇒ **500** ; un mot unique de 100 caractères (`a`×100) ⇒ **403 HTML** du pare-feu.
- **Règle** : 500 identiques deux fois ⇒ déterministes : V-69 consignée, deux tentatives sur 500 gardées (hypothèse 7 de 09 §4) — un réessai y est perdu mais borné. Si `any ≥ all ≥ exact` alors V-79 consignée et transmise à T-M4-10. Si la longueur maximale réelle est sous 500 alors C3 ; si un 403 HTML apparaît alors C4. V-87 et V-88 : lus dans `identifiants/` et `dates/` (sens de `date` des hits, `num` des extraits KALI), **non analysés à l'essai**.
- **Consignation** : 09 §2.20 V-69, V-79, V-80, V-81, V-87, V-88 ; 04 §5.2, §6.4, §6.8 ; 06 §5.4.
- [ ] **Step 1** : analyse V-87, V-88 ; **Step 2** : lignes ; **Step 3** : commit `docs: erreurs 500, modes de recherche et longueur de requête (sandbox)`.

### Task 17 (C3 et C4, conditionnelles, déclenchées à l'essai)

**C3 — longueur de `QueryText`** (décision 6). Files : `crates/lf-domain/src/search.rs` (borne de `QueryText::parse`), message de 05 §7.3 au besoin, 03 §4.1.
- [ ] **Step 1 : test** (rouge) dans `lf-domain` : `query_text_accepts_up_to_the_api_limit` (450 caractères acceptés, 451 refusés, si 450 est retenu).
- [ ] **Step 2 : implémenter** ; `make mutants FILE=crates/lf-domain/src/search.rs` en Task 20 ; commit `fix(domain): longueur maximale de la requête alignée sur PISTE (V-81)`.

**C4 — 403 HTML de la passerelle.** Aujourd'hui `classify` rend tout 403 en `Auth { "HTTP 403 on <endpoint>" }` (`classify.rs:148`), lu comme « application non souscrite ». Files : `src/classify.rs`.
- [ ] **Step 1 : test** (rouge) `a_gateway_html_403_is_not_an_auth_error` : corps et `Content-Type` recopiés de `search/longueur/mot-unique-100.json` ⇒ la variante retenue par la décision 7 (recommandée : `InvalidRequest { status: Some(403), detail: "request rejected by the PISTE gateway" }`) ; un 403 JSON ou vide reste `Auth`.
- [ ] **Step 2 : implémenter** ; commit `fix(adapter): un 403 HTML de la passerelle n'est pas une erreur d'accès (V-81)`.

### Task 18 : T-M2-19 et T-M2-21, accès, quotas, pagination (V-60, V-62 à V-65, V-70, V-74, V-82, V-83)

- **Commande** : familles `pagination/` ; recette de 09 §2.2 pour V-60 et V-74 ; console PISTE pour V-62 (propriétaire).
- **Observable (essai)** : jeton 200, `expires_in` 3600, `openid resource.READ` (recette manuelle, branche `docs/identifiants-piste`) ; application non souscrite ⇒ 403 partout ; aucun 429 ni 401 en 173 appels ; `Content-Encoding` invisible (E1). CODE « contrat » : la page 1 en `DEFAUT` répond `typePagination: "DEFAUT"` (pas `ARTICLE` comme la fixture de référence) ; la page 2 en `ARTICLE` **rend les hits de la page 1**. JURI : positions 9 901 à 10 000 ⇒ 200 (100 hits) ; 10 001 à 10 100 et page 1001 × 10 ⇒ **503** « Une erreur est survenue dans le moteur de recherche ».
- **Règle** : si la page 2 en `ARTICLE` répète la page 1 alors C5 (le repli de la règle 3 de `memo::choose_mode` sert la page 1 en page 2) ; V-82 passe `partiel`, levée formelle en M5 (T-M5-06). Si la fenêtre s'arrête à 10 000 alors `REACHABLE_RESULTS` est confirmé (V-83 `partiel`) ; le 503 hors fenêtre est noté (réessayable par `classify`, mais la garde du domaine l'empêche). V-70 : décision 3. V-63, V-64 : observation continue.
- **Consignation** : 09 §2.20 V-60, V-62, V-63, V-64, V-65, V-70, V-74, V-82, V-83 ; 04 §3.5, §6.6, §6.7 ; 06 §5.4.

**C5 (conditionnelle, décision 8)** — Files : `src/memo.rs` (`choose_mode`, l. ~180), 04 §6.6.
- [ ] **Step 1 : test** (rouge) `a_code_page_without_memo_falls_back_to_defaut` (page 2, `Fond::Code`, mémo absent ⇒ `PaginationMode::Defaut`) ; adapter `a_code_page_without_a_known_type_pagination_leaves_no_memo` si son attendu change.
- [ ] **Step 2 : implémenter** ; commit `fix(adapter): repli DEFAUT en page 2 de CODE sans mémo (V-82)`.

### Task 19 : T-M2-22, contenu des consultations (V-90 à V-95, V-102 à V-113, V-115, V-140 à V-142)

- **Commande** : familles `complet/`, `contenu/`, `identifiants/`, `mojibake/` ; `live_texte_html_presence`.
- **Observable (essai)** : `texteHtml` présent sur `getArticle` (LEGIARTI000006419288, LEGIARTI000033205058) et `juri` ; `idEli` et `idEliAlias` `null` (V-102) ; 800 hits de `pageSize` 100 sur 8 fonds : identifiants tous `8 lettres + 12 chiffres`, suffixe `_JJ-MM-AAAA` sur les `LEGITEXT` (V-103) ; LEGIARTI000033205058 : `textTitles` = `JORFTEXT000033202746` et `LEGITEXT000033205014`, `nature` `LOI` tous deux (V-142). V-91 à V-95, V-104 à V-113, V-115, V-140, V-141 : enregistrés, **non analysés à l'essai**.
- **Règle** : un écart de V-142 ou de V-104 à V-109 avec 04 §8, §9 ⇒ tâche conditionnelle de DTO ou de mapping ajoutée au ledger, sur le modèle de C1 (test d'abord, corps recopié de l'enregistrement) ; sinon consignation seule. V-103 : si une forme autre que 8 + 12 apparaît alors 03 §2 et `lf-domain::id` (bloquant pour v0.1.0).
- **Consignation** : 09 §2.20 pour chaque V ; 06 §5.4.
- [ ] **Step 1** : script d'analyse `python3 -I` (au ledger) ; **Step 2** : lignes ; **Step 3** : commit `docs: contenu des consultations en sandbox (V-90 à V-142)`.

### Task 20 : mutation de la PR 2, revue, clôture

- [ ] **Step 1** : mutation des seuls fichiers touchés par les tâches conditionnelles déclenchées (`error_body.rs`, `classify.rs`, `memo.rs`, `lf-domain/src/search.rs`), un à la fois, après `make mem-check` ; 0 MISSED, 0 TIMEOUT.
- [ ] **Step 2** : `git diff main...HEAD > /var/tmp/m2h2.diff` ; `cargo mutants --list --in-diff` (les fixtures n'en produisent aucun).
- [ ] **Step 3** : revue `code-reviewer` puis `security-auditor` (fixtures relues, aucun identifiant ; Review Focus 1). `make -k ci` (seul `deny`) ; `cargo +1.88 check --workspace --all-targets --all-features --locked`. Rien n'est poussé.

---

## Décisions à faire valider par le propriétaire

1. **Logique dans la bibliothèque (D1) et cov/mutants `--all-features` (D2).** Recommandé : oui, la couverture à 100 % de l'écrivain en dépend.
2. **SHA-256 déplacé dans `sha256.rs` (D4)**, au prix de 63 mutants de code déjà éprouvé dans `mutants-diff`. Recommandé : oui (204 mutants restent sous le plafond) ; repli de la Task 9 step 3 sinon.
3. **V-70 (gzip)** invisible par `reqwest` (E1). Recommandé : retirer `live_gzip_negotiated` de 04 §13.5 et consigner V-70 par une ligne de la recette de 09 §2.2 (`curl --compressed -s -D - -o /dev/null`, en-têtes de **réponse** seulement, jeton lu d'une variable), à la main.
4. **V-60 (`expires_in`)** hors de portée du port (E2). Recommandé : garder le constat manuel (3600) et ne pas exposer le jeton à la feature `record`.
5. **`lawDecree` sur un code (C2)** : 6,2 et 31,4 Mio. Recommandé : sonder `/consult/legi/tableMatieres` (2 appels par code) avant de choisir ; à défaut, option (b) pour la v0.1.
6. **Longueur maximale de requête (C3)** : PISTE répond 500 à 499 caractères, 200 à 450. Recommandé : borne du domaine à 450.
7. **403 HTML de la passerelle (C4)**. Recommandé : `InvalidRequest { status: Some(403) }` au message générique, pour ne pas dire « application non souscrite ».
8. **Repli `ARTICLE` en page 2 de CODE (C5)** : l'essai montre qu'il rejoue la page 1. Recommandé : corriger dans la PR 2 (repli `DEFAUT`) plutôt qu'attendre M5.
9. **Fixtures versionnées ≤ 1 Mio (D11)**, les deux codes en mesures seulement. Recommandé : oui.
10. **`elapsed_ms` dans le sidecar et étiquette générique de `--redact` (D5).** Recommandé : oui.
11. **Fusion de `docs/identifiants-piste` avant la PR 2**, puis deux PR jamais empilées (D12). Recommandé : oui.

## Auto-revue

- Couverture de la spec : T-M2-11 (Tasks 1, 2, 4, 5), T-M2-12 (Task 7), T-M2-14 à T-M2-22 (Tasks 11 à 19), I-5 (Task 3), détecteur (Task 6) ; T-M2-13 en M4, T-M2-23 hors plan. Restent sans test écrit : `live_token_reuse_and_expires_in`, `live_gzip_negotiated` (décisions 3 et 4), les tests M5 et M6 de 04 §13.5.
- Types : `Exchange`, `Probe`, `RecordError`, `exchange`, `example_command`, `RECORDED` ont les mêmes noms d'une tâche à l'autre.
- Proportion : le code est dans l'essai (2 921 lignes ajoutées) ; le plan n'en donne que les signatures, les messages et les valeurs.

## Hors périmètre

- **T-M2-23** : PR documentaire de clôture de M2 (06 §5.4, 09 §2.20 complétés, marqueurs **NON VÉRIFIÉ** retirés, risques R-03 à R-29 réévalués).
- D4 du plan 2f (plafond de corps par endpoint), I-3, niveau 2 du golden (M5) ; T-M2-13 (`check-config --online`, M4) ; tests live de M5 à M7.
- PR `security-fix` : `tokio` 1.53.3 (RUSTSEC-2026-0386).
- Ménage de `/var/tmp/m2h-dry` et des branches `dryrun/m2h`, `plan/m2h-outils-sondes` (propriétaire).
