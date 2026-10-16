# ADR-0002 — Tests unitaires, couverture et mutation au maximum

Statut : accepté (2026-10-15). Exigence utilisateur, `docs/plan/00-decisions.md` §10.

## Contexte

Le serveur produit des citations juridiques : une URL fausse, une date mal
convertie ou un « 1 janvier » au lieu de « 1er janvier » passe inaperçu dans
une réponse de modèle. La couverture seule ne prouve pas que les tests
vérifient quelque chose ; la mutation le prouve.

## Décision

- TDD : chaque comportement naît d'un test qui échoue.
- Tests unitaires dans le fichier du code (`#[cfg(test)] mod tests`), propriétés
  `proptest` pour les parseurs et utilitaires texte, cibles `cargo-fuzz` pour
  les entrées non fiables.
- Couverture `cargo llvm-cov --lib` : 100 % des lignes, régions et fonctions,
  bloquant en local (`make cov`) et en CI.
- Mutation `cargo-mutants` : 0 mutant `MISSED`, bloquant sur le diff de chaque
  PR ; balayage complet hebdomadaire. Deux exclusions seulement, déclarées dans
  `.cargo/mutants.toml` avec leur justification en commentaire : `exclude_re`
  pour un mutant prouvé équivalent, `exclude_globs` pour la glue d'I/O
  irréductible justifiée (`xtask/src/main.rs`, `fuzz/**`).

## Conséquences

- Mesuré sur M0–M1 à la clôture (2026-10-15, `make ci` et
  `make mutants-diff BASE=main`, 20 fichiers) : 137 tests (129 `lf-domain`,
  8 `xtask`) ; 100 % des lignes, régions et fonctions ; 369 mutants, 0
  `MISSED`, 0 `TIMEOUT` (334 tués, 35 non compilables), sans aucun
  `exclude_re` (seul `exclude_globs` couvre `xtask/src/main.rs` et `fuzz/**`).
  Deux survivants équivalents (`text/rank.rs`) avaient disparu en simplifiant
  une garde redondante, pas en les excluant.
- Les adapters à I/O (M2+) isolent leur logique dans des fonctions pures
  (mapping DTO, politique de retry) pour rester sous ces portes ; la glue d'I/O
  irréductible est déclarée dans `exclude_globs` avec sa justification.
- Coût : chaque PR paie la mutation de son diff en CI.
