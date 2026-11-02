# ADR-0022 — Règle de dispositif de la CNIL et seuil de similarité de `fr_diff_article`

- **Statut** : **Accepté** (2026-11-02, revue finale de la PR 2 de la 1.1.0 ; décisions D-09 et D-06 du plan de correction)
- **Date** : 2026-11-02
- **Jalons concernés** : version 1.1.0 (tâches D-09, D-06 et D-06-suite)
- **Questions liées** : aucune ouverte ; le seuil est **provisoire** côté amendements (voir plus bas)
- Amendements : AM-74 et AM-75 (00 §14). Amende 03 §7.9 (résumé) et 05 §5.12.

## Contexte

Deux règles de 1.1.0 reposent sur des seuils ou des marqueurs mesurés sur peu de relevés, sans fonction de contrôle en production :

1. **Résumé d'une délibération CNIL (D-09).** Le résumé d'une sanction ou d'une mise en demeure était le début du texte (`lead`), qui ne contient ni l'amende ni l'injonction. Le dispositif d'une décision de la CNIL n'a pas de balise : il faut des marqueurs de texte.
2. **Note de faible similarité de `fr_diff_article` (D-06).** Quand deux versions d'un article ne partagent presque aucun mot (article renuméroté ou remplacé), le diff mot à mot est illisible et rien ne le dit.

## Décision

### Règle de dispositif de la CNIL (`DispositifRule::Cnil`, `lf-domain/src/text/summary.rs`)

- **En-têtes** (vue repliée : casse, accents et blancs ignorés) : « PAR CES MOTIFS » (sanctions) et « II- Mesures correctrices prononcées par la CNIL » (mise en demeure).
- **Clôtures** : la mention des voies de recours (« Cette décision peut faire… », « Cette décision est susceptible… ») ou la signature, « Le Président » / « La Présidente » seuls ou suivis d'au plus quatre mots à majuscule initiale (le nom du signataire). Une phrase du dispositif qui commence par ces mots (« Le Président de la CNIL est chargé… », « Cette décision est rendue publique… ») ne le ferme pas.
- **Dispositif fermé** : sans clôture, le résumé reste le début du texte (`lead`) ; un avis, qui n'a pas d'en-tête, aussi. Aucun marqueur n'est deviné.
- **Niveau de preuve** : marqueurs lus sur **quatre sanctions** relevées. Celui de la **mise en demeure n'est établi que sur un seul texte** (MED-2024-150) : tant qu'un second relevé ne l'a pas confirmé, un texte de mise en demeure qui s'écarte de ce modèle retombe sur `lead`, sans erreur ni signal. C'est le comportement d'avant, pas un nouveau défaut.

### Seuil de similarité (`LOW_SIMILARITY_PERCENT`, `lf-application/src/usecase/diff_article.rs`)

- La `note` est posée si `identical` est faux et que moins de **40 %** des mots de la version A se retrouvent dans la version B. Le diff reste rendu en entier. Aucune note quand le diff n'est qu'approché (l'échéance de 2 secondes est atteinte) : la part n'est alors pas fiable.
- **Calibrage côté « sans rapport »** : six textes courts (Code civil 1382 et 1240, CGI 777) et 45 paires (dans les deux sens) de dix articles réels sans rapport, de 130 à 3 928 mots, qui gardent au plus 25,8 % de leurs mots ; deux textes courts sans rapport peuvent garder jusqu'à 30,4 % de mots outils. Un amendement relevé (CGI 777, versions 13 à 14) garde 99 %.
- **Provisoire côté amendements** : aucun amendement réel gardant 40 à 70 % des mots n'a été relevé. Un tel amendement recevrait la note ou non selon sa part conservée ; le seuil est donc tenu pour **provisoire** de ce côté.
- **Condition de réexamen** : dès qu'un amendement réel gardant 40 à 70 % des mots est relevé (retour d'usage, ou campagne de relevés suivante), le seuil est mesuré dessus et confirmé ou modifié par un nouvel ADR. Aucune capture n'est lancée pour cela.

## Conséquences

- Un résumé de sanction cite l'amende, l'injonction, l'astreinte et la publicité ; un avis reste en `lead`.
- Une note de faible similarité peut manquer pour un amendement lourd (faux négatif) ou être posée à tort sur un amendement qui garde moins de 40 % des mots (faux positif) : dans les deux cas le diff complet est rendu, la note n'en retire rien.
- Les deux règles sont dans le code sous des constantes nommées et des tests qui citent les relevés ; une modification passe par un ADR et 00 §14.

## Alternatives écartées

| Alternative | Raison |
|---|---|
| Garder le début du texte pour toute la CNIL | Perd l'amende et l'injonction, le contenu utile d'une sanction (D-09). |
| Marqueurs devinés pour les mises en demeure | Un seul relevé : un marqueur deviné aurait pu couper un dispositif sans signal. Le repli sur `lead` est sûr. |
| Seuil de 30 % (plan initial) | Des textes courts sans rapport gardent jusqu'à 30,4 % de mots outils : la note aurait manqué. |
| Lancer une capture pour trouver un amendement à 40-70 % | Hors du périmètre de la revue ; la condition de réexamen suffit. |

## Références et sources

- [03-domaine.md](../plan/03-domaine.md) §7.9 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.12 ; [00-decisions.md](../plan/00-decisions.md) §14 (AM-74, AM-75).
- Code : `crates/lf-domain/src/text/summary.rs` (`CNIL`, `is_signature`), `crates/lf-application/src/usecase/diff_article.rs` (`LOW_SIMILARITY_PERCENT`).
