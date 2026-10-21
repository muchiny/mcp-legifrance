# ADR-0009 — Contrat de citation v2 et corrections typographiques

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : tâche de tête de M2 (`lf-domain::contract`, livrée : plan 2a, tâches 5, 7, 9 et 16), M2 (mapping), M4 (schémas de sortie), M6 (vérification des citations)
- **Questions liées** : Q12 (formes de citation complémentaires ; proposition adoptée par défaut, à confirmer en M2, avant T-M2-10 (génération des golden)) ; Q16 (typographie des citations, décidée le 2026-10-15) ; Q17 (URL d'un article de loi, à vérifier en M2)
- Révisé le 2026-10-15 (revue) : formes complémentaires adoptées par défaut (00 §4, AM-19, Q12) ; URL d'un `LEGIARTI` et vérifications bloquantes pour v0.1.0 (00 §4, AM-18).

> **Statut d'implémentation (décision C2, 2026-10-16 ; livré le 2026-10-21, plan 2a).** Le format v2 est **implémenté** dans `lf-domain`, avant tout mapper (00 §11.1, point 1). Le module `citation` est devenu `contract` et `Citable::citation()` est devenu `Citable::contract()` (plan 2a, tâche 5), ce qui libère `citation` pour le parseur M6. Les newtypes `CitationText` (`contract/text.rs` : une ligne, jamais vide, au plus `CITATION_MAX_CHARS` = 1 000 caractères avec `…`), `Eli` (`contract/eli.rs` : ELI natif validé, jamais fabriqué) et `SourceUrl` (`url.rs`, construite par `legifrance_url` seule, ré-exportée par `contract`) sont nés en tâche 7. La struct `CitationContract { citation, source_url, ecli, eli }` (`crates/lf-domain/src/contract/mod.rs:40`), `from_url(source_url, citation, id)` avec le repli `Légifrance {ID}`, le texte d'`ELI_NOTE`, les sept `impl Citable`, `ecli` propagé depuis `Decision.ecli`, `constit_citation` sans paramètre `title` et la suppression de `bracketed` datent de la tâche 9 ; `from_url` est réservé au crate (`pub(crate)`, tâche 16). Le contrat v1 de M1 (`eli_uri`, `human_readable_citation: Option`, suffixe « - objet ») n'existe plus. Aucun mapper, aucune vue ni golden ne consomme encore le contrat : la règle `/loda/` de la décision 6 attend V-140 et V-142, et l'écart d'un hit incomplet (D-30) revient au mapper de M2.

## Contexte

- **Le contrat n'était pas garanti.** La CONSTITUTION de la référence exige un contrat de citation sur chaque réponse (`CONSTITUTION.md:37-46`). Pourtant tous ses champs sont optionnels (`models.py:51-53`…), et le JSON Schema publié le déclare (`source_url: anyOf[string, null]`, `tests/tools.lock.json`). Un hit LODA sans `id` sort avec `source_url = None` (D-30).
- **`eli_uri` est trompeur.** Il vaut toujours `source_url` (`citations.py:170-171`) : ce n'est pas un ELI. Les notes `eli_note` et `dataset_note` (171 + 335 caractères) sont répétées dans chaque réponse (D-30, D-33). Aujourd'hui, l'API renvoie `eli`/`idEli` à `null` (fixtures `consult_loda.json`, `consult_article.json`) ; c'est **NON VÉRIFIÉ** en production [V-102 · eli-natif-null].
- **La typographie est fautive** (D-27, E-12) :
  - « Cons. const., decision n° … » et « CNIL, deliberation n° … », sans accents (`citations.py:309`, `:378`), figés par `test_parse.py:116` et `:198` ;
  - mois `fevrier`, `aout`, `decembre`, et « 1 » au lieu de « 1er » (`citations.py:283-295`) ;
  - suffixe « - objet » extrait par une regex sur le **premier** crochet (`citations.py:315`), fausse avec des crochets multiples ou imbriqués.
- **L'URL n'est pas choisie par le bon critère.**
  - Le type d'URL d'une décision dépend du champ `origine` et non du préfixe déjà validé (`citations.py:333-334` ; D-28, E-18).
  - Tout `LEGIARTI` reçoit `/codes/article_lc/`, y compris un article de loi, alors que `/loda/article_lc/` existe (D-29, E-19).
- **Les ECLI natifs sont présents dans les fixtures** : `ECLI:FR:CC:2025:2025.1173.QPC` (`consult_juri_constit.json`), `ECLI:FR:CECHR:2026:506507.20260529` (`consult_juri_cetat.json`).

## Décision

1. **Toute entité citable expose** (00 §4, 03 §5.1) :
   - `citation` : chaîne au format français correct, non vide, sur une ligne, au plus 1 000 caractères. Repli ultime : `Légifrance {ID}` ;
   - `source_url` : URL `https://www.legifrance.gouv.fr/…` construite depuis l'identifiant **typé**, jamais nulle.

   Les deux champs sont **requis** dans chaque `outputSchema`.
2. **`ecli`** n'apparaît que s'il est natif et valide (`ECLI:FR:…`). Il n'est **jamais fabriqué** ; les règles de dérivation ne servent qu'au contrôle des citations en M6.
3. **`eli`** n'apparaît que si l'API renvoie un ELI natif. Il n'est jamais construit depuis le NOR ou la date ; aujourd'hui, il est toujours absent. La note ELI figure **une seule fois**, dans les instructions et dans `fr_coverage`.
4. **Renommages** : `eli_uri` n'est pas reproduit ; `human_readable_citation` devient `citation`.
5. **Formats** (divergences volontaires de la parité) :

   | Famille | Format |
   |---|---|
   | Conseil constitutionnel | « Cons. const., décision n° 2025-1173 QPC du 7 novembre 2025 » ; l'objet entre crochets reste lisible dans `title` |
   | CNIL | « CNIL, délibération n° SAN-2026-002 du 8 janvier 2026 » |
   | Dates en prose | mois accentués et « 1er » (`CivilDate::to_french`, livré, `date.rs:287` ; ex-Q9 livrée, devenue Q16 dans 00 §13) |
   | Article de code | « Code civil, art. 9 », **inchangé** |
   | JURI, CETAT | titre nettoyé (parité) |
   | Signe numéro | `n°` suivi d'une espace U+0020 |

   Formes complémentaires (00 §4, adoptées par défaut, Q12 ; décidées en M2, avant T-M2-10 (génération des golden)) :

   | Cas | Forme |
   |---|---|
   | Article d'un texte non codifié | « {titre officiel du texte, sans appel de note final « (n) »}, art. {num} », `num` verbatim (aucune réécriture, par exemple de « 1 » en « 1er ») ; la citation du texte lui-même (`fr_get_act`, hit LODA) garde l'appel de note (annexe A, clé `law_article_citation`) ; la forme courte « Loi n° 2016-1321 du 7 octobre 2016, art. 1 » reste réexaminée en M2 sur les titres réels |
   | Convention KALI | titre nettoyé, sans IDCC (l'IDCC est un champ) |
   | Hits de recherche CONSTIT et CNIL | titre officiel (parité) ; la règle de consultation (lignes ci-dessus) s'applique aux consultations ; réexamen en v0.2 avec le parseur de M6 |
   | Conseil constitutionnel | sans l'objet entre crochets, qui reste lisible dans `title` |

6. **URL et règle de citation viennent du préfixe validé** (`DecisionId`, `ArticleId`), jamais de `origine`, qui ne sert qu'au contrôle de cohérence. Un `LEGIARTI` dont le texte parent est un code reçoit `/codes/article_lc/{id}` ; dont le texte parent n'est pas codifié (LOI, ORDONNANCE, DECRET, ARRETE…), `/loda/article_lc/{id}` ; une nature de parent inconnue retombe sur `/codes/article_lc/{id}`, repli de parité (00 §4, 03 §5.5). URL non datée en v0.1. Divergence volontaire E-19/D-29 (scénario smoke 3 transposé) ; bloquant pour v0.1.0 : V-140 (forme canonique) et V-142 (`textTitles[].nature`). **Cette règle `/loda/` (E-19) ne s'applique qu'après les vérifications live V-140 et V-142 en M2** ; d'ici là, le livré garde `/codes/article_lc/` comme la référence (Q17, ex-Q10 livrée, V-141 [V-141 · url-codes-article-lc-loi]).
7. **Les données officielles des autres champs restent verbatim** après `clean` : `num`, `title`, `formation`. Les ordinaux comme « 2ème » ne sont pas corrigés.
8. Les divergences portent les clés de l'annexe A §4 : `citation_renamed`, `eli_uri_dropped`, `notes_dropped`, `citation_typography`, `citation_never_empty`, `law_article_url`, `law_article_citation`, `decision_routing_by_prefix`.

## Conséquences

### Positives

- Une réponse sans URL ni citation est **impossible à construire** : le type `CitationContract` v2 est non optionnel (garanti par `CitationText`, pas par la vue). Un hit incomplet est écarté et compté (D-30).
- Citations conformes à l'usage français, attendues par des juristes.
- Aucune information trompeuse : plus de faux ELI, et plus d'URL de décision choisie par un champ libre.
- Moins de tokens par réponse ([ADR-0006](0006-sortie-compacte-retour-progressif.md)).

### Négatives

- **Rupture de parité.** Deux assertions de la référence (`test_parse.py:116`, `:198`) et les golden correspondants sont transposés. Un client qui compare des chaînes de citation ou lit `eli_uri` doit être adapté (annexe A §6).
- Une citation calculée par une règle peut différer du titre officiel. Le `title` verbatim reste donc rendu quand la citation ne le contient pas (Conseil constitutionnel).

### À surveiller

- URL jamais ouvertes dans un navigateur : **NON VÉRIFIÉ** [V-139 · url-patterns-navigateur] ; `/loda/article_lc/` canonique [V-140 · url-loda-article-lc] (V-140, bloquante pour v0.1.0) ; valeur de `textTitles[].nature` pour un article de loi [V-142 · texttitles-nature-loi] (V-142, bloquante pour v0.1.0) ; `/codes/article_lc/` pour un article de loi, assertion de la référence [V-141 · url-codes-article-lc-loi] (V-141) ; `/ceta/id/` ou `/juri/id/` [V-143 · url-ceta-vs-juri]. Une recette est prévue avant v0.1.0.
- Libellés des natures rares du Conseil constitutionnel : **NON VÉRIFIÉ** [V-114 · natures-constit]. Format des décisions CNIL : **NON VÉRIFIÉ** [V-115 · cnil-nature-decision-format].
- Les formes complémentaires de la décision 5 sont adoptées par défaut (Q12, 09 §3.12) : la réponse de l'utilisateur est attendue en M2, avant T-M2-10 (génération des golden), et la citation d'un article de loi et la citation KALI se revoient alors sur les titres réels. Une réponse contraire produit un nouvel ADR (index, §2). Les hits CONSTIT et CNIL sont réexaminés en v0.2 avec le parseur de M6.

### Tests qui la gardent

- `constit_citation_golden`, `cnil_citation_golden`, `format_fr_twelve_months_and_premier` (à rapprocher des tests livrés sur `CivilDate::to_french`).
- `article_citation_rules_table`, `citation_never_empty`, `law_article_url_uses_loda_article_lc`, `url_depends_on_id_type_not_payload_origine`.
- `no_view_has_eli_uri`, `ported_no_fabricated_eli`, `ecli_is_never_fabricated` (P-031, P-034, P-035, P-039).
- Livrés en tête de M2 (plan 2a, tâche 9) : `constit_citation_golden`, `cnil_citation_golden`, `article_citation_rules_table` (`contract/rules.rs`), `citation_never_empty` (`entity/mod.rs`), `ecli_is_never_fabricated` (`entity/decision.rs`) ; `format_fr_twelve_months_and_premier`, `law_article_url_uses_loda_article_lc`, `url_depends_on_id_type_not_payload_origine`, `no_view_has_eli_uri` et `ported_no_fabricated_eli` arrivent avec le mapper (M2) et les vues (M4).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Garder la typographie de la référence (E-12, parité stricte) | Fige des fautes dans le livrable principal d'un outil juridique (D-27). |
| Garder `eli_uri` égal à `source_url` | Doublon et nom trompeur (D-30). |
| Fabriquer l'ELI depuis le NOR et la date (`/eli/loi/AAAA/M/J/NOR/jo/texte`) | Interdit par la règle « jamais fabriqué » (00 §4, P-039). Risque de lien faux. |
| Garder le suffixe « - objet » avec une analyse équilibrée des crochets (recherche 02 D-27) | Absent du format normatif de 00 §4 ; l'objet reste disponible dans `title`. |
| Forme courte « Loi n° 2016-1321 du 7 octobre 2016, art. 1 » pour un article de loi non codifiée ; IDCC dans la citation KALI (« … (IDCC 2121) ») (première version de 03 §11) | Réécrivent les données officielles au-delà du nécessaire ; 00 §4 retient le titre officiel sans l'appel de note, et l'IDCC comme champ (Q12). |
| Router par `origine` avec repli sur le préfixe (recherche 01 E-18) | Le préfixe est déjà validé et fiable ; `origine` peut être nul ou bruité (`"constit "`). |
| Champ `official_title` obligatoire dans le contrat (recherche 02 D-30) | Doublon de `title` dans la plupart des familles ([ADR-0006](0006-sortie-compacte-retour-progressif.md)). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §4 (révision 2, AM-18, AM-19), §7, §13 (Q12) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §3.12 ; [03-domaine.md](../plan/03-domaine.md) §5 (champs, ECLI, ELI, règles, URL, divergences 1 à 25), §6.5 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §1.4 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §2.1 (P-031, P-034, P-035, P-039), §4.1, §4.2 ; [annexes/B-citations.md](../plan/annexes/B-citations.md).
- Recherche : [02-defauts.md](../recherche/02-defauts.md) D-27 à D-30, D-33 ; [01-parite.md](../recherche/01-parite.md) §4.5, §11.4, E-12, E-18, E-19 ; [07-citations.md](../recherche/07-citations.md) §4.10, §4.13, §6.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/citations.py:76-102, 166-171, 256-261, 283-318, 333-338, 376-380` ; `tests/test_parse.py:106-118, 189-199` ; `CONSTITUTION.md:37-46`.
