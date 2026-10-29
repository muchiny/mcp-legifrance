# Fin du projet en parallèle : plan d'exécution (M3b à M9)

> **Ce plan remplace**, pour toute la suite du projet, les plans 3b et 3c (annoncés, jamais écrits : `2026-10-28-m3a-socle-application.md:5` et `:201-204`) et la section 3 de `docs/plan/07-feuille-de-route.md` (« Ordre conseillé et parallélisation », l.1012-1065). La section 2 de 07 reste la référence des tâches `T-*` ; l'annexe A indique l'unité qui fait chacune, ou son sort (allégée, reportée, propriétaire).
>
> **Révision 2 (2026-10-29).** Elle intègre les deux relectures de la révision 1, soit 4 constats critiques (2 en double), 20 importants et 7 mineurs. Tous les critiques et importants sont corrigés. Le §10 justifie chaque cas où le correctif retenu diffère de celui proposé.
>
> **Pour les agents.** L'exécution passe par des workflows ultracode, un par vague (§2.5). Chaque unité suit `superpowers:test-driven-development` dans le worktree que lui crée le harnais. L'intégration se fait sur une seule branche, `feat/fin-du-projet`, et aboutit à une seule PR vers `main`.

**Goal :** livrer en une branche et une PR le serveur MCP Légifrance. La v0.1 (M3 à M5 et la distribution) doit être verte d'ici la fin du 2026-10-30. M7, M6, M9 et M8 suivent, selon les points de décision du §8.

**Architecture :**

- **Vague 0, séquentielle.** Elle déclare les crates et toutes leurs dépendances, normales comme de développement, ainsi que les règles d'architecture. Elle déclare aussi tous les modules à venir, et gèle sous forme de stubs qui compilent les seules interfaces que deux unités consomment.
- **Quatre vagues parallèles**, de 4 à 16 unités chacune. Chaque unité possède ses fichiers en exclusivité et travaille dans le worktree et sur la branche que crée le harnais. L'intégrateur fusionne ensuite les unités par squash dans `feat/fin-du-projet`.
- **Tranches verticales.** Pour un outil, le cas d'usage, la méthode de façade, la sortie, les paramètres, la vue et le routeur forment une seule unité.
- **Compilation bornée.** Au plus 4 commandes cargo tournent en même temps (jetons `flock`), quel que soit le nombre d'agents.

**Tech Stack :**

- Langage : Rust 1.99.0, MSRV 1.88, édition 2024, resolver 3.
- Dépendances :
  - MCP : rmcp et rmcp-macros `=3.5.1`, schemars 1.2.
  - Asynchrone : tokio 1.53, tokio-util 0.7, futures 0.3.
  - Cache, empreintes et verrous : moka 0.12, sha2 0.11, hex 0.4, fs4 1.1 (fs4 0.13 est tiré par tantivy).
  - Ligne de commande et erreurs : clap 4.6, thiserror 2, anyhow 1.
  - Parseur de citations : winnow 1.0, aho-corasick 1.1, regex 1.13, unicode-normalization 0.1.
  - Diff : similar 2.7.
  - Index local (M9) : tantivy 0.26.
  - Tests : tempfile 3.
- Outils : cargo-nextest, cargo-llvm-cov, cargo-mutants 27.1.0, cargo-deny, cargo-machete, dist 0.33.0.

**Spec :** `docs/plan/00-decisions.md`, `02-architecture.md`, `03-domaine.md`, `04-adapter-piste.md`, `05-outils-mcp.md`, `06-qualite.md`, `07-feuille-de-route.md` §2, `08-distribution.md`, `09-risques-et-questions.md`, annexes A à D, `docs/adr/*`.

## Global Constraints

- Chaîne 1.99.0, MSRV 1.88 (job `msrv`), `edition = "2024"`, `resolver = "3"` (`Cargo.toml:2`).
- `rmcp` et `rmcp-macros` restent épinglés à `=3.5.1`, sans renommage (`xtask/src/arch.rs:155`).
- Règle d'âge : tout paquet du registre ajouté ou monté dans un verrou a au moins 14 jours, **transitifs compris** (`.github/scripts/check-min-age.sh:4-8`). L'exigence est écrite au niveau mineur et le correctif est fixé par `cargo update -p … --precise` (02 l.364-366).
- Tout passe en `--locked`. `Cargo.lock` et `fuzz/Cargo.lock` ne changent qu'en 0B, au préambule de V4, ou par une `shared_request` appliquée par l'intégrateur.
- Règles R1 à R10 contrôlées par `cargo xtask check-arch`.
- Lints du workspace : `unsafe_code = "forbid"`, clippy `pedantic`, `todo = "warn"`, `-D warnings` (`Cargo.toml`, `[workspace.lints]`).
- Couverture : 100 % en lignes, régions et fonctions, mesurée par `cargo llvm-cov --workspace --lib` (`Makefile:11-12`) et bloquante à la PR finale. Cette mesure **ne compte que les `#[cfg(test)] mod tests`** (06-qualite.md:86, :130), d'où la règle T du §2.4.
- Plafond de réponse : 45 000 caractères de JSON compact. Estimation conservatrice : somme des longueurs plus 64 par champ, cible 42 000 (02 §6.1 ; 05 l.1273-1274).
- Instructions du serveur : 3 500 caractères au plus. `title` et descriptions en anglais, prompts en français (Q14).
- Audit strict par défaut (Q9) : chaque issue d'un appel donne exactement une ligne d'audit. stdout ne porte que du JSON-RPC. Aucun secret ni aucun texte de requête dans un journal, un message ou l'audit.
- WSL (`~/.claude/rules/wsl-safety.md`) :
  - au plus 4 commandes cargo à la fois (D19) ;
  - jamais `cargo test -j 4` ou plus ;
  - `cargo mutants` seulement avec `TMPDIR=/var/tmp … -j 1 --file` ;
  - aucun `--release` ni `--profile dist` en local.

## Review Focus

Les classes d'entrées ci-dessous n'ont aucun test hérité, et le spec ne les mentionne pas. Chacune reçoit ses tests, placés dans l'unité qui écrit le code concerné. Un nom de test sans chemin désigne un `mod tests` (règle T).

1. **Entiers mal formés** : `10.0`, `"10"`, `null`, ou hors des bornes d'un `i64`. Attendu : décodage tolérant, `null` lu comme absent, message de bornes de parité. Tests (V1-09) :
   - `lenient_int_accepts_integer_float_and_decimal_string` ;
   - `null_is_absent_for_optional_params` ;
   - `huge_integer_is_out_of_range_not_panic`.
2. **Lecture au bord du texte** : `offset` égal ou supérieur à `text_length`, texte vide, `detail="summary"` avec `offset > 0`. Attendu : `[invalid_arg]` sans réseau, et une `note` sur un texte vide. Tests (V1-06) :
   - `offset_past_end_is_invalid_arg` ;
   - `empty_text_offset_zero_gives_note` ;
   - `detail_summary_with_offset_is_invalid_arg`.
