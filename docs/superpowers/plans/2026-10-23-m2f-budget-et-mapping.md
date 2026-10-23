# M2f Budget de décodage et mapping — plan d'implémentation (R11, R13, D-24, `map/`)

> **Origine.** Plan 2f, écrit le 2026-10-23 sur `plan/m2f-mapping` (worktree `/home/muchini/mcp-ligitfrance-m2f`), base `feat/m2e-domaine` @ `bc142c1` (plan 2e exécuté, non poussé). Sept sections ont été rédigées en parallèle (budget, infrastructure, recherche, textes consolidés, article, décisions, accord et contrats), chacune mesurée sur les 16 fixtures par `python3 -I` (scripts `/var/tmp/m2f-plan/*.py`, non versionnés), puis intégrées ici : noms et signatures alignés, contradictions tranchées (« Rulings d'intégration »), fichiers et symboles du domaine relus par `grep` sur `bc142c1`. Les valeurs de fixtures sont des mesures Python : **le test Rust est l'oracle** ; une valeur différente au vert est rapportée au ledger, jamais forcée dans le code.
>
> **Préalables (hors plan, sur `feat/m2e-domaine`)** : Mi-1 corrigé (flake du proptest `status.rs:276`), sinon `make test` échoue au hasard. `$WT` désigne le worktree d'exécution (branche `feat/m2f-mapping` créée depuis `plan/m2f-mapping`). Ledger : `$WT/.superpowers/sdd/2026-10-23-m2f-budget-et-mapping/progress.md` (git-ignoré).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** borner le décodage des réponses PISTE (budget de 200 000 éléments par document, `results` ≤ 100 puis ≤ `page_size`, `typePagination` et `num` tolérants, cible de fuzz `dto_decode`), puis écrire `map/` : les sept fonctions pures DTO → domaine (`search_page`, `act`, `convention`, `article`, `decision`, `deliberation`, `agreement`), leurs diagnostics comptés et journalisés, et les tests de contrat qui ne dépendent que des fixtures.

**Architecture :** tout vit dans `lf-adapter-piste`, en `pub(crate)`, sans E/S. Le budget est un état thread-local armé par `decode` et remis à zéro par une garde `Drop` ; chaque liste de DTO passe par `de::bounded_list` (ou `capped_list`). `map/mod.rs` (`Diagnostics`, `Field`, `endpoint`), `map/dates.rs` (règle e) et `map/rules.rs` (règles a, b, c, d, f) sont partagés par un fichier par mapper ; `map/fixtures.rs` et `src/view.rs` (`cfg(test)`) sont le chargeur de fixtures et la projection uniques. Rien n'est branché : `impl LegalCorpus` (plan 2g) consommera `map::*` et appellera `Stats::record_mapping`. Deux retouches du domaine : `TextBody.cid` (Task 18) et `DecisionId::fond` (Task 25).

**Tech Stack :** Rust 2024 (MSRV 1.88, toolchain 1.99.0) ; `serde` 1.0.229, `serde_json` 1.0.154, `serde_path_to_error` 0.1.20, `tracing` (déjà au verrou) ; `cargo nextest`, `cargo llvm-cov`, `cargo-mutants` 27.1.0 ; `cargo-fuzz` 0.13.1 sous nightly (essai court local, matrice hebdomadaire).

**Spec :** `docs/plan/04-adapter-piste.md` §6.6, §8.2 à §8.5, §9.1 à §9.3, §10.2, §10.5, §13.3 ; `docs/plan/03-domaine.md` §2.4, §2.5, §4.0 à §4.7, §7.3 ; `docs/plan/06-qualite.md` §1.4, §2.3, §3.1 (D-20 à D-35) ; `docs/plan/07-feuille-de-route.md` T-M2-06, T-M2-07, T-M2-09 critères (1) et (2), T-M2-10 ; `docs/plan/annexes/A-parite.md` §4.7, §5.4, P-032, E-26 ; `docs/plan/00-decisions.md` §11.1.

---

## Global Constraints

- Rust edition 2024, `rust-version = "1.88"` : `cargo +1.88 check --workspace --all-targets --locked` vert en fin de chaque PR ; toolchain `1.99.0`.
- Lints : `clippy::pedantic` et `unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`, `dbg_macro` sous `-D warnings` (`make lint` = `clippy --workspace --all-targets --all-features --locked`) ; `unwrap`/`expect` permis dans les tests seulement ; `missing_docs`, `unreachable_pub`, `unsafe_code = forbid`. Docs et commentaires en français, identifiants en anglais, messages d'erreur et de journal en anglais, sans point final.
- Couverture : `make cov` = 100 % des lignes, régions et fonctions (`--lib`, code `cfg(test)` compris) ; chaque branche `Option`/`Result` a son cas ; jamais `assert!(matches!(..))` sur un bras jamais pris ; jamais de test « ignoré si le fichier manque ».
- Dépendances : **aucune nouvelle dépendance au workspace** (`Cargo.lock` inchangé, `make min-age` vert, règle d'âge de 14 jours) ; seul `fuzz/Cargo.toml` gagne `lf-application`, `serde`, `serde_json`, `serde_path_to_error` (Task 8), figés aux versions déjà dans `Cargo.lock`. `insta`, `sha2` et `proptest` dans l'adapter : plan 2g.
- Architecture : `cargo xtask check-arch` (`make arch`, R1 à R9) vert ; `lf-domain` reste pur (seule dépendance normale `thiserror`) et ne change qu'en Tasks 18 et 25.
- Aucune valeur du corps dans un `Debug`, un `Display`, un `detail` ou un journal : `Decode { detail }` = `<endpoint>: <chemin>: <catégorie>`, chemin fait de noms de champs déclarés et d'indices (ou `[]`), catégorie à texte fixe ; jamais le `Display` d'une `DomainError` (il recopie jusqu'à 64 caractères de l'entrée).
- Lecture des DTO : par `dto::decode::decode` (qui arme le budget) ou par un DTO construit à la main. Un `serde_json::from_str` sur un DTO à listes échoue (`decoding budget not armed`), sauf sous `budget::arm()` explicite (Task 6). Fixtures lues par `include_str!` dans `map/fixtures.rs` seulement (Task 11), jamais reformatées ni réparées sur disque.
- Aucune décision dans une `fn new` ni dans un `impl Default` (cargo-mutants ne les mute pas) : toute règle vit dans une fonction privée nommée autrement, testée directement.
- WSL (`~/.claude/rules/wsl-safety.md`) : jamais `cargo test -j 4` ni plus ; temporaires sous `/var/tmp` (jamais `/tmp`, tmpfs en RAM) ; un seul texte de 16 Mio vivant à la fois dans un test ; **aucune mutation avant la Task 33**.
- `make -k ci` à chaque tâche : seule l'étape `deny` peut rester rouge, et seulement pour RUSTSEC-2026-0386 (`tokio` 1.53.2, Ruling 10 du plan 2e, montée par PR séparée) ; dès que le workspace monte `tokio`, `fuzz/Cargo.lock` suit.
- Commits : un par tâche, message conventionnel en français, sans trailer ajouté par l'implémenteur (le contrôleur ajoute `Co-Authored-By`) ; rien n'est poussé ; le propriétaire ouvre et merge les PR.

## Review Focus

1. **Valeur hostile dans un détail** : une sonde (`sekret-42-Ω`, `SECRET-VALUE-0001`, `TOPSECRET`) dans un identifiant, une date, un `cid` ou un élément refusé n'apparaît dans aucun `detail`, `Debug` ni journal (Tasks 2, 3, 10, 14, 19, 24, 27, 28, 29).
2. **Corps de 600 Ko qui gonfle à 100 Mo** : 200 000 éléments passent, le 200 001e échoue avec son chemin ; budget remis à zéro après succès, erreur et panique, sans fuite d'un fil à l'autre (Tasks 1, 2, 6, sonde).
3. **Page `/search` hors contrat** : 101 résultats ou plus que `page_size` ⇒ `Decode` ; `typePagination` numérique ou objet ⇒ `None`, jamais d'échec (Tasks 3, 4, 15).
4. **Identifiant renvoyé ≠ demandé, absent, suffixé ou d'une autre famille** : même règle dans les sept mappers (gardé, demandé + `id_mismatch`, suffixe compté, ou `Decode`) (Task 14 et un test de câblage par mapper).
5. **Dates aux limites** : sentinelles 2999/2222 ⇒ `None`, texte illisible ⇒ `Decode` (jamais `None`), horodatage hors minuit compté, `i64::MIN`/`MAX` sans panique (Task 11).

---

## Décisions de cadrage

Les 14 rulings du plan 2e restent valables. **[P]** = engage le propriétaire ; les autres lignes sont des rulings du plan, annulables. Le contrôleur reporte tout écart constaté au ledger.

### Décisions reprises des sections (dédupliquées)

| # | Décision | Source | Si elle est fausse |
|---|---|---|---|
| D1 | Budget de **200 000 éléments par document**, exact (200 000 passe, 200 001 échoue), sans surcoût par liste ; charge dans une graine `DeserializeSeed` **avant** la lecture de l'élément (le chemin porte l'indice refusé, l'élément n'est jamais construit) ; compteur non armé = refus `decoding budget not armed`, jamais illimité | S0 | sonde > 160 Mio : budget à 100 000 ou 4 places facturées par liste, refaire Tasks 1 et 6 |
| D2 | Un seul texte `too many elements` (plafond de 100 de `results` et budget) ; `MAX_RESULTS = 100` en dur, relié à `PageSize::MAX` par un test (`as usize` en `const` lève `cast_possible_truncation`) | S0 | deux textes |
| D3 **[P]** | `results` > `page_size` au mapping ⇒ `Decode` de toute la page (`results: more results than page size`, `ensure_fits`, `==` accepté) | S0, S2 | tronquer + diagnostic si V-82/V-83 montrent des pages trop longues en mode `ARTICLE` |
| D4 **[P]** | **Pas de plafond de corps par endpoint en M2** : le budget borne la mémoire des DTO ; un seuil sans corps réels (V-75, V-96) serait deviné et un faux positif casserait une consultation | S0 | 4 Mio (`/search`, `getArticle`, `juri`, `cnil`), 16 Mio ailleurs : une ligne dans `Attempt`, après les sondes |
| D5 | `null_default` ne sert plus aucune liste ; un balayage des sources compte 25 listes bornées et 2 `null_default` (structs `ArticleDto.context`, `JuriTextDto.decision_attaquee`) | S0 | relecture seule |
| D6 | R13 : `typePagination` par `opt_string_or_number` ; nombre ⇒ chaîne (inconnue, pas de mémo) ; autre type, vide ou blanc ⇒ `None` | S0 | `Option<String>` strict : une page perdue |
| D7 **[P]** | Cible de fuzz `dto_decode` **en 2f**, par inclusion `#[path]` des `dto/*.rs` dans `fuzz/`, sans feature ni `#[doc(hidden)]` ; ajoutée à la matrice de `weekly.yml` | S0 (RI-15) | renoncer, justifier en 06 §1.4 si `make deny` refuse le graphe |
| D8 | Pic mémoire mesuré par une sonde hors dépôt (écart à 04 §8.5 « binaire de test dédié » : 06 §1.4 réserve `tests/`) ; aucune garde du pic en CI | S0 | — |
| D9 **[P]** | D-24, option mixte : `num` et `numParution` (8 champs) lus en chaîne ou en nombre, type étranger ⇒ `None` ; `numeroAffaire` reste une liste stricte (scalaire ⇒ `Decode`) ; 06 amendé | S6 P9 | amender 06 seul : un entier ferait perdre le document |
| D10 | `civil(raw, Field, diags)` : convertir, compter l'horodatage, **puis** `open_ended` ; illisible ⇒ `Decode` `unreadable date` ; année hors 1..=9999 ⇒ `Decode` ; chemins statiques, indices écrits `[]` (aucune allocation par date) | S1 | `None` pour une date illisible : perte silencieuse |
| D11 | Fuseau jamais appliqué : seuls `+0000`, `+00:00`, `Z` ou absent sont UTC ; autre fuseau ou heure ≠ minuit ⇒ `non_midnight_timestamps`, partie date gardée (NON VÉRIFIÉ, V-100) | S1 | arithmétique de fuseau |
| D12 | Deux sélections de date : `first_civil` (recherche : premier candidat présent hors sentinelle, paresseux) et `prefer_computed` (décisions : un `…Computed` présent l'emporte même s'il donne `None`) | S1, S5 D-9 | `first_civil` partout |
| D13 | Identifiant absent ou blanc dans un objet rempli ⇒ demandé + `id_mismatch` (pas de `missing_id`) ; lu ≠ demandé ⇒ lu gardé + `id_mismatch` ; illisible ⇒ `Decode` `invalid identifier` | S1, S3, S4, S5, S6 | — |
| D14 | Neuf compteurs : les sept du brief, `invalid_nor` (03 §4.2) et `toc_dropped` (S3) ; `warn` pour `id_mismatch`, `non_midnight_timestamps`, `origin_mismatch`, `debug` sinon ; jamais d'identifiant en journal ; `record_mapping` une fois par mapping, y compris sur `Err` (imposé au plan 2g) | S1, S3 | — |
| D15 | `check_origin` ne contrôle que sept familles mesurées (`LEGITEXT`/`LEGI` … `ACCOTEXT`/`ACCO`), jamais `JORFTEXT`, `LEGIARTI`, `KALIARTI`, et ne route jamais | S1 | `warn` parasite |
| D16 | `Field::domain` : cinq catégories fixes (`invalid identifier`, `invalid date`, `text too large`, `table of contents too deep`, `invalid value`) | S1 | — |
| D17 **[P]** | Invisibles : seuls les **textes longs** alimentent `invisible_chars_removed` (`LongText::invisible_removed()`, textes en ligne de la TOC) ; champs courts (`clean_field`), extraits et sommaires les retirent sans les compter ; « partout » exigerait de changer `Snippet::new`, `OfficialSummary::new` et `Toc::build` (domaine) | S1, S3-D8, S4, S5 D-10 (RI-14) | compter partout : trois API du domaine et une aide `map::short` |
| D18 | `search_page_with_mode` fait le travail et rend le mode reçu (R12 : `PaginationMemo::remember` exige l'`Option`) ; `search_page` n'est que l'enveloppe (retirée ou `cfg(test)` en 2g) | S2 | 2g relit `typePagination` (second `warn!`) |
| D19 | Hit : date = `first_civil(date, datePublication, dateSignature)` ; extrait = `text`, sinon premier `values[0]` non vide ; articles CODE (`Legi`, `num` nettoyé) et KALI (`Kali`, `num` `None`) seulement, extraits invalides ignorés sans compteur ; `cid` invalide ⇒ `None` ; `nature`, `etat`, `idcc`, `raison_sociale` pour tous les fonds ; `total` = `totalResultNumber` (même `0`), sinon hits gardés. **Écart de la clôture (Ruling 8)** : hors LODA et CODE, où le contrat fait du `cid` la clé de l'URL, un `cid` d'une autre famille que l'`id` est omis (`None`, sans compteur) ; le suffixe hors `LEGITEXT` d'un hit, d'un `cid` ou d'un article **gardés** est compté (04 §9.1 c, §9.2) | S2 | compteur `dropped_articles` |
| D20 **[P]** | `TextBody.cid` devient `Option<DocumentId>` (rouvre `lf-domain` après le plan 2e) : le `cid` d'un code est un `LEGITEXT`, celui d'une convention son `KALITEXT` | S3-D1 | `JorfTextId` : `map::act` d'un code échouerait |
| D21 **[P]** | Identifiant secondaire illisible ⇒ `Decode` de toute la consultation : `cid` d'un texte, `cid`/`id` du texte parent d'un article | S3-D7, S4 | `None` + compteur (trois lignes) si le live montre des `cid` d'autres familles |
| D22 | `date_debut_version` = date du suffixe `_JJ-MM-AAAA` (après `open_ended`), sinon `dateDebutVersion` ; les trois dates d'un texte sont toujours lues, ordre figé `cid`, `dateParution`, `dateDebutVersion`, `dateFinVersion`, puis TOC | S3-D4 | — |
| D23 | NOR illisible ⇒ `None` + `invalid_nor` ; IDCC illisible ⇒ `None` sans compteur ; `etat` inconnu ⇒ `LegalStatus::Other` | S3, S6 P5 | compteur `idcc_unreadable` |
| D24 | Convention : `conteneurs[0]` seul ; `container_id` = `cid` rogné ; `bulletin` = `originePubli`, sinon `numeroBo` ; `content_html = content` sans condition ; `textAbroge` non lu ; pas de garde de profondeur avant `Toc::build` (serde plafonne à 61-63, `TocTooDeep` inatteignable par JSON) | S3 | — |
| D25 | `map::act(dto, &LegiTextId, &mut Diagnostics)` : le `VersionDate` de 04 §9.2 est retiré (aucune entité ne le porte) | S3 | — |
| D26 | Article : `code_title` et `parent` lus sur la **même** entrée de `textTitles` (première de nature `CODE`, casse et blancs ignorés, sinon la première) ; parent = `cid`, sinon `id` ; `etat` par `LegalStatus::parse` ; `nota` vide ⇒ `None` | S4 | — |
| D27 | Variante golden KALIARTI : l'entité garde `LEGIARTI000006419288` (renvoyé), `id_mismatch` = 1 ; divergence `returned_id_kept` (annexe A §4.7, Golden = oui) ; le harnais de 2g rejoue la fixture non modifiée | S4, S6 P10 | patcher `article.id` comme `golden.py:317` |
| D28 | `DecisionId::fond()` (`pub const fn`) ajoutée au domaine ; le fond de la répartition du sommaire et du contrôle d'`origine` est celui de l'identifiant **renvoyé** | S5 D-1, D-3 | `match` local |
| D29 | Sommaire : JURI `abstrats` ⇒ analyses, `resumePrincipal` puis `autreResume` ⇒ titrages ; CETAT `resumePrincipal` ⇒ analyses, `abstrats` puis `autreResume` ⇒ titrages (`autreResume` CETAT non attesté, V-109) ; CONSTIT et CNIL ignorés ; `titrages` de premier niveau (nomenclature) jamais lu | S5 D-7, D-8 | ignorer `autreResume` CETAT |
| D30 | `title` sans repli sur `titreLong` ; `numero_affaire` nettoyé, sans dédoublonnage, repli `[num]` pour CETAT seul (V-112) ; `decisionAttaquee` non mappée ; `Deliberation` ignore `ecli`, `numeroAffaire`, `sommaire`, `juridiction`, `formation`, `solution` ; `Deliberation.etat = status_of(etat, None)` | S5 | — |
| D31 | ACCO : `dateTexte`, `dateEffet`, `dateDepot` converties, `dateFin` et `dateDiffusion` jamais lues ; `themes`/`syndicats` = libellés nettoyés, vides écartés, ni tri ni dédoublonnage ; six champs DTO non lus (2g) | S6 P3, P4 | — |
| D32 | Tests de contrat dans `src/` sous `cfg(test)` (`map/fixtures.rs`, `view.rs`, plus tard `golden.rs`) ; `tests/fixtures_contract.rs` et `tests/parity_golden.rs` (T-M2-10) abandonnés (`make cov` mesure `--lib`) ; projection unique `view`, exhaustive ; `every_normalized_fixture_has_contract` structurel, valeurs du golden en 2g | S6 P7, P8, P10, P11 | `tests/` : couverture aveugle |

### Rulings d'intégration (contradictions entre sections)

| # | Les sections disaient | Le plan retient |
|---|---|---|
| RI-1 | `Diagnostics` : 8 champs, `Copy` (S1) ; `toc_dropped` en plus (S3) ; sans `Copy` (S4) | 9 champs `u32`, `Copy` ; `toc_dropped`, `add_toc_dropped` et sa ligne `debug` posés en Task 10 |
| RI-2 | `civil(raw, Field, diags)` (S1, S5, S6) contre `civil(raw, diags)` (S3, S4) ; `invalid date` (S3-D3) | S1 ; `body::date_field` (S3) et `date_field` (S4) supprimés ; détail `unreadable date` partout |
| RI-3 | indice réel dans le chemin, `article.textTitles[3].cid` (S4) | `Field.path` est `&'static str` : `article.textTitles[].cid` |
| RI-4 | `body::{not_found, invalid}` (S3) ; `ENDPOINT`, `decode_error`, `not_found`, `bump` locaux (S4) | `map::not_found`, `Field::{decode, domain}`, `Diagnostics::note_*`, `map::endpoint::*` ; aucune constante d'endpoint locale en production ; `text_body` et `toc::build` prennent `endpoint: &'static str` |
| RI-5 | « rempli » = `id`, `cid`, `num`, `texte`, `texteHtml` ou un titre de `textTitles` (S4 `is_filled`) ; `is_empty` (S3) | `rules::require` partout ; `ArticleDto` = `[id, cid, None]` (04 §9.1 a à la lettre) : `{"article":{"num":"9","texte":"t"}}` ⇒ `NotFound` |
| RI-6 | `identify` (S3 : `parse_versioned`, `KaliTextId::parse`) ; `returned_id` (S4) | `rules::resolve_id` partout ; un suffixe bien formé hors `LEGITEXT` est compté aussi pour l'article (S4 disait non : 06 D-28 l'exige) |
| RI-7 | `version_start_of` (S2) ; `start_of` (S3) | `rules::suffix_start(raw) -> Option<CivilDate>` (`open_ended` compris), Task 14 ; `date_debut_version = suffix.or(dated)` |
| RI-8 | hits par `DocumentId::parse`, sans `check_origin` (S2) ; `parse_counted` « partagé avec la recherche » (S1) | hits par `rules::parse_counted` (suffixe hors `LEGITEXT` compté) puis `rules::check_origin` (règle d, 04 §9.1) ; `map_hit` reçoit `diags` dès Task 15 |
| RI-9 | `long_text` local à chemin (S4) ; `juri::long_text` à endpoint (S5) | `rules::long_text(html, plain, Field, diags)` (règle f), Task 13 ; chemin = champ brut (`article.texte`, `article.nota`, `text.texte`) |
| RI-10 | `map/juri.rs` (`date_field`, `long_text`, `support`) (S5 D-2) | pas de `juri.rs` : `dates::prefer_computed` (Task 11), `rules::long_text`, `fixtures::{decode_as, juri_with}` |
| RI-11 | chargeurs `fixture()` (S2), `body::fixtures` (S3), `FIXTURE` (S4), `support::parse_fixture` (S5) ; golden lu trois fois | `map/fixtures.rs` seul ; Task 12 (S6-1) avancée juste après Task 11, avec `decode_as::<T>(name)` (chemin de production : `decode_body(.., true)` puis `decode`) ; `fixtures::golden()` en Task 17 |
| RI-12 | `pub(crate) use act::act;` (S3, S5, S6) contre aucun ré-export (S2, S4) | `pub(crate) mod <mapper>;` sans ré-export : un `pub(crate) use` inutilisé hors tests lève `unused_imports`, que l'`expect(dead_code)` ne couvre pas ; appels `map::act::act(..)` |
| RI-13 | `reason` de l'`expect(dead_code)` éditée par S1, S2, S3, S5, S6 | éditée en Task 9 (budget) et Task 10 (`map` entier, `Stats::record_mapping`) seulement ; Task 31 ajoute `#[cfg(test)] mod view;` |
| RI-14 | `short` (`clean_with_report`) compte les invisibles des champs courts de l'ACCO (S6 P6) | `clean_field` (D17) ; le test ACCO attend un compteur à 0 |
| RI-15 | fuzz en 2g, feature `fuzzing` et `fuzz_decode` `#[doc(hidden)]` (S6 P13) | D7 : une feature activée par `--all-features` (`make lint`, `test`, `cov`) rendrait l'`expect(dead_code)` inaccomplie |
| RI-16 | ordres locaux à chaque section | S0 (Tasks 1-6), D-24 (Task 7, mêmes fichiers DTO), fuzz (Task 8, DTO finaux), docs (Task 9) ; puis S1, S6-1, S2, S3, S4, S5, S6 |
| RI-17 | 04 §9.3 réécrite par chaque section (« la première remplace ») ; docs dans chaque tâche | docs du budget en Task 9, docs du mapping en Task 32 (une seule réécriture de §9.3) ; seule Task 18 (domaine) documente sur place |
| RI-18 | erreur de TOC autre ⇒ `invalid table of contents` (S3-D13) | `Field::domain` : `invalid value` |
| RI-19 | chemins `/home/muchini/mcp-ligitfrance-m2e` (S0) | `$WT` |
| RI-20 | `make deny` « doit rester vert » après la cible de fuzz (S0) | déjà rouge (RUSTSEC-2026-0386) ; `tokio` 1.53.2 entre aussi dans `fuzz/Cargo.lock` par `lf-application` : toléré pour ce seul avis |

---

## Pré-vérification croisée

Faite à la rédaction : chaque nom ci-dessous a été relu dans le code de `bc142c1` (`grep`) ou dans l'Interfaces de la tâche qui le produit.

### Paires de tâches (fichier ou interface partagés)

| Tâches | Fichier ou interface | Constat | Alignement |
|---|---|---|---|
| 1 ↔ 2 | `budget::{arm, charge, TOO_MANY, remaining}` | mêmes noms et types | — |
| 2 ↔ 3 | `de::capped_list(d, cap)` ← `search::bounded_results` ; `expecting` = `a sequence` | les tests livrés attendent `invalid type: string, expected a sequence` (search.rs, juri.rs, decode.rs) | texte gardé |
| 3 ↔ 4 | `dto/search.rs`, `SearchResponseDto` | champs distincts (`results`, `type_pagination`, l. 29 et 37) | séquentiel |
| 2, 3 ↔ 5 | listes des DTO | 25 `pub …: Vec<` de production (search 8, consult_text 8, juri 4, article 3, acco 2) et 27 `null_default`, dont 2 structs (article.rs l. 72, juri.rs l. 118) | balayage à 25 et 2 |
| 5 ↔ 6 | tests de `consult_text.rs` | la constante de test est `LODA` (l. 189), pas `ENDPOINT` | `LODA` employée |
| 6 ↔ sonde | `dto/{de,budget,decode,search,consult_text}.rs` inclus | `search.rs` importe `PageSize` dès Task 3 | sonde dépend de `lf-domain` ; chemins `$WT` (RI-19) |
| 5 ↔ 7 | mêmes fichiers DTO | 8 champs `num`/`numParution` : 5 en `opt_nonempty`, 3 sans attribut (`TocArticleDto.num` l. 125, `ArticleDto.num` l. 45, `SearchResultDto.num` l. 70) | Task 7 après Task 5, balayage inchangé |
| 7 ↔ 8 | DTO inclus par `#[path]` | leurs `use` de production : `super::de`, `serde`, `lf_application`, `serde_json`, `serde_path_to_error` seulement ; `oauth.rs` exige `secrecy` | `oauth.rs` et `error.rs` exclus ; trois `../` |
| 8 ↔ `make deny` | graphe de `fuzz/` | `tokio` 1.53.2 arrive par `lf-application` (`rt`, `time`) | RI-20 |
| 3 ↔ 15 | `ensure_fits(&self, endpoint: &str, page_size: PageSize)` | détail `/search: results: more results than page size` ; `endpoint::SEARCH == "/search"` | — |
| 4 ↔ 15 | `type_pagination: Option<String>` ; `memo::received_mode(Option<&str>)` (memo.rs l. 193) | cas `7` ⇒ `(Defaut, None)` exige R13 | Task 4 avant Task 15 |
| 2 ↔ 10-31 | budget non armé hors `decode` | toutes les lectures de DTO des sections passent par `decode`, `decode_as` ou un DTO construit à la main ; le seul `from_str` sur DTO est sous `arm()` (Task 6) | Global Constraints |
| 10 ↔ 11-31 | `Diagnostics` | S1 : 8 champs ; S3 : `toc_dropped` ; S4 : `missing_id` | RI-1 |
| 10 ↔ 11 | `Field`, `endpoint::JURI` | — | — |
| 11 ↔ 16, 21, 22, 24, 27, 28, 29 | `dates::civil(Option<&RawDate>, Field, &mut Diagnostics)` | deux signatures et deux textes dans les sections | RI-2 |
| 11 ↔ 27, 28 | `dates::prefer_computed((raw, Field), (raw, Field), diags)` | `juri::date_field` de S5 | RI-10 |
| 11 ↔ 12 | `map/fixtures.rs` | `NAMES`, `text`, `value` (S1) puis `Kind`, `Fixture`, `ALL`, `decoded` (S6) | Task 12 suit Task 11 ; `decode_as` ajouté |
| 12 ↔ 17, 20, 21, 22, 24, 27, 28, 30 | chargement des fixtures | quatre aides divergentes | `fixtures::decode_as::<T>(name)` (RI-11) |
| 13 ↔ 21, 22, 24, 27, 28, 29 | `rules::require` ; `Identified` | champs relus : `ConsultTextDto.{id,cid,title}`, `ArticleDto.{id,cid}`, `JuriTextDto.{id,titre,titre_long}`, `AccordDto.{id,titre_texte}` | RI-5 |
| 13 ↔ 24, 27, 28 | `rules::long_text` | `LongText::best_of` rend `Result<_, DomainError>` (`TextTooLarge` seul) | RI-9 |
| 14 ↔ 15 | `rules::{parse_counted, suffix_start, check_origin}` | S2 parsait par `DocumentId::parse` | RI-7, RI-8 |
| 14 ↔ 21, 22, 24, 27, 28, 29 | `resolve_id<T: FromStr<Err = DomainError> + Display + PartialEq + Clone>` | `Id<K>` : `FromStr`, `Display`, `PartialEq`, `Clone` manuels (id.rs l. 219-250) ; `ArticleId`, `DecisionId`, `DocumentId` dérivés | RI-6 |
| 14 ↔ 15, 27, 28 | `check_origin(origin, id: &str, diags)` | S5 passait `&id.to_string()` | `id.as_str()` |
| 17 ↔ 20, 30 | golden | trois `include_str!` dans les sections | `fixtures::golden()` (Task 17) |
| 18 ↔ 19, 21, 22 | `TextBody.cid` | `Option<JorfTextId>` aujourd'hui (entity/act.rs l. 19) | `Option<DocumentId>` ; `body::cid_of` le rend |
| 19 ↔ 20 ↔ 21 ↔ 22 | `body::{non_blank, cid_of, nor_of, aliases_of, text_body}`, `toc::build` | S3 avait aussi `not_found`, `invalid`, `date_field`, `start_of`, `is_empty` | retirés (RI-4, RI-5, RI-7) |
| 20 | erreurs de `Toc::build` | `TocTooDeep { max: u16 }`, `TextTooLarge { chars: u64 }`, `EmptyQuery` constructibles (error.rs) | RI-18 |
| 25 ↔ 27 | `DecisionId::fond` | absente (id.rs : `parse`, `as_str` seulement) | créée en Task 25 |
| 26 ↔ 27 | `native_ecli`, `numero_affaire`, `summary_of` | `SommaireDto.{resume_principal, abstrats, autre_resume}` relus (juri.rs l. 126-137) | — |
| 29 ↔ 13, 23, 27 | compteur d'invisibles des champs courts | `short` (S6) contre `clean_field` | RI-14 |
| 15-29 ↔ 30 | `map_body` appelle les sept mappers | signatures des Interfaces ci-dessous ; S3, S5, S6 prévoyaient un ré-export | RI-12 |
| 30 ↔ 31 | `Mapped`, `map_fixture`, `decoded` | — | — |
| 9, 10, 31 | `lib.rs` | `reason` actuelle finit par « au plan 2e » ; `mod map;` entre `limit` et `memo` | RI-13 |
| 9 ↔ 32 | 04 §8.5, §9.2, §9.3, 06, 07, A-parite | six sections les éditaient | RI-17 |

### Cohérence interne de chaque tâche

| Tâche | Contrôle |
|---|---|
| 1 | 10 tests couvrent `spend`, `Refusal::text`, `arm`, `charge`, la garde (chute, panique), le fil ; `remaining` sous `cfg(test)` |
| 2 | 9 tests de `de.rs`, 5 de `decode.rs` ; aides `Lists`, `Capped`, `Bounded` locales aux tests |
| 3 | 3 éléments consommés avant `values` ⇒ frontière 199 997 / 199 998 ; `fits` couvre `u32::try_from` en échec |
| 4 | 12 valeurs JSON couvrent les sept méthodes de `CodeVisitor` (de.rs l. 185-226) |
| 5 | 17 listes = 25 − 8 ; indice refusé = 200 000 − ancêtres comptés |
| 6 | 2 000 × (1 + 9 × (1 + 3)) = 74 000 éléments ; 600 014 et 600 017 octets mesurés |
| sonde | 9 formes ↔ seuils (1) à (5) ; repli décrit |
| 7 | un `num_accepts_int_and_string` par module (search, consult_text, article, juri) + `ConteneurDto` ; `numeroAffaire` verrouillé |
| 8 | 8 fichiers inclus ; cinq types ↔ cinq gabarits d'endpoint |
| 9 | chaque phrase cite un test des Tasks 1 à 8 |
| 10 | 9 champs ↔ 9 notes ou ajouts ↔ 9 lignes de journal ↔ 9 champs de `StatsSnapshot` ; les 7 constantes d'endpoint ont un test (aucune tolérance `dead_code` en test) |
| 11 | 154 absentes + 22 sentinelles + 59 dates = 235 feuilles ; `UTC_ZONES` couverts par `is_utc_midnight_table` |
| 12 | 16 lignes d'`ALL` = `NAMES` ; M7 : 6 réparées + 1 refusée + 9 saines = 16 |
| 13 | 4 `impl Identified` ↔ 4 enveloppes ; `long_text` : les 4 bras de `best_of` et l'erreur |
| 14 | `ORIGINS` (7) ↔ `origin_matches_on_every_fixture` ; 17 hits = M4 |
| 15 | 7 résultats ⇒ 2 gardés, 5 écartés (cas énumérés) |
| 16 | `article_ref` : les trois bras du `match` exercés |
| 17 | 17 hits = 2+1+2+3+3+2+2+2 ; 7 extraits coupés |
| 18 | trois familles de `cid` (`LEGITEXT`, `JORFTEXT`, `KALITEXT`) |
| 19 | 4 fonctions, chaque bras `Option` a son cas |
| 20 | 88 entrées = 26 sections + 62 articles = 3 + 7 + 26 + 52 par profondeur |
| 21 | ordre figé (`cid`, 3 dates, TOC) ↔ `a_decode_error_names_the_first_failing_field` |
| 22 | `bulletin_of` : 4 cas = 4 bras ; `conteneurs[0]` seul |
| 23 | `preferred_title` 5 cas ; `parent_text` : `cid`, `id`, aucun, illisible |
| 24 | 322 caractères réparés (336 bruts) ; variante KALIARTI = même entité, `id_mismatch` 1 |
| 25 | 3 variantes ↔ 3 bras sans `_` |
| 26 | répartition par fond ↔ 4 tests ; `native_ecli` : absent, valide, invalide |
| 27 | 3 fixtures en table ; ordre d'assemblage = ordre des erreurs |
| 28 | 3 paragraphes 153 + 262 + 174 = 589, + 2 × 2 séparateurs = 593 |
| 29 | 3 dates lues, 2 jamais lues ↔ `agreement_ignores_the_unmapped_dates_and_fields` |
| 30 | 17 entrées golden = 16 + variante ; 26 contrats = 17 hits + 8 consultations + 1 variante |
| 31 | `view` sans `..` pour les 7 entités ; contrôle négatif sur 6 fixtures |
| 32 | chaque phrase renvoie à une tâche livrée |
| 33 | liste des fichiers ↔ `git diff --name-only` |

---

## Structure des fichiers

```text
crates/lf-adapter-piste/src/
  dto/budget.rs                              [T1]        budget thread-local, garde Drop
  dto/de.rs, dto/decode.rs                   [T2]        bounded_list, capped_list ; decode arme le budget
  dto/search.rs                              [T3 T4 T7]  results ≤ 100, ensure_fits, typePagination, num
  dto/{consult_text,article,juri,acco}.rs    [T5 T6 T7]  listes bornées, num tolérant
  dto/mod.rs, memo.rs                        [T1 T5, T4] déclaration, balayage ; un test
  lib.rs                                     [T9 T10 T31] reason, mod map, mod view
  stats.rs, corpus.rs                        [T10]       9 compteurs, record_mapping ; deux tests
  map/mod.rs                                 [T10 …]     Diagnostics, Field, endpoint, not_found ; déclarations
  map/dates.rs                               [T11]       civil, first_civil, prefer_computed
  map/fixtures.rs (cfg(test))                [T11 T12 T17 T27 T30] chargeur unique, ALL, decode_as, golden, juri_with, Mapped
  map/rules.rs                               [T13 T14]   require, long_text, resolve_id, parse_counted, suffix_start, check_origin
  map/search.rs                              [T15-T17]
  map/{body,toc,act,convention}.rs           [T19-T22]
  map/article.rs                             [T23 T24]
  map/{decision,deliberation}.rs             [T26-T28]
  map/agreement.rs                           [T29]
  view.rs (cfg(test))                        [T31]
crates/lf-domain/src/entity/act.rs [T18] ; crates/lf-domain/src/id.rs [T25]
fuzz/fuzz_targets/dto_decode.rs, fuzz/Cargo.toml, fuzz/Cargo.lock, .github/workflows/weekly.yml   [T8]
docs/plan/{00,02,03,04,05,06,07}-*.md, docs/plan/annexes/A-parite.md   [T9 T18 T32]
```

---

## PR 1 — budget de décodage et DTO (tâches 1 à 9)

Aucune tâche ne touche `map/`. Faits mesurés (`python3 -I /var/tmp/m2f-plan/measure_s0.py`, `count_budget.py`) : `consult_loda.json` = 91 éléments (26 sections, 62 articles, 3 appellations) pour 88 nœuds de TOC et 12 093 octets ; `consult_kali.json` = 5 ; `search_*.json` de 4 à 27. TOC de code extrapolée : 20 000 nœuds ≈ 20 700 éléments (10 % du budget), 2,6 Mio. Pire forme estimée (« un article par section », listes de 4 places à la première insertion) ≈ 110 Mio résidents pour 200 000 éléments, contre 42 Mo annoncés par 04 §8.5 : **la sonde tranche** (seuil 160 Mio).

### Task 1 : `dto/budget.rs`, budget d'éléments par document

04 §8.5 limite 1 (a). État thread-local armé par `decode` (Task 2), remis à zéro par une garde `Drop`, même sur erreur ou panique ; `decode` est synchrone (aucun `await` entre `arm` et la chute de la garde).

**Files:**
- Create: `crates/lf-adapter-piste/src/dto/budget.rs`
- Modify: `crates/lf-adapter-piste/src/dto/mod.rs` (`pub(crate) mod budget;` entre `article` et `consult_text`)

**Interfaces:**
- Produces :

```rust
pub(crate) const MAX_ELEMENTS: u32 = 200_000;
pub(crate) const TOO_MANY: &str = "too many elements";
pub(crate) const NOT_ARMED: &str = "decoding budget not armed";
#[must_use = "le budget est désarmé dès que la garde tombe"]
pub(crate) struct Armed(());                                   // Drop : REMAINING.set(None)
pub(crate) fn arm() -> Armed;                                  // Some(MAX_ELEMENTS) ; non réentrant (documenté)
pub(crate) fn charge<E: serde::de::Error>() -> Result<(), E>;  // un élément ; refus = E::custom(texte fixe)
#[cfg(test)] pub(crate) fn remaining() -> Option<u32>;
enum Refusal { Exhausted, NotArmed }                           // Debug, Clone, Copy, PartialEq, Eq ; fn text(self) -> &'static str
fn spend(remaining: Option<u32>) -> Result<u32, Refusal>;      // None ⇒ NotArmed ; Some(0) ⇒ Exhausted ; Some(n) ⇒ Ok(n - 1)
```

État : `thread_local! { static REMAINING: Cell<Option<u32>> = const { Cell::new(None) }; }` ; `charge` = `spend(get())`, refus ⇒ `E::custom(refusal.text())`, sinon `set(Some(left))`. Les deux textes n'ont ni guillemet ni accent grave (`redact` les garde tels quels).

- [ ] **Step 1 : tests** (rouges ; `E = serde_json::Error`, comparer `to_string()`).
  - `spend_decrements_and_refuses_at_zero` : `spend(Some(2)) == Ok(1)`, `spend(Some(1)) == Ok(0)`, `spend(Some(0)) == Err(Exhausted)`, `spend(None) == Err(NotArmed)`.
  - `refusal_texts_are_fixed` (`"too many elements"`, `"decoding budget not armed"`) ; `max_elements_is_200_000`.
  - `a_fresh_arm_grants_exactly_max_elements` : après `arm()`, `MAX_ELEMENTS` charges `Ok`, `remaining() == Some(0)`, la suivante `Err` de texte `"too many elements"`, `remaining()` toujours `Some(0)`.
  - `charging_decrements_the_remaining_budget` (3 charges ⇒ `Some(MAX_ELEMENTS - 3)`) ; `charging_without_a_guard_is_refused` (texte `"decoding budget not armed"`).
  - `the_guard_disarms_on_drop` (`Some(MAX_ELEMENTS)` dans le bloc, `None` après) ; `the_guard_disarms_on_unwind` (`catch_unwind(AssertUnwindSafe(|| { let _armed = arm(); resume_unwind(Box::new(())) }))` est `Err`, puis `remaining() == None`).
  - `arming_again_restarts_the_count` (5 charges, chute, `arm()` ⇒ `Some(MAX_ELEMENTS)`) ; `each_thread_has_its_own_budget` (`arm()` ici, `thread::spawn(|| charge::<serde_json::Error>().is_err()).join().unwrap()` vaut `true`, ici `Some(MAX_ELEMENTS)`).
- [ ] **Step 2 : implémenter** (docs en français, `# Errors` sur `charge`).
- [ ] **Step 3 : vérifier.** `cargo nextest run -p lf-adapter-piste dto::budget` (10 verts) ; `make lint` ; `make cov`. `budget` est mort hors tests jusqu'à Task 2 : couvert par l'`expect(dead_code)` de `lib.rs`.
- [ ] **Step 4 : commit** `feat(adapter): budget d'éléments par document (R11, 04 §8.5)`.

### Task 2 : lecteur de liste borné et armement par `decode`

04 §8.5 limite 1 (a). Remplacera `de::null_default` pour les `Vec`.

**Files:**
- Modify: `crates/lf-adapter-piste/src/dto/de.rs`, `crates/lf-adapter-piste/src/dto/decode.rs`

**Interfaces:**
- Consumes : `budget::{arm, charge, TOO_MANY, remaining}`.
- Produces :

```rust
/// `null` ou absent donne `Vec::new()` ; chaque élément lu coûte 1 au budget du document.
pub(crate) fn bounded_list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error>; // = capped_list(d, usize::MAX)
/// Comme `bounded_list`, au plus `cap` éléments : le suivant donne `too many elements`.
pub(crate) fn capped_list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D, cap: usize) -> Result<Vec<T>, D::Error>;
// privés : ListVisitor<T> { cap, marker: PhantomData<fn() -> T> } : deserialize_option ; visit_none ⇒ vide ; visit_some ⇒ deserialize_seq ;
//   visit_seq : boucle next_element_seed(Element { under_cap: items.len() < cap }) ; expecting ⇒ exactement "a sequence"
// Element<T> : DeserializeSeed, admit(under_cap)? puis T::deserialize
// fn admit<E: Error>(under_cap: bool) -> Result<(), E>   // !under_cap ⇒ E::custom(TOO_MANY), sinon budget::charge()
```

`decode` : première ligne `let _budget = budget::arm();` (variable nommée, vit jusqu'à la sortie, `?` compris) ; sa doc gagne un paragraphe (budget de 200 000 éléments par document, `Decode` à texte fixe `too many elements`, chemin de l'élément refusé). `null_default` reste (deux champs de struct) ; sa doc dit qu'il ne sert plus de liste.

- [ ] **Step 1 : tests de `de.rs`** (rouges). Aides : `Lists { #[serde(default, deserialize_with = "bounded_list")] a: Vec<String>, …même attribut… b: Vec<String> }` ; `Capped { #[serde(default, deserialize_with = "two_at_most")] items: Vec<u8> }`, `two_at_most` = `capped_list(d, 2)`. Tout passe par `decode(raw, "/e")`.
  - `a_bounded_list_reads_null_absent_and_empty_as_empty` : `{"a":null}`, `{}`, `{"a":[]}` ⇒ `a == []` ; `a_bounded_list_keeps_the_elements_in_order` : `["x","y","z"]`.
  - `a_bounded_list_of_the_wrong_type_keeps_the_sequence_message` : `{"a":"secret-text-42"}` ⇒ `Decode { detail: "/e: a: invalid type: string, expected a sequence" }` ; `{"a":{}}` ⇒ `…map, expected a sequence` ; `{"a":7}` ⇒ `…integer, expected a sequence`.
  - `a_bounded_list_element_error_keeps_its_path` : `{"a":["ok",5]}` ⇒ `"/e: a[1]: invalid type: integer, expected a string"`.
  - `the_budget_is_global_across_lists` : `a` de 100 000 `"x"` et `b` de 100 001 ⇒ `"/e: b[100000]: too many elements"` ; `b` de 100 000 ⇒ `Ok`.
  - `a_capped_list_accepts_exactly_the_cap` : `[1,2]` ⇒ `[1,2]` ; `[1,2,3]` ⇒ `"/e: items[2]: too many elements"` ; `a_capped_list_refusal_does_not_spend_budget` : sous `arm()` manuel et `serde_json::from_str`, après le refus, `remaining()` n'a baissé que de 2.
  - `a_list_read_outside_decode_is_refused` : `serde_json::from_str::<Lists>(r#"{"a":["x"]}"#)` est `Err` dont `to_string()` commence par `decoding budget not armed`.
  - `a_refusal_never_carries_a_value` : 101 éléments `"sekret-42-Ω"` dans un plafond de 100 ⇒ ni `Debug` ni `Display` ne contiennent `sekret`.
- [ ] **Step 2 : tests de `decode.rs`** (`mod tests` existant, `ENDPOINT = "/consult/juri"` ; aide `Bounded { #[serde(default, deserialize_with = "crate::dto::de::bounded_list")] items: Vec<u8> }`).
  - `decode_disarms_the_budget_after_success` et `decode_disarms_the_budget_after_an_error` (`{"items":["x"]}`) : `budget::remaining() == None` ensuite.
  - `each_decode_gets_a_full_budget` : 200 001 `1` échoue, puis deux décodages de 150 000 `1` réussissent à la suite.
  - `too_many_elements_maps_to_decode_with_the_element_path` : 200 001 `1` ⇒ `Decode { detail: "/consult/juri: items[200000]: too many elements" }`.
  - `a_too_many_elements_detail_is_kept_as_is_by_redact` : `redact("too many elements")` et `redact("too many elements at line 1 column 12")` inchangés.
- [ ] **Step 3 : implémenter**, puis `cargo nextest run -p lf-adapter-piste dto::` ; `make lint` ; `make cov`.
- [ ] **Step 4 : commit** `feat(adapter): lecteur de liste borné et armement du budget par decode (R11)`.

### Task 3 : `/search` : `results` borné à 100 et `ensure_fits`

04 §8.5 limite 1 (b) ; 07 T-M2-09 critère (1).

**Files:**
- Modify: `crates/lf-adapter-piste/src/dto/search.rs`

**Interfaces:**
- Consumes : `de::{bounded_list, capped_list}`, `lf_domain::search::PageSize`, `CorpusError`.
- Produces :

```rust
pub(crate) const MAX_RESULTS: usize = 100;                     // PageSize::MAX, 04 §8.5
fn bounded_results<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<SearchResultDto>, D::Error>; // capped_list(d, MAX_RESULTS)
impl SearchResponseDto {
    /// `Decode` si la page porte plus de résultats que `page_size`.
    pub(crate) fn ensure_fits(&self, endpoint: &str, page_size: PageSize) -> Result<(), CorpusError>;
}
fn fits(results: usize, page_size: u32) -> bool;               // u32::try_from(results).is_ok_and(|count| count <= page_size)
```

`results` : `#[serde(deserialize_with = "bounded_results")]` ; les sept autres `Vec` (`titles`, `themes`, `appellations`, `resume_principal`, `sections`, `extracts`, `values`) : `de::bounded_list`. Détail de l'erreur : `format!("{endpoint}: results: more results than page size")`. Appelant : Task 15, avant toute autre lecture.

- [ ] **Step 1 : tests** (rouges, `ENDPOINT = "/search"`).
  - `max_results_equals_the_domain_page_size_maximum` : `u32::try_from(MAX_RESULTS) == Ok(PageSize::MAX)`.
  - `a_page_of_100_results_is_read` (`{}`×100) ; `a_page_of_101_results_maps_to_decode_with_a_path` : `{}`×101 (316 octets) ⇒ `Decode { detail: "/search: results[100]: too many elements" }`.
  - `a_secret_in_a_refused_page_never_reaches_the_detail` : 101 résultats `{"nature":"sekret-42-Ω"}` ⇒ ni `Debug` ni `Display` ne contiennent `sekret`.
  - `the_search_lists_share_the_document_budget` : `{"results":[{"sections":[{"extracts":[{"values":[…]}]}]}]}` : 199 997 `""` ⇒ `Ok` (`values.len() == 199_997`) ; 199 998 ⇒ `"/search: results[0].sections[0].extracts[0].values[199997]: too many elements"`.
  - `ensure_fits_table` : `(0,1)`, `(1,1)`, `(10,10)`, `(100,100)` ⇒ `Ok` ; `(2,1)`, `(11,10)` ⇒ `Decode { detail: "/search: results: more results than page size" }` (`PageSize::new(n).unwrap()`) ; `fits_is_inclusive_at_the_page_size` : `fits(10, 10)`, `!fits(11, 10)`, `fits(0, 1)`, `!fits(usize::MAX, u32::MAX)`.
  - `results_null_or_absent_is_an_empty_list` et `null_lists_inside_a_result_are_empty` restent verts, non modifiés.
- [ ] **Step 2 : implémenter**, puis `cargo nextest run -p lf-adapter-piste dto::search` ; `make cov`.
- [ ] **Step 3 : commit** `feat(adapter): results borné à 100 et page_size contrôlé (R11)`.

### Task 4 : R13, `typePagination` tolérant

04 §8.5 limite 5, §6.6 ; 07 T-M2-09 critère (2).

**Files:**
- Modify: `crates/lf-adapter-piste/src/dto/search.rs` (champ `type_pagination`, doc du module), `crates/lf-adapter-piste/src/memo.rs` (un test)

**Interfaces:**
- Produces : `SearchResponseDto.type_pagination: Option<String>` avec `#[serde(deserialize_with = "de::opt_string_or_number")]` ; doc du champ : « `DEFAUT` ou `ARTICLE` ; nombre lu comme chaîne ; tout autre type ignoré (R13) ». `memo::received_mode(Option<&str>) -> Option<PaginationMode>` inchangée.

- [ ] **Step 1 : tests** (rouges).
  - `type_pagination_accepts_a_string_a_number_and_ignores_the_rest` (sur `{"typePagination":<raw>}`) : `"ARTICLE"`, `"DEFAUT"` ⇒ eux-mêmes ; `3` ⇒ `Some("3")` ; `-1` ⇒ `Some("-1")` ; `1.5`, `true`, `{"a":1}`, `[1,2]`, `null`, `""`, `"  "`, absent ⇒ `None`.
  - `type_pagination_skips_a_deep_value_without_building_it` : 200 niveaux de `[` autour de `1` ⇒ `None`, sans `Decode`.
  - `a_page_with_a_non_string_type_pagination_is_still_read` : `{"typePagination":{"x":1},"totalResultNumber":5,"results":[{}]}` ⇒ `total_result_number == Some(5)`, 1 résultat.
  - `memo.rs`, `a_numeric_type_pagination_is_unknown_without_a_memo` (journal capté par `crate::testing::capture_at`, comme `an_unknown_type_pagination_leaves_no_memo`) : `received_mode(Some("3")) == None` ; journal avec `unknown typePagination` et `len=1`, sans la valeur.
- [ ] **Step 2 : implémenter** (un attribut et sa doc), puis `cargo nextest run -p lf-adapter-piste dto::search memo::` ; `make cov`.
- [ ] **Step 3 : commit** `fix(adapter): typePagination tolérant, une page n'est plus perdue (R13)`.

### Task 5 : les 17 autres listes et le balayage des sources

04 §8.5 limite 1 (a) : « chaque liste passant par un lecteur borné ».

**Files:**
- Modify: `crates/lf-adapter-piste/src/dto/{consult_text,article,juri,acco}.rs` (8 + 3 + 4 + 2 listes), `crates/lf-adapter-piste/src/dto/mod.rs` (module de test)

**Interfaces:**
- Produces : chaque `#[serde(deserialize_with = "de::null_default")]` au-dessus d'un `Vec<…>` devient `de::bounded_list` ; `#[serde(rename = "titresTM", deserialize_with = "de::bounded_list")]` ; `ArticleDto.context` et `JuriTextDto.decision_attaquee` gardent `null_default`. Aucun type de champ ne change.

- [ ] **Step 1 : balayage** (`dto/mod.rs`, rouge). Sources par `include_str!("search.rs")`, `("consult_text.rs")`, `("article.rs")`, `("juri.rs")`, `("acco.rs")`, coupées à la première occurrence de `#[cfg(test)]`.
  - `every_dto_list_is_bounded` : chaque ligne qui commence (après blancs) par `pub ` et contient `: Vec<` a, dans les 3 lignes au-dessus, un `serde(` contenant `bounded_list` ou `bounded_results` ; **25** listes.
  - `null_default_serves_only_the_two_struct_fields` : **2** occurrences de `deserialize_with = "de::null_default"`, chacune suivie à moins de 3 lignes d'un champ sans `Vec<`.
- [ ] **Step 2 : un test par fichier** (rouges ; constante d'endpoint du `mod tests`, `LODA` pour `consult_text.rs`). Chaque liste reçoit 200 001 éléments ; attendu `Decode` de détail exact `<endpoint>: <chemin>: too many elements`.
  - `every_consult_text_list_is_budgeted` : racine `appellations` (`"x"`), `conteneurs`, `sections`, `articles` (`{}`) ⇒ `<liste>[200000]` ; `sections[0].sections[199999]` ; `sections[0].articles[199999]` ; `articles[0].lstLienCitation[199999]` ; `articles[0].lstLienModification[199999]`.
  - `every_article_list_is_budgeted` (`/consult/getArticle`) : `article.textTitles[200000]`, `article.context.titresTM[200000]`, `article.context.titreTxt[200000]`.
  - `every_juri_list_is_budgeted` (`/consult/juri`) : `text.numeroAffaire[200000]`, `text.titrages[200000]` (chaînes), `text.sommaire[200000]`, `text.liens[200000]` (`{}`).
  - `every_acco_list_is_budgeted` (`/consult/acco`) : `acco.themes[200000]`, `acco.syndicats[200000]`.
  - Restent verts sans modification : `null_*_is_empty`, `a_wrong_type_is_a_decode_error_with_a_json_path_and_no_value`, `a_toc_of_63_levels_is_decoded`, `a_toc_of_64_levels_or_more_maps_to_decode`.
- [ ] **Step 3 : implémenter** (remplacement des attributs), puis `cargo nextest run -p lf-adapter-piste dto::` ; `make cov`. Un test au-delà de 5 s en debug : réduire les cas de `consult_text.rs` aux endpoints distincts, sans toucher au budget.
- [ ] **Step 4 : commit** `feat(adapter): toutes les listes des DTO passent par le budget (R11)`.

### Task 6 : frontière exacte et TOC de la taille d'un code

04 §8.5 limite 1 (« 15 000 à 20 000 nœuds restent sous le budget ») ; T-M2-09 critère (1). Tests seulement (`crates/lf-adapter-piste/src/dto/consult_text.rs`, module de test) ; emploie `budget::{arm, remaining, MAX_ELEMENTS}`.

- [ ] **Step 1 : tests** (verts si Tasks 2 et 5 sont justes).
  - `exactly_200_000_elements_are_decoded` : `{"articles":[{}×200000]}` (600 014 octets) via `decode` ⇒ `articles.len() == 200_000`.
  - `one_element_more_maps_to_decode_with_a_path` : `{}`×200 001 (600 017 octets) ⇒ `format!("{error:?}") == r#"Decode { detail: "/consult/lawDecree: articles[200000]: too many elements" }"#`.
  - `the_budget_spans_the_root_lists` : `articles` de 100 000 puis `sections` de 100 001 ⇒ `"/consult/lawDecree: sections[100000]: too many elements"`.
  - `a_code_sized_toc_stays_far_under_the_budget` : `article = {"id":"LEGIARTI000000000001","num":"1","etat":"VIGUEUR","lstLienCitation":[{},{}],"lstLienModification":[{}]}`, `section = {"id":"LEGISCTA000000000001","title":"T","sections":[],"articles":[<9 articles>]}`, `corps = {"sections":[<2 000 sections>]}` : `corps.len() == 2_118_014` ; sous `let guard = budget::arm();` et `serde_json::from_str::<ConsultTextDto>` : `MAX_ELEMENTS - remaining().unwrap() == 74_000`, `sections.len() == 2_000`, `sections[0].articles.len() == 9`, `sections[1_999].articles[8].lst_lien_citation.len() == 2` ; `drop(guard)` ; `decode(&corps, LODA)` ⇒ `Ok`. Commentaire : 20 000 nœuds, 74 000 éléments = 37 % du budget.
  - `an_article_with_nine_links_costs_ten_elements` : 1 000 articles racine à 9 `lstLienCitation` ⇒ 10 000 éléments consommés.
- [ ] **Step 2 : vérifier.** `cargo nextest run -p lf-adapter-piste dto::consult_text` ; le test de 2 118 014 octets reste sous 2 s en debug (sinon le noter au ledger, ne pas réduire) ; `make cov`.
- [ ] **Step 3 : commit** `test(adapter): frontière du budget et TOC de la taille d'un code (R11)`.

### Étape du contrôleur : sonde de mémoire hors dépôt

Pas une tâche de sous-agent. Après Task 6, avant Task 9 (Tasks 7 et 8 peuvent précéder) ; machine au repos (aucun `cargo test` en parallèle d'un build `--release`).

- [ ] **Mise en place** (jamais dans le dépôt) :

```bash
mkdir -p /var/tmp/m2f-probe/src && cd /var/tmp/m2f-probe
ln -sfn "$WT/crates/lf-adapter-piste/src/dto" dto
cat > Cargo.toml <<EOF
[package]
name = "probe"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
lf-application = { path = "$WT/crates/lf-application" }
lf-domain = { path = "$WT/crates/lf-domain" }
serde = { version = "1.0", default-features = false, features = ["derive"] }
serde_json = "1.0"
serde_path_to_error = "0.1"
[profile.release]
debug = false
EOF
cp "$WT/Cargo.lock" .
```

- [ ] **Source** `/var/tmp/m2f-probe/src/main.rs` (non compilée à la rédaction ; le contrôleur corrige les fautes de frappe) : `#![allow(dead_code, unused_imports)]` ; `mod dto` avec `#[path = "/var/tmp/m2f-probe/dto/<f>.rs"] pub(crate) mod <f>;` pour `de`, `budget`, `decode`, `search`, `consult_text` ; allocateur global compteur sur `System` (`CURRENT`, `PEAK` en `AtomicUsize` ; `realloc` retranche l'ancienne taille puis ajoute la nouvelle) ; `run::<T>(texte, endpoint) -> String` : `base = CURRENT`, `PEAK = base`, chronomètre autour de `dto::decode::decode::<T>`, rend `live`, `peak` (relatifs à `base`), `ms`, la ligne `VmHWM` de `/proc/self/status` et le verdict ; `rep(item, n) = vec![item; n].join(",")` ; une forme par processus, en argument :

| Forme | Type, endpoint | Corps |
|---|---|---|
| `results100`, `results101` | `SearchResponseDto`, `/search` | `{"results":[{}×100 ou 101]}` |
| `articles200000`, `articles200001` | `ConsultTextDto`, `/consult/lawDecree` | `{"articles":[{}×n]}` |
| `one-article-per-section` | idem | `{"sections":[{"articles":[{}]}×99 999]}` |
| `two-children-per-section` | idem | `{"sections":[{"sections":[{},{}]}×66 666]}` |
| `strings200000` | idem | `{"appellations":[""×200 000]}` |
| `ignored-16mib` | idem | `{"visas":[{"a":1}×(16*1024*1024/8 - 1)],"title":"x"}` |
| `toc20000` | idem | corps de `a_code_sized_toc_stays_far_under_the_budget` |

- [ ] **Lancement** : `cd /var/tmp/m2f-probe && cargo build --release --offline`, puis pour chaque forme `( ulimit -v 2097152; ./target/release/probe "$s" )`.
- [ ] **Seuils** (reportés en 04 §8.5 par Task 9, avec date et valeurs) : (1) `results101`, `articles200001` : refus avec chemin, < 100 ms, pic < 64 Mio ; (2) `articles200000`, `one-article-per-section`, `two-children-per-section`, `strings200000` : **pic ≤ 160 Mio** et ≤ 250 ms ; (3) `toc20000` : `ok`, pic ≤ 64 Mio (estimation ≈ 31 Mio) ; (4) `ignored-16mib` : `ok`, pic proche de la taille du corps, durée relevée ; (5) quatre décodages simultanés (04 §5.5) : ≤ 4 × pic de (2) ≤ 640 Mio, calculé. *Clôture (Ruling 6)* : ce calcul suppose le permis de concurrence tenu pendant le décodage ; `send` le rend dès le corps lu, donc la borne n'est tenue qu'avec l'exigence du plan 2g (Hors périmètre, 00 §11.1). **Si (2) échoue** : repli de D1, refaire Tasks 1 et 6 avec la nouvelle frontière, relancer.
- [ ] **Nettoyage** : `rm -rf /var/tmp/m2f-probe`.

### Task 7 : D-24, `num` et `numParution` en chaîne ou en nombre

06 §3.1 (D-24) ; 04 §8.1 (3), §8.4, §8.5 limite 3 ; annexe A E-14. Après Task 5 (mêmes fichiers). Mesure M8 : 78 `num` en chaîne, 34 `null`, 0 entier ; `numParution` 2 chaînes, 22 `null` ; `numeroAffaire` 4 listes.

**Files:**
- Modify: `crates/lf-adapter-piste/src/dto/juri.rs` (`JuriTextDto.num`, `TexteLienDto.num`), `dto/consult_text.rs` (`ConsultTextDto.num_parution`, `TocArticleDto.num`, `ConteneurDto.num`), `dto/article.rs` (`ArticleDto.num`), `dto/search.rs` (`SearchResultDto.num`, `SearchExtractDto.num`)

**Interfaces:**
- Produces : les huit champs restent `Option<String>`, avec `#[serde(deserialize_with = "de::opt_string_or_number")]` (remplace `opt_nonempty` ou l'absence d'attribut) ; doc : « en chaîne ou en nombre, rendu en chaîne (`""` : alors `None`) ».

- [ ] **Step 1 : tests** (rouges). Un `num_accepts_int_and_string` par module (search, consult_text, article, juri), par `decode::<Dto>(r#"{"num":<valeur>}"#, ENDPOINT)` (clé `numParution` pour `num_parution`) : `12500399` et `"12500399"` ⇒ `Some("12500399")` ; `"0045"` ⇒ `Some("0045")` ; `""`, `"  "`, `null`, absent ⇒ `None` (changement pour les trois champs sans attribut : `Some("")` avant) ; `2.5`, `true`, `[1]`, `{"a":1}` ⇒ `None`. `ConteneurDto.num` : `2121` ⇒ `Some("2121")`. Dans `juri.rs`, `numero_affaire_scalar_is_a_decode_error` : `decode::<JuriTextDto>(r#"{"numeroAffaire":12}"#, "/consult/juri")` ⇒ `Err(Decode)` dont le `detail` commence par `"/consult/juri: numeroAffaire: "`.
- [ ] **Step 2 : implémenter**, puis `cargo nextest run -p lf-adapter-piste num_accepts numero_affaire` ; `make cov` ; `make -k ci`.
- [ ] **Step 3 : commit** `fix(adapter): num et numParution lus en chaîne ou en nombre (D-24)`.

### Task 8 : cible de fuzz `dto_decode`

04 §8.5 limite 5 ; 06 §1.4 ; 07 T-M2-06 (« reste »). Inclusion `#[path]` (D7, RI-15).

**Files:**
- Create: `fuzz/fuzz_targets/dto_decode.rs`
- Modify: `fuzz/Cargo.toml`, `fuzz/Cargo.lock`, `.github/workflows/weekly.yml` (matrice `target`, aujourd'hui `[from_html, ids, text, version_date]`)

**Interfaces:**
- `fuzz/Cargo.toml` (a déjà `libfuzzer-sys` 0.4.13, `lf-domain`) : ajouter `lf-application = { path = "../crates/lf-application" }`, `serde = { version = "1.0", default-features = false, features = ["derive"] }`, `serde_json = "1.0"`, `serde_path_to_error = "0.1"` ; `[[bin]] name = "dto_decode"`, `path = "fuzz_targets/dto_decode.rs"`, `test = false`, `doc = false`, `bench = false` ; `[package.metadata.cargo-machete] ignored = ["serde_json", "serde_path_to_error"]` (commentaire : « employés par les sources incluses par #[path] », hors du répertoire du paquet).
- `fuzz/Cargo.lock` : nouveaux paquets aux versions du verrou du workspace (relevées le 2026-10-23) : `serde`, `serde_core`, `serde_derive` 1.0.229 ; `serde_json` 1.0.154 ; `serde_path_to_error` 0.1.20 ; `tokio` 1.53.2 ; `pin-project-lite` 0.2.17 ; `itoa` 1.0.18 ; `memchr` 2.8.3 ; `zmij` 1.0.23 (pas `ryu`). Relire avant d'écrire : `python3 -I -c "import tomllib; [print(p['name'], p['version']) for p in tomllib.load(open('Cargo.lock','rb'))['package'] if p['name'] in {'serde','serde_core','serde_derive','serde_json','serde_path_to_error','tokio','pin-project-lite','itoa','memchr','zmij'}]"`.

```rust
#![no_main]
use lf_application::ports::CorpusError;
use libfuzzer_sys::fuzz_target;

/// Plus long détail possible : 127 niveaux de 64 caractères d'identifiant, un point et un indice.
const MAX_DETAIL: usize = 16 * 1024;

#[allow(dead_code, unreachable_pub)]
mod dto {
    // Relatifs à `fuzz/fuzz_targets/dto/` (module en ligne d'une racine de crate) : trois `../`.
    #[path = "../../../crates/lf-adapter-piste/src/dto/de.rs"] pub(crate) mod de;
    // idem : budget, decode, search, consult_text, article, juri, acco (ni oauth.rs, qui exige secrecy, ni error.rs)
}

fuzz_target!(|data: &[u8]| { /* str::from_utf8 ou return ; check::<T>(text, endpoint) pour SearchResponseDto "/search",
    ConsultTextDto "/consult/lawDecree", GetArticleResponseDto "/consult/getArticle", ConsultJuriResponseDto "/consult/juri",
    ConsultAccoResponseDto "/consult/acco" */ });

fn check<T: serde::de::DeserializeOwned>(text: &str, endpoint: &str);
```

`check` : `dto::decode::decode::<T>(text, endpoint).map(drop)` ; sur `Err`, exige `CorpusError::Decode` seul, détail préfixé par `endpoint`, ASCII sans caractère de contrôle, `len() <= MAX_DETAIL` ; puis `assert_eq!` avec un second décodage du même texte sur le même fil (garde `Drop` et déterminisme).

- [ ] **Step 1 : écrire la cible** (ni test unitaire, ni couverture, ni mutation : `fuzz/**` est exclu par `.cargo/mutants.toml`). Un chemin `#[path]` refusé : ajuster le nombre de `../`.
- [ ] **Step 2 : dépendances et verrou.** `cargo check --manifest-path fuzz/Cargo.toml` (génère le verrou) ; chaque version différente : `cargo update --manifest-path fuzz/Cargo.toml -p <paquet>@<obtenue> --precise <workspace>` ; contrôle `python3 -I` (tomllib) : paquets présents dans les deux verrous à versions égales.
- [ ] **Step 3 : vérifier.** `cargo check --manifest-path fuzz/Cargo.toml --locked` ; `make deny` (seul RUSTSEC-2026-0386 toléré, sur les deux graphes ; tout autre refus : **s'arrêter et rapporter**, repli = renoncer, justifié en Task 9) ; `make machete` ; essai court par le contrôleur, une fois : `cargo +nightly fuzz run dto_decode -- -max_total_time=60 -max_len=65536`.
- [ ] **Step 4 : CI.** `dto_decode` dans la matrice `target` de `weekly.yml` ; `make workflow-lint` si `zizmor` et `actionlint` sont disponibles.
- [ ] **Step 5 : commit** `test(fuzz): cible dto_decode par inclusion des DTO (04 §8.5)`.

### Task 9 : documentation du budget, de R13 et de D-24

Aucun code de production. Repères par `grep` (les numéros de ligne bougent).

**Files:**
- Modify: `docs/plan/04-adapter-piste.md`, `docs/plan/06-qualite.md`, `docs/plan/07-feuille-de-route.md`, `docs/plan/00-decisions.md`, `crates/lf-adapter-piste/src/lib.rs`, `crates/lf-adapter-piste/src/dto/de.rs` (en-tête)

- [ ] **Step 1 : 04 §8.5.** Limite 1 : « Correction décidée (R11), à faire au plan 2f » devient « **Levée au plan 2f** » : budget de 200 000 éléments (`dto/budget.rs`), 25 listes bornées (`de::bounded_list`, `de::capped_list`), `results` ≤ 100 à la lecture puis ≤ `page_size` au mapping (`ensure_fits`), chemin de l'élément refusé, tests `a_page_of_101_results_maps_to_decode_with_a_path` et `one_element_more_maps_to_decode_with_a_path`, TOC de 74 000 éléments, chiffres de la sonde (valeurs, date), sonde hors dépôt au lieu du « binaire de test dédié ». Point (c) : « pas de plafond de corps par endpoint en M2 » (D4, à revoir après V-75 et V-96). Limite 3 : « `numeroAffaire` est une liste stricte : un scalaire donne `Decode` ; test `numero_affaire_scalar_is_a_decode_error` ». Limite 5 : « **Levée au plan 2f (R13)** : `opt_string_or_number` ; nombre = valeur inconnue sans mémo ; autre type ou chaîne vide = `None` ». Tableau « Éléments par document » : budget de 200 000, plafond de 100 résultats.
- [ ] **Step 2 : 04 §6.6, §8.3, §8.4.** « Valeur reçue inconnue » : un type non chaîne ne fait plus échouer la page ; tableau des DTO : `null_default` ⇒ `bounded_list`, `typePagination` ⇒ `Option<String>` par `opt_string_or_number` ; §8.4, ligne : `| num et numParution : chaînes dans toutes les fixtures (78 chaînes, 34 null, 0 entier), entier possible (D-24) ; conteneurs[].num d'une convention est son IDCC | tous les endpoints | opt_string_or_number | num_accepts_int_and_string |`.
- [ ] **Step 3 : 06.** §1.4 (`fuzz_targets/{…}`) : `dto_decode` **livré** (plan 2f, inclusion `#[path]`, sans feature) ; lignes cargo-fuzz et job `fuzz` : `dto_decode` ajouté ; D-23 : « cible de fuzzing `dto_decode` (plan 2f) » ; D-24 : `numero_affaire_accepts_scalar_and_list` remplacé par `numero_affaire_scalar_is_a_decode_error`, et « `num_accepts_int_and_string` (un test par module de DTO qui lit `num` ou `numParution`) ».
- [ ] **Step 4 : 07 et 00.** T-M2-06 : retirer la cible `dto_decode` du « reste » ; T-M2-09 critère (1) « livré par le plan 2f (tâches 1 à 6) », critère (2) « R13 décidé : `opt_string_or_number` » ; « Découpage des plans suivants » : 2f = budget, R13 et D-24 livrés. 00 §11.1 (`grep -n "plan 2f"`) et la liste des cibles hebdomadaires (`grep -n "from_html"`) : `dto_decode`.
- [ ] **Step 5 : code.** `lib.rs` : la `reason` de l'`expect(dead_code)` dit « plan 2g » (au lieu de « plan 2e ») et cite `bounded_list`, `capped_list`, `budget` ; la liste des modules décrit `dto` avec le budget. En-tête de `dto/de.rs` : renvoi à `budget.rs`, `null_default` ne sert plus de liste.
- [ ] **Step 6 : vérifier.** `make fmt-check lint doc` ; `git diff --stat` limité aux fichiers listés ; `make -k ci`.
- [ ] **Step 7 : commit** `docs: R11, R13 et D-24 levées, cible dto_decode (04 §8.5, 06, 07)`.

---

## PR 2 — `map/` (tâches 10 à 33)

Commande de test commune : `cargo nextest run -p lf-adapter-piste <filtre>` (rouge, puis vert) ; puis `make lint`, `make cov`, `make -k ci`. Chaque nouveau module de `map/` est déclaré par la tâche qui le crée, sans ré-export (RI-12).

### Task 10 : `Diagnostics`, `Field`, `endpoint`, compteurs et `Stats::record_mapping`

04 §9.3, §12.5 ; 03 §1.1.

**Files:**
- Create: `crates/lf-adapter-piste/src/map/mod.rs`
- Modify: `crates/lf-adapter-piste/src/lib.rs` (`mod map;` entre `limit` et `memo` ; une ligne `map` dans « Modules » ; la `reason` cite « `map` (tous ses modules) et `Stats::record_mapping` : consommés par le mapping puis par `impl LegalCorpus` au plan 2g »), `crates/lf-adapter-piste/src/stats.rs`, `crates/lf-adapter-piste/src/corpus.rs` (comparaisons de test l. ~1080 et ~1097 : `StatsSnapshot { mojibake_repairs: 1, ..StatsSnapshot::default() }` et `{ mojibake_unrepaired: 1, ..StatsSnapshot::default() }`)

**Interfaces:**
- Produces (`map/mod.rs`) :

```rust
pub(crate) mod endpoint {   // gabarits passés à `classify` et `post_json` : ni URL ni paramètre
    pub(crate) const SEARCH: &str = "/search";             pub(crate) const LAW_DECREE: &str = "/consult/lawDecree";
    pub(crate) const KALI_TEXT: &str = "/consult/kaliText"; pub(crate) const GET_ARTICLE: &str = "/consult/getArticle";
    pub(crate) const JURI: &str = "/consult/juri";          pub(crate) const CNIL: &str = "/consult/cnil";
    pub(crate) const ACCO: &str = "/consult/acco";
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Diagnostics {   // que des u32, dans l'ordre de StatsSnapshot
    pub(crate) dropped_hits: u32, pub(crate) id_mismatch: u32, pub(crate) non_midnight_timestamps: u32,
    pub(crate) upstream_suffix_stripped: u32, pub(crate) origin_mismatch: u32, pub(crate) invalid_native_ecli: u32,
    pub(crate) invalid_nor: u32, pub(crate) toc_dropped: u32, pub(crate) invisible_chars_removed: u32,
}
impl Diagnostics {   // chacune : saturating_add sur son champ, sans if
    pub(crate) fn note_dropped_hit(&mut self);     pub(crate) fn note_id_mismatch(&mut self);
    pub(crate) fn note_non_midnight(&mut self);    pub(crate) fn note_suffix_stripped(&mut self);
    pub(crate) fn note_origin_mismatch(&mut self); pub(crate) fn note_invalid_ecli(&mut self);
    pub(crate) fn note_invalid_nor(&mut self);
    pub(crate) fn add_toc_dropped(&mut self, dropped: u32); pub(crate) fn add_invisible(&mut self, removed: u32);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Field { pub(crate) endpoint: &'static str, pub(crate) path: &'static str }
impl Field {
    pub(crate) fn decode(self, category: &'static str) -> CorpusError;   // Decode { detail: "<endpoint>: <path>: <category>" }
    pub(crate) fn domain(self, error: &DomainError) -> CorpusError;      // self.decode(category(error))
}
fn category(error: &DomainError) -> &'static str; // InvalidId | BadSuffix ⇒ "invalid identifier" ; InvalidDate ⇒ "invalid date" ;
                                                  // TextTooLarge ⇒ "text too large" ; TocTooDeep ⇒ "table of contents too deep" ; _ ⇒ "invalid value"
pub(crate) fn not_found(endpoint: &str) -> CorpusError;   // NotFound { what: endpoint.to_owned() }
```

- Produces (`stats.rs`) : `StatsSnapshot` gagne, après les deux champs existants et dans l'ordre de `Diagnostics`, neuf champs `u64` documentés (toujours `#[non_exhaustive]`) ; `Stats` gagne neuf `AtomicU64` ; `pub(crate) fn record_mapping(&self, endpoint: &str, diags: &Diagnostics)` ajoute chaque champ (`fn add(&AtomicU64, u32)`, `fetch_add(u64::from(n), Relaxed)`) puis journalise **une ligne par champ non nul**, champs `endpoint` et `count`, jamais d'identifiant :

| Champ | Niveau | Message |
|---|---|---|
| `dropped_hits` | `debug!` | `search hits dropped: invalid identifier or identifier of another fond` |
| `id_mismatch` | `warn!` | `returned identifier is missing or differs from the requested one` |
| `non_midnight_timestamps` | `warn!` | `timestamps not at midnight UTC were read as UTC calendar days` |
| `upstream_suffix_stripped` | `debug!` | `version suffix stripped from identifiers other than LEGITEXT` |
| `origin_mismatch` | `warn!` | `origin field disagrees with the identifier family` |
| `invalid_native_ecli` | `debug!` | `native ECLI rejected and left out` |
| `invalid_nor` | `debug!` | `NOR rejected and left out` |
| `toc_dropped` | `debug!` | `table of contents entries dropped: invalid or duplicate identifier` |
| `invisible_chars_removed` | `debug!` | `invisible or bidirectional characters removed from text` |

- [ ] **Step 1 : tests de `map/mod.rs`** (rouges ; les `DomainError` à variantes `#[non_exhaustive]` se fabriquent par `LegiTextId::parse`, `CivilDate::parse`, `Ecli::parse`).
  - `diagnostics_default_is_all_zero` (littéral à neuf zéros) ; `each_note_bumps_only_its_own_counter` : `[(fn(&mut Diagnostics), Diagnostics); 7]`, chaque `note_*` depuis `default()` ⇒ `Diagnostics { <champ>: 1, ..Default::default() }`.
  - `add_counters_accumulate_and_saturate` : `add_invisible(3)` puis `(4)` ⇒ 7 ; `add_toc_dropped(2)` puis `(5)` ⇒ 7 ; `u32::MAX` puis `+1` ⇒ `u32::MAX` pour les deux ; `note_dropped_hit` à `u32::MAX` ⇒ `u32::MAX`.
  - `not_found_is_the_endpoint_template` : `not_found("/consult/juri") == NotFound { what: "/consult/juri".into() }`.
  - `field_decode_writes_endpoint_path_and_category` : `Field { endpoint: "/consult/juri", path: "text.dateTexte" }.decode("unreadable date")` ⇒ `Decode { detail: "/consult/juri: text.dateTexte: unreadable date" }`.
  - `domain_errors_map_to_a_fixed_category` (chemin `text.id`) : `LegiTextId::parse("x")` et `("LEGITEXT000033205014_foo")` ⇒ `invalid identifier` ; `CivilDate::parse("x")` ⇒ `invalid date` ; `TextTooLarge { chars: 16_000_001 }` ⇒ `text too large` ; `TocTooDeep { max: 64 }` ⇒ `table of contents too deep` ; `Ecli::parse("x")` ⇒ `invalid value`.
  - `domain_detail_never_echoes_the_rejected_value` : `LegiTextId::parse("SECRET-VALUE-0001")` par `Field::domain` ⇒ détail sans `SECRET`.
  - `endpoint_templates_are_the_api_paths` : les sept constantes (`make lint` refuse une constante qu'aucun test n'emploie : pas de tolérance `dead_code` en test).
- [ ] **Step 2 : tests de `stats.rs`** (rouges ; réutiliser `Capture`/`capturing`).
  - `new_stats_start_at_zero` mis à jour : onze champs à 0.
  - `record_mapping_adds_every_counter_to_its_own_field` : `Diagnostics { dropped_hits: 2, id_mismatch: 3, non_midnight_timestamps: 5, upstream_suffix_stripped: 7, origin_mismatch: 11, invalid_native_ecli: 13, invalid_nor: 17, toc_dropped: 19, invisible_chars_removed: 23 }` ⇒ mêmes valeurs dans les champs homonymes, mojibake à 0 ; un second appel les double.
  - `record_mapping_logs_each_diagnostic_at_its_level` : chaque champ seul à 42 ⇒ exactement une ligne, du niveau du tableau, avec le fragment du message, `count=42` et `endpoint="/consult/juri"`.
  - `record_mapping_is_silent_without_diagnostics` ; `mapping_log_carries_no_identifier_or_body_text` : neuf champs non nuls ⇒ 9 lignes, 18 guillemets (ceux des `endpoint="…"`), ni `LEGITEXT` ni `JURITEXT`.
- [ ] **Step 3 : implémenter** (`map/mod.rs`, `stats.rs`, `corpus.rs`, `lib.rs`), vert, `make lint`, `make cov`, `make -k ci`.
- [ ] **Step 4 : commit** `feat(piste): Diagnostics, Field et compteurs de mapping (T-M2-07, infrastructure)`.

### Task 11 : `map/dates.rs` (règle e) et `map/fixtures.rs`

04 §9.1 (e), §8.2 ; 03 §6 ; 06 §2.3 ; T-M2-07 (« `Text` non convertible ⇒ `Decode` »).

**Files:**
- Create: `crates/lf-adapter-piste/src/map/dates.rs`, `crates/lf-adapter-piste/src/map/fixtures.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod dates;`, `#[cfg(test)] pub(crate) mod fixtures;`)

**Interfaces:**
- Consumes : `Field`, `Diagnostics`, `dto::de::{RawDate, opt_raw_date}`, `CivilDate::{from_unix_millis, parse}`, `open_ended`.
- Produces :

```rust
// map/dates.rs
/// `None` ⇒ `Ok(None)` ; illisible ⇒ `Err(field.decode("unreadable date"))` ; sentinelle (≥ 2222-01-01) ⇒ `Ok(None)`.
pub(crate) fn civil(raw: Option<&RawDate>, field: Field, diags: &mut Diagnostics) -> Result<Option<CivilDate>, CorpusError>;
/// Première date présente et hors sentinelle, dans l'ordre ; paresseux.
pub(crate) fn first_civil(candidates: &[(Field, Option<&RawDate>)], diags: &mut Diagnostics) -> Result<Option<CivilDate>, CorpusError>;
/// `computed` s'il est présent (même si sa conversion donne `None`), sinon `plain`.
pub(crate) fn prefer_computed(computed: (Option<&RawDate>, Field), plain: (Option<&RawDate>, Field), diags: &mut Diagnostics)
    -> Result<Option<CivilDate>, CorpusError>;
fn from_millis(millis: i64, diags: &mut Diagnostics) -> Option<CivilDate>;
fn from_text(text: &str, diags: &mut Diagnostics) -> Option<CivilDate>;
fn is_utc_midnight(time: &str) -> bool;
const MILLIS_PER_DAY: i64 = 86_400_000; const DATE_LEN: usize = 10; const UNREADABLE: &str = "unreadable date";
const UTC_ZONES: [&str; 3] = ["+0000", "+00:00", "Z"];
// map/fixtures.rs (cfg(test)) : fixtures brutes, jamais réparées
pub(crate) const NAMES: [&str; 16];   // triés : consult_{acco,article,cnil,juri,juri_cetat,juri_constit,kali,loda}, search_{acco,cetat,cnil,code,constit,juri,kali,loda}
const FIXTURES: [(&str, &str); 16];   // (nom, include_str!("../../tests/fixtures/reference/<nom>.json"))
pub(crate) fn text(name: &str) -> &'static str;        // .expect("known fixture name")
pub(crate) fn value(name: &str) -> serde_json::Value;  // .expect("fixture is JSON")
```

Règles : (1) `civil` : convertir ; échec ⇒ `field.decode(UNREADABLE)` ; puis `open_ended`. (2) `from_millis` : `CivilDate::from_unix_millis(millis)?` ; `millis.rem_euclid(MILLIS_PER_DAY) != 0` ⇒ `note_non_midnight()`. (3) `from_text` : `trim()` ; `as_bytes().get(DATE_LEN)` : `None` ⇒ `CivilDate::parse(trimmed).ok()` ; `Some(b'T')` ⇒ date = `parse(trimmed.get(..DATE_LEN)?)`, heure = `trimmed.get(DATE_LEN + 1..)?`, `note_non_midnight()` si `!is_utc_midnight(heure)` ; autre octet ⇒ `None` ; jamais d'indexation par plage (`trimmed[..]` panique sur `2024-03-1é`). (4) `is_utc_midnight` : retirer le premier suffixe de `UTC_ZONES` qui convient, puis `starts_with("00:00:00")` et tous les octets dans `0`, `:`, `.`.

- [ ] **Step 1 : tests de `fixtures.rs`** (rouges) : `fixtures_names_are_sorted_and_unique` (16, tri strict) ; `every_fixture_is_a_json_object` ; `text_returns_the_raw_file` (`text("consult_loda")` commence par `{` et contient `"LEGITEXT000033205014_08-09-2023"`).
- [ ] **Step 2 : tests de `dates.rs`** (rouges). Aides : `AT = Field { endpoint: endpoint::JURI, path: "text.dateTexte" }`, `d(y, m, day)`, `run(raw) -> (Result<Option<CivilDate>, CorpusError>, Diagnostics)`, `raw_at(fixture, pointer)` (`fixtures::value(..).pointer(..)` puis `de::opt_raw_date(leaf.clone()).unwrap().unwrap()`). Détail attendu partout : `"/consult/juri: text.dateTexte: unreadable date"`.
  - `sentinel_occurrences_in_fixtures_become_none` (06 §2.3) : chaque feuille de date des 16 fixtures (`is_date_key(key) = key.starts_with("date") || key == "debut" || key == "fin"`, parcours récursif de `Value`) passe `opt_raw_date` puis `civil(Some(&raw), AT, ..)` en `Ok` ; une sentinelle se reconnaît à son littéral brut (`Millis(32_472_144_000_000)`, `Text` commençant par `2999-01-01` sans ou avec `T`) et donne `Ok(None)`, toute autre date `Ok(Some(_))`. Assertions : `leaves == 235`, `absent == 154` (150 `null` + 4 chaînes vides), `sentinels == [11, 6, 5]` (ms, AAAA-MM-JJ, ISO), `dates == 59`, `diags.non_midnight_timestamps == 2` (`search_kali` `results[0|1].sections[0].dateVersion`).
  - `sentinel_2999_and_2222_are_none` : `Millis(32_472_144_000_000)`, `Millis(7_952_342_400_000)`, `Text("2999-01-01")`, `Text("2222-01-01")`, `Text("2999-01-01T00:00:00.000+0000")` ⇒ `Ok(None)` sans diagnostic ; `Millis(7_952_256_000_000)` et `Text("2221-12-31")` ⇒ 2221-12-31.
  - `iso_plus0000_string_parses` : `raw_at("search_loda", "/results/0/date")` ⇒ 2016-10-09 ; `…/datePublication` ⇒ 2016-10-08 ; `Text("2016-10-09T00:00:00+0000")`, `…Z`, `…000+00:00`, `Text("2016-10-09T00:00:00")` ⇒ 2016-10-09 ; aucun diagnostic.
  - `negative_ms_1804` : `Millis(-5_231_606_400_000)` ⇒ 1804-03-21 ; même date pour `raw_at("consult_article", "/article/textTitles/0/dateDebut")`, `…/article/context/titresTM/0/debut`, `raw_at("search_code", "/results/0/date")`.
  - `millis_vectors` : `17_193_600_000` ⇒ 1970-07-19 ; `1_475_884_800_000` ⇒ 2016-10-08 ; `1_710_460_800_000` ⇒ 2024-03-15.
  - `calendar_range_edges_are_decode_errors` : `Millis(-62_135_596_800_000)` ⇒ 0001-01-01 ; `-62_135_596_800_001`, `253_402_300_800_000`, `i64::MIN`, `i64::MAX` ⇒ `Err(Decode)` sans panique ; `253_402_214_400_000` (9999-12-31) ⇒ `Ok(None)`.
  - `text_that_is_not_a_date_is_a_decode_error` : `"15/03/2024"`, `"2024-13-45"`, `"2024-02-30"`, `"2024-03-15 00:00:00"`, `"2024-03-15t00:00:00"`, `"2024-03-1é"`, `"é2024-03-15T00:00:00"`, `"T00:00:00"` ⇒ détail exact ; `!detail.contains("2024")`.
  - `non_midnight_is_counted_not_refused` (compteur 1 chaque fois) : `Millis(1_710_460_800_000 + 3_600_000)` ⇒ 2024-03-15 ; `Millis(-1)` ⇒ 1969-12-31 ; `Text("2026-07-08T07:47:16.102+0000")` ⇒ 2026-07-08 ; `Text("2024-03-15T")` ⇒ 2024-03-15 ; `Text("2999-01-01T07:47:16.102+0000")` ⇒ `Ok(None)`. `yyyymmdd_digits_read_as_1970_with_a_diagnostic` : `Millis(20_240_315)` ⇒ 1970-01-01, compteur 1.
  - `is_utc_midnight_table` : vrai pour `00:00:00`, `00:00:00.000`, `00:00:00.000+0000`, `00:00:00+0000`, `00:00:00Z`, `00:00:00.000+00:00` ; faux pour `00:00:01.000+0000`, `00:00:00.001+0000`, `00:00:00.000+0200`, `07:47:16.102+0000`, `00:00`, `""`, `01:00:00`.
  - `civil_of_none_is_none` (diagnostics inchangés) ; `text_dates_tolerate_surrounding_blanks` (`" 2024-03-15 "`).
  - `first_civil_reads_the_computed_date_first` : `decode::<ConsultJuriResponseDto>(fixtures::text(name), endpoint)` ; candidats `[(COMPUTED, date_texte_computed), (RAW, date_texte)]` : `consult_cnil` ⇒ 2026-01-08 (`datePubliComputed` ⇒ 2026-01-14), `consult_juri` ⇒ 2025-06-04, `consult_juri_cetat` ⇒ 2026-05-29, `consult_juri_constit` ⇒ 2025-11-07.
  - `first_civil_skips_absent_and_sentinel_candidates` (`search_kali` résultat 0 ⇒ `None` ; `[sentinelle, absente, 2016-12-28]` ⇒ 2016-12-28) ; `first_civil_stops_at_the_first_date` (`[2024-03-15, Text("n'importe quoi")]` ⇒ 2024-03-15) ; `first_civil_reports_the_field_of_the_unreadable_candidate` ; `first_civil_of_nothing_is_none`.
  - `prefer_computed` (`COMPUTED = Field { JURI, "text.dateTexteComputed" }`, `PLAIN = Field { JURI, "text.dateTexte" }`) : `prefer_computed_takes_the_computed_date` (`Millis(1_748_995_200_000)` contre `Millis(1_762_473_600_000)` ⇒ 2025-06-04) ; `prefer_computed_falls_back_to_the_plain_date` (calculé absent ⇒ 2025-11-07, aussi pour `Text("2025-11-07")` ; les deux absents ⇒ `None`) ; `prefer_computed_sentinel_is_none` (calculé `Millis(32_472_144_000_000)` et brut lisible ⇒ `None`) ; `prefer_computed_unreadable_names_the_chosen_field` (calculé `Text("sekret-42")` ⇒ `…text.dateTexteComputed: unreadable date` ; calculé absent, brut `Text("sekret-42")` ⇒ `…text.dateTexte: unreadable date` ; `Debug` sans `sekret`).
- [ ] **Step 3 : implémenter** (`UTC_ZONES` et le test de minuit dans `is_utc_midnight`, jamais dans `civil`), vert, `make cov`, `make -k ci`.
- [ ] **Step 4 : commit** `feat(piste): dates brutes vers dates civiles, sentinelles et horodatages (T-M2-07, règle e)`.

### Task 12 : table des 16 fixtures, `decode_as` et `dto_parses_every_fixture`

04 §13.3 ; 06 §2.3 ; D-11, D-23 (S6-1, avancée : RI-11). Aucune logique de production.

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/fixtures.rs`

**Interfaces:**
- Produces (`pub(crate)`, `cfg(test)`) :

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub(crate) enum Kind { Search(Fond), LawDecree, KaliText, GetArticle, Juri, Cnil, Acco }
pub(crate) struct Fixture { pub(crate) name: &'static str, pub(crate) endpoint: &'static str, pub(crate) kind: Kind, pub(crate) requested: &'static str }
impl Fixture { pub(crate) fn bytes(&self) -> &'static [u8]; }          // text(self.name).as_bytes()
pub(crate) static ALL: [Fixture; 16];                                    // dans l'ordre de NAMES
pub(crate) fn fixture(name: &str) -> &'static Fixture;
pub(crate) fn decoded(fixture: &Fixture, repair: bool) -> DecodedBody;   // decode_body(bytes, endpoint, repair).expect("fixtures are UTF-8")
/// Chemin de production : `decoded(fixture(name), true)` puis `decode` à l'endpoint de la fixture.
pub(crate) fn decode_as<T: DeserializeOwned>(name: &str) -> T;
```

`ALL` (`name, endpoint, kind, requested`) : les 8 `search_<fond>` ⇒ `endpoint::SEARCH`, `Search(Fond::<fond>)`, `""` ; `consult_loda` ⇒ `LAW_DECREE`, `LawDecree`, `LEGITEXT000033205014` ; `consult_kali` ⇒ `KALI_TEXT`, `KaliText`, `KALITEXT000049878950` ; `consult_article` ⇒ `GET_ARTICLE`, `LEGIARTI000006419288` ; `consult_juri` ⇒ `JURI`, `JURITEXT000051743650` ; `consult_juri_cetat` ⇒ `CETATEXT000054178503` ; `consult_juri_constit` ⇒ `CONSTEXT000052555275` ; `consult_cnil` ⇒ `CNIL`, `CNILTEXT000053352643` ; `consult_acco` ⇒ `ACCO`, `ACCOTEXT000053643828`.

- [ ] **Step 1 : tests** (rouges).
  - `fixtures_table_lists_the_sixteen_reference_files` : `ALL.len() == 16`, noms égaux à `NAMES` dans l'ordre, `bytes()` non vide, chaque ligne égale à `(endpoint, kind, requested)` écrits en dur (lit `requested` et la charge de `Kind::Search`, exigé par `dead_code` en test).
  - `mojibake_detection_per_fixture` (M7) : `(detected, repaired)` de `decoded(f, true)` = `(true, true)` pour `consult_article`, `consult_juri`, `consult_loda`, `search_code`, `search_juri`, `search_loda` ; `(true, false)` pour `consult_juri_constit` ; `(false, false)` pour les 9 autres ; `decoded(f, false)` ⇒ `repaired == false` et `text.as_bytes() == f.bytes()`. Ce test fait foi sur M7.
  - `dto_parses_every_fixture` (par `decode_as`, valeurs affirmées) : `search_*` : `results.len()`, `total_result_number`, `total_article_result_number`, `type_pagination` (acco 2/54491/0/`DEFAUT` ; cetat 3/51531 ; cnil 2/1134 ; code 1/1/1/`ARTICLE` ; constit 3/1817 ; juri 2/122147 ; kali 2/27337 ; loda 2/1786), `titles[0].id` (`LEGITEXT000033205014_08-09-2023`, `LEGITEXT000033696785_28-12-2016` pour loda, `LEGITEXT000006070721_09-04-2026` pour code), extraits par résultat (acco [2,2], cetat [2,2,2], cnil [0,0], code [1], constit [0,0,0], juri [4,4], kali [1,3], loda [0,0]) ; `consult_loda` : `id`, 3 sections de tête, 26 sections et 62 articles au total, 0 conteneur ; `consult_kali` : 1 section, 1 article, `conteneurs[0].num == Some("2121")` ; `consult_article` : `num == Some("9")`, 1 `textTitles` `CODE`, 2 `titresTM`, 1 `titreTxt`, `texte_html == None` ; `consult_juri` : `num == Some("12500399")`, `numero_affaire == ["24-13.470"]`, 1 sommaire, 2 liens, ECLI ; `consult_juri_cetat` : `num == Some("506507")`, 6 sommaires, 0 lien ; `consult_juri_constit` : `num == Some("2025-1173")`, 0 sommaire ; `consult_cnil` : `num == Some("SAN-2026-002")`, sommaire et liens vides ; `consult_acco` : `code_idcc == Some("275")`, 1 thème, 3 syndicats, `date_fin == Some(RawDate::Millis(32_472_144_000_000))`.
- [ ] **Step 2 : implémenter** (`expect` pour les échecs, aucun bras jamais pris), vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `test(adapter): table des 16 fixtures et dto_parses_every_fixture (T-M2-10, en partie)`.

### Task 13 : `map/rules.rs`, règles (a) et (f) : enveloppe, identité, texte long

04 §9.1 (a), (f), §8.4 (« enveloppe `null` ») ; T-M2-07 (« enveloppes `null` ⇒ `NotFound` »).

**Files:**
- Create: `crates/lf-adapter-piste/src/map/rules.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod rules;`)

**Interfaces:**
- Consumes : `not_found`, `Field`, `Diagnostics::add_invisible`, `LongText::best_of`.
- Produces :

```rust
pub(crate) trait Identified { fn identity(&self) -> [Option<&str>; 3]; }
// ConsultTextDto : [id, cid, title] ; ArticleDto : [id, cid, None] ; JuriTextDto (juri, cnil) : [id, titre, titre_long] ; AccordDto : [id, titre_texte, None]
/// Règle (a) : enveloppe absente ou `null`, ou objet sans `id`, `cid` ni titre non blanc ⇒ `NotFound { what: endpoint }`.
pub(crate) fn require<T: Identified>(candidate: Option<T>, endpoint: &str) -> Result<T, CorpusError>;
fn any_present(fields: &[Option<&str>]) -> bool;   // au moins un champ non vide après trim
/// Règle (f) : `LongText::best_of(html, plain)`, puis `add_invisible(invisible_removed())` ; erreur ⇒ `field.domain(&e)`.
pub(crate) fn long_text(html: Option<&str>, plain: Option<&str>, field: Field, diags: &mut Diagnostics) -> Result<LongText, CorpusError>;
```

Emplois : `require(Some(dto), LAW_DECREE | KALI_TEXT)`, `require(dto.article, GET_ARTICLE)`, `require(dto.text, JURI | CNIL)`, `require(dto.acco, ACCO)`.

- [ ] **Step 1 : tests** (rouges ; corps par `dto::decode::decode`).
  - `consult_empty_envelope_maps_to_not_found` (04 §13.2 « ×6 ») : `NotFound { what: <gabarit> }` pour `{}` (lawDecree, kaliText), `{"article":null}`, `{}`, `{"article":{}}` (getArticle), `{"text":null}`, `{"text":{}}` (juri puis cnil), `{"acco":null}`, `{}`, `{"acco":{}}` (acco).
  - `object_without_identity_is_not_found` : `{"nature":"LOI","etat":"VIGUEUR"}` ; `{"article":{"num":"9","texte":"t"}}` ; `{"text":{"texte":"t","nature":"ARRET"}}` ; `{"acco":{"siret":"32862158600143"}}` ; `{"title":"  "}` et `{"title":"\u{3000}"}` ⇒ `NotFound`.
  - `any_one_identity_field_is_enough` (`Ok`, DTO inchangé, rien n'est parsé) : `{"id":"X"}`, `{"cid":"X"}`, `{"title":"X"}` ; `{"article":{"id":"X"}}`, `{"article":{"cid":"X"}}` ; `{"text":{"id":"X"}}`, `{"text":{"titre":"X"}}`, `{"text":{"titreLong":"X"}}` ; `{"acco":{"id":"X"}}`, `{"acco":{"titreTexte":"X"}}`.
  - `consult_fixtures_have_an_identity` : les 8 `consult_*` (par `fixtures::decode_as`) passent `require`.
  - `any_present_ignores_blank_and_absent_fields` : `[None, None, None]`, `[Some(""), Some("  "), None]`, `[]` faux ; `[None, Some(" x "), None]` vrai.
  - `long_text` (`TEXTE = Field { endpoint: endpoint::JURI, path: "text.texte" }`) : `long_text_prefers_html` (`("<p>a</p><p>b</p>", "a b")` ⇒ 2 paragraphes `a`, `b`) ; `long_text_falls_back_to_plain` (`(None, Some("Lorsque R<P, le taux T = 5 %."))` ⇒ texte identique, 1 paragraphe, `char_len` 29 ; `(Some("   "), Some("Plain"))` ⇒ `Plain`) ; `long_text_keeps_a_complete_html` (`(Some("<p>Alinéa un.</p><p>Alinéa deux.</p>"), Some("Alinéa un. Alinéa deux."))` ⇒ `"Alinéa un.\n\nAlinéa deux."`, 2 paragraphes, `char_len` 24 : garde-fou 90 % à 100 %) ; `long_text_without_either_is_empty` ; `long_text_counts_invisible_chars` (plain `"a\u{202e}b"` ⇒ `"ab"`, compteur parti de 2 ⇒ 3 ; html `"<p>a&#173;b</p>"` ⇒ +1) ; `long_text_too_large_is_decode_without_the_text` (plain `"a".repeat(16_000_001)` ⇒ `Decode { detail: "/consult/juri: text.texte: text too large" }` ; seul test du module à 16 Mio).
- [ ] **Step 2 : implémenter** (`require` = `filter(any_present(identity))`, sinon `not_found(endpoint)` ; quatre `impl Identified` d'une ligne), vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(piste): enveloppe vide, objet sans identité et texte long (T-M2-07, règles a et f)`.

### Task 14 : règles (b), (c), (d) : identifiant renvoyé, suffixe de version, `origine`

04 §9.1 (b)(c)(d), §8.4 (suffixe `_JJ-MM-AAAA`) ; 03 §2.4 ; 06 D-28 ; 00 §4.

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/rules.rs`

**Interfaces:**
- Produces :

```rust
/// (b)(c) : `returned` absent ou blanc ⇒ demandé + `id_mismatch` ; sinon `parse_counted::<T>`, erreur ⇒ `field.domain(&e)` ;
/// lu ≠ demandé ⇒ l'entité garde le lu + `id_mismatch`.
pub(crate) fn resolve_id<T>(requested: &T, returned: Option<&str>, field: Field, diags: &mut Diagnostics) -> Result<T, CorpusError>
where T: FromStr<Err = DomainError> + Display + PartialEq + Clone;
/// Casse, blancs, suffixe bien formé tolérés (retiré) ; compte un suffixe retiré d'un identifiant autre que LEGITEXT. Partagé avec la recherche.
pub(crate) fn parse_counted<T>(raw: &str, diags: &mut Diagnostics) -> Result<T, DomainError> where T: FromStr<Err = DomainError> + Display;
fn suffix_dropped(raw: &str, normalized: &str) -> bool;   // raw.contains('_') && !normalized.starts_with("LEGITEXT")
/// Date du suffixe d'un LEGITEXT, après `open_ended` ; tout autre identifiant ou suffixe illisible ⇒ None.
pub(crate) fn suffix_start(raw: &str) -> Option<CivilDate>;
/// (d), contrôle seul : `origin` absent ou blanc, famille non mesurée, ou accord (casse ignorée, après trim) ⇒ rien ; sinon `origin_mismatch`.
pub(crate) fn check_origin(origin: Option<&str>, id: &str, diags: &mut Diagnostics);
fn expected_origin(id: &str) -> Option<&'static str>;   // ORIGINS sur id.get(..8)
const ORIGINS: [(&str, &str); 7] = [("LEGITEXT","LEGI"), ("JURITEXT","JURI"), ("CONSTEXT","CONSTIT"), ("CETATEXT","CETAT"),
                                    ("CNILTEXT","CNIL"), ("KALITEXT","KALI"), ("ACCOTEXT","ACCO")];
```

- [ ] **Step 1 : tests** (rouges ; `ID = Field { endpoint: endpoint::LAW_DECREE, path: "id" }`).
  - `suffix_is_version_date` : `consult_loda`, `id` `LEGITEXT000033205014_08-09-2023` : `suffix_start` ⇒ 2023-09-08 = `civil` de `dateDebutVersion` ; `resolve_id(&LEGITEXT000033205014, Some(id), ..)` ⇒ `Ok(LEGITEXT000033205014)`, `Diagnostics::default()` ; `search_loda` résultat 1 (`LEGITEXT000033696785_28-12-2016`) ⇒ 2016-12-28 = `date` du hit.
  - `suffix_start_reads_only_a_legitext_suffix` : sans suffixe ⇒ `None` ; `" legitext000033205014_08-09-2023 "` ⇒ 2023-09-08 ; `_01-01-2999`, `_01-01-2222` ⇒ `None` ; `_31-12-2221` ⇒ 2221-12-31 ; `JURITEXT000048465611_01-01-2000` ⇒ `None` ; `LEGITEXT000033205014_31-02-2023` ⇒ `None`.
  - `matching_ids_leave_no_diagnostic` : les 8 `consult_*` (`AccoTextId`, `ArticleId`, `CnilTextId`, `DecisionId` ×3, `KaliTextId`, `LegiTextId`) avec l'identifiant demandé égal ⇒ `Ok`, `Diagnostics::default()`.
  - `returned_id_differing_from_requested_is_kept_and_counted` : `LEGITEXT000033205014` demandé, `LEGITEXT000006070721` renvoyé ⇒ renvoyé, `id_mismatch == 1` ; `JURITEXT000051743650` demandé, `CETATEXT000054178503` renvoyé (`DecisionId`) ⇒ `Cetat(..)`, 1 ; `requested_kaliarti_with_legiarti_body_keeps_the_returned_id` : `KALIARTI000049886059` demandé, `LEGIARTI000006419288` renvoyé ⇒ `ArticleId::Legi(..)`, 1 (test synthétique ; la parité de la variante golden est en Task 24).
  - `missing_or_blank_returned_id_falls_back_to_requested` : `None`, `Some("")`, `Some("  ")` ⇒ demandé, `id_mismatch == 1`.
  - `malformed_returned_id_is_a_decode_error_without_the_value` : `Some("LEGITEXT0000")` et `Some("JURITEXT000051743650")` pour un `LegiTextId` ⇒ `Decode { detail: "/consult/lawDecree: id: invalid identifier" }`, sans la valeur.
  - `upstream_suffix_on_other_prefix_is_stripped_with_diagnostic` (06 D-28) : `ACCOTEXT000053643828_01-01-2020` ⇒ sans suffixe, `upstream_suffix_stripped == 1`, `id_mismatch == 0` ; `LEGIARTI000006419288_08-09-2023`, `CETATEXT000054178503_01-01-2020`, `" accotext000053643828_01-01-2020 "`, `parse_counted::<DocumentId>("CNILTEXT000053352643_08-01-2026")` ⇒ comptés ; sans suffixe ⇒ 0 ; `legitext_suffix_is_not_counted` (`LegiTextId` et `DocumentId`).
  - `malformed_suffix_is_refused_and_not_counted` : `JURITEXT000051743650_foo`, `ACCOTEXT000053643828_31-02-2023`, `LEGITEXT000033205014_` ⇒ `Err(BadSuffix { .. })` (`parse_counted`), `invalid identifier` (`resolve_id`), compteur 0 ; `suffix_dropped_table` (trois paires).
  - `search_fixture_hit_ids_parse_without_diagnostic` : les 17 `titles[0].id` passent `parse_counted::<DocumentId>` ; 3 suffixés, tous LEGITEXT ; compteur 0 ; préfixes LEGITEXT ×3, JURITEXT ×2, CONSTEXT ×3, CETATEXT ×3, CNILTEXT ×2, ACCOTEXT ×2, KALITEXT ×2.
  - `origine_mismatch_emits_diagnostic` : `(Some("CETAT"), JURITEXT…)` ⇒ 1 ; `(Some("JURI"), LEGITEXT…)` ⇒ 1 ; `(Some("juri "), JURITEXT…)` ⇒ 0 ; `None`, `""`, `"  "` ⇒ 0 ; `JORFTEXT000033202746`, `LEGIARTI000006419288`, `KALIARTI000005805295` ⇒ 0 ; id `"JURI"` ⇒ 0 sans panique.
  - `origin_matches_on_every_fixture` : 4 décisions (`JuriTextDto.origine` contre `id`) et 17 hits (`SearchResultDto.origin` contre `titles[0].id`) ⇒ 0 ; `expected_origin` rend une valeur pour les 7 préfixes.
- [ ] **Step 2 : implémenter** : `resolve_id` = `returned.filter(|raw| !raw.trim().is_empty())` ; `None` ⇒ `note_id_mismatch` + `requested.clone()` ; `Some(raw)` ⇒ `parse_counted(raw, diags).map_err(|e| field.domain(&e))?` puis `note_id_mismatch` si `parsed != *requested`. `parse_counted` : `raw.parse()?` puis `note_suffix_stripped` si `suffix_dropped(raw, &id.to_string())`. Vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(piste): identifiant renvoyé, suffixe de version et origine (T-M2-07, règles b à d)`.

### Task 15 : `map::search_page`, hits et identifiants

04 §9.1 (c, d, i), §9.2 ; 03 §4.1.

**Files:**
- Create: `crates/lf-adapter-piste/src/map/search.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod search;`)

**Interfaces:**
- Consumes : `SearchResponseDto::ensure_fits` (Task 3), `memo::received_mode`, `rules::{parse_counted, suffix_start, check_origin}`, `SearchHit::new`, `SearchPage::new`, `clean_field`, `status_of`, `Idcc::parse`.
- Produces :

```rust
pub(crate) fn search_page(dto: SearchResponseDto, query: &SearchQuery, diags: &mut Diagnostics) -> Result<SearchPage, CorpusError>; // = …_with_mode(..).map(|(page, _)| page)
pub(crate) fn search_page_with_mode(dto: SearchResponseDto, query: &SearchQuery, diags: &mut Diagnostics)
    -> Result<(SearchPage, Option<PaginationMode>), CorpusError>;
fn map_hit(result: &SearchResultDto, fond: Fond, diags: &mut Diagnostics) -> Option<SearchHit>;   // None = écarté (Result en Task 16)
fn start_hit(fond: Fond, raw_id: Option<&str>, diags: &mut Diagnostics) -> Option<SearchHit>;     // parse_counted::<DocumentId>(raw?, diags).ok()? puis SearchHit::new(fond, id, suffix_start(raw)).ok()
fn hit_cid(raw: Option<&str>) -> Option<DocumentId>;                                             // DocumentId::parse(raw?).ok(), sans contrôle de préfixe
```

Ordre de `search_page_with_mode` : (1) `dto.ensure_fits(endpoint::SEARCH, query.page_size())?` ; (2) `results.into_iter()`, `map_hit` ; `None` ⇒ `diags.note_dropped_hit()` et `dropped` local `saturating_add(1)` ; (3) `received_mode(dto.type_pagination.as_deref())` lu une fois ; (4) `SearchPage::new(fond, query.text().as_str(), total, total_articles, hits, dropped, received.unwrap_or_default())`, `Ok((page, received))`. `map_hit` : `start_hit` d'abord (un hit écarté n'est jamais lu plus loin), puis `check_origin(result.origin.as_deref(), hit.id.as_str(), diags)`, puis depuis `titles.first()` : `cid`, `title` ; `nature`, `raison_sociale` par `clean_field`, `etat = status_of(.., None)`, `idcc = ..and_then(Idcc::parse)`. Aucune valeur du corps journalisée ni propagée (`.ok()`).

- [ ] **Step 1 : tests** (rouges). Aides : `query(fond, page_size) -> SearchQuery` (texte `"responsabilité civile"`, page 1, `MatchMode::Any`), `dto(json)` (`decode(json, "/search")`), `page_of(fond, json) -> (SearchPage, Diagnostics)`, `day(y, m, d)`.
  - `results_beyond_the_page_size_are_a_decode_error` : page_size 2 ; 2 résultats ⇒ `Ok` ; 3 ⇒ `Decode { detail: "/search: results: more results than page size" }`, `Diagnostics` intact.
  - `invalid_hit_is_dropped_and_counted` (Juri, page_size 10, `totalResultNumber` 50) : (0) `JURITEXT000051743650` titre `"Un"` ; (1) `CNILTEXT000053352643` ; (2) `"titles": []` ; (3) `JURITEXT12` ; (4) `" juritext000048465611_01-01-2000 "` titre `"Suffixe toléré"` ; (5) `JURITEXT000048465611_31-02-2000` ; (6) `[{"title":"Sans id"}]`. Attendu : 2 hits (« Un », « Suffixe toléré » d'id `JURITEXT000048465611`, `version_start == None`), `page.dropped == 5`, `dropped_hits == 5`, `upstream_suffix_stripped == 1` (cas 4, RI-8), `page.total == 50`.
  - `a_dropped_hit_is_not_read_further` (`CNILTEXT…` en Juri avec `"date":"hier"` ⇒ `Ok`, 1 écarté) ; `dropped_hits_accumulate_in_the_diagnostics` (départ à 5, 2 écartés ⇒ 7, `page.dropped == 2`).
  - `degenerate_responses_map_without_panic` : `{}`, `{"results":null}`, `{"results":[]}` ⇒ 0 hit, `total == 0`, `total_articles == None`, `Defaut` ; `[{}]`, `[{"titles":[{}]}]`, `[{"titles":[{"id":""}]}]`, `[{"titles":[{"id":"   "}]}]` ⇒ 1 écarté.
  - `start_hit_checks_the_family_of_the_fond` : pour `Fond::ALL`, `<hit_prefix>000012345678` gardé, préfixe d'un autre fond écarté ; `LEGIARTI000006419288` en Loda écarté ; `None` écarté ; Code et Loda gardent la date du suffixe ; `LEGITEXT000033205014_31-02-2023` écarté.
  - `hit_origin_is_checked_and_never_drops` : `JURITEXT000051743650` en Juri, `origin` `"CETAT"` ⇒ gardé, `origin_mismatch == 1` ; `"JURI"` ⇒ 0.
  - `hit_cid_is_parsed_without_a_prefix_check` : `JORFTEXT000033202746` ⇒ `Some` ; `"  legitext000006070721 "` ⇒ `LEGITEXT000006070721` ; `"xx"`, `""`, `None` ⇒ `None` ; dans une page, `cid` invalide ⇒ hit gardé, `cid == None`.
  - `hit_text_fields_are_cleaned_and_typed` (Loda) : `"LOI <mark>numérique</mark>  (1)"` ⇒ `"LOI numérique (1)"` ; `" <mark>LOI</mark> "` ⇒ `"LOI"` ; `" vigueur_eten "` ⇒ `VigueurEtendue` ; `" ACME  <mark>SA</mark> "` ⇒ `"ACME SA"` ; vides ⇒ `None` ; `"FOO_BAR"` ⇒ `Other(..)`.
  - `idcc_accepts_string_and_number_in_a_hit` : `"275"` ⇒ 275 ; `1486` ⇒ 1486 ; `"0000000000002121"` ⇒ 2121 ; `"10000"`, `"12a"`, `0`, `2.5`, `null`, absent ⇒ `None`.
  - `invisible_characters_are_removed_from_hit_texts` : titre `"A\u{202e}B\u{200b}C"` ⇒ `"ABC"`, nature `"L\u{202e}OI"` ⇒ `"LOI"`, raison sociale `"X\u{ad}Y"` ⇒ `"XY"` ; `invisible_chars_removed == 0` (D17).
  - `total_comes_from_the_response_or_the_kept_hits` (`7` ⇒ 7 ; `0` ⇒ 0 ; absent + 1 gardé ⇒ 1 ; absent + 1 gardé sur 2 ⇒ 1) ; `total_articles_keeps_only_positive_counts` (`3`, `"3"` ⇒ `Some(3)` ; `0`, absent ⇒ `None`).
  - `search_page_with_mode_returns_the_received_mode` : `"DEFAUT"` ⇒ `(Defaut, Some(Defaut))` ; `"ARTICLE"`, `" article "` ⇒ `(Article, Some(Article))` ; `"WEIRD"`, absent, `null`, `7` ⇒ `(Defaut, None)` ; `search_page_is_search_page_with_mode_without_the_mode` (même page, mêmes diagnostics).
  - `the_page_carries_the_fond_and_the_query` : Juri, `"  responsabilité   civile "` ⇒ `page.query == "responsabilité civile"`.
- [ ] **Step 2 : implémenter** (`# Errors` sur les deux fonctions publiques ; aucun cast, `usize::try_from`), vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): map::search_page, hits et identifiants (T-M2-07, D-30)`.

### Task 16 : dates, extraits et articles d'un hit

04 §9.1 (e), §9.2 (`date`, `snippet`, `articles`) ; 03 §4.1, §7.3 ; E-26.

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/search.rs`

**Interfaces:**
- Consumes : `dates::first_civil`, `Snippet::new`, `ArticleId::parse`, `CodeArticleRef::new`.
- Produces : `map_hit(result, fond, diags) -> Result<Option<SearchHit>, CorpusError>` (propagé par `?`), puis :

```rust
const DATE: Field = Field { endpoint: endpoint::SEARCH, path: "results[].date" };  // idem DATE_PUBLICATION, DATE_SIGNATURE
fn hit_date(result: &SearchResultDto, diags: &mut Diagnostics) -> Result<Option<CivilDate>, CorpusError>; // first_civil(date, datePublication, dateSignature)
fn hit_snippet(result: &SearchResultDto) -> Option<Snippet>;           // text.and_then(Snippet::new).or_else(|| extract_snippet(..))
fn extract_snippet(sections: &[SearchSectionDto]) -> Option<Snippet>;  // sections ⇒ extracts ⇒ values.first() ⇒ find_map(Snippet::new)
fn hit_articles(fond: Fond, sections: &[SearchSectionDto]) -> Vec<CodeArticleRef>;
fn article_ref(fond: Fond, extract: &SearchExtractDto) -> Option<CodeArticleRef>;
```

`article_ref` : `ArticleId::parse(extract.id.as_deref()?).ok()?` ; `match (fond, &article_id) { (Fond::Code, ArticleId::Legi(_)) => clean_field(extract.num.as_deref()), (Fond::Kali, ArticleId::Kali(_)) => None, _ => return None }` ; extrait = `values.first()` (sinon `""`). Date, extrait et articles sont lus **après** le rejet du hit.

- [ ] **Step 1 : tests** (rouges ; `SearchResultDto { date: Some(RawDate::Text(..)), ..Default::default() }`).
  - `hit_date_takes_the_first_date_out_of_the_sentinel_zone` (textes `…T00:00:00.000+0000`) : (2016-10-09, 2016-10-08, -) ⇒ 2016-10-09 ; (-, 2026-01-14, -) ⇒ 2026-01-14 ; (-, -, 2025-12-19) ⇒ 2025-12-19 ; (2999-01-01, 2016-10-08, -) ⇒ 2016-10-08 ; (-, 2999-01-01, -) ⇒ `None` ; (`Millis(32_503_680_000_000)`, -, `Millis(1_475_884_800_000)`) ⇒ 2016-10-08 ; (`Millis(-5_231_606_400_000)`, -, -) ⇒ 1804-03-21 ; rien ⇒ `None`.
  - `an_unreadable_date_is_a_decode_error_with_its_path` : `Text("hier")` ou `Text("2024-13-45")` en `date`, `datePublication`, `dateSignature` ⇒ `"/search: results[].date: unreadable date"` (puis `.datePublication`, `.dateSignature`) ; `date` illisible avec `datePublication` valide ⇒ `Err` ; `no_date_text_appears_in_the_decode_error` (`Text("SECRET-DATE-VALUE")`) ; `later_dates_are_not_read_once_one_is_found` (`date` valide + `dateSignature` `"hier"` ⇒ `Ok`).
  - `snippet_prefers_the_text` (`"a <mark>b</mark> c"` + extrait `"zzz"` ⇒ `"a b c"`) ; `snippet_falls_back_to_the_first_extract_value` (`[{extracts: []}, {extracts: [{values: []}, {values: ["", "x"]}, {values: ["<mark>y</mark>", "z"]}]}]` ⇒ `"y"`) ; `blank_text_falls_back_to_the_extracts` ; `snippet_is_none_without_text_or_usable_extract` ; `snippet_is_truncated_to_280_chars` (280 `a` ⇒ 280 sans `…` ; 281 ⇒ 279 `a` + `…`).
  - `code_article_refs_come_from_the_extracts` (Code ; section 1 : `LEGIARTI000006419288` `num` `" 9 <mark>bis</mark> "` `values` `["[...]Chacun <mark>a</mark> droit[...]", "second"]`, extrait sans `id`, `KALIARTI000005805295`, `LEGIARTI12`, `LEGIARTI000006419289` sans `num` ni `values` ; section 2 : `LEGIARTI000006419290_01-01-2020` `num` `"4"` `values` `["<mark> </mark>"]`) ⇒ dans l'ordre `(Legi(…288), Some("9 bis"), "[...]Chacun a droit[...]")`, `(Legi(…289), None, None)`, `(Legi(…290), Some("4"), None)` ; extrait du hit = celui du premier.
  - `kali_article_refs_have_no_num` : `KALIARTI000005805295` (`num` `"8.4 de la convention)"`, `["v1"]`), `LEGIARTI000006419288` ignoré, `KALIARTI000005805297` (`num` `""`, `[]`) ⇒ `[Kali(…295), None, "v1"]`, `[Kali(…297), None, None]`.
  - `other_fonds_have_no_article_refs` : Juri, Constit, Cetat, Cnil, Acco, Loda ⇒ vide ; Code avec `KALIARTI…` seul et Kali avec `LEGIARTI…` seul ⇒ vide.
- [ ] **Step 2 : implémenter**, vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): dates, extraits et articles des hits de recherche (E-26, D-33)`.

### Task 17 : fixtures de recherche et golden

06 §2.3 ; T-M2-07 ; P-032. Tests seulement ; les fixtures n'imposent aucune production nouvelle (un rouge signale une erreur des Tasks 15-16 ou une valeur mal recopiée : re-mesurer avec `/var/tmp/m2f-plan/s2-measure.py` avant de toucher au code).

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/search.rs` (tests), `crates/lf-adapter-piste/src/map/fixtures.rs` (`golden`)

**Interfaces:**
- Produces : `pub(crate) fn golden() -> serde_json::Value` (`include_str!("../../tests/fixtures/reference/parite-golden.json")`, `.expect("golden is JSON")`) ; aide de test `fixture(fond, name) -> (SearchPage, Diagnostics)` = `fixtures::decode_as::<SearchResponseDto>(name)` puis `search_page(.., &query(fond, 10), ..)`.

- [ ] **Step 1 : tests.** Pour tous : `dropped == 0`, `diags == Diagnostics::default()`, `hit.fond` = fond demandé, `hit.id` sans suffixe.
  - `search_loda_fixture_maps` : `total == 1786`, `total_articles == None`, `Defaut`, 2 hits ; hit 0 : `LEGITEXT000033205014`, `version_start` 2023-09-08, `cid` `JORFTEXT000033202746`, titre `"LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)"`, date 2016-10-09, nature `"LOI"`, `Vigueur`, `snippet`/`idcc`/`raison_sociale` `None`, `articles` vide ; hit 1 : `LEGITEXT000033696785`, 2016-12-28, `cid` `JORFTEXT000033691882`, titre de 293 caractères commençant par `"Décret n° 2016-1849 du 23 décembre 2016 relatif à la carte mobilité inclusion"` et finissant par `"relative à l'Imprimerie nationale"`, date 2016-12-28, nature `"DECRET"`.
  - `search_code_fixture_maps` : `total == 1`, `total_articles == Some(1)`, `Article`, hit entier égal à `SearchHit { fond: Code, id: LEGITEXT000006070721, version_start: 2026-04-09, cid: Some(LEGITEXT000006070721), title: Some("Code civil"), date: 1804-03-21, nature: Some("code"), etat: Some(Vigueur), snippet: Some(S), idcc: None, raison_sociale: None, articles: [CodeArticleRef { Legi(LEGIARTI000006419288), Some("9"), Some(S) }] }`, `S = Snippet::new("[...]Chacun a droit au respect de sa vie privée. Les juges peuvent, sans préjudice de la réparation [...]")`.
  - `search_juri_fixture_maps` : `total == 122147` ; `JURITEXT000051743650`, `JURITEXT000048465611`, `cid == id`, nature `"arret"`, `etat`/`date` `None` ; titres `"Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin"`, `"Cour de cassation, civile, Chambre commerciale, 22 novembre 2023, 22-17.843, Publié au bulletin"` ; extrait 0 de 277 caractères, de `"[...] Les consorts [E] font grief à l'arrêt"` à `"es, les actions en responsab"`, sans `…` ; extrait 1 de **275**, finissant par `"du code des…"`.
  - `search_constit_fixture_maps` : `total == 1817`, natures `"dc"`, `"dc"`, `"qpc"`, titres `"Décision 2021-817 DC - 20 mai 2021 - Loi pour une sécurité globale préservant les libertés - Non conformité partielle - réserve"`, `"Décision 2018-769 DC - 04 septembre 2018 - Loi pour la liberté de choisir son avenir professionnel - Non conformité partielle"`, `"Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de publicité des débats devant le juge des libertés et de la détention statuant sur la détention provisoire du prévenu] - Conformité"` (sans U+00A0) ; extraits de 277, 278, 277 caractères finissant par `"du code de…"`, `"légalement conclues…"`, `"aucun autre…"`.
  - `search_cetat_fixture_maps` : `total == 51531`, `CETATEXT000054242861`, `CETATEXT000051655317`, `CETATEXT000054178503`, nature `"texte"`, titres `"CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429"`, `"CAA de NANCY, 3ème chambre, 22/05/2025, 22NC02259"`, `"Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507"` ; extraits de 239 (non coupé, finit par `"l'urbanisme \". 10. [...]"`), 275, 277.
  - `search_cnil_fixture_maps` : `total == 1134`, `CNILTEXT000053352643`, `CNILTEXT000053408671`, nature `"deliberation"`, `Vigueur`, dates 2026-01-14 et 2026-01-29, tiret demi-cadratin de `"SAN–2026-003"` gardé, extraits de 155 et 249 finissant par `"de la sanction 202. [...]"` et `"de la sanction 145. [...]"`.
  - `search_kali_fixture_maps` : `total == 27337` ; hit 0 `KALITEXT000005661929`, nature `"lettre d'adhesion"`, `Vigueur`, date `None`, extrait de 275 finissant par `"ensemble de ses…"`, `articles == [CodeArticleRef { Kali(KALIARTI000005807044), None, extrait du hit }]` ; hit 1 `KALITEXT000005661030`, `"avenant"`, `Abroge`, date `None`, extrait `"[...] Le présent avenant est codifié sous l'article 8.4 de la convention collective de la manutention portuaire. [...]"`, articles `KALIARTI000005805295`, `…297`, `…296`, `num` `None`, extraits de 118, 218, 213.
  - `search_acco_fixture_maps` : `total == 54491`, `ACCOTEXT000053643828`, `ACCOTEXT000051888026`, titres `"Accord d'entreprise relatif au télétravail"`, `"Accord Télétravail Hopium"`, `idcc` 275 et 1486, raisons sociales `"CORSAIR (Télétravail)"`, `"HOPIUM (Télétravail)"`, dates 2025-12-19 et 2025-02-07, nature et etat `None`, extraits de 215 (commence par `"[...]définitions liées au télétravail 4\n3.1. Définition du Télétravail 4"`, 5 `\n`) et 212 (`"[...]ACCORD TELETRAVAIL HOPIUM\nPRÉAMBULE 3"`, 4 `\n`).
  - `search_fixtures_keep_every_hit_and_drop_none` : 17 hits, aucun titre ni extrait ne contient `<` ni U+00A0.
  - `search_fixtures_match_the_golden_on_shared_fields` : `golden()["tool_output"]["search_<fond>.json"]` : `total`, et par hit `id`, `cid`, `nature`, `etat` (`as_api_code()`), `idcc` (`to_string()`), `raison_sociale`, `hit.contract().source_url.as_str()` égaux ; titre du golden réparé (`repair_latin1_mojibake`) égal après repli des blancs des deux côtés ; `articles` de même longueur sauf KALI (golden `[]`, nous 1 et 3 : E-26). Remplacé en 2g par `mapping_matches_golden`.
- [ ] **Step 2 : vérifier** (vert sans code nouveau hors `golden`), `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `test(adapter): fixtures de recherche et golden de map::search_page (P-032, E-26)`.

### Task 18 : `TextBody.cid` devient un `DocumentId` (domaine)

D20 **[P]** ; 03 §4.2, §4.6. Mesuré : `cid` de `consult_kali.json` = `KALITEXT000049878950` (= `id`) ; `cid` d'un code = `LEGITEXT000006070721` (`search_code.json`).

**Files:**
- Modify: `crates/lf-domain/src/entity/act.rs`, `docs/plan/03-domaine.md` (§4.2, état l. ~415, ligne `cid` l. ~424 ; §4.6), `docs/plan/04-adapter-piste.md` (§8.4, ligne « `cid` LODA = `JORFTEXT` ≠ `id` »)

**Interfaces:**
- Produces : `TextBody { cid: Option<DocumentId>, .. }` ; import `id::{DocumentId, KaliTextId, LegiTextId}` ; doc du champ : famille selon le texte (`JORFTEXT` loi ou décret, `LEGITEXT` code, `KALITEXT` égal à l'`id` pour une convention).

- [ ] **Step 1 : tests** (rouges, échec de compilation). `text_body_carries_typed_fields` : `cid: Some(DocumentId::parse("JORFTEXT000033202746").unwrap())`, `as_str()` et `prefix() == "JORFTEXT"` ; `text_body_cid_holds_every_text_family` : `LEGITEXT000006070721`, `JORFTEXT000033202746`, `KALITEXT000049878950` ⇒ `as_str() == raw`, `prefix()` attendu ; `text_body_default_is_empty` inchangé.
- [ ] **Step 2 : implémenter** ; `cargo nextest run -p lf-domain entity::act` ; `make cov` ; `make arch`.
- [ ] **Step 3 : documents.** 03 : `cid: Option<DocumentId>` (LODA `JORFTEXT`, code `LEGITEXT`, KALI `KALITEXT` = `id`) ; retirer « `TextBody.cid` pour KALI » des reportés. 04 §8.4 : parade `Option<DocumentId>`.
- [ ] **Step 4 : `make -k ci`**, commit `refactor(domain): TextBody.cid devient un DocumentId (cid de code et de convention)`.

### Task 19 : `map/body.rs`, fonctions des textes consolidés

04 §9.1 (c) ; 03 §4.2.

**Files:**
- Create: `crates/lf-adapter-piste/src/map/body.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`mod body;`)

**Interfaces:**
- Consumes : `Field`, `Diagnostics::note_invalid_nor`, `Nor::parse`, `DocumentId::parse`, `clean_field`.
- Produces (`pub(super)`, testées directement) :

```rust
pub(super) fn non_blank(raw: Option<&str>) -> Option<&str>;                                   // trim ; vide ⇒ None
pub(super) fn cid_of(raw: Option<&str>, endpoint: &'static str) -> Result<Option<DocumentId>, CorpusError>; // non_blank, DocumentId::parse ; erreur ⇒ Field { endpoint, path: "cid" }.domain(&e)
pub(super) fn nor_of(raw: Option<&str>, diags: &mut Diagnostics) -> Option<Nor>;            // non_blank, Nor::parse ; présent mais illisible ⇒ note_invalid_nor
pub(super) fn aliases_of(appellations: &[String]) -> Vec<String>;                          // clean_field par entrée, ordre gardé, vides écartés
```

- [ ] **Step 1 : tests** (rouges).
  - `non_blank_trims_and_drops_blank_text` : `None`, `Some("")`, `Some(" \n\t")` ⇒ `None` ; `Some("  x ")` ⇒ `Some("x")`.
  - `nor_is_validated_and_an_invalid_one_is_counted` : `Some("ecfi1524250l")` ⇒ `ECFI1524250L`, compteur 0 ; `Some("ECFI152425L")` ⇒ `None`, `invalid_nor == 1` ; `None`, `Some("  ")` ⇒ `None`, compteur inchangé ; compteur à 4 ⇒ 5.
  - `cid_accepts_every_text_family_and_rejects_the_rest` : `JORFTEXT000033202746`, `LEGITEXT000006070721`, `KALITEXT000049878950` ⇒ `Ok(Some(..))` du préfixe attendu ; `None`, `Some(" ")` ⇒ `Ok(None)` ; `Some("FOOTEXT000000000001")` ⇒ `Decode { detail: "/consult/lawDecree: cid: invalid identifier" }` ; `decode_details_never_contain_the_value` (`Some("SECRET-VALUE-42")` ⇒ `Debug` sans `SECRET`).
  - `aliases_drop_blank_entries_and_keep_order` : `["loi Lemaire", " ", "<br>", "loi <mark>numérique</mark>"]` ⇒ `["loi Lemaire", "loi numérique"]`.
- [ ] **Step 2 : implémenter**, vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): map/body, fonctions communes des textes consolidés (T-M2-07)`.

### Task 20 : `map/toc.rs`, brouillon de table des matières

04 §9.2 (TOC) ; 03 §4.2 ; D-25.

**Files:**
- Create: `crates/lf-adapter-piste/src/map/toc.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`mod toc;`)

**Interfaces:**
- Consumes : `Toc::build`, `TocDraft`, `SectionDraft`, `ArticleDraft`, `TocNode`, `Field::domain`, `Diagnostics::{add_toc_dropped, add_invisible}`, `fixtures::{decode_as, golden}`.
- Produces :

```rust
pub(super) fn build(sections: Vec<TocSectionDto>, articles: Vec<TocArticleDto>, endpoint: &'static str, diags: &mut Diagnostics)
    -> Result<Toc, CorpusError>;   // Toc::build(draft(..)), erreur ⇒ Field { endpoint, path: failure_path(&e) }.domain(&e) ;
                                   // puis add_toc_dropped(toc.dropped()) et add_invisible(invisible_in(toc.roots()))
fn draft(sections: Vec<TocSectionDto>, articles: Vec<TocArticleDto>) -> TocDraft;
fn section_draft(dto: TocSectionDto) -> SectionDraft;   // id, title, int_ordre, etat, sections (récursif), articles
fn article_draft(dto: TocArticleDto) -> ArticleDraft;   // id, num, int_ordre, etat ; content_html = content
fn failure_path(error: &DomainError) -> &'static str;   // TextTooLarge ⇒ "sections[].articles[].content" ; sinon "sections"
fn invisible_in(nodes: &[TocNode]) -> u32;              // somme saturée, récursive, de inline_text.invisible_removed()
```

Chaînes **déplacées**, jamais clonées (`content` peut peser plusieurs Mio) ; `dateDebut`/`dateFin` des DTO non lues.

- [ ] **Step 1 : tests** (rouges ; macro locale `text!`, comme `dto/consult_text.rs`).
  - `section_draft_copies_every_field` (titre `" Titre Ier "` non nettoyé ici, dates ignorées) ; `article_draft_takes_content_as_content_html` (`Some("<p>x</p>")` et `None` ; `id`, `num`, `etat`, `int_ordre` copiés ; `draft` : 2 sections + 1 article racine, ordre gardé).
  - `build_counts_dropped_articles_into_the_diagnostics` : `LEGIARTI000000000001`, `XYZ`, `LEGIARTI000000000001` ⇒ `toc.dropped() == 2`, `toc_dropped == 2` ; départ à 5 ⇒ 7.
  - `build_counts_invisible_characters_of_inline_texts` : racine `"<p>a\u{200b}b</p>"`, section fille `"<p>c\u{200b}\u{200b}d</p>"` ⇒ 3 ; `invisible_in(&[]) == 0` ; article sans `content` ⇒ 0.
  - `a_toc_of_64_levels_is_built` (`chain(64)` ⇒ `toc.len() == 64`) ; `a_toc_of_65_levels_is_a_decode_error` (`chain(65)` ⇒ `Decode { detail: "/consult/lawDecree: sections: table of contents too deep" }`) ; `chain(levels)` construit le DTO à la main (le JSON est plafonné par serde à 61-63).
  - `failure_path_names_the_field_of_every_error` : `TocTooDeep { max: 64 }` ⇒ `"sections"` ; `TextTooLarge { chars: 16_000_001 }` ⇒ `"sections[].articles[].content"` ; `EmptyQuery` ⇒ `"sections"` (catégorie `invalid value`, RI-18).
  - `toc_article_projection_matches_reference_order` : `build` sur `decode_as::<ConsultTextDto>("consult_loda")` ; les `article_id` des `TocEntryRef::Article` de `toc.page(0, NonZeroU32::new(1000).unwrap())` égalent, dans l'ordre, `golden()["normalizer"]["consult_loda.json"]["articles"][*]["article_id"]` : 62 identifiants, de `LEGIARTI000033205126` à `LEGIARTI000033205268` ; `consult_kali` ⇒ `["KALIARTI000049886059"]`. Discriminant : `LEGISCTA000033205020` (5 articles puis 3 sous-sections).
  - `toc_preserves_section_titles` (`consult_loda`) : `toc.len() == 88`, `page.total == 88`, `next_offset == None`, `dropped() == 0`, 26 sections et 62 articles, profondeurs `{1: 3, 2: 7, 3: 26, 4: 52}` ; trois premières entrées `(1, "Titre Ier : LA CIRCULATION DES DONNÉES ET DU SAVOIR")`, `(2, "Chapitre Ier : Economie de la donnée")`, `(3, "Section 1 : Ouverture de l'accès aux données publiques")`, puis l'article `LEGIARTI000033205126`, `num` `"1"`, profondeur 4 ; 62 `inline_text == None` ; racines `Titre Ier…`, `Titre II : LA PROTECTION DES DROITS DANS LA SOCIÉTÉ NUMÉRIQUE`, `Titre III : L'ACCÈS AU NUMÉRIQUE`.
  - `kali_inline_content_is_available_without_extra_call` (`consult_kali`) : `toc.len() == 2` (section `"Chapitre 1er : Recours au CDD d'usage"`, profondeur 1 ; article `KALIARTI000049886059`, profondeur 2, `VigueurEtendue`) ; `inline_text` commence par `"Les parties conviennent de préciser au niveau de la branche les conditions d'un recours légitime"`, sans `'<'` ni `"href"`, 1 paragraphe (source tronquée dans une balise `<a`, Ruling 4 du plan 2e). Si le vert diffère : ne pas contourner, consigner au ledger.
- [ ] **Step 2 : implémenter**, vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): map/toc, brouillon de table des matières depuis les DTO (T-M2-07)`.

### Task 21 : `body::text_body` et `map::act`

04 §9.1 (a, b, c, e, g), §9.2 (`Act`) ; D-20, D-21, D-25.

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/body.rs` (`text_body`), `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod act;`), `crates/lf-adapter-piste/src/dto/consult_text.rs` (doc de `jorf_text` : « Référence au Journal officiel, ex. `JORF n°0235 du 8 octobre 2016` : un libellé, pas un identifiant (`""` : alors `None`) »)
- Create: `crates/lf-adapter-piste/src/map/act.rs`

**Interfaces:**
- Consumes : Tasks 13, 14, 18, 19, 20 ; `dates::civil` ; `status_of`.
- Produces :

```rust
// body.rs — ordre d'évaluation figé : cid_of, puis civil de dateParution, dateDebutVersion, dateFinVersion (chemins de même nom), puis toc::build
pub(super) fn text_body(dto: ConsultTextDto, endpoint: &'static str, suffix_start: Option<CivilDate>, diags: &mut Diagnostics)
    -> Result<TextBody, CorpusError>;
// champs : nor_of(nor) ; clean_field sur title, nature, num_parution, jorf_text (jo_reference) ; status_of(etat, juris_state) ;
//          date_debut_version = suffix_start.or(date de dateDebutVersion) ; aliases_of(appellations)
// act.rs
pub(crate) fn act(dto: ConsultTextDto, requested: &LegiTextId, diags: &mut Diagnostics) -> Result<Act, CorpusError>;
// require(Some(dto), LAW_DECREE)? ; text_id = resolve_id(requested, dto.id.as_deref(), Field { LAW_DECREE, "id" }, diags)? ;
// suffix = dto.id.as_deref().and_then(rules::suffix_start) ; body = text_body(dto, LAW_DECREE, suffix, diags)?
```

- [ ] **Step 1 : tests de `text_body`** (rouges, `body.rs`).
  - `loda_status_falls_back_to_juris_state` : `(None, "Vigueur")` ⇒ `Vigueur` ; `("ABROGE", "Vigueur")` ⇒ `Abroge` ; `("  ", "VIGUEUR_ETEN")` ⇒ `VigueurEtendue` ; rien ⇒ `None`.
  - `short_fields_are_cleaned_and_never_empty` : `" Loi  n° 1 <mark>x</mark> "` ⇒ `"Loi n° 1 x"` ; `nature "  "` ⇒ `None` ; `num_parution " 0235 "` ⇒ `"0235"` ; `jorf_text "<i>JORF</i> n°0235"` ⇒ `"JORF n°0235"`.
  - `a_decode_error_names_the_first_failing_field` : `cid "FOO"` + `dateParution Text("hier")` ⇒ erreur du `cid` ; `cid` valide + deux dates illisibles ⇒ `dateParution` ; `dateDebutVersion` seule ⇒ `"/consult/lawDecree: dateDebutVersion: unreadable date"` (sans `hier`).
  - `text_body_builds_the_toc_from_sections_and_root_articles` (article racine + section ⇒ `roots().len() == 2`, `roots()[0]` est un `TocNode::Article`) ; `text_body_prefers_the_suffix_start` (`Some(2023-09-08)` et `dateDebutVersion "2020-01-01"` ⇒ 2023-09-08 ; `None` ⇒ 2020-01-01).
- [ ] **Step 2 : tests de `act.rs`** (rouges).
  - `act_fixture_maps` (`decode_as::<ConsultTextDto>("consult_loda")`, demandé `LEGITEXT000033205014`) : `text_id` sans suffixe ; `body.cid` `JORFTEXT000033202746` (`prefix() == "JORFTEXT"`) ; `nor == "ECFI1524250L"` ; `title == "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)"` ; `nature == "LOI"` ; `etat == Some(Vigueur)` ; `date_parution` 2016-10-08 ; `num_parution == "0235"` ; `date_debut_version` 2023-09-08 ; `date_fin_version == None` ; `jo_reference == "JORF n°0235 du 8 octobre 2016"` ; `aliases == ["loi Lemaire", "loi numérique", "loi Lemaire [2016]"]` ; `toc.len() == 88`, `dropped() == 0`, `roots().len() == 3` ; `diags == Diagnostics::default()` ; `contract().source_url` `https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014`, citation = titre.
  - `act_invalid_nor_is_none_with_a_diagnostic` (`"pas-un-nor"` ⇒ `None`, `invalid_nor == 1`) ; `act_empty_object_is_not_found` (`ConsultTextDto::default()`, `{ title: "  " }` ⇒ `NotFound { what: "/consult/lawDecree" }`, `diags` inchangé) ; `act_with_only_a_title_uses_the_requested_id` (`id_mismatch == 1`).
  - `act_keeps_the_returned_id_on_mismatch` (`LEGITEXT000006069414` ⇒ gardé, 1 ; `" legitext000033205014_08-09-2023 "` ⇒ 0, `upstream_suffix_stripped == 0`) ; `act_unparsable_id_is_a_decode_error` (`JORFTEXT000033202746`, `LEGITEXT000033205014_31-02-2023`, `"n'importe quoi"` ⇒ `"/consult/lawDecree: id: invalid identifier"`).
  - `act_version_start_is_the_suffix_then_date_debut_version` : suffixe `_08-09-2023` + `Text("2020-01-01")` ⇒ 2023-09-08 ; sans suffixe ⇒ 2020-01-01 ; `Text("2999-01-01")` sans suffixe ⇒ `None` ; suffixe `_01-01-2999` + `Text("2020-01-01")` ⇒ 2020-01-01 ; `Text("hier")` avec suffixe ⇒ `Decode` ; `dateFinVersion` `"2999-01-01"` ⇒ `None`, `"2024-05-01"` ⇒ 2024-05-01.
  - `act_nature_is_cleaned_before_the_contract` (demandé `LEGITEXT000006069414`) : `" code\n"` et `"<b>CODE</b>"` ⇒ `nature == "code"`/`"CODE"` et URL `…/codes/texte_lc/LEGITEXT000006069414` ; `"LOI"` ⇒ `…/loda/id/…`.
- [ ] **Step 3 : implémenter**, vert, `make cov`, `make -k ci`.
- [ ] **Step 4 : commit** `feat(adapter): map::act, texte de loi ou de décret consolidé (T-M2-07)`.

### Task 22 : `map::convention`

04 §9.2 (`Convention`) ; 03 §4.6 ; D-20, D-25.

**Files:**
- Create: `crates/lf-adapter-piste/src/map/convention.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod convention;`)

**Interfaces:**
- Produces :

```rust
pub(crate) fn convention(dto: ConsultTextDto, requested: &KaliTextId, diags: &mut Diagnostics) -> Result<Convention, CorpusError>;
fn bulletin_of(origine: Option<&str>, numero_bo: Option<&str>) -> Option<String>;   // clean_field(origine).or_else(|| clean_field(numero_bo))
fn container_id_of(cid: Option<&str>) -> Option<String>;                            // body::non_blank(cid).map(str::to_owned)
```

`convention` (`dto` en `mut`) : `require(Some(dto), KALI_TEXT)?` ; `resolve_id(requested, dto.id.as_deref(), Field { KALI_TEXT, "id" }, diags)?` ; **prendre** `conteneurs`, `numero_bo`, `origine_publi` (`mem::take`/`take`) avant `body::text_body(dto, KALI_TEXT, None, diags)?` ; premier conteneur ou `Default` : `idcc = num.and_then(Idcc::parse)`, `container_title = clean_field(titre)`, `container_id = container_id_of(cid)`.

- [ ] **Step 1 : tests** (rouges).
  - `convention_fixture_maps` (`decode_as::<ConsultTextDto>("consult_kali")`, demandé `KALITEXT000049878950`) : `body.cid` `KALITEXT000049878950` ; `nor == "ASET2450455M"` ; `title == "Annexe IX - Stipulations spécifiques applicables aux techniciens du spectacle et aux artistes interprètes, engagés pour la production de phonogrammes et vidéogrammes musicaux"` ; `nature`, `num_parution`, `date_debut_version`, `date_fin_version`, `jo_reference`, `date_parution` (sentinelle) `None` ; `etat == Some(VigueurEtendue)` ; `aliases` vide ; `idcc` 2121 ; `container_title == "Convention collective nationale de l'édition du 14 janvier 2000"` ; `container_id == "KALICONT000005635096"` ; `bulletin == "BO n°2024-20"` ; `toc.len() == 2`, `dropped() == 0` ; `diags == Diagnostics::default()` ; URL `https://www.legifrance.gouv.fr/conv_coll/id/KALITEXT000049878950`, citation = titre ; l'article `KALIARTI000049886059` a un `inline_text`.
  - `convention_reads_only_the_first_container` (`[{cid: "KALICONT000000000001"}, {cid: "…002", num: "2121", titre: "Autre"}]` ⇒ `idcc`/`container_title` `None`, `container_id` `…001` ; aucun conteneur ⇒ trois `None`).
  - `convention_invalid_idcc_is_none_without_diagnostic` (`"ABC"`, `"0"`, `"10000"` ⇒ `None`, `Diagnostics::default()` ; `"0000000000002121"` ⇒ 2121) ; `container_id_of` (`Some(" KALICONT000005635096 ")` ⇒ rogné ; `Some("  ")`, `None` ⇒ `None`).
  - `bulletin_falls_back_to_numero_bo` : `("BO n°2024-20", "2024-20")` ⇒ `"BO n°2024-20"` ; `(None, "2024-20")` ⇒ `"2024-20"` ; `("<br>", "2024-20")` ⇒ `"2024-20"` ; `(" ", None)`, `(None, None)` ⇒ `None`.
  - `convention_id_rules` : absent ⇒ demandé, `id_mismatch == 1` ; `KALITEXT000000000001` ⇒ gardé, 1 ; `KALITEXT000049878950_08-09-2023` ⇒ sans suffixe, `upstream_suffix_stripped == 1`, `id_mismatch == 0` ; `LEGITEXT000033205014`, `KALITEXT000049878950_99-99-9999` ⇒ `"/consult/kaliText: id: invalid identifier"` ; `ConsultTextDto::default()` ⇒ `NotFound { what: "/consult/kaliText" }`.
  - `convention_status_and_dates_follow_the_shared_rules` : `"ABROGE"` + `"VIGUEUR_ETEN"` ⇒ `Abroge` ; `dateDebutVersion "2024-01-01"` ⇒ 2024-01-01.
- [ ] **Step 2 : implémenter**, vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): map::convention, convention collective consolidée (T-M2-07)`.

### Task 23 : `map/article.rs`, règles propres à l'article

04 §9.1 (a, b, c, e, f, g, h), §9.2 (`ArticleText`) ; 03 §4.3. Faits mesurés (`consult_article.json`) : `id` = `cid` = `LEGIARTI000006419288` ; 1 `textTitles` (`CODE`, `Code civil`, `cid` = `id` = `LEGITEXT000006070721`) ; `titresTM` `Livre Ier : Des personnes`, `Titre Ier : Des droits civils` ; `dateDebut` `17193600000` (1970-07-19), `dateFin` sentinelle ; `texte` de 336 caractères bruts, 322 réparés ; `texteHtml`, `notaHtml` absents, `nota` `""` ; `sha256` `80897c89…dccb3e` (ne jamais reformater la fixture).

**Files:**
- Create: `crates/lf-adapter-piste/src/map/article.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod article;`)

**Interfaces:**
- Consumes : `rules::long_text`, `Field`, `ParentText::new`, `DocumentId::parse`, `clean_field`.
- Produces (privé au fichier) :

```rust
const ID: Field = Field { endpoint: endpoint::GET_ARTICLE, path: "article.id" };
// idem PARENT_CID ("article.textTitles[].cid"), PARENT_ID ("article.textTitles[].id"), DATE_DEBUT ("article.dateDebut"),
//      DATE_FIN ("article.dateFin"), TEXTE ("article.texte"), NOTA ("article.nota")
fn is_code(nature: Option<&str>) -> bool;                                      // trim().eq_ignore_ascii_case("CODE")
fn preferred_title(titles: &[TextTitleDto]) -> Option<&TextTitleDto>;          // première de nature CODE, sinon la première
fn parent_text(title: &TextTitleDto) -> Result<Option<ParentText>, CorpusError>; // premier non blanc de [(PARENT_CID, cid), (PARENT_ID, id)] ;
                                                                               // aucun ⇒ Ok(None) ; illisible ⇒ field.domain(&e) ; sinon ParentText::new(id, nature)
fn section_path(entries: &[ContextEntryDto]) -> Vec<String>;                   // clean_field(titre), vides omis
fn nota_of(html: Option<&str>, plain: Option<&str>, diags: &mut Diagnostics) -> Result<Option<LongText>, CorpusError>; // long_text(.., NOTA, ..), vide ⇒ None
```

- [ ] **Step 1 : tests directs** (rouges ; préfixe des détails `"/consult/getArticle: "`).
  - `code_nature_matching` : vrai pour `CODE`, `code`, `" Code "` ; faux pour `CODES`, `""`, `LOI`, `None`.
  - `preferred_title_picks_the_first_code_entry` : `[]` ⇒ `None` ; `[(LOI,"A"),(CODE,"B")]` ⇒ `"B"` ; `[(code,"A"),(CODE,"B")]` ⇒ `"A"` ; `[(LOI,"A"),(DECRET,"B")]` ⇒ `"A"` ; `[(sans nature,"A")]` ⇒ `"A"`.
  - `parent_text_rules` : `cid LEGITEXT000006070721` + `CODE` ⇒ `ParentText { LEGITEXT000006070721, Code }` ; `cid` absent ou `"  "` + `id` valide ⇒ idem ; `cid KALITEXT000005635534` sans nature ⇒ `Kali` ; `cid JORFTEXT000033202746` + `LOI` ⇒ `Text` ; ni `cid` ni `id` ⇒ `Ok(None)` ; `cid` valide + `id` illisible ⇒ `Ok(Some(..))` ; `cid "FOO"` ⇒ `"… article.textTitles[].cid: invalid identifier"` ; `cid` absent + `id "FOO"` ⇒ `"… article.textTitles[].id: invalid identifier"`.
  - `section_path_keeps_cleaned_titles_in_order` : `Livre <mark>Ier</mark>`, `"   "`, absent, `Titre II` ⇒ `["Livre Ier", "Titre II"]` ; `[]` ⇒ `[]`.
  - `nota_rules` : `(None, None)` ⇒ `None` ; `(None, "Nota de l'article")` ⇒ ce texte ; deux paragraphes HTML ⇒ `"Alinéa un.\n\nAlinéa deux."` ; `(None, "\u{200b}")` ⇒ `None`, compteur 1 ; `invisible_counts_accumulate_across_text_and_nota` (`long_text(None, "a\u{200b}", TEXTE)` puis `nota_of(None, "\u{200b}\u{200b}")` ⇒ 3).
- [ ] **Step 2 : implémenter**, vert, `make cov`.
- [ ] **Step 3 : commit** `feat(adapter): règles pures du mapping d'un article (titre, parent, chemin, nota)`.

### Task 24 : `map::article` sur la fixture réelle

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/article.rs`

**Interfaces:**
- Consumes : Task 23 ; `rules::{require, resolve_id}`, `dates::civil`, `LegalStatus::parse`.
- Produces :

```rust
/// Mappe la réponse de `/consult/getArticle` en `ArticleText` (04 §9.1, §9.2).
pub(crate) fn article(dto: GetArticleResponseDto, requested: &ArticleId, diags: &mut Diagnostics) -> Result<ArticleText, CorpusError>;
```

Assemblage sans décision locale, dans l'ordre des erreurs : `require(dto.article, GET_ARTICLE)?` ; destructuration d'`ArticleDto` ; `resolve_id(requested, id, ID, diags)?` ; `chosen = preferred_title(&text_titles)` ; `parent = chosen.map(parent_text).transpose()?.flatten()` ; `civil(date_debut, DATE_DEBUT)?`, `civil(date_fin, DATE_FIN)?` ; `rules::long_text(texte_html, texte, TEXTE, diags)?` ; `nota_of(nota_html, nota, diags)?` ; `num`, `code_title` (titre de `chosen`), `full_sections_titre` par `clean_field` ; `etat = etat.and_then(LegalStatus::parse)` ; `path = section_path(&context.titres_tm)`. Aide de test : `map_fixture(requested) -> (Result<ArticleText, CorpusError>, Diagnostics)` sur `decode_as::<GetArticleResponseDto>("consult_article")` ; `run(article_json, requested)` décode `{"article": <json>}`.

- [ ] **Step 1 : tests** (rouges).
  - `article_fixture_maps` : `LEGIARTI000006419288` ; `num` `"9"` ; `Vigueur` ; `code_title` `"Code civil"` ; `parent` `{ LEGITEXT000006070721, Code }` ; `path` `["Livre Ier : Des personnes", "Titre Ier : Des droits civils"]` ; `full_sections_titre` `"Livre Ier : Des personnes"` ; `date_debut` 1970-07-19 ; `date_fin` `None` ; `text.as_str()` = `"Chacun a droit au respect de sa vie privée. Les juges peuvent, sans préjudice de la réparation du dommage subi, prescrire toutes mesures, telles que séquestre, saisie et autres, propres à empêcher ou faire cesser une atteinte à l'intimité de la vie privée : ces mesures peuvent, s'il y a urgence, être ordonnées en référé."`, `char_len` 322, 1 paragraphe ; `nota` `None` ; `diags == Diagnostics::default()` (commentaire : le golden annonce `byte_size` 364, octets du mojibake, P-037).
  - `kaliarti_variant_keeps_the_returned_id_with_diagnostic` : `map_fixture("KALIARTI000049886059")` ⇒ `article_id` `LEGIARTI000006419288`, `id_mismatch == 1`, entité égale à celle du cas nominal ; `source_url` `https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288` (golden : `…/conv_coll/id/KALIARTI000049886059`, divergence `returned_id_kept`).
  - `article_citation` : `"Code civil, art. 9"`, URL `…/codes/article_lc/LEGIARTI000006419288`.
  - `article_identity_follows_the_shared_rules` : `{"article":null}`, `{}`, `{"executionTime":1,"dereferenced":false}`, `{"article":{}}`, `{"article":{"etat":"VIGUEUR","texte":"  ","textTitles":[{"nature":"CODE"}]}}`, `{"article":{"num":"9","texte":"t"}}` ⇒ `NotFound { what: "/consult/getArticle" }`, `diags` inchangé ; `{"cid":"LEGIARTI000006419288"}` ⇒ `Ok`, identifiant demandé, `id_mismatch == 1`, `text` vide ; `id` `"LEGIARTI000006419288_01-02-2003"` ⇒ 0, `upstream_suffix_stripped == 1` (RI-6) ; `id` `"LEGITEXT000006070721"` ⇒ `"… article.id: invalid identifier"`.
  - `article_without_html_uses_plain_text` (`"Lorsque R<P, le taux T = 5 %."` et `texteHtml "   "` ⇒ identique, 29) ; `code_title_prefers_nature_code` (`[{LOI, "Loi X", JORFTEXT000033202746}, {code, "Code civil", LEGITEXT000006070721}]` ⇒ `"Code civil"` ; sans `CODE` ⇒ `"Loi X"` ; vide ou `null` ⇒ `code_title` et `parent` `None`).
  - `parent_comes_from_the_same_text_title` : mêmes `textTitles` ⇒ parent `LEGITEXT000006070721`/`Code` ; sans `CODE` ⇒ `JORFTEXT000033202746`/`Text` ; `CODE` d'indice 1 à `cid "FOO"` ⇒ `"… article.textTitles[].cid: invalid identifier"` (l'indice 0 n'est jamais lu) ; entrée choisie sans `cid` ni `id` ⇒ `parent None`, `code_title` rempli.
  - `short_fields_are_cleaned` (`num " 9 <sup>bis</sup> "` ⇒ `"9 bis"` ; `fullSectionsTitre "  Livre   Ier  "` ⇒ `"Livre Ier"` ; titre `"Code <mark>civil</mark>"` ⇒ `"Code civil"` ; `num "   "`, `fullSectionsTitre "<br>"` ⇒ `None`) ; `status_code_is_typed` (`VIGUEUR`, `abroge`, `ABROGE_DIFF`, `NOUVEAU_CODE` ⇒ `Other`, `"  "` et absent ⇒ `None`).
  - `dates_go_through_the_shared_converter` : `dateDebut 17197200000` ⇒ 1970-07-19, `non_midnight_timestamps == 1` ; `dateFin "hier"` ⇒ `"… article.dateFin: unreadable date"` ; `dateDebut "hier"` ⇒ `"… article.dateDebut: …"` ; `dateFin "2222-01-01"` ⇒ `None`, `"2221-12-31"` ⇒ 2221-12-31.
  - `nota_prefers_html_then_plain` ; `oversized_text_or_nota_is_a_decode_error` (`texte` de `usize::try_from(LongText::MAX_CHARS).unwrap() + 1` `a` ⇒ `"… article.texte: text too large"` ; idem `nota` ⇒ `"… article.nota: text too large"` ; un seul gros texte vivant à la fois).
  - `kali_article_maps_with_a_kali_parent` (demandé et rendu `KALIARTI000005805295`, `textTitles [{"nature":"???","titre":"Convention X","cid":"KALITEXT000005635534"}]` ⇒ `Kali`, URL `https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000005805295`, `id_mismatch == 0`) ; `law_article_citation_through_the_mapping` (`LEGIARTI000033219575`, `num` `1`, `LOI`, titre `"LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)"`, `JORFTEXT000033202746` ⇒ `Text`, citation `"LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique, art. 1"`, URL `…/codes/article_lc/LEGIARTI000033219575`, Q12).
  - `error_details_never_contain_body_values` : `article.id`, `textTitles[0].cid`, `dateFin` à `secret-token-42` ⇒ aucun détail ne le contient.
- [ ] **Step 2 : implémenter** `article()`, vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): map::article, mapping de /consult/getArticle vers ArticleText`.

### Task 25 : `DecisionId::fond()` (domaine)

D28 ; 03 §2.5.

**Files:**
- Modify: `crates/lf-domain/src/id.rs` (bloc `impl DecisionId`, après `as_str` ; tests)

**Interfaces:**
- Produces : `pub const fn fond(&self) -> Fond` (`use crate::fond::Fond;`, `match` à trois bras explicites, sans `_`) ; doctest `DecisionId::parse("CETATEXT000051655317")?.fond() == Fond::Cetat`.

- [ ] **Step 1 : test rouge** `decision_id_fond_follows_the_variant` : `JURITEXT000048465611` ⇒ `Juri` ; `CONSTEXT000037434572` ⇒ `Constit` ; `CETATEXT000051655317` ⇒ `Cetat` ; `"juritext000048465611 "` ⇒ `Juri`.
- [ ] **Step 2 : implémenter** ; `cargo nextest run -p lf-domain` ; `cargo test -p lf-domain --doc DecisionId` ; `make cov` ; `make arch`.
- [ ] **Step 3 : commit** `feat(domain): DecisionId::fond (03 §2.5)`.

### Task 26 : ECLI natif, numéro d'affaire et sommaire par fond

03 §4.4 ; D29, D30. Mesures (`python3 -I /var/tmp/m2f-plan/s5-mesures.py`) : sommaire JURI, 1 entrée, `abstrats`/`resumePrincipal`/`autreResume` nettoyés 829/179/915 caractères (les 854/193/978 des docs sont ceux du texte non réparé) ; CETAT, 6 entrées : 3 `resumePrincipal` (736 caractères chacune sans le code de classement : 1 analyse) et 3 `abstrats` (468, 344, 457 sans `CETAT…` : 3 titrages).

**Files:**
- Create: `crates/lf-adapter-piste/src/map/decision.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod decision;`)

**Interfaces:**
- Produces (privé) :

```rust
/// Validé par `Ecli::parse`, jamais fabriqué ; absent ⇒ None sans diagnostic ; invalide ⇒ None et note_invalid_ecli().
fn native_ecli(raw: Option<&str>, diags: &mut Diagnostics) -> Option<Ecli>;
/// Entrées nettoyées (vides écartés, ordre gardé, sans dédoublonnage) ; liste vide et Fond::Cetat ⇒ [num] nettoyé s'il est non vide.
fn numero_affaire(fond: Fond, listed: &[String], num: Option<&str>) -> Vec<String>;
/// Répartition de D29 puis OfficialSummary::new ; autres fonds ⇒ OfficialSummary::default().
fn summary_of(fond: Fond, sommaire: &[SommaireDto]) -> OfficialSummary;
```

- [ ] **Step 1 : tests** (rouges).
  - `native_ecli_keeps_a_valid_value` (`ECLI:FR:CCASS:2025:C100399`, `ECLI:FR:CC:2025:2025.1173.QPC`, `ECLI:FR:CECHR:2026:506507.20260529` ⇒ identiques, aucun diagnostic) ; `native_ecli_invalid_is_none_and_counted` (`ECLI:FR:CC:2025`, `ecli:fr:cc:2025:2025.1173.qpc`, `ECLI:EN:CC:2025:1`, `ECLI:FR:CC:2025:2025.1173.QPC:extra` ⇒ `None`, `invalid_native_ecli == 1` ; départ à 2 ⇒ 3 ; `None` ⇒ inchangé).
  - `numero_affaire_keeps_the_listed_values` (`Juri`, `["24-13.470"]` ⇒ idem ; `["", " 24-13.470 ", "  ", "25-00.001"]` ⇒ deux ; doublons gardés) ; `numero_affaire_fallback_is_for_cetat_only` (`Cetat`, `[]`, `"506507"` ou `" 506507 "` ⇒ `["506507"]` ; `Cetat`, `["22NC02259"]` ⇒ lui ; `Cetat`, `[]`, `None` ou `"  "` ⇒ `[]` ; `Juri` et `Constit` sans liste ⇒ `[]`).
  - `summary_of_juri_splits_abstrats_from_titrages` (une entrée ⇒ `analyses ["Analyse A"]`, `titrages [resumePrincipal, autreResume]` ; deux entrées `(A1, R1, X1)`, `(A2, R2, -)` ⇒ `["A1", "A2"]`, `["R1", "X1", "R2"]`).
  - `summary_of_cetat_splits_resume_from_abstrats` (`"01-03-02-02 Analyse un."`, `"44 Analyse un."`, `"CETAT01-03-02-02 TITRAGE UN."`, `"CETAT44 TITRAGE DEUX."` ⇒ `["Analyse un."]`, `["TITRAGE UN.", "TITRAGE DEUX."]`) ; `summary_of_cetat_keeps_autre_resume_as_titrage` (`{abstrats: "T1", autre_resume: "T2", resume_principal: "A1"}` ⇒ `["A1"]`, `["T1", "T2"]`).
  - `summary_of_other_fonds_is_empty` (`Constit`, `Cnil` remplis ; `Juri`/`Cetat` avec `&[]` ; entrées sans champ ⇒ `default()`) ; `summary_ignores_the_nomenclature_ids` (`titrages: ["JURINOME000007644451-JURINOME000007645358"]`, `sommaire: []` ⇒ vide).
- [ ] **Step 2 : implémenter** (`match fond { Fond::Juri => …, Fond::Cetat => …, _ => return OfficialSummary::default() }`), vert, `make cov`.
- [ ] **Step 3 : commit** `feat(adapter): ECLI natif, numéro d'affaire et sommaire par fond de map::decision (T-M2-07)`.

### Task 27 : `map::decision` sur les fixtures

03 §4.4 ; 04 §9.1 (a) à (f) ; 06 §2.3.

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/decision.rs`, `crates/lf-adapter-piste/src/map/fixtures.rs` (`juri_with`)

**Interfaces:**
- Consumes : Tasks 11, 13, 14, 25, 26.
- Produces :

```rust
/// `/consult/juri` ⇒ `Decision` (03 §4.4). Sur `Err(Decode)`, `diags` garde les anomalies relevées avant l'erreur : `Stats::record_mapping` est appelé sur `Ok` comme sur `Err` (04 §9.3, D14, branché au plan 2g) ; un `NotFound` ne touche pas `diags`.
pub(crate) fn decision(dto: ConsultJuriResponseDto, requested: &DecisionId, diags: &mut Diagnostics) -> Result<Decision, CorpusError>;
// fixtures.rs (cfg(test)) :
pub(crate) fn juri_with(edit: impl FnOnce(&mut JuriTextDto)) -> ConsultJuriResponseDto;   // text: Some(JuriTextDto::default() modifié)
```

Ordre : `require(dto.text, JURI)?` ⇒ `resolve_id(requested, text.id, Field { JURI, "text.id" }, diags)?` ⇒ `fond = decision_id.fond()` ⇒ `check_origin(text.origine, decision_id.as_str(), diags)` ⇒ `prefer_computed((date_texte_computed, "text.dateTexteComputed"), (date_texte, "text.dateTexte"), diags)?` ⇒ `rules::long_text(texte_html, texte, Field { JURI, "text.texte" }, diags)?` ⇒ `clean_field` sur `num`, `juridiction`, `formation`, `solution`, `nature`, `titre` ⇒ `native_ecli`, `numero_affaire`, `summary_of`.

- [ ] **Step 1 : tests** (rouges ; fixtures par `decode_as::<ConsultJuriResponseDto>`, DTO synthétiques par `juri_with` : au moins `decision_origin_mismatch_is_counted_and_never_routes` et `invisible_chars_are_counted_for_the_text_only` l'emploient, sinon `dead_code` en test).
  - `decision_fixture_maps` (trois cas, `diags == Diagnostics::default()`) :

| | juri | cetat | constit |
|---|---|---|---|
| `decision_id` | `JURITEXT000051743650` | `CETATEXT000054178503` | `CONSTEXT000052555275` |
| `ecli` | `ECLI:FR:CCASS:2025:C100399` | `ECLI:FR:CECHR:2026:506507.20260529` | `ECLI:FR:CC:2025:2025.1173.QPC` |
| `num` | `12500399` | `506507` | `2025-1173` |
| `juridiction` | `Cour de cassation` | `Conseil d'État` | `Conseil constitutionnel` |
| `formation` / `solution` | `CHAMBRE_CIVILE_1` / `Cassation` | `2ème - 7ème chambres réunies` / `None` | `None` / `Conformité` |
| `numero_affaire` | `["24-13.470"]` | `["506507"]` | `[]` |
| `date_texte` / `nature` | 2025-06-04 / `ARRET` | 2026-05-29 / `Texte` | 2025-11-07 / `QPC` |
| `title` | `Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin` | `Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507` | `Décision 2025-1173 QPC - 07 novembre 2025 - M. Amine G. [Absence de publicité des débats devant le juge des libertés et de la détention statuant sur la détention provisoire du prévenu] - Conformité` |
| paragraphes / `char_len()` | **1** / **577** (commence par `LA COUR DE CASSATION, PREMIÈRE CHAMBRE CIVILE`) | **43** / **14 608** (`Vu la procédure suivante :` … `Signé : Mme Sandrine Mendy`) | **49** / **14 332** (`LE CONSEIL CONSTITUTIONNEL A ÉTÉ SAISI le 8 août 2025` … `Rendu public le 7 novembre 2025.`) |
| sommaire (analyses / titrages) | 1 / 2 | 1 / 3 | `OfficialSummary::default()` |

  - `summary_by_fond_juri` : analyse de 829 caractères, de `L'action en responsabilité dirigée contre le fabricant d'un produit` à `et de l'identité du producteur` ; titrages de 179 puis 915, le premier de `Cassation civil - RESPONSABILITE DU FAIT DES PRODUITS DEFECTUEUX - Producteur` à `Prescription - Délai - Détermination`, le second finissant par `du Conseil du 25 juillet 1985` ; ni `<` ni `\n`.
  - `summary_by_fond_cetat` : 1 analyse de 736 caractères, de `Projet de plan local d’urbanisme dont la mise en œuvre est` (U+2019) à `d’une telle consultation.` ; titrages de 468, 344, 457 commençant par `ACTES LÉGISLATIFS ET ADMINISTRATIFS. - VALIDITÉ DE`, `NATURE ET ENVIRONNEMENT. - OBLIGATION DE TRANSMISS`, `URBANISME ET AMÉNAGEMENT DU TERRITOIRE. - PLANS D`, finissant par `SEULES COMMUNES LIMITROPHES DU PROJET.` ; aucune entrée ne commence par un chiffre ni par `CETAT`.
  - `native_ecli_is_validated_not_fabricated` (CONSTIT sans `ecli` ⇒ `None`, aucun diagnostic ; `ECLI:FR:CC:2025` ⇒ `None`, 1 ; les trois fixtures : `contract().ecli == decision.ecli`).
  - `empty_text_envelope_maps_to_not_found` : `{}`, `{"text":null}`, `{"text":{}}`, `{"text":{"origine":"JURI","texte":"abc"}}` ⇒ `NotFound { what: "/consult/juri" }`.
  - `decision_returned_id_wins_and_drives_the_fond` : `{"text":{"titre":"T"}}` ⇒ demandé, `id_mismatch == 1` ; demandé `JURITEXT000051743650`, corps `CETATEXT000054178503`, `origine` `CETAT`, sommaire `[{resumePrincipal: "01-02 Analyse un."}, {abstrats: "CETAT01-02 TITRAGE UN."}]` ⇒ `DecisionId::Cetat`, `id_mismatch == 1`, `origin_mismatch == 0`, `["Analyse un."]`, `["TITRAGE UN."]`.
  - `decision_origin_mismatch_is_counted_and_never_routes` : `CONSTEXT000052555275`, `origine` `JURI` ⇒ 1, URL `https://www.legifrance.gouv.fr/cons/id/CONSTEXT000052555275`.
  - `decision_date_prefers_computed` et `decision_unreadable_date_is_decode` (`"/consult/juri: text.dateTexte: unreadable date"`, puis `text.dateTexteComputed`) ; `cetat_numero_affaire_falls_back_to_num` (fixture CETAT ⇒ `["506507"]` ; `["22NC02259"]` gardé ; fixture JURI à liste vidée ⇒ `[]`).
  - `invisible_chars_are_counted_for_the_text_only` (`texte` `a\u{202e}b` ⇒ `"ab"`, 1 ; `juridiction` `Cour\u{200b}` ⇒ `"Cour"`, compteur toujours 1) ; `attacked_decision_does_not_change_the_decision` (fixture JURI avec `decision_attaquee` remis à `Default` ⇒ même `Decision`, mêmes diagnostics) ; `decision_errors_never_echo_body_values` (`id` `JURITEXT-TOPSECRET`, `dateTexte` `"TOPSECRET"` ⇒ `Decode` sans `TOPSECRET`).
- [ ] **Step 2 : implémenter** (assemblage d'une trentaine de lignes), vert, `make cov`, `make -k ci`. Si 577 diffère : rapporter, ne pas changer le code.
- [ ] **Step 3 : commit** `feat(adapter): map::decision et contrats sur les fixtures juri, cetat et constit (T-M2-07, 03 §4.4)`.

### Task 28 : `map::deliberation`

03 §4.5. Mesures `consult_cnil.json` : `num` `SAN-2026-002`, `natureDelib` `Sanction`, `etat` `VIGUEUR`, dates 2026-01-08 et 2026-01-14 (`…Computed` égaux), `titre` 43 caractères, `titreLong` 140, `ecli` `null`, clé `sommaire` absente, 3 paragraphes de 153, 262, 174 (canonique 593, garde-fou à 97,52 % : HTML gardé).

**Files:**
- Create: `crates/lf-adapter-piste/src/map/deliberation.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod deliberation;`)

**Interfaces:**
- Produces :

```rust
/// `/consult/cnil` ⇒ `Deliberation` (03 §4.5). Même règle sur `diags` que `decision`.
pub(crate) fn deliberation(dto: ConsultJuriResponseDto, requested: &CnilTextId, diags: &mut Diagnostics) -> Result<Deliberation, CorpusError>;
```

Sources : `require(dto.text, CNIL)`, `resolve_id(.., Field { CNIL, "text.id" }, ..)`, `check_origin(origine, id.as_str(), ..)` ; `clean_field` sur `num`, `nature`, `natureDelib`, `titre`, `titreLong` ; `etat = status_of(etat, None)` ; `date_texte = prefer_computed((dateTexteComputed, "text.dateTexteComputed"), (dateTexte, "text.dateTexte"))` ; `date_publication` de même avec `datePubliComputed`/`datePubli` ; `text = rules::long_text(texteHtml, texte, Field { CNIL, "text.texte" }, diags)`.

- [ ] **Step 1 : tests** (rouges ; `decode_as::<ConsultJuriResponseDto>("consult_cnil")`, `juri_with`).
  - `deliberation_fixture_maps` : identifiant `CNILTEXT000053352643` ; `num` `SAN-2026-002` ; `nature` `Délibération` ; `nature_delib` `Sanction` ; `Vigueur` ; 2026-01-08 ; 2026-01-14 ; `title` `"Délibération SAN-2026-002 du 8 janvier 2026"` ; `title_long` `"Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 prononçant une sanction pécuniaire à l'encontre de la société FREE"` ; paragraphes `[(0, 153), (155, 262), (419, 174)]` (offset, caractères), `char_len() == 593`, dernier terminé par `traitement des données à [...]` ; `diags == Diagnostics::default()` ; citation `"CNIL, délibération n° SAN-2026-002 du 8 janvier 2026"`.
  - `deliberation_status_unknown_code_is_other` (`EN_COURS` ⇒ `Other` ; `""`, `"  "`, absent ⇒ `None` ; `vigueur` ⇒ `Vigueur`) ; `deliberation_publication_date_prefers_computed` (`Millis(1_768_348_800_000)` contre `Millis(1_762_473_600_000)` ⇒ 2026-01-14 ; calculé absent ⇒ 2025-11-07 ; rien ⇒ `None`).
  - `deliberation_unreadable_dates_are_decode_with_their_path` (`datePubli "TOPSECRET"` ⇒ `"/consult/cnil: text.datePubli: unreadable date"` ; puis `text.datePubliComputed`, `text.dateTexte` ; aucun détail avec `TOPSECRET`) ; `deliberation_empty_text_envelope_maps_to_not_found` (`{}`, `{"text":null}`, `{"text":{}}` ⇒ `NotFound { what: "/consult/cnil" }`).
  - `deliberation_id_and_origin_rules` (`CNILTEXT000053408671` ⇒ gardé, 1 ; absent ⇒ demandé, 1 ; `JURITEXT000051743650` ⇒ `"/consult/cnil: text.id: invalid identifier"` ; `origine JURI` ⇒ `origin_mismatch == 1`, `cnil` ⇒ 0).
  - `deliberation_ignores_juri_only_fields` (fixture avec `ecli`, `numero_affaire` et `sommaire` remplis ⇒ même entité) ; `deliberation_text_falls_back_to_plain` (`Un seul bloc` ⇒ 1 paragraphe ; `a\u{202e}b` ⇒ compteur 1) ; `deliberation_titles_have_no_fallback`.
- [ ] **Step 2 : implémenter** (aucune logique propre), vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): map::deliberation et contrat sur la fixture CNIL (T-M2-07, 03 §4.5)`.

### Task 29 : `map::agreement`, accord d'entreprise en métadonnées

04 §9.2 (`CompanyAgreement`) ; 03 §4.7 ; P-036, E-14. Mesures `consult_acco.json` : dates 2025-12-19, 2025-12-20, 2026-02-27, `dateFin` sentinelle, `dateDiffusion` 2026-03-10 ; `codeIdcc` `"275"` ; 1 thème `Télétravail`, 3 syndicats `CFTC`, `CFE-CGC`, `Autre`.

**Files:**
- Create: `crates/lf-adapter-piste/src/map/agreement.rs`
- Modify: `crates/lf-adapter-piste/src/map/mod.rs` (`pub(crate) mod agreement;`)

**Interfaces:**
- Produces :

```rust
pub(crate) fn agreement(dto: ConsultAccoResponseDto, requested: &AccoTextId, diags: &mut Diagnostics) -> Result<CompanyAgreement, CorpusError>;
fn labels(entries: &[LibelleDto]) -> Vec<String>;   // clean_field(libelle) par entrée, None écartés, ordre et doublons gardés
```

`require(dto.acco, ACCO)?` ; `resolve_id(requested, acco.id, Field { ACCO, "acco.id" }, diags)?` ; `title` (`titre_texte`), `nature`, `raison_sociale`, `siret`, `code_ape`, `secteur` par `clean_field` (D17) ; `idcc = code_idcc.and_then(Idcc::parse)` ; `date_texte`, `date_effet`, `date_depot` par `civil(.., Field { ACCO, "acco.dateTexte" | "acco.dateEffet" | "acco.dateDepot" }, ..)?` ; `themes`, `syndicats` par `labels`. Aucun champ texte (`data`, `attachment`, `fileSize`, `numero`) n'est lu.

- [ ] **Step 1 : tests** (rouges ; JSON synthétiques par `decode`, demandé `ACCOTEXT000053643828`).
  - `consult_acco_fixture_maps` (`decode_as`) : `agreement_id` `ACCOTEXT000053643828` ; `nature` `ACCORD` ; `title` `"Accord d'entreprise relatif au télétravail"` ; `raison_sociale` `"CORSAIR (Télétravail)"` ; `siret` `32862158600143` ; `idcc` 275 ; `code_ape` `5110Z` ; `secteur` `"Transports aériens de passagers"` ; `themes ["Télétravail"]` ; `syndicats ["CFTC", "CFE-CGC", "Autre"]` ; dates 2025-12-19, 2025-12-20, 2026-02-27 ; citation `"Accord d'entreprise relatif au télétravail (CORSAIR (Télétravail))"` ; URL `https://www.legifrance.gouv.fr/acco/id/ACCOTEXT000053643828` ; `diags == Diagnostics::default()`.
  - `acco_without_identity_maps_to_not_found` : `{"acco":null}`, `{}`, `{"executionTime":1,"dereferenced":false}`, `{"acco":{}}`, `{"acco":{"nature":"ACCORD","raisonSociale":"X"}}`, `{"acco":{"id":"  ","titreTexte":"  "}}` ⇒ `NotFound { what: "/consult/acco" }`, `diags` inchangé.
  - `agreement_without_id_uses_the_requested_one_and_counts` (`{"titreTexte":"Accord X"}`, `{"id":" ","titreTexte":"Accord X"}` ⇒ demandé, 1 ; `{"id":"  ","titreTexte":"<br>"}` ⇒ `title None`, 1) ; `agreement_keeps_the_returned_id_and_counts_the_mismatch` (`ACCOTEXT000051888026` ⇒ gardé, URL terminée par lui, 1 ; `" accotext000053643828 "` ⇒ 0).
  - `agreement_with_a_foreign_or_malformed_id_is_a_decode_error` (`JURITEXT000051743650`, `"SECRET-ID-42"`, `"ACCOTEXT1"` ⇒ `"/consult/acco: acco.id: invalid identifier"` exact, sans `SECRET`, `id_mismatch == 0`).
  - `agreement_idcc_accepts_string_and_number` (`"275"`, `275`, `"0000000000000275"`, `"0275"` ⇒ 275 ; `"10000"`, `"0"`, `"0000"`, `"abc"`, `""`, `2.5`, `true`, `null`, absent ⇒ `None` ; toujours `Ok`, aucun diagnostic) ; `agreement_secteur_empty_is_none` (`""`, `"  "`, `"<br>"` ⇒ `None` ; `nature ""` ⇒ `None`).
  - `agreement_labels_are_cleaned_one_by_one` (`themes [{"libelle":"Télé<b>travail</b>"},{"libelle":""},{"libelle":null},{"code":"9"},{"libelle":"  <br> "},{"libelle":"Salaires"},{"libelle":"Salaires"}]` ⇒ `["Télétravail", "Salaires", "Salaires"]` ; `syndicats [{"libelle":"CFTC"},{"libelle":" CGT "}]` ⇒ `["CFTC", "CGT"]` ; `null`/absents ⇒ `[]`).
  - `agreement_dates_accept_every_encoding` (`1766102400000`, `"2025-12-20"`, `"2026-02-27T00:00:00.000+0000"`) ; `agreement_sentinel_dates_are_none` ; `agreement_non_midnight_timestamp_is_counted` (`dateDepot 1772150400001` ⇒ 2026-02-27, 1) ; `agreement_unreadable_text_date_is_a_decode_error` (chaque date à `"SECRET-DATE-42"` ⇒ `"/consult/acco: acco.<champ>: unreadable date"`).
  - `agreement_ignores_the_unmapped_dates_and_fields` (`dateFin`, `dateDiffusion` `"SECRET-DATE-42"`, `fileSize [1]`, `numero "T1"`, `conformeVersionIntegrale true`, `signataires ["91"]` ⇒ même entité que sans eux).
  - `agreement_short_fields_are_cleaned_without_counting` (`titreTexte "Accord <mark>télé\u{200b}travail</mark>"`, `raisonSociale "COR\u{00ad}SAIR"`, thème `"A\u{200b}\u{200b}b"` ⇒ `"Accord télétravail"`, `"CORSAIR"`, `["Ab"]`, `invisible_chars_removed == 0`, RI-14) ; `agreement_errors_never_carry_a_body_value`.
