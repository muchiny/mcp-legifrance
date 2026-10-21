# Journal des modifications

Format : [Keep a Changelog 1.1.0](https://keepachangelog.com/fr/1.1.0/) et [SemVer](https://semver.org/lang/fr/). Avant 1.0, une rupture ou un ajout donne `0.(x+1).0` (`docs/plan/05-outils-mcp.md` §9.5).

Rubriques (`docs/plan/06-qualite.md` §13.3) : `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`, plus :

- **Tool surface** : obligatoire dès que le verrou des descripteurs change.
- **Parity** : clé de `docs/plan/annexes/A-parite.md` §4.
- **Protocol** : versions MCP, ères, montée de rmcp.

`[Unreleased]` reste en tête. Chaque version portera sa date ISO et un lien de comparaison. Rien n'est publié avant 0.1.0 (M5).

## [Unreleased]

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

### Removed

- Contrat de citation v1 (`eli_uri`, `human_readable_citation`), `bracketed` et suffixe « - objet » des citations du Conseil constitutionnel.

### Fixed

- Un suffixe de version impossible (`_31-02-2023`, `_foo`) est refusé au lieu d'être retiré en silence.
- Les dates sentinelles (2999-01-01, 2222-02-22) se lisent comme « sans fin ».

### Security

- Actions GitHub épinglées par SHA, `yanked = "deny"`, bans `openssl`, `openssl-sys`, `native-tls` et `backoff`, cargo-deny chaque nuit.
- Règle d'âge de 14 jours des dépendances.
- Politique de signalement `SECURITY.md`.
- Adapter PISTE : le champ `error` d'un refus OAuth n'est cité dans le `detail` que s'il est l'un des six codes de la RFC 6749 §5.2 (liste blanche, plus aucun secret, `client_id` ni jeton renvoyé par une passerelle) ; `Debug` de `Payload`, `Attempt` et `RawResponse` écrit à la main, sans valeur de formulaire, corps, jeton ni URL ; un `access_token` qui n'est pas une valeur d'en-tête valide est refusé et jamais mis en cache ; `Retry-After` hostile écrêté à 1 h dans l'erreur ; test du corps lu au goutte-à-goutte, borné par le délai de la tentative.
- TLS rustls avec aws-lc, à titre provisoire (ADR-0013).
- `rustls` 0.23.46, `rustls-webpki` 0.103.16 et `webpki-root-certs` 1.0.10 (RUSTSEC-2026-0346 à -0349, -0382, -0383) par l'exception `security-fix` de la règle d'âge, sans `ignore` dans `deny.toml`.

### Parity

- Domaine seulement, visible par un client à partir de M4 : citation du Conseil constitutionnel sans « - objet » (P-035), plus de `eli_uri` (P-031, P-039), sentinelle de date omise (P-030).

[Unreleased]: https://github.com/muchiny/mcp-legifrance/commits/main
