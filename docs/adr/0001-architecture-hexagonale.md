# ADR 0001 — Architecture hexagonale en workspace de crates

Statut : accepté (2026-10-15).

## Contexte

La référence Python (`matematicsolutions/fr-eli-mcp`, d3b5b13) mêle dans
`server.py` validation, appels HTTP, normalisation et protocole MCP. L'API de
`rmcp` a changé de version majeure quatre fois en 2026 ; un second adapter
sortant (dumps DILA hors ligne) est prévu en v1.0.

## Décision

Un crate par couche (`docs/plan/00-decisions.md` §2) : `lf-domain` pur,
`lf-application` (ports et cas d'usage), un crate par adapter, `lf-app` comme
seule racine de composition. La règle de dépendance est vérifiée par
`cargo xtask check-arch` en CI.

## Conséquences

- Le protocole MCP reste confiné à `lf-adapter-mcp` : un changement de `rmcp`
  ne touche qu'un crate.
- `lf-domain` se teste sans réseau ni runtime async, ce qui rend tenables les
  portes de couverture et de mutation de l'ADR 0002.
- Plus de manifestes à maintenir qu'un crate unique.
