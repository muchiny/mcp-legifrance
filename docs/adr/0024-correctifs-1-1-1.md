# ADR-0024 — Correctifs de la 1.1.1 et rejeu de non-régression en production

- **Statut** : **AM-88 acceptée** par le propriétaire le 2026-11-03, avec le lanceur `legifrance-run` relu et installé. La mesure T11 du 2026-11-02 (213 appels de production) a été faite avant cette confirmation explicite ; le propriétaire l'a ratifiée le 2026-11-03 et a retenu sa règle de franchissement « option a » : seuls un recouvrement et un décalage d'un jour hors version mort-née bloquent, un trou et le décalage d'une version mort-née sont neutres (T11 franchie). Les entrées AM-78 à AM-87 (statut **Proposé**) sont ajoutées à la fin de la vague 0 du plan de la 1.1.1, une par commit, pour les seules unités retenues.
- **Date** : 2026-11-02
- **Jalons concernés** : version 1.1.1 (correctifs du test de bout en bout de la 1.1.0 en production)
- **Questions liées** : Q3 (politique des tests live et des fixtures, 00 §10)
- Amendements : AM-78 (U14), AM-79 (U09), AM-83 (U11), AM-87 (U10) et AM-88 (00 §14), tous dans `00-decisions.md` §14. Les numéros AM-80, AM-81, AM-82, AM-84, AM-85 et AM-86 sont des propositions, pas des réservations : une unité qui sort de la 1.1.1 libère le sien, et cet ADR le dit en une ligne ; leurs entrées s'ajoutent ici avec le code de leur unité.

## Contexte

Le test de bout en bout de la 1.1.0 en production (2026-11-02) a confirmé 21 défauts importants et moyens, corrigés en 1.1.1. Le propriétaire exige qu'aucune réponse hors de ces défauts ne change. La preuve retenue est un rejeu des 2 092 appels distincts du test, sur la 1.1.0 et sur la 1.1.1, en production, avec le bruit de PISTE mesuré.

00 §10 (Q3) n'admet en production que des **sondes** en lecture seule, lancées à la main par le mainteneur avant chaque étiquette, à 1 requête par seconde au plus. Un rejeu de plusieurs milliers d'appels sur trois serveurs n'est pas une sonde : il faut une dérogation écrite.

## Décision

### Borne `date_fin` exclusive de `fr_diff_article` (AM-78, U14)

1. **Règle.** Une version d'article couvre `date_debut <= D < date_fin` : `date_fin` est le premier jour hors vigueur (jour d'entrée en vigueur de la suivante, ou jour de l'abrogation). Au jour de `date_fin` de la dernière version, `fr_diff_article` rend `not_found`, comme `fr_resolve`. Un trou entre deux versions ne sert rien ; une version de durée nulle ou mort-née (fin avant début) n'est jamais servie ; la plus récente l'emporte quand plusieurs contiennent le jour ; pas de `date_fin - 1`. Remplace « bornes `date_debut..=date_fin` » de 00 §7 (L5-02).
2. **`fr_get_act` (D19).** `date_fin_version` d'un texte suit la même règle (`end <= today` donne la note « past version »). Cet alignement est fait par analogie : la mesure T11 ne porte que sur des articles, aucun relevé au niveau du texte.
3. **Mesure T11 (production, 2026-11-02).** 213 lectures, 62 articles, 119 frontières : 99 contiguës, 14 terminales, 5 trous, 1 décalage d'un jour après une version mort-née (99 + 14 + 5 + 1 = 119), aucun recouvrement. Le propriétaire a retenu le 2026-11-03 la règle « option a » : seuls un recouvrement et un décalage d'un jour hors version mort-née bloquent. Le décalage mort-né est donc une exception ratifiée au seuil « 0 fin veille » de la porte T11, traitée par la règle des versions mort-nées ci-dessus.
4. **Limite connue.** Le libellé « Until {date_fin} » de la ressource `legifrance://article/{id}` se lit comme une borne incluse alors que la fin est exclue ; il n'est pas changé en 1.1.1 (aucun changement de surface).

### Page d'un code demandée à froid : relecture interne en mode `ARTICLE` (AM-79, U09)

