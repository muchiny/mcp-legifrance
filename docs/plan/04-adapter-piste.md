# 04 — Adapter PISTE (`lf-adapter-piste`) : OAuth, HTTP, résilience, requêtes, DTO, mapping

> **Statut** : plan, sans implémentation. Ce document applique [00-decisions.md](00-decisions.md), qui est normatif et l'emporte en cas de doute.
> **Jalons** : M0 (spike TLS), **M2** (tout le socle v0.1 : OAuth, client, réessais, débit, délai, DTO, mapping, mojibake, tests wiremock et fixtures, smoke tests live), M5 (pagination de la recherche vérifiée en direct), M6 (endpoints de résolution), M7 (endpoints de navigation).
> **Lecteurs** : développeurs de `lf-adapter-piste`, et de `lf-app` (composition, configuration) et `lf-adapter-cache` (clés, TTL).
> Les extraits Rust sont **non compilés, à valider en M0**. Les points non vérifiés sont marqués **NON VÉRIFIÉ** avec une étiquette `[v:…]`. Le registre `V-xx` les reprend dans [09-risques-et-questions.md](09-risques-et-questions.md).

Documents liés : [02-architecture.md](02-architecture.md) (ports, erreurs, décorateurs), [03-domaine.md](03-domaine.md) (types construits par le mapping), [05-outils-mcp.md](05-outils-mcp.md) (paramètres et rendu), [06-qualite.md](06-qualite.md) (stratégie de test et CI), [07-feuille-de-route.md](07-feuille-de-route.md) (tâches), [08-distribution.md](08-distribution.md) (cibles de compilation, images), [09-risques-et-questions.md](09-risques-et-questions.md), [annexes/A-parite.md](annexes/A-parite.md), [annexes/C-api-legifrance.md](annexes/C-api-legifrance.md), ADR-0004, ADR-0005, ADR-0007, ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0014 (index : [../adr/README.md](../adr/README.md)).

---

## 0. Résumé

