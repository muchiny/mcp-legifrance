# 02 — Audit critique de la référence Python `fr-eli-mcp` (commit d3b5b13, 2026-09-24)

> Axe : bugs, failles de conception, sécurité, performance, maintenabilité — pour que la réécriture Rust (workspace hexagonal `domain` / `application` / `adapter-legifrance` / `adapter-audit` / `adapter-mcp` / `app`) ne les reproduise pas.
> Rédigé le 2026-10-15. Langue : français ; identifiants techniques laissés tels quels.

---

## 0. Méthode, périmètre, conventions

**Sources lues intégralement** (dépôt en lecture seule `/home/user/matematicsolutions/fr-eli-mcp`, clone superficiel à 1 commit, `git log` = `d3b5b13 test: lock tool descriptors against silent drift`) :
`src/fr_eli_mcp/{server,client,citations,models,cache,audit,runtime,coverage,__main__,__init__}.py`, `runtime/fr-runtime.json`, `tests/*.py`, `tests/tools.lock.json`, les 16 `tests/fixtures/*.json`, `README.md`, `CONSTITUTION.md`, `DISCOVERY.md`, `SOURCES.md`, `pyproject.toml`, `Dockerfile`, `server.json`, `.mcp.json.example`, `.gitignore`, `.github/workflows/{ci,release}.yml`.

**Vérifications complémentaires** (clones publics en lecture seule sous `scratchpad/clones/`, aucun code exécuté) :
- `PrefectHQ/fastmcp` @ `e2fcc41` (2026-10-08) — gestion des erreurs d'outil et du contenu structuré.
- `encode/httpx` @ `b5addb6` (2026-02-23) — message de `HTTPStatusError`, `Response.json()`, `URL.__str__`.
- `grantjenks/python-diskcache` @ `ebfa37c` — valeurs par défaut, pickle, NFS, disque plein.
- `python/cpython` branche 3.13 (clone partiel) — `datetime.fromtimestamp` sous Windows.
- `modelcontextprotocol/modelcontextprotocol` (clone partiel) — spécification « tools ».
- `rdassignies/pylegifrance` @ `d8e0e59` (2026-08-19) — client tiers de la même API : enums `typeRecherche`, `pageSize` max, quotas PISTE, sentinelle 2999-01-01, endpoints.
- `chronotope/chrono` @ `6adaa52` (2026-08-02) — gestion des timestamps négatifs.
- Index sparse crates.io (2026-10-15) pour les versions de crates citées.

**Mesures sur fixtures** : scripts personnels (dans `scratchpad/scripts/`) lancés en `python3 -I`, lisant les JSON comme données. Pour les tailles « normalisées » et les cas limites, le module `citations.py` (486 lignes, lu en entier, imports `re`/`datetime`/`typing` uniquement, sans effet de bord) a été chargé par chemin depuis le scratchpad ; aucun autre module de la référence n'a été exécuté (ni serveur, ni client, ni réseau PISTE).