3. **Plafond de réponse** : fenêtres riches en échappements JSON (`"`, `\`, U+0000 à U+001F) et en caractères hors BMP. Attendu : la taille mesurée reste sous 45 000 caractères. Tests :
   - `fit_degrades_until_under_cap` (0W) ;
   - `every_tool_response_under_hard_cap`, avec un cas « échappements et emoji » (V3-03, dans `tests/`).
4. **Journal d'audit en panne** : dossier non inscriptible, deux processus, Windows. Attendu en mode strict : `[audit_error]` sans résultat, et aucune ligne coupée. Tests :
   - `unwritable_dir_is_an_error_not_a_panic` et `no_torn_lines_under_concurrent_writers` (V1-07) ;
   - `audit_unwritable_dir_fails_call` et `two_processes_share_audit_file` (V3-01, sur le binaire).
5. **Cycle de vie stdio** : notification ou réponse avant `initialize`, EOF pendant un appel. Attendu : le serveur continue, puis se termine avec le code 0 en moins d'une seconde. Tests :
   - `premature_notification_does_not_stop_server` (V1-10, `lifecycle.rs`) ;
   - `eof_with_inflight_call_exits_zero_within_1s` (V2-06 en processus, V3-01 sur le binaire).

---

## 1. Décisions (avec leurs coûts)

| # | Décision | Coût | Gain |
|---|---|---|---|
| D1 | **Une seule branche, une seule PR.** Tout se fait sur `feat/fin-du-projet` (worktree `~/mcp-ligitfrance-m3a-exec`). La PR unique vers `main` est ouverte **en brouillon après V1**. Aucune autre branche durable, aucune PR intermédiaire. | La revue humaine porte sur un diff de 15 à 25 000 lignes. | Aucun rebase entre jalons, et la CI tourne sur trois OS dès V1. |
| D2 | **Branches éphémères = celles du harnais.** Une unité travaille sur la branche que lui crée `isolation: 'worktree'` ; il n'y a jamais de branche `wip/*`. La fusion se fait par squash, un commit par unité : `<type>(<scope>): <titre> [V<N>-<id>]`. La branche et le worktree de l'unité sont supprimés juste après sa fusion (§5). | Les commits intermédiaires d'une unité sont perdus. | Le propriétaire ne voit qu'une branche, et couper un jalon revient à `git revert` de commits nommés. |
| D3 | **Gel minimal en V0.** On ne gèle que : le port `LegalCorpus` complet (12 méthodes, car en ajouter plus tard casserait `PisteCorpus` `corpus/port.rs:43`, `FakeCorpus` `testing.rs:250`, `Named` `ports/corpus.rs:216` et `CachedCorpus`), les types que deux unités d'une même vague consomment, et **la déclaration de tous les modules à venir** (fichiers réduits à leur doc). Ainsi, aucune unité ne touche un fichier de déclaration. | V0 dure 5 à 7 h. | 16 unités par vague, lancées sans attente et sans conflit. |
| D4 | **Tranches verticales.** Une tranche `T(x)` (§4) écrit `usecase/x.rs`, `output/x.rs` et `service/x.rs` (un `impl LegalService` par outil) dans `lf-application`, et `params/x.rs`, `views/x.rs` et `tools/x.rs` dans `lf-adapter-mcp`. Chaque tranche construit son cas d'usage comme il lui convient : `CoverageUseCase` lit `ServiceSettings`, `VerifyCitationsUseCase` lit `settings.policy.env`. M5 (lecture, pagination, budgets) est fondu dans les tranches. | — | Un outil s'éprouve de bout en bout, en processus, dans son unité. Les cas d'usage n'ont plus de forme uniforme imposée. |
| D5 | **`find_article(&self, text: &LegiTextId, num: &str, at: ArticleAt)`**, avec `pub enum ArticleAt { Today(VersionDate), On(VersionDate) }` dans `lf_application::ports`. C'est le signal « aujourd'hui » prévu par 02 l.855-857. L'application résout `CodeRef` en `LegiTextId` par `list_codes()`, et choisit `Today` quand la date vaut `clock.today_paris()`. | Amender 02 §5.1, 03 §4.11 et l'annexe B §6.7 en F6. | Le port se gèle en V0 sans les types de citation. L'adapter, qui n'a pas d'horloge, peut quand même choisir `getArticleWithIdAndNum`. |
| D6 | **Couverture `--lib` à 100 % maintenue** (ADR-0002, 06 §7.6). Les tests qui portent la couverture vivent dans des `#[cfg(test)] mod tests` (règle T). Une seule exclusion nommée dans `COV_FLAGS` : `crates/lf-adapter-mcp/src/stdio.rs`, réduit à la colle `serve_stdio` sur les vrais stdin et stdout et couvert par l'e2e du binaire ; la logique du cycle de vie vit dans `lifecycle.rs`, qui est mesuré. `mcp/testing.rs` et `app/contract.rs` sont couverts par les tests de leur propre crate. `lf-app` est un binaire seul (`arch.rs:240-241`), donc hors `--lib`. `make cov` tourne à titre informatif aux intégrations de V2 et V3. | Rien de plus si la règle T est suivie dès V1. Les `make cov` informatifs prennent environ 20 min chacun, en parallèle de la revue. Le balayage F3 est estimé à 1 à 2 h. | La PR peut être verte sans baisser le seuil. Refusé : retirer `--lib` (§10). |
| D7 | **Job CI `mutants`** (aujourd'hui `ci.yml:182-219` : 60 min, `--in-diff` sur tout le diff). La matrice `mutants-shard` passe en 4 shards (`--shard k/4`, k de 0 à 3), avec 120 min par shard et `-j 2`. Ses mutants se limitent aux fichiers à risque de `.github/mutants-scope.txt`, un `--file` par ligne, filtre que `--in-diff` vient encore restreindre. Le job agrégateur `mutants`, qui `needs` la matrice, garde le nom actuel `mutants (diff de la PR)` : la protection de branche n'a pas à changer. Une étape échoue si une ligne de la liste ne désigne aucun fichier. | Les fichiers hors liste ne sont pas mutés avant la fusion. Le premier `weekly.yml` après la fusion signalera des `MISSED` sur ces fichiers : c'est une dette, traitée après la PR. | La PR passe : environ 600 à 900 mutants attendus, soit 25 à 75 min par shard. Muter tout le diff dépasserait 24 h. |
| D8 | Aucune mutation locale pendant les vagues. En fin de projet, `make mutants FILE=…` seulement sur les fichiers où la CI signale un `MISSED`. | — | Plusieurs heures de mutation sérialisée (`-j 1`) économisées. |
| D9 | Toutes les dépendances sont déclarées en 0B, sauf celles de M8 (axum, transport HTTP de rmcp, tower), qui arrivent au préambule de V4 car M8 est abandonnable. | tantivy se compile dans la base chaude et en CI. | Les verrous ne bougent plus de V1 à V3. |
| D10 | `similar` en **2.7**, déjà au verrou via insta (`Cargo.lock`, `similar 2.7.0`), au lieu de 3.2.0 (02 §3.0.1). | — | Pas de doublon. |
| D11 | `Coverage` : la forme livrée est gardée (`coverage.rs:84`, constructeur faillible `:97`), et on y ajoute la fonction pure `coverage(env, &ConfigFacts, served)`. La forme cible de 03 §4.8 est abandonnée, ce qui tranche M1-D20. | — | Pas de refonte. |
| D12 | Allègements de T-M5-14 : ni `MaxChars`, ni `PassageCount`, ni `TocLimit` (constantes de `ResponseBudget`). La `window` à niveaux est reportée : la moitié comptée en octets (`window.rs:19-21`) est gardée, sauf si `paged_reading_reconstructs_full_text` échoue. Reports consignés en F6 : M1-D1 à M1-D5 et M1-D9. | Écart à 03 §7.6. | Environ 2 h. |
| D13 | **`ServiceSettings { policy: AuditPolicy, configuration: ConfigurationState, budget: ResponseBudget }`** remplace les champs séparés de 02 l.1093 : `AuditPolicy` porte déjà `mode`, `deadline`, `env` et `server_version` (`audited.rs:74-84`). `service/mod.rs` porte aussi le champ `complete_gate: CompleteGate`, qui tient le débit de 10 complétions par seconde et par processus (02 l.1080). | Amender 02 §6.3. | Aucune donnée en double, et l'état par processus a sa place. |
| D14 | Aucune release, aucune étiquette poussée par un agent. La PR finale porte la version du dernier jalon complet (`0.1.0`, `0.2.0` ou `0.3.0`, jamais `1.0.0` sans le propriétaire) ; `publish = false` est gardé (Q13). | — | Aucune action irréversible. |
| D15 | Une revue groupée par vague (agent `code-reviewer`, Sonnet) : seuls les constats critiques et importants sont corrigés. Une seule revue sécurité (agent `security-auditor`, Opus), en fin de projet. | Moins d'yeux par unité. | Environ 30 min par vague. |
| D16 | Aucune modification de `docs/plan/*`, `CHANGELOG.md` ou `docs/adr/*` pendant les vagues, sauf ADR-0018 en 0B et le rapport de V1-16. Une seule passe documentaire, en F6. | Les docs divergent du code pendant la campagne. | Aucun conflit. |
| D17 | **Sondes sandbox**, lancées par l'intégrateur seul, qui lit les identifiants dans `~/.config/mcp-legifrance/piste.env`. Ils ne sont jamais copiés dans un prompt, un commit ou un journal, et au plus une requête par seconde part. Deux moments : (1) l'**étape I-R** de l'intégration de V1 enregistre les fixtures des points d'accès M6 et M7 avant V2 (§5) ; (2) en **F2**, les sondes live de M5, de M6, de M7 et de l'e2e. | Les constats live arrivent en fin de projet. | Les DTO de M6 et M7 sont écrits sur des corps réels, et aucun agent de vague ne voit un identifiant. |
| D18 | **Stubs.** Un stub `todo!()` porte `#[expect(clippy::todo, reason = "STUB(V0) → <unité>")]`, plus `clippy::unused_async` s'il s'agit d'un `async fn` inhérent. Un stub de méthode de trait rend `std::future::ready(Err(…)).await`, avec le commentaire `// STUB(V0) → <unité>`. Un stub fonctionnel porte ce même commentaire. Les paramètres inutilisés sont préfixés par `_`. Jamais d'`expect` sur un lint incertain (`dead_code`, `unused_variables`) : dans ce cas, `#[allow(…, reason = "STUB(V0) → …")]`. Contrôle en F1 : `! grep -rn "STUB(V0)" crates xtask`. | — | Aucun stub oublié, et aucune attente non remplie en V0. |
| D19 | **WSL : 4 jetons de compilation.** `CARGO_BUILD_JOBS=4`, `NEXTEST_TEST_THREADS=2`, et la mémoire disponible (au moins 8 Gio, comme `MEM_CHECK`, `Makefile:17`) se vérifie **après** la prise du jeton. Jusqu'à 16 agents écrivent du code en même temps, mais 4 commandes cargo au plus tournent (§2.2). Les unités compilent exactement le graphe de la base chaude : `--workspace --all-features --locked`. | Les unités attendent parfois un jeton ; compter 30 à 60 min de plus par vague. | Au plus 16 rustc à la fois sur 24 Go, et aucune course entre plusieurs contrôles de mémoire. |
| D20 | **Boîte de temps** : 4 h par vague pour les unités. Passé ce délai, une unité rend `blocked: true, reason: "timebox"` avec ce qu'elle a commité. Elle est reprise à la vague suivante depuis sa branche, qui est alors rebasée, ou coupée selon le §8. | Une unité lente déborde sur la vague suivante. | La durée murale reste bornée. |
| D21 | **fs4 en deux versions** : tantivy 0.26.2 tire `fs4 ^0.13.1` par sa feature par défaut `mmap` (relevé dans l'index crates.io, sparse `ta/nt/tantivy`). Dans `deny.toml`, `tantivy` figure parmi les `wrappers` de `fs4`, et F1 ajoute un `skip` justifié pour `fs4@0.13.1`. `mmap` est gardé, car l'index sur disque de M9 l'exige. | Un doublon toléré. | `cargo deny check bans` reste vert. |
| D22 | **V-165 (ACL Windows) mesurée, non affirmée** : test `#[ignore]` `windows_acl_report`. Défaut : ACL héritées, documentées comme non vérifiées dans `PRIVACY.md`. | V-165 reste ouverte (§7). | Aucun job Windows rouge sans correctif possible (`unsafe_code = "forbid"`). |

### Questions ouvertes : défaut appliqué

Q1, Q2 et Q16 sont déjà décidées (00 §13). Pour toute autre question ouverte (05 §13.2, 03 §11, 09 §3), la proposition par défaut du document qui la porte s'applique telle quelle.

| Question | Défaut appliqué |
|---|---|
| Q3, PISTE production | Recette en sandbox seulement ; les sondes de production sont reportées à la release et reviennent au propriétaire. |
| Q4, HTTP et CGU | Profil local seul (`127.0.0.1`, jeton statique) ; T-M8-06 (profil distant) n'est pas fait. |
| Q5, signature | Aucun certificat : binaires et MCPB non signés, avec un avertissement documenté ; ni T-MD-08 ni T-MD-09. |
| Q7 | Versions legacy 2025-11-25 et 2025-06-18 ; 2025-03-26 est exclue. |
| Q8 | `ttlMs = 3 600 000` et `cacheScope = public`, pour les pairs modernes seulement. |
| Q9 | `LF_AUDIT_MODE=strict` par défaut, posé par `lf-app` (`AuditMode` n'a pas de `Default`, `audited.rs:63`). |
| Q10 | `match` vaut `any` par défaut (parité). |
| Q11 | `openWorldHint = true`. |
| Q12 | Formes de 00 §4 telles qu'adoptées. |
| Q13 | `cargo install --git … --tag vX.Y.Z --locked mcp-legifrance` documenté ; pas de crates.io. |
| Q14 | Descriptions en anglais, prompts en français. |
| Q15 | Attribution et `consulted_on` dans `fr_coverage`, avec un rappel dans le README. |
| Q17 | URL `/codes/article_lc/` gardée, vérifiée par V3-11. |
| T-M7-01 | Ni surface JORF, ni `CodeId`, ni outil de navigation de code, ni `resource_link`. |
| V-163 | Les cas « de mémoire » sont exclus des tests live et marqués « non garantis ». |
| V-165 | ACL héritées (D22). |
| M9 | LEGI seul ; dumps téléchargés par script, hors binaire ; feature `dila` absente des binaires publiés. |
| T-M9-05 | Pas de crates.io, pas de cache disque, rétention de l'audit selon 09 §3.16.3. |

## 2. Réglages d'exécution

### 2.1 Lancement

La session orchestratrice démarre avec pour répertoire courant `/home/muchini/mcp-ligitfrance-m3a-exec`, sur `feat/fin-du-projet`. Sinon, `isolation: 'worktree'` créerait des worktrees d'un autre dépôt : le répertoire de mémoire est lui aussi un dépôt git.

Au pré-vol, chaque unité :

1. vérifie que `git remote get-url origin` contient `muchiny/mcp-legifrance` ;
2. lance `git reset --hard <BASE>` ;
3. amorce son répertoire de build (§2.2).

Les appels Bash ne gardent pas l'environnement d'un appel à l'autre. Chaque commande cargo passe donc `CARGO_TARGET_DIR` et appelle `/var/tmp/lf-fin/lf-cargo`, qui source lui-même `env.sh`. Pour ces appels, poser `timeout: 600000`.

### 2.2 Mémoire et compilation (WSL, D19)

Hors dépôt, l'intégrateur crée en 0B trois fichiers dans `/var/tmp/lf-fin/`.

**`env.sh`** :

- `TMPDIR=/var/tmp`, `CARGO_BUILD_JOBS=4`, `NEXTEST_TEST_THREADS=2`, `RUST_TEST_THREADS=2` ;
- `CARGO_PROFILE_DEV_DEBUG=line-tables-only` et `CARGO_PROFILE_TEST_DEBUG=line-tables-only`, identiques pour la base chaude, les unités et l'intégrateur (sinon les empreintes cargo diffèrent et tout se recompile) ;
- `lf_mem_ok` : vrai si `MemAvailable` vaut au moins 8 388 608 kio, soit 8 Gio.

**`lf-cargo <arguments cargo>`**, script exécutable :

1. Il essaie un à un les jetons `/var/tmp/lf-fin/slot.{1..4}` par `flock -n`.
2. Une fois le jeton pris, il vérifie `lf_mem_ok`. Si la vérification réussit, il lance `cargo "$@"` sous le jeton et rend son code de sortie.
3. Sinon, ou si aucun jeton n'est libre, il relâche le jeton, dort 20 s dans le script et recommence.
4. Il abandonne avec le code 75 après 30 min, et l'agent relance la commande.

**`lf-all <commande>`** prend les quatre jetons (`flock slot.1 flock slot.2 flock slot.3 flock slot.4 env -u CARGO_BUILD_JOBS "$@"`). L'intégrateur l'utilise seul, avec le parallélisme cargo par défaut, comme `make ci` aujourd'hui.

**Base chaude.** À la fin de chaque intégration, `target/` du worktree principal (bâti par `make lint test` avec `env.sh`) est copié par `cp -a --reflink=auto target/. /var/tmp/lf-fin/base/`. En V0, la base est bâtie par `lf-all cargo nextest run --workspace --all-features --locked --no-run`, puis par `lf-all make lint`.

**Amorçage d'une unité.** `cp -a --reflink=auto /var/tmp/lf-fin/base/. /var/tmp/lf-fin/t-<id>/`. Les dépendances du registre sont reprises ; seuls les crates du workspace se recompilent, car leur chemin diffère d'un worktree à l'autre. Comptez 4 à 8 Go par unité, soit jusqu'à environ 130 Go par vague (683 Go libres sur `/`). Les répertoires `t-*` sont supprimés à chaque intégration.

**Incident.**

- Au premier OOM, ou si une attente de mémoire se répète, retirer le jeton 4 du script (3 jetons).
- Ne jamais relancer la même commande après un OOM (règle WSL).
- Le nombre d'agents (`maxPar`, 16 au plus) ne change pas : un agent qui attend un jeton continue d'écrire du code.

### 2.3 Contrôles d'unité

Une unité est finie quand ces commandes passent. `<ids>` est l'expression de filtre nextest des paquets que l'unité touche ; le paquet de `lf-app` s'appelle `mcp-legifrance`.

```bash
cargo fmt --all -- --check                    # sans compilation, hors jeton
CARGO_TARGET_DIR=/var/tmp/lf-fin/t-<id> /var/tmp/lf-fin/lf-cargo nextest run --workspace --all-features --locked -E '<ids>'
CARGO_TARGET_DIR=/var/tmp/lf-fin/t-<id> /var/tmp/lf-fin/lf-cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

S'y ajoutent `lf-cargo xtask check-arch` si l'unité touche `xtask/` ou un manifeste, et la vérification du périmètre : `git diff --name-only <BASE>..HEAD` ne sort aucun fichier hors de la liste de l'unité. Itérer avec `nextest`, et ne lancer `clippy` qu'à la fin.

### 2.4 Règles des unités

- **Propriété.** Une unité ne touche que ses « Fichiers exclusifs », plus les snapshots de leurs modules (règle S).
- **Fichiers interdits** sauf mention dans la ligne de l'unité : tout `Cargo.toml`, `Cargo.lock`, `fuzz/Cargo.lock`, tout `lib.rs` ou `mod.rs`, `CHANGELOG.md`, `tests/tools.lock.json`, `docs/**`, `Makefile`, `deny.toml`, `clippy.toml`, `.cargo/**` et `.github/**`.
- **Règle T (tests).** Un test qui porte la couverture vit dans un `#[cfg(test)] mod tests` d'un fichier `src/` que l'unité possède. Un nom de test sans chemin dans un tableau désigne ce module. `tests/` n'est employé que là où le tableau le dit : e2e du binaire, live, compléments (contrat, budgets, descripteurs, corpus), données. Ces tests ne comptent pas dans la couverture (06-qualite.md:130).
- **Règle S (snapshots).** `src/snapshots/<crate>__<module avec __>__*.snap` appartient au propriétaire du module. `tests/snapshots/<cible>__*.snap` appartient au propriétaire de la cible de test.
- **Visibilité.** Un item nouveau est `pub(crate)`. Un `pub use` à ajouter dans un fichier de déclaration que l'unité ne possède pas va dans `shared_requests`. Les items gelés en V0 sont déjà réexportés.
- **Gel.** Une signature gelée ne se change pas. Si elle se révèle fausse, l'unité rend `blocked: true` avec le motif et ne la contourne pas.
- **Dépendances.** Elles sont toutes déclarées en 0B. Une dépendance manquante va dans `shared_requests` : l'intégrateur l'ajoute avec verrou, `min-age` et `deny`. L'unité continue sans elle, ou rend `blocked`.
- **Interdits.** `cargo update`, `--release`, `git push`, toute création de branche, et la lecture de `~/.config/mcp-legifrance/piste.env`.
- **Commits.** L'unité commite sur sa branche de harnais, avec le titre de D2.

### 2.5 Script de vague (esquisse ; l'auteur du workflow l'écrit)

```js
export const meta = { name: 'fin-vague', description: 'Une vague du plan fin-du-projet',
  phases: [{ title: 'Unités' }, { title: 'Intégration' }, { title: 'Revue' }, { title: 'Corrections' }] }
// args = { wave, base, deadline, units: [{ id, title, model, allow, consumes, produces, tests, after }], maxPar: 16 }
// Unités : agent(prompt(u), { isolation: 'worktree', label: u.id, model: u.model, schema: UNIT_RESULT })
// Intégration : 1 agent sans isolation (worktree principal) : §5 étapes 1 à 4
// Revue : 1 agent agentType 'code-reviewer' sur git diff base..HEAD, PENDANT que l'intégrateur lance `make cov` (V2, V3)
// Corrections : 1 agent, constats critiques et importants seulement, liste blanche = fichiers cités
```

`UNIT_RESULT` vaut `{ id, branch, worktree, head, files_changed[], outside_allowlist[], checks: { fmt, clippy, tests }, stubs_removed[], shared_requests[], blocked, reason, notes }`. `branch` est `git branch --show-current` et `worktree` est `git rev-parse --show-toplevel`.

Le prompt d'une unité reprend : sa ligne de tableau, les §2.1 à §2.4, les sections de spec citées, l'échéance de D20, et pour une tranche le motif `#[tool]` retenu par 0A (commentaire de `mcp/tools/mod.rs`).

## 3. Vague 0 : socle et interfaces gelées (séquentielle, 5 à 7 h)

**Ordre, avec intégration au fil de l'eau.** Chaque étape part de la tête de `feat/fin-du-projet` après la fusion de l'étape précédente.

| Étape | Unités | Fusion, dans l'ordre |
|---|---|---|
| 1 | 0A ‖ 0B | squash de 0B, puis de 0A |
| 2 | 0C ‖ 0M | squash de 0C, puis de 0M |
| 3 | 0W | squash de 0W |
| 4 | 0D | contrôles et base chaude |

Avant l'étape 2, l'intégrateur lance `lf-all make lint test arch`, et avant l'étape 3 `lf-all make lint test`. L'agent intégrateur fait toutes les fusions de V0.

### 0A : spike rmcp (1 h au plus, Opus)

- **Fichiers** : `spikes/rmcp-stdio/{src/lib.rs, src/main.rs, tests/spike.rs, README.md, Cargo.lock}`. Le crate est hors workspace : son `Cargo.toml` a son propre `[workspace]` et résout déjà `rmcp =3.5.1` avec `transport-io`.
- **Ce qu'il éprouve, un test par point :**
  - un serveur générique `Spike<C: LegalCorpus>` ;
  - deux blocs `#[tool_router(router = …, vis = "pub(crate)", allow_empty)]` combinés par `+` (`allow_empty` : `rmcp-macros-3.5.1/src/tool_router.rs:21`) ;
  - `#[tool_handler(router = self.tool_router)]` sur l'`impl ServerHandler` (`ToolHandlerAttribute { router: Expr }`, `rmcp-macros-3.5.1/src/tool_handler.rs:10-12`) ;
  - un outil qui reçoit un `JsonObject` brut (extracteur `rmcp-3.5.1/src/handler/server/tool.rs:252-258`), avec `input_schema` et `output_schema` explicites ;
  - des arguments invalides qui atteignent l'outil, sans l'erreur « failed to deserialize parameters » de `Parameters<P>` (`tool.rs:235-247`) ;
  - une route ajoutée au routeur depuis un test, dont 0A relève l'API exacte de `ToolRouter`, pour V1-10 ;
  - le jeton d'annulation transmis comme futur `cancel` à `Audited::run` (`audited.rs:140`) ;
  - `PreLifecycleFilter` autour de `AsyncRwTransport::new_server(stdin, stdout)` (`async_rw.rs:95`), sous `forbid(unsafe_code)`.
- **Tests** :
  - `generic_server_combines_routers` ;
  - `tool_handler_dispatches_to_router_field` ;
  - `jsonobject_tool_receives_raw_arguments` ;
  - `invalid_arguments_reach_the_tool` ;
  - `test_route_can_be_added` ;
  - `tools_list_has_explicit_input_and_output_schema` ;
  - `cancellation_reaches_audited_run` ;
  - `notification_before_initialize_is_ignored` ;
  - `eof_exits_zero`.
- **Verdict**, dans `README.md` : V-28 et V-29 OK ou KO, avec le motif de code exact du `#[tool]` et les bornes génériques que copieront 0W et les tranches.
- **Replis**, choisis au bout d'une heure si le verdict n'est pas clair. Pour chacun, 0A écrit la variante de chaque gel concerné :
  - V-28 KO : `Parameters<P>` à champs `Option<serde_json::Value>`, sans `deny_unknown_fields`, décodés par `decode.rs`. Les tests de V1-09 sont ajustés (`unknown_field_message_lists_expected` devient un contrôle dans `decode`). L'écart à D-02 et à ADR-0008 est consigné en F6.
  - Génériques KO : routes construites à la main (`ToolRoute`).
  - V-29 KO : pas de filtre, et le risque R-02 est consigné.

### 0B : socle (Sonnet, 1,5 à 2 h)

**`Cargo.toml` racine.**

- `members` (`Cargo.toml:3`) : ajouter `crates/lf-adapter-cache`, `crates/lf-adapter-audit`, `crates/lf-adapter-mcp`, `crates/lf-app` et `crates/lf-adapter-dila`.
- `[workspace.dependencies]` :
  - internes en `"=0.0.0"` ;
  - externes au niveau mineur de 02 §3.0.1, sauf D10, avec `default-features = false` quand c'est possible (features minimales vérifiées par context7, règle de `CLAUDE.md`) : `rmcp "=3.5.1"`, `rmcp-macros "=3.5.1"`, `schemars 1.2`, `tokio-util 0.7`, `futures 0.3`, `moka 0.12`, `sha2 0.11`, `hex 0.4`, `fs4 1.1`, `clap 4.6`, `dirs 6.0`, `anyhow 1.0`, `jsonschema 0.58`, `winnow 1.0`, `aho-corasick 1.1`, `regex 1.13`, `unicode-normalization 0.1`, `similar 2.7`, `tantivy 0.26`, `quick-xml 0.42`, `tar 0.4`, `tempfile 3`.
- Lints clippy : `print_stdout`, `print_stderr` et `exit` en `warn` (T-M4-15). Poser `#[expect(…, reason)]` sur les sites existants : `xtask/src/main.rs:34`, `:52`, `:63`, `:75` ; `crates/lf-adapter-piste/examples/record_fixture.rs:121`, `:141`, `:142` ; `crates/lf-adapter-piste/tests/live.rs:42`, `:48`, `:320`.
- Profils : `[profile.release]` (`lto = "thin"`, `codegen-units = 1`, `panic = "unwind"`) et `[profile.dist]` (`inherits = "release"`) (08 l.133, l.224). Métadonnées de paquet de T-MD-02.

**Manifestes : toutes les dépendances, normales et de développement, déclarées une fois.** Celles qui ne servent pas encore vont dans `[package.metadata.cargo-machete] ignored`, avec le commentaire `STUB(V0)`. Chaque nouveau crate a `[lints] workspace = true` et, s'il est une bibliothèque, un `src/lib.rs` qui porte la doc du crate.

| Crate | `[dependencies]` | `[dev-dependencies]` | `[features]` |
|---|---|---|---|
| `lf-domain` | + `winnow`, `aho-corasick`, `regex`, `unicode-normalization` | inchangées (`proptest`, `pretty_assertions`, `serde_json`) | — |
| `lf-application` | + `futures` (`std`), `similar` (déjà admis par R2, `arch.rs:111-112`) | inchangées | `test-support` (existant) |
| `lf-adapter-piste` | inchangées | + `lf-application` avec `test-support` (suite de contrat de V2-05 et V3-12) | `record` (existant) |
| `lf-adapter-cache` (nouveau) | `lf-application`, `lf-domain`, `moka` (`future`), `tokio` (`sync`), `tracing`, `thiserror` | `lf-application` (`test-support`), `tokio` (`macros`, `rt`, `test-util`), `pretty_assertions` | — |
| `lf-adapter-audit` (nouveau) | `lf-application` (seul interne admis, `arch.rs:228`), `sha2`, `hex`, `fs4` (`sync`), `serde_json`, `chrono`, `tokio` (`rt`, `sync`), `tracing`, `thiserror` | `lf-application` (`test-support`), `tokio` (`macros`, `rt`, `rt-multi-thread`, `test-util`), `tempfile` | — |
| `lf-adapter-mcp` (nouveau) | `lf-application`, `lf-domain`, `rmcp` (défauts + `transport-io`), `rmcp-macros`, `schemars`, `serde`, `serde_json`, `serde_path_to_error`, `tokio` (`io-std`, `io-util`, `rt`, `sync`, `time`, `macros`), `tokio-util`, `tracing`, `thiserror` | `lf-application` (`test-support`), `insta` (`json`), `jsonschema`, `tokio` (`test-util`), `pretty_assertions` | `test-support = ["rmcp/client"]` (le harnais `testing.rs` est un client rmcp) |
| `mcp-legifrance` (`crates/lf-app`, nouveau) | tous les `lf-*`, dont `lf-adapter-dila` en optionnel ; `clap` (`derive`), `tracing`, `tracing-subscriber`, `anyhow`, `dirs`, `secrecy`, `chrono` (`clock`), `chrono-tz`, `tokio` (`rt-multi-thread`, `signal`, `io-std`, `macros`), `tokio-util` | `lf-adapter-mcp` (`test-support`), `lf-application` (`test-support`), `wiremock`, `serde_json`, `tempfile`, `tokio` (`process`, `io-util`, `time`, `test-util`). Pas d'`assert_cmd` : `env!("CARGO_BIN_EXE_mcp-legifrance")` suffit. | `dila = ["dep:lf-adapter-dila"]` |
| `lf-adapter-dila` (nouveau) | `lf-application`, `lf-domain`, `tantivy` (défauts, donc `mmap`), `quick-xml`, `tar`, `flate2`, `tokio` (`rt`), `tracing`, `thiserror` | `lf-application` (`test-support`), `tokio` (`macros`, `rt`, `test-util`), `tempfile` | — |
| `xtask` | + `jsonschema` (`default-features = false`, 02 l.402) | inchangées | — |

`crates/lf-app` ne reçoit qu'un **`src/main.rs`** (`fn main() -> ExitCode { ExitCode::SUCCESS }`) et jamais de `lib.rs`, puisque la règle `mcp-legifrance` vaut `lib: false` (`arch.rs:240-241`).

**Verrous.**

1. Résolution unique de `Cargo.lock` par `cargo metadata`, sans `--locked`.
2. `make min-age MIN_AGE_BASE=origin/main` (réseau). Pour chaque violation, y compris transitive, `cargo update -p <crate>@<v> --precise <plus haute version de 14 jours ou plus>` (02 l.366), en boucle jusqu'au vert. Prévoir 30 à 60 min.
3. Mettre à jour `fuzz/Cargo.lock` : `lf-domain` et `lf-application` y entrent par chemin (`fuzz/Cargo.toml:17` et suivantes). Pour chaque nouveau paquet, `cargo update --manifest-path fuzz/Cargo.toml -p <crate> --precise <version du verrou racine>`, puis `MIN_AGE_LOCKFILE=fuzz/Cargo.lock make min-age`.
4. `make deny audit-fuzz`.

**`xtask/src/arch.rs`.**

- `DOMAIN_ALLOWED` (`:107`) ajoute `winnow`, `aho-corasick`, `regex` et `unicode-normalization`.
- `RULES` (`:211`) passe de 8 à 9 entrées :
  - `lf-adapter-dila`, avec pour internes `lf-application` et `lf-domain` ;
  - la règle `mcp-legifrance` admet `lf-adapter-dila`.
- Mettre à jour le commentaire `:126-128` sur les portes de `lf-adapter-dila`.
- `DEV_ONLY_FEATURES` (`:121`) ajoute `("lf-adapter-mcp", "test-support")`.
- Nouveau contrôle : exigence `=3.5.1` exacte pour `rmcp` et `rmcp-macros` (T-M4-15).
- Tests : `rmcp_must_be_pinned_exactly`, `dila_member_has_a_rule`, `domain_allows_m6_parsers`, `mcp_test_support_is_dev_only`.

**`deny.toml`.**

- `wrappers` (`:45-49`) : ce sont les parents directs réels, relevés par `cargo tree -i <crate> -e normal,dev --all-features --depth 1` ; dans `deny.toml`, `[graph] all-features = true`. Ce qui est connu :

  | Crate | Wrappers | Source |
  |---|---|---|
  | `rmcp` | `lf-adapter-mcp` | — |
  | `rmcp-macros` | `lf-adapter-mcp`, `rmcp` | `rmcp-3.5.1/Cargo.toml:809-816` |
  | `schemars` | `lf-adapter-mcp`, `rmcp` | `rmcp-3.5.1/Cargo.toml:809-816` |
  | `moka` | `lf-adapter-cache` | — |
  | `sha2` | `lf-adapter-audit` | — |
  | `fs4` | `lf-adapter-audit`, `tantivy` | D21 |
  | `tantivy`, `quick-xml`, `tar` | `lf-adapter-dila` | — |

- Ajouter à la liste exacte les licences rencontrées, chacune justifiée en commentaire.
- `multiple-versions` reste `"warn"` (`:37`) jusqu'à F1.

**`clippy.toml`, `disallowed-methods` (T-M3-10).**

- Méthodes interdites, chacune avec sa raison : `chrono::Utc::now`, `chrono::Local::now`, `std::time::SystemTime::now`, `std::env::var`, `std::env::var_os`, `std::env::vars` et `std::env::vars_os`.
- `#[expect(clippy::disallowed_methods, reason)]` sur tous les sites existants (relevé par `grep`) :
  - `crates/lf-adapter-piste/src/http.rs:1417`, `config.rs:1055`, `corpus.rs:1575` et `auth.rs:1509`, dans des modules de test ;
  - `crates/lf-adapter-piste/examples/record_fixture.rs:33` et `:74` ;
  - `crates/lf-adapter-piste/tests/live.rs:41`, `:45` et `:46` ;
  - `xtask/src/main.rs:13` et `:45` (`CARGO`).
- Exceptions admises par la suite : `lf-app` (`clock.rs`, et `config.rs` pour `ProcessEnv`) et `xtask/src/main.rs`.

**`Makefile` et `.cargo/mutants.toml`.**

- `COV_FLAGS` (`Makefile:11`) reçoit `--ignore-filename-regex 'crates/lf-adapter-mcp/src/stdio\.rs$'`, avec sa justification (D6, 06 §7.6).
- `exclude_globs` de `.cargo/mutants.toml` reçoit le même fichier, avec la même justification.

**`.github/workflows/ci.yml`.**

- Job `mutants` refait selon D7 : matrice `mutants-shard` plus agrégateur `mutants` qui garde le nom actuel. La commande est `cargo mutants --in-diff pr.diff $(sed 's/^/--file /' .github/mutants-scope.txt) --shard <k>/4 -j 2 --baseline skip --timeout 60`. `--in-diff` s'applique sur le résultat des autres filtres, et les shards se combinent avec les filtres (livre de cargo-mutants : `in-diff.md`, `shards.md`, vu par context7). Une étape préalable lance `while read -r f; do test -f "$f" || exit 1; done < .github/mutants-scope.txt`. Les entrées passent par `env`, jamais interpolées (comme `:208-212`).
- `timeout-minutes` du job `test` : de 30 à 45 (`:50`).

**`.github/mutants-scope.txt`**, un chemin par ligne. Chaque nom de fichier figure aussi dans la colonne « Fichiers exclusifs » de son unité :

- `crates/lf-adapter-mcp/src/{canonical,decode,lenient,failure,lifecycle}.rs` ;
- `crates/lf-application/src/budget/{mod,reading}.rs`, `usecase/search.rs`, `citation/{verify,resolve}.rs` ;
- `crates/lf-domain/src/text/{summary,focus}.rs`, `citation/{normalize,scan,post}.rs`, `citation/grammar/codes.rs` ;
- `crates/lf-adapter-audit/src/{line,hash,rotation}.rs` ;
- `crates/lf-adapter-cache/src/key.rs` ;
- `crates/lf-app/src/config.rs` ;
- `xtask/src/lock_descriptors.rs`.

**ADR.** `docs/adr/0018-dependances-fin-de-projet.md`, plus une ligne dans `docs/adr/README.md`. Elle consigne :

- l'élargissement de `DOMAIN_ALLOWED` (00 §11.1, l.290) ;
- le crate `dila` et ses dépendances ;
- les décisions D7, D9, D10 et D21 ;
- la feature `test-support` de `lf-adapter-mcp`.

**Hors dépôt.** `/var/tmp/lf-fin/{env.sh, lf-cargo, lf-all}`, décrits au §2.2.

### 0C : interfaces du domaine et de l'application (Opus, 2 à 2,5 h)

Notations : `dom` = `crates/lf-domain/src`, `app` = `crates/lf-application/src`, `piste` = `crates/lf-adapter-piste/src`. Tous les items gelés sont réexportés dès V0.

**`lf-domain`**

- **`error.rs`** : 3 variantes de 03 §9 (l.1274-1281), ce qui porte le total de 17 à 20 : `OffsetPastEnd { offset: usize, text_length: usize }`, `FocusWithoutTerms`, `NumberKindNotApplicable { fond: Fond, kind: &'static str }`. Les tests exhaustifs `error.rs:227`, `:283` et `:387` sont mis à jour.
- **`coverage.rs`** :
  - `pub struct ConfigFacts { pub credentials_configured: bool, pub custom_host: bool, pub has_problems: bool }` ;
  - `pub fn coverage(env: PisteEnvironment, facts: &ConfigFacts, served: &[Fond]) -> Coverage`, en stub fonctionnel (une lacune fixe) pour V1-03.
- **`text/summary.rs`**, stub fonctionnel pour V1-04 (source `Lead`, coupe aux `max_chars` premiers caractères) :
  - `pub enum SummarySource { Official, Dispositif, Lead }`, avec `const fn as_str(self) -> &'static str` ;
  - `pub struct Summary { pub source: SummarySource, pub text: String, pub truncated: bool }` ;
  - `pub enum DispositifRule { ConseilConstitutionnel, ConseilEtat, None }` ;
  - `pub const SUMMARY_CAP_CHARS: usize = 1_200` et `pub const LEAD_MIN_CHARS: usize = 600` ;
  - `pub fn summary(text: &LongText, official: Option<&OfficialSummary>, rule: DispositifRule, max_chars: usize) -> Summary`.
- **`text/focus.rs`**, stub fonctionnel pour V1-05 (découpe aux espaces) : `pub struct Focus` (champ privé `terms: Vec<String>`), `pub fn parse(raw: &str) -> Result<Focus, DomainError>`, `pub fn terms(&self) -> &[String]`, `pub const FOCUS_MAX_CHARS: usize = 500`.
- **`text/stopwords.rs`** : `pub(crate) fn is_stopword(folded: &str) -> bool`, stub qui rend `false`.
- **`text/mod.rs`** : `mod summary; mod focus; mod stopwords;`, puis `pub use summary::{DispositifRule, LEAD_MIN_CHARS, SUMMARY_CAP_CHARS, Summary, SummarySource, summary}` et `pub use focus::{FOCUS_MAX_CHARS, Focus}`.
- **`search.rs`** reçoit `mod number; pub use number::{NumberKind, NumberQuery, NumberValue};`, ce qui garde le chemin `lf_domain::search::NumberQuery` de l'annexe B §1.2. Le fichier **`search/number.rs`** contient :
  - `NumberKind`, avec ses 9 variantes (annexe B l.1072-1082) ;
  - `NumberValue::parse(&str) -> Result<Self, DomainError>` : 1 à 128 caractères `[A-Za-z0-9.*/:_ -]` ;
  - `NumberQuery` (champs de 03 l.630-632), avec `new(fond, kind, value, at: VersionDate) -> Result<Self, DomainError>` et `with_within(self, DateRange) -> Self`.

  Ses corps sont des stubs pour V2-08.
- **`entity/{codes,versions,related}.rs`**, réels (03 l.633-642) : `CodeListing { id: LegiTextId, title: String, etat: Option<LegalStatus> }`, `ArticleVersion`, `RelatedLinks`, `RelatedLink` et `RelatedNature`. Ajouts dans `entity/mod.rs` (`:21-28`), avec leurs `pub use`.
- **`citation/mod.rs`** : la doc seule. `lib.rs` reçoit `pub mod citation;`.

**`lf-application`**

- **`ports/corpus.rs`** :
  - les 5 méthodes de M6 et M7 (02 l.851-874) avec D5 : `find_article(&self, text: &LegiTextId, num: &str, at: ArticleAt)`, `find_by_number(&self, q: &NumberQuery) -> Vec<SearchHit>`, `list_codes(&self) -> Vec<CodeListing>`, `article_versions(&self, id: &ArticleId) -> Vec<ArticleVersion>`, `related_links(&self, id: &ArticleId) -> RelatedLinks`, chacune rendant `Result<_, CorpusError>` ;
  - `pub enum ArticleAt { Today(VersionDate), On(VersionDate) }`, réexporté par `ports` ;
  - `Named` (`:216`) les implémente.
- **`testing.rs`**, réel : `FakeCorpus` reçoit `push_find_article`, `push_find_by_number`, `push_list_codes`, `push_article_versions` et `push_related_links`, avec les variantes correspondantes de `PortRequest`. Leurs tests vivent dans le module de test du fichier, pour la couverture.
- **`input.rs`**, réel. Les noms de champs sont ceux de 05, en `Option<String>` ou `Option<i64>` (i64 tolérant de 05 §3.3). Tous les types dérivent `Debug, Clone, Default, PartialEq, Eq` :
  - `SearchInput { query, fond, match_mode, page, page_size, date_from, date_to, version_date }` ;
  - `GetActInput { text_id, date, toc_offset, toc_limit }` ;
  - `ReadingInput { detail, offset, max_chars, focus, passages }` ;
  - `GetTextInput { article_id, reading }`, `GetDecisionInput { decision_id, reading }`, `GetDeliberationInput { deliberation_id, reading }` ;
  - `GetConventionInput { convention_id, toc_offset, toc_limit }`, `GetCompanyAgreementInput { agreement_id }`, `CoverageInput` ;
  - `VerifyCitationsInput { text, max_citations }`, `ResolveInput { citation, date }`, `GetRelatedInput { id }` ;
  - `DiffArticleInput { article_id, date_a, date_b }`, `SearchAllInput { query, fonds: Option<Vec<String>>, per_fond }` ;
  - `ResourceInput { uri }`, `CompleteInput { argument, value }`.
- **`budget/mod.rs`**, réel et testé :
  - `ResponseBudget`, avec ses constantes :

    | Constante | Valeur |
    |---|---|
    | `MAX_CHARS_DEFAULT` | 4 000 |
    | `MAX_CHARS_MIN` | 500 |
    | `MAX_CHARS_MAX` | 40 000 |
    | `PASSAGES_DEFAULT` | 3 |
    | `PASSAGES_MAX` | 10 |
    | `SNIPPET_MAX_CHARS` | 280 |
    | `TOC_LIMIT_DEFAULT` | 100 |
    | `TOC_LIMIT_MAX` | 500 |
    | `MAX_CITATIONS_DEFAULT` | 50 |
    | `MAX_CITATIONS_MAX` | 100 |
    | `HARD_CAP_CHARS` | 45 000 |
    | `ESTIMATE_TARGET_CHARS` | 42 000 |
    | `ESTIMATE_PER_FIELD` | 64 |
    | `REACHABLE_RESULTS` | 10 000 |

  - `pub const fn estimate_chars(fields: usize, text_chars: usize) -> usize` (saturant) ;
  - `pub fn shrink_from_end<T>(items: &mut Vec<T>, over: impl FnMut(&[T]) -> bool) -> usize` : TOC et listes ; rend le nombre d'éléments retirés ;
  - `pub fn strip_from_end<T>(items: &mut [T], over: impl FnMut(&[T]) -> bool, strip: impl FnMut(&mut T) -> bool) -> usize` : extraits retirés en partant du dernier élément, jamais l'élément lui-même ;
  - tests : `estimate_counts_64_per_field`, `shrink_stops_as_soon_as_under`, `strip_never_removes_items`.
- **`budget/reading.rs`**, signatures gelées et corps `todo!()` pour V1-06 :
  - `pub struct ReadingRequest` (forme interne laissée à V1-06) et `ReadingRequest::parse(input: &ReadingInput) -> Result<Self, AppError>` ;
  - `pub struct PassageOut { pub offset: u32, pub text: String }` (05 §5.0.3) ;
  - `pub struct Reading { pub summary: Option<Summary>, pub text: Option<String>, pub text_length: u32, pub next_offset: Option<u32>, pub passages: Option<Vec<PassageOut>>, pub note: Option<String> }` ;
  - `pub fn read(text: &LongText, official: Option<&OfficialSummary>, rule: DispositifRule, req: &ReadingRequest) -> Result<Reading, AppError>`.
- **`service/mod.rs`**, réel (D13) :
  - `LegalService<C: LegalCorpus, A: AuditLog, K: Clock> { corpus: C, audited: Audited<A>, clock: K, settings: ServiceSettings, complete_gate: CompleteGate }`, avec `new(corpus, audit, clock, settings)` et `settings()` ;
  - `ServiceSettings`, et `ConfigurationState { credentials_configured: bool, custom_host: bool, problems: Vec<String> }` avec `fn facts(&self) -> ConfigFacts` ;
  - `mod` des 15 fichiers `service/<x>.rs` (13 outils, plus `resource` et `complete`), réduits à leur doc.
  - Motif gelé, que chaque tranche écrit dans son fichier : `pub async fn <outil>(&self, call: ToolCall<XInput>, cancel: impl Future<Output = ()> + Send) -> Result<XOutput, AppError>`. Les modules enfants voient les champs privés.
  - Exceptions au motif : `read_resource(&self, call: ToolCall<ResourceInput>, cancel)`, audité sous `resource:<gabarit>` ; `complete(&self, input: CompleteInput) -> CompleteOutput`, non audité, qui rend une liste vide en cas d'erreur.
- **`usecase/mod.rs` et `output/mod.rs`** déclarent les 15 fichiers `usecase/<x>.rs` et `output/<x>.rs`, réduits à leur doc. Exception : `usecase/complete.rs` contient `pub struct CompleteGate` et `CompleteGate::new() -> Self`, stub fonctionnel pour V3-07.
- **`messages.rs`**, réel, avec les textes de 05 §7.3 :
  - `required(field: &str) -> AppError` ;
  - `out_of_range(field: &'static str, value: i64, min: i64, max: i64) -> AppError` ;
  - `not_one_of(field: &'static str, value: &str, allowed: &[&str]) -> AppError` ;
  - `domain(tool: ToolName, error: &DomainError) -> AppError` : la famille « entrée » de 03 §9 donne `invalid_arg`, avec la phrase d'aide.
- **`tools.rs`** :
  - constantes `FR_VERIFY_CITATIONS`, `FR_RESOLVE`, `FR_GET_RELATED`, `FR_DIFF_ARTICLE`, `FR_SEARCH_ALL` ;
  - constantes `RESOURCE_ARTICLE` (`"resource:article"`), `RESOURCE_DECISION`, `RESOURCE_ACT`, `RESOURCE_CODE_ARTICLE`, `RESOURCE_COVERAGE` ;
  - `ALL` (`:32`) reste à 8 et s'étend à l'intégration qui enregistre les nouveaux outils.
- **`citation/mod.rs`** déclare `pub mod {verify, resolve, related, policy, notes};` (annexe B §1.2, l.66), fichiers réduits à leur doc.
- **`contract.rs`**, sous `#[cfg(feature = "test-support")]`, réduit à sa doc pour V2-05.
- **`lib.rs`** (`:17-29`) ajoute `budget`, `citation`, `contract` (sous la feature), `output`, `service` et `usecase`.

**`lf-adapter-piste`**

- `corpus/port.rs` : les 5 méthodes délèguent à `corpus/m6.rs` et `corpus/m7.rs`, où chaque fonction inhérente `async` est un stub `todo!()` (D18).
- `corpus.rs` (`:41-43`) reçoit `mod m6; mod m7;`. `request/mod.rs`, `dto/mod.rs` et `map/mod.rs` déclarent leurs `m6` et `m7`, et `lib.rs` déclare `mod codes;`. Ces fichiers sont réduits à leur doc.

### 0M : infrastructure MCP, squelettes du binaire et de xtask (Sonnet, 1,5 à 2 h, en parallèle de 0C)

Notation : `mcp` = `crates/lf-adapter-mcp/src`.

**Fichiers de `mcp`**

| Fichier | État | Contenu, et unité qui finit le stub |
|---|---|---|
| `schema.rs` | réel | `pub(crate) fn input_schema<P: JsonSchema + 'static>() -> Arc<JsonObject>` et `without_dollar_schema` (05 §3.3). |
| `lenient.rs` | stub fonctionnel | `opt_int` (entiers JSON seuls), fini par V1-09. |
| `canonical.rs` | stub fonctionnel | `pub(crate) fn of(args: &JsonObject) -> CanonicalInput` (`serde_json::to_string`), fini par V1-09. |
| `decode.rs` | réel | `pub(crate) fn to_call<P: DeserializeOwned + Into<I>, I>(args: JsonObject) -> ToolCall<I>`, avec le message « chemin: motif » de `serde_path_to_error`. |
| `failure.rs` | réel, minimal | `pub(crate) struct ToolFailure(AppError)` et `IntoContents` : un bloc texte `[code] message`, `isError: true`, sans `structuredContent`. Il passe par `AppError::code()` et `message()` (`error.rs:144`, `:158`). |
| `versions.rs` | réel | `SUPPORTED` = 2026-07-28, 2025-11-25, 2025-06-18. |
| `lifecycle.rs` | réel, testé | `PreLifecycleFilter`, repris du spike. |
| `stdio.rs` | réel, colle exclue de la couverture (D6) | `pub async fn serve_stdio<S: ServerHandler>(server: S, shutdown: CancellationToken) -> Result<ServeOutcome, ServeError>`, avec `pub enum ServeOutcome { Eof, Shutdown }` et `ServeError` (`thiserror`). |
| `text.rs` | stub | `INSTRUCTIONS_V01`, `INSTRUCTIONS_V02` et `INSTRUCTIONS_V03` (`&str` vides, marqués `STUB(V0) → V1-10`) ; `pub enum Surface { V01, V02, V03 }` et `pub const SURFACE: Surface = Surface::V01`. |
| `lib.rs` | — | Déclare ces modules. |

**`crates/lf-app/src/main.rs`** :

- déclare `mod cli; mod config; mod check; mod clock; mod logging; mod shutdown; mod wiring;`, chaque fichier réduit à sa doc ;
- `main` rend `ExitCode::SUCCESS`.

**`xtask`**

- `lib.rs` (`:3-6`) reçoit `pub mod lock_descriptors; pub mod instructions;`.
- `main.rs` : sous-commandes `lock-descriptors` (`--check` par défaut, `--update`, `--compare <ref>`) et `instructions-size`.
- `lock_descriptors.rs` : `pub enum LockMode { Check, Update, Compare(String) }` et `pub fn run(mode: LockMode) -> Result<(), String>`, en stub pour V1-13.
- `instructions.rs` : `pub fn run() -> Result<(), String>`, en stub pour V1-10.

### 0W : câblage MCP (Sonnet, environ 1 h, après la fusion de 0C et 0M)

**`server.rs`**

- `pub struct LegifranceServer<C, A, K> { service: Arc<LegalService<C, A, K>>, tool_router: ToolRouter<Self> }`.
- `pub fn new(service: Arc<LegalService<C, A, K>>) -> Self` compose `Self::search_router() + … + Self::search_all_router()`, soit 13 routeurs.
- `#[tool_handler(router = self.tool_router)] impl ServerHandler` porte :
  - un `get_info` minimal (V1-10) ;
  - les gestionnaires resources, prompts et completion, délégués à `resources.rs`, `prompts.rs` et `completions.rs`, des stubs fonctionnels qui rendent des listes vides (V3-07).

**`params/`**

- `mod.rs` déclare les 13 fichiers d'outils, plus `reading`.
- `params/reading.rs` est complet : `ReadingParams` (05 §5.0, avec `schemars`, `deny_unknown_fields` et `opt_int`, aplati dans les trois lectures longues) et `impl From<ReadingParams> for ReadingInput`.
- `params/<x>.rs` × 13 sont réduits à leur doc.

**`views/`**

- `mod.rs` déclare `common`, `fit`, `reading`, `toc` et les 13 fichiers d'outils.
- `common.rs` est réel (05 §5.0.4) : `title_unless_in_citation(title: Option<&str>, citation: &str) -> Option<String>`, `etat_code(Option<LegalStatus>) -> Option<&'static str>` et `date_unless_sentinel(Option<CivilDate>) -> Option<String>` (seuil 2222-01-01).
- `fit.rs` est réel : `pub(crate) fn fit_under_cap<V: Serialize>(view: &mut V, degrade: impl FnMut(&mut V) -> bool) -> usize`. Il mesure le JSON compact, en caractères, et dégrade tant que la taille dépasse `HARD_CAP_CHARS` et que `degrade` rend `true`. S'il reste au-dessus, il émet un `warn!`. Tests : `fit_returns_size_without_degrading_when_under_cap`, `fit_degrades_until_under_cap` (avec échappements et emoji), `fit_stops_when_degrade_gives_up`.
- Les autres fichiers sont réduits à leur doc.

**`tools/`**

- `mod.rs` donne en commentaire le motif `#[tool]` retenu par 0A et déclare les 13 fichiers.
- `tools/<x>.rs` × 13 : `#[tool_router(router = <x>_router, vis = "pub(crate)", allow_empty)] impl<C, A, K> LegifranceServer<C, A, K> {}`, avec les bornes du spike.

**`testing.rs`** (feature `test-support`) : `pub async fn connect<S: ServerHandler>(server: S) -> RunningService<RoleClient, ()>`, par `tokio::io::duplex` et un client rmcp. Test `connect_lists_no_tools_on_empty_router`.

**`lib.rs`** : déclare les modules, puis `pub use server::LegifranceServer; pub use stdio::{ServeOutcome, ServeError, serve_stdio};`.

### 0D : contrôles de V0 (intégrateur, 45 min)

1. `lf-all make fmt-check lint test doc arch purity machete deny`, puis `make audit-fuzz`, `make min-age`, `MIN_AGE_LOCKFILE=fuzz/Cargo.lock make min-age` et `cargo test --manifest-path spikes/rmcp-stdio/Cargo.toml`. `make cov` ne tourne pas (il reste des stubs).
2. Construire la base chaude (§2.2). L'étape `cargo nextest run --workspace --all-features --locked --no-run` prouve que toutes les dépendances de développement compilent.
3. Si la chaîne 1.88 est installée, `cargo +1.88 check --workspace --all-features --locked`. Sinon, le job `msrv` se lit après le premier push.
4. Nettoyer les branches et worktrees du harnais (§5, étape 3). V0 ne se pousse pas.

## 4. Vagues parallèles

**Notations.**

- `app` = `crates/lf-application/src`, `mcp` = `crates/lf-adapter-mcp/src`, `dom` = `crates/lf-domain/src`, `piste` = `crates/lf-adapter-piste/src`, `bin` = `crates/lf-app/src`.
- **`T(x)`** désigne une tranche d'outil : `app/{usecase,output,service}/x.rs` et `mcp/{params,views,tools}/x.rs`, plus leurs snapshots (règle S).
- Les tests d'une tranche vivent dans les `mod tests` de `usecase/x.rs` (sur `FakeCorpus` et `FixedClock`) et de `tools/x.rs` (de bout en bout, par `testing::connect`).
- Chaque tableau donne l'**ordre de fusion**.
- Tailles : S < 1 h, M de 1 à 2 h, L de 2 à 3 h.

### V1 : briques v0.1 et voies longues (16 unités ; 4 à 5 h)

| Id | Titre | Fichiers exclusifs | Consomme | Produit | Tests et critère de fin | Taille | Modèle |
|---|---|---|---|---|---|---|---|
| V1-01 | Tranche `fr_search` | `T(search)` | `SearchInput`, `messages`, `decode`, `failure`, `estimate_chars`, `strip_from_end`, `fit_under_cap`, `FakeCorpus`, `connect` | `fr_search` | **Cas d'usage** : `search_validation_order_matches_reference`, `page_beyond_reachable_window_is_invalid_arg_without_network`, `invalid_date_is_rejected_locally`, `version_date_rejected_outside_loda_code`, `has_more_false_at_reachable_end_with_note`, `port_query_version_date_follows_paris_day`. **Outil** : `snippets_removed_from_last_hits_when_over_cap`, `schema_has_enum_for_fond_and_bounds_for_page_size`, `invalid_type_is_audited_invalid_arg`. | M | Sonnet |
| V1-02 | Tranches `fr_get_act` et `fr_get_convention` (TOC) | `T(get_act)`, `T(get_convention)`, `mcp/views/toc.rs` | `Toc::page` (`toc.rs:226`), `shrink_from_end` | les deux outils | `get_act_default_date_is_today_paris`, `get_act_suffix_date_is_default_date`, `toc_page_bounds`, `toc_shrunk_under_estimate_with_next_offset`, `toc_etat_omitted_when_equal_to_text`, `get_act_on_code_adds_note`, `convention_has_no_date_param` | M | Sonnet |
| V1-03 | Tranches `fr_get_company_agreement` et `fr_coverage`, reports du domaine | `T(get_company_agreement)`, `T(coverage)`, `dom/coverage.rs`, `dom/id.rs` | `ServiceSettings`, `ConfigFacts` | `coverage()` réel ; `Ord` sur les identifiants (M1-D11) | `coverage_invariants_ported`, `coverage_gaps_fr001_to_fr009`, `coverage_degraded_without_credentials`, `coverage_rejects_any_argument`, `ids_have_total_order`, `agreement_view_has_no_text_field` | M | Sonnet |
| V1-04 | Résumé (03 §7.9) | `dom/text/summary.rs` | `LongText::paragraphs`, `passages` | `summary()` réel | `official_analyses_first_then_titrages`, `dispositif_constit_between_decide_and_juge_par`, `dispositif_cetat_skips_decorative_paragraphs`, `no_dispositif_falls_back_to_lead`, `lead_reaches_600_chars`, `summary_capped_at_min_max_chars_1200_cut_at_sentence` | M | Sonnet |
| V1-05 | Focus, mots vides, jetons juridiques | `dom/text/{focus,stopwords,rank}.rs` | `fold` | `Focus::parse`, `is_stopword` ; la signature publique de `rank` est inchangée (`text/mod.rs:22`) | `focus_rejects_only_stopwords`, `focus_terms_folded_and_deduplicated`, `focus_over_500_chars_rejected`, `legal_numbers_are_single_tokens` (`24-13.470`, `2025-1173`, `761-1`, `85/374`), `rank_is_deterministic_and_within_budget` | M | Sonnet |
| V1-06 | Politique de lecture : `read` complet, trois modes | `app/budget/reading.rs`, `mcp/views/reading.rs` | `window` (`window.rs:52`), `passages`, `rank`, ainsi que `summary` et `Focus` dans leurs signatures gelées | `ReadingRequest::parse`, `read` et `ReadingView`, avec les modes résumé, plein texte et passages tous reliés. Les tests du mode résumé sont **structurels** (source parmi trois, longueur au plus égale au plafond) et ne dépendent pas de l'algorithme de V1-04. | `reading_request_modes`, `offset_without_detail_means_full`, `detail_summary_with_offset_is_invalid_arg`, `max_chars_bounds_500_40000`, `passages_ignored_without_focus`, `offset_past_end_is_invalid_arg`, `empty_text_offset_zero_gives_note`, `short_text_returned_whole`, `summary_mode_shape_is_algorithm_independent`, `paged_reading_reconstructs_full_text` | M | Opus |
| V1-07 | `lf-adapter-audit` (T-M3-06) | `crates/lf-adapter-audit/src/{lib,line,hash,rotation,file}.rs` | `AuditLog`, `AuditRecord` | `JsonlAuditLog::open(dir: &Path, rotate_bytes: u64) -> Result<Self, AuditError>` | `line_fields_in_order`, `input_hash_matches_p041_vectors`, `rotation_under_same_lock`, `no_torn_lines_under_concurrent_writers`, `unwritable_dir_is_an_error_not_a_panic`, `file_mode_0600_on_unix`. Mesures `#[ignore]` : `windows_acl_report` (D22), V-166. | L | Opus |
| V1-08 | `lf-adapter-cache` (T-M3-07), 12 opérations | `crates/lf-adapter-cache/src/{lib,key,policy,corpus}.rs` | le port complet, `ArticleAt` | `CachedCorpus::new(inner: C, env: PisteEnvironment, policy: CachePolicy) -> Self` | les 8 tests de T-M3-07, plus `m6_m7_ops_have_their_ttl` (codes : 7 jours) et `article_at_today_key_changes_daily` | M | Sonnet |
| V1-09 | Décodage exact | `mcp/{lenient,canonical,decode}.rs` | `ToolCall`, `InputError`, `ReadingParams` | `opt_int` et `of`, exacts | `canonical_input_matches_python_vectors` (P-041), `lenient_int_accepts_integer_float_and_decimal_string`, `null_is_absent_for_optional_params`, `huge_integer_is_out_of_range_not_panic`, `unknown_field_message_lists_expected`, `invalid_arguments_keep_canonical_input` | M | Opus |
| V1-10 | Protocole, erreurs, instructions | `mcp/{server,versions,stdio,lifecycle,failure,text}.rs`, `xtask/src/instructions.rs` | le verdict du spike, `connect` | `get_info` ; textes v0.1 à v0.3 de 05 §4 | `server_info_is_mcp_legifrance_with_crate_version`, `supported_versions_are_explicit`, `capabilities_same_in_both_eras`, `error_results_are_text_with_code_prefix`, `error_text_snapshot_per_code` (`src/snapshots/lf_adapter_mcp__failure__*`), `unknown_tool_is_jsonrpc_32602_without_audit`, `tool_panic_is_internal_error_and_server_continues` (route de test, V-36), `premature_notification_does_not_stop_server`, `instructions_fit_budget`, `instructions_carry_attribution` | L | Opus |
| V1-11 | Configuration et `check-config` | `bin/{config,check}.rs` | `PisteConfig::new` (`piste/config.rs:602`) | `trait EnvSource`, `ProcessEnv`, `AppConfig`, `load(&impl EnvSource) -> Loaded` (variables de 00 §1) | `fr_eli_variables_are_read_with_deprecation_warning`, `legacy_unread_variables_warn_ignored`, `fr_eli_runtime_url_is_ignored`, `invalid_advanced_setting_falls_back_to_default_with_warn`, `audit_mode_defaults_to_strict`, `tool_deadline_capped_at_600s`, `config_errors_never_contain_secret_or_userinfo` | M | Sonnet |
| V1-12 | CLI, horloge, journaux, arrêt | `bin/{cli,clock,logging,shutdown}.rs` | `Clock` | `SystemClock`, `Cli`, `init_logging(filter: &str)`, `shutdown_token() -> CancellationToken` | `today_is_paris_date_at_2230_utc_in_summer`, `tz_env_does_not_change_today`, `default_log_filter_caps_rmcp`, `cli_defaults_to_serve`, `sigterm_cancels_token` (unix) | M | Sonnet |
| V1-13 | `cargo xtask lock-descriptors` : logique | `xtask/src/lock_descriptors.rs`, `xtask/src/lock_descriptors/**` | sessions JSON-RPC de 05 §9.2 | `--check`, `--update`, `--compare`, manifeste MCPB, version de `server.json` | Sur du JSON synthétique : `lock_diff_classification`, `descriptor_lock_rejects_empty_baseline`, `lock_controls_reject_forbidden_shapes`, `lock_generates_mcpb_manifest_and_server_json_version`. | L | Opus |
| V1-14 | Socle du parseur de citations (annexe B §3, §5, §6.1-6.5) | `dom/citation/**` (pour V1 seulement), `crates/lf-domain/tests/data/citations/codes-table.md` | — | Disposition de l'annexe B §1.2 (l.63) et de 03 l.140 : `mod.rs`, `normalize.rs`, `types.rs`, `dict/{mod,codes,courts}.rs`, `scan.rs`, `post.rs`, `format.rs`, `quote.rs`, `grammar/mod.rs` et `grammar/{codes,textes,constit,cassation,fond,ce,ta_caa,cc,ecli,cnil,kali,jorf,puits}.rs`. Gel pour V2, détaillé sous ce tableau. | `offset_map_roundtrip`, `key_view_is_one_to_one`, `fold_table_matches_annex_b_3_2`, `dict_matches_codes_table` (annexe B §5.5), `juridictions_closed_values` | L (3 à 3,5 h) | Opus |
| V1-15 | Distribution A (T-MD-01, -03, -04, -07, -11) | `dist-workspace.toml`, `.github/workflows/{release,release-gates,smoke-targets}.yml` | dist 0.33.0 (binaire GitHub, empreinte vérifiée) | `dist plan` vert ; attestation et SBOM selon 08 §9 ; `pr-run-mode = "plan"` | `dist plan` vert, `make workflow-lint` vert | M | Sonnet |
| V1-16 | M9 : reconnaissance (T-M9-01) | `docs/recherche/10-dumps-dila.md`, `crates/lf-adapter-dila/tests/fixtures/**` (extraits LEGI minimaux, Licence Ouverte) | réseau (`echanges.dila.gouv.fr`) ; échantillons dans `/var/tmp/lf-fin/dila` | constats V-185 à V-189, fixtures d'ingestion | rapport rempli ; fixtures avec un `.meta.toml` de provenance | M | Opus |

**Gel produit par V1-14 pour les grammaires de V2.**

- **Livré réel** :
  - `Normalized` et `OffsetMap` dans `normalize.rs` (annexe B §3.4) ;
  - les types de §6.2 à §6.5 dans `types.rs` ;
  - `CodeKey` et `CODES` dans `dict/codes.rs` ;
  - `pub(crate) struct Anchor { pub class: AnchorClass, pub range: Range<usize> }` dans `scan.rs`, constructible à la main ;
  - `pub(crate) struct Candidate { pub range: Range<usize>, pub value: CitationRef, pub priority: u8, pub confidence: Confidence, pub flags: Flags }` dans `grammar/mod.rs`.
- **Livré en stub** :
  - `pub(crate) fn find_anchors(n: &Normalized) -> Vec<Anchor>` ;
  - `pub(crate) fn scan(n: &Normalized, anchors: &[Anchor]) -> Vec<Candidate>` dans chaque `grammar/<famille>.rs` ;
  - `post::arbitrate(c: Vec<Candidate>, opts: &ParseOptions) -> Vec<Spanned<CitationRef>>` ;
  - `parse`, `parse_with`, `adjacent_quote` et `ParseOptions` (annexe B §6.1), réexportés par `citation/mod.rs` ;
  - `format::canonical` et `resolvability`.

### V2 : lectures longues, binaire, contrat, M6 et M7 côté port, grammaires (16 unités ; 4 à 5 h)

**Préalable** : l'étape I-R de l'intégration de V1 (§5).

| Id | Titre | Fichiers exclusifs | Consomme | Produit | Tests et critère de fin | Taille | Modèle |
|---|---|---|---|---|---|---|---|
| V2-01 | Tranche `fr_get_text` | `T(get_text)` | `read`, `Focus` (réels après V1) | `fr_get_text` | `summary_mode_uses_lead_for_articles`, `focus_returns_passages_with_offsets`, `focus_without_terms_message`, `kaliarti_accepted`, `sentinel_dates_omitted`, `paged_reading_end_to_end` | M | Sonnet |
| V2-02 | Tranche `fr_get_decision` | `T(get_decision)` | `read`, `DispositifRule` | `fr_get_decision` | `dispositif_rule_from_prefix`, `not_found_message_by_prefix` (E-15), `ecli_native_only`, `default_decision_response_under_budget` (2 000 caractères au plus) | M | Sonnet |
| V2-03 | Tranche `fr_get_deliberation` | `T(get_deliberation)` | `read` | `fr_get_deliberation` | `summary_source_is_lead_for_cnil`, `title_long_rule` | S | Sonnet |
| V2-04 | Binaire : `main` et câblage (T-M4-07, Q9) | `bin/{main,wiring}.rs`, `crates/lf-app/tests/smoke.rs` | `CachedCorpus`, `JsonlAuditLog`, `load`, `SystemClock`, `serve_stdio`, `LegifranceServer` | `wiring::run(cfg: AppConfig, shutdown: CancellationToken) -> anyhow::Result<ExitCode>` ; alias `AppService` ; `mcp-legifrance serve` et `check-config` ; codes de sortie | Module de test de `wiring` : `audit_mode_strict_reaches_policy`. Dans `tests/smoke.rs` : `binary_answers_initialize`, `stdout_contains_only_jsonrpc`, `e2e_discover_without_network`, `missing_credentials_starts_degraded`. | M | Sonnet |
| V2-05 | Suite de contrat sur les 7 opérations de M3 à M5, golden du résumé, sondes M5 | `app/contract.rs`, `crates/lf-adapter-piste/tests/{contract,summary_golden,live_m5}.rs`, `crates/lf-adapter-cache/tests/contract.rs` | `PisteCorpus` sous wiremock et fixtures | `pub async fn corpus_contract_m5<C: LegalCorpus>(corpus: &C, fx: &ContractFixtures)` | `contract_on_fake_corpus` (module de test de `contract.rs`, pour la couverture), `contract_on_piste_corpus_with_fixtures`, `contract_on_cached_corpus`, `summary_matches_fixture_lengths` (551, 411, 829, 736). Sondes `#[ignore]` : `live_type_pagination_page2_code`, `live_deep_pagination_limit`, `live_response_sizes` (V-82, V-83, V-75). | M | Sonnet |
| V2-06 | Transcriptions JSON-RPC en processus | `mcp/testing.rs`, `crates/lf-adapter-mcp/tests/jsonrpc/**` (données) | serveur v0.1 | `Transcript::load`, `replay_in_process` (exposés sous `test-support`) ; 22 transcriptions (06 §6.1) | Module de test de `testing.rs` : les transcriptions qui ne touchent pas aux trois lectures longues, vertes ; `eof_with_inflight_call_exits_zero_within_1s` ; constats V-33, V-34, V-37, V-168. L'intégrateur vérifie les autres une fois V2-01 à V2-03 fusionnées. | L | Opus |
| V2-07 | Tranches `fr_diff_article` et `fr_search_all` (M7) | `T(diff_article)`, `T(search_all)` | port M7 (sur `FakeCorpus`), `similar`, `futures`, `shrink_from_end` | les deux outils | `search_all_respects_concurrency_cap`, `search_all_error_per_group`, `search_all_trimmed_under_estimate`, `diff_is_compact_hunks`, `diff_dates_default_today_paris`, `no_progress_after_cancel` | L | Sonnet |
| V2-08 | PISTE M6 (T-M6-04) | `piste/{corpus/m6,request/m6,dto/m6,map/m6,codes}.rs`, `dom/search/number.rs`, `crates/lf-adapter-piste/tests/live_m6.rs` | 04 §7.1-7.2, annexe B §6.6 ; fixtures I-R en lecture seule, ou à défaut des corps synthétiques en ligne | `find_article` (avec `ArticleAt`), `find_by_number`, `list_codes`, `NumberQuery::new` | `find_article_today_uses_get_article_with_id_and_num`, `find_article_on_date_uses_search_exacte`, `find_by_number_couples_of_annex_b_6_6`, `list_codes_lazy_single_table`, `ecli_facet_filter`, `number_kind_not_applicable` | L | Opus |
| V2-09 | PISTE M7 (T-M7-02) | `piste/{corpus/m7,request/m7,dto/m7,map/m7}.rs`, `crates/lf-adapter-piste/tests/live_m7.rs` | 04 §7.3 ; fixtures I-R ou corps synthétiques | `article_versions`, `related_links` | `article_versions_canonical_then_fallback_by_cid`, `related_links_maps_natures`, `related_link_without_consultable_target_keeps_title` | M | Sonnet |
| V2-10 | Grammaires A : codes, textes, bloc de constitutionnalité (§4.2-4.4) | `dom/citation/grammar/{codes,textes,constit}.rs` | `Normalized`, `Anchor`, `Candidate`, `dict` | `scan` × 3 | Cas de §4.2 à §4.4 sur des ancres construites à la main. Les tableaux complets passent par le pipeline en V3-04. | L | Opus |
| V2-11 | Grammaires B : juridictions (§4.5-4.10) | `dom/citation/grammar/{cassation,fond,ce,ta_caa,cc,ecli}.rs` | idem | `scan` × 6 | idem, cas de §4.5 à §4.10 | L | Opus |
| V2-12 | Grammaires C : CNIL, KALI et ACCO, JORF, NOR, ELI, puits (§4.11-4.14) | `dom/citation/grammar/{cnil,kali,jorf,puits}.rs` | idem | `scan` × 4 | idem, cas de §4.11 à §4.14 | M | Sonnet |
| V2-13 | Ancres, arbitrage et pipeline (§4.15-4.19) | `dom/citation/{scan,post,mod}.rs` | `scan` de chaque famille (signatures) | `find_anchors`, `arbitrate`, `parse`, `parse_with` | `anchor_classes_of_annex_b_4_16`, `overlaps_resolved_by_annex_rules` (candidats construits à la main), `sink_drops_contained_candidates`, `enumeration_spans_are_numbers`, `context_stack_resets_per_call`, `max_citations_guard`, `no_panic_on_arbitrary_input` (proptest) | L | Opus |
| V2-14 | Formes canoniques, résolvabilité, extrait cité (§6.5, §7.9) | `dom/citation/{format,quote}.rs` | `types` | `format::canonical`, `resolvability`, `adjacent_quote` | `canonical_forms_of_annex_b_6_5`, `resolvability_by_kind`, `adjacent_quote_detects_guillemets` | M | Sonnet |
| V2-15 | M9 : ingestion (T-M9-02) | `crates/lf-adapter-dila/src/**` | fixtures de V1-16 | flux `flate2` → `tar` → `quick-xml` ; registre des archives | `ingest_streams_without_extraction`, `deletions_applied_in_order`, `rebuild_on_new_global` | L | Opus |
| V2-16 | Distribution B (T-MD-04, -05, -06) | `packaging/**`, `server.json`, `.github/workflows/{build-mcpb,publish-oci,publish-mcp-registry}.yml`, `.github/scripts/install-mcp-publisher.sh` | `dist-workspace.toml` | MCPB à lanceur POSIX, image OCI distroless, `server.json` | `make workflow-lint` vert. `docker build` n'est pas lancé en local (règles WSL). | M | Sonnet |

### V3 : bout en bout, verrou, budgets, puis M6, M7 et M9 côté outils (12 unités ; 3,5 à 4,5 h)

V3-01 à V3-03 forment le chemin critique de la v0.1. Ils sont fusionnés en premier et prioritaires dans l'ordre de lancement.

| Id | Titre | Fichiers exclusifs | Consomme | Produit | Tests et critère de fin | Taille | Modèle |
|---|---|---|---|---|---|---|---|
| V3-01 | e2e du binaire (T-M4-07, T-M4-08, T-M4-13) | `crates/lf-app/tests/{e2e_stdio,cli,leaks,perf,live_e2e}.rs`, `crates/lf-app/tests/common/mod.rs` | `Transcript::load`, le binaire | `replay_on_child` (dans `tests/common`) ; V-30 et V-35 levées | **Transcriptions** : les 22 sur le binaire, dont `eof_with_open_listen`. **Arrêt et configuration** : `e2e_sigterm_exits_quickly`, `e2e_missing_credentials_config_error_audited`, `e2e_invalid_config_starts_degraded`. **Audit** : `audit_unwritable_dir_fails_call`, `two_processes_share_audit_file`. **Fuites** : `error_messages_never_contain_secret_or_userinfo`, `logs_never_contain_query_text`. **Divers** : `user_agent_contains_crate_version`, `second_identical_loda_search_hits_cache` (wiremock), `perf_rss_idle` (V-18). **Live** : `live_e2e_reference_scenarios`, en `#[ignore]`. | L | Opus |
| V3-02 | Verrou des descripteurs : génération et tests | `tests/tools.lock.json`, `crates/lf-adapter-mcp/tests/descriptors.rs` | `lock-descriptors`, le binaire | le verrou des outils fusionnés | `tool_router_builds_and_roots_are_objects`, `tools_list_matches_surface`, `annotations_are_read_only_on_all_tools`, `reference_parameters_preserved`, `descriptions_cite_only_registered_tools`, `descriptions_have_no_cross_server_directives`, `every_tool_has_object_output_schema`, `structured_content_identical_across_eras` ; `--check` vert | M | Sonnet |
| V3-03 | Budgets et golden (T-M5-04, T-M5-05) | `crates/lf-adapter-mcp/tests/budgets.rs`, `crates/lf-adapter-mcp/tests/snapshots/budgets__*.snap`, `crates/lf-app/tests/parity_tool_output.rs`, `crates/lf-app/tests/snapshots/parity_tool_output__*.snap` | les outils v0.1 et M7 | budgets bloquants | `every_tool_response_under_hard_cap` (cas échappements et emoji), `default_outputs_within_target` (05 §6.5, plus 10 %), `tool_output_matches_golden`, `regression_catalog_is_implemented` (D-01 à D-47) ; snapshots sur les 16 fixtures | L | Opus |
| V3-04 | Corpus et robustesse du parseur (T-M6-03) | tout `dom/citation/**` (V3), `crates/lf-domain/tests/citations_corpus.rs`, `crates/lf-domain/tests/data/citations/*.jsonl`, `fuzz/fuzz_targets/citation_parse.rs`, `fuzz/Cargo.toml`, `fuzz/Cargo.lock` | le parseur de V2 | un corpus vert | Le corpus de l'annexe B §8 passe, hors cas « de mémoire ». Les tableaux de §4.x passent par `parse_with`. `perf_parse_20k` est mesuré (cible : 50 ms au plus), et le fuzz compile. | L | Opus |
| V3-05 | Tranche `fr_verify_citations` et moteur de résolution (T-M6-05, T-M6-06) | `app/citation/{mod,verify,resolve,policy,notes}.rs`, `T(verify_citations)` | `parse_with`, port M6, `list_codes` | `resolve_ref(...)`, que V4-01 consomme | **Annexe B** : les tests de §8.6. **Résolution** : `statuses_and_spans_in_code_points`, `today_date_uses_article_at_today`, `sandbox_never_not_found`, `quote_auto_threshold` (V-162). **Bornes** : `stops_at_80_percent_of_deadline_with_note`, `concurrency_is_buffer_unordered_4`, `list_truncated_under_estimate`. | L | Opus |
| V3-06 | Tranche `fr_get_related` | `app/citation/related.rs`, `T(get_related)` | `related_links`, `parse_with` (`strip_html`) | `fr_get_related` | `related_capped_at_100_with_truncated`, `decision_related_from_citationjp`, `nature_to_kind_mapping` | M | Sonnet |
| V3-07 | Resources, prompts et completions (T-M7-04, T-M7-05) | `mcp/{resources,prompts,completions,server}.rs`, `app/{usecase,output,service}/{resource,complete}.rs` | les lectures de V2, `list_codes`, `CompleteGate` | capacités resources et prompts | `resource_not_found_codes_by_era` (-32002 en legacy, -32602 en moderne), `resource_read_audited_as_resource_name`, `completion_code_only_max_100_values`, `completion_rate_limited_10_per_second`, `prompts_are_in_french`, `ttl_and_cache_scope_modern_only` ; C-15 étendu, C-31, C-32 | L | Opus |
| V3-08 | M9 : index et port (T-M9-03) | `crates/lf-adapter-dila/src/**` | l'ingestion | `DilaCorpus` (opérations non couvertes : `CorpusError` de 00 §5) | `contract_on_dila_corpus`, `french_analyzer_folds_accents`, `index_runs_in_spawn_blocking` | L | Opus |
| V3-09 | Docs de sécurité et licences (T-MD-13) | `SECURITY.md`, `PRIVACY.md` (V-165 selon D22), `CONTRIBUTING.md`, `about.toml`, `about.hbs`, `THIRD_PARTY_LICENSES.md` | `cargo-about` à une version épinglée | — | `cargo about generate` reproductible | M | Sonnet |
| V3-10 | README (T-M5-12, T-MD-12) | `README.md` | les variables de V1-11 | — | installation par canal, configuration des clients, hôtes contactés, attribution, non-affiliation | M | Sonnet |
| V3-11 | Recette des URL (T-M5-10, partie faisable) | aucun fichier : un rapport JSON rendu à l'intégrateur | Playwright MCP | statuts de V-103, V-139 à V-144 et Q17 | chaque URL des fixtures ouverte, et son verdict | S | Sonnet |
| V3-12 | Contrat étendu aux 5 opérations de M6 et M7 | `app/contract.rs`, `crates/lf-adapter-piste/tests/contract.rs`, `crates/lf-adapter-cache/tests/contract.rs` | les méthodes M6 et M7 de V2 | `corpus_contract` sur les 12 opérations | `contract_on_{fake,piste,cached}_corpus` verts sur les 12 opérations | S | Sonnet |

### V4 : dernière vague, abandonnable (4 unités ; 2,5 à 3 h)

**Préambule de l'intégrateur** (30 à 45 min) :

- Dépendances de M8 :
  - `axum 0.8` ;
  - la feature `transport-streamable-http-server` de rmcp ;
  - `tower` en dev ;
  - une feature `http` dans `lf-adapter-mcp` et dans `lf-app` (`http = ["lf-adapter-mcp/http"]`).
- `axum` reçoit ses `wrappers` dans `deny.toml`.
- Stubs :
  - `mcp/http.rs` : `pub async fn serve_http<S: ServerHandler + Clone>(make: impl Fn() -> S, opts: HttpOptions, shutdown: CancellationToken) -> Result<ServeOutcome, ServeError>` ;
  - `bin/http_serve.rs` : `pub(crate) async fn run(service: AppService, opts: HttpOptions, shutdown: CancellationToken) -> anyhow::Result<ExitCode>` ;
  - l'arm `Transport::Http` de `wiring.rs`, qui appelle `http_serve::run`.
- `make deny min-age`, puis `MIN_AGE_LOCKFILE=fuzz/Cargo.lock make min-age` si `fuzz/Cargo.lock` a changé.

| Id | Titre | Fichiers exclusifs | Consomme | Produit | Tests et critère de fin | Taille | Modèle |
|---|---|---|---|---|---|---|---|
| V4-01 | Tranche `fr_resolve`, plus les cas M6 et M7 des budgets | `T(resolve)`, `crates/lf-adapter-mcp/tests/budgets_m6.rs` | `resolve_ref` | `fr_resolve` | `at_most_5_candidates_best_first`, `interpreted_as_present`, `date_defaults_today_paris` ; le plafond de réponse est tenu pour verify, related et resolve | M | Sonnet |
| V4-02 | M9 : sélection et configuration (T-M9-04) | `bin/{wiring,config}.rs` | `DilaCorpus` | `AnyCorpus` (interne à `wiring.rs`), feature `dila` | `dila_feature_selects_dila_corpus`, `dereferenced_and_inap_excluded` (V-180) | M | Sonnet |
| V4-03 | M8 : transport HTTP et ses tests (T-M8-02, T-M8-03) | `mcp/http.rs`, `bin/{main,cli,http_serve,http_config}.rs`, `crates/lf-adapter-mcp/tests/http.rs` (complément) | le stub `serve_http` | écoute sur `127.0.0.1` seulement ; C-40 à C-60 | Module de test de `http.rs` (`tower::ServiceExt::oneshot`, pour la couverture) : `get_and_delete_are_405`, `allowed_hosts_enforced`, `http_missing_version_header_rejected`, `http_unsupported_known_version_header_rejected`, `http_origin_forbidden`, `http_body_too_large_413`, `http_local_token_required` | L | Opus |
| V4-04 | M8 : conformité officielle (T-M8-04) | `.github/workflows/conformance-http.yml`, `crates/lf-adapter-mcp/examples/conformance_server.rs`, `tests/conformance-baseline.yml` | `serve_http` (signature gelée) | job de conformité, version épinglée | Compile contre le stub, `make workflow-lint` vert, chaque entrée de la base justifiée. Le passage se lit en CI après la fusion de V4-03 (V-42). | M | Sonnet |

**Abandon.** Si V4-03 n'est pas vert à la boîte de temps, V4-03 et V4-04 ne sont pas fusionnés et le préambule M8 est annulé par `git revert`. V4-01 et V4-02 sont alors fusionnés seuls.

## 5. Intégration de fin de vague

L'intégrateur est un seul agent, dans le worktree principal.

1. **Périmètre.** Pour chaque `UNIT_RESULT`, vérifier `outside_allowlist`. Un fichier existant modifié hors liste est remis à son état de `BASE` (`git checkout <BASE> -- <f>`). Un fichier créé hors liste est retiré (`git rm`). Le besoin passe dans `shared_requests`.
2. **Fusion.** Dans l'ordre du tableau : `git merge --squash <branch>`, puis un commit au titre de D2, avec les lignes d'attribution de la session.
3. **Nettoyage immédiat**, que le propriétaire ne voie qu'une branche : `git worktree remove --force <worktree>` puis `git branch -D <branch>` pour chaque unité fusionnée, ensuite `git worktree prune` et `rm -rf /var/tmp/lf-fin/t-*`. Contrôle : `git for-each-ref --format='%(refname:short)' refs/heads` rend exactement les 13 branches présentes avant V0. Une unité non fusionnée (D20) garde sa branche jusqu'à sa reprise.
4. **`shared_requests`.**
   - Réexports de `lib.rs`, `ToolName::ALL` étendue aux outils enregistrés, constante `SURFACE`.
   - Cibles du `Makefile` : la cible `instructions` en V1 et la cible `lock` en V3, ajoutées à `ci`.
   - Retirer de `ignored` les dépendances que machete voit maintenant employées.
   - Ajouter les dépendances demandées (verrou, `min-age`, `deny`).
5. **Contrôle de vague** : `lf-all make fmt-check lint test doc arch purity machete`.
   - Ajouter `make deny min-age` si un verrou a changé.
   - À partir de V3, ajouter `cargo xtask lock-descriptors --update`, puis `--check`.
   - Les commits squash permettent de bisecter par unité. Une correction revient à l'intégrateur, ou à un agent dont la liste blanche se limite au fichier fautif.
   - Ensuite, base chaude (§2.2).
6. **Revue groupée et couverture informative, en parallèle.**
   - La revue groupée est confiée à un agent `code-reviewer` (Sonnet), sur `git diff <BASE>..HEAD`, avec la liste des unités et le Review Focus.
   - Pendant ce temps, aux intégrations de V2 et V3, l'intégrateur lance `lf-all make cov`, non bloquant. Les fichiers sous 100 % deviennent des unités S de la vague suivante, ou des entrées de F3.
7. **Corrections** : un agent, constats critiques et importants seulement. Les mineurs sont consignés pour F6.
8. **Unité `blocked`.**
   - Si une interface gelée est fausse, l'intégrateur la corrige par un commit, puis relance l'unité seule, depuis la nouvelle `BASE` et sa branche rebasée.
   - Sinon, elle est reportée à la vague suivante, ou coupée selon le §8.
9. **Push** de `feat/fin-du-projet`, à partir de V1.
   - **Après V1** : ouvrir la PR en brouillon par le MCP `github` (`create_pull_request`, avec le modèle `.github/pull_request_template.md`).
   - Les résultats CI d'une vague se lisent au début de la suivante. Un échec propre à un OS devient une unité.

**Étape I-R, à l'intégration de V1 seulement** (D17 ; 30 à 45 min, pendant la revue) :

1. Ajouter à `crates/lf-adapter-piste/tests/fixtures/plan.toml` des sondes nommées `m6/*` et `m7/*` (identifiants connus des fixtures existantes) pour les points d'accès suivants :
   - `/list/code` ;
   - `/consult/getArticleWithIdAndNum` ;
   - `/search` EXACTE sur `NUM_ARTICLE` ;
   - `/search` avec la facette ECLI ;
   - `/consult/getJoWithNor` ;
   - `/consult/kaliContIdcc` ;
   - `/consult/relatedLinksArticle` ;
   - `/consult/canonicalArticleVersion`.
2. Lancer `set -a; . ~/.config/mcp-legifrance/piste.env; set +a; cargo xtask record-fixture --plan crates/lf-adapter-piste/tests/fixtures/plan.toml --only m6/`, puis la même commande avec `--only m7/`.
3. Relire chaque fixture, sans secret. Mettre à jour `RECORDED` (`piste/integrity.rs:365`) et les sidecars `.meta.toml`.
4. Faire un seul commit, avant de lancer V2.

Si la sandbox est indisponible, ou si le propriétaire refuse (§7), V2-08 et V2-09 écrivent des corps synthétiques, tirés de 04 §7 et de l'annexe C, dans les modules de test de leurs propres fichiers. Leurs DTO sont alors tolérants, et les sondes sont jouées en F2.

## 6. Fin : PR unique et contrôles finaux (4 à 6 h, dont 1 à 2 cycles de CI)

- **F0. Décision de coupe**, selon le §8.
  - Un jalon coupé est retiré par `git revert` de ses commits d'unité. Une méthode de port implémentée et testée peut rester.
  - Si M9 est coupé, `crates/lf-adapter-dila` sort de `members` et ses dépendances partent avec lui, de même que le `skip` `fs4@0.13.1` de D21 et le wrapper `tantivy`.
- **F1. Échafaudage.**
  - Plus aucun `STUB(V0)` ni aucun `ignored` machete.
  - `multiple-versions = "deny"` (T-M5-09), chaque `skip` justifié (dont `fs4@0.13.1`, D21 ; syntaxe des `skip` vérifiée par context7).
  - Version selon D14.
  - `ToolName::ALL` et `SURFACE` finals ; verrou et manifeste MCPB régénérés.
  - Corriger le commentaire de `audited.rs:129-131` (réponse d'un appel annulé) d'après le verdict de 0A.
- **F2. Sondes live en sandbox** (D17) : `cargo nextest run --profile live --run-ignored only`, avec `piste.env`. Elles couvrent :
  - T-M4-13 (`live_e2e_reference_scenarios`) ;
  - T-M5-06 (V-75, V-82, V-83) ;
  - `live_m6` et `live_m7`.

  Les constats vont en F6, et aucune sortie brute n'est commitée.
- **F3. `make ci` complet**, couverture bloquante comprise. Si la couverture manque : un workflow de balayage, un agent par crate en défaut (au plus 6), dont la liste blanche se limite aux fichiers `src/` de ce crate pour leurs `mod tests`. On estime ce balayage à 1 à 2 h, puisque la règle T est appliquée depuis V1. Aucune exclusion nouvelle sans justification de « colle d'E/S irréductible » (06 §7.6).
- **F4. Revue sécurité unique** : agent `security-auditor` (Opus) sur `git diff origin/main...HEAD`.
  - Points examinés : les secrets (configuration, journaux, audit, `Debug`), l'entrée stdio, les permissions du journal d'audit, la désérialisation, le transport HTTP s'il est livré, et les workflows de release.
  - Corriger les constats critiques et importants, puis relancer `lf-all make lint test`.
- **F5. Push et passage de la PR en « ready ».** La CI doit être verte : `check`, `test` sur trois OS, `coverage`, `mutants (diff de la PR)` (D7), `msrv`, `deny` et `workflow-lint`. Sonar reste informatif.
  - Compter 1 à 2 h par cycle (mutants de 25 à 75 min par shard, `test` jusqu'à 45 min).
  - Un mutant `MISSED` donne `make mutants FILE=<fichier>` en local, puis un test (D8).
- **F6. Documentation groupée**, par un agent propriétaire de `docs/plan/*`, `docs/adr/*` et `CHANGELOG.md` :
  - 07 §4.3 (tableau d'état, l.1087) et cases `T-*`, d'après l'annexe A ;
  - 00 §14 (amendements D5, D7, D9 à D13, D21, ADR-0018) et 00 §13 (défauts datés du §1) ;
  - CHANGELOG, rubrique `Tool surface` ;
  - corrections connues : 05 cite `e.into_message()`, qui n'existe pas (`error.rs:158` : `message()`) ; 02 §6.1 dit `Option<u32>` là où 05 §3.3 fixe un i64 tolérant ; 06 §2.4 contredit `arch.rs:227` sur les dépendances de `lf-adapter-cache` ; 02 §5.1 sur `find_article` (D5) ;
  - 09 : statuts des V-xx levées ou consignées, constats de F2 et de V3-11 ;
  - annexe D §9.1 : statut des bugs rmcp (T-M4-09), lu par le MCP `github` ;
  - 06 §7.6 : périmètre de mutation de D7 et dette hebdomadaire ;
  - ADR-0014 acceptée si les mesures de F2 la confirment.
- **F7. Remise au propriétaire.** Il relit et fusionne lui-même. Le plan lui propose, sans rien supprimer d'office, la liste des 11 branches locales anciennes et des 6 worktrees anciens (§7).

## 7. Actions du propriétaire et de tiers

**Ce que les agents font seuls** : V0 à V4 et F0 à F6, sans aucun accès aux identifiants. Seul l'intégrateur lance les sondes sandbox, à l'étape I-R et en F2.

| Action | Quand | Défaut appliqué en attendant | Ce qu'elle bloque |
|---|---|---|---|
| Valider ce plan et arrêter le travail en cours (le spike entamé dans `spikes/rmcp-stdio` sera repris par 0A) | maintenant | — | V0 |
| Lancer la session dans `~/mcp-ligitfrance-m3a-exec`, en mode ultracode | maintenant | — | toutes les vagues |
| Accepter les sondes sandbox de l'intégrateur (I-R et F2, `piste.env`, au plus 1 requête par seconde) | avant la fin de V1 | **acceptées** (compte sandbox déjà employé au plan 2h) | si refus : DTO M6 et M7 écrits sur des corps synthétiques, et V-75, V-82, V-83 restent ouvertes |
| Compte PISTE production (Q3) | avant la release | aucune sonde de production | release v0.1.0 (T-M5-13) |
| Essais dans Claude Code, Claude Desktop, Cursor et VS Code (V-46, V-51, V-52, V-55 ; protocole de l'annexe D §10.3) | après V3 | non faits, consignés « à faire » | la release, pas la PR |
| Mesures V-47, V-48 et V-58 (`count_tokens` exige une clé API) | quand il veut | ratio de 4 caractères par jeton supposé | ADR-0014 « Accepté » |
| Poste Windows 11 réel : V-164, et V-165 (ACL du journal d'audit) | quand il veut | ACL héritées, documentées comme non vérifiées dans `PRIVACY.md` (D22) | la levée de V-165 dans 09 |
| Certificats de signature (Q5) | — | binaires non signés | T-MD-08, T-MD-09 |
| Relecture juridique par un tiers (T-M6-08, V-146 à V-159, V-163) | après la PR | cas non garantis exclus | réexamen de Q12 |
| Protection de `main` : rien à changer (D7 garde le nom du check `mutants (diff de la PR)`) | — | réglage actuel | — |
| Fusionner la PR, étiqueter, publier (GHCR, registre MCP) | après F5 | aucune release | les releases |
| Supprimer, s'il le veut, les branches et worktrees anciens (proposés, jamais supprimés d'office). Branches : `docs/identifiants-piste`, `docs/m2-cloture`, `docs/m3a-cloture`, `dryrun/m2h`, `feat/m2h-outillage`, `feat/m2h-sondes`, `feat/m3a-socle`, `plan/m2h-outils-sondes`, `plan/m3a-socle`, `spec/fonctions-a-valeur`, `wip/durcissement-2026-10-19`. Worktrees : `~/mcp-ligitfrance-m2-cloture`, `-m2h`, `-m2h-outillage`, `-m2h-sondes`, `-m3a`, `/var/tmp/m2h-dry`. | après F7 | conservés | rien |
| Q4 (CGU PISTE pour un mode partagé) | — | profil local seul | T-M8-06 |

## 8. Ligne de coupe et estimation

H désigne le début de V0. Les durées murales supposent un enchaînement sans pause, nuit comprise. Chaque intégration compte 45 à 75 min : fusions, contrôle, revue en parallèle de `make cov`, corrections, push. Ce sont des estimations, non des mesures.

| Vague | Unités | Agents / compilations simultanées | Durée murale | Cumul depuis H |
|---|---|---|---|---|
| V0 | 5 (0A, 0B, 0C, 0M, 0W) | 2 / 4 | 5 à 7 h | 5 à 7 h |
| V1 | 16 | 16 / 4 | 4 à 5 h | 9 à 12 h |
| V2 | 16 | 16 / 4 | 4 à 5 h | 13 à 17 h |
| V3 | 12 | 12 / 4 | 3,5 à 4,5 h | 16,5 à 21,5 h |
| V4 | 4 | 4 / 4 | 2,5 à 3 h | 19 à 24,5 h |
| Fin | jusqu'à 6 agents de balayage, 1 revue, 1 agent docs | 6 / 4 | 4 à 6 h, dont 1 à 2 cycles de CI | **23 à 30,5 h** |

Total : **53 unités**, dont 5 en V0. Largeur parallèle maximale : **16 agents, 4 compilations**.

**Réalisme de « fini demain ».** Le périmètre complet (M3 à M9) demande 23 à 30,5 h sans pause. Il **ne sera probablement pas fini** le 2026-10-30. La v0.1 seule demande V0, V1, V2, V3 réduite à V3-01, V3-02, V3-03, V3-09 et V3-10 (environ 3 h), puis la fin : soit **20 à 26,5 h**, ce qui tient d'ici la fin du 2026-10-30 à condition de démarrer tôt le 29 et d'enchaîner la nuit.

**Ligne de coupe**, par ordre de sacrifice (probabilités estimées, non mesurées) :

| Niveau | Contenu | Commentaire |
|---|---|---|
| **Certain d'ici demain soir** | V0, V1 et V2 intégrées et vertes en local : tout le code de la v0.1 est écrit et testé. | — |
| **Probable (~70 %)** | **PR v0.1 verte en CI** : M3, M4, M5 hors mesures client, distribution sans signature. | C'est l'engagement « fini demain ». |
| **Possible (~50 %)** | M7 : `fr_diff_article`, `fr_search_all`, `fr_get_related`, resources, prompts. | Ses unités sont réparties sur V2 et V3. |
| **Risqué (~35 %)** | M6 complet. | Coupe interne admise : les familles §4.2 à §4.10 seulement (codes, textes, juridictions, ECLI), avec un corpus vert sur ces familles ; les autres sont « non reconnues ». |
| **Probablement hors délai (≤ 25 %)** | M9 et M8. | — |

**Points de décision horodatés.** Chacun se décide à la fin d'une intégration, de façon automatique, sans consulter le propriétaire.

| Condition | Conséquence |
|---|---|
| Intégration de V1 finie après H+13 | M9 est coupé : ni V2-15, ni V3-08, ni V4-02. Le rapport de V1-16 est gardé. |
| Intégration de V2 finie après H+18 | V3 se réduit à V3-01, V3-02, V3-03, V3-09 et V3-10, puis on passe à la fin. M6 et M7 sont coupés côté outils ; les méthodes de port vertes restent. |
| V2-13 rend `blocked` | M6 passe en coupe interne. |
| Intégration de V3 finie après H+22 | V4 est abandonnée et on passe à la fin. |
| Pendant V4 | La règle d'abandon de M8 (§4) s'applique. |

## 9. Risques

| Risque | Parade |
|---|---|
| Spike KO (V-28, génériques, V-29) | Replis écrits par 0A pour chaque gel ; une heure de plus sur V0. |
| OOM sous WSL | 4 jetons, mémoire vérifiée sous le jeton (8 Gio), `line-tables-only`. Au premier incident, 3 jetons, et ne jamais relancer la même commande. |
| Attente des jetons plus longue que prévu | Itérer avec `nextest` filtré et `clippy` une fois à la fin. D20 borne la vague ; une unité qui déborde est reportée. |
| Interface gelée fausse | Une unité `blocked` ne contourne pas : correctif de l'intégrateur et relance ciblée (§5.8). |
| Couverture qui déborde en fin | Règle T dès V1, `make cov` informatif en V2 et V3, balayage F3 par crate. |
| Mutants CI trop longs | Liste de D7 et 4 shards ; si un shard dépasse 120 min, retirer de la liste le fichier le plus coûteux (consigné en F6). |
| Échecs propres à un OS (chemins Windows, `fs4`, signaux, tantivy sous Windows) | PR brouillon dès V1, CI lue à chaque vague. |
| Bugs de rmcp (#1337, #1347, #1348…) | Transcriptions qui figent le comportement (V2-06), statut lu en F6. |
| Parseur M6 sous-estimé | Découpé sur trois vagues et sept unités, coupe interne prévue. |
| Une unité touche un fichier partagé | Liste blanche vérifiée à l'intégration ; fichier remis à `BASE` ou retiré (§5.1). |
| Formes M6 et M7 inattendues | Fixtures I-R avant V2, et DTO tolérants à défaut. |
| Sandbox indisponible ou quota | I-R et F2 reportées, V-xx consignées ouvertes ; la PR n'en dépend pas. |
| Dumps DILA (taille, disponibilité, licence) | LEGI seul, extraits minimaux versionnés ; M9 coupable sans toucher à la v0.1. |
| Revue allégée qui laisse passer un défaut | Revue sécurité unique en fin, mutation CI des fichiers à risque, scénarios du Review Focus. |
| Disque (jusqu'à environ 130 Go de builds par vague) | Purge des `t-*` à chaque intégration ; 683 Go libres. |
| Worktrees du harnais placés sous le dépôt (`.claude/worktrees/`), donc vus par `make machete` | Nettoyage (§5.3) avant le contrôle de vague (§5.5). |

## 10. Critiques rejetées ou adaptées

Tous les constats critiques et importants des deux relectures sont pris en compte. Les cas ci-dessous ne reprennent pas le correctif proposé à la lettre.

| Constat | Correctif proposé | Ce que fait le plan, et pourquoi |
|---|---|---|
| Couverture `--lib` (relecture 2, critique) | Variante : retirer `--lib` de `COV_FLAGS` et amender ADR-0002. | **Rejeté.** Cette variante ferait entrer dans la mesure les binaires et les tests `#[ignore]` (live), donc du code jamais exécuté en CI. Elle contredit aussi ADR-0002 l.18 et 06 l.86. La règle T est plus légère et suit déjà 06 §1.4. |
| Couverture `--lib` (relecture 2) | `cargo llvm-cov -p <crate>` à chaque vague. | **Adapté** : un seul `make cov` informatif aux intégrations de V2 et V3, en parallèle de la revue, pour ne pas allonger le chemin critique. |
| WSL (relecture 1) | « Les unités qui touchent lf-app, sauf V4-02, passent sans la feature `dila`. » | **Rejeté.** La base chaude est bâtie en `--all-features`, tantivy compris. Retirer une feature changerait l'unification des features et recompilerait les dépendances, c'est-à-dire ce que le même constat veut éviter. |
| WSL (relecture 2) | `MAX_PAR` ramené à 4 au premier OOM. | **Adapté** : c'est le nombre de jetons qui borne la mémoire, pas le nombre d'agents. On passe de 4 à 3 jetons au premier incident. |
| `read` en mode résumé (relecture 1, a) | Liaison par l'intégrateur de V1, ou une unité V2-00. | **Adapté** : V1-06 relie les trois modes aux signatures gelées de `summary` et `Focus`, avec des tests structurels indépendants de l'algorithme. Aucune unité ni aucun code d'intégrateur en plus. |
| `budget.rs` sans propriétaire (relecture 1) | `degrade.rs` et `fit_under_cap` confiés à V1-06. | **Adapté** : les aides de dégradation (`estimate_chars`, `shrink_from_end`, `strip_from_end`) et `fit_under_cap` sont **réelles dès V0** (0C, 0W), car V1-01 et V1-02 en ont besoin dans la même vague que V1-06. |
| Contrat M6 et M7 (relecture 1, c) | L'intégrateur de V2 ajoute les 5 opérations. | **Adapté** : une unité S, V3-12, plutôt que du code écrit par l'intégrateur. |
| `fs4` et tantivy (relecture 1) | `{ crate = "fs4@1", wrappers = [...] }`. | **Adapté** : `tantivy` est ajouté aux `wrappers` de `fs4`, ce qui évite une syntaxe de version non vérifiée. Le `skip` de F1 est conservé. |
| Cas d'usage uniformes (relecture 2) | Variante : constructeurs propres à chaque outil gelés en V0. | **Option 1 retenue** : `service/<outil>.rs`, propriété de la tranche. La tranche choisit son constructeur, et seul l'état par processus (`CompleteGate`) est gelé. |
| Fixtures M6 et M7 (relectures 1 et 2) | Corps synthétiques seulement (relecture 1), ou enregistrement à la fin de V0 (relecture 2). | **Combiné** : enregistrement à l'étape I-R de l'intégration de V1, en parallèle de la revue et hors du chemin critique de V0, avec repli sur des corps synthétiques. |
| Noms de l'annexe B (relecture 2) | Aligner sur §1.2, ou fournir une table de correspondance. | **Aligné** sur §1.2 et 03 l.140. `scan.rs` porte les ancres et `post.rs` l'arbitrage. Une unité V2-14 (`format`, `quote`) est ajoutée pour décharger V2-13. |

## Annexe A : correspondance des tâches de 07 avec les unités

| Tâches de 07 | Unités, ou sort |
|---|---|
| T-M3-01 | 0C (port complet, `ArticleAt`) |
| T-M3-02, T-M3-03 | livrées au plan 3a ; rejouées par les tranches |
| T-M3-04 | V1-01 à V1-03, V2-01 à V2-03 |
| T-M3-05 | 0C (`service/mod.rs`), V2-05 et V3-12 (contrat) |
| T-M3-06 | V1-07 |
| T-M3-07 | V1-08 |
| T-M3-08 | V1-12 |
| T-M3-09 | défaut Q9, consigné en F6 |
| T-M3-10 | V1-03 et 0B (`clippy.toml`) |
| T-M4-01 | V1-10 |
| T-M4-02 | V1-09 et 0W |
| T-M4-03 | les tranches |
| T-M4-04 | les tranches et V3-02 |
| T-M4-05, T-M4-06 | V1-10 |
| T-M4-07 | V1-11, V1-12, V2-04, V3-01 |
| T-M4-08 | V2-06, V3-01 |
| T-M4-09, T-M4-10, T-M4-14 | F6 |
| T-M4-11 | V1-13, V3-02 |
| T-M4-12 | propriétaire |
| T-M4-13 | V3-01 et F2 |
| T-M4-15 | 0B |
| T-M5-01 | 0C (`budget/mod.rs`), V1-06 |
| T-M5-02 | V2-01 à V2-03 |
| T-M5-03 | V1-01, V1-02 |
| T-M5-04, T-M5-05 | V3-03 |
| T-M5-06 | V2-05 et F2 |
| T-M5-07 | V3-01 (V-18) et propriétaire |
| T-M5-08 | F6 |
| T-M5-09 | F1 |
| T-M5-10 | V3-11, F2 et propriétaire |
| T-M5-11 | défauts du §1 |
| T-M5-12 | V3-09, V3-10 |
| T-M5-13 | propriétaire |
| T-M5-14 | V1-04, V1-05, V1-06, avec D12 |
| T-M6-01 | 0C (`NumberKind`, `ArticleAt`) et D5 |
| T-M6-02 | V1-14, V2-10 à V2-14 |
| T-M6-03 | V3-04 |
| T-M6-04 | I-R, puis V2-08 |
| T-M6-05, T-M6-06 | V3-05, V4-01 |
| T-M6-07 | F2 (partiel) |
| T-M6-08 | tiers |
| T-M6-09 | propriétaire |
| T-M7-01 | défauts du §1 |
| T-M7-02 | I-R, puis V2-09 |
| T-M7-03 | V2-07, V3-06 |
| T-M7-04, T-M7-05 | V3-07 |
| T-M7-06 | intégration de V3 et F1 |
| T-M7-07 | F2 (partiel) |
| T-M7-08 | propriétaire |
| T-M8-01 | défaut Q4 |
| T-M8-02, T-M8-03 | V4-03 |
| T-M8-04 | V4-04 |
| T-M8-05 | propriétaire |
| T-M8-06 | non fait (Q4) |
| T-M8-07 | V3-09 (`SECURITY.md`), sans release |
| T-M9-01 | V1-16 |
| T-M9-02 | V2-15 |
| T-M9-03 | V3-08 |
| T-M9-04 | V4-02 |
| T-M9-05 | défauts du §1 |
| T-M9-06 | propriétaire |
| T-MD-01, -03, -04, -07, -11 | V1-15 |
| T-MD-02 | 0B |
| T-MD-04 (packaging), -05, -06 | V2-16 |
| T-MD-08, -09 | non faits (Q5) |
| T-MD-10 | propriétaire |
| T-MD-12 | V3-10 |
| T-MD-13 | V3-09 |
| T-MD-14, -15 | F1 (manifeste régénéré), sans release |
