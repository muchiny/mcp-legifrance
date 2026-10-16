# M0–M1 Fondations et domaine — plan d'implémentation

> **Numérotation historique.** Ce plan cite `docs/plan/00-decisions.md` dans sa numérotation du brouillon, antérieure à la fusion du plan complet (2026-10-16) ; elle est conservée telle quelle. Correspondance vers le 00 fusionné : §2 → §2 ; §4 Domaine → §4 ; §7 Jalons → §11 (écarts du livré : §11.1) ; §8 Questions → §13 ; **§9 Qualité → §10**. Questions : **Q9** (typographie des citations) → **Q16**, décidée ; **Q10** (`/codes/article_lc/` pour une loi non codifiée) → **Q17**, à vérifier en M2 (V-141) ; ces deux questions s'appelaient Q11 et Q12 dans la fusion de la PR #3. ADR : les ADR 0001 et 0002 cités ici sont ceux livrés (0001 remplacé par ADR-0016 ; 0002 fonde le §10) ; les ADR 0001/0002 du plan complet sont devenus 0015/0016. Les points différés par la revue de clôture de M1 sont tenus dans 09 §3.19 sous les identifiants M1-D1…M1-D24.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** poser le workspace Cargo et ses portes de qualité (M0), puis livrer le crate pur `lf-domain` complet (M1) : identifiants, fonds, dates, ECLI, URL, contrat et règles de citation, entités, utilitaires texte. Tests unitaires, couverture et mutation sont au maximum.

**Architecture :** workspace hexagonal (`docs/plan/00-decisions.md` §2). Ce plan crée seulement `lf-domain`, sans I/O ni async, avec `thiserror` comme unique dépendance, et `xtask`, qui vérifie la règle de dépendance. Chaque module naît en TDD : tests inline d'abord, puis code minimal, couverture à 100 %, mutation du fichier à 0 survivant.

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain `stable`), `thiserror` 2.0.21, `proptest` 1.12.0, `pretty_assertions` 1.4.1, `serde_json` 1 (xtask), `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants` 27, `cargo-deny`, `cargo-machete`, `cargo-fuzz` (nightly), GitHub Actions.

**Spec :** `docs/plan/00-decisions.md` (§0–§8, plus le §9 Qualité ajouté le 2026-10-15). Référence Python : `matematicsolutions/fr-eli-mcp` @ `d3b5b13`. Pour la lire, la cloner dans un dossier à part, hors du dépôt, et ne rien y exécuter.

**Essai à blanc (2026-10-15) :** l'intégralité du code de ce plan a été compilée et testée dans un workspace jetable, puis rejouée tâche par tâche dans l'ordre du plan : clippy, format et nombre de tests vérifiés à chaque état intermédiaire. Résultats : clippy `pedantic` `-D warnings` propre, 95 tests verts, 100 % des lignes, régions et fonctions couvertes (`--lib`), 337 mutants générés, **0 survivant** (303 tués, 34 non compilables), sans aucune exclusion. Les sorties « Expected » ci-dessous en viennent.

---

## Feuille de route des plans

Le programme M0→M9 réunit plusieurs sous-systèmes indépendants, d'où un plan par sous-système, chacun livrant un logiciel testable :

| Plan | Jalons | Prérequis bloquants |
|---|---|---|
| **1 (ce document)** | M0 Fondations + M1 Domaine | aucun |
| 2 | M2 Adapter PISTE (`lf-adapter-piste`, OAuth, retries, DTO, réparation d'encodage, wiremock + fixtures) | Q3 (compte PISTE sandbox), Q6 (réseau), Q10 |
| 3 | M3 Application (ports, cas d'usage, `Audited`, `lf-adapter-audit`, `lf-adapter-cache`, `Clock` / `today_paris`) | plan 2 |
| 4 | M4 Adapter MCP + `lf-app` (8 outils, descriptor lock, dérive des instructions) | rapports `08-mcp-2026-07-28.md` / `09-rmcp-2026.md` (absents du dépôt au 2026-10-15), Q7, Q8 |
| 5 | M5 Économie de contexte (s'appuie sur `text::window`, `passages`, `rank` livrés ici) + distribution → **v0.1.0** | plan 4, Q1, Q2, Q5 |
| 6+ | M6 Citations, M7 Navigation, M8 HTTP, M9 DILA | plan 5, Q4 |

Chaque plan hérite du §9 : ses tâches se terminent par `make cov` (100 %) et `make mutants FILE=…` (0 survivant).

## Global Constraints

- Rust edition 2024, `rust-version = "1.88"` (plancher imposé par rmcp 3.5.1 et proptest 1.12), toolchain `stable` avec `clippy`, `rustfmt` et `llvm-tools-preview`.
- Crates préfixés `lf-` ; identifiants Rust en anglais et ASCII ; docs et commentaires en français (§0) ; messages d'erreur en anglais (parité avec la référence) ; citations en français.
- `lf-domain` : dépendance normale unique `thiserror`, dépendances de dev `proptest` et `pretty_assertions`. Aucune I/O, aucun async, aucun `unsafe` (`unsafe_code = "forbid"`).
- Règle de dépendance du §2 vérifiée par `cargo xtask check-arch` (`make arch`).
- Lints : `clippy::pedantic` en `warn`, CI en `-D warnings`. `unwrap` et `expect` sont permis dans les tests seulement (`clippy.toml`).
- Toute règle portée de la référence cite `fichier:ligne` (§0). Les divergences assumées sont écrites en commentaire, avec la raison.
- Qualité (§9) : TDD ; `make cov` = 100 % des lignes, régions et fonctions ; `make mutants FILE=<fichier touché>` = 0 `MISSED`. Pas d'exclusion sans preuve d'équivalence.
- WSL (`~/.claude/rules/wsl-safety.md`) : jamais `cargo test -j 4` ni plus. `cargo mutants` uniquement par `make mutants` (contrôle mémoire, `TMPDIR=/var/tmp`, `-j 1`, un fichier). La mutation de tout le workspace tourne en CI.
- Licence Apache-2.0, recommandée par Q2 (à confirmer), avec un `NOTICE` qui crédite la référence Python.
- Typographie des citations : français correct (Q9), soit `décision`, `délibération`, mois accentués et `1er`.
- `~/.claude/rules/rust-conventions.md` recommande `async_trait` et `Arc<dyn …>`. Le §3 du projet (async natif et génériques) prime ; c'est sans objet en M0–M1, qui n'a pas d'async.
- Pas de `typos` : 30 faux positifs français mesurés à l'essai à blanc.

## Review Focus

1. **Identifiants tapés par un humain ou un modèle** : espaces autour, minuscules, suffixe de version `_JJ-MM-AAAA` sur tous les préfixes, `CONSTEXT` compris. La référence oublie `CONSTEXT` (`citations.py:34-44`). Ces entrées doivent être acceptées et normalisées. Tests : tâche 4, `tolerates_whitespace_and_lowercase` et `strips_version_suffix_on_constext_too`.
2. **Entrée énorme ou hostile renvoyée dans un message d'erreur** : la recopie est tronquée à 64 caractères suivis de `…`, et aucun parseur ne panique. Tests : tâche 1, `echo_*` ; tâche 4, `error_echo_is_truncated` et le proptest `parse_never_panics`.
3. **Dates** : le 29 février, les siècles, le `1er` du mois, les mois accentués, un epoch à minuit UTC, un epoch négatif ou hors bornes. Elles doivent donner la bonne date civile ou `None`, sans panique. Tests : tâche 6, `new_validates_calendar`, `from_unix_millis_edges` et le proptest contre l'oracle `days_from_civil`.
4. **Texte juridique contenant `<` ou `&` littéraux** (« a < b », « & » seul, entité inconnue) : le nettoyage HTML doit les conserver tels quels. Tests : tâche 10, `keeps_literal_angle_brackets` et `keeps_unknown_or_broken_entities`.
5. **Pagination sur du texte multi-octets** : les fenêtres successives doivent recomposer exactement le texte, sans caractère perdu ni dupliqué, et toujours avancer. Tests : tâche 14, proptest `windows_partition_the_text` ; cible de fuzz `text` (tâche 16).

---

## Structure des fichiers

```
Cargo.toml                    workspace, lints, profil mutants
rust-toolchain.toml           stable + clippy/rustfmt/llvm-tools-preview
rustfmt.toml, clippy.toml     format 100 colonnes ; msrv, unwrap permis en test
deny.toml                     advisories, licences, bans, sources
Makefile                      pipeline local ; preuve = sortie de `make ci`
.cargo/config.toml            alias `cargo xtask`
.cargo/mutants.toml           config cargo-mutants (nextest, profil, exclusions justifiées)
.github/workflows/ci.yml      portes à chaque push/PR + mutation du diff de PR
.github/workflows/weekly.yml  mutation complète (4 shards) + fuzz
LICENSE, NOTICE, CONTRIBUTING.md, docs/adr/000{1,2}-*.md
xtask/src/{lib,arch,main}.rs  `cargo xtask check-arch` (logique pure dans arch.rs)
crates/lf-domain/src/
  lib.rs                      déclarations de modules
  error.rs                    DomainError, echo()
  id.rs                       Id<K>, alias *Id, AnyId, ArticleId, DecisionId
  fond.rs                     Fond
  date.rs                     CivilDate (parse, epoch ms UTC, prose française)
  ecli.rs                     Ecli
  url.rs                      ResourceKind, legifrance_url
  citation.rs                 CitationContract, Citable, règles de citation
  coverage.rs                 Coverage (invariant : lacunes non vides)
  entity/{mod,act,article,decision,deliberation,agreement,search}.rs
  text/{mod,clean,window,passages,fold,rank}.rs
fuzz/                         cargo-fuzz (hors workspace) : cibles ids, text
```

---

## Task 0 : branche de travail

- [ ] **Step 1 : créer la branche**

Si l'exécution se fait en worktree isolé, c'est le skill `superpowers:using-git-worktrees` qui le crée. Sinon :

```bash
cd ~/mcp-ligitfrance && git switch main && git pull --ff-only && git switch -c feat/m0-m1-domaine
```

---

## Task 1 : workspace, toolchain et erreurs du domaine

**Files :**
- Create : `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `.gitignore`
- Create : `crates/lf-domain/Cargo.toml`, `crates/lf-domain/src/lib.rs`, `crates/lf-domain/src/error.rs`

**Interfaces :**
- Produces : `lf_domain::DomainError` (variantes `InvalidId { input: String, expected: &'static str }`, `InvalidDate { input }`, `InvalidEcli { input }`, `UnknownFond { input }`, `EmptyCoverageGaps`), `lf_domain::error::ECHO_MAX_CHARS: usize = 64`, `lf_domain::error::echo(&str) -> String`.

- [ ] **Step 1 : fichiers du workspace**

`Cargo.toml` (le membre `xtask` arrive à la tâche 2) :

```toml
[workspace]
resolver = "3"
members = ["crates/lf-domain"]
exclude = ["fuzz"]

[workspace.package]
version = "0.0.0"
edition = "2024"
rust-version = "1.88"
license = "Apache-2.0"
repository = "https://github.com/muchiny/mcp-ligitfrance"
publish = false

[workspace.dependencies]
thiserror = "2.0.21"
serde_json = "1.0"
proptest = "1.12.0"
pretty_assertions = "1.4.1"

[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
unreachable_pub = "warn"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
must_use_candidate = "allow"
unwrap_used = "warn"
expect_used = "warn"
dbg_macro = "warn"
todo = "warn"

# Profil dédié à cargo-mutants (`profile = "mutants"` dans .cargo/mutants.toml) :
# pas d'infos de debug, build plus rapide par mutant.
[profile.mutants]
inherits = "test"
debug = false
```

`rust-toolchain.toml` :

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt", "llvm-tools-preview"]
```

`rustfmt.toml` :

```toml
max_width = 100
use_small_heuristics = "Max"
```

`clippy.toml` :

```toml
msrv = "1.88.0"
allow-unwrap-in-tests = true
allow-expect-in-tests = true
```

`.gitignore` :

```
/target
/fuzz/target
/fuzz/corpus
/fuzz/artifacts
/fuzz/coverage
mutants.out*/
```

`crates/lf-domain/Cargo.toml` :

```toml
[package]
name = "lf-domain"
description = "Cœur pur du MCP Légifrance : identifiants, dates, citations, entités, utilitaires texte."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[dependencies]
thiserror.workspace = true

[dev-dependencies]
proptest.workspace = true
pretty_assertions.workspace = true

[lints]
workspace = true
```

`crates/lf-domain/src/lib.rs` (les tâches suivantes y ajoutent leur `pub mod`, dans l'ordre alphabétique) :

```rust
//! Cœur pur du serveur MCP Légifrance.
//!
//! Aucune I/O, aucun async : identifiants typés, fonds, dates civiles, ECLI,
//! URL Légifrance, contrat de citation, entités et utilitaires texte.

pub mod error;

pub use error::DomainError;
```

- [ ] **Step 2 : écrire les tests qui échouent**

Créer `crates/lf-domain/src/error.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn echo_keeps_short_input() {
        assert_eq!(echo("LEGITEXT"), "LEGITEXT");
    }

    #[test]
    fn echo_keeps_input_of_exactly_max_chars() {
        let input = "é".repeat(ECHO_MAX_CHARS);
        assert_eq!(echo(&input), input);
    }

    #[test]
    fn echo_truncates_long_input_with_ellipsis() {
        let input = "x".repeat(ECHO_MAX_CHARS + 1);
        assert_eq!(echo(&input), format!("{}…", "x".repeat(ECHO_MAX_CHARS)));
    }

    #[test]
    fn messages_are_actionable() {
        let err = DomainError::InvalidId { input: "ABC".into(), expected: "LEGITEXT" };
        assert_eq!(
            err.to_string(),
            "invalid Legifrance identifier \"ABC\": expected LEGITEXT followed by 12 digits"
        );
        assert_eq!(
            DomainError::InvalidDate { input: "x".into() }.to_string(),
            "invalid date \"x\": expected YYYY-MM-DD"
        );
        assert_eq!(
            DomainError::InvalidEcli { input: "x".into() }.to_string(),
            "invalid ECLI \"x\": expected ECLI:FR:<court>:<year>:<ordinal>"
        );
        assert_eq!(
            DomainError::UnknownFond { input: "x".into() }.to_string(),
            "unknown fond \"x\": expected one of LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO"
        );
        assert_eq!(
            DomainError::EmptyCoverageGaps.to_string(),
            "coverage must declare at least one known gap"
        );
    }
}
```

- [ ] **Step 3 : les voir échouer**

Run : `cargo nextest run -p lf-domain error::`
Expected : échec de compilation, symboles introuvables (`DomainError`, `echo`).

- [ ] **Step 4 : implémenter**

Insérer au-dessus du module de tests :

```rust
//! Erreurs du domaine.

use thiserror::Error;

/// Longueur maximale, en caractères, d'une entrée recopiée dans un message d'erreur.
pub const ECHO_MAX_CHARS: usize = 64;

/// Erreur de validation d'une valeur du domaine.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// Identifiant Légifrance absent ou malformé.
    #[error("invalid Legifrance identifier {input:?}: expected {expected} followed by 12 digits")]
    InvalidId {
        /// Entrée reçue, tronquée à [`ECHO_MAX_CHARS`] caractères.
        input: String,
        /// Préfixe(s) accepté(s), ex. `LEGITEXT`.
        expected: &'static str,
    },
    /// Date absente ou malformée.
    #[error("invalid date {input:?}: expected YYYY-MM-DD")]
    InvalidDate {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// ECLI malformé.
    #[error("invalid ECLI {input:?}: expected ECLI:FR:<court>:<year>:<ordinal>")]
    InvalidEcli {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// Fond inconnu.
    #[error(
        "unknown fond {input:?}: expected one of LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO"
    )]
    UnknownFond {
        /// Entrée reçue, tronquée.
        input: String,
    },
    /// Déclaration de couverture sans lacune connue.
    #[error("coverage must declare at least one known gap")]
    EmptyCoverageGaps,
}

/// Recopie `input` pour un message d'erreur, tronquée à [`ECHO_MAX_CHARS`] caractères.
pub fn echo(input: &str) -> String {
    let mut chars = input.chars();
    let head: String = chars.by_ref().take(ECHO_MAX_CHARS).collect();
    if chars.next().is_some() { head + "…" } else { head }
}
```

- [ ] **Step 5 : tests verts, lint, format**

Run : `cargo nextest run -p lf-domain error:: && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected : `4 tests run: 4 passed`, aucun avertissement, aucun écart de format.

- [ ] **Step 6 : couverture**

Run : `cargo llvm-cov --workspace --lib --summary-only --fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100`
Expected : ligne `TOTAL` à 100.00 % sur les trois colonnes, code de sortie 0.

- [ ] **Step 7 : mutation (règle WSL)**

```bash
free -m | awk 'NR==2 { if ($7 >= 8*1024) print "OK: -j 1 safe"; else print "BLOCK: not enough RAM, wait" }'
TMPDIR=/var/tmp NEXTEST_TEST_THREADS=2 cargo mutants -j 1 --file crates/lf-domain/src/error.rs
```

Expected : `OK: -j 1 safe`, puis `4 mutants tested in …: 2 caught, 2 unviable`, aucun `missed`. Un mutant `unviable` ne compile pas : ce n'est pas un trou de test.

- [ ] **Step 8 : commit**

```bash
git add Cargo.toml rust-toolchain.toml rustfmt.toml clippy.toml .gitignore crates/lf-domain
git commit -m "chore(m0): workspace Cargo, toolchain et erreurs du domaine"
```

---

## Task 2 : pipeline local, `xtask check-arch`, configs deny/mutants

**Files :**
- Modify : `Cargo.toml` (membre `xtask`)
- Create : `xtask/Cargo.toml`, `xtask/src/lib.rs`, `xtask/src/arch.rs`, `xtask/src/main.rs`
- Create : `.cargo/config.toml`, `.cargo/mutants.toml`, `deny.toml`, `Makefile`

**Interfaces :**
- Produces : `xtask::arch::{Package { name: String, deps: Vec<String> }, violations(&[Package]) -> Vec<String>, workspace_packages(&serde_json::Value) -> Vec<Package>}` ; commande `cargo xtask check-arch` ; cibles `make fmt fmt-check lint test doc arch deny machete cov mem-check mutants mutants-diff ci`.

- [ ] **Step 1 : déclarer le membre**

Dans `Cargo.toml`, remplacer la ligne `members = ["crates/lf-domain"]` par :

```toml
members = ["crates/lf-domain", "xtask"]
```

- [ ] **Step 2 : écrire les tests qui échouent**

`xtask/Cargo.toml` :

```toml
[package]
name = "xtask"
description = "Tâches de développement de mcp-legifrance (cargo xtask …)."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
serde_json.workspace = true

[dev-dependencies]
pretty_assertions.workspace = true

[lints]
workspace = true
```

`xtask/src/lib.rs` :

```rust
//! Tâches de développement, testables hors du binaire.

pub mod arch;
```

`xtask/src/arch.rs`, module de tests seul :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn pkg(name: &str, deps: &[&str]) -> Package {
        Package { name: name.into(), deps: deps.iter().map(|d| (*d).to_owned()).collect() }
    }

    #[test]
    fn clean_graph_has_no_violation() {
        let graph = [
            pkg("lf-domain", &["thiserror"]),
            pkg("lf-application", &["lf-domain", "tokio"]),
            pkg("lf-adapter-piste", &["lf-application", "lf-domain", "reqwest"]),
            pkg("lf-app", &["lf-adapter-piste", "lf-adapter-mcp", "lf-application"]),
            pkg("xtask", &["serde_json"]),
        ];
        assert_eq!(violations(&graph), Vec::<String>::new());
    }

    #[test]
    fn domain_stays_pure() {
        let graph = [pkg(
            "lf-domain",
            &[
                "lf-application",
                "tokio",
                "reqwest",
                "serde_json",
                "rmcp",
                "hyper",
                "moka",
                "thiserror",
            ],
        )];
        assert_eq!(
            violations(&graph),
            [
                "lf-domain must not depend on lf-application",
                "lf-domain must not depend on tokio",
                "lf-domain must not depend on reqwest",
                "lf-domain must not depend on serde_json",
                "lf-domain must not depend on rmcp",
                "lf-domain must not depend on hyper",
                "lf-domain must not depend on moka",
            ]
        );
    }

    #[test]
    fn application_and_adapters_respect_boundaries() {
        let graph = [
            pkg("lf-application", &["lf-adapter-piste", "lf-app"]),
            pkg("lf-adapter-cache", &["lf-adapter-piste", "lf-app", "lf-application"]),
        ];
        assert_eq!(
            violations(&graph),
            [
                "lf-application must not depend on lf-adapter-piste",
                "lf-application must not depend on lf-app",
                "lf-adapter-cache must not depend on lf-adapter-piste",
                "lf-adapter-cache must not depend on lf-app",
            ]
        );
    }

    #[test]
    fn reads_cargo_metadata_members_and_normal_deps() {
        let metadata = json!({
            "workspace_members": ["path+file:///w/crates/lf-domain#0.0.0"],
            "packages": [
                {
                    "id": "path+file:///w/crates/lf-domain#0.0.0",
                    "name": "lf-domain",
                    "dependencies": [
                        { "name": "thiserror", "kind": null },
                        { "name": "proptest", "kind": "dev" },
                        { "name": "cc", "kind": "build" }
                    ]
                },
                { "id": "registry+https://x#thiserror@2.0.21", "name": "thiserror", "dependencies": [] }
            ]
        });
        assert_eq!(workspace_packages(&metadata), [pkg("lf-domain", &["thiserror"])]);
    }

    #[test]
    fn tolerates_missing_fields() {
        assert_eq!(workspace_packages(&json!({})), Vec::<Package>::new());
        let metadata = json!({ "workspace_members": ["a"], "packages": [{ "id": "a" }] });
        assert_eq!(workspace_packages(&metadata), [pkg("", &[])]);
    }
}
```

- [ ] **Step 3 : les voir échouer**

Run : `cargo nextest run -p xtask`
Expected : échec de compilation, symboles introuvables (`Package`, `violations`, `workspace_packages`).

- [ ] **Step 4 : implémenter la règle**

Insérer au-dessus du module de tests de `xtask/src/arch.rs` :

```rust
//! Règle de dépendance de l'architecture hexagonale (00-decisions.md §2).
//!
//! `lf-domain` ne dépend d'aucun crate `lf-*` ni d'I/O ; `lf-application` ne
//! dépend d'aucun adapter ; un adapter ne dépend d'aucun autre adapter ; seul
//! `lf-app` connaît tout. Seules les dépendances normales comptent (pas `dev`
//! ni `build`).

use serde_json::Value;

/// Crate du workspace et ses dépendances normales directes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Nom du crate.
    pub name: String,
    /// Noms des dépendances normales.
    pub deps: Vec<String>,
}