- `lf-adapter-piste` fournit `PisteCorpus: LegalCorpus`, la seule implémentation du port sortant jusqu'à M9. Il traduit chaque appel du port en requêtes `POST` JSON vers l'API Légifrance 2.4.2 derrière PISTE, puis convertit la réponse en entités de `lf-domain`. Il ne connaît ni MCP, ni l'audit, ni le cache.
- **Environnements** : `LF_PISTE_ENV=sandbox|production` (défaut `sandbox`) fixe ensemble l'URL OAuth et la base de l'API. Les surcharges `LF_PISTE_OAUTH_URL` et `LF_PISTE_BASE_URL` sont validées (https, pas d'identifiants dans l'URL, cohérence d'environnement). Aucune URL ne vient d'ailleurs (ADR-0005, corrige D-04).
- **OAuth2 client_credentials** : jeton en mémoire (`SecretString`), renouvelé 60 s avant expiration, rafraîchissement mono-vol. Sur l'API, un 401 déclenche **un seul** renouvellement, puis `CorpusError::Auth`. Une erreur de l'endpoint token n'est jamais `NotFound` (corrige D-10, D-12, E-20).
- **HTTP** : un `reqwest::Client` 0.13 unique (rustls, fournisseur crypto tranché par le spike M0), connexion 10 s, requête 40 s, aucune redirection suivie, corps plafonné à 16 Mio, User-Agent versionné (corrige D-06, D-41, D-43).
- **Résilience** : `backon` (3 tentatives sur 429/500/502/503/504 et erreurs réseau, backoff exponentiel avec gigue, `Retry-After`), délai global par outil `LF_TOOL_DEADLINE_S` (60 s), `governor` (5 req/s, rafale 10, configurables) piloté par une horloge `tokio`, sémaphore de 4 requêtes en vol (corrige D-12, D-13, D-14).
- **Requêtes `/search`** : payload de la référence, `typeRecherche` choisi par `match`, `DATE_VERSION` à minuit Europe/Paris (stable sur la journée, corrige D-01), facettes de date par fond tirées de la documentation officielle, `typePagination` mémorisé et renvoyé.
- **DTO** serde minimaux et tolérants, un module par endpoint, `serde_path_to_error` pour des `Decode` lisibles. **Mapping** en fonctions pures vers les entités de 03.
- **Mojibake** : réparation tout-ou-rien au niveau du corps, avant serde et avant toute normalisation des NBSP, active par défaut, comptée et journalisée en `warn`.
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
| Décodage UTF-8 strict et réparation du mojibake | `encoding.rs` (appelle `lf_domain::text::encoding::repair_latin1_mojibake`) | D-35 (côté production) |
| Construction des corps de requête (`/search`, consultations) | `request/` | D-01, D-09, D-32 |
| DTO serde minimaux | `dto/` | D-11, D-23, D-24 |
| Mapping DTO vers entités du domaine | `map/` | D-20, D-21, D-25, D-26, D-28, D-30, D-33 |
| Classification des réponses en `CorpusError` | `classify.rs` | D-10, D-11 |
| Mémos internes (`typePagination`, table des codes en M6) | `memo.rs` | D-09 |

### 1.2 Ce que le crate ne fait pas

| Responsabilité | Crate | Raison |
|---|---|---|
| Noms d'outils, schémas, messages `[code] …`, rendu compact | `lf-adapter-mcp` | rien de MCP ici : ni `rmcp`, ni `schemars` (00 §2) |
| Journal d'audit, mesure de `duration_ms` | `lf-application` (`Audited`) + `lf-adapter-audit` | l'adapter ne journalise aucun appel d'outil (§12.4) |
| Cache des réponses | `lf-adapter-cache` (`CachedCorpus<PisteCorpus>`) | l'adapter ne met rien en cache, hors deux mémos techniques (§12.3) |
| « Aujourd'hui » à Paris | port `Clock` (`lf-app`) | l'application passe une `VersionDate` déjà résolue (03 §6.4) |
| Validation des paramètres utilisateur | `lf-domain` (constructeurs), appelé par `lf-adapter-mcp` | l'adapter reçoit des types déjà valides (`DecisionId`, `QueryText`, `PageSize`…) |
| Règles de citation, URLs Légifrance, découpage du texte | `lf-domain` | le mapping appelle les constructeurs du domaine |
| Lecture des variables d'environnement | `lf-app` | l'adapter reçoit une `PisteConfig` (§2.3) |

Règles de dépendance (00 §2) : `lf-adapter-piste` dépend de `lf-application` et de `lf-domain`, jamais d'un autre adapter. `reqwest` n'apparaît que dans ce crate (recherche 04 §5.3).

### 1.3 Méthodes du port implémentées

| Méthode de `LegalCorpus` (00 §3) | Endpoint(s) | Jalon |
|---|---|---|
| `search(&SearchQuery) -> SearchPage` | `POST /search` | M2 |
| `get_act(&LegiTextId, VersionDate) -> Act` | `POST /consult/lawDecree` (M7 : `/consult/legi/tableMatieres` pour un code, §7.3) | M2 |
| `get_article(&ArticleId) -> ArticleText` | `POST /consult/getArticle` (LEGIARTI et KALIARTI) | M2 |
| `get_decision(&DecisionId) -> Decision` | `POST /consult/juri` (JURITEXT, CONSTEXT, CETATEXT) | M2 |
| `get_deliberation(&CnilTextId) -> Deliberation` | `POST /consult/cnil` | M2 |
| `get_convention(&KaliTextId) -> Convention` | `POST /consult/kaliText` | M2 |
| `get_company_agreement(&AccoTextId) -> CompanyAgreement` | `POST /consult/acco` | M2 |
| `find_article(&CodeRef, &ArticleNum, VersionDate)` | `/consult/getArticleWithIdAndNum`, `/search` (`NUM_ARTICLE`, `EXACTE`), `/list/code` | M6 |
| `find_by_number(&NumberQuery)` | `/search` en `EXACTE` (`NUM`, `NUM_AFFAIRE`, `NUM_DEC`, `NUM_DELIB`, `IDCC`), `/consult/getJoWithNor`, `/consult/kaliContIdcc` | M6 |
| `article_versions(&ArticleId) -> Vec<ArticleVersion>` | `/search/canonicalArticleVersion`, repli `/consult/getArticleByCid` | M7 |
| `related_links(&ArticleId)` | `/consult/relatedLinksArticle` | M7 |
| `list_codes()` | `/list/code` | M7 (endpoint utilisé en interne dès M6) |

Les traits sont en async natif (RPITIT + `Send`, ADR-0012). L'implémentation utilise `async fn`. La sélection PISTE/DILA par `enum AnyCorpus` arrive en M9 (ADR-0004) et ne change rien ici.

```rust
// lf-adapter-piste/src/lib.rs — non compilé, à valider en M0
#[derive(Clone)]
pub struct PisteCorpus<K: MonoClock = TokioClock, S: Sleeper = TokioSleeper> { inner: Arc<Inner<K, S>> }

impl PisteCorpus {
    /// Aucune E/S (02 §3.3). Le client reqwest est construit au premier appel (OnceCell, mono-vol) ;
    /// un échec de construction (fournisseur TLS, racines) devient CorpusError::Config à l'appel.
    pub fn new(cfg: PisteConfig) -> Self;
}
impl<K: MonoClock, S: Sleeper> PisteCorpus<K, S> {
    pub fn environment(&self) -> PisteEnv;                         // pour le cache (namespace) et fr_coverage
    pub fn config_problem(&self) -> Option<&ConfigProblem>;        // identifiants absents, URL invalide…
    pub fn mojibake_repairs(&self) -> u64;                         // compteur (02 §3.3)
    pub fn stats(&self) -> StatsSnapshot;                          // tous les compteurs en mémoire, jamais exportés (ADR-0005)
    pub async fn check_credentials(&self) -> Result<(), CorpusError>; // `check-config --online` (02 §3.3, 08)
}
impl<K: MonoClock, S: Sleeper> LegalCorpus for PisteCorpus<K, S> { /* async fn … */ }
```

`MonoClock` (temps monotone) et `Sleeper` (attente) sont deux traits **internes** au crate, pas des ports. Ils existent pour rendre déterministes les tests de jeton, de backoff, de `Retry-After` et de débit sans geler l'horloge `tokio` (§13.1). En production : `TokioClock` (`tokio::time::Instant`) et `TokioSleeper` (`tokio::time::sleep`).

### 1.4 Dépendances

Versions relevées dans l'index crates.io le 2026-10-15 (recherche 04 §4.1). Les features sont celles de ce crate. Elles se déclarent `default-features = false` au niveau du workspace quand c'est voulu (piège d'héritage, 00 §2).

| Crate | Version | Features | Usage |
|---|---|---|---|
| `reqwest` | 0.13.5 | `json`, `form`, `rustls` **ou** `rustls-no-provider` (spike M0), `gzip`, `system-proxy` | client HTTP (§4) |
| `rustls` | 0.23.46 | seulement si `rustls-no-provider` : `ring`, `std`, `tls12`, `logging` | fournisseur crypto (§4.1) |
| `tokio` | 1.53.3 | `time`, `sync` | délais, sémaphore, verrou du jeton |
| `serde`, `serde_json` | 1.0.229, 1.0.154 | `derive` | DTO |
| `serde_path_to_error` | 0.1.20 | — | chemin JSON des erreurs `Decode` (§8.5) |
| `secrecy` | 0.10.3 | `serde` | `client_secret` et jeton (§3.6) |
| `backon` | 1.6.0 | `std`, `tokio-sleep` | réessais (§5.2) |
| `governor` | 0.10.4 | `std` (sans `quanta` ni `dashmap`) | limitation de débit (§5.4) |
| `chrono`, `chrono-tz` | 0.4.45, 0.10.4 | `std` | dates des DTO, minuit Europe/Paris pour `singleDate` (§6.3) |
| `tracing` | 0.1.44 | — | journaux sur stderr (§12.5) |
| `thiserror` | 2.0.21 | — | erreurs internes |
| `url` | 2.5.8 | — | validation de la configuration (via `reqwest::Url`) |
| dev : `wiremock`, `insta`, `proptest`, `tokio` (`macros`, `rt-multi-thread`) | 0.6.5, 1.49.0, 1.12.0 | `insta` : `json`, `redactions` | tests (§13) |

`charset` (décodage selon l'en-tête `Content-Type`) est **exclu** : le corps est toujours décodé en UTF-8 strict à partir des octets (§4.4). `http2` n'est pas activé en v0.1 : HTTP/1.1 avec réutilisation des connexions suffit à 4 requêtes en vol.

### 1.5 Organisation des modules

```text
crates/lf-adapter-piste/src/
  lib.rs          PisteCorpus, ré-exports (PisteConfig, PisteEnv, PisteEndpoints, RateQuota, StatsSnapshot)
  config.rs       PisteEnv, PisteEndpoints, PisteCredentials, PisteConfig, RateQuota, validation (§2)
  auth.rs         TokenManager, TokenLease, DTO de l'endpoint token (§3)
  http.rs         construction du client, envoi d'une tentative, lecture bornée du corps (§4)
  encoding.rs     decode_body : UTF-8 strict puis réparation du mojibake (§4.4, §10)
  classify.rs     statut + corps → issue d'une tentative (§5.1, §11)
  retry.rs        politique de réessai backon, Retry-After (§5.2)
  deadline.rs     Deadline (lecture du délai courant, §5.3)
  limit.rs        RateGate : sémaphore + governor sur horloge tokio (§5.4, §5.5)
  time.rs         MonoClock, Sleeper, TokioClock, TokioSleeper (+ ManualClock, RecordingSleeper en cfg(test))
  request/        search.rs (SearchRequestDto), consult.rs (corps des consultations), number.rs (M6)
  dto/            de.rs (helpers), search.rs, text.rs, article.rs, juri.rs, acco.rs, error.rs, oauth.rs,
                  jorf.rs, kalicont.rs, codes.rs (M6), versions.rs, links.rs, suggest.rs (M7)
  map/            dates.rs, search.rs, act.rs, article.rs, decision.rs, deliberation.rs, convention.rs,
                  agreement.rs (+ M6/M7)
  memo.rs         PaginationMemo (M2), CodeTable (M6) (§12.3)
  corpus.rs       impl LegalCorpus for PisteCorpus
crates/lf-adapter-piste/tests/
  fixtures/reference/*.json (+ .meta.toml)   16 fixtures de la référence (attribution NOTICE)
  fixtures/recorded/<endpoint>/*.json (+ .meta.toml)   fixtures enregistrées par xtask
  oauth.rs, resilience.rs, body.rs, endpoints.rs, search_payload.rs, contract_fixtures.rs, live.rs
```

### 1.6 Pipeline d'un appel

```mermaid
flowchart TD
  P["méthode du port, ex. get_decision(&DecisionId)"] --> D["Deadline::current_or(LF_TOOL_DEADLINE_S)"]
  D --> C{"configuration utilisable ?"}
  C -- non --> EC["CorpusError::Config, sans aucune E/S"]
  C -- oui --> B["corps JSON minimal (request/)"]
  B --> L["boucle de réessai backon : 3 tentatives au plus"]
  L --> G["RateGate : sémaphore (4) puis governor (5/s, rafale 10)"]
  G --> T["TokenManager::token() mono-vol"]
  T --> H["POST reqwest, timeout = min(40 s, reste du délai)"]
  H --> K{"classification §5.1"}
  K -- "401, 1re fois" --> R["invalidate(génération), nouveau jeton, renvoi immédiat"]
  R --> H
  K -- "429 / 500 / 502 / 503 / 504 / réseau" --> L
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

Les URLs de production sont documentées (Swagger) mais n'ont jamais été appelées par nous ni par la référence, qui n'avait qu'une application sandbox : **NON VÉRIFIÉ** [v:oauth-production-live]. L'ampleur de l'écart de données entre sandbox et production est **NON VÉRIFIÉ** [v:sandbox-prod-donnees]. Les valeurs numériques des quotas ne sont pas publiques : **NON VÉRIFIÉ** [v:quotas-piste].

Conséquences pour l'adapter :

- l'environnement est une donnée de premier rang : `PisteCorpus::environment()` alimente l'espace de noms du cache (00 §9, corrige D-15) et `fr_coverage` (00 §7, corrige D-18) ;
- une erreur `invalid_client` doit produire un message qui suggère un mélange d'environnements (§3.5) ;
- les fixtures enregistrées portent leur environnement dans leurs métadonnées (§13.4) : un golden sandbox n'est pas comparé à une réponse de production.

### 2.2 Variables lues (par `lf-app`)

| Variable (00 §1) | Défaut | Champ de `PisteConfig` | Remarques |
|---|---|---|---|
| `LF_PISTE_ENV` | `sandbox` | `env` | `sandbox` ou `production`, insensible à la casse ; toute autre valeur est un problème de configuration |
| `LF_PISTE_CLIENT_ID` | — | `credentials.client_id` | `trim()` ; vide = absent (parité référence client.py:51-52) |
| `LF_PISTE_CLIENT_SECRET` | — | `credentials.client_secret` | idem, enveloppé immédiatement dans `SecretString` |
| `LF_PISTE_OAUTH_URL` | URL officielle de `env` | `endpoints.oauth_url` | surcharge explicite, validée (§2.3) |
| `LF_PISTE_BASE_URL` | URL officielle de `env` | `endpoints.base_url` | surcharge explicite, validée ; `/` final retiré (parité client.py:47) |
| `LF_HTTP_TIMEOUT_S` | `40` | `request_timeout` | délai total d'une tentative HTTP (connexion comprise, corps compris) |
| `LF_TOOL_DEADLINE_S` | `60` | `tool_deadline` | délai global (§5.3) |
| `FR_ELI_CLIENT_ID`, `FR_ELI_CLIENT_SECRET`, `FR_ELI_OAUTH_URL`, `FR_ELI_BASE_URL` | — | mêmes champs | compatibilité : lues seulement si la variable `LF_*` correspondante est absente, avec un avertissement de dépréciation sur stderr ; si les deux sont définies et diffèrent, `LF_*` gagne et un avertissement le signale |
| proposées : `LF_PISTE_RPS`, `LF_PISTE_BURST`, `LF_PISTE_CONCURRENCY`, `LF_PISTE_MOJIBAKE_REPAIR` | `5`, `10`, `4`, `on` | `rate`, `max_concurrency`, `repair_mojibake` | 00 exige un débit « configurable » et une réparation « active par défaut » sans nommer de variable : noms à inscrire dans 00 §1 (voir §16) |

Le connecteur ne lit aucune configuration distante, ne télécharge aucun manifeste et n'émet aucune télémétrie (ADR-0005, corrige D-04 et D-05). La configuration est lue **une fois** au démarrage (corrige E-24 : la référence relisait l'environnement à chaque appel).

### 2.3 `PisteConfig` et validation

```rust
// lf-adapter-piste/src/config.rs — non compilé, à valider en M0
pub enum PisteEnv { Sandbox, Production }

pub struct PisteEndpoints { pub oauth_url: Url, pub base_url: Url, pub official: bool }

pub struct PisteCredentials { pub client_id: String, pub client_secret: SecretString }

pub struct RateQuota { pub per_second: NonZeroU32, pub burst: NonZeroU32 }   // nom exporté fixé par 02 §3.3

pub struct PisteConfig {
    pub env: PisteEnv,
    pub endpoints: Result<PisteEndpoints, ConfigProblem>, // un problème n'empêche pas le démarrage
    pub credentials: Option<PisteCredentials>,            // None ⇒ CorpusError::Config sans E/S (D-39)
    pub user_agent: String,                               // construit par lf-app (§4.2)
    pub connect_timeout: Duration,                        // 10 s
    pub request_timeout: Duration,                        // LF_HTTP_TIMEOUT_S, 40 s
    pub tool_deadline: Duration,                          // LF_TOOL_DEADLINE_S, 60 s
    pub rate: RateQuota,                                  // 5/s, rafale 10
    pub max_concurrency: NonZeroU16,                      // 4
    pub max_body_bytes: u64,                              // 16 Mio
    pub repair_mojibake: bool,                            // true
}
impl PisteEndpoints {
    pub fn official(env: PisteEnv) -> Self;
    pub fn with_overrides(env: PisteEnv, oauth: Option<&str>, base: Option<&str>)
        -> Result<(Self, Vec<ConfigWarning>), ConfigProblem>;
}
impl fmt::Debug for PisteConfig { /* client_secret affiché "[REDACTED]" par secrecy */ }
```

Règles de `with_overrides`, toutes testées (§13.2) :

1. L'URL se parse avec `Url::parse`. Le schéma doit être `https`. Exception : `http` est accepté si l'hôte est une boucle locale (`127.0.0.1`, `::1`, `localhost`), avec `ConfigWarning::InsecureLoopback`. Cette exception sert aux tests wiremock et aux passerelles locales.
2. Ni `userinfo`, ni requête (`?…`), ni fragment : sinon `ConfigProblem`. Une URL ne contient jamais de secret, ce qui corrige le risque de fuite par les messages d'erreur (D-40).
3. Hôte hors de la liste des quatre hôtes officiels (`sandbox-oauth.piste.gouv.fr`, `oauth.piste.gouv.fr` pour le jeton ; `sandbox-api.piste.gouv.fr`, `api.piste.gouv.fr` pour l'API) : accepté, car la surcharge est un choix explicite et local de l'utilisateur, mais `ConfigWarning::CustomHost` est affiché au démarrage, par `check-config` et dans `fr_coverage` (rendu dans 05). `official = false`.
4. Si les deux hôtes sont officiels, ils doivent désigner le **même** environnement, égal à `LF_PISTE_ENV`. Sinon `ConfigProblem::EnvMismatch`. Cela empêche d'envoyer des identifiants sandbox à la production, ou l'inverse.
5. Sur un hôte API officiel, le chemin doit être exactement `/dila/legifrance/lf-engine-app`.

L'URL de base ne change plus après le démarrage. Aucune URL renvoyée par l'API (`lienJo`, `urlCC`, `attachementUrl`, `href` des contenus KALI) n'est jamais appelée (§14.3).

### 2.4 Démarrage sans réseau ; identifiants absents

- `PisteCorpus::new` ne fait **aucun** appel réseau. Le jeton est demandé paresseusement, au premier appel d'outil (00 §6).
- Identifiants absents, `ConfigProblem` ou `LF_PISTE_ENV` invalide : le serveur démarre quand même. Chaque méthode du port renvoie immédiatement `CorpusError::Config`, sans E/S ni attente (corrige D-39). Le message garde celui de la référence avec les nouveaux noms de variables (P-014, E-34) : `PISTE credentials missing: set LF_PISTE_CLIENT_ID and LF_PISTE_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials).`
- `check-config` (sous-commande de `lf-app`, 08) affiche l'environnement, les URLs effectives, les avertissements et les problèmes, sans le secret. L'option `--online` appelle `check_credentials()` (02 §3.3) : une seule requête token, aucun appel à l'API.

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
// dto/oauth.rs — non compilé, à valider en M0
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
- Formes réelles des erreurs de l'endpoint token, statut et corps : **NON VÉRIFIÉ** [v:oauth-reponses].

### 3.3 Gestionnaire de jeton

```rust
// auth.rs — non compilé, à valider en M0
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
    /// attendent puis lisent le jeton neuf. L'attente du verrou est bornée par `deadline`.
    pub async fn token(&self, deadline: &Deadline) -> Result<TokenLease, CorpusError>;
    /// N'invalide que si le jeton courant est celui qui a reçu le 401.
    pub async fn invalidate(&self, generation: u64);
}
```

- **État** : au plus un jeton en mémoire, jamais écrit sur disque ni journalisé (P-026 ; référence client.py:10-13). Il n'est pas non plus dans `lf-adapter-cache`.
- **Renouvellement anticipé** : `refresh_at = obtenu_à + expires_in − marge`, avec `marge = min(60 s, expires_in / 2)`. `obtenu_à` est lu **avant** l'envoi de la requête, comme la référence (client.py:74, 93), ce qui reste prudent. La marge de 60 s est celle de la référence et de deux clients tiers (recherche 05 §3.1). Le plafond `expires_in / 2` évite un jeton déjà périmé si `expires_in ≤ 60`.
- **Mono-vol** : un `tokio::sync::Mutex` couvre la lecture et le rafraîchissement (même principe que l'`anyio.Lock` de la référence, client.py:70-73). Dix appels simultanés au démarrage produisent **une** requête token (P-022, test `concurrent_calls_share_one_token_refresh`).
- **Générations** : chaque jeton obtenu reçoit un numéro croissant. `invalidate(g)` ne vide l'état que si le jeton courant porte `g`. Une tâche qui reçoit un 401 avec un vieux jeton n'efface donc pas le jeton neuf obtenu entre-temps par une autre.
- **Annulation** : si le futur appelant est abandonné (délai global, annulation MCP), la requête token en cours est abandonnée et le verrou est relâché sans modifier l'état. L'appelant suivant rafraîchit. L'acquisition du verrou de `tokio` est sûre vis-à-vis de l'annulation (test `cancel_during_refresh_releases_lock`).
- La requête token passe par la même politique de réessai que l'API (429, 5xx, réseau), dans le même délai global. Elle ne passe pas par `governor`, car les quotas visent l'API Légifrance et le jeton n'est demandé qu'environ une fois par heure.

### 3.4 401 sur l'API : un seul renouvellement

Algorithme par appel du port, corrige D-12 et E-20 :

1. La tentative envoie la requête avec le jeton de génération `g`.
2. Réponse 401 et aucun renouvellement encore fait pour cet appel : `invalidate(g)`, `token()` (mono-vol), puis **renvoi immédiat**, sans backoff. Ce renvoi ne consomme pas l'une des 3 tentatives transitoires.
3. Second 401 dans le même appel : `CorpusError::Auth`, sans autre requête. Message : `PISTE rejected the access token twice; check that LF_PISTE_CLIENT_ID/SECRET belong to the {env} application and that it is subscribed to the Légifrance API.`
4. La suite `401, 503, 200` produit exactement **deux** requêtes token et trois requêtes API (la référence en demandait trois, recherche 02 D-12).

Un rapport tiers dit que PISTE renverrait parfois `401 invalid_client` pendant de courtes fenêtres de limitation : **NON VÉRIFIÉ** [v:401-sous-throttling]. Si c'est vrai, l'étape 3 produirait un `config_error` à tort. Le smoke test live (§13.5) cherche ce cas. La règle « un seul renouvellement » est maintenue (00 §8).

### 3.5 Classification des erreurs de l'endpoint token

Une erreur de l'endpoint token est un problème de configuration, jamais `NotFound` (00 §5, corrige D-10). `CorpusError::Auth` et `CorpusError::Config` sont rendus `config_error` côté MCP ; la correspondance exacte `CorpusError` → `AppError` relève de [02-architecture.md](02-architecture.md).

| Réponse de l'endpoint token | `CorpusError` | Réessai | Message (sans secret) |
|---|---|---|---|
| 200, jeton présent | — | — | — |
| 200 sans `access_token`, ou JSON illisible | `Config` | non | `PISTE OAuth returned no access_token.` |
| 400 ou 401 (`invalid_client`, `invalid_grant`, `unauthorized_client`…) | `Auth` | non | `PISTE OAuth refused the credentials ({error}). Check LF_PISTE_CLIENT_ID/SECRET and that they belong to the {env} environment (sandbox and production use different applications).` |
| 403 | `Auth` | non | `PISTE OAuth forbade the request (403).` |
| 404, 405, page HTML | `Config` | non | `PISTE OAuth endpoint not found at {scheme://host/path}; check LF_PISTE_OAUTH_URL.` |
| 429 | réessai, puis `RateLimited` | oui (§5.2) | — |
| 500, 502, 503, 504, erreur réseau | réessai, puis `Upstream` ou `Timeout` | oui | — |
| erreur DNS ou TLS (certificat refusé) | `Upstream { status: None, retryable: false }` | réseau : oui ; TLS : non | `could not reach PISTE OAuth ({kind}).` |

Le champ `error` d'`OAuthErrorDto` est repris s'il est court (≤ 64 caractères ASCII), sinon omis. `error_description` n'est jamais recopié : il pourrait contenir un fragment de la requête.

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

**Spike M0** (00 §11), à trancher avant M2 :

| Option | Features | Coût | À mesurer |
|---|---|---|---|
| A (défaut si le spike n'aboutit pas) | `reqwest/rustls` (aws-lc) | `aws-lc-sys` demande une chaîne C (`cc`, `cmake`, parfois NASM ou clang selon la cible) | compilation croisée des 7 cibles cargo-dist (Linux x86_64/aarch64 gnu+musl, macOS x86_64/aarch64, Windows x86_64 msvc), durée, taille |
| B | `reqwest/rustls-no-provider` + `rustls/ring` ; `PisteCorpus::new` appelle `rustls::crypto::ring::default_provider().install_default()` et ignore l'erreur « déjà installé » | moins d'outillage natif | mêmes mesures ; poignée de main TLS 1.2 et 1.3 vers un serveur rustls local, et vers `sandbox-api.piste.gouv.fr` si le réseau le permet (Q6) |

Les exigences exactes d'`aws-lc-sys` par cible et l'API d'installation de `ring` avec reqwest 0.13 sont **NON VÉRIFIÉ** [v:tls-fournisseur-cross] (recherche 04 §6 points 5 et 6). Le résultat est consigné dans ADR-0013 et dans le risque TLS de [09-risques-et-questions.md](09-risques-et-questions.md). Le fournisseur est installé **par l'adapter**, seul crate à dépendre de reqwest : `lf-app` n'a pas à dépendre de rustls.

### 4.2 Construction du client

Un seul `reqwest::Client`, construit une fois au premier appel (`OnceCell`, mono-vol ; `PisteCorpus::new` ne fait aucune E/S, 02 §3.3) et partagé par `TokenManager` et les appels API (pool de connexions par hôte). Un échec de construction (fournisseur TLS, racines) devient `CorpusError::Config`. Cela corrige D-06 et E-22 : la référence recréait un client par appel d'outil.

| Réglage | Valeur | Source / raison |
|---|---|---|
| `connect_timeout` | 10 s | 00 §8 ; parité P-025 (référence client.py:30) |
| `timeout` (total par tentative, connexion et corps compris) | `LF_HTTP_TIMEOUT_S` (40 s), réduit tentative par tentative à `min(40 s, reste du délai global)` via `RequestBuilder::timeout` | reqwest : « applied from when the request starts connecting until the response body has finished » (`RQ:src/async_impl/client.rs:1444-1453`, `request.rs:289-299`) |
| `user_agent` | `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-ligitfrance)`, chaîne fournie par `lf-app` avec `env!("CARGO_PKG_VERSION")` du binaire | 00 §8 ; corrige D-43 et E-08. L'URL suivra le renommage du dépôt (Q1) |
| en-tête par défaut | `Accept: application/json` | exemple officiel (`exemples…md:155-163`) ; `Content-Type: application/json` est posé par `.json()` |
| `redirect` | `redirect::Policy::none()` | un 3xx devient `Upstream { status, retryable: false }`. Parité : la référence ne suivait pas les redirections (recherche 01 §6.3). Un `POST` authentifié n'a aucune raison d'être redirigé. reqwest suit 10 redirections par défaut (`RQ:src/redirect.rs:160-165`) |
| `https_only` | `true`, sauf `ConfigWarning::InsecureLoopback` (tests) | `RQ:src/async_impl/client.rs:2262` |
| `gzip` | activé (feature `gzip`) : envoie `Accept-Encoding: gzip`, décompression transparente | réponses JSON très compressibles ; prise en charge par PISTE **NON VÉRIFIÉ** [v:piste-gzip]. La borne de taille s'applique aux octets **décompressés** (§4.3) |
| `retry` | défaut de reqwest conservé : seuls les NACK de protocole sûrs sont rejoués (`RQ:src/async_impl/client.rs:1405-1414`, `src/retry.rs:9-16`). Toute la politique applicative est dans `backon` (§5.2) | évite deux couches de réessai concurrentes |
| proxy | défaut conservé (proxy système, feature `system-proxy` : réglages Windows et macOS, `RQ:src/lib.rs:216`) | réseaux d'entreprise ; prise en compte de `HTTPS_PROXY` et `NO_PROXY` selon les features retenues : **NON VÉRIFIÉ** [v:reqwest-proxy-env], à tester au spike M0 |
| `pool_idle_timeout` | défaut (90 s) | une connexion fermée par la passerelle PISTE produit une erreur réseau, rejouée par la politique §5.2 |
| `danger_accept_invalid_certs` | **jamais** | §14.2 |

### 4.3 Lecture bornée du corps

Corrige D-41 et D-11.

1. Si `Response::content_length()` est connu et dépasse `max_body_bytes` (16 Mio), refus immédiat.
2. Sinon, lecture par `Response::chunk()` (`RQ:src/async_impl/response.rs:310`) dans un `Vec<u8>`, avec contrôle du cumul à chaque morceau. Les morceaux sont décompressés, si bien qu'une bombe gzip est arrêtée à 16 Mio.
3. Dépassement : `CorpusError::Decode { detail }`, avec un `detail` de la forme `response too large (> 16 MiB) on /consult/acco`. Pas de réessai.

Choix de 16 Mio : la plus grosse fixture fait 42 Ko (search_kali.json, recherche 03 §2), mais une réponse `/consult/lawDecree` sur un gros texte ou `/consult/acco` avec son champ `data` (`.docx` en base64, jamais désérialisé mais tout de même lu) peut être bien plus volumineuse. Tailles réelles **NON VÉRIFIÉ** [v:taille-max-textes] ; présence et taille de `data` et `attachment` sur `/consult/acco` **NON VÉRIFIÉ** [v:acco-data-attachment]. La borne est une constante, ajustée en M5 sur les fixtures recapturées.

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

| Issue d'une tentative | Réessai (≤ 3 tentatives) | Résultat final | Source |
|---|---|---|---|
| 2xx avec corps | — | DTO, puis entité ; corps vide : `NotFound` sur une consultation, `Decode` sur `/search` | Swagger : 200 et 201 déclarés pour chaque opération (recherche 05 §1.1) |
| 3xx | non | `Upstream { status, retryable: false }` | §4.2 |
| 400 sur une consultation avec le marqueur « introuvable » | non | `NotFound` | §11 |
| 400, autres cas | non | `InvalidRequest { status: 400, detail }` (message PISTE nettoyé, ≤ 200 caractères) ; rendu `upstream_error` (02 §9, parité S10) | — |
| 401 (API) | renouvellement unique (§3.4), hors budget | second 401 : `Auth` | 00 §8 |
| 403 | non | `Auth` (application non abonnée à l'API Légifrance) | recherche 05 §3.4 (constaté sur Judilibre avec un jeton Légifrance ; sens exact d'un 403 de l'API Légifrance **NON VÉRIFIÉ** [v:forme-403-piste], étiquette de 02) |
| 404 | non | `NotFound` ou `Config` selon §11.2 | D-10 |
| 405 | non | `Config` | — |
| autre 4xx | non | `Upstream { status, retryable: false }` | — |
| 429 | **oui**, avec `Retry-After` | `RateLimited { retry_after }` | 00 §8 |
| 500 | **oui** | `Upstream { status: 500, retryable: true }` (02 §9.2, ligne 17) | 00 §8 ; voir §16 (un 500 répété est peut-être déterministe) |
| 502, 503, 504 | **oui**, `Retry-After` respecté s'il est présent | `Upstream { status, retryable: true }` | 00 §8 |
| autre 5xx | non | `Upstream { status, retryable: false }` | — |
| erreur réseau (connexion, réinitialisation, corps interrompu) | **oui** | `Upstream { status: None, retryable: true }` | 00 §8 |
| délai de tentative dépassé (`reqwest::Error::is_timeout`) | **oui** | `Timeout` | — |
| délai global dépassé | non | `Timeout` | §5.3 |
| corps illisible (UTF-8, JSON, trop gros) | non | `Decode` | §4.3, §8.5 |
| incohérence de domaine (`DomainError` non utilisateur, ex. `TocTooDeep`) | non | `Decode` | 03 §9 |

`Upstream.status` est un `Option<u16>` (`None` pour une erreur réseau). Le `Display` de chaque `CorpusError` est en anglais, court, sans URL complète ni secret. Il reprend le format de la parité, par exemple `Legifrance/PISTE HTTP 503 on /consult/juri after 3 attempts` (P-014). Les erreurs reqwest passent par `Error::without_url()` (`RQ:src/error.rs:91`) avant tout affichage, et seul le chemin du gabarit d'endpoint est cité. Les noms et champs exacts des variantes sont fixés dans [02-architecture.md](02-architecture.md).

Selon des clients tiers, une facette inconnue, un champ JSON inconnu ou `fond=ALL` avec filtres provoquent un **500** plutôt qu'un 400 : **NON VÉRIFIÉ** [v:500-requete-invalide] (recherche 05 §2.2). L'adapter n'envoie que des champs et facettes issus de tables vérifiées (§6), ce qui limite ce risque.

### 5.2 Politique de réessai

| Paramètre | Valeur | Raison |
|---|---|---|
| Tentatives transitoires | 3 au plus : 1 + 2 réessais | 00 §8 ; parité P-024 |
| Attente avant la 2e tentative | 500 ms + gigue uniforme dans [0, 500 ms) | P-024 exige au moins 0,5 s puis au moins 1 s ; la gigue s'ajoute et ne retranche rien |
| Attente avant la 3e tentative | 1 s + gigue dans [0, 1 s) | idem |
| `Retry-After` (429, 503) | en secondes entières ou en date HTTP (IMF-fixdate, parsée par `chrono`) ; attente = `max(backoff, Retry-After)`, plafonnée à 30 s | corrige D-12 ; présence et format réels **NON VÉRIFIÉ** [v:retry-after-piste] |
| Attente prévue supérieure au reste du délai global | pas d'attente : échec immédiat, `RateLimited { retry_after }` (429) ou dernière erreur (5xx) | une réponse rapide et actionnable plutôt qu'un dépassement certain |
| Budget 401 | 1 renouvellement par appel du port, séparé du budget transitoire | §3.4 |
| Erreurs mises en cache | jamais | parité (recherche 01 §6.4) ; c'est au cache de ne pas les stocker (§12.2) |

Mise en œuvre avec `backon` 1.6.0 (`default = std, std-blocking-sleep, tokio-sleep, gloo-timers-sleep`, index crates.io) : un `ExponentialBuilder` (délai minimal 500 ms, facteur 2, 2 réessais, gigue), un prédicat `when` qui ne retient que les issues transitoires, un ajustement du délai par `Retry-After`, et un « sleeper » qui délègue au `Sleeper` du crate, pour les tests. Les noms exacts de l'API backon 1.6 (`when`, `adjust`, `sleep`, `notify`) et la sémantique de sa gigue sont **NON VÉRIFIÉ** [v:backon-api]. Si `adjust` n'existe pas ou si la gigue de backon peut réduire le délai, on écrit un itérateur `Backoff` maison (backon accepte tout `Iterator<Item = Duration>`), sans changer la politique ci-dessus.

```rust
// retry.rs — esquisse non compilée, à valider en M0 (la logique compte, pas l'API backon exacte)
async fn call<D: DeserializeOwned>(&self, ep: Endpoint, body: &impl Serialize) -> Result<D, CorpusError> {
    let deadline = Deadline::current_or(&self.clock, self.cfg.tool_deadline);
    let mut refreshed = false;
    let mut plan = BackoffPlan::new(&deadline);                 // 500 ms, 1 s (+ gigue), au plus 2 réessais
    loop {
        let permit = self.gate.acquire(&deadline).await?;      // sémaphore + governor (§5.4)
        let lease = self.tokens.token(&deadline).await?;       // mono-vol (§3.3)
        let outcome = self.attempt(ep, body, &lease, &deadline).await; // §5.1
        drop(permit);
        match outcome {
            Outcome::Ok(bytes) => return decode_and_parse::<D>(ep, bytes, &self.cfg),
            Outcome::Unauthorized if !refreshed => { refreshed = true; self.tokens.invalidate(lease.generation).await; continue; }
            Outcome::Unauthorized => return Err(CorpusError::Auth),
            Outcome::Transient { err, retry_after } => match plan.next(retry_after) {
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

- 00 §2 place le délai global dans l'adapter, mais les signatures du port n'ont pas de paramètre de délai (00 §3). [02-architecture.md](02-architecture.md) §7.1 exécute chaque cas d'usage sous `tokio::time::timeout(LF_TOOL_DEADLINE_S)` (garde extérieure) et plafonne les réessais de l'adapter par `LF_TOOL_DEADLINE_S`. Pour que l'adapter connaisse le temps **restant** quand un outil enchaîne plusieurs appels du port, proposition complémentaire : `lf-application` définit un `Deadline` porté par une variable locale de tâche (`tokio::task_local!`), posée par le décorateur `Audited` au début de l'exécution du cas d'usage. L'adapter la lit avec `Deadline::current_or(default)`. En son absence (tests, appel direct), il crée son propre délai de `tool_deadline` à l'entrée de la méthode du port.
- Les sous-futurs d'un même cas d'usage (`fr_search_all`, `fr_verify_citations`) sont interrogés dans la même tâche (`join_all`, `buffer_unordered`). Ils voient donc la même échéance. Une tâche lancée par `tokio::spawn` ne la verrait pas : le cas d'usage doit soit ne pas lancer de tâche, soit propager explicitement le `Deadline`.
- Chaque tentative reçoit `RequestBuilder::timeout(min(request_timeout, reste))`. Chaque attente (`Sleeper`, sémaphore, verrou du jeton) est bornée par `reste`.
- La garde extérieure de 02 §7.1 reste en place. L'adapter garantit de rendre la main avant l'échéance, avec `Timeout` ou la dernière erreur, ce qui permet de rendre une erreur précise plutôt qu'une coupure.
- Pire cas d'un appel du port : `≤ tool_deadline + ε`, où ε est le temps d'observer l'annulation (quelques millisecondes).
- Délais imposés par les clients MCP (Claude Code, VS Code) à un appel d'outil : **NON VÉRIFIÉ** [v:delai-clients-mcp]. Si l'un coupe avant 60 s, le défaut doit baisser.

### 5.4 Limitation de débit

- Un `governor::RateLimiter` **direct** (non indexé), global au processus, **5 requêtes/s, rafale 10** par défaut (00 §8), configurable (§2.2). Les quotas PISTE valent pour toute l'application consommatrice et sont donc partagés par tous les utilisateurs d'un éventuel serveur HTTP (recherche 05 §3.3, §4.3, Q4). Valeurs réelles **NON VÉRIFIÉ** [v:quotas-piste] : l'utilisateur les lit dans la console PISTE et ajuste `LF_PISTE_RPS`/`LF_PISTE_BURST` (variables proposées).
- Le limiteur est consulté **avant chaque tentative**, réessais compris. Il ne l'est pas pour la requête token (§3.3).
- **Piège vérifié** : `RateLimiter::until_ready` attend avec `futures_timer::Delay` (governor 0.10.4, `src/state/direct/future.rs:11`, `:45-56`), pas avec `tokio::time`. Il échappe donc à toute horloge de test et ne se borne pas proprement par le délai global. L'adapter **n'utilise pas** `until_ready`. Il boucle sur `check()` (`src/state/direct.rs:73`) : en cas de refus, `wait = not_until.wait_time_from(clock.now())` ; si `wait` dépasse le reste du délai, échec immédiat `RateLimited { retry_after: Some(wait) }` ; sinon `Sleeper::sleep(wait)`.
- Horloge du limiteur : une implémentation de `governor::clock::Clock` dont l'`Instant` est un `Duration` écoulé depuis une origine `MonoClock`. Elle généralise la piste de 02 §3.3 (`governor::clock::FakeRelativeClock` dans les tests). `Duration` implémente déjà `Reference` et `Add<Nanos>` (`src/clock.rs:61-117`). En test, `ManualClock` rend le limiteur déterministe. Les features `quanta` et `dashmap` sont inutiles. Les noms de constructeurs (`Quota::per_second`, `allow_burst`, `RateLimiter::direct_with_clock`) sont à confirmer à la compilation : **NON VÉRIFIÉ** [v:governor-api].

### 5.5 Concurrence et `fr_search_all`

- Un `tokio::sync::Semaphore` de **4** permis (variable proposée `LF_PISTE_CONCURRENCY`) borne les requêtes API en vol pour tout le processus. Le permis est pris avant le limiteur et rendu après la lecture du corps. Valeur reprise de la pratique tierce de 4 requêtes simultanées (recherche 05 §3.3).
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
      { "facette": "DATE_VERSION", "singleDate": 1792015200000 },
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
| `typePagination` | §6.6 | Swagger `RechercheSpecifiqueDTO.typePagination` |
| `secondSort`, `fromAdvancedRecherche`, `proximite`, `multiValeurs`, `valeurs` | **jamais envoyés** en v0.1 | parité ; champs facultatifs dans le Swagger |

Les champs requis du Swagger sont tous présents : `fond` et `recherche` (`SearchRequestDTO`) ; `champs`, `operateur`, `pageNumber`, `pageSize`, `sort` et `typePagination` (`RechercheSpecifiqueDTO`) ; `operateur`, `typeRecherche` et `valeur` (`CritereDTO`).

Les DTO de requête de la recherche 03 §12.3 sont repris (`SearchRequestDto`, `RechercheDto`, `ChampDto`, `CritereDto`, `FiltreDto`, `DatePeriodDto`, enums en `SCREAMING_SNAKE_CASE`), avec `FiltreDto.single_date: Option<i64>` (epoch ms) et `dates: Option<DatePeriodDto { start: String, end: String }>`. Les variantes inutiles en v0.1 (`multi_valeurs`, `proximite`, sous-critères) ne sont pas déclarées.

### 6.2 Fond et `DATE_VERSION`

- `LODA_DATE` et `CODE_DATE` **exigent** un filtre `DATE_VERSION` en `singleDate` (référence DISCOVERY.md:22-25, constat live). L'adapter l'ajoute toujours pour ces deux fonds, à partir de `SearchQuery.version_date`. L'application la renseigne avec `Clock::today_paris()` si l'utilisateur ne l'a pas donnée (00 §7).
- Pour les autres fonds, `version_date` n'est jamais envoyé. Le rejet ou l'ignorance de ce paramètre est décidé dans [05-outils-mcp.md](05-outils-mcp.md) (03 §4.1).
- `singleDate` est un **entier epoch ms**, forme validée en direct par la référence (server.py:136). La forme chaîne (`"2025-04-15"`, exemple officiel `exemples…md:815`) n'est pas utilisée.

### 6.3 `DATE_VERSION` à minuit Europe/Paris

Règle (00 §8) : `singleDate` = l'instant de minuit, heure de Paris, au jour `version_date`, exprimé en millisecondes depuis l'epoch. Il est stable toute la journée : deux recherches identiques le même jour donnent le même corps, donc la même clé de cache. Cela corrige D-01 et E-06 : la référence envoyait `now_ms`, qui changeait à chaque milliseconde.

```rust
// request/search.rs — non compilé, à valider en M0
fn paris_midnight_ms(d: NaiveDate) -> i64 {
    let local = d.and_time(NaiveTime::MIN);                       // minuit local, sans unwrap
    match chrono_tz::Europe::Paris.from_local_datetime(&local) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.timestamp_millis(), // earliest
        LocalResult::None => Utc.from_utc_datetime(&local).timestamp_millis(),          // repli théorique
    }
}
```

Vecteurs de test (calculés avec la base IANA, `zoneinfo` de Python, et à reproduire avec `chrono-tz`) :

| `version_date` | `singleDate` | UTC |
|---|---|---|
| 2026-10-15 | `1792015200000` | 2026-10-14T22:00Z (heure d'été) — même valeur que 03 §6.4 |
| 2026-01-15 | `1768431600000` | 2026-01-14T23:00Z (heure d'hiver) |
| 2026-03-29 (passage à l'heure d'été à 2 h) | `1774738800000` | 2026-03-28T23:00Z |
| 2026-10-25 (retour à l'heure d'hiver à 3 h) | `1792879200000` | 2026-10-24T22:00Z |
| 1804-03-21 (avant 1911 : temps moyen de Paris, UTC+0:09:21) | `-5231606961000` | 1804-03-20T23:50:39Z |

Les changements d'heure à Paris ont lieu à 2 h et à 3 h : minuit n'est ni ambigu ni inexistant pour les dates contemporaines. Le repli ne sert qu'à rendre la fonction totale.

**Risque.** Tous les horodatages de date observés dans les réponses sont à minuit **UTC** (recherche 02 D-20, recherche 03 §11.2). Si PISTE compare `singleDate` à des bornes de version ainsi stockées, minuit Paris, soit 22 h ou 23 h UTC la veille, sélectionnerait la version **de la veille** : un texte entrant en vigueur le jour J n'apparaîtrait pas le jour J. **NON VÉRIFIÉ** [v:singledate-minuit-paris]. Vérification live prévue (§13.5) : chercher un texte dont une version commence à une date D connue, avec `version_date = D`. Le suffixe `_JJ-MM-AAAA` de `titles[0].id`, qui est la date de début de la version (03 §2.4), dit quelle version l'API a retenue. La décision reste appliquée jusqu'au résultat. Une alternative est proposée dans [09-risques-et-questions.md](09-risques-et-questions.md) (voir aussi §16).

### 6.4 `match` → `typeRecherche`

| `match` (00 §7) | `typeRecherche` | Remarque |
|---|---|---|
| `any` (défaut proposé, Q10) | `UN_DES_MOTS` | parité (server.py:144) ; « un des mots » = OU logique, d'où beaucoup de bruit (1 786 résultats LODA pour « republique numerique », recherche 02 D-32) |
| `all` | `TOUS_LES_MOTS_DANS_UN_CHAMP` | sémantique exacte avec `typeChamp=ALL` (tous les mots dans un **même** champ, ou dans l'un quelconque des champs ?) : **NON VÉRIFIÉ** [v:mode-all-semantique] |
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

- La facette retenue représente « la date du document » pour un juriste : signature d'un acte, date d'une décision ou d'une délibération. La table vit dans l'adapter (`request/search.rs`, `fn date_facet(Fond) -> Option<&'static str>`), car ce sont des noms d'API (03 §3.1).
- `Code` avec une plage de dates : `CorpusError::InvalidRequest` sans appel réseau (`date_from/date_to are not supported for fond CODE; use version_date`). [05-outils-mcp.md](05-outils-mcp.md) doit rejeter ce cas en amont (`invalid_arg`) ; le contrôle de l'adapter n'est qu'une défense.
- Borne absente : si `DateRange` admet une borne ouverte (forme à fixer en M1, 03 §6.1), l'adapter envoie toujours les deux champs, avec `1500-01-01` ou `2221-12-31`, les bornes de `VersionDate` (03 §6.1). Tous les exemples officiels fournissent les deux bornes.
- Noms de facettes : vérifiés dans la documentation officielle. **Effet réel** en direct (le total diminue-t-il ?), caractère inclusif de `end`, comportement d'une borne lointaine, et combinaison avec `DATE_VERSION` : **NON VÉRIFIÉ** [v:filtres-dates-effet]. Un exemple officiel combine déjà `DATE_SIGNATURE` et `DATE_VERSION` sur `LODA_DATE` (`exemples…md:535-553`).

### 6.6 Pagination et `typePagination`

- `typePagination` : `DEFAUT` ou `ARTICLE` (Swagger). « Lors de la navigation dans plusieurs pages, il est nécessaire de passer la valeur reçue dans la réponse précédente » (`RechercheSpecifiqueDTO.typePagination`, recherche 03 §3.2). Observé : `CODE_DATE` répond `ARTICLE` à une requête `DEFAUT` (search_code.json : `typePagination: "ARTICLE"`, `totalArticleResultNumber: 1`).
- La valeur reçue est rendue dans `SearchPage.pagination` (03 §4.1). `fr_search` la renvoie telle quelle (00 §7).
- **Lacune comblée ici** : 00 §7 ne prévoit aucun paramètre d'entrée pour la rendre à la page suivante, et `fr_search` est sans état. L'adapter tient donc un `PaginationMemo` (§12.3) dont la clé est la requête normalisée **sans** `page` (`fond`, `mode`, `text`, `version_date`, `date_range`, `page_size`). Il conserve la dernière `typePagination` reçue (TTL 1 h, comme le cache de recherche ; 1 024 entrées au plus).
- Valeur envoyée, dans l'ordre :
  1. `SearchQuery.pagination` si elle diffère de `Defaut` (valeur explicite fournie par l'application ou par une recherche `NUM_ARTICLE`, §6.9) ;
  2. si `page > 1`, la valeur du mémo ;
  3. si `page > 1`, `fond = Code` et aucun mémo : `ARTICLE`, valeur observée pour ce fond ;
  4. sinon `DEFAUT`.
- Effet réel de `DEFAUT` contre `ARTICLE` en page 2 sur CODE, et cohérence de `has_more` (calculé sur `totalResultNumber`, 03 §4.1) quand la pagination porte sur les articles : **NON VÉRIFIÉ** [v:typepagination-page2]. À vérifier en M5 (§13.5).
- Valeur reçue inconnue (ni `DEFAUT` ni `ARTICLE`) : traitée comme `Defaut`, avec `warn`.

### 6.7 `pageSize` et pagination profonde

- `pageSize` ∈ 1..=100, garanti par `PageSize` (03 §1.4) ; maximum documenté à 100 (Swagger ; exemple officiel `pageSize: 100`, `exemples…md:1027`). `pageNumber` commence à 1 (exemples officiels ; référence server.py:150).
- Aucune limite de profondeur n'est documentée pour `/search`. Le moteur est vraisemblablement Elasticsearch, dont la fenêtre par défaut est de 10 000 résultats (recherche 05 §2.4) : **NON VÉRIFIÉ** [v:pagination-profonde]. Proposition à trancher en M5 : si `page × page_size > 10 000`, l'adapter renvoie `InvalidRequest` (`results beyond the first 10000 are not reachable; narrow the search with date_from/date_to`) au lieu de risquer un 500 opaque.

### 6.8 Assainissement de la requête

- La normalisation est faite **une fois**, par le domaine (`QueryText`, 03 §4.1) : `trim`, repli des blancs, retrait des caractères de contrôle, 1 à 500 caractères (proposition D-32 ; limite réelle de l'API **NON VÉRIFIÉ** [v:longueur-max-query]).
- L'adapter ne réécrit **pas** la requête. Il n'échappe rien, car la sérialisation JSON suffit à empêcher toute injection structurelle (recherche 02 D-32). Il ne retire ni guillemets ni opérateurs.
- La syntaxe interne du moteur dans `valeur` (guillemets, jokers, `*` seul qui signifie « tout » d'après un exemple officiel JURI, `exemples…md:1045-1119`) est **NON VÉRIFIÉ** [v:syntaxe-moteur-valeur]. Une requête `*` est transmise telle quelle ; le résultat est borné par la pagination.
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

Le comportement réel de chaque recherche exacte (casse, tirets typographiques, zéros de tête) est **NON VÉRIFIÉ** [v:recherche-exacte-par-numero]. La forme attendue par `NUM_ARTICLE` (`L5-8` ou `5-8`, `1er`, suffixes latins) est **NON VÉRIFIÉ** [v:num-article-forme-api] (03). Les stratégies complètes de `find_article` et `find_by_number` sont au §7.2.

### 6.10 Tests de sérialisation

- `search_request_matches_reference_json` : pour chacun des 8 fonds, avec `match=any`, `page=1` et `page_size=10`, le corps est égal, comme `serde_json::Value`, au payload de la référence (recherche 01 §3.1). Seule exception : la valeur de `singleDate`, qui est minuit Paris (divergence volontaire listée dans [annexes/A-parite.md](annexes/A-parite.md), P-020).
- `type_recherche_follows_match` (3 cas) ; `filtres_order_is_deterministic` ; `date_facet_table_matches_doc` (table du §6.5) ; `code_date_range_is_invalid_request_without_network` (wiremock `expect(0)`) ; `paris_midnight_vectors` (table du §6.3) ; `enum_serialization_screaming_snake_case` (`AucuneCorrespondanceACetteExpression` donne `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION`, recherche 03 §12.3).

---

## 7. Endpoints par jalon

### 7.1 Table

Tous les chemins sont présents dans le Swagger 2.4.2, ce qui a été vérifié par script sur la copie de pylegifrance (`paths`, `definitions`, réponses déclarées `200, 201, 401, 403, 404`). Base : `{base_url}` (§2.1). Méthode : `POST` JSON partout.

| Endpoint | Corps envoyé (champs requis du Swagger en gras) | DTO de réponse (§8.3) | Méthode du port | Jalon |
|---|---|---|---|---|
| `/search` | `SearchRequestDto` (§6.1) | `SearchResponseDto` | `search` | M2 |
| `/consult/lawDecree` | `{"**textId**": "LEGITEXT…", "**date**": "AAAA-MM-JJ"}` | `ConsultTextDto` | `get_act` | M2 |
| `/consult/getArticle` | `{"**id**": "LEGIARTI…" \| "KALIARTI…"}` | `GetArticleResponseDto` | `get_article` | M2 |
| `/consult/juri` | `{"**textId**": "JURITEXT…" \| "CONSTEXT…" \| "CETATEXT…"}` | `ConsultJuriResponseDto` | `get_decision` | M2 |
| `/consult/cnil` | `{"**textId**": "CNILTEXT…"}` | `ConsultJuriResponseDto` (même `TexteSimple`) | `get_deliberation` | M2 |
| `/consult/kaliText` | `{"**id**": "KALITEXT…"}` | `ConsultTextDto` (champs KALI) | `get_convention` | M2 |
| `/consult/acco` | `{"**id**": "ACCOTEXT…"}` | `ConsultAccoResponseDto` | `get_company_agreement` | M2 |
| `/consult/getArticleWithIdAndNum` | `{"id": "LEGITEXT…", "num": "L1242-2"}` | `GetArticleResponseDto` | `find_article` (date du jour) | M6 |
| `/search` en `EXACTE` | `typeChamp` du §6.9 (+ `DATE_VERSION` pour CODE et LODA) | `SearchResponseDto` | `find_article` (autre date, repli), `find_by_number` | M6 |
| `/consult/getJoWithNor` | `{"**nor**": "ECFI1524250L"}` | `ConsultJorfDto` | `find_by_number` (NOR) | M6 |
| `/consult/kaliContIdcc` | `{"**id**": "2121"}` | `KaliContDto` | `find_by_number` (IDCC → convention) | M6 |
| `/list/code` | `{"**pageNumber**": 1, "**pageSize**": 100, "sort": "TITLE_ASC"}` | `CodeListResponseDto` | interne (table des codes, §12.3) ; `list_codes` en M7 | M6 |
| `/consult/getArticleByCid` | `{"**cid**": "LEGIARTI…"}` | `GetListArticleResponseDto` | `article_versions` (repli) | M7 |
| `/search/canonicalArticleVersion` | `{"**id**": "LEGIARTI…"}` | `CanonicalVersionsDto` | `article_versions` | M7 |
| `/consult/relatedLinksArticle` | `{"**articleId**": "LEGIARTI…"}` | `RelatedLinksDto` | `related_links` | M7 |
| `/consult/legi/tableMatieres` | `{"**textId**": "LEGITEXT…", "**date**": "AAAA-MM-JJ", "nature": "CODE"}` | `ConsultTextDto` | `get_act` quand le texte est un code (§7.3) | M7 |
| `/suggest` | `{"searchText": "…", "supplies": ["LODA_LIST", "CODE_LIST"]}` | `SuggestResponseDto` | à définir (voir §16) | M7 |
| `/search` avec `fond = JORF` | `SearchRequestDto` | `SearchResponseDto` | `search` (`Fond::Jorf`) | M7 |

Corps des consultations : exactement ceux de la référence (client.py:187-208 ; parité P-021). Les clés diffèrent selon l'endpoint : `textId` pour `lawDecree`, `juri` et `cnil` ; `id` pour `getArticle`, `kaliText` et `acco`. `searchedString` et `fromSuggest` ne sont jamais envoyés. L'effet de `searchedString`, probablement un surlignage, n'est pas documenté (recherche 03 §3.3) ; c'est sans incidence puisqu'il n'est pas envoyé. La date de `lawDecree` est la `VersionDate` formatée `%Y-%m-%d`, forme validée en direct par la référence (client.py:190-193).

Chemins utilisés par des tiers mais **absents** du Swagger 2.4.2 (`consult/loda/version`, `consult/loda/versions`, `consult/juri/version(s)`, `consult/juri/ancienId`, recherche 05 §1.12) : interdits. Un test de dérive compare la liste des chemins appelés par le crate (constantes `Endpoint::path()`) à une copie versionnée de la liste des chemins du Swagger (`crates/lf-adapter-piste/tests/data/swagger-2.4.2-paths.txt`). Ce test empêche aussi tout chemin hors de la liste de 00 §8.

### 7.2 Stratégies M6

**`find_article(&CodeRef, &ArticleNum, VersionDate)`**

1. `CodeRef` → `LegiTextId` du code par la table des codes (`/list/code`, §12.3). Aucun identifiant de code n'est codé en dur : [annexes/donnees/codes-table.md](annexes/donnees/codes-table.md) signale même un conflit d'identifiant (Code rural et de la pêche maritime). Elle ne sert qu'au dictionnaire du parseur (03 §8).
2. Si `VersionDate` est aujourd'hui : `/consult/getArticleWithIdAndNum {id, num}`. Limites officielles : l'endpoint ne renvoie que la version « en vigueur au moment de la recherche », et « seuls les articles ne comportant pas de version en vigueur différée peuvent être trouvés » (Swagger, description du chemin). Sur `NotFound` ou réponse vide, passer à l'étape 3.
3. Sinon : `/search` `CODE_DATE`, `typeChamp = NUM_ARTICLE`, `EXACTE`, `DATE_VERSION = VersionDate`, `pageSize = 100`, **sans** filtre `NOM_CODE`. Les hits CODE sont des codes porteurs d'extraits d'articles (search_code.json, recherche 03 §4.2). On garde le hit dont `titles[0].cid` est le `LEGITEXT` du code, puis l'extrait dont `num` égale le numéro demandé : il donne le `LEGIARTI`, puis `get_article`.
4. Pas de `NOM_CODE` : la documentation officielle l'utilise, mais deux clients tiers le disent cause de 500 (recherche 05 §2.2) : **NON VÉRIFIÉ** [v:nom-code-filtre]. Il pourra servir d'optimisation après vérification.
5. Comportement réel de `getArticleWithIdAndNum` (forme de `num`, réponse sans article) : **NON VÉRIFIÉ** [v:idandnum-comportement].

**`find_by_number(&NumberQuery)`** — signature fixée par [02-architecture.md](02-architecture.md) §5.1 ; les variantes de `NumberQuery` sont définies dans [03-domaine.md](03-domaine.md) et [annexes/B-citations.md](annexes/B-citations.md). L'adapter a seulement besoin de trois informations par requête : le fond, la nature du numéro et, en option, une plage de dates. Forme minimale attendue, à reprendre dans 03 :

```rust
// non compilé, à valider en M0 — besoins de l'adapter, pas une définition normative
pub enum NumberKind { Text /*NUM*/, Case /*NUM_AFFAIRE*/, Decision /*NUM_DEC*/, Deliberation /*NUM_DELIB*/, Idcc, Nor }
pub struct NumberQuery { pub fond: Fond, pub kind: NumberKind, pub value: String, pub within: Option<DateRange> }
```

- `Nor` : `/consult/getJoWithNor`. Une réponse avec `hasLoda = true` permet de basculer vers la version consolidée. La documentation rappelle que « tous les textes n'ont pas de NOR » (`exemples…md:1209`).
- `Idcc` sur `Kali` : `/consult/kaliContIdcc` donne le conteneur (`KALICONT…`, `texteBaseId[]` → `KaliTextId`). En repli, `/search` `KALI` `IDCC` `EXACTE`. Sur `Acco` : `/search` `ACCO` `IDCC`.
- Autres : `/search` du fond, `typeChamp` du §6.9, `EXACTE`, `pageSize = 20`, `within` mappé sur la facette du §6.5 (ex. `DATE_DECISION` pour lever l'ambiguïté d'un numéro de pourvoi, comme pylegifrance, recherche 03 §11.6).
- Formes réelles des réponses de `getJoWithNor`, `kaliContIdcc` et `/list/code` (champs effectivement remplis) : **NON VÉRIFIÉ** [v:endpoints-m6-reponses].

### 7.3 Stratégies M7

- **`article_versions`** : `/search/canonicalArticleVersion {id}` renvoie `articleVersions[{id, version, numero, etat, dateDebut, dateFin, ordre}]` (Swagger `ArticleVersion`), soit des métadonnées légères. Si la liste est vide ou si l'appel répond 404 : repli sur `/consult/getArticleByCid {cid}` (`listArticle[Article]`, toutes les versions avec leur texte), réduit à des `ArticleVersion`. `fr_diff_article` lit ensuite le texte des deux versions par `get_article`, mis en cache 24 h.
- **`related_links`** : `/consult/relatedLinksArticle {articleId}` renvoie `liensCite[]` et `liensCitePar[]` de `LiensRelatifsDTO {id, cidText, cidParent, nature ∈ {LODA, JORF, CODE, KALI, JORFCONT}, name, dateVigeur}` (Swagger). `dateVigeur` est une chaîne de chiffres (exemple `1470787200000`), lue par `opt_raw_date`. Taille réelle des listes : voir le libellé ci-dessous ; la pagination éventuelle est faite côté serveur MCP (05).
- **`get_act` sur un code (D-47)** : si `LegiTextId` figure dans la table des codes, l'adapter appelle `/consult/legi/tableMatieres {textId, date, nature: "CODE"}` plutôt que `/consult/lawDecree`, pour obtenir la table des matières sans le contenu des articles. Comportement de `lawDecree` sur un code : **NON VÉRIFIÉ** [v:lawdecree-sur-code] (03). Le passage à `tableMatieres` est conditionné à la vérification.
- **`list_codes`** : `/list/code` sans filtre `states`, paginé par 100 jusqu'à `totalResultNumber` (au plus 10 pages). Chaque entrée garde son `etat`, et les complétions filtrent `VIGUEUR`. 76 codes sont en vigueur (facette `FOND`, recherche 03 §5).
- **`/suggest`** : `SuggestSupplyRequest {searchText, supplies[]}` → `results: map<id, SuggestValue{id, idTexte, section, label, nature, origin, dateVersion, appellations[]}>` (Swagger). C'est un **POST** JSON, pas un GET (recherche 05 §1.10). Usage visé : appellations usuelles (« loi Macron ») pour `fr_resolve` et complétions. Méthode de port à définir (§16).
- Formes réelles et tailles des réponses de `tableMatieres`, `canonicalArticleVersion`, `getArticleByCid`, `relatedLinksArticle` et `suggest` : **NON VÉRIFIÉ** [v:endpoints-m7-reponses].
- La **consultation** d'un `JORFTEXT` (fond `Jorf`, M7) n'a pas d'endpoint dans la liste de 00 §8 (`/consult/jorf` existe dans le Swagger mais n'y figure pas) : voir §16.

---

## 8. DTO

### 8.1 Principes

Repris de la recherche 03 §12.1 et figés ici :

1. **Un module par endpoint** ; aucune struct partagée entre endpoints pour des champs homonymes de types différents. `signataires` est un objet dans `/consult/kaliText` et une chaîne dans `TexteSimple` (recherche 03 §9) ; il n'est d'ailleurs pas déclaré du tout.
2. **Tolérance** : jamais `#[serde(deny_unknown_fields)]`. Serde ignore les champs inconnus par défaut. On ne déclare **que** les champs utilisés.
3. **Tout est optionnel** : `Option<T>` et `#[serde(default)]` partout. Tout champ à désérialiseur personnalisé porte `#[serde(default)]`, sinon son absence est une erreur `missing field` (serde_derive, recherche 03 §12.1 point 3). Toute liste passe par `null_default`, car `Vec<T>` refuse `null` (des listes `null` existent : `textTitles[].appellations`).
4. **Dates brutes** (`RawDate`) dans le DTO, converties en `NaiveDate` dans le mapping (§9.1).
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

Présence en direct de `texteHtml` sur `/consult/getArticle` : **NON VÉRIFIÉ** [v:texte-html-getarticle-juri] (03), d'où le repli sur `texte`.

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

**M7**

| DTO | Champs |
|---|---|
| `CanonicalVersionsDto` | `articleVersions: Vec<ArticleVersionDto { id, version, numero, etat, dateDebut, dateFin, ordre: i64 }>` |
| `GetListArticleResponseDto` | `listArticle: Vec<ArticleDto>` |
| `RelatedLinksDto` | `liensCite`, `liensCitePar: Vec<LienRelatifDto { id, cidText, cidParent, nature, name, dateVigeur }>` |
| `SuggestResponseDto` | `totalResultNumber`, `results: BTreeMap<String, SuggestValueDto { id, idTexte, section, label, nature, origin, dateVersion, appellations }>` |
| `/consult/legi/tableMatieres` | `ConsultTextDto` réutilisé |

### 8.4 Pièges

| Piège | Où | Parade | Test |
|---|---|---|---|
| `titresTM` : `rename_all = "camelCase"` produirait `titresTm` | `article.context` | `#[serde(rename = "titresTM")]` explicite | `titres_tm_is_renamed` |
| `signataires` : objet en KALI, chaîne dans `TexteSimple` | kaliText / juri | non déclaré ; jamais de DTO commun | `kali_and_juri_dto_are_distinct` |
| `idcc`, `codeIdcc`, `sizeAttachment` : chaînes numériques, nombres possibles (la référence typait `str \| int`) | search ACCO, acco | `opt_string_or_number` (E-14, D-24) | `idcc_accepts_string_and_number` |
| casse de `nature` : `arret`, `qpc`, `code` en recherche ; `ARRET`, `QPC`, `Texte`, `Délibération` en consultation | partout | valeur brute conservée ; comparaisons insensibles à la casse dans le mapping | `nature_case_is_preserved` |
| NOR en minuscules en recherche (`cscl2115694s`) | search | `Nor` du domaine en majuscules (03 §4.2) | — |
| `isMultipleVersions` (fixture) contre `multipleVersions` (Swagger, KALI) | getArticle | `alias` | `multiple_versions_alias` |
| `typePagination` renvoyé différent de l'envoyé | search CODE | §6.6 | `type_pagination_is_echoed_on_next_page` |
| suffixe `_JJ-MM-AAAA` sur `titles[].id` et `id` de lawDecree | search LODA/CODE, lawDecree | `VersionedId` du domaine (03 §2.4) | `suffix_is_version_date` |
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
- Limite de récursion : serde_json limite par défaut la profondeur d'imbrication (128 niveaux selon sa documentation, **NON VÉRIFIÉ** [v:serde-json-recursion]). Elle n'est pas levée (pas de feature `unbounded_depth`). Le domaine borne de son côté la TOC à 64 niveaux (03 §1.3).
- Corps HTML (page d'erreur d'une passerelle), JSON `null` à la racine, JSON tronqué : `Decode`, sans réessai (corrige D-11 et E-21 : la référence levait une exception sans code).

---

## 9. Mapping DTO → domaine

### 9.1 Règles communes

Fonctions pures de `map/`, du type `fn(dto, &contexte de requête) -> Result<Entité, CorpusError>`. Elles appellent les constructeurs de `lf-domain` et ne font aucune E/S.

| Règle | Contenu | Défauts corrigés |
|---|---|---|
| (a) Introuvable | enveloppe absente ou `null`, ou objet sans `id`, `cid` ni titre ⇒ `NotFound`. Objet rempli mais sans `id` ⇒ l'identifiant **demandé** est utilisé, avec un diagnostic `warn` | parité (recherche 03 §12.8 a ; S15) |
| (b) Identifiant renvoyé ≠ identifiant demandé (après retrait du suffixe) | l'entité garde l'identifiant renvoyé, vérité de l'API ; diagnostic `warn` | — |
| (c) Identifiants | `FromStr` stricts du domaine ; suffixe `_JJ-MM-AAAA` vers `VersionedId` (03 §2.4) | D-28, D-31, E-16 |
| (d) Routage | la famille de juridiction, l'URL et la règle de citation viennent du **type** d'identifiant. `origine` n'est qu'un contrôle de cohérence (diagnostic s'il diverge) | D-28, E-18 |
| (e) Dates | `RawDate::Millis(ms)` → `date::from_epoch_ms` (03 §6.3, epoch négatifs compris, sans dépendance à la plateforme) ; `Text("AAAA-MM-JJ")` → `NaiveDate` ; `Text("AAAA-MM-JJT…+0000")` → partie date ; puis `date::open_ended` (toute date ≥ 2222-01-01 devient `None`) ; les champs `…Computed` sont lus en priorité. Un horodatage qui n'est pas à minuit UTC produit un diagnostic : convention générale **NON VÉRIFIÉ** [v:convention-horaire-timestamps] (03) | D-20, D-21, E-11, E-35 |
| (f) Textes | `LongText::best_of(texteHtml, texte)` (03 §7.4) ; les champs courts passent par `text::clean` | D-26 |
| (g) Statut | `status_of(etat, jurisState)` | D-21, E-10 |
| (h) Erreurs du domaine | `DomainError` non utilisateur (`TocTooDeep`, `TextTooLarge`) ⇒ `Decode` | — |
| (i) Contrat de citation | construit par le domaine, jamais `None` (03 §5) ; un hit dont l'identifiant est invalide ou incohérent avec le fond est **écarté et compté** (`SearchPage.dropped`) au lieu d'être renvoyé à moitié | D-30 |

### 9.2 Par entité

| Entité (03 §4) | Fonction | Sources principales | Points propres | Défauts |
|---|---|---|---|---|
| `SearchPage`, `SearchHit` | `map::search_page(dto, &SearchQuery)` | `results[]`, `totalResultNumber`, `totalArticleResultNumber`, `typePagination` | `kind` = fond demandé ; `id` = `titles[0].id` ; `title` sans `<mark>` ; `date` = `date`, puis `datePublication`, puis `dateSignature` ; `snippet` = `text`, sinon `extracts[0].values[0]` (≤ 280 caractères) ; `articles` CODE et KALI depuis `sections[].extracts[]` ; `company`, `idcc` (ACCO) ; `total_articles` si > 0 ; `has_more = page × page_size < total` | D-09, D-30, D-33, E-26 |
| `Act` | `map::act(dto, &LegiTextId, VersionDate)` | lawDecree | `version_start` = suffixe ou `dateDebutVersion` ; `version_end` = `dateFinVersion` (2999 → `None`) ; `cid: JorfTextId` ; `Toc::build(TocDraft)` depuis `sections`/`articles` (titres de sections, tri `intOrdre`, dédoublonnage ; 03 §4.2) ; `nature == "CODE"` : avertissement prévu par 05 | D-25, D-47, E-10, E-11 |
| `ArticleText` | `map::article(dto, &ArticleId)` | getArticle | `code_title` = `textTitles[nature=="CODE"].titre`, sinon `textTitles[0].titre` ; `parent` (nature du texte parent → URL `code_article` ou `loda_article`, 03 §5.5) ; `path` = `context.titresTM[].titre` ; `nota` = `notaHtml`, sinon `nota` ; `previous_version` | D-26, D-29 |
| `Decision` | `map::decision(dto, &DecisionId)` | juri | `ecli` natif validé (`Ecli::from_str`), jamais fabriqué ; `numero_affaire` = `numeroAffaire`, sinon `[num]` pour CETAT ; `date` = `dateTexteComputed`, sinon `dateTexte` ; `OfficialSummary` selon le fond (03 §4.4) ; `cited_case_law` = `citationJp` ; `applied_texts` | D-26, D-27, D-28 |
| `Deliberation` | `map::deliberation(dto, &CnilTextId)` | cnil | `date`, `date_publication` (`…Computed` d'abord) ; `nature_delib` | D-27 |
| `Convention` | `map::convention(dto, &KaliTextId)` | kaliText | `idcc` = `conteneurs[0].num` ; `container_title` (saut de ligne final retiré) ; `bulletin` = `originePubli`, sinon `numeroBo` ; TOC avec `inline_text` depuis `content` (pas de N+1 appels) | D-20, D-25 |
| `CompanyAgreement` | `map::agreement(dto, &AccoTextId)` | acco | métadonnées seules ; `themes` et `unions` limités aux libellés non vides ; **aucun** champ texte (P-036) | E-14 |
| M6 : hits de résolution, `CodeTable` | `map::code_list`, `map::kali_cont`, `map::jorf_by_nor` | §8.3 M6 | `CodeTable` : titre normalisé (vue repliée du domaine) → `LegiTextId` | D-47 |
| M7 : `ArticleVersion`, liens | `map::article_versions`, `map::related_links` | §8.3 M7 | tri par `dateDebut`, puis `ordre` | — |

Les tests de contrat (§13.3) figent ces mappings par snapshots `insta` sur les 16 fixtures et par les valeurs golden de [annexes/donnees/parite-golden.json](annexes/donnees/parite-golden.json), aux divergences volontaires près (listées dans [annexes/A-parite.md](annexes/A-parite.md) et 03 §5.6).

### 9.3 Diagnostics

Le domaine renvoie ses anomalies comme valeurs (03 §1.1). L'adapter les journalise (`debug` pour un hit écarté, `warn` pour un identifiant incohérent, un mojibake réparé ou un horodatage hors minuit) et les compte dans `StatsSnapshot` (`dropped_hits`, `mojibake_repaired`, `id_mismatch`, `non_midnight_timestamps`, `retries`, `rate_limited_waits`). Ces compteurs sont lisibles par `lf-app` (`check-config`) et éventuellement par `fr_coverage` (décision dans 05). Ils ne quittent jamais la machine (ADR-0005).

---

## 10. Réparation du mojibake

### 10.1 Constat

6 fixtures sur 16 (`search_loda`, `search_code`, `search_juri`, `consult_loda`, `consult_article`, `consult_juri`) contiennent de l'UTF-8 décodé à tort en ISO-8859-1 : 252 séquences, 13 motifs, 60 valeurs et 14 clés de facettes, et aucun caractère accentué correct. Les 10 autres fixtures sont saines (recherche 03 §11.1 ; recherche 02 D-35, §5). La présence de caractères de contrôle C1 (`PREMIÃ\x88RE`) désigne ISO-8859-1 et non Windows-1252. La cause la plus probable est l'outil de capture : le même endpoint `/consult/juri` est sain dans une campagne ultérieure. Origine **NON VÉRIFIÉ** [v:mojibake-origine] (03). Dans les fixtures, le mojibake est en caractères bruts, sans aucune séquence d'échappement `\uXXXX` (contrôle de ce document : 0 occurrence dans les 16 fichiers).

### 10.2 Algorithme exact

Appliqué au **texte entier du corps** (réponses 2xx et corps d'erreur), après le décodage UTF-8 strict et avant serde (§4.4). La fonction pure est `lf_domain::text::encoding::repair_latin1_mojibake(&str) -> Cow<str>` (03 §1.5, §7.1) ; l'esquisse est dans la recherche 03 §11.1.

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
| Risque résiduel | un texte entièrement latin-1 dont tous les caractères non ASCII formeraient par hasard de l'UTF-8 valide | jamais observé |

Angle mort : si PISTE échappait les caractères non ASCII (`Ã©`), un mojibake éventuel serait invisible au niveau du corps, qui serait alors en ASCII. Forme des chaînes servies par PISTE (caractères bruts ou échappements) : **NON VÉRIFIÉ** [v:json-echappements-unicode]. Le test de contrat `no_mojibake_after_mapping` contrôle chaque chaîne des entités produites, et le smoke test live (§13.5) le contrôle aussi. Si l'angle mort se confirmait, la même fonction pure serait appliquée par champ dans le mapping, ce qui demanderait une révision de 00 §8.

### 10.4 Ordre impératif

La réparation se fait **avant toute normalisation des NBSP** (00 §8 ; 03 §7.1). La séquence mojibake de `à` est `Ã` suivi de U+00A0 : normaliser U+00A0 avant la réparation casse 10 champs des fixtures (recherche 03 §11.1, mesure `dto_repair_check.py`). Au niveau du corps, aucune normalisation n'a encore eu lieu. Les helpers DTO ne modifient pas les chaînes (`opt_nonempty` ne fait `trim` que pour tester le vide) ; `text::clean` (domaine) intervient après, dans le mapping.

### 10.5 Métrique et journalisation

- Réparation appliquée : compteur `mojibake_repaired` (+1 par corps) et `tracing::warn!(endpoint = ep.template(), bytes = len, "latin1 mojibake repaired in PISTE response")`. Le corps et l'identifiant demandé ne sont pas journalisés.
- Détection positive mais réparation refusée (étapes 3, 5 ou 6) : compteur `mojibake_unrepaired` et `warn`. Le corps est gardé tel quel. Ce cas signale des données réellement ambiguës, à recapturer.
- `LF_PISTE_MOJIBAKE_REPAIR=off` (variable proposée) désactive la réparation mais **pas** la détection ni ses journaux.
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
| `/consult/juri` (JURITEXT, CETATEXT) | **HTTP 400**, corps `{"timestamp":…,"error":400,"status":"Bad Request","message":"L'expression à valider est fausse.","errorCode":null,"exceptionDetails":null}` ; pas de 200 vide | sondé en direct par pylegifrance le 2026-04-13 (`pylegifrance/fonds/juri.py:124-135`, forme du corps dans `tests/unit/fonds/test_juri_verification_endpoints.py:115-121`) ; tiers, non revérifié par nous | `NotFound` |
| `/consult/juri` (CONSTEXT) | inconnu (pylegifrance n'a sondé que JURITEXT et CETATEXT) | **NON VÉRIFIÉ** [v:introuvable-par-endpoint] | règles du §11.2 |
| `/consult/lawDecree`, `/consult/getArticle`, `/consult/cnil`, `/consult/kaliText`, `/consult/acco` | inconnu : 400 avec le même marqueur, 404, 200 avec une enveloppe `null` ou 500 sont tous plausibles | **NON VÉRIFIÉ** [v:introuvable-par-endpoint] (recherche 01 §16 point 4 ; recherche 03 §15 point 9) | règles du §11.2 |
| M6 et M7 (`getArticleWithIdAndNum`, `getJoWithNor`, `kaliContIdcc`, `canonicalArticleVersion`, `getArticleByCid`, `relatedLinksArticle`) | inconnu | **NON VÉRIFIÉ** [v:introuvable-par-endpoint] | règles du §11.2 ; pour une **recherche** (`find_*`), « aucun résultat » est un résultat vide, pas une erreur |
| `/search` | 0 résultat = **succès** (`total = 0`, `hits = []`) | parité S16, E-04 | jamais `NotFound` |
| endpoint token | — | — | jamais `NotFound` (§3.5) |

Le smoke test live `live_unknown_ids_classification` (§13.5) enregistre la réponse réelle de chaque endpoint pour des identifiants `…000000000000` et `…999999999999`. Le résultat complète cette table en M2.

### 11.2 Classification

Arbre appliqué par `classify.rs`, après lecture bornée, décodage et réparation du corps :

1. **2xx** : DTO de l'endpoint. Enveloppe absente ou vide ⇒ `NotFound` (§9.1 a). Pour `/search`, jamais `NotFound`.
2. **400** sur une consultation, et `PisteErrorDto.message` contient `valider est fausse` (comparaison insensible à la casse, sur le texte réparé ; la forme `a valider` sans accent est aussi acceptée) ⇒ `NotFound`. Le marqueur ne s'applique qu'aux consultations dont l'identifiant a été validé par le domaine, comme le fait pylegifrance (`juri.py:829-838`) : un 400 ne peut alors pas venir d'un identifiant malformé.
3. **400**, autres cas ⇒ `InvalidRequest { status: 400, detail }` (message PISTE nettoyé, ≤ 200 caractères, sans caractère de contrôle). [02-architecture.md](02-architecture.md) le rend `upstream_error` (parité S10) et le journalise en `warn` comme bug probable. Option ouverte (§16) : `invalid_arg` quand la cause est vraisemblablement l'entrée utilisateur (requête trop longue, caractères refusés), pour que le modèle puisse reformuler.
4. **404** :
   - sur `/search` ou sur l'endpoint token ⇒ `Config`. Il n'y a pas de « ressource » à ne pas trouver : le chemin ou l'URL est faux ;
   - sur une consultation, si l'URL de base est officielle (`PisteEndpoints.official`) et que le corps n'est pas du HTML ⇒ `NotFound`. Le chemin vient de la liste vérifiée du Swagger, donc il existe ;
   - sinon (hôte personnalisé ou page HTML de passerelle) ⇒ `Config` : `PISTE returned 404 for the API path; check LF_PISTE_BASE_URL.`
   - La forme réelle d'un 404 PISTE, de routage ou de ressource, est **NON VÉRIFIÉ** [v:forme-404-piste] (recherche 02 D-10). Cette règle est provisoire jusqu'au smoke test.
5. **500** répété : `Upstream { status: 500, … }` (§5.1), jamais `NotFound`. « Ne jamais conclure à l'inexistence sans preuve », principe repris d'un client tiers (recherche 05 §1.13).

Un `not_found` signifie donc toujours que l'API a dit que la ressource n'existait pas. Un problème d'URL ou de configuration ne peut jamais faire croire au modèle qu'un texte n'existe pas, ce que la référence permettait (D-10).

### 11.3 Corps d'erreur PISTE

```rust
// dto/error.rs — non compilé, à valider en M0
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PisteErrorDto {
    #[serde(default, deserialize_with = "de::opt_u64_lenient")] pub error: Option<u64>,   // 400
    #[serde(default)] pub status: Option<serde_json::Value>,   // "Bad Request" (chaîne) ; forme variable tolérée
    #[serde(default)] pub message: Option<String>,
    #[serde(default)] pub error_code: Option<String>,
}
```

La désérialisation est **facultative** : un corps d'erreur non JSON (HTML, texte vide) est classé par le statut seul. La forme générale des corps d'erreur de l'API (tous statuts confondus) est **NON VÉRIFIÉ** [v:corps-erreur-piste]. La seule forme connue est celle du 400 de `/consult/juri` (pylegifrance).

---

## 12. Interaction avec le cache et l'audit

### 12.1 Opérations et clés

`lf-adapter-cache` enveloppe l'adapter : `CachedCorpus<PisteCorpus>` (00 §9, ADR-0007). La clé est `(environnement, opération, paramètres normalisés)`. L'environnement vient de `PisteCorpus::environment()`, fourni à la composition par `lf-app`, ce qui corrige D-15.

| Opération (nom de cache) | Méthode du port | Paramètres de la clé | TTL (00 §9) |
|---|---|---|---|
| `search` | `search` | `SearchQuery` normalisée : fond, mode, texte, page, page_size, version_date (au jour), date_range, pagination | 1 h |
| `act` | `get_act` | `LegiTextId`, `VersionDate` | 24 h |
| `article` | `get_article` | `ArticleId` | 24 h |
| `decision` | `get_decision` | `DecisionId` | 24 h |
| `deliberation` | `get_deliberation` | `CnilTextId` | 24 h |
| `convention` | `get_convention` | `KaliTextId` | 24 h |
| `agreement` | `get_company_agreement` | `AccoTextId` | 24 h |
| `find_article` (M6) | `find_article` | `CodeRef` normalisé, `ArticleNum`, `VersionDate` | 24 h (consultation) |
| `find_by_number` (M6) | `find_by_number` | fond, type, valeur, `within` | 1 h (recherche) |
| `article_versions`, `related_links` (M7) | idem | `ArticleId` | 24 h |
| `codes` (M7) | `list_codes` | — | 7 j |

Ces TTL sont ceux de [02-architecture.md](02-architecture.md) §7.2 (`Search` et `FindByNumber` 1 h ; consultations, `FindArticle`, `Versions` et `Related` 24 h ; `Codes` 7 jours).

### 12.2 Exigences sur l'adapter

1. **Déterminisme** : à paramètres de port égaux, corps de requête égal. Aucun champ ne dépend de l'horloge : `singleDate` se déduit de `VersionDate`, jamais de `now` (D-01). La seule exception est la `typePagination` mémorisée (§6.6), cohérente pendant la durée de vie du mémo.
2. **Erreurs** : toujours `Err`, jamais un `Ok` vide déguisé. `NotFound` est une erreur, donc il n'est pas mis en cache (00 §9 ; corrige D-15, où la référence gardait les « introuvables » 24 h).
3. **Réponses vides** : une `SearchPage` sans résultat est un `Ok`. C'est le cache qui décide de ne pas la stocker (00 §9).
4. **Identifiants absents** : la configuration est fixée au démarrage et le cache est vide au démarrage. Aucune réponse en cache ne peut donc exister sans configuration valide. La question de la référence, qui exigeait les identifiants avant de lire son cache disque persistant (E-23), ne se pose plus.
5. Le jeton OAuth n'est jamais dans le cache.

### 12.3 Mémos internes

Deux structures en mémoire, petites, bornées, propres à l'API PISTE, donc dans l'adapter et non dans `lf-adapter-cache` :

| Mémo | Contenu | Borne | TTL | Jalon |
|---|---|---|---|---|
| `PaginationMemo` | clé de recherche sans `page` → dernière `typePagination` reçue | 1 024 entrées, éviction de la plus ancienne | 1 h | M2 |
| `CodeTable` | titre de code normalisé → `LegiTextId`, `etat` | ~150 entrées | 7 j, rafraîchissement mono-vol ; en cas d'échec, l'ancienne table est gardée jusqu'à 7 j de plus, sinon `Upstream` | M6 |

Implémentation : `std::sync::Mutex<…>` avec horodatage `MonoClock`, sans dépendance supplémentaire. Le mémo n'est jamais tenu pendant un `await`.

### 12.4 Audit : rien ici

- L'adapter n'écrit aucune ligne d'audit et ne mesure pas `duration_ms`. Le décorateur `Audited` de `lf-application` enveloppe **chaque** exécution d'outil, succès comme erreurs, avec une durée réelle (00 §3, corrige D-02 et D-03).
- Exigence sur l'adapter : le `Display` de `CorpusError`, réutilisé dans le champ `error` de l'audit (≤ 500 caractères, 00 §7), ne contient ni secret, ni jeton, ni URL avec paramètres, ni valeur du corps (§5.1, §8.5).
- Le code d'erreur de l'audit (`error_code`) découle de la variante de `CorpusError` via `AppError` (02).

### 12.5 Journalisation (`tracing`, stderr uniquement)

- Un span `piste.call` par appel du port : `endpoint` (gabarit, ex. `/consult/juri`), `env`, `attempts`, `status` final, `duration_ms`, `bytes`, `mojibake_repaired`. Un événement `debug` par tentative : statut, durée, attente prévue. Corrige D-46.
- **Jamais** journalisés, à aucun niveau : corps de requête et de réponse, texte de la requête de recherche (seule sa longueur l'est), jeton, secret, `client_id`, en-têtes.
- Les identifiants de documents (publics) peuvent apparaître au niveau `debug`.
- Les journaux `debug`/`trace` de reqwest et hyper ne contiennent pas les corps. Le filtre par défaut (`LF_LOG`, défini dans `lf-app`) reste au niveau `info` pour ces crates.

---

## 13. Tests

### 13.1 Infrastructure

- **wiremock** 0.6.5 : un `MockServer` par test. La `PisteConfig` de test pointe `oauth_url` vers `{server}/oauth/token` et `base_url` vers `{server}/dila/legifrance/lf-engine-app` (`ConfigWarning::InsecureLoopback`).
- **Temps** : on ne gèle **pas** l'horloge `tokio` (`start_paused`) pendant des E/S réelles avec wiremock. L'avance automatique d'une horloge gelée pourrait déclencher à tort le délai de reqwest pendant l'attente d'une socket. Ce comportement est **NON VÉRIFIÉ** [v:wiremock-temps-pause], et la conception l'évite. À la place :
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
| `oauth_invalid_client_401_maps_to_auth_no_retry` | token : 401 `{"error":"invalid_client"}` | `Auth`, 1 seule requête token, message qui évoque l'environnement | D-10 |
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
| `429_retry_after_http_date_is_honoured` | `Retry-After: <date HTTP + 3 s>` | attente ≈ 3 s | D-12 |
| `429_retry_after_beyond_deadline_fails_fast` | `Retry-After: 120`, délai 60 s | `RateLimited { retry_after: 120 s }`, aucune attente | §5.2 |
| `429_three_times_maps_to_rate_limited` | 429 ×3 | `RateLimited` ; exactement 3 requêtes | 00 §8 |
| `500_then_200_succeeds_after_backoff` | 500, 200 | succès ; attente notée dans [0,5 s ; 1 s) | P-024 |
| `503_three_times_maps_to_upstream_retryable` | 503 ×3 | `Upstream { 503, retryable: true }` ; attentes [0,5 ; 1) puis [1 ; 2) s | P-024 |
| `400_is_not_retried` | 400 sans marqueur | `InvalidRequest` ; `expect(1)` | §5.1 |
| `403_maps_to_auth` | 403 | `Auth`, sans réessai | §5.1 |
| `404_on_search_is_config_not_not_found` | `/search` → 404 | `Config` | D-10 |
| `404_on_consult_official_host_is_not_found` / `404_html_is_config` | — | `NotFound` / `Config` | §11.2 |
| `redirect_is_not_followed` | 302 `Location` vers un 2e mock | `Upstream { 302 }` ; 2e mock `expect(0)` | §4.2 |
| `network_error_is_retried` | port local fermé | 3 tentatives, puis `Upstream { status: None, retryable: true }` | 00 §8 |
| `request_timeout_maps_to_timeout` | réponse retardée de 1 s, `request_timeout = 200 ms` | `Timeout` après 3 tentatives (ou à l'échéance) | §5.1 |
| `tool_deadline_caps_retries` | délai 1 s (virtuel), 503 permanent | rend la main avant l'échéance virtuelle ; aucune attente au-delà | D-14 |
| `deadline_from_task_local_is_used` | `Deadline` posé par une portée de test | l'adapter s'y conforme | §5.3 |
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
| `search_{loda,code,juri,constit,cetat,cnil,kali,acco}_fixture_maps` (8 tests) | `search_*.json` | fond API ; `typeRecherche` ; `DATE_VERSION` à minuit Paris (LODA, CODE) ; `filtres: []` ailleurs ; `pageNumber`, `pageSize` | P-020, P-032 |
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

`config_rejects_http_scheme_except_loopback` ; `config_rejects_userinfo_query_fragment` ; `config_warns_on_custom_host` ; `config_rejects_env_mismatch` (hôte OAuth sandbox avec base production) ; `fr_eli_variables_are_read_with_deprecation_warning` (dans `lf-app`) ; `debug_of_config_redacts_secret` ; `bearer_header_is_sensitive` ; `error_messages_never_contain_secret_or_token` (secret et jeton aléatoires, contrôle des journaux capturés, des `Display` et des `Debug`) (D-40).

### 13.3 Contrats sur les fixtures de la référence

Les 16 fixtures de la référence (Apache-2.0, attribution dans `NOTICE`) sont copiées **sans modification** dans `tests/fixtures/reference/`. Chacune reçoit un `.meta.toml` : `source = "matematicsolutions/fr-eli-mcp@d3b5b13"`, `endpoint`, `fond`, `sha256_raw`, `known_issues` (par exemple `["mojibake", "truncated:texte@600"]`, d'après la recherche 03 §2), `env = "sandbox"`, `captured_at = "inconnu"`.

| Test | Contenu | Couvre |
|---|---|---|
| `dto_parses_every_fixture` | les 16 fixtures se désérialisent dans leur DTO | D-11, D-23 |
| `mapping_snapshots` | snapshot `insta` (feature `json`, maps triées) de chaque entité produite | P-038 |
| `mapping_matches_golden` | valeurs de [annexes/donnees/parite-golden.json](annexes/donnees/parite-golden.json), divergences volontaires exceptées (liste de [annexes/A-parite.md](annexes/A-parite.md) : typographie des citations, sentinelles 2999 → `None`, `etat` LODA, textes réparés des 6 fixtures en mojibake, TOC avec sections…) | P-030 à P-039 |
| `no_mojibake_after_mapping` | aucune chaîne d'une entité produite ne contient de motif | §10 |
| `fixtures_have_meta_sidecar` ; `fixture_sha256_matches_meta` | intégrité des fixtures | D-35 |
| `toc_preserves_section_titles` (consult_loda.json : 26 sections + 62 articles = 88 entrées) ; `kali_inline_content_is_available_without_extra_call` (`expect(0)` sur `getArticle`) | TOC | D-25 |
| `null_list_is_empty_vec`, `missing_list_is_empty_vec`, `empty_string_is_none`, `raw_date_accepts_ms_numeric_string_ymd_iso`, `sentinel_2999_and_2222_are_none`, `negative_ms_1804`, `bool_is_not_a_date`, `suffix_is_version_date`, `titres_tm_is_renamed`, `multiple_versions_alias`, `idcc_accepts_string_and_number`, `loda_status_falls_back_to_juris_state` | helpers et pièges (§8.4) | D-20, D-21, D-24 |
| fuzz « jamais de panique » : mutations des fixtures (`null` injectés, champs supprimés, types changés) | `Ok` ou `Err(Decode)`, jamais de panique | D-23 |

Limites connues de ces fixtures (élaguées, tronquées, sans métadonnées de capture, recherche 03 §2) : elles valident les **formes**, pas les **volumes**. Les budgets de M5 se mesurent sur des fixtures recapturées.

### 13.4 Enregistrement de nouvelles fixtures : `cargo xtask record-fixture`

```text
cargo xtask record-fixture --endpoint /consult/juri --body '{"textId":"JURITEXT000051743650"}' \
    --name juri/JURITEXT000051743650 [--env sandbox|production] [--allow-error] [--redact <pointeur JSON>] [--force]
cargo xtask record-fixture --plan crates/lf-adapter-piste/tests/fixtures/plan.toml   # lot
```

- **Identifiants** : `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` uniquement, jamais en argument. `sandbox` par défaut ; `--env production` est un choix explicite (CGU et compte, Q3).
- **Client** : xtask réutilise le client de l'adapter (même User-Agent, même OAuth, même limiteur) au moyen d'une API brute exposée derrière une feature `record` de `lf-adapter-piste`, qui renvoie statut, en-têtes utiles et octets. Proposition : 00 §2 ne fixe pas les dépendances de `xtask`. Débit de 1 req/s.
- **Sorties** : `tests/fixtures/recorded/<endpoint>/<name>.json`, contenant les **octets bruts reçus**, non reformatés et non réparés, et `<name>.meta.toml` :

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
mojibake_detected = false           # résultat de has_latin1_mojibake sur le corps brut
redactions = []
tool = "xtask record-fixture 0.1.0"
license = "Licence Ouverte 2.0 — source : Légifrance / DILA"
```

- **Caviardage** : aucun secret ne peut figurer dans un fichier, car en-têtes et jeton ne sont jamais écrits. Par défaut, seul `acco.data` (base64 du `.docx`, volumineux) est remplacé par `"<redacted: N bytes base64>"`. Le fichier devient alors une dérivée : `sha256_raw` garde l'empreinte des octets d'origine et `redactions` liste les pointeurs JSON. `--redact` permet d'en ajouter. Les données sont publiques (Licence Ouverte 2.0), pseudonymisées par la DILA pour JURI et occultées pour la CNIL (D-45).
- `--allow-error` enregistre aussi les réponses 4xx et 5xx, pour les sondes « introuvable » du §11.
- Refus d'écraser un fichier sans `--force`. Le plan de lot par défaut couvre les 6 fixtures en mojibake (mêmes identifiants), des versions **non tronquées** de `consult_juri` (JURI), `consult_cnil` et `lawDecree`, un `getArticle` avec `texteHtml`, les sondes « introuvable » de chaque endpoint, la page 2 d'une recherche CODE, et une recherche avec chaque facette de date.

### 13.5 Smoke tests live (opt-in)

- Fichier `crates/lf-adapter-piste/tests/live.rs`. Chaque test porte `#[ignore = "live: needs LF_PISTE_CLIENT_ID/SECRET"]`. Sans identifiants, le test sort aussitôt avec un message sur stderr.
- Exécution : `LF_PISTE_ENV=sandbox cargo test -p lf-adapter-piste --test live -- --ignored --test-threads=1` (séquentiel, 200 ms au moins entre deux appels, comme les tests d'intégration de pylegifrance, recherche 05 §3.3). Hors CI par défaut (00 §10) ; un job manuel facultatif avec secrets est décrit dans [06-qualite.md](06-qualite.md).

| Test live | Vérifie | Libellés traités |
|---|---|---|
| les 9 scénarios de la référence (recherche 01 §11.7 : LODA, CODE, JURI, CONSTIT, CETAT, CNIL, KALI, ACCO, puis consultation des hits) | parité de bout en bout | P-060 |
| `live_token_reuse_and_expires_in` | `expires_in`, `scope`, réutilisation | [v:oauth-reponses] |
| `live_unknown_ids_classification` | réponse de chaque endpoint pour `…000000000000` et `…999999999999` (enregistrée par xtask) | [v:introuvable-par-endpoint], [v:forme-404-piste], [v:corps-erreur-piste] |
| `live_no_mojibake_on_reference_ids` | §10.7 | [v:mojibake-origine], [v:json-echappements-unicode] |
| `live_singledate_midnight_paris_semantics` | §6.3 (suffixe de version du hit pour `version_date = D`) | [v:singledate-minuit-paris] |
| `live_type_pagination_page2_code` | §6.6 | [v:typepagination-page2] |
| `live_date_facets_reduce_total` | pour chaque facette du §6.5, le total diminue ; bornes incluses | [v:filtres-dates-effet] |
| `live_match_modes_totals` | `any` ≥ `all` ≥ `exact` sur une même requête | [v:mode-all-semantique] |
| `live_gzip_negotiated` | en-tête `Content-Encoding` | [v:piste-gzip] |
| `live_texte_html_presence` | `texteHtml` sur `getArticle` et `juri` (JURI) | [v:texte-html-getarticle-juri] |
| (M6) `live_find_article_today_and_past`, `live_exact_number_searches` | §7.2 | [v:idandnum-comportement], [v:recherche-exacte-par-numero], [v:num-article-forme-api], [v:endpoints-m6-reponses] |
| (M7) `live_versions_links_toc_suggest` | §7.3 | [v:endpoints-m7-reponses], [v:lawdecree-sur-code] |

Ni 429 ni `Retry-After` ne sont provoqués volontairement : ce serait contraire à l'esprit des CGU. [v:retry-after-piste] et [v:quotas-piste] se vérifient par observation fortuite (journal `warn`) et par la console PISTE.

### 13.6 Couverture des défauts

| Défauts | Tests (§13.2, §13.3) |
|---|---|
| D-01 | `search_request_matches_reference_json`, `paris_midnight_vectors` (la clé de cache elle-même est testée dans `lf-adapter-cache`) |
| D-04, D-18, D-39, D-40 | tests de configuration, `missing_credentials_makes_no_network_call`, `error_messages_never_contain_secret_or_token` |
| D-06 | client unique (test de construction : un seul `Client` dans `Inner`) |
| D-09, D-32 | `search_page_2_sends_page_number_2`, `type_recherche_follows_match` |
| D-10 | `oauth_*_maps_to_*`, `404_on_search_is_config_not_not_found` |
| D-11, D-23, D-24 | tests de corps, `dto_parses_every_fixture`, fuzz |
| D-12, E-20 | `api_401_*`, `401_then_503_then_200_*`, `429_*`, `concurrent_calls_share_one_token_refresh` |
| D-13 | `rate_limiter_spaces_requests`, `concurrency_cap_is_respected` |
| D-14 | `tool_deadline_caps_retries` |
| D-20, D-21, D-25, D-26, D-28, D-30 | tests de contrat et snapshots |
| D-35 | méta-données des fixtures, tests de mojibake |
| D-41 | `oversized_body_is_rejected`, `gzip_body_is_limited_after_decompression` |
| D-43 | `user_agent_contains_crate_version` |
| D-46 | span `piste.call` vérifié par le subscriber de test |

---

## 14. Sécurité

### 14.1 Secrets

- `client_secret` et jeton en `SecretString`, exposés à deux endroits seulement (§3.6) ; en-tête `Authorization` marqué sensible ; `Debug` de `PisteConfig` expurgé ; jamais écrits sur disque, ni dans le cache, ni dans l'audit, ni dans les fixtures, ni dans les journaux (P-026).
- Mode HTTP partagé (M8, Q4) : le jeton et le secret ne sont jamais renvoyés à un client MCP ; aucune transmission de jeton entrant vers PISTE (00 §6).
- Une compromission (« surconsommation », « publication des identifiants ») impose de réinitialiser les identifiants sur PISTE (CGU 2022, recherche 05 §4.1). La documentation utilisateur (08) le rappelle.

### 14.2 TLS

- rustls uniquement, avec le fournisseur choisi au spike M0 (§4.1) et la vérification par `rustls-platform-verifier` (magasin du système) ; TLS 1.2 minimum (défaut de rustls) ; `https_only`.
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

**M0 — Fondations (part de ce crate)**

- [ ] Spike TLS : options A et B compilées pour les 7 cibles cargo-dist ; poignée de main TLS testée ; choix consigné dans ADR-0013 et dans 09 ([v:tls-fournisseur-cross]).
- [ ] Spike proxy : prise en compte de `HTTPS_PROXY` et `NO_PROXY` avec les features retenues ([v:reqwest-proxy-env]).
- [ ] Squelette du crate compilé avec les dépendances du §1.4 ; `cargo xtask check-deps` vert (pas de `rmcp`, pas d'autre adapter).
- [ ] API de `backon` 1.6 et de `governor` 0.10.4 confirmées à la compilation ([v:backon-api], [v:governor-api]).

**M2 — Adapter PISTE (v0.1)**

- [ ] `PisteConfig` et validation du §2.3 ; démarrage sans réseau ; identifiants absents ⇒ `Config` sans E/S.
- [ ] `TokenManager` mono-vol, renouvellement anticipé, générations, annulation sûre ; classification du §3.5.
- [ ] Client unique du §4.2 ; lecture bornée ; décodage strict ; réparation du mojibake (§10) avec compteur et `warn`.
- [ ] Politique de réessai du §5.2, délai global du §5.3, `RateGate` du §5.4 et §5.5.
- [ ] Les 7 endpoints v0.1 du §7.1 ; payload `/search` du §6 (facettes de date, `typePagination` et `PaginationMemo`).
- [ ] DTO du §8.3 (v0.1) ; mapping du §9 vers les entités de 03.
- [ ] Classification « introuvable » du §11, complétée par les résultats de `live_unknown_ids_classification`.
- [ ] Tous les tests des §13.2 et §13.3 verts sous Linux, macOS et Windows, sans réseau.
- [ ] `cargo xtask record-fixture` opérationnel ; plan de lot exécuté en sandbox (Q3) ; résultats reportés dans le registre V-xx.
- [ ] Smoke tests live du §13.5 (partie v0.1) exécutés au moins une fois en sandbox ; résultats consignés dans 09.

**M5 — Économie de contexte**

- [ ] Pagination de la recherche vérifiée en direct : `typePagination` page 2 ([v:typepagination-page2]) et fenêtre profonde ([v:pagination-profonde]) ; décision sur le plafond de 10 000.
- [ ] Borne de corps ajustée sur les fixtures recapturées ([v:taille-max-textes]).

**M6 — Citations**

- [ ] Endpoints M6 du §7.1 ; `find_article` et `find_by_number` (§7.2) ; `CodeTable`.
- [ ] DTO M6 ; scénarios wiremock pour chaque endpoint ; smoke tests M6.

**M7 — Navigation**

- [ ] Endpoints M7 du §7.1 ; `article_versions`, `related_links`, `list_codes` ; `get_act` sur un code via `tableMatieres` (si [v:lawdecree-sur-code] le justifie) ; fond `Jorf` en recherche.
- [ ] `fr_search_all` : plafond de concurrence vérifié (`search_all_respects_concurrency_cap`).

---

## 16. Questions ouvertes propres à l'adapter

Elles sont reportées dans [09-risques-et-questions.md](09-risques-et-questions.md). Les questions communes Q1 à Q10 de 00 restent la référence : Q3 conditionne les tests live, Q4 le partage des quotas en mode HTTP, Q10 le mode de recherche par défaut.

1. **`singleDate`** : garder minuit Europe/Paris (00 §8) ou passer à **midi** Europe/Paris, tout aussi stable sur la journée et correct quelle que soit l'interprétation de l'instant par PISTE (§6.3) ? À trancher après [v:singledate-minuit-paris].
2. **Variables de configuration** à inscrire dans 00 §1 : `LF_PISTE_RPS`, `LF_PISTE_BURST`, `LF_PISTE_CONCURRENCY`, `LF_PISTE_MOJIBAKE_REPAIR` (noms proposés).
3. **500** : garder 3 tentatives (00 §8) ou se limiter à 2, et rendre `retryable: false` après échec (02 §9.2 dit `true`), puisque des 500 seraient déterministes selon des clients tiers ([v:500-requete-invalide]) ?
4. **Temps restant** : `Deadline` portée par une variable locale de tâche définie dans `lf-application` (§5.3), en complément de la garde extérieure de 02 §7.1 : à valider dans [02-architecture.md](02-architecture.md).
5. **Rendu de `InvalidRequest`** : `upstream_error` (02, parité S10) ; faut-il un `invalid_arg` quand la cause est vraisemblablement l'entrée utilisateur ?
6. **Plafond de pagination profonde** (`page × page_size ≤ 10 000`) : à adopter en M5 ?
7. **Méthode de port pour `/suggest`** (endpoint M7 de 00 §8 sans méthode en 00 §3) ; recherche par `typeChamp = ECLI` pour `fr_resolve` (absente de la liste de 00 §8) ; consultation d'un `JORFTEXT` (`/consult/jorf`, présent dans le Swagger mais absent de 00 §8) pour le fond `Jorf` en M7.
8. **`list_codes`** : inclure les codes abrogés (proposé : oui, avec `etat`) pour résoudre des citations anciennes ?
9. **`check-config --online`** : autoriser une requête token pour valider les identifiants (proposé : oui, option explicite) ?
10. **Enregistrement de fixtures en production** : acceptable au regard des CGU et du compte (Q3) ?

---

## Points NON VÉRIFIÉS de ce document

| Étiquette | Vérification à faire |
|---|---|
| [v:quotas-piste] | Valeurs des quotas PISTE (sandbox et production), granularité (par application, par méthode), pour calibrer `governor`. |
| [v:oauth-production-live] | L'endpoint token et la base API de production répondent comme documenté avec une application de production. |
| [v:sandbox-prod-donnees] | Ampleur des écarts de données entre sandbox et production. |
| [v:oauth-reponses] | Statuts et corps réels des erreurs de l'endpoint token ; valeurs de `expires_in` et `token_type`. |
| [v:401-sous-throttling] | PISTE renvoie-t-il `401 invalid_client` pendant une limitation de débit ? |
| [v:tls-fournisseur-cross] | Exigences de compilation d'`aws-lc-sys` par cible ; installation du fournisseur `ring` avec reqwest 0.13 `rustls-no-provider`. |
| [v:reqwest-proxy-env] | Prise en compte de `HTTPS_PROXY` et `NO_PROXY` par reqwest 0.13 avec les features retenues. |
| [v:piste-gzip] | PISTE compresse-t-il les réponses (`Content-Encoding: gzip`) ? |
| [v:taille-max-textes] | Tailles maximales réelles des réponses (gros textes, codes, TOC) au regard de la borne de 16 Mio. |
| [v:acco-data-attachment] | Présence et taille de `data` et `attachment` dans `/consult/acco`. |
| [v:retry-after-piste] | Présence et format (secondes ou date HTTP) de `Retry-After` sur les 429 et 503 de PISTE. |
| [v:500-requete-invalide] | Une facette ou un champ inconnu provoque-t-il un 500 plutôt qu'un 400 ? |
| [v:backon-api] | Noms et sémantique de l'API backon 1.6 (`when`, `adjust`, `sleep`, `notify`, gigue). |
| [v:governor-api] | Constructeurs de `Quota` et `RateLimiter::direct_with_clock` avec une horloge `Duration`, à la compilation. |
| [v:delai-clients-mcp] | Délais imposés par Claude Code et VS Code à un appel d'outil, à comparer aux 60 s de `LF_TOOL_DEADLINE_S`. |
| [v:singledate-minuit-paris] | Avec `singleDate` à minuit Paris (22 h ou 23 h UTC la veille), un texte entrant en vigueur le jour J est-il retenu le jour J ? |
| [v:mode-all-semantique] | Sens de `TOUS_LES_MOTS_DANS_UN_CHAMP` avec `typeChamp = ALL`. |
| [v:filtres-dates-effet] | Effet réel des facettes de date du §6.5, bornes incluses, borne lointaine, combinaison avec `DATE_VERSION`. |
| [v:typepagination-page2] | Effet de `DEFAUT` ou `ARTICLE` en page 2 (CODE) et cohérence de `has_more`. |
| [v:pagination-profonde] | Fenêtre maximale de résultats accessibles par `/search` (10 000 ?) et réponse au-delà. |
| [v:syntaxe-moteur-valeur] | Syntaxe interprétée dans `valeur` (guillemets, jokers, `*`). |
| [v:recherche-exacte-par-numero] | Comportement des recherches `EXACTE` sur `NUM`, `NUM_ARTICLE`, `NUM_AFFAIRE`, `NUM_DEC`, `NUM_DELIB`, `IDCC` (casse, tirets, zéros). |
| [v:nom-code-filtre] | Le filtre `NOM_CODE` (ou `TEXT_NOM_CODE`) sur `CODE_DATE` provoque-t-il un 500 ? |
| [v:idandnum-comportement] | Comportement de `/consult/getArticleWithIdAndNum` (forme de `num`, réponse sans article, versions différées). |
| [v:endpoints-m6-reponses] | Formes réelles des réponses de `getJoWithNor`, `kaliContIdcc` et `/list/code`. |
| [v:endpoints-m7-reponses] | Formes et tailles réelles des réponses de `tableMatieres`, `canonicalArticleVersion`, `getArticleByCid`, `relatedLinksArticle` et `suggest`. |
| [v:json-echappements-unicode] | PISTE sert-il les caractères non ASCII bruts ou échappés en `\uXXXX` (angle mort de la réparation au niveau du corps) ? |
| [v:serde-json-recursion] | Valeur exacte de la limite de récursion par défaut de serde_json. |
| [v:introuvable-par-endpoint] | Réponse de chaque consultation (CONSTEXT compris) à un identifiant bien formé mais inexistant. |
| [v:forme-404-piste] | Forme d'un 404 PISTE (routage ou ressource), pour la règle du §11.2. |
| [v:corps-erreur-piste] | Forme générale des corps d'erreur de l'API (champs `error`, `status`, `message`). |
| [v:forme-403-piste] | (02) Sens exact d'un 403 de l'API Légifrance (application non abonnée ou autre cause). |
| [v:wiremock-temps-pause] | Interaction entre une horloge `tokio` gelée et les E/S réelles de wiremock (motive le choix de `ManualClock`). |
| [v:mojibake-origine] | (03) Le mojibake vient-il de la capture ou de l'API ? |
| [v:lawdecree-sur-code] | (03) Comportement et taille de `/consult/lawDecree` avec le `LEGITEXT` d'un code. |
| [v:texte-html-getarticle-juri] | (03) Présence de `texteHtml` sur `/consult/getArticle` et sur `/consult/juri` pour JURI. |
| [v:convention-horaire-timestamps] | (03) Tous les horodatages de date sont-ils à minuit UTC ? |
| [v:longueur-max-query] | (03) Longueur maximale de `query` acceptée par `/search`. |
| [v:num-article-forme-api] | (03) Forme attendue par `NUM_ARTICLE`. |

---

## Sources

**Plan et décisions**
- [00-decisions.md](00-decisions.md) : §1 (variables), §2 (crates et dépendances), §3 (ports), §5 (erreurs), §6 (démarrage sans réseau), §7 (paramètres de `fr_search` et des outils), §8 (adapter PISTE), §9 (cache), §10 (tests), §11 (jalons), §13 (Q1 à Q10).
- [02-architecture.md](02-architecture.md) : §3.3 (fiche du crate : `PisteCorpus::new` sans E/S, `check_credentials`, `mojibake_repairs`, `RateQuota`), §5.1 (`LegalCorpus`, `CorpusError`, `find_by_number(&NumberQuery)`), §7.1 (délai global), §7.2 (clés et TTL du cache), §9.2 (correspondance des erreurs).
- [03-domaine.md](03-domaine.md) : §1.5–§1.6 (frontières, `repair_latin1_mojibake`), §2.4 (`VersionedId`), §3.1 (`Fond`, `PaginationMode`), §3.3 (`status_of`), §4 (entités), §6 (dates, minuit Paris), §7.1 (ordre mojibake/NBSP), §9 (erreurs), étiquettes partagées.

**Rapports de recherche** (`docs/recherche/`)
- [01-parite.md](../recherche/01-parite.md) : §2.4–§2.6 (erreurs, scénarios S1–S17), §3.1–§3.7 (payloads, corps des consultations), §6.1–§6.5 (client de la référence), §11.4–§11.7 (tests, golden, smoke), §13 (fixtures), §14 (E-01 à E-36), §15 (P-014, P-020 à P-026, P-030 à P-039, P-060), §16.
- [02-defauts.md](../recherche/02-defauts.md) : §2.a–§2.d ; §3.3 (D-04, D-05, D-18, D-39) ; §3.4 (D-06, D-10, D-11, D-12, D-13, D-14, D-41) ; §3.5 (D-07, D-08) ; §3.6 (D-19 à D-26) ; §3.11 (D-35, D-36, D-37) ; D-40, D-43, D-46 ; §5 (mojibake) ; §6 (matrice de tests) ; §7.
- [03-dto.md](../recherche/03-dto.md) : §2 (fiabilité des fixtures), §3 (payloads, identifiant inconnu), §4–§10 (formes des réponses), §11 (anomalies, dont §11.1 mojibake), §12 (DTO, helpers, mapping), §14–§15.
- [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) : §4.1 (versions de reqwest, secrecy, governor, backon, wiremock, rustls, aws-lc-rs, serde_path_to_error), §4.2 (reqwest 0.13 et TLS), §4.3, §5.3, §6 points 5, 6, 8.
- [05-piste-dila.md](../recherche/05-piste-dila.md) : §0 (Swagger 2.4.2, copies sandbox et prod identiques), §1.1–§1.14 (catalogue des endpoints, pièges de données), §2 (options de recherche, facettes par fond, pagination), §3 (OAuth, sandbox/production, quotas, codes d'erreur), §4.1–§4.3 (CGU), §5.1, §7, §8.

**Référence Python** (`matematicsolutions/fr-eli-mcp@d3b5b13`, lecture seule)
- `src/fr_eli_mcp/client.py:28-35` (URLs par défaut, délais, User-Agent, statuts réessayés, marge de 60 s), `:42-58` (variables), `:61-99` (`TokenManager`), `:117-142` (client par appel), `:149-183` (boucle `_post`), `:187-208` (endpoints et corps).
- `src/fr_eli_mcp/server.py:91-100` (`_FOND_MAP`), `:110-122` (`_map_upstream`), `:125-136` (`_today`, `_now_ms`), `:133-156` (payload `/search`).
- `DISCOVERY.md:9-13` (OAuth, scope, production), `:22-33` (`DATE_VERSION` requis), `:78-82` (`/consult/juri` pour les trois juridictions) ; `SOURCES.md:12` (identifiants sandbox refusés en production, Judilibre), `:14-15` (KALIARTI sur `getArticle`, ACCO).
- Fixtures `tests/fixtures/*.json` (16 fichiers), dont search_code.json (`typePagination: "ARTICLE"`), consult_juri.json, consult_loda.json, consult_article.json (mojibake en caractères bruts, aucun `\uXXXX` : contrôle de ce document).

**Documentation officielle DILA** (copies archivées dans `pylegifrance/docs/raw/legifrance/`, via la recherche 05)
- Swagger 2.4.2 `legifrance.json` : `paths` des 16 chemins du §7.1 et `definitions` (`SearchRequestDTO`, `RechercheSpecifiqueDTO`, `ChampDTO`, `CritereDTO`, `FiltreDTO`, `DatePeriod`, `LawDecreeConsultRequest`, `ArticleRequest`, `JuriConsultRequest`, `CnilConsultRequest`, `KaliTextConsultRequest`, `AccoConsultRequest`, `ArticleConsultWithIdAndNum`, `JorfConsultWithNorRequest`, `KaliContConsultIdccRequest`, `CodeListRequest`, `CodeListResult`, `ArticleCidRequest`, `ArticleVersion`, `RelatedLinksArticleRequest`, `LiensRelatifsDTO`, `LegiSommaireConsultRequest`, `SuggestSupplyRequest`, `SuggestValue`, `ConsultKaliContResponse`), `info.description`, `securityDefinitions` (vérifiés par script dans ce document).
- `exemples-d-utilisation-de-l-api.md:107-163` (OAuth, en-têtes), `:221`, `:497-570` (article d'une loi par `NUM_ARTICLE`, `typePagination: "ARTICLE"`), `:535-553`, `:815`, `:1011-1019`, `:1027`, `:1045-1119`, `:1089-1097`, `:1209`.
- `description-des-tris-et-filtres-de-l-api.md:51-52, 78, 99-112, 154-165, 178-186, 206-216, 229, 249-265, 277-288, 301-307` (facettes et tris par fond).
- `cgu-legifrance-api.md:171-183, 187-202, 212-231` (clés, disponibilité, quotas).

**Code tiers lu** (clones en lecture seule)
- reqwest 0.13.5 (https://github.com/seanmonstar/reqwest) : `Cargo.toml:39-80` (features), `src/lib.rs:205-217`, `src/async_impl/client.rs:1392-1478` (redirections, réessais, proxy, délais), `:2262` (`https_only`), `src/async_impl/request.rs:273-299` (`bearer_auth` sensible, délai par requête), `src/async_impl/response.rs:90, 290-351` (`content_length`, `chunk`), `src/error.rs:91-201` (`without_url`, `is_timeout`), `src/redirect.rs:155-165, 239`, `src/retry.rs:1-104`.
- governor 0.10.4 (https://github.com/boinkor-net/governor, commit 9f3a79d) : `src/state/direct.rs:43-58, 73`, `src/state/direct/future.rs:11, 30-56` (`until_ready` sur `futures_timer::Delay`), `src/clock.rs:55-117` (traits `Clock`, `Reference`, implémentation pour `Duration`).
- pylegifrance (https://github.com/rdassignies/pylegifrance, d8e0e59) : `pylegifrance/fonds/juri.py:116-135, 760-840` (marqueur « L'expression à valider est fausse »), `tests/unit/fonds/test_juri_verification_endpoints.py:104-121` (forme du corps 400).
- Index crates.io (copies `index04/*.jsonl`, 2026-10-15) : backon 1.6.0 (features), governor 0.10.4, secrecy 0.10.3, serde_path_to_error 0.1.20, reqwest 0.13.5, rustls 0.23.46, wiremock 0.6.5.

**Calculs de ce document** : vecteurs de minuit Europe/Paris en epoch ms (Python `zoneinfo`, base IANA) ; recherche de séquences `\uXXXX` dans les 16 fixtures ; extraction des chemins et définitions du Swagger 2.4.2 par script (`python3 -I`).
