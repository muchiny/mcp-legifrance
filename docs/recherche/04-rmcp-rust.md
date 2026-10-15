# 04 — SDK MCP Rust (`rmcp`) et écosystème de crates

> Axe de recherche : exactitude de l'API `rmcp` (version courante), versions/MSRV des crates de l'écosystème, recommandations d'architecture Rust pour la réécriture hexagonale de `fr-eli-mcp`.
> Date de collecte : 2026-10-15. Aucune compilation n'a été possible (crates.io API / static.crates.io / docs.rs bloqués) : tout ce qui suit est lu dans le code source des dépôts clonés ou dans l'index sparse crates.io.

## 0. Sources et conventions de citation

| Alias | Emplacement | Version / révision |
|---|---|---|
| `RS:` | `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/clones/rust-sdk/` (clone de https://github.com/modelcontextprotocol/rust-sdk) | tag `rmcp-v3.5.1`, commit `79437f291b2c44053d00dcd5db969fd0cca7c887` (2026-10-05) |
| `RQ:` | `…/scratchpad/clones/reqwest-04/` (https://github.com/seanmonstar/reqwest) | tag `v0.13.5` (2026-09-08) |
| `SC:` | `…/scratchpad/clones/schemars-04/` (https://github.com/GREsau/schemars) | tag `v1.2.3` (2026-10-12) |
| `IN:` | `…/scratchpad/clones/insta-04/` (https://github.com/mitsuhiko/insta) | tag `1.49.0` |
| `SE:` | `…/scratchpad/clones/iqlusion-04/secrecy/` (https://github.com/iqlusioninc/crates) | branche principale, `Cargo.toml` = `version = "0.10.3"` (= dernière publiée) |
| `CG:` | `…/scratchpad/clones/cargo-04/doc/book/src/reference/` (https://github.com/rust-lang/cargo) | master `01b026f0` (2026-10-14) |
| `CD:` | `…/scratchpad/clones/cargo-dist-04/` (https://github.com/axodotdev/cargo-dist) | tag `v0.33.0` (2026-09-10) |
| `PY:` | `/home/user/matematicsolutions/fr-eli-mcp/` | d3b5b13 (référence Python, lecture seule) |
| `IDX:` | `https://index.crates.io/<chemin>` ; copies locales `…/scratchpad/index04/<crate>.jsonl` | lu le 2026-10-15 |

Les numéros de ligne `fichier:N` ou `fichier:N-M` renvoient à ces révisions. « NON VÉRIFIÉ » = non confirmé par un fichier lu ou une source web consultée.

---

## 1. `rmcp` sur l'index crates.io

### 1.1 Versions
Source : `IDX: rm/cp/rmcp` (68 versions, une seule yankée : `0.1.0`).

| Version | `rust_version` | Publication |
|---|---|---|
| **3.5.1** (dernière stable, non yankée) | **1.88** | 2026-10-05 |
| 3.5.0 | 1.88 | 2026-09-28 |
| 3.4.1 | 1.88 | 2026-09-23 |
| 3.4.0 | 1.88 | 2026-09-15 |
| 3.3.0 | 1.88 | 2026-09-10 |
| 3.2.0 | 1.88 | 2026-08-31 |
| 3.0.0 | 1.88 | 2026-07-28 |

`rmcp-macros` suit la même numérotation (3.5.1, rust_version 1.88, 2026-10-05 — `IDX: rm/cp/rmcp-macros`) ; les deux crates sont « versionnées ensemble » (`RS:VERSIONING.md:11-12`). Le dépôt déclare `edition = "2024"`, `rust-version = "1.88"`, `version = "3.5.1"` (`RS:Cargo.toml:11-14`) et épingle sa toolchain de dev à `channel = "1.96"` (`RS:rust-toolchain.toml:2`).

**Rythme de versions très rapide** : 1.0.0 le 2026-03-03, 2.0.0 le 2026-06-27, 3.0.0 le 2026-07-28 (`RS:crates/rmcp/CHANGELOG.md:313,496,…`). Une migration 2.x→3.x a son guide (`RS:README.md:9`, discussion GitHub #969, NON VÉRIFIÉ : contenu non lu). Conséquence : tout tutoriel/blog antérieur à juillet 2026 est probablement faux pour 3.x (voir pièges §2.19).

Politique semver : MSRV relevée seulement en MINOR/MAJOR (`RS:VERSIONING.md:44-48`) ; semver évaluée sur les features par défaut et toutes sauf `local` (`RS:VERSIONING.md:33-36`).

### 1.2 Features exactes de `rmcp` 3.5.1
Source : `IDX` (champ `features`/`features2`) et `RS:crates/rmcp/Cargo.toml:124-201`.

| Feature | Contenu exact | Commentaire |
|---|---|---|
| `default` | `["base64", "macros", "server"]` | `Cargo.toml:125` |
| `server` | `["transport-async-rw", "schemars", "dep:pastey", "uuid"]` | **active schemars automatiquement** (`:128`) |
| `macros` | `["dep:rmcp-macros", "dep:pastey"]` | `:129` |
| `client` | `["dep:tokio-stream"]` | utile en dev-dependency pour les tests in-process |
| `schemars` | `["dep:schemars"]` | `:201` |
| `transport-io` | `["transport-async-rw", "tokio/io-std"]` | **requis pour `transport::stdio`** (non inclus par défaut) (`:176`) |
| `transport-async-rw` | `["tokio/io-util", "tokio-util/codec"]` | `:175` |
| `transport-streamable-http-server` | `["transport-streamable-http-server-session", "server-side-http", "transport-worker"]` | `:186-190` |
| `transport-streamable-http-server-session` | `["transport-async-rw", "dep:async-trait", "dep:tokio-stream"]` | tire `async-trait` |
| `server-side-http` | `uuid, dep:rand, dep:tokio-stream, dep:http, dep:http-body, dep:http-body-util, dep:bytes, dep:sse-stream, tower, base64` | `:144-155` |
| `tower` | `["dep:tower-service"]` | `:197` |
| `transport-worker` | `["dep:tokio-stream"]` | `:157` |
| `transport-child-process` | `["transport-async-rw", "tokio/process", "dep:process-wrap"]` | côté client |
| `which-command` | `["transport-child-process", "dep:which"]` | |
| `transport-streamable-http-client` | `["client-side-sse", "transport-worker"]` | |
| `transport-streamable-http-client-reqwest` | `["transport-streamable-http-client", "__reqwest"]` | |
| `transport-streamable-http-client-unix-socket` | `[… "dep:hyper", "dep:hyper-util", …, "tokio/net"]` | |
| `client-side-sse` | `["dep:sse-stream", "dep:http", "dep:bytes", "base64"]` | |
| `auth` | `["dep:async-trait", "dep:oauth2", "__reqwest", "dep:url"]` | OAuth **pour MCP** (pas pour PISTE) |
| `auth-client-credentials-jwt` | `["auth", "dep:jsonwebtoken", "uuid"]` | jsonwebtoken avec `aws_lc_rs` |
| `auth-enterprise-managed` | `["auth", "base64"]` | |
| `elicitation` | `["dep:url"]` | |
| `request-state` | `["dep:hmac", "dep:sha2", "dep:zeroize", "base64"]` | MRTR (SEP-2322) |
| `reqwest` | `["__reqwest", "reqwest?/rustls"]` | TLS rustls pour les transports HTTP client |
| `reqwest-native-tls` | `["__reqwest", "reqwest?/native-tls"]` | |
| `reqwest-tls-no-provider` | `["__reqwest", "reqwest?/rustls-no-provider"]` | |
| `__reqwest` | `["dep:reqwest"]` | interne |
| `local` | `["rmcp-macros?/local"]` | handlers `!Send` (exclu du contrat semver) |
| `base64`, `uuid` | features implicites (dépendances optionnelles) | listées dans `[package.metadata.docs.rs]` (`Cargo.toml:22,44`) |

**Il n'existe pas** de feature nommée `transport-sse-server`, `transport-sse`, `server-http`, etc. Le transport HTTP+SSE legacy (2024-11-05) est un non-objectif explicite (`RS:README.md:1670-1691`).

### 1.3 Dépendances notables de `rmcp` 3.5.1
`RS:crates/rmcp/Cargo.toml:48-122` :
- toujours : `serde 1 (derive, rc)`, `serde_json 1`, `thiserror 2`, `tokio 1 (sync, macros, rt, time)`, `futures 0.3`, `indexmap 2 (serde)`, `tracing 0.1`, `tokio-util 0.7`, `pin-project-lite 0.2`, **`chrono 0.4.38` (default-features=false, `serde`, `now`)** (`:113-114`) ;
- optionnelles : **`schemars = { version = "1.0", features = ["chrono04"] }`** (`:66`), `reqwest 0.13.2` (default-features=false, `json`, `stream`) (`:77-80`), `oauth2 5.0`, `jsonwebtoken 11 (aws_lc_rs)`, `uuid 1 (v4)`, `sha2 0.11`, `hmac 0.13`, `base64 0.23`, `rand 0.10`, `http 1`, `hyper 1`, `tower-service 0.3`, `async-trait 0.1.89`, `process-wrap 10`.
- `rmcp-macros` 3.5.1 dépend de `syn = "3"`, `darling 0.24`, `quote 1`, `proc-macro2 1` (`RS:crates/rmcp-macros/Cargo.toml:18-23`).

Le crate `rmcp` active `[lints.clippy] exhaustive_structs = "warn"` et `exhaustive_enums = "warn"` (`RS:crates/rmcp/Cargo.toml:13-15`) : **quasiment toutes les structures/énumérations publiques du modèle sont `#[non_exhaustive]`** → impossible de les construire par littéral struct hors du crate ; il faut les constructeurs `new(..)`/`with_*(..)`/builders (voir §2.19).

---

## 2. API `rmcp` 3.5.1 lue dans le code et les exemples

### 2.1 Changements structurants de la génération 3.x (à connaître avant tout)
1. `ServerHandler::get_info()` retourne **`ServerConfig`** (alias de `InitializeResult`) : `pub type ServerConfig = InitializeResult;` (`RS:crates/rmcp/src/model.rs:1178`). `ServerInfo` est un alias **déprécié** (`model.rs:1205-1206`), ajouté en 3.4.0 (`CHANGELOG.md` §3.4.0 « add ServerConfig and ClientConfig (#1266) »). → exiger `rmcp >= 3.4`.
2. `call_tool` retourne `Result<CallToolResponse, ErrorData>` (`RS:crates/rmcp/src/handler/server.rs:531-537`), `read_resource` → `Result<ReadResourceResponse, _>` (`server.rs:445-453`), `get_prompt` → `Result<GetPromptResponse, _>` (`server.rs:415-421`). Ces énumérations (MRTR, SEP-2322) ont des `From<CallToolResult>`/`From<ReadResourceResult>`/`From<GetPromptResult>` (`RS:crates/rmcp/src/model/mrtr.rs:105-119,146-157,177-188`) → écrire `.into()`.
3. Protocole par défaut : `ProtocolVersion::default() == LATEST == V_2026_07_28` (`model.rs:156-160,169-176`) ; `KNOWN_VERSIONS` = 2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25, 2026-07-28 (`model.rs:204-210`) ; `LATEST_WITH_INITIALIZE = V_2025_11_25` (`model.rs:201`). Le SDK négocie automatiquement (`RS:crates/rmcp/src/service/server.rs:486-517`) : si le client demande une version connue avec `initialize`, elle est acceptée ; le `protocol_version` de `get_info()` ne sert que de repli. **Ne pas** recopier `with_protocol_version(ProtocolVersion::V_2024_11_05)` des exemples (`RS:examples/servers/src/common/counter.rs:218`, `…/progress_demo.rs:136`).
4. Le type de contenu s'appelle **`ContentBlock`** (`RS:crates/rmcp/src/model/content.rs:259-265`) avec `ContentBlock::text(..)`, `::image(..)`, `::resource(..)`, `::embedded_text(..)`, `::resource_link(..)`, `::json(..)` (`content.rs:268-309`).
5. Logging, Sampling et Roots sont **dépréciés** (SEP-2577) (`RS:README.md:652,858,923` ; `model/capabilities.rs:370-374`).

### 2.2 Le trait `ServerHandler`
Déclaration (`RS:crates/rmcp/src/handler/server.rs:640-650`) :
```rust
#[cfg(not(feature = "local"))]
pub trait ServerHandler: Sized + Send + Sync + 'static { server_handler_methods!(); }
```
Toutes les méthodes ont une implémentation par défaut et sont déclarées en RPITIT : `fn x(&self, …) -> impl Future<Output = Result<…, McpError>> + MaybeSendFuture + '_` (`server.rs:308-638`), où `MaybeSendFuture` = `Send` hors feature `local` (`RS:crates/rmcp/src/service.rs:30-34`). On les implémente avec `async fn` (pratique de tous les exemples, ex. `RS:examples/servers/src/common/counter.rs:222-269`).

Méthodes et défauts (`server.rs`) :

| Méthode | Signature (paramètres → sortie) | Défaut |
|---|---|---|
| `ping` | `(ctx)` → `()` | `Ok(())` (`:310-315`) |
| `initialize` | `(InitializeRequestParams, ctx)` → `InitializeResult` | `set_peer_info` + `negotiate_initialize` (`:317-324`) |
| `negotiate_initialize` | `(&InitializeRequestParams)` → `Result<InitializeResult>` | (`:365-376`) |
| `supported_protocol_versions` | → `Cow<'static, [ProtocolVersion]>` | `KNOWN_VERSIONS` (`:388-390`) |
| `discover` | `(ctx)` → `DiscoverResult` | (`:392-400`) |
| `complete` | `(CompleteRequestParams, ctx)` → `CompleteResult` | `Ok(CompleteResult::default())` (`:401-407`) |
| `set_level` | `(SetLevelRequestParams, ctx)` → `()` | `method_not_found` (`:408-414`) |
| `get_prompt` | `(GetPromptRequestParams, ctx)` → `GetPromptResponse` | `method_not_found` (`:415-421`) |
| `list_prompts` | `(Option<PaginatedRequestParams>, ctx)` → `ListPromptsResult` | vide (`:422-428`) |
| `list_resources` | `(Option<PaginatedRequestParams>, ctx)` → `ListResourcesResult` | vide (`:429-435`) |
| `list_resource_templates` | `(Option<PaginatedRequestParams>, ctx)` → `ListResourceTemplatesResult` | vide (`:436-444`) |
| `read_resource` | `(ReadResourceRequestParams, ctx)` → `ReadResourceResponse` | `method_not_found` (`:445-453`) |
| `accepted_subscription_filter` / `listen` | abonnements 2026-07-28 (SEP-2575) | `None` = non implémenté (`:460-480`) |
| `subscribe` / `unsubscribe` | legacy, `#[deprecated]` | (`:481-502`) |
| `call_tool` | `(CallToolRequestParams, ctx)` → `CallToolResponse` | `method_not_found` (`:531-537`) |
| `list_tools` | `(Option<PaginatedRequestParams>, ctx)` → `ListToolsResult` | vide (`:538-544`) |
| `get_tool` | `(&str)` → `Option<Tool>` (sync) | `None` (`:549-551`) |
| `on_custom_request`, `on_cancelled`, `on_progress`, `on_initialized`, `on_roots_list_changed`, `on_custom_notification` | notifications | no-op (`:552-600`) ; `on_initialized` loggue `info!("client initialized")` (`:584`) |
| `get_info` | `()` → `ServerConfig` (sync) | `ServerConfig::default()` (`:602-604`) |
| `get_task`, `update_task`, `cancel_task` | extension Tasks (SEP-2663) | `method_not_found` (`:607-636`) |

`ServerHandler` est aussi implémenté pour `Box<T>` et `Arc<T>` (`server.rs:872-873`).

Dispatch (`server.rs:50-272`) : le SDK appelle `self.get_info()` dans plusieurs chemins (tâches, listen) → `get_info` doit rester bon marché. Après le handler, pour les pairs ≥ 2026-07-28, le SDK remplit les indications de cache manquantes, sinon retire `resultType` (`:246-262`) ; il convertit `RESOURCE_NOT_FOUND` (-32002) en `INVALID_PARAMS` (-32602) pour les pairs ≥ 2026-07-28 (`:264-271`).

### 2.3 `get_info` : `ServerConfig` / `InitializeResult` / `Implementation` / capabilities
- `InitializeResult` (`model.rs:1113-1129`) : champs `protocol_version`, `capabilities: ServerCapabilities`, `server_info: Implementation`, `instructions: Option<String>`, `meta: Option<MetaObject>` ; `#[non_exhaustive]`.
- Constructeurs (`model.rs:1131-1160`) : `ServerConfig::new(capabilities)` (protocole = défaut, `server_info = Implementation::from_build_env()`), `.with_instructions(impl Into<String>)`, `.with_server_info(Implementation)`, `.with_protocol_version(ProtocolVersion)`.
- `Implementation` (`model.rs:1520-1590`) : `name`, `title`, `version`, `description`, `icons`, `website_url` ; `Implementation::new(name, version)`, `.with_title`, `.with_description`, `.with_icons`, `.with_website_url`.
- **Piège `from_build_env()`** : son corps est `env!("CARGO_CRATE_NAME")`/`env!("CARGO_PKG_VERSION")` **compilé dans le crate rmcp** (`model.rs:1556-1565`) → il renvoie le nom/version de *rmcp*, pas de notre binaire. Le test `RS:crates/rmcp/tests/test_streamable_http_session_store.rs:130-131` attend d'ailleurs `client_info.name == "rmcp"`. Les macros `#[tool_handler]`/`#[prompt_handler]` utilisent `Implementation::from_build_env()` quand `name`/`version` ne sont pas fournis (`RS:crates/rmcp-macros/src/tool_handler.rs:143-152`). → **toujours** fournir `Implementation::new(NOM, VERSION)` explicitement. Même remarque pour `env!("CARGO_PKG_NAME")` écrit dans un crate bibliothèque (`adapter-mcp`) : il vaut le nom de ce crate ; passer l'identité depuis le crate `app` ou partager `version.workspace = true`.
- `ServerCapabilities` (`RS:crates/rmcp/src/model/capabilities.rs:219-243`) : `experimental`, `extensions`, `logging` (déprécié), `completions`, `prompts`, `resources`, `tools`. Builder à états typés (`capabilities.rs:255-380`) : `ServerCapabilities::builder().enable_tools().enable_resources().enable_completions().enable_prompts().build()` ; variantes `enable_x_with(valeur)` ; `.enable_tool_list_changed()` (après `enable_tools`, `:383-392`), `.enable_prompts_list_changed()`, `.enable_resources_list_changed()`, `.enable_resources_subscribe()` (`:394-423`), `.enable_tasks()` (`:425-…`). `enable_logging()` porte `#[deprecated]` (`:370-374`) → avertissement (bloquant avec `-D warnings`).
- `instructions` : chaîne envoyée une fois à l'initialisation (exemples : `RS:examples/servers/src/completion_stdio.rs:313-330`).

### 2.4 Macros `#[tool_router]`, `#[tool]`, `#[tool_handler]`
#### `#[tool]` (`RS:crates/rmcp-macros/src/tool.rs`, doc `RS:crates/rmcp-macros/src/lib.rs:13-41`)
Attributs acceptés (`ToolAttribute`, `tool.rs:61-82`) : `name` (défaut = nom de la fonction), `title`, `description` (expression ; à défaut, commentaires `///` de la fonction ; depuis 3.4.1 accepte chemins de `const` et `concat!` — CHANGELOG §3.4.1 #1243), `input_schema`, `output_schema`, `annotations(...)`, `icons`, `meta`, `local`.
Effets :
- génère `pub fn <fn>_tool_attr() -> rmcp::model::Tool` construisant `Tool::new_with_raw(name, description, input_schema)` + `.with_title`, `.with_raw_output_schema`, `.with_annotations`, `.with_icons`, `.with_meta` (`tool.rs:95-143`) ;
- `inputSchema` : si un argument `Parameters<T>` est trouvé → `schema_for_input::<T>()` **avec `panic!` si le schéma n'est pas un objet** ; sinon `schema_for_empty_input()` (`tool.rs:194-218`) ;
- `outputSchema` : déduit **uniquement** si le type de retour s'écrit littéralement `Json<T>` ou `Result<Json<T>, E>` (dernier segment de chemin `Json` / `Result`) → `schema_for_output::<T>()` (`tool.rs:8-60, 249-256`). Un alias de type (`type R<T> = Result<Json<T>, E>`) **désactive** la détection ;
- une `async fn` est réécrite en `fn … -> Pin<Box<dyn Future<Output = R> + Send + '_>>` avec `Box::pin(async move { … })` (`tool.rs:273-315`) : **le corps doit être `Send`**.
- le code généré référence `rmcp::…` (chemin relatif, `tool.rs:27,129-139`) → la dépendance doit s'appeler `rmcp` (pas de renommage `package = "rmcp"`), et aucun module local ne doit s'appeler `rmcp`.

#### `#[tool_router]` (`RS:crates/rmcp-macros/src/tool_router.rs`, doc `lib.rs:43-162`)
Attributs : `router` (ident, défaut `tool_router`), `vis` (ex. `vis = "pub"`), `server_handler` (flag), `allow_empty` (flag) (`tool_router.rs:12-33`). Génère `#vis fn tool_router() -> rmcp::handler::server::router::tool::ToolRouter<Self>` avec `.with_route((Self::f_tool_attr(), Self::f))` pour chaque fonction marquée `#[tool]` (`:78-91`). Un bloc sans `#[tool]` est une **erreur de compilation** sauf `allow_empty` (`:65-77`, depuis 3.3.0). Un `macro_rules!` *à l'intérieur* du bloc n'est pas vu (doc `lib.rs:138-156`). `server_handler` émet en plus `#[::rmcp::tool_handler(router = Self::tool_router())] impl ServerHandler for Self {}` (`:107-112`) — réservé aux serveurs « outils seulement » (incompatible avec un `impl ServerHandler` écrit à la main). Les génériques sont supportés (`RS:examples/servers/src/common/generic_service.rs:48-73`, `RS:crates/rmcp/tests/test_tool_macros.rs:98-122`). Plusieurs routeurs se combinent par `+` (`lib.rs:95-125`).

#### `#[tool_handler]` (`RS:crates/rmcp-macros/src/tool_handler.rs`, doc `lib.rs:164-230`)
Attributs : `router` (expression, **défaut `Self::tool_router()`**), `meta`, `name`, `version`, `instructions` (`tool_handler.rs:8-31`). Ajoute, s'ils sont absents du bloc : `call_tool` (via `ToolCallContext::new(self, request, context)` puis `#router.call(tcc).await`, `:44-56`), `list_tools` (avec `ttl_ms: Some(0)` + `CacheScope::Public` pour ≥ 2026-07-28, `:64-86`), `get_tool` (`:88-95`), `get_info` (capabilities tools [+ prompts si `#[prompt_handler]` voisin], `:97-169`). Si `get_info` est écrit à la main, il n'est pas généré — **il faut alors activer soi-même `enable_tools()`**.
→ Avec le défaut `Self::tool_router()`, le routeur est **reconstruit à chaque requête** ; stocker un champ `tool_router: ToolRouter<Self>` et écrire `#[tool_handler(router = self.tool_router)]` (motif de `RS:examples/servers/src/structured_output.rs:44-64`, `RS:crates/rmcp/tests/test_tool_macro_annotations.rs:5-23`).

`ToolRouter<S>` (`RS:crates/rmcp/src/handler/server/router/tool.rs`) : `Clone` sans contrainte sur `S` (`:361-370`), `new`, `with_route`, `with_sync_tool`, `with_async_tool`, `add_route`, `merge`, `remove_route`, `has_route`, `disable_route`/`enable_route`/`is_disabled`/`with_disabled` (`:385-503`), `list_all()` **trié par nom** (`:581-590`, utile pour des snapshots déterministes), `get(name)` (`:596-601`). Noms d'outils : 1–128 caractères `[A-Za-z0-9_.-]` (avertissements sinon) (`RS:crates/rmcp/src/handler/server/tool_name_validation.rs:1-7`).

Alternative sans macro : traits `ToolBase` + `AsyncTool<S>`/`SyncTool<S>` (`RS:crates/rmcp/src/handler/server/router/tool/tool_traits.rs:22-101`, doc `router/tool.rs:41-121`). **Mais** `ToolBase::Error: Into<ErrorData>` (`tool_traits.rs:34`) ⇒ les erreurs deviennent des erreurs **protocole** (invisibles pour le LLM) et `Parameter: Default` est exigé (`:27`). Pour notre besoin (« `[code] message` » lisible par le LLM, comme `PY:src/fr_eli_mcp/server.py:69-79`), la voie macro + `Result<Json<T>, E: IntoContents>` est préférable.

### 2.5 `Parameters<T>` et extracteurs d'arguments
- `pub struct Parameters<P>(pub P);` `#[serde(transparent)]`, `JsonSchema` délégué à `P` (`RS:crates/rmcp/src/handler/server/wrapper/parameters.rs:46-56`).
- Désérialisation : `serde_json::from_value(Value::Object(arguments))` ; en cas d'échec → `ErrorData::invalid_params("failed to deserialize parameters: …")` (`RS:crates/rmcp/src/handler/server/tool.rs:235-250`) que le routeur **convertit en erreur d'outil in-band** `CallToolResult::error(..)` (`router/tool.rs:144-156,573-576` ; CHANGELOG 3.5.1 « keep handler-generated invalid params errors in-band (#1322) ») → le LLM voit le message et peut corriger ses arguments.
- Schéma : seuls les noms/types/docs **des champs** de `T` comptent ; le titre et la doc du type racine sont retirés (`RS:README.md:227` ; `RS:crates/rmcp/src/handler/server/common.rs:53-73`). La racine **doit** être `type: "object"` (sinon panic à la construction du routeur, `common.rs:55-73` + `tool.rs` macro `:203-209`) → toujours une `struct` nommée, jamais un enum/tuple/primitive comme paramètre racine.
- Extracteurs disponibles en arguments d'une méthode `#[tool]` (n'importe quel ordre, jusqu'à 16 — macro `impl_for!(T0 … T15)`, `tool.rs:405`) : `Parameters<P>`, `JsonObject` (arguments bruts, `tool.rs:252-258`), `RequestContext<RoleServer>` (`common.rs:152-159`), **`tokio_util::sync::CancellationToken`** (`common.rs:161-168`), `Extensions` (`:170-177`), `Extension<T>` (`:182-201`), `Peer<RoleServer>` (`:203-210`), `RequestMetaObject` (`:212-222`), `RequestId` (`:225-234`), `ToolName`, `RequestState`, `InputResponses` (`tool.rs:204-232`). Exemple réel `RequestContext` + `Parameters` sur un outil : `RS:examples/servers/src/elicitation_stdio.rs:71-74`.

### 2.6 Valeurs de retour d'un outil : `IntoCallToolResult`, `Json<T>`, sortie structurée
- Trait `IntoCallToolResult { fn into_call_tool_result(self) -> Result<CallToolResponse, ErrorData>; }` (`RS:crates/rmcp/src/handler/server/tool.rs:91-93`) implémenté pour (`tool.rs:95-146`, `wrapper/json.rs:32-43`) : tout `T: IntoContents` (→ `CallToolResult::success`), `CallToolResult`, `InputRequiredResult`, `CallToolResponse`, `ErrorData` (→ erreur protocole), `Json<T>` (`T: Serialize + JsonSchema + 'static` → `CallToolResult::structured`), et `Result<T, E>` avec `T, E: IntoCallToolResult`.
- **Branche `Err(E)` de `Result`** : si `E` produit un `CallToolResult`, rmcp force `is_error = Some(true)` (erreur d'outil in-band) ; si `E = ErrorData` → erreur JSON-RPC (`tool.rs:125-146`). Donc `Result<Json<T>, String>` donne une erreur visible par le LLM (motif `RS:examples/servers/src/structured_output.rs:68-109`).
- `IntoContents` (`RS:crates/rmcp/src/model/content.rs:359-379`) n'est implémenté **que** pour `ContentBlock`, `String` et `()` : **pas** pour `&'static str` ni `Vec<ContentBlock>` (pour plusieurs blocs, retourner `CallToolResult::success(vec![...])`). On peut l'implémenter pour un type d'erreur local (règle d'orphelin respectée) — c'est ce que fait le squelette §3.
- `pub struct Json<T>(pub T);` (`wrapper/json.rs:18`).
- `CallToolResult` (`RS:crates/rmcp/src/model.rs:3964-3999`, `#[non_exhaustive]`, champs **publics** : `result_type`, `content: Vec<ContentBlock>`, `structured_content: Option<Value>`, `is_error: Option<bool>`, `meta`). Constructeurs (`model.rs:4072-4224`) : `success(Vec<ContentBlock>)`, `error(Vec<ContentBlock>)`, `structured(Value)`, `structured_error(Value)`, `.with_meta(Option<MetaObject>)`, `.into_typed::<T>()`.
- **Point « économie de contexte »** : `CallToolResult::structured(v)` place **aussi** `v.to_string()` dans `content` (`model.rs:4155-4163`) — le JSON est donc émis deux fois sur le fil (texte + `structuredContent`). Les champs étant publics, on peut remplacer `content` par un résumé compact après construction (`let mut r = CallToolResult::structured(v); r.content = vec![ContentBlock::text(resume)];`) en déclarant alors `output_schema = rmcp::handler::server::tool::schema_for_output::<T>()` dans `#[tool(...)]` (l'attribut existe, `tool.rs` macro `:71-72,115-117`). Lequel des deux champs les clients (Claude Desktop/Code…) injectent dans le contexte du modèle : **NON VÉRIFIÉ**.
- Depuis 2026-07-28 (SEP-2106), `outputSchema` peut être n'importe quel type JSON Schema et `structuredContent` n'importe quelle valeur (`RS:README.md:229-232`) ; `schema_for_output` ne valide pas la racine (`common.rs:109-144`). Aucune validation côté serveur du `structuredContent` contre `outputSchema` (grep `output_schema` dans `src/` : seulement génération).
- `ContentBlock::resource_link(Resource)` (`content.rs:307-309`) permet de renvoyer un **lien** vers une resource (ex. `legifrance://article/{id}`) au lieu du texte intégral — levier d'économie de contexte ; support réel par les clients **NON VÉRIFIÉ**.

### 2.7 `ErrorData` / `McpError`
- `rmcp::ErrorData` (ré-export de `model::ErrorData`, `RS:crates/rmcp/src/error.rs:3`, `lib.rs:6`) ; alias conventionnel `use rmcp::ErrorData as McpError;` (`RS:README.md:312`). Implémente `Display` et `std::error::Error` (`error.rs:4-14`).
- Struct **exhaustive** `{ code: ErrorCode, message: Cow<'static, str>, data: Option<Value> }` (`model.rs:639-653`).
- Codes (`model.rs:621-633`) : `UNSUPPORTED_PROTOCOL_VERSION -32022`, `MISSING_REQUIRED_CLIENT_CAPABILITY -32021`, `HEADER_MISMATCH -32020`, `RESOURCE_NOT_FOUND -32002`, `INVALID_REQUEST -32600`, `METHOD_NOT_FOUND -32601`, `INVALID_PARAMS -32602`, `INTERNAL_ERROR -32603`, `PARSE_ERROR -32700`.
- Constructeurs (`model.rs:655-715`) : `ErrorData::new(code, msg, data)`, `resource_not_found(msg, data)`, `header_mismatch`, `unsupported_protocol_version`, `missing_required_client_capability`, `parse_error`, `invalid_request`, `method_not_found::<M>()`, `invalid_params(msg, data)`, `internal_error(msg, data)` ; `msg: impl Into<Cow<'static, str>>` (accepte `String` et `&'static str`).
- Doctrine du SDK (`server.rs:503-530`, `RS:README.md:297-331`, `model.rs:4084-4131`) : erreur d'outil `Ok(CallToolResult::error(..))` pour « l'outil a tourné et échoué » (le message atteint l'utilisateur/LLM) ; `Err(McpError)` seulement pour « requête inroutable / serveur inutilisable » (le client l'affiche de façon opaque).
- `RmcpError` (`error.rs:30-60`) : erreur unifiée service/initialisation/transport (côté composition root).

### 2.8 Annotations d'outils
- Attribut macro : `annotations(title = "…", read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true)` (`RS:crates/rmcp-macros/src/tool.rs:146-181,219-248` ; exemple `lib.rs:31`, test `RS:crates/rmcp/tests/test_tool_macro_annotations.rs:14-17,42-44`). Valeurs **littérales** uniquement (darling `Option<bool>`) : pas de constante partagée → répéter le bloc sur chaque outil (la référence Python factorise `READ_ONLY`, `PY:src/fr_eli_mcp/server.py:81-86`).
- Modèle `ToolAnnotations` (`RS:crates/rmcp/src/model/tool.rs:50-148`) : `title`, `read_only_hint`, `destructive_hint`, `idempotent_hint`, `open_world_hint` ; `ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(true)`, `ToolAnnotations::from_raw(..)`, `with_title(..)`.
- Défauts spec rappelés dans la doc macro : `destructive` défaut `true`, `open_world` défaut `true`, `read_only` défaut `false` (`tool.rs` macro `:152-180`) → les fixer explicitement.
- `Tool` (`model/tool.rs:13-40`) : `name`, `title`, `description`, `input_schema: Arc<JsonObject>`, `output_schema`, `annotations`, `icons`, `meta`.

### 2.9 Resources et resource templates
- Pas de macro : implémenter `list_resources`, `read_resource`, `list_resource_templates` dans `impl ServerHandler`, et activer `enable_resources()` (`RS:README.md:355-357`).
- Modèle (`RS:crates/rmcp/src/model/resource.rs`) : `Resource::new(uri, name)` + `with_title/with_description/with_mime_type/with_size/with_icons/with_meta/with_annotations` (`:12-89`) ; `ResourceTemplate::new(uri_template, name)` (+ mêmes `with_*` sauf size) (`:96-164`), `uri_template` = gabarit RFC 6570 (`:97`) ; `ResourceContents::text(text, uri)`, `ResourceContents::blob(base64, uri)`, `.with_mime_type(..)`, `.with_meta(..)` (`:166-221`).
- `ReadResourceResult::new(Vec<ResourceContents>)` + `.with_ttl_ms(..)`, `.with_cache_scope(..)` (`model.rs:1890-1955`), à convertir en `ReadResourceResponse` par `.into()` (exemple compilé `RS:examples/servers/src/common/counter.rs:236-258`).
- `ListResourcesResult` / `ListResourceTemplatesResult` sont générés par `paginated_result!` (`model.rs:1729-1800,1817-1828`) avec les champs `result_type, meta, next_cursor, ttl_ms, cache_scope, <items>` : utiliser **`ListResourceTemplatesResult::with_all_items(vec![...])`** ou `..Default::default()` (`counter.rs:227-233,265-268`).
- Le SDK **ne parse pas** les gabarits : `read_resource` reçoit l'URI concrète et doit la décoder lui-même (`RS:README.md:417-428`).
- Erreur « inconnue » : `McpError::resource_not_found("…", Some(json!({"uri": …})))` (`README.md:429-432`), convertie en -32602 pour les pairs 2026-07-28 (§2.2).
- Notifications : `context.peer.notify_resource_list_changed()`, `notify_resource_updated(..)` (`README.md:479-487`). En 2026-07-28, les abonnements passent par `subscriptions/listen` (`README.md:1237-1316`) — inutile pour un corpus en lecture.

### 2.10 Prompts
- Macros `#[prompt_router]`, `#[prompt]`, `#[prompt_handler]` (`RS:crates/rmcp-macros/src/lib.rs:232-332` ; exemple complet `RS:README.md:524-589`, `RS:examples/servers/src/common/counter.rs:155-207`).
- `#[prompt(name, description, arguments, meta)]` ; arguments déduits de `Parameters<T>` (`lib.rs:240-245`). Retours : `Vec<PromptMessage>`, `GetPromptResult`, ou `Result<_, McpError>` (`README.md:591-594`). Constructeurs : `PromptMessage::new_text(Role::User, ..)`, `GetPromptResult::new(msgs).with_description(..)`.
- `#[prompt_handler]` (attributs `router` défaut `Self::prompt_router()`, `meta`) génère `get_prompt`/`list_prompts` et `get_info` s'il manque (`lib.rs:299-326`). Empilable avec `#[tool_handler]` sur le **même** `impl ServerHandler` (`counter.rs:206-207`) ; stocker `prompt_router: PromptRouter<Self>` (`counter.rs:59-64`).
- **Piège** : la spec type les arguments de prompt en `Record<string,string>` ; un champ entier doit accepter la forme chaîne (désérialiseur maison `counter.rs:30-57`).

### 2.11 Completions (`completion/complete`)
- Implémenter `complete` + `enable_completions()` (`RS:README.md:1002-1071`, exemple `RS:examples/servers/src/completion_stdio.rs:311-413`).
- Types (`model.rs`) : `CompleteRequestParams { meta, r#ref: Reference, argument: ArgumentInfo, context: Option<CompletionContext> }` (`:3377-3408`) ; `Reference::Prompt(PromptReference { name, title })` / `Reference::Resource(ResourceTemplateReference { uri })` (`:3539-3628`, enum `#[non_exhaustive]` → bras `_ =>` obligatoire) ; `ArgumentInfo { name, value }` (`:3631-3647`) ; `CompletionContext::get_argument(name)`, `argument_names()` (`:3330-3375`) ; `CompletionInfo::new / with_all_values / with_pagination(values, total, has_more)` retournent `Result<Self, String>` et refusent > `CompletionInfo::MAX_VALUES = 100` (`:3421-3498`) ; `CompleteResult::new(CompletionInfo)` (`:3500-3537`).
- Pour un template de resource, `ref.uri` = **la chaîne du gabarit** (ex. `"users://{user_id}/profile"`) et `argument.name` = la variable (`README.md:1046-1056`).

### 2.12 Logging MCP (`notifications/message`)
Déprécié par SEP-2577 (`RS:README.md:921-969` ; `notify_logging_message` porte `#[deprecated(since = "1.8.0", …)]`, `RS:crates/rmcp/src/service/server.rs:964-969`). Par défaut `set_level` répond `method_not_found` (`server.rs:408-414`). **Recommandation** : ne pas implémenter le logging MCP ; logs `tracing` sur stderr uniquement.

### 2.13 Progress et annulation
- `RequestContext<R>` (`RS:crates/rmcp/src/service.rs:1208-1216`) : `ct: CancellationToken` (annulé à réception de `notifications/cancelled`), `id: RequestId`, `meta: RequestMetaObject`, `extensions: Extensions`, `peer: Peer<R>` ; méthodes `protocol_version()`, `client_info()`, `client_capabilities()` (`service.rs:1231-1260`).
- Chaque requête entrante est exécutée dans une tâche séparée (`spawn_service_task`, `service.rs:1582-1619`) avec `ct` enfant du token de la boucle (`:1585-1587`) : **l'annulation est coopérative** (le futur n'est pas abandonné par le SDK) → faire `tokio::select!` sur `ctx.ct.cancelled()` dans les outils longs (`fr_search_all`).
- Jeton de progression : `ctx.meta.get_progress_token() -> Option<ProgressToken>` (`RS:crates/rmcp/src/model/meta.rs:435-438`) ; envoi : `ctx.peer.notify_progress(ProgressNotificationParam::new(token, n as f64).with_total(t).with_message(..)).await` (`RS:examples/servers/src/common/progress_demo.rs:99-121`, `RS:README.md:1110-1126`).
- Handler de notification `on_cancelled(&self, CancelledNotificationParam, NotificationContext<RoleServer>)` disponible (`README.md:1140-1152`).
- **Journalisation par le SDK** : `tracing::debug!(%id, ?result, "response message")` (`service.rs:1609`) et `tracing::info!(?notification, "received notification")` (`service.rs:1625`) → en `RUST_LOG=debug` les textes juridiques complets partent dans stderr ; filtrer `rmcp=warn` par défaut.

### 2.14 Transport stdio
- `rmcp::transport::stdio()` = `(tokio::io::stdin(), tokio::io::stdout())` (`RS:crates/rmcp/src/transport/io.rs:1-6`), derrière la feature `transport-io` (`transport.rs:91-94`).
- Démarrage : `let running = server.serve(stdio()).await?; running.waiting().await?;` (`RS:examples/servers/src/calculator_stdio.rs:20-24`). `ServiceExt::serve` / `serve_with_ct(transport, CancellationToken)` (`service.rs:330-357`) ; `RunningService::waiting()` → `Result<QuitReason, JoinError>` (`service.rs:1094-1104`), `close()`/`cancel()` (`:1106-…`). `QuitReason` et `ServerInitializeError` dérivent `Debug` (`service.rs:1197-1199`, `service/server.rs:83-85`).
- **stdout est le canal JSON-RPC** : logs `tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false)` (`calculator_stdio.rs:11-15`). Contre-exemple à ne pas copier : `completion_stdio.rs:422-437` fait des `println!` sur stdout avant `serve(stdio())`.
- `IntoTransport` accepte `(AsyncRead, AsyncWrite)`, un `AsyncRead + AsyncWrite` combiné (ex. `tokio::io::duplex`), `(Sink, Stream)`, un `Worker` ou un `Transport` (`RS:crates/rmcp/README.md:62-67`).

### 2.15 Transport Streamable HTTP (serveur)
- Feature `transport-streamable-http-server` ; types `rmcp::transport::streamable_http_server::{StreamableHttpService, StreamableHttpServerConfig, session::local::LocalSessionManager, session::never::NeverSessionManager}` (`RS:crates/rmcp/src/transport/streamable_http_server.rs:1-6`, `…/session.rs:33-35`).
- `StreamableHttpService::new(service_factory: impl Fn() -> Result<S, std::io::Error> + Send + Sync + 'static, session_manager: Arc<M>, config)` avec `S: ServerHandler + Send + 'static` (`RS:crates/rmcp/src/transport/streamable_http_server/tower.rs:1413-1435`) ; c'est un **service Tower** (`impl tower_service::Service<Request<B>>`, `tower.rs:1359-1365`, `Clone` `:1347`) monté dans **axum 0.8** par `axum::Router::new().nest_service("/mcp", service)` (`RS:examples/servers/src/counter_streamhttp.rs:25-38`) ou via `hyper_util::service::TowerToHyperService` (`RS:examples/servers/src/counter_hyper_streamable_http.rs`).
- `StreamableHttpServerConfig` (`tower.rs:78-186`) : `sse_keep_alive` (défaut 15 s), `sse_retry` (3 s), `legacy_session_mode` (défaut **true**, ne concerne que les versions < 2026-07-28), `json_response` (false), `cancellation_token`, **`allowed_hosts` (défaut `["localhost","127.0.0.1","::1"]`, protection DNS-rebinding)**, `allowed_origins` (vide = pas de contrôle), `session_store`, `max_request_body_bytes`, `stateless_protocol_metadata_required` (`tower.rs:195-210`). Builders : `with_allowed_hosts`, `disable_allowed_hosts`, `with_allowed_origins`, `enforce_origin_validation`, `with_sse_keep_alive`, `with_sse_retry`, `with_legacy_session_mode`, `with_json_response`, `with_cancellation_token`, `with_max_request_body_bytes`, `with_stateless_protocol_metadata_required` (`tower.rs:213-290`). Un champ privé (`validate_empty_origin_allowlist`, `:132`) interdit la syntaxe `StreamableHttpServerConfig { …, ..Default::default() }` hors du crate (malgré l'exemple de doc `tower.rs:149-152`) → utiliser les builders.
- 2026-07-28 est servi **sans session** automatiquement (SEP-2567) ; en mode sans état, la factory est appelée **à chaque requête** → garder l'état partagé dans des `Arc` clonés (`RS:README.md:1529-1574`).
- Accès aux en-têtes HTTP dans un handler : `ctx.extensions.get::<axum::http::request::Parts>()` (`counter.rs:139-144,276-280`) — utile pour une éventuelle authentification en mode HTTP.
- **Piège Docker** : écouter sur `0.0.0.0` derrière un nom d'hôte ≠ loopback exige `with_allowed_hosts([...])`, sinon requêtes rejetées.

### 2.16 schemars et génération de schémas
- rmcp 3.5.1 exige **`schemars ^1.0`** (feature `chrono04`) (`RS:crates/rmcp/Cargo.toml:66`) ; ré-exporté `rmcp::schemars` si la feature `schemars` (incluse par `server`) est active (`RS:crates/rmcp/src/lib.rs:39-40`). schemars 0.8 est **incompatible** (traits différents). Dernière stable : **schemars 1.2.3**, rust_version 1.74, 2026-10-12 (`IDX: sc/he/schemars`).
- Génération : `SchemaSettings::draft2020_12()`, sans `AddNullable` (mot-clé OpenAPI) (`RS:crates/rmcp/src/handler/server/common.rs:16-51`), cache `thread_local` par `TypeId`.
- Le derive schemars émet des chemins relatifs `schemars::…` (`SC:schemars_derive/src/lib.rs:51-55` : alias seulement si `#[schemars(crate = "…")]`). Donc soit dépendre directement de `schemars = "1"` (choix des exemples officiels, `RS:examples/servers/Cargo.toml:38`), soit `use rmcp::schemars;` dans le module, soit `#[schemars(crate = "rmcp::schemars")]`. Recommandation : dépendance directe `schemars = "1.2.3"` (unifiée avec celle de rmcp par semver).
- Attributs utiles (`SC:docs/_includes/attributes.md`) : doc-comments → `description` ; `#[schemars(description = ..)]`, `title`, `example = valeur`, `extend("clé" = valeur)`, `inline`, `transform = f`, `length(min, max)`, `range(min, max)`, `regex(pattern = r"…")`/`pattern(..)`, `required`, `inner(..)`, `deprecated` ; respecte `#[serde(rename_all, default, skip_serializing_if, tag, flatten, …)]` ; les valeurs `default` non-`Serialize` sont simplement omises (`SC:schemars/src/_private/mod.rs:169-195`).
- schemars 1.2.3 corrige du code généré qui violait le lint `unused_qualifications` (`SC:CHANGELOG.md:3-8`) → si ce lint est activé, exiger ≥ 1.2.3.

### 2.17 Nouveautés protocole 2026-07-28 gérées par rmcp (à connaître, rarement à coder)
`server/discover`, abonnements `subscriptions/listen`, Tasks (SEP-2663), indications de cache `ttlMs`/`cacheScope` (SEP-2549), MRTR `InputRequiredResult` (SEP-2322), en-têtes HTTP standard `Mcp-Method`/`Mcp-Name`/`Mcp-Param-*` (SEP-2243, annotation `x-mcp-header` dans le schéma), HTTP sans état (SEP-2567) (`RS:README.md:16-22,1235-1590`). Pour un serveur en lecture seule : aucune action obligatoire ; éventuellement `ReadResourceResult::with_ttl_ms(..)` pour les textes consolidés.

### 2.18 Tests in-process
Motif officiel : `let (server_io, client_io) = tokio::io::duplex(4096);` serveur `X.serve(server_io)` dans une tâche, client `().serve(client_io)` (feature `client`), puis `client.list_all_tools()`, `client.call_tool(CallToolRequestParams::new("nom").with_arguments(rmcp::object!({...})))`, `client.cancel()` (`RS:crates/rmcp/tests/test_custom_request.rs:44-85`, `RS:crates/rmcp/tests/test_logging.rs:18-51`). `impl ClientHandler for ()` (`RS:crates/rmcp/src/handler/client.rs:296`). `call_tool` client → `Result<CallToolResult, ServiceError>` (`RS:crates/rmcp/src/service/client.rs:1966-1972`). `CallToolRequestParams::new(name).with_arguments(JsonObject)` (`model.rs:4246-4295`). `rmcp::object!` développe `serde_json::json!` (`model.rs:59-67`) → `serde_json` doit être une dépendance du crate de test.
Équivalent Rust du `PY:tests/tools.lock.json` (verrou des descripteurs, `PY:tests/test_descriptor_lock.py`) : snapshot `insta::assert_json_snapshot!` de `list_all_tools()` avec `sort_maps => true` (`IN:insta/src/macros.rs:203-210,499-507`, `IN:insta/src/settings.rs:253-261`) — feature insta `json` requise (`IN:insta/Cargo.toml:38`).

### 2.19 Pièges rmcp récapitulés
1. `get_info` → `ServerConfig` (pas `ServerInfo`, déprécié) ; `call_tool`/`read_resource`/`get_prompt` → `*Response` + `.into()`.
2. **Le README contient des extraits non compilables en 3.5.1** : `read_resource` retournant `ReadResourceResult` (`RS:README.md:395-434`) au lieu de `ReadResourceResponse` ; littéraux `ListResourcesResult { resources, next_cursor: None, meta: None }` (`README.md:385-392,443-449`) incomplets (manquent `result_type`, `ttl_ms`, `cache_scope`). Se fier aux exemples compilés (`examples/servers/src/common/counter.rs`).
3. `#[non_exhaustive]` partout : pas de littéraux struct pour `Tool`, `CallToolResult`, `InitializeResult`, `Implementation`, `ResourceTemplate`, `CompleteResult`, `CompletionInfo`… ; bras `_` obligatoire sur `Reference`, `CallToolResponse`, `ContentBlock`.
4. `Implementation::from_build_env()` = identité de rmcp, pas du binaire.
5. `transport-io` absent des features par défaut → `stdio` introuvable sinon.
6. Racine des paramètres = objet, sinon panic à la construction du routeur.
7. `outputSchema` détecté seulement sur `Json<T>`/`Result<Json<T>, E>` écrits littéralement.
8. `#[tool_handler]` sans `router = self.tool_router` reconstruit le routeur à chaque appel.
9. `get_info` manuel ⇒ penser à `enable_tools()` (et `enable_prompts()` si prompts).
10. Futures d'outils `+ Send` imposées : tout port appelé dans un outil doit renvoyer un futur `Send`.
11. `enable_logging()` et `notify_logging_message` dépréciés.
12. HTTP : `allowed_hosts` loopback par défaut ; config sans littéral struct.
13. Pas de `println!` : stdout = protocole.
14. Logs `debug` de rmcp contiennent les réponses complètes.
15. Les macros émettent `rmcp::…` : ne pas renommer la dépendance.

---

## 3. Squelette de code (rmcp 3.5.1) — 1 outil + 1 resource template + completions

Squelette **autonome** (un seul crate binaire) pour être « compilable en principe » sans chemins inter-crates ; les modules `domain` / `ports` / `fake` / `mcp` indiquent la future répartition dans le workspace (§5.3). Non compilé (pas d'accès à static.crates.io) : chaque appel d'API ci-dessous a été vérifié contre le code cité en §2.

### 3.1 `Cargo.toml`
```toml
[package]
name = "mcp-legifrance-skeleton"
version = "0.1.0"
edition = "2024"
rust-version = "1.88"
license = "Apache-2.0"
publish = false

[features]
default = []
http = ["rmcp/transport-streamable-http-server", "dep:axum", "dep:tokio-util", "tokio/net", "tokio/signal"]

[dependencies]
# défaut rmcp = ["base64", "macros", "server"] ; `server` active `schemars`
rmcp = { version = "3.5.1", features = ["transport-io"] }
schemars = "1.2.3"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
thiserror = "2.0.21"
anyhow = "1.0.104"
tokio = { version = "1.53.3", features = ["macros", "rt-multi-thread", "io-std"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter", "fmt"] }
axum = { version = "0.8.9", optional = true }
tokio-util = { version = "0.7.20", optional = true }

[dev-dependencies]
rmcp = { version = "3.5.1", features = ["client", "transport-io"] }
tokio = { version = "1.53.3", features = ["macros", "rt-multi-thread", "io-util"] }
insta = { version = "1.49.0", features = ["json"] }
```

### 3.2 `src/main.rs`
```rust
use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, Json, RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CompleteRequestParams, CompleteResult, CompletionInfo, ContentBlock, Implementation,
        IntoContents, ListResourceTemplatesResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Reference,
        ResourceContents, ResourceTemplate, ServerCapabilities, ServerConfig,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

use crate::domain::{Article, ArticleId};
use crate::ports::{ArticleSource, SourceError};

// ───────────────────────── crate `domain` (pur, sans I/O) ─────────────────────────
mod domain {
    use std::fmt;

    /// Identifiant d'article Légifrance (newtype validé à la frontière).
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ArticleId(String);

    #[derive(Debug, thiserror::Error)]
    #[error("identifiant d'article invalide {0:?} : préfixe LEGIARTI ou KALIARTI attendu")]
    pub struct InvalidArticleId(pub String);

    impl ArticleId {
        pub fn parse(raw: &str) -> Result<Self, InvalidArticleId> {
            let raw = raw.trim();
            let ok_prefix = raw.starts_with("LEGIARTI") || raw.starts_with("KALIARTI");
            if ok_prefix && raw.len() > 8 && raw.bytes().all(|b| b.is_ascii_alphanumeric()) {
                Ok(Self(raw.to_owned()))
            } else {
                Err(InvalidArticleId(raw.to_owned()))
            }
        }
        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl fmt::Display for ArticleId {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    #[derive(Debug, Clone)]
    pub struct Article {
        pub id: ArticleId,
        pub citation: String,
        pub source_url: String,
        pub text: String,
    }
}

// ───────────────────────── crate `application` : ports ─────────────────────────
mod ports {
    use crate::domain::{Article, ArticleId};

    #[derive(Debug, thiserror::Error)]
    pub enum SourceError {
        #[error("introuvable : {0}")]
        NotFound(String),
        #[error("erreur Légifrance/PISTE : {0}")]
        Upstream(String),
        #[error("configuration : {0}")]
        Config(String),
    }

    /// Port « driven ». RPITIT avec `+ Send` explicite : obligatoire, car rmcp boxe
    /// les futures d'outils en `Pin<Box<dyn Future + Send>>`.
    /// (`Future` est dans le prélude de l'édition 2024.)
    pub trait ArticleSource: Send + Sync + 'static {
        fn get_article(
            &self,
            id: &ArticleId,
        ) -> impl Future<Output = Result<Article, SourceError>> + Send;

        fn suggest_ids(
            &self,
            prefix: &str,
        ) -> impl Future<Output = Result<Vec<ArticleId>, SourceError>> + Send;
    }
}

// ───────────────────────── adapter factice (tests / démo) ─────────────────────────
mod fake {
    use crate::domain::{Article, ArticleId};
    use crate::ports::{ArticleSource, SourceError};

    pub struct InMemorySource;

    impl ArticleSource for InMemorySource {
        // `async fn` dans l'impl satisfait `-> impl Future + Send` du trait.
        async fn get_article(&self, id: &ArticleId) -> Result<Article, SourceError> {
            Ok(Article {
                id: id.clone(),
                citation: format!("Article {id}"),
                source_url: format!("https://www.legifrance.gouv.fr/codes/article_lc/{id}"),
                text: "Lorem ipsum. ".repeat(500),
            })
        }

        async fn suggest_ids(&self, prefix: &str) -> Result<Vec<ArticleId>, SourceError> {
            Ok(["LEGIARTI000006419292", "LEGIARTI000006419293"]
                .into_iter()
                .filter(|s| s.starts_with(prefix))
                .filter_map(|s| ArticleId::parse(s).ok())
                .collect())
        }
    }
}

// ───────────────────────── crate `adapter-mcp` ─────────────────────────
const SERVER_NAME: &str = "mcp-legifrance";
const ARTICLE_URI_PREFIX: &str = "legifrance://article/";
const ARTICLE_URI_TEMPLATE: &str = "legifrance://article/{article_id}";
const SUMMARY_CHARS: usize = 600;
const INSTRUCTIONS: &str = "Serveur MCP Légifrance (PISTE). Citer human_readable_citation et source_url. \
Ne jamais inventer d'identifiant, d'ELI ni d'ECLI.";

/// Niveau de détail : le résumé est le défaut pour économiser le contexte du LLM.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// Métadonnées + extrait court.
    #[default]
    Summary,
    /// Texte intégral, paginé par `offset` / `max_chars`.
    Full,
}

fn default_max_chars() -> usize {
    4000
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTextParams {
    /// Identifiant Légifrance de l'article (LEGIARTI… ou KALIARTI…).
    pub article_id: String,
    /// `summary` (défaut) ou `full`.
    #[serde(default)]
    pub detail: Detail,
    /// Position de départ, en caractères, dans le texte intégral (mode `full`).
    #[serde(default)]
    pub offset: usize,
    /// Nombre maximal de caractères renvoyés.
    #[serde(default = "default_max_chars")]
    #[schemars(range(min = 200, max = 20000))]
    pub max_chars: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ArticleView {
    pub article_id: String,
    pub human_readable_citation: String,
    pub source_url: String,
    /// Extrait (summary) ou tranche du texte (full).
    pub text: String,
    pub total_chars: usize,
    /// Offset à repasser pour lire la suite ; absent en fin de texte.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
}

fn article_view(article: Article, detail: Detail, offset: usize, max_chars: usize) -> ArticleView {
    let total_chars = article.text.chars().count();
    let (start, len) = match detail {
        Detail::Summary => (0, max_chars.min(SUMMARY_CHARS)),
        Detail::Full => (offset.min(total_chars), max_chars),
    };
    let text: String = article.text.chars().skip(start).take(len).collect();
    let end = start + text.chars().count();
    ArticleView {
        article_id: article.id.to_string(),
        human_readable_citation: article.citation,
        source_url: article.source_url,
        text,
        total_chars,
        next_offset: (end < total_chars).then_some(end),
    }
}

/// Erreur d'outil « in-band » (isError: true), lisible par le LLM avec un préfixe [code].
#[derive(Debug, thiserror::Error)]
pub enum ToolFailure {
    #[error("[invalid_arg] {0}")]
    InvalidArg(String),
    #[error("[not_found] {0}")]
    NotFound(String),
    #[error("[upstream_error] {0}")]
    Upstream(String),
    #[error("[config_error] {0}")]
    Config(String),
}

impl From<SourceError> for ToolFailure {
    fn from(e: SourceError) -> Self {
        match e {
            SourceError::NotFound(m) => Self::NotFound(m),
            SourceError::Upstream(m) => Self::Upstream(m),
            SourceError::Config(m) => Self::Config(m),
        }
    }
}

/// `IntoContents` ⇒ `IntoCallToolResult` (impl générique de rmcp) ; dans la branche `Err`
/// d'un `Result`, rmcp positionne `is_error = Some(true)`.
impl IntoContents for ToolFailure {
    fn into_contents(self) -> Vec<ContentBlock> {
        vec![ContentBlock::text(self.to_string())]
    }
}

pub struct LegifranceMcp<S: ArticleSource> {
    source: Arc<S>,
    tool_router: ToolRouter<Self>,
}

// Clone manuel : `#[derive(Clone)]` exigerait `S: Clone`.
impl<S: ArticleSource> Clone for LegifranceMcp<S> {
    fn clone(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            tool_router: self.tool_router.clone(),
        }
    }
}

#[tool_router]
impl<S: ArticleSource> LegifranceMcp<S> {
    pub fn new(source: Arc<S>) -> Self {
        Self {
            source,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "fr_get_text",
        description = "Lit un article Légifrance par son identifiant (LEGIARTI…/KALIARTI…). \
                       Par défaut : extrait court + citation. detail=full avec offset/max_chars \
                       pour paginer le texte intégral.",
        annotations(
            title = "Lire un article Légifrance",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fr_get_text(
        &self,
        Parameters(p): Parameters<GetTextParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ArticleView>, ToolFailure> {
        let id = ArticleId::parse(&p.article_id)
            .map_err(|e| ToolFailure::InvalidArg(e.to_string()))?;
        // Annulation coopérative : rmcp annule `ctx.ct` sur notifications/cancelled.
        let article = tokio::select! {
            res = self.source.get_article(&id) => res?,
            () = ctx.ct.cancelled() => {
                return Err(ToolFailure::Upstream("requête annulée par le client".to_owned()));
            }
        };
        Ok(Json(article_view(article, p.detail, p.offset, p.max_chars)))
    }
}

#[tool_handler(router = self.tool_router)]
impl<S: ArticleSource> ServerHandler for LegifranceMcp<S> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_completions()
                .build(),
        )
        .with_server_info(
            Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION"))
                .with_title("Légifrance (PISTE)"),
        )
        .with_instructions(INSTRUCTIONS)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new(ARTICLE_URI_TEMPLATE, "legifrance_article")
                .with_title("Article Légifrance (texte intégral)")
                .with_description("Texte verbatim d'un article identifié par LEGIARTI…/KALIARTI…")
                .with_mime_type("text/plain"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let Some(raw_id) = request.uri.strip_prefix(ARTICLE_URI_PREFIX) else {
            return Err(McpError::resource_not_found(
                "URI de ressource inconnue",
                Some(serde_json::json!({ "uri": request.uri })),
            ));
        };
        let id = ArticleId::parse(raw_id).map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let article = self.source.get_article(&id).await.map_err(|e| match e {
            SourceError::NotFound(m) => McpError::resource_not_found(m, None),
            other => McpError::internal_error(other.to_string(), None),
        })?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(article.text, request.uri.clone()).with_mime_type("text/plain"),
        ])
        .into())
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, McpError> {
        let candidates: Vec<String> = match &request.r#ref {
            Reference::Resource(template)
                if template.uri == ARTICLE_URI_TEMPLATE && request.argument.name == "article_id" =>
            {
                self.source
                    .suggest_ids(&request.argument.value)
                    .await
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect()
            }
            _ => Vec::new(), // `Reference` est #[non_exhaustive]
        };
        let total = candidates.len();
        let values: Vec<String> = candidates.into_iter().take(CompletionInfo::MAX_VALUES).collect();
        let info = CompletionInfo::with_pagination(
            values,
            u32::try_from(total).ok(),
            total > CompletionInfo::MAX_VALUES,
        )
        .map_err(|e| McpError::internal_error(e, None))?;
        Ok(CompleteResult::new(info))
    }
}

// ───────────────────────── crate `app` (composition root) ─────────────────────────
#[cfg(feature = "http")]
async fn serve_http<S: ArticleSource>(
    server: LegifranceMcp<S>,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    let ct = tokio_util::sync::CancellationToken::new();
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false) // sans état aussi pour les clients < 2026-07-28
        .with_json_response(true)
        .with_allowed_hosts(["localhost", "127.0.0.1", "::1"])
        .with_cancellation_token(ct.child_token());
    let service = StreamableHttpService::new(
        move || Ok(server.clone()), // appelé à chaque requête en mode sans état
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout = canal JSON-RPC : tous les logs sur stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,rmcp=warn")),
        )
        .init();

    let server = LegifranceMcp::new(Arc::new(fake::InMemorySource));
    let running = server.serve(stdio()).await?;
    let quit_reason = running.waiting().await?;
    tracing::info!(?quit_reason, "serveur MCP arrêté");
    Ok(())
}

#[cfg(test)]
mod tests {
    use rmcp::model::CallToolRequestParams;

    use super::*;

    #[tokio::test]
    async fn outils_resource_template_et_appels_in_process() -> anyhow::Result<()> {
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let server = LegifranceMcp::new(Arc::new(fake::InMemorySource));
        tokio::spawn(async move {
            let running = server.serve(server_io).await?;
            running.waiting().await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_io).await?;

        let tools = client.list_all_tools().await?;
        insta::with_settings!({ sort_maps => true }, {
            insta::assert_json_snapshot!("tools_list", tools);
        });

        let ok = client
            .call_tool(
                CallToolRequestParams::new("fr_get_text")
                    .with_arguments(rmcp::object!({ "article_id": "LEGIARTI000006419292" })),
            )
            .await?;
        assert_eq!(ok.is_error, Some(false));
        assert!(ok.structured_content.is_some());

        let ko = client
            .call_tool(
                CallToolRequestParams::new("fr_get_text")
                    .with_arguments(rmcp::object!({ "article_id": "XYZ" })),
            )
            .await?;
        assert_eq!(ko.is_error, Some(true)); // erreur in-band, visible par le LLM

        client.cancel().await?;
        Ok(())
    }
}
```

Points de vigilance du squelette (à vérifier à la première compilation, faute d'avoir pu exécuter `cargo build`) :
- `use crate::domain::…` en tête pendant que `domain` est déclaré plus bas dans le même fichier : légal en Rust (résolution non ordonnée).
- `tokio::select!` avec un motif `()` sur `ctx.ct.cancelled()` : le futur `WaitForCancellationFuture` a pour sortie `()` ; si le motif `()` pose problème, utiliser `_ =`.
- `insta::with_settings!` exécute son bloc dans une fermeture synchrone (`IN:insta/src/macros.rs:499-507`) : ne pas y mettre d'`.await`.
- L'en-tête `#[tool_router] impl<S: ArticleSource>` + champ `ToolRouter<Self>` générique est validé par `RS:crates/rmcp/tests/test_tool_macros.rs:98-122`.

---

## 4. Écosystème de crates (index crates.io, lu le 2026-10-15)

Colonnes : dernière version stable non yankée / `rust_version` déclaré (« — » = non déclaré) / date de publication. Source pour chaque ligne : `IDX: <chemin du crate>` (copies dans `…/scratchpad/index04/`).

### 4.1 Tableau
| Crate | Version | MSRV | Publiée | Notes / pièges |
|---|---|---|---|---|
| tokio | 1.53.3 | 1.71 | 2026-10-13 | `default = []` : activer `macros`, `rt-multi-thread`, `io-std` (stdio), `net`/`signal` (HTTP), `time`, `sync`. `tokio::io::duplex` demande `io-util`. |
| tokio-util | 0.7.20 | 1.85 | 2026-10-09 | `CancellationToken` (non ré-exporté par rmcp). |
| reqwest | 0.13.5 | 1.85 | 2026-09-08 | voir §4.2 (TLS). Features : `rustls`, `rustls-no-provider`, `native-tls*`, `json`, `form`, `query`, `http2`, `charset`, `gzip`, `brotli`, `zstd`, `deflate`, `stream`, `cookies`, `system-proxy`, `hickory-dns`, `blocking`, `multipart`, `http3`, `socks`. `default = ["default-tls","charset","http2","system-proxy"]`, `default-tls = ["rustls"]`. |
| serde | 1.0.229 | 1.56 | 2026-07-18 | |
| serde_json | 1.0.154 | 1.71 | 2026-10-11 | feature `preserve_order` unifiée dans tout le graphe → ordre des clés non garanti : `sort_maps` dans les snapshots. |
| schemars | 1.2.3 | 1.74 | 2026-10-12 | **1.x imposé par rmcp** (§2.16). Features : `derive`, `std` (défaut), `preserve_order`, `raw_value` + implicites `chrono04`, `jiff02`, `url2`, `uuid1`, `indexmap2`, `bytes1`, `either1`, `semver1`, `smallvec1`, `arrayvec07`, `bigdecimal04`, `rust_decimal1`, `smol_str02/03`. |
| thiserror | 2.0.21 | 1.77 | 2026-09-23 | rmcp utilise thiserror 2. |
| anyhow | 1.0.104 | 1.68 | 2026-07-18 | composition root uniquement. |
| secrecy | 0.10.3 | 1.60 | 2024-10-09 | `SecretString = SecretBox<str>` ; `From<String>`, `From<&str>`, `FromStr`, `Clone`, `Default` ; `Debug` affiche `[REDACTED]` ; `ExposeSecret::expose_secret()` ; `Deserialize` derrière la feature implicite `serde` (`SE:src/lib.rs:142-146,148-157,215-249,295-312`). |
| tracing | 0.1.44 | 1.65 | 2025-12-18 | |
| tracing-subscriber | 0.3.23 | 1.65 | 2026-03-13 | **`env-filter` n'est pas par défaut** (`default = smallvec, fmt, ansi, tracing-log, std`) ; feature `json` pour logs JSON. |
| moka | 0.12.16 | 1.71.1 | 2026-08-09 | **`default = []`** : activer `future` pour le cache asynchrone. |
| governor | 0.10.4 | — | 2025-12-16 | `default = std, dashmap, jitter, quanta`. |
| backon | 1.6.0 | 1.85 | 2025-10-18 | `default = std, std-blocking-sleep, tokio-sleep, gloo-timers-sleep`. |
| backoff | 0.4.0 | — | 2021-12-14 | dernière publication 2021 : à éviter (avis RUSTSEC « unmaintained » : NON VÉRIFIÉ). |
| tokio-retry | 0.3.2 | 1.85 | 2026-06-09 | alternative ; `tokio-retry2` 0.9.1 (MSRV 1.88, 2026-01-11). |
| quick-xml | 0.42.0 | 1.86 | 2026-08-22 | features `serialize`, `encoding`, `async-tokio`, `escape-html`, `overlapped-lists`, `serde-types` ; `default = []`. (adapter DILA, plus tard) |
| tantivy | 0.26.2 | 1.86 | 2026-09-08 | `default = mmap, stopwords, lz4-compression, columnar-zstd-compression, stemmer`. (adapter DILA) |
| insta | 1.49.0 | 1.66.0 | 2026-10-03 | features `json`, `redactions`, `yaml`, `filters`, `glob`… ; outil `cargo-insta` 1.49.0 (MSRV 1.66, 2026-10-03). |
| wiremock | 0.6.5 | — | 2025-08-24 | dépend de `hyper ^1.7`, `tokio ^1.47.1`. |
| nom | 8.0.0 | 1.65.0 | 2025-01-26 | |
| winnow | 1.0.4 | 1.65.0 | 2026-07-13 | 1.0 stabilisée ; candidat pour `fr_resolve` (parseur de citations). |
| regex | 1.13.1 | 1.65 | 2026-07-15 | |
| similar | 3.2.0 | 1.85 | 2026-08-17 | `fr_diff_article` ; features `text` (défaut), `inline`, `unicode`, `bytes`. |
| async-trait | 0.1.92 | 1.71 | 2026-08-08 | déjà tiré par rmcp si `transport-streamable-http-server` (§1.2). |
| trait-variant | 0.1.3 | 1.75 | 2026-07-22 | |
| dynosaur | 0.3.1 | 1.84 | 2026-07-03 | génère un wrapper `dyn` pour traits AFIT (API NON VÉRIFIÉE). |
| clap | 4.6.7 | 1.85 | 2026-09-14 | features `derive`, `env`. |
| figment | 0.10.19 | — | 2024-05-17 | |
| config | 0.15.27 | 1.88.0 | 2026-09-30 | défaut tire toml/json/yaml/ini/ron/json5 : `default-features = false`. |
| url | 2.5.8 | 1.63 | 2026-01-05 | |
| time | 0.3.55 | 1.88.0 | 2026-08-01 | 0.3.48 et 0.3.50 yankées. |
| jiff | 0.2.38 | 1.70 | 2026-10-06 | défaut `tz-system` + `tzdb-bundle-platform` : sur Linux lit `/usr/share/zoneinfo` → image Docker minimale sans tzdata : activer **`tzdb-bundle-always`** ; 0.2.36 yankée. |
| chrono | 0.4.45 | 1.62.0 | 2026-06-04 | **déjà dans le graphe via rmcp** (`RS:crates/rmcp/Cargo.toml:113-114`). |
| chrono-tz | 0.10.4 | 1.65 | 2025-07-11 | base IANA embarquée à la compilation (fraîcheur des données liée à la date de publication). |
| sha2 | 0.11.1 | 1.88 | 2026-10-12 | (rmcp dépend de `sha2 ^0.11` derrière `request-state`). API `digest 0.11` : NON VÉRIFIÉE. |
| hex | 0.4.3 | — | 2021-03-03 | |
| uuid | 1.28.0 | **1.89.0** | 2026-10-10 | **1.27.0+ exigent Rust 1.89** ; 1.26.1 (MSRV 1.85) est la dernière compatible 1.88 → avec `rust-version = "1.88"`, déclarer `uuid = "1.26"` (le résolveur v3 « préfère » mais n'interdit pas une version incompatible si l'exigence l'impose). rmcp tire `uuid ^1` (`v4`) via `server`. |
| futures | 0.3.34 | 1.71 | 2026-08-11 | |
| bytes | 1.12.1 | 1.57 | 2026-07-08 | |
| html2text | 0.17.3 | 1.85 | 2026-10-12 | `default = []` ; dépend de `html5ever ^0.40.1`. |
| scraper | 0.27.0 | — | 2026-05-11 | |
| htmd | 0.5.6 | — | 2026-10-15 | HTML→Markdown (publiée le jour de la collecte). |
| encoding_rs | 0.8.42 | 1.88 | 2026-09-24 | tiré par la feature `charset` de reqwest. |
| unicode-normalization | 0.1.25 | 1.36 | 2025-10-30 | |
| unicode-segmentation | 1.13.3 | 1.85.0 | 2026-06-01 | |
| deunicode | 1.6.2 | 1.66 | 2025-04-27 | repli des accents (recherche, BM25, complétions). |
| rust-stemmers | 1.2.0 | — | 2019-11-17 | racinisation (français inclus — NON VÉRIFIÉ). |
| bm25 | 2.3.2 | — | 2025-09-07 | `default_tokenizer` tire `cached ^0.56`, `stop-words ^0.9 (nltk)`, `rust-stemmers`, `deunicode`, `unicode-segmentation` ; `parallelism` tire rayon. |
| proptest | 1.12.0 | 1.88 | 2026-10-10 | |
| jsonschema | 0.58.6 | 1.85.0 | 2026-10-06 | valider en test le `structuredContent` contre l'`outputSchema`. |
| axum | 0.8.9 | 1.80 | 2026-04-14 | utilisé par les exemples rmcp (`axum 0.8`). |
| tower | 0.5.3 | 1.64.0 | 2026-01-12 | |
| tower-http | 0.7.1 | 1.65 | 2026-08-31 | `default = []` ; exemples rmcp : `tower-http 0.7 (cors)`. |
| hyper | 1.12.0 | 1.63 | 2026-10-06 | |
| http | 1.5.0 | 1.57.0 | 2026-07-29 | |
| rustls | 0.23.46 | 1.71 | 2026-10-14 | `default = aws_lc_rs, logging, prefer-post-quantum, std, tls12` ; feature `ring` disponible ; 0.24.0-dev.2 = préversion. |
| aws-lc-rs | 1.18.2 | 1.71.0 | 2026-10-12 | `aws-lc-sys` 0.46.0 : dépendances de build `cc`, `cmake`, `bindgen` (opt.) → chaîne C requise ; besoin exact de cmake/NASM selon la cible : NON VÉRIFIÉ. |
| ring | 0.17.14 | 1.66.0 | 2025-03-11 | |
| rustls-platform-verifier | 0.7.1 | 1.85 | 2026-09-24 | sur Unix non-Apple utilise `rustls-native-certs ^0.8` → **magasin CA système requis dans l'image Docker**. |
| webpki-root-certs | 1.0.9 | 1.70 | 2026-07-18 | racines Mozilla embarquées (pour `tls_certs_only`). |
| oauth2 | 5.0.0 | 1.65 | 2025-01-21 | probablement surdimensionné pour un simple client_credentials PISTE. |
| serde_with | 3.24.0 | 1.88 | 2026-09-26 | |
| serde_path_to_error | 0.1.20 | 1.61 | 2025-09-15 | messages de désérialisation des réponses PISTE avec le chemin JSON fautif. |
| rstest | 0.27.0 | 1.85.0 | 2026-09-06 | |
| pretty_assertions | 1.4.1 | — | 2024-09-15 | |
| tempfile | 3.27.0 | 1.63 | 2026-03-11 | |
| assert_cmd / predicates | 2.2.2 / 3.1.4 | 1.85 / 1.74 | 2026-05-11 / 2026-02-11 | tests e2e du binaire stdio. |
| tracing-appender | 0.2.5 | 1.63.0 | 2026-04-17 | |
| mockall | 0.15.0 | 1.77.0 | 2026-06-28 | facultatif (fakes écrits à la main préférables). |
| nutype | 0.8.0 | — | 2026-09-20 | newtypes validés (optionnel). |
| derive_more | 2.1.1 | 1.81.0 | 2025-12-22 | |
| bon | 3.10.2 | 1.88.0 | 2026-10-02 | builders. |
| dashmap | 6.2.1 | 1.65 | 2026-05-17 | (7.0.0-rc2 en préversion) |
| arc-swap | 1.9.2 | — | 2026-06-28 | rotation du jeton OAuth sans verrou. |
| quick_cache | 0.7.0 | 1.85 | 2026-06-27 | alternative légère à moka. |
| reqwest-middleware / reqwest-retry | 0.5.2 / 0.9.1 | — | 2026-05-19 / 2026-02-05 | compatibilité avec reqwest 0.13 : NON VÉRIFIÉE. |
| strsim | 0.11.1 | 1.56 | 2024-04-02 | suggestions « vouliez-vous dire ». |

Outils (pas des dépendances) :
| Outil | Version | Source | MSRV pour `cargo install` |
|---|---|---|---|
| cargo-nextest | 0.9.148 (2026-10-08) | `IDX` + tag Git `cargo-nextest-0.9.148` (`git ls-remote https://github.com/nextest-rs/nextest`) | **1.91** → installer en binaire (ex. action `taiki-e/install-action`, NON VÉRIFIÉ) plutôt que compiler avec la toolchain MSRV |
| cargo-deny | 0.20.2 (2026-07-09) | `IDX` + tag Git `0.20.2` (EmbarkStudios/cargo-deny) | 1.88 |
| cargo-dist (binaire `dist`) | **GitHub : v0.33.0 (2026-09-10)** ; crates.io : 0.32.0 (2026-05-22) | `CD:` (commit `a1c829ff`, « release: 0.33.0 »), `CD:CHANGELOG.md` §0.33.0 ; `IDX: ca/rg/cargo-dist` | rust-version 1.74 (`CD:Cargo.toml:16`) |
| cargo-insta | 1.49.0 (2026-10-03) | `IDX` | 1.66 |
| cargo-llvm-cov | 0.9.1 (2026-09-06) | `IDX` | 1.87 |
| cargo-machete | 0.9.2 (2026-04-15) | `IDX` | — |

cargo-dist : `dist init` (relançable) écrit la config dans `Cargo.toml`, ajoute un profil de build « shippable » et génère `.github/workflows/release.yml` ; `dist build`, `dist plan` (`CD:book/src/quickstart/rust.md:16-45,60-80`). 0.33.0 : signature Azure pour Windows, déplacement des scripts `env` des installateurs shell (`CD:CHANGELOG.md` §0.33.0).

### 4.2 Piège majeur : reqwest 0.13 et TLS
D'après `RQ:CHANGELOG.md:39-52` (v0.13.0) :
- rustls devient le backend **par défaut** (au lieu de native-tls) ; le fournisseur crypto par défaut de rustls est **aws-lc** (plus *ring*) ; `rustls-no-provider` existe pour un autre fournisseur ;
- **`rustls-tls` est renommée `rustls`** (anciens tutoriels faux) ;
- les features de racines rustls sont supprimées : **`rustls-platform-verifier`** est utilisé par défaut ; pour d'autres racines : `ClientBuilder::tls_certs_only(..)` (`RQ:src/async_impl/client.rs:1902-1906`) ; `add_root_certificate` est déprécié (`:1908-1915`) ;
- **`query` et `form` deviennent des features désactivées par défaut** → le POST `application/x-www-form-urlencoded` du jeton OAuth2 client_credentials PISTE exige `features = ["form"]` (`RQ:src/async_impl/request.rs:427-429` : `#[cfg(feature = "form")] pub fn form`).
- Définitions : `rustls = ["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]`, `rustls-no-provider = ["dep:rustls-platform-verifier", "__rustls"]` (`RQ:Cargo.toml:41-46`).
- Fournisseur : reqwest prend `rustls::crypto::CryptoProvider::get_default()` s'il a été installé, sinon aws-lc ; avec `rustls-no-provider` sans fournisseur installé → **panic** avec le message « you must install a crypto provider… » (`RQ:src/async_impl/client.rs:719-723,2498-2510`).
- Option sans aws-lc (cross-compilation plus simple, cible musl) : `reqwest = { default-features = false, features = ["rustls-no-provider", "json", "form", "http2"] }` + `rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12", "logging"] }` + au démarrage `rustls::crypto::ring::default_provider().install_default()` (API `install_default` citée dans le message d'erreur de reqwest, `RQ:src/async_impl/client.rs:2503-2506`, appliquée ici à `ring` : NON VÉRIFIÉ).
- Le transport rmcp côté serveur ne tire **pas** reqwest (seules les features `auth`/`transport-streamable-http-client-reqwest` le font, §1.2) : pas de double reqwest.

### 4.3 Autres pièges d'écosystème
- **Héritage `default-features` dans un workspace** : `default-features` au niveau du membre (avec `workspace = true`) n'est pris en compte que pour les paquets **édition 2024 avec Rust ≥ 1.99** ; avant, `default-features = false` au niveau membre « peut être ignoré ou rejeté » si la dépendance du workspace ne désactive pas déjà les features par défaut (`CG:specifying-dependencies.md:646-656`). Avec une MSRV 1.88 : mettre `default-features = false` **dans `[workspace.dependencies]`** et rajouter les features par membre (elles sont additives, `:648`).
- **Résolveur** : un workspace *virtuel* reste au résolveur 1 quel que soit l'édition des membres → écrire `resolver = "3"` (commentaire `RS:Cargo.toml:4-5`) ; le résolveur 3 (défaut édition 2024, Rust ≥ 1.84) passe `incompatible-rust-versions` de `allow` à `fallback` (préférence pour les versions compatibles avec `rust-version`) (`CG:resolver.md:235-240,549`).
- **Lints de workspace** : `[workspace.lints.<outil>]` + `[lints] workspace = true` dans chaque membre, respecté depuis Cargo 1.74 (`CG:workspaces.md:248-275`) ; `priority` (entier signé, plus bas = surchargé) pour les groupes (`CG:manifest.md:542-557`).
- `chrono` est déjà compilé à cause de rmcp ; ajouter `jiff` ou `time` = une 2ᵉ bibliothèque de dates.
- `bm25` (feature par défaut) ajoute `cached` + listes de mots vides ; pour classer quelques dizaines de passages par appel (« focus »), une implémentation maison (~100 lignes) avec `deunicode` + `unicode-segmentation` suffit.

---

## 5. Recommandations argumentées

### 5.1 Ports : AFIT natif + génériques vs `#[async_trait]` + `dyn`
**Recommandation : AFIT/RPITIT natif avec `+ Send` explicite, dispatch statique par génériques ; enum de dispatch dans la composition root pour le choix d'adapter à l'exécution ; motif « double trait » seulement si un `dyn` devient indispensable.**

Arguments :
1. rmcp lui-même suit ce modèle : `ServerHandler`/`Service` sont des traits RPITIT `-> impl Future<…> + MaybeSendFuture + '_` (`RS:crates/rmcp/src/service.rs:282-292`, `handler/server.rs:308-638`) implémentés en `async fn` (`examples/servers/src/common/counter.rs:222-283`) ; `async-trait` y est devenu optionnel (CHANGELOG 3.1.1 « make async-trait optional (#1119) ») et n'est gardé que pour l'auth (`RS:crates/rmcp/src/transport/auth.rs:10,300`), où des objets `dyn` sont stockés.
2. Contrainte dure : les outils `#[tool]` sont boxés en `Pin<Box<dyn Future + Send>>` (`rmcp-macros/src/tool.rs:273-315`) et `ServerHandler: Send + Sync + 'static` (`handler/server.rs:642`). Avec un port générique `P`, le futur de `P::get_article()` doit être prouvé `Send` → il faut l'écrire dans la signature du trait : `fn get_article(&self, id: &ArticleId) -> impl Future<Output = Result<Article, SourceError>> + Send;`. Un `async fn` nu dans le trait ne permettrait pas cette preuve en contexte générique (avertissement `async_fn_in_trait` de rustc à ce sujet : NON VÉRIFIÉ ici).
3. Les implémentations restent en `async fn` (même motif que rmcp) ; zéro allocation par appel ; erreurs de type à la compilation.
4. Décorateurs (`Audited<P>`, `Cached<P>`, `RateLimited<P>`) = structs génériques qui implémentent le même trait en déléguant ; composition statique dans `app` : `type Source = Audited<Cached<LegifranceClient>>;`. Le serveur MCP générique est validé par `RS:examples/servers/src/common/generic_service.rs:37-73`.
5. Choix d'adapter à l'exécution (PISTE vs futur adapter DILA) : `enum AnySource { Piste(..), Dila(..) }` implémentant le port par `match` (exhaustif, pas de boxing). Si un vrai `dyn` est nécessaire (registre de plugins…), copier le motif rmcp : trait statique + trait objet-sûr `DynService` à futurs boxés avec impl générique (`RS:crates/rmcp/src/service.rs:334-336,359-…,423-437`), ou `dynosaur` 0.3.1 (MSRV 1.84 ; API NON VÉRIFIÉE).
6. `#[async_trait]` + `Arc<dyn Port>` reste acceptable pour un port à faible fréquence (ex. `AuditSink`) : le coût d'un `Box` par appel est négligeable devant une requête HTTP ; mais mélanger deux styles complique les décorateurs génériques — à éviter sans raison.
7. Coût des génériques : signature `LegifranceMcp<S>` qui se propage, temps de compilation (monomorphisation). Atténuer avec des alias de type dans `app` et un seul paramètre générique « bundle de ports » (`trait Ports: Send + Sync + 'static { type Source: ArticleSource; type Audit: AuditSink; … }`).

### 5.2 Édition, MSRV, toolchain
- **Édition 2024** (celle de rmcp, `RS:Cargo.toml:12`) : résolveur 3 par défaut pour les paquets, `Future`/`IntoFuture` dans le prélude (rmcp écrit `impl Future` sans import, `handler/server.rs:3-12,313`), let-chains utilisées par rmcp (`rmcp-macros/src/tool.rs:10-14`) — cohérent avec sa MSRV 1.88 (version exacte de stabilisation des let-chains : NON VÉRIFIÉE en ligne).
- **`rust-version = "1.88"`** au niveau `[workspace.package]` : c'est le plancher imposé par rmcp 3.5.1 et par plusieurs crates récentes (time 0.3.55, sha2 0.11.1, encoding_rs 0.8.42, proptest 1.12, config 0.15.27, serde_with 3.24, bon 3.10.2 — §4.1). Attention à uuid (1.27+ = 1.89) : épingler `uuid = "1.26"` ou monter la MSRV à 1.89.
- **Toolchain épinglée** dans `rust-toolchain.toml` sur une stable récente (rmcp épingle 1.96, `RS:rust-toolchain.toml:2`). Dernière stable publiée : **1.99.0** (tag `refs/tags/1.99.0` listé par `git ls-remote --tags https://github.com/rust-lang/rust` ; annonce du 2026-10-01 selon une source secondaire trouvée par recherche web — https://www.warp2search.net/story/rust-1990-ships-with-stable-cvariadic-functions/ — date NON VÉRIFIÉE sur blog.rust-lang.org, injoignable). Avec 1.99+, `default-features` hérité au niveau membre fonctionne (`CG:specifying-dependencies.md:649-653`), mais la MSRV 1.88 l'interdit en pratique (§4.3).
- CI : un job `cargo +1.88 check --workspace --all-targets --all-features` (MSRV), un job toolchain épinglée (clippy `-D warnings`, nextest, deny, fmt). rmcp « déclare et vérifie la MSRV » (CHANGELOG 3.0.0-beta.2 #1034).
- Formatage : rmcp utilise `style_edition = "2024"`, `max_width = 100`, et des options instables nécessitant `cargo +nightly fmt` (`RS:rustfmt.toml`) ; pour nous, rester sur les options stables (`style_edition = "2024"`, `max_width = 100`).

### 5.3 Organisation du workspace
```
Cargo.toml                 # [workspace] virtuel, resolver = "3", [workspace.package], [workspace.dependencies], [workspace.lints.*]
rust-toolchain.toml        # channel épinglé (ex. "1.99.0")
deny.toml                  # cargo-deny (licences compatibles Apache-2.0, sources, advisories)
crates/
  domain/                  # pur : newtypes (ArticleId, TextId, DecisionId, Fond…), Citation, ELI/ECLI, pagination, BM25 « focus » ; deps : thiserror, (serde), unicode-*, deunicode
  application/             # ports (traits RPITIT + Send), cas d'usage, AppError ; aucun I/O ; deps : domain, thiserror, futures
  adapter-legifrance/      # reqwest 0.13 + OAuth2 client_credentials (SecretString, arc-swap/RwLock), DTO PISTE, mapping → domain ; tests wiremock + fixtures JSON de PY:tests/fixtures/
  adapter-audit/           # décorateur JSONL (tokio::fs), sha2+hex pour l'empreinte des entrées
  adapter-cache/ (ou module de application) # décorateur moka (feature "future")
  adapter-mcp/             # rmcp : serveur, DTO JsonSchema (schemars 1), ToolFailure, resources/completions/prompts ; feature "http"
  app/                     # bin : clap/config, tracing → stderr, composition des décorateurs, stdio | http
```
Règles : `schemars`/`rmcp` n'apparaissent **que** dans `adapter-mcp` (les DTO `JsonSchema` sont des vues, pas des types du domaine) ; `reqwest` que dans `adapter-legifrance` ; `tokio` absent de `domain`.

Extrait de `Cargo.toml` racine recommandé :
```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.88"
license = "Apache-2.0"

[workspace.dependencies]
domain = { path = "crates/domain" }
application = { path = "crates/application" }
rmcp = { version = "3.5.1", features = ["transport-io"] }
schemars = "1.2.3"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
tokio = { version = "1.53.3", default-features = false }      # features ajoutées par membre
tokio-util = "0.7.20"
reqwest = { version = "0.13.5", default-features = false, features = ["rustls", "json", "form", "http2", "charset", "gzip"] }
secrecy = { version = "0.10.3", features = ["serde"] }
thiserror = "2.0.21"
anyhow = "1.0.104"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter", "json"] }   # défauts conservés (fmt, ansi, std…)
moka = { version = "0.12.16", features = ["future"] }
governor = "0.10.4"
backon = "1.6.0"
url = "2.5.8"
uuid = { version = "1.26", features = ["v4", "v7", "serde"] }   # 1.27+ exige Rust 1.89
sha2 = "0.11.1"
hex = "0.4.3"
chrono = { version = "0.4.45", default-features = false, features = ["std", "clock", "serde"] }
chrono-tz = "0.10.4"
regex = "1.13.1"
winnow = "1.0.4"
similar = "3.2.0"
html2text = "0.17.3"
unicode-normalization = "0.1.25"
unicode-segmentation = "1.13.3"
deunicode = "1.6.2"
clap = { version = "4.6.7", features = ["derive", "env"] }
axum = "0.8.9"
futures = "0.3.34"
# dev
insta = { version = "1.49.0", features = ["json", "redactions"] }
wiremock = "0.6.5"
proptest = "1.12.0"
jsonschema = "0.58.6"
```
(Noms de features vérifiés dans l'index : tracing-subscriber `env-filter`, `json`, `fmt`, `ansi`, `std` ; uuid `v4`, `v7`, `serde` ; secrecy `serde` (implicite, dépendance optionnelle) ; chrono `std`, `clock` (= `winapi`, `iana-time-zone`, `now`), `now`, `serde` (implicite), défaut = `clock, std, oldtime, wasmbind` ; reqwest cf. §4.1.)

### 5.4 Lints de workspace proposés
```toml
[workspace.lints.rust]
unsafe_code = "forbid"                 # exemple de la doc Cargo (CG:workspaces.md:261-262) ; repasser à "deny" si une macro émet #[allow(unsafe_code)]
rust_2018_idioms = { level = "warn", priority = -1 }
unused_qualifications = "warn"         # nécessite schemars >= 1.2.3 (SC:CHANGELOG.md:3-8)
missing_debug_implementations = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
must_use_candidate = "allow"
print_stdout = "deny"                  # stdout = protocole MCP stdio (cf. RS:examples/servers/src/completion_stdio.rs:422-437)
dbg_macro = "deny"
unwrap_used = "deny"
expect_used = "warn"
todo = "warn"
```
Chaque membre : `[lints] workspace = true`. `unreachable_pub` est déconseillé sur `adapter-mcp` : `#[tool]` génère des `pub fn …_tool_attr()` (`rmcp-macros/src/tool.rs:129`) — bruit probable (NON VÉRIFIÉ). Comportement de `clippy::pedantic` sur le code généré par les macros rmcp : NON VÉRIFIÉ (prévoir des `#[allow]` ciblés sur les blocs `#[tool_router]`). rmcp configure `too-many-arguments-threshold = 10` (`RS:clippy.toml:1`).

### 5.5 Conseils transverses pour l'adapter MCP
- Un seul `ToolFailure` (`IntoContents`) qui reproduit les codes Python `invalid_arg | not_found | upstream_error | config_error` (`PY:src/fr_eli_mcp/server.py:66-79`) ; `Err(McpError)` réservé aux pannes de protocole.
- Descriptions longues en `const` (supporté depuis 3.4.1) dans un module dédié, verrouillées par snapshot insta de `tools/list` (équivalent de `PY:tests/tools.lock.json`).
- Annotations `read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true` sur chaque outil (parité `PY:src/fr_eli_mcp/server.py:81-86`).
- Retour progressif : `detail = summary` par défaut, `offset`/`max_chars`, `next_offset` ; envisager `ContentBlock::resource_link` vers `legifrance://…` et un `content` texte compact plutôt que le JSON dupliqué de `CallToolResult::structured` (§2.6).
- `fr_search_all` : `tokio::select!` sur `ctx.ct.cancelled()`, notifications de progression si `ctx.meta.get_progress_token()` est présent, limitation de débit `governor` dans le décorateur du port.
- Logs : `EnvFilter` par défaut `info,rmcp=warn`, `with_writer(std::io::stderr)`, pas d'ANSI.

---

## 6. Questions ouvertes / points non vérifiés
1. **Aucune compilation effectuée** : le squelette §3 et l'extrait de workspace §5.3 n'ont pas été compilés (static.crates.io bloqué). À valider en premier lieu (`cargo check`, `cargo test`).
2. Comportement des clients MCP (Claude Desktop, Claude Code, Inspector) vis-à-vis de `content` vs `structuredContent` (duplication de `CallToolResult::structured`) et du support de `resource_link` / resource templates / completions : NON VÉRIFIÉ.
3. Versions du protocole effectivement négociées par les clients cibles (2025-11-25 ? 2026-07-28 ?) : NON VÉRIFIÉ ; rmcp gère les deux.
4. Date officielle de Rust 1.99.0 (2026-10-01 selon source secondaire) : NON VÉRIFIÉE sur blog.rust-lang.org (DNS injoignable via WebFetch) ; seule l'existence du tag `1.99.0` est confirmée.
5. Exigences de build exactes d'`aws-lc-sys` 0.46 (cmake, NASM, clang selon la cible, notamment musl/aarch64 pour cargo-dist) : NON VÉRIFIÉ. Le choix `rustls` (aws-lc) vs `rustls-no-provider` + `ring` reste à trancher avec la matrice de cibles de distribution.
6. Usage de `rustls::crypto::ring::default_provider().install_default()` avec reqwest 0.13 + `rustls-no-provider` : API rustls non relue (seul le message d'erreur de reqwest cite la forme aws-lc).
7. Contenu du guide de migration 2.x→3.x (discussion GitHub #969) : non lu.
8. Avis RUSTSEC sur `backoff` (non maintenu) : NON VÉRIFIÉ.
9. API de `dynosaur` 0.3.1 et compatibilité de `reqwest-middleware`/`reqwest-retry` avec reqwest 0.13 : NON VÉRIFIÉES.
10. Données IANA embarquées par `chrono-tz` 0.10.4 (publiée 2025-07-11) vs `jiff` + `tzdb-bundle-always` : fraîcheur non vérifiée (impact faible pour Europe/Paris, règles stables — affirmation NON VÉRIFIÉE).
11. Format exact des identifiants Légifrance (longueur des chiffres après `LEGIARTI`/`KALIARTI`) : le squelette ne valide que le préfixe, comme la référence Python ; règle précise à prendre dans le rapport consacré au domaine.
12. Comportement de `clippy::pedantic`/`unreachable_pub` sur le code généré par `#[tool_router]`/`#[tool]` : à mesurer à la première compilation.
13. Le motif `tokio::select! { … () = ctx.ct.cancelled() => … }` et `insta::with_settings!` en contexte `#[tokio::test]` sont supposés corrects d'après les signatures lues, non exécutés.
