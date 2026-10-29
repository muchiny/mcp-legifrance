# Spike rmcp 3.5.1 : serveur stdio générique (unité 0A)

Crate hors workspace (son `Cargo.toml` porte son propre `[workspace]` et son propre verrou).
Il éprouve, sur la chaîne réelle (`lf-application` : `ToolCall`, `CanonicalInput`, `Audited`,
`MemoryAuditLog`), les motifs rmcp que copieront `lf-adapter-mcp` (0M, 0W) et les tranches
d'outils du plan « fin du projet » (T-M0-13, T-M0-14).

```bash
CARGO_TARGET_DIR=/var/tmp/lf-fin/t-0A /var/tmp/lf-fin/lf-cargo test   --manifest-path spikes/rmcp-stdio/Cargo.toml --locked
CARGO_TARGET_DIR=/var/tmp/lf-fin/t-0A /var/tmp/lf-fin/lf-cargo clippy --manifest-path spikes/rmcp-stdio/Cargo.toml --all-targets --locked -- -D warnings
```

Les sources citées sont `rmcp-3.5.1/src/…` et `rmcp-macros-3.5.1/src/…` (registre cargo local).
`rmcp` et `rmcp-macros` 3.5.1 ont été publiés le 2026-10-05 sur crates.io (non retirés), soit 24
jours au 2026-10-29 : la règle d'âge de 14 jours est tenue.

## Verdict

| Vérification | Verdict | Preuve |
|---|---|---|
| **V-28** : `JsonObject` en entrée, `input_schema` et `output_schema` explicites, retour `Json<T>` | **OK** | `jsonobject_tool_receives_raw_arguments`, `invalid_arguments_reach_the_tool`, `tools_list_has_explicit_input_and_output_schema` |
| **V-29** : `PreLifecycleFilter` autour d'`AsyncRwTransport::new_server` accepté par `serve_with_ct` | **OK** | `notification_before_initialize_is_ignored`, `eof_exits_zero` |
| Génériques (`Spike<C: LegalCorpus, A: AuditLog>`), routeurs combinés par `+`, `#[tool_handler(router = self.tool_router)]` | **OK** | `generic_server_combines_routers`, `tool_handler_dispatches_to_router_field` |
| V-15 : macros rmcp sous `unsafe_code = "forbid"` | **OK** | le crate compile sous `forbid`, aucune dérogation |
| V-36 : panique dans un outil | **silence, le service survit** (voir plus bas) | `test_route_can_be_added` |

Aucun repli du §3 (0A) n'est nécessaire : `Parameters<P>`, les routes à la main et l'absence de
filtre restent des options écartées.

## Motif exact du `#[tool]` (à copier)

```rust
pub struct Spike<C: LegalCorpus, A: AuditLog> {   // bornes sur la struct, comme `Audited<A>`
    corpus: Arc<C>,
    audited: Arc<Audited<A>>,
    tool_router: ToolRouter<Self>,
}

impl<C: LegalCorpus, A: AuditLog> Spike<C, A> {
    pub fn new(corpus: Arc<C>, audited: Arc<Audited<A>>) -> Self {
        Self { corpus, audited,
               tool_router: Self::search_router() + Self::coverage_router() + Self::empty_router() }
    }
}

#[tool_router(router = search_router, vis = "pub(crate)")]
impl<C: LegalCorpus, A: AuditLog> Spike<C, A> {
    #[tool(
        name = "fr_search",
        title = "Search Légifrance (spike)",
        description = FR_SEARCH_DESCRIPTION,                 // une constante, pas un doc comment
        input_schema = input_schema::<SearchParams>(),       // helper local, voir plus bas
        output_schema = output_schema::<SearchView>(),
        annotations(title = "…", read_only_hint = true, destructive_hint = false,
                    idempotent_hint = true, open_world_hint = true)
    )]
    async fn fr_search(&self, args: JsonObject, ct: CancellationToken)
        -> Result<Json<SearchView>, ToolFailure> { /* … */ }
}

// Un bloc vide (forme des stubs de 0W) : `allow_empty`, rmcp-macros-3.5.1/src/tool_router.rs:21.
#[tool_router(router = empty_router, vis = "pub(crate)", allow_empty)]
impl<C: LegalCorpus, A: AuditLog> Spike<C, A> {}

#[expect(clippy::unused_async_trait_impl, reason = "méthodes async générées, sans .await")]
#[tool_handler(router = self.tool_router)]   // ToolHandlerAttribute { router: Expr }, tool_handler.rs:10-12
impl<C: LegalCorpus, A: AuditLog> ServerHandler for Spike<C, A> { /* get_info, supported_protocol_versions */ }
```

