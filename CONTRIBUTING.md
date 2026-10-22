# Contribuer à mcp-legifrance

Le plan et les décisions communes sont dans `docs/plan/00-decisions.md` ; les
exigences de qualité (§10) y font foi. Ce fichier dit comment les tenir au quotidien.

## Boucle de travail

1. Écrire le test qui échoue (`#[cfg(test)] mod tests` dans le fichier du code testé).
2. Le lancer et le voir échouer : `cargo nextest run -p <crate> <nom>`.
3. Écrire le code minimal qui le fait passer, relancer.
4. Couverture : `make cov` (100 % lignes, régions et fonctions, sinon échec).
5. Mutation du fichier touché : `make mutants FILE=<chemin>` → `0 missed`.
6. `make ci` vert, puis commit.

## Portes de qualité

| Porte | Commande | Seuil |
|---|---|---|
| Format | `make fmt-check` | aucun écart |
| Lint | `make lint` | zéro avertissement (`clippy::pedantic` inclus) |
| Tests | `make test` | tous verts : nextest (profil `default`, `ci` en CI) puis doc-tests ; en CI, Linux, macOS et Windows |
| Doc | `make doc` | zéro avertissement |
| Architecture | `make arch` | liste blanche des dépendances normales (`lf-domain` : `thiserror` seul), frontières application et adapters (§2), R6, R8 ; échoue fermé si `cargo metadata` est incomplet |
| Dépendances | `make deny`, `make machete` | aucun avis, aucune dépendance inutile ; `make deny` couvre le workspace et `fuzz/` ; `make audit-fuzz` (réseau, hors `make ci`) |
| Âge des dépendances | `make min-age` (PR) ; `make min-age-selftest` dans `make ci` | toute version ajoutée ou montée publiée depuis 14 jours au moins |
| Workflows | `make workflow-lint` | zizmor et actionlint sans alerte, actions épinglées par SHA |
| Couverture | `make cov` | 100 % lignes, régions, fonctions (`--lib`) |
| Mutation | `make mutants FILE=…` / CI | 0 mutant `MISSED` |

La CI rejoue tout sur chaque PR et mute le diff de la PR ; le lundi, `weekly.yml`
mute tout le workspace (4 shards) et lance les cibles de fuzz ; chaque nuit,
`deny-nightly.yml` relance cargo-deny.

## Écrire des tests qui tiennent 100 % des régions

Le code de test compte dans la couverture. Trois formes laissent une région
jamais exécutée :

- `assert!(cond, "{}", expr)` : l'expression du message n'est évaluée qu'en cas
  d'échec. Écrire `assert_eq!` ou un message à arguments capturés (`"{bad:?}"`).
- `assert!(matches!(x, Variant(_)))` : le bras `_ => false` n'est jamais pris.
  Comparer la valeur entière avec `assert_eq!`.
- `let … else { panic!() }` : comparer la valeur entière avec `assert_eq!`.

Deux autres pièges, vus sur l'adapter PISTE :

- tester le code générique avec un seul couple (horloge, sleeper) :
  `cargo-llvm-cov` résume une fonction générique par son instance la plus
  couverte, pas par l'union des instances ;
- derrière un proxy, un test réseau local exige
  `NO_PROXY=127.0.0.1,localhost,::1` (la CI le pose, avec `HTTPS_PROXY` et
  `HTTP_PROXY` factices).

## Mutants survivants

Un mutant `MISSED` signale un test manquant : on écrit le test. Deux exceptions
seulement, déclarées dans `.cargo/mutants.toml` :

- un mutant prouvé équivalent (même comportement observable) : `exclude_re`,
  avec la preuve en commentaire ;
- un fichier de glue d'I/O irréductible (ex. `xtask/src/main.rs`) :
  `exclude_globs`, avec la justification en commentaire.

Un mutant `TIMEOUT` (code de sortie 3, bloquant en CI) signale une boucle qui
peut ne pas terminer. Toute boucle doit garantir sa progression, même mutée :
consommer au moins un élément par tour (`text/clean.rs`) ou borner le nombre de
tours (`text/passages.rs`).

