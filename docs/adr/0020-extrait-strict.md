# ADR-0020 — Règle d'extrait stricte pour `fr_verify_citations`

- **Statut** : **Accepté** (2026-11-01, décision du propriétaire, question Q5)
- **Date** : 2026-11-01
- **Jalons concernés** : version 1.0.1 (tâche L2-03 du plan de correction)
- **Questions liées** : Q5 (règle d'extrait stricte)
- Remplace le seuil de similarité V-162 (annexe B §7.9). Amendement : AM-67 (00 §14).

## Contexte

Jusqu'à 1.0.0, un extrait entre guillemets jouxtant une citation était tenu pour fidèle (`quote: "match"`) s'il figurait tel quel dans le texte officiel, ou si 90 % de ses paires de mots consécutifs y figuraient n'importe où. Le seuil était marqué **NON VÉRIFIÉ** (V-162). L'E2E du 2026-11-01 (défaut verify_citations-1, rejoué en direct) a montré qu'un extrait de l'article L. 1234-9 du code du travail où « 8 mois » devient « 12 mois » reste `match`, de même que « un préjudice » pour « un dommage » à l'article 1240 du code civil : une altération d'un seul mot, chiffre ou négation compris, passe dès que l'extrait est assez long. Pour un outil dont le rôle est de repérer les citations falsifiées ou inventées, c'est une sortie trompeuse.

## Décision

`check_quote` (`crates/lf-application/src/citation/policy.rs`) applique une égalité stricte des mots.

1. L'extrait est découpé aux ellipses (`…`, `...`, `[…]`, `[...]`, `(…)`, `(...)`) avant tout repli. Les tronçons sans mot disparaissent ; si aucun ne reste, l'extrait est un écart (`quote_mismatch`).
2. Chaque tronçon est replié (minuscules, accents et ligatures, ponctuation, apostrophes droites ou typographiques, espaces dont les insécables) et doit être une suite contiguë de mots du texte officiel replié. Les tronçons se suivent dans l'ordre du texte, sans réemploi.
3. Sont tolérés, des deux côtés : les repères d'énumération (« 1° », « 4° bis », « a) », « (2) », « I.- », « II. – », « 1. ») en tête d'alinéa ou après « ; », « : », « . », et, dans l'extrait seulement, les repères de forme sûre (« 1° », « a) ») au milieu d'une phrase ; la forme du numéro d'article (« L. 1234-9 » = « L1234-9 », lettres L, R, D, A).
4. Restent des mots, donc des écarts s'ils diffèrent : les chiffres, les nombres (« 12 109 » n'est pas « 12109 »), les symboles € % ‰ $ £, un renvoi en milieu de phrase (« au 3° du I »), un ajout éditorial entre crochets (« [sic] », « [le salarié] »).
5. Un extrait à ellipses compte au moins trois mots sur ses tronçons ; en deçà, des tronçons d'un ou deux mots sont vrais presque partout. Le minimum s'applique dès que l'extrait porte une ellipse, y compris en bordure (« […] le […] ») ; un extrait sans ellipse n'a pas de minimum.
6. Le champ `quote` garde ses deux valeurs (`match`, `quote_mismatch`) ; aucun champ n'est ajouté en 1.0.1. Un champ `quote_diff` est envisagé pour 1.1.0 (Q17/Q26).

## Critère d'acceptation

Zéro faux écart sur un jeu d'au moins 20 extraits fidèles réels. Le jeu est un test unitaire (`faithful_real_quotes_never_raise_a_false_mismatch`, 38 extraits tirés des articles L. 1234-9, L. 1233-3, L. 1237-19-1, L. 221-18, L. 3121-27, 9, 544, 1103, 1231-1, 1240 et du CGI 150 U, et d'une décision des fixtures), avec son jumeau d'altérations (`altered_real_quotes_are_mismatches`).

## Conséquences

- Plus aucun `match` sur un chiffre, une négation ou un mot changé.
- Des citations approximatives mais honnêtes (paraphrase légère, mot omis sans ellipse) donnent désormais `quote_mismatch` : mieux vaut une alerte qu'un faux `match`.
- Faiblesses acceptées : un tronçon d'un seul mot près d'une ellipse est vrai presque partout (« Chacun […] sa vie ») ; un repère d'énumération changé (« 1° » pour « 2° ») ou omis en tête d'alinéa n'est pas un écart ; un extrait de moins de trois mots avec ellipse est toujours un écart.
- Le coût reste borné : recherche en O(mots du texte × mots de l'extrait) au pire, états mémorisés (pas de retour arrière exponentiel face à une suite hostile de repères).

## Alternatives écartées

| Alternative | Raison |
|---|---|
| Garder le seuil en le relevant | Un mot altéré reste invisible sur les longs extraits. |
| Diff mot à mot renvoyé à l'appelant (`quote_diff`) | Ajout de surface, reporté à 1.1.0. |
| Retirer tous les repères d'énumération partout | Cacherait un changement de « 3° » en « 2° » dans un renvoi. |

## Références et sources

- [00-decisions.md](../plan/00-decisions.md) §7, §14 (AM-67) ; [B-citations.md](../plan/annexes/B-citations.md) §7.9 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.9.
- Code : `crates/lf-application/src/citation/policy.rs`.