/// Crates d'I/O ou de protocole interdits dans `lf-domain`.
const DOMAIN_FORBIDDEN: [&str; 6] = ["tokio", "reqwest", "serde_json", "rmcp", "hyper", "moka"];

fn is_adapter(name: &str) -> bool {
    name.starts_with("lf-adapter-")
}

fn forbidden(package: &str, dep: &str) -> bool {
    match package {
        "lf-domain" => dep.starts_with("lf-") || DOMAIN_FORBIDDEN.contains(&dep),
        "lf-application" => is_adapter(dep) || dep == "lf-app",
        p if is_adapter(p) => is_adapter(dep) || dep == "lf-app",
        _ => false,
    }
}

/// Liste des violations, une ligne lisible par violation.
pub fn violations(packages: &[Package]) -> Vec<String> {
    packages
        .iter()
        .flat_map(|p| {
            p.deps
                .iter()
                .filter(|dep| forbidden(&p.name, dep))
                .map(move |dep| format!("{} must not depend on {dep}", p.name))
        })
        .collect()
}

/// Extrait les membres du workspace de la sortie de `cargo metadata --format-version 1`.
pub fn workspace_packages(metadata: &Value) -> Vec<Package> {
    let members: Vec<&str> = metadata["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(&id)))
        .map(|p| Package {
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            deps: p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["kind"].is_null())
                .filter_map(|d| d["name"].as_str().map(str::to_owned))
                .collect(),
        })
        .collect()
}
```

Run : `cargo nextest run -p xtask`
Expected : `5 tests run: 5 passed`.

- [ ] **Step 5 : binaire et alias**

`xtask/src/main.rs` (glue d'I/O mince, exclue de la mutation) :

```rust
//! `cargo xtask <tâche>` — voir `.cargo/config.toml`.

use std::{env, process::Command, process::ExitCode};

use xtask::arch::{violations, workspace_packages};

fn check_arch() -> Result<(), String> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata JSON: {e}"))?;
    let found = violations(&workspace_packages(&metadata));
    if found.is_empty() {
        println!("check-arch: ok");
        Ok(())
    } else {
        Err(found.join("\n"))
    }
}

