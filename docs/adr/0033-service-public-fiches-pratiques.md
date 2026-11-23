# ADR-0033 — Service-Public : les fiches pratiques derrière un port distinct, lues en direct, cherchées sur leurs titres sans que les mots quittent la machine

- **Statut** : **Proposé** (accepté à la fusion de la PR qui le porte ; choix du propriétaire du 2026-11-23, pris avant l'écriture de ce document)
- **Date** : 2026-11-23
- **Jalons concernés** : MS (Service-Public), nouveau jalon qui suit la 1.4.1. Deux outils de plus : version mineure, soit la 1.5.0 si le propriétaire retient ce numéro.
- **Questions liées** : Q38 (mode d'accès), Q39 (publics servis), Q40 (lien entre la loi et les fiches) : **décidées par le propriétaire le 2026-11-23, A pour chacune** (voir « Questions tranchées »).
- Amendements : AM-100 (00 §14), qui révise aussi le plafond des instructions fixé par AM-93 (00 §6). Applique la décision 7 d'[ADR-0004](0004-piste-puis-dila.md) (une source par adapter) comme [ADR-0028](0028-bofip-doctrine-fiscale.md) pour le BOFiP. Rien ne change dans les ADR du BOFiP.

## Contexte

- **Le manque.** Service-Public.gouv.fr est le site d'information administrative de l'État, édité par la DILA. Ses fiches pratiques disent en langage courant ce que la loi prévoit pour une situation donnée : « En quoi consiste l'encadrement des loyers à respecter en zone tendue ? », « Contrat de travail temporaire ». Ce n'est pas la loi, mais c'est souvent le point d'entrée d'un non-juriste. La plupart des fiches citent les textes qu'elles résument : les outils de Légifrance peuvent alors lire le texte exact.
- **La source.** Deux jeux de data.gouv.fr publiés par la DILA, Licence Ouverte 2.0, mise à jour quotidienne, flux au format 3.5 :
  - Particuliers (`53699fe5a3a729239d206232`) : archive `vosdroits-latest.zip` de 24 329 745 octets le 2026-11-22 ;
  - Entreprendre (`53699fe5a3a729239d206233`) : archive de 11 056 719 octets.

  Un troisième flux, Associations, existe sur le même serveur (792 704 octets) ; aucun jeu de data.gouv.fr ne l'a fait remonter à la recherche du 2026-11-23.
- **Sonde du 2026-11-23** (environ 25 requêtes publiques, une par seconde au plus, sans compte ni clé, aucun appel à PISTE ; notes hors dépôt) :
  - chaque fichier du flux se lit seul, en direct : `https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/xml/F1314.xml` rend les mêmes octets que l'archive ; de même pour les dossiers (`N….xml`), le menu (`menu.xml`, 31 763 octets) et le catalogue (`idxDateModifImp.xml`, 405 498 octets), et pour le flux Entreprendre (`/vdd/3.5/pro/xml/…`) ;
  - **il n'existe aucune recherche en ligne** : le répertoire du flux ne liste que `xml/` et `zip/`, les niveaux supérieurs répondent 403 ;
  - aucun en-tête de quota n'est servi.
- **Mesure de l'archive Particuliers** (lue sans extraction) :
  - 5 555 fichiers, 145 955 765 octets décompressés : 2 997 fiches (`F…`), 2 306 ressources (`R…` : formulaires, modèles, services en ligne), 243 pages de navigation (`N…` : 232 dossiers, 11 thèmes) et une dizaine d'index ;
  - les fiches : 1 497 fiches d'information, 1 376 questions-réponses, 87 recherches guidées, 22 « comment faire si », 15 fiches de liens ;
  - leur texte : 10 237 caractères en médiane, 40 108 au 95ᵉ centile ; 13 fichiers dépassent 256 Kio, un seul 1 Mio (`F37096`, recherche guidée de 18 771 182 octets) ;
  - aucune déclaration `DOCTYPE` ni `ENTITY` ;
  - beaucoup de contenu est conditionné : `ListeSituations` dans 898 fiches, `BlocCas` dans 1 809 (« Vous êtes locataire », « Vous êtes propriétaire ») ;
  - **2 694 fiches sur 2 997 citent Légifrance** (`dc:source` et éléments `Reference`). Formes d'adresse relevées : `codes/section_lc/LEGITEXT…` (7 741), `codes/id/LEGISCTA…` (7 123), `codes/article_lc/LEGIARTI…` (5 410), `loda/id/JORFTEXT…` (4 553), `loda/article_lc/LEGIARTI…` (1 728), `jorf/id/JORFTEXT…` (1 603), puis `KALITEXT`, `JURITEXT` et d'autres plus rares ;
  - le catalogue donne, pour chaque fiche, son numéro, son type, sa date de dernière modification importante et son titre ; le menu donne l'arbre thème, sous-thème, dossier.
- **Les deux publics.** Entreprendre compte 775 fiches. 106 numéros existent dans les deux flux : même titre et même texte pour les 106, seuls changent l'adresse publique (`www.service-public.gouv.fr/particuliers/…` ou `entreprendre.service-public.gouv.fr/…`) et la place dans le menu.
- **Les conditions de la licence**, écrites sur la page de chaque jeu et dans sa fiche de présentation par la DILA (mise à jour du 1er octobre 2025, lue le 2026-11-23). Le réutilisateur doit mentionner :
  - la paternité, **propre à chaque public** : « Service-Public.gouv.fr/DILA » pour Particuliers, « Entreprendre.Service-Public.gouv.fr/DILA » pour Entreprendre ;
  - l'adresse complète de téléchargement (« l'url d'accès longue ») ;
  - le nom du fichier téléchargé ;
  - sa date.

  Il doit aussi tenir compte des mises à jour que la DILA annonce dans le forum de discussion du jeu.
- **La vie du flux.** La DILA garde au plus deux versions du flux, et l'ancienne disparaît six mois après la parution de la nouvelle (la 3.3 a été retirée le 2026-05-04). Des champs peuvent s'ajouter à tout moment : le consommateur doit ignorer ce qu'il ne connaît pas.
- **Les contraintes du projet.**
  - `LegalCorpus` porte les fonds de Légifrance, `TaxDoctrine` le BOFiP (00 §3) : aucun ne convient à une fiche, qui n'a ni version à une date ni fonds.
  - `reqwest` est confiné à `lf-adapter-piste` et `lf-adapter-bofip`, `quick-xml` à `lf-adapter-dila` (`CONFINED`, `xtask/src/arch.rs`). Les deux sont déjà dans `Cargo.lock`.
  - Les instructions du serveur comptent 3 688 caractères pour un plafond de 3 700 (AM-93).

## Décision

1. **Un port distinct, `PublicGuidance`** (`lf-application::ports`), à deux opérations : `search` (mots, public, page) et `get` (un public et un numéro). `LegalCorpus` et `TaxDoctrine` ne sont pas touchés. Dispatch statique, sans `dyn` (ADR-0012), source active ou coupée choisie au démarrage par un `enum`, comme `AnyDoctrine`. Les erreurs réemploient les codes en place (ADR-0010).
2. **Un crate, `lf-adapter-service-public`**. Ses dépendances internes sont `lf-application` et `lf-domain`, et il ne dépend d'aucun autre adapter. Il a sa ligne dans `RULES`.
   - Dans `CONFINED`, deux portes s'ouvrent à lui : celle de `reqwest` (même pile TLS que le BOFiP) et celle de `quick-xml` (lecture du XML).
   - **Aucun paquet externe nouveau** : `Cargo.lock` ne gagne que le membre interne. `quick-xml` (0.42.0, avec la seule dépendance `memchr`) entre dans le graphe par défaut, alors qu'il n'était tiré que par la feature `dila` ; le verrou et `THIRD_PARTY_LICENSES.md` le listent déjà. Les `wrappers` de `deny.toml` s'ouvrent au nouveau crate pour `reqwest` et `quick-xml`. S'il fallait un paquet externe, le travail s'arrête et le propriétaire décide.
3. **Lecture en direct, recherche locale sur les titres** (Q38 = A).
   - Une fiche ou un dossier se lit par son numéro, fichier par fichier, sur le serveur de la DILA.
   - La recherche ne part pas sur le réseau : elle porte sur un **catalogue** propre à chaque public, le catalogue des fiches (`idxDateModifImp.xml`) joint au menu (`menu.xml`).
   - Le catalogue est téléchargé au premier besoin, gardé en mémoire 24 heures, puis relu. C'est le seul cache de la source.
   - **Les mots de l'appelant ne quittent donc jamais la machine** : le serveur de la DILA ne voit que des numéros de fiche et deux noms de fichier fixes.
   - Ni archive téléchargée, ni index sur disque.
4. **Une requête fermée.**
   - L'hôte `lecomarquage.service-public.gouv.fr`, la version du flux (3.5) et les chemins sont fixés dans le code ; changer de version de flux demande une PR.
   - `https` seul, sans aucune variable de surcharge ni redirection suivie.
   - Le délai et l'échéance de l'appel d'outil sont respectés (`LF_HTTP_TIMEOUT_S`, `Deadline`).
   - La réponse est plafonnée en octets : 2 Mio pour une fiche, un dossier ou un catalogue.
   - L'adresse se construit à partir de valeurs typées par le domaine (public, numéro), jamais par concaténation d'une chaîne de l'appelant.
   - Une fiche plus longue que le plafond n'est pas lue : l'erreur le dit et donne son adresse publique sur service-public.gouv.fr.
5. **Le XML est lu par l'adapter, le texte est rendu par le domaine.**
   - L'adapter lit le fichier avec le lecteur de `quick-xml`, sans `serde`, et le traduit en blocs purs du domaine.
   - Garde-fous de la lecture :
     - un document qui déclare un `DOCTYPE` ou une entité est refusé ;
     - la profondeur et le nombre d'éléments sont bornés ;
     - un élément ou un attribut inconnu est ignoré (engagement de la DILA).
   - Le domaine rend le texte :
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
   - **Fiche** : numéro, public, type de fiche du flux (`sheet_type`, par exemple « Fiche d'information conditionnée »), titre, description, thème, date de dernière modification importante, adresse publique, blocs, références légales.
   - **Dossier** : numéro, titre, sous-dossiers et fiches, chacun avec son titre.
   - **Adresse publique** : rendue seulement si elle suit une grammaire stricte, `https://www.service-public.gouv.fr/particuliers/vosdroits/{numéro}` ou `https://entreprendre.service-public.gouv.fr/vosdroits/{numéro}`. Elle n'est jamais appelée.
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
   - seules les adresses de `www.legifrance.gouv.fr` dont le chemin suit une grammaire stricte sont retenues : fonds, vue, un ou deux identifiants, date et ancre facultatives (118 formes relevées le 2026-11-23, dont `codes/section_lc/LEGITEXT…/LEGISCTA…`) ; l'adresse rendue est reconstruite à partir des parties lues, jamais recopiée. Les autres (arrêtés préfectoraux, documents hébergés ailleurs, `download/pdf?…`) ne sont jamais rendues ;
   - la cible est le **dernier** identifiant du chemin : une référence à une section (`LEGISCTA`) ne donne pas le numéro du code entier ;
   - le numéro est donné quand un outil en place sait le lire :
     - `LEGIARTI` pour `fr_get_text` ;
     - `LEGITEXT` pour `fr_get_act` ;
     - `KALITEXT` pour `fr_get_convention` ;
     - `JURITEXT`, `CETATEXT`, `CONSTEXT` pour `fr_get_decision`.

     Sinon (`JORFTEXT`, `LEGISCTA`, `JORFARTI`…), seuls le titre et l'adresse Légifrance sont donnés ;
   - chaque référence une seule fois, dans l'ordre de première apparition, 50 au plus, `legal_references_truncated` au-delà.

   Le sens inverse, de l'article vers ses fiches par la route PISTE `servicePublicLinksArticle`, n'est pas branché.
9. **Les mentions de la licence et la nature du texte, à chaque réponse.**
   - L'attribution suit le public lu : « Source : Service-Public.gouv.fr / DILA — Licence Ouverte 2.0 » pour Particuliers, « Source : Entreprendre.Service-Public.gouv.fr / DILA — Licence Ouverte 2.0 » pour Entreprendre ; une recherche sur les deux publics porte les deux, dans cet ordre, séparées par « ; ».
   - Une lecture donne en plus :
     - l'adresse complète du fichier lu ;
     - son nom ;
     - sa date, lue dans l'en-tête `Last-Modified` de la réponse, ou à défaut la date de modification portée par la fiche.
   - Une recherche donne les mêmes mentions pour le catalogue.
   - Une note fixe dit que la fiche est une information administrative rédigée pour le public et non la loi. Elle dit aussi que seule la version du jour est servie, et invite à lire les textes de `legal_references` avant de s'appuyer sur un point précis.
10. **Active par défaut, sans réglage** (comme le BOFiP, ADR-0028 décision 8).
    - La variable `LF_SERVICE_PUBLIC` vaut `on` ou `off`, `on` par défaut, casse ignorée.
    - Quand `LF_DILA_DIR` est défini, la source est coupée d'office, et ne revient que par `LF_SERVICE_PUBLIC=on` explicite.
    - Toute autre valeur coupe la source, avec un avis de niveau problème qui ne dégrade pas les outils de Légifrance.
    - Coupée, chaque outil répond `[config_error]` avec un texte fixe, sans E/S.
    - Le bundle MCPB déclare le réglage `service_public` (`on`), comme `bofip` (AM-94).
    - Aucune variable d'adresse, aucun identifiant : la source marche sans configuration PISTE.
11. **Audit, débit, cache.**
    - Une ligne d'audit par appel ; le format du journal ne change pas.
    - Débit : 2 départs de requête par seconde et 2 lectures en vol au plus, 10 000 requêtes par jour (UTC) et par processus, comme le BOFiP (ADR-0029, décision 7).
    - Un refus 429 est rendu `[rate_limited]`, sans nouvelle tentative, puis la source se repose.
    - Le catalogue est gardé en mémoire 24 heures ; aucune fiche n'est gardée.
12. **Couverture.** `fr_coverage` gagne, à la surface v0.3 seulement :
    - une famille `SERVICE_PUBLIC` (`fond`, `label` qui dit la source et la licence, `tools`), comme `BOFIP` ;
    - deux lacunes calculées : FR-108 quand la source est active (recherche sur les titres, version du jour, flux Associations non servi), FR-109 quand elle est coupée.

    Le domaine calcule les lacunes sans connaître la surface ; la vue filtre celles de Service-Public hors de la v0.3, pour que les réponses v0.1 et v0.2 restent identiques à l'octet (05 §5.8).
13. **Hors de cette version** :
    - le flux Associations (Q39 = A) ;
    - un index local du texte des fiches (Q38 = B) ;
    - le lien de l'article vers ses fiches (Q40 = B) ;
    - les ressources (`R…` : formulaires, modèles, services en ligne), dont une fiche ne rend que le titre ;
    - les adresses locales (`OuSAdresser`, `PivotLocal`), qui demandent l'annuaire de l'administration ;
    - la reconnaissance des numéros `F…` par `fr_resolve` et `fr_verify_citations`.
14. **Version mineure, surface élargie.**
    - Deux outils de plus : rubrique « Tool surface » du CHANGELOG, `lock-descriptors --update`.
    - **Le plafond des instructions passe de 3 700 à 4 000 caractères**, pour une ligne de plus en v0.3 (248 caractères dans le projet de 05 §4.2 ; 3 688 mesurés en 1.4.1, soit environ 3 937). Claude Code coupe à 4 096 (00 §6) : le plafond reste 96 caractères sous la coupe.
    - Le plafond de `tools/list` est recalculé à la livraison par la règle d'AM-63 (mesure finale + 3 %, arrondie à la centaine supérieure).

## Questions tranchées

Posées au propriétaire avec une recommandation ; réponse du 2026-11-23 : « A partout ».

| Question | Décision | Options écartées |
|---|---|---|
| **Q38** Mode d'accès | **A** : lecture en direct, recherche locale sur les titres du catalogue | B : index local du texte entier, construit depuis l'archive quotidienne de 24 Mo (recherche plus complète, mais téléchargement à chaque mise à jour et beaucoup plus de code) |
| **Q39** Publics servis | **A** : Particuliers et Entreprendre | B : Particuliers seul ; C : les trois flux, Associations compris |
| **Q40** Lien entre la loi et les fiches | **A** : de la fiche vers la loi seulement, à partir des références de la fiche | B : aussi de l'article vers ses fiches, par la route PISTE `servicePublicLinksArticle` (une requête de plus par lecture d'article, sonde de production préalable) |

## Conséquences

### Positives

- L'explication en langage courant et le texte exact se rejoignent : une fiche lue donne les numéros Légifrance que les outils en place savent lire.
- Les mots d'une recherche restent sur la machine : seul un numéro de fiche part vers la DILA.
- Les deux outils marchent dès l'installation, sans compte, même sans identifiants PISTE.
- Le port laisse la place à un second adapter sur l'archive, comme DILA est venu après PISTE.

### Négatives

- **La recherche ne lit que les titres** : une fiche dont le titre ne porte pas les mots cherchés n'est pas trouvée. Le menu et les dossiers compensent en partie (V-229).
- **Un hôte de plus est contacté** : `lecomarquage.service-public.gouv.fr` reçoit l'adresse IP et les numéros des fiches lues (V-230).
- **Une version de flux a une durée de vie** : à chaque nouvelle version, six mois pour suivre, par une PR.
- **Pas de version passée** : la fiche servie est celle du jour, alors que la règle applicable est celle de la date des faits. La note le dit, elle ne le répare pas.
- **Une obligation de suivi pour le propriétaire** : la licence demande de tenir compte des mises à jour annoncées dans le forum du jeu (V-234).

### À surveiller

- **NON VÉRIFIÉ** [V-229 · sp-recherche-titres] : la recherche sur les titres met-elle la fiche attendue dans les premiers résultats ?
- **NON VÉRIFIÉ** [V-230 · sp-hebergeur-journaux] : qui exploite `lecomarquage.service-public.gouv.fr`, et que journalise-t-il d'une requête ?
- **NON VÉRIFIÉ** [V-231 · sp-quota] : aucun quota n'est publié ni servi dans les en-têtes ; que renvoie un dépassement ?
- **NON VÉRIFIÉ** [V-232 · sp-mentions-licence] : la forme des mentions (date du fichier, adresse de téléchargement) convient-elle pour une lecture fiche par fiche, alors que la page du jeu parle du fichier téléchargé ?
- **NON VÉRIFIÉ** [V-233 · sp-rendu-conditionne] : le rendu des fiches conditionnées et des recherches guidées est-il lisible par un modèle (cas, situations, choix successifs) ?
- **NON VÉRIFIÉ** [V-234 · sp-forum-et-version] : comment suivre le forum du jeu et la parution du flux suivant, et qui le fait ?

### Tests qui la gardent

- **Mode hors ligne et source coupée** : aucune requête. Le proxy témoin des tests du binaire le prouve, avec un contraste : une lecture, source active, ouvre exactement une connexion vers `lecomarquage.service-public.gouv.fr:443`.
- **Mots de recherche** : aucune requête amont ne porte un mot de la recherche (test sur l'adresse de chaque requête du faux serveur).
- **Lecture du XML** : document à `DOCTYPE` ou à entité refusé ; profondeur et nombre d'éléments bornés ; corps plafonné ; test de propriété sur des octets arbitraires (aucune panique, bornes tenues). Une cible de fuzz demanderait `quick-xml` dans `fuzz/`, qui a son propre verrou et passe sous `deny.toml` : hors de cette version.
- **Hôtes** : le test D-04 (`production_sources_contain_only_allowlisted_urls`) admet trois hôtes exacts de plus, en `https` seul : `lecomarquage.service-public.gouv.fr`, `www.service-public.gouv.fr`, `entreprendre.service-public.gouv.fr`.
- **Adresses** : grammaires strictes de l'adresse publique et des références Légifrance, et aucune autre adresse rendue.
- **Surface** : `sha256` des quinze descripteurs en place inchangés dans `tests/tools.lock.json`.
- **En direct** : tests `live_service_public_*`, ignorés par défaut, lancés chaque semaine sans secret.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Index local du texte entier, dès maintenant | Q38 = B écartée : téléchargement de 24 Mo par jour et par public, construction, code de la taille de l'adapter DILA. Reste ouvert derrière le même port. |
| Recherche sur le site service-public.gouv.fr | Pas d'API ; un jeu ouvert existe. Les mots partiraient en plus vers un hôte tiers. |
| Jeu vectorisé de la DINUM (`AgentPublic/service-public`, Hugging Face) | Trimestriel, intermédiaire entre la DILA et l'utilisateur, recherche par plongements sémantiques (*embeddings*) qui demande un modèle. |
| Lien de l'article vers ses fiches par PISTE | Q40 = B écartée : une requête PISTE de plus par lecture, sur une route jamais sondée. |
| Méthodes ajoutées à `LegalCorpus` ou à `TaxDoctrine` | Ni fonds, ni version à une date, ni doctrine : la fiche partagerait des types qui ne sont pas les siens. |
| Variable de surcharge de l'adresse | Surface d'attaque sans besoin : l'adresse du flux est publique et unique. |

## Références et sources

- Jeux : <https://www.data.gouv.fr/datasets/fiches-pratiques-et-ressources-de-service-public-gouv-fr-particuliers> (Particuliers, `53699fe5a3a729239d206232`) et <https://www.data.gouv.fr/datasets/fiches-pratiques-et-ressources-entreprendre-service-public-gouv-fr> (Entreprendre, `53699fe5a3a729239d206233`), lus par l'API `https://www.data.gouv.fr/api/1/datasets/{id}/` le 2026-11-23 ; flux `https://lecomarquage.service-public.gouv.fr/vdd/3.5/{part,pro}/` ; schémas `https://echanges.dila.gouv.fr/OPENDATA/SERVICE-PUBLIC_DTD/schema_3.5.zip`.
- Route PISTE non branchée : [annexe C](../plan/annexes/C-api-legifrance.md) (`/consult/servicePublicLinksArticle`).
- [ADR-0004](0004-piste-puis-dila.md) décision 7 ; [ADR-0006](0006-sortie-compacte-retour-progressif.md) ; [ADR-0010](0010-erreurs-in-band.md) ; [ADR-0012](0012-ports-async-rpitit.md) ; [ADR-0023](0023-article-entier-et-plafond-tools-list.md) ; [ADR-0028](0028-bofip-doctrine-fiscale.md) décisions 4, 8 et 10 ; [ADR-0029](0029-bofip-precisions-apres-mesure.md) décision 7 (débit et budget du jour).
- [00-decisions.md](../plan/00-decisions.md) §1, §2, §3, §6, §7, §11, §14 (AM-63, AM-93, AM-94, AM-100) ; [07-feuille-de-route.md](../plan/07-feuille-de-route.md) §2.12 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.23 (V-229 à V-234).
