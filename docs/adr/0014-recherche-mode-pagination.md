# ADR-0014 — Recherche : mode par défaut et pagination

- **Statut** : **Proposé**. L'acceptation dépend de Q10 (`any` ou `all`) et des mesures de M5 (pagination profonde, `typePagination` en page 2).
- **Date** : 2026-10-15
- **Jalons concernés** : M2 (payload `/search`), M5 (pagination, mesures, v0.1.0), M7 (`fr_search_all`)
- **Questions liées** : Q10

## Contexte

- **La référence ne permet ni de choisir le mode ni de paginer** (D-09, D-32 ; recherche 02 §2.d) :
  - `typeRecherche` est figé à `UN_DES_MOTS` (`server.py:144`) ;
  - `pageNumber` est figé à 1 et aucun paramètre de page n'existe (`server.py:150`, `:174`).
- **Conséquences mesurées** :
  - « un des mots » est un OU logique : « republique numerique » ramène 1 786 textes LODA (`search_loda.json`) ;
  - les totaux de 122 147 (JURI « responsabilite », `search_juri.json`) et de 51 531 (CETAT, `search_cetat.json`) sont inaccessibles au-delà des 100 premiers résultats.
- **Ce que permet l'API** (Swagger 2.4.2, `CritereDTO.typeRecherche`) : `UN_DES_MOTS`, `EXACTE`, `TOUS_LES_MOTS_DANS_UN_CHAMP`, `AUCUN_DES_MOTS` et `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION` (04 §6.4). `pageSize` vaut au plus 100 et `pageNumber` commence à 1 (recherche 05 §2.4).
- **Points non vérifiés de l'API** :
  - aucune limite de profondeur n'est documentée ; Elasticsearch a une fenêtre de 10 000 résultats par défaut : **NON VÉRIFIÉ** [v:pagination-profonde] ;
  - il faut renvoyer la `typePagination` reçue (« passer la valeur reçue dans la réponse précédente », `RechercheSpecifiqueDTO`) ; `CODE_DATE` répond `ARTICLE` à une requête `DEFAUT` (`search_code.json`, 04 §6.6).
- **Les outils sont sans état** : aucun handle ; les curseurs sont des entiers rejouables (recherche 06 §3.6, 05 §1.4 règle 7).
- **Désaccord entre sources.** La recherche 02 (D-32) recommande `tous_les_mots` par défaut, plus précis. 00 §7 propose `any`, pour la parité, et laisse la décision à l'utilisateur (Q10).

## Décision (proposée)

1. **Paramètre `match`** de `fr_search` : `any` (défaut proposé, Q10), `all` ou `exact` (00 §7, 04 §6.4).

   | `match` | `typeRecherche` |
   |---|---|
   | `any` | `UN_DES_MOTS` |
   | `all` | `TOUS_LES_MOTS_DANS_UN_CHAMP` |
   | `exact` | `EXACTE` |

   Les autres valeurs de l'API ne sont pas exposées. Le reste du payload ne change pas : `typeChamp ALL`, `operateur ET`, `sort PERTINENCE`.
2. **Pagination par entiers rejouables** :
   - `page` ≥ 1, 1 par défaut ;
   - `page_size` de 1 à 100, 10 par défaut ;
   - la sortie porte `total`, `page` et `has_more`, avec `has_more = page × page_size < total` (05 §5.1).
3. **Pagination profonde** : proposition à confirmer en M5. Si `page × page_size > 10 000`, la réponse est `[invalid_arg]`, avec un message qui invite à filtrer par `date_from`/`date_to` ou par `match` (04 §6.7, 05 §7.3). On évite ainsi un 500 opaque.
4. **`typePagination`, « renvoyée telle que reçue »** (00 §7) :
   - elle n'est ni un paramètre d'entrée ni un champ de sortie ;
   - l'adapter tient un `PaginationMemo`, dont la clé est la requête normalisée **sans** `page` (TTL 1 h, au plus 1 024 entrées) ;
   - en page > 1, il renvoie la valeur mémorisée, ou `ARTICLE` pour CODE à défaut de mémo (04 §6.6) ;
   - si les mesures de M5 montrent que le mémo ne suffit pas, on ajoutera un paramètre opaque `pagination`, classé comme « ajout » (05 §13.1 n° 7).
5. **Une recherche vide est un succès** `{"total":0,"page":1,"has_more":false,"hits":[]}`, jamais `not_found` (parité E-04).
6. **Requête normalisée par le domaine** (`QueryText`) : trim, blancs repliés, caractères de contrôle retirés, 1 à 500 caractères (D-32).
7. **Le défaut et la manière d'obtenir la suite** figurent dans la description de `fr_search` et dans les instructions. Ils ne sont pas répétés dans chaque réponse ([ADR-0006](0006-sortie-compacte-retour-progressif.md)).

## Critère pour trancher Q10

