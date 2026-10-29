# mcp-legifrance

Projet non officiel, sans lien avec la DILA. *Not affiliated with or endorsed by DILA.*

Dépôt : <https://github.com/muchiny/mcp-legifrance>

Serveur MCP en **Rust** pour interroger le droit français officiel (lois, codes, jurisprudence, conventions collectives) : il lit l'API Légifrance du portail PISTE, en **lecture seule**, et renvoie le texte tel que Légifrance le publie, avec une citation et l'adresse de la source. Il cible la spécification **MCP 2026-07-28** et reste compatible avec les clients en 2025-11-25 et 2025-06-18.

> **Binaires non signés.** Les binaires et le bundle MCPB ne sont signés par aucun certificat (ni Authenticode sous Windows, ni notarisation sous macOS). Sous Windows, Smart App Control peut bloquer l'exécutable : ne le désactivez pas, utilisez l'image Docker ou WSL. Sous macOS, Gatekeeper peut demander une confirmation. Les sommes SHA-256 et les attestations de provenance GitHub permettent de contrôler ce que vous installez (voir [Vérifier un téléchargement](#vérifier-un-téléchargement)).
>
> **Statut.** Cette page décrit la version **0.3.0**. Les commandes d'installation s'appliquent dès que l'étiquette `v0.3.0` et sa release GitHub existent ; avant cela, seule la compilation depuis un clone du dépôt est possible.

## Sommaire

1. [Ce que fait le serveur](#ce-que-fait-le-serveur)
2. [Avant d'installer : une application PISTE](#avant-dinstaller--une-application-piste)
3. [Installation par canal](#installation-par-canal)
4. [Configuration des clients](#configuration-des-clients)
5. [Variables d'environnement](#variables-denvironnement)
6. [Hôtes contactés](#hôtes-contactés)
7. [Attribution, licence des données et non-affiliation](#attribution-licence-des-données-et-non-affiliation)
8. [Migrer depuis fr-eli-mcp](#migrer-depuis-fr-eli-mcp)
9. [Confidentialité et sécurité](#confidentialité-et-sécurité)
10. [Ce qui n'est pas encore vérifié](#ce-qui-nest-pas-encore-vérifié)
11. [Développer et contribuer](#développer-et-contribuer)

## Ce que fait le serveur

Treize outils, tous en lecture seule (`readOnlyHint`), qui interrogent Légifrance sur le réseau (`openWorldHint`) :

| Outil | Rôle |
|---|---|
| `fr_search` | Recherche par mots-clés dans un fonds : LODA (lois et décrets), CODE, JURI (Cour de cassation et cours d'appel), CONSTIT, CETAT, CNIL, KALI (conventions collectives) ou ACCO (accords d'entreprise). Pagination, filtres de dates, version en vigueur à une date. |
| `fr_get_act` | Table des matières d'un texte (loi, décret, code) à une date donnée. |
| `fr_get_text` | Texte d'un article, avec lecture progressive (voir ci-dessous). |
| `fr_get_decision` | Décision de justice (Cour de cassation, Conseil constitutionnel, Conseil d'État, cours d'appel, tribunaux administratifs). |
| `fr_get_deliberation` | Délibération de la CNIL. |
| `fr_get_convention` | Convention collective : métadonnées et table des matières. |
| `fr_get_company_agreement` | Accord d'entreprise : métadonnées seules (le texte est une pièce jointe). |
| `fr_coverage` | Ce que le serveur couvre et ne couvre pas, l'environnement PISTE, l'état de la configuration, la source et la date de consultation. |
| `fr_resolve` | Transforme une citation en texte libre (« article 1240 du code civil », « Cass. civ. 1re, 4 juin 2025, n° 24-13.470 », un ECLI) en identifiants Légifrance, avec un niveau de confiance. Rien n'est deviné ni inventé. |
| `fr_verify_citations` | Vérifie toutes les citations juridiques d'un texte (jusqu'à 20 000 caractères, par exemple un brouillon de réponse) contre Légifrance : codes, lois, décrets, décisions, ECLI, délibérations de la CNIL, conventions. |
| `fr_get_related` | Liens juridiques d'un article ou d'une décision : textes cités, textes qui le citent, jurisprudence mentionnée. |
| `fr_diff_article` | Compare les versions d'un article à deux dates : passages retirés et ajoutés, avec un peu de contexte. |
| `fr_search_all` | Recherche dans plusieurs fonds à la fois : les meilleurs résultats de chacun, groupés par fonds, avec leurs totaux. |

**Retour progressif.** Pour économiser le contexte du modèle, un texte de plus de 1 200 caractères est d'abord rendu en résumé. Pour le lire en entier, demandez `detail: "full"` et enchaînez avec `offset = next_offset` ; `focus` extrait les passages qui correspondent à des mots donnés. Une réponse ne dépasse jamais 45 000 caractères de JSON compact.

**Resources, prompts et complétions.** En plus des outils, le serveur expose la resource `legifrance://coverage`, quatre modèles de resources (`legifrance://act/{text_id}`, `legifrance://article/{article_id}`, `legifrance://code/{code}/article/{num}`, `legifrance://decision/{decision_id}`), trois prompts en français (`analyser_decision`, `etat_du_droit`, `verifier_citations`) et la complétion de leurs arguments.

**Options de compilation, absentes des binaires publiés.** Deux fonctions existent dans le code et se compilent à la demande, mais aucune n'est active par défaut ni livrée dans les binaires, l'image Docker et le bundle MCPB : le transport Streamable HTTP local (`--features http`, sous-commande `serve-http`, boucle locale seulement, jeton dans `LF_MCP_HTTP_TOKEN`) et l'index local DILA (`--features dila`, fonds LEGI seul, activé par `LF_DILA_DIR`). L'index local est un instantané : il peut être en retard sur Légifrance et ne prouve donc jamais qu'un texte n'existe pas. Le [CHANGELOG](CHANGELOG.md) dit, version par version, ce que livre chaque release.

**Protocole.** MCP 2026-07-28 (« moderne »), ainsi que 2025-11-25 et 2025-06-18 (« legacy ») dans le même binaire. La version 2025-03-26 n'est pas servie. Le démarrage ne fait aucun appel réseau : l'authentification PISTE se fait au premier appel d'outil.

## Avant d'installer : une application PISTE

Chaque utilisateur apporte **ses propres identifiants** : le projet n'en fournit aucun, et les conditions d'utilisation de PISTE interdisent de partager les vôtres. Étapes, à faire une seule fois :

1. Créer un compte nominatif sur <https://piste.gouv.fr> et accepter les conditions d'utilisation de PISTE.
2. Dans le catalogue, choisir l'API Légifrance et accepter ses conditions (l'accès est gratuit).
3. Créer **une application par environnement** : une pour le bac à sable (`sandbox`), une pour la `production`. Relever le `client_id` et le `client_secret` dans la section « OAuth Credentials » de l'application.

Les identifiants du bac à sable ne sont **pas** valables en production (PISTE répond `invalid_client`). Le serveur démarre en `sandbox` par défaut ; le bac à sable peut renvoyer d'autres données que la production et n'offre aucun engagement de service. Pour la production, voir [Passer en production](#passer-en-production).

Si vos clés sont divulguées, réinitialisez-les sur PISTE sans attendre, puis prévenez la DILA et l'AIFE.

## Installation par canal

| Canal | Pour qui | Commande ou action | Intégrité |
|---|---|---|---|
| Installeur (archives de la release) | juristes et développeurs à l'aise en terminal | `curl … \| sh` (Linux, macOS) ; `irm … \| iex` (Windows) | HTTPS ; sommes SHA-256 et attestations, non signé |
| **Bundle MCPB** | Claude Desktop | double-clic sur `mcp-legifrance-0.3.0.mcpb` | `fileSha256` vérifié par le client ; non signé |
| **Image Docker** | isolation, VS Code (galerie), Docker Desktop | `docker run -i --rm …` | digest, attestation |
| **`cargo install --git`** | développeurs Rust | compilation locale depuis le dépôt, à l'étiquette | `Cargo.lock` imposé par `--locked` ; non signé |

Cibles des binaires : Linux x86_64 et aarch64 (gnu et musl), macOS x86_64 et aarch64, Windows x86_64. Windows sur ARM64 n'est pas une cible : le binaire x86_64 y fonctionne par émulation (non vérifié).

Le serveur est aussi déclaré au registre MCP sous le nom `io.github.muchiny/mcp-legifrance` (paquets `mcpb` et `oci`).

### Installeurs (Linux, macOS, Windows)

```sh
# Linux, macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/muchiny/mcp-legifrance/releases/download/v0.3.0/mcp-legifrance-installer.sh | sh
```

```powershell
# Windows
powershell -c "irm https://github.com/muchiny/mcp-legifrance/releases/download/v0.3.0/mcp-legifrance-installer.ps1 | iex"
```

Le binaire est posé dans `~/.mcp-legifrance/bin/mcp-legifrance` (Windows : dossier `.mcp-legifrance\bin` de votre dossier utilisateur, fichier `mcp-legifrance.exe`) et ce répertoire est ajouté au `PATH`. Une application graphique déjà ouverte (Claude Desktop, VS Code) ne voit pas le nouveau `PATH` : redémarrez-la, ou donnez le **chemin absolu** dans sa configuration, comme dans les exemples plus bas. L'installeur ne fait aucune mise à jour automatique : pour changer de version, relancez-le avec l'étiquette voulue.

### Vérifier un téléchargement

Plutôt que d'exécuter un script téléchargé, vous pouvez prendre l'archive de votre cible sur la [page des releases](https://github.com/muchiny/mcp-legifrance/releases), la vérifier, puis l'extraire :

```sh
# somme SHA-256 : comparez le résultat à la ligne correspondante du fichier sha256.sum de la release
sha256sum mcp-legifrance-x86_64-unknown-linux-gnu.tar.xz      # macOS : shasum -a 256 ; Windows : Get-FileHash

# provenance : l'archive a été construite par la CI de ce dépôt
gh attestation verify mcp-legifrance-x86_64-unknown-linux-gnu.tar.xz --repo muchiny/mcp-legifrance
```

Les noms d'archives suivent le motif `mcp-legifrance-<cible>.tar.xz` (Windows : `.zip`). Chaque archive contient le binaire, la licence, le fichier `NOTICE`, les licences tierces, la présente page et le journal des modifications.

### Bundle MCPB (Claude Desktop)

1. Télécharger `mcp-legifrance-0.3.0.mcpb` depuis la page de la release.
2. Le double-cliquer, ou le glisser dans les réglages d'extensions de Claude Desktop.
3. Renseigner les trois champs demandés : identifiant client PISTE, secret client PISTE (tous deux conservés par le stockage sécurisé du client) et environnement (`sandbox` par défaut).

Un seul bundle sert toutes les plateformes ; il choisit le bon binaire au lancement. C'est la voie la plus simple si vous n'êtes pas développeur : aucune configuration JSON à écrire.

### Image Docker

```sh
docker pull ghcr.io/muchiny/mcp-legifrance:0.3.0

# vérification de la configuration (aucun secret sur la ligne de commande : -e NOM sans valeur)
docker run --rm -e LF_PISTE_CLIENT_ID -e LF_PISTE_CLIENT_SECRET ghcr.io/muchiny/mcp-legifrance:0.3.0 check-config
```

L'image est publique, minimale (distroless), exécutée par un utilisateur non root, pour linux/amd64 et linux/arm64. `-i` est indispensable : le client dialogue avec le serveur par l'entrée et la sortie standard. Le journal d'audit est écrit dans `/data/audit` ; montez-y un volume nommé pour le conserver (exemple plus bas). L'image est faite pour un usage **local**.

### `cargo install --git`

```sh
cargo install --git https://github.com/muchiny/mcp-legifrance --tag v0.3.0 --locked mcp-legifrance
```

- Prérequis : une chaîne Rust d'au moins la version 1.88 et un compilateur C. Le fournisseur TLS actuel (aws-lc) peut réclamer d'autres outils de compilation sur certaines plateformes (non confirmé hors Linux).
- Le nom du paquet (`mcp-legifrance`) est obligatoire : le dépôt en contient plusieurs. `--locked` impose les versions de dépendances contrôlées par le projet.
- Le binaire est posé dans le répertoire `bin` de votre installation Cargo (`~/.cargo/bin` par défaut) et n'est pas signé.
- **N'exécutez pas `cargo install mcp-legifrance` sans `--git`** : le projet ne publie rien sur crates.io en version 0.x, et un crate de ce nom, s'il existait, ne serait pas celui-ci.

### Vérifier l'installation

```sh
mcp-legifrance --version
mcp-legifrance check-config            # lit l'environnement, n'appelle aucun service
mcp-legifrance check-config --online   # obtient un jeton OAuth auprès de PISTE, sans appeler l'API
```

`check-config` affiche une ligne par variable (nom, origine, valeur, état), jamais un secret (« set (n characters) » ; l'identifiant client est tronqué à 4 caractères), puis les avertissements et les problèmes. Codes de sortie : `0` tout va bien ; `2` configuration invalide ; `3` PISTE refuse les identifiants ; `4` erreur réseau.

## Configuration des clients

Principes communs :

- **Nom du serveur côté client** : `legifrance` (recommandé). Les outils apparaissent par exemple sous `mcp__legifrance__fr_search` dans Claude Code.
- **Commande** : `mcp-legifrance`, avec l'argument `serve` (c'est le comportement par défaut ; l'écrire évite toute ambiguïté). Utilisez le **chemin absolu** du binaire : les applications graphiques n'héritent pas toujours de votre `PATH`, et un `~` n'est pas développé dans un fichier JSON. Les exemples ci-dessous supposent l'installeur (`/home/alice/.mcp-legifrance/bin/mcp-legifrance`) ; adaptez le chemin.
- **Secrets** : le serveur lit `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` dans son environnement. Aucune option de ligne de commande ne les porte (un argument se lit dans la liste des processus). Du plus sûr au moins sûr : stockage sécurisé du client (MCPB, invites de VS Code) ; variable d'environnement alimentée par un gestionnaire de secrets ; valeur en clair dans un fichier de configuration, à éviter et à ne jamais versionner.
- **Diagnostic** : en cas de doute, lancez `mcp-legifrance check-config --online` dans le même environnement que le client.

La syntaxe des clients a été relevée dans leur documentation officielle le 2026-10-29. Elle n'a pas été éprouvée sur une installation réelle de chacun (voir [Ce qui n'est pas encore vérifié](#ce-qui-nest-pas-encore-vérifié)).

### Claude Code

Fichier `.mcp.json` à la racine du projet. Claude Code remplace `${NOM}` par la variable d'environnement correspondante : le fichier ne contient aucun secret et peut être versionné.

```json
{
  "mcpServers": {
    "legifrance": {
      "type": "stdio",
      "command": "/home/alice/.mcp-legifrance/bin/mcp-legifrance",
      "args": ["serve"],
      "env": {
        "LF_PISTE_ENV": "sandbox",
        "LF_PISTE_CLIENT_ID": "${LF_PISTE_CLIENT_ID}",
        "LF_PISTE_CLIENT_SECRET": "${LF_PISTE_CLIENT_SECRET}"
      }
    }
  }
}
```

Définissez les deux variables dans l'environnement du shell qui lance Claude Code. La même entrée peut s'ajouter en ligne de commande avec `claude mcp add-json legifrance '<json>'`, où `<json>` est le contenu de l'entrée `legifrance` ci-dessus (option `--scope user` pour toutes vos sessions). Évitez `claude mcp add --env LF_PISTE_CLIENT_SECRET=…` : le secret resterait dans l'historique de votre shell. Vérification : `claude mcp list`, puis `/mcp` dans une session.

### Claude Desktop

- **Voie recommandée** : le bundle MCPB ([ci-dessus](#bundle-mcpb-claude-desktop)).
- **Configuration manuelle** : fichier `claude_desktop_config.json` (sous macOS `~/Library/Application Support/Claude/` ; sous Windows, dossier `Claude` des données d'application de l'utilisateur, `AppData\Roaming\Claude`). Le secret y est **en clair**.

```json
{
  "mcpServers": {
    "legifrance": {
      "command": "/Users/alice/.mcp-legifrance/bin/mcp-legifrance",
      "args": ["serve"],
      "env": {
        "LF_PISTE_ENV": "sandbox",
        "LF_PISTE_CLIENT_ID": "<votre client_id>",
        "LF_PISTE_CLIENT_SECRET": "<votre client_secret>"
      }
    }
  }
}
```

### VS Code

Fichier `.vscode/mcp.json`. Les invites `promptString` avec `password: true` demandent les identifiants au premier démarrage et VS Code les conserve ; rien n'est écrit dans le fichier.

```json
{
  "inputs": [
    { "type": "promptString", "id": "lf-client-id", "description": "PISTE client ID", "password": true },
    { "type": "promptString", "id": "lf-client-secret", "description": "PISTE client secret", "password": true }
  ],
  "servers": {
    "legifrance": {
      "type": "stdio",
      "command": "/home/alice/.mcp-legifrance/bin/mcp-legifrance",
      "args": ["serve"],
      "env": {
        "LF_PISTE_ENV": "sandbox",
        "LF_PISTE_CLIENT_ID": "${input:lf-client-id}",
        "LF_PISTE_CLIENT_SECRET": "${input:lf-client-secret}"
      }
    }
  }
}
```

- Un fichier `.mcp.json` à la racine de l'espace de travail est aussi lu par VS Code, mais **sans** `inputs` ni substitution : n'y mettez pas de secret.
- Variante avec fichier d'environnement : `"envFile": "${workspaceFolder}/.env.legifrance"`, fichier ignoré par git.
- Les outils sont annoncés en lecture seule mais ouverts sur le monde extérieur : VS Code peut demander une confirmation avant de transmettre un résultat. Son comportement exact n'a pas été mesuré.
- Depuis la galerie MCP de VS Code, seul le paquet Docker est installable.

### Cursor

Fichier `~/.cursor/mcp.json` (global) ou `.cursor/mcp.json` (projet). Format et substitution de variables non vérifiés ; le secret est en clair.

```json
{
  "mcpServers": {
    "legifrance": {
      "command": "/home/alice/.mcp-legifrance/bin/mcp-legifrance",
      "args": ["serve"],
      "env": { "LF_PISTE_CLIENT_ID": "<votre client_id>", "LF_PISTE_CLIENT_SECRET": "<votre client_secret>" }
    }
  }
}
```

### Variante Docker (tous les clients)

```json
{
  "mcpServers": {
    "legifrance": {
      "command": "docker",
      "args": ["run", "-i", "--rm",
               "-e", "LF_PISTE_CLIENT_ID", "-e", "LF_PISTE_CLIENT_SECRET", "-e", "LF_PISTE_ENV",
               "-v", "mcp-legifrance-audit:/data/audit",
               "ghcr.io/muchiny/mcp-legifrance:0.3.0"],
      "env": {
        "LF_PISTE_CLIENT_ID": "${LF_PISTE_CLIENT_ID}",
        "LF_PISTE_CLIENT_SECRET": "${LF_PISTE_CLIENT_SECRET}",
        "LF_PISTE_ENV": "sandbox"
      }
    }
  }
}
```

Le client pose les variables sur le processus `docker`, qui les transmet au conteneur par `-e NOM`. La substitution `${…}` dans `env` est celle de Claude Code ; pour un autre client, utilisez ses propres invites ou valeurs. C'est aussi la voie de repli sous Windows quand Smart App Control bloque le binaire.

### Windows

- En JSON, doublez les barres obliques inverses : `"C:\\Users\\alice\\.mcp-legifrance\\bin\\mcp-legifrance.exe"`. Pas de `cmd /c` : c'est un exécutable natif.
- Après l'installeur, redémarrez le client (le `PATH` est mis à jour dans le registre utilisateur) ou donnez le chemin absolu.
- N'écrivez pas le secret avec `setx` : il serait stocké en clair dans le registre et dans l'historique PowerShell. Préférez le bundle MCPB ou les invites de VS Code.
- Le chemin par défaut du journal d'audit sous Windows n'est pas vérifié : `check-config` affiche le chemin effectif.

### Passer en production

1. Créer une application PISTE **de production** et y souscrire l'API Légifrance.
2. Positionner `LF_PISTE_ENV=production` et les identifiants de cette application.
3. Lancer `mcp-legifrance check-config --online`.
4. Appeler `fr_coverage` : l'environnement affiché doit être `production`.

Les adresses de production (`oauth.piste.gouv.fr`, `api.piste.gouv.fr`) n'ont jamais été appelées par le projet : elles sont documentées mais non vérifiées.

## Variables d'environnement

La configuration vient **uniquement** de l'environnement (aucun fichier, aucune configuration distante). Elle est lue une fois au démarrage : après toute modification, redémarrez le serveur. Une variable vide ou blanche compte comme absente.

### Réglages usuels

| Variable | Rôle | Valeurs et défaut |
|---|---|---|
| `LF_PISTE_CLIENT_ID` | identifiant OAuth de votre application PISTE | obligatoire |
| `LF_PISTE_CLIENT_SECRET` | secret OAuth de la même application | obligatoire, jamais affiché |
| `LF_PISTE_ENV` | environnement PISTE | `sandbox` (défaut) ou `production` |
| `LF_PISTE_OAUTH_URL`, `LF_PISTE_BASE_URL` | surcharge des adresses PISTE (à éviter : préférez `LF_PISTE_ENV`) | voir les règles ci-dessous |
| `LF_AUDIT_DIR` | répertoire du journal d'audit | chemin absolu (un `~/` en tête est développé) ; défaut : répertoire de données de l'utilisateur, sous-dossier `mcp-legifrance/audit` |
| `LF_AUDIT_MODE` | conduite si le journal ne peut pas être écrit | `strict` (défaut : l'appel d'outil échoue en `[audit_error]`) ou `best-effort` |
| `LF_LOG` | filtre des journaux (syntaxe `tracing`), écrits sur stderr | défaut : `info`, avec `warn` pour les bibliothèques réseau et MCP |
| `LF_HTTP_TIMEOUT_S` | délai par requête vers PISTE, en secondes | 1 à 300, défaut 40 |
| `LF_TOOL_DEADLINE_S` | délai global par appel d'outil, en secondes | 1 à 600, défaut 60 |

Règles des surcharges d'adresse : `https` obligatoire (`http` n'est admis que vers `127.0.0.1`, `::1` et `localhost`) ; ni identifiant utilisateur, ni paramètres, ni fragment dans l'adresse. Un hôte autre que les quatre hôtes officiels est accepté avec l'avertissement `CustomHost` (au démarrage, dans `check-config` et dans les lacunes de `fr_coverage`). Un hôte officiel de l'**autre** environnement est un problème `EnvMismatch` : le serveur ne déduit jamais l'environnement des adresses, il demande `LF_PISTE_ENV=production`.

N'activez pas `debug` ou `trace` pour `rmcp` dans `LF_LOG` : la bibliothèque y journalise alors les réponses entières.

### Réglages avancés

Ces quatre réglages ne figurent ni dans `server.json` ni dans le manifeste du bundle MCPB. Pour les employer, ajoutez-les au bloc `env` de votre client. Une valeur invalide ne bloque pas le démarrage : le défaut s'applique, un avertissement est journalisé, et `check-config` affiche la valeur effective.

| Variable | Rôle | Valeurs et défaut |
|---|---|---|
| `LF_PISTE_RPS` | requêtes par seconde vers PISTE | 1 à 100, défaut 5 |
| `LF_PISTE_BURST` | rafale autorisée du limiteur | 1 à 100, défaut 10 |
| `LF_PISTE_CONCURRENCY` | requêtes en vol en même temps | 1 à 16, défaut 4 |
| `LF_PISTE_MOJIBAKE_REPAIR` | réparation des textes dont les accents sont corrompus | `on` (défaut) ou `off` |

### Configuration invalide : mode dégradé

Si la configuration PISTE est invalide (environnement inconnu, adresse refusée, identifiants absents), le serveur **démarre quand même** : chaque outil PISTE répond `[config_error] …` sans appel réseau, et `fr_coverage` comme `check-config` décrivent le problème.

### Variables héritées de fr-eli-mcp

Ces noms viennent du serveur Python dont ce projet est la réécriture ([NOTICE](NOTICE)). Ils sont documentés ici pour la migration.

| Variable héritée | Comportement | À utiliser à la place |
|---|---|---|
| `FR_ELI_CLIENT_ID` | lue si `LF_PISTE_CLIENT_ID` est absente, avec un avertissement de dépréciation sur stderr ; `LF_PISTE_CLIENT_ID` l'emporte si les deux existent | `LF_PISTE_CLIENT_ID` |
| `FR_ELI_CLIENT_SECRET` | idem | `LF_PISTE_CLIENT_SECRET` |
| `FR_ELI_OAUTH_URL` | idem | `LF_PISTE_ENV`, ou `LF_PISTE_OAUTH_URL` |
| `FR_ELI_BASE_URL` | idem | `LF_PISTE_ENV`, ou `LF_PISTE_BASE_URL` |
| `FR_ELI_AUDIT_DIR` | idem | `LF_AUDIT_DIR` |
| `FR_ELI_CACHE_DIR` | **jamais lue** : plus de cache disque. Définie, elle produit l'avertissement « ignored » au démarrage et dans `check-config` | aucune |
| `FR_ELI_RUNTIME_URL` | **jamais lue** : plus de configuration distante. Avertissement « ignored » | aucune |
| `FR_ELI_ENV` | **jamais lue** (la référence ne l'employait pas non plus). Avertissement « ignored » | `LF_PISTE_ENV` |
| `PISTE_CLIENT_ID`, `PISTE_CLIENT_SECRET` | **jamais lues** (la documentation de la référence les citait à tort). Avertissement « ignored » | `LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET` |

L'avertissement a la forme `FR_ELI_ENV ignored: it is never read`, ou `FR_ELI_CLIENT_ID is deprecated: use LF_PISTE_CLIENT_ID` pour une variable lue par compatibilité. Il n'est jamais accompagné de la valeur de la variable.

## Hôtes contactés

Le serveur ne contacte que les hôtes ci-dessous, et seulement dans les cas indiqués. Aucune télémétrie, aucun compte, aucune vérification de version en ligne, aucune mise à jour automatique.

| Composant | Hôtes | Quand |
|---|---|---|
| Binaire, `LF_PISTE_ENV=sandbox` (défaut) | `sandbox-oauth.piste.gouv.fr`, `sandbox-api.piste.gouv.fr` | au premier appel d'outil, jamais au démarrage |
| Binaire, `LF_PISTE_ENV=production` | `oauth.piste.gouv.fr`, `api.piste.gouv.fr` (jamais appelés par le projet : non vérifiés) | idem |
| Binaire avec `LF_PISTE_OAUTH_URL` ou `LF_PISTE_BASE_URL` | l'hôte que vous fournissez (`https`, ou `http` vers la boucle locale seulement) ; avertissement `CustomHost` s'il n'est pas l'un des quatre hôtes officiels | idem |
| `check-config --online` | l'hôte OAuth de l'environnement choisi : une requête de jeton, jamais d'appel à l'API | à la demande |
| Installeurs shell et PowerShell, bundle MCPB, archives | `github.com` et l'hôte vers lequel GitHub redirige le téléchargement des fichiers de release | à l'installation |
| `cargo install --git` | `github.com` (clone du dépôt à l'étiquette), `index.crates.io` et `static.crates.io` (dépendances) | à l'installation |
| Image Docker | `ghcr.io` | au `docker pull` |
| Script de téléchargement des dumps DILA (version 1.0, hors binaire) | `echanges.dila.gouv.fr` | lancé explicitement par l'utilisateur |

Derrière un proxy d'entreprise, le binaire emploie le proxy système ; la prise en compte des variables de proxy usuelles n'est pas vérifiée. Les installeurs de dist déclarent respecter les variables de proxy usuelles.

## Attribution, licence des données et non-affiliation

**Source des données.** Les contenus servis proviennent de Légifrance (<https://www.legifrance.gouv.fr>), service de la Direction de l'information légale et administrative (DILA), par l'API Légifrance du portail PISTE. Ils sont réutilisables sous **Licence Ouverte 2.0** (Etalab). La mention à reprendre est :

> Source : Légifrance / DILA — Licence Ouverte 2.0

**Date de consultation.** Quand vous citez ou republiez un texte obtenu par ce serveur, indiquez aussi la date à laquelle vous l'avez consulté. L'outil `fr_coverage` la fournit : son champ `consulted_on` donne la date du jour (fuseau Europe/Paris), et son champ `attribution` porte la mention ci-dessus. Les instructions du serveur rappellent la source et la licence. La Licence Ouverte demande le concédant et la date de dernière mise à jour ; le serveur ne dispose pas d'un champ fiable de mise à jour par document, et la date de consultation en tient lieu. Qu'une mention globale suffise est une lecture adoptée par défaut, non confirmée.

**Valeur des textes.** Seules les éditions du Journal officiel signées électroniquement font foi : une réponse de ce serveur n'a pas de valeur opposable et n'est pas un avis juridique. Les décisions de justice sont diffusées **pseudonymisées** et les délibérations de la CNIL **occultées** ; le serveur ne lève ni l'un ni l'autre.

**Non-affiliation.** Ce projet n'est ni édité, ni approuvé, ni soutenu par la DILA, ni par l'AIFE (qui exploite PISTE), ni par aucune autorité publique. « Légifrance » désigne le service public de diffusion du droit : le produit s'appelle `mcp-legifrance`, ses titres portent « (unofficial) », et il n'emploie aucun logo de Légifrance ni de l'État. *Not affiliated with or endorsed by DILA, AIFE or any French public body.*

**Licence du code.** Apache-2.0 ([LICENSE](LICENSE)), titulaire Loïc Wernert. Le projet est une réécriture en Rust de [`matematicsolutions/fr-eli-mcp`](https://github.com/matematicsolutions/fr-eli-mcp) (commit `d3b5b13`, Apache-2.0), dont il reprend des textes adaptés et des fixtures de test : voir [NOTICE](NOTICE). Aucune ligne n'est reprise de projets sous licence AGPL. Les licences des bibliothèques tierces liées au binaire sont réunies dans le fichier de licences tierces fourni à la racine du dépôt et dans chaque archive.

## Migrer depuis fr-eli-mcp

Pour un utilisateur de la référence Python (`fr-eli-mcp`). La note complète, champ par champ, est dans [`docs/plan/annexes/A-parite.md`](docs/plan/annexes/A-parite.md) §6.

- **Commande** : remplacez `fr-eli-mcp` (ou `uvx fr-eli-mcp`) par `mcp-legifrance` dans la configuration du client. Le nom du serveur et son nom au registre changent aussi (`io.github.muchiny/mcp-legifrance`).
- **Variables** : renommez `FR_ELI_CLIENT_ID` et `FR_ELI_CLIENT_SECRET` en `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` (les anciens noms marchent encore, avec un avertissement). Supprimez `FR_ELI_CACHE_DIR`, `FR_ELI_RUNTIME_URL`, `FR_ELI_ENV`, `PISTE_CLIENT_ID` et `PISTE_CLIENT_SECRET`, sans effet.
- **Production** : `LF_PISTE_ENV=production`, et supprimez les surcharges d'adresse. Des surcharges vers la production sans cette variable mettent le serveur en mode dégradé (`EnvMismatch`).
- **Vérification** : `mcp-legifrance check-config`, puis `--online` pour tester les identifiants.
- **Sorties** : lisez `citation` (ex-`human_readable_citation`) ; `eli_uri`, `eli_note` et `dataset_note` n'existent plus ; une valeur absente est une clé absente, jamais `null` ; ne comptez pas sur `title`. Les tables des matières s'appellent `toc` (et non `articles`) et se paginent avec `toc_offset = next_toc_offset`. D'autres champs sont renommés (`date`, `convention_id`, `date_publication`, `path`, `gaps`…) : voir la note complète.
- **Lecture des textes** : au-delà de 1 200 caractères, la réponse par défaut est un résumé ; passez `detail: "full"` et bouclez sur `offset = next_offset`, ou employez `focus`.
- **Erreurs** : le préfixe `[code]` donne la nature de l'erreur. Deux codes s'ajoutent à `invalid_arg`, `not_found`, `upstream_error` et `config_error` : `rate_limited` (réessayer après le délai) et `audit_error`. Ne réessayez jamais `config_error` ni `audit_error`.
- **Audit** : nouveau dossier et nouveau nom de fichier (`mcp-legifrance.jsonl`) ; trois clés de plus ; les empreintes et les métriques ne se comparent pas à celles de la référence.
- **Client** : protocole 2025-06-18 au minimum.

## Confidentialité et sécurité

- **Ce qui part sur le réseau** : vos requêtes de recherche et les identifiants de documents, avec vos identifiants PISTE, vers l'API Légifrance (DILA) par PISTE (AIFE). Rien d'autre (voir [Hôtes contactés](#hôtes-contactés)).
- **Ce qui reste sur votre machine** : un journal d'audit au format JSONL (`mcp-legifrance.jsonl`, avec rotation), à raison d'une ligne par appel d'outil, quelle qu'en soit l'issue. Il contient une **empreinte** SHA-256 des arguments, jamais le texte de vos requêtes en clair, ni secret. En mode `strict` (défaut), un échec d'écriture fait échouer l'appel. Plusieurs processus peuvent partager le même dossier.
- **Journaux** : sur stderr uniquement ; stdout ne porte que le protocole. Ni secret, ni texte de requête n'y figure.
- Les textes renvoyés sont des **données**, pas des instructions : un modèle ne doit pas exécuter ce qu'un texte juridique lui demanderait.
- Détails : [PRIVACY.md](PRIVACY.md) (confidentialité) et [SECURITY.md](SECURITY.md) (signalement d'une vulnérabilité par le signalement privé de GitHub, limites connues).

## Ce qui n'est pas encore vérifié

Le projet note ses incertitudes plutôt que de les taire. Pour la 0.3.0 :

- Les adresses PISTE de **production** n'ont jamais été appelées.
- La syntaxe de configuration de chaque client est tirée de sa documentation ; elle n'a pas été éprouvée sur une installation réelle (priorité : Claude Code et VS Code). Les formats de Cursor et l'emplacement des fichiers de Claude Desktop sont à confirmer.
- Le bundle MCPB n'a pas encore été essayé dans Claude Desktop sous macOS et Windows.
- `cargo install --git` n'a pas encore été essayé sous macOS et Windows.
- Le chemin par défaut du journal d'audit sous Windows, ses permissions, la prise en compte d'un proxy et le comportement de VS Code face à `openWorldHint` ne sont pas mesurés.
- Le bac à sable PISTE renvoie parfois d'autres données que la production.
- L'index local DILA (option de compilation `dila`) n'a été éprouvé que sur de petits jeux de test : sa taille, son temps de construction sur de vrais dumps et la licence propre à chaque fonds ne sont pas mesurés.
- L'adresse Légifrance qu'un article de loi reçoit dans `source_url` n'a pas été ouverte dans un navigateur pour confirmer qu'elle s'affiche.
- Un passage de 21 tests en bac à sable PISTE a réussi le 2026-10-29 ; il ne dit rien de la production, et la CI ne rejoue qu'une partie de ces tests chaque semaine.

Le registre complet des points ouverts est dans [`docs/plan/09-risques-et-questions.md`](docs/plan/09-risques-et-questions.md).

## Développer et contribuer

| | |
|---|---|
| Langage | Rust (édition 2024, MSRV 1.88, chaîne 1.99.0 épinglée), workspace Cargo `crates/lf-*`, binaire `mcp-legifrance` |
| Architecture | hexagonale : `lf-domain` pur → `lf-application` (ports, cas d'usage) → adapters PISTE, cache, audit, MCP → `lf-app` (binaire) |
| SDK MCP | `rmcp` 3.5.1 |
| Qualité | 100 % de couverture en lignes, régions et fonctions, à une exception nommée près (`stdio.rs`, la colle sur les vrais stdin et stdout, couverte par un test de bout en bout du binaire) ; mutation : le diff de chaque PR est muté sur les fichiers à risque seulement, le reste du code l'est chaque semaine, et des mutants survivants y sont connus et non résolus ; `make ci` est la preuve de complétion |

- **[CONTRIBUTING.md](CONTRIBUTING.md)** : boucle de développement, portes de qualité, règles de licence des contributions.
- **[docs/plan/](docs/plan/)** : point d'entrée du plan et ordre de lecture ; **[docs/plan/00-decisions.md](docs/plan/00-decisions.md)** : décisions normatives ; **[docs/plan/07-feuille-de-route.md](docs/plan/07-feuille-de-route.md)** : jalons et tâches.
- **[docs/adr/](docs/adr/)** : décisions d'architecture ; **[docs/recherche/](docs/recherche/)** : notes de recherche sourcées.
- **[CHANGELOG.md](CHANGELOG.md)** : ce que contient chaque version.
