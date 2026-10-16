# ADR-0009 — Contrat de citation v2 et corrections typographiques

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : tâche de tête de M2 (`lf-domain::contract`, 00 §11.1), M2 (mapping), M4 (schémas de sortie), M6 (vérification des citations)
- **Questions liées** : —

> **Statut d'implémentation (décision C2, 2026-10-16).** Le format v2 est **décidé** et s'applique ; il est implémenté en **tâche de tête de M2** (00 §11.1), avant tout mapper. M1 a livré un `CitationContract` **v1** de parité (`crates/lf-domain/src/citation.rs:16` : `eli_uri`, `human_readable_citation: Option`, `source_url` ; `Citable::citation()` ; suffixe « - objet » via `bracketed`). Le passage à v2 est interne à `lf-domain` (aucun mapper, aucune vue ni golden ne consomme encore le contrat) : struct, `from_url`, `ELI_NOTE`, nouveaux newtypes `CitationText`, `SourceUrl` et `Eli`, les 7 `impl Citable`, `constit_citation` sans paramètre `title`, suppression de `bracketed`, renommage du module `citation` en `contract` (libère `citation` pour le parseur M6) et de `Citable::citation()` en `Citable::contract()`.

## Contexte

- **Le contrat n'était pas garanti.** La CONSTITUTION de la référence exige un contrat de citation sur chaque réponse (`CONSTITUTION.md:37-46`). Pourtant tous ses champs sont optionnels (`models.py:51-53`…), et le JSON Schema publié le déclare (`source_url: anyOf[string, null]`, `tests/tools.lock.json`). Un hit LODA sans `id` sort avec `source_url = None` (D-30).
- **`eli_uri` est trompeur.** Il vaut toujours `source_url` (`citations.py:170-171`) : ce n'est pas un ELI. Les notes `eli_note` et `dataset_note` (171 + 335 caractères) sont répétées dans chaque réponse (D-30, D-33). Aujourd'hui, l'API renvoie `eli`/`idEli` à `null` (fixtures `consult_loda.json`, `consult_article.json`) ; c'est **NON VÉRIFIÉ** en production [v:eli-natif-null].
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
   | Dates en prose | mois accentués et « 1er » (`CivilDate::to_french`, livré, `date.rs:115` ; ex-Q9 livrée, devenue Q11 dans 00 §13) |
   | Article de code | « Code civil, art. 9 », **inchangé** |
   | JURI, CETAT | titre nettoyé (parité) |
   | Signe numéro | `n°` suivi d'une espace U+0020 |

6. **URL et règle de citation viennent du préfixe validé** (`DecisionId`, `ArticleId`), jamais de `origine`, qui ne sert qu'au contrôle de cohérence. Un `LEGIARTI` dont le texte parent n'est pas un code reçoit `/loda/article_lc/` ; une nature de parent inconnue retombe sur `/codes/article_lc/` (03 §5.5). **Cette règle `/loda/` (E-19) ne s'applique qu'après les vérifications live V-140 et V-142 en M2** ; d'ici là, le livré garde `/codes/article_lc/` comme la référence (Q12, ex-Q10 livrée, V-141 [v:url-codes-article-lc-loi]).
7. **Les données officielles des autres champs restent verbatim** après `clean` : `num`, `title`, `formation`. Les ordinaux comme « 2ème » ne sont pas corrigés.
8. Les divergences portent les clés de l'annexe A §4 : `citation_renamed`, `eli_uri_dropped`, `notes_dropped`, `citation_typography`, `citation_never_empty`, `law_article_url`, `decision_routing_by_prefix`.

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

- URL jamais ouvertes dans un navigateur : **NON VÉRIFIÉ** [v:url-patterns-navigateur] ; `/loda/article_lc/` canonique [v:url-loda-article-lc] ; `/ceta/id/` ou `/juri/id/` [v:url-ceta-vs-juri]. Une recette est prévue avant v0.1.0.
- Libellés des natures rares du Conseil constitutionnel : **NON VÉRIFIÉ** [v:natures-constit]. Format des décisions CNIL : **NON VÉRIFIÉ** [v:cnil-nature-decision-format].
- Deux formats, alignés sur 03 §5.4 et 05 :
  - article d'un texte non codifié : citation avec le **titre complet du parent** jusqu'à V-140 et V-142 ; la forme courte « Loi n° 2016-1321 du 7 octobre 2016, art. 1 » est réexaminée en M2 sur les titres réels ;
  - citation KALI **sans IDCC** (« … (IDCC 2121) » écarté).

  Les deux se revoient en M2 sur les titres réels, avant les golden.

### Tests qui la gardent

- `constit_citation_golden`, `cnil_citation_golden`, `format_fr_twelve_months_and_premier` (à rapprocher des tests livrés sur `CivilDate::to_french`).
- `article_citation_rules_table`, `citation_never_empty`, `law_article_url_uses_loda_article_lc`, `url_depends_on_id_type_not_payload_origine`.
- `no_view_has_eli_uri`, `ported_no_fabricated_eli`, `ecli_is_never_fabricated` (P-031, P-034, P-035, P-039).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Garder la typographie de la référence (E-12, parité stricte) | Fige des fautes dans le livrable principal d'un outil juridique (D-27). |
| Garder `eli_uri` égal à `source_url` | Doublon et nom trompeur (D-30). |
| Fabriquer l'ELI depuis le NOR et la date (`/eli/loi/AAAA/M/J/NOR/jo/texte`) | Interdit par la règle « jamais fabriqué » (00 §4, P-039). Risque de lien faux. |
| Garder le suffixe « - objet » avec une analyse équilibrée des crochets (recherche 02 D-27) | Absent du format normatif de 00 §4 ; l'objet reste disponible dans `title`. |
| Router par `origine` avec repli sur le préfixe (recherche 01 E-18) | Le préfixe est déjà validé et fiable ; `origine` peut être nul ou bruité (`"constit "`). |
| Champ `official_title` obligatoire dans le contrat (recherche 02 D-30) | Doublon de `title` dans la plupart des familles ([ADR-0006](0006-sortie-compacte-retour-progressif.md)). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §4, §7 ; [03-domaine.md](../plan/03-domaine.md) §5 (champs, ECLI, ELI, règles, URL, divergences 1 à 25), §6.5 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §1.4 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §2.1 (P-031, P-034, P-035, P-039), §4.1, §4.2 ; [annexes/B-citations.md](../plan/annexes/B-citations.md).
- Recherche : [02-defauts.md](../recherche/02-defauts.md) D-27 à D-30, D-33 ; [01-parite.md](../recherche/01-parite.md) §4.5, §11.4, E-12, E-18, E-19 ; [07-citations.md](../recherche/07-citations.md) §4.10, §4.13, §6.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/citations.py:76-102, 166-171, 256-261, 283-318, 333-338, 376-380` ; `tests/test_parse.py:106-118, 189-199` ; `CONSTITUTION.md:37-46`.
