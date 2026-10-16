# ADR-0008 — Audit obligatoire en décorateur, strict par défaut

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M3 (`Audited`, `lf-adapter-audit`), M4 (décodage audité dans `lf-adapter-mcp`), M7 (lectures de resources)
- **Questions liées** : Q9 (politique si le disque est plein ; proposition `strict` adoptée par défaut, à confirmer avant M3). Spike M0 V-28 (`JsonObject`).
- Révisé le 2026-10-15 (revue) : cas limites et écriture multi-processus de 00 §7 (AM-30) : écriture sous verrou ligne par ligne, empreinte non salée non compatible avec la référence, prompts et completions non audités ; décompte des 15 issues précisé ; décodage `JsonObject` conforme à 00 §5 (AM-08).

## Contexte

- **Exigence de la référence.** La CONSTITUTION, art. 2, dit : « Every tool call MUST append one JSON line … Inability to write = the tool returns an error, it does not silently skip » (`CONSTITUTION.md:23-27`). La référence justifie son journal par l'article 12 de l'AI Act (`audit.py:1`). Ce plan reprend l'exigence technique sans se prononcer sur la conformité juridique (06 §11).
- **Ce que fait réellement la référence** (recherche 02 §2.b ; recherche 01 §2.6, matrice S1 à S17) :
  - D-02 et E-02 (bloquant) : `invalid_arg` est levé avant tout `audit.log` (`server.py:191-197`…) ; le `not_found` de normalisation n'est pas audité (`:256-258`) ; les erreurs pydantic non plus ;
  - D-03 et E-03 : `duration_ms` est lu dans le `with timer()` avant `__exit__`, donc toujours 0 en erreur (`audit.py:81-90`) ;
  - D-07 : 8 instanciations d'`AuditLogger`, 15 appels `audit.log`, logique recopiée par outil ;
  - D-16 : ouverture et fermeture du fichier à chaque ligne, sans `fsync`, sans rotation ni verrou ; D-38 : écriture synchrone dans la boucle async ;
  - D-17 : SHA-256 non salé, sans code d'erreur ni version.
- **Piège de rmcp 3.5.1.** Un échec de désérialisation de `Parameters<T>` devient un résultat d'erreur in-band produit **par le routeur**, donc hors de tout décorateur applicatif (`router/tool.rs:144-156`, CHANGELOG 3.5.1 #1322, 02 §13.1 n° 13). La recherche 01 (E-29) supposait une erreur JSON-RPC.

## Décision

