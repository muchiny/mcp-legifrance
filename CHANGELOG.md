# Journal des modifications

Format : [Keep a Changelog 1.1.0](https://keepachangelog.com/fr/1.1.0/) et [SemVer](https://semver.org/lang/fr/). Avant 1.0, une rupture ou un ajout donne `0.(x+1).0` (`docs/plan/05-outils-mcp.md` §9.5).

Rubriques (`docs/plan/06-qualite.md` §13.3) : `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`, plus :

- **Tool surface** : obligatoire dès que le verrou des descripteurs change.
- **Parity** : clé de `docs/plan/annexes/A-parite.md` §4.
- **Protocol** : versions MCP, ères, montée de rmcp.

`[Unreleased]` reste en tête. Chaque version portera sa date ISO et un lien de comparaison. Rien n'est publié avant 0.1.0 (M5).

**État au 2026-10-29 (D14).** La version du workspace est `0.3.0`, mais aucune version, aucune étiquette et aucun binaire n'ont été publiés : les versions 0.1.0 et 0.2.0 n'ont jamais existé hors du dépôt. Les entrées de M0 à M9 sont donc réunies sous `[0.3.0] - non publiée`. Au moment d'étiqueter, le mainteneur remplace « non publiée » par la date ISO (le contrôle de `release-gates.yml` exige `## [X.Y.Z] - AAAA-MM-JJ`) et ajoute le lien de comparaison.

## [Unreleased]

Rien depuis `0.3.0`.

## [0.3.0] - non publiée

### Tool surface

Verrou des descripteurs `tests/tools.lock.json` (format 1, version du serveur `0.3.0`), régénéré par `cargo xtask lock-descriptors --update` (`make lock`) ; les trois ères sont identiques.

- **13 outils** : `fr_search`, `fr_search_all`, `fr_get_text`, `fr_get_act`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_get_company_agreement`, `fr_get_related`, `fr_diff_article`, `fr_resolve`, `fr_verify_citations` et `fr_coverage`.
- **1 ressource** `legifrance://coverage` et **4 modèles de ressource** : `legifrance://act/{text_id}`, `legifrance://article/{article_id}`, `legifrance://code/{code}/article/{num}` et `legifrance://decision/{decision_id}` (la variable `code` se complète).
- **3 prompts** : `analyser_decision`, `etat_du_droit` et `verifier_citations`.
- **Capacités** : `tools`, `resources`, `prompts` et `completions`.
- **Tailles mesurées** : instructions du serveur 3 278 caractères ; `tools/list` 31 853 octets, sous le plafond de 32 900 octets.
- **Descriptions** : les exemples de `fr_get_text`, `fr_resolve` et `fr_verify_citations` s'écrivent « for example » (aucun point dans « e.g. », que le comptage de phrases lisait comme une fin de phrase) ; `fr_get_related` ajoute à sa sortie le champ optionnel `note` (voir Added).

### Added

- M0 : workspace et lints, licence Apache-2.0 et `NOTICE`.
- M0 : portes de `make ci` et mutation fichier par fichier.
- M0 : CI `ci.yml` et `weekly.yml`.
- M1 : crate `lf-domain` et ses modules ; cibles de fuzz `ids` et `text`.
- Newtypes `CitationText`, `SourceUrl` et `Eli`.
- Contrat de citation v2 `CitationContract { citation, source_url, ecli, eli }` et `ELI_NOTE`.
- Table des matières en arbre : `Toc`, `TocNode`, `TocSection`, `TocArticle`, `SectionId`, pagination `Toc::page`, erreurs `TocTooDeep` (profondeur supérieure à 64) et `TocOffsetPastEnd`.
- `parse_versioned`, `VersionedId`, erreur `BadSuffix`, champ `SearchHit.version_start`.
- `SENTINEL_FLOOR`, `open_ended`, `VersionDate` borné à `1500-01-01..=2221-12-31`, `DateRange`, erreur `InvertedDateRange`.
- `Hash` sur les identifiants, les dates, `Ecli` et `Fond` ; `Display` et `FromStr` manquants.
- `text::encoding` : `repair_latin1_mojibake` et `has_latin1_mojibake`.
- Cible de fuzz `version_date`, cibles `ids` et `text` étendues, 12 doc-tests.
- `SECURITY.md`, `PRIVACY.md`, modèle de PR et ce journal.
- Workflows `deny-nightly.yml` et `workflow-lint.yml` (zizmor, actionlint), `dependabot.yml`.
- `make min-age` et son auto-test.
- Profils nextest, matrice Linux, macOS et Windows du job `test`, `.gitattributes`.
- `lf_domain::search` : `QueryText`, `PageSize`, `MatchMode`, `SearchQuery`, `has_more`, `REACHABLE_RESULTS` ; `Fond::needs_version_date`, `Fond::has_date_filter` ; erreurs `EmptyQuery`, `QueryTooLong`, `OutOfRange`, `VersionDateNotApplicable`, `DateFilterNotApplicable`, `PageBeyondReach`.
- Crate `lf-application` : port `LegalCorpus`, `CorpusError`, `Deadline`.
- `make purity` (`cargo check -p lf-application`).
- `lf_domain::coverage::PisteEnvironment`, seul type d'environnement du workspace.
- Crate `lf-adapter-piste` : configuration validée, client HTTP unique à corps borné (16 Mio après décompression), jeton OAuth 2.0 `client_credentials` mono-vol, classification, réessais par boucle maison (3 tentatives, 2 sur 500, `Retry-After` ; ADR-0017), limiteur de débit et de concurrence, `CallDeadline`, UTF-8 strict et mojibake (`StatsSnapshot`), `PisteCorpus` (appel API, mode dégradé, `check_credentials`).
- `check-arch` : R6 (crates confinés) et R8 (dev-dependencies).
- `spikes/tls-provider` et `tls-spike.yml` (matrice TLS à déclencher à la main).
- `make audit-fuzz` (`cargo-audit` sur `fuzz/Cargo.lock`).
- `lf_domain::fond::PaginationMode` et `SearchPage.pagination`.
- Adapter PISTE : `PisteTuning`, `TuningError` et `PisteConfig::with_tuning`.
- Adapter PISTE : lecteurs serde tolérants sans tampon, `dto::decode::decode` (chemin JSON, expurgation, 127 imbrications au plus, soit 62 ou 63 niveaux de sections) et `PisteErrorDto`.
- Adapter PISTE : DTO de `/search`, `lawDecree`, `kaliText`, `getArticle`, `juri`, `cnil` et `acco`.
- Adapter PISTE : `request::search::search_body` (payload, facettes, `DATE_VERSION` à midi Europe/Paris, gardes) et `PaginationMemo`.
- Adapter PISTE : classification « introuvable » : un 400 de consultation au marqueur « introuvable », un 404 de consultation sur les deux adresses officielles (corps non HTML) et un 2xx vide d'une consultation donnent `NotFound` (un 2xx vide de `/search` donne `Decode`) ; message PISTE nettoyé d'un 400 de consultation, jeton et secret client retirés ; corps d'erreur plafonné à 64 Kio.
- Dépendances `serde_path_to_error` 0.1.20 et `chrono-tz` 0.10.4 (binaire +1,86 Mo).
- Domaine (plan 2e) : `LegalStatus` et `status_of`, `Nor`, `Idcc`, `DocumentId`, `Fond::hit_prefix`, erreurs `HitKindMismatch` et `TextTooLarge`.
- Domaine (plan 2e) : lexer HTML partagé (`text/html.rs`), `clean_with_report`, `clean_field`, `CleanReport`, `LongText` (`from_html`, `from_plain`, `best_of`, faillibles), cible de fuzz `from_html`.
- Domaine (plan 2e) : `Snippet`, `SearchHit::new`, `SearchPage.total_articles` et `dropped`, `OfficialSummary`, `ParentText` et `ParentNature`, `article_citation_for` (appel de note final retiré pour un texte non codifié, Q12 par défaut).
- Domaine (plan 2e) : `Decision.official_summary` ; `Act` et `Convention` complets (statut, NOR, texte en ligne ; IDCC, conteneur, bulletin) ; les 16 fixtures de la référence et leur golden installés dans `lf-adapter-piste/tests/fixtures/reference/`.
- Application (plan 3a), socle sans cas d'usage : ports `AuditLog` (`AuditRecord`, `AuditStatus`, `AuditError`, `AuditErrorKind`) et `Clock`, ré-export de `PisteEnvironment` ; `ToolCall`, `CanonicalInput`, `InputError` ; `ToolName` (8 outils v0.1) ; `ErrorCode`, `UpstreamCause`, `AppError` ; module `messages`, seule source des textes vus par le modèle (`docs/plan/05-outils-mcp.md` §7.2 à §7.4), qui ne lit le `detail` d'une `CorpusError` que pour `InvalidRequest` et `Config { kind: Settings }` ; décorateur `Audited` (`AuditMode`, `AuditPolicy`, `CallMeta`, `AuditMetric`) : délai global, échéance publiée dans `Deadline`, annulation prioritaire, une ligne d'audit acquittée par issue, modes `Strict` (résultat retenu) et `BestEffort` (compteur d'échecs d'écriture), span `tool_call` ; doublures de la feature `test-support` (`FakeCorpus`, `FixedClock`, `MemoryAuditLog`, `FailingAuditLog`, `NO_DEADLINE`). Dépendances de `lf-application` : `chrono`, `tracing`, feature `macros` de `tokio`.
- `check-arch` (plan 3a) : la feature `test-support` de `lf-application` n'est admise qu'en dépendance de développement ; `tracing` borné à `std` dans `lf-application`.
- Application (plan 3a, revue finale) : `failure::port_failure`, point d'entrée des cas d'usage pour une `CorpusError` (événement `corpus call failed` avec la variante, l'outil et le `detail`, jamais l'identifiant demandé ; puis `messages::corpus_error`) ; champ `cause` du span `tool_call` ; `FakeCorpus::seen_requests` (`PortRequest`, arguments de chaque appel) et `FakeCorpus::unserved`.
- `check-arch` (plan 3a, revue finale) : chez `lf-application`, aucune feature autre que `test-support` (`default`, `full`…) ne peut lister `test-support`.
- M2 à M7 : surface de lecture et de recherche (`fr_search`, `fr_search_all`, `fr_get_text`, `fr_get_act`, `fr_get_decision`, `fr_get_deliberation`, `fr_get_convention`, `fr_get_company_agreement`, `fr_diff_article`, `fr_coverage`), adapter PISTE, cache en mémoire (`lf-adapter-cache`), audit en décorateur (`lf-adapter-audit`), serveur MCP (`lf-adapter-mcp`) et binaire `mcp-legifrance` (`lf-app`), transport stdio.
- M6 : parseur de citations de `lf-domain` (ancres, arbitrage, formes canoniques, extrait cité) et outils `fr_verify_citations` et `fr_resolve` ; corpus de l'annexe B et fuzz.
- M7 : outil `fr_get_related` (liens d'un article ou d'une décision), resources, prompts et completions.
- M9 : crate `lf-adapter-dila` (ingestion en flux des dumps LEGI, registre des archives, index tantivy) et port `DilaCorpus`, huit opérations servies hors ligne ; source choisie par `LF_DILA_DIR` derrière la feature `dila` (ADR-0019). Absente des binaires publiés.
- M8 : sous-commande `serve-http` (Streamable HTTP sans session, profil local) derrière la feature `http`, absente des binaires publiés ; job de conformité HTTP.
- `fr_get_related` : le champ optionnel `note` dit, pour une décision, que l'API ne donne aucune référence (`NoDecisionReferences`) ; une liste vide ne prouve plus l'absence de liens.
- `ConfigFacts.local_snapshot` et lacune `FR-105` : un index local est un instantané ; `fr_verify_citations` et `fr_resolve` rendent `unresolvable`, jamais `not_found`, et `fr_coverage` décrit la source comme dégradée.
- Distribution : lanceur POSIX du bundle MCPB, image OCI distroless, `server.json` (`packaging/`), verrou et manifeste alignés sur `0.3.0`.
- Documentation : ADR-0019 (`DilaCorpus`).

### Changed

- Toolchain épinglée 1.99.0 (MSRV 1.88 inchangée) ; lints `clippy::panic`, `unreachable` et `unimplemented`.
- Variantes de `DomainError` porteuses d'une entrée fermées hors du crate ; `IdKind` scellé.
- `check-arch` par table de règles : membre inconnu, R3, R4, R5, R7, contrôles de manifeste.
- Module `citation` renommé `contract` ; `Citable::citation()` devient `Citable::contract()`.
- `legifrance_url` rend une `SourceUrl`.
- `constit_citation` sans paramètre `title`.
- `TocEntry` renommé `TocArticle` ; `TextBody.articles` remplacé par `TextBody.toc`.
- `CitationContract::from_url` réservé au crate.
- `--locked` sur toute commande cargo.
- `check-arch` : liste fermée et features de `lf-application` (R2), `[lints] workspace = true` (5e), fermeture transitive, doublons critiques (R9) ; graphe résolu complet.
- `deny.toml` : 7 licences (liste exacte du graphe), exception `NCSA` de `libfuzzer-sys`, wrappers `reqwest` et `governor`.
- `make deny` et les jobs `deny` couvrent aussi `fuzz/`.
- Adapter PISTE : un `Retry-After` supérieur à 30 s n'est pas attendu (échec immédiat) et la valeur rangée dans `RateLimited { retry_after }` est écrêtée à 1 h ; un hôte officiel écrit avec un point final (`api.piste.gouv.fr.`) est reconnu comme officiel (`EnvMismatch`, absence d'avertissement `CustomHost`).
- Workflow `tls-spike.yml` : `apt-get update` avant l'installation de `musl-tools`.
- Adapter PISTE : champs de `PisteEndpoints`, `PisteCredentials`, `RateQuota` et `PisteConfig` privés (accesseurs).
- Adapter PISTE : le `detail` d'un 400 de consultation porte le message PISTE nettoyé ; celui d'un 400 de `/search` reste générique (`HTTP 400 on /search`).
- Domaine (plan 2e) : `text`, `etat`, `idcc` et `nor` des entités typés (`LongText`, `LegalStatus`, `Idcc`, `Nor`) à la place de chaînes ; `CodeArticleRef.snippet` est un `Option<Snippet>` ; `clean` délègue à `clean_with_report`.
- `CorpusError` typé (plan 3a) : `Auth { origin: AuthOrigin, status, detail }`, `Config { kind: ConfigKind, detail }` et nouvelle variante `TooLarge` ; l'adapter PISTE rend `TooLarge` pour un corps au-delà de la borne sur `/search` et les consultations (`Decode` sur l'endpoint du jeton), et pose l'origine, le statut et la nature à chaque site.
- Adapter PISTE (plan 3a) : `ConfigProblem::InvalidEnv { value: RejectedEnv }`, construit par `ConfigProblem::invalid_env` (texte de `docs/plan/05-outils-mcp.md` §7.2 ; la valeur n'est citée que si elle fait 1 à 16 lettres ASCII, sinon le message ne donne que son nombre de caractères et la valeur brute n'est jamais conservée, pas même dans le `Debug`) ; `ConfigProblem::EnvMismatch` porte `variable` (`LF_PISTE_BASE_URL` ou `LF_PISTE_OAUTH_URL`) à la place de `setting`, et son texte devient celui de 05 §7.2.
- Version du workspace `0.3.0` (D14), `server.json` et manifeste MCPB alignés ; aucune étiquette n'est posée.
- `LegalCorpus::find_article` prend `(&LegiTextId, &str, ArticleAt)` avec `ArticleAt::{Today, On}` (D5) ; `ServiceSettings { policy, configuration, budget }` (D13).
- `cargo deny` : `multiple-versions = "deny"`, huit `skip` nominatifs et sans `skip-tree` (T-M5-09) ; `fs4` en deux versions (D21).
- `cargo machete` : plus aucune entrée `ignored` ni `STUB(V0)` (ADR-0018).
- Plafond de `tools/list` : 24 000 puis 33 500 puis 32 900 octets (mesure de 31 853 octets plus 3 %).
- CI : mutation du diff de chaque PR limitée aux fichiers à risque de `.github/mutants-scope.txt` (D7) ; couverture : seule exclusion nommée, `crates/lf-adapter-mcp/src/stdio.rs` (D6).
- Adapter PISTE : `list_codes` complète une pagination instable de la sandbox (union dédoublonnée, jusqu'à 3 parcours).

### Removed

- Contrat de citation v1 (`eli_uri`, `human_readable_citation`), `bracketed` et suffixe « - objet » des citations du Conseil constitutionnel.
- Adapter PISTE (plan 3a) : `act_too_large` et `TOO_LARGE_PREFIX` ; plus aucun appariement de texte sur le `detail` d'une erreur.

### Fixed

- Sonde live du scénario 3 : un article de loi rend `/codes/article_lc/` en sandbox (Q17) ; la sonde n'exige plus `/loda/article_lc/`.
- Un suffixe de version impossible (`_31-02-2023`, `_foo`) est refusé au lieu d'être retiré en silence.
- Les dates sentinelles (2999-01-01, 2222-02-22) se lisent comme « sans fin ».
- Tests de l'adapter PISTE : port « fermé » rendu déterministe (`ClosedPort`).
- Adapter PISTE : `get_act` sur un texte dont la réponse dépasse la borne de corps (Code du travail, 31,4 Mio) rend une erreur explicite qui oriente vers la lecture par article ou la recherche, au lieu de l'erreur opaque de lecture (V-96) ; depuis le plan 3a, l'erreur est `TooLarge` et le texte (`this text is too large for fr_get_act; read its articles with fr_get_text, or find them with fr_search fond=CODE.`, qui ne nomme que des outils exposés) est rédigé par `lf-application::messages`.
- Adapter PISTE : un 403 à corps HTML (page de la passerelle) n'est plus lu comme une application non souscrite ; il devient `InvalidRequest { status: Some(403), .. }` (V-81). Réserve : le 403 d'une application non souscrite n'a pas été observé (V-65).
- `QueryText` : longueur maximale ramenée de 500 à 450 caractères, la limite mesurée de PISTE (accepté à 450, erreur 500 à 499 ; V-81) ; limite mesurée en caractères (ASCII et accentués), l'intervalle 451-498 n'est pas sondé et 450 est la borne conservatrice.

### Security

- Actions GitHub épinglées par SHA, `yanked = "deny"`, bans `openssl`, `openssl-sys`, `native-tls` et `backoff`, cargo-deny chaque nuit.
- Règle d'âge de 14 jours des dépendances.
- Politique de signalement `SECURITY.md`.
- Adapter PISTE : le champ `error` d'un refus OAuth n'est cité dans le `detail` que s'il est l'un des six codes de la RFC 6749 §5.2 (liste blanche, plus aucun secret, `client_id` ni jeton renvoyé par une passerelle) ; `Debug` de `Payload`, `Attempt` et `RawResponse` écrit à la main, sans valeur de formulaire, corps, jeton ni URL ; un `access_token` qui n'est pas une valeur d'en-tête valide est refusé et jamais mis en cache ; `Retry-After` hostile écrêté à 1 h dans l'erreur ; test du corps lu au goutte-à-goutte, borné par le délai de la tentative.
- TLS rustls avec aws-lc, à titre provisoire (ADR-0013).
- Application (plan 3a, revue finale) : `Debug` de `CanonicalInput` (longueur en octets seule) et de `ToolCall` écrit à la main, sans argument ; champ `error` de la ligne d'audit neutralisé (contrôles, U+2028, U+2029 et formats bidi remplacés par une espace) ; message de décodage coupé à 500 caractères ; `Deadline::after` sature au lieu de paniquer sur un budget démesuré. Adapter PISTE : la valeur refusée de `LF_PISTE_ENV` n'est plus jamais gardée (citée seulement si elle fait 1 à 16 lettres ASCII, sinon son nombre de caractères).
- `rustls` 0.23.46, `rustls-webpki` 0.103.16 et `webpki-root-certs` 1.0.10 (RUSTSEC-2026-0346 à -0349, -0382, -0383) par l'exception `security-fix` de la règle d'âge, sans `ignore` dans `deny.toml`.
- `serve-http` refuse de démarrer sans `LF_MCP_HTTP_TOKEN`, n'écoute que la boucle locale et refuse tout `Origin` non autorisé ; la source locale ne se replie jamais sur PISTE ; les refus de `DilaCorpus` sont des textes fixes sans chemin.

### Protocol

- MCP `2026-07-28`, `2025-11-25` et `2025-06-18` servis par `rmcp =3.5.1` (ADR-0003). Annulation (spike 0A) : rmcp 3.5.1 jette la réponse d'un appel annulé ; à l'arrêt, le transport HTTP répond 500.
- Statut des sept issues rmcp suivies, relevé le 2026-10-29 (annexe D §9.1) : 4 ouvertes, 3 fermées ; présence des correctifs dans une version publiée non établie (**NON VÉRIFIÉ**).

### Parity

- Domaine seulement, visible par un client à partir de M4 : citation du Conseil constitutionnel sans « - objet » (P-035), plus de `eli_uri` (P-031, P-039), sentinelle de date omise (P-030).

[Unreleased]: https://github.com/muchiny/mcp-legifrance/commits/main
[0.3.0]: https://github.com/muchiny/mcp-legifrance/commits/main
