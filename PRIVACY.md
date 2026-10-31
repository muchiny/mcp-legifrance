# Confidentialité

Dernière mise à jour : 2026-10-29.

mcp-legifrance est un serveur MCP qui tourne **sur votre poste**. Il interroge l'API Légifrance du portail PISTE avec **vos** identifiants, et ne parle à personne d'autre. Il n'a ni compte, ni télémétrie, ni service tiers : le projet ne reçoit aucune de vos données. Cette page dit précisément ce qui part, ce qui reste sur votre poste, et ce qui n'est pas vérifié (`docs/plan/08-distribution.md` §1.4 et §9.8). Elle est la politique de confidentialité référencée par `privacy_policies` du bundle MCPB.

Ce produit n'est ni édité, ni approuvé, ni soutenu par la DILA.

## Données envoyées

Elles partent vers PISTE (DILA, AIFE), par HTTPS, et seulement quand un outil est appelé :

- **Vos identifiants PISTE** (`LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET`) vont à l'hôte OAuth de PISTE pour obtenir un jeton. Le jeton accompagne ensuite chaque appel à l'API.
- **Le contenu de vos requêtes** : mots de recherche, filtres, identifiants de textes et de décisions, codes, numéros d'articles et dates. Pour les outils de résolution et de vérification de citations (`fr_resolve` et `fr_verify_citations`), le texte que vous soumettez est analysé sur votre poste ; seules les références qui en sont extraites (code, numéro d'article, date, juridiction, numéro de décision) sont envoyées, pas le texte lui-même.
- **Les en-têtes de toute requête HTTPS** : votre adresse IP, vue par PISTE comme pour n'importe quel client, et un `User-Agent` de la forme `mcp-legifrance/<version> (+https://github.com/muchiny/mcp-legifrance)`.

Ce que vous cherchez peut révéler votre activité. PISTE et la DILA voient les requêtes faites avec votre application ; le projet, lui, ne les voit pas. D'après les CGU de PISTE, la DILA collecte l'adresse électronique du compte PISTE et des statistiques agrégées d'usage, conservées 12 mois au plus ; leur contact est `rgpd@dila.gouv.fr`.

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

**En mémoire seulement**, perdu à l'arrêt du processus : le jeton PISTE, et un cache des réponses (recherche 1 h, consultations 24 h, listes 7 jours). Il n'y a aucun cache sur disque.

**Journaux de diagnostic.** Ils vont sur stderr, sans texte de requête ni secret. Où ils atterrissent dépend de votre client MCP, qui peut les conserver dans ses propres fichiers.

## Options de compilation facultatives (absentes des binaires publiés)

- **Index local DILA** (`dila`). Quand `LF_DILA_DIR` désigne un répertoire, les textes (codes, lois et décrets seulement) sont lus dans un index que vous avez construit vous-même à partir des dumps publics de la DILA, sur votre disque : aucun identifiant PISTE n'est demandé et aucune requête ne part vers PISTE. Le serveur n'écrit rien dans ce répertoire en fonctionnement normal et ne télécharge rien ; seule la commande `build-dila-index`, que vous lancez, y écrit, à partir d'archives déjà présentes sur votre disque. Le script de téléchargement des dumps est extérieur au binaire. Le journal d'audit est tenu comme d'habitude, et il étiquette cette source `production` (c'est un nom d'environnement, non une indication de réseau). Le contenu de l'index est public ; ce que vous y cherchez reste sur votre poste.
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
| `check-config --online` | L'hôte OAuth de l'environnement choisi, sans appel à l'API | À la demande |
| M9 (`dila`) | Aucun | Jamais |
| Script de téléchargement des dumps (M9) | `echanges.dila.gouv.fr` | À l'exécution du script |
| Installeurs sh/ps1 | `github.com` et l'hôte de redirection des assets | À l'installation |
| `cargo install --git` | `github.com`, `index.crates.io`, `static.crates.io` | À l'installation |
| Image OCI | `ghcr.io` | Au téléchargement de l'image |

Derrière un proxy d'entreprise, le binaire est prévu pour suivre le proxy du système ; la prise en compte de `HTTPS_PROXY` n'est pas encore vérifiée (V-03).

## Politiques officielles de la DILA et de PISTE

Le traitement de vos données par PISTE et par Légifrance relève de leurs propres politiques. Les adresses officielles de leurs pages de données personnelles n'ont pas été vérifiées (V-213) : ce document n'en cite donc aucune. Cherchez-les dans les mentions légales de `legifrance.gouv.fr` et de `piste.gouv.fr`.

## Contacts

Issues du dépôt, ou signalement privé décrit dans [SECURITY.md](SECURITY.md). Pour les données traitées par PISTE et Légifrance : DILA, `rgpd@dila.gouv.fr`.
