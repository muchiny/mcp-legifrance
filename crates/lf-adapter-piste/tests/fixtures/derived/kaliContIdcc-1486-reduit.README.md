# kaliContIdcc-1486-reduit.json

Dérivé réduit, **pas un relevé brut** : il n'est pas compté par `RECORDED` (`src/integrity.rs`) et n'a
pas de sidecar `.meta.toml`.

Origine : relevé C5 de `/consult/kaliContIdcc` pour l'IDCC 1486 (Syntec), sandbox PISTE, capturé le
2026-11-01T01:59:34Z par `xtask record-fixture 1.0.0` (1 224 830 octets, sha256
`c80317aa701ee7d63fa48d65dcdffd001033f117f981a073bb51500b90185e41`). Le relevé complet dépasse le
plafond de 1 Mio des fixtures versionnées ; il est gardé hors dépôt.

Conservé, valeurs recopiées telles quelles : `id`, `titre`, `num`, `texteBaseId` et, pour chacune
des quatre sections de premier niveau, `id`, `etat` et `title`.

Retiré : le reste de l'enveloppe (`executionTime`, `numeroTexte`, `nature`, `categorisation`,
`activitePro`, `descriptionFusionHtml`), les champs des sections que le DTO ne lit pas
(`executionTime`, `dereferenced`, `cid`, `intOrdre`, `dateModif`, `dateDebut`, `dateFin`,
`sectionConsultee`, `commentaire`, `renvoi`, `renvoiNum`, `notaHtml`, `notaSectionsAafficher`) et
surtout les sous-arbres `sections[].sections` et `sections[].articles` (la table des matières
entière, 1,2 Mio). Aucun jeton, aucun identifiant de compte : seulement des identifiants publics
Légifrance.

Ce que le relevé montre : `texteBaseId` = [`KALITEXT000005679895` (ABROGE, version de 1987),
`KALITEXT000044253019` (VIGUEUR_ETEN, version 2021)] ; le titre du conteneur est celui de la version
2021 (défaut journey_general-1, L2-11).

Licence : Licence Ouverte 2.0, source Légifrance / DILA.