fn main() -> ExitCode {
    let result = match env::args().nth(1).as_deref() {
        Some("check-arch") => check_arch(),
        _ => Err("usage: cargo xtask check-arch".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
```

`.cargo/config.toml` :

```toml
[alias]
xtask = "run --quiet --package xtask --"
```

Run : `cargo xtask check-arch`
Expected : `check-arch: ok`.

- [ ] **Step 6 : configs de qualité**

`.cargo/mutants.toml` :

```toml
# Configuration cargo-mutants (https://mutants.rs/config-file.html).
# Toute exclusion (`exclude_re`) doit être justifiée en commentaire : mutant
# prouvé équivalent, avec le raisonnement. Le code `#[cfg(test)]` n'est jamais muté.
test_tool = "nextest"
profile = "mutants"
exclude_globs = [
    "xtask/src/main.rs", # glue d'I/O (cargo metadata, stdout) ; la logique est dans arch.rs
    "fuzz/**",
]
exclude_re = []
```

`deny.toml` :

```toml
# cargo-deny : advisories, licences, doublons, sources.
[graph]
all-features = true

[advisories]
version = 2
ignore = []

[licenses]
version = 2
allow = ["MIT", "Apache-2.0", "Unicode-3.0"]

[licenses.private]
ignore = true

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

`Makefile` (tabulations en début de recette) :

```make
# Pipeline local de mcp-legifrance. Preuve de complétion : sortie de `make ci`.
#
# WSL (~/.claude/rules/wsl-safety.md) : parallélisme cargo par défaut ; cargo-mutants
# toujours `-j 1`, un fichier à la fois, scratch dans /var/tmp (/tmp est un tmpfs en RAM).
# La mutation de tout le workspace d'un coup tourne en CI (weekly.yml), jamais ici.

COV_FLAGS := --workspace --lib --summary-only \
	--fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100
MUTANTS_SAFE := TMPDIR=/var/tmp NEXTEST_TEST_THREADS=2 cargo mutants -j 1
BASE ?= origin/main

.PHONY: fmt fmt-check lint test doc arch deny machete cov mem-check mutants mutants-diff ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo nextest run --workspace
	cargo test --workspace --doc

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

arch:
	cargo xtask check-arch

deny:
	cargo deny check

machete:
	cargo machete

# Couverture des tests unitaires (--lib) : 100 % lignes, régions et fonctions.
cov:
	cargo llvm-cov $(COV_FLAGS)

# Contrôle mémoire obligatoire avant cargo-mutants : au moins 8 Gio disponibles.
mem-check:
	@free -m | awk 'NR==2 { if ($$7 >= 8*1024) print "OK: -j 1 safe"; else { print "BLOCK: not enough RAM, wait"; exit 1 } }'

# Un fichier : make mutants FILE=crates/lf-domain/src/id.rs
mutants: mem-check
	@test -n "$(FILE)" || { echo "usage: make mutants FILE=<path>"; exit 2; }
	$(MUTANTS_SAFE) --file $(FILE)

# Chaque fichier .rs modifié depuis $(BASE), l'un après l'autre.
mutants-diff: mem-check
	@set -e; for f in $$(git diff --name-only $(BASE)...HEAD -- '*.rs'); do \
		if [ -f "$$f" ]; then $(MUTANTS_SAFE) --file "$$f"; fi; \
	done

ci: fmt-check lint test doc arch deny machete cov
```

- [ ] **Step 7 : la preuve de complétion**

Run : `make ci`
Expected : chaque cible passe. `check-arch: ok`, `advisories ok, bans ok, licenses ok, sources ok`, machete `didn't find any unused dependencies`, et `TOTAL` de `make cov` à 100.00 % sur les trois colonnes.

- [ ] **Step 8 : mutation**

Run : `make mutants FILE=xtask/src/arch.rs`
Expected : `18 mutants tested in …: 17 caught, 1 unviable`, aucun `missed`.

- [ ] **Step 9 : commit**

```bash
git add Cargo.toml Cargo.lock xtask .cargo deny.toml Makefile
git commit -m "chore(m0): pipeline local, check-arch, configs deny et mutants"
```

---

## Task 3 : CI, licence et documents de contribution

**Files :**
- Create : `.github/workflows/ci.yml`, `.github/workflows/weekly.yml`
- Create : `LICENSE`, `NOTICE`, `CONTRIBUTING.md`, `docs/adr/0001-architecture-hexagonale.md`, `docs/adr/0002-portes-qualite.md`

**Interfaces :**
- Consumes : cibles `make` de la tâche 2.
- Produces : jobs CI `check`, `test`, `coverage`, `deny`, `msrv`, `mutants` (PR) ; job hebdomadaire `mutants` (4 shards). Le job `fuzz` s'ajoute à la tâche 16.

- [ ] **Step 1 : CI à chaque push et PR**

`.github/workflows/ci.yml` (versions d'actions alignées sur `bridge-mcp`) :

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: fmt, clippy, doc, arch, machete
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-machete
      - run: make fmt-check lint doc arch machete

  test:
    name: tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-nextest
      - run: make test

  coverage:
    name: couverture 100 %
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-llvm-cov
      - run: make cov

  deny:
    name: cargo-deny
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: EmbarkStudios/cargo-deny-action@v2

  msrv:
    name: MSRV 1.88
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: "1.88"
      - uses: Swatinem/rust-cache@v2
      - run: cargo +1.88 check --workspace --all-targets

  mutants:
    name: mutants (diff de la PR)
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          key: mutants-diff
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-nextest,cargo-mutants
      # BASE_REF passe par l'environnement, jamais interpolé dans le script.
      # Sortie non nulle dès qu'un mutant survit (code 2) ou boucle (TIMEOUT,
      # code 3) : la PR est bloquée. `--baseline skip` (le job test prouve déjà
      # l'arbre sain) impose un `--timeout` explicite, sinon 300 s par mutant.
      - name: Muter le diff
        env:
          BASE_REF: ${{ github.base_ref }}
        run: |
          git diff "origin/${BASE_REF}...HEAD" > pr.diff
          cargo mutants --in-diff pr.diff -j 2 --baseline skip --timeout 60
      - uses: actions/upload-artifact@v7
        if: always()
        with:
          name: mutants-diff
          path: mutants.out
```

- [ ] **Step 2 : balayage hebdomadaire**

`.github/workflows/weekly.yml` :

```yaml
name: Weekly

on:
  schedule:
    - cron: "17 3 * * 1"
  workflow_dispatch:

permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always

jobs:
  mutants:
    name: mutants (shard ${{ matrix.shard }}/4)
    runs-on: ubuntu-latest
    timeout-minutes: 120
    strategy:
      fail-fast: false
      matrix:
        shard: [0, 1, 2, 3]
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          key: mutants-full
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-nextest,cargo-mutants
      - run: cargo mutants --shard ${{ matrix.shard }}/4 -j 2 --baseline skip --timeout 60
      - uses: actions/upload-artifact@v7
        if: always()
        with:
          name: mutants-shard-${{ matrix.shard }}
          path: mutants.out

```

- [ ] **Step 3 : valider les workflows**

Run : `uvx --from actionlint-py actionlint .github/workflows/*.yml`
Expected : aucune sortie, code 0. Vérifié à l'essai à blanc (actionlint 1.7.12).

- [ ] **Step 4 : licence et NOTICE**

```bash
curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE
head -3 LICENSE   # attendu : "Apache License" / "Version 2.0, January 2004"
```

`NOTICE` :

```
mcp-legifrance
Copyright 2026 Loïc Wernert

Ce produit porte en Rust des règles de normalisation et de citation issues de
fr-eli-mcp (https://github.com/matematicsolutions/fr-eli-mcp, commit d3b5b13),
Copyright Matematic Solutions et Wieslaw Mazur, distribué sous licence
Apache-2.0. Les emplacements d'origine sont cités en commentaire dans le code
(`citations.py:<ligne>`, `server.py:<ligne>`, `coverage.py:<ligne>`).
```

- [ ] **Step 5 : CONTRIBUTING et ADR**

`CONTRIBUTING.md` :

```markdown
# Contribuer à mcp-legifrance

Le plan et les décisions communes sont dans `docs/plan/00-decisions.md` ; les
exigences de qualité (§9) y font foi. Ce fichier dit comment les tenir au quotidien.

## Boucle de travail

1. Écrire le test qui échoue (`#[cfg(test)] mod tests` dans le fichier du code testé).
2. Le lancer et le voir échouer : `cargo nextest run -p <crate> <nom>`.
3. Écrire le code minimal qui le fait passer, relancer.
4. Couverture : `make cov` (100 % lignes, régions et fonctions, sinon échec).
5. Mutation du fichier touché : `make mutants FILE=<chemin>` → `0 missed`.
6. `make ci` vert, puis commit.

## Portes de qualité

| Porte | Commande | Seuil |
|---|---|---|
| Format | `make fmt-check` | aucun écart |
| Lint | `make lint` | zéro avertissement (`clippy::pedantic` inclus) |
| Tests | `make test` | tous verts |
| Doc | `make doc` | zéro avertissement |
| Architecture | `make arch` | aucune dépendance interdite (§2) |
| Dépendances | `make deny`, `make machete` | aucun avis, aucune dépendance inutile |
| Couverture | `make cov` | 100 % lignes, régions, fonctions (`--lib`) |
| Mutation | `make mutants FILE=…` / CI | 0 mutant `MISSED` |

La CI rejoue tout sur chaque PR et mute le diff de la PR ; le lundi, `weekly.yml`
mute tout le workspace (4 shards) et lance les cibles de fuzz.

## Écrire des tests qui tiennent 100 % des régions

Le code de test compte dans la couverture. Trois formes laissent une région
jamais exécutée :

- `assert!(cond, "{}", expr)` : l'expression du message n'est évaluée qu'en cas
  d'échec. Écrire `assert_eq!` ou un message à arguments capturés (`"{bad:?}"`).
- `assert!(matches!(x, Variant(_)))` : le bras `_ => false` n'est jamais pris.
  Comparer la valeur entière avec `assert_eq!`.
- `let … else { panic!() }` : comparer la valeur entière avec `assert_eq!`.

## Mutants survivants

Un mutant `MISSED` signale un test manquant : on écrit le test. Exception
unique : un mutant prouvé équivalent (même comportement observable). Il va
alors dans `exclude_re` de `.cargo/mutants.toml` avec, en commentaire, la preuve.

Un mutant `TIMEOUT` (code de sortie 3, bloquant en CI) signale une boucle qui
peut ne pas terminer. Toute boucle doit garantir sa progression, même mutée :
consommer au moins un élément par tour (`text/clean.rs`) ou borner le nombre de
tours (`text/passages.rs`).

## Sur WSL

`/tmp` est un tmpfs en RAM. Ne lancer `cargo mutants` que par `make mutants`
(`TMPDIR=/var/tmp`, `-j 1`, un fichier, contrôle mémoire préalable). Jamais
`cargo test -j 4` ni plus.
```

`docs/adr/0001-architecture-hexagonale.md` :

```markdown
# ADR 0001 — Architecture hexagonale en workspace de crates

Statut : accepté (2026-10-15).

## Contexte

La référence Python (`matematicsolutions/fr-eli-mcp`, d3b5b13) mêle dans
`server.py` validation, appels HTTP, normalisation et protocole MCP. L'API de
`rmcp` a changé de version majeure quatre fois en 2026 ; un second adapter
sortant (dumps DILA hors ligne) est prévu en v1.0.

## Décision

Un crate par couche (`docs/plan/00-decisions.md` §2) : `lf-domain` pur,
`lf-application` (ports et cas d'usage), un crate par adapter, `lf-app` comme
seule racine de composition. La règle de dépendance est vérifiée par
`cargo xtask check-arch` en CI.

## Conséquences

- Le protocole MCP reste confiné à `lf-adapter-mcp` : un changement de `rmcp`
  ne touche qu'un crate.
- `lf-domain` se teste sans réseau ni runtime async, ce qui rend tenables les
  portes de couverture et de mutation de l'ADR 0002.
- Plus de manifestes à maintenir qu'un crate unique.
```

`docs/adr/0002-portes-qualite.md` :

```markdown
# ADR 0002 — Tests unitaires, couverture et mutation au maximum

Statut : accepté (2026-10-15). Exigence utilisateur, `docs/plan/00-decisions.md` §9.

## Contexte

Le serveur produit des citations juridiques : une URL fausse, une date mal
convertie ou un « 1 janvier » au lieu de « 1er janvier » passe inaperçu dans
une réponse de modèle. La couverture seule ne prouve pas que les tests
vérifient quelque chose ; la mutation le prouve.

## Décision

- TDD : chaque comportement naît d'un test qui échoue.
- Tests unitaires dans le fichier du code (`#[cfg(test)] mod tests`), propriétés
  `proptest` pour les parseurs et utilitaires texte, cibles `cargo-fuzz` pour
  les entrées non fiables.
- Couverture `cargo llvm-cov --lib` : 100 % des lignes, régions et fonctions,
  bloquant en local (`make cov`) et en CI.
- Mutation `cargo-mutants` : 0 mutant `MISSED`, bloquant sur le diff de chaque
  PR ; balayage complet hebdomadaire. Une exclusion n'est admise que pour un
  mutant prouvé équivalent, preuve en commentaire dans `.cargo/mutants.toml`.

## Conséquences

- Mesuré sur M0–M1 en essai à blanc (2026-10-15) : 100 % des lignes, régions
  et fonctions ; 337 mutants, 0 survivant (303 tués, 34 non compilables), sans
  aucune exclusion. Deux survivants équivalents (`text/rank.rs`) ont disparu en
  simplifiant une garde redondante, pas en les excluant.
- Les adapters à I/O (M2+) isolent leur logique dans des fonctions pures
  (mapping DTO, politique de retry) pour rester sous ces portes ; la glue d'I/O
  irréductible est déclarée dans `exclude_globs` avec sa justification.
- Coût : chaque PR paie la mutation de son diff en CI.
```

- [ ] **Step 6 : commit**

```bash
git add .github LICENSE NOTICE CONTRIBUTING.md docs/adr
git commit -m "chore(m0): CI, mutation hebdomadaire, licence et ADR"
```

---

## Task 4 : identifiants Légifrance typés

**Files :**
- Create : `crates/lf-domain/src/id.rs`
- Modify : `crates/lf-domain/src/lib.rs`

**Interfaces :**
- Consumes : `DomainError::InvalidId`, `echo` (tâche 1).
- Produces : `IdKind { const PREFIX: &'static str }` ; `Id<K>` : `parse(&str) -> Result<Self, DomainError>`, `as_str(&self) -> &str`, `Clone`, `PartialEq`, `Eq`, `Debug`, `Display`, `FromStr`. Alias `LegiTextId`, `LegiArtiId`, `JorfTextId`, `KaliTextId`, `KaliArtiId`, `JuriTextId`, `ConsTextId`, `CetaTextId`, `CnilTextId`, `AccoTextId`. `AnyId::{parse, as_str}`. `ArticleId::{Legi(LegiArtiId), Kali(KaliArtiId)}` et `DecisionId::{Juri, Constit, Cetat}`, avec `parse`, `as_str` et `FromStr`. Constantes `PREFIX_LEN = 8`, `ID_DIGITS = 12`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod id;` (ordre alphabétique).

Créer `crates/lf-domain/src/id.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn parses_fixture_ids() {
        // Un identifiant par préfixe, tirés de tests/fixtures/*.json de la référence.
        assert_eq!(
            LegiTextId::parse("LEGITEXT000006070721").unwrap().as_str(),
            "LEGITEXT000006070721"
        );
        assert_eq!(
            LegiArtiId::parse("LEGIARTI000006419288").unwrap().as_str(),
            "LEGIARTI000006419288"
        );
        assert_eq!(
            JorfTextId::parse("JORFTEXT000020445850").unwrap().as_str(),
            "JORFTEXT000020445850"
        );
        assert_eq!(
            KaliTextId::parse("KALITEXT000005661030").unwrap().as_str(),
            "KALITEXT000005661030"
        );
        assert_eq!(
            KaliArtiId::parse("KALIARTI000005805295").unwrap().as_str(),
            "KALIARTI000005805295"
        );
        assert_eq!(
            JuriTextId::parse("JURITEXT000048465611").unwrap().as_str(),
            "JURITEXT000048465611"
        );
        assert_eq!(
            ConsTextId::parse("CONSTEXT000037434572").unwrap().as_str(),
            "CONSTEXT000037434572"
        );
        assert_eq!(
            CetaTextId::parse("CETATEXT000051655317").unwrap().as_str(),
            "CETATEXT000051655317"
        );
        assert_eq!(
            CnilTextId::parse("CNILTEXT000053352643").unwrap().as_str(),
            "CNILTEXT000053352643"
        );
        assert_eq!(
            AccoTextId::parse("ACCOTEXT000051888026").unwrap().as_str(),
            "ACCOTEXT000051888026"
        );
    }

    #[test]
    fn strips_version_suffix() {
        let id = LegiTextId::parse("LEGITEXT000033205014_08-09-2023").unwrap();
        assert_eq!(id.as_str(), "LEGITEXT000033205014");
    }

    #[test]
    fn strips_version_suffix_on_constext_too() {
        let id = ConsTextId::parse("CONSTEXT000037434572_01-01-2020").unwrap();
        assert_eq!(id.as_str(), "CONSTEXT000037434572");
    }

    #[test]
    fn tolerates_whitespace_and_lowercase() {
        let id = LegiTextId::parse("  legitext000006070721\n").unwrap();
        assert_eq!(id.as_str(), "LEGITEXT000006070721");
    }

    #[test]
    fn rejects_wrong_prefix_with_actionable_error() {
        let err = LegiTextId::parse("JORFTEXT000020445850").unwrap_err();
        assert_eq!(
            err,
            DomainError::InvalidId { input: "JORFTEXT000020445850".into(), expected: "LEGITEXT" }
        );
    }

    #[test]
    fn rejects_malformed_digits() {
        for bad in [
            "",
            "   ",
            "LEGITEXT",
            "LEGITEXT00000607072",   // 11 chiffres
            "LEGITEXT0000060707210", // 13 chiffres
            "LEGITEXT00000607072X",
            "LEGITEX1000006070721",
            "LÉGITEXT000006070721", // préfixe non ASCII
            "_LEGITEXT000006070721",
        ] {
            assert!(LegiTextId::parse(bad).is_err(), "{bad:?} doit être refusé");
        }
    }

    #[test]
    fn error_echo_is_truncated() {
        let long = "A".repeat(10_000);
        assert_eq!(
            LegiTextId::parse(&long).unwrap_err(),
            DomainError::InvalidId {
                input: format!("{}…", "A".repeat(crate::error::ECHO_MAX_CHARS)),
                expected: "LEGITEXT",
            }
        );
    }

    #[test]
    fn id_traits() {
        let a = LegiTextId::parse("LEGITEXT000006070721").unwrap();
        let b: LegiTextId = "LEGITEXT000006070721".parse().unwrap();
        let c = LegiTextId::parse("LEGITEXT000033205014").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.clone(), a);
        assert_eq!(a.to_string(), "LEGITEXT000006070721");
        assert_eq!(format!("{a:?}"), "Id(\"LEGITEXT000006070721\")");
        assert!("JORFTEXT000020445850".parse::<LegiTextId>().is_err());
    }

    #[test]
    fn any_id_accepts_every_prefix_shape() {
        assert_eq!(
            AnyId::parse("JORFTEXT000020445850_01-01-2020").unwrap().as_str(),
            "JORFTEXT000020445850"
        );
        assert_eq!(AnyId::parse("KALISCTA000005705795").unwrap().as_str(), "KALISCTA000005705795");
        assert_eq!(
            AnyId::parse("nope").unwrap_err(),
            DomainError::InvalidId { input: "nope".into(), expected: "8 letters" }
        );
    }

    #[test]
    fn article_id_dispatches_on_prefix() {
        let legi = ArticleId::parse("LEGIARTI000006419288").unwrap();
        let kali: ArticleId = "KALIARTI000005805295".parse().unwrap();
        assert_eq!(legi, ArticleId::Legi(LegiArtiId::parse("LEGIARTI000006419288").unwrap()));
        assert_eq!(kali, ArticleId::Kali(KaliArtiId::parse("KALIARTI000005805295").unwrap()));
        assert_eq!(legi.as_str(), "LEGIARTI000006419288");
        assert_eq!(kali.as_str(), "KALIARTI000005805295");
        assert_eq!(
            ArticleId::parse("LEGITEXT000006070721").unwrap_err(),
            DomainError::InvalidId {
                input: "LEGITEXT000006070721".into(),
                expected: "LEGIARTI or KALIARTI",
            }
        );
    }

    #[test]
    fn decision_id_dispatches_on_prefix() {
        let juri = DecisionId::parse("JURITEXT000048465611").unwrap();
        let constit = DecisionId::parse("CONSTEXT000037434572").unwrap();
        let cetat: DecisionId = "CETATEXT000051655317".parse().unwrap();
        assert_eq!(juri, DecisionId::Juri(JuriTextId::parse("JURITEXT000048465611").unwrap()));
        assert_eq!(
            constit,
            DecisionId::Constit(ConsTextId::parse("CONSTEXT000037434572").unwrap())
        );
        assert_eq!(cetat, DecisionId::Cetat(CetaTextId::parse("CETATEXT000051655317").unwrap()));
        assert_eq!(juri.as_str(), "JURITEXT000048465611");
        assert_eq!(constit.as_str(), "CONSTEXT000037434572");
        assert_eq!(cetat.as_str(), "CETATEXT000051655317");
        assert_eq!(
            DecisionId::parse("CNILTEXT000053352643").unwrap_err(),
            DomainError::InvalidId {
                input: "CNILTEXT000053352643".into(),
                expected: "JURITEXT, CONSTEXT or CETATEXT",
            }
        );
    }

    proptest! {
        #[test]
        fn parse_never_panics(s in "\\PC*") {
            let _ = LegiTextId::parse(&s);
            let _ = AnyId::parse(&s);
            let _ = ArticleId::parse(&s);
            let _ = DecisionId::parse(&s);
        }

        #[test]
        fn valid_shape_roundtrips(digits in "[0-9]{12}", suffix in "(_[0-9]{2}-[0-9]{2}-[0-9]{4})?") {
            let raw = format!("LEGITEXT{digits}");
            let id = LegiTextId::parse(&format!("{raw}{suffix}")).unwrap();
            prop_assert_eq!(id.as_str(), raw.as_str());
            prop_assert_eq!(LegiTextId::parse(id.as_str()).unwrap(), id);
        }
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain id::`
Expected : échec de compilation, symboles introuvables (par exemple `LegiTextId`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/id.rs` :

```rust
//! Identifiants Légifrance typés (« parse, don't validate »).
//!
//! Forme vérifiée sur les 170 identifiants des fixtures de la référence Python
//! (`tests/fixtures/*.json`, d3b5b13) : préfixe de 8 lettres + 12 chiffres.
//! Le suffixe de version `_JJ-MM-AAAA` (ex. `LEGITEXT000033205014_08-09-2023`)
//! est retiré à la normalisation (`citations.py:55-63`). Contrairement à la
//! référence, le retrait vaut pour tous les préfixes (elle oublie `CONSTEXT`,
//! `citations.py:34-44`).

use std::{fmt, marker::PhantomData, str::FromStr};

use crate::error::{DomainError, echo};

/// Nombre de lettres du préfixe.
pub const PREFIX_LEN: usize = 8;
/// Nombre de chiffres après le préfixe.
pub const ID_DIGITS: usize = 12;

/// Normalise une entrée : espaces retirés, suffixe `_…` retiré, majuscules ASCII.
/// Renvoie `None` si le reste n'a pas la forme `[A-Z]{8}[0-9]{12}`.
fn normalize(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let head = trimmed.split_once('_').map_or(trimmed, |(head, _)| head);
    let upper = head.to_ascii_uppercase();
    let (prefix, digits) = upper.split_at_checked(PREFIX_LEN)?;
    let well_formed = prefix.bytes().all(|b| b.is_ascii_uppercase())
        && digits.len() == ID_DIGITS
        && digits.bytes().all(|b| b.is_ascii_digit());
    well_formed.then_some(upper)
}

/// Famille d'identifiants : fixe le préfixe attendu.
pub trait IdKind {
    /// Préfixe de 8 lettres majuscules, ex. `LEGITEXT`.
    const PREFIX: &'static str;
}

/// Identifiant Légifrance validé de la famille `K`.
pub struct Id<K> {
    raw: String,
    kind: PhantomData<fn() -> K>,
}

impl<K: IdKind> Id<K> {
    /// Valide et normalise `input`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si la forme ou le préfixe ne convient pas.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        normalize(input)
            .filter(|raw| raw.starts_with(K::PREFIX))
            .map(|raw| Self { raw, kind: PhantomData })
            .ok_or_else(|| DomainError::InvalidId { input: echo(input), expected: K::PREFIX })
    }
}

impl<K> Id<K> {
    /// Forme normalisée, ex. `LEGITEXT000006070721`.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        Self { raw: self.raw.clone(), kind: PhantomData }
    }
}

impl<K> PartialEq for Id<K> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<K> Eq for Id<K> {}

impl<K> fmt::Debug for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Id").field(&self.raw).finish()
    }
}

impl<K> fmt::Display for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl<K: IdKind> FromStr for Id<K> {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Texte consolidé (loi, décret, code) : `LEGITEXT`.
pub enum LegiText {}
impl IdKind for LegiText {
    const PREFIX: &'static str = "LEGITEXT";
}
/// Article consolidé : `LEGIARTI`.
pub enum LegiArti {}
impl IdKind for LegiArti {
    const PREFIX: &'static str = "LEGIARTI";
}
/// Texte du Journal officiel : `JORFTEXT`.
pub enum JorfText {}
impl IdKind for JorfText {
    const PREFIX: &'static str = "JORFTEXT";
}
/// Convention collective : `KALITEXT`.
pub enum KaliText {}
impl IdKind for KaliText {
    const PREFIX: &'static str = "KALITEXT";
}
/// Article de convention collective : `KALIARTI`.
pub enum KaliArti {}
impl IdKind for KaliArti {
    const PREFIX: &'static str = "KALIARTI";
}
/// Décision judiciaire : `JURITEXT`.
pub enum JuriText {}
impl IdKind for JuriText {
    const PREFIX: &'static str = "JURITEXT";
}
/// Décision du Conseil constitutionnel : `CONSTEXT`.
pub enum ConsText {}
impl IdKind for ConsText {
    const PREFIX: &'static str = "CONSTEXT";
}
/// Décision administrative : `CETATEXT`.
pub enum CetaText {}
impl IdKind for CetaText {
    const PREFIX: &'static str = "CETATEXT";
}
/// Délibération CNIL : `CNILTEXT`.
pub enum CnilText {}
impl IdKind for CnilText {
    const PREFIX: &'static str = "CNILTEXT";
}
/// Accord d'entreprise : `ACCOTEXT`.
pub enum AccoText {}
impl IdKind for AccoText {
    const PREFIX: &'static str = "ACCOTEXT";
}

/// Identifiant `LEGITEXT…`.
pub type LegiTextId = Id<LegiText>;
/// Identifiant `LEGIARTI…`.
pub type LegiArtiId = Id<LegiArti>;
/// Identifiant `JORFTEXT…`.
pub type JorfTextId = Id<JorfText>;
/// Identifiant `KALITEXT…`.
pub type KaliTextId = Id<KaliText>;
/// Identifiant `KALIARTI…`.
pub type KaliArtiId = Id<KaliArti>;
/// Identifiant `JURITEXT…`.
pub type JuriTextId = Id<JuriText>;
/// Identifiant `CONSTEXT…`.
pub type ConsTextId = Id<ConsText>;
/// Identifiant `CETATEXT…`.
pub type CetaTextId = Id<CetaText>;
/// Identifiant `CNILTEXT…`.
pub type CnilTextId = Id<CnilText>;
/// Identifiant `ACCOTEXT…`.
pub type AccoTextId = Id<AccoText>;

/// Identifiant de forme valide, quel que soit son préfixe (résultats de recherche).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnyId(String);

impl AnyId {
    /// Valide et normalise `input`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si la forme ou le préfixe ne convient pas.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        normalize(input)
            .map(Self)
            .ok_or_else(|| DomainError::InvalidId { input: echo(input), expected: "8 letters" })
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Article consultable par `/consult/getArticle` (`server.py:284`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArticleId {
    /// Article consolidé `LEGIARTI…`.
    Legi(LegiArtiId),
    /// Article de convention collective `KALIARTI…`.
    Kali(KaliArtiId),
}

impl ArticleId {
    /// Valide `input` contre les deux préfixes acceptés.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        LegiArtiId::parse(input)
            .map(Self::Legi)
            .or_else(|_| KaliArtiId::parse(input).map(Self::Kali))
            .map_err(|_| DomainError::InvalidId {
                input: echo(input),
                expected: "LEGIARTI or KALIARTI",
            })
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Legi(id) => id.as_str(),
            Self::Kali(id) => id.as_str(),
        }
    }
}

impl FromStr for ArticleId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Décision consultable par `/consult/juri` (`server.py:103`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionId {
    /// Cour de cassation, cours d'appel : `JURITEXT…`.
    Juri(JuriTextId),
    /// Conseil constitutionnel : `CONSTEXT…`.
    Constit(ConsTextId),
    /// Conseil d'État, CAA, TA : `CETATEXT…`.
    Cetat(CetaTextId),
}

impl DecisionId {
    /// Valide `input` contre les trois préfixes acceptés.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidId`] si aucun préfixe ne convient.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        JuriTextId::parse(input)
            .map(Self::Juri)
            .or_else(|_| ConsTextId::parse(input).map(Self::Constit))
            .or_else(|_| CetaTextId::parse(input).map(Self::Cetat))
            .map_err(|_| DomainError::InvalidId {
                input: echo(input),
                expected: "JURITEXT, CONSTEXT or CETATEXT",
            })
    }

    /// Forme normalisée.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Juri(id) => id.as_str(),
            Self::Constit(id) => id.as_str(),
            Self::Cetat(id) => id.as_str(),
        }
    }
}

impl FromStr for DecisionId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain id:: && make lint fmt-check`
Expected : `13 tests run: 13 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/id.rs`
Expected : `27 mutants tested in …: 19 caught, 8 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/id.rs
git commit -m "feat(domain): identifiants Légifrance typés"
```

---

## Task 5 : fonds

**Files :**
- Create : `crates/lf-domain/src/fond.rs`
- Modify : `crates/lf-domain/src/lib.rs`

**Interfaces :**
- Consumes : `DomainError::UnknownFond`, `echo`.
- Produces : `Fond::{Loda, Code, Juri, Constit, Cetat, Cnil, Kali, Acco}` ; `Fond::ALL: [Fond; 8]` ; `name(self) -> &'static str` ; `api_name(self) -> &'static str` ; `parse(&str) -> Result<Fond, DomainError>` ; `Display`, `FromStr`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod fond;` (ordre alphabétique).

Créer `crates/lf-domain/src/fond.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn names_match_reference() {
        let names: Vec<_> = Fond::ALL.iter().map(|f| f.name()).collect();
        assert_eq!(names, ["LODA", "CODE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"]);
    }

    #[test]
    fn api_names_match_reference() {
        let api: Vec<_> = Fond::ALL.iter().map(|f| f.api_name()).collect();
        assert_eq!(
            api,
            ["LODA_DATE", "CODE_DATE", "JURI", "CONSTIT", "CETAT", "CNIL", "KALI", "ACCO"]
        );
    }

    #[test]
    fn parse_is_case_and_space_insensitive() {
        for fond in Fond::ALL {
            assert_eq!(Fond::parse(fond.name()).unwrap(), fond);
            assert_eq!(format!(" {} ", fond.name().to_lowercase()).parse::<Fond>().unwrap(), fond);
        }
    }

    #[test]
    fn parse_rejects_api_names_and_unknowns() {
        assert_eq!(
            Fond::parse("LODA_DATE").unwrap_err(),
            DomainError::UnknownFond { input: "LODA_DATE".into() }
        );
        assert!(Fond::parse("").is_err());
    }

    #[test]
    fn display_is_user_name() {
        assert_eq!(Fond::Constit.to_string(), "CONSTIT");
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain fond::`
Expected : échec de compilation, symboles introuvables (par exemple `Fond`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/fond.rs` :

```rust
//! Fonds Légifrance interrogeables (`server.py:91-100`).

use std::{fmt, str::FromStr};

use crate::error::{DomainError, echo};

/// Fond exposé à l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fond {
    /// Lois et décrets.
    Loda,
    /// Codes.
    Code,
    /// Cour de cassation, cours d'appel.
    Juri,
    /// Conseil constitutionnel.
    Constit,
    /// Conseil d'État, CAA, TA.
    Cetat,
    /// Délibérations CNIL.
    Cnil,
    /// Conventions collectives.
    Kali,
    /// Accords d'entreprise.
    Acco,
}

impl Fond {
    /// Tous les fonds, dans l'ordre de la référence.
    pub const ALL: [Self; 8] = [
        Self::Loda,
        Self::Code,
        Self::Juri,
        Self::Constit,
        Self::Cetat,
        Self::Cnil,
        Self::Kali,
        Self::Acco,
    ];

    /// Nom côté utilisateur, ex. `LODA`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Loda => "LODA",
            Self::Code => "CODE",
            Self::Juri => "JURI",
            Self::Constit => "CONSTIT",
            Self::Cetat => "CETAT",
            Self::Cnil => "CNIL",
            Self::Kali => "KALI",
            Self::Acco => "ACCO",
        }
    }

    /// Nom côté API PISTE, ex. `LODA_DATE`.
    pub fn api_name(self) -> &'static str {
        match self {
            Self::Loda => "LODA_DATE",
            Self::Code => "CODE_DATE",
            other => other.name(),
        }
    }

    /// Lit un nom de fond, sans tenir compte de la casse ni des espaces.
    ///
    /// # Errors
    ///
    /// [`DomainError::UnknownFond`] si le nom ne correspond à aucun fond.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let key = input.trim().to_ascii_uppercase();
        Self::ALL
            .into_iter()
            .find(|fond| fond.name() == key)
            .ok_or_else(|| DomainError::UnknownFond { input: echo(input) })
    }
}

impl fmt::Display for Fond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Fond {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain fond:: && make lint fmt-check`
Expected : `5 tests run: 5 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/fond.rs`
Expected : `8 mutants tested in …: 6 caught, 2 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/fond.rs
git commit -m "feat(domain): fonds Légifrance"
```

---

## Task 6 : dates civiles

**Files :**
- Create : `crates/lf-domain/src/date.rs`
- Modify : `crates/lf-domain/src/lib.rs`

**Interfaces :**
- Consumes : `DomainError::InvalidDate`, `echo`.
- Produces : `CivilDate` (`Copy`, `Ord`) : `new(u16, u8, u8) -> Option<Self>`, `parse(&str) -> Result<Self, DomainError>`, `from_unix_millis(i64) -> Option<Self>`, `year()`, `month()`, `day()`, `to_french(self) -> String` ; `Display` (`AAAA-MM-JJ`), `FromStr` ; alias `VersionDate = CivilDate`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod date;` (ordre alphabétique).

Créer `crates/lf-domain/src/date.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn d(y: u16, m: u8, day: u8) -> CivilDate {
        CivilDate::new(y, m, day).unwrap()
    }

    /// Inverse indépendant (`days_from_civil`, même source) servant d'oracle.
    fn days_from_civil(date: CivilDate) -> i64 {
        let (m, dd) = (i64::from(date.month()), i64::from(date.day()));
        let y = i64::from(date.year()) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + dd - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    #[test]
    fn new_validates_calendar() {
        assert!(CivilDate::new(2024, 2, 29).is_some());
        assert!(CivilDate::new(2023, 2, 29).is_none());
        assert!(CivilDate::new(2000, 2, 29).is_some());
        assert!(CivilDate::new(1900, 2, 29).is_none());
        assert!(CivilDate::new(2023, 2, 28).is_some());
        assert!(CivilDate::new(2023, 4, 31).is_none());
        assert!(CivilDate::new(2023, 4, 30).is_some());
        for month in [6, 9, 11] {
            assert!(CivilDate::new(2023, month, 31).is_none());
            assert!(CivilDate::new(2023, month, 30).is_some());
        }
        assert!(CivilDate::new(2023, 12, 31).is_some());
        assert!(CivilDate::new(2023, 1, 0).is_none());
        assert!(CivilDate::new(2023, 0, 1).is_none());
        assert!(CivilDate::new(2023, 13, 1).is_none());
        assert!(CivilDate::new(0, 1, 1).is_none());
        assert!(CivilDate::new(1, 1, 1).is_some());
        assert!(CivilDate::new(9999, 12, 31).is_some());
        assert!(CivilDate::new(10_000, 1, 1).is_none());
    }

    #[test]
    fn parse_reads_iso_dates() {
        assert_eq!(CivilDate::parse("2023-09-08").unwrap(), d(2023, 9, 8));
        assert_eq!(" 2999-01-01 ".parse::<CivilDate>().unwrap(), d(2999, 1, 1));
    }

    #[test]
    fn parse_rejects_other_shapes() {
        for bad in [
            "",
            "2023-9-08",
            "2023-09-8",
            "20230908",
            "2023/09/08",
            "08-09-2023",
            "2023-02-30",
            "2023-13-01",
            "0000-01-01",
            "2023-0a-01",
            "2023-01-0a",
            "a023-01-01",
            "2a23-01-01",
            "20a3-01-01",
            "202a-01-01",
            "2023-a1-01",
            "2023-01-a1",
            "+2023-01-01",
            "2023-01-01T00:00",
        ] {
            assert_eq!(
                CivilDate::parse(bad).unwrap_err(),
                DomainError::InvalidDate { input: bad.into() },
                "{bad:?}"
            );
        }
    }

    #[test]
    fn from_unix_millis_matches_fixtures() {
        // consult_juri_constit.json, consult_loda.json, consult_article.json
        assert_eq!(CivilDate::from_unix_millis(1_762_473_600_000), Some(d(2025, 11, 7)));
        assert_eq!(CivilDate::from_unix_millis(1_475_884_800_000), Some(d(2016, 10, 8)));
        assert_eq!(CivilDate::from_unix_millis(17_193_600_000), Some(d(1970, 7, 19)));
        assert_eq!(CivilDate::from_unix_millis(32_472_144_000_000), Some(d(2999, 1, 1)));
    }

    #[test]
    fn from_unix_millis_edges() {
        assert_eq!(CivilDate::from_unix_millis(0), Some(d(1970, 1, 1)));
        assert_eq!(CivilDate::from_unix_millis(-1), Some(d(1969, 12, 31)));
        assert_eq!(CivilDate::from_unix_millis(MILLIS_PER_DAY - 1), Some(d(1970, 1, 1)));
        assert_eq!(CivilDate::from_unix_millis(951_782_400_000), Some(d(2000, 2, 29)));
        assert_eq!(CivilDate::from_unix_millis(951_868_800_000), Some(d(2000, 3, 1)));
        assert_eq!(CivilDate::from_unix_millis(i64::MIN), None);
        assert_eq!(CivilDate::from_unix_millis(i64::MAX), None);
        assert_eq!(CivilDate::from_unix_millis(-62_135_596_800_001), None); // 0000-12-31
    }

    #[test]
    fn getters_and_display() {
        let date = d(2025, 3, 7);
        assert_eq!((date.year(), date.month(), date.day()), (2025, 3, 7));
        assert_eq!(date.to_string(), "2025-03-07");
        assert_eq!(d(812, 1, 2).to_string(), "0812-01-02");
    }

    #[test]
    fn french_prose() {
        assert_eq!(d(2025, 11, 7).to_french(), "7 novembre 2025");
        assert_eq!(d(2026, 1, 1).to_french(), "1er janvier 2026");
        assert_eq!(d(2026, 2, 2).to_french(), "2 février 2026");
        assert_eq!(d(2026, 8, 15).to_french(), "15 août 2026");
        assert_eq!(d(2026, 12, 31).to_french(), "31 décembre 2026");
    }

    proptest! {
        #[test]
        fn civil_roundtrips_through_oracle(y in 1u16..=9999, m in 1u8..=12, day in 1u8..=31) {
            if let Some(date) = CivilDate::new(y, m, day) {
                let days = days_from_civil(date);
                prop_assert_eq!(civil_from_days(days), Some(date));
                prop_assert_eq!(CivilDate::from_unix_millis(days * MILLIS_PER_DAY + 3_600_000), Some(date));
                prop_assert_eq!(CivilDate::parse(&date.to_string()).unwrap(), date);
            }
        }

        #[test]
        fn parse_never_panics(s in "\\PC*") {
            let _ = CivilDate::parse(&s);
        }
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain date::`
Expected : échec de compilation, symboles introuvables (par exemple `CivilDate`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/date.rs` :

```rust
//! Dates civiles (calendrier grégorien, sans fuseau).
//!
//! Les dates PISTE arrivent en millisecondes epoch à minuit UTC (vérifié sur
//! les fixtures : `consult_juri_constit.json` `dateTexte` = 2025-11-07T00:00Z),
//! d'où la conversion UTC de [`CivilDate::from_unix_millis`] (`citations.py:66-73`).

use std::{fmt, str::FromStr};

use crate::error::{DomainError, echo};

/// Noms de mois français, accentués (décision Q9 : la référence les écrit
/// sans accents, `citations.py:283-286`).
const FR_MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

const MILLIS_PER_DAY: i64 = 86_400_000;

/// Date civile validée, années 1 à 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CivilDate {
    year: u16,
    month: u8,
    day: u8,
}

/// Date de version d'un texte consolidé (`get_act`).
pub type VersionDate = CivilDate;

fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn digit(b: u8) -> Option<u8> {
    b.is_ascii_digit().then(|| b - b'0')
}

impl CivilDate {
    /// Construit une date si elle existe dans le calendrier.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let valid = (1..=9999).contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day);
        valid.then_some(Self { year, month, day })
    }

    /// Lit une date `AAAA-MM-JJ` (espaces autour tolérés).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidDate`] si la forme ou la date est invalide.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        Self::parse_iso(input.trim().as_bytes())
            .ok_or_else(|| DomainError::InvalidDate { input: echo(input) })
    }

    fn parse_iso(bytes: &[u8]) -> Option<Self> {
        let &[y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2] = bytes else {
            return None;
        };
        let year = u16::from(digit(y1)?) * 1000
            + u16::from(digit(y2)?) * 100
            + u16::from(digit(y3)?) * 10
            + u16::from(digit(y4)?);
        let month = digit(m1)? * 10 + digit(m2)?;
        let day = digit(d1)? * 10 + digit(d2)?;
        Self::new(year, month, day)
    }

    /// Jour UTC d'un instant exprimé en millisecondes epoch.
    pub fn from_unix_millis(millis: i64) -> Option<Self> {
        civil_from_days(millis.div_euclid(MILLIS_PER_DAY))
    }

    /// Année.
    pub fn year(self) -> u16 {
        self.year
    }

    /// Mois, 1 à 12.
    pub fn month(self) -> u8 {
        self.month
    }

    /// Jour, 1 à 31.
    pub fn day(self) -> u8 {
        self.day
    }

    /// Date en prose française : `1er janvier 2026`, `7 novembre 2025`.
    pub fn to_french(self) -> String {
        let month = FR_MONTHS[usize::from(self.month - 1)];
        if self.day == 1 {
            format!("1er {month} {}", self.year)
        } else {
            format!("{} {month} {}", self.day, self.year)
        }
    }
}

/// Jours depuis 1970-01-01 vers date civile (algorithme `civil_from_days`
/// de H. Hinnant, <https://howardhinnant.github.io/date_algorithms.html>).
fn civil_from_days(days: i64) -> Option<CivilDate> {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Hinnant : mp ∈ [0, 11], doy ∈ [0, 365], donc month ∈ [1, 12] et day ∈ [1, 31].
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (month, day) = (month as u8, day as u8);
    CivilDate::new(u16::try_from(year).ok()?, month, day)
}

impl fmt::Display for CivilDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for CivilDate {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}
```

L'oracle du proptest, `days_from_civil`, est un algorithme inverse indépendant. Il tue les mutants arithmétiques de `civil_from_days`, ce que des dates choisies à la main ne font pas toutes.

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain date:: && make lint fmt-check`
Expected : `9 tests run: 9 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/date.rs`
Expected : `114 mutants tested in …: 109 caught, 5 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/date.rs
git commit -m "feat(domain): dates civiles, epoch UTC et prose française"
```

---

## Task 7 : ECLI

**Files :**
- Create : `crates/lf-domain/src/ecli.rs`
- Modify : `crates/lf-domain/src/lib.rs`

**Interfaces :**
- Consumes : `DomainError::InvalidEcli`, `echo`.
- Produces : `Ecli` : `parse(&str) -> Result<Self, DomainError>`, `as_str(&self) -> &str`, `Display`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod ecli;` (ordre alphabétique).

Créer `crates/lf-domain/src/ecli.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn accepts_fixture_eclis() {
        for ecli in [
            "ECLI:FR:CC:2025:2025.1173.QPC",
            "ECLI:FR:CCASS:2025:C100399",
            "ECLI:FR:CECHR:2026:506507.20260529",
        ] {
            assert_eq!(Ecli::parse(ecli).unwrap().as_str(), ecli);
        }
        assert_eq!(Ecli::parse(" ECLI:FR:CC:2025:1 ").unwrap().to_string(), "ECLI:FR:CC:2025:1");
    }

    #[test]
    fn length_bounds() {
        assert!(Ecli::parse("ECLI:FR:ABCDEF7:2025:1").is_ok());
        assert!(Ecli::parse("ECLI:FR:ABCDEFG8:2025:1").is_err());
        assert!(Ecli::parse(&format!("ECLI:FR:CC:2025:{}", "1".repeat(25))).is_ok());
        assert!(Ecli::parse(&format!("ECLI:FR:CC:2025:{}", "1".repeat(26))).is_err());
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "ECLI",
            "ecli:FR:CC:2025:1",
            "ECLI:BE:CC:2025:1",
            "ECLI:FR::2025:1",
            "ECLI:FR:cc:2025:1",
            "ECLI:FR:C-C:2025:1",
            "ECLI:FR:CC:25:1",
            "ECLI:FR:CC:20255:1",
            "ECLI:FR:CC:2O25:1",
            "ECLI:FR:CC:2025:",
            "ECLI:FR:CC:2025:1-2",
            "ECLI:FR:CC:2025:1:extra",
            "ECLI:FR:CC:2025",
        ] {
            assert_eq!(
                Ecli::parse(bad).unwrap_err(),
                DomainError::InvalidEcli { input: bad.into() },
                "{bad:?}"
            );
        }
    }

    proptest! {
        #[test]
        fn parse_never_panics(s in "\\PC*") {
            let _ = Ecli::parse(&s);
        }
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain ecli::`
Expected : échec de compilation, symboles introuvables (par exemple `Ecli`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/ecli.rs` :

```rust
//! European Case Law Identifier, tel que renvoyé par PISTE — jamais fabriqué.
//!
//! Forme : `ECLI:FR:<juridiction 1-7>:<année 4>:<numéro 1-25>` (conclusions du
//! Conseil de l'UE 2011/C 127/01, annexe). Exemples des fixtures :
//! `ECLI:FR:CC:2025:2025.1173.QPC`, `ECLI:FR:CCASS:2025:C100399`,
//! `ECLI:FR:CECHR:2026:506507.20260529`.

use std::fmt;

use crate::error::{DomainError, echo};

/// ECLI français validé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ecli(String);

fn court_ok(court: &str) -> bool {
    (1..=7).contains(&court.len())
        && court.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn year_ok(year: &str) -> bool {
    year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit())
}

fn ordinal_ok(ordinal: &str) -> bool {
    (1..=25).contains(&ordinal.len())
        && ordinal.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

impl Ecli {
    /// Valide `input` (espaces autour tolérés).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidEcli`] si la forme ne convient pas.
    pub fn parse(input: &str) -> Result<Self, DomainError> {
        let trimmed = input.trim();
        let mut parts = trimmed.split(':');
        let valid = parts.next() == Some("ECLI")
            && parts.next() == Some("FR")
            && parts.next().is_some_and(court_ok)
            && parts.next().is_some_and(year_ok)
            && parts.next().is_some_and(ordinal_ok)
            && parts.next().is_none();
        if valid {
            Ok(Self(trimmed.to_owned()))
        } else {
            Err(DomainError::InvalidEcli { input: echo(input) })
        }
    }

    /// Forme textuelle.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ecli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain ecli:: && make lint fmt-check`
Expected : `4 tests run: 4 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/ecli.rs`
Expected : `24 mutants tested in …: 23 caught, 1 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/ecli.rs
git commit -m "feat(domain): ECLI validé, jamais fabriqué"
```

---

## Task 8 : URL Légifrance

**Files :**
- Create : `crates/lf-domain/src/url.rs`
- Modify : `crates/lf-domain/src/lib.rs`

**Interfaces :**
- Consumes : `Fond` (tâche 5).
- Produces : `WEB_BASE` ; `ResourceKind::{Loda, Code, CodeArticle, Juri, Constit, Cetat, Jorf, Cnil, Kali, Acco}` ; `impl From<Fond> for ResourceKind` ; `legifrance_url(ResourceKind, &str) -> String`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod url;` (ordre alphabétique).

Créer `crates/lf-domain/src/url.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn urls_match_reference() {
        let id = "X";
        let cases = [
            (ResourceKind::Loda, "https://www.legifrance.gouv.fr/loda/id/X"),
            (ResourceKind::Code, "https://www.legifrance.gouv.fr/codes/texte_lc/X"),
            (ResourceKind::CodeArticle, "https://www.legifrance.gouv.fr/codes/article_lc/X"),
            (ResourceKind::Juri, "https://www.legifrance.gouv.fr/juri/id/X"),
            (ResourceKind::Constit, "https://www.legifrance.gouv.fr/cons/id/X"),
            (ResourceKind::Cetat, "https://www.legifrance.gouv.fr/ceta/id/X"),
            (ResourceKind::Jorf, "https://www.legifrance.gouv.fr/jorf/id/X"),
            (ResourceKind::Cnil, "https://www.legifrance.gouv.fr/cnil/id/X"),
            (ResourceKind::Kali, "https://www.legifrance.gouv.fr/conv_coll/id/X"),
            (ResourceKind::Acco, "https://www.legifrance.gouv.fr/acco/id/X"),
        ];
        for (kind, url) in cases {
            assert_eq!(legifrance_url(kind, id), url);
        }
    }

    #[test]
    fn fond_maps_to_its_page_kind() {
        let kinds: Vec<_> = Fond::ALL.into_iter().map(ResourceKind::from).collect();
        assert_eq!(
            kinds,
            [
                ResourceKind::Loda,
                ResourceKind::Code,
                ResourceKind::Juri,
                ResourceKind::Constit,
                ResourceKind::Cetat,
                ResourceKind::Cnil,
                ResourceKind::Kali,
                ResourceKind::Acco,
            ]
        );
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain url::`
Expected : échec de compilation, symboles introuvables (par exemple `ResourceKind`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/url.rs` :

```rust
//! URL stables de legifrance.gouv.fr (`citations.py:76-102`).

use crate::fond::Fond;

/// Racine du site public.
pub const WEB_BASE: &str = "https://www.legifrance.gouv.fr";

/// Type de page Légifrance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// Loi ou décret : `/loda/id/`.
    Loda,
    /// Code : `/codes/texte_lc/`.
    Code,
    /// Article `LEGIARTI` : `/codes/article_lc/`.
    CodeArticle,
    /// Décision judiciaire : `/juri/id/`.
    Juri,
    /// Décision du Conseil constitutionnel : `/cons/id/`.
    Constit,
    /// Décision administrative : `/ceta/id/`.
    Cetat,
    /// Texte du JO : `/jorf/id/`.
    Jorf,
    /// Délibération CNIL : `/cnil/id/`.
    Cnil,
    /// Convention collective et ses articles : `/conv_coll/id/`.
    Kali,
    /// Accord d'entreprise : `/acco/id/`.
    Acco,
}

impl ResourceKind {
    fn path(self) -> &'static str {
        match self {
            Self::Loda => "loda/id",
            Self::Code => "codes/texte_lc",
            Self::CodeArticle => "codes/article_lc",
            Self::Juri => "juri/id",
            Self::Constit => "cons/id",
            Self::Cetat => "ceta/id",
            Self::Jorf => "jorf/id",
            Self::Cnil => "cnil/id",
            Self::Kali => "conv_coll/id",
            Self::Acco => "acco/id",
        }
    }
}

impl From<Fond> for ResourceKind {
    fn from(fond: Fond) -> Self {
        match fond {
            Fond::Loda => Self::Loda,
            Fond::Code => Self::Code,
            Fond::Juri => Self::Juri,
            Fond::Constit => Self::Constit,
            Fond::Cetat => Self::Cetat,
            Fond::Cnil => Self::Cnil,
            Fond::Kali => Self::Kali,
            Fond::Acco => Self::Acco,
        }
    }
}

/// URL publique d'une ressource ; `id` est un identifiant déjà validé.
pub fn legifrance_url(kind: ResourceKind, id: &str) -> String {
    format!("{WEB_BASE}/{}/{id}", kind.path())
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain url:: && make lint fmt-check`
Expected : `2 tests run: 2 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/url.rs`
Expected : `5 mutants tested in …: 4 caught, 1 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/url.rs
git commit -m "feat(domain): URL stables legifrance.gouv.fr"
```

---

## Task 9 : contrat et règles de citation

**Files :**
- Create : `crates/lf-domain/src/citation.rs`
- Modify : `crates/lf-domain/src/lib.rs`

**Interfaces :**
- Consumes : `CivilDate::to_french` (tâche 6).
- Produces : `ELI_NOTE` ; `CitationContract { eli_uri: String, human_readable_citation: Option<String>, source_url: String }` avec `from_url(String, Option<String>) -> Self` ; `trait Citable { fn citation(&self) -> CitationContract; }` ; `article_citation(Option<&str>, Option<&str>) -> Option<String>` ; `constit_citation(num, nature, Option<CivilDate>, titre) -> Option<String>` ; `cnil_citation(Option<&str>, Option<CivilDate>) -> Option<String>` ; `with_company(Option<&str>, Option<&str>) -> Option<String>`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod citation;` (ordre alphabétique).

Créer `crates/lf-domain/src/citation.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn nov_7() -> Option<CivilDate> {
        CivilDate::new(2025, 11, 7)
    }

    #[test]
    fn contract_uses_url_twice() {
        let c = CitationContract::from_url("https://u".into(), Some("Cit".into()));
        assert_eq!(c.eli_uri, "https://u");
        assert_eq!(c.source_url, "https://u");
        assert_eq!(c.human_readable_citation.as_deref(), Some("Cit"));
    }

    #[test]
    fn article_rules() {
        assert_eq!(
            article_citation(Some("Code civil"), Some("9")).as_deref(),
            Some("Code civil, art. 9")
        );
        assert_eq!(article_citation(None, Some("9")).as_deref(), Some("Art. 9"));
        assert_eq!(article_citation(Some("Code civil"), None).as_deref(), Some("Code civil"));
        assert_eq!(article_citation(None, None), None);
    }

    #[test]
    fn constit_full_citation_from_fixture() {
        // consult_juri_constit.json : num 2025-1173, nature QPC, 2025-11-07.
        let titre = "Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de publicité des débats]";
        assert_eq!(
            constit_citation(Some("2025-1173"), Some("qpc"), nov_7(), Some(titre)).as_deref(),
            Some(
                "Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 - Absence de publicité des débats"
            )
        );
    }

    #[test]
    fn constit_optional_parts() {
        assert_eq!(
            constit_citation(Some("2021-817"), None, nov_7(), Some("pas de crochets")).as_deref(),
            Some("Cons. const., décision n° 2021-817 du 7 novembre 2025")
        );
        assert_eq!(
            constit_citation(Some("2021-817"), Some(""), nov_7(), None).as_deref(),
            Some("Cons. const., décision n° 2021-817 du 7 novembre 2025")
        );
        assert_eq!(constit_citation(None, Some("DC"), nov_7(), None), None);
        assert_eq!(constit_citation(Some("2021-817"), Some("DC"), None, None), None);
    }

    #[test]
    fn bracket_edges() {
        assert_eq!(bracketed("a [b] [c]"), Some("b"));
        assert_eq!(bracketed("a []"), None);
        assert_eq!(bracketed("a [b\nc]"), None);
        assert_eq!(bracketed("a [b"), None);
        assert_eq!(bracketed("a b]"), None);
    }

    #[test]
    fn cnil_rules() {
        let jan_8 = CivilDate::new(2026, 1, 8);
        assert_eq!(
            cnil_citation(Some("SAN-2026-002"), jan_8).as_deref(),
            Some("CNIL, délibération n° SAN-2026-002 du 8 janvier 2026")
        );
        assert_eq!(cnil_citation(None, jan_8), None);
        assert_eq!(cnil_citation(Some("SAN-2026-002"), None), None);
    }

    #[test]
    fn company_rules() {
        assert_eq!(
            with_company(Some("Télétravail"), Some("CORSAIR")).as_deref(),
            Some("Télétravail (CORSAIR)")
        );
        assert_eq!(with_company(Some("Télétravail"), None).as_deref(), Some("Télétravail"));
        assert_eq!(with_company(None, Some("CORSAIR")), None);
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain citation::`
Expected : échec de compilation, symboles introuvables (par exemple `CitationContract`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/citation.rs` :

```rust
//! Contrat de citation et conventions de citation françaises.
//!
//! CONSTITUTION art. 4 de la référence : toute entité citable porte `eli_uri`,
//! `human_readable_citation` et `source_url`. PISTE renvoie un ELI nul pour la
//! législation : `eli_uri` porte donc l'URL Légifrance stable, jamais un ELI
//! fabriqué (`citations.py:8-23`).

use crate::date::CivilDate;

/// Note jointe aux résultats de législation (`models.py:14-17`).
pub const ELI_NOTE: &str = "Legifrance lf-engine-app returns the ELI field null for legislation; \
'eli_uri' is the stable CID-keyed legifrance.gouv.fr resource URL, not a synthesized /eli/ identifier.";

/// Contrat de citation porté par toute entité citable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationContract {
    /// URL Légifrance stable (pas d'ELI fabriqué).
    pub eli_uri: String,
    /// Citation selon l'usage français, absente si la source ne la permet pas.
    pub human_readable_citation: Option<String>,
    /// Page legifrance.gouv.fr de la ressource.
    pub source_url: String,
}

impl CitationContract {
    /// `eli_uri` et `source_url` reçoivent tous deux `url`.
    pub fn from_url(url: String, human_readable_citation: Option<String>) -> Self {
        Self { eli_uri: url.clone(), human_readable_citation, source_url: url }
    }
}

/// Entité qui sait produire son contrat de citation.
pub trait Citable {
    /// Contrat de citation de l'entité.
    fn citation(&self) -> CitationContract;
}

/// `Code civil, art. 9` ; `Art. 9` sans titre de code (`citations.py:256-261`).
pub fn article_citation(code_title: Option<&str>, num: Option<&str>) -> Option<String> {
    match (code_title, num) {
        (Some(code), Some(num)) => Some(format!("{code}, art. {num}")),
        (None, Some(num)) => Some(format!("Art. {num}")),
        (code, None) => code.map(str::to_owned),
    }
}

/// Premier contenu entre crochets, non vide et sur une seule ligne.
fn bracketed(titre: &str) -> Option<&str> {
    let (_, after) = titre.split_once('[')?;
    let (inner, _) = after.split_once(']')?;
    (!inner.is_empty() && !inner.contains('\n')).then_some(inner)
}

/// `Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 - <objet>`
/// (`citations.py:298-318`, accents rétablis : décision Q9).
pub fn constit_citation(
    num: Option<&str>,
    nature: Option<&str>,
    date: Option<CivilDate>,
    titre: Option<&str>,
) -> Option<String> {
    let (num, date) = (num?, date?);
    let mut out = format!("Cons. const., décision n° {num}");
    if let Some(nature) = nature.filter(|n| !n.is_empty()) {
        out.push(' ');
        out.push_str(&nature.to_uppercase());
    }
    out.push_str(" du ");
    out.push_str(&date.to_french());
    if let Some(subject) = titre.and_then(bracketed) {
        out.push_str(" - ");
        out.push_str(subject);
    }
    Some(out)
}

/// `CNIL, délibération n° SAN-2026-002 du 8 janvier 2026` (`citations.py:376-380`).
pub fn cnil_citation(num: Option<&str>, date: Option<CivilDate>) -> Option<String> {
    Some(format!("CNIL, délibération n° {} du {}", num?, date?.to_french()))
}

/// `<titre> (<raison sociale>)` pour un accord d'entreprise (`citations.py:458`).
pub fn with_company(title: Option<&str>, company: Option<&str>) -> Option<String> {
    match (title, company) {
        (Some(title), Some(company)) => Some(format!("{title} ({company})")),
        (title, _) => title.map(str::to_owned),
    }
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain citation:: && make lint fmt-check`
Expected : `7 tests run: 7 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/citation.rs`
Expected : `20 mutants tested in …: 19 caught, 1 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/citation.rs
git commit -m "feat(domain): contrat et conventions de citation françaises"
```

---

## Task 10 : nettoyage HTML

**Files :**
- Create : `crates/lf-domain/src/text/clean.rs`
- Modify : `crates/lf-domain/src/lib.rs`
- Create : `crates/lf-domain/src/text/mod.rs`

**Interfaces :**
- Consumes : rien.
- Produces : `lf_domain::text::clean(&str) -> String`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod text;` (ordre alphabétique).

Créer `crates/lf-domain/src/text/mod.rs` :

```rust
//! Utilitaires texte purs : nettoyage HTML, fenêtres, passages, classement BM25.

mod clean;

pub use clean::clean;
```

Créer `crates/lf-domain/src/text/clean.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn strips_search_marks() {
        assert_eq!(
            clean("Loi pour une sécurité globale préservant les <mark>libertés</mark>  - réserve"),
            "Loi pour une sécurité globale préservant les libertés - réserve"
        );
    }

    #[test]
    fn line_breaks_and_paragraphs() {
        assert_eq!(clean("a<br>b<BR></BR>c<br/>d"), "a\nb\nc\nd");
        assert_eq!(clean("<p>un</p><p>deux</p>"), "un\n\ndeux");
        assert_eq!(clean("a<br><br><br><br>b"), "a\n\nb");
    }

    #[test]
    fn keeps_link_text_and_unknown_tags_content() {
        assert_eq!(
            clean("voir <a href=\"/x\">article 9</a> et <RECH>ici</RECH>"),
            "voir article 9 et ici"
        );
    }

    #[test]
    fn keeps_literal_angle_brackets() {
        assert_eq!(clean("a < b et c > d"), "a < b et c > d");
        assert_eq!(clean("a <3"), "a <3");
        assert_eq!(clean("x <a <b>"), "x <a");
        assert_eq!(clean("fin <"), "fin <");
        assert_eq!(clean("<>"), "<>");
        assert_eq!(clean("</>"), "</>");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(clean("A &amp; B &lt;C&gt; &quot;d&quot; l&apos;e"), "A & B <C> \"d\" l'e");
        assert_eq!(clean("art.&nbsp;9 &#233;t&#xE9; &#XE9;"), "art. 9 été é");
    }

    #[test]
    fn keeps_unknown_or_broken_entities() {
        assert_eq!(
            clean("&unknown; &#xZZ; &#; & seul &#1114112;"),
            "&unknown; &#xZZ; &#; & seul &#1114112;"
        );
        assert_eq!(clean("&verylongentityname;"), "&verylongentityname;");
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(clean("  a \t b\r\n\n\n\n  c  "), "a b\n\nc");
        assert_eq!(clean("\u{a0}a\u{202f}:"), "a :");
        assert_eq!(clean(""), "");
        assert_eq!(clean("<br><p>"), "");
    }

    proptest! {
        #[test]
        fn never_panics_and_is_trimmed(s in "\\PC*") {
            let out = clean(&s);
            prop_assert_eq!(out.trim(), out.as_str());
            prop_assert!(!out.contains("\n\n\n"));
        }

        #[test]
        fn plain_text_is_unchanged(words in proptest::collection::vec("[a-zA-Zéèà0-9.,;:]{1,8}", 1..20)) {
            let text = words.join(" ");
            prop_assert_eq!(clean(&text), text);
        }
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain text::clean::`
Expected : échec de compilation, symboles introuvables (par exemple `clean`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/text/clean.rs` :

```rust
//! HTML Légifrance vers texte brut.
//!
//! Balises relevées dans les fixtures de la référence : `<br>`, `<BR></BR>`,
//! `<mark>` (surlignage de recherche), `<p>`, `<RECH>`, `<a>`. Aucune entité
//! dans les fixtures ; les entités XML de base et numériques sont tout de même
//! décodées (texte HTML).

/// Nettoie un fragment HTML : balises retirées, `<br>` → saut de ligne,
/// `<p>` → paragraphe, entités décodées, espaces normalisés.
pub fn clean(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find(['<', '&']) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let consumed = if tail.starts_with('<') {
            tag(tail).map(|(name, closing, len)| {
                out.push_str(tag_text(&name, closing));
                len
            })
        } else {
            entity(tail).map(|(ch, len)| {
                out.push(ch);
                len
            })
        };
        let len = consumed.unwrap_or_else(|| {
            out.push_str(&tail[..1]);
            1
        });
        // Au moins un octet consommé par tour : la boucle termine toujours.
        rest = &tail[len.max(1)..];
    }
    out.push_str(rest);
    normalize_whitespace(&out)
}

/// Balise en tête de `tail` : (nom en minuscules, fermante, longueur avec `>`).
fn tag(tail: &str) -> Option<(String, bool, usize)> {
    let end = tail.find('>')?;
    let inner = &tail[1..end];
    if inner.contains('<') {
        return None;
    }
    let (closing, body) = inner.strip_prefix('/').map_or((false, inner), |b| (true, b));
    let name: String = body
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let starts_with_letter = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    starts_with_letter.then_some((name, closing, end + 1))
}

fn tag_text(name: &str, closing: bool) -> &'static str {
    match (name, closing) {
        ("br", false) => "\n",
        ("p", _) => "\n\n",
        _ => "",
    }
}

/// Entité en tête de `tail` : (caractère, longueur avec `;`).
fn entity(tail: &str) -> Option<(char, usize)> {
    let end = tail.get(..12).unwrap_or(tail).find(';')?;
    let name = &tail[1..end];
    let ch = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let code =
                if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    name.strip_prefix('#')?.parse().ok()?
                };
            char::from_u32(code)?
        }
    };
    Some((ch, end + 1))
}

/// Espaces internes réduits à un, lignes rognées, au plus une ligne vide.
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_blank = false;
    for line in s.split('\n') {
        let mut words = line.split_whitespace().peekable();
        if words.peek().is_none() {
            pending_blank = true;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if pending_blank { "\n\n" } else { "\n" });
        }
        pending_blank = false;
        for (i, word) in words.enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(word);
        }
    }
    out
}
```

`rest = &tail[len.max(1)..]` garantit la terminaison, même sous mutation. Sans cette ligne, le mutant `tag -> Some((String::new(), true, 0))` fait boucler `clean()` : cargo-mutants le classe `TIMEOUT` et sort en code 3, ce qui bloque la CI (constaté à l'essai à blanc).

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain text::clean:: && make lint fmt-check`
Expected : `9 tests run: 9 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/text/clean.rs`
Expected : `34 mutants tested in …: 34 caught`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/text/mod.rs crates/lf-domain/src/text/clean.rs
git commit -m "feat(domain): nettoyage du HTML Légifrance"
```

---

## Task 11 : entités consultables

**Files :**
- Create : `crates/lf-domain/src/entity/act.rs`
- Create : `crates/lf-domain/src/entity/article.rs`
- Create : `crates/lf-domain/src/entity/decision.rs`
- Create : `crates/lf-domain/src/entity/deliberation.rs`
- Create : `crates/lf-domain/src/entity/agreement.rs`
- Modify : `crates/lf-domain/src/lib.rs`
- Create : `crates/lf-domain/src/entity/mod.rs`

**Interfaces :**
- Consumes : ids (tâche 4), `CivilDate` (6), `Ecli` (7), `legifrance_url` (8), règles de citation (9).
- Produces : `TocEntry`, `TextBody` (`Default`), `Act { text_id: LegiTextId, body }`, `Convention { text_id: KaliTextId, body }`, `ArticleText`, `Decision`, `Deliberation`, `CompanyAgreement`, `ATTACHMENT_NOTE`. Chacune implémente `Citable` ; champs publics listés dans le code.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod entity;` (ordre alphabétique).

Créer `crates/lf-domain/src/entity/mod.rs` :

```rust
//! Entités renvoyées par le corpus, chacune [`Citable`](crate::citation::Citable).

mod act;
mod agreement;
mod article;
mod decision;
mod deliberation;

pub use act::{Act, Convention, TextBody, TocEntry};
pub use agreement::{ATTACHMENT_NOTE, CompanyAgreement};
pub use article::ArticleText;
pub use decision::Decision;
pub use deliberation::Deliberation;
```

Créer `crates/lf-domain/src/entity/act.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn body(title: &str) -> TextBody {
        TextBody { title: Some(title.into()), ..TextBody::default() }
    }

    #[test]
    fn act_cites_its_title_and_loda_url() {
        let act = Act {
            text_id: LegiTextId::parse("LEGITEXT000033205014").unwrap(),
            body: body("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique"),
        };
        assert_eq!(
            act.citation(),
            CitationContract::from_url(
                "https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014".into(),
                Some("LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique".into()),
            )
        );
    }

    #[test]
    fn convention_uses_conv_coll_url() {
        let convention = Convention {
            text_id: KaliTextId::parse("KALITEXT000049878950").unwrap(),
            body: body("Avenant"),
        };
        let citation = convention.citation();
        assert_eq!(
            citation.source_url,
            "https://www.legifrance.gouv.fr/conv_coll/id/KALITEXT000049878950"
        );
        assert_eq!(citation.human_readable_citation.as_deref(), Some("Avenant"));
    }
}
```

Créer `crates/lf-domain/src/entity/article.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn article(id: &str) -> ArticleText {
        ArticleText {
            article_id: ArticleId::parse(id).unwrap(),
            num: Some("9".into()),
            etat: None,
            code_title: Some("Code civil".into()),
            full_sections_titre: None,
            date_debut: None,
            date_fin: None,
            text: None,
        }
    }

    #[test]
    fn legi_article_cites_code_and_number() {
        let citation = article("LEGIARTI000006419288").citation();
        assert_eq!(
            citation,
            CitationContract::from_url(
                "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288".into(),
                Some("Code civil, art. 9".into()),
            )
        );
    }

    #[test]
    fn kali_article_uses_conv_coll_url() {
        // test_kali_article_url_uses_conv_coll de la référence.
        let citation = article("KALIARTI000005805295").citation();
        assert_eq!(
            citation.source_url,
            "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295"
        );
    }
}
```

Créer `crates/lf-domain/src/entity/decision.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn decision(id: &str) -> Decision {
        Decision {
            decision_id: DecisionId::parse(id).unwrap(),
            ecli: None,
            num: Some("2025-1173".into()),
            juridiction: None,
            formation: None,
            solution: None,
            numero_affaire: vec![],
            date_texte: CivilDate::new(2025, 11, 7),
            nature: Some("QPC".into()),
            title: Some("Titre Légifrance [Objet]".into()),
            text: None,
        }
    }

    #[test]
    fn juri_cites_title() {
        let c = decision("JURITEXT000048465611").citation();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/juri/id/JURITEXT000048465611");
        assert_eq!(c.human_readable_citation.as_deref(), Some("Titre Légifrance [Objet]"));
    }

    #[test]
    fn cetat_cites_title() {
        let c = decision("CETATEXT000051655317").citation();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/ceta/id/CETATEXT000051655317");
        assert_eq!(c.human_readable_citation.as_deref(), Some("Titre Légifrance [Objet]"));
    }

    #[test]
    fn constit_uses_french_convention() {
        let c = decision("CONSTEXT000037434572").citation();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000037434572");
        assert_eq!(
            c.human_readable_citation.as_deref(),
            Some("Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 - Objet")
        );
    }

    #[test]
    fn constit_falls_back_to_title() {
        let mut d = decision("CONSTEXT000037434572");
        d.num = None;
        assert_eq!(
            d.citation().human_readable_citation.as_deref(),
            Some("Titre Légifrance [Objet]")
        );
    }
}
```

Créer `crates/lf-domain/src/entity/deliberation.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn deliberation(num: Option<&str>) -> Deliberation {
        Deliberation {
            deliberation_id: CnilTextId::parse("CNILTEXT000053352643").unwrap(),
            num: num.map(Into::into),
            nature: None,
            nature_delib: None,
            etat: None,
            date_texte: CivilDate::new(2026, 1, 8),
            date_publication: None,
            title: Some("Délibération SAN-2026-002 du 8 janvier 2026".into()),
            title_long: None,
            text: None,
        }
    }

    #[test]
    fn cites_with_cnil_convention() {
        let c = deliberation(Some("SAN-2026-002")).citation();
        assert_eq!(c.source_url, "https://www.legifrance.gouv.fr/cnil/id/CNILTEXT000053352643");
        assert_eq!(
            c.human_readable_citation.as_deref(),
            Some("CNIL, délibération n° SAN-2026-002 du 8 janvier 2026")
        );
    }

    #[test]
    fn falls_back_to_title() {
        let c = deliberation(None).citation();
        assert_eq!(
            c.human_readable_citation.as_deref(),
            Some("Délibération SAN-2026-002 du 8 janvier 2026")
        );
    }
}
```

Créer `crates/lf-domain/src/entity/agreement.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn cites_title_and_company() {
        let agreement = CompanyAgreement {
            agreement_id: AccoTextId::parse("ACCOTEXT000051888026").unwrap(),
            nature: None,
            title: Some("Télétravail".into()),
            raison_sociale: Some("CORSAIR".into()),
            siret: Some("32862158600143".into()),
            idcc: Some("275".into()),
            code_ape: None,
            secteur: None,
            themes: vec![],
            syndicats: vec![],
            date_texte: None,
            date_effet: None,
            date_depot: None,
        };
        assert_eq!(
            agreement.citation(),
            CitationContract::from_url(
                "https://www.legifrance.gouv.fr/acco/id/ACCOTEXT000051888026".into(),
                Some("Télétravail (CORSAIR)".into()),
            )
        );
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain entity::`
Expected : échec de compilation, symboles introuvables (par exemple `Act`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/entity/act.rs` :

```rust
//! Textes consolidés : loi/décret (LODA) et convention collective (KALI).
//! Même forme côté API (`citations.py:204-228` et `403-436`).

use crate::{
    citation::{Citable, CitationContract},
    date::CivilDate,
    id::{ArticleId, KaliTextId, LegiTextId},
    url::{ResourceKind, legifrance_url},
};

/// Entrée de table des matières : un article résoluble par `get_article`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché, ex. `L. 1234-5`.
    pub num: Option<String>,
    /// État juridique brut, ex. `VIGUEUR`.
    pub etat: Option<String>,
}

/// Métadonnées et sommaire communs aux textes consolidés.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBody {
    /// Identifiant chronologique (CID).
    pub cid: Option<String>,
    /// Numéro NOR.
    pub nor: Option<String>,
    /// Titre officiel.
    pub title: Option<String>,
    /// Nature, ex. `LOI`.
    pub nature: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// Date de parution au JO.
    pub date_parution: Option<CivilDate>,
    /// Numéro de parution au JO.
    pub num_parution: Option<String>,
    /// Début de la version consolidée.
    pub date_debut_version: Option<CivilDate>,
    /// Fin de la version consolidée (`2999-01-01` = sans fin connue).
    pub date_fin_version: Option<CivilDate>,
    /// Articles, dans l'ordre du texte.
    pub articles: Vec<TocEntry>,
}

/// Loi ou décret consolidé (`fr_get_act`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Act {
    /// Identifiant du texte.
    pub text_id: LegiTextId,
    /// Métadonnées et sommaire.
    pub body: TextBody,
}

/// Convention collective (`fr_get_convention`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convention {
    /// Identifiant du texte.
    pub text_id: KaliTextId,
    /// Métadonnées et sommaire.
    pub body: TextBody,
}

impl Citable for Act {
    fn citation(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Loda, self.text_id.as_str()),
            self.body.title.clone(),
        )
    }
}

impl Citable for Convention {
    fn citation(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Kali, self.text_id.as_str()),
            self.body.title.clone(),
        )
    }
}
```

Insérer au-dessus du module de tests de `crates/lf-domain/src/entity/article.rs` :

```rust
//! Article seul (`fr_get_text`, `citations.py:248-276`).

use crate::{
    citation::{Citable, CitationContract, article_citation},
    date::CivilDate,
    id::ArticleId,
    url::{ResourceKind, legifrance_url},
};

/// Texte d'un article de code, de loi ou de convention collective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleText {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché.
    pub num: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// Titre du code (entrée `textTitles` de nature `CODE` en priorité).
    pub code_title: Option<String>,
    /// Chemin des sections.
    pub full_sections_titre: Option<String>,
    /// Début de vigueur.
    pub date_debut: Option<CivilDate>,
    /// Fin de vigueur.
    pub date_fin: Option<CivilDate>,
    /// Texte brut de l'article.
    pub text: Option<String>,
}

impl Citable for ArticleText {
    fn citation(&self) -> CitationContract {
        let kind = match self.article_id {
            ArticleId::Legi(_) => ResourceKind::CodeArticle,
            ArticleId::Kali(_) => ResourceKind::Kali,
        };
        CitationContract::from_url(
            legifrance_url(kind, self.article_id.as_str()),
            article_citation(self.code_title.as_deref(), self.num.as_deref()),
        )
    }
}
```

Insérer au-dessus du module de tests de `crates/lf-domain/src/entity/decision.rs` :

```rust
//! Décision JURI / CONSTIT / CETAT (`fr_get_decision`, `citations.py:321-355`).
//!
//! La référence choisit l'URL d'après le champ `origine` ; ici le préfixe de
//! l'identifiant en tient lieu (`CONSTEXT` ⇔ origine `CONSTIT`, vérifié sur
//! `consult_juri_constit.json` et `consult_juri_cetat.json`).

use crate::{
    citation::{Citable, CitationContract, constit_citation},
    date::CivilDate,
    ecli::Ecli,
    id::DecisionId,
    url::{ResourceKind, legifrance_url},
};

/// Décision de justice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Identifiant de la décision.
    pub decision_id: DecisionId,
    /// ECLI natif, absent pour une partie des CAA/TA — jamais fabriqué.
    pub ecli: Option<Ecli>,
    /// Numéro (Conseil constitutionnel : `2025-1173`).
    pub num: Option<String>,
    /// Juridiction.
    pub juridiction: Option<String>,
    /// Formation.
    pub formation: Option<String>,
    /// Solution.
    pub solution: Option<String>,
    /// Numéros d'affaire, ex. `24-13.470`.
    pub numero_affaire: Vec<String>,
    /// Date de la décision.
    pub date_texte: Option<CivilDate>,
    /// Nature (Conseil constitutionnel : `QPC`, `DC`…).
    pub nature: Option<String>,
    /// Titre Légifrance.
    pub title: Option<String>,
    /// Texte intégral.
    pub text: Option<String>,
}

impl Citable for Decision {
    fn citation(&self) -> CitationContract {
        let (kind, citation) = match self.decision_id {
            DecisionId::Juri(_) => (ResourceKind::Juri, self.title.clone()),
            DecisionId::Cetat(_) => (ResourceKind::Cetat, self.title.clone()),
            DecisionId::Constit(_) => (
                ResourceKind::Constit,
                constit_citation(
                    self.num.as_deref(),
                    self.nature.as_deref(),
                    self.date_texte,
                    self.title.as_deref(),
                )
                .or_else(|| self.title.clone()),
            ),
        };
        CitationContract::from_url(legifrance_url(kind, self.decision_id.as_str()), citation)
    }
}
```

Insérer au-dessus du module de tests de `crates/lf-domain/src/entity/deliberation.rs` :

```rust
//! Délibération CNIL (`fr_get_deliberation`, `citations.py:363-395`).

use crate::{
    citation::{Citable, CitationContract, cnil_citation},
    date::CivilDate,
    id::CnilTextId,
    url::{ResourceKind, legifrance_url},
};

/// Délibération de la CNIL ; ECLI et ELI toujours nuls côté API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deliberation {
    /// Identifiant.
    pub deliberation_id: CnilTextId,
    /// Numéro, ex. `SAN-2026-002`.
    pub num: Option<String>,
    /// Nature.
    pub nature: Option<String>,
    /// Nature de délibération.
    pub nature_delib: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// Date de la délibération.
    pub date_texte: Option<CivilDate>,
    /// Date de publication.
    pub date_publication: Option<CivilDate>,
    /// Titre court.
    pub title: Option<String>,
    /// Titre long.
    pub title_long: Option<String>,
    /// Texte intégral.
    pub text: Option<String>,
}

impl Citable for Deliberation {
    fn citation(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Cnil, self.deliberation_id.as_str()),
            cnil_citation(self.num.as_deref(), self.date_texte).or_else(|| self.title.clone()),
        )
    }
}
```

Insérer au-dessus du module de tests de `crates/lf-domain/src/entity/agreement.rs` :

```rust
//! Accord d'entreprise, métadonnées seules (`fr_get_company_agreement`,
//! `citations.py:444-486`) : le texte n'existe qu'en pièce jointe `.docx`.

use crate::{
    citation::{Citable, CitationContract, with_company},
    date::CivilDate,
    id::AccoTextId,
    url::{ResourceKind, legifrance_url},
};

/// Note jointe à chaque accord (`citations.py:479-482`).
pub const ATTACHMENT_NOTE: &str = "Full text is only distributed by Legifrance as a .docx \
attachment; consult source_url for the human-readable page. This tool returns metadata only.";

/// Accord d'entreprise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyAgreement {
    /// Identifiant.
    pub agreement_id: AccoTextId,
    /// Nature.
    pub nature: Option<String>,
    /// Titre.
    pub title: Option<String>,
    /// Raison sociale de l'entreprise.
    pub raison_sociale: Option<String>,
    /// SIRET.
    pub siret: Option<String>,
    /// Convention collective de rattachement (IDCC).
    pub idcc: Option<String>,
    /// Code APE.
    pub code_ape: Option<String>,
    /// Secteur.
    pub secteur: Option<String>,
    /// Thèmes.
    pub themes: Vec<String>,
    /// Syndicats signataires.
    pub syndicats: Vec<String>,
    /// Date du texte.
    pub date_texte: Option<CivilDate>,
    /// Date d'effet.
    pub date_effet: Option<CivilDate>,
    /// Date de dépôt.
    pub date_depot: Option<CivilDate>,
}

impl Citable for CompanyAgreement {
    fn citation(&self) -> CitationContract {
        CitationContract::from_url(
            legifrance_url(ResourceKind::Acco, self.agreement_id.as_str()),
            with_company(self.title.as_deref(), self.raison_sociale.as_deref()),
        )
    }
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain entity:: && make lint fmt-check`
Expected : `11 tests run: 11 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/entity/act.rs`
Expected : `WARN No mutants were viable` : attendu ici. `citation()` renvoie `CitationContract`, qui n'a pas de `Default`, donc le remplacement du corps ne compile pas. La logique est couverte par les tests unitaires (100 % des régions).

Run : `make mutants FILE=crates/lf-domain/src/entity/article.rs`
Expected : `WARN No mutants were viable` : attendu ici. `citation()` renvoie `CitationContract`, qui n'a pas de `Default`, donc le remplacement du corps ne compile pas. La logique est couverte par les tests unitaires (100 % des régions).

Run : `make mutants FILE=crates/lf-domain/src/entity/decision.rs`
Expected : `WARN No mutants were viable` : attendu ici. `citation()` renvoie `CitationContract`, qui n'a pas de `Default`, donc le remplacement du corps ne compile pas. La logique est couverte par les tests unitaires (100 % des régions).

Run : `make mutants FILE=crates/lf-domain/src/entity/deliberation.rs`
Expected : `WARN No mutants were viable` : attendu ici. `citation()` renvoie `CitationContract`, qui n'a pas de `Default`, donc le remplacement du corps ne compile pas. La logique est couverte par les tests unitaires (100 % des régions).

Run : `make mutants FILE=crates/lf-domain/src/entity/agreement.rs`
Expected : `WARN No mutants were viable` : attendu ici. `citation()` renvoie `CitationContract`, qui n'a pas de `Default`, donc le remplacement du corps ne compile pas. La logique est couverte par les tests unitaires (100 % des régions).

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/entity/mod.rs crates/lf-domain/src/entity/act.rs crates/lf-domain/src/entity/article.rs crates/lf-domain/src/entity/decision.rs crates/lf-domain/src/entity/deliberation.rs crates/lf-domain/src/entity/agreement.rs
git commit -m "feat(domain): entités consultables et leur citation"
```

---

## Task 12 : résultats de recherche

**Files :**
- Create : `crates/lf-domain/src/entity/search.rs`
- Modify : `crates/lf-domain/src/entity/mod.rs`

**Interfaces :**
- Consumes : `Fond` (5), `AnyId`, `ArticleId` (4), `text::clean` (10), `with_company` (9).
- Produces : `SNIPPET_MAX_CHARS = 280` ; `CodeArticleRef::new(ArticleId, Option<String>, &str) -> Self` ; `SearchHit { fond, id: AnyId, cid: Option<AnyId>, title, nature, etat, idcc, raison_sociale, articles }` (`Citable`) ; `SearchPage::new(Fond, &str, Option<u64>, Vec<SearchHit>) -> Self`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/entity/mod.rs`, ajouter `mod search;` après `mod deliberation;` et `pub use search::{CodeArticleRef, SNIPPET_MAX_CHARS, SearchHit, SearchPage};` en fin de fichier.

Créer `crates/lf-domain/src/entity/search.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn hit(fond: Fond, id: &str, cid: Option<&str>) -> SearchHit {
        SearchHit {
            fond,
            id: AnyId::parse(id).unwrap(),
            cid: cid.map(|c| AnyId::parse(c).unwrap()),
            title: Some("Titre".into()),
            nature: None,
            etat: None,
            idcc: None,
            raison_sociale: Some("CORSAIR".into()),
            articles: vec![],
        }
    }

    #[test]
    fn loda_and_code_use_id() {
        let loda = hit(Fond::Loda, "JORFTEXT000020445850", Some("JORFTEXT000099999999"));
        assert_eq!(
            loda.citation().source_url,
            "https://www.legifrance.gouv.fr/loda/id/JORFTEXT000020445850"
        );
        let code = hit(Fond::Code, "LEGITEXT000006070721", Some("LEGITEXT000099999999"));
        assert_eq!(
            code.citation().source_url,
            "https://www.legifrance.gouv.fr/codes/texte_lc/LEGITEXT000006070721"
        );
    }

    #[test]
    fn other_fonds_prefer_cid() {
        let constit = hit(Fond::Constit, "CONSTEXT000043596386", Some("CONSTEXT000011111111"));
        assert_eq!(
            constit.citation().source_url,
            "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000011111111"
        );
        let juri = hit(Fond::Juri, "JURITEXT000048465611", None);
        assert_eq!(
            juri.citation().source_url,
            "https://www.legifrance.gouv.fr/juri/id/JURITEXT000048465611"
        );
        assert_eq!(juri.citation().human_readable_citation.as_deref(), Some("Titre"));
    }

    #[test]
    fn acco_cites_company() {
        let acco = hit(Fond::Acco, "ACCOTEXT000051888026", None);
        assert_eq!(acco.citation().human_readable_citation.as_deref(), Some("Titre (CORSAIR)"));
    }

    #[test]
    fn snippet_is_cleaned_and_cut() {
        let id = ArticleId::parse("LEGIARTI000006419288").unwrap();
        let r = CodeArticleRef::new(
            id.clone(),
            Some("9".into()),
            "Chacun a droit au <mark>respect</mark>",
        );
        assert_eq!(r.snippet, "Chacun a droit au respect");
        let long = "é".repeat(SNIPPET_MAX_CHARS + 10);
        assert_eq!(CodeArticleRef::new(id, None, &long).snippet.chars().count(), SNIPPET_MAX_CHARS);
    }

    #[test]
    fn page_total_defaults_to_hit_count() {
        let hits = vec![hit(Fond::Juri, "JURITEXT000048465611", None)];
        let page = SearchPage::new(Fond::Juri, "  responsabilité ", None, hits.clone());
        assert_eq!((page.total, page.query.as_str()), (1, "responsabilité"));
        assert_eq!(SearchPage::new(Fond::Juri, "q", Some(1817), hits).total, 1817);
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain entity::search::`
Expected : échec de compilation, symboles introuvables (par exemple `SearchHit`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/entity/search.rs` :

```rust
//! Résultats de recherche (`fr_search`, `citations.py:110-182`).

use crate::{
    citation::{Citable, CitationContract, with_company},
    fond::Fond,
    id::{AnyId, ArticleId},
    text::clean,
    url::{ResourceKind, legifrance_url},
};

/// Longueur maximale d'un extrait d'article, en caractères (`citations.py:162`).
pub const SNIPPET_MAX_CHARS: usize = 280;

/// Article trouvé dans un résultat de recherche sur les codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeArticleRef {
    /// Identifiant de l'article.
    pub article_id: ArticleId,
    /// Numéro affiché.
    pub num: Option<String>,
    /// Extrait nettoyé, au plus [`SNIPPET_MAX_CHARS`] caractères.
    pub snippet: String,
}

impl CodeArticleRef {
    /// Nettoie l'extrait brut (balises `<mark>`…) puis le coupe.
    pub fn new(article_id: ArticleId, num: Option<String>, raw_snippet: &str) -> Self {
        let snippet = clean(raw_snippet).chars().take(SNIPPET_MAX_CHARS).collect();
        Self { article_id, num, snippet }
    }
}

/// Un résultat de recherche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Fond interrogé.
    pub fond: Fond,
    /// Identifiant du texte, suffixe de version retiré.
    pub id: AnyId,
    /// Identifiant chronologique, préféré pour l'URL hors LODA/CODE.
    pub cid: Option<AnyId>,
    /// Titre nettoyé.
    pub title: Option<String>,
    /// Nature.
    pub nature: Option<String>,
    /// État juridique brut.
    pub etat: Option<String>,
    /// IDCC (KALI, ACCO).
    pub idcc: Option<String>,
    /// Raison sociale (ACCO).
    pub raison_sociale: Option<String>,
    /// Articles trouvés (CODE).
    pub articles: Vec<CodeArticleRef>,
}

impl Citable for SearchHit {
    fn citation(&self) -> CitationContract {
        let key = match self.fond {
            Fond::Loda | Fond::Code => &self.id,
            _ => self.cid.as_ref().unwrap_or(&self.id),
        };
        let citation = match self.fond {
            Fond::Acco => with_company(self.title.as_deref(), self.raison_sociale.as_deref()),
            _ => self.title.clone(),
        };
        CitationContract::from_url(
            legifrance_url(ResourceKind::from(self.fond), key.as_str()),
            citation,
        )
    }
}

/// Page de résultats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    /// Fond interrogé.
    pub fond: Fond,
    /// Requête, espaces retirés.
    pub query: String,
    /// Nombre total de résultats annoncé par la source.
    pub total: u64,
    /// Résultats de la page.
    pub hits: Vec<SearchHit>,
}

impl SearchPage {
    /// `total` absent : on retient le nombre de résultats reçus (`citations.py:179-181`).
    pub fn new(fond: Fond, query: &str, total: Option<u64>, hits: Vec<SearchHit>) -> Self {
        let total = total.unwrap_or_else(|| u64::try_from(hits.len()).unwrap_or(u64::MAX));
        Self { fond, query: query.trim().to_owned(), total, hits }
    }
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain entity::search:: && make lint fmt-check`
Expected : `5 tests run: 5 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/entity/search.rs`
Expected : `3 mutants tested in …: 2 caught, 1 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/entity/mod.rs crates/lf-domain/src/entity/search.rs
git commit -m "feat(domain): résultats de recherche"
```

---

## Task 13 : couverture déclarée

**Files :**
- Create : `crates/lf-domain/src/coverage.rs`
- Modify : `crates/lf-domain/src/lib.rs`

**Interfaces :**
- Consumes : `DomainError::EmptyCoverageGaps`, `CivilDate`.
- Produces : `CoverageStatus::{Ok, Degraded, Failed}`, `CoverageFamily`, `CoverageGap` (champs publics) ; `Coverage::new(status, String, Vec<CoverageFamily>, Vec<CoverageGap>) -> Result<Coverage, DomainError>`, avec les getters `status()`, `as_of_note()`, `families()` et `known_gaps()`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/lib.rs`, ajouter `pub mod coverage;` (ordre alphabétique).

Créer `crates/lf-domain/src/coverage.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn gap() -> CoverageGap {
        CoverageGap {
            id: "FR-001".into(),
            family: "Legislation (LODA, codes)".into(),
            missing: "native ELI".into(),
            fallback: "use eli_uri".into(),
        }
    }

    fn family() -> CoverageFamily {
        CoverageFamily {
            name: "Case law (JURI)".into(),
            tool: "fr_get_decision".into(),
            source: "legifrance.gouv.fr via the PISTE API".into(),
            captured_at: None,
            live: true,
        }
    }

    #[test]
    fn refuses_empty_gaps() {
        assert_eq!(
            Coverage::new(CoverageStatus::Ok, "note".into(), vec![family()], vec![]),
            Err(DomainError::EmptyCoverageGaps)
        );
    }

    #[test]
    fn exposes_its_parts() {
        let c = Coverage::new(CoverageStatus::Degraded, "note".into(), vec![family()], vec![gap()])
            .unwrap();
        assert_eq!(c.status(), CoverageStatus::Degraded);
        assert_eq!(c.as_of_note(), "note");
        assert_eq!(c.families(), [family()]);
        assert_eq!(c.known_gaps(), [gap()]);
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain coverage::`
Expected : échec de compilation, symboles introuvables (par exemple `Coverage`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/coverage.rs` :

```rust
//! Couverture déclarée du connecteur (`coverage.py`, élément 9 du canon `MateMatic`).
//!
//! Invariant : la liste des lacunes connues n'est jamais vide — une liste vide
//! voudrait dire « personne n'a vérifié », pas « aucune lacune » (`coverage.py:1-8`).
//! Les données déclarées (qui nomment les outils MCP) vivent dans l'adapter MCP.

use crate::{date::CivilDate, error::DomainError};

/// État de la source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    /// Source joignable.
    Ok,
    /// Source partiellement joignable.
    Degraded,
    /// Source injoignable.
    Failed,
}

/// Famille de données exposée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageFamily {
    /// Nom de la famille.
    pub name: String,
    /// Outil(s) qui y donnent accès.
    pub tool: String,
    /// Source officielle.
    pub source: String,
    /// Date de capture, absente pour une interrogation en direct.
    pub captured_at: Option<CivilDate>,
    /// Interrogée en direct plutôt que depuis un instantané.
    pub live: bool,
}

/// Lacune connue et moyen de la contourner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageGap {
    /// Identifiant stable, ex. `FR-001`.
    pub id: String,
    /// Famille concernée.
    pub family: String,
    /// Ce qui manque.
    pub missing: String,
    /// Où aller à la place.
    pub fallback: String,
}

/// Couverture du connecteur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    status: CoverageStatus,
    as_of_note: String,
    families: Vec<CoverageFamily>,
    known_gaps: Vec<CoverageGap>,
}

impl Coverage {
    /// Construit la déclaration.
    ///
    /// # Errors
    ///
    /// [`DomainError::EmptyCoverageGaps`] si `known_gaps` est vide.
    pub fn new(
        status: CoverageStatus,
        as_of_note: String,
        families: Vec<CoverageFamily>,
        known_gaps: Vec<CoverageGap>,
    ) -> Result<Self, DomainError> {
        if known_gaps.is_empty() {
            return Err(DomainError::EmptyCoverageGaps);
        }
        Ok(Self { status, as_of_note, families, known_gaps })
    }

    /// État de la source.
    pub fn status(&self) -> CoverageStatus {
        self.status
    }

    /// Ce que signifient (et ne garantissent pas) les dates.
    pub fn as_of_note(&self) -> &str {
        &self.as_of_note
    }

    /// Familles exposées.
    pub fn families(&self) -> &[CoverageFamily] {
        &self.families
    }

    /// Lacunes connues, jamais vides.
    pub fn known_gaps(&self) -> &[CoverageGap] {
        &self.known_gaps
    }
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain coverage:: && make lint fmt-check`
Expected : `2 tests run: 2 passed`, aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/coverage.rs`
Expected : `7 mutants tested in …: 4 caught, 3 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/lib.rs crates/lf-domain/src/coverage.rs
git commit -m "feat(domain): couverture déclarée, lacunes jamais vides"
```

---

## Task 14 : fenêtres et passages

**Files :**
- Create : `crates/lf-domain/src/text/window.rs`
- Create : `crates/lf-domain/src/text/passages.rs`
- Modify : `crates/lf-domain/src/text/mod.rs`

**Interfaces :**
- Consumes : rien.
- Produces : `Window<'a> { text: &'a str, next_offset: Option<usize> }`, `window(&str, offset: usize, max_chars: usize) -> Window<'_>` (positions en caractères) ; `Passage<'a> { offset: usize, text: &'a str }`, `passages(&str, max_chars: usize) -> Vec<Passage<'_>>`.

- [ ] **Step 1 : écrire les tests qui échouent**

Dans `crates/lf-domain/src/text/mod.rs`, ajouter `mod passages;` et `mod window;` après `mod clean;`, puis `pub use passages::{Passage, passages};` et `pub use window::{Window, window};` après `pub use clean::clean;`.

Créer `crates/lf-domain/src/text/window.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn whole_text_fits() {
        assert_eq!(window("abc", 0, 10), Window { text: "abc", next_offset: None });
        assert_eq!(window("abc", 0, 3), Window { text: "abc", next_offset: None });
        assert_eq!(window("", 0, 10), Window { text: "", next_offset: None });
    }

    #[test]
    fn offset_past_end_is_empty() {
        assert_eq!(window("abc", 99, 10), Window { text: "", next_offset: None });
    }

    #[test]
    fn prefers_sentence_end() {
        let text = "Une phrase. Puis une autre phrase longue";
        assert_eq!(window(text, 0, 20), Window { text: "Une phrase. ", next_offset: Some(12) });
    }

    #[test]
    fn prefers_newline() {
        let text = "Ligne une\nligne deux suite";
        assert_eq!(window(text, 0, 14), Window { text: "Ligne une\n", next_offset: Some(10) });
    }

    #[test]
    fn falls_back_to_last_space() {
        let text = "mot mot mot mot mot";
        assert_eq!(window(text, 0, 10), Window { text: "mot mot ", next_offset: Some(8) });
    }

    #[test]
    fn ignores_boundaries_in_first_half() {
        // Seule fin de phrase avant la moitié : on prend le dernier espace.
        let text = "A. bbbbbbb cccccccccccccc";
        assert_eq!(window(text, 0, 12), Window { text: "A. bbbbbbb ", next_offset: Some(11) });
        // Aucun espace dans la seconde moitié : coupe dure.
        let text = "A. bbbbbbbbbbbbbbbbbbbbbb";
        assert_eq!(window(text, 0, 10), Window { text: "A. bbbbbbb", next_offset: Some(10) });
    }

    #[test]
    fn boundary_exactly_at_half_is_accepted() {
        // head = "abc defg" (8 octets), min = 4 ; l'espace est à 3 < 4 : refusé.
        assert_eq!(window("abc defgh", 0, 8), Window { text: "abc defg", next_offset: Some(8) });
        // head = "abcd efg" ; l'espace est à 4 = min : accepté.
        assert_eq!(window("abcd efgh", 0, 8), Window { text: "abcd ", next_offset: Some(5) });
    }

    #[test]
    fn offsets_are_in_chars() {
        let text = "été été été";
        assert_eq!(window(text, 4, 4), Window { text: "été ", next_offset: Some(8) });
    }

    #[test]
    fn zero_max_chars_still_progresses() {
        assert_eq!(window("ab", 0, 0), Window { text: "a", next_offset: Some(1) });
    }

    proptest! {
        #[test]
        fn windows_partition_the_text(text in "\\PC{0,300}", max in 1usize..60) {
            let mut rebuilt = String::new();
            let mut offset = 0;
            loop {
                let w = window(&text, offset, max);
                prop_assert!(w.text.chars().count() <= max);
                rebuilt.push_str(w.text);
                match w.next_offset {
                    Some(next) => {
                        prop_assert!(next > offset);
                        offset = next;
                    }
                    None => break,
                }
            }
            prop_assert_eq!(rebuilt, text);
        }
    }
}
```

Créer `crates/lf-domain/src/text/passages.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn splits_on_sentences_with_offsets() {
        let text = "Première phrase. Deuxième phrase. Troisième.";
        assert_eq!(
            passages(text, 20),
            [
                Passage { offset: 0, text: "Première phrase. " },
                Passage { offset: 17, text: "Deuxième phrase. " },
                Passage { offset: 34, text: "Troisième." },
            ]
        );
    }

    #[test]
    fn empty_text_has_no_passage() {
        assert_eq!(passages("", 10), []);
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain text::`
Expected : échec de compilation, symboles introuvables (par exemple `window`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/text/window.rs` :

```rust
//! Fenêtres de lecture paginées (`offset` / `max_chars`, jalon M5).
//!
//! Positions en caractères (pas en octets) : sûres en UTF-8 et compréhensibles
//! par un modèle. Les fenêtres successives partitionnent exactement le texte.

/// Fenêtre de texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window<'a> {
    /// Contenu de la fenêtre.
    pub text: &'a str,
    /// Position de la fenêtre suivante, absente en fin de texte.
    pub next_offset: Option<usize>,
}

/// Fenêtre d'au plus `max_chars` caractères (au moins 1) à partir de `offset`,
/// coupée de préférence après une fin de phrase, sinon après un espace, dans la
/// seconde moitié de la fenêtre.
pub fn window(text: &str, offset: usize, max_chars: usize) -> Window<'_> {
    let rest = &text[byte_at(text, offset)..];
    let hard = byte_at(rest, max_chars.max(1));
    if hard == rest.len() {
        return Window { text: rest, next_offset: None };
    }
    let head = &rest[..hard];
    let cut = soft_cut(head).unwrap_or(hard);
    let chunk = &rest[..cut];
    Window { text: chunk, next_offset: Some(offset + chunk.chars().count()) }
}

/// Indice en octets du caractère n° `chars`, ou longueur de `s`.
fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(i, _)| i)
}

/// Coupe juste après un espace, dans la seconde moitié de `head` : d'abord un
/// espace précédé d'une fin de phrase ou un saut de ligne, sinon tout espace.
fn soft_cut(head: &str) -> Option<usize> {
    let min = head.len() / 2;
    let mut fallback = None;
    let mut chars = head.char_indices().rev().peekable();
    while let Some((i, c)) = chars.next() {
        if i < min {
            break;
        }
        if !c.is_whitespace() {
            continue;
        }
        let end = i + c.len_utf8();
        let after_sentence =
            chars.peek().is_some_and(|&(_, p)| matches!(p, '.' | '!' | '?' | '…' | ';'));
        if c == '\n' || after_sentence {
            return Some(end);
        }
        fallback = fallback.or(Some(end));
    }
    fallback
}
```

Insérer au-dessus du module de tests de `crates/lf-domain/src/text/passages.rs` :

```rust
//! Découpe d'un texte long en passages adressables par `offset`.

use super::window::window;

/// Passage de texte et sa position (en caractères) dans le texte complet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Passage<'a> {
    /// Position du premier caractère.
    pub offset: usize,
    /// Contenu.
    pub text: &'a str,
}

/// Fenêtres successives d'au plus `max_chars` caractères couvrant tout `text`.
pub fn passages(text: &str, max_chars: usize) -> Vec<Passage<'_>> {
    let mut out = Vec::new();
    let mut offset = Some(0);
    // Chaque fenêtre avance d'au moins un caractère : `len + 1` tours suffisent,
    // et la borne garantit la terminaison quoi qu'il arrive.
    for _ in 0..=text.len() {
        let Some(start) = offset else { break };
        let w = window(text, start, max_chars);
        if !w.text.is_empty() {
            out.push(Passage { offset: start, text: w.text });
        }
        offset = w.next_offset;
    }
    out
}
```

Dans `passages()`, la boucle est bornée à `text.len() + 1` tours : une fenêtre avance toujours d'au moins un caractère, et la borne empêche tout `TIMEOUT` si un mutant de `window()` cesse d'avancer. Dans le proptest, `prop_assert!(next > offset)` fait échouer vite au lieu de boucler.

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain text:: && make lint fmt-check`
Expected : `21 tests run: 21 passed` (21 = 9 (clean) + 10 (window) + 2 (passages)), aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/text/window.rs`
Expected : `19 mutants tested in …: 18 caught, 1 unviable`, aucun `missed`.

Run : `make mutants FILE=crates/lf-domain/src/text/passages.rs`
Expected : `3 mutants tested in …: 2 caught, 1 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/text/mod.rs crates/lf-domain/src/text/window.rs crates/lf-domain/src/text/passages.rs
git commit -m "feat(domain): fenêtres paginées et passages"
```

---

## Task 15 : tokenisation et classement BM25

**Files :**
- Create : `crates/lf-domain/src/text/fold.rs`
- Create : `crates/lf-domain/src/text/rank.rs`
- Modify : `crates/lf-domain/src/text/mod.rs`

**Interfaces :**
- Consumes : rien.
- Produces : `pub(crate) fold(&str) -> String`, `pub(crate) tokens(&str) -> Vec<String>` ; `K1 = 1.2`, `B = 0.75`, `Scored { index: usize, score: f64 }`, `rank(&[&str], query: &str, k: usize) -> Vec<Scored>`.

- [ ] **Step 1 : écrire les tests qui échouent**

Remplacer `crates/lf-domain/src/text/mod.rs` par sa forme finale :

```rust
//! Utilitaires texte purs : nettoyage HTML, fenêtres, passages, classement BM25.

mod clean;
mod fold;
mod passages;
mod rank;
mod window;

pub use clean::clean;
pub use passages::{Passage, passages};
pub use rank::{B, K1, Scored, rank};
pub use window::{Window, window};
```

Créer `crates/lf-domain/src/text/fold.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn folds_french_accents() {
        assert_eq!(fold("ÀÂÄ Ç ÉÈÊË ÎÏ ÔÖ ÙÛÜ Ÿ Œuvre Æ"), "aaa c eeee ii oo uuu y oeuvre ae");
    }

    #[test]
    fn tokenizes_and_drops_single_chars() {
        assert_eq!(
            tokens("L'article L. 1234-5 du Code « civil »"),
            ["article", "1234", "du", "code", "civil"]
        );
        assert_eq!(tokens(""), Vec::<String>::new());
    }
}
```

Créer `crates/lf-domain/src/text/rank.rs` avec le seul module de tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Score arrondi à 12 décimales, comparable avec `assert_eq!`.
    fn r12(score: f64) -> String {
        format!("{score:.12}")
    }

    #[test]
    fn single_term_score_matches_hand_computation() {
        // N = 2, df = 1 : idf = ln(1 + 1.5 / 1.5) = ln 2.
        // doc 0 : 2 jetons, avg = 2.5 : len_norm = 0.25 + 0.75 * 0.8 = 0.85.
        // score = ln 2 * 1 * 2.2 / (1 + 1.2 * 0.85) = ln 2 * 2.2 / 2.02.
        let ranked = rank(&["droit civil", "code pénal article"], "civil", 5);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].index, 0);
        assert_eq!(r12(ranked[0].score), r12(2.0f64.ln() * 2.2 / 2.02));
    }

    #[test]
    fn term_frequency_and_length_matter() {
        // N = 3, df("bail") = 2 : idf = ln(1 + 1.5 / 2.5) = ln 1.6 ; avg = 8 / 3.
        let ranked = rank(&["bail bail commercial", "bail rural de location", "vente"], "bail", 5);
        let idf = 1.6f64.ln();
        let avg = 8.0 / 3.0;
        let s0 = idf * 2.0 * 2.2 / (2.0 + 1.2 * (0.25 + 0.75 * 3.0 / avg));
        let s1 = idf * 1.0 * 2.2 / (1.0 + 1.2 * (0.25 + 0.75 * 4.0 / avg));
        assert_eq!(ranked.iter().map(|s| s.index).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(r12(ranked[0].score), r12(s0));
        assert_eq!(r12(ranked[1].score), r12(s1));
    }

    #[test]
    fn multiple_terms_add_up_and_fold_accents() {
        let ranked = rank(
            &["responsabilité civile délictuelle", "responsabilite", "autre chose"],
            "Responsabilité CIVILE",
            5,
        );
        assert_eq!(ranked.iter().map(|s| s.index).collect::<Vec<_>>(), [0, 1]);
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn ties_keep_input_order_and_k_truncates() {
        let ranked = rank(&["x loyer", "loyer x", "rien"], "loyer", 1);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].index, 0);
    }

    #[test]
    fn degenerate_inputs_give_nothing() {
        assert_eq!(rank(&[], "loyer", 5), []);
        assert_eq!(rank(&["", "a"], "loyer", 5), []);
        assert_eq!(rank(&["loyer"], "a", 5), []);
        assert_eq!(rank(&["loyer"], "bail", 5), []);
        assert_eq!(rank(&["loyer"], "loyer", 0), []);
    }
}
```

- [ ] **Step 2 : les voir échouer**

Run : `cargo nextest run -p lf-domain text::`
Expected : échec de compilation, symboles introuvables (par exemple `rank`).

- [ ] **Step 3 : implémenter**

Insérer au-dessus du module de tests de `crates/lf-domain/src/text/fold.rs` :

```rust
//! Tokenisation pour la recherche plein texte : minuscules, accents français
//! repliés, découpe sur les non-alphanumériques, jetons d'au moins 2 caractères.

/// Minuscules et accents français repliés (`é` → `e`, `œ` → `oe`).
pub(crate) fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' => out.push('a'),
            'ç' => out.push('c'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'î' | 'ï' => out.push('i'),
            'ô' | 'ö' => out.push('o'),
            'ù' | 'û' | 'ü' => out.push('u'),
            'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            other => out.push(other),
        }
    }
    out
}

/// Jetons repliés d'au moins 2 caractères.
pub(crate) fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().nth(1).is_some())
        .map(str::to_owned)
        .collect()
}
```

Insérer au-dessus du module de tests de `crates/lf-domain/src/text/rank.rs` :

```rust
//! Classement BM25 des passages pour le paramètre `focus` (jalon M5).
//!
//! Okapi BM25, IDF de Lucene `ln(1 + (N - df + 0.5) / (df + 0.5))` (toujours
//! positif), `k1 = 1.2`, `b = 0.75`.

use std::collections::HashSet;

use super::fold::tokens;

/// Saturation de la fréquence des termes.
pub const K1: f64 = 1.2;
/// Normalisation par la longueur.
pub const B: f64 = 0.75;

/// Passage classé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// Indice du passage dans l'entrée.
    pub index: usize,
    /// Score BM25, strictement positif.
    pub score: f64,
}

/// Les `k` meilleurs passages pour `query`, score décroissant puis indice
/// croissant ; les passages sans terme commun sont exclus.
#[allow(clippy::cast_precision_loss)] // comptes de passages et de jetons ≪ 2^52
pub fn rank(passages: &[&str], query: &str, k: usize) -> Vec<Scored> {
    let docs: Vec<Vec<String>> = passages.iter().map(|p| tokens(p)).collect();
    let terms: HashSet<String> = tokens(query).into_iter().collect();
    let total_len: usize = docs.iter().map(Vec::len).sum();
    // Aucun jeton (y compris aucun passage) : rien à classer, et pas de 0/0.
    // Une requête sans jeton donne des scores nuls, filtrés plus bas.
    if total_len == 0 {
        return Vec::new();
    }
    let n = docs.len() as f64;
    let avg_len = total_len as f64 / n;
    let mut scored: Vec<Scored> = docs
        .iter()
        .enumerate()
        .filter_map(|(index, doc)| {
            let len_norm = 1.0 - B + B * doc.len() as f64 / avg_len;
            let score: f64 = terms
                .iter()
                .map(|term| {
                    let tf = doc.iter().filter(|t| *t == term).count() as f64;
                    let df = docs.iter().filter(|d| d.contains(term)).count() as f64;
                    let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
                    idf * tf * (K1 + 1.0) / (tf + K1 * len_norm)
                })
                .sum();
            (score > 0.0).then_some(Scored { index, score })
        })
        .collect();
    scored.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.index.cmp(&b.index)));
    scored.truncate(k);
    scored
}
```

- [ ] **Step 4 : tests verts, lint**

Run : `cargo nextest run -p lf-domain text:: && make lint fmt-check`
Expected : `28 tests run: 28 passed` (28 = 21 + 2 (fold) + 5 (rank)), aucun avertissement.

- [ ] **Step 5 : couverture**

Run : `make cov`
Expected : `TOTAL` à 100.00 % (lignes, régions, fonctions). Une ligne ou une région manquante appelle un test de plus, jamais un seuil abaissé.

- [ ] **Step 6 : mutation**

Run : `make mutants FILE=crates/lf-domain/src/text/fold.rs`
Expected : `6 mutants tested in …: 6 caught`, aucun `missed`.

Run : `make mutants FILE=crates/lf-domain/src/text/rank.rs`
Expected : `39 mutants tested in …: 38 caught, 1 unviable`, aucun `missed`.

Un mutant `MISSED` appelle un test qui le tue ; on relance ensuite `make cov` et `make mutants`.

- [ ] **Step 7 : commit**

```bash
git add crates/lf-domain/src/text/mod.rs crates/lf-domain/src/text/fold.rs crates/lf-domain/src/text/rank.rs
git commit -m "feat(domain): classement BM25 des passages"
```

---
---

## Task 16 : cibles de fuzz

**Files :**
- Create : `fuzz/Cargo.toml`, `fuzz/fuzz_targets/ids.rs`, `fuzz/fuzz_targets/text.rs`
- Modify : `.github/workflows/weekly.yml` (job `fuzz`)

**Interfaces :**
- Consumes : API publique de `lf-domain` (tâches 4 à 15).
- Produces : cibles `cargo +nightly fuzz run ids` et `cargo +nightly fuzz run text`.

`exclude = ["fuzz"]` figure déjà dans `Cargo.toml` (tâche 1) : le crate de fuzz reste hors du workspace, donc hors de `check-arch`, `deny` et `machete`.

- [ ] **Step 1 : crate de fuzz**

`fuzz/Cargo.toml` :

```toml
[package]
name = "lf-fuzz"
version = "0.0.0"
publish = false
edition = "2024"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4.13"
lf-domain = { path = "../crates/lf-domain" }

[[bin]]
name = "ids"
path = "fuzz_targets/ids.rs"
test = false
doc = false
bench = false

[[bin]]
name = "text"
path = "fuzz_targets/text.rs"
test = false
doc = false
bench = false
```

`fuzz/fuzz_targets/ids.rs` :

```rust
#![no_main]

use lf_domain::{
    date::CivilDate,
    ecli::Ecli,
    fond::Fond,
    id::{AnyId, ArticleId, DecisionId, LegiTextId},
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // La normalisation est idempotente.
    if let Ok(id) = AnyId::parse(data) {
        assert_eq!(AnyId::parse(id.as_str()), Ok(id.clone()));
    }
    let _ = LegiTextId::parse(data);
    let _ = ArticleId::parse(data);
    let _ = DecisionId::parse(data);
    let _ = Ecli::parse(data);
    let _ = Fond::parse(data);
    // Toute date acceptée se relit à l'identique.
    if let Ok(date) = CivilDate::parse(data) {
        assert_eq!(CivilDate::parse(&date.to_string()), Ok(date));
    }
});
```

`fuzz/fuzz_targets/text.rs` :

```rust
#![no_main]

use lf_domain::text::{clean, passages, rank, window};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (&str, &str, u16, u16)| {
    let (html, query, offset, max) = input;
    let max = usize::from(max).max(1);
    let text = clean(html);
    let w = window(&text, usize::from(offset), max);
    assert!(w.text.chars().count() <= max);
    // Les passages recouvrent exactement le texte nettoyé.
    let parts = passages(&text, max);
    let rebuilt: String = parts.iter().map(|p| p.text).collect();
    assert_eq!(rebuilt, text);
    let texts: Vec<&str> = parts.iter().map(|p| p.text).collect();
    for scored in rank(&texts, query, 5) {
        assert!(scored.score > 0.0 && scored.index < texts.len());
    }
});
```

- [ ] **Step 2 : courte campagne locale**

Run : `cd fuzz && cargo +nightly fuzz run ids -- -max_total_time=60 && cargo +nightly fuzz run text -- -max_total_time=60; cd ..`
Expected : deux lignes `Done … runs in 61 second(s)`, aucun crash. Essai à blanc : `ids` 2,1 M exécutions, `text` 944 k.

- [ ] **Step 3 : job hebdomadaire**

Ajouter à la fin de `.github/workflows/weekly.yml`, sous `jobs:` :

```yaml
  fuzz:
    name: fuzz ${{ matrix.target }}
    runs-on: ubuntu-latest
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        target: [ids, text]
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@nightly
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-fuzz
      - run: cargo +nightly fuzz run ${{ matrix.target }} -- -max_total_time=300
      - uses: actions/upload-artifact@v7
        if: failure()
        with:
          name: fuzz-${{ matrix.target }}
          path: fuzz/artifacts
```

Run : `uvx --from actionlint-py actionlint .github/workflows/weekly.yml`
Expected : aucune sortie.

- [ ] **Step 4 : commit**

```bash
git add fuzz .github/workflows/weekly.yml
git commit -m "test(domain): cibles cargo-fuzz ids et text"
```

---

## Task 17 : clôture du jalon M1

**Files :** aucun nouveau fichier. Corrections éventuelles issues des revues.

- [ ] **Step 1 : portes complètes**

Run : `make ci`
Expected : tout vert ; 95 tests ; `TOTAL` à 100.00 % (lignes, régions, fonctions).

- [ ] **Step 2 : mutation de tout ce qui a changé depuis `main`**

Run : `make mutants-diff BASE=main`
Expected : chaque fichier finit sans `MISSED`. Total de l'essai à blanc : 337 mutants générés, **0 survivant** (303 tués, 34 non compilables), sans aucune exclusion. Environ 5 minutes en `-j 1`.

- [ ] **Step 3 : revues, dans l'ordre de `~/.claude/rules/agent-coordination.md`**

Dispatcher, l'un après l'autre : `quality-checker`, `test-amplifier` (cherche des tests manquants malgré 100 %/0 survivant), `security-auditor` (parseurs d'entrées non fiables : `id`, `date`, `ecli`, `text::clean`), puis `code-reviewer` (hexagonal et idiomes Rust). Chaque correction suit la boucle TDD et repasse `make cov` et `make mutants FILE=…`.

- [ ] **Step 4 : mettre à jour la spec**

Dans `docs/plan/00-decisions.md`, marquer M0 et M1 comme livrés dans le tableau du §7 (colonne Contenu : `livré, plan 1`).

```bash
git add docs/plan/00-decisions.md
git commit -m "docs(plan): M0 et M1 livrés"
```

- [ ] **Step 5 : publication**

Demander à l'utilisateur s'il veut pousser `feat/m0-m1-domaine` et ouvrir la PR. La CI y rejoue les portes et mute le diff.