1. **Règle.** Quand PISTE pagine un code par article (un seul code, beaucoup d'articles), une page > 1 de `fr_search` envoyée en `DEFAUT` n'existe pas côté PISTE : elle revient vide, alors que `totalResultNumber` est positif et que `typePagination` répond `ARTICLE`. Cela arrive quand l'adaptateur n'a pas encore appris le mode (processus neuf, mémo expiré ou évincé, clé différente de celle de la page 1). En 1.1.0, `fr_search` rendait alors `hits: []`, `has_more: false`, sans note. L'adaptateur relit désormais cette page **une fois** en `ARTICLE` (`reread_in_article`, `port.rs:183-196`, appelée par `search`, `port.rs:79-112`). Trois conditions sont communes : fond CODE, page envoyée en `DEFAUT`, mode reçu `ARTICLE`. S'y ajoute l'une des deux branches : la requête a été doublée (page de taille 1, branche déjà présente en 1.1.0, inchangée), ou la page est > 1, vide, avec `total > 0`. Hors branche doublée, il n'y a pas de relecture pour la page 1, une page non vide ou un `total` nul. Dans tous les cas, il n'y en a pas pour un autre fond, un mode reçu `DEFAUT` ou absent, ni quand la page est déjà partie en `ARTICLE` (table `reread_in_article_table`, `port.rs:985-1015`). La relecture ne change que `typePagination` (`port.rs:1052`). Le mémo retient le mode reçu à la première réponse (`port.rs:83`) puis celui de la relecture (`port.rs:106`) : les pages suivantes de la même requête partent directement en `ARTICLE` (`port.rs:877`).
2. **Ce qui est rendu.** La relecture n'est rendue que si l'un de ses résultats porte au moins un article (`port.rs:107`). Sinon, la première réponse vide est rendue, comme en 1.1.0 : une page au-delà de la fin des articles reste vide (`port.rs:1091-1127`) et le mémo a quand même appris `ARTICLE` (`port.rs:1103`). Sur la branche « doublée » (taille 1), la relecture est rendue telle quelle, comme avant (`port.rs:1250`). Hors de cette branche, une erreur de la relecture est rendue telle quelle (`port.rs:105`, test `port.rs:1066`) : là où 1.1.0 rendait une page vide muette, la 1.1.1 peut rendre une erreur ; un nouvel essai part alors directement en `ARTICLE`, car le mode est déjà appris.
3. **Pas de surface.** Aucun paramètre ni champ de sortie ne change. `00-decisions.md` §7 (règles de `fr_search`) et AM-04 prévoyaient, si le mémo ne suffisait pas, un paramètre opaque `pagination` (classe « ajout »). La 1.1.1 prend l'autre voie, interne à l'adaptateur. Le paramètre n'est pas retenu pour la 1.1.1.
4. **Coût.** Une requête `/search` de plus, seulement à froid, une fois par clé de mémo et par heure (la requête sans sa page, avec `page_size` : `memo.rs:41-49` ; durée `memo.rs:32`, 1 024 entrées au plus, `memo.rs:35`). Une page hors de la fin des articles paie aussi cette requête, puis rend un résultat vide. Un changement de `page_size`, de `match`, de date de version ou de période crée une clé neuve et relit une fois de plus (`port.rs:928`, `:948`). Le mémo est propre au processus : chaque processus neuf relit une fois par clé. Rien ne change à chaud, ni pour les autres fonds, ni pour la page 1.
5. **Limites connues, figées par un test garde et écrites au CHANGELOG (garantie 14).** (n° 1) CODE en mode `ARTICLE` avec `page_size` 1 : la page est envoyée telle quelle, sans doublement, comme en 1.1.0 (test `port.rs:1273`) ; le CHANGELOG consigne que PISTE y rend le même article à toutes les pages (constat du rejeu de production du 2026-11-02, pas une assertion du code). (n° 2) À chaud, une page au-delà de la fin des articles rend le résultat du code sans article, alors qu'à froid elle reste vide (test `port.rs:1291`) : un même appel rend donc `hits: []` au premier passage à froid, qui apprend le mode `ARTICLE`, puis le résultat du code sans article aux suivants.
6. **Hors 1.1.1.** Un envoi de `ceil(p/2)` en taille 2 pour la taille 1 (cas n° 1), qui exige un relevé en bac à sable du mode `ARTICLE` à taille 2. Une normalisation commune du froid et du chaud pour la page hors fin (cas n° 2). Un paramètre opaque `pagination`.

### Tiret et barre entre numéros du Conseil d'État (AM-83, U11)

1. **Grammaire.** `n° 194658-196116` et `n° 194658/196116` sont lus comme deux numéros de requête. Le second ne peut être une année (1500-2099) et aucun chiffre ni point ne le suit (`n° 194658-196116-5`, `n° 410000-5`, `n° 2021-1234` restent non lus). L'espace entre deux numéros n'est pas lu (1.2.0).
2. **Résolution.** Chaque numéro cité (3 au plus, hors 1500-2099) est essayé en recherche exacte `NUM_DEC` ; si aucun hit du Conseil d'État ne reste, une seule recherche plein texte CETAT exacte du premier numéro (page de 10, sans période) ne retient que les décisions dont le titre porte ce numéro. Coût d'une citation introuvable : N + 1 appels pour N numéros cités (N <= 3), contre 1 en 1.1.0. Une erreur du repli laisse `unresolvable`, jamais un échec ; elle est journalisée au niveau `debug` par le nom de sa variante seul.
3. **Note.** « Only one of the case numbers was checked. » s'applique à toute décision du Conseil d'État citée avec plus d'un numéro, y compris quand le second est une année jamais cherchée ou que le premier numéro a suffi.

### Lecture du sommaire d'un texte non codifié (AM-87, U10 et étage 2 d'U01)

1. **Règle.** Pour un article d'une loi, d'un décret ou d'une ordonnance, l'échec de `find_article` ne prouve pas l'absence : `getArticleWithIdAndNum` ne rend que la version en vigueur. Le résolveur lit le sommaire du texte au jour cité (à défaut celui d'aujourd'hui) une fois par appel, puis l'article de même numéro (les suites de chiffres comptent : `10-2` et `102` sont deux articles). Un article modificatif (ni état ni date) est `verified` de confiance moyenne avec la note `ArticleWithoutVersion` ; un article absent d'un sommaire complet reste `not_found` ; un sommaire incomplet, trop gros ou illisible donne `unresolvable` ou l'erreur amont. Les codes ne lisent aucun sommaire.
2. **Étage 2 d'U01.** Le repli daté de l'adaptateur lit lui-même la table des matières à la date pour un texte non codifié, mais ne rend l'article que si sa lecture le dit en vigueur ce jour-là (`in_force_on` : début présent, fin exclue) et si le numéro est le même (suites de chiffres comprises). Un article modificatif, ou une version qui ne couvre pas la date, est un `NotFound` de recherche, et le sommaire du résolveur le qualifie : jamais un `verified` de confiance haute.
3. **Coût.** Sur le chemin daté qui échoue seulement : au plus 12 appels amont par citation à deux variantes de clé (U01), plus la lecture du sommaire et celle de l'article listé (U10, mémorisées par appel). Un `lawDecree` peut peser jusqu'à 16 Mio ; les lectures de sommaire de l'étage 2 ne passent pas par le mémo du résolveur (limite connue, bornée par le limiteur de débit et le budget de temps de l'appel).

