# ADR-0033 — Service-Public : les fiches pratiques derrière un port distinct, lues dans l'archive du jour gardée en mémoire, cherchées sur leurs titres et résumés sans que rien ne quitte la machine

- **Statut** : **Proposé** (accepté à la fusion de la PR qui le porte). Choix du propriétaire du 2026-11-23 ; Q38 révisée le 2026-11-24 après la mesure de T-MS-03.
- **Date** : 2026-11-23, révisé le 2026-11-24
- **Jalons concernés** : MS (Service-Public), après la 1.4.1. Deux outils de plus : version mineure, 1.5.0 proposée.
- **Questions liées** : Q38 (mode d'accès) : A le 2026-11-23, **B le 2026-11-24** ; Q39 (publics) et Q40 (lien entre la loi et les fiches) : **A**, le 2026-11-23.
- **Amendements** : AM-100 (00 §14), qui révise le plafond des instructions d'AM-93 (00 §6). Applique la décision 7 d'[ADR-0004](0004-piste-puis-dila.md) (une source par adapter), comme [ADR-0028](0028-bofip-doctrine-fiscale.md) pour le BOFiP. Rien ne change dans les ADR du BOFiP.

## Contexte

- **Le manque.** Les fiches pratiques de Service-Public.gouv.fr (DILA) disent en langage courant ce que la loi prévoit pour une situation donnée. Ce n'est pas la loi, mais c'est souvent le point d'entrée d'un non-juriste, et la plupart citent les textes qu'elles résument.
- **La source.** Deux jeux de data.gouv.fr de la DILA (Licence Ouverte 2.0, flux 3.5, mis à jour chaque jour) : Particuliers (`53699fe5a3a729239d206232`) et Entreprendre (`53699fe5a3a729239d206233`). Chacun publie une archive quotidienne, `vosdroits-latest.zip` (24,3 Mo et 11,1 Mo le 2026-11-24, avec `Last-Modified` et `ETag`). Chaque fichier se lit aussi seul ; **aucune recherche en ligne n'existe**. Le flux Associations n'a pas de jeu repéré.
- **Mesures** (sondes des 2026-11-23 et 2026-11-24 : une cinquantaine de requêtes publiques, une par seconde au plus, aucun refus, aucun cookie, aucun appel à PISTE ; détail au journal de 09 §2.20) :
  - Particuliers : 5 555 fichiers, dont 2 997 fiches (`F…`), 2 306 ressources (`R…`), 243 pages de navigation (`N…`) ; Entreprendre : 775 fiches, dont 106 communes aux deux publics, au même texte ;
  - archive en `deflate`, sans zip64, chiffrement, descripteur ni commentaire ; aucun `DOCTYPE` ni `ENTITY` ; texte médian d'une fiche : 10 237 caractères, une seule au-delà de 1 Mio ;
  - **2 694 fiches sur 2 997 citent Légifrance** (`dc:source`, éléments `Reference`), sous 118 formes d'adresse ;
  - le catalogue `idxDateModifImp.xml` ne liste que 1 740 fiches sur 2 997.
- **Mesure de la recherche** (T-MS-03, 2026-11-23). Vingt questions de non-juriste, écrites avec leurs requêtes par un agent qui ne voyait aucun catalogue ; bonne fiche désignée par un second agent qui ne voyait pas les requêtes ; protocole écrit avant les résultats ; questions non publiées. Succès : la fiche dans les cinq premiers ; seuil : 15. Titres du catalogue (Q38 = A) : **13 sur 20**, encore 13 avec tous les titres. Conçues après coup : un mot suffit sur les petits index, 15 ; BM25 sur les titres et résumés de l'archive, **17** ; texte entier, 17 ou 18.
- **La licence** (fiches de présentation de la DILA, 1er octobre 2025) : paternité propre à chaque public (« Service-Public.gouv.fr/DILA », « Entreprendre.Service-Public.gouv.fr/DILA »), « l'url d'accès longue de téléchargement », « le nom du fichier téléchargé ainsi que la date du fichier » ; suivi du forum du jeu. Deux versions du flux au plus en parallèle, l'ancienne retirée six mois après la nouvelle.
- **Le projet.** `LegalCorpus` et `TaxDoctrine` ne conviennent pas à une fiche, qui n'a ni fonds ni version à une date. `reqwest`, `quick-xml` et `flate2` sont dans `Cargo.lock`, confinés par `CONFINED` ; `flate2` est déjà dans le graphe par défaut. Aucun lecteur zip. Instructions : 3 688 caractères pour un plafond de 3 700 (AM-93). Délai d'un appel : 60 s par défaut, garde dure posée par `Audited`.

## Décision

1. **Port distinct `PublicGuidance`** (`lf-application::ports`) : `search` (mots, public, page) et `get` (public, numéro). Dispatch statique (ADR-0012), source active ou coupée choisie au démarrage par un `enum`, comme `AnyDoctrine` ; codes d'erreur en place (ADR-0010).
2. **Crate `lf-adapter-service-public`**, qui dépend de `lf-application` et `lf-domain` seulement ; ligne dans `RULES`. Portes de `reqwest`, `quick-xml` et `flate2` ouvertes à lui dans `CONFINED`, `wrappers` de `deny.toml` pour `reqwest` et `quick-xml` ; `tokio` avec `rt`. **Aucun paquet externe nouveau** : s'il en fallait un, le travail s'arrête et le propriétaire décide.
3. **Archive du jour en mémoire, recherche locale** (Q38 = B).
   - **Téléchargement**, par public : au premier besoin, une seule requête, dans une tâche à part bornée à 300 s et partagée par les appels qui l'attendent ; gardée en mémoire, jamais sur disque ; revalidée au bout de 24 h par `If-None-Match` (un `304` la garde).
   - **Attente d'un appel** : jusqu'à l'échéance publiée (`Deadline::current_or`) moins une seconde. Ensuite, `upstream_error` : l'archive est en cours de téléchargement, réessayer dans une minute. Le téléchargement continue.
   - **Archive refusée ou revalidation échouée** : l'archive précédente reste servie 48 h de plus au plus, et sa date le montre.
   - **Index**, construit au chargement : titre, résumé, nature, thème et date de chaque fiche et page de navigation, arbre du menu ; ressources exclues. Recherche BM25 sur le titre (poids 3) et le résumé ; chaque mot correspond aux mots qu'il commence, sans casse ni accents ; liste fermée de mots vides ; à égalité, numéro croissant.
   - **Chemin** (précision du 2026-11-24, Q41) : le menu donne à chaque page sa place dans le site, rendue par les deux outils dans `path` : les intitulés du thème, du sous-thème et du dossier, du plus large au plus étroit. Une fiche prend la place du dossier qu'elle nomme (`DossierPere`), plus l'intitulé de ce dossier ; un dossier ou un thème, les intitulés qui le contiennent ; un numéro placé deux fois garde sa première place ; hors du menu, `path` est omis. Mesure du 2026-11-24 : pour les 2 972 fiches de Particuliers et les 762 d'Entreprendre qui nomment leur dossier, sa place concorde avec le thème et le sous-thème écrits par la fiche ; 147 caractères au plus. Le classement n'en dépend pas.
   - **Lecture** : dans l'archive en mémoire, sans requête. La DILA ne voit qu'un téléchargement par public et par 24 h, ni les mots cherchés, ni les numéros lus.
   - **Précisions de la revue** (T-MS-41, 2026-11-24, deux vagues de corrections ; cas que le texte taisait, aucune décision changée) :
     - l'âge d'une archive est le plus long des temps mesurés par l'horloge monotone et par l'horloge civile (port `Clock`, injecté sans `dyn`, ADR-0012) : la monotone ne compte pas la mise en veille de la machine sous Linux et macOS ; une horloge civile qui saute en avant vieillit l'archive d'autant (au-delà de 24 h, une revalidation ; au-delà de 72 h, l'archive ne se sert plus avant le succès suivant) ;
     - pendant la revalidation d'une archive qui se sert encore, les appels attendent 10 s au plus, comptées depuis le début du téléchargement et communes à tous les appels qui l'attendent (une recherche `all` aussi), puis reçoivent l'archive précédente ;
     - les échecs qui ont coûté un téléchargement espacent les tentatives quand ils se suivent : une minute, un quart d'heure, une heure, puis six heures après une archive reçue puis refusée par le lecteur ; la même suite, plafonnée à une heure, après un corps coupé ou un délai dépassé ; le compte repart de zéro au premier succès ou `304` ; les autres échecs gardent la minute ;
     - l'index d'un public a un budget : 100 000 mots distincts, 500 000 couples (mot, page) et 8 Mio de texte gardé (titres, résumés, chemins) au plus, chaque mot coupé à 40 caractères ; les pages sont lues une à une et la lecture s'arrête au dépassement, qui fait refuser l'archive (relevé du 2026-11-24 : 6 334 mots, 76 955 couples et 0,86 Mo pour Particuliers ; aucun mot de plus de 20 caractères) ;
     - une recherche `all` échoue entière si l'archive d'un des deux publics manque ;
     - la citation nomme le site de l'adresse rendue (une page de l'archive d'Entreprendre peut être publiée sur Service-Public.gouv.fr ; la paternité reste celle de l'archive lue) ; `sheet_type` n'est rendu que pour une fiche.
4. **Requête fermée** : hôte `lecomarquage.service-public.gouv.fr`, deux chemins `/vdd/3.5/{part,pro}/zip/vosdroits-latest.zip` fixés dans le code ; `https` seul, aucune redirection suivie, aucune variable de surcharge ; corps plafonné à 64 Mio.
5. **Lecture bornée.**
   - **Lecteur d'archive** écrit dans le crate pour le format mesuré : répertoire central recoupé avec l'en-tête local ; 20 000 entrées au plus ; `deflate` ou aucune compression ; zip64, chiffrement ou descripteur font refuser l'archive ; entrée au nom hors de la forme `[A-Za-z0-9]+.xml`, ou répertoire, ignorée et comptée ; décompression par `flate2`, arrêtée au premier octet en trop (2 Mio par entrée lue, 64 Kio pour un en-tête) ; CRC-32 vérifié ; rien sur disque.
   - **XML** : lecteur de `quick-xml`, sans `serde` ; `DOCTYPE` et entités refusés ; profondeur et nombre d'éléments bornés ; inconnus ignorés. Fiche plus longue que 2 Mio : `upstream_error` avec son adresse publique.
   - **Rendu** (domaine) : chapitres, paragraphes, listes, tableaux ligne par ligne, situations et cas annoncés par leur intitulé, encadrés ; un lien vers une fiche garde son numéro.
6. **Types purs du domaine** : numéro (`F` ou `N` et 1 à 7 chiffres) ; public (`particuliers`, `entreprendre`) ; nature (`sheet`, `folder`, `theme`) et `sheet_type` ; adresse publique à grammaire stricte (`https://www.service-public.gouv.fr/particuliers/vosdroits/{id}`, `https://entreprendre.service-public.gouv.fr/vosdroits/{id}`), jamais appelée ; index et classement.
7. **Deux outils en lecture seule.** `fr_search_service_public` rend numéro, public, nature, titre, chemin, date et adresse publique, jamais le texte ; public `particuliers` par défaut, `entreprendre` ou `all` (un numéro commun rendu une fois). `fr_get_service_public` lit une fiche avec le retour progressif de `fr_get_text` (ADR-0006), ou un dossier avec la liste de ses fiches ; un numéro absent rend `not_found`, et le message nomme l'autre public.
8. **De la fiche vers la loi seulement** (Q40 = A) : `legal_references`, tirées de `Reference` et de `dc:source` (liste à virgules).
   - Adresses de `www.legifrance.gouv.fr` seulement, chemin à grammaire stricte (fonds, vue, un ou deux identifiants, date et ancre facultatives, requête ignorée), adresse reconstruite, jamais recopiée ; toute autre adresse omise.
   - Cible : le dernier identifiant ; une section (`LEGISCTA`) ne donne jamais le numéro du code. Numéro donné quand un outil en place le lit : `LEGIARTI` et `KALIARTI` (`fr_get_text`), `LEGITEXT` (`fr_get_act`), `KALITEXT` (`fr_get_convention`), `JURITEXT`, `CETATEXT`, `CONSTEXT` (`fr_get_decision`), `CNILTEXT` (`fr_get_deliberation`) ; sinon titre et adresse.
   - Une fois chacune, dans l'ordre d'apparition, 50 au plus (`legal_references_truncated`). Le sens inverse (`servicePublicLinksArticle` de PISTE) n'est pas branché.
9. **Mentions de la licence et nature du texte, à chaque réponse.**
   - `attribution` selon le public : « Source : Service-Public.gouv.fr / DILA — Licence Ouverte 2.0 », « Source : Entreprendre.Service-Public.gouv.fr / DILA — Licence Ouverte 2.0 », les deux séparées par « ; » pour `all`.
   - Fichier téléchargé, dans les termes de la DILA : `url` complète de l'archive, `name` (`vosdroits-latest.zip`), `date` (`Last-Modified`) ; dans `source_files` (une liste, toujours) pour la recherche, dans `source_file` pour la lecture, qui ajoute `entry` (par exemple `F1234.xml`).
   - Note fixe : information administrative, pas la loi ; version du jour seulement ; lire les textes de `legal_references` avant de s'appuyer sur un point précis.
10. **Active par défaut, sans réglage**, comme le BOFiP (ADR-0028 décision 8) : `LF_SERVICE_PUBLIC=on|off`, `on` par défaut ; coupée d'office quand `LF_DILA_DIR` est défini, sauf `on` explicite ; valeur invalide : source coupée, avec un avis ; coupée : `[config_error]` sans E/S ; réglage MCPB `service_public`. Aucune variable d'adresse, aucun identifiant.
11. **Audit, débit, mémoire** : une ligne d'audit par appel, format inchangé ; aucune autre requête que le téléchargement et la revalidation ; un 429 rend `[rate_limited]` sans nouvelle tentative, la source se repose et l'archive en mémoire reste servie ; environ 35 Mio d'archives quand les deux publics ont servi, plus l'index (V-235) ; rien n'est chargé avant le premier appel.
12. **Couverture**, à la surface v0.3 seulement : famille `SERVICE_PUBLIC` après `BOFIP` ; lacune FR-108 (source active) ou FR-109 (source coupée), calculées par le domaine pour toute surface et filtrées par la vue hors de la v0.3 (05 §5.8).
13. **Hors de cette version** : le flux Associations ; la recherche dans le texte entier (gain nul ou d'un succès, index bien plus lourd) ; une archive sur disque partagée entre processus ; le lien de l'article vers ses fiches (Q40 = B) ; les ressources `R…` et les adresses locales ; les numéros `F…` dans `fr_resolve` et `fr_verify_citations`.
14. **Version mineure** : rubrique « Tool surface » du CHANGELOG ; plafond des instructions de 3 700 à **4 000** caractères, pour une ligne de 248 caractères en v0.3 (environ 3 937 au total, 96 sous la coupe de Claude Code à 4 096) ; plafond de `tools/list` recalculé par la règle d'AM-63.

## Questions tranchées

| Question | Décision | Options écartées |
|---|---|---|
| **Q38** Mode d'accès | **B** (2026-11-24, « continue » à la recommandation, après la mesure de T-MS-03) : archive du jour en mémoire, index des titres et résumés | A (2026-11-23) : lecture en direct, recherche sur les titres du catalogue ; 13 sur 20, catalogue à 58 % |
| **Q39** Publics servis | **A** (2026-11-23) : Particuliers et Entreprendre | B : Particuliers seul ; C : avec Associations |
| **Q40** Lien loi et fiches | **A** (2026-11-23) : de la fiche vers la loi | B : aussi de l'article vers ses fiches, par PISTE |
| **Q41** Usage du menu | **Chemin dans le site** (2026-11-24, après la vague 1 : « 2 », lire le menu, puis le chemin) : `path` dans chaque résultat et chaque page lue | retirer le menu de l'index, rien ne le lisant (recommandé) ; rendre lisibles les 73 sous-thèmes sans page ; les deux |

## Conséquences

- **Positives** : l'explication en langage courant rejoint le texte exact ; toutes les fiches sont cherchables ; après le premier téléchargement, chaque appel répond sans réseau ; rien de ce que l'on cherche ni de ce que l'on lit ne part vers la DILA ; les mentions exigées s'appliquent telles quelles ; aucun compte.
- **Négatives** : la première recherche de la journée télécharge 24 Mo et peut dépasser l'échéance sur une liaison lente ; environ 35 Mio de mémoire en plus ; un lecteur d'archive à maintenir, qui lit une entrée venue du réseau ; le texte des fiches n'est pas cherché ; pas de version passée ; une version de flux vit six mois après la suivante ; le propriétaire doit suivre le forum du jeu.
- **À surveiller** (09 §2.23) :
  - **NON VÉRIFIÉ** [V-229 · sp-recherche-titres] : confirmation de la recherche sur 20 questions nouvelles (T-MS-46), seuil 15 ;
  - **NON VÉRIFIÉ** [V-230 · sp-hebergeur-journaux] : journaux de l'hôte ;
  - **NON VÉRIFIÉ** [V-231 · sp-quota] : quota du serveur ;
  - **NON VÉRIFIÉ** [V-232 · sp-mentions-licence] : forme des mentions ;
  - **NON VÉRIFIÉ** [V-233 · sp-rendu-conditionne] : rendu des cas et des recherches guidées ;
  - **NON VÉRIFIÉ** [V-234 · sp-forum-et-version] : suivi du forum et du flux suivant ;
  - **NON VÉRIFIÉ** [V-235 · sp-archive-memoire-delai] : délai du téléchargement et mémoire.
- **Tests qui la gardent** : binaire, par le proxy témoin (hors ligne ou source coupée : aucune connexion ; source active, `LF_TOOL_DEADLINE_S=1` : une connexion vers `lecomarquage.service-public.gouv.fr:443`) ; faux serveur (aucune requête une fois l'archive chargée, aucune requête ne porte un mot ni un numéro) ; lecteur d'archive (archives mal formées, entrée qui gonfle au-delà du plafond, propriété sur octets arbitraires) ; XML (`DOCTYPE`, bornes, propriété) ; test D-04 avec trois hôtes exacts de plus ; `sha256` des quinze descripteurs inchangés ; tests en direct hebdomadaires sans secret.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Lecture en direct et recherche sur les titres (Q38 = A) | 13 sur 20 ; catalogue à 58 % ; la DILA aurait vu chaque numéro lu |
| Même lecture, un mot suffit, petits index | 15 sur 20, au seuil, mesuré après coup ; 32 % des fiches hors de l'index |
| Texte entier | Gain nul ou d'un succès, index bien plus lourd ; reste ouvert derrière le port |
| Paquet zip externe | Paquet nouveau (décision 2) ; le format servi tient dans un lecteur borné sur `flate2` |
| Archive sur disque | Chemin, écritures atomiques, partage entre processus : pas nécessaires pour cette version |
| Recherche du site ; jeu vectorisé de la DINUM | Pas d'API, ou intermédiaire trimestriel à plongements sémantiques |
| Méthodes ajoutées à `LegalCorpus` ou `TaxDoctrine` | Ni fonds, ni version à une date, ni doctrine |

## Références et sources

- Jeux data.gouv.fr cités ci-dessus (API lue le 2026-11-23) ; flux `https://lecomarquage.service-public.gouv.fr/vdd/3.5/{part,pro}/` ; schémas `https://echanges.dila.gouv.fr/OPENDATA/SERVICE-PUBLIC_DTD/schema_3.5.zip` ; fiches de présentation de la DILA du 1er octobre 2025 ; pages P10025 et P10001 de service-public.gouv.fr (2026-11-23) ; `flate2` 1 par context7 (2026-11-24).
- [ADR-0004](0004-piste-puis-dila.md), [ADR-0006](0006-sortie-compacte-retour-progressif.md), [ADR-0010](0010-erreurs-in-band.md), [ADR-0012](0012-ports-async-rpitit.md), [ADR-0028](0028-bofip-doctrine-fiscale.md), [ADR-0029](0029-bofip-precisions-apres-mesure.md) ; [00-decisions.md](../plan/00-decisions.md) §1, §2, §3, §6, §7, §11, §14 (AM-63, AM-93, AM-94, AM-100) ; [05](../plan/05-outils-mcp.md) §4.2, §5.8, §5.16, §5.17 ; [07](../plan/07-feuille-de-route.md) §2.12 ; [09](../plan/09-risques-et-questions.md) §2.20, §2.23.
