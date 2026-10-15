# Décisions communes — plan de réécriture Rust/hexagonale du MCP Légifrance

> Document normatif du plan. Tous les autres documents de `docs/plan/` emploient exactement ces noms, ces découpages, ces paramètres et ces jalons. Toute divergence est un défaut du plan.

## 0. Cadre
- Langue des documents : français. Identifiants Rust, crates, champs JSON, noms d'outils MCP : tels quels (ASCII/anglais).
- Le plan décrit QUOI construire, COMMENT, dans quel ordre et avec quels critères d'acceptation. Pas d'implémentation complète : extraits courts (signatures, structs, squelettes, schémas) seulement quand ils lèvent une ambiguïté. Tout extrait de code non compilé est marqué « non compilé, à valider en M0 ».
- Référence Python : `matematicsolutions/fr-eli-mcp`, commit `d3b5b13` (2026-09-24), v0.5.3, Apache-2.0. Toujours la désigner : « la référence Python ».
- Repo cible : `muchiny/mcp-ligitfrance` (coquille probable pour « legifrance » : Q1).
- Sourcer chaque fait non trivial (chemin:ligne de la référence, fixture, URL, ou rapport `docs/recherche/NN-*.md`). Ce qui n'est pas vérifié est marqué **NON VÉRIFIÉ** et renvoie à une vérification `V-xx` (registre dans `09-risques-et-questions.md`).
- Identifiants transverses : défauts de la référence `D-01…D-47` (rapport 02) ; points de parité `P-001…` et écarts `E-01…` (rapport 01) ; tâches `T-Mx-yy` ; risques `R-xx` ; vérifications live `V-xx` ; questions `Q-xx` ; ADR `ADR-00xx`.

## 1. Noms
- Produit et binaire : `mcp-legifrance`. Préfixe des crates : `lf-`.
- Outils MCP : préfixe `fr_` ; on garde les 8 noms de la référence.
- Configuration : variables `LF_*` — `LF_PISTE_ENV=sandbox|production` (défaut `sandbox`, affiché dans `fr_coverage`), `LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET`, `LF_PISTE_OAUTH_URL` / `LF_PISTE_BASE_URL` (surcharges explicites), `LF_AUDIT_DIR` (défaut : répertoire de données utilisateur de la plateforme), `LF_AUDIT_MODE=strict|best-effort` (défaut `strict`), `LF_LOG` (filtre `tracing`), `LF_HTTP_TIMEOUT_S`, `LF_TOOL_DEADLINE_S`. Lecture de compatibilité des `FR_ELI_CLIENT_ID`/`FR_ELI_CLIENT_SECRET`/`FR_ELI_OAUTH_URL`/`FR_ELI_BASE_URL`/`FR_ELI_AUDIT_DIR` avec avertissement de dépréciation sur stderr.
- AUCUNE configuration distante : pas de manifeste téléchargé, aucune télémétrie (ADR-0005, corrige D-xx du manifeste runtime).

## 2. Workspace Cargo
- Édition 2024, `resolver = "3"` explicite dans le `Cargo.toml` racine (workspace virtuel), `[workspace.dependencies]`, `[workspace.lints]` (rust + clippy, dont `unsafe_code = "forbid"`), `rust-version = "1.91"` (MSRV), toolchain épinglée dans `rust-toolchain.toml` sur la stable courante vérifiée (1.99.0, NON VÉRIFIÉ : date de sortie) avec `rustfmt` et `clippy`. Piège : `default-features` hérité d'une dépendance de workspace n'est respecté qu'à partir de Rust 1.99 en édition 2024 → déclarer `default-features = false` au niveau du workspace quand c'est voulu.

