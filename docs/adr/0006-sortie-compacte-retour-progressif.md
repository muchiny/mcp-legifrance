# ADR-0006 — Sortie compacte via `structuredContent` et retour progressif

- **Statut** : Accepté
- **Date** : 2026-10-15
- **Jalons concernés** : M4 (vues de sortie, schémas), M5 (économie de contexte, budgets mesurés : v0.1.0)
- **Questions liées** : Q11 (`openWorldHint`, proposition adoptée par défaut, à confirmer avant v0.1.0)
- Révisé le 2026-10-15 (revue) : champs omissibles en `#[serde(default, …)]` ; `detail = "full"` quand `offset > 0` (AM-43) ; sources du résumé (AM-16) ; plafond dur de 45 000 caractères estimé puis mesuré, avec ordre de dégradation complet (AM-15) ; annotations et Q11 (AM-14).

## Contexte

- **Ce que voit le modèle sur un succès** (05 §1.2, recherche 06 §3.3) :
  - Claude Code remplace les blocs `content` par le `structuredContent` et ne transmet pas le texte. Sur une erreur, il ne transmet que le texte (mainteneur, issue anthropics/claude-code#86032, v2.1.233 et 2.1.260) ;
  - VS Code envoie `JSON.stringify(structuredContent)` à la place du texte et ne transmet pas l'`outputSchema` (`mcpLanguageModelToolContribution.ts:144-160`, `:328-337`).
- **Spécification** : un outil qui renvoie du contenu structuré « SHOULD also return the serialized JSON in a TextContent block » (`server/tools.mdx:500`, recherche 06 §3.3).
- **rmcp** : `CallToolResult::structured(v)` place aussi `v.to_string()` dans `content` (`model.rs:4155-4163`, recherche 04 §2.6).
- **Taille des réponses de la référence** (D-33, recherche 02 §4) :
  - une décision CONSTIT ou CETAT normalisée pèse environ 16 Ko et est rendue en entier ;
  - `eli_note` et `dataset_note` (171 + 335 caractères) sont répétées dans chaque réponse ;
  - `eli_uri` est toujours égal à `source_url` ;
  - descripteurs et instructions coûtent environ 22 Ko avant tout appel.
- **Seuils de Claude Code** (05 §1.3) :
  - avertissement au-delà de 10 000 tokens, limite à 25 000 ;
  - un résultat de plus de 50 000 caractères est écrit dans un fichier ;
  - les instructions sont coupées à 4 096 caractères.
- **Structure des textes** : `texte` est aplati (0 saut de ligne), alors que `texteHtml` porte les alinéas (D-26). Un découpage en paragraphes est donc possible.

## Décision

1. **Le `structuredContent` EST la réponse compacte** (00 §6) :
   - champs `None` omis (`skip_serializing_if = "Option::is_none"`) ;
   - listes optionnelles vides omises ; les listes principales (`hits`, `toc`, `passages`…) restent toujours présentes ;
   - tout champ non-`Option` omis conditionnellement (liste vide, booléen par défaut) porte `#[serde(default, skip_serializing_if = …)]` : sans `default`, schemars le laisse dans `required`, car rmcp génère l'`outputSchema` en contrat `Deserialize`, et le `structuredContent` qui l'omet ne serait plus conforme au schéma annoncé (05 §1.4 règle 1 ; 02 §3.6) ;
   - aucun doublon : `eli_uri` disparaît ([ADR-0009](0009-contrat-de-citation-v2.md)) et `title` est omis quand `citation` le contient ;
   - aucune note répétée : elles vont dans les instructions et dans `fr_coverage` ;
   - racine toujours objet.
2. **Repli texte** : le `TextContent` de succès est la **sérialisation JSON compacte du même objet**. Aucune information n'existe seulement dans le texte.
3. **Sur erreur** : texte seul `[code] message`, sans `structuredContent` ([ADR-0010](0010-erreurs-in-band.md)).
4. **Retour progressif des lectures longues** (`fr_get_text`, `fr_get_decision`, `fr_get_deliberation`, 00 §7) :

   | Paramètre | Défaut | Bornes | Effet |
   |---|---|---|---|
   | `detail` | `"summary"` ; `"full"` quand `offset > 0` est fourni sans `detail` | `"summary"`, `"full"` ; `"summary"` explicite avec `offset > 0` → `invalid_arg` | `summary` : sommaire officiel (`OfficialSummary` mappé par fond), sinon dispositif, sinon début du texte (`lead`), indiqué par `summary_source` ; résumé ≤ min(`max_chars`, 1 200) |
   | `offset` | 0 | ≥ 0, en caractères Unicode | début de la fenêtre |
   | `max_chars` | 4 000 | 500 à 40 000 | taille de la fenêtre, coupée en fin de paragraphe ou de phrase |
   | `focus` | — | chaîne | remplace la fenêtre par des `passages` classés par BM25 |
   | `passages` | 3 | ≤ 10 | nombre de passages |

   La sortie porte `text_length` et `next_offset` (absent en fin de texte). `max_chars` borne tout texte rendu : en mode résumé, un texte de longueur ≤ min(`max_chars`, 1 200) est rendu entier dans `text`, un texte plus long donne `summary` (00 §7). Les tables des matières se paginent par `toc_offset` et `toc_limit` (100, ≤ 500), avec `next_toc_offset`. La recherche se pagine selon [ADR-0014](0014-recherche-mode-pagination.md).
5. **Plafond dur** (00 §7, 05 §6.4) : le `structuredContent` sérialisé (JSON compact) ne dépasse jamais 45 000 caractères.
   - **Estimation** dans `lf-application`, qui n'a pas `serde_json` : somme des longueurs en caractères des chaînes + 64 par champ ou élément, cible 42 000. **Mesure exacte** dans `lf-adapter-mcp`, seul crate qui sérialise ; si l'estimation est prise en défaut, la dégradation reprend sur la vue, avec un `warn!`.
   - Ordre de dégradation : (1) TOC : moins d'entrées, `next_toc_offset` ajusté ; (2) recherches : retrait des `snippet` en partant du dernier hit, jamais d'un hit, avec `note`, puis des `articles[]` et des champs optionnels des derniers hits ; si la page ne tient toujours pas, `[invalid_arg]` qui donne le `page_size` maximal (règle déterministe et rejouable) ; (3) listes de `fr_verify_citations` et `fr_get_related` tronquées avec `truncated: true` ; (4) lectures longues : fenêtre effective raccourcie pour tenir sous le plafond, `next_offset` restant exact.
   - Aucune clé `_meta` `anthropic/*`.
6. **Pas de `resource_link`** en v0.1 ni en v0.2 : Claude Code le perdrait sur un succès (05 §1.2, point e).
7. **Coût fixe borné en CI** : `tools/list` ≤ 20 000 octets en v0.1, instructions ≤ 3 500 caractères. Ces budgets seront recalibrés sur la mesure de M4 (05 §6.6).
8. **Annotations** (00 §6) : `read_only_hint = true`, `destructive_hint = false`, `idempotent_hint = true`, `open_world_hint = true` (parité P-002 ; contenu externe traité comme non fiable ; adoptée par défaut, Q11). Réexamen avant v0.1.0 au vu de V-53 (confirmation des résultats dans VS Code) ; tout changement passe par le verrou des descripteurs et le CHANGELOG.

## Conséquences

### Positives

- Sur le prototype de 05 §6.5, une décision CONSTIT passe de **15 140 à 1 147** caractères par défaut (÷ 13). Une recherche CETAT de 3 hits fait 1 501 caractères avec extraits, contre 1 949 sans pour la référence. Les mesures définitives viennent des snapshots de M5.
- Même réponse lisible quelle que soit l'ère et dans les deux clients vérifiés ([ADR-0003](0003-mcp-2026-07-28-dual-era-rmcp.md)).
- La lecture complète reste possible et exacte : enchaîner les pages reconstruit le texte.

### Négatives

- Les octets sur le fil sont doublés (JSON en texte et en `structuredContent`), mais pas le contexte du modèle.
- Lire un texte en entier demande plusieurs appels. Le résumé heuristique (extraction du dispositif) peut manquer la partie utile ; `focus` y remédie.
- Les descriptions de 3 à 6 phrases et les schémas bornés coûtent environ 4 Ko de descripteurs de plus que la référence (05 §6.6).

### À surveiller

- Points **NON VÉRIFIÉS** sur les clients, dont dépend toute cette décision :
  - [V-46 · cc-structured-content-succes] : Claude Code transmet-il toujours le `structuredContent` sur un succès ? À re-tester à chaque version ;
  - [V-51 · desktop-structured-content] : forme transmise par Claude Desktop et claude.ai ;
  - [V-52 · cursor-ere-et-contenu] : ère et forme transmise par Cursor ;
  - [V-47 · cc-output-schema-contexte] : Claude Code transmet-il l'`outputSchema` au modèle ?
- Si un client majeur transmet le **texte** et non le `structuredContent`, cet ADR est à revoir (09).
- Ratio tokens par caractère en français juridique : **NON VÉRIFIÉ** [V-58 · tokens-par-caractere]. Tailles réelles des textes et des TOC : **NON VÉRIFIÉ** [V-75 · taille-max-textes], à mesurer en M5 sur des fixtures recapturées.
- Effet de `openWorldHint: true` dans VS Code (confirmation des résultats après exécution) : **NON VÉRIFIÉ** [V-53 · vscode-openworld-confirm] (V-53), mesuré avant v0.1.0 pour Q11.

### Tests qui la gardent

- `text_content_is_json_of_structured_content` et `structured_content_validates_against_output_schema` (P-004), y compris sur des vues dont toutes les listes omissibles sont vides.
- `paged_reading_reconstructs_full_text`, `offset_without_detail_defaults_to_full`, `summary_mode_returns_whole_text_only_within_max_chars`, `descriptor_size_budget`, `instructions_fit_budget`.
- `every_tool_response_under_hard_cap` (dont 100 hits à citations de 1 000 caractères et une fenêtre de 40 000 caractères riche en échappements), `snippets_removed_from_last_hits_when_over_cap`, `oversized_page_is_invalid_arg_with_max_page_size`.
- `tool_output_matches_golden`, avec les clés `notes_dropped`, `nulls_omitted` et `title_omitted_when_in_citation` (annexe A §4.1).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Texte Markdown lisible et `structuredContent` (recherche 06 §3.3, option b) | Claude Code jette le texte sur un succès. Une information propre au texte serait perdue, et la règle « rien que dans le texte » serait violée. |
| Remplacer `content` par un résumé court (recherche 04 §2.6) | S'écarte du SHOULD de la spécification. Les clients qui lisent le texte verraient une autre réponse. |
| Texte seul, sans `outputSchema` | Perd la sortie typée et sa validation ; VS Code et Claude Code privilégient le `structuredContent`. |
| `resource_link` vers `legifrance://…` au lieu du texte | Supprimé par Claude Code dès qu'un `structuredContent` existe (recherche 06 §3.3, point 5). |
| `max_chars` vers 8 000 et `hint` de continuation dans chaque réponse (recherche 06 §7.3 d) | Plus de tokens par appel. La manière de continuer est dite une fois, dans les descriptions et les instructions (05 §13.1 n° 6). |
| Texte complet par défaut (parité) | Reproduit D-33. |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §6, §7 ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §1.2 à §1.4, §5.0, §6, §13.1 ; [03-domaine.md](../plan/03-domaine.md) §7.5 à §7.9 ; [annexes/A-parite.md](../plan/annexes/A-parite.md) §4.1 ; [annexes/D-mcp-2026-07-28.md](../plan/annexes/D-mcp-2026-07-28.md) §4.6, §10.
- Recherche : [06-mcp-spec.md](../recherche/06-mcp-spec.md) §3.3, §7.1 à §7.3 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §2.6 ; [02-defauts.md](../recherche/02-defauts.md) §4, D-26, D-30, D-33 ; [03-dto.md](../recherche/03-dto.md) §13.
- URL : https://github.com/anthropics/claude-code/issues/86032 ; spécification `docs/specification/2026-07-28/server/tools.mdx` (dépôt `modelcontextprotocol/modelcontextprotocol`).
