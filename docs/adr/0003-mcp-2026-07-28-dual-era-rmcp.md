# ADR-0003 — MCP 2026-07-28 en dual-era avec `rmcp =3.5.1`

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M0 (spike de compilation), M4 (stdio, deux ères), M7 (resources, prompts, completions), M8 (HTTP)
- **Questions liées** : Q7 (versions legacy servies), Q8 (`ttlMs`/`cacheScope`), Q4 (HTTP partagé), Q14 (langues) ; propositions adoptées par défaut, à confirmer (00 §13). Spikes M0 V-28 et V-29 ; V-34 et V-35 en M4.
- Révisé le 2026-10-15 (revue) : alignement sur 00 §6 révision 2 : `supportedVersions` et repli `-32022` (AM-11) ; `PreLifecycleFilter` et écarts du cycle de vie (AM-12) ; ressource introuvable en `-32002` aux pairs legacy (AM-09) ; `subscriptions/listen` (AM-10) ; HTTP de M8 (AM-13) ; contrôle final du 2026-10-16 : rmcp soumis à la règle d'âge (éligible le 2026-10-19) ; version connue non servie dans l'en-tête HTTP et débit des invocations (00 §6).

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
- **Bugs ouverts** (recherche 09 §4) : #1337, `structuredContent` non objet envoyé aux pairs legacy ; #1347, `-32601` au lieu de `-32602` ; #1348 (P0), cache de schéma des `Mcp-Param-*`.
- **Lecture du code de 3.5.1** (annexe D §4.2, §9.2) :
  - une notification ou une réponse reçue avant l'ouverture d'une ère arrête le service (`ExpectedInitializeRequest`, `service/server.rs:681-685`) ; les requêtes prématurées, elles, reçoivent toujours une réponse ; un EOF avant toute ère donne `ServerInitializeError::ConnectionClosed` (`service/server.rs:433-444`) ;
  - `transport::stdio()` renvoie le tuple `(Stdin, Stdout)`, qui implémente `IntoTransport` mais pas `Transport` (`transport/io.rs:4-6`, `transport/async_rw.rs:24-34`) ;
  - `ErrorData::resource_not_found` donne `-32002`, réécrit en `-32602` pour les pairs ≥ 2026-07-28 (`handler/server.rs:264-270`) ; `subscriptions/listen` reçoit `-32601` par défaut (`handler/server.rs:147-154`), alors que la révision 2026-07-28 rend l'accusé obligatoire (annexe D §5.3).
- **2025-03-26** n'a ni `structuredContent` ni `outputSchema` (apparus en 2025-06-18). Or notre réponse **est** le `structuredContent` (annexe D §4.4, [ADR-0006](0006-sortie-compacte-retour-progressif.md)).

## Décision

1. **Serveur dual-era** :
   - ère moderne 2026-07-28 ;
   - ère legacy par `initialize`, pour 2025-11-25 et 2025-06-18 ;
   - `supported_protocol_versions()` explicite `[2026-07-28, 2025-11-25, 2025-06-18]` ;
   - `supportedVersions` de `server/discover` = ces trois versions (00 §6, AM-11) ;
   - 2025-03-26 et 2024-11-05 ne sont **pas** servies (Q7, 00 §6).
