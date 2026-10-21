# mcp-legifrance

Projet non officiel, sans lien avec la DILA. *Not affiliated with or endorsed by DILA.*

Dépôt : <https://github.com/muchiny/mcp-legifrance>

Serveur MCP en **Rust**, en **architecture hexagonale**, pour interroger le droit français officiel : l'API Légifrance via PISTE, puis les dumps open data de la DILA. Il cible la spécification **MCP 2026-07-28** (« MCP v2 ») et reste compatible avec les clients en 2025-11-25.

> **État : M0 et M1 livrés** (plan 1, [`docs/superpowers/plans/2026-10-15-m0-m1-fondations-domaine.md`](docs/superpowers/plans/2026-10-15-m0-m1-fondations-domaine.md)) : workspace, portes qualité (`make ci`) et crate `lf-domain`. Le reste (M2 à M9) est un plan, qui reprend le serveur Python [`matematicsolutions/fr-eli-mcp`](https://github.com/matematicsolutions/fr-eli-mcp) (commit `d3b5b13`, Apache-2.0). M2 commence par les reliquats de [00 §11.1](docs/plan/00-decisions.md).

## Par où commencer

- **[docs/plan/README.md](docs/plan/README.md)** : point d'entrée du plan (résumé, ordre de lecture selon votre rôle, carte des documents).
- **[docs/plan/00-decisions.md](docs/plan/00-decisions.md)** : décisions normatives (crates, ports, outils, protocole, jalons, questions Q1 à Q17).
- **[docs/plan/07-feuille-de-route.md](docs/plan/07-feuille-de-route.md)** : jalons M0 à M9 (M0 et M1 livrés), tâches à cocher et critères d'acceptation.
- **[docs/adr/](docs/adr/README.md)** : décisions d'architecture (ADR-0001 à ADR-0016).
- **[docs/recherche/](docs/recherche/README.md)** : notes de recherche sourcées qui ont servi à écrire le plan.

## En bref

| | |
|---|---|
| Langage | Rust (édition 2024, MSRV 1.88, toolchain 1.99.0 épinglée en tête de M2), workspace Cargo `crates/lf-*`, binaire `mcp-legifrance` |
| Architecture | `lf-domain` pur → `lf-application` (ports, cas d'usage) → adapters PISTE, cache, audit, MCP, puis DILA |
| Protocole | MCP 2026-07-28 et 2025-11-25 / 2025-06-18 dans le même binaire, SDK `rmcp` 3.5.1 (M4) |
| v0.1.0 | Les 8 outils de la référence (`fr_search`, `fr_get_act`, `fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_get_company_agreement`, `fr_coverage`), sortie compacte et retour progressif pour économiser le contexte |
| Ensuite | v0.2 vérification et résolution de citations ; v0.3 liens, diff de versions, recherche multi-fonds, resources et prompts ; v1.0 index hors ligne des dumps DILA |

## Qualité

Exigence du propriétaire ([00 §10](docs/plan/00-decisions.md), ADR-0002) : 100 % de couverture en lignes, régions et fonctions, 0 mutant survivant, TIMEOUT bloquant. `make ci` est la preuve de complétion ; voir [CONTRIBUTING.md](CONTRIBUTING.md).

## Décisions ouvertes

Les questions ouvertes de [00-decisions.md §13](docs/plan/00-decisions.md) ont chacune une proposition par défaut, à confirmer : compte PISTE, signature des binaires, etc. Q1 (nom `mcp-legifrance`) et Q2 (Apache-2.0, titulaire « Loïc Wernert ») sont décidées le 2026-10-16. Le plan a été rédigé sans appeler l'API PISTE. Ce qui reste à vérifier est inscrit au registre `V-xx` de [09-risques-et-questions.md](docs/plan/09-risques-et-questions.md).

## Données

Le contenu juridique provient de Légifrance / DILA, sous Licence Ouverte 2.0. L'accès à l'API exige un compte et des identifiants PISTE personnels, qui ne doivent jamais être versionnés.
