# ADR-0031 — BOFiP : la recherche demande les mots exacts ; la recherche approchée du portail ne sert plus qu'en secours

- **Statut** : **Accepté** (2026-11-16, fusion de la PR #90 ; choix du propriétaire du 2026-11-16, pris avant l'écriture du code). Précisé avec le code, le même jour (voir la note ci-dessous).
- **Date** : 2026-11-16
- **Jalons concernés** : MB (BOFiP), second correctif après la 1.3.0, soit la 1.3.2 (numéro décidé par le propriétaire le 2026-11-16, qui retient le classement de la décision 6 ; un seul descripteur change, par deux textes)
- **Questions liées** : Q36 (règle de correspondance des mots d'une recherche) : **décidée par le propriétaire le 2026-11-16, A** (voir « Question tranchée »)
- **Précisions apportées avec le code (2026-11-16)**, sans changer la décision : (1) la description du paramètre `query` disait elle aussi « le dernier en préfixe » ; elle est récrite de même (décision 6), et `lock-descriptors --compare` classe les deux changements en textes reformulés ; (2) le portail ne coupe pas un mot à un point, une apostrophe ou un deux-points pris entre deux lettres : dans un texte où une phrase colle à la suivante (« pratiquées.L’abattement »), le mot suivant n'est trouvé ni par `like` ni par `search()` (vérifié sur une publication, prédiction écrite avant) ; avec cette règle de découpage, le modèle local redonne le total du portail pour les 27 clauses `like` essayées et pour 35 des 44 recherches approchées connues (au lieu de 28) ; (3) la mesure avec le binaire de la branche (garantie J4) redonne tous les chiffres de ce document (voir « Conséquences ») ; (4) tests écrits, au regard de la liste « Tests qui la gardent » : la clause de cinq combinaisons de nature, de série et de mode est écrite en entier, et une propriété contrôle toutes les autres (nombre de littéraux, et mots faits de lettres, de chiffres et d'espaces) ; « aucune erreur ne contient un mot de la requête » est vérifié au cas d'usage pour les deux appels, et le test de l'adapter (`transport_errors_never_contain_query_words`) envoie désormais la requête exacte, la requête approchée étant celle qu'il couvrait avant (ses serveurs bruts ne servent qu'une connexion) ; (5) les mots étant écrits deux fois, l'adresse s'allonge : 11 122 caractères au plus (450 lettres codées sur quatre octets, série de 16 caractères, actualités, page 100 de 100), au lieu de 5 698 ; le portail accepte une adresse de 11 136 caractères (deux requêtes, `200` et total nul, prédiction écrite avant), et un test de l'adapter garde ce plafond.
- Amendements : AM-98 (00 §14). Révise, dans [ADR-0029](0029-bofip-precisions-apres-mesure.md), les deux dernières phrases de la décision 3 (« La recherche reste `search(contenu, …)` » ; « La description de l'outil dit la règle : tous les mots, dernier mot en préfixe, pas d'opérateur ») et complète la ligne « Recherche » de son contexte, qui n'avait pas vu que la recherche du portail est approchée. Précise sa décision 7 : une recherche peut envoyer deux requêtes ; la seconde n'est pas un réessai. Rien d'autre ne change dans [ADR-0028](0028-bofip-doctrine-fiscale.md) ni dans ADR-0029. Amende [00-decisions.md](../plan/00-decisions.md) §2.

## Contexte

Mesures du 2026-11-16, hors dépôt : un index local des 9 173 publications du jeu `bofip-vigueur` (export du 2026-11-15, aucune requête), les 31 recherches gardées du rejeu des 13 questions (07 §2.11, T-MB-44) et les 13 de T-MB-02, puis 28 requêtes au portail (185 sur les 200 du jalon, une par seconde, aucun refus 429). Aucun appel à PISTE.

