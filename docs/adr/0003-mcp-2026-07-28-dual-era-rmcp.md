# ADR-0003 — MCP 2026-07-28 en dual-era avec `rmcp =3.5.1`

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : spike rmcp (avant la conception des ports de M3), M4 (stdio, deux ères), M7 (resources, prompts, completions), M8 (HTTP)
- **Questions liées** : Q7 (versions legacy servies), Q8 (`ttlMs`/`cacheScope`), Q4 (HTTP partagé)

## Contexte

- **Exigence de l'utilisateur : « MCP v2 ».** La spécification ne connaît pas ce terme. Elle parle de la révision **2026-07-28** et d'ères *modern* et *legacy*. 2026-07-28 est la dernière révision : aucun tag ni RC postérieur n'existe au 2026-10-15 (recherche 09 §1, annexe D §1.1, §1.2).
- **Ce que change l'ère moderne** (annexe D §2) :
  - plus d'`initialize` : chaque requête porte un `_meta` ;
  - `server/discover` est **obligatoire** côté serveur ;
  - tout résultat porte `resultType`, et six résultats portent `ttlMs`/`cacheScope` ;
  - `ping` et `logging/setLevel` sont retirés, Logging est déprécié ;
  - « ressource introuvable » passe de `-32002` à `-32602` (recherche 09 §1).
- **Les clients sont dans des ères différentes** (annexe D §10.1) :
  - Claude Code ≥ 2.1.292 négocie 2026-07-28 par défaut en stdio, après une sonde `server/discover` ;
  - VS Code (`main` du 2026-10-15) n'envoie que `initialize` 2025-11-25 ;
  - Cursor et Claude Desktop : **NON VÉRIFIÉ** [v:cursor-ere-et-contenu], [v:desktop-structured-content].

  Un client legacy face à un serveur modern-only échoue (spécification `docs/specification/2026-07-28/basic/versioning.mdx:164-172`, recherche 09 §1).
- **rmcp 3.5.1** (2026-10-05) sert les deux ères dans un même binaire, en stdio comme en HTTP. Le SDK Rust est Tier 1 depuis le 2026-08-21 (recherche 09 §0, §3). Deux précautions s'imposent :
  - `LATEST` est passé à 2026-07-28 dans une **mineure**, la 3.5.0 (recherche 09 §3.2) ;
  - `rmcp` déclare `rmcp-macros` en caret (02 §13), ce qui laisse les macros monter seules.
- **Bugs ouverts** (recherche 09 §4) :
  - #1337 : `structuredContent` non objet envoyé aux pairs legacy ;
  - #1347 : `-32601` au lieu de `-32602` ;
  - #1348 (P0) : cache de schéma des `Mcp-Param-*`.
- **Lecture du code de 3.5.1** : une notification reçue avant l'ouverture d'une ère arrête le service (`ExpectedInitializeRequest`, `service/server.rs:681-685`). Les requêtes prématurées, elles, reçoivent toujours une réponse (annexe D §4.2).
- **2025-03-26** n'a ni `structuredContent` ni `outputSchema` (apparus en 2025-06-18). Or notre réponse **est** le `structuredContent` (annexe D §4.4, [ADR-0006](0006-sortie-compacte-retour-progressif.md)).

## Décision

1. **Serveur dual-era** :
   - ère moderne 2026-07-28 ;
   - ère legacy par `initialize`, pour 2025-11-25 et 2025-06-18 ;
   - `supported_protocol_versions()` explicite `[2026-07-28, 2025-11-25, 2025-06-18]` ;
   - 2025-03-26 et 2024-11-05 ne sont **pas** servies (Q7, 00 §6).
2. **SDK** : `rmcp` **et** `rmcp-macros` épinglés `=3.5.1`, avec les features par défaut et `transport-io`.
   - `transport-streamable-http-server` n'est ajoutée qu'en M8.
   - `request-state`, `elicitation` et `auth` ne sont pas activées.
   - `Cargo.lock` est versionné.
3. **Confinement** : seul `lf-adapter-mcp` dépend de rmcp ([ADR-0016](0016-hexagonal-workspace.md)).
4. **Identité** : `get_info()` renvoie un `ServerConfig` avec `Implementation::new("mcp-legifrance", env!("CARGO_PKG_VERSION"))`, jamais `from_build_env`. Les instructions sont identiques dans `DiscoverResult` et `InitializeResult`. Elles sont en anglais et font au plus 3 500 caractères.
5. **Règles de 00 §6** :
   - `server/discover` est servi depuis des données statiques ;
   - le démarrage ne fait **aucun** appel réseau ;
   - le serveur ne quitte jamais sur une méthode inattendue ;
   - `ttlMs = 3 600 000` et `cacheScope = public` sur les listes et `resources/read`, pour les pairs modernes seulement (Q8) ;
   - ressource inconnue → `-32602` ;
   - pas de capacité `logging` : journaux `tracing` sur stderr, garde-fou `rmcp=warn` ;
   - ni sampling, ni roots, ni élicitation, ni MRTR, ni `x-mcp-header` ;
   - `outputSchema` et `structuredContent` à racine objet, sans combinateur racine ni `$ref` externe ;
   - propriétés de premier niveau ≤ 64 caractères, noms d'outils `^[a-zA-Z0-9_-]{1,64}$`.
