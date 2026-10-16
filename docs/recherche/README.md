# Notes de recherche (2026-10-15)

Rapports bruts qui ont servi à écrire le plan de `docs/plan/`. Le plan en extrait l'essentiel. Ces rapports gardent le détail, les preuves (`chemin:ligne`, URL) et la liste de ce qui n'a pas pu être vérifié.

| Rapport | Sujet |
|---|---|
| [01-parite.md](01-parite.md) | Spécification de parité de la référence Python `fr-eli-mcp@d3b5b13`. Elle couvre les outils, les validations, la normalisation, le client PISTE, l'audit, les tests et le packaging. Check-list `P-001…` et écarts `E-01…`. Les sorties attendues sur les fixtures sont dans [`../plan/annexes/donnees/parite-golden.json`](../plan/annexes/donnees/parite-golden.json). |
| [02-defauts.md](02-defauts.md) | Audit critique de la référence : 47 défauts (`D-01…D-47`), avec preuve, conséquence, mesure côté Rust et tests de non-régression. |
| [03-dto.md](03-dto.md) | Formes JSON réelles de l'API (fixtures + Swagger 2.4.2), DTO serde minimaux, mojibake, dates, facettes, économie de contexte mesurée. |
| [04-rmcp-rust.md](04-rmcp-rust.md) | SDK `rmcp` 3.5.1 (API, macros, pièges) et versions de plus de 70 crates de l'écosystème. |
| [05-piste-dila.md](05-piste-dila.md) | API Légifrance complète (68 opérations), options de recherche, OAuth, quotas, CGU, Judilibre, dumps DILA. |
| [06-mcp-spec.md](06-mcp-spec.md) | Spécification MCP 2026-07-28 pour un serveur en lecture seule ; conception d'outils économes en contexte ; registre MCP. |
| [07-citations.md](07-citations.md) | Grammaire des citations juridiques françaises, dictionnaire des codes, stratégie de résolution. Corpus de tests dans [`../plan/annexes/donnees/`](../plan/annexes/donnees/). |
| [08-mcp-2026-07-28.md](08-mcp-2026-07-28.md) | Révision MCP 2026-07-28 (« MCP v2 ») en détail : changelog, cycle sans session, compatibilité dual-era, check-list. |
| [09-rmcp-2026.md](09-rmcp-2026.md) | Support de 2026-07-28 par `rmcp` (oui, depuis la 3.x) ; bugs ouverts ; squelette de serveur. |

## Avertissements

- **Chemins.** Les chemins absolus `/tmp/claude-0/…/scratchpad/…` renvoient à des clones temporaires qui n'existent plus. Les références de type `RS:`, `SPEC:` et `[CL]/…` se retrouvent dans les dépôts publics cités, au commit indiqué dans chaque rapport.
- **Ce qui n'a pas pu être vérifié.** Ces rapports ont été écrits sans accès authentifié à l'API PISTE et sans pouvoir compiler de Rust : `static.crates.io` et `docs.rs` étaient bloqués. Les sites `legifrance.gouv.fr`, `piste.gouv.fr`, `data.gouv.fr` et `echanges.dila.gouv.fr` étaient injoignables. Ce qui n'a pas été vérifié est marqué **NON VÉRIFIÉ** et repris dans le registre `V-xx` de [`../plan/09-risques-et-questions.md`](../plan/09-risques-et-questions.md).
- **Licences.** Les projets sous AGPL-3.0 (`juridix`, `mcp-legifrance` de Ktulu-Analog) n'ont été lus que pour leurs idées : aucune ligne de leur code ne doit être reprise. La référence Python et ses fixtures sont sous Apache-2.0 ; leur réutilisation impose l'attribution prévue dans `NOTICE`.
