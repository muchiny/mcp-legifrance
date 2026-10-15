# ADR-0002 — Architecture hexagonale en workspace multi-crates

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M0 (workspace, `check-deps`), M1 à M4 (remplissage des crates), M9 (`lf-adapter-dila`)
- **Questions liées** : —

## Contexte

- Dans la référence, le métier, le transport MCP, l'audit et le cache sont mêlés dans `server.py`. L'audit y est recopié outil par outil (D-07), ce qui est la cause racine de D-02 et D-03. Le client HTTP et le cache y sont construits dans chaque outil (D-06, D-39) (recherche 02 §2.d, §3.4).
- **Le SDK MCP est l'élément le plus instable du système.** rmcp est passé en majeure 1.0 le 2026-03-03, 2.0 le 2026-06-29 et 3.0 le 2026-07-28. `LATEST` a changé dans une mineure, la 3.5.0 (recherche 09 §2, §3.2). La recherche 09 §7a recommande de **confiner** rmcp et de n'exposer aucun type `rmcp::model::*` hors de l'adapter.
- Une seconde source de données, les dumps DILA, est prévue en M9 derrière le même port ([ADR-0004](0004-piste-puis-dila.md)).
- La recherche 04 §5.3 propose un découpage en crates `domain`, `application`, `adapter-*` et `app`. Elle relève aussi deux pièges :
  - un workspace **virtuel** reste au résolveur 1 si `resolver = "3"` n'est pas écrit ;
  - `default-features` hérité n'est respecté au niveau d'un membre qu'à partir de Rust 1.99 en édition 2024 (recherche 04 §4.3).

## Décision

1. **Workspace Cargo virtuel**, édition 2024, avec dans le `Cargo.toml` racine :
   - `resolver = "3"` explicite ;
   - `[workspace.dependencies]` et `[workspace.lints]` (rust et clippy, dont `unsafe_code = "forbid"`) ;
   - `rust-version = "1.91"` ;
   - `default-features = false` déclaré **au niveau du workspace** quand c'est voulu (00 §2).
2. **Crates** (00 §2) :

   | Rôle | Crates |
   |---|---|
   | Cœur pur | `lf-domain` |
   | Ports, cas d'usage, `LegalService`, `Audited`, politique de budget | `lf-application` |
   | Adapters sortants | `lf-adapter-piste`, `lf-adapter-cache`, `lf-adapter-audit` ; `lf-adapter-dila` en M9 |
   | Adapter entrant | `lf-adapter-mcp` |
   | Composition root et binaire | `lf-app` |
   | Outillage | `xtask` |

   L'horloge système est un module `clock` de `lf-app`, pas un crate.
3. **Règles de dépendance** (00 §2), vérifiées en CI :

   ```mermaid
   graph TD
     app[lf-app] --> mcp[lf-adapter-mcp]
     app --> piste[lf-adapter-piste]
     app --> cache[lf-adapter-cache]
     app --> audit[lf-adapter-audit]
     app --> appl
     app --> dom
     mcp --> appl[lf-application]
     mcp --> dom[lf-domain]
     piste --> appl
     piste --> dom
     cache --> appl
     cache --> dom
     audit --> appl
     appl --> dom
   ```

   - `lf-domain` : aucun crate `lf-*`, ni tokio, reqwest, serde_json ou rmcp.
   - `lf-application` : aucun adapter, ni rmcp, ni reqwest.
   - Aucun adapter ne dépend d'un autre ; seul `lf-app` dépend de tout.
   - `lf-adapter-mcp` ne dépend **pas** de `lf-app` : l'inversion proposée par le squelette de la recherche 09 §8 est écartée. `lf-app` ne voit pas rmcp : il appelle `lf_adapter_mcp::serve_stdio`, dont l'API publique ne contient aucun type du SDK (02 §12.3).
4. **Contrôle** : `cargo xtask check-deps` lit `cargo metadata` et applique les règles écrites comme des données. `cargo deny check bans` sert de seconde ligne (02 §4.2, §4.3).
5. **Ports** : les ports vivent dans `lf-application::ports`, à côté de leurs consommateurs. Le style est fixé par [ADR-0012](0012-ports-async-rpitit.md).

## Conséquences

### Positives

- Les frontières sont garanties par Cargo et par `check-deps`, pas seulement par la revue (02 §13).
- **Changer de SDK ne touche que `lf-adapter-mcp`**. Le repli prévu est `pmcp` (02 §12.3, recherche 09 §7b). Les oracles boîte noire (`tests/tools.lock.json`, transcriptions JSON-RPC) ne dépendent pas du SDK.
- Le domaine se teste sans runtime ni réseau (proptest, golden). Chaque adapter se teste isolément (wiremock, fichiers temporaires).

### Négatives

- **Plus de surface** : huit manifestes, des alias de types génériques dans `lf-app`, des temps de compilation plus longs (monomorphisation, ADR-0012).
- **Publication** : le canal `cargo install` impose de publier les sept crates `lf-*` sur crates.io (08 §12.1 n° 2, [ADR-0013](0013-distribution.md)).
- Les DTO MCP (schemars) et les DTO PISTE (serde) dupliquent en partie les entités du domaine : c'est le prix du découplage, assumé.

### À surveiller

- Le comportement de `forbid(unsafe_code)` et de clippy pedantic sur le code généré par les macros rmcp : **NON VÉRIFIÉ** [v:rmcp-macros-unsafe-forbid], à mesurer au spike M0.
- **Interprétation à confirmer** : `lf-application` utilise `tokio` (features `time` et `macros` seulement) et `futures` pour le délai global et `select!` dans `Audited`. La colonne « Dépend de » de 00 §2 ne liste que les crates internes. `check-deps` borne ces features (02 §13.1 n° 7, ADR-0012).

### Tests qui la gardent

- `cargo xtask check-deps`, avec les tests de son propre moteur (06 §2.8).
- `cargo deny check bans`.
- Contrôles de pureté (02 §4.4).
- `every_tool_goes_through_audited` (D-07).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Un seul crate découpé en modules | Les frontières ne seraient gardées que par la revue ; rmcp et reqwest seraient visibles partout. |
| Crate `lf-ports` séparé | Éloigne les ports de leurs consommateurs (les cas d'usage) sans rien apporter (02 §13). |
| Un crate par port | Trop de crates pour trois traits courts. |
| Crate dédié pour `SystemClock` | Un crate pour une vingtaine de lignes (00 §2). |
| `lf-adapter-mcp` dépendant de `lf-app` (recherche 09 §8) | Inverse la composition root ; `lf-app` connaîtrait rmcp. |
| Outils tiers de vérification d'architecture | Une dépendance de CI de plus ; règles moins lisibles qu'une table dans `xtask` (02 §13). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §2, §3 ; [02-architecture.md](../plan/02-architecture.md) §1 à §4, §12.3, §13, §13.1 ; [06-qualite.md](../plan/06-qualite.md) §2.8, §8.1.
- Recherche : [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §4.3, §5.3 ; [09-rmcp-2026.md](../recherche/09-rmcp-2026.md) §2, §3.2, §7a, §7b, §8 ; [02-defauts.md](../recherche/02-defauts.md) D-06, D-07, D-39.
- Alistair Cockburn, « Hexagonal architecture » (2005) : https://alistair.cockburn.us/hexagonal-architecture/
