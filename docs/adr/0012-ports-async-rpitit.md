# ADR-0012 — Ports en async natif (RPITIT) et génériques

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M0 (compilation du squelette), M3 (ports, faits avant l'adapter de M2 : T-M3-01, 00 §11 ; `LegalService`, décorateurs), M9 (`AnyCorpus`)
- **Questions liées** : —
- Révisé le 2026-10-15 (revue) : délai global par l'échéance `Deadline` en variable locale de tâche, sans paramètre ajouté aux ports ; liste fermée des crates externes de `lf-application`, dont tokio `rt` pour `task_local!` seulement (00 §2-§3, AM-07) ; contrôle final du 2026-10-16 : liste cible, dépendances déclarées seulement quand un code les emploie (00 §2).

## Contexte

- **rmcp suit déjà ce modèle** (recherche 04 §5.1) :
  - `ServerHandler` et `Service` sont des traits dont les méthodes renvoient `impl Future<…>`, implémentées par des `async fn` (`handler/server.rs:308-638`, `service.rs:282-292`) ;
  - `async-trait` y est devenu optionnel en 3.1.1 (#1119), et ne reste que pour l'authentification, où des objets `dyn` sont stockés.
- **Les futurs doivent être `Send`.** Les outils `#[tool]` sont boxés en `Pin<Box<dyn Future + Send>>`, et `ServerHandler: Send + Sync + 'static` (`rmcp-macros/src/tool.rs:273-315`, `handler/server.rs:642`).
  - Le futur d'une méthode d'un port générique doit donc être prouvé `Send` à la compilation.
  - Un `async fn` nu dans le trait ne permet pas de l'exiger côté appelant générique. Il faut écrire `+ Send` dans la signature (recherche 04 §5.1, point 2).
- **Stabilité de la syntaxe.** Les traits à `async fn` et à `impl Trait` en retour sont stables depuis Rust 1.75 (https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits.html) ; la MSRV du projet est 1.91 (00 §2).
- **Besoins de composition** :
  - des décorateurs génériques : `CachedCorpus<C>` ([ADR-0007](0007-cache-memoire-port.md)) et `Audited` ([ADR-0008](0008-audit-decorateur-strict.md)) ;
  - un choix d'adapter à l'exécution en M9 ([ADR-0004](0004-piste-puis-dila.md)).
- **Proposition écartée.** La recherche 07 §9.2 proposait un port `CitationResolver` en `async_trait` (annexe B §6.6).
- **Délai global.** Les signatures des ports n'ont pas de paramètre de délai (00 §3), mais l'adapter PISTE doit plafonner ses réessais et ses attentes par le délai de l'outil (00 §8 ; D-14). `tokio::task_local!` n'existe qu'avec la feature tokio `rt` (documentation de tokio, « Available on crate feature rt only » ; à confirmer au spike M0, 00 §11).

## Décision

1. **Ports sortants** dans `lf-application::ports` : `LegalCorpus`, `AuditLog` et `Clock` (00 §3).
   - Les méthodes asynchrones sont déclarées en RPITIT avec `+ Send` et implémentées par des `async fn`.
   - Les traits sont `Send + Sync + 'static`.
   - `LegalCorpus` n'a **aucune méthode par défaut**.
   - `Clock` est synchrone (`now_utc()`, `today_paris()`).

   ```rust
   // non compilé, à valider en M0
   pub trait LegalCorpus: Send + Sync + 'static {
       fn get_decision(&self, id: &DecisionId)
           -> impl Future<Output = Result<Decision, CorpusError>> + Send;
       // … search, get_act, get_article, … (00 §3, 02 §5.1)
   }
   impl LegalCorpus for PisteCorpus {
       async fn get_decision(&self, id: &DecisionId) -> Result<Decision, CorpusError> { /* … */ }
   }
   ```

2. **Dispatch statique par génériques** :
   - `LegalService<C: LegalCorpus, A: AuditLog, K: Clock>` ;
   - alias dans `lf-app` : `type Corpus = CachedCorpus<PisteCorpus>; type Service = LegalService<Corpus, JsonlAuditLog, SystemClock>;` (02 §8.4).
3. **Choix à l'exécution** : un `enum AnyCorpus` dans `lf-app` (M9), qui implémente le port par un `match` sur chaque méthode.
4. **Côté entrant** : pas de trait. Chaque cas d'usage est un `…UseCase` avec `async fn execute(&self, input) -> Result<Output, AppError>`, regroupé dans `LegalService` (00 §3).
5. **Ni `async_trait` ni `dyn`**, sauf justification dans un **nouvel ADR**. Aucune exception n'est prévue en v0.x.
6. **Runtime dans l'application.** Les crates externes de `lf-application` forment une liste fermée (00 §2), vérifiée par `cargo xtask check-deps` (règle R2 de 02 §4.1, §13.1 n° 7) : `thiserror`, `chrono` (`std`), `tracing`, `futures` (`std`, `async-await`), `tokio` (`time`, `macros`, `rt` — `rt` pour `task_local!` seulement), `similar` (M7). Sont interdits : `serde`, `serde_json`, `rmcp`, `reqwest`, `moka`, `governor`, `backon`, `async-trait`. Aucune E/S n'y est faite, aucune tâche n'y est lancée, aucun runtime n'y est construit. Cette liste décrit la cible : en M0, le squelette ne déclare une dépendance que lorsqu'un code l'emploie (00 §2).
7. **Délai global** (00 §3) :
   - `Audited` mesure la durée (`tokio::time::Instant`), exécute le cas d'usage sous `tokio::time::timeout(LF_TOOL_DEADLINE_S)`, seule garde dure, et publie l'échéance dans `lf_application::deadline::Deadline`, variable locale de tâche (`tokio::task_local!`) ;
   - les adapters la lisent (`Deadline::current_or(défaut)`) comme une **durée restante** (`Duration`), convertie dans leur propre horloge monotone à l'entrée de la méthode du port, ce qui garde cohérentes leurs horloges de test ; ils ne programment aucune tentative ni attente qui la dépasserait et rendent tout de suite une erreur explicite (`RateLimited { retry_after }`, `Timeout` ou dernière erreur) ;
   - les cas d'usage n'utilisent pas `tokio::spawn` (sinon, propagation explicite) ; ceux à appels multiples cessent d'en lancer à 80 % du délai et rendent un résultat partiel motivé ;
   - dépassement → `upstream_error` (`UpstreamCause::Deadline`).

## Conséquences

### Positives

- **Zéro allocation par appel**, et les erreurs de type sont détectées à la compilation.
- `Send` est prouvé statiquement, ce qu'exigent rmcp et le runtime multi-thread (02 §10).
- Le `match` de `AnyCorpus` est exhaustif. Un décorateur qui oublie de déléguer une méthode ne compile pas.
- Même modèle que le SDK, donc pas de pont entre deux styles d'async.

### Négatives

- **Les génériques se propagent** dans `LegalService` et dans le serveur MCP. La monomorphisation allonge la compilation, ce que les alias de `lf-app` atténuent.
- **Les traits ne sont pas objet-sûrs** : pas de `dyn LegalCorpus`. Les doublures de test sont des types concrets (`FakeCorpus`), ce qui est voulu.
- Les messages d'erreur du compilateur sur les bornes `Send` sont parfois verbeux.

### À surveiller

- **NON VÉRIFIÉ** [v:squelette-compile-m0] : le squelette (traits, décorateurs, `LegalService` dans un `ServerHandler` rmcp) compile, et ses futurs sont `Send`, au spike M0.
- Si un vrai `dyn` devient nécessaire (registre d'extensions), on reprend le motif de rmcp : un trait statique, plus un trait objet-sûr à futurs boxés avec une implémentation générique (`service.rs:334-336`, recherche 04 §5.1, point 5). Cela passe par un nouvel ADR.
- Le nombre de paramètres génériques : au-delà de trois, regrouper les ports dans un trait « bundle » à types associés (recherche 04 §5.1, point 7).
- `Deadline` n'est visible que dans la tâche qui exécute le cas d'usage : un `tokio::spawn` ajouté plus tard perdrait l'échéance sans erreur de compilation. Le test `deadline_is_published_to_the_port` et la revue (02 §7.1) le gardent.

### Tests qui la gardent

- Suite de contrat du port exécutée sur `FakeCorpus` et `PisteCorpus` (06 §14 M3), puis sur `DilaCorpus` en M9.
- Assertion de compilation `Send` sur les futurs des cas d'usage (proposition : `fn assert_send<T: Send>(_: &T)`).
- `cargo xtask check-deps` : liste fermée des crates externes et features de tokio admises dans `lf-application` ; `cargo check -p lf-application` seul (02 §4.4).
- `deadline_is_published_to_the_port`, `deadline_maps_to_upstream_error_before_deadline_plus_epsilon`, `multi_call_use_case_stops_at_80_percent` (application) ; `deadline_from_task_local_is_used`, `no_deadline_published_uses_tool_deadline`, `tool_deadline_caps_retries` (adapter PISTE) (06 §2.2, §2.3).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| `#[async_trait]` avec `Arc<dyn Port>` | Une allocation par appel. Mélanger deux styles complique les décorateurs génériques (recherche 04 §5.1, point 6). |
| `trait-variant` (variante `Send` générée) | Dépendance supplémentaire, pour un besoin couvert par `+ Send` écrit à la main sur un seul trait principal. |
| `dynosaur` (dyn sur RPITIT) | Aucun besoin de `dyn` en v0.x. API de la 0.3.1 non vérifiée (recherche 04 §6, point 9). |
| `Box<dyn LegalCorpus>` pour PISTE et DILA | Exige l'objet-sûreté et le boxing des futurs ; l'enum suffit pour deux variantes (02 §13). |
| Port `CitationResolver` séparé, en `async_trait` (recherche 07 §9.2) | 00 §3 impose un port sortant unique. La résolution est un service de l'application (annexe B §6.6). |
| Méthode `sleep` dans `Clock` au lieu de `tokio::time` | Modifierait le port normatif de 00 §3 (02 §13). |
| Paramètre de délai dans chaque méthode du port | Touche tous les cas d'usage, `CachedCorpus`, `AnyCorpus` et les faux, pour un effet obtenu par `Deadline` (AM-07). |
| Garde extérieure seule, ou budget de l'adapter mesuré depuis le début de l'appel au port | L'adapter programmerait des attentes que la garde couperait (« tool deadline exceeded » au lieu de `rate_limited` avec délai) et ignorerait le temps déjà consommé par un cas d'usage à appels multiples (02 §13). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §2, §3 ; [02-architecture.md](../plan/02-architecture.md) §5, §6.3, §8.4, §8.5, §10, §13, §13.1 n° 7 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §1.3 ; [annexes/B-citations.md](../plan/annexes/B-citations.md) (résolution sans port dédié) ; [06-qualite.md](../plan/06-qualite.md) §2.2, §14.
- Recherche : [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §2.2, §2.4, §5.1, §6 ; [07-citations.md](../recherche/07-citations.md) §9.2.
- Code : https://github.com/modelcontextprotocol/rust-sdk (tag `rmcp-v3.5.1`) ; billet Rust « Announcing `async fn` and return-position `impl Trait` in traits » (2023-12-21) : https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits.html.
