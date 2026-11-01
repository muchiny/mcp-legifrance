# ADR-0021 — Nettoyages de schéma classés en correctif par le verrou des descripteurs

- **Statut** : **Accepté** (2026-11-01, plan de correction 1.0.1 accepté par le propriétaire)
- **Date** : 2026-11-01
- **Jalons concernés** : version 1.0.1 (tâche L5-01 du plan de correction, défaut protocol-9)
- **Questions liées** : A14 du plan de correction (classement du retrait de `"null"`)
- Amende 05 §9.5 (classe « Correctif »). Amendement : AM-71 (00 §14).

## Contexte

Les schémas d'entrée et de sortie des 13 outils sont produits par schemars. En 1.0.0, ils portaient des mots-clés que JSON Schema 2020-12 ne définit pas ou qui ne disent rien : `format: uint32` (et `uint`, `uint16`, `uint64`) sur les entiers non signés (42 occurrences), `default: null` sur des paramètres optionnels (8), et `"null"` dans le `type` de champs de sortie que le serveur n'émet jamais à `null` (39, tous en `skip_serializing_if`). L'E2E du 2026-11-01 (protocol-9) a relevé ces mots-clés : un client strict peut refuser un `format` inconnu, et `"null"` annonce une valeur jamais servie.

Les retirer change l'empreinte de chaque descripteur concerné. Or `cargo xtask lock-descriptors --compare` classait jusqu'ici tout changement de `format` ou de `default` en rupture, ce qui aurait imposé une version majeure pour un nettoyage sans effet sur un client.

## Décision

1. `crate::schema::published(schema, side)` (`crates/lf-adapter-mcp/src/schema.rs`) retire, en parcourant le schéma selon sa grammaire (`properties`, `$defs`, `items`, `anyOf`, `oneOf`, `allOf`, `prefixItems`…), jamais à l'aveugle : un `format` hors du vocabulaire standard, un `default: null`, et, côté sortie seulement, `"null"` d'un `type` en liste qui garde au moins un autre type. Il ne descend jamais dans `enum`, `const`, `default` non nul ni `examples`, et ne touche pas une propriété *nommée* `format` ou `default`. `minimum: 0` est gardé ; aucun `maximum` n'est ajouté.
2. `cargo xtask lock-descriptors` refuse (erreur) un `format` non standard ou un `default: null` dans un schéma publié.
3. `xtask/src/lock_descriptors/diff.rs` classe en **correctif** (`Class::Fix`) ces trois retraits ; tout autre changement de `format` (dont le retrait de `format: date`) ou de `default` (dont `default: null` vers une valeur) reste une **rupture**.
4. Les listes de formats standard et de mots-clés de sous-schémas existent en trois copies (schema.rs, checks.rs de xtask, test de descripteurs), xtask ne pouvant pas dépendre de `lf-adapter-mcp` (règles d'architecture). Un test (`the_copies_of_the_schema_constants_stay_identical`) les tient identiques.

## Conséquences

- En 1.0.1, `--compare v1.0.0` donne 89 lignes, toutes `[fix]` (42 `format`, 8 `default: null`, 39 `"null"`), aucune `[addition]` ni `[breaking]`. La version corrective suffit.
- `tools/list` passe de 31 853 à 30 634 octets.
- Un client qui épingle l'empreinte d'un descripteur redemande une approbation (05 §9.5) : la rubrique « Tool surface » du CHANGELOG le signale.
- Un futur `format` non standard (nouvelle version de schemars) fait échouer `make lock` au lieu d'être publié.

## Alternatives écartées

| Alternative | Raison |
|---|---|
| Garder les mots-clés | Un client strict peut refuser `format: uint32` ; `"null"` décrit une sortie qui n'existe pas. |
| Les retirer et accepter une rupture (2.0.0) | Une version majeure pour un nettoyage sans effet sur les valeurs servies tromperait les clients. |
| Parcours aveugle de tout l'arbre JSON | Retirerait aussi une donnée nommée `format` ou une valeur d'`enum` ; refusé par la revue du plan. |

## Références et sources

- [05-outils-mcp.md](../plan/05-outils-mcp.md) §2.5 et §9.5 ; [00-decisions.md](../plan/00-decisions.md) §14 (AM-71).
- Code : `crates/lf-adapter-mcp/src/schema.rs`, `xtask/src/lock_descriptors/checks.rs`, `xtask/src/lock_descriptors/diff.rs`, `crates/lf-adapter-mcp/tests/descriptors.rs`.
