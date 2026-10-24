# M2e Domaine — plan d'implémentation (valeurs, texte structuré, entités complètes)

> **Origine.** Plan 2e, écrit le 2026-10-23 après le merge du plan 2d (PR #10, #11 et #13 ; `main` = `ecf227c`). Il n'a **pas** de passage à blanc : les interfaces viennent de l'analyse d'écart entre la spécification (03 §3.3, §4, §5, §7 ; 04 §9) et le code livré (aucun fichier `lf-domain` n'est supposé, tout est relu). Le plan 2d annonçait un plan 2e « mapping et `impl LegalCorpus` » ; l'analyse d'écart montre que **le mapping ne peut pas s'écrire** tant que le domaine n'a pas `LegalStatus`, `Nor`, `Idcc`, `DocumentId`, `LongText`, `from_html`, `OfficialSummary` et les champs d'entité correspondants (00 §11.1, « Reports par jalon : M2 »). Le travail est donc découpé en **quatre plans** :
>
> | Plan | Contenu | Crate |
> |---|---|---|
> | **2e (ce plan)** | compléments du domaine : valeurs, texte structuré, entités complètes, fixtures installées | `lf-domain` (+ données dans `lf-adapter-piste/tests`) |
> | 2f | budget de décodage R11 et R13, `map/` (T-M2-07), diagnostics et compteurs | `lf-adapter-piste` |
> | 2g | `request/consult.rs`, `impl LegalCorpus` (T-M2-09), retrait de l'`expect(dead_code)`, contrats sur fixtures, golden, `insta` | `lf-adapter-piste` |
> | 2h | `record-fixture` (T-M2-11), tests live (T-M2-12), sondes T-M2-14 à T-M2-22 (compte PISTE) ; T-M2-13 passe en M4 | adapter, `xtask` |
>
> **R11 n'est pas un frein** : rien n'appelle `decode` en production avant le plan 2g, et le plan 2f le pose en première tâche. Les décisions de portée sont dans le tableau « Décisions de cadrage ».

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** donner à `lf-domain` tout ce dont le mapping DTO → domaine du plan 2f a besoin et qui manque aujourd'hui : statut juridique, NOR, IDCC, identifiant de document typé, HTML vers paragraphes, `LongText`, extrait de recherche, sommaire officiel, champs complets de `Act`, `Convention`, `ArticleText`, `Decision`, `Deliberation` et `CompanyAgreement`, plus les 16 fixtures de la référence installées pour les tests.

**Architecture :** tout reste pur (aucune E/S, aucun `async`, seule dépendance normale `thiserror`, R1). Le lexer HTML est écrit à la main dans `text/html.rs` et partagé par `clean` (champs courts) et `from_html` (paragraphes). Les constructeurs qui portent un invariant déléguent leur logique à des fonctions privées (cargo-mutants ne mute jamais une fonction `new`). Les champs d'entité restent publics (cohérence avec le livré) ; `SearchHit::new` est la seule voie du mapper pour construire un hit (invariant du préfixe).

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain 1.99.0) ; `thiserror` ; dev-dépendances `proptest`, `pretty_assertions` et, nouveauté, `serde_json` (workspace) pour lire les fixtures dans les tests.

**Spec :** `docs/plan/03-domaine.md` §2.5, §3.1, §3.3, §4.0 à §4.7, §5.4, §7.2 à §7.5, §9 ; `docs/plan/04-adapter-piste.md` §9.1, §9.2, §13.3 ; `docs/plan/00-decisions.md` §4, §10, §11.1 ; `docs/plan/06-qualite.md` §2.1, §2.3 ; `docs/plan/annexes/A-parite.md` §5.3, §5.4.

---

## Global Constraints