2. **SDK** : `rmcp` **et** `rmcp-macros` épinglés `"=3.5.1"`, avec les features par défaut et `transport-io` : seuls crates épinglés exactement, les autres exigences étant écrites au niveau mineur (00 §2). La règle d'âge minimal s'applique aussi : 3.5.1 (2026-10-05) devient éligible le 2026-10-19, et M0 ne fait pas le verrou initial avant cette date ; le spike M0 peut commencer avant (00 §2, §6). `rmcp-macros`, déclaré pour son épinglage sans emploi direct, est ignoré par `cargo machete` (02 §3.0.1). `transport-streamable-http-server` n'est ajoutée qu'en M8 ; `request-state`, `elicitation` et `auth` ne sont pas activées ; `Cargo.lock` est versionné.
3. **Confinement** : seul `lf-adapter-mcp` dépend de rmcp ([ADR-0002](0002-hexagonal-workspace.md)).
4. **Identité** : `get_info()` renvoie un `ServerConfig` avec `Implementation::new("mcp-legifrance", env!("CARGO_PKG_VERSION"))`, jamais `from_build_env`. Les instructions sont identiques dans `DiscoverResult` et `InitializeResult`. Elles sont en anglais et font au plus 3 500 caractères. Langues (00 §6, adoptées par défaut, Q14) : instructions, descriptions, `title` des outils, messages d'erreur et libellés de resources en anglais ; noms et textes des prompts en français.
5. **Règles de 00 §6** :
   - `server/discover` est servi depuis des données statiques : `DiscoverResult::from_server_info(SUPPORTED.to_vec(), self.get_info())` (deux arguments en 3.5.1), avec `ttlMs`/`cacheScope` explicites ;
   - repli si V-34 confirme à l'exécution (M4) qu'une requête à enveloppe moderne portant une version legacy est acceptée et servie sans `resultType` ni indices de cache : `[2026-07-28]` seul dans discover, sans toucher `supported_protocol_versions()`, et `-32022` (`data.supported = ["2026-07-28"]`) répondu par `PreLifecycleFilter` à toute requête non-`initialize` dont le `_meta` déclare une version antérieure ; sinon, écart consigné dans l'annexe D ;
   - `serverInfo` dans le `_meta` des autres résultats modernes : posé si rmcp le permet (V-32), sinon écart accepté et documenté ;
   - le démarrage ne fait **aucun** appel réseau ;
   - le serveur ne quitte jamais sur un message inattendu (décision 6) ;
   - `ttlMs = 3 600 000` et `cacheScope = public` sur les listes et `resources/read`, pour les pairs modernes seulement (Q8) ;
   - ressource inconnue ou introuvable (M7) → `ErrorData::resource_not_found` avec `data.uri` : rmcp émet `-32002` aux pairs legacy (spécification 2025-11-25) et `-32602` aux pairs 2026-07-28 ; identifiant invalide dans une URI de gabarit → `-32602` (`invalid_params`) dans les deux ères ; jamais `contents: []` ;
   - pas de capacité `logging` : journaux `tracing` sur stderr, garde-fou `rmcp=warn` ;
   - ni sampling, ni roots, ni élicitation, ni MRTR, ni `x-mcp-header` ;
   - `outputSchema` et `structuredContent` à racine objet, sans combinateur racine ni `$ref` externe ;
   - propriétés de premier niveau ≤ 64 caractères, noms d'outils `^[a-zA-Z0-9_-]{1,64}$`.
6. **Compléments** (annexe D §4.3, §4.7 et §5.3), dès le spike M0 :
   - `PreLifecycleFilter`, enveloppe `Transport<RoleServer>` d'`AsyncRwTransport::new_server(tokio::io::stdin(), tokio::io::stdout())` (le tuple de `stdio()` ne convient pas), ignore (`debug!`) toute notification ou réponse reçue avant l'ouverture d'une ère ; il est retiré quand rmcp corrige ce comportement ;
   - écarts acceptés et testés : `ping` avant toute ère → `{}` ; `initialize` portant un `_meta` moderne → poignée de main legacy ; ligne non JSON ignorée ;
   - un EOF (`ConnectionClosed`) ou un signal (`Cancelled`) avant toute ère est une fin normale (code 0) ; tout autre échec d'initialisation donne le code 1 ;
   - `subscriptions/listen` (ère moderne) : accusé minimal (`accepted_subscription_filter` → filtre vide ; premier message `notifications: {}`), puis aucune notification jusqu'à l'annulation ; ère legacy : `-32601` (ni `resources/subscribe` ni `listChanged`) ;
   - critère M4 : un EOF de stdin avec un `listen` ouvert termine le processus (code 0) en moins d'une seconde (V-35).