| Crate (`crates/`) | Rôle | Dépend de |
|---|---|---|
| `lf-domain` | Cœur pur : IDs typés, `Fond`, entités, contrat de citation, règles de citation FR, URLs Légifrance, dates (civiles), texte (nettoyage, découpe en passages, BM25, troncature), parseur de citations (M6). Pas d'I/O, pas d'async, pas de serde_json. | `thiserror`, `chrono` (sans feature `clock`), `unicode-normalization`, `winnow`, `aho-corasick`, `regex` (M6) |
| `lf-application` | Ports (traits), cas d'usage, façade `LegalService`, décorateur `Audited`, politique de budget de réponse. | `lf-domain` |
| `lf-adapter-piste` | `PisteCorpus: LegalCorpus` : OAuth2 client_credentials, `reqwest` 0.13 (rustls), retries (`backon`), limitation de débit (`governor`), délai global, DTO serde minimaux, mapping DTO→domaine, réparation du mojibake, construction des requêtes `/search`. | `lf-application`, `lf-domain` |
| `lf-adapter-cache` | `CachedCorpus<C: LegalCorpus>` : `moka` (feature `future`), en mémoire, TTL par opération, coalescence des requêtes identiques, clé namespacée par environnement. | `lf-application`, `lf-domain` |
| `lf-adapter-audit` | `JsonlAuditLog: AuditLog` : JSONL append-only, rotation par taille/date, hash SHA-256 canonique de l'entrée. | `lf-application` |
| `lf-adapter-mcp` | Adapter entrant : serveur `rmcp`, outils, resources, prompts, completions, mapping erreurs→MCP, schémas de sortie, rendu. | `lf-application`, `lf-domain` |
| `lf-app` | Composition root, binaire `mcp-legifrance` : `clap` (sous-commandes `serve` (défaut, stdio), `serve-http` (M8), `check-config`), chargement de la config, câblage, `tracing-subscriber` sur **stderr** uniquement, horloge système. | tous |
| `xtask` | `cargo xtask lock-descriptors`, `cargo xtask check-deps` (règles de dépendance via `cargo metadata`), `cargo xtask record-fixture` (identifiants requis), `cargo xtask instructions-size`. | — |
| `lf-adapter-dila` (M9) | `DilaCorpus: LegalCorpus` hors ligne (dumps DILA : `tar`+`flate2`, `quick-xml`, index `tantivy` avec analyseur français). | `lf-application`, `lf-domain` |

L'horloge système (`SystemClock: Clock`, UTC + Europe/Paris via `chrono-tz`) est un module `clock` de `lf-app` : pas de crate dédié.

Règles de dépendance, vérifiées en CI par `cargo xtask check-deps` : `lf-domain` ne dépend d'aucun crate `lf-*` ni de tokio/reqwest/serde_json/rmcp ; `lf-application` ne dépend d'aucun adapter ni de rmcp/reqwest ; aucun adapter ne dépend d'un autre adapter ; seul `lf-app` dépend de tout.

## 3. Ports (`lf-application::ports`)
- Style : traits en async natif façon RPITIT, `fn get_decision(&self, id: &DecisionId) -> impl Future<Output = Result<Decision, CorpusError>> + Send;` implémentés avec `async fn` ; génériques (dispatch statique) ; sélection PISTE/DILA à l'exécution par un `enum AnyCorpus` (M9). Pas de `async_trait`/`dyn` sauf justification (ADR-0012).
- `LegalCorpus` (sortant, unique) : `search(&SearchQuery) -> SearchPage` ; `get_act(&LegiTextId, VersionDate) -> Act` ; `get_article(&ArticleId) -> ArticleText` ; `get_decision(&DecisionId) -> Decision` ; `get_deliberation(&CnilTextId) -> Deliberation` ; `get_convention(&KaliTextId) -> Convention` ; `get_company_agreement(&AccoTextId) -> CompanyAgreement`. Ajouts : M6 `find_article(&CodeRef, &ArticleNum, VersionDate)`, `find_by_number(...)` pour la résolution ; M7 `article_versions(&ArticleId) -> Vec<ArticleVersion>`, `related_links(&ArticleId)`, `list_codes()`.
- `AuditLog` (sortant) : `record(&AuditRecord) -> Result<(), AuditError>`.
- `Clock` (sortant) : `now_utc()`, `today_paris() -> NaiveDate`.
- Entrant : pas de trait ; cas d'usage `…UseCase` avec `async fn execute(&self, input) -> Result<Output, AppError>`, regroupés dans `LegalService<C: LegalCorpus, A: AuditLog, K: Clock>` injecté dans l'adapter MCP. `Audited` enveloppe CHAQUE exécution : succès, `invalid_arg`, `not_found`, erreurs amont — tout est journalisé avec une durée réelle (corrige D-xx audit).

