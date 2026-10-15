> **Brouillon en cours** — décisions communes du plan de réécriture Rust/hexagonale (MCP 2026-07-28). Le plan complet (architecture, domaine, adapter PISTE, outils MCP, qualité, feuille de route, distribution, ADR) remplacera ce fichier.
>
> Plans d'implémentation exécutables (format superpowers, tâche par tâche, TDD) : `docs/superpowers/plans/`. Plan 1 = M0 + M1 (`2026-10-15-m0-m1-fondations-domaine.md`). Les exigences de qualité du §9 s'appliquent à tous les plans.

# Décisions communes — plan de réécriture Rust/hexagonale du MCP Légifrance


## 0. Cadre
- Langue des documents : français. Identifiants Rust, noms de crates, de champs JSON et d'outils MCP : tels quels (anglais/ASCII).
- Le plan décrit QUOI construire et COMMENT, dans quel ordre, avec quels critères d'acceptation. Il ne contient PAS d'implémentation complète ; des extraits de code courts (signatures de traits, structs, squelettes) sont bienvenus quand ils lèvent une ambiguïté.
- Référence Python : `matematicsolutions/fr-eli-mcp`, commit `d3b5b13` (2026-09-24), Apache-2.0. Toujours la désigner ainsi : « la référence Python ».
- Repo cible : `muchiny/mcp-ligitfrance` (le nom contient probablement une coquille pour « legifrance » : question ouverte Q1).
- Chaque fait technique non trivial doit être sourcé (chemin:ligne de la référence, fichier de fixture, URL, ou rapport de recherche). Ce qui n'est pas vérifié est marqué **NON VÉRIFIÉ** avec la façon de le vérifier.

## 1. Noms
- Nom du produit : `mcp-legifrance` (binaire `mcp-legifrance`). Préfixe des crates : `lf-`.
- Préfixe des outils MCP : `fr_` (on garde les 8 noms de la référence pour faciliter la migration).
- Variables d'environnement : préfixe `LF_` (ex. `LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET`, `LF_PISTE_ENV=sandbox|production`, `LF_PISTE_OAUTH_URL`, `LF_PISTE_BASE_URL`, `LF_AUDIT_DIR`, `LF_LOG`). Compatibilité optionnelle : lecture des `FR_ELI_*` de la référence avec un avertissement de dépréciation.

## 2. Workspace Cargo (répertoire `crates/`)
| Crate | Rôle | Dépend de |
|---|---|---|
| `lf-domain` | Cœur pur : IDs typés, entités, contrat de citation, règles de citation FR, construction d'URL, dates FR, parseur de citations, utilitaires texte purs (nettoyage, découpe en passages, BM25, troncature). Aucune I/O, aucun async. | std + dépendances pures minimales (ex. `thiserror`, `time`/`jiff` sans tz si nécessaire) |
| `lf-application` | Ports (traits) + cas d'usage + décorateurs transverses (`Audited`). Async, sans I/O concrète. | `lf-domain` |
| `lf-adapter-piste` | Adapter sortant Légifrance via PISTE : OAuth2 client_credentials, client `reqwest` (rustls), retries/backoff, limitation de débit, DTO serde minimaux, mapping DTO→domaine, réparation d'encodage. Implémente `LegalCorpus`. | `lf-application`, `lf-domain` |
| `lf-adapter-cache` | Décorateur `CachedCorpus<C: LegalCorpus>` (moka, en mémoire, TTL par opération, coalescence des requêtes identiques). | `lf-application`, `lf-domain` |
| `lf-adapter-audit` | Adapter sortant `AuditLog` : JSONL append-only, rotation, hash d'entrée canonique. | `lf-application` |
| `lf-adapter-mcp` | Adapter entrant : serveur rmcp, outils, resources, prompts, completions, mapping erreurs→MCP, rendu compact. | `lf-application`, `lf-domain` |
| `lf-app` | Composition root, binaire `mcp-legifrance` : config (env + fichier optionnel), câblage, `tracing` sur stderr, choix du transport (stdio par défaut). | tous |
| `xtask` | Tâches de dev : `cargo xtask lock-descriptors`, `cargo xtask record-fixture`, etc. | — |
| `lf-adapter-dila` (v1, plus tard) | Adapter sortant hors ligne sur les dumps DILA (quick-xml + tantivy). Implémente le même `LegalCorpus`. | `lf-application`, `lf-domain` |

