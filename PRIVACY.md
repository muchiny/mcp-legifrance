# Confidentialité

Dernière mise à jour : 2026-11-27.

mcp-legifrance est un serveur MCP qui tourne **sur votre poste**. Il interroge l'API Légifrance du portail PISTE avec **vos** identifiants et, pour les deux outils de doctrine fiscale (`fr_search_bofip`, `fr_get_bofip`, depuis la 1.3.0), le portail de données ouvertes du ministère de l'Économie et, pour les deux outils de fiches pratiques (`fr_search_service_public`, `fr_get_service_public`, depuis la 1.5.0), le serveur de la DILA qui publie les archives de Service-Public et, pour les deux outils de décisions de justice (`fr_search_judilibre`, `fr_get_judilibre`, pas encore publiés), l'API Judilibre de la Cour de cassation, par le même portail PISTE. Il ne parle à personne d'autre. Il n'a ni compte, ni télémétrie, ni service tiers à lui : le projet ne reçoit aucune de vos données. Cette page dit précisément ce qui part, ce qui reste sur votre poste, et ce qui n'est pas vérifié (`docs/plan/08-distribution.md` §1.4 et §9.8). Elle est la politique de confidentialité référencée par `privacy_policies` du bundle MCPB.

Ce produit n'est ni édité, ni approuvé, ni soutenu par la DILA, ni par la DGFiP, ni par la Cour de cassation.

## Données envoyées

### Vers PISTE (treize outils Légifrance)

Elles partent vers PISTE (DILA, AIFE), par HTTPS, et seulement quand un outil est appelé :

- **Vos identifiants PISTE** (`LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET`) vont à l'hôte OAuth de PISTE pour obtenir un jeton. Le jeton accompagne ensuite chaque appel à l'API.
- **Le contenu de vos requêtes** : mots de recherche, filtres, identifiants de textes et de décisions, codes, numéros d'articles et dates. Pour les outils de résolution et de vérification de citations (`fr_resolve` et `fr_verify_citations`), le texte que vous soumettez est analysé sur votre poste ; seules les références qui en sont extraites (code, numéro d'article, date, juridiction, numéro de décision) sont envoyées, pas le texte lui-même.
- **Les en-têtes de toute requête HTTPS** : votre adresse IP, vue par PISTE comme pour n'importe quel client, et un `User-Agent` de la forme `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-legifrance)`.

Ce que vous cherchez peut révéler votre activité. PISTE et la DILA voient les requêtes faites avec votre application ; le projet, lui, ne les voit pas. D'après les CGU de PISTE, la DILA collecte l'adresse électronique du compte PISTE et des statistiques agrégées d'usage, conservées 12 mois au plus ; leur contact est `rgpd@dila.gouv.fr`.

### Vers le portail du BOFiP (`fr_search_bofip`, `fr_get_bofip`)

Ces deux outils lisent le jeu ouvert `bofip-vigueur` de la DGFiP sur `data.economie.gouv.fr`, par HTTPS, et seulement quand l'un d'eux est appelé. Les douze autres outils, `fr_coverage`, `check-config` et le démarrage n'y envoient rien.

