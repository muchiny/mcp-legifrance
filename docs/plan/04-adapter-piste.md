# 04 — Adapter PISTE (`lf-adapter-piste`) : OAuth, HTTP, résilience, requêtes, DTO, mapping

> **Statut** : M0-M1 livrés (plan 1), le reste est un plan. Ce document applique [00-decisions.md](00-decisions.md) (révision 2), qui est normatif et l'emporte en cas de doute.
> **Document propriétaire** (00 §0) : ce document fait foi pour les internes de `lf-adapter-piste` (modules, configuration, classification « introuvable », stratégies par endpoint) et pour ses fixtures (disposition, sidecars, `cargo xtask record-fixture`). Il renvoie sans recopier le détail à [02-architecture.md](02-architecture.md) (dépendances, règles d'architecture, correspondance des erreurs, spans), à [05-outils-mcp.md](05-outils-mcp.md) (gabarits des messages vus par le modèle), à [06-qualite.md](06-qualite.md) (chemins des fichiers de test, CI, lints) et à [09-risques-et-questions.md](09-risques-et-questions.md) (registres R-xx et V-xx).
> **Jalons** : tâche 0 de M2 (spikes TLS, proxy, `backon`, `governor` ; 00 §11.1), **M2** (tout le socle v0.1 : OAuth, client, réessais, débit, délai, DTO, mapping, mojibake, tests wiremock et fixtures, smoke tests live ; V-66, V-68, V-76 et V-96 bloquantes, 00 §11, 09 §2.18), M5 (pagination de la recherche vérifiée en direct), M6 (endpoints de résolution), M7 (endpoints de navigation).
> **Lecteurs** : développeurs de `lf-adapter-piste`, et de `lf-app` (composition, configuration) et `lf-adapter-cache` (clés, TTL).
> Les extraits Rust sont **non compilés, à valider au jalon qui les implémente**. Le domaine livré (`lf-domain`, M1) fait foi pour ses types : dates en `CivilDate` (`crates/lf-domain/src/date.rs`), identifiants `Id<K>(String)` ; ce qui manque encore au domaine est une tâche de tête de M2 (00 §11.1). Les points non vérifiés sont marqués **NON VÉRIFIÉ** avec une étiquette `[v:…]`. Le registre `V-xx` les reprend dans [09-risques-et-questions.md](09-risques-et-questions.md).

Documents liés : [02-architecture.md](02-architecture.md) (ports, erreurs, décorateurs), [03-domaine.md](03-domaine.md) (types construits par le mapping), [05-outils-mcp.md](05-outils-mcp.md) (paramètres et rendu), [06-qualite.md](06-qualite.md) (stratégie de test et CI), [plans d'implémentation](../superpowers/plans/) (tâches : « plan N, tâche M »), [08-distribution.md](08-distribution.md) (cibles de compilation, images), [09-risques-et-questions.md](09-risques-et-questions.md), [annexes/A-parite.md](annexes/A-parite.md), [annexes/C-api-legifrance.md](annexes/C-api-legifrance.md), ADR-0004, ADR-0005, ADR-0007, ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0014 (index : [../adr/README.md](../adr/README.md)).

---

## 0. Résumé

- `lf-adapter-piste` fournit `PisteCorpus: LegalCorpus`, la seule implémentation du port sortant jusqu'à M9. Il traduit chaque appel du port en requêtes `POST` JSON vers l'API Légifrance 2.4.2 derrière PISTE, puis convertit la réponse en entités de `lf-domain`. Il ne connaît ni MCP, ni l'audit, ni le cache.
- **Environnements** : `LF_PISTE_ENV=sandbox|production` (défaut `sandbox`) fixe ensemble l'URL OAuth et la base de l'API. Les surcharges `LF_PISTE_OAUTH_URL` et `LF_PISTE_BASE_URL` sont validées (https, pas d'identifiants dans l'URL, cohérence d'environnement). Aucune URL ne vient d'ailleurs (ADR-0005, corrige D-04).
- **OAuth2 client_credentials** : jeton en mémoire (`SecretString`), renouvelé 60 s avant expiration, rafraîchissement mono-vol. Sur l'API, un 401 déclenche **un seul** renouvellement, puis `CorpusError::Auth`. Une erreur de l'endpoint token n'est jamais `NotFound` (corrige D-10, D-12, E-20).
- **HTTP** : un `reqwest::Client` 0.13 unique (rustls, fournisseur crypto tranché par le spike TLS (tâche 0 de M2)), connexion 10 s, requête 40 s, aucune redirection suivie, corps plafonné à 16 Mio, User-Agent versionné (corrige D-06, D-41, D-43).
- **Résilience** : `backon` (3 tentatives au plus sur 429/502/503/504 et erreurs réseau, 2 au plus sur 500 puis `retryable: false` ; backoff exponentiel avec gigue, `Retry-After`), respect du délai global par outil `LF_TOOL_DEADLINE_S` (60 s), lu dans `Deadline` (variable locale de tâche posée par `Audited`, 00 §3), `governor` (5 req/s, rafale 10, `LF_PISTE_RPS`/`LF_PISTE_BURST`) consulté par `check()` sur une horloge bâtie sur `MonoClock` (jamais `until_ready`), sémaphore de 4 requêtes en vol (`LF_PISTE_CONCURRENCY`) (corrige D-12, D-13, D-14).
- **Requêtes `/search`** : payload de la référence, `typeRecherche` choisi par `match`, `DATE_VERSION` à **midi** Europe/Paris (stable sur la journée, corrige D-01 ; confirmé par V-76 en M2), facettes de date par fond tirées de la documentation officielle, `typePagination` mémorisé par l'adapter (`PaginationMemo`) et renvoyé à PISTE, jamais exposé au modèle (00 §7).
- **DTO** serde minimaux et tolérants, un module par endpoint, `serde_path_to_error` pour des `Decode` lisibles. **Mapping** en fonctions pures vers les entités de 03.
- **Mojibake** : réparation tout-ou-rien au niveau du corps, avant serde et avant toute normalisation des NBSP, active par défaut (`LF_PISTE_MOJIBAKE_REPAIR=off` la désactive), comptée et journalisée en `warn`.
- **Tests** : une cinquantaine de scénarios wiremock, des tests de contrat sur les 16 fixtures de la référence, `cargo xtask record-fixture` pour enregistrer des fixtures brutes avec métadonnées, et des smoke tests live opt-in.

---

## 1. Rôle et frontières

### 1.1 Responsabilités

| Responsabilité | Où dans le crate | Défauts corrigés |
|---|---|---|
| Configuration PISTE validée (URLs, identifiants, délais, débit) | `config.rs` | D-04, D-18, D-39, D-40 |
| Jeton OAuth2 client_credentials, mono-vol | `auth.rs` | D-10, D-12 |
| Client HTTP unique, lecture bornée des corps | `http.rs` | D-06, D-41, D-43 |
| Réessais, délai global, limitation de débit et de concurrence | `retry.rs`, `limit.rs`, `deadline.rs` | D-12, D-13, D-14 |
| Décodage UTF-8 strict et réparation du mojibake | `encoding.rs` (appelle `lf_domain::text::encoding::repair_latin1_mojibake` ; module du domaine à créer en tâche de tête de M2, 00 §11.1, reliquat de M1, avant `encoding.rs` ; nom du module de l'adapter à trancher : `encoding.rs` ici, `mojibake.rs` en 02) | D-35 (côté production) |
| Construction des corps de requête (`/search`, consultations) | `request/` | D-01, D-09, D-32 |
| DTO serde minimaux | `dto/` | D-11, D-23, D-24 |
| Mapping DTO vers entités du domaine | `map/` | D-20, D-21, D-25, D-26, D-28, D-30, D-33 |
| Classification des réponses en `CorpusError` | `classify.rs` | D-10, D-11 |
| Mémos internes (`PaginationMemo` pour `typePagination`, table des codes `CodeTable` en M6) | `memo.rs` | D-09 |

### 1.2 Ce que le crate ne fait pas

| Responsabilité | Crate | Raison |
|---|---|---|
| Noms d'outils, schémas, messages `[code] …`, rendu compact | `lf-adapter-mcp` | rien de MCP ici : ni `rmcp`, ni `schemars` (00 §2) |
| Journal d'audit, mesure de `duration_ms` | `lf-application` (`Audited`) + `lf-adapter-audit` | l'adapter ne journalise aucun appel d'outil (§12.4) |
| Cache des réponses | `lf-adapter-cache` (`CachedCorpus<PisteCorpus>`) | l'adapter ne met rien en cache, hors deux mémos techniques (§12.3) |
| « Aujourd'hui » à Paris | port `Clock` (`lf-app`) | l'application passe une `VersionDate` déjà résolue (03 §6.4) |
| Validation des paramètres utilisateur | `lf-domain` (constructeurs), appelé par les cas d'usage de `lf-application`, sous `Audited` (02 §6.1) ; `lf-adapter-mcp` ne fait que le décodage JSON des arguments (00 §5) | l'adapter reçoit des types déjà valides (`DecisionId`, `QueryText`, `PageSize`…) ; les rejets `invalid_arg` de `fr_search` (00 §7) sont faits par `SearchQuery::new` (03 §4.1), avant l'appel du port |
| Règles de citation, URLs Légifrance, découpage du texte | `lf-domain` | le mapping appelle les constructeurs du domaine |
| Lecture des variables d'environnement | `lf-app` | l'adapter reçoit une `PisteConfig` (§2.3) |

Règles de dépendance (00 §2) : `lf-adapter-piste` dépend de `lf-application` et de `lf-domain`, jamais d'un autre adapter. `reqwest` n'apparaît que dans ce crate (recherche 04 §5.3).

### 1.3 Méthodes du port implémentées

| Méthode de `LegalCorpus` (00 §3) | Endpoint(s) | Jalon |
|---|---|---|
| `search(&SearchQuery) -> SearchPage` | `POST /search` | M2 |
| `get_act(&LegiTextId, VersionDate) -> Act` | `POST /consult/lawDecree` ; `/consult/legi/tableMatieres` (nature CODE) pour un code si V-96, bloquante en M2, montre que `lawDecree` sur un code dépasse 4 Mio ou 10 s (00 §8, §7.3) | M2 |
| `get_article(&ArticleId) -> ArticleText` | `POST /consult/getArticle` (LEGIARTI et KALIARTI) | M2 |
| `get_decision(&DecisionId) -> Decision` | `POST /consult/juri` (JURITEXT, CONSTEXT, CETATEXT) | M2 |
| `get_deliberation(&CnilTextId) -> Deliberation` | `POST /consult/cnil` | M2 |
| `get_convention(&KaliTextId) -> Convention` | `POST /consult/kaliText` | M2 |
| `get_company_agreement(&AccoTextId) -> CompanyAgreement` | `POST /consult/acco` | M2 |
| `find_article(&CodeRef, &ArticleNum, VersionDate)` | `/consult/getArticleWithIdAndNum` (version en vigueur seulement) ; autre date : `/search` `EXACTE` sur `NUM_ARTICLE` + `DATE_VERSION`, hits filtrés par `cid` ; table des codes (`/list/code`) | M6 |
| `find_by_number(&NumberQuery)` | `/search` en `EXACTE` (`NUM_ARTICLE`, `NUM`, `NUM_AFFAIRE`, `NUM_DEC`, `NUM_DELIB`, `IDCC`), `/search` filtré par la facette `ECLI` (JURI, CETAT ; V-128), `/consult/getJoWithNor`, `/consult/kaliContIdcc`, `/consult/getArticleWithIdEliOrAlias` et `/consult/eliAndAliasRedirectionTexte` (ELI cités, jamais fabriqués ; V-133) — forme de `NumberQuery` : annexe B §6.6 et 03 §4.11 | M6 |
| `list_codes() -> Vec<CodeListing>` | `/list/code` (codes abrogés compris, avec leur `etat`) ; chargée paresseusement au premier besoin, jamais au démarrage ; une seule table (`CodeTable`, TTL 7 j) sert aussi `find_article` et les completions de M7 (00 §3) | M6 |
| `article_versions(&ArticleId) -> Vec<ArticleVersion>` | `/search/canonicalArticleVersion`, repli `/consult/getArticleByCid` | M7 |
| `related_links(&ArticleId) -> RelatedLinks` | `/consult/relatedLinksArticle` | M7 |

Les traits sont en async natif (RPITIT + `Send`, ADR-0012). L'implémentation utilise `async fn`. La sélection PISTE/DILA par `enum AnyCorpus` arrive en M9 (ADR-0004) et ne change rien ici.

```rust
// lf-adapter-piste/src/lib.rs — non compilé, à valider au jalon qui l'implémente
#[derive(Clone)]
pub struct PisteCorpus<K: MonoClock = TokioClock, S: Sleeper = TokioSleeper> { inner: Arc<Inner<K, S>> }

impl PisteCorpus {
    /// Aucune E/S (02 §3.3). Le client reqwest est construit au premier appel (OnceCell, mono-vol) ;
    /// un échec de construction (fournisseur TLS, racines) devient CorpusError::Config à l'appel.
    pub fn new(cfg: PisteConfig) -> Self;
}
impl<K: MonoClock, S: Sleeper> PisteCorpus<K, S> {
    pub fn environment(&self) -> PisteEnvironment;                 // lf_domain::PisteEnvironment : cache (namespace), fr_coverage
    pub fn config_problem(&self) -> Option<&ConfigProblem>;        // identifiants absents, URL invalide…
    pub fn mojibake_repairs(&self) -> u64;                         // compteur (02 §3.3)
    pub fn stats(&self) -> StatsSnapshot;                          // tous les compteurs en mémoire, jamais exportés (ADR-0005)
    pub async fn check_credentials(&self) -> Result<(), CorpusError>; // `check-config --online` (02 §3.3, 08)
}
impl<K: MonoClock, S: Sleeper> LegalCorpus for PisteCorpus<K, S> { /* async fn … */ }
```

`MonoClock` (temps monotone) et `Sleeper` (attente) sont deux traits **internes** au crate, pas des ports. Ils existent pour rendre déterministes les tests de jeton, de backoff, de `Retry-After`, de débit et de délai restant sans geler l'horloge `tokio` (§13.1). En production : `TokioClock` (`tokio::time::Instant`) et `TokioSleeper` (`tokio::time::sleep`). L'environnement est le `lf_domain::PisteEnvironment` (03 §4.8), seul type d'environnement du workspace : l'adapter n'en définit pas d'autre, car `lf-adapter-cache` et `lf-application` l'emploient sans dépendre de lui (R3, 02 §3.3).

### 1.4 Dépendances

Versions de référence relevées dans l'index crates.io le 2026-10-15 (recherche 04 §4.1), **pas des planchers** (00 §2, AM-26) : `[workspace.dependencies]` écrit l'exigence au niveau mineur (`reqwest = "0.13"`, `tokio = "1.53"`, `serde_json = "1.0"`, `rustls = "0.23"`…, 02 §3.0.1). Le `Cargo.lock` livré avec M0 (plan 1) fait foi pour les dépendances déjà verrouillées ; chaque dépendance ajoutée en M2 retient la plus haute version publiée depuis au moins 14 jours à la date de sa PR (pubtime de l'index sparse, contrôlé par script), fixée par `cargo update -p <crate> --precise <version>` ; si aucune version de la série mineure n'est assez ancienne, l'exigence descend d'une version mineure et c'est elle qui est écrite (00 §2 ; règle et versions de référence : 02 §3.0.1 ; verrou de M0-M1 livré, proptest 1.12.0, R-22). La table décrit la cible : une dépendance n'est déclarée dans le manifeste du crate que lorsqu'un code l'emploie, sans déclaration d'avance dans le squelette, pour que `cargo machete` reste vert (00 §2). Les features sont celles de ce crate. Elles se déclarent `default-features = false` au niveau du workspace quand c'est voulu (piège d'héritage, 00 §2) ; les déclarations de workspace et le confinement des crates sont fixés par 02 §3.0 et §4.1 (document propriétaire, 00 §0). Au workspace, reqwest ne porte que `json` et `form` ; ce crate ajoute `gzip`, `system-proxy` et la feature TLS retenue au spike, puisque les features sont additives et qu'un membre ne peut pas en retirer.

| Crate | Version de référence | Features | Usage |
|---|---|---|---|
| `reqwest` | 0.13.5 | `json`, `form`, `rustls` **ou** `rustls-no-provider` (spike TLS (tâche 0 de M2)), `gzip`, `system-proxy` | client HTTP (§4) |
| `rustls` | 0.23.46 | seulement si `rustls-no-provider` : `ring`, `std`, `tls12`, `logging` | fournisseur crypto (§4.1) |
| `tokio` | 1.53.3 | `time`, `sync` | délais, sémaphore, verrou du jeton |
| `serde`, `serde_json` | 1.0.229, 1.0.154 | `derive` | DTO |
| `serde_path_to_error` | 0.1.20 | — | chemin JSON des erreurs `Decode` (§8.5) |
| `secrecy` | 0.10.3 | `serde` | `client_secret` et jeton (§3.6) |
| `backon` | 1.6.0 | `std` ; `tokio-sleep` seulement si le spike de la tâche 0 de M2 montre que backon l'exige : toutes les attentes passent par le `Sleeper` du crate (§5.2) | réessais (§5.2) |
| `governor` | 0.10.4 | `std` (sans `quanta`, `jitter` ni `dashmap`) | limitation de débit (§5.4) |
| `chrono`, `chrono-tz` | 0.4.45, 0.10.4 | `std` | midi Europe/Paris pour `singleDate` (§6.3) et date HTTP de `Retry-After` ; `chrono-tz` n'est admis que dans `lf-app` (`clock`) et dans ce crate (00 §2) ; **hors du domaine** : `lf-domain` reste limité à `thiserror` (liste blanche de `xtask/src/arch.rs`, `DOMAIN_ALLOWED`) et porte ses dates en `CivilDate` (00 §2, C3) |
| `tracing` | 0.1.44 | — | journaux sur stderr (§12.5) |
| `thiserror` | 2.0.21 | — | erreurs internes |
| `reqwest::Url` (ré-export de `url` 2.5.8) | — | — | validation de la configuration ; **pas** de dépendance directe à `url` (02 §3.3) |
| dev : `wiremock`, `insta`, `proptest`, `tokio` (`macros`, `rt-multi-thread`) | 0.6.5, 1.49.0, 1.12.0 | `insta` : `json`, `redactions` | tests (§13) |

`charset` (décodage selon l'en-tête `Content-Type`) est **exclu** : le corps est toujours décodé en UTF-8 strict à partir des octets (§4.4). `http2` n'est pas activé (00 §8) : HTTP/1.1 avec réutilisation des connexions suffit à 4 requêtes en vol.

Feature Cargo du crate : `record`, qui expose une API brute (statut, en-têtes utiles, octets) sous `#[cfg(feature = "record")]`, utilisée seulement par `examples/record_fixture.rs` (§13.4) et jamais activée dans le binaire.

### 1.5 Organisation des modules

```text
crates/lf-adapter-piste/src/
  lib.rs          PisteCorpus, ré-exports (PisteConfig, PisteEndpoints, RateQuota, StatsSnapshot)
  config.rs       PisteEndpoints, PisteCredentials, PisteConfig, RateQuota, ConfigWarning, ConfigProblem,
                  validation (§2) ; l'environnement est lf_domain::PisteEnvironment
  auth.rs         TokenManager, TokenLease, DTO de l'endpoint token (§3)
  http.rs         construction du client, envoi d'une tentative, lecture bornée du corps (§4)
  encoding.rs     decode_body : UTF-8 strict puis réparation du mojibake (§4.4, §10)
  classify.rs     statut + corps → issue d'une tentative (§5.1, §11)
  retry.rs        politique de réessai backon, Retry-After (§5.2)
  deadline.rs     CallDeadline : lecture de lf_application::deadline::Deadline (durée restante),
                  convertie en échéance MonoClock à l'entrée de la méthode du port (§5.3)
  limit.rs        RateGate : sémaphore + governor (check(), horloge bâtie sur MonoClock) (§5.4, §5.5)
  time.rs         MonoClock, Sleeper, TokioClock, TokioSleeper (+ ManualClock, RecordingSleeper en cfg(test))
  request/        search.rs (SearchRequestDto), consult.rs (corps des consultations), number.rs (M6)
  dto/            de.rs (helpers), search.rs, text.rs, article.rs, juri.rs, acco.rs, error.rs, oauth.rs,
                  jorf.rs, kalicont.rs, codes.rs (M6), versions.rs, links.rs (M7)
  map/            dates.rs, search.rs, act.rs, article.rs, decision.rs, deliberation.rs, convention.rs,
                  agreement.rs (+ M6/M7)
  memo.rs         PaginationMemo (M2), CodeTable (M6) (§12.3)
  corpus.rs       impl LegalCorpus for PisteCorpus
  raw.rs          API brute, seulement sous #[cfg(feature = "record")] (§13.4)
crates/lf-adapter-piste/examples/
  record_fixture.rs   (feature record) lancé par cargo xtask record-fixture (§13.4)
crates/lf-adapter-piste/tests/
  fixtures/reference/*.json (+ .meta.toml)   16 fixtures de la référence (attribution NOTICE)
  fixtures/recorded/<endpoint>/*.json (+ .meta.toml)   fixtures enregistrées par xtask
  fixtures/synthetic/   cas construits (listes null, sentinelles, types mêlés, corps d'erreur)
  live.rs   smoke tests live, `#[ignore]` (§13.5)
```

Les tests unitaires et wiremock vivent **dans le crate**, en `#[cfg(test)] mod tests` du fichier testé (async pour wiremock), conformément à l'ADR-0002 et à 00 §10 (la liste de 06 §1.4 fait foi pour les chemins) : jeton (`auth.rs`), résilience (`retry.rs`, `limit.rs`, `deadline.rs`), corps (`http.rs`, `encoding.rs`), payloads (`request/search.rs`), mapping et contrats sur fixtures (`map/*.rs`, via `include_bytes!`) et scénarios d'endpoints (`corpus.rs`). `tests/` est réservé à `live.rs`, aux tests e2e du binaire et aux compléments, car `make cov` mesure `--lib` : un code exercé seulement par `tests/*.rs` compterait comme non couvert. La glue HTTP irréductible est exclue **nommément** ; prévoir en M2 un `--ignore-filename-regex` dans `COV_FLAGS` du `Makefile`, en même temps que les `exclude_globs` justifiés de `.cargo/mutants.toml`. Les noms de fichiers de test cités plus bas (`oauth.rs`, `resilience.rs`…) désignent désormais ces modules `tests`.

Les fixtures sont protégées de toute conversion de fin de ligne (`**/tests/fixtures/** -text` dans `.gitattributes`, 06 §1.2, principe PT5), condition de stabilité de `sha256_raw` sous Windows (D-35).

### 1.6 Pipeline d'un appel

```mermaid
flowchart TD
  P["méthode du port, ex. get_decision(&DecisionId)"] --> D["reste = Deadline::current_or(tool_deadline) ; échéance convertie en MonoClock"]
  D --> C{"configuration utilisable ?"}
  C -- non --> EC["CorpusError::Config, sans aucune E/S"]
  C -- oui --> B["corps JSON minimal (request/)"]
  B --> L["boucle de réessai backon : 3 tentatives au plus (2 sur un 500)"]
  L --> G["RateGate : sémaphore (4) puis governor check() (5/s, rafale 10)"]
  G --> T["TokenManager::token() mono-vol"]
  T --> H["POST reqwest, timeout = min(40 s, reste du délai)"]
  H --> K{"classification §5.1"}
  K -- "401, 1re fois" --> R["invalidate(génération), nouveau jeton, renvoi immédiat"]
  R --> H
  K -- "429 / 500 / 502 / 503 / 504 / réseau, si budget et reste du délai le permettent" --> L
  K -- "2xx" --> RB["lecture bornée (16 Mio)"]
  RB --> U["UTF-8 strict"] --> M["réparation du mojibake (corps entier)"]
  M --> SD["serde_path_to_error → DTO"] --> MP["mapping → entité lf-domain"]
  K -- "autres" --> E["CorpusError"]
```

---

## 2. Environnements et configuration

### 2.1 Sandbox et production

| Aspect | Sandbox | Production | Source |
|---|---|---|---|
| Endpoint token | `https://sandbox-oauth.piste.gouv.fr/api/oauth/token` | `https://oauth.piste.gouv.fr/api/oauth/token` | exemple officiel DILA (recherche 05 §3.1, `exemples-d-utilisation-de-l-api.md:115`) ; Swagger 2.4.2 `securityDefinitions…tokenUrl` (prod) |
| Base de l'API | `https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app` | `https://api.piste.gouv.fr/dila/legifrance/lf-engine-app` | Swagger 2.4.2 `host` + `basePath` des copies sandbox et prod (recherche 05 §1.1) ; référence client.py:28-29 |
| Surface d'API | 68 opérations, Swagger 2.4.2 | identique (diff programmatique des deux copies : seuls `host`, `schemes` et URLs OAuth diffèrent) | recherche 05 §0 |
| Identifiants | application PISTE sandbox | application PISTE production, **distincte** : des identifiants sandbox présentés à `oauth.piste.gouv.fr` reçoivent `invalid_client` | référence SOURCES.md:12 (recherche 05 §3.2) |
| Données | « Les données ne sont pas identiques à celles de Production » ; les correctifs y arrivent d'abord | données de référence | Swagger `info.description` (recherche 05 §3.2) |
| Engagement de service | **aucun** : tests de performance, réindexation, indisponibilité « sans information préalable » | objectif de 95 % par jour, engagement de moyens, aucun niveau minimal garanti | CGU API Légifrance 2022, `cgu-legifrance-api.md:187-202` (recherche 05 §3.2) |
| Quotas | consultables dans PISTE (« Applications » → « Actions » → « Consulter les quotas ») | idem ; modifiables par la DILA avec information par courriel | `cgu-legifrance-api.md:212-231` (recherche 05 §3.3) |

Les URLs de production sont documentées (Swagger) mais n'ont jamais été appelées par nous ni par la référence, qui n'avait qu'une application sandbox : **NON VÉRIFIÉ** [V-59 · oauth-production-live]. L'ampleur de l'écart de données entre sandbox et production est **NON VÉRIFIÉ** [V-61 · sandbox-prod-donnees]. Les valeurs numériques des quotas ne sont pas publiques : **NON VÉRIFIÉ** [V-62 · quota-piste-prod].

Conséquences pour l'adapter :

- l'environnement est une donnée de premier rang : `PisteCorpus::environment()` alimente l'espace de noms du cache (00 §9, corrige D-15) et `fr_coverage` (00 §7, corrige D-18) ;
- une erreur `invalid_client` doit produire un message qui suggère un mélange d'environnements (§3.5) ;
- les fixtures enregistrées portent leur environnement dans leurs métadonnées (§13.4) : un golden sandbox n'est pas comparé à une réponse de production.

### 2.2 Variables lues (par `lf-app`)

Liste fermée de 00 §1 ; formats, priorités et messages détaillés : 02 §8.2, qui fait foi. Ce tableau ne fixe que le champ de `PisteConfig` alimenté par chaque variable.

| Variable (00 §1) | Défaut | Champ de `PisteConfig` | Remarques |
|---|---|---|---|
| `LF_PISTE_ENV` | `sandbox` | `env` (`lf_domain::PisteEnvironment`) | `sandbox` ou `production`, insensible à la casse ; toute autre valeur est un problème de configuration (mode dégradé, §2.4) |
| `LF_PISTE_CLIENT_ID` | — | `credentials.client_id` | `trim()` ; vide = absent (parité référence client.py:51-52) |
| `LF_PISTE_CLIENT_SECRET` | — | `credentials.client_secret` | idem, enveloppé immédiatement dans `SecretString` |
| `LF_PISTE_OAUTH_URL` | URL officielle de `env` | `endpoints.oauth_url` | surcharge explicite, validée (§2.3) |
| `LF_PISTE_BASE_URL` | URL officielle de `env` | `endpoints.base_url` | surcharge explicite, validée ; `/` final retiré (parité client.py:47) |
| `LF_HTTP_TIMEOUT_S` | `40` (1..=300) | `request_timeout` | délai total d'une tentative HTTP (connexion comprise, corps compris) |
| `LF_TOOL_DEADLINE_S` | `60` (1..=600) | `tool_deadline` | délai global par défaut quand aucun `Deadline` n'est publié (§5.3) |
| `FR_ELI_CLIENT_ID`, `FR_ELI_CLIENT_SECRET`, `FR_ELI_OAUTH_URL`, `FR_ELI_BASE_URL` | — | mêmes champs | compatibilité : lues seulement si la variable `LF_*` correspondante est absente, avec un avertissement de dépréciation sur stderr ; si les deux sont définies, `LF_*` l'emporte et un avertissement le signale. Les surcharges `FR_ELI_*_URL` suivent les règles du §2.3 |
| `LF_PISTE_RPS` | `5` (1..=100) | `rate.per_second` | réglage avancé (00 §1) |
| `LF_PISTE_BURST` | `10` (1..=100) | `rate.burst` | réglage avancé |
| `LF_PISTE_CONCURRENCY` | `4` (1..=16) | `max_concurrency` | requêtes API en vol (§5.5) ; réglage avancé |
| `LF_PISTE_MOJIBAKE_REPAIR` | `on` (`on`\|`off`) | `repair_mojibake` | réparation du mojibake (§10) ; réglage avancé |

Toute valeur invalide d'une variable numérique ou de `LF_PISTE_MOJIBAKE_REPAIR` est remplacée par le défaut, avec un `warn!` et une ligne de `check-config` (00 §1). Les quatre réglages avancés ne sont déclarés ni dans `server.json` ni dans le manifeste MCPB : README et `check-config` seulement (08). Les variables héritées `FR_ELI_CACHE_DIR`, `FR_ELI_RUNTIME_URL`, `FR_ELI_ENV`, `PISTE_CLIENT_ID` et `PISTE_CLIENT_SECRET` ne sont jamais lues ; si elles sont définies, `lf-app` émet un avertissement « ignored » au démarrage et dans `check-config`.

Le connecteur ne lit aucune configuration distante, ne télécharge aucun manifeste et n'émet aucune télémétrie (ADR-0005, corrige D-04 et D-05). La configuration est lue **une fois** au démarrage (corrige E-24 : la référence relisait l'environnement à chaque appel).

### 2.3 `PisteConfig` et validation

```rust
// lf-adapter-piste/src/config.rs — non compilé, à valider au jalon qui l'implémente
use lf_domain::PisteEnvironment;   // seul type d'environnement du workspace (03 §4.8) ; Url = reqwest::Url

pub struct PisteEndpoints { pub oauth_url: Url, pub base_url: Url, pub official: bool }

pub struct PisteCredentials { pub client_id: String, pub client_secret: SecretString }

pub struct RateQuota { pub per_second: NonZeroU32, pub burst: NonZeroU32 }   // nom exporté fixé par 02 §3.3

pub struct PisteConfig {
    pub env: PisteEnvironment,
    pub endpoints: Result<PisteEndpoints, ConfigProblem>, // un problème n'empêche pas le démarrage
    pub credentials: Option<PisteCredentials>,            // None ⇒ CorpusError::Config sans E/S (D-39)
    pub user_agent: String,                               // construit par lf-app (§4.2)
    pub connect_timeout: Duration,                        // 10 s
    pub request_timeout: Duration,                        // LF_HTTP_TIMEOUT_S, 40 s
    pub tool_deadline: Duration,                          // LF_TOOL_DEADLINE_S, 60 s
    pub rate: RateQuota,                                  // LF_PISTE_RPS, LF_PISTE_BURST : 5/s, rafale 10
    pub max_concurrency: NonZeroU16,                      // LF_PISTE_CONCURRENCY : 4
    pub max_body_bytes: u64,                              // 16 Mio (00 §8)
    pub repair_mojibake: bool,                            // LF_PISTE_MOJIBAKE_REPAIR : true
}
impl PisteEndpoints {
    pub fn official(env: PisteEnvironment) -> Self;
    pub fn with_overrides(env: PisteEnvironment, oauth: Option<&str>, base: Option<&str>)
        -> Result<(Self, Vec<ConfigWarning>), ConfigProblem>;
}
impl fmt::Debug for PisteConfig { /* client_secret affiché "[REDACTED]" par secrecy */ }
```

Règles de `with_overrides` (00 §1, AM-27), toutes testées (§13.2), communes à `LF_PISTE_OAUTH_URL`/`LF_PISTE_BASE_URL` et à leurs équivalents `FR_ELI_*` :

1. L'URL se parse avec `Url::parse`. Le schéma doit être `https`. Exception : `http` est accepté si l'hôte est une boucle locale (`127.0.0.1`, `::1`, `localhost`), avec `ConfigWarning::InsecureLoopback`. Cette exception sert aux tests wiremock et aux passerelles locales.
2. Ni `userinfo`, ni requête (`?…`), ni fragment : sinon `ConfigProblem`. Une URL ne contient jamais de secret, ce qui corrige le risque de fuite par les messages d'erreur (D-40).
3. Hôte hors de la liste des quatre hôtes officiels (`sandbox-oauth.piste.gouv.fr`, `oauth.piste.gouv.fr` pour le jeton ; `sandbox-api.piste.gouv.fr`, `api.piste.gouv.fr` pour l'API) : accepté, car la surcharge est un choix explicite et local de l'utilisateur, mais `ConfigWarning::CustomHost` est affiché au démarrage, par `check-config` et dans `fr_coverage` (rendu dans 05). `official = false`.
4. Tout hôte officiel doit désigner l'environnement de `LF_PISTE_ENV`, explicite ou par défaut (`sandbox`) ; si les deux hôtes sont officiels, ils désignent donc le **même** environnement. Sinon `ConfigProblem::EnvMismatch`, dont le message demande de poser `LF_PISTE_ENV=production` (ou de corriger l'URL) : aucune déduction implicite de l'environnement à partir de l'hôte. Cela empêche d'envoyer des identifiants sandbox à la production, ou l'inverse, et de fausser l'espace de noms du cache et `fr_coverage`. Piège visé : un utilisateur de la référence qui pointait `FR_ELI_*_URL` vers la production sans définir `LF_PISTE_ENV`.
5. Sur un hôte API officiel, le chemin doit être exactement `/dila/legifrance/lf-engine-app`.

L'URL de base ne change plus après le démarrage. Aucune URL renvoyée par l'API (`lienJo`, `urlCC`, `attachementUrl`, `href` des contenus KALI) n'est jamais appelée (§14.3).

### 2.4 Démarrage sans réseau ; identifiants absents

- `PisteCorpus::new` ne fait **aucun** appel réseau. Le jeton est demandé paresseusement, au premier appel d'outil (00 §6).
- Identifiants absents, `ConfigProblem` (dont `EnvMismatch`) ou `LF_PISTE_ENV` invalide : le serveur démarre quand même, en **mode dégradé** (00 §1) : `lf-app` journalise le problème en `error!`, chaque méthode du port renvoie immédiatement `CorpusError::Config`, sans E/S ni attente (corrige D-39), et chaque outil PISTE répond `[config_error] …`. `fr_coverage` et `check-config` décrivent le problème ; l'avertissement `CustomHost` apparaît aussi comme lacune dans `fr_coverage` (00 §7 ; rendu dans 05). Pour les identifiants absents, le message garde celui de la référence avec les nouveaux noms de variables (P-014, E-34) : `PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials).` (gabarit de 05 §7.2).
- `check-config` (sous-commande de `lf-app`, 02 §8.1, 08) affiche l'environnement, les URLs effectives, les avertissements et les problèmes, sans le secret. L'option explicite `--online` appelle `check_credentials()` (02 §3.3) : une seule requête token, aucun appel à l'API.

---

## 3. OAuth2 client_credentials

### 3.1 Requête

`POST {oauth_url}`, `Content-Type: application/x-www-form-urlencoded` (méthode `RequestBuilder::form`, feature `form` de reqwest 0.13, recherche 04 §4.2) :

```text
grant_type=client_credentials&client_id=<id>&client_secret=<secret>&scope=openid
```

Corps et en-têtes conformes à l'exemple officiel (`exemples-d-utilisation-de-l-api.md:115-129`) et à la référence (client.py:77-86), donc à la parité P-022. En-têtes : `User-Agent` et `Accept: application/json`, communs au client.

### 3.2 Réponse

Réponse documentée : `{"access_token": "…", "token_type": "Bearer", "expires_in": 3600, "scope": "openid"}`, où `expires_in` est en secondes (`exemples-d-utilisation-de-l-api.md:135-151`). Observé en sandbox : scope `openid resource.READ` (référence DISCOVERY.md:12).

```rust
// dto/oauth.rs — non compilé, à valider au jalon qui l'implémente
#[derive(Deserialize)]
pub(crate) struct TokenResponseDto {
    #[serde(default)] pub access_token: Option<SecretString>,   // feature serde de secrecy
    #[serde(default)] pub token_type: Option<String>,
    #[serde(default, deserialize_with = "de::opt_u64_lenient")] pub expires_in: Option<u64>, // nombre, chaîne ou null
}
#[derive(Deserialize)]
pub(crate) struct OAuthErrorDto { #[serde(default)] pub error: Option<String>, #[serde(default)] pub error_description: Option<String> }
```

Règles :

- `access_token` absent ou vide ⇒ `CorpusError::Config` (`PISTE OAuth returned no access_token.`, parité S7). Une réponse 200 sans jeton indique un endpoint qui n'est pas celui de PISTE.
- `token_type` différent de `Bearer` (sans tenir compte de la casse) : `warn`, jeton utilisé quand même.
- `expires_in` absent, `null`, non numérique ou nul ⇒ **3 600 s**. La référence plantait sur `null` (recherche 02 D-10). La valeur est bornée à [1 s, 86 400 s].
- Formes réelles des erreurs de l'endpoint token, statut et corps : **NON VÉRIFIÉ** [V-60 · oauth-reponses].

### 3.3 Gestionnaire de jeton

```rust
// auth.rs — non compilé, à valider au jalon qui l'implémente
pub(crate) struct TokenManager<K: MonoClock> {
    http: reqwest::Client,                       // le client partagé (§4.2)
    oauth_url: Url,
    creds: PisteCredentials,
    state: tokio::sync::Mutex<Option<CachedToken>>,
    next_generation: AtomicU64,
    clock: K,
}
struct CachedToken { value: SecretString, refresh_at: Duration /* horloge MonoClock */, generation: u64 }
pub(crate) struct TokenLease { pub value: SecretString, pub generation: u64 }

impl<K: MonoClock> TokenManager<K> {
    /// Mono-vol : le verrou est tenu pendant le rafraîchissement ; les appelants concurrents
    /// attendent puis lisent le jeton neuf. L'attente du verrou est bornée par `deadline`
    /// (CallDeadline : échéance MonoClock dérivée de lf_application::deadline::Deadline, §5.3).
    pub async fn token(&self, deadline: &CallDeadline) -> Result<TokenLease, CorpusError>;
    /// N'invalide que si le jeton courant est celui qui a reçu le 401.
    pub async fn invalidate(&self, generation: u64);
}
```

- **État** : au plus un jeton en mémoire, jamais écrit sur disque ni journalisé (P-026 ; référence client.py:10-13). Il n'est pas non plus dans `lf-adapter-cache`.
- **Renouvellement anticipé** : `refresh_at = obtenu_à + expires_in − marge`, avec `marge = min(60 s, expires_in / 2)`. `obtenu_à` est lu **avant** l'envoi de la requête, comme la référence (client.py:74, 93), ce qui reste prudent. La marge de 60 s est celle de la référence et de deux clients tiers (recherche 05 §3.1). Le plafond `expires_in / 2` évite un jeton déjà périmé si `expires_in ≤ 60`.
- **Mono-vol** : un `tokio::sync::Mutex` couvre la lecture et le rafraîchissement (même principe que l'`anyio.Lock` de la référence, client.py:70-73). Dix appels simultanés au démarrage produisent **une** requête token (P-022, test `concurrent_calls_share_one_token_refresh`).
- **Générations** : chaque jeton obtenu reçoit un numéro croissant. `invalidate(g)` ne vide l'état que si le jeton courant porte `g`. Une tâche qui reçoit un 401 avec un vieux jeton n'efface donc pas le jeton neuf obtenu entre-temps par une autre.
- **Annulation** : si le futur appelant est abandonné (délai global, annulation MCP), la requête token en cours est abandonnée et le verrou est relâché sans modifier l'état. L'appelant suivant rafraîchit. L'acquisition du verrou de `tokio` est sûre vis-à-vis de l'annulation (test `cancel_during_refresh_releases_lock`).
- La requête token passe par la même politique de réessai que l'API (§5.2 : 429, 502, 503, 504 et réseau jusqu'à 3 tentatives, 500 jusqu'à 2), dans le même délai global. Elle ne passe pas par `governor`, car les quotas visent l'API Légifrance et le jeton n'est demandé qu'environ une fois par heure.

### 3.4 401 sur l'API : un seul renouvellement

Algorithme par appel du port, corrige D-12 et E-20 :

1. La tentative envoie la requête avec le jeton de génération `g`.
2. Réponse 401 et aucun renouvellement encore fait pour cet appel : `invalidate(g)`, `token()` (mono-vol), puis **renvoi immédiat**, sans backoff. Ce renvoi ne consomme pas l'une des 3 tentatives transitoires.
3. Second 401 dans le même appel : `CorpusError::Auth { detail }`, sans autre requête (`detail` pour les journaux : `API returned 401 twice on {gabarit d'endpoint} ({env})`). Le modèle reçoit le gabarit `Config` des identifiants refusés de 05 §7.2 (02 §9.2 ligne 11), qui renvoie à `LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET` et `LF_PISTE_ENV`.
4. La suite `401, 503, 200` produit exactement **deux** requêtes token et trois requêtes API (la référence en demandait trois, recherche 02 D-12).

Un rapport tiers dit que PISTE renverrait parfois `401 invalid_client` pendant de courtes fenêtres de limitation : **NON VÉRIFIÉ** [V-64 · 401-sous-throttling]. Si c'est vrai, l'étape 3 produirait un `config_error` à tort. Le smoke test live (§13.5) cherche ce cas. La règle « un seul renouvellement » est maintenue (00 §8).

### 3.5 Classification des erreurs de l'endpoint token

Une erreur de l'endpoint token est un problème de configuration, jamais `NotFound` (00 §5, corrige D-10). `CorpusError::Auth` et `CorpusError::Config` sont rendus `config_error` côté MCP ; la correspondance `CorpusError` → `AppError` est celle de 02 §9.2 (source unique, lignes 7 à 10), et le texte vu par le modèle est le gabarit de 05 §7.2, rédigé par `lf-application::messages` (02 §9.3). La dernière colonne n'en est qu'un rappel ; le `detail` de l'erreur, lui, ne sert qu'aux journaux.

| Réponse de l'endpoint token | `CorpusError` | Réessai | Message vu par le modèle (rappel de 02 §9.2 ; 05 §7.2 fait foi) |
|---|---|---|---|
| 200, jeton présent | — | — | — |
| 200 sans `access_token`, ou JSON illisible | `Config` | non | `PISTE OAuth returned no access_token.` (S7) |
| 400 ou 401 (`invalid_client`, `invalid_grant`, `unauthorized_client`…) | `Auth` | non | `PISTE rejected the credentials (HTTP {status}): check LF_PISTE_CLIENT_ID, LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV.` ; `detail` (journaux) : `{error}` et l'environnement, pour signaler un mélange sandbox/production |
| 403 | `Auth` | non | même gabarit que la ligne précédente |
| 404, 405, page HTML | `Config` | non | `PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.` (02 §9.2 ligne 8) ; `detail` : `scheme://host/path` sans requête |
| 429 | réessai, puis `RateLimited` | oui (§5.2) | gabarit `rate_limited` |
| 502, 503, 504, erreur réseau | réessai (3 tentatives au plus), puis `Upstream` ou `Timeout` | oui | gabarit `upstream_error` |
| 500 | réessai (2 tentatives au plus), puis `Upstream { status: 500, retryable: false }` | oui, une fois | gabarit `upstream_error`, sans invitation à réessayer |
| erreur DNS ou TLS (certificat refusé) | `Upstream { status: None, retryable: false }` | réseau : oui ; TLS : non | gabarit `upstream_error` ; `detail` : `could not reach PISTE OAuth ({kind})` |

Le champ `error` d'`OAuthErrorDto` est repris dans le `detail` s'il est court (≤ 64 caractères ASCII), sinon omis. `error_description` n'est jamais recopié : il pourrait contenir un fragment de la requête. Le statut exact renvoyé pour `invalid_client` (400 ou 401 : la RFC 6749 §5.2 admet les deux ; la référence ne relève que la valeur `invalid_client`, SOURCES.md:12) est **NON VÉRIFIÉ** [V-60 · oauth-reponses] : les deux statuts suivent la même ligne.

### 3.6 Secrets

- `client_secret` et `access_token` sont des `SecretString` (secrecy 0.10.3 : `Debug` affiche `[REDACTED]`, zéroïsation à la libération ; recherche 04 §4.1).
- `expose_secret()` n'est appelé qu'à deux endroits, revus et testés : la construction du formulaire OAuth et l'en-tête `Authorization`.
- L'en-tête est posé avec `RequestBuilder::bearer_auth`, qui le marque **sensible** (`header_sensitive(…, true)`, reqwest `src/async_impl/request.rs:273-279`). `http` ne l'affiche donc pas en `Debug`. La chaîne temporaire `Bearer …` construite par reqwest n'est pas zéroïsée : risque accepté, documenté.
- Aucun corps de requête OAuth n'est journalisé, à aucun niveau. `connection_verbose(false)` (défaut) est gardé.
- Un test injecte un secret et un jeton aléatoires, collecte tous les journaux `tracing`, tous les `Display` et `Debug` des erreurs, et vérifie qu'ils n'y figurent pas (`error_messages_never_contain_secret_or_token`, corrige D-40).

---

## 4. Client HTTP

### 4.1 reqwest 0.13 et TLS

Constats sur reqwest 0.13.5 (recherche 04 §4.2, `RQ:Cargo.toml:39-80`, `RQ:CHANGELOG.md:39-52`) :

- `rustls` est le backend TLS par défaut. Le fournisseur crypto par défaut est **aws-lc** (`rustls = ["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]`). `rustls-no-provider` laisse le choix du fournisseur.
- `form` et `query` ne sont plus activées par défaut. `form` est indispensable pour l'OAuth (§3.1).
- Les racines de certificats viennent de `rustls-platform-verifier`, c'est-à-dire du magasin du système. Sous Unix hors Apple, il faut donc un magasin CA dans l'image OCI (08).
- Avec `rustls-no-provider`, reqwest panique au premier client construit si aucun fournisseur n'est installé (`RQ:src/async_impl/client.rs:2498-2510`).

**Spikes TLS et proxy** (tâche 0 de M2, 00 §11.1), à trancher avant d'écrire le client :

| Option | Features | Coût | À mesurer |
|---|---|---|---|
| A (défaut si le spike n'aboutit pas) | `reqwest/rustls` (aws-lc) | `aws-lc-sys` demande une chaîne C (`cc`, `cmake`, parfois NASM ou clang selon la cible) | compilation croisée des 7 cibles cargo-dist (Linux x86_64/aarch64 gnu+musl, macOS x86_64/aarch64, Windows x86_64 msvc), durée, taille |
| B | `reqwest/rustls-no-provider` + `rustls/ring` ; `PisteCorpus::new` appelle `rustls::crypto::ring::default_provider().install_default()` et ignore l'erreur « déjà installé » | moins d'outillage natif | mêmes mesures ; poignée de main TLS 1.2 et 1.3 vers un serveur rustls local, et vers `sandbox-api.piste.gouv.fr` si le réseau le permet (Q6) |

Les exigences exactes d'`aws-lc-sys` par cible et l'API d'installation de `ring` avec reqwest 0.13 sont **NON VÉRIFIÉ** [V-02 · tls-fournisseur-cross] (recherche 04 §6 points 5 et 6). Le résultat est consigné dans ADR-0013 et dans le risque TLS de [09-risques-et-questions.md](09-risques-et-questions.md). Le fournisseur est installé **par l'adapter**, seul crate à dépendre de reqwest : `lf-app` n'a pas à dépendre de rustls.

### 4.2 Construction du client

Un seul `reqwest::Client`, construit une fois au premier appel (`OnceCell`, mono-vol ; `PisteCorpus::new` ne fait aucune E/S, 02 §3.3) et partagé par `TokenManager` et les appels API (pool de connexions par hôte). Un échec de construction (fournisseur TLS, racines) devient `CorpusError::Config`. Cela corrige D-06 et E-22 : la référence recréait un client par appel d'outil.

| Réglage | Valeur | Source / raison |
|---|---|---|
| `connect_timeout` | 10 s | 00 §8 ; parité P-025 (référence client.py:30) |
| `timeout` (total par tentative, connexion et corps compris) | `LF_HTTP_TIMEOUT_S` (40 s), réduit tentative par tentative à `min(40 s, reste du délai global)` via `RequestBuilder::timeout` | reqwest : « applied from when the request starts connecting until the response body has finished » (`RQ:src/async_impl/client.rs:1444-1453`, `request.rs:289-299`) |
| `user_agent` | `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-legifrance)`, chaîne fournie par `lf-app` avec `env!("CARGO_PKG_VERSION")` du binaire | 00 §8 ; corrige D-43 et E-08. Le dépôt porte ce nom depuis son renommage décidé le 2026-10-16 (Q1, 00 §0, C6) ; l'URL du dépôt GitHub suit le renommage |
| en-tête par défaut | `Accept: application/json` | exemple officiel (`exemples…md:155-163`) ; `Content-Type: application/json` est posé par `.json()` |
| `redirect` | `redirect::Policy::none()` | un 3xx devient `Upstream { status, retryable: false }`. Parité : la référence ne suivait pas les redirections (recherche 01 §6.3). Un `POST` authentifié n'a aucune raison d'être redirigé. reqwest suit 10 redirections par défaut (`RQ:src/redirect.rs:160-165`) |
| `https_only` | `true`, sauf `ConfigWarning::InsecureLoopback` (tests) | `RQ:src/async_impl/client.rs:2262` |
| `gzip` | activé (feature `gzip`) : envoie `Accept-Encoding: gzip`, décompression transparente | réponses JSON très compressibles ; prise en charge par PISTE **NON VÉRIFIÉ** [V-70 · piste-gzip]. La borne de taille s'applique aux octets **décompressés** (§4.3) |
| `retry` | défaut de reqwest conservé : seuls les NACK de protocole sûrs sont rejoués (`RQ:src/async_impl/client.rs:1405-1414`, `src/retry.rs:9-16`). Toute la politique applicative est dans `backon` (§5.2) | évite deux couches de réessai concurrentes |
| proxy | défaut conservé (proxy système, feature `system-proxy` : réglages Windows et macOS, `RQ:src/lib.rs:216`) | réseaux d'entreprise ; prise en compte de `HTTPS_PROXY` et `NO_PROXY` selon les features retenues : **NON VÉRIFIÉ** [V-03 · reqwest-proxy-env], à tester au spike TLS (tâche 0 de M2) |
| `pool_idle_timeout` | défaut (90 s) | une connexion fermée par la passerelle PISTE produit une erreur réseau, rejouée par la politique §5.2 |
| `danger_accept_invalid_certs` | **jamais** | §14.2 |

### 4.3 Lecture bornée du corps

Corrige D-41 et D-11.

1. Si `Response::content_length()` est connu et dépasse `max_body_bytes` (16 Mio), refus immédiat.
2. Sinon, lecture par `Response::chunk()` (`RQ:src/async_impl/response.rs:310`) dans un `Vec<u8>`, avec contrôle du cumul à chaque morceau. Les morceaux sont décompressés, si bien qu'une bombe gzip est arrêtée à 16 Mio.
3. Dépassement : `CorpusError::Decode { detail }`, avec un `detail` de la forme `response too large (> 16 MiB) on /consult/acco`. Pas de réessai.

Choix de 16 Mio : la plus grosse fixture fait 42 Ko (search_kali.json, recherche 03 §2), mais une réponse `/consult/lawDecree` sur un gros texte ou `/consult/acco` avec son champ `data` (`.docx` en base64, jamais désérialisé mais tout de même lu) peut être bien plus volumineuse. Tailles réelles **NON VÉRIFIÉ** [V-75 · taille-max-textes] ; présence et taille de `data` et `attachment` sur `/consult/acco` **NON VÉRIFIÉ** [V-106 · acco-data-attachment]. La borne est une constante (00 §8), réexaminée en M5 sur les fixtures recapturées (V-75) ; toute modification amende 00 §8. La mesure de `lawDecree` sur un code (V-96) a lieu dès M2, car elle décide de l'usage de `tableMatieres` en v0.1 (§7.3).

### 4.4 Décodage

1. `std::str::from_utf8(&bytes)` **strict**. Échec : `CorpusError::Decode { detail }` (`invalid UTF-8 at byte N on /consult/juri`). JSON impose UTF-8 et l'en-tête `charset` n'est pas pris en compte (feature `charset` exclue, §1.4).
2. Réparation du mojibake sur le texte entier si `repair_mojibake` (§10).
3. `serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_str(&text))` vers le DTO de l'endpoint (§8.5).
4. Mapping vers l'entité du domaine (§9).

Le corps des **réponses d'erreur** suit les étapes 1 et 2, puis une désérialisation optionnelle en `PisteErrorDto` (§11.3), pour que le marqueur « introuvable » soit reconnu même dans un corps en mojibake.

---

## 5. Résilience

### 5.1 Statut → action → erreur

Une **tentative** produit une issue. La boucle de réessai (§5.2) décide de la suite.

| Issue d'une tentative | Réessai (≤ 3 tentatives ; ≤ 2 sur un 500) | Résultat final | Source |
|---|---|---|---|
| 2xx avec corps | — | DTO, puis entité ; corps vide : `NotFound` sur une consultation, `Decode` sur `/search` | Swagger : 200 et 201 déclarés pour chaque opération (recherche 05 §1.1) |
| 3xx | non | `Upstream { status, retryable: false }` | §4.2 |
| 400 sur une consultation avec le marqueur « introuvable » | non | `NotFound` | §11 |
| 400, autres cas | non | `InvalidRequest { status: Some(400), detail }` (message PISTE nettoyé, ≤ 200 caractères) et `warn!` « bug probable » ; rendu `[upstream_error] Legifrance rejected the request (HTTP 400): {détail}` (00 §5, parité S10) | 00 §5 ; 02 §9.2 ligne 15 |
| 401 (API) | renouvellement unique (§3.4), hors budget | second 401 : `Auth` | 00 §8 |
| 403 | non | `Auth`, rendu `config_error` avec le message « check that the PISTE application is subscribed to the Legifrance API » (00 §5) | recherche 05 §3.4 (constaté sur Judilibre avec un jeton Légifrance ; sens exact d'un 403 de l'API Légifrance **NON VÉRIFIÉ** [V-65 · forme-403-piste], V-65) |
| 404 | non | `NotFound` ou `Config` selon §11.2 | D-10 |
| 405 | non | `Config` | — |
| autre 4xx | non | `Upstream { status, retryable: false }` | — |
| 429 | **oui**, avec `Retry-After` | `RateLimited { retry_after }` | 00 §8 |
| 500 | **oui**, 2 tentatives au plus (1 réessai) | `Upstream { status: 500, retryable: false }` : un 500 répété est présumé déterministe (facette ou champ refusé) ; retour à 3 tentatives si V-69 montre des 500 transitoires fréquents | 00 §8 ; 02 §9.2 ligne 17 ; [V-69 · 500-requete-invalide] |
| 502, 503, 504 | **oui**, 3 tentatives au plus, `Retry-After` respecté s'il est présent | `Upstream { status, retryable: true }` | 00 §8 |
| autre 5xx | non | `Upstream { status, retryable: false }` | — |
| erreur réseau (connexion, réinitialisation, corps interrompu) | **oui** | `Upstream { status: None, retryable: true }` | 00 §8 |
| délai de tentative dépassé (`reqwest::Error::is_timeout`) | **oui** | `Timeout` | — |
| reste du délai global insuffisant pour une nouvelle tentative ou attente | non | `RateLimited { retry_after }` (429 ou limiteur), sinon dernière erreur, sinon `Timeout` | 00 §3 ; §5.3 |
| corps illisible (UTF-8, JSON, trop gros) | non | `Decode` | §4.3, §8.5 |
| incohérence de domaine (`DomainError` non utilisateur, ex. `TocTooDeep`, variante ajoutée en M2 avec la TOC en arbre) | non | `Decode` | 03 §9 |

`Upstream.status` est un `Option<u16>` (`None` pour une erreur réseau). Le `detail` de chaque `CorpusError` est en anglais, court, sans URL complète ni secret, par exemple `HTTP 503 on /consult/juri after 3 attempts` ; il sert aux journaux (stderr). Le message vu par le modèle (et inscrit dans le champ `error` de l'audit) est le gabarit de 05 §7.2 (avec, pour `upstream_error`, le préfixe de parité `Legifrance/PISTE HTTP {status}` de P-014), rédigé par `lf-application::messages` à partir de la variante et de ses champs (02 §9.3). `retryable` ne sert qu'au message : seul un échec `retryable: true` (429, 502, 503, 504, réseau, timeout) invite à réessayer (00 §8). `Timeout`, `Decode`, le délai global et l'annulation sont rendus `upstream_error`, la cause étant détaillée dans les journaux et l'audit (`UpstreamCause`, 00 §5). Les erreurs reqwest passent par `Error::without_url()` (`RQ:src/error.rs:91`) avant tout affichage, et seul le chemin du gabarit d'endpoint est cité. Les noms et champs exacts des variantes sont fixés dans [02-architecture.md](02-architecture.md) §5.1.

Selon des clients tiers, une facette inconnue, un champ JSON inconnu ou `fond=ALL` avec filtres provoquent un **500** plutôt qu'un 400 : **NON VÉRIFIÉ** [V-69 · 500-requete-invalide] (recherche 05 §2.2). L'adapter n'envoie que des champs et facettes issus de tables vérifiées (§6), ce qui limite ce risque. C'est la raison du budget réduit sur 500 (00 §8, AM-03) : un 500 déterministe coûterait 3 requêtes de quota par appel, puis autant à chaque relance du modèle s'il était invité à réessayer.

### 5.2 Politique de réessai

| Paramètre | Valeur | Raison |
|---|---|---|
| Tentatives transitoires (429, 502, 503, 504, réseau, délai de tentative) | 3 au plus : 1 + 2 réessais | 00 §8 ; parité P-024 |
| Tentatives sur 500 | 2 au plus : 1 + 1 réessai, puis `Upstream { status: 500, retryable: false }`. Dans un même appel, la boucle s'arrête après 3 tentatives au total, ou dès la deuxième réponse 500 | 00 §8 (AM-03) ; un 500 répété est présumé déterministe (V-69) |
| Attente avant la 2e tentative | 500 ms + gigue uniforme dans [0, 500 ms) | P-024 exige au moins 0,5 s puis au moins 1 s ; la gigue s'ajoute et ne retranche rien |
| Attente avant la 3e tentative | 1 s + gigue dans [0, 1 s) | idem |
| `Retry-After` (429, 503) | en secondes entières ; ou en date HTTP (IMF-fixdate, parsée par `chrono`), convertie en délai par différence avec l'en-tête `Date` de la **même** réponse, sans horloge murale (l'adapter n'a que `MonoClock` ; `Utc::now` est interdit hors de `lf-app::clock`, 02 §2.2) ; date HTTP sans `Date` exploitable, ou différence négative : en-tête ignoré (attente = backoff) ; attente = `max(backoff, Retry-After)`, plafonnée à 30 s | corrige D-12 ; RFC 9110 §10.2.3 (deux formes) et §6.6.1 (`Date` obligatoire dans les réponses 4xx d'un serveur d'origine doté d'une horloge, facultatif en 5xx) ; présence et format réels de `Retry-After` (et de `Date`) chez PISTE **NON VÉRIFIÉ** [V-63 · retry-after-piste] |
| Attente prévue supérieure au reste du délai global | pas d'attente : échec immédiat, `RateLimited { retry_after }` (429) ou dernière erreur (5xx) | une réponse rapide et actionnable plutôt qu'un dépassement certain (00 §3) |
| Budget 401 | 1 renouvellement par appel du port, séparé du budget transitoire | §3.4 |
| Erreurs mises en cache | jamais | parité (recherche 01 §6.4) ; c'est au cache de ne pas les stocker (§12.2) |

Mise en œuvre avec `backon` 1.6.0 (`default = std, std-blocking-sleep, tokio-sleep, gloo-timers-sleep`, index crates.io ; seule `std` est activée, §1.4) : un `ExponentialBuilder` (délai minimal 500 ms, facteur 2, 2 réessais, gigue), un prédicat `when` qui ne retient que les issues transitoires et applique le budget propre au 500, un ajustement du délai par `Retry-After`, et un « sleeper » qui délègue au `Sleeper` du crate, pour les tests. Les noms exacts de l'API backon 1.6 (`when`, `adjust`, `sleep`, `notify`) et la sémantique de sa gigue sont **NON VÉRIFIÉ** [V-06 · backon-api]. Si `adjust` n'existe pas ou si la gigue de backon peut réduire le délai, on écrit un itérateur `Backoff` maison (backon accepte tout `Iterator<Item = Duration>`), sans changer la politique ci-dessus.

```rust
// retry.rs — esquisse non compilée, à valider au jalon qui l'implémente (la logique compte, pas l'API backon exacte)
async fn call<D: DeserializeOwned>(&self, ep: Endpoint, body: &impl Serialize) -> Result<D, CorpusError> {
    // durée restante publiée par Audited (lf_application::deadline::Deadline), convertie dans MonoClock (§5.3)
    let deadline = CallDeadline::start(&self.clock, Deadline::current_or(self.cfg.tool_deadline));
    let mut refreshed = false;
    let mut plan = BackoffPlan::new(&deadline);                 // 500 ms, 1 s (+ gigue) ; 2 réessais, 1 seul après un 500
    loop {
        let permit = self.gate.acquire(&deadline).await?;      // sémaphore + governor (§5.4)
        let lease = self.tokens.token(&deadline).await?;       // mono-vol (§3.3)
        let outcome = self.attempt(ep, body, &lease, &deadline).await; // §5.1
        drop(permit);
        match outcome {
            Outcome::Ok(bytes) => return decode_and_parse::<D>(ep, bytes, &self.cfg),
            Outcome::Unauthorized if !refreshed => { refreshed = true; self.tokens.invalidate(lease.generation).await; continue; }
            Outcome::Unauthorized => return Err(CorpusError::auth_twice(ep)),
            Outcome::Transient { err, retry_after } => match plan.next(&err, retry_after) { // budget 500 compris
                Some(wait) => self.sleeper.sleep(wait).await,   // attente comprise dans le délai global
                None => return Err(err.finalize(retry_after)),  // budget épuisé ou attente > reste du délai
            },
            Outcome::Fatal(err) => return Err(err),
        }
    }
}
```

### 5.3 Délai global par outil

`LF_TOOL_DEADLINE_S` (60 s) borne **tout** le travail réseau d'un appel d'outil : attente du sémaphore et du limiteur, jeton, tentatives et attentes de backoff. Cela corrige D-14 : le pire cas de la référence dépassait 200 s (recherche 02 D-14).

Décision (00 §3, AM-07) ; les signatures du port n'ont pas de paramètre de délai :

- **Garde dure** : `Audited` (`lf-application`) exécute chaque cas d'usage sous `tokio::time::timeout(LF_TOOL_DEADLINE_S)`, seule garde dure, et publie l'échéance dans `lf_application::deadline::Deadline`, variable locale de tâche (`tokio::task_local!` ; feature `rt` de tokio admise dans `lf-application` pour cet usage seulement, 00 §2, 02 §3.2). Un dépassement est rendu `upstream_error` (`UpstreamCause::Deadline`) par `Audited`.
- **Lecture par l'adapter** : à l'entrée de chaque méthode du port, l'adapter lit `Deadline::current_or(tool_deadline)` comme une **durée restante** (`Duration`) et la convertit aussitôt en échéance dans sa propre horloge monotone : `CallDeadline { at: clock.now() + reste }` (module `deadline.rs`). Aucun instant absolu ne passe d'une horloge à l'autre : les horloges de test de l'adapter (`ManualClock`, §13.1) restent cohérentes. Sans `Deadline` publié (tests, appel direct), le reste vaut `tool_deadline`.
- Les sous-futurs d'un même cas d'usage (`fr_search_all`, `fr_verify_citations`) sont interrogés dans la même tâche (`join_all`, `buffer_unordered`). Ils voient donc la même échéance. Une tâche lancée par `tokio::spawn` ne la verrait pas : les cas d'usage n'en lancent pas (sinon, propagation explicite du `Deadline`). Les cas d'usage à appels multiples cessent d'en lancer à 80 % du délai et rendent un résultat partiel motivé (00 §3) ; ce n'est pas le rôle de l'adapter.
- Chaque tentative reçoit `RequestBuilder::timeout(min(request_timeout, reste))`. Chaque attente (`Sleeper`, sémaphore, limiteur, verrou du jeton) est bornée par `reste`.
- L'adapter ne programme aucune tentative ni attente qui dépasserait l'échéance : il rend tout de suite une erreur explicite (`RateLimited { retry_after }`, `Timeout` ou dernière erreur), ce qui permet au modèle de recevoir une erreur précise plutôt que la coupure de la garde dure.
- Pire cas d'un appel du port : `≤ tool_deadline + ε`, où ε est le temps d'observer l'annulation (quelques millisecondes).
- Délais imposés par les clients MCP (Claude Code, VS Code) à un appel d'outil : **NON VÉRIFIÉ** [V-55 · delai-clients-mcp]. Si l'un coupe avant 60 s, le défaut doit baisser.

### 5.4 Limitation de débit

- Un `governor::RateLimiter` **direct** (non indexé), global au processus, **5 requêtes/s, rafale 10** par défaut (00 §8), configurable par `LF_PISTE_RPS` et `LF_PISTE_BURST` (00 §1, §2.2). Les quotas PISTE valent pour toute l'application consommatrice et sont donc partagés par tous les utilisateurs d'un éventuel serveur HTTP (recherche 05 §3.3, §4.3, Q4). Valeurs réelles **NON VÉRIFIÉ** [V-62 · quota-piste-prod] : l'utilisateur les lit dans la console PISTE et ajuste `LF_PISTE_RPS`/`LF_PISTE_BURST`.
- Le limiteur est consulté **avant chaque tentative**, réessais compris. Il ne l'est pas pour la requête token (§3.3).
- **Piège vérifié** : `RateLimiter::until_ready` attend avec `futures_timer::Delay` (governor 0.10.4, `src/state/direct/future.rs:11`, `:45-56`), pas avec `tokio::time`. Il échappe donc à toute horloge de test et ne se borne pas proprement par le délai global. L'adapter **n'utilise pas** `until_ready`. Il boucle sur `check()` (`src/state/direct.rs:73`) : en cas de refus, `wait = not_until.wait_time_from(clock.now())` ; si `wait` dépasse le reste du délai, échec immédiat `RateLimited { retry_after: Some(wait) }` ; sinon `Sleeper::sleep(wait)`.
- Horloge du limiteur : une implémentation de `governor::clock::Clock` dont l'`Instant` est un `Duration` écoulé depuis une origine `MonoClock`. C'est la seule horloge du limiteur, en production comme en test (02 §3.3, piège 3 : ni `FakeRelativeClock`, ni constructeur `with_clock`, ni horloge tokio gelée). `Duration` implémente déjà `Reference` et `Add<Nanos>` (`src/clock.rs:61-117`). En test, `ManualClock` rend le limiteur déterministe. Les features `quanta`, `jitter` et `dashmap` sont inutiles (seule `std` est activée, §1.4). Les noms de constructeurs (`Quota::per_second`, `allow_burst`, `RateLimiter::direct_with_clock`) sont à confirmer à la compilation : **NON VÉRIFIÉ** [V-07 · governor-api].

### 5.5 Concurrence et `fr_search_all`

- Un `tokio::sync::Semaphore` de **4** permis par défaut (`LF_PISTE_CONCURRENCY`, 1..=16, 00 §1) borne les requêtes API en vol pour tout le processus. Le permis est pris avant le limiteur et rendu après la lecture du corps. Valeur reprise de la pratique tierce de 4 requêtes simultanées (recherche 05 §3.3).
- `fr_search_all` (M7, 00 §7) : le cas d'usage lance `|fonds|` recherches (5 par défaut) dans la même tâche. L'adapter garantit au plus 4 requêtes en vol et le débit `governor`. Avec la rafale de 10, 5 recherches partent sans attente sous le plafond de concurrence. Le cas d'usage ne crée pas de tâche (§5.3). La sémantique d'un échec partiel (un fond en erreur) relève de [05-outils-mcp.md](05-outils-mcp.md).
- 00 retient des requêtes parallèles par fond, et non un appel unique `fond=ALL` (recherche 05 §1.13 évoquait cette alternative). Rien de `fond=ALL` n'est implémenté.

### 5.6 Bornes de temps

| Étape | Borne |
|---|---|
| attente du sémaphore | reste du délai |
| attente du limiteur | reste du délai ; échec immédiat si l'attente prévue la dépasse |
| jeton (au plus ~1 fois par heure) | 1 requête, ≤ `min(40 s, reste)`, réessais compris dans le délai |
| tentative API | ≤ `min(40 s, reste)` |
| backoff | ≈ 0,5–1 s puis 1–2 s, ou `Retry-After` ≤ 30 s, toujours ≤ reste |
| **total** | **≤ `LF_TOOL_DEADLINE_S` (60 s) + ε** |

---

## 6. Construction des requêtes `/search`

### 6.1 Payload

Base : le payload de la référence (server.py:133-156, recherche 03 §3.1, recherche 01 §3.1). Exemple pour `fr_search(query="république numérique", fond=LODA, match=all, page=2, date_from=2016-01-01, date_to=2016-12-31, version_date=2026-10-15)` :

```json
{
  "fond": "LODA_DATE",
  "recherche": {
    "champs": [
      {
        "typeChamp": "ALL",
        "criteres": [
          { "typeRecherche": "TOUS_LES_MOTS_DANS_UN_CHAMP", "valeur": "république numérique", "operateur": "ET" }
        ],
        "operateur": "ET"
      }
    ],
    "filtres": [
      { "facette": "DATE_VERSION", "singleDate": 1792058400000 },
      { "facette": "DATE_SIGNATURE", "dates": { "start": "2016-01-01", "end": "2016-12-31" } }
    ],
    "pageNumber": 2,
    "pageSize": 10,
    "operateur": "ET",
    "sort": "PERTINENCE",
    "typePagination": "DEFAUT"
  }
}
```

| Champ | Valeur | Origine |
|---|---|---|
| `fond` | `Fond::api_name()` : `LODA_DATE`, `CODE_DATE`, `JURI`, `CONSTIT`, `CETAT`, `CNIL`, `KALI`, `ACCO` (`JORF` en M7) | 03 §3.1 ; référence server.py:91-100 |
| `champs[0].typeChamp` | `ALL` | 00 §8 ; parité |
| `criteres[0].typeRecherche` | selon `match` (§6.4) | 00 §8 |
| `criteres[0].valeur` | `QueryText` (déjà normalisée par le domaine, §6.8) | 03 §4.1 |
| `criteres[0].operateur`, `champs[0].operateur`, `recherche.operateur` | `ET` | parité ; requis par le Swagger (`CritereDTO`, `RechercheSpecifiqueDTO`) |
| `filtres` | toujours présent : `[]` si aucun filtre (parité server.py:149) ; ordre déterministe : `DATE_VERSION` d'abord, puis la facette de plage | tests de sérialisation, stabilité du cache |
| `pageNumber`, `pageSize` | `page` (≥ 1), `page_size` (1..=100) | 00 §7 ; Swagger : `pageSize` « (max=100) » |
| `sort` | `PERTINENCE` | 00 §8 ; seul tri attesté pour CODE (recherche 05 §2.2) |
| `typePagination` | §6.6 (`PaginationMemo` ; ici `DEFAUT`, page 2 d'une recherche LODA sans mémo) | Swagger `RechercheSpecifiqueDTO.typePagination` |
| `secondSort`, `fromAdvancedRecherche`, `proximite`, `multiValeurs`, `valeurs` | **jamais envoyés** en v0.1 | parité ; champs facultatifs dans le Swagger |

Les champs requis du Swagger sont tous présents : `fond` et `recherche` (`SearchRequestDTO`) ; `champs`, `operateur`, `pageNumber`, `pageSize`, `sort` et `typePagination` (`RechercheSpecifiqueDTO`) ; `operateur`, `typeRecherche` et `valeur` (`CritereDTO`).

Les DTO de requête de la recherche 03 §12.3 sont repris (`SearchRequestDto`, `RechercheDto`, `ChampDto`, `CritereDto`, `FiltreDto`, `DatePeriodDto`, enums en `SCREAMING_SNAKE_CASE`), avec `FiltreDto.single_date: Option<i64>` (epoch ms) et `dates: Option<DatePeriodDto { start: String, end: String }>`. Les variantes inutiles en v0.1 (`multi_valeurs`, `proximite`, sous-critères) ne sont pas déclarées.

### 6.2 Fond et `DATE_VERSION`

- `LODA_DATE` et `CODE_DATE` **exigent** un filtre `DATE_VERSION` en `singleDate` (référence DISCOVERY.md:22-25, constat live). L'adapter l'ajoute toujours pour ces deux fonds, à partir de `SearchQuery.version_date`. L'application la renseigne avec `Clock::today_paris()` si l'utilisateur ne l'a pas donnée (00 §7).
- Pour les autres fonds, `version_date` n'est jamais envoyé : il est rejeté en `invalid_arg` par le cas d'usage, sans réseau (00 §7 ; `SearchQuery::new`, 03 §4.1 ; message de 05 §7.3), donc `SearchQuery.version_date` vaut `None` hors LODA et CODE.
- `singleDate` est un **entier epoch ms**, forme validée en direct par la référence (server.py:136). La forme chaîne (`"2025-04-15"`, exemple officiel `exemples…md:815`) n'est pas utilisée.

### 6.3 `DATE_VERSION` à midi Europe/Paris

Règle (00 §8, AM-02) : `singleDate` = l'instant de **midi**, heure de Paris, au jour `version_date`, exprimé en millisecondes depuis l'epoch. Il est stable toute la journée : deux recherches identiques le même jour donnent le même corps (la clé de cache repose d'ailleurs sur `VersionDate`, pas sur l'instant). Cela corrige D-01 et E-06 : la référence envoyait `now_ms`, qui changeait à chaque milliseconde.

Pourquoi midi : tous les horodatages de date observés dans les réponses sont à minuit **UTC** (recherche 02 D-20, recherche 03 §11.2 ; V-100). PISTE peut comparer l'instant à des bornes stockées à J 00:00Z, le tronquer en date UTC, ou le tronquer en date de Paris. Minuit Paris (22 h ou 23 h UTC la veille) n'est correct que dans le troisième cas : dans les deux autres, la recherche du jour J retiendrait la version **de la veille**, et un texte entrant en vigueur le jour J n'apparaîtrait pas le jour J. Midi Paris (10 h ou 11 h UTC) est correct dans les trois cas, avec 10 à 11 heures de marge de chaque côté ; c'est aussi le régime le plus proche de celui que la référence a validé en direct (`now_ms`, envoyé à des heures de journée).

```rust
// request/search.rs — non compilé, à valider au jalon qui l'implémente
fn paris_noon_ms(d: VersionDate) -> i64 {                          // VersionDate = CivilDate (lf-domain, C3)
    // Aucune branche d'erreur : jours depuis l'epoch par arithmétique entière (algorithme days_from_civil
    // de Hinnant, total sur i64), puis décalage de Paris lu à midi UTC par une API chrono-tz totale.
    let utc_noon_ms = days_from_civil(d.year(), d.month(), d.day()) * 86_400_000 + 43_200_000;
    let utc = NaiveDateTime::UNIX_EPOCH + TimeDelta::milliseconds(utc_noon_ms); // année 1..=9999 (CivilDate) : pas de dépassement
    let offset_ms = i64::from(chrono_tz::Europe::Paris.offset_from_utc_datetime(&utc).fix().local_minus_utc()) * 1_000;
    utc_noon_ms - offset_ms                                       // midi local = midi UTC - décalage
}
```

Vecteurs de test (00 §8 ; calculés avec la base IANA, `zoneinfo` de Python 3.13, et à reproduire avec `chrono-tz`) :

| `version_date` | `singleDate` | UTC |
|---|---|---|
| 2026-10-15 | `1792058400000` | 2026-10-15T10:00Z (heure d'été) — même valeur que 03 §6.4 |
| 2026-01-15 | `1768474800000` | 2026-01-15T11:00Z (heure d'hiver) |
| 2026-03-29 (passage à l'heure d'été à 2 h) | `1774778400000` | 2026-03-29T10:00Z |
| 2026-10-25 (retour à l'heure d'hiver à 3 h) | `1792926000000` | 2026-10-25T11:00Z |
| 1804-03-21 (avant 1911 : temps moyen de Paris, UTC+0:09:21) | `-5231563761000` | 1804-03-21T11:50:39Z |

Les changements d'heure à Paris ont lieu à 2 h et à 3 h : midi n'est jamais ambigu ni inexistant, donc le décalage lu à midi UTC est celui de midi local. La fonction est totale sans branche de repli (`NaiveDate::from_ymd_opt`, `LocalResult` et leurs cas morts sont évités) : elle respecte les 100 % de régions de 00 §10 sans test artificiel ; `days_from_civil` est couverte par les vecteurs ci-dessous. La version de la base IANA embarquée par `chrono-tz` 0.10.4 est **NON VÉRIFIÉ** [V-17 · chrono-tz-iana] (V-17) : le test `paris_noon_vectors` la contrôle pour l'adapter dès M2.

**Vérification (bloquante en M2).** La sémantique réelle de `singleDate` chez PISTE est **NON VÉRIFIÉ** [V-76 · singledate-midi-paris] (V-76, R-17). Vérification live (§13.5) : chercher un texte dont une version commence à une date D connue, avec `version_date = D`, et comparer avec des instants à minuit UTC, à minuit Paris et à la veille. Le suffixe `_JJ-MM-AAAA` de `titles[0].id`, qui est la date de début de la version (03 §2.4), dit quelle version l'API a retenue. Midi reste appliqué jusqu'au résultat ; s'il se révélait faux, toute autre règle amende 00 §8.

### 6.4 `match` → `typeRecherche`

| `match` (00 §7) | `typeRecherche` | Remarque |
|---|---|---|
| `any` (défaut adopté jusqu'à V-79, puis `all` si V-79 le confirme, décision consignée avant le verrou des descripteurs de M4 : Q10) | `UN_DES_MOTS` | parité (server.py:144) ; « un des mots » = OU logique, d'où beaucoup de bruit (1 786 résultats LODA pour « republique numerique », recherche 02 D-32) |
| `all` | `TOUS_LES_MOTS_DANS_UN_CHAMP` | sémantique exacte avec `typeChamp=ALL` (tous les mots dans un **même** champ, ou dans l'un quelconque des champs ?) : **NON VÉRIFIÉ** [V-79 · mode-all-semantique] |
| `exact` | `EXACTE` | l'expression exacte (exemples officiels, `exemples…md:837-843`) |

Valeurs de l'enum vérifiées dans le Swagger 2.4.2 (`CritereDTO.typeRecherche`). `AUCUN_DES_MOTS` et `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION` ne sont pas exposés.

### 6.5 Filtres de date par fond

`date_from` et `date_to` (00 §7) sont envoyés sous la forme `{"facette": <F>, "dates": {"start": "AAAA-MM-JJ", "end": "AAAA-MM-JJ"}}`. Ce format est celui des exemples officiels : `DATE_SIGNATURE` sur `LODA_DATE` (`exemples…md:535-545`), `DATE_DECISION` sur `CETAT` (`:1011-1019`) et sur `JURI` (`:1089-1097`).

| Fond | Facette `F` retenue | Ligne du document officiel « description des tris et filtres » | Autres facettes de date du fond (non utilisées) |
|---|---|---|---|
| `Loda` (`LODA_DATE`) | `DATE_SIGNATURE` | `description…md:99` ; exemple `exemples…md:535` | `DATE_PUBLICATION` (`:100`), `DATE_VERSION` (`:106`, réservée à `version_date`) |
| `Code` (`CODE_DATE`) | **aucune** : seule `DATE_VERSION` existe (`:78`) | — | — |
| `Juri` | `DATE_DECISION` | `:210` ; exemple `exemples…md:1089` | `DATE_DECISION_ATTAQUEE` (`:206`) |
| `Constit` | `DATE_DECISION` | `:159` | `DATE_LOI` (`:154`) |
| `Cetat` | `DATE_DECISION` | `:178` ; exemple `exemples…md:1011` | `DATE_VERSEMENT` (`:179`) |
| `Cnil` | `DATE_DELIB` | `:301` (champ technique `timeInterval`) | — |
| `Kali` | `DATE_SIGNATURE` | `:249` | `DATE_PUBLICATION` (`:250`), `DATE_PUBLI_TEXTE_CITE` (`:253`) |
| `Acco` | `DATE_SIGNATURE` | `:277` | `DATE_DIFFUSION` (`:283`) |
| `Jorf` (M7) | `DATE_PUBLICATION` (à confirmer en M7) | `:52` | `DATE_SIGNATURE` (`:51`) |

Règles :

- La facette retenue représente « la date du document » pour un juriste : signature d'un acte, date d'une décision ou d'une délibération. La table vit dans l'adapter (`request/search.rs`, `fn date_facet(Fond) -> Option<&'static str>`), car ce sont des noms d'API (03 §3.1) ; le domaine sait seulement si le fond en a une : `date_facet(f).is_some()` vaut `f.has_date_filter()` pour chaque fond (03 §3.1), ce que contrôle `date_facet_table_matches_doc` (§6.10).
- `Code` avec une plage de dates : rejeté en `invalid_arg` par le cas d'usage, sans réseau (00 §7 ; `SearchQuery::new`, 03 §4.1 ; message de 05 §7.3). L'adapter n'en garde qu'une copie défensive (00 §5) : `date_facet(Fond::Code)` vaut `None`, et une `SearchQuery` qui porterait quand même une plage sur CODE donne `CorpusError::InvalidRequest { status: None, detail }` sans appel réseau, avec un `warn!` « bug probable » ; aucun statut HTTP n'est fabriqué (02 §5.1).
- Borne absente : `DateRange` est toujours une période fermée (03 §6.1). `DateRange::new` complète une borne absente par l'extrémité de la plage de `VersionDate` (`1500-01-01` ou `2221-12-31`) ; l'adapter envoie donc toujours les deux champs, tels que le domaine les fournit, sans valeur de remplissage propre, et la clé de cache reste unique. Tous les exemples officiels fournissent les deux bornes. **Tâche de tête de M2** (00 §11.1) : `DateRange` avec `InvertedDateRange` et `VersionDate` en newtype borné 1500-01-01..=2221-12-31 (bornes **NON VÉRIFIÉ**) ; le livré n'a aujourd'hui que `VersionDate = CivilDate`, alias non borné.
- Noms de facettes : vérifiés dans la documentation officielle. **Effet réel** en direct (le total diminue-t-il ?), caractère inclusif de `end`, comportement d'une borne lointaine, et combinaison avec `DATE_VERSION` : **NON VÉRIFIÉ** [V-78 · filtres-dates-effet]. Un exemple officiel combine déjà `DATE_SIGNATURE` et `DATE_VERSION` sur `LODA_DATE` (`exemples…md:535-553`).

### 6.6 Pagination et `typePagination`

- `typePagination` : `DEFAUT` ou `ARTICLE` (Swagger). « Lors de la navigation dans plusieurs pages, il est nécessaire de passer la valeur reçue dans la réponse précédente » (`RechercheSpecifiqueDTO.typePagination`, recherche 03 §3.2). Observé : `CODE_DATE` répond `ARTICLE` à une requête `DEFAUT` (search_code.json : `typePagination: "ARTICLE"`, `totalArticleResultNumber: 1`).
- La valeur reçue est lue dans `SearchPage.pagination` (`PaginationMode`, 03 §3.1, §4.1), qui alimente le mémo et les journaux. `fr_search` ne la rend **jamais** au modèle et n'a aucun paramètre pour la recevoir (00 §7, AM-04) : `typePagination` n'est pas exposé.
- **Règle (00 §7)** : `fr_search` est sans état, l'application aussi ; `SearchQuery` ne porte pas de mode de pagination (03 §4.1). L'adapter tient donc un `PaginationMemo` (§12.3) dont la clé est la requête normalisée **sans** `page` (`fond`, `mode`, `text`, `version_date`, `date_range`, `page_size`). Il conserve la dernière `typePagination` reçue (TTL 1 h, comme le cache de recherche ; 1 024 entrées au plus).
- Valeur envoyée, dans l'ordre :
  1. `ARTICLE` pour les recherches `NUM_ARTICLE` construites par l'adapter lui-même (`find_article`, `find_by_number`, §6.9) ;
  2. si `page > 1`, la valeur du mémo ;
  3. si `page > 1`, `fond = Code` et aucun mémo (redémarrage, autre processus) : `ARTICLE`, valeur observée pour ce fond ;
  4. sinon `DEFAUT`.
- Effet réel de `DEFAUT` contre `ARTICLE` en page 2 sur CODE, et cohérence de `has_more` (calculé sur `totalResultNumber`, 03 §4.1) quand la pagination porte sur les articles : **NON VÉRIFIÉ** [V-82 · typepagination-page2]. À vérifier en M5 (§13.5). Si le mémo ne suffit pas, un paramètre opaque `pagination` de `fr_search` est ajouté (classe « ajout », 00 §7).
- Valeur reçue inconnue (ni `DEFAUT` ni `ARTICLE`) : traitée comme `Defaut`, avec `warn`.

### 6.7 `pageSize` et pagination profonde

- `pageSize` ∈ 1..=100, garanti par `PageSize` (03 §1.4) ; maximum documenté à 100 (Swagger ; exemple officiel `pageSize: 100`, `exemples…md:1027`). `pageNumber` commence à 1 (exemples officiels ; référence server.py:150).
- Aucune limite de profondeur n'est documentée pour `/search`. Le moteur est vraisemblablement Elasticsearch, dont la fenêtre par défaut est de 10 000 résultats (recherche 05 §2.4) : **NON VÉRIFIÉ** [V-83 · pagination-profonde] (V-83).
- **Règle (00 §7, AM-04)** : `page × page_size > 10 000` est rejeté en `invalid_arg` par le cas d'usage, sans réseau (`SearchQuery::new`, constante `REACHABLE_RESULTS` de 03 §4.1, arithmétique `checked` ; message de 05 §7.3), au lieu de risquer un 500 opaque. L'adapter n'en garde qu'une copie défensive : `InvalidRequest { status: None, detail }` sans appel réseau, avec un `warn!` « bug probable » (00 §5). Le sondage live de M5 (`live_deep_pagination_limit`, V-83) confirme la fenêtre ou fait ajuster `REACHABLE_RESULTS`.

### 6.8 Assainissement de la requête

- La normalisation est faite **une fois**, par le domaine (`QueryText`, 03 §4.1) : `trim`, repli des blancs, retrait des caractères de contrôle, 1 à 500 caractères (borne du domaine, 03 §4.1, qui corrige D-32 ; limite réelle de l'API **NON VÉRIFIÉ** [V-81 · longueur-max-query]).
- L'adapter ne réécrit **pas** la requête. Il n'échappe rien, car la sérialisation JSON suffit à empêcher toute injection structurelle (recherche 02 D-32). Il ne retire ni guillemets ni opérateurs.
- La syntaxe interne du moteur dans `valeur` (guillemets, jokers, `*` seul qui signifie « tout » d'après un exemple officiel JURI, `exemples…md:1045-1119`) est **NON VÉRIFIÉ** [V-80 · syntaxe-moteur-valeur]. Une requête `*` est transmise telle quelle ; le résultat est borné par la pagination.
- Le texte de la requête n'est **jamais** journalisé, seule sa longueur l'est (§12.5). C'est cohérent avec l'audit, qui ne garde qu'une empreinte.

### 6.9 Recherches exactes par numéro (M6)

`find_article` et `find_by_number` emploient `/search` avec `typeRecherche = EXACTE` et un `typeChamp` spécialisé (00 §8). Correspondance champ × fond tirée de la documentation officielle (recherche 05 §2.2) :

| Numéro | `typeChamp` | Fonds qui l'acceptent | Exemple de valeur |
|---|---|---|---|
| numéro d'article | `NUM_ARTICLE` | `CODE_DATE`, `LODA_DATE` (`JORF` en M7) | `L1242-2`, `9` |
| numéro de texte | `NUM` | `LODA_DATE` (`JORF` en M7) | `2016-1321` |
| numéro de pourvoi | `NUM_AFFAIRE` | `JURI` | `24-13.470` |
| numéro de décision ou de requête | `NUM_DEC` | `CONSTIT`, `CETAT` | `2025-1173`, `506507` |
| numéro de délibération | `NUM_DELIB` | `CNIL` | `SAN-2026-002` |
| IDCC | `IDCC` | `KALI`, `ACCO` | `2121` |

Les recherches `NUM_ARTICLE` envoient `typePagination: "ARTICLE"`. C'est ce que fait l'exemple officiel « article 57 de la loi du 17 juillet 1978 » (`exemples…md:497-570` : `LODA_DATE`, `NUM_ARTICLE` en `EXACTE`, filtres `DATE_SIGNATURE` sur le jour de la loi et `DATE_VERSION`). Cet exemple sert aussi de modèle pour résoudre « article N de la loi du D » en M6.

Le comportement réel de chaque recherche exacte (casse, tirets typographiques, zéros de tête) est **NON VÉRIFIÉ** [V-120 · recherche-exacte-par-numero]. La forme attendue par `NUM_ARTICLE` (`L5-8` ou `5-8`, `1er`, suffixes latins) est **NON VÉRIFIÉ** [V-119 · num-article-forme-api] (03). Les stratégies complètes de `find_article` et `find_by_number` sont au §7.2.

### 6.10 Tests de sérialisation

- `search_request_matches_reference_json` : pour chacun des 8 fonds, avec `match=any`, `page=1` et `page_size=10`, le corps est égal, comme `serde_json::Value`, au payload de la référence (recherche 01 §3.1). Seule exception : la valeur de `singleDate`, qui est midi Paris (divergence volontaire listée dans [annexes/A-parite.md](annexes/A-parite.md), P-020).
- `type_recherche_follows_match` (3 cas) ; `filtres_order_is_deterministic` ; `date_facet_table_matches_doc` (table du §6.5, cohérente avec `Fond::has_date_filter`) ; `code_date_range_is_defensive_invalid_request_without_network` (garde défensive : `InvalidRequest { status: None }`, wiremock `expect(0)` ; le rejet `invalid_arg` lui-même est testé dans `lf-application`, 06 §2.2) ; `paris_noon_vectors` (table du §6.3) ; `enum_serialization_screaming_snake_case` (`AucuneCorrespondanceACetteExpression` donne `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION`, recherche 03 §12.3).

---

## 7. Endpoints par jalon

### 7.1 Table

Tous les chemins sont présents dans le Swagger 2.4.2 : vérifié par script sur la copie de pylegifrance pour les chemins de la version initiale de ce document (`paths`, `definitions`, réponses déclarées `200, 201, 401, 403, 404`) ; les chemins ajoutés par 00 §8 révision 2 (`getArticleWithIdEliOrAlias`, `eliAndAliasRedirectionTexte`, `/consult/jorf`) et la facette `ECLI` de JURI et CETAT sont relevés dans le Swagger et la documentation officielle par l'annexe C (§2.3, §3.7). Base : `{base_url}` (§2.1). Méthode : `POST` JSON partout. Liste fermée : 00 §8.

| Endpoint | Corps envoyé (champs requis du Swagger en gras) | DTO de réponse (§8.3) | Méthode du port | Jalon |
|---|---|---|---|---|
| `/search` | `SearchRequestDto` (§6.1) | `SearchResponseDto` | `search` | M2 |
| `/consult/lawDecree` | `{"**textId**": "LEGITEXT…", "**date**": "AAAA-MM-JJ"}` | `ConsultTextDto` | `get_act` | M2 |
| `/consult/getArticle` | `{"**id**": "LEGIARTI…" \| "KALIARTI…"}` | `GetArticleResponseDto` | `get_article` | M2 |
| `/consult/juri` | `{"**textId**": "JURITEXT…" \| "CONSTEXT…" \| "CETATEXT…"}` | `ConsultJuriResponseDto` | `get_decision` | M2 |
| `/consult/cnil` | `{"**textId**": "CNILTEXT…"}` | `ConsultJuriResponseDto` (même `TexteSimple`) | `get_deliberation` | M2 |
| `/consult/kaliText` | `{"**id**": "KALITEXT…"}` | `ConsultTextDto` (champs KALI) | `get_convention` | M2 |
| `/consult/acco` | `{"**id**": "ACCOTEXT…"}` | `ConsultAccoResponseDto` | `get_company_agreement` | M2 |
| `/consult/legi/tableMatieres` | `{"**textId**": "LEGITEXT…", "**date**": "AAAA-MM-JJ", "nature": "CODE"}` | `ConsultTextDto` | `get_act` quand le texte est un code (§7.3) | M2, conditionnel : seulement si V-96 (bloquante en M2) montre que `lawDecree` sur un code dépasse 4 Mio ou 10 s (00 §8) |
| `/consult/getArticleWithIdAndNum` | `{"id": "LEGITEXT…", "num": "L1242-2"}` | `GetArticleResponseDto` | `find_article` (version en vigueur seulement) | M6 |
| `/search` en `EXACTE` | `typeChamp` du §6.9 (+ `DATE_VERSION` pour CODE et LODA) | `SearchResponseDto` | `find_article` (autre date : `NUM_ARTICLE` + `DATE_VERSION`, hits filtrés par `cid`), `find_by_number` | M6 |
| `/search` filtré par la facette `ECLI` | `SearchRequestDto`, fond `JURI` ou `CETAT`, filtre `ECLI` à valeur exacte | `SearchResponseDto` | `find_by_number` (ECLI cité, §7.2) | M6 |
| `/consult/getJoWithNor` | `{"**nor**": "ECFI1524250L"}` | `ConsultJorfDto` | `find_by_number` (NOR) | M6 |
| `/consult/kaliContIdcc` | `{"**id**": "2121"}` | `KaliContDto` | `find_by_number` (IDCC → convention) | M6 |
| `/list/code` | `{"**pageNumber**": 1, "**pageSize**": 100, "sort": "TITLE_ASC"}` | `CodeListResponseDto` | `list_codes` et table des codes (`CodeTable`, §12.3), chargées paresseusement | M6 |
| `/consult/getArticleWithIdEliOrAlias` | `{"**idEliOrAlias**": "/eli/…/article_1"}` | `GetArticleResponseDto` | résolution d'un ELI d'article cité (§7.2) | M6 |
| `/consult/eliAndAliasRedirectionTexte` | `{"**idEliOrAlias**": "/eli/…/jo/texte"}` | `ConsultJorfDto` | résolution d'un ELI de texte cité (§7.2) | M6 |
| `/consult/getArticleByCid` | `{"**cid**": "LEGIARTI…"}` | `GetListArticleResponseDto` | `article_versions` (repli) | M7 |
| `/search/canonicalArticleVersion` | `{"**id**": "LEGIARTI…"}` | `CanonicalVersionsDto` | `article_versions` | M7 |
| `/consult/relatedLinksArticle` | `{"**articleId**": "LEGIARTI…"}` | `RelatedLinksDto` | `related_links` | M7 |
| `/search` avec `fond = JORF` | `SearchRequestDto` | `SearchResponseDto` | `search` (`Fond::Jorf`) | M7, si la surface JORF est retenue à l'ouverture de M7 (00 §4) |
| `/consult/jorf` (candidat) | `{"**textCid**": "JORFTEXT…"}` | `ConsultJorfDto` | consultation d'un hit `Jorf`, ou hits limités à `citation` + `source_url` | M7 : tranché à l'ouverture (V-135, 00 §8) |

`/suggest` est **retiré** (00 §8, AM-29) : aucun outil ni méthode de port ne le consomme ; il n'est ni appelé, ni déclaré dans `Endpoint`.

Corps des consultations : exactement ceux de la référence (client.py:187-208 ; parité P-021). Les clés diffèrent selon l'endpoint : `textId` pour `lawDecree`, `juri` et `cnil` ; `id` pour `getArticle`, `kaliText` et `acco`. `searchedString` et `fromSuggest` ne sont jamais envoyés. L'effet de `searchedString`, probablement un surlignage, n'est pas documenté (recherche 03 §3.3) ; c'est sans incidence puisqu'il n'est pas envoyé. La date de `lawDecree` est la `VersionDate` formatée `%Y-%m-%d`, forme validée en direct par la référence (client.py:190-193).

Chemins utilisés par des tiers mais **absents** du Swagger 2.4.2 (`consult/loda/version`, `consult/loda/versions`, `consult/juri/version(s)`, `consult/juri/ancienId`, recherche 05 §1.12) : interdits. Un test de dérive compare la liste des chemins appelés par le crate (constantes `Endpoint::path()`) à une copie versionnée de la liste des chemins du Swagger (`crates/lf-adapter-piste/tests/data/swagger-2.4.2-paths.txt`). Ce test empêche aussi tout chemin hors de la liste de 00 §8 (`/suggest` compris).

### 7.2 Stratégies M6

**`find_article(&CodeRef, &ArticleNum, VersionDate)`**

1. `CodeRef` → `LegiTextId` du code par la table des codes (`/list/code`, §12.3), chargée paresseusement au premier besoin (jamais au démarrage, 00 §6). Aucun identifiant de code n'est codé en dur : [annexes/donnees/codes-table.md](annexes/donnees/codes-table.md) signale même un conflit d'identifiant (Code rural et de la pêche maritime). Elle ne sert qu'au dictionnaire du parseur (03 §8).
2. Si `VersionDate` est la date du jour : `/consult/getArticleWithIdAndNum {id, num}`. L'adapter n'a pas d'horloge (§1.2) : il ne peut le savoir que si l'application le lui indique (forme à fixer avec la signature de `find_article` en M6, 02 §5.1) ; à défaut, l'étape 3 sert pour toutes les dates. Limites officielles : l'endpoint ne renvoie que la version « en vigueur au moment de la recherche », et « seuls les articles ne comportant pas de version en vigueur différée peuvent être trouvés » (Swagger, description du chemin). Sur `NotFound` ou réponse vide, passer à l'étape 3.
3. Sinon (autre date, 00 §8) : `/search` `CODE_DATE`, `typeChamp = NUM_ARTICLE`, `EXACTE`, `DATE_VERSION = VersionDate` (midi Paris, §6.3), `typePagination = ARTICLE` (§6.6), `pageSize = 100`, **sans** filtre `NOM_CODE`. Les hits CODE sont des codes porteurs d'extraits d'articles (search_code.json, recherche 03 §4.2). On garde le hit dont `titles[0].cid` est le `LEGITEXT` du code, puis l'extrait dont `num` égale le numéro demandé : il donne le `LEGIARTI`, puis `get_article`.
4. Pas de `NOM_CODE` : la documentation officielle l'utilise, mais deux clients tiers le disent cause de 500 (recherche 05 §2.2) : **NON VÉRIFIÉ** [V-121 · nom-code-filtre]. Il ne servira d'optimisation qu'après V-121 (00 §8).
5. Comportement réel de `getArticleWithIdAndNum` (forme de `num`, réponse sans article) : **NON VÉRIFIÉ** [V-118 · idandnum-comportement].

**`find_by_number(&NumberQuery)`** — signature fixée par [02-architecture.md](02-architecture.md) §5.1. `NumberQuery`, `NumberKind` et `NumberValue` sont des types de `lf-domain::search` (03 §4.11, 02 §5.1) dont la forme et les couples (fond, kind) autorisés sont fixés par [annexes/B-citations.md](annexes/B-citations.md) §6.6, qui fait foi ; ce document ne les redéfinit pas. L'adapter y lit : le fond, la nature du numéro (`NumberKind` → `typeChamp` du §6.9), la valeur (envoyée telle quelle), la plage `within` (facette du §6.5) et `version_date`, **présente pour les recherches CODE et LODA** (`Article`, `Text` ; `DATE_VERSION`, §6.2) et absente pour les autres couples (annexe B §6.6) : l'adapter n'a pas d'horloge, l'application fournit la date (§1.2).

- `Article` (CODE, LODA) : `/search` `NUM_ARTICLE` en `EXACTE`, `DATE_VERSION`, `typePagination = ARTICLE` (§6.9).
- `Nor` : `/consult/getJoWithNor`. Une réponse avec `hasLoda = true` permet de basculer vers la version consolidée. La documentation rappelle que « tous les textes n'ont pas de NOR » (`exemples…md:1209`).
- `Idcc` sur `Kali` : `/consult/kaliContIdcc` donne le conteneur (`KALICONT…`, `texteBaseId[]` → `KaliTextId`). En repli, `/search` `KALI` `IDCC` `EXACTE`. Sur `Acco` : `/search` `ACCO` `IDCC`.
- Autres : `/search` du fond, `typeChamp` du §6.9, `EXACTE`, `pageSize = 20`, `within` mappé sur la facette du §6.5 (ex. `DATE_DECISION` pour lever l'ambiguïté d'un numéro de pourvoi, comme pylegifrance, recherche 03 §11.6).
- **ECLI cité** (JURI, CETAT ; 00 §8) : `/search` du fond, requête `*` (forme de la sonde V-128 ; sens de `*` : §6.8), filtre par la facette `ECLI` à valeur exacte ; l'adapter rend les hits, et le contrôle de l'ECLI natif de la décision consultée relève de la résolution (annexe B). Sans ce filtre, un ECLI de la Cour de cassation ne serait vérifiable que par une recherche plein texte. Effet réel du filtre **NON VÉRIFIÉ** [V-128 · ecli-filtre-juri-cetat] (V-128). Un ECLI d'un autre pays n'atteint pas l'adapter (`ForeignEcli`, 00 §4).
- **ELI cité** (00 §8) : `/consult/getArticleWithIdEliOrAlias {idEliOrAlias}` pour un ELI d'article, `/consult/eliAndAliasRedirectionTexte {idEliOrAlias}` pour un ELI de texte ; l'ELI reçu de l'utilisateur est transmis tel quel, l'identifiant renvoyé (`LEGIARTI`, `JORFTEXT`…) est validé par le domaine, et aucun ELI n'est jamais fabriqué ni renvoyé s'il n'est pas natif (00 §4). Comportement réel **NON VÉRIFIÉ** [V-133 · eli-redirection-endpoints] (V-133).
- Les requêtes ECLI et ELI passent par `NumberQuery`, avec les variantes `NumberKind::Ecli` (fonds `Juri`, `Cetat`) et `NumberKind::Eli` (fond `Loda` ; valeur = chemin `/eli/…` cité, sans hôte), fixées par l'annexe B §6.6 ; l'adapter n'en dépend que par le `typeChamp`/la facette et l'endpoint ci-dessus. `Nor`, `Eli` et `Idcc` sur `Kali` passent par une consultation et rendent au plus un hit, dont l'identifiant est validé par le domaine (annexe B §6.6).
- Formes réelles des réponses de `getJoWithNor`, `kaliContIdcc` et `/list/code` (champs effectivement remplis) : **NON VÉRIFIÉ** [V-122 · endpoints-m6-reponses].

### 7.3 Stratégies M7

- **`article_versions`** : `/search/canonicalArticleVersion {id}` renvoie `articleVersions[{id, version, numero, etat, dateDebut, dateFin, ordre}]` (Swagger `ArticleVersion`), soit des métadonnées légères. Si la liste est vide ou si l'appel répond 404 : repli sur `/consult/getArticleByCid {cid}` (`listArticle[Article]`, toutes les versions avec leur texte), réduit à des `ArticleVersion`. `fr_diff_article` lit ensuite le texte des deux versions par `get_article`, mis en cache 24 h.
- **`related_links`** : `/consult/relatedLinksArticle {articleId}` renvoie `liensCite[]` et `liensCitePar[]` de `LiensRelatifsDTO {id, cidText, cidParent, nature ∈ {LODA, JORF, CODE, KALI, JORFCONT}, name, dateVigeur}` (Swagger). `dateVigeur` est une chaîne de chiffres (exemple `1470787200000`), lue par `opt_raw_date`. Le mapping produit `RelatedLinks { cites, cited_by }` de 03 §4.11 : `RelatedLink.target` vient de `id` (sinon `cidText`), validé en `DocumentId` ; un identifiant sans type consultable (`JORFCONT`, `JORFARTI`) donne `None`, le lien gardant son titre, sa nature et sa date. L'adapter rend les listes entières ; la correspondance `nature` → `kind`, la borne de 100 liens et la suite éventuelle relèvent de 05 §5.11. Forme et taille réelles : **NON VÉRIFIÉ** [V-136 · related-links-forme] (V-136).
- **`list_codes`** (M6, §7.2) : `/list/code` sans filtre `states`, paginé par 100 jusqu'à `totalResultNumber` (au plus 10 pages), chargé paresseusement au premier besoin, jamais au démarrage. Codes abrogés compris : chaque entrée (`CodeListing`, 03 §4.11) garde son `etat`, et les complétions de M7 filtrent `VIGUEUR`. 76 codes sont en vigueur (facette `FOND`, recherche 03 §5).
- Formes réelles et tailles des réponses de `tableMatieres`, `canonicalArticleVersion`, `getArticleByCid` et `relatedLinksArticle` : **NON VÉRIFIÉ** [V-135 · endpoints-m7-reponses].
- **Surface JORF** (00 §4, §8) : la recherche `fond = JORF`, la valeur `JORF` des enums, et la consultation d'un `JORFTEXT` (`/consult/jorf {textCid}`, avec bascule vers la version consolidée si `hasLoda = true`, ou hits limités à `citation` + `source_url`) sont tranchées ensemble à l'ouverture de M7 (V-135). D'ici là, l'adapter ne sert aucun `JORFTEXT` et `Fond` a 8 valeurs.

**`get_act` sur un code (D-47, v0.1)** : la décision se prend en M2, sur la mesure V-96 (bloquante en M2, 00 §8, §11) de `/consult/lawDecree` appliqué au `LEGITEXT` d'un code (Code civil, Code du travail). Si la réponse dépasse 4 Mio ou 10 s, l'adapter appelle `/consult/legi/tableMatieres {textId, date, nature: "CODE"}` plutôt que `/consult/lawDecree` pour un `LEGITEXT` de code, afin d'obtenir la table des matières sans le contenu des articles ; sinon `lawDecree` reste seul utilisé, et une réponse `nature == "CODE"` porte l'avertissement prévu par 05. Le port ne dit pas si un `LEGITEXT` est un code (même préfixe, limite D-47 de 03 §2.7) : le moyen de le savoir **avant** l'appel (table des codes chargée paresseusement, ce qui avancerait `/list/code` en M2, ou autre critère) est fixé en M2 avec le résultat de V-96 (§16, question 13). Comportement de `lawDecree` sur un code : **NON VÉRIFIÉ** [V-96 · lawdecree-sur-code] (V-96).

---

## 8. DTO

### 8.1 Principes

Repris de la recherche 03 §12.1 et figés ici :

1. **Un module par endpoint** ; aucune struct partagée entre endpoints pour des champs homonymes de types différents. `signataires` est un objet dans `/consult/kaliText` et une chaîne dans `TexteSimple` (recherche 03 §9) ; il n'est d'ailleurs pas déclaré du tout.
2. **Tolérance** : jamais `#[serde(deny_unknown_fields)]`. Serde ignore les champs inconnus par défaut. On ne déclare **que** les champs utilisés.
3. **Tout est optionnel** : `Option<T>` et `#[serde(default)]` partout. Tout champ à désérialiseur personnalisé porte `#[serde(default)]`, sinon son absence est une erreur `missing field` (serde_derive, recherche 03 §12.1 point 3). Toute liste passe par `null_default`, car `Vec<T>` refuse `null` (des listes `null` existent : `textTitles[].appellations`).
4. **Dates brutes** (`RawDate`) dans le DTO, converties en `CivilDate` du domaine dans le mapping (§9.1) ; `chrono` ne sert dans l'adapter qu'à `singleDate` (§6.3) et à `Retry-After`.
5. **Chaînes vides** : `""` devient `None` par `opt_nonempty` pour les champs où `""` signifie « absent ». Le contenu des chaînes non vides n'est jamais retouché (pas de `trim` du texte juridique dans le DTO).
6. **Rien de volumineux inutile** : `facets` de `/search` (jusqu'à 88 % de la réponse KALI, recherche 03 §5) et `data` d'ACCO (base64) ne sont **pas déclarés**. Serde les parcourt sans les allouer. En v0.1, les facettes ne sont jamais renvoyées (00 §7) : la proposition `Option<Box<RawValue>>` de la recherche 03 est inutile.
7. Les DTO sont `pub(crate)`. Le domaine ne voit jamais un type serde.

### 8.2 Helpers (`dto/de.rs`)

Signatures (code complet proposé dans la recherche 03 §12.2, à reprendre tel quel en M2) :

| Helper | Signature | Comportement |
|---|---|---|
| `null_default` | `fn<'de, D, T: Default + Deserialize<'de>>(D) -> Result<T, D::Error>` | `null` ou absent → `T::default()` (listes, maps) |
| `opt_nonempty` | `fn<'de, D>(D) -> Result<Option<String>, D::Error>` | `null`, absent, `""` ou blanc → `None` ; sinon la chaîne **telle quelle** |
| `opt_raw_date` | `fn<'de, D>(D) -> Result<Option<RawDate>, D::Error>`, `enum RawDate { Millis(i64), Text(String) }` | entier ms (négatifs compris) ; chaîne de chiffres → `Millis` ; autre chaîne non vide → `Text` ; flottant entier → `Millis` ; `bool`, objet, flottant fractionnaire → `None` (toléré, compté dans un diagnostic) |
| `opt_string_or_number` | `fn<'de, D>(D) -> Result<Option<String>, D::Error>` | chaîne non vide ou entier → `Some(String)` (`idcc`, `codeIdcc`, `sizeAttachment`) |
| `opt_u64_lenient` (ajout) | `fn<'de, D>(D) -> Result<Option<u64>, D::Error>` | nombre ou chaîne de chiffres → `Some` ; sinon `None` (`expires_in`, totaux) |

`RawDate::Text` accepte `AAAA-MM-JJ` et l'ISO `AAAA-MM-JJTHH:MM:SS.sss+0000`, dont seule la partie date est gardée (§9.1).

### 8.3 Champs minimaux par endpoint

Noms JSON tels que vus dans les fixtures et le Swagger. Sauf mention, tous les champs sont `Option<…>` avec `#[serde(default)]`. Les structures détaillées sont celles de la recherche 03 §12.3 à §12.7 ; ce tableau en fixe la liste.

**`/search` — `SearchResponseDto`** (recherche 03 §4, §12.3)

| Struct | Champs (`rename_all = "camelCase"`) |
|---|---|
| `SearchResponseDto` | `results: Vec<SearchResultDto>` (`null_default`), `totalResultNumber: u64` (`opt_u64_lenient`), `totalArticleResultNumber: u64` (idem), `typePagination: String` |
| `SearchResultDto` | `titles: Vec<SearchTitleDto>`, `text`, `nature`, `origin`, `etat`, `solution`, `date`, `datePublication`, `dateSignature` (`opt_raw_date`), `nor`, `num`, `idcc` (`opt_string_or_number`), `raisonSociale`, `themes: Vec<String>`, `appellations: Vec<String>`, `resumePrincipal: Vec<String>`, `sections: Vec<SearchSectionDto>` |
| `SearchTitleDto` | `id`, `cid`, `title` |
| `SearchSectionDto` | `id`, `title` (`opt_nonempty`), `extracts: Vec<SearchExtractDto>` |
| `SearchExtractDto` | `id`, `num` (`opt_nonempty`), `values: Vec<String>`, `searchFieldName` |

**`/consult/lawDecree` et `/consult/kaliText` — `ConsultTextDto`** (recherche 03 §6, §9, §12.4)

| Struct | Champs |
|---|---|
| `ConsultTextDto` | `id`, `cid`, `title`, `nor`, `nature`, `etat`, `jurisState`, `textAbroge: bool`, `dateParution`, `dateDebutVersion`, `dateFinVersion` (`opt_raw_date`), `numParution`, `jorfText` (`opt_nonempty`), `appellations: Vec<String>`, `sections: Vec<TocSectionDto>`, `articles: Vec<TocArticleDto>` ; KALI : `conteneurs: Vec<ConteneurDto>`, `numeroBo`, `originePubli`, `typeTexte` (`opt_nonempty`) |
| `TocSectionDto` (récursif) | `id`, `title`, `intOrdre: i64`, `etat`, `dateDebut`, `dateFin` (`opt_raw_date` : `AAAA-MM-JJ` en KALI), `sections`, `articles` |
| `TocArticleDto` | `id`, `num`, `etat`, `intOrdre: i64`, `content` (HTML, attesté en KALI), `dateDebut`, `dateFin` (ms en KALI), `lstLienCitation`, `lstLienModification: Vec<LienArticleDto>` |
| `LienArticleDto` | `linkType`, `linkOrientation`, `textCid`, `textTitle`, `articleId`, `articleNum`, `natureText` |
| `ConteneurDto` | `cid` (`KALICONT…`), `num` (IDCC, `opt_nonempty`), `titre` |

**`/consult/getArticle` — `GetArticleResponseDto { article: Option<ArticleDto> }`** (recherche 03 §7, §12.5)

| Struct | Champs |
|---|---|
| `ArticleDto` | `id`, `cid`, `num`, `etat`, `texte`, `texteHtml`, `nota`, `notaHtml` (`opt_nonempty`), `dateDebut`, `dateFin` (`opt_raw_date`), `fullSectionsTitre`, `sectionParentTitre`, `context: ArticleContextDto`, `textTitles: Vec<TextTitleDto>`, `versionPrecedente` (`opt_nonempty`), `isMultipleVersions: bool` (**alias** `multipleVersions`) ; M7 : `articleVersions: Vec<ArticleVersionDto>` |
| `ArticleContextDto` | `titresTM: Vec<ContextEntryDto>` (**`rename = "titresTM"`**), `titreTxt: Vec<ContextEntryDto>` |
| `ContextEntryDto` | `id`, `titre`, `debut`, `fin` (`opt_raw_date`, `AAAA-MM-JJ`) |
| `TextTitleDto` | `id`, `cid`, `titre`, `nature` |

Présence en direct de `texteHtml` sur `/consult/getArticle` : **NON VÉRIFIÉ** [V-90 · texte-html-getarticle-juri] (03), d'où le repli sur `texte`.

**`/consult/juri` et `/consult/cnil` — `ConsultJuriResponseDto { text: Option<JuriTextDto> }`** (recherche 03 §8, §12.6)

| Struct | Champs |
|---|---|
| `JuriTextDto` | `id`, `origine`, `nature`, `num`, `ecli` (`opt_nonempty`), `titre`, `titreLong`, `juridiction`, `formation`, `solution`, `numeroAffaire: Vec<String>`, `dateTexte`, `dateTexteComputed`, `datePubli`, `datePubliComputed` (`opt_raw_date`), `texte`, `texteHtml`, `sommaire: Vec<SommaireDto>`, `citationJp` (`opt_nonempty`), `liens: Vec<TexteLienDto>`, `publicationRecueil`, `typePublicationBulletin` (`opt_nonempty`), `decisionAttaquee: DecisionAttaqueeDto` ; CONSTIT : `natureQualifiee`, `titreLoiDef`, `idTexteJo`, `titreJo` ; CNIL : `natureDelib`, `etat` ; navigation : `ancienId`, `titrages: Vec<String>` |
| `SommaireDto` | `id`, `resumePrincipal`, `abstrats`, `autreResume` (`opt_nonempty`) |
| `TexteLienDto` | `typeLien`, `sens`, `title`, `cidTexte`, `id`, `num`, `natureTexte` (`opt_nonempty` : les identifiants sont vides en JURI) |
| `DecisionAttaqueeDto` | `formation`, `date` |

Les personnes (`president`, `avocats`, `rapporteur`, `commissaire`, `avocatGl`) ne sont **pas** déclarées : elles ne servent pas au résumé, et la loi interdit d'évaluer ou de comparer les pratiques des magistrats à partir de leur identité (recherche 05 §5.1).

**`/consult/acco` — `ConsultAccoResponseDto { acco: Option<AccordDto> }`** (recherche 03 §10, §12.7)

`AccordDto` : `id`, `nature`, `titreTexte`, `raisonSociale`, `siret`, `codeIdcc` (`opt_string_or_number`), `codeApe`, `secteur`, `themes: Vec<LibelleDto>`, `syndicats: Vec<LibelleDto>`, `numero`, `dateTexte`, `dateEffet`, `dateDepot`, `dateDiffusion`, `dateFin` (`opt_raw_date`), `conformeVersionIntegrale: bool`, `fileSize: String` ; `LibelleDto { code, libelle }`. **`data` jamais déclaré.**

**Erreurs et OAuth** : `PisteErrorDto` (§11.3), `TokenResponseDto` et `OAuthErrorDto` (§3.2).

**M6** (Swagger 2.4.2 `definitions`)

| DTO | Champs |
|---|---|
| `ConsultJorfDto` (`/consult/getJoWithNor`) | `id`, `cid`, `title`, `nor`, `nature`, `dateTexte`, `dateParution` (`opt_raw_date`), `hasLoda: bool`, `eli` (`opt_nonempty`, jamais fabriqué), `textNumber` |
| `KaliContDto` (`/consult/kaliContIdcc`) | `id` (`KALICONT…`), `titre`, `num`, `numeroTexte` (« IDCC 1261 »), `nature`, `texteBaseId: Vec<String>`, `sections: Vec<TocSectionDto>` |
| `CodeListResponseDto` (`/list/code`) | `totalResultNumber`, `results: Vec<CodeListResultDto { id, cid, titre, etat, dateDebut, dateFin, lastUpdate }>` (dates ISO `…+0000` d'après l'exemple du Swagger) |
| `/consult/getArticleWithIdEliOrAlias` | `GetArticleResponseDto` réutilisé (Swagger `GetArticleResponse`, annexe C §2.3) |
| `/consult/eliAndAliasRedirectionTexte` | `ConsultJorfDto` réutilisé (Swagger `ConsultJorfResponse`, annexe C §2.3) |

**v0.1, conditionnel (V-96)** : `/consult/legi/tableMatieres` réutilise `ConsultTextDto`.

**M7**

| DTO | Champs |
|---|---|
| `CanonicalVersionsDto` | `articleVersions: Vec<ArticleVersionDto { id, version, numero, etat, dateDebut, dateFin, ordre: i64 }>` |
| `GetListArticleResponseDto` | `listArticle: Vec<ArticleDto>` |
| `RelatedLinksDto` | `liensCite`, `liensCitePar: Vec<LienRelatifDto { id, cidText, cidParent, nature, name, dateVigeur }>` |
| `/consult/jorf` (si retenu à l'ouverture de M7) | `ConsultJorfDto` (+ `hasLoda`) |

### 8.4 Pièges

| Piège | Où | Parade | Test |
|---|---|---|---|
| `titresTM` : `rename_all = "camelCase"` produirait `titresTm` | `article.context` | `#[serde(rename = "titresTM")]` explicite | `titres_tm_is_renamed` |
| `signataires` : objet en KALI, chaîne dans `TexteSimple` | kaliText / juri | non déclaré ; jamais de DTO commun | `kali_and_juri_dto_are_distinct` |
| `idcc`, `codeIdcc`, `sizeAttachment` : chaînes numériques, nombres possibles (la référence typait `str \| int`) | search ACCO, acco | `opt_string_or_number` (E-14, D-24) | `idcc_accepts_string_and_number` |
| casse de `nature` : `arret`, `qpc`, `code` en recherche ; `ARRET`, `QPC`, `Texte`, `Délibération` en consultation | partout | valeur brute conservée ; comparaisons insensibles à la casse dans le mapping | `nature_case_is_preserved` |
| NOR en minuscules en recherche (`cscl2115694s`) | search | `Nor` du domaine en majuscules (03 §4.2) | — |
| `isMultipleVersions` (fixture) contre `multipleVersions` (Swagger, KALI) | getArticle | `alias` | `multiple_versions_alias` |
| `typePagination` renvoyé différent de l'envoyé | search CODE | `PaginationMemo`, valeur rendue à PISTE en page suivante, jamais au modèle (§6.6) | `type_pagination_is_echoed_on_next_page` |
| suffixe `_JJ-MM-AAAA` sur `titles[].id` et `id` de lawDecree | search LODA/CODE, lawDecree | `parse_versioned` / `VersionedId` du domaine (03 §2.4 ; tâche de tête de M2, C4 : le suffixe est validé comme une vraie date et conservé, un suffixe mal formé est refusé, un suffixe bien formé hors `LEGITEXT` est toléré puis retiré) | `suffix_is_version_date` |
| `cid` LODA = `JORFTEXT` ≠ `id` | search, lawDecree | `Option<JorfTextId>` | — |
| `etat` LODA `null` mais `jurisState` = `Vigueur` | lawDecree | `status_of(etat, jurisState)` (03 §3.3, E-10) | `loda_status_falls_back_to_juris_state` |
| `liens[]` JURI : identifiants `""`, titres fragmentés | juri | `opt_nonempty` ; titres passés au parseur (M7) | `empty_string_is_none` |
| `facets` absent (CODE) ou énorme (KALI) | search | non déclaré | `facets_are_never_allocated` |
| dates sous 3 encodages, parfois mêlés dans une même réponse (KALI : sections en `AAAA-MM-JJ`, articles en ms) | partout | `opt_raw_date` | `raw_date_accepts_ms_numeric_string_ymd_iso` |
| `bool` jamais une date (la référence acceptait `true` → 1970-01-01) | partout | `visit_bool` → `None` | `bool_is_not_a_date` |
| `num` des extraits KALI malformé (`"8.4 de la convention)"`) | search KALI | `num = None` pour KALI (03 §4.1) | — |
| enveloppe `null` (`{"text": null}`, `{"article": null}`) | consultations | `NotFound` (§11) | `consult_empty_envelope_maps_to_not_found` |
| `lstLienCitation` décrit « Toujours vide » par le Swagger mais rempli en KALI | kaliText | déclaré, lu | — |

### 8.5 `serde_path_to_error` et `CorpusError::Decode`

- Toute désérialisation de corps passe par `serde_path_to_error::deserialize`. L'erreur devient `CorpusError::Decode { detail }` (variantes fixées par 02 §5.1), avec un `detail` qui assemble l'endpoint, le chemin JSON et la catégorie serde, par exemple `/consult/juri: text.sommaire[0].abstrats: invalid type: integer, expected a string`.
- Le message ne contient **aucune valeur** du corps. Seuls figurent le chemin JSON et la catégorie serde, car une valeur pourrait contenir des données personnelles ou un texte long.
- Limite de récursion : serde_json limite par défaut la profondeur d'imbrication (128 niveaux selon sa documentation, **NON VÉRIFIÉ** [V-10 · serde-json-recursion]). Elle n'est pas levée (pas de feature `unbounded_depth`). Le domaine borne de son côté la TOC à 64 niveaux (03 §1.3).
- Corps HTML (page d'erreur d'une passerelle), JSON `null` à la racine, JSON tronqué : `Decode`, sans réessai (corrige D-11 et E-21 : la référence levait une exception sans code).

---

## 9. Mapping DTO → domaine

### 9.1 Règles communes

Fonctions pures de `map/`, du type `fn(dto, &contexte de requête) -> Result<Entité, CorpusError>`. Elles appellent les constructeurs de `lf-domain` et ne font aucune E/S.

| Règle | Contenu | Défauts corrigés |
|---|---|---|
| (a) Introuvable | enveloppe absente ou `null`, ou objet sans `id`, `cid` ni titre ⇒ `NotFound`. Objet rempli mais sans `id` ⇒ l'identifiant **demandé** est utilisé, avec un diagnostic `warn` | parité (recherche 03 §12.8 a ; S15) |
| (b) Identifiant renvoyé ≠ identifiant demandé (après retrait du suffixe) | l'entité garde l'identifiant renvoyé, vérité de l'API ; diagnostic `warn` | — |
| (c) Identifiants | `FromStr`/`Id::parse` du domaine, tolérants (espaces, casse) ; suffixe malformé refusé ; suffixe `_JJ-MM-AAAA` conservé par `parse_versioned` (tâche de tête de M2, C4 ; 03 §2.4) | D-28, D-31, E-16 |
| (d) Routage | la famille de juridiction, l'URL et la règle de citation viennent du **type** d'identifiant. `origine` n'est qu'un contrôle de cohérence (diagnostic s'il diverge) | D-28, E-18 |
| (e) Dates | `RawDate::Millis(ms)` → `CivilDate::from_unix_millis` (livré, `date.rs:91` ; epoch négatifs compris par `div_euclid`, sans dépendance à la plateforme ; vecteur 1804-03-21 = -5231606400000, test ajouté en tâche de tête de M2) ; `Text("AAAA-MM-JJ")` → `CivilDate::parse` ; `Text("AAAA-MM-JJT…+0000")` → partie date puis `CivilDate::parse` ; puis `open_ended` sur `CivilDate` (toute date ≥ 2222-01-01 devient `None` ; **tâche de tête de M2**, 00 §11.1 : le livré garde aujourd'hui la sentinelle `2999-01-01`, `act.rs:41`) ; les champs `…Computed` sont lus en priorité. Un horodatage qui n'est pas à minuit UTC produit un diagnostic : convention générale **NON VÉRIFIÉ** [V-100 · convention-horaire-timestamps] (03) | D-20, D-21, E-11, E-35 |
| (f) Textes | `LongText::best_of(texteHtml, texte)` (03 §7.4) ; les champs courts passent par `text::clean` | D-26 |
| (g) Statut | `status_of(etat, jurisState)` | D-21, E-10 |
| (h) Erreurs du domaine | `DomainError` non utilisateur (`is_input()` faux, 03 §9 : `TocTooDeep`, `TextTooLarge`) ⇒ `Decode` ; `HitKindMismatch` sur un hit de recherche ⇒ hit écarté et compté (règle i) | — |
| (i) Contrat de citation | construit par le domaine, jamais `None` (03 §5) ; un hit dont l'identifiant est invalide ou incohérent avec le fond est **écarté et compté** (`SearchPage.dropped`) au lieu d'être renvoyé à moitié | D-30 |

### 9.2 Par entité

Types et champs absents du livré : `TocNode` = tâche de tête de M2 ; `LegalStatus`, `Nor`, `Idcc`, `parent`/`path`/`nota`, `SearchHit::new`, `DocumentId` et `LongText` = reports de M2 (00 §11.1, « Reports par jalon : M2 ») ; `LodaArticle` seulement après V-140/V-142.

| Entité (03 §4) | Fonction | Sources principales | Points propres | Défauts |
|---|---|---|---|---|
| `SearchPage`, `SearchHit` | `map::search_page(dto, &SearchQuery)` | `results[]`, `totalResultNumber`, `totalArticleResultNumber`, `typePagination` | `kind` = fond demandé ; `id` = `titles[0].id` ; `title` sans `<mark>` ; `date` = `date`, puis `datePublication`, puis `dateSignature` ; `snippet` = `text`, sinon `extracts[0].values[0]` (≤ 280 caractères) ; `articles` CODE et KALI depuis `sections[].extracts[]` ; `company`, `idcc` (ACCO) ; `total_articles` si > 0 ; `has_more = page × page_size < total` | D-09, D-30, D-33, E-26 |
| `Act` | `map::act(dto, &LegiTextId, VersionDate)` | lawDecree | `version_start` = suffixe (via `parse_versioned`) ou `dateDebutVersion` ; `version_end` = `dateFinVersion` (≥ 2222-01-01 → `None` via `open_ended`, 00 §4 ; tâche de tête de M2, le livré gardant la sentinelle `2999-01-01`, `act.rs:41`) ; `cid: JorfTextId` ; `Toc::build(TocDraft)` depuis `sections`/`articles` (arbre `TocNode` : titres de sections, tri `intOrdre`, dédoublonnage, profondeur ≤ 64 ; 03 §4.2). **Tâche de tête de M2** (C4) : le livré n'a que `TocEntry` (liste plate d'articles, `act.rs:13`), renommé `TocArticle` en tâche de tête de M2 (00 §11.1, tâche 2) ; `nature == "CODE"` : avertissement prévu par 05 | D-25, D-47, E-10, E-11 |
| `ArticleText` | `map::article(dto, &ArticleId)` | getArticle | `code_title` = `textTitles[nature=="CODE"].titre`, sinon `textTitles[0].titre` ; `parent` (nature du texte parent → URL `code_article` ou `loda_article`, 03 §5.5) ; `path` = `context.titresTM[].titre` ; `nota` = `notaHtml`, sinon `nota` ; `previous_version` | D-26, D-29 |
| `Decision` | `map::decision(dto, &DecisionId)` | juri | `ecli` natif validé (`Ecli::parse` livré, `ecli.rs` ; `Display` livré (`ecli.rs:61`), `FromStr` ajouté en M2, M1-D11), jamais fabriqué ; `numero_affaire` = `numeroAffaire`, sinon `[num]` pour CETAT ; `date` = `dateTexteComputed`, sinon `dateTexte` ; `OfficialSummary { analyses, titrages }` (report de M2, 00 §11.1) mappé par fond (00 §4 ; 03 §4.4) : JURI `abstrats` → analyses, `resumePrincipal`/`autreResume` → titrages ; CETAT `resumePrincipal` → analyses, `abstrats` → titrages ; codes de classement retirés et doublons supprimés par le domaine (sémantique par fond **NON VÉRIFIÉ** [V-109 · sommaire-semantique-par-fond], V-109) ; `cited_case_law` = `citationJp` ; `applied_texts` | D-26, D-27, D-28 |
| `Deliberation` | `map::deliberation(dto, &CnilTextId)` | cnil | `date`, `date_publication` (`…Computed` d'abord) ; `nature_delib` | D-27 |
| `Convention` | `map::convention(dto, &KaliTextId)` | kaliText | `idcc` = `conteneurs[0].num` ; `container_title` (saut de ligne final retiré) ; `bulletin` = `originePubli`, sinon `numeroBo` ; TOC avec `inline_text` depuis `content` (pas de N+1 appels) | D-20, D-25 |
| `CompanyAgreement` | `map::agreement(dto, &AccoTextId)` | acco | métadonnées seules ; `themes` et `unions` limités aux libellés non vides ; **aucun** champ texte (P-036) | E-14 |
| M6 : hits de résolution, `CodeListing`, `CodeTable` | `map::code_list`, `map::kali_cont`, `map::jorf_by_nor`, `map::eli_redirect` | §8.3 M6 | `CodeListing { id, title, etat }` (03 §4.11 ; pas `CodeEntry`, nom du dictionnaire statique de l'annexe B) ; entrée sans `LEGITEXT` valide ou sans titre écartée et comptée ; `CodeTable` : titre normalisé (vue repliée du domaine) → `LegiTextId`, `etat` | D-47 |
| M7 : `ArticleVersion`, `RelatedLinks` | `map::article_versions`, `map::related_links` | §8.3 M7 | versions triées par `dateDebut`, puis `ordre` ; liens : §7.3 | — |

Les tests de contrat (§13.3) figent ces mappings par snapshots `insta` sur les 16 fixtures et par les valeurs golden de [annexes/donnees/parite-golden.json](annexes/donnees/parite-golden.json), aux divergences volontaires près (listées dans [annexes/A-parite.md](annexes/A-parite.md) et 03 §5.6).

### 9.3 Diagnostics

Le domaine renvoie ses anomalies comme valeurs (03 §1.1). L'adapter les journalise (`debug` pour un hit écarté, `warn` pour un identifiant incohérent, un mojibake réparé ou un horodatage hors minuit) et les compte dans `StatsSnapshot` (`dropped_hits`, `mojibake_repaired`, `id_mismatch`, `non_midnight_timestamps`, `retries`, `rate_limited_waits`). Ces compteurs sont lisibles par `lf-app` (`check-config`) et éventuellement par `fr_coverage` (décision dans 05). Ils ne quittent jamais la machine (ADR-0005).

---

## 10. Réparation du mojibake

### 10.1 Constat

6 fixtures sur 16 (`search_loda`, `search_code`, `search_juri`, `consult_loda`, `consult_article`, `consult_juri`) contiennent de l'UTF-8 décodé à tort en ISO-8859-1 : 252 séquences de 13 motifs distincts (235 dans 60 valeurs, 17 dans 14 clés de facettes), et aucun caractère accentué correct. Les « 235 séquences sur 60 champs » de la recherche 02 (D-35) correspondent aux seules valeurs ; 252 est le chiffre de référence, unité « séquence », recompté par ce document avec le motif du §10.2 sur les 6 fixtures (`python3 -I`, Python 3.13). Les 10 autres fixtures sont saines (recherche 03 §11.1 ; recherche 02 D-35, §5). La présence de caractères de contrôle C1 (`PREMIÃ\x88RE`) désigne ISO-8859-1 et non Windows-1252. La cause la plus probable est l'outil de capture : le même endpoint `/consult/juri` est sain dans une campagne ultérieure. Origine **NON VÉRIFIÉ** [V-72 · mojibake-origine] (03). Dans les fixtures, le mojibake est en caractères bruts, sans aucune séquence d'échappement `\uXXXX` (contrôle de ce document : 0 occurrence dans les 16 fichiers).

### 10.2 Algorithme exact

Appliqué au **texte entier du corps** (réponses 2xx et corps d'erreur), après le décodage UTF-8 strict et avant serde (§4.4). La fonction pure est `lf_domain::text::encoding::repair_latin1_mojibake(&str) -> Cow<str>` (module non livré : tâche de tête de M2, 00 §11.1, reliquat de M1 ; 03 §1.5, §7.1) ; l'esquisse est dans la recherche 03 §11.1.

1. **Détection** `has_latin1_mojibake(s)` : vrai si `s` contient un caractère C1 (U+0080 à U+009F), **ou** une paire adjacente `(p, c)` avec `p` ∈ [U+00C2, U+00F4] et `c` ∈ [U+0080, U+00BF].
2. Si la détection est fausse : identité (`Cow::Borrowed`).
3. Si un caractère de `s` dépasse U+00FF : identité. Un texte qui contient `’`, `œ`, `–` ou `…` n'a pas été décodé en latin-1.
4. Sinon, `bytes = s.chars().map(|c| c as u8)`, soit le ré-encodage latin-1, exact puisque tous les caractères sont ≤ U+00FF.
5. `String::from_utf8(bytes)` **strict**. En cas d'échec : identité.
6. Si le résultat contient encore un motif (étape 1) : identité. Sinon, le résultat remplace le corps.

Les caractères structurels JSON sont ASCII et identiques en latin-1 et en UTF-8 : la structure du document est préservée.

### 10.3 Garanties

| Garantie | Raison | Mesure |
|---|---|---|
| Identité sur des données saines | tout caractère > U+00FF bloque la réparation ; un `é` isolé (octet `E9` suivi d'ASCII) n'est pas de l'UTF-8 valide | identité sur les 10 fixtures saines : 7 bloquées par un caractère > U+00FF, 3 sans motif (recherche 03 §11.1) |
| Réparation complète des 6 fixtures | aller-retour latin-1 → UTF-8 | 100 % réparées, 0 motif résiduel |
| Idempotence | le résultat accepté ne contient plus de motif, donc un second passage est l'identité | 0 changement au second passage |
| Tout-ou-rien | une seule décision pour tout le corps | réparation du corps entier égale à la réparation champ par champ sur les 6 fixtures (égalité stricte des JSON) |
| Risque résiduel | un texte entièrement latin-1 dont tous les caractères non ASCII formeraient par hasard de l'UTF-8 valide | jamais observé ; la détection seule a un faux positif dans une fixture saine (`é` suivi d'une NBSP, « constitutionnalité ; », 2 champs de consult_juri_constit.json), neutralisé par l'étape 3 (caractère > U+00FF dans le corps) et, à défaut, par l'étape 5 (`E9 A0` n'est pas de l'UTF-8 valide) |

Angle mort : si PISTE échappait les caractères non ASCII (`Ã©`), un mojibake éventuel serait invisible au niveau du corps, qui serait alors en ASCII. Forme des chaînes servies par PISTE (caractères bruts ou échappements) : **NON VÉRIFIÉ** [V-71 · json-echappements-unicode]. Le test de contrat `no_mojibake_after_mapping` contrôle chaque chaîne des entités produites, et le smoke test live (§13.5) le contrôle aussi. Si l'angle mort se confirmait, la même fonction pure serait appliquée par champ dans le mapping, ce qui demanderait une révision de 00 §8.

### 10.4 Ordre impératif

La réparation se fait **avant toute normalisation des NBSP** (00 §8 ; 03 §7.1). La séquence mojibake de `à` est `Ã` suivi de U+00A0 : normaliser U+00A0 avant la réparation casse 10 champs des fixtures (recherche 03 §11.1 ; mesure de `docs/plan/annexes/donnees/scripts/dto_repair_check.py`, script versionné, SHA-256 `ef5c1c80c82a063a407ba9134d3afc3e5526bc659e5650810b38a3096d9d0530`, exécuté avec Python 3.13 en `python3 -I`, 00 §10). Le même script contrôle les mesures du §10.3 (identité, idempotence, égalité corps entier / champ par champ) ; toute nouvelle mesure passe par lui, jamais par une copie locale. Au niveau du corps, aucune normalisation n'a encore eu lieu. Les helpers DTO ne modifient pas les chaînes (`opt_nonempty` ne fait `trim` que pour tester le vide) ; `text::clean` (domaine) intervient après, dans le mapping.

### 10.5 Métrique et journalisation

- Réparation appliquée : compteur `mojibake_repaired` (+1 par corps) et `tracing::warn!(endpoint = ep.template(), bytes = len, "latin1 mojibake repaired in PISTE response")`. Le corps et l'identifiant demandé ne sont pas journalisés.
- Détection positive mais réparation refusée (étapes 3, 5 ou 6) : compteur `mojibake_unrepaired` et `warn`. Le corps est gardé tel quel. Ce cas signale des données réellement ambiguës, à recapturer.
- `LF_PISTE_MOJIBAKE_REPAIR=off` (00 §1, défaut `on`) désactive la réparation mais **pas** la détection ni ses journaux. La réparation n'est jamais appliquée par défaut au texte soumis par l'utilisateur (`fr_verify_citations`, 00 §8) : elle ne concerne que les corps de réponse PISTE.
- La recherche 02 §5 recommandait de détecter sans jamais « réparer silencieusement » un texte officiel. 00 §8 tranche pour une réparation active par défaut. Le compromis retenu : réparer, compter, journaliser en `warn`, et permettre la désactivation.

### 10.6 Tests

`repairs_all_six_corrupt_fixtures` (0 motif résiduel ; contrôle de `PREMIÈRE CHAMBRE CIVILE`, de `République numérique` et de la clé de facette `Juridictions du premier degré`) ; `identity_on_clean_fixtures` (10 fichiers, octets identiques) ; `idempotent` ; `body_level_equals_field_level` ; `nbsp_normalization_before_repair_is_a_bug` (test qui démontre le piège) ; `does_not_touch_text_with_chars_above_ff` ; `synthetic_mojibake_roundtrip` (une fixture saine encodée en UTF-8, relue en latin-1 puis réencodée, redonne exactement l'original) ; `error_body_marker_survives_mojibake` (§11) ; proptest : pour toute `String` `s`, `repair(s)` vaut `s`, ou bien son encodage UTF-8 est l'encodage latin-1 de `s` et il ne contient plus de motif ; `repair_toggle_off_keeps_detection`.

### 10.7 Vérification live et sortie de la réparation

Smoke test live (§13.5) : requêtes sur `JURITEXT000051743650`, `LEGIARTI000006419288` et `LEGITEXT000033205014` (les identifiants des fixtures corrompues, recherche 02 §7 point 1), avec enregistrement des octets bruts par `cargo xtask record-fixture`. Attendu : `é` encodé `C3 A9`, aucun motif, compteur à 0. Si la sandbox et la production n'en produisent jamais, la réparation reste active, car elle ne coûte rien sur des données saines, et les 6 fixtures sont remplacées par des captures saines (D-35). Si l'API en produit, le corpus de tests est enrichi de ces cas réels.

---

## 11. « Introuvable »

### 11.1 Comportement par endpoint

| Endpoint | Comportement connu pour un identifiant bien formé mais inexistant | Statut | Classification |
|---|---|---|---|
| `/consult/juri` (JURITEXT) | **HTTP 400**, corps `{"timestamp":…,"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null,"exceptionDetails":null}` ; pas de 200 vide | sondé en direct par pylegifrance le 2026-04-13 avec `JURITEXT000000000000` et `JURITEXT999999999999` (`pylegifrance/fonds/juri.py:124-135`, forme du corps dans `tests/unit/fonds/test_juri_verification_endpoints.py:115-121`, test unitaire sur réponse simulée) ; tiers, non revérifié par nous | `NotFound` |
| `/consult/juri` (CETATEXT) | même 400 affirmé par pylegifrance, mais sans sonde live (seulement une docstring de test unitaire, réponse simulée) | **NON VÉRIFIÉ** [V-68 · introuvable-par-endpoint] | règles du §11.2 (le marqueur suffit) |
| `/consult/juri` (CONSTEXT) | inconnu (pylegifrance n'a sondé en direct que JURITEXT) | **NON VÉRIFIÉ** [V-68 · introuvable-par-endpoint] | règles du §11.2 |
| `/consult/lawDecree`, `/consult/getArticle`, `/consult/cnil`, `/consult/kaliText`, `/consult/acco` | inconnu : 400 avec le même marqueur, 404, 200 avec une enveloppe `null` ou 500 sont tous plausibles | **NON VÉRIFIÉ** [V-68 · introuvable-par-endpoint] (recherche 01 §16 point 4 ; recherche 03 §15 point 9) | règles du §11.2 |
| M6 et M7 (`getArticleWithIdAndNum`, `getJoWithNor`, `kaliContIdcc`, `canonicalArticleVersion`, `getArticleByCid`, `relatedLinksArticle`) | inconnu | **NON VÉRIFIÉ** [V-68 · introuvable-par-endpoint] | règles du §11.2 ; pour une **recherche** (`find_*`), « aucun résultat » est un résultat vide, pas une erreur |
| `/search` | 0 résultat = **succès** (`total = 0`, `hits = []`) | parité S16, E-04 | jamais `NotFound` |
| endpoint token | — | — | jamais `NotFound` (§3.5) |

Le smoke test live `live_unknown_ids_classification` (§13.5) enregistre la réponse réelle de chaque endpoint pour des identifiants `…000000000000` et `…999999999999`. Le résultat complète cette table en M2 et rend définitive la classification du §11.2 : V-68 (réponses à un identifiant inexistant) et V-66 (forme d'un 404 PISTE) sont bloquantes en M2 (00 §11, 09 §2.18).

### 11.2 Classification

Arbre appliqué par `classify.rs`, après lecture bornée, décodage et réparation du corps. Cette classification « introuvable » fait foi (00 §0) : 02 §9.2 (lignes 13, 13 bis et 14) la reprend, et les messages vus par le modèle sont ceux de 05 §7.2.

1. **2xx** : DTO de l'endpoint. Enveloppe absente ou vide ⇒ `NotFound` (§9.1 a). Pour `/search`, jamais `NotFound`.
2. **400** sur une consultation, et `PisteErrorDto.message` contient `valider est fausse` (comparaison insensible à la casse, sur le texte réparé ; la forme `a valider` sans accent est aussi acceptée) ⇒ `NotFound`. Le marqueur ne s'applique qu'aux consultations dont l'identifiant a été validé par le domaine, comme le fait pylegifrance (`juri.py:829-838`) : un 400 ne peut alors pas venir d'un identifiant malformé.
3. **400**, autres cas ⇒ `InvalidRequest { status: Some(400), detail }` (message PISTE nettoyé, ≤ 200 caractères, sans caractère de contrôle), journalisé en `warn` comme bug probable, rendu `[upstream_error] Legifrance rejected the request (HTTP 400): {détail}` (00 §5, parité S10). Jamais `invalid_arg` : toute contrainte imputable à l'entrée (dates, fond sans facette, pagination profonde, formats, longueur de `query`) est validée avant l'appel du port, et une heuristique sur un corps d'erreur de forme inconnue (V-67) produirait des `invalid_arg` trompeurs ; le détail nettoyé permet quand même au modèle de reformuler.
4. **404** :
   - sur l'endpoint token ⇒ `Config`, rendu `PISTE OAuth endpoint not found: check LF_PISTE_OAUTH_URL.` (05 §7.2 ; 02 §9.2 ligne 8 ; §3.5) ;
   - sur `/search` ⇒ `Config`, rendu `Legifrance endpoint not found: check LF_PISTE_BASE_URL.` (05 §7.2, 404 de routage ; 02 §9.2 ligne 14). Il n'y a pas de « ressource » à ne pas trouver : le chemin ou l'URL est faux ;
   - sur une consultation, si l'URL de base est officielle (`PisteEndpoints.official`) et que le corps n'est pas du HTML ⇒ `NotFound`. Le chemin vient de la liste vérifiée du Swagger, donc il existe ;
   - sinon (hôte personnalisé ou page HTML de passerelle) ⇒ `Config`, rendu par le même gabarit `Legifrance endpoint not found: check LF_PISTE_BASE_URL.` (05 §7.2).
   - La forme réelle d'un 404 PISTE, de routage ou de ressource, est **NON VÉRIFIÉ** [V-66 · forme-404-piste] (recherche 02 D-10 ; V-66, bloquante en M2). Cette règle est provisoire jusqu'au smoke test.
5. **500** après 2 tentatives : `Upstream { status: 500, retryable: false }` (§5.1), jamais `NotFound`. « Ne jamais conclure à l'inexistence sans preuve », principe repris d'un client tiers (recherche 05 §1.13).

Un `not_found` signifie donc toujours que l'API a dit que la ressource n'existait pas. Un problème d'URL ou de configuration ne peut jamais faire croire au modèle qu'un texte n'existe pas, ce que la référence permettait (D-10).

### 11.3 Corps d'erreur PISTE

```rust
// dto/error.rs — non compilé, à valider au jalon qui l'implémente
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PisteErrorDto {
    #[serde(default, deserialize_with = "de::opt_u64_lenient")] pub error: Option<u64>,   // 400
    #[serde(default)] pub status: Option<serde_json::Value>,   // "Bad Request" (chaîne) ; forme variable tolérée
    #[serde(default)] pub message: Option<String>,
    #[serde(default)] pub error_code: Option<String>,
}
```

La désérialisation est **facultative** : un corps d'erreur non JSON (HTML, texte vide) est classé par le statut seul. La forme générale des corps d'erreur de l'API (tous statuts confondus) est **NON VÉRIFIÉ** [V-67 · corps-erreur-piste]. La seule forme connue est celle du 400 de `/consult/juri` (pylegifrance).

---

## 12. Interaction avec le cache et l'audit

### 12.1 Opérations et clés

`lf-adapter-cache` enveloppe l'adapter : `CachedCorpus<PisteCorpus>` (00 §9, ADR-0007). La clé est `(environnement, opération, paramètres normalisés)` (`CacheKey { env: PisteEnvironment, op: Op }`, 02 §7.2). L'environnement est celui de `PisteConfig.env` (`PisteCorpus::environment()`), passé à `CachedCorpus` par `lf-app` à la composition, ce qui corrige D-15. `Hash` sur les identifiants (clés moka) : tâche de tête de M2 selon 00 §11.1 (M1-D11, avancé depuis M3).

| Opération (nom de cache) | Méthode du port | Paramètres de la clé | TTL (00 §9) |
|---|---|---|---|
| `search` | `search` | `SearchQuery` normalisée : fond, mode, texte, page, page_size, version_date (au jour), date_range ; **pas** de `typePagination`, choisie par l'adapter (03 §4.1) | 1 h |
| `act` | `get_act` | `LegiTextId`, `VersionDate` | 24 h |
| `article` | `get_article` | `ArticleId` | 24 h |
| `decision` | `get_decision` | `DecisionId` | 24 h |
| `deliberation` | `get_deliberation` | `CnilTextId` | 24 h |
| `convention` | `get_convention` | `KaliTextId` | 24 h |
| `agreement` | `get_company_agreement` | `AccoTextId` | 24 h |
| `find_article` (M6) | `find_article` | `CodeRef` normalisé, `ArticleNum`, `VersionDate` | 24 h (consultation) |
| `find_by_number` (M6) | `find_by_number` | `NumberQuery` : fond, type, valeur, `within`, `version_date` | 1 h (recherche) |
| `codes` (M6) | `list_codes` | — | 7 j |
| `article_versions`, `related_links` (M7) | idem | `ArticleId` | 24 h |

Ces TTL sont ceux de [02-architecture.md](02-architecture.md) §7.2 (`Search` et `FindByNumber` 1 h ; consultations, `FindArticle`, `Versions` et `Related` 24 h ; `Codes` 7 jours), qui fait foi pour la clé et la politique du cache.

### 12.2 Exigences sur l'adapter

1. **Déterminisme** : à paramètres de port égaux, corps de requête égal. Aucun champ ne dépend de l'horloge : `singleDate` se déduit de `VersionDate`, jamais de `now` (D-01). La seule exception est la `typePagination` mémorisée (§6.6), cohérente pendant la durée de vie du mémo. Elle n'entre pas dans la clé de cache : une page > 1 mise en cache a pu être obtenue avec le repli (`ARTICLE` pour CODE, `DEFAUT` ailleurs) plutôt qu'avec la valeur mémorisée. Le mémo et le cache de recherche ont le même TTL (1 h), et pour CODE, seul cas observé, repli et mémo donnent la même valeur ; l'écart éventuel ailleurs est mesuré par V-82 (M5).
2. **Erreurs** : toujours `Err`, jamais un `Ok` vide déguisé. `NotFound` est une erreur, donc il n'est pas mis en cache (00 §9 ; corrige D-15, où la référence gardait les « introuvables » 24 h).
3. **Réponses vides** : une `SearchPage` sans résultat est un `Ok`. C'est le cache qui décide de ne pas la stocker (00 §9).
4. **Identifiants absents** : la configuration est fixée au démarrage et le cache est vide au démarrage. Aucune réponse en cache ne peut donc exister sans configuration valide. La question de la référence, qui exigeait les identifiants avant de lire son cache disque persistant (E-23), ne se pose plus.
5. Le jeton OAuth n'est jamais dans le cache.

### 12.3 Mémos internes

Deux structures en mémoire, petites, bornées, propres à l'API PISTE, donc dans l'adapter et non dans `lf-adapter-cache` :

| Mémo | Contenu | Borne | TTL | Jalon |
|---|---|---|---|---|
| `PaginationMemo` | clé de recherche sans `page` → dernière `typePagination` reçue | 1 024 entrées, éviction de la plus ancienne | 1 h | M2 |
| `CodeTable` | titre de code normalisé → `LegiTextId`, `etat` (codes abrogés compris) | ~150 entrées | 7 j, rafraîchissement mono-vol ; en cas d'échec, l'ancienne table est gardée jusqu'à 7 j de plus, sinon `Upstream` | M6 |

Implémentation : `std::sync::Mutex<…>` avec horodatage `MonoClock`, sans dépendance supplémentaire. Le mémo n'est jamais tenu pendant un `await`. La `CodeTable` est chargée paresseusement au premier besoin, jamais au démarrage (00 §6) ; c'est la seule table des codes du processus : elle sert `find_article`, `list_codes` et les completions de M7 (00 §3).

### 12.4 Audit : rien ici

- L'adapter n'écrit aucune ligne d'audit et ne mesure pas `duration_ms`. Le décorateur `Audited` de `lf-application` enveloppe **chaque** exécution d'outil, succès comme erreurs, avec une durée réelle (00 §3, corrige D-02 et D-03).
- Exigence sur l'adapter : le `detail` et le `Display` de `CorpusError`, journalisés sur stderr, ne contiennent ni secret, ni jeton, ni URL avec paramètres, ni valeur du corps (§5.1, §8.5). Le champ `error` de l'audit (≤ 500 caractères, 00 §7) reçoit le message final porté par `AppError`, rédigé par `lf-application::messages` (02 §3.2, §9.3), qui n'y recopie que des données de même nature (statut, détail PISTE nettoyé ≤ 200 caractères).
- Le code d'erreur de l'audit (`error_code`) découle de la variante de `CorpusError` via `AppError` (02 §9.2).

### 12.5 Journalisation (`tracing`, stderr uniquement)

- Noms et champs des spans : 02 §11.1, qui fait foi. Un span `piste_request` par **tentative** : `endpoint` (chemin seul, ex. `/consult/juri`), `fond`, `attempt`, `http_status`, `elapsed_ms`, `retry_in_ms`, `token_refreshed`, `body_bytes`, `mojibake_repaired` ; niveau `debug`, et `warn` quand les réessais sont épuisés. Un span `oauth_token` par requête token : `outcome`, `expires_in_s` (`debug` en succès, `warn` en échec). Corrige D-46.
- **Jamais** journalisés, à aucun niveau : corps de requête et de réponse, texte de la requête de recherche (seule sa longueur l'est), jeton, secret, `client_id`, en-têtes.
- Les identifiants de documents (publics) peuvent apparaître au niveau `debug`.
- Les journaux `debug`/`trace` de reqwest et hyper ne contiennent pas les corps. Le filtre par défaut (`LF_LOG`, défini dans `lf-app`) reste au niveau `info` pour ces crates.

---

## 13. Tests

### 13.1 Infrastructure

- **wiremock** 0.6.5 : un `MockServer` par test, dans un `#[cfg(test)] mod tests` async du crate (pas dans `tests/`, 00 §10). La `PisteConfig` de test pointe `oauth_url` vers `{server}/oauth/token` et `base_url` vers `{server}/dila/legifrance/lf-engine-app` (`ConfigWarning::InsecureLoopback`).
- **Temps** : on ne gèle **pas** l'horloge `tokio` (`start_paused`) pendant des E/S réelles avec wiremock. L'avance automatique d'une horloge gelée pourrait déclencher à tort le délai de reqwest pendant l'attente d'une socket. Ce comportement est **NON VÉRIFIÉ** [V-19 · wiremock-temps-pause], et la conception l'évite. À la place :
  - `ManualClock: MonoClock` et `RecordingSleeper: Sleeper` (qui avance la `ManualClock` de la durée demandée et la note) rendent déterministes l'expiration du jeton, le backoff, `Retry-After`, le limiteur et le délai global, sans attente réelle ;
  - les tests de délai HTTP utilisent de vraies durées courtes : `request_timeout = 200 ms`, réponse retardée de 1 s par `ResponseTemplate::set_delay`.
- **Comptages** : `Mock::expect(n)` (vérifié à la fin du test), et un `Respond` personnalisé qui compte les requêtes en vol (`AtomicUsize`) pour le plafond de concurrence.
- **Journaux** : un `tracing` subscriber de test capture la sortie, pour les tests de non-fuite de secrets.
- **Fixtures** : `include_bytes!` des fichiers bruts, servis tels quels (`set_body_raw(bytes, "application/json")`), pour que la réparation du mojibake soit éprouvée sur les octets réels.

### 13.2 Scénarios wiremock

Les noms suivent la matrice de la recherche 02 §6 quand elle en propose.

**OAuth**

| Test | Mise en place | Attendu | Couvre |
|---|---|---|---|
| `oauth_success_form_body_and_token_reuse` | token 200 ; 5 appels API successifs | 1 seule requête token (`expect(1)`) ; corps formulaire exact (`grant_type`, `client_id`, `client_secret`, `scope=openid`) ; `Content-Type` formulaire ; `Authorization: Bearer <jeton>` sur l'API | P-022 |
| `oauth_token_refreshed_60s_before_expiry` | `expires_in = 3600` ; `ManualClock` avancée de 3 539 s puis de 3 541 s | pas de nouveau jeton à 3 539 s ; nouveau jeton à 3 541 s | P-022 |
| `oauth_expires_in_tiny_uses_half_margin` | `expires_in = 30` | renouvellement à 15 s | §3.3 |
| `oauth_expires_in_null_uses_default_3600` | `expires_in: null`, puis absent, puis `"3600"` | 3 600 s | D-10 |
| `concurrent_calls_share_one_token_refresh` | token retardé de 100 ms ; 10 appels simultanés | 1 requête token, 10 succès | D-12, P-022 |
| `api_401_refreshes_once_then_succeeds` | API : 401 puis 200 | 2 requêtes token, 2 requêtes API, succès | P-023 |
| `api_401_twice_maps_to_auth` | API : 401, 401 | `Auth` ; 2 requêtes token ; pas de 3e requête API | P-023, D-12 |
| `401_then_503_then_200_requests_exactly_two_tokens` | API : 401, 503, 200 | 2 requêtes token, 3 requêtes API | D-12, E-20 |
| `invalidate_does_not_drop_fresher_token` | deux tâches, l'une reçoit un 401 avec un vieux jeton | le jeton neuf est gardé | §3.3 |
| `oauth_invalid_client_401_maps_to_auth_no_retry` | token : 401 `{"error":"invalid_client"}`, puis variante 400 avec le même corps (statut réel inconnu, RFC 6749 §5.2 admet les deux : **NON VÉRIFIÉ** [V-60 · oauth-reponses]) | `Auth` dans les deux cas, 1 seule requête token, `detail` qui cite `invalid_client` et l'environnement | D-10 |
| `oauth_400_maps_to_auth` | token : 400 | `Auth` | D-10 |
| `oauth_404_maps_to_config_not_not_found` | token : 404 HTML | `Config` | D-10 |
| `oauth_200_without_access_token_maps_to_config` | token : `{}` | `Config` (`no access_token`) | S7 |
| `oauth_503_is_retried` | token : 503, 200 | succès, 2 requêtes token | §3.3 |
| `missing_credentials_makes_no_network_call` | `credentials = None` | `Config` immédiat ; `expect(0)` sur tous les mocks | D-39, P-014 |
| `cancel_during_refresh_releases_lock` | token retardé ; futur abandonné | l'appel suivant obtient un jeton | §3.3 |

**Résilience**

| Test | Mise en place | Attendu | Couvre |
|---|---|---|---|
| `429_retry_after_is_honoured` | 429 `Retry-After: 2`, puis 200 | attente notée ≥ 2 s ; succès | D-12 |
| `429_retry_after_http_date_is_honoured` | `Date: <D>` et `Retry-After: <D + 3 s>` (IMF-fixdate) | attente notée ≈ 3 s (différence des deux en-têtes, sans horloge murale) | D-12, §5.2 |
| `429_retry_after_http_date_without_date_header_uses_backoff` | `Retry-After: <date HTTP>` sans `Date` | en-tête ignoré : attente = backoff | §5.2 |
| `429_retry_after_beyond_deadline_fails_fast` | `Retry-After: 120`, délai 60 s | `RateLimited { retry_after: 120 s }`, aucune attente | §5.2 |
| `429_three_times_maps_to_rate_limited` | 429 ×3 | `RateLimited` ; exactement 3 requêtes | 00 §8 |
| `500_then_200_succeeds_after_backoff` | 500, 200 | succès ; attente notée dans [0,5 s ; 1 s) | P-024 |
| `500_twice_maps_to_upstream_not_retryable` | 500 ×2 (un 3e mock `expect(0)`) | `Upstream { 500, retryable: false }` ; exactement 2 requêtes | 00 §8 (AM-03) |
| `503_then_500_then_500_stops_at_second_500` | 503, 500, 500 | `Upstream { 500, retryable: false }` ; 3 requêtes, dont 2 sur 500 | §5.2 |
| `503_three_times_maps_to_upstream_retryable` | 503 ×3 | `Upstream { 503, retryable: true }` ; attentes [0,5 ; 1) puis [1 ; 2) s | P-024 |
| `400_is_not_retried` | 400 sans marqueur | `InvalidRequest { status: Some(400) }`, `detail` nettoyé ≤ 200 caractères ; `expect(1)` | §5.1 |
| `403_maps_to_auth` | 403 | `Auth`, sans réessai | §5.1, 00 §5 |
| `404_on_search_is_config_not_not_found` | `/search` → 404 | `Config` | D-10 |
| `404_on_consult_official_host_is_not_found` / `404_html_is_config` | — | `NotFound` / `Config` | §11.2 |
| `redirect_is_not_followed` | 302 `Location` vers un 2e mock | `Upstream { 302 }` ; 2e mock `expect(0)` | §4.2 |
| `network_error_is_retried` | port local fermé | 3 tentatives, puis `Upstream { status: None, retryable: true }` | 00 §8 |
| `request_timeout_maps_to_timeout` | réponse retardée de 1 s, `request_timeout = 200 ms` | `Timeout` après 3 tentatives (ou à l'échéance) | §5.1 |
| `tool_deadline_caps_retries` | délai 1 s (virtuel), 503 permanent | rend la main avant l'échéance virtuelle ; aucune attente au-delà | D-14 |
| `deadline_from_task_local_is_used` | portée `Deadline::scope` de `lf-application` (02 §7.1) ouverte par le test, avec 1 s restante ; `ManualClock` partie d'une origine arbitraire ; 503 permanent | la durée restante est convertie dans `ManualClock` ; l'adapter rend la main avant l'échéance, sans attente au-delà | §5.3, 00 §3 |
| `no_deadline_published_uses_tool_deadline` | aucun `Deadline` publié | reste = `tool_deadline` | §5.3 |
| `rate_limiter_spaces_requests` | `ManualClock`, 2 req/s, rafale 1, 10 acquisitions | ≥ 4,5 s virtuelles cumulées | D-13 |
| `rate_limit_wait_beyond_deadline_fails_fast` | limiteur saturé, délai court | `RateLimited` local | §5.4 |
| `concurrency_cap_is_respected` | réponses retardées ; 10 appels | au plus 4 requêtes en vol | D-13 |
| `search_all_respects_concurrency_cap` (avec `lf-application`, M7) | 5 fonds | au plus 4 en vol, résultats complets | D-13 |

**Corps**

| Test | Mise en place | Attendu | Couvre |
|---|---|---|---|
| `invalid_json_maps_to_decode_with_path` | `{"text":{"numeroAffaire":12}}` | `Decode`, dont le `detail` contient `text.numeroAffaire` et aucune valeur du corps | §8.5 |
| `html_body_maps_to_decode` | 200 `text/html` | `Decode` | D-11 |
| `json_null_body_maps_to_decode` | corps `null` | `Decode` | D-11 |
| `empty_2xx_body_on_consult_is_not_found` | 200 vide | `NotFound` | §11.2 |
| `oversized_body_is_rejected` | `Content-Length` = 17 Mio ; variante découpée en morceaux sans `Content-Length` | `Decode` (« too large »), sans réessai | D-41 |
| `gzip_body_is_limited_after_decompression` | corps gzip qui se décompresse au-delà de 16 Mio | `Decode` (« too large ») | §4.3 |
| `invalid_utf8_body_maps_to_decode` | octets `C3 28` | `Decode` (« invalid UTF-8 ») | §4.4 |
| `mojibake_body_is_repaired_and_counted` | consult_juri.json brut | `PREMIÈRE CHAMBRE CIVILE` dans le texte ; compteur 1 ; un `warn` | §10 |
| `clean_body_is_untouched` | consult_juri_constit.json | compteur 0 ; entité identique au golden | §10 |
| `error_body_marker_survives_mojibake` | 400 dont le message est en mojibake | `NotFound` | §11.2 |

**Endpoints** (un test par fixture ; le corps de la requête est vérifié par `body_json` sur un `serde_json::Value` attendu)

| Test | Fixture | Requête vérifiée | Couvre |
|---|---|---|---|
| `search_{loda,code,juri,constit,cetat,cnil,kali,acco}_fixture_maps` (8 tests) | `search_*.json` | fond API ; `typeRecherche` ; `DATE_VERSION` à midi Paris (LODA, CODE) ; `filtres: []` ailleurs ; `pageNumber`, `pageSize` | P-020, P-032 |
| `consult_law_decree_fixture_maps` | consult_loda.json | `{"textId","date"}` | P-021, P-033 |
| `consult_get_article_fixture_maps` (+ variante KALIARTI) | consult_article.json | `{"id"}` | P-021, P-034 |
| `consult_juri_fixture_maps` ×3 | consult_juri*.json | `{"textId"}` | P-021, P-035 |
| `consult_cnil_fixture_maps` | consult_cnil.json | `{"textId"}` | P-035 |
| `consult_kali_text_fixture_maps` | consult_kali.json | `{"id"}` | P-033 |
| `consult_acco_fixture_maps` ; `acco_large_data_field_is_skipped` | consult_acco.json ; variante synthétique avec un `data` de 5 Mio | `{"id"}` ; aucune chaîne de 5 Mio allouée dans le DTO | P-036 |
| `unknown_juri_id_400_maps_to_not_found` | corps 400 de pylegifrance | — | §11 |
| `consult_empty_envelope_maps_to_not_found` (×6) | `{"text":null}`, `{"article":null}`, `{"acco":null}`, `{}` | — | §9.1 a |
| `search_page_2_sends_page_number_2` | — | `pageNumber: 2` | D-09 |
| `type_pagination_is_echoed_on_next_page` | page 1 répond `ARTICLE` | la page 2 envoie `ARTICLE` | §6.6 |
| `code_page_2_without_memo_sends_article` | mémo vide | `ARTICLE` | §6.6 |
| `date_range_maps_to_fond_facet` (table, 7 fonds) | — | facette et format `dates` du §6.5 | §6.5 |
| `user_agent_contains_crate_version` ; `accept_json_header` | — | en-têtes | D-43, P-025 |

**Sécurité et configuration**

`config_rejects_http_scheme_except_loopback` ; `config_rejects_userinfo_query_fragment` ; `config_warns_on_custom_host` ; `config_rejects_env_mismatch` (hôte OAuth sandbox avec base production) ; `config_env_mismatch_on_production_host_with_default_env` (hôtes de production, `LF_PISTE_ENV` absent : `EnvMismatch`, message qui demande `LF_PISTE_ENV=production`, aucune déduction) ; `config_problem_starts_in_degraded_mode` (`Config` immédiat, `expect(0)`) ; `fr_eli_variables_are_read_with_deprecation_warning`, `legacy_unread_variables_warn_ignored` et `invalid_advanced_setting_falls_back_to_default_with_warn` (`LF_PISTE_RPS=0`, `LF_PISTE_MOJIBAKE_REPAIR=maybe` ; tous trois dans `lf-app`) ; `debug_of_config_redacts_secret` ; `bearer_header_is_sensitive` ; `error_messages_never_contain_secret_or_token` (secret et jeton aléatoires, contrôle des journaux capturés, des `Display` et des `Debug`) (D-40).

### 13.3 Contrats sur les fixtures de la référence

Les 16 fixtures de la référence (Apache-2.0, attribution dans `NOTICE`) sont copiées **sans modification** dans `tests/fixtures/reference/`. Chacune reçoit un `.meta.toml` : `source = "matematicsolutions/fr-eli-mcp@d3b5b13"`, `endpoint`, `fond`, `sha256_raw`, `known_issues` (par exemple `["mojibake", "truncated:texte@600"]`, d'après la recherche 03 §2), `env = "sandbox"`, `captured_at = "inconnu"`.

| Test | Contenu | Couvre |
|---|---|---|
| `dto_parses_every_fixture` | les 16 fixtures se désérialisent dans leur DTO | D-11, D-23 |
| `mapping_snapshots` | snapshot `insta` (feature `json`, maps triées) de chaque entité produite | P-038 |
| `mapping_matches_golden` | valeurs de [annexes/donnees/parite-golden.json](annexes/donnees/parite-golden.json), divergences volontaires exceptées (liste de [annexes/A-parite.md](annexes/A-parite.md) : typographie des citations, sentinelles (toute date ≥ 2222-01-01, 00 §4) → `None`, `etat` LODA, textes réparés des 6 fixtures en mojibake, TOC avec sections…) | P-030 à P-039 |
| `no_mojibake_after_mapping` | aucune chaîne d'une entité produite ne contient de motif | §10 |
| `fixtures_have_meta_sidecar` ; `fixture_sha256_matches_meta` | intégrité des fixtures | D-35 |
| `toc_preserves_section_titles` (consult_loda.json : 26 sections + 62 articles = 88 entrées) ; `kali_inline_content_is_available_without_extra_call` (`expect(0)` sur `getArticle`) | TOC | D-25 |
| `null_list_is_empty_vec`, `missing_list_is_empty_vec`, `empty_string_is_none`, `raw_date_accepts_ms_numeric_string_ymd_iso`, `sentinel_2999_and_2222_are_none`, `negative_ms_1804`, `bool_is_not_a_date`, `suffix_is_version_date`, `titres_tm_is_renamed`, `multiple_versions_alias`, `idcc_accepts_string_and_number`, `loda_status_falls_back_to_juris_state` | helpers et pièges (§8.4) | D-20, D-21, D-24 |
| fuzz « jamais de panique » : mutations des fixtures (`null` injectés, champs supprimés, types changés) | `Ok` ou `Err(Decode)`, jamais de panique | D-23 |

Limites connues de ces fixtures (élaguées, tronquées, sans métadonnées de capture, recherche 03 §2) : elles valident les **formes**, pas les **volumes**. Les budgets de M5 se mesurent sur des fixtures recapturées.

### 13.4 Enregistrement de nouvelles fixtures : `cargo xtask record-fixture`

Cette section fait foi pour la syntaxe, les sorties et le sidecar de `record-fixture` (00 §0 ; 02 §3.8 la reprend).

```text
cargo xtask record-fixture --endpoint /consult/juri --body '{"textId":"JURITEXT000051743650"}' \
    --name juri/JURITEXT000051743650 [--env sandbox|production] [--allow-error] [--redact <pointeur JSON>] [--force]
cargo xtask record-fixture --plan crates/lf-adapter-piste/tests/fixtures/plan.toml   # lot
```

- **Identifiants** : `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` uniquement, jamais en argument. `sandbox` par défaut. En production (adopté par défaut, Q3 ; 00 §10) : `--env production` n'est employé qu'à la main par le mainteneur, pour des sondes en lecture seule et des fixtures de données publiques, marquées `env = "production"` dans leur sidecar ; jamais en CI, où aucun identifiant de production n'existe.
- **Client** : `xtask` ne dépend d'aucun crate `lf-*` (00 §2). Il lance `cargo run -p lf-adapter-piste --example record_fixture --features record` en boîte noire et lui passe ses arguments. L'exemple réutilise le vrai client de l'adapter (même User-Agent, même OAuth, même limiteur) au moyen de l'API brute de la feature `record` (§1.4), qui renvoie statut, en-têtes utiles et octets ; c'est lui qui calcule `sha256_raw`, `sha256_file` et `mojibake_detected` (détection de l'étape 1 du §10.2, exposée par la même API brute) et qui écrit les fichiers. Débit de 1 requête/s au plus, en sandbox comme en production.
- **Sorties** : `crates/lf-adapter-piste/tests/fixtures/recorded/<endpoint>/<name>.json`, contenant les **octets bruts reçus**, non reformatés et non réparés, et `<name>.meta.toml` :

```toml
endpoint = "/consult/juri"
request = '{"textId":"JURITEXT000051743650"}'
env = "sandbox"
captured_at = "2026-11-02T10:15:00Z"
http_status = 200
content_type = "application/json"
content_encoding = "gzip"           # tel que reçu (si présent)
bytes_raw = 8239
sha256_raw = "…"
sha256_file = "…"                   # différent de sha256_raw seulement si caviardage
mojibake_detected = false           # détection du §10.2 (étape 1) sur le corps brut, calculée par l'exemple
redactions = []
tool = "xtask record-fixture 0.1.0"
license = "Licence Ouverte 2.0 — source : Légifrance / DILA"
```

- **Caviardage** : aucun secret ne peut figurer dans un fichier, car en-têtes et jeton ne sont jamais écrits. Par défaut, seul `acco.data` (base64 du `.docx`, volumineux) est remplacé par `"<redacted: N bytes base64>"`. Le fichier devient alors une dérivée : `sha256_raw` garde l'empreinte des octets d'origine et `redactions` liste les pointeurs JSON. `--redact` permet d'en ajouter. Les données sont publiques (Licence Ouverte 2.0), pseudonymisées par la DILA pour JURI et occultées pour la CNIL (D-45).
- `--allow-error` enregistre aussi les réponses 4xx et 5xx, pour les sondes « introuvable » du §11.
- Refus d'écraser un fichier sans `--force`. Le plan de lot par défaut couvre les 6 fixtures en mojibake (mêmes identifiants), des versions **non tronquées** de `consult_juri` (JURI), `consult_cnil` et `lawDecree`, un `getArticle` avec `texteHtml`, les sondes « introuvable » de chaque endpoint, `lawDecree` sur le Code civil et le Code du travail (taille et durée, V-96, bloquante en M2), la page 2 d'une recherche CODE, et une recherche avec chaque facette de date. Le plan de lot s'exécute en sandbox ; ses éventuelles entrées de production sont lancées à la main par le mainteneur (Q3).

### 13.5 Smoke tests live (opt-in)

- Fichier `crates/lf-adapter-piste/tests/live.rs`. Chaque test porte `#[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]`. Sans identifiants, le test sort aussitôt avec un message sur stderr.
- Exécution : `LF_PISTE_ENV=sandbox cargo test -p lf-adapter-piste --test live -- --ignored --test-threads=1` (séquentiel, 200 ms au moins entre deux appels, comme les tests d'intégration de pylegifrance, recherche 05 §3.3). Politique (adoptée par défaut, Q3 ; 00 §10) : **sandbox par défaut** ; la CI ne lance les tests live qu'en sandbox, par le workflow `live.yml` décrit dans [06-qualite.md](06-qualite.md) §7.5 ; **jamais d'identifiants de production en CI**. En production, seules des sondes en lecture seule, lancées à la main par le mainteneur avant chaque étiquette, à 1 requête/s au plus (`LF_PISTE_ENV=production`, `LF_PISTE_RPS=1`, `LF_PISTE_BURST=1`), servent à lever les vérifications propres à la production (V-59, V-61).

| Test live | Vérifie | Libellés traités |
|---|---|---|
| les 9 scénarios de la référence (recherche 01 §11.7 : LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO, puis consultation des hits) | parité de bout en bout | P-060 |
| `live_token_reuse_and_expires_in` | `expires_in`, `scope`, réutilisation | [V-60 · oauth-reponses] |
| `live_unknown_ids_classification` (M2, bloquant : V-66, V-68) | réponse de chaque endpoint pour `…000000000000` et `…999999999999` (enregistrée par xtask) | [V-68 · introuvable-par-endpoint], [V-66 · forme-404-piste], [V-67 · corps-erreur-piste] |
| `live_no_mojibake_on_reference_ids` | §10.7 | [V-72 · mojibake-origine], [V-71 · json-echappements-unicode] |
| `live_singledate_noon_paris_semantics` (M2, bloquant) | §6.3 : suffixe de version du hit pour `version_date = D` avec `singleDate` à midi Paris, comparé à minuit UTC, à minuit Paris et à la veille | [V-76 · singledate-midi-paris] |
| `live_lawdecree_on_code_size` (M2, bloquant) | §7.3 : taille et durée de `lawDecree` sur le Code civil et le Code du travail, seuils 4 Mio et 10 s | [V-96 · lawdecree-sur-code], [V-75 · taille-max-textes] |
| `live_type_pagination_page2_code` (M5) | §6.6 | [V-82 · typepagination-page2] |
| `live_deep_pagination_limit` (M5) | §6.7 : positions 9 901 à 10 000, puis au-delà | [V-83 · pagination-profonde] |
| `live_date_facets_reduce_total` | pour chaque facette du §6.5, le total diminue ; bornes incluses | [V-78 · filtres-dates-effet] |
| `live_match_modes_totals` | `any` ≥ `all` ≥ `exact` sur une même requête | [V-79 · mode-all-semantique] |
| `live_gzip_negotiated` | en-tête `Content-Encoding` | [V-70 · piste-gzip] |
| `live_texte_html_presence` | `texteHtml` sur `getArticle` et `juri` (JURI) | [V-90 · texte-html-getarticle-juri] |
| (M6) `live_find_article_today_and_past`, `live_exact_number_searches` | §7.2 | [V-118 · idandnum-comportement], [V-120 · recherche-exacte-par-numero], [V-119 · num-article-forme-api], [V-122 · endpoints-m6-reponses], [V-121 · nom-code-filtre] |
| (M6) `live_ecli_facet_and_eli_redirects` | §7.2 : facette `ECLI` sur JURI et CETAT ; `getArticleWithIdEliOrAlias`, `eliAndAliasRedirectionTexte` | [V-128 · ecli-filtre-juri-cetat], [V-133 · eli-redirection-endpoints] |
| (M7) `live_versions_links_toc` | §7.3 (`/suggest` retiré) | [V-135 · endpoints-m7-reponses], [V-136 · related-links-forme] |

Ni 429 ni `Retry-After` ne sont provoqués volontairement : ce serait contraire à l'esprit des CGU. [V-63 · retry-after-piste] et [V-62 · quota-piste-prod] se vérifient par observation fortuite (journal `warn`) et par la console PISTE.

### 13.6 Couverture des défauts

| Défauts | Tests (§13.2, §13.3) |
|---|---|
| D-01 | `search_request_matches_reference_json`, `paris_noon_vectors` (la clé de cache elle-même est testée dans `lf-adapter-cache`) |
| D-04, D-18, D-39, D-40 | tests de configuration, `missing_credentials_makes_no_network_call`, `error_messages_never_contain_secret_or_token` |
| D-06 | client unique (test de construction : un seul `Client` dans `Inner`) |
| D-09, D-32 | `search_page_2_sends_page_number_2`, `type_recherche_follows_match` |
| D-10 | `oauth_*_maps_to_*`, `404_on_search_is_config_not_not_found` |
| D-11, D-23, D-24 | tests de corps, `dto_parses_every_fixture`, fuzz |
| D-12, E-20 | `api_401_*`, `401_then_503_then_200_*`, `429_*`, `500_twice_maps_to_upstream_not_retryable`, `concurrent_calls_share_one_token_refresh` |
| D-13 | `rate_limiter_spaces_requests`, `concurrency_cap_is_respected` |
| D-14 | `tool_deadline_caps_retries`, `deadline_from_task_local_is_used` |
| D-20, D-21, D-25, D-26, D-28, D-30 | tests de contrat et snapshots |
| D-35 | méta-données des fixtures, tests de mojibake |
| D-41 | `oversized_body_is_rejected`, `gzip_body_is_limited_after_decompression` |
| D-43 | `user_agent_contains_crate_version` |
| D-46 | `piste_request_span_fields` : spans `piste_request` (un par tentative) et `oauth_token` de 02 §11.1, vérifiés par le subscriber de test |

---

## 14. Sécurité

### 14.1 Secrets

- `client_secret` et jeton en `SecretString`, exposés à deux endroits seulement (§3.6) ; en-tête `Authorization` marqué sensible ; `Debug` de `PisteConfig` expurgé ; jamais écrits sur disque, ni dans le cache, ni dans l'audit, ni dans les fixtures, ni dans les journaux (P-026).
- Mode HTTP partagé (M8, Q4) : le jeton et le secret ne sont jamais renvoyés à un client MCP ; aucune transmission de jeton entrant vers PISTE (00 §6).
- Une compromission (« surconsommation », « publication des identifiants ») impose de réinitialiser les identifiants sur PISTE (CGU 2022, recherche 05 §4.1). La documentation utilisateur (08) le rappelle.

### 14.2 TLS

- rustls uniquement, avec le fournisseur choisi au spike TLS (tâche 0 de M2) (§4.1) et la vérification par `rustls-platform-verifier` (magasin du système) ; TLS 1.2 minimum (défaut de rustls) ; `https_only`.
- Jamais `danger_accept_invalid_certs` ni vérificateur personnalisé. Un proxy d'entreprise qui intercepte TLS fonctionne si sa racine est installée dans le magasin du système. Une image OCI doit embarquer les certificats CA (08).

### 14.3 Aucune URL pilotée à distance

- Les URLs ne viennent que de `LF_PISTE_ENV` et des surcharges locales validées (§2.3). Pas de manifeste, pas de découverte, pas de redirection suivie (ADR-0005, corrige D-04).
- Aucune URL contenue dans une réponse (`lienJo`, `urlCC`, `attachementUrl`, `href` des contenus KALI, `url` d'ACCO) n'est jamais appelée. Elles ne servent que de données.
- Les chemins appelés sont des constantes contrôlées par le test de dérive du §7.1. Les identifiants ne vont que dans le **corps** JSON, jamais dans le chemin. Un identifiant hostile ne peut donc pas modifier la cible de la requête (recherche 02 D-31).

### 14.4 Limites de taille

| Limite | Valeur | Où |
|---|---|---|
| Corps de réponse (décompressé) | 16 Mio | §4.3 |
| Profondeur JSON | défaut de serde_json | §8.5 |
| Profondeur de TOC | 64 | domaine (03 §1.3) |
| Requête de recherche | 500 caractères | domaine (`QueryText`) |
| Message d'erreur PISTE recopié | 200 caractères | §11.2 |
| `Retry-After` honoré | 30 s, et toujours ≤ reste du délai | §5.2 |
| Mémos | 1 024 et ~150 entrées | §12.3 |
| Requêtes en vol | 4 | §5.5 |

### 14.5 Données personnelles et journaux

Les requêtes de recherche peuvent révéler l'activité d'un juriste : elles ne sont jamais journalisées (§12.5). Les champs nominatifs des décisions (présidents, avocats, rapporteurs) ne sont pas désérialisés (§8.3). Les contenus `dereferenced` et `inap` relèvent d'une décision d'exposition prise dans 05 et 08 (recherche 05 §4.3 point 6).

---

## 15. Livrables et critères d'acceptation par jalon

**Tâche 0 de M2 — Spikes (part de ce crate ; ex-M0, 00 §11.1)**

- [ ] Spike TLS : options A et B compilées pour les 7 cibles cargo-dist ; poignée de main TLS testée ; choix consigné dans ADR-0013 et dans 09 ([V-02 · tls-fournisseur-cross]).
- [ ] Spike proxy : prise en compte de `HTTPS_PROXY` et `NO_PROXY` avec les features retenues ([V-03 · reqwest-proxy-env]).
- [ ] Squelette du crate compilé ; une dépendance du §1.4 n'y est déclarée que lorsqu'un code l'emploie (aucune déclaration d'avance, 00 §2), `cargo machete` vert ; `make arch` (`cargo xtask check-arch`, `xtask/src/arch.rs`) vert (pas de `rmcp`, pas d'autre adapter, pas de dépendance directe à `url`) ; la règle R2 et la fermeture transitive arrivent en M2 avec `lf-application` ; dépendances ajoutées selon la règle d'âge de 14 jours à la date de leur PR (§1.4, 00 §2, AM-26).
- [ ] API de `backon` 1.6 et de `governor` 0.10.4 confirmées à la compilation ([V-06 · backon-api], [V-07 · governor-api]) ; `backon` sans `tokio-sleep` si le `Sleeper` du crate suffit.
- [ ] Lecture de `lf_application::deadline::Deadline` (`tokio::task_local!`) depuis l'adapter, vérifiée par le spike `task_local!` du jalon qui crée `lf-application` (00 §11.1).

**M2 — Adapter PISTE (v0.1)**

- [ ] `PisteConfig` et validation du §2.3 (`CustomHost`, `EnvMismatch` sans déduction) ; variables du §2.2 avec bornes et défauts ; démarrage sans réseau ; mode dégradé : identifiants absents ou configuration invalide ⇒ `Config` sans E/S.
- [ ] `TokenManager` mono-vol, renouvellement anticipé, générations, annulation sûre ; classification du §3.5.
- [ ] Client unique du §4.2 ; lecture bornée ; décodage strict ; réparation du mojibake (§10) avec compteur et `warn`, interrupteur `LF_PISTE_MOJIBAKE_REPAIR`.
- [ ] Politique de réessai du §5.2 (2 tentatives sur 500, `Retry-After` en secondes ou en date HTTP rapportée à `Date`), respect du délai global du §5.3 (`Deadline` converti en `CallDeadline`), `RateGate` du §5.4 et §5.5.
- [ ] Les 7 endpoints v0.1 du §7.1 ; payload `/search` du §6 (`singleDate` à midi Paris, facettes de date, `typePagination` et `PaginationMemo`, gardes défensives `InvalidRequest { status: None }`).
- [ ] V-66 et V-68 (`live_unknown_ids_classification`), V-76 (`live_singledate_noon_paris_semantics`) et V-96 (`live_lawdecree_on_code_size`), **bloquantes** (00 §11, 09 §2.18) : résultats consignés dans 09 ; décision sur `tableMatieres` en v0.1 (§7.3) et sur le moyen de reconnaître un code (§16, question 13).
- [ ] DTO du §8.3 (v0.1) ; mapping du §9 vers les entités de 03.
- [ ] Classification « introuvable » du §11 rendue définitive par les résultats de `live_unknown_ids_classification` (V-66, V-68).
- [ ] Tous les tests des §13.2 et §13.3 verts sous Linux, macOS et Windows (la matrice démarre en M2), sans réseau ; `make cov` à 100 % (glue HTTP irréductible exclue nommément).
- [ ] `cargo xtask record-fixture` opérationnel ; plan de lot exécuté en sandbox (Q3) ; résultats reportés dans le registre V-xx.
- [ ] Smoke tests live du §13.5 (partie v0.1) exécutés au moins une fois en sandbox ; résultats consignés dans 09. En production, sondes manuelles en lecture seule seulement (Q3).

**M5 — Économie de contexte**

- [ ] Pagination de la recherche vérifiée en direct : `typePagination` page 2 ([V-82 · typepagination-page2] : le mémo suffit, ou paramètre opaque `pagination` en classe « ajout ») et fenêtre profonde ([V-83 · pagination-profonde] : confirmation ou ajustement de `REACHABLE_RESULTS`, 03 §4.1).
- [ ] Borne de corps réexaminée sur les fixtures recapturées ([V-75 · taille-max-textes]) ; toute modification amende 00 §8.

**M6 — Citations**

- [ ] Endpoints M6 du §7.1 ; `find_article` et `find_by_number` (§7.2), dont la facette `ECLI` et les deux endpoints ELI ; `list_codes()` et `CodeTable` chargées paresseusement.
- [ ] DTO M6 ; scénarios wiremock pour chaque endpoint ; smoke tests M6.

**M7 — Navigation**

- [ ] Endpoints M7 du §7.1 ; `article_versions`, `related_links` ; surface JORF tranchée à l'ouverture de M7 (`fond = JORF`, `/consult/jorf` ou hits limités à `citation` + `source_url`, V-135).
- [ ] `fr_search_all` : plafond de concurrence vérifié (`search_all_respects_concurrency_cap`).

---

## 16. Questions ouvertes propres à l'adapter

Elles sont reportées dans [09-risques-et-questions.md](09-risques-et-questions.md). Les questions communes Q1 à Q17 de 00 §13 restent la référence (Q1 et Q2 décidées le 2026-10-16, C6 ; Q16 typographie des citations, décidée le 2026-10-15 ; Q17 URL `/codes/article_lc/` d'un article de loi, à vérifier en M2, V-141) : Q3 conditionne les tests live et les fixtures de production, Q4 le partage des quotas en mode HTTP, Q10 le mode de recherche par défaut. La numérotation des questions 1 à 10 de cette section est conservée, car d'autres documents la citent ; elles sont toutes tranchées par la révision 2 de 00.

1. **`singleDate`** — *tranchée* (00 §8, AM-02) : midi Europe/Paris (§6.3), confirmé par V-76, bloquante en M2.
2. **Variables de configuration** — *tranchée* (00 §1, AM-05) : `LF_PISTE_RPS`, `LF_PISTE_BURST`, `LF_PISTE_CONCURRENCY`, `LF_PISTE_MOJIBAKE_REPAIR`, avec bornes et défauts, hors `server.json` et MCPB (§2.2).
3. **500** — *tranchée* (00 §8, AM-03) : 2 tentatives au plus, puis `Upstream { status: 500, retryable: false }` ; retour à 3 tentatives si V-69 ([V-69 · 500-requete-invalide]) montre des 500 transitoires fréquents (§5.1, §5.2).
4. **Temps restant** — *tranchée* (00 §3, AM-07) : `Deadline` en variable locale de tâche posée par `Audited`, lue par l'adapter comme durée restante et convertie dans `MonoClock` (§5.3).
5. **Rendu de `InvalidRequest`** — *tranchée* (00 §5, AM-28) : toujours `upstream_error` avec le détail PISTE nettoyé, jamais `invalid_arg` ; les contraintes imputables à l'entrée sont validées avant le port (§11.2 règle 3).
6. **Plafond de pagination profonde** — *tranchée* (00 §7, AM-04) : `page × page_size > 10 000` rejeté en `invalid_arg` par le cas d'usage ; V-83 (M5) confirme la fenêtre (§6.7).
7. **`/suggest`, ECLI, JORF** — *tranchée* (00 §8, AM-29) : `/suggest` retiré ; facette `ECLI` (JURI, CETAT) et ELI cités en M6 (§7.2) ; consultation d'un `JORFTEXT` tranchée à l'ouverture de M7 (question 12).
8. **`list_codes`** — *tranchée* (00 §3) : codes abrogés compris, avec leur `etat` ; méthode du port dès M6, chargée paresseusement.
9. **`check-config --online`** — *tranchée* (02 §8.1, 08) : option explicite, une seule requête token, aucun appel à l'API (§2.4).
10. **Enregistrement de fixtures en production** — *adoptée par défaut, à confirmer par l'utilisateur* (Q3, AM-40) : sandbox par défaut ; en production, sondes en lecture seule et fixtures de données publiques marquées `env = production`, lancées à la main par le mainteneur avant chaque étiquette (≤ 1 requête/s) ; jamais d'identifiants de production en CI (§13.4, §13.5).

Questions 11 à 13 (la question 11 est tranchée depuis ; 12 et 13 restent ouvertes) :

11. **Requêtes ECLI et ELI au port** — *tranchée* (annexe B §6.6) : variantes `NumberKind::Ecli` (fonds `Juri`, `Cetat`) et `NumberKind::Eli` (fond `Loda`) de `NumberQuery`, constructeur `NumberQuery::new(fond, kind, value, at)` et `with_within` (§7.2). L'adapter n'en dépend que par le `typeChamp`/la facette et l'endpoint.
12. **Surface JORF** : `/consult/jorf` (avec bascule vers LODA si `hasLoda = true`) ou hits limités à `citation` + `source_url` ; à l'ouverture de M7, avec la valeur `JORF` des enums et les instructions (00 §4, §8 ; V-135).
13. **Reconnaître un code avant `get_act`** : si V-96 impose `tableMatieres` en v0.1, comment l'adapter sait-il, avant l'appel, qu'un `LEGITEXT` est un code (table des codes chargée paresseusement, ce qui avancerait `/list/code` de M6 à M2 et amenderait 00 §8, ou autre critère) ? À décider en M2 avec le résultat de V-96 (§7.3).

---

## Points NON VÉRIFIÉS de ce document

Index local des étiquettes employées ci-dessus ; le registre qui fait foi (numéros V-xx, jalons, caractère bloquant) est celui de [09-risques-et-questions.md](09-risques-et-questions.md) §2.

| Étiquette | Vérification à faire |
|---|---|
| [V-62 · quota-piste-prod] | Valeurs des quotas PISTE (sandbox et production), granularité (par application, par méthode), pour calibrer `governor` (V-62). |
| [V-59 · oauth-production-live] | L'endpoint token et la base API de production répondent comme documenté avec une application de production. |
| [V-61 · sandbox-prod-donnees] | Ampleur des écarts de données entre sandbox et production. |
| [V-60 · oauth-reponses] | Statuts et corps réels des erreurs de l'endpoint token (dont le statut, 400 ou 401, de `invalid_client`) ; valeurs de `expires_in` et `token_type`. |
| [V-64 · 401-sous-throttling] | PISTE renvoie-t-il `401 invalid_client` pendant une limitation de débit ? |
| [V-02 · tls-fournisseur-cross] | Exigences de compilation d'`aws-lc-sys` par cible ; installation du fournisseur `ring` avec reqwest 0.13 `rustls-no-provider`. |
| [V-03 · reqwest-proxy-env] | Prise en compte de `HTTPS_PROXY` et `NO_PROXY` par reqwest 0.13 avec les features retenues. |
| [V-70 · piste-gzip] | PISTE compresse-t-il les réponses (`Content-Encoding: gzip`) ? |
| [V-75 · taille-max-textes] | Tailles maximales réelles des réponses (gros textes, codes, TOC) au regard de la borne de 16 Mio. |
| [V-106 · acco-data-attachment] | Présence et taille de `data` et `attachment` dans `/consult/acco`. |
| [V-63 · retry-after-piste] | Présence et format (secondes ou date HTTP) de `Retry-After` sur les 429 et 503 de PISTE, et présence de l'en-tête `Date` qui sert à convertir la forme date (§5.2). |
| [V-69 · 500-requete-invalide] | Une facette ou un champ inconnu provoque-t-il un 500 plutôt qu'un 400 ? |
| [V-06 · backon-api] | Noms et sémantique de l'API backon 1.6 (`when`, `adjust`, `sleep`, `notify`, gigue). |
| [V-07 · governor-api] | Constructeurs de `Quota` et `RateLimiter::direct_with_clock` avec une horloge `Duration`, à la compilation. |
| [V-55 · delai-clients-mcp] | Délais imposés par Claude Code et VS Code à un appel d'outil, à comparer aux 60 s de `LF_TOOL_DEADLINE_S`. |
| [V-76 · singledate-midi-paris] | Avec `singleDate` à midi Paris (règle de 00 §8), un texte entrant en vigueur le jour J est-il retenu le jour J ? Comparaison avec minuit UTC, minuit Paris et la veille (bloquante en M2, V-76, dont c'est l'étiquette de référence dans 09 §2.19). |
| [V-17 · chrono-tz-iana] | Version de la base IANA embarquée par `chrono-tz` 0.10.4 ; reproduction des vecteurs du §6.3 (dont 1804-03-21). |
| [V-79 · mode-all-semantique] | Sens de `TOUS_LES_MOTS_DANS_UN_CHAMP` avec `typeChamp = ALL`. |
| [V-78 · filtres-dates-effet] | Effet réel des facettes de date du §6.5, bornes incluses, borne lointaine, combinaison avec `DATE_VERSION`. |
| [V-82 · typepagination-page2] | Effet de `DEFAUT` ou `ARTICLE` en page 2 (CODE) et cohérence de `has_more`. |
| [V-83 · pagination-profonde] | Fenêtre maximale de résultats accessibles par `/search` (10 000 ?) et réponse au-delà. |
| [V-80 · syntaxe-moteur-valeur] | Syntaxe interprétée dans `valeur` (guillemets, jokers, `*`). |
| [V-120 · recherche-exacte-par-numero] | Comportement des recherches `EXACTE` sur `NUM`, `NUM_ARTICLE`, `NUM_AFFAIRE`, `NUM_DEC`, `NUM_DELIB`, `IDCC` (casse, tirets, zéros). |
| [V-121 · nom-code-filtre] | Le filtre `NOM_CODE` (ou `TEXT_NOM_CODE`) sur `CODE_DATE` provoque-t-il un 500 ? |
| [V-118 · idandnum-comportement] | Comportement de `/consult/getArticleWithIdAndNum` (forme de `num`, réponse sans article, versions différées). |
| [V-122 · endpoints-m6-reponses] | Formes réelles des réponses de `getJoWithNor`, `kaliContIdcc` et `/list/code`. |
| [V-135 · endpoints-m7-reponses] | Formes et tailles réelles des réponses de `tableMatieres`, `canonicalArticleVersion`, `getArticleByCid` et `relatedLinksArticle` (`/suggest` retiré). |
| [V-128 · ecli-filtre-juri-cetat] | (B, C) Effet du filtre par la facette `ECLI` sur JURI et CETAT avec une valeur exacte (§7.2). |
| [V-133 · eli-redirection-endpoints] | (B, C) Comportement de `getArticleWithIdEliOrAlias` et `eliAndAliasRedirectionTexte` pour un ELI cité (§7.2). |
| [V-136 · related-links-forme] | (03) Forme et taille de `relatedLinksArticle` ; prise en charge des `KALIARTI` (§7.3). |
| [V-109 · sommaire-semantique-par-fond] | (03) Sens de `resumePrincipal` et `abstrats` (titrage ou analyse) en JURI et en CETAT, pour `OfficialSummary` (§9.2). |
| [V-71 · json-echappements-unicode] | PISTE sert-il les caractères non ASCII bruts ou échappés en `\uXXXX` (angle mort de la réparation au niveau du corps) ? |
| [V-10 · serde-json-recursion] | Valeur exacte de la limite de récursion par défaut de serde_json. |
| [V-68 · introuvable-par-endpoint] | Réponse de chaque consultation (CETATEXT et CONSTEXT compris : seul JURITEXT a été sondé en direct, par pylegifrance) à un identifiant bien formé mais inexistant (bloquante en M2, V-68). |
| [V-66 · forme-404-piste] | Forme d'un 404 PISTE (routage ou ressource), pour la règle du §11.2 (bloquante en M2, V-66). |
| [V-67 · corps-erreur-piste] | Forme générale des corps d'erreur de l'API (champs `error`, `status`, `message`). |
| [V-65 · forme-403-piste] | (02) Sens exact d'un 403 de l'API Légifrance (application non abonnée ou autre cause). |
| [V-19 · wiremock-temps-pause] | Interaction entre une horloge `tokio` gelée et les E/S réelles de wiremock (motive le choix de `ManualClock`). |
| [V-72 · mojibake-origine] | (03) Le mojibake vient-il de la capture ou de l'API ? |
| [V-96 · lawdecree-sur-code] | (03) Comportement, taille et durée de `/consult/lawDecree` avec le `LEGITEXT` d'un code ; bloquante en M2 (V-96) : au-delà de 4 Mio ou 10 s, `tableMatieres` dès v0.1 (§7.3). |
| [V-90 · texte-html-getarticle-juri] | (03) Présence de `texteHtml` sur `/consult/getArticle` et sur `/consult/juri` pour JURI. |
| [V-100 · convention-horaire-timestamps] | (03) Tous les horodatages de date sont-ils à minuit UTC ? |
| [V-81 · longueur-max-query] | (03) Longueur maximale de `query` acceptée par `/search`. |
| [V-119 · num-article-forme-api] | (03) Forme attendue par `NUM_ARTICLE`. |

---

## Sources

**Plan et décisions**
- [00-decisions.md](00-decisions.md) (révision 2) : §0 (documents propriétaires), §1 (variables, surcharges, mode dégradé), §2 (crates et dépendances, `chrono-tz`, `xtask`), §3 (ports, délai global `Deadline`), §5 (erreurs, `InvalidRequest`, 403), §6 (démarrage sans réseau), §7 (paramètres et règles de `fr_search`, `PaginationMemo`), §8 (adapter PISTE), §9 (cache), §10 (tests, scripts versionnés, tests live et fixtures), §11 (jalons), §11.1 (écarts et reports), §13 (Q1 à Q17), §14 (amendements AM-02 à AM-07, AM-16, AM-26 à AM-29, AM-31, AM-36, AM-40 ; contrôle final du 2026-10-16 : versions de référence, dépendances des squelettes, bloquantes de M2).
- [02-architecture.md](02-architecture.md) : §2.2 (`clippy.toml`), §3.0 (déclarations de workspace), §3.2 (`lf-application`, `Deadline`), §3.3 (fiche du crate : `PisteCorpus::new` sans E/S, `check_credentials`, `mojibake_repairs`, `RateQuota`, feature `record`), §3.8 (`xtask`), §5.1 (`LegalCorpus`, `CorpusError`, `find_by_number(&NumberQuery)`), §7.1 (délai global), §7.2 (clés et TTL du cache), §8.1-§8.2 (CLI, variables), §9.2-§9.3 (correspondance des erreurs, messages), §11.1 (spans).
- [03-domaine.md](03-domaine.md) : §1.5–§1.6 (frontières, `repair_latin1_mojibake`), §2.4 (`VersionedId`), §3.1 (`Fond`, `PaginationMode`), §3.3 (`status_of`), §4 (entités ; §4.1 `SearchQuery::new` et ses rejets ; §4.8 `PisteEnvironment` ; §4.11 `NumberQuery`, `CodeListing`, `RelatedLinks`), §6 (dates : `CivilDate`, midi Paris), §7.1 (ordre mojibake/NBSP), §9 (erreurs), étiquettes partagées.
- [annexes/B-citations.md](annexes/B-citations.md) §6.6 (`NumberQuery`, `NumberKind::Ecli` et `Eli`, couples autorisés), §10.1 ; [annexes/C-api-legifrance.md](annexes/C-api-legifrance.md) §2.3, §3.7 (chemins ELI, `/consult/jorf`, facette `ECLI`) ; [05-outils-mcp.md](05-outils-mcp.md) §5.11, §7.2-§7.3 (liens, gabarits) ; [06-qualite.md](06-qualite.md) §1.2 (PT5, `.gitattributes`), §1.4, §2.2, §7.5 (fichiers de test, tests de `lf-application`, `live.yml`).
- Scripts versionnés (00 §10) : `docs/plan/annexes/donnees/scripts/dto_repair_check.py` (SHA-256 `ef5c1c80c82a063a407ba9134d3afc3e5526bc659e5650810b38a3096d9d0530`), relancé par ce document sur les 16 fixtures (Python 3.13, `python3 -I`).

**Rapports de recherche** (`docs/recherche/`)
- [01-parite.md](../recherche/01-parite.md) : §2.4–§2.6 (erreurs, scénarios S1–S17), §3.1–§3.7 (payloads, corps des consultations), §6.1–§6.5 (client de la référence), §11.4–§11.7 (tests, golden, smoke), §13 (fixtures), §14 (E-01 à E-36), §15 (P-014, P-020 à P-026, P-030 à P-039, P-060), §16.
- [02-defauts.md](../recherche/02-defauts.md) : §2.a–§2.d ; §3.3 (D-04, D-05, D-18, D-39) ; §3.4 (D-06, D-10, D-11, D-12, D-13, D-14, D-41) ; §3.5 (D-07, D-08) ; §3.6 (D-19 à D-26) ; §3.11 (D-35, D-36, D-37) ; D-40, D-43, D-46 ; §5 (mojibake) ; §6 (matrice de tests) ; §7.
- [03-dto.md](../recherche/03-dto.md) : §2 (fiabilité des fixtures), §3 (payloads, identifiant inconnu), §4–§10 (formes des réponses), §11 (anomalies, dont §11.1 mojibake), §12 (DTO, helpers, mapping), §14–§15.
- [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) : §4.1 (versions de reqwest, secrecy, governor, backon, wiremock, rustls, aws-lc-rs, serde_path_to_error), §4.2 (reqwest 0.13 et TLS), §4.3, §5.3, §6 points 5, 6, 8.
- [05-piste-dila.md](../recherche/05-piste-dila.md) : §0 (Swagger 2.4.2, copies sandbox et prod identiques), §1.1–§1.14 (catalogue des endpoints, pièges de données), §2 (options de recherche, facettes par fond, pagination), §3 (OAuth, sandbox/production, quotas, codes d'erreur), §4.1–§4.3 (CGU), §5.1, §7, §8.

**Référence Python** (`matematicsolutions/fr-eli-mcp@d3b5b13`, lecture seule)
- `src/fr_eli_mcp/client.py:28-35` (URLs par défaut, délais, User-Agent, statuts réessayés, marge de 60 s), `:42-58` (variables), `:61-99` (`TokenManager`), `:117-142` (client par appel), `:149-183` (boucle `_post`), `:187-208` (endpoints et corps).
- `src/fr_eli_mcp/server.py:91-100` (`_FOND_MAP`), `:110-122` (`_map_upstream`), `:125-136` (`_today`, `_now_ms`), `:133-156` (payload `/search`).
- `DISCOVERY.md:9-13` (OAuth, scope, production), `:22-33` (`DATE_VERSION` requis), `:78-82` (`/consult/juri` pour les trois juridictions) ; `SOURCES.md:12` (identifiants sandbox refusés en production par la valeur `invalid_client`, sans statut ni corps relevés ; Judilibre), `:14-15` (KALIARTI sur `getArticle`, ACCO).
- Fixtures `tests/fixtures/*.json` (16 fichiers), dont search_code.json (`typePagination: "ARTICLE"`), consult_juri.json, consult_loda.json, consult_article.json (mojibake en caractères bruts, aucun `\uXXXX` : contrôle de ce document).

**Documentation officielle DILA** (copies archivées dans `pylegifrance/docs/raw/legifrance/`, via la recherche 05)
- Swagger 2.4.2 `legifrance.json` : `paths` des 16 chemins de la version initiale du §7.1 (dont `/suggest`, retiré depuis) et `definitions` (`SearchRequestDTO`, `RechercheSpecifiqueDTO`, `ChampDTO`, `CritereDTO`, `FiltreDTO`, `DatePeriod`, `LawDecreeConsultRequest`, `ArticleRequest`, `JuriConsultRequest`, `CnilConsultRequest`, `KaliTextConsultRequest`, `AccoConsultRequest`, `ArticleConsultWithIdAndNum`, `JorfConsultWithNorRequest`, `KaliContConsultIdccRequest`, `CodeListRequest`, `CodeListResult`, `ArticleCidRequest`, `ArticleVersion`, `RelatedLinksArticleRequest`, `LiensRelatifsDTO`, `LegiSommaireConsultRequest`, `SuggestSupplyRequest`, `SuggestValue`, `ConsultKaliContResponse`), `info.description`, `securityDefinitions` (vérifiés par script dans ce document).
- `exemples-d-utilisation-de-l-api.md:107-163` (OAuth, en-têtes), `:221`, `:497-570` (article d'une loi par `NUM_ARTICLE`, `typePagination: "ARTICLE"`), `:535-553`, `:815`, `:1011-1019`, `:1027`, `:1045-1119`, `:1089-1097`, `:1209`.
- `description-des-tris-et-filtres-de-l-api.md:51-52, 78, 99-112, 154-165, 178-186, 206-216, 229, 249-265, 277-288, 301-307` (facettes et tris par fond).
- `cgu-legifrance-api.md:171-183, 187-202, 212-231` (clés, disponibilité, quotas).

**Code tiers lu** (clones en lecture seule)
- reqwest 0.13.5 (https://github.com/seanmonstar/reqwest) : `Cargo.toml:39-80` (features), `src/lib.rs:205-217`, `src/async_impl/client.rs:1392-1478` (redirections, réessais, proxy, délais), `:2262` (`https_only`), `src/async_impl/request.rs:273-299` (`bearer_auth` sensible, délai par requête), `src/async_impl/response.rs:90, 290-351` (`content_length`, `chunk`), `src/error.rs:91-201` (`without_url`, `is_timeout`), `src/redirect.rs:155-165, 239`, `src/retry.rs:1-104`.
- governor 0.10.4 (https://github.com/boinkor-net/governor, commit 9f3a79d) : `src/state/direct.rs:43-58, 73`, `src/state/direct/future.rs:11, 30-56` (`until_ready` sur `futures_timer::Delay`), `src/clock.rs:55-117` (traits `Clock`, `Reference`, implémentation pour `Duration`).
- pylegifrance (https://github.com/rdassignies/pylegifrance, d8e0e59) : `pylegifrance/fonds/juri.py:116-135, 760-840` (marqueur « L'expression à valider est fausse » ; sonde live du 2026-04-13 sur des `JURITEXT` seulement), `tests/unit/fonds/test_juri_verification_endpoints.py:104-121` (forme du corps 400, réponse simulée) ; `tests/integration/fonds/juri/test_verification_endpoints.py:57-59` (sonde `JURITEXT000000000000` et `JURITEXT999999999999`). Lecture rapportée par la revue du plan ; non relue dans cette révision (dépôt hors d'atteinte du réseau du conteneur).
- Index crates.io (copies `index04/*.jsonl`, 2026-10-15) : backon 1.6.0 (features), governor 0.10.4, secrecy 0.10.3, serde_path_to_error 0.1.20, reqwest 0.13.5, rustls 0.23.46, wiremock 0.6.5.

**Calculs de ce document** : vecteurs de midi Europe/Paris en epoch ms (Python 3.13 `zoneinfo`, base IANA ; identiques à ceux de 00 §8) ; recherche de séquences `\uXXXX` dans les 16 fixtures ; décompte des séquences de mojibake par valeur et par clé (§10.1 : 252 = 235 + 17) et du faux positif de détection de consult_juri_constit.json (§10.3), avec le motif de `dto_repair_check.py` ; extraction des chemins et définitions du Swagger 2.4.2 par script (`python3 -I`).