Règle de dépendance (vérifiée en CI, ex. `cargo deny` bans / script `cargo metadata`) : `lf-domain` ne dépend d'aucun crate `lf-*` ni de tokio/reqwest/serde_json/rmcp ; `lf-application` ne dépend d'aucun adapter ; aucun adapter ne dépend d'un autre adapter ; seul `lf-app` connaît tout.

## 3. Ports (dans `lf-application::ports`)
- `LegalCorpus` (sortant, un seul trait, implémenté par `PisteCorpus`, décoré par `CachedCorpus`, plus tard `DilaCorpus`) :
  `search(&SearchQuery) -> Result<SearchPage, CorpusError>`, `get_act(&LegiTextId, VersionDate)`, `get_article(&ArticleId)`, `get_decision(&DecisionId)`, `get_deliberation(&CnilTextId)`, `get_convention(&KaliTextId)`, `get_company_agreement(&AccoTextId)` ; ajouts ultérieurs : `article_versions(&ArticleId)` (v0.3), `resolve(&CitationRef)` peut être un cas d'usage composé plutôt qu'une méthode de port.
- `AuditLog` (sortant) : `record(AuditRecord) -> Result<(), AuditError>` — un échec d'écriture fait échouer l'appel d'outil (CONSTITUTION art. 2 de la référence).
- `Clock` (sortant) : `now_utc()`, `today_paris()` — pour rendre testables les dates par défaut.
- Côté entrant : pas de trait ; les cas d'usage sont des structs `…UseCase` exposant `async fn execute(&self, input) -> Result<Output, AppError>`, regroupés dans une façade `LegalService` injectée dans l'adapter MCP.
- Choix async : `async fn` natif dans les traits + génériques (dispatch statique) côté application ; si un `dyn` est nécessaire, le justifier (rapport rmcp).

