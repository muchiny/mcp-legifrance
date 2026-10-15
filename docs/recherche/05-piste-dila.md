# 05 — API Légifrance (PISTE) complète + Judilibre + dumps open data DILA

Rapport de recherche (axe 05) pour la réécriture Rust de `fr-eli-mcp`. Date de rédaction : 2026-10-15.
Toutes les affirmations portent une source (fichier:ligne ou URL). Légende des statuts :

- **VÉRIFIÉ** : lu dans une source primaire (Swagger officiel, documentation DILA, CGU, fixtures réelles enregistrées), ou observé dans du code tiers de façon non ambiguë (et alors signalé « tiers »).
- **NON VÉRIFIÉ** : affirmation tierce non recoupée, contradiction entre sources, ou déduction de notre part. À tester en sandbox avant d'en dépendre.

Aucun appel authentifié n'a été fait. Les hôtes `legifrance.gouv.fr`, `piste.gouv.fr`, `data.gouv.fr`, `echanges.dila.gouv.fr`, `dila.gouv.fr` sont injoignables depuis cet environnement (curl → `CONNECT tunnel failed, response 403` ; WebFetch → `ENOTFOUND`). Les faits web proviennent donc de résumés WebSearch (URL citées) et de copies de documents officiels archivées dans des dépôts publics.

---

## 0. Abréviations de chemins et inventaire des sources

