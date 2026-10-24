# M2g `impl LegalCorpus`, permis tenu pendant le mapping, golden et intégrité des fixtures — plan d'implémentation (T-M2-09, T-M2-10)

> **Origine.** Plan 2g, écrit le 2026-10-24 sur `plan/m2g-corpus` (worktree `/home/muchini/mcp-ligitfrance-m2g`), base `feat/m2f-mapping` @ `7a3e185` (plan 2f exécuté, PR #17 ouverte vers `main`). Il est écrit **depuis un essai à blanc** : chaque tâche a été implémentée dans le worktree jetable `/var/tmp/m2g-dry` (branche `dryrun/m2g`, même base), en commits `dry: …` vérifiés par `make fmt-check lint` et `cargo nextest run -p lf-adapter-piste --lib -j 2`, puis `make cov` et `make -k ci` en fin d'essai. Signatures, noms de tests, valeurs attendues et pièges viennent de ce diff ; les écarts découverts pendant l'essai sont consignés plus bas (« Écarts de l'essai »). **Le test Rust reste l'oracle** : une valeur différente au vert est rapportée au ledger, jamais forcée dans le code.
>
> **Préalables (hors plan).** PR #17 (plan 2f) fusionnée dans `main` ; la branche d'exécution `feat/m2g-corpus` part de `main` à jour (jamais d'une branche non fusionnée : le propriétaire fusionne dans l'ordre, sans PR empilée). `$WT` désigne le worktree d'exécution. Ledger : `$WT/.superpowers/sdd/2026-10-24-m2g-impl-legalcorpus/progress.md` (git-ignoré).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** brancher l'adapter PISTE sur le port (`impl LegalCorpus for PisteCorpus`, sept méthodes) en tenant le permis de concurrence jusqu'à la fin du décodage et du mapping, ne déclarer dans les DTO que les champs lus, et clore T-M2-10 : golden de niveau 1, snapshots `insta`, intégrité des fixtures, mutations des fixtures, job CI `surface`.

**Architecture :** tout vit dans `lf-adapter-piste`. Une tentative réussie rend sa réponse **avec** son permis (`Reply`) ; `post_json` exécute un lecteur synchrone (`read`) sous ce permis, et `fetch` y enchaîne `decode` (budget armé) puis le mapper et `Stats::record_mapping` (sur `Ok` comme sur `Err`). `corpus/port.rs`, sous-module de `corpus` (il voit l'état privé), implémente les sept méthodes sur `request::consult::Consultation` et `request::search::search_body` + `PaginationMemo`. Les tests de contrat restent sous `cfg(test)` dans `src/` (D32 du plan 2f) : `golden.rs`, `integrity.rs`, `map/mutations.rs`, `view.rs` (`insta`).

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain 1.99.0) ; `tokio`, `wiremock` 0.6.5, `serde_json` (déjà au verrou) ; **dev** : `insta` 1.49.0 (`json` seule, tire `similar` 2.7.0), `toml` (déjà au verrou par `xtask`) ; `cargo nextest`, `cargo llvm-cov`, `cargo-mutants` 27.1.0 ; `cargo-fuzz` 0.13.1 sous nightly ; Python 3.13 (`python3 -I`) pour `golden.py` et `check_golden.py` ; `actions/setup-python` 7.0.0.

**Spec :** `docs/plan/07-feuille-de-route.md` T-M2-05 à T-M2-10 et la ligne « Plan 2g » ; `docs/plan/00-decisions.md` §3, §5, §7, §8, §11.1 (exigence du plan 2g, table des écarts) ; `docs/plan/04-adapter-piste.md` §1.3, §1.5, §5.5, §6, §7.1, §8.3 à §8.5, §9, §11, §12.3, §13.2, §13.3, §14.3, §14.4 ; `docs/plan/06-qualite.md` §2.3, §3, §4, §7.1, §12.1 ; `docs/plan/annexes/A-parite.md` §4, §5.3, §5.4 ; clôture du plan 2f (Ruling 6, Mn-5, audit Mi-4).

---

## Global Constraints

- Rust edition 2024, `rust-version = "1.88"` : `cargo +1.88 check --workspace --all-targets --locked` vert en fin de plan (`slice::as_chunks`, employé par la Task 9, existe en 1.88) ; toolchain `1.99.0`.
- Lints : `clippy::pedantic` et `unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`, `dbg_macro` sous `-D warnings` (`make lint` = `clippy --workspace --all-targets --all-features --locked`) ; `unwrap`/`expect`/`panic` permis dans les tests seulement ; `missing_docs`, `unreachable_pub`, `unsafe_code = forbid`. Docs et commentaires en français, identifiants en anglais, messages d'erreur et de journal en anglais, sans point final. Aucun `allow` nouveau ; un `#[expect(lint, reason = "…")]` seulement là où le plan le nomme.
- Couverture : `make cov` = 100 % des lignes, régions et fonctions (`--lib`, code `cfg(test)` compris). Conséquences mesurées à l'essai : une fermeture jamais appelée est une **fonction** non couverte ; une branche d'échec d'un harnais de test (écart, panique) doit vivre dans une fonction exercée par un témoin négatif ; jamais `assert!(matches!(..))`.
- Architecture : `cargo xtask check-arch` (R1 à R9) vert ; `lf-domain` reste pur (seule dépendance normale `thiserror`) et **n'est pas modifié** par ce plan.
- Dépendances : aucune dépendance de production nouvelle. Dev seulement : `insta = { version = "1.49", default-features = false }` au workspace, `features = ["json"]` dans l'adapter (deux paquets au verrou : `insta` 1.49.0, publié le 2026-10-03, et `similar` 2.7.0) ; `toml.workspace = true` en dev (aucun paquet nouveau). `make min-age` vert (règle des 14 jours), `make deny` : licences et sources vertes.
- Aucune valeur du corps dans un `Debug`, un `Display`, un `detail` ou un journal ; `NotFound { what }` porte le gabarit d'endpoint, jamais l'identifiant. Les identifiants vont dans le **corps** des requêtes, jamais dans le chemin ; aucune URL lue dans une réponse n'est appelée.
- Aucune décision dans une `fn new` ni dans un `impl Default` (cargo-mutants ne les mute pas).
- Tests qui lisent une source ou un document par `include_str!` : tolérants au CRLF (`str::lines`, ou `replace("\r\n", "\n")`) ; les octets comparés à l'octet (`include_bytes!`) viennent de chemins marqués `-text` dans `.gitattributes` (leçon de la PR #17).
- WSL (`~/.claude/rules/wsl-safety.md`) : jamais `cargo test -j 4` ni plus (`cargo nextest run -j 2`) ; temporaires sous `/var/tmp` (jamais `/tmp`, tmpfs en RAM) ; un seul texte de 16 Mio vivant à la fois dans un test ; **aucune mutation avant la Task 17**, puis une seule campagne : `make mutants FILE=<fichier>` un fichier à la fois, 0 MISSED et 0 TIMEOUT attendus.
- `make -k ci` à chaque tâche : seule l'étape `deny` peut rester rouge, et seulement pour RUSTSEC-2026-0386 (`tokio` 1.53.2, montée par PR `security-fix` séparée).
- Commits : un par tâche, message conventionnel en français, **sans trailer** ajouté par l'implémenteur (le contrôleur ajoute `Co-Authored-By`) ; rien n'est poussé ; GitHub par le MCP seulement ; le propriétaire ouvre et fusionne les PR.

## Review Focus

1. **Permis rendu trop tôt** (la borne « `max_concurrency` × 102 Mio » de 04 §8.5 redevient fausse) : un lecteur qui tourne hors permis, un permis rendu avant `decode`, ou un appelant qui décode le texte rendu par `post_json` après coup. Épinglé par `the_concurrency_permit_is_held_while_the_body_is_read`, `a_failed_attempt_gives_its_permit_back_before_the_retry`, `the_permit_is_given_back_when_the_reader_fails` et `a_call_waits_while_another_reads_its_body` (Task 2), puis par la forme de l'API : `post_json` ne rend jamais le texte, seulement ce que le lecteur en tire.
2. **Cible de requête pilotée par l'entrée ou par la réponse** (SSRF) : un identifiant toléré (casse, blancs, suffixe) ou une requête qui ressemble à un chemin (`../../oauth/token?grant_type=x#frag`) ne change pas le chemin appelé ; `lienJo`, `urlCC`, `url`, `attachementUrl` et un `href` KALI qui visent le même serveur ne reçoivent aucune requête. Épinglé par `ids_go_in_body_never_in_path` et `urls_in_responses_are_never_fetched` (Task 6).
3. **Appel N+1** : une convention dont la TOC porte des articles en ligne ne déclenche aucun `getArticle`. Épinglé par `kali_inline_content_is_available_without_extra_call` (`expect(0)`, Task 6).
4. **Valeur du corps dans un message** par le chemin complet du port : `sekret-42-Ω` dans un champ strict ne sort ni dans le `detail`, ni dans le `Debug`, ni dans un journal. Épinglé par `a_port_error_never_carries_a_body_value` (Task 6) et `fetch_decodes_with_the_endpoint_and_the_path_in_the_detail` (Task 3).
5. **Page `/search` pleine** : `page_size = 100` et 100 résultats passent ; 101 sont refusés à la lecture (`results[100]: too many elements`) ; 11 résultats pour `page_size = 10` donnent `results: more results than page size` ; aucun compteur n'est touché. Épinglé par `a_full_page_of_100_results_passes_and_more_is_refused` (Task 5).

---

## Décisions de cadrage

Les décisions du plan 2f (D1 à D32, Rulings) restent valables. **[P]** = engage le propriétaire ; les autres lignes sont des rulings du plan, annulables. Le contrôleur reporte tout écart constaté au ledger.

| # | Décision | Source | Si elle est fausse |
|---|---|---|---|
| D1 **[P]** | **Forme du permis** : `send` rend `Outcome<Reply>` où `Reply { response, permit }` ; `post_json(kind, endpoint, body, read)` décode le corps (UTF-8, mojibake), **rend le corps brut**, puis exécute `read(&text)` **avant** de rendre le permis. Écarté : un sémaphore de décodage dédié pris par l'appelant (le corps lu attendrait ce second permis hors de toute borne, et l'oubli de le prendre compilerait) | 00 §11.1 (Ruling 6) | sémaphore dédié de `max_concurrency` places dans `fetch` |
| D2 **[P]** | **Pas de `spawn_blocking`** : `read` est synchrone et s'exécute dans la tâche ; le budget (état du fil) et sa garde `Armed` (`!Send`) restent sur le fil de `decode`, aucun `await` ne les traverse. Coût : jusqu'à ~60 ms de fil bloqué par décodage au pire cas mesuré (04 §8.5), au plus `max_concurrency` à la fois | 04 §8.5, plan 2f D1 | `spawn_blocking` avec texte et identifiant déplacés dans la fermeture, permis compris ; à mesurer en M5 (`perf_upstream_overhead`) |
| D3 | `fetch` appelle `Stats::record_mapping` **à chaque appel**, après `decode` puis le mapper, quel que soit le résultat : sur un `Decode` de lecture ou un `NotFound`, `Diagnostics` est nul et l'appel n'a aucun effet (D14 du plan 2f) | 04 §9.3 | — |
| D4 | `impl LegalCorpus` dans `src/corpus/port.rs`, **sous-module** de `corpus` (`mod port;`) : il lit `self.inner.memo` et appelle `fetch` sans accesseur ; ses tests réutilisent les aides de `corpus::tests` rendues `pub(super)` | 04 §1.5 | accesseurs `pub(crate)` |
| D5 | `Consultation<'a>` lie gabarit et clé du corps (`textId` pour `lawDecree`, `juri`, `cnil` ; `id` pour `getArticle`, `kaliText`, `acco`) ; corps par `serde_json::json!(..).to_string()`, infaillible (aucune branche d'erreur, aucune région morte) | 04 §7.1, P-021 | — |
| D6 **[P]** | `NotFound { what }` garde le **gabarit d'endpoint** au port (aucun identifiant dans une erreur) ; le message `No JURI decision found for {id}.` est rédigé par l'application, qui connaît l'identifiant (M3) | 04 §5.1, 00 §5 | remplacer `what` par l'identifiant dans `port.rs` |
| D7 **[P]** | **Retrait de tous les champs de DTO que le mapping ne lit pas** : 61 champs et trois structs (`LienArticleDto`, `TexteLienDto`, `DecisionAttaqueeDto`), mesurés par le compilateur (méthode en Task 8), et non les seuls cinq champs listés à la clôture du plan 2f ; principe 2 de 04 §8.1. M7 redéclare ce qu'il lit (`versionPrecedente`, `citationJp`, `liens`, `decisionAttaquee`, `lstLien*`…) | 04 §8.1, clôture du plan 2f | ne retirer que `textAbroge`, `isMultipleVersions`, `conformeVersionIntegrale`, `versionPrecedente` et les trois champs que `dead_code` révèle |
| D8 | `search_page` (enveloppe sans le mode) passe en `#[cfg(test)]` : la production appelle `search_page_with_mode` et retient le mode | 04 §9.2 (D18 du plan 2f) | — |
| D9 **[P]** | SHA-256 des sidecars : **implémentation de test** (FIPS 180-4, vecteurs du NIST) plutôt que `sha2` en dev (huit paquets environ ; `sha2` 0.11.1 n'a que 12 jours) ; sidecars lus par `toml` (dev, déjà au verrou par `xtask`), car ceux de `record-fixture` (plan 2h) portent entiers, booléens et chaînes littérales | 06 §4.6, 04 §13.4 | `sha2` 0.11.0 en dev |
| D10 **[P]** | `insta` 1.49.0, `default-features = false`, `json` seule : ni `colors` (`console`) ni `redactions` (`pest`, `sha2`) ; la projection `view` est déterministe, aucune rédaction n'est utile | 04 §1.4, §13.3 | snapshots écrits à la main (`serde_json` + fichiers) |
| D11 **[P]** | **Suffixe de version des identifiants de la TOC : exclu du compteur** `upstream_suffix_stripped`, décision écrite (04 §9.1 c, §9.3, 03 §2.4) ; `lf-domain` inchangé ; la sonde V-104 (plan 2h) lit les corps bruts | Mn-5 (clôture du plan 2f) | compter dans le domaine : `Toc::suffix_stripped()`, report par `map::toc`, mutation du domaine |
| D12 **[P]** | Golden de niveau 1 : `every_golden_divergence_is_exercised` exige les **onze** divergences d'entité et laisse les **huit** de la sortie d'outil (`GoldenDivergence::TOOL_OUTPUT_ONLY`) au niveau 2 (M5) ; `attachment_note` est `Dropped` (texte de l'outil, absent de l'entité) ; texte CNIL comparé jusqu'à la première marque ` [...]` **de l'un ou l'autre côté** | annexe A §5.4 | exiger les 19 au niveau 1 : impossible sans vue d'outil |
| D13 | `golden_copy_matches_plan_data` lit les deux fichiers par `include_bytes!` (jamais « ignoré si `docs/` manque », règle du plan 2f) ; `docs/plan/annexes/donnees/** -text` dans `.gitattributes` | annexe A §5.3, PR #17 | — |
| D14 | `fixture_mutations_never_panic` énumère quatre mutations par nœud (`null`, retrait, changement de type, vidage) sur les 16 fixtures : 10 763 documents, ~10,6 s en debug ; pas de `proptest` | 06 §2.3, T-M2-06 | échantillonner (une mutation sur N) si la CI dépasse ses délais |
| D15 **[P]** | `make surface` entre dans `make ci` ; job `surface` de `ci.yml` (`ubuntu-24.04`, `actions/setup-python` 7.0.0 en Python 3.13, `make surface SURFACE_DIR="$RUNNER_TEMP/surface"`), à ajouter aux checks requis de `main` par le propriétaire (06 §7.6) | 06 §3.2, §7.1 | job séparé hors `make ci` |
| D16 | Dictionnaire libFuzzer `fuzz/dto_decode.dict` (marqueur `sekret`, noms des champs déclarés après la Task 8, valeurs des lecteurs tolérants), passé par `-dict=` dans `weekly.yml` | audit Mi-4 (clôture du plan 2f) | — |
| D17 | **Une seule PR** vers `main` : `cargo mutants --list --in-diff` (diff `7a3e185...HEAD` de l'essai) rend **24 mutants**, très loin du plafond d'environ 450 du job `mutants-diff` (60 min) | consigne du propriétaire | deux PR basées sur `main`, la seconde ouverte après la fusion de la première |

### Écarts de l'essai (mesurés, déjà pris en compte dans les tâches)

| # | Constat | Conséquence |
|---|---|---|
| E1 | Sans l'`expect(dead_code)`, le compilateur ne signale que **trois champs** (`PisteErrorDto.{status, error_code}`, `OAuthErrorDto.error_description`) et la fonction `search_page` : le `#[serde(default)]` de conteneur lit chaque champ de sa valeur par défaut, ce qui les masque à `dead_code` | Task 8 mesure les champs non lus en retirant ce `default` le temps d'un `cargo clippy --lib` (61 champs, liste figée) |
| E2 | Un test de concurrence dont le lecteur **bloque** son fil (`recv()`) passe même quand le permis est rendu trop tôt : la tâche de connexion reste dans l'emplacement LIFO du fil bloqué | le lecteur bloque par `tokio::task::block_in_place` ; vérifié à l'essai en inversant `drop(permit)` et `read` : trois des quatre tests du permis échouent (le quatrième, `the_permit_is_given_back_when_the_reader_fails`, prouve l'autre sens) |
| E3 | Une fermeture passée à `Outcome::map` sur une issue non réussie n'est jamais appelée : fonction non couverte (`make cov` à 95,4 % des fonctions de `classify.rs`) | tests de `map` avec une fonction nommée (`plus_one`) et une fabrique de fermeture (`holding`) appelée aussi sur un succès |
| E4 | `consult_juri_constit` est compté `mojibake_unrepaired = 1` (faux positif `é` + NBSP) | les compteurs attendus se calculent depuis `decoded(fixture, true)` (`repaired`, `detected && !repaired`) |
| E5 | `toml` est déjà au verrou (`xtask`) | sidecars lus par `toml` (D9) ; le verrou ne gagne qu'une arête |
| E6 | Le golden concorde dès le premier passage, sauf le texte CNIL (le `texte` du golden est coupé plus loin que le `texteHtml` débalisé que Rust retient) ; `kind` du golden = nom du fond en minuscules ; `attachment_note` absent de l'entité | règles de la Task 11 (D12) |
| E7 | Retirer `lstLien*` change deux mesures : la TOC de la taille d'un code coûte 20 000 éléments (10 % du budget, 74 000 au plan 2f) et le plafond de 61 niveaux « avec des liens » disparaît (62 et 63 restent) | tests et 04 §8.5 mis à jour (Tasks 8 et 16) |
| E8 | Retirer `PisteErrorDto.status` vide l'unique exemption du balayage des listes (`UNBUDGETED`) : sa branche deviendrait morte en `cfg(test)` | l'exemption est supprimée (champ `exempted`, constante, branche) |
| E9 | `SearchQuery::new` refuse une plage de dates sur CODE : la garde défensive CODE + plage de l'adapter est inatteignable par le port | test réseau nul sur la garde atteignable, `version_date` absente sur LODA ou CODE |
| E10 | `fixture_mutations_never_panic` : 10 763 documents en ~10,6 s ; le chemin d'échec doit être une fonction (`refused`) testée à part | Task 13 |
| E11 | Une fixture sans le champ visé (`data` d'ACCO) : un remplacement par pointeur JSON échoue | l'aide `edited` insère la clé si elle manque |
| E12 | Fuzz de 60 s avec dictionnaire : 293 528 exécutions, `cov: 6085` (5 629 sans dictionnaire au plan 2f), RSS 543 Mo, aucun plantage | Task 15 |
| E13 | Le flake `auth.rs:906-916` (plan 2b) **ne s'est pas reproduit** : `make -k ci` (1 350 tests) puis deux passes `NEXTEST_PROFILE=ci` (1 351 tests, `-j 2`) vertes | rien à faire ici |

### Correspondance des tâches et des commits de l'essai

Branche `dryrun/m2g` (worktree `/var/tmp/m2g-dry`), base `7a3e185`. Deux correctifs et un complément de l'essai sont **fondus** dans leur tâche : le plan décrit directement la forme finale.

| Tâche | Commit(s) `dry:` | Contenu |
|---|---|---|
| 1 | `ce9b4d5` | `request/consult.rs` |
| 2 | `3bf049b` + `f29293f` (correctif E3 : tests de `Outcome::map` sans fermeture jamais appelée) | `Reply`, `post_json(read)`, tests du permis |
| 3 | `25b8c42` | `fetch` |
| 4 | `2b1ccd5` | `corpus/port.rs`, 14 scénarios par fixture |
| 5 | `b4065bc` + `7945b7f` (complément : page de 100 résultats, Review Focus 5) | pagination, mémo, facettes, gardes |
| 6 | `a7b0cae` | sécurité |
| 7 | `9fb2a09` | erreurs |
| 8 | `7630d62` | `expect(dead_code)` et 61 champs retirés |
| 9 | `a44eac6` + `e013b32` (correctif E5 : sidecars lus par `toml` au lieu d'un lecteur maison) | `integrity.rs` |
| 10 | `69b23ee` | `golden.rs`, `.gitattributes` |
| 11 | `ec0a3c2` | niveau 1 du golden |
| 12 | `4918e1c` | `insta`, 17 snapshots |
| 13 | `c16f5c6` | mutations des fixtures |
| 14 | `542b673` | `make surface`, job `surface` |
| 15 | `6c8f782` | dictionnaire libFuzzer |
| 16 | `a2bb7fe` | documents |
| 17 | — (mutation et revues : hors essai) | — |

Fin de l'essai : `make -k ci` vert sauf `deny` (RUSTSEC-2026-0386 seul) ; `make cov` à 100 % (43 464 régions, 3 475 fonctions, 24 347 lignes) ; 1 351 tests au workspace, 879 dans la bibliothèque de l'adapter (816 au départ) ; `cargo +1.88 check --workspace --all-targets --locked` vert ; `make surface` vert ; `actionlint` et `zizmor` (en ligne) muets.

---

## Pré-vérification croisée

Faite à la rédaction : chaque nom ci-dessous a été relu dans le diff de l'essai (`git diff 7a3e185..dryrun/m2g`) ou dans l'Interfaces de la tâche qui le produit.

### Paires de tâches (fichier ou interface partagés)

| Tâches | Fichier ou interface | Constat | Alignement |
|---|---|---|---|
| 1 ↔ 4 | `Consultation::{endpoint, body}` | `endpoint(self)` et `body(self)` prennent `self` (`Copy`) ; `port.rs` appelle `consultation.endpoint()` puis `&consultation.body()` | — |
| 1, 3 ↔ 8 | `reason` de l'`expect(dead_code)` (`lib.rs`) | la Task 1 y ajoute `request::consult`, la Task 3 `fetch` et `Reply` ; des éléments restent morts jusqu'à la Task 8 (`search_page`, trois champs d'erreur), donc l'`expect` reste accompli | Task 8 retire l'attribut entier |
| 2 ↔ 3 | `post_json<T>(kind, endpoint, body, read)` | `fetch` lui passe une fermeture qui capture `self`, `endpoint` et `map` | — |
| 2 ↔ 4 à 7 | `owned` (aide de test) | `fn owned(text: &str) -> Result<String, CorpusError>` reste privée à `corpus::tests` ; `port.rs` ne l'emploie pas | — |
| 3 ↔ 4 | `fetch<D, T>(kind, endpoint, body, map)` | `port.rs` : `consult` l'appelle avec `EndpointKind::Consult`, `search` avec `EndpointKind::Search` et `endpoint::SEARCH` | — |
| 3 ↔ 4 | champ `memo: PaginationMemo<K>` | ajouté en **Task 4** (en Task 3 il serait mort dans le build de test, où l'`expect` ne s'applique pas : `field memo is never read`) | mesuré à l'essai |
| 4 ↔ 5, 6, 7 | aides de `corpus::tests` en `pub(super)` | `SECRET`, `API_BASE`, `TOKEN_PATH`, `secs`, `ms`, `rate`, `Setup` (`credentials`, `tuning`), `json_reply`, `token_mock`, `never`, `requests_to`, `Rig` (`server`, `sleeper`, `corpus`, `new`, `tuned`) | Task 4 les ouvre toutes |
| 4 ↔ 5, 6, 7 | aides de `corpus::port::tests` | `fixture_reply`, `serve`, `expected`, `stats_after`, `golden`, `sent_body`, `check_search` (Task 4) ; `golden_page`, `search_replies`, `sent_modes`, `EMPTY_PAGE`, `juri_results` (Task 5) ; `every_endpoint_replies_empty`, `edited`, `reply_on` (Task 6) ; `each_consultation`, `not_found` (Task 7) | chaque aide est déclarée par la tâche qui l'emploie la première |
| 4 ↔ 8 | `map::search::search_page` | `port.rs` n'emploie que `search_page_with_mode` ; `search_page` passe en `cfg(test)` (D8) | — |
| 8 ↔ 13 | DTO réduits | les mutations portent sur le JSON des fixtures, pas sur les DTO : le compte de 10 763 ne dépend pas de la Task 8 | — |
| 8 ↔ 15 | noms de champs du dictionnaire | le dictionnaire liste les champs **après** la Task 8 | Task 15 après Task 8 |
| 8 ↔ 2f fuzz | `fuzz/fuzz_targets/dto_decode.rs` inclut `dto/*.rs` par `#[path]` | aucun champ n'y est nommé ; la cible compile après la Task 8 (essai de 60 s, Task 15) | — |
| 9 ↔ 10, 11 | `integrity::{sidecar, text_at, known_issues, sha256, hex}` | `golden.rs` emploie `sidecar`, `text_at`, `sha256`, `hex` (Task 10) et `known_issues` (Task 11) | — |
| 9 ↔ 10 | contrôle de l'empreinte du golden | dans `golden_meta_is_expected` (Task 10) seulement, pas dans `fixture_sha256_matches_meta` | — |
| 10 ↔ 11 | `overridden(entry, pointer)`, `GoldenDivergence::TOOL_OUTPUT_ONLY` | produits en Task 10, consommés par `Level1` (Task 11) | — |
| 11 ↔ 12 | `view::view` | le harnais compare au golden la projection `view` ; les snapshots figent la même projection | — |
| 12 ↔ 4 | projection attendue des tests d'endpoint | `expected(name, requested)` = `view(map_body(..))` ; aucun snapshot n'est relu par `port.rs` | — |
| 13 ↔ 4 | `map::fixtures::{map_body, Mapped, decoded, ALL}` | `mutations.rs` les emploie tels que livrés au plan 2f | — |
| 14 ↔ 10 | copie du golden | `make surface` refait par `cmp` ce que `golden_copy_matches_plan_data` prouve en Rust | double contrôle voulu (06 §3.2) |
| 16 ↔ toutes | documents | chaque phrase renvoie à un test ou à un fichier livré par une tâche | — |

### Cohérence interne de chaque tâche

| Tâche | Contrôle |
|---|---|
| 1 | 6 variantes ↔ 6 lignes de table ↔ 6 constantes de `map::endpoint` |
| 2 | 4 bras de `Outcome::map` ↔ 4 assertions ; 11 appels de test de `post_json` gagnent `owned` ; si `drop(permit)` précède `read`, 3 des 4 tests du permis échouent (le 4e vérifie la restitution après un lecteur en erreur) |
| 3 | 3 tests ↔ succès compté, erreur comptée, `Decode` de lecture sans compte |
| 4 | 8 recherches + 8 consultations (dont la variante KALIARTI et les trois décisions) = 14 tests ; corps envoyé vérifié pour chacun |
| 5 | 7 tests ; R12 : trois réponses (absente, inconnue, numérique) |
| 6 | 5 tests ↔ Review Focus 2, 3, 4 |
| 7 | 8 tests ; 6 enveloppes vides ↔ 6 méthodes de consultation |
| 8 | 61 champs ↔ 18 structs ; 3 structs retirées ; 17 listes bornées (25 − 8) ; 1 `null_default` |
| 9 | 6 tests ; 16 sidecars + celui du golden = 17 inclus ; 6 marqueurs `mojibake` ↔ 6 corps réparés |
| 10 | 19 variantes ↔ 19 clés « oui » de l'annexe A §4 ; 6 surcharges ; 8 divergences de sortie d'outil |
| 11 | 17 entrées ; 6 tables de règles (12, 14, 11, 13, 13 et 17 clés) ; 11 + 8 = 19 ; témoin négatif à 4 écarts |
| 12 | 16 fixtures + 1 variante = 17 snapshots |
| 13 | 4 mutations ; 10 763 documents ; `refused` : 4 cas |
| 14 | 5 commandes de `make surface` ↔ étape unique du job |
| 15 | 88 entrées de dictionnaire = 2 marqueurs + 74 noms de champs + 12 valeurs (libFuzzer l'annonce) |
| 16 | chaque document de la liste ↔ une étape |
| 17 | liste des fichiers mutés ↔ `git diff --name-only main...HEAD -- '*.rs'` |

---

## Structure des fichiers

```text
crates/lf-adapter-piste/src/
  request/consult.rs (nouveau)               [T1]        Consultation : gabarit et corps des six consultations
  request/mod.rs                             [T1]        déclaration, en-tête
  classify.rs                                [T2]        Outcome::map
  limit.rs                                   [T2]        RateGate::available (cfg(test)), documentation du permis
  corpus.rs                                  [T2 T3 T4]  Reply, post_json(read), fetch, champ memo, mod port, aides de test pub(super)
  corpus/port.rs (nouveau)                   [T4-T7]     impl LegalCorpus, consult ; 34 tests wiremock
  lib.rs                                     [T1 T3 T8-T10] reason, retrait de l'expect, mod integrity, mod golden, en-tête
  dto/{acco,article,consult_text,error,juri,oauth,search}.rs [T8] champs non lus retirés, tests
  dto/{mod,de}.rs                            [T8]        balayage des listes (17), null_default (1), en-têtes
  map/search.rs                              [T8]        search_page en cfg(test)
  map/{decision,deliberation,fixtures,toc,article}.rs [T8] tests qui lisaient des champs retirés
  integrity.rs (nouveau, cfg(test))          [T9]        SHA-256 de test, sidecars, chaînes de forme jeton
  golden.rs (nouveau, cfg(test))             [T10 T11]   GoldenDivergence, OVERRIDES, annexe A, niveau 1
  view.rs                                    [T12]       mapping_snapshots
  snapshots/*.snap (17, nouveaux)            [T12]
  map/mod.rs, map/mutations.rs (nouveau)     [T13]       fixture_mutations_never_panic
crates/lf-adapter-piste/Cargo.toml           [T9 T12]    dev : toml, insta
Cargo.toml, Cargo.lock                       [T9 T12]    insta au workspace ; arête toml, paquets insta et similar
.gitattributes                               [T10]       docs/plan/annexes/donnees/** -text
Makefile, .github/workflows/ci.yml           [T14]       surface
fuzz/dto_decode.dict (nouveau), .github/workflows/weekly.yml [T15]
docs/plan/{00,02,03,04,06,07,09}-*.md, docs/plan/annexes/A-parite.md [T16]
```

---

## Découpage en PR

**Une seule PR** `feat/m2g-corpus` → `main`, ouverte par le propriétaire **après la fusion de la PR #17** (jamais empilée sur `feat/m2f-mapping`). Estimation de l'essai : `git diff 7a3e185...HEAD > /var/tmp/m2g.diff`, puis `cargo mutants --list --in-diff /var/tmp/m2g.diff | wc -l` (aucune compilation) = **24 mutants** (`corpus/port.rs` 8, `corpus.rs` 7, `request/consult.rs` 5, `classify.rs` 4 ; les champs retirés et le code `cfg(test)` n'en produisent aucun), pour un plafond d'environ 450 dans les 60 min du job `mutants-diff`. La Task 17 refait la mesure sur le diff réel contre `main`.

Commande de test commune à toutes les tâches : `cargo nextest run -p lf-adapter-piste --lib -j 2 --locked <filtre>` (rouge, puis vert) ; puis `make fmt-check lint`, `make cov`, `make -k ci`.

---

### Task 1 : `request/consult.rs`, corps des six consultations

04 §7.1 (corps de la référence, P-021), §14.3 (identifiants dans le corps).

**Files:**
- Create: `crates/lf-adapter-piste/src/request/consult.rs`
- Modify: `crates/lf-adapter-piste/src/request/mod.rs` (`pub(crate) mod consult;` avant `search` ; l'en-tête dit « `consult` ceux des six consultations de la v0.1 ; `number` en M6 ») ; `crates/lf-adapter-piste/src/lib.rs` (la `reason` de l'`expect(dead_code)` gagne « `request::consult (Consultation, endpoint, body)` » après « `request::search::search_body et ce que lui seul emploie` »)

**Interfaces:**
- Consumes : `map::endpoint::{LAW_DECREE, GET_ARTICLE, JURI, CNIL, KALI_TEXT, ACCO}` ; `LegiTextId`, `VersionDate` (`Display` = `AAAA-MM-JJ`), `ArticleId`, `DecisionId`, `CnilTextId`, `KaliTextId`, `AccoTextId` (`as_str`).
- Produces :

```rust
#[derive(Debug, Clone, Copy)]
pub(crate) enum Consultation<'a> {
    LawDecree(&'a LegiTextId, VersionDate), Article(&'a ArticleId), Decision(&'a DecisionId),
    Deliberation(&'a CnilTextId), Convention(&'a KaliTextId), Agreement(&'a AccoTextId),
}
impl Consultation<'_> {
    pub(crate) fn endpoint(self) -> &'static str;   // gabarit de map::endpoint, sans paramètre
    pub(crate) fn body(self) -> Vec<u8>;            // json!(..).to_string().into_bytes() : infaillible
}
```

Clés : `{"textId", "date"}` pour `LawDecree` ; `{"textId"}` pour `Decision` et `Deliberation` ; `{"id"}` pour `Article`, `Convention`, `Agreement`. Sans `preserve_order`, `json!` trie les clés : les tests comparent des `serde_json::Value`, jamais des octets.

- [ ] **Step 1 : tests** (rouges ; aide `body_of(c) -> Value` = `serde_json::from_slice(&c.body())`).
  - `each_consultation_has_its_endpoint_and_its_body_key` : table des six variantes (identifiants des fixtures `LEGITEXT000033205014` au 2026-10-15, `KALIARTI000049886059`, `CETATEXT000054178503`, `CNILTEXT000053352643`, `KALITEXT000049878950`, `ACCOTEXT000053643828`) ⇒ `(endpoint(), body_of(..)) == (gabarit, json!(..))`.
  - `the_body_carries_the_normalized_identifier` : `LegiTextId::parse("  legitext000033205014_01-01-2020 ")` au `1804-03-21` ⇒ `{"textId": "LEGITEXT000033205014", "date": "1804-03-21"}`.
  - `an_endpoint_template_never_holds_an_identifier` : pour `JURITEXT000051743650`, le gabarit ne contient pas `JURITEXT` et n'a que des lettres ASCII et des `/`.
- [ ] **Step 2 : implémenter** (en-tête du module : identifiant dans le corps, jamais dans le chemin ; `searchedString` et `fromSuggest` jamais envoyés). `make fmt-check lint` ; tests verts.
- [ ] **Step 3 : commit** `feat(adapter): corps des six consultations, identifiant dans le corps (request/consult.rs)`.

### Task 2 : le permis de concurrence suit la réponse jusqu'à la fin de la lecture

00 §11.1 (exigence du plan 2g, Ruling 6 de la clôture du plan 2f), 04 §5.5, §8.5 point 5 ; D1, D2. Review Focus 1.

**Files:**
- Modify: `crates/lf-adapter-piste/src/classify.rs` (`impl<T> Outcome<T>` après l'enum ; deux tests)
- Modify: `crates/lf-adapter-piste/src/limit.rs` (`available` sous `cfg(test)` ; l'en-tête et la doc d'`acquire` disent « jusqu'après le décodage et le mapping du corps d'un succès (`corpus`, 04 §5.5) »)
- Modify: `crates/lf-adapter-piste/src/corpus.rs` (`Reply`, `post_json`, `attempt`, `send` ; tests). La `reason` de `lib.rs` n'est pas touchée ici (Task 3 la reprend avec `fetch`) : `post_json` et `Reply` restent morts hors tests, l'`expect` reste accompli.

**Interfaces:**
- Produces :

```rust
// classify.rs
impl<T> Outcome<T> {
    /// La même issue, où la valeur d'un succès passe par `f` ; toute autre issue est recopiée et
    /// `f` tombe sans être appelée (avec ce qu'elle a capturé).
    pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> Outcome<U>;
}
// limit.rs
#[cfg(test)] pub(crate) fn available(&self) -> usize;   // semaphore.available_permits()
// corpus.rs (privés au module)
struct Reply { response: RawResponse, permit: Permit }
async fn attempt(&self, call: &Call<'_>) -> Outcome<Reply>;
async fn send(&self, call: &Call<'_>) -> Result<Outcome<Reply>, CorpusError>;
pub(crate) async fn post_json<T>(
    &self, kind: EndpointKind, endpoint: &str, body: &[u8],
    read: impl FnOnce(&str) -> Result<T, CorpusError>,
) -> Result<T, CorpusError>;
```

Ordre imposé dans `post_json` : `let Reply { response, permit } = run(..).await?;` ; `decode_body(&response.body, ..)` ; `drop(response)` (le corps brut tombe avant la lecture) ; `?` sur le décodage ; `record_decode` ; `let value = read(&decoded.text);` ; `drop(permit)` ; `value`. Dans `send`, `try_send(..).await.map(|response| Reply { response, permit })` **avant** l'oubli du jeton refusé : le permis d'un 401 ou d'un échec tombe avec la fermeture, sans attendre le verrou du jeton. La doc de `post_json` dit que `read` s'exécute sous le permis, est synchrone (le budget ne traverse aucun `await`) et reçoit un corps brut déjà rendu ; `# Errors` finit par « enfin l'erreur de `read` ».

- [ ] **Step 1 : tests de `classify.rs`** (rouges). Aides : `fn plus_one(n: u8) -> u8` ; `fn holding(held: Arc<()>) -> impl FnOnce(u8) -> (u8, Arc<()>)` (`move |n| (n, held)`). **Aucune fermeture qui ne serait jamais appelée** (E3).
  - `map_transforms_a_success_and_copies_every_other_outcome` : `Success(20).map(plus_one) == Success(21)` ; `Unauthorized(Timeout)`, `Transient { error: Timeout, retry_after: Some(2 s), budget: Reduced }` et `Fatal(Timeout)` recopiés à l'identique.
  - `map_drops_its_closure_unused_outside_a_success` : `Fatal(Timeout).map(holding(held.clone()))` ⇒ `strong_count(&held) == 1` et `Fatal(Timeout)` ; `Success(7).map(holding(held.clone()))` ⇒ `strong_count == 2` et `Success((7, held))`.
- [ ] **Step 2 : tests de `corpus.rs`** (rouges). Aide `fn owned(text: &str) -> Result<String, CorpusError>` avec `#[expect(clippy::unnecessary_wraps, reason = "signature imposée par post_json")]` ; les **onze** appels existants `post_json(…, BODY)` deviennent `post_json(…, BODY, owned)` (`Rig::call`, `call_on`, les tests de concurrence, `assert_send`…).
  - `the_concurrency_permit_is_held_while_the_body_is_read` : `Rig::new`, jeton et API (`{}`) une fois ; `free = gate.available()` ; le lecteur rend `(text.len(), gate.available())` ⇒ `Ok((2, free - 1))`, puis `gate.available() == free`.
  - `a_failed_attempt_gives_its_permit_back_before_the_retry` : API 503 puis 200 ; le lecteur rend `gate.available()` ⇒ `Ok(free - 1)` et 2 requêtes API.
  - `the_permit_is_given_back_when_the_reader_fails` : le lecteur rend `Err(Decode { detail: "/consult/juri: .: invalid data" })` ⇒ cette erreur, puis `available() == free`.
  - `a_call_waits_while_another_reads_its_body` (`#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`) : `PisteCorpus::new`, `setup.tuning.max_concurrency = NonZeroU16::MIN`, jeton une fois, API `{}` à volonté ; le premier appel (tâche) signale un `oneshot` depuis son lecteur puis attend un `std::sync::mpsc` par `tokio::task::block_in_place(|| released.recv())` (**jamais un `recv()` nu**, E2) ; une fois le signal reçu, un second appel part ; après `tokio::time::sleep(300 ms)`, `requests_to(API_PATH) == 1` ; libérer ; les deux rendent `Ok("{}")` ; `requests_to == 2`.
- [ ] **Step 3 : implémenter.** `NonZeroU16` rejoint `NonZeroU32` dans les imports de test ; `make fmt-check lint` ; `cargo nextest run -p lf-adapter-piste --lib -j 2` (vert).
- [ ] **Step 4 : contre-épreuve** (non commitée) : intervertir `drop(permit)` et `let value = read(..)` ⇒ `the_concurrency_permit_is_held_while_the_body_is_read`, `a_failed_attempt_gives_its_permit_back_before_the_retry` et `a_call_waits_while_another_reads_its_body` échouent (mesuré à l'essai) ; rétablir.
- [ ] **Step 5 : vérifier** `make cov` (la couverture des fonctions de `classify.rs` à 100 % prouve E3) ; commit `fix(adapter): permis de concurrence tenu pendant le décodage et le mapping du corps (00 §11.1)`.

### Task 3 : `PisteCorpus::fetch`, décodage sous budget, mapping et diagnostics

04 §8.5 (budget armé par `decode`), §9.3 ; D3.

**Files:**
- Modify: `crates/lf-adapter-piste/src/corpus.rs` (imports `serde::de::DeserializeOwned`, `crate::dto::decode::decode`, `crate::map::Diagnostics` ; `fetch` après `post_json` ; tests) ; `crates/lf-adapter-piste/src/lib.rs` (`reason` : « `PisteCorpus::fetch, post_json et ce que seuls ils emploient (Call, Reply, attempt, send, …` »)

**Interfaces:**
- Consumes : `post_json` (Task 2), `decode<T: DeserializeOwned>(text, endpoint)`, `Stats::record_mapping(endpoint, &Diagnostics)`.
- Produces :

```rust
pub(crate) async fn fetch<D: DeserializeOwned, T>(
    &self, kind: EndpointKind, endpoint: &str, body: &[u8],
    map: impl FnOnce(D, &mut Diagnostics) -> Result<T, CorpusError>,
) -> Result<T, CorpusError>;
// = post_json(kind, endpoint, body, |text| { diags = default ; decode(text, endpoint).and_then(|dto| map(dto, &mut diags)) ;
//   record_mapping(endpoint, &diags) (toujours) ; rendre le résultat })
```

Le champ `memo` **n'est pas** ajouté ici (il serait mort dans le build de test) : Task 4.

- [ ] **Step 1 : tests** (rouges). DTO de test `Items { #[serde(default, deserialize_with = "crate::dto::de::bounded_list")] items: Vec<u8> }` (sa lecture exige le budget que seul `decode` arme : un `serde_json::from_str` direct échouerait) ; mapper `fn count(Items { items }: Items, diags: &mut Diagnostics) -> Result<usize, CorpusError>` (paramètre déstructuré, sinon `needless_pass_by_value` ; `#[expect(clippy::unnecessary_wraps, reason = "signature imposée par fetch")]`) qui note un `id_mismatch` et rend `items.len()`.
  - `fetch_maps_the_decoded_body_and_records_its_diagnostics` : `{"items":[1,2,3]}` ⇒ `Ok(3)` ; `stats() == StatsSnapshot { id_mismatch: 1, ..Default }`.
  - `fetch_records_the_diagnostics_of_a_mapping_that_fails` : `{"items":[]}`, mapper qui note un `dropped_hit` puis rend `Decode { detail: "/consult/juri: text.dateTexte: unreadable date" }` ⇒ cette erreur et `dropped_hits == 1`.
  - `fetch_decodes_with_the_endpoint_and_the_path_in_the_detail` : `{"items":[1,"sekret"]}` ⇒ `Decode { detail: "/consult/juri: items[1]: invalid type: string, expected u8" }` ; `stats() == default` (Review Focus 4).
- [ ] **Step 2 : implémenter** (doc : sous le permis ; `record_mapping` une fois par appel, sur `Ok` comme sur `Err` ; un `NotFound` ou un corps illisible n'ont rien compté). Vert, `make fmt-check lint`.
- [ ] **Step 3 : commit** `feat(adapter): PisteCorpus::fetch, décodage sous budget, mapping et diagnostics sur Ok et Err`.

### Task 4 : `impl LegalCorpus for PisteCorpus` et un scénario par fixture

T-M2-09 ; 04 §1.3, §7.1, §12.3, §13.2 (« Endpoints ») ; D4, D5, D6.

**Files:**
- Create: `crates/lf-adapter-piste/src/corpus/port.rs`
- Modify: `crates/lf-adapter-piste/src/corpus.rs` : champ `memo: PaginationMemo<K>` dans `Inner` (doc : « `typePagination` reçu par requête sans page (04 §6.6, §12.3), lu et écrit par `search` »), `memo: PaginationMemo::new(clock.clone())` dans `with_parts`, import `crate::memo::PaginationMemo`, `mod port;` juste avant `fn auth_twice` ; dans `mod tests`, passer en `pub(super)` : `SECRET`, `API_BASE`, `TOKEN_PATH`, `secs`, `ms`, `rate`, `Setup` et ses champs `credentials`, `tuning`, `json_reply`, `token_mock`, `never`, `requests_to`, `Rig` et ses champs `server`, `sleeper`, `corpus`, `Rig::new`, `Rig::tuned`

**Interfaces:**
- Consumes : `fetch` (Task 3), `Consultation` (Task 1), `search_body(&SearchQuery, PaginationMode)`, `PaginationMemo::{mode_for, remember}`, `map::{search::search_page_with_mode, act::act, article::article, decision::decision, deliberation::deliberation, convention::convention, agreement::agreement}`.
- Produces :

```rust
impl<K: MonoClock + Clone, S: Sleeper + Clone> PisteCorpus<K, S> {
    /// = fetch(EndpointKind::Consult, consultation.endpoint(), &consultation.body(), map)
    async fn consult<D: DeserializeOwned, T>(&self, consultation: Consultation<'_>,
        map: impl FnOnce(D, &mut Diagnostics) -> Result<T, CorpusError>) -> Result<T, CorpusError>;
}
impl<K: MonoClock + Clone, S: Sleeper + Clone> LegalCorpus for PisteCorpus<K, S> { /* sept async fn */ }
```

`search` : `let body = search_body(query, memo.mode_for(query))?;` (garde défensive avant toute requête, jeton compris) ; `let (page, received) = self.fetch(EndpointKind::Search, endpoint::SEARCH, &body, |dto, diags| search_page_with_mode(dto, query, diags)).await?;` ; `memo.remember(query, received)` (rien n'est retenu sur `Err`) ; `Ok(page)`. Chaque consultation : `self.consult(Consultation::X(id), |dto, diags| map::x::x(dto, id, diags))` (`get_act` passe `LawDecree(id, date)`). Les futurs sont `Send` sans borne ajoutée (le compilateur le prouve contre la RPITIT `+ Send` du port). L'en-tête du module dit : identifiant dans le corps, gabarits constants, aucune URL de réponse appelée, mémo lu avant l'envoi et retenu après le mapping (R12).

- [ ] **Step 1 : aides de test** (`#[cfg(test)] mod tests` de `port.rs`) : `NOON_2026_10_15: i64 = 1_792_058_400_000` ; `version_date()` (2026-10-15) ; `fixture_reply(name)` (200, `set_body_raw(fixture(name).bytes(), "application/json")`) ; `serve(rig, endpoint, request: Value, name)` (jeton une fois ; `path(API_BASE + endpoint)` et `body_json(request)` ; `expect(1)`) ; `expected(name, requested) -> Value` = `view(&map_body(fixture.kind, &decoded(fixture, true).text, requested, &mut Diagnostics::default()).unwrap())` ; `stats_after(name, id_mismatch) -> StatsSnapshot` = `mojibake_repairs = repaired`, `mojibake_unrepaired = detected && !repaired` (E4), `id_mismatch` ; `golden(fond) -> SearchQuery` (texte `golden`, page 1, taille et mode par défaut, sans plage, `version_date` pour LODA et CODE seulement) ; `sent_body(rig, endpoint) -> Value` (corps de la première requête reçue sur la route) ; `check_search(name, fond, api, filtres)` : page `view` == `expected(name, "")`, et dans le corps envoyé `fond == api`, `typeRecherche == "UN_DES_MOTS"`, `valeur == "golden"`, `filtres` attendus, `(pageNumber, pageSize, typePagination) == (1, 10, "DEFAUT")`, `stats == stats_after(name, 0)`.
- [ ] **Step 2 : tests** (rouges, 14).
  - `search_{loda,code,juri,constit,cetat,cnil,kali,acco}_fixture_maps` : `check_search` avec les fonds d'API `LODA_DATE`, `CODE_DATE`, `JURI`, `CONSTIT`, `CETAT`, `CNIL`, `KALI`, `ACCO` ; `filtres` = `[{"facette": "DATE_VERSION", "singleDate": 1792058400000}]` pour LODA et CODE, `[]` ailleurs.
  - `consult_law_decree_fixture_maps` (`{"textId": "LEGITEXT000033205014", "date": "2026-10-15"}`, `consult_loda`) ; `consult_get_article_fixture_maps` : `LEGIARTI000006419288` (`id_mismatch` 0) puis `KALIARTI000049886059` (variante : `id_mismatch` 1, fixture non modifiée) ; `consult_juri_fixture_maps` : `consult_juri`, `consult_juri_constit`, `consult_juri_cetat` (`{"textId": …}`) ; `consult_cnil_fixture_maps` ; `consult_kali_text_fixture_maps` (`{"id": …}`) ; `consult_acco_fixture_maps`. Chaque test : `view(&Mapped::X(entité)) == expected(name, requested)` et `stats() == stats_after(name, n)`.
- [ ] **Step 3 : implémenter** ; vert ; `make fmt-check lint`.
- [ ] **Step 4 : commit** `feat(adapter): impl LegalCorpus for PisteCorpus (T-M2-09) et un scénario wiremock par fixture`.

### Task 5 : `search` par le port : pagination, mémo (R12, R13), facettes, gardes, page pleine

04 §6.2, §6.5 à §6.7 ; 07 T-M2-05 et T-M2-09 critère (2) ; plan 2f D3. Review Focus 5.

**Files:**
- Modify: `crates/lf-adapter-piste/src/corpus/port.rs` (tests seulement ; imports `lf_domain::date::DateRange`, `lf_domain::fond::{Fond, PaginationMode}`, `crate::corpus::tests::{TOKEN_PATH, never}`)

**Interfaces:**
- Consumes : aides de la Task 4. Produit (tests) : `golden_page(fond, page: u32, range: Option<DateRange>) -> SearchQuery` ; `search_replies(rig, body: &str)` (jeton une fois, `/search` répond `body` à volonté) ; `sent_modes(rig) -> Vec<Value>` (`recherche.typePagination` de chaque requête `/search`, dans l'ordre) ; `EMPTY_PAGE = {"results":[],"totalResultNumber":0}` ; `juri_results(count) -> String` (`count` résultats `JURITEXT{n:012}` titrés `t`, `totalResultNumber = count`).

- [ ] **Step 1 : tests** (rouges, 7).
  - `search_page_2_sends_page_number_2` : LODA page 2 ⇒ `(pageNumber, typePagination) == (2, "DEFAUT")`, page vide.
  - `type_pagination_is_echoed_on_next_page` : JURI (dont le repli est `DEFAUT`), réponse `typePagination: "ARTICLE"` ; pages 1 puis 2 ⇒ `sent_modes == ["DEFAUT", "ARTICLE"]`.
  - `code_page_2_without_memo_sends_article` : CODE page 2, mémo vide ⇒ `["ARTICLE"]`.
  - `a_code_page_without_a_known_type_pagination_leaves_no_memo` : pour `EMPTY_PAGE`, `typePagination: "PAR_PAGE"` et `typePagination: 7`, CODE pages 1 puis 2 ⇒ `first.pagination == Defaut` et `sent_modes == ["DEFAUT", "ARTICLE"]` (mémoriser `DEFAUT` aurait annulé le repli).
  - `date_range_maps_to_fond_facet` : plage 2016-01-01 à 2016-12-31 (`DateRange::new(Some, Some)?.unwrap()`) ; dernier filtre envoyé == `{"facette": F, "dates": {"start": "2016-01-01", "end": "2016-12-31"}}` pour LODA `DATE_SIGNATURE`, JURI, CONSTIT, CETAT `DATE_DECISION`, CNIL `DATE_DELIB`, KALI et ACCO `DATE_SIGNATURE`.
  - `a_loda_or_code_query_without_version_date_never_reaches_the_network` : `never(TOKEN_PATH)`, `never(API_BASE + "/search")` ; `SearchQuery::new(fond, …, None, None)` pour LODA et CODE ⇒ `InvalidRequest { status: None, detail: "version_date missing for fond LODA" }` (`fond.name()`). La garde CODE + plage est inatteignable par le port (E9) : aucun test.
  - `a_full_page_of_100_results_passes_and_more_is_refused` : JURI, `page_size` 100 et 100 résultats ⇒ `Ok(100)` ; `page_size` 100 et 101 ⇒ `Decode { detail: "/search: results[100]: too many elements" }` ; `page_size` 10 et 11 ⇒ `Decode { detail: "/search: results: more results than page size" }` ; chaque fois `stats() == default`.
- [ ] **Step 2 : vert** (aucun code de production ; s'il en faut, la Task 4 est fausse : le rapporter) ; `make fmt-check lint`.
- [ ] **Step 3 : commit** `test(adapter): search par le port, pagination, mémo R12 et R13, facettes, page pleine`.

### Task 6 : sécurité par le port

04 §14.3 ; 06 §2.3 (compléments) ; D-04, D-31, D-25, D-41. Review Focus 2, 3, 4.

**Files:**
- Modify: `crates/lf-adapter-piste/src/corpus/port.rs` (tests ; `path_regex` ; `crate::corpus::tests::requests_to` ; `crate::map::fixtures::text as fixture_text` ; `crate::testing::capture_at`)

**Interfaces:**
- Produit (tests) : `every_endpoint_replies_empty(rig)` (jeton une fois ; `path_regex("^{API_BASE}/")` répond `{}`) ; `edited(name, edits: &[&str], value) -> String` : fixture lue en `Value`, chaque `"objet/clé"` posé à `value` **par insertion dans l'objet parent** (`rsplit_once('/')`, la clé est créée si elle manque, E11) ; `reply_on(rig, endpoint, body: String)` (200 à volonté).

- [ ] **Step 1 : tests** (rouges, 5).
  - `ids_go_in_body_never_in_path` : `search` JURI de texte `../../oauth/token?grant_type=x#frag`, puis les six consultations sur `" legitext000033205014_01-01-2020 "` (2026-10-15), `" kaliarti000049886059 "`, `"constext000052555275"`, `"CNILTEXT000053352643 "`, `" KALITEXT000049878950"`, `"accotext000053643828"` (résultats ignorés) ; requêtes hors jeton = 7, chacune `(chemin, query, corps)` : `/search` sans query et `valeur` égale au texte, puis les six chemins exacts `API_BASE + gabarit`, `query == None`, corps normalisés (`{"textId": "LEGITEXT000033205014", "date": "2026-10-15"}`, `{"id": "KALIARTI000049886059"}`, `{"textId": "CONSTEXT000052555275"}`, `{"textId": "CNILTEXT000053352643"}`, `{"id": "KALITEXT000049878950"}`, `{"id": "ACCOTEXT000053643828"}`).
  - `urls_in_responses_are_never_fetched` : `trap = {uri}/trap` ; mock `path_regex("^/trap")` `expect(0)` ; `consult_juri_constit` avec `/text/lienJo` et `/text/urlCC` = `trap` ; `consult_cnil` idem ; `consult_acco` avec `/acco/url` et `/acco/attachementUrl` ; `consult_kali` dont le texte brut remplace `/affichCodeArticle.do` par `{trap}/kali` (`assert_ne!` avec l'original) ; les quatre consultations rendent `Ok` ; `requests_to("/trap") == 0`.
  - `kali_inline_content_is_available_without_extra_call` : `serve` de `consult_kali`, `never(API_BASE + "/consult/getArticle")` ; dans la projection, `toc[0].section.children[0].article` a `article_id == "KALIARTI000049886059"` et un `inline_text.text` qui commence par « Les parties conviennent de préciser ».
  - `acco_large_data_field_is_skipped` : `edited("consult_acco", &["/acco/data"], "QUJD" × (5 Mio / 4))` ; la chaîne est rendue (`drop`) avant l'appel ; l'accord égale `expected("consult_acco", …)`.
  - `a_port_error_never_carries_a_body_value` : `/consult/juri` répond `{"text":{"titre":"t","numeroAffaire":"sekret-42-Ω"}}` ; sous `capture_at(Level::TRACE)` ⇒ `Decode { detail: "/consult/juri: text.numeroAffaire: invalid type: string, expected a sequence" }` ; ni `to_string()`, ni `Debug`, ni le journal capturé ne contiennent `sekret`.
- [ ] **Step 2 : vert** ; `make fmt-check lint`.
- [ ] **Step 3 : commit** `test(adapter): sécurité par le port, identifiants dans le corps, aucune URL suivie, aucun appel N+1`.

### Task 7 : erreurs par le port

04 §5.1, §4.3, §8.5, §9.3, §11.2 ; T-M2-08 (enveloppes 2xx `null`).

**Files:**
- Modify: `crates/lf-adapter-piste/src/corpus/port.rs` (tests)

**Interfaces:**
- Produit (tests) : `each_consultation(rig) -> [Result<(), CorpusError>; 6]` (les six méthodes, dans l'ordre lawDecree, getArticle, juri, cnil, kaliText, acco, sur les identifiants des fixtures, `.map(drop)`) ; `not_found(endpoint) -> Result<(), CorpusError>`.

- [ ] **Step 1 : tests** (rouges, 8).
  - `consult_empty_envelope_maps_to_not_found` : `{}` (lawDecree, kaliText), `{"article":null}`, `{"text":null}` (juri, cnil), `{"acco":null}` ⇒ six `NotFound { what: <gabarit> }` ; `stats() == default` (un `NotFound` ne compte rien).
  - `unknown_ids_400_map_to_not_found_on_every_consultation` : 400 `{"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null}` sur `^/…/consult/` ⇒ six `NotFound`.
  - `the_400_without_marker_is_an_invalid_request_with_a_generic_detail_on_search` : 400 `{"message":"Champ golden invalide"}` ⇒ `get_decision` : `InvalidRequest { status: Some(400), detail: "Champ golden invalide" }` ; `search` JURI : `detail: "HTTP 400 on /search"`.
  - `api_401_refreshes_once_then_maps` : `tok-1` puis `tok-2` ; `/consult/juri` répond 401 à `Bearer tok-1` et la fixture `consult_juri` à `Bearer tok-2` (`expect(1)` chacun) ⇒ décision égale à `expected("consult_juri", …)`.
  - `the_429_and_the_503_are_retried_then_classified` : `/consult/acco` répond (`expect(3)`) 429 `retry-after: 1` ⇒ `RateLimited { retry_after: Some(1 s) }` ; 503 ⇒ `Upstream { status: Some(503), retryable: true, detail: "HTTP 503 on /consult/acco" }` ; chaque fois `sleeper.waits().len() == 2`.
  - `a_body_of_more_than_16_mib_maps_to_decode` : 16 Mio + 1 octet d'espaces ⇒ `Decode { detail: "response too large (> 16 MiB) on /consult/acco" }`.
  - `a_body_beyond_the_element_budget_maps_to_decode_with_its_path` : `{"articles":[{}×200 001]}` sur lawDecree ⇒ `Decode { detail: "/consult/lawDecree: articles[200000]: too many elements" }`.
  - `a_mapping_error_keeps_the_diagnostics_counted_before_it` : `/search` répond `{"totalResultNumber":2,"results":[{"titles":[{"id":"not-an-id"}]},{"titles":[{"id":"JURITEXT000051743650","title":"t"}],"date":"sekret"}]}` ⇒ `Decode { detail: "/search: results[].date: unreadable date" }` et `stats() == { dropped_hits: 1, .. }`.
- [ ] **Step 2 : vert** ; `make fmt-check lint` ; `make cov` (100 %).
- [ ] **Step 3 : commit** `test(adapter): erreurs par le port, enveloppes vides, 400, 401, 429, 503, 16 Mio, budget, diagnostics sur Err`.

### Task 8 : retrait de l'`expect(dead_code)` et des champs de DTO non lus

07 T-M2-09 (« retire l'`expect(dead_code)` unique de `lib.rs` ») ; 04 §8.1 principe 2 ; D7, D8 ; E1, E7, E8.

**Files:**
- Modify: `crates/lf-adapter-piste/src/lib.rs` (attribut retiré ; l'en-tête dit « Livré au plan 2g : les corps des consultations (`request::consult`) et `impl LegalCorpus for PisteCorpus` (`corpus::port`), qui branche le décodage et le mapping sous le permis de concurrence ; les DTO ne déclarent plus que les champs que le mapping lit » ; la ligne `corpus` des modules cite `fetch` et `corpus::port`, la ligne `request` cite `request::consult`)
- Modify: `crates/lf-adapter-piste/src/dto/{acco,article,consult_text,error,juri,oauth,search}.rs` (champs, structs, en-têtes, tests), `dto/mod.rs` (balayage), `dto/de.rs` (en-tête : `null_default` ne sert plus que le champ `context` d'un article)
- Modify: `crates/lf-adapter-piste/src/map/search.rs` (`#[cfg(test)]` sur `search_page`, doc : « tests seulement : la production passe par `search_page_with_mode` »)
- Modify (tests) : `map/decision.rs`, `map/deliberation.rs`, `map/fixtures.rs`, `map/toc.rs`, `map/article.rs`

- [ ] **Step 1 : mesurer** (sans commit). Retirer l'attribut, puis, dans une copie de travail : `sed -i 's/#\[serde(rename_all = "camelCase", default)\]/#[serde(rename_all = "camelCase")]/; s/^#\[serde(default)\]$//' crates/lf-adapter-piste/src/dto/*.rs` et `cargo clippy -p lf-adapter-piste --lib --locked --message-format=short 2>&1 | grep 'never read'` ; restaurer `dto/` (`git checkout -- crates/lf-adapter-piste/src/dto`). Attendu : exactement la liste du Step 2 (sinon, rapporter l'écart au ledger) ; sans ce retrait temporaire, `dead_code` ne signale que `PisteErrorDto.{status, error_code}`, `OAuthErrorDto.error_description` et `search_page` (E1).
- [ ] **Step 2 : retirer ces 61 champs** (déclaration, doc et attributs qui la précèdent) :
  - `AccordDto` : `file_size`, `numero`, `date_diffusion`, `date_fin`, `conforme_version_integrale` ; `LibelleDto` : `code`.
  - `ArticleDto` : `version_precedente`, `section_parent_titre`, `is_multiple_versions` ; `ArticleContextDto` : `titre_txt` ; `ContextEntryDto` : `id`, `debut`, `fin` (reste `titre` ; doc « Section traversée par un article (entrée de `titresTM`) »).
  - `ConsultTextDto` : `text_abroge`, `type_texte` ; `TocSectionDto` : `date_debut`, `date_fin` ; `TocArticleDto` : `date_debut`, `date_fin`, `lst_lien_citation`, `lst_lien_modification` ; struct `LienArticleDto` (sept champs) retirée.
  - `PisteErrorDto` : `status`, `error_code` (doc : seuls `error` et `message` sont déclarés ; les autres sont sautés sans valeur construite, aucun type ne peut faire perdre le message ni le marqueur, V-67).
  - `JuriTextDto` : `nature_qualifiee`, `citation_jp`, `publication_recueil`, `type_publication_bulletin`, `titre_loi_def`, `id_texte_jo`, `titre_jo`, `ancien_id`, `titrages`, `liens`, `decision_attaquee` ; `SommaireDto` : `id` ; structs `TexteLienDto` (sept champs) et `DecisionAttaqueeDto` (deux) retirées.
  - `OAuthErrorDto` : `error_description`.
  - `SearchResultDto` : `solution`, `nor`, `num`, `themes`, `appellations`, `resume_principal` ; `SearchSectionDto` : `id`, `title` ; `SearchExtractDto` : `search_field_name`.
  Chaque en-tête de module dit : « Seuls les champs que le mapping lit sont déclarés (04 §8.1) ; depuis le plan 2g, <liste> sont sautés par `serde_json` sans construire de valeur et ne peuvent faire échouer aucun décodage ». L'en-tête de `consult_text.rs` : plafond effectif de 62 ou 63 niveaux (les `lstLien*` ne comptent plus, E7).
- [ ] **Step 3 : adapter les tests** (le compilateur les liste) :
  - littéraux nominaux (`a_nominal_*_is_read`, `a_constit_decision_is_read_with_a_null_appealed_decision`, `a_juri_result_keeps_its_snippet_solution_and_summary` renommé `a_juri_result_keeps_its_snippet_and_skips_what_the_mapping_never_reads`) : retirer les champs ; `label(code, libelle)` devient `label(libelle)` ;
  - remplacés : `unread_file_size_and_numero_accept_any_json_value` (acco) et `multiple_versions_alias` + `both_spellings_of_the_multiple_versions_flag_are_a_decode_error` (article) par `fields_the_mapping_never_reads_are_not_declared` (un par fichier : des types hostiles sur `fileSize` `[1]`, `numero` `{"a":1}`, `conformeVersionIntegrale` `"oui"`, `dateFin` `true`, `dateDiffusion` `[2]`, `themes[].code` `{"x":1}` ; resp. `isMultipleVersions` + `multipleVersions` ensemble, `versionPrecedente` `[1]`, `sectionParentTitre` `{"a":1}`, `titresTM[].{id,debut,fin}` de mauvais types, `titreTxt` `"x"` ⇒ DTO réduit aux champs lus) ; `error_is_lenient_and_status_keeps_any_shape` et `error_code_is_lenient_and_never_costs_the_message` par `error_is_lenient` et `status_and_error_code_of_any_shape_never_cost_the_message` (7 valeurs, `message` toujours `Some("kept")`) ;
  - budgets : `every_juri_list_is_budgeted` ne garde que `numeroAffaire` et `sommaire` ; `every_article_list_is_budgeted` perd `titreTxt` ; `every_consult_text_list_is_budgeted` perd la boucle `lstLien*` ; `every_search_list_is_paid_on_the_budget` passe à 5 cas ; `a_code_sized_toc_stays_far_under_the_budget` : `spent == 20_000` (10 % ; dernière assertion sur `articles[8].num == text!("1")`) ; `an_article_with_nine_links_costs_ten_elements` devient `the_links_of_an_article_are_skipped_without_cost` (`spent == 1_000`) ; `a_wrong_type_is_a_decode_error_with_a_json_path_and_no_value` (search) vise `titles` (`results[1].titles: invalid type: string, expected a sequence`) ;
  - `dto/mod.rs` : `every_dto_list_is_bounded` attend `Scan { bounded: 17, readers: 4, unbounded: [] }` ; l'exemption `UNBUDGETED`, le champ `exempted` et sa branche sont supprimés (E8) ; `null_default_serves_only_the_two_struct_fields` devient `null_default_serves_only_the_article_context` (`uses == 1`) ;
  - `map/decision.rs` : `attacked_decision_does_not_change_the_decision` devient `the_attacked_decision_is_never_read` (la fixture `consult_juri` dont `text.decisionAttaquee` vaut `[1, {"formation": true}]`, relue par `fixtures::read`, donne la même `(Decision, Diagnostics)` que la fixture) ; `SommaireDto { id: None, .. }` perd `id` (aussi `map/deliberation.rs`) ; `summary_ignores_the_nomenclature_ids` ne lit plus `dto.titrages` ;
  - `map/fixtures.rs` (`dto_parses_every_fixture`) : plus de `titre_txt` ni de `liens` ; `acco.date_depot == Some(RawDate::Millis(1_772_150_400_000))` remplace `date_fin` ; `map/toc.rs` : littéraux sans dates (import `RawDate` retiré) ; `map/article.rs` : `ContextEntryDto { titre }` (sans `..Default`, `needless_update`).
- [ ] **Step 4 : vérifier.** `make lint` (aucun `allow` nouveau), `cargo clippy -p lf-adapter-piste --lib --locked -- --force-warn dead_code` sans avertissement, `cargo nextest run -p lf-adapter-piste --lib -j 2` (vert), `make cov`.
- [ ] **Step 5 : commit** `refactor(adapter): retrait de l'expect(dead_code) et des 61 champs de DTO non lus (04 §8.1)`.

### Task 9 : `src/integrity.rs`, intégrité des fixtures

04 §13.3 ; 06 §4.4 point 5, §4.6 ; D-35 ; D9 ; E5.

**Files:**
- Create: `crates/lf-adapter-piste/src/integrity.rs` (`cfg(test)`)
- Modify: `crates/lf-adapter-piste/src/lib.rs` (`#[cfg(test)] mod integrity;` entre `http` et `limit` ; ligne de module « `integrity` (tests seulement) : … ») ; `crates/lf-adapter-piste/Cargo.toml` (dev : `toml.workspace = true`, commentaire « lecture des sidecars `.meta.toml` (integrity.rs) ; déjà au verrou par xtask ») ; `Cargo.lock` (arête `toml` de `lf-adapter-piste`, par `cargo check -p lf-adapter-piste --tests --offline`)

**Interfaces:**
- Produces :

```rust
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32];     // FIPS 180-4 ; K et H0 en constantes ; blocs par as_chunks::<64>()
fn compress(state: &mut [u32; 8], block: &[u8; 64]); // #[expect(clippy::many_single_char_names, reason = "notation de FIPS 180-4 : a à h et w")]
pub(crate) fn hex(digest: &[u8]) -> String;         // fold + write!("{byte:02x}")
const SIDECARS: [(&str, &str); 17];                 // 16 fixtures + parite-golden, include_str!
pub(crate) fn sidecar(name: &str) -> toml::Table;   // toml::from_str(..).expect(..)
pub(crate) fn text_at<'a>(meta: &'a toml::Table, key: &str) -> Option<&'a str>;
pub(crate) fn known_issues(meta: &toml::Table) -> Vec<&str>;
```

- [ ] **Step 1 : tests** (rouges, 6).
  - `sha256_matches_the_fips_180_vectors` : `""` ⇒ `e3b0c442…b855`, `"abc"` ⇒ `ba7816bf…15ad`, le message de 448 bits ⇒ `248d6a61…06c1`, un million de `a` ⇒ `cdc76e5c…2cd0`.
  - `a_sidecar_reads_texts_and_lists` : `consult_juri` : `text_at("endpoint") == Some("/consult/juri")`, `text_at("absent") == None`, `known_issues == ["mojibake"]` ; `consult_acco` : `[]`.
  - `fixtures_have_meta_sidecar` : pour les 16 fixtures (`ALL`), clés exactement `source, endpoint, sha256_raw, known_issues, env, captured_at` plus `fond` (absent pour `consult_article`, seul endpoint sans fond) ; `source == "matematicsolutions/fr-eli-mcp@d3b5b13"`, `endpoint == fixture.endpoint`, `fond` attendu (nom du fond pour une recherche, `LODA`, `KALI`, `CNIL`, `ACCO`, et pour `/consult/juri` `JURI`, `CETAT`, `CONSTIT` selon la fixture), `(env, captured_at) == ("sandbox", "inconnu")` ; `known_issues == ["mojibake"]` **si et seulement si** `decoded(fixture, true).repaired` ; 6 en tout.
  - `fixture_sha256_matches_meta` : `hex(sha256(bytes)) == sha256_raw` pour les 16.
  - `the_reference_directory_holds_the_fixtures_their_sidecars_and_the_golden` : `read_dir(CARGO_MANIFEST_DIR/tests/fixtures/reference)` == les 16 `.json` et `.meta.toml` + `parite-golden.json` et `.meta.toml` (34 noms).
  - `fixtures_contain_no_token_like_strings` : aucun `Bearer `, `access_token`, `client_secret` ; aucune forme JWT (`eyJ` puis deux segments `[A-Za-z0-9_-]+` séparés de points) ; aucune valeur JSON de plus de 40 caractères de l'alphabet base64 (`=` final ôté), sauf le pointeur déclaré `("consult_kali", "/sections/0/articles/0/path")` ; témoins : `looks_like_jwt("x eyJhbGciOi.eyJzdWIiOi.c2ln")`, `!looks_like_jwt("eyJ seul")`, `looks_like_base64("QUJD" × 11)`, `!looks_like_base64("/KALISCTA000049886029/KALIARTI000049886059.")`.
- [ ] **Step 2 : implémenter** ; vert ; `make fmt-check lint` ; `make cov` (chaque bras couvert par un test).
- [ ] **Step 3 : commit** `test(adapter): intégrité des fixtures, sidecars, SHA-256 et chaînes de forme jeton (integrity.rs)`.

### Task 10 : `src/golden.rs`, divergences, surcharges et cohérence avec l'annexe A

Annexe A §4, §5.1 à §5.4 ; 06 §2.3 (`parity_golden`) ; D12, D13.

**Files:**
- Create: `crates/lf-adapter-piste/src/golden.rs` (`cfg(test)`)
- Modify: `crates/lf-adapter-piste/src/lib.rs` (`#[cfg(test)] mod golden;` entre `error_body` et `http` ; ligne de module) ; `.gitattributes` (`docs/plan/annexes/donnees/** -text`, commentaire : empreintes citées en 00 §10, copie comparée à l'octet)

**Interfaces:**
- Consumes : `integrity::{sidecar, text_at, sha256, hex}`, `map::fixtures::golden()`.
- Produces :

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GoldenDivergence { /* 19 variantes, ordre de l'annexe A §4 : CitationRenamed … ReturnedIdKept */ }
impl GoldenDivergence {
    pub(crate) const ALL: [Self; 19];
    pub(crate) const TOOL_OUTPUT_ONLY: [Self; 8]; // NotesDropped, NullsOmitted, SearchEchoDropped, FieldsRenamed,
                                                  // FieldsNotRendered, TitleOmittedWhenInCitation, FieldsAdded, ByteSizeToTextLength
    pub(crate) const fn key(self) -> &'static str; // snake_case de l'annexe
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct Override { pub(crate) entry: &'static str, pub(crate) pointer: &'static str,
                             pub(crate) expected: &'static str /* JSON */, pub(crate) because: GoldenDivergence }
pub(crate) const OVERRIDES: [Override; 6];        // les six lignes de l'annexe A §5.4 (deux CitationTypography, EtatJurisStateFallback,
                                                  // NumeroAffaireCetat, deux ReturnedIdKept)
pub(crate) fn overridden(entry: &str, pointer: &str) -> Option<(Value, GoldenDivergence)>;
```

- [ ] **Step 1 : tests** (rouges, 5 ; `COPY` et `PLAN` par `include_bytes!` de `../tests/fixtures/reference/parite-golden.json` et `../../../docs/plan/annexes/donnees/parite-golden.json`, `ANNEX_A` par `include_str!` de `../../../docs/plan/annexes/A-parite.md`).
  - `golden_divergences_match_annex_a` : clés de `ALL` (19) == clés de l'annexe A, extraites entre `## 4. Divergences volontaires consolidées` et `## 5. Données golden` (lignes `| \``, première cellule sans accents graves, dernière cellule commençant par `oui`), et le même ensemble sur l'annexe aux fins de ligne `\r\n` (`lines`).
  - `tool_output_only_divergences_are_known_variants` : 8, sous-ensemble de `ALL`.
  - `overrides_point_to_existing_golden_values` : chaque pointeur existe dans `golden()["normalizer"][entry]` et diffère de la valeur attendue ; `overridden(entry, pointer) == Some((expected, because))` ; `overridden("consult_loda.json", "/title") == None`.
  - `golden_meta_is_expected` : `_meta` égale les quatre textes de l'annexe A §5.2 (dont `generated_by` historique) ; `normalizer` a 17 entrées, `tool_output` 16 ; sidecar du golden : clés `license, reference, sha256, source`, `source == "docs/plan/annexes/donnees/parite-golden.json"`, `reference == "matematicsolutions/fr-eli-mcp@d3b5b13"`, `sha256 == hex(sha256(COPY))`.
  - `golden_copy_matches_plan_data` : `COPY.len() == 118_107` et `COPY == PLAN` (message court sur échec, sans afficher 118 Ko).
- [ ] **Step 2 : implémenter** ; vert ; `git status` : `.gitattributes` seul modifié hors `src/` (aucun fichier de `donnees/` ne change).
- [ ] **Step 3 : commit** `test(adapter): harnais du golden, divergences, surcharges, annexe A et copie (golden.rs)`.

### Task 11 : `mapping_matches_golden` (niveau 1) et `every_golden_divergence_is_exercised`

Annexe A §5.4 (comparateurs, correspondance des clés) ; P-030 à P-039 ; D12 ; E6.

**Files:**
- Modify: `crates/lf-adapter-piste/src/golden.rs`

**Interfaces:**
- Consumes : `overridden`, `GoldenDivergence` (Task 10) ; `integrity::{sidecar, known_issues}` ; `map::fixtures::{Fixture, Kind, decoded, fixture, golden, map_body}` ; `view::view` ; `lf_domain::text::clean_field`, `lf_domain::text::encoding::repair_latin1_mojibake`.
- Produces (privés au module) :

```rust
enum Rule { Eq(&'static str), Clean(&'static str), Date(&'static str), LongText, Toc, Articles, Citation, SourceUrl, Ecli, Kind, Dropped }
const HIT: [(&str, Rule); 12]; const TEXT: [(&str, Rule); 14]; const ARTICLE: [(&str, Rule); 11];
const DECISION: [(&str, Rule); 13]; const DELIBERATION: [(&str, Rule); 13]; const AGREEMENT: [(&str, Rule); 17];
struct Level1<'a> { entry: &'a str, mojibake: bool, rech_ecli: bool, exercised: BTreeSet<GoldenDivergence>, mismatches: Vec<String> }
impl Level1<'_> { fn entity(&mut self, kind: Kind, expected: &Value, rust: &Value); /* compare, object, cleaned, dated, long_text, … */ }
fn level1() -> (BTreeSet<GoldenDivergence>, Vec<String>);
```

Règles, clé du golden ⇒ clé de `view` : `Eq` égalité stricte ; `Clean` : `clean_field(Some(repair(golden)))` (`repair` seulement si le sidecar porte `mojibake`) ; `Date` : `≥ "2222-01-01"` ⇒ `null` ; `Citation` : `human_readable_citation` ⇒ `contract.citation` en `Clean` ; `SourceUrl` ⇒ `contract.source_url` ; `Ecli` ⇒ `contract.ecli` ; `Kind` : `kind` == nom du fond en minuscules ; `Toc` : `[article_id, clean(num), etat]` des articles du golden == articles aplatis en préordre de `view["toc"]` ; `Articles` (hits) : `[article_id, clean(num), clean(snippet)]` ; `LongText` : `fold_ws` (blancs Unicode repliés) des deux côtés, ECLI final ôté du golden quand le corps porte `<RECH_ECLI>` (le golden doit finir par `" " + contract.ecli`), puis comparaison du **préfixe commun jusqu'à la première marque ` [...]` de l'un ou l'autre côté** ; `Dropped` : `eli_uri`, `attachment_note`. Tables : `HIT` (`kind`, `id`, `cid`, `title` C, `nature` C, `etat`, `idcc`, `raison_sociale` C, `articles`, citation, URL, `eli_uri`) ; `TEXT` (`text_id`, `cid`, `nor`, `title` C, `nature` C, `etat`, trois dates, `num_parution`, `articles` ⇒ Toc, citation, URL, `eli_uri`) ; `ARTICLE` ; `DECISION` ; `DELIBERATION` ; `AGREEMENT` (dont `attachment_note` Dropped). Une surcharge (`overridden`) remplace la valeur du golden avant toute règle. Divergences exercées : `MojibakeRepaired` et `ShortTextCleaned` quand la transformation change la valeur, `SentinelDatesNone` sur une sentinelle, `LongTextStructured`, `TocTree`, `CitationRenamed` à chaque usage, `EliUriDropped` sur `eli_uri`, la divergence d'une surcharge appliquée. Une clé du golden absente de la table de son entité est un écart (`unclassified golden key`).

- [ ] **Step 1 : tests** (rouges, 3).
  - `mapping_matches_golden` : `level1().1 == []` (les 17 entrées).
  - `every_golden_divergence_is_exercised` : exercées ∩ `TOOL_OUTPUT_ONLY` = ∅, exercées ∪ `TOOL_OUTPUT_ONLY` = `ALL`, 11 exercées.
  - `the_harness_reports_mismatches_and_unknown_keys` (témoin négatif, couvre chaque branche d'écart) : `Level1` sur `entry = "synthetic.json"`, `entity(Kind::Search(Fond::Juri), golden, rust)` avec `golden = {"total": 2, "hits": [{"id": "A", "secret_key": 1}, {"id": "B"}]}`, `rust = {"total": 3, "hits": [{"id": "Z"}]}` ⇒ exactement `["synthetic.json /total: golden 2 != rust 3", "synthetic.json /hits: golden 2 != rust 1", "synthetic.json /hits/0/id: golden \"A\" != rust \"Z\"", "synthetic.json /hits/0/secret_key: unclassified golden key"]`.
- [ ] **Step 2 : implémenter.** Mesure de l'essai : au premier passage, seul le texte CNIL diffère (le golden suit `texte`, coupé à 606 caractères ; Rust suit `texteHtml`, coupé plus tôt, à « … données à [...] ») : la règle du préfixe commun aux deux marques le résout. Toute autre différence est un écart à rapporter, jamais une règle ajoutée sans l'annexe A.
- [ ] **Step 3 : vérifier** `make cov` (aucune branche du harnais non couverte) ; commit `test(adapter): mapping_matches_golden de niveau 1 et every_golden_divergence_is_exercised (annexe A §5.4)`.

### Task 12 : `mapping_snapshots` (`insta`)

04 §13.3, P-038 ; annexe A §5.5 point 10 ; D10.

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`, après `pretty_assertions` : `insta = { version = "1.49", default-features = false }`, commentaire : sans `colors` ni `redactions`) ; `crates/lf-adapter-piste/Cargo.toml` (dev : `insta = { workspace = true, features = ["json"] }`) ; `Cargo.lock` (`cargo check -p lf-adapter-piste --tests` : `insta` 1.49.0, `similar` 2.7.0)
- Modify: `crates/lf-adapter-piste/src/view.rs` (test)
- Create: `crates/lf-adapter-piste/src/snapshots/lf_adapter_piste__view__tests__<nom>.snap` (17 ; `**/snapshots/** -text` est déjà dans `.gitattributes`, `*.snap.new` dans `.gitignore`)

- [ ] **Step 1 : test** `mapping_snapshots` : pour chaque fixture de `ALL`, `insta::assert_json_snapshot!(fixture.name, view(&map_fixture(fixture, ..)))` ; puis la variante (`map_body(Kind::GetArticle, decoded(consult_article).text, "KALIARTI000049886059", ..)`) sous le nom `consult_article_kaliarti000049886059` (aucun crochet dans un nom de fichier). Doc : un snapshot neuf ou différent fait échouer le test ; hors CI, `insta` écrit un `.snap.new` à relire (`INSTA_UPDATE=auto` vaut `no` quand `CI` est posé).
- [ ] **Step 2 : écrire les snapshots** : `INSTA_UPDATE=always cargo nextest run -p lf-adapter-piste --lib -j 2 mapping_snapshots`, puis relire les 17 fichiers (en-tête `source: crates/lf-adapter-piste/src/view.rs`, `expression: view(&mapped)`, ~128 Ko en tout, aucun chemin absolu) ; relancer sans variable, puis avec `CI=true` : vert. Contre-épreuve (non commitée) : changer `"275"` en `"276"` dans `…consult_acco.snap` ⇒ `CI=true` rouge ; rétablir ; aucun `.snap.new` ne reste.
- [ ] **Step 3 : vérifier** `make min-age` (`insta` 21 jours, `similar` 642 jours), `make deny` (licences, sources ; seul RUSTSEC-2026-0386), `make machete`, `cargo +1.88 check --workspace --all-targets --locked`.
- [ ] **Step 4 : commit** `test(adapter): mapping_snapshots, snapshots insta de la projection view (17)`.

### Task 13 : `fixture_mutations_never_panic`

06 §2.3 (compléments) ; T-M2-06 ; D-23, D-24 ; D14 ; E10.

**Files:**
- Create: `crates/lf-adapter-piste/src/map/mutations.rs` (`cfg(test)`)
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`#[cfg(test)] mod mutations;` après `fixtures`)

**Interfaces:**
- Consumes : `map::fixtures::{ALL, Fixture, Mapped, decoded, map_body}`.
- Produces (privés) :

```rust
enum Mutation { Null, Removed, Retyped, Emptied }      // Debug, Clone, Copy, PartialEq, Eq
const MUTATIONS: [Mutation; 4];
fn pointers(value: &Value, pointer: &str, out: &mut Vec<String>);       // préordre, racine "" comprise ; clés échappées ~0 ~1
fn retyped(node: &Value) -> Value;     // null ⇒ "x", bool ⇒ son texte, nombre ⇒ son texte, texte ⇒ 7, liste ⇒ {}, objet ⇒ []
fn mutated(document: &Value, pointer: &str, mutation: Mutation) -> Option<Value>; // None : racine ou parent liste (Removed), vide ou scalaire (Emptied)
fn refused(label: &str, outcome: std::thread::Result<Result<Mapped, CorpusError>>) -> Option<String>; // Ok, Decode, NotFound ⇒ None
fn check(fixture: &Fixture) -> (Vec<String>, usize);  // catch_unwind(AssertUnwindSafe(map_body(..))) ; failures.extend(refused(..))
```

- [ ] **Step 1 : tests** (rouges, 3).
  - `fixture_mutations_never_panic` : écarts de toutes les fixtures `== []` ; documents essayés `== 10_763` (mesure de l'essai, ~10,6 s en debug).
  - `every_mutation_has_its_effect` : sur `{"a": [1, "t", true, null, {"b": {}}], "c~/d": {"e": []}}` : pointeurs `["", "/a", "/a/0", "/a/1", "/a/2", "/a/3", "/a/4", "/a/4/b", "/c~0~1d", "/c~0~1d/e"]` ; `Null` sur `/a/0` ; `Removed` sur `/c~0~1d` (clé `c~/d` retirée), `None` sur `/a/0` et `""` ; `Emptied` sur `/a` et `/a/4`, `None` sur `/a/4/b` ; `retyped` des six types.
  - `only_a_panic_or_an_unexpected_error_is_refused` : `Decode` et `NotFound` ⇒ `None` ; `Timeout` ⇒ `Some("m: Timeout")` ; `Err(Box::new("boom"))` ⇒ `Some("m: panic")`.
- [ ] **Step 2 : implémenter** (le chemin d'échec passe **uniquement** par `refused`, sans fermeture ni bras propres à `check`, sinon des régions de test restent non couvertes) ; vert ; `make cov`.
- [ ] **Step 3 : commit** `test(adapter): fixture_mutations_never_panic, 10 763 documents mutés par le chemin de production`.

### Task 14 : `make surface` et job CI `surface`

06 §3.2, §7.1 ; 07 T-M2-10 (« job `surface` ») ; D15.

**Files:**
- Modify: `Makefile` (`surface` dans `.PHONY` ; variables `SURFACE_DIR ?= /var/tmp/mcp-legifrance-surface` et `DONNEES := docs/plan/annexes/donnees` ; cible `surface` ; `ci: … cov surface`)
- Modify: `.github/workflows/ci.yml` (job `surface` avant `deny`)

- [ ] **Step 1 : cible.** Commentaire : golden reproduit par son script versionné depuis les fixtures copiées dans le crate, comparé à l'octet au fichier du plan (et la copie du crate aussi), puis contrôlé par `check_golden.py`, Python 3.13 en `python3 -I` (00 §10). Recette, dans cet ordre : `mkdir -p "$(SURFACE_DIR)"` ; `python3 -I $(DONNEES)/scripts/golden.py crates/lf-adapter-piste/tests/fixtures/reference "$(SURFACE_DIR)/parite-golden.json"` ; `cmp "$(SURFACE_DIR)/parite-golden.json" $(DONNEES)/parite-golden.json` ; `cmp crates/lf-adapter-piste/tests/fixtures/reference/parite-golden.json $(DONNEES)/parite-golden.json` ; `python3 -I $(DONNEES)/scripts/check_golden.py $(DONNEES)/parite-golden.json`.
- [ ] **Step 2 : job.** `surface`, `name: golden de parité`, `runs-on: ubuntu-24.04`, `timeout-minutes: 5` ; étapes : `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1` (`persist-credentials: false`), `actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0` (`python-version: "3.13"` ; v7.0.0 publiée le 2026-07-20 ; la v7.1.0, du 2026-10-22, n'a pas 14 jours), `run: make surface SURFACE_DIR="$RUNNER_TEMP/surface"` (variable du shell, aucune expansion `${{ }}`).
- [ ] **Step 3 : vérifier** `make surface` (dernière ligne `all test_parse assertions hold on golden`) ; `actionlint` 1.7.12 (binaire de `workflow-lint.yml`, somme `8aca8db9…a3d8`, sous `/var/tmp`) muet ; `TMPDIR=/var/tmp uvx --from zizmor==1.30.1 zizmor .github/` muet, en ligne avec `GH_TOKEN=$(gh auth token)` (audits `ref-version-mismatch`, `impostor-commit`).
- [ ] **Step 4 : commit** `ci: job surface et make surface, golden reproduit par golden.py puis cmp et check_golden.py`. Le propriétaire ajoute `golden de parité` aux checks requis de `main` (06 §7.6).

### Task 15 : dictionnaire libFuzzer de `dto_decode`

Audit Mi-4 et rapport docs de la clôture du plan 2f ; 06 §1.4 ; D16 ; E12.

**Files:**
- Create: `fuzz/dto_decode.dict`
- Modify: `.github/workflows/weekly.yml` (étape `dto_decode` : `-dict=fuzz/dto_decode.dict` après `-max_len=65536`, sur une ligne de continuation ; le commentaire de l'étape dit que le dictionnaire porte les noms des champs déclarés et le marqueur `sekret` de l'oracle, lignes de 100 colonnes au plus)

- [ ] **Step 1 : dictionnaire** (format libFuzzer, commentaires `#`) : `marker="sekret"`, `marker_quoted="\"sekret\""` ; les noms JSON des champs **déclarés après la Task 8**, entre guillemets (`"\"results\""`, `"\"titles\""`, … `"\"dateDepot\""` : 74 noms) ; valeurs des lecteurs tolérants (`"\"DEFAUT\""`, `"\"ARTICLE\""`, `"\"2999-01-01\""`, `"\"2222-02-22\""`, `"\"2016-10-09T00:00:00.000+0000\""`, `"\"1475884800000\""`, `"-5231606400000"`, `"\"LEGITEXT000033205014_08-09-2023\""`, `"null"`, `"true"`, `"[]"`, `"{}"`). Aucun nom de champ retiré à la Task 8.
- [ ] **Step 2 : essai de 60 s** : `mkdir -p fuzz/corpus/dto_decode && TMPDIR=/var/tmp cargo +nightly fuzz run dto_decode fuzz/corpus/dto_decode crates/lf-adapter-piste/tests/fixtures/reference -- -max_total_time=60 -max_len=65536 -dict=fuzz/dto_decode.dict`. Attendu : `Dictionary: 88 entries`, aucun plantage, code de sortie 0 (essai : 293 528 exécutions, `cov: 6085`, RSS 543 Mo). `actionlint` et `zizmor` muets.
- [ ] **Step 3 : commit** `ci(fuzz): dictionnaire libFuzzer de dto_decode, marqueur sekret et champs déclarés`.

### Task 16 : documentation du plan 2g

Aucune ligne de Rust. Repères par `grep -n "plan 2g" docs/plan/*.md docs/plan/annexes/*.md` (le passage de l'essai a remplacé chacune des mentions ci-dessous).

**Files:**
- Modify: `docs/plan/04-adapter-piste.md`, `docs/plan/00-decisions.md`, `docs/plan/07-feuille-de-route.md`, `docs/plan/06-qualite.md`, `docs/plan/03-domaine.md`, `docs/plan/02-architecture.md`, `docs/plan/09-risques-et-questions.md`, `docs/plan/annexes/A-parite.md`

- [ ] **Step 1 : 04.** Statut (l. 3) : plan 2g livré, reste le plan 2h. §1.3 : le commentaire du bloc devient `/* livré, plan 2g : corpus/port.rs */` ; le paragraphe « Livré (plan 2c, tâche 11) » décrit `post_json(kind, endpoint, body, read)`, `fetch` et le retrait de l'`expect`. §1.4 : ligne « dev du plan 2g » = `insta` 1.49.0 (`json`, tire `similar` 2.7.0) et `toml`, `proptest` n'y entre pas (D10, D14). §1.5 : `corpus.rs` (`fetch`), `corpus/port.rs`, `golden.rs`, `integrity.rs`, `request/consult.rs` livré, `map/mutations.rs`. §5.5 : le permis d'une tentative réussie est rendu après le décodage et le mapping. §5.1 (`NotFound { what }`) : gardé par le port (D6). §6.10 : garde CODE + plage inatteignable, garde `version_date` testée sans réseau (E9). §8.3 : les 61 champs retirés de chaque liste (`SearchResultDto`, `SearchSectionDto`, `SearchExtractDto`, `ConsultTextDto`, `TocSectionDto`, `TocArticleDto`, `ArticleDto`, `ArticleContextDto`, `ContextEntryDto`, `JuriTextDto`, `SommaireDto`, `AccordDto`, `LibelleDto`), lignes `LienArticleDto`, `TexteLienDto`, `DecisionAttaqueeDto` supprimées, phrase sur la mesure (E1) et sur M7 ; listes `opt_nonempty` et « Types livrés différents » réécrites. §8.4 : lignes `isMultipleVersions`, `liens[]`, `lstLienCitation` (non déclarés). §8.5 : plafond 62 ou 63 (E7), point 1 (`a_code_sized_toc_stays_far_under_the_budget` : 20 000 éléments, 10 %), point 3 (`textAbroge` retiré), point 4 (sans objet), point 5 (borne vraie depuis le plan 2g, tests du permis). §9.1 (c) et §9.3 (`upstream_suffix_stripped`) : identifiants de la TOC non comptés (D11). §9.2, §9.3 : branchement livré (`fetch`), `search_page` en `cfg(test)`, `textAbroge` non déclaré, champs ACCO retirés, `record_mapping` sur `Err` (test du port). §12.3 : intégration livrée. §13.2 « Endpoints » : livrés dans `corpus/port.rs`, lignes ajoutées pour les tests des Tasks 5 à 7 (noms exacts), `user_agent_contains_crate_version` livré sous le nom `api_request_carries_bearer_accept_and_user_agent`. §13.3 : `mapping_snapshots`, `mapping_matches_golden`, intégrité (`integrity.rs`), `fixture_mutations_never_panic` (10 763) livrés, `multiple_versions_alias` retiré de la ligne des helpers. §14.4 : ligne « Requêtes en vol ». §15 (critères de M2) : branchement de `search` livré.
- [ ] **Step 2 : 00.** §11.1 : ligne `tests/fixtures_contract.rs` (livré au plan 2g, `golden.rs`, `integrity.rs`, `map/mutations.rs`) ; ligne du permis **résorbée** (D1, D2, tests) ; trois lignes nouvelles : champs de DTO non lus (D7, méthode E1), suffixe de la TOC (D11), niveau 1 du golden (D12). §14 : paragraphe « Plan 2g … — **Faits au plan 2g** » (huit points : port, permis, `fetch`, DTO, wiremock, golden et intégrité, `surface` et fuzz, TOC) avant « §14.1 ».
- [ ] **Step 3 : 07.** État (l. 9) : plan 2g ajouté à la liste. T-M1-04 et T-M2-05 : branchement livré. T-M2-06 : `fixture_mutations_never_panic` livré. T-M2-07 : branché (`PisteCorpus::fetch`). T-M2-08 : enveloppes 2xx `null` livrées par le port. **T-M2-09** : « Livrée au plan 2g », critère **(4)** du permis tenu jusqu'à la fin de `decode` puis de `map::*` (exigence de la clôture du plan 2f, tests nommés) ; Où : `corpus.rs`, `corpus/port.rs`, `request/consult.rs` (au lieu de `tests/endpoints.rs`). T-M2-10 : livrée (tests et job), Où : `map/mutations.rs`, `integrity.rs`, `src/snapshots/`. Découpage (l. ~396) : « 2g (livré) ». Tableau M2 (l. ~1095). Cases non cochées : le propriétaire tranche.
- [ ] **Step 4 : 06.** §2.3 : phrase d'introduction (le plan 2g ajoute `endpoints`, la fin de `fixtures_contract` et `parity_golden`), lignes `endpoints` (34 tests), `fixtures_contract`, `parity_golden` (8 tests, onze divergences au niveau 1). §3.2 : `returned_id_kept` « créée au plan 2g ». §7.1 : ligne `surface` (livrée pour le golden, `setup-python` 7.0.0). §12.1 : la ligne « Mémoire résidente en charge » est valable depuis le plan 2g.
- [ ] **Step 5 : 03, 02, 09, annexe A.** 03 l. 4 (branché au plan 2g) et §2.4 (identifiants de la TOC non comptés, D11). 02 l. ~286 (`corpus/port.rs`), l. ~404 et ~566 (`insta` entre, `proptest` non). 09 l. ~369 : `search` n'instrumente ni ne journalise la requête. Annexe A l. ~131 (`mapping_matches_golden` livré), l. ~529 (`ReturnedIdKept` créée), §5.3 (`include_bytes!`, jamais ignoré, `donnees/` en `-text`), §5.4 (harnais livré : écarts à l'esquisse, D12, témoin négatif).
- [ ] **Step 6 : vérifier.** `grep -n "à créer au plan 2g\|exigence du plan 2g\|reste : .impl LegalCorpus" docs/plan/*.md docs/plan/annexes/*.md` ne rend plus que l'historique des journaux ; `make fmt-check lint doc` ; `git diff --stat` limité aux documents.
- [ ] **Step 7 : commit** `docs: impl LegalCorpus, permis tenu pendant le mapping, DTO réduits, golden et intégrité (plan 2g)`.

### Task 17 : mutation finale, revue de branche et PR

Une seule campagne de mutation pour tout le plan (aucune tâche précédente n'en lance). cargo-mutants ne mute ni les `fn new` ni les `impl Default`, ni le code `#[cfg(test)]` (`golden.rs`, `integrity.rs`, `map/mutations.rs`, `view.rs`, tests de `port.rs`), ni `fuzz/**`.

**Files:**
- Modify (seulement si un mutant survit) : le module de test de la tâche qui possède le code ; `.cargo/mutants.toml` seulement pour un mutant **prouvé équivalent**, raisonnement en commentaire.

- [ ] **Step 1 : préalables.** `make mem-check` (≥ 8 Gio disponibles) ; aucune autre compilation en cours ; `make cov` vert.
- [ ] **Step 2 : liste.** `git diff --name-only main...HEAD -- '*.rs'`. Fichiers de production à muter, un à un, avec le nombre de mutants que liste `cargo mutants --list --file` à l'essai (viables et non viables) : `crates/lf-adapter-piste/src/corpus.rs` (28), `src/corpus/port.rs` (8), `src/request/consult.rs` (5), `src/classify.rs` (45), `src/limit.rs` (7), `src/map/search.rs` (28), `src/map/mod.rs` (18), et les DTO, dont des tests ont été retirés ou réécrits ailleurs : `src/dto/de.rs` (76), `src/dto/search.rs` (6), `src/dto/oauth.rs` (7), `src/dto/error.rs` (1) ; soit 229 ; `dto/{acco,article,consult_text,juri,mod}.rs`, `lib.rs` et `request/mod.rs` n'en ont aucun.
- [ ] **Step 3 : mutation, un fichier à la fois, jamais en parallèle** : `make mutants FILE=<fichier>` (la cible pose `TMPDIR=/var/tmp`, `-j 1`, `NEXTEST_TEST_THREADS=2`) ; au ledger : « N mutants tested : caught, unviable, missed, timeout » (forme singulière « 1 mutant tested » comprise) ; attendu **0 MISSED et 0 TIMEOUT** partout. Le test de la Task 13 (~10,6 s) et les tests de 16 Mio allongent chaque mutant : compter environ 40 s par mutant viable.
- [ ] **Step 4 : survivants.** Ajouter le test qui tue le mutant dans la tâche propriétaire (commit `test(adapter): mutants survivants du plan 2g`), relancer `make mutants FILE=<fichier>`.
- [ ] **Step 5 : budget du job `mutants-diff`.** `git diff main...HEAD > /var/tmp/m2g.diff` ; `TMPDIR=/var/tmp cargo mutants --list --in-diff /var/tmp/m2g.diff | wc -l` (essai : 24) : bien sous 450, **une seule PR** (D17).
- [ ] **Step 6 : revue de branche.** Agent `code-reviewer` (Sonnet) sur toute la branche, puis `security-auditor` (Opus) : permis et mémoire (Review Focus 1, 5), cible des requêtes et URL des réponses (2, 3), valeurs du corps dans les erreurs et journaux (4), retrait des champs de DTO (tolérance, aucun champ lu retiré). Corriger ce qui est retenu (une vague de clôture), consigner le reste au ledger.
- [ ] **Step 7 : clôture.** `make -k ci` (seul `deny` toléré, RUSTSEC-2026-0386) ; `cargo +1.88 check --workspace --all-targets --locked` ; `make min-age` ; `make arch` ; essai `dto_decode` de 60 s refait si un `dto/*.rs` a bougé depuis la Task 15 ; deux passes `NEXTEST_PROFILE=ci cargo nextest run --workspace --all-features --locked -j 2` (flake `auth.rs:906-916` : le signaler seulement s'il se reproduit). Rien n'est poussé : le contrôleur remet au propriétaire la PR unique, son compte de mutants et les décisions [P].

---

## Hors périmètre

- **Plan 2h** : `cargo xtask record-fixture` (T-M2-11, sidecars complets : leur lecture par `toml` est prête, D9), `recorded_fixtures_have_no_mojibake`, tests live et `live.yml` (T-M2-12), sondes V-66, V-67 et V-68 (forme des 404 et classification « introuvable » définitive), V-76, V-96, V-75 (plafond de corps par endpoint, D4 du plan 2f), V-82, V-83, V-100, V-104 (suffixes hors `LEGITEXT`, corps bruts, D11), V-105, V-109, V-112, V-140, V-142 ; compte PISTE sandbox.
- **PR séparée `security-fix`** : montée de `tokio` 1.53.3 (RUSTSEC-2026-0386), éligible le 2026-10-27 ; `fuzz/Cargo.lock` suit.
- **Plan 2b** : le flake `auth.rs:906-916` (borne murale de 1 s) ; non reproduit pendant l'essai (E13).
- **M3 et au-delà** : gabarit distinct de `InvalidRequest { status: None }` et message `not_found` par identifiant (`lf-application::messages`, D6) ; niveau 2 du golden (`tool_output_matches_golden`, les huit divergences de `TOOL_OUTPUT_ONLY`, M5) ; les 19 `ported_*` (M5) ; mesure de `perf_upstream_overhead` et réexamen de `spawn_blocking` (D2, M5) ; DTO de M6 et M7 (redéclaration de `versionPrecedente`, `citationJp`, `liens`, `decisionAttaquee`, `lstLien*`, `titrages`) ; compteurs `retries` et `rate_limited_waits`.
- **Hors plan** : ajout du check `golden de parité` aux protections de `main` (propriétaire, D15) ; ménage des worktrees `/var/tmp/m2g-dry` et des branches `dryrun/m2g`, `plan/m2g-corpus` (décision du propriétaire).
