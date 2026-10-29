# ADR-0018 — Dépendances de la fin du projet : déclarées en une fois, par la vague 0

- **Statut** : **Proposé** — accepté à la fusion de la PR unique du plan fin-du-projet.
- **Date** : 2026-10-29
- **Jalons concernés** : M3 à M9 (plan `docs/superpowers/plans/2026-10-29-fin-du-projet-parallele.md`, vague 0, unité 0B)
- **Questions liées** : —
- Révisé le 2026-10-29 (F6-b, fin de projet) : les exceptions `STUB(V0)` de `cargo machete` ont disparu (décision 1) ; `winnow` n'est pas employé (décision 3) ; `fs4@0.13.1` est un `skip` de `deny.toml` et `multiple-versions` vaut `deny` (décision 5, T-M5-09). Le statut reste « Proposé » jusqu'à la fusion de la PR unique.
- Amende [00-decisions.md](../plan/00-decisions.md) §2 (tableau des crates par membre) et §11.1 (liste blanche de `lf-domain`), [02-architecture.md](../plan/02-architecture.md) §3.0.1 (`similar`), §3.0.2 (les squelettes ne déclaraient pas de dépendance d'avance) et §4.3 (`wrappers`), et [06-qualite.md](../plan/06-qualite.md) §7.6 et §9.1 (mutation en CI, `deny.toml`).

## Contexte

- **Une campagne en parallèle.** Le plan fin-du-projet écrit M3b à M9 en quatre vagues de 4 à 16 unités qui travaillent en même temps, chacune dans son worktree. Un `Cargo.toml` ou un `Cargo.lock` modifié par deux unités serait un conflit certain, et une dépendance ajoutée en cours de vague obligerait toutes les autres à recompiler.
- **La règle de 00 §2 ne tient plus.** Jusqu'ici, un crate ne déclarait une dépendance que lorsqu'un code l'employait, pour que `cargo machete` reste vert. Cette règle suppose des jalons séquentiels. Elle est remplacée, le temps de la campagne, par une déclaration unique en vague 0 et une liste d'exceptions pour `cargo machete`.
- **Aucune dépendance de M8.** `axum`, le transport HTTP de rmcp et `tower` ne sont pas déclarés : M8 est abandonnable et ses dépendances arrivent au préambule de V4, si M8 est retenu.

## Décision

1. **Toutes les dépendances, normales et de développement, sont déclarées en vague 0 (D9)**, dans les manifestes des membres et dans `[workspace.dependencies]`. Celles qu'aucun code n'employait encore ont été listées dans `[package.metadata.cargo-machete] ignored`, avec le commentaire `STUB(V0)`, **le temps des vagues** : à la fin du projet, aucune ligne `STUB(V0)` ni aucune exception `cargo machete` ne subsiste dans `crates`, `xtask` ni les manifestes (`grep -rn "STUB(V0)" crates xtask Cargo.toml deny.toml fuzz` ne trouve rien au 2026-10-29). Les verrous (`Cargo.lock`, `fuzz/Cargo.lock`) ne bougent plus de V1 à V3 ; une dépendance manquante passe par une `shared_request` à l'intégrateur.
2. **Nouveaux membres.** `lf-adapter-cache`, `lf-adapter-audit`, `lf-adapter-mcp`, `lf-app` (paquet `mcp-legifrance`, binaire seul) et `lf-adapter-dila`. Ce dernier reçoit sa ligne dans `RULES` de `check-arch` (9 règles) : internes `lf-application` et `lf-domain`, et ses portes R6 (`tantivy`, `quick-xml`, `tar`, `flate2`) deviennent actives. `mcp-legifrance` l'admet, derrière sa feature `dila`, absente des binaires publiés.
3. **`DOMAIN_ALLOWED` s'élargit (R1)** à `winnow`, `aho-corasick`, `regex` et `unicode-normalization`, les quatre bibliothèques prévues pour le parseur de citations de M6 (03 §4). **Écart au livré (2026-10-29) : `winnow` n'est pas employé.** Les grammaires de `lf-domain` sont écrites à la main, avec un curseur sur une vue en caractères ; `lf-domain` déclare `thiserror`, `aho-corasick`, `regex` et `unicode-normalization`, et `winnow` n'y figure pas (il reste dans la liste admise de `xtask/src/arch.rs`, où il ne coûte rien, et au verrou seulement comme dépendance transitive d'autres paquets). `lf-domain` reste sans `serde`, sans `tokio`, sans `chrono` et sans dépendance interne. Chacune est sans E/S et sans horloge.
4. **`similar` en 2.7 (D10)**, déjà au verrou par `insta`, au lieu de 3.2 (02 §3.0.1) : pas de doublon.
5. **`fs4` en deux versions (D21).** `tantivy` 0.26.2 tire `fs4 ^0.13.1` par sa feature `mmap`, que M9 exige pour l'index sur disque. `lf-adapter-audit` prend `fs4` 1.1. Dans `deny.toml`, `tantivy` figure parmi les `wrappers` de `fs4` ; `multiple-versions` vaut `deny` (T-M5-09) : `fs4@0.13.1` y figure parmi les huit `skip` nominatifs, chacun avec sa raison, sans `skip-tree`. Livré par F1 (2026-10-29), il remplace le « `warn` » de la rédaction d'origine.
6. **`tantivy` sans `columnar-zstd-compression`.** Cette feature par défaut tire `zstd` 0.13, `zstd-safe` 7.3 et `zstd-sys` 2.1 : quatre avis RustSec (RUSTSEC-2026-0391, -0398, -0403 et -0410), qui rendent `cargo deny` rouge, et leurs correctifs exigent `zstd` 0.14, hors de la plage de tantivy. Les features gardées sont `mmap`, `stopwords`, `lz4-compression` et `stemmer` ; la compression des documents reste lz4. Écart au « défauts de tantivy » de 02 §3.0.1, à lever quand tantivy montera `zstd`.
7. **Licences de `deny.toml`.** `MIT-0` est ajoutée à la liste exacte (`borrow-or-share`, tiré par `jsonschema` en développement et par `xtask`). `MPL-2.0` ne l'est pas : `option-ext` 0.2.0, tiré par `dirs-sys` puis `dirs` 6.0 (défaut de `LF_AUDIT_DIR`), reçoit une exception nominative, afin que tout autre paquet sous MPL-2.0 reste visible. MPL-2.0 est un copyleft de fichier : la dépendance, liée sans modification, n'impose rien au code du projet.
8. **`wrappers` de `deny.toml`** : les parents directs réels, relevés par `cargo tree -i <crate> -e normal,dev --all-features --depth 1` (`[graph] all-features = true`). `rmcp` : `lf-adapter-mcp` ; `rmcp-macros` et `schemars` : `lf-adapter-mcp` et `rmcp` ; `moka` : `lf-adapter-cache` ; `sha2` : `lf-adapter-audit` ; `fs4` : `lf-adapter-audit` et `tantivy` ; `tantivy`, `quick-xml` et `tar` : `lf-adapter-dila`.
9. **`rmcp` et `rmcp-macros` restent épinglés `=3.5.1`**, et `check-arch` le contrôle désormais (`PINNED`, T-M4-15) : `rmcp` déclare une exigence caret sur `rmcp-macros`, de sorte qu'un `cargo update` pourrait les désassortir.
10. **Feature `test-support` de `lf-adapter-mcp`** (`test-support = ["rmcp/client"]`). Le harnais `testing.rs` est un client rmcp en processus : il active `rmcp/client`, qui ne doit jamais entrer dans le binaire. Comme `lf-application/test-support`, la feature rejoint `DEV_ONLY_FEATURES` (R10) : seul un `[dev-dependencies]` d'un autre membre peut l'activer, et aucune autre feature du propriétaire ne la liste.
11. **Job CI de mutation (D7).** La matrice `mutants-shard` passe en 4 tranches (`--shard k/4`, 120 min chacune, `-j 2`). Ses mutants se limitent aux fichiers à risque de `.github/mutants-scope.txt` (un `--file` par ligne), que `--in-diff` restreint encore au diff de la PR. Le job agrégateur garde le nom `mutants (diff de la PR)` : la protection de branche ne change pas. Une étape échoue si une ligne de la liste ne désigne aucun fichier. Les fichiers hors liste ne sont pas mutés avant la fusion : le premier `weekly.yml` signalera leurs `MISSED`, dette traitée après la PR.
12. **Lints et `clippy.toml`.** `print_stdout`, `print_stderr` et `exit` passent en `warn` (stdout ne porte que du JSON-RPC, Q9), et `disallowed-methods` interdit `chrono::Utc::now`, `chrono::Local::now`, `std::time::SystemTime::now` et `std::env::{var, var_os, vars, vars_os}` (T-M3-10). Chaque site existant porte un `#[expect(…, reason)]`. Exceptions admises par la suite : `lf-app` (`clock.rs`, et `config.rs` pour `ProcessEnv`) et `xtask/src/main.rs`.
13. **Couverture.** `COV_FLAGS` exclut un seul fichier, `crates/lf-adapter-mcp/src/stdio.rs` (D6, 06 §7.6), réduit à la colle `serve_stdio` sur les vrais stdin et stdout et couvert par l'e2e du binaire. `.cargo/mutants.toml` l'exclut aussi.

