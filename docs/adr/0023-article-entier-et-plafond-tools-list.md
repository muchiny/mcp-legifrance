# ADR-0023 — Article entier jusqu'à 2 000 caractères en mode résumé et plafond de `tools/list` de la 1.1.0

- **Statut** : **Proposé** (accepté à la fusion de la PR 2 de la 1.1.0 ; Q20 = B et Q27 = A sont des décisions du propriétaire, le recalcul du plafond selon la règle d'AM-63 est de l'orchestrateur de la revue finale et le propriétaire le confirmera à la PR)
- **Date** : 2026-11-02
- **Jalons concernés** : version 1.1.0 (tâche D-01 du plan de correction, constat decisions-4 ; tâche D-12 pour les descriptions)
- **Questions liées** : Q20 (décidée : B, par le propriétaire), Q27 (décidée : A, par le propriétaire)
- Amendements : AM-72 et AM-73 (00 §14), proposés jusqu'à la fusion. Amende 03 §7.9, 05 §5.3 et §6.6, 06 §12.2 et AM-63.

## Contexte

Deux règles de la 1.1.0 modifient des valeurs posées en 00, ce qui exige un ADR (06 §13.2).

1. **Seuil du résumé d'un article (D-01).** En mode résumé, `fr_get_text` rendait entier un texte de `min(max_chars, 1 200)` caractères au plus ; au-delà, il rendait le début du texte. Le constat decisions-4 relève l'article 757 B du CGI (1 307 caractères) : il tombait juste au-dessus du seuil et revenait donc en résumé (début du texte). Le seuil de 1 200 caractères est fixé par 00 §4 et 03 §7.9, qui prévoient qu'une mesure puisse motiver un amendement.
2. **Plafond de `tools/list` (Q27).** AM-63 a posé la règle du plafond (mesure plus 3 %, arrondie à la centaine supérieure) à 32 900 octets pour la mesure de 31 853 octets de 1.0.0. Les descriptions de D-12 (glossaire de `etat`, mise en garde sur `verified`, `date`, `match`, `in_force_only`…) alourdissent `tools/list`. Q27 = A a relevé le plafond à 34 500 octets pour la mesure de la vague V3 (34 229, puis 34 482 après ses remarques). Les corrections de la revue finale de la PR 2 (glossaire de `etat` sans joker, phrase sur `DENONCE`, reformulation de la description de `fr_verify_citations`) ont porté la mesure à 35 395 octets, puis à 35 424 octets au second tour : 34 500 aurait laissé 18 octets de marge, puis aucune.

## Décision

### Article entier jusqu'à 2 000 caractères (Q20 = B, AM-72)

- En mode résumé, `fr_get_text` rend entier (`text`, sans `summary`) un article (LEGIARTI ou KALIARTI) de `min(max_chars, 2 000)` caractères au plus, au lieu de `min(max_chars, 1 200)`. Le seuil de 1 200 caractères reste celui des décisions et des délibérations ; le plafond du résumé d'un texte plus long reste 1 200 pour les trois outils.
- `summary_truncated` (champ optionnel, absent sauf si vrai) est vrai quand le résumé lui-même est coupé ou n'est que le début d'un texte plus long ; il est absent pour un sommaire officiel ou un dispositif donné entier.
- Code : `lf-application/src/budget/reading.rs` (`ReadingRequest::for_article`, `ARTICLE_WHOLE_CHARS`) et `usecase/get_text.rs` ; la place que la lecture peut encore prendre sous la cible du budget compte les nouveaux champs (`room_for_reading`).

### Plafond de `tools/list` de la 1.1.0 (Q27 = A, AM-73, règle d'AM-63 appliquée)

- La règle d'AM-63 s'applique à la mesure **finale** : mesure 35 424 octets, plus 3 % (36 486,72), arrondie à la centaine supérieure : **36 500** octets pour v0.3 (v0.1 : 20 000 et v0.2 : 24 000 inchangés), soit 1 076 octets de marge. La valeur est la même qu'au premier tour de la revue finale (35 395 octets donnaient 36 456,85, donc 36 500).
- Même valeur dans `descriptor_size_budget` (`crates/lf-adapter-mcp/tests/budgets.rs`) et dans `TOOLS_LIST_BUDGETS` de `xtask` (`lock-descriptors`).
- **Origine des chiffres.** Q27 = A (34 500) est une décision du propriétaire. Le passage à 36 500 est un **recalcul de l'orchestrateur** selon la règle d'AM-63, non une décision du propriétaire : le propriétaire le confirmera à la PR. Il est à recalculer si la mesure change (mesure plus 3 %, arrondie à la centaine supérieure) aux deux endroits et dans 05 §6.6, 06 §12.2, AM-73 et le CHANGELOG.
- La limite de 3 500 caractères des instructions ne bouge pas (3 499 mesurés) : une règle courte y figure, la règle complète est dans la description du champ `etat` de `fr_get_text`.

## Conséquences

- Un article jusqu'à 2 000 caractères se lit en un appel, sans `detail="full"` ; une réponse de `fr_get_text` pèse jusqu'à 800 caractères de plus qu'avant en mode résumé, sans effet sur le plafond de réponse (la lecture est de toute façon bornée par la cible du budget).
- Le coût fixe d'une session passe de 30 634 octets (1.0.1) à 35 424 octets pour les descriptions de D-12 (glossaire de `etat`, `verified` ne dit pas que l'article dit ce que le brouillon prétend). Un glossaire complet dans les cinq champs aurait donné 36 484 octets, sous le plafond de 36 500 mais avec 16 octets de marge : la version courte est conservée.
- Une nouvelle hausse de la mesure ne passe pas par un nouveau plafond posé à la main : elle suit la règle d'AM-63 et se consigne dans AM-73 et 05 §6.6.

## Alternatives écartées

| Alternative | Raison |
|---|---|
| Garder 1 200 caractères | Laisse l'article 757 B du CGI (1 307 caractères) résumé par son début ; décision du propriétaire (Q20 = B). |
| Plafond de 34 500 (Q27 = A) maintenu | Posé avant les corrections de la revue finale ; inférieur à la mesure finale. |
| Retirer des descriptions pour tenir 34 500 | Perd le glossaire de `etat` sans joker et les mises en garde de D-12. |

## Références et sources

- [03-domaine.md](../plan/03-domaine.md) §7.9 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.3 et §6.6 ; [06-qualite.md](../plan/06-qualite.md) §12.2 ; [00-decisions.md](../plan/00-decisions.md) §14 (AM-63, AM-72, AM-73).
- Code : `crates/lf-application/src/budget/reading.rs`, `crates/lf-application/src/usecase/get_text.rs`, `crates/lf-adapter-mcp/tests/budgets.rs`, `xtask/src/lock_descriptors/checks.rs`.
- [ADR-0022](0022-dispositif-cnil-et-seuil-de-similarite.md) pour les autres décisions de la même PR (AM-74 et AM-75).
