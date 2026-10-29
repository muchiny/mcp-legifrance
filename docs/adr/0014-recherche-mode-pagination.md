# ADR-0014 — Recherche : mode par défaut et pagination

- **Statut** : **Proposé**. L'acceptation dépend de Q10 (défaut `any` adopté jusqu'à V-79, en M2, puis `all` si V-79 le justifie ; décision consignée avant le verrou des descripteurs de M4) et des mesures de M5 (pagination profonde, V-83 ; `typePagination` en page 2, V-82).
- **Date** : 2026-10-15
- **Jalons concernés** : M2 (payload `/search`, mesure V-79), M3 (validations du cas d'usage), M4 (verrou des descripteurs : Q10 tranchée avant), M5 (pagination, mesures, v0.1.0), M7 (`fr_search_all`)
- **Questions liées** : Q10 (proposition adoptée par défaut, à confirmer)
- Point au 2026-10-29 (F6-b, d'après l'exécution live de F2, 06 §5.4) : le statut **reste « Proposé »**. Les mesures de M5 ont réussi en sandbox : `live_type_pagination_page2_code` (V-82 : la page 2 de `CODE_DATE` ne rejoue pas la page 1 quand le mémo de `typePagination` est rejoué) et `live_deep_pagination_limit` (V-83 : la page 100 de 100 résultats, positions 9 901 à 10 000, est pleine et le total atteint la fenêtre). En revanche la condition de Q10 n'est pas remplie : V-79 n'est que **partielle** (l'ordre `any` ≥ `all` ≥ `exact` est confirmé, 09 §2.20 ; le sens de `all` avec `typeChamp = ALL` pour des mots répartis entre titre et article, et le rappel en `all` sur les 9 scénarios de la référence, ne sont **pas mesurés**), et la réponse du propriétaire à Q10 n'est pas consignée. Le défaut reste `any`. Le passage à « Accepté » attend cette réponse ; il n'est pas déduit des seules mesures M5.
- Révisé le 2026-10-15 (revue) : défaut de `match` et calendrier de Q10 (AM-41) ; `PaginationMemo` et rejets `invalid_arg` dans le cas d'usage (00 §7, AM-04) ; `has_more` borné par la fenêtre atteignable ; copie défensive de l'adapter (00 §5, AM-28) ; contrôle final du 2026-10-16 : règle exacte de `has_more` (05 §5.1, 03 §4.1).

## Contexte

- **La référence ne permet ni de choisir le mode ni de paginer** (D-09, D-32 ; recherche 02 §2.d) :
  - `typeRecherche` est figé à `UN_DES_MOTS` (`server.py:144`) ;
  - `pageNumber` est figé à 1 et aucun paramètre de page n'existe (`server.py:150`, `:174`).
- **Conséquences mesurées** :
  - « un des mots » est un OU logique : « republique numerique » ramène 1 786 textes LODA (`search_loda.json`) ;
  - les totaux de 122 147 (JURI « responsabilite », `search_juri.json`) et de 51 531 (CETAT, `search_cetat.json`) sont inaccessibles au-delà des 100 premiers résultats.
- **Ce que permet l'API** (Swagger 2.4.2, `CritereDTO.typeRecherche`) : `UN_DES_MOTS`, `EXACTE`, `TOUS_LES_MOTS_DANS_UN_CHAMP`, `AUCUN_DES_MOTS` et `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION` (04 §6.4). `pageSize` vaut au plus 100 et `pageNumber` commence à 1 (recherche 05 §2.4).
- **Points non vérifiés de l'API** :
  - aucune limite de profondeur n'est documentée ; Elasticsearch a une fenêtre de 10 000 résultats par défaut : **NON VÉRIFIÉ** [V-83 · pagination-profonde] ;
  - il faut renvoyer la `typePagination` reçue (« passer la valeur reçue dans la réponse précédente », `RechercheSpecifiqueDTO`) ; `CODE_DATE` répond `ARTICLE` à une requête `DEFAUT` (`search_code.json`, 04 §6.6).
- **Les outils sont sans état** : aucun handle ; les curseurs sont des entiers rejouables (recherche 06 §3.6, 05 §1.4 règle 7).
- **Désaccord entre sources.** La recherche 02 (D-32) recommande `tous_les_mots` par défaut, plus précis. 00 §7 adopte `any` par défaut, pour la parité ; la décision revient à l'utilisateur (Q10, 00 §13).

## Décision (proposée)

1. **Paramètre `match`** de `fr_search` : `any` (défaut adopté jusqu'à V-79, Q10), `all` ou `exact` (00 §7, 04 §6.4).

   | `match` | `typeRecherche` |
   |---|---|
   | `any` | `UN_DES_MOTS` |
   | `all` | `TOUS_LES_MOTS_DANS_UN_CHAMP` |
   | `exact` | `EXACTE` |

   Les autres valeurs de l'API ne sont pas exposées. Le reste du payload ne change pas : `typeChamp ALL`, `operateur ET`, `sort PERTINENCE`.
2. **Pagination par entiers rejouables** :
   - `page` ≥ 1, 1 par défaut ;
   - `page_size` de 1 à 100, 10 par défaut ;
   - la sortie porte `total` (valeur brute de l'API), `page` et `has_more` ;
   - `has_more` est vrai si la page suivante existe **et** reste atteignable dans la fenêtre de 10 000 résultats : `page × page_size < total` et `(page + 1) × page_size ≤ 10 000`, même borne que la validation de la décision 3 (05 §5.1, 03 §4.1). Cela revient à `page × page_size < min(total, 10 000)` quand `page_size` divise 10 000 ; avec `page_size = 30`, la page 333 a `has_more = false`, car la page 334 serait refusée. À la fin de la fenêtre, `has_more` vaut `false` même si `total` est plus grand, et une `note` invite à affiner la requête (lacune FR-008).
3. **Validations sans réseau, dans le cas d'usage** (00 §7, AM-04) : rejet `[invalid_arg]` de `page × page_size` > 10 000 (fenêtre supposée, V-83, confirmée en M5), de `version_date` hors LODA/CODE, de `date_from`/`date_to` sur un fond sans facette de date (CODE) et de `date_from` > `date_to`, avec les messages de 05 §7.3 (filtrer par `date_from`/`date_to` ou par `match`). On évite ainsi un 500 opaque ou un filtre ignoré en silence.
   - L'adapter n'en garde qu'une copie défensive (`CorpusError::InvalidRequest`, 00 §5 ; 04 §6.5, §6.7), qu'aucun appel normal n'atteint : le rejet a lieu avant l'appel du port.
4. **`typePagination`, jamais exposée au modèle** (00 §7) :
   - elle n'est ni un paramètre d'entrée ni un champ de sortie ;
   - l'adapter mémorise la valeur reçue dans un `PaginationMemo`, dont la clé est la requête normalisée **sans** `page` (TTL 1 h, au plus 1 024 entrées), et la renvoie à PISTE pour la page suivante, comme l'exige le Swagger ;
   - sans mémo (redémarrage, autre processus) : `ARTICLE` pour CODE en page > 1, `DEFAUT` sinon (04 §6.6) ;
   - si les mesures de M5 montrent que le mémo ne suffit pas (V-82), on ajoutera un paramètre opaque `pagination`, classé comme « ajout » (05 §13.1 n° 7).
5. **Une recherche vide est un succès** `{"total":0,"page":1,"has_more":false,"hits":[]}`, jamais `not_found` (parité E-04).
6. **Requête normalisée par le domaine** (`QueryText`) : trim, blancs repliés, caractères de contrôle retirés, 1 à 450 caractères (D-32 ; borne ramenée de 500 à 450 par V-81).
7. **Le défaut et la manière d'obtenir la suite** figurent dans la description de `fr_search` et dans les instructions. Ils ne sont pas répétés dans chaque réponse ([ADR-0006](0006-sortie-compacte-retour-progressif.md)).

## Critère pour trancher Q10

- **Proposition adoptée par défaut** (00 §13, AM-41) : `any` (parité) jusqu'à V-79 (M2) ; passage à `all` si V-79 confirme la sémantique « tous les mots dans un champ quelconque » et un rappel acceptable sur les 9 scénarios de la référence.
- **Calendrier** (09 §3.10) : décision du propriétaire, sur les mesures du mainteneur, consignée **avant le verrou des descripteurs de M4**, puisque la description de `fr_search` annonce le défaut. D'ici là, on garde `any`, tant que [V-79 · mode-all-semantique] n'est pas levée.
- **Mesures** :
  - V-79 (M2) : totaux par mode relevés en sandbox par `live_match_modes_totals`, et rappel de `all` sur les 9 scénarios de la référence (`live_reference_scenario_1` à `_9`, 06 §5) ;
  - en complément, jeu d'évaluation de requêtes juridiques réalistes, sur tous les fonds (recherche 06 §7.3 i). On compare `any` et `all` sur la pertinence des 10 premiers hits (jugée par relecture) et sur le nombre d'appels nécessaires au modèle.
- **Si `all` réduit le bruit sans perdre de résultats pertinents**, le défaut passe à `all` avant le verrou de M4, donc avant v0.1.0. La divergence est alors listée dans l'annexe A.
- **Après v0.1.0**, changer le défaut modifie le résultat d'un appel inchangé : ce serait une **rupture** au sens de 05 §9.5 (passage de 0.x à 0.(x+1).0), avec une entrée « Tool surface » au CHANGELOG.

## Conséquences

### Positives

- **Corrige D-09** : on peut paginer jusqu'à la limite de l'API, filtrer par date et préciser la requête.
- Le défaut `any` garde le comportement de la référence, donc aucune surprise lors de la migration.
- Pas d'état exposé au modèle : la continuation passe par des entiers rejouables, compatibles avec le cache ([ADR-0007](0007-cache-memoire-port.md)). Le `PaginationMemo` n'est qu'un cache interne : sans lui, la page est servie avec la valeur de repli, sans erreur (05 §1.4 règle 7).
- `has_more` n'annonce jamais une page que la validation refuserait.

### Négatives

- **`any` reste bruyant** (D-32) : le modèle doit penser à `all` ou `exact`. La description le lui dit.
- Le mémo de `typePagination` est un état caché, propre au processus : il est perdu au redémarrage et n'est pas partagé entre clients. Le repli sans mémo est `DEFAUT` pour tous les fonds : le repli `ARTICLE` pour CODE, tiré de `search_code.json`, rejouait la page 1 en sandbox (V-82, 2026-10-28, tâche C5 du plan 2h) ; l'écart éventuel entre une page servie avec le mémo et sans lui, ailleurs ou en production, reste mesuré par V-82 (M5).
- La borne de 10 000 est une hypothèse. Si elle est fausse, elle prive de résultats atteignables ou laisse passer des erreurs.

### À surveiller

- **Partiel** [V-79 · mode-all-semantique] : en sandbox (2026-10-28), `any` ≥ `all` ≥ `exact` (JURI 606 333, 119 868, 86 213 ; LODA 125 908, 4 633, 554) ; sens exact de `TOUS_LES_MOTS_DANS_UN_CHAMP` avec `typeChamp=ALL` et rappel sur les 9 scénarios non mesurés (09 §2.20 ; base de Q10, T-M4-10).
- **NON VÉRIFIÉ** [V-82 · typepagination-page2] : effet de `DEFAUT` ou `ARTICLE` en page 2, et cohérence de `has_more` (V-82, M5).
- **NON VÉRIFIÉ** [V-83 · pagination-profonde] : limite de profondeur (V-83, M5).
- **Vérifié en sandbox le 2026-10-28** [V-81 · longueur-max-query] : l'API accepte 450 caractères et refuse 499 (500), soit une limite entre 451 et 498 (09 §2.20).
- **Vérifié en sandbox le 2026-10-28** [V-78 · filtres-dates-effet] : les sept facettes réduisent le total et leurs bornes sont incluses ; borne lointaine non essayée (09 §2.20).

### Tests qui la gardent

- `search_page_2_sends_page_number_2`, `has_more_computed_from_total`, `has_more_false_at_reachable_window_end`, `type_pagination_is_echoed_on_next_page`, `code_page_2_without_memo_sends_defaut` (D-09 ; `ARTICLE` avant la tâche C5 du plan 2h).
- `page_beyond_reachable_window_is_invalid_arg_without_network`, `code_date_range_is_invalid_arg`, `version_date_outside_loda_code_is_invalid_arg`, `inverted_date_range_is_invalid_arg` (0 appel au port, 06 §2.2) ; copie défensive de l'adapter : `code_date_range_is_defensive_invalid_request_without_network` (04 §6.10).
- `type_recherche_follows_match`, `query_too_long_is_invalid_arg`, `query_control_chars_are_stripped` (D-32).
- Recherche vide rendue comme un succès (E-04) ; tests live de M5 (06 §3.1, §5.3).

## Alternatives envisagées

| Option | Raison du rejet (provisoire) |
|---|---|
| `all` par défaut dès maintenant (recherche 02 D-32) | Rompt la parité sans mesure. Le sens exact de `all` n'est pas vérifié. La décision revient à l'utilisateur (Q10). |
| `all`, puis repli automatique sur `any` si aucun résultat (09 §3.10, option C) | Une requête de plus, et un sens de la réponse qui dépend du résultat. Reste envisageable si les mesures de M5 le justifient. |
| Exposer les cinq valeurs de `typeRecherche` | Les modes négatifs (`AUCUN_DES_MOTS`…) n'ont pas d'usage seuls et alourdissent la description. |
| Curseur ou handle opaque rendu par le serveur | État serveur et vecteur d'attaque. Les entiers suffisent (recherche 06 §3.6). |
| Rendre `typePagination` en sortie et l'accepter en entrée dès v0.1 | Ajoute un paramètre que le modèle doit recopier sans le comprendre, pour un effet non vérifié (V-82). Gardé en repli selon les mesures (05 §13.1 n° 7). |
| Règle sans état seule (`DEFAUT`, `ARTICLE` pour CODE avant V-82), sans mémo | 00 §7 retient le mémo, qui renvoie la valeur reçue comme le demande le Swagger ; la règle sans état en reste le repli. |
| Refus de la pagination profonde dans l'adapter seulement (`InvalidRequest`) | Rendu `upstream_error` « HTTP 400 » sans appel réseau ; une contrainte de l'entrée doit donner `invalid_arg` avant le port (00 §5, AM-28). |
| `not_found` sur une recherche vide (instructions de la référence) | Contredit le comportement réel de la référence (E-04). Une liste vide n'est pas une erreur. |
| Pas de borne de profondeur | Risque d'un 500 opaque au-delà de la fenêtre du moteur. |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §5, §7, §8, §13 (Q10), §14 (AM-04, AM-41) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §3.10 ; [06-qualite.md](../plan/06-qualite.md) §2.2 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.1, §7.3, §9.5, §13.1 n° 7 et 8 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §6.1, §6.4 à §6.8 ; [03-domaine.md](../plan/03-domaine.md) §4.1 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §4.5 (`search_params_added`, `query_normalized`).
- Recherche : [02-defauts.md](../recherche/02-defauts.md) §2.d, D-09, D-32 ; [05-piste-dila.md](../recherche/05-piste-dila.md) §2.1, §2.4 ; [03-dto.md](../recherche/03-dto.md) §3.2 ; [06-mcp-spec.md](../recherche/06-mcp-spec.md) §3.6, §7.3 ; [01-parite.md](../recherche/01-parite.md) §3.1, E-04.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/server.py:140-174, 191-222` ; fixtures `search_loda.json`, `search_juri.json`, `search_cetat.json`, `search_code.json`.
