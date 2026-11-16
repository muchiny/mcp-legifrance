# ADR-0032 — BOFiP : la lecture d'une publication liste les articles de code qu'elle cite ; leur vérification reste à l'appelant

- **Statut** : **Accepté** à la fusion de la PR qui le porte (choix du propriétaire du 2026-11-16, pris avant l'écriture de ce document).
- **Date** : 2026-11-16
- **Jalons concernés** : MB (BOFiP), troisième correctif après la 1.3.0. Il ajoute deux champs de sortie facultatifs : une version mineure est proposée (décision 7), et le numéro reste décidé par le propriétaire à la PR de version.
- **Questions liées** : Q37 (comment dire qu'une publication peut être plus ancienne que la loi qu'elle commente) : **décidée par le propriétaire le 2026-11-16, option 1** (voir « Question tranchée »).
- Amendements : AM-99 (00 §14). Complète la décision 6 d'[ADR-0028](0028-bofip-doctrine-fiscale.md) (ce que rend la lecture) sans la changer. Rien ne change dans [ADR-0029](0029-bofip-precisions-apres-mesure.md), [ADR-0030](0030-bofip-texte-lu-dans-le-html.md) ni [ADR-0031](0031-bofip-recherche-mots-exacts.md). Amende [00-decisions.md](../plan/00-decisions.md) §7.

## Contexte

C'est le dernier des trois défauts relevés par le rejeu des 13 questions (09 §2.20, constat du 2026-11-15) : une publication peut être plus ancienne que la loi qu'elle commente, et rien dans la réponse ne le signale. Exemple du rejeu : `BOI-ENR-DMTG-10-20-30-120`, datée du 2024-10-29, cite encore 2027 et 2028, alors que l'article 1135 bis du CGI dit 2037 et 2038 depuis février 2025.

Mesures du 2026-11-16, hors dépôt. La première est faite hors ligne, sur l'export `jsonl` du jeu du 2026-11-15 (9 173 publications, aucune requête). Elle passe le texte servi depuis ADR-0030 (`publication_text`) au parseur de citations du domaine, inchangé, avec ses options par défaut. La seconde est faite en production, sur accord du propriétaire.

- **Ancienneté.** 5 147 des 6 367 pages de doctrine (81 %) ont un début de validité antérieur à 2024.
- **Ce que le parseur trouve.** Au moins un article d'un code du dictionnaire (annexe B) dans :
  - 5 057 pages de doctrine sur 6 367 (79 %) : 5 articles distincts en médiane, 20 au 90ᵉ centile, 47 au 99ᵉ, 224 au plus ;
  - 1 908 actualités sur 2 806 (68 %) : 15 articles au plus.

  Le CGI domine, avec 28 916 couples (publication, article). Suivent le livre des procédures fiscales (3 033), le code de commerce (1 643), le code civil (1 523) et le code du travail (1 337). La page du rejeu cite les articles 1135 bis, 769 et 1840 G ter du CGI et l'article 2447 du code civil : tous sont trouvés.
- **Coût.**
  - Le temps de lecture des citations d'une publication est de 1,2 ms en médiane, 22 ms au 99ᵉ centile et 65 ms au plus (la page la plus longue, 167 694 caractères ; binaire optimisé).
  - Une publication porte au plus 365 citations de toute sorte, sous le garde-fou de 1 000 du parseur (00 §4).
- **Relecture des formes rendues.**
  - Les 10 809 formes canoniques distinctes (`Code général des impôts, art. 1135 bis`) sont relues à l'identique par le parseur, sauf 186. Ces 186 sont toutes des plages (« art. 1078-1 à 1078-3 »), relues comme leur première borne.
  - Une plage écrite en ses deux bornes est relue à l'identique.
  - Chaque liste ainsi formée, à raison d'une par publication (6 965 listes, 50 articles au plus), est relue à l'identique quand ses éléments sont joints par « ; » ou par des retours à la ligne. C'est la forme sous laquelle un appelant la passerait à `fr_verify_citations`.
- **Les actualités ne suffisent pas.** Aucune actualité n'annonce le changement de 2025 de l'article 1135 bis. La seule qui en parle, `ACTU-2024-00105`, date du jour de la page.
- **Ce que dit Légifrance (mesure en production).** Le propriétaire a donné son accord le 2026-11-16. La sonde passe par le binaire 1.3.2 installé :
  - conditions : une session, une requête par seconde, `LF_BOFIP=off` ;
  - volume : 38 appels d'outil, au plus 475 requêtes environ, aucun refus 429 ;
  - échantillon : 60 pages de doctrine, quatre par année de 2012 à 2026, tirées avec une graine fixe ; la page du rejeu remplace l'une des pages de 2024, comme témoin ;
  - articles : au plus dix articles du CGI ou du LPF par page, soit 272 articles distincts, lus par `fr_verify_citations` ;
  - statut des articles : 229 sont en vigueur ; 43 n'ont aucune version en vigueur, le plus souvent parce qu'ils sont abrogés (par exemple l'article 1605 du CGI) ;
  - 43 pages sur 60 (72 %) citent au moins un article dont la version en vigueur commence après la page ;
  - 24 pages (40 %) citent un article sans version en vigueur ;
  - 52 pages (87 %) sont dans l'un des deux cas, de deux à quatre pages par année, 2025 et 2026 compris ;
  - le délai médian entre la page et la version en vigueur est de 1 831 jours, et aucun n'est inférieur à 90 jours ;
  - témoin : l'article 1135 bis est en vigueur depuis le 2025-02-09, la page est donc repérée.
- **Les changements touchent rarement la page.** Dix couples (page, article plus récent) ont été tirés au hasard. Pour chacun, la sonde lit le changement de l'article par `fr_diff_article`, de la date de la page à aujourd'hui, et le compare au texte de la page :
  - **un seul touche ce que dit la page** : `BOI-DJC-EXPC-20-40-20` décrit une obligation fondée sur le 1° du 7 de l'article 158, qui est abrogé ;
  - **deux sont incertains** : une phrase a été ajoutée au 3° bis du I de l'article 286, que la page cite ; le renvoi « des 6° et 6° bis » a été réduit dans la phrase de l'article 206-1 que la page cite ;
  - **sept sont sans effet sur la page** : montant indexé, ponctuation, autre alinéa (articles 302 septies A, 261, 793, 1382, 196 B, 1727 et 221 du CGI).
- **Conclusion.** « Un article cité est plus récent que la page » est un fait exact, mais presque constant (72 à 87 % des pages), et le plus souvent sans effet sur ce que dit la page. Une alerte calculée par l'outil serait surtout du bruit : seule la lecture du changement, comparée au texte de la page, permet de trancher. Les outils de cette lecture existent déjà, et la sonde les a enchaînés : `fr_verify_citations` donne la date de début de la version en vigueur et l'identifiant `LEGIARTI`, puis `fr_diff_article` montre ce qui a changé depuis une date.

## Décision

1. **La lecture liste, l'appelant vérifie** (Q37 = 1). Une publication lue par `fr_get_bofip` porte `cited_articles` : les articles de code que cite son texte. Chaque article y figure une fois, sous sa forme canonique (`Code général des impôts, art. 1135 bis`), dans l'ordre de sa première apparition. L'outil n'appelle jamais Légifrance et n'envoie aucune requête de plus au portail.
2. **Ce qui est listé.**
   - Les articles d'un code du dictionnaire (`CodeKey`, annexe B) que reconnaît le parseur de citations du domaine. Le parseur est inchangé et garde ses options par défaut.
   - Une plage figure par ses deux bornes, comme `fr_verify_citations` les contrôle (05 §5.9).
   - Ne sont pas listés : les articles d'un texte non codifié (loi, décret), un article sans code, un article dont le code est seulement désigné par « dudit code » et que le parseur ne rattache à aucun, et les codes absents du dictionnaire.
   - La liste porte sur **tout le texte** de la publication, celui que sert ADR-0030, et non sur la fenêtre rendue. Elle est donc la même à chaque page d'une lecture, quels que soient `detail`, `focus` et `offset`.
3. **Cinquante articles au plus.** C'est le défaut de `max_citations` de `fr_verify_citations` : un seul appel contrôle la liste entière. Mesure : la liste est complète pour 99,25 % des pages de doctrine qui citent un article (38 pages sont coupées) et pour toutes les actualités. Au-delà, ce sont les 50 premiers dans l'ordre d'apparition, et `cited_articles_truncated` vaut `true`. Il en irait de même si le garde-fou du parseur (1 000 citations) jouait, ce qui n'arrive pour aucune publication du jeu.
4. **Absent quand il n'y a rien.** `cited_articles` est omis :
   - quand la liste est vide ;
   - pour une publication sans texte ;
   - dans la forme « liste » d'un identifiant à deux publications.

   `cited_articles_truncated` n'est rendu que s'il vaut `true`.
5. **Ce que la réponse dit.**
   - La note de doctrine de `fr_get_bofip` reçoit une phrase de plus, fixe. Elle dit qu'une publication peut être plus ancienne que la loi qu'elle explique, et qu'avant de s'y fier, on contrôle les articles de `cited_articles` : `fr_verify_citations` donne la date de début de chaque version en vigueur, puis `fr_diff_article`, depuis `valid_from`, montre ce qui a changé.
   - La phrase ne porte aucune chaîne libre. Son texte exact s'écrit avec le code, dans les bornes de 05.
   - La note de `fr_search_bofip` ne change pas.
   - La description de `fr_get_bofip` nomme le champ, en 3 à 6 phrases.
   - Les instructions du serveur ne changent pas (3 688 caractères sur 3 700, AM-93).
6. **Où la règle est écrite.**
   - Une fonction pure du domaine (module `doctrine`) bâtit la liste à partir du texte, avec le parseur de citations.
   - Le cas d'usage `get_bofip` l'appelle sur le texte entier, avant la lecture.
   - Le port `TaxDoctrine`, l'adapter du BOFiP et le parseur de citations ne changent pas.
7. **Surface et classement du changement.**
   - Deux champs de sortie facultatifs : c'est un **ajout** (05 §9.5), donc une version mineure. Le propriétaire décide du numéro à la PR de version.
   - `lock-descriptors --compare v1.3.2` ne doit relever qu'un ajout, sur `fr_get_bofip` seul, avec sa description récrite. Les quatorze autres descripteurs gardent leur empreinte.
   - S'il relève autre chose, le travail s'arrête et le propriétaire est prévenu.
8. **Débit, quota, vie privée.**
   - Aucune requête n'est ajoutée ; `PRIVACY.md` ne change pas.
   - La vérification, si l'appelant la fait, se paie à son compte : il faut un compte PISTE. Mesure : 272 articles en 444 s à une requête par seconde, soit une à deux requêtes par article. Une liste de 50 articles demande donc 10 à 20 s au débit par défaut (5 requêtes par seconde).
   - Sans compte PISTE, la liste reste rendue, et la note dit quoi faire.
9. **Rien d'autre ne change** :
   - la recherche, la lecture du texte, les fenêtres et les passages ;
   - les erreurs et l'audit (même métrique : caractères renvoyés ou publications listées) ;
   - les treize outils de Légifrance.

   `fr_resolve` et `fr_verify_citations` ne lisent toujours pas les références `BOI-…` (Q31 = A).

## Question tranchée

La question a été posée au propriétaire avec une recommandation, après la sonde en production. Il a répondu « 1 » le 2026-11-16.

| Question | Décision | Options écartées |
|---|---|---|
| **Q37** Comment dire qu'une publication peut être plus ancienne que la loi qu'elle commente | **1** : la lecture liste les articles de code cités et dit comment les vérifier ; l'appelant vérifie quand sa réponse en dépend | 2 : vérification automatique dans Légifrance à chaque lecture. Une note sortirait pour 72 à 87 % des pages, sans effet sur la page sept fois sur dix. Chaque lecture coûterait 10 à 20 requêtes PISTE, et la doctrine, lisible aujourd'hui sans compte, dépendrait d'un compte PISTE. 3 : la phrase seule, sans la liste. L'appelant devrait retrouver les articles dans le texte, souvent en plusieurs fenêtres |

## Conséquences

### Positives

- La réponse relie la date de la page et la loi qu'elle commente, sans jamais affirmer qu'une page est périmée.
- L'appelant vérifie en deux appels, avec des outils déjà livrés et testés. La forme rendue est celle que `fr_verify_citations` relit, ce qui est mesuré sur tout le jeu.
- La lecture garde le même coût réseau et ne demande toujours aucun compte.
- La liste sert aussi à vérifier une réponse qui s'appuie sur la doctrine.

### Négatives

- **L'appelant peut ne pas vérifier.** Le défaut relevé par le rejeu ne se corrige que si le modèle suit la consigne (V-228). À l'inverse, il peut conclure qu'une page est périmée à cause d'un changement sans effet.
- **La liste n'est pas tout ce que la page cite.**
  - Les articles des lois et décrets non codifiés n'y sont pas.
  - Le parseur ne reconnaît pas la forme longue d'un article d'annexe (« article 75 de l'annexe II au code général des impôts »), seulement la forme courte (« CGI, ann. II, art. 75 »). C'est une limite du parseur, relevée par la sonde et non corrigée ici.
- Un article abrogé ressort de `fr_verify_citations` en `not_found` : la note de cet outil dit qu'aucune version n'est en vigueur aujourd'hui et propose de réessayer avec une date. L'appelant doit comprendre que l'article a disparu.
- Chaque lecture coûte jusqu'à 65 ms de plus, et la réponse grandit (128 caractères en médiane pour la liste, environ 2 500 au plus).
- Un descripteur change : les clients qui épinglent les descripteurs redemandent une approbation.
- La vérification exige un compte PISTE.

### À surveiller

- **NON VÉRIFIÉ** [V-228 · bofip-consigne-suivie] : un modèle qui lit une publication vérifie-t-il les articles cités quand sa réponse en dépend, et en tire-t-il la bonne conclusion ? Seul un rejeu le mesure, en production, sur accord du propriétaire. Ce point ne bloque pas la version.
- Les taux de 72 à 87 % et la proportion de sept sur dix viennent d'un échantillon de 60 pages et de 10 couples. Ils fondent le choix, pas une promesse.
- La forme longue des articles d'annexe : à reprendre dans le parseur, par une PR à part, si l'usage le demande.
- Condition de révision : si, sur le même export, la mesure faite avec le code de la branche (07 §2.11, garantie K4) donne d'autres listes que la sonde, le travail s'arrête avant la fusion et ce document est repris.

### Tests qui la gardent

À écrire avec le code :

- **Fonction du domaine** :
  - ordre de première apparition, sans doublon ;
  - codes du dictionnaire seuls ;
  - plage écrite en deux bornes ;
  - plafond de 50 et drapeau de troncature ;
  - texte sans article.
- **Propriétés** :
  - chaque élément est relu à l'identique par le parseur, seul ou dans la liste jointe ;
  - jamais plus de 50 éléments, jamais de doublon.
- **Cas d'usage** :
  - même liste quels que soient `detail`, `focus` et `offset` ;
  - champ absent pour une publication sans texte et pour la forme « liste ».
- **Vue** :
  - champs omis quand ils sont absents ;
  - budget de réponse tenu avec cinquante citations parmi les plus longues ;
  - phrase fixe de la note.
- **Descripteur** : description mot pour mot, dans les bornes de 05, et verrou.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Vérification automatique à chaque lecture (Q37 = 2) | Bruit (72 à 87 % des pages signalées, sept changements sur dix sans effet). 10 à 20 requêtes PISTE par lecture. La doctrine dépendrait d'un compte PISTE. |
| La phrase seule (Q37 = 3) | L'appelant devrait retrouver les articles dans le texte, souvent en plusieurs fenêtres. |
| Les actualités du BOFiP | Aucune n'annonce le changement de 2025 de l'article 1135 bis. |
| Lister aussi les lois, décrets et décisions cités | Un texte non codifié change rarement, et sa vérification coûte plus cher (sommaire du texte, 05 §5.9). À reprendre par un ADR si l'usage le demande. |
| Ne lister que le CGI et le LPF | Les pages citent aussi le code civil, le code de commerce et le code du travail (plus de 1 300 couples chacun). Le dictionnaire les couvre déjà. |
| Liste sans plafond | 224 articles au plus. Un seul appel de `fr_verify_citations` n'en contrôle que 100, et 50 par défaut. |

## Références et sources

- Jeu et API : ceux d'[ADR-0028](0028-bofip-doctrine-fiscale.md). Sondes du 2026-11-16, hors dépôt :
  - export `jsonl` du 2026-11-15, lu par une crate jetable qui appelle `publication_text` et le parseur de `lf-domain` 1.3.2 ;
  - 38 appels d'outil en production, avec le binaire 1.3.2 installé (`fr_verify_citations`, `fr_diff_article`) ;
  - outils, échantillon et réponses gardés hors dépôt.
- [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.9 (`fr_verify_citations`), §5.12 (`fr_diff_article`), §5.15 (`fr_get_bofip`), §9.5 (politique de version) ; [07-feuille-de-route.md](../plan/07-feuille-de-route.md) §2.11 (tâches T-MB-67 à T-MB-73) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.20 et §2.22 (V-228).
- [00-decisions.md](../plan/00-decisions.md) §4 (parseur de citations), §7, §14 (AM-99).
