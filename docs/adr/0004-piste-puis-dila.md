# ADR-0004 — PISTE d'abord, DILA ensuite derrière le même port

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M2 (adapter PISTE), M3 (port, fait avant l'adapter de M2 : T-M3-01, 00 §11), M6 et M7 (extensions du port), M9 (`lf-adapter-dila`, v1.0.0)
- **Questions liées** : Q3 (compte PISTE, sandbox puis production), Q6 (accès réseau à `echanges.dila.gouv.fr`), Q15 (attribution Licence Ouverte et date) ; propositions adoptées par défaut, à confirmer (00 §13)
- Révisé le 2026-10-15 (revue) : `list_codes()` en M6 (00 §3, AM-29) ; politique des tests live et des fixtures de production (00 §10, AM-40, Q3) ; attribution des données (00 §12, AM-39, Q15) ; classement des erreurs hors ligne renvoyé à M9 (00 §5, AM-28) ; contrôle final du 2026-10-16 : date de consultation rendue par `fr_coverage`, à laquelle renvoient les instructions statiques (00 §12, AM-39).

## Contexte

- **Source unique de la référence.** La référence n'interroge que l'API Légifrance `lf-engine-app` derrière PISTE. Elle utilise 7 endpoints `POST` JSON et OAuth2 `client_credentials` (recherche 01 §0). Le Swagger 2.4.2 décrit 68 opérations ; sa surface est identique en sandbox et en production (recherche 05 §1, §3.2).
- **Contraintes de PISTE** :
  - les applications sandbox et production sont **distinctes** ;
  - la sandbox n'a aucun engagement de service ; la production vise 95 % par jour, sans niveau garanti (recherche 05 §3.2) ;
  - les quotas ne sont pas publics : **NON VÉRIFIÉ** [v:quota-piste-prod] ;
  - les données de la sandbox « ne sont pas identiques à celles de Production » : ampleur **NON VÉRIFIÉE** [v:sandbox-prod-donnees].
- **CGU** : clés « strictement réservées à votre application », données sous Licence Ouverte 2.0, aucune clause qui limite le cache local (recherche 05 §4.1, §4.2).
- **Dumps DILA** (recherche 05 §6) :
  - archives `Freemium_<fond>_global_*.tar.gz`, puis incréments quotidiens à appliquer dans l'ordre strict, avec listes de suppression, sous `https://echanges.dila.gouv.fr/OPENDATA/<FOND>/` ;
  - le dernier global LEGI date du 2025-07-13 ; le rythme des globaux est **NON VÉRIFIÉ** [v:dila-rythme-globaux] ;
  - volumes observés chez un tiers : `legi.db` 3,8 Go, `jade.db` 14 Go, judiciaire environ 21 Go (recherche 05 §6.5) ;
  - la base LEGI diffusée peut différer de Légifrance (recherche 05 §6.3) ;
  - tantivy 0.26.2 fournit un stemmer et des mots vides français (recherche 05 §6.7).
- **Réseau.** L'hôte `echanges.dila.gouv.fr` était injoignable pendant la recherche (recherche README) : Q6.
- **Judilibre** est une API distincte de la Cour de cassation, non intégrée par la référence. C'est une lacune de couverture (D-34, recherche 05 §5).

## Décision

1. **v0.1.0 à v0.3.0 : une seule implémentation** du port sortant, `PisteCorpus: LegalCorpus` (`lf-adapter-piste`, M2). Le port `LegalCorpus` est **unique** (00 §3) :
   - consultations v0.1 ;
   - ajouts de M6 (`find_article`, `find_by_number`, `list_codes()`, chargée paresseusement, jamais au démarrage) et de M7 (`article_versions`, `related_links`) ; pas de méthode `suggest` (`/suggest` retiré, 00 §8).

   Il n'a **aucune méthode par défaut** : une implémentation incomplète ne compile pas (02 §5.1).
2. **M9 (v1.0.0) : `lf-adapter-dila`**, `DilaCorpus: LegalCorpus`, hors ligne :
   - ingestion en flux `tar` + `flate2`, puis `quick-xml`, puis un index `tantivy` avec analyseur français (00 §2) ;
   - **même** batterie de tests de contrat que PISTE, sur un petit dump fixture : contrat de citation v2, sentinelles, TOC ordonnée (02 §12.2).
3. **Choix de l'adapter à l'exécution** dans `lf-app`, seul crate qui voit les deux adapters :

   ```rust
   // non compilé, à valider en M0
   pub enum AnyCorpus { Piste(CachedCorpus<PisteCorpus>), Dila(DilaCorpus) }
   impl LegalCorpus for AnyCorpus { /* un match par méthode, sans Box<dyn> */ }
   ```

   - Le cache ne sert qu'à PISTE ([ADR-0007](0007-cache-memoire-port.md)).
   - Il n'y a ni `Box<dyn>` ni boxing de futur ([ADR-0012](0012-ports-async-rpitit.md)).
   - Feature `dila` de `lf-app`, désactivée par défaut.
   - Le **nom de la variable de sélection** et la valeur `env` de l'audit pour DILA ne sont **pas fixés avant M9**, faute de décision (02 §8.5).
4. **Opérations indisponibles hors ligne** : erreur explicite, déclarée comme lacune dans `fr_coverage` ; son code est tranché en M9 (00 §5). Les entités du domaine sont identiques quelle que soit la source.
5. **Fraîcheur et provenance** :
   - dès v0.1 (00 §12, adoptée par défaut, Q15) : `fr_coverage` porte « Source : Légifrance / DILA — Licence Ouverte 2.0 » et la date de consultation (date du jour, Europe/Paris, champ `consulted_on`) ; les instructions serveur, texte statique, portent la même mention et renvoient à `consulted_on` pour la date ; un champ `last_update?` par document n'est ajouté (classe « ajout ») que lorsqu'un champ fiable de l'API est identifié (V-108) ;
   - en M9 : la date du dernier incrément appliqué et la source (dump DILA ou API) figurent au moins dans `fr_coverage`. Leur présence dans chaque réponse, recommandée par la recherche 05 §6.7, sera tranchée en M9.
6. **Configuration par défaut** : `LF_PISTE_ENV=sandbox`, affichée dans `fr_coverage` (00 §1). La production demande une application PISTE dédiée (Q3).
   - Tests live et fixtures (00 §10, adoptée par défaut, Q3) : sandbox par défaut ; en production, uniquement des sondes en lecture seule lancées à la main par le mainteneur avant chaque étiquette (≤ 1 requête/s), et des fixtures de données publiques marquées `env = production` ; jamais d'identifiants de production en CI.
7. Judilibre et les autres sources (BOFiP, AN, Sénat) sont hors périmètre. Leur ajout passe par un nouvel ADR, derrière le même port ou un port distinct justifié.

## Conséquences

### Positives

- La valeur arrive dès M5 avec une seule source, et le port est façonné par des besoins réels (endpoints vérifiés dans le Swagger, 00 §8).
- Le passage hors ligne en M9 ne touche ni le domaine, ni l'application, ni l'adapter MCP : seuls un adapter et une variante d'enum s'ajoutent.
- `match` exhaustif : une méthode ajoutée au port sans être implémentée par DILA est une erreur de compilation.

### Négatives

- Jusqu'en M9, l'outil exige un compte PISTE, le réseau et le respect des quotas. La dépendance à un compte personnel est un risque (09).
- Un port taillé sur l'API PISTE (`/search`, `typePagination`, facettes) peut mal correspondre à un index local : le classement tantivy diffère de `PERTINENCE`. Des écarts de résultat entre sources sont inévitables et doivent être documentés.
- Ingestion lourde (plusieurs Go, millions de petits fichiers) et pas de nouveau global depuis 15 mois au 2026-10-15.

### À surveiller

- Points **NON VÉRIFIÉS** sur les dumps :
  - [v:dila-structure-archives] : arborescence et balises d'une archive réelle ;
  - [v:dila-volumetrie-archives] : volumes compressés ;
  - [v:dila-licence-par-fonds] : licence exacte de chaque fonds.
- Sur PISTE : [v:oauth-production-live], les URL de production n'ont jamais été appelées (V-59, recette de v0.1.0 prévue par Q3).
- Champ fiable de date de mise à jour par document : **NON VÉRIFIÉ** [v:date-maj-par-document] (V-108) ; forme exacte de l'attribution attendue : **NON VÉRIFIÉ** [v:attribution-licence-ouverte] (V-177).
- Q6 : ouverture réseau vers `echanges.dila.gouv.fr` avant M9.

### Tests qui la gardent

- Suite de contrat du port exécutée sur `FakeCorpus` et `PisteCorpus` dès M3 (06 §14), puis sur `DilaCorpus` en M9.
- `every_served_fond_has_a_family` et `coverage_lists_known_gaps` (D-34) ; `coverage_attribution_and_consultation_date` (Q15).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| DILA d'abord (hors ligne dès v0.1) | Ingestion lourde avant toute valeur. Parité impossible avec une référence en ligne. Global LEGI vieux de 15 mois. Hôte injoignable en développement (Q6). |
| Les deux sources en parallèle dès M2 | Double l'effort avant que le port soit stabilisé par M6 et M7. |
| `Box<dyn LegalCorpus>` | Allocation et boxing des futurs. Impose un trait objet-sûr, contraire au style RPITIT (ADR-0012, 02 §13). |
| Deux binaires (en ligne, hors ligne) | Double la distribution et la signature ([ADR-0013](0013-distribution.md)) pour un simple choix de configuration. |
| Un port par source ou par famille (`CitationResolver`…) | 00 §3 impose un port sortant unique. La résolution des citations est un service de l'application (annexe B §6.6). |
| Judilibre en v0.x | API et CGU distinctes, non lues (recherche 05 §8, points 12 et 16). Hors périmètre de la parité. |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §2, §3, §5, §8 à §13 (révision 2) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §3.3, §3.6, §3.15 (Q3, Q6, Q15) ; [02-architecture.md](../plan/02-architecture.md) §5.1, §8.5, §12.2, §13 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §1, §2 ; [annexes/C-api-legifrance.md](../plan/annexes/C-api-legifrance.md) (dumps DILA) ; [annexes/B-citations.md](../plan/annexes/B-citations.md) (résolution).
- Recherche : [05-piste-dila.md](../recherche/05-piste-dila.md) §1, §3, §4, §5, §6, §8 ; [01-parite.md](../recherche/01-parite.md) §0 ; [02-defauts.md](../recherche/02-defauts.md) D-18, D-34.
- Données ouvertes DILA : https://echanges.dila.gouv.fr/OPENDATA/ (injoignable pendant la recherche).