## Conséquences

### Positives

- Les verrous et les manifestes ne bougent plus de V1 à V3 : aucune unité ne recompile à cause d'une autre, aucun conflit sur un fichier de dépendances.
- `check-arch` et `cargo deny` portent déjà toutes les règles de confinement : une dépendance mal placée par une unité est rouge dès sa première exécution.

### Négatives

- **`cargo machete` a porté des exceptions provisoires** (`STUB(V0)`), retirées à mesure que les unités employaient leurs dépendances ; le contrôle de F1 (`! grep -rn "STUB(V0)" crates xtask`) est vert.
- **tantivy se compile** dans la base chaude et en CI dès la vague 0 (feature `dila` de `mcp-legifrance`, activée par `--all-features`), pour un code qui n'arrive qu'avec M9.
- **Un doublon toléré** (`fs4` 0.13 et 1.1) et un `tantivy` privé de la compression zstd des colonnes.

### À surveiller

- Les avis RustSec sur `zstd` : si tantivy monte à `zstd` 0.14, rétablir `columnar-zstd-compression`.
- Âge des paquets (00 §2) : la vague 0 a rétrogradé, par `cargo update --precise`, `arc-swap` (1.9.2), `bon` et `bon-macros` (3.10.2), `erased-serde` (0.4.10), `memmap2` (0.9.11), `sketches-ddsketch` (0.4.1) et `unicode-normalization` (0.1.25). Elles pourront monter à partir du 2026-11-01.

