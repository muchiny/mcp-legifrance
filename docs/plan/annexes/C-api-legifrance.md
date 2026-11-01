# Annexe C — API Légifrance (PISTE), Judilibre et dumps DILA

> **Statut** : annexe de référence du plan, rédigée le 2026-10-15. Elle décrit l'API Légifrance telle que la documentent le Swagger 2.4.2 et les documents de la DILA, et telle que l'observent les 16 fixtures de la référence Python. Elle est subordonnée à [00-decisions.md](../00-decisions.md). Les choix de conception de l'adapter (classification des erreurs, réessais, mémo de pagination, mapping) relèvent de [04-adapter-piste.md](../04-adapter-piste.md), qui s'appuie sur cette annexe. En cas d'écart, 00 prévaut, puis 04 ; l'écart est signalé au §9. M0-M1 sont livrés (plan 1) : pour le domaine (`CivilDate`, `Id<K>`, `Fond`), le code livré fait foi et les écarts sont en 00 §11.1 ; les points reportés par la décision du 2026-10-16 (suffixe de version, TOC en arbre `TocNode` avec renommage du `TocEntry` livré en `TocArticle`, `text::encoding`) sont des tâches de tête de M2.
> **Jalons** : M2 (7 endpoints de v0.1, plus `/consult/legi/tableMatieres` si V-96 l'exige), M5 (pagination vérifiée en direct), M6 (résolution), M7 (navigation), M9 (dumps DILA). Judilibre : hors périmètre jusqu'à décision (§6.6).
> **Révision** : alignée sur 00 révision 2 du 2026-10-15 (amendements AM-02, AM-03, AM-28, AM-29 et AM-39 notamment ; journal en 00 §14) et sur le contrôle final du 2026-10-16 (étiquette de référence de V-76, versions de référence des crates, 00 §2). Les choix adoptés par défaut sur une question de l'utilisateur sont appliqués ici et restent à confirmer (Q3, Q10, Q15 notamment, 00 §13).
> **Lecteurs** : développeurs de `lf-adapter-piste` et de `lf-adapter-dila`, auteurs des tests wiremock et des smoke tests live, et toute personne qui doit savoir ce que l'API permet sans accès au portail PISTE.
> **Conventions** : « VÉRIFIÉ » signifie lu dans une source primaire (Swagger, document DILA, CGU, fixture enregistrée) ; « tiers » signifie observé dans le code d'un client tiers, sans recoupement ; **NON VÉRIFIÉ** suivi d'une étiquette `[v:…]` signifie à confirmer (registre `V-xx` dans [09-risques-et-questions.md](../09-risques-et-questions.md), liste au §8). Les extraits Rust sont **non compilés, à valider au jalon qui l'implémente**. Dans les corps de requête, les champs requis par le Swagger sont en **gras**.

Documents liés : [00-decisions.md](../00-decisions.md) §3, §4, §5, §8, §10, §11, §12, §13 (normatif) ; [02-architecture.md](../02-architecture.md) (ports, erreurs) ; [03-domaine.md](../03-domaine.md) (identifiants, dates, `Fond`) ; [04-adapter-piste.md](../04-adapter-piste.md) (comportement de l'adapter) ; [05-outils-mcp.md](../05-outils-mcp.md) (paramètres des outils) ; [06-qualite.md](../06-qualite.md) (tests) ; [07-feuille-de-route.md](../07-feuille-de-route.md) et les plans d'implémentation exécutables (retirés : voir [README du plan §3.5](../README.md#35-plans-dimplémentation-retirés)) ; [09-risques-et-questions.md](../09-risques-et-questions.md) ; [A-parite.md](A-parite.md) ; [B-citations.md](B-citations.md) ; [donnees/codes-table.md](donnees/codes-table.md) ; ADR-0004 (PISTE d'abord, DILA ensuite derrière le même port), ADR-0005, ADR-0007, ADR-0010, ADR-0011 (index : [../../adr/README.md](../../adr/README.md)). Rapports bruts : [recherche 05](../../recherche/05-piste-dila.md) et [recherche 03](../../recherche/03-dto.md).

## Sommaire

- [0. Résumé](#0-résumé)
- [1. Accès à l'API](#1-accès-à-lapi)
- [2. Catalogue des 68 opérations](#2-catalogue-des-68-opérations)
- [3. Recherche (`POST /search`)](#3-recherche-post-search)
- [4. Formes JSON des réponses utilisées](#4-formes-json-des-réponses-utilisées)
- [5. DTO serde minimaux](#5-dto-serde-minimaux)
- [6. Judilibre](#6-judilibre)
- [7. Dumps open data DILA (M9)](#7-dumps-open-data-dila-m9)
- [8. Points NON VÉRIFIÉS](#8-points-non-vérifiés)
- [9. Écarts entre sources et décisions](#9-écarts-entre-sources-et-décisions)
- [10. Questions ouvertes propres à l'annexe](#10-questions-ouvertes-propres-à-lannexe)
- [Sources](#sources)

---

## 0. Résumé

- **Surface** : 68 opérations (59 `POST`, 9 `GET`) réparties en 6 contrôleurs : Consult 39, List 13, Search 5, Suggest 4, Chrono 4, Misc 3. Le Swagger 2.4.2 est identique en sandbox et en production, hors hôtes et URLs OAuth. Le plan utilise 7 chemins en v0.1, plus `/consult/legi/tableMatieres` si V-96 (bloquante en M2) l'exige, 6 de plus en M6 (sans compter `/search` en recherche exacte et filtré par la facette `ECLI`) et 3 en M7, soit 17 chemins au plus, 18 si `/consult/jorf` est retenu à l'ouverture de M7 (V-135) ; `/suggest` est retiré (00 §8 révision 2, AM-29). Aucun chemin absent du Swagger n'est appelé (00 §8).
- **Accès** : compte PISTE nominatif, une application par environnement, OAuth2 `client_credentials`, jeton de 3 600 s envoyé en `Bearer`. Les quotas s'appliquent par seconde, minute ou jour, à l'application ou à une méthode. Leurs valeurs ne sont pas publiques.
- **Codes HTTP** : le Swagger ne déclare que 200, 201, 401, 403 et 404. Les 400 (identifiant `JURITEXT` inexistant sur `/consult/juri`), 429 (quota) et 500 sont observés ou rapportés par des tiers. Réessais : 3 tentatives au plus sur 429, 502, 503, 504 et erreurs réseau, 2 au plus sur 500 (00 §8, AM-03).
- **Recherche** : 14 fonds, 32 `typeChamp`, 5 `typeRecherche`, 3 niveaux d'opérateurs, filtres sous 4 formes, facettes et tris propres à chaque fonds (document officiel), `pageSize` ≤ 100, `typePagination` à renvoyer tel que reçu.
- **Formes JSON** : dates sous 3 encodages, horodatages de date à minuit UTC dans les fixtures (d'où `singleDate` à midi Europe/Paris, 00 §8), sentinelle `2999-01-01`, epoch négatifs, identifiants suffixés `_JJ-MM-AAAA`, texte servi deux fois (`texte` et `texteHtml`, 81 à 90 % du JSON d'une décision), mojibake dans 6 fixtures sur 16 (origine probable : la capture), fixtures élaguées et tronquées.
- **DTO** : un module par endpoint, tolérants, tout en `Option` avec `#[serde(default)]`, quatre helpers (`null_default`, `opt_nonempty`, `opt_raw_date`, `opt_string_or_number`) et `opt_u64_lenient` (04 §8.2).
- **Judilibre** : API distincte de la Cour de cassation, souscription PISTE séparée. Elle apporte le zonage des décisions et des rapprochements identifiés. Hors périmètre.
- **Dumps DILA** : un global `Freemium_*_global_*.tar.gz` puis des incréments quotidiens, à appliquer dans l'ordre strict, avec listes de suppression. XML LEGI documenté par le code tiers ; règle de vigueur `debut ≤ D < fin` ; Licence Ouverte 2.0. C'est la base de `lf-adapter-dila` (M9).

---

## 1. Accès à l'API

### 1.1 Portail PISTE et souscription

Étapes, toutes manuelles et faites une seule fois par l'utilisateur (recherche 05 §4.1 ; référence DISCOVERY.md:15-16) :

1. Créer un compte sur PISTE (`piste.gouv.fr`) et accepter les CGU de PISTE. « L'accès à PISTE se fait par un compte nominatif » (CGU API l.177).
2. Dans le catalogue de PISTE, choisir l'API Légifrance et accepter ses CGU. La mise à disposition est gratuite (CGU API l.159-169).
3. Une **application par environnement** : sandbox et production sont deux applications distinctes, chacune avec son `client_id` et son `client_secret`, lus dans la section « OAuth Credentials » de l'application (référence client.py:55-56). Des identifiants sandbox présentés à `oauth.piste.gouv.fr` reçoivent `invalid_client` (référence SOURCES.md:12).
4. Les quotas de l'application se lisent dans l'onglet « Applications », puis l'application, « Actions » et « Consulter les quotas » (CGU API l.224-227).

Le projet ne fournit aucun identifiant. Un compte et une application sandbox sont nécessaires aux smoke tests live (Q3, adoptée par défaut : sandbox avant M2, production avant v0.1.0 ; en production, sondes en lecture seule lancées à la main par le mainteneur, jamais d'identifiants de production en CI, 00 §10). Judilibre exige une souscription séparée (§6.2).

### 1.2 Environnements

| Aspect | Sandbox | Production | Source |
|---|---|---|---|
| Endpoint token | `https://sandbox-oauth.piste.gouv.fr/api/oauth/token` | `https://oauth.piste.gouv.fr/api/oauth/token` | doc DILA « exemples » l.115 ; Swagger `securityDefinitions.SecurityProfile.OAuthAccessCode.tokenUrl` |
| Base de l'API | `https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app` | `https://api.piste.gouv.fr/dila/legifrance/lf-engine-app` | Swagger `host` + `basePath` des deux copies ; référence client.py:28-29 |
| Surface | Swagger 2.4.2 | identique : seuls `host`, `schemes` et URLs OAuth diffèrent | diff programmatique (recherche 05 §0) |
| Données | « Les données ne sont pas identiques à celles de Production » ; les correctifs y arrivent d'abord | données de référence | Swagger `info.description` |
| Disponibilité | aucun engagement (tests de performance, réindexation) ; « peut donc être indisponible sans information préalable » | 95 % par jour visés, engagement de moyens, aucun niveau minimal garanti | CGU API l.187-202 |
| État pour nous | utilisé en direct par la référence | jamais appelé : **NON VÉRIFIÉ** [V-59 · oauth-production-live] | référence DISCOVERY.md:3, SOURCES.md:12 |

- `LF_PISTE_ENV=sandbox|production` (défaut `sandbox`) fixe ensemble les deux URLs. Les surcharges `LF_PISTE_OAUTH_URL` et `LF_PISTE_BASE_URL` sont validées (00 §1 ; 04 §2.3) : `https` obligatoire (sauf boucle locale), hôte hors des quatre hôtes officiels accepté avec l'avertissement `CustomHost`, hôte officiel d'un autre environnement que `LF_PISTE_ENV` = problème `EnvMismatch` (mode dégradé, `config_error`, sans déduction implicite), chemin officiel exact `/dila/legifrance/lf-engine-app`.
- Un client tiers affirme que sandbox et production partagent les mêmes données et ne diffèrent que par les quotas (recherche 05 §3.2, `[KMCP]/README.md:67`). Le Swagger officiel le contredit : affirmation rejetée. L'ampleur réelle de l'écart de données est **NON VÉRIFIÉ** [V-61 · sandbox-prod-donnees].
- Version : la copie lue déclare `info.version = 2.4.2`. Elle a été générée le 2025-05-28 par un projet tiers (recherche 03 §1.1). Un fil data.gouv.fr de juillet 2025 signale une spec publiée en 1.6.2.5 alors que PISTE affichait 2.4.2 (recherche 05 §0). La version servie aujourd'hui par PISTE est **NON VÉRIFIÉ** [V-74 · swagger-version-servie]. Le test de dérive de 04 §7.1 compare les chemins appelés à une liste versionnée des chemins de la 2.4.2.

### 1.3 OAuth2 `client_credentials`

Flux RFC 6749 §4.4 (doc DILA « exemples » l.107). Requête officielle (l.115-129), en-têtes non essentiels omis :

```http
POST /api/oauth/token HTTP/1.1
Host: sandbox-oauth.piste.gouv.fr
Content-Type: application/x-www-form-urlencoded
Accept-Encoding: gzip,deflate

grant_type=client_credentials&client_id=<client_id>&client_secret=<client_secret>&scope=openid
```

Réponse documentée (l.135-151) :

```json
{ "access_token": "th2uv3lq…", "token_type": "Bearer", "expires_in": 3600, "scope": "openid" }
```

- `expires_in` est en secondes (l.151). Observé en sandbox par la référence : `token_type=Bearer`, `expires_in=3600`, scope `openid resource.READ` (référence DISCOVERY.md:9-12).
- Appels à l'API : en-tête `Authorization: Bearer <access_token>` ; pour les `POST`, aussi `accept: application/json` et `Content-Type: application/json` (l.155-163). L'exemple officiel teste avec `GET …/list/ping` (l.159).
- Le Swagger déclare aussi des profils `implicit` et `accessCode` (`authorizationUrl https://oauth.piste.gouv.fr/api/oauth/authorize`, scope `openid`). Ils sont inutiles pour un serveur et ne sont pas utilisés.
- Les formes réelles des erreurs de l'endpoint token (statut, corps) ne sont pas connues. Seule indication : la valeur `invalid_client` pour des identifiants d'un autre environnement (statut et corps exacts non relevés, référence SOURCES.md:12 ; RFC 6749 §5.2 prévoit 400 ou 401) **NON VÉRIFIÉ** [V-60 · oauth-reponses]. Les tests de l'adapter couvrent donc 400 **et** 401 (04 §3.5, §13.2).
- Comportement retenu (00 §8 ; détail 04 §3) : formulaire ci-dessus, jeton en mémoire (`SecretString`), renouvelé 60 s avant expiration, rafraîchissement mono-vol, un seul renouvellement après un 401 de l'API puis `Auth`. Une erreur de l'endpoint token (400, 401 ou 404) donne `Auth` ou `Config`, rendu `config_error`, jamais `not_found` (00 §5, corrige D-10, D-12, E-20).

### 1.4 Quotas

Ce que disent les sources officielles (CGU API l.212-231 ; Swagger `info.description` : « Les appels sont limités par quotas ») :

| Dimension | Valeurs possibles |
|---|---|
| Fenêtre | par seconde, par minute, par jour |
| Mesure | nombre de requêtes, ou bande passante |
| Portée | l'application consommatrice entière, ou une méthode particulière de l'API |
| Consultation | console PISTE, « Consulter les quotas » de l'application |
| Évolution | modifiables à tout moment par la DILA, avec information par courriel |

- **Valeurs numériques** : non publiques, aucune source officielle trouvée : **NON VÉRIFIÉ** [V-62 · quota-piste-prod]. À relever dans la console PISTE pour les deux environnements, méthode par méthode si la granularité l'impose.
- Pratiques de clients tiers, qui ne sont pas des quotas : 5 appels/s (README du même projet : 10/s), pause de 0,5 s, 200 ms entre appels de test, 4 requêtes simultanées (recherche 05 §3.3).
- Décisions du plan : `governor` global au processus, consulté par `check()` avant chaque tentative (jamais `until_ready`), 5 req/s, rafale 10 (`LF_PISTE_RPS`, `LF_PISTE_BURST`) ; sémaphore de 4 requêtes en vol (`LF_PISTE_CONCURRENCY`) (00 §1, §8 ; 04 §5.4, §5.5).
- Dépassement : 429, selon des tiers concordants (recherche 05 §3.4). La présence et le format de `Retry-After` sont **NON VÉRIFIÉ** [V-63 · retry-after-piste]. Un tiers rapporte des `401 invalid_client` pendant de courtes fenêtres de limitation : **NON VÉRIFIÉ** [V-64 · 401-sous-throttling].
- Les quotas valent pour toute l'application. En mode HTTP partagé (M8), ils seraient donc communs à tous les utilisateurs du serveur (§1.7.3, Q4).

### 1.5 Codes HTTP

Le Swagger déclare pour **chaque** opération `POST` les réponses `200`, `201`, `401`, `403`, `404`, et pour chaque `GET` `200`, `401`, `403`, `404` (VÉRIFIÉ par script, §2.1). Aucune opération ne déclare 400, 429 ou 5xx.

| Statut | Déclaré | Observé ou rapporté | Source | Traitement retenu (00 §5, 04 §5.1, §11) |
|---|---|---|---|---|
| 200 | oui | toutes les fixtures | fixtures | DTO puis entité |
| 201 | oui (`POST`) | jamais observé | Swagger | traité comme un 2xx |
| 400 | non | `/consult/juri` avec un `JURITEXT` bien formé mais inexistant : corps `"message": "L'expression à valider est fausse."` ; même 400 affirmé pour `CETATEXT`, sans sonde (docstring d'un test unitaire sur réponse simulée) ; vérifié en sandbox le 2026-10-28 : `CETATEXT` et `CONSTEXT` donnent le même 400 [V-68 · introuvable-par-endpoint] | `JURITEXT` : pylegifrance, sonde live du 2026-04-13 avec `JURITEXT000000000000` et `JURITEXT999999999999` (`pylegifrance/fonds/juri.py:124-135`, `tests/integration/fonds/juri/test_verification_endpoints.py:52-59`) ; `CETATEXT` : `tests/unit/fonds/test_juri_verification_endpoints.py:108-112` (recherche 03 §3.4 ; 04 §11.1) | `NotFound` si le marqueur est présent sur une consultation, sinon `InvalidRequest`, rendu `[upstream_error] Legifrance rejected the request (HTTP 400): {détail PISTE nettoyé, ≤ 200 caractères}` (00 §5, AM-28) ; endpoint token : `Auth` (`config_error`) |
| 401 | oui | jeton absent ou expiré ; sur l'endpoint token, la valeur `invalid_client` (mauvais environnement), statut non relevé (400 ou 401, RFC 6749 §5.2 ; [V-60 · oauth-reponses], §1.3) | référence client.py:164-168, SOURCES.md:12 | API : un renouvellement puis `Auth` ; token : `Auth` (rendu `config_error`) |
| 403 | oui | application non abonnée (constaté sur Judilibre avec un jeton Légifrance) | référence DISCOVERY.md:89-93 | `Auth`, rendu `config_error` avec le message « check that the PISTE application is subscribed to the Legifrance API » (00 §5, AM-28) ; sens exact sur l'API Légifrance **NON VÉRIFIÉ** [V-65 · forme-403-piste] |
| 404 | oui | ressource inconnue (supposé) ; mauvais chemin ou mauvaise méthode (ex. `GET /suggest`) | recherche 05 §3.4 ; `[JRX]` | `NotFound` ou `Config` selon 04 §11.2 ; forme réelle : aucun 404 observé en sandbox le 2026-10-28 (route inexistante : 403 JSON de la passerelle) [V-66 · forme-404-piste] |
| 429 | non | dépassement de quota | tiers concordants | 3 tentatives au plus, `Retry-After` attendu jusqu'à 30 s (au-delà : refus d'attendre, échec immédiat ; valeur propagée écrêtée à 1 h), puis `RateLimited { retry_after }` |
| 500 | non | erreur serveur ; selon un tiers, aussi facette inconnue, champ JSON inconnu ou `fond=ALL` avec filtres (« Une exception non gérée est survenue ») | `[JRX]` (recherche 05 §2.2) | 2 tentatives au plus (00 §8, AM-03), puis `Upstream { status: 500, retryable: false }` : un 500 répété est présumé déterministe ; retour à 3 tentatives si V-69 montre des 500 transitoires fréquents ; cause vérifiée en sandbox le 2026-10-28 : 500 déterministes pour une facette inconnue et pour `fond = ALL` avec un filtre de date [V-69 · 500-requete-invalide] |
| 502, 503, 504 | non | — | — | 3 tentatives au plus, puis `Upstream { status, retryable: true }` |

Les réessais sont plafonnés par le délai global de l'outil (`LF_TOOL_DEADLINE_S`, `Deadline`, 00 §3). `retryable` ne sert qu'au message : seul un échec `retryable: true` (429, 502, 503, 504, réseau, timeout) invite le modèle à réessayer (00 §8). Toute contrainte imputable à l'entrée (dates, fond sans facette, pagination profonde, formats) est rejetée en `invalid_arg` avant l'appel du port : un 400 de PISTE sans marqueur « introuvable » signale donc un défaut de l'adapter (00 §5).

Le comportement « introuvable » des autres consultations (400, 200 avec enveloppe `null` ou vide, 500 pour un corps `{}`, jamais de 404) est vérifié en sandbox le 2026-10-28 pour les endpoints de la v0.1 [V-68 · introuvable-par-endpoint]. La forme générale des corps d'erreur est vérifiée en sandbox le 2026-10-28 : six champs JSON [V-67 · corps-erreur-piste]. Forme du 400 de `/consult/juri` :

```json
{"timestamp": "…", "error": 400, "status": "Bad Request", "message": "L'expression à valider est fausse.", "errorCode": null, "exceptionDetails": null}
```

(04 §11.1, d'après pylegifrance `tests/unit/fonds/test_juri_verification_endpoints.py:115-121`.)

### 1.6 Compression et en-têtes

- L'exemple OAuth officiel envoie `Accept-Encoding: gzip,deflate` (l.117). La sandbox ne compresse pas ses réponses : aucun `Content-Encoding` même avec `Accept-Encoding: gzip` (mesure manuelle du 2026-10-28, V-70 levé, constat négatif ; production non sondée) ; l'adapter l'active et borne la taille après décompression (04 §4.3).
- Aucun en-tête de pagination ni de version n'est documenté. `GET /misc/commitId` renvoie une map de chaînes « Informations déploiement et versionning » (§2.7), non utilisée.

### 1.7 CGU et licence

Sources : CGU API Légifrance **v1.0 du 15/12/2022** (canonique), CGU bêta v1.1 du 04/08/2020, copies archivées par pylegifrance (recherche 05 §4). Les CGU générales du portail PISTE (AIFE) n'ont pas pu être lues : **NON VÉRIFIÉ** [V-175 · cgu-piste-generales]. Lecture non juridique.

#### 1.7.1 Obligations résumées

| Obligation (CGU API v1.0) | Source | Conséquence pour le plan |
|---|---|---|
| Documents du contrat : mentions légales Légifrance, CGU PISTE, CGU API, Licence Ouverte 2.0 ; les CGU API prévalent en cas de contradiction | l.94-104 | — |
| Âge minimal : 15 ans, majorité numérique | l.110-111 | — |
| Compte PISTE nominatif ; identifiants conformes à l'état de l'art (CNIL, ANSSI) | l.171-177 | aucun identifiant fourni par le projet ; chaque utilisateur crée le sien |
| OAuth 2.0 « dans chaque Requête » | l.179-180 | en-tête `Bearer` |
| « vos clés d'authentification à l'API Légifrance sont strictement réservées à votre application et ne doivent en aucun cas être rendues publiques ou divulguées à un tiers » | l.182-183 | secrets en `SecretString`, jamais journalisés ni renvoyés (00 §5 ; 04 §3.6) ; `server.json` les déclare `isSecret: true` (00 §12) ; fixtures enregistrées sans en-tête `Authorization` ; mode HTTP soumis à Q4 |
| Responsabilité exclusive de l'usage ; respect de la Licence Ouverte 2.0 et des règles de protection des données personnelles ; obligation d'indemniser la DILA | l.235-246 | attribution (§1.7.4) ; aucune exploitation des noms de magistrats (§1.7.5) |
| Sécurité des postes : antivirus, correctifs, pare-feu, verrouillage de session… | l.255-272 | recommandations dans le README (08) |
| Compromission (« surconsommation sur l'API Légifrance, publication des identifiants en ligne, etc. ») : réinitialiser les identifiants sur PISTE et prévenir la DILA ou l'AIFE | l.274-285 | procédure dans le README et la politique de sécurité (08) |
| Données non garanties (contenu, complétude, fraîcheur) et **non opposables** : « seul les PDF signés publiés au Journal officiel de la République Française sont opposables » | l.289-316 | avertissement dans les instructions serveur et `fr_coverage` ([05-outils-mcp.md](../05-outils-mcp.md)) |
| Usage commercial autorisé, « au risque et péril de l'Utilisateur » | l.318-332 | compatible avec Apache-2.0 pour le code (Q2) |
| Quotas modifiables à tout moment | l.212-231 | débit configurable (§1.4) |
| Suspension sans préavis et résiliation de plein droit en cas de manquement | l.336-345 | débit prudent par défaut |
| Notifications : courriel du compte PISTE et `retours-legifrance-modernise@dila.gouv.fr` (aussi le contact pour signaler une anomalie de données) | l.347-353 ; l.306-311 | contact cité dans la documentation |
| Données collectées par la DILA via PISTE : courriel et statistiques agrégées d'usage, conservées 12 mois au plus ; contact `rgpd@dila.gouv.fr` | l.367-402 | — |

#### 1.7.2 Ce que les CGU ne disent pas

- Aucune clause ne limite la mise en cache ni le stockage local des réponses (lecture intégrale des CGU API, recherche 05 §4.2). Le cache mémoire de 00 §9 n'appelle donc pas de réserve au regard des CGU API. Les CGU PISTE générales restent à lire [V-175 · cgu-piste-generales].
- Aucune clause sur une application mutualisée servant des tiers. C'est l'objet de Q4.

#### 1.7.3 Partage de compte et mode HTTP (Q4)

| Usage | Lecture des CGU | Position du plan |
|---|---|---|
| stdio local (v0.1 à v0.3) : chaque utilisateur lance le binaire avec ses propres identifiants | conforme : l'application est celle de l'utilisateur | mode par défaut |
| Distribution (binaires, MCPB, OCI, registre) | aucune clé embarquée ; variables `LF_PISTE_CLIENT_ID`/`LF_PISTE_CLIENT_SECRET` fournies par l'utilisateur | 00 §12 ; [08-distribution.md](../08-distribution.md) |
| Serveur HTTP local (M8, profil jeton statique) | même application, même utilisateur | conditionné à M8 |
| Serveur HTTP distant avec les identifiants de l'opérateur, servant des utilisateurs finaux | les clés ne sont pas divulguées, mais elles servent « votre application » au bénéfice de tiers ; des services publics tiers fonctionnent ainsi (recherche 05 §4.3) ; interprétation **NON VÉRIFIÉ** [V-175 · cgu-piste-generales] | local seulement tant que Q4 n'est pas tranchée (00 §6, Q4) |
| Identifiants apportés par chaque utilisateur au serveur distant | conforme au compte nominatif ; le serveur manipulerait des secrets de tiers | option à arbitrer en M8, jamais de stockage disque |

Interdits de conception, quel que soit le mode : ne jamais renvoyer le `client_secret` ni le jeton à un client MCP, ne jamais relayer un jeton client vers PISTE (00 §6, pas de *token passthrough*), journaux expurgés (04 §12.5).

#### 1.7.4 Licence Ouverte 2.0 et attribution

- « Les données mises à disposition par l'interface de programmation sont soumises à la licence ouverte 2.0 » (CGU API l.360-365) ; le Swagger déclare aussi `info.license = Licence ouverte v2.0` (VÉRIFIÉ).
- La licence autorise copie, redistribution et réutilisation, y compris commerciale, à condition de mentionner au minimum le nom du concédant et la **date de dernière mise à jour** de l'information réutilisée. Un lien vers l'information suffit s'il assure une attribution effective. L'attribution ne doit suggérer aucun aval du concédant (recherche 05 §4.2, d'après le texte Etalab).
- Mise en œuvre (00 §12, adoptée par défaut, Q15, AM-39) : `fr_coverage` et les instructions serveur portent « Source : Légifrance / DILA — Licence Ouverte 2.0 » et la date de consultation (date du jour, Europe/Paris) ; dans `fr_coverage`, ce sont les champs `attribution` et `consulted_on`, et les instructions, statiques, renvoient à `consulted_on` (05 §4, §5.8) ; le README le rappelle ; mention « Légifrance / DILA » dans `NOTICE` (Q2) ; `source_url` sur chaque entité citable (00 §4). Qu'une mention globale suffise, sans mention dans chaque réponse, est **NON VÉRIFIÉ** [V-177 · attribution-licence-ouverte] (V-177).
- Date de mise à jour : l'API n'expose pas de champ uniforme. Champs candidats : `modifDate` (lawDecree, kaliText), `dateMaj` (ACCO), `lastUpdate` (`/list/code`), et `refInjection` dont la valeur `MD-20260424_200447_879_BDJQUOT` semble horodater l'injection (fixture consult_article.json). Leur sémantique est **NON VÉRIFIÉ** [V-108 · date-maj-par-document] (V-108). Un champ `last_update?` par document n'est ajouté (classe « ajout ») que lorsqu'un champ fiable de l'API est identifié (00 §12) ; d'ici là, la date de consultation en tient lieu (§10 question 5, Q15).

#### 1.7.5 Données personnelles et contenus sensibles

- Les décisions judiciaires sont diffusées **pseudonymisées** ; la référence ne le signalait pas (D-45). Les noms de magistrats et d'avocats (`president`, `rapporteur`, `avocatGl`, `avocats`, `commissaire`) ne sont pas déclarés dans les DTO (04 §8.3).
- La loi interdit de réutiliser l'identité des magistrats et des membres du greffe pour « évaluer, analyser, comparer ou prédire leurs pratiques professionnelles » (recherche 05 §5.1, d'après des résumés de presse). L'article exact (art. 33 de la loi n° 2019-222, codifié L. 10 CJA et L. 111-13 COJ) est **NON VÉRIFIÉ** [V-179 · profilage-magistrats-article]. Aucune fonctionnalité d'agrégation par personne n'est prévue.
- `dereferenced` (« Identifie si le contenu est référençable par les robots d'indexation ») figure dans toutes les enveloppes de consultation ; `inap` (informations nominatives à accès protégé, JORF, art. R. 221-15 et R. 221-16 CRPA, lexique DILA l.147-149). Recommandation : ne jamais indexer publiquement (mode HTTP distant, index tantivy de M9) un contenu `dereferenced = true` ou `inap = true`. L'obligation exacte est **NON VÉRIFIÉ** [V-180 · dereferenced-inap-obligation].

#### 1.7.6 Différences avec les CGU bêta de 2020

v2020 : API « en bêta », aucune disponibilité garantie, quotas sous « API Sélectionnées → Actions → Consulter les quotas », contact DPO `dpd@pm.gouv.fr`. v2022 : objectif de 95 % par jour en production, licence intégrée au contrat, âge de 15 ans, notifications par courriel, contact `rgpd@dila.gouv.fr` (recherche 05 §4.4).

---

## 2. Catalogue des 68 opérations

### 2.1 Vue d'ensemble

- Comptage par script sur la copie du Swagger 2.4.2 (§Sources) : **68 opérations**, 59 `POST` et 9 `GET` (5 `ping`, `GET /chrono/textCid/{textCid}`, 3 `GET /misc/*`). Tags : Chrono « Accès aux versions de texte », Consult « Accès aux textes », List « Liste les textes », Misc « Services divers », Search « Recherche sur les textes », Suggest « Autosuggestion/Autocompletion sur les textes et articles ». Le Swagger compte 179 définitions.
- Tous les `POST` consomment et produisent `application/json`, sauf les `ping` (`text/plain`, « pong »).
- Champ facultatif commun à de nombreuses requêtes de consultation : `searchedString`, « Texte de la recherche ayant aboutie à la consultation du texte ». Son effet (surlignage ?) n'est pas documenté. Le plan ne l'envoie jamais (04 §7.1).
- Champs communs à de nombreuses réponses : `executionTime` (ignoré) et `dereferenced` (§1.7.5).
- Colonne « Plan » : **M2**, **M6** ou **M7** = chemin utilisé à ce jalon (liste fermée de 00 §8 révision 2) ; « **M2**, conditionnel » = utilisé dès v0.1 seulement si la vérification indiquée l'exige ; « candidat » = hors plan, utile plus tard, à ajouter par amendement de 00 §8 (§2.10) ; « hors plan » = sans usage prévu ; « retiré » = sorti de la liste par 00 §8 révision 2 ; « interdit » = à ne jamais appeler (§2.9).
- La référence Python n'utilise que les 7 chemins de v0.1 (référence client.py:187-208).

### 2.2 Search Controller (5)

| Chemin | Corps | Réponse (champs clés) | Rôle | Plan |
|---|---|---|---|---|
| `POST /search` | `SearchRequestDTO {`**`fond`**`, `**`recherche`**`}` (§3) | `SearchResponseDTO {totalResultNumber, totalArticleResultNumber, typePagination, facets[], results[], executionTime, descriptionFusionHtml}` | recherche générique | **M2** (`fr_search`) ; **M6** (recherche exacte par numéro, `find_article` à une autre date que le jour, facette `ECLI` sur JURI et CETAT) ; **M7** (`fr_search_all` ; fond `JORF` si la surface JORF est retenue à l'ouverture de M7, 00 §4) |
| `POST /search/canonicalArticleVersion` | `ArticleRequest {`**`id`**`}` | `{articleVersions[ArticleVersion{id, version, numero, etat, dateDebut, dateFin, ordre}]}` | versions d'un article, métadonnées seules | **M7** (`article_versions`) |
| `POST /search/canonicalVersion` | `{`**`cidText`**`, `**`date`**`, cidSection}` | `{cid, dateDebut, dateFin}` | version d'un texte en vigueur à une date | candidat |
| `POST /search/nearestVersion` | `{`**`cidText`**`, `**`date`**`, cidSection}` | `{title: SearchTitle, sectionTitle}` | version la plus proche d'une date | candidat |
| `GET /search/ping` | — | « pong » | test du contrôleur | hors plan |

### 2.3 Consult Controller (39)

**Textes LEGI, sections, tables des matières (7)**

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /consult/lawDecree` | `{`**`textId`**`, `**`date`**`, searchedString, fromSuggest}` | `ConsultTextResponse` | texte LODA à une date | **M2** (`get_act`) |
| `POST /consult/code` | `{`**`textId`**`, `**`date`**`, sctCid, abrogated, searchedString, fromSuggest}` ; `sctCid` « Non requis pour la consultation de la table des matières sinon obligatoire » | `ConsultTextResponse` | contenu d'une section de code | candidat (lecture d'un gros code par section, D-47) |
| `POST /consult/legiPart` | `{`**`textId`**`, `**`date`**`, searchedString}` | `ConsultTextResponse` | texte LEGI à une date (exemple officiel pour une loi, doc « exemples » l.483-491) | hors plan |
| `POST /consult/legi/tableMatieres` | `{`**`textId`**`, `**`date`**`, nature}` ; `textId` « id OU cid du texte » ; `nature` ∈ CODE, DECRET, ARRETE, LOI, ORDONNANCE… | `ConsultTextResponse` | table des matières d'un code ou d'une LODA | **M2**, conditionnel (`get_act` sur un code, nature `CODE`, si V-96, bloquante en M2, montre que `lawDecree` sur un code dépasse 4 Mio ou 10 s ; 00 §8, 04 §7.3) |
| `POST /consult/code/tableMatieres` | `CodeConsultRequest` | `ConsultTextResponse` | **déprécié** : « utiliser /legi/tableMatieres avec le champ 'nature'='CODE' à la place » | interdit |
| `POST /consult/getSectionByCid` | `{`**`cid`**`}` (LEGISCTA) | `{listSection[Section{id, cid, titre, dateDebut, dateFin, liensSection[], liensArticle[], commentaire, nota, notaHtml, liensModification[], context}]}` | toutes les versions d'une section | candidat (navigation TOC) |
| `POST /consult/getCodeWithAncienId` | `{ancienId}` (ex. `CASSURAL`) | `ConsultTextResponse` | résolution d'un ancien identifiant de code | candidat |

**Articles, versions, liens (9)**

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /consult/getArticle` | `{`**`id`**`}` (LEGIARTI ; KALIARTI validé en direct par la référence, SOURCES.md:14) | `GetArticleResponse {article: Article, executionTime, dereferenced}` | un article (une version) | **M2** (`get_article`) |
| `POST /consult/getArticleByCid` | `{`**`cid`**`}` | `{listArticle[Article]}`, « Liste des versions de l'article » | toutes les versions, texte compris | **M7** (repli de `article_versions`, `fr_diff_article`) |
| `POST /consult/getArticleWithIdAndNum` | `{id, num}` (aucun champ requis selon le Swagger ; les deux sont nécessaires) | `GetArticleResponse` | article en vigueur d'un texte par numéro | **M6** (`find_article`, version en vigueur seulement ; autre date : `/search`, §3.10) |
| `POST /consult/getArticleWithIdEliOrAlias` | `{`**`idEliOrAlias`**`}` (ex. `/eli/decret/2021/7/13/PRMD2117108D/jo/article_1`) | `GetArticleResponse` | ELI → article | **M6** (ELI d'article cité, jamais fabriqué ; 00 §8, V-133) |
| `POST /consult/relatedLinksArticle` | `{`**`articleId`**`}` | `{liensCite[LiensRelatifsDTO], liensCitePar[LiensRelatifsDTO]}` | graphe « cite / cité par » | **M7** (`related_links`) |
| `POST /consult/concordanceLinksArticle` | `{`**`articleId`**`}` | `SectionsRevisionArticleResponse {oldTexts[ArticleDTO], newTexts[ArticleDTO]}` | concordances (renumérotation, recodification) | candidat (citations anciennes) |
| `POST /consult/sameNumArticle` | `{`**`textCid`**`, `**`articleCid`**`, `**`articleNum`**`, `**`date`**`}` | `SectionsRevisionArticleResponse` | articles ayant porté le même numéro | candidat (citations anciennes) |
| `POST /consult/servicePublicLinksArticle` | `{articleCid, fond}` | `{liensSP: map<string,string>}` | liens vers service-public.fr | hors plan |
| `POST /consult/hasServicePublicLinksArticle` | tableau de chaînes (ids) | tableau de chaînes | filtrage en lot des articles ayant des liens SP | hors plan |

**JORF, ELI, NOR (7)**

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /consult/jorf` | `{`**`textCid`**`, searchedString}` (JORFTEXT) | `ConsultJorfResponse` (= `ConsultTextResponse` + `hasLoda`, `jorfFileMetadata[]`, `hasSinglePdf`) | texte JO initial ; `hasLoda` « Indique si le texte a une version consolidée » | candidat M7 : consultation d'un hit `Jorf`, tranchée à l'ouverture de M7 (00 §8, V-135 ; §10 question 1) |
| `POST /consult/jorfPart` | `{`**`textCid`**`}` | `ConsultJorfResponse` | partie (section, article) d'un texte JO | hors plan |
| `POST /consult/jorfCont` | `{id, num, date, start, end, searchText, pageNumber, pageSize, highlightActivated}` ; `pageSize` max 100 | `GetJosResponse {totalNbResult, items[{joCont, joEA, joInap, josPat[]}]}` | sommaire d'un JO (JORFCONT → JORFTEXT) | hors plan |
| `POST /consult/lastNJo` | `{`**`nbElement`**`}` ; « Ce chiffre doit être inférieur à 2500, sinon vous aurez des erreurs » (doc « exemples » l.1187) | `GetJorfContResponse {totalNbResult, containers[]}` | derniers JO | hors plan |
| `POST /consult/getJoWithNor` | `{`**`nor`**`}` | `ConsultJorfResponse` | NOR → texte JO ; « tous les textes n'ont pas de NOR » (l.1209) | **M6** (`find_by_number`, NOR) |
| `POST /consult/eliAndAliasRedirectionTexte` | `{`**`idEliOrAlias`**`}` (ex. `/eli/decret/2018/2/13/JUSC1732516D/jo/texte`) | `ConsultJorfResponse` | ELI → texte JO | **M6** (ELI de texte cité, jamais fabriqué ; 00 §8, V-133) |
| `POST /consult/getTables` | `{`**`endYear`**`, startYear}` | `{tables[FileMetadata]}` | tables annuelles (PDF) | hors plan |

Les JO antérieurs à juin 2004 n'ont pas de version HTML et ne sont pas récupérables par l'API (doc « exemples » l.1223).

**KALI, ACCO, BOCC (7)**

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /consult/kaliText` | `{`**`id`**`, searchedString}` ; `id` « Identifiant du texte ou d'un de ses éléments enfants (section/article) » | `ConsultKaliTextResponse` | texte d'une convention collective | **M2** (`get_convention`) |
| `POST /consult/kaliCont` | `{`**`id`**`, searchedString}` (KALICONT) | `ConsultKaliContResponse {id, titre, num, numeroTexte, nature, texteBaseId[], activitePro[], categorisation[], sections[]}` | conteneur d'une convention (textes de base, avenants, salaires) | hors plan |
| `POST /consult/kaliContIdcc` | `{`**`id`**`}` « Identifiant de la convention collective ou son numéro IDCC » (ex. `1261`) | `ConsultKaliContResponse` | convention par IDCC | **M6** (`find_by_number`, IDCC) |
| `POST /consult/kaliArticle` | `{`**`id`**`}` (KALIARTI) | `ConsultKaliTextResponse` | texte KALI depuis un article | hors plan |
| `POST /consult/kaliSection` | `{`**`id`**`}` (KALISCTA) | `ConsultKaliTextResponse` | texte KALI depuis une section | hors plan |
| `POST /consult/acco` | `{`**`id`**`, searchedString}` | `ConsultAccoResponse {acco: Accord}` | accord d'entreprise | **M2** (`get_company_agreement`) |
| `POST /consult/getBoccTextPdfMetadata` | `{id, forGlobalBocc}` (ex. `boc_20200028_0001_p000.pdf`) | `ConsultBoccResponse` | métadonnées PDF du BOCC | hors plan |

Incohérence documentaire : le document officiel « tris et filtres » donne `KALITEXT000005677408` en exemple de `/consult/kaliCont` et `KALICONT000005635384` pour `/consult/kaliContIdcc`, à l'inverse du Swagger (recherche 05 §1.6). Le Swagger fait foi.

**Jurisprudence, CNIL, circulaires, parlement (8)**

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /consult/juri` | `{`**`textId`**`, searchedString}` (JURITEXT, CONSTEXT, CETATEXT, validés en direct, référence DISCOVERY.md:78-82) | `ConsultJuriTextResponse {text: TexteSimple, executionTime, dereferenced}` | décision | **M2** (`get_decision`) |
| `POST /consult/getJuriWithAncienId` | `{ancienId}` (ex. `JG_L_2006_09_000000269553`) | `ConsultJuriTextResponse` | ancien identifiant de décision | candidat |
| `POST /consult/getJuriPlanClassement` | `{id, libelle, niveau, page, fond, searchByNiveau, searchSuggest}` | `{listPlanClassementJuri[Nomenclature{id, parent, niveau, libelleNiveau, libelleArbo, arbo, feuille, idJuritext, titreJuritext}]}` | plan de classement (titrages JURINOME, CETANOME) | hors plan |
| `POST /consult/cnil` | `{`**`textId`**`}` | `ConsultCnilTextResponse` (même `TexteSimple`) | délibération CNIL | **M2** (`get_deliberation`) |
| `POST /consult/getCnilWithAncienId` | `{ancienId}` (ex. `MCN97020008A`) | `ConsultCnilTextResponse` | ancien identifiant CNIL | candidat |
| `POST /consult/circulaire` | `{`**`id`**`}` (ex. `44128`) | `ConsultCirculaireResponse` | circulaire (fonds CIRC) | hors plan |
| `POST /consult/dossierLegislatif` | `{`**`id`**`}` (JORFDOLE) | `{dossierLegislatif{titre, type, legislature, exposeMotif, echeancier, dossiers[], …}}` | travaux préparatoires, échéancier | hors plan |
| `POST /consult/debat` | `{`**`id`**`}` (ex. `AN_2020-090.pdf`) | `ConsultDebatResponse` | débat parlementaire (PDF) | hors plan |

**Test du contrôleur (1)** : `GET /consult/ping` → « pong » ; hors plan.

### 2.4 List Controller (13)

Toutes les listes paginées exigent `pageNumber` et `pageSize` (« max 100 ») (VÉRIFIÉ, Swagger).

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /list/code` | `{`**`pageNumber`**`, `**`pageSize`**`, sort ∈ {TITLE_ASC}, states[] (17 états), codeName}` | `CodeListResponse {totalResultNumber, results[CodeListResult{id, cid, titre, etat, dateDebut, dateFin, lastUpdate, pdfFile*}], codeNames: Facet, states: Facet}` | liste des codes | **M6** (`list_codes`, codes abrogés compris, et table des codes interne, une seule table TTL 7 j, chargée paresseusement au premier besoin, jamais au démarrage ; 00 §3) ; **M7** (complétions de la variable `code`) |
| `POST /list/loda` | `{`**`pageNumber`**`, `**`pageSize`**`, sort/secondSort ∈ {PUBLICATION_DATE_*, SIGNATURE_DATE_*, ID_*}, natures[], legalStatus[], signatureDate, publicationDate}` | `{results[{id, cid, titre, etat, dateDebut, dateFin, lastUpdate, dossiersLegislatifs[]}], natures, legalStatus}` | listes chronologiques sans plein texte | hors plan |
| `POST /list/conventions` | `{`**`pageNumber`**`, `**`pageSize`**`, idcc, titre, keyWords[], legalStatus[], textesBase[], sort, secondSort}` | `{results[{id, cid, titre, etat, idcc, cidConteneur, descriptionFusion}], legalStatus, typeTexte, moteCles}` | conventions par IDCC ou mots-clés | candidat (repli IDCC) |
| `POST /list/dossiersLegislatifs` | `{`**`type`**`, `**`legislatureId`**`}` | `DossiersLegislatifsListResponse` | dossiers législatifs par législature | hors plan |
| `POST /list/legislatures` | pas de corps | `{legislatures[{id, libelle, dateDebut, dateFin}]}` | référentiel | hors plan |
| `POST /list/bocc` | `{`**`pageNumber`**`, `**`pageSize`**`, idGlobalBocc, sortValue, intervalPublication, searchForSingleGlobalBocc}` | `BoccGlobalListResponse` | bulletins des conventions collectives | hors plan |
| `POST /list/boccTexts` | `{`**`pageNumber`**`, `**`pageSize`**`, idccs[], idMainBocc, departments[], titre, intervalPublication, sortValue, …}` | `BoccTextsListResponse` | textes du BOCC | hors plan |
| `POST /list/boccsAndTexts` | `{`**`pageNumber`**`, `**`pageSize`**`, idcc, titre, intervalPublication, sortValue}` | `BoccGlobalListResponse` | idem | hors plan |
| `POST /list/bodmr` | `{`**`pageNumber`**`, `**`pageSize`**`, sort, years[]}` | `BodmrListResponse` | décorations et médailles | hors plan |
| `POST /list/debatsParlementaires` | `{`**`pageNumber`**`, `**`pageSize`**`, typesPublication[], dateParution, sortValue, secondSortValue}` | `DebatsParlementairesListResponse` | débats | hors plan |
| `POST /list/questionsEcritesParlementaires` | `{`**`pageNumber`**`, `**`pageSize`**`, parlementTypes[], periodePublication, sortValue, secondSortValue}` | `QuestionsEcritesParlementairesListResponse` | questions écrites | hors plan |
| `POST /list/docsAdmins` | `{years[]}` | `DocsAdminsListResponse` | documents administratifs | hors plan |
| `GET /list/ping` | — | « pong » | test (exemple officiel de l'appel Bearer) | hors plan |

Un client tiers envoie `"sort": "CODE_DATE_MAJ"` à `/list/code`, valeur absente de l'enum `TITLE_ASC` : **NON VÉRIFIÉ** [V-125 · list-code-sort]. Le plan envoie `TITLE_ASC` (04 §7.1).

### 2.5 Suggest Controller (4)

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /suggest` | `{searchText, supplies[], documentsDits}` (aucun champ requis) ; `supplies` ∈ {ALL, ALL_SUGGEST, LODA_LIST, CODE_LIST, CODE_RELEASE_DATE, CODE_RELEASE_DATE_SUGGEST, CODE_LEGAL_STATUS, LODA_RELEASE_DATE, LODA_RELEASE_DATE_SUGGEST, LODA_LEGAL_STATUS, KALI, KALI_TEXT, CONSTIT, CETAT, JUFI, JURI, JORF, JORF_SUGGEST, CNIL, ARTICLE, CIRC, ACCO, PDF} | `SuggestResponse {totalResultNumber, results: map<id, object>}` ; la définition `SuggestValue {id, idTexte, section, label, nature, origin, dateVersion, appellations[]}` existe mais la map est typée `object` | suggestions (appellations usuelles : « Loi Macron ») | retiré (00 §8 révision 2, AM-29 : aucun consommateur ni méthode de port) |
| `POST /suggest/acco` | `{searchText}` | `{results: map<string, {siret, raisonSociale}>}` | SIRET et raison sociale | hors plan |
| `POST /suggest/pdc` | `{searchText, origin, fond}` | `SuggestResponse` | libellés du plan de classement | hors plan |
| `GET /suggest/ping` | — | « pong » | test | hors plan |

`/suggest` est un **POST** JSON. Un client tiers documente l'échec d'un `GET` (recherche 05 §1.10).

### 2.6 Chrono Controller (4)

| Chemin | Corps | Réponse | Rôle | Plan |
|---|---|---|---|---|
| `POST /chrono/textCid` | `{`**`textCid`**`, `**`dateConsult`**`, `**`startYear`**`, `**`endYear`**`}` | `ChronolegiResponse {datePublication, regroupements[{title, detailLoaded, versions: map<date, VersionDTO>}]}` | historique des versions d'un texte avec les articles modificateurs | candidat (motif d'un changement dans `fr_diff_article`) |
| `POST /chrono/textCidAndElementCid` | `{`**`textCid`**`, `**`elementCid`**`}` | `ChronolegiResponse` | historique restreint à un article ou une section | candidat (idem) |
| `GET /chrono/textCid/{textCid}` | paramètre de chemin `textCid` | `{chronolegiExists}` | le texte a-t-il des versions ? | hors plan |
| `GET /chrono/ping` | — | « pong » | test | hors plan |

Modèle chrono (Swagger `definitions`) : `VersionDTO {dateDebut, isEndVersion, articlesModificateurs: map<AAAA-MM-JJ, ArticleModificateurDTO>}` ; `ArticleModificateurDTO {id, title (« LOI n°2018-287 du 20 avril 2018 - art. 6 »), nature, dateDebutCible, actions: map<type, ActionChronoDTO>}` ; `action` ∈ {CREATION, CODIFICATION, MODIFICATION, TRANSFERT, ABROGATION, ANNULATION, PEREMPTION, DISJONCTION, RECTIFICATION, SUBSTITUTION, DEPLACE, VERSEMENT, DENONCIATION, REMPLACEMENT, EXTENSION, ELARGISSEMENT} (recherche 05 §1.8).

### 2.7 Misc Controller (3)

| Chemin | Réponse | Rôle | Plan |
|---|---|---|---|
| `GET /misc/commitId` | `map<string,string>` | version de déploiement de l'API amont | hors plan (diagnostic possible, jamais au démarrage : 00 §6) |
| `GET /misc/datesWithoutJo` | `{lstDateDisabled[]}` (epoch ms) | dates sans JO | hors plan |
| `GET /misc/yearsWithoutTable` | `{lstYearDisabled[]}` | années sans table | hors plan |

### 2.8 Détail des opérations utilisées par le plan

Les corps exacts envoyés et les DTO sont fixés dans 04 §7.1 et §8.3 ; les formes observées sont au §4 ; les DTO au §5. Résumé par endpoint :

| Endpoint | Jalon, méthode du port | Corps envoyé | Champs utiles de la réponse | Points d'attention |
|---|---|---|---|---|
| `/search` | M2 `search` ; M6 `find_article` (autre date que le jour), `find_by_number` (numéros, facette `ECLI` sur JURI et CETAT) ; M7 `search` (`Fond::Jorf`, si la surface JORF est retenue) | §3.10 ; 04 §6.1, §7.2 | `totalResultNumber`, `totalArticleResultNumber`, `typePagination`, `results[]` (§4.2) | `DATE_VERSION` requis pour LODA_DATE et CODE_DATE ; `typePagination` à renvoyer |
| `/consult/lawDecree` | M2 `get_act` | `{"textId": "LEGITEXT…", "date": "AAAA-MM-JJ"}` (référence client.py:190-193, validé en direct) | `id` suffixé, `cid` (JORFTEXT), `title`, `nor`, `nature`, `jurisState`, `dateDebutVersion`, `dateFinVersion`, `dateParution`, `jorfText`, `numParution`, `textAbroge`, `appellations`, `sections[]`, `articles[]` (§4.3) | `etat` nul mais `jurisState` rempli ; `content`, `intOrdre`, `visa` absents de la fixture élaguée |
| `/consult/getArticle` | M2 `get_article` | `{"id": "LEGIARTI…" \| "KALIARTI…"}` | `article.{id, num, etat, texte, texteHtml, nota, dateDebut, dateFin, context.titresTM, textTitles, versionPrecedente, isMultipleVersions}` (§4.4) | `texteHtml`, `articleVersions`, liens absents de la fixture |
| `/consult/juri` | M2 `get_decision` | `{"textId": "JURITEXT…" \| "CONSTEXT…" \| "CETATEXT…"}` | `text.{ecli, juridiction, formation, solution, numeroAffaire, num, titre, dateTexteComputed, texte, texteHtml, sommaire, citationJp, liens, titrages}` (§4.5) | identifiant inexistant : 400 avec marqueur (sondé en direct pour `JURITEXT` seulement, §1.5) |
| `/consult/cnil` | M2 `get_deliberation` | `{"textId": "CNILTEXT…"}` | `text.{num, nature, natureDelib, etat, titre, titreLong, dateTexte, datePubli, texte, texteHtml}` | pas d'ECLI ni d'ELI |
| `/consult/kaliText` | M2 `get_convention` | `{"id": "KALITEXT…"}` | `title`, `nor`, `jurisState`, `conteneurs[0].{cid, num (IDCC), titre}`, `numeroBo`, `originePubli`, `typeTexte`, `sections[].articles[].{content, lstLienCitation, lstLienModification}` (§4.6) | contenu HTML des articles inclus (pas d'appel N+1) |
| `/consult/acco` | M2 `get_company_agreement` | `{"id": "ACCOTEXT…"}` | `acco.{titreTexte, raisonSociale, siret, codeIdcc, codeApe, secteur, themes, syndicats, numero, dates}` (§4.7) | `data` (base64) jamais déclaré |
| `/consult/getArticleWithIdAndNum` | M6 `find_article` (date du jour ; autre date : `/search` `CODE_DATE` en `EXACTE` sur `NUM_ARTICLE` + `DATE_VERSION`, hits filtrés par `cid`, jamais `NOM_CODE` sans V-121, 00 §8) | `{"id": "LEGITEXT…", "num": "L1242-2"}` | `GetArticleResponse` | « Récupère uniquement les versions d'un article qui est en vigueur au moment de la recherche » ; « seuls les articles ne comportant pas de version en vigueur différée peuvent être trouvés par cette méthode » (Swagger) ; forme de `num` : Swagger `5-8`, document officiel `L5-8` : **NON VÉRIFIÉ** [V-119 · num-article-forme-api] ; comportement **NON VÉRIFIÉ** [V-118 · idandnum-comportement] |
| `/consult/getJoWithNor` | M6 `find_by_number` (NOR) | `{"nor": "ECFI1524250L"}` | `id`, `cid`, `title`, `nor`, `nature`, `dateTexte`, `dateParution`, `hasLoda`, `eli` | `hasLoda` permet de basculer vers la version consolidée |
| `/consult/kaliContIdcc` | M6 `find_by_number` (IDCC) | `{"id": "2121"}` | `id` (KALICONT), `titre`, `num`, `numeroTexte` (« IDCC 1261 »), `nature` (« IDCC »), `texteBaseId[]`, `sections[]` | — |
| `/consult/getArticleWithIdEliOrAlias` | M6 `find_by_number` (ELI d'article cité, 04 §7.2) | `{"idEliOrAlias": "/eli/decret/2021/7/13/PRMD2117108D/jo/article_1"}` (ELI reçu de l'utilisateur, transmis tel quel) | `GetArticleResponse` (`article.id` validé par le domaine) | jamais d'ELI fabriqué (00 §4) ; comportement **NON VÉRIFIÉ** [V-133 · eli-redirection-endpoints] |
| `/consult/eliAndAliasRedirectionTexte` | M6 `find_by_number` (ELI de texte cité, 04 §7.2) | `{"idEliOrAlias": "/eli/decret/2018/2/13/JUSC1732516D/jo/texte"}` | `ConsultJorfResponse` : `id`, `cid`, `title`, `nor`, `hasLoda`, `eli` | idem [V-133 · eli-redirection-endpoints] |
| `/list/code` | M6 `list_codes` et table des codes interne (une seule table, TTL 7 j, chargée paresseusement, sert aussi `find_article` et, en M7, les complétions) | `{"pageNumber": 1, "pageSize": 100, "sort": "TITLE_ASC"}` (sans `states` : codes abrogés compris, avec leur `etat`) | `totalResultNumber`, `results[].{id, cid, titre, etat, dateDebut, dateFin, lastUpdate}` (dates ISO `+0000` d'après les exemples du Swagger) | 76 codes indexés en sandbox (facette `FOND`) ; formes réelles M6 **NON VÉRIFIÉ** [V-122 · endpoints-m6-reponses] |
| `/consult/getArticleByCid` | M7 `article_versions` (repli) | `{"cid": "LEGIARTI…"}` | `listArticle[Article]` | réponse potentiellement lourde (texte de chaque version) |
| `/search/canonicalArticleVersion` | M7 `article_versions` | `{"id": "LEGIARTI…"}` | `articleVersions[].{id, version, numero, etat, dateDebut, dateFin, ordre}` | — |
| `/consult/relatedLinksArticle` | M7 `related_links` | `{"articleId": "LEGIARTI…"}` | `liensCite[]`, `liensCitePar[]` de `{id, cidText, cidParent, nature ∈ {LODA, JORF, CODE, KALI, JORFCONT}, name, dateVigeur}` ; `dateVigeur` exemple `"1470787200000"` (chaîne) | KALIARTI accepté ? **NON VÉRIFIÉ** [V-136 · related-links-forme] |
| `/consult/legi/tableMatieres` | M2, conditionnel : `get_act` sur un code si V-96 (bloquante en M2) montre que `lawDecree` sur un code dépasse 4 Mio ou 10 s (00 §8) | `{"textId": "LEGITEXT…", "date": "AAAA-MM-JJ", "nature": "CODE"}` | `ConsultTextResponse` ; l'absence du contenu des articles est supposée : **NON VÉRIFIÉ** [V-135 · endpoints-m7-reponses] | conditionné à [V-96 · lawdecree-sur-code] (04 §7.3) ; reconnaître un code avant l'appel : 04 §16 question 13 |
| `/consult/jorf` (candidat M7) | M7, tranché à l'ouverture (consultation d'un hit `Jorf`, 00 §8, V-135) | `{"textCid": "JORFTEXT…"}` | `ConsultJorfResponse` : `title`, `nor`, `hasLoda`, `eli`, sections et articles | bascule vers la version consolidée si `hasLoda = true` (§10 question 1) ; formes réelles **NON VÉRIFIÉ** [V-135 · endpoints-m7-reponses] |

`/suggest` est **retiré** de la liste (00 §8 révision 2, AM-29 : aucun outil ni méthode de port ne le consomme ; pas de méthode `suggest` au port, 00 §3) ; son corps et sa réponse restent décrits au §2.5 pour mémoire.

`ConsultArticle` (articles des TOC) déclare `comporteLiens` (« Utiliser l'API relatedLinksArticle pour récupérer la liste des liens ») et `lstLienCitation` (« Toujours vide (voir propriété comporteLiens) »). Or la fixture KALI contient un `lstLienCitation` rempli (§4.6) : le DTO le lit quand il est présent.

### 2.9 Chemins à ne pas utiliser

| Chemin ou usage | Raison | Source |
|---|---|---|
| `consult/loda/version`, `consult/loda/versions`, `consult/juri/version`, `consult/juri/versions`, `consult/juri/ancienId` | appelés par pylegifrance, **absents** du Swagger 2.4.2 (prod et sandbox) ; existence réelle **NON VÉRIFIÉ** [V-74 · swagger-version-servie] | recherche 05 §1.12 (`[PLF]/pylegifrance/fonds/loda.py:935,966`, `fonds/juri.py:609,643,669`) ; remplacés par `/consult/getArticleByCid`, `/search/canonicalArticleVersion`, `/chrono/*`, `/consult/getJuriWithAncienId` |
| `/consult/code/tableMatieres` | déprécié par le Swagger lui-même | Swagger, `summary` du chemin |
| `GET /suggest` | méthode inexistante (le chemin est un `POST`) | recherche 05 §1.10 |
| toute URL renvoyée dans les données : `lienJo`, `urlCC`, `attachementUrl`, `href` des contenus KALI (`/affichCodeArticle.do?…`) | jamais appelée : la base d'URL ne change pas après le démarrage | 04 §2.3, §14.3 |
| tout chemin hors de la liste fermée de 00 §8 | décision normative | 00 §8 ; test de dérive `crates/lf-adapter-piste/tests/data/swagger-2.4.2-paths.txt` (04 §7.1) |

### 2.10 Opérations candidates (hors plan)

Utiles plus tard, chacune exige un amendement de 00 §8 (liste fermée) et, si elle change le port, de 00 §3. Elles sont listées ici pour éviter de les redécouvrir.

La résolution d'un ELI cité par l'utilisateur, candidate dans la version initiale de cette annexe (l'API ne fournit pas d'ELI côté LODA, référence DISCOVERY.md:34-40, mais l'accepte en entrée), est adoptée en M6 par 00 §8 révision 2 (AM-29) : `/consult/getArticleWithIdEliOrAlias` et `/consult/eliAndAliasRedirectionTexte` (§2.8), sans changer la règle « jamais d'ELI fabriqué » (00 §4) ; comportement **NON VÉRIFIÉ** [V-133 · eli-redirection-endpoints] (V-133).

| Besoin | Opérations | Jalon envisageable | Condition |
|---|---|---|---|
| Consulter un hit JORF (`Fond::Jorf`, M7) | `/consult/jorf` | M7 | tranché à l'ouverture de M7 avec la surface JORF (00 §4, §8 ; V-135) ; §10 question 1 |
| Anciens identifiants (`ancienId` CETAT `JG_L_…`, CNIL `MCN…`, codes `CASSURAL`) | `/consult/getJuriWithAncienId`, `/consult/getCnilWithAncienId`, `/consult/getCodeWithAncienId` | M6 | besoin attesté dans le corpus de citations (annexe B) |
| Lire un gros code par section (D-47) | `/consult/code` (`sctCid`), `/consult/getSectionByCid` | M7 | taille réelle des TOC [V-75 · taille-max-textes] |
| Expliquer un changement de version | `/chrono/textCidAndElementCid` | M7 (`fr_diff_article`) | forme réelle |
| Citations à numérotation ancienne | `/consult/sameNumArticle`, `/consult/concordanceLinksArticle` | M6 (`fr_verify_citations`) | — |
| Version d'un texte à une date | `/search/canonicalVersion`, `/search/nearestVersion` | M6 | — |
| Veille JO, listes chronologiques | `/consult/lastNJo`, `/consult/jorfCont`, `/list/loda`, `/misc/datesWithoutJo` | après v1.0 | nouvel outil ou ressource |
| Conventions par IDCC ou mots-clés | `/list/conventions` | M6 (repli) | — |
| Diagnostic de l'API amont | `/*/ping`, `/misc/commitId` | après v1.0 | `check-config --online` n'appelle que l'endpoint token, une seule fois, sans appel à l'API (04 §2.4, §16 question 9) ; jamais au démarrage (00 §6) |

---

## 3. Recherche (`POST /search`)

### 3.1 Structure de la requête

Swagger 2.4.2 (`definitions.SearchRequestDTO` et suivantes), VÉRIFIÉ ; `*` = requis :

```text
SearchRequestDTO        { fond*: enum(14), recherche*: RechercheSpecifiqueDTO }
RechercheSpecifiqueDTO  { champs*: [ChampDTO], filtres: [FiltreDTO]  (ET implicite entre filtres),
                          operateur*: ET|OU  (entre champs), sort*: string (dépend du fonds), secondSort: string,
                          pageNumber*: int32, pageSize*: int32 (max=100), typePagination*: DEFAUT|ARTICLE,
                          fromAdvancedRecherche: bool }
ChampDTO                { typeChamp: enum(32), criteres: [CritereDTO], operateur: ET|OU (entre critères) }
CritereDTO              { valeur*: string, typeRecherche*: enum(5), operateur*: ET|OU (entre sous-critères),
                          proximite: int32 (distance maximale en mots), criteres: [CritereDTO] (récursif) }
FiltreDTO               { facette: string, valeurs: [string], dates: DatePeriod{start, end},
                          singleDate: date-time, multiValeurs: map<string, [string]> }
```

`ChampDTO` et `FiltreDTO` n'ont aucun champ requis. Le plan envoie toujours un seul champ `ALL` avec un seul critère (00 §8 ; 04 §6.1).

### 3.2 Fonds (14)

`fond` : « Pour rechercher dans tous les fonds, il faut définir la valeur ALL. Pour les fonds LODA et CODE il existe deux types de recherche : la recherche par date (_DATE) de version ou la recherche par état juridique (_ETAT) » (Swagger). Volumes : facette `FOND` des fixtures (sandbox, juin et juillet 2026 ; la production peut différer).

| `fond` | Contenu | Volume (facette `FOND`) | `Fond` du domaine (00 §4) | Plan |
|---|---|---|---|---|
| `LODA_DATE` | lois, ordonnances, décrets, arrêtés consolidés, recherche à une date de version (`DATE_VERSION` requis) | `LEGI` ≈ 117 230 (valeur de la facette ; sa répartition entre LODA et CODE n'est pas documentée) | `Loda` | M2 |
| `LODA_ETAT` | idem, recherche par état juridique | — | — | hors plan |
| `CODE_DATE` | codes, à une date de version (`DATE_VERSION` requis) | `CODE` = 76 | `Code` | M2 |
| `CODE_ETAT` | codes, par état juridique | — | — | hors plan |
| `JORF` | textes publiés au Journal officiel | ≈ 1 125 515 | `Jorf` | M7, si la surface JORF est retenue à l'ouverture de M7 (00 §4) |
| `JURI` | jurisprudence judiciaire (Cour de cassation, appel, premier degré) | ≈ 605 651 | `Juri` | M2 |
| `CETAT` | jurisprudence administrative (CE, CAA, TA, Tribunal des conflits…) | ≈ 569 322 | `Cetat` | M2 |
| `CONSTIT` | Conseil constitutionnel | ≈ 7 372 | `Constit` | M2 |
| `JUFI` | juridictions financières (déduit du nom ; contenu **NON VÉRIFIÉ** [V-184 · contenu-fonds-hors-plan]) | ≈ 3 502 | — | hors plan |
| `CNIL` | délibérations et décisions de la CNIL | ≈ 26 759 | `Cnil` | M2 |
| `KALI` | conventions collectives | ≈ 21 911 | `Kali` | M2 |
| `ACCO` | accords d'entreprise | ≈ 387 844 | `Acco` | M2 |
| `CIRC` | circulaires et instructions | ≈ 10 646 | — | hors plan |
| `ALL` | tous les fonds ; champs `ALL` et `TITLE`, filtre `FOND` | — | — | non utilisé : `fr_search_all` lance une requête par fond (00 §7) |

Sources des volumes : recherche 05 §2.4, recherche 03 §5. Ces lacunes de couverture (LODA_ETAT, CODE_ETAT, JUFI, CIRC, débats, dossiers législatifs) alimentent les « lacunes calculées » de `fr_coverage` (00 §7 ; 05).

### 3.3 `typeChamp` (32 valeurs)

Enum du Swagger (`ChampDTO.typeChamp`), VÉRIFIÉ. Fonds qui acceptent chaque champ d'après le document officiel « description des tris et filtres » (recherche 05 §2.2, lignes 26-307 de la conversion Markdown) :

| `typeChamp` | Fonds (document officiel) | Usage du plan |
|---|---|---|
| `ALL` | tous | **M2** : seul champ envoyé par `fr_search` (00 §8) |
| `TITLE` | tous | — |
| `TABLE` | CODE_* | — |
| `NOR` | JORF, LODA_*, CIRC, CNIL | — (le NOR passe par `/consult/getJoWithNor` en M6) |
| `NUM` | JORF, LODA_* | **M6** (numéro de texte, `EXACTE`) |
| `NUM_ARTICLE` | JORF, CODE_*, LODA_* | **M6** (numéro d'article, `EXACTE`, `typePagination=ARTICLE` ; `find_article` à une autre date que le jour, `find_by_number`) |
| `ARTICLE` | JORF, CODE_*, LODA_*, KALI | — |
| `VISA`, `NOTICE`, `VISA_NOTICE`, `TRAVAUX_PREP`, `SIGNATURE`, `NOTA` | JORF, LODA_* | — |
| `NUM_DEC` | CONSTIT, CETAT, JUFI | **M6** (numéro de décision ou de requête) |
| `NUM_AFFAIRE` | JURI | **M6** (numéro de pourvoi) |
| `NUM_DELIB` | CNIL | **M6** |
| `IDCC` | KALI, ACCO | **M6** |
| `ABSTRATS` | CETAT, JURI, JUFI | — |
| `RESUMES` | CETAT, JURI | — |
| `TEXTE` | CONSTIT, CETAT, JURI, JUFI, CNIL | — |
| `RESUME_CIRC`, `TEXTE_REF` | CIRC | — |
| `RAISON_SOCIALE` | ACCO | — |
| `MOTS_CLES` | KALI | — |
| `ADVANCED_TEXTE_ID`, `MINISTERE`, `ECLI`, `NUM_LOI_DEF`, `TYPE_DECISION`, `NUMERO_INTERNE`, `REF_PUBLI`, `TITRE_LOI_DEF` | **aucun** fonds dans le document officiel (8 valeurs) | — ; pylegifrance cherche pourtant par `typeChamp=ECLI` en `EXACTE` (recherche 03 §3.2), et `ECLI` est une **facette** documentée pour JURI et CETAT (§3.7). 00 §8 révision 2 (AM-29) adopte en M6 la **facette** `ECLI` (JURI, CETAT), pas le `typeChamp` : effet **NON VÉRIFIÉ** [V-128 · ecli-filtre-juri-cetat] (V-128) ; `typeChamp = ECLI` reste un repli à sonder (V-127) ; §10 question 2 |

### 3.4 `typeRecherche` (5 valeurs)

| `typeRecherche` | Sens | Plan |
|---|---|---|
| `UN_DES_MOTS` | OU logique entre les mots ; avec `proximite`, recherche d'expression approchée | `match=any`, valeur par défaut (parité) adoptée par défaut (Q10) jusqu'à V-79 en M2 ; passage à `all` si V-79 le confirme, décision consignée avant le verrou des descripteurs de M4 (00 §7) |
| `TOUS_LES_MOTS_DANS_UN_CHAMP` | tous les mots dans un même champ ; sémantique exacte avec `typeChamp=ALL` **NON VÉRIFIÉ** [V-79 · mode-all-semantique] (V-79, qui conditionne Q10) | `match=all` |
| `EXACTE` | expression exacte (doc « exemples » l.837-843) | `match=exact` ; recherches par numéro (M6) |
| `AUCUN_DES_MOTS` | exclusion | non exposé |
| `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION` | exclusion d'expression | non exposé |

Correspondance `match` → `typeRecherche` : 00 §8 ; 04 §6.4.

### 3.5 Opérateurs et critères

- Trois niveaux d'`operateur` (`ET`, `OU`) : `recherche.operateur` entre champs, `champs[].operateur` entre critères, `criteres[].operateur` entre sous-critères. Les filtres sont toujours combinés en ET. Le plan envoie `ET` partout (parité, référence server.py:133-156).
- Critères imbriqués et `proximite` : exemple officiel « mineur » à 2 mots de « travail » (doc « exemples » l.903-967) et exemple du Swagger (`RechercheSpecifiqueDTO.champs.example`). Non utilisés par le plan.
- `valeur: "*"` = « tout » dans un exemple officiel JURI (l.1063). La syntaxe interne du moteur (guillemets, jokers) est vérifiée en sandbox le 2026-10-28 pour les formes essayées (guillemets : expression exacte ; joker de fin actif ; `ET` et `OU` ne sont pas des opérateurs ; `AND` et `-mot` non essayés) [V-80 · syntaxe-moteur-valeur]. La longueur maximale de `valeur` est mesurée en sandbox le 2026-10-28 : 450 caractères acceptés, 499 refusés par un 500, soit une limite en caractères entre 451 et 498 et non 500 (C3) [V-81 · longueur-max-query].
- `fromAdvancedRecherche` est un booléen ; un exemple officiel KALI envoie la chaîne `"False"` (l.1263, l.1315). Le plan ne l'envoie pas.

### 3.6 Filtres

| Forme | Champs | Exemple | Usage du plan |
|---|---|---|---|
| textuelle | `facette` + `valeurs[]` | `{"facette": "NATURE", "valeurs": ["LOI", "ORDONNANCE", "ARRETE"]}` (Swagger) | non utilisé en v0.1 ; M6 : facette `ECLI` à valeur exacte (JURI, CETAT ; 00 §8, §3.10) |
| plage de dates | `facette` + `dates {start, end}` | `{"facette": "DATE_SIGNATURE", "dates": {"start": "2015-01-01", "end": "2018-01-31"}}` (Swagger) | `date_from` / `date_to` (00 §7 ; 04 §6.5) |
| date unique | `facette` + `singleDate` | `{"facette": "DATE_VERSION", "singleDate": 1514802418000}` (exemples officiels, pylegifrance) | `version_date` pour LODA_DATE et CODE_DATE, midi Europe/Paris du jour, en epoch ms (00 §8, AM-02) |
| hiérarchique | `facette` + `valeurs[]` + `multiValeurs` | `valeurs: ["TRIBUNAL_ADMINISTATIF", "COURS_APPEL"]`, `multiValeurs: {"TRIBUNAL_ADMINISTATIF": ["Bordeaux", "Lille"]}` (Swagger) | non utilisé |

- `LODA_DATE` et `CODE_DATE` **exigent** un filtre `DATE_VERSION` en `singleDate` (constat live, référence DISCOVERY.md:22-25). VÉRIFIÉ.
- `singleDate` est déclaré `date-time` avec l'exemple `"2016-01-01"`. Les exemples officiels utilisent un entier epoch ms (`1514802418000`) et aussi `"singleDate": "2025-04-15"` (l.221, l.815). La référence envoie un entier ms, l'instant de l'appel (`now_ms`), validé en direct à des heures de journée seulement (référence server.py:130-136). La forme chaîne est acceptée en sandbox le 2026-10-28 [V-77 · formats-dates-entree] ; le fuseau de l'entier au-delà de V-76 et les formats invalides ne sont pas essayés.
- Règle du plan (00 §8 révision 2, AM-02) : `singleDate` = midi Europe/Paris du jour `version_date`, en epoch ms, stable sur la journée (la clé de cache repose sur `VersionDate`, pas sur l'instant). Elle est correcte que PISTE compare l'instant à des bornes stockées à minuit UTC (§4.9) ou le tronque en date UTC ou Paris ; minuit Paris (règle de la version initiale de 00) n'était correct que dans le dernier cas. Vecteurs (00 §8) : 2026-10-15 → `1792058400000` (10:00Z) ; 2026-01-15 → `1768474800000` (11:00Z) ; 2026-03-29 → `1774778400000` ; 2026-10-25 → `1792926000000`. L'effet réel est vérifié en sandbox le 2026-10-28 : midi Paris retient la version qui commence le jour D [V-76 · singledate-midi-paris] (04 §6.3, R-17), production non essayée.
- `dates {start, end}` : `AAAA-MM-JJ` dans les exemples officiels et le Swagger ; pylegifrance envoie un ISO naïf `"2020-03-04T00:00:00"`. L'effet réel (total réduit, bornes incluses) est vérifié en sandbox le 2026-10-28, sauf pour une borne lointaine, non essayée [V-78 · filtres-dates-effet].
- Faute d'orthographe officielle à reproduire telle quelle : **`TRIBUNAL_ADMINISTATIF`** (sans R), dans le Swagger et les facettes CETAT (fixture search_cetat.json).
- Contradictions documentaires : des exemples officiels écrivent `"facette": "TEXT_LEGAL_STATUS", "valeur": "VIGUEUR"` au singulier (l.461, l.561, l.667) alors que `FiltreDTO` ne définit que `valeurs`. Un exemple filtre `CASSATION_NATURE_DECISION` avec `ARRET` alors que la facette renvoie `arret` (recherche 05 §2.3). Toujours envoyer `valeurs: [..]` avec la valeur exacte renvoyée par la facette. Le comportement du singulier et la sensibilité à la casse sont **NON VÉRIFIÉ** [V-86 · facette-casse-valeurs]. L'acceptation en filtre des valeurs de facettes fermées (`NATURE`, `CASSATION_FORMATION`, `JURIDICTION_NATURE`) est **NON VÉRIFIÉ** [V-129 · facettes-fermees-filtre] ; le plan n'en envoie aucune en v0.1 et v0.2 (annexe [B](B-citations.md)).

### 3.7 Facettes et tris par fond

Document officiel « description des tris et filtres » (recherche 05 §2.2 ; feuille CODE relue dans le xlsx) :

| Fonds | Filtres (`facette`) | Tris (`sort`) |
|---|---|---|
| `ALL` | `FOND` (champ `origine`) | non listé |
| `JORF` | NATURE, DATE_SIGNATURE, DATE_PUBLICATION, MINISTERE, EMETTEUR, NOR, NUM_TEXTE, NUM_ARTICLE, DECORATION, DELEGATION | PERTINENCE, SIGNATURE_DATE_DESC/ASC, PUBLICATION_DATE_DESC/ASC |
| `CODE_DATE` | DATE_VERSION, NOM_CODE, NUM_ARTICLE | **aucun tri listé** |
| `CODE_ETAT` | ARTICLE_LEGAL_STATUS, TEXT_LEGAL_STATUS, TEXT_NOM_CODE, NUM_ARTICLE | **aucun tri listé** |
| `LODA_DATE` | NATURE, DATE_SIGNATURE, DATE_PUBLICATION, NOR, NUM_TEXTE, NUM_ARTICLE, DATE_VERSION | PERTINENCE, PUBLICATION_DATE_DESC/ASC, SIGNATURE_DATE_DESC/ASC |
| `LODA_ETAT` | TEXT_NATURE, TEXT_DATE_SIGNATURE, TEXT_DATE_PUBLICATION, ARTICLE_LEGAL_STATUS, TEXT_LEGAL_STATUS, NOR, NUM_TEXTE, NUM_ARTICLE | idem LODA_DATE |
| `CIRC` | DATE_MEL, DATE_SIGNATURE, DATE_MEA, OPPOSABILITE, DOMAINE, MIN_DEPOSANT, MIN_CONCERNE, MOTS_CLEFS, NUMERO_INTERNE, NOR, REF_PUBLI | PERTINENCE, SIGNATURE_DATE_DESC/ASC, PUBLI_DATE_DESC/ASC |
| `CONSTIT` | NATURE_CONSTIT, NATURE_NORME_AUTRE, SOLUTION_CONSTIT, TITRE_DEFEREE, NUM_LOI, DATE_LOI, TYPE_DECISION, SOLUTION_ELECT, NATURE_AUTRE, SOLUTION_AUTRE, DATE_DECISION, NUMERO_DECISION, NOR | PERTINENCE, DATE_DESC, DATE_ASC |
| `CETAT` | JURIDICTION_NATURE, DATE_DECISION, DATE_VERSEMENT, PUBLICATION_RECUEIL, NUMERO_DECISION, ECLI | PERTINENCE, DATE_DESC, DATE_ASC |
| `JURI` | JURIDICTION_JUDICIAIRE, CASSATION_TYPE_PUBLICATION_BULLETIN, NUM_BULLETIN, ANNEE_BULLETIN, CASSATION_NATURE_DECISION, CASSATION_FORMATION, CASSATION_DECISION_ATTAQUEE, LIEU_DECISION, DATE_DECISION_ATTAQUEE, APPEL_SIEGE_APPEL, PREMIER_DEGRE_TYPE_JURIDICTION, PREMIER_DEGRE_SIEGE, DATE_DECISION, NUM_AFFAIRE, ECLI | PERTINENCE, DATE_DESC, DATE_ASC |
| `JUFI` | PUBLICATION_RECUEIL, JURIDICTION_NATURE, DATE_DECISION, NUMERO_DECISION | PERTINENCE, DATE_DESC, DATE_ASC |
| `KALI` | LEGAL_STATUS, ARTICLE_LEGAL_STATUS, ACTIVITE, ARTICLE_QUESTION_USUELLE, DATE_SIGNATURE, DATE_PUBLICATION, NATURE_TEXTE_CITE, NUM_TEXTE_CITE, DATE_PUBLI_TEXTE_CITE, NOM_CODE_CITE, NUM_ARTICLE_CODE_CITE, IDCC, NOR, CODE_NAF_OU_APE, NUMERO_BO, ARTICLE_NUMERO | PERTINENCE, MODIFICATION_DATE_DESC, SIGNATURE_DATE_DESC/ASC |
| `ACCO` | THEME, SIGNATAIRE, DATE_SIGNATURE, ACTIVITE_PRINCIPALE, CODE_APE, IDCC, VILLE, CODE_POSTAL, DATE_DIFFUSION, SIRET_RAISON_SOCIALE | PERTINENCE, DATE_DESC, DATE_ASC |
| `CNIL` | TYPE, NATURE_DELIB, DATE_DELIB, NUMERO_DELIB, NOR | PERTINENCE, DATE_DECISION_DESC, DATE_DECISION_ASC |

**Facettes de date par fonds** (synthèse du même document) et choix du plan pour `date_from`/`date_to` (04 §6.5) :

| Fonds | Facettes de date | Retenue par le plan |
|---|---|---|
| LODA_DATE | DATE_SIGNATURE, DATE_PUBLICATION, DATE_VERSION | DATE_SIGNATURE (DATE_VERSION réservée à `version_date`) |
| CODE_DATE | DATE_VERSION seulement | aucune : plage refusée (`invalid_arg`) |
| JORF | DATE_SIGNATURE, DATE_PUBLICATION | DATE_PUBLICATION, à confirmer en M7 |
| CONSTIT | DATE_DECISION, DATE_LOI | DATE_DECISION |
| CETAT | DATE_DECISION, DATE_VERSEMENT | DATE_DECISION |
| JURI | DATE_DECISION, DATE_DECISION_ATTAQUEE | DATE_DECISION |
| CNIL | DATE_DELIB | DATE_DELIB |
| KALI | DATE_SIGNATURE, DATE_PUBLICATION, DATE_PUBLI_TEXTE_CITE | DATE_SIGNATURE |
| ACCO | DATE_SIGNATURE, DATE_DIFFUSION | DATE_SIGNATURE |
| LODA_ETAT | TEXT_DATE_SIGNATURE, TEXT_DATE_PUBLICATION | hors plan |
| CIRC | DATE_SIGNATURE, DATE_MEL, DATE_MEA | hors plan |

**Format d'une facette renvoyée** (`definitions.Facet`) : `{facetElem, field, values: map<libellé, nombre>, childs: map<valeur, Facet>, totalElement}`. `facetElem` est le nom à réutiliser comme `facette` ; `childs` correspond à `multiValeurs`. Valeurs observées (fixtures search_*.json ; recherche 03 §5, recherche 05 §2.3) :

| Fonds | `facetElem` → `field` | Valeurs observées |
|---|---|---|
| tous sauf CODE | `FOND` → `origine` | JORF, JURI, CETAT, ACCO, LEGI, CNIL, KALI, CIRC, CONSTIT, JUFI, CODE (volumétrie, §3.2) |
| LODA | `NATURE` → `nature` | ARRETE, DECLARATION, DECISION, LOI, ORDONNANCE, DECRET, DECRET_LOI, CONVENTION, CONSTITUTION, DELIBERATION, ACCORD_FONCTION_PUBLIQUE |
| LODA | `ARTICLE_LEGAL_STATUS` → `articles.etat` | les 17 états de l'enum `legalStatus` (liste fermée, identique à l'enum `CodeListRequest.states` du Swagger, recomptée le 2026-10-15 ; `DENONCE` est courant en KALI, facette `LEGAL_STATUS` de search_kali.json) : ABROGE, ABROGE_DIFF, ANNULE, DENONCE, DISJOINT, INITIALE, MODIFIE, MODIFIE_MORT_NE, PERIME, REMPLACE, SANS_ETAT, SUBSTITUE, TRANSFERE, VIGUEUR, VIGUEUR_DIFF, VIGUEUR_ETEN, VIGUEUR_NON_ETEN |
| JURI | `JURIDICTION_JUDICIAIRE`, `CASSATION_TYPE_PUBLICATION_BULLETIN` (T, F), `CASSATION_NATURE_DECISION` (avis, ordonnance, arret, autres_decisions), `CASSATION_FORMATION` (16 : CHAMBRE_CIVILE_1… TRIBUNAL_CONFLIT), `CASSATION_DECISION_ATTAQUEE` (19), `APPEL_SIEGE_APPEL` (35), `PREMIER_DEGRE_TYPE_JURIDICTION` (19), `PDC_CHECKBOX_RESTREINDRE_ARRET` (vide) | sémantique de `T`/`F` **NON VÉRIFIÉ** [V-116 · publication-bulletin-semantique] |
| CETAT | `JURIDICTION_NATURE` (CONSEIL_ETAT, COURS_APPEL, TRIBUNAL_ADMINISTATIF, TRIBUNAL_CONFLIT, COURS_DE_DISCIPLINE, COURS_APPEL_FINANCIERE, COURS_COMPTES, CHAMBRES_COMPTES ; `childs` : 9 cours, 42 tribunaux), `PUBLICATION_RECUEIL` (PUBLIE, NON_PUBLIE), `pdcRestreindreArret` | — |
| CONSTIT | `NATURE_CONSTIT` (8), `SOLUTION_CONSTIT` (34), `SOLUTION_ELECT` (12), `SOLUTION_AUTRE` (4), `NATURE_AUTRE` (5), `NATURE_ELCT` (5), `NATURE_NORME_AUTRE` (2) | `NATURE_ELCT` dans la réponse, `TYPE_DECISION` dans le document officiel |
| CNIL | `TYPE` (Décision, Délibération), `NATURE_DELIB` (11) | — |
| KALI | `LEGAL_STATUS`, `ARTICLE_LEGAL_STATUS` (8 états), `TEXTE_BASE`, `ACTIVITE` (551 libellés) | facettes = 32 354 caractères sur 36 883 (88 %) |
| ACCO | `SIGNATAIRE` (CFDT, CFE-CGC, CFTC, CGT, CGT-FO, SOLIDAIRES, UNSA, Autre), `THEME` (59) | — |
| CODE_DATE | **aucune facette** (clé `facets` absente, fixture search_code.json) | — |

Conséquences : les facettes sont découvrables à l'exécution et très volumineuses. En v0.1 elles ne sont ni renvoyées (00 §7) ni déclarées dans les DTO (04 §8.1).

**Tris** : le plan envoie `PERTINENCE` partout (00 §8), seul tri attesté pour CODE (tous les exemples officiels CODE l'utilisent, l.233, l.755, l.827, l.895). Un tiers autorise `PUBLICATION_DATE_*` sur CODE. `secondSort` : exemples officiels `ID`, `DATE_DESC`, `PERTINENCE` (l.959, l.1037, l.1111, l.1163, l.1271), liste non exhaustive. Validité des tris par fonds et liste de `secondSort` : **NON VÉRIFIÉ** [V-85 · tris-par-fond].

### 3.8 Pagination

- `pageSize` : **max 100** (`RechercheSpecifiqueDTO.pageSize` « (max=100) » ; toutes les `/list/*` « (max 100) » ; `jorfCont.pageSize` « (max 100) »). Un exemple officiel CETAT utilise 100 (l.1027). VÉRIFIÉ.
- `pageNumber` commence à **1** (exemples officiels ; référence server.py:150). Le document officiel indique « 0 (valeur par défaut) » pour `jorfCont` seulement (recherche 05 §2.4).
- `typePagination` : `DEFAUT` ou `ARTICLE`, « Spécifique pour les recherches dans les articles d'un texte, dans les autres cas la valeur sera toujours DEFAULT. Lors de la navigation dans plusieurs pages, il est nécessaire de passer la valeur reçue dans la réponse précédente » (Swagger). Observé : `CODE_DATE` répond `ARTICLE` avec `totalArticleResultNumber = 1` (fixture search_code.json) ; les 7 autres fixtures répondent `DEFAUT`. Le plan mémorise la valeur reçue par requête normalisée sans `page` (`PaginationMemo`, TTL 1 h, 1 024 entrées) et la renvoie pour la page suivante ; sans mémo, `ARTICLE` pour CODE en page > 1, `DEFAUT` sinon ; `typePagination` n'est jamais exposé au modèle (00 §7 ; 04 §6.6). Effet réel en page 2 : **NON VÉRIFIÉ** [V-82 · typepagination-page2] (V-82).
- Pagination profonde : aucune limite documentée. Le moteur est vraisemblablement Elasticsearch (`idTechInjection: "index_DATA_NEXT_ARTICLE_…"`, paramètre `page` de `getJuriPlanClassement` décrit comme « la requete ELK »), dont la fenêtre par défaut est de 10 000 résultats, limite appliquée par Judilibre (§6.4). **NON VÉRIFIÉ** [V-83 · pagination-profonde] (V-83). Décision (00 §7, AM-04) : `page × page_size > 10 000` est rejeté en `invalid_arg` par le cas d'usage, sans appel réseau (04 §6.7).

### 3.9 Limites et pièges connus

1. Facette inexistante, champ JSON inconnu ou `fond=ALL` avec filtres → 500 selon un tiers ; « ALL : aucun filtre de date » selon un autre (recherche 05 §1.13, §2.2). Vérifié en sandbox le 2026-10-28 : une facette inconnue et `fond = ALL` avec un filtre de date donnent un 500, un champ racine inconnu est ignoré [V-69 · 500-requete-invalide] ; **NON VÉRIFIÉ** [V-84 · fond-all-comportement]. Parade : n'envoyer que des facettes des tables du §3.7, jamais de champ hors Swagger, jamais `ALL` ; un 500 n'est réessayé qu'une fois (00 §8, §1.5).
2. `NOM_CODE` (CODE_DATE) et `TEXT_NOM_CODE` (CODE_ETAT) : la documentation officielle les utilise avec le titre exact du code ; deux tiers les disent cause de 500 avec du texte libre. **NON VÉRIFIÉ** [V-121 · nom-code-filtre]. Le plan ne les envoie pas : jamais `NOM_CODE` sans V-121 ; `find_article` filtre les hits par `cid` (00 §8 ; 04 §7.2).
3. Le filtre `CASSATION_TYPE_PUBLICATION_BULLETIN` serait inopérant (commentaire pylegifrance, recherche 03 §3.2) : non utilisé.
4. `nature` et NOR changent de casse entre recherche et consultation (`arret`/`ARRET`, `cscl2115694s`/`CSCX2531180S`, §4.12).
5. Aucune date dans les hits JURI, CONSTIT et CETAT : la date n'est que dans le titre en prose (recherche 03 §4.2). Sémantique de `date` par fonds mesurée en sandbox le 2026-10-28 [V-87 · date-hit-semantique] (09 §2.20 : elle dépend du fond).
6. Aucun `ecli` dans les hits de recherche.
7. `num` des extraits KALI malformé (`"8.4 de la convention)"`) : **partiel** en sandbox le 2026-10-28 [V-88 · kali-extraits-num] : aucun `num` mal formé sur 193 extraits (09 §2.20).
8. 0 résultat = succès (`totalResultNumber = 0`), jamais `not_found` (04 §11.1 ; parité E-04).

### 3.10 Gabarits du plan

Gabarit v0.1 (`fr_search`, parité de la référence server.py:133-156, `typeRecherche` selon `match`, `singleDate` à midi Europe/Paris, 00 §8) : 04 §6.1. Gabarit M6 de recherche exacte, d'après l'exemple officiel « article 57 de la loi du 17 juillet 1978 » (doc « exemples » l.497-570), pour `version_date` = 2026-10-15 :

```json
{
  "fond": "LODA_DATE",
  "recherche": {
    "champs": [{ "typeChamp": "NUM_ARTICLE", "operateur": "ET",
                 "criteres": [{ "typeRecherche": "EXACTE", "valeur": "57", "operateur": "ET" }] }],
    "filtres": [
      { "facette": "DATE_VERSION", "singleDate": 1792058400000 },
      { "facette": "DATE_SIGNATURE", "dates": { "start": "1978-07-17", "end": "1978-07-17" } }
    ],
    "pageNumber": 1, "pageSize": 20, "operateur": "ET", "sort": "PERTINENCE", "typePagination": "ARTICLE"
  }
}
```

`singleDate` vaut ici midi Europe/Paris du 2026-10-15 (vecteur de 00 §8). Les autres recherches exactes (`NUM`, `NUM_AFFAIRE`, `NUM_DEC`, `NUM_DELIB`, `IDCC`) suivent la même forme avec la facette de date du fonds pour lever une ambiguïté (04 §6.9, §7.2). Leur comportement réel (casse, tirets typographiques, zéros de tête) est **NON VÉRIFIÉ** [V-120 · recherche-exacte-par-numero].

Autres gabarits M6 (00 §8 révision 2, AM-29 ; détail 04 §7.2) :

- `find_article` à une autre date que le jour : `fond = CODE_DATE`, `NUM_ARTICLE` en `EXACTE`, filtre `DATE_VERSION` (midi Paris de la date demandée), `typePagination = ARTICLE`, `pageSize = 100`, **sans** `NOM_CODE` ([V-121 · nom-code-filtre], V-121) ; l'adapter garde le hit dont `titles[0].cid` est le `LEGITEXT` du code, puis l'extrait dont `num` égale le numéro demandé. `/consult/getArticleWithIdAndNum` ne sert que la version en vigueur (§2.8).
- ECLI cité (JURI, CETAT) : `valeur: "*"` sur `ALL`, filtre textuel `{"facette": "ECLI", "valeurs": ["ECLI:FR:CCASS:2025:C100399"]}` (forme de la sonde V-128) ; effet **NON VÉRIFIÉ** [V-128 · ecli-filtre-juri-cetat].

---

## 4. Formes JSON des réponses utilisées

### 4.1 Fiabilité des fixtures

16 fixtures de la référence (`tests/fixtures/*.json`, Apache-2.0). Mesures de la recherche 03 §2 (caractères = JSON compact) :

| Fixture | Endpoint | Fond | Octets | Caractères | Mojibake | Élagage ou troncature |
|---|---|---|---|---|---|---|
| search_loda.json | `/search` | LODA_DATE | 3 821 | 2 959 | oui (3 champs) | 2 hits sur 1 786 |
| search_code.json | `/search` | CODE_DATE | 1 865 | 1 433 | oui (1) | 1 hit ; `facets` absente |
| search_juri.json | `/search` | JURI | 12 850 | 10 115 | oui (22 valeurs, 14 clés) | 2 hits sur 122 147 |
| search_constit.json | `/search` | CONSTIT | 10 068 | 7 450 | non | 3 hits sur 1 817 |
| search_cetat.json | `/search` | CETAT | 14 049 | 9 776 | non | 3 hits sur 51 531 |
| search_cnil.json | `/search` | CNIL | 3 860 | 3 090 | non | 2 hits sur 1 134 |
| search_kali.json | `/search` | KALI | 42 487 | 36 883 | non | 2 hits sur 27 337 ; facettes 32 354 caractères |
| search_acco.json | `/search` | ACCO | 7 984 | 6 205 | non | 2 hits sur 54 491 |
| consult_loda.json | `/consult/lawDecree` | — | 12 093 | 7 488 | oui (25) | ≤ 5 articles par section ; sections réduites à 4 clés, articles à 3 ; 62 articles alors que la numérotation va à 109 |
| consult_article.json | `/consult/getArticle` | — | 3 198 | 2 612 | oui (1) | absents mais déclarés : `texteHtml`, `articleVersions`, `lienCitations`, `lienModifications`, `lienConcordes`, `lienAutres`, `notaHtml` |
| consult_juri.json | `/consult/juri` | JURI | 8 239 | 7 421 | oui (8) | `texte` coupé à 600 caractères ; les 11 clés `*Html` absentes |
| consult_juri_constit.json | `/consult/juri` | CONSTIT | 34 758 | 32 324 | non | texte apparemment complet (14 322) |
| consult_juri_cetat.json | `/consult/juri` | CETAT | 39 367 | 37 220 | non | texte apparemment complet (14 601) |
| consult_cnil.json | `/consult/cnil` | — | 4 042 | 3 551 | non | `texte` et `texteHtml` = 600 + `" [...]"` |
| consult_kali.json | `/consult/kaliText` | — | 4 887 | 4 035 | non | 1 section, 1 article ; `content` = 400 + `" [...]"`, balise `<a` non refermée |
| consult_acco.json | `/consult/acco` | — | 1 294 | 1 064 | non | `data` et `attachment` absents |

- **Trois campagnes de capture** se distinguent (recherche 03 §2) : (a) indentation 1 et mojibake, fonds de la v0.1.0 de la référence (2026-06-24) ; (b) indentation 2, saines, CONSTIT et CETAT (v0.2.0, 2026-07-07) ; (c) indentation 1, saines, CNIL, KALI, ACCO (v0.3.0). Le même endpoint `/consult/juri` est corrompu en (a) et sain en (b).
- Les `[...]` en tête et en fin d'**extraits de recherche** viennent de l'API (présents dans les 8 fixtures `/search`, saines comprises). Les `[...]` en fin de **texte intégral** (CNIL, KALI) sont des troncatures de l'outil de capture.
- Conséquence : ces fixtures valent pour les **types**, la **nullabilité** et les **clés**, pas pour les **volumes**. Les volumes (textes longs, TOC réelles, listes de liens) exigent de nouvelles captures brutes par `cargo xtask record-fixture` (00 §2 ; 04 §13.4) : sandbox par défaut ; en production, seulement des fixtures de données publiques marquées `env = production`, enregistrées à la main par le mainteneur (00 §10, adoptée par défaut, Q3). Tailles réelles **NON VÉRIFIÉ** [V-75 · taille-max-textes] (D-35).

### 4.2 `/search` : `SearchResponseDTO`

Enveloppe :

| Champ | Type observé | Nullabilité | Usage |
|---|---|---|---|
| `results` | tableau | jamais nul (1 à 3 éléments dans les fixtures) | hits |
| `totalResultNumber` | entier | jamais nul | `total` |
| `totalArticleResultNumber` | entier | 0 sauf CODE (1) | articles trouvés (CODE) |
| `typePagination` | chaîne | jamais nulle | à renvoyer (§3.8) |
| `facets` | tableau | absent pour CODE | non déclaré (§3.7) |
| `executionTime` | entier | — | ignoré |
| `descriptionFusionHtml` | null | toujours nul | ignoré |

`results[]` (`SearchResult`) porte 32 clés présentes dans tous les fonds, même inutiles (31 pour LODA, sans `dossiersLegislatifs`). Champs utiles (recherche 03 §4.2) :

| Champ | LODA | CODE | JURI | CONSTIT | CETAT | CNIL | KALI | ACCO |
|---|---|---|---|---|---|---|---|---|
| `titles[0].id` | `LEGITEXT…_JJ-MM-AAAA` | `LEGITEXT…_09-04-2026` | `JURITEXT…` | `CONSTEXT…` | `CETATEXT…` | `CNILTEXT…` | `KALITEXT…` | `ACCOTEXT…` |
| `titles[0].cid` | `JORFTEXT…` (≠ id) | `LEGITEXT…` | = id | = id | = id | = id | = id | = id |
| `titles[0].title` | `<mark>`, mojibake | « Code civil » | sans `<mark>` | `<mark>` | sans | `<mark>` | `<mark>`, espace final | `<mark>` |
| `titles[0].{legalStatus, startDate, endDate, nature}` | toujours `null` dans les 8 fonds | | | | | | | |
| `text` (extrait) | null | null | ≤ 300 | ≤ 488 | ≤ 493 | ≤ 275 | null | ≤ 267 (avec `\t`, `\n`) |
| `nature` | `LOI`, `DECRET` | `code` | `arret` | `dc`, `qpc` | `texte` | `deliberation` | `lettre d'adhesion`, `avenant` | null |
| `origin` | `LEGI` | `LEGI` | `JURI` | `CONSTIT` | `CETAT` | `CNIL` | `KALI` | `ACCO` |
| `etat` | `VIGUEUR` | `VIGUEUR` | null | null | null | `VIGUEUR` | `VIGUEUR`, `ABROGE` | null |
| `solution` | null | null | `Cassation`, `Rejet` | `Conformité`… | null | null | null | null |
| `date` (ISO `+0000`) | `2016-10-09…` | `1804-03-21…` | null | null | null | null | null | null |
| `datePublication` | `2016-10-08…` | sentinelle 2999 | null | null | null | `2026-01-14…` | sentinelle 2999 | null |
| `dateSignature`, `dateDiffusion` | null | null | null | null | null | null | null | `2025-12-19…`, `2026-03-10…` |
| `nor` | `ECFI1524250L` | null | null | minuscules | null | null | minuscules | null |
| `appellations[]` | `["loi Lemaire", …]` | [] | [] | [] | [] | [] | [] | [] |
| `resumePrincipal[]` | [] | [] | 1 (≤ 193, `<br/>` final) | [] | 1 à 3 (≤ 598) | [] | [] | [] |
| `raisonSociale`, `idcc`, `themes[]` | — | — | — | — | — | — | `idcc` **null** | `CORSAIR (Télétravail)`, `"275"` (chaîne), `["Télétravail"]` |
| `sections[]` | [] | 1 section | 1-2 (tout nul sauf `extracts`) | [] | 1 | [] | 1-3 | 1 |

`sections[]` (`SearchSection {id, title, dateVersion, legalStatus, extracts[]}`) et `extracts[]` (`SearchExtract {id, title, legalStatus, dateVersion, dateDebut, dateFin, searchFieldName, num, values[], type}`) :

- CODE : section `LEGISCTA…` « Titre Ier : Des droits civils », extrait `id = LEGIARTI000006419288`, `num = "9"`, `dateDebut`/`dateFin` ISO (fin sentinelle), `values[0]` avec `[...]`. Source des `articles[]` des hits CODE (parité référence citations.py:150-165).
- KALI : `id`/`title` parfois `""` ; `dateVersion = 2026-07-08T07:47:16.102+0000` (heure de la requête, pas une date juridique) ; extraits `KALIARTI…` dont le `num` n'est pas fiable.
- JURI, CETAT, ACCO : `id`/`num` nuls ; `searchFieldName` dit où le terme a été trouvé (JURI : `Résumé principal`, `Abstrat`, `Résumé autre`, `Texte appliqué`, `Précédent jurisprudentiel` ; ACCO : `Raison sociale`, `Thématiques`).

Poids : `results` de 1 294 (CODE) à 7 911 (CETAT) caractères pour 1 à 3 hits ; `facets` de 699 (CNIL) à 32 354 (KALI) caractères (recherche 03 §4.2).

### 4.3 `/consult/lawDecree` : `ConsultTextResponse`

Fixture consult_loda.json (élaguée) :

| Champ | Type | Valeur ou nullabilité | Usage |
|---|---|---|---|
| `id` | chaîne | `LEGITEXT000033205014_08-09-2023` (**suffixé**) | id + date de début de version |
| `cid` | chaîne | `JORFTEXT000033202746` | JORF d'origine (lexique DILA l.39-59) |
| `title` | chaîne (71) | « LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1) » | titre, citation |
| `nor` | chaîne | `ECFI1524250L` | NOR |
| `eli`, `alias` | null | toujours nuls (référence DISCOVERY.md:34-40) | jamais fabriqué ; vérifié en sandbox le 2026-10-28, production non essayée [V-102 · eli-natif-null] |
| `etat` | null | null | — |
| `jurisState` | chaîne | `Vigueur` (casse « Titre ») | statut (repli, E-10) |
| `nature` | chaîne | `LOI` | nature |
| `dateDebutVersion`, `dateFinVersion` | `AAAA-MM-JJ` | `2023-09-08`, `2999-01-01` (sentinelle) | bornes de version |
| `modifDate`, `jurisDate` | `AAAA-MM-JJ` | `2023-09-08`, `2016-10-09` | — |
| `dateParution` | entier ms | 1475884800000 (2016-10-08) | parution |
| `dateTexte`, `textNumber` | null | numéro et date de signature seulement dans `title` | — |
| `jorfText`, `numParution` | chaîne | `JORF n°0235 du 8 octobre 2016`, `0235` | référence JO |
| `textAbroge` | booléen | false | statut |
| `appellations[]` | chaînes | `["loi Lemaire", "loi numérique", "loi Lemaire [2016]"]` | alias (`fr_resolve`) |
| `sections[]` | récursif | 3 au premier niveau, 26 au total, profondeur 3 ; clés `{id, title, articles, sections}` seulement | TOC |
| `sections…articles[]` | objets | 62 articles `{id, num, etat}` | TOC |
| `articles[]` (racine) | tableau | [] | articles hors section |

Le Swagger déclare en plus `visa`, `signers`, `prepWork`, `notice`, `nota`, `liens`, `dossiersLegislatifs`, et pour chaque `ConsultSection`/`ConsultArticle` : `intOrdre` (« Numéro indiquant l'ordre d'affichage »), `dateDebut`, `dateFin`, `content` (« Contenu HTML de l'article »), `lstLienModification`, `comporteLiens`. Les listes `sections` et `articles` sont « ordonnées ». Présences réelles : `intOrdre` présent sur tous les nœuds, vérifié en sandbox le 2026-10-28 [V-97 · intordre-lawdecree] ; `content` présent sur tous les articles, vérifié en sandbox le 2026-10-28 [V-95 · lawdecree-content-articles] ; ordre sans `intOrdre` **NON VÉRIFIÉ** [V-98 · toc-ordre-sans-intordre]. Comportement, taille et durée sur un `LEGITEXT` de code : mesurés en sandbox le 2026-10-28 [V-96 · lawdecree-sur-code] (D-47 ; V-96 : seuil de 4 Mio dépassé, 6,2 Mio pour le Code civil et 31,4 Mio pour le Code du travail, au-dessus de la borne de corps de 16 Mio ; décision C2 option (b) : `get_act` d'un texte trop gros est refusé par un message clair, 09 §2.20, 04 §7.3).

### 4.4 `/consult/getArticle` : `GetArticleResponse`

Enveloppe `{executionTime, dereferenced, article}`. `article` dans consult_article.json :

| Champ | Type | Valeur | Usage |
|---|---|---|---|
| `id`, `cid` | chaîne | `LEGIARTI000006419288` (égaux) | id |
| `idTexte`, `cidTexte` | null | — | le texte parent est dans `textTitles` et `context` |
| `num`, `etat`, `type`, `nature`, `origine`, `versionArticle` | chaîne | `9`, `VIGUEUR`, `AUTONOME`, `Article`, `LEGI`, `3.0` | — |
| `texte` | chaîne (336) | « Chacun a droit au respect de sa vie privée. … » (mojibake, 2 NBSP, un seul paragraphe pour deux alinéas) | texte (D-26) |
| `dateDebut` | entier ms | 17193600000 (1970-07-19) | début de vigueur |
| `dateFin`, `dateDebutExtension`, `dateFinExtension` | entier ms | 32472144000000 (sentinelle 2999-01-01) | fin |
| `context.titresTM[]` | `{debut, fin, titre, xPath, cid, id, etat}` | « Livre Ier : Des personnes » → « Titre Ier : Des droits civils » (dates `AAAA-MM-JJ`) | chemin complet |
| `context.titreTxt[]` | idem | « Code civil », `LEGITEXT000006070721` | texte parent |
| `sectionParentCid`, `sectionParentId`, `sectionParentTitre` | chaîne | `LEGISCTA000006117610`, « Titre Ier : Des droits civils » | section parente |
| `fullSectionsTitre` | chaîne | « Livre Ier : Des personnes » : **incomplet** | parité |
| `textTitles[]` | 1 objet | `titre = "Code civil"`, `nature = "CODE"`, `dateDebut = -5231606400000` (1804-03-21, **négatif**), `nor`/`num` = `""`, `appellations = null` | titre du code |
| `versionPrecedente` | chaîne | `LEGIARTI000050396821` | lien de version (M7) |
| `isMultipleVersions` | booléen | true (le Swagger dit `multipleVersions`) | alias |
| `computedNums[]`, `ordre`, `refInjection`, `idTechInjection` | — | `["9"]`, 128847, `MD-20260424_…`, `index_DATA_NEXT_ARTICLE_…` | ignorés |

Déclarés par le Swagger, absents de la fixture : `texteHtml`, `notaHtml`, `articleVersions[]`, `lienCitations[]` (`LienCitation {articleId, articleNum, textCid, textTitle, natureText, linkType, linkOrientation, dateDebut, datePubli, parentCid, numTexte}`), `lienModifications[]` (`LienModification {…, datePubliTexte, dateSignaTexte, dateDebutCible}`), `lienConcordes[]`, `lienAutres[]`, `calipsos`, `conteneurs`. Présence en direct : `texteHtml` vérifié en sandbox le 2026-10-28, production non essayée [V-90 · texte-html-getarticle-juri] ; autres champs présents sans élagage, vérifié en sandbox le 2026-10-28 [V-107 · getarticle-champs-elagues].

### 4.5 `/consult/juri` et `/consult/cnil` : `TexteSimple`

`ConsultJuriTextResponse` et `ConsultCnilTextResponse` ont la même enveloppe `{executionTime, dereferenced, text}`. `text` est un `TexteSimple` de 108 clés déclarées (Swagger), toutes présentes dans les fixtures CONSTIT et CETAT. JURI n'a pas les 11 clés `*Html` ; CNIL n'a pas `citationJp`, `citationJpHtml`, `sommaire`, `visas`, `visasHtml`.

| Champ | JURI | CONSTIT | CETAT | CNIL |
|---|---|---|---|---|
| `id` | `JURITEXT000051743650` | `CONSTEXT000052555275` | `CETATEXT000054178503` | `CNILTEXT000053352643` |
| `cid` | null | null | null | null |
| `origine` / `nature` | `JURI` / `ARRET` | `CONSTIT` / `QPC` | `CETAT` / `Texte` | `CNIL` / `Délibération` |
| `num` | `12500399` (n° interne) | `2025-1173` | `506507` (n° de requête ? **NON VÉRIFIÉ** [V-112 · cetat-num-requete]) | `SAN-2026-002` |
| `numeroAffaire[]` | `["24-13.470"]` | [] | [] | [] |
| `ecli` | `ECLI:FR:CCASS:2025:C100399` | `ECLI:FR:CC:2025:2025.1173.QPC` | `ECLI:FR:CECHR:2026:506507.20260529` | null |
| `juridiction` / `natureJuridiction` | `Cour de cassation` / idem | `Conseil constitutionnel` / idem | `Conseil d'État` / `CONSEIL_ETAT` | null |
| `formation` | `CHAMBRE_CIVILE_1` (code) | null | `2ème - 7ème chambres réunies` (prose) | null |
| `solution` | `Cassation` | `Conformité` | null | null |
| `titre` | `Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin` | `Décision 2025-1173 QPC - 07 novembre 2025 - …` | `Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507` | `Délibération SAN-2026-002 du 8 janvier 2026` (`titreLong` différent, 140 car.) |
| `dateTexte` = `dateTexteComputed` | 1748995200000 | 1762473600000 | 1780012800000 | 1767830400000 |
| `datePubli` = `datePubliComputed` | null | null | null | 1768348800000 |
| `publicationRecueil`, `typePublicationBulletin` | `C`, `T` | null | `B`, null | null |
| `decisionAttaquee` | `{formation: "Cour d'appel de Lyon", date: 1704844800000}` | null | null | null |
| `texte` | 600 (tronqué) | 14 322 | 14 601 | 606 (tronqué) |
| `texteHtml` | **absent** | 14 791 (`<BR></BR>` ×52, `<br/>` ×4, `<RECH_ECLI>`) | 15 572 (`<br/>` ×85, indentation) | 606 (`<p>` ×3) |
| `sommaire[]` | 1 × `{id, abstrats, resumePrincipal, autreResume}` | [] | 6 entrées (`9A`… `resumePrincipal`, `8A`… `abstrats`, moitié nulles) | absent |
| `citationJp` | prose de 213 caractères (« … à rapprocher : 1re Civ., 25 mai 2023, pourvoi n° 21-23.174 … ») | null | `""` | absent |
| `liens[]` | 2 × `{typeLien: "CITATION", sens: "source", title, cidTexte: "", id: "", …}` | [] | [] | [] |
| `titrages[]` | chaînes de `JURINOME…` séparés par `-` | [] | chaînes de `CETANOME…` | [] |
| CONSTIT : `natureQualifiee`, `titreLoiDef`, `idTexteJo`, `titreJo`, `urlCC` | — | remplis | — | — |
| CNIL : `natureDelib`, `etat` | — | — | — | `Sanction`, `VIGUEUR` |
| `ancienId` | null | null | `JG_L_2026_05_000000506507` | null |

Règles documentées par le Swagger : `dateTexteComputed` et `datePubliComputed` valent `null` « si la date du texte est null ou >= 2222-01-01 » ; `relevantDate` contient la date du texte « si elle existe (!= null && != 2999-01-01) sinon […] la date de publication ». Le mapping lit les `…Computed` en priorité (04 §9.1).

Les `liens[]` JURI ont des identifiants vides et des titres fragmentés : « article 2270-1 du code civil » est coupé entre deux entrées, dans l'ordre inverse (recherche 03 §11.6). Cause **NON VÉRIFIÉ** [V-117 · liens-juri-fragmentes]. Ce sont des indices en prose pour le parseur de citations (M6, M7), pas des liens résolus. Le texte « Publié au bulletin » accompagne `typePublicationBulletin = "T"`, mais pylegifrance documente aussi `T` comme « Cité au BICC » : [V-116 · publication-bulletin-semantique].

### 4.6 `/consult/kaliText` : `ConsultKaliTextResponse`

Racine (consult_kali.json) : `id` = `cid` = `KALITEXT000049878950` (sans suffixe), `title` (174 caractères, espace final), `nor` = `ASET2450455M`, `jurisState` = `VIGUEUR_ETEN`, `etat` = `nature` = null, `modifDate` = `2026-03-01`, `dateParution` = **sentinelle** 32472144000000, `numeroBo` = `2024-20`, `originePubli` = `BO n°2024-20`, `typeTexte` = `TEXTE_ATTACHE`, `nota`, `visas`, `visasHtml` = `""`.

- `conteneurs[]` : `{cid: "KALICONT000005635096", num: "2121" (IDCC), numero: "0000000000002121", titre: "Convention collective nationale de l'édition du 14 janvier 2000\n"}`. L'IDCC d'un texte KALI vient d'ici : il est nul dans les hits KALI de `/search`.
- `signataires` est un **objet** `{faitA, patron, syndic, adhesion, denonciation, signExt, execution}` alors que `TexteSimple.signataires` est une **chaîne** : même nom, types différents selon l'endpoint.
- `sections[]` (18 clés) : `KALISCTA…`, `intOrdre = 1074003967`, `etat`, `title` (espace final), `dateDebut`/`dateFin` en **`AAAA-MM-JJ`**.
- `sections[].articles[]` (33 clés) : `KALIARTI…`, `intOrdre`, `num` jusqu'à 96 caractères (« 19 (des dispositions communes de la convention collective nationale de l'édition phonographique) »), `path`, `pathTitle[]`, **`content`** (HTML, avec des liens `<a href="/affichCodeArticle.do?cidTexte=LEGITEXT…&idArticle=LEGIARTI…">`), `dateDebut`/`dateFin` en **entiers ms** (alors que la section parente est en `AAAA-MM-JJ`), `multipleVersions`, `comporteLiens = true`, `surtitre`, `lstLienModification[]` (`linkType: "EXTENSION"`, `articleId: "JORFARTI…"`, dates `AAAA-MM-JJ`), `lstLienCitation[]` (`linkType: "CITATION"`, `articleId: "LEGIARTI000006901195"`, `articleNum: "L1242-2"`, dates ms), **non vide** malgré la description « Toujours vide » du Swagger.

### 4.7 `/consult/acco` : `ConsultAccoResponse`

`acco` (consult_acco.json) : `id`, `origine = ACCO`, `nature = ACCORD`, `titreTexte`, `url` (chemin XML interne), `attachementUrl = /2025/12/19/T09426064936-32862158600143.docx`, `fileSize = "624,3 Ko"` (chaîne avec unité), `numero = T09426064936`, `siret`, `codeUniteSignataire`, `codeApe = 5110Z`, `codeIdcc = "275"` (chaîne), `raisonSociale`, `conformeVersionIntegrale`, `secteur`, `themes[] = {code, libelle, groupe}`, `syndicats[] = {code, libelle}`, `signataires[] = ["91"]`, dates en ms (`dateMaj`, `dateDepot`, `dateTexte` = `relevantDate`, `dateEffet`, `dateDiffusion`, `dateFin` sentinelle).

Absents de la fixture, déclarés par le Swagger : `data` (« Contenu du fichier attaché en base64 ») et `attachment` (`Attachment {title, name, language, author, keywords, date, content « Contenu du fichier textuel », content_length, content_type}`). Un `attachment.content` textuel donnerait le texte intégral sans décoder le `.docx`. Présence, contenu et taille vérifiés en sandbox le 2026-10-28 : `attachment.content` porte le texte extrait du .docx (34 192 caractères pour l'accord sondé) [V-106 · acco-data-attachment]. Le plan ne rend que les métadonnées (P-036 ; référence SOURCES.md:15).

### 4.8 Endpoints M6 et M7

Aucune fixture : formes du Swagger seulement (§2.8). Points notables :

- `ConsultJorfResponse.id` a pour exemple `LEGITEXT000006070994_01-08-2016` (suffixe de version, comme `ConsultTextResponse.id`) ; `eli` a pour exemple `/eli/arrete/2019/1/18/TREK1901124A/jo/texte`. Côté JORF, un ELI natif est donc plausible ; il serait rendu tel quel, jamais fabriqué (00 §4). `ConsultJorfResponse` est la réponse de `getJoWithNor`, `eliAndAliasRedirectionTexte` (M6) et `/consult/jorf` (candidat M7).
- `getArticleWithIdEliOrAlias` (M6) renvoie un `GetArticleResponse`, de même forme que `/consult/getArticle` (§4.4) ; son corps est `ArticleIdEliOrAliasRequest {`**`idEliOrAlias`**`}`, celui d'`eliAndAliasRedirectionTexte` `JorfConsultWithIdEliAliasRequest {`**`idEliOrAlias`**`}` (Swagger).
- `CodeListResult` et `SuggestValue.dateVersion` (`/suggest`, retiré) : exemples ISO `2021-04-15T16:49:47.707+0000`.
- `ArticleVersion.ordre` : « Sert au tri des articles au sein de leur élément parent ».
- `LiensRelatifsDTO.cidParent` : « Renseigné si besoin pour créer le lien vers l'élément ».
- Formes réelles : **NON VÉRIFIÉ** [V-122 · endpoints-m6-reponses], [V-133 · eli-redirection-endpoints], [V-135 · endpoints-m7-reponses], [V-136 · related-links-forme].

### 4.9 Dates

Mesure sur les 16 fixtures (recherche 03 §11.2) : **39** entiers epoch ms (27 dates, 11 sentinelles `32472144000000`, 1 négatif `-5231606400000` = 1804-03-21) ; **34** chaînes ISO `AAAA-MM-JJTHH:MM:SS.sss+0000` (dont 5 sentinelles) ; **16** chaînes `AAAA-MM-JJ` (dont 6 sentinelles).

| Endpoint | Encodage |
|---|---|
| `/search` | ISO `+0000` uniquement (`date`, `datePublication`, `dateSignature`, `dateDiffusion`, `sections[].dateVersion`, `extracts[].{dateVersion, dateDebut, dateFin}`) |
| `/consult/lawDecree` | `dateParution` ms ; `dateDebutVersion`, `dateFinVersion`, `modifDate`, `jurisDate` en `AAAA-MM-JJ` |
| `/consult/kaliText` | `dateParution` ms ; `sections[].dateDebut/dateFin` `AAAA-MM-JJ` ; `sections[].articles[].dateDebut/dateFin` ms ; `lstLienModification` `AAAA-MM-JJ` ; `lstLienCitation` ms |
| `/consult/getArticle` | ms (`dateDebut`, `dateFin`, `…Extension`, `textTitles[]`) ; `context.titresTM[].debut/fin` `AAAA-MM-JJ` |
| `/consult/juri`, `/consult/cnil`, `/consult/acco` | ms |
| Swagger | `type: string, format: date-time` avec des exemples en **chaîne de chiffres** (`"1546819200000"`, `"1470787200000"`) : accepter aussi un nombre en chaîne |

Règles (00 §4 ; 03-domaine §6 ; 04 §9.1) :

- Tous les entiers non sentinelles sont des multiples exacts de 86 400 000 (minuit UTC) ; une seule chaîne ISO n'est pas à minuit (heure de requête KALI). La date civile UTC est donc la bonne lecture. Convention générale vérifiée en sandbox le 2026-10-28, sauf deux métadonnées de fichier joint [V-100 · convention-horaire-timestamps]. C'est aussi la raison de la règle de midi Europe/Paris pour `singleDate` en entrée (00 §8, §3.6).
- Toute date ≥ 2222-01-01 devient `None`, donc un champ omis en sortie dont le sens est documenté par champ (00 §4, AM-01) : règle des champs `…Computed` du Swagger, qui couvre `2999-01-01` (fin ouverte) et `2222-02-22`. Cette seconde valeur signifie, dans les dumps DILA, « entrée en vigueur différée à une date non déterminée » (§7.8) ; son emploi par l'API est **NON VÉRIFIÉ** [V-101 · sentinelle-2222-debut].
- Epoch négatifs : conversion calendaire pure : le mapping appelle `CivilDate::from_unix_millis` (domaine, `div_euclid`, date.rs ; la recherche 03 §11.2 avait relevé le même comportement dans `chrono`, historique), indépendante de la plateforme (corrige D-20, E-35).
- Un booléen n'est jamais une date (la référence acceptait `true`, D-20).

### 4.10 Identifiants

- Préfixes observés, tous suivis d'exactement **12 chiffres** : `LEGITEXT`, `JORFTEXT`, `LEGIARTI`, `LEGISCTA`, `JURITEXT`, `CONSTEXT`, `CETATEXT`, `CNILTEXT`, `KALITEXT`, `KALIARTI`, `KALISCTA`, `KALICONT`, `JORFARTI`, `ACCOTEXT`, `JURINOME`, `CETANOME` (recherche 03 §11.3). Autres préfixes du Swagger : `JORFCONT`, `JORFDOLE`. Généralité du format hors fixtures : vérifiée en sandbox le 2026-10-28, production non essayée [V-103 · id-format-hors-fixtures].
- Suffixe `_JJ-MM-AAAA` : observé seulement sur des `LEGITEXT`, dans `titles[].id` (LODA, CODE) et l'`id` de `/consult/lawDecree`. C'est la **date de début de la version** (`LEGITEXT000033205014_08-09-2023` ↔ `dateDebutVersion = 2023-09-08`). Le domaine la sépare par `parse_versioned` (00 §4), qui vérifie que le suffixe est une vraie date, la conserve, et refuse un suffixe mal formé (`_foo`, `_` seul, `_31-02-2023`) ; `Id::parse` livré reste tolérant et jette encore le suffixe (**tâche de tête de M2**, 00 §11.1). Suffixe bien formé sur un autre préfixe que `LEGITEXT` : toléré puis retiré, tolérance documentée ; le suffixe n'apparaît sur aucun autre préfixe en sandbox le 2026-10-28 [V-104 · suffixe-version-autres-prefixes].
- `id` (version) contre `cid` (« Chronical ID », commun à toutes les versions, lexique DILA l.37-59) : LODA `cid = JORFTEXT…` ≠ `id` ; CODE `cid = LEGITEXT` sans suffixe ; ailleurs `cid = id` en recherche et `cid = null` en `/consult/juri`.
- La juridiction et l'URL se déduisent du préfixe validé, jamais d'`origine` (00 §4, corrige D-28).
- Identifiants dans les textes : `idArticle=(LEGIARTI\d{12})` dans les `href` KALI ; `<RECH_ECLI>ECLI:…</RECH_ECLI>` dans `texteHtml`.

### 4.11 Textes et HTML

- **Doublon** : le texte figure deux fois, `texte` (plat) et `texteHtml`. Ils représentent 90 % du JSON compact pour CONSTIT (29 117 sur 32 324) et 81 % pour CETAT (30 244 sur 37 220) (recherche 03 §8.3). `texte` est plus long de 30 à 35 caractères (ECLI final de `<RECH_ECLI>`).
- `texte` n'a **aucun saut de ligne** ; `texteHtml` porte les paragraphes : 49 (CONSTIT), 43 (CETAT), 3 (CNIL tronqué) après découpe (D-26). Seuil de complétude du HTML : **NON VÉRIFIÉ** [V-92 · texthtml-vs-texte-complet].
- Balises : `<BR>`/`</BR>` en majuscules (CONSTIT), `<br/>`, `<p>`/`</p>` (CNIL), `<p align="left">` et `<a href=… title=…>` (KALI), `<RECH_ECLI>` (à supprimer avec son contenu, l'ECLI étant dans `ecli`) ; `<br/>` dans les résumés et extraits ; `<mark>` dans les titres et extraits de recherche, jamais dans les consultations. Autres balises : **NON VÉRIFIÉ** [V-93 · balises-html-autres]. Entités HTML : aucune dans les fixtures ni, en sandbox le 2026-10-28, dans `texteHtml`, `content` et `nota` (quelques `&gt;` et `&nbsp;` dans des extraits de recherche) [V-94 · entites-html-legifrance].
- Caractères : NBSP U+00A0 (65 dans CONSTIT), apostrophe U+2019 (117), tiret U+2013 (`SAN–2026-003`), `\t` et `\n` dans les extraits ACCO, espaces finales dans des titres KALI, `\n` final dans `conteneurs[].titre`.

### 4.12 Chaînes vides, `null` et casse

- `""` au lieu d'absent : `liens[].{cidTexte, id, num, …}` (JURI), `nota`, `numLoiDef`, `citationJp` (CETAT), `textTitles[].nor`. Listes `null` : `textTitles[].appellations`.
- Casse : `nature` `arret`/`qpc`/`dc`/`texte`/`deliberation`/`code` en recherche contre `ARRET`/`QPC`/`Texte`/`Délibération` en consultation ; NOR en minuscules en recherche ; statut `etat` (`VIGUEUR`) contre `jurisState` (`Vigueur` LODA, `VIGUEUR_ETEN` KALI) ; `natureJuridiction` libellé ou code ; `formation` code (JURI) ou prose (CETAT).
- Types : `idcc`, `codeIdcc`, `sizeAttachment` sont des chaînes numériques (la référence typait `str | int`, E-14) ; `isMultipleVersions` (fixture) contre `multipleVersions` (Swagger, KALI) ; `signataires` objet (KALI) contre chaîne (`TexteSimple`).

### 4.13 Mojibake

- 6 fixtures sur 16 (search_loda, search_code, search_juri, consult_loda, consult_article, consult_juri) contiennent de l'UTF-8 décodé à tort en ISO-8859-1 : **252** séquences, chiffre de référence (235 dans 60 valeurs, 17 dans **14 clés** de facettes, toutes dans search_juri.json ; les « 235 séquences, 60 champs » de la recherche 02, D-35, ne comptent que les valeurs), **13** motifs (`Ã©` → `é` ×184, `Ã` + NBSP → `à` ×15, `Ã¨` → `è` ×15, `Â°` → `°` ×14, `Ãª` → `ê` ×8, `Ã\x89` → `É` ×6, `Ã\x88` → `È` ×3, `Ã\x87` → `Ç` ×2, et 5 motifs uniques), 12 caractères de contrôle C1 (recherche 03 §11.1 ; recompté pour cette annexe en `python3 -I`, Python 3.13, sur les 16 fixtures ; `docs/plan/annexes/donnees/scripts/dto_repair_check.py` répare exactement 60 valeurs et 14 clés). Les 6 fichiers n'ont aucun caractère accentué correct ni aucun caractère > U+00FF ; aucun motif Windows-1252 (`â€`, `Å“`).
- **Origine probable : la capture**, pas l'API : le même endpoint est sain dans une campagne ultérieure (§4.1) et pylegifrance filtre avec la chaîne correcte `Juridictions du premier degré` (recherche 03 §11.1 ; D-35). Vérifié en sandbox le 2026-10-28 : la capture, non PISTE, production non essayée [V-72 · mojibake-origine]. Les fixtures ne contiennent aucun échappement `\uXXXX` ; PISTE sert des caractères bruts, vérifié en sandbox le 2026-10-28 [V-71 · json-echappements-unicode].
- Traitement (00 §4, §8 ; 04 §10) : fonction pure du domaine `text::encoding::repair_latin1_mojibake`, appliquée par l'adapter au corps entier, tout-ou-rien, après le décodage UTF-8 strict, avant serde et **avant** toute normalisation des NBSP (`Ã` + U+00A0 est l'encodage de `à` : normaliser d'abord casse 10 champs, mesuré par `dto_repair_check.py`), active par défaut, désactivable par `LF_PISTE_MOJIBAKE_REPAIR=off`, comptée, journalisée en `warn`. Stratégies contrôlées sur les fixtures par `docs/plan/annexes/donnees/scripts/dto_repair_check.py` (empreinte SHA-256 de la version qui a produit ces mesures : `ef5c1c80c82a063a407ba9134d3afc3e5526bc659e5650810b38a3096d9d0530` ; Python 3.13, `python3 -I` ; 00 §10).

---

## 5. DTO serde minimaux

Consolidation de la recherche 03 §12, alignée sur la liste de champs fixée par 04 §8.3 (qui fait foi pour l'adapter). Les DTO sont `pub(crate)` dans `lf-adapter-piste::dto` ; le domaine ne voit jamais un type serde (00 §2).

### 5.1 Principes

1. **Un module par endpoint** (`dto::search`, `dto::text` pour lawDecree et kaliText, `dto::article`, `dto::juri` pour juri et cnil, `dto::acco`, `dto::oauth`, `dto::error`, puis `dto::m6`, `dto::m7`). Aucune struct partagée pour des homonymes de types différents (`signataires`).
2. **Tolérance** : jamais `deny_unknown_fields` ; on ne déclare que les champs lus. Serde parcourt sans allouer les champs non déclarés : `facets` et `acco.data` ne sont jamais déclarés.
3. **Tout est optionnel** : `Option<T>` + `#[serde(default)]`. Un champ à désérialiseur personnalisé **sans** `default` absent produit `missing field` : `default` est donc obligatoire avec `deserialize_with`. `Vec<T>` refuse `null` : toute liste passe par `null_default` (recherche 03 §12.1, d'après le code de serde).
4. **Dates brutes** `RawDate` dans le DTO, converties en `CivilDate` par le mapping (§4.9).
5. **`""` → `None`** par `opt_nonempty` là où `""` signifie absent ; jamais de `trim` du texte juridique.
6. **Renommages explicites** : `rename_all = "camelCase"` produirait `titresTm` alors que l'API envoie `titresTM` ; alias `isMultipleVersions`/`multipleVersions`.
7. **Erreurs lisibles** : `serde_path_to_error` ; `CorpusError::Decode` cite l'endpoint, le chemin JSON et la catégorie serde, jamais une valeur (04 §8.5).
8. **Avant serde** : décodage UTF-8 strict et réparation du mojibake (§4.13) ; corps lu en flux et borné à 16 Mio après décompression, `Decode` au-delà (00 §8 ; 04 §4.3).

### 5.2 Helpers (`dto/de.rs`)

> **Supersédé par 04 §8.2 (livré, plan 2d).** Ce bloc est l'esquisse de la recherche 03 : les helpers `opt_string_or_number` et `opt_u64_lenient` ne passent plus par un `enum` `untagged` (`SN`, `NS`), qui met toute la valeur en mémoire tampon et échoue dès 127 tableaux imbriqués, et `opt_raw_date` ne rend pas `Text(trim)` : ce sont des `Visitor` écrits à la main (`dto/de.rs`), qui gardent la chaîne telle quelle et ignorent un tableau ou un objet à toute profondeur. Le code de référence est celui du dépôt.

```rust
// non compilé, à valider au jalon qui l'implémente — d'après la recherche 03 §12.2 et 04 §8.2
use serde::{de::{self, IgnoredAny}, Deserialize, Deserializer};

/// `null` ou absent -> Default (listes, maps).
pub fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where D: Deserializer<'de>, T: Default + Deserialize<'de> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `null`, absent, "" ou blanc -> None ; sinon la chaîne telle quelle (pas de trim).
pub fn opt_nonempty<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.filter(|s| !s.trim().is_empty()))
}

/// Date telle que l'API l'envoie : entier ms (négatif compris), chaîne de chiffres,
/// "AAAA-MM-JJ" ou "AAAA-MM-JJTHH:MM:SS.sss+0000". Booléen, objet, flottant fractionnaire -> None.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawDate { Millis(i64), Text(String) }
pub fn opt_raw_date<'de, D: Deserializer<'de>>(d: D) -> Result<Option<RawDate>, D::Error>;
// Visiteur : visit_i64/visit_u64 -> Millis ; visit_f64 entier -> Millis ; visit_str : chiffres -> Millis,
// sinon Text(trim) ; vide -> None ; visit_bool/visit_map/visit_seq -> None (consommés, diagnostic).

/// Chaîne non vide ou entier -> Some(String) (idcc, codeIdcc, sizeAttachment) ; autre type -> None.
pub fn opt_string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)] #[serde(untagged)] enum SN { S(String), N(i64), Other(IgnoredAny) }
    Ok(match Option::<SN>::deserialize(d)? {
        Some(SN::S(s)) if !s.trim().is_empty() => Some(s),
        Some(SN::N(n)) => Some(n.to_string()),
        _ => None,
    })
}

/// Nombre ou chaîne de chiffres -> Some(u64) ; sinon None (expires_in, totaux).
pub fn opt_u64_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    #[derive(Deserialize)] #[serde(untagged)] enum NS { N(u64), S(String), Other(IgnoredAny) }
    Ok(match Option::<NS>::deserialize(d)? {
        Some(NS::N(n)) => Some(n),
        Some(NS::S(s)) => s.trim().parse().ok(),
        _ => None,
    })
}
```

La variante `Other(IgnoredAny)` rend les deux derniers helpers tolérants à un type inattendu (un booléen ferait sinon échouer l'`untagged`) ; c'est un ajout à la recherche 03, à confirmer à la compilation.

### 5.3 Requêtes

```rust
// non compilé, à valider au jalon qui l'implémente
#[derive(Serialize)] pub(crate) struct SearchRequestDto<'a> { pub fond: &'a str, pub recherche: RechercheDto<'a> }

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub(crate) struct RechercheDto<'a> {
    pub champs: Vec<ChampDto<'a>>,
    pub filtres: Vec<FiltreDto<'a>>,          // toujours sérialisé ([] par défaut, parité server.py:149)
    pub page_number: u32,                      // >= 1
    pub page_size: u8,                         // 1..=100
    pub operateur: Operateur,
    pub sort: &'a str,                         // "PERTINENCE"
    pub type_pagination: TypePagination,       // valeur reçue à la page précédente (04 §6.6)
}
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub(crate) struct ChampDto<'a> { pub type_champ: TypeChamp, pub criteres: Vec<CritereDto<'a>>, pub operateur: Operateur }
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub(crate) struct CritereDto<'a> { pub type_recherche: TypeRecherche, pub valeur: &'a str, pub operateur: Operateur }
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub(crate) struct FiltreDto<'a> {
    pub facette: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")] pub valeurs: Option<Vec<&'a str>>, // M6 : facette ECLI (§3.10)
    #[serde(skip_serializing_if = "Option::is_none")] pub single_date: Option<i64>,      // epoch ms, midi Paris (00 §8)
    #[serde(skip_serializing_if = "Option::is_none")] pub dates: Option<DatePeriodDto>,  // AAAA-MM-JJ
}
#[derive(Serialize)] pub(crate) struct DatePeriodDto { pub start: String, pub end: String }

#[derive(Serialize, Clone, Copy)] #[serde(rename_all = "SCREAMING_SNAKE_CASE")] pub(crate) enum Operateur { Et, Ou }
#[derive(Serialize, Clone, Copy)] #[serde(rename_all = "SCREAMING_SNAKE_CASE")] pub(crate) enum TypePagination { Defaut, Article }
#[derive(Serialize, Clone, Copy)] #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum TypeRecherche { UnDesMots, Exacte, TousLesMotsDansUnChamp, AucunDesMots, AucuneCorrespondanceACetteExpression }
#[derive(Serialize, Clone, Copy)] #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum TypeChamp { All, Title, Table, Nor, Num, AdvancedTexteId, NumDelib, NumDec, NumArticle, Article,
    Ministere, Visa, Notice, VisaNotice, TravauxPrep, Signature, Nota, NumAffaire, Abstrats, Resumes, Texte,
    Ecli, NumLoiDef, TypeDecision, NumeroInterne, RefPubli, ResumeCirc, TexteRef, TitreLoiDef, RaisonSociale,
    MotsCles, Idcc }   // enums complets (Swagger) ; le plan n'envoie que ALL et les champs de M6 (§3.3, §3.4)

// Consultations (corps exacts de la référence, client.py:187-208 ; M6/M7 : 04 §7.1)
#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub(crate) struct TextIdDateReq<'a> { pub text_id: &'a str, pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub nature: Option<&'a str> }  // lawDecree ; legi/tableMatieres ("CODE")
#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub(crate) struct TextIdReq<'a> { pub text_id: &'a str }   // juri, cnil
#[derive(Serialize)] pub(crate) struct IdReq<'a> { pub id: &'a str }        // getArticle, kaliText, acco, canonicalArticleVersion, kaliContIdcc
#[derive(Serialize)] pub(crate) struct IdNumReq<'a> { pub id: &'a str, pub num: &'a str }  // getArticleWithIdAndNum
#[derive(Serialize)] pub(crate) struct NorReq<'a> { pub nor: &'a str }      // getJoWithNor
#[derive(Serialize)] pub(crate) struct CidReq<'a> { pub cid: &'a str }      // getArticleByCid
#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub(crate) struct ArticleIdReq<'a> { pub article_id: &'a str } // relatedLinksArticle
#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub(crate) struct CodeListReq { pub page_number: u32, pub page_size: u8, pub sort: &'static str }
#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub(crate) struct IdEliOrAliasReq<'a> { pub id_eli_or_alias: &'a str } // getArticleWithIdEliOrAlias, eliAndAliasRedirectionTexte (M6)
#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub(crate) struct TextCidReq<'a> { pub text_cid: &'a str }  // consult/jorf, si retenu à l'ouverture de M7
// /suggest retiré (00 §8 révision 2) : pas de SuggestReq
```

Le passage `SCREAMING_SNAKE_CASE` donne exactement les valeurs du Swagger (`AucuneCorrespondanceACetteExpression` → `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION`, `AdvancedTexteId` → `ADVANCED_TEXTE_ID`) ; un test de sérialisation le verrouille (04 §6.10). Les variantes inutiles (`multiValeurs`, `proximite`, sous-critères, `secondSort`, `fromAdvancedRecherche`, `searchedString`, `fromSuggest`) ne sont pas déclarées ; `valeurs` ne sert qu'à la facette `ECLI` de M6 (jamais envoyé en v0.1, 04 §6.1).

### 5.4 Réponse `/search`

Tous les champs des DTO de réponse portent `#[serde(default)]`, omis dans les extraits pour la lisibilité. Variante plus compacte, à valider au jalon qui l'implémente : `#[derive(Default, Deserialize)]` avec `#[serde(default)]` au niveau du conteneur ; un champ absent prend alors la valeur du `Default` du conteneur, y compris s'il a un `deserialize_with` (comportement de serde_derive à confirmer par un test `missing_custom_field_is_default`).

```rust
// non compilé, à valider au jalon qui l'implémente
#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub(crate) struct SearchResponseDto {
    #[serde(deserialize_with = "de::null_default")] pub results: Vec<SearchResultDto>,
    #[serde(deserialize_with = "de::opt_u64_lenient")] pub total_result_number: Option<u64>,
    #[serde(deserialize_with = "de::opt_u64_lenient")] pub total_article_result_number: Option<u64>,
    pub type_pagination: Option<String>,
    // facets : NON déclaré (jusqu'à 88 % du corps, jamais renvoyé, 00 §7)
}
#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub(crate) struct SearchResultDto {
    #[serde(deserialize_with = "de::null_default")] pub titles: Vec<SearchTitleDto>,
    pub text: Option<String>, pub nature: Option<String>, pub origin: Option<String>,
    pub etat: Option<String>, pub solution: Option<String>,
    #[serde(deserialize_with = "de::opt_raw_date")] pub date: Option<RawDate>,
    #[serde(deserialize_with = "de::opt_raw_date")] pub date_publication: Option<RawDate>,
    #[serde(deserialize_with = "de::opt_raw_date")] pub date_signature: Option<RawDate>,
    pub nor: Option<String>, pub num: Option<String>,
    #[serde(deserialize_with = "de::opt_string_or_number")] pub idcc: Option<String>,
    pub raison_sociale: Option<String>,
    #[serde(deserialize_with = "de::null_default")] pub themes: Vec<String>,
    #[serde(deserialize_with = "de::null_default")] pub appellations: Vec<String>,
    #[serde(deserialize_with = "de::null_default")] pub resume_principal: Vec<String>,
    #[serde(deserialize_with = "de::null_default")] pub sections: Vec<SearchSectionDto>,
}
#[derive(Deserialize)] pub(crate) struct SearchTitleDto { pub id: Option<String>, pub cid: Option<String>, pub title: Option<String> }
#[derive(Deserialize)] pub(crate) struct SearchSectionDto {
    #[serde(deserialize_with = "de::opt_nonempty")] pub id: Option<String>,
    #[serde(deserialize_with = "de::opt_nonempty")] pub title: Option<String>,
    #[serde(deserialize_with = "de::null_default")] pub extracts: Vec<SearchExtractDto>,
}
#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub(crate) struct SearchExtractDto {
    #[serde(deserialize_with = "de::opt_nonempty")] pub id: Option<String>,    // LEGIARTI | KALIARTI
    #[serde(deserialize_with = "de::opt_nonempty")] pub num: Option<String>,   // non fiable en KALI
    #[serde(deserialize_with = "de::null_default")] pub values: Vec<String>,
    pub search_field_name: Option<String>,
}
```

Pour un usage ultérieur des facettes (complétions, hors plan en v0.1) : `FacetDto { facet_elem, field, values: BTreeMap<String, u64>, childs: BTreeMap<String, FacetDto> }`, toutes en `null_default` (recherche 03 §12.3).

### 5.5 Consultations v0.1

```rust
// non compilé, à valider au jalon qui l'implémente — #[serde(default)] partout ; RD = Option<RawDate> via de::opt_raw_date ;
// NE = Option<String> via de::opt_nonempty ; V<T> = Vec<T> via de::null_default
// dto/text.rs — /consult/lawDecree, /consult/kaliText (et /consult/legi/tableMatieres, dès M2 si V-96 l'exige)
pub(crate) struct ConsultTextDto {           // rename_all = "camelCase"
    id, cid, title, nor, nature, etat, juris_state: Option<String>, text_abroge: Option<bool>,
    date_parution, date_debut_version, date_fin_version: RD,
    num_parution, jorf_text: NE, appellations: V<String>,
    sections: V<TocSectionDto>, articles: V<TocArticleDto>,
    conteneurs: V<ConteneurDto>, numero_bo, origine_publi, type_texte: NE,          // KALI
}
pub(crate) struct TocSectionDto { id, title, etat: Option<String>, int_ordre: Option<i64>,
    date_debut, date_fin: RD, sections: V<TocSectionDto>, articles: V<TocArticleDto> }   // récursif
pub(crate) struct TocArticleDto { id, num, etat, content: Option<String>, int_ordre: Option<i64>,
    date_debut, date_fin: RD, lst_lien_citation, lst_lien_modification: V<LienArticleDto> }
pub(crate) struct LienArticleDto { link_type, link_orientation, text_title, nature_text: Option<String>,
    text_cid, article_id, article_num: NE }
pub(crate) struct ConteneurDto { cid: Option<String>, num: NE /* IDCC */, titre: Option<String> }

// dto/article.rs — /consult/getArticle
pub(crate) struct GetArticleResponseDto { article: Option<ArticleDto> }
pub(crate) struct ArticleDto {               // rename_all = "camelCase"
    id, cid, num, etat, texte, texte_html, full_sections_titre, section_parent_titre: Option<String>,
    nota, nota_html, version_precedente: NE, date_debut, date_fin: RD,
    context: Option<ArticleContextDto>, text_titles: V<TextTitleDto>,
    #[serde(alias = "multipleVersions")] is_multiple_versions: Option<bool>,
    article_versions: V<ArticleVersionDto>,   // M7, déclaré par le Swagger
}
pub(crate) struct ArticleContextDto { #[serde(rename = "titresTM")] titres_tm: V<ContextEntryDto>,
    #[serde(rename = "titreTxt")] titre_txt: V<ContextEntryDto> }
pub(crate) struct ContextEntryDto { id, titre: Option<String>, debut, fin: RD }
pub(crate) struct TextTitleDto { id, cid, titre, nature: Option<String> }    // nature "CODE" -> titre du code

// dto/juri.rs — /consult/juri et /consult/cnil
pub(crate) struct ConsultJuriResponseDto { text: Option<JuriTextDto> }
pub(crate) struct JuriTextDto {              // rename_all = "camelCase"
    id, origine, nature, titre, titre_long, juridiction, formation, solution, texte, texte_html: Option<String>,
    num, ecli, citation_jp, publication_recueil, type_publication_bulletin: NE,
    numero_affaire: V<String>, sommaire: V<SommaireDto>, liens: V<TexteLienDto>, titrages: V<String>,
    date_texte, date_texte_computed, date_publi, date_publi_computed: RD,
    decision_attaquee: Option<DecisionAttaqueeDto>,
    nature_qualifiee: Option<String>, titre_loi_def, id_texte_jo, titre_jo: NE,   // CONSTIT
    nature_delib, etat: Option<String>,                                           // CNIL
    ancien_id: NE,
    // jamais déclarés : president, rapporteur, avocatGl, avocats, commissaire (§1.7.5)
}
pub(crate) struct SommaireDto { id: Option<String>, resume_principal, abstrats, autre_resume: NE }
pub(crate) struct TexteLienDto { type_lien, sens, title: Option<String>, cid_texte, id, num, nature_texte: NE }
pub(crate) struct DecisionAttaqueeDto { formation: Option<String>, date: RD }

// dto/acco.rs — /consult/acco ; `data` (base64 .docx) volontairement NON déclaré
pub(crate) struct ConsultAccoResponseDto { acco: Option<AccordDto> }
pub(crate) struct AccordDto {                // rename_all = "camelCase"
    id, nature, titre_texte, raison_sociale, secteur, file_size: Option<String>,
    siret, code_ape, numero: NE, #[serde(deserialize_with = "de::opt_string_or_number")] code_idcc: Option<String>,
    themes, syndicats: V<LibelleDto>, conforme_version_integrale: Option<bool>,
    date_texte, date_effet, date_depot, date_diffusion, date_fin: RD,
}
pub(crate) struct LibelleDto { code, libelle: Option<String> }
```

Notation abrégée pour la lisibilité : `id, cid: Option<String>` signifie un champ `Option<String>` par nom. Les `lienCitations`/`lienModifications` de `getArticle` ne sont pas déclarés : les liens d'articles viennent de `/consult/relatedLinksArticle` en M7.

### 5.6 DTO M6 et M7

| DTO | Endpoint | Champs |
|---|---|---|
| `ConsultJorfDto` | `/consult/getJoWithNor`, `/consult/eliAndAliasRedirectionTexte` (M6) ; `/consult/jorf` si retenu en M7 | `id`, `cid`, `title`, `nor`, `nature`, `textNumber` ; `dateTexte`, `dateParution` (RD) ; `hasLoda: bool` ; `eli` (NE, jamais fabriqué) |
| `GetArticleResponseDto` (réutilisé) | `/consult/getArticleWithIdEliOrAlias` (M6), `/consult/getArticleWithIdAndNum` (M6) | §5.5 |
| `KaliContDto` | `/consult/kaliContIdcc` | `id` (KALICONT), `titre`, `num`, `numeroTexte`, `nature`, `texteBaseId: V<String>`, `sections: V<TocSectionDto>` |
| `CodeListResponseDto` | `/list/code` | `totalResultNumber` (`opt_u64_lenient`), `results: V<CodeListResultDto { id, cid, titre, etat, dateDebut, dateFin, lastUpdate }>` |
| `CanonicalVersionsDto` | `/search/canonicalArticleVersion` | `articleVersions: V<ArticleVersionDto { id, version, numero, etat, dateDebut, dateFin (RD), ordre: i64 }>` |
| `GetListArticleResponseDto` | `/consult/getArticleByCid` | `listArticle: V<ArticleDto>` |
| `RelatedLinksDto` | `/consult/relatedLinksArticle` | `liensCite`, `liensCitePar: V<LienRelatifDto { id, cidText, cidParent, nature, name, dateVigeur (RD) }>` |
| `ConsultTextDto` (réutilisé) | `/consult/legi/tableMatieres` (M2, conditionnel : V-96) | §5.5 |
| `TokenResponseDto`, `OAuthErrorDto` | endpoint token | 04 §3.2 |
| `PisteErrorDto` | corps d'erreur | `error` (`opt_u64_lenient`), `status` (forme variable), `message`, `errorCode` (04 §11.3) |

`/suggest` étant retiré (00 §8 révision 2), le `SuggestResponseDto` de la version initiale de cette annexe (`totalResultNumber`, `results: BTreeMap<String, SuggestValueDto { id, idTexte, section, label, nature, origin, dateVersion (RD), appellations: V<String> }>` en `null_default`) n'est plus prévu ; il ne serait repris que par un amendement de 00 §3 et §8.

### 5.7 Mapping et tests

Le mapping DTO → entités (`SearchHit`, `SearchPage`, `Act`, `TocNode` (arbre ; le livré a `TocEntry`, renommé `TocArticle` en tête de M2), `ArticleText`, `Decision`, `Deliberation`, `Convention`, `CompanyAgreement`) est fixé par 04 §9 et 03-domaine §4. Tests propres aux DTO, détaillés dans 04 §13.3 : `dto_parses_every_fixture`, `null_list_is_empty_vec`, `missing_list_is_empty_vec`, `empty_string_is_none`, `raw_date_accepts_ms_numeric_string_ymd_iso`, `sentinel_2999_and_2222_are_none`, `negative_ms_1804`, `bool_is_not_a_date`, `suffix_is_version_date`, `titres_tm_is_renamed`, `multiple_versions_alias`, `idcc_accepts_string_and_number`, `search_request_matches_reference_json`, `facets_are_skipped_without_building_a_value`, `acco_large_data_field_is_skipped`, fuzz « jamais de panique ». Limite de récursion de `serde_json` : 127 tableaux et objets imbriqués passent, soit 62 ou 63 niveaux de sections selon le contenu de la plus profonde (V-10 levé, plan 2d, 09 §2.20 ; 04 §8.5).

---

## 6. Judilibre

### 6.1 Apport par rapport au fonds JURI

| Apport | Détail | Source |
|---|---|---|
| Zonage | `zones {introduction, expose, moyens, motivations, dispositif, annexes}` = listes de fragments `{start, end}` (offsets dans `text`, fin exclue, pas forcément séquentiels) : permettrait un `focus` natif (« motivations » + « dispositif ») | `[JDL]/README.md:72-160` (recherche 05 §5.3) |
| Rapprochements identifiés | `rapprochements[{id: "JURITEXT000036780011", date, title, url, number}]`, `visa[{title}]` (textes appliqués), `contested {…}`, `files[]`, `titlesAndSummaries[]`, `themes[]` : des liens entre décisions **avec identifiants**, ce que `/consult/juri` ne fournit pas (§4.5) | fixture `[JDL]/src/data/sample_detail_resolved.json` |
| Taxonomies | `jurisdiction` ∈ {cc, ca, tj, tcom, cph} ; `field` ∈ {themes, text, introduction, expose, moyens, motivations, dispositif, annexes, visa, summary} ; `sort` ∈ {score, scorepub (défaut), date} ; `publication` ∈ {b, r, l, c, n} | `[JDL]/src/taxons/*.js` |
| Synchronisation | `/transactionalhistory` + `/scan` | §6.3 |
| Couverture | Cour de cassation et juridictions judiciaires (cours d'appel en civil, social et commercial depuis mars 2022, selon le calendrier de l'arrêté du 28 avril 2021) ; ni Conseil d'État ni juridictions administratives selon la taxonomie (un tiers affirme le contraire : **NON VÉRIFIÉ** [V-183 · judilibre-couverture]) | `[JDL]/README.md:5-7` |

### 6.2 Accès et souscription

- Bases : sandbox `https://sandbox-api.piste.gouv.fr/cassation/judilibre/v1.0`, production `https://api.piste.gouv.fr/cassation/judilibre/v1.0` (`[JDL]/README.md:41-44`). VÉRIFIÉ.
- Souscription PISTE **distincte** de Légifrance : valider les CGU « Judilibre », puis cocher l'API dans l'application (`[JDL]/README.md:26-35`). Constat live de la référence : jeton Légifrance valide → **403** sur `/healthcheck`, `/search`, `/stats` ; la demande de scope `cassation`/`judilibre` est ignorée ; `KeyId` ou absence d'authentification → **400** (référence SOURCES.md:12, DISCOVERY.md:89-93).
- Authentification : l'OpenAPI 3.0.2 déclare un en-tête **`KeyId`** (`securitySchemes.access {type: apiKey, name: KeyId, in: header}`) ; les clients tiers utilisent un jeton OAuth `Bearer` PISTE (recherche 05 §5.1). Les deux semblent acceptés : **NON VÉRIFIÉ** [V-182 · judilibre-auth-keyid].
- L'API se déclare « version bêta […] susceptible d'évoluer » (`[JDL]/README.md`).

### 6.3 Endpoints (OpenAPI 3.0.2, v1.2.5)

| Endpoint | Paramètres principaux | Notes |
|---|---|---|
| `GET /search` | `query`, `field[]`, `operator` (or, and, exact), `type[]`, `theme[]`, `chamber[]`, `formation[]`, `jurisdiction[]`, `location[]`, `publication[]`, `solution[]`, `date_start`, `date_end`, `sort`, `order`, `page_size` (≤ 50, défaut 10), `page` (base **0**), `resolve_references` | pas de texte intégral dans les résultats |
| `GET /decision` | `id`, `resolve_references`, `query`, `operator` | texte, zones, titrages, sommaire, visas, rapprochements |
| `GET /taxonomy` | `id`, `key`, `value`, `context_value` | référentiels |
| `GET /stats` | `jurisdiction`, `location`, `date_start`, `date_end`, `keys` | statistiques |
| `GET /export` | filtres + `date_type`, `order`, `batch_size` (≤ 1 000), `batch`, `abridged` | « limité par défaut (pour une connexion non authentifiée) à 10 résultats par lot, pour un maximum de 1000 résultats au total » |
| `GET /scan` | idem + `source[]`, `search_after` | export par curseur, sans limite de fenêtre |
| `GET /transactionalhistory` | `date`, `page_size`, `from_id` | journal create/update/delete ; querystring de suite valable 1 min |
| `GET /healthcheck` | — | 403 si non abonné |

### 6.4 Limites

- `/search` : « nécessairement paginé (avec un maximum de 50 résultats par page, pour un maximum de 10 000 résultats au total) » ; contrôle `page * page_size + page_size > 10000` (`[JDL]/src/api/search.js:271`).
- `/export` : 416 au-delà de 10 000 par partition (tiers, `[JL]/export_piste_v2.py:110-111`) ; un tiers attend 60 s sur 429.
- Quotas « prochainement sur le portail PISTE » (`[JDL]/README.md`) : valeurs inconnues, comme pour Légifrance [V-62 · quota-piste-prod].

### 6.5 Conditions

Licence Ouverte 2.0, CGU PISTE, et « conditions générales d'utilisation pour la réutilisation des données issues des décisions de justice de l'ordre judiciaire diffusées en open data par la Cour de cassation » (`[JDL]/README.md:15-22`). Ces dernières (version du 6 janvier 2022, `CGU_open_data_V8.pdf`) n'ont pas été lues : **NON VÉRIFIÉ** [V-181 · cgu-judilibre]. Les décisions sont pseudonymisées (art. R. 111-10 COJ, d'après un résumé). L'interdiction de profilage des magistrats s'applique (§1.7.5).

### 6.6 Position du plan

Hors périmètre jusqu'à décision explicite : aucun crate `lf-adapter-judilibre` n'existe dans 00 §2, aucun outil ne l'utilise, et le fonds JURI couvre déjà la Cour de cassation. Une adoption demanderait un ADR, un amendement de 00 §2, §3 et §8 (nouvelle source derrière `LegalCorpus` ou port de jurisprudence enrichie), une seconde souscription par utilisateur, et une détection propre de l'absence d'abonnement (403 → fonctionnalité désactivée, jamais une erreur bloquante). Déclencheurs à réévaluer après v0.3.0 : besoin d'un `focus` par zone de décision, ou de `fr_get_related` avec des rapprochements identifiés plutôt qu'analysés.

---

## 7. Dumps open data DILA (M9)

Base de `lf-adapter-dila` (00 §2 : `tar` + `flate2`, `quick-xml`, index `tantivy` avec analyseur français ; ADR-0004). Les hôtes de la DILA étaient injoignables pendant la recherche : les faits ci-dessous viennent de la documentation DILA archivée, de résumés web et du code de projets tiers concordants (recherche 05 §6). L'ouverture réseau d'`echanges.dila.gouv.fr` est l'objet de Q6.

### 7.1 Bases

- Serveur : `https://echanges.dila.gouv.fr/OPENDATA/<FONDS>/` (et `ftps://echanges.dila.gouv.fr`), DTD dans `/OPENDATA/DTD_LEGIFRANCE/`, contact `donnees-dila@dila.gouv.fr`. URL citée par la DILA elle-même (doc « exemples » l.1333).
- Fonds listés par la DILA : « ACCO / BOCC / CAPP / CASS / CIRCULAIRES / CNIL / CONSTIT / DOLE / Debats / INCA / JADE / KALI / LEGI / Questions-Reponses » (l.1337) ; `JORF` existe aussi (`/OPENDATA/JORF/`, utilisé par justicelibre). Chaque fonds a un document de présentation ; les DTD sont partagées entre bases.

| Fonds DILA | Contenu | `Fond` du domaine |
|---|---|---|
| LEGI | codes, lois et règlements consolidés | `Loda`, `Code` |
| JORF | textes publiés au JO | `Jorf` |
| KALI | conventions collectives nationales | `Kali` |
| CASS | décisions publiées de la Cour de cassation | `Juri` |
| INCA | décisions inédites de la Cour de cassation | `Juri` |
| CAPP | cours d'appel et premières instances judiciaires | `Juri` |
| CONSTIT | Conseil constitutionnel | `Constit` |
| JADE | juridictions administratives : CE et 9 CAA seulement ; les TA ne sont diffusés que par opendata.justice-administrative.fr (tiers, `[JL]/download_opendata.py:4-8`), alors que le fonds CETAT de l'API contient des TA | `Cetat` (partiel) |
| CNIL | délibérations CNIL | `Cnil` |
| ACCO | accords d'entreprise | `Acco` |
| CIRCULAIRES, DOLE, BOCC, Debats, Questions-Reponses | circulaires, dossiers législatifs, bulletins, débats, questions | hors plan |

### 7.2 Licence

Licence Ouverte (Etalab) 2.0 pour LEGI (`[LCX]/README.md:85-88` ; data.gouv.fr d'après un résumé). Base juridique de la gratuité : arrêté du 24 juin 2014 (JORF, LEGI, KALI, JADE, CONSTIT, CASS-INCA, CAPP, CNIL, CIRCULAIRES). La mise à disposition ne dispense pas de respecter la loi Informatique et Libertés. Licence exacte de chaque fonds : **NON VÉRIFIÉ** [V-189 · dila-licence-par-fonds]. Attribution recommandée : « Source : DILA — base LEGI, Légifrance », avec la date du dernier incrément appliqué (§1.7.4).

### 7.3 Nommage et cadence

| Archive | Nom | Source |
|---|---|---|
| Global (dump complet) | `Freemium_legi_global_AAAAMMJJ-HHMMSS.tar.gz` ; autres fonds `Freemium_<fonds>_global_AAAAMMJJ-HHMMSS.tar.gz` | `[LCX]/scripts/fetch.py`, `[JL]/scripts/dila_reingest_all.sh:58,71` |
| Incrément | `LEGI_AAAAMMJJ-HHMMSS.tar.gz` ; autres fonds `<FONDS>_AAAAMMJJ-HHMMSS.tar.gz` | idem |
| Expression de legi.py | `(.+_)?legi(?P<global>_global)?_(?P<date>[0-9]{8}-[0-9]{6})\..+` (insensible à la casse) | `[LEGIPY]/legi/tar2sqlite.py:596` |

- **Dernier global** : `Freemium_legi_global_20250713-140000.tar.gz` (13/07/2025), complété d'incréments quotidiens jusqu'en octobre 2026 au moins (`LEGI_20261010-211641`) (recherche 05 §6.2). Le rythme annoncé (« environ deux fois par an ») n'est pas tenu depuis 15 mois : **NON VÉRIFIÉ** [V-186 · dila-rythme-globaux].
- **Incréments** : « En général la DILA publie une nouvelle archive à la fin de chaque jour ouvré » (`[LEGIPY]/README.md:61-65`) ; heures variables ; archives vides (< 200 octets) possibles (`[JL]/dila_catchup.sh:45-49`).
- Téléchargement : listing HTML de l'index, reprise par `Range` (206, 200, 416 gérés), réessais (`[LEGIPY]/legi/download.py:46-100`) ; un tiers découpe le global en morceaux de 64 Mio, au plus 4 en parallèle (`[LCX]/scripts/fetch.py`).

### 7.4 Ordre d'application et suppressions

1. Appliquer le **global**, puis **tous** les incréments postérieurs dans l'ordre chronologique strict. Sinon les textes reviennent à l'état du global (`[JL]/scripts/dila_reingest_all.sh:5-10` : « le GLOBAL du 13/07/2025, PUIS tous les deltas quotidiens dans l'ordre chronologique »).
2. Un global plus récent remplace tout : legi.py **recrée la base** (`tar2sqlite.py:604-616`).
3. Exception tierce : pour LEGI, une clé `(legiarti, date_debut)` rend l'ordre indifférent (`dila_reingest_all.sh:9-10`) ; à ne pas généraliser aux autres fonds.
4. **Suppressions** : fichiers `liste_suppression_legi.dat` (chemins séparés par des espaces) et `liste_suppression_legi_dossier.dat` (séparateur `" D\n"`) dans les archives (`tar2sqlite.py:243-252`). Ils s'appliquent **après** le contenu de l'archive (`tar2sqlite.py:541-544`). Un tiers gère aussi la suppression d'un répertoire JORFTEXT entier (`[LCX]/scripts/ingest.py`, fonction `suppress`).
5. Doublons : un même identifiant peut apparaître dans `en_vigueur` et `non_vigueur` ; legi.py garde le plus récent par `mtime` et trace les doublons (`tar2sqlite.py:265-322`).
6. Chemins tar parfois en octets non UTF-8 : décodage latin-1 (`tar2sqlite.py:237-239`).

### 7.5 Arborescence LEGI dans l'archive

```text
[<préfixe>/]legi/global/code_et_TNC_(en|non)_vigueur/(code|TNC)_(en|non)_vigueur/
    AAAA/BB/CC/DD/EE/FF/GG/<CID de 20 caractères>/                 (parts[11] = cid)
        (article | section_ta | texte/struct | texte/version)/       (parts[13])
            AAAA/BB/CC/DD/EE/FF/GG/<ID de 20 caractères>.xml
```

`TNC` = textes non codifiés (`tar2sqlite.py:62-71, 253-262`). Un tiers ne garde que `code_et_TNC_en_vigueur` et supprime les textes passés en `non_vigueur` (`[LCX]/scripts/ingest.py`). Vérification sur une archive réelle : **NON VÉRIFIÉ** [V-187 · dila-structure-archives].

### 7.6 Balises XML LEGI

| Racine | Contenu | Balises clés |
|---|---|---|
| `ARTICLE` (LEGIARTI) | une **version** d'article | `META/META_COMMUN/{ID, NATURE, ANCIEN_ID}` ; `META/META_SPEC/META_ARTICLE/{NUM, ETAT, DATE_DEBUT, DATE_FIN, TYPE}` ; `CONTEXTE/TEXTE@cid @nature` (+ `TITRE_TXT`, `TM/TITRE_TM@id`, chaîne des sections) ; `BLOC_TEXTUEL/CONTENU` (HTML) ; `NOTA/CONTENU` ; `LIENS/LIEN` |
| `SECTION_TA` (LEGISCTA) | section, sans `META_COMMUN` | `ID`, `TITRE_TA`, `COMMENTAIRE`, `CONTEXTE/TEXTE@cid`, `STRUCTURE_TA/(LIEN_SECTION_TA \| LIEN_ART)@id @debut @fin @etat @num` |
| `TEXTELR` (texte/struct) | structure d'un texte (sans titre) | `META/META_COMMUN/ID`, `VERSIONS`, `STRUCT/(LIEN_SECTION_TA \| LIEN_ART)@id @debut @fin @etat` |
| `TEXTE_VERSION` (texte/version) | version d'un texte | `META_TEXTE_CHRONICLE/{CID, NUM, NUM_SEQUENCE, NOR, DATE_PUBLI, DATE_TEXTE, DERNIERE_MODIFICATION, ORIGINE_PUBLI, PAGE_DEB_PUBLI, PAGE_FIN_PUBLI, NUM_PARUTION}` ; `META_TEXTE_VERSION/{TITRE, TITREFULL, ETAT, DATE_DEBUT, DATE_FIN, AUTORITE, MINISTERE, LIENS}` ; `VISAS`, `SIGNATAIRES`, `TP`, `NOTA`, `ABRO`, `RECT` (chacun avec `CONTENU`) |

Sources : `[LEGIPY]/legi/tar2sqlite.py:28-39, 329-442` ; `[LCX]/scripts/ingest.py` ; `[JL]/parse_dila_bulk.py:307-310, 349-426`.

**Liens** (`LIENS/LIEN`) : attributs `typelien`, `sens` (`source` | `cible`), `id`, `cidtexte` ; texte = titre de la cible. Quand `sens="cible"`, legi.py inverse source et destination ainsi que le type (`MODIFICATION`↔`MODIFIE`, `ABROGATION`↔`ABROGE`, `CODIFICATION`↔`CODIFIE`, `CONCORDANCE`↔`CONCORDE`, `CREATION`↔`CREE`, `TRANSFERE`↔`TRANSFERT`, `PEREMPTION`↔`PERIME`, `RATIFICATION`↔`RATIFIE`, `ANNULATION`↔`ANNULE`, `DISJOINT`↔`DISJONCTION`, `DEPLACE`↔`DEPLACEMENT`) (`tar2sqlite.py:47-60, 417-442`). C'est l'équivalent hors ligne de `/consult/relatedLinksArticle` et de `lienModifications`.

**Autres fonds** (code tiers récent, `[JL]/parse_dila_bulk.py`, vérifié par son auteur sur `CASS_20260824` et `KALI_20260907-214720`) :

- Jurisprudence (CASS, CAPP, INCA, JADE, CONSTIT) : métadonnées sur **deux** conteneurs, `META_JURI` (`TITRE`, `DATE_DEC`, `JURIDICTION`, `SOLUTION`, `NUMERO`) et `META_JURI_JUDI` (`ECLI`, `FORMATION`, `PRESIDENT`, `RAPPORTEUR`, `AVOCAT_GL` et non `AVOCAT_GENERAL`, `NUMEROS_AFFAIRES/NUMERO_AFFAIRE`, `PUBLI_BULL@publie`), ou `META_JURI_ADMIN` (`COMMISSAIRE_GVT`, `TYPE_REC`, `PUBLI_RECUEIL` A/B/C), ou `META_JURI_CONSTIT` (`NATURE_QUALIFIEE`, `LOI_DEF@num @date`, `ECLI`, `NOR`, `TITRE_JO`, `URL_CC`) ; `SOMMAIRE/SCT@TYPE` (PRINCIPAL, REFERENCE), `SOMMAIRE/ANA` ; `CITATION_JP`, `RAPPROCHEMENTS` ; `LIENS/LIEN` ; `SAISINES`.
- KALI : `KALICONT` (racine `IDCC`) → `META_SPEC/META_CONTENEUR/{TITRE, ETAT, NUM (= IDCC), DATE_PUBLI}` + `STRUCTURE_TXT/TM/LIEN_TXT` ; `KALITEXT` (racine `TEXTEKALI` ou `TEXTE_VERSION`) ; `KALIARTI` (racine `ARTICLE`, `META_ARTICLE/{NUM, TITRE, ETAT, DATE_DEBUT, DATE_FIN, DATE_DEB_EXT, DATE_FIN_EXT}`, `CONTEXTE/CONTENEUR@num @titre @etat @date_publi`) ; `KALISCTA` (`SECTION_TA`). Les balises `META_TEXTE_KALI` et `META_CONVENTION_COLLECTIVE` n'existent pas.
- Encodage déclaré des fichiers XML : **NON VÉRIFIÉ** [V-188 · dila-encodage-xml] ; utiliser un parseur qui respecte la déclaration XML (feature `encoding` de quick-xml).

### 7.7 Règle de vigueur à une date D

- Version en vigueur à la date D : **`debut ≤ D < fin`** sur chaque lien de structure (`LIEN_SECTION_TA`, `LIEN_ART`), avec `fin` par défaut `2999-01-01` (`[LCX]/scripts/build.py`, `Builder.in_window`). Archéo Lex calcule les dates de versions d'un texte comme l'union des `debut`/`fin` de ses éléments (`[ALEX]/marcheolex/exporterlegi.py:266-374`).
- Le même type de date que l'API (00 §4) : `2999-01-01` → `None` (fin ouverte), `2222-02-22` → `None` avec l'état `VIGUEUR_DIFF` qui porte l'information (03-domaine §6.2).

### 7.8 Dates spéciales et anomalies

- `2999-01-01` = pas de date de fin (`[ALEX]/marcheolex/exporterlegi.py:172-182` ; `[LEGIPY]/legi/anomalies.py:23`) ; **`2222-02-22`** = « entrée en vigueur différée à une date non-déterminée » (`[ALEX]/doc/limitations.md`).
- Articles **mort-nés** (`MODIFIE_MORT_NE`) : fin antérieure au début, jamais en vigueur (`[ALEX]/doc/limitations.md` ; lexique DILA l.125). La règle `debut ≤ D < fin` les exclut naturellement.
- Anomalies connues : contenus d'article manquants, articles rattachés à une mauvaise section, différences entre la base diffusée et Légifrance (`[ALEX]/doc/limitations.md`) ; 236 liens de structure sur environ 377 000 pointent vers des versions d'articles absentes (`[LCX]/README.md`) ; incohérences d'état et de dates, éléments de sommaire introuvables, orphelins, doublons de numéros (`[LEGIPY]/legi/anomalies.py:10-119`).
- `CID` commun à toutes les versions, mais aucun identifiant ne relie réellement les versions d'un même texte : legi.py « factorise » par (nature, numéro), NOR et titre normalisé (`[LEGIPY]/README.md:78-81`).
- HTML : nettoyer les espaces, attributs `id`/`dir`, `span` vides et **césures molles U+00AD** ; 93 millions de caractères inutiles mesurés en 2020 (`[LEGIPY]/README.md:83-95`).
- `TYPE` d'article : AUTONOME, entièrement modificateur (sans état ni dates), partiellement modificateur (lexique DILA l.275-301).

### 7.9 Volumétrie

| Mesure | Valeur | Source | Statut |
|---|---|---|---|
| SQLite legi.py, LEGI complet toutes versions | ≈ 5 Go (2026) | `[LEGIPY]/README.md:56` | tiers |
| LEGI en vigueur au 2026-10-11 | 76 codes ; 165 279 articles de codes en vigueur ; ARRETE 69 708, DECRET 44 813, LOI 2 978, ORDONNANCE 1 320 textes ; 176 093 articles de décrets | `[LCX]/scripts/stats-*.json` | tiers |
| justicelibre (2026) | `legi.db` 3,8 Go (1 780 693 versions d'articles) ; `jorf.db` 2,3 Go ; `jade.db` 14 Go (570 063 décisions) ; `kali.db` 1,3 Go ; `cnil.db` 147 Mo ; `judiciaire.db` ≈ 21 Go (1 213 903 décisions) | `[JL]/ARCHITECTURE.md:29-56` | tiers |
| mcp-fr-legal (codes et lois en vigueur) | 3 953 documents, 193 681 dispositions, ≈ 303 Mo | `[MFL]/README.md:10-22` | tiers |
| Archive LEGI | ≈ 745-796 Mo compressée (anciens instantanés), 9-15 Go extraite (2017) ; « environ 5 Gio » selon Archéo Lex | résumés web ; `[ALEX]/README.md` | **NON VÉRIFIÉ** [V-185 · dila-volumetrie-archives] |

### 7.10 Leçons des projets existants

| Projet (licence) | Approche | Leçons à retenir |
|---|---|---|
| legi.py (Legilibre, CC0) | libarchive en flux → SQLite ; incrémental ; normalisation, factorisation, nettoyage HTML, détection d'anomalies | lecture en flux sans extraction ; `mtime` pour sauter l'inchangé ; doublons entre dossiers ; listes de suppression ; recréation sur nouveau global ; écritures non journalisées pendant l'import initial puis WAL (`tar2sqlite.py:557-560, 631-660`) |
| Archéo Lex (WTFPL, inactif depuis 2019) | une version Git par date de changement | dates sentinelles ; mort-nés ; articles manquants ; erreurs de rattachement de section |
| LawCodex/fr (2026, licence du code non relevée ; données LEGI sous Licence Ouverte 2.0) | `tarfile r\|gz` en flux → SQLite → Markdown à date | sélection `debut ≤ D < fin` ; ne garder que `en_vigueur` ; exclure les arrêtés pour la taille ; mesurer les liens cassés |
| justicelibre (MIT, 2026) | 9 fonds DILA + Judilibre → SQLite FTS5 | registre des archives appliquées ; fenêtre de rattrapage de 10 jours ; verrou d'ingestion ; garde-fou disque ; « UPSERT ne remplaçant que par une valeur non vide » ; vrais pièges de balises (KALI, `AVOCAT_GL`, `META_JURI_JUDI`) |
| mcp-fr-legal (MIT, 2026) | SQLite FTS5 en lecture seule, citations verbatim, validation « fail-closed » | provenance (`snapshot`, `age_days`, licence) dans chaque réponse ; avertissement si l'instantané a plus de 365 jours |

Aucun de ces projets n'est sous AGPL à notre connaissance (la licence du code de LawCodex/fr n'a pas été relevée). Seules leurs idées sont reprises ; le code Rust est écrit de zéro (00 §12).

### 7.11 Conséquences pour `lf-adapter-dila`

1. **Ingestion en flux** : `flate2::read::GzDecoder` → `tar::Archive::entries()` → `quick_xml::Reader` sur chaque entrée ; jamais d'extraction sur disque (des millions de petits fichiers). Versions de référence relevées le 2026-10-15, pas des planchers : `quick-xml` 0.42.0, `tantivy` 0.26.2 (stemmer `Language::French`, mots vides français, `ascii_folding_filter`), `tar` 0.4.46, `flate2` 1.1.10 (recherche 05 §6.7). L'exigence est écrite au niveau mineur dans `[workspace.dependencies]` et la version verrouillée suit la règle d'âge de 00 §2 ; ces crates ne sont déclarés dans `lf-adapter-dila` qu'avec le code d'ingestion qui les emploie (M9), jamais d'avance dans le squelette (00 §2).
2. **Registre des archives appliquées** (nom, type, date, statut) ; ordre chronologique strict ; reconstruction complète sur nouveau global ; archives vides tolérées ; rattrapage automatique des échecs.
3. **Suppressions** appliquées après le contenu de chaque archive.
4. **Stockage** : index `tantivy` (00 §2), clé de version `(id, date_debut)`, documents textes, versions, sections, articles, liens de structure (`debut`, `fin`, `etat`, `num`, position), liens typés et orientés. La recherche 05 propose en plus SQLite comme stockage de vérité : écart au §9.
5. **Même port** `LegalCorpus` que l'adapter PISTE, sélection par `enum AnyCorpus` (00 §3) ; chaque réponse signale sa **fraîcheur** (date du dernier incrément appliqué, âge en jours) et sa source (`dila-dump` contre API), avec l'attribution Licence Ouverte.
6. **Robustesse des balises** : tests de non-régression sur des extraits XML réels, un par racine et par fonds ; ne pas supposer un conteneur unique (`META_JURI` contre `META_JURI_*`) ; ne jamais écraser une valeur par une valeur vide.
7. **Écarts avec l'API** : la base diffusée peut différer de Légifrance (§7.8) ; pas d'indexation publique des contenus `inap` (§1.7.5).
8. **Politesse réseau** : au plus 4 connexions, reprise par `Range`, User-Agent identifiant sur le modèle de celui de l'adapter PISTE (00 §8).

---

## 8. Points NON VÉRIFIÉS

| Étiquette | Vérification à faire |
|---|---|
| [V-59 · oauth-production-live] | L'endpoint token et la base de production répondent comme documenté avec une application de production. |
| [V-60 · oauth-reponses] | Statuts et corps réels des erreurs de l'endpoint token (dont le statut, 400 ou 401, de `invalid_client`) ; `expires_in` et `token_type` réels. |
| [V-61 · sandbox-prod-donnees] | Ampleur des écarts de données entre sandbox et production. |
| [V-74 · swagger-version-servie] | Version du Swagger servie aujourd'hui par PISTE, comparée à la copie 2.4.2 ; existence des chemins `consult/loda/version(s)` et `consult/juri/*`. |
| [V-62 · quota-piste-prod] | Valeurs des quotas (sandbox, production, Judilibre), granularité (application ou méthode, requêtes ou bande passante). |
| [V-63 · retry-after-piste] | Présence et format de `Retry-After` sur 429 et 503. |
| [V-64 · 401-sous-throttling] | `401 invalid_client` pendant une limitation de débit. |
| [V-65 · forme-403-piste] | Sens et corps d'un 403 de l'API Légifrance. |
| [V-66 · forme-404-piste] | Forme d'un 404 PISTE, de routage ou de ressource. |
| [V-67 · corps-erreur-piste] | Forme générale des corps d'erreur de l'API. |
| [V-68 · introuvable-par-endpoint] | Réponse de chaque consultation à un identifiant bien formé mais inexistant (dont `CETATEXT` et `CONSTEXT` sur `/consult/juri` : seul `JURITEXT` a été sondé en direct). |
| [V-69 · 500-requete-invalide] | Une facette ou un champ inconnu provoque-t-il un 500 plutôt qu'un 400 ? Fréquence des 500 transitoires (retour à 3 tentatives sur 500 si elle est élevée, 00 §8). |
| [V-84 · fond-all-comportement] | Comportement de `fond=ALL` (avec filtres, filtres de date, mélange, tri). |
| [V-70 · piste-gzip] | Compression des réponses de l'API. |
| [V-175 · cgu-piste-generales] | CGU générales du portail PISTE : partage de compte, application mutualisée, cache. |
| [V-108 · date-maj-par-document] | Sémantique de `modifDate`, `dateMaj`, `lastUpdate`, `refInjection` comme « date de dernière mise à jour » (Licence Ouverte) ; condition d'un champ `last_update?` par document (00 §12, Q15). |
| [V-177 · attribution-licence-ouverte] | Une mention globale (`fr_coverage`, instructions, README, `NOTICE`) suffit-elle à l'attribution exigée par la Licence Ouverte 2.0, ou faut-il une mention dans chaque réponse ? (étiquette de 09, V-177) |
| [V-179 · profilage-magistrats-article] | Article exact de l'interdiction de profilage des magistrats. |
| [V-180 · dereferenced-inap-obligation] | Obligations attachées à `dereferenced = true` et `inap = true`. |
| [V-184 · contenu-fonds-hors-plan] | Contenu réel du fonds JUFI. |
| [V-79 · mode-all-semantique] | Sens de `TOUS_LES_MOTS_DANS_UN_CHAMP` avec `typeChamp = ALL` et rappel sur les scénarios de la référence (conditionne Q10). |
| [V-80 · syntaxe-moteur-valeur] | Syntaxe interne du moteur dans `valeur` (guillemets, jokers, `*`). |
| [V-81 · longueur-max-query] | Longueur maximale acceptée pour `valeur`. |
| [V-77 · formats-dates-entree] | `singleDate` en chaîne ; fuseau attendu pour l'entier ms ; formats acceptés pour `dates {start, end}`. |
| [V-76 · singledate-midi-paris] | Un texte entrant en vigueur le jour J est-il retenu avec `singleDate` = midi Europe/Paris du jour J (règle de 00 §8 révision 2), et la version précédente le jour J-1 ? Comparaison avec minuit UTC, minuit Paris et la veille (V-76, bloquante en M2 ; étiquette de référence depuis le contrôle final du 2026-10-16, 09 §2.1). |
| [V-78 · filtres-dates-effet] | Effet réel des facettes de date (total réduit, bornes incluses, combinaison avec `DATE_VERSION`). |
| [V-86 · facette-casse-valeurs] | Filtre `valeur` au singulier ; sensibilité à la casse des valeurs (`ARRET` contre `arret`). |
| [V-129 · facettes-fermees-filtre] | Les valeurs de facettes fermées (`NATURE`, `CASSATION_FORMATION`, `JURIDICTION_NATURE`) sont-elles acceptées en filtre ? (étiquette de l'annexe B) |
| [V-128 · ecli-filtre-juri-cetat] | Effet du filtre `ECLI` (JURI, CETAT), adopté en M6 par 00 §8, et du `typeChamp = ECLI` (étiquette de l'annexe B). |
| [V-133 · eli-redirection-endpoints] | Comportement de `getArticleWithIdEliOrAlias` et `eliAndAliasRedirectionTexte`, adoptés en M6 par 00 §8 (étiquette de l'annexe B). |
| [V-85 · tris-par-fond] | Tris valides par fonds (CODE notamment) et liste des `secondSort`. |
| [V-121 · nom-code-filtre] | `NOM_CODE` / `TEXT_NOM_CODE` avec un titre exact : résultat ou 500 ? |
| [V-82 · typepagination-page2] | Effet de `DEFAUT` contre `ARTICLE` en page 2 (CODE) et cohérence de `has_more`. |
| [V-83 · pagination-profonde] | Fenêtre de pagination profonde de `/search` (10 000 ?) et comportement au-delà. |
| [V-120 · recherche-exacte-par-numero] | Comportement des recherches `EXACTE` par numéro (casse, tirets, zéros de tête). |
| [V-119 · num-article-forme-api] | Forme de `num` attendue par `getArticleWithIdAndNum` et `NUM_ARTICLE` (`L5-8` ou `5-8`, `1er`, suffixes latins). |
| [V-118 · idandnum-comportement] | Réponse de `getArticleWithIdAndNum` (article sans version en vigueur, numéro inexistant). |
| [V-125 · list-code-sort] | Valeurs acceptées par `/list/code.sort` (`TITLE_ASC`, `CODE_DATE_MAJ`). |
| [V-122 · endpoints-m6-reponses] | Formes réelles de `getJoWithNor`, `kaliContIdcc` et `/list/code`. |
| [V-135 · endpoints-m7-reponses] | Formes et tailles réelles de `tableMatieres`, `canonicalArticleVersion`, `getArticleByCid`, `relatedLinksArticle` et, pour la surface JORF, `/consult/jorf` (`/suggest` est retiré). |
| [V-136 · related-links-forme] | Forme et taille de `relatedLinksArticle` ; prise en charge de `KALIARTI`. |
| [V-87 · date-hit-semantique] | Sens du champ `date` des hits par fonds. |
| [V-88 · kali-extraits-num] | Fiabilité du `num` des extraits KALI. |
| [V-116 · publication-bulletin-semantique] | Sens de `typePublicationBulletin` `T`/`F` et de `publicationRecueil`. |
| [V-117 · liens-juri-fragmentes] | Cause des titres fragmentés et des identifiants vides de `liens[]` JURI (API ou capture). |
| [V-112 · cetat-num-requete] | `num` CETAT est-il toujours le numéro de requête ? |
| [V-102 · eli-natif-null] | `eli`, `idEli`, `idEliAlias` sont-ils toujours nuls en production (LODA, articles) ? |
| [V-97 · intordre-lawdecree] | Présence d'`intOrdre` dans les réponses `lawDecree` réelles. |
| [V-98 · toc-ordre-sans-intordre] | Ordre réel des sections et articles sans `intOrdre`. |
| [V-95 · lawdecree-content-articles] | Présence du `content` HTML des articles dans `lawDecree`. |
| [V-96 · lawdecree-sur-code] | Comportement, taille et durée de `lawDecree` sur un `LEGITEXT` de code, comparés aux seuils de 4 Mio et 10 s (V-96, bloquante en M2). |
| [V-90 · texte-html-getarticle-juri] | Présence de `texteHtml` sur `getArticle` et sur `/consult/juri` pour JURI. |
| [V-107 · getarticle-champs-elagues] | Présence de `articleVersions`, `lienCitations`, `lienModifications`, `lienConcordes`, `notaHtml` sur `getArticle`. |
| [V-92 · texthtml-vs-texte-complet] | Seuil de complétude de `texteHtml` par rapport à `texte`. |
| [V-93 · balises-html-autres] | Autres balises HTML présentes dans les textes réels. |
| [V-94 · entites-html-legifrance] | Présence d'entités HTML dans les données. |
| [V-106 · acco-data-attachment] | Présence, contenu et taille de `data` et `attachment` sur `/consult/acco`. |
| [V-75 · taille-max-textes] | Tailles maximales réelles des réponses (textes, codes, TOC) face à la borne de 16 Mio. |
| [V-72 · mojibake-origine] | Le mojibake vient-il de la capture ou de l'API ? Présence en production. |
| [V-71 · json-echappements-unicode] | PISTE sert-il les caractères non ASCII bruts ou échappés (`\uXXXX`) ? |
| [V-100 · convention-horaire-timestamps] | Tous les horodatages de date sont-ils à minuit UTC ? |
| [V-101 · sentinelle-2222-debut] | Usage de `2222-02-22` par l'API et lien avec `VIGUEUR_DIFF`. |
| [V-103 · id-format-hors-fixtures] | Préfixe de 8 lettres + 12 chiffres pour tous les identifiants réels. |
| [V-104 · suffixe-version-autres-prefixes] | Suffixe `_JJ-MM-AAAA` sur d'autres préfixes que `LEGITEXT`. |
| [V-10 · serde-json-recursion] | Levé (plan 2d, 09 §2.20) : 127 imbrications passent avec `serde_json`, 62 ou 63 niveaux de sections. |
| [V-183 · judilibre-couverture] | Couverture réelle de Judilibre (juridictions administratives ?). |
| [V-182 · judilibre-auth-keyid] | `KeyId` ou `Bearer` pour Judilibre, selon les endpoints. |
| [V-181 · cgu-judilibre] | CGU open data de la Cour de cassation (V8) : obligations précises. |
| [V-189 · dila-licence-par-fonds] | Licence exacte et document de présentation de chaque fonds DILA. |
| [V-186 · dila-rythme-globaux] | Rythme de publication des globaux (dernier : 13/07/2025). |
| [V-187 · dila-structure-archives] | Arborescence et balises sur une archive réelle de chaque fonds visé en M9. |
| [V-188 · dila-encodage-xml] | Encodage déclaré des fichiers XML et DTD actuelles. |
| [V-185 · dila-volumetrie-archives] | Taille compressée et extraite actuelle du global et des incréments. |

---

## 9. Écarts entre sources et décisions

Chaque fois, la décision de 00 révision 2 (ou, à défaut, de 04) est appliquée dans cette annexe. Les lignes 2, 9, 11, 12, 17, 20 et 21 décrivent des écarts tranchés par la révision 2 (la ligne 10 l'est à l'ouverture de M7) ; elles sont gardées pour la traçabilité.

| # | Écart | Décision appliquée | Recommandation |
|---|---|---|---|
| 1 | La recherche 05 §3.4 propose des variantes d'erreur `NotSubscribed` (403), `BadRequestSuspected` (500 répété), `Transport` | `CorpusError` de 00 §5 (AM-28) : 403 → `Auth`, rendu `config_error` avec le message « check that the PISTE application is subscribed to the Legifrance API » ; 400 sans marqueur → `InvalidRequest`, rendu `upstream_error` avec le détail PISTE nettoyé (consultation ; générique pour `/search`, 04 §11.2) ; 500 répété → `Upstream { status: 500, retryable: false }` ; réseau → `Upstream { status: None, retryable: true }` | appliqué (00 §5) |
| 2 | La recherche 05 §3.4 et §2.2 recommande de ne jamais réessayer plus d'une fois un 500 dû à la requête ; la version initiale de 00 §8 prévoyait 3 tentatives sur 500 | 00 §8 révision 2 (AM-03) : 2 tentatives au plus sur 500, puis `Upstream { status: 500, retryable: false }` ; 3 sur 429, 502, 503, 504 et réseau | réévaluer après [V-69 · 500-requete-invalide] (V-69) : retour à 3 tentatives si les 500 transitoires sont fréquents (04 §16 question 3, tranchée sous cette réserve) |
| 3 | La recherche 05 §6.7 propose `2222-02-22 → DateDifferee` ; la recherche 03 §11.2 propose `LegalDate::Open` | 00 §4 (AM-01) : toute date ≥ 2222-01-01 → `None`, champ omis en sortie, sens documenté par champ (date de début omise : « date d'entrée en vigueur non déterminée ») ; l'état `VIGUEUR_DIFF` porte l'information (V-101) | garder 00 |
| 4 | La recherche 03 §12.3 déclare `facets: Option<Box<RawValue>>` | 04 §8.1 : facettes non déclarées (00 §7 : jamais renvoyées) | garder 04 ; `FacetDto` seulement si des complétions de facettes sont décidées |
| 5 | La recherche 05 §1.13 suggère `fond=ALL` pour `fr_search_all` | 00 §7 : une requête par fond, bornée par le limiteur | garder 00 ; `ALL` est en outre réputé fragile avec filtres |
| 6 | La recherche 05 §6.7 propose SQLite (rusqlite) comme stockage de vérité plus tantivy | 00 §2 : `lf-adapter-dila` dépend de `tar`, `flate2`, `quick-xml`, `tantivy` | tantivy seul (champs stockés) pour v1.0 ; réévaluer par ADR en M9 si les mises à jour incrémentales ou la taille l'exigent |
| 7 | La recherche 05 §7 nomme le port `LegalSource` et lui ajoute suggest, listes JO, TOC par section | 00 §3 : port `LegalCorpus` et méthodes listées, sans méthode `suggest` (`/suggest` retiré, 00 §8) | garder 00 ; candidats au §2.10 |
| 8 | La recherche 05 §1.13 évoque des outils `fr_get_toc`, `fr_status` et des ressources `legifrance://codes`, `legifrance://jorf/latest` | 00 §7 : 13 outils et 5 ressources nommés | non repris |
| 9 | La recherche 05 §1.13 propose pour `fr_resolve` des endpoints ELI, `ancienId`, `chrono`, `typeChamp=ECLI` | 00 §8 révision 2 (AM-29) : ELI cités (`getArticleWithIdEliOrAlias`, `eliAndAliasRedirectionTexte`) et facette `ECLI` (JURI, CETAT) adoptés en M6 ; `ancienId`, `chrono` et `typeChamp=ECLI` non retenus | `ancienId` et `chrono` restent candidats au §2.10 (question 4 du §10) ; vérifier [V-128 · ecli-filtre-juri-cetat] et [V-133 · eli-redirection-endpoints] (questions 2 et 3 du §10) |
| 10 | 00 §4 ajoute `Fond::Jorf` en M7 et la recherche JORF, mais aucun endpoint de consultation d'un `JORFTEXT` | 00 §4 et §8 révision 2 (AM-29) : consultation des hits `Jorf` (`/consult/jorf`, ou hits limités à `citation` + `source_url`) et surface JORF tranchées à l'ouverture de M7 (V-135) ; d'ici là, `fond` a 8 valeurs | ajouter `/consult/jorf`, avec bascule vers LODA si `hasLoda = true` (§10 question 1) |
| 11 | La version initiale de 00 §8 listait `/suggest` en M7 sans méthode de port en 00 §3 | 00 §8 révision 2 (AM-29) : `/suggest` retiré (aucun consommateur) ; pas de méthode `suggest` (00 §3) | résolu (04 §16 question 7, tranchée) |
| 12 | La version initiale de 00 §8 fixait `singleDate` à minuit Europe/Paris ; tous les horodatages observés sont à minuit UTC | 00 §8 révision 2 (AM-02) : midi Europe/Paris du jour, vecteurs fixés (§3.6, §3.10) | confirmer par V-76, bloquante en M2 ([V-76 · singledate-midi-paris] ; 04 §16 question 1, tranchée) |
| 13 | Le document officiel inverse les exemples de `/consult/kaliCont` et `/consult/kaliContIdcc` par rapport au Swagger | Swagger | — |
| 14 | Le document officiel utilise `NOM_CODE` ; deux tiers le disent cause de 500 | 00 §8 et 04 §7.2 : jamais `NOM_CODE` sans V-121 ; `find_article` filtre les hits par `cid` | vérifier [V-121 · nom-code-filtre] |
| 15 | Swagger : `lstLienCitation` « Toujours vide » ; fixture KALI : rempli | lu quand présent | — |
| 16 | Swagger `multipleVersions` ; fixture `isMultipleVersions` | `alias` | — |
| 17 | Borne de taille du corps : 16 Mio dans 04 (§0, §2.3, §4.3), « proposition : 32 Mio » dans la version initiale de [02-architecture.md](../02-architecture.md) §3.3 | 16 Mio après décompression (00 §8, AM-06) | résolu : 02 §3.3 est aligné sur 16 Mio |
| 18 | Étiquettes en double dans le plan : `v:quota-piste-prod` (02, 05, annexes A et B) et `v:quotas-piste` (04) ; `v:origine-mojibake` (02) et `v:mojibake-origine` (03, 04, annexes A et B) | cette annexe emploie les étiquettes majoritaires `v:quota-piste-prod` et `v:mojibake-origine` | fusionnées dans le registre de 09 (V-62, V-72) |
| 19 | Un client tiers affirme que sandbox et production ont les mêmes données | Swagger officiel : données non identiques | — |
| 20 | 00 §3 donne `find_article(&CodeRef, &ArticleNum, VersionDate)`, mais `/consult/getArticleWithIdAndNum` n'a pas de paramètre de date et ne sert que la version en vigueur (Swagger) | 00 §8 révision 2 (AM-29) : `getArticleWithIdAndNum` pour la version en vigueur seulement ; autre date : `/search` `EXACTE` sur `NUM_ARTICLE` + `DATE_VERSION`, hits filtrés par `cid` (§3.10) | vérifier [V-118 · idandnum-comportement] et [V-120 · recherche-exacte-par-numero] |
| 21 | `fr_get_act` sur un code est servi dès v0.1 (`fr_search` CODE renvoie des `LEGITEXT` de codes), alors que la version initiale de 00 §8 plaçait `/consult/legi/tableMatieres` en M7 | 00 §8 révision 2 (AM-29) : `tableMatieres` (nature `CODE`) en v0.1 si V-96, bloquante en M2, montre que `lawDecree` sur un code dépasse 4 Mio ou 10 s | reconnaître un code avant l'appel : 04 §7.3, §16 question 13 |
| 22 | La version initiale de cette annexe donnait pour l'endpoint token la forme `401 {"error":"invalid_client"}`, alors que la référence ne relève que la valeur `invalid_client` (SOURCES.md:12) | statut et corps exacts non relevés ; 400 et 401 traités de même (04 §3.5) | vérifier [V-60 · oauth-reponses] |
| 23 | La version initiale de cette annexe attribuait le 400 « L'expression à valider est fausse » de `/consult/juri` à une sonde live portant sur `JURITEXT` et `CETATEXT` ; pylegifrance n'a sondé que des `JURITEXT` (le cas `CETATEXT` vient d'une docstring de test unitaire sur réponse simulée) | §1.5 corrigé ; même règle de classification (marqueur) pour tous les préfixes (04 §11.1, §11.2) | vérifier [V-68 · introuvable-par-endpoint] |

---

## 10. Questions ouvertes propres à l'annexe

Elles sont reportées dans [09-risques-et-questions.md](../09-risques-et-questions.md). Les questions communes Q1 à Q17 de 00 §13 restent la référence (Q16 : typographie des citations, décidée par le plan 1 ; Q17 : URL `/codes/article_lc/` d'un article de loi, à vérifier en M2 par V-140 à V-142 ; Q3 conditionne les vérifications live et les fixtures de production, Q4 le mode HTTP, Q6 l'accès aux dumps, Q10 le mode de recherche par défaut, Q15 l'attribution). La numérotation ci-dessous est conservée, car d'autres documents la citent ; les questions 2, 3 et 5 sont tranchées ou adoptées par défaut par la révision 2 de 00.

1. **Consultation JORF en M7** — *à trancher à l'ouverture de M7* (00 §4, §8, AM-29 ; V-135) : ajouter `/consult/jorf` à 00 §8 pour consulter les hits `Fond::Jorf`, ou limiter ces hits à `citation` + `source_url` ? (Proposition maintenue : ajouter, avec bascule vers LODA quand `hasLoda = true`.)
2. **Résolution par ECLI** (`fr_resolve`, M6) — *tranchée* (00 §8, AM-29) : facette `ECLI` (JURI, CETAT) en M6, effet à vérifier [V-128 · ecli-filtre-juri-cetat] (V-128) ; repli par `ALL` en `EXACTE` puis `typeChamp = ECLI` (enum du Swagger, utilisé par pylegifrance, absent du document officiel) à sonder (V-127) ; même question posée par l'annexe [B](B-citations.md).
3. **Résolution d'un ELI cité** — *tranchée* (00 §8, AM-29) : `/consult/getArticleWithIdEliOrAlias` et `/consult/eliAndAliasRedirectionTexte` en M6, en lecture seule, sans changer la règle « jamais d'ELI fabriqué » ; comportement [V-133 · eli-redirection-endpoints] (V-133).
4. **Anciens identifiants** (`getJuriWithAncienId`, `getCnilWithAncienId`, `getCodeWithAncienId`) : utiles au corpus de citations de l'annexe B ? (Proposition : à décider en M6 selon le corpus.)
5. **Date de mise à jour** pour l'attribution Licence Ouverte — *adoptée par défaut, à confirmer par l'utilisateur* (Q15, AM-39 ; 00 §12) : « Source : Légifrance / DILA — Licence Ouverte 2.0 » et la date de consultation (date du jour, Europe/Paris) dans `fr_coverage` (`attribution`, `consulted_on`) et les instructions serveur, rappel dans le README ; un champ `last_update?` par document (`modifDate`, `dateMaj`, `refInjection`…) n'est ajouté (classe « ajout ») que lorsqu'un champ fiable est identifié ([V-108 · date-maj-par-document], V-108). Suffisance d'une mention globale : [V-177 · attribution-licence-ouverte] (V-177).
6. **Copie du Swagger dans le dépôt** : versionner seulement la liste des chemins (04 §7.1) ou tout le fichier, dont la licence de redistribution n'est pas explicite ? (Proposition : liste des chemins et des définitions utilisées seulement.)
7. **Périmètre de M9** : quels fonds DILA en v1.0 (LEGI seul, ou aussi JORF, KALI, CASS, CONSTIT, JADE, CNIL) et quel budget disque (justicelibre : plus de 40 Go pour tout) ? (Proposition : LEGI d'abord.)
8. **Contenus `dereferenced` et `inap`** : les exclure de tout index et de toute réponse en mode HTTP distant ? (Proposition : oui par défaut.)
9. **Judilibre** : faut-il ouvrir un ADR après v0.3.0 si le besoin de zonage ou de rapprochements identifiés se confirme ? (Proposition : réévaluer après v0.3.0, §6.6.)

---

## Sources

**Plan et décisions**
- [00-decisions.md](../00-decisions.md) (normatif) : §1 (variables `LF_*`), §2 (crates, dépendances de `lf-adapter-dila`, versions de référence et règle d'âge), §3 (port `LegalCorpus`), §4 (identifiants, dates, sentinelles, contrat de citation), §5 (erreurs), §6 (démarrage sans réseau, HTTP), §7 (outils, facettes jamais brutes), §8 (adapter PISTE, liste fermée des endpoints, révision 2), §9 (cache), §10 (scripts versionnés dans `docs/plan/annexes/donnees/scripts/`), §11 (jalons), §12 (licence, attribution), §13 (Q1 à Q17), §14 (journal des amendements AM-01…AM-43).
- [04-adapter-piste.md](../04-adapter-piste.md) : §2 (environnements, validation des URL), §3 (OAuth), §4.3 (borne de 16 Mio), §5 (résilience), §6 (requêtes `/search`, facettes de date, `typePagination`, recherches exactes), §7 (endpoints par jalon, test de dérive), §8 (DTO), §9 (mapping), §10 (mojibake), §11 (introuvable, corps d'erreur), §13 (tests), §16 (questions).
- [03-domaine.md](../03-domaine.md) : §3.1 (`Fond`, `api_name`), §6 (dates, sentinelles) ; [02-architecture.md](../02-architecture.md) §3.3 (borne de corps, alignée sur 16 Mio) ; [05-outils-mcp.md](../05-outils-mcp.md) §4 et §5.8 (attribution dans les instructions, champs `attribution` et `consulted_on` de `fr_coverage`).
- Scripts versionnés (`docs/plan/annexes/donnees/scripts/`, Python 3.13, `python3 -I`) : `dto_repair_check.py` (`ef5c1c80c82a063a407ba9134d3afc3e5526bc659e5650810b38a3096d9d0530`), relancé pour cette annexe sur les 16 fixtures (§4.13).

**Rapports de recherche** (`docs/recherche/`)
- [05-piste-dila.md](../../recherche/05-piste-dila.md), lu en entier : §0 (sources, identité des Swagger sandbox et prod), §1 (catalogue, pièges de données), §2 (recherche, facettes, tris, pagination), §3 (OAuth, environnements, quotas, codes d'erreur), §4 (CGU), §5 (Judilibre), §6 (dumps DILA), §7, §8.
- [03-dto.md](../../recherche/03-dto.md), lu en entier : §2 (fixtures et campagnes), §3 (payloads), §4 (enveloppe et résultats de recherche), §5 (facettes), §6 à §10 (consultations), §11 (mojibake, dates, identifiants, HTML, liens, casse), §12 (DTO, helpers, mapping), §13 (économie de contexte), §14, §15.
- [01-parite.md](../../recherche/01-parite.md) : P-020 à P-026, P-036 ; E-04, E-10, E-14, E-20, E-35. [02-defauts.md](../../recherche/02-defauts.md) : D-10, D-12, D-20, D-25, D-26, D-28, D-35, D-45, D-47.

**Swagger officiel** : Swagger 2.0 de l'API Légifrance, `info.version = 2.4.2`, host `api.piste.gouv.fr`, basePath `/dila/legifrance/lf-engine-app`, copie embarquée par `rdassignies/pylegifrance@d8e0e59` (MIT), fichier `pylegifrance/models/generated/legifrance.json`. Relu pour cette annexe par trois scripts en `python3 -I` (2026-10-15) : liste des 68 opérations (tag, méthode, corps, réponse, codes déclarés), champs requis et exemples de chaque corps, définitions des réponses utilisées (`ConsultTextResponse`, `ConsultSection`, `ConsultArticle`, `Article`, `TexteSimple`, `ConsultKaliTextResponse`, `Accord`, `Attachment`, `ConsultJorfResponse`, `ConsultKaliContResponse`, `CodeListResponse`, `ArticleVersion`, `LiensRelatifsDTO`, `SuggestResponse`, `SuggestValue`, `TexteLien`, `LienCitation`, `LienModification`), descriptions de chemins (`getArticleWithIdAndNum`, `legi/tableMatieres`, `kaliContIdcc`), `info.description`, `info.license`, `securityDefinitions`, `tags` ; relu pour la révision 2 : corps `ArticleIdEliOrAliasRequest` et `JorfConsultWithIdEliAliasRequest`, réponses de `eliAndAliasRedirectionTexte` et `/consult/jorf`, propriétés de `FiltreDTO`, enum `CodeListRequest.states` (17 valeurs).

**Documents DILA archivés** (copies dans `pylegifrance/docs/raw/legifrance/`, même commit) : `exemples-d-utilisation-de-l-api.md` (V3 du 17/09/2025 : OAuth l.105-163, `NUM_ARTICLE` l.497-570, `*` l.1063, `lastNJo` l.1187, NOR l.1209, JO avant 2004 l.1223, `fromAdvancedRecherche` l.1263, OPENDATA l.1333-1337) ; `cgu-legifrance-api.md` (v1.0 du 15/12/2022 : l.94-111, l.150-231, l.235-285, l.289-365, l.367-402, relues) ; `description-des-tris-et-filtres-de-l-api.md` (feuilles ALL, JORF, CODE relues l.22-82) ; `lexique-api-lgf.md` ; `cgu-piste-legifrance-beta.md`.

**Référence Python** (`/home/user/matematicsolutions/fr-eli-mcp`, commit `d3b5b13`, lecture seule) : `src/fr_eli_mcp/client.py:28-35` (URLs sandbox, délais, statuts réessayés, marge de jeton), `:42-58` (variables `FR_ELI_*`, message d'identifiants), `:150-208` (boucle de réessai, 401, 7 endpoints) ; `DISCOVERY.md:3-44` (sondes live, ELI nul, ECLI natif), `:63-93` (CONSTIT, CETAT, Judilibre 403) ; `SOURCES.md:8-15` (Judilibre rejeté, identifiants sandbox, ACCO). Fixtures `tests/fixtures/*.json` (16 fichiers), relues par script pour cette annexe : `typePagination` et `totalArticleResultNumber` de search_code.json, totaux et noms de facettes des 7 autres recherches, champs de consult_article.json, consult_kali.json, consult_loda.json, consult_juri*.json, consult_acco.json.

**Projets tiers** (clones en lecture seule de la recherche 05) : `rdassignies/pylegifrance@d8e0e59` (MIT ; relus pour le 400 de `/consult/juri` : `pylegifrance/fonds/juri.py:124-135`, `tests/integration/fonds/juri/test_verification_endpoints.py:52-59`, `tests/unit/fonds/test_juri_verification_endpoints.py:105-121`) ; `Cour-de-cassation/judilibre-search@ddfc166` (`README.md`, `public/JUDILIBRE-public.json` relu : v1.2.5, 8 chemins, `KeyId`, `page_size` ≤ 50, `page` base 0) ; `Legilibre/legi.py@bc68599` (CC0 ; `legi/tar2sqlite.py` relu : l.63, 79, 230-262, 541-544, 596, 604) ; `Dahliyaal/justicelibre@c2440ab` (MIT ; `scripts/dila_reingest_all.sh:1-12` relu) ; `LawCodex/fr@a1681ad` ; `Legilibre/Archeo-Lex@f35c3bc` ; `matematicsolutions/mcp-fr-legal@3a0ad0f` ; `tantivy@e1acb3e`. Les projets AGPL (`Ktulu-Analog/juridix`, `Ktulu-Analog/mcp-legifrance`) ne sont cités que pour des constats, sans reprise de code.

**Web** (résumés de la recherche 05, pages injoignables directement) : https://www.legifrance.gouv.fr/contenu/pied-de-page/open-data-et-api ; https://www.data.gouv.fr/dataservices/legifrance/discussions ; https://www.etalab.gouv.fr/wp-content/uploads/2017/04/ETALAB-Licence-Ouverte-v2.0.pdf ; https://www.data.gouv.fr/datasets/legi-codes-lois-et-reglements-consolides ; https://piste.gouv.fr/images/cgu/CGU_open_data_V8.pdf.