- **Calendrier** (09 §3.10) : décision du propriétaire avant le verrou des descripteurs de M4, avec un réexamen possible en M5. D'ici là, on garde `any`, tant que [v:mode-all-semantique] n'est pas levée.
- **Mesures** :
  - totaux par mode relevés en sandbox par `live_match_modes_totals` ;
  - jeu d'évaluation de requêtes juridiques réalistes, sur tous les fonds (recherche 06 §7.3 i). On compare `any` et `all` sur la pertinence des 10 premiers hits (jugée par relecture) et sur le nombre d'appels nécessaires au modèle.
- **Si `all` réduit le bruit sans perdre de résultats pertinents**, le défaut passe à `all` **avant** v0.1.0. La divergence est alors listée dans l'annexe A.
- **Après v0.1.0**, changer le défaut modifie le résultat d'un appel inchangé : ce serait une **rupture** au sens de 05 §9.5 (passage de 0.x à 0.(x+1).0), avec une entrée « Tool surface » au CHANGELOG.

## Conséquences

### Positives

- **Corrige D-09** : on peut paginer jusqu'à la limite de l'API, filtrer par date et préciser la requête.
- Le défaut `any` garde le comportement de la référence, donc aucune surprise lors de la migration.
- Pas d'état exposé au modèle : les appels restent rejouables et compatibles avec le cache ([ADR-0007](0007-cache-memoire-port.md)).

### Négatives

- **`any` reste bruyant** (D-32) : le modèle doit penser à `all` ou `exact`. La description le lui dit.
- Le mémo de `typePagination` est propre au processus. Il est perdu au redémarrage et n'est pas partagé entre clients, d'où un risque de page 2 incohérente sur CODE.
- La borne de 10 000 est une hypothèse. Si elle est fausse, elle prive de résultats atteignables ou laisse passer des erreurs.

### À surveiller

- **NON VÉRIFIÉ** [v:mode-all-semantique] : sens exact de `TOUS_LES_MOTS_DANS_UN_CHAMP` avec `typeChamp=ALL`.
- **NON VÉRIFIÉ** [v:typepagination-page2] : effet de `DEFAUT` ou `ARTICLE` en page 2, et cohérence de `has_more`.
- **NON VÉRIFIÉ** [v:pagination-profonde] : limite de profondeur.
- **NON VÉRIFIÉ** [v:longueur-max-query] : longueur maximale acceptée par l'API.
- **NON VÉRIFIÉ** [v:filtres-dates-effet] : effet réel des filtres de date.

### Tests qui la gardent

- `search_page_2_sends_page_number_2`, `has_more_computed_from_total`, `type_pagination_is_echoed_on_next_page`, `code_page_2_without_memo_sends_article` (D-09).
- `type_recherche_follows_match`, `query_too_long_is_invalid_arg`, `query_control_chars_are_stripped` (D-32).
- Recherche vide rendue comme un succès (E-04) ; tests live de M5 (06 §3.1, §5.3).

## Alternatives envisagées

| Option | Raison du rejet (provisoire) |
|---|---|
| `all` par défaut dès maintenant (recherche 02 D-32) | Rompt la parité sans mesure. Le sens exact de `all` n'est pas vérifié. La décision revient à l'utilisateur (Q10). |
| `all`, puis repli automatique sur `any` si aucun résultat (09 §3.10, option C) | Une requête de plus, et un sens de la réponse qui dépend du résultat. Reste envisageable si les mesures de M5 le justifient. |
| Exposer les cinq valeurs de `typeRecherche` | Les modes négatifs (`AUCUN_DES_MOTS`…) n'ont pas d'usage seuls et alourdissent la description. |
| Curseur ou handle opaque rendu par le serveur | État serveur et vecteur d'attaque. Les entiers suffisent (recherche 06 §3.6). |
| Rendre `typePagination` en sortie et l'accepter en entrée dès v0.1 | Ajoute un paramètre que le modèle doit recopier. Gardé en repli selon les mesures (05 §13.1 n° 7). |
| `not_found` sur une recherche vide (instructions de la référence) | Contredit le comportement réel de la référence (E-04). Une liste vide n'est pas une erreur. |
| Pas de borne de profondeur | Risque d'un 500 opaque au-delà de la fenêtre du moteur. |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §7, §8, §13 (Q10) ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.1, §7.3, §9.5, §13.1 n° 7 et 8 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §6.1, §6.4 à §6.8 ; [03-domaine.md](../plan/03-domaine.md) §4.1 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §4.5 (`search_params_added`, `query_normalized`).
- Recherche : [02-defauts.md](../recherche/02-defauts.md) §2.d, D-09, D-32 ; [05-piste-dila.md](../recherche/05-piste-dila.md) §2.1, §2.4 ; [03-dto.md](../recherche/03-dto.md) §3.2 ; [06-mcp-spec.md](../recherche/06-mcp-spec.md) §3.6, §7.3 ; [01-parite.md](../recherche/01-parite.md) §3.1, E-04.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/server.py:140-174, 191-222` ; fixtures `search_loda.json`, `search_juri.json`, `search_cetat.json`, `search_code.json`.