- [ ] **Step 2 : implémenter**, vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `feat(adapter): map::agreement, accord d'entreprise en métadonnées seules (T-M2-07)`.

### Task 30 : `Mapped`, `map_body` et `every_normalized_fixture_has_contract`

D-30 (06 §3.1) ; 03 §5 ; A-parite P-030 à P-032, §5.4. Point d'entrée « corps ⇒ entité » réutilisé par Task 31 et le plan 2g.

**Files:**
- Modify: `crates/lf-adapter-piste/src/map/fixtures.rs`

**Interfaces:**
- Consumes : `map::search::search_page`, `map::act::act`, `map::convention::convention`, `map::article::article`, `map::decision::decision`, `map::deliberation::deliberation`, `map::agreement::agreement` (signatures ci-dessus).
- Produces (`cfg(test)`) :

```rust
#[derive(Debug)] pub(crate) enum Mapped { Search(SearchPage), Act(Act), Convention(Convention), Article(ArticleText),
                                          Decision(Decision), Deliberation(Deliberation), Agreement(CompanyAgreement) }
pub(crate) fn map_body(kind: Kind, text: &str, requested: &str, diags: &mut Diagnostics) -> Result<Mapped, CorpusError>;
pub(crate) fn map_fixture(fixture: &Fixture, diags: &mut Diagnostics) -> Result<Mapped, CorpusError>;   // decoded(f, true).text, f.requested
impl Mapped { pub(crate) fn contracts(&self) -> Vec<CitationContract>; pub(crate) fn dropped(&self) -> u32; }
```