Points à retenir :

- **Bornes génériques.** Aucune borne n'est ajoutée par les macros. `ToolRouter<S>` exige
  `S: MaybeSend + 'static`, soit `Send + Sync + 'static` sans la feature `local`
  (`router/tool.rs:385-388`, `service.rs:20-22`) ; `LegalCorpus` et `AuditLog` portent déjà
  `Send + Sync + 'static` (`ports/corpus.rs:21`, `ports/audit.rs:23`). Les `impl<C: LegalCorpus,
  A: AuditLog>` des trois blocs, plus celui de `ServerHandler`, suffisent.
- **Extracteurs d'un outil.** `&self`, `JsonObject` (`tool.rs:253-258` ; `arguments` absent donne un
  objet vide) et `CancellationToken` (`handler/server/common.rs:161-168`, le `ct` de la requête)
  se combinent librement. Le retour `Result<Json<V>, E>` fonctionne dès que `E: IntoContents` :
  l'`Err` devient `isError: true` sans `structuredContent` (`tool.rs:124-146`).
- **`description = CONST`.** L'attribut explicite l'emporte sur le doc comment de la fonction
  (`rmcp-macros-3.5.1/src/tool.rs:258-260`). Un outil sans `description` prend son doc comment.
- **`input_schema` explicite.** `schema_for_input::<P>()` rend un `Result` (`common.rs:77`) : le
  helper `input_schema::<P>()` du spike le déballe (panique de montage, `#[expect(clippy::panic)]`)
  et retire `$schema`. `output_schema::<V>()` fait de même (`common.rs:121`).
  Sans `output_schema` explicite, rmcp déduit le schéma du type de retour `Json<V>` **avec**
  `$schema` (cas de `fr_coverage`, vérifié par le test) : d'où l'attribut explicite (05 §2.5).
- **Ordre d'`#[expect]`.** `#[expect(…)]` posé **avant** `#[tool_handler]` est conservé par la macro.

## Lints : bruit de `clippy::pedantic`

Le crate est sous `pedantic = warn` + `-D warnings`. Trois sujets, tous résolus dans le spike :

1. `#[tool_handler]` génère des `async fn` sans `.await` : `clippy::unused_async_trait_impl` se
   déclenche sur le site de la macro. À neutraliser par `#[expect(clippy::unused_async_trait_impl,
   reason = …)]` sur l'`impl ServerHandler` (0M et 0W).
2. `ServerInitializeError` pèse 368 octets : un `enum ServeError { Initialize(ServerInitializeError), … }`
   déclenche `large_enum_variant` et `result_large_err`. Le spike boîte la variante
   (`Initialize(Box<ServerInitializeError>)`).
3. Le helper `input_schema` panique au montage : `#[expect(clippy::panic, reason = …)]`.

Les `#[tool_router]` et `#[tool]` ne produisent aucun avertissement pedantic.

## `ToolRouter` : ajouter une route depuis un test (pour V1-10)

`ToolRouter<S>` implémente `Add` et `AddAssign` (`router/tool.rs:604`, `:616`). Son champ `map`
est public. Les méthodes utiles (`router/tool.rs:385-600`) :

