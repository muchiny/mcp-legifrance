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

[Unreleased]: https://github.com/muchiny/mcp-legifrance/commits/main
