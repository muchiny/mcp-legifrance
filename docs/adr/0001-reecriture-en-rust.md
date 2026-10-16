# ADR-0001 — Réécriture en Rust

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : tous (M0 → M9)
- **Questions liées** : Q1 (nom du produit, du dépôt et du paquet), Q2 (licence), Q6 (réseau du conteneur de développement et toolchain) ; propositions adoptées par défaut, à confirmer : Q1 avant le premier commit de M0, Q2 au premier commit, Q6 avant M0. Sans réponse à l'ouverture de M0, M0 démarre sous le nom actuel du dépôt avec ces propositions ; le renommage éventuel suit, redirection GitHub vérifiée (00 §13)
- Révisé le 2026-10-15 (revue) : paquet Cargo du binaire `mcp-legifrance` (AM-32) ; toolchain locale et réseau du conteneur (Q6, AM-35) ; règle d'âge minimal des dépendances (AM-26) ; décompte des versions incompatibles de rmcp (AM-42) ; contrôle final du 2026-10-16 : versions de référence, exigences au niveau mineur et verrou initial pas avant le 2026-10-19 (AM-26) ; démarrage de M0 sans réponse aux questions (00 §13).

## Contexte

- **Demande.** L'utilisateur demande de réécrire le serveur MCP Python « fr-eli-mcp » en Rust, en architecture hexagonale et selon les bonnes pratiques ([00-decisions.md](../plan/00-decisions.md), titre et §0). La référence est `matematicsolutions/fr-eli-mcp`, commit `d3b5b13` (2026-09-24), v0.5.3, Apache-2.0 (00 §0). Elle comporte 8 outils, FastMCP 3.x, httpx, pydantic et diskcache (recherche 01 §1).
- **Défauts de la référence.** L'audit critique en relève 47 : 2 bloquants (D-02 audit incomplet, D-04 manifeste distant), 27 majeurs et 18 mineurs (recherche 02 §1). Ces défauts sont **structurels** : ils viennent de la forme du code, pas de bugs isolés.
  - D-07 : le code d'audit, de chronométrage et de `try/except` est recopié dans chaque outil (7 `try/except`, 15 appels `audit.log`). C'est la cause racine de D-02 (audit absent sur `invalid_arg`) et de D-03 (`duration_ms = 0`).
  - D-06 : client HTTP et cache disque recréés à chaque appel d'outil ; D-38 : E/S synchrones dans la boucle asyncio.
  - D-08 et E-01 : les erreurs sont des chaînes que FastMCP enveloppe dans « Error calling tool '<nom>': … », alors que les instructions promettent un préfixe `[code]` (recherche 01 §2.4).
  - D-24 : typage pydantic rigide hors `try` ; D-37 : dépendances non verrouillées, d'où un verrou de descripteurs instable ; D-36 : CI sur Python 3.12 seulement, aucun test du client ni de l'audit.
- **Distribution.** Smart App Control (Windows 11) bloque les lanceurs non signés `fr-eli-mcp.exe`, `uvx.exe` et `pip.exe`. La référence doit faire passer ses utilisateurs par `python -m` (référence `__main__.py:1-9`, `README.md:22-42`).
- **SDK.** `rmcp` 3.5.1 (2026-10-05) implémente la révision MCP 2026-07-28 tout en servant les clients 2025-11-25 dans le même binaire. Le SDK Rust est Tier 1 depuis le 2026-08-21 (recherche 09 §0, §1).

## Décision

1. Le serveur est **réécrit entièrement en Rust** :
   - produit et binaire `mcp-legifrance`, crates `lf-*` ; le paquet Cargo du binaire s'appelle aussi `mcp-legifrance` (répertoire `crates/lf-app`), seule exception au préfixe (00 §1, §2 ; Q1) ;
   - édition 2024, MSRV 1.91, toolchain épinglée sur 1.99.0 dans `rust-toolchain.toml` ; tant que le conteneur de développement ne peut pas l'installer, `RUSTUP_TOOLCHAIN=1.97.0` en local, la CI gardant l'épinglage (00 §2, Q6) ;
   - les versions de crates citées par le plan sont des versions de référence, pas des planchers : `[workspace.dependencies]` écrit l'exigence au niveau mineur (par exemple `tokio = "1.53"`), sauf `rmcp` et `rmcp-macros`, épinglés `"=3.5.1"` ; toute version figée dans `Cargo.lock` est publiée depuis au moins 14 jours au verrouillage (pubtime de l'index sparse ; sauf correctif de sécurité justifié dans la PR) ; le verrou initial (T-M0-05) retient la plus haute version éligible de chaque crate et n'a pas lieu avant le 2026-10-19, date d'éligibilité de rmcp 3.5.1 (00 §2).
2. **Parité fonctionnelle en v0.1.0** (M0 à M5) : 8 outils de mêmes noms, paramètres et sens, définis par [annexes/A-parite.md](../plan/annexes/A-parite.md). Les divergences volontaires sont listées par clé dans l'annexe A §4 et justifiées par les ADR 0005 à 0011.
3. **Aucune ligne Python n'est portée telle quelle.**
   - Sont réutilisés, avec attribution dans `NOTICE` (00 §12) : les fixtures de la référence, ses assertions de test (`ported_*`) et les sorties golden ([parite-golden.json](../plan/annexes/donnees/parite-golden.json)).
   - Le golden est produit par [golden.py](../plan/annexes/donnees/scripts/golden.py), réimplémentation indépendante des normaliseurs de la référence qui n'importe pas son code, et contrôlé par `check_golden.py` (même répertoire ; Python 3.13, `python3 -I` ; empreintes SHA-256 des versions utilisées : 00 §10). Toute régénération passe par ces scripts versionnés.
   - Aucune ligne n'est reprise des projets AGPL (juridix, mcp-legifrance de Ktulu-Analog) : on n'en garde que les idées.
