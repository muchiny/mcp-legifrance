# ADR-0010 — Erreurs métier in-band `[code]`

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M2 (`CorpusError`), M3 (`AppError`, table de correspondance), M4 (rendu MCP, instructions)
- **Questions liées** : —

## Contexte

- **Le préfixe `[code]` n'arrive pas en tête.** La référence lève une `ToolError` à 4 codes (`invalid_arg`, `not_found`, `upstream_error`, `config_error`) avec le message `"[code] message"` (`server.py:69-78`). FastMCP l'enveloppe en « Error calling tool '<nom>': [code] … ». Le préfixe promis par les instructions n'est donc pas en tête (E-01, recherche 01 §2.4).
- **Erreurs mal classées** :
  - E-21 et D-11 : un corps non JSON donne une erreur **sans code** ;
  - D-10 : un 401 OAuth (`invalid_client`) devient `upstream_error`, et un 404 de l'endpoint OAuth ou d'un mauvais chemin devient `not_found`. Le modèle conclut alors qu'un texte n'existe pas ;
  - D-40 : l'URL httpx complète apparaît dans le message ;
  - D-08 : trace de pile en ERROR pour une erreur attendue.
- **Spécification** :
  - les erreurs d'exécution, **dont la validation des entrées**, se signalent par `isError: true`, avec un message actionnable. C'est la règle SEP-1303 depuis 2025-11-25 (`server/tools.mdx:760-785`, recherche 06 §3.5) ;
  - le client SHOULD les transmettre au modèle ; il MAY ne pas transmettre les erreurs de protocole ;
  - la plage `-32020` à `-32099` est réservée.
- **Clients** : sur une erreur, Claude Code ne transmet que les blocs texte et `resource_link`, et ignore le `structuredContent` (recherche 06 §3.3).
- **rmcp 3.5.1** : `Err(CallToolResult)` devient `isError: true`, et `Err(ErrorData)` devient une erreur JSON-RPC (recherche 04 §2.6, §2.7). Bug #1347 : une méthode connue aux `params` invalides reçoit `-32601` au lieu de `-32602` (recherche 09 §4).

## Décision

1. **Erreur métier** = `CallToolResult { isError: true }`, avec **un seul bloc texte** `[code] message actionnable`, **sans** `structuredContent` (00 §5, 05 §7.1).
2. **Six codes**, et pas un de plus : `invalid_arg`, `not_found`, `upstream_error`, `rate_limited`, `config_error`, `audit_error`.
   - `ErrorCode` en est la source unique.
   - Le test de dérive vérifie que les codes cités dans les instructions sont exactement `ErrorCode::ALL`.
   - Correspondance complète, cause par cause : 02 §9.2.
3. **Erreurs de protocole → JSON-RPC** :
   - outil inconnu : `-32602` ;
   - JSON illisible, batch, `_meta` moderne incomplet : `-32700`, `-32600` ou `-32602` ;
   - version non supportée : `-32022` ;
   - méthode inconnue : `-32601`.

   Aucun code JSON-RPC maison. Aucune ligne d'audit, puisque rien n'a été exécuté.
4. **Classement des erreurs amont** (02 §9.2) :
   - OAuth 400, 401 ou 404 → `config_error`, **jamais** `not_found` ;
   - API 401 après **un** renouvellement de jeton → `config_error` ;
   - 404 de routage (sur `/search`, corps non JSON, mauvaise `LF_PISTE_BASE_URL`) → `config_error` ;
   - 404 reconnu sur `/consult/*`, ou 200 vide → `not_found` ;
   - 429 → `rate_limited` ;
   - `Timeout`, `Decode`, 5xx, 400 non reconnu et délai global → `upstream_error`.

   La cause détaillée (`UpstreamCause`) va dans `tracing` et l'audit, sans créer de code.