### Tests qui la gardent

`rmcp_must_be_pinned_exactly`, `dila_member_has_a_rule`, `domain_allows_m6_parsers` et `mcp_test_support_is_dev_only` (`xtask/src/arch.rs`) ; `cargo deny check` (bans, licences, avis) ; `make min-age` ; l'étape « Vérifier la liste des fichiers à risque » du job `mutants-shard`.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Déclarer chaque dépendance au jalon qui l'emploie (règle de 00 §2) | Plusieurs unités éditeraient les mêmes manifestes et verrous en parallèle ; chaque ajout invaliderait la base chaude de compilation. |
| `similar` 3.2 | Doublon avec la 2.7 d'`insta`, déjà au verrou (D10). |
| `tantivy` avec ses features par défaut | Quatre avis RustSec par `zstd` (décision 6) ; le correctif exige une version hors de la plage de tantivy. |
| `MPL-2.0` dans la liste globale de licences | Tout futur paquet sous MPL-2.0 passerait inaperçu ; une exception nominative suffit. |
| Retirer `dirs` pour écarter `option-ext` | Le défaut de `LF_AUDIT_DIR` s'appuie sur `dirs::data_dir()` (02 §3.0.1). À rouvrir si l'exception devenait un obstacle. |
| Muter tout le diff en CI | Plus de 24 h de mutation (D7). |

## Références et sources

- Plan : `docs/superpowers/plans/2026-10-29-fin-du-projet-parallele.md` (§1 D6, D7, D9, D10, D18 et D21 ; §3, unité 0B) ; [00-decisions.md](../plan/00-decisions.md) §2, §11.1 ; [02-architecture.md](../plan/02-architecture.md) §3.0.1, §3.0.2, §4.3 ; [06-qualite.md](../plan/06-qualite.md) §7.6, §9.1 ; [07-feuille-de-route.md](../plan/07-feuille-de-route.md) T-M3-10 et T-M4-15.
- Code : `Cargo.toml`, `deny.toml`, `clippy.toml`, `Makefile`, `.cargo/mutants.toml`, `.github/workflows/ci.yml`, `.github/mutants-scope.txt`, `xtask/src/arch.rs`.
- Index crates.io (dates de publication, sparse) et base d'avis RustSec, relevés le 2026-10-29.
