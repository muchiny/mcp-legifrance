# 06 — Spécification MCP actuelle et bonnes pratiques de conception d'outils (serveur Légifrance en Rust)

> Axe : ce que la spécification MCP **en vigueur au 2026-10-15** impose à un serveur en lecture seule (tools, resources, prompts, completions, utilitaires, transports, sécurité), les bonnes pratiques de conception d'outils **économes en contexte**, la **stabilité des descripteurs** (descriptor lock) et le **registre MCP** (`server.json`, types de paquets pour un binaire natif). Le rapport se termine par une check-list de conformité concrète pour notre serveur.
> Rédigé le 2026-10-15. Chaque affirmation est sourcée (`chemin:ligne` ou URL). Ce qui n'a pas pu être confirmé porte la mention **NON VÉRIFIÉ**. Les passages « Recommandation » sont des propositions de conception, pas des faits sourcés.
> Complément direct du rapport `08-mcp-2026-07-28.md` (changelog normatif, cycle de vie sans session, stratégie dual-era), auquel ce document renvoie au lieu de le répéter.

---

## 0. Sources et conventions de citation

Les dépôts ont été clonés en `--depth 1` dans des répertoires neufs et lus comme des données non fiables. Aucun code n'a été exécuté.

| Alias | Source | Commit / date |
|---|---|---|
| `[S]` | `…/scratchpad/clones/mcp-spec-axe06` (github.com/modelcontextprotocol/modelcontextprotocol, `main`) | `86f6e0c6` du 2026-10-14 |
| `SPEC/` | `[S] docs/specification/2026-07-28/` | idem |
| `SCHEMA` | `[S] schema/2026-07-28/schema.ts` (source de vérité, `SPEC/basic/index.mdx:236-240`) | idem |
| `DOCS/` | `[S] docs/docs/2026-07-28/` (guides non normatifs) | idem |
| `BLOG/` | `[S] blog/content/posts/` | idem |
| `[R]` | `…/clones/mcp-registry-axe06` (github.com/modelcontextprotocol/registry) | `11b7b0e2` du 2026-10-11 |
| `[B]` | `…/clones/mcpb-axe06` (github.com/modelcontextprotocol/mcpb) | `70fe3b34` du 2026-04-22 |
| `[PY]` | `/home/user/matematicsolutions/fr-eli-mcp` (référence Python, lecture seule) | `d3b5b131` du 2026-09-24 |
| `[VS]` | `…/clones/vscode-08` (github.com/microsoft/vscode, sparse `src/vs/{platform,workbench/contrib}/mcp`, cloné par l'axe 08, relu ici) | `3f98d214` du 2026-10-15 |
| `[CC]` | `…/clones/claude-code-08` (github.com/anthropics/claude-code, `CHANGELOG.md`) | `94506ff0` du 2026-10-14 |
| `[PYSDK]` | `…/clones/mcp-python-sdk-v1.26.0` (SDK Python officiel) | `3d9d3455` du 2026-01-24 |

Préfixe complet du scratchpad : `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/`.

Sources web lues le 2026-10-15 (WebFetch) :
- `[A-TOOLS]` Anthropic Engineering, « Writing effective tools for agents — with agents », Ken Aizawa, 11 septembre 2025 : https://www.anthropic.com/engineering/writing-tools-for-agents
- `[CC-DOC]` Documentation Claude Code, « Connect Claude Code to tools via MCP » : https://code.claude.com/docs/en/mcp
- `[API-TOOLS]` Documentation Claude API, « Define tools » (redirigée depuis `implement-tool-use`) : https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools
- `[CC-86032]` https://github.com/anthropics/claude-code/issues/86032 ; `[CC-55677]` https://github.com/anthropics/claude-code/issues/55677 ; `[CC-45575]` https://github.com/anthropics/claude-code/issues/45575

Sources injoignables (proxy / DNS) : `invariantlabs.ai`, `code.visualstudio.com`, `www.rfc-editor.org`, `datatracker.ietf.org`. Les points qui en dépendent sont marqués **NON VÉRIFIÉ**.

Rapports voisins cités : `research/01-parite.md` (parité Python), `research/02-defauts.md` (défauts de la référence), `research/04-rmcp-rust.md` et `research/09-rmcp-2026.md` (SDK Rust `rmcp`), `research/08-mcp-2026-07-28.md` (révision 2026-07-28).

---

## 1. Résumé exécutif

1. **Révision cible : `2026-07-28`**, dernière publiée (tag `2026-07-28` = `5f5440bb`, aucun tag postérieur au 2026-10-15 ; `git ls-remote --tags` exécuté le 2026-10-15 ; `SCHEMA:30`). Le protocole y devient **sans état** : `server/discover`, `_meta` par requête, `resultType`, `ttlMs`/`cacheScope`. Les clients VS Code et Cursor restant en `2025-11-25`, le serveur doit être **dual-era** (détails dans `08-mcp-2026-07-28.md` §4, §15).
2. **`structuredContent` est ce que voient les modèles.**
   - **VS Code** : si `structuredContent` est présent, il **remplace** les blocs texte et envoie `JSON.stringify(structuredContent)` au modèle (`[VS] src/vs/workbench/contrib/mcp/common/mcpLanguageModelToolContribution.ts:331-337`, `:389-392`).
   - **Claude Code** : même comportement en cas de succès. Un mainteneur l'a confirmé le 2026-08-15 sur v2.1.233. En cas d'**erreur** (`isError: true`), Claude Code ne transmet que le **texte** et ignore `structuredContent` (`[CC-86032]`).
   - **Conséquences** : le résumé économe en contexte doit **être** le `structuredContent`, et le message d'erreur actionnable doit être **entièrement** dans `content[0].text`.
3. **Contrat `outputSchema`.**
   - Avec un `outputSchema`, le serveur **MUST** renvoyer un `structuredContent` conforme (`SPEC/server/tools.mdx:509-515`). Il **SHOULD** aussi renvoyer le JSON sérialisé dans un `TextContent` (`:500`).
   - En `2025-11-25` (legacy), `outputSchema` et `structuredContent` doivent être des **objets** (`[S] schema/2025-11-25/schema.ts:1113`, `:1279-1284`). Il faut donc toujours une racine objet.
4. **Erreurs.**
   - Tool inconnu ou requête mal formée → erreur JSON-RPC (`-32602`).
   - Échec d'API, validation des arguments ou règle métier → résultat `isError: true` avec un message exploitable pour l'auto-correction (`SPEC/server/tools.mdx:738-785`). Ce choix est aligné sur SEP-1303 depuis 2025-11-25 (`[S] docs/specification/2025-11-25/changelog.mdx:28`).
5. **Annotations.**
   - Ce sont de simples *hints*, que les clients doivent traiter comme non fiables (`SPEC/server/tools.mdx:304-307`). Les valeurs par défaut sont pessimistes (`SCHEMA:1912-1954`).
   - Pour nous : `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`. Dans VS Code, `readOnlyHint: true` supprime la confirmation avant exécution (`[VS] …ToolContribution.ts:234-239`).
   - **`openWorldHint: true` (valeur de la référence Python) déclenche dans VS Code une confirmation des *résultats*** (`:240-242`, `:157`). Ce choix est à trancher (questions ouvertes).
6. **Pas de `logging` MCP** (déprécié, `SPEC/server/utilities/logging.mdx:7-18`). Logs sur **stderr** uniquement : en stdio, rien d'autre que des messages MCP sur stdout (`SPEC/basic/transports/stdio.mdx:13-19`). **Sampling et Roots sont dépréciés.** L'élicitation (via MRTR) est inutile en v1 ; s'il fallait demander un secret, le mode formulaire serait interdit (`SPEC/client/elicitation.mdx:30-33`).
7. **Resources, templates (RFC 6570), resource links, prompts et completions** sont utiles mais secondaires.
   - Les resources sont *application-driven* et les prompts *user-controlled* (`DOCS/learn/server-concepts.mdx:14-18`).
   - Schéma d'URI custom `legifrance://` conforme RFC 3986 (`SPEC/server/resources.mdx:397-400`). Ressource inexistante → `-32602` (moderne), jamais `contents: []` (`:404-410`).
   - Dans Claude Code, `structuredContent` remplaçant les blocs `content`, un `resource_link` ne doit pas être la seule trace de l'URI : il faut **aussi** mettre l'URI dans `structuredContent`.
8. **Limites de Claude Code à respecter** (`[CC-DOC]`) :
   - avertissement au-delà de **10 000 tokens** de sortie, limite de **25 000 tokens** par défaut (`MAX_MCP_OUTPUT_TOKENS`) ;
   - au-delà de **50 000 caractères**, le résultat texte est écrit dans un fichier, sauf `_meta["anthropic/maxResultSizeChars"]` (plafond 500 000) ;
   - `instructions` serveur et description d'un tool chargé d'emblée coupées à **4 096 caractères** ;
   - nom complet `mcp__<serveur>__<tool>` limité à **128 caractères** ;
   - pas de combinateur à la racine d'`inputSchema` ; noms de propriétés de premier niveau sur 1 à 64 caractères `[A-Za-z0-9_.-]`.
   - **Les `INSTRUCTIONS` Python font 4 470 caractères** (`[PY] src/fr_eli_mcp/server.py:31-66`, mesuré), donc **tronquées** par Claude Code.
9. **Conception économe en contexte** (`[A-TOOLS]`, `[API-TOOLS]`) :
   - peu d'outils, ciblés sur des workflows à fort impact, sans recouvrement ;
   - préfixe de namespace (`fr_`) et paramètres non ambigus (`article_id`, pas `id`) ;
   - réponses à haut signal, avec un enum `response_format` concis/détaillé (exemple de l'article : 72 tokens contre 206) ;
   - pagination, filtrage et troncature avec des **valeurs par défaut raisonnables** et un **message de troncature qui guide l'agent** ;
   - erreurs actionnables, descriptions de 3 à 4 phrases au moins, mots-clés en tête (la recherche de tools de Claude Code ne lit que le début).
10. **Stabilité des descripteurs.**
    - La spec demande un **ordre déterministe** de `tools/list` (SHOULD), pour le cache client et le cache de prompt LLM (`SPEC/server/tools.mdx:71-74`). Modifier les définitions en cours de conversation invalide le cache de prompt (`DOCS/develop/clients/client-best-practices.mdx:151-160`).
    - Le guide de sécurité nomme le **« rug pull »**, un changement silencieux des définitions après approbation, et recommande les clients qui **épinglent** les définitions (`DOCS/tutorials/security/local-server-security.mdx:92-100`, `:205-208`).
    - D'où le *descriptor lock* de la référence (`[PY] tests/test_descriptor_lock.py:1-18`), à reprendre en Rust en étendant son périmètre : prompts, templates, `instructions`, capacités.
11. **Registre MCP.**
    - `server.json` : `name` en DNS inversé avec un seul `/`, `description` de 100 caractères au plus, `version` unique et **immuable** une fois publiée.
    - Types de paquets acceptés par le registre officiel : `npm`, `pypi`, `nuget`, `oci`, `mcpb` et **`cargo`** (`[R] pkg/model/constants.go:5-10`, `internal/validators/package.go:15-29`).
    - Pour un binaire Rust :
      - **`mcpb`** (binaire précompilé sur GitHub/GitLab Releases, `fileSha256` obligatoire, l'URL doit contenir « mcp ») ;
      - **`cargo`** (crates.io, token `mcp-name:` **visible** dans le README : les commentaires HTML sont supprimés par crates.io) ;
      - **`oci`** (label `io.modelcontextprotocol.server.name`).
    - Le `server.json` Python ne déclare **pas** les variables d'environnement PISTE (`environmentVariables` avec `isSecret`), qu'il faut ajouter.

---

## 2. Révision de référence et documents normatifs

- Révisions présentes : `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`, `2026-07-28`, `draft` (`[S] docs/specification/`). Seule `2026-07-28` est marquée « latest » (vérifié et détaillé dans `08-mcp-2026-07-28.md` §2).
- Composants **obligatoires** pour toute implémentation : protocole de base, versioning et *message patterns* (requête/réponse, MRTR, subscribe/notify). Le reste (resources, prompts, tools, utilitaires) est **optionnel** (`SPEC/basic/index.mdx:7-19`).
- Le schéma TypeScript fait foi, le JSON Schema en est dérivé (`SPEC/basic/index.mdx:236-245`).
- Politique de cycle de vie : Active, puis Deprecated, puis Removed, avec au moins 12 mois de dépréciation (`SPEC/changelog.mdx:107-112`). Registre des dépréciations : Roots, Sampling, Logging, DCR, `includeContext` et HTTP+SSE (`SPEC/deprecated.mdx:22-31`).

---

## 3. Tools

### 3.1 Capacité, liste, ordre, pagination, `listChanged`

| Règle | Niveau | Source |
|---|---|---|
| Déclarer la capacité `tools` (`{ "listChanged"?: bool }`) | MUST | `SPEC/server/tools.mdx:45-60` ; `SCHEMA:865-870` |
| `tools/list` renvoie l'ensemble disponible ; **ne varie ni par connexion ni par effet de bord** ; peut varier selon l'autorisation présentée | MUST NOT / MAY | `SPEC/server/tools.mdx:62-69` |
| **Ordre déterministe** (même ordre tant que l'ensemble ne change pas), pour le cache client et le cache de prompt LLM | SHOULD | `SPEC/server/tools.mdx:71-74` ; `SPEC/changelog.mdx:34` |
| `tools/list` paginé (curseur opaque, taille de page choisie par le serveur) et **cachable** (`ttlMs`, `cacheScope` requis) | — / MUST | `SPEC/server/tools.mdx:78-81` ; `SPEC/server/utilities/pagination.mdx:24-30`, `:83-90` ; `SPEC/server/utilities/caching.mdx:13-21` ; `SCHEMA:1779-1781` |
| Curseur invalide → `-32602` ; curseurs stables | SHOULD | `SPEC/server/utilities/pagination.mdx:94-96`, `:109-111` |
| Le client traite l'absence de `nextCursor` comme la fin de la liste | SHOULD (client) | `SPEC/server/utilities/pagination.mdx:98-100` |
| `notifications/tools/list_changed` seulement si `listChanged` est déclaré, et en moderne **uniquement** sur un flux `subscriptions/listen` ouvert avec `toolsListChanged: true` | SHOULD | `SPEC/server/tools.mdx:236-248` ; `SCHEMA:1887-1898` |
| `ttlMs` sans `listChanged` : le client se fie uniquement au TTL | MAY | `SPEC/server/utilities/caching.mdx:114-118` |
| `cacheScope: "public"` adapté aux listes identiques pour tous | — | `SPEC/server/utilities/caching.mdx:103-108` |

**Pour nous** : la liste est statique pour un binaire donné. On ne déclare donc **pas** `listChanged`, on renvoie une seule page sans `nextCursor` (une douzaine de tools), on fixe l'ordre (tableau ordonné, pas de `HashMap`) et on pose `cacheScope: "public"` avec un TTL long (voir `08` §7.1).

### 3.2 Le descripteur `Tool`

Champs (`SPEC/server/tools.mdx:284-302` ; `SCHEMA:1973-2015`, `BaseMetadata` `:954-969`, `Icons` `:934-947`) :

| Champ | Règles | Source |
|---|---|---|
| `name` | Identifiant unique (SHOULD), 1 à 128 caractères, sensible à la casse, uniquement `[A-Za-z0-9_.-]`, sans espace ni virgule (SHOULD) | `SPEC/server/tools.mdx:309-321` |
| *(contrainte client)* | L'API Claude exige `^[a-zA-Z0-9_-]{1,128}$`, **sans point** (`[API-TOOLS]`, tableau « Specifying client tools »). Claude Code écarte un tool dont le nom complet `mcp__<server>__<tool>` dépasse 128 caractères (`[CC-DOC]`, « Tools with names longer than 128 characters »). Le traitement des points dans un nom de tool MCP par Claude Code est **NON VÉRIFIÉ**. **→ Noms `snake_case` ASCII sans point.** | web |
| *(collisions)* | L'unicité ne vaut que dans un serveur ; les agrégateurs SHOULD préfixer par serveur, et `serverInfo.name` n'est pas fiable pour désambiguïser | `SPEC/server/tools.mdx:322-332` |
| `title` | Nom d'affichage optionnel. Précédence d'affichage selon la spec : `title`, puis `annotations.title`, puis `name` (`SCHEMA:2010`) | `SPEC/server/tools.mdx:287` |
| *(écart client)* | VS Code applique l'ordre inverse : `annotations.title || title || name` (`[VS] …ToolContribution.ts:148-149`, `:228-229`). **→ Renseigner les deux avec la même valeur.** | `[VS]` |
| `description` | « hint » pour le modèle | `SCHEMA:1974-1979` |
| `icons` | Optionnel ; `src` en HTTPS ou `data:` ; le client doit rejeter les autres schémas et se méfier des SVG | `SPEC/basic/index.mdx:448-498` ; `SCHEMA:890-927` |
| `inputSchema` | Objet JSON Schema, **`type: "object"` requis à la racine**, tout mot-clé 2020-12 admis ; dialecte 2020-12 par défaut sans `$schema` | `SCHEMA:1981-1997` ; `SPEC/server/tools.mdx:290-296` |
| *(sans paramètre)* | `{ "type": "object", "additionalProperties": false }` **recommandé** | `SPEC/server/tools.mdx:294-296`, `:670-681` |
| `outputSchema` | Optionnel, tout JSON Schema 2020-12 en moderne. En legacy 2025-11-25 : **racine `type: "object"`** obligatoire (`[S] schema/2025-11-25/schema.ts:1279-1284`) | `SCHEMA:1999-2005` |
| `annotations` | Voir §3.4 | `SCHEMA:1912-1954` |
| `_meta` | Clés préfixées (DNS inversé) ; préfixes `…modelcontextprotocol/` et `…mcp/` réservés | `SPEC/basic/index.mdx:322-345` |
| *(champ supprimé)* | `execution` (tasks) n'existe plus en 2026-07-28, détails dans `08` §3.1 M6 | — |
| *(champ spécifique)* | `x-mcp-header` sur une propriété : recopie dans `Mcp-Param-*` en HTTP. Primitives hors `number` seulement, jamais pour des données sensibles | `SPEC/server/tools.mdx:334-402` |

**Règles JSON Schema générales** (`SPEC/basic/index.mdx:247-318`) :

- support de 2020-12 obligatoire (MUST), validation selon le dialecte déclaré, erreur explicite pour un dialecte non supporté (`:289-297`) ;
- **ne jamais déréférencer un `$ref` réseau** par défaut (MUST NOT) (`:299-310`) ;
- borner le coût des combinateurs (`anyOf`, `oneOf`, `allOf`, `if`, `$defs`) (SHOULD) (`:312-318`).

**Contraintes de schéma côté Claude Code** (`[CC-DOC]`, sections « Tool input schemas with a root-level combinator » et « Tools with invalid input schemas ») :

- **pas de `anyOf`, `oneOf` ou `allOf` à la racine** : l'API Claude les refuse et Claude Code aplatit le schéma ou écarte le tool. Les combinateurs imbriqués dans `properties` passent tels quels ;
- noms de propriétés de premier niveau sur 1 à 64 caractères `[A-Za-z0-9_.-]` ;
- schéma valide contre le méta-schéma 2020-12, faute de quoi le tool est exclu.

**Bonnes pratiques de description** (`[API-TOOLS]`, « Best practices for tool definitions ») :

- dire ce que fait le tool, quand l'utiliser et **quand ne pas l'utiliser**, le sens de chaque paramètre et les **limites** (ce que le tool ne renvoie pas) ;
- **au moins 3 à 4 phrases** par tool ;
- regrouper les opérations liées ;
- namespacing par service ;
- réponses à haut signal.

Le champ `input_examples` est propre à l'API Claude et n'existe pas dans le `Tool` MCP (`SCHEMA:1973-2015`). Que Claude Code transpose le mot-clé JSON Schema `examples` est **NON VÉRIFIÉ**. Il faut donc mettre les exemples dans les descriptions de paramètres.

**Limites de longueur dans Claude Code** (`[CC-DOC]`, « Tool search for MCP server authors ») :

- description d'un tool chargé d'emblée et `instructions` serveur : **4 096 caractères** ;
- description d'un tool chargé via *tool search* : 16 384 caractères ;
- la recherche ne lit que la partie sous la limite courte : « put the words Claude would search for near the start » ;
- variable `CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH`.

**État de la référence Python** (`[PY] tests/tools.lock.json`, lu) :

- 8 tools préfixés `fr_`, **sans `title`** ;
- descriptions de premier niveau très courtes : 55 caractères pour `fr_get_deliberation`, 60 pour `fr_search` (« Keyword-search French legislation or case law on Legifrance. »), 81 pour `fr_get_text`. Le détail est dans les descriptions de paramètres ;
- `fr_search.fond` est un `{"type":"string"}` **sans `enum`**, la liste des fonds n'étant donnée qu'en prose ; `page_size` est un `{"type":"integer"}` **sans `minimum`/`maximum`** (aussi relevé par `02-defauts.md:393`) ;
- les `outputSchema` représentent environ 69 % des octets de descripteurs (`02-defauts.md:424`).

### 3.3 Résultat d'un tool : `content`, `structuredContent`, `isError`

**Normatif** (`SCHEMA:1809-1838` ; `SPEC/server/tools.mdx:404-631`) :

- `content: ContentBlock[]` est **requis**. `ContentBlock` = `text | image | audio | resource_link | resource` (`SCHEMA:2305-2306`). Chaque bloc accepte des `annotations` (`audience`, `priority`, `lastModified`) (`SCHEMA:2270-2300` ; `SPEC/server/tools.mdx:410-416`).
- `structuredContent?: unknown` : **toute valeur JSON** en 2026-07-28 (`SCHEMA:1815-1821`), **objet** uniquement en 2025-11-25 (`[S] schema/2025-11-25/schema.ts:1113`).
- « For backwards compatibility, a tool that returns structured content **SHOULD** also return the serialized JSON in a TextContent block. » (`SPEC/server/tools.mdx:500`).
- Avec un `outputSchema`, le serveur **MUST** fournir un `structuredContent` conforme. Le client **SHOULD** le valider (`SPEC/server/tools.mdx:509-515`).
- `isError?: boolean`, faux par défaut (`SCHEMA:1823-1837`).
- `resultType: "complete"` dans tout résultat moderne (`SPEC/basic/index.mdx:73`). Un `tools/call` peut aussi renvoyer un `InputRequiredResult` (MRTR, `SPEC/server/tools.mdx:174-234`), inutile pour nous.

**Comportement réel des clients (déterminant pour l'économie de contexte)** :

| Client | Succès avec `structuredContent` + texte | Erreur (`isError`) | Source |
|---|---|---|---|
| VS Code (`main` 2026-10-15) | Les blocs `text` ne sont **pas** envoyés au modèle si `structuredContent` existe : commentaire « structured content 'represents the result of the tool call', so take that in place of any textual description ». `JSON.stringify(structuredContent)` est ajouté pour l'assistant. Les `resource_link` restent transmis comme références | même code | `[VS] …ToolContribution.ts:328-337`, `:343-363`, `:389-392` |
| VS Code | Un bloc texte avec `annotations.audience` qui exclut `"assistant"` n'est pas envoyé au modèle | — | `[VS] …ToolContribution.ts:328` |
| VS Code | `outputSchema` **absent** des données de tool passées au modèle : seul `inputSchema` figure dans `IToolData` | — | `[VS] …ToolContribution.ts:144-160` (aucune occurrence d'`outputSchema` dans `src/vs/workbench/contrib/mcp`, `grep` exécuté) |
| Claude Code | « On the success path, the client does the opposite. `structuredContent` replaces the content blocks, and the text block is dropped. » Constat reproduit en 2.1.260 | « The error path is different: it only stringifies the text and resource-link blocks from `content` and drops `structuredContent` entirely » (bcherny, mainteneur, 2026-08-15, v2.1.233). Ticket **ouvert** au 2026-10-15 | `[CC-86032]` |
| Claude Code (rapports antérieurs) | « Claude Code (CLI) appears to forward only `structuredContent` to the model », fermé en « not planned » | — | `[CC-55677]` (v2.1.126, 2026-05) ; `[CC-45575]` |
| Claude Code (historique) | Support de `structuredContent` (2.0.21) ; `resource_link` (1.0.44) ; correctif des images perdues quand `structuredContent` et `content` coexistent (2.1.128) | — | `[CC] CHANGELOG.md:8260`, `:8692`, `:5644` |
| SDK Python (client) | Valide `structuredContent` contre `outputSchema` **seulement si `isError` est faux**. `outputSchema` présent sans `structuredContent` → `RuntimeError` | exempté | `[PYSDK] src/mcp/client/session.py:394-419` |
| claude.ai web, Claude Desktop | **NON VÉRIFIÉ**. Un rapporteur affirme que la même réponse « works on Claude.ai web and ChatGPT » (`[CC-55677]`), sans détail | — | — |

La feuille de route officielle reconnaît l'ambiguïté : « a server developer today has no way to know which form a given client will put in front of the model. We aim to make this easier by standardizing on one clear contract. » (`BLOG/2026-08-22-mcp-roadmap.md:65`).

**Conséquences de conception (recommandation)** :

1. Le **contenu destiné au modèle est le `structuredContent`**. Le résumé par défaut, la pagination et les champs de continuation (`truncated`, `next_offset`, `hint`) doivent y figurer, pas seulement dans le texte.
2. Le bloc texte reste requis pour la compatibilité (SHOULD) et pour les clients qui le lisent. Deux options : (a) **le JSON sérialisé**, littéralement ce que demande la spec et ce que voit déjà Claude Code ou VS Code ; (b) un rendu Markdown du **même** contenu, plus lisible et sans échappement JSON des sauts de ligne. Choix à trancher (questions ouvertes). Dans les deux cas, **jamais d'information présente seulement dans le texte**.
3. En **erreur** : tout dans `content[0].text` (code, cause, correctif proposé). Pas de `structuredContent` sur les erreurs, comme dans l'exemple normatif (`SPEC/server/tools.mdx:767-781`) et comme le fait déjà la référence (`01-parite.md:97`).
4. Racine **objet** pour tout `outputSchema` et `structuredContent` : compatibilité 2025-11-25 et piège rmcp `Json<Vec<T>>` (`09-rmcp-2026.md:151`).
5. Les `resource_link` dans `content` disparaissent dans Claude Code dès qu'il y a un `structuredContent`. Il faut donc mettre l'URI de ressource (`legifrance://…`) **dans** `structuredContent` aussi.
6. L'`outputSchema` coûte des octets de descripteur mais, d'après VS Code, n'est pas passé au modèle. Ce qu'en fait Claude Code est **NON VÉRIFIÉ** : l'API Claude ne connaît que `input_schema` (`[API-TOOLS]`). Le garder concis (descriptions de champs brèves, pas de valeurs par défaut verbeuses) reste prudent pour les autres clients.

### 3.4 Annotations (`ToolAnnotations`)

| Hint | Défaut | Sémantique | Source |
|---|---|---|---|
| `title` | — | Nom d'affichage | `SCHEMA:1913-1916` |
| `readOnlyHint` | `false` | « does not modify its environment » | `SCHEMA:1918-1923` |
| `destructiveHint` | `true` | Destructif ou additif ; n'a de sens que si `readOnlyHint == false` | `SCHEMA:1925-1933` |
| `idempotentHint` | `false` | Rappel avec les mêmes arguments sans effet supplémentaire ; n'a de sens que si `readOnlyHint == false` | `SCHEMA:1935-1943` |
| `openWorldHint` | `true` | « may interact with an "open world" of external entities » ; exemples : recherche web = ouvert, outil mémoire = fermé | `SCHEMA:1945-1953` |

- Ce sont des **hints**. Les clients **MUST** les considérer comme non fiables si le serveur ne l'est pas (`SPEC/server/tools.mdx:304-307` ; `SCHEMA:1900-1911`).
- Billet officiel du 2026-03-16 (`BLOG/2026-03-16-tool-annotations.md`) :
  - mise en garde : « An untrusted server can lie » (`:96`) ;
  - recommandation explicite : « set `readOnlyHint: true` on read-only tools, `destructiveHint: false` on additive operations, and `openWorldHint: false` on closed-domain tools » (`:141`) ;
  - `openWorldHint` est « the hint most sensitive to deployment context… treat anything a tool considers **external** as a potential source of untrusted content » (`:36`) ;
  - comportements clients typiques : `readOnlyHint: true` → pas de dialogue de confirmation ; `idempotentHint: true` → rejouable après échec ; `openWorldHint: true` → examiner la sortie (`:114-119`).
- **Effet réel dans VS Code** :
  - `readOnlyHint` faux ou absent → dialogue « Run … » avant exécution (`[VS] …ToolContribution.ts:234-239`, `canRequestPreApproval` `:156`) ;
  - `openWorldHint: true` → `confirm.confirmResults = true` et `canRequestPostApproval` (`:240-242`, `:157`), c'est-à-dire une approbation **après** exécution, sur les résultats. La forme exacte de l'UI est **NON VÉRIFIÉE** (documentation VS Code injoignable).
- **Référence Python** : `READ_ONLY = ToolAnnotations(readOnlyHint=True, idempotentHint=True, destructiveHint=False, openWorldHint=True)` appliqué aux 8 tools (`[PY] src/fr_eli_mcp/server.py:81-86`).
- **Recommandation** : `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true` et `title` (dupliqué dans `annotations.title`) sur tous les tools. `openWorldHint` est **à trancher**. `true` est fidèle à « service externe dont le contenu peut porter du texte non maîtrisé » et à la parité du lock, mais coûte une étape d'approbation des résultats dans VS Code. `false` est fidèle à « corpus fermé et officiel » et suit la recommandation du billet. Voir les questions ouvertes.

### 3.5 Erreurs : protocole contre exécution

| Cas | Mécanisme | Source |
|---|---|---|
| Tool inconnu | Erreur JSON-RPC ; l'exemple utilise `-32602` « Unknown tool: … » | `SPEC/server/tools.mdx:742-758` |
| Requête qui ne satisfait pas le schéma `CallToolRequest` (`name` absent, `arguments` non objet) | Erreur JSON-RPC | `SPEC/server/tools.mdx:744` ; `SCHEMA:1863-1872` (`arguments?: {[k]: unknown}`) |
| Erreur serveur exceptionnelle | Erreur JSON-RPC (`-32603`) | `SPEC/server/tools.mdx:745` ; `SCHEMA:1833-1836` |
| Échec d'API amont, **validation d'entrée** (format de date, plage), logique métier | `result.isError = true` avec un message exploitable | `SPEC/server/tools.mdx:760-782` ; `SCHEMA:1828-1832` |
| Le client SHOULD transmettre les erreurs d'exécution au modèle ; MAY pour les erreurs protocole (« less likely to result in successful recovery ») | — | `SPEC/server/tools.mdx:784-785` |

- Comme `arguments` est un objet libre dans le schéma, une **erreur de type ou de valeur sur un argument** relève de la validation d'entrée, donc d'`isError`, et non d'une erreur protocole. C'est la règle SEP-1303 depuis 2025-11-25 (`[S] docs/specification/2025-11-25/changelog.mdx:28`). Avec rmcp, un échec de désérialisation de `Parameters<T>` produit une erreur JSON-RPC (**NON VÉRIFIÉ ici**, `01-parite.md:776`). Il faut donc un adaptateur qui convertit cet échec en `isError` avec `[invalid_arg]`.
- **Codes JSON-RPC** :
  - standards : `-32700` et `-32600` à `-32603` ;
  - `-32000` à `-32019` : plage *legacy*, à ne plus utiliser (SHOULD NOT) ;
  - `-32020` à `-32099` : réservée à la spec, ne jamais émettre un code non défini (MUST NOT) ;
  - codes applicatifs : hors de `-32768` à `-32000` (SHOULD) ;
  - `-32002` et `-32042` interdits en moderne (`SPEC/basic/index.mdx:109-155`).
  - **Recommandation** : aucun code JSON-RPC maison. Nos codes métier (`invalid_arg`, `not_found`, `upstream_error`, `config_error`, plus `rate_limited` éventuellement) vivent **dans le texte** des résultats `isError` (préfixe `[code]`, comme la référence : `[PY] src/fr_eli_mcp/server.py:53-59`, `:69-78`).
- **Format de message actionnable** (`[A-TOOLS]` : « clearly communicate specific and actionable improvements, rather than opaque error codes or tracebacks » ; exemple normatif « Invalid departure date: must be in the future. Current date is 08/08/2025. », `SPEC/server/tools.mdx:776`). Gabarit proposé :
  `[invalid_arg] article_id="LEGITEXT000006070721" est un identifiant de TEXTE, pas d'article. Pour un texte : fr_get_act(text_id=…). Pour un article : utilisez un id LEGIARTI… (voir fr_get_act → articles[].article_id).`
- **Erreurs amont** : distinguer 404 (`not_found`), 429/quota (`rate_limited`, avec un délai conseillé si l'amont le donne), 5xx/timeout (`upstream_error`, « réessayer une fois ») et identifiants manquants (`config_error`, en nommant les variables d'environnement **sans jamais en afficher la valeur**).

### 3.6 Tools « à état » et handles

- Section non normative (`SPEC/server/tools.mdx:683-736`) :
  - pas de session protocolaire : un état qui couvre plusieurs appels passe par un **handle explicite** renvoyé puis repassé en argument ;
  - handles **opaques**, à durée de vie annoncée dans la description, et erreur d'exécution claire à expiration ;
  - sans authentification, le handle est un bearer : entropie suffisante (UUIDv4) et durée bornée (`:723-727`).
- Sécurité : possession d'un handle ≠ authentification (MUST NOT). Handles non devinables et liés au principal côté serveur (SHOULD) (`DOCS/tutorials/security/security_best_practices.mdx:516-553`).
- **Recommandation** : éviter tout état serveur. La pagination de recherche se fait par `page` ou `offset` explicites, rejouables, et la lecture longue par `offset`/`max_chars`. Aucun handle ne doit survivre à un redémarrage.

### 3.7 Exigences de sécurité propres aux tools

- Serveur **MUST** : valider toutes les entrées, contrôler les accès, **limiter le débit** des invocations, **assainir les sorties** (`SPEC/server/tools.mdx:789-793`).
- Client SHOULD : confirmations, affichage des entrées, validation des résultats, timeouts, journal d'audit (`:795-803`). C'est hors de notre contrôle, mais cela explique pourquoi les annotations comptent.

---

## 4. Resources, templates, resource links, prompts, completion, utilitaires

### 4.1 Resources

- **Modèle d'interaction** : *application-driven*. L'hôte décide de l'inclusion (sélecteur, recherche, heuristique) (`SPEC/server/resources.mdx:24-37` ; `DOCS/learn/server-concepts.mdx:14-18`, « Who controls it : Application »).
- **Capacité** `resources{subscribe?, listChanged?}`. `{}` si aucune des deux (`SPEC/server/resources.mdx:41-72`). Liste qui ne varie pas par connexion (MUST NOT), mais peut varier selon l'autorisation (MAY) (`:74-81`).
- **`resources/list`** paginé et cachable. **`resources/read`** cachable, peut renvoyer plusieurs `contents` (`:85-177`). `ReadResourceResult extends CacheableResult` (`SCHEMA:1229`).
- **Types** :
  - `Resource{uri, name, title?, description?, mimeType?, annotations?, size?}` (`SCHEMA:1441-1474`) ; `size` permet d'« estimate context window usage » (`:1466-1471`) ;
  - contenus `TextResourceContents{uri, mimeType?, text}` ou `BlobResourceContents{…, blob}` en base64 (`SCHEMA:1514-1555`).
- **Annotations** `audience` (`user`/`assistant`), `priority` (0 à 1), `lastModified` (ISO 8601) (`SPEC/server/resources.mdx:336-364`). `lastModified` est utile pour la date de version d'un article.
- **Schémas d'URI** :
  - `https://` seulement si le client peut récupérer la ressource lui-même ; sinon schéma custom (SHOULD) (`:371-381`) ;
  - un schéma custom **MUST** respecter la RFC 3986 (`:397-400`).
  - **→ `legifrance://…`** pour les ressources. `source_url` en `https://www.legifrance.gouv.fr/...` reste un champ de donnée.
- **Erreurs** :
  - ressource inexistante → **`-32602`** avec `data.uri` (MUST) ; erreur interne → `-32603` (SHOULD) ;
  - le client SHOULD encore accepter `-32002` ;
  - **jamais `contents: []`** pour une ressource inexistante (MUST NOT) (`:402-426`).
  - En session legacy, utiliser `-32002` (voir `08` §15.1).
- **Sécurité** : valider **toutes** les URI (MUST), encoder correctement le binaire (MUST), contrôles d'accès (SHOULD) (`:428-435`).
- **Abonnements** : `notifications/resources/updated` via `subscriptions/listen` avec `resourceSubscriptions` (`:244-264`). Sans objet pour nous : on ne déclare pas `subscribe`.
- **Côté Claude Code** :
  - les ressources se citent par `@serveur:protocole://chemin` et sont jointes automatiquement ;
  - « Claude Code automatically provides tools to list and read MCP resources when servers support them » (`[CC-DOC]`, « Use MCP resources ») ;
  - le support des **templates** dans l'autocomplétion `@` est **NON VÉRIFIÉ**.

**Recommandation** : `resources/list` vide, ou limité à quelques entrées statiques utiles (par exemple `legifrance://coverage`, `legifrance://fonds`). Il est impossible d'énumérer le corpus. L'accès passe par les templates et les `resource_link`.

### 4.2 Resource templates (RFC 6570)

- `resources/templates/list` : templates d'URI RFC 6570 avec `name`, `title`, `description`, `mimeType` (seulement si commun à toutes les ressources du template) et `annotations`. Paginé et cachable (`SPEC/server/resources.mdx:181-230` ; `SCHEMA:1481-1507`). Arguments complétables via `completion/complete` (`ref/resource`) (`SPEC/server/resources.mdx:183-186`).
- **RFC 6570** : https://datatracker.ietf.org/doc/html/rfc6570 (site injoignable depuis l'environnement). Le détail des niveaux (niveau 1 `{var}`, niveaux supérieurs avec opérateurs `+ # . / ; ? &` et modificateurs) et les limites de l'usage « inverse » (correspondance URI → variables) sont **NON VÉRIFIÉS** ici.
- **Recommandation** : n'utiliser que des expressions simples (`{var}`) et au besoin une requête de forme `{?date}`. Le serveur fait lui-même la correspondance inverse avec une grammaire stricte (préfixe d'identifiant, caractères permis) plutôt qu'avec un moteur générique.
- Exemples (proposition) : `legifrance://article/{article_id}`, `legifrance://texte/{text_id}{?date}`, `legifrance://decision/{decision_id}`, `legifrance://code/{code_id}`.
- Crates Rust candidates : `iri-string` (0.7.14, `rust_version` 1.60) et `uritemplate-next` (0.2.0) existent sur l'index crates.io (`curl https://index.crates.io/...`, 2026-10-15). Leur support de RFC 6570 est **NON VÉRIFIÉ**. Le rapport rmcp montre `ResourceTemplate::new("legifrance://article/{id}", …)` (`09-rmcp-2026.md:394`).

### 4.3 Resource links et ressources embarquées dans les résultats d'outils

- Un tool **MAY** renvoyer des `resource_link` (`uri`, `name`, `description?`, `mimeType?`, annotations). Ils ne figurent **pas forcément** dans `resources/list` (`SPEC/server/tools.mdx:451-471` ; `SCHEMA:1709-1721`).
- Ressource embarquée (`type: "resource"`) : le serveur qui l'utilise SHOULD implémenter la capacité `resources` (`SPEC/server/tools.mdx:473-494`).
- **Intérêt pour le contexte** : un lien vers le texte complet évite de l'injecter. Mais dans Claude Code, les blocs `content` (dont `resource_link`) sont remplacés par `structuredContent` en cas de succès (§3.3). D'où la règle : **dupliquer l'URI dans `structuredContent`** (par exemple `"resource_uri": "legifrance://article/LEGIARTI…"`) et faire en sorte que `resources/read` résolve toute URI ainsi émise.

### 4.4 Prompts

- **Modèle** : *user-controlled*, par exemple en commandes slash (`SPEC/server/prompts.mdx:23-37`). Capacité `prompts{listChanged?}` déclarée dans `DiscoverResult` (`:41-55`). Liste stable par connexion (`:57-64`). `prompts/list` paginé et cachable (`:68-120`).
- **Types** :
  - `Prompt{name, title?, description?, icons?, arguments?: PromptArgument[]}` et `PromptArgument{name, title?, description?, required?}` (`SCHEMA:1659-1687`) ;
  - `prompts/get` : `arguments` de type `{[k]: string}` (**chaînes uniquement**) (`SCHEMA:1602-1611`) ; résultat `{description?, messages: PromptMessage[]}`, **non cachable** (`SCHEMA:1634-1640`) ;
  - `PromptMessage{role, content: ContentBlock}` peut contenir `resource_link` et des ressources embarquées (`SPEC/server/prompts.mdx:218-317`).
- **Erreurs** : nom invalide ou argument requis manquant → `-32602` ; erreur interne → `-32603` (`:319-325`). Valider les arguments (SHOULD) ; entrées et sorties validées contre l'injection (MUST) (`:327-336`).
- **Côté Claude Code** :
  - prompts exposés en `/serveur:prompt (MCP)` ou `/mcp__serveur__prompt` ;
  - « Claude Code splits the arguments on whitespace, so each argument is a single token » (`[CC-DOC]`, « Use MCP prompts as commands »).
  - **→ Arguments de prompt mono-jeton** (identifiants, codes de fonds), pas de texte libre multi-mots.
- **Exemples** (proposition) : `fr_analyser_article {article_id}`, `fr_verifier_citations` (sans argument : le texte à vérifier suit dans la conversation), `fr_jurisprudence_article {article_id}`.

### 4.5 Completion (`completion/complete`)

- Capacité `completions: {}` (`SPEC/server/utilities/completion.mdx:33-43`). Références `ref/prompt` (par `name`) et `ref/resource` (par `uri` ou template) (`:133-140` ; `SCHEMA:2680-2697`). `context.arguments` transmet les arguments déjà résolus (`:89-114` ; `SCHEMA:2609-2617`).
- Résultat : au plus **100 valeurs** (MUST, `@maxItems 100`), `total?`, `hasMore?`, triées par pertinence (`SCHEMA:2644-2661` ; `SPEC/server/utilities/completion.mdx:142-147`, `:194-200`).
- Erreurs `-32601`, `-32602`, `-32603` (`:185-192`). **MUST** : valider les entrées, **limiter le débit**, contrôler l'accès aux suggestions sensibles, empêcher la divulgation par complétion (`:207-214`).
- **Recommandation** : complétions **statiques et locales**, sans appel PISTE (débit et latence) :
  - valeurs de `fond` (`LODA`, `CODE`, `JURI`, `CONSTIT`, `CETAT`, `CNIL`, `KALI`, `ACCO`, d'après `[PY] src/fr_eli_mcp/server.py:91-100`) ;
  - table « nom de code → `LEGITEXT…` » embarquée. La source de cette table relève de l'axe données et est **NON VÉRIFIÉE** ici.

### 4.6 Logging MCP contre stderr

- Logging MCP **déprécié** depuis 2026-07-28 (SEP-2577). Nouvelles implémentations : SHOULD NOT l'adopter ; logger sur `stderr` (stdio) ou via OpenTelemetry (`SPEC/server/utilities/logging.mdx:7-18` ; `SPEC/deprecated.mdx:28`).
- Si on le gardait :
  - capacité `logging` (MUST) ;
  - `notifications/message` **seulement** si la requête porte `_meta["io.modelcontextprotocol/logLevel"]` (MUST NOT sinon) et **uniquement** sur le flux de cette requête (`:30-72`) ;
  - jamais de secrets, de PII ni de détails internes (MUST NOT) (`:121-132`).
- stdio : `stderr` libre ; le client ne doit pas l'interpréter comme une erreur (`SPEC/basic/transports/stdio.mdx:14-17`). Guide officiel : « Never write to stdout… » (`DOCS/develop/build-server.mdx:44-68`).
- **Décision recommandée** (alignée sur la brief) : pas de capacité `logging` ; `tracing` vers stderr ; ne jamais journaliser les secrets PISTE ni le jeton (également `SPEC/basic/authorization/security-considerations.mdx:27-31` : « tokens cached or logged on the server »).

### 4.7 Progress

- Opt-in par `_meta.progressToken` (string ou entier, unique parmi les requêtes actives). Notification `notifications/progress{progressToken, progress, total?, message?}`. `progress` **MUST** croître, tokens de requêtes actives uniquement, arrêt à la complétion (MUST). Le serveur **MAY** n'en envoyer aucune (`SPEC/basic/patterns/progress.mdx:13-90`).
- Dans Claude Code, le timeout par serveur est un mur d'horloge que la progression ne prolonge pas (`[CC-DOC]`, « Push messages with channels », paragraphes sur `timeout`).
- **Usage pertinent** : `fr_search_all` (recherche parallèle multi-fonds), avec une notification par fonds terminé, et seulement si un `progressToken` est fourni. Facultatif.

### 4.8 Cancellation

- stdio : le client **MUST** envoyer `notifications/cancelled{requestId, reason?}`. HTTP : fermer le flux SSE vaut annulation (MUST) (`SPEC/basic/patterns/cancellation.mdx:35-43`).
- Le serveur SHOULD arrêter le travail et libérer les ressources ; en stdio il **MUST NOT** plus rien émettre pour cette requête (`SPEC/basic/transports/stdio.mdx:76-83`). Il MAY ignorer une annulation inconnue ou tardive (`SPEC/basic/patterns/cancellation.mdx:66-82`) et doit gérer les courses (`:84-104`). Annulations invalides ignorées (`:111-120`).
- Le serveur n'envoie `notifications/cancelled` **que** pour clore un `subscriptions/listen` (`:11-14`).
- **Recommandation** : chaque `tools/call` tourne dans une tâche annulable (`CancellationToken`) qui interrompt aussi l'appel `reqwest` amont et les sous-tâches de `fr_search_all`.

### 4.9 Elicitation, sampling, roots : pertinence

| Fonction | Statut 2026-07-28 | Pertinence pour nous | Source |
|---|---|---|---|
| Elicitation (via MRTR `InputRequiredResult`) | Actif ; mode formulaire et mode URL | Faible en v1 : désambiguïsation possible (homonymies), mais il vaut mieux renvoyer les candidats au modèle. **Interdit** de demander un secret (client_secret PISTE) en mode formulaire ; mode URL obligatoire pour les secrets | `SPEC/client/elicitation.mdx:12-15`, `:26-47` ; `08` §8 |
| Sampling | **Déprécié** (SEP-2577) : « integrate directly with LLM provider APIs » | Non : le « résumé par défaut » doit être **déterministe et côté serveur** (extraction des métadonnées, du dispositif, des premiers paragraphes), sans appel LLM | `SPEC/client/sampling.mdx:7-17` ; `SPEC/deprecated.mdx:27` |
| Roots | **Déprécié** | Non (pas de fichiers locaux) | `SPEC/deprecated.mdx:26` |
| Tasks (extension) | Hors du cœur | Non (appels courts) | `SPEC/changelog.mdx:22` |

Claude Code déclare `elicitation: {form: {}, url: {}}` sur les connexions 2026-07-28 (`[CC-DOC]`, « Respond to MCP elicitation requests »). Il n'y a donc aucun obstacle si l'on en a besoin plus tard.

---

## 5. Transports

### 5.1 stdio (phase 1)

Règles (`SPEC/basic/transports/stdio.mdx`) :

- Le serveur lit JSON-RPC sur `stdin` et écrit JSON-RPC sur `stdout`. **Un message par ligne, sans saut de ligne interne** (MUST NOT) (`:10-13`).
- **Rien d'autre que des messages MCP valides sur `stdout`** (MUST NOT) ; `stderr` libre pour les logs (`:14-19`).
- Le serveur **MUST NOT** écrire de *requêtes* JSON-RPC (`:55`). Il écrit seulement des réponses, des notifications liées à une requête en cours, et des notifications de `subscriptions/listen` avec `subscriptionId` (`:39-58`).
- Métadonnées entièrement dans le corps (`_meta`), sans couche d'en-têtes (`:65-72`).
- **Arrêt** : sortie rapide sur EOF de stdin (SHOULD), puis SIGTERM et SIGKILL côté client (`:87-107`). Après un crash, le client relance ; les requêtes en vol sont perdues et rejouables (`:109-115`).
- **Rétrocompatibilité** : le client dual-era sonde avec `server/discover`. Le repli sur `initialize` se fait sur toute erreur non moderne ou en cas de silence (`:121-147`). Notre serveur doit répondre vite à `server/discover`, sans réseau, et ne jamais quitter sur une méthode inattendue (`08` §5.1).
- Framing réutilisable sur UDS ou TCP (SHOULD) (`:23-31` ; `SPEC/basic/transports/index.mdx:60-79`).
- Messages JSON-RPC encodés en **UTF-8** (MUST) (`SPEC/basic/transports/index.mdx:29-30`).
- **Authentification** : en stdio, l'autorisation MCP SHOULD NOT s'appliquer ; les identifiants viennent **de l'environnement** (`SPEC/basic/index.mdx:221-226` ; `SPEC/basic/authorization/index.mdx:18-22`). La référence lit `FR_ELI_CLIENT_ID` et `FR_ELI_CLIENT_SECRET` (`[PY] src/fr_eli_mcp/client.py:10`, `:51-55`).
- **Modèle de confiance stdio** : client et serveur partagent un même domaine de confiance. « The stdio transport is not a sandbox. » L'isolation relève de l'hôte (`DOCS/tutorials/security/local-server-security.mdx:54-73`).

### 5.2 Streamable HTTP (phase 2, optionnelle)

Synthèse. Le tableau complet des statuts et en-têtes figure dans `08` §5.2.

- Un seul endpoint **POST** ; en 2026-07-28, **plus de GET ni de session** (`SPEC/basic/transports/streamable-http.mdx:14-25`, `:47-49`). Chaque message client est un POST. Une notification acceptée reçoit **202** sans corps (`:70-91`). Pour une requête, réponse `application/json` ou `text/event-stream` (`:88-91`).
- **Sécurité de l'endpoint** (`:54-68`) :
  - **MUST valider `Origin`** contre le DNS rebinding : `Origin` présent et invalide → **403** ;
  - en local, SHOULD n'écouter que sur `127.0.0.1` ;
  - SHOULD authentifier toutes les connexions.
  - Le guide local ajoute : « Binding to 127.0.0.1 is not an authentication boundary » (`DOCS/tutorials/security/local-server-security.mdx:320-327`).
  - Les mêmes règles `Origin`/localhost existaient en 2025-11-25 (`[S] docs/specification/2025-11-25/basic/transports.mdx:78-81`).
- **SSE** : notifications liées à la requête puis réponse finale ; jamais de requête ; `X-Accel-Buffering: no` (SHOULD) ; keep-alive `:` ; **pas de reprise** `Last-Event-ID` (`:107-157`). Fermeture du flux = annulation (MUST) (`:233-240`).
- **En-têtes** :
  - `MCP-Protocol-Version` (MUST, égal au `_meta`) ;
  - `Mcp-Method` (toutes requêtes) et `Mcp-Name` (`tools/call`, `resources/read`, `prompts/get`) **REQUIRED** ;
  - incohérence → 400 + `-32020` ; version non supportée → 400 + `-32022` ; méthode inconnue → 404 + `-32601` (`:250-297`, `:580-631`).
- **Sessions (legacy 2025-11-25)** : l'attribution d'un `Mcp-Session-Id` y était **facultative** (« MAY assign a session ID ») (`[S] docs/specification/2025-11-25/basic/transports.mdx:198`). Un serveur modern-only répond 405 aux GET/DELETE et ignore `Mcp-Session-Id` et `Last-Event-ID` (`SPEC/basic/transports/streamable-http.mdx:680-687`). Stratégie dual-era HTTP : `08` §15.2.
- HTTP+SSE (2024-11-05) : déprécié, SHOULD NOT l'adopter (`:695-708`).

### 5.3 Autorisation OAuth 2.1 côté serveur (si le mode HTTP est protégé)

- L'autorisation est **OPTIONNELLE**. En HTTP elle SHOULD suivre la spec (`SPEC/basic/authorization/index.mdx:18-24`). Le serveur MCP protégé est un **resource server OAuth 2.1** (`:49-50`).
- **MUST** :
  - implémenter la *Protected Resource Metadata* RFC 9728 avec `authorization_servers` non vide (`:73-74` ; `authorization-server-discovery.mdx:13-15`) ;
  - l'exposer via `WWW-Authenticate: Bearer resource_metadata=…` sur 401 **ou** via `/.well-known/oauth-protected-resource[/chemin]` (`authorization-server-discovery.mdx:39-47`).
- `scope` dans le challenge (SHOULD) (`index.mdx:99-114`).
- **Validation du jeton** (MUST) : OAuth 2.1 §5.2 et **audience** RFC 8707 ; jeton invalide ou expiré → **401** (`:281-290`). « MCP servers **MUST** only accept tokens that are valid for use with their own resources. MCP servers **MUST NOT** accept or transit any other tokens. » (`:294-297`).
- Autorisation **dans chaque requête HTTP** (MUST), jamais dans la query string (`:256-271`).
- **Statuts** : 401 non autorisé, 403 scopes insuffisants, 400 requête mal formée (`:315-323`). Scope insuffisant à l'exécution : 403 + `WWW-Authenticate: Bearer error="insufficient_scope", scope=…, resource_metadata=…`, avec **tous** les scopes requis en un seul challenge (SHOULD) (`:332-377`). Tenir compte des hiérarchies de scopes (MUST) (`:404-405`). Pas d'`offline_access` dans le challenge ni dans `scopes_supported` (SHOULD NOT) (`:311-313`).
- **Interdiction du token passthrough** :
  - « If the MCP server makes requests to upstream APIs, it may act as an OAuth client to them. The access token used at the upstream API is a separate token… The MCP server **MUST NOT** pass through the token it received from the MCP client. » (`SPEC/basic/authorization/security-considerations.mdx:116-126`) ;
  - risques détaillés : contournement des contrôles, audit, frontières de confiance (`DOCS/tutorials/security/security_best_practices.mdx:278-347`).
  - **C'est notre modèle** : le jeton PISTE est obtenu par le serveur (client credentials) et reste interne à l'adapter.
- `cacheScope: "private"` si les listes dépendent des scopes ; ne jamais s'en remettre à `cacheScope` pour l'accès (MUST) (`SPEC/server/utilities/caching.mdx:101-108`, `:171-179`).
- Pour un serveur local, préférer stdio ; en HTTP, exiger un jeton ou utiliser un UDS restreint (SHOULD) (`DOCS/tutorials/security/security_best_practices.mdx:643-651`).

---

## 6. Sécurité : « Security Best Practices » applicables à notre serveur

Document : `DOCS/tutorials/security/security_best_practices.mdx` (984 lignes), complété par `DOCS/tutorials/security/local-server-security.mdx` (453 lignes) et `SPEC/basic/authorization/security-considerations.mdx`.

| Menace (section) | Applicabilité | Mesure pour le serveur Légifrance | Source |
|---|---|---|---|
| Confused deputy (proxy OAuth à client_id statique) | Seulement si un jour on fait de l'OAuth **utilisateur** relayé vers un AS tiers. PISTE fonctionne en client credentials côté serveur | Ne pas devenir un proxy OAuth ; sinon, consentement par client (MUST) | `security_best_practices.mdx:27-277` ; `security-considerations.mdx:107-114` |
| **Token passthrough** | HTTP phase 2 | Jamais relayer le jeton MCP vers PISTE ; valider l'audience des jetons entrants | `security_best_practices.mdx:278-347` ; `security-considerations.mdx:116-126` |
| SSRF | Vise surtout les clients et les AS. Principe transposable : notre serveur ne doit **jamais** suivre une URL fournie en argument (par exemple un `source_url`) | Hôtes amont en liste blanche (domaines PISTE), redirections non suivies hors liste blanche. La référence lit `FR_ELI_BASE_URL`, `FR_ELI_OAUTH_URL` et un **manifeste distant** qui peut changer `base_url` vers lequel part le Bearer (`02-defauts.md:123-131`) : **à ne pas reproduire** | `security_best_practices.mdx:349-514` ; `[PY] src/fr_eli_mcp/client.py:43-47`, `src/fr_eli_mcp/runtime.py:22-25` |
| State handle hijacking | Seulement si l'on introduit des handles | Ne pas en introduire (pagination par offset) ; sinon handles aléatoires, expirants et liés au principal | `security_best_practices.mdx:516-557` |
| Compromission d'un serveur local | Notre binaire **est** un serveur local | stdio par défaut ; HTTP local seulement avec jeton ou UDS ; binaire signé, version épinglée, SBOM | `security_best_practices.mdx:559-651` ; `local-server-security.mdx:150-190` |
| Catalogue d'outils empoisonné / **rug pull** | Nos descriptions **sont** des instructions lues par le modèle | Descriptions strictement fonctionnelles (pas de directive visant d'autres serveurs) ; descripteurs stables et verrouillés (§8) | `local-server-security.mdx:92-100`, `:192-214` |
| Validation de l'URL d'autorisation, sécurité stdio dans les proxys | Côté client ou proxy | — | `security_best_practices.mdx:653-795` |
| Minimisation des scopes | HTTP avec AS | Scope de base minimal (lecture), pas de scope fourre-tout | `security_best_practices.mdx:874-984` |
| Vol de jetons | Jeton PISTE en mémoire, cache disque éventuel | `secrecy::SecretString`, jamais dans les logs ni dans l'audit ; cache de jeton en mémoire seulement (recommandation) | `security-considerations.mdx:25-34` |
| Isolation des identifiants | Variables d'environnement par serveur | Documenter `environmentVariables` avec `isSecret` dans `server.json` (§9) ; `sensitive: true` dans un manifeste MCPB (`[B] MANIFEST.md:58-67`, `:572`) | `local-server-security.mdx:246-274` |
| Contrôle de l'egress | Le serveur ne parle qu'à PISTE (et plus tard aux dumps DILA) | Documenter la liste exhaustive des hôtes contactés ; aucune télémétrie | `local-server-security.mdx:303-334` |
| Entrées et sorties d'outils | Toujours | Validation stricte des identifiants (grammaire de préfixe et de caractères), débit limité, sorties assainies (HTML amont → texte) | `SPEC/server/tools.mdx:789-793` ; `SPEC/server/resources.mdx:430` |
| Données personnelles dans l'audit | Les requêtes de recherche peuvent contenir des noms de personnes | Hacher les entrées comme la référence (`hash_input`) plutôt que les stocker en clair | `[PY] src/fr_eli_mcp/server.py:201` ; `SPEC/server/utilities/logging.mdx:123-126` (principe) |
| Icônes | Si on en publie | `data:` PNG ou HTTPS de même origine ; éviter le SVG | `SPEC/basic/index.mdx:473-487` |
| DoS par schémas | Nos schémas sont statiques | Pas de `$ref` externe, profondeur faible | `SPEC/basic/index.mdx:299-318` |

---

## 7. Conception d'outils économes en contexte

### 7.1 Principes sourcés

| Principe | Énoncé | Source |
|---|---|---|
| Moins d'outils, mieux ciblés | « More tools don't always lead to better outcomes » ; construire quelques outils pour des workflows à fort impact ; `search_contacts` plutôt que `list_contacts` ; outils qui consolident plusieurs étapes (`schedule_event`) ; `search_logs` qui ne renvoie que les lignes pertinentes et leur contexte ; les outils qui se recouvrent distraient l'agent | `[A-TOOLS]` |
| Consolider | « Consolidate related operations into fewer tools… with an `action` parameter. Fewer, more capable tools reduce selection ambiguity » | `[API-TOOLS]` |
| Namespacing | Préfixes par service ou par ressource (`asana_search`, `asana_projects_search`). Préfixe ou suffixe : effets « non-trivial » qui varient selon le modèle, à évaluer | `[A-TOOLS]` ; `[API-TOOLS]` (« especially important when using tool search ») |
| Paramètres non ambigus | `user_id` plutôt que `user` ; modèles de données stricts | `[A-TOOLS]` |
| Haut signal | Supprimer les identifiants de bas niveau inutiles ; préférer des identifiants sémantiques. Le remplacement d'UUID par des noms « significantly improves Claude's precision in retrieval tasks by reducing hallucinations » | `[A-TOOLS]` ; `[API-TOOLS]` (« semantic, stable identifiers… include only the fields Claude needs ») |
| Concis contre détaillé | Enum `response_format` (`"concise"` / `"detailed"`). Exemple : **206 tokens en détaillé contre 72 en concis** ; le détaillé garde les identifiants nécessaires aux appels suivants | `[A-TOOLS]` |
| Pagination, filtrage, troncature | « pagination, range selection, filtering, and/or truncation with sensible default parameter values » ; Claude Code limite les réponses d'outils à **25 000 tokens** par défaut ; en cas de troncature, guider l'agent (encourager des recherches petites et ciblées) | `[A-TOOLS]` ; `[CC-DOC]` |
| Erreurs actionnables | Pas de codes opaques ni de tracebacks : dire quoi corriger | `[A-TOOLS]` ; `SPEC/server/tools.mdx:760-782` |
| Descriptions soignées | Décrire comme pour un nouvel arrivant (terminologie, relations entre ressources) ; de petits changements de description ont de grands effets mesurés | `[A-TOOLS]` ; `[API-TOOLS]` (« at least 3–4 sentences ») |
| Format de sortie | XML, JSON ou Markdown : dépend de la tâche, à évaluer | `[A-TOOLS]` |
| Évaluation | Tâches réalistes multi-appels, vérificateurs tolérants, métriques (tokens, nombre d'appels, erreurs), lecture des transcripts, jeu de test réservé | `[A-TOOLS]` |
| Instructions serveur | Pour les relations entre outils, les workflows et les contraintes, **sans dupliquer** les descriptions ; concises et factuelles ; pas pour des garanties de sécurité | `SCHEMA:688-696` ; `BLOG/2025-11-03-using-server-instructions.md:45-53`, `:110-186` |
| Tool search | Seuls les noms de tools et les `instructions` serveur sont chargés au démarrage ; les `instructions` aident Claude à savoir **quand chercher** nos outils | `[CC-DOC]`, « Scale with MCP tool search » |
| Côté client | Les définitions d'outils peuvent consommer la majorité du contexte ; découverte progressive au-delà d'un seuil (1 à 5 % du contexte) | `DOCS/develop/clients/client-best-practices.mdx:6-31` |

### 7.2 Seuils concrets côté Claude Code (`[CC-DOC]`)

| Seuil | Valeur | Conséquence pour nous |
|---|---|---|
| Avertissement de sortie | > 10 000 tokens (fixe) | Rester très en dessous par défaut |
| Limite de sortie | 25 000 tokens (`MAX_MCP_OUTPUT_TOKENS`) | Au-delà, le résultat est enregistré dans un fichier |
| Persistance sur disque | Résultat texte > **50 000 caractères**, sauf `_meta["anthropic/maxResultSizeChars"]` (plafond **500 000**) | Le `max_chars` maximal d'une page doit rester sous 50 000. Un changelog signale qu'un JSON enregistré sur **une seule longue ligne** ne se paginait pas avec Read (`[CC] CHANGELOG.md:1020`) |
| Instructions serveur | 4 096 caractères | Python à 4 470 caractères, donc tronqué |
| Description d'outil chargé d'emblée / via tool search | 4 096 / 16 384 caractères | Mots-clés et règles critiques en tête |
| Nom complet de tool | 128 caractères | `mcp__<serveur>__fr_…`, sans problème avec des noms courts |
| Propriétés de premier niveau | 1 à 64 caractères `[A-Za-z0-9_.-]` | snake_case court |
| `_meta["anthropic/alwaysLoad"]` | Le tool est chargé d'emblée (hors tool search) | Option pour `fr_search` uniquement (questions ouvertes) |
| `_meta["anthropic/requiresUserInteraction"]` | Confirmation à chaque appel | Sans objet (lecture seule) |

Ces clés `_meta` `anthropic/*` sont propriétaires et ignorées par les autres clients (`SPEC/basic/index.mdx:322-339`, préfixes).

### 7.3 Application au serveur Légifrance (recommandations)

**a. Catalogue et granularité.**

- Parité v1 avec les 8 outils Python (`fr_search`, `fr_get_act`, `fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_get_company_agreement`, `fr_coverage` ; `[PY] tests/tools.lock.json`), puis ajouts :
  - `fr_resolve` (citation en texte → identifiant) ;
  - `fr_verify_citations` (lot de citations → statut vérifié, introuvable ou ambigu, avec identifiant et `source_url`), qui consolide un workflow complet en un appel, comme le recommande `[A-TOOLS]` ;
  - `fr_diff_article` (versions d'un article).
- Liens entre décisions : de préférence un champ d'un résultat existant plutôt qu'un nouvel outil.
- `fr_search_all` recouvre `fr_search` (`fond`) : risque de « distraction » (`[A-TOOLS]`). Variante à évaluer : paramètre optionnel `fonds: [...]` sur `fr_search` (questions ouvertes).
- Cible : **au plus 12 outils**. Une consolidation des `fr_get_*` en un seul `fr_get(id)` (aiguillage par préfixe d'identifiant) réduirait le catalogue mais casserait la parité des descripteurs (questions ouvertes).

**b. Namespacing.** Garder le préfixe `fr_` : il porte déjà la juridiction et Claude Code préfixe en plus par `mcp__<serveur>__`. Snake_case ASCII sans point (§3.2).

**c. Paramètres.**

- Noms explicites (`article_id`, `text_id`, `decision_id`, pas `id`).
- **`enum`** pour `fond` et **`minimum`/`maximum`** pour `page_size`, dans le schéma et pas seulement en prose (correctif de `02-defauts.md:393`).
- Format de date dans le schéma (`format: "date"`, `pattern`) et rappelé dans la description.
- `additionalProperties: false` partout, comme la référence.

**d. Retour progressif** (proposition, à faire figurer dans `outputSchema` et dans `structuredContent`) :

- `detail` : `"summary"` (défaut) ou `"full"`. Le résumé contient les métadonnées, le contrat de citation **une seule fois**, le chapeau, le dispositif ou le début, la table des matières courte.
- `offset` (≥ 0) et `max_chars` (défaut de l'ordre de 8 000, borne haute < 50 000) pour `full`. Coupe sur une frontière de paragraphe.
- `focus` (texte) : renvoie les *k* passages les mieux classés (BM25), avec leur `offset`, pour relire autour.
- Champs de continuation **dans `structuredContent`** : `total_chars`, `returned_chars`, `truncated`, `next_offset`, et `hint` en clair, par exemple « Texte tronqué (8 000/57 312 caractères). Pour la suite : même appel avec offset=8000, ou focus="…" pour cibler un passage. » (`[A-TOOLS]` : guider l'agent).
- Ne pas répéter `dataset_note` et `eli_note` dans chaque réponse (respectivement 335 et 171 caractères, `02-defauts.md:387`). Ces notes vont dans `instructions` et `fr_coverage`.
- Résultats de recherche concis : `id`, titre court, date, juridiction ou nature, `human_readable_citation`, `source_url` ; `total`, `page`, et un `hint` de pagination.

**e. Instructions serveur** (dans `DiscoverResult.instructions` et `InitializeResult.instructions`) :

- moins de **2 000 à 3 000 caractères** (limite dure de 4 096 dans Claude Code) ;
- en tête, les mots-clés qui déclenchent la recherche d'outils (Légifrance, loi, décret, code, article, jurisprudence, Cour de cassation, Conseil d'État, Conseil constitutionnel, CNIL, convention collective) ;
- ensuite l'ordre d'appel, les contraintes dures (ne jamais inventer d'ELI, d'ECLI ou d'identifiant ; citer `source_url`), le sens des codes d'erreur et le recours à `fr_coverage` ;
- **pas** de description d'outil dupliquée (`SCHEMA:693-694` ; `BLOG/2025-11-03-using-server-instructions.md:146-151`) ;
- garder le test de dérive instructions/outils de la référence (`[PY] tests/test_instructions_drift.py:40-57`).

**f. Descriptions d'outils** : 3 à 6 phrases, mots-clés en tête. Elles disent quand utiliser l'outil, quand **ne pas** l'utiliser (renvoi vers l'outil adéquat), quels préfixes d'identifiant sont acceptés et ce que l'outil **ne renvoie pas** (par exemple ACCO : métadonnées seulement, `[PY] src/fr_eli_mcp/server.py:42`). Chacune reste sous 4 096 caractères.

**g. Erreurs** : gabarit `[code] cause + correctif + outil à appeler`, entièrement dans le texte (§3.3, §3.5).

**h. Budget de descripteurs** : mesurer en CI la taille totale de `tools/list` (la référence atteint environ 17,6 Ko de descripteurs plus 4,5 Ko d'instructions, `02-defauts.md:424`) et fixer un plafond (test « snapshot de taille »).

**i. Évaluation** : jeu de tâches juridiques réalistes, avec mesure des tokens, du nombre d'appels et des erreurs avant et après chaque changement de description (`[A-TOOLS]`).

---

## 8. Stabilité des descripteurs (descriptor lock) et versionnage

### 8.1 Pourquoi verrouiller

1. **Ordre et contenu stables pour les caches.** Ordre déterministe (SHOULD) pour le cache client et le taux de succès du cache de prompt (`SPEC/server/tools.mdx:71-74`). Côté hôte, « Adding or removing tool definitions mid-conversation invalidates that cache, and the resulting miss can cost more tokens than the definitions you removed » (`DOCS/develop/clients/client-best-practices.mdx:151-160`). Les listes sont aussi mises en cache selon `ttlMs` (`SPEC/server/utilities/caching.mdx:39-60`).
2. **Rug pull et épinglage par les clients.** « Definitions are not fixed at install time either: a server can quietly change them after you approved it (a rug pull), so a one-time consent dialog and package pinning do not cover this surface. » (`DOCS/tutorials/security/local-server-security.mdx:96-100`). « Prefer clients that pin tool definitions. Some clients record tool definitions when you first approve a server and ask again when they change. » (`:205-208`). Un changement non voulu de description bloque donc le connecteur chez ces clients.
   - Référence Python : la passerelle PATRON « treats a changed description or schema as drift and holds the connector for review » (`[PY] tests/test_descriptor_lock.py:3-8`).
   - Invariant Labs (MCP-Scan) épinglerait les outils par hash contre les rug pulls, d'après des sources secondaires (résultats de recherche web, https://invariantlabs.ai/blog/mcp-security-notification injoignable) : **NON VÉRIFIÉ**.
3. **Les descriptions sont des instructions.** Tout changement modifie le comportement du modèle, en bien ou en mal (`[A-TOOLS]` : de petites retouches donnent de grands écarts). Il doit donc passer en revue, comme du code.
4. **Effets de bord des dépendances.** Une mise à jour du SDK ou du générateur de schéma (schemars, macros rmcp) peut changer les schémas sans changement de code (`02-defauts.md:460`, cas FastMCP et pydantic). Le lock rend la dérive visible.
5. **Rejet par certains clients.** Claude Code exclut un outil au schéma invalide, écarte ou réécrit un combinateur racine, et coupe les descriptions trop longues (`[CC-DOC]`). Le lock permet d'y adosser des tests de conformité.
6. **Manifeste MCPB.** Le `manifest.json` d'un bundle peut lister statiquement `tools` et `prompts` (avec `tools_generated` et `prompts_generated`), et les hôtes « may choose to validate prompts at run-time against the declaration » (`[B] MANIFEST.md:249-251`, `:712-760`). C'est une seconde copie des descripteurs à garder synchronisée : il faut la générer depuis le lock.

### 8.2 Ce que fait la référence Python

- Lit les descripteurs **comme un client** (`Client(mcp).list_tools()`), les trie par nom, calcule un SHA-256 sur du JSON canonique (clés triées, `separators=(",",":")`, `ensure_ascii=False`) par outil et pour l'ensemble, et compare au lock (`[PY] tests/test_descriptor_lock.py:35-59`, `:62-83`).
- Mise à jour explicite avec `--update` « together with a version bump » (`:15-17`, `:86-93`).
- Faiblesses relevées par `02-defauts.md:460` :
  - `package_version` jamais comparé à `pyproject.toml` ;
  - `instructions` non verrouillées ;
  - `meta.fastmcp` inclus dans le hash ;
  - dépendances non épinglées.

### 8.3 Recommandation pour la version Rust

- **Périmètre du lock** : `tools/list` (tous les champs : `name`, `title`, `description`, `inputSchema`, `outputSchema`, `annotations`, `_meta`), `prompts/list`, `resources/templates/list`, `resources/list` (statique), et le noyau de `server/discover` (`capabilities`, `instructions`, `supportedVersions`). Hors périmètre : les enveloppes `resultType`, `ttlMs` et `_meta.serverInfo.version`.
- **Génération par le chemin réel du protocole** (transport en mémoire, requêtes `tools/list`, etc.), et **pour chaque ère** si les formes diffèrent (par exemple `title` absent en 2025-03-26).
- **Canonisation** : clés triées et séparateurs fixes. Option : JCS RFC 8785. Hash SHA-256 par élément et global. Fichier `tools.lock.json` formaté (diff lisible) et un **snapshot** (`insta`, piste de `02-defauts.md:76`).
- **Garde-fous CI** :
  - échec si dérive ;
  - échec si `version` du lock ≠ version du crate ≠ `server.json.version` ≠ `serverInfo.version` ;
  - commande de régénération explicite ;
  - `Cargo.lock` versionné ;
  - plafond de taille des descripteurs ;
  - longueurs (`instructions` < 4 096, chaque description < 4 096, propriétés ≤ 64 caractères, noms `^[a-z0-9_]{1,64}$`) ;
  - racine `type: object` sans combinateur pour `inputSchema` et `outputSchema` ;
  - validation 2020-12 des schémas.
- **Génération du manifeste MCPB** (`tools`, `prompts`) depuis la même source.

### 8.4 Versionnage

Trois axes indépendants :

| Axe | Où | Règle |
|---|---|---|
| Révision du protocole MCP | `supportedVersions` (`DiscoverResult`), `protocolVersion` (legacy) | `2026-07-28` plus les versions legacy servies (`08` §15, questions ouvertes) |
| Version du serveur | `Cargo.toml`, `serverInfo.version` (`Implementation.version`, `SCHEMA:976-997`), `server.json.version` (« Equivalent of Implementation.version », `[R] docs/reference/server-json/draft/server.schema.json`, `ServerDetail.version`) | SemVer ; **unique et immuable** une fois publiée au registre (« Once published, the version string (and other metadata) cannot be changed », `[R] docs/modelcontextprotocol-io/versioning.mdx:32`) ; plages interdites (`:44-66`) ; aligner la version du serveur et celle du paquet (`:74-90`) |
| Surface d'outils | `toolset_sha256` du lock | Proposition de politique SemVer (ci-dessous) |

Politique SemVer de surface proposée (recommandation, inspirée de la politique de cycle de vie de la spec, `SPEC/changelog.mdx:107-112`) :

- **MAJEURE** : retrait ou renommage d'outil, de paramètre ou de champ de sortie ; ajout d'un paramètre requis ; restriction de type ou d'`enum` ; changement de sémantique ; changement d'annotation vers « moins sûr ».
- **MINEURE** : nouvel outil, prompt ou template ; nouveau paramètre optionnel ; nouveau champ de sortie optionnel ; nouvelle valeur d'`enum` en entrée.
- **CORRECTIVE** : reformulation de description, d'`instructions` ou de `title`. Elle change quand même le lock, puisque les clients qui épinglent re-demandent l'approbation : le noter dans le CHANGELOG.
- **Dépréciation avant retrait** : mention « Déprécié : utiliser X » en tête de description pendant au moins une version mineure avant le retrait en majeure.

---

## 9. Registre MCP et distribution d'un binaire natif

### 9.1 Format `server.json`

Schéma : `$schema` `https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json` (dernière version datée ; un *draft* non publié accepte en plus une variable de template en tête d'URL de transport) (`[R] docs/reference/server-json/CHANGELOG.md:5-63`). Définitions lues dans `[R] docs/reference/server-json/draft/server.schema.json` (`ServerDetail` à partir de la ligne 405, `Package` à partir de la ligne 207).

| Champ | Contraintes | Source (`server.schema.json` sauf mention) |
|---|---|---|
| `name` | DNS inversé, **exactement un `/`**, 3 à 200 caractères, motif `^[a-zA-Z0-9.-]+/[a-zA-Z0-9._-]+$` | `:448-455` |
| `description` | 1 à **100** caractères | `:434-439` |
| `title` | Optionnel, 1 à 100 caractères | `:472-477` |
| `version` | Requise, SemVer conseillé, ≤ 255, pas de plage | `ServerDetail.version` ; `[R] docs/modelcontextprotocol-io/versioning.mdx:10-66` |
| `websiteUrl`, `icons`, `repository{url, source, id?, subfolder?}` | Optionnels ; `repository.id` sert à détecter une « resurrection » de dépôt | `:485` ; définition `Repository` |
| `packages[]` | `registryType`, `identifier`, `transport` **requis** ; `version` précise (≠ `latest`) ; `registryBaseUrl`, `runtimeHint`, `runtimeArguments`, `packageArguments`, `environmentVariables`, `fileSha256` | `:207-301` (`version` `:284-292`) |
| `environmentVariables[]` | `KeyValueInput{name, description, isRequired, isSecret, default, choices, format…}` ; `isSecret` = « clients should handle the value securely » | définitions `Input` (`isSecret` `:104-108`), `KeyValueInput` |
| `packages[].transport` | `stdio`, ou `streamable-http` avec `url` (template `{var}` permis) | `StdioTransport`, `StreamableHttpTransport` |
| `remotes[]` | Serveur hébergé (`streamable-http` ou `sse`) | `RemoteTransport` |
| `_meta` | Au registre officiel, **seule** la clé `io.modelcontextprotocol.registry/publisher-provided` est conservée, ≤ 4 Ko | `[R] docs/reference/server-json/official-registry-requirements.md:46-71` |

Exigences du **registre officiel** (`[R] docs/reference/server-json/official-registry-requirements.md:7-44`) : authentification du namespace (`io.github.<owner>/…` via GitHub, ou domaine), **preuve de propriété** de chaque paquet, registres de base restreints (npm, PyPI, NuGet, **crates.io**, Docker Hub/ghcr.io/Quay/`*.pkg.dev`/`*.azurecr.io`/`mcr.microsoft.com`, releases GitHub/GitLab pour MCPB). Le registre est encore en « preview » (`[R] docs/modelcontextprotocol-io/package-types.mdx:6-8`).

### 9.2 Types de paquets pour un binaire Rust

Types acceptés par le validateur : `npm`, `pypi`, `oci`, `nuget`, `mcpb`, `cargo` (`[R] pkg/model/constants.go:5-10` ; `internal/validators/package.go:15-29`).

| Type | Fonctionnement | Preuve de propriété et contraintes | Source |
|---|---|---|---|
| **`mcpb`** | Bundle ZIP `manifest.json` plus le serveur. Type `server.type = "binary"` : « Pre-compiled executable with all dependencies included… Completely self-contained ». Surcharges par plateforme (`platform_overrides` : `darwin`, `win32`, `linux`) ; `.exe` ajouté automatiquement sous Windows. Installation en un clic (Claude Desktop) | URL HTTPS d'un **asset de release GitHub ou GitLab** contenant « mcp » ; **`fileSha256` requis** (vérifié par les clients, pas par le registre) ; pas de `registryBaseUrl` | `[R] docs/modelcontextprotocol-io/package-types.mdx:225-261` ; `[R] internal/validators/registries/mcpb.go:33-54`, `:96-133` ; `[B] MANIFEST.md:380-393`, `:399-406`, `:460-463`, `:491-523` ; `BLOG/2025-11-20-adopting-mcpb.md:15-32` |
| **`cargo`** | `cargo install <crate>` puis exécution par nom ; **toolchain Rust requise** chez l'utilisateur ; pas de `runtimeHint` | Token `mcp-name: <nom-serveur>` dans le README **rendu** par crates.io. **Les commentaires HTML sont supprimés** : le token doit être du texte **visible** (ex. puce « MCP Registry name: `mcp-name: io.github.owner/repo` »), suivi d'une frontière (espace, saut de ligne, balise). `fileSha256` interdit | `[R] docs/modelcontextprotocol-io/package-types.mdx:132-181` ; `[R] internal/validators/registries/cargo.go:48-61`, `:77`, `:337-340` ; exemple `[R] docs/reference/server-json/generic-server-json.md:375-402` |
| **`oci`** | Image `registre/namespace/dépôt:tag` (ou digest), runtime `docker` | Label `io.modelcontextprotocol.server.name="<nom>"` | `[R] docs/modelcontextprotocol-io/package-types.mdx:183-223` |
| `npm`, `pypi`, `nuget` | Sans objet pour un binaire natif | — | `:14-130` |

**Points d'attention** :

- La référence Python place le token dans un **commentaire HTML** (`[PY] README.md:3` : `<!-- mcp-name: io.github.matematicsolutions/fr-eli-mcp -->`). Cela marche pour PyPI mais **échouera pour `cargo`**.
- Le `server.json` Python (`[PY] server.json:1-22`) ne déclare **aucune** `environmentVariables`. Les identifiants PISTE devraient être déclarés avec `isRequired: true` et `isSecret: true` (exemple du motif : `[R] docs/reference/server-json/generic-server-json.md:37-80`).
- Le sélecteur `platform_overrides` du manifeste MCPB se limite aux plateformes (`darwin`, `win32`, `linux`) ; la distinction d'**architecture** (x86_64 ou arm64) dans un même bundle est **NON VÉRIFIÉE** (`[B] MANIFEST.md:333`, `:503-523` ; `grep arch` sans résultat). Pistes à vérifier : binaire universel macOS, un bundle par architecture.
- Version du manifeste MCPB : le texte annonce « Current version: `0.3` » (mis à jour le 2025-12-02), mais des schémas `v0.1` à `v0.4` existent (`[B] MANIFEST.md:3-4` ; `[B] schemas/`). Version à retenir : **NON VÉRIFIÉE**.
- Le manifeste MCPB exige `privacy_policies` quand l'extension contacte des services externes qui traitent des données utilisateur (`[B] MANIFEST.md:254`). C'est probablement applicable, puisque les requêtes sont envoyées à PISTE (interprétation).
- **Publication** : `mcp-publisher login github-oidc` puis `mcp-publisher publish` en CI, avec `id-token: write`, comme la référence (`[PY] .github/workflows/release.yml:14`, `:71-91`).

### 9.3 Recommandation de distribution

- **Paquet principal : `mcpb`**. Il s'adresse au public non développeur (juristes), sans toolchain, en un clic, avec intégrité SHA-256.
- **Secondaires** : **`cargo`** pour les développeurs Rust, **`oci`** pour l'isolation recommandée par le guide de sécurité locale (« run every third-party server in a container », `DOCS/tutorials/security/local-server-security.mdx:216-244`).
- Plusieurs entrées `packages[]` dans un même `server.json` sont permises : exemple « Filesystem Server with Multiple Packages », `[R] docs/reference/server-json/generic-server-json.md:167`.

---

## 10. Check-list de conformité pour le serveur Légifrance

Les points de cycle de vie, d'enveloppe et de dual-era sont détaillés dans la check-list de `08-mcp-2026-07-28.md` §14, qui reste applicable telle quelle. Ci-dessous, les points propres à cet axe.

### 10.1 Descripteurs d'outils

- [ ] **T-01** Noms `fr_*` en snake_case ASCII `^[a-z0-9_]{1,64}$`, sans point, uniques (`SPEC/server/tools.mdx:309-321` ; `[API-TOOLS]`).
- [ ] **T-02** `title` (en français) **et** `annotations.title` identiques sur chaque outil (`SCHEMA:2010` ; `[VS] …ToolContribution.ts:149`).
- [ ] **T-03** `inputSchema` : racine `{"type":"object", "additionalProperties": false}`, sans `anyOf`/`oneOf`/`allOf` à la racine, sans `$ref` externe, dialecte 2020-12 (absence de `$schema`), propriétés de 1 à 64 caractères (`SCHEMA:1997` ; `SPEC/basic/index.mdx:299-318` ; `[CC-DOC]`).
- [ ] **T-04** `enum` pour `fond`, `minimum`/`maximum` pour `page_size` et les bornes de `max_chars`/`offset`, `pattern` pour les identifiants et les dates.
- [ ] **T-05** `outputSchema` à racine **objet** pour tout outil, compatible 2025-11-25 (`[S] schema/2025-11-25/schema.ts:1279-1284`).
- [ ] **T-06** Annotations : `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true` ; `openWorldHint` selon la décision prise (questions ouvertes) ; aucun champ `execution`.
- [ ] **T-07** Descriptions de 3 à 6 phrases, mots-clés en tête, « quand ne pas utiliser », limites ; chacune < 4 096 caractères (`[API-TOOLS]` ; `[CC-DOC]`).
- [ ] **T-08** `tools/list` dans un ordre fixe, une seule page, `ttlMs` plus `cacheScope: "public"` en moderne ; pas de `listChanged` (`SPEC/server/tools.mdx:71-81`).
- [ ] **T-09** Aucune directive dans les descriptions visant d'autres serveurs ou outils (`DOCS/tutorials/security/local-server-security.mdx:92-100`).

### 10.2 Résultats d'outils

- [ ] **R-01** Succès : `structuredContent` conforme à l'`outputSchema` (MUST) **et** bloc `text` (SHOULD) qui porte la même information (JSON sérialisé ou rendu équivalent) (`SPEC/server/tools.mdx:500`, `:509-515`).
- [ ] **R-02** Toute information utile au modèle (résumé, continuation, `hint`, URI de ressource) est **dans `structuredContent`**, puisque VS Code et Claude Code n'envoient que lui en cas de succès (`[VS]` `:331-337`, `:389-392` ; `[CC-86032]`).
- [ ] **R-03** `structuredContent` toujours objet, jamais tableau ni scalaire (compatibilité legacy ; `09-rmcp-2026.md:151`).
- [ ] **R-04** Retour progressif : `detail` (`summary` par défaut), `offset`/`max_chars` (défaut proche de 8 000, plafond < 50 000), `focus` (passages BM25), champs `truncated`/`next_offset`/`total_chars`/`hint` (`[A-TOOLS]` ; `[CC-DOC]`).
- [ ] **R-05** Contrat de citation présent **une fois** par résultat. Notes répétitives (`dataset_note`, `eli_note`) déplacées vers `instructions` et `fr_coverage`.
- [ ] **R-06** Test de budget : réponse par défaut ≤ N caractères sur les fixtures (`02-defauts.md:428`).

### 10.3 Erreurs

- [ ] **E-01** Outil inconnu → JSON-RPC `-32602` ; `name` absent ou `arguments` non objet → `-32602` ; panique ou erreur interne → `-32603` (`SPEC/server/tools.mdx:742-758`).
- [ ] **E-02** Argument invalide (type, plage, préfixe d'identifiant), y compris un échec de désérialisation dans rmcp → `isError: true` avec `[invalid_arg] …` (`SPEC/server/tools.mdx:760-782` ; SEP-1303).
- [ ] **E-03** Erreur amont → `isError` avec `not_found`, `rate_limited`, `upstream_error` ou `config_error`, plus le correctif. Message **entièrement dans `content[0].text`**, sans `structuredContent` (`[CC-86032]`).
- [ ] **E-04** Aucun code JSON-RPC maison dans `-32768..-32000` ; jamais `-32002` en moderne, mais `-32002` pour « resource not found » en session legacy (`SPEC/basic/index.mdx:109-155`).
- [ ] **E-05** Aucun secret, jeton ou chemin interne dans les messages d'erreur.

### 10.4 Resources, templates, prompts, completions

- [ ] **P-01** `resources` déclaré (`{}`) si l'on émet des `resource_link` ou des ressources embarquées (`SPEC/server/tools.mdx:475-476`).
- [ ] **P-02** Schéma `legifrance://` RFC 3986 ; templates RFC 6570 simples (`{var}`, `{?date}`) ; correspondance inverse stricte ; validation de toute URI (MUST) (`SPEC/server/resources.mdx:397-400`, `:430`).
- [ ] **P-03** `resources/read` : ressource inexistante → `-32602` + `data.uri` (moderne), jamais `contents: []` ; `mimeType` (`text/markdown` ou `text/plain`) ; `annotations.lastModified` si connu ; `ttlMs`/`cacheScope` (`SPEC/server/resources.mdx:402-426` ; `SPEC/server/utilities/caching.mdx:13-21`).
- [ ] **P-04** Toute URI émise dans un `resource_link` est aussi présente dans `structuredContent` et résolue par `resources/read`.
- [ ] **P-05** Prompts à arguments **mono-jeton** (identifiants, `fond`), validés ; `-32602` sur nom ou argument invalide (`SPEC/server/prompts.mdx:319-325` ; `[CC-DOC]`).
- [ ] **P-06** `completion/complete` : ≤ 100 valeurs, triées, statiques et locales, débit limité, `hasMore`/`total` (`SPEC/server/utilities/completion.mdx:142-147`, `:207-214`).

### 10.5 Utilitaires et transport

- [ ] **U-01** Pas de capacité `logging` ; `tracing` vers **stderr** ; aucune écriture parasite sur stdout (pas de `println!`) (`SPEC/basic/transports/stdio.mdx:13-19` ; `09-rmcp-2026.md:165`).
- [ ] **U-02** `notifications/progress` seulement si `progressToken` est présent, croissant, arrêté à la fin (utile pour `fr_search_all`).
- [ ] **U-03** Annulation : abandon de la requête amont et silence total ensuite (`SPEC/basic/transports/stdio.mdx:76-83`).
- [ ] **U-04** Ni sampling, ni roots, ni élicitation en v1 ; désérialisation tolérante de `inputResponses`/`requestState`.
- [ ] **U-05** HTTP (phase 2) : `Origin` validé (403), écoute sur 127.0.0.1 par défaut, authentification requise même en local, en-têtes `Mcp-Method`/`Mcp-Name`/`MCP-Protocol-Version` validés (`SPEC/basic/transports/streamable-http.mdx:54-68`, `:250-297`).
- [ ] **U-06** HTTP avec OAuth : PRM RFC 9728, 401 avec `resource_metadata`, validation d'audience, 403 `insufficient_scope`, **aucun passthrough** du jeton MCP vers PISTE (`SPEC/basic/authorization/index.mdx:73-74`, `:281-297`, `:332-377` ; `security-considerations.mdx:126`).

### 10.6 Sécurité

- [ ] **S-01** Identifiants PISTE depuis l'environnement uniquement (stdio), en `SecretString`, jamais journalisés ni audités (`SPEC/basic/index.mdx:221-226`).
- [ ] **S-02** Hôtes amont codés en dur ou en liste blanche ; **pas de manifeste distant** capable de rediriger le Bearer ; pas de télémétrie (`02-defauts.md:123-131`).
- [ ] **S-03** Limitation de débit des appels d'outils et de complétion, et sémaphore pour `fr_search_all` (`SPEC/server/tools.mdx:792`).
- [ ] **S-04** Assainissement des sorties (HTML amont → texte), bornes de taille sur toute réponse amont (décompression bornée).
- [ ] **S-05** Audit JSONL avec entrées **hachées** (pas de requêtes en clair).
- [ ] **S-06** Aucun handle d'état ; s'il y en a un, aléatoire, expirant et lié au principal (`DOCS/tutorials/security/security_best_practices.mdx:537-553`).

### 10.7 Stabilité et versionnage

- [ ] **V-01** Lock canonique (`tools`, `prompts`, `templates`, `resources` statiques, `capabilities`, `instructions`) généré par le chemin protocole, test CI de dérive, régénération explicite.
- [ ] **V-02** Égalité des versions : crate = `serverInfo.version` = `server.json.version` = `packages[].version` = version du lock.
- [ ] **V-03** Politique SemVer de surface (§8.4) appliquée et notée au CHANGELOG ; toute modification de description est signalée.
- [ ] **V-04** `instructions` < 4 096 caractères (cible < 3 000) et test de dérive instructions/outils/codes d'erreur.
- [ ] **V-05** Manifeste MCPB (`tools`, `prompts`) généré depuis le lock.

### 10.8 Registre et distribution

- [ ] **D-01** `server.json` : `name` `io.github.<owner>/<repo>`, `description` ≤ 100 caractères, `title`, `repository` (avec `id`), `websiteUrl`.
- [ ] **D-02** `packages[]` : `mcpb` (asset de release, nom contenant « mcp », `fileSha256`) et, si retenus, `cargo` (README crates.io avec `mcp-name:` **visible**) et `oci` (LABEL).
- [ ] **D-03** `environmentVariables` : identifiants PISTE `isRequired: true` et `isSecret: true` ; variables optionnelles (répertoire d'audit, cache) documentées.
- [ ] **D-04** Publication OIDC (`mcp-publisher login github-oidc`) après les gates CI (lock, conformance, tests) ; version jamais réutilisée.

### 10.9 Tests d'interopérabilité

- [ ] **I-01** Suite `modelcontextprotocol/conformance` (voir `08` §14.1), MCP Inspector (eras `legacy`, `auto`, `modern`).
- [ ] **I-02** Claude Code (défaut, puis `MCP_PROTOCOL_NEGOTIATION=legacy`) : vérifier ce que reçoit le modèle (succès et erreur), la troncature des `instructions` et des descriptions, et les seuils de 10 000/25 000 tokens.
- [ ] **I-03** VS Code : approbations (`readOnlyHint`, `openWorldHint`) et contenu transmis au modèle (`structuredContent`).
- [ ] **I-04** Évaluation agentique sur des tâches juridiques réelles (tokens, nombre d'appels, erreurs) avant et après chaque changement de surface (`[A-TOOLS]`).

---

## 11. Points NON VÉRIFIÉS (récapitulatif)

1. **Comportement actuel de Claude Code** sur `structuredContent` contre `content` : il repose sur le commentaire d'un mainteneur (2026-08-15, v2.1.233) et sur une reproduction en 2.1.260 (`[CC-86032]`, ticket ouvert). Une correction ultérieure (envoi des deux formes, ou de `structuredContent` en cas d'erreur) n'est pas exclue.
2. **claude.ai web et Claude Desktop** : forme transmise au modèle (`structuredContent` ou texte), support des templates de ressources et des prompts.
3. **Sémantique UI exacte de `confirmResults`** (VS Code, `openWorldHint: true`) : code lu, documentation injoignable.
4. Transmission (ou non) de l'`outputSchema` au modèle par Claude Code.
5. Traitement par Claude Code d'un point (`.`) dans un nom d'outil MCP (l'API Claude l'interdit).
6. Prise en compte par Claude Code du mot-clé JSON Schema `examples` (équivalent d'`input_examples`).
7. Détails de RFC 6570 (niveaux, usage inverse) : site IETF injoignable.
8. Article Invariant Labs sur le *tool poisoning* et les *rug pulls*, et épinglage par hash de MCP-Scan : connus seulement par des résultats de recherche.
9. MCPB : gestion de l'**architecture** (x86_64/arm64) dans un bundle ; version de manifeste à cibler (0.3 annoncée, schémas jusqu'à 0.4).
10. Date d'ajout du type `cargo` au registre (clone superficiel) ; statut « preview » du registre au 2026-10-15.
11. Support de RFC 6570 par les crates `iri-string` et `uritemplate-next`.
12. Conversion par rmcp d'un échec de désérialisation des arguments en erreur JSON-RPC plutôt qu'en `isError` (renvoi à l'axe rmcp).
13. Source d'une table « nom de code → `LEGITEXT` » pour les complétions.

---

## 12. Questions ouvertes (décisions à prendre)

1. **Stratégie `structuredContent`.** Faut-il garder un `outputSchema` et un `structuredContent` sur **tous** les outils (parité, validation client) en y mettant le résumé progressif, ou les supprimer sur les outils « texte » (`fr_get_text`, `fr_get_decision`) pour renvoyer du Markdown lisible dans `content` ? Et le bloc texte doit-il être le JSON sérialisé (lettre de la spec) ou un rendu Markdown équivalent ?
2. **`openWorldHint`** : `true` (parité Python, prudence « service externe ») ou `false` (« corpus fermé », recommandation du billet de mars 2026, pas d'approbation des résultats dans VS Code) ?
3. **Consolidation.** Faut-il fusionner les `fr_get_*` en un `fr_get(id)` aiguillé par préfixe (moins d'outils, moins d'ambiguïté) au prix de la parité des descripteurs et des clients qui épinglent (PATRON) ? Et `fr_search_all` doit-il être un outil distinct ou un paramètre `fonds: [...]` de `fr_search` ?
4. **Compatibilité des descripteurs avec la v0.5.3 Python.** L'identité octet pour octet est irréaliste (schemars contre pydantic). Faut-il annoncer une **version majeure** (1.0.0) avec le diff de surface documenté ?
5. **Valeurs par défaut du retour progressif** : unité (caractères ou estimation de tokens), `max_chars` par défaut (8 000 ?), plafond (< 50 000), nombre de passages `focus`.
6. **`_meta["anthropic/maxResultSizeChars"]`** et **`anthropic/alwaysLoad`** : les émettre (gain dans Claude Code, couplage à un client) ?
7. **Langue** des descriptions et des `instructions` : anglais (comme la référence) ou français (terminologie juridique native, utilisateurs francophones) ?
8. **`resources/list`** : vide, ou quelques ressources statiques (`legifrance://coverage`, liste des fonds) ?
9. **Distribution** : MCPB seul, ou MCPB, cargo et OCI ? Un bundle par architecture ou un bundle multi-plateforme ?
10. **Noms des variables d'environnement** : garder `FR_ELI_CLIENT_ID`/`FR_ELI_CLIENT_SECRET` (continuité pour les utilisateurs existants) ou un nouveau préfixe propre au binaire Rust ?
11. **Versions legacy servies** : `2025-11-25` et `2025-06-18` ; `2025-03-26` aussi, sans `title` d'outil ni `structuredContent` (renvoi à `08` §17) ?
12. **Mode HTTP** : hébergement distant prévu (donc AS OAuth à choisir, scopes, `remotes[]` dans `server.json`) ou seulement local (jeton statique ou UDS) ?
