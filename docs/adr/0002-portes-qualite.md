# ADR 0002 — Tests unitaires, couverture et mutation au maximum

Statut : accepté (2026-10-15). Exigence utilisateur, `docs/plan/00-decisions.md` §9.

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
  PR ; balayage complet hebdomadaire. Une exclusion n'est admise que pour un
  mutant prouvé équivalent, preuve en commentaire dans `.cargo/mutants.toml`.

## Conséquences

- Mesuré sur M0–M1 en essai à blanc (2026-10-15) : 100 % des lignes, régions
  et fonctions ; 337 mutants, 0 survivant (303 tués, 34 non compilables), sans
  aucune exclusion. Deux survivants équivalents (`text/rank.rs`) ont disparu en
  simplifiant une garde redondante, pas en les excluant.
- Les adapters à I/O (M2+) isolent leur logique dans des fonctions pures
  (mapping DTO, politique de retry) pour rester sous ces portes ; la glue d'I/O
  irréductible est déclarée dans `exclude_globs` avec sa justification.
- Coût : chaque PR paie la mutation de son diff en CI.