- Rust edition 2024, `rust-version = "1.88"` (`cargo +1.88 check --workspace --all-targets --locked`), toolchain `1.99.0`.
- `lf-domain` : **seule dépendance normale `thiserror`** (R1) ; pas de `regex`, pas de `serde`, pas de crate HTML : lexers écrits à la main. `serde_json` n'entre qu'en dev-dépendance (R8 ne vise que les adapters ; `cargo xtask check-arch` doit rester vert). Aucune nouvelle dépendance de production ; `serde_json` est déjà au workspace (`"1.0"`), sa version est déjà dans `Cargo.lock` (règle d'âge sans objet).
- Lints : `clippy::pedantic`, `unwrap_used`, `expect_used`, `panic`, `unreachable`, `unimplemented`, `todo`, `dbg_macro` bloquants par `-D warnings` (`unwrap`/`expect` permis dans les tests seulement) ; `missing_docs`, `unreachable_pub` ; `unsafe_code = forbid`. Docs et commentaires en français ; un nom CamelCase ou de produit prend des backticks (`clippy::doc_markdown`) ; identifiants Rust en anglais ; messages d'erreur en anglais, sans point final.
- Qualité (00 §10) : TDD (test rouge montré, puis code) ; `make cov` = 100 % des lignes, régions et fonctions (`--lib`) ; `make mutants FILE=<fichier>` = 0 `MISSED` et 0 `TIMEOUT` sur chaque fichier de **production** dont une fonction change, **un fichier à la fois** (jamais `cargo mutants` hors `make`). Les constructeurs `new` et les `impl Default` ne sont jamais mutés : toute décision vit dans une fonction privée nommée autrement, testée directement. Une boucle de lexer consomme au moins un octet par tour (sinon TIMEOUT bloquant).
- Formes de test interdites (région non exécutée, CONTRIBUTING.md) : `assert!(matches!(..))` sur une valeur dont le mauvais bras n'est jamais pris ; test « ignoré si le fichier manque » (les fixtures se lisent par `include_str!`, crates `publish = false`). Chaque branche `Option` d'une fonction exige un cas.
- Chaque champ ajouté à une entité casse les constructions littérales des tests (`entity/mod.rs` `contracts()`, tests de chaque entité) : les corriger dans la même tâche.
- Fixtures : copiées **sans modification** (jamais reformatées ni réparées) ; les chemins de test sont relatifs au fichier source : `include_str!("../../../lf-adapter-piste/tests/fixtures/reference/<nom>.json")` depuis `crates/lf-domain/src/<dir>/`. Un test qui lit une fixture connue en mojibake applique d'abord `text::encoding::repair_latin1_mojibake` au texte entier (ordre imposé, 03 §7.1).
- WSL (`~/.claude/rules/wsl-safety.md`) : jamais `cargo test -j 4` ni plus ; `cargo mutants` uniquement par `make mutants`.
- Commits : un par tâche, message en français au format conventionnel, sans trailer ajouté par l'implémenteur (le contrôleur ajoute `Co-Authored-By`). Rien n'est poussé avant la fin ; le propriétaire merge lui-même.

## Review Focus

1. **HTML hostile** : `<` jamais refermé, 1 Mio de `<<<<`, 100 000 balises ouvrantes, guillemet d'attribut non fermé, commentaire non fermé, `<RECH_` sans fermante, entité à la limite (`&#x110000;`, `&#0;`) — jamais de panique, temps linéaire, jamais de perte de texte quand une fermante manque. Tests : tâches 4 et 5.
2. **Caractères invisibles** : bidi (`U+202E`), tags `U+E0000…`, `U+200B`, `U+00AD`, contrôles C0/C1 dans un titre, une entité numérique, un extrait — retirés **et comptés**, y compris quand ils viennent d'une entité (`&#173;`, `&#x202E;`). Tests : tâche 4.
3. **Sommaire commençant par deux chiffres** : « 15 jours après la décision… » ne perd pas son début (le code de classement n'est retiré que suivi d'une majuscule) ; trois analyses CETAT identiques n'en font qu'une. Tests : tâche 8.
4. **Hit d'un autre fond** : un `CNILTEXT` dans un hit JURI, un `LEGITEXT` dans un hit KALI, un identifiant vide ou malformé — `HitKindMismatch` ou `InvalidId`, jamais un hit renvoyé à moitié. Tests : tâches 3 et 7.
5. **IDCC et NOR malformés** : IDCC `0`, `10000`, `12a`, `"0000000000002121"` ; NOR en minuscules (`cscl2115694s`), trop court, avec un chiffre à la place d'une lettre — `None` ou valeur normalisée, jamais de panique. Tests : tâche 2.
6. **Texte énorme** : `LongText` au-delà de 16 000 000 caractères ⇒ `TextTooLarge`, comptage en `u32` sans troncature silencieuse ; un `texteHtml` presque vide face à un `texte` complet ⇒ `texte` préféré (garde-fou 90 %). Tests : tâche 6.

## Décisions de cadrage (à valider par le propriétaire)

Le propriétaire a demandé « le plan 2e » le 2026-10-23, sans autre précision, et n'est pas joignable pendant l'écriture : chaque ligne est un choix de la rédaction, annulable. Le contrôleur les reporte dans le ledger de l'exécution.

| Décision | Pourquoi | Si elle est fausse |
|---|---|---|
| Plan 2e = **domaine seul** ; mapping en 2f ; branchement en 2g ; outils/sondes en 2h | le mapping exige ces types ; un plan par crate donne des PR relisibles (le plan 2d en avait 10 tâches) | fusionner 2e et 2f en une PR |
| `LongText::{from_html, from_plain, best_of}` renvoient `Result<LongText, DomainError>` (`TextTooLarge`) | 03 §7.5 donne `-> LongText` mais impose l'échec au-delà de 16 000 000 caractères ; un corps de 16 Mio ASCII l'atteint | passer en `LongText` infaillible tronquant (perte silencieuse : non) |
| `LongText` dérive `Clone, PartialEq, Eq, Default` ; `Debug` écrit à la main (longueur seulement, jamais le texte) | les entités dérivent `Debug, PartialEq, Eq` ; un texte juridique ne doit pas fuir dans un `{:?}` | un test fondé sur `{:?}` voit moins : parcours explicite des chaînes |
| `Fond::hit_prefix() -> &'static str` (pas `IdPrefix`) | `IdPrefix` n'existe pas et n'a de consommateur qu'en M6 | introduire `IdPrefix` en M6 |
| `Idcc(u16)` (1 à 4 chiffres significatifs, 1..=9999, `0` refusé) | annexe B l. 1044 ; 03 §4.6 dit `u32` : aligner 03 | `u32` : changer un type |
| Les noms de champs **livrés** sont gardés (`fond`, `raison_sociale`, `syndicats`, `date_debut_version`, `date_fin_version`, `text_id`) | annexe A §5.4 les retient ; 03 emploie d'autres noms | renommer dans 03 seulement |
| Hors plan (M7) : `previous_version`, `cited_case_law`, `applied_texts`, `SearchHit.solution` ; **inclus** : `Act.aliases`, `jo_reference`, `Convention.bulletin` | 00 §11.1 ne les liste pas pour M2 ; 05 rend `aliases` en v0.1 | ajouter un champ à une entité publique |
| `ArticleText.parent` en M2 ; l'URL d'un article reste `/codes/article_lc/` (V-140 et V-142 non levées) | `parent` sert `parent_id` et la citation ; changer l'URL exige les sondes live | ajouter `loda_article` après V-140 |
| **Q12 appliquée avec la proposition par défaut** : la citation d'un article d'un texte non codifié (`parent.nature == Text`) retire l'appel de note final « (n) » du titre du texte parent ; `num` verbatim | 00 §4 et 03 §5.4 l'adoptent par défaut, à figer avant les golden ; le propriétaire n'a pas répondu | retirer un test et une fonction de `contract::rules` |
| `clean` **garde** son comportement actuel (`<br>` ⇒ `\n`, `<p>` ⇒ ligne vide) et gagne le durcissement ; les autres balises de bloc donnent un saut de ligne, `td`/`th` une espace | 00 §4 (normatif) et les tests livrés décrivent ce comportement ; 03 §7.3 (cible) replie tout en une espace | basculer vers l'espace unique : tests de `clean` et du golden à réécrire |
| Le comptage des invisibles passe par `clean_with_report` ; `clean` délègue et garde sa signature | 03 §7.3 exige « leur nombre est rendu comme valeur » sans casser les appelants | aucun |
| Pas de `DomainError::is_input()` ; pas de champ `has_more` dans `SearchPage` | 03 §9 exclut `is_input()` ; 00 §11.1 renvoie `has_more` à M5 (la fonction libre `search::has_more` existe) | corriger 04 §9.1 (h) au plan 2f |
| Le code de classement d'un sommaire n'est retiré que **suivi d'une majuscule** (écart à 03 §4.4, qui ne pose pas de garde) | « 15 jours après… » commence par deux chiffres puis une espace | retirer la garde : le test `summary_keeps_a_leading_number_before_lowercase_text` saute |
| Tests de contrat dans `src/` sous `#[cfg(test)]` (pas de `tests/`) | `make cov` mesure `--lib` ; 06 §2.3 | déplacer |
| Fixtures installées **dans ce plan** (données seules), pas au plan 2g | les tests de `from_html` (49/43/3 paragraphes) et de `LongText` (14 332 caractères) en dépendent | décaler la tâche 1 |

## Structure des fichiers

```text
crates/lf-adapter-piste/tests/fixtures/reference/   [T1] 16 *.json + 16 *.meta.toml + parite-golden.json + parite-golden.meta.toml
NOTICE                                               [T1] crédit des fixtures
crates/lf-domain/
  Cargo.toml                                         [T1] dev-dépendance serde_json
  src/lib.rs                                         [T2] pub mod status
  src/status.rs                                      [T2] LegalStatus, status_of
  src/contract/nor.rs, idcc.rs                       [T2] Nor, Idcc ; contract/mod.rs les ré-exporte
  src/id.rs                                          [T3] DocumentId
  src/fond.rs                                        [T3] Fond::hit_prefix
  src/error.rs                                       [T3] HitKindMismatch, TextTooLarge
  src/text/html.rs                                   [T4, T5] lexer partagé, entités, paragraphes
  src/text/clean.rs                                  [T4] clean durci, clean_with_report, clean_field
  src/text/long_text.rs                              [T6] LongText
  src/text/mod.rs                                    [T4-T6] exports
  src/contract/text.rs                               [T7] cut_at_word paramétré
  src/entity/search.rs                               [T7] Snippet, SearchHit, SearchPage
  src/entity/summary.rs                              [T8] OfficialSummary
  src/entity/act.rs, toc.rs                          [T9] TextBody, Act, Convention, TOC
  src/entity/article.rs, decision.rs, deliberation.rs, agreement.rs   [T10, T11]
  src/contract/rules.rs                              [T10] citation d'un article d'un texte non codifié (Q12)
  src/entity/mod.rs                                  [T7-T11] exports et contracts()
fuzz/fuzz_targets/text.rs, from_html.rs, fuzz/Cargo.toml   [T4, T6]
docs/plan/03-domaine.md, 00-decisions.md, 07-feuille-de-route.md   [T11]
```

---

### Task 1 : installer les fixtures de la référence (données seules)

Prérequis des tâches 5, 6 et 9, et du plan 2g. Aucun code livré. Source publique (Apache-2.0) : `matematicsolutions/fr-eli-mcp` au commit `d3b5b13`, dossier `tests/fixtures/` ; 16 fichiers, 204 862 octets au total.

**Files:**
- Create: `crates/lf-adapter-piste/tests/fixtures/reference/{consult_acco,consult_article,consult_cnil,consult_juri,consult_juri_cetat,consult_juri_constit,consult_kali,consult_loda,search_acco,search_cetat,search_cnil,search_code,search_constit,search_juri,search_kali,search_loda}.json`
- Create: les 16 `.meta.toml` homonymes ; `parite-golden.json` (copie à l'octet de `docs/plan/annexes/donnees/parite-golden.json`) et `parite-golden.meta.toml`
- Modify: `NOTICE`, `crates/lf-domain/Cargo.toml` (`serde_json.workspace = true` en `[dev-dependencies]`)

**Interfaces:**
- Produces : le chemin `crates/lf-adapter-piste/tests/fixtures/reference/<nom>.json` et le format des sidecars, consommés par les tests des tâches 5, 6, 9 et par le plan 2g.

- [ ] **Step 1 : télécharger.** Pour chaque nom, `curl -fsSL https://raw.githubusercontent.com/matematicsolutions/fr-eli-mcp/d3b5b13/tests/fixtures/<nom>.json -o …` (le SHA court se résout ; sinon utiliser le SHA complet obtenu par `git ls-remote`). Vérifier : 16 fichiers, 204 862 octets (`cat … | wc -c`). Si le total diffère, **s'arrêter et rapporter** (mauvais commit).
- [ ] **Step 2 : sidecars.** Un `.meta.toml` par fixture, mêmes clés que 04 §13.3 : `source = "matematicsolutions/fr-eli-mcp@d3b5b13"`, `endpoint` (ex. `"/consult/juri"` pour `consult_juri*`, `"/search"` pour `search_*`, `"/consult/lawDecree"` pour `consult_loda`, `"/consult/kaliText"` pour `consult_kali`, `"/consult/getArticle"`, `"/consult/cnil"`, `"/consult/acco"`), `fond` (`"JURI"`, `"CETAT"`, `"CONSTIT"`, `"CNIL"`, `"KALI"`, `"LODA"`, `"CODE"`, `"ACCO"`, `null` omis pour getArticle), `sha256_raw` (`sha256sum`), `known_issues`, `env = "sandbox"`, `captured_at = "inconnu"`. `known_issues = ["mojibake"]` pour exactement les fixtures où `python3 -I docs/plan/annexes/donnees/scripts/dto_repair_check.py <fichier>` signale une réparation au niveau du corps (attendu : 6 fichiers ; si le compte diffère, rapporter), `[]` sinon.
- [ ] **Step 3 : golden.** `cp docs/plan/annexes/donnees/parite-golden.json crates/lf-adapter-piste/tests/fixtures/reference/` puis `cmp` (identique). `parite-golden.meta.toml` : `source = "docs/plan/annexes/donnees/parite-golden.json"`, `sha256`, `reference = "matematicsolutions/fr-eli-mcp@d3b5b13"`, `license = "Apache-2.0 (référence) ; données Légifrance, Licence Ouverte 2.0"` (annexe A §5.3).
- [ ] **Step 4 : `NOTICE`.** Ajouter l'attribution des 16 fixtures (projet, commit, licence Apache-2.0, titulaire à établir : V-214) à côté du crédit existant des règles portées.
- [ ] **Step 5 : dev-dépendance.** `serde_json.workspace = true` sous `[dev-dependencies]` de `crates/lf-domain/Cargo.toml` ; `cargo metadata --format-version 1 --locked > /dev/null` (le verrou ne bouge pas) ; `make arch deny machete` (machete peut signaler la dépendance tant qu'aucun test ne l'emploie : la tâche 5 la consomme ; si `make ci` de la tâche l'exige, ajouter dès cette tâche un test minimal `fixtures_are_valid_json` dans `crates/lf-domain/src/lib.rs` sous `#[cfg(test)]` qui parse les 16 fichiers par `include_str!` et `serde_json::from_str::<serde_json::Value>`).
- [ ] **Step 6 : vérifier.** `sha256sum -c` des 16 sommes de leurs sidecars (script shell d'une ligne dans le message de commit, non versionné), `git diff --stat` (aucun fichier modifié hors liste), `.gitattributes` protège déjà `**/tests/fixtures/** -text` (contrôler que `git diff --check` est muet).
- [ ] **Step 7 : commit** `test(fixtures): fixtures de la référence et golden (T-M2-10, données seules)`.

---

### Task 2 : `LegalStatus`, `status_of`, `Nor`, `Idcc`

03 §3.3 et §4.6 ; 04 §9.1 (g). Fonctions pures, sans dépendance de la suite.

**Files:**
- Create: `crates/lf-domain/src/status.rs`, `crates/lf-domain/src/contract/nor.rs`, `crates/lf-domain/src/contract/idcc.rs`
- Modify: `crates/lf-domain/src/lib.rs` (`pub mod status;`), `crates/lf-domain/src/contract/mod.rs` (`mod nor; mod idcc; pub use nor::Nor; pub use idcc::Idcc;`)

**Interfaces:**
- Produces :
  - `#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub enum LegalStatus { Vigueur, VigueurEtendue, VigueurNonEtendue, VigueurDifferee, Abroge, AbrogeDiff, Modifie, ModifieMortNe, Perime, Annule, Disjoint, Transfere, Remplace, Denonce, Initiale, SansEtat, Substitue, Other(String) }`
  - `impl LegalStatus { pub fn parse(raw: &str) -> Option<Self>; pub fn as_api_code(&self) -> &str }` — `parse` : `trim`, insensible à la casse, `""` ou blanc ⇒ `None`, code inconnu ⇒ `Other(<texte nettoyé par echo>)` (jamais plus de `ECHO_MAX_CHARS` caractères + `…`).
  - `pub fn status_of(etat: Option<&str>, juris_state: Option<&str>) -> Option<LegalStatus>` — `etat` s'il n'est pas vide après trim, sinon `juris_state`.
  - `#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Nor(String)` ; `Nor::parse(raw: &str) -> Option<Self>` (trim, majuscules ASCII, exactement `[A-Z]{4}[0-9]{7}[A-Z]`) ; `as_str(&self) -> &str` ; `Display`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub struct Idcc(u16)` ; `Idcc::parse(raw: &str) -> Option<Self>` (trim ; uniquement des chiffres ASCII ; zéros de tête ignorés ; valeur 1..=9999 ; sinon `None`) ; `get(self) -> u16` ; `Display` sans zéros de tête.
  - Les 17 codes de l'API (03 §3.3) : `ABROGE`, `ABROGE_DIFF`, `ANNULE`, `DENONCE`, `DISJOINT`, `INITIALE`, `MODIFIE`, `MODIFIE_MORT_NE`, `PERIME`, `REMPLACE`, `SANS_ETAT`, `SUBSTITUE`, `TRANSFERE`, `VIGUEUR`, `VIGUEUR_DIFF`, `VIGUEUR_ETEN`, `VIGUEUR_NON_ETEN` ↔ variantes dans l'ordre ci-dessus (`VIGUEUR_ETEN` = `VigueurEtendue`, `VIGUEUR_DIFF` = `VigueurDifferee`, `VIGUEUR_NON_ETEN` = `VigueurNonEtendue`).

- [ ] **Step 1 : tests de `status.rs`** (rouges). `legal_status_table_roundtrips_the_17_codes` (chaque code ⇒ variante ⇒ `as_api_code` identique, casse mêlée et espaces acceptés en entrée) ; `legal_status_unknown_code_is_other` (`"EN_COURS"` ⇒ `Other("EN_COURS")`, `as_api_code` rend `"EN_COURS"`) ; `legal_status_blank_is_none` (`""`, `"  "`) ; `legal_status_other_is_truncated` (100 caractères ⇒ 64 + `…`) ; `legal_status_from_etat_or_juris_state` :

```rust
assert_eq!(status_of(None, Some("Vigueur")), Some(LegalStatus::Vigueur));               // LODA : etat null
assert_eq!(status_of(None, Some("VIGUEUR_ETEN")), Some(LegalStatus::VigueurEtendue));   // KALI
assert_eq!(status_of(Some("  "), Some("Vigueur")), Some(LegalStatus::Vigueur));         // etat blanc
assert_eq!(status_of(Some("ABROGE"), Some("Vigueur")), Some(LegalStatus::Abroge));      // etat prime
assert_eq!(status_of(None, None), None);
```
- [ ] **Step 2 : tests de `nor.rs` et `idcc.rs`.** `nor_uppercases_and_validates` (`"cscl2115694s"` ⇒ `CSCL2115694S` ; `"ASET2450455M"`, `"ECFI1524250L"`, `"CSCX2531180S"` acceptés), `nor_rejects_malformed` (11 et 13 caractères, chiffre à la place d'une lettre de tête, lettre dans les chiffres, vide, espace interne), `idcc_strips_leading_zeros` (`"0000000000002121"` ⇒ 2121, `"275"`, `"0275"` ⇒ 275, Display `"275"`), `idcc_rejects_out_of_range_and_garbage` (`"0"`, `"0000"`, `"10000"`, `"12a"`, `"-5"`, `""`, `"２１２１"` chiffres pleine chasse), proptests d'aller-retour : `Nor::parse(&nor.to_string()) == Some(nor)` et `Idcc::parse(&idcc.to_string()) == Some(idcc)`, et `parse` ne panique jamais sur une chaîne arbitraire.
- [ ] **Step 3 : lancer, constater le rouge** `cargo nextest run -p lf-domain status nor idcc` (échec de compilation attendu : types absents).
- [ ] **Step 4 : implémenter.** La logique de décision (table des 17 codes, validation de forme, bornes) dans des fonctions **privées** (`fn code_to_status(upper: &str) -> Option<LegalStatus>`, `fn is_nor_shape(s: &str) -> bool`, `fn idcc_value(digits: &str) -> Option<u16>`) ; `parse` ne fait que les appeler. Pas de `regex` : balayage d'octets.
- [ ] **Step 5 : vert + qualité.** `cargo nextest run -p lf-domain`, `make cov`, puis `make mutants FILE=crates/lf-domain/src/status.rs`, `…/contract/nor.rs`, `…/contract/idcc.rs` (un par un ; 0 MISSED/0 TIMEOUT).
- [ ] **Step 6 : `make ci`** puis commit `feat(domain): LegalStatus, status_of, Nor et Idcc (T-M2-07, domaine)`.

---

### Task 3 : `DocumentId`, `Fond::hit_prefix`, `HitKindMismatch` et `TextTooLarge`

03 §2.5, §3.1, §9. Prépare `SearchHit::new` (tâche 7) et `ParentText` (tâche 10).

**Files:**
- Modify: `crates/lf-domain/src/id.rs` (`DocumentId` après `DecisionId`, tests dans `mod tests`), `crates/lf-domain/src/fond.rs` (`hit_prefix`), `crates/lf-domain/src/error.rs` (deux variantes)

**Interfaces:**
- Consumes : `Id<K>` et ses dix alias (`LegiTextId`, `LegiArtiId`, `JorfTextId`, `KaliTextId`, `KaliArtiId`, `JuriTextId`, `ConsTextId`, `CetaTextId`, `CnilTextId`, `AccoTextId`), `IdKind::PREFIX`, `Fond`, `echo`.
- Produces :
  - `#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub enum DocumentId { LegiText(LegiTextId), LegiArti(LegiArtiId), JorfText(JorfTextId), KaliText(KaliTextId), KaliArti(KaliArtiId), JuriText(JuriTextId), ConsText(ConsTextId), CetaText(CetaTextId), CnilText(CnilTextId), AccoText(AccoTextId) }`
  - `impl DocumentId { pub fn parse(input: &str) -> Result<Self, DomainError>; pub fn as_str(&self) -> &str; pub fn prefix(&self) -> &'static str }` ; `Display` (= `as_str`), `FromStr` (= `parse`). `parse` répartit selon les 8 premières lettres après normalisation (trim, majuscules) et laisse `Id::parse` valider (suffixe de version bien formé retiré, mal formé ⇒ `BadSuffix`) ; préfixe inconnu ⇒ `InvalidId { input, expected: "LEGITEXT, LEGIARTI, JORFTEXT, KALITEXT, KALIARTI, JURITEXT, CONSTEXT, CETATEXT, CNILTEXT or ACCOTEXT" }`.
  - `impl Fond { pub const fn hit_prefix(self) -> &'static str }` : `Loda | Code` ⇒ `"LEGITEXT"`, `Juri` ⇒ `"JURITEXT"`, `Constit` ⇒ `"CONSTEXT"`, `Cetat` ⇒ `"CETATEXT"`, `Cnil` ⇒ `"CNILTEXT"`, `Kali` ⇒ `"KALITEXT"`, `Acco` ⇒ `"ACCOTEXT"`.
  - `DomainError::HitKindMismatch { id: String, fond: Fond }` (`#[non_exhaustive]`, `id` passé par `echo`, message `hit identifier {id:?} does not belong to fond {fond}`) ; `DomainError::TextTooLarge { chars: u64 }` (message `text of {chars} characters exceeds the 16000000 limit`). Docs avec le bloc `compile_fail` des autres variantes pour `HitKindMismatch` (non constructible hors crate).

- [ ] **Step 1 : tests.** `document_id_dispatches_on_prefix` (un exemple valide par préfixe, `as_str` rend la forme normalisée, `prefix()` rend le préfixe), `document_id_strips_a_wellformed_version_suffix` (`"legitext000033205014_08-09-2023"` ⇒ `LEGITEXT000033205014`), `document_id_rejects_unknown_prefix_and_bad_suffix` (`"FOOTEXT000000000001"` ⇒ `InvalidId`, `"LEGITEXT000033205014_foo"` ⇒ `BadSuffix`, `""`), proptest `document_id_roundtrips_through_display`, `hit_prefix_per_fond` (les 8 fonds, via `Fond::ALL`), `hit_kind_mismatch_echoes_a_truncated_id`, `text_too_large_message_names_the_limit`.
- [ ] **Step 2 : rouge, implémenter, vert.** `DocumentId::parse` : une fonction privée `fn prefix_of(normalized: &str) -> Option<&'static str>` décide ; `parse` appelle ensuite le `Id::parse` du bon alias. Mutation : `make mutants FILE=crates/lf-domain/src/id.rs`, `…/fond.rs`, `…/error.rs`.
- [ ] **Step 3 : `make ci`** puis commit `feat(domain): DocumentId, hit_prefix, HitKindMismatch et TextTooLarge`.

---

### Task 4 : lexer HTML partagé et `clean` durci

03 §7.2 (étapes 1 à 3), §7.3 ; 00 §4 ; M1-D6, M1-D7. C'est la tâche à plus fort risque (fuzz, linéarité) : le lexer sert aussi `from_html` (tâche 5).

**Files:**
- Create: `crates/lf-domain/src/text/html.rs` (lexer, classement des balises, entités)
- Modify: `crates/lf-domain/src/text/clean.rs` (réécrit au-dessus du lexer ; `clean` garde sa signature), `crates/lf-domain/src/text/mod.rs` (`mod html;`, exports `clean_with_report`, `clean_field`, `CleanReport`), `fuzz/fuzz_targets/text.rs`

**Interfaces:**
- Produces (`text/html.rs`, `pub(crate)`) :
  - `enum Event<'a> { Text(&'a str), Break(Block), Cell }` et `enum Block { Br, P, Other }` ; `fn scan(html: &str, sink: impl FnMut(Event<'_>))` : un seul passage linéaire ; `<` + `/` facultatif + nom `[A-Za-z][A-Za-z0-9_:-]*` + attributs jusqu'à `>` (**guillemets simples et doubles respectés**) = balise ; tout autre `<` est du texte littéral ; `<!-- … -->` supprimé ; un commentaire **non fermé** reste littéral (le `<!--` et ce qui suit sont du texte), pour ne jamais perdre de texte ; balise non fermée en fin d'entrée : supprimée jusqu'à la fin (03 §7.2 étape 1) ; noms commençant par `RECH_` (insensible à la casse), `script`, `style` : élément **supprimé avec son contenu** jusqu'à la fermante correspondante, et si la fermante manque seule la balise est retirée (le texte suivant est gardé) ; classement : bloc `br p div li ul ol tr table blockquote h1`–`h6` ⇒ `Break` (`br` ⇒ `Block::Br`, `p` ⇒ `Block::P`, autres ⇒ `Block::Other`), `td th` ⇒ `Cell`, tout le reste ⇒ balise retirée sans espace (`1<sup>er</sup>` ⇒ `1er`).
  - `fn decode_entities(text: &str) -> String` : `&amp; &lt; &gt; &quot; &apos; &nbsp;`, numériques `&#NNN;` et `&#xHH;` (scalaires valides, jamais contrôle/bidi/tag/invisible : l'entité reste alors littérale), plus une table fermée d'environ 100 entités latin-1 et typographiques (`&eacute; &egrave; &agrave; &ccedil; &laquo; &raquo; &rsquo; &lsquo; &ndash; &mdash; &hellip; &oelig; &deg; &sect; &euro; &copy; &reg; &times; …`) ; entité inconnue : verbatim. Appliquée **après** le retrait des balises.
  - `fn strip_invisible(text: &str) -> (String, u32)` : retire et compte U+00AD, U+061C, U+200B–U+200F, U+2060, U+FEFF, les contrôles C0/C1 sauf `\t` `\n` `\r`, les bidi U+202A–U+202E et U+2066–U+2069, les tags U+E0000–U+E007F. **Une entité numérique qui décode un de ces caractères le fait retirer et compter** (`&#173;`, `&#x202E;` : ils sont décodés puis retirés par `strip_invisible`, plus refusés comme aujourd'hui).
- Produces (`text/clean.rs`, publics, ré-exportés par `text`) :
  - `pub fn clean(html: &str) -> String` (inchangé en signature) ; `Break(Br)` ⇒ `\n`, `Break(P)` ⇒ ligne vide (`\n\n`), `Break(Other)` ⇒ `\n`, `Cell` ⇒ espace ; ensuite `decode_entities`, `strip_invisible`, normalisation des blancs existante (espaces internes réduits, lignes rognées, au plus une ligne vide) ; `trim`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub struct CleanReport { pub invisible_removed: u32 }` ; `pub fn clean_with_report(html: &str) -> (String, CleanReport)` ; `clean` = `clean_with_report(html).0`.
  - `pub fn clean_field(raw: Option<&str>) -> Option<String>` : `raw.map(clean)` puis vide ⇒ `None` (remplace `cleaned` privé de `toc.rs`, tâche 9).

- [ ] **Step 1 : tests de `html.rs` et `clean.rs`** (rouges), noms de 06 §2.1 et 03 §10 : `clean_strips_mark_tags` (`<mark>x</mark>`, `<MARK>`), `clean_folds_whitespace_after_repair` (NBSP et espaces mêlés ⇒ une espace), `clean_removes_invisible_and_control_chars` (U+00AD, U+200B, U+FEFF, `\u{0007}` ⇒ absents, comptés), `clean_removes_bidi_controls` (U+202E ⇒ absent, `invisible_removed == 1`), `clean_drops_rech_and_script_with_content` (`a<RECH_ECLI>ECLI:FR:CC:2025:2025.1173.QPC</RECH_ECLI>b` ⇒ `ab` ; `<script>x</script>y` ⇒ `y` ; `<RECH_ECLI>texte sans fermante` ⇒ `texte sans fermante`), `clean_removes_comments` (`a<!-- b -->c` ⇒ `ac` ; `a<!-- b` ⇒ `a<!-- b`), `clean_keeps_gt_inside_quoted_attributes` (`<a title="a>b">x</a>` ⇒ `x`), `clean_unclosed_tag_at_end_is_removed` (`abc <a href="x` ⇒ `abc`), `clean_literal_less_than_is_text` (`a < b`), `clean_block_tags_separate_lines` (`<li>a</li><li>b</li>` ⇒ `a\nb`), `clean_cells_become_a_space` (`<td>a</td><td>b</td>` ⇒ `a b`), `clean_numeric_entity_invisible_is_removed_and_counted` (`&#173;`, `&#x202E;`), `clean_invalid_numeric_entity_is_kept` (`&#x110000;`, `&#0;`, `&#xD800;` restent littéraux), `clean_field_blank_is_none`, `inline_tags_do_not_add_space` (`1<sup>er</sup>` ⇒ `1er`). Les tests livrés de `clean.rs` qui décrivent les limites levées (`known_tag_limits` : `>` dans un attribut, commentaires littéraux) sont **réécrits** pour la nouvelle sortie ; les autres restent verts tels quels. Proptests : `clean_never_panics`, `clean_output_has_no_invisible_chars` (aucun caractère du jeu retiré dans la sortie), `scan_is_linear` (une entrée de 1 Mio de `<` ou de `<a ` termine sous 2 s : test déterministe `std::time::Instant` avec marge large, ou comptage de pas via `sink`).
- [ ] **Step 2 : rouge, implémenter.** Une seule fonction de balayage dans `scan` ; classement des noms dans `fn classify(name_lower: &str) -> Class` (privée, testée directement) ; entités : table `const ENTITIES: &[(&str, char)]` triée + `binary_search`, fenêtre bornée à 12 octets avant `;` comme aujourd'hui. Chaque tour de boucle consomme au moins un octet.
- [ ] **Step 3 : fuzz.** `fuzz/fuzz_targets/text.rs` : ajouter l'assertion « la sortie de `clean` ne contient aucun caractère invisible » et l'appel à `clean_with_report` ; `cargo check --manifest-path fuzz/Cargo.toml --locked` (si la chaîne ne permet pas de compiler `libfuzzer-sys` localement, le dire dans le rapport : la CI hebdomadaire `weekly.yml` le compile).
- [ ] **Step 4 : vert + qualité.** `make cov`, `make mutants FILE=crates/lf-domain/src/text/html.rs` puis `…/text/clean.rs` (compter aussi « 1 mutant tested »).
- [ ] **Step 5 : `make ci`** puis commit `feat(domain): lexer HTML partagé, clean durci, clean_with_report et clean_field (M1-D6, M1-D7)`.

---

### Task 5 : `html::paragraphs` et fixtures de texte

03 §7.2 étape 4 et mesures. Consomme les fixtures (tâche 1) et le lexer (tâche 4).

**Files:**
- Modify: `crates/lf-domain/src/text/html.rs` (`paragraphs`, tests sur fixtures)

**Interfaces:**
- Consumes : `scan`, `decode_entities`, `strip_invisible` (tâche 4), `clean`'s normalisation de blancs ; `text::encoding::repair_latin1_mojibake`.
- Produces : `pub(crate) fn paragraphs(html: &str) -> (Vec<String>, u32)` : paragraphes non vides, sans `\n`, sans blancs de bord (chaque segment entre deux `Break` passe par `decode_entities`, `strip_invisible`, repli de **tous** les blancs en une espace, `trim`) ; second élément : nombre de caractères invisibles retirés. Aucune balise de bloc ⇒ un seul paragraphe ; entrée sans texte ⇒ `Vec` vide.

- [ ] **Step 1 : tests** (rouges). Synthétiques : `from_html_block_tags_split_paragraphs` (`<p>a</p><p>b</p>` ⇒ `["a","b"]` ; `a<br/>b` ⇒ 2), `from_html_drops_rech_and_script_with_content`, `from_html_unclosed_tag_at_end`, `from_html_literal_less_than` (`a < b` ⇒ `["a < b"]`), `from_html_invalid_numeric_entity_kept`, `from_html_no_block_tag_gives_one_paragraph`, `from_html_empty_paragraphs_are_dropped` (`<br/><br/>` ⇒ `[]`), `from_html_folds_inner_newlines` (`a\n   b` ⇒ `a b`), proptest `from_html_never_panics_and_preserves_text` (le texte sans balise ni entité d'une entrée sans `<` ni `&` est conservé, blancs repliés). **Fixtures** (helper de test `fn field(fixture: &str, path: &[&str]) -> String` lisant le JSON avec `serde_json::Value`, après réparation du mojibake du texte entier) :

```rust
assert_eq!(paragraphs(&text_html("consult_juri_constit", &["text","texteHtml"])).0.len(), 49);   // constit_fixture_yields_49_paragraphs
assert_eq!(paragraphs(&text_html("consult_juri_cetat",   &["text","texteHtml"])).0.len(), 43);   // cetat_fixture_yields_multiple_paragraphs
assert_eq!(paragraphs(&text_html("consult_cnil",         &["text","texteHtml"])).0.len(), 3);    // cnil_fixture_yields_3_paragraphs
```
et `constit_paragraph_chars_sum_to_14236` (somme des `chars().count()`), `cetat_paragraph_chars_sum_to_14524`, `cnil_paragraph_chars_sum_to_589`, `constit_text_is_html_plus_trailing_ecli` (`fold_ws(texte) == fold_ws(paragraphs.join(" ")) + " " + ecli` avec `fold_ws` = blancs repliés ; 03 §10 : la différence est exactement l'ECLI final supprimé avec `RECH_ECLI`). **Si un nombre diffère, s'arrêter et rapporter la valeur mesurée : ne pas ajuster le test.**
- [ ] **Step 2 : implémenter, vert.** `make cov`, `make mutants FILE=crates/lf-domain/src/text/html.rs`.
- [ ] **Step 3 : `make ci`** puis commit `feat(domain): from_html, HTML vers paragraphes, mesures sur les fixtures (03 §7.2)`.

---

### Task 6 : `LongText`

03 §7.4, §7.5. Le texte canonique est défini **après** `clean` ; toutes les entités textuelles de la suite l'emploient.

**Files:**
- Create: `crates/lf-domain/src/text/long_text.rs`
- Modify: `crates/lf-domain/src/text/mod.rs` (`mod long_text; pub use long_text::LongText;`), `fuzz/Cargo.toml` + `fuzz/fuzz_targets/from_html.rs` (cible `from_html`)

**Interfaces:**
- Consumes : `html::paragraphs` (tâche 5), `DomainError::TextTooLarge` (tâche 3).
- Produces :

```rust
#[derive(Clone, PartialEq, Eq, Default)]   // Debug à la main : `LongText(<n> chars)`, jamais le texte
pub struct LongText { canonical: Arc<str>, paras: Arc<[Para]>, char_len: u32, invisible_removed: u32 }   // Para privé : byte_start, char_start, char_len (u32)
impl LongText {
    pub const MAX_CHARS: u32 = 16_000_000;
    pub fn from_html(html: &str) -> Result<Self, DomainError>;        // TextTooLarge
    pub fn from_plain(text: &str) -> Result<Self, DomainError>;       // coupure sur les suites de \n (\r\n normalisé), clean par paragraphe
    pub fn best_of(html: Option<&str>, plain: Option<&str>) -> Result<Self, DomainError>;
    pub fn as_str(&self) -> &str;                                      // paragraphes joints par "\n\n"
    pub fn char_len(&self) -> u32;
    pub fn is_empty(&self) -> bool;
    pub fn invisible_removed(&self) -> u32;                            // invisibles et bidi retirés à la construction (diagnostic, 00 §4)
    pub fn paragraphs(&self) -> impl Iterator<Item = (u32 /* char_start */, &str)>;
}
```
- `best_of` : (1) `html` présent et non blanc ⇒ `from_html` ; (2) garde-fou : si la longueur du texte issu du HTML (paragraphes joints par une espace, en caractères) × 10 < 9 × la longueur de `plain` replié (blancs repliés, `trim`, en caractères), alors `plain` est préféré ; (3) sinon `from_plain(plain)` si le HTML est absent ou blanc ; (4) aucun des deux ⇒ `LongText::default()`. Un résultat qui dépasse `MAX_CHARS` ⇒ `TextTooLarge { chars }` (comptage en `u64`, jamais de troncature).

- [ ] **Step 1 : tests** (rouges). Invariants : `canonical_is_paragraphs_joined_by_blank_line`, `no_paragraph_is_empty_or_contains_newline`, `char_len_counts_chars_not_bytes` (accents, emoji), `paragraphs_report_char_starts` (`char_start` croissants, cohérents avec `char_len` et `\n\n`), `text_length_counts_chars_of_canonical_text` : **CONSTIT = 14 332** (14 236 + 2 × 48), `from_plain_splits_on_newline_runs` et `from_plain_without_newline_is_one_paragraph` (le `texte` de l'API n'en a pas, D-26), `best_of_prefers_html_when_present`, `best_of_falls_back_to_plain_below_90_percent` (html 3 caractères, plain 10 ⇒ plain), `best_of_keeps_html_at_exactly_90_percent` (9 contre 10 ⇒ html), `best_of_constit_keeps_html` (99,8 % : fixture), `best_of_empty_inputs_give_empty` (`(None,None)`, `(Some(" "),Some(""))`), `long_text_over_the_limit_is_text_too_large` (16 000 001 caractères de `a` ⇒ `TextTooLarge { chars: 16_000_001 }`, 16 000 000 accepté), `long_text_counts_removed_invisibles` (`<p>a\u{202E}b</p>` ⇒ `invisible_removed() == 1`, `as_str() == "ab"`), `debug_never_prints_the_text`, `clone_shares_the_buffer` (`Arc::ptr_eq` via un accesseur `pub(crate)` ou comparaison de `as_str().as_ptr()`), `verbatim_text_is_unchanged_except_markup` (`fold_ws(texte)` = `fold_ws(canonical)` + ECLI pour les 3 fixtures de texte).
- [ ] **Step 2 : implémenter.** Un constructeur privé `fn assemble(paras: Vec<String>) -> Result<LongText, DomainError>` qui calcule les offsets et applique la limite ; `from_html` / `from_plain` ne font que préparer les paragraphes ; la décision du garde-fou dans `fn html_is_complete(html_chars: usize, plain_chars: usize) -> bool` (privée, testée à la frontière). `Default` et `new`-like : aucune logique.
- [ ] **Step 3 : fuzz.** Cible `from_html` : `LongText::from_html(data)` ne panique pas, `canonical == paragraphs.join("\n\n")`, aucun `\n` dans un paragraphe ; ajouter `[[bin]] name = "from_html"` à `fuzz/Cargo.toml` ; `cargo check --manifest-path fuzz/Cargo.toml --locked`.
- [ ] **Step 4 : vert + qualité.** `make cov`, `make mutants FILE=crates/lf-domain/src/text/long_text.rs`.
- [ ] **Step 5 : `make ci`** puis commit `feat(domain): LongText, best_of et garde-fou 90 % (03 §7.5)`.

---

### Task 7 : `Snippet`, `SearchHit::new` et `SearchPage` complète

03 §4.1, §7.3 (Snippet) ; 04 §9.2 (ligne `SearchPage`). Les types de la suite (`DocumentId`, `LegalStatus`, `Idcc`) viennent des tâches 2 et 3.

**Files:**
- Modify: `crates/lf-domain/src/contract/text.rs` (`cut_at_word` paramétré), `crates/lf-domain/src/entity/search.rs`, `crates/lf-domain/src/entity/mod.rs` (exports, `contracts()`), tous les appelants que le compilateur désigne (tests de `entity/mod.rs`, `lf-application`)

**Interfaces:**
- Consumes : `DocumentId`, `Fond::hit_prefix`, `HitKindMismatch` (tâche 3) ; `LegalStatus`, `Idcc` (tâche 2) ; `clean` (tâche 4) ; `CivilDate`, `PaginationMode`.
- Produces :
  - `contract/text.rs` : `pub(crate) fn cut_at_word(text: &str, max_chars: usize) -> Option<String>` (même algorithme qu'aujourd'hui : coupe à la dernière espace des `CUT_WINDOW_CHARS` = 40 derniers caractères, sinon coupure franche, puis `…` **inclus dans** `max_chars`) ; l'appel de la citation passe `CITATION_MAX_CHARS`.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Snippet(String)` ; `Snippet::new(raw: &str) -> Option<Self>` (`clean`, vide ⇒ `None`, ≤ `SNIPPET_MAX_CHARS` = 280 caractères avec `…` si coupé ; les `[...]` de l'API sont conservés) ; `as_str(&self) -> &str`. La logique dans `fn snippet_text(raw: &str) -> Option<String>` privée.
  - `CodeArticleRef { article_id: ArticleId, num: Option<String>, snippet: Option<Snippet> }` ; `CodeArticleRef::new(article_id, num, raw_snippet: &str)` construit `Snippet::new(raw_snippet)`.
  - `SearchHit { fond: Fond, id: DocumentId, version_start: Option<CivilDate>, cid: Option<DocumentId>, title: Option<String>, date: Option<CivilDate>, nature: Option<String>, etat: Option<LegalStatus>, snippet: Option<Snippet>, idcc: Option<Idcc>, raison_sociale: Option<String>, articles: Vec<CodeArticleRef> }` (champs publics ; ordre à respecter pour la lisibilité).
  - `SearchHit::new(fond: Fond, id: DocumentId, version_start: Option<CivilDate>) -> Result<Self, DomainError>` : autres champs vides/`None` ; le préfixe est contrôlé par `fn check_prefix(fond: Fond, id: &DocumentId) -> Result<(), DomainError>` (privée) : `id.prefix() != fond.hit_prefix()` ⇒ `HitKindMismatch`. Le mapper remplit ensuite les champs publics.
  - `SearchPage { …existants…, total_articles: Option<u64>, dropped: u32 }` ; `SearchPage::new(fond: Fond, query: &str, total: Option<u64>, total_articles: Option<u64>, hits: Vec<SearchHit>, dropped: u32, pagination: PaginationMode) -> Self` ; `total_articles` ne retient que `> 0` (`fn positive_total(n: Option<u64>) -> Option<u64>` privée) ; `total` inchangé (`totalResultNumber`, sinon `hits.len()`).
  - `Citable for SearchHit` : mêmes règles qu'aujourd'hui sur `DocumentId` (`as_str()`).

- [ ] **Step 1 : tests** (rouges). `snippet_is_truncated_to_280_chars` (300 caractères avec espaces ⇒ ≤ 280 se terminant par `…`, coupé à une espace ; 280 pile ⇒ inchangé sans `…`), `snippet_cuts_hard_without_a_space_in_the_window`, `snippet_keeps_api_ellipsis_brackets` (`[...] texte [...]`), `snippet_strips_marks_and_blank_is_none`, `cut_at_word_respects_the_bound_it_is_given` (bornes 10 et 280), `search_hit_new_accepts_the_right_prefix_per_fond` (les 8 fonds, via `Fond::ALL`), `invalid_hit_is_rejected_with_kind_mismatch` (partie domaine de `invalid_hit_is_dropped_and_counted` : `CNILTEXT…` pour `Fond::Juri`, `LEGIARTI…` pour `Fond::Loda`), `search_hit_new_keeps_version_start`, `search_page_total_articles_keeps_only_positive` (`Some(0)` ⇒ `None`, `Some(3)`), `search_page_carries_dropped`, tests existants de `Citable` mis à jour.
- [ ] **Step 2 : rouge, implémenter, corriger les appelants.** `cargo clippy --workspace --all-targets` guide (les tests de `lf-application` qui construisent un `SearchHit`/`SearchPage`). Mutation : `…/contract/text.rs` et `…/entity/search.rs`.
- [ ] **Step 3 : `make ci`** puis commit `feat(domain): Snippet, SearchHit::new et SearchPage complète (D-30, D-33, E-26)`.

---

### Task 8 : `OfficialSummary`

03 §4.4. Entité autonome ; le répartiteur par fond est du mapper (plan 2f).

**Files:**
- Create: `crates/lf-domain/src/entity/summary.rs`
- Modify: `crates/lf-domain/src/entity/mod.rs` (`mod summary; pub use summary::OfficialSummary;`)

**Interfaces:**
- Consumes : `clean` (tâche 4), `text::fold` (vue repliée : casse, diacritiques, blancs ; relire l'API de `text/fold.rs`).
- Produces : `#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct OfficialSummary { pub analyses: Vec<String>, pub titrages: Vec<String> }` ; `OfficialSummary::new<A, T>(analyses: A, titrages: T) -> Self where A: IntoIterator, A::Item: AsRef<str>, T: IntoIterator, T::Item: AsRef<str>` ; logique dans `fn normalize_entries(entries: impl Iterator<Item = impl AsRef<str>>) -> Vec<String>` privée : chaque entrée passe par `clean`, ses sauts de ligne deviennent une espace, le **code de classement de tête** est retiré (`^\d{2}(-\d{2,3})*\s+` ou `^CETAT\d{2}(-\d{2,3})*\s+`, scanners manuels, **seulement si le reste commence par une majuscule**), les entrées vides sont écartées, les doublons supprimés sur la vue repliée (première occurrence gardée, ordre conservé).

- [ ] **Step 1 : tests** (rouges). `summary_cetat_official_analysis` (3 analyses `"01-03-02-02 Il résulte …"` identiques ⇒ 1 entrée sans code), `summary_strips_cetat_prefixed_titrage` (`"CETAT01-03-02-02 ACTES LÉGISLATIFS"` ⇒ `"ACTES LÉGISLATIFS"`), `summary_strips_two_digit_and_dashed_codes` (`"44 Compétence"`, `"68-01-002 Pour"`), `summary_keeps_a_leading_number_before_lowercase_text` (`"15 jours après"` ⇒ inchangé), `summary_br_becomes_a_space` (`"a<br/>b"` ⇒ `"a b"`), `summary_dedupes_on_folded_view` (casse et accents ignorés), `summary_drops_blank_entries`, `summary_keeps_order`, `summary_new_with_empty_inputs_is_default`.
- [ ] **Step 2 : rouge, implémenter, vert.** `make mutants FILE=crates/lf-domain/src/entity/summary.rs`.
- [ ] **Step 3 : `make ci`** puis commit `feat(domain): OfficialSummary (03 §4.4)`.

---

### Task 9 : textes consolidés — `TextBody`, `Act`, `Convention` et table des matières

03 §4.2, §4.6 ; 04 §9.2 (lignes `Act`, `Convention`). Consomme les tâches 2, 4 et 6.

**Files:**
- Modify: `crates/lf-domain/src/entity/act.rs`, `crates/lf-domain/src/entity/toc.rs`, `crates/lf-domain/src/entity/mod.rs` (`contracts()` et exports), appelants désignés par le compilateur

**Interfaces:**
- Consumes : `JorfTextId`, `Nor`, `Idcc`, `LegalStatus` (+ `LegalStatus::parse`), `LongText`, `clean_field`.
- Produces :
  - `TextBody { cid: Option<JorfTextId>, nor: Option<Nor>, title: Option<String>, nature: Option<String>, etat: Option<LegalStatus>, date_parution: Option<CivilDate>, num_parution: Option<String>, date_debut_version: Option<CivilDate>, date_fin_version: Option<CivilDate>, jo_reference: Option<String>, aliases: Vec<String>, toc: Toc }` (`Default` dérivé).
  - `Convention { text_id: KaliTextId, body: TextBody, idcc: Option<Idcc>, container_title: Option<String>, container_id: Option<String>, bulletin: Option<String> }`.
  - `Act::contract` : URL `ResourceKind::Code` si `nature` est « CODE » (insensible à la casse), `Loda` sinon ; citation = titre officiel nettoyé, inchangée.
  - `ArticleDraft` gagne `content_html: Option<String>` (le brouillon reste brut : `etat` en `String`) ; `TocArticle { article_id, num, etat: Option<LegalStatus>, inline_text: Option<LongText> }` ; `TocSection.etat: Option<LegalStatus>`. `Toc::build` convertit `etat` par `LegalStatus::parse`, construit `inline_text = LongText::from_html(content_html)` seulement si le contenu est présent et non blanc (un texte vide après nettoyage ⇒ `None`), **propage `TextTooLarge`** (la signature `Result<Toc, DomainError>` existe déjà), et remplace `cleaned` par `text::clean_field`.

- [ ] **Step 1 : tests** (rouges). `act_with_nature_code_uses_the_code_url` (nature `"code"` et `"CODE"` ⇒ `/codes/texte_lc/`, `"LOI"` ⇒ `/loda/id/`), `text_body_default_is_empty`, `toc_article_keeps_inline_text_from_content_html` (`kali_inline_content_is_available_without_extra_call`, côté domaine : `content_html = "<p>Art. 1</p><p>Alinéa</p>"` ⇒ `inline_text.paragraphs()` à 2), `toc_article_without_content_has_no_inline_text`, `toc_blank_content_gives_no_inline_text`, `toc_status_is_parsed` (`"VIGUEUR"` ⇒ `Vigueur`, `""` ⇒ `None`, `"INCONNU"` ⇒ `Other`), `toc_propagates_text_too_large` (contenu de 16 000 001 caractères), tests de `Toc` existants (ordre, dédoublonnage, profondeur 64, `Toc::page`) **inchangés dans leurs assertions** ; `convention_carries_idcc_container_and_bulletin`.
- [ ] **Step 2 : rouge, implémenter, corriger les appelants.** Mutation : `…/entity/act.rs` et `…/entity/toc.rs`.
- [ ] **Step 3 : `make ci`** puis commit `feat(domain): Act, Convention et TOC complets (statut, NOR, IDCC, texte en ligne)`.

---

### Task 10 : `ArticleText`, `ParentText` et la citation d'un article de loi (Q12)

03 §4.3, §5.4. Consomme les tâches 2, 3, 4 et 6.

**Files:**
- Modify: `crates/lf-domain/src/entity/article.rs`, `crates/lf-domain/src/contract/rules.rs`, `crates/lf-domain/src/entity/mod.rs`, appelants désignés par le compilateur

**Interfaces:**
- Consumes : `DocumentId`, `LegalStatus`, `LongText`, `article_citation`.
- Produces :
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ParentNature { Code, Text, Kali, Unknown }` ; `#[derive(Debug, Clone, PartialEq, Eq)] pub struct ParentText { pub id: DocumentId, pub nature: ParentNature }` ; `ParentText::new(id: DocumentId, nature_raw: Option<&str>) -> Self` (logique dans `fn parent_nature(id: &DocumentId, raw: Option<&str>) -> ParentNature` privée) : préfixe `KALITEXT` ⇒ `Kali` ; sinon `raw` (insensible à la casse, `_`, `-` et espaces repliés) : `CODE` ⇒ `Code` ; `LOI`, `LOI ORGANIQUE`, `LOI CONSTITUTIONNELLE`, `ORDONNANCE`, `DECRET`, `DECRET LOI`, `ARRETE` ⇒ `Text` ; tout le reste ou `None` ⇒ `Unknown` (valeurs réelles **NON VÉRIFIÉ** : V-142).
  - `ArticleText { article_id: ArticleId, num: Option<String>, etat: Option<LegalStatus>, code_title: Option<String>, parent: Option<ParentText>, path: Vec<String>, full_sections_titre: Option<String>, date_debut: Option<CivilDate>, date_fin: Option<CivilDate>, text: LongText, nota: Option<LongText> }`.
  - `contract/rules.rs` : `pub fn article_citation_for(parent: ParentNature, code_title: Option<&str>, num: Option<&str>) -> Option<String>` — pour `ParentNature::Text`, retire du titre l'appel de note final ` (n)` (espace, `(`, un ou plusieurs chiffres, `)`, en fin de titre après `trim`) puis applique `article_citation` ; autres natures : `article_citation` inchangée. `fn strip_note_call(title: &str) -> &str` privée, scanner manuel. `article_citation` (exposée) ne change pas. `Citable for ArticleText` : `article_citation_for(self.parent.map_or(Unknown, |p| p.nature), …)` ; **URL toujours `code_article` pour `Legi`** (V-140 et V-142 non levées), `/conv_coll/id/` pour `Kali`.

- [ ] **Step 1 : tests** (rouges). `parent_nature_per_prefix_and_label` (table : `CODE`, `code`, `Loi`, `DECRET-LOI`, `ORDONNANCE`, `ARRETE`, `"???"`, `None`, préfixe `KALITEXT`), `law_article_citation_drops_the_note_call` (`"LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)"` + num `"1"` ⇒ `"LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique, art. 1"`), `code_article_citation_is_unchanged` (`"Code civil"`, `"9"` ⇒ `"Code civil, art. 9"`), `note_call_only_stripped_at_the_end_and_for_digits` (`"Titre (bis)"`, `"Texte (1) suite"` inchangés ; `"Texte (12)"` ⇒ `"Texte"`), `unknown_parent_nature_falls_back_to_codes_url`, `article_url_depends_on_id_type_not_parent` (`Legi` ⇒ `/codes/article_lc/`, `Kali` ⇒ `/conv_coll/id/`), `article_text_carries_path_nota_and_parent`.
- [ ] **Step 2 : rouge, implémenter, corriger les appelants.** Mutation : `…/entity/article.rs`, `…/contract/rules.rs`.
- [ ] **Step 3 : `make ci`** puis commit `feat(domain): ArticleText complet, ParentText et citation d'un article de loi (Q12)`.

---

### Task 11 : `Decision`, `Deliberation`, `CompanyAgreement` et documentation

03 §4.4, §4.5, §4.7. Dernière tâche : documents et mise à jour de l'état livré.

**Files:**
- Modify: `crates/lf-domain/src/entity/{decision,deliberation,agreement}.rs`, `crates/lf-domain/src/entity/mod.rs`, `docs/plan/03-domaine.md`, `docs/plan/00-decisions.md` (§11.1 : « Faits au plan 2e »), `docs/plan/07-feuille-de-route.md` (plans 2e à 2h ; T-M2-13 passe en M4 ; T-M2-07 « domaine livré, mapping au plan 2f »)

**Interfaces:**
- Consumes : `LongText`, `OfficialSummary`, `LegalStatus`, `Idcc`.
- Produces : `Decision.text: LongText` et `Decision.official_summary: OfficialSummary` (les autres champs inchangés) ; `Deliberation.etat: Option<LegalStatus>` et `Deliberation.text: LongText` ; `CompanyAgreement.idcc: Option<Idcc>`.

- [ ] **Step 1 : tests** (rouges) sur les contrats existants adaptés aux nouveaux types (`url_depends_on_id_type_not_payload_origine` : le préfixe décide de l'URL, jamais `origine`), `decision_carries_official_summary_and_long_text`, `deliberation_status_and_text_types`, `agreement_idcc_is_typed` ; les citations restent inchangées (CONSTIT `Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025`, CNIL `CNIL, délibération n° SAN-2026-002 du 8 janvier 2026`, ACCO `{titre} ({raison sociale})`).
- [ ] **Step 2 : rouge, implémenter, corriger les appelants.** Mutation : un fichier à la fois (`decision.rs`, `deliberation.rs`, `agreement.rs`).
- [ ] **Step 3 : documentation.** `03-domaine.md` : retirer les mentions périmées (`open_ended`, `TocNode`, contrat v2, `date.rs:91`, `act.rs:41` encore « à faire » alors que le code les a ; §0, §2.4, §4.2, §5, §6.2), décrire l'état livré de `LegalStatus`, `Nor`, `Idcc`, `DocumentId`, `LongText` (faillible), `Snippet`, `OfficialSummary`, `ParentText`, ajouter l'écart `hit_prefix -> &'static str` et `Idcc(u16)`. `00-decisions.md` §11.1 : « Faits au plan 2e » (liste ci-dessus). `07-feuille-de-route.md` : le découpage 2e/2f/2g/2h et le déplacement de T-M2-13 en M4 (`lf-app` naît en M4). Q12 : noter « appliquée par défaut dans le code au plan 2e ; réponse du propriétaire toujours attendue ».
- [ ] **Step 4 : clôture.** `make ci` complet, `cargo +1.88 check --workspace --all-targets --locked`, `make min-age`, `cargo xtask check-arch`. Commit `feat(domain): Decision, Deliberation, CompanyAgreement complets et documentation du plan 2e`.