- **Le défaut.** Les juges du rejeu ont relevé des résultats hors sujet pour 10 des 11 questions qui ont lu la doctrine (09 §2.20). C'est le deuxième des trois défauts reportés après la 1.3.0.
- **La cause principale : `search()` est une recherche approchée.** La documentation du portail le dit (Opendatasoft, « Explore API v2.1 », `search()`) : un mot de plus de 5 lettres est retrouvé à 2 fautes près, un mot de 3 à 5 lettres à 1 faute près, et le dernier mot vaut préfixe. Mesuré en plus : les accents sont retirés avant la comparaison. ADR-0028, ADR-0029, 05 §5.14 et la description de l'outil ne disaient que « tous les mots, le dernier en préfixe ». Exemple, sur un mot déjà cité dans ce dépôt et d'après le modèle local décrit ci-dessous : dans une recherche de plusieurs mots, « donation » trouve aussi les publications qui disent « fonction » (1 688 publications), « location » (1 192), « vocation » ou « dotation » ; de même, « déplacement » trouve « placement », « remplacement » et « emplacement ». Vérifié sur le portail, prédiction écrite avant la requête : `BOI-TVA-LIQ-30-10-20` (taux de TVA de l'alimentation animale), qui ne dit ni « donation » ni aucun de ses voisins sauf « fonction », sort pour « donation frais » ; avec `like`, elle ne sort pas. Les résultats du rejeu montrent la même chose : des publications rendues ne portent pas le mot cherché, mais un mot voisin.
- **Un modèle local le reproduit.** La règle « mots approchés sans accents, dernier mot en préfixe », appliquée à l'index local, redonne le total rendu par le portail au résultat près pour 28 des 44 recherches connues ; la règle « mots exacts » pour 7. Les 16 autres totaux sont proches : le traitement du dernier mot n'est pas entièrement compris (voir « À surveiller »). Trois prédictions neuves, écrites avant la requête, sont confirmées au résultat près (868, 354 et 442 publications).
- **Le portail sait chercher les mots exacts.** Pour une même requête de deux mots, doctrine seule :

| Clause | Publications | Ce qu'elle fait |
|---|---|---|
| `search(contenu, Q)` (la 1.3.1) | 868 | mots approchés, dernier mot en préfixe |
| un `search(contenu, mot)` par mot | 354 | chaque mot en préfixe |
| `contenu like Q` | **171** | tous les mots, exacts, accents et casse ignorés, ordre libre |
| chaîne nue `"…"` | 874 | mots approchés, dans tous les champs |
| `suggest(contenu, Q)` | 1 | inutilisable ici |

  `search(contenu, Q) and contenu like Q` rend 171 publications aussi, classées par le portail. Le total de la règle « mots exacts » du modèle local, écrit avant la requête, est celui du portail pour les dix requêtes essayées (11, 11, 14, 16, 20, 25, 26, 42, 100 et 171 publications).
- **Ce que la règle exacte change, sur les 31 recherches du rejeu** : 820 résultats au total relevés pendant le rejeu (la recherche de la 1.3.0, inchangée en 1.3.1), 169 en mots exacts d'après le modèle local. 27 recherches sur 31 tiennent alors dans une page de dix. Des 25 publications que les réponses ont ensuite lues, 22 restent trouvées ; les trois autres ne portaient qu'un mot approchant (deux) ou le dernier mot en préfixe (une). Recherches sans aucun résultat : 2 pendant le rejeu, 5 en mots exacts.
- **Classement.** Neuf requêtes dont la page attendue a été choisie avant la mesure (six de T-MB-02, trois du rejeu ; celles dont la liste en mots exacts dépasse dix résultats). La page attendue est dans les cinq premiers résultats pour 5 requêtes sur 9 avec la 1.3.1, pour 8 sur 9 avec `search(contenu, Q) and contenu like Q`, pour 7 sur 9 avec `search(titre, contenu, Q) and contenu like Q`. Une recherche du rejeu rendait 408 résultats, la page ensuite lue au rang 10 : en mots exacts, 11 résultats, la même page au rang 3.
- **Deux autres causes, plus petites, que cette décision ne traite pas.** (1) Tous les mots sont exigés : une page à qui il manque un mot ne sort pas. `BOI-CF-IOR-30-20` ne porte pas deux des mots de la recherche qui ne l'a pas trouvée : la cause est là, pas dans l'absence du filtre de série que 09 §2.20 relevait. (2) Le classement : `BOI-BAREME-000001` porte tous les mots de « barème kilométrique frais de déplacement » et sort au rang 12 sur 20 avec `kind="all"`, derrière neuf actualités.

## Décision

1. **Les mots exacts d'abord** (Q36 = A). Une recherche envoie `search(contenu, Q) and contenu like Q`, suivie des filtres de nature et de série inchangés. Le portail rend les publications qui portent **tous** les mots, **tels qu'ils sont écrits**, accents et casse ignorés, dans n'importe quel ordre. Le dernier mot n'est plus un préfixe. Le classement reste celui du portail sur le texte seul : ADR-0028 n'est pas changé sur ce point.
2. **La recherche approchée en secours, et seulement quand rien d'exact n'existe.** Si la requête exacte ne trouve **aucune** publication (total nul), la recherche de la 1.3.1 est envoyée (`search(contenu, Q)` seul, mêmes filtres, même page) et la réponse le dit dans `note`. Si la requête exacte trouve au moins une publication, la recherche approchée n'est jamais envoyée, même pour une page au-delà de la dernière.
3. **Où la règle est écrite.** La requête du domaine (`DoctrineQuery`) porte le mode demandé, exact ou approché. Le cas d'usage de `lf-application` décide du second appel ; l'adapter ne fait que traduire le mode en clause. La signature du port `TaxDoctrine` ne change pas.
4. **Les mots et la clause.** La mise en forme des mots ne change pas (ADR-0029, décision 3 : forme NFC, tout caractère qui n'est ni une lettre ni un chiffre devient un espace) : aucun caractère spécial ne peut atteindre le littéral de `like`. La clause exacte porte les mots dans deux littéraux, écrits par la même fonction que tous les autres.
5. **Ce que la réponse dit.** Résultats approchés servis : une `note` le dit (aucune publication ne porte tous les mots tels quels ; ceux-ci ne correspondent qu'à peu près, à vérifier sur le titre). Aucun résultat, même approché : la note « No match » de la 1.3.1, mise à jour. Résultats exacts : pas de note. Aucun champ nouveau.
6. **Surface et classement du changement.** La description de `fr_search_bofip` est récrite (les mots tels quels ; plus de préfixe ; le secours et sa note ; essayer le singulier et le pluriel). Son schéma de sortie garde son contenu ; son schéma d'entrée aussi, sauf la description du paramètre `query`, récrite de même (précisé avec le code). Les quatorze autres descripteurs et les instructions du serveur ne changent pas. **Classement proposé : correctif.** 05 §9.5 range dans les correctifs une description reformulée « à sens constant » ; ici la règle décrite change sur un point, le dernier mot n'est plus un préfixe. Ce document le classe quand même en correctif, pour une raison : la description de la 1.3.x promettait « tous les mots », et le portail rendait des mots seulement approchants ; le changement met le comportement d'accord avec cette promesse. Aucun paramètre, champ ni valeur n'est ajouté ni retiré. Le numéro de version reste décidé par le propriétaire à la PR de version ; une version mineure est l'autre lecture possible. *Décidé le 2026-11-16 : correctif, 1.3.2.* Si `lock-descriptors --compare` relève autre chose qu'une description récrite, le travail s'arrête et le propriétaire est prévenu.
7. **Débit et quota.** Une recherche compte pour une requête, deux au plus, chacune dans le débit (deux départs par seconde) et dans le quota du jour (10 000 par processus) d'ADR-0029. La seconde requête est une autre question posée après une réponse reçue : ce n'est pas un réessai, et il n'y en a toujours aucun après une erreur.
8. **Rien d'autre ne change** : hôte, adresse, `https` seul, redirections refusées, plafond de 1 Mio après décompression, champs demandés (toujours aucun texte), validation des identifiants, erreurs, repos après un 429, audit (une ligne par appel d'outil), absence de cache ; `fr_get_bofip` ; les treize outils de Légifrance.

## Question tranchée

Posée au propriétaire avec une recommandation ; réponse du 2026-11-16 : A.

| Question | Décision | Options écartées |
|---|---|---|
| **Q36** Règle de correspondance des mots d'une recherche | **A** : les mots exacts ; la recherche approchée seulement quand rien d'exact n'existe, et la réponse le dit | B : les résultats exacts, puis les approchants à la suite, étiquetés (rien n'est perdu, mais quatre résultats sur cinq restent du bruit dans la réponse). C : ne rien changer, et écrire dans la description que la recherche est approchée (le bruit reste) |

## Conséquences

### Positives

- Sur les 31 recherches du rejeu, quatre résultats sur cinq disparaissent (820 → 169), et 27 recherches tiennent dans une page.
- La page attendue monte : dans les cinq premiers résultats pour 8 requêtes sur 9, contre 5.
- La règle se dit en une phrase et l'appelant peut la suivre : les mots tels qu'il les écrit.
- Une faute de frappe ne laisse pas l'appelant sans rien : si aucun résultat exact n'existe, la recherche approchée répond, et le dit.

### Négatives

- **Le singulier et le pluriel ne se valent plus**, ni le masculin et le féminin : « donation » ne trouve plus « donations ». Sur les 25 publications lues dans le rejeu, trois ne sortent plus de la recherche qui les avait trouvées. La description de l'outil le dit ; l'appelant doit reformuler.
- **Plus de recherches sans résultat exact** (5 sur 31 au lieu de 2) : chacune coûte une seconde requête, et sa réponse est celle de la 1.3.1, avec une note.
- **Le dernier mot n'est plus un préfixe** : un début de mot ne suffit plus.
- La description de `fr_search_bofip` change, et pas à sens constant sur le préfixe du dernier mot (décision 6) : un descripteur du verrou change, et les clients qui épinglent les descripteurs redemandent une approbation.
- Deux clauses à entretenir, et le sens de `like` tient à une mesure (V-227).

### À surveiller

- **NON VÉRIFIÉ** [V-227 · bofip-like-mots-exacts] : la définition de `like` n'a pas été lue dans la documentation du portail (elle n'y figure, dans les pages lues, que par un exemple : `my_text_field like "paris"`). Son sens est établi par la mesure : dix totaux prédits au résultat près. Un test en direct hebdomadaire contrôle qu'un mot mal écrit n'a aucun résultat exact et qu'il en a par la recherche approchée.
- Le modèle local ne redonne pas tous les totaux de la recherche approchée (28 sur 44) : le traitement du dernier mot par le portail reste mal compris. Cela ne touche que le secours, dont la réponse est celle de la 1.3.1.
- Les deux autres causes du bruit restent : tous les mots sont exigés, et le classement peut placer des actualités devant la page de doctrine avec `kind="all"`. Elles ne sont pas traitées ici ; un mode « la plupart des mots » demanderait un paramètre de plus, donc une version mineure et un nouvel ADR.
- Le jeu change chaque semaine : les totaux de ce document sont ceux de l'export du 2026-11-15 (celui du 2026-11-16, téléchargé pour la mesure finale, porte les mêmes 9 173 textes).
- Le découpage en mots du portail (précision 2 ci-dessus) écarte un mot collé à la phrase précédente par un point, dans les deux modes : c'est une limite du portail, que ce document ne corrige pas.
- Condition de révision : si la mesure faite avec le code de la branche (07 §2.11, garantie J4) s'écarte de ces chiffres, le travail s'arrête avant la fusion et ce document est repris. **Mesure faite le 2026-11-16** : les 31 recherches du rejeu et les 9 requêtes du classement, envoyées au binaire de la branche (45 requêtes) ; pour les 35 recherches qui ont des résultats exacts, la liste rendue est exactement celle que le modèle local avait prédite ; le secours a joué pour les 5 recherches prédites et pour elles seules ; 169 résultats, 27 recherches en une page, 22 pages lues sur 25, page attendue dans les cinq premiers pour 8 requêtes sur 9 (mêmes rangs que ci-dessus).

