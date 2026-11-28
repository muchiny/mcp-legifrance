# Fixtures Judilibre (jalon MJ, T-MJ-02)

Dérivés réduits, **pas des relevés bruts** : ni sidecar `.meta.toml`, ni compte dans `RECORDED`
(`src/integrity.rs`). Ils passent le même détecteur de secrets que les autres fixtures
(`judilibre_fixtures_pass_the_secret_detectors_and_are_listed`).

Origine : relevés de l'API Judilibre (`/cassation/judilibre/v1.0`), production PISTE, faits par le
propriétaire le 2026-11-27 (sondes de l'étude, annexe C §6.7). Les relevés complets sont gardés
hors dépôt. Le générateur, hors dépôt aussi, ne lit que ces relevés et n'envoie aucune requête.

| Fixture | Relevé | Ce qu'elle montre |
|---|---|---|
| `search-cc.json` | 08, recherche `cc`, `ca`, `tj` | trois décisions de la Cour de cassation : `chamber` codée (`civ1`), `formation` (`fs`, `frh`), `publication` codée (`b`), `summary`, `ecli`, `highlights.text` avec `<em>` |
| `search-ca.json` | 10, recherche `ca` | deux arrêts de cour d'appel : `chamber` en clair (`Pôle 3 - Chambre 1`), `location` (`ca_paris`), `solution` à `other` et `solution_alt`, `publication` vide, sans `ecli` ni `summary` |
| `search-empty-page.json` | 07, recherche `tj` triée par date | `results` vide malgré `total` = 336 114 |
| `error-416.json` | 12, `page=200&page_size=50` | corps du refus `416` (`Range Not Satisfiable`) |
| `decision-cc.json` | 13, décision `5fca624b7eeb384a40cafb85` | métadonnées entières : `visa` et `rapprochements` avec `<a href>`, `contested`, `timeline`, `titlesAndSummaries`, `update_date` |
| `decision-ca.json` | 14, décision `63c8eefddc5b777c90992fd8` | arrêt de cour d'appel zoné, `contested` et `timeline` nuls, `nac` |
| `decision-ca-sans-zones.json` | 15, décision `6253cd8cbd3db21cbdd93a2f` | source `dila` : ni `zones` ni `chamber`, `decision_date` et `update_date` à `0201-02-24` |

Conservé, valeurs recopiées telles quelles : l'enveloppe de recherche (`page`, `page_size`,
`total`, `took`, `max_score`, `relaxed`) et, pour chaque décision, toutes les métadonnées.

Retiré ou remplacé :

- recherche : `query`, `searchQuery`, `previous_page` et `next_page` (écho des mots cherchés et
  requête interne du moteur), les résultats au-delà des trois ou deux premiers, et les extraits
  au-delà des deux premiers ;
- décision : le `text` est **remplacé par un texte inventé**, sans aucun nom de magistrat, de
  greffier, d'avocat ni de partie. Les `zones` sont recalculées sur ce texte avec les propriétés
  mesurées sur les relevés : indices en unités UTF-16, dernier `end` égal à la longueur plus 1,
  un ou deux caractères entre certaines zones, `moyens` et `motivations` en fragments alternés
  (décision de la Cour de cassation), zones contiguës (cour d'appel).

Aucun jeton, aucun identifiant de compte : seulement des identifiants publics de Judilibre
(24 caractères hexadécimaux) et des numéros d'affaire.

Licence : Licence Ouverte 2.0, source Cour de cassation, base Open Data des décisions judiciaires
(Judilibre). Les textes de décision des fixtures sont inventés.