`map_body` décode à l'endpoint de `kind` puis appelle le mapper ; `Search(fond)` : `SearchQuery::new(fond, QueryText::parse("golden"), NonZeroU32::MIN, PageSize::default(), MatchMode::default(), None, None).expect("golden query")` (pas de `From<DomainError>`) ; sinon `parse(requested).expect(..)` du type demandé (`LegiTextId`, `KaliTextId`, `ArticleId`, `DecisionId`, `CnilTextId`, `AccoTextId`).

- [ ] **Step 1 : tests** (rouges).
  - `every_normalized_fixture_has_contract` (D-30, `citation_never_empty`, `ported_no_fabricated_eli`) : `golden()["normalizer"]` a **17** clés ; chaque clé privée de `[id=…]` et de `.json` est un nom d'`ALL` (16 couverts, `consult_article` deux fois ; l'identifiant de la variante vient de la clé). Préfixes sous `https://www.legifrance.gouv.fr/` : `search_loda` 2 hits `loda/id/`, `search_code` 1 `codes/texte_lc/`, `search_juri` 2 `juri/id/`, `search_constit` 3 `cons/id/`, `search_cetat` 3 `ceta/id/`, `search_cnil` 2 `cnil/id/`, `search_kali` 2 `conv_coll/id/`, `search_acco` 2 `acco/id/` (aucun ECLI) ; `consult_loda` `loda/id/LEGITEXT000033205014` ; `consult_article` et la variante `codes/article_lc/LEGIARTI000006419288` ; `consult_juri` `juri/id/JURITEXT000051743650` + `ECLI:FR:CCASS:2025:C100399` ; `consult_juri_constit` `cons/id/CONSTEXT000052555275` + `ECLI:FR:CC:2025:2025.1173.QPC` ; `consult_juri_cetat` `ceta/id/CETATEXT000054178503` + `ECLI:FR:CECHR:2026:506507.20260529` ; `consult_cnil` `cnil/id/CNILTEXT000053352643` ; `consult_kali` `conv_coll/id/KALITEXT000049878950` ; `consult_acco` `acco/id/ACCOTEXT000053643828`. Pour les 26 contrats : citation non vide, préfixe, `eli.is_none()`, ECLI attendu ; chaque hit : `source_url.ends_with(hit.id.as_str())`, `dropped() == 0`.
  - `fixtures_map_without_diagnostics` : `Diagnostics::default()` pour les 16 fixtures (hits LODA/CODE d'origine `LEGI` compris).
  - `kaliarti_variant_keeps_the_returned_id` : `map_body(GetArticle, <corps réparé de consult_article>, "KALIARTI000049886059", ..)` ⇒ `LEGIARTI000006419288`, `id_mismatch == 1`, le reste à `Default`.
  - `map_body_rejects_a_body_of_the_wrong_shape` : `map_body(Acco, "null", ..)` ⇒ `Decode` de détail commençant par `/consult/acco: .: ` ; `r#"{"acco":null}"#` ⇒ `NotFound { what: "/consult/acco" }`.
- [ ] **Step 2 : implémenter** (chaque bras de `map_body` et de `contracts` pris par les 17 entrées), vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `test(adapter): map_body et every_normalized_fixture_has_contract sur les 17 entrées du golden`.

### Task 31 : `src/view.rs` et `no_mojibake_after_mapping`

04 §10.3, §10.6, §13.3 ; 06 §2.3 ; D-35.

**Files:**
- Create: `crates/lf-adapter-piste/src/view.rs`
- Modify: `crates/lf-adapter-piste/src/lib.rs` (`#[cfg(test)] mod view;` et une ligne de la liste des modules)

**Interfaces:**
- Consumes : `map::fixtures::{Mapped, ALL, decoded, map_body, map_fixture}`, `has_latin1_mojibake`, `Citable`, `LongText::{as_str, paragraphs, char_len, invisible_removed}`, `LegalStatus::as_api_code`.
- Produces (`cfg(test)`) :

```rust
/// Tous les champs de l'entité (clés snake_case du domaine) ; identifiants par as_str, dates AAAA-MM-JJ ou null, Option absent ⇒ null,
/// énumérations par code, LongText ⇒ {"text","paragraphs","char_len","invisible_removed"}, TOC en arbre sur roots(),
/// contrat {"citation","source_url","ecli"}. Chaque entité déstructurée SANS `..` (un champ ajouté au domaine casse la compilation).
pub(crate) fn view(mapped: &Mapped) -> serde_json::Value;
pub(crate) fn leaf_strings(value: &Value) -> Vec<(String, &str)>;   // (chemin text.text, hits[0].title, toc[3].article.num ; contenu), clés exclues
pub(crate) fn mojibake_paths(mapped: &Mapped) -> Vec<String>;
```

- [ ] **Step 1 : tests** (rouges).
  - `no_mojibake_after_mapping` (D-35) : les 16 fixtures (corps réparé) et la variante KALIARTI ⇒ `mojibake_paths` vide ; `leaf_strings(view(..))` non vide pour chacune.
  - `no_mojibake_check_detects_unrepaired_fixtures` : les 6 fixtures réparées de M7, mappées depuis `decoded(f, false)` ⇒ chemins non vides ; `consult_article` ⇒ exactement `["text.text"]`.
  - `constit_nbsp_false_positive_is_neutralised_by_mapping` (04 §10.2 étape 3) : `decoded(consult_juri_constit, true)` ⇒ `(true, false)` ; 2 chaînes brutes au motif (`text.texte`, `text.texteHtml`) ; `mojibake_paths(map_fixture(..)) == []`.
  - `view_lists_every_acco_field` : clés `agreement_id, nature, title, raison_sociale, siret, idcc, code_ape, secteur, themes, syndicats, date_texte, date_effet, date_depot, contract` ; `view["idcc"] == "275"`, `view["date_effet"] == "2025-12-20"`, `view["themes"] == ["Télétravail"]`.
- [ ] **Step 2 : implémenter** (aucun bras jamais pris), vert, `make cov`, `make -k ci`.
- [ ] **Step 3 : commit** `test(adapter): projection view et no_mojibake_after_mapping (D-35)`.

### Task 32 : documentation du mapping

Aucune ligne de Rust. Repères par `grep`.

**Files:**
- Modify: `docs/plan/04-adapter-piste.md`, `docs/plan/02-architecture.md`, `docs/plan/03-domaine.md`, `docs/plan/05-outils-mcp.md`, `docs/plan/06-qualite.md`, `docs/plan/07-feuille-de-route.md`, `docs/plan/00-decisions.md`, `docs/plan/annexes/A-parite.md`

- [ ] **Step 1 : 04 §1.5.** `map/` liste `mod.rs`, `dates.rs`, `rules.rs`, `search.rs`, `body.rs`, `toc.rs`, `act.rs`, `convention.rs`, `article.rs`, `decision.rs`, `deliberation.rs`, `agreement.rs`, `fixtures.rs` (`cfg(test)`) ; `src/view.rs` (`cfg(test)`) ; `stats.rs` compte aussi le mapping ; les tests lisent les fixtures par `include_str!` dans `map/fixtures.rs`.
- [ ] **Step 2 : 04 §9.1.** (a) demandé + `id_mismatch` (`warn`), `rules::require` ; (b) livré (`rules::resolve_id`) : identifiants comparés normalisés, la famille compte (`KALIARTI` demandé et `LEGIARTI` rendu est un écart), l'entité garde le renvoyé, écart golden `returned_id_kept` ; (c) `parse_counted` : suffixe hors `LEGITEXT` retiré et compté ; (d) `check_origin`, sept familles (hits, décisions, délibérations) ; (e) `dates::{civil, first_civil, prefer_computed}` ; (f) `rules::long_text`, champs courts par `clean_field`.
- [ ] **Step 3 : 04 §9.2.** Signatures `map::search::search_page(dto, &SearchQuery, &mut Diagnostics)` et `search_page_with_mode` (mode reçu, R12), `map::act::act(dto, &LegiTextId, &mut Diagnostics)` (sans `VersionDate`), `convention`, `article`, `decision`, `deliberation`, `agreement` ; points propres : D19 (dates, extrait, articles, `results` > `page_size` ⇒ `Decode`), D20 à D25 (`cid: DocumentId`, `status_of`, TOC par `map::toc::build`, compteurs `toc_dropped` et `invisible_chars_removed`), D26 (`parent` sur la même entrée que `code_title`, `nota` vide ⇒ absent, `best_of` brut non lexé, Ruling 12), D28 à D30 (fond de l'identifiant renvoyé, `origine` en contrôle seul, `decisionAttaquee` non mappée), D31.
- [ ] **Step 4 : 04 §9.3, une seule réécriture.** Remplacer « cible, plan 2f et suivants : … » et le paragraphe « à décider au plan 2f » par : neuf compteurs livrés (`dropped_hits`, `id_mismatch`, `non_midnight_timestamps`, `upstream_suffix_stripped`, `origin_mismatch`, `invalid_native_ecli`, `invalid_nor`, `toc_dropped`, `invisible_chars_removed`), journalisés par `Stats::record_mapping` (niveaux de D14, jamais d'identifiant), appelé une fois par mapping y compris sur `Err` (plan 2g) ; `retries`, `rate_limited_waits` à venir ; invisibles : textes longs seulement (D17), la recherche n'en compte aucun.
- [ ] **Step 5 : 04 §10.5, §13.3 et 02 §3.3.** `StatsSnapshot` : onze champs ; tableau des contrats : `src/map/fixtures.rs`, `src/view.rs`.
- [ ] **Step 6 : annexe A.** P-032 : « aucun cas sur les fixtures » ⇒ « 7 extraits de hits coupés (juri 1, constit ×3, cetat ×2, kali 0), dont 1 extrait d'article (kali 0) » ; tests : `snippet_is_truncated_to_280_chars`, `search_fixtures_match_the_golden_on_shared_fields`, `mapping_matches_golden` (2g). E-26 : tests `search_kali_fixture_maps`, `search_fixtures_match_the_golden_on_shared_fields` ; golden à `articles: []` pour KALI, titres CONSTIT aux blancs non repliés (`short_text_cleaned`). §4.7, nouvelle ligne `returned_id_kept` (Golden = oui) : variante `consult_article.json[id=KALIARTI000049886059]` (`test_parse.py:226`, `golden.py:317`) ; Rust garde l'identifiant renvoyé, `id_mismatch` = 1, `article_id` = `LEGIARTI000006419288`, `source_url` = `https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000006419288`, `eli_uri` couvert par `eli_uri_dropped` ; justification 04 §9.1 (b), D-28. §5.4 : deux `Override` (`/article_id`, `/source_url`) pour cette entrée, `GoldenDivergence::ReturnedIdKept` à créer en 2g ; ligne 5 des transformations : `returned_id_kept` (« niveau 1 seulement ») ; « les 18 variantes » ⇒ « une variante par clé du §4 dont la colonne Golden vaut oui » ; niveau 1 : « pour une clé `fixture.json[id=X]`, le harnais passe `X` comme identifiant demandé et la fixture non modifiée ».
- [ ] **Step 7 : 03 et 05.** 03 §2.5 (l. ~230 et ~233) : `DecisionId::fond()` livrée (plan 2f) ; §4.4 tableau du sommaire (l. ~524) : JURI `resumePrincipal` 189 bruts (179 nettoyés), `abstrats` 829, `autreResume` 958 (915) ; CETAT 6 entrées (736 ; 468, 344, 457) ; règles D29 ; l. ~1099 : 854 ⇒ 829 ; `decision_id`, `deliberation_id` : « absent ou blanc ⇒ identifiant demandé et `id_mismatch` ; objet sans identité ⇒ `NotFound` » ; §7.3 : invisibles comptés pour les textes longs seulement (D17). 05 l. ~1299 : `(854)` ⇒ `(829)`. Vérifier d'abord par `grep -n "854\|193 caractères\|978" docs/plan/03-domaine.md docs/plan/05-outils-mcp.md`.
- [ ] **Step 8 : 06, 07, 00.** 06 §2.3 (ligne `fixtures_contract`) : tests dans `src/map/fixtures.rs` et `src/view.rs`. 07 T-M2-07 : « mapping pur livré au plan 2f (branchement au plan 2g) », Vérification + `consult_acco_fixture_maps`, `every_normalized_fixture_has_contract`, `no_mojibake_after_mapping`, `dto_parses_every_fixture` (case non cochée : le propriétaire tranche) ; T-M2-10 Où : `crates/lf-adapter-piste/src/{map/fixtures,view,golden}.rs` (au lieu de `tests/fixtures_contract.rs`, `tests/parity_golden.rs`), trois tests livrés, `fixture_mutations_never_panic` en 2g ; « Découpage » : 2f livré. 00 §11.1 : « Faits au plan 2f ».
- [ ] **Step 9 : vérifier.** `grep -n "à décider au plan 2f" docs/plan/*.md` vide ; `grep -n "numero_affaire_cetat" docs/plan/annexes/A-parite.md` répond ; `make fmt-check lint doc` ; `git diff --stat` limité aux documents.
- [ ] **Step 10 : commit** `docs: mapping DTO vers domaine, diagnostics et divergence returned_id_kept (plan 2f)`.

### Task 33 : mutation finale, revue de branche et découpage des PR

Une seule campagne de mutation pour tout le plan (aucune tâche précédente n'en a lancé). cargo-mutants ne mute ni les `fn new` ni les `impl Default`, ni le code `#[cfg(test)]` (`map/fixtures.rs`, `view.rs`), ni `fuzz/**` (`.cargo/mutants.toml`) : aucune décision ne doit y vivre.

**Files:**
- Modify (seulement si un mutant survit) : le fichier de test de la tâche qui possède le code ; `.cargo/mutants.toml` seulement pour un mutant **prouvé équivalent**, raisonnement en commentaire.

- [ ] **Step 1 : préalables.** `make mem-check` (≥ 8 Gio disponibles) ; aucune autre compilation en cours ; `make cov` vert.
- [ ] **Step 2 : liste.** `git diff --name-only feat/m2e-domaine...HEAD -- '*.rs'` ; fichiers de production attendus : `crates/lf-adapter-piste/src/dto/{budget,de,decode,search,consult_text,article,juri,acco}.rs`, `crates/lf-adapter-piste/src/stats.rs`, `crates/lf-adapter-piste/src/map/{mod,dates,rules,search,body,toc,act,convention,article,decision,deliberation,agreement}.rs`, `crates/lf-domain/src/entity/act.rs`, `crates/lf-domain/src/id.rs` (les fichiers à tests seuls, `dto/mod.rs`, `memo.rs`, `corpus.rs`, `lib.rs`, donnent 0 mutant : les passer quand même).
- [ ] **Step 3 : mutation, un fichier à la fois, jamais en parallèle.** Pour chaque fichier : `make mutants FILE=<fichier>` (la cible pose `TMPDIR=/var/tmp`, `-j 1`, `NEXTEST_TEST_THREADS=2`) ; noter au ledger « N mutants tested : caught, unviable, missed, timeout » (compter aussi la forme singulière « 1 mutant tested ») ; attendu **0 MISSED et 0 TIMEOUT** partout.
- [ ] **Step 4 : survivants.** Ajouter le test qui tue le mutant dans la tâche propriétaire (commit `test(adapter): mutants survivants du plan 2f`), relancer `make mutants FILE=<fichier>` ; un TIMEOUT dans une boucle de lecture signale une boucle qui ne consomme rien : corriger le code, pas le délai.
- [ ] **Step 5 : budget de la CI et découpage.** Le job `mutants (diff de la PR)` (`ci.yml`, `-j 2`, `--timeout 60`) plafonne à **60 min** : `git diff feat/m2e-domaine...HEAD > /var/tmp/m2f.diff` puis `cargo mutants --list --in-diff /var/tmp/m2f.diff | wc -l` (sans compilation). Au-delà d'environ **450 mutants**, scinder : **PR 1** = Tasks 1 à 9 (budget, R13, D-24, fuzz) ; **PR 2** = Tasks 10 à 32 ; si PR 2 seule dépasse encore 450, la scinder entre Task 17 (infrastructure et recherche) et Task 18 (consultations). Chaque PR empilée est re-basée sur `main` avant sa fusion ; mesurer chaque morceau avec son propre diff.
- [ ] **Step 6 : revue de branche.** Agent `code-reviewer` (Sonnet) sur toute la branche, puis `security-auditor` (Opus) : entrées réseau, budget mémoire, absence de valeur du corps dans les erreurs et journaux. Corriger ce qui est retenu (une vague de clôture), consigner le reste au ledger.
- [ ] **Step 7 : clôture.** `make -k ci` (seul `deny` toléré, RUSTSEC-2026-0386) ; `cargo +1.88 check --workspace --all-targets --locked` ; `make min-age` ; `make arch` ; essai `dto_decode` de 60 s refait si un `dto/*.rs` a bougé depuis la Task 8. Rien n'est poussé : le contrôleur remet au propriétaire la liste des PR, le compte de mutants de chacune et les décisions [P].

---

## Hors périmètre

- **Plan 2g** : `request/consult.rs` (réutilise `map::endpoint`) ; `impl LegalCorpus for PisteCorpus` (T-M2-09) avec `Stats::record_mapping` sur `Ok` **et** `Err`, `search_page_with_mode` et `PaginationMemo::remember`, et le **permis de concurrence tenu pendant `decode` puis `map::*`** (exigence de la clôture, Ruling 6, 00 §11.1 : `send` rend le `Permit` avec l'`Outcome`, ou un sémaphore de décodage dédié de `max_concurrency` places ; le travail est synchrone ; aujourd'hui `send` rend le permis dès le corps lu, donc la borne « 4 × pic » du seuil (5) de la sonde de mémoire (étape du contrôleur) n'est pas tenue), retrait de `search_page` ou passage en `cfg(test)` ; retrait de l'`expect(dead_code)` (dont les six champs ACCO de D31, les champs d'`ArticleDto` non lus : `version_precedente`, `section_parent_titre`, `is_multiple_versions`, `context.titre_txt`, `ContextEntryDto.{id, debut, fin}`) ; `src/golden.rs` (`GoldenDivergence` avec `ReturnedIdKept`, `OVERRIDES` de la variante KALIARTI, `mapping_matches_golden`, `golden_divergences_match_annex_a` et les tests de cohérence) ; `mapping_snapshots` (`insta`, sur `view`) ; les 19 `ported_*` ; sidecars (`fixtures_have_meta_sidecar`, `fixture_sha256_matches_meta`, `fixtures_contain_no_token_like_strings` : choix `sha2` ou SHA-256 de test) ; `fixture_mutations_never_panic` (10 449 mutants de fixtures par `map_body`) ; tests wiremock (`consult_*_fixture_maps` par HTTP, `acco_large_data_field_is_skipped`, `ids_go_in_body_never_in_path`, `urls_in_responses_are_never_fetched`, `kali_inline_content_is_available_without_extra_call` avec `expect(0)` sur `getArticle` : le test pur du plan 2f, `map/toc.rs`, ne prouve que le contenu en ligne, 04 §13.3) ; assertions différées du ledger 2e (plus long paragraphe, `best_of` CNIL, sommaires).
- **Plan 2h** : `record_fixture`, `recorded_fixtures_have_no_mojibake`, tests live ; sondes V-75 et V-96 (taille réelle d'une TOC de code, D4), V-82 et V-83 (pages `ARTICLE` hors `pageSize`, D3), V-100 (fuseau), V-104 (suffixes hors LEGITEXT), V-105 (`num` entier), V-109 et V-112 (sommaire par fond, numéro d'affaire CETAT), V-140 et V-142 (URL d'article de loi).
- **M3 et au-delà** : gabarit distinct pour `InvalidRequest { status: None }` (`lf-application::messages`, M3, critère (3) de T-M2-09) ; lecture des compteurs par `lf-app` (`check-config`, M4) ; DTO et mappers M6 (`code_list`, `kali_cont`, `jorf_by_nor`, `eli_redirect`) ; M7 (`previous_version`, `cited_case_law`, `applied_texts`, `decisionAttaquee`, `SearchHit.solution`, `textAbroge`, `LodaArticle`).
- **Hors plan, à faire à côté** : montée de `tokio` 1.53.3 (RUSTSEC-2026-0386, éligible le 2026-10-27 ou libellé `security-fix`) ; Mi-1 sur `feat/m2e-domaine` ; ménage des worktrees et branches (décision du propriétaire).
