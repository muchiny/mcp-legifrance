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
| Architecture | `make arch` | liste blanche des dépendances normales (`lf-domain` : `thiserror`, `aho-corasick`, `regex` et `unicode-normalization`), frontières application et adapters (§2), R6, R8 ; échoue fermé si `cargo metadata` est incomplet |
| Pureté | `make purity` | `lf-application` compile seul (ses features ne dépendent d'aucun autre membre) |
| Dépendances | `make deny`, `make machete` | aucun avis, aucune dépendance inutile, aucun doublon de version hors des `skip` justifiés de `deny.toml` ; `make deny` couvre le workspace et `fuzz/` ; `make audit-fuzz` (réseau, hors `make ci`) |
| Licences tierces | `cargo about generate about.hbs` (job `release-gates`, à l'étiquette) | `THIRD_PARTY_LICENSES.md` identique à la régénération (voir « Licences tierces ») |
| Âge des dépendances | `make min-age` (PR) ; `make min-age-selftest` dans `make ci` | toute version ajoutée ou montée publiée depuis 14 jours au moins |
| Workflows | `make workflow-lint` | zizmor et actionlint sans alerte, actions épinglées par SHA |
| Couverture | `make cov` | 100 % lignes, régions, fonctions (`--lib`) |
| Mutation | `make mutants FILE=…` / CI | 0 mutant `MISSED` |
| Parité | `make surface` | golden de parité reproduit à l'octet depuis les fixtures, table des codes identique à celle du plan |
| Instructions du serveur | `make instructions` | 3 500 caractères au plus (05 §4.1), mesurés sur le binaire |
| Verrou des descripteurs | `make lock` | `tests/tools.lock.json`, manifeste MCPB et `server.json` identiques au binaire ; après un changement de description, de schéma, de version ou de surface : `cargo xtask lock-descriptors --update`, puis relire le diff (les identifiants de `server.json` se changent à la main) |
| Analyse statique | job `sonar` (CI) ; en local, `make cov` puis `make cov-cobertura` et `make clippy-json` produisent ses deux rapports | informatif, non bloquant |

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

## Identifiants PISTE (développement local)

Les sondes live et les tests contre l'API Légifrance ont besoin d'un compte PISTE.
Sandbox par défaut ; l'application de production est distincte et n'est jamais
utilisée en CI (`docs/plan/00-decisions.md` §10, Q3).

1. Créer un compte sur `piste.gouv.fr`, puis une application **sandbox**.
2. Souscrire cette application à l'API **Légifrance** (DILA), et à elle seule.
   Sans cette souscription, le jeton s'obtient mais tout appel à l'API répond 403.
3. Relever, dans la section « Identifiants Oauth » de l'application (pas « API
   Keys »), le Client ID et le secret.
4. Les ranger hors du dépôt, lisibles par leur propriétaire seul :

   ```sh
   mkdir -p ~/.config/mcp-legifrance && chmod 700 ~/.config/mcp-legifrance
   # puis créer ~/.config/mcp-legifrance/piste.env avec ces trois lignes :
   #   LF_PISTE_ENV=sandbox
   #   LF_PISTE_CLIENT_ID=<Client ID>
   #   LF_PISTE_CLIENT_SECRET=<secret>
   chmod 600 ~/.config/mcp-legifrance/piste.env
   ```

5. Les charger dans le terminal qui lance la sonde ou le serveur :
   `set -a; . ~/.config/mcp-legifrance/piste.env; set +a`.
6. Vérifier le jeton avec la recette de `docs/plan/09-risques-et-questions.md`
   §2.2 : la réponse doit être un 200 avec `token_type` `Bearer`.

Le secret reste dans l'environnement du processus : jamais dans une ligne de
commande, un fichier du dépôt, un journal, une fixture, une issue ou une
conversation avec un assistant. `.env` et `.env.*` sont dans `.gitignore`, mais
un fichier hors du dépôt évite l'accident.

## Licences

Le projet est sous licence Apache-2.0. La licence entrante est la licence
sortante (section 5 de la licence) : toute contribution est publiée sous
Apache-2.0, sans CLA (`docs/plan/08-distribution.md` §9.1).

## Licences tierces

`THIRD_PARTY_LICENSES.md` reproduit les licences des crates liés au binaire
(MIT et BSD exigent de reproduire leur notice). Il est versionné, généré par
`cargo-about` à partir de `about.toml` et `about.hbs`, et jamais modifié à la
main. Le job `release-gates` le régénère à chaque étiquette et échoue au
moindre écart (`diff -u`).

- **Version épinglée : cargo-about 0.9.2**, la même que `release-gates`. Sa
  feature `cli` n'est pas active par défaut :
  `cargo install cargo-about --locked --version 0.9.2 --features cli`. Compter
  quelques minutes de compilation (profil `release`) ; sur WSL, `TMPDIR=/var/tmp`
  et aucune autre compilation en parallèle (section « Sur WSL »).
- **Régénérer**, à la racine du dépôt, sans option (c'est la commande exacte de
  `release-gates`, toute la configuration est dans `about.toml`) :

  ```sh
  cargo about generate about.hbs > THIRD_PARTY_LICENSES.md
  ```

  Relancer la commande donne le même fichier à l'octet : `diff -u
  THIRD_PARTY_LICENSES.md <(cargo about generate about.hbs)` ne sort rien.
- **Quand** : dès qu'une PR change `Cargo.lock` (ajout, montée ou retrait d'un
  paquet), `about.toml` ou `about.hbs`. Commiter le fichier dans la même PR.
- **Licence refusée** : la génération échoue (`failed to satisfy license
  requirements`) si un paquet n'offre aucune licence de la liste `accepted`.
  Cette liste est celle de `deny.toml` (`[licenses] allow`), dans le même ordre ;
  une licence se discute une fois (ADR) puis s'ajoute aux deux fichiers. Une
  exception à un seul paquet (comme MPL-2.0 pour `option-ext`) s'écrit dans les
  deux, sous le nom du crate.
- **Périmètre** : tout le workspace, hors dépendances de développement et de
  construction, pour les sept cibles de `dist-workspace.toml`. Seuls les
  fichiers de licence des paquets font foi, aucun service tiers ne les complète :
  la sortie est la même avec `--offline`.

## Sécurité et confidentialité

- **Vulnérabilité** : jamais d'issue publique. Signalement privé selon
  `SECURITY.md`, sans identifiant PISTE, jeton ni journal d'audit réel.
- **Secrets** : aucune option de ligne de commande, aucun journal, message,
  fixture ni ligne d'audit n'en porte (`SECURITY.md`, menace MEN-01). Une clé
  PISTE publiée par erreur se réinitialise sur PISTE tout de suite.
- **`SECURITY.md`** : la même PR le met à jour quand elle change une limite
  connue, le périmètre ou une garantie qu'il énonce (journal d'audit, TLS,
  transport HTTP).
- **`PRIVACY.md`** : la même PR le met à jour quand le binaire contacte un
  nouvel hôte (tableau « Hôtes contactés », 08 §1.4), envoie une donnée de plus à
  PISTE ou conserve un nouveau fichier en local. `packaging/mcpb/manifest.json`
  (`privacy_policies`) pointe vers ce fichier.
- Rien ne se vérifie de mémoire : un point non vérifié s'écrit « non vérifié »
  avec son numéro `V-nnn`, comme V-165 (ACL Windows du journal d'audit) dans
  `PRIVACY.md`.

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
- Tout changement de `Cargo.lock` régénère `THIRD_PARTY_LICENSES.md` dans la
  même PR (section « Licences tierces »).

## Toolchain

Rust 1.99.0 est épinglée, avec `clippy`, `rustfmt` et `llvm-tools-preview`
(requis par `make cov`). Une PR par montée de version, qui aligne les étapes
`dtolnay/rust-toolchain`. MSRV 1.88 (`rust-version`, `msrv` de `clippy.toml`),
vérifiée par le job `msrv` (`cargo +1.88 check`).
