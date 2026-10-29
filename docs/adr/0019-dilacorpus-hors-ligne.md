# ADR-0019 — `DilaCorpus` : le port `LegalCorpus` hors ligne, sur un index tantivy des dumps LEGI

- **Statut** : **Proposé** — accepté à la fusion de la PR unique du plan fin-du-projet.
- **Date** : 2026-10-29
- **Jalons concernés** : M9 (`lf-adapter-dila`, tâches T-M9-02 et T-M9-03), câblage dans `lf-app`
- **Questions liées** : Q6 (accès réseau à `echanges.dila.gouv.fr`, proposition adoptée par défaut, à confirmer)
- Complète [ADR-0004](0004-piste-puis-dila.md) (PISTE d'abord, DILA derrière le même port) et [ADR-0018](0018-dependances-fin-de-projet.md) (dépendances, `fs4` en deux versions). Ne modifie aucune décision de [00-decisions.md](../plan/00-decisions.md) : elle consigne ce que M9 a livré de 00 §5 (« M9 : LEGI seul »).

## Contexte

- **Les dumps de la DILA portent le fonds LEGI seul** : les codes et les textes consolidés (lois, décrets, arrêtés). Ni jurisprudence, ni délibérations CNIL, ni conventions collectives (KALI), ni accords (ACCO), ni ELI (00 §5).
- **Les dumps sont des archives `tar.gz`** : un global, puis des incréments par ordre chronologique, avec des listes de suppression (`.dat`). Le global vide le corpus.
- **Le port est déjà fixé** : `LegalCorpus` (00 §3, ADR-0012). `DilaCorpus` n'ajoute aucune méthode au port ; il implémente celles que le fonds permet.
- **Le binaire ne télécharge rien** : les dumps sont posés par script, hors binaire (plan fin-du-projet). Q6 (le réseau de construction atteint-il `echanges.dila.gouv.fr` ?) n'est donc pas un obstacle d'exécution, seulement de construction de l'index.
- **Non vérifié** : la volumétrie et le temps de construction sur de vrais dumps (R-16, V-185 à V-188), la licence par fonds ([V-189 · dila-licence-par-fonds]) et le coût de la synchronisation ([V-166 · cout-sync-data]). Les tests de l'adapter tournent sur des fixtures.

## Décision

1. **Huit opérations sont servies hors ligne** : `search` (fonds `CODE` et `LODA`), `get_act`, `get_article`, `find_article`, `find_by_number` (numéro d'article, numéro de texte, NOR), `list_codes`, `article_versions` et `related_links`.
2. **Toute autre opération est refusée par `CorpusError::Config` de nature `Settings`, avec un texte fixe** : « `<opération>` is not available in the local DILA corpus, which holds the LEGI fund only (codes, laws and decrees); use the PISTE source for it ». `<opération>` est une constante du code, jamais une valeur de l'appelant. Une lecture d'index en échec rend aussi un texte fixe (« the local DILA index could not be read; rebuild it from the dumps ») ; la cause va au journal, jamais un chemin du disque au modèle.
3. **Ingestion en flux, sans rien extraire sur le disque** (`flate2` → `tar` → `quick-xml`). Les chemins sont validés (pas de `..`, pas de chemin absolu, 512 octets au plus), les liens ne sont jamais suivis, un membre pèse 16 Mio au plus, `DOCTYPE` et `ENTITY` sont refusés. Un membre défectueux est compté dans le rapport sans arrêter l'archive ; une archive illisible arrête la synchronisation. Aucune erreur ne cite le contenu d'un membre.
4. **Une archive n'est « appliquée » qu'une fois durable.** `ingest_archive` applique les suppressions **après** tout le contenu, puis appelle `DocStore::commit` en dernier ; `sync_directory` le rappelle (utile pour une archive vide) avant d'inscrire l'archive au registre, puis enregistre le registre. Un global est inscrit comme « en cours » et enregistré **avant** que le magasin ne soit vidé : une coupure laisse un registre qui impose de refaire le corpus, jamais un registre qui déclare appliqué ce que l'index n'a pas.
5. **Recherche plein texte français sur un index tantivy** (analyseur français : minuscules, pliage des accents, mots vides, racinisation ; `MmapDirectory` ; compression des documents en lz4, ADR-0018 §6). Les lectures passent par `spawn_blocking`. Les résultats sont les **textes** (un `LEGITEXT` par résultat), classés par leur meilleur document ; pour un code, chaque résultat cite ses articles trouvés.
6. **Plafond de 2 000 candidats.** Le classement porte sur les 2 000 meilleurs documents (`CANDIDATES`, `corpus.rs`) ; `total` compte les textes de cet ensemble et n'est donc qu'**une borne basse** quand la requête en trouve davantage. La période d'une requête `LODA` s'applique à la date de signature du texte ; celle d'un numéro est ignorée.
7. **`fs4` en deux versions (D21).** `tantivy` tire `fs4 ^0.13.1` ; `lf-adapter-audit` prend `fs4` 1.1. `deny.toml` passe `multiple-versions` à `deny` et porte `fs4@0.13.1` en `skip` nominatif ; `lf-adapter-audit` et `tantivy` sont les deux seuls `wrappers` de `fs4` (ADR-0018 §5).
8. **Choix de la source au démarrage, jamais de repli.** `LF_DILA_DIR` désigne un index déjà bâti ; défini, il remplace PISTE (feature `dila`, absente des binaires publiés). Un répertoire absent, un index vide, un index illisible ou une feature absente rendent la source indisponible : aucune requête ne part sur le réseau quand l'utilisateur a choisi l'hors-ligne.
9. **Un instantané ne prouve pas l'absence.** La source locale se déclare `production` dans l'audit et dans `fr_coverage`, avec le drapeau `local_snapshot` : `fr_verify_citations` et `fr_resolve` rendent une absence `unresolvable`, jamais `not_found`, et `fr_coverage` la décrit comme un instantané des dumps LEGI (statut `degraded`, lacune FR-105, familles LODA et CODE seules) (02 §8.5).

## Conséquences

### Positives

- Les huit opérations LEGI marchent sans réseau ni identifiants, avec le même port, les mêmes vues et le même journal d'audit que PISTE.
- Un refus d'opération hors LEGI est lisible par le modèle et orienté (« use the PISTE source »), sans fuite de chemin.
- La durabilité est ordonnée (contenu, suppressions, commit, registre) : un redémarrage après coupure reprend sur un état connu.

### Négatives

- **`total` n'est pas comparable à celui de PISTE** : borne basse à 2 000 documents, non le total du fonds.
- **Le fonds est incomplet par construction** : les familles hors LEGI sont refusées, et un index périmé sert des versions anciennes sans le dire autrement que par `local_snapshot`.
- **tantivy se compile** dans toute construction avec `--all-features` (ADR-0018).

### À surveiller

- Volumétrie, temps de construction et taille de l'index sur de vrais dumps ; licence par fonds ([V-189 · dila-licence-par-fonds]) ; coût de la synchronisation ([V-166 · cout-sync-data]) : **NON VÉRIFIÉ**.
- Si un usage réel dépasse 2 000 candidats (requêtes très larges), relever le plafond ou paginer l'index ; décision à prendre sur une mesure.
- L'ajout d'un fonds (JURI, CNIL, KALI) change la décision 1 et appelle un nouvel ADR.

### Tests qui la gardent

`unsupported_operations_are_settings_errors_the_model_can_read` (`map.rs`), `every_failed_read_reaches_the_model_as_a_fixed_text` (`corpus.rs`), `the_store_is_committed_once_after_the_content_and_the_deletions` (`ingest.rs`), `an_archive_is_applied_only_once_the_store_has_made_it_durable` (`sync.rs`), `a_dila_dir_without_the_feature_is_unusable_and_never_falls_back_to_piste` et `an_unusable_dila_dir_is_a_problem_and_never_falls_back_to_piste` (`lf-app/src/config.rs`), `dila_feature_selects_dila_corpus` (`lf-app/src/wiring.rs`) ; `cargo deny check` (`fs4`) et `dila_member_has_a_rule` (`xtask/src/arch.rs`).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Repli sur PISTE quand l'index local est inutilisable | Une requête partirait sur le réseau contre le choix explicite de l'utilisateur ; l'indisponibilité est dite au modèle. |
| Rendre `NotFound` pour les opérations hors LEGI | Ferait croire qu'un texte de jurisprudence n'existe pas ; `Config` dit que la source ne le couvre pas. |
| Télécharger les dumps dans le binaire | Hors périmètre (plan fin-du-projet) : un script les pose ; le binaire ne contacte pas la DILA. |
| Extraire les archives sur le disque avant lecture | Surface d'attaque (chemins, liens) et volume ; la lecture en flux n'écrit que dans le magasin et le registre. |
| Un index sans plafond de candidats | Mémoire et latence non bornées sur les requêtes larges ; le plafond rend le `total` honnête (borne basse). |
| Écrire sa propre recherche plein texte | Tantivy fournit l'indexation, le classement et les filtres de langue ; la refaire à la main n'apporte rien. |

## Références et sources

- Plan : `docs/superpowers/plans/2026-10-29-fin-du-projet-parallele.md` (D9, D21) ; [00-decisions.md](../plan/00-decisions.md) §3, §5 ; [02-architecture.md](../plan/02-architecture.md) §8.5 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) R-16, V-166, V-185 à V-189, Q6.
- Code : `crates/lf-adapter-dila/src/{corpus,map,ingest,sync,registry,store,index}.rs` ; `crates/lf-app/src/{config,wiring}.rs` ; `deny.toml`.
