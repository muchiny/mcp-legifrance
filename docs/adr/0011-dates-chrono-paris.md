# ADR-0011 — Dates : `CivilDate` dans le domaine, chrono hors domaine, Europe/Paris, sentinelles

> **Amendé le 2026-10-16 (décision C3).** Le domaine garde le `CivilDate` maison livré avec M1 (`crates/lf-domain/src/date.rs` : `new`, `parse`, `from_unix_millis`, `to_french`, `year`, `month`, `day`) et **n'utilise pas `chrono`** ; `lf-domain` n'a pour dépendance normale que `thiserror`, ce que `check-arch` impose par une liste blanche qui échoue fermé (`xtask/src/arch.rs:26`, [ADR-0016](0016-hexagonal-workspace.md)). `chrono` (`std`) ne sert qu'hors du domaine (`lf-application` : `Clock::now_utc`, horodatage d'audit ; adapters) ; la feature `clock` n'est activée que dans `lf-app::clock` (date du jour à Paris) ; `chrono-tz` n'est utilisé que dans `lf-app::clock` et `lf-adapter-piste` (midi Europe/Paris de `singleDate`, AM-02). Là où le texte ci-dessous écrit `NaiveDate`, lire `CivilDate` dans le domaine.

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M1 (livré : `CivilDate`, conversions pures), M2 (formats d'entrée, `singleDate`, `open_ended`, `VersionDate` borné, test du vecteur 1804-03-21), M3 (`Clock`, `SystemClock`)
- **Questions liées** : —
- Révisé le 2026-10-15 (revue) : alignement sur 00 révision 2 ; `chrono-tz` aussi dans `lf-adapter-piste` ; seuil ≥ 2222-01-01 et sens de l'absence par champ, désormais décision de 00 §4 (AM-01) ; `singleDate` à midi Europe/Paris et nouveaux vecteurs (AM-02) ; contrôle final du 2026-10-16 : V-76 citée par son étiquette de référence, à midi (09 §2.1).

## Contexte

- **« Aujourd'hui » est calculé en UTC** (`time.gmtime()`, `server.py:125-126` ; D-19, E-05). Entre 00:00 et 01:00 (hiver), ou 02:00 (été), heure de Paris, `fr_get_act` renvoie la version d'hier.
- **`ms_to_date` est fragile** (D-20, E-35) :
  - sous Windows, toute date antérieure à 1970 devient `None` (CPython passe par `gmtime_s`). Exemple : `-5231606400000`, soit 1804-03-21 (Code civil, `consult_article.json`). Conclusion jugée très probable, non testée sur Windows : **NON VÉRIFIÉ** [V-191 · ref-windows-dates-pre-1970] ;
  - `True` donne `1970-01-01`.
- **La sentinelle `2999-01-01` est exposée comme une vraie date** (D-20, E-11) : le modèle peut écrire « en vigueur jusqu'au 1er janvier 2999 ». Elle apparaît 22 fois dans les fixtures, sous trois formes : epoch ms, `AAAA-MM-JJ` et ISO (recherche 03 §11.2).
  - L'OpenAPI documente la règle « `null` si date ≥ 2222-01-01 » pour les champs `…Computed` (recherche 03 §11.2).
  - Dans les dumps, `2222-02-22` signifie « entrée en vigueur différée à une date non déterminée » (recherche 05 §6.3).
- **Trois formats d'entrée** (D-21, recherche 03 §11.2) :
  - entier epoch ms : 39 occurrences, toutes à minuit UTC hors sentinelles ;
  - ISO `…+0000` : 34 occurrences, seul format de `/search` ;
  - `AAAA-MM-JJ` : 16 occurrences ;
  - et des chiffres en chaîne, selon les exemples de l'OpenAPI.
- **Le paramètre `date` n'est pas validé** : « 2016-13-45 » part vers l'API (D-22).
- **`singleDate` à la milliseconde** rend la clé de cache volatile (D-01, [ADR-0007](0007-cache-memoire-port.md)).
- **Bibliothèque.** `chrono::DateTime::from_timestamp_millis` calcule en Rust pur (`div_euclid`/`rem_euclid`), indépendamment de la plateforme (`chrono/src/datetime/mod.rs:838-842` @ `6adaa52`, recherche 02 D-20). `chrono` est déjà dans le graphe via rmcp (recherche 04 §4.3). Le domaine n'a besoin que de la validation calendaire, des epochs négatifs et de la prose française, que `CivilDate` couvre déjà (`div_euclid`, 100 % de couverture, 0 mutant survivant) ; il n'utilise aucune autre fonction de chrono.

## Décision

1. **`CivilDate` dans le domaine, `chrono` hors du domaine.**
   - `lf-domain` n'importe pas `chrono` et ne lit jamais l'heure ; sa dépendance normale unique est `thiserror` (liste blanche de `check-arch`).
   - `chrono` (feature `std`) est utilisé hors du domaine (`lf-application` : `Clock::now_utc`, `AuditRecord.ts` ; adapters) ; la feature `clock` n'est activée que dans `lf-app::clock` (date du jour à Paris) ; `chrono-tz` n'est utilisé que dans `lf-app::clock` et `lf-adapter-piste` (midi Europe/Paris de `DATE_VERSION`, 04 §6.3 ; conversion `CivilDate` vers `NaiveDate`) (00 §2, §4 ; 02 §3.0.1, §3.5). La liste blanche du domaine ne s'élargit que par un ADR au jalon qui en a besoin.
2. **Types** (03 §6.1) :
   - toute date juridique civile est un `CivilDate` (années 1 à 9999, `Ord`) ; `from_epoch_ms` s'appelle `CivilDate::from_unix_millis` et `format_fr` `CivilDate::to_french` ;
   - `VersionDate` est aujourd'hui un alias de `CivilDate` (livré) ; en tâche de tête de M2 il devient un newtype `VersionDate(CivilDate)` qui se parse strictement : `AAAA-MM-JJ` ASCII, date calendaire valide, plage `1500-01-01..=2221-12-31`. Sinon, `invalid_arg` local, **sans appel réseau** ;
   - `DateRange { from ≤ to }`.
3. **Sentinelles → `None`** (00 §4) : toute date ≥ 2222-01-01 (sentinelles observées `2999-01-01` et `2222-02-22`, règle des champs `…Computed` de l'OpenAPI) et tout `…Computed` nul deviennent `None`, donc un champ omis en sortie.
   - Le domaine l'applique par une règle de seuil (`open_ended`, 03 §6.2) : 2222-01-01 devient `None`, 2221-12-31 est conservé. **Non livré en M1** : tâche de tête de M2 avec `SENTINEL_FLOOR` (00 §11.1) ; la doc de `act.rs:41` et des autres champs de date est corrigée alors.
   - Le sens de `None` dépend du champ et est documenté dans les descriptions d'outils (05 §1.4 règle 9) : date de fin → « en vigueur sans date de fin » ; date de début → « date d'entrée en vigueur non déterminée », l'`etat` `VIGUEUR_DIFF` portant l'information (V-101) ; date de publication ou du texte → « inconnue ».
4. **Formats d'entrée** : parsés dans `lf-adapter-piste`.
   - Acceptés : epoch ms entier, chiffres en chaîne, ISO `+0000` (partie date), `AAAA-MM-JJ`.
   - Rejetés : booléens et flottants non entiers.
   - Un horodatage qui n'est pas à minuit UTC produit un diagnostic.
5. **« Aujourd'hui » = `Clock::today_paris() -> CivilDate`**, date civile Europe/Paris, fournie par `SystemClock` (`lf-app::clock`). `chrono::Utc::now()` n'est appelé qu'à cet endroit.
6. **`DATE_VERSION.singleDate`** = **midi** Europe/Paris du jour `version_date`, en epoch ms, calculé par l'adapter (`paris_noon_ms`, 04 §6.3 ; 00 §8) pour LODA_DATE et CODE_DATE, qui convertit d'abord `CivilDate` en `chrono::NaiveDate`.
   - Midi (10:00Z en été, 11:00Z en hiver) est stable sur la journée et reste correct que PISTE compare l'instant à des bornes stockées à minuit UTC ou le tronque en date UTC ou Paris. La règle est confirmée par V-76 en M2 (bloquante).
   - Vecteurs (00 §8, calculés avec la base IANA de `zoneinfo`) : `2026-10-15` → `1792058400000` ; `2026-01-15` → `1768474800000` ; `2026-03-29` → `1774778400000` ; `2026-10-25` → `1792926000000` (jours de changement d'heure compris).
7. **Sortie** :
   - les dates du JSON sont des chaînes ISO `AAAA-MM-JJ` ;
   - une sentinelle n'est **jamais** rendue : le champ est omis ;
   - les dates en prose (« 1er février 2025 ») n'apparaissent que dans `citation` ([ADR-0009](0009-contrat-de-citation-v2.md)).

## Conséquences

### Positives

- La version « en vigueur aujourd'hui » est correcte toute la journée, y compris les jours de changement d'heure (D-19).
- Même résultat sous Windows, Linux et macOS (D-20, E-35).
- Plus jamais de « 2999 » dans une réponse.
- Les dates invalides sont refusées sans quota consommé (D-22). La clé de cache est stable sur la journée (D-01).

### Négatives

- **Divergences de parité** (`date_fin` et `date_fin_version` omis, date du jour décalée de 0 à 2 h) : clés de l'annexe A §4.3.
- **Règle de seuil** (00 §4 révision 2, fondée sur l'OpenAPI) : une vraie date postérieure au 2221-12-31 serait perdue. Aucune n'est connue dans les données réelles.
- Le domaine porte sa propre arithmétique de dates (`CivilDate`) ; l'oracle `proptest` et la mutation (100 % des lignes, 0 mutant survivant, [ADR-0002](0002-portes-qualite.md)) la gardent. L'adapter PISTE convertit vers `chrono`.
- Une seconde bibliothèque de fuseaux (`chrono-tz`, hors domaine) embarque une base IANA qu'il faut tenir à jour, dans deux crates (`lf-app`, `lf-adapter-piste`).

### À surveiller

- **NON VÉRIFIÉ** [V-76 · singledate-midi-paris] : un texte qui entre en vigueur le jour J est-il retenu avec `singleDate` à midi Paris (10 h ou 11 h UTC du jour J) ? Comparaison avec minuit UTC, minuit Paris et la veille, bloquante en M2 (V-76).
- **NON VÉRIFIÉ** [V-100 · convention-horaire-timestamps] : les horodatages de date sont-ils tous à minuit UTC ?
- **NON VÉRIFIÉ** [V-101 · sentinelle-2222-debut] : sens de `2222-02-22` en date de début, côté API.
- **NON VÉRIFIÉ** [V-89 · version-date-bornes] : bornes de `VersionDate` et réaction de l'API aux dates extrêmes.
- **NON VÉRIFIÉ** [V-17 · chrono-tz-iana] : version de la base IANA embarquée par `chrono-tz` 0.10.4.

### Tests qui la gardent

- `today_is_paris_date_at_2230_utc_in_summer` (2026-07-14T22:30Z donne 2026-07-15), `today_paris_in_winter`, `dst_transition_days` (2026-03-29 et 2026-10-25).
- `from_epoch_ms_vectors` (nom stable du catalogue, correspondance livrée : 06 §3.1 ; teste `CivilDate::from_unix_millis`), `open_ended_maps_sentinels_to_none`, `sentinel_2999_and_2222_are_none`, `bool_is_not_a_date`, `negative_ms_1804` (vecteur 1804-03-21 = -5231606400000 sur `CivilDate::from_unix_millis`, test à ajouter en tâche de tête de M2).
- `invalid_date_is_rejected_locally` (wiremock `expect(0)`), `paris_noon_vectors` (vecteurs de 00 §8).
- Sonde live `live_singledate_noon_paris_semantics` (M2, bloquante, V-76 ; 04 §13.5).
- Exécution sous Windows en CI (06 §2.1, §3.1).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| `chrono::NaiveDate` dans `lf-domain` (ADR-0011 d'origine) | Ajoute une dépendance au domaine pour des besoins que `CivilDate` couvre déjà ; élargir la liste blanche de `check-arch` (échec fermé) sans nécessité. Réécrire `date.rs` (342 lignes) ferait perdre l'oracle `proptest` et refaire la mutation (décision C3, 2026-10-16). |
| `jiff` | Seconde bibliothèque de dates, puisque chrono est déjà tiré par rmcp (recherche 04 §4.3). |
| `time` | Même raison. |
| Enum `LegalDate { Date, Open, Unknown }` (recherche 02 D-20, recherche 03 §11.2) | 00 §4 retient `Option<CivilDate>`. Le sens de `None` dépend du champ et est documenté par outil. |
| Date du jour en UTC (parité E-05) | Erreur juridique jusqu'à deux heures par jour (D-19). |
| `singleDate` = instant de l'appel (référence) | Clé de cache volatile (D-01). Résultat différent d'une minute à l'autre. |
| `singleDate` = minuit Europe/Paris (première version du plan) | 22 h ou 23 h UTC la veille : correct seulement si PISTE tronque l'instant en date de Paris ; sinon la version de la veille est retenue (R-17, AM-02). |
| `singleDate` = minuit UTC | Exactement sur la borne : risqué si PISTE compare strictement (AM-02). |
| Restituer les sentinelles telles quelles (parité E-11) | Le modèle cite une date fictive (D-20). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §2, §3, §4, §8 (révision 2, AM-01, AM-02) ; [03-domaine.md](../plan/03-domaine.md) §6.1 à §6.5 ; `crates/lf-domain/src/date.rs` ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §6.3, §8.2, §13.5 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §1.4 (règle 9) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) (R-17, V-76) ; [02-architecture.md](../plan/02-architecture.md) §5.3, §8.4 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §4.3.
- Recherche : [02-defauts.md](../recherche/02-defauts.md) D-01, D-19 à D-22 ; [03-dto.md](../recherche/03-dto.md) §11.2 ; [05-piste-dila.md](../recherche/05-piste-dila.md) §6.3 ; [01-parite.md](../recherche/01-parite.md) §4.4, E-05, E-11, E-35 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §4.3, §6 (point 10).
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/server.py:125-136, 243` ; `src/fr_eli_mcp/citations.py:66-73, 283-295`.
- chrono : https://github.com/chronotope/chrono (`6adaa52`).
