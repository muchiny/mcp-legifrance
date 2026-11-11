# ADR-0026 — Montée de rmcp à 3.9.0 (correctif de sécurité)

- **Statut** : Accepté
- **Date** : 2026-11-11
- **Jalons concernés** : version 1.1.1
- **Questions liées** : — (décision du propriétaire du 2026-11-11 : monter plutôt qu'ajouter une exception `ignore` dans `deny.toml`)
- Amendements : AM-90 (00 §14). Révise l'épinglage d'[ADR-0003](0003-mcp-2026-07-28-dual-era-rmcp.md) (`=3.5.1` devient `=3.9.0`) ; le reste d'ADR-0003 est inchangé. Les numéros ADR-0025 et AM-89 sont réservés par la branche locale `docs/debit-rejeux-1.1.2` (rejeux de production accélérés, prévus en 1.1.2), non fusionnée : d'où 0026 et AM-90.

## Contexte

- Trois avis RustSec sont publiés contre `rmcp` 3.5.1, la version épinglée par ADR-0003 (mesure du 2026-11-11) :

  | Avis | Objet | Corrigé en |
  |---|---|---|
  | RUSTSEC-2026-0457 | client OAuth : filtre SSRF sur le nom d'hôte | 3.7.0 |
  | RUSTSEC-2026-0461 | transport Streamable HTTP serveur : cache de schémas d'outils sans borne (versions 3.0.0 à < 3.6.0) | 3.6.0 |
  | RUSTSEC-2026-0462 | clients Streamable HTTP : corps de réponse lu sans borne | 3.9.0 |

- Effet sur la CI : les trois avis font échouer `cargo deny` (CI de la PR #77, job de nuit, portes de publication). La règle maison est de corriger et jamais d'ignorer (CONTRIBUTING « Dépendances », 06 §13.1).
- **Exposition réelle.** Features de `rmcp` compilées : celles par défaut (`base64`, `macros`, `server`) plus `transport-io` ; avec notre feature `http`, `transport-streamable-http-server` ; avec `test-support` (tests seulement), `client`. Les features `auth` et `transport-streamable-http-client-*` ne sont compilées dans aucune configuration (`cargo tree --all-features`) : RUSTSEC-2026-0457 et -0462 ne touchent aucun code compilé. RUSTSEC-2026-0461 touche du code compilé seulement avec la feature `http`, absente des binaires publiés (`dist-workspace.toml` et l'image OCI : `features = ["dila"]`). Le profil HTTP local écoute sur la boucle locale seulement, refuse de démarrer sans jeton, et une couche d'authentification Bearer précède `StreamableHttpService` (`crates/lf-adapter-mcp/src/http.rs`, fonctions `router` et `authenticate`).
- Âge : `rmcp` 3.9.0 et `rmcp-macros` 3.9.0 sont publiés le 2026-11-05 ; ils ont 14 jours le 2026-11-19.

## Décision

1. **Épinglage exact `=3.9.0`** de `rmcp` et de `rmcp-macros` dans `[workspace.dependencies]` (`Cargo.toml`) et dans la table `PINNED` de `check-arch` (`xtask/src/arch.rs`). La politique d'ADR-0003 (épinglage exact des deux crates, montée manuelle et testée) est inchangée ; seule la version change.
2. **Exception `security-fix` à la règle d'âge** (00 §2, 06 §13.1, CONTRIBUTING « Dépendances ») pour `rmcp` et `rmcp-macros` 3.9.0 : publiés le 2026-11-05, éligibles le 2026-11-19. Sortie de `make min-age SECURITY_FIX=true` : 2 paquets trop récents, acceptés par l'exception. L'exception s'éteint seule le 2026-11-19.
3. **`const-oid` figé à 0.10.2** (nouvelle entrée de `Cargo.lock`, tirée par `digest` 0.11.3) : publiée le 2026-01-07, alors que la 0.10.3 date du 2026-11-10 et n'a pas l'âge. L'exception ne couvre que `rmcp` et `rmcp-macros` : la 0.10.3 n'en bénéficie pas.
4. **`sha2` admis sous `rmcp`** dans `deny.toml` (`wrappers = ["lf-adapter-audit", "rmcp"]`) : `rmcp` 3.9.0 en dépend désormais (sha2 0.11, déjà présent au verrou ; il dépend aussi de `libc`, déjà présente). Le confinement R6 est maintenu : `rmcp` est le seul autre parent admis.
5. Aucune ligne de code de production ou de test n'a été adaptée à `rmcp` 3.9.0.

## Conséquences

### Positives

- `cargo deny` (advisories, bans, licences, sources) passe sans `ignore`.
- Le profil HTTP local, construit depuis les sources avec la feature `http`, n'est plus visé par RUSTSEC-2026-0461.

### Négatives

- Une dépendance de plus dans le verrou (`const-oid` 0.10.2) et une exception à la règle d'âge, qui s'éteint le 2026-11-19.
- Les numéros de ligne de `rmcp` cités dans ADR-0003 et dans les §9.1 à §9.3 de l'annexe D sont ceux de 3.5.1 ; le relevé de l'annexe D §9.5 donne ceux de 3.9.0 pour les constats relus.

### À surveiller

- **Mesuré le 2026-11-11 avec `rmcp` 3.9.0** (Linux, toolchain 1.99.0) : compilation `--all-features --all-targets` sans changement de source ; 4 963 tests passés, 26 sautés (les mêmes qu'avant) ; clippy et rustdoc verts ; verrou des descripteurs inchangé (`lock-descriptors --check` ok ; `tools/list` 35 424 octets, 13 outils ; instructions 3 499 caractères) ; couverture à 100 % ; `cargo deny` : advisories, bans, licences, sources ok. La MSRV de `rmcp` 3.9.0 et de `rmcp-macros` 3.9.0 est 1.88 (`rust-version` de leur `Cargo.toml`), comme en 3.5.1.
- **NON VÉRIFIÉ en local** : build à la MSRV 1.88 (CI), macOS, Windows, suite de conformance HTTP (CI).
- **NON VÉRIFIÉ : pas de nouveau rejeu de production.** Le rejeu de non-régression de la 1.1.1 (porte 8 de la PR #77) a été fait avec `rmcp` 3.5.1. Le propriétaire a décidé le 2026-11-11 de ne pas le refaire. Comparaison hors ligne faite le 2026-11-11, sans appel de production (faux serveur PISTE dans un espace de noms réseau vide), entre le binaire rejoué en production (`rmcp` 3.5.1) et un binaire de la branche avec `rmcp` 3.9.0 : 275 sessions stdio (les 22 transcriptions JSON-RPC du dépôt, la surface dans les deux ères, 250 appels d'outil dans les deux ères), sorties JSON-RPC identiques à l'octet et mêmes codes de sortie sur les 275, mêmes requêtes vers l'amont. Contrôle positif : la même méthode voit 5 sessions différentes sur 126 entre la 1.1.0 et le binaire rejoué. Limites : les réponses amont sont des fixtures, seul stdio est comparé, et cela ne remplace pas un rejeu de production.
- Les constats de lecture du code de `rmcp` (annexe D §9.2) sont ceux de 3.5.1. Leur relecture sur 3.9.0, par lecture du code et sans mesure, est dans l'annexe D §9.5 (relevé du 2026-11-11) : 23 constats inchangés, 2 qui changent (constat 11 : plafond de ligne stdio de 16 Mio ; constat 7 : résultats « skills » ajoutés), et le constat 1 non corrigé en amont, donc `PreLifecycleFilter` reste.
- Procédure pour la prochaine montée : annexe D §9.4.

### Tests qui la gardent

- `make arch` : `check-arch` exige `=3.9.0` pour les deux crates (table `PINNED`, tests du module `tests` de `xtask/src/arch.rs`).
- `make deny` et `make min-age` (avec `SECURITY_FIX=true` tant que l'exception vaut).
- `cargo xtask lock-descriptors --check` (inclus dans `make lock`) : le verrou des descripteurs ne change pas.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Exception `ignore` dans `deny.toml` (aucun binaire publié n'est exposé) | La règle maison est de corriger, pas d'ignorer (CONTRIBUTING, 06 §13.1) ; `deny.toml` n'a aucun `ignore`. Le propriétaire a choisi de monter (2026-11-11). |
| Montée partielle à 3.6.0, 3.7.0 ou 3.8.0 (plus de 14 jours) | Laisse RUSTSEC-2026-0462 signalé (corrigé en 3.9.0 seulement) : `cargo deny` resterait rouge. |
| Attendre le 2026-11-19 pour monter sans exception | Le job `deny` reste rouge d'ici là (PR #77, nuit, portes de publication). |

## Références et sources

- [00-decisions.md](../plan/00-decisions.md) §2 (âge minimal), §6 (SDK), §14 (AM-90) ; [ADR-0003](0003-mcp-2026-07-28-dual-era-rmcp.md) ; [annexe D](../plan/annexes/D-mcp-2026-07-28.md) §9.
- [CONTRIBUTING.md](../../CONTRIBUTING.md) « Dépendances » ; [06-qualite.md](../plan/06-qualite.md) §13.1.
- Avis : RUSTSEC-2026-0457, RUSTSEC-2026-0461, RUSTSEC-2026-0462 (base RustSec). Code : https://github.com/modelcontextprotocol/rust-sdk (tags `rmcp-v3.5.1` et `rmcp-v3.9.0`).