5. **Décodage des arguments** : une erreur devient `[invalid_arg] chemin: erreur serde`, in-band et auditée. C'est la décision prise sur E-29, au moyen de `JsonObject` ([ADR-0008](0008-audit-decorateur-strict.md)).
6. **Rédaction des messages** (02 §9.3, 05 §7.2 à §7.4) :
   - en anglais ; la première phrase de parité est conservée (P-012 à P-014) ;
   - le message dit **quoi changer** ou **quel outil appeler**, par exemple `… CNILTEXT... identifies a CNIL deliberation: use fr_get_deliberation.` ;
   - « retry once » seulement pour les codes réessayables ;
   - jamais de secret, de jeton, d'en-tête, de chemin local ni d'URL avec paramètres ;
   - les erreurs attendues sont journalisées en `info`.

## Conséquences

### Positives

- Le modèle peut se corriger : il voit le code, la cause et le correctif, quels que soient l'ère et le client (le texte passe partout).
- Les six codes restent compatibles avec la référence (4 codes historiques plus 2 nouveaux, annexe A §6.5).
- Les erreurs de configuration ne passent plus pour des absences en droit (D-10).

### Négatives

- Le code n'est pas un champ structuré : un client programmatique doit lire le préfixe du texte.
- `retryable` et `retry_after` ne sont pas structurés ; le délai n'est donné qu'en clair (`retry in 12 s`).
- `Timeout` et `Decode` ne se distinguent pas par le code. La cause n'est visible que dans les journaux.

### À surveiller

- Points **NON VÉRIFIÉS** sur PISTE :
  - [v:forme-404-piste] : forme d'un 404 PISTE ;
  - [v:forme-403-piste] : sens d'un 403, provisoirement `config_error` ;
  - [v:retry-after-piste] : présence de `Retry-After` sur un 429 ;
  - [v:401-sous-throttling] : 401 `invalid_client` renvoyé sous throttling.
- **NON VÉRIFIÉ** [v:cc-structured-content-succes] : Claude Code transmet-il toujours le texte seul sur une erreur ?
- **NON VÉRIFIÉ** [v:rmcp-bugs-statut] : statut de l'issue #1347 de rmcp, acceptée telle quelle.

### Tests qui la gardent

- `error_results_are_text_with_code_prefix`, `error_text_snapshot_per_code`, `invalid_arg_is_not_logged_as_error`.
- `oauth_404_maps_to_config_not_not_found`, `404_on_search_is_config_not_not_found`, `corpus_error_maps_to_app_error_table`.
- `error_messages_never_contain_secret_or_userinfo`, et test de dérive des instructions (06 §3.1, D-08, D-10, D-40).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Objet d'erreur dans `structuredContent` (`code`, `retryable`, `hint`) (recherche 02 D-08) | Ignoré par Claude Code sur une erreur (recherche 06 §3.3). L'exemple normatif de la spécification n'en a pas. |
| Erreurs JSON-RPC pour les erreurs métier | Le client MAY ne pas les transmettre au modèle. Contraire à SEP-1303 pour la validation d'entrée. |
| Codes JSON-RPC applicatifs | Plage réservée (MUST NOT) ou déconseillée (recherche 06 §3.5). |
| Codes supplémentaires `timeout` et `invalid_response` (recherche 02 D-08) | 00 §5 fixe six codes. Le modèle réagit de la même façon, en réessayant une fois puis en signalant. |
| Préfixe à la FastMCP « Error calling tool … » | E-01 : le code n'est plus en tête. |
| `Parameters<T>` et erreurs de décodage produites par rmcp | Non auditées et non préfixées ([ADR-0008](0008-audit-decorateur-strict.md)). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §5, §6 ; [02-architecture.md](../plan/02-architecture.md) §9.1 à §9.3, §13, §13.1 n° 4 et 13 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §7.1 à §7.5, §13.1 n° 5 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §3.5, §11 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §2.2, §6.5.
- Recherche : [01-parite.md](../recherche/01-parite.md) §2.4 à §2.6, E-01, E-21, E-29, P-012 à P-015 ; [02-defauts.md](../recherche/02-defauts.md) D-08, D-10, D-11, D-40, D-43 ; [06-mcp-spec.md](../recherche/06-mcp-spec.md) §3.3, §3.5 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §2.6, §2.7 ; [09-rmcp-2026.md](../recherche/09-rmcp-2026.md) §4.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/server.py:46-60, 69-78, 110-122`.