| Abréviation | Chemin / URL | Version lue |
|---|---|---|
| `[PY]` | `/home/user/matematicsolutions/fr-eli-mcp` (référence Python, lecture seule) | d3b5b13 (2026-09-24) |
| `[CL]` | `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/clones` | — |
| `[SPEC]` | `[CL]/pylegifrance/pylegifrance/models/generated/legifrance.json` — **Swagger 2.0 officiel de l'API Légifrance, `info.version` = 2.4.2, host `api.piste.gouv.fr` (prod)** | pylegifrance d8e0e59 (2026-08-19), MIT |
| `[SPEC-SBX]` | `[CL]/p05-jmtanguy__droit-francais-mcp/Légifrance.json` — même Swagger 2.4.2 téléchargé côté **sandbox** (host `sandbox-api.piste.gouv.fr`) | f79a9ae (2025-11-21), MIT |
| `[RAW]` | `[CL]/pylegifrance/docs/raw/legifrance/` — copies archivées des documents officiels DILA : `description-des-tris-et-filtres-de-l-api.{xlsx,md}`, `exemples-d-utilisation-de-l-api.{docx,md}` (V3 du 17/09/2025), `lexique-api-lgf.{docx,md}`, `cgu-legifrance-api.{pdf,md}` (CGU v1.0 du 15/12/2022, canonique), `cgu-piste-legifrance-beta.{pdf,md}` (CGU Beta v1.1 du 04/08/2020 = le PDF `DILA_Legifrance_Beta_v2.pdf`), `conditions-generales-d-utilisation-de-l-api.{docx,md}` (version 2020 éditable) | idem |
| `[JDL]` | `[CL]/p05-Cour-de-cassation__judilibre-search` — dépôt officiel de l'API Judilibre (Cour de cassation), `public/JUDILIBRE-public.json` (OpenAPI 3.0.2, v1.2.5) et `public/JUDILIBRE-public-swagger.json` (Swagger 2.0) | ddfc166 (2026-10-13), licence non déclarée (`package.json:16` `"license": ""`) |
| `[JRX]` | `[CL]/p05-Ktulu-Analog__juridix` — client Rust (Tauri) Légifrance+Judilibre, **AGPL-3.0** | a177523 (2026-10-13) |
| `[KMCP]` | `[CL]/p05-Ktulu-Analog__mcp-legifrance` — MCP Python, **AGPL-3.0** | 3c916e3 (2026-06-26) |
| `[JMT]` | `[CL]/p05-jmtanguy__droit-francais-mcp` — MCP Python Légifrance+Judilibre, MIT | f79a9ae |
| `[PLF]` | `[CL]/pylegifrance` — SDK Python, MIT | d8e0e59 |
| `[PLFMCP]` | `[CL]/p05-pylegifrance__mcp-server-legifrance`, MIT | 3c20aa7 (2025-05-13) |
| `[LEGIPY]` | `[CL]/p05-Legilibre__legi.py` — import LEGI → SQLite, **CC0** (`README.md:118-120`) | bc68599 (2026-09-15) |
| `[ALEX]` | `[CL]/p05-Legilibre__Archeo-Lex` — versions de textes sous Git, WTFPL | f35c3bc (2019-03-10, inactif) |
| `[JL]` | `[CL]/p05-Dahliyaal__justicelibre` — pipeline bulk DILA (LEGI, JORF, JADE, CASS, CAPP, INCA, CONSTIT, KALI, CNIL) + Judilibre, MIT | c2440ab (2026-10-15) |
| `[LCX]` | `[CL]/p05-LawCodex__fr` — LEGI → Markdown, scripts `scripts/{fetch,ingest,build}.py` (lus via `git show`, clone sans blobs) | a1681ad (2026-10-11) |
| `[MFL]` | `[CL]/p05-matematicsolutions__mcp-fr-legal` — MCP offline SQLite FTS5 sur corpus LEGI, MIT | 3a0ad0f (2026-09-17) |
| `[TANT]` | `[CL]/p05-tantivy` — dépôt tantivy (lecture d'arborescence via `git show`) | e1acb3e (2026-10-15) |

Point de méthode important : **les deux copies du Swagger (`[SPEC]` prod et `[SPEC-SBX]` sandbox) sont identiques** pour `paths` et `definitions` ; elles ne diffèrent que par `host`, `schemes` et les URL OAuth de `securityDefinitions` (diff programmatique effectué). Cela confirme que sandbox et production exposent la même surface d'API 2.4.2. **VÉRIFIÉ.**

Version publiée sur data.gouv.fr : un fil de discussion (juillet 2025) signale que PISTE affichait 2.4.2 alors que la spec data.gouv.fr était encore 1.6.2.5 ; un utilisateur a demandé en mars 2026 un passage à OpenAPI 3.0 (résumé WebSearch, https://www.data.gouv.fr/dataservices/legifrance/discussions). **NON VÉRIFIÉ** (contenu de la réponse DILA non lu).

---

## 1. Catalogue complet des endpoints `lf-engine-app` (Swagger 2.4.2)

### 1.1 Vue d'ensemble

- Base prod : `https://api.piste.gouv.fr/dila/legifrance/lf-engine-app` (`[SPEC]` `host`+`basePath`) ; base sandbox : `https://sandbox-api.piste.gouv.fr/dila/legifrance/lf-engine-app` (`[SPEC-SBX]`, `[PY]/src/fr_eli_mcp/client.py:29`). **VÉRIFIÉ.**
- **68 opérations** : 59 `POST` + 9 `GET`, réparties en 6 contrôleurs : Consult 39, List 13, Search 5, Suggest 4, Chrono 4, Misc 3 (comptage programmatique sur `[SPEC]`). **VÉRIFIÉ.**
- Description des tags (`[SPEC]` `tags`) : Chrono = « Accès aux versions de texte », Consult = « Accès aux textes », List = « Liste les textes », Misc = « Services divers », Search = « Recherche sur les textes », Suggest = « Autosuggestion/Autocompletion sur les textes et articles ».
- Tous les `POST` : `consumes`/`produces` = `application/json`. Réponses déclarées pour chaque opération : `200`, `201`, `401`, `403`, `404` (aucune mention de 400/429/500 dans le Swagger). **VÉRIFIÉ** (`[SPEC]`, dump intégral produit dans `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/work/spec_dump.txt`).
- La référence Python n'utilise que 7 endpoints (`[PY]/src/fr_eli_mcp/client.py:187-208`) : `/search`, `/consult/lawDecree`, `/consult/getArticle`, `/consult/juri`, `/consult/cnil`, `/consult/kaliText`, `/consult/acco`. Ce sont les seuls « testés live » par nous indirectement (fixtures `[PY]/tests/fixtures/*.json`).
- Champ commun de nombreuses requêtes consult : `searchedString` (facultatif, « Texte de la recherche ayant aboutie à la consultation du texte », sert au surlignage). Champ commun de nombreuses réponses : `executionTime`, `dereferenced` (« Identifie si le contenu est référençable par les robots d'indexation »).

Notation des requêtes ci-dessous : `champ*` = requis selon le Swagger. Les types sont ceux du Swagger.

### 1.2 Search Controller

| Opération | Requête | Réponse | Utilité pour nos features | Statut |
|---|---|---|---|---|
| `POST /search` | `SearchRequestDTO {fond*, recherche*: RechercheSpecifiqueDTO}` (détail §2) | `SearchResponseDTO {totalResultNumber, totalArticleResultNumber, executionTime, typePagination, facets[Facet], results[SearchResult], descriptionFusionHtml}` | Cœur de `fr_search`, `fr_search_all` (fond `ALL` + facette `FOND`), `fr_resolve` (recherche `NUM`/`NUM_ARTICLE`/`NOR`/`ECLI`/`NUM_AFFAIRE` en `EXACTE`), `fr_verify_citations` | VÉRIFIÉ, live (réf. Python) |
| `POST /search/canonicalArticleVersion` | `ArticleRequest {id*}` (ex. `LEGIARTI000006307920`) | `SearchCanonicalArticleVersionResponse {articleVersions[ArticleVersion{id, version, numero, etat, dateDebut, dateFin, ordre}]}` | **Liste légère des versions d'un article** (métadonnées seules) → index de versions pour `fr_diff_article`, choix « version à la date D » | VÉRIFIÉ (spec) |
| `POST /search/canonicalVersion` | `SearchCanonicalVersionRequest {cidText*, date*, cidSection}` | `SearchCanonicalVersionResponse {cid, dateDebut, dateFin}` | « Quelle version d'un texte est en vigueur à la date D » | VÉRIFIÉ (spec) |
| `POST /search/nearestVersion` | `SearchNearestVersionRequest {cidText*, date*, cidSection}` | `SearchNearestVersionResponse {title: SearchTitle, sectionTitle}` | Version la plus proche d'une date (gestion des dates hors bornes) | VÉRIFIÉ (spec) |
| `GET /search/ping` | — | `text/plain` « pong » | Healthcheck | VÉRIFIÉ (spec) |

### 1.3 Consult Controller — LEGI (codes, LODA, sections, tables des matières)

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /consult/lawDecree` | `LawDecreeConsultRequest {textId*, date*, searchedString, fromSuggest}` (textId = « Chronical ID du texte », ex. `LEGITEXT000006075116`) | `ConsultTextResponse` | Texte LODA complet à une date (`fr_get_act`) | VÉRIFIÉ, live |
| `POST /consult/code` | `CodeConsultRequest {textId*, date*, sctCid, abrogated, searchedString, fromSuggest}` ; `sctCid` « Non requis pour la consultation de la table des matières sinon obligatoire » | `ConsultTextResponse` | Contenu d'une **section** de code à une date (lecture paginée par section) | VÉRIFIÉ (spec) |
| `POST /consult/legiPart` | `LegiConsultRequest {textId*, date*, searchedString}` | `ConsultTextResponse` | Texte LEGI à une date (exemple officiel pour une loi : `[RAW]/exemples-d-utilisation-de-l-api.md:483-491`) | VÉRIFIÉ (spec + doc) |
| `POST /consult/legi/tableMatieres` | `LegiSommaireConsultRequest {textId*, date*, nature}` ; `textId` = « id OU cid du texte » ; `nature` = CODE, DECRET, ARRETE, LOI, ORDONNANCE… | `ConsultTextResponse` | **Table des matières** d'un code ou d'une LODA (`fr_get_toc`, resource MCP `legifrance://code/{id}/toc`) | VÉRIFIÉ (spec) |
| `POST /consult/code/tableMatieres` | `CodeConsultRequest` | `ConsultTextResponse` | **Déprécié** : « utiliser /legi/tableMatieres avec le champ 'nature'='CODE' » (`[SPEC]` summary ; `[RAW]/description-des-tris-et-filtres-de-l-api.md:422-424`) | VÉRIFIÉ — ne pas utiliser |
| `POST /consult/getSectionByCid` | `SectionCidRequest {cid*}` (ex. `LEGISCTA000006163288`) | `GetListSectionResponse {listSection[Section{id, cid, titre, dateDebut, dateFin, liensSection[StructureLienSection], liensArticle[StructureLienArticle], commentaire, nota, notaHtml, liensModification[Lien], context}]}` | Toutes les versions d'une section + structure (navigation TOC) | VÉRIFIÉ (spec) |
| `POST /consult/getCodeWithAncienId` | `CodeConsultWithAncienId {ancienId}` (ex. `CASSURAL`) | `ConsultTextResponse` | Résolution d'anciens identifiants de codes | VÉRIFIÉ (spec) |

`ConsultTextResponse` (extrait, `[SPEC]` `definitions.ConsultTextResponse`) : `id` (ex. `LEGITEXT000006070994_01-08-2016`), `cid`, `title`, `nature`, `nor`, `eli`, `alias`, `etat`, `jurisState`, `jurisDate`, `dateTexte`, `dateParution`, `dateDebutVersion`, `dateFinVersion`, `textNumber`, `numParution`, `visa`, `signers`, `prepWork`, `nota`, `notice`, `resume`, `observations`, `rectificatif`, `appellations[]`, `motsCles[]`, `dossiersLegislatifs[]`, `liens[TexteLien]`, `articles[ConsultArticle]` (racine, ordonnée), `sections[ConsultSection]` (1er niveau, ordonnée), `textAbroge`, `inap`, `idConteneur`, `pagePdf`, `filePath/fileName/fileSize`, `dereferenced`.

`ConsultSection` : `id`, `cid`, `title`, `etat`, `dateDebut`, `dateFin`, `intOrdre`, `sectionConsultee`, `commentaire`, `renvoi`, `renvoiNum`, `notaHtml`, `notaSectionsAafficher[]`, `articles[ConsultArticle]`, `sections[ConsultSection]` (récursif).

`ConsultArticle` : `id`, `cid`, `num`, `etat`, `type` (AUTONOME…), `content` (HTML), `dateDebut`, `dateFin`, `articleVersion`, `versionLabel`, `multipleVersions`, `intOrdre`, `path` (ex. `/LEGISCTA…/LEGIARTI…`), `pathTitle[]`, `nota`, `modificatorTitle`, `modificatorDate`, `modificatorCid`, **`comporteLiens`** (« Utiliser l'API relatedLinksArticle pour récupérer la liste des liens »), **`lstLienCitation`** (« Toujours vide (voir propriété comporteLiens) »), `lstLienModification[LienModification]`, **`comporteLiensSP`** (« Utiliser l'API servicePublicLinksArticle »), `conditionDiffere`, `surtitre`, `historique`, `infosComplementaires(Html)`, `infosRestructurationBranche(Html)`, `renvoi`, `notaSectionsAafficher[]`. **VÉRIFIÉ** (`[SPEC]` `definitions.ConsultArticle`).

### 1.4 Consult Controller — articles, versions, liens

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /consult/getArticle` | `ArticleRequest {id*}` (LEGIARTI ; KALIARTI validé live par la réf. Python, `[PY]/SOURCES.md:14` ; JORFARTI : NON VÉRIFIÉ) | `GetArticleResponse {article: Article, executionTime, dereferenced}` | `fr_get_text` ; l'objet `Article` contient **`articleVersions[ArticleVersion]`**, `versionPrecedente`, `lienCitations[LienCitation]`, `lienModifications[LienModification]`, `lienConcordes[LienConcorde]`, `lienAutres[Lien]`, `context`, `textTitles[TextTitle]` | VÉRIFIÉ, live |
| `POST /consult/getArticleByCid` | `ArticleCidRequest {cid*}` (« Chronical ID de l'article ») | `GetListArticleResponse {listArticle[Article]}` — « Liste des versions de l'article » | **Toutes les versions complètes (texte inclus) d'un article en un appel** → base idéale de `fr_diff_article` | VÉRIFIÉ (spec) |
| `POST /consult/getArticleWithIdAndNum` | `ArticleConsultWithIdAndNum {id, num}` (id = LEGITEXT ; num ex. `5-8` dans la spec, `L5-8` dans le xlsx) | `GetArticleResponse` | **`fr_resolve` direct « code + numéro d'article → article en vigueur »**. Limitation officielle : « seuls les articles ne comportant pas de version en vigueur différée peuvent être trouvés par cette méthode » | VÉRIFIÉ (spec) |
| `POST /consult/getArticleWithIdEliOrAlias` | `ArticleIdEliOrAliasRequest {idEliOrAlias*}` (ex. `/eli/decret/2021/7/13/PRMD2117108D/jo/article_1`) | `GetArticleResponse` | Résolution **ELI → article** (`fr_resolve`) | VÉRIFIÉ (spec) |
| `POST /consult/relatedLinksArticle` | `RelatedLinksArticleRequest {articleId*}` | `RelatedLinksArticleResponse {liensCitePar[LiensRelatifsDTO], liensCite[LiensRelatifsDTO]}` ; `LiensRelatifsDTO {id, cidText, cidParent, nature (LODA/JORF/CODE/KALI/JORFCONT), name, dateVigeur}` | **Graphe de citations** « cite / cité par » d'un article (feature « liens ») | VÉRIFIÉ (spec) |
| `POST /consult/concordanceLinksArticle` | `ConcordanceLinksRequest {articleId*}` | `SectionsRevisionArticleResponse {oldTexts[ArticleDTO], newTexts[ArticleDTO]}` ; `ArticleDTO {id, idText, name, nature, dateDebut, dateFin, modifications[ModificationDTO]}` | Concordances (renumérotations, recodifications : ancien ↔ nouvel article) | VÉRIFIÉ (spec) |
| `POST /consult/sameNumArticle` | `SameNumArticleRequest {textCid*, articleCid*, articleNum*, date*}` | `SectionsRevisionArticleResponse` | « Articles ayant eu le même numéro dans des versions précédentes du texte » (désambiguïsation de citations anciennes) | VÉRIFIÉ (spec) |
| `POST /consult/servicePublicLinksArticle` | `ServicePublicLinksArticleRequest {articleCid, fond}` | `ServicePublicLinksArticleResponse {liensSP: map<string,string>}` | Liens vers fiches service-public.fr (contexte grand public) | VÉRIFIÉ (spec) |
| `POST /consult/hasServicePublicLinksArticle` | corps = tableau de chaînes (ids d'articles) | tableau de chaînes | Filtrage en lot des articles ayant des liens SP | VÉRIFIÉ (spec) |

Champs de liens (`[SPEC]` `definitions`) :

- `LienCitation {articleId, articleNum, textCid, textTitle, natureText, linkType (ex. MODIFIE), linkOrientation (ex. « cible »), dateDebut, datePubli, parentCid, numTexte}`.
- `LienModification {articleId, articleNum, textCid, textTitle (ex. « LOI n°2015-990 du 6 août 2015 - art. 53 (V) »), natureText, linkType, linkOrientation, datePubliTexte, dateSignaTexte, dateDebutCible}`.
- `LienConcorde {articleId, articleNum, textCid, textTitle, natureText, linkType, linkOrientation}`.
- `ArticleVersion {id, version, numero, etat, dateDebut, dateFin, ordre}`.
- `Article` contient aussi : `texte`, `texteHtml`, `nota`, `notaHtml`, `num`, `etat`, `type`, `dateDebut`, `dateFin`, `dateDebutExtension`, `dateFinExtension`, `versionArticle`, `versionPrecedente`, `multipleVersions`, `idEli`, `idEliAlias`, `cid`, `cidTexte`, `idTexte`, `sectionParentId/Cid/Titre`, `fullSectionsTitre`, `computedNums[]`, `ordre`, `origine`, `nature`, `inap`, `conteneurs[]`, `activitePro[]`, `numeroBrochure[]`, `calipsos[]`, `refInjection`, `idTechInjection`, `comporteLiensSP`, champs KALI (`conditionDiffere`, `surtitre`, `historique`, `infosRestructurationBranche`).

**Écart spec ↔ réel observé** : la fixture `[PY]/tests/fixtures/consult_article.json` contient `isMultipleVersions: true` (pas `multipleVersions`), et ne contient pas `articleVersions`, `lienCitations`, `lienModifications` (fixture possiblement élaguée par les auteurs Python — **NON VÉRIFIÉ**). Elle contient en plus `versionPrecedente: "LEGIARTI000050396821"`, `computedNums: ["9"]`, `context.titresTM[]` (chemin des sections avec `debut`/`fin`/`etat`/`xPath`), `refInjection: "MD-20260424_200447_879_BDJQUOT"`, `idTechInjection: "index_DATA_NEXT_ARTICLE_LEGIARTI000006419288"`. **Conséquence Rust** : DTO tolérants (`#[serde(default)]`, alias `isMultipleVersions`/`multipleVersions`, pas de `deny_unknown_fields`).

### 1.5 Consult Controller — JORF, ELI, NOR

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /consult/jorf` | `JorfConsultRequest {textCid*, searchedString}` (ex. `JORFTEXT000033736934`) | `ConsultJorfResponse` (comme ConsultTextResponse + `hasLoda` « Indique si le texte a une version consolidée », `jorfFileMetadata[FileMetadata]`, `hasSinglePdf`) | Texte JO original (version initiale) ; `hasLoda` → bascule vers LEGI | VÉRIFIÉ (spec) |
| `POST /consult/jorfPart` | `JorfConsultRequest {textCid*}` (section ou article JORF) | `ConsultJorfResponse` | Partie d'un texte JO | VÉRIFIÉ (spec) |
| `POST /consult/jorfCont` | `JorfContConsultRequest {id, num, date, start: ConsultDateRequest, end: ConsultDateRequest, searchText, pageNumber, pageSize (max 100), highlightActivated}` ; `ConsultDateRequest {year, month, dayOfMonth}` | `GetJosResponse {totalNbResult, items[GetJorfContResponseItem{joCont: FullConteneur, joEA, joInap, josPat[FileMetadata]}]}` | **Sommaire d'un JO** (JORFCONT → liste des JORFTEXT) ; `FullConteneur.structure` (Tms récursif `{titre, niv, ordre, liensTxt[], tms[]}`), `idEli` (ex. `/eli/jo/2019/1/25/0021`) | VÉRIFIÉ (spec + doc `[RAW]/exemples…md:1191-1205`) |
| `POST /consult/lastNJo` | `LastNElementRequest {nbElement*}` | `GetJorfContResponse {totalNbResult, containers[Conteneur]}` | « Derniers JO » (veille, resource MCP `legifrance://jorf/latest`) ; doc officielle : « Ce chiffre doit être inférieur à 2500, sinon vous aurez des erreurs » (`[RAW]/exemples…md:1187`) | VÉRIFIÉ |
| `POST /consult/getJoWithNor` | `JorfConsultWithNorRequest {nor*}` | `ConsultJorfResponse` | Résolution **NOR → texte JO** (`fr_resolve`). La doc conseille plutôt jorfCont→jorf « car tous les textes n'ont pas de NOR » (`[RAW]/exemples…md:1209`) | VÉRIFIÉ |
| `POST /consult/eliAndAliasRedirectionTexte` | `JorfConsultWithIdEliAliasRequest {idEliOrAlias*}` (ex. `/eli/decret/2018/2/13/JUSC1732516D/jo/texte`) | `ConsultJorfResponse` | Résolution **ELI → texte JO** : comble le constat de `[PY]/DISCOVERY.md:34-40` (ELI null côté LODA) — l'ELI existe côté JORF | VÉRIFIÉ (spec) |
| `POST /consult/getTables` | `TableRequest {endYear*, startYear}` | `GetTableResponse {tables[FileMetadata]}` | Tables annuelles (PDF) — faible intérêt | VÉRIFIÉ (spec) |

Remarque officielle : « les JO anciens ne comportent pas de version HTML des textes. Vous ne pourrez donc pas les récupérer via l'API (avant juin 2004) » (`[RAW]/exemples…md:1223`) ; le JORF électronique authentifié (JOEA) existe depuis juin 2004 (`[RAW]/lexique-api-lgf.md:159`). **VÉRIFIÉ.**

### 1.6 Consult Controller — KALI (conventions collectives), ACCO, BOCC

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /consult/kaliText` | `KaliTextConsultRequest {id*}` (KALITEXT) | `ConsultKaliTextResponse` (sections, articles, `typeTexte` TEXTE_BASE…, `conteneurs[]`, `signataires`, `visas`, `libelleExtension`, `libelleElargissement`, `numeroBo`…) | `fr_get_convention` (existant) | VÉRIFIÉ, live |
| `POST /consult/kaliCont` | `KaliContConsultRequest {id*}` (spec : ex. `KALICONT000005635384`) | `ConsultKaliContResponse {id, titre, num, numeroTexte (« IDCC 1261 »), nature (IDCC), texteBaseId[], activitePro[], categorisation[], sections[ConsultSection]}` | **Conteneur d'une convention** = toute la convention (textes de base, avenants, salaires…) | VÉRIFIÉ (spec) |
| `POST /consult/kaliContIdcc` | `KaliContConsultIdccRequest {id*}` (ex. `1261`) | `ConsultKaliContResponse` | **Convention par IDCC** (`fr_get_convention(idcc=…)`, completion d'IDCC) | VÉRIFIÉ (spec) |
| `POST /consult/kaliArticle` | `KaliTextConsultArticleRequest {id*}` (KALIARTI) | `ConsultKaliTextResponse` | Texte KALI depuis un article | VÉRIFIÉ (spec) |
| `POST /consult/kaliSection` | `KaliTextConsultSectionRequest {id*}` (KALISCTA) | `ConsultKaliTextResponse` | Texte KALI depuis une section | VÉRIFIÉ (spec) |
| `POST /consult/acco` | `AccoConsultRequest {id*, searchedString}` | `ConsultAccoResponse {acco: Accord}` ; `Accord` a `attachment: Attachment {content, content_type, content_length, language, title, author, keywords, date, name}` et `attachementUrl`, `data` | `fr_get_company_agreement` (existant : métadonnées seulement, texte en .docx — `[PY]/SOURCES.md:15`) | VÉRIFIÉ (spec, live). Le contenu de `attachment.content` (texte extrait ? base64 ?) : **NON VÉRIFIÉ** (absent de la fixture `consult_acco.json`) |
| `POST /consult/getBoccTextPdfMetadata` | `BoccConsultRequest {id, forGlobalBocc}` (ex. `boc_20200028_0001_p000.pdf`) | `ConsultBoccResponse` | Bulletins officiels des conventions collectives (PDF) | VÉRIFIÉ (spec) |

Incohérence documentaire : le xlsx officiel donne `KALITEXT000005677408` comme exemple pour `/consult/kaliCont` et `KALICONT000005635384` pour `/consult/kaliContIdcc` (`[RAW]/description-des-tris-et-filtres-de-l-api.md:606-620`), à l'inverse du Swagger. Se fier au Swagger. **VÉRIFIÉ (incohérence).**

### 1.7 Consult Controller — jurisprudence, CNIL, circulaires, parlement

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /consult/juri` | `JuriConsultRequest {textId*, searchedString}` | `ConsultJuriTextResponse {text: TexteSimple}` | `fr_get_decision` pour JURITEXT/CONSTEXT/CETATEXT (`[PY]/DISCOVERY.md:78-82`) | VÉRIFIÉ, live |
| `POST /consult/getJuriWithAncienId` | `JuriConsultWithAncienId {ancienId}` (ex. `JG_L_2006_09_000000269553`) | `ConsultJuriTextResponse` | Anciens identifiants de décisions | VÉRIFIÉ (spec) |
| `POST /consult/getJuriPlanClassement` | `JuriPlanClassementRequest {id (JURINOME…), libelle, niveau, page, fond (ex. « juri »), searchByNiveau, searchSuggest}` | `GetListPlanClassementJuriResponse {listPlanClassementJuri[Nomenclature{id, parent, niveau, libelleNiveau, libelleArbo, arbo, feuille, idJuritext, titreJuritext}]}` | **Plan de classement** (titrages JURINOME / CETANOME présents dans les décisions) → navigation thématique, liens « décisions de même titrage » | VÉRIFIÉ (spec) |
| `POST /consult/cnil` | `CnilConsultRequest {textId*}` | `ConsultCnilTextResponse` | `fr_get_deliberation` | VÉRIFIÉ, live |
| `POST /consult/getCnilWithAncienId` | `CnilConsultWithAncienId {ancienId}` (ex. `MCN97020008A`) | `ConsultCnilTextResponse` | Anciens ids CNIL | VÉRIFIÉ (spec) |
| `POST /consult/circulaire` | `CirculaireConsultRequest {id*}` (ex. `44128`) | `ConsultCirculaireResponse` | Fonds CIRC (non couvert aujourd'hui) | VÉRIFIÉ (spec) |
| `POST /consult/dossierLegislatif` | `DossierLegislatifRequest {id*}` (ex. `JORFDOLE000038049286`) | `ConsultDossierLegislatifResponse {dossierLegislatif: DossierLegislatif{titre, type, legislature, exposeMotif, echeancier, dossiers[], arborescence…}}` | Travaux préparatoires / échéancier d'application d'une loi (lien loi → décrets d'application) | VÉRIFIÉ (spec) |
| `POST /consult/debat` | `DebatParlementaireConsultRequest {id*}` (ex. `AN_2020-090.pdf`) | `ConsultDebatResponse` | Débats parlementaires (PDF) | VÉRIFIÉ (spec) |

`TexteSimple` (réponse JURI/CONSTIT/CETAT/CNIL…) — champs utiles aux **liens entre décisions** (`[SPEC]` `definitions.TexteSimple`) : `ecli`, `numeroAffaire[]`, `juridiction`, `formation`, `solution`, `natureJuridiction`, `decisionAttaquee {date, formation}`, `publicationRecueil` (ex. « C »), `typePublicationBulletin`, `numeroPublicationBulletin`, `sommaire[TexteSommaire]`, **`citationJp` / `citationJpHtml`** (« citations jurisprudentielles »), **`liens[TexteLien]`** (« Liste de liens vers d'autres textes »), `titrages[]` / `titragesKey[]`, `texte`/`texteHtml`, `president`, `rapporteur`, `avocatGl`, `avocats`, `commissaire`, `provenance` (ex. « CAPP »), `typeControleNormes`, `numLoiDef`, `titreLoiDef`, `dateLoiDef` (CONSTIT).

Observé dans les fixtures (`[PY]/tests/fixtures/consult_juri*.json`, analyse programmatique) :

- Cour de cassation (`consult_juri.json`) : `citationJp` = texte libre de rapprochements (« … à rapprocher : 1re Civ., 25 mai 2023, pourvoi n° 21-23.174 (cassation partielle) et l'arrêt cité. ») ; `liens[]` = objets `{typeLien: "CITATION", sens: "source", title: "1 du code civil.", cidTexte: "", id: "", …}` (identifiants **vides**) ; `titrages` = chaînes `JURINOME…-JURINOME…` ; `decisionAttaquee = {formation: "Cour d'appel de Lyon", date: 1704844800000}` ; `provenance: "CASC"`. **VÉRIFIÉ.**
- Conseil d'État (`consult_juri_cetat.json`) : `liens = []`, `citationJp = ""`, `titrages` = `CETANOME…` ; `sommaire[].resumePrincipal` contient les abstracts. **VÉRIFIÉ.**
- Conseil constitutionnel (`consult_juri_constit.json`) : `liens = []`, `citationJp = null`, `sommaire = []`. **VÉRIFIÉ.**

Conclusion « liens entre décisions » via Légifrance : **pas d'identifiants structurés** de décisions citées ; seul `citationJp` (texte) + `titrages` (classification). Il faudra un extracteur de références (regex « pourvoi n° NN-NN.NNN », ECLI, dates) puis `fr_resolve` via `/search` (`NUM_AFFAIRE` en `EXACTE`, exemple officiel `[RAW]/exemples…md:1121-1171`). Judilibre fournit en revanche des `rapprochements` avec ID JURITEXT (§5). **VÉRIFIÉ (constat)** ; l'approche d'extraction est une recommandation.

### 1.8 Chrono Controller (versions de textes)

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /chrono/textCid` | `ChronoLegiTextRequest {textCid*, dateConsult*, startYear*, endYear*}` | `ChronolegiResponse {datePublication, regroupements[RegroupementDTO{title (ex. « 2015 »), detailLoaded, versions: map<date, VersionDTO>}]}` | **Historique des versions d'un texte** avec, pour chaque version, les articles modificateurs et leurs actions | VÉRIFIÉ (spec) |
| `POST /chrono/textCidAndElementCid` | `ChronoLegiArticleRequest {textCid*, elementCid*}` | `ChronolegiResponse` | Historique restreint à un article ou une section | VÉRIFIÉ (spec) |
| `GET /chrono/textCid/{textCid}` | path `textCid` | `HasChronolegiResponse {chronolegiExists}` | Test rapide « ce texte a-t-il plusieurs versions » | VÉRIFIÉ (spec) |
| `GET /chrono/ping` | — | « pong » | Healthcheck | VÉRIFIÉ (spec) |

Modèle chrono (`[SPEC]` `definitions`) : `VersionDTO {dateDebut, isEndVersion, articlesModificateurs: map<AAAA-MM-JJ, ArticleModificateurDTO>}` ; `ArticleModificateurDTO {id, title (« LOI n°2018-287 du 20 avril 2018 - art. 6 »), nature, dateDebutCible, actions: map<type, ActionChronoDTO>}` ; `ActionChronoDTO {action ∈ {CREATION, CODIFICATION, MODIFICATION, TRANSFERT, ABROGATION, ANNULATION, PEREMPTION, DISJONCTION, RECTIFICATION, SUBSTITUTION, DEPLACE, VERSEMENT, DENONCIATION, REMPLACEMENT, EXTENSION, ELARGISSEMENT}, parents: map<id, ParentChronoDTO>}` ; `ParentChronoDTO {id, cid, cidText, name, nature, dateDebut, texteCible, articlesCibles: map, sectionsCibles: map}` ; `CibleChronoDTO {id, cid, cidText, idParent, cidParent, path (chemin complet LEGITEXT/…/LEGIARTI), nature, name, dateDebut, dateFin, sens}`. → permet à `fr_diff_article` d'annoter **pourquoi** une version a changé (« modifié par LOI n°… art. … »).

### 1.9 List Controller

Toutes les listes paginées ont `pageNumber*` et `pageSize*` (« max 100 »). **VÉRIFIÉ** (`[SPEC]`).

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /list/code` | `CodeListRequest {pageNumber*, pageSize*, sort ∈ {TITLE_ASC}, states[] (enum états), codeName}` | `CodeListResponse {totalResultNumber, results[CodeListResult{id, cid, titre, etat, dateDebut, dateFin, lastUpdate, pdfFile*}], codeNames: Facet, states: Facet}` | **Liste des codes (en vigueur)** → resource MCP `legifrance://codes`, completion du nom de code, table nom→LEGITEXT dynamique (ne pas coder en dur) | VÉRIFIÉ (spec) |
| `POST /list/loda` | `LODAListRequest {pageNumber*, pageSize*, sort/secondSort ∈ {PUBLICATION_DATE_ASC/DESC, SIGNATURE_DATE_ASC/DESC, ID_ASC/DESC}, natures[] ∈ {LOI, ORDONNANCE, DECRET, DECRET_LOI, ARRETE, CONSTITUTION, DECISION, CONVENTION, DECLARATION, ACCORD_FONCTION_PUBLIQUE}, legalStatus[], signatureDate: DatePeriod, publicationDate: DatePeriod}` | `LODAListResponse {results[LODAListResult{id, cid, titre, etat, dateDebut, dateFin, lastUpdate, dossiersLegislatifs[]}], natures: Facet, legalStatus: Facet}` | Listes chronologiques (« lois publiées ce mois-ci »), sans requête plein texte | VÉRIFIÉ (spec) |
| `POST /list/conventions` | `ConventionsListRequest {pageNumber*, pageSize*, idcc, titre, keyWords[], legalStatus[], textesBase[] ∈ {TEXTE_BASE}, sort/secondSort ∈ {DATE_UPDATE, DATE_PUBLI_ASC/DESC, SIGNATURE_DATE_ASC/DESC, ID_ASC/DESC}}` | `ConventionsListResponse {results[ConventionsListResult{id, cid, titre, etat, idcc, cidConteneur, descriptionFusion(Html)}], legalStatus, typeTexte, moteCles: Facet}` | **Conventions par IDCC** + filtre mots-clés | VÉRIFIÉ (spec) |
| `POST /list/dossiersLegislatifs` | `DossiersLegislatifsRequest {type* ∈ {LOI_PUBLIEE, ORDONNANCE_PUBLIEE, PROJET_LOI, PROPOSITION_LOI}, legislatureId*}` | `DossiersLegislatifsListResponse` | Dossiers législatifs par législature | VÉRIFIÉ (spec) |
| `POST /list/legislatures` | (pas de corps) | `LegislaturesListResponse {legislatures[Legislature{id, libelle, dateDebut, dateFin}]}` | Référentiel | VÉRIFIÉ (spec) |
| `POST /list/bocc` | `GlobalBoccListRequest {pageNumber*, pageSize*, idGlobalBocc, sortValue (BOCC_SORT_ASC/DESC), intervalPublication (« 01/01/2020 > 31/01/2020 »), searchForSingleGlobalBocc}` | `BoccGlobalListResponse` | BOCC | VÉRIFIÉ (spec) |
| `POST /list/boccTexts` | `BoccListRequest {pageNumber*, pageSize*, idccs[], idMainBocc, departments[], titre, intervalPublication, sortValue, searchForGlobalBocc, searchForTextsBocc}` | `BoccTextsListResponse` | Textes BOCC par IDCC | VÉRIFIÉ (spec) |
| `POST /list/boccsAndTexts` | `BoccAndTextListRequest {pageNumber*, pageSize*, idcc, titre, intervalPublication, sortValue}` | `BoccGlobalListResponse` | idem | VÉRIFIÉ (spec) |
| `POST /list/bodmr` | `BodmrListRequest {pageNumber*, pageSize*, sort (PUBLICATION_DATE_ASC…), years[]}` | `BodmrListResponse` | Bulletins décorations/médailles — hors périmètre | VÉRIFIÉ (spec) |
| `POST /list/debatsParlementaires` | `{pageNumber*, pageSize*, typesPublication[] (« AN »…), dateParution (« jj/mm/aaaa > jj/mm/aaaa »), sortValue (DEBAT_PARLEMENTAIRE_ASC/DESC), secondSortValue (ID_ASC/DESC)}` | `DebatsParlementairesListResponse` | Hors périmètre initial | VÉRIFIÉ (spec + xlsx `…md:318-327`) |
| `POST /list/questionsEcritesParlementaires` | `{pageNumber*, pageSize*, parlementTypes[], periodePublication, sortValue (QUESTION_ECRITE_PARLEMENTAIRE_ASC/DESC), secondSortValue}` | `QuestionsEcritesParlementairesListResponse` | Hors périmètre initial | VÉRIFIÉ |
| `POST /list/docsAdmins` | `DocsAdminsListRequest {years[]}` | `DocsAdminsListResponse` | Documents administratifs | VÉRIFIÉ (spec) |
| `GET /list/ping` | — | « pong » | Healthcheck | VÉRIFIÉ (spec) |

Note : un client tiers envoie `"sort": "CODE_DATE_MAJ"` à `/list/code` (`[JL]/probe_legifrance.py:32`), valeur absente de l'enum `TITLE_ASC` de la spec. **NON VÉRIFIÉ.**

### 1.10 Suggest Controller (autocomplétion)

| Opération | Requête | Réponse | Utilité | Statut |
|---|---|---|---|---|
| `POST /suggest` | `SuggestSupplyRequest {searchText, supplies[] ∈ {ALL, ALL_SUGGEST, LODA_LIST, CODE_LIST, CODE_RELEASE_DATE, CODE_RELEASE_DATE_SUGGEST, CODE_LEGAL_STATUS, LODA_RELEASE_DATE, LODA_RELEASE_DATE_SUGGEST, LODA_LEGAL_STATUS, KALI, KALI_TEXT, CONSTIT, CETAT, JUFI, JURI, JORF, JORF_SUGGEST, CNIL, ARTICLE, CIRC, ACCO, PDF}, documentsDits}` | `SuggestResponse {totalResultNumber, results: map<id, SuggestValue>}` ; `SuggestValue {id, idTexte, section, label, nature, origin, dateVersion, appellations[] (ex. « Loi Macron »)}` | **MCP `completion/complete`** (noms de textes, appellations usuelles « loi Macron » → id), pré-résolution de `fr_resolve` | VÉRIFIÉ (spec). **C'est un POST JSON**, pas un GET (`[JRX]/src-tauri/src/juridique/legifrance.rs:8-11` documente l'erreur d'une version antérieure) |
| `POST /suggest/acco` | `SuggestRequest {searchText}` | `SuggestAccoResponse {results: map<string, SuggestAccoValue{siret, raisonSociale}>}` | Completion SIRET/raison sociale pour ACCO | VÉRIFIÉ (spec) |
| `POST /suggest/pdc` | `SuggestPdcRequest {searchText, origin, fond}` | `SuggestResponse` | Completion de libellés du plan de classement JURI | VÉRIFIÉ (spec) |
| `GET /suggest/ping` | — | « pong » | Healthcheck | VÉRIFIÉ (spec) |

### 1.11 Misc Controller et pings

| Opération | Réponse | Utilité | Statut |
|---|---|---|---|
| `GET /misc/commitId` | `map<string,string>` (déploiement/versioning) | Afficher la version de l'API amont dans `fr_status` / diagnostics ; détection de changement d'API | VÉRIFIÉ (spec) |
| `GET /misc/datesWithoutJo` | `DatesWithNoJoResponse {lstDateDisabled[] (epoch ms)}` | Éviter les requêtes JO inutiles | VÉRIFIÉ (spec) |
| `GET /misc/yearsWithoutTable` | `YearsWithNoTableResponse {lstYearDisabled[]}` | — | VÉRIFIÉ (spec) |
| `GET /{chrono,consult,list,search,suggest}/ping` | « pong » (`text/plain`) | Healthcheck par contrôleur ; exemple officiel `curl … /list/ping` avec Bearer (`[RAW]/exemples…md:159`) | VÉRIFIÉ |

### 1.12 Chemins utilisés par des tiers mais **absents** du Swagger 2.4.2

`[PLF]` appelle `consult/loda/version`, `consult/loda/versions`, `consult/juri/version`, `consult/juri/versions`, `consult/juri/ancienId` (`[PLF]/pylegifrance/fonds/loda.py:935,966` ; `[PLF]/pylegifrance/fonds/juri.py:609,643,669`). Ces chemins n'existent pas dans `[SPEC]`/`[SPEC-SBX]`. **NON VÉRIFIÉ — ne pas reproduire** ; utiliser `/consult/getArticleByCid`, `/search/canonicalArticleVersion`, `/chrono/*`, `/consult/getJuriWithAncienId`.

### 1.13 Mapping endpoints → features prévues du serveur Rust

| Feature | Endpoints principaux | Remarques |
|---|---|---|
| `fr_search` (existant) | `/search` | Garder résumé + pagination ; exposer `typeChamp`, `typeRecherche`, filtres, tri (§2) |
| `fr_search_all` (multi-fonds) | `/search` `fond=ALL` (champs ALL/TITLE, filtre `FOND`) **ou** N×`/search` en parallèle | `fond=ALL` renvoie la facette `FOND` (compte par fonds) en un seul appel → utile pour « où chercher » ; puis appels parallèles par fonds pour les résultats détaillés. Règle tierce : « fond ALL sans filtres » sinon 500 (`[JRX]/…/legifrance.rs:36-37,500-502`), « ALL : aucun filtre de date » (`[KMCP]/server.py:757`) — **NON VÉRIFIÉ** |
| `fr_resolve` (citation → id) | `/consult/getArticleWithIdAndNum` (code+num), `/consult/getArticleWithIdEliOrAlias`, `/consult/eliAndAliasRedirectionTexte` (ELI), `/consult/getJoWithNor` (NOR), `/consult/getCodeWithAncienId`/`getJuriWithAncienId`/`getCnilWithAncienId`, `/search` (`NUM`, `NUM_ARTICLE`, `NOR`, `ECLI`, `NUM_AFFAIRE`, `NUM_DEC`, `IDCC` en `EXACTE`), `/suggest` (appellations), `/list/code` (nom de code → LEGITEXT) | Workflows officiels de résolution : `[RAW]/exemples…md:165-695` (article de code par numéro, article d'un texte numéroté, loi par numéro, loi par date de signature, Constitution) |
| `fr_verify_citations` (anti-hallucination) | `fr_resolve` + `/consult/getArticle` (état `etat`, `dateDebut`/`dateFin`) + `/search/canonicalArticleVersion` | Principe tiers utile : « ne jamais conclure introuvable sans preuve » — statuts `EnVigueur / Abrogé / Introuvable / NonVérifiable / Erreur`, concurrence bornée à 4, cache par (code, numéro, jour) TTL 12 h (`[JRX]/src-tauri/src/juridique/verify.rs:22-25,66-76,1071-1078`). **Licence AGPL : idées uniquement, pas de copie de code** |
| `fr_diff_article` (versions) | `/consult/getArticleByCid` (toutes versions avec texte), `/search/canonicalArticleVersion` (index léger), `Article.versionPrecedente`, `/chrono/textCidAndElementCid` (motif du changement), `/consult/sameNumArticle`, `/consult/concordanceLinksArticle` | Diff texte côté serveur (crate `similar` 3.2.0, §6.7) sur `texte` (texte brut) plutôt que `texteHtml` |
| Liens (articles) | `/consult/relatedLinksArticle` (cite / cité par), `Article.lienModifications`, `lienCitations`, `lienConcordes`, `ConsultArticle.comporteLiens` | Paginer côté serveur (les listes peuvent être longues — taille **NON VÉRIFIÉE**) |
| Liens (décisions) | `TexteSimple.citationJp`, `liens`, `titrages` + `/consult/getJuriPlanClassement` ; Judilibre `rapprochements` (§5) | Extraction de références + résolution |
| Table des matières | `/consult/legi/tableMatieres` (nature CODE/LOI…), `/consult/code` (section), `/consult/getSectionByCid` | Résumé TOC par défaut, sous-arbre à la demande (retour progressif) |
| JORF | `/consult/lastNJo`, `/consult/jorfCont`, `/consult/jorf`, `/consult/jorfPart`, `/misc/datesWithoutJo` | Resource MCP « JO du jour » |
| Codes en vigueur / liste des codes | `/list/code {states:[VIGUEUR]}` | Resource MCP + completion |
| Conventions par IDCC | `/consult/kaliContIdcc`, `/list/conventions {idcc}`, `/search` KALI `typeChamp=IDCC` (exemple officiel `[RAW]/exemples…md:1279-1327`) | |
| Completions MCP | `/suggest`, `/suggest/acco`, `/suggest/pdc`, `/list/code` (cache) | |
| Diagnostics | `/*/ping`, `/misc/commitId` | `fr_status` / healthcheck HTTP |

### 1.14 Pièges de données observés (impact direct sur les DTO Rust)

1. **Formats de dates hétérogènes** (VÉRIFIÉ, fixtures) : epoch millisecondes entiers (`article.dateDebut = 17193600000` → 1970-07-19 ; `dateFin = 32472144000000` → **2999-01-01**, sentinelle « pas de fin » ; `textTitles[].dateDebut = -5231606400000` → 1804-03-21, valeurs **négatives**) ; chaînes ISO avec offset `+0000` dans `/search` (`"date": "1804-03-21T00:00:00.000+0000"`, `extracts[].dateFin: "2999-01-01T00:00:00.000+0000"`) ; chaînes `AAAA-MM-JJ` (`consult_loda.json` `dateDebutVersion: "2023-09-08"`, `dateFinVersion: "2999-01-01"`, `context.titresTM[].debut`). La spec annonce aussi `datePubliComputed`/`dateTexteComputed` « null si la date est ≥ 2222-01-01 » (`[SPEC]` `TexteSimple`). → un type `LegalDate` avec désérialiseur multi-format + `Option` pour les sentinelles 2999-01-01 et 2222-02-22 (§6.3).
2. **Format des dates en entrée** : la spec donne `date: "2021-04-15"` (chaîne) pour lawDecree/legiPart/tableMatieres ; les exemples officiels utilisent epoch ms pour `DATE_VERSION.singleDate` (`1514802418000`) et pour `legiPart.date` (`1561132975000`), mais aussi `"singleDate": "2025-04-15"` (`[RAW]/exemples…md:221,487,815`) ; le xlsx affiche des numéros de série Excel (`44368`, `44301`) — artefact de conversion (`[RAW]/description…md:416,661`). La référence Python envoie epoch ms pour `DATE_VERSION` (`[PY]/src/fr_eli_mcp/server.py:136`) et `YYYY-MM-DD` pour `lawDecree.date` (`[PY]/src/fr_eli_mcp/client.py:190-193`), tous deux validés live. **VÉRIFIÉ** pour ces deux formes ; l'acceptation de chaînes dans `singleDate` : **NON VÉRIFIÉ** (exemple officiel seulement).
3. **Identifiants versionnés** : `titles[].id = "LEGITEXT000006070721_09-04-2026"` (suffixe `_JJ-MM-AAAA`) alors que `cid = "LEGITEXT000006070721"` (`search_code.json`) ; `consult_loda.json` `id = "LEGITEXT000033205014_08-09-2023"`, `cid = "JORFTEXT000033202746"` (pour un texte non codifié, le CID LEGI est l'id JORF de la version initiale — conforme au lexique `[RAW]/lexique-api-lgf.md:39-59`). → newtypes `TextId` qui acceptent/strippent le suffixe, et distinguent `id` (version) de `cid` (chronique). **VÉRIFIÉ.**
4. **Surlignage `<mark>`** dans `titles[].title` et `extracts[].values` (`search_loda.json`, `search_kali.json`) ; la référence Python les retire (`[PY]/src/fr_eli_mcp/citations.py:45-49`). **VÉRIFIÉ.**
5. **Mojibake (double encodage UTF-8→Latin-1)** dans toutes les chaînes des fixtures LEGI et JURI (`consult_article.json`, `consult_loda.json`, `consult_juri.json`, `search_code.json`, `search_loda.json`, `search_juri.json` : « vie privÃ©e », « PubliÃ© au bulletin »), mais **pas** dans CETAT, CONSTIT, CNIL, KALI, ACCO (analyse programmatique). Cause **NON VÉRIFIÉE** (données sandbox amont ? enregistrement des fixtures ?). Swagger : « Les données ne sont pas identiques à celles de Production » (`[SPEC]` `info.description`). → décoder le corps strictement en UTF-8 depuis les octets, et prévoir une réparation heuristique optionnelle (détection de séquences `Ã[\x80-\xBF]`), testée sur ces fixtures.
6. **`typePagination` renvoyé ≠ envoyé** : `search_code.json` renvoie `typePagination: "ARTICLE"` (et `totalArticleResultNumber: 1`) alors que la référence Python envoie toujours `DEFAUT` (`[PY]/src/fr_eli_mcp/server.py:154`). Spec : « Lors de la navigation dans plusieurs pages, il est nécessaire de passer la valeur reçue dans la réponse précédente » (`[SPEC]` `RechercheSpecifiqueDTO.typePagination`). → le curseur de pagination Rust doit transporter `typePagination`. **VÉRIFIÉ** (le fait que la requête de la fixture était `DEFAUT` est une déduction : **NON VÉRIFIÉ**).
7. **Champs vides plutôt qu'absents** (`liens[].cidTexte = ""`) et `null` fréquents (`ecli: null` pour CAA/TA, `[PY]/DISCOVERY.md:80-82`). → `Option<String>` + normalisation `"" → None`.
8. **Structures de résultats par fond** : `SearchResult.type` = `data_legi`, `data_juri`, `data_kali_text`… ; `origin` = `LEGI`, `JURI`, `KALI`… ; `nature` = `code`, `LOI`, `arret`, « lettre d'adhesion »… (casse non homogène) ; pour CODE, `sections[].extracts[]{id LEGIARTI, num, values, legalStatus, dateDebut, dateFin}` (`search_code.json`). **VÉRIFIÉ.**
9. **Contradictions documentaires** : exemples officiels avec `"facette": "TEXT_LEGAL_STATUS", "valeur": "VIGUEUR"` (singulier, `[RAW]/exemples…md:457-462`) alors que `FiltreDTO` ne définit que `valeurs` (tableau) ; `"fromAdvancedRecherche": "False"` (chaîne) dans l'exemple KALI (`…md:1263`) contre booléen dans la spec. → toujours envoyer `valeurs: [..]` et un booléen. Le comportement de `valeur` singulier : **NON VÉRIFIÉ.**

---

## 2. Options de recherche (`POST /search`)

### 2.1 Structure de la requête (`[SPEC]` `definitions`, VÉRIFIÉ)

```
SearchRequestDTO   { fond*: enum, recherche*: RechercheSpecifiqueDTO }
RechercheSpecifiqueDTO {
  champs*: [ChampDTO], filtres: [FiltreDTO] (ET implicite entre filtres),
  operateur*: ET|OU (entre champs), sort*: string (dépend du fonds), secondSort: string,
  pageNumber*: int, pageSize*: int (max=100), typePagination*: DEFAUT|ARTICLE,
  fromAdvancedRecherche: bool }
ChampDTO   { typeChamp: enum, criteres: [CritereDTO], operateur: ET|OU (entre critères) }
CritereDTO { valeur*: string, typeRecherche*: enum, operateur*: ET|OU (entre sous-critères),
             proximite: int (distance max en mots), criteres: [CritereDTO] (récursif) }
FiltreDTO  { facette: string, valeurs: [string], dates: DatePeriod{start,end},
             singleDate: date-time, multiValeurs: map<string,[string]> }
```

- `fond` (enum, 14 valeurs) : `JORF`, `CNIL`, `CETAT`, `JURI`, `JUFI`, `CONSTIT`, `KALI`, `CODE_DATE`, `CODE_ETAT`, `LODA_DATE`, `LODA_ETAT`, `ALL`, `CIRC`, `ACCO`. « Pour les fonds LODA et CODE il existe deux types de recherche : la recherche par date (_DATE) de version ou la recherche par état juridique (_ETAT) ». **VÉRIFIÉ.**
- `typeRecherche` : `UN_DES_MOTS`, `EXACTE`, `TOUS_LES_MOTS_DANS_UN_CHAMP`, `AUCUN_DES_MOTS`, `AUCUNE_CORRESPONDANCE_A_CETTE_EXPRESSION`. Pour une expression : `UN_DES_MOTS` + `proximite`, ou `EXACTE` (`[RAW]/exemples…md:837-843`). **VÉRIFIÉ.**
- Critères imbriqués : exemple officiel « mineur » à 2 mots de « travail » (`[RAW]/exemples…md:903-967`) et exemple Swagger avec sous-critères et `proximite` (`[SPEC]` `RechercheSpecifiqueDTO.champs.example`). **VÉRIFIÉ.**
- `multiValeurs` : sous-valeurs d'un filtre hiérarchique, ex. `{"TRIBUNAL_ADMINISTATIF": ["Bordeaux", "Lille"]}` avec `valeurs: ["TRIBUNAL_ADMINISTATIF", "COURS_APPEL"]` (`[SPEC]` `FiltreDTO`) — correspond aux `childs` de la facette `JURIDICTION_NATURE` (fixture `search_cetat.json`). Noter la faute d'orthographe officielle **`TRIBUNAL_ADMINISTATIF`** (sans R), à reproduire telle quelle. **VÉRIFIÉ.**
- Requête « tout » : `valeur: "*"` (exemple officiel JURI janvier 2025, `[RAW]/exemples…md:1045-1119`). **VÉRIFIÉ.**

Enum global `typeChamp` (`[SPEC]` `ChampDTO.typeChamp`, 32 valeurs) : `ALL, TITLE, TABLE, NOR, NUM, ADVANCED_TEXTE_ID, NUM_DELIB, NUM_DEC, NUM_ARTICLE, ARTICLE, MINISTERE, VISA, NOTICE, VISA_NOTICE, TRAVAUX_PREP, SIGNATURE, NOTA, NUM_AFFAIRE, ABSTRATS, RESUMES, TEXTE, ECLI, NUM_LOI_DEF, TYPE_DECISION, NUMERO_INTERNE, REF_PUBLI, RESUME_CIRC, TEXTE_REF, TITRE_LOI_DEF, RAISON_SOCIALE, MOTS_CLES, IDCC`. **VÉRIFIÉ.**

### 2.2 Champs, filtres et tris par fonds (document officiel « description des tris et filtres »)

Source : `[RAW]/description-des-tris-et-filtres-de-l-api.md` (conversion du xlsx officiel ; feuille CODE re-vérifiée directement dans le xlsx). Colonnes : champs de recherche (`typeChamp`) | filtres (`facette`) | tris (`sort`). **VÉRIFIÉ** sauf mention.

| Fonds | `typeChamp` | `facette` (filtres) | `sort` | Lignes |
|---|---|---|---|---|
| `ALL` | ALL, TITLE | FOND (champ `origine`) | — (non listé) | 26-32 |
| `JORF` | ALL, TITLE, NOR, NUM, NUM_ARTICLE, ARTICLE, VISA, NOTICE, VISA_NOTICE, TRAVAUX_PREP, SIGNATURE, NOTA | NATURE, DATE_SIGNATURE, DATE_PUBLICATION, MINISTERE, EMETTEUR, NOR, NUM_TEXTE, NUM_ARTICLE, DECORATION, DELEGATION | PERTINENCE, SIGNATURE_DATE_DESC/ASC, PUBLICATION_DATE_DESC/ASC | 34-65 |
| `CODE_DATE` | ALL, TITLE, TABLE, NUM_ARTICLE, ARTICLE | DATE_VERSION, NOM_CODE, NUM_ARTICLE | **aucun tri listé** dans la feuille officielle | 67-80 |
| `CODE_ETAT` | idem | ARTICLE_LEGAL_STATUS, TEXT_LEGAL_STATUS, TEXT_NOM_CODE, NUM_ARTICLE | **aucun tri listé** | 67-80 |
| `LODA_DATE` | ALL, TITLE, NOR, NUM, NUM_ARTICLE, ARTICLE, VISA, NOTICE, VISA_NOTICE, TRAVAUX_PREP, SIGNATURE, NOTA | NATURE, DATE_SIGNATURE, DATE_PUBLICATION, NOR, NUM_TEXTE, NUM_ARTICLE, DATE_VERSION | PERTINENCE, PUBLICATION_DATE_DESC/ASC, SIGNATURE_DATE_DESC/ASC | 82-112 |
| `LODA_ETAT` | idem | TEXT_NATURE, TEXT_DATE_SIGNATURE, TEXT_DATE_PUBLICATION, ARTICLE_LEGAL_STATUS, TEXT_LEGAL_STATUS, NOR, NUM_TEXTE, NUM_ARTICLE | idem | 82-112 |
| `CIRC` | ALL, TITLE, NOR, RESUME_CIRC, TEXTE_REF | DATE_MEL, DATE_SIGNATURE, DATE_MEA, OPPOSABILITE, DOMAINE, MIN_DEPOSANT, MIN_CONCERNE, MOTS_CLEFS, NUMERO_INTERNE, NOR, REF_PUBLI | PERTINENCE, SIGNATURE_DATE_DESC/ASC, PUBLI_DATE_DESC/ASC | 114-139 |
| `CONSTIT` | ALL, TITLE, NUM_DEC, TEXTE | NATURE_CONSTIT, NATURE_NORME_AUTRE, SOLUTION_CONSTIT, TITRE_DEFEREE, NUM_LOI, DATE_LOI, TYPE_DECISION, SOLUTION_ELECT, NATURE_AUTRE, SOLUTION_AUTRE, DATE_DECISION, NUMERO_DECISION, NOR | PERTINENCE, DATE_DESC, DATE_ASC | 141-165 |
| `CETAT` | ALL, TITLE, NUM_DEC, ABSTRATS, TEXTE, RESUMES | JURIDICTION_NATURE, DATE_DECISION, DATE_VERSEMENT, PUBLICATION_RECUEIL, NUMERO_DECISION, ECLI | PERTINENCE, DATE_DESC, DATE_ASC | 167-186 |
| `JURI` | ALL, TITLE, ABSTRATS, NUM_AFFAIRE, TEXTE, RESUMES | JURIDICTION_JUDICIAIRE, CASSATION_TYPE_PUBLICATION_BULLETIN, NUM_BULLETIN, ANNEE_BULLETIN, CASSATION_NATURE_DECISION, CASSATION_FORMATION, CASSATION_DECISION_ATTAQUEE, LIEU_DECISION, DATE_DECISION_ATTAQUEE, APPEL_SIEGE_APPEL, PREMIER_DEGRE_TYPE_JURIDICTION, PREMIER_DEGRE_SIEGE, DATE_DECISION, NUM_AFFAIRE, ECLI | PERTINENCE, DATE_DESC, DATE_ASC | 188-216 |
| `JUFI` | ALL, TITLE, NUM_DEC, ABSTRATS, TEXTE | PUBLICATION_RECUEIL, JURIDICTION_NATURE, DATE_DECISION, NUMERO_DECISION | PERTINENCE, DATE_DESC, DATE_ASC | 218-234 |
| `KALI` | ALL, TITLE, IDCC, MOTS_CLES, ARTICLE | LEGAL_STATUS, ARTICLE_LEGAL_STATUS, ACTIVITE, ARTICLE_QUESTION_USUELLE, DATE_SIGNATURE, DATE_PUBLICATION, NATURE_TEXTE_CITE, NUM_TEXTE_CITE, DATE_PUBLI_TEXTE_CITE, NOM_CODE_CITE, NUM_ARTICLE_CODE_CITE, IDCC, NOR, CODE_NAF_OU_APE, NUMERO_BO, ARTICLE_NUMERO | PERTINENCE, MODIFICATION_DATE_DESC, SIGNATURE_DATE_DESC/ASC | 236-265 |
| `ACCO` | ALL, TITLE, RAISON_SOCIALE, IDCC | THEME, SIGNATAIRE, DATE_SIGNATURE, ACTIVITE_PRINCIPALE, CODE_APE, IDCC, VILLE, CODE_POSTAL, DATE_DIFFUSION, SIRET_RAISON_SOCIALE | PERTINENCE, DATE_DESC, DATE_ASC | 267-288 |
| `CNIL` | ALL, TITLE, NOR, TEXTE, NUM_DELIB | TYPE, NATURE_DELIB, DATE_DELIB, NUMERO_DELIB, NOR | PERTINENCE, DATE_DECISION_DESC, DATE_DECISION_ASC | 290-307 |

`secondSort` : exemples officiels `"ID"` (LODA, JURI par numéro d'affaire), `"DATE_DESC"` (CETAT, JURI), `"PERTINENCE"` (KALI) (`[RAW]/exemples…md:959,1037,1111,1163,1271`). Liste exhaustive des valeurs de `secondSort` : **NON VÉRIFIÉ.**

Points de vigilance :

- **Tris CODE** : la feuille officielle n'en liste aucun ; tous les exemples officiels CODE utilisent `PERTINENCE` (`…exemples…md:233,755,827,895`). `[JRX]` autorise aussi `PUBLICATION_DATE_DESC/ASC` pour CODE (`[JRX]/src-tauri/src/juridique/legifrance.rs:119-123`). **NON VÉRIFIÉ** → n'autoriser que `PERTINENCE` pour CODE_* tant que non testé.
- **NOM_CODE** : la doc officielle utilise `NOM_CODE` avec `CODE_DATE` et `TEXT_NOM_CODE` avec `CODE_ETAT`, valeur = titre exact du code (« Code des postes et des communications électroniques », « Code civil », « Code pénal ») (`…exemples…md:203-223,735-745,797-809,875-885`). Deux clients tiers divergent : `[JRX]` réécrit `NOM_CODE` → `TEXT_NOM_CODE` sur CODE_DATE/CODE_ETAT (« anti-500 », `…legifrance.rs:36-37,414-418`) ; `[KMCP]` déconseille `NOM_CODE` (« cause systématiquement un 500 avec du texte libre », `[KMCP]/server.py:802-805`). **NON VÉRIFIÉ — test sandbox prioritaire.** Recommandation : ne passer que des titres issus de `/list/code` (valeurs exactes), jamais du texte libre.
- **Facettes inexistantes / champs inconnus → HTTP 500** (pas 400) selon `[JRX]` (`…legifrance.rs:166-169,207-210,1345-1350` : « un champ inconnu dans le body fait planter PISTE en 500 (« Une exception non gérée est survenue ») »). **NON VÉRIFIÉ** mais plausible : valider fond×facette×sort côté domaine (enums Rust par fonds) avant envoi, et ne jamais réessayer un 500 dû à la requête plus d'une fois.
- **Facettes de date par fonds** (synthèse des sources officielles ci-dessus) : CODE_DATE `DATE_VERSION` (`singleDate`) ; CODE_ETAT aucune ; LODA_DATE `DATE_SIGNATURE`/`DATE_PUBLICATION`/`DATE_VERSION` ; LODA_ETAT `TEXT_DATE_SIGNATURE`/`TEXT_DATE_PUBLICATION` ; JORF `DATE_SIGNATURE`/`DATE_PUBLICATION` ; CONSTIT/CETAT/JURI/JUFI `DATE_DECISION` (CETAT aussi `DATE_VERSEMENT`) ; CNIL `DATE_DELIB` ; KALI `DATE_SIGNATURE`/`DATE_PUBLICATION` ; ACCO `DATE_SIGNATURE`/`DATE_DIFFUSION` ; CIRC `DATE_SIGNATURE`/`DATE_MEL`/`DATE_MEA`. Les plages utilisent `dates {start, end}` (`AAAA-MM-JJ`), les dates ponctuelles `singleDate`. **VÉRIFIÉ.**
- `LODA_DATE`/`CODE_DATE` **exigent** un filtre `DATE_VERSION` `singleDate` (constat live `[PY]/DISCOVERY.md:22-25`). **VÉRIFIÉ (live tiers = réf. Python).**

### 2.3 Valeurs de facettes observées (réponses réelles, `[PY]/tests/fixtures/search_*.json`)

Format d'une facette (`[SPEC]` `Facet`) : `{facetElem (nom à réutiliser comme `facette`), field (champ technique), values: map<libellé, count>, childs: map<valeur, Facet>, totalElement}`. Valeurs observées (VÉRIFIÉ) :

- **LODA** : `NATURE` ∈ {ARRETE, DECLARATION, DECISION, LOI, ORDONNANCE, DECRET, DECRET_LOI, CONVENTION, CONSTITUTION, DELIBERATION, ACCORD_FONCTION_PUBLIQUE} ; `ARTICLE_LEGAL_STATUS` (champ `articles.etat`) ∈ {ABROGE, ABROGE_DIFF, ANNULE, DENONCE, DISJOINT, INITIALE, MODIFIE, MODIFIE_MORT_NE, PERIME, REMPLACE, SANS_ETAT, SUBSTITUE, TRANSFERE, VIGUEUR, VIGUEUR_DIFF, VIGUEUR_ETEN, VIGUEUR_NON_ETEN} (= les 17 valeurs de l'enum `legalStatus` de la spec).
- **JURI** : `JURIDICTION_JUDICIAIRE` ∈ {Cour de cassation, Juridictions d'appel, Juridictions du premier degré} ; `CASSATION_TYPE_PUBLICATION_BULLETIN` ∈ {T, F} ; `CASSATION_NATURE_DECISION` ∈ {avis, ordonnance, arret, autres_decisions} (l'exemple officiel filtre avec `"ARRET"` en majuscules, `[RAW]/exemples…md:1079-1085` — casse acceptée **NON VÉRIFIÉ**) ; `CASSATION_FORMATION` ∈ {AVIS, ORDONNANCE_PREMIER_PRESIDENT, COUR_REVISION, CHAMBRE_CIVILE_1, CHAMBRE_CIVILE_2, CHAMBRE_CIVILE_3, CHAMBRE_CIVILE, CHAMBRE_COMMERCIALE, CHAMBRE_SOCIALE, CHAMBRE_CRIMINELLE, CHAMBRE_MIXTE, ASSEMBLEE_PLENIERE, CHAMBRES_REUNIES, COMMISSION_REPARATION_DETENTION, COMMISSION_REVISION, TRIBUNAL_CONFLIT} ; `CASSATION_DECISION_ATTAQUEE` (19 valeurs : CONSEIL_PRUDHOMME, COUR_APPEL, COUR_ASSISES, TRIBUNAL_COMMERCE, TRIBUNAL_CORRECTIONNEL, TRIBUNAL_GRANDE_INSTANCE, …) ; `APPEL_SIEGE_APPEL` (35 sièges : PARIS, VERSAILLES, LYON, AIX-PROVENCE…) ; `PREMIER_DEGRE_TYPE_JURIDICTION` (19 libellés) ; `PDC_CHECKBOX_RESTREINDRE_ARRET` (vide).
- **CETAT** : `JURIDICTION_NATURE` ∈ {CONSEIL_ETAT, COURS_APPEL, TRIBUNAL_ADMINISTATIF, TRIBUNAL_CONFLIT, COURS_DE_DISCIPLINE, COURS_APPEL_FINANCIERE, COURS_COMPTES, CHAMBRES_COMPTES} avec `childs` : COURS_APPEL → {Douai, Nantes, Bordeaux, Marseille, Lyon, Versailles, Nancy, Toulouse, Paris} ; TRIBUNAL_ADMINISTATIF → 42 villes ; `PUBLICATION_RECUEIL` ∈ {PUBLIE, NON_PUBLIE} ; `pdcRestreindreArret`.
- **CONSTIT** : `NATURE_CONSTIT` (8 : « Dispositions promulguées (QPC) », « Loi ordinaire (DC) », « Loi organique (DC) », « Traité (DC) », « Loi du pays (LP) », « Referendum d'initiative partagée (RIP) »…) ; `SOLUTION_CONSTIT` (34) ; `SOLUTION_ELECT` (12) ; `SOLUTION_AUTRE` (4) ; `NATURE_AUTRE` (5) ; `NATURE_ELCT` (5) ; `NATURE_NORME_AUTRE` (2). Noter `NATURE_ELCT` dans la réponse vs `TYPE_DECISION` dans le xlsx pour le contentieux électoral.
- **CNIL** : `TYPE` ∈ {Décision, Délibération} ; `NATURE_DELIB` (11 : Avis, Sanction, Mise en demeure, Autorisation de transfert…).
- **KALI** : `LEGAL_STATUS` et `ARTICLE_LEGAL_STATUS` ∈ {ABROGE, DENONCE, MODIFIE, PERIME, REMPLACE, VIGUEUR, VIGUEUR_ETEN, VIGUEUR_NON_ETEN} ; `TEXTE_BASE` ; `ACTIVITE` (551 libellés).
- **ACCO** : `SIGNATAIRE` ∈ {CFDT, CFE-CGC, CFTC, CGT, CGT-FO, SOLIDAIRES, UNSA, Autre} ; `THEME` (59 libellés).
- **CODE_DATE** : la réponse ne contient **aucune** facette (`search_code.json`).

Conséquence : les valeurs de facettes **sont découvrables à l'exécution** (chaque réponse les renvoie). Le serveur Rust peut (a) proposer ces valeurs en completion MCP, (b) valider les filtres contre un cache de facettes par fonds. **VÉRIFIÉ.**

### 2.4 Pagination et limites

- `pageSize` max **100** (`[SPEC]` `RechercheSpecifiqueDTO.pageSize` « (max=100) », toutes les `/list/*` « (max 100) », `jorfCont.pageSize` « (max 100) »). **VÉRIFIÉ.** Un exemple officiel utilise `pageSize: 100` (CETAT, `[RAW]/exemples…md:1027`).
- `pageNumber` commence à **1** (exemples officiels, `[PY]/src/fr_eli_mcp/server.py:150`). Le xlsx indique « 0 (valeur par défaut) » pour `jorfCont.pageNumber/pageSize` (`[RAW]/description…md:570-571`). Clamp tiers `pageNumber ≥ 1`, `pageSize ∈ [1,100]` (`[JRX]/…/legifrance.rs:364-366`). **VÉRIFIÉ / tiers.**
- `typePagination` : `DEFAUT` ou `ARTICLE` (spécifique aux recherches « dans les articles d'un texte ») ; réutiliser la valeur reçue (§1.14 pt 6). **VÉRIFIÉ.**
- **Pagination profonde** : aucune limite documentée pour Légifrance `/search`. Le moteur est Elasticsearch (indices : `idTechInjection: "index_DATA_NEXT_ARTICLE_…"` dans la fixture article ; paramètre `page` de `getJuriPlanClassement` décrit comme « la requete ELK » dans la spec), dont la fenêtre par défaut est de 10 000 résultats — limite appliquée explicitement par Judilibre (§5). Pour Légifrance : **NON VÉRIFIÉ**. Recommandation : plafonner `pageNumber × pageSize ≤ 10 000` côté serveur et inciter à filtrer (dates) au-delà ; `[KMCP]` avertit l'utilisateur au-delà d'un seuil de résultats (`[KMCP]/server.py:1393`).
- Volumétrie indexée (facette `FOND`, fixtures `search_acco.json`/`search_cetat.json`, juin–juillet 2026) : JORF ≈ 1 125 515 ; JURI ≈ 605 651 ; CETAT ≈ 569 322 ; ACCO ≈ 387 844 ; LEGI ≈ 117 230 ; CNIL ≈ 26 759 ; KALI ≈ 21 911 ; CIRC ≈ 10 646 ; CONSTIT ≈ 7 372 ; JUFI ≈ 3 502 ; CODE = 76. **VÉRIFIÉ** (sandbox ; prod peut différer).

---

## 3. Authentification, environnements, quotas, erreurs

### 3.1 OAuth2 PISTE (VÉRIFIÉ)

- Flux **client_credentials** (RFC 6749 §4.4) (`[RAW]/exemples…md:107`).
- Token sandbox : `POST https://sandbox-oauth.piste.gouv.fr/api/oauth/token` (`[RAW]/exemples…md:115`, `[PY]/src/fr_eli_mcp/client.py:28`). Token prod : `POST https://oauth.piste.gouv.fr/api/oauth/token` (`[SPEC]` `securityDefinitions.SecurityProfile.OAuthAccessCode.tokenUrl` ; `[PLF]/pylegifrance/config.py:24` ; `[JRX]/src-tauri/src/juridique/client.rs:19`).
- Corps `application/x-www-form-urlencoded` : `grant_type=client_credentials&client_id=…&client_secret=…&scope=openid` (`[RAW]/exemples…md:119,129`).
- Réponse : `{"access_token": "...", "token_type": "Bearer", "expires_in": 3600, "scope": "openid"}` (`[RAW]/exemples…md:135-151`) ; observé en sandbox : scope `openid resource.READ` (`[PY]/DISCOVERY.md:12`). « expires_in correspond au délai d'expiration du jeton en seconde » (`…md:151`).
- Appels API : en-tête `Authorization: Bearer <access_token>` ; pour les POST, en plus `accept: application/json` et `Content-Type: application/json` (`[RAW]/exemples…md:155-163`).
- La spec déclare aussi des profils `implicit` / `accessCode` avec `authorizationUrl https://oauth.piste.gouv.fr/api/oauth/authorize` et scope `openid` (`[SPEC]` `securityDefinitions`) — inutiles pour un serveur.
- Exemples OAuth supplémentaires « sur le Gitlab de PISTE, après inscription » (`[RAW]/exemples…md:109`).
- Pratiques de la référence Python à conserver : renouvellement 60 s avant expiration, invalidation + 1 retry sur 401, verrou pour éviter les rafraîchissements concurrents, token jamais persisté (`[PY]/src/fr_eli_mcp/client.py:33-35,61-99,164-168`). Même marge de 60 s chez `[JRX]` (`client.rs:25`) et `[JMT]` (`api_judilibre.py:84`, `expires_in - 60`).

### 3.2 Sandbox vs production

| Aspect | Constat | Statut |
|---|---|---|
| Surface API | Identique (Swagger 2.4.2 identique dans les deux copies) | VÉRIFIÉ |
| Données | « Les données ne sont pas identiques à celles de Production » ; « L'environnement de test peut être indisponible » ; « Les mises à jour correctives et évolutives sont réalisées dans un premier temps sur cet environnement » (`[SPEC]` `info.description`) | VÉRIFIÉ |
| Contradiction tierce | `[KMCP]/README.md:67` : « L'environnement Sandbox et l'environnement Production utilisent les mêmes données. La différence porte uniquement sur les quotas » | Contredit par la source officielle → **rejeté** |
| Disponibilité | Prod : « engagement de service de 95 % par jour », engagement de moyens, aucun niveau minimal garanti ; sandbox : aucun engagement (tests de performance, réindexation…), « peut donc être indisponible sans information préalable » (`[RAW]/cgu-legifrance-api.md:187-202`) | VÉRIFIÉ |
| Identifiants | Applications PISTE distinctes : des identifiants sandbox sur `oauth.piste.gouv.fr` → `invalid_client` (`[PY]/SOURCES.md:12`) ; `[JMT]` utilise des variables séparées `PISTE_SANDBOX_CLIENT_ID` / `PISTE_CLIENT_ID` (`[JMT]/api_judilibre.py:35-44`) | VÉRIFIÉ (live tiers) |
| Quotas | Différents selon l'environnement d'après `[KMCP]` | NON VÉRIFIÉ |
| Historique | « version stable de l'API disponible depuis le 4 avril 2023 ; version bêta fermée définitivement depuis le 6 juin 2023 » (résumé WebSearch, page probable https://www.legifrance.gouv.fr/contenu/pied-de-page/open-data-et-api) | NON VÉRIFIÉ (non recoupé) |

Recommandation : config `PisteEnv { Sandbox, Production }` qui fixe ensemble l'URL OAuth et la base API (+ surcharge explicite par variables d'environnement comme `FR_ELI_OAUTH_URL` / `FR_ELI_BASE_URL`, `[PY]/src/fr_eli_mcp/client.py:42-47`), pour empêcher le mélange sandbox/prod.

### 3.3 Quotas / limites de débit

- **Officiel** : quotas « par seconde / minute / jour », sur « le nombre de requête » ou « la bande passante », appliqués « pour l'ensemble de l'application consommatrice, soit pour une méthode particulière » ; consultables dans PISTE : onglet « Applications » → application → « Actions » → « Consulter les quotas » ; modifiables à tout moment par la DILA avec information par email (`[RAW]/cgu-legifrance-api.md:212-231`). Swagger : « Les appels sont limités par quotas » (`[SPEC]` `info.description`). **VÉRIFIÉ.**
- **Valeurs numériques** : non publiques ; aucune source officielle trouvée (WebSearch : https://packagist.org/packages/omnilex/legifrance indique « detailed on the PISTE portal »). **NON VÉRIFIÉ.**
- Choix des clients tiers (indicatifs, pas des quotas) : `[PLFMCP]` décorateur 5 appels/s (`src/server.py:272`) alors que son README dit 10/s (`README.md:236`) ; omnilex throttle 0,5 s (résumé WebSearch, packagist) ; `[JRX]` concurrence 4 pour la vérification (`verify.rs:68-69`) ; tests `[PLF]` : 200 ms entre appels + pause 500 ms entre tests (`tests/integration/conftest.py:6-14,28-30`) ; `[LCX]` « politeness: <= 4 connections » pour les dumps (`scripts/fetch.py`, `ThreadPoolExecutor(max_workers=4)`).
- **Recommandation** : rate limiter configurable (crate `governor` 0.10.4, §6.7) global au processus (les quotas sont par application, donc partagés entre tous les utilisateurs du serveur HTTP) + sémaphore de concurrence (ex. 4) pour `fr_search_all` ; valeurs par défaut prudentes (ex. 2–5 req/s) **à calibrer** sur la page « Consulter les quotas ».

### 3.4 Codes d'erreur et signification

| Code | Signification connue | Source | Statut |
|---|---|---|---|
| 401 | Jeton absent/expiré/invalide → invalider et réessayer une fois | `[PY]/src/fr_eli_mcp/client.py:164-168` | VÉRIFIÉ (live) |
| 401 `{"error":"invalid_client"}` | (a) identifiants d'un autre environnement (`[PY]/SOURCES.md:12`) ; (b) « très courtes fenêtres de throttle où PISTE refuse un jeton valide un instant plus tôt » observées en CI (`[PLF]/tests/integration/conftest.py:8-17`) ; à l'obtention du jeton : identifiants invalides ou abonnement expiré (`[PLF]/docs/src/content/docs/concepts/piste-oauth.md:41-42`) | tiers | (a) VÉRIFIÉ live tiers ; (b) NON VÉRIFIÉ |
| 403 | Application non abonnée à l'API (ex. Judilibre avec un jeton Légifrance seul, `[PY]/DISCOVERY.md:89-93`, `[PY]/SOURCES.md:12`) ; message tiers « Votre compte n'est probablement pas abonné à l'API Légifrance » (`[JMT]/api_legifrance.py:174`) | live tiers | VÉRIFIÉ |
| 404 | Ressource inconnue (mappée en `not_found`, `[PY]/src/fr_eli_mcp/server.py:114-115`) ; aussi mauvais chemin/méthode (ex. GET /suggest, `[JRX]/…/legifrance.rs:8-11`) | | VÉRIFIÉ |
| 429 | Dépassement de quota (`[PLF]/docs/…/api-terms-and-quotas.md:47-48` ; `[KMCP]/server.py:303-304`) ; la référence Python réessaie 429/500/502/503/504 avec backoff 0,5·2^n, 3 tentatives (`client.py:33-34,175-181`) ; `[JL]` attend 60 s sur 429 Judilibre (`export_piste_v2.py:107-109`) ; présence d'un en-tête `Retry-After` | tiers | 429 = quota : VÉRIFIÉ (tiers concordants) ; `Retry-After` : NON VÉRIFIÉ |
| 500 | Erreur serveur ; selon `[JRX]`, aussi déclenchée par facette inexistante, champ JSON inconnu, `fond=ALL` avec filtres (« Une exception non gérée est survenue ») | `[JRX]/…/legifrance.rs:36-37,166-169,1345-1350` | NON VÉRIFIÉ |
| 400 | Judilibre avec `KeyId` invalide ou sans authentification | `[PY]/SOURCES.md:12` | VÉRIFIÉ (live tiers) |
| 416 | Judilibre `/export` : partition trop grande (> 10 000) | `[JL]/export_piste_v2.py:110-111` | tiers |

Recommandation d'erreurs Rust (thiserror) : distinguer `Auth` (401/invalid_client), `NotSubscribed` (403), `NotFound` (404), `RateLimited { retry_after: Option<Duration> }` (429), `BadRequestSuspected` (500 répété sur requête identique), `Upstream` (5xx transitoires), `Transport`, `Decode` (JSON/encodage) ; ne jamais logger le secret ni le jeton (secrecy).

---

## 4. CGU PISTE / Légifrance

Sources : CGU API Légifrance **v1.0 du 15/12/2022** (canonique) `[RAW]/cgu-legifrance-api.md` (PDF original : https://www.legifrance.gouv.fr/contenu/Media/files/pied-de-page/cgu-legifrance-api-vf-15-12-2022_0.pdf, d'après `[PLF]/docs/raw/README.md` et WebSearch) ; CGU Beta v1.1 du 04/08/2020 (= https://piste.gouv.fr/images/cgu/DILA_Legifrance_Beta_v2.pdf) `[RAW]/cgu-piste-legifrance-beta.md`. Les CGU PISTE générales (portail AIFE) n'ont pas pu être lues (seul un résumé WebSearch de https://piste.gouv.fr/images/com_apiportal/CGU/cgu_portal_EN.pdf, texte en français, décrivant le périmètre). **VÉRIFIÉ** sauf mention.

### 4.1 Obligations de l'utilisateur de l'API (v2022)

- Documents intégrés au contrat : mentions légales Légifrance, CGU PISTE, CGU API Légifrance, **Licence Ouverte Etalab 2.0** ; en cas de contradiction les CGU API Légifrance prévalent (`cgu-legifrance-api.md:94-104`).
- Âge : « majeur numérique en France et avoir au minimum 15 ans » (`:110-111`).
- Acceptation en deux temps : inscription PISTE puis choix de l'API dans le catalogue (`:115-123`) ; gratuit (`:159-169`).
- Identifiants conformes à l'état de l'art (CNIL, ANSSI) ; **« L'accès à PISTE se fait par un compte nominatif »** ; OAuth 2.0 « dans chaque Requête » ; **« ATTENTION : vos clés d'authentification à l'API Légifrance sont strictement réservées à votre application et ne doivent en aucun cas être rendues publiques ou divulguées à un tiers »** (`:171-183`).
- Responsabilité exclusive de l'usage, conformité à la Licence Ouverte 2.0 **et aux règles de protection des données personnelles**, obligation d'indemniser la DILA (`:235-246`).
- Sécurité des postes (antivirus, correctifs, pare-feu, contrôle des AC, pas de mots de passe mémorisés dans le navigateur, verrouillage de session) (`:255-272`).
- Compromission (« surconsommation sur l'API Légifrance, publication des identifiants en ligne, etc. ») : réinitialiser les identifiants sur PISTE et prévenir DILA/AIFE (`:274-285`).
- Suspension sans préavis et résiliation de plein droit en cas de manquement (`:336-345`). Notifications : email du compte PISTE ↔ `retours-legifrance-modernise@dila.gouv.fr` (`:347-353`).
- Données personnelles collectées par la DILA via PISTE : email + statistiques agrégées d'usage (conservées 12 mois max) (`:367-390`) ; contact `rgpd@dila.gouv.fr` (`:400-402`).

### 4.2 Droits de réutilisation, mention de source, redistribution, usage commercial

- « Les données mises à disposition par l'interface de programmation sont soumises à la licence ouverte 2.0 » (`cgu-legifrance-api.md:360-365`) ; le Swagger déclare aussi `license: Licence ouverte v2.0` (`[SPEC]` `info.license`).
- Usage commercial autorisé, « au risque et péril de l'Utilisateur » ; la DILA n'est responsable d'aucun dommage d'une application fondée sur ces données (`:318-332`).
- Données **non garanties** (contenu, complétude, fraîcheur) et **non opposables** : « seul les PDF signés publiés au Journal officiel de la République Française sont opposables » (`:289-316`) → bandeau/avertissement obligatoire dans nos réponses (le serveur actuel a déjà une logique de citation).
- **Licence Ouverte 2.0 — mention de paternité** : la source doit mentionner au minimum le nom du Concédant et **la date de dernière mise à jour** de l'information réutilisée ; un lien URL vers l'information suffit s'il assure une attribution effective ; l'attribution ne doit pas suggérer un aval du Concédant ni induire en erreur sur le contenu, la source ou la date de mise à jour (résumé WebSearch du texte https://www.etalab.gouv.fr/wp-content/uploads/2017/04/ETALAB-Licence-Ouverte-v2.0.pdf et https://www.data.gouv.fr/pages/legal/licences/etalab-2.0). **VÉRIFIÉ (via résumé)**. Formule d'attribution utilisée par un tiers : « Source : DILA — base LEGI, Légifrance » (`[LCX]/README.md:85-88`).
- Redistribution/copie/stockage : la Licence Ouverte autorise copie, redistribution et réutilisation (y compris commerciale) sous condition d'attribution (même source). Les CGU API **ne contiennent aucune clause** limitant le cache ou le stockage local des réponses (lecture intégrale de `[RAW]/cgu-legifrance-api.md`). **VÉRIFIÉ (absence de clause).** `[PLF]` recommande même un snapshot/cache local des données citées pour un produit commercial (`[PLF]/docs/…/api-terms-and-quotas.md:77-81`).

### 4.3 Points pertinents pour le mode serveur HTTP partagé

Lecture des textes (pas un avis juridique) :

1. Le cocontractant est l'« Utilisateur » inscrit sur PISTE (personne physique ou morale) qui consomme l'API (`cgu-legifrance-api.md:75-77`) ; les clés sont « réservées à votre application » et ne doivent pas être « divulguées à un tiers » (`:182-183`). Un serveur HTTP qui appelle PISTE **avec les identifiants de son opérateur** et sert des utilisateurs finaux n'expose pas les clés : c'est l'application de l'opérateur qui consomme l'API, ce qui ressemble au modèle de réutilisation encouragé (`:152-157`). Des services tiers fonctionnent ainsi (proxy `lab.dassignies.law` avec sa propre clé, `[PLFMCP]/src/server.py:41-44`, `README.md:38-44` ; service public `justicelibre.org/mcp`, d'après WebFetch de https://github.com/Dahliyaal/justicelibre). **Interprétation — NON VÉRIFIÉ juridiquement.**
2. **Interdits pratiques** pour notre design : ne jamais renvoyer le `client_secret` ni le jeton aux clients MCP ; ne pas permettre à un client distant de lire la config ; journaux expurgés.
3. **Quotas partagés** : un seul quota par application, commun à tous les utilisateurs du serveur → rate limiting global et, idéalement, équité par session/utilisateur.
4. **Alternative « BYO credentials »** (chaque utilisateur fournit ses propres identifiants PISTE) : conforme au « compte nominatif » mais le serveur manipulerait des secrets de tiers (stockage par session, jamais sur disque). Option de conception à arbitrer.
5. **Responsabilité** : l'opérateur reste « seul responsable de l'usage » et doit respecter la protection des données personnelles (`:237-246`) → audit JSONL (déjà prévu), TTL de cache, pas de profilage.
6. **Contenus sensibles** : champ `dereferenced` (« Identifie si le contenu est référençable par les robots d'indexation ») présent dans les réponses consult ; champ `inap` (informations nominatives à accès protégé, uniquement dans JORF, « n'ont pas vocation à être consolidés », art. R. 221-15 et R. 221-16 CRPA, `[RAW]/lexique-api-lgf.md:147-149`). Recommandation : ne pas indexer/exposer publiquement (mode HTTP, futur index tantivy) les contenus `dereferenced=true` ou `inap=true`. Obligation exacte : **NON VÉRIFIÉ.**

### 4.4 Différences v2020 (Beta) → v2022

v2020 : API « actuellement en bêta », aucune disponibilité garantie (`[RAW]/cgu-piste-legifrance-beta.md:185`) ; quotas consultables via « API Sélectionnées → Actions → Consulter les quotas » (`:216`) ; acceptation pour majeurs ou mineurs autorisés ; contact DPO `dpd@pm.gouv.fr`. v2022 : objectif 95 %/jour en prod, licence explicitement intégrée, âge 15 ans, notifications par email, contact `rgpd@dila.gouv.fr` (synthèse concordante `[PLF]/docs/src/content/docs/concepts/api-terms-and-quotas.md:18-116`). **VÉRIFIÉ.**

---

## 5. Judilibre (API séparée de la Cour de cassation)

### 5.1 Accès et souscription (VÉRIFIÉ)

- Endpoints : sandbox `https://sandbox-api.piste.gouv.fr/cassation/judilibre/v1.0`, prod `https://api.piste.gouv.fr/cassation/judilibre/v1.0` (`[JDL]/README.md:41-44`).
- Souscription PISTE **distincte** de Légifrance : valider les CGU en cherchant « Judilibre » (sandbox et/ou prod), puis dans l'application sandbox cocher l'API Judilibre (`[JDL]/README.md:26-35`). Constat live : jeton Légifrance valide → 403 sur Judilibre ; demander un scope `cassation`/`judilibre` est ignoré (`[PY]/SOURCES.md:12`, `[PY]/DISCOVERY.md:89-93`).
- Authentification : le README et l'OpenAPI 3 décrivent un en-tête **`KeyId`** (clé d'API de l'application, `securitySchemes.access {type: apiKey, name: KeyId, in: header}`, `[JDL]/public/JUDILIBRE-public.json` ; exemple curl `[JDL]/README.md:52-53`). Les clients tiers utilisent un **jeton OAuth Bearer PISTE** (même flux client_credentials) : `[JRX]/src-tauri/src/juridique/client.rs:23,212,240` (`bearer_auth`), `[JL]/export_piste_v2.py:36-48,96-101`, `[JMT]/api_judilibre.py:46,91-97`. Les deux modes semblent acceptés par PISTE. **VÉRIFIÉ (sources concordantes)** ; nous recommandons Bearer pour mutualiser le `TokenManager`.
- Conditions : Licence Ouverte 2.0 + CGU PISTE + « conditions générales d'utilisation pour la réutilisation des données issues des décisions de justice de l'ordre judiciaire diffusées en open data par la Cour de cassation » + quotas (`[JDL]/README.md:15-22`). CGU de la Cour de cassation (version du 6 janvier 2022) : réutilisation subordonnée à l'acceptation de la Licence Ouverte et des conditions ; décisions **pseudonymisées** (occultation des nom et prénoms des parties et tiers, art. R. 111-10 COJ) (résumé WebSearch, https://piste.gouv.fr/images/cgu/CGU_open_data_V8.pdf). **NON VÉRIFIÉ (document non lu).**
- Interdiction légale de réutiliser les données d'identité des magistrats et membres du greffe pour « évaluer, analyser, comparer ou prédire leurs pratiques professionnelles » (5 ans d'emprisonnement, 300 000 €), jugée conforme par le Conseil constitutionnel (résumé WebSearch : https://next.ink/6972/107769-open-data-decisions-justice-en-depit-contestations-feu-vert-conseil-constitutionnel/, https://www.lemondedudroit.fr/decryptages/65390-pour-bien-juger-jugez-caches.html). Article exact (art. 33 loi n° 2019-222, codifié L. 10 CJA / L. 111-13 COJ) : **NON VÉRIFIÉ** (mémoire). → aucune feature d'agrégation par magistrat ; s'applique aussi aux champs `president`/`rapporteur` de Légifrance JURI.

### 5.2 Endpoints (OpenAPI 3.0.2 v1.2.5, `[JDL]/public/JUDILIBRE-public.json`, VÉRIFIÉ)

| Endpoint | Paramètres | Notes |
|---|---|---|
| `GET /search` | `query`, `field[]`, `operator` (or par défaut, and, exact), `type[]`, `theme[]`, `chamber[]`, `formation[]`, `jurisdiction[]`, `location[]`, `publication[]`, `solution[]`, `date_start`, `date_end` (ISO-8601), `sort` (score, scorepub, date), `order` (asc/desc), `page_size` (**max 50**, défaut 10), `page` (**base 0**), `resolve_references`, `withFileOfType[]`, `particularInterest` | « nécessairement paginé (avec un maximum de 50 résultats par page, pour un maximum de 10 000 résultats au total) » ; contrôle `page * page_size + page_size > 10000` (`[JDL]/src/api/search.js:271`) ; requête vide → résultat vide ; pas de texte intégral dans les résultats |
| `GET /decision` | `id`, `resolve_references`, `query`, `operator` | Texte intégral + **zones** + titrages + sommaire + documents associés + « textes appliqués » (`visa`) + « rapprochements de jurisprudence » |
| `GET /taxonomy` | `id`, `key`, `value`, `context_value` | Référentiels (type, theme, chamber, formation, jurisdiction, location, publication, solution, field, filetype…) → **completions MCP** |
| `GET /stats` | `jurisdiction`, `location`, `date_start`, `date_end`, `particularInterest`, `keys` | Statistiques |
| `GET /export` | filtres + `date_type` (creation/update), `order`, `batch_size` (**max 1000**, défaut 10), `batch`, `abridged` | Lots ; « limité par défaut (pour une connexion non authentifiée) à 10 résultats par lot, pour un maximum de 1000 résultats au total » |
| `GET /scan` | idem + `source[]`, `search_after` (curseur via `next_batch`) | Export par curseur (pas de limite de fenêtre) |
| `GET /transactionalhistory` | `date`, `page_size`, `from_id` | Journal create/update/delete pour synchroniser une copie locale ; querystring de suite valable 1 min |
| `GET /healthcheck` | — | |

Taxonomies (`[JDL]/src/taxons/*.js`) : `jurisdiction` ∈ {cc: Cour de cassation, ca: Cour d'appel, tj: Tribunal judiciaire, tcom: Tribunal de commerce, cph: Conseil de prud'hommes} (`jurisdiction.js:1-11`) ; `field` ∈ {themes, text, introduction, expose, moyens, motivations, dispositif, annexes, visa, summary} (`field.js`) ; `sort` ∈ {score, scorepub (défaut), date} (`sort.js`) ; `publication` ∈ {b: Bulletin, r: Rapport, l: Lettres de chambre, c: Communiqué, n: Non publié} (`publication.js`).

### 5.3 Apport par rapport au fonds JURI de Légifrance

- **Zonage** : `zones {introduction, expose, moyens, motivations, dispositif, annexes}` = listes de fragments `{start, end}` (offsets dans `text`, fin exclue), non nécessairement séquentiels ; linéariser en triant par `start` (`[JDL]/README.md:72-160`). → support natif du **retour progressif / `focus`** (ne renvoyer que « motivations » + « dispositif »).
- **Rapprochements structurés** : `rapprochements[{id: "JURITEXT000036780011", date, title, url (legifrance), number}]` et `visa[{title}]` (textes appliqués), `contested {date, title, jurisdiction, location, chamber, solution, …}`, `files[{type: « Arrêt traduit », « Communiqué »…, url}]`, `titlesAndSummaries[{titles[], summary}]`, `themes[]` (fixture `[JDL]/src/data/sample_detail_resolved.json`). → **liens entre décisions avec identifiants JURITEXT résolvables via Légifrance**, ce que `/consult/juri` ne fournit pas (§1.7).
- Couverture : Cour de cassation + juridictions judiciaires (décisions civiles, sociales et commerciales des cours d'appel à partir de mars 2022 selon le calendrier de l'arrêté du 28 avril 2021, `[JDL]/README.md:5-7`) ; la taxonomie `jurisdiction` ne contient ni le Conseil d'État ni les juridictions administratives (`[JDL]/src/taxons/jurisdiction.js:1-11`) — `[JRX]/src-tauri/src/juridique/judilibre.rs:4-5` affirme le contraire : NON VÉRIFIÉ.
- Synchronisation : `/transactionalhistory` + `/scan` pour un futur adapter « bulk ».
- `[PY]/SOURCES.md:12` : intégration rejetée pour `fr-eli-mcp` faute de souscription ; JURI couvre déjà la Cour de cassation. Pour le Rust : **adapter optionnel** (`adapter-judilibre`) derrière le même port de jurisprudence, activé seulement si la souscription existe (healthcheck 403 → désactivation propre).

---

## 6. Dumps open data DILA

### 6.1 Bases, URL, licence

- Serveur : `https://echanges.dila.gouv.fr/OPENDATA/<FOND>/` (HTTPS) et `ftps://echanges.dila.gouv.fr` ; DTD dans `/OPENDATA/DTD_LEGIFRANCE/` ; contact `donnees-dila@dila.gouv.fr` (résumés WebSearch des pages data.gouv.fr, ex. https://www.data.gouv.fr/datasets/legi-codes-lois-et-reglements-consolides, https://www.data.gouv.fr/datasets/jade, https://www.data.gouv.fr/datasets/inca). URL citée par la DILA elle-même : `https://echanges.dila.gouv.fr/OPENDATA/` (`[RAW]/exemples…md:1331-1333`). **VÉRIFIÉ.** (Hôte injoignable depuis notre environnement.)
- Liste officielle des fonds (doc DILA V3, 17/09/2025) : « ACCO / BOCC / CAPP / CASS / CIRCULAIRES / CNIL / CONSTIT / DOLE / Debats / INCA / JADE / KALI / LEGI / Questions-Reponses » ; chaque fonds a un document « présentation » ; la « DTD_LEGIFRANCE » décrit la structure (`[RAW]/exemples…md:1335-1339`). **VÉRIFIÉ.** JORF n'est pas dans cette ligne mais existe : `/OPENDATA/JORF/` est utilisé par `[JL]` (`scripts/dila_reingest_all.sh:57-58` avec `f=jorf`) et l'arrêté de 2014 le cite. 
- Contenu (tableau DILA, résumé WebSearch https://www.dila.gouv.fr/services/repertoire-des-informations-publiques/les-donnees-juridiques) : LEGI = codes, lois et règlements consolidés ; JORF = textes publiés au JO ; KALI = conventions collectives nationales ; CASS = décisions publiées de la Cour de cassation ; INCA = décisions inédites de la Cour de cassation ; CAPP = cours d'appel et premières instances judiciaires ; CONSTIT = Conseil constitutionnel ; JADE = juridictions administratives ; tous en XML, DTD génériques partagées entre bases (un changement de DTD peut affecter plusieurs bases). DOLE = dossiers législatifs (https://www.data.gouv.fr/datasets/dole-les-dossiers-legislatifs/). **VÉRIFIÉ (via résumé).**
- Licence : **Licence Ouverte (Etalab) v2.0** pour LEGI (`[LCX]/README.md:85-88` ; data.gouv.fr via résumé) ; producteur « Premier ministre » pour JADE/INCA/KALI (résumé). Base juridique de la gratuité : arrêté du 24 juin 2014 (réutilisation gratuite des bases JORF, LEGI, KALI, JADE, CONSTIT, CASS-INCA, CAPP, CNIL, CIRCULAIRES) (résumé WebSearch, https://www.les-infostrateges.com/actu/open-data-les-licences-libres-et-gratuites-de-legifrance). Avertissement DILA : la mise à disposition ne dispense pas le réutilisateur de respecter la loi Informatique et Libertés (données personnelles possibles). **VÉRIFIÉ (via résumé)** ; version exacte de la licence par fonds : **NON VÉRIFIÉ** pour chaque fonds.

### 6.2 Format, nommage, cadence

- **Archives `.tar.gz`** : un dump complet « Freemium » puis des **incréments**. Nommage LEGI : `Freemium_legi_global_AAAAMMJJ-HHMMSS.tar.gz` + `LEGI_AAAAMMJJ-HHMMSS.tar.gz` (`[LCX]/scripts/fetch.py` regex `(?:Freemium_legi_global|LEGI)_\d{8}-\d{6}\.tar\.gz` ; `[LEGIPY]/legi/tar2sqlite.py:596` regex `(.+_)?legi(_global)?_(\d{8}-\d{6})\..+`). Nommage générique pour les autres fonds : `Freemium_<fond>_global_AAAAMMJJ-HHMMSS.tar.gz` et `<FOND>_AAAAMMJJ-HHMMSS.tar.gz` (`[JL]/scripts/dila_reingest_all.sh:58,71` ; `[JL]/update_dila.sh:15,30,44` pour CASS/CAPP/INCA). **VÉRIFIÉ (tiers concordants).**
- **Dernier global** : 13/07/2025 (`Freemium_legi_global_20250713-140000.tar.gz`, résumé WebSearch ; `[LCX]` README via WebFetch : « dump de base du 2025-07-13, complété par les incréments quotidiens jusqu'au 2026-10-10 (`LEGI_20261010-211641`) » ; `[JL]/scripts/dila_reingest_all.sh:7` « le GLOBAL du 13/07/2025, PUIS tous les deltas quotidiens »). Le rythme de publication des globaux (« environ deux fois par an », résumé WebSearch) : **NON VÉRIFIÉ** — de fait aucun nouveau global depuis 15 mois.
- **Incréments** : « En général la DILA publie une nouvelle archive à la fin de chaque jour ouvré » (`[LEGIPY]/README.md:61-65`) ; heures de publication **variables** (`[JL]/scripts/dila_update_daily.sh:7-8`) ; archives vides (< 200 octets) possibles (`[JL]/dila_catchup.sh:45-49`). **VÉRIFIÉ (tiers).**
- **Ordre d'application obligatoire** : global puis incréments en ordre chronologique strict (sinon retour à l'état du global) (`[JL]/scripts/dila_reingest_all.sh:5-10` ; `[LCX]/scripts/ingest.py` manifest « global first, then increments chronologically ») ; legi.py **recrée la base** quand un global plus récent apparaît (`[LEGIPY]/legi/tar2sqlite.py:604-616`). Exception tierce : LEGI rendu insensible à l'ordre par la clé `(legiarti, date_debut)` (`[JL]/scripts/dila_reingest_all.sh:9-10`).
- **Suppressions** : fichiers `liste_suppression_legi.dat` (chemins à supprimer, ASCII, séparés par espaces) et `liste_suppression_legi_dossier.dat` (séparateur `" D\n"`) dans les archives (`[LEGIPY]/legi/tar2sqlite.py:243-252,79-182`) ; `[LCX]/scripts/ingest.py` (fonction `suppress`) traite aussi la suppression d'un répertoire JORFTEXT entier. **VÉRIFIÉ (tiers).**
- Téléchargement : listing HTML de l'index, reprise par `Range` (statuts 206/200/416 gérés), retries (`[LEGIPY]/legi/download.py:46-100` ; `[LCX]/scripts/fetch.py` découpe le global en morceaux de 64 Mio téléchargés en parallèle ≤ 4).

### 6.3 Structure des fichiers LEGI (VÉRIFIÉ via code tiers concordant)

**Arborescence dans l'archive** (`[LEGIPY]/legi/tar2sqlite.py:62-71,253-262`) :

```
[<prefixe>/]legi/global/code_et_TNC_(en|non)_vigueur/(code|TNC)_(en|non)_vigueur/
    AAAA/BB/CC/DD/EE/FF/GG/<CID 20 car.>/                      (parts[11] = cid)
        (article | section_ta | texte/struct | texte/version)/   (parts[13] = struct|version)
            AAAA/BB/CC/DD/EE/FF/GG/<ID 20 car.>.xml
```

- `TNC` = textes non codifiés ; `en_vigueur` / `non_vigueur` séparent les textes. Les mêmes IDs peuvent apparaître dans deux dossiers (doublons gérés par `mtime`, table `duplicate_files`, `tar2sqlite.py:265-322`). Chemins tar parfois en octets → décodage latin-1 (`tar2sqlite.py:237-239`).
- `[LCX]` ne garde que `code_et_TNC_en_vigueur` et supprime les textes passés en `non_vigueur` (`scripts/ingest.py`, fonction `handle`).

**Types de documents XML** (racines) et champs (`[LEGIPY]/legi/tar2sqlite.py:28-39,329-442` ; `[LCX]/scripts/ingest.py` ; `[JL]/parse_dila_bulk.py:349-426`) :

| Racine | Contenu | Champs clés |
|---|---|---|
| `ARTICLE` (LEGIARTI) | une **version** d'article | `META/META_COMMUN/{ID, NATURE (= « Article »), ANCIEN_ID}`, `META/META_SPEC/META_ARTICLE/{NUM, ETAT, DATE_DEBUT, DATE_FIN, TYPE}`, `CONTEXTE/TEXTE@cid @nature` (+ `TITRE_TXT`, `TM/TITRE_TM@id` chaîne des sections), `BLOC_TEXTUEL/CONTENU` (HTML), `NOTA/CONTENU`, `LIENS/LIEN` |
| `SECTION_TA` (LEGISCTA) | section (pas de `META_COMMUN`) | `ID`, `TITRE_TA`, `COMMENTAIRE`, `CONTEXTE/TEXTE@cid`, `STRUCTURE_TA/(LIEN_SECTION_TA | LIEN_ART)@id @debut @fin @etat @num` |
| `TEXTELR` (texte/struct) | structure d'un texte | `META/META_COMMUN/ID`, `VERSIONS`, `STRUCT/(LIEN_SECTION_TA | LIEN_ART)@id @debut @fin @etat` (ne porte pas le titre, `[JL]/parse_dila_bulk.py:307-310`) |
| `TEXTE_VERSION` (texte/version) | version d'un texte | `META/META_SPEC/META_TEXTE_CHRONICLE/{CID, NUM, NUM_SEQUENCE, NOR, DATE_PUBLI, DATE_TEXTE, DERNIERE_MODIFICATION, ORIGINE_PUBLI, PAGE_DEB_PUBLI, PAGE_FIN_PUBLI, NUM_PARUTION}`, `META/META_SPEC/META_TEXTE_VERSION/{TITRE, TITREFULL, ETAT, DATE_DEBUT, DATE_FIN, AUTORITE, MINISTERE, LIENS}`, `VISAS`, `SIGNATAIRES`, `TP` (travaux préparatoires), `NOTA`, `ABRO`, `RECT` (chacun avec `CONTENU`) |

**Liens** (`LIENS/LIEN`, `[LEGIPY]/legi/tar2sqlite.py:417-442,47-60`) : attributs `typelien`, `sens` (`source` | `cible`), `id`, `cidtexte`, texte = titre de la cible ; quand `sens="cible"`, legi.py inverse source/destination et le type (`MODIFICATION`↔`MODIFIE`, `ABROGATION`↔`ABROGE`, `CODIFICATION`↔`CODIFIE`, `CONCORDANCE`↔`CONCORDE`, `CREATION`↔`CREE`, `TRANSFERE`↔`TRANSFERT`, `PEREMPTION`↔`PERIME`, `RATIFICATION`↔`RATIFIE`, `ANNULATION`↔`ANNULE`, `DISJOINT`↔`DISJONCTION`, `DEPLACE`↔`DEPLACEMENT`). Table SQLite `liens (src_id, dst_cid, dst_id, dst_titre, typelien, _reversed)` (`legi/sql/schema.sql:100-111`). `[JL]` stocke tous les attributs (`cidtexte`, `nortexte`, …) en JSON (`parse_dila_bulk.py:99-113`). → équivalent offline de `/consult/relatedLinksArticle` et `lienModifications`.

**Sommaires / versions** : `sommaires (cid, parent, element, debut, fin, etat, num, position, _source)` (`schema.sql:86-98`). Sélection de la version en vigueur à la date D : `debut ≤ D < fin` sur chaque lien de structure (`[LCX]/scripts/build.py`, `Builder.in_window`, `fin` par défaut `2999-01-01`). Archéo Lex calcule les dates de versions d'un texte comme l'union des `debut`/`fin` de ses éléments (`[ALEX]/marcheolex/exporterlegi.py:266-374`).

**Dates de vigueur et anomalies** :

- `2999-01-01` = pas de date de fin (`[ALEX]/marcheolex/exporterlegi.py:172-182` ; `[LEGIPY]/legi/anomalies.py:23`) ; **`2222-02-22`** = « entrée en vigueur différée à une date non-déterminée » (`[ALEX]/doc/limitations.md`, section « Dates bizarres et spéciales »).
- Articles **mort-nés** (`MODIFIE_MORT_NE`) : date de fin antérieure à la date de début ; jamais en vigueur (`[ALEX]/doc/limitations.md` ; `[RAW]/lexique-api-lgf.md:125`).
- Contenus d'article manquants (métadonnées seules), articles rattachés à une mauvaise section, et différences entre la base LEGI diffusée et celle de Légifrance (`[ALEX]/doc/limitations.md`) ; incohérences dans le dump du 13/07/2025 (décret à la fois en vigueur et abrogé, articles référencés absents, résumé WebSearch d'une discussion data.gouv.fr) ; `[LCX]` : 236 liens de structure sur ~377 000 pointent vers des versions d'articles absentes (`README.md` section « What is NOT included ») ; contrôles d'anomalies de legi.py : état incohérent avec `date_fin`, éléments de sommaire introuvables, articles/sections orphelins, doublons de numéros (`[LEGIPY]/legi/anomalies.py:10-119`), publiés sur anomalies.legilibre.fr.
- `CID` : identifiant commun à toutes les versions (`[RAW]/lexique-api-lgf.md:37-59`) ; LEGI n'a pas d'identifiant reliant réellement les versions d'un même texte → « factorisation » par (nature, num), NOR, titre normalisé (`[LEGIPY]/README.md:78-81`, `legi/factorize.py:9-57`).
- HTML : nettoyage (espaces, attributs `id`/`dir`, `span` vides) et **césures molles `­`** à retirer ; 93 M de caractères inutiles détectés en 2020 (`[LEGIPY]/README.md:83-95`, `tar2sqlite.py:223-224`).
- Statut d'article (`TYPE`) : AUTONOME / entièrement modificateur (sans état ni dates) / partiellement modificateur (`[RAW]/lexique-api-lgf.md:275-301`).

### 6.4 Structure des autres fonds (code tiers récent, `[JL]/parse_dila_bulk.py`)

- **Jurisprudence (CASS, CAPP, INCA, JADE, CONSTIT)** : métadonnées réparties sur **deux conteneurs** : `META_JURI` (`TITRE`, `DATE_DEC`, `JURIDICTION`, `SOLUTION`, `NUMERO` interne) et `META_JURI_JUDI` (judiciaire : `ECLI`, `FORMATION`, `PRESIDENT`, `RAPPORTEUR`, `AVOCAT_GL` — et non `AVOCAT_GENERAL` —, `AVOCATS`/`AVOCAT`, `NUMEROS_AFFAIRES/NUMERO_AFFAIRE` = vrai n° de pourvoi, `PUBLI_BULL@publie`) ou `META_JURI_ADMIN` (`COMMISSAIRE_GVT`, `TYPE_REC`, `PUBLI_RECUEIL` A/B/C) ou `META_JURI_CONSTIT` (`NATURE_QUALIFIEE`, `LOI_DEF@num @date`, `ECLI`, `NOR`, `TITRE_JO`, `URL_CC`) ; `SOMMAIRE/SCT@TYPE` (PRINCIPAL/REFERENCE) et `SOMMAIRE/ANA` ; renvois `CITATION_JP` et `RAPPROCHEMENTS` ; `LIENS/LIEN` ; `SAISINES` (`parse_dila_bulk.py:715-800,904-982`, vérifié par l'auteur sur `CASS_20260824`). **VÉRIFIÉ (tiers).**
- **KALI** : `KALICONT` (racine `IDCC`) → `META_SPEC/META_CONTENEUR/{TITRE, ETAT, NUM (= IDCC), DATE_PUBLI}` + `STRUCTURE_TXT/TM/LIEN_TXT` ; `KALITEXT` (racine `TEXTEKALI` ou `TEXTE_VERSION`) → `META_TEXTE_CHRONICLE` / `META_TEXTE_VERSION` ; `KALIARTI` (racine `ARTICLE`) → `META_ARTICLE/{NUM, TITRE, ETAT, DATE_DEBUT, DATE_FIN, DATE_DEB_EXT, DATE_FIN_EXT}` + `CONTEXTE/CONTENEUR@num (IDCC) @titre @etat @date_publi` ; `KALISCTA` (`SECTION_TA`) sans `META_COMMUN`. Les balises `META_TEXTE_KALI` / `META_CONVENTION_COLLECTIVE` n'existent pas (vérifié sur `KALI_20260907-214720`) (`parse_dila_bulk.py:1043-1066`). **VÉRIFIÉ (tiers).**
- **JADE** du bulk ne contient que CE + 9 CAA ; les 40 TA ne sont diffusés que par opendata.justice-administrative.fr (`[JL]/download_opendata.py:4-8`). Le fonds CETAT de l'API Légifrance contient des décisions de TA (facette `JURIDICTION_NATURE` → `TRIBUNAL_ADMINISTATIF` 772 résultats dans `search_cetat.json`). **VÉRIFIÉ (tiers + fixture).**
- Encodage des fichiers XML (déclaration `encoding`) : **NON VÉRIFIÉ** (aucun fichier lu) ; utiliser un parseur respectant la déclaration XML.

### 6.5 Volumétrie

| Mesure | Valeur | Source | Statut |
|---|---|---|---|
| Base SQLite legi.py (LEGI complet, toutes versions) | ≈ 5 Go (2026) | `[LEGIPY]/README.md:56` | VÉRIFIÉ (tiers) |
| LEGI en vigueur au 2026-10-11 | 76 codes ; 165 279 articles de codes en vigueur ; 148 articles de Constitution ; textes en vigueur par nature : ARRETE 69 708, DECRET 44 813, LOI 2 978, ORDONNANCE 1 320, LOI_ORGANIQUE 106, LOI_CONSTIT 19, CODE 76… ; 176 093 articles de décrets | `[LCX]/scripts/stats-codes-constitution.json`, `stats-decrets.json` | VÉRIFIÉ (tiers) |
| Entrepôt justicelibre (2026) | `legi.db` 3,8 Go / 1 780 693 versions d'articles ; `jorf.db` 2,3 Go / 1 257 034 textes (post-1990) ; `jade.db` 14 Go / 570 063 décisions ; `kali.db` 1,3 Go / 305 839 textes ; `cnil.db` 147 Mo / 26 663 délibérations ; `judiciaire.db` ≈ 21 Go (1 213 903 décisions CASS+CAPP+INCA+CONSTIT) ; intermédiaires inca/cass/capp ≈ 11 Go | `[JL]/ARCHITECTURE.md:29-56` | VÉRIFIÉ (tiers) |
| Snapshot `mcp-fr-legal` (codes + lois, en vigueur) | 3 953 documents / 193 681 dispositions, `database.db` ≈ 303 Mo | `[MFL]/README.md:10-22` | VÉRIFIÉ (tiers) |
| Archive LEGI compressée / extraite | ≈ 745–796 Mo compressé (anciens snapshots), 9–15 Go extrait (2017) ; Archéo Lex : « environ 5 Gio » à télécharger | résumé WebSearch (sources non identifiées) ; `[ALEX]/README.md` (section Lancement) | **NON VÉRIFIÉ** |

### 6.6 Projets open source exploitant les dumps et leçons

| Projet | Approche | Leçons |
|---|---|---|
| legi.py (Legilibre, CC0) | libarchive en streaming → SQLite ; incrémental ; normalisation des titres/numéros ; factorisation ; nettoyage HTML ; anomalies | Streaming sans extraction ; `mtime` pour sauter les fichiers inchangés ; doublons inter-dossiers ; listes de suppression ; recréation sur nouveau global ; PRAGMA `journal_mode=off`/`synchronous=off` pendant l'import initial puis WAL (`tar2sqlite.py:557-560,631-660`) |
| Archéo Lex (2019, inactif) | une version Git par date de changement | Dates sentinelles ; mort-nés ; articles manquants ; bugs de rattachement de section |
| LawCodex/fr (2026) | `tarfile r|gz` en flux → SQLite → Markdown à date | Sélection `debut ≤ D < fin` ; ne garder que `en_vigueur` ; exclure les arrêtés pour la taille ; mesurer les liens cassés |
| justicelibre (2026, MIT) | 9 fonds DILA + Judilibre → SQLite FTS5 (external content) | Registre des archives appliquées, fenêtre de rattrapage de 10 jours, verrou d'ingestion, garde-fou disque, « UPSERT ne remplaçant que par une valeur non vide », bugs de balises réels (KALI, AVOCAT_GL, META_JURI_JUDI) |
| mcp-fr-legal (2026, MIT) | SQLite FTS5 en lecture seule, citations verbatim, `fr_validate_citation` fail-closed, `staleness_advisory` si snapshot > 365 j | Provenance (`snapshot`, `age_days`, licence) dans chaque réponse ; ancrage anti-hallucination par mécanisme |

### 6.7 Recommandations pour le futur adapter Rust « dumps DILA » (même port que Légifrance)

Versions crates relevées sur l'index sparse crates.io le 2026-10-15 (dernière version non-yanked) : `quick-xml` 0.42.0 (MSRV 1.86, features `encoding`, `serialize`, `async-tokio`) ; `tantivy` 0.26.2 (MSRV 1.86, features `stemmer`, `stopwords`, `mmap`, `zstd-compression`) ; `tar` 0.4.46 ; `flate2` 1.1.10 ; `rusqlite` 0.40.2 ; `similar` 3.2.0 (MSRV 1.85) ; `governor` 0.10.4 ; `encoding_rs` 0.8.42 (MSRV 1.88) ; `reqwest` 0.13.5 (MSRV 1.85) ; `secrecy` 0.10.3 ; `tokio-util` 0.7.20 ; `async-compression` 0.4.50. tantivy dispose d'un stemmer `Language::French` et d'une liste de mots vides française (`[TANT]` `src/tokenizer/stemmer.rs:12-42`, `src/tokenizer/stop_word_filter/mod.rs:37-51`) et d'un `ascii_folding_filter`. **VÉRIFIÉ.**

1. **Ingestion en flux** : `flate2::read::GzDecoder` → `tar::Archive::entries()` → `quick_xml::Reader` sur le contenu de chaque entrée ; ne jamais extraire sur disque (millions de petits fichiers).
2. **Registre d'archives appliquées** (table `applied_archives(name, kind, date, applied_at, status)`) ; ordre chronologique strict ; reconstruction complète quand un nouveau `Freemium_*_global_*` apparaît ; tolérer les archives vides ; rejouer automatiquement les échecs (fenêtre glissante).
3. **Suppressions** : appliquer `liste_suppression_*.dat` après le contenu de l'archive (comme legi.py, `tar2sqlite.py:541-544`).
4. **Modèle de stockage** : clé de version `(id, date_debut)` ; tables textes/versions, sections, articles, sommaires (liens de structure avec `debut/fin/etat/num/position`), liens (typés, orientés) ; SQLite (rusqlite) comme stockage de vérité + **tantivy** comme index plein texte (stemmer FR + stopwords + ascii folding) — ou FTS5 si l'on veut un seul fichier (précédents `[JL]`, `[MFL]`).
5. **Dates** : `Option<NaiveDate>` avec `2999-01-01 → None` (fin ouverte) et `2222-02-22 → DateDifferee` ; même type `LegalDate` que pour l'API (§1.14).
6. **Parité de port** : l'adapter doit implémenter le même port que `adapter-legifrance` (recherche, consultation d'article à date, versions, TOC, liens) et signaler sa **fraîcheur** (date du dernier incrément appliqué, `age_days`) dans chaque réponse, avec l'attribution Licence Ouverte (source + date de dernière mise à jour).
7. **Robustesse des balises** : tests de non-régression sur des extraits XML réels (un par racine et par fonds) ; ne pas supposer l'existence d'un conteneur unique (`META_JURI` vs `META_JURI_*`) ; l'UPSERT ne doit pas écraser une valeur par une valeur vide.
8. **Différences avec l'API** : la base LEGI diffusée peut différer de Légifrance (§6.3) → exposer la source (`dila-dump` vs `legifrance-api`) dans la réponse ; fallback API pour vérifier une citation douteuse.
9. **Politesse réseau** : ≤ 4 connexions, reprise par `Range`, User-Agent identifiant.

---

## 7. Synthèse des recommandations pour le plan d'implémentation

1. **Générer/écrire les DTO depuis le Swagger 2.4.2** (`[SPEC]`, 179 définitions) mais avec tolérance (`#[serde(default)]`, champs inconnus ignorés, alias pour écarts observés comme `isMultipleVersions`) ; versionner une copie du Swagger dans le repo pour un test de dérive (comme `tests/tools.lock.json` côté Python).
2. **Types de domaine** : enums `Fond`, `TypeChamp` (par fonds), `TypeRecherche`, `Operateur`, `Sort` (par fonds, cf. §2.2), `Facette` (par fonds), `EtatJuridique` (17 valeurs), `TypePagination` ; newtypes d'ID avec préfixes (`LEGITEXT`, `LEGIARTI`, `LEGISCTA`, `JORFTEXT`, `JORFCONT`, `JORFARTI`, `JORFDOLE`, `KALITEXT`, `KALICONT`, `KALIARTI`, `KALISCTA`, `JURITEXT`, `CONSTEXT`, `CETATEXT`, `CNILTEXT`, `ACCOTEXT`, `JURINOME`, `CETANOME`) et distinction id/cid + suffixe de version.
3. **Port `LegalSource`** couvrant : search, consult (article, texte à date, section, TOC), versions (liste, contenu, motif chrono), liens (article, décision), résolution (num/ELI/NOR/ancienId), suggest, listes (codes, conventions, JO) — implémenté par `adapter-legifrance` puis `adapter-dila-dump` (et optionnellement `adapter-judilibre` pour la jurisprudence judiciaire enrichie).
4. **Client HTTP** : TokenManager interne (marge 60 s, verrou, 1 retry sur 401), rate limiter global + sémaphore, retry backoff sur 429/502/503/504 (et au plus 1 sur 500), décodage UTF-8 strict + réparation mojibake optionnelle, `Accept`/`Content-Type` JSON, User-Agent explicite.
5. **Retour progressif** : TOC résumée, sections à la demande (`/consult/code` avec `sctCid`), versions listées via `canonicalArticleVersion` puis contenu via `getArticleByCid` ; Judilibre `zones` pour le `focus`.
6. **Conformité** : attribution Licence Ouverte (source + date de mise à jour) dans chaque réponse, avertissement « seul le JO fait foi », respect `dereferenced`/`inap`, aucune analyse par magistrat, secrets jamais exposés en mode HTTP partagé.

---

## 8. Questions ouvertes / points NON VÉRIFIÉS (à tester en sandbox)

1. Valeurs numériques des quotas PISTE (sandbox et prod), granularité (par méthode ?), présence de `Retry-After` sur 429.
2. Limite de pagination profonde de `/search` Légifrance (fenêtre Elasticsearch 10 000 ?) et comportement au-delà.
3. `NOM_CODE` vs `TEXT_NOM_CODE` sur `CODE_DATE` (doc officielle vs `[JRX]`/`[KMCP]`), et tris autorisés sur CODE_* (aucun dans la doc ; `PUBLICATION_DATE_*` selon `[JRX]`).
4. Une facette inconnue / un champ JSON inconnu / `fond=ALL` avec filtres provoquent-ils vraiment un HTTP 500 (et non 400) ?
5. Formats de date acceptés en entrée (`singleDate` chaîne vs epoch ms ; `legiPart.date` epoch vs chaîne ; fuseau horaire attendu pour epoch ms).
6. Filtre `valeur` (singulier) des exemples officiels vs `valeurs` ; casse des valeurs (`ARRET` vs `arret`).
7. Cause du mojibake dans les fixtures LEGI/JURI (sandbox seulement ? en-tête `Content-Type` sans charset ? données amont ?) et présence en production.
8. Contenu réel de `Article.articleVersions`, `lienCitations`, `lienModifications` dans `/consult/getArticle` (absents de la fixture, peut-être élaguée), taille des listes de `relatedLinksArticle`.
9. Contenu de `Accord.attachment.content` (texte extrait ou base64 du .docx) pour ACCO.
10. Différences de données sandbox/prod (le Swagger dit « non identiques » ; ampleur inconnue) ; dates « stable 04/04/2023, bêta fermée 06/06/2023 ».
11. CGU PISTE générales (portail AIFE) : clauses sur le partage de compte, l'usage par des tiers via une application mutualisée, la mise en cache — non lues ; l'analyse du §4.3 est une interprétation des CGU Légifrance seules.
12. CGU Judilibre (Cour de cassation, V8) non lues ; obligations précises (mention, réidentification, quotas) ; article de loi exact de l'interdiction de profilage des magistrats.
13. Licence exacte et document « présentation » de chaque fonds DILA (JORF, KALI, CASS, CAPP, INCA, JADE, CONSTIT, CNIL, ACCO, DOLE, CIRCULAIRES…), DTD actuelles, encodage XML déclaré.
14. Volumétrie compressée actuelle des dumps (global LEGI, incréments quotidiens) et rythme de publication des globaux (dernier : 13/07/2025).
15. Valeurs acceptées par `/list/code.sort` (`TITLE_ASC` selon la spec ; `CODE_DATE_MAJ` utilisé par `[JL]`).
16. Judilibre : un jeton Bearer PISTE suffit-il partout ou `KeyId` est-il requis pour certains endpoints (`/export` au-delà de 10×1000) ? Différence « connexion non authentifiée » vs authentifiée pour `/export`.
