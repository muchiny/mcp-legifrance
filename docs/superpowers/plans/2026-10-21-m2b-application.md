# M2b Application — plan d'implémentation

> **Origine.** Plan 2b (suite de la tête de M2), écrit le 2026-10-21 depuis l'essai à blanc de la branche locale `dryrun/m2b` (worktree jetable `/var/tmp/m2b-dry`, jamais poussée) : chaque tâche reprend les fichiers, les signatures, les valeurs et les tests d'un commit de l'essai, qui a passé `make ci` (259 tests, couverture 100 %), la MSRV 1.88 et `make mutants` sur chaque fichier touché (0 `MISSED`, 0 `TIMEOUT`). Les écarts entre l'essai et la spécification sont notés « Écart : … » dans la tâche concernée.
>
> **Découpage décidé le 2026-10-21.** La fin du plan 2a annonçait un plan 2b qui mêlait deux blocs indépendants. Le propriétaire a retenu deux plans : **2b** (ce plan) = bloc sans réseau, vérifiable en local : `SearchQuery` et ses types (T-M1-04, préalable du port), création de `lf-application` avec `LegalCorpus`, `CorpusError` (partie « port de corpus » de T-M3-01) et `Deadline` (T-M0-15, côté application), extensions de `check-arch` (R2 fermée, fermeture transitive, R9, 5e ; reste de T-M2-28). **2c** = naissance de `lf-adapter-piste` avec les spikes TLS, proxy, `backon` et `governor`, les licences de `cargo deny`, l'ADR-0013 et l'exclusion de couverture de la glue HTTP (T-M2-27, reste de T-M2-28), puis la suite de M2.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** donner au domaine sa requête de recherche validée, créer `lf-application` avec le port sortant `LegalCorpus` et l'échéance `Deadline`, et étendre `check-arch` aux règles qui naissent avec ce crate (R2 fermée et bornes de features, fermeture transitive, R9, `[lints] workspace = true`).

**Architecture :** `lf-domain` gagne un module `search` (objets de valeur purs, validés sans réseau). Un nouveau membre `crates/lf-application` dépend de `lf-domain`, `thiserror` et `tokio` (`rt`, `time`) ; il porte le trait `LegalCorpus` (RPITIT, futurs `+ Send`, dispatch statique, ADR-0012), l'enum `CorpusError` et `Deadline` (variable locale de tâche). `xtask` lit désormais le graphe résolu complet de `cargo metadata` : `arch.rs` garde les règles par membre, un nouveau `graph.rs` porte la fermeture transitive et R9 ; les manifestes sont lus par `toml`. Aucun adapter n'est créé.

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain 1.99.0), `thiserror` 2.0.21, `tokio` 1.53 (`rt`, `time` ; dev : `macros`, `rt`, `test-util`), `toml` 1.1 (`std`, `parse`, `serde`, xtask), `serde_json` 1 (xtask), `proptest` 1.12.0, `pretty_assertions` 1.4.1, `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-deny`, `cargo-machete`, `zizmor` 1.30.1, `actionlint` 1.7.12.

**Spec :** `docs/plan/03-domaine.md` §4.1 (`SearchQuery`), §3.1 (`Fond`), §6.1 (`DateRange`), §9 (erreurs) ; `docs/plan/05-outils-mcp.md` §7.3 (messages `invalid_arg`) ; `docs/plan/02-architecture.md` §3.2 (`lf-application`), §3.0 (versions, workspace), §4.1, §4.2 (règles R1 à R9, algorithme, étapes 4, 5, 7), §4.4 (pureté), §5.1 (`LegalCorpus`, `CorpusError`) ; `docs/plan/00-decisions.md` §2, §3 (délai global), §5, §7, §11.1 ; `docs/plan/04-adapter-piste.md` §5.3 (lecture du `Deadline`), §6.2, §6.5, §6.7, §6.8 ; `docs/plan/06-qualite.md` §2.1 (tests du domaine), §2.2, §9.3 (R9) ; `docs/plan/07-feuille-de-route.md` T-M1-04, T-M0-15, T-M3-01, T-M2-28 ; registre `M1-D10` de `docs/plan/09-risques-et-questions.md` §3.19 ; ADR-0012. Le plan 2a (`docs/superpowers/plans/2026-10-16-m2a-tete.md`) donne l'état de départ.

---

## Global Constraints

- Rust edition 2024, `rust-version = "1.88"` (job `msrv` : `cargo +1.88 check --workspace --all-targets --locked`), toolchain `1.99.0`. Chaque nouveau membre écrit `version.workspace`, `edition.workspace`, `rust-version.workspace`, `license.workspace`, `repository.workspace`, `publish.workspace` et `[lints] workspace = true`.
- Crates préfixés `lf-` ; identifiants Rust en anglais et ASCII ; docs et commentaires en français ; messages d'erreur en anglais, sans point final (style du livré : `toc_offset=300 is past the end (toc_total=88)`).
- `lf-domain` : dépendance normale unique `thiserror` ; aucune I/O, aucun async. `lf-application` : liste fermée de R2 (`thiserror`, `chrono` (`std`), `tracing`, `futures`, `tokio` (`time`, `macros`, `rt`), `similar`) en plus de `lf-domain` ; ce plan n'emploie que `lf-domain`, `thiserror` et `tokio` (`rt`, `time`). Une dépendance n'est déclarée que lorsqu'un code l'emploie (`cargo machete`).
- Dépendances : exigence au niveau mineur dans `[workspace.dependencies]` (`tokio = "1.53"`, `toml = "1.1"`), `default-features = false` déclaré au niveau du workspace ; exigence interne `=` + version de `[workspace.package]` (`lf-domain = { path = "crates/lf-domain", version = "=0.0.0" }`). Âge minimal : toute version ajoutée dans `Cargo.lock`, transitive comprise, est publiée depuis au moins 14 jours (`make min-age`) ; procédure de verrouillage en tâche 4.
- Lints : `clippy::pedantic`, `unwrap_used`, `expect_used`, `panic`, `unreachable`, `unimplemented`, `todo`, `dbg_macro` en `warn`, bloquants par `-D warnings` ; `unwrap`/`expect` permis dans les tests seulement. Aucun `#[allow]` ni `#[expect]` sans `reason`.
- Ports : pas de `async_trait`, pas de `dyn`, aucune méthode par défaut (ADR-0012, 02 §3.2).
- `--locked` sur toute commande cargo du `Makefile`, de la CI et de cargo-mutants.
- Qualité (00 §10) : TDD ; `make cov` = 100 % des lignes, régions et fonctions (`--lib`) ; `make mutants FILE=<fichier touché>` = 0 `MISSED` et 0 `TIMEOUT`, un fichier à la fois (`WARN No mutants found` vaut succès : fichier sans mutant possible). Aucune décision dans une fonction `new` ni dans un `impl Default` (cargo-mutants ne les mute jamais, `CONTRIBUTING.md`) : la logique va dans un helper. Toute boucle garantit sa progression, même mutée. Dans les tests : ni `assert!(matches!(…))`, ni `let … else { panic!() }`.
- WSL (`~/.claude/rules/wsl-safety.md`) : jamais `cargo test -j 4` ni plus ; `cargo mutants` uniquement par `make mutants` (un fichier, `-j 1`, `TMPDIR=/var/tmp`).
- `.github/` modifié ⇒ `make workflow-lint` (ou `uvx --from zizmor==1.30.1 zizmor --offline .github/` et `actionlint` 1.7.12).
- Commits : un par tâche, message en français au format conventionnel, sans trailer ajouté par l'implémenteur (le contrôleur ajoute le trailer `Co-Authored-By`). Rien n'est poussé.

## Review Focus