cargo-mutants ne mute jamais une fonction nommée `new` ni un `impl Default`
(wiki de cargo-mutants, page « Mutations », section « General exclusions » :
« Functions called `new`, and implementations of `Default` are skipped » ;
<https://github.com/sourcefrog/cargo-mutants/wiki/Mutations>). Une décision
écrite dans leur corps échappe donc à la mutation. Règle : une fonction `new`
et un `impl Default` écrit à la main ne prennent aucune décision (ni `if`, ni
`match`, ni opérateur booléen, de comparaison ou arithmétique, ni appel à une
méthode qui décide à leur place, comme `filter`, `is_none` ou `unwrap_or_else`).
Ils assemblent leurs arguments dans la struct et confient tout calcul à une
fonction privée nommée autrement, mutée normalement (`CivilDate::checked`,
`Coverage::checked`, `cut_snippet`…). Préférer une forme qui donne des mutants
viables : un retour `Self` sans `Default` ne produit qu'un mutant non
compilable, une décision écrite avec un opérateur (`!`, `&&`, `>`) en produit
de vrais. Contrôle : `cargo mutants --list --file <chemin>` liste les mutants
de la fonction privée.

## Sur WSL

`/tmp` est un tmpfs en RAM. Ne lancer `cargo mutants` que par `make mutants`
(`TMPDIR=/var/tmp`, `-j 1`, un fichier, contrôle mémoire préalable). Jamais
`cargo test -j 4` ni plus.

## Licences

Le projet est sous licence Apache-2.0. La licence entrante est la licence
sortante (section 5 de la licence) : toute contribution est publiée sous
Apache-2.0, sans CLA (`docs/plan/08-distribution.md` §9.1).

## Projets AGPL : idées seulement

Deux projets sont sous AGPL : juridix (`Ktulu-Analog/juridix`) et le
mcp-legifrance de Ktulu-Analog. Cinq règles (08 §9.4) :

1. Aucune copie de code, de test, de fixture, de dictionnaire ni de donnée,
   même traduite.
2. Les idées et les comportements documentés sont permis.
3. Les faits recoupés (`LEGITEXT`, abréviations de codes) sont vérifiés sur une
   source non AGPL avant d'entrer dans le code.
4. La case du modèle de PR le rappelle, et `cargo deny` refuse les dépendances
   AGPL.
5. Les projets MIT (`pylegifrance`, `droit-francais-mcp`, `justicelibre`)
   peuvent être repris, avec leur notice.

## Dépendances

- Âge minimal de 14 jours (`pubtime` de l'index sparse de crates.io), contrôlé
  par `make min-age` dans le job `deny`.
- Exception : le libellé `security-fix`, justifié dans la PR. Poser un libellé
  ne relance pas la CI : relancer le job `deny` à la main. Avis RUSTSEC dont la
  version corrigée a moins de 14 jours : `cargo update -p <crate> --precise
  <version>`, libellé `security-fix`, avis, paquets et dates d'éligibilité dans
  la description, relance manuelle du job `deny`, `make min-age
  SECURITY_FIX=true` en local ; jamais d'`ignore` dans `deny.toml`
  (`docs/plan/06-qualite.md` §13.1).
- Après un ajout : `cargo fetch`, `make min-age`, puis `cargo update -p
  <transitif> --precise <version éligible>` pour chaque paquet signalé (index
  `https://index.crates.io/…`, champ `pubtime`, versions retirées ignorées),
  `make min-age` à nouveau ; une exigence `1.53` laisse cargo prendre `1.54` :
  toujours `--precise`.
- Exigence au niveau mineur dans `[workspace.dependencies]` ; version fixée par
  `cargo update -p <crate> --precise <version>`.
- Dépendance déclarée seulement dans le crate qui l'emploie, licence dans la
  liste blanche de `deny.toml`, `make deny` et `make machete` verts.

## Toolchain

Rust 1.99.0 est épinglée, avec `clippy`, `rustfmt` et `llvm-tools-preview`
(requis par `make cov`). Une PR par montée de version, qui aligne les étapes
`dtolnay/rust-toolchain`. MSRV 1.88 (`rust-version`, `msrv` de `clippy.toml`),
vérifiée par le job `msrv` (`cargo +1.88 check`).
