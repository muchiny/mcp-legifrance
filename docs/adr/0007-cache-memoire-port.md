# ADR-0007 — Cache mémoire au niveau du port

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M3 (`lf-adapter-cache`), M9 (réévaluation d'un cache disque)
- **Questions liées** : —
- Révisé le 2026-10-15 (revue) : `singleDate` à midi Europe/Paris (00 §8, AM-02) ; `typePagination` mémorisée par l'adapter, hors de la clé (00 §7, AM-04).

## Contexte

- **La clé de cache de la référence ne sert jamais pour les recherches LODA et CODE** (D-01, E-06, recherche 02 §2.a). La clé est le corps JSON entier (`client.py:144-147`). Or ce corps contient `singleDate = _now_ms()`, recalculé à la milliseconde (`server.py:129-136`). Résultat : 0 % de hits sur les recherches de législation, et une écriture disque de 3 à 40 Ko jamais relue par recherche.
- **Autres défauts du cache de la référence** (D-15) :
  - la clé n'est namespacée ni par environnement ni par hôte : des réponses sandbox peuvent être resservies en production ;
  - les réponses 200 vides sont mises en cache 24 h ;
  - un échec d'écriture fait échouer l'outil ;
  - les valeurs sont désérialisées par **pickle** (exécution de code par quiconque écrit dans le répertoire) ;
  - les requêtes sont stockées en clair sur disque.
- **Coût d'accès** : le cache est rouvert à chaque appel (D-06), en SQLite synchrone dans la boucle async (D-38). diskcache a pour défaut 1 Gio et l'éviction *least-recently-stored* (recherche 01 §7).
- **CGU** : aucune clause des CGU de l'API Légifrance ne limite le cache ou le stockage local des réponses (recherche 05 §4.2).
- **moka 0.12.16**, lu au tag `v0.12.16` (02 §7.2) :
  - `try_get_with` ne met jamais en cache une erreur (`future/cache.rs:1194-1207`) ;
  - les appels concurrents sur une même clé sont coalescés ;
  - une `Expiry` peut être définie par entrée (`policy.rs:182`).

## Décision

1. **Décorateur du port** : `CachedCorpus<C: LegalCorpus>`, dans `lf-adapter-cache`, implémente `LegalCorpus` en déléguant à `C` (00 §2, §9). Composition `CachedCorpus<PisteCorpus>` dans `lf-app`. `Audited` reste au-dessus, au niveau du cas d'usage ([ADR-0008](0008-audit-decorateur-strict.md)).
2. **Mémoire uniquement** en v0.x (`moka`, feature `future`) : pas de cache disque, pas de pickle. Un cache disque optionnel sera réévalué en M9, par un nouvel ADR.
3. **Clé de domaine** : `CacheKey { env, op }`, où `op` est une variante typée par opération (`Search(SearchQuery)`, `Act(LegiTextId, VersionDate)`, `Article(ArticleId)`…, 02 §7.2).
   - Elle est construite à partir des types du domaine déjà normalisés, **jamais** à partir du JSON envoyé à PISTE.
   - `singleDate` est recalculé par l'adapter à partir de `VersionDate`, qui est au jour près (midi Europe/Paris du jour, 00 §8) : la clé est stable sur la journée de Paris ([ADR-0011](0011-dates-chrono-paris.md)).
   - Le texte de la requête n'est pas replié (ni casse, ni accents) : la clé reflète exactement ce qui part vers PISTE.
   - La `typePagination` n'entre pas dans la clé : elle est choisie par l'adapter (`PaginationMemo`, même TTL d'une heure, ou repli `ARTICLE` pour CODE en page > 1 ; 00 §7, 04 §12.2).
4. **TTL** : recherche 1 h ; consultations 24 h ; listes (codes) 7 jours (00 §9).
5. **Non mis en cache** : toutes les erreurs, les réponses vides (recherche sans hit, listes vides) et `fr_coverage`.
6. **Coalescence** des appels concurrents identiques (`entry(key).or_try_insert_with`).
7. **Capacité** bornée en octets (`weigher`). Proposition : 64 Mio, constante en v0.1 faute de variable dans 00 §1.
8. Sur un hit : ni jeton OAuth, ni attente du limiteur, ni réessai.

## Conséquences

### Positives

- **D-01 corrigé** : une seconde recherche LODA identique dans la journée ne sollicite plus PISTE. Le quota et la latence diminuent.
- **D-15 corrigé** : namespace par environnement ; aucun négatif durable ; plus aucune désérialisation de données non fiables ; aucune requête sur disque.
- L'adapter PISTE ne connaît pas le cache, et le cache ne connaît pas PISTE : un adapter DILA sans cache se branche sans changement ([ADR-0004](0004-piste-puis-dila.md)).

### Négatives

- **Cache perdu au redémarrage** (E-30) : le premier appel est plus lent après un redémarrage. Claude Code peut démarrer le serveur deux fois par session (annexe D §4.5).
- Pas de partage entre processus : Claude Code et VS Code ont chacun leur cache.
- **Fraîcheur** : une consultation peut être servie jusqu'à 24 h après une modification en amont.
  - Pour `get_act`, la `VersionDate` fait partie de la clé.
  - Pour un article lu par identifiant, la version servie peut avoir jusqu'à 24 h de retard. C'est acceptable pour des textes consolidés, mais à signaler dans la documentation.

### À surveiller

- **NON VÉRIFIÉ** [v:moka-horloge-tests] : moka n'utilise pas l'horloge tokio, si bien que tester les TTL sans attente réelle n'est pas acquis. À défaut, on teste directement la fonction `Expiry` (02 §3.4).
- **NON VÉRIFIÉ** [v:singledate-minuit-paris] : avec `singleDate` à midi Europe/Paris (00 §8 révision 2 ; minuit dans la première version du plan, d'où le nom de l'étiquette), un texte qui entre en vigueur le jour J est-il retenu le jour J ? Vérification bloquante en M2 (V-76). La clé stable sur la journée n'en dépend pas : elle repose sur `VersionDate`.
- Une page > 1 mise en cache a pu être obtenue avec la valeur de repli de `typePagination` plutôt qu'avec la valeur mémorisée ; pour CODE, seul cas observé, les deux coïncident. **NON VÉRIFIÉ** [v:typepagination-page2] (V-82, M5).
- La taille réelle des réponses, pour calibrer la capacité : **NON VÉRIFIÉ** [v:taille-max-textes].

### Tests qui la gardent

- `second_identical_loda_search_hits_cache` (wiremock `expect(1)`).
- `concurrent_identical_calls_coalesce`.
- `errors_and_empty_results_are_not_cached`.
- `cache_key_changes_when_paris_day_changes`.
- `cache_namespaced_by_env` (02 §7.2, 06 §2.4, §3.1).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Cache HTTP indexé par le corps JSON (référence) | Une clé volatile reproduit D-01. Le cache serait couplé au format de PISTE. |
| Cache disque `diskcache` ou pickle | D-15 (exécution de code, requêtes en clair) et D-38 (E/S synchrones). |
| Port `CacheStore` dans l'application, en mémoire (moka) et sur disque (`redb`) (recherche 02 D-15) | Un port de plus pour un besoin couvert par un décorateur. Le disque n'est pas requis en v0.x (00 §9). |
| Négatifs en cache avec un TTL court (recherche 02 D-15) | 00 §9 exclut erreurs et réponses vides. Un « introuvable » transitoire ne doit pas persister. |
| Cache interne à `PisteCorpus` | Mélange résilience HTTP et politique de cache. Inutilisable pour d'autres adapters. Ne se teste pas isolément. |
| Clé SHA-256 de la requête canonique (recherche 02 D-15) | Inutile en mémoire : les types du domaine sont `Hash + Eq` et plus lisibles en diagnostic. |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §2, §7, §8, §9 ; [02-architecture.md](../plan/02-architecture.md) §7.2, §8.4, §13 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §6.3, §6.6, §12 ; [06-qualite.md](../plan/06-qualite.md) §2.4 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §4.9 (clé `memory_cache`).
- Recherche : [02-defauts.md](../recherche/02-defauts.md) §2.a, D-01, D-06, D-15, D-38 ; [01-parite.md](../recherche/01-parite.md) §7, E-06, E-30 ; [05-piste-dila.md](../recherche/05-piste-dila.md) §4.2.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/server.py:129-136` ; `src/fr_eli_mcp/client.py:144-154, 169-171` ; `src/fr_eli_mcp/cache.py:15-57`.
- moka : https://github.com/moka-rs/moka (tag `v0.12.16`, `a616ec1`).