| Méthode | Rôle |
|---|---|
| `add_route(&mut self, ToolRoute<S>)` (`:436`) | ajoute ou remplace une route (le nom est la clé) |
| `merge(&mut self, ToolRouter<S>)` (`:442`) | fusionne un autre routeur |
| `remove_route(&mut self, &str)` (`:456`) | retire une route ; l'appel rend alors `-32602` « tool not found » |
| `has_route`, `get`, `list_all` (`:462`, `:596`, `:581`) | lecture ; `list_all` est trié par nom |

Deux fabriques de `ToolRoute<S>` (`router/tool.rs:184-215`) :

```rust
// Route écrite à la main : le futur est boxé, `context.arguments` donne les arguments bruts.
fn echo_route(context: ToolCallContext<'_, TestSpike>) -> Pin<Box<dyn Future<Output = Result<CallToolResponse, ErrorData>> + Send + '_>> { … }
spike.router_mut().add_route(ToolRoute::new_dyn(
    Tool::new("echo", "echo the arguments", schema_for_empty_input()), echo_route));

// Route à extracteurs typés (ici `Parameters<P>`, pour le contraste) :
spike.router_mut().add_route(ToolRoute::new(Tool::new("typed", "typed", input_schema::<P>()), typed_tool));
```

Le routeur du **champ** est le seul que lit `#[tool_handler(router = self.tool_router)]` : un
`&mut` sur ce champ (accesseur de test) suffit, et un routeur reconstruit par
`Self::search_router()` ne verrait pas la route ajoutée (`tool_handler_dispatches_to_router_field`).

## Arguments invalides : la forme de l'erreur

Avec `JsonObject`, le décodage est le nôtre (`serde_path_to_error` dans `to_call`) et l'échec
voyage jusqu'à `Audited::run` : réponse **in-band**, `isError: true`, **sans** `structuredContent`,
un seul bloc texte `[code] message`, une ligne d'audit en `invalid_arg`.

| Entrée | Texte rendu |
|---|---|
| `{"page_size": "x"}` | `[invalid_arg] page_size: invalid type: string "x", expected i64` |
| `{"query": "bail", "foo": 1}` | `[invalid_arg] foo: unknown field `foo`, expected `query` or `page_size`` |
| `{}` | `[invalid_arg] query is required.` |
| `arguments` absent | idem `{}` (objet vide, `tool.rs:253-258`) |
| `arguments` non objet (`"bail"`) | erreur JSON-RPC `-32601`, **avant** notre code, **sans** audit (rmcp #1347, 09 R-02) |

Contraste avec `Parameters<P>` : le message `failed to deserialize parameters: …`
(`tool.rs:235-247`) est converti en `isError: true` par le routeur (`router/tool.rs:146-155`, `:575`),
mais sans code `[invalid_arg]`, sans chemin de champ et **hors de tout audit**. C'est le motif
retenu contre `Parameters<P>` (ADR-0008, D-02).

## Annulation : rmcp jette la réponse

`ct: CancellationToken` est le jeton de la requête : `notifications/cancelled` l'annule (et l'EOF,
voir plus bas). Passé tel quel en `ct.cancelled()` au paramètre `cancel` d'`Audited::run`
(`audited.rs:140`), il abandonne le travail et produit la ligne d'audit
`status = error`, `error_code = None`, `error = "cancelled by client"`
(`cancellation_reaches_audited_run`, moins de 2 s pour une latence simulée de 30 s).

**rmcp 3.5.1 n'envoie pas la réponse d'une requête annulée** : `service.rs:1503-1505` journalise
« dropping response for cancelled request » et passe. Aucune ligne n'apparaît sur stdout pour cet
`id` (le test envoie un `ping` ensuite et vérifie qu'aucune autre ligne n'est sortie). La ligne
d'audit est donc la seule trace de l'appel annulé : l'invariant « une ligne d'audit par appel »
tient bien, il n'y a rien à ajouter côté réponse.

## Panique dans un outil (V-36, mesure)

