//! Les 13 outils, un fichier par outil : chacun porte son bloc `#[tool_router]`, que
//! [`LegifranceServer::new`](crate::LegifranceServer::new) additionne aux douze autres.
//!
//! # Motif `#[tool]` retenu par le spike 0A (`spikes/rmcp-stdio/README.md`, V-28 et V-29 OK)
//!
//! ```text
//! #[tool_router(router = search_router, vis = "pub(crate)")]
//! impl<C: LegalCorpus, A: AuditLog, K: Clock> LegifranceServer<C, A, K> {
//!     #[tool(
//!         name = "fr_search",
//!         title = "Search Légifrance",
//!         description = text::FR_SEARCH,                       // une constante, pas un doc comment
//!         input_schema = input_schema::<SearchParams>(),       // crate::schema, sans `$schema`
//!         output_schema = output_schema::<SearchView>(),       // idem : jamais déduit de `Json<V>`
//!         annotations(title = "Search Légifrance", read_only_hint = true, destructive_hint = false,
//!                     idempotent_hint = true, open_world_hint = true)
//!     )]
//!     async fn fr_search(&self, args: JsonObject, ct: CancellationToken)
//!         -> Result<Json<SearchView>, ToolFailure> {
//!         let call = decode::to_call::<SearchParams, SearchInput>(args);      // l'échec voyage dans `call.input`
//!         let output = self.service.search(call, ct.cancelled()).await.map_err(ToolFailure)?;
//!         Ok(Json(SearchView::from(output)))
//!     }
//! }
//! ```
//!
//! - **Pas de `Parameters<P>`** : un échec de désérialisation y serait converti en résultat par le
//!   routeur, hors audit et sans `[invalid_arg]` (ADR-0008). On reçoit les arguments bruts
//!   (`JsonObject`) et on décode nous-mêmes.
//! - **Extracteurs** : `&self`, `JsonObject` et `CancellationToken` se combinent librement. Le jeton
//!   est celui de la requête (`notifications/cancelled`, EOF) ; on le passe à la façade en
//!   `ct.cancelled()`.
//! - **Retour** : `Result<Json<V>, ToolFailure>`. L'`Err` devient `isError: true` sans
//!   `structuredContent` (`failure.rs`). Un `type` alias de `Json<V>` désactiverait la déduction
//!   du schéma de sortie : on l'écrit toujours littéralement, avec `output_schema` explicite.
//! - **Panique d'un outil** (V-36, mesurée) : rmcp ne répond rien à cet `id`. Un `catch_unwind` ou
//!   une tâche isolée autour de l'exécution est nécessaire pour obtenir une erreur et une ligne
//!   d'audit (V1-10).
//! - **Bloc vide** : `allow_empty` (rmcp-macros `tool_router.rs:21`).
//! - **Import des macros** : `#[tool]`, `#[tool_router]` et `#[tool_handler]` viennent de
//!   `rmcp_macros` (dépendance directe, épinglée `=3.5.1` comme `rmcp`, R6) et non de leur
//!   réexport par `rmcp` : la dépendance est employée, `cargo machete` n'a rien à ignorer.
//! - **Bornes génériques** : aucune n'est ajoutée par les macros ; `LegalCorpus`, `AuditLog` et
//!   `Clock` portent déjà `Send + Sync + 'static`.

mod coverage;
mod diff_article;
mod get_act;
mod get_company_agreement;
mod get_convention;
mod get_decision;
mod get_deliberation;
mod get_related;
mod get_text;
mod resolve;
mod search;
mod search_all;
mod verify_citations;
