# Contribuer à mcp-legifrance

Le plan et les décisions communes sont dans `docs/plan/00-decisions.md` ; les
exigences de qualité (§9) y font foi. Ce fichier dit comment les tenir au quotidien.

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
| Tests | `make test` | tous verts |
| Doc | `make doc` | zéro avertissement |
| Architecture | `make arch` | liste blanche des dépendances normales (`lf-domain` : `thiserror` seul), frontières application et adapters (§2) ; échoue fermé si `cargo metadata` est incomplet |
| Dépendances | `make deny`, `make machete` | aucun avis, aucune dépendance inutile |
| Couverture | `make cov` | 100 % lignes, régions, fonctions (`--lib`) |
| Mutation | `make mutants FILE=…` / CI | 0 mutant `MISSED` |

La CI rejoue tout sur chaque PR et mute le diff de la PR ; le lundi, `weekly.yml`
mute tout le workspace (4 shards) et lance les cibles de fuzz.

## Écrire des tests qui tiennent 100 % des régions

Le code de test compte dans la couverture. Trois formes laissent une région
jamais exécutée :

- `assert!(cond, "{}", expr)` : l'expression du message n'est évaluée qu'en cas
  d'échec. Écrire `assert_eq!` ou un message à arguments capturés (`"{bad:?}"`).
- `assert!(matches!(x, Variant(_)))` : le bras `_ => false` n'est jamais pris.
  Comparer la valeur entière avec `assert_eq!`.
- `let … else { panic!() }` : comparer la valeur entière avec `assert_eq!`.

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

## Sur WSL

`/tmp` est un tmpfs en RAM. Ne lancer `cargo mutants` que par `make mutants`
(`TMPDIR=/var/tmp`, `-j 1`, un fichier, contrôle mémoire préalable). Jamais
`cargo test -j 4` ni plus.
