# Plan de réécriture Rust/hexagonale du serveur MCP Légifrance — index

> Point d'entrée de `docs/plan/`. Ce README résume le plan, indique quoi lire selon son rôle et relie les documents, les données, les ADR et les notes de recherche. Il ne décide rien : [00-decisions.md](00-decisions.md) (révision 2 du 2026-10-15) est normatif, et chaque sujet a un document propriétaire (00 §0). En cas d'écart, ce README a tort.
>
> Rédigé le 2026-10-15. « La référence Python » désigne `matematicsolutions/fr-eli-mcp@d3b5b13` (v0.5.3, Apache-2.0).

## Sommaire

1. [Résumé exécutif](#1-résumé-exécutif)
2. [Ordre de lecture selon le profil](#2-ordre-de-lecture-selon-le-profil)
3. [Carte des documents](#3-carte-des-documents)
4. [Décisions clés](#4-décisions-clés)
5. [Jalons et versions](#5-jalons-et-versions)
6. [Questions ouvertes Q1 à Q15](#6-questions-ouvertes-q1-à-q15)
7. [Conventions du plan](#7-conventions-du-plan)

## 1. Résumé exécutif

**Objectif.** Réécrire en Rust la référence Python, un serveur MCP stdio en lecture seule qui recherche et consulte le droit français officiel (8 fonds de Légifrance, API PISTE) et renvoie le texte verbatim avec une citation vérifiable. La réécriture garde ses 8 outils et ses bonnes idées : lecture seule, contrat de citation, couverture déclarée, audit obligatoire, verrou des descripteurs ([01](01-existant.md) §2). Elle corrige ses 47 défauts `D-01…D-47`, dont l'audit incomplet et le manifeste distant (01 §4), et ajoute des fonctions de vérification juridique. La parité visée est fonctionnelle : chaque divergence est volontaire, nommée et testée ([annexe A](annexes/A-parite.md)).

**Choix structurants.**

- **Rust** : édition 2024, MSRV 1.91, toolchain 1.99.0 épinglée, `unsafe_code = "forbid"`, un seul binaire `mcp-legifrance` (ADR-0001, 00 §2).
- **Hexagonal en workspace** : `lf-domain` (cœur pur, sans I/O ni async), `lf-application` (ports en async natif RPITIT, cas d'usage, façade `LegalService`, décorateur `Audited`, échéance `Deadline`), adapters `lf-adapter-piste`, `lf-adapter-cache`, `lf-adapter-audit`, `lf-adapter-mcp`, puis `lf-adapter-dila` (M9), composition root `lf-app`. Les règles de dépendance sont vérifiées en CI par `cargo xtask check-deps` (ADR-0002, ADR-0012, [02](02-architecture.md)).
- **MCP 2026-07-28 (« MCP v2 ») en dual-era** avec `rmcp`/`rmcp-macros` `=3.5.1` : ère moderne (`server/discover`, `_meta` par requête) et ère legacy (`initialize` en 2025-11-25 et 2025-06-18) dans le même binaire, démarrage sans réseau, filtre `PreLifecycleFilter` contre les arrêts intempestifs de rmcp (ADR-0003, [annexe D](annexes/D-mcp-2026-07-28.md)).
- **PISTE, puis DILA hors ligne, derrière le même port** `LegalCorpus` : l'API Légifrance via PISTE (OAuth2, débit, réessais, délai global) est la seule implémentation du port jusqu'à M9 ; les dumps DILA indexés par tantivy arrivent alors, avec une sélection à l'exécution par `AnyCorpus` (ADR-0004, [04](04-adapter-piste.md), [annexe C](annexes/C-api-legifrance.md)).
- **Économie de contexte** : le `structuredContent` compact **est** la réponse (champs vides omis, aucun doublon). Les textes longs sont rendus progressivement : résumé par défaut, fenêtre `offset`/`max_chars` (4 000 par défaut), passages BM25 sur `focus`, pagination. Plafond dur de 45 000 caractères par réponse, instructions ≤ 3 500 caractères. Sur la décision CONSTIT des fixtures, la sortie passe de 15 140 à 1 147 caractères (ADR-0006, [05](05-outils-mcp.md) §0, §6).
- **Nouvelles fonctions** : vérification des citations d'un texte libre (`fr_verify_citations`) et résolution d'une citation en identifiants (`fr_resolve`) en v0.2 ; liens d'un article ou d'une décision (`fr_get_related`), diff entre versions d'un article (`fr_diff_article`) et recherche multi-fonds (`fr_search_all`) en v0.3 (05 §5.9 à §5.13, [annexe B](annexes/B-citations.md)).
- **Confiance** : aucune configuration distante ni télémétrie (ADR-0005) ; audit JSONL de chaque appel, strict par défaut (ADR-0008) ; erreurs métier in-band `[code] message` (ADR-0010) ; identifiants, ECLI et ELI jamais fabriqués (ADR-0009).

**Ce que livre la v0.1.0** (jalons M0 à M5) :

- les 8 outils de la référence sous leurs noms : `fr_search`, `fr_get_act`, `fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_get_company_agreement`, `fr_coverage` ; contrat de citation v2 (typographie française corrigée, `source_url` stable) et couverture calculée (lacunes `FR-001…FR-009`, 05 §5.8) ;
- le serveur stdio dual-era, des schémas d'entrée et de sortie verrouillés (`tests/tools.lock.json`), le retour progressif et le plafond dur ;
- l'adapter PISTE résilient (sandbox par défaut, production par `LF_PISTE_ENV`), le cache mémoire, l'audit strict, la configuration par variables `LF_*` (lecture des `FR_ELI_*` avec avertissement de dépréciation) ;
- la distribution : binaires cargo-dist pour 7 cibles, bundle MCPB, image OCI, registre MCP (`io.github.muchiny/mcp-legifrance`, Q1), `cargo install --git` ; signature selon Q5 ([08](08-distribution.md)).

**État du plan.** Le plan a été rédigé et relu le 2026-10-15 (révision 2 de 00, amendements `AM-01…AM-43`). Aucune ligne de code n'existe : les extraits Rust ne sont pas compilés (« à valider en M0 »). Le conteneur de rédaction n'avait accès ni aux archives des crates (`static.crates.io`) ni à la toolchain 1.99.0, ni à PISTE, ni à legifrance.gouv.fr, ni aux dumps DILA ([09](09-risques-et-questions.md) §0.3). Les faits sur l'API viennent des 16 fixtures de la référence, du Swagger 2.4.2 et de clients tiers ; ceux sur rmcp, de la lecture de son code au tag `rmcp-v3.5.1`. Tout ce qui n'a pas pu être vérifié est marqué **NON VÉRIFIÉ** et inscrit au registre `V-01…V-215` de 09. Aucune vérification n'est levée et neuf sont partielles (V-31 à V-34, V-39, V-40, V-169 à V-171 ; 09 §2.20). Vingt-cinq bloquent un jalon d'ici v0.1.0 ([09 §2.18](09-risques-et-questions.md#218-synthèse-par-jalon)). Les plus structurantes :

- compilation du squelette et TLS en compilation croisée (V-01, V-02) ;
- arguments `JsonObject` et filtre pré-cycle de rmcp (V-28, V-29) ;
- comportement « introuvable » et sémantique de `singleDate` chez PISTE (V-66, V-68, V-76) ;
- taille de `lawDecree` sur un code (V-96) ;
- URL Légifrance (V-139, V-140, V-142 à V-144).

Deux prérequis conditionnent la plupart des levées : un réseau de construction (Q6, avant M0) et un compte PISTE sandbox (Q3, avant M2). Quatre risques ont un score ≥ 9 : R-13 (accès réseau, 12), R-06 (« introuvable »), R-08 (signature) et R-16 (dumps DILA) (09 §1.1).

## 2. Ordre de lecture selon le profil

Tous les profils commencent par ce README, puis par [00-decisions.md](00-decisions.md) (250 lignes).

**Décideur** (propriétaire du dépôt : produit, juridique, comptes, budget)

1. Ce README, §1 et §5 ; [00](00-decisions.md#11-jalons-identiques-partout) §11 (jalons) et [§13](00-decisions.md#13-questions-ouvertes-q1q15) (questions).
2. [09 §3](09-risques-et-questions.md#3-questions-ouvertes-q1-à-q15-et-arbitrages-secondaires) : contexte, options, décideur et échéance de chaque question ; puis 09 §1.1 (risques).
3. [01](01-existant.md) §0 et §5 : ce qu'on garde, ce qu'on corrige, ce qui change pour un client.
4. [08](08-distribution.md) §0 et §9 : licence, Licence Ouverte, CGU PISTE, marque « Légifrance ».
5. [Index des ADR](../adr/README.md) §5 : statuts et conditions d'acceptation.

**Développeur qui démarre M0**

1. [00](00-decisions.md) en entier.
2. [02](02-architecture.md) §1 à §4 (hexagone, arborescence, socle du workspace, `check-deps`), puis §5 (ports).
3. [06](06-qualite.md) §1.4, §7 à §9 et §14 : fichiers de test, CI, lints, cargo-deny, critères de M0.
4. Les spikes : [05](05-outils-mcp.md) §3.2 et §3.3 (squelette rmcp, `JsonObject`, V-28), [annexe D](annexes/D-mcp-2026-07-28.md) §4.3 (`PreLifecycleFilter`, V-29), [08](08-distribution.md) §2 (cargo-dist, TLS aws-lc ou ring, V-02).
5. [09](09-risques-et-questions.md) §0.3 (état du réseau), §2.3 et §2.4 (V-01 à V-45), §5 (critères de M0).
6. [07](07-feuille-de-route.md) : tâches `T-M0-yy` ; ADR-0001, ADR-0002, ADR-0003 et ADR-0012.

**Contributeur aux outils MCP**

1. [00](00-decisions.md) §5 à §7 : erreurs, protocole, contrat des outils.
2. [05](05-outils-mcp.md) en entier : principes (§1), protocole (§2), rmcp (§3), catalogue (§5), rendu et budgets (§6), erreurs (§7), verrou des descripteurs et SemVer de la surface (§9).
3. [Annexe D](annexes/D-mcp-2026-07-28.md) §3, §4 et §8 : cycle sans session, routage dual-era, exigences `C-xx`.
4. [02](02-architecture.md) §6 et §12.1 (cas d'usage, ajouter un outil) ; [06](06-qualite.md) §2.6 et §6 (tests de l'adapter, conformité).
5. [Annexe A](annexes/A-parite.md) §4 et §6 (divergences visibles, migration) ; ADR-0006, ADR-0010 et ADR-0014.

**Contributeur aux citations** (M6)

1. [00](00-decisions.md) §4 (contrat de citation, parseur) et §7 (`fr_verify_citations`, `fr_resolve`).
2. [Annexe B](annexes/B-citations.md) en entier : formes observées (§2), normalisation à offsets (§3), grammaire (§4), dictionnaire des codes (§5), types (§6), résolution (§7), corpus (§8), critères (§9).
3. [03](03-domaine.md) §5 et §8 ; [04](04-adapter-piste.md) §6.9 et §7.2 (recherches exactes, endpoints M6) ; [05](05-outils-mcp.md) §5.9 à §5.11.
4. Les données : corpus positif et négatif, [table des codes](annexes/donnees/codes-table.md) (§3.3 ci-dessous).
5. [09](09-risques-et-questions.md) §2.9 (V-118 à V-138) et §2.11 (V-146 à V-163), R-19 ; ADR-0009 ; [recherche 07](../recherche/07-citations.md).

**Mainteneur release**

1. [08](08-distribution.md) en entier, surtout §7 (processus de release, `release-gates`), §6 (registre MCP, immuabilité), §3 (signature) et §10 (critères par version).
2. [05](05-outils-mcp.md) §9 : verrou des descripteurs, `--compare`, politique SemVer de la surface.
3. [06](06-qualite.md) §5 (tests live, sondes manuelles de production, registre des exécutions) et §13.3 (CHANGELOG).
4. [09 §2.18](09-risques-et-questions.md#218-synthèse-par-jalon) (ligne « Recette v0.1.0 et distribution ») et §2.13 ; [annexe D](annexes/D-mcp-2026-07-28.md) §10.3 (test manuel par client) ; [annexe A](annexes/A-parite.md) §6 (note de migration) ; ADR-0013.

## 3. Carte des documents

### 3.1 Plan

| Document | Contenu | Quand le lire |
|---|---|---|
| [00-decisions.md](00-decisions.md) | **Normatif** : noms, variables `LF_*`, crates, ports, domaine, erreurs, protocole, contrat des 13 outils, adapter PISTE, cache, qualité, jalons, distribution, Q1 à Q15, journal `AM-01…AM-43` | avant tout ; à chaque doute |
| [01-existant.md](01-existant.md) | la référence Python, ce qu'on garde, les quatre constats, les 47 défauts et leur traitement, ce qui change pour un client, incertitudes sur l'API | pour comprendre pourquoi on réécrit |
| [02-architecture.md](02-architecture.md) | hexagone, arborescence, fiches par crate, règles de dépendance, ports, cas d'usage, décorateurs, composition root, erreurs en couches, concurrence, observabilité, extensibilité | M0, M3 ; avant toute nouvelle dépendance |
| [03-domaine.md](03-domaine.md) | `lf-domain` : identifiants, fonds, entités, contrat de citation v2, dates, texte (HTML, fenêtre, BM25, résumé), architecture du parseur | M1, M6 |
| [04-adapter-piste.md](04-adapter-piste.md) | `lf-adapter-piste` : environnements, OAuth, HTTP, résilience, `/search`, endpoints par jalon, DTO, mapping, mojibake, « introuvable », fixtures et tests | M2, puis M6 et M7 |
| [05-outils-mcp.md](05-outils-mcp.md) | `lf-adapter-mcp` : protocole, mise en œuvre rmcp, instructions, schémas des 13 outils, rendu et budgets, erreurs, resources et prompts, verrou des descripteurs | M4 à M7 |
| [06-qualite.md](06-qualite.md) | stratégie de test, catalogue de non-régression, fixtures, tests live, conformité MCP, CI, lints, cargo-deny, sécurité, spécification de l'audit, budgets, processus (revue, ADR, CHANGELOG, « terminé ») | dès M0 ; à chaque PR |
| [07-feuille-de-route.md](07-feuille-de-route.md) | découpage de chaque jalon en tâches `T-Mx-yy` | pour planifier ou prendre une tâche |
| [08-distribution.md](08-distribution.md) | cargo-dist, signature, MCPB, OCI, registre MCP, processus de release, configuration des clients, licence et conformité | M0 (spike TLS), puis avant chaque release |
| [09-risques-et-questions.md](09-risques-et-questions.md) | registres des risques `R-xx` et des vérifications `V-xx` (synthèse par jalon, index des étiquettes, journal des levées), questions Q1 à Q15, arbitrages secondaires, hypothèses, critères par jalon | à l'ouverture et à la clôture de chaque jalon |

### 3.2 Annexes

| Document | Contenu | Quand le lire |
|---|---|---|
| [A-parite.md](annexes/A-parite.md) | parité fonctionnelle : check-list `P-001…P-060`, scénarios d'erreur S1 à S17, écarts `E-01…E-36`, divergences volontaires, données golden, guide de migration | M1, M2, M4 (tests de parité) ; migration depuis la référence |
| [B-citations.md](annexes/B-citations.md) | parseur de citations (normalisation à offsets, grammaire EBNF par famille, dictionnaire des codes), résolution et vérification, corpus de tests, critères | M6 ; M7 pour `fr_get_related` |
| [C-api-legifrance.md](annexes/C-api-legifrance.md) | API Légifrance via PISTE : accès, quotas, CGU, 68 opérations, `/search`, formes JSON, DTO ; Judilibre ; dumps DILA | M2, M6, M7, M9 |
| [D-mcp-2026-07-28.md](annexes/D-mcp-2026-07-28.md) | MCP 2026-07-28 : changements normatifs, cycle sans session, dual-era sur stdio avec rmcp 3.5.1, transports, autorisation, check-list `C-xx`, bugs de rmcp, support des clients | M0 (spike), M4, M7, M8 ; à chaque montée de rmcp |

### 3.3 Données (`annexes/donnees/`)

| Fichier | Contenu | Quand le lire |
|---|---|---|
| [parite-golden.json](annexes/donnees/parite-golden.json) | golden de parité, figé : sortie des normaliseurs (`normalizer`, 17 entrées) et des outils (`tool_output`, 16) de la référence sur ses 16 fixtures | tests de parité (M2, M5) ; annexe A §5 |
| [citations-corpus-positifs.jsonl](annexes/donnees/citations-corpus-positifs.jsonl), [citations-corpus-negatifs.jsonl](annexes/donnees/citations-corpus-negatifs.jsonl) | corpus du parseur : 170 cas positifs (`P001…P200`) et 42 négatifs, ambigus ou suspects (`N01…N42`), avec leur provenance | M6 ; annexe B §8 |
| [codes-table.md](annexes/donnees/codes-table.md) | 77 codes : titre officiel, `LEGITEXT` candidat, abréviations, sigles ambigus ; la table vivante vient de `/list/code`, jamais du code source | M6 ; annexe B §5 |
| [scripts/](annexes/donnees/scripts/) | `golden.py` (génère le golden), `check_golden.py` (y rejoue les assertions de la référence), `dto_repair_check.py` (contrôle la réparation du mojibake) ; Python 3.13, `python3 -I`, empreintes en 00 §10 | pour régénérer ou contester une donnée, jamais à partir d'une copie locale |

### 3.4 ADR et notes de recherche

| Document | Contenu | Quand le lire |
|---|---|---|
| [docs/adr/](../adr/README.md) | 14 ADR (`ADR-0001…ADR-0014`) : pourquoi chaque décision, options écartées, tests qui la gardent ; index, cycle de vie, modèle | avant de modifier une décision : tout changement de 00 exige un ADR (06 §13.2) |
| [docs/recherche/](../recherche/README.md) | 9 rapports bruts du 2026-10-15 (parité, défauts, DTO, rmcp, PISTE et DILA, spécification MCP, citations, MCP 2026-07-28, rmcp en 2026), avec preuves `chemin:ligne` et URL | pour remonter à la source d'un fait ou lever un `V-xx` |

## 4. Décisions clés

| Décision | ADR | Document de référence |
|---|---|---|
| Réécriture en Rust : édition 2024, MSRV 1.91, `unsafe` interdit, un seul binaire `mcp-legifrance` | [ADR-0001](../adr/0001-reecriture-en-rust.md) | 00 §1-§2, [01](01-existant.md) |
| Hexagone en workspace multi-crates, dépendances contrôlées par `cargo xtask check-deps` | [ADR-0002](../adr/0002-hexagonal-workspace.md) | [02](02-architecture.md) §1 à §4 |
| MCP 2026-07-28 dual-era (legacy 2025-11-25 et 2025-06-18) avec `rmcp =3.5.1`, montées manuelles | [ADR-0003](../adr/0003-mcp-2026-07-28-dual-era-rmcp.md) | [annexe D](annexes/D-mcp-2026-07-28.md), 05 §2-§3 |
| PISTE d'abord, DILA hors ligne en M9, derrière le même port `LegalCorpus` | [ADR-0004](../adr/0004-piste-puis-dila.md) | [04](04-adapter-piste.md), [annexe C](annexes/C-api-legifrance.md) |
| Aucune configuration distante ni télémétrie ; variables d'environnement seulement | [ADR-0005](../adr/0005-pas-de-configuration-distante.md) | 02 §8.2, 08 §1.4 |
| `structuredContent` compact = la réponse ; résumé par défaut, fenêtre, `focus`, plafond dur de 45 000 caractères | [ADR-0006](../adr/0006-sortie-compacte-retour-progressif.md) | 05 §1, §6 |
| Cache mémoire au niveau du port (`moka`, TTL par opération, coalescence) ; cache disque réévalué en M9 | [ADR-0007](../adr/0007-cache-memoire-port.md) | 02 §7.2 |
| Audit de chaque exécution par `Audited`, JSONL, `strict` par défaut (Q9) ; arguments décodés depuis `JsonObject` et audités | [ADR-0008](../adr/0008-audit-decorateur-strict.md) | 02 §7.1, 06 §11 |
| Contrat de citation v2 : `citation` en français correct, `source_url` stable, ECLI et ELI jamais fabriqués | [ADR-0009](../adr/0009-contrat-de-citation-v2.md) | 03 §5 |
| Erreurs métier in-band `[code] message actionnable` (6 codes) ; erreurs de protocole en JSON-RPC | [ADR-0010](../adr/0010-erreurs-in-band.md) | 02 §9, 05 §7 |
| Dates `chrono::NaiveDate`, « aujourd'hui » à Paris, sentinelles ≥ 2222-01-01 omises, `singleDate` à midi Europe/Paris | [ADR-0011](../adr/0011-dates-chrono-paris.md) | 03 §6, 04 §6.3 |
| Ports en async natif (RPITIT), dispatch statique, `AnyCorpus` en M9 | [ADR-0012](../adr/0012-ports-async-rpitit.md) | 02 §5, §10 |
| Distribution : cargo-dist, MCPB, OCI, registre MCP, signature (**proposé** : Q1, Q5, Q13, spike TLS) | [ADR-0013](../adr/0013-distribution.md) | [08](08-distribution.md) |
| Recherche : `match = any` par défaut jusqu'à V-79, `typePagination` interne (**proposé** : Q10, mesures de M5) | [ADR-0014](../adr/0014-recherche-mode-pagination.md) | 05 §5.1, 04 §6 |

## 5. Jalons et versions

Jalons de 00 §11, identiques dans tout le plan ; découpage en tâches dans [07-feuille-de-route.md](07-feuille-de-route.md). Les vérifications bloquantes viennent de [09 §2.18](09-risques-et-questions.md#218-synthèse-par-jalon). Critères de clôture : [09 §5](09-risques-et-questions.md#5-critères-dacceptation-de-ce-registre-par-jalon) et la section « critères » de chaque document (03 §11, 04 §15, 05 §11, 06 §14, 08 §10, annexe B §9, annexe D §8.4).

| Jalon | Contenu | Version | Questions à trancher | Vérifications bloquantes |
|---|---|---|---|---|
| M0 Fondations | workspace, toolchain, CI, cargo-deny, xtask, ADR, `LICENSE`/`NOTICE`, `CHANGELOG.md`, verrou de `Cargo.lock`, spikes (rmcp, `task_local!`, TLS) | — | Q1, Q2, Q6 | V-01, V-02, V-25, V-28, V-29 |
| M1 Domaine | IDs, fonds, entités, contrat de citation v2, URL, dates, texte | — | Q12 | — |
| M2 Adapter PISTE | OAuth, client, réessais, débit, délai, DTO, mapping, mojibake, wiremock, smoke tests live | — | Q3 (sandbox) | V-66, V-68, V-76, V-96 |
| M3 Application | ports, cas d'usage, `LegalService`, `Audited`, `Deadline`, audit, cache, horloge | — | Q9 | — |
| M4 Adapter MCP | rmcp dual-era, 8 outils, instructions, `server/discover`, `subscriptions/listen`, verrou des descripteurs, binaire | — | Q7, Q8 (listes), Q10, Q14 | V-30, V-35, V-38 |
| M5 Économie de contexte | `detail`, fenêtre, `focus`, pagination, plafond dur, budgets mesurés | **v0.1.0** (M0→M5) | Q3 (production), Q5, Q11, Q13, Q15 | V-75, V-82 ; recette : V-46, V-59, V-103, V-139, V-140, V-142, V-143, V-144, V-173 (si certificat), V-197, V-204 |
| M6 Citations | parseur et corpus, `fr_verify_citations`, `fr_resolve`, `list_codes()`, ECLI, ELI cités | **v0.2.0** | Q12 (réexamen des hits CONSTIT et CNIL) | V-119, V-120, V-122, V-163 |
| M7 Navigation | `fr_get_related`, `fr_diff_article`, `fr_search_all`, resources, completions, prompts ; surface JORF | **v0.3.0** | Q8 (resources) | V-135 |
| M8 HTTP (optionnel) | Streamable HTTP local, puis distant avec resource server OAuth | v0.4.0 | Q4 | V-42 ; V-62, V-175, V-176 pour un profil partagé |
| M9 Hors ligne | `lf-adapter-dila` (dumps, incréments, index tantivy), sélection d'adapter | **v1.0.0** | Q6 (DILA), Q13 (réexamen) | V-180, V-185, V-186, V-187 |
| Transverse Distribution | cargo-dist, signature, MCPB, OCI, registre MCP, `cargo install --git` | dès v0.1.0 | Q1, Q5, Q13 | dans la recette de v0.1.0 |

## 6. Questions ouvertes Q1 à Q15

Chaque question porte une proposition **adoptée par défaut**, appliquée dans tout le plan jusqu'à la réponse de l'utilisateur, qui la confirme ou la remplace (00 §13). Contexte, options, décideur et réponses datées : [09 §3](09-risques-et-questions.md#3-questions-ouvertes-q1-à-q15-et-arbitrages-secondaires). Au 2026-10-15, aucune réponse n'est consignée.

- **Q1 — Nom** : `mcp-legifrance` partout ; dépôt renommé avant le premier commit de M0 ; paquet `mcp-legifrance` dans `crates/lf-app`. Avant M0 et avant toute publication immuable.
- **Q2 — Licence** : Apache-2.0 + `NOTICE`. Au premier commit de M0.
- **Q3 — Compte PISTE** : compte du propriétaire ; sandbox par défaut ; en production, sondes manuelles en lecture seule, jamais en CI. Sandbox avant M2, production avant v0.1.0.
- **Q4 — HTTP partagé et CGU** : profil local seulement tant que les CGU ne sont pas clarifiées. Avant M8.
- **Q5 — Signature et MCPB** : Azure Artifact Signing ou SSL.com eSigner ; `codesign` + `notarytool` ; sinon binaires non signés et replis OCI/WSL ; bundle MCPB unique. Avant v0.1.0.
- **Q6 — Réseau et toolchain** : ouvrir `static.crates.io` et `static.rust-lang.org`, puis `echanges.dila.gouv.fr` ; `RUSTUP_TOOLCHAIN=1.97.0` en local. Avant M0, puis avant M9.
- **Q7 — Versions legacy** : 2025-11-25 et 2025-06-18 ; 2025-03-26 exclue. Avant M4.
- **Q8 — `ttlMs` et `cacheScope`** : 3 600 000 ms, `public`, pairs modernes seulement. M4 (listes), M7 (resources).
- **Q9 — Audit** : `LF_AUDIT_MODE=strict`, `best-effort` sur option. Avant M3.
- **Q10 — Mode de recherche** : `any` jusqu'à V-79, puis `all` si V-79 le confirme. Avant le verrou des descripteurs de M4.
- **Q11 — `openWorldHint`** : `true` jusqu'à V-53. Avant v0.1.0.
- **Q12 — Formes de citation** : titre sans appel de note, KALI sans IDCC, hits CONSTIT et CNIL = titre officiel, Conseil constitutionnel sans l'objet. En M1, avant les golden.
- **Q13 — Canal Cargo** : `cargo install --git … --tag vX.Y.Z --locked`, sans crates.io en v0.x. Avant v0.1.0 ; réexamen à v1.0.0.
- **Q14 — Langues** : `title` et descriptions des outils en anglais, prompts en français. Avant le verrou des descripteurs de M4.
- **Q15 — Attribution** : « Source : Légifrance / DILA — Licence Ouverte 2.0 » et date de consultation dans `fr_coverage` et les instructions. Avant v0.1.0.

## 7. Conventions du plan

- **Langue** : français ; identifiants Rust, crates, champs JSON et noms d'outils tels quels (00 §0).
- **Document propriétaire** : un seul document fait foi par sujet, les autres y renvoient sans recopier ; 00 reste au-dessus de tous (00 §0).
- **Code** : tout extrait non compilé est marqué « non compilé, à valider en M0 ».
- **Questions** : un choix pris sur une question ouverte est écrit « adopté par défaut — à confirmer Qn ».

| Identifiants | Objet | Défini dans | Nouveaux identifiants |
|---|---|---|---|
| `D-01…D-47` | défauts de la référence Python | [recherche 02](../recherche/02-defauts.md) ; traitement : 01 §4 ; tests : 06 §3.1 | liste close |
| `P-001…P-060` | points de parité (36, numérotation à trous) | [recherche 01](../recherche/01-parite.md) §15 ; décisions : annexe A §2 | liste close |
| `E-01…E-36` | écarts de la référence | recherche 01 §14 ; décisions : annexe A §3 | liste close |
| `T-Mx-yy` | tâche `yy` du jalon `Mx` | [07](07-feuille-de-route.md) | 07 |
| `R-01…R-30` | risques résiduels (probabilité × impact) | 09 §1 | à la suite du dernier, jamais réutilisés ; plage de 00 §0 amendée dans la même PR |
| `V-01…V-215` | vérifications : tout ce qui est NON VÉRIFIÉ | 09 §2 | à la suite du dernier, jamais réutilisés ; plage de 00 §0 amendée dans la même PR |
| `Q1…Q15` | questions ouvertes de l'utilisateur | 00 §13 ; réponses : 09 §3 | numérotation définitive |
| `AM-01…AM-43` | amendements de la revue, intégrés à la révision 2 | 00 §14 | journal de 00 |
| `ADR-0001…ADR-0014` | décisions d'architecture | [docs/adr/](../adr/README.md) | numérotation continue après le dernier ADR, numéro jamais réutilisé |
| `C-01…C-34`, `C-40…C-60` | exigences de conformité MCP (stdio, HTTP) | annexe D §8 ; tests : 06 §6 | annexe D |

Autres identifiants locaux : `S1…S17` (scénarios d'erreur, annexe A §2.2), `FR-001…FR-009` (lacunes de `fr_coverage`, 05 §5.8), `MEN-01…MEN-23` (modèle de menaces, 06 §10.2), `P001…P200` et `N01…N42` (cas du corpus de citations, annexe B §8).

**Marquage des points non vérifiés.**

- Un fait non vérifié est écrit **NON VÉRIFIÉ**, suivi d'une étiquette `[v:…]` (par exemple `[v:quota-piste-prod]`). L'index de [09 §2.19](09-risques-et-questions.md#219-index-des-étiquettes) rattache les 225 étiquettes du plan, des ADR et du registre aux 215 vérifications, alias compris ; 09 cite directement les `V-xx`.
- Les listes « points NON VÉRIFIÉS » de chaque document sont des index locaux. En cas d'écart (jalon, caractère bloquant, formulation), le registre de 09 fait foi.
- Statuts d'une vérification : `ouvert`, `levé`, `partiel`, `contourné`, `caduc`. Une levée produit une ligne du [journal des levées](09-risques-et-questions.md#220-journal-des-levées) avec sa preuve, et le marqueur est retiré des documents concernés.
- Une nouvelle incertitude reçoit l'identifiant suivant. Une étiquette qui recouvre une vérification existante en devient un alias (09 §0.1).
