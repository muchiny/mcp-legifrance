# mcp-legifrance

Projet non officiel, sans lien avec la DILA. *Not affiliated with or endorsed by DILA.*

Dépôt : <https://github.com/muchiny/mcp-legifrance>

Serveur MCP en **Rust** pour interroger le droit français officiel (lois, codes, jurisprudence, conventions collectives) : il lit l'API Légifrance du portail PISTE, en **lecture seule**, et renvoie le texte tel que Légifrance le publie, avec une citation et l'adresse de la source. Il cible la spécification **MCP 2026-07-28** et reste compatible avec les clients en 2025-11-25 et 2025-06-18.

Ce qu'il fait en plus de chercher et de lire un texte :

- **Vérifier les citations.** `fr_verify_citations` contrôle contre Légifrance toutes les citations d'un texte, par exemple un brouillon de réponse ; `fr_resolve` transforme une citation écrite en toutes lettres en identifiant officiel, avec un niveau de confiance. Rien n'est deviné ni inventé.
- **Lire le droit à une date.** Un texte ou un article tel qu'il était en vigueur un jour donné (`fr_search`, `fr_get_act`, `fr_resolve`), et la comparaison de deux versions d'un article (`fr_diff_article`).
- **Sans intermédiaire.** Le serveur tourne sur votre machine et parle à PISTE avec vos propres identifiants : aucun service tiers ne voit vos requêtes (voir [Hôtes contactés](#hôtes-contactés)).
- **Hors ligne, en option.** Les codes, lois et décrets peuvent être lus dans une copie locale, sans compte PISTE ni réseau (voir [Mode hors ligne](#mode-hors-ligne-index-local-dila)).
- **Doctrine fiscale (BOFiP).** `fr_search_bofip` et `fr_get_bofip` lisent le Bulletin officiel des finances publiques, c'est-à-dire la lecture que l'administration fiscale fait de la loi, sur le portail de données ouvertes de la DGFiP, sans compte (voir [Doctrine fiscale](#doctrine-fiscale-bofip)).
- **Fiches pratiques (Service-Public).** `fr_search_service_public` et `fr_get_service_public` lisent les fiches de Service-Public.gouv.fr et d'Entreprendre.Service-Public.gouv.fr, le guide de l'administration sur les droits et les démarches, sans compte et sans envoyer vos mots de recherche (voir [Fiches pratiques](#fiches-pratiques-service-public)).
- **Décisions de justice (Judilibre).** `fr_search_judilibre` et `fr_get_judilibre` lisent Judilibre, la base de décisions que la Cour de cassation publie en données ouvertes : ses propres arrêts, mais aussi ceux des cours d'appel, des tribunaux judiciaires, des tribunaux de commerce et des conseils de prud'hommes, dont la plupart ne sont pas sur Légifrance. Ils passent par la même application PISTE, qui doit être abonnée à l'API Judilibre (voir [Décisions de justice](#décisions-de-justice-judilibre)).

> **Binaires non signés.** Les binaires et le bundle MCPB ne sont signés par aucun certificat (ni Authenticode sous Windows, ni notarisation sous macOS). Sous Windows, Smart App Control peut bloquer l'exécutable : ne le désactivez pas, utilisez l'image Docker ou WSL. Sous macOS, Gatekeeper peut demander une confirmation. Les sommes SHA-256 et les attestations de provenance GitHub permettent de contrôler ce que vous installez (voir [Vérifier un téléchargement](#vérifier-un-téléchargement)).
>
> **Statut.** Cette page décrit la version **1.6.0**, qui ajoute les décisions de justice de Judilibre, la base open data de la Cour de cassation (voir le [CHANGELOG](CHANGELOG.md)). Les fiches pratiques de Service-Public.gouv.fr et d'Entreprendre.Service-Public.gouv.fr sont là depuis la 1.5.0. Depuis la 1.4.0, la lecture du BOFiP liste les articles de code que cite une publication, pour vérifier qu'elle est à jour. La 1.2.0 n'a pas été publiée : la 1.3.0 la contient. Les commandes d'installation citent l'étiquette `v1.6.0` : elles s'appliquent dès que sa release figure sur la [page des releases](https://github.com/muchiny/mcp-legifrance/releases) ; pour une autre version publiée, remplacez le numéro. La première version publiée est la 1.0.0 (2026-10-31).
>
> **Depuis la 1.3.0.** Les deux outils du BOFiP (`fr_search_bofip`, `fr_get_bofip`) et la variable `LF_BOFIP` décrits ci-dessous arrivent avec la 1.3.0 (voir le [CHANGELOG](CHANGELOG.md)). Un binaire plus ancien sert treize outils.
>
> **Depuis la 1.5.0.** Les deux outils de Service-Public (`fr_search_service_public`, `fr_get_service_public`) et la variable `LF_SERVICE_PUBLIC` décrits ci-dessous arrivent avec la 1.5.0 (voir le [CHANGELOG](CHANGELOG.md)). Un binaire plus ancien sert quinze outils (treize avant la 1.3.0).
>
> **Depuis la 1.6.0.** Les deux outils de Judilibre (`fr_search_judilibre`, `fr_get_judilibre`) et la variable `LF_JUDILIBRE` décrits ci-dessous arrivent avec la 1.6.0 (voir le [CHANGELOG](CHANGELOG.md)). Un binaire plus ancien sert dix-sept outils (quinze avant la 1.5.0, treize avant la 1.3.0).

## Sommaire

1. [Ce que fait le serveur](#ce-que-fait-le-serveur)
2. [Avant d'installer : une application PISTE](#avant-dinstaller--une-application-piste)
3. [Installation par canal](#installation-par-canal)
4. [Configuration des clients](#configuration-des-clients)
5. [Variables d'environnement](#variables-denvironnement)
6. [Mode hors ligne (index local DILA)](#mode-hors-ligne-index-local-dila)
7. [Hôtes contactés](#hôtes-contactés)
8. [Attribution, licence des données et non-affiliation](#attribution-licence-des-données-et-non-affiliation)
9. [Migrer depuis fr-eli-mcp](#migrer-depuis-fr-eli-mcp)
10. [Confidentialité et sécurité](#confidentialité-et-sécurité)
11. [Ce qui n'est pas encore vérifié](#ce-qui-nest-pas-encore-vérifié)
12. [Développer et contribuer](#développer-et-contribuer)

## Ce que fait le serveur

Dix-neuf outils, tous en lecture seule (`readOnlyHint`), qui interrogent le réseau (`openWorldHint`) : treize lisent Légifrance, deux le BOFiP, deux les fiches pratiques de Service-Public, deux les décisions de Judilibre.

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
| `fr_search_bofip` | Recherche dans le BOFiP (doctrine de l'administration fiscale) par quelques mots simples, avec un filtre de série (`ENR`, `IR`, `RFPI`…) et de nature (doctrine, actualités). Rend des titres et des identifiants, jamais le texte. |
| `fr_get_bofip` | Texte d'une publication du BOFiP par son identifiant (`BOI-…`, `ACTU-…`), avec la même lecture progressive que `fr_get_text`, et la liste des articles de code qu'elle cite. |
| `fr_search_service_public` | Recherche dans les fiches pratiques de Service-Public.gouv.fr (particuliers, par défaut) ou d'Entreprendre.Service-Public.gouv.fr (entreprises et indépendants), ou les deux, par quelques mots simples cherchés dans les titres et les résumés. Rend des titres, des identifiants et la place de chaque page dans le site, jamais le texte. |
| `fr_get_service_public` | Fiche (`F…`) ou dossier (`N…`) par son identifiant : le texte d'une fiche, avec la même lecture progressive que `fr_get_text`, et les textes de Légifrance sur lesquels elle s'appuie ; la liste des pages d'un dossier. |
| `fr_search_judilibre` | Recherche dans Judilibre : décisions de la Cour de cassation, des cours d'appel, des tribunaux judiciaires, des tribunaux de commerce et des conseils de prud'hommes, par mots, avec un filtre de juridictions, de partie de la décision et de dates, triées par pertinence ou par date. Rend la juridiction, le siège, la chambre, la date, le numéro et le résumé ou un extrait, jamais le texte entier. |
| `fr_get_judilibre` | Décision de Judilibre par son identifiant (24 caractères hexadécimaux) : le texte tel que publié, avec la même lecture progressive que `fr_get_text`, ou une seule partie (`part` : les motifs, le dispositif…) ; les textes appliqués (`visa`), les décisions rapprochées et la décision attaquée. |

**Retour progressif.** Pour économiser le contexte du modèle, un texte de plus de 1 200 caractères est d'abord rendu en résumé. Pour le lire en entier, demandez `detail: "full"` et enchaînez avec `offset = next_offset` ; `focus` extrait les passages qui correspondent à des mots donnés. Une réponse ne dépasse jamais 45 000 caractères de JSON compact.

### Doctrine fiscale (BOFiP)

Le BOFiP (Bulletin officiel des finances publiques, impôts) est la **doctrine de l'administration fiscale** : ce que la Direction générale des finances publiques (DGFiP) écrit pour dire comment elle lit la loi fiscale. Ce n'est pas la loi : un article du code général des impôts se lit avec `fr_get_text`, le commentaire que l'administration en donne se lit avec `fr_get_bofip`.

- **Aucun compte.** Les deux outils lisent le jeu ouvert `bofip-vigueur` du portail `data.economie.gouv.fr`. Ils marchent sans identifiants PISTE, et même si la configuration PISTE est absente ou invalide.
- **En vigueur seulement.** Le jeu ne porte que les publications en vigueur aujourd'hui : aucune version passée n'est servie, alors que la doctrine applicable à une situation est celle du jour des faits (l'article L. 80 A du livre des procédures fiscales protège le contribuable qui a suivi l'interprétation publiée et non rapportée « à la date des opérations en cause »). Chaque réponse le rappelle (`doctrine_note`) et donne la date de début de validité (`valid_from`) et l'adresse de la page sur le site du BOFiP (`source_url`), où les versions passées se consultent.
- **Chercher.** Chaque mot de `query` doit figurer dans la publication tel qu'il est écrit, accents et majuscules mis à part : pas de début de mot, ni opérateur, ni expression entre guillemets. Employez quelques mots simples, et essayez le singulier comme le pluriel. Si aucune publication ne porte tous les mots, la recherche admet des mots approchants (une ou deux lettres de différence) et le dit dans `note`.
- **Lire.** Quelques identifiants désignent deux publications : la réponse les liste alors, et `publication_id` en choisit une.
- **Vérifier qu'une publication est à jour.** Une publication peut être plus ancienne que la loi qu'elle commente. La lecture ne le juge pas : elle liste les articles de code que cite le texte (`cited_articles`, 50 au plus). Passez-les une fois à `fr_verify_citations` (la liste est la même à chaque page), qui donne pour chacun la date de début de la version en vigueur ; pour un article dont la version a commencé après `valid_from`, `fr_diff_article` depuis `valid_from` montre ce qui a changé. Cette vérification interroge Légifrance : elle demande des identifiants PISTE, la lecture du BOFiP non. Les articles de lois et de décrets non codifiés ne sont pas listés.
- **Débit.** Le serveur envoie au plus 2 requêtes par seconde au portail et 10 000 par jour et par processus ; après un refus `429` du portail, il attend sans réessayer et répond `[rate_limited]`.
- **Couper la source.** `LF_BOFIP=off` : les deux outils répondent `[config_error]` sans appel réseau. Voir [Variables d'environnement](#variables-denvironnement).

### Fiches pratiques (Service-Public)

Les fiches pratiques de Service-Public.gouv.fr sont le **guide de l'administration** sur les droits et les démarches, écrit pour le public par la Direction de l'information légale et administrative (DILA) : elles expliquent les règles en langage courant, elles ne sont pas la loi. Les fiches pour les particuliers viennent de Service-Public.gouv.fr ; celles pour les entreprises et les indépendants, d'Entreprendre.Service-Public.gouv.fr (`audience` : `particuliers`, par défaut, ou `entreprendre`). Les fiches pour les associations ne sont pas servies.

- **Aucun compte, aucun mot envoyé.** La première fois qu'un des deux outils sert un public, le serveur télécharge l'archive du jour de ce public sur le serveur de la DILA (24 Mo pour Particuliers et 11 Mo pour Entreprendre le 2026-11-24), la garde en mémoire, puis cherche et lit dans cette copie : ni vos mots ni les numéros de fiches ne quittent votre machine. La copie sert sans nouvelle requête pendant 24 heures ; ensuite, l'appel suivant la revalide, sans la télécharger de nouveau si elle n'a pas changé.
- **Premier appel.** Sur une liaison lente, le téléchargement peut durer plus que l'échéance d'un appel (`LF_TOOL_DEADLINE_S`, 60 s par défaut) : l'outil répond alors `[upstream_error] Service-Public archive is still downloading; retry in a minute.`, et le téléchargement continue. Réessayez une minute plus tard.
- **La version du jour seulement.** Aucune version passée n'est servie. Chaque réponse le rappelle (`guidance_note`), donne la date de l'archive lue (`source_files` pour la recherche, `source_file` pour la lecture) et l'adresse de la page sur le site (`source_url`).
- **Chercher.** Les mots de `query` sont cherchés dans les titres et les résumés des fiches et des dossiers, accents et majuscules mis à part ; un mot trouve aussi les mots plus longs qu'il commence. Le texte des fiches n'est pas cherché : un résultat vide ne prouve pas qu'aucune fiche ne traite du sujet. `audience: "all"` cherche dans les deux sites.
- **Lire.** Une fiche (`F…`) se lit comme un texte de Légifrance (`detail`, `offset`, `max_chars`, `focus`) ; un dossier (`N…`) liste ses pages, à lire ensuite une par une. `path` donne la place de la page dans le site, de la rubrique la plus large à la plus étroite.
- **Vérifier la règle.** Une fiche peut retarder sur un changement de la loi. Elle liste dans `legal_references` les textes de Légifrance sur lesquels elle s'appuie (50 au plus), avec leur identifiant quand un outil du serveur sait le lire : lisez-les avant de vous fier à un montant, un délai ou une condition. Cette vérification interroge Légifrance et demande des identifiants PISTE ; la lecture des fiches non.
- **Débit.** Le serveur n'envoie que ces téléchargements et revalidations. Après un refus `429`, il ne réessaie pas : il répond `[rate_limited]`, se repose, et continue de servir l'archive déjà en mémoire s'il en a une.
- **Couper la source.** `LF_SERVICE_PUBLIC=off` : les deux outils répondent `[config_error]` sans appel réseau. Voir [Variables d'environnement](#variables-denvironnement).

### Décisions de justice (Judilibre)

Judilibre est la base des décisions de la justice judiciaire que la Cour de cassation publie en données ouvertes. Elle complète Légifrance, qui ne publie qu'une sélection d'arrêts de cours d'appel et presque aucune décision de première instance. Elle ne contient aucune décision des juridictions administratives : pour le Conseil d'État, cherchez avec `fr_search` dans le fonds CETAT.

- **Une application PISTE abonnée.** Judilibre passe par PISTE, avec les mêmes identifiants que Légifrance. L'accès est gratuit, mais votre application doit être abonnée à l'API Judilibre, une fois pour chaque environnement :
  1. sur <https://piste.gouv.fr>, dans le catalogue des API, ouvrir l'API Judilibre et accepter ses conditions d'utilisation (celles des données ouvertes de la Cour de cassation) ;
  2. dans votre application PISTE (bac à sable ou production), cocher l'API Judilibre, à côté de Légifrance, puis enregistrer.

  Sans cet abonnement, les deux outils répondent `[config_error] Your PISTE application is not subscribed to the Judilibre API (free): …`, et le serveur ne renvoie aucune requête à Judilibre pendant 10 minutes ; un redémarrage efface cette attente. Seule une application de production abonnée a été essayée (2026-11-27) : la réponse exacte de PISTE à une application non abonnée n'a pas été observée.
- **Vos mots partent vers PISTE.** Comme pour Légifrance, les mots d'une recherche partent vers PISTE et la Cour de cassation. Rien n'est gardé en cache : chaque appel fait une requête, plus une demande de jeton s'il faut le renouveler.
- **Débit partagé.** Judilibre et Légifrance passent par le même jeton, le même limiteur et la même file d'attente : `LF_PISTE_RPS`, `LF_PISTE_BURST` et `LF_PISTE_CONCURRENCY` comptent les deux API ensemble.
- **Chercher.** `match` vaut `all` (tous les mots, par défaut), `any` (un des mots) ou `exact` (l'expression exacte). `courts` choisit parmi `cc` (Cour de cassation), `ca` (cours d'appel), `tj` (tribunaux judiciaires), `tcom` (tribunaux de commerce) et `cph` (conseils de prud'hommes), toutes par défaut. `part` limite la recherche à une partie de la décision, par exemple les motifs (`motivations`), ce qui est plus lent. `date_from`, `date_to` et `sort` (`relevance` ou `date`) complètent la recherche. Judilibre ne sert pas plus de 10 000 résultats (page × taille de page) : au-delà, affinez la recherche.
- **Lire.** `fr_get_judilibre` rend le texte tel que publié, avec la lecture progressive de `fr_get_text`. `part` n'en lit qu'une partie (`introduction`, `expose`, `moyens`, `motivations`, `dispositif` ou `annexes`) quand la décision est découpée ; sinon, le texte entier, avec une note. `visa` liste les textes appliqués, à passer à `fr_resolve` ; `related`, les décisions rapprochées, avec leur identifiant Judilibre ; `contested`, la décision attaquée.
- **Citer.** Les conditions d'utilisation de la Cour de cassation demandent de citer, pour chaque décision, la juridiction, la formation, le siège et la date : chaque résultat les donne (`court`, `chamber`, `formation`, `date`), avec une `citation` prête à reprendre. Pour un tribunal, le siège peut n'apparaître que sous forme de code.
- **Ce qui est interdit.** Les décisions sont **pseudonymisées** : n'essayez jamais de retrouver une partie. La loi (article L. 111-13 du code de l'organisation judiciaire) interdit d'employer l'identité des magistrats et des greffiers pour évaluer, analyser, comparer ou prédire leurs pratiques professionnelles, sous peine de sanctions pénales. Le serveur n'offre donc aucun filtre par personne, chambre, formation ou siège, ni aucune statistique, et chaque réponse le rappelle (`case_law_note`).
- **Pas encore d'adresse de page.** Les réponses ne donnent pas l'adresse de la décision sur le site de la Cour de cassation : elle n'a pas été vérifiée.
- **Couper la source.** `LF_JUDILIBRE=off` : les deux outils répondent `[config_error]` sans appel réseau. Voir [Variables d'environnement](#variables-denvironnement).

**Resources, prompts et complétions.** En plus des outils, le serveur expose la resource `legifrance://coverage`, quatre modèles de resources (`legifrance://act/{text_id}`, `legifrance://article/{article_id}`, `legifrance://code/{code}/article/{num}`, `legifrance://decision/{decision_id}`), trois prompts en français (`analyser_decision`, `etat_du_droit`, `verifier_citations`) et la complétion de leurs arguments.

**Option de compilation, absente des binaires publiés.** Le transport Streamable HTTP local (`--features http`, sous-commande `serve-http`, boucle locale seulement, jeton dans `LF_MCP_HTTP_TOKEN`) existe dans le code et se compile à la demande, mais n'est pas livré dans les binaires, l'image Docker et le bundle MCPB.

**Mode hors ligne, inclus dans les binaires publiés à partir de la 1.0.0.** L'index local DILA (fonctionnalité `dila`, fonds LEGI seul, activé par `LF_DILA_DIR`) se construit avec la sous-commande `build-dila-index`, voir [Mode hors ligne](#mode-hors-ligne-index-local-dila). L'index local est un instantané : il peut être en retard sur Légifrance et ne prouve donc jamais qu'un texte n'existe pas. Le [CHANGELOG](CHANGELOG.md) dit, version par version, ce que livre chaque release.

**Protocole.** MCP 2026-07-28 (« moderne »), ainsi que 2025-11-25 et 2025-06-18 (« legacy ») dans le même binaire. La version 2025-03-26 n'est pas servie. Le démarrage ne fait aucun appel réseau : l'authentification PISTE se fait au premier appel d'outil.

## Avant d'installer : une application PISTE

Chaque utilisateur apporte **ses propres identifiants** : le projet n'en fournit aucun, et les conditions d'utilisation de PISTE interdisent de partager les vôtres. Étapes, à faire une seule fois :

1. Créer un compte nominatif sur <https://piste.gouv.fr> et accepter les conditions d'utilisation de PISTE.
2. Dans le catalogue, choisir l'API Légifrance et accepter ses conditions (l'accès est gratuit). Pour les décisions de justice, faire de même avec l'API Judilibre (voir [Décisions de justice](#décisions-de-justice-judilibre)).
3. Créer **une application par environnement** : une pour le bac à sable (`sandbox`), une pour la `production`. Relever le `client_id` et le `client_secret` dans la section « OAuth Credentials » de l'application.

Les identifiants du bac à sable ne sont **pas** valables en production (PISTE répond `invalid_client`). Le serveur démarre en `sandbox` par défaut ; le bac à sable peut renvoyer d'autres données que la production et n'offre aucun engagement de service. Pour la production, voir [Passer en production](#passer-en-production).

Si vos clés sont divulguées, réinitialisez-les sur PISTE sans attendre, puis prévenez la DILA et l'AIFE.

## Installation par canal

| Canal | Pour qui | Commande ou action | Intégrité |
|---|---|---|---|
| Installeur (archives de la release) | juristes et développeurs à l'aise en terminal | `curl … \| sh` (Linux, macOS) ; `irm … \| iex` (Windows) | HTTPS ; sommes SHA-256 et attestations, non signé |
| **Bundle MCPB** | Claude Desktop | double-clic sur `mcp-legifrance-1.6.0.mcpb` | `fileSha256` vérifié par le client ; non signé |
| **Image Docker** | isolation, VS Code (galerie), Docker Desktop | `docker run -i --rm …` | digest, attestation |
| **`cargo install --git`** | développeurs Rust | compilation locale depuis le dépôt, à l'étiquette | `Cargo.lock` imposé par `--locked` ; non signé |

Cibles des binaires : Linux x86_64 et aarch64 (gnu et musl), macOS x86_64 et aarch64, Windows x86_64. Windows sur ARM64 n'est pas une cible : le binaire x86_64 y fonctionne par émulation (non vérifié).

Le serveur est aussi déclaré au registre MCP sous le nom `io.github.muchiny/mcp-legifrance` (paquets `mcpb` et `oci`).

### Installeurs (Linux, macOS, Windows)

```sh
# Linux, macOS
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/muchiny/mcp-legifrance/releases/download/v1.6.0/mcp-legifrance-installer.sh | sh
```

```powershell
# Windows
powershell -c "irm https://github.com/muchiny/mcp-legifrance/releases/download/v1.6.0/mcp-legifrance-installer.ps1 | iex"
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

1. Télécharger `mcp-legifrance-1.6.0.mcpb` depuis la page de la release.
2. Le double-cliquer, ou le glisser dans les réglages d'extensions de Claude Desktop.
3. Renseigner les quatre champs proposés : identifiant client PISTE, secret client PISTE (tous deux conservés par le stockage sécurisé du client), environnement (`sandbox` par défaut) et source de doctrine fiscale (`on` par défaut ; `off` la coupe).

Un seul bundle sert toutes les plateformes ; il choisit le bon binaire au lancement. C'est la voie la plus simple si vous n'êtes pas développeur : aucune configuration JSON à écrire.

### Image Docker

```sh
docker pull ghcr.io/muchiny/mcp-legifrance:1.6.0

# vérification de la configuration (aucun secret sur la ligne de commande : -e NOM sans valeur)
docker run --rm -e LF_PISTE_CLIENT_ID -e LF_PISTE_CLIENT_SECRET ghcr.io/muchiny/mcp-legifrance:1.6.0 check-config
```

L'image est publique, minimale (distroless), exécutée par un utilisateur non root, pour linux/amd64 et linux/arm64. `-i` est indispensable : le client dialogue avec le serveur par l'entrée et la sortie standard. Le journal d'audit est écrit dans `/data/audit` ; montez-y un volume nommé pour le conserver (exemple plus bas). L'image est faite pour un usage **local**.

### `cargo install --git`

```sh
cargo install --git https://github.com/muchiny/mcp-legifrance --tag v1.6.0 --locked mcp-legifrance
```

- Prérequis : une chaîne Rust d'au moins la version 1.88 et un compilateur C. Le fournisseur TLS actuel (aws-lc) peut réclamer d'autres outils de compilation sur certaines plateformes (non confirmé hors Linux).
- Le nom du paquet (`mcp-legifrance`) est obligatoire : le dépôt en contient plusieurs. `--locked` impose les versions de dépendances contrôlées par le projet.
- Le binaire est posé dans le répertoire `bin` de votre installation Cargo (`~/.cargo/bin` par défaut) et n'est pas signé.
- **N'exécutez pas `cargo install mcp-legifrance` sans `--git`** : le projet ne publie rien sur crates.io, et un crate de ce nom, s'il existait, ne serait pas celui-ci.

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
               "ghcr.io/muchiny/mcp-legifrance:1.6.0"],
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

1. Créer une application PISTE **de production** et y souscrire l'API Légifrance (et l'API Judilibre pour les décisions de justice).
2. Positionner `LF_PISTE_ENV=production` et les identifiants de cette application.
3. Lancer `mcp-legifrance check-config --online`.
4. Appeler `fr_coverage` : l'environnement affiché doit être `production`.

Les adresses de production (`oauth.piste.gouv.fr`, `api.piste.gouv.fr`) ont été exercées à la main par le mainteneur à partir du 2026-11-02 (test de bout en bout de la 1.1.0, voir le [CHANGELOG](CHANGELOG.md)). La CI ne les appelle jamais : ses tests live ne jouent que le bac à sable.

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
| `LF_HTTP_TIMEOUT_S` | délai par requête vers PISTE et vers le portail du BOFiP, en secondes | 1 à 300, défaut 40 |
| `LF_TOOL_DEADLINE_S` | délai global par appel d'outil, en secondes | 1 à 600, défaut 60 |
| `LF_BOFIP` | source de doctrine fiscale (outils `fr_search_bofip` et `fr_get_bofip`) | `on` (défaut) ou `off`, casse indifférente ; voir ci-dessous |
| `LF_SERVICE_PUBLIC` | source des fiches pratiques (outils `fr_search_service_public` et `fr_get_service_public`) | `on` (défaut) ou `off`, casse indifférente ; mêmes règles que `LF_BOFIP` |
| `LF_JUDILIBRE` | source des décisions de justice (outils `fr_search_judilibre` et `fr_get_judilibre`) | `on` (défaut) ou `off`, casse indifférente ; mêmes règles que `LF_BOFIP`, identifiants PISTE en plus |

`LF_BOFIP` ne figure pas dans `server.json` : pour l'employer, ajoutez-la au bloc `env` de votre client. **Le bundle MCPB, lui, n'a pas de bloc `env` à modifier : il porte un réglage « Tax doctrine source (BOFiP) »**, `on` par défaut ; écrivez-y `off` pour couper la source. Ce réglage arrive avec les deux outils, en 1.3.0 : un bundle plus ancien n'a ni les uns ni l'autre. Trois règles :

- sans cette variable (ou si elle est vide : une variable vide compte comme absente, voir plus haut), la source est **active**, sauf en [mode hors ligne](#mode-hors-ligne-index-local-dila) : dès que `LF_DILA_DIR` est défini, elle est coupée d'office, et seul `LF_BOFIP=on` la rétablit ;
- **toute autre valeur que `on` ou `off` coupe la source** (`false`, `0`, `no`, une faute de frappe) : un refus mal écrit ne doit pas envoyer vos mots de recherche à un tiers. Le serveur démarre, le dit sur stderr, et `check-config` le signale comme un problème (code de sortie 2) ;
- il n'existe aucune variable d'adresse pour cette source : l'hôte est fixé dans le code.

`check-config` n'affiche la ligne `LF_BOFIP` que si la variable est définie ou si la source est coupée d'office.

`LF_SERVICE_PUBLIC` suit les mêmes trois règles pour la source des fiches pratiques : active par défaut et coupée d'office en mode hors ligne, sauf `LF_SERVICE_PUBLIC=on` ; coupée par toute autre valeur que `on` ou `off`, avec un problème signalé ; aucune variable d'adresse. Elle ne figure pas non plus dans `server.json` ; le bundle MCPB porte le réglage « Practical sheets source (Service-Public) », `on` par défaut. Le téléchargement de l'archive n'est pas borné par `LF_HTTP_TIMEOUT_S` : un appel l'attend au plus `LF_TOOL_DEADLINE_S` moins une seconde, puis le laisse continuer. `check-config` n'affiche sa ligne, après celle de `LF_BOFIP`, que si la variable est définie ou si la source est coupée d'office.

`LF_JUDILIBRE` suit les mêmes règles pour la source des décisions de justice : active par défaut et coupée d'office en mode hors ligne, sauf `LF_JUDILIBRE=on` ; coupée par toute autre valeur que `on` ou `off`, avec un problème signalé. Elle n'a pas de variable d'adresse : Judilibre est sur l'hôte de l'API PISTE, celui de `LF_PISTE_BASE_URL` s'il est défini. De cette variable, seuls le schéma, l'hôte et le port servent : son chemin est remplacé par `/cassation/judilibre/v1.0/…`. Derrière un proxy inverse qui sert PISTE sous un préfixe (`https://passerelle/prefixe/…`), les requêtes de Judilibre, jeton compris, partent donc hors de ce préfixe ; coupez la source (`LF_JUDILIBRE=off`) si ce chemin mène ailleurs. Elle demande les identifiants PISTE : sans eux, les deux outils répondent `[config_error]` sans aucune requête, comme ceux de Légifrance. Elle ne figure pas dans `server.json` ; le bundle MCPB porte le réglage « Court decisions source (Judilibre) », `on` par défaut. `check-config` n'affiche sa ligne, après celle de `LF_SERVICE_PUBLIC`, que si la variable est définie ou si la source est coupée d'office.

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

Si la configuration PISTE est invalide (environnement inconnu, adresse refusée, identifiants absents), le serveur **démarre quand même** : chaque outil PISTE répond `[config_error] …` sans appel réseau, et `fr_coverage` comme `check-config` décrivent le problème. Les deux outils du BOFiP ne dépendent pas de PISTE : ils continuent de répondre.

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

## Mode hors ligne (index local DILA)

Option : au lieu d'appeler PISTE, le serveur lit une **copie locale** des codes, lois et décrets de Légifrance (le fonds LEGI des données ouvertes de la DILA). Vous n'avez alors ni compte PISTE à créer, ni requête qui quitte votre ordinateur. Les **décisions de justice, les délibérations de la CNIL, les conventions collectives et les accords d'entreprise ne sont pas dans cette copie** : ces outils répondent `[config_error]` et demandent PISTE. Quand `LF_DILA_DIR` est défini, le serveur ne se rabat jamais sur PISTE.

**Et le BOFiP ?** En mode hors ligne, la source du BOFiP est **coupée d'office**, que le dossier d'index soit utilisable ou non : `fr_search_bofip` et `fr_get_bofip` répondent `[config_error]`, et aucune requête ne part. Si vous voulez la doctrine fiscale tout en gardant les textes de Légifrance en local, ajoutez `LF_BOFIP=on` : ces deux outils, et eux seuls, contactent alors `data.economie.gouv.fr`.

**Et Service-Public ?** Même règle : en mode hors ligne, la source des fiches pratiques est coupée d'office. `LF_SERVICE_PUBLIC=on` la rétablit : `fr_search_service_public` et `fr_get_service_public`, et eux seuls, téléchargent alors l'archive du jour sur `lecomarquage.service-public.gouv.fr`.

**Et Judilibre ?** Même règle : en mode hors ligne, la source des décisions de justice est coupée d'office. `LF_JUDILIBRE=on` la rétablit : `fr_search_judilibre` et `fr_get_judilibre`, et eux seuls, contactent alors PISTE, avec vos identifiants PISTE.

**Place nécessaire** (mesurée le 2026-10-31 sur les archives réelles : 1 fichier complet du 2025-07-13 et 481 mises à jour) : 3,1 Go d'archives à télécharger, puis un index de 3,6 Go (2 405 838 documents, aucun membre refusé), soit environ 7 Go de disque. La construction a pris 1 h 10 avec un binaire de développement non optimisé et 3,6 Go de mémoire au plus ; la durée avec le binaire publié n'a pas été mesurée.

**Trois étapes.**

1. Télécharger les archives avec le script du dépôt (bash et `curl` ; une seule connexion, avec des pauses, reprise possible) :

   ```bash
   scripts/fetch-dila-dumps.sh ~/dila/archives
   ```

2. Construire l'index (aucun accès au réseau ; rien n'est extrait sur le disque ; relancer la commande n'applique que ce qui est nouveau) :

   ```bash
   mcp-legifrance build-dila-index --archives ~/dila/archives --index ~/dila/index
   ```

3. Déclarer l'index dans la configuration de votre client MCP, à la place des variables PISTE, avec un chemin **absolu** : `LF_DILA_DIR=/chemin/absolu/vers/dila/index`. Vérifiez avec `mcp-legifrance check-config`.

**À savoir.**

- C'est un **instantané** : un texte publié après le fichier téléchargé manque, et une absence est rendue `unresolvable`, jamais `not_found`. La date de l'instantané n'est pas encore indiquée dans les réponses ; le fichier `registry.tsv` du dossier d'index liste les archives appliquées.
- Pour mettre à jour, relancez les étapes 1 et 2. Un nouveau fichier complet de la DILA refait l'index depuis zéro. Ne lancez pas `build-dila-index` pendant qu'un client lit le même index.
- **Après une montée de version qui change l'analyseur de textes** (la 1.3.0, qui reprend la 1.2.0 jamais publiée, voir le [CHANGELOG](CHANGELOG.md) ; `tantivy` 0.27.0 change la racine de quelques mots français), le serveur et `build-dila-index` **refusent l'ancien index et le disent** : le serveur rend la source locale indisponible, avec un texte qui demande de reconstruire l'index (il ne se rabat pas sur PISTE), et la commande s'arrête avec « must be rebuilt ». Rien n'est supprimé ni reconstruit automatiquement. Marche à suivre : supprimez le dossier d'index **en entier** (le registre `registry.tsv` s'y trouve : le garder avec un index vidé ferait répondre « à jour » à la commande), puis relancez l'étape 2 avec les mêmes archives, qui restent sur le disque : rien à retélécharger. Pour garder l'ancien dossier le temps de vérifier, construisez dans un dossier neuf (`--index`), puis changez `LF_DILA_DIR`. Ne repassez pas à une version antérieure sur un index reconstruit : elle ne voit pas la marque et lirait des racines fausses sans rien dire.
- Le script est en bash : sous Windows, il n'a pas été essayé (WSL ou Git Bash). Le bundle MCPB ne propose pas ce réglage, et l'image Docker n'a pas été essayée avec un index monté.
- **Licence des données.** Les fiches de la DILA demandent de mentionner la paternité (DILA), l'adresse de téléchargement, le nom du fichier et sa date. En cas de doute, citez : « Source : DILA, base LEGI, Légifrance, fichier `<nom>` du `<date>`, `https://echanges.dila.gouv.fr/OPENDATA/LEGI/` ». Les fiches citent la Licence Ouverte 2.0 et le portail data.gouv.fr celle de 2014 : l'écart n'est pas tranché (voir `docs/recherche/10-dumps-dila.md` §8).

## Hôtes contactés

Le serveur ne contacte que les hôtes ci-dessous, et seulement dans les cas indiqués. Aucune télémétrie, aucun compte, aucune vérification de version en ligne, aucune mise à jour automatique.

| Composant | Hôtes | Quand |
|---|---|---|
| Binaire, `LF_PISTE_ENV=sandbox` (défaut) | `sandbox-oauth.piste.gouv.fr`, `sandbox-api.piste.gouv.fr` | au premier appel d'outil, jamais au démarrage |
| Binaire, `LF_PISTE_ENV=production` | `oauth.piste.gouv.fr`, `api.piste.gouv.fr` (exercés à la main par le mainteneur depuis le 2026-11-02, jamais par la CI) | idem |
| Binaire avec `LF_PISTE_OAUTH_URL` ou `LF_PISTE_BASE_URL` | l'hôte que vous fournissez (`https`, ou `http` vers la boucle locale seulement) ; avertissement `CustomHost` s'il n'est pas l'un des quatre hôtes officiels | idem |
| Binaire, source du BOFiP active (défaut ; coupée par `LF_BOFIP=off` ou par `LF_DILA_DIR`) | `data.economie.gouv.fr` (portail de données ouvertes du ministère de l'Économie, jeu `bofip-vigueur`), en `https`, à une adresse fixée dans le code, sans redirection suivie | à chaque appel de `fr_search_bofip` ou de `fr_get_bofip`, jamais au démarrage ni par les autres outils |
| Binaire, source de Service-Public active (défaut ; coupée par `LF_SERVICE_PUBLIC=off` ou par `LF_DILA_DIR`) | `lecomarquage.service-public.gouv.fr` (serveur de la DILA), en `https`, à deux adresses fixées dans le code (l'archive du jour de chaque public), sans redirection suivie ; aucune requête ne porte un mot de recherche ni un numéro de fiche | au premier appel de `fr_search_service_public` ou de `fr_get_service_public` pour un public, puis pour une revalidation après 24 heures ; jamais au démarrage ni par les autres outils |
| Binaire, source de Judilibre active (défaut ; coupée par `LF_JUDILIBRE=off` ou par `LF_DILA_DIR`) | les hôtes PISTE de l'environnement ci-dessus (ou ceux de `LF_PISTE_OAUTH_URL` et `LF_PISTE_BASE_URL`), en deux chemins seulement : `/cassation/judilibre/v1.0/search` et `/cassation/judilibre/v1.0/decision` ; l'adresse d'une recherche porte vos mots | à chaque appel de `fr_search_judilibre` ou de `fr_get_judilibre`, jamais au démarrage ni par les autres outils |
| `check-config --online` | l'hôte OAuth de l'environnement choisi : une requête de jeton, jamais d'appel à l'API (Légifrance ou Judilibre), au portail du BOFiP ni au serveur de Service-Public | à la demande |
| Installeurs shell et PowerShell, bundle MCPB, archives | `github.com` et l'hôte vers lequel GitHub redirige le téléchargement des fichiers de release | à l'installation |
| `cargo install --git` | `github.com` (clone du dépôt à l'étiquette), `index.crates.io` et `static.crates.io` (dépendances) | à l'installation |
| Image Docker | `ghcr.io` | au `docker pull` |
| Script de téléchargement des dumps DILA (version 1.0, hors binaire) | `echanges.dila.gouv.fr` | lancé explicitement par l'utilisateur |

Le site `bofip.impots.gouv.fr` n'est jamais contacté : son adresse n'apparaît que dans le champ `source_url` des réponses, pour que vous puissiez ouvrir la page. Il en va de même de `www.service-public.gouv.fr` et d'`entreprendre.service-public.gouv.fr`.

Derrière un proxy d'entreprise, le binaire emploie le proxy système. Pour PISTE, les tests du binaire montrent que la demande de jeton, commune à Légifrance et à Judilibre, suit `HTTPS_PROXY` : le proxy voit l'hôte OAuth (`sandbox-oauth.piste.gouv.fr` dans le test). Pour le BOFiP, les tests du binaire montrent qu'il suit `HTTPS_PROXY` : le proxy voit alors le nom de l'hôte (`data.economie.gouv.fr`), jamais les mots cherchés, qui restent dans la connexion chiffrée. Pour Service-Public aussi, les tests du binaire montrent qu'il suit `HTTPS_PROXY` : le proxy voit `lecomarquage.service-public.gouv.fr`, et aucune requête ne porte vos mots. Les installeurs de dist déclarent respecter les variables de proxy usuelles.

## Attribution, licence des données et non-affiliation

**Source des données.** Les contenus servis proviennent de Légifrance (<https://www.legifrance.gouv.fr>), service de la Direction de l'information légale et administrative (DILA), par l'API Légifrance du portail PISTE. Ils sont réutilisables sous **Licence Ouverte 2.0** (Etalab). La mention à reprendre est :

> Source : Légifrance / DILA — Licence Ouverte 2.0

**Doctrine fiscale.** Les publications rendues par `fr_search_bofip` et `fr_get_bofip` proviennent du jeu `bofip-vigueur` de la Direction générale des finances publiques (DGFiP), publié sur `data.economie.gouv.fr` sous **Licence Ouverte 2.0**. Chaque réponse de ces deux outils porte la mention à reprendre dans son champ `attribution` :

> Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0

**Fiches pratiques.** Les fiches rendues par `fr_search_service_public` et `fr_get_service_public` proviennent des jeux de données ouverts de la DILA (flux 3.5 de Service-Public, publiés sur data.gouv.fr), sous **Licence Ouverte 2.0**. Chaque réponse de ces deux outils porte dans `attribution` la mention propre au site du public :

- Particuliers : `Source : Service-Public.gouv.fr / DILA — Licence Ouverte 2.0` ;
- Entreprendre : `Source : Entreprendre.Service-Public.gouv.fr / DILA — Licence Ouverte 2.0`.

Elle porte aussi, dans `source_files` (recherche) ou `source_file` (lecture), l'adresse de téléchargement de l'archive, son nom et sa date, que les fiches de présentation de la DILA demandent de citer avec la source.

**Décisions de justice.** Les décisions rendues par `fr_search_judilibre` et `fr_get_judilibre` proviennent de la base Open Data des décisions judiciaires de la Cour de cassation (Judilibre), sous **Licence Ouverte 2.0** et sous les conditions générales d'utilisation de ces données. Chaque réponse de ces deux outils porte la mention à reprendre dans son champ `attribution` :

> Source : Cour de cassation, base Open Data des décisions judiciaires (Judilibre) — Licence Ouverte 2.0

Elle porte aussi la date de mise à jour de la décision (`updated`, lecture) ou le jour de la consultation (`consulted_on`, recherche) et, pour chaque décision, la juridiction, la formation, le siège et la date que ces conditions demandent de citer.

**Date de consultation.** Quand vous citez ou republiez un texte obtenu par ce serveur, indiquez aussi la date à laquelle vous l'avez consulté. L'outil `fr_coverage` la fournit : son champ `consulted_on` donne la date du jour (fuseau Europe/Paris), et son champ `attribution` porte la mention ci-dessus. Les instructions du serveur rappellent la source et la licence. La Licence Ouverte demande le concédant et la date de dernière mise à jour ; le serveur ne dispose pas d'un champ fiable de mise à jour par document, et la date de consultation en tient lieu. Qu'une mention globale suffise est une lecture adoptée par défaut, non confirmée.

**Valeur des textes.** Seules les éditions du Journal officiel signées électroniquement font foi : une réponse de ce serveur n'a pas de valeur opposable et n'est pas un avis juridique. Le BOFiP est la doctrine de l'administration, pas la loi ; le serveur n'en sert que les publications en vigueur aujourd'hui, et ne dit pas si un passage s'applique à votre situation ni à une période passée. Les fiches de Service-Public sont un guide écrit pour le public, pas la loi non plus, et seule leur version du jour est servie. Les décisions de justice sont diffusées **pseudonymisées** et les délibérations de la CNIL **occultées** ; le serveur ne lève ni l'un ni l'autre. Les décisions de Judilibre sont servies telles que publiées : n'essayez pas d'en retrouver les parties, et n'employez pas l'identité des magistrats ou des greffiers pour juger leurs pratiques (voir [Décisions de justice](#décisions-de-justice-judilibre)).

**Non-affiliation.** Ce projet n'est ni édité, ni approuvé, ni soutenu par la DILA, ni par l'AIFE (qui exploite PISTE), ni par la DGFiP, ni par la Cour de cassation, ni par aucune autorité publique. « Légifrance » désigne le service public de diffusion du droit : le produit s'appelle `mcp-legifrance`, ses titres portent « (unofficial) », et il n'emploie aucun logo de Légifrance ni de l'État. *Not affiliated with or endorsed by DILA, AIFE or any French public body.*

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

- **Ce qui part sur le réseau** : vos requêtes de recherche et les identifiants de documents, avec vos identifiants PISTE, vers l'API Légifrance (DILA) par PISTE (AIFE). Pour les deux outils du BOFiP : vos mots de recherche et les identifiants de publications, sans aucun identifiant de compte mais avec votre adresse IP, vers le portail `data.economie.gouv.fr`, hébergé par un prestataire du ministère. Rien d'autre (voir [Hôtes contactés](#hôtes-contactés)). Une recherche fiscale peut en dire long sur votre situation : `LF_BOFIP=off` coupe cette source. Pour les deux outils de Service-Public : le seul téléchargement de l'archive du jour de chaque public, sans mot de recherche ni numéro de fiche, mais avec votre adresse IP et l'heure, vers `lecomarquage.service-public.gouv.fr` (DILA) ; `LF_SERVICE_PUBLIC=off` coupe cette source. Pour les deux outils de Judilibre : vos mots de recherche et les identifiants de décisions, avec vos identifiants PISTE, vers l'API Judilibre de la Cour de cassation par PISTE, comme pour Légifrance ; `LF_JUDILIBRE=off` coupe cette source.
- **Ce qui reste sur votre machine** : un journal d'audit au format JSONL (`mcp-legifrance.jsonl`, avec rotation), à raison d'une ligne par appel d'outil, quelle qu'en soit l'issue. Il contient une **empreinte** SHA-256 des arguments, jamais le texte de vos requêtes en clair, ni secret. En mode `strict` (défaut), un échec d'écriture fait échouer l'appel. Plusieurs processus peuvent partager le même dossier. Après un arrêt brutal, une ligne coupée reste dans le journal, seule sur sa ligne : l'appel suivant garde sa ligne complète (lecture tolérante : `jq -R 'fromjson?'`).
- **Journaux** : sur stderr uniquement ; stdout ne porte que le protocole. Ni secret, ni texte de requête n'y figure.
- Les textes renvoyés sont des **données**, pas des instructions : un modèle ne doit pas exécuter ce qu'un texte juridique lui demanderait.
- Détails : [PRIVACY.md](PRIVACY.md) (confidentialité) et [SECURITY.md](SECURITY.md) (signalement d'une vulnérabilité par le signalement privé de GitHub, limites connues).

## Ce qui n'est pas encore vérifié

Le projet note ses incertitudes plutôt que de les taire. Pour la 1.6.0 :

- La **production** PISTE n'est exercée qu'à la main par le mainteneur (test de bout en bout de la 1.1.0, le 2026-11-02) : la CI ne joue que le bac à sable, et seulement une partie des tests live chaque semaine.
- La syntaxe de configuration de chaque client est tirée de sa documentation ; elle n'a pas été éprouvée sur une installation réelle (priorité : Claude Code et VS Code). Les formats de Cursor et l'emplacement des fichiers de Claude Desktop sont à confirmer.
- Le bundle MCPB n'a pas encore été essayé dans Claude Desktop sous macOS et Windows.
- `cargo install --git` n'a pas encore été essayé sous macOS et Windows.
- Le chemin par défaut du journal d'audit sous Windows, ses permissions, la prise en compte d'un proxy et le comportement de VS Code face à `openWorldHint` ne sont pas mesurés.
- Le bac à sable PISTE renvoie parfois d'autres données que la production.
- L'index local DILA a été construit une fois sur les archives réelles (1 h 10 avec un binaire de développement) et interrogé sur quelques requêtes, sur Linux seulement, avant la montée de `tantivy` (livrée par la 1.3.0) : depuis, la reconstruction n'a été contrôlée que sur les mini-archives du dépôt, pas sur les archives réelles (ADR-0027). La durée avec le binaire publié, le comportement sous Windows et macOS, la date de l'instantané dans les réponses et la licence propre à chaque fonds ne sont pas vérifiés.
- L'adresse Légifrance qu'un article de loi reçoit dans `source_url` n'a pas été ouverte dans un navigateur pour confirmer qu'elle s'affiche.
- **BOFiP** : le quota du portail n'est connu que par ses en-têtes (50 000 requêtes par jour, portée inconnue) et aucun refus `429` n'a été observé ; ni le pays d'hébergement du portail ni la durée de conservation de ses journaux ne sont publiés ; le portail peut changer de forme sans prévenir (un test hebdomadaire le signale, il ne l'empêche pas) ; le sens de l'opérateur `like`, sur lequel repose la recherche en mots exacts, est mesuré sur le portail, pas lu dans sa documentation (deux tests hebdomadaires le suivent).
- **Service-Public** (depuis la 1.5.0) : la recherche sur les titres et les résumés a mis la fiche attendue dans les cinq premiers résultats pour 18 questions sur 20, des questions nouvelles écrites et jugées à l'aveugle ; une recherche formulée autrement peut la manquer. Mesuré sur le binaire de la branche de code, avant la 1.5.0 : le premier appel prend quelques secondes sur une bonne liaison (3,6 s pour Particuliers), et les deux archives prennent environ 47 Mio de mémoire. Le serveur de la DILA ne publie aucun quota. Le flux peut changer de version (la DILA retire un flux six mois après le suivant) : un test hebdomadaire le signale, il ne l'empêche pas.
- **Judilibre** (depuis la 1.6.0) : l'API est en version bêta ; seule une application de production abonnée a été essayée, et la réponse à une application non abonnée n'a pas été observée. Les intitulés complets des tribunaux sont vérifiés depuis le 2026-11-28 (pour la Cour de cassation, les cours d'appel, les tribunaux judiciaires et de commerce, pas encore pour les conseils de prud'hommes), mais ne sont servis qu'à partir de la version suivante ; l'adresse publique d'une décision sur le site de la Cour de cassation et le quota propre à Judilibre ne sont pas vérifiés ; l'apport aux questions qui manquaient de jurisprudence des juges du fond n'est pas encore mesuré.

Le registre complet des points ouverts est dans [`docs/plan/09-risques-et-questions.md`](docs/plan/09-risques-et-questions.md).

## Développer et contribuer

| | |
|---|---|
| Langage | Rust (édition 2024, MSRV 1.88, chaîne 1.99.0 épinglée), workspace Cargo `crates/lf-*`, binaire `mcp-legifrance` |
| Architecture | hexagonale : `lf-domain` pur → `lf-application` (ports, cas d'usage) → adapters PISTE, DILA, cache, audit, MCP → `lf-app` (binaire) |
| SDK MCP | `rmcp` 3.9.0 |
| Qualité | 100 % de couverture en lignes, régions et fonctions, à une exception nommée près (`stdio.rs`, la colle sur les vrais stdin et stdout, couverte par un test de bout en bout du binaire) ; mutation : le diff de chaque PR est muté sur les fichiers à risque seulement, le reste du code l'est chaque semaine, et des mutants survivants y sont connus et non résolus ; `make ci` est la preuve de complétion |

- **[CONTRIBUTING.md](CONTRIBUTING.md)** : boucle de développement, portes de qualité, règles de licence des contributions.
- **[docs/plan/](docs/plan/)** : point d'entrée du plan et ordre de lecture ; **[docs/plan/00-decisions.md](docs/plan/00-decisions.md)** : décisions normatives ; **[docs/plan/07-feuille-de-route.md](docs/plan/07-feuille-de-route.md)** : jalons et tâches.
- **[docs/adr/](docs/adr/)** : décisions d'architecture ; **[docs/recherche/](docs/recherche/)** : notes de recherche sourcées.
- **[CHANGELOG.md](CHANGELOG.md)** : ce que contient chaque version.