1. **`Audited`** vit dans `lf-application`. Il enveloppe l'**exécution complète** de chaque cas d'usage : décodage des arguments, validation du domaine, appels au port, politique de budget et délai global (`LF_TOOL_DEADLINE_S`).
   - Il écrit **une ligne par issue** : succès, `invalid_arg`, `not_found`, erreurs amont, quota, configuration, délai, annulation. La table de 06 §2.2 compte 15 issues : 12 écrivent une ligne, 3 (échec d'écriture du journal, modes `strict` et `best-effort`) n'en écrivent aucune.
   - La durée est réelle, mesurée par `tokio::time::Instant` (00 §3, 02 §7.1).
2. **Décodage audité.** `lf-adapter-mcp` reçoit les arguments en `JsonObject` et publie un `input_schema` explicite (`schema_for_input::<P>()`, pas `Parameters<T>`), décodé par `serde_path_to_error`. Une erreur de décodage devient `InputError`, rendue `[invalid_arg] chemin: motif` et auditée ([ADR-0010](0010-erreurs-in-band.md), 00 §5). `Parameters<T>` ne sert qu'aux prompts.
3. **Port** `AuditLog::record(&AuditRecord) -> Result<(), AuditError>`, implémenté par `JsonlAuditLog` dans `lf-adapter-audit` (00 §7, 06 §11.5) :
   - JSONL en ajout seul ; aucune E/S disque avant le premier `record()` ;
   - fil d'écriture dédié, alimenté par un canal borné, avec accusé `oneshot` envoyé **après** `sync_data` ;
   - pour **chaque ligne** : verrou consultatif `fs4` sur `.mcp-legifrance.lock`, ouverture en ajout, `write_all`, `sync_data`, fermeture ; aucun descripteur ne reste ouvert entre deux lignes, si bien que plusieurs processus (sonde et session de Claude Code, VS Code) écrivent le même répertoire et que Windows peut renommer le fichier (V-164) ;
   - rotation à 16 Mio ou au changement de date UTC, **sous le même verrou** ; aucune suppression automatique ;
   - répertoire `LF_AUDIT_DIR` (défaut : répertoire de données de la plateforme) en `0700`, fichiers en `0600`.
4. **Format de ligne** (00 §7, 06 §11.2), clés dans cet ordre : `ts`, `tool`, `input_hash`, `output_count_or_size`, `duration_ms`, `status`, `error_code?`, `error?` (≤ 500 caractères, sans secret), `env`, `server_version`.
   - `input_hash` est le SHA-256 **non salé** de la forme canonique « Python » (clés triées, JSON compact, `ensure_ascii`) de l'objet `arguments` **brut**, calculé même quand le décodage échoue.
   - Il n'est pas compatible, en général, avec les empreintes de la référence, qui hachait une charge partiellement normalisée (P-041 documenté, annexe A E-32). Les 8 vecteurs P-041 servent à tester la forme canonique (annexe A §6.6 ; recherche 01 §8).
   - Métrique `output_count_or_size` par outil : 00 §7 ; 0 en erreur.
5. **Mode** `LF_AUDIT_MODE` :
   - `strict` (défaut, Q9) : un échec d'écriture rend `[audit_error]`. Le résultat est retenu, et le code d'origine est cité s'il y en avait un ;
   - `best-effort` : le résultat est rendu, avec `warn!` et un compteur.
6. **Annulation explicite** : `ct.cancelled()` est passé à la façade, `select!` est biaisé, et la ligne est écrite avec `status: error`, sans `error_code`, et `error = "cancelled by client"` (00 §7). Il n'y a pas de garde `Drop` (02 §7.1).
7. **Hors audit** : erreurs de protocole (aucune exécution), prompts et completions (aucun cas d'usage d'outil exécuté). Les lectures de resources (M7) sont auditées sous `tool: resource:<nom du gabarit>`, avec pour métrique les caractères renvoyés (00 §7).

## Conséquences

### Positives

- Aucune issue n'échappe au journal (D-02), et les durées sont exploitables (D-03).
- Un seul point de passage (D-07) : un outil ajouté est audité sans code supplémentaire.
- Une ligne acquittée survit à un arrêt brutal. Plusieurs clients (Claude Code, VS Code) peuvent partager le répertoire.

### Négatives

- **En mode `strict`, un disque plein ou un répertoire inutilisable rend tous les outils indisponibles.** C'est voulu par l'art. 2 de la référence, mais c'est un risque d'exploitation (09, risque « audit strict ») ; `best-effort` existe pour ce cas.
- Un `sync_data` par appel : le coût selon le système de fichiers est **NON VÉRIFIÉ** [v:cout-sync-data].
- **Empreinte non salée** : elle est réversible par énumération pour des entrées courtes (D-17). C'est le choix de 00 §7 (AM-30), qui ne garantit pas pour autant l'égalité avec les empreintes de la référence (P-041). Le `request_id` et l'état du cache restent dans `tracing`. Un sel ou un HMAC exige un **nouvel ADR**, réexaminé après v0.1.0 (00 §7, 02 §13.1 n° 6).
- Une ouverture et un `sync_data` par ligne, sous verrou : coût accepté devant un appel PISTE (06 §11.5).

### À surveiller

- Points **NON VÉRIFIÉS** :
  - [v:audit-multi-processus] : atomicité multi-processus, surtout sous Windows ;
  - [v:fs4-verrou-api] : API exacte de `fs4` 1.1 ;
  - [v:chemin-donnees-plateforme] : chemins par défaut selon l'OS ;
  - [v:audit-acl-windows] : ACL du répertoire sous Windows ;
  - [v:rmcp-jsonobject-input-schema] : `JsonObject` et `input_schema` explicite acceptés par rmcp, à valider au spike M0.
- Si le spike invalide `JsonObject`, le repli `Parameters<T>` exige une autre façon d'auditer les erreurs de décodage : il faut un amendement par un nouvel ADR.

### Tests qui la gardent

- `audit_written_for_every_outcome` (15 issues de 06 §2.2 : 12 avec une ligne, 3 sans ligne en cas d'échec d'écriture), `error_duration_is_nonzero`, `cancelled_call_is_audited`, `input_hash_of_invalid_arguments_is_computed`, `audit_metric_per_tool`, `resource_read_is_audited` (M7).
- `audit_failure_returns_audit_error`, `best_effort_keeps_result`, `audit_failure_does_not_mask_original_cause`.
- `every_tool_goes_through_audited`, `record_returns_only_after_sync`, `two_processes_share_audit_file`.
- `audit_line_validates_against_schema` et vecteurs P-041 (06 §3.1, §11).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Journalisation dans chaque outil (référence) | Cause racine de D-02, D-03 et D-07. |
| Décorateur du **port** `LegalCorpus` | Ne voit ni `invalid_arg` ni les erreurs de décodage, qui n'appellent jamais le port (02 §7.1). |
| Audit via `tracing` | Ni accusé ni durabilité. Le mode `strict` serait impossible. |
| Garde `Drop` qui écrit à l'abandon | Ne peut pas attendre l'écriture, donc non déterministe (02 §13). |
| `Parameters<T>` de rmcp | Les erreurs de décodage seraient in-band mais non auditées (`router/tool.rs:144-156`). |
| HMAC salé, `request_id` et état du cache dans la ligne (recherche 02 D-17) | Alourdit la ligne et ses tests ; 00 §7 retient l'empreinte non salée en v0.1. Sel ou HMAC réexaminé par ADR après v0.1.0. |
| Fichier ouvert en permanence par le fil d'écriture, renommé pour la rotation (version initiale de 02 §3.5) | Incompatible avec plusieurs processus sur le même répertoire et avec Windows, qui refuse de renommer un fichier ouvert (00 §7, AM-30). |
| `best-effort` par défaut | Contraire à l'art. 2 de la référence et à Q9 (proposition `strict`). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §1, §3, §7, §13 (Q9) ; [02-architecture.md](../plan/02-architecture.md) §3.5, §5.2, §7.1, §9.2 (lignes 21 à 23), §13, §13.1 ; [06-qualite.md](../plan/06-qualite.md) §3.1, §11 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §3.3 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §4.8.
- Recherche : [02-defauts.md](../recherche/02-defauts.md) §2.b, D-02, D-03, D-07, D-16, D-17, D-38 ; [01-parite.md](../recherche/01-parite.md) §2.6, §8, E-02, E-03, E-29, P-040 à P-043 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §2.5.
- Référence Python (`d3b5b13`) : `CONSTITUTION.md:23-27` ; `src/fr_eli_mcp/audit.py:1-94` ; `src/fr_eli_mcp/server.py:191-262`.