7. **Montée de version** : toujours manuelle (procédure de l'annexe D §9.4) : lire le CHANGELOG (`LATEST`, `KNOWN_VERSIONS`, `fill_missing_cache_hints`, boucle pré-cycle) ; rejouer les transcriptions JSON-RPC des deux ères ; régénérer et comparer le verrou des descripteurs ; en M8, relancer la suite officielle de conformité.
8. **Retrait futur du legacy** : seulement par un nouvel ADR, et seulement quand le support de 2026-07-28 est vérifié dans VS Code, Cursor et Claude Desktop (annexe D §10.2).
9. **HTTP (M8, optionnel, conditionné aux CGU, Q4)** (00 §6, annexe D §5.2) : Streamable HTTP sans session (POST seul ; GET et DELETE → 405, gestionnaire de sessions sans event store, V-40), écoute sur `127.0.0.1` par défaut, `allowed_hosts`.
   - Validation `Origin` à notre charge : `with_allowed_origins(liste)` + `enforce_origin_validation()`, car rmcp ne valide rien avec la liste vide par défaut ; `Origin` invalide → 403. Tout POST autre qu'`initialize` sans `MCP-Protocol-Version` → 400 + `-32020` par une couche axum : rmcp supposerait 2025-03-26, non servie (V-39) ; un en-tête qui nomme une version connue non servie (`2025-03-26`, `2024-11-05`) → 400 + `-32022` par la même couche (annexe D §5.2, C-48).
   - Débit des invocations d'outils (C-34) : limiteur dans la couche axum, 429 + `Retry-After` au-delà ; sur stdio, écart justifié (annexe D §4.7). Corps ≤ 256 Kio ; fabrique sans état = clone d'un prototype `LegifranceServer` (routeur construit une fois) ; profil local à jeton statique (`LF_MCP_HTTP_TOKEN`, nom proposé, figé en M8), puis profil distant (resource server OAuth, jamais de token passthrough vers PISTE).

## Conséquences

### Positives

- Un seul binaire sert Claude Code (moderne) et VS Code (legacy), ce qui satisfait l'exigence « MCP v2 » sans perdre de client.
- Le protocole est délégué à un SDK officiel, qui annonce 30/30 sur les suites de conformité 2025-11-25 et 2026-07-28 (chiffres du projet, recherche 09 §3.1).
- L'épinglage exact rend chaque changement de comportement du SDK visible et volontaire.

### Négatives

- Deux ères à tester en permanence : exigences stdio C-01 à C-34 (annexe D §8.1). Les formes de sortie et le code de « ressource introuvable » diffèrent selon l'ère (`resultType`, indices de cache, `-32002` ou `-32602`).
- Claude Code démarre le serveur deux fois par session (sonde, puis session) : le démarrage doit rester minimal (annexe D §4.5).
- L'épinglage prive des correctifs non publiés de `main` (`stdio_pipes()`, #1283). Une parade maison (`PreLifecycleFilter`) est à maintenir jusqu'à correction en amont.

### À surveiller

- Points **NON VÉRIFIÉS**, levés en M0 et M4 :
  - [v:rmcp-methode-inattendue] : aucun message inattendu ne fait quitter le processus ; la lecture du code (annexe D §4.2) le prévoit pour les requêtes, seules les notifications prématurées imposent le filtre ; transcriptions en processus au spike M0, puis sur le binaire en M4 (V-30) ;
  - [v:rmcp-transport-enveloppe] : `PreLifecycleFilter` autour d'`AsyncRwTransport::new_server(…)` est accepté par `serve_with_ct` (V-29, bloquante M0) ;
  - [v:rmcp-enveloppe-version-legacy] : requête moderne portant une version legacy (V-34, M4) ; détermine le repli de la décision 5 ;
  - [v:rmcp-listen-cloture] : EOF avec un `subscriptions/listen` ouvert, sortie en moins d'une seconde (V-35, M4) ;
  - [v:rmcp-meta-serverinfo-resultats] : `serverInfo` dans le `_meta` des résultats modernes (V-32) ;
  - [v:rmcp-bugs-statut] : statut des issues #1337, #1347, #1348.
- **M8** : [v:rmcp-http-entete-absent] (V-39, requête sans `MCP-Protocol-Version`) et [v:rmcp-http-get-405] (V-40, GET et DELETE sans event store), levées par la suite de conformité HTTP.
- Si le spike M0 invalide `PreLifecycleFilter`, la parade de remplacement passe par un nouvel ADR (règle de l'index, §2).

### Tests qui la gardent

- Transcriptions `modern_discover`, `legacy_2025_11_25`, `premature_notification` et `mrtr_fields_tolerated` (spike M0).
- Liste minimale de M4 (06 §6.1), dont `modern_envelope_legacy_version`, `modern_listen`, `eof_after_discover` et `eof_with_open_listen` ; `e2e_eof_after_discover_exits_zero` ; exigences C-01 à C-34 de l'annexe D (M4).
- M7 : `resource_not_found_by_era` (`-32002` en legacy, `-32602` en moderne) et `malformed_resource_uri`.
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

- Plan : [00-decisions.md](../plan/00-decisions.md) §6 (révision 2, AM-09 à AM-13) ; [annexes/D-mcp-2026-07-28.md](../plan/annexes/D-mcp-2026-07-28.md) §1 à §5, §8 à §10, §12 ; [06-qualite.md](../plan/06-qualite.md) §2.6, §6.1 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §2, §3 ; [02-architecture.md](../plan/02-architecture.md) §3.6, §8.6, §12.3, §13 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) (risques rmcp et clients).
- Recherche : [09-rmcp-2026.md](../recherche/09-rmcp-2026.md) §0 à §8 ; [08-mcp-2026-07-28.md](../recherche/08-mcp-2026-07-28.md) §3, §4, §15 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §1.2, §2.1.
- Code : https://github.com/modelcontextprotocol/rust-sdk (tag `rmcp-v3.5.1`, `79437f2`) ; spécification https://github.com/modelcontextprotocol/modelcontextprotocol (tag `2026-07-28`) ; billet de GA https://blog.modelcontextprotocol.io/posts/2026-07-28/.