### Tests qui la gardent

À écrire avec le code : clause exacte et clause approchée pour chaque combinaison de nature et de série ; propriété sur le nombre de littéraux de chaque clause, quelle que soit l'entrée ; second appel envoyé si et seulement si le total exact est nul ; aucun second appel quand le premier échoue ; une erreur du second appel est rendue ; même page demandée aux deux appels ; les trois notes (résultats approchés, aucun résultat, aucune note) ; budget de réponse tenu avec la note ; réponses réelles du portail (exacte avec résultats, exacte vide suivie de la recherche approchée) ; description de l'outil mot pour mot, dans les bornes de 05 ; test en direct de V-227 ; aucune erreur ne contient un mot de la requête (test d'ADR-0029, étendu à la seconde requête).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Résultats exacts puis approchants, étiquetés (Q36 = B) | Deux requêtes à chaque recherche ; le bruit reste dans la réponse et dans le budget de caractères. |
| Ne rien changer, le dire dans la description (Q36 = C) | Le défaut relevé par le rejeu reste entier. |
| Un `search()` par mot (chaque mot en préfixe) | 198 résultats au lieu de 169 sur le rejeu, une page lue de plus retrouvée (23 sur 25) ; mais un mot court devient un préfixe très large, et le pluriel n'est retrouvé que dans un sens (« donation » trouve « donations », pas l'inverse). |
| Classer avec le titre (`search(titre, contenu, Q)`) | 7 requêtes sur 9 au lieu de 8 : pas mieux que le texte seul ; ADR-0028 reste inchangé sur ce point. |
| Chaîne nue, `suggest()` | Approchée dans tous les champs (874) ; un seul résultat. |
| Reclasser sur la machine d'après les titres | Non mesuré ; une règle maison de plus, alors que le classement du portail suffit une fois les approchants retirés. |
| Un paramètre `match` (« tous les mots » ou « la plupart ») | Traite une autre cause ; un paramètre de plus, donc une version mineure. À reprendre par un ADR si l'usage le demande. |
| Un index local du jeu | Écarté par ADR-0028 (aucun téléchargement, aucun index) ; V-220 reste ouverte. |

## Références et sources

- Jeu et API : ceux d'[ADR-0028](0028-bofip-doctrine-fiscale.md). Documentation de `search()`, de `suggest()` et de la clause `where` : aide en ligne d'Opendatasoft, « Explore API v2.1 », lue le 2026-11-16. Mesures du 2026-11-16 : index local sur l'export `jsonl` du 2026-11-15, 28 requêtes `records`, recherches gardées du rejeu ; outils et réponses hors dépôt.
- [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.14 ; [07-feuille-de-route.md](../plan/07-feuille-de-route.md) §2.11 (tâches T-MB-58 à T-MB-66) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.20 et §2.22 (V-221, V-227).
- [00-decisions.md](../plan/00-decisions.md) §2, §14 (AM-98) ; [ADR-0029](0029-bofip-precisions-apres-mesure.md) décisions 3 et 7.