1. **Requête hostile ou bruitée** (contrôles seuls, espaces insécables, 501 caractères accentués, tabulations) : normalisée une fois, refusée par `EmptyQuery` ou `QueryTooLong` sans panique ; la longueur se compte en caractères, après normalisation. Tests : tâche 1, `blank_query_is_empty`, `query_too_long_is_invalid_arg`, `query_length_is_counted_after_normalization` et le proptest `parse_never_panics_and_output_is_normal`.
2. **Pagination profonde** (`page × page_size` = 10 000 pile, 10 100, `page = u32::MAX`) : la page à la limite passe, au-delà `PageBeyondReach`, sans débordement ; `has_more` vaut `false` quand la page suivante serait refusée. Tests : tâche 2, `page_beyond_reachable_window_is_rejected`, `has_more_false_at_reachable_window_end`, proptest `accepted_pages_stay_within_reach`.
3. **Échéance hors de portée** (tâche lancée par `tokio::spawn`, appel direct de l'adapter, échéance déjà passée) : la valeur par défaut s'applique hors portée, zéro une fois l'échéance passée, jamais de panique. Tests : tâche 4, `spawned_task_does_not_see_the_deadline`, `outside_any_scope_the_default_applies`, `past_deadline_leaves_nothing`.
4. **Graphe de dépendances piégé** (cycle, arête de dev vers un crate interdit, arête de build, arête propre à une plateforme, deux chemins vers le même crate) : la fermeture termine, ignore le dev, compte le build et les arêtes de plateforme, signale chaque crate interdit une fois avec le plus court chemin. Tests : tâche 6, `each_forbidden_crate_is_reported_once_by_its_shortest_path`, `reads_production_edges_of_the_resolve_graph`.
5. **Manifeste écrit autrement** (`lints.workspace = true`, table en ligne, `workspace = "true"` chaîne, TOML invalide) : les trois formes équivalentes passent, la chaîne et l'absence échouent, un TOML invalide est une erreur. Tests : tâche 5, `reads_workspace_lints_in_every_toml_form`.

---

## Structure des fichiers

```
crates/lf-domain/src/error.rs        + EmptyQuery, QueryTooLong, OutOfRange (1) ; VersionDateNotApplicable,
                                       DateFilterNotApplicable, PageBeyondReach (2)
crates/lf-domain/src/search.rs       nouveau : QueryText, PageSize, MatchMode (1) ; SearchQuery, has_more,
                                       REACHABLE_RESULTS (2)
crates/lf-domain/src/fond.rs         + needs_version_date, has_date_filter (2)
crates/lf-domain/src/lib.rs          + pub mod search (1)
Cargo.toml                           membre lf-application, lf-domain en [workspace.dependencies] (3) ;
                                       tokio (4) ; toml (5)
Cargo.lock                           tokio et ses dépendances (4) ; toml et ses dépendances (5)
crates/lf-application/Cargo.toml     nouveau (3) ; tokio (4)
crates/lf-application/src/lib.rs     nouveau (3) ; + pub mod deadline (4)
crates/lf-application/src/ports/mod.rs      nouveau (3)
crates/lf-application/src/ports/corpus.rs   nouveau : LegalCorpus, CorpusError (3)
crates/lf-application/src/deadline.rs       nouveau : Deadline (4)
Makefile, .github/workflows/ci.yml   cible et étape `purity` (4)
xtask/Cargo.toml                     + toml (5)
xtask/src/arch.rs                    R2 fermée et features, 5e, Package.features/manifest_path/lints_workspace (5)
xtask/src/graph.rs                   nouveau : Resolved, resolved_graph, graph_violations (6)
xtask/src/lib.rs                     + pub mod graph (6)
xtask/src/main.rs                    lecture des manifestes (5) ; métadonnées complètes, graphe (6)
docs/plan/00, 02, 06, 07, 09, CHANGELOG.md   état livré, renvois au plan 2c (7)
```

Ordre : 1 → 7, linéaire. Les tâches 1-2 (domaine) et 5-6 (xtask) ne dépendent pas l'une de l'autre, mais la tâche 5 suppose `lf-application` présent (tâche 3) et ses features `tokio` réelles (tâche 4) pour que `make arch` reste vert.

---

### Task 1 : `QueryText`, `PageSize`, `MatchMode` et leurs erreurs

**Files:**
- Create: `crates/lf-domain/src/search.rs`
- Modify: `crates/lf-domain/src/lib.rs` (ajouter `pub mod search;` après `pub mod id;`)
- Modify: `crates/lf-domain/src/error.rs` (variantes après `InvertedDateRange` ; tests `messages_are_actionable`, `echoed`, `variants_without_input_echo_nothing`)

**Interfaces:**
- Consumes : `DomainError` et son style (variantes sans `input` ⇒ pas de `#[non_exhaustive]`).
- Produces :
  - `pub const QUERY_MAX_CHARS: usize = 500;`
  - `pub struct QueryText(String)` (`Debug, Clone, PartialEq, Eq, Hash`) ; `pub fn parse(input: &str) -> Result<QueryText, DomainError>` ; `pub fn as_str(&self) -> &str`.
  - `pub struct PageSize(u32)` (`Debug, Clone, Copy, PartialEq, Eq, Hash`) ; `pub const MIN: u32 = 1`, `pub const MAX: u32 = 100`, `pub const DEFAULT: PageSize` (10) ; `pub fn new(value: u32) -> Result<PageSize, DomainError>` ; `pub fn get(self) -> u32` ; `impl Default` (rend `DEFAULT`).
  - `pub enum MatchMode { #[default] Any, All, Exact }` (`Debug, Clone, Copy, PartialEq, Eq, Hash, Default`).
  - `DomainError::EmptyQuery` (`"query is required"`), `DomainError::QueryTooLong { len: usize, max: usize }` (`"query is too long ({len} characters, max {max})"`), `DomainError::OutOfRange { field: &'static str, value: i64, min: i64, max: i64 }` (`"{field}={value} out of range ({min}..{max})"`).

- [ ] **Step 1 : tests d'erreurs.** Dans `error.rs`, `messages_are_actionable` gagne :

```rust
assert_eq!(DomainError::EmptyQuery.to_string(), "query is required");
assert_eq!(
    DomainError::QueryTooLong { len: 612, max: 500 }.to_string(),
    "query is too long (612 characters, max 500)"
);
assert_eq!(
    DomainError::OutOfRange { field: "page_size", value: 0, min: 1, max: 100 }.to_string(),
    "page_size=0 out of range (1..100)"
);
```

  et `variants_without_input_echo_nothing` liste `EmptyQuery`, `QueryTooLong { len: 501, max: 500 }` et `OutOfRange { field: "page_size", value: 0, min: 1, max: 100 }` ; le `match` de `echoed` range les trois variantes dans le bras `=> ""`.

- [ ] **Step 2 : tests du module** (`search.rs`, `#[cfg(test)] mod tests`, `pretty_assertions::assert_eq`, `proptest::prelude::*`), avec les helpers `text(s) = QueryText::parse(s).unwrap()` et `size(n) = PageSize::new(n).unwrap()` :

```rust
#[test]
fn query_is_trimmed_and_folded() {
    assert_eq!(text("  droit \t\r\n de\u{a0}\u{a0}grève  ").as_str(), "droit de grève");
}

#[test]
fn query_control_chars_are_stripped() {
    assert_eq!(text("a\u{0}b \u{1} c\u{7f}\u{9b}").as_str(), "ab c");
}

#[test]
fn blank_query_is_empty() {
    for blank in ["", "   ", "\t\n", "\u{0}\u{1}", " \u{7} \u{1b} "] {
        assert_eq!(QueryText::parse(blank), Err(DomainError::EmptyQuery), "{blank:?}");
    }
}

#[test]
fn query_too_long_is_invalid_arg() {
    let at_max = "é".repeat(QUERY_MAX_CHARS);
    assert_eq!(text(&at_max).as_str(), at_max);
    assert_eq!(
        QueryText::parse(&"é".repeat(QUERY_MAX_CHARS + 1)),
        Err(DomainError::QueryTooLong { len: 501, max: 500 })
    );
}

#[test]
fn query_length_is_counted_after_normalization() {
    let padded = format!("  {}  \u{0}", "a".repeat(QUERY_MAX_CHARS));
    assert_eq!(text(&padded).as_str().len(), QUERY_MAX_CHARS);
}

#[test]
fn page_size_bounds() {
    let out = |value| DomainError::OutOfRange { field: "page_size", value, min: 1, max: 100 };
    assert_eq!(PageSize::new(0), Err(out(0)));
    assert_eq!(PageSize::new(1).map(PageSize::get), Ok(1));
    assert_eq!(PageSize::new(100).map(PageSize::get), Ok(100));
    assert_eq!(PageSize::new(101), Err(out(101)));
}

#[test]
fn defaults_match_reference() {
    assert_eq!(PageSize::default().get(), 10);
    assert_eq!(MatchMode::default(), MatchMode::Any);
}

proptest! {
    #[test]
    fn parse_never_panics_and_output_is_normal(s in "\\PC*|[\\x00-\\x20a ]*") {
        if let Ok(q) = QueryText::parse(&s) {
            let t = q.as_str();
            prop_assert!(!t.chars().any(char::is_control));
            prop_assert!(!t.contains("  ") && t.trim() == t && !t.is_empty());
            prop_assert!(t.chars().count() <= QUERY_MAX_CHARS);
            prop_assert_eq!(QueryText::parse(t), Ok(q.clone()));
        }
    }
}
```

- [ ] **Step 3 : lancer** `cargo nextest run -p lf-domain --locked -E 'test(/search|error/)'` — attendu : échec de compilation (module et variantes absents).

- [ ] **Step 4 : implémenter.** Variantes de `DomainError` documentées champ par champ (`missing_docs`). `QueryText::parse` : normalise, puis `0` caractère ⇒ `EmptyQuery`, plus de `QUERY_MAX_CHARS` ⇒ `QueryTooLong { len, max: QUERY_MAX_CHARS }`. La normalisation (fonction privée `normalize`) découpe sur les blancs Unicode (`split_whitespace`, qui couvre U+00A0), retire de chaque mot les caractères `char::is_control`, écarte les mots devenus vides, joint par une espace :

```rust
input
    .split_whitespace()
    .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
    .filter(|word| !word.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
```

  `PageSize::new` délègue à un helper privé `checked(value)` (contrôle `(MIN..=MAX).contains`, sinon `OutOfRange { field: "page_size", value: i64::from(value), min: i64::from(MIN), max: i64::from(MAX) }`) ; `impl Default` rend `Self::DEFAULT`. Doc de module : objets de valeur de `fr_search` (03 §4.1, 00 §7), validés sans réseau ; doc-tests sur `QueryText::parse` (normalisation, refus d'un blanc). Constante et doc citent `[V-81 · longueur-max-query]` pour la borne de 500.

- [ ] **Step 5 : vérifier** `cargo nextest run -p lf-domain --locked`, `make lint`, `make cov` (100 %), puis `make mutants FILE=crates/lf-domain/src/search.rs` et `make mutants FILE=crates/lf-domain/src/error.rs` — attendu : 0 `MISSED`, 0 `TIMEOUT`.

- [ ] **Step 6 : commit** `feat(domain): QueryText, PageSize et MatchMode de la requête de recherche`.

Écart : 03 §9 n'a pas de variante pour une requête vide (« un argument absent est détecté par `lf-adapter-mcp` ») ; une requête faite de blancs ou de contrôles n'est pas absente mais vide après normalisation, d'où `EmptyQuery` (message de parité 05 §7.3, sans point). `page` reste un `NonZeroU32` (tâche 2) : `page = 0` est refusé au décodage des arguments (M4), pas par le domaine.

---

### Task 2 : `SearchQuery`, `has_more` et les prédicats de `Fond`

**Files:**
- Modify: `crates/lf-domain/src/search.rs`
- Modify: `crates/lf-domain/src/fond.rs` (après `api_name`, test après `api_names_match_reference`)
- Modify: `crates/lf-domain/src/error.rs` (importer `fond::Fond` : `use crate::{date::VersionDate, fond::Fond};`)

**Interfaces:**
- Consumes : `QueryText`, `PageSize`, `MatchMode` (tâche 1) ; `DateRange`, `VersionDate` (`date.rs`, livrés).
- Produces :
  - `impl Fond { pub const fn needs_version_date(self) -> bool /* Loda | Code */; pub const fn has_date_filter(self) -> bool /* tous sauf Code */ }`.
  - `pub const REACHABLE_RESULTS: u64 = 10_000;`
  - `pub struct SearchQuery { /* champs privés */ fond: Fond, text: QueryText, page: NonZeroU32, page_size: PageSize, mode: MatchMode, date_range: Option<DateRange>, version_date: Option<VersionDate> }` (`Debug, Clone, PartialEq, Eq, Hash`).
  - `pub fn new(fond: Fond, text: QueryText, page: NonZeroU32, page_size: PageSize, mode: MatchMode, date_range: Option<DateRange>, version_date: Option<VersionDate>) -> Result<SearchQuery, DomainError>` ; accesseurs `fond(&self) -> Fond`, `text(&self) -> &QueryText`, `page(&self) -> NonZeroU32`, `page_size(&self) -> PageSize`, `mode(&self) -> MatchMode`, `date_range(&self) -> Option<DateRange>`, `version_date(&self) -> Option<VersionDate>`.
  - `pub fn has_more(page: NonZeroU32, page_size: PageSize, total: u64) -> bool`.
  - `DomainError::VersionDateNotApplicable { fond: Fond }` (`"version_date only applies to fond LODA or CODE, not {fond}"`), `DomainError::DateFilterNotApplicable { fond: Fond }` (`"date_from/date_to are not supported for fond={fond}"`), `DomainError::PageBeyondReach { page: u32, page_size: u32, max: u64 }` (`"page={page} with page_size={page_size} goes beyond the {max} reachable results"`).

- [ ] **Step 1 : tests de `Fond`** (`fond.rs`) :

```rust
#[test]
fn version_date_and_date_filter_by_fond() {
    let table: Vec<_> = Fond::ALL
        .iter()
        .map(|f| (f.name(), f.needs_version_date(), f.has_date_filter()))
        .collect();
    assert_eq!(
        table,
        [
            ("LODA", true, true),
            ("CODE", true, false),
            ("JURI", false, true),
            ("CONSTIT", false, true),
            ("CETAT", false, true),
            ("CNIL", false, true),
            ("KALI", false, true),
            ("ACCO", false, true),
        ]
    );
}
```

- [ ] **Step 2 : tests d'erreurs** (`messages_are_actionable`, `variants_without_input_echo_nothing`, bras `""` de `echoed`) :

```rust
assert_eq!(
    DomainError::VersionDateNotApplicable { fond: Fond::Juri }.to_string(),
    "version_date only applies to fond LODA or CODE, not JURI"
);
assert_eq!(
    DomainError::DateFilterNotApplicable { fond: Fond::Code }.to_string(),
    "date_from/date_to are not supported for fond=CODE"
);
assert_eq!(
    DomainError::PageBeyondReach { page: 200, page_size: 100, max: 10_000 }.to_string(),
    "page=200 with page_size=100 goes beyond the 10000 reachable results"
);
```

- [ ] **Step 3 : tests de `SearchQuery` et `has_more`** (`search.rs`), helpers `page(n) = NonZeroU32::new(n).unwrap()`, `date(s) = VersionDate::parse(s).unwrap()`, `range() = DateRange::new(Some(date("2020-01-01")), None).unwrap()`, et `query(fond, page_n, size_n, date_range, version_date)` qui appelle `SearchQuery::new(fond, text("bail"), page(page_n), size(size_n), MatchMode::default(), date_range, version_date)` :

```rust
#[test]
fn version_date_outside_loda_code_is_rejected() {
    let v = Some(date("2024-01-01"));
    for fond in Fond::ALL {
        let got = query(fond, 1, 10, None, v);
        if fond.needs_version_date() {
            assert_eq!(got.map(|q| q.version_date()), Ok(v), "{fond}");
        } else {
            assert_eq!(got, Err(DomainError::VersionDateNotApplicable { fond }), "{fond}");
        }
    }
}

#[test]
fn date_range_on_code_is_rejected() {
    for fond in Fond::ALL {
        let got = query(fond, 1, 10, range(), None);
        if fond == Fond::Code {
            assert_eq!(got, Err(DomainError::DateFilterNotApplicable { fond }));
        } else {
            assert_eq!(got.map(|q| q.date_range()), Ok(range()), "{fond}");
        }
    }
}

#[test]
fn version_date_is_checked_before_date_range() {
    assert_eq!(
        query(Fond::Juri, 1, 10, range(), Some(date("2024-01-01"))),
        Err(DomainError::VersionDateNotApplicable { fond: Fond::Juri })
    );
}

#[test]
fn page_beyond_reachable_window_is_rejected() {
    assert_eq!(query(Fond::Juri, 100, 100, None, None).map(|q| q.page()), Ok(page(100)));
    assert_eq!(
        query(Fond::Juri, 101, 100, None, None),
        Err(DomainError::PageBeyondReach { page: 101, page_size: 100, max: 10_000 })
    );
    assert_eq!(
        query(Fond::Juri, u32::MAX, 100, None, None),
        Err(DomainError::PageBeyondReach { page: u32::MAX, page_size: 100, max: 10_000 })
    );
}

#[test]
fn accessors_return_the_validated_fields() {
    let q = SearchQuery::new(Fond::Loda, text(" bail "), page(3), size(25), MatchMode::Exact,
        range(), Some(date("2024-01-01"))).unwrap();
    assert_eq!(
        (q.fond(), q.text().as_str(), q.page(), q.page_size(), q.mode()),
        (Fond::Loda, "bail", page(3), size(25), MatchMode::Exact)
    );
    assert_eq!((q.date_range(), q.version_date()), (range(), Some(date("2024-01-01"))));
}

#[test]
fn normalized_queries_share_a_cache_key() {
    let a = query(Fond::Juri, 1, 10, None, None).unwrap();
    let b = SearchQuery::new(Fond::Juri, text("  bail\t"), page(1), PageSize::default(),
        MatchMode::Any, None, None).unwrap();
    assert_eq!(HashSet::from([a.clone(), b]).len(), 1);
    assert_ne!(a, query(Fond::Juri, 2, 10, None, None).unwrap());
}

#[test]
fn has_more_false_at_reachable_window_end() {
    assert!(has_more(page(1), size(10), 11));
    assert!(!has_more(page(1), size(10), 10));
    assert!(has_more(page(99), size(100), 20_000));
    assert!(!has_more(page(100), size(100), 20_000));
    assert!(has_more(page(332), size(30), 50_000));
    assert!(!has_more(page(333), size(30), 50_000));
    assert!(!has_more(page(u32::MAX), size(100), u64::MAX));
}

proptest! {
    #[test]
    fn accepted_pages_stay_within_reach(p in 1u32..=10_000, n in 1u32..=100) {
        let reachable = u64::from(p) * u64::from(n) <= REACHABLE_RESULTS;
        prop_assert_eq!(query(Fond::Juri, p, n, None, None).is_ok(), reachable);
    }
}
```

- [ ] **Step 4 : lancer** `cargo nextest run -p lf-domain --locked -E 'test(/search|fond|error/)'` — attendu : échec de compilation.

- [ ] **Step 5 : implémenter.** `SearchQuery::new` construit la valeur et délègue à un helper privé `checked(self) -> Result<Self, DomainError>`, qui applique dans cet ordre : `version_date.is_some() && !fond.needs_version_date()` ⇒ `VersionDateNotApplicable` ; `date_range.is_some() && !fond.has_date_filter()` ⇒ `DateFilterNotApplicable` ; `!within_reach(page, page_size)` ⇒ `PageBeyondReach { page, page_size, max: REACHABLE_RESULTS }`. La plage inversée est déjà refusée par `DateRange::new`. Un seul calcul de fenêtre, partagé par le rejet et `has_more`, en `u64` (deux `u32` multipliés ne débordent pas ; `page + 1` non plus) :

```rust
pub fn has_more(page: NonZeroU32, page_size: PageSize, total: u64) -> bool {
    let (page, size) = (u64::from(page.get()), u64::from(page_size.get()));
    page * size < total && within_reach(page + 1, size)
}

fn within_reach(page: u64, page_size: u64) -> bool {
    page * page_size <= REACHABLE_RESULTS
}
```

  Doc de `REACHABLE_RESULTS` : fenêtre supposée, `[V-83 · pagination-profonde]`. Doc de `has_more` avec doc-test (`page(1)`, `PageSize::DEFAULT`, `total` 11 ⇒ vrai ; page 333 × 30 sur 50 000 ⇒ faux). Doc de `SearchQuery` : `Eq + Hash`, clé de cache (00 §9), sans `typePagination` (04 §6.6) ; invariants.

- [ ] **Step 6 : vérifier** `make lint`, `make test`, `make cov`, puis `make mutants FILE=…` sur `search.rs`, `fond.rs` et `error.rs`, l'un après l'autre — attendu : 0 `MISSED`, 0 `TIMEOUT` (essai : 46 mutants sur `search.rs`, 40 tués, 6 non viables).

- [ ] **Step 7 : commit** `feat(domain): SearchQuery, has_more et prédicats de fond (T-M1-04)`.

Écarts : 02 §5.1 et 03 §4.1 montrent des champs publics ; ils sont privés avec accesseurs, pour que les invariants de `new` tiennent (même choix que `DateRange`). `PaginationMode` (T-M1-04) n'entre pas : son consommateur est le `PaginationMemo` de l'adapter (plan 2c), et chaque objet de valeur entre au jalon qui le consomme (00 §11.1). `has_more` est une fonction libre : `SearchPage` n'a ni `page` ni `page_size` avant M5 ; le mapper de 2c l'appellera.

---

### Task 3 : crate `lf-application`, port `LegalCorpus` et `CorpusError`

**Files:**
- Modify: `Cargo.toml` (`members = ["crates/lf-domain", "crates/lf-application", "xtask"]` ; en tête de `[workspace.dependencies]` : commentaire « internes : exigence « = » + version de [workspace.package] (check-arch, étape 5h) », `lf-domain = { path = "crates/lf-domain", version = "=0.0.0" }`, puis commentaire « externes »)
- Create: `crates/lf-application/Cargo.toml`, `src/lib.rs`, `src/ports/mod.rs`, `src/ports/corpus.rs`

**Interfaces:**
- Consumes : `lf_domain::{date::VersionDate, entity::{Act, ArticleText, CompanyAgreement, Convention, Decision, Deliberation, SearchPage}, id::{AccoTextId, ArticleId, CnilTextId, DecisionId, KaliTextId, LegiTextId}, search::SearchQuery}`.
- Produces :
  - `pub trait LegalCorpus: Send + Sync + 'static` à sept méthodes, chacune `-> impl Future<Output = Result<T, CorpusError>> + Send`, sans corps par défaut : `search(&self, query: &SearchQuery) -> SearchPage` ; `get_act(&self, id: &LegiTextId, date: VersionDate) -> Act` ; `get_article(&self, id: &ArticleId) -> ArticleText` ; `get_decision(&self, id: &DecisionId) -> Decision` ; `get_deliberation(&self, id: &CnilTextId) -> Deliberation` ; `get_convention(&self, id: &KaliTextId) -> Convention` ; `get_company_agreement(&self, id: &AccoTextId) -> CompanyAgreement`.
  - `#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)] pub enum CorpusError` : `NotFound { what: String }`, `InvalidRequest { status: Option<u16>, detail: String }`, `Upstream { status: Option<u16>, retryable: bool, detail: String }`, `Timeout`, `RateLimited { retry_after: Option<Duration> }`, `Auth { detail: String }`, `Config { detail: String }`, `Decode { detail: String }` — messages et commentaires de 02 §5.1.
  - Ré-export `pub use corpus::{CorpusError, LegalCorpus};` dans `ports`.

- [ ] **Step 1 : manifeste** `crates/lf-application/Cargo.toml` : `name = "lf-application"`, `description = "Couche application du MCP Légifrance : ports, cas d'usage, échéance des appels."`, les six champs `*.workspace = true` de `lf-domain`, `[dependencies] lf-domain.workspace = true`, `thiserror.workspace = true`, `[dev-dependencies] pretty_assertions.workspace = true`, `[lints] workspace = true`.

- [ ] **Step 2 : tests** (`ports/corpus.rs`). Un faux corpus `Named` implémente le port par des `async fn` qui rendent `Err(CorpusError::NotFound { what: "<méthode>".into() })` ; chaque corps attend un futur prêt (`std::future::ready(Err(named("search"))).await`), ce qui satisfait `clippy::unused_async` sans `#[allow]`. Un helper sonde le futur une fois, en exigeant `Send` :

```rust
fn poll_once<F: Future + Send>(future: F) -> Poll<F::Output> {
    pin!(future).poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn port_is_implemented_by_async_fns_with_send_futures() {
    let errors = poll_once(every_method(&Named));
    let names = ["search", "get_act", "get_article", "get_decision", "get_deliberation",
        "get_convention", "get_company_agreement"];
    assert_eq!(errors, Poll::Ready(names.map(named).to_vec()));
}
```

  `every_method<C: LegalCorpus>(corpus: &C) -> Vec<CorpusError>` (async, générique : dispatch statique) appelle les sept méthodes dans l'ordre avec `SearchQuery::new(Fond::Juri, QueryText::parse("bail").unwrap(), NonZeroU32::MIN, PageSize::default(), MatchMode::default(), None, None).unwrap()`, `LEGITEXT000006070721` + `VersionDate::MIN`, `LEGIARTI000006419288`, `JURITEXT000037999394`, `CNILTEXT000053352643`, `KALITEXT000005652329`, `ACCOTEXT000037731479` (`.parse().unwrap()`), et collecte les `unwrap_err()`. Le second test fixe les messages :

```rust
let cases = [
    (CorpusError::NotFound { what: "JURITEXT000037999394".into() }, "not found: JURITEXT000037999394"),
    (CorpusError::InvalidRequest { status: Some(400), detail: "x".into() }, "request rejected by Legifrance"),
    (CorpusError::Upstream { status: None, retryable: true, detail: "x".into() }, "upstream error"),
    (CorpusError::Timeout, "upstream timeout"),
    (CorpusError::RateLimited { retry_after: Some(Duration::from_secs(12)) }, "rate limited"),
    (CorpusError::Auth { detail: "x".into() }, "credentials rejected"),
    (CorpusError::Config { detail: "credentials missing".into() }, "configuration error: credentials missing"),
    (CorpusError::Decode { detail: "invalid UTF-8 at byte 3".into() }, "undecodable upstream response: invalid UTF-8 at byte 3"),
];
for (error, message) in cases {
    assert_eq!(error.to_string(), message);
}
```

  (test `messages_carry_no_secret_and_name_the_failure`). Le helper `poll_once` rend un `Poll` et non un `Option` : aucune branche `Pending` non couverte.

- [ ] **Step 3 : lancer** `cargo nextest run -p lf-application --locked` — attendu : échec de compilation, puis implémenter `lib.rs` (doc : 02 §3.2, liste fermée R2 vérifiée par `check-arch`, ce que le plan 2b livre et ce qui arrive en M3 ; `pub mod ports;`), `ports/mod.rs` (doc : ports sortants RPITIT, ADR-0012) et `ports/corpus.rs` (doc de module et de trait : aucune méthode par défaut, futurs `+ Send` à cause de rmcp, 02 §3.2 pièges 1 et 2 ; méthodes M6 et M7 annoncées dans la doc, non déclarées).

- [ ] **Step 4 : vérifier** `make lint test doc arch machete cov` — attendu : verts, `check-arch: ok` (la ligne `lf-application` de `RULES` existe déjà), `lf-application` à 100 %. `make mutants FILE=crates/lf-application/src/ports/corpus.rs` — attendu : `WARN No mutants found` (trait et dérivations seulement).

- [ ] **Step 5 : commit** `feat(application): crate lf-application, port LegalCorpus et CorpusError`.

Écart : aucun ré-export de types du domaine (02 §3.2, « `pub use lf_domain::PisteEnvironment` ») : `PisteEnvironment` n'existe pas encore, et le seul consommateur des ré-exports, `lf-adapter-audit`, naît en M3. Les doublures `FakeCorpus` et la feature `test-support` restent en M3 (T-M3-01).

---

### Task 4 : `Deadline` en variable locale de tâche, `tokio` et contrôle de pureté

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`, section « externes »), `Cargo.lock`
- Modify: `crates/lf-application/Cargo.toml`, `crates/lf-application/src/lib.rs` (`pub mod deadline;` avant `pub mod ports;`)
- Create: `crates/lf-application/src/deadline.rs`
- Modify: `Makefile` (cible `purity`, ajoutée à `.PHONY` et à `ci` après `arch`), `.github/workflows/ci.yml` (job `check`)

**Interfaces:**
- Produces :
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Deadline(tokio::time::Instant);`
  - `pub fn after(budget: Duration) -> Deadline` ; `pub fn remaining(self) -> Duration` (zéro une fois passée) ; `pub async fn scope<F: Future>(self, future: F) -> F::Output` ; `pub fn current() -> Option<Deadline>` ; `pub fn current_or(default: Duration) -> Duration`.
  - Consommateurs : `Audited` (M3) publie par `Deadline::after(LF_TOOL_DEADLINE_S).scope(…)` ; l'adapter PISTE (plan 2c) lit `Deadline::current_or(tool_deadline)` à l'entrée de chaque méthode du port.

- [ ] **Step 1 : dépendances.** `[workspace.dependencies]` : `tokio = { version = "1.53", default-features = false }`, précédé du commentaire « externes : exigence au niveau mineur, correctif choisi par le verrou (règle d'âge, 02 §3.0.1) ; default-features = false déclaré ici : au niveau d'un membre, l'héritage ne le respecte qu'à partir de 1.99 ». `lf-application` : `tokio = { workspace = true, features = ["rt", "time"] }` avec le commentaire « rt : pour task_local! seulement (Deadline) ; time : Instant (R2) » ; dev : `tokio = { workspace = true, features = ["macros", "rt", "test-util"] }`.

- [ ] **Step 2 : verrou** (règle d'âge, 00 §2). Laissé à lui-même, cargo prend la plus haute version compatible (essai du 2026-10-21 : `tokio` 1.54.0, publiée le 2026-10-19). Fixer la plus haute `1.53.x` publiée depuis au moins 14 jours, puis contrôler toute dépendance ajoutée :

```bash
curl -s https://index.crates.io/to/ki/tokio | python3 -c "import sys, json; [print(d['vers'], d['pubtime'][:10]) for d in map(json.loads, sys.stdin) if d['vers'].startswith('1.53.')]"
cargo update -p tokio --precise <plus haute 1.53.x publiée depuis ≥ 14 jours>
make min-age
```

  Attendu de `make min-age` : `check-min-age : tous les paquets ont au moins 14 j`. Un paquet transitif trop récent se redescend de même (`cargo update -p <paquet> --precise <version>`). Essai : `tokio` 1.53.2 (2026-10-03), `tokio-macros` 2.7.2, `pin-project-lite` 0.2.17 ; 1.53.3 (2026-10-13) devient éligible le 2026-10-27.

- [ ] **Step 3 : tests** (`deadline.rs`, `#[tokio::test(start_paused = true)]`, `const DEFAULT: Duration = Duration::from_secs(60)`, `secs(n)`) :

```rust
async fn published_deadline_reads_as_remaining_time() {
    let deadline = Deadline::after(secs(10));
    let seen = deadline
        .scope(async {
            let first = Deadline::current_or(DEFAULT);
            advance(secs(3)).await;
            (first, Deadline::current_or(DEFAULT), Deadline::current())
        })
        .await;
    assert_eq!(seen, (secs(10), secs(7), Some(deadline)));
}

async fn outside_any_scope_the_default_applies() {
    assert_eq!((Deadline::current(), Deadline::current_or(DEFAULT)), (None, DEFAULT));
}

async fn past_deadline_leaves_nothing() {
    let deadline = Deadline::after(secs(1));
    advance(secs(2)).await;
    assert_eq!(deadline.remaining(), Duration::ZERO);
    assert_eq!(deadline.scope(async { Deadline::current_or(DEFAULT) }).await, Duration::ZERO);
}

async fn joined_sub_futures_share_the_deadline() {
    let read = || async { Deadline::current_or(DEFAULT) };
    let both = Deadline::after(secs(5)).scope(async { tokio::join!(read(), read()) }).await;
    assert_eq!(both, (secs(5), secs(5)));
}

async fn spawned_task_does_not_see_the_deadline() {
    let spawned = Deadline::after(secs(5))
        .scope(async { tokio::spawn(async { Deadline::current_or(DEFAULT) }).await })
        .await;
    assert_eq!(spawned.unwrap(), DEFAULT);
}
```

- [ ] **Step 4 : implémenter.** `tokio::task_local! { static CURRENT: Deadline; }` ; `after` = `Self(Instant::now() + budget)` (budget ≤ 600 s, `LF_TOOL_DEADLINE_S` : pas de débordement) ; `remaining` = `saturating_duration_since(Instant::now())` ; `scope` = `CURRENT.scope(self, future).await` ; `current` = `CURRENT.try_with(|d| *d).ok()` ; `current_or` = `Self::current().map_or(default, Self::remaining)`. Doc de module : garde dure dans `Audited` (M3), lecture comme durée restante convertie dans l'horloge de l'adapter (04 §5.3), visibilité limitée à la tâche (02 §3.2, piège 5), `rt` pour `task_local!` seulement (R2).

- [ ] **Step 5 : pureté (02 §4.4).** `Makefile` :

```make
# Pureté de lf-application (02 §4.4) : compilé seul, ses features sont résolues pour son
# seul graphe (tokio time/macros/rt, chrono sans clock) ; une feature qu'un autre membre
# active ne masque plus un manque.
purity:
	cargo check -p lf-application --locked
```

  `ci: fmt-check lint test doc arch purity deny min-age-selftest machete cov`. `ci.yml`, job `check` : `name: fmt, clippy, doc, arch, purity, machete` et `run: make fmt-check lint doc arch purity machete`. Puis `make workflow-lint` (zizmor et actionlint propres).

- [ ] **Step 6 : vérifier** `make ci` — attendu : vert ; `cargo +1.88 check --workspace --all-targets --locked` — attendu : vert ; `make mutants FILE=crates/lf-application/src/deadline.rs` — attendu : 0 `MISSED`, 0 `TIMEOUT` (essai : 8 mutants, 4 tués, 4 non viables).

- [ ] **Step 7 : commit** `feat(application): Deadline en variable locale de tâche, tokio et contrôle de pureté (T-M0-15)`.

Écart : 02 nomme `Deadline` et `Deadline::current_or` sans fixer le reste ; le type est un newtype sur `tokio::time::Instant` (temps suspendu en test) avec `after`, `remaining`, `scope` et `current`. Il ne pose pas de délai : la garde dure (`tokio::time::timeout`) reste à `Audited` (M3). La lecture depuis l'adapter (`CallDeadline`, test `start_paused` de T-M0-15) se fait au plan 2c.

---

### Task 5 : `check-arch`, liste fermée et features de R2, `[lints] workspace = true` (5e)

**Files:**
- Modify: `Cargo.toml` (`toml = { version = "1.1", default-features = false, features = ["std", "parse", "serde"] }`), `Cargo.lock`, `xtask/Cargo.toml` (`toml.workspace = true`)
- Modify: `xtask/src/arch.rs`, `xtask/src/main.rs`

**Interfaces:**
- Produces :
  - `Package` gagne `pub features: Vec<(String, Vec<String>)>` (features activées de chaque dépendance normale, plus `"default"` sauf si `uses_default_features` vaut `false` ; absent ⇒ `default`, échec fermé), `pub manifest_path: String` (obligatoire : sinon `Err("cargo metadata: {name} has no manifest_path")`) et `pub lints_workspace: bool` (`false` à la lecture : 5e en violation tant que le manifeste n'a pas été lu).
  - `pub fn inherits_workspace_lints(manifest: &str) -> Result<bool, String>` : `manifest.parse::<toml::Table>()`, puis `table.get("lints").and_then(|l| l.get("workspace")) == Some(&toml::Value::Boolean(true))` ; erreur `format!("Cargo.toml: {e}")`.
  - Messages : `"{membre} must not depend on {dep}"` (liste fermée), `"{membre} must not enable {dep} feature {f}"`, `"{membre} must declare [lints] workspace = true"`.

- [ ] **Step 1 : dépendance.** Ajouter `toml` (ligne ci-dessus) et la verrouiller comme en tâche 4, étape 2 : plus haute `1.1.x` publiée depuis au moins 14 jours (essai : `1.1.6+spec-1.1.0`, du 2026-09-10 ; `cargo update -p toml --precise 1.1.6+spec-1.1.0`), puis `make min-age` ; redescendre chaque transitif trop récent (essai : `serde_spanned` 1.1.2 et `toml_writer` 1.1.3+spec-1.1.0, publiés le 2026-10-08, redescendus en 1.1.1 et 1.1.2+spec-1.1.0 ; éligibles à partir du 2026-10-22). La feature `serde` est nécessaire : `toml::Table` et `toml::Value` n'existent pas sans elle.

- [ ] **Step 2 : données de règles.** Dans `arch.rs` :

```rust
/// Liste fermée des dépendances normales externes de `lf-application` (R2, 00 §2).
const APPLICATION_ALLOWED: [&str; 6] =
    ["thiserror", "chrono", "tracing", "futures", "tokio", "similar"];

/// Features admises par dépendance dans `lf-application` (R2) : `rt` pour
/// `task_local!` seulement ; `chrono` sans `clock` ni `now`.
const APPLICATION_FEATURES: [(&str, &[&str]); 2] =
    [("tokio", &["time", "macros", "rt"]), ("chrono", &["std"])];
```

  `CrateRule` gagne `features: &'static [(&'static str, &'static [&'static str])]` (`&[]` dans le gabarit `LIBRARY`) et `fn allowed_features(&self, dep: &str) -> Option<&'static [&'static str]>` ; la ligne `lf-application` devient `external: Some(&APPLICATION_ALLOWED), features: &APPLICATION_FEATURES`. `member_violations` enchaîne, dans cet ordre : dépendances interdites, features hors bornes (pour chaque `(dep, activées)` de `p.features` dont `allowed_features(dep)` est `Some`, chaque feature absente de la liste), cibles, 5e (`(!p.lints_workspace).then(…)`), contrôles du workspace. `read_package` remplit `features` (dépendances de `kind` nul), `manifest_path` (lu après `targets`) et `lints_workspace: false`. Doc de module : R2 complète, 5e, nouvelles conditions d'échec fermé ; retirer le paragraphe « Restent pour le plan 2b ».

- [ ] **Step 3 : tests.** Le helper `pkg` gagne `features: Vec::new()`, `manifest_path: format!("/w/{name}/Cargo.toml")`, `lints_workspace: true` ; nouveau helper `enabling(package, &[(&str, &[&str])]) -> Package`. Les deux tests de lecture ajoutent `"manifest_path"` à leur JSON ; le premier attend `Package { lints_workspace: false, ..enabling(pkg("lf-domain", &["thiserror"]), &[("thiserror", &["default"])]) }` ; le second donne à `tokio` `"features": ["rt", "time"], "uses_default_features": false` et attend `features: [("lf-domain", ["default"]), ("tokio", ["rt", "time"])]`, `manifest_path: "/w/crates/lf-app/Cargo.toml"`, `lints_workspace: false`. Nouveaux tests :

```rust
#[test]
fn fails_closed_on_member_without_manifest_path() {
    let metadata = json!({
        "workspace_members": ["a"],
        "packages": [{ "id": "a", "name": "lf-domain", "dependencies": [],
                       "version": "0.0.0", "edition": "2024", "targets": [] }]
    });
    assert_eq!(
        workspace_packages(&metadata),
        Err("cargo metadata: lf-domain has no manifest_path".to_owned())
    );
}

#[test]
fn application_external_list_is_closed() {
    let allowed = ["lf-domain", "thiserror", "chrono", "tracing", "futures", "tokio", "similar"];
    let graph = [
        pkg("lf-domain", &["thiserror"]),
        pkg("lf-application", &allowed),
        pkg("lf-adapter-piste", &["lf-application", "serde", "reqwest", "backon"]),
    ];
    assert_eq!(violations(&graph), Vec::<String>::new());
    let forbidden = ["serde", "serde_json", "rmcp", "reqwest", "moka", "governor", "backon"];
    let graph = [pkg("lf-domain", &["thiserror"]), pkg("lf-application", &forbidden)];
    let expected: Vec<String> =
        forbidden.iter().map(|d| format!("lf-application must not depend on {d}")).collect();
    assert_eq!(violations(&graph), expected);
}

#[test]
fn application_features_are_bounded() {
    let bounded = enabling(pkg("lf-application", &["tokio", "chrono"]),
        &[("tokio", &["time", "macros", "rt"]), ("chrono", &["std"])]);
    let unbounded = enabling(pkg("lf-adapter-piste", &["tokio"]), &[("tokio", &["full", "default"])]);
    assert_eq!(violations(&[pkg("lf-domain", &[]), bounded, unbounded]), Vec::<String>::new());
    let wide = enabling(pkg("lf-application", &["tokio", "chrono", "thiserror"]), &[
        ("tokio", &["rt", "rt-multi-thread", "time"]),
        ("chrono", &["std", "clock", "default"]),
        ("thiserror", &["default"]),
    ]);
    assert_eq!(violations(&[pkg("lf-domain", &[]), wide]), [
        "lf-application must not enable tokio feature rt-multi-thread",
        "lf-application must not enable chrono feature clock",
        "lf-application must not enable chrono feature default",
    ]);
}

#[test]
fn every_member_inherits_workspace_lints() {
    let graph = [pkg("lf-domain", &[]), Package { lints_workspace: false, ..pkg("xtask", &[]) }];
    assert_eq!(violations(&graph), ["xtask must declare [lints] workspace = true"]);
}

#[test]
fn reads_workspace_lints_in_every_toml_form() {
    let cases = [
        ("[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n", Ok(true)),
        ("lints.workspace = true", Ok(true)),
        ("lints = { workspace = true }", Ok(true)),
        ("[lints]\nworkspace = false", Ok(false)),
        ("[lints]\nworkspace = \"true\"", Ok(false)),
        ("[lints.clippy]\npedantic = \"warn\"", Ok(false)),
        ("[package]\nname = \"x\"", Ok(false)),
    ];
    for (manifest, expected) in cases {
        assert_eq!(inherits_workspace_lints(manifest), expected, "{manifest}");
    }
    assert!(inherits_workspace_lints("[lints").unwrap_err().starts_with("Cargo.toml: "));
}
```

  et trois lignes dans `check_arch_detects_each_rule`, avant « R3 » : `("R2, liste fermée", [domain(), pkg("lf-application", &["serde"])], "lf-application must not depend on serde")`, `("R2, features", [domain(), enabling(pkg("lf-application", &["tokio"]), &[("tokio", &["net"])])], "lf-application must not enable tokio feature net")`, `("5e", [domain(), Package { lints_workspace: false, ..pkg("lf-application", &[]) }], "lf-application must declare [lints] workspace = true")`.

- [ ] **Step 4 : `main.rs`.** Après `workspace_packages`, lire chaque manifeste et fixer `lints_workspace` :

```rust
let mut packages = workspace_packages(&metadata)?;
for package in &mut packages {
    let manifest = fs::read_to_string(&package.manifest_path)
        .map_err(|e| format!("{}: {e}", package.manifest_path))?;
    package.lints_workspace = inherits_workspace_lints(&manifest)
        .map_err(|e| format!("{}: {e}", package.manifest_path))?;
}
```

- [ ] **Step 5 : vérifier** `cargo nextest run -p xtask --locked`, `make ci` (`check-arch: ok` sur le vrai workspace), `make mutants FILE=xtask/src/arch.rs` — attendu : 0 `MISSED`, 0 `TIMEOUT` (essai : 66 mutants, 63 tués, 3 non viables). Contrôle manuel de la porte (puis tout restaurer, verrou compris) : ajouter `"sync"` aux features normales de `tokio` dans `crates/lf-application/Cargo.toml` et remplacer son `[lints]` par `[lints.rust]` + `unsafe_code = "forbid"`, `cargo update -w --offline`, `cargo xtask check-arch` — attendu : `lf-application must not enable tokio feature sync` et `lf-application must declare [lints] workspace = true`.

- [ ] **Step 6 : commit** `feat(xtask): check-arch, liste fermée et features de R2, lints hérités (5e)`.

Écarts : 02 §4.2 (étape 1) vise le crate `cargo_metadata` ; le livré lit le JSON par `serde_json` (fail-closed, testé) et ce plan le garde : `cargo_metadata` n'apporterait que des dépendances. `toml` 1.1 remplace la version de référence 0.9.8 (02 §3.0.1, relevé du 2026-10-15) : la série 0.9 n'a plus de correctif, la 1.x est sortie depuis février 2026. Les bornes de features de `tokio` étaient prévues en M3 (02 §4.2, tableau) : elles entrent avec R2, puisque `lf-application` naît avec `tokio`. La feature `default` est refusée par les bornes : le workspace déclare `default-features = false`.

---

### Task 6 : `check-arch`, fermeture transitive et doublons critiques (R9)

**Files:**
- Create: `xtask/src/graph.rs`
- Modify: `xtask/src/lib.rs` (`pub mod graph;` après `pub mod arch;`), `xtask/src/main.rs`

**Interfaces:**
- Produces :
  - `#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct Resolved { pub packages: BTreeMap<String, (String, String)> /* id -> (nom, version) */, pub edges: BTreeMap<String, Vec<String>> /* id -> ids, production */, pub members: Vec<String> }`.
  - `pub fn resolved_graph(metadata: &Value) -> Result<Resolved, String>` : `Err("cargo metadata: no resolve graph (run without --no-deps)")` si `resolve.nodes` n'est pas un tableau ; un paquet sans `id`, `name` ou `version` est ignoré ; une arête compte si un de ses `dep_kinds` a `kind` nul ou `"build"` ; la cible est lue dans `pkg` (le `name` d'une arête est le nom de crate souligné, `lf_domain`).
  - `pub fn graph_violations(graph: &Resolved) -> Vec<String>` : fermeture de chaque membre (ordre de `members`), puis doublons. Messages : `"{membre} must not reach {crate} ({chemin})"` avec le chemin `a -> b -> c` ; `"duplicate versions of {crate} in the resolved graph: {v1}, {v2}"`.
  - Données : `TRANSITIVE_FORBIDDEN = [("lf-domain", ["tokio", "reqwest", "serde_json", "rmcp"]), ("lf-application", ["rmcp", "reqwest", "hyper", "moka"])]` (02 §4.2, étape 4) ; `CRITICAL = ["tokio", "rustls", "reqwest", "hyper", "http", "serde", "schemars", "rmcp", "chrono"]` (R9, 06 §9.3).

- [ ] **Step 1 : tests** (`graph.rs`), avec un helper `graph(members, edges, extra) -> Resolved` dont les identifiants sont `nom@version` :

```rust
#[test]
fn domain_must_not_reach_io_crates() {
    let g = graph(&["lf-domain@0.0.0"], &[
        ("lf-domain@0.0.0", &["thiserror@2.0.21", "x@1.0.0"]),
        ("x@1.0.0", &["y@1.0.0", "serde_json@1.0.154"]),
        ("y@1.0.0", &["tokio@1.53.2"]),
    ], &[]);
    assert_eq!(graph_violations(&g), [
        "lf-domain must not reach serde_json (lf-domain -> x -> serde_json)",
        "lf-domain must not reach tokio (lf-domain -> x -> y -> tokio)",
    ]);
}

#[test]
fn application_must_not_reach_network_or_mcp_crates() {
    let g = graph(&["lf-application@0.0.0"], &[
        ("lf-application@0.0.0", &["tokio@1.53.2", "z@1.0.0"]),
        ("z@1.0.0", &["hyper@1.7.0", "rmcp@3.5.1", "moka@0.12.16", "reqwest@0.13.5"]),
    ], &[]);
    let expected: Vec<String> = ["hyper", "rmcp", "moka", "reqwest"].iter()
        .map(|n| format!("lf-application must not reach {n} (lf-application -> z -> {n})")).collect();
    assert_eq!(graph_violations(&g), expected);
}

#[test]
fn other_members_reach_anything() {
    let g = graph(&["xtask@0.0.0", "lf-adapter-piste@0.0.0"], &[
        ("xtask@0.0.0", &["serde_json@1.0.154"]),
        ("lf-adapter-piste@0.0.0", &["reqwest@0.13.5", "tokio@1.53.2"]),
    ], &[]);
    assert_eq!(graph_violations(&g), Vec::<String>::new());
}

#[test]
fn each_forbidden_crate_is_reported_once_by_its_shortest_path() {
    let g = graph(&["lf-domain@0.0.0"], &[
        ("lf-domain@0.0.0", &["a@1.0.0", "b@1.0.0"]),
        ("a@1.0.0", &["c@1.0.0", "lf-domain@0.0.0"]),
        ("b@1.0.0", &["tokio@1.53.2"]),
        ("c@1.0.0", &["tokio@1.53.2", "a@1.0.0"]),
    ], &[]);
    assert_eq!(graph_violations(&g), ["lf-domain must not reach tokio (lf-domain -> b -> tokio)"]);
}

#[test]
fn critical_crates_have_a_single_version() {
    let g = graph(&["xtask@0.0.0"], &[],
        &["tokio@1.53.2", "tokio@1.54.0", "chrono@0.4.45", "rand@0.8.5", "rand@0.9.2"]);
    assert_eq!(graph_violations(&g),
        ["duplicate versions of tokio in the resolved graph: 1.53.2, 1.54.0"]);
}
```

  `reads_production_edges_of_the_resolve_graph` : métadonnées avec le membre `d` (`lf-domain`) et des arêtes vers `t` (`kind` nul), `p` (`"dev"`), `c` (`"build"`), `s` (deux genres : `"dev"`, et nul avec `"target": "cfg(unix)"`) ; paquets supplémentaires `{ "id": "anonymous" }`, `{ "id": "unversioned", "name": "x" }`, `{ "name": "no-id", "version": "1.0.0" }` (ignorés) ; un nœud sans `id` (ignoré) ; attendu : `packages` = `d`, `t`, `p`, `c`, `s` avec leurs noms et versions, `edges` = `d -> [t, c, s]`, `t -> []`, `members` = `[d]`. `fails_closed_without_resolve_graph` : `{"resolve": null}` et `{"resolve": {"nodes": null}}` ⇒ l'erreur ci-dessus.

- [ ] **Step 2 : implémenter.** Parcours en largeur depuis chaque membre qui a une ligne dans `TRANSITIVE_FORBIDDEN` ; un ensemble `seen` contient le membre dès le départ, et `seen.insert` décide seul de la visite : avec deux conditions (« est le membre » ou « déjà vu »), la mutation `||` → `&&` fait boucler le parcours sur un cycle et finit en `TIMEOUT` (essai). Chemin reconstruit par une table `parent`.

```rust
let mut seen = BTreeSet::from([member]);
let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
let mut queue = VecDeque::from([member]);
let mut found = Vec::new();
while let Some(id) = queue.pop_front() {
    for next in graph.edges.get(id).into_iter().flatten() {
        if !seen.insert(next) {
            continue;
        }
        parent.insert(next, id);
        queue.push_back(next);
        let name = name_of(graph, next);
        if forbidden.contains(&name) {
            found.push(format!("{start} must not reach {name} ({})", path(graph, &parent, next)));
        }
    }
}
```

  `name_of(graph, id)` rend le nom du paquet, ou l'identifiant s'il est inconnu. Doublons : `BTreeMap<&str, BTreeSet<&str>>` des versions des seuls crates de `CRITICAL`, une violation par nom à plus d'une version, versions dans l'ordre lexicographique. Doc de module : arêtes normales et de build seulement pour la fermeture, R9 sur tout le graphe, `--all-features` pour le pire cas, échec fermé sans graphe résolu.

- [ ] **Step 3 : `main.rs`.** `cargo metadata --format-version 1 --all-features --locked` (sans `--no-deps`, commentaire : « graphe résolu complet : `--all-features` pour le pire cas, `--locked` pour le verrou ») ; après `violations(&packages)`, `found.extend(graph_violations(&resolved_graph(&metadata)?))`.

- [ ] **Step 4 : vérifier** `make ci` (`check-arch: ok`), `make mutants FILE=xtask/src/graph.rs` — attendu : 0 `MISSED`, 0 `TIMEOUT` (essai : 24 mutants, 24 tués). Contrôle manuel (puis tout restaurer, verrou compris) : ajouter `tokio = { workspace = true, features = ["time"] }` aux dépendances de `crates/lf-domain/Cargo.toml`, `cargo update -w --offline`, `cargo xtask check-arch` — attendu : `lf-domain must not depend on tokio` et `lf-domain must not reach tokio (lf-domain -> tokio)`.

- [ ] **Step 5 : commit** `feat(xtask): check-arch, fermeture transitive et doublons critiques (R9)`.

Note : sans `--no-deps`, `cargo metadata` lit les manifestes des dépendances ; la CI les a déjà (`cargo fetch` ou cache), un poste hors ligne aussi après un premier build. R6 (crates confinés) et R8 (dev-dependencies) restent non programmées : elles entrent au plan 2c, avec le premier adapter et le premier crate confiné (`reqwest`).

---

### Task 7 : documentation

**Files:**
- Modify: `docs/plan/00-decisions.md`, `docs/plan/02-architecture.md`, `docs/plan/06-qualite.md`, `docs/plan/07-feuille-de-route.md`, `docs/plan/09-risques-et-questions.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes : le livré des tâches 1 à 6.
- Produces : documents sans « plan 2b » pour ce que le plan 2c reprend ; état livré écrit une fois par document.

- [ ] **Step 1 : renvois au plan 2c.** Partout où T-M2-27 (spikes, licences, ADR-0013), M1-D16, M1-D17, `--ignore-filename-regex`, R6 ou R8 sont attribués au « plan 2b », écrire « plan 2c » : 00 §11 (ligne M2, l. 198) et §11.1 (l. 245, 249), 07 l. 9, 248, 376, 379, 09 l. 1456, 1457, 1528 (relevé par `grep -n "plan 2b" docs/plan/*.md`). Ajouter dans 00 §11.1, après la phrase sur le plan 2a, le découpage du 2026-10-21 (plan 2b sans réseau ; plan 2c : naissance de l'adapter et T-M2-27).

- [ ] **Step 2 : état livré.**
  - 07 : T-M1-04 → *Livrée (plan 2b, tâches 1 et 2), sauf `PaginationMode` (plan 2c, avec le `PaginationMemo`)* ; T-M3-01 → partie `LegalCorpus` et `CorpusError` *livrée (plan 2b, tâche 3)* ; T-M0-15 → *côté application livré (plan 2b, tâche 4) ; lecture par l'adapter au plan 2c* ; T-M2-28 → `check-arch` R2 fermée (avec bornes de features), fermeture transitive, R9 et 5e *livrés (plan 2b, tâches 5 et 6)*, reste `--ignore-filename-regex` (plan 2c) ; table des étapes (l. 1019) à jour.
  - 00 : §2, paragraphe des règles de `check-arch` (l. 52) : R2 fermée, bornes de features de `tokio` et `chrono`, fermeture transitive et R9 livrées (plan 2b) ; §11 journal : ajouter après l'entrée du plan 2a « Plan 2b livré le AAAA-MM-JJ (`docs/superpowers/plans/2026-10-21-m2b-application.md`, tâches 1 à 7) : `SearchQuery` et ses types, `lf-application` (`LegalCorpus`, `CorpusError`, `Deadline`), `check-arch` R2, 5e, fermeture transitive, R9. »
  - 02 : §3.0.1, ligne `cargo_metadata / toml` : `toml` 1.1 (exigence `"1.1"`, `std`, `parse`, `serde`) remplace 0.9.8, `cargo_metadata` non retenu (lecture JSON par `serde_json`) ; §3.0.2 : « Livré : aucune entrée lf-* » devient « Livré : `lf-domain` (plan 2b) » ; §4.1 et §4.2 : R2 fermée, 5e, étapes 4 et 7 livrées (plan 2b), bornes de features de `tokio` avancées de M3 au plan 2b ; §4.4 : `make purity` livré.
  - 06 §9.3 : « refusera (règle R9, M2…) » devient « refuse (R9, livrée au plan 2b) ».
  - 09 §3.19 : M1-D10 → statut « résolu (plan 2b, tâches 5 et 6) » ; §5, critères M2 : `SearchQuery`, `lf-application`, extensions de `check-arch` cochées.
  - `CHANGELOG.md`, `[Unreleased]` : *Added* — « `lf_domain::search` : `QueryText`, `PageSize`, `MatchMode`, `SearchQuery`, `has_more`, `REACHABLE_RESULTS` ; `Fond::needs_version_date`, `Fond::has_date_filter` ; erreurs `EmptyQuery`, `QueryTooLong`, `OutOfRange`, `VersionDateNotApplicable`, `DateFilterNotApplicable`, `PageBeyondReach`. », « Crate `lf-application` : port `LegalCorpus`, `CorpusError`, `Deadline`. », « `make purity` (`cargo check -p lf-application`). » ; *Changed* — « `check-arch` : liste fermée et features de `lf-application` (R2), `[lints] workspace = true` (5e), fermeture transitive, doublons critiques (R9) ; graphe résolu complet. »

- [ ] **Step 3 : vérifier** `grep -n "plan 2b" docs/plan/*.md` — attendu : seulement des mentions du livré du plan 2b ; `make ci` vert (aucun `.rs` touché : pas de mutation).

- [ ] **Step 4 : commit** `docs(plan): état livré du plan 2b, renvois au plan 2c`.

---

## Après la tâche 7

- **Vérifications finales** avant la PR : `make ci` vert ; `cargo +1.88 check --workspace --all-targets --locked` ; `make mutants FILE=…` relancé sur chaque `.rs` touché, un à la fois (`search.rs`, `fond.rs`, `error.rs`, `ports/corpus.rs`, `deadline.rs`, `arch.rs`, `graph.rs`) : 0 `MISSED`, 0 `TIMEOUT` ; `make min-age` vert à la date de la PR (un transitif redescendu à l'essai peut remonter si sa version récente est devenue éligible : relancer la procédure de la tâche 4, étape 2) ; la PR suit le modèle de dépôt et cite ce plan ; le propriétaire fusionne lui-même.
- **Plan 2c** : naissance de `lf-adapter-piste` ; spikes TLS (aws-lc ou ring, 7 cibles sur les runners GitHub, V-02), proxy (V-03), `backon` (V-06), `governor` (V-07) ; `CallDeadline` lisant `Deadline::current_or` (T-M0-15) ; licences de `deny.toml` et `fuzz/Cargo.lock` (M1-D16, M1-D17), inventaire des licences (V-25), ADR-0013 ; `PaginationMode` ; R6, R8 et `wrappers` de `cargo deny` pour `reqwest` ; `--ignore-filename-regex` avec la glue HTTP ; puis T-M2-01 et suivantes.
