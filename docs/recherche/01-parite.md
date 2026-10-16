# 01 — Spécification de parité exhaustive de la référence Python (`fr-eli-mcp`)

> Axe de recherche : **ce que la version Rust v0.1 doit reproduire** (check-list d'acceptation).
> Référence : `matematicsolutions/fr-eli-mcp`, commit `d3b5b13` (2026-09-24, unique commit du clone, message « test: lock tool descriptors against silent drift »), lu en lecture seule dans `/home/user/matematicsolutions/fr-eli-mcp`. Licence Apache-2.0.
> Tous les chemins `fichier:ligne` ci-dessous sont relatifs à la racine de ce dépôt, sauf mention contraire.
> Données d'appui produites pendant cette recherche (hors dépôt) :
> - `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/research/01-parite-golden.json` : sorties attendues (« golden ») de chaque normaliseur et de chaque outil sur les 16 fixtures, calculées par une **réimplémentation indépendante** (`scratchpad/scripts/golden.py`, qui n'importe pas le code de référence) puis contrôlées contre toutes les assertions de `tests/test_parse.py` (`scratchpad/scripts/check_golden.py` : toutes passent).
> - Les blocs « verbatim » de l'annexe A ont été extraits par `ast` (analyse syntaxique, aucune exécution) via `scratchpad/scripts/extract_consts.py`, donc copiés octet pour octet.

Conventions :
- **PARITÉ** = comportement à reproduire tel quel en v0.1.
- **ÉCART-REF** = défaut ou incohérence de la référence ; une décision est proposée (reproduire ou corriger avec déviation documentée). Liste consolidée au §14.
- **NON VÉRIFIÉ** = non confirmé par lecture de fichier ou source web ; la façon de vérifier est indiquée.

---

## 0. Résumé exécutif

- 8 outils MCP, tous `async`, tous annotés `READ_ONLY` (`readOnlyHint=true, idempotentHint=true, destructiveHint=false, openWorldHint=true`) : `fr_search`, `fr_get_act`, `fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_coverage`, `fr_get_company_agreement` (ordre d'enregistrement, `src/fr_eli_mcp/server.py:173-514`).
- Une seule source amont : l'API Légifrance `lf-engine-app` derrière PISTE, 7 endpoints `POST` JSON (`/search`, `/consult/lawDecree`, `/consult/getArticle`, `/consult/juri`, `/consult/cnil`, `/consult/kaliText`, `/consult/acco`), OAuth2 `client_credentials` (`src/fr_eli_mcp/client.py:61-208`).
- Contrat de citation sur chaque réponse : `eli_uri` (= `source_url`, URL Légifrance, jamais un `/eli/` fabriqué), `human_readable_citation`, `source_url` ; `ecli` natif pour la jurisprudence (`src/fr_eli_mcp/citations.py:1-24`).
- Erreurs : `ToolError` à 4 codes (`invalid_arg`, `not_found`, `upstream_error`, `config_error`), message `"[code] message"`, mais **sur le fil** FastMCP le préfixe en `"Error calling tool '<nom>': [code] message"` (vérifié dans le code de FastMCP 3.4.8, §2.4).
- Audit JSONL append-only dans `~/.matematic/audit/fr-eli-mcp.jsonl` (`src/fr_eli_mcp/audit.py`) ; cache disque `diskcache` (`src/fr_eli_mcp/cache.py`) ; manifeste runtime distant optionnel (`src/fr_eli_mcp/runtime.py`).
- Tests : verrou des descripteurs (sha256 canonique), dérive INSTRUCTIONS ↔ outils/codes, parsing hors ligne sur 16 fixtures, couverture, smoke live (exclu en CI).
- Une trentaine d'écarts entre ce que la référence promet (INSTRUCTIONS, CONSTITUTION) et ce qu'elle fait réellement (audit non écrit sur `invalid_arg`/`not_found`, `duration_ms=0` sur les erreurs, recherche vide ≠ `not_found`, date « today » en UTC, cache LODA/CODE jamais touché, etc.) : voir §14.

---

## 1. Périmètre lu et versions

| Élément | Valeur | Source |
|---|---|---|
| Version du paquet | `0.5.3` | `pyproject.toml:7`, `src/fr_eli_mcp/__init__.py:3`, `server.json:4,14`, `tests/tools.lock.json:2` |
| User-Agent HTTP (version **périmée**) | `fr-eli-mcp/0.3.0 (+https://github.com/matematicsolutions/fr-eli-mcp)` | `src/fr_eli_mcp/client.py:31` |
| Python requis | `>=3.11` (CI en 3.12, Docker en 3.11) | `pyproject.toml:11`, `.github/workflows/ci.yml:30`, `Dockerfile:3` |
| Dépendances | `fastmcp>=3.4,<4`, `httpx>=0.27,<1`, `pydantic>=2.6`, `diskcache>=5.6`, `anyio>=4.3` | `pyproject.toml:27-33` |
| Dev | `pytest>=8.0`, `pytest-asyncio>=0.23`, `mypy>=1.10`, `ruff>=0.5` ; `asyncio_mode="auto"`, `testpaths=["tests"]` | `pyproject.toml:35-41,79-81` |
| Entrées | script `fr-eli-mcp = "fr_eli_mcp.server:main"` ; `python -m fr_eli_mcp` (`__main__.py`, docstring en polonais sur Windows Smart App Control) | `pyproject.toml:48-49`, `src/fr_eli_mcp/__main__.py:1-15` |
| Fichiers source | `server.py` (523 l.), `citations.py` (486), `client.py` (208), `models.py` (190), `runtime.py` (92), `coverage.py` (77), `audit.py` (94), `cache.py` (57), `__init__.py`, `__main__.py` | `wc -l` |
| Tests | `conftest.py`, `test_coverage.py`, `test_descriptor_lock.py`, `test_instructions_drift.py`, `test_parse.py`, `test_smoke.py`, `tools.lock.json`, 16 fixtures | `tests/` |
| Absents bien que référencés | `CHANGELOG.md` (cité par `CONSTITUTION.md:63`), `NOTICE` | `ls` |

Bibliothèques tierces lues pour caractériser le comportement « sur le fil » (clones publics, lecture seule, sous `scratchpad/clones/`) :
- FastMCP `v3.4.8` (dernier tag 3.x ; des tags 4.x existent mais sont exclus par `<4`) — `PrefectHQ/fastmcp`.
- SDK MCP Python `v1.26.0` (version épinglée dans `uv.lock` de FastMCP 3.4.8, ligne 1662-1663) — `LATEST_PROTOCOL_VERSION = "2025-11-25"` (`src/mcp/types.py:27`).
- httpx `0.28.1`, diskcache `5.6.3`, registre MCP (`modelcontextprotocol/registry`, commit `11b7b0e` du 2026-10-11).
- **NON VÉRIFIÉ** : la version exacte de FastMCP/SDK ayant produit `tests/tools.lock.json`. Indice : la présence de `"meta": {"fastmcp": {"tags": []}}` dans chaque descripteur est cohérente avec FastMCP 3.x.

---

## 2. Surface MCP globale

### 2.1 Serveur

| Propriété | Valeur | Source |
|---|---|---|
| Construction | `FastMCP(name="fr-eli-mcp", instructions=INSTRUCTIONS)` | `src/fr_eli_mcp/server.py:88` |
| `serverInfo.name` | `fr-eli-mcp` | idem |
| `serverInfo.version` | **version de FastMCP**, pas celle du paquet (aucune `version=` passée → `fastmcp.__version__`) | FastMCP `server/server.py:434` (v3.4.8) — ÉCART-REF E-09 |
| `instructions` | texte intégral en annexe A.1 (4470 caractères, 4471 octets UTF-8 ; seul caractère non ASCII : `°` de « n° ») | `src/fr_eli_mcp/server.py:31-66` |
| Transport | stdio uniquement (`mcp.run()` sans argument) | `src/fr_eli_mcp/server.py:517-519` |
| Capacités annoncées | FastMCP enregistre les handlers `tools/list`, `resources/list`, `prompts/list`, `logging/setLevel` → le SDK annonce `tools`, `resources`, `prompts`, `logging` (resources/prompts vides) | FastMCP `server/mixins/mcp_operations.py:67-82` ; SDK `server/lowlevel/server.py` (`get_capabilities`) |
| Validation d'entrée JSON Schema côté SDK | désactivée (`strict_input_validation=False` par défaut) ; la validation est faite par pydantic en mode « lax » (ex. `"10"` → `10` pour un `int`) | FastMCP `settings.py:297-311`, `mixins/mcp_operations.py:77` |
| Masquage des erreurs | désactivé (`mask_error_details=False` par défaut) → le texte de l'exception est renvoyé au client | FastMCP `settings.py:268-281` |
| Ressources / prompts / completions | aucun | `server.py` (aucun `@mcp.resource` / `@mcp.prompt`) |

### 2.2 Annotations (constante `READ_ONLY`)

```python
READ_ONLY = ToolAnnotations(readOnlyHint=True, idempotentHint=True, destructiveHint=False, openWorldHint=True)
```
`src/fr_eli_mcp/server.py:81-86`. Appliquée aux 8 outils (`@mcp.tool(annotations=READ_ONLY)`). Pas de `title`. Dans le verrou : `"annotations": {"destructiveHint": false, "idempotentHint": true, "openWorldHint": true, "readOnlyHint": true}` pour chaque outil (`tests/tools.lock.json`).

### 2.3 Sérialisation des résultats (succès)

Chaque outil renvoie un modèle pydantic. FastMCP (v3.4.8) :
- `content` = un `TextContent` dont le texte est `pydantic_core.to_json(modèle, fallback=str, by_alias=True)` : JSON **compact** (sans espaces), UTF-8 non échappé, champs `null` **inclus**, ordre = ordre de déclaration des champs du modèle (FastMCP `tools/base.py:89-92`, `_convert_to_content`).
- `structuredContent` = `pydantic_core.to_jsonable_python(modèle)` (même contenu sous forme d'objet) (FastMCP `tools/base.py:312-368`).
- `outputSchema` présent dans `tools/list` pour chaque outil (schéma JSON du modèle de retour, `additionalProperties: true` car `extra="allow"`), sans `x-fastmcp-wrap-result` → `structuredContent` n'est pas enveloppé dans `{"result": …}`.
- Le SDK valide `structuredContent` contre `outputSchema` avant d'envoyer (SDK `server/lowlevel/server.py:559-569`).
- Les ordres de champs exacts de chaque modèle sont au §5.

### 2.4 Erreurs : format exact sur le fil

Classe (`src/fr_eli_mcp/server.py:69-78`) :
```python
class ToolError(Exception):
    VALID_CODES = frozenset({"invalid_arg", "not_found", "upstream_error", "config_error"})
    def __init__(self, code, message):
        if code not in self.VALID_CODES:
            raise ValueError(f"Unknown ToolError code: {code}. Valid: {sorted(self.VALID_CODES)}")
        self.code = code
        super().__init__(f"[{code}] {message}")
```
Chemin réel jusqu'au client (vérifié dans le code tiers) :
1. Cette `ToolError` n'hérite **pas** de `fastmcp.exceptions.ToolError` ; FastMCP l'attrape dans `except Exception as e` (FastMCP `server/server.py:1389`) et relève `fastmcp.exceptions.ToolError(f"Error calling tool {name!r}: {e}")` (`server/server.py:1405`, `mask_error_details=False`).
2. Le SDK attrape toute exception du handler et renvoie `CallToolResult(content=[TextContent(text=str(e))], isError=True)` sans `structuredContent` (SDK `server/lowlevel/server.py:467-474, 584`).

Donc le texte reçu par le modèle est, par exemple :
```
Error calling tool 'fr_search': [invalid_arg] fond='XYZ' must be one of ['ACCO', 'CETAT', 'CNIL', 'CODE', 'CONSTIT', 'JURI', 'KALI', 'LODA'].
```
(et non `[invalid_arg] …` en tête comme l'affirme INSTRUCTIONS « Tools return a structured error with a `[code]` prefix » — ÉCART-REF E-01).

Autres erreurs « hors ToolError » :
- **Arguments invalides au sens pydantic** (champ requis manquant, type non coercible, argument inconnu probable vu `additionalProperties: false`) : FastMCP convertit en `fastmcp.exceptions.ValidationError(str(pydantic_error))` (FastMCP `tools/function_tool.py:419-425`), relevée telle quelle (pas de préfixe « Error calling tool ») → `isError=true`, texte = message pydantic multi-ligne (ex. `1 validation error for call[fr_search]\nquery\n  Missing required argument …`). Format exact du texte pydantic : **NON VÉRIFIÉ** (dépend de la version de pydantic ; vérifiable en appelant l'outil sans `query` via `fastmcp.Client`). Le rejet d'un argument supplémentaire est **NON VÉRIFIÉ** (le schéma porte `additionalProperties: false`).
- **Outil inconnu** : `NotFoundError("Unknown tool: 'x'")` → `isError=true`, texte `Unknown tool: 'x'` (FastMCP `server/mixins/mcp_operations.py:243-246` ; SDK ligne 584).
- **Exception non mappée** (ex. JSON amont illisible, erreur disque d'audit) : `isError=true`, texte `Error calling tool '<nom>': <message de l'exception>` **sans code** (ÉCART-REF E-21).
- Cas spéciaux FastMCP `httpx` 429 / timeout (`server/server.py:1393-1401`) : **jamais atteints** car la référence enveloppe déjà ces exceptions dans sa propre `ToolError` (`raise _map_upstream(exc) from exc`).

### 2.5 Mapping amont → code (`_map_upstream`, `src/fr_eli_mcp/server.py:110-122`)

| Exception | Code | Message (après `[code] `) |
|---|---|---|
| `CredentialsError` | `config_error` | `str(exc)` (voir §6.2) |
| `httpx.HTTPStatusError` statut 404 | `not_found` | `Legifrance returned 404 for that resource.` |
| `httpx.HTTPStatusError` autre statut | `upstream_error` | `Legifrance/PISTE HTTP {status}: {exc}` où `str(exc)` httpx = `"{Client error\|Server error\|Redirect response\|Informational response\|Invalid status code} '{status} {reason}' for url '{url}'\nFor more information check: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/{status}"` (variante avec `Redirect location: '…'` pour un 3xx avec `Location`) — httpx `_models.py:794-829` |
| `httpx.TransportError` / `httpx.TimeoutException` | `upstream_error` | `Legifrance/PISTE error: {type(exc).__name__}: {exc}` (ex. `ReadTimeout: …`) |
| toute autre exception | (aucun) | l'exception est relevée telle quelle → §2.4 « non mappée » |

Remarque : une erreur OAuth (400/401 sur l'endpoint token) produit `upstream_error` (et non `config_error`), car c'est un `HTTPStatusError` (`client.py:87`).

### 2.6 Matrice des scénarios d'erreur (comportement exact de la référence)

| # | Scénario | Ligne d'audit ? | Texte MCP renvoyé (`isError=true`) |
|---|---|---|---|
| S1 | `fond` invalide | **non** | `Error calling tool 'fr_search': [invalid_arg] fond='XYZ' must be one of ['ACCO', 'CETAT', 'CNIL', 'CODE', 'CONSTIT', 'JURI', 'KALI', 'LODA'].` |
| S2 | `query` vide ou blanche | non | `… [invalid_arg] query is required.` |
| S3 | `page_size` hors 1..100 | non | `… [invalid_arg] page_size=0 out of range (1..100).` |
| S4 | identifiant vide / blanc | non | `… [invalid_arg] text_id is required.` (resp. `article_id`, `decision_id`, `deliberation_id`, `convention_id`, `agreement_id`) |
| S5 | mauvais préfixe | non | `… [invalid_arg] text_id='JURITEXT0001' must be a Legifrance LEGITEXT... identifier.` ; `fr_get_text` : `article_id='X' must be a Legifrance LEGIARTI... or KALIARTI... identifier.` ; `fr_get_decision` : `decision_id='X' must start with one of ('JURITEXT', 'CONSTEXT', 'CETATEXT').` |
| S6 | identifiants PISTE absents | **oui** (`status:"error"`, `error:"CredentialsError: PISTE credentials missing: …"`, `duration_ms:0`) | `… [config_error] PISTE credentials missing: set FR_ELI_CLIENT_ID and FR_ELI_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials).` |
| S7 | OAuth 200 sans `access_token` | oui | `… [config_error] PISTE OAuth returned no access_token.` |
| S8 | OAuth HTTP 4xx (ex. `invalid_client`) | oui | `… [upstream_error] Legifrance/PISTE HTTP 401: Client error '401 Unauthorized' for url 'https://sandbox-oauth.piste.gouv.fr/api/oauth/token'\nFor more information check: …/401` (raison exacte NON VÉRIFIÉE : dépend de la réponse PISTE) |
| S9 | API 404 | oui | `… [not_found] Legifrance returned 404 for that resource.` |
| S10 | API 400/403 | oui | `… [upstream_error] Legifrance/PISTE HTTP 400: Client error '400 Bad Request' for url '<base>/consult/juri'\n…` |
| S11 | API 429/500/502/503/504 trois fois | oui | `… [upstream_error] Legifrance/PISTE HTTP 503: Server error '503 Service Unavailable' …` |
| S12 | API 401 deux fois de suite | oui | `… [upstream_error] Legifrance/PISTE HTTP 401: …` |
| S13 | timeout / erreur réseau trois fois | oui | `… [upstream_error] Legifrance/PISTE error: ConnectTimeout: …` |
| S14 | 200 avec corps non JSON | oui (`error:"JSONDecodeError: …"`) | `Error calling tool 'fr_get_text': Expecting value: line 1 column 1 (char 0)` (sans code) |
| S15 | 200 avec charge vide / sans `id` | **non** | `… [not_found] No LODA text found for LEGITEXT….` (messages par outil au §3) |
| S16 | recherche sans résultat | oui (`status:"ok"`, `output_count_or_size:0`) | **succès** `{"total":0,"hits":[],…}` (pas de `not_found`, ÉCART-REF E-04) |
| S17 | répertoire d'audit non créable / non inscriptible | sans objet | `Error calling tool '<nom>': [Errno 13] Permission denied: '…'` (levé dès `_audit()` en tête d'outil, avant toute validation) |

Comment on obtient « oui/non » : les `ToolError("invalid_arg", …)` sont levées avant tout `audit.log` ; les `not_found` post-normalisation sont levées après la sortie du `with timer()` mais **avant** le `audit.log(... status="ok")` (ex. `src/fr_eli_mcp/server.py:256-262`) ; seules les exceptions de l'appel client sont auditées en erreur (ex. `server.py:207-211`).

---

## 3. Spécification outil par outil

Schéma commun à tous les outils « consult » (exemple `fr_get_text`, `src/fr_eli_mcp/server.py:281-310`) :
```
audit = AuditLogger()                     # mkdir du répertoire d'audit à CHAQUE appel
<validations> -> ToolError("invalid_arg", ...)   # non audité
input_hash = hash_input({<param>: <valeur brute>})
with timer() as t:
    try:
        async with LegifranceClient() as client:   # nouveau client httpx + cache par appel
            raw = await client.<endpoint>(...)
    except Exception as exc:
        audit.log(..., output_count_or_size=0, duration_ms=t.duration_ms or 0,   # toujours 0 (timer non sorti)
                  status="error", error=f"{type(exc).__name__}: {exc}")
        raise _map_upstream(exc) from exc
norm = citations.normalize_<x>(raw)
if norm is None: raise ToolError("not_found", "...")   # non audité
result = Modèle.model_validate(norm)
audit.log(..., output_count_or_size=<métrique>, duration_ms=t.duration_ms, status="ok")
return result
```
`duration_ms` du succès = durée de l'appel HTTP (cache compris), **pas** de la normalisation.

### 3.1 `fr_search` (`src/fr_eli_mcp/server.py:173-222`)

**Signature** : `fr_search(query: str, fond: str = "LODA", page_size: int = 10) -> SearchResult`.
**Descripteur** : description `Keyword-search French legislation or case law on Legifrance.` ; descriptions de paramètres exactes en annexe A ; `inputSchema` : `query` `string` requis ; `fond` `string` défaut `"LODA"` (pas d'`enum`) ; `page_size` `integer` défaut `10` (pas de `minimum`/`maximum`) ; `additionalProperties: false`. sha256 du descripteur : `81727319c2603cf8aadc1bb8189c1c9429846d9be37f5f4c960c4f37993dabd1`.

**Validations, dans cet ordre** (`server.py:191-197`) :
1. `fond_key = (fond or "").strip().upper()` — insensible à la casse et aux espaces (`" loda "` accepté).
2. `fond_key ∉ _FOND_MAP` → `invalid_arg` `fond={fond!r} must be one of {sorted(_FOND_MAP)}.` (`repr` Python de la valeur **d'origine** ; liste triée : `['ACCO', 'CETAT', 'CNIL', 'CODE', 'CONSTIT', 'JURI', 'KALI', 'LODA']`).
3. `not query or not query.strip()` → `invalid_arg` `query is required.`
4. `not 1 <= page_size <= 100` → `invalid_arg` `page_size={page_size} out of range (1..100).`

**Mapping des fonds** (`server.py:91-100`) :

| fond utilisateur | fond API | `kind` des hits | Filtre ajouté |
|---|---|---|---|
| `LODA` | `LODA_DATE` | `loda` | `DATE_VERSION` |
| `CODE` | `CODE_DATE` | `code` | `DATE_VERSION` |
| `JURI` | `JURI` | `juri` | — |
| `CONSTIT` | `CONSTIT` | `constit` | — |
| `CETAT` | `CETAT` | `cetat` | — |
| `CNIL` | `CNIL` | `cnil` | — |
| `KALI` | `KALI` | `kali` | — |
| `ACCO` | `ACCO` | `acco` | — |

**Hash d'entrée** : `hash_input({"query": query, "fond": fond_key, "page_size": page_size})` — `query` **brute** (non strippée), `fond` normalisé (`server.py:201`).

**Requête** : `POST {base}/search`, catégorie de cache `search` (TTL 1 h), corps construit par `_build_search_payload(query.strip(), api_fond, page_size)` (`server.py:133-156`) ; ordre des clés sur le fil = ordre d'insertion :
```json
{
  "fond": "LODA_DATE",
  "recherche": {
    "champs": [
      {
        "typeChamp": "ALL",
        "criteres": [{"typeRecherche": "UN_DES_MOTS", "valeur": "republique numerique", "operateur": "ET"}],
        "operateur": "ET"
      }
    ],
    "filtres": [{"facette": "DATE_VERSION", "singleDate": 1760540000000}],
    "pageNumber": 1,
    "pageSize": 5,
    "operateur": "ET",
    "sort": "PERTINENCE",
    "typePagination": "DEFAUT"
  }
}
```
- `singleDate` = `int(time.time() * 1000)` à l'instant de l'appel (heure comprise, pas minuit) (`server.py:129-130`). Pour les fonds autres que `LODA_DATE`/`CODE_DATE`, `"filtres": []`.
- `pageNumber` toujours `1` : **aucune pagination** possible.
- httpx sérialise le corps en JSON compact `separators=(",", ":")`, `ensure_ascii=False` (httpx `_content.py:176-183`).
- Le besoin d'un filtre `DATE_VERSION` pour `LODA_DATE`/`CODE_DATE` est documenté comme « require » (`DISCOVERY.md:22-26`).

**Normalisation** : `citations.normalize_search(raw, api_fond)` (§4.6). **Sortie** `SearchResult` :

| Champ | Type | Origine | Transformation |
|---|---|---|---|
| `fond` | string | entrée | `fond_key` (ex. `LODA`, pas `LODA_DATE`) |
| `query` | string | entrée | `query.strip()` |
| `total` | integer | `totalResultNumber` | si non entier → `len(hits)` |
| `hits` | array<SearchHit> | `results[]` | §4.6 |
| `eli_note` | string | constante `ELI_NOTE` | — |
| `dataset_note` | string | constante `DATASET_NOTE` | — |

**Audit** : `output_count_or_size = len(result.hits)` (nombre de hits de la page, pas `total`).
**Résultat vide** : succès avec `total` (0 si `totalResultNumber` = 0) et `hits: []` — jamais `not_found`.

### 3.2 `fr_get_act` (`server.py:230-262`)

**Signature** : `fr_get_act(text_id: str, date: str | None = None) -> Act`. Description `Consult a LODA law or decree by its ``LEGITEXT...`` id.` ; sha256 `3dfbd11fd5027ac5180773c51dd58c1b4bf599871b31e40d0fa0b9efde4b89e2` ; `date` : `anyOf [string, null]`, défaut `null`.
**Validations** : `_require_prefix(text_id, "LEGITEXT", "text_id")` (`server.py:159-165`) :
- `not value or not value.strip()` → `text_id is required.`
- `not value.startswith("LEGITEXT")` → `text_id={value!r} must be a Legifrance LEGITEXT... identifier.`
- Aucune validation du reste de l'identifiant ; **pas de strip** (un espace final est envoyé à l'API) ; un id suffixé `_08-09-2023` passe et est envoyé tel quel.
**Date** : `version_date = (date or _today()).strip()` ; `_today()` = `time.strftime("%Y-%m-%d", time.gmtime())` → **date UTC** (`server.py:125-126`). Aucune validation de format ; `date="   "` → envoie `""`.
**Hash** : `{"text_id": text_id, "date": version_date}`.
**Requête** : `POST {base}/consult/lawDecree` corps `{"textId": text_id, "date": version_date}`, catégorie `act` (TTL 24 h) (`client.py:190-193`).
**Normalisation** : `normalize_law_decree` (§4.7). `None` → `not_found` `No LODA text found for {text_id}.`
**Sortie** : `Act` (§5). **Audit** : `len(result.articles)`.

### 3.3 `fr_get_text` (`server.py:270-310`)

**Signature** : `fr_get_text(article_id: str) -> ArticleText`. Description `Fetch the verbatim text of a single article (``LEGIARTI...`` or ``KALIARTI...``).` ; sha256 `c7a0d883c96b756cacb9c80845af28f0e5711dc6b5f127d3cca309a8176202c8`.
**Validations** : vide/blanc → `article_id is required.` ; `not article_id.startswith(("LEGIARTI", "KALIARTI"))` → `article_id={article_id!r} must be a Legifrance LEGIARTI... or KALIARTI... identifier.`
**Hash** : `{"article_id": article_id}`.
**Requête** : `POST {base}/consult/getArticle` corps `{"id": article_id}`, catégorie `act` (`client.py:195-196`). Pas de paramètre de date (chaque `LEGIARTI` désigne une version).
**Normalisation** : `normalize_article` (§4.8) ; `None` → `not_found` `No article found for {article_id}.` ; puis `byte_size = len((text or "").encode("utf-8"))`.
**Audit** : `result.byte_size or 0`.

### 3.4 `fr_get_decision` (`server.py:318-363`)

**Signature** : `fr_get_decision(decision_id: str) -> Decision`. Description (multi-ligne, annexe A) ; sha256 `10c07370a00d5c306e9297e9a91a32a8ced6ea90a5835cbed71e6807e7ab3f0e`.
**Validations** : vide → `decision_id is required.` ; `not decision_id.startswith(("JURITEXT", "CONSTEXT", "CETATEXT"))` → `decision_id={decision_id!r} must start with one of ('JURITEXT', 'CONSTEXT', 'CETATEXT').` (repr de tuple Python, `server.py:103, 338-342`).
**Hash** : `{"decision_id": decision_id}`.
**Requête** : `POST {base}/consult/juri` corps `{"textId": decision_id}`, catégorie `act` — même endpoint pour les 3 juridictions (`client.py:198-199` ; validé en live selon `DISCOVERY.md:78-82`).
**Normalisation** : `normalize_juri` (§4.9) ; `None` → `not_found` `No JURI decision found for {decision_id}.` (même pour CONSTIT/CETAT) ; `byte_size` idem 3.3.
**Audit** : `result.byte_size or 0`.

### 3.5 `fr_get_deliberation` (`server.py:371-405`)

`fr_get_deliberation(deliberation_id: str) -> Deliberation` ; description `Consult a CNIL deliberation by its ``CNILTEXT...`` id.` ; sha256 `3a3a47fedaa9076ce4eb930925d9750bc237e67d8f7a0fb0e75db1b698218de9`.
Validation `_require_prefix(deliberation_id, "CNILTEXT", "deliberation_id")`. Hash `{"deliberation_id": …}`. `POST {base}/consult/cnil` corps `{"textId": id}`, catégorie `act` (`client.py:201-202`). `normalize_cnil` (§4.10) ; `None` → `No CNIL deliberation found for {deliberation_id}.` ; `byte_size`. Audit `byte_size or 0`.

### 3.6 `fr_get_convention` (`server.py:413-445`)

`fr_get_convention(convention_id: str) -> Act` (même modèle que `fr_get_act`) ; description `Consult a collective labour agreement text by its ``KALITEXT...`` id.` ; sha256 `d3193636fe8efda9632c3f1cd7598ef292790530fc3ff4adc96363fd977d783a`.
Validation `_require_prefix(convention_id, "KALITEXT", "convention_id")`. Hash `{"convention_id": …}`. `POST {base}/consult/kaliText` corps `{"id": id}` (clé `id`, pas `textId`), catégorie `act` (`client.py:204-205`). `normalize_kali` (§4.11) ; `None` → `No collective agreement text found for {convention_id}.` Audit `len(result.articles)`.

### 3.7 `fr_get_company_agreement` (`server.py:480-514`)

`fr_get_company_agreement(agreement_id: str) -> CompanyAgreement` ; description multi-ligne (annexe A) ; sha256 `17f0a0eecba40ea7d5d69aa804d0602ca12e2efdd4555f39ff3d8f9a181bfa6c`.
Validation `_require_prefix(agreement_id, "ACCOTEXT", "agreement_id")`. Hash `{"agreement_id": …}`. `POST {base}/consult/acco` corps `{"id": id}`, catégorie `act` (`client.py:207-208`). `normalize_acco` (§4.12, **métadonnées seulement**, le `.docx` base64 du champ `data` n'est jamais décodé ni renvoyé). `None` → `No company agreement found for {agreement_id}.` Audit **`1`** (constante).

### 3.8 `fr_coverage` (`server.py:450-474`)

`fr_coverage() -> Coverage`, aucun paramètre (`inputSchema = {"additionalProperties": false, "properties": {}, "type": "object"}`) ; sha256 `c814149c98c5dbb81f24e003326ac36736f6cdf4b5331cac39a6041cf7239452`.
Description = **docstring complète y compris la section `Returns:`** (FastMCP ne garde que la première section texte quand il trouve des paramètres documentés ; sans paramètre, il garde la docstring entière — FastMCP `utilities/docstring_parsing.py:35-66`). Texte exact en annexe A.
Hash `hash_input({})` = sha256(`{}`) = `44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a`. Aucun appel réseau. `duration_ms` = durée de `build_coverage()`. Audit `len(coverage.known_gaps)` (= 2). Sortie exacte en annexe A.
Remarque : l'en-tête de commentaire « fr_get_company_agreement (fond ACCO, feature-003) » (`server.py:448-449`) précède en réalité `fr_coverage` ; d'où l'ordre d'enregistrement `… fr_get_convention, fr_coverage, fr_get_company_agreement`.

### 3.9 Ordre d'enregistrement et de listage

Ordre de déclaration : `fr_search`, `fr_get_act`, `fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_coverage`, `fr_get_company_agreement`. Ordre exact renvoyé par `tools/list` : **NON VÉRIFIÉ** (probablement l'ordre d'enregistrement ; vérifiable avec `fastmcp.Client.list_tools()`). Le verrou trie par nom ; un client ne doit pas dépendre de l'ordre.

---

## 4. Règles de normalisation (`src/fr_eli_mcp/citations.py`)

### 4.1 Constantes

- `WEB_BASE = "https://www.legifrance.gouv.fr"` (`citations.py:32`).
- `_ID_PREFIXES = ("LEGITEXT", "JORFTEXT", "JURITEXT", "CETATEXT", "KALITEXT", "KALIARTI", "LEGIARTI", "CNILTEXT", "ACCOTEXT")` (`citations.py:34-44`) — **`CONSTEXT` n'y figure pas** (sans effet en pratique : les ids CONSTIT n'ont pas de suffixe dans les fixtures).
- `_MARK_RE = re.compile(r"</?mark>")` (`citations.py:45`).
- `_FR_MONTHS = ("janvier", "fevrier", "mars", "avril", "mai", "juin", "juillet", "aout", "septembre", "octobre", "novembre", "decembre")` — **sans accents** (`citations.py:283-286`).
- Format des identifiants observé dans les 16 fixtures : préfixe de 8 lettres + **12 chiffres** pour tous les types rencontrés (`ACCOTEXT`, `CETATEXT`, `CNILTEXT`, `CONSTEXT`, `JORFARTI`, `JORFTEXT`, `JURITEXT`, `KALIARTI`, `KALISCTA`, `KALITEXT`, `LEGIARTI`, `LEGISCTA`, `LEGITEXT`) ; suffixe de version facultatif `_JJ-MM-AAAA` vu sur `LEGITEXT` (`search_loda.json`, `search_code.json`, `consult_loda.json`). Généralité hors fixtures : NON VÉRIFIÉ.

### 4.2 `strip_marks(text)` (`citations.py:48-52`)

`None` → `None` ; sinon supprime toutes les occurrences exactes de `<mark>` et `</mark>` (minuscules, sans attribut), puis `.strip()`, puis chaîne vide → `None`.
Pièges Rust :
- `str.strip()` Python retire les caractères `str.isspace()` : espaces Unicode (dont U+00A0, U+2000-U+200A, U+3000…) **et** U+001C-U+001F ; `char::is_whitespace` de Rust n'inclut pas U+001C-U+001F (différence négligeable, à documenter).
- Les espaces internes (ex. double espace « libertés  - Non conformité » dans `search_constit.json`) sont conservés.
- Les autres balises HTML ne sont pas traitées.
Test : `strip_marks("a <mark>b</mark> c") == "a b c"` (`tests/test_parse.py:133`).

### 4.3 `strip_date_suffix(identifier)` (`citations.py:55-63`)

Falsy (`None`, `""`) → renvoyé tel quel. Si `"_"` présent : `head = identifier.split("_", 1)[0]` ; si `head` commence par un des `_ID_PREFIXES` → `head`, sinon identifiant inchangé.
Tests : `"LEGITEXT000033205014_08-09-2023"` → `"LEGITEXT000033205014"` ; `"JURITEXT000051743650"` inchangé (`tests/test_parse.py:131-132`).

### 4.4 Dates

`ms_to_date(ms)` (`citations.py:66-73`) : si `not isinstance(ms, int)` → `None` (donc : `None`, chaînes et **flottants** rejetés ; `bool` accepté car sous-type d'`int` en Python : `True` → `1970-01-01`) ; sinon `datetime.fromtimestamp(ms / 1000, tz=UTC).strftime("%Y-%m-%d")` ; `OverflowError/OSError/ValueError` → `None`.
- **UTC** (pas Europe/Paris). Les horodatages Légifrance des fixtures sont à minuit UTC (ex. `1475884800000` → `2016-10-08`).
- Valeur sentinelle `32472144000000` → `"2999-01-01"` (fin de validité « infinie » ; aussi `dateParution` de KALI) — reproduite telle quelle.
- Négatifs : `-5231606400000` (date de début du Code civil dans `consult_article.json` `textTitles[0].dateDebut`) fonctionne sous Linux ; sous Windows `fromtimestamp` négatif lève `OSError` → `None` (comportement dépendant de la plateforme, ÉCART-REF E-35). Rust : calcul calendaire pur (ex. `time`/`jiff`), même résultat partout.
- Année > 9999 → `None`.

`_fr_date(ms)` (`citations.py:289-295`) : `ms_to_date` puis `f"{int(day)} {_FR_MONTHS[int(month) - 1]} {year}"` → jour sans zéro initial, mois sans accent, **« 1 » et non « 1er »** (ex. `1762473600000` → `7 novembre 2025`).

### 4.5 `legifrance_url(identifier, kind)` (`citations.py:76-102`)

Identifiant falsy → `None`. Table exhaustive (`kind` inconnu → `None`) :

| kind | URL |
|---|---|
| `loda` | `https://www.legifrance.gouv.fr/loda/id/{id}` |
| `code` | `https://www.legifrance.gouv.fr/codes/texte_lc/{id}` |
| `code_article` | `https://www.legifrance.gouv.fr/codes/article_lc/{id}` |
| `juri` | `https://www.legifrance.gouv.fr/juri/id/{id}` |
| `constit` | `https://www.legifrance.gouv.fr/cons/id/{id}` |
| `cetat` | `https://www.legifrance.gouv.fr/ceta/id/{id}` |
| `jorf` | `https://www.legifrance.gouv.fr/jorf/id/{id}` (défini, **jamais utilisé**) |
| `cnil` | `https://www.legifrance.gouv.fr/cnil/id/{id}` |
| `kali` | `https://www.legifrance.gouv.fr/conv_coll/id/{id}` |
| `acco` | `https://www.legifrance.gouv.fr/acco/id/{id}` |

Vérifications externes : `/cons/id/CONSTEXT…` et `/ceta/id/CETATEXT…` confirmés par recherche web par l'auteur (`DISCOVERY.md:83-88`). Le motif `/loda/article_lc/LEGIARTI…` (éventuellement suivi de `/AAAA-MM-JJ`) existe sur Légifrance (résultats WebSearch, ex. https://www.legifrance.gouv.fr/loda/article_lc/LEGIARTI000006716311) alors que la référence utilise `/codes/article_lc/` **pour tout `LEGIARTI`**, y compris les articles de lois LODA ; que `/codes/article_lc/{LEGIARTI d'une loi}` résolve est **NON VÉRIFIÉ** (le site renvoie 403 aux robots selon `DISCOVERY.md:83-85` ; à vérifier dans un navigateur). `/conv_coll/id/KALIARTI…` (URL d'article KALI) : **NON VÉRIFIÉ** (aucune occurrence trouvée par WebSearch).

`eli_uri` vaut **toujours** `source_url` (même valeur, y compris `None`) pour toutes les entités.

### 4.6 Hits de recherche : `normalize_search_hit(result, fond)` / `normalize_search(payload, fond)` (`citations.py:110-182`)

`fond` reçu = **fond API** (`LODA_DATE`, `CODE_DATE`, `JURI`, …).
1. `titles = result.get("titles") or [{}]` ; `head = titles[0] if titles else {}` (un `titles[0]` à `None` ferait planter la référence : `AttributeError` ; Rust : tolérer).
2. `id = strip_date_suffix(head.id)` ; `cid = head.cid` (brut) ; `title = strip_marks(head.title)`.
3. Champs de base : `id, cid, title, nature = result.nature, etat = result.etat, human_readable_citation = title`.
4. Branches, testées **dans cet ordre** :

| Condition | `kind` | URL | Champs ajoutés / citation |
|---|---|---|---|
| `fond == "CONSTIT"` | `constit` | `constit` sur `cid or id` | — |
| `fond == "CNIL"` | `cnil` | `cnil` sur `cid or id` | — |
| `fond == "KALI"` | `kali` | `kali` sur `cid or id` | `idcc = result.idcc` |
| `fond == "ACCO"` | `acco` | `acco` sur `cid or id` | `idcc = result.idcc`, `raison_sociale = result.raisonSociale` ; si `title` et `raisonSociale` truthy : citation = `f"{title} ({raisonSociale})"` |
| `fond == "CETAT"` | `cetat` | `cetat` sur `cid or id` | — |
| `fond.startswith("JURI")` | `juri` | `juri` sur `cid or id` | — |
| `fond.startswith("CODE")` | `code` | `code` sur **`id`** (LEGITEXT du code) | `articles` (voir ci-dessous) |
| sinon (LODA_DATE…) | `loda` | `loda` sur **`id`** (LEGITEXT, **pas** le `cid` JORFTEXT) | — |

5. `source_url = eli_uri = url`.
6. `articles` (CODE uniquement) : pour chaque `sec` de `result.sections` (un seul niveau, pas de récursion), pour chaque `ex` de `sec.extracts` ayant un `id` truthy : `{"article_id": ex.id, "num": ex.num, "snippet": (strip_marks((ex.values or [""])[0]) or "")[:280]}` — troncature à **280 points de code** (pas d'octets), sans ellipse ajoutée. Les extraits KALI (`KALIARTI` présents dans `search_kali.json`) sont **ignorés**.
7. `normalize_search` : `hits = [normalize_search_hit(r, fond) for r in payload.results or []]` ; `total = payload.totalResultNumber` si `int`, sinon `len(hits)`. `facets`, `totalArticleResultNumber`, `typePagination`, `executionTime` sont ignorés.
8. Après `SearchHit.model_validate` : les clés absentes prennent les valeurs par défaut du modèle → `articles: []`, `idcc: null`, `raison_sociale: null` pour tous les fonds qui ne les renseignent pas.

### 4.7 Loi/décret LODA : `normalize_law_decree(payload)` (`citations.py:190-228`)

- `None` si `payload` vide ou ni `id` ni `cid`.
- `text_id = strip_date_suffix(payload.id)` (si `id` absent mais `cid` présent → `text_id = None` et `source_url = None`).
- Table des matières `_walk_articles(sections)` (`citations.py:190-201`) : parcours **préfixe** — pour chaque section : d'abord ses `articles[]` (ceux qui ont un `id`) → `{"article_id": id, "num": num, "etat": etat}`, puis récursion sur `sections[]` de la section. Les `articles` de premier niveau du payload ne sont **pas** lus pour LODA.
- Champs : `text_id`, `cid` (JORFTEXT), `nor`, `title` (brut, pas de `strip_marks`), `nature`, **`etat = payload.etat`** (null dans la fixture ; `jurisState: "Vigueur"` ignoré — ÉCART-REF E-10), `date_parution = ms_to_date(dateParution)`, `num_parution = numParution`, `date_debut_version = dateDebutVersion` et `date_fin_version = dateFinVersion` (chaînes brutes `AAAA-MM-JJ`), `articles`, `human_readable_citation = title`, `source_url = eli_uri = /loda/id/{text_id}`.

### 4.8 Article : `normalize_article(payload)` (`citations.py:236-276`)

- `article = payload.article` ; `None` si absent ou sans `id`.
- `_code_title` : `textTitles` doit être une liste ; entrée préférée = première dont `nature == "CODE"`, sinon `textTitles[0]` ; renvoie `(entry.titre, entry.id)`.
- Citation : `f"{code_title}, art. {num}"` si les deux ; sinon `f"Art. {num}"` si `num` ; sinon `code_title` (éventuellement `None`). Pour un article de loi LODA, `code_title` = titre de la loi (`textTitles[0]`), donc `"LOI n° …, art. 1"`.
- URL : `kali` si `str(id).startswith("KALIARTI")`, sinon `code_article`.
- Champs : `article_id`, `num`, `etat`, `code_title`, `full_sections_titre = fullSectionsTitre`, `date_debut = ms_to_date(dateDebut)`, `date_fin = ms_to_date(dateFin)`, `text = texte` (texte brut ; `texteHtml` ignoré), citation, URL.

### 4.9 Décision JURI / CONSTIT / CETAT : `normalize_juri(payload)` (`citations.py:321-355`)

- `text = payload.text` ; `None` si absent ou sans `id`.
- `origine = (text.origine or "").upper()` → URL `constit` si `CONSTIT`, `cetat` si `CETAT`, sinon `juri`. **Le choix dépend du champ `origine`, pas du préfixe de l'id.**
- Citation : `titre` brut ; si `origine == "CONSTIT"` : `_constit_citation(text) or titre`.
- `_constit_citation` (`citations.py:298-318`) : `num = text.num`, `nature = (text.nature or "").upper()`, `date_prose = _fr_date(text.dateTexte)` ; `None` si `num` ou `date_prose` manquant ; sinon `"Cons. const., decision n° {num}"` + (`" {nature}"` si non vide) + `" du {date_prose}"` + (`" - {g1}"` si `re.search(r"\[(.+?)\]", titre)` trouve un groupe : **premier** crochet, non gourmand, au moins un caractère, `.` n'englobe pas `\n`). Le nom des parties hors crochets (« M. Amine G. ») est perdu. Mot **« decision » sans accent**.
- Champs : `decision_id`, `ecli` (brut, jamais fabriqué ; souvent `null` pour CAA/TA), `juridiction`, `formation`, `solution`, `numero_affaire = numeroAffaire` (liste brute, `[]` possible), `date_texte = ms_to_date(dateTexte)`, `nature`, `title = titre`, `text = texte`, citation, URL.
- Ignorés (utiles pour les fonctionnalités futures « liens entre décisions ») : `sommaire`, `titrages`, `liens[]` (`typeLien`, `sens`, `cidTexte`…), `citationJp`, `decisionAttaquee`, `texteHtml`… (présents dans `consult_juri.json`).

### 4.10 Délibération CNIL : `normalize_cnil(payload)` (`citations.py:363-395`)

- `text = payload.text` ; `None` si absent ou sans `id`.
- Citation : `f"CNIL, deliberation n° {num} du {_fr_date(dateTexte)}"` si `num` et date ; sinon `titre`. Mot **« deliberation » sans accent**.
- Champs : `deliberation_id`, `num`, `nature`, `nature_delib = natureDelib`, `etat`, `date_texte = ms_to_date(dateTexte)`, `date_publication = ms_to_date(datePubli)`, `title = titre`, `title_long = titreLong`, `text = texte`, citation, URL `/cnil/id/{id}`.

### 4.11 Convention KALI : `normalize_kali(payload)` (`citations.py:403-436`)

Comme LODA sauf : `etat = payload.jurisState or payload.etat` ; TOC = `_walk_articles(sections)` **puis** les `articles[]` de premier niveau (avec `id`) ; URL `kali` (`/conv_coll/id/{text_id}`).

### 4.12 Accord d'entreprise ACCO : `normalize_acco(payload)` (`citations.py:444-486`)

- `acco = payload.acco` ; `None` si absent ou sans `id`.
- Citation : `f"{titreTexte} ({raisonSociale})"` si les deux, sinon `titreTexte`.
- `themes` = `[t.libelle for t in acco.themes if isinstance(t, dict)]` puis filtrage des valeurs falsy ; `syndicats` idem sur `acco.syndicats`.
- Champs : `agreement_id`, `nature`, `title = titreTexte`, `raison_sociale = raisonSociale`, `siret`, `idcc = codeIdcc`, `code_ape = codeApe`, `secteur`, `themes`, `syndicats`, `date_texte`, `date_effet`, `date_depot` (`ms_to_date` de `dateTexte`, `dateEffet`, `dateDepot`), `attachment_note` (constante, annexe A), citation, URL `/acco/id/{id}`. **Aucune clé `text`** (assertion `"text" not in norm`, `tests/test_parse.py:222`).
- Ignorés : `data` (base64 .docx, absent de la fixture), `attachementUrl`, `fileSize`, `numero`, `dateMaj`, `dateFin`, `dateDiffusion`, `signataires`, `adressesPostales`, `conformeVersionIntegrale`…

### 4.13 Sémantique Python à émuler en Rust (pièges)

- **Vérité Python** : `x or y`, `if a and b`, `if not value` traitent `None`, `""`, `0`, `[]` comme faux. Concerne : `cid or text_id`, `jurisState or etat`, `title and raisonSociale`, `num and date_prose`, `code_title and num`, filtrage des thèmes, `(ex.values or [""])`, `strip_marks(...) or None`. Prévoir un utilitaire `non_empty(Option<&str>)`.
- **Tranches** `[:280]` et `error[:500]` : en points de code (pas en octets, pas en graphèmes).
- `isinstance(ms, int)` : refuse les flottants JSON (`1.7e12`), accepte `true/false`.
- `str.upper()` Unicode (sans effet utile ici : seules des clés ASCII passent).
- `repr()` Python dans les messages d'erreur : guillemets simples par défaut ; guillemets doubles si la chaîne contient `'` et pas `"` ; échappement de `\\`, `\n`, `\r`, `\t`, et des non imprimables en `\xNN`/`\uNNNN`/`\UNNNNNNNN` ; caractères non ASCII imprimables conservés. Un utilitaire `py_repr` est nécessaire pour une parité stricte des messages (ou accepter l'écart E-27).
- Robustesse : la référence plante (`AttributeError`) si un élément attendu objet est `null` ou d'un autre type (`titles[0]`, `sections[i]`, `extracts[j]`, `article`, `text`…). Rust doit être tolérant (navigation `Option`, DTO `#[serde(default)]`), ce qui est une amélioration sans changement de résultat sur les entrées valides.
- **Typage pydantic** : `num_parution`, `num`, etc. sont `str | None` ; si l'API renvoie un nombre, `model_validate` lève une erreur de validation dans le corps de l'outil (renvoyée comme erreur non mappée). `idcc` accepte `str | int | None` (type conservé dans la sortie). `numero_affaire` est `list[str] | None`. Rust : DTO tolérants (`string_or_number`) en préservant le type de sortie.

---

## 5. Modèles de sortie (ordre de sérialisation exact)

Tous héritent de `_Tolerant` (`extra="allow"`, `populate_by_name=True`, `src/fr_eli_mcp/models.py:20-21`) ; tous les champs « simples » sont `str | None = None` sauf mention. Les normaliseurs ne produisent aucune clé hors modèle, donc aucun champ « extra » n'apparaît.

| Modèle (outil) | Champs dans l'ordre | Source |
|---|---|---|
| `SearchResult` (`fr_search`) | `fond: str` (requis), `query: str` (requis), `total: int` (requis), `hits: list[SearchHit] = []`, `eli_note: str = ELI_NOTE`, `dataset_note: str = DATASET_NOTE` | `models.py:56-64` |
| `SearchHit` | `kind`, `id`, `cid`, `title`, `nature`, `etat`, `idcc: str\|int\|None`, `raison_sociale`, `articles: list[CodeArticleRef] = []`, `eli_uri`, `human_readable_citation`, `source_url` | `models.py:37-53` |
| `CodeArticleRef` | `article_id`, `num`, `snippet` | `models.py:29-34` |
| `Act` (`fr_get_act`, `fr_get_convention`) | `text_id`, `cid`, `nor`, `title`, `nature`, `etat`, `date_parution`, `num_parution`, `date_debut_version`, `date_fin_version`, `articles: list[ArticleRef] = []`, `eli_uri`, `human_readable_citation`, `source_url`, `eli_note: str = ELI_NOTE` | `models.py:80-98` |
| `ArticleRef` | `article_id`, `num`, `etat` | `models.py:72-77` |
| `ArticleText` (`fr_get_text`) | `article_id`, `num`, `etat`, `code_title`, `full_sections_titre`, `date_debut`, `date_fin`, `text`, `byte_size: int\|None`, `eli_uri`, `human_readable_citation`, `source_url`, `eli_note: str = ELI_NOTE` | `models.py:106-122` |
| `Deliberation` (`fr_get_deliberation`) | `deliberation_id`, `num`, `nature`, `nature_delib`, `etat`, `date_texte`, `date_publication`, `title`, `title_long`, `text`, `byte_size: int\|None`, `eli_uri`, `human_readable_citation`, `source_url` — **pas d'`eli_note`** | `models.py:130-147` |
| `CompanyAgreement` (`fr_get_company_agreement`) | `agreement_id`, `nature`, `title`, `raison_sociale`, `siret`, `idcc: str\|int\|None`, `code_ape`, `secteur`, `themes: list[str] = []`, `syndicats: list[str] = []`, `date_texte`, `date_effet`, `date_depot`, `attachment_note`, `eli_uri`, `human_readable_citation`, `source_url` — pas d'`eli_note` | `models.py:150-170` |
| `Decision` (`fr_get_decision`) | `decision_id`, `ecli`, `juridiction`, `formation`, `solution`, `numero_affaire: list[str]\|None`, `date_texte`, `nature`, `title`, `text`, `byte_size: int\|None`, `eli_uri`, `human_readable_citation`, `source_url` — pas d'`eli_note` | `models.py:173-190` |
| `Coverage` (`fr_coverage`) | `status: Literal["ok","degraded","failed"] = "ok"`, `as_of_note: str` (requis), `families: list[CoverageFamily] = []`, `known_gaps: list[CoverageGap] = []` | `coverage.py:45-54` |
| `CoverageFamily` | `name` (req.), `tool` (req.), `source` (req.), `captured_at: str\|None = None`, `live: bool = True` | `coverage.py:23-33` |
| `CoverageGap` | `id`, `family`, `missing`, `fallback` (tous requis) | `coverage.py:36-42` |

Les champs `description` des sous-modèles Coverage (`Field(description=…)`) apparaissent dans l'`outputSchema` (ex. `"Data family, e.g. 'Federal legislation'."` — exemple hérité d'un autre connecteur). Les `outputSchema` complets sont dans `tests/tools.lock.json`.

---

## 6. Client PISTE (`src/fr_eli_mcp/client.py`)

### 6.1 Variables d'environnement et défauts

| Variable | Défaut | Lecture | Source |
|---|---|---|---|
| `FR_ELI_OAUTH_URL` | `https://sandbox-oauth.piste.gouv.fr/api/oauth/token` | à chaque appel d'outil | `client.py:28,42-43` |
| `FR_ELI_BASE_URL` | `runtime.base_url("eli", DEFAULT_BASE_URL)` → manifeste distant sinon `https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app` ; `rstrip("/")` | à chaque appel ; **le manifeste est récupéré même si la variable est définie** (argument par défaut de `os.environ.get` évalué avant l'appel) | `client.py:29,46-47` |
| `FR_ELI_CLIENT_ID` | — (requis) ; `.strip()` | à chaque appel | `client.py:50-58` |
| `FR_ELI_CLIENT_SECRET` | — (requis) ; `.strip()` | à chaque appel | idem |
| `FR_ELI_CACHE_DIR` | `~/.matematic/cache/fr-eli` (`expanduser`) | à chaque appel (cache + manifeste) | `cache.py:21-25`, `runtime.py:33-37` |
| `FR_ELI_AUDIT_DIR` | `~/.matematic/audit` (`expanduser`) | à chaque appel | `audit.py:28-32` |
| `FR_ELI_RUNTIME_URL` | `https://github.com/matematicsolutions/fr-eli-mcp/releases/latest/download/fr-runtime.json.gz` ; `""` désactive | **à l'import** du module | `runtime.py:22-25` |
| `FR_ELI_ENV` | présent dans `.mcp.json.example:6` (`"sandbox"`) mais **lu nulle part** | — | grep |
| `PISTE_CLIENT_ID` / `PISTE_CLIENT_SECRET` | cités par `README.md:14` mais **lus nulle part** (erreur de doc) | — | grep |

Production : « swap the sandbox hosts for `oauth.piste.gouv.fr` / `api.piste.gouv.fr` » (`DISCOVERY.md:13`, `CONSTITUTION.md:56-57`) → `https://oauth.piste.gouv.fr/api/oauth/token` et `https://api.piste.gouv.fr/dila/legifrance/lf-engine-app` (chemins déduits par substitution d'hôte : NON VÉRIFIÉ ici, à confirmer dans la documentation PISTE).

### 6.2 Identifiants et OAuth2 (`client.py:50-114`)

- Identifiants absents/vides → `CredentialsError("PISTE credentials missing: set FR_ELI_CLIENT_ID and FR_ELI_CLIENT_SECRET (from your piste.gouv.fr application, section OAuth Credentials).")`, levée dans le constructeur du client (donc avant toute lecture de cache : sans identifiants, même une réponse en cache n'est pas servie — ÉCART-REF E-23).
- `TokenManager` (un par couple `(oauth_url, client_id)`, dictionnaire de module `_TOKEN_MANAGERS`, durée de vie = processus ; changer le secret sans changer l'id ne crée pas de nouveau gestionnaire — E-24).
- `get_token(http, force=False)` sous `anyio.Lock` (sérialise les rafraîchissements concurrents) :
  - `now = time.monotonic()` (mesuré **avant** la requête) ; si `not force` et jeton présent et `now < expires_at - 60` → jeton en cache.
  - Sinon `POST {oauth_url}`, corps formulaire `grant_type=client_credentials&client_id=…&client_secret=…&scope=openid`, en-tête `Content-Type: application/x-www-form-urlencoded` (+ en-têtes du client : `User-Agent`, `Accept: application/json`).
  - `raise_for_status()` ; `access_token` absent/vide → `CredentialsError("PISTE OAuth returned no access_token.")`.
  - `expires_at = now + float(expires_in or 3600)` (défaut 3600 si clé absente) ; la marge de 60 s est `_TOKEN_SKEW_S = 60.0` (`client.py:35`).
  - `token_type` non vérifié ; scope accordé observé `openid resource.READ` (`DISCOVERY.md:12`, `README.md:95-96`).
- Le jeton n'est jamais écrit sur disque ni journalisé (`client.py:10-13`, `CONSTITUTION.md:21,32-33`).
- `invalidate()` remet `token=None, expires_at=0`.

### 6.3 Client HTTP

- Nouveau `httpx.AsyncClient` **à chaque appel d'outil** (`async with LegifranceClient()`), fermé à la fin avec le cache (`client.py:120-142`) → pas de réutilisation de connexion entre appels.
- Timeouts `httpx.Timeout(40.0, connect=10.0)` : connexion 10 s ; lecture/écriture/pool 40 s (`client.py:30`).
- En-têtes par défaut : `User-Agent: fr-eli-mcp/0.3.0 (+https://github.com/matematicsolutions/fr-eli-mcp)`, `Accept: application/json` ; par requête API : `Authorization: Bearer {token}` ; `Content-Type: application/json` (httpx `json=`).
- Redirections non suivies (`follow_redirects=False` par défaut pour `AsyncClient`, httpx `_client.py:1367`) → un 3xx devient `HTTPStatusError` → `upstream_error`.
- Décodage : `resp.json()` = `json.loads(resp.content)` (octets ; détection UTF-8/16/32 par la bibliothèque standard) (httpx `_models.py:831-832`).

### 6.4 Boucle de requête `_post(path, payload, category)` (`client.py:149-183`) — algorithme exact

```
url = base_url + path
key = "POST " + path + " " + json.dumps(payload, sort_keys=True, separators=(",", ":"), default=str)
if cache.get(key) is dict: return it                       # pas de jeton nécessaire... mais le constructeur a déjà exigé les identifiants
forced_refresh = False
for attempt in 0..2:                                       # _MAX_ATTEMPTS = 3
    try:
        token = tokens.get_token(http, force=forced_refresh)
        resp = POST url, json=payload, Authorization: Bearer token
        if resp.status == 401 and not forced_refresh:
            tokens.invalidate(); forced_refresh = True
            continue                                       # consomme une tentative, SANS attente
        resp.raise_for_status()
        data = resp.json()                                 # JSONDecodeError non rattrapée -> propagée
        cache.set(key, data, ttl=ttl_for(category)); return data
    except HTTPStatusError as e:                           # inclut les erreurs du POST OAuth
        if e.status not in {429,500,502,503,504} or attempt == 2: raise
    except (TransportError, TimeoutException):
        if attempt == 2: raise
    sleep(0.5 * 2**attempt)                                # 0,5 s puis 1 s
raise last_exc                                             # atteint seulement si la 3e tentative était un 401 "continue"
```
Conséquences exactes :
- Au plus 3 tentatives ; attentes 0,5 s et 1 s ; pas de gigue ; `Retry-After` ignoré.
- Après un 401, `forced_refresh` reste `True` : **chaque tentative suivante redemande un jeton** (E-20).
- Deuxième 401 → `HTTPStatusError` 401 non réessayable → `upstream_error`.
- Séquence 503, 503, 401 : la boucle se termine par `continue` → relève la **503 précédente**.
- Les 5xx/429 de l'endpoint OAuth sont réessayés par la même boucle.
- Les erreurs ne sont jamais mises en cache.

### 6.5 Endpoints

| Méthode client | Chemin | Corps | Catégorie (TTL) | Enveloppe de réponse lue |
|---|---|---|---|---|
| `search` | `/search` | §3.1 | `search` (3600 s) | `{results[], totalResultNumber, …}` |
| `consult_law_decree` | `/consult/lawDecree` | `{"textId", "date"}` | `act` (86400 s) | objet texte à la racine |
| `consult_article` | `/consult/getArticle` | `{"id"}` | `act` | `{article: …}` |
| `consult_juri` | `/consult/juri` | `{"textId"}` | `act` | `{text: …}` |
| `consult_cnil` | `/consult/cnil` | `{"textId"}` | `act` | `{text: …}` |
| `consult_kali_text` | `/consult/kaliText` | `{"id"}` | `act` | objet texte à la racine |
| `consult_acco` | `/consult/acco` | `{"id"}` | `act` | `{acco: …}` |

Sources : `client.py:187-208`, fixtures. Comportement de l'API pour un identifiant inexistant (404 ? 200 vide ? 500 ?) : **NON VÉRIFIÉ** (à mesurer avec un compte sandbox ; détermine si on obtient S9 ou S15).

---

## 7. Cache disque (`src/fr_eli_mcp/cache.py`)

- `diskcache.Cache(str(dir))` ouvert puis fermé **à chaque appel d'outil** ; répertoire `FR_ELI_CACHE_DIR` ou `~/.matematic/cache/fr-eli`, créé (`mkdir(parents=True, exist_ok=True)`) (`cache.py:21-43`).
- Valeur = dict JSON désérialisé (picklé par diskcache) ; clé = `"POST {path} {json trié compact, ensure_ascii=True}"` ; lecture acceptée seulement si la valeur est un `dict` (`client.py:152-154`).
- TTL (`cache.py:15-18, 45-57`) : `act` = 86400 s ; `list`/`search` = 3600 s ; `dict` = 604800 s ; `changes` = 300 s ; inconnu = 3600 s. Seuls `act` et `search` sont utilisés.
- Réglages diskcache par défaut (v5.6.3, `diskcache/core.py:48-61`) : limite 1 Gio (`size_limit=2**30`), éviction `least-recently-stored`, SQLite WAL. Persistant entre redémarrages.
- Les jetons OAuth n'y sont jamais stockés (`cache.py:1-5`).
- **Quirk** : la clé des recherches LODA/CODE contient `singleDate` à la milliseconde → **jamais de hit** pour ces fonds (E-06). Pour `fr_get_act` sans `date`, la clé change chaque jour UTC.

---

## 8. Audit JSONL (`src/fr_eli_mcp/audit.py`)

- Fichier : `{FR_ELI_AUDIT_DIR ou ~/.matematic/audit}/fr-eli-mcp.jsonl` (`audit.py:25-32,44-47`). Répertoire créé à chaque instanciation (`AuditLogger()` en tête de chaque outil).
- Écriture : ouverture en ajout (`"a"`, UTF-8), une ligne par appel, pas de verrou, pas de `fsync`, pas de rotation (`audit.py:73-75`).
- Format exact d'une ligne (`audit.py:63-73`) : `json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n"`, clés **dans cet ordre** :
  1. `ts` : `datetime.now(UTC).isoformat(timespec="milliseconds")` → `2026-10-15T15:09:12.345+00:00` (suffixe `+00:00`, pas `Z`) ;
  2. `tool` : nom de l'outil ;
  3. `input_hash` : hex sha256 minuscule (64 caractères) ;
  4. `output_count_or_size` : entier (métrique par outil, §3) ;
  5. `duration_ms` : entier (`int((perf_counter() - t0) * 1000)`, troncature) ;
  6. `status` : `"ok"` ou `"error"` ;
  7. `error` (seulement si non `None`) : `f"{type(exc).__name__}: {exc}"` tronqué à **500 points de code** (`audit.py:72`).
- Exemple succès : `{"ts":"2026-10-15T15:09:12.345+00:00","tool":"fr_coverage","input_hash":"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a","output_count_or_size":2,"duration_ms":0,"status":"ok"}`.
- Le message d'erreur peut contenir l'URL appelée (jamais le jeton, qui est dans un en-tête).
- **Hash d'entrée canonique** (`audit.py:35-38`) : `sha256(json.dumps(payload, sort_keys=True, separators=(",", ":"), default=str).encode("utf-8"))` — **`ensure_ascii=True` par défaut** : tout caractère non ASCII est échappé en `\uXXXX` (hexadécimal minuscule, paires de substitution UTF-16 au-delà du BMP). Pour une parité d'empreinte, Rust doit émuler cet échappement (formatter `serde_json` personnalisé). Vecteurs de test calculés avec la bibliothèque standard Python :

| Entrée (dict) | JSON canonique | sha256 |
|---|---|---|
| `{}` | `{}` | `44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a` |
| `{"query":"republique numerique","fond":"LODA","page_size":5}` | `{"fond":"LODA","page_size":5,"query":"republique numerique"}` | `44a8ee11979f621ab2584f279aca6523f357c5cd2d4adcb89550253865beb0d3` |
| `{"query":"vie privée","fond":"CODE","page_size":10}` | `{"fond":"CODE","page_size":10,"query":"vie privée"}` | `60f4c0810787e4b88d338d513b2306aacbba2ecdec2c44346cae299ce87ecd14` |
| `{"query":"  vie privée ","fond":"CODE","page_size":10}` | `{"fond":"CODE","page_size":10,"query":"  vie privée "}` | `8e247a13f8c46327e3e3940cbbf85a1c9af59212878927af334a424dce056709` |
| `{"query":"emoji 😀","fond":"JURI","page_size":3}` | `{"fond":"JURI","page_size":3,"query":"emoji 😀"}` | `b097b014240c833f1347c250d05463c855488144feb98eba1c36751007f65b79` |
| `{"article_id":"LEGIARTI000006419288"}` | idem | `ae5137eaa92ffcd7e3948ae676e95ef72bb9d6535df3f141d31f67344873568a` |
| `{"text_id":"LEGITEXT000033205014","date":"2026-10-15"}` | `{"date":"2026-10-15","text_id":"LEGITEXT000033205014"}` | `d8a8e991b913e763ce637f43b0b983a647d7d106aca8e6c13c4dabb1199e2183` |
| `{"decision_id":"CONSTEXT000052555275"}` | idem | `26eab19dad0d8885c5c5d5ab0716f3eccfb7f14ca193c26e8378e111203fad48` |

- **Quand une ligne est écrite** : voir matrice §2.6 (succès et erreurs de l'appel client uniquement ; pas les `invalid_arg`, pas les `not_found` post-normalisation, pas les erreurs de validation pydantic). Contredit `CONSTITUTION.md:23-27` (« Every tool call MUST append ») et INSTRUCTIONS (« every tool call appends ») — E-02.
- **`duration_ms` des lignes d'erreur toujours `0`** : l'`except` est à l'intérieur du `with timer()`, donc `t.duration_ms` vaut encore sa valeur initiale 0 (`audit.py:78-90`, ex. `server.py:203-210`) — E-03.
- Échec d'écriture : l'exception `OSError` remonte et l'appel échoue (« Inability to write = the tool returns an error », `CONSTITUTION.md:26-27`) ; si l'échec survient dans la branche d'erreur, l'`OSError` remplace l'erreur d'origine.
- Le test `test_tool_writes_an_audit_entry` vérifie seulement qu'une ligne contenant `fr_coverage` est écrite dans un répertoire temporaire (`tests/test_coverage.py:62-78`).

---

## 9. Manifeste runtime distant (`src/fr_eli_mcp/runtime.py`, `runtime/fr-runtime.json`)

- But : repointer l'URL de base amont (sandbox → production) **sans nouvelle version PyPI**, en publiant un actif de release ; le compteur de téléchargements sert de métrique d'activation (`runtime.py:1-11`).
- `get_runtime()` (`runtime.py:72-82`) : une fois par processus ; réseau d'abord (`urllib` **synchrone**, donc bloquant la boucle async jusqu'à 15 s au premier appel d'outil), puis cache disque `{cache_dir}/fr-runtime.json`, puis `{}` ; ne lève jamais. Résultat mémorisé même s'il vaut `{}` (pas de nouvelle tentative).
- `_fetch_and_cache()` (`runtime.py:51-69`) : `GET MANIFEST_URL`, `User-Agent: fr-eli-mcp-runtime (+https://github.com/matematicsolutions/fr-eli-mcp)`, timeout 15 s, `gzip.decompress`, JSON objet exigé, écrit la version décompressée en cache.
- `base_url(key, default)` (`runtime.py:85-92`) : `runtime["base_urls"][key]` si chaîne non vide (strippée), sinon `default`.
- Contenu actuel (`runtime/fr-runtime.json:1-8`) : `schema_version "1.0"`, `generated_note`, `base_urls.eli = "https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app"`, `citation_grammar_version "fr-eli/2026.07"` (**non utilisé** par le code).
- Risque : quiconque contrôle l'actif de release peut rediriger les appels API (et le jeton Bearer) vers un autre hôte quand `FR_ELI_BASE_URL` n'est pas défini (l'URL OAuth, elle, n'est pas pilotable) — E-07. Recommandation : ne pas porter en v0.1 (le brief prévoit `LF_PISTE_ENV=sandbox|production`).

---

## 10. Couverture (`src/fr_eli_mcp/coverage.py`)

- `SOURCE = 'legifrance.gouv.fr via the PISTE API'` ; `AS_OF_NOTE` ; `_FAMILIES` (3 familles) ; `_GAPS` (2 lacunes `FR-001`, `FR-002`) — textes exacts en annexe A.
- `build_coverage()` (`coverage.py:70-77`) : `status="ok"`, chaque famille complétée par `source=SOURCE, live=True` (`captured_at` reste `None`).
- Principe documenté : la liste des lacunes n'est **jamais vide** (« an empty list would mean 'nobody checked' », `coverage.py:1-10`).
- Pauvreté notable (à signaler, pas à corriger en parité) : la famille « Case law (JURI) » ne cite que `fr_get_decision` (pas `fr_search`) et ne mentionne ni CONSTIT ni CETAT ; aucune lacune n'est déclarée pour : ECLI souvent absent des CAA/TA, texte ACCO non servi (.docx), JORF non couvert, absence de pagination, Judilibre non intégré (`README.md:93-100`, `SOURCES.md:12`), absence de TOC de code.

---

## 11. Tests de la référence et portage

### 11.1 `tests/conftest.py`
Charge `.env` (racine du dépôt) dans `os.environ` sans écraser (`setdefault`) ; lignes `KEY=VALUE`, ignore vides/commentaires/sans `=`, retire guillemets simples/doubles autour de la valeur (`conftest.py:17-32`). Équivalent Rust optionnel (`dotenvy` dans les tests live).

### 11.2 `tests/test_descriptor_lock.py` (verrou des descripteurs)
- Descripteurs lus **comme un client** : `fastmcp.Client(mcp).list_tools()`, puis `t.model_dump(mode="json", exclude_none=True)` par outil, tri par nom → `{nom: descripteur}` (`test_descriptor_lock.py:35-43`). Clés présentes : `name`, `description`, `inputSchema`, `outputSchema`, `annotations`, `meta` (nom de champ pydantic, pas `_meta`).
- Empreinte `_digest(obj) = sha256(json.dumps(obj, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")).hexdigest()` (`:46-48`) — calculée sur le JSON, jamais sur les octets du fichier.
- Fichier verrou (`:51-59`, `--update` `:86-93`) : `{"package_version": <pyproject>, "toolset_sha256": _digest({nom: descripteur}), "tools": {nom: {"sha256": _digest(descripteur), "descriptor": descripteur}}}`, écrit avec `indent=2, ensure_ascii=False, sort_keys=True` + `"\n"`.
- Test (`:62-83`) : le verrou existe ; `tools` non vide (refus d'une base vide) ; calcul de `added`/`removed`/`changed` ; échec si l'un est non vide (message invitant à bumper la version et relancer `--update`) ; égalité de `toolset_sha256`.
- Vérifié indépendamment : les 8 `sha256` et le `toolset_sha256` du verrou actuel se recalculent à l'identique avec cet algorithme (`toolset_sha256 = 6012a30da2ec402c8f55e0ff0a899e9c0b747348d293b895b2e81bf0939c41f9`).
- Portage Rust : l'algorithme est transposable (serde_json avec clés triées, séparateurs compacts, UTF-8 brut : mêmes échappements que `json.dumps(ensure_ascii=False)` pour les chaînes ; éviter les flottants dont le formatage diffère). Mais les **descripteurs Rust ne seront pas identiques** (schemars produit d'autres formes de schéma ; pas de `meta.fastmcp`) : il faut un nouveau verrou de référence, et une comparaison « sémantique » avec l'ancien (noms d'outils, noms/types/défauts/requis des paramètres, descriptions, annotations, propriétés de sortie).

### 11.3 `tests/test_instructions_drift.py`
- `_registered_tool_names()` : `mcp._tool_manager._tools` si présent, sinon regex `@mcp\.tool\([^)]*\)\s+async def (\w+)` sur le source de `server.py` (`:20-25`).
- `_referenced_tool_names_in_instructions()` : jetons entre accents graves correspondant à `` `([a-z][a-z0-9_]{3,})` ``, contenant `_`, hors `{"isError","true","false","xml"}` (`:28-37`).
- `test_instructions_only_reference_registered_tools` : les jetons qui commencent par `fr_` doivent être des outils enregistrés (`:40-47`).
- `test_error_codes_documented_in_instructions` : chaque code de `VALID_CODES` apparaît dans INSTRUCTIONS avec frontières de mot `\b` (`:50-57`).
- `test_raised_error_codes_in_valid_codes` : tous les `ToolError("xxx",` du source sont des codes valides (`:60-66`) — en Rust, garanti par le typage (enum).
- `test_tool_error_format` : `str(ToolError("invalid_arg","bad"))` commence par `"[invalid_arg] "` (`:69-71`).
- `test_tool_error_rejects_unknown_code` : `ValueError` « Unknown ToolError code » (`:74-76`) — en Rust, rendu impossible par l'enum.
- `test_all_registered_tools_mentioned_in_instructions` : chaque outil enregistré apparaît en `` `nom` `` ou `` `nom( `` dans INSTRUCTIONS (`:78-92`).

### 11.4 `tests/test_parse.py` (hors ligne, fixtures) — assertions exactes

| Test (ligne) | Fixture → fonction | Assertions |
|---|---|---|
| `test_search_loda_hits` (21) | `search_loda.json`, `LODA_DATE` | `total >= 1` ; hit0 `kind == "loda"` ; `id` commence par `LEGITEXT` et ne contient pas `_` ; `"Rpublique"` absent du titre (assertion vestigiale, toujours vraie) ; pas de `<mark>` ; `source_url` commence par `https://www.legifrance.gouv.fr/loda/id/` ; `eli_uri == source_url` ; citation non vide |
| `test_search_code_hits_carry_article_ids` (35) | `search_code.json`, `CODE_DATE` | `kind == "code"` ; `articles` non vide ; `articles[0].article_id` commence par `LEGIARTI` |
| `test_search_juri_hits` (43) | `search_juri.json`, `JURI` | `kind == "juri"` ; URL `…/juri/id/` ; citation |
| `test_law_decree_metadata_and_toc` (51) | `consult_loda.json` | non `None` ; `text_id` `LEGITEXT…` ; `nature == "LOI"` ; citation ; URL `…/loda/id/` ; `eli_uri == source_url` ; TOC non vide ; `articles[0].article_id` `LEGIARTI…` |
| `test_article_text_and_citation` (63) | `consult_article.json` | `article_id` `LEGIARTI…` ; `num == "9"` ; `code_title == "Code civil"` ; citation `== "Code civil, art. 9"` ; URL `…/codes/article_lc/` ; `text` non vide |
| `test_juri_native_ecli` (74) | `consult_juri.json` | `decision_id` `JURITEXT…` ; `ecli` commence par `ECLI:FR:` ; `juridiction` ; URL `…/juri/id/` ; citation |
| `test_search_constit_hits` (84) | `search_constit.json`, `CONSTIT` | `total >= 1` ; `kind == "constit"` ; id `CONSTEXT…` ; URL `…/cons/id/` ; `eli_uri == source_url` ; citation |
| `test_search_cetat_hits` (95) | `search_cetat.json`, `CETAT` | idem avec `cetat`, `CETATEXT`, `…/ceta/id/` |
| `test_consult_juri_constit_native_ecli_and_citation` (106) | `consult_juri_constit.json` | id `CONSTEXT…` ; `ecli == "ECLI:FR:CC:2025:2025.1173.QPC"` ; `juridiction == "Conseil constitutionnel"` ; `source_url == "https://www.legifrance.gouv.fr/cons/id/CONSTEXT000052555275"` ; `eli_uri == source_url` ; citation commence par `"Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025"` |
| `test_consult_juri_cetat_native_ecli` (119) | `consult_juri_cetat.json` | id `CETATEXT…` ; `ecli` commence par `ECLI:FR:CE` ; `juridiction == "Conseil d'État"` ; `source_url == "https://www.legifrance.gouv.fr/ceta/id/CETATEXT000054178503"` ; `eli_uri == source_url` ; citation |
| `test_strip_date_suffix_and_marks` (130) | — | voir §4.2-4.3 |
| `test_no_fabricated_eli` (136) | 4 recherches (LODA, JURI, CONSTIT, CETAT) | aucun `eli_uri` ne contient `/eli/` |
| `test_search_cnil_hits` (153) | `search_cnil.json`, `CNIL` | `total >= 1` ; `kind == "cnil"` ; id `CNILTEXT…` ; URL `…/cnil/id/` ; `eli_uri == source_url` ; citation ; pas de `<mark>` |
| `test_search_kali_hits` (165) | `search_kali.json`, `KALI` | `kind == "kali"` ; id `KALITEXT…` ; URL `…/conv_coll/id/` ; `eli_uri == source_url` ; citation |
| `test_search_acco_hits_carry_company` (176) | `search_acco.json`, `ACCO` | `kind == "acco"` ; id `ACCOTEXT…` ; URL `…/acco/id/` ; `raison_sociale` et `idcc` truthy ; `raison_sociale` contenue dans la citation |
| `test_consult_cnil_citation_and_text` (189) | `consult_cnil.json` | id `CNILTEXT…` ; `nature_delib == "Sanction"` ; `text` ; `source_url == "https://www.legifrance.gouv.fr/cnil/id/CNILTEXT000053352643"` ; `eli_uri == source_url` ; citation commence par `"CNIL, deliberation n° SAN-2026-002 du"` |
| `test_consult_kali_toc_resolvable_via_get_text` (201) | `consult_kali.json` | `text_id` `KALITEXT…` ; URL `…/conv_coll/id/` ; `eli_uri == source_url` ; citation ; TOC non vide ; `articles[0].article_id` `KALIARTI…` |
| `test_consult_acco_metadata_only_no_docx_payload` (212) | `consult_acco.json` | id `ACCOTEXT…` ; `raison_sociale`, `siret`, `themes` truthy ; URL `…/acco/id/` ; `eli_uri == source_url` ; `attachment_note` ; **`"text" not in norm`** ; société dans la citation |
| `test_kali_article_url_uses_conv_coll` (226) | `consult_article.json` avec `article.id` remplacé par `KALIARTI000049886059` | `source_url == "https://www.legifrance.gouv.fr/conv_coll/id/KALIARTI000049886059"` |

Note : ces tests appellent les normaliseurs directement (pas les outils) ; aucun ne vérifie `byte_size`, `eli_note`, `dataset_note`, les ordres de champs ni `etat`.

### 11.5 Valeurs « golden » sur les fixtures (extrait ; intégral dans `01-parite-golden.json`)

| Fixture | Valeurs attendues (sortie d'outil) |
|---|---|
| `search_loda.json` (LODA) | `total=1786` ; 2 hits ; hit0 `id=LEGITEXT000033205014`, `cid=JORFTEXT000033202746`, `nature=LOI`, `etat=VIGUEUR`, `source_url=…/loda/id/LEGITEXT000033205014`, titre mojibake `LOI nÂ° 2016-1321 du 7 octobre 2016 pour une RÃ©publique numÃ©rique (1)` ; hit1 `LEGITEXT000033696785` (`DECRET`) |
| `search_code.json` (CODE) | `total=1` ; hit0 `id=cid=LEGITEXT000006070721`, `title=Code civil`, `nature=code`, URL `…/codes/texte_lc/LEGITEXT000006070721`, `articles=[{article_id: LEGIARTI000006419288, num: "9", snippet: "[...]Chacun a droit au respect de sa vie privÃ©e. Les juges peuvent, sans prÃ©judice de la rÃ©paration [...]"}]` (108 caractères) |
| `search_juri.json` | `total=122147` ; `JURITEXT000051743650` (`nature=arret`, `etat=null`), `JURITEXT000048465611` ; URL `…/juri/id/…` |
| `search_constit.json` | `total=1817` ; `CONSTEXT000043596386` (`dc`), `CONSTEXT000037434572` (`dc`), `CONSTEXT000052555275` (`qpc`) ; citation = titre nettoyé |
| `search_cetat.json` | `total=51531` ; `CETATEXT000054242861` (CAA Marseille), `CETATEXT000051655317` (CAA Nancy), `CETATEXT000054178503` (CE) ; `nature=texte` |
| `search_cnil.json` | `total=1134` ; `CNILTEXT000053352643`, `CNILTEXT000053408671` ; `nature=deliberation`, `etat=VIGUEUR` |
| `search_kali.json` | `total=27337` ; `KALITEXT000005661929` (`lettre d'adhesion`, titre sans l'espace final de la donnée brute), `KALITEXT000005661030` (`avenant`, `ABROGE`) ; `idcc=null` ; `articles=[]` (extraits KALIARTI ignorés) |
| `search_acco.json` | `total=54491` ; hit0 `ACCOTEXT000053643828`, `idcc="275"`, `raison_sociale="CORSAIR (Télétravail)"`, citation `Accord d'entreprise relatif au télétravail (CORSAIR (Télétravail))` ; hit1 `idcc="1486"`, `Accord Télétravail Hopium (HOPIUM (Télétravail))` |
| `consult_loda.json` | `text_id=LEGITEXT000033205014`, `cid=JORFTEXT000033202746`, `nor=ECFI1524250L`, `nature=LOI`, **`etat=null`**, `date_parution=2016-10-08`, `num_parution="0235"`, `date_debut_version=2023-09-08`, `date_fin_version=2999-01-01`, **62** articles (premier `LEGIARTI000033205126` num 1, dernier `LEGIARTI000033205268` num 109) → métrique d'audit 62 |
| `consult_article.json` | `num="9"`, `etat=VIGUEUR`, `code_title=Code civil`, `full_sections_titre=Livre Ier : Des personnes`, `date_debut=1970-07-19`, `date_fin=2999-01-01`, citation `Code civil, art. 9`, URL `…/codes/article_lc/LEGIARTI000006419288`, `byte_size=364` (336 caractères, mojibake) |
| `consult_juri.json` | `ecli=ECLI:FR:CCASS:2025:C100399`, `juridiction=Cour de cassation`, `formation=CHAMBRE_CIVILE_1`, `solution=Cassation`, `numero_affaire=["24-13.470"]`, `date_texte=2025-06-04`, `nature=ARRET`, citation = titre, `byte_size=644` |
| `consult_juri_constit.json` | `ecli=ECLI:FR:CC:2025:2025.1173.QPC`, `juridiction=Conseil constitutionnel`, `formation=null`, `solution=Conformité`, `numero_affaire=[]`, `date_texte=2025-11-07`, `nature=QPC`, citation **`Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025 - Absence de publicité des débats devant le juge des libertés et de la détention statuant sur la détention provisoire du prévenu`**, `byte_size=15188` |
| `consult_juri_cetat.json` | `ecli=ECLI:FR:CECHR:2026:506507.20260529`, `juridiction=Conseil d'État`, `formation=2ème - 7ème chambres réunies`, `solution=null`, `numero_affaire=[]`, `date_texte=2026-05-29`, `nature=Texte`, citation = titre `Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507`, `byte_size=15055` |
| `consult_cnil.json` | `num=SAN-2026-002`, `nature=Délibération`, `nature_delib=Sanction`, `etat=VIGUEUR`, `date_texte=2026-01-08`, `date_publication=2026-01-14`, `title=Délibération SAN-2026-002 du 8 janvier 2026`, citation **`CNIL, deliberation n° SAN-2026-002 du 8 janvier 2026`**, `byte_size=634` |
| `consult_kali.json` | `text_id=cid=KALITEXT000049878950`, `nor=ASET2450455M`, `nature=null`, `etat=VIGUEUR_ETEN` (depuis `jurisState`), `date_parution=2999-01-01` (sentinelle), `num_parution=null`, 1 article `KALIARTI000049886059` (num long `19 (des dispositions communes …)`) |
| `consult_acco.json` | `nature=ACCORD`, `siret=32862158600143`, `idcc="275"`, `code_ape=5110Z`, `secteur=Transports aériens de passagers`, `themes=["Télétravail"]`, `syndicats=["CFTC","CFE-CGC","Autre"]`, `date_texte=2025-12-19`, `date_effet=2025-12-20`, `date_depot=2026-02-27` |

### 11.6 `tests/test_coverage.py`
Familles non vides avec `name`, `tool`, `source` (`:21-25`) ; `known_gaps` non vide (`:28-32`) ; ids uniques et chaque lacune a `id`, `family`, `missing`, `fallback` (`:35-41`) ; chaque `gap.family` est le nom d'une famille (`:44-48`) ; une famille non `live` doit avoir `captured_at` (`:51-54`) ; l'outil renvoie un `status` dans `{ok, degraded, failed}` et des lacunes (`:57-60`) ; l'outil écrit une ligne d'audit mentionnant `fr_coverage` dans `FR_ELI_AUDIT_DIR` temporaire (`:62-78`).

### 11.7 `tests/test_smoke.py` (live, ignoré sans identifiants)
`pytestmark = skipif(not (FR_ELI_CLIENT_ID and FR_ELI_CLIENT_SECRET))` (`:26-27`). Tests (appels directs des fonctions d'outil) :
1. `fr_search("republique numerique", fond="LODA", page_size=5)` : `total >= 1`, hit0 id `LEGITEXT…`, URL Légifrance, citation (`:30-37`).
2. `fr_search("vie privee", fond="CODE", page_size=5)` : `total >= 1` (`:40-43`).
3. `fr_get_act("LEGITEXT000033205014")` : `nature == "LOI"`, citation, URL, TOC ; puis `fr_get_text(articles[0].article_id)` : texte non vide, URL contenant `/codes/article_lc/` (`:46-57`).
4. `fr_search("responsabilite", fond="JURI", page_size=3)` puis `fr_get_decision(hits[0].id)` : `ecli` `ECLI:FR:…`, URL `/juri/id/`, citation (`:60-67`).
5. `fr_search("liberte", fond="CONSTIT", page_size=3)` : hit0 `CONSTEXT…`, URL `/cons/id/` ; décision : `ecli` `ECLI:FR:CC:`, `juridiction == "Conseil constitutionnel"`, URL `/cons/id/`, citation commençant par `Cons. const.` (`:70-83`).
6. `fr_search("urbanisme", fond="CETAT", page_size=5)` : hit0 `CETATEXT…`, URL `/ceta/id/` ; pour **chaque** hit, `fr_get_decision` : URL `/ceta/id/`, citation ; au moins un `ecli` (commençant par `ECLI:FR:CE`) (`:86-105`).
7. `fr_search("sanction", fond="CNIL", page_size=3)` puis `fr_get_deliberation` : texte, URL `/cnil/id/`, citation (`:108-117`).
8. `fr_search("convention collective", fond="KALI", page_size=3)` puis `fr_get_convention` : URL `/conv_coll/id/`, citation ; si TOC non vide, `fr_get_text(KALIARTI…)` : texte, URL `/conv_coll/id/` (`:120-134`).
9. `fr_search("teletravail", fond="ACCO", page_size=3)` puis `fr_get_company_agreement` : `raison_sociale`, URL `/acco/id/`, `attachment_note`, citation (`:137-147`).

Portage Rust : tests d'intégration `#[ignore]` (ou feature `live`) dans `lf-adapter-piste`, mêmes requêtes et assertions, plus un test bout-en-bout via le serveur MCP.

---

## 12. CI, release et empaquetage

| Fichier | Contenu exact | Source |
|---|---|---|
| `.github/workflows/ci.yml` | `on: push (main), pull_request` ; `permissions: contents: read` ; `concurrency: ci-${{ github.ref }}`, `cancel-in-progress: true` ; job `gates` (ubuntu-latest) : `actions/checkout@v4`, `actions/setup-python@v5` Python 3.12, `pip install -e ".[dev]"`, `pytest tests/ -q -k "not smoke"` (pas de ruff/mypy en CI) | `ci.yml:1-36` |
| `.github/workflows/release.yml` | `on: push tags "v*"` + `workflow_dispatch` ; `permissions: contents: read, id-token: write`. Job 1 `build-and-publish-pypi` (si push) : gates puis `python -m build`, `pypa/gh-action-pypi-publish@release/v1` avec `secrets.PYPI_API_TOKEN`. Job 2 `build-and-upload-runtime-manifest` (si push, `contents: write`) : `gzip -9 -n -c runtime/fr-runtime.json > fr-runtime.json.gz`, `sha256sum … > fr-runtime.json.gz.sha256`, `gh release create "$TAG" … \|\| gh release upload "$TAG" … --clobber`. Job 3 `publish-mcp-registry` (`needs` job 1 ; si dispatch ou succès du job 1) : télécharge `mcp-publisher` « latest » (linux amd64) via `curl \| tar`, `mcp-publisher login github-oidc`, `mcp-publisher publish` | `release.yml:1-91` |
| `Dockerfile` | `FROM python:3.11-slim` ; `RUN pip install --no-cache-dir fr-eli-mcp` ; `ENTRYPOINT ["fr-eli-mcp"]` (sert à Glama pour introspecter) | `Dockerfile:1-5` |
| `server.json` | `$schema https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json` ; `name io.github.matematicsolutions/fr-eli-mcp` ; description « MCP server for French Legifrance (PISTE): legislation and case law with citations and ECLI. » ; `version 0.5.3` ; `repository {url, source: github}` ; `packages: [{registryType: pypi, registryBaseUrl: https://pypi.org, identifier: fr-eli-mcp, version: 0.5.3, runtimeHint: uvx, transport: {type: stdio}}]` ; **aucune `environmentVariables`** déclarée | `server.json:1-22` |
| `README.md` | marqueur de propriété registre `<!-- mcp-name: io.github.matematicsolutions/fr-eli-mcp -->` (commentaire HTML) | `README.md:3` |
| `glama.json` | `{"$schema": "https://glama.ai/mcp/schemas/server.json", "maintainers": ["mazurwieslaw2022-cmd"]}` | `glama.json:1-4` |
| `.mcp.json.example` | serveur `fr-eli-mcp`, `command: fr-eli-mcp`, env : `FR_ELI_ENV=sandbox` (non lu), `FR_ELI_OAUTH_URL`, `FR_ELI_BASE_URL` (sandbox), `FR_ELI_CLIENT_ID`/`FR_ELI_CLIENT_SECRET` (`REPLACE_WITH_YOUR_PISTE_CLIENT_…`), `FR_ELI_CACHE_DIR=~/.matematic/cache/fr-eli`, `FR_ELI_AUDIT_DIR=~/.matematic/audit` | `.mcp.json.example:1-16` |
| `.publication-gate.json` | autorise `tests/fixtures/` pour un outil interne « pubgate » (heuristiques de sommes de contrôle sensibles aux numéros polonais) | `.publication-gate.json:1-4` |
| `.gitignore` | ignore `.env`, `.mcp.json`, `.matematic/`, `*.log`, `_probe/` ; les fixtures **sont** versionnées | `.gitignore:1-27` |
| `runtime/fr-runtime.json` | §9 | `runtime/fr-runtime.json:1-8` |

Faits utiles pour la distribution Rust (registre MCP, `modelcontextprotocol/registry@11b7b0e`, `docs/modelcontextprotocol-io/package-types.mdx:132-181,225-258`) :
- `registryType: "cargo"` est documenté (crates.io uniquement), sans `runtimeHint` (exécution par nom de binaire après `cargo install`) ; vérification de propriété par la présence du texte `mcp-name: <nom>` **visible** dans le README rendu par crates.io (les commentaires HTML sont supprimés par crates.io, donc la forme `<!-- mcp-name: … -->` de la référence **ne fonctionne pas** pour cargo).
- `registryType: "mcpb"` : binaire préconstruit sur une release GitHub/GitLab, URL contenant « mcp », `fileSha256` obligatoire.
- Le schéma `Package` accepte `environmentVariables` (`name`, `description`, `isRequired`, `isSecret`, `default`, `choices`, `format`) (`docs/reference/server-json/draft/server.schema.json`) — à utiliser pour `LF_PISTE_CLIENT_ID`/`SECRET` (absent de la référence).
- Disponibilité effective de `cargo` sur le registre en production : NON VÉRIFIÉ (documentation du dépôt seulement ; `static.modelcontextprotocol.io` bloqué depuis cet environnement).

---

## 13. Inventaire des fixtures

| Fixture | Octets | Endpoint | Remarques |
|---|---|---|---|
| `search_loda.json` | 3821 | `/search` LODA_DATE | 2 résultats ; **mojibake** ; ids suffixés `_08-09-2023`/`_28-12-2016` |
| `search_code.json` | 1865 | `/search` CODE_DATE | 1 résultat ; mojibake ; `typePagination: "ARTICLE"` alors que le code envoie `DEFAUT` (enregistrée avec une autre requête ? NON VÉRIFIÉ) ; pas de facettes |
| `search_juri.json` | 12850 | `/search` JURI | 2 résultats ; mojibake |
| `search_constit.json` | 10068 | `/search` CONSTIT | 3 résultats ; propre |
| `search_cetat.json` | 14049 | `/search` CETAT | 3 résultats (2 CAA, 1 CE) ; propre |
| `search_cnil.json` | 3860 | `/search` CNIL | 2 résultats ; propre |
| `search_kali.json` | 42487 | `/search` KALI | 2 résultats ; propre |
| `search_acco.json` | 7984 | `/search` ACCO | 2 résultats ; propre |
| `consult_loda.json` | 12093 | `/consult/lawDecree` | mojibake (+6 caractères de contrôle C1) ; TOC **réduite** (≤ 5 articles par section, numérotation à trous : 1-5, 17-21, 25-29…) |
| `consult_article.json` | 3198 | `/consult/getArticle` | mojibake ; Code civil art. 9 |
| `consult_juri.json` | 8239 | `/consult/juri` | mojibake ; `texte` **tronqué à 600 caractères** (finit au milieu d'une énumération) |
| `consult_juri_constit.json` | 34758 | `/consult/juri` | complet (14322 caractères) ; propre |
| `consult_juri_cetat.json` | 39367 | `/consult/juri` | complet (14601 caractères) ; propre |
| `consult_cnil.json` | 4042 | `/consult/cnil` | `texte` tronqué (606 caractères, se termine par ` [...]`) ; propre |
| `consult_kali.json` | 4887 | `/consult/kaliText` | 1 section, 1 article ; propre |
| `consult_acco.json` | 1294 | `/consult/acco` | sans champ `data` (.docx) ; propre |

Mojibake : 6 fixtures (les plus anciennes, périmètre v0.1 : LODA, CODE, JURI) contiennent de l'UTF-8 décodé en Latin-1 puis ré-encodé (`Ã©` pour `é`, `Â°` pour `°`, `Ã\x88` pour `È`) ; la réparation `texte.encode("latin-1").decode("utf-8")` réussit sur la totalité du fichier pour les 6 (contrôle fait) ; les 10 autres sont propres. **Cause NON VÉRIFIÉE** : défaut de l'API sur ces fonds à l'époque, ou artefact de l'outil d'enregistrement ? (`resp.json()` d'httpx décode les octets comme UTF-8, donc l'outil d'enregistrement de la référence ne l'aurait pas produit avec `resp.json()`.) À vérifier en ré-enregistrant les mêmes requêtes en sandbox. La référence ne répare rien ; si Rust répare, les golden de ces 6 fixtures diffèrent de la référence (et les assertions de §11.4 restent vraies).

Les fixtures sont donc **réduites/tronquées** : utiles pour les tests de mapping, insuffisantes pour mesurer des tailles réelles (économie de contexte) — prévoir un ré-enregistrement complet (`cargo xtask record-fixture`).

---

## 14. Écarts de la référence et décisions proposées

| Id | Constat (source) | Proposition v0.1 |
|---|---|---|
| E-01 | Texte d'erreur sur le fil = `Error calling tool '<nom>': [code] msg` alors qu'INSTRUCTIONS promet un préfixe `[code]` (§2.4) | **Corriger** : texte `[code] msg` en tête (`isError=true`) — conforme à INSTRUCTIONS et au brief ; garder les messages identiques après le préfixe |
| E-02 | Pas d'audit pour `invalid_arg`, `not_found` post-normalisation, erreurs de validation (`server.py:191-197,256-258`) malgré `CONSTITUTION.md:23-27` | **Corriger** : décorateur `Audited` sur tous les chemins |
| E-03 | `duration_ms=0` sur toutes les lignes d'erreur (`server.py:203-210`) | **Corriger** : durée réelle |
| E-04 | Recherche vide = succès, INSTRUCTIONS dit `not_found` « or a search returned nothing » (`server.py:57`) | Garder le succès vide (parité, non bloquant) ; **décision** : ajuster la phrase d'INSTRUCTIONS ou lever `not_found` |
| E-05 | `date` par défaut = date UTC (`server.py:125-126`) | Brief : `Clock::today_paris()` → déviation documentée (différence entre 00:00 et 01:00/02:00 heure de Paris) |
| E-06 | Clé de cache LODA/CODE contient `singleDate` en ms → jamais de hit (`server.py:136`, `client.py:144-147`) | Corriger : arrondir au jour ou exclure de la clé |
| E-07 | Manifeste runtime distant pouvant repointer l'URL de base (risque d'exfiltration du jeton), fetch bloquant, évalué même si `FR_ELI_BASE_URL` est défini (`client.py:47`, `runtime.py`) | Ne pas porter ; `LF_PISTE_ENV` + URLs explicites |
| E-08 | User-Agent `0.3.0` périmé (`client.py:31`) | UA = `mcp-legifrance/<CARGO_PKG_VERSION> (+<repo>)` |
| E-09 | `serverInfo.version` = version FastMCP | Version du crate |
| E-10 | `fr_get_act.etat` lu dans `etat` (null) et non `jurisState` (`citations.py:219`) alors que KALI fait le repli (`citations.py:427`) | Décision : parité stricte (null) ou repli `jurisState` comme KALI (recommandé, aucun test ne fige `etat`) |
| E-11 | Sentinelle `2999-01-01` restituée telle quelle (`date_fin`, `date_fin_version`, `date_parution` KALI) | Parité (documenter le sens « sans fin ») ; éventuellement `null` en mode résumé |
| E-12 | Citations sans accents (« decision », « deliberation », mois `fevrier/aout/decembre`) et « 1 » au lieu de « 1er » (`citations.py:283-318,378`) | **Décision** : la v0.1 peut garder la parité (tests figés sur `decision n°` et `deliberation n°`), ou corriger la typographie française et adapter les tests ; à trancher par l'utilisateur |
| E-13 | Mojibake de 6 fixtures non réparé | Réparation conditionnelle dans l'adapter (brief) après vérification sur données live |
| E-14 | `idcc` `str\|int` | DTO tolérant, type conservé |
| E-15 | Message `No JURI decision found` même pour CONSTIT/CETAT | Parité (ou message neutre, à décider) |
| E-16 | Préfixe seulement vérifié, pas de `strip`, id envoyé tel quel (`server.py:159-165`) | Newtypes stricts (préfixe + 12 chiffres) ; décider : `trim()` toléré ? suffixe `_JJ-MM-AAAA` retiré ou refusé ? Garder le texte des messages |
| E-17 | `fr_get_act` accepte un id suffixé et l'envoie | idem E-16 |
| E-18 | Type d'URL de décision choisi par `origine` et non par préfixe (`citations.py:333-335`) | Parité ; repli sur le préfixe si `origine` absent (amélioration) |
| E-19 | `/codes/article_lc/` pour tout LEGIARTI, y compris articles de lois (`/loda/article_lc/` existe) | Parité en v0.1 (test smoke figé) ; vérifier la résolution ; envisager `/loda/article_lc/` |
| E-20 | Après un 401, chaque tentative suivante force un nouveau jeton ; 401 sans attente ; `Retry-After` ignoré (`client.py:157-181`) | Corriger (un seul rafraîchissement, respecter `Retry-After`, gigue) — même nombre maximal de tentatives |
| E-21 | JSON invalide → erreur sans code (`client.py:170`, `server.py:122`) | `upstream_error` (Decode) |
| E-22 | Nouveau client HTTP et nouveau cache par appel | Client `reqwest` partagé |
| E-23 | Identifiants exigés avant lecture du cache | Sans importance ; aligner sur l'architecture |
| E-24 | Gestionnaire de jeton indexé par `(oauth_url, client_id)` seulement ; env relu à chaque appel | Config lue au démarrage (documenter) |
| E-25 | Doc incohérente : `PISTE_CLIENT_ID` (README), `FR_ELI_ENV` (exemple) jamais lus | Variables `LF_*` réellement lues ; compat `FR_ELI_*` optionnelle (brief) |
| E-26 | Extraits KALIARTI des hits KALI ignorés | Amélioration possible (même forme que CODE) |
| E-27 | `repr()` Python dans les messages | Émuler `py_repr` ou accepter des guillemets différents (décision) |
| E-28 | `fr_coverage` : description contient « Returns: … » (artefact du parseur FastMCP) | Reprendre le texte exact si on vise la parité des descripteurs |
| E-29 | Validation d'arguments → résultat `isError` (FastMCP) ; avec rmcp, un échec de désérialisation de `Parameters<T>` donne une erreur JSON-RPC (NON VÉRIFIÉ ici, voir le rapport rmcp) | Décision : renvoyer `isError` + `[invalid_arg]` pour toute entrée invalide (recommandé, le modèle peut se corriger) |
| E-30 | Cache persistant sur disque (diskcache) vs brief : `moka` en mémoire | Déviation assumée (documenter : cache perdu au redémarrage) |
| E-31 | Aucune limitation de débit | Ajout (brief) |
| E-32 | `fr_search` : `query` brute dans le hash, strippée dans la requête et la sortie | Parité (hash) |
| E-33 | Lignes d'audit `ts` en `+00:00` | Parité (format identique) |
| E-34 | INSTRUCTIONS mentionne `FR_ELI_CLIENT_ID / FR_ELI_CLIENT_SECRET` et `~/.matematic/audit/fr-eli-mcp.jsonl` | Doit changer avec le renommage `LF_*` et le nouveau chemin d'audit : INSTRUCTIONS ne peut pas être identique octet pour octet (lister les lignes modifiées dans le plan) |
| E-35 | `ms_to_date` négatif dépendant de la plateforme | Calcul calendaire pur |
| E-36 | `CONSTEXT` absent de `_ID_PREFIXES` | Inclure tous les préfixes dans le domaine |

---

## 15. Check-list d'acceptation v0.1 (parité)

Chaque item doit être couvert par un test automatisé (U = unitaire `lf-domain`, A = adapter PISTE avec fixtures/wiremock, M = adapter MCP, E = bout-en-bout, L = live `#[ignore]`).

**Surface MCP**
- P-001 (M) Exactement 8 outils nommés `fr_search`, `fr_get_act`, `fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_get_company_agreement`, `fr_coverage`.
- P-002 (M) Annotations des 8 : `readOnlyHint=true`, `idempotentHint=true`, `destructiveHint=false`, `openWorldHint=true`.
- P-003 (M) Paramètres : noms, types, défauts, caractère requis identiques au §3 / annexe A (`fond` défaut `"LODA"`, `page_size` défaut `10`, `date` optionnelle nullable) ; descriptions reprises (ou divergences listées).
- P-004 (M) Chaque outil publie un `outputSchema` et renvoie `structuredContent` + un `TextContent` JSON équivalent ; propriétés de sortie identiques au §5 (noms, nullabilité, valeurs par défaut `eli_note`/`dataset_note`).
- P-005 (M) INSTRUCTIONS transmises à l'initialisation ; contenu = annexe A.1 aux seules modifications listées (E-34, nouveaux paramètres).
- P-006 (M) Test de dérive : tout `fr_*` cité existe ; tout outil enregistré est cité (`` `nom` `` ou `` `nom( ``) ; tout code d'erreur est documenté (`\b`).
- P-007 (M) Verrou de descripteurs : algorithme sha256 canonique du §11.2, refus d'une base vide, détection added/removed/changed, commande de régénération.

**Validation et erreurs**
- P-010 (M) `fond` insensible à la casse et aux espaces ; message d'erreur exact avec la liste triée.
- P-011 (M) Ordre des validations de `fr_search` : fond → query → page_size ; bornes 1..100 inclusives.
- P-012 (M) Messages `invalid_arg` exacts pour chaque identifiant (§2.6 S4-S5).
- P-013 (M) Messages `not_found` exacts par outil (§3).
- P-014 (A) Mapping : identifiants absents → `config_error` (message du §6.2) ; 404 → `not_found` `Legifrance returned 404 for that resource.` ; autres HTTP → `upstream_error` `Legifrance/PISTE HTTP {status}: …` ; réseau/timeout → `upstream_error` `Legifrance/PISTE error: …`.
- P-015 (M) Erreurs rendues `isError=true` avec préfixe `[code] ` (E-01) ; aucune fuite de secret ni de jeton.

**Requêtes PISTE**
- P-020 (A) Corps de `/search` identique au §3.1 (clés, valeurs constantes, `DATE_VERSION` pour LODA/CODE seulement, `pageNumber=1` par défaut).
- P-021 (A) Corps des 6 consultations identiques (clés `textId` vs `id`) ; `fr_get_act` envoie la date fournie (strippée) ou la date du jour.
- P-022 (A) OAuth : POST formulaire `grant_type=client_credentials`, `client_id`, `client_secret`, `scope=openid` ; jeton réutilisé jusqu'à `expires_in − 60 s` ; `expires_in` absent → 3600 ; accès concurrent sérialisé (un seul POST token pour N appels simultanés).
- P-023 (A) 401 → invalidation + un nouvel essai avec jeton neuf ; second 401 → `upstream_error`.
- P-024 (A) Réessais sur 429/500/502/503/504 et erreurs réseau : 3 tentatives max, attentes ≥ 0,5 s puis ≥ 1 s.
- P-025 (A) Timeouts : connexion 10 s, requête 40 s ; en-têtes `Accept: application/json`, `Authorization: Bearer …`, `User-Agent` versionné.
- P-026 (A) Aucune écriture du jeton sur disque ni dans les journaux.

**Normalisation (golden sur les 16 fixtures)**
- P-030 (U) `strip_marks`, `strip_date_suffix`, `ms_to_date` (UTC, flottants rejetés, sentinelle 2999-01-01), `_fr_date` : cas du §4.
- P-031 (U) `legifrance_url` : les 10 kinds du §4.5 ; `eli_uri == source_url` partout.
- P-032 (A) Hits de recherche : règles du §4.6 (kind, URL sur `id` pour LODA/CODE et sur `cid or id` sinon, citation ACCO, `articles` CODE avec snippet ≤ 280 caractères, défauts `articles: []`, `idcc/raison_sociale: null`).
- P-033 (A) TOC LODA en parcours préfixe (62 entrées sur `consult_loda.json`) ; TOC KALI = sections puis articles de premier niveau.
- P-034 (U/A) Citation article `"{code}, art. {num}"` / `"Art. {num}"` / `code` ; URL `conv_coll` pour KALIARTI.
- P-035 (U/A) Citation Cons. const. exacte (fixture constit) ; citation CNIL exacte ; JURI/CETAT = `titre`.
- P-036 (A) ACCO : métadonnées seulement, aucune clé `text`, `attachment_note` exacte, thèmes/syndicats filtrés.
- P-037 (A) `byte_size` = longueur UTF-8 du texte (364 / 644 / 15188 / 15055 / 634 sur les fixtures).
- P-038 (A) Toutes les assertions du tableau §11.4 et toutes les valeurs du §11.5 (ou leur version réparée pour les 6 fixtures mojibake, si E-13 est retenu).
- P-039 (A) Aucun `eli_uri` ne contient `/eli/` ; `ecli` jamais fabriqué (null conservé).

**Audit**
- P-040 (A/E) Une ligne JSONL par appel (y compris erreurs, E-02) ; clés et ordre `ts, tool, input_hash, output_count_or_size, duration_ms, status[, error]` ; `ts` ISO-8601 millisecondes `+00:00` ; `error` ≤ 500 caractères.
- P-041 (U) `input_hash` : algorithme du §8 ; les 8 vecteurs de test du §8 reproduits (si la compatibilité d'empreinte est retenue).
- P-042 (E) Métriques `output_count_or_size` par outil (§3) : hits, articles, octets, 1, nombre de lacunes, 0 en erreur.
- P-043 (E) Échec d'écriture de l'audit → l'appel d'outil échoue.

**Couverture**
- P-050 (U/M) `fr_coverage` renvoie exactement la sortie de l'annexe A (3 familles, 2 lacunes) ; tests du §11.6 portés.

**Live**
- P-060 (L) Les 9 scénarios smoke du §11.7 passent en sandbox avec les mêmes requêtes et assertions.

---

## 16. Points NON VÉRIFIÉS et questions ouvertes (axe parité)

1. Version exacte de FastMCP / SDK MCP / pydantic utilisée pour générer `tests/tools.lock.json` (indices : FastMCP 3.x). Vérification : `pip show fastmcp mcp pydantic` dans l'environnement de l'auteur, ou régénérer le verrou et comparer.
2. Texte exact des erreurs de validation pydantic renvoyées au client (S-validation) et rejet des arguments inconnus malgré `additionalProperties: false`. Vérification : `fastmcp.Client` sur la référence.
3. Ordre exact de `tools/list` (enregistrement ?).
4. Réponse de PISTE pour un identifiant inexistant (404 vs 200 vide vs 5xx) sur chaque endpoint.
5. Origine du mojibake des 6 fixtures (API vs enregistrement).
6. Résolution des URL `/codes/article_lc/{LEGIARTI d'une loi}` et `/conv_coll/id/{KALIARTI}` sur legifrance.gouv.fr.
7. URLs de production exactes (`https://oauth.piste.gouv.fr/api/oauth/token`, `https://api.piste.gouv.fr/dila/legifrance/lf-engine-app`) — déduites, non testées ici.
8. Comportement de `/consult/lawDecree` avec un `LEGITEXT` de **code** (renvoyé par `fr_search` fond CODE) et avec une date future ou mal formée.
9. Limite réelle de `pageSize` côté PISTE (la référence borne à 100 sans source).
10. Sémantique précise de `typeRecherche: "UN_DES_MOTS"` (« un des mots » = OU ?) et effet de `operateur: "ET"`.
11. Disponibilité effective de `registryType: "cargo"` sur le registre MCP de production.
12. Décisions à faire trancher par l'utilisateur : E-04, E-05, E-10, E-12, E-16/E-17, E-27, E-29 ; et compatibilité d'empreinte `input_hash` avec la référence (P-041).

---

## Annexe A — Textes verbatim (extraits par `ast`, sans exécution)

### A.1 `INSTRUCTIONS` (server.py:31-66) — texte intégral, octet pour octet

Longueur : 4470 caractères Unicode, 4471 octets UTF-8, 34 sauts de ligne (la chaîne se termine par `\n`). Le `\` initial de `"""\` supprime le saut de ligne de tête.

```text
This MCP server exposes the French Legifrance API through PISTE (piste.gouv.fr). It covers consolidated legislation (LODA laws & decrees, and codes), case law from three jurisdictions - Cour de cassation / cours d'appel (JURI), the Conseil constitutionnel (CONSTIT - QPC/DC decisions), and the administrative courts (CETAT - Conseil d'Etat, cours administratives d'appel, tribunaux administratifs) - plus CNIL deliberations (CNIL), collective labour agreements (KALI) and company-level agreements (ACCO). Every response carries the citation contract: a stable `eli_uri`, a `human_readable_citation` (French convention) and a `source_url` (a resolvable legifrance.gouv.fr page).

## Call order

1. `fr_search` - keyword search a `fond` (`LODA` laws/decrees, `CODE` codes, `JURI` Cour de cassation case law, `CONSTIT` Conseil constitutionnel decisions, `CETAT` administrative case law, `CNIL` CNIL deliberations, `KALI` collective labour agreements, `ACCO` company-level agreements). Each hit carries `id`, `title`, `human_readable_citation`, `source_url`. For `CODE` hits, matched `articles` carry their `article_id`.
2. `fr_get_act` - consult a LODA law/decree by its `text_id` (a `LEGITEXT...` id). Returns metadata, citation and a table of contents (`articles` with their `article_id` + `num`).
3. `fr_get_text` - the verbatim text of a single article by `article_id` (a `LEGIARTI...` id from legislation/codes, or a `KALIARTI...` id from a collective agreement's table of contents).
4. `fr_get_decision` - a court decision by `decision_id` (a `JURITEXT...`, `CONSTEXT...` or `CETATEXT...` id, from `fr_search` on fond `JURI`, `CONSTIT` or `CETAT`). Returns the native `ecli`, court, formation, solution and the decision text.
5. `fr_get_deliberation` - a CNIL deliberation by its `CNILTEXT...` id (from `fr_search` on fond `CNIL`). Returns the verbatim deliberation text.
6. `fr_get_convention` - a collective labour agreement text by its `KALITEXT...` id (from `fr_search` on fond `KALI`). Returns metadata and a table of contents whose `KALIARTI...` ids resolve via `fr_get_text`.
7. `fr_get_company_agreement` - a company-level agreement by its `ACCOTEXT...` id (from `fr_search` on fond `ACCO`). Returns METADATA ONLY (company, SIRET, IDCC, sector, themes, unions, dates) - Legifrance distributes the full text of ACCO agreements only as a .docx attachment; cite `source_url`.

## Hard constraints

- **Do not answer past the edge of this corpus** - when a search comes back empty, or the question touches material this connector does not carry, call `fr_coverage` and relay what it says is missing. Absence here is not absence in the law.
- **ELI on Legifrance:** the PISTE consult API returns the native ELI field *null* for legislation. `eli_uri` therefore carries the stable resolvable Legifrance resource URL - it is NOT a fabricated `/eli/...` string. Read `eli_note` and relay it; never invent an ELI.
- **ECLI is real for case law, but not always populated** - `fr_get_decision` returns a native authoritative `ecli` for Cour de cassation (e.g. `ECLI:FR:CCASS:2025:C100399`), Conseil constitutionnel (e.g. `ECLI:FR:CC:2025:2025.1173.QPC`) and Conseil d'Etat (e.g. `ECLI:FR:CECHR:2026:506507.20260529`). Cite it verbatim when present. CAA/TA decisions under fond `CETAT` frequently have `ecli=null` - never fabricate one; cite `human_readable_citation` + `source_url` instead.
- **Every response has `human_readable_citation` + `source_url`** - cite both to the user.
- **No modification of official text** - articles and decisions are returned verbatim from Legifrance.
- **Audit log JSONL** - every tool call appends to `~/.matematic/audit/fr-eli-mcp.jsonl`.

## Error iteration

Tools return a structured error with a `[code]` prefix:
- `invalid_arg` - a parameter is missing, malformed, or an id has the wrong prefix.
- `not_found` - no act / article / decision exists for that id, or a search returned nothing.
- `upstream_error` - a Legifrance / PISTE API error (HTTP, timeout, OAuth rejection). Retry once before surfacing.
- `config_error` - PISTE credentials are not configured (FR_ELI_CLIENT_ID / FR_ELI_CLIENT_SECRET).

## Response style

- Cite legislation as `human_readable_citation` with the `source_url`: "LOI n° 2016-1321 du 7 octobre 2016, https://www.legifrance.gouv.fr/loda/id/LEGITEXT000033205014".
- Cite case law with its `ecli` and `source_url`.
- NEVER invent an id, an ELI, an ECLI or a citation - take each from the tool output.
```

### A.2 `DATASET_NOTE` (models.py:7-12)

```text
Legifrance via PISTE. Legislation (LODA laws/decrees, codes) and case law (JURI) are searchable by keyword. The PISTE consult API does NOT expose a populated native ELI for legislation, so 'eli_uri' carries the stable resolvable Legifrance resource URL (never a fabricated /eli/ string). Case law carries a native authoritative 'ecli'.
```

### A.3 `ELI_NOTE` (models.py:14-17)

```text
Legifrance lf-engine-app returns the ELI field null for legislation; 'eli_uri' is the stable CID-keyed legifrance.gouv.fr resource URL, not a synthesized /eli/ identifier.
```

### A.4 `SOURCE` (coverage.py:57-57)

```text
legifrance.gouv.fr via the PISTE API
```

### A.5 `AS_OF_NOTE` (coverage.py:59-63)

```text
Data is queried live against the source, so there is no local snapshot date. Any date on a record states when the source published it - not that this connector verified the text is still in force today.
```

### A.6 `attachment_note` (citations.py:480-481, littéral dans `normalize_acco`)

```text
Full text is only distributed by Legifrance as a .docx attachment; consult source_url for the human-readable page. This tool returns metadata only.
```

### A.7 `_FAMILIES` (coverage.py:65)

```json
[
  {
    "name": "Legislation (LODA, codes)",
    "tool": "fr_search / fr_get_act / fr_get_text"
  },
  {
    "name": "Case law (JURI)",
    "tool": "fr_get_decision"
  },
  {
    "name": "Agreements and deliberations",
    "tool": "fr_get_convention / fr_get_deliberation / fr_get_company_agreement"
  }
]
```

### A.8 `_GAPS` (coverage.py:67)

```json
[
  {
    "id": "FR-001",
    "family": "Legislation (LODA, codes)",
    "missing": "The PISTE consult API returns a null ELI for legislation, so eli_uri carries the stable Legifrance resource URL instead of a native ELI identifier.",
    "fallback": "Treat eli_uri as a Legifrance CID-keyed URL, not an /eli/ path."
  },
  {
    "id": "FR-002",
    "family": "Legislation (LODA, codes)",
    "missing": "Legal force on a given date is not evaluated. Text is returned as the source publishes it; this connector does not decide whether a provision is in force.",
    "fallback": "Check the validity or consolidation dates on the source record itself."
  }
]
```

### A.9 Sortie attendue de `fr_coverage` (dérivée de coverage.py:65-77 + ordre des champs des modèles coverage.py:23-55)

```json
{
  "status": "ok",
  "as_of_note": "Data is queried live against the source, so there is no local snapshot date. Any date on a record states when the source published it - not that this connector verified the text is still in force today.",
  "families": [
    {
      "name": "Legislation (LODA, codes)",
      "tool": "fr_search / fr_get_act / fr_get_text",
      "source": "legifrance.gouv.fr via the PISTE API",
      "captured_at": null,
      "live": true
    },
    {
      "name": "Case law (JURI)",
      "tool": "fr_get_decision",
      "source": "legifrance.gouv.fr via the PISTE API",
      "captured_at": null,
      "live": true
    },
    {
      "name": "Agreements and deliberations",
      "tool": "fr_get_convention / fr_get_deliberation / fr_get_company_agreement",
      "source": "legifrance.gouv.fr via the PISTE API",
      "captured_at": null,
      "live": true
    }
  ],
  "known_gaps": [
    {
      "id": "FR-001",
      "family": "Legislation (LODA, codes)",
      "missing": "The PISTE consult API returns a null ELI for legislation, so eli_uri carries the stable Legifrance resource URL instead of a native ELI identifier.",
      "fallback": "Treat eli_uri as a Legifrance CID-keyed URL, not an /eli/ path."
    },
    {
      "id": "FR-002",
      "family": "Legislation (LODA, codes)",
      "missing": "Legal force on a given date is not evaluated. Text is returned as the source publishes it; this connector does not decide whether a provision is in force.",
      "fallback": "Check the validity or consolidation dates on the source record itself."
    }
  ]
}
```

### A.10 Descripteurs d'outils (tests/tools.lock.json) — `description` et descriptions de paramètres exactes

`package_version` = `0.5.3` ; `toolset_sha256` = `6012a30da2ec402c8f55e0ff0a899e9c0b747348d293b895b2e81bf0939c41f9`

#### `fr_coverage` — sha256 `c814149c98c5dbb81f24e003326ac36736f6cdf4b5331cac39a6041cf7239452`

description :
```text
Declare what this connector covers, how it is sourced, and what it does NOT cover.

Call this before telling a user that the law "does not contain" something, and whenever
a search comes back empty: the absence may be a gap in this connector rather than in the
law. Every gap carries a fallback saying where to look instead.

Returns:
    ``Coverage`` with families, an as-of note, and a non-empty list of known gaps.
```

(aucun paramètre ; `inputSchema` = `{"additionalProperties": false, "properties": {}, "type": "object"}`)

#### `fr_get_act` — sha256 `3dfbd11fd5027ac5180773c51dd58c1b4bf599871b31e40d0fa0b9efde4b89e2`

description :
```text
Consult a LODA law or decree by its ``LEGITEXT...`` id.
```

| paramètre | type JSON Schema | défaut | requis |
|---|---|---|---|
| `date` | `string \| null` | `null` | non |
| `text_id` | `string` | `—` | oui |

description de `date` :
```text
version date ``YYYY-MM-DD`` (default: today - the version in force now).
```
description de `text_id` :
```text
a ``LEGITEXT...`` identifier (from ``fr_search`` on fond ``LODA``).
```

#### `fr_get_company_agreement` — sha256 `17f0a0eecba40ea7d5d69aa804d0602ca12e2efdd4555f39ff3d8f9a181bfa6c`

description :
```text
Consult a company-level agreement by its ``ACCOTEXT...`` id - METADATA ONLY.

Legifrance distributes the full text of ACCO agreements only as a ``.docx``
attachment, so this tool returns the agreement metadata (company, SIRET, IDCC,
sector, themes, signatory unions, dates) plus the resolvable ``source_url``.
```

| paramètre | type JSON Schema | défaut | requis |
|---|---|---|---|
| `agreement_id` | `string` | `—` | oui |

description de `agreement_id` :
```text
an ``ACCOTEXT...`` identifier (from ``fr_search`` on fond ``ACCO``).
```

#### `fr_get_convention` — sha256 `d3193636fe8efda9632c3f1cd7598ef292790530fc3ff4adc96363fd977d783a`

description :
```text
Consult a collective labour agreement text by its ``KALITEXT...`` id.
```

| paramètre | type JSON Schema | défaut | requis |
|---|---|---|---|
| `convention_id` | `string` | `—` | oui |

description de `convention_id` :
```text
a ``KALITEXT...`` identifier (from ``fr_search`` on fond ``KALI``).
```

#### `fr_get_decision` — sha256 `10c07370a00d5c306e9297e9a91a32a8ced6ea90a5835cbed71e6807e7ab3f0e`

description :
```text
Consult a court decision by its id (returns the native ECLI).

Covers three jurisdictions via the same Legifrance ``consult/juri`` endpoint:

- ``JURITEXT...`` - Cour de cassation / cours d'appel (fond ``JURI``).
- ``CONSTEXT...`` - Conseil constitutionnel QPC/DC decisions (fond ``CONSTIT``).
- ``CETATEXT...`` - Conseil d'Etat, CAA, TA administrative case law (fond ``CETAT``).
```

| paramètre | type JSON Schema | défaut | requis |
|---|---|---|---|
| `decision_id` | `string` | `—` | oui |

description de `decision_id` :
```text
a ``JURITEXT...``, ``CONSTEXT...`` or ``CETATEXT...`` identifier (from
``fr_search`` on fond ``JURI``, ``CONSTIT`` or ``CETAT`` respectively).
```

#### `fr_get_deliberation` — sha256 `3a3a47fedaa9076ce4eb930925d9750bc237e67d8f7a0fb0e75db1b698218de9`

description :
```text
Consult a CNIL deliberation by its ``CNILTEXT...`` id.
```

| paramètre | type JSON Schema | défaut | requis |
|---|---|---|---|
| `deliberation_id` | `string` | `—` | oui |

description de `deliberation_id` :
```text
a ``CNILTEXT...`` identifier (from ``fr_search`` on fond ``CNIL``).
```

#### `fr_get_text` — sha256 `c7a0d883c96b756cacb9c80845af28f0e5711dc6b5f127d3cca309a8176202c8`

description :
```text
Fetch the verbatim text of a single article (``LEGIARTI...`` or ``KALIARTI...``).
```

| paramètre | type JSON Schema | défaut | requis |
|---|---|---|---|
| `article_id` | `string` | `—` | oui |

description de `article_id` :
```text
a ``LEGIARTI...`` identifier (from ``fr_get_act`` or a ``CODE`` search
hit), or a ``KALIARTI...`` identifier (from ``fr_get_convention``).
```

#### `fr_search` — sha256 `81727319c2603cf8aadc1bb8189c1c9429846d9be37f5f4c960c4f37993dabd1`

description :
```text
Keyword-search French legislation or case law on Legifrance.
```

| paramètre | type JSON Schema | défaut | requis |
|---|---|---|---|
| `fond` | `string` | `"LODA"` | non |
| `page_size` | `integer` | `10` | non |
| `query` | `string` | `—` | oui |

description de `fond` :
```text
one of ``LODA`` (laws & decrees), ``CODE`` (codes), ``JURI`` (Cour de cassation
case law), ``CONSTIT`` (Conseil constitutionnel - QPC/DC decisions), ``CETAT``
(administrative case law: Conseil d'Etat, cours administratives d'appel, tribunaux
administratifs), ``CNIL`` (CNIL deliberations, incl. sanctions), ``KALI``
(collective labour agreements / conventions collectives), ``ACCO`` (company-level
agreements - metadata + search only).
```
description de `page_size` :
```text
number of hits (1..100).
```
description de `query` :
```text
free-text query (e.g. "republique numerique", "responsabilite").
```

---

## Annexe B — Fichiers d'appui

- `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/research/01-parite-golden.json` : clés `normalizer` (sortie brute de chaque `normalize_*` par fixture, plus la variante `consult_article.json[id=KALIARTI000049886059]`) et `tool_output` (structuredContent attendu, ordre des champs des modèles, `byte_size`, `eli_note`, `dataset_note` ; `query` laissé en marqueur `<query.strip()>`).
- `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/scripts/golden.py` (réimplémentation indépendante), `check_golden.py` (assertions de `test_parse.py` rejouées : toutes vraies), `extract_consts.py` (extraction verbatim par `ast`).
- Clones tiers consultés (lecture seule) : `scratchpad/clones/fastmcp-v3.4.8`, `scratchpad/clones/mcp-python-sdk-v1.26.0`, `scratchpad/clones/httpx` (0.28.1), `scratchpad/clones/diskcache` (5.6.3), `scratchpad/clones/mcp-registry-p1` (commit 11b7b0e).

Sources web : https://www.legifrance.gouv.fr/loda/article_lc/LEGIARTI000006716311 (existence du motif `/loda/article_lc/`, via WebSearch) ; https://glama.ai/mcp/servers/matematicsolutions/fr-eli-mcp/tools/fr_get_text (la référence est répertoriée sur Glama).
