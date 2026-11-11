# ADR-0027 — Montée de tantivy à 0.27.0 et marque de version de l'index

- **Statut** : **Proposé** — accepté à la fusion de la PR qui l'applique (la conception a été retenue sous sa forme minimale, voir « Alternatives »)
- **Date** : 2026-11-11
- **Jalons concernés** : M9 (`lf-adapter-dila`), version qui suit la 1.1.1 (numéro non décidé par le propriétaire)
- **Questions liées** : — (le numéro de la version qui porte ce changement reste à décider)
- Amendements : AM-91 (00 §14). Révise le « huit `skip` » d'[ADR-0018](0018-dependances-fin-de-projet.md) §5 (dix depuis cette montée) et complète la décision 8 d'[ADR-0019](0019-dilacorpus-hors-ligne.md). Les numéros ADR-0025 et AM-89 restent réservés par la branche locale `docs/debit-rejeux-1.1.2` ; 0026 et AM-90 sont ceux de la montée de `rmcp`.

## Contexte

- `tantivy` 0.27.0 (publiée le 2026-10-17, PR Dependabot #67) remplace `rust-stemmers` 1.2.0 (2019) par `frostem` 1.20260821.8, qui suit le Snowball récent. Le format d'index sur disque ne change pas (version 7 aux deux versions, lecture garantie jusqu'à 4) : un index bâti en 0.26.2 s'ouvre en 0.27.0 sans erreur.
- **Mesuré le 2026-11-11** avec l'analyseur français du dépôt (`french_analyzer` : découpage, minuscules, repli des accents, mots vides, racinisation) sur 57 mots : **7 changent de racine, 50 restent identiques**.

  | Mot | Racine en 0.26.2 | Racine en 0.27.0 |
  |---|---|---|
  | française | `francais` | `franc` |
  | francaise | `francais` | `franc` |
  | mauvais | `mauv` | `mauvais` |
  | niais | `ni` | `niais` |
  | jaloux | `jaloux` | `jalou` |
  | époux | `epoux` | `epou` |
  | portugaise | `portugais` | `portug` |

  Effet voulu : `française` et `français` (`franc`) se rejoignent, `mauvais` et `mauve` (`mauv`) se séparent, `ni` et `niais` se séparent. Les deux sorties complètes sont gardées hors dépôt. **Non mesuré** : l'effet sur le classement et sur le rappel avec de vrais dumps (la mesure porte sur des mots, pas sur des requêtes) ; BM25 n'est pas modifié selon le CHANGELOG amont.
- **Aujourd'hui, la seule garde est le schéma** (`open_or_create` refuse un schéma différent). Un index 0.26.2 déjà bâti, ouvert par un serveur 0.27.0, répondrait donc sans erreur avec les anciennes racines pour les documents et les nouvelles pour les requêtes : les mots touchés deviennent introuvables, en silence.
- La feature `dila` est dans les binaires publiés depuis la 1.0.0 (`dist-workspace.toml`, `features = ["dila"]`) : des utilisateurs du mode hors ligne ont un index 0.26.2.

## Décision

1. **`tantivy` 0.27.0**, mêmes features (`mmap`, `stopwords`, `lz4-compression`, `stemmer`). Aucune ligne de code de production n'est adaptée à l'API de tantivy. `columnar-zstd-compression` **reste retirée** (tantivy 0.27.0 est sur `zstd` 0.14 : la rétablir est une suite possible, pas faite ici).
2. **Marque de version de l'index** : la constante `INDEX_STAMP` (`fr_legi-1`, `lf-adapter-dila/src/index.rs`) est la charge utile de commit de tantivy, donc écrite dans `meta.json` avec le commit lui-même. Aucun fichier de plus. Elle est posée à **chaque** commit (tantivy remet la charge utile à zéro à chaque commit) par le seul site de commit de production, `IndexStore::commit` (`prepare_commit`, `set_payload`, `commit`), et lue une seule fois, à l'ouverture sur disque (`Engine::open`). Un index en mémoire est toujours neuf et n'est pas contrôlé.
3. **Règle d'acceptation** (`stamp_is_current`) : marque égale à `INDEX_STAMP`, accepté ; autre marque (y compris plus récente), refusé ; **pas de marque** : refusé si l'index contient des segments, accepté s'il est vide (dossier neuf, ou premier build interrompu avant son premier commit : rien de périmé à cacher). Le refus est l'erreur `OpenError::Stale`.
4. **Serveur** : un index refusé rend la source locale indisponible (jamais de repli sur PISTE, ADR-0019 décision 8), avec le texte fixe, en anglais, sans chemin ni numéro de version : « the local DILA index was built by another version of this server and must be rebuilt with mcp-legifrance build-dila-index (see the README, offline mode) ». La cause détaillée va au journal (stderr). Les autres refus (absent, vide, illisible) gardent leurs textes.
5. **`build-dila-index`** ouvre l'index par `DilaCorpus::open` : sur un index refusé, il s'arrête avec le message « cannot open the local DILA index: the local DILA index was built by another version of this server's text analyser, or by one that did not record it, and must be rebuilt: remove the index directory, then run build-dila-index again », code de sortie 1, **sans rien supprimer, reconstruire ni écrire**. L'utilisateur supprime lui-même le dossier d'index (le registre `registry.tsv` y est : il part avec lui) puis relance ; ou il pointe `--index` vers un dossier neuf et change `LF_DILA_DIR`. Pas d'option `--rebuild`.
6. **Règle d'entretien** (commentaire de `INDEX_STAMP`) : tout changement qui modifie les jetons de `french_analyzer` (racinisateur, mots vides, filtre, découpage) incrémente la marque. Le test `french_stems_are_pinned` (56 mots, racines mesurées avec 0.27.0) fait rougir la CI quand l'une de ces racines change.
7. **Le numéro de la version qui porte ce changement n'est pas décidé.** Le CHANGELOG l'écrit sous `[Unreleased]` ; les documents disent « la version qui suit la 1.1.1 ».
8. **Hors périmètre, suites possibles** (non faites) : lecture de la marque par `check-config` ; option `--rebuild` ; empreinte calculée à l'exécution à la place d'une marque entretenue à la main ; fixture binaire d'un index 0.26 ; épinglage par test des versions de `tantivy` et de `frostem` ; rétablissement de `columnar-zstd-compression` ; protection des binaires 1.1.x déjà publiés (impossible : ils ignorent la marque).

## Conséquences

### Positives

- Un index d'un autre analyseur n'est plus lu en silence : le modèle et l'opérateur sont dirigés vers la reconstruction.
- 47 lignes de code de production (hors commentaires et lignes vides ; mesure du 2026-11-11) dans 5 fichiers, aucune dépendance ni table de `check-arch` de plus ; la marque naît et meurt avec l'index.

### Négatives

- **Tout utilisateur du mode hors ligne doit reconstruire son index** (un index de 3,6 Go, archives déjà là, rien à retélécharger ; 1 h 10 et 3,6 Go de mémoire mesurés sur les archives réelles avec un binaire de développement, durée du binaire publié non mesurée). Les utilisateurs de PISTE ne sont pas touchés.
- Un seul texte pour « index plus ancien » et « plus récent » : voulu.
- Entretien manuel de la marque : un changement d'analyseur oublié n'est vu que par les mots listés dans le test.
- Dix `skip` dans `deny.toml` au lieu de huit (`rand@0.9.5` et `rand_core@0.9.5`, tirés par `tantivy-columnar` 0.27.0 ; `base64@0.22.1` retiré, devenu inutile). Le skip `similar@2.7.0` est signalé inutile par `cargo deny` depuis avant cette montée (dev seulement) : laissé tel quel.

### À surveiller

- **Retour à un binaire 1.1.x** sur un index reconstruit : tantivy 0.26 ignore la charge utile et ouvre l'index ; les racines redeviennent fausses, en silence, sans garde possible. Ne repassez pas à une version antérieure sur un index reconstruit.
- La marque n'est lue qu'à l'ouverture : un index remplacé sous un serveur en marche n'est pas couvert (déjà déconseillé : ne lancez pas `build-dila-index` pendant qu'un client lit le même index).
- Un registre gardé avec un index vidé fait répondre « à jour » à `build-dila-index` sur un index vide (comportement antérieur) : supprimez le dossier entier.
- **Mesuré le 2026-11-11, de bout en bout, sur les mini-archives du dépôt** (Linux, binaires de développement, sans réseau ni PISTE) : un index bâti par le binaire 1.1.0 installé (`tantivy` 0.26.2 ; 23 documents, 2 segments, charge utile de commit absente) est ouvert par le binaire de la branche sans erreur de format, puis refusé par la marque. `build-dila-index` sort en code 1 avec le message de la décision 5 et laisse le dossier identique (`diff -r` vide). `serve` avec `LF_DILA_DIR` sur ce dossier répond `[config_error]` avec le texte fixe de la décision 4 à `fr_search`. Après suppression du dossier, `build-dila-index` le reconstruit (23 documents, marque `fr_legi-1`), le relancer répond « à jour », et `serve` répond à `fr_search`.
- **NON VÉRIFIÉ** : classement et rappel sur de vrais dumps ; reconstruction sur les archives réelles avec le binaire neuf.
- MSRV : 1.88 pour `tantivy` 0.27.0, sans marge.

### Tests qui la gardent

- `index.rs` : marque posée à chaque commit (`a_commit_stamps_the_index`, `every_kind_of_commit_keeps_the_stamp`), table de vérité (`only_the_current_stamp_or_an_empty_index_is_accepted`), ouverture sur disque (index neuf, marque courante, sans marque, autre marque, lecture des métadonnées en échec), `french_stems_are_pinned`.
- `error.rs` : texte exact de `OpenError`. `corpus.rs` : `DilaCorpus::open` refuse un index d'une autre marque.
- `wiring.rs` : `a_stale_index_is_unavailable_and_asks_for_a_rebuild`. `dila_index.rs` : `build_refuses_a_stale_index_and_changes_nothing`, `build_into_a_new_directory_works_after_a_stale_one`.
- `make deny`, `make min-age`, `make lock` (le verrou des descripteurs ne change pas).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Ne rien garder (montée seule) | Résultats faux en silence sur les mots touchés. |
| Fichier témoin à côté de l'index | Peut survivre à une suppression partielle ou être copié seul ; la charge utile de commit est écrite avec le commit. |
| Empreinte de l'analyseur calculée à l'exécution | Plus de code, opaque ; la marque entretenue à la main avec un test de racines suffit. |
| Contrôle à chaque requête | Coût sans gain : la marque ne change que si le dossier est remplacé. |
| Reconstruction ou suppression automatique, option `--rebuild` | Le code ne détruit jamais de données de l'utilisateur ; le serveur n'a pas les archives ; un dossier mal désigné serait détruit. Suite possible. |
| Texte générique élargi (sans nommer la commande) | Le modèle doit pouvoir dire quoi faire. |

## Références et sources

- [00-decisions.md](../plan/00-decisions.md) §14 (AM-91) ; [ADR-0018](0018-dependances-fin-de-projet.md) ; [ADR-0019](0019-dilacorpus-hors-ligne.md) ; [02-architecture.md](../plan/02-architecture.md) §8.5 ; [README](../../README.md) « Mode hors ligne ».
- Sources de `tantivy` 0.27.0 : `prepared_commit.rs` (`set_payload`), `index_meta.rs` (`payload`, « entirely unused by tantivy »), `segment_updater.rs` (la fusion reporte la charge utile). CHANGELOG amont de `tantivy` 0.27.0 (remplacement de `rust-stemmers` par `frostem`) ; NEWS de Snowball 3.0.0 et 3.1.0 (français).
