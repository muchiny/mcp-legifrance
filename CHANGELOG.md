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

### Removed

- Contrat de citation v1 (`eli_uri`, `human_readable_citation`), `bracketed` et suffixe « - objet » des citations du Conseil constitutionnel.

### Fixed

- Un suffixe de version impossible (`_31-02-2023`, `_foo`) est refusé au lieu d'être retiré en silence.
- Les dates sentinelles (2999-01-01, 2222-02-22) se lisent comme « sans fin ».

### Security

- Actions GitHub épinglées par SHA, `yanked = "deny"`, bans `openssl`, `openssl-sys`, `native-tls` et `backoff`, cargo-deny chaque nuit.
- Règle d'âge de 14 jours des dépendances.
- Politique de signalement `SECURITY.md`.

### Parity

- Domaine seulement, visible par un client à partir de M4 : citation du Conseil constitutionnel sans « - objet » (P-035), plus de `eli_uri` (P-031, P-039), sentinelle de date omise (P-030).

[Unreleased]: https://github.com/muchiny/mcp-legifrance/commits/main