4. L'architecture suit [ADR-0002](0002-hexagonal-workspace.md), le protocole [ADR-0003](0003-mcp-2026-07-28-dual-era-rmcp.md) et la distribution [ADR-0013](0013-distribution.md).

## Conséquences

### Positives

- **Le typage élimine des classes entières de défauts** :
  - newtypes d'identifiants à `FromStr` strict (D-31, E-16) ;
  - `enum ErrorCode` fermé : le test Python `test_tool_error_rejects_unknown_code` n'a plus d'objet (recherche 01 §11.3) ;
  - `DecisionId` dont le préfixe fixe la juridiction (D-28).
- Un seul `reqwest::Client` partagé et aucune E/S bloquante dans tokio, qui corrigent D-06, D-38 et D-05.
- Un binaire natif unique, que l'on peut signer (Q5) : Smart App Control n'est plus contourné par un interpréteur signé.
- `Cargo.lock` versionné : le verrou des descripteurs devient reproductible (D-37).

### Négatives

- **Coût** : réécriture complète, avec un plan de dix jalons. Les compétences Rust et async sont requises pour contribuer.
- **Écosystème MCP Rust instable** : l'API de rmcp a connu au moins quatre versions incompatibles en 2026 (0.17, 1.0, 2.0, 3.0), et `LATEST` a changé dans une mineure, la 3.5.0 (00 §6, recherche 09 §2). Le risque est contenu par le confinement de rmcp dans un crate (ADR-0002, ADR-0003).
- **Rien n'a été compilé pendant la recherche**, car `static.crates.io` et `docs.rs` étaient bloqués (recherche README). Tous les extraits sont donc **NON VÉRIFIÉ** [v:squelette-compile-m0] ; ce point est levé par les spikes de M0. Le conteneur de développement doit pouvoir joindre `static.crates.io` et `static.rust-lang.org` dès M0 (Q6, proposition adoptée par défaut ; les lints propres à 1.98 et 1.99 ne sont vus qu'en CI d'ici là).
- Le paquet PyPI disparaît. L'utilisateur de la référence doit migrer : variables `LF_*`, chemin d'audit, champs de sortie. La procédure est décrite dans l'annexe A §6.

### À surveiller

- La date de sortie de Rust 1.99.0 (2026-10-01 d'après la cadence de six semaines), dont dépendent l'épinglage et la règle d'âge minimal des dépendances : **NON VÉRIFIÉ** [v:date-rust-1-99] (V-16).
- Q1 se tranche avant toute publication immuable (registre MCP, MCPB, crates.io, GHCR), au vu de l'homonymie avec `Ktulu-Analog/mcp-legifrance` (AGPL) et du statut de la marque « Légifrance » : **NON VÉRIFIÉ** [v:marque-legifrance] (V-215).
- La parité est mesurée, pas supposée : on suit `tool_output_matches_golden` et les divergences de l'annexe A §4.

### Tests qui la gardent

- Golden de parité (annexe A §5), régénéré et contrôlé par les scripts versionnés de `docs/plan/annexes/donnees/scripts/` (00 §10).
- Tests portés `ported_test_parse.rs` et `live_reference_scenario_1..9` (P-060).
- Catalogue de non-régression D-01…D-47 (06 §3.1).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Corriger la référence Python (fork) | Défauts répartis dans tout le code (D-02, D-06, D-07, D-38). FastMCP impose son enveloppe d'erreur (E-01). Le blocage par Smart App Control des lanceurs Python resterait (02 §13). |
| TypeScript (SDK Tier 1) | Contraire à la demande. Exige un environnement Node.js chez l'utilisateur, sans binaire natif unique. |
| Go (SDK Tier 1 à la GA, recherche 09 §1) | Contraire à la demande. Aucun avantage décisif sur rmcp, qui sert déjà les deux ères. |
| Cœur Rust appelé depuis Python (extension native) | Garde FastMCP, l'interpréteur et la distribution PyPI, c'est-à-dire les sources de D-08, D-37 et du blocage Smart App Control. Double la chaîne de build. |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §0, §1, §2, §6, §10 à §13 (révision 2) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §3.1, §3.2, §3.6 (Q1, Q2, Q6) ; [01-existant.md](../plan/01-existant.md) ; [02-architecture.md](../plan/02-architecture.md) §13 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §4 à §6 ; [08-distribution.md](../plan/08-distribution.md) §3.2.
- Recherche : [01-parite.md](../recherche/01-parite.md) §1, §2.4, §11.3, §14 ; [02-defauts.md](../recherche/02-defauts.md) §1, D-02 à D-08, D-24, D-36 à D-38 ; [09-rmcp-2026.md](../recherche/09-rmcp-2026.md) §0 à §2.
- Référence Python (`d3b5b13`, lecture seule) : `src/fr_eli_mcp/__main__.py:1-9`, `README.md:22-42`, `src/fr_eli_mcp/server.py:69-78`.