### Dérogation de rejeu de production (AM-88)

1. **Objet.** Le rejeu de non-régression de la 1.1.1 (plan de la 1.1.1, section 6) peut appeler l'API PISTE de production, en lecture seule : rejeu de base de la 1.1.0, pilote A/A (bruit), mesure de la borne `date_fin` (porte T11 de l'unité U14, au moins 130 lectures), « porte 0 » de chaque vague (environ 300 appels) et rejeu complet final (2 092 appels sur trois serveurs : 1.1.0, 1.1.1, 1.1.0).
2. **Débit.** Débit PISTE **cumulé** de 1 requête par seconde au plus : un seul serveur actif à la fois, `LF_PISTE_RPS=1`, `LF_PISTE_BURST=1`, `LF_PISTE_CONCURRENCY=1`. Sur `[rate_limited]`, attente du délai donné.
3. **Lancement.** À la main, sur la machine du mainteneur, par le lanceur `legifrance-run` (hors dépôt, relu par le propriétaire et par l'agent `security-auditor` avant installation). Le lanceur lit lui-même les identifiants ; aucun secret en argument, en fichier du dépôt, en fixture ni en journal. Jamais en CI.
4. **Portée.** Seulement les binaires de la liste autorisée du lanceur (1.1.0 sauvegardée, candidats 1.1.1 construits depuis la branche de correctifs). Sorties hors dépôt (`/var/tmp/lf-nr/`), droits 0700. Les réponses rejouées ne deviennent pas des fixtures.
5. **Durée.** Cette dérogation vaut pour la 1.1.1 seulement. Elle tombe à la publication de la 1.1.1, ou à la décision du propriétaire de l'arrêter.

## Conséquences

- La release 1.1.1 exige un rejeu complet qui PASSE (aucun écart inattendu, indéterminé ou instable) sur la tête de la branche de correctifs avant la fusion. Sans rejeu, la release est suspendue, sauf accord écrit du propriétaire de livrer sans rejeu.
- Le quota de production réel n'est pas documenté : un `[rate_limited]` est noté au compte rendu.
- La politique Q3 reste inchangée pour tout le reste : sandbox par défaut, production jamais en CI.

## Alternatives écartées

| Alternative | Raison |
|---|---|
| Rejeu en sandbox | La sandbox n'a pas les données du test (production) : les réponses ne seraient pas comparables à celles du test E2E. |
| Échantillon seulement | Le propriétaire veut la certitude qu'aucune réponse du test ne change ; un échantillon laisse passer une régression rare. Un échantillon sert seulement au pilote et à la porte 0. |
| Fixtures enregistrées au lieu de la production | Une fixture fige une réponse amont ; elle ne prouve pas l'effet du correctif sur la forme réelle d'aujourd'hui. |