- **Ce qui part** : les mots de votre recherche, la série et la nature demandées, le numéro de page ; pour une lecture, l'identifiant de la publication (`BOI-…`). Une recherche envoie une requête, deux quand aucune publication ne porte tous vos mots tels quels : la seconde, en mots approchants, porte les mêmes valeurs. Ces valeurs sont dans l'adresse de la requête : elles sont chiffrées pendant le transport, mais le portail les reçoit en clair et peut les écrire dans ses journaux, comme tout serveur web.
- **Ce qui ne part pas** : vos identifiants PISTE, un compte, un cookie, un `Referer`. Avec les deux en-têtes techniques `Host` et `Accept-Encoding: gzip`, la requête ne porte que `Accept` et le `User-Agent` `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-legifrance)` (liste contrôlée par un test).
- **Votre adresse IP**, vue par le portail comme pour n'importe quel client.
- **Qui reçoit ces requêtes.** Le portail est géré par le secrétariat général du ministère ; il est créé et hébergé par la société Opendatasoft (conditions générales du portail, lues le 2026-11-15). **Ni le pays d'hébergement, ni la durée de conservation des journaux ne sont publiés** ; la politique de confidentialité du portail cite une « journalisation des connexions », sans détail pour un appel d'API (V-223). Ce document ne peut donc pas vous dire combien de temps vos mots de recherche y sont gardés.
- **Pourquoi c'est sensible.** Une recherche fiscale (« donation », « plus-value résidence secondaire », « redressement ») peut révéler votre situation, et elle est associée à votre adresse IP.
- **Couper cette source** : `LF_BOFIP=off` dans l'environnement du serveur (bloc `env` de votre client). Dans le bundle MCPB, qui n'a pas de bloc `env` à modifier, c'est le réglage « Tax doctrine source (BOFiP) » des paramètres de l'extension : `on` par défaut, écrivez-y `off` pour couper la source. Les deux outils répondent alors `[config_error]` sans aucune requête (vérifié par un test du binaire, avec un proxy témoin). Toute autre valeur que `on` ou `off` coupe aussi la source ; seule exception, une variable vide ou blanche compte comme absente, comme toutes les variables du serveur : la source reste alors active. En mode hors ligne (`LF_DILA_DIR` défini), elle est coupée d'office et ne revient que par `LF_BOFIP=on`.
- **Site du BOFiP.** `bofip.impots.gouv.fr` n'est jamais contacté par le serveur : son adresse figure seulement dans le champ `source_url` des réponses. Si vous ou votre client ouvrez ce lien, c'est une autre requête, hors de ce serveur.

### Vers le serveur de Service-Public (`fr_search_service_public`, `fr_get_service_public`)

Ces deux outils lisent les fiches pratiques de Service-Public.gouv.fr et d'Entreprendre.Service-Public.gouv.fr dans une copie de l'archive du jour, téléchargée sur `lecomarquage.service-public.gouv.fr` par HTTPS et gardée en mémoire. Le téléchargement n'a lieu que la première fois qu'un de ces outils sert un public, puis pour revalider l'archive au plus tôt 24 heures après ; après un échec, pas de nouvelle tentative avant une minute (plus longtemps quand des téléchargements refusés, coupés ou trop lents se suivent : un quart d'heure, une heure, puis six heures pour une archive refusée, une heure au plus sinon). Les dix-sept autres outils (`fr_coverage` compris), `check-config` et le démarrage n'y envoient rien (vérifié par des tests du binaire, avec un proxy témoin).