6. **Compléments** (annexe D §4.3 et §5.3) :
   - un filtre `PreLifecycleFilter` autour de `stdio()` écarte les notifications reçues avant l'ouverture d'une ère ;
   - un EOF avant toute ère est une fin normale (code 0) ;
   - `subscriptions/listen` reçoit un accusé minimal, et aucune notification n'est jamais émise.
7. **Montée de version** : toujours manuelle (procédure de l'annexe D §9.4) :
   - lire le CHANGELOG : `LATEST`, `KNOWN_VERSIONS`, `fill_missing_cache_hints`, boucle pré-cycle ;
   - rejouer les transcriptions JSON-RPC des deux ères ;
   - régénérer et comparer le verrou des descripteurs ;
   - en M8, relancer la suite officielle de conformité.
8. **Retrait futur du legacy** : seulement par un nouvel ADR, et seulement quand le support de 2026-07-28 est vérifié dans VS Code, Cursor et Claude Desktop (annexe D §10.2).

## Conséquences

### Positives

- Un seul binaire sert Claude Code (moderne) et VS Code (legacy), ce qui satisfait l'exigence « MCP v2 » sans perdre de client.
- Le protocole est délégué à un SDK officiel, qui annonce 30/30 sur les suites de conformité 2025-11-25 et 2026-07-28 (chiffres du projet, recherche 09 §3.1).
- L'épinglage exact rend chaque changement de comportement du SDK visible et volontaire.

### Négatives

- Deux ères à tester en permanence : transcriptions stdio C-01 à C-32 (annexe D §8). Les formes de sortie diffèrent selon l'ère (`resultType`, indices de cache).
- Claude Code démarre le serveur deux fois par session (sonde, puis session) : le démarrage doit rester minimal (annexe D §4.5).
- L'épinglage prive des correctifs non publiés de `main` (`stdio_pipes()`, #1283). Une parade maison (`PreLifecycleFilter`) est à maintenir jusqu'à correction en amont.

### À surveiller

- Points **NON VÉRIFIÉS**, levés par le spike rmcp (avant M3) et en M4 :
  - [v:rmcp-methode-inattendue] : aucun message inattendu ne fait quitter le processus ;
  - [v:rmcp-transport-enveloppe] : l'enveloppe du transport compile ;
  - [v:rmcp-enveloppe-version-legacy] : requête moderne portant une version legacy ;
  - [v:rmcp-bugs-statut] : statut des issues #1337, #1347, #1348.
- **Écart avec la spécification** : 00 §6 impose `-32602` pour une ressource inconnue dans les deux ères, alors que la spécification 2025-11-25 et rmcp utilisent `-32002` en legacy (annexe D §12.1 n° 1). La décision de 00 est appliquée. Une modification passerait par un nouvel ADR.
- **M8** : rmcp ne valide pas `Origin` par défaut. La validation exigée par 00 §6 est une configuration à notre charge, `enforce_origin_validation()` (annexe D §5.2, §12.1 n° 6).

### Tests qui la gardent

- Transcriptions `modern_discover`, `legacy_2025_11_25` et `premature_notification` (spike rmcp, avant M3).
- Liste C-01 à C-32 (M4).
- Test qui vérifie que les descripteurs servis sont identiques dans les deux ères.
- Test de dérive et de taille des instructions.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Ère moderne seule (`with_stateless_protocol_metadata_required`) | Inutilisable dans VS Code, et probablement dans Cursor (annexe D §10.2). |
| Ère legacy seule (2025-11-25) | Ne répond pas à l'exigence « MCP v2 ». Claude Code est moderne par défaut. |
| `rmcp = "~3.5.1"` (recherche 09 §7a) | Laisse passer des correctifs non relus, et `rmcp-macros` reste en caret. Écarté par 00 §6. |
| `pmcp` 2.22.7 | Tiers, MSRV 1.91 ; gardé comme **repli** derrière la même façade (recherche 09 §6, §7b). |
| `turbomcp` 4.0 alpha ; `rust-mcp-sdk` 2.0 | Alpha pour le premier. Modern-only pour le second : la ligne 1.x sert 2025-11-25 (recherche 09 §6). |
| Couche protocolaire maison | Estimée à 2 500 à 4 000 lignes plus les tests (ordre de grandeur non vérifié, recherche 09 §7b), avec une conformité et une maintenance à assumer à chaque révision. |
| Servir aussi 2025-03-26 | Pas de `structuredContent` : la réponse compacte serait invisible (Q7). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §6 ; [annexes/D-mcp-2026-07-28.md](../plan/annexes/D-mcp-2026-07-28.md) §1 à §5, §8 à §10, §12 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §2, §3 ; [02-architecture.md](../plan/02-architecture.md) §3.6, §8.6, §12.3, §13 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) (risques rmcp et clients).
- Recherche : [09-rmcp-2026.md](../recherche/09-rmcp-2026.md) §0 à §8 ; [08-mcp-2026-07-28.md](../recherche/08-mcp-2026-07-28.md) §3, §4, §15 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §1.2, §2.1.
- Code : https://github.com/modelcontextprotocol/rust-sdk (tag `rmcp-v3.5.1`, `79437f2`) ; spécification https://github.com/modelcontextprotocol/modelcontextprotocol (tag `2026-07-28`) ; billet de GA https://blog.modelcontextprotocol.io/posts/2026-07-28/.
