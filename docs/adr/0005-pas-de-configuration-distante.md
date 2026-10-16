# ADR-0005 — Aucune configuration distante ni télémétrie

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M2 (configuration de l'adapter PISTE), M4 (binaire), transverse Distribution
- **Questions liées** : —
- Révisé le 2026-10-15 (revue) : liste fermée des variables, dont les quatre réglages avancés, et avertissement « ignored » des variables héritées non lues (00 §1, AM-05) ; règles des surcharges, `EnvMismatch` sans déduction et mode dégradé (00 §1, AM-27).

## Contexte

- **Le manifeste distant de la référence** (recherche 02 §2.c, D-04, gravité bloquante ; recherche 01 §9, E-07) :
  - `runtime.py` télécharge `fr-runtime.json.gz` depuis `releases/latest` du dépôt GitHub (`runtime.py:22-25`, `:55-58`) ;
  - ce manifeste peut **remplacer `base_url`**, sans contrôle d'hôte ni de schéma (`runtime.py:85-92`) ;
  - le jeton Bearer est ensuite envoyé à cet hôte (`client.py:150`, `:161-163`) ;
  - le téléchargement a lieu **même si `FR_ELI_BASE_URL` est défini**, car le défaut de `os.environ.get` est évalué avant (`client.py:46-47`), et même sans identifiants (`client.py:126-128`) ;
  - le `.sha256` publié n'est jamais vérifié ; la décompression gzip n'est pas bornée (`runtime.py:57-58`).
- **Ce qu'un éditeur de release compromis pourrait faire** : recevoir les jetons, lire toutes les requêtes du juriste et servir de **faux textes de loi** présentés comme « verbatim from Legifrance » (`server.py:50`).
- **Télémétrie.** Le compteur de téléchargements de cet actif sert de « métrique d'activation » (`runtime.py:7-8`, `release.yml:46`). Cela contredit la CONSTITUTION de la référence, art. 3 : « The server talks only to the PISTE OAuth + Legifrance endpoints and the local filesystem » (`CONSTITUTION.md:31-32`).
- **Blocage de la boucle async.** Le fetch `urllib` synchrone (délai de 15 s) gèle la boucle asyncio au premier appel d'outil (D-05).
- **CGU PISTE** : clés « strictement réservées à votre application » ; suspension possible en cas de compromission (recherche 05 §4.1).

## Décision

1. **La configuration est 100 % locale.**
   - Variables d'environnement `LF_*` de la liste fermée de 00 §1, **lues une fois au démarrage** (corrige E-24) ; toute nouvelle variable amende 00 §1.
   - Réglages avancés de l'adapter PISTE (`LF_PISTE_RPS`, `LF_PISTE_BURST`, `LF_PISTE_CONCURRENCY`, `LF_PISTE_MOJIBAKE_REPAIR`) : bornés ; une valeur invalide donne le défaut et un `warn!`, affichés par `check-config` ; ils ne sont déclarés ni dans `server.json` ni dans le manifeste MCPB (README et `check-config` seulement).
   - Compatibilité : lecture des `FR_ELI_CLIENT_ID`, `FR_ELI_CLIENT_SECRET`, `FR_ELI_OAUTH_URL`, `FR_ELI_BASE_URL` et `FR_ELI_AUDIT_DIR`, avec avertissement de dépréciation sur stderr ; `LF_*` l'emporte si les deux sont définies.
   - Aucun fichier de configuration, distant ou local, aucun manifeste. `FR_ELI_RUNTIME_URL`, `FR_ELI_CACHE_DIR`, `FR_ELI_ENV`, `PISTE_CLIENT_ID` et `PISTE_CLIENT_SECRET` ne sont jamais lues ; définies, elles produisent un avertissement « ignored » au démarrage et dans `check-config`.
2. **Sorties réseau limitées** aux hôtes PISTE de l'environnement choisi : jeton OAuth et API. S'y ajoutent :
   - les surcharges explicites `LF_PISTE_OAUTH_URL` et `LF_PISTE_BASE_URL` ;
   - en M9, le téléchargement explicite des dumps DILA ([ADR-0004](0004-piste-puis-dila.md)).

   Aucune URL renvoyée par l'API (`lienJo`, `urlCC`…) n'est jamais appelée (04 §2.3). La liste des hôtes contactés est publiée (08 §1.4).
3. **Règles des surcharges** `LF_PISTE_OAUTH_URL`, `LF_PISTE_BASE_URL` et `FR_ELI_*` (00 §1, 04 §2.3) :
   - `https` obligatoire ; `http` admis vers 127.0.0.1, ::1 et localhost seulement ;
   - ni userinfo, ni requête, ni fragment ; chemin officiel exact `/dila/legifrance/lf-engine-app` ;
   - un hôte hors des quatre hôtes officiels (`sandbox-oauth.piste.gouv.fr`, `oauth.piste.gouv.fr`, `sandbox-api.piste.gouv.fr`, `api.piste.gouv.fr`) est **accepté** comme choix local explicite, avec l'avertissement `CustomHost` au démarrage, dans `check-config` et comme lacune de `fr_coverage` ;
   - un hôte officiel d'un autre environnement que `LF_PISTE_ENV` (explicite ou défaut `sandbox`) est un problème `EnvMismatch`, dont le message demande `LF_PISTE_ENV=production` ; aucune déduction implicite de l'environnement.
4. **Aucune télémétrie** :
   - ni compteur d'activation, ni vérification de version en ligne ;
   - aucun programme de mise à jour (`install-updater = false`, 08 §1.3) ;
   - aucun export OpenTelemetry (02 §8.6 ; annexe D §2, ligne 17) ;
   - les journaux `tracing` vont sur stderr seulement, et l'audit reste local ([ADR-0008](0008-audit-decorateur-strict.md)).
5. **Démarrage sans réseau.** Le jeton OAuth est demandé paresseusement, au premier appel d'outil (00 §6). Sans identifiants, ou sur une configuration invalide (`EnvMismatch`, URL refusée), le serveur démarre en **mode dégradé** (`error!`) : chaque outil PISTE répond `[config_error] …` sans E/S (D-39), et `fr_coverage` et `check-config` décrivent le problème (00 §1). Refuser de démarrer ne laisserait au client MCP qu'un « server failed » sans contexte.

## Conséquences

### Positives

- D-04, D-05 et E-07 disparaissent par construction.
- Les sorties réseau sont auditables et documentées (README, `PRIVACY.md`).
- La confidentialité des recherches ne dépend que de PISTE.
- **Aucune clé n'est embarquée** : la distribution publique devient possible ([ADR-0013](0013-distribution.md)).

### Négatives

- Plus aucun moyen de **repointer à distance** les installations (bascule sandbox vers production, changement d'URL) : il faut une nouvelle version ou une variable `LF_*`.
- Aucune statistique d'usage : seuls les signalements des utilisateurs révèlent un blocage, par exemple par Smart App Control (09).
- La mise à jour exige une réinstallation explicite.
- Un hôte personnalisé reste possible et peut être mal choisi. C'est un compromis assumé avec la liste blanche stricte de la recherche 02 D-04 (02 §13.1 n° 8) : l'avertissement et la lacune de couverture rendent ce choix visible.

### À surveiller

- La prise en compte de `HTTPS_PROXY` par reqwest derrière un proxy d'entreprise : **NON VÉRIFIÉ** [v:reqwest-proxy-env].
- Les URL de production n'ont jamais été appelées : **NON VÉRIFIÉ** [v:oauth-production-live].
- Toute demande future (diagnostic à distance, métriques) exige un **nouvel ADR**, avec un opt-in explicite.

### Tests qui la gardent

Tests du catalogue de non-régression pour D-04 (06 §3.1) et de la configuration (06 §2.2, §2.3) :

- `no_network_call_except_piste` ;
- `fr_eli_runtime_url_is_ignored` ;
- `production_sources_contain_only_allowlisted_urls` ;
- `config_rejects_http_scheme_except_loopback` ;
- `config_rejects_userinfo_query_fragment` ;
- `config_warns_on_custom_host` ;
- `config_rejects_env_mismatch` et `config_env_mismatch_on_production_host_with_default_env` (aucune déduction) ;
- `config_problem_starts_in_degraded_mode` et `coverage_degraded_on_config_problem` ;
- `invalid_advanced_setting_falls_back_to_default_with_warn` ;
- `urls_in_responses_are_never_fetched`.

S'y ajoute, pour D-05, `server_answers_discover_while_tool_waits_upstream`.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Manifeste distant signé (clé publique embarquée) | Garde une sortie réseau et un levier de télémétrie, et ajoute une gestion de clés. Le besoin réel (sandbox ou production) se règle par `LF_PISTE_ENV`. |
| Liste blanche stricte et option `--allow-custom-host` (recherche 02 D-04) | 00 §1 accepte les surcharges `LF_PISTE_*_URL` comme choix explicite. L'avertissement et la lacune de couverture suffisent (02 §13.1 n° 8). |
| Télémétrie opt-in | Sans besoin exprimé. Contraire à la CONSTITUTION de la référence et à l'esprit des CGU. Coût de conformité RGPD. |
| Mise à jour automatique (`install-updater`) | Appel réseau non sollicité. La mise à jour doit rester un geste explicite (08 §1.3). |
| `LF_PISTE_ENV` obligatoire, sans défaut (recherche 02 D-18) | 00 §1 fixe le défaut `sandbox`. Il est affiché dans `fr_coverage` et rappelé au démarrage (02 §13.1 n° 5). |
| Déduire `LF_PISTE_ENV` de l'hôte officiel surchargé | Masquerait l'erreur : des identifiants sandbox envoyés à la production échouent toujours et faussent l'espace de noms du cache (00 §1, `EnvMismatch`). |
| Refuser de démarrer sur une configuration invalide | Le client MCP n'afficherait qu'un échec sans contexte ; le mode dégradé explique le problème par `fr_coverage` et `[config_error]` (00 §1). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §1 (révision 2, AM-05, AM-27), §6 ; [02-architecture.md](../plan/02-architecture.md) §8.2, §8.3, §8.6, §13, §13.1 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §2.2 à §2.4 ; [08-distribution.md](../plan/08-distribution.md) §1.3, §1.4 ; [06-qualite.md](../plan/06-qualite.md) §3.1.
- Recherche : [02-defauts.md](../recherche/02-defauts.md) §2.c, D-04, D-05, D-18, D-39, D-40 ; [01-parite.md](../recherche/01-parite.md) §9, E-07, E-24 ; [05-piste-dila.md](../recherche/05-piste-dila.md) §4.1.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/runtime.py:7-8, 22-25, 55-58, 85-92` ; `src/fr_eli_mcp/client.py:46-47, 126-128, 150, 161-163` ; `CONSTITUTION.md:31-32` ; `.github/workflows/release.yml:44-69`.
