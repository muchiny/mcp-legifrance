# ADR-0024 — Correctifs de la 1.1.1 et rejeu de non-régression en production

- **Statut** : **AM-88 acceptée** par le propriétaire le 2026-11-02, avant tout appel de production. Les entrées AM-78 à AM-87 (statut **Proposé**) sont ajoutées à la fin de la vague 0 du plan de la 1.1.1, une par commit, pour les seules unités retenues.
- **Date** : 2026-11-02
- **Jalons concernés** : version 1.1.1 (correctifs du test de bout en bout de la 1.1.0 en production)
- **Questions liées** : Q3 (politique des tests live et des fixtures, 00 §10)
- Amendements : AM-88 (00 §14). Les numéros AM-78 à AM-87 sont des propositions, pas des réservations : une unité qui sort de la 1.1.1 libère le sien, et cet ADR le dit en une ligne.

## Contexte

Le test de bout en bout de la 1.1.0 en production (2026-11-02) a confirmé 21 défauts importants et moyens, corrigés en 1.1.1. Le propriétaire exige qu'aucune réponse hors de ces défauts ne change. La preuve retenue est un rejeu des 2 092 appels distincts du test, sur la 1.1.0 et sur la 1.1.1, en production, avec le bruit de PISTE mesuré.

00 §10 (Q3) n'admet en production que des **sondes** en lecture seule, lancées à la main par le mainteneur avant chaque étiquette, à 1 requête par seconde au plus. Un rejeu de plusieurs milliers d'appels sur trois serveurs n'est pas une sonde : il faut une dérogation écrite.

## Décision

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