## 4. Domaine (`lf-domain`)
- IDs : newtypes `LegiTextId`, `LegiArtiId`, `JorfTextId`, `KaliTextId`, `KaliArtiId`, `JuriTextId`, `ConsTextId`, `CetaTextId`, `CnilTextId`, `AccoTextId` = préfixe de 8 lettres + 12 chiffres (vérifié sur toutes les fixtures) ; `FromStr` strict ; le suffixe de version `_JJ-MM-AAAA` (LEGITEXT seulement) est séparé en `VersionedId { id, version_start }` à la normalisation. Enums `ArticleId { Legi, Kali }`, `DecisionId { Juri, Constit, Cetat }` : la juridiction et l'URL se déduisent du préfixe validé, jamais du champ `origine` (corrige D-xx).
- `Fond` : `Loda, Code, Juri, Constit, Cetat, Cnil, Kali, Acco` (+ `Jorf` M7) ↔ fond API (`LODA_DATE`, `CODE_DATE`, `JURI`, …).
- `Ecli` : newtype validé `ECLI:FR:…`, jamais fabriqué.
- Contrat de citation v2 (ADR-0009) : chaque entité citable expose `citation` (chaîne au format français correct) et `source_url` (URL legifrance.gouv.fr stable) ; `ecli` seulement s'il est natif ; `eli` seulement si un ELI natif est renvoyé (jamais fabriqué ; aujourd'hui toujours absent). Le champ `eli_uri` de la référence (toujours égal à `source_url`) n'est pas reproduit ; la note ELI passe dans les instructions et `fr_coverage`. Formats corrigés (divergences volontaires de la parité) : « Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 », « CNIL, délibération n° SAN-2026-002 du 8 janvier 2026 », mois accentués, « 1er » ; « Code civil, art. 9 » inchangé.
- Dates : `chrono::NaiveDate` ; epoch ms négatifs supportés (1804) ; sentinelles `2999-01-01` et `2222-02-22` (et `*Computed` nuls) → `None` (« sans date de fin ») ; formats d'entrée multiples (epoch ms entier ou chaîne, ISO `+0000`, `AAAA-MM-JJ`) parsés dans l'adapter ; « aujourd'hui » = date civile Europe/Paris.
- Entités : `SearchHit`, `SearchPage`, `Act`, `TocNode` (sections ET articles, ordonnés par `intOrdre` si présent, dédoublonnés), `ArticleText`, `Decision`, `Deliberation`, `Convention`, `CompanyAgreement`, `Coverage`, `LongText` (texte structuré en paragraphes).
- Texte : `text::from_html` (paragraphes reconstruits depuis `texteHtml` quand disponible, sinon `texte`), `text::clean` (`<mark>`, espaces, NBSP — APRÈS réparation du mojibake), `text::passages`, `text::rank` (BM25 maison, repli accents/casse, mots vides français), `text::window` (fenêtre `offset`/`max_chars` coupée en fin de phrase/paragraphe), `text::summary` (résumé officiel `sommaire`, sinon extraction du dispositif : CONSTIT « LE CONSEIL CONSTITUTIONNEL DÉCIDE : » → « Jugé par », CETAT « D E C I D E » → « Délibéré »).
- Parseur de citations (M6) : `citation::parse(&str) -> Vec<Spanned<CitationRef>>` (winnow 1.0 sur `LocatingSlice`, ancres aho-corasick, vue « clé » repliée 1:1 pour garder les offsets) ; grammaire et corpus : annexe B.
- Erreurs : `DomainError` (thiserror).