- **Ce qui part** : une requête `GET` vers l'une des deux adresses fixes de l'archive, `/vdd/3.5/part/zip/vosdroits-latest.zip` (Particuliers) ou `/vdd/3.5/pro/zip/vosdroits-latest.zip` (Entreprendre). **Rien de ce que vous cherchez ou lisez** : ni mot de recherche, ni numéro de fiche ; la recherche et la lecture se font dans la copie en mémoire (vérifié par un test sur un faux serveur, qui relève l'adresse et les en-têtes de chaque requête reçue). La requête porte `Host`, `Accept` et le `User-Agent` `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-legifrance)`, plus, pour une revalidation, `If-None-Match` avec l'étiquette (`ETag`) que le serveur a rendue.
- **Ce qui ne part pas** : vos identifiants PISTE, un compte, un cookie, un `Referer`.
- **Votre adresse IP et l'heure** : le serveur apprend qu'un poste a téléchargé l'archive d'un public à telle heure, une fois par jour environ. Il n'apprend rien de vos questions.
- **Qui reçoit ces requêtes.** La DILA édite Service-Public.gouv.fr ; les mentions légales du site nomment l'hébergeur, Worldline France. La page « Données personnelles et sécurité » du site indique des journaux de connexion (horodatage, adresse IP) gardés un an, sans nommer le sous-domaine `lecomarquage` du flux (lu le 2026-11-23, V-230).
- **Couper cette source** : `LF_SERVICE_PUBLIC=off` dans l'environnement du serveur. Dans le bundle MCPB, c'est le réglage « Practical sheets source (Service-Public) » des paramètres de l'extension, `on` par défaut. Les deux outils répondent alors `[config_error]` sans aucune requête (vérifié par un test du binaire, avec un proxy témoin). Les mêmes règles que pour `LF_BOFIP` s'appliquent : toute autre valeur que `on` ou `off` coupe la source, une variable vide compte comme absente, et en mode hors ligne (`LF_DILA_DIR` défini) la source est coupée d'office et ne revient que par `LF_SERVICE_PUBLIC=on`.
- **Sites de Service-Public.** `www.service-public.gouv.fr` et `entreprendre.service-public.gouv.fr` ne sont jamais contactés par le serveur : leurs adresses figurent seulement dans le champ `source_url` des réponses.

### Vers Judilibre, par PISTE (`fr_search_judilibre`, `fr_get_judilibre`)

Ces deux outils interrogent l'API Judilibre de la Cour de cassation sur l'hôte de l'API PISTE, avec les mêmes identifiants, le même jeton et le même débit que les outils de Légifrance, et seulement quand l'un d'eux est appelé. Les dix-sept autres outils, `check-config` et le démarrage ne l'appellent pas (vérifié par des tests du binaire : avec un proxy témoin, et avec un faux PISTE qui relève l'adresse de chaque requête reçue).

- **Ce qui part** : vos identifiants PISTE pour le jeton, comme pour Légifrance ; pour une recherche, vos mots, la façon de les combiner, les juridictions, la partie de la décision, les dates, le tri et le numéro de page ; pour une lecture, l'identifiant de la décision. Ces valeurs sont dans l'adresse de la requête : chiffrées pendant le transport, elles arrivent en clair chez PISTE et à la Cour de cassation, qui peuvent les écrire dans leurs journaux, comme tout serveur web. Votre adresse IP et le `User-Agent` partent comme pour toute requête.
- **Qui reçoit ces requêtes.** PISTE (AIFE), puis la Cour de cassation, qui édite Judilibre. Leurs politiques s'appliquent ; ce document n'en cite pas les adresses (V-213).
- **Rien n'est gardé.** Aucun cache : chaque appel fait une requête, et ni vos mots ni les décisions lues ne sont écrits sur votre poste, hors l'empreinte du journal d'audit.
- **Couper cette source** : `LF_JUDILIBRE=off` dans l'environnement du serveur. Dans le bundle MCPB, c'est le réglage « Court decisions source (Judilibre) » des paramètres de l'extension, `on` par défaut. Les deux outils répondent alors `[config_error]` sans aucune requête (vérifié par un test du binaire, avec un proxy témoin). Les mêmes règles que pour `LF_BOFIP` s'appliquent : toute autre valeur que `on` ou `off` coupe la source, une variable vide compte comme absente, et en mode hors ligne (`LF_DILA_DIR` défini) la source est coupée d'office et ne revient que par `LF_JUDILIBRE=on`. Sans identifiants PISTE, rien ne part non plus.
- **Données personnelles dans les décisions.** La Cour de cassation publie les décisions pseudonymisées : le nom des parties est remplacé. Les noms des magistrats et des greffiers restent lisibles dans le texte ; le serveur ne les extrait, ne les indexe ni ne les journalise, et n'offre aucun filtre ni aucune statistique par personne, chambre ou siège. La loi interdit de les employer pour évaluer, analyser, comparer ou prédire leurs pratiques professionnelles (article L. 111-13 du code de l'organisation judiciaire).

## Données conservées localement

**Journal d'audit.** Une ligne JSON par appel d'outil, dans le répertoire `LF_AUDIT_DIR`. Elle contient : l'horodatage UTC, le nom de l'outil, une **empreinte** SHA-256 des arguments, un nombre ou une taille de résultat, la durée, le statut, le code et le message d'erreur le cas échéant (500 caractères au plus, sans secret, sans chemin ni URL à paramètres), l'environnement PISTE et la version du serveur. Elle ne contient **jamais** vos requêtes en clair, ni un texte juridique renvoyé, ni un secret.

- L'empreinte n'est pas salée : une requête courte ou devinable peut être retrouvée en essayant des candidats. Le journal n'est donc pas anonyme pour qui y a accès et sait quoi chercher.
- Emplacement par défaut :

  | Système | Répertoire |
  |---|---|
  | Linux | `$XDG_DATA_HOME/mcp-legifrance/audit`, sinon `~/.local/share/mcp-legifrance/audit` |
  | macOS | `~/Library/Application Support/mcp-legifrance/audit` |
  | Windows | `%APPDATA%\mcp-legifrance\audit` |
  | Image OCI | aucun défaut : pointez `LF_AUDIT_DIR` vers un volume |

- **Droits sous Linux et macOS** : répertoire en `0700`, fichiers en `0600`. Un répertoire ou un fichier préexistant plus permissif n'est pas modifié, le serveur avertit.
- **Droits sous Windows : ACL héritées, non vérifiées.** Le serveur ne pose aucune ACL. Les fichiers reprennent celles du dossier de profil (`%APPDATA%`), a priori privées pour votre compte, mais ce point n'est **pas vérifié** (V-165 ; mesure prévue par le test `windows_acl_report`, ignoré par défaut). Contrôlez-le chez vous : `icacls "%APPDATA%\mcp-legifrance\audit"`.
- **Rotation et purge.** Le journal tourne chaque jour (date UTC) ou à 16 Mio, sans rien supprimer. La purge vous revient : vous pouvez effacer le répertoire à tout moment, sans autre effet que de perdre l'historique.
- `LF_AUDIT_MODE` règle la conduite en cas d'échec d'écriture : `strict` (défaut) écarte le résultat, `best-effort` le rend quand même.

- **Outils du BOFiP.** Leurs appels sont audités comme les autres : une empreinte des arguments, jamais vos mots en clair. Quand un appel échoue, la ligne garde aussi le message d'erreur rendu, qui peut citer l'identifiant de publication demandé (`BOI-…`) ou la valeur refusée d'un paramètre `id`, `serie` ou `kind` ; il ne cite jamais les mots de la recherche. La réserve ci-dessus vaut d'autant plus : une recherche fiscale de deux ou trois mots courants se retrouve facilement par essais. Le champ d'environnement de la ligne garde le nom de l'environnement PISTE configuré, même si ces outils ne passent pas par PISTE : c'est le nom de l'outil qui dit la source.
- **Outils de Service-Public.** Leurs appels sont audités de la même façon : une empreinte des arguments, jamais vos mots en clair. Quand un appel échoue, la ligne garde le message d'erreur rendu, qui peut citer le numéro de fiche demandé (`F…`, `N…`) ou la valeur refusée d'un paramètre `id` ou `audience` ; il ne cite jamais les mots de la recherche.
- **Outils de Judilibre.** Leurs appels sont audités de la même façon : une empreinte des arguments, jamais vos mots en clair. Quand un appel échoue, la ligne garde le message d'erreur rendu, qui peut citer l'identifiant de décision demandé ou la valeur refusée d'un paramètre ; il ne cite jamais les mots de la recherche.

**En mémoire seulement**, perdu à l'arrêt du processus : le jeton PISTE, un cache des réponses de Légifrance (recherche 1 h, consultations 24 h, listes 7 jours), le compteur de requêtes du jour vers le portail du BOFiP, l'heure d'un refus d'abonnement de Judilibre (gardée 10 minutes), et les archives du jour de Service-Public avec leur index (environ 47 Mio pour les deux publics, mesurés sur le binaire de la branche le 2026-11-24). Il n'y a aucun cache sur disque, et aucun cache des réponses du BOFiP.

**Journaux de diagnostic.** Ils vont sur stderr, sans texte de requête ni secret. Où ils atterrissent dépend de votre client MCP, qui peut les conserver dans ses propres fichiers. Au niveau `trace` (réglage `LF_LOG`, à éviter), la bibliothèque HTTP y écrit le nom et le port de l'hôte contacté, jamais le chemin ni les mots de la requête (contrôlé par un test pour le BOFiP). Pour Service-Public, un test vérifie au même niveau que vos mots de recherche n'y figurent pas ; ils ne partent de toute façon pas sur le réseau.

## Options de compilation facultatives (absentes des binaires publiés)

- **Index local DILA** (`dila`). Quand `LF_DILA_DIR` désigne un répertoire, les textes (codes, lois et décrets seulement) sont lus dans un index que vous avez construit vous-même à partir des dumps publics de la DILA, sur votre disque : aucun identifiant PISTE n'est demandé et aucune requête ne part vers PISTE. Aucune ne part non plus vers le portail du BOFiP : la source de doctrine fiscale est coupée d'office dans ce mode, sauf si vous écrivez vous-même `LF_BOFIP=on`. Le serveur n'écrit rien dans ce répertoire en fonctionnement normal et ne télécharge rien ; seule la commande `build-dila-index`, que vous lancez, y écrit, à partir d'archives déjà présentes sur votre disque. Le script de téléchargement des dumps est extérieur au binaire. Le journal d'audit est tenu comme d'habitude, et il étiquette cette source `production` (c'est un nom d'environnement, non une indication de réseau). Le contenu de l'index est public ; ce que vous y cherchez reste sur votre poste.
- **Transport HTTP local** (`http`, sous-commande `serve-http`). Le serveur écoute sur la boucle locale seulement, derrière un jeton que vous fournissez dans `LF_MCP_HTTP_TOKEN`. Ce jeton n'est jamais transmis à PISTE, journalisé ni audité. Ce transport n'ouvre aucune connexion sortante de plus que le mode par défaut.

## Ni télémétrie, ni compte, ni service tiers

Aucune télémétrie, aucune vérification de version en ligne, aucune mise à jour automatique, aucune configuration distante, aucun compte (ADR-0005). Chaque utilisateur apporte ses identifiants. Le projet n'exploite aucun serveur.

## Ce que voient votre client et votre modèle

Les résultats d'un outil sont remis à votre client MCP, qui les transmet au modèle de langage que vous utilisez. Leur traitement relève de la politique de confidentialité de ce client et de ce modèle, pas de ce serveur. Les décisions de justice sont diffusées pseudonymisées et les délibérations de la CNIL occultées : le serveur les rend telles que publiées et ne cherche pas à ré-identifier qui que ce soit.

## Hôtes contactés

| Composant | Hôtes | Quand |
|---|---|---|
| Binaire, `LF_PISTE_ENV=sandbox` (défaut) | `sandbox-oauth.piste.gouv.fr`, `sandbox-api.piste.gouv.fr` | Au premier appel d'outil, jamais au démarrage |
| Binaire, `LF_PISTE_ENV=production` | `oauth.piste.gouv.fr`, `api.piste.gouv.fr` | Au premier appel d'outil |
| Surcharges `LF_PISTE_OAUTH_URL` / `LF_PISTE_BASE_URL` | L'hôte fourni (`https` obligatoire ; `http` admis vers 127.0.0.1, ::1 et localhost seulement) | Au premier appel d'outil ; avertissement au démarrage hors des quatre hôtes officiels |
| Binaire, source du BOFiP active (défaut ; coupée par `LF_BOFIP=off`, par une valeur invalide de `LF_BOFIP` ou par `LF_DILA_DIR`) | `data.economie.gouv.fr` (adresse fixée dans le code, `https` seul, aucune redirection suivie, aucune variable de surcharge) | À chaque appel de `fr_search_bofip` ou de `fr_get_bofip`, jamais au démarrage |
| Binaire, source de Service-Public active (défaut ; coupée par `LF_SERVICE_PUBLIC=off`, par une valeur invalide de `LF_SERVICE_PUBLIC` ou par `LF_DILA_DIR`) | `lecomarquage.service-public.gouv.fr` (deux adresses fixées dans le code, `https` seul, aucune redirection suivie, aucune variable de surcharge) | Au premier appel de `fr_search_service_public` ou de `fr_get_service_public` pour un public, puis pour une revalidation après 24 heures ; jamais au démarrage |
| Binaire, source de Judilibre active (défaut ; coupée par `LF_JUDILIBRE=off`, par une valeur invalide de `LF_JUDILIBRE` ou par `LF_DILA_DIR`) | Les hôtes PISTE ci-dessus (ou ceux des surcharges), chemins `/cassation/judilibre/v1.0/search` et `/cassation/judilibre/v1.0/decision` seulement | À chaque appel de `fr_search_judilibre` ou de `fr_get_judilibre`, jamais au démarrage |
| `check-config --online` | L'hôte OAuth de l'environnement choisi, sans appel à l'API (Légifrance ou Judilibre), au portail du BOFiP ni au serveur de Service-Public | À la demande |
| M9 (`dila`) | Aucun (BOFiP, Service-Public et Judilibre coupés d'office, sauf `LF_BOFIP=on`, `LF_SERVICE_PUBLIC=on` ou `LF_JUDILIBRE=on`) | Jamais |
| Script de téléchargement des dumps (M9) | `echanges.dila.gouv.fr` | À l'exécution du script |
| Installeurs sh/ps1 | `github.com` et l'hôte de redirection des assets | À l'installation |
| `cargo install --git` | `github.com`, `index.crates.io`, `static.crates.io` | À l'installation |
| Image OCI | `ghcr.io` | Au téléchargement de l'image |

Derrière un proxy d'entreprise, le binaire est prévu pour suivre le proxy du système ; pour PISTE, un test du binaire montre que la demande de jeton, commune à Légifrance et à Judilibre, suit `HTTPS_PROXY`, et le proxy n'y voit que l'hôte OAuth ; le reste de V-03 n'est pas mesuré. Pour le BOFiP, un test du binaire montre qu'il suit `HTTPS_PROXY` : le proxy voit alors le nom de l'hôte (`data.economie.gouv.fr`), l'heure et le volume de chaque échange, pas l'adresse complète ni vos mots, qui restent dans la connexion chiffrée. Un proxy qui déchiffre le trafic (inspection TLS, avec son certificat installé sur votre poste) les verrait. Pour Service-Public, un test du binaire montre aussi qu'il suit `HTTPS_PROXY` : le proxy voit `lecomarquage.service-public.gouv.fr`, et même déchiffré, le trafic ne porte aucun de vos mots.

## Politiques officielles de la DILA, de PISTE et du portail du ministère

Le traitement de vos données par PISTE, par Légifrance, par la Cour de cassation pour Judilibre, par le portail `data.economie.gouv.fr` et par le serveur de la DILA qui publie les archives de Service-Public relève de leurs propres politiques. Pour ce dernier, lisez les mentions légales et la page « Données personnelles et sécurité » de service-public.gouv.fr. Pour le portail, lisez ses conditions générales et sa politique de confidentialité, accessibles depuis son pied de page (ce document n'en cite pas l'adresse, pour la même raison). Les adresses officielles de leurs pages de données personnelles n'ont pas été vérifiées (V-213) : ce document n'en cite donc aucune. Cherchez-les dans les mentions légales de `legifrance.gouv.fr` et de `piste.gouv.fr`.

## Contacts

Issues du dépôt, ou signalement privé décrit dans [SECURITY.md](SECURITY.md). Pour les données traitées par PISTE et Légifrance : DILA, `rgpd@dila.gouv.fr`.
