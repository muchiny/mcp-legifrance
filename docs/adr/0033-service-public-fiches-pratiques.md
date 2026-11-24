# ADR-0033 — Service-Public : les fiches pratiques derrière un port distinct, lues dans l'archive du jour gardée en mémoire, cherchées sur leurs titres et résumés sans que rien ne quitte la machine

- **Statut** : **Proposé** (accepté à la fusion de la PR qui le porte). Choix du propriétaire du 2026-11-23, pris avant l'écriture de ce document ; Q38 révisée le 2026-11-24 après la mesure de T-MS-03.
- **Date** : 2026-11-23, révisé le 2026-11-24
- **Jalons concernés** : MS (Service-Public), nouveau jalon qui suit la 1.4.1. Deux outils de plus : version mineure, soit la 1.5.0 si le propriétaire retient ce numéro.
- **Questions liées** :
  - Q38 (mode d'accès) : A le 2026-11-23, puis **B le 2026-11-24**, après la mesure de la recherche (voir « Questions tranchées ») ;
  - Q39 (publics servis) et Q40 (lien entre la loi et les fiches) : **A**, décidées le 2026-11-23.
- Amendements : AM-100 (00 §14), qui révise aussi le plafond des instructions fixé par AM-93 (00 §6). Applique la décision 7 d'[ADR-0004](0004-piste-puis-dila.md) (une source par adapter) comme [ADR-0028](0028-bofip-doctrine-fiscale.md) pour le BOFiP. Rien ne change dans les ADR du BOFiP.

## Contexte

- **Le manque.** Service-Public.gouv.fr est le site d'information administrative de l'État, édité par la DILA. Ses fiches pratiques disent en langage courant ce que la loi prévoit pour une situation donnée : « En quoi consiste l'encadrement des loyers à respecter en zone tendue ? », « Contrat de travail temporaire ». Ce n'est pas la loi, mais c'est souvent le point d'entrée d'un non-juriste. La plupart des fiches citent les textes qu'elles résument : les outils de Légifrance peuvent alors lire le texte exact.
- **La source.** Deux jeux de data.gouv.fr publiés par la DILA, Licence Ouverte 2.0, mise à jour quotidienne, flux au format 3.5 :
  - Particuliers (`53699fe5a3a729239d206232`) : archive `vosdroits-latest.zip`, 24 329 745 octets le 2026-11-22, 24 332 119 le 2026-11-24 ;
  - Entreprendre (`53699fe5a3a729239d206233`) : 11 056 719 octets, puis 11 058 942.

  Un troisième flux, Associations, existe sur le même serveur (792 704 octets) ; aucun jeu de data.gouv.fr ne l'a fait remonter à la recherche du 2026-11-23.
- **Sondes des 2026-11-23 et 2026-11-24** (environ 50 requêtes publiques, une par seconde au plus, sans compte ni clé, aucun appel à PISTE ; notes hors dépôt) :
  - chaque fichier du flux se lit seul : `https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/xml/F1314.xml` rend les mêmes octets que l'archive ; de même pour les dossiers (`N….xml`), le menu (`menu.xml`), le catalogue (`idxDateModifImp.xml`) et le flux Entreprendre (`/vdd/3.5/pro/…`) ;
  - **il n'existe aucune recherche en ligne** : le répertoire du flux ne liste que `xml/` et `zip/`, les niveaux supérieurs répondent 403 ;
  - l'archive est servie avec `Last-Modified`, `ETag` et `Accept-Ranges`, régénérée chaque jour ; aucun cookie, aucun en-tête de quota.
- **Mesure de l'archive Particuliers** (lue sans extraction) :
  - 5 555 fichiers, 145 955 765 octets décompressés : 2 997 fiches (`F…`), 2 306 ressources (`R…` : formulaires, modèles, services en ligne), 243 pages de navigation (`N…` : 232 dossiers, 11 thèmes) et neuf index ;
  - **format** : chaque entrée compressée en `deflate`, aucune extension zip64, aucun chiffrement, aucun descripteur de données, aucun commentaire ; de même pour Entreprendre (2 665 entrées) ;
  - les fiches : 1 497 fiches d'information, 1 376 questions-réponses, 87 recherches guidées, 22 « comment faire si », 15 fiches de liens ;
  - leur texte : 10 237 caractères en médiane, 40 108 au 95ᵉ centile ; 13 fichiers dépassent 256 Kio, un seul 1 Mio (`F37096`, recherche guidée de 18 771 182 octets) ;
  - aucune déclaration `DOCTYPE` ni `ENTITY` ;
  - beaucoup de contenu est conditionné : `ListeSituations` dans 898 fiches, `BlocCas` dans 1 809 (« Vous êtes locataire », « Vous êtes propriétaire ») ;
  - **2 694 fiches sur 2 997 citent Légifrance** (`dc:source` et éléments `Reference`). Formes d'adresse relevées : `codes/section_lc/LEGITEXT…` (7 741), `codes/id/LEGISCTA…` (7 123), `codes/article_lc/LEGIARTI…` (5 410), `loda/id/JORFTEXT…` (4 553), `loda/article_lc/LEGIARTI…` (1 728), `jorf/id/JORFTEXT…` (1 603), puis `KALITEXT`, `JURITEXT` et d'autres plus rares.
- **Les petits index servis en direct sont incomplets.** Le catalogue `idxDateModifImp.xml` ne liste que 1 740 fiches sur 2 997 (58 %). Il manque :
  - 712 questions-réponses ;
  - 443 fiches d'information ;
  - les 87 recherches guidées.

  `questionsReponses.xml` et `commentFaireSi.xml` portent ce total à 2 051 (68 %). Seule `arborescence.xml` (25 Mo, titres seuls) les liste toutes.
- **Mesure de la recherche** (T-MS-03, 2026-11-23, protocole écrit avant les résultats ; questions jamais publiées) :
  - 20 questions de non-juriste (14 Particuliers, 6 Entreprendre) ; un agent les a écrites, avec leurs requêtes, sans voir aucun catalogue ;
  - un second agent a désigné la bonne fiche de chacune dans l'archive entière, sans voir les requêtes ;
  - succès : une fiche attendue dans les cinq premiers résultats ; seuil : 15 sur 20.

  | Recherche | Succès |
  |---|---|
  | Q38 = A : chaque mot doit commencer un mot du titre, catalogue et menu | **13 / 20** ; 13 aussi avec les petits index ou avec tous les titres, quel que soit l'ordre |
  | Un seul mot suffit, petits index servis en direct | 15 / 20 |
  | Titres et résumés de toutes les fiches et pages, archive du jour | **17 / 20** |
  | Texte entier de toutes les fiches, archive du jour | 17 / 20 avec les pages de navigation, 18 / 20 sans elles |

  Les trois dernières lignes ont été conçues après la première : elles orientent le choix, elles ne le prouvent pas. Sur 20 questions, un écart de deux questions peut tenir au hasard. T-MS-46 refait la mesure sur 20 questions nouvelles (V-229).
- **Les deux publics.** Entreprendre compte 775 fiches. 106 numéros existent dans les deux flux : même titre et même texte pour les 106, seuls changent l'adresse publique (`www.service-public.gouv.fr/particuliers/…` ou `entreprendre.service-public.gouv.fr/…`) et la place dans le menu.
- **Les conditions de la licence**, écrites sur la page de chaque jeu et dans sa fiche de présentation par la DILA (mise à jour du 1er octobre 2025, lue le 2026-11-23). Le réutilisateur doit mentionner :
  - la paternité, **propre à chaque public** : « Service-Public.gouv.fr/DILA » pour Particuliers, « Entreprendre.Service-Public.gouv.fr/DILA » pour Entreprendre ;
  - « l'url d'accès longue de téléchargement » ;
  - « le nom du fichier téléchargé ainsi que la date du fichier ».

  Il doit aussi tenir compte des mises à jour que la DILA annonce dans le forum de discussion du jeu.
- **La vie du flux.** La DILA garde au plus deux versions du flux, et l'ancienne disparaît six mois après la parution de la nouvelle (la 3.3 a été retirée le 2026-05-04). Des champs peuvent s'ajouter à tout moment : le consommateur doit ignorer ce qu'il ne connaît pas.
- **Les contraintes du projet.**
  - `LegalCorpus` porte les fonds de Légifrance, `TaxDoctrine` le BOFiP (00 §3) : aucun ne convient à une fiche, qui n'a ni version à une date ni fonds.
  - Dans `CONFINED` (`xtask/src/arch.rs`), `reqwest` est confiné à `lf-adapter-piste` et `lf-adapter-bofip`, `quick-xml` et `flate2` à `lf-adapter-dila`. Les trois sont déjà dans `Cargo.lock`, et `flate2` est déjà dans le graphe par défaut, par `reqwest`.
  - Aucun lecteur du format zip n'est dans `Cargo.lock`.
  - Les instructions du serveur comptent 3 688 caractères pour un plafond de 3 700 (AM-93).
  - Délai d'un appel d'outil : 60 s par défaut (`LF_TOOL_DEADLINE_S`).

## Décision

1. **Un port distinct, `PublicGuidance`** (`lf-application::ports`), à deux opérations : `search` (mots, public, page) et `get` (un public et un numéro). `LegalCorpus` et `TaxDoctrine` ne sont pas touchés. Dispatch statique, sans `dyn` (ADR-0012), source active ou coupée choisie au démarrage par un `enum`, comme `AnyDoctrine`. Les erreurs réemploient les codes en place (ADR-0010).
2. **Un crate, `lf-adapter-service-public`**. Ses dépendances internes sont `lf-application` et `lf-domain`, et il ne dépend d'aucun autre adapter. Il a sa ligne dans `RULES`.
   - Dans `CONFINED`, trois portes s'ouvrent à lui :
     - `reqwest`, même pile TLS que le BOFiP ;
     - `quick-xml`, pour lire le XML ;
     - `flate2`, pour décompresser les entrées de l'archive.
   - **Aucun paquet externe nouveau** : `Cargo.lock` ne gagne que le membre interne.
     - `quick-xml` (0.42.0, avec la seule dépendance `memchr`) entre dans le graphe par défaut, alors qu'il n'était tiré que par la feature `dila` ; le verrou et `THIRD_PARTY_LICENSES.md` le listent déjà.
     - `flate2` y était déjà.
     - Les `wrappers` de `deny.toml` s'ouvrent au nouveau crate pour ces trois paquets.
     - S'il fallait un paquet externe, le travail s'arrête et le propriétaire décide.
3. **L'archive du jour, gardée en mémoire ; une recherche locale sur les titres et les résumés** (Q38 = B, révisée le 2026-11-24).
   - **Téléchargement**, pour chaque public :
     - l'archive `vosdroits-latest.zip` est téléchargée au premier besoin, par une seule requête ;
     - elle est gardée en mémoire, jamais écrite sur disque ;
     - au bout de 24 heures, elle est revalidée par une requête conditionnelle (`If-None-Match` avec l'`ETag` reçu). Un `304` la garde et repart pour 24 heures.
   - **Un seul téléchargement à la fois par public**, partagé par les appels qui l'attendent.
     - Il tourne dans sa propre tâche, borné par son propre délai, qui vaut 300 s.
     - Un appel qui atteint son échéance avant la fin rend `upstream_error`. Le message dit que l'archive est en cours de téléchargement et de réessayer dans une minute. Le téléchargement continue, et l'appel suivant s'en sert.
   - **Revalidation échouée** : l'archive en mémoire reste servie, 48 heures de plus au plus. Sa date, donnée par les mentions (décision 9), le montre. Au-delà, la source répond `upstream_error`.
   - **Index en mémoire**, construit au chargement de l'archive, pour chaque fiche (`F…`) et chaque page de navigation (`N…`). Les ressources (`R…`) sont exclues. Il garde :
     - le numéro, la nature et le type de fiche ;
     - le titre et le résumé (`dc:description`) ;
     - le thème (`dc:subject`) et la date.

     Le menu (`menu.xml`) donne l'arbre des thèmes et des dossiers.
   - **Recherche** :
     - classement BM25 sur le titre (poids 3) et le résumé, comme la mesure de T-MS-03 ;
     - un mot de la requête correspond à tout mot de l'index qu'il commence, sans casse ni accents ;
     - une courte liste fermée de mots vides est retirée de la requête ;
     - à score égal, numéro croissant.
   - **Lecture** : une fiche ou un dossier est lu dans l'archive en mémoire, sans requête.
   - **Rien ne quitte la machine** : ni les mots d'une recherche, ni le numéro d'une fiche lue. Le serveur de la DILA ne voit qu'un téléchargement de l'archive par public et par 24 heures, à une adresse fixe.
   - Aucun index ni archive sur disque. Chaque processus télécharge sa propre archive.
4. **Une requête fermée.**
   - L'hôte `lecomarquage.service-public.gouv.fr`, la version du flux (3.5) et les deux chemins (`/vdd/3.5/part/zip/vosdroits-latest.zip`, `/vdd/3.5/pro/zip/vosdroits-latest.zip`) sont fixés dans le code. Changer de version de flux demande une PR.
   - `https` seul, sans aucune variable de surcharge ni redirection suivie.
   - Le corps est plafonné à 64 Mio. Le flux Particuliers en mesure 24.
   - L'adresse se choisit à partir du public typé par le domaine, jamais à partir d'une chaîne de l'appelant.
5. **L'archive et le XML sont lus par l'adapter, le texte est rendu par le domaine.**
   - **Lecteur d'archive** : écrit dans le crate, pour le seul sous-ensemble du format que sert la DILA. Toute autre forme fait refuser l'archive entière, et l'archive précédente reste servie. Les règles :
     - le répertoire central est cherché à la fin du fichier et recoupé avec l'en-tête local de chaque entrée lue ;
     - 20 000 entrées au plus ;
     - noms de la forme `[A-Za-z0-9]+.xml` seulement, sans chemin ;
     - compression `deflate` ou aucune ; zip64, chiffrement et descripteurs de données refusés ;
     - **décompression bornée** : 2 Mio au plus pour une entrée lue en entier, 64 Kio pour la lecture de l'en-tête d'une fiche quand l'index se construit. La décompression s'arrête au premier octet en trop ;
     - la somme CRC-32 d'une entrée lue en entier est vérifiée ;
     - rien n'est écrit sur disque.
   - **Une fiche plus longue que le plafond** n'est pas lue : l'erreur le dit et donne son adresse publique sur service-public.gouv.fr.
   - **Lecture du XML** : lecteur de `quick-xml`, sans `serde`, qui traduit le fichier en blocs purs du domaine. Garde-fous :
     - un document qui déclare un `DOCTYPE` ou une entité est refusé ;
     - la profondeur et le nombre d'éléments sont bornés ;
     - un élément ou un attribut inconnu est ignoré (engagement de la DILA).
   - **Le domaine rend le texte** :
     - titres de chapitre ;
     - paragraphes ;
     - listes ;
     - tableaux, ligne par ligne ;
     - situations et cas, chacun annoncé par son intitulé ;
     - encadrés « À savoir » et « Attention », annoncés comme tels.
   - Un lien vers une autre fiche garde son numéro, pour que l'appelant puisse le suivre.
6. **Les types du domaine, purs.**
   - **Numéro** : `F` ou `N` suivi de 1 à 7 chiffres, validé avant tout appel.
   - **Public** : `particuliers` ou `entreprendre`.
   - **Nature d'une entrée** : `sheet` (fiche), `folder` (dossier) ou `theme` (thème), la même valeur partout.
   - **Fiche** : numéro, public, type de fiche du flux (`sheet_type`, par exemple « Fiche d'information conditionnée »), titre, description, thème, date, adresse publique, blocs, références légales.
   - **Dossier** : numéro, titre, sous-dossiers et fiches, chacun avec son titre.
   - **Adresse publique** : rendue seulement si elle suit une grammaire stricte, `https://www.service-public.gouv.fr/particuliers/vosdroits/{numéro}` ou `https://entreprendre.service-public.gouv.fr/vosdroits/{numéro}`. Elle n'est jamais appelée.
   - **Index et classement** : fonctions pures du domaine. L'adapter leur donne les entrées lues.
7. **Deux outils, `fr_search_service_public` et `fr_get_service_public`**, en lecture seule.
   - **La recherche** :
     - elle rend numéro, public, nature (`sheet`, `folder`, `theme`), titre, date et adresse publique, jamais le texte ;
     - le public vaut `particuliers` par défaut, sinon `entreprendre` ou `all` ;
     - avec `all`, un numéro présent dans les deux flux n'est rendu qu'une fois.
   - **La lecture** :
     - une fiche suit le retour progressif de `fr_get_text` (résumé, `detail`, `offset`, `focus`, ADR-0006) ;
     - un dossier rend la liste de ses fiches ;
     - un numéro absent du public demandé rend `not_found`, et le message nomme l'autre public.
   - `fr_search_all` reste sur les fonds de Légifrance.
8. **De la fiche vers la loi seulement** (Q40 = A). Une fiche lue porte `legal_references` : ses références vers Légifrance, tirées des éléments `Reference` et de `dc:source`. Règles :
   - **Adresses retenues** : seules celles de `www.legifrance.gouv.fr` dont le chemin suit une grammaire stricte.
     - Le chemin compte un fonds, une vue, un ou deux identifiants, une date et une ancre facultatives : 118 formes relevées le 2026-11-23, dont `codes/section_lc/LEGITEXT…/LEGISCTA…`.
     - Une requête est ignorée, par exemple `?idSecParent=…`, présente sur plus d'une centaine de liens vers des articles.
     - L'adresse rendue est reconstruite à partir des parties lues, jamais recopiée.
     - `dc:source` est une liste d'adresses séparées par des virgules.
     - Les autres adresses ne sont jamais rendues : arrêtés préfectoraux, documents hébergés ailleurs, `download/pdf?id=…`, `circulaire/id/…`.
   - **Cible** : le **dernier** identifiant du chemin. Une référence à une section (`LEGISCTA`) ne donne pas le numéro du code entier.
   - **Numéro** : donné quand un outil en place sait le lire :
     - `LEGIARTI` pour `fr_get_text` ;
     - `LEGITEXT` pour `fr_get_act` ;
     - `KALITEXT` pour `fr_get_convention` ;
     - `JURITEXT`, `CETATEXT`, `CONSTEXT` pour `fr_get_decision`.

     Sinon (`JORFTEXT`, `LEGISCTA`, `JORFARTI`…), seuls le titre et l'adresse Légifrance sont donnés.
   - **Liste** : chaque référence une seule fois, dans l'ordre de première apparition, 50 au plus, `legal_references_truncated` au-delà.

   Le sens inverse, de l'article vers ses fiches par la route PISTE `servicePublicLinksArticle`, n'est pas branché.
9. **Les mentions de la licence et la nature du texte, à chaque réponse.**
   - **Attribution** : elle suit le public lu.
     - Particuliers : « Source : Service-Public.gouv.fr / DILA — Licence Ouverte 2.0 ».
     - Entreprendre : « Source : Entreprendre.Service-Public.gouv.fr / DILA — Licence Ouverte 2.0 ».
     - Une recherche sur les deux publics porte les deux, dans cet ordre, séparées par « ; ».
   - **Fichier téléchargé** : chaque réponse le décrit, dans les termes de la DILA (`source_file`, ou `source_files` pour une recherche sur les deux publics) :
     - l'adresse complète de l'archive : `url`, « l'url d'accès longue de téléchargement » ;
     - son nom : `name`, `vosdroits-latest.zip` ;
     - sa date : `date`, lue dans l'en-tête `Last-Modified` reçu.

     Une lecture nomme en plus l'entrée lue (`F1234.xml`).
   - **Note fixe** : la fiche est une information administrative rédigée pour le public, et non la loi. La note dit aussi :
     - que seule la version du jour est servie ;
     - qu'il faut lire les textes de `legal_references` avant de s'appuyer sur un point précis.
10. **Active par défaut, sans réglage** (comme le BOFiP, ADR-0028 décision 8).
    - La variable `LF_SERVICE_PUBLIC` vaut `on` ou `off`, `on` par défaut, casse ignorée.
    - Quand `LF_DILA_DIR` est défini, la source est coupée d'office, et ne revient que par `LF_SERVICE_PUBLIC=on` explicite.
    - Toute autre valeur coupe la source, avec un avis de niveau problème qui ne dégrade pas les outils de Légifrance.
    - Coupée, chaque outil répond `[config_error]` avec un texte fixe, sans E/S.
    - Le bundle MCPB déclare le réglage `service_public` (`on`), comme `bofip` (AM-94).
    - Aucune variable d'adresse, aucun identifiant : la source marche sans configuration PISTE.
11. **Audit, débit, mémoire.**
    - Une ligne d'audit par appel ; le format du journal ne change pas.
    - Débit : un téléchargement en vol au plus par public, une revalidation par public et par 24 heures ; un appel d'outil n'émet aucune autre requête.
    - Un refus 429 est rendu `[rate_limited]`, sans nouvelle tentative. La source se repose, et l'archive en mémoire reste servie.
    - Mémoire : environ 35 Mio d'archives quand les deux publics ont servi, plus l'index, à mesurer (V-235). Rien n'est chargé tant qu'aucun des deux outils n'est appelé.
12. **Couverture.** `fr_coverage` gagne, à la surface v0.3 seulement :
    - une famille `SERVICE_PUBLIC` (`fond`, `label` qui dit la source et la licence, `tools`), comme `BOFIP` ;
    - deux lacunes calculées :
      - FR-108 quand la source est active : recherche sur les titres et résumés et non sur le texte, version du jour, archive téléchargée à la première recherche, flux Associations non servi ;
      - FR-109 quand elle est coupée.

    Le domaine calcule les lacunes sans connaître la surface ; la vue filtre celles de Service-Public hors de la v0.3, pour que les réponses v0.1 et v0.2 restent identiques à l'octet (05 §5.8).
13. **Hors de cette version** :
    - le flux Associations (Q39 = A) ;
    - la recherche dans le texte entier : un succès de plus sur 20 sans les pages de navigation, aucun avec elles, pour un index bien plus lourd ;
    - une archive gardée sur disque et partagée entre processus ;
    - le lien de l'article vers ses fiches (Q40 = B) ;
    - les ressources (`R…` : formulaires, modèles, services en ligne), dont une fiche ne rend que le titre ;
    - les adresses locales (`OuSAdresser`, `PivotLocal`), qui demandent l'annuaire de l'administration ;
    - la reconnaissance des numéros `F…` par `fr_resolve` et `fr_verify_citations`.
14. **Version mineure, surface élargie.**
    - Deux outils de plus : rubrique « Tool surface » du CHANGELOG, `lock-descriptors --update`.
    - **Le plafond des instructions passe de 3 700 à 4 000 caractères**, pour une ligne de plus en v0.3. Cette ligne compte 248 caractères dans le projet de 05 §4.2 ; avec les 3 688 mesurés en 1.4.1, le total atteint environ 3 937. Claude Code coupe à 4 096 (00 §6) : le plafond reste 96 caractères sous la coupe.
    - Le plafond de `tools/list` est recalculé à la livraison par la règle d'AM-63 (mesure finale + 3 %, arrondie à la centaine supérieure).

## Questions tranchées

Posées au propriétaire avec une recommandation. Première réponse le 2026-11-23 : « A partout ». Q38 a été reposée le 2026-11-24, avec la mesure de T-MS-03 ; le propriétaire a répondu « continue » à la recommandation, l'option 2.

| Question | Décision | Options écartées |
|---|---|---|
| **Q38** Mode d'accès | **B**, révisée le 2026-11-24 : archive du jour en mémoire, index local des titres et résumés, fiches lues dans l'archive | A, choisie le 2026-11-23 : lecture en direct et recherche sur les titres du catalogue ; 13 succès sur 20, catalogue à 58 %. A assouplie : un mot suffit, petits index ; 15 sur 20, index à 68 %. Texte entier : voir la décision 13 |
| **Q39** Publics servis | **A** : Particuliers et Entreprendre | B : Particuliers seul ; C : les trois flux, Associations compris |
| **Q40** Lien entre la loi et les fiches | **A** : de la fiche vers la loi seulement, à partir des références de la fiche | B : aussi de l'article vers ses fiches, par la route PISTE `servicePublicLinksArticle` (une requête de plus par lecture d'article, sonde de production préalable) |

## Conséquences

### Positives

- L'explication en langage courant et le texte exact se rejoignent : une fiche lue donne les numéros Légifrance que les outils en place savent lire.
- **Rien de ce que l'on cherche ni de ce que l'on lit ne part vers la DILA** : elle ne voit qu'un téléchargement par public et par jour.
- Toutes les fiches et pages de navigation sont cherchables, et pas seulement les 58 % du catalogue.
- Après le premier téléchargement, chaque appel répond sans réseau.
- Les mentions exigées par la DILA (adresse, nom et date du fichier téléchargé) s'appliquent telles quelles.
- Les deux outils marchent dès l'installation, sans compte, même sans identifiants PISTE.

### Négatives

- **La première recherche de la journée télécharge l'archive** : 24 Mo pour Particuliers, 11 Mo pour Entreprendre. Sur une liaison lente, elle peut dépasser l'échéance de l'appel ; l'appel suivant s'en sert (V-235).
- **De la mémoire en plus** : environ 35 Mio d'archives quand les deux publics ont servi, plus l'index.
- **Un lecteur d'archive à maintenir** : c'est du code qui lit une entrée venue du réseau. D'où les bornes de la décision 5 et ses tests.
- **La recherche ne lit pas le texte** : une fiche dont ni le titre ni le résumé ne portent les mots cherchés n'est pas trouvée. Les dossiers compensent en partie.
- **Une version de flux a une durée de vie** : à chaque nouvelle version, six mois pour suivre, par une PR.
- **Pas de version passée** : la fiche servie est celle du jour, alors que la règle applicable est celle de la date des faits. La note le dit, elle ne le répare pas.
- **Une obligation de suivi pour le propriétaire** : la licence demande de tenir compte des mises à jour annoncées dans le forum du jeu (V-234).

### À surveiller

- **NON VÉRIFIÉ** [V-229 · sp-recherche-titres] : la recherche sur les titres et résumés met-elle la fiche attendue dans les cinq premiers résultats ? Mesurée à 17 sur 20 le 2026-11-23, après coup ; une confirmation sur 20 questions nouvelles est faite à la livraison (T-MS-46), seuil 15.
- **NON VÉRIFIÉ** [V-230 · sp-hebergeur-journaux] : que journalise l'hôte d'un téléchargement ? La DILA dit garder ses journaux de connexion un an pour son site (page « Données personnelles et sécurité », lue le 2026-11-23) ; le sous-domaine du flux n'y est pas nommé.
- **NON VÉRIFIÉ** [V-231 · sp-quota] : aucun quota n'est publié ni servi dans les en-têtes ; que renvoie un dépassement ?
- **NON VÉRIFIÉ** [V-232 · sp-mentions-licence] : la forme des mentions (adresse, nom et date de l'archive, entrée lue) convient-elle ? Elle reprend les termes de la fiche de présentation de la DILA.
- **NON VÉRIFIÉ** [V-233 · sp-rendu-conditionne] : le rendu des fiches conditionnées et des recherches guidées est-il lisible par un modèle (cas, situations, choix successifs) ?
- **NON VÉRIFIÉ** [V-234 · sp-forum-et-version] : comment suivre le forum du jeu et la parution du flux suivant, et qui le fait ?
- **NON VÉRIFIÉ** [V-235 · sp-archive-memoire-delai] : combien de temps prend le téléchargement de l'archive, et combien de mémoire l'archive et l'index prennent-ils, dans le binaire ?

### Tests qui la gardent

- **Mode hors ligne et source coupée** : aucune requête.
  - Le proxy témoin des tests du binaire le prouve, avec un contraste : source active, une première lecture ouvre exactement une connexion vers `lecomarquage.service-public.gouv.fr:443`, et une seconde lecture n'en ouvre aucune.
- **Mots de recherche et numéros** : aucune requête amont ne porte un mot d'une recherche ni un numéro de fiche. Le test lit l'adresse de chaque requête reçue par le faux serveur : seuls les deux chemins fixes y apparaissent.
- **Lecteur d'archive** :
  - archives mal formées : zip64, chiffrement, nom avec chemin, en-tête local qui contredit le répertoire central, somme CRC fausse, entrée qui gonfle au-delà de son plafond ;
  - test de propriété sur des octets arbitraires : aucune panique, bornes tenues ;
  - l'archive précédente reste servie quand la nouvelle est refusée.
- **Lecture du XML** : document à `DOCTYPE` ou à entité refusé ; profondeur et nombre d'éléments bornés ; test de propriété sur des octets arbitraires. Une cible de fuzz demanderait `quick-xml` dans `fuzz/`, qui a son propre verrou et passe sous `deny.toml` : hors de cette version.
- **Hôtes** : le test D-04 (`production_sources_contain_only_allowlisted_urls`) admet trois hôtes exacts de plus, en `https` seul : `lecomarquage.service-public.gouv.fr`, `www.service-public.gouv.fr`, `entreprendre.service-public.gouv.fr`.
- **Adresses** : grammaires strictes de l'adresse publique et des références Légifrance, et aucune autre adresse rendue.
- **Surface** : `sha256` des quinze descripteurs en place inchangés dans `tests/tools.lock.json`.
- **En direct** : tests `live_service_public_*`, ignorés par défaut, lancés chaque semaine sans secret.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Lecture en direct, recherche sur les titres du catalogue (Q38 = A du 2026-11-23) | Mesurée à 13 sur 20, sous le seuil de 15 ; le catalogue ne liste que 58 % des fiches. Le serveur de la DILA aurait vu le numéro de chaque fiche lue. |
| Même lecture, recherche où un mot suffit, sur les petits index servis en direct | 15 sur 20, au seuil juste et mesuré après coup ; 32 % des fiches hors de l'index. |
| Recherche dans le texte entier | Voir la décision 13 : gain nul ou d'un succès sur 20, pour un index bien plus lourd. Reste ouverte derrière le même port. |
| Paquet externe pour lire le format zip | Paquet nouveau, contraire à G11. La DILA ne sert qu'un sous-ensemble du format, mesuré : `deflate`, sans zip64 ni chiffrement. Il tient dans un lecteur borné, sur `flate2` déjà présent. |
| Archive gardée sur disque | Demande un chemin, des écritures atomiques et le partage entre processus. La mémoire suffit pour cette version. |
| Recherche sur le site service-public.gouv.fr | Pas d'API ; un jeu ouvert existe. Les mots partiraient en plus vers un hôte tiers. |
| Jeu vectorisé de la DINUM (`AgentPublic/service-public`, Hugging Face) | Trimestriel, intermédiaire entre la DILA et l'utilisateur, recherche par plongements sémantiques (*embeddings*) qui demande un modèle. |
| Lien de l'article vers ses fiches par PISTE | Q40 = B écartée : une requête PISTE de plus par lecture, sur une route jamais sondée. |
| Méthodes ajoutées à `LegalCorpus` ou à `TaxDoctrine` | Ni fonds, ni version à une date, ni doctrine : la fiche partagerait des types qui ne sont pas les siens. |
| Variable de surcharge de l'adresse | Surface d'attaque sans besoin : l'adresse du flux est publique et unique. |

## Références et sources

- Jeux : <https://www.data.gouv.fr/datasets/fiches-pratiques-et-ressources-de-service-public-gouv-fr-particuliers> (Particuliers, `53699fe5a3a729239d206232`) et <https://www.data.gouv.fr/datasets/fiches-pratiques-et-ressources-entreprendre-service-public-gouv-fr> (Entreprendre, `53699fe5a3a729239d206233`), lus par l'API `https://www.data.gouv.fr/api/1/datasets/{id}/` le 2026-11-23 ; flux `https://lecomarquage.service-public.gouv.fr/vdd/3.5/{part,pro}/` ; schémas `https://echanges.dila.gouv.fr/OPENDATA/SERVICE-PUBLIC_DTD/schema_3.5.zip`.
- Fiches de présentation de la DILA (1er octobre 2025) : `https://echanges.dila.gouv.fr/OPENDATA/SERVICE-PUBLIC/DILA_PARTICULIERS_SERVICE-PUBLIC_GOUV_FR_Presentation_20251001.pdf` et `https://echanges.dila.gouv.fr/OPENDATA/ENTREPRENDRE_SERVICE-PUBLIC/DILA_ENTREPRENDRE_SERVICE-PUBLIC_GOUV_FR_Presentation_20251001.pdf`.
- Mentions légales et données personnelles du site : `https://www.service-public.gouv.fr/P10025` et `https://www.service-public.gouv.fr/P10001`, lues le 2026-11-23.
- `flate2` 1 (décompression `deflate` brute, `Crc`) : documentation lue par context7 le 2026-11-24.
- Route PISTE non branchée : [annexe C](../plan/annexes/C-api-legifrance.md) (`/consult/servicePublicLinksArticle`).
- [ADR-0004](0004-piste-puis-dila.md) décision 7 ; [ADR-0006](0006-sortie-compacte-retour-progressif.md) ; [ADR-0010](0010-erreurs-in-band.md) ; [ADR-0012](0012-ports-async-rpitit.md) ; [ADR-0023](0023-article-entier-et-plafond-tools-list.md) ; [ADR-0028](0028-bofip-doctrine-fiscale.md) décisions 4, 8 et 10 ; [ADR-0029](0029-bofip-precisions-apres-mesure.md) décision 7.
- [00-decisions.md](../plan/00-decisions.md) §1, §2, §3, §6, §7, §11, §14 (AM-63, AM-93, AM-94, AM-100) ; [07-feuille-de-route.md](../plan/07-feuille-de-route.md) §2.12 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.23 (V-229 à V-235).