Une route qui panique (`boom_route`) ne produit **aucune** réponse pour son `id` (la tâche de la
requête meurt, `service.rs:1603-1618`), mais le service continue : le `ping` suivant est servi, et
aucune erreur `-32603` n'est émise. Un client qui attend cet `id` reste donc pendu. `Audited::run`
laisse traverser la panique sans ligne d'audit (09, ligne 1495). Conclusion pour 0M et la couche
rmcp de `lf-app` : un `catch_unwind` (ou une tâche isolée) **autour de l'exécution de l'outil**
est nécessaire pour obtenir une réponse d'erreur et une ligne d'audit.

## Cycle de vie stdio

`PreLifecycleFilter<T>` (`src/lib.rs`) enveloppe `AsyncRwTransport::new_server(read, write)` et
implémente `Transport<RoleServer>` ; il tient sous `forbid(unsafe_code)`.

- Sans filtre, une notification, une réponse ou une erreur reçue avant `initialize` arrête le
  service (`ServerInitializeError::ExpectedInitializeRequest`, `service/server.rs:681-685`) ; le
  test le démontre par contraste. Avec le filtre, ces messages sont écartés ; un `ping` avant toute
  ère reçoit `{}` de rmcp lui-même (`service/server.rs:582-593`) ; `initialize` ouvre l'ère.
  Une requête autre que `server/discover` et `ping`, dont le `_meta` est complet et la version
  dans `SUPPORTED`, l'ouvre aussi (`opens_lifecycle`, miroir de `service/server.rs:597-668`).
- **EOF.** `cancel_on_eof(token)` annule le jeton du service quand `receive()` rend `None`. Sans
  cela, l'EOF clôt le service en `QuitReason::Closed` et rmcp attend **5 s** la réponse d'un appel
  en vol (`service.rs:1762-1766` ; mesuré par `eof_without_cancel_on_eof_waits_for_the_drain`).
  Avec lui : fin en moins d'1 s, même avec un appel de 30 s en vol (`eof_exits_zero`), et le
  binaire sort avec le code 0, stdout ne portant que le JSON-RPC.
- `serve_io` distingue `ServeOutcome::Eof` de `ServeOutcome::Shutdown` par un jeton parent
  (`shutdown`) et un jeton enfant (`serving`) : l'EOF n'annule que l'enfant.

## Taille du `tools/list` (deux outils)

Mesurée sur le binaire (`tools_list_has_explicit_input_and_output_schema`) :

| Mesure | Octets |
|---|---|
| ligne JSON-RPC complète de la réponse `tools/list` | **1 266** |
| tableau `tools` seul (deux outils) | **1 222** |
| `fr_search` (schémas d'entrée et de sortie explicites, annotations, titre) | environ 846 |
| `fr_coverage` (schéma de sortie déduit, avec `$schema`, 57 octets) | environ 373 |

Ordre de grandeur seulement : les vraies descriptions (05) sont bien plus longues que celles du
spike. Le test fige ces deux nombres comme documentation, il faudra les mettre à jour si le spike
change.

## Tests

| Test | Point éprouvé |
|---|---|
| `generic_server_combines_routers` | trois blocs `#[tool_router]` dont un vide, combinés par `+`, sur un serveur générique |
| `tool_handler_dispatches_to_router_field` | `#[tool_handler(router = self.tool_router)]` lit le champ |
| `jsonobject_tool_receives_raw_arguments` | arguments bruts, audit en JSON canonique, `Json<T>` en `structuredContent` et en texte |
| `invalid_arguments_reach_the_tool` | pas de « failed to deserialize parameters » ; contraste avec `Parameters<P>` |
| `test_route_can_be_added` | API de `ToolRouter`, route à la main, panique d'un outil |
| `tools_list_has_explicit_input_and_output_schema` | schémas explicites, sans `$schema`, taille |
| `cancellation_reaches_audited_run` | jeton d'annulation jusqu'à `Audited::run`, réponse jetée |
| `notification_before_initialize_is_ignored` | `PreLifecycleFilter`, avec contraste sans filtre |
| `eof_exits_zero` | EOF avant l'ère, après, avec appel en vol, arrêt demandé, binaire |
| `eof_without_cancel_on_eof_waits_for_the_drain` | contraste : sans `cancel_on_eof`, 5 s d'attente |