**Ce qui n'a pas pu être vérifié** : aucun accès à l'API PISTE/Légifrance (pas d'identifiants) ; `modelcontextprotocol.io` et `bugs.python.org` injoignables (DNS) ; `raw.githubusercontent.com` refusé par le proxy ; docs.rs bloqué. Tout ce qui en dépend est marqué **NON VÉRIFIÉ**.

**Gravité** :
- **bloquant** : viole la CONSTITUTION du projet, la sécurité/intégrité des textes juridiques, ou rend une fonctionnalité centrale fausse — ne doit en aucun cas être reproduit.
- **majeur** : erreur fonctionnelle, de performance ou de conception à fort impact (utilisateur, LLM, quota, maintenance).
- **mineur** : défaut localisé, cosmétique, ou cas limite.

**Attention aux numéros de ligne** : tous les `chemin:ligne` ci-dessous sont les numéros réels dans chaque fichier (vérifiés par `grep -n`).

---

## 1. Tableau de synthèse

| ID | Défaut (résumé) | Gravité | Composant Rust qui le neutralise |
|---|---|---|---|
| D-01 | Cache disque jamais réutilisé pour `fr_search` LODA/CODE (clé contient `singleDate=_now_ms()`) | majeur | décorateur `CachedLegalSource` (application) + clé canonique sans champs volatils |
| D-02 | Audit absent pour `invalid_arg`, `not_found` « normalisation », exceptions de parsing — violation CONSTITUTION art. 2 | **bloquant** | décorateur `AuditedUseCase` unique (application) + `adapter-audit` |
| D-03 | `duration_ms=0` sur toutes les erreurs ; durée « ok » qui exclut la normalisation | majeur | mesure `Instant` dans le décorateur, `Drop`-safe |
| D-04 | Manifeste distant (GitHub) qui peut changer `base_url` → Bearer + requêtes + intégrité des textes ; télémétrie d'activation ; téléchargé même si `FR_ELI_BASE_URL` est défini | **bloquant** | `app` : configuration 100 % locale, allowlist d'hôtes, pas de fetch distant |
| D-05 | Fetch `urllib` synchrone (timeout 15 s) dans un outil `async` → boucle bloquée | majeur | idem D-04 (supprimé) ; règle « aucune E/S bloquante dans tokio » |
| D-06 | `httpx.AsyncClient` + `diskcache.Cache` recréés à chaque appel d'outil | majeur | `reqwest::Client` unique, injecté par `app` |
| D-07 | Boilerplate audit/try/except dupliqué (7 `try/except`, 8 tools, 15 `audit.log`) | majeur | décorateurs + `thiserror` + mapping unique |
| D-08 | Erreurs = chaîne « [code] … » dans une `Exception` non-FastMCP ; code perdu si masquage ; traceback ERROR pour erreurs attendues | majeur | `AppError` typé → `CallToolResult{isError, structuredContent{code,…}}` |
| D-09 | Pas de pagination (`pageNumber: 1` figé), ni tri, ni filtres, ni facettes ; totaux articles CODE jetés | majeur | `SearchQuery` (domain) avec `page`, `sort`, `filters` |
| D-10 | Erreurs OAuth mal classées (401/400 → `upstream_error` ; 404 OAuth/base → `not_found`) | majeur | `adapter-legifrance` : `AuthError` distinct, 404 « endpoint » ≠ 404 « ressource » |
| D-11 | Corps non-JSON / `null` / types inattendus → exceptions non mappées et non auditées | majeur | DTO serde tolérants + `UpstreamError::InvalidResponse` |
| D-12 | Retry : le 401 consomme une tentative ; chaque tentative suivante force un nouveau token ; pas de `Retry-After`, pas de jitter | majeur | politique de retry explicite (budget 401 séparé, backoff+jitter, `Retry-After`) |
| D-13 | Aucune limitation de débit / concurrence côté client (quotas PISTE s/min/jour) | majeur | `RateLimitedSource` (governor) + sémaphore |
| D-14 | Aucun délai global par outil (pire cas > 200 s) | majeur | `tokio::time::timeout` par cas d'usage |
| D-15 | Cache : non namespacé par environnement, « not found » cachés 24 h, échec d'écriture = échec de l'outil, pickle, requêtes en clair sur disque | majeur | clé SHA-256 namespacée, fail-open, serde_json, TTL négatif court |
| D-16 | Journal d'audit : pas de rotation, pas de fsync, append multi-process non garanti, `OSError` brute | mineur | writer dédié (tâche + canal), rotation, verrou fichier |
| D-17 | Enregistrement d'audit pauvre : SHA-256 non salé (réversible), pas de code d'erreur, pas d'id de requête | mineur | schéma d'audit v2 (HMAC, `error_code`, `request_id`, `version`) |
| D-18 | Valeurs par défaut = **SANDBOX** PISTE (sans SLA), non signalé par `fr_coverage` ni par les réponses | majeur | `app` : environnement obligatoire, exposé dans chaque réponse |
| D-19 | « Aujourd'hui » calculé en UTC et non Europe/Paris | mineur | port `Clock` + `chrono-tz`/`jiff` Europe/Paris |
| D-20 | `ms_to_date` : pré-1970 → `None` sous Windows ; sentinelle 2999-01-01 exposée comme vraie date ; `bool` accepté | majeur | type `LegalDate` (domain) + désérialiseur dédié |
| D-21 | Formats de date hétérogènes non unifiés ; dates de recherche jetées ; `etat` LODA toujours `null` | mineur | désérialiseur unique ms / `YYYY-MM-DD` / ISO `+0000` |
| D-22 | Paramètre `date` non validé (format) → appel upstream, 500 éventuellement retenté 3× | mineur | `VersionDate::parse` (domain) → `invalid_arg` local |
| D-23 | Robustesse : un `null` dans une liste ou un payload `null` → `AttributeError` (démontré) | majeur | DTO serde `Option<Vec<Option<T>>>` + filtrage |
| D-24 | Modèles pydantic `str` stricts (int refusé) → `ValidationError` non gérée ; hétérogénéité réelle non observée dans les fixtures | mineur | désérialiseurs « int-ou-chaîne » |
| D-25 | Table des matières : 26 titres de sections perdus (aplatissement), `intOrdre` ignoré, doublons possibles, contenu KALI inline jeté (N+1 appels) | majeur | modèle `Toc` arborescent + tri `intOrdre` + dédup |
| D-26 | `texte` sans aucun saut de ligne (structure en alinéas perdue) alors que `texteHtml` la porte | majeur | conversion `texteHtml` → paragraphes (domain `Paragraph`) |
| D-27 | Citations « French convention » fautives : `decision`/`deliberation` sans accent, mois `fevrier/aout/decembre`, `1` au lieu de `1er`, regex « premier crochet » | majeur | `CitationFormatter` (domain) + tests de golden strings |
| D-28 | Routage URL par le champ `origine` du payload au lieu du préfixe d'ID ; `_ID_PREFIXES` sans `CONSTEXT` | mineur | `DecisionId` enum (domain) détermine l'URL |
| D-29 | URL `/codes/article_lc/` pour tout `LEGIARTI`, y compris articles de loi (`/loda/article_lc/` existe) | mineur | `ArticleUrl` selon nature du texte parent |
| D-30 | Contrat de citation non garanti (tous les champs `Optional`) ; `eli_uri == source_url` (nommage trompeur, doublon) | majeur | `CitationContract` non optionnel ; `eli: Option<Eli>` séparé |
| D-31 | Validation des IDs par préfixe seul ; `fond` et `page_size` non contraints dans le JSON Schema | mineur | newtypes + `schemars` enum/min/max |
| D-32 | Recherche figée `UN_DES_MOTS` (OU logique, bruit) ; longueur de requête non bornée | majeur | `SearchMode` (exacte / tous les mots / un des mots) + bornes |
| D-33 | Taille des réponses non maîtrisée : textes complets, doublon TextContent+structuredContent, ~22 Ko fixes de descripteurs/instructions, snippets jetés | majeur | retour progressif (résumé / `max_chars` / `offset` / `focus`) |
| D-34 | `fr_coverage` incomplet et incohérent (3 familles, 2 lacunes, `status="ok"` codé en dur) | majeur | `CoverageService` (application) calculé + test de cohérence outils↔familles |
| D-35 | Fixtures non fiables : 6/16 en mojibake (235 séquences), tronquées (600 car., listes ≤ 5), sans métadonnées ; assertion vide de sens | majeur | fixtures « brutes + sidecar », test anti-mojibake |
| D-36 | Aucun test unitaire client/cache/audit/runtime/mapping d'erreurs ; CI 3.12 seule ; ruff/mypy non exécutés | majeur | `wiremock` + `insta` + CI matrice (Linux/Windows/macOS) |
| D-37 | « Descriptor lock » fragile (méta FastMCP, schémas pydantic, deps non verrouillées), version non vérifiée, INSTRUCTIONS non verrouillées | mineur | snapshot `insta` des descripteurs normalisés + `Cargo.lock` |
| D-38 | E/S synchrones (SQLite, fichier d'audit, urllib) dans l'event loop | majeur (mode HTTP) / mineur (stdio) | `spawn_blocking` ou E/S async ; audit via tâche dédiée |
| D-39 | Fuite de ressources : cache ouvert puis `CredentialsError` ; fetch du manifeste même sans identifiants | mineur | construction unique au démarrage, échec précoce de config |
| D-40 | Messages d'erreur : URL complète (userinfo non masqué), bruit MDN, chemin local d'audit | mineur | messages d'erreur rédigés, `url` masquée |
| D-41 | Pas de borne de taille de corps (réponse API, manifeste gzip) | mineur | limite de corps dans l'adapter |
| D-42 | README annonce `PISTE_CLIENT_ID/SECRET` alors que le code lit `FR_ELI_CLIENT_ID/SECRET` | majeur (UX) | une seule source de vérité de config + test doc↔code |
| D-43 | Autres dérives doc/config (UA 0.3.0, `FR_ELI_ENV` inutilisé, CONSTITUTION obsolète, CHANGELOG absent, INSTRUCTIONS fausses) | mineur | tests de cohérence doc↔code |
| D-44 | Chaîne CI/release : `curl latest` sans checksum, actions non épinglées, token PyPI, Docker non épinglé/root, manifeste `latest` couplant toutes les versions | mineur | CI Rust épinglée, artefacts signés, pas de manifeste |
| D-45 | Pseudonymisation JURI / occultation CNIL non signalées ; attribution Etalab (date de mise à jour) non fournie | mineur | champs `redaction_note`, `last_update` |
| D-46 | Aucun log de diagnostic (aucun `logging`) ni corrélation | mineur | `tracing` sur stderr + `request_id` |
| D-47 | `fr_get_act` accepte tout `LEGITEXT…`, y compris un code renvoyé par `fr_search CODE` (comportement NON VÉRIFIÉ, risque de réponse énorme) | mineur | `TextId` distingue LODA / CODE ; outil code dédié paginé |

Décompte : 2 bloquants, 27 majeurs (dont D-38 conditionnel), 18 mineurs.

---

## 2. Vérification des 4 constats déjà annoncés à l'utilisateur

### 2.a « Le cache disque ne sert jamais pour LODA/CODE car la clé contient `singleDate=_now_ms()` »

**Verdict : CONFIRMÉ pour `fr_search` sur les fonds LODA et CODE ; INFIRMÉ pour la consultation (`fr_get_act`, `fr_get_text`)** — la formulation « jamais pour LODA/CODE » doit être restreinte à la *recherche*.

Preuves :
- `src/fr_eli_mcp/server.py:129-130` — `def _now_ms() -> int: return int(time.time() * 1000)`.
- `src/fr_eli_mcp/server.py:135-136` — `if api_fond in ("LODA_DATE", "CODE_DATE"): filtres.append({"facette": "DATE_VERSION", "singleDate": _now_ms()})`.
- `src/fr_eli_mcp/client.py:144-147` — la clé de cache est le corps JSON entier : `body = json.dumps(payload, sort_keys=True, ...)` ; `return f"POST {path} {body}"`.
- `src/fr_eli_mcp/client.py:151-154` (lecture) et `:171` (écriture, TTL `search` = 1 h, `cache.py:16`).

Raisonnement : la clé inclut `filtres[0].singleDate`, valeur en millisecondes qui change à chaque appel ; deux recherches identiques ne partagent donc jamais leur clé (sauf collision dans la même milliseconde). Chaque recherche LODA/CODE **écrit** pourtant une entrée (3 à 40 Ko d'après les fixtures, cf. §4) jamais relue : pur coût d'écriture disque, borné seulement par l'éviction diskcache (`size_limit` 1 Go par défaut, `diskcache/core.py:52`, politique `least-recently-stored` ligne 51).
Nuance : `fr_get_act` appelle `/consult/lawDecree` avec `{"textId", "date": "YYYY-MM-DD"}` (`client.py:190-193`) → clé stable sur la journée (TTL 24 h) ; `fr_get_text` utilise `{"id": …}` (`client.py:195-196`) → stable. Les fonds JURI/CONSTIT/CETAT/CNIL/KALI/ACCO n'ont pas de filtre daté (`server.py:134-136`) → recherches cachées 1 h.

→ Détail et mesure Rust : **D-01**.

### 2.b « L'audit n'enregistre pas `invalid_arg` / `not_found`, et les erreurs upstream sont loggées avec `duration_ms=0` »

**Verdict : CONFIRMÉ, avec deux précisions** : (1) un `not_found` dû à un HTTP 404 *est* journalisé (dans le `except`), seul le `not_found` issu de la normalisation (`norm is None`) ne l'est pas ; (2) les échecs de parsing/validation (pydantic, `AttributeError`) ne le sont pas non plus.

Preuves :
- Exigence : `CONSTITUTION.md:25-27` — « Every tool call MUST append one JSON line … Inability to write = the tool returns an error ». Promesse au LLM : `server.py:51` — « every tool call appends to `~/.matematic/audit/fr-eli-mcp.jsonl` ».
- `invalid_arg` levé **avant** tout `audit.log` : `server.py:191-197` (fr_search), `:242` (fr_get_act via `_require_prefix`, défini `:159-165`), `:282-289`, `:336-342`, `:383`, `:425`, `:495`.
- `not_found` de normalisation levé **avant** `audit.log` : `server.py:256-258` puis log `:260` ; idem `:302-304`/`:308`, `:355-357`/`:361`, `:396-398`/`:402`, `:438-440`/`:442`, `:508-510`/`:512`.
- `Act.model_validate(norm)` (`:259`, etc.) hors `try` → `ValidationError` non journalisée.
- `duration_ms=0` : `audit.py:81-83` (`self.duration_ms = 0` à l'init), `audit.py:89-90` (valeur calculée seulement dans `__exit__`) ; or `audit.log(... duration_ms=t.duration_ms or 0 ...)` est appelé **dans** le bloc `with timer() as t:` (`server.py:203-211`, idem `:246-254`, `:292-300`, `:345-353`, `:386-394`, `:428-436`, `:498-506`) → `__exit__` pas encore exécuté → toujours 0.
- Le test d'audit existant ne couvre que le chemin heureux de `fr_coverage` (`tests/test_coverage.py:62-78`).

→ **D-02** (bloquant) et **D-03** (majeur).

### 2.c « `runtime.py` télécharge au démarrage un manifeste distant qui peut changer `base_url`, vers lequel le Bearer token est envoyé (+ métrique d'activation) — contradiction avec CONSTITUTION art. 3 »

**Verdict : CONFIRMÉ sur le fond, avec une correction et deux aggravations.**
- *Correction* : le téléchargement n'a pas lieu « au démarrage » mais **paresseusement, au premier appel d'un outil qui construit un `LegifranceClient`** (tous sauf `fr_coverage`), une fois par processus (mémo global `_runtime`, `runtime.py:30`, `:72-82`).
- *Aggravation 1* : il a lieu **même quand `FR_ELI_BASE_URL` est défini**, car l'argument par défaut de `os.environ.get` est évalué avant l'appel : `client.py:46-47` — `return os.environ.get("FR_ELI_BASE_URL", runtime.base_url("eli", DEFAULT_BASE_URL)).rstrip("/")`. Seul `FR_ELI_RUNTIME_URL=""` le désactive (`runtime.py:10`, `:52-53`).
- *Aggravation 2* : il a lieu **même sans identifiants** : `LegifranceClient.__init__` évalue `_env_base_url()` (`client.py:126`) avant `_token_manager()` (`client.py:128`) qui lève `CredentialsError`.

Chaîne prouvée : `runtime.py:22-25` (URL `…/releases/latest/download/fr-runtime.json.gz`) → `:55-58` (`urllib.request.urlopen`, gunzip, `json.loads`) → `:85-92` (`base_urls["eli"]` accepté s'il est une chaîne non vide, **sans validation d'hôte ni de schéma**) → `client.py:126` → `client.py:150` (`url = f"{self.base_url}{path}"`) → `client.py:161-163` (`headers={"Authorization": f"Bearer {token}"}`). Aucune vérification d'intégrité : le workflow publie un `.sha256` (`.github/workflows/release.yml:58-59`, `:68`) que le client ne télécharge ni ne vérifie. Le manifeste est en outre persisté sur disque (`runtime.py:61-66`) et resservi hors ligne (`:78-80`).
Télémétrie : `runtime.py:7-8` — « The unlinked asset's download_count is a clean per-connector activation metric » ; `release.yml:46` — « Unlinked URL -> download_count = real activation ».
Contradictions : `CONSTITUTION.md:31-32` (« no … commercial telemetry. The server talks only to the PISTE OAuth + Legifrance endpoints and the local filesystem ») et `CONSTITUTION.md:18-19` (« sends nothing beyond the search query / document id »).
Portée exacte du risque : l'URL OAuth n'est **pas** pilotée par le manifeste (`client.py:42-43`), donc le `client_secret` n'est pas redirigé ; sont exposés le **jeton Bearer** (`expires_in=3600` en sandbox, `DISCOVERY.md:12`), **toutes les requêtes** (confidentialité de ce que cherche le juriste) et surtout **l'intégrité des textes renvoyés** (un hôte malveillant peut servir de faux articles/décisions présentés comme « verbatim from Legifrance », `server.py:50`).

→ **D-04** (bloquant) et **D-05** (majeur, blocage de boucle).

### 2.d « Client HTTP + cache disque recréés à chaque appel, try/except d'audit dupliqué 8 fois, erreurs en chaînes "[code] …", pas de pagination (`pageNumber=1` figé) »

**Verdict : CONFIRMÉ, avec une correction de compte.**
- Recréation : `async with LegifranceClient() as client:` dans 7 outils (`server.py:205, 248, 294, 347, 388, 430, 500`) ; le constructeur crée un `HttpCache()` (`client.py:127` → `cache.py:31-34` : `mkdir` + `diskcache.Cache(...)` = ouverture SQLite) et un `httpx.AsyncClient` (`client.py:129-132`) fermé en fin d'appel (`client.py:140-142`) → nouvelle connexion TCP+TLS à chaque appel d'outil. Seul le `TokenManager` est partagé (dictionnaire module `_TOKEN_MANAGERS`, `client.py:103-114`), donc le jeton, lui, est bien réutilisé.
- Duplication : **7** blocs `try/except Exception` (pas 8 : `fr_coverage` n'en a pas) aux lignes `server.py:207, 250, 296, 349, 390, 432, 502` ; **8** instanciations `audit = _audit()` (une par outil) ; **15** appels `audit.log(` au total.
- Erreurs-chaînes : `server.py:69-78` (`class ToolError(Exception)` ; message `f"[{code}] {message}"`).
- Pagination : `server.py:150` — `"pageNumber": 1` ; `:151` `pageSize` ; `:153` `"sort": "PERTINENCE"` ; `:154` `"typePagination": "DEFAUT"` ; aucun paramètre d'outil pour la page (`server.py:174`).

→ **D-06**, **D-07**, **D-08**, **D-09**.

---

## 3. Défauts détaillés

### 3.1 Cache

#### D-01 — Cache de recherche LODA/CODE inopérant et générateur d'écritures inutiles
- **Gravité** : majeur.
- **Preuve** : voir §2.a (`server.py:129-136`, `client.py:144-154`, `client.py:171`, `cache.py:16`).
- **Conséquence** : 0 % de hits pour les recherches de législation (les plus fréquentes), consommation de quota PISTE et de latence, écriture disque systématique jusqu'à 1 Go.
- **Mesure Rust** : décorateur `CachedLegalSource<S: LegalSource, C: CacheStore>` dans `application`. La clé est construite à partir de l'**objet de domaine** `SearchQuery` (et non du JSON upstream) : `{env, base_url_id, fond, mode, terms normalisés, page, page_size, sort, filters, as_of_date (jour civil Europe/Paris)}` sérialisé canoniquement puis haché (SHA-256). `singleDate` est recalculé *dans l'adapter* à partir de `as_of_date`, jamais dans la clé.
- **Tests de non-régression** : `cache_key_is_stable_across_calls_same_day` (deux `SearchQuery` identiques à 10 ms d'écart → même clé) ; `second_identical_loda_search_hits_cache` (wiremock : `expect(1)` sur `/search`) ; `cache_key_changes_when_paris_day_changes` (horloge simulée 2026-03-29 23:59 → 00:01 Europe/Paris).

#### D-15 — Cache : namespace, négatifs, fail-closed, pickle, confidentialité
- **Gravité** : majeur.
- **Preuves** :
  - Clé sans environnement ni hôte : `client.py:144-147` (seuls `path` et `payload`). Or `base_url` peut changer (env, manifeste D-04, bascule sandbox→prod) → réponses sandbox resservies en prod (24 h pour `act`, `cache.py:15`).
  - Les réponses 200 « vides » (ex. `{"text": null}`) sont cachées comme des succès : `client.py:169-171` cache tout `resp.json()` ; la décision `not_found` n'est prise qu'après (`server.py:256-258`) → un « introuvable » persiste 24 h.
  - Échec d'écriture = échec de l'outil : `self._cache.set(...)` (`client.py:171`) est dans le `try` mais seules `httpx.HTTPStatusError` et `httpx.TransportError/TimeoutException` sont rattrapées (`client.py:173-180`) ; diskcache lève `sqlite3.OperationalError` disque plein (`diskcache/docs/tutorial.rst:885-888`) → la réponse obtenue est perdue, l'outil échoue sans code (`_map_upstream` renvoie l'exception brute, `server.py:122`).
  - Désérialisation pickle : `diskcache/core.py:221` (`pickle.dumps(value…)`), `:282-284` (`pickle.load`) — quiconque écrit dans `~/.matematic/cache/fr-eli` obtient l'exécution de code au prochain `get` (risque local, faible mais réel).
  - Confidentialité incohérente : l'audit ne garde qu'un hash de l'entrée (`audit.py:35-38`) « pour la vie privée », mais la clé de cache stocke **la requête en clair** et la réponse complète sur disque (`client.py:146-147`), les entrées expirées restant présentes jusqu'au « cull ».
  - SQLite déconseillé sur NFS (`diskcache/docs/tutorial.rst:879-883`) — pertinent si le home est réseau.
  - TTL inutilisés hérités : `cache.py:17-18`, `:52-55` (`dict`, `changes`) — code mort.
- **Mesure Rust** : `CacheStore` port (application) ; implémentations `moka` (mémoire, défaut) et disque optionnel (ex. `redb`) avec valeurs **serde_json** (jamais d'exécution à la désérialisation) ; clé = SHA-256 de `{schema_version, env, host, request canonique}` ; **fail-open** (erreur de cache = `tracing::warn!` + appel direct) ; résultats négatifs cachés avec TTL court distinct (ex. 5 min) ou pas du tout ; option `cache.store_queries=false` qui interdit la mise en cache disque des recherches ; TTL par type de ressource paramétrable.
- **Tests** : `cache_namespaced_by_env` (même requête sandbox/prod → 2 appels upstream) ; `cache_write_failure_does_not_fail_tool` (store qui renvoie `Err` → l'outil réussit) ; `negative_result_ttl_is_short` ; `disk_cache_never_contains_raw_query_when_disabled` (grep du fichier de cache).

### 3.2 Audit

#### D-02 — Audit incomplet (violation CONSTITUTION art. 2)
- **Gravité** : **bloquant** (le projet en fait une exigence légale : `audit.py:1` « JSONL audit logger for AI Act art. 12 »).
- **Preuves** : §2.b. Ajouter : si `audit.log` échoue dans le chemin d'erreur, l'`OSError` remplace l'erreur d'origine (l'appel est dans le `except`, `server.py:208-211`) ; dans le chemin heureux, l'échec d'audit après le travail produit une erreur sans code (« Error calling tool … [Errno 28] … »), conforme à l'esprit de l'art. 2 mais non structurée.
- **Conséquence** : les appels les plus révélateurs (arguments hallucinés, IDs inventés par le LLM, introuvables) disparaissent du journal ; la promesse faite au LLM (`server.py:51`) est fausse.
- **Mesure Rust** : un seul point de passage — décorateur `Audited<U: UseCase>` dans `application` qui enveloppe **l'exécution complète** (validation des arguments incluse : la validation se fait *dans* le cas d'usage via les constructeurs de newtypes, pas avant le décorateur) et écrit un `AuditRecord` pour **chaque** issue `Ok | Err(code)`. Politique art. 2 : si l'écriture échoue, renvoyer `AppError::AuditUnavailable` (code `audit_error`) — jamais de succès silencieux ; l'erreur d'origine reste dans la cause chaînée.
- **Tests** : table de cas `audit_written_for_every_outcome` paramétrée par `{ok, invalid_arg, not_found(404), not_found(empty), upstream_error, config_error, timeout, invalid_response}` → exactement 1 ligne par appel ; `audit_failure_returns_audit_error` (writer qui échoue) ; `audit_failure_does_not_mask_original_cause` (cause chaînée présente).

#### D-03 — `duration_ms` faux
- **Gravité** : majeur.
- **Preuve** : §2.b ; en plus, dans le chemin heureux le `with timer()` n'englobe que l'appel HTTP (`server.py:203-206`), pas la normalisation (`:213-219`) ni la validation pydantic.
- **Mesure Rust** : `let start = Instant::now()` au début du décorateur ; durée calculée à la sortie quel que soit le résultat (y compris `?` précoce et annulation via un garde `Drop`).
- **Tests** : `error_duration_is_nonzero` (mock upstream avec `tokio::time::pause` + délai 200 ms → `duration_ms >= 200` pour une erreur 503) ; `duration_includes_normalization`.

#### D-16 — Journal d'audit : rotation, durabilité, concurrence
- **Gravité** : mineur.
- **Preuves** : `audit.py:73-75` — `with self._path.open("a", encoding="utf-8") as fh: fh.write(line)` : ouverture/fermeture à chaque ligne, pas de `fsync`, aucune rotation ni rétention ; `audit.py:44-47` `mkdir` à chaque instanciation (une par appel, `server.py:106-107`). Plusieurs clients MCP lancent chacun un processus qui écrit le même fichier : sur POSIX `O_APPEND` positionne atomiquement l'offset par `write()` mais l'absence d'entrelacement n'est garantie par POSIX que pour les pipes ; sous Windows le comportement multi-processus du mode `"a"` **NON VÉRIFIÉ**. Une `OSError` d'écriture expose le chemin local complet dans le message renvoyé au LLM (cf. D-40).
- **Mesure Rust** (`adapter-audit`) : tâche dédiée recevant les `AuditRecord` par canal borné (`tokio::sync::mpsc`), écriture d'une ligne complète par `write_all`, `flush` + `sync_data` configurable (`every_record` / `interval`), rotation par taille et/ou par jour (ex. `tracing-appender`-like ou implémentation maison), verrou consultatif inter-processus (ex. `fs4`) ou un fichier par processus `fr-eli-<pid>.jsonl` ; accusé de réception (oneshot) pour respecter l'art. 2 (le cas d'usage attend la confirmation d'écriture).
- **Tests** : `concurrent_writers_produce_valid_jsonl` (N tâches × M lignes → chaque ligne parse en JSON) ; `rotation_creates_new_file_after_limit` ; `audit_write_is_acknowledged_before_tool_returns`.

#### D-17 — Contenu de l'enregistrement d'audit
- **Gravité** : mineur.
- **Preuves** : `audit.py:35-38` SHA-256 **non salé** d'un JSON canonique : pour `{"article_id": "LEGIARTI000006419288"}` ou une requête courte, l'entrée est retrouvable par énumération/dictionnaire (identifiants publics séquentiels) → ni confidentialité réelle, ni traçabilité exploitable ; `audit.py:63-70` : pas de code d'erreur (`status` ∈ `ok|error`), pas d'identifiant de requête/session, pas de version du serveur, pas de fond ; `audit.py:72` tronque `error` à 500 caractères.
- **Mesure Rust** : schéma versionné `AuditRecord { v, ts (RFC 3339, UTC), request_id (UUID/ULID), tool, version, env, input_digest (HMAC-SHA256 avec clé locale générée au 1er lancement, ou mode `clear` explicite), outcome: ok|error, error_code, upstream_status, duration_ms, output_size, cache: hit|miss|bypass }`.
- **Tests** : `audit_record_schema_snapshot` (insta) ; `input_digest_differs_between_installations` (deux clés → deux digests).

### 3.3 Configuration distante, environnement, sécurité réseau

#### D-04 — Manifeste distant : redirection du jeton, intégrité des textes, télémétrie
- **Gravité** : **bloquant**.
- **Preuve** : §2.c.
- **Conséquences concrètes** : (1) quiconque obtient le droit de publier une release sur `matematicsolutions/fr-eli-mcp` (compte ou CI compromis) redirige **toutes les installations** — y compris d'anciennes versions, puisque l'URL vise `releases/latest` (`runtime.py:24`) — vers un hôte qui reçoit les Bearer et peut **servir de faux textes de loi** ; (2) pas de signature, pas de vérification du `.sha256` ; (3) manifeste gzip décompressé sans borne (`runtime.py:57-58` : `resp.read()` + `gzip.decompress`) → bombe gzip possible ; (4) télémétrie d'activation contraire à l'art. 3 ; (5) le champ `citation_grammar_version` du manifeste (`runtime/fr-runtime.json`) n'est lu nulle part (seul `base_urls` est consommé, `runtime.py:87`) — code mort.
- **Mesure Rust** (`app`, composition root) : **aucune configuration distante**. Configuration = variables d'environnement + fichier local optionnel, validées au démarrage : schéma `https` obligatoire, hôte dans une allowlist codée (`api.piste.gouv.fr`, `sandbox-api.piste.gouv.fr`, `oauth.piste.gouv.fr`, `sandbox-oauth.piste.gouv.fr`) sauf option explicite `--allow-custom-host` journalisée en `warn` ; aucun appel réseau hors PISTE (test de conformité art. 3). Pas de compteur d'activation.
- **Tests** : `config_rejects_non_allowlisted_host` ; `config_rejects_http_scheme` ; `no_network_call_except_piste` (wiremock + client `reqwest` configuré avec un résolveur qui refuse tout autre hôte, ou test d'intégration qui compte les requêtes) ; test « grep » CI interdisant `github.com` dans le code de prod hors métadonnées.

#### D-05 — E/S réseau bloquante dans un contexte async
- **Gravité** : majeur.
- **Preuve** : `runtime.py:55-57` (`urllib.request.urlopen(req, timeout=_TIMEOUT)`, `_TIMEOUT = 15`, `runtime.py:27`) appelé depuis `LegifranceClient.__init__` (`client.py:126`) exécuté dans les coroutines d'outil (`server.py:205`…) → la boucle asyncio est gelée jusqu'à 15 s (le timeout `urlopen` couvre connexion et lectures ; la résolution DNS n'est pas couverte — comportement standard de `socket`, NON VÉRIFIÉ ici) pendant le premier appel ; toutes les autres requêtes MCP (y compris `ping`/annulations) attendent.
- **Mesure Rust** : supprimé par D-04 ; règle générale : aucune E/S bloquante dans une tâche tokio (lint `clippy::disallowed_methods` sur `std::fs::*`/`std::net::*` dans les crates async, ou revue).
- **Test** : `server_answers_ping_while_tool_waits_upstream` (upstream mock lent 2 s, `ping` répond en < 100 ms).

#### D-18 — Valeurs par défaut pointant vers la SANDBOX PISTE, non signalé
- **Gravité** : majeur.
- **Preuves** : `client.py:28-29` (`DEFAULT_OAUTH_URL = "https://sandbox-oauth…"`, `DEFAULT_BASE_URL = "https://sandbox-api…"`) ; `runtime/fr-runtime.json` (`base_urls.eli` = sandbox) ; `CONSTITUTION.md:56-57` (point ouvert). La DILA « does not target any service commitment on the sandbox environment » (CGU API Légifrance 2022 §IV.1, cité par `pylegifrance/docs/src/content/docs/en/concepts/api-terms-and-quotas.md:19-27` ; document primaire non consulté → citation de seconde main). `fr_coverage` déclare `live=True` et `status="ok"` sans mentionner l'environnement (`coverage.py:73-75`).
- **Conséquence** : un utilisateur en production peut, sans le savoir, citer des données servies par un environnement sans engagement de service ; l'écart de contenu sandbox/prod est **NON VÉRIFIÉ** (les fixtures de 2026-07 montrent des données récentes : décision CE du 29/05/2026, `consult_juri_cetat.json` ; KALI `dateVersion` 2026-07-08, `search_kali.json`).
- **Mesure Rust** : `LEGIFRANCE_ENV=production|sandbox` **obligatoire** (pas de défaut) dans `app` ; l'environnement est un champ de provenance de *chaque* réponse (`provenance.env`) et de `fr_coverage` (famille marquée `degraded` en sandbox).
- **Tests** : `startup_fails_without_env` ; `every_response_carries_provenance_env` (snapshot) ; `coverage_reports_sandbox_as_degraded`.

#### D-39 — Fuites de ressources et ordre de construction
- **Gravité** : mineur.
- **Preuve** : `client.py:126-128` : `base_url` (déclenche D-04), puis `HttpCache()` (ouvre SQLite), puis `_token_manager()` qui peut lever `CredentialsError` (`client.py:50-58`) → l'objet n'atteint jamais `__aenter__`/`aclose()` (`client.py:134-142`) : la connexion diskcache reste ouverte jusqu'au GC ; à chaque appel sans identifiants.
- **Mesure Rust** : configuration et dépendances construites **une fois** au démarrage dans `app` ; identifiants absents → le serveur démarre quand même mais chaque outil renvoie immédiatement `config_error` sans E/S (ou refuse de démarrer, au choix documenté) ; RAII (`Drop`).
- **Test** : `missing_credentials_makes_no_network_or_disk_io`.

### 3.4 Client HTTP, OAuth, retry, débit, délais

#### D-06 — Client HTTP et cache recréés à chaque appel
- **Gravité** : majeur (performance, quota de connexions).
- **Preuve** : §2.d.
- **Conséquence** : poignée de main TLS par appel (le coût exact n'a pas été mesuré — NON VÉRIFIÉ), pas de réutilisation keep-alive, ouverture/fermeture SQLite + `mkdir` par appel ; les usages « table des matières → N articles » multiplient ce coût.
- **Mesure Rust** : un `reqwest::Client` (pool de connexions) unique construit dans `app` et injecté dans `LegifranceAdapter` ; `CacheStore` unique ; `Arc` partagés.
- **Test** : `adapter_reuses_connections` (wiremock : compter les connexions TCP n'est pas direct ; à défaut test de construction : le client est un champ `Arc` de l'adapter, et bench `criterion` optionnel).

#### D-10 — Classification erronée des erreurs OAuth et des 404
- **Gravité** : majeur.
- **Preuves** :
  - `client.py:87` : `resp.raise_for_status()` sur le point OAuth → `httpx.HTTPStatusError` ; dans `_post`, un 400/401 OAuth (ex. `invalid_client`) n'est pas dans `_RETRY_STATUS` (`client.py:33`) → relancé (`client.py:173-176`) → `_map_upstream` le transforme en `upstream_error` « Legifrance/PISTE HTTP 401 » (`server.py:113-119`) alors qu'il s'agit d'un problème de configuration ; les INSTRUCTIONS disent au LLM « Retry once before surfacing » (`server.py:58`) → nouvelles tentatives vaines.
  - `server.py:114-115` : **tout** 404 devient `not_found` « Legifrance returned 404 for that resource », y compris un 404 du point OAuth (mauvaise `FR_ELI_OAUTH_URL`) ou d'un chemin d'API erroné (mauvaise `base_url`, cf. D-04/D-18). Le LLM conclut alors qu'un texte n'existe pas — exactement le travers que les INSTRUCTIONS veulent éviter (« Absence here is not absence in the law », `server.py:46`).
  - `client.py:93` : `float(data.get("expires_in", 3600))` → `TypeError` si `expires_in` vaut `null`, non rattrapé.
- **Mesure Rust** (`adapter-legifrance`) : enum `LegifranceError { Auth(AuthError::{InvalidClient, Forbidden, EndpointNotFound, …}), NotFound { resource }, RateLimited { retry_after }, Upstream { status }, Timeout, InvalidResponse { reason }, Transport }` ; le 404 n'est `NotFound` que sur un endpoint de consultation connu **et** si le corps ne trahit pas une erreur de routage (à définir avec des captures réelles — NON VÉRIFIÉ : forme d'un 404 PISTE) ; erreurs OAuth → `config_error` côté MCP ; `expires_in` optionnel avec défaut et borne.
- **Tests** (wiremock) : `oauth_401_maps_to_config_error` ; `oauth_404_maps_to_config_error_not_not_found` ; `api_route_404_is_not_not_found` ; `oauth_expires_in_null_uses_default`.

#### D-11 — Réponses non-JSON / `null` non mappées
- **Gravité** : majeur.
- **Preuves** : `client.py:88` et `:170` — `resp.json()` ; `httpx/_models.py:831-832` : `return jsonlib.loads(self.content, **kwargs)` → `json.JSONDecodeError` (sous-classe de `ValueError`) si le corps est HTML/vide ; non rattrapée par `client.py:173-180`, `_map_upstream` la renvoie brute (`server.py:122`) → FastMCP produit « Error calling tool 'x': Expecting value… » sans code. Un corps JSON `null` passe (`json.loads` → `None`), est mis en cache (`client.py:171` exige seulement que la valeur lue soit un dict pour la *resservir*, `:153`), puis `normalize_*` lève `AttributeError` (démontré : `normalize_search(None, …)`, `normalize_article(None)`, `normalize_juri(None)` → `AttributeError: 'NoneType' object has no attribute 'get'`) — hors `try`, donc non audité (D-02).
- **Mesure Rust** : lecture `bytes()` bornée (D-41) → `serde_json::from_slice::<Dto>` ; échec → `InvalidResponse { content_type, snippet_redacted }` ; jamais de panique (`#![deny(clippy::unwrap_used, clippy::expect_used)]` dans adapters).
- **Tests** : `html_body_maps_to_invalid_response` ; `json_null_body_maps_to_invalid_response_or_not_found` ; `invalid_response_is_audited`.

#### D-12 — Politique de retry incorrecte
- **Gravité** : majeur.
- **Preuves** (`client.py:156-183`) :
  - 401 : tentative 0 reçoit 401 → `invalidate()` + `forced_refresh = True` + `continue` (`:164-168`) ; il ne reste que 2 tentatives pour les 5xx/429 (`_MAX_ATTEMPTS = 3`, `:34`).
  - `forced_refresh` n'est jamais remis à `False` → **chaque tentative suivante redemande un jeton** (`:160` : `get_token(self._http, force=forced_refresh)`) : 401 puis 503 puis 503 = 3 jetons demandés.
  - Backoff fixe `0.5 * 2**attempt` (`:181`) : pas de jitter, en-tête `Retry-After` ignoré sur 429.
  - 500 retenté (`:33`) même quand l'erreur est déterministe (ex. paramètre `date` invalide — comportement upstream NON VÉRIFIÉ, cf. D-22).
  - Signal externe : selon `pylegifrance/tests/integration/conftest.py:1-20`, PISTE renverrait parfois `HTTP 401 {"error":"invalid_client"}` sous throttling (« very short throttle windows where PISTE refuses a token that was valid a moment ago ») — **rapport tiers, NON VÉRIFIÉ** ; si c'est exact, la stratégie « 401 ⇒ nouveau jeton » aggrave la charge sur l'OAuth.
- **Mesure Rust** : politique explicite et testée dans `adapter-legifrance` : budget de retry « transitoire » (429/502/503/504/timeout ; 500 configurable) séparé d'**un seul** rafraîchissement de jeton sur 401 par appel ; backoff exponentiel + jitter plafonné ; respect de `Retry-After` (secondes ou date HTTP) borné par le délai global (D-14) ; `TokenManager` à « single-flight » (un seul rafraîchissement concurrent, `tokio::sync::Mutex`/`OnceCell` réinitialisable) ; crate candidate `backon` (1.6.0 observée dans l'index sparse le 2026-10-15) ou boucle maison.
- **Tests** (wiremock + `tokio::time::pause`) : `401_then_503_then_200_requests_exactly_two_tokens` ; `429_retry_after_is_honoured` ; `401_twice_maps_to_config_error_after_single_refresh` ; `concurrent_calls_share_one_token_refresh`.

#### D-13 — Aucune limitation de débit ni de concurrence
- **Gravité** : majeur (bloquant pour la future feature `fr_search_all` multi-fonds parallèle).
- **Preuves** : aucun limiteur dans `client.py` ; PISTE applique des quotas « per-second / per-minute / per-day », visibles dans la console, modifiables à tout moment, dépassement = 429 (`pylegifrance/docs/src/content/docs/en/concepts/api-terms-and-quotas.md:35-48`, de seconde main ; valeurs exactes **NON VÉRIFIÉES**). Le tiers pylegifrance espace ses appels de tests de 200 ms (`tests/integration/conftest.py:27`).
- **Mesure Rust** : décorateur/couche `RateLimitedSource` (application ou adapter) : limiteur GCRA (crate `governor`, 0.10.4 observée) configurable `LEGIFRANCE_RPS`, + `tokio::sync::Semaphore` pour la concurrence max ; `fr_search_all` passe par ce limiteur.
- **Tests** : `rate_limiter_spaces_requests` (`tokio::time::pause`, 10 appels à 2 rps → ≥ 4,5 s simulées) ; `search_all_respects_concurrency_cap`.

#### D-14 — Pas de délai global par outil
- **Gravité** : majeur.
- **Preuves** : `client.py:30` `httpx.Timeout(40.0, connect=10.0)` = délais *par opération* (connexion 10 s ; lecture/écriture/pool 40 s chacun) ; 3 tentatives (`:34`) + sommeils 0,5 s et 1 s (`:181`) + jeton(s) OAuth sous les mêmes délais + manifeste 15 s (D-05). Pire cas grossier : 15 + (10+40) + 3×(10+40) + 1,5 ≈ **216 s**, davantage avec les jetons forcés (D-12) ; un délai de lecture httpx porte sur l'intervalle entre octets, une réponse « goutte-à-goutte » peut donc dépasser 40 s au total (comportement standard httpx, non re-vérifié ici). Les délais côté clients MCP sont **NON VÉRIFIÉS**.
- **Mesure Rust** : `tokio::time::timeout(deadline_outil, use_case)` dans le décorateur d'application (ex. 25 s recherche, 30 s consultation, configurables) ; le budget restant est transmis à la politique de retry ; `reqwest::ClientBuilder` avec `connect_timeout` et `timeout` total par requête (API exacte à confirmer sur docs.rs, bloqué ici).
- **Test** : `tool_returns_timeout_code_before_deadline` (upstream qui ne répond jamais → `code=timeout` en ≤ deadline + ε, temps simulé).

#### D-41 — Taille des corps non bornée
- **Gravité** : mineur.
- **Preuves** : `client.py:170` lit tout le corps en mémoire puis le met en cache ; `runtime.py:57-58` décompresse sans limite. Taille maximale réelle d'une réponse `/consult/lawDecree` ou d'un code entier : **NON VÉRIFIÉE** (fixtures élaguées, D-35).
- **Mesure Rust** : lecture en flux avec plafond (ex. 16 Mo configurable) → `InvalidResponse::TooLarge`.
- **Test** : `oversized_body_is_rejected`.

### 3.5 Contrat d'erreur

#### D-07 — Duplication du boilerplate
- **Gravité** : majeur (c'est la cause racine de D-02/D-03).
- **Preuve** : §2.d ; chaque outil répète `audit = _audit()`, `hash_input`, `with timer()`, `try/except`, `audit.log` ×2, `raise _map_upstream(exc) from exc`.
- **Mesure Rust** : les outils MCP (`adapter-mcp`) ne font que : parser les arguments → appeler `UseCase::execute` → convertir le résultat. Audit, cache, délai, limitation = décorateurs composés une fois dans `app`.
- **Test** : test d'architecture (ex. `cargo-deny`/script) qui interdit l'import de `adapter-audit` depuis `adapter-mcp` ; test de chaque outil via un `UseCase` factice.

#### D-08 — Erreurs en chaînes et interaction FastMCP
- **Gravité** : majeur.
- **Preuves** :
  - `server.py:69-78` : `ToolError` hérite d'`Exception`, pas de `fastmcp.exceptions.ToolError`.
  - FastMCP (`fastmcp_slim/fastmcp/server/server.py:1494-1532` @ e2fcc41) : toute exception non-FastMCP est journalisée en **ERROR avec traceback** (`exc_info=status_code is None`, `:1515-1517`) puis réémise comme « `Error calling tool {name!r}: {e}` » (`:1532`) ou, si `mask_error_details` est activé, « `Error calling tool {name!r}` » **sans le code** (`:1530-1531` ; réglage `fastmcp_slim/fastmcp/settings.py:207-219`, défaut `False`). Comportement de la version 3.4 effectivement installée : NON VÉRIFIÉ (vérifié sur `main`).
  - Le LLM ne reçoit qu'une chaîne à analyser ; pas de champ `code`, `retryable`, `hint`, `retry_after`.
  - Spécification MCP : les erreurs d'exécution d'outil se signalent dans le résultat avec `isError: true` et doivent être « actionable feedback that language models can use to self-correct » (`docs/specification/2025-11-25/server/tools.mdx:469-476`).
- **Mesure Rust** : `AppError` (thiserror) → `ErrorCode` stable (`invalid_arg`, `not_found`, `upstream_error`, `rate_limited`, `timeout`, `config_error`, `audit_error`, `invalid_response`) ; `adapter-mcp` renvoie `CallToolResult { is_error: true, content: [texte court "[code] message"], structured_content: { code, message, retryable, retry_after_s?, hint? } }` ; aucune trace de pile vers le client ; `tracing::info!` (pas `error!`) pour les erreurs attendues (`invalid_arg`, `not_found`).
- **Tests** : `error_result_has_structured_code` (snapshot insta) pour chaque `ErrorCode` ; `invalid_arg_is_not_logged_as_error`.

### 3.6 Normalisation, dates, robustesse

#### D-19 — « Aujourd'hui » en UTC
- **Gravité** : mineur (mais sensible juridiquement).
- **Preuve** : `server.py:125-126` — `time.strftime("%Y-%m-%d", time.gmtime())`, utilisé comme date de version par défaut de `fr_get_act` (`server.py:243`).
- **Conséquence** : entre 00:00 et 01:00 (hiver) / 02:00 (été) heure de Paris, la date UTC est celle de la veille → la version « en vigueur aujourd'hui » renvoyée est celle d'hier ; un texte entrant en vigueur le jour J à 00:00 n'apparaît pas pendant 1 à 2 h. (`singleDate` de la recherche est un instant, `server.py:136` — non affecté.)
- **Mesure Rust** : port `Clock` dans `application` (`fn today_paris(&self) -> NaiveDate`), implémentation `chrono-tz` (0.10.4 observée) ou `jiff` (0.2.38 observée) avec `Europe/Paris` ; injectable en test.
- **Tests** : `today_is_paris_date_at_2330_utc_in_summer` (2026-07-14T22:30Z → 2026-07-15) ; `dst_transition_days` (2026-03-29, 2026-10-25).

#### D-20 — `ms_to_date` : Windows pré-1970, sentinelle 2999, types
- **Gravité** : majeur.
- **Preuves** :
  - `citations.py:66-73` : `datetime.fromtimestamp(ms / 1000, tz=UTC)` ; exceptions `OverflowError, OSError, ValueError` → `None`.
  - Windows : avec `tz` fourni, CPython utilise `_PyTime_gmtime` (`Modules/_datetimemodule.c:5446-5448`, branche 3.13) qui, sous `MS_WINDOWS`, appelle `gmtime_s` et lève `OSError` sur erreur (`Python/pytime.c:1325-1336`) ; la documentation Microsoft indique qu'une valeur `< 0` renvoie `EINVAL` (page `_gmtime_s` VS2008 : https://msdn.microsoft.com/de-de/library/3stkd9be(v=vs.90), et borne basse 1970 dans la doc actuelle : https://learn.microsoft.com/zh-cn/cpp/c-runtime-library/reference/gmtime-s-gmtime32-s-gmtime64-s?view=msvc-140). Conclusion : **sous Windows (plate-forme visée par le README, section « Windows 11 with Smart App Control »), toute date antérieure au 1970-01-01 devient `None`** — ex. `textTitles[].dateDebut = -5231606400000` (1804-03-21, Code civil) dans `consult_article.json`. Non testé sur une machine Windows : conclusion **très probable**, à confirmer. Sous Linux, `ms_to_date(-5231606400000)` renvoie bien `"1804-03-21"` (mesuré).
  - Sentinelle : `32472144000000` ms = 2999-01-01 apparaît **11 fois** dans les fixtures (ex. `consult_article.json` `article.dateFin`, `consult_kali.json` `dateParution`, `consult_acco.json` `acco.dateFin`) et sous forme chaîne `"2999-01-01"` (ex. `consult_loda.json` `dateFinVersion`, `consult_kali.json` `sections[].dateFin`) ou ISO `"2999-01-01T00:00:00.000+0000"` (`search_code.json` `datePublication`). La documentation OpenAPI officielle (via le modèle généré de pylegifrance, `pylegifrance/models/generated/model.py:3504` : « Contient la date du texte si elle existe (!= null && != 2999-01-01) ») confirme que c'est une valeur « absence de date ». La référence l'expose comme une vraie date : mesuré `normalize_article(...)["date_fin"] == "2999-01-01"`, `normalize_kali(...)["date_parution"] == "2999-01-01"`, `normalize_law_decree(...)["date_fin_version"] == "2999-01-01"`. Le LLM peut écrire « en vigueur jusqu'au 1er janvier 2999 » ou « convention publiée le 1er janvier 2999 ».
  - Types : `isinstance(True, int)` est vrai → `ms_to_date(True) == "1970-01-01"` (mesuré) ; un flottant (`1748995200000.0`) → `None` (mesuré).
  - Fuseau : les 27 timestamps « date » non sentinelles des fixtures sont des multiples exacts de 86 400 000 (minuit UTC), donc la conversion UTC est correcte *sur ces données* ; un timestamp « minuit Paris » (ex. 22:00Z) serait décalé d'un jour (`ms_to_date(1748991600000) == "2025-06-03"`, mesuré). La convention de l'API pour tous les champs est **NON VÉRIFIÉE**.
- **Mesure Rust** (`domain`) : type `LegalDate` = `enum { Date(NaiveDate), Open /* sentinelle 2999-01-01 = sans fin */, Unknown }` ; désérialiseur unique acceptant `i64` ms (conversion `chrono::DateTime::from_timestamp_millis`, qui utilise `div_euclid`/`rem_euclid` en Rust pur — `chrono/src/datetime/mod.rs:838-842` @ 6adaa52 — donc indépendant du `gmtime` de la plate-forme), `"YYYY-MM-DD"`, ISO `…+0000` ; rejet explicite des booléens ; avertissement `tracing::debug!` si un timestamp n'est pas aligné sur minuit UTC. Sortie MCP : `"date_fin": null, "en_vigueur_sans_date_de_fin": true` (ou équivalent), jamais « 2999 ».
- **Tests** : `pre_1970_dates_roundtrip` (proptest sur [-10^13, 10^13]) exécuté en **CI Windows** ; `sentinel_2999_is_open_not_a_date` (les 11 occurrences des fixtures) ; `bool_is_rejected` ; `iso_plus0000_string_parses`.

#### D-21 — Formats hétérogènes, champs ignorés, incohérences LODA/KALI
- **Gravité** : mineur.
- **Preuves** : trois formats dans les fixtures (mesure par script) : entier ms (27 + 11 sentinelles + 1 négatif), `"YYYY-MM-DD"` (ex. `consult_loda.json` `dateDebutVersion`, `jurisDate`), ISO `"2016-10-09T00:00:00.000+0000"` (champs de recherche `date`, `datePublication`, `dateSignature`…). `normalize_law_decree` convertit `dateParution` mais recopie `dateDebutVersion`/`dateFinVersion` bruts (`citations.py:220-223`). Les dates des résultats de recherche ne sont pas exposées (`citations.py:118-125`). `etat` LODA lu dans `payload["etat"]` (`citations.py:219`) qui vaut `null` dans `consult_loda.json` alors que `jurisState = "Vigueur"` est présent ; `normalize_kali` fait le repli `jurisState or etat` (`citations.py:427`) — incohérence entre deux fonctions quasi identiques (`citations.py:204-228` vs `:403-436`).
- **Mesure Rust** : désérialiseur de date unique (D-20) ; normaliseur commun `TextConsultDto → TextVersion` pour LODA et KALI ; état juridique typé `enum LegalStatus { Vigueur, VigueurEtendue, Abroge, Modifie, … , Other(String) }` alimenté par `etat` **ou** `jurisState`.
- **Tests** : snapshots insta des sorties normalisées de chaque fixture ; `loda_status_falls_back_to_juris_state`.

#### D-22 — Paramètre `date` non validé
- **Gravité** : mineur.
- **Preuve** : `server.py:243` — `version_date = (date or _today()).strip()` puis envoi direct (`client.py:190-193`). « 07/10/2016 », « hier », « 2016-13-45 » partent à l'API ; la réponse upstream (400 ? 500 ?) est **NON VÉRIFIÉE** ; si 500, elle est retentée 3 fois (D-12).
- **Mesure Rust** : `VersionDate::parse(&str) -> Result<NaiveDate, DomainError>` (format strict `YYYY-MM-DD`, plage 1789-01-01..=aujourd'hui+N ans) dans `domain` ; `invalid_arg` local, sans appel réseau.
- **Tests** : `invalid_date_is_rejected_locally` (wiremock `expect(0)`).

#### D-23 — `null` dans les listes / payload `null`
- **Gravité** : majeur.
- **Preuves (démontrées sur `citations.py`)** : `normalize_search({'results':[{'titles':[None]}]}, 'JURI')` → `AttributeError` (`citations.py:112-114` : `titles = result.get("titles") or [{}]` ne protège que la liste vide) ; `results=[None]` → `AttributeError` (`:178`) ; `sections=[None]` pour CODE → `AttributeError` (`:154-155`) ; `normalize_law_decree({'id':…,'sections':[None]})` et `articles=[None]` → `AttributeError` (`:191-193`). Ces exceptions surviennent hors `try` (`server.py:213`, `:256`…), donc sans code ni audit.
  Cas bien gérés (mesurés) : `values=[None]`/`[]` dans un extrait CODE, `themes=[None, {'libelle': None}]` ACCO, `textTitles` dict au lieu de liste.
- **Mesure Rust** : DTO serde avec `#[serde(default)]` et `Vec<Option<T>>` filtré, ou désérialiseur « tolérant » qui ignore les éléments invalides en les comptant (`dropped_items` en `tracing::debug!`) ; tests de fuzz.
- **Tests** : `proptest`/`arbitrary` : « tout JSON dérivé d'une fixture par mutation (`null` injectés, champs supprimés, types changés) ⇒ `Ok(_)` ou `Err(InvalidResponse)`, jamais de panique ».

#### D-24 — Typage rigide pydantic
- **Gravité** : mineur.
- **Preuve** : modèles `str | None` (`models.py:80-98`, `106-122`…), `ConfigDict(extra="allow", populate_by_name=True)` (`models.py:20-21`) sans `coerce_numbers_to_str`. Mesuré avec pydantic 2.13.5 local sur un modèle de même forme : `num_parution: str | None` reçoit `235` → `ValidationError` « Input should be a valid string » ; `numero_affaire: list[str]` reçoit `"24-13.470"` → `ValidationError`. La validation (`Act.model_validate`, `server.py:259` etc.) est hors `try` → erreur sans code, non auditée. **Dans les fixtures, aucun champ de ce type n'est hétérogène** (vérifié par script : `num`, `idcc`, `numParution`, `codeIdcc` sont toujours des chaînes, ex. `search_kali.json` `num = "18"`, `search_acco.json` `idcc = "275"`) ; seul indice d'une variabilité réelle : l'auteur a typé `idcc: str | int | None` (`models.py:46`, `:158`). Le risque est donc démontré sur entrée synthétique, son occurrence réelle est **NON VÉRIFIÉE**.
- **Mesure Rust** : désérialiseurs tolérants `string_or_int`, `one_or_many` (via `serde_with` ou maison) dans `adapter-legifrance`, pour les champs identifiants/numéros ; le domaine ne voit que des types propres ; échec de désérialisation → `InvalidResponse` (jamais de panique).
- **Tests** : `num_accepts_int_and_string` ; `numero_affaire_accepts_scalar_and_list`.

#### D-25 — Table des matières aplatie, non ordonnée, non dédupliquée, contenu jeté
- **Gravité** : majeur.
- **Preuves** :
  - `citations.py:190-201` (`_walk_articles`) n'émet que `{article_id, num, etat}` : les **26 titres de sections** (Titre/Chapitre/Section) de `consult_loda.json` disparaissent (mesuré : 26 sections en entrée, 0 titre en sortie) → le LLM ne peut pas naviguer « Titre II, chapitre Ier ».
  - Ordre : articles de la section **puis** sous-sections, sans tenir compte d'`intOrdre` ; démontré sur entrée synthétique (section avec articles 1, 3 et sous-section contenant 2 → sortie `["1","3","2"]`). Les fixtures KALI portent `intOrdre` sur sections et articles (`consult_kali.json` : `1074003967`, `1073741823`) ; dans `consult_loda.json` élagué, `intOrdre` est absent → impact réel sur LODA **NON VÉRIFIÉ**.
  - Doublons : `normalize_kali` parcourt `sections` puis `articles` de premier niveau (`citations.py:413-419`) sans dédup ; démontré synthétiquement (même `KALIARTI1` aux deux endroits → 2 entrées). Fréquence réelle **NON VÉRIFIÉE** (`articles` de premier niveau vide dans la fixture).
  - Contenu jeté : la réponse `/consult/kaliText` contient déjà `sections[].articles[].content` (HTML, `consult_kali.json`) ; DISCOVERY dit que `/consult/lawDecree` renvoie le « full LODA text » (`DISCOVERY.md:27`) ; la référence ne garde que les identifiants et oblige à un appel `fr_get_text` par article (N+1, aggravé par D-06). Présence du contenu dans `lawDecree` réel : **NON VÉRIFIÉE** (fixture élaguée à 3 clés par article).
- **Mesure Rust** (`domain`) : `Toc { nodes: Vec<TocNode> }`, `TocNode::Section { id, title, children }` / `TocNode::Article { id, num, status, has_inline_text }`, tri stable par `intOrdre` si présent sinon ordre du tableau, dédup par `article_id` (premier rencontré conservé) ; retour progressif : `fr_get_act(..., depth, section_id, include_text=false|summary|full, max_chars)`.
- **Tests** : `toc_preserves_section_titles` (26 titres sur la fixture) ; `toc_sorted_by_int_ordre` ; `toc_dedups_articles` ; `kali_inline_content_is_available_without_extra_call` (wiremock `expect(0)` sur `getArticle`).

#### D-26 — Structure des textes perdue (`texte` vs `texteHtml`)
- **Gravité** : majeur.
- **Preuves** (mesure sur fixtures) : `texte` contient **0 saut de ligne** dans `consult_article.json`, `consult_juri.json`, `consult_juri_cetat.json` (14 601 caractères), `consult_juri_constit.json` (14 322 caractères), `consult_cnil.json` ; `texteHtml` porte la structure : 85 `<br/>` dans `consult_juri_cetat.json`, 4 dans `consult_juri_constit.json`, 3 `<p>` dans `consult_cnil.json`. Exemple : l'article 9 du Code civil (deux alinéas) est rendu en une seule ligne (`consult_article.json` `article.texte`). La référence ne lit que `texte` (`citations.py:272`, `:351`, `:391`). Que l'API elle-même aplatisse `texte` (plutôt que la capture) est **probable** — `texteHtml` dans la même fixture garde ses `\n` d'indentation, donc la capture n'a pas normalisé les blancs — mais **NON VÉRIFIÉ** en direct.
- **Conséquence** : impossible de citer « al. 2 » ou « point 5 » de façon fiable ; impossible de paginer/cibler par paragraphe (retour progressif, `focus` BM25).
- **Mesure Rust** : `domain::Paragraph { index, text }` ; conversion `texteHtml` → paragraphes (crates candidates `scraper` 0.27.0 / `html2text` 0.17.3 observées ; choix et API NON VÉRIFIÉS) avec repli sur `texte` ; normalisation Unicode légère pour la *recherche* seulement (NBSP U+00A0 présent dans `consult_juri_constit.json`, tiret demi-cadratin U+2013 dans le numéro « SAN–2026-003 » de `search_cnil.json`), jamais pour le texte restitué.
- **Tests** : `cetat_fixture_yields_multiple_paragraphs` ; `article_9_has_two_paragraphs` (avec fixture recapturée propre) ; `verbatim_text_is_unchanged_except_markup` (concaténation des paragraphes == texte HTML débalisé).

### 3.7 Citations, URLs, identifiants

#### D-27 — Citations « convention française » fautives
- **Gravité** : majeur (outil juridique : la citation est le livrable).
- **Preuves** :
  - `citations.py:309` — `head = f"Cons. const., decision n° {num}"` (sans accent) ; `:378` — `f"CNIL, deliberation n° {num} du {date_prose}"` ; test qui fige la faute : `tests/test_parse.py:116` et `:198`.
  - `citations.py:283-286` — mois `"fevrier"`, `"aout"`, `"decembre"` ; mesuré : `_fr_date` du 1er février/août/décembre 2025 → « 1 fevrier 2025 », « 1 aout 2025 », « 1 decembre 2025 » (accents manquants et « 1 » au lieu de « 1er »).
  - `citations.py:315` — `re.search(r"\[(.+?)\]", titre)` prend le **premier** crochet : avec un titre contenant un pseudonyme « M. [X] [Y] [Objet réel] » → « … - X » ; avec crochets imbriqués « [Contribution [art. 1]] » → « Contribution [art. 1 » (démontrés). Les titres réels de CONSTIT de la fixture n'ont qu'un crochet (« M. Amine G. [Absence de publicité …] ») ; la fréquence de crochets multiples est **NON VÉRIFIÉE**.
  - `nature` CONSTIT injectée telle quelle en majuscules (`citations.py:305`) : correct pour QPC/DC ; libellés d'autres natures (contentieux électoral, etc.) **NON VÉRIFIÉS**.
- **Mesure Rust** : `domain::citation::CitationFormatter` avec table des mois accentuée, « 1er », « décision », « délibération » ; extraction de l'objet entre crochets par analyse équilibrée sur le segment après la date, ou mieux : usage du champ structuré s'il existe (à identifier dans l'API — NON VÉRIFIÉ) ; la citation calculée est **toujours accompagnée** du `titre` brut verbatim (`official_title`), pour que le LLM puisse vérifier.
- **Tests** : golden strings : « Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 » ; « CNIL, délibération n° SAN-2026-002 du 8 janvier 2026 » ; `first_of_month_is_1er` ; `months_have_accents` (12 cas) ; `nested_brackets_are_balanced`.

#### D-28 — Routage d'URL par `origine` au lieu du préfixe ; préfixes incomplets
- **Gravité** : mineur.
- **Preuves** : `citations.py:333-334` — `url_kind` dérivé de `text["origine"]` ; démontré : `origine=None` pour un `CETATEXT…` → `https://www.legifrance.gouv.fr/juri/id/CETATEXT000054178503` au lieu de `/ceta/id/…` ; `origine="constit "` (espace) → `/juri/id/CONSTEXT…` et citation CONSTIT non appliquée (`:338`). Or le préfixe d'ID a déjà été validé par l'outil (`server.py:338`). `_ID_PREFIXES` (`citations.py:34-44`) ne contient ni `CONSTEXT`, ni `JUFITEXT`, ni `LEGISCTA` : `strip_date_suffix("CONSTEXT000052555275_07-11-2025")` est renvoyé tel quel (démontré). Remarque : pylegifrance affirme que `JURITEXT` et `CETATEXT` « both resolve under the same /juri/id/ path » (`pylegifrance/fonds/juri.py:91-98`) tandis que la référence a vérifié `/ceta/id/` (`DISCOVERY.md`, section « Update 2026-07-07 ») — les deux formes peuvent coexister ; **NON VÉRIFIÉ** laquelle est canonique.
- **Mesure Rust** : `enum DecisionId { Juri(JuriTextId), Constit(ConsTextId), Cetat(CetaTextId) }` dans `domain` ; l'URL et le formateur de citation sont choisis par le **type** de l'ID ; `origine` n'est qu'un contrôle de cohérence (avertissement si divergent).
- **Tests** : `url_depends_on_id_type_not_payload_origine` ; `strip_date_suffix_handles_all_known_prefixes`.

#### D-29 — URL d'article de loi
- **Gravité** : mineur.
- **Preuves** : `citations.py:262` — `url_kind = "kali" if … else "code_article"` → `/codes/article_lc/LEGIARTI…` pour **tout** article LEGI, y compris ceux d'une loi (le test smoke l'impose pour un article de la loi 2016-1321 : `tests/test_smoke.py:57`). Le motif `https://www.legifrance.gouv.fr/loda/article_lc/LEGIARTI…` existe (résultats indexés, ex. https://www.legifrance.gouv.fr/loda/article_lc/LEGIARTI000006716311, et version datée https://www.legifrance.gouv.fr/loda/article_lc/LEGIARTI000006258433/2011-08-31). Que `/codes/article_lc/` redirige correctement pour un article de loi : **NON VÉRIFIÉ**.
- **Mesure Rust** : `ArticleUrl::for_article(article_id, parent_nature)` : `CODE` → `/codes/article_lc/`, sinon `/loda/article_lc/`, KALI → `/conv_coll/id/` (forme reprise de la référence, `citations.py:97-98`) ; variante datée `…/{id}/{YYYY-MM-DD}` pour `fr_diff_article`.
- **Test** : `loda_article_url_uses_loda_path` (vérification manuelle des URL en recette).

#### D-30 — Contrat de citation non garanti ; `eli_uri` trompeur
- **Gravité** : majeur.
- **Preuves** : `CONSTITUTION.md:37-46` (« Every response MUST carry the citation contract ») vs modèles où **tous** les champs du contrat sont optionnels (`models.py:51-53`, `95-97`, `119-121`, `145-147`, `168-170`, `188-190`) et JSON Schema publié en conséquence (`tests/tools.lock.json`, ex. `fr_get_decision.outputSchema.properties.source_url = anyOf[string,null]`). Démontré : un hit LODA sans `id` mais avec `cid` → `source_url = None` (`citations.py:166-168` construit l'URL avec `text_id` seulement). `eli_uri` est systématiquement égal à `source_url` (`citations.py:170-171`, `:226-227`…) : doublon de tokens et nom trompeur (ce n'est pas un ELI) ; `eli_note`/`dataset_note` (171 + 335 caractères, `models.py:7-17`) répétés dans chaque réponse.
- **Mesure Rust** : `domain::CitationContract { source_url: Url, human_readable_citation: NonEmptyString, official_title: Option<String>, eli: Option<Eli>, ecli: Option<Ecli> }` — impossible de construire une réponse sans URL ni citation ; un hit incomplet est rejeté (compté) plutôt que renvoyé à moitié ; la note ELI est déclarée **une seule fois** (description d'outil / ressource MCP), pas répétée.
- **Tests** : `every_normalized_fixture_has_contract` ; `hit_without_resolvable_url_is_dropped_and_counted`.

#### D-31 — Validation faible des entrées et schémas permissifs
- **Gravité** : mineur.
- **Preuves** : `server.py:159-165` (`_require_prefix`) et `:284`, `:338` : contrôle de **préfixe** seulement ; `"LEGITEXT"` seul ou `"LEGITEXT1/../../x?y"` passent (le second est envoyé comme valeur JSON — inoffensif pour la requête, mesuré `legifrance_url("LEGITEXT1/../../x?y","loda")` → URL non échappée si l'API renvoyait cet id) ; espace final non retiré (`"LEGITEXT… "` passe et part avec l'espace). JSON Schema : `fond` = `{"type":"string"}` sans `enum`, `page_size` = `{"type":"integer"}` sans `minimum/maximum` (`tests/tools.lock.json`, `fr_search.inputSchema`) — la contrainte 1..100 n'est que dans la prose (`server.py:185`) et le code (`:196-197`). Le maximum 100 côté API est cohérent avec la description OpenAPI « Nombre de résultats par page (max 100) » (`pylegifrance/models/generated/model.py:112-117`, généré depuis `legifrance.json`).
- **Mesure Rust** : newtypes `LegiTextId`, `LegiArtiId`, `JuriTextId`, `ConsTextId`, `CetaTextId`, `CnilTextId`, `KaliTextId`, `KaliArtiId`, `AccoTextId` avec `FromStr` strict (préfixe + 12 chiffres, suffixe `_JJ-MM-AAAA` toléré et retiré — format exact à confirmer sur un échantillon plus large, NON VÉRIFIÉ) ; `Fond` enum sérialisée en `enum` JSON Schema (`schemars` 1.2.3 observée) ; `page_size` borné 1..=100 dans le schéma.
- **Tests** : proptest `id_parse_roundtrip` ; `schema_has_enum_for_fond_and_bounds_for_page_size` (snapshot du schéma).

#### D-47 — `fr_get_act` accepte un identifiant de code
- **Gravité** : mineur (**NON VÉRIFIÉ** côté API).
- **Preuves** : `fr_search` sur `CODE` renvoie un `id` `LEGITEXT000006070721` (Code civil, `search_code.json`) ; `fr_get_act` n'exige que le préfixe `LEGITEXT` (`server.py:242`). Comportement de `/consult/lawDecree` sur un code et taille de la réponse : **NON VÉRIFIÉS** ; l'API dispose d'endpoints dédiés `consult/code` et `consult/legi/tableMatieres` (`pylegifrance/fonds/code.py:491`, `pylegifrance/models/constants.py:359`).
- **Mesure Rust** : distinguer `LodaTextId` et `CodeId` au niveau du domaine (la nature vient du hit de recherche) ; outil de navigation de code paginé distinct.
- **Test** : `get_act_rejects_code_id_with_hint` (message orientant vers l'outil code).

### 3.8 Recherche

#### D-09 — Ni pagination, ni tri, ni filtres, ni facettes
- **Gravité** : majeur.
- **Preuves** : §2.d ; en outre : `totalResultNumber` exposé (`citations.py:179-182`) — ex. 122 147 pour JURI « responsabilite » (`search_juri.json`), 51 531 CETAT (`search_cetat.json`) — sans moyen d'aller au-delà de 100 ; facettes renvoyées par l'API (ex. 9 facettes JURI, 8 CONSTIT) jetées ; pour CODE, `total` = nombre de **codes** (1 dans `search_code.json`) alors que `totalArticleResultNumber`/`moreArticlesCount` sont ignorés ; les extraits KALI (`sections[].extracts[].id = KALIARTI…`) ne sont exploités que pour CODE (`citations.py:150-165`). Le tiers pylegifrance pagine bien (`tests/integration/fonds/juri/steps.py:35-41` : `page_number=1` puis `2`).
- **Mesure Rust** : `SearchQuery { page: NonZeroU32, page_size: 1..=100, sort: Pertinence|DateAsc|DateDesc, filters: { date_range, juridiction, nature, code } }` (valeurs de tri et facettes exactes à relever dans la doc officielle — NON VÉRIFIÉES ici) ; la réponse contient `page`, `page_size`, `total`, `has_more`, `next_page` ; facettes optionnelles résumées.
- **Tests** : `search_page_2_sends_page_number_2` (wiremock sur le corps) ; `has_more_computed_from_total` ; `code_search_reports_article_totals`.

#### D-32 — Mode de recherche figé `UN_DES_MOTS`, requête non bornée
- **Gravité** : majeur (pertinence).
- **Preuves** : `server.py:144` — `{"typeRecherche": "UN_DES_MOTS", "valeur": query, "operateur": "ET"}` ; les valeurs possibles de l'API sont `UN_DES_MOTS`, `EXACTE`, `TOUS_LES_MOTS_DANS_UN_CHAMP`, `AUCUN_DES_MOTS`, `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION` (`pylegifrance/models/generated/model.py:1885-1892`), avec un critère optionnel `proximite` (`:1895-1899`). « Un des mots » = OU logique : une requête « republique numerique » ramène tout texte contenant l'un ou l'autre (1 786 résultats LODA, `search_loda.json`). Aucune borne de longueur sur `query` (seulement « non vide », `server.py:194-195`) ; la requête complète est écrite en clair dans la clé de cache (D-15) et renvoyée dans `SearchResult.query` (`server.py:216`). Pas d'injection structurelle possible (sérialisation JSON par httpx), mais la syntaxe interne du moteur (guillemets, jokers) est **NON VÉRIFIÉE**.
- **Mesure Rust** : `SearchMode` enum exposée à l'outil (défaut `tous_les_mots`, plus précis — choix à valider), `query` bornée (ex. 1..=500 caractères), caractères de contrôle retirés.
- **Tests** : `mode_maps_to_type_recherche` ; `query_too_long_is_invalid_arg`.

### 3.9 Taille des réponses et économie de contexte

#### D-33 — Taille non maîtrisée
- **Gravité** : majeur (au cœur du « retour progressif » demandé).
- **Preuves** :
  - Mesures (§4) : une décision CONSTIT/CETAT normalisée ≈ 16 Ko (≈ 4 000 tokens à 4 octets/token, estimation grossière), dont ≈ 15 Ko de texte ; aucune option `max_chars`/`offset`/résumé ; tailles maximales réelles des décisions et des tables des matières **NON VÉRIFIÉES** (fixtures tronquées).
  - Doublon sur le fil : FastMCP renvoie `content = _convert_to_content(structured)` **et** `structured_content = structured` quand un schéma de sortie existe (`fastmcp_slim/fastmcp/tools/base.py:409-430` @ e2fcc41), conformément à la spec (« For backwards compatibility, a tool that returns structured content SHOULD also return the serialized JSON in a TextContent block », `docs/specification/2025-11-25/server/tools.mdx:326`, idem `2026-07-28/server/tools.mdx:500`). Le fait que le modèle voie les deux dépend du client — **NON VÉRIFIÉ**.
  - Coût fixe par session : descripteurs d'outils ≈ 17 643 octets dont 12 167 de `outputSchema` (69 %), plus `INSTRUCTIONS` = 4 470 caractères (`server.py:31-66`) → ≈ 22 Ko (≈ 5 500 tokens) avant tout appel (mesuré sur `tests/tools.lock.json`).
  - Redondances par réponse : `title` == `human_readable_citation` pour JURI/CETAT (`citations.py:337`, `:350-352`) ; `eli_uri` == `source_url` ; notes répétées (D-30).
  - Inversement, information utile jetée : les extraits de recherche `text` (ex. « [...] Les consorts [E] font grief … », `search_juri.json`) ne sont pas renvoyés hors CODE (`citations.py:118-125`) → le LLM doit ouvrir des décisions entières pour juger de leur pertinence.
- **Mesure Rust** (`application` + `adapter-mcp`) : réponses en trois niveaux : `summary` (métadonnées + citation + 1er paragraphe/extrait, défaut), `paged` (`offset` en paragraphes + `max_chars`, réponse portant `next_offset`), `focus` (passages classés BM25 sur les paragraphes du document pour une requête donnée, `top_k`) ; plafond dur de taille de réponse ; extraits de recherche conservés (débalisés, ≤ 280 caractères comme la référence le fait pour CODE, `citations.py:162`) ; schémas de sortie compacts (pas de répétition de notes) et, si le client le permet, texte lisible court + `structuredContent` (stratégie exacte à décider : point ouvert).
- **Tests** : `default_decision_response_under_budget` (ex. ≤ 2 Ko sur les fixtures CONSTIT/CETAT) ; `paged_reading_reconstructs_full_text` (concaténation des pages == texte complet) ; `focus_returns_relevant_paragraphs` (requête « publicité des débats » sur la fixture CONSTIT → paragraphe contenant ces mots en tête) ; `tool_descriptors_total_size_budget` (snapshot de taille).

### 3.10 Couverture déclarée

#### D-34 — `fr_coverage` incomplet et incohérent
- **Gravité** : majeur (c'est l'outil censé empêcher « absence ici ≠ absence en droit », `server.py:46`).
- **Preuves** : `coverage.py:65` — 3 familles seulement : « Legislation (LODA, codes) », « Case law (JURI) » avec `tool = 'fr_get_decision'` (CONSTIT et CETAT non nommés, `fr_search` absent), « Agreements and deliberations » ; `coverage.py:67` — 2 lacunes (ELI nul, force juridique non évaluée) ; `coverage.py:73` — `status="ok"` codé en dur (jamais `degraded`/`failed`, même sans identifiants ou en sandbox) ; `coverage.py:75` — `live=True` pour tout. Lacunes connues **non déclarées** alors qu'elles figurent ailleurs dans le dépôt : ACCO métadonnées seules (`server.py:42`, `citations.py:444-449`), ECLI souvent nul en CAA/TA (`server.py:48`, `DISCOVERY.md` update 2026-07-07), Judilibre non intégré (`SOURCES.md:12`), fonds JORF non exposé (`_FOND_MAP`, `server.py:91-100` ; `CONSTITUTION.md:58`), pagination limitée à la 1re page (D-09), sandbox par défaut (D-18), pseudonymisation JURI et occultation CNIL (D-45), BOFiP/AN/Sénat/ADLC « todo » (`SOURCES.md:16-19`). Les tests (`tests/test_coverage.py:21-60`) vérifient la non-vacuité mais pas la cohérence outils↔familles.
- **Mesure Rust** : `CoverageService` (application) qui assemble une déclaration statique **versionnée** (fichier RON/TOML embarqué, une famille par fond) + état dynamique (`env`, identifiants présents, dernier succès/échec upstream, quotas atteints) → `status` calculé ; exposé aussi comme ressource MCP.
- **Tests** : `every_registered_tool_belongs_to_a_family` ; `every_fond_has_a_family` ; `coverage_status_degraded_without_credentials` ; `coverage_lists_known_gaps` (liste minimale : ACCO texte, ECLI CAA/TA, JORF, Judilibre, pagination, sandbox, pseudonymisation).

### 3.11 Tests, fixtures, mojibake, verrou des descripteurs, CI

#### D-35 — Fixtures non fiables (mojibake, troncature, absence de métadonnées)
- **Gravité** : majeur.
- **Preuves** :
  - Mojibake (détail §5) : **6 fixtures sur 16** — `consult_article.json`, `consult_juri.json`, `consult_loda.json`, `search_code.json`, `search_juri.json`, `search_loda.json` — contiennent **235 séquences** de mojibake réparties sur 60 champs, et **0** caractère accentué correct ; 100 % des champs touchés se réparent par un unique aller-retour `str.encode('latin-1').decode('utf-8')`. Les 10 autres fixtures n'en contiennent aucune (et 3 305 caractères accentués corrects au total). Les deux fichiers corrompus qui contiennent des octets 0x80-0x9F en portent sous forme de **caractères de contrôle C1** (12 au total, ex. « PREMIÃ\x88RE ») : le décodage fautif est **ISO-8859-1** (un décodage Windows-1252 aurait produit « ˆ » U+02C6).
  - Attribution : les 6 fixtures corrompues correspondent exactement au périmètre de la v0.1.0 (fonds `LODA_DATE`, `CODE_DATE`, `JURI` ; endpoints `search`, `lawDecree`, `getArticle`, `juri` — `DISCOVERY.md:18-33`, 2026-06-24) ; les fixtures `consult_juri_constit.json` et `consult_juri_cetat.json`, issues **du même endpoint** `/consult/juri` (v0.2.0, 2026-07-07), sont propres. Le client de référence décode les octets (`httpx.Response.json()` → `json.loads(self.content)`, `httpx/_models.py:831-832`), il ne peut donc pas produire ce défaut à partir d'une réponse UTF-8. Conclusion : **très probablement un artefact de l'outil de capture de juin 2026** (outil inconnu ; hypothèse — non vérifiable sans historique git, le clone n'a qu'un commit — : un client HTTP appliquant le défaut ISO-8859-1 à un `Content-Type` sans `charset`) ; qu'un sous-ensemble ancien des données JURI/LEGI soit corrompu à la source est **NON VÉRIFIÉ** (nécessite un appel live).
  - Troncature : `consult_juri.json` `text.texte` = exactement 600 caractères, coupé au milieu d'une phrase ; `consult_cnil.json` `texte` et `texteHtml` = 606 caractères se terminant par « [...] » (600 + suffixe) ; `consult_kali.json` `articles[].content` = 406 caractères terminés par « [...] » ; `consult_loda.json` : toutes les listes plafonnent à **5** éléments (26 sections, numéros d'articles 1-5 puis 17-21 puis 25-29…), articles réduits à 3 clés (`id`, `num`, `etat`) et sections à 4 clés → la loi 2016-1321 ne compte que 62 articles dans la fixture. Les résultats de recherche ont 1 à 3 hits.
  - Aucune métadonnée de capture (requête envoyée, date, environnement, règles d'élagage).
  - Assertion vide de sens : `tests/test_parse.py:28` — `assert "Rpublique" not in (hit["title"] or "")` (la chaîne réelle est « RÃ©publique » ; « Rpublique » n'apparaît jamais) ; aucun test ne détecte le mojibake, donc les tests de parsing passent sur des données corrompues.
- **Conséquence** : les tests ne prouvent ni la fidélité des accents (cœur des citations françaises) ni le comportement sur des volumes réels (pagination de texte, taille).
- **Mesure Rust** : `tests/fixtures/<fond>/<cas>.json` **brut non modifié** + `<cas>.meta.toml` (`endpoint`, `request` exact, `captured_at`, `env`, `sha256_raw`, `tool`) ; fixtures recapturées en UTF-8 strict ; si élagage nécessaire, faire une fixture « réduite » *dérivée* par script versionné à côté de la brute.
- **Tests** : `fixtures_have_no_mojibake` (regex `[ÂÃ][\u0080-¿]|â[\u0080-¿]{2}` + absence de U+0080–U+009F) ; `fixtures_have_meta_sidecar` ; `fixture_sha256_matches_meta`.

#### D-36 — Couverture de tests et CI insuffisantes
- **Gravité** : majeur.
- **Preuves** : fichiers de test : `conftest.py`, `test_coverage.py`, `test_descriptor_lock.py`, `test_instructions_drift.py`, `test_parse.py`, `test_smoke.py` — **aucun** test de `client.py` (retry, 401, jeton, cache), `cache.py`, `audit.py` (hors un chemin heureux), `runtime.py`, `_map_upstream` ; les smoke tests (réseau) sont exclus du CI (`.github/workflows/ci.yml:36` : `-k "not smoke"`). CI sur Python **3.12** uniquement (`ci.yml:30`, `release.yml:26`) alors que `pyproject.toml:11` annonce `>=3.11` et que l'image Docker est en 3.11 (`Dockerfile:3`) ; `ruff` et `mypy --strict` sont configurés (`pyproject.toml`, sections `[tool.ruff]`, `[tool.mypy]`) mais jamais exécutés en CI ; aucun test Windows alors que Windows est une cible documentée (README) et que D-20 en dépend.
- **Mesure Rust** : pyramide de tests : unitaires `domain` (proptest), intégration adapter avec `wiremock` (0.6.5 observée), snapshots `insta` (1.49.0 observée) des sorties MCP, test end-to-end stdio (lancer le binaire, `initialize`, `tools/list`, `tools/call` contre wiremock) ; CI : `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` sur **Linux + Windows + macOS**, `cargo deny`/`cargo audit` ; smoke live dans un job séparé manuel/nocturne non bloquant.
- **Tests** : la matrice §6.

#### D-37 — Verrou des descripteurs fragile
- **Gravité** : mineur.
- **Preuves** : `tests/test_descriptor_lock.py:62-83` compare un SHA-256 du descripteur complet sérialisé par FastMCP, incluant `meta.fastmcp.tags` (`tests/tools.lock.json` ; émis par `fastmcp_slim/fastmcp/utilities/components.py:201`) et les `outputSchema` générés par pydantic ; dépendances non épinglées (`pyproject.toml:28-32` : `fastmcp>=3.4,<4`, `pydantic>=2.6`, `diskcache>=5.6`, sans fichier de verrouillage) et CI qui installe la dernière version (`ci.yml:32-33`) → le test peut rougir sans changement de code ; `package_version` est écrit dans le lock (`:56`) mais jamais comparé à `pyproject.toml` ; `INSTRUCTIONS` (vues par le client à `initialize`) ne sont pas verrouillées ; `tests/test_instructions_drift.py:21-25` dépend de l'attribut privé `mcp._tool_manager` avec repli sur une regex du source.
- **Mesure Rust** : `Cargo.lock` versionné ; snapshot `insta` d'une **forme normalisée** des descripteurs (nom, description, `inputSchema`, `outputSchema`, annotations — sans méta propre au SDK) + des instructions serveur ; test qui exige un bump de `version` du crate quand le snapshot change (comparaison avec le snapshot de la dernière version taggée).
- **Tests** : `tool_descriptors_snapshot` ; `server_instructions_snapshot` ; `descriptor_change_requires_version_bump`.

### 3.12 Performance et E/S

#### D-38 — E/S synchrones dans la boucle d'événements
- **Gravité** : majeur en mode HTTP (prévu), mineur en stdio mono-utilisateur.
- **Preuves** : `cache.py:36-40` (`diskcache` = SQLite synchrone) appelé dans `async def _post` (`client.py:152`, `:171`) ; `audit.py:73-75` (écriture fichier synchrone) dans chaque outil `async` ; `runtime.py:55-57` (D-05).
- **Mesure Rust** : store disque via `spawn_blocking` ou crate async ; audit via tâche dédiée (D-16) ; aucune E/S bloquante dans les handlers.
- **Test** : `concurrent_tool_calls_progress_while_disk_is_slow` (store factice qui dort 500 ms en `spawn_blocking` ; un second appel en cache mémoire répond immédiatement).

### 3.13 Sécurité des messages, diagnostics

#### D-40 — Contenu des messages d'erreur
- **Gravité** : mineur.
- **Preuves** : `server.py:116-119` inclut `{exc}` ; pour un `HTTPStatusError`, le message httpx est « `{error_type} '{status} {reason}' for url '{url}'\nFor more information check: https://developer.mozilla.org/…` » (`httpx/_models.py:815-818`) — l'URL est formatée via `URL.__str__` qui **ne masque pas** le mot de passe d'un userinfo (`httpx/_urls.py:374-375`, alors que `__repr__` le masque, `:377-382`) → fuite si un utilisateur met des identifiants dans `FR_ELI_BASE_URL`/`FR_ELI_OAUTH_URL` (cas limite) ; sinon, pas de secret dans l'URL (jeton en en-tête `client.py:162`, `client_secret` dans le corps `client.py:78-84`). L'erreur complète (500 caractères) part aussi dans l'audit (`server.py:208-210`, `audit.py:72`). Une `OSError` d'audit révèle le chemin local `~/.matematic/audit/…` (D-16).
- **Mesure Rust** : `SecretString` (`secrecy` 0.10.3 observée) pour `client_secret` et le jeton (jamais `Debug`/`Display`) ; URLs journalisées via une fonction `redact_url` (sans userinfo ni query) ; messages LLM sans URL interne ni chemin local ; détails techniques seulement dans `tracing` (stderr).
- **Tests** : `error_messages_never_contain_secret_or_userinfo` (proptest : identifiants aléatoires injectés partout → absents des messages, logs capturés et audit) ; `debug_of_config_redacts_secret`.

#### D-46 — Aucun log de diagnostic
- **Gravité** : mineur.
- **Preuve** : aucune occurrence de `import logging`/`logger` dans `src/` (grep) ; seuls les logs internes de FastMCP existent.
- **Mesure Rust** : `tracing` + `tracing-subscriber` (0.3.23 observée) **sur stderr** (stdout réservé au protocole stdio), niveau via `RUST_LOG`, champ `request_id` partagé avec l'audit, spans par appel upstream (fond, endpoint, statut, tentative, durée).
- **Test** : `stdout_contains_only_jsonrpc` (test e2e stdio : chaque ligne de stdout parse en JSON-RPC).

### 3.14 Documentation, configuration, distribution

#### D-42 — Noms des variables d'identifiants faux dans le README
- **Gravité** : majeur (UX : l'installation « one command » échoue).
- **Preuves** : `README.md:14` — « Requires PISTE credentials in env: `PISTE_CLIENT_ID`, `PISTE_CLIENT_SECRET` » ; le code lit `FR_ELI_CLIENT_ID`/`FR_ELI_CLIENT_SECRET` (`client.py:51-52`), comme le reste du README (`README.md:124-125`) et `.mcp.json.example:9-10`.
- **Mesure Rust** : une seule définition de la configuration (struct `Config` avec noms de variables en constantes) d'où sont générés l'aide `--help` et la section README (ou test de cohérence).
- **Test** : `readme_mentions_only_supported_env_vars` (extrait les `[A-Z_]{6,}` du README et compare à la liste des variables supportées).

#### D-43 — Autres dérives
- **Gravité** : mineur.
- **Preuves** : User-Agent `fr-eli-mcp/0.3.0` (`client.py:31`) vs version 0.5.3 (`src/fr_eli_mcp/__init__.py:3`, `pyproject.toml:7`, `server.json:5`) ; `FR_ELI_ENV` dans `.mcp.json.example:6` n'est lu nulle part ; `CONSTITUTION.md:58-59` (« Other fonds (JORF, CETAT, CONSTIT, KALI) are later features ») obsolète ; `CONSTITUTION.md:63` exige un `CHANGELOG.md` absent du dépôt ; INSTRUCTIONS : `not_found` « or a search returned nothing » (`server.py:57`) alors que `fr_search` renvoie `total=0` sans erreur (`server.py:213-222`) ; « Retry once before surfacing » (`server.py:58`) s'ajoute aux 3 tentatives internes ; `citation_grammar_version` du manifeste jamais lu ; commentaire de section mal placé (`server.py:448-449` annonce `fr_get_company_agreement` mais précède `fr_coverage`) ; docstring de `__main__.py` en polonais (`src/fr_eli_mcp/__main__.py:1-9`) ; `Decision` documenté « a JURI court decision » (`models.py:174`) alors qu'il couvre trois juridictions.
- **Mesure Rust** : version unique `env!("CARGO_PKG_VERSION")` pour UA, `serverInfo`, audit ; test de cohérence INSTRUCTIONS ↔ codes d'erreur ↔ comportements (ex. pas de mention « retry » pour les codes non réessayables).
- **Tests** : `user_agent_contains_crate_version` ; `instructions_reference_only_existing_tools_and_codes`.

#### D-44 — Chaîne CI/release et image
- **Gravité** : mineur.
- **Preuves** : `release.yml:84` — `curl -L ".../registry/releases/latest/download/mcp-publisher_…tar.gz" | tar xz` sans somme de contrôle ni version ; actions épinglées par tag (`actions/checkout@v4`, `pypa/gh-action-pypi-publish@release/v1`, `release.yml:40`) ; publication PyPI par jeton (`release.yml:42`) alors que `id-token: write` est déjà accordé ; job « runtime manifest » (`release.yml:44-69`) qui couple toutes les versions installées à la dernière release (D-04) ; `Dockerfile:4` installe `fr-eli-mcp` sans version et s'exécute en root.
- **Mesure Rust** : actions épinglées par SHA, binaires de release avec sommes de contrôle et attestation de provenance (outil exact à choisir — NON VÉRIFIÉ), image Docker multi-étapes non-root avec version épinglée.
- **Test** : vérification CI (ex. `zizmor`/`actionlint` — à évaluer) ; non fonctionnel.

#### D-45 — Pseudonymisation/occultation non signalées ; attribution Etalab
- **Gravité** : mineur.
- **Preuves** : JURI pseudonymisé (« Mme [M] [E], épouse [I], domiciliée [Adresse 5] », `consult_juri.json` `text.texte`) ; CNIL occulté (« … sont remplacés par le signe […] », `consult_cnil.json` `text.texte`) — aucune note dans les réponses ni dans `fr_coverage`. Licence des données : Etalab 2.0, qui demande de mentionner la source et la date de dernière mise à jour (`pylegifrance/docs/src/content/docs/en/concepts/api-terms-and-quotas.md:51-60`, de seconde main) ; la référence jette les champs de mise à jour disponibles (`dateMaj` ACCO, `modifDate` LODA/KALI). L'applicabilité de cette obligation à la sortie d'un connecteur MCP est un **point ouvert**.
- **Mesure Rust** : champs `redaction: Pseudonymized|Redacted|None` (déduit du fond) et `last_update: Option<LegalDate>` dans la provenance ; mention de la licence dans les métadonnées serveur/ressource.
- **Test** : `juri_and_cnil_responses_carry_redaction_note`.

---

## 4. Tailles mesurées (fixtures)

Méthode : `raw` = JSON compact UTF-8 de la fixture ; `norm` = JSON compact de la sortie de `citations.normalize_*` (sans les notes constantes ajoutées par pydantic : +171 octets `eli_note` pour Act/ArticleText, +506 pour SearchResult, et **sans** le doublon TextContent de FastMCP) ; tokens ≈ octets/4 (estimation grossière).

| Fixture | raw (o) | norm (o) | ratio | texte (o) | ≈ tokens norm |
|---|---:|---:|---:|---:|---:|
| search_loda.json (2 hits) | 2 989 | 1 416 | 0,47 | – | 354 |
| search_code.json (1 hit) | 1 439 | 535 | 0,37 | – | 133 |
| search_juri.json (2 hits) | 10 287 | 979 | 0,10 | – | 244 |
| search_constit.json (3 hits) | 7 642 | 1 831 | 0,24 | – | 457 |
| search_cetat.json (3 hits) | 9 886 | 1 238 | 0,13 | – | 309 |
| search_cnil.json (2 hits) | 3 133 | 1 247 | 0,40 | – | 311 |
| search_kali.json (2 hits ; facettes = 33 554 o) | 37 664 | 1 129 | 0,03 | – | 282 |
| search_acco.json (2 hits) | 6 321 | 901 | 0,14 | – | 225 |
| consult_loda.json (élaguée, 62 art.) | 7 586 | 4 674 | 0,62 | – | 1 168 |
| consult_article.json | 2 640 | 776 | 0,29 | 364 | 194 |
| consult_juri.json (texte tronqué à 600 car.) | 7 591 | 1 257 | 0,17 | 644 | 314 |
| consult_juri_constit.json | 34 094 | 16 005 | 0,47 | 15 188 | 4 001 |
| consult_juri_cetat.json | 38 271 | 15 639 | 0,41 | 15 055 | 3 909 |
| consult_cnil.json (texte tronqué) | 3 618 | 1 283 | 0,35 | 634 | 320 |
| consult_kali.json | 4 057 | 951 | 0,23 | – | 237 |
| consult_acco.json | 1 071 | 815 | 0,76 | – | 203 |

Lecture : un hit de recherche normalisé pèse ≈ 300-600 o → 10 hits (défaut) ≈ 3-6 Ko, 100 hits ≈ 30-60 Ko ; une décision complète ≈ 16 Ko, doublée sur le fil par FastMCP. Coût fixe par session : ≈ 17,6 Ko de descripteurs + 4,5 Ko d'INSTRUCTIONS. Les maxima réels (grandes décisions d'Assemblée du CE, gros arrêts, TOC de lois longues, codes) sont **NON VÉRIFIÉS** faute de fixtures non tronquées.

---

## 5. Analyse du mojibake (fixtures)

Motif compté : `[ÂÃ][\u0080-¿]` ou `â[\u0080-¿]{2}` dans toutes les chaînes JSON ; « réparable » = `s.encode('latin-1').decode('utf-8')` réussit.

| Fixture | séquences mojibake | champs touchés | réparables | accents corrects | C1 (U+0080-009F) |
|---|---:|---:|---:|---:|---:|
| consult_article.json | 14 | 1 | 1/1 | 0 | 0 |
| consult_juri.json | 85 | 8 | 8/8 | 0 | 6 |
| consult_loda.json | 49 | 25 | 25/25 | 0 | 6 |
| search_code.json | 3 | 1 | 1/1 | 0 | 0 |
| search_juri.json | 69 | 22 | 22/22 | 0 | 0 |
| search_loda.json | 15 | 3 | 3/3 | 0 | 0 |
| 10 autres fixtures (acco, cnil, kali, constit, cetat, consult_*) | 0 | 0 | – | 3 305 au total | 0 |
| **Total** | **235** | **60** | **60/60** | | **12** |

Exemples : `consult_loda.json` `$.title` = « LOI nÂ° 2016-1321 du 7 octobre 2016 pour une RÃ©publique numÃ©rique (1) » → réparé « LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1) » ; `consult_juri.json` `$.text.texte` commence par « LA COUR DE CASSATION, PREMIÃ\x88RE CHAMBRE CIVILE ».

Ce qui est vérifiable : la corruption est (i) totale dans 6 fichiers et nulle dans 10, (ii) entièrement réversible par un unique décodage ISO-8859-1 fautif, (iii) limitée au périmètre capturé le 2026-06-24, (iv) absente de fixtures plus récentes issues des **mêmes endpoints** (`/consult/juri`, `/search`), (v) impossible à produire par le client de référence lui-même (`json.loads(bytes)`). Ce qui ne l'est pas : l'outil de capture exact et l'absence totale de corruption à la source pour ces documents (nécessite un appel live : **NON VÉRIFIÉ**).
Conséquence pour le plan Rust : (1) recapturer ces 6 fixtures ; (2) test « anti-mojibake » sur fixtures **et** sur sorties (en mode debug, `tracing::warn!` si une réponse upstream contient le motif — défense en profondeur sans jamais « réparer » silencieusement un texte officiel).

---

## 6. Matrice consolidée des tests de non-régression Rust

| Test | Crate | Défauts couverts |
|---|---|---|
| `cache_key_is_stable_across_calls_same_day`, `second_identical_loda_search_hits_cache`, `cache_key_changes_when_paris_day_changes` | application | D-01 |
| `cache_namespaced_by_env`, `cache_write_failure_does_not_fail_tool`, `negative_result_ttl_is_short`, `disk_cache_never_contains_raw_query_when_disabled` | application / store | D-15 |
| `audit_written_for_every_outcome` (8 issues), `audit_failure_returns_audit_error`, `audit_failure_does_not_mask_original_cause` | application + adapter-audit | D-02 |
| `error_duration_is_nonzero`, `duration_includes_normalization` | application | D-03 |
| `concurrent_writers_produce_valid_jsonl`, `rotation_creates_new_file_after_limit`, `audit_write_is_acknowledged_before_tool_returns`, `audit_record_schema_snapshot` | adapter-audit | D-16, D-17 |
| `config_rejects_non_allowlisted_host`, `config_rejects_http_scheme`, `no_network_call_except_piste`, `startup_fails_without_env`, `missing_credentials_makes_no_network_or_disk_io` | app | D-04, D-18, D-39 |
| `server_answers_ping_while_tool_waits_upstream`, `concurrent_tool_calls_progress_while_disk_is_slow` | adapter-mcp / e2e | D-05, D-38 |
| `oauth_401_maps_to_config_error`, `oauth_404_maps_to_config_error_not_not_found`, `api_route_404_is_not_not_found`, `oauth_expires_in_null_uses_default` | adapter-legifrance | D-10 |
| `html_body_maps_to_invalid_response`, `json_null_body_maps_to_invalid_response_or_not_found`, fuzz « jamais de panique » | adapter-legifrance | D-11, D-23, D-24 |
| `401_then_503_then_200_requests_exactly_two_tokens`, `429_retry_after_is_honoured`, `concurrent_calls_share_one_token_refresh` | adapter-legifrance | D-12 |
| `rate_limiter_spaces_requests`, `search_all_respects_concurrency_cap` | application | D-13 |
| `tool_returns_timeout_code_before_deadline`, `oversized_body_is_rejected` | application / adapter | D-14, D-41 |
| `error_result_has_structured_code` (snapshot), `invalid_arg_is_not_logged_as_error` | adapter-mcp | D-07, D-08 |
| `today_is_paris_date_at_2330_utc_in_summer`, `dst_transition_days` | domain/application | D-19 |
| `pre_1970_dates_roundtrip` (CI Windows), `sentinel_2999_is_open_not_a_date`, `bool_is_rejected`, `iso_plus0000_string_parses`, `invalid_date_is_rejected_locally` | domain | D-20, D-21, D-22 |
| `toc_preserves_section_titles`, `toc_sorted_by_int_ordre`, `toc_dedups_articles`, `kali_inline_content_is_available_without_extra_call` | domain/adapter | D-25 |
| `cetat_fixture_yields_multiple_paragraphs`, `verbatim_text_is_unchanged_except_markup` | domain | D-26 |
| golden citations, `first_of_month_is_1er`, `months_have_accents`, `nested_brackets_are_balanced` | domain | D-27 |
| `url_depends_on_id_type_not_payload_origine`, `loda_article_url_uses_loda_path`, `every_normalized_fixture_has_contract` | domain | D-28, D-29, D-30 |
| `id_parse_roundtrip`, `schema_has_enum_for_fond_and_bounds_for_page_size`, `get_act_rejects_code_id_with_hint` | domain / adapter-mcp | D-31, D-47 |
| `search_page_2_sends_page_number_2`, `has_more_computed_from_total`, `mode_maps_to_type_recherche`, `query_too_long_is_invalid_arg` | application/adapter | D-09, D-32 |
| `default_decision_response_under_budget`, `paged_reading_reconstructs_full_text`, `focus_returns_relevant_paragraphs`, `tool_descriptors_total_size_budget` | application / adapter-mcp | D-33 |
| `every_registered_tool_belongs_to_a_family`, `coverage_status_degraded_without_credentials`, `coverage_lists_known_gaps` | application | D-34 |
| `fixtures_have_no_mojibake`, `fixtures_have_meta_sidecar`, `fixture_sha256_matches_meta` | tests (workspace) | D-35 |
| `tool_descriptors_snapshot`, `server_instructions_snapshot`, `descriptor_change_requires_version_bump` | adapter-mcp | D-37 |
| `error_messages_never_contain_secret_or_userinfo`, `debug_of_config_redacts_secret`, `stdout_contains_only_jsonrpc` | transverse | D-40, D-46 |
| `readme_mentions_only_supported_env_vars`, `user_agent_contains_crate_version`, `instructions_reference_only_existing_tools_and_codes` | app / docs | D-42, D-43 |
| `juri_and_cnil_responses_carry_redaction_note` | application | D-45 |

Crates candidates (dernière version non retirée observée dans l'index sparse crates.io le 2026-10-15 ; dates de publication et API **non vérifiées**, docs.rs inaccessible) : `reqwest` 0.13.5, `tokio` 1.53.3, `rmcp` 3.5.1, `secrecy` 0.10.3, `thiserror` 2.0.21, `governor` 0.10.4, `moka` 0.12.16, `tracing` 0.1.44, `tracing-subscriber` 0.3.23, `tracing-appender` 0.2.5, `chrono` 0.4.45, `chrono-tz` 0.10.4, `jiff` 0.2.38, `wiremock` 0.6.5, `insta` 1.49.0, `proptest` 1.12.0, `serde_json` 1.0.154, `sha2` 0.11.1, `hmac` 0.13.0, `backon` 1.6.0, `html2text` 0.17.3, `scraper` 0.27.0, `redb` 4.4.0, `fs4` 1.1.0, `schemars` 1.2.3, `url` 2.5.8.

---

## 7. Questions ouvertes et points NON VÉRIFIÉS

1. **Origine du mojibake** des 6 fixtures v0.1.0 : capture (très probable) ou données source — à trancher par un appel live sur `JURITEXT000051743650`, `LEGIARTI000006419288`, `LEGITEXT000033205014`.
2. **`texte` sans sauts de ligne** : propriété de l'API (probable) ou de la capture ? Et `texteHtml` est-il présent pour `getArticle` (absent de `consult_article.json`) et `consult/juri` JURI (absent de `consult_juri.json`) ?
3. **Réponse de l'API pour un identifiant inconnu** : 404, 200 avec `text: null`, ou 500 ? Et forme d'un 404 de routage PISTE (pour distinguer D-10).
4. **Réponse à un `date` mal formé** sur `/consult/lawDecree` (400 vs 500) — détermine la politique de retry (D-12, D-22).
5. **`/consult/lawDecree` renvoie-t-il le contenu des articles** (fixture élaguée) et que renvoie-t-il pour un `LEGITEXT` de **code** (D-25, D-47) ? Taille maximale réaliste d'une TOC.
6. **Ordre `intOrdre` vs ordre des tableaux** dans les réponses LODA réelles (D-25).
7. **Quotas PISTE** effectifs de l'application (console PISTE) et réalité du « 401 sous throttling » rapporté par pylegifrance (D-12, D-13).
8. **Convention horaire des timestamps** : tous à minuit UTC ? (vrai pour les 27 cas des fixtures).
9. **Comportement Windows** de `datetime.fromtimestamp(<0, tz=UTC)` : déduit du code CPython 3.13 et de la doc Microsoft, non exécuté sur Windows.
10. **URL canoniques** : `/codes/article_lc/` pour un article de loi redirige-t-il ? `/ceta/id/` vs `/juri/id/` pour `CETATEXT` (la référence et pylegifrance divergent).
11. **Écart de contenu sandbox / production** et fraîcheur de la sandbox.
12. **Duplication TextContent + structuredContent** : quels clients MCP cibles exposent les deux au modèle ? Stratégie Rust (texte court + structuré, ou structuré seul) à décider.
13. **Comportement exact de FastMCP 3.4** (version minimale déclarée) sur l'enveloppe d'erreur et le contenu structuré — vérifié seulement sur `main` (e2fcc41).
14. **Syntaxe interne du moteur de recherche** (guillemets, jokers, opérateurs) dans `valeur` ; valeurs de tri et de facettes disponibles par fond.
15. **Obligation d'attribution Etalab** (source + date de mise à jour) applicable aux réponses d'un connecteur ?
16. **Atomicité des appends multi-processus** du JSONL sous Windows (D-16) — à éviter par conception plutôt qu'à vérifier.
17. Valeur par défaut de `SearchMode` côté Rust (`tous_les_mots` vs `un_des_mots`) : choix produit à valider avec l'utilisateur.
18. Politique art. 2 en Rust : refuser la réponse si l'audit échoue (fail-closed, conforme à la CONSTITUTION) — à confirmer explicitement avec l'utilisateur, car cela rend l'outil indisponible en cas de disque plein.

---

## 8. Sources

Fichiers de la référence (chemins sous `/home/user/matematicsolutions/fr-eli-mcp/`) : cités en ligne ci-dessus.

Dépôts publics clonés (lecture seule, sous `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/clones/`) :
- `fastmcp/fastmcp_slim/fastmcp/server/server.py:1465-1532`, `fastmcp_slim/fastmcp/settings.py:205-219`, `fastmcp_slim/fastmcp/tools/base.py:114-130, 395-430`, `fastmcp_slim/fastmcp/utilities/components.py:201`, `fastmcp_slim/fastmcp/utilities/exceptions.py:26-38` (https://github.com/PrefectHQ/fastmcp @ e2fcc41).
- `httpx/httpx/_models.py:794-832`, `httpx/httpx/_urls.py:374-382` (https://github.com/encode/httpx @ b5addb6).
- `diskcache/diskcache/core.py:51-60, 161-177, 221, 282-284`, `diskcache/README.rst:77`, `diskcache/docs/tutorial.rst:874-888` (https://github.com/grantjenks/python-diskcache @ ebfa37c).
- `cpython` 3.13 : `Python/pytime.c:1325-1349`, `Modules/_datetimemodule.c:5446-5448` (https://github.com/python/cpython).
- `mcp-spec` : `docs/specification/2025-11-25/server/tools.mdx:326, 464-477`, `docs/specification/2026-07-28/server/tools.mdx:500` (https://github.com/modelcontextprotocol/modelcontextprotocol).
- `pylegifrance` : `pylegifrance/models/generated/model.py:112-117, 1885-1899, 3504`, `pylegifrance/models/code/search.py:255-265`, `pylegifrance/fonds/juri.py:91-98`, `pylegifrance/fonds/code.py:491`, `pylegifrance/models/constants.py:359`, `tests/integration/conftest.py:1-30`, `tests/integration/fonds/juri/steps.py:35-41`, `docs/src/content/docs/en/concepts/api-terms-and-quotas.md`, `docs/src/content/docs/en/concepts/piste-oauth.md` (https://github.com/rdassignies/pylegifrance @ d8e0e59).
- `chrono/src/datetime/mod.rs:813-842` (https://github.com/chronotope/chrono @ 6adaa52).

Web :
- Microsoft, `_gmtime_s` (VS2008) : https://msdn.microsoft.com/de-de/library/3stkd9be(v=vs.90) ; doc actuelle : https://learn.microsoft.com/zh-cn/cpp/c-runtime-library/reference/gmtime-s-gmtime32-s-gmtime64-s?view=msvc-140
- Rapport de bogue CPython sur `fromtimestamp` négatif sous Windows (localtime) : https://bugs.python.org/msg338893 (page non chargée, DNS ; connue via les résultats de recherche) ; https://github.com/arrow-py/arrow/issues/668
- URL Légifrance d'articles de loi : https://www.legifrance.gouv.fr/loda/article_lc/LEGIARTI000006716311 ; https://www.legifrance.gouv.fr/loda/article_lc/LEGIARTI000006258433/2011-08-31
- Index sparse crates.io : https://index.crates.io/ (requêtes du 2026-10-15).