## 5. Erreurs
- `CorpusError` : `NotFound`, `InvalidRequest`, `Upstream { status, retryable }`, `Timeout`, `RateLimited { retry_after }`, `Auth` (identifiants refusés), `Config`, `Decode`.
- `AppError` : `InvalidArgument`, `NotFound`, `Upstream`, `RateLimited`, `Config`, `Audit`.
- MCP (ADR-0010) : erreurs métier → résultat `isError: true`, texte seul `[code] message actionnable` avec codes `invalid_arg`, `not_found`, `upstream_error`, `rate_limited`, `config_error`, `audit_error` ; erreurs de protocole (outil inconnu, params non désérialisables quand rmcp les traite ainsi) → JSON-RPC. Une erreur OAuth (400/401/404 sur l'endpoint token) → `config_error`, jamais `not_found` (corrige D-xx). Jamais de secret, de token ni d'URL avec paramètres sensibles dans un message ou un log.

## 6. Protocole MCP (EXIGENCE UTILISATEUR : « MCP v2 »)
- Cible : révision **2026-07-28** (« modern »), serveur **dual-era** : sert aussi `2025-11-25` et `2025-06-18` (legacy, via `initialize`) car VS Code n'envoie encore que `initialize` 2025-11-25 alors que Claude Code sonde `server/discover`. `2025-03-26` non supportée (pas de `structuredContent`) — Q7.
- SDK : `rmcp` + `rmcp-macros` épinglés `=3.5.1` (MSRV 1.88, édition 2024), features par défaut + `transport-io` ; `transport-streamable-http-server` seulement en M8. Montées de version : manuelles, testées (l'API a changé de majeure 4 fois en 2026 ; `LATEST` a changé dans une mineure).
- `supported_protocol_versions()` explicite : `[2026-07-28, 2025-11-25, 2025-06-18]`. `get_info()` renvoie `ServerConfig` avec `Implementation::new("mcp-legifrance", env!("CARGO_PKG_VERSION"))` explicite (jamais `from_build_env`).
- Règles : `server/discover` servi depuis des données statiques ; démarrage SANS appel réseau (OAuth paresseux au premier appel d'outil) ; ne jamais quitter sur une méthode inattendue ; `ttlMs`/`cacheScope` explicites (proposition Q8 : listes `ttlMs = 3 600 000`, `cacheScope = public` ; `resources/read` `ttlMs = 3 600 000`, `public`) ; ressource inconnue → `-32602` ; pas de capacité `logging` (dépréciée) → journaux `tracing` sur stderr seulement, jamais au niveau debug de rmcp en production (il journalise les réponses complètes) ; ni sampling, ni roots, ni elicitation, ni MRTR ; pas de `x-mcp-header` (bug rmcp #1348) ; `outputSchema`/`structuredContent` toujours à racine objet (bug rmcp #1337) ; pas de combinateur racine ni de `$ref` externe ; propriétés de premier niveau ≤ 64 caractères ; noms d'outils `^[a-zA-Z0-9_-]{1,64}$`.
- Instructions serveur : en anglais, ≤ 3 500 caractères (Claude Code coupe à 4 096 ; la référence fait 4 470), testées par un test de dérive (outils et codes d'erreur cités = outils et codes réels) et un test de taille.
- Descriptions d'outils : anglais, 3 à 6 phrases, disent quand utiliser l'outil, les valeurs par défaut et comment obtenir la suite ; verrouillées par le descriptor lock.
- Sortie (ADR-0006) : sur succès, Claude Code et VS Code transmettent au modèle `structuredContent` (pas le texte) → le `structuredContent` EST la réponse compacte : champs `None` omis (`skip_serializing_if`), pas de doublons, pas de métadonnées inutiles ; le `TextContent` de repli contient la sérialisation JSON du même objet (lettre de la spec) ; sur erreur, texte seul. Annotations : `read_only_hint = true`, `destructive_hint = false`, `idempotent_hint = true`, `open_world_hint = true` (service externe ; parité).
- HTTP (M8, optionnel) : Streamable HTTP sans session (POST seul ; GET/DELETE → 405), validation `Origin` (403), écoute sur `127.0.0.1` par défaut, `allowed_hosts`, en-têtes `MCP-Protocol-Version`/`Mcp-Method`/`Mcp-Name` validés par rmcp ; deux profils : local (jeton statique) puis distant (resource server OAuth : PRM RFC 9728, audience RFC 8707, 401/403 `insufficient_scope`, jamais de token passthrough vers PISTE). Conditionné aux CGU (Q4).

## 7. Outils (contrat de référence ; `05-outils-mcp.md` en donne les schémas complets)
Paramètres communs aux consultations à texte long (`fr_get_decision`, `fr_get_deliberation`, `fr_get_text`) :
`detail: "summary" | "full"` (défaut `"summary"`), `offset: u32` (défaut 0, en caractères Unicode), `max_chars: u32` (défaut 4 000, min 500, max 40 000 — sous le seuil de 50 000 caractères de Claude Code), `focus: string` optionnel (si présent : `passages` BM25 au lieu de la fenêtre), `passages: u8` (défaut 3, max 10). Sortie : métadonnées + `citation` + `source_url` + selon le mode `summary` (résumé officiel ou dispositif), `text` (fenêtre) avec `text_length` et `next_offset` (absent en fin de texte), ou `passages: [{offset, text, score}]`.

| Outil | Version | Paramètres | Notes |
|---|---|---|---|
| `fr_search` | v0.1 | `query`, `fond` (enum, défaut `LODA`), `page_size` (1–100, défaut 10), `page` (≥1, défaut 1), `match` (`any`=UN_DES_MOTS par défaut, `all`, `exact`), `date_from`/`date_to` (AAAA-MM-JJ, mappés sur la facette de date du fond quand elle existe), `version_date` (LODA/CODE, défaut aujourd'hui Paris, au jour près → le cache fonctionne) | Sortie : `total`, `page`, `has_more`, `hits[{kind,id,title,date?,nature?,etat?,snippet?(≤280),articles?[{article_id,num,snippet}],citation,source_url}]` ; facettes jamais renvoyées brutes ; `typePagination` renvoyé tel que reçu |
| `fr_get_act` | v0.1 | `text_id` (LEGITEXT), `date` (défaut aujourd'hui Paris), `toc_offset` (défaut 0), `toc_limit` (défaut 100, max 500) | TOC avec titres de sections et articles, profondeur, `next_toc_offset` |
| `fr_get_text` | v0.1 | `article_id` (LEGIARTI/KALIARTI) + paramètres communs | `date_debut`, `date_fin` (None si sentinelle), `etat`, `code_title` |
| `fr_get_decision` | v0.1 | `decision_id` (JURITEXT/CONSTEXT/CETATEXT) + paramètres communs | `ecli` natif, `juridiction`, `formation`, `solution`, `numero_affaire`, `date` |
| `fr_get_deliberation` | v0.1 | `deliberation_id` (CNILTEXT) + paramètres communs | |
| `fr_get_convention` | v0.1 | `convention_id` (KALITEXT), `toc_offset`, `toc_limit` | `idcc`, TOC (KALIARTI → `fr_get_text`) |
| `fr_get_company_agreement` | v0.1 | `agreement_id` (ACCOTEXT) | métadonnées seules (texte = pièce jointe .docx) |
| `fr_coverage` | v0.1 | — | familles, lacunes calculées (jamais vide), environnement PISTE (sandbox/production), état de la configuration ; la note ELI |
| `fr_verify_citations` | v0.2 | `text` (≤ 20 000 caractères), `max_citations` (défaut 50) | par citation : `span`, `raw`, `kind`, `status` (`verified`, `not_found`, `ambiguous`, `unresolvable`, `invalid`), `resolved?{id,citation,source_url}`, `note?` ; vérification d'extrait cité (`quote_mismatch`) en option si le texte contient une citation entre guillemets adjacente |
| `fr_resolve` | v0.2 | `citation` (chaîne libre), `date?` | `candidates[{id,kind,citation,source_url,confidence}]` |
| `fr_get_related` | v0.3 | `id` (article ou décision) | articles : `relatedLinksArticle` ; décisions : `citationJp` + `liens` analysés par le parseur |
| `fr_diff_article` | v0.3 | `article_id`, `date_a`, `date_b` (défaut aujourd'hui) | diff compact (`similar`) entre versions (`getArticleByCid` / `canonicalArticleVersion`) |
| `fr_search_all` | v0.3 | `query`, `fonds` (défaut `[LODA, CODE, JURI, CETAT, CONSTIT]`), `per_fond` (défaut 3, max 10) | requêtes parallèles bornées par le limiteur, résultats groupés par fond |

Resources (v0.3) : templates `legifrance://article/{article_id}`, `legifrance://decision/{decision_id}`, `legifrance://act/{text_id}`, `legifrance://code/{code}/article/{num}` (complétion sur `code` via `/list/code`) ; ressource statique `legifrance://coverage`. Prompts (v0.3) : `analyser_decision(decision_id)`, `verifier_citations(texte)`, `etat_du_droit(question)`. Completions : valeurs de `fond`, noms de codes (≤ 100 valeurs, débit limité).

Métriques d'audit (`output_count_or_size`) : nombre de hits (`fr_search`, `fr_search_all`), nombre d'entrées de TOC renvoyées (`fr_get_act`, `fr_get_convention`), caractères renvoyés (consultations texte), 1 (`fr_get_company_agreement`, `fr_resolve`), nombre de lacunes (`fr_coverage`), nombre de citations (`fr_verify_citations`), nombre de liens (`fr_get_related`), nombre de hunks (`fr_diff_article`), 0 en erreur. Ligne d'audit : `ts`, `tool`, `input_hash`, `output_count_or_size`, `duration_ms`, `status` (`ok`/`error`), `error_code?`, `error?` (≤ 500 caractères, sans secret), `env` (sandbox/production), `server_version`.

## 8. Adapter PISTE (`lf-adapter-piste`)
- OAuth : `POST` form `grant_type=client_credentials`, `client_id`, `client_secret`, `scope=openid` ; jeton en mémoire (`secrecy::SecretString`), renouvelé 60 s avant expiration, rafraîchissement mono-vol (un seul appel concurrent), un 401 API → un seul renouvellement puis échec `Auth` (corrige le 401 qui force un nouveau jeton à chaque tentative). URLs : sandbox `https://sandbox-oauth.piste.gouv.fr/api/oauth/token` + `https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app` ; production `https://oauth.piste.gouv.fr/api/oauth/token` + `https://api.piste.gouv.fr/dila/legifrance/lf-engine-app`.
- HTTP : un seul `reqwest::Client` partagé (pool), `reqwest` 0.13 avec features `json`, `form` et TLS rustls (fournisseur à trancher en M0 : aws-lc par défaut vs ring — R-xx), timeouts connexion 10 s / requête 40 s, User-Agent `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-ligitfrance)`.
- Retries : `backon`, 3 tentatives max sur 429/500/502/503/504 et erreurs réseau, backoff exponentiel avec jitter, respect de `Retry-After`, plafonné par le délai global de l'outil (`LF_TOOL_DEADLINE_S`, défaut 60 s).
- Débit : `governor` (quota par défaut prudent configurable, ex. 5 req/s, rafale 10 — valeurs PISTE réelles NON VÉRIFIÉES : V-xx).
- DTO : structs serde minimales, `#[serde(default)]` partout, helpers `null_default`, `opt_nonempty`, `opt_raw_date`, `opt_string_or_number` ; renommages explicites (piège `titresTM`) ; champs inconnus ignorés ; `serde_path_to_error` pour des erreurs `Decode` lisibles.
- Mojibake : réparation au niveau du corps, tout-ou-rien (si aucun caractère > U+00FF et motif `[U+00C2–U+00F4][U+0080–U+00BF]` ou C1 → octets latin-1 → UTF-8 strict, accepté seulement s'il ne reste aucun motif), AVANT toute normalisation des NBSP ; active par défaut, compteur + `warn` ; origine à confirmer en live (V-xx).
- Requêtes `/search` : `typeChamp` `ALL`, `typeRecherche` selon `match` (`UN_DES_MOTS`/`TOUS_LES_MOTS_DANS_UN_CHAMP`/`EXACTE`), `operateur` `ET`, `sort` `PERTINENCE`, `pageNumber`/`pageSize`, `typePagination` renvoyé ; `DATE_VERSION` `singleDate` = minuit Europe/Paris de `version_date` (stable sur la journée) pour LODA_DATE/CODE_DATE.
- Endpoints : v0.1 `/search`, `/consult/lawDecree`, `/consult/getArticle`, `/consult/juri`, `/consult/cnil`, `/consult/kaliText`, `/consult/acco` ; M6 `/consult/getArticleWithIdAndNum`, `/search` en `EXACTE` sur `NUM_ARTICLE`/`NUM`/`NUM_AFFAIRE`/`NUM_DEC`/`NUM_DELIB`/`IDCC`, `/consult/getJoWithNor`, `/consult/kaliContIdcc`, `/list/code` ; M7 `/consult/getArticleByCid`, `/search/canonicalArticleVersion`, `/consult/relatedLinksArticle`, `/consult/legi/tableMatieres`, `/suggest`. Aucun chemin absent du Swagger 2.4.2.

## 9. Cache (`lf-adapter-cache`)
- En mémoire uniquement en v0.1 (pas de cache disque, pas de pickle) ; clé = (environnement, opération, paramètres normalisés) ; TTL : recherche 1 h, consultations 24 h, listes (codes) 7 j ; pas de mise en cache des erreurs ni des réponses vides ; coalescence des requêtes concurrentes identiques ; capacité bornée (poids en octets). Cache disque optionnel : hors périmètre (réévalué en M9).

## 10. Qualité (résumé ; `06-qualite.md` détaille)
- Tests : unitaires domaine (+ `proptest` pour IDs, dates, fenêtrage), table-driven du parseur sur le corpus de l'annexe B, tests golden de parité depuis `docs/plan/annexes/donnees/parite-golden.json` (divergences volontaires listées), tests d'adapter `wiremock` + fixtures de la référence (avec attribution), tests MCP in-process (`tokio::io::duplex`) + snapshots `insta`, descriptor lock (`tests/tools.lock.json` régénéré par `cargo xtask lock-descriptors`), test de dérive et de taille des instructions, tests JSON-RPC bruts en stdio pour les deux ères (discover moderne + initialize legacy), suite de conformité officielle sur le transport HTTP (M8), smoke tests live opt-in (`--ignored`, identifiants requis).
- CI GitHub Actions : `fmt --check`, `clippy --all-targets -D warnings`, `cargo nextest`, doc-tests, job MSRV 1.91, `cargo deny check` (licences, advisories, bans, sources), `cargo xtask check-deps`, matrice Linux/macOS/Windows, pas de réseau requis hors smoke tests.

## 11. Jalons (identiques partout)
| Jalon | Contenu | Version |
|---|---|---|
| M0 Fondations | workspace, toolchain, lints, CI, cargo-deny, xtask, ADR, CONTRIBUTING, LICENSE + NOTICE, spikes (compilation du squelette rmcp 3.5.1, TLS rustls aws-lc vs ring en cross-compilation) | — |
| M1 Domaine | IDs, fonds, entités, contrat de citation v2, URLs, dates, texte (HTML→paragraphes, passages, BM25, fenêtre, résumé) | — |
| M2 Adapter PISTE | OAuth, client, retries, débit, délai, DTO, mapping, mojibake, wiremock + fixtures, smoke tests live | — |
| M3 Application | ports, cas d'usage, `LegalService`, `Audited`, `lf-adapter-audit`, `lf-adapter-cache`, horloge | — |
| M4 Adapter MCP | rmcp dual-era, 8 outils, instructions, erreurs, `server/discover`, descriptor lock, dérive, tests stdio deux ères, binaire | — |
| M5 Économie de contexte | `detail`, fenêtre `offset`/`max_chars`, `focus` BM25, pagination recherche et TOC, budgets mesurés | **v0.1.0** (M0→M5) |
| M6 Citations | parseur + corpus, `fr_verify_citations`, `fr_resolve` | **v0.2.0** |
| M7 Navigation | `fr_get_related`, `fr_diff_article`, `fr_search_all`, resources, completions, prompts | **v0.3.0** |
| M8 HTTP (optionnel) | Streamable HTTP local puis distant + OAuth resource server | v0.4.0 |
| M9 Hors ligne | `lf-adapter-dila` (dumps, incréments, index tantivy), sélection d'adapter | **v1.0.0** |
| Transverse Distribution | cargo-dist (binaire `dist`), signature Windows/macOS, MCPB, image OCI, registre MCP (`server.json`) | dès v0.1.0 |

## 12. Distribution et licence (résumé ; `08-distribution.md` détaille)
- Binaires par `cargo-dist` (Linux x86_64/aarch64 gnu+musl, macOS x86_64/aarch64, Windows x86_64 msvc) ; signature Authenticode (Smart App Control) et notarisation macOS ; bundle MCPB (registryType `mcpb`, asset de release dont l'URL contient « mcp », `fileSha256`) ; image OCI distroless avec `LABEL io.modelcontextprotocol.server.name` ; `cargo install` possible (registryType `cargo` : jeton `mcp-name:` visible en texte dans le README). `server.json` déclare `LF_PISTE_CLIENT_ID`/`LF_PISTE_CLIENT_SECRET` en `environmentVariables` (`isSecret: true`). Nom de registre : `io.github.muchiny/mcp-legifrance` (dépend de Q1).
- Licence proposée : Apache-2.0 ; `NOTICE` créditant la référence Python (et les fixtures reprises) ; aucune ligne de code reprise des projets AGPL (juridix, mcp-legifrance de Ktulu-Analog) — idées seulement. Données : Licence Ouverte 2.0 (mention de la source « Légifrance / DILA » et de la date de mise à jour), clés PISTE jamais divulguées (CGU).

## 13. Questions ouvertes (numérotation commune ; propositions par défaut entre parenthèses)
- Q1 Nom du repo/binaire : renommer `mcp-ligitfrance` → `mcp-legifrance` ? (oui)
- Q2 Licence : Apache-2.0 + NOTICE ? (oui)
- Q3 Compte PISTE disponible ? Sandbox d'abord puis production ? (oui)
- Q4 Mode HTTP partagé compatible avec les CGU PISTE (clés « strictement réservées à votre application ») ? (local seulement tant que non clarifié)
- Q5 Certificat Authenticode / compte Apple Developer disponibles ? (sinon binaires non signés + avertissement)
- Q6 Ouverture réseau du conteneur de dev : `static.crates.io` (indispensable), `echanges.dila.gouv.fr` (M9).
- Q7 Versions MCP legacy servies : 2025-11-25 et 2025-06-18 (2025-03-26 exclue) ?
- Q8 `ttlMs`/`cacheScope` : 1 h, `public` ?
- Q9 Politique d'audit si le disque est plein : `strict` (l'outil échoue) par défaut ?
- Q10 Mode de recherche par défaut : `any` (parité) ou `all` ?