## 4. Domaine (`lf-domain`)
- IDs : un newtype par préfixe Légifrance (`LegiTextId`, `LegiArtiId`, `JorfTextId`, `KaliTextId`, `KaliArtiId`, `JuriTextId`, `ConsTextId`, `CetaTextId`, `CnilTextId`, `AccoTextId`), construits par `FromStr`/`TryFrom<&str>` avec validation (préfixe + format numérique vérifié sur les fixtures), suffixe `_JJ-MM-AAAA` retiré à la normalisation ; enums `ArticleId { Legi, Kali }` et `DecisionId { Juri, Constit, Cetat }`. Principe : « parse, don't validate ».
- `Fond` : `Loda, Code, Juri, Constit, Cetat, Cnil, Kali, Acco` (+ `Jorf` plus tard), avec le mapping vers le fond API (`LODA_DATE`, `CODE_DATE`, …).
- `Ecli` : newtype validé (`ECLI:FR:…`), jamais fabriqué.
- `CitationContract { eli_uri, human_readable_citation, source_url }` présent sur toute entité citable ; `eli_uri` = URL Légifrance stable (pas d'ELI fabriqué), avec la note ELI.
- Entités : `SearchHit`, `SearchPage`, `Act`, `TocEntry`, `ArticleText`, `Decision`, `Deliberation`, `Convention`, `CompanyAgreement`, `Coverage`.
- Parseur de citations : `lf_domain::citation::parse(&str) -> Vec<Spanned<CitationRef>>` (v0.2).
- Texte : `text::clean` (HTML→texte, espaces, `<mark>`), `text::passages` (découpe), `text::rank` (BM25), `text::truncate` (coupe en fin de phrase).
- Erreurs : `DomainError` (thiserror).

## 5. Erreurs et contrat MCP
- `CorpusError` : `NotFound`, `Upstream { status, retryable }`, `Timeout`, `RateLimited`, `Auth`, `Config`, `Decode`.
- `AppError` : `InvalidArgument`, `NotFound`, `Upstream`, `Config`, `RateLimited`, `Audit`.
- Côté MCP : erreurs métier rendues comme résultat d'outil `isError: true` avec un préfixe de code stable `[invalid_arg]`, `[not_found]`, `[upstream_error]`, `[config_error]` (parité) + `[rate_limited]` ; message actionnable ; jamais de secret ni de token dans un message.

## 5bis. Version du protocole MCP (EXIGENCE UTILISATEUR)
- Le serveur cible la DERNIÈRE révision de la spécification MCP, **2026-07-28** (« MCP v2 », nouvelle génération sans session) : cycle de vie sans `initialize`/`Mcp-Session-Id`, `server/discover`, résultats de list cachables, framework d'extensions, autorisation durcie ; sampling et roots dépréciés (ne pas les utiliser).
- Les détails normatifs viennent des rapports `08-mcp-2026-07-28.md` et `09-rmcp-2026.md` ; le rapport `06-mcp-spec.md` peut décrire une révision antérieure : en cas de divergence, 08/09 font foi.
- Rétrocompatibilité avec les clients encore en 2025-11-25 : stratégie à tirer du rapport 08 (négociation de version), jamais au détriment de la conformité 2026-07-28.
- DÉCIDÉ (rapport 09) : SDK `rmcp` épinglé `~3.5.1` (+ `rmcp-macros` même version), features par défaut + `transport-io` ; MSRV de rmcp = 1.88. rmcp 3.5.1 implémente 2026-07-28 ET sert automatiquement les pairs legacy (dual-era) : pas de couche protocolaire maison. Le protocole reste confiné à `lf-adapter-mcp` (l'API de rmcp a changé de version majeure 4 fois en 2026).
- Versions annoncées explicitement (`supported_protocol_versions`) : `2026-07-28`, `2025-11-25`, `2025-06-18` (à confirmer, Q7). Le serveur est « dual-era » : nécessaire car VS Code (main) ne parle encore que 2025-11-25 via `initialize`, alors que Claude Code sonde `server/discover` en stdio.
- Règles normatives à refléter : `server/discover` obligatoire (fourni par rmcp, servi depuis des données statiques) ; démarrage sans aucun appel réseau (OAuth paresseux au premier appel d'outil) ; ne jamais quitter sur une méthode inattendue ; `ttlMs`/`cacheScope` sur les listes et `resources/read` (valeurs à fixer : Q8) ; `resultType` géré par rmcp ; ressource introuvable → `-32602` (rmcp réécrit) ; codes -32020/-32021/-32022 ; pas de capacité `logging` (dépréciée ; `logging/setLevel` et `ping` supprimés) → journaux uniquement via `tracing` sur stderr ; ni sampling, ni roots, ni elicitation, ni MRTR (inutile en lecture seule) ; pas d'en-tête `x-mcp-header`/`Mcp-Param-*` (bug P0 rmcp #1348) ; `structuredContent`/`outputSchema` TOUJOURS à racine objet (bug rmcp #1337 : jamais `Json<Vec<T>>`) + toujours un `TextContent` de repli ; pas de combinateur à la racine des schémas ni de `$ref` externe.
- Mode HTTP futur (M8) : Streamable HTTP sans session (POST seul, GET/DELETE 405), validation `MCP-Protocol-Version`/`Mcp-Method`/`Mcp-Name` (rmcp), Origin, autorisation OAuth resource server (PRM RFC 9728, validation d'audience, pas de token passthrough vers PISTE), chaque requête authentifiée.

## 6. Outils MCP
- v0.1 (parité + économie de contexte) : `fr_search`, `fr_get_act`, `fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_get_company_agreement`, `fr_coverage`.
  Paramètres ajoutés (optionnels, rétrocompatibles) : `page` pour `fr_search` ; `detail: "summary" | "full"` (défaut `summary`) pour les consultations à texte long ; `offset` + `max_chars` (défaut 4000, max à fixer) avec `next_offset` en sortie ; `focus` (requête libre → top-k passages BM25) pour `fr_get_decision`, `fr_get_deliberation`, `fr_get_text` ; filtres de recherche ciblés (dates, juridiction…) si confirmés par la recherche API.
- v0.2 : `fr_verify_citations(text)`, `fr_resolve(citation)`.
- v0.3 : `fr_get_related(decision_id)`, `fr_diff_article(article_id, date_a, date_b)`, `fr_search_all(query, fonds?)` ; resources (`legifrance://article/{id}`, `legifrance://decision/{id}`, `legifrance://act/{id}`), completions (fonds, codes), prompts.
- v0.4 (optionnel) : transport Streamable HTTP + authentification, sous réserve des CGU PISTE (partage d'un compte).
- v1.0 : `lf-adapter-dila` (index local), sélection d'adapter par configuration.
- Toutes les sorties portent le contrat de citation. Rendu : texte compact lisible par le modèle + `structuredContent` conforme à un `outputSchema` (à confirmer par le rapport MCP/rmcp).
- Annotations : `readOnlyHint=true`, `idempotentHint=true`, `destructiveHint=false`, `openWorldHint=true`.

## 7. Jalons (doivent être identiques partout)
| Jalon | Contenu | Version |
|---|---|---|
| M0 Fondations | workspace, toolchain, lints, rustfmt, CI, cargo-deny, ADR, CONTRIBUTING, NOTICE/licence | — |
| M1 Domaine | IDs, fonds, entités, contrat de citation, règles de citation de parité, URL, dates FR, utilitaires texte | — |
| M2 Adapter PISTE | OAuth, client, retries, rate limit, DTO, mapping, réparation d'encodage, tests wiremock + fixtures | — |
| M3 Application | ports, cas d'usage, `Audited`, `lf-adapter-audit`, `lf-adapter-cache`, `Clock` | — |
| M4 Adapter MCP (parité) | 8 outils, instructions, mapping d'erreurs, descriptor lock, test de dérive des instructions, binaire `lf-app` | — |
| M5 Économie de contexte | `detail`, `offset`/`max_chars`, `focus` BM25, pagination de recherche, budgets | **v0.1.0** (M0→M5) |
| M6 Citations | parseur + corpus de tests, `fr_verify_citations`, `fr_resolve` | **v0.2.0** |
| M7 Navigation | `fr_get_related`, `fr_diff_article`, `fr_search_all`, resources, completions, prompts | **v0.3.0** |
| M8 HTTP (optionnel) | Streamable HTTP, auth, déploiement partagé | v0.4.0 |
| M9 Hors ligne | `lf-adapter-dila` (quick-xml + tantivy), mises à jour quotidiennes | **v1.0.0** |
| Transverse : Distribution | cargo-dist, signature Windows, image OCI, registre MCP | dès v0.1.0 |

## 8. Questions ouvertes (numérotation commune)
- Q1 Nom du repo/binaire (`ligitfrance` → `legifrance` ?).
- Q2 Licence du nouveau projet (Apache-2.0 recommandé, conservation de la mention d'origine et NOTICE).
- Q3 Compte PISTE disponible ? sandbox puis production ?
- Q4 Le mode HTTP partagé est-il compatible avec les CGU PISTE ?
- Q5 Signature Authenticode Windows : certificat disponible ?
- Q6 Ouverture réseau du conteneur de dev (`static.crates.io`, `echanges.dila.gouv.fr`).
- Q7 Versions MCP legacy à servir (proposition : 2025-11-25 et 2025-06-18 ; 2025-03-26 ?).
- Q8 Valeurs `ttlMs`/`cacheScope` (données publiques servies via un compte PISTE : `public` ou `private` ?).
- Q9 Typographie des citations. Proposition retenue par le plan 1 (réversible) : français correct, soit `décision`, `délibération`, mois accentués (`février`, `août`, `décembre`) et `1er` pour le premier du mois. La référence écrit `decision`, `deliberation`, `fevrier` et `1 janvier` (`citations.py:283-318`, `376-378`), alors que Légifrance accentue lui-même ses titres (fixture `consult_cnil.json` : « Délibération SAN-2026-002 du 8 janvier 2026 »).
- Q10 **NON VÉRIFIÉ** : l'URL `/codes/article_lc/<LEGIARTI>` utilisée par la référence pour tout `LEGIARTI` (`citations.py:262`) résout-elle aussi pour l'article d'une loi non codifiée ? À vérifier en M2 : ouvrir l'URL d'un `LEGIARTI` tiré de `consult_loda.json` et noter le code HTTP final.

## 9. Qualité : tests unitaires, couverture, mutation (EXIGENCE UTILISATEUR)
- Principe : **tests unitaires au maximum, mutation testing au maximum, couverture au maximum**. Les seuils ci-dessous bloquent, en local (`make ci`, `make mutants`) comme en CI. Ils ne baissent jamais ; un nouveau crate entre directement à ces seuils.
- TDD strict : test rouge, code minimal, test vert, refactorisation. Toute règle portée de la référence Python cite `fichier:ligne` dans le code ou le test.
- Tests unitaires dans le fichier du code testé (`#[cfg(test)] mod tests`), avec accès aux items privés. Viennent en plus :
  - des propriétés `proptest` pour chaque parseur, normaliseur et utilitaire texte : ne panique jamais, aller-retour, partition exacte ;
  - des cibles `cargo-fuzz` pour les entrées non fiables (identifiants, dates, HTML, texte) ;
  - en M2, les fixtures JSON de la référence pour le mapping DTO.
- Couverture mesurée par `cargo llvm-cov --workspace --lib` : **100 % des lignes, des régions et des fonctions**. Le code de test compte dans la mesure, d'où trois formes interdites dans les tests (détail dans `CONTRIBUTING.md`) :
  - `assert!` avec des arguments de message non capturés ;
  - `assert!(matches!(…))` ;
  - `let … else { panic!() }`.

  Les adapters à I/O isolent leur logique en fonctions pures testées (mapping, retry, backoff, rendu). La glue irréductible (`main.rs`, appel réseau brut) tient dans des fichiers minces, exclus nommément et avec justification.
- Mutation par `cargo-mutants` : **0 mutant `MISSED`** sur les mutants viables. Un `TIMEOUT` (code de sortie 3) bloque aussi. Toute boucle doit donc garantir sa progression, même mutée : consommer au moins un élément par tour, ou borner le nombre de tours. Un mutant de `text::clean` bouclait sans fin à l'essai à blanc. Un mutant n'est exclu que s'il est prouvé équivalent, preuve en commentaire dans `.cargo/mutants.toml` (`exclude_re`).
  - En local, un fichier à la fois : `make mutants FILE=…`, soit `TMPDIR=/var/tmp NEXTEST_TEST_THREADS=2 cargo mutants -j 1 --file …` après contrôle mémoire, selon la règle WSL.
  - En CI, le diff de chaque PR est muté et bloque la PR. Le workspace entier est balayé chaque semaine en 4 shards, avec les cibles de fuzz.
- Mesure de référence : essai à blanc du plan 1 sur M0–M1, le 2026-10-15. Le relevé (nombre de tests, couverture lignes/régions/fonctions, mutants testés et survivants) figure dans l'en-tête du plan 1.
- Outils écartés : `typos`, faux positifs sur le français (30 mots distincts relevés sur l'essai à blanc).
- Fin de jalon : agents `quality-checker`, `test-amplifier`, `security-auditor` puis `code-reviewer`, dans l'ordre de `~/.claude/rules/agent-coordination.md`.
