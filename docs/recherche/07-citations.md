# 07 — Grammaire des citations juridiques françaises (parseur du crate `domain`)

Rapport de recherche (axe 07) pour la réécriture Rust de `fr-eli-mcp`, à destination des rédacteurs du plan d'implémentation (features `fr_verify_citations` et `fr_resolve`, extraction de « liens entre décisions »). Date de rédaction : 2026-10-15.

## 0. Méthode, légende, sources

### 0.1 Légende des statuts

- **VÉRIFIÉ** : lu dans un fichier (chemin:ligne) — fixtures réelles de l'API, documents officiels DILA archivés, code de la référence Python, ou code/données tiers non ambigus (alors marqués « TIERS »).
- **WEB** : fait issu d'un **résumé WebSearch** (URL citée). Moins fiable : les pages elles-mêmes n'ont pas pu être ouvertes (voir 0.3).
- **NON VÉRIFIÉ** : déduction, souvenir, contradiction entre sources, ou fait non recoupé. À tester (sandbox PISTE) avant d'en dépendre.
- Dans le corpus de tests (§8), chaque cas porte une provenance : `FX` (fixture réelle), `OFF` (document officiel DILA), `TP` (donnée tierce), `WEB` (résumé WebSearch), `SYN` (cas synthétique construit pour la grammaire ; son **contenu juridique n'est pas garanti**, seule sa forme compte).

### 0.2 Abréviations de chemins

| Abrév. | Chemin / URL | Version, licence |
|---|---|---|
| `[PY]` | `/home/user/matematicsolutions/fr-eli-mcp` | d3b5b13 (2026-09-24), Apache-2.0 |
| `[FX]` | `[PY]/tests/fixtures/*.json` (vraies réponses API enregistrées) | idem |
| `[CL]` | `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/clones` | — |
| `[DILA]` | `[CL]/pylegifrance/docs/raw/legifrance/` — copies archivées des documents officiels DILA (`exemples-d-utilisation-de-l-api.md` V3 du 17/09/2025, `lexique-api-lgf.md`, `description-des-tris-et-filtres-de-l-api.md`) | pylegifrance d8e0e59, MIT |
| `[R05]` | `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/research/05-piste-dila.md` (rapport axe 05 : Swagger Légifrance 2.4.2, endpoints, champs, facettes) | — |
| `[JDL]` | `[CL]/p05-Cour-de-cassation__judilibre-search` (dépôt officiel de l'API Judilibre) | ddfc166 (2026-10-13), licence non déclarée |
| `[JL]` | `[CL]/p05-Dahliyaal__justicelibre` (pipeline DILA + MCP) | c2440ab (2026-10-15), MIT |
| `[KM]` | `[CL]/p05-Ktulu-Analog__mcp-legifrance` | 3c916e3 (2026-06-26), **AGPL-3.0 → données/idées seulement, aucune copie de code** |
| `[JRX]` | `[CL]/p05-Ktulu-Analog__juridix` (vérificateur de citations Rust/Tauri) | a177523 (2026-10-13), **AGPL-3.0 → idées seulement** |
| `[LEGIPY]` | `[CL]/p05-Legilibre__legi.py` (import LEGI, normalisation des numéros d'articles et titres) | bc68599 (2026-09-15), **CC0** (réutilisable) |
| `[PLF]` | `[CL]/pylegifrance` | d8e0e59 (2026-08-19), MIT |
| `[LRR]` | `[CL]/p07-jurism-lrr` (Juris-M legal-resource-registry, cloné pour cet axe) | 0438a01 (2026-09-30), MIT |
| `[WIN]` | `[CL]/p07-winnow` (cloné pour cet axe) | 88251a8, winnow 1.0.4, MIT |
| `[RGX]` | `[CL]/p07-regex` (cloné pour cet axe) | regex 1.13.1, MIT OR Apache-2.0 |
| `[UN]` | `[CL]/p07-unicode-normalization` (cloné pour cet axe) | 0.1.25, (MIT OR Apache-2.0) AND Unicode-3.0 |
| `[CORPUS]` | `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/work07/corpus_pos.jsonl` et `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/work07/corpus_neg.jsonl` (copie intégrale au §8) ; validateur `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/scripts07/validate.py` | — |

Aucun code tiers n'a été exécuté. Les scripts d'analyse écrits pour cet axe sont dans `/tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/scripts07/` et lisent les clones comme données (`python3 -I`).

### 0.3 Contraintes réseau rencontrées

- `WebFetch` ne résout que `github.com` ; `e-justice.europa.eu`, `reflex.sne.fr`, `libguides.bodleian.ox.ac.uk`, `fr.wikipedia.org`, `ltd.univ-amu.fr`, `bu.univ-cotedazur.fr`, `www.clementfrancois.fr` → `getaddrinfo ENOTFOUND`. `curl` vers `e-justice.europa.eu`, `courdecassation.fr`, `conseil-constitutionnel.fr`, `reflex.sne.fr`, `raw.githubusercontent.com` → `CONNECT tunnel failed, response 403`. Les faits « WEB » proviennent donc uniquement des **résumés WebSearch**.
- Index sparse crates.io joignable (versions de crates du §9 vérifiées ainsi) ; docs.rs bloqué → API des crates vérifiée en lisant les sources clonées.

---

## 1. Synthèse (à lire en premier)

1. **La référence Python n'a aucun parseur de citations** : `citations.py` ne contient que des normaliseurs de réponses et des *formateurs* (`[PY]/src/fr_eli_mcp/citations.py:1-486`). Tout le parseur est à créer. Ses formateurs produisent des chaînes défectueuses (`decision`/`deliberation` sans accent, mois `fevrier/aout/decembre`, jour `1` au lieu de `1er` : `citations.py:283-295`, `:309`, `:378`) que le parseur doit **accepter en entrée** (un LLM recopiera les sorties de l'ancien serveur).
2. **Les données réelles sont sales** et la normalisation est la moitié du travail : mojibake UTF-8→Latin-1 dans les fixtures LEGI/JURI (`nÂ°`, `PubliÃ©`, et `à` devenu `Ã`+U+00A0), U+00A0 dans les titres CONSTIT, tiret demi-cadratin dans un numéro CNIL (`SAN–2026-003`, `[FX]/search_cnil.json:53`), apostrophes typographiques, NOR en minuscules (`cscx2531180s`, `[FX]/search_constit.json:118`), `2ème - 7ème` dans les formations CE, dates `29/05/2026` et `07 novembre 2025`.
3. **Douze familles** sont couvertes par la grammaire (§4) : articles de codes, textes (lois/ordonnances/décrets/arrêtés…), articles de textes, blocs constitutionnels (Constitution, DDHC, Préambule 1946, Charte), décisions Cour de cassation, juridictions judiciaires du fond (CA, TJ…), Conseil d'État, CAA/TA, Conseil constitutionnel, ECLI, délibérations CNIL, conventions collectives (IDCC/brochure/avenant), plus JORF, NOR, ELI et identifiants Légifrance ; une famille « hors périmètre reconnu » (UE, doctrine, CEDH) sert à **éviter les faux positifs** (`D. 2016, p. 12` n'est pas un article D.).
4. **Numéros d'articles de code** : la forme interrogeable par l'API est **sans point ni espace** (`L36-11`, `[DILA]/exemples-d-utilisation-de-l-api.md:189`) ; préfixes `L`, `LO`, `R`, `D`, `A`, et `R*`/`D*` (et `R**`/`D**` dans LEGI) (`[DILA]/lexique-api-lgf.md:341-355`, `[LEGIPY]/legi/articles.py:20`) ; suffixes latins jusqu'à `nonagies` et lettres capitales (`199 undecies B`).
5. **ECLI** : structure européenne `ECLI:{pays}:{juridiction ≤7}:{année}:{ordinal ≤25, points autorisés}` (WEB) ; déclinaisons françaises **vérifiées sur fixtures** : Cour de cassation `C100399` = code de chambre `C1` + numéro d'arrêt `00399` (« Arrêt n° 399 F » dans le texte, `[FX]/consult_juri.json:43,69`) ; Conseil d'État `CECHR:2026:506507.20260529` = formation + requête + date ; Conseil constitutionnel `CC:2025:2025.1173.QPC`, et l'année ECLI est celle de la **décision** (`ECLI:FR:CC:2026:2025.1180.QPC`, TIERS `[JL]/tests/test_parse_dila_champs.py:130`). Codes CAA/TA/CA : **NON VÉRIFIÉS**.
6. **Résolution** (§7) : presque tout passe par `POST /search` avec `typeChamp` en `EXACTE` + facettes (`NUM_ARTICLE`+`NOM_CODE`, `NUM`, `NUM_AFFAIRE`, `NUM_DEC`, `NUM_DELIB`, `IDCC`, facette `ECLI`) et par quelques consult directs (`getArticleWithIdAndNum`, `getJoWithNor`, `kaliContIdcc`, ELI). Les exemples officiels DILA donnent 5 recettes complètes (`[DILA]/exemples-d-utilisation-de-l-api.md:165-695,1121-1171,1279-1327`).
7. **Non résoluble via Légifrance** : actes UE, CEDH/CJUE, doctrine, références de recueils seules (`Bull.`, `Rec. p.`), mentions sans numéro (« Crim., 28 mars 2018 » → candidats seulement), anaphores sans contexte, une grande partie des décisions de CA/TJ/TA, numéros de brochure KALI.
8. **Corpus** (§8) : 170 cas positifs (63 FX, 9 OFF, 23 TP, 10 WEB, 65 SYN) et 42 négatifs/ambigus/suspects, en JSONL directement chargeable en tests table-driven Rust ; chaque `match` est vérifié comme sous-chaîne de l'entrée (214 sous-chaînes, 0 erreur, script `validate.py`).
9. **Implémentation recommandée** (§9) : normalisation en amont avec **table de correspondance d'offsets** → scanner d'ancres (`aho-corasick` + `regex`) → parseurs `winnow` 1.0.4 sur `LocatingSlice` (spans natifs) → post-traitement (énumérations, anaphores, qualificatifs de version, drapeaux de suspicion). `regex` seul est insuffisant (pas de look-around ni de backreferences, `[RGX]/README.md:5-7`).
10. **Type central** : `enum Citation { CodeArticle, TextArticle, Text, Decision, Ecli, CnilDeliberation, Convention, Jorf, Nor, Eli, LegifranceId, Unsupported }`, chaque élément enveloppé dans `Located<T> { span, raw, confidence, flags }` avec des **spans en octets du texte d'origine**.

---

## 2. Ce que montrent les données réelles

### 2.1 Formes de citation renvoyées par l'API, par fonds (VÉRIFIÉ, fixtures)

| Fonds | Champ | Valeur réelle | Source |
|---|---|---|---|
| CODE | `article.num` / extrait `num` | `"9"` (Code civil art. 9, `LEGIARTI000006419288`) | `[FX]/consult_article.json:5,9` ; `[FX]/search_code.json` |
| CODE | `textTitles[].titre` | `"Code civil"` (`LEGITEXT000006070721`) | `[FX]/consult_article.json` |
| LODA | `title` | `"LOI nÂ° 2016-1321 du 7 octobre 2016 pour une RÃ©publique numÃ©rique (1)"` (mojibake) | `[FX]/consult_loda.json` ; `[FX]/search_loda.json:9` |
| LODA | `id` / `cid` / `nor` / `numParution` / `dateParution` | `LEGITEXT000033205014_08-09-2023` / `JORFTEXT000033202746` / `ECFI1524250L` / `"0235"` / `1475884800000` (= 2016-10-08) | `[FX]/consult_loda.json:4,6,8,17,19` |
| LODA | titre de décret citant des lois | `"DÃ©cret nÂ° 2016-1849 du 23 dÃ©cembre 2016 relatif Ã  la carte mobilitÃ© inclusion pris en application de l'article 107 de la loi nÂ° 2016-1321 du 7 octobre 2016 … et en application de l'article 2 de la loi nÂ° 93-1419 du 31 dÃ©cembre 1993 relative Ã  l'Imprimerie nationale"` | `[FX]/search_loda.json:56` |
| JURI (Cass.) | `titre` | `"Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, PubliÃ© au bulletin"` | `[FX]/consult_juri.json:45` |
| JURI | `numeroAffaire` | `["24-13.470"]` (avec le point) | `[FX]/consult_juri.json:17-19` |
| JURI | `num` / `natureNumero` | `"12500399"` / `"ARRET 12500399"` | `[FX]/consult_juri.json:11,90` |
| JURI | `ecli` | `"ECLI:FR:CCASS:2025:C100399"` | `[FX]/consult_juri.json:43` |
| JURI | `formation` / `publicationRecueil` / `typePublicationBulletin` | `"CHAMBRE_CIVILE_1"` / `"C"` / `"T"` | `[FX]/consult_juri.json:31,30,123` |
| JURI | `citationJp` | `"… à rapprocher : 1re Civ., 25 mai 2023, pourvoi nÂ° 21-23.174 (cassation partielle) et l'arrÃªt citÃ©."` | `[FX]/consult_juri.json:92` |
| JURI | en-tête du `texte` | `"CIV. 1 CC … Arrêt du 4 juin 2025 Cassation Mme CHAMPALAUNE, président Arrêt n° 399 F Pourvoi n° N 24-13.470"` | `[FX]/consult_juri.json:69` |
| JURI | `liens[].title` | `"1 du code civil."` et `"Article 10 de la directive 85/374/CEE du Conseil, du 25 juillet 1985, … ;  article 2270-"` (titres **tronqués**, `id`/`cidTexte` vides) | `[FX]/consult_juri.json:105` |
| JURI | `decisionAttaquee` | `{"formation": "Cour d'appel de Lyon", "date": 1704844800000}` | `[FX]/consult_juri.json:34` |
| CETAT (CE) | `titre` / `formation` | `"Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507"` / `"2ème - 7ème chambres réunies"` | `[FX]/consult_juri_cetat.json:40,29` |
| CETAT | `ecli` / `num` / `natureNumero` / `publicationRecueil` / `ancienId` | `ECLI:FR:CECHR:2026:506507.20260529` / `506507` / `"Texte 506507"` / `"B"` / `JG_L_2026_05_000000506507` | `[FX]/consult_juri_cetat.json:38,11,124,28,154` |
| CETAT (CAA) | `titles[].title` | `"CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429"`, `"CAA de NANCY, 3ème chambre, 22/05/2025, 22NC02259"` | `[FX]/search_cetat.json:9,93` |
| CETAT | texte : renvois aux juges du fond | `"Par un jugement nos 2005064, 2006015, 2006016 du 30 juin 2022, le tribunal administratif a rejeté sa demande. Par un arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy …"` | `[FX]/consult_juri_cetat.json:68` |
| CONSTIT | `titre` | `"Décision 2025-1173 QPC - 07 novembre 2025 - M.\u00a0Amine G. [Absence de publicité …] - Conformité"` (U+00A0, jour sur 2 chiffres) | `[FX]/consult_juri_constit.json:40` |
| CONSTIT | `num` / `natureNumero` / `ecli` / `nor` / `titreJo` / `urlCC` | `2025-1173` / `"QPC 2025-1173"` / `ECLI:FR:CC:2025:2025.1173.QPC` / `CSCX2531180S` / `"JORF n°0263 du 8 novembre 2025, texte n° 152"` / `http://www.conseil-constitutionnel.fr/decision/2025/20251173QPC.htm` | `[FX]/consult_juri_constit.json:11,73,38,19,42,49` |
| CONSTIT (search) | `nor`, `nature` | `"cscx2531180s"`, `"cscl2115694s"` (minuscules), `nature: "qpc"`/`"dc"` | `[FX]/search_constit.json:30,118` |
| CNIL | `num` / `titre` / `titreLong` | `SAN-2026-002` / `"Délibération SAN-2026-002 du 8 janvier 2026"` / `"Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 prononçant …"` | `[FX]/consult_cnil.json:11,40` |
| CNIL (search) | `titles[].title` | `"… n° SAN–2026-003 du 22 janvier 2026 …"` (**U+2013** entre `SAN` et l'année) | `[FX]/search_cnil.json:53` |
| KALI | `conteneurs[0]` | `{cid: "KALICONT000005635096", num: "2121", numero: "0000000000002121", titre: "Convention collective nationale de l'édition du 14 janvier 2000\n"}` | `[FX]/consult_kali.json:145-149` |
| KALI | `titles[].title` | `"Avenant n° 18 du 19 juillet 2002 relatif à la médaille d'honneur du travail"` (`KALITEXT000005661030`) | `[FX]/search_kali.json:76` |
| KALI | `num` d'articles | `"19 (des dispositions communes de la convention collective nationale de l'édition phonographique)"`, `"8.4 de la convention)"`, `"8.4 de la convention),  2"` (valeurs amont malformées) | `[FX]/consult_kali.json` ; `[FX]/search_kali.json:100,122` |
| ACCO | `numero` / `codeIdcc` | `"T09426064936"` / `"275"` ; autre hit `idcc: "1486"` | `[FX]/consult_acco.json:12,23` ; `[FX]/search_acco.json:75,158` |

### 2.2 Pièges concrets que le parseur doit absorber (VÉRIFIÉ sauf mention)

1. **Mojibake** (double encodage) dans `consult_article`, `consult_loda`, `consult_juri`, `search_code`, `search_loda`, `search_juri` (nombre de lignes contenant `Ã` : 1, 24, 8, 1, 3, 36) mais pas dans CETAT/CONSTIT/CNIL/KALI/ACCO. Formes : `nÂ°` (°), `Ã©` (é), `Ã` + **U+00A0** (= `à` mal décodé). Conséquence : réparer le mojibake **avant** de replier les espaces insécables, sinon `Ã\u00a0` devient `Ã ` irréparable. Cause amont (sandbox ou enregistrement) : NON VÉRIFIÉE (`[R05]` §1.14 point 5).
2. **Balises `<mark>`** dans les titres et extraits de `/search` (`[FX]/search_loda.json:9`) ; la référence les retire (`citations.py:45-52`).
3. **Espaces** : U+00A0 dans `M.\u00a0Amine G.` (`[FX]/consult_juri_constit.json:40`) ; doubles espaces avant `- Non conformité` (`[FX]/search_constit.json:9,53`) ; `\n` final dans les titres KALI.
4. **Tirets** : U+2013 dans `SAN–2026-003` ; ` - ` séparateur dans les titres CONSTIT et `2ème - 7ème` CE.
5. **Ordinaux non typographiques** dans les données officielles : `2ème`, `4ème`, `3ème` (CETAT) alors que la règle typographique est `2e` (WEB, §6).
6. **Dates** : `29/05/2026` (CETAT), `07 novembre 2025` (CONSTIT, zéro initial), `4 juin 2025` (JURI), `du 24 février 2005` (anciens titres JURI, TIERS `[JL]/web/hub.html`), ISO `2026-04-30` (titres `[JL]`), `DU 4 JUIN 2025` en majuscules (en-têtes d'arrêts).
7. **Numéros parasites dans les titres** : `loi n° 2019-222 … de programmation 2018-2022 …` et `… ministère de la justice 2023-2027` (`[FX]/consult_juri_constit.json:68`) — `2018-2022` ne doit **pas** être lu comme un numéro de texte.
8. **Pluriel** `nos 2005064, 2006015, 2006016` (CETAT) ; **lettre-clé** devant le pourvoi `Pourvoi n° N 24-13.470` (`[FX]/consult_juri.json:69`, et TIERS `pourvoi n° G 19-19.311`, `n° B 20-11.926` dans `[JL]/web/hub.html`).
9. **Liens amont inutilisables** : `liens[].title = "1 du code civil."` sans identifiants (`[FX]/consult_juri.json:105`) → l'extraction depuis le texte est la seule voie (constat partagé par `[R05]` §1.7).
10. **NOR en minuscules** dans `/search` (CONSTIT, KALI : `aset0450970m`) vs majuscules dans `/consult`.

### 2.3 Dérivations ECLI vérifiables sur les fixtures

| ECLI natif | Éléments de la décision | Règle déduite | Statut |
|---|---|---|---|
| `ECLI:FR:CCASS:2025:C100399` | 1re chambre civile, 4 juin 2025, « Arrêt n° 399 F » | ordinal = code chambre (`C1`) + numéro d'arrêt sur 5 chiffres ; année = année de la décision | VÉRIFIÉ (`[FX]/consult_juri.json:43,69`) + WEB e-Justice (« deux caractères pour la formation, suivis des 5 derniers chiffres du numéro administratif ») |
| `ECLI:FR:CECHR:2026:506507.20260529` | « 2ème - 7ème chambres réunies », 29/05/2026, n° 506507 | court = `CE` + suffixe de formation ; ordinal = `requête.AAAAMMJJ` | VÉRIFIÉ (`[FX]/consult_juri_cetat.json:38,40`) + WEB e-Justice |
| `ECLI:FR:CC:2025:2025.1173.QPC` | décision n° 2025-1173 QPC du 7 novembre 2025 | ordinal = `AAAA.N.TYPE` (année d'enregistrement) | VÉRIFIÉ (`[FX]/consult_juri_constit.json:38`) |
| `ECLI:FR:CC:2026:2025.1180.QPC` | n° 2025-1180, `DATE_DEC` 2026-02-06 | **année ECLI = année de la décision**, différente de l'année du numéro | TIERS (`[JL]/tests/test_parse_dila_champs.py:120-131`, XML DILA d'exemple) |

Correspondance chambre Cour de cassation → préfixe d'ordinal observée sur 928 décisions d'exemple Judilibre : Chambre sociale → `SO`, Deuxième chambre civile → `C2`, Chambre criminelle → `CR`, Première chambre civile → `C1`, Troisième chambre civile → `C3`, Chambre commerciale financière et économique → `CO` (TIERS, `[JDL]/src/data/sample_list.json`, analyse programmatique). Préfixes complémentaires : `OR` (ordonnance du premier président, `ECLI:FR:CCASS:2022:OR31631`, TIERS `[JL]/web/hub.html`), `AP` (assemblée plénière, `ECLI:FR:CCASS:2015:AP00623`, WEB https://reflex.sne.fr/jurisprudence-francaise via résumé), `AV` (avis, WEB e-Justice). **Chambre mixte : code NON VÉRIFIÉ** (probablement `MI`, aucune occurrence trouvée).

Conseil d'État — suffixes de formation (WEB e-Justice, https://e-justice.europa.eu/topics/legislation-and-case-law/european-case-law-identifier-ecli/fr_fr, résumé) : `CEASS` (Assemblée), `CESEC` (Section du contentieux), `CECHR` (chambres réunies), `CECHS` (chambre jugeant seule), `CESSR` (sous-sections réunies, ancien), `CESJS` (sous-section jugeant seule, ancien), `CEORD` (ordonnance) ; « le code CE n'est jamais utilisé seul ». Occurrences réelles : `CECHR`, `CEORD`, `CECHS`, `CESSR`, `CESJS` dans `[JL]` (`tests/fixtures/ariane_entetes.json`, `scratchpad/audit/inventaire_champs_13sept.md:195`) ; correspondances d'en-têtes Ariane TIERS : `"1ère CHS"` → `CECHS`, `"6-5  CHR"` → `CECHR`, `"Autres CHR"` → `CESSR` (`[JL]/web/hub.html`).

### 2.4 Valeurs de facettes réelles (utiles à la résolution) — VÉRIFIÉ (fixtures)

- **JURI `CASSATION_FORMATION`** (`[FX]/search_juri.json:370-393`) : `AVIS`, `ORDONNANCE_PREMIER_PRESIDENT`, `COUR_REVISION`, `CHAMBRE_CIVILE_1`, `COMMISSION_REPARATION_DETENTION`, `CHAMBRE_CRIMINELLE`, `CHAMBRE_MIXTE`, `CHAMBRE_CIVILE`, `COMMISSION_REVISION`, `TRIBUNAL_CONFLIT`, `CHAMBRE_SOCIALE`, `CHAMBRE_CIVILE_3`, `CHAMBRE_CIVILE_2`, `CHAMBRE_COMMERCIALE`, `ASSEMBLEE_PLENIERE`, `CHAMBRES_REUNIES`.
- **JURI `APPEL_SIEGE_APPEL`** (`[FX]/search_juri.json:233`) : 35 sièges `GRENOBLE, TOULOUSE, PAU, AMIENS, BASSE-TERRE, ROUEN, MONTPELLIER, NIMES, LYON, COLMAR, AGEN, AIX-PROVENCE, ST-DENIS-REUNION, BESANCON, CAEN, NANCY, ANGERS, BORDEAUX, BASTIA, ORLEANS, DIJON, PARIS, CHAMBERY, NOUMEA, VERSAILLES, DOUAI, RIOM, BOURGES, PAPEETE, REIMS, POITIERS, METZ, LIMOGES, FORT-DE-FRANCE, RENNES`.
- **JURI `JURIDICTION_JUDICIAIRE`** : `Cour de cassation`, `Juridictions d'appel`, `Juridictions du premier degré` ; `CASSATION_TYPE_PUBLICATION_BULLETIN` : `T`, `F` ; `CASSATION_NATURE_DECISION` : `avis`, `ordonnance`, `arret`, `autres_decisions` (`[FX]/search_juri.json:276-401`). `natureJuridiction` contient aussi `Tribunal supérieur d'appel de Mamoudzou`, `Chambre de l'application des peines du TSA de St Pierre`, des juridictions de proximité…
- **CETAT `JURIDICTION_NATURE`** (`[FX]/search_cetat.json:259-342`) : `COURS_APPEL_FINANCIERE`, `TRIBUNAL_CONFLIT`, `TRIBUNAL_ADMINISTATIF` (**sic**, sans R), `COURS_DE_DISCIPLINE`, `COURS_APPEL`, `COURS_COMPTES`, `CHAMBRES_COMPTES`, `CONSEIL_ETAT` ; sous-valeurs `COURS_APPEL` : `Douai, Nantes, Bordeaux, Marseille, Lyon, Versailles, Nancy, Toulouse, Paris` (9 CAA) ; sous-valeurs `TRIBUNAL_ADMINISTATIF` (42) : `Papeete, Nîmes, Bordeaux, Cergy-Pontoise, Pau, Marseille, Lille, Nancy, Saint-Barthélemy, Limoges, Châlons-en-Champagne, Orléans, Grenoble, Poitiers, Rouen, Nice, Saint-Martin, Saint-Pierre et Miquelon, Melun, Mamoudzou, Nantes, Besançon, Lyon, Fort-de-France, Toulouse, Dijon, Basse-Terre, Montpellier, Montreuil, Strasbourg, Amiens, Rennes, Bastia, Saint-Denis de la Réunion, Caen, Versailles, Clermont-Ferrand, Cayenne, Toulon, Nouméa, Paris, Mata-Utu`. `PUBLICATION_RECUEIL` : `NON_PUBLIE`, `PUBLIE`.
- **CONSTIT `NATURE_CONSTIT` / `NATURE_AUTRE` / `NATURE_ELCT` / `NATURE_NORME_AUTRE`** (`[FX]/search_constit.json:139-260`) — libellés et non codes : `Compétences outre-mer (LOM)`, `Dispositions promulguées (QPC)`, `Loi du pays (LP)`, `Loi ordinaire (DC)`, `Loi organique (DC)`, `Referendum d'initiative partagée (RIP)`, `Règlements des Assemblées (DC)`, `Traité (DC)` ; `Avis sur circonstances exceptionnelles`, `Déchéance parlementaire`, `Décision d'organisation interne`, `Incompatibilité`, `Obligations fiscales` ; `Autres décisions du contentieux électoral`, `Election du Président de la République`, `Election du Sénat`, `Election à l'Assemblée Nationale`, `Référendum` ; `Délégalisation (L)`, `Fin de non recevoir (FNR)`.
- **CNIL `natureDelib`** (`[FX]/search_cnil.json`) : `Autorisation de recherche`, `Autorisation de transfert`, `Autre autorisation`, `Avis`, `Certification/label`, `Cloture`, `Disposition interne cnil`, `Mise en demeure`, `Recommandation/lignes directrices`, `Referentiel/reglement type/norme`, `Sanction` ; `nature` : `Décision`, `Délibération`.
- **LODA `nature`** (`[FX]/search_loda.json`) : `ARRETE`, `DECLARATION`, `DECISION`, `LOI`, `ORDONNANCE`, `DECRET`, `DECRET_LOI`, `CONVENTION`, `CONSTITUTION`, `DELIBERATION`, `ACCORD_FONCTION_PUBLIQUE`.

### 2.5 Chaînes produites par la référence Python que le parseur doit relire

| Producteur | Forme | Source |
|---|---|---|
| article de code | `"{code_title}, art. {num}"` → `"Code civil, art. 9"` ; sinon `"Art. {num}"` | `[PY]/src/fr_eli_mcp/citations.py:256-261` ; `tests/test_parse.py:69` |
| Conseil constitutionnel | `"Cons. const., decision n° {num} {NATURE} du {D mois AAAA}[ - {premier crochet}]"` (« decision » sans accent, mois sans accent : `fevrier`, `aout`, `decembre`, jour `1` sans `er`) | `citations.py:283-318` ; `tests/test_parse.py:114-116` |
| CNIL | `"CNIL, deliberation n° {num} du {D mois AAAA}"` | `citations.py:376-380` |
| ACCO | `"{titre} ({raisonSociale})"` | `citations.py:141-142,458` |
| LODA/JURI/CETAT/KALI | titre amont brut (`title`/`titre`) | `citations.py:124,225,337,433` |

Les défauts de ces formateurs sont déjà répertoriés par l'axe 02 (`…/research/02-defauts.md:66`, D-27).

---

## 3. Normalisation préalable (avant tout parsing)

### 3.1 Pipeline (l'ordre compte)

```
texte d'origine (UTF-8)
  │ 0. (optionnel) retrait HTML : <mark>, </mark>, <br/>, <sup>…</sup>, entités &nbsp; &apos; &#8217;…
  │ 1. (optionnel, activé par défaut pour les données amont) réparation du mojibake
  │ 2. NFC (unicode-normalization) — "e\u0301" → "é"
  │ 3. repli caractère par caractère (table §3.2) : espaces, apostrophes, tirets, n°, exposants, zéro-chasse
  │ 4. vue « clé » 1:1 : minuscules + diacritiques retirés (é→e, Œ→œ…) pour les dictionnaires
  ▼
texte normalisé + OffsetMap(normalisé → origine) + vue clé (même nombre de caractères que le normalisé)
```

- **Étape 1 avant l'étape 3** : la séquence mojibake de `à` est `Ã` suivi de **U+00A0** ; replier U+00A0 en espace avant réparation détruit l'information (VÉRIFIÉ : `[FX]/consult_juri.json:92` contient `Ã\u00a0 rapprocher`). Heuristique : détecter des runs `[\u00C2\u00C3\u00C5\u00E2][\u0080-\u00BF]+` (et `\u00C3` + `\u00A0`) et tenter `encode(latin-1 | cp1252) → decode(utf-8)` **par run** ; ne garder que si le décodage réussit. Test d'acceptation suggéré par l'axe 02 : regex `[ÂÃ][\u0080-¿]|â[\u0080-¿]{2}` (`…/research/02-defauts.md:450`).
- **Pas de NFKC global** : il replierait utilement `ʳ`→`r`, `ᵉ`→`e`, `º`→`o`, `№`→`No`, mais altérerait aussi d'autres caractères (fractions, exposants numériques, ligatures). Utiliser une **table explicite** (déterministe, testée) ; NFKC peut servir de source pour construire la table. (Recommandation ; comportement NFKC des caractères cités : connaissance Unicode, NON VÉRIFIÉ ici.)
- **OffsetMap** : pour chaque octet du texte normalisé, l'offset octet correspondant dans l'origine (ou une liste de segments `(norm_start, orig_start, len, kind)` pour l'économie mémoire). Un caractère supprimé (zéro-chasse) ne crée pas d'entrée ; une expansion (`№` → `n°`) mappe tous les octets produits sur le même caractère d'origine. Le span d'une citation = `[map(norm_start), map_end(norm_end))` où `map_end` renvoie la fin du caractère d'origine.

### 3.2 Table de repli (étape 3)

| Classe | Caractères d'entrée | Sortie | Exemples réels / sources |
|---|---|---|---|
| Espaces | U+00A0 NBSP, U+202F NNBSP, U+2009 fine, U+2007, U+2002–U+2006, U+2008, U+205F, U+3000, `\t`, `\r`, `\n` | U+0020 (runs **conservés** un-pour-un ; le parseur accepte `WS+`) | `[FX]/consult_juri_constit.json:40` ; titres KALI avec `\n` |
| Zéro-chasse | U+200B, U+200C, U+200D, U+2060, U+FEFF, U+00AD (trait d'union conditionnel) | supprimé | recommandation |
| Apostrophes | U+2019 `’`, U+2018 `‘`, U+02BC `ʼ`, U+0060 (accent grave), U+00B4 `´`, U+2032 `′` | `'` | `l’urbanisme` (`[FX]/search_cetat.json`), `l’article 61-1` (`[FX]/consult_juri_constit.json:68`) |
| Tirets | U+2010, U+2011 (insécable), U+2012, U+2013 `–`, U+2014 `—`, U+2212 `−`, U+FE63, U+FF0D | `-` | `SAN–2026-003` (`[FX]/search_cnil.json:53`) |
| Signe « numéro » | `nº` (U+00BA), `n°` (U+00B0), `№` (U+2116), `n o`/`n°s` | `n°` (et `n°s` pour le pluriel) ; `no`/`nos` restent tels quels mais sont reconnus par la grammaire **seulement devant un chiffre** | WEB (Lexique IN : forme `no` ; `n°` répandu ; espace insécable après) https://societebotaniquedefrance.fr/wp-content/uploads/2020/10/Insctructions-regles-typo.pdf ; https://wikicap.ulb.be/images/4/47/Regles-typographiques-ULB.pdf (résumés) |
| Exposants ordinaux | `ʳ` U+02B3, `ᵉ` U+1D49, `ᵈ` U+1D48, `ˢ` U+02E2 ; `<sup>re</sup>` (après étape 0) | `r`, `e`, `d`, `s` (`1ʳᵉ` → `1re`) | recommandation |
| Signe degré des items | `1°` (U+00B0) ou `1º` (U+00BA) | `°` | WEB : « 1°, 2° … abréviations de primo, secundo… le signe supérieur étant un o » (https://www.thierry-lequeu.fr/data/Abreviation-des-ordinaux.pdf, résumé) |
| Guillemets | `«` `»` `“` `”` `„` | conservés (non significatifs) | — |
| Ligatures | `œ`, `æ` | conservées dans le normalisé ; `œ`→`oe` **dans la vue clé seulement** | — |

### 3.3 Vue « clé » (étape 4)

- Minuscule + suppression des diacritiques **1:1** (après NFC, chaque lettre accentuée latine est un seul scalaire ; retirer le diacritique laisse un scalaire) → la vue clé a exactement le même nombre de `char` que le texte normalisé ; on mappe les offsets clé→normalisé par indice de caractère (ou on construit la clé en ASCII sur une table Latin-1/Latin Extended-A).
- Utilisée pour : dictionnaire des codes (« Code de l'Energie » ≡ « code de l'énergie »), mois sans accents (`fevrier`), « Conseil d'Etat » ≡ « Conseil d'État », chambres (`PREMIÈRE CHAMBRE CIVILE`), majuscules d'en-têtes (`DU 4 JUIN 2025`).
- `winnow::ascii::Caseless` est **ASCII uniquement** (VÉRIFIÉ `[WIN]/src/ascii/caseless.rs:7-25`) : l'insensibilité aux accents doit passer par la vue clé.

---

## 4. Grammaire (EBNF)

### 4.0 Conventions

- Notation EBNF ISO simplifiée : `"…"` littéral **insensible à la casse et aux accents** (comparé sur la vue clé) ; `/…/` expression régulière sur le texte normalisé ; `[x]` optionnel ; `{x}` zéro ou plus ; `x+` un ou plus ; `|` alternative ; `(* … *)` commentaire.
- `WS` = un ou plusieurs espaces (après repli) ; `SEP` = `[WS] ("," | ";" | ":") [WS]` ou `WS` ; `ET` = `[WS] ("," | "et" | "ou") [WS]`.
- Toutes les règles produisent une valeur typée (§9) ; les contraintes sémantiques (date valide, chambre existante, cohérence année du numéro/date) sont vérifiées **après** le parsing et produisent des *flags*, pas des échecs silencieux.
- Priorité en cas de chevauchement (le plus long gagne, puis) : `Ecli` > `LegifranceId` > `Eli` > `Nor` > `Decision` > `CnilDeliberation` > `Convention` > `Jorf` > `TextArticle` > `Text` > `CodeArticle` > `Unsupported` (qui sert de « puits » anti-faux-positifs : un span `Unsupported` bloque les autres).

### 4.1 Lexique commun

```ebnf
DIGIT      = /[0-9]/ ;
NUM_SIGN   = ("n°" | "no" | "num." | "numéro") [WS] ;           (* "no" seulement si suivi d'un chiffre *)
NUM_SIGN_PL= ("n°s" | "nos" | "numéros") [WS] ;
ORD1_F     = "1re" | "1ère" | "1ere" | "première" | "premiere" ;  (* 1ʳᵉ → 1re par la normalisation *)
ORD1_M     = "1er" | "premier" ;
ORDN       = /[2-9]|1[0-9]/ ("e" | "ème" | "eme" | "è") | ORD_WORD ;   (* 2e, 2ème (données DILA), deuxième *)
ORD_WORD   = "deuxième" | "second" | "seconde" | "troisième" | "quatrième" | "cinquième" | "sixième"
           | "septième" | "huitième" | "neuvième" | "dixième" | "onzième" | "douzième" | "dernier" ;
DAY        = ORD1_M | /0?[1-9]|[12][0-9]|3[01]/ ;
MONTH      = "janvier" | "février" | "mars" | "avril" | "mai" | "juin" | "juillet" | "août"
           | "septembre" | "octobre" | "novembre" | "décembre"            (* accents facultatifs via vue clé *)
           | "janv." | "févr." | "fév." | "avr." | "juill." | "juil." | "sept." | "oct." | "nov." | "déc."
           | "janv" | "fevr" | "avr" | "juil" | "sept" | "oct" | "nov" | "dec" ;   (* point facultatif toléré *)
YEAR4      = /1[5-9][0-9]{2}|20[0-9]{2}/ ;
DATE_PROSE = DAY WS MONTH WS YEAR4 ;
DATE_NUM   = /(0?[1-9]|[12][0-9]|3[01])[\/.](0?[1-9]|1[0-2])[\/.]/ YEAR4 ;          (* 29/05/2026 *)
DATE_ISO   = YEAR4 "-" /(0[1-9]|1[0-2])/ "-" /(0[1-9]|[12][0-9]|3[01])/ ;          (* 2026-04-30 *)
MOIS_REPU  = "vendémiaire" | "brumaire" | "frimaire" | "nivôse" | "pluviôse" | "ventôse"
           | "germinal" | "floréal" | "prairial" | "messidor" | "thermidor" | "fructidor" ;
DATE_REPU  = DAY WS MOIS_REPU WS "an" WS ROMAN ;                               (* textes révolutionnaires *)
DATE       = DATE_PROSE | DATE_NUM | DATE_ISO | DATE_REPU ;
DATE_LINK  = ["du" | "en date du" | "le" | "des"] [WS] DATE ;
ROMAN      = /M{0,4}(CM|CD|D?C{0,3})(XC|XL|L?X{0,3})(IX|IV|V?I{0,3})/ (non vide, suivi d'une frontière de mot) ;  (* [LEGIPY]/legi/roman.py ; le look-ahead d'origine n'existe pas dans `regex` → contrôle en winnow *)
LATIN_ADV  = "bis" | "ter" | "quater" | "quinquies" | "sexies" | "septies" | "octies" | "nonies"
           | /(un|duo|ter|quater|quin|sex|sept|octo|novo)?(dec|vic|tric|quadrag|quinquag|sexag|septuag|octog|nonag)i[eè]s/
           | "semel" | "quinto" ;                     (* forme validée sur tout LEGI par [LEGIPY]/legi/articles.py:13-16 ;
                                                         "quinquedecies" et "sexties" fautifs mais présents dans LEGI *)
```

Sources du lexique : mois et abréviations (`janv., févr., avr., juill., sept., oct., nov., déc.` ; `mars`, `mai`, `juin`, `août` non abrégés ; `juil.` rencontré) — WEB, https://vitrinelinguistique.oqlf.gouv.qc.ca/23619/les-abreviations-et-les-symboles/les-abreviations/listes-dabreviations/abreviation-des-noms-de-mois et https://forums.zotero.org/discussion/comment/72635 (résumés). Ordinaux `1re, 2e, 3e et non 1ère, 2ème` et `1er/1re/2d/2de` — WEB, https://www.thierry-lequeu.fr/data/Abreviation-des-ordinaux.pdf (résumé). Mois révolutionnaires et `1 janvier` → `1er janvier` — VÉRIFIÉ `[LEGIPY]/legi/fr_calendar.py:11-14`, `[LEGIPY]/legi/titles.py:37,79`. Mois sans accents produits par la référence — VÉRIFIÉ `citations.py:283-286`.

Validation sémantique de `DATE` : jour ≤ nombre de jours du mois (bissextiles), sinon flag `date_invalide` (corpus N09, N37). Années hors [1789, année courante + 1] → flag `annee_improbable` (sauf DATE_REPU).

### 4.2 Articles de codes

```ebnf
code_article  = art_kw WS art_list [SEP sub_list] [WS] code_link code_ref [version_q]      (* article L. 761-1 du code de justice administrative *)
              | code_ref SEP art_kw WS art_item                                           (* C. civ., art. 1240 ; Code civil, art. 9 *)
              | art_kw WS art_item WS code_abbr                                           (* art. 1240 C. civ. *)
              | prefixed_num [SEP sub_list] [WS] code_link code_ref                      (* L. 1234-5 du code du travail ; R. 104-26 du code de l'urbanisme *)
              | sub_lead art_kw WS art_item [WS] code_link code_ref ;                     (* premier alinéa de l'article 396 du code de procédure pénale *)

art_kw        = "article" | "articles" | "art." | "art" | "arts" ;
art_list      = art_item { ET art_item } | art_item WS "à" WS art_item ;                 (* énumération / plage ; « au » introduit une date : « au 1er janvier 2018 » *)
art_item      = art_num [SEP sub_list] ;
art_num       = ORD1_M | "unique" | "liminaire" | "préliminaire"
              | [prefix [WS]] base_num { suffix } ;
prefixed_num  = prefix [WS] base_num { suffix } ;                                         (* préfixe OBLIGATOIRE si pas de art_kw *)
prefix        = ("LO" | "L" | "R" | "D" | "A") ["*" | "**"] ["."]                        (* L., LO., R*, R**, D*, D** *)
              ;                                                                            (* "R." suivi de chiffres *)
base_num      = DIGIT+ { "-" DIGIT+ } ;                                                   (* 1234-5 ; 600-4-1 ; 9-1 ; 397-1-1 *)
suffix        = WS LATIN_ADV                                                              (* 39 octies ; 238 bis *)
              | WS /[A-Z]{1,2}/ (?! lettre minuscule)                                      (* 199 undecies B ; 2 ter A *)
              | "-" DIGIT+ ;
sub_list      = sub { SEP sub } ;
sub           = ("al." | "alinéa" | "alinéas") WS DIGIT+                                   (* al. 2 *)
              | DIGIT+ "°"                                                                  (* 2° , 3° (item) *)
              | ROMAN                                                                       (* I, II (paragraphe) *)
              | "§" [WS] DIGIT+
              | /[a-z]\)/ ;                                                                 (* a) *)
sub_lead      = [phrase_lead] (alinea_lead | item_lead | roman_lead) { WS ("du" | "de la" | "de l'") WS (alinea_lead | item_lead | roman_lead) } WS ("de l'" | "du" | "de la" | "des") [WS] ;
phrase_lead   = (ORD1_F | ORD_WORD | "dernière") WS "phrase" WS "du" WS ;
alinea_lead   = (ORD1_M | ORD_WORD) WS "alinéa" ;                                          (* premier alinéa ; troisième alinéa ; dernier alinéa *)
item_lead     = ["le" WS] DIGIT+ "°" ;                                                      (* (le) 3° du I de l'article … ; le span commence au chiffre (P040) *)
roman_lead    = ["le" WS] ROMAN ;
code_link     = "du" WS | "de la" WS | "de l'" | "des" WS | ε ;
code_ref      = code_name | code_abbr | code_anaphora ;
code_name     = "code" WS <titre du dictionnaire §5, sans « Code »> | "livre des procédures fiscales" ;
code_abbr     = <abréviation du dictionnaire §5> ;
code_anaphora = "même code" | "dudit code" | "du présent code" | "de ce code" | "dudit Code" ;
version_q     = WS "ancien" | WS "(ancien)" | WS "nouveau"
              | SEP ("dans sa rédaction" | "dans leur rédaction") WS
                    ( "applicable au litige" | "applicable à la date des faits" | "en vigueur" [WS DATE_LINK]
                    | ("issue de" | "résultant de" | "antérieure à celle issue de" | "antérieure à") WS text_ref )
              | SEP "en vigueur au" WS DATE ;
```

**Clé canonique `num_key`** (forme interrogeable `NUM_ARTICLE`) : préfixe sans point ni espace + numéro + suffixes séparés d'une espace : `L. 761-1` → `L761-1` ; `LO. 141` → `LO141` ; `R* 111-1` → `R*111-1` (**stockage LEGI de l'astérisque NON VÉRIFIÉ** : `[JRX]` normalise `R*4127-1` → `R4127-1`, `verify.rs:2064` ; tester les deux) ; `39 octies` → `39 octies` ; `1er` → candidats `["1er", "1"]` (NON VÉRIFIÉ : LEGI semble contenir les deux formes, `[LEGIPY]/legi/articles.py:20` accepte `1er`). Sources : valeur `"L36-11"` envoyée en `NUM_ARTICLE` (`[DILA]/exemples-d-utilisation-de-l-api.md:181-189`) ; `legi_articles.num` « sans ponctuation : 'R772-8', pas 'R. 772-8' » (TIERS `[JL]/sources/citations.py:9-12`, `_normalize_num` `:44-49`) ; `getArticleWithIdAndNum` : exemple `5-8` dans le Swagger, `L5-8` dans le xlsx (`[R05]` §1.4).

**Préfixes** (VÉRIFIÉ `[DILA]/lexique-api-lgf.md:337-355`) : `L` partie législative (`LO` loi organique), `R` réglementaire décret en Conseil d'État, `D` décret simple, `A` arrêtés ; `R*`/`D*` = décret (en CE / simple) pris en conseil des ministres. `R**`/`D**` acceptés par `[LEGIPY]/legi/articles.py:20` (`[RD]\*{1,2}`).

**Numérotation d'insertion** : en code on privilégie le tiret (`17-1`) plutôt que les locutions latines, `bis` seulement si le texte en comporte déjà (WEB, fiche Légifrance « 3.4.1 Modifications et insertions », https://www.legifrance.gouv.fr/contenu/Media/files/autour-de-la-loi/guide-de-legistique/2026_01_14_fiche_3.4.1_modifications_et_insertions.pdf, résumé) ; en navette, `2 ter A` (lettre capitale) (WEB, Sénat, https://www.senat.fr/fileadmin/Seance/Documents/Guides_pratiques/Le_montage_des_textes.pdf, résumé).

**Règles de désambiguïsation (obligatoires)** :

1. `L.` / `D.` / `Ord.` suivis de `[WS] n°` → **nature de texte** (loi / décret / ordonnance), pas préfixe d'article (corpus P068–P070).
2. `D.` suivi de `YEAR4` puis `, p.` ou `, somm.` ou `, chron.` → **Recueil Dalloz** (`Unsupported/doctrine`), jamais un article (N33).
3. Sans `art_kw`, un numéro n'est un article que s'il porte un **préfixe** (`L. 1234-5`) **et** est suivi d'un `code_ref`, ou s'il fait partie d'une énumération ouverte par `art_kw` (« articles L. 104-1 et L. 104-2 du code… »). Un nombre nu (« 1 du code civil. », N01) n'est **pas** émis en mode texte libre.
4. Suffixe minuscule collé (`L. 1234-5a`) → pas de citation (ne pas tronquer en `L1234-5`, N36 ; idée `[JRX]/…/verify.rs:2021`).
5. `art_kw` sans `code_ref` ni texte → `CodeArticle{code: None}` + `ambiguity = texte_manquant` (N03, N26) ; préfixe sans code → `code_manquant` (N02).
6. Sigles ambigus (`CE`, `CP`, `CC`, `CT`, `CU`, `CPI`, `CCP`) → `confidence = Low` et `ambiguity = sigle_ambigu:<X>` ; `CE` n'est **jamais** résolu en code sans contexte (N04, N05 ; §5.3).

### 4.3 Textes (lois, ordonnances, décrets, arrêtés…) et articles de textes

```ebnf
text_ref      = nature [WS emitter] [WS NUM_SIGN text_num] [WS DATE_LINK] [WS title_hint] [WS text_anaphora]
              | nature WS appellation ;                                                 (* loi Macron → résolution /suggest *)
nature        = "loi" [WS ("organique" | "constitutionnelle" | "de finances" [WS "rectificative"] [WS "pour" WS YEAR4]
                           | "de financement de la sécurité sociale" [WS "pour" WS YEAR4] | "du pays" | "locale")]
              | "L." (* seulement si suivi de n° *)
              | "ordonnance" | "ord." | "décret" | "décret-loi" | "décr." | "D." (* seulement si suivi de n° *)
              | "arrêté" | "arr." | "décision" | "délibération" | "règlement" | "circulaire" | "instruction" ;
emitter       = "ministériel" | "interministériel" | "préfectoral" | "du Conseil d'État" | "du Roi" ;   (* [LEGIPY]/legi/titles.py:41 *)
text_num      = (YEAR4 | /[0-9]{2}/) "-" DIGIT+ ["-" DIGIT+]                               (* 2016-1321 ; 58-1067 ; 93-1419 *)
              | DIGIT+ ;                                                                    (* numérotations anciennes/locales : NON VÉRIFIÉ *)
title_hint    = ("portant" | "relatif" | "relative" | "relatifs" | "pour" | "sur" | "de" | "d'" | "tendant"
              | "visant" | "modifiant" | "pris" | "prise" | "transposant" | "habilitant") <texte jusqu'à ; . ( [ ou fin, borné à 300 car.> ;
text_anaphora = "susvisé" ["e"] ["s"] | "précité" ["e"] ["s"] | "mentionné" ["e"] ["s"] WS "ci-dessus" | "ci-dessus mentionné" ["e"] ;
text_article  = art_kw WS art_list [SEP sub_list] [WS "au" WS DATE] WS ("de la" | "de l'" | "du" | "de") [WS] text_ref
              | sub_lead art_kw WS art_list WS ("de la" | "de l'" | "du") [WS] text_ref ;
NOR           = /[A-Z]{3}[A-Z][0-9]{2}[0-9]{5}[A-Z]/ ;                                       (* §4.13 *)
```

- **Numéro** : `AA-N` avec année sur 2 chiffres pour les textes du XXe siècle (`ordonnance n° 58-1067`, `loi n° 86-1067`, `loi n° 93-1419`, `loi n° 98-389` — VÉRIFIÉ fixtures/DILA) et sur 4 chiffres ensuite (`2016-1321`, `2019-222`, `2008-724`…). Normaliser `year = 1900 + AA` pour 2 chiffres. Date exacte de bascule (2000 ?) : **NON VÉRIFIÉ** (WebSearch sans source ; connaissance générale « loi n° 99-… puis n° 2000-… »).
- **Contrôle anti-hallucination** : si `text_num` et `DATE` sont présents et `year(num) ≠ year(date)` → flag `annee_numero_differente_annee_date` (corpus N07). Le numéro porte en principe l'année de signature (cohérent avec toutes les fixtures : 58-1067 / 7 novembre 1958 ; 2016-1321 / 7 octobre 2016 ; 2019-222 / 23 mars 2019) — règle générale NON VÉRIFIÉE (cas limites de fin d'année possibles) → flag *suspect*, pas rejet.
- **Ne pas lire** `2018-2022` (« de programmation 2018-2022 ») comme numéro : `text_num` n'est accepté qu'après `NUM_SIGN` ; `title_hint` est consommé d'un bloc (P056, P057).
- Formes à date seule : « loi du 20 novembre 2023 mentionnée ci-dessus », « ordonnance du 7 novembre 1958 susvisée », « loi du 17 juillet 1978 », « règlement du 4 février 2010 » (VÉRIFIÉ fixtures / DILA) → `Text{num: None, date}` + anaphore.
- Normalisation de `n°` en entrée de titres : `n°°`, `n° °`, `n°X` collé → `n° ` (`[LEGIPY]/legi/titles.py:36,88`) ; numéro avec tiret demi-cadratin accepté (`titles.py:47,130`).
- Natures LEGI connues : `ARRETE, DECISION, DECLARATION, DECRET, DECRET_LOI, LOI_CONSTIT, LOI_LOCALE, LOI_ORGANIQUE` + `LOI, ORDONNANCE, CODE, CONSTITUTION, CONVENTION` (`[LEGIPY]/legi/titles.py:20-29,44`) ; facette LODA (§2.4).

### 4.4 Blocs constitutionnels

```ebnf
const_block   = constitution | ddhc | preambule46 | charte_env ;
constitution  = "la Constitution" [WS ("du 4 octobre 1958" | "de 1958")] | "Const." ;
ddhc          = "Déclaration des droits de l'homme et du citoyen" [WS ("du 26 août 1789" | "de 1789")]
              | "Déclaration de 1789" | "DDHC" ;
preambule46   = "Préambule de la Constitution du 27 octobre 1946" | "Préambule de 1946" ;
charte_env    = "Charte de l'environnement" [WS "de 2004"] ;
const_article = art_kw WS art_list WS ("de la" | "de l'" | "du") WS const_block
              | (ORD_WORD | DIGIT+ "e") WS "alinéa" WS "du" WS preambule46 ;                 (* onzième alinéa du Préambule … *)
anaphore      = ("Son" | "son") WS "article" WS art_num ;                                    (* « Son article 16 dispose » : contexte *)
```

Formes réelles : « l’article 61-1 de la Constitution », « articles 6, 8, 9 et 16 de la Déclaration des droits de l’homme et du citoyen de 1789 », « l’article 6 de la Déclaration de 1789 », « Son article 16 dispose » (VÉRIFIÉ `[FX]/consult_juri_constit.json:68`) ; « article 54 de la Constitution » (`[DILA]/exemples-d-utilisation-de-l-api.md:595`). Sigle `DDHC` : TIERS (`[JL]/sources/citations.py:55`).

### 4.5 Cour de cassation

```ebnf
cass_decision = cass_court [SEP cass_chamber] SEP [kind WS] [DATE_LINK | DATE] SEP cass_numbers
                    { SEP publication } [WS "(" solution ")"] [SEP form_pub_code]
              | cass_short SEP DATE SEP cass_numbers { SEP publication } [WS "(" solution ")"]   (* style citationJp *)
              | lf_title_cass                                                                      (* titre Légifrance *)
              | cass_court WS "(" cass_chamber_long SEP "arrêt" WS NUM_SIGN DIGIT+ WS "du" WS DATE ")"  (* style CC *)
              | "Arrêt" WS NUM_SIGN DIGIT+ [WS form_letter] WS "Pourvoi" WS NUM_SIGN [key_letter] pourvoi ;  (* en-tête *)
cass_court    = "Cass." | "Cass" | "Cour de cassation" | "C. cass." ;
cass_chamber  = civ_n | "civ." | "com." | "soc." | "crim." | "ass. plén." | "ass. plén" | "AP"
              | "ch. mixte" | "ch. mixt." | "ch. réunies" | "req." | "avis" ;
civ_n         = (ORD1_F | ORDN) WS "civ." | "civ." WS (ORD1_F | ORDN) | "Civ." WS DIGIT ;
cass_short    = (ORD1_F | ORDN) WS "Civ." | "Civ." WS (ORD1_F | ORDN) | "Com." | "Soc." | "Crim."
              | "Ass. plén." | "Ch. mixte" ;                       (* seulement suivi de SEP DATE : sinon pas une chambre (N18, N19) *)
cass_chamber_long = ("Première" | "Deuxième" | "Troisième") WS "chambre civile" | "chambre commerciale" ["," WS "économique et financière" | WS "financière et économique"]
              | "chambre sociale" | "chambre criminelle" | "assemblée plénière" | "chambre mixte" | "chambres réunies" ;
lf_title_cass = "Cour de cassation" SEP ("civile" | "criminelle") SEP "Chambre" WS ("civile" WS DIGIT | "commerciale" | "sociale" | "criminelle")
                SEP DATE_LINK SEP pourvoi SEP ("Publié au bulletin" | "Inédit" | <autre libellé>)
              | "Cour de Cassation" SEP "Chambre" WS ... SEP "du" WS DATE SEP pourvoi SEP ... ;      (* ancien format *)
kind          = "avis" | "AVIS" | "ord." | "ordonnance" | "arrêt" ;
cass_numbers  = ["pourvoi" WS | "pourvois" WS] (NUM_SIGN | NUM_SIGN_PL) [key_letter] pourvoi { ET [key_letter] pourvoi }
              | pourvoi { ET pourvoi } ;
key_letter    = /[A-Z]/ WS ;                                           (* Pourvoi n° N 24-13.470 *)
pourvoi       = /[0-9]{2}-[0-9]{2}\.[0-9]{3}/ | /[0-9]{2}-[0-9]{5}/ ;  (* forme sans point : TIERS [JL] « numero »: "19-19311" *)
publication   = "Publié au bulletin" | "Publié au Bulletin" | "Inédit" | "Bull." <…jusqu'à n° DIGIT+> | "Rapport" ;
bulletin      = "Bull." [WS ("civ." | "crim." | "Ass. plén." | "ch. mixte")] [SEP] [YEAR4] [SEP ROMAN] SEP NUM_SIGN DIGIT+ ;
form_pub_code = /(FP|FS|FM|F)(-[PBRIDL+]+)?/ ;                         (* FS-B, F-D, FS-P+B+R+I : NON VÉRIFIÉ exhaustivement *)
form_letter   = /[A-Z]{1,3}(-[A-Z+]+)?/ ;                              (* « Arrêt n° 399 F » *)
solution      = "rejet" | "cassation" | "cassation partielle" | "cassation sans renvoi" | "irrecevabilité" | "non-lieu" | <mot(s)> ;
```

Correspondances (pour le type `CassChamber`) :

| Variantes reconnues | `CassChamber` | Facette JURI `CASSATION_FORMATION` (VÉRIFIÉ §2.4) | Préfixe ECLI | Taxon Judilibre (`[JDL]/src/taxons/chamber.js:1-14`) |
|---|---|---|---|---|
| `Cass. 1re civ.`, `Cass. civ. 1re`, `Cass. Civ. 1ère`, `1re Civ.`, `Civ. 1re`, `Civ. 1`, `CIV. 1`, `Chambre civile 1`, `Première chambre civile`, `PREMIÈRE CHAMBRE CIVILE` | `Civ1` | `CHAMBRE_CIVILE_1` | `C1` | `civ1` |
| `2e civ.`, `2e Civ.`, `Civ. 2e`, `Chambre civile 2`, `Deuxième chambre civile` | `Civ2` | `CHAMBRE_CIVILE_2` | `C2` | `civ2` |
| `3e civ.`, `3e Civ.`, `Chambre civile 3`, `Troisième chambre civile` | `Civ3` | `CHAMBRE_CIVILE_3` | `C3` | `civ3` |
| `Com.`, `Cass. com.`, `Chambre commerciale`, `Chambre commerciale financière et économique` | `Com` | `CHAMBRE_COMMERCIALE` | `CO` | `comm` |
| `Soc.`, `Cass. soc.`, `Chambre sociale` | `Soc` | `CHAMBRE_SOCIALE` | `SO` | `soc` |
| `Crim.`, `Cass. crim.`, `Chambre criminelle` | `Crim` | `CHAMBRE_CRIMINELLE` | `CR` | `cr` |
| `Ass. plén.`, `Cass. ass. plén.`, `Assemblée plénière` | `AssPlen` | `ASSEMBLEE_PLENIERE` | `AP` (WEB) | `pl` |
| `Ch. mixte`, `Cass. ch. mixte`, `Chambre mixte` | `Mixte` | `CHAMBRE_MIXTE` | **NON VÉRIFIÉ** | `mi` |
| `Ch. réunies` (historique) | `Reunies` | `CHAMBRES_REUNIES` | NON VÉRIFIÉ | `creun` |
| `Cass. civ.` (sans numéro, chambre civile unique historique) | `CivUnique` | `CHAMBRE_CIVILE` | NON VÉRIFIÉ | — |
| `Cass. req.` (chambre des requêtes, historique) | `Requetes` | NON VÉRIFIÉ | NON VÉRIFIÉ | — |
| `ord.`, `ordonnance du premier président` | `PremierPresident` | `ORDONNANCE_PREMIER_PRESIDENT` | `OR` (TIERS) | `ordo` |
| `avis`, `AVIS` | `Avis` (kind) | `AVIS` | `AV` (WEB) | — |

Sources des formes : `1re Civ., 25 mai 2023, pourvoi n° 21-23.174 (cassation partielle)` (`[FX]/consult_juri.json:92`) ; `Crim., 28 mars 2018, pourvoi n° 17-82.138, Bull. crim. 2018, n° 59 (cassation).` (TIERS `[JDL]/src/data/sample_detail_resolved.json`, `rapprochements[0].title`, lié à `JURITEXT000036780011`) ; `1re Civ., 25 avril 2007, pourvoi n° 04-17.632, Bull. 2007, I, n° 159 (rejet)` (TIERS `[JL]`) ; `Cass. Civ. 1ère, 17 déc. 2010, n°10-10.906, Bull. civ., I, n°270` (WEB, https://bu.univ-lorraine.fr/sites/default/files/users/user779/Guide%20biblio%20Droit%202025-2026.pdf, résumé) ; `Cass. 1re civ., 2 juill. 2014, n° 13-21.929` et « les éditeurs peuvent omettre Cass. » (WEB, https://reflex.sne.fr/sites/default/files/guide/guide-fr-4.pdf et https://www.universite-paris-saclay.fr/sites/default/files/2021-09/bu_liste_abreviations_juridiques.pdf, résumés) ; titres Légifrance (`[FX]/consult_juri.json:45`, `[FX]/search_juri.json:122`, TIERS ancien format `Cour de Cassation, Chambre civile 2, du 24 février 2005, 04-10.362, Publié au bulletin`). Taxonomies de publication Judilibre `b` Bulletin, `r` Rapport, `l` Lettres de chambre, `c` Communiqué, `n` Non publié (`[JDL]/src/taxons/publication.js:1-7`) ; formations `fp`, `fm`, `fs`, `f`, `frh`, `frr` (`[JDL]/src/taxons/formation.js:1-8`).

**Numéro de pourvoi** : structure `AA-NN.NNN` (VÉRIFIÉ, nombreuses occurrences) ; les deux premiers chiffres semblent l'année d'enregistrement — signification du second bloc **NON VÉRIFIÉE** (WebSearch sans réponse). Observations TIERS sur `[JDL]/src/data/sample_list.json` : QPC `20-40.060`, `20-40.059` ; avis `19-70.006` (`[JL]`) ; pénal `20-84.116`, `17-82.138` — suggère des séries par nature de procédure (NON VÉRIFIÉ). Judilibre détecte un pourvoi par `/\D?\d\d\D?\d\d\D?\d\d\d\D?/` puis retire les non-chiffres (`[JDL]/src/modules/elastic/query.js:51-56`).

### 4.6 Juridictions judiciaires du fond (CA, TJ, TC, CPH…)

```ebnf
ca_decision   = ca_court WS seat SEP [ca_chamber SEP] DATE_LINK SEP rg_part
              | "N°" WS "RG" WS rg [WS "-" WS "N°" WS "Portalis" WS portalis] ;           (* en-tête CA *)
ca_court      = "CA" | "C.A." | "Cour d'appel de" | "Cour d'appel d'" | "cour d'appel de" ;
seat          = <dictionnaire des 36 sièges de CA : facette APPEL_SIEGE_APPEL + taxon Judilibre> ;
ca_chamber    = "Pôle" WS DIGIT+ WS "-" WS "Chambre" WS DIGIT+                              (* Pôle 1 - Chambre 12 *)
              | "pôle" WS DIGIT+ SEP "ch." WS DIGIT+
              | (ORD1_F | ORDN) WS ("Chambre" | "ch.") [WS <qualificatif>]                   (* 6e Chambre ; 1re Chambre *)
              | "chambre" WS <libellé> | "CHAMBRE" WS DIGIT+ WS "SECTION" WS DIGIT+ ;
rg_part       = [NUM_SIGN] ["RG" WS] rg ;
rg            = /[0-9]{2}\/[0-9]{4,5}/ ;                                                     (* 24/15215 ; 14/00554 *)
portalis      = /[0-9A-Z]{4}-[A-Z]-[A-Z0-9]{3}-[A-Z0-9]{5}/ ;                                (* 35L7-V-B7H-CIDHB, forme observée *)
tj_decision   = ("TJ" | "Tribunal judiciaire de" | "Tribunal judiciaire d'") WS seat_tj SEP DATE SEP rg_part ;
autres        = ("T. com." | "Tribunal de commerce de") ... /[0-9]{4}[A-Z]?[0-9]{5,6}/ (* 2024G00010, 2024038196 : TIERS *)
              | ("CPH" | "Conseil de prud'hommes de") ... ;
```

Formes réelles : `Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215`, `Cour d'appel de Versailles, 6e Chambre, 4 juin 2020, n°17/04940` (WEB, https://www.labase-lextenso.fr/jp-commente/ca/2025-06/24-15215-CAPARIS-23062025-24_15215 et pages voisines, résumés ; « le premier chiffre fait référence au pôle, le second à la chambre » — WEB Ref-Lex) ; `Cour d'appel de Limoges, 21 mai 2015, 14/00554` (titre Légifrance, TIERS `[JL]`, `JURITEXT000030629569`) ; `N° RG 23/13690 - N° Portalis 35L7-V-B7H-CIDHB`, formation `Pôle 5 - Chambre 9`, `CHAMBRE 2 SECTION 1` (TIERS `[JL]/web/hub.html`) ; numéros de tribunaux de commerce `2024G00010`, `2024038196` (TIERS `[JDL]/src/data/tcom/sample_list.json`). Les échantillons CA/TJ de `[JDL]` sont manifestement synthétiques (chambres de Cour de cassation recopiées) : ne pas en tirer de règle.

### 4.7 Conseil d'État

```ebnf
ce_decision   = ce_court SEP [ce_formation SEP] DATE_LINK SEP [parties SEP] ce_numbers [SEP lebon]
              | lf_title_ce ;
ce_court      = "CE" | "C.E." | "Conseil d'État" | "Conseil d'Etat" | "Cons. d'État" ;
ce_formation  = "ass." | "Ass." | "assemblée" | "sect." | "Sect." | "section"
              | ch_reunies | ch_seule | ssr | ssjs | "ord." | "réf." | "juge des référés" | "avis" ;
ch_reunies    = ORDN { [WS] ("et" | "-" | "/" | ",") [WS] ORDN } WS ("ch." | "chambres") WS "réunies" | "CHR" ;   (* 2ème - 7ème chambres réunies *)
ch_seule      = (ORD1_F | ORDN) WS ("ch." | "chambre") [WS "jugeant seule"] | "CHS" ;
ssr           = DIGIT+ [WS] "/" [WS] DIGIT+ WS "SSR" | ORDN WS "et" WS ORDN WS "sous-sections réunies" | "SSR" ;    (* 7 / 5 SSR *)
ssjs          = (ORD1_F | ORDN) WS "sous-section" [WS "jugeant seule"] | "SSJS" ;
ce_numbers    = [("req." | "requête") WS] (NUM_SIGN | NUM_SIGN_PL) ce_num { ET ce_num } | ce_num ;
ce_num        = /[0-9]{4,6}/ ;                                       (* 506507 ; 216642 ; 74052 ; 99522 *)
lebon         = ("Rec." | "Leb." | "Lebon" | "Rec. Lebon" | "recueil Lebon") [WS ("T." | "tables")] [SEP] ["p." WS DIGIT+]
              | "Publié au recueil Lebon" | "mentionné aux tables du recueil Lebon" | "Inédit au recueil Lebon" ;
lf_title_ce   = ("Conseil d'État" | "Conseil d'Etat") SEP <formation libre> SEP ["du" WS] (DATE_NUM | DATE_PROSE) SEP ce_num [SEP lebon] ;
```

Formes réelles : `Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507` (`[FX]/consult_juri_cetat.json:40`) ; `Conseil d'Etat, 7 / 5 SSR, du 28 décembre 2001, 216642, mentionné aux tables du recueil Lebon` (WEB, https://www.legifrance.gouv.fr/ceta/id/CETATEXT000008089130…, titre via résumé) ; `CE, 14 avril 2023, n° 462797, mentionné aux tables du Recueil Lebon` (WEB, https://juricaf.org/recherche/references:%22Mentionn%C3%A9+aux+tables+du+recueil+Lebon%22, résumé) ; `CE, Sect., 22 février 2007, req. n° 264541` + `Leb., p. 541` (WEB, https://droit-oi.univ-reunion.fr/fileadmin/Fichiers/droit-oi/Revues/Publier_a_la_rjoi/charte_graphique_de_la_rjoi.pdf, résumé) ; « Conseil d'État N° 99522 ECLI:FR:CESSR:1976:99522.19761215 Publié au recueil Lebon Section du Contentieux » (TIERS `[JL]/web/hub.html`). Codes de publication Lebon : `B` mentionné aux tables, `C` inédit (WEB, résumé sur https://fr.wikipedia.org/wiki/Recueil_Lebon) ; `A` pour « publié au Recueil » : NON VÉRIFIÉ ; cohérent avec `publicationRecueil: "B"` de la fixture CE et la facette CETAT `PUBLIE`/`NON_PUBLIE`. **Attention** : côté JURI, la fixture porte `publicationRecueil: "C"` pour un arrêt « Publié au bulletin » (`[FX]/consult_juri.json:30,45`) — la sémantique de ce champ diffère selon le fonds (NON VÉRIFIÉE pour JURI) ; ne pas l'utiliser pour vérifier une mention de publication Cass. Renommage des sous-sections en « chambres » par la loi déontologie d'avril 2016 (WEB, https://www.conseil-etat.fr/site/actualites/changement-d-appellation-des-sous-sections-en-chambres, résumé ; date d'effet NON VÉRIFIÉE) → `SSR`/`SSJS` pour les décisions anciennes, `CHR`/`CHS` ensuite ; le parseur accepte les deux.

Correspondance formation → suffixe ECLI : `ass.` → `CEASS` ; `sect.` → `CESEC` ; chambres réunies → `CECHR` ; chambre (jugeant) seule → `CECHS` ; sous-sections réunies → `CESSR` ; sous-section jugeant seule → `CESJS` ; ordonnance/référé → `CEORD` ; **avis contentieux : NON VÉRIFIÉ**.

### 4.8 Cours administratives d'appel et tribunaux administratifs

```ebnf
caa_decision  = caa_court WS caa_seat SEP [chamber SEP] DATE_LINK SEP [NUM_SIGN] caa_num
              | ("arrêt" | "ordonnance") WS NUM_SIGN caa_num WS "du" WS DATE [SEP | WS] ("la" WS)? "cour administrative d'appel de" WS caa_seat ;
caa_court     = "CAA" | "C.A.A." | "CAA de" | "Cour administrative d'appel de" | "Cour administrative d'appel d'" ;
caa_seat      = "Paris" | "Versailles" | "Marseille" | "Lyon" | "Bordeaux" | "Nantes" | "Nancy" | "Douai" | "Toulouse" ;  (* 9, facette §2.4 *)
caa_num       = /[0-9]{2}/ caa_code /[0-9]{5}/ ;                                           (* 24MA02429 ; 22NC02259 *)
caa_code      = "PA" | "VE" | "MA" | "LY" | "BX" | "NT" | "NC" | "DA" | "TL" ;
ta_decision   = ta_court WS ta_seat SEP [formation SEP] DATE_LINK SEP [NUM_SIGN | NUM_SIGN_PL] ta_num { ET ta_num }
              | "jugement" WS (NUM_SIGN | NUM_SIGN_PL) ta_num { ET ta_num } WS "du" WS DATE [SEP] ["le" WS] "tribunal administratif" [WS ("de" | "d'") WS ta_seat] ;
ta_court      = "TA" | "T.A." | "TA de" | "Tribunal administratif de" | "Tribunal administratif d'" | "trib. adm." ;
ta_seat       = <42 sièges de la facette §2.4> ;
ta_num        = /[0-9]{7}/ ;                                                               (* 2005064 = AA + 5 chiffres *)
```

- `caa_code` : `MA` Marseille et `NC` Nancy VÉRIFIÉS (fixtures) ; `PA`, `LY`, `BX`, `NT`, `DA`, `TL` observés dans `[JL]` (comptage : NC 14, NT 12, TL 11, LY 11, PA 10, DA 10, MA 8, BX 4) avec la juridiction associée pour `NC`, `NT`, `PA`, `TL` (`[JL]/web/hub.html`) ; **`VE` (Versailles) NON VÉRIFIÉ** (aucune occurrence). Sièges des CAA et TA : VÉRIFIÉS (facette). Codes internes `CAA13`, `TA69`… de l'open data justice administrative : TIERS (`[JL]/data/juridictions_map.json`) — ne pas confondre avec ECLI.
- Formes réelles : `CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429`, `Par un arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy`, `Par un jugement nos 2005064, 2006015, 2006016 du 30 juin 2022, le tribunal administratif` (VÉRIFIÉ fixtures). Abréviations `C.A.A. %s`, `TA %s` (TIERS `[LRR]/src/juris-fr-desc.json`).
- ECLI des CAA/TA : fréquemment `null` sur Légifrance (`[PY]/DISCOVERY.md:80-82`, `[PY]/README.md:87,112`) ; **format NON VÉRIFIÉ** (aucun exemple trouvé, e-Justice ne liste que CC, CCASS, CE*). Le parseur ECLI générique l'acceptera si présent.

### 4.9 Conseil constitutionnel

```ebnf
cc_decision   = [cc_court SEP] ["déc." | "décision" | "decision"] [WS] [NUM_SIGN] cc_num WS cc_type [WS ("du" WS DATE | "-" WS DATE)] [WS "-" WS cc_tail]
              | cc_court SEP DATE SEP NUM_SIGN cc_num WS cc_type
              | cc_type WS cc_num ;                                     (* natureNumero « QPC 2025-1173 » *)
cc_court      = "Cons. const." | "CC" | "C. const." | "Conseil constitutionnel" ;
cc_num        = YEAR4 "-" DIGIT+ { "/" DIGIT+ } ;                       (* 2025-1173 ; 2010-14/22 (jonction) *)
cc_type       = "DC" | "QPC" | "LP" | "LOM" | "FNR" | "RIP" | "AR16" | "ORGA" | "PDR" | "AN" | "SEN" | "ELEC"
              | "REF" | "OF" | "AUTR" | "L" | "D" | "I" ;               (* types à 1 lettre acceptés UNIQUEMENT après cc_num *)
cc_tail       = [parties] [WS "[" sujet "]"] [WS "-" WS solution] ;      (* « M. Amine G. [Absence de …] - Conformité » *)
```

| `cc_type` | Sens | Libellé de facette observé (correspondance déduite) | Source |
|---|---|---|---|
| `DC` | contrôle a priori (loi ordinaire, organique, traité, règlements des assemblées) | `Loi ordinaire (DC)`, `Loi organique (DC)`, `Traité (DC)`, `Règlements des Assemblées (DC)` | WEB + FX |
| `QPC` | question prioritaire de constitutionnalité | `Dispositions promulguées (QPC)` | WEB + FX |
| `LP` | loi du pays (Nouvelle-Calédonie) | `Loi du pays (LP)` | WEB + FX |
| `LOM` | compétences outre-mer | `Compétences outre-mer (LOM)` | WEB + FX |
| `RIP` | référendum d'initiative partagée | `Referendum d'initiative partagée (RIP)` | FX |
| `L` | délégalisation | `Délégalisation (L)` | WEB + FX |
| `FNR` | fin de non-recevoir | `Fin de non recevoir (FNR)` | WEB + FX |
| `I` | incompatibilités | `Incompatibilité` | WEB |
| `D` | déchéance | `Déchéance parlementaire` | WEB |
| `OF` | obligations fiscales (NON VÉRIFIÉ) | `Obligations fiscales` | WEB (liste) |
| `ORGA` | organisation interne (NON VÉRIFIÉ) | `Décision d'organisation interne` | WEB (liste) |
| `AR16` | avis article 16 (NON VÉRIFIÉ) | `Avis sur circonstances exceptionnelles` | WEB (liste) |
| `PDR` | élection présidentielle | `Election du Président de la République` | WEB |
| `AN`, `SEN` | élections législatives / sénatoriales | `Election à l'Assemblée Nationale`, `Election du Sénat` | WEB + site CC |
| `ELEC` | autres contentieux électoraux (NON VÉRIFIÉ) | `Autres décisions du contentieux électoral` | WEB (liste) |
| `REF` | référendum (NON VÉRIFIÉ) | `Référendum` | WEB (liste) |
| `AUTR` | autres (NON VÉRIFIÉ) | — | WEB (liste) |

Sources : « Décision n° 2026-1191 QPC désigne la 1191e saisine de 2026 » ; liste des 18 types `DC, QPC, PDR, AN, SEN, D, I, OF, ELEC, LOM, LP, FNR, REF, RIP, L, ORGA, AR16, AUTR` (WEB, https://www.conseil-constitutionnel.fr/le-conseil-constitutionnel/presentation-generale et https://actu.dalloz-etudiant.fr/a-la-une/article/point-sur-les-differents-types-de-decisions-rendues-par-le-conseil-constitutionnel/h/4212925882b862ae6818c5e19a5c439b.html, résumés). Numéros atypiques rencontrés dans des données DILA : `2020-5684 SEN/` (titre « SEN, Haute-Saône… », texte « n° 2020-5684 SEN/QPC »), `2015-491R` (rectification d'erreur matérielle) (TIERS `[JL]/web/hub.html`) → accepter `cc_type` composé `SEN/QPC` et un suffixe `R` en mode tolérant (NON VÉRIFIÉ côté site officiel). Jonctions `2010-14/22 QPC` : forme connue (NON VÉRIFIÉ ici). URL officielle dérivable : `…/decision/{AAAA}/{AAAA}{N}{TYPE}.htm` (VÉRIFIÉ sur un cas, `[FX]/consult_juri_constit.json:49`).

### 4.10 ECLI

```ebnf
ecli          = "ECLI" ":" country ":" court ":" year ":" ordinal ;   (* casse indifférente en entrée ; ECLI normalisé en majuscules *)
country       = /[A-Z]{2}/ | "EU" ;
court         = /[A-Z][A-Z0-9]{0,6}/ ;                                (* 1 à 7 caractères, commence par une lettre *)
year          = /[0-9]{4}/ ;
ordinal       = /[A-Z0-9.]{1,25}/ ;                                   (* points autorisés, rien d'autre *)
(* Interprétations françaises (sémantiques, après parsing) *)
fr_ccass_ord  = /(C1|C2|C3|CO|SO|CR|AP|OR|AV|MI|[A-Z0-9]{2})[0-9]{5}/ ;   (* MI : NON VÉRIFIÉ *)
fr_ce_court   = "CEASS" | "CESEC" | "CECHR" | "CECHS" | "CESSR" | "CESJS" | "CEORD" ;
fr_ce_ord     = /[0-9]{1,6}\.[0-9]{8}/ ;                              (* requête.AAAAMMJJ *)
fr_cc_ord     = YEAR4 "." DIGIT+ "." cc_type ;
```

- Règles générales : 5 composants, code juridiction 1–7 caractères commençant par une lettre, ordinal ≤ 25 caractères, seuls les points sont autorisés (WEB, https://blogs.bodleian.ox.ac.uk/lawbod/2011/05/09/ecli-european-case-law-identifier/, https://eur-lex.europa.eu/content/help/eurlex-content/ecli.html, https://curia.europa.eu/jcms/jcms/P_126035/en, résumés ; conclusions du Conseil 2011/C 127/01). France : `ECLI:FR:{code}:{année de la décision}:{ordinal}`, coordinateur DILA (WEB e-Justice, page du 04/08/2020 selon le résumé).
- Regex Judilibre (indicative) : `/^ecli:\w+:\w+:\d+:[a-z0-9.]+$/i` (`[JDL]/src/modules/elastic/query.js:51`).
- **Drapeaux de suspicion** (corpus N11–N14) : court `CE` seul ; CC avec ordinal non `AAAA.N.TYPE` ; préfixe de chambre CCASS inconnu ; année > année courante ; pour CE, date de l'ordinal ≠ date de décision connue ; pour CC, `year(ecli) < year(num)`.
- Exemples tiers **faux** à ne pas reproduire : `ECLI:FR:CE:2024:123456.20240101`, `ECLI:FR:CC:2023:123.QPC` (`[PLF]/pylegifrance/fonds/juri.py:864-865`).

### 4.11 Délibérations CNIL

```ebnf
cnil_cite     = ["CNIL" SEP] cnil_kw [WS "de la formation restreinte"] [WS "CNIL"] [WS] [NUM_SIGN] cnil_num [WS "du" WS DATE] ;
cnil_kw       = "Délibération" | "délibération" | "deliberation" | "délib." | "décision" | "Décision" | "mise en demeure" ;
cnil_num      = [cnil_series "-"] YEAR4 "-" /[0-9]{3}/ ;               (* tirets après repli U+2013 *)
cnil_series   = "SAN" | "MED" | /[A-Z]{2,4}/ ;
```

Formes réelles : `SAN-2026-002`, `SAN–2026-003` (VÉRIFIÉ), `Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 …` (VÉRIFIÉ), `2012-022`, `MED-2018-024`, `SAN-2019-006`, `Délibération n°2023-062 du 13 avril 2023` (TIERS `[JL]/scratchpad/audit/inventaire_champs_13sept.md`), `SAN-2023-009` (WEB, https://www.labase-lextenso.fr/autres-sources-commente/cnil/2023-06/san-2023-009-CNILTEXT000047707063, résumé), anciens ids `MCN97020008A` (`[R05]` §1.7). `natureNumero: "DELIBERATION SAN-2026-002"` (`[FX]/consult_cnil.json:70`). Sens exact de `MED` (mise en demeure) : NON VÉRIFIÉ. Sans « CNIL » ni série, un `AAAA-NNN` est ambigu (délibérations municipales, N16). La procédure de sanction simplifiée « n'est pas publique » (WEB, https://hlc.com/fr/publications/overview-of-the-cnils-enforcement-actions-in-2024-the-simplified-procedure-generates-an-increase, résumé) → introuvable par construction.

### 4.12 Conventions collectives (KALI) et accords d'entreprise (ACCO)

```ebnf
convention    = conv_kind [WS conv_title] [WS "du" WS DATE] [WS "(" conv_ids ")"]
              | conv_ids
              | "Avenant" WS NUM_SIGN DIGIT+ [WS "du" WS DATE] [WS title_hint] [WS "à la" WS convention] ;
conv_kind     = "Convention collective nationale" | "convention collective" | "CCN" | "Accord national interprofessionnel"
              | "ANI" | "Accord" | "Annexe" WS ROMAN ;
conv_ids      = idcc_part [SEP brochure_part] | brochure_part [SEP idcc_part] ;
idcc_part     = "IDCC" [WS] [NUM_SIGN] /[0-9]{1,4}/ ;                   (* 275 ; 1486 ; 2121 *)
brochure_part = "brochure" [WS "JO"] [WS] [NUM_SIGN] /[0-9]{4}/ ;       (* 3018 *)
acco_numero   = /T[0-9]{11}/ ;                                         (* T09426064936 : forme observée, règle NON VÉRIFIÉE *)
```

Faits : IDCC `2121` = « Convention collective nationale de l'édition du 14 janvier 2000 » (`KALICONT000005635096`, `[FX]/consult_kali.json:145-149`) ; `codeIdcc` sur 3 chiffres `275` (`[FX]/consult_acco.json:23`). Syntec = IDCC 1486, brochure 3018 ; numéros de brochure entre 3001 et 3999 ; les conventions récentes n'ont plus de numéro de brochure (WEB, https://payfit.com/fr/ccn/syntec/idcc-1486/, https://alan.com/fr-fr/ressources/guides-pratiques/a/tout-savoir-utilite-code-idcc, résumés). Borne supérieure de l'IDCC (4 chiffres ?) : NON VÉRIFIÉ.

### 4.13 JORF, NOR, ELI, identifiants Légifrance

```ebnf
jorf          = ("JORF" | "JO" | "J.O." | "Journal officiel" [WS "de la République française"]) [WS NUM_SIGN /[0-9]{1,4}/]
                WS "du" WS DATE [SEP "texte" WS NUM_SIGN DIGIT+] [SEP ("p." | "page") WS DIGIT+] ;
nor           = ["NOR" [WS] [":"] [WS]] /[A-Za-z]{4}[0-9]{7}[A-Za-z]/ ;   (* ancre « NOR » OBLIGATOIRE pour une forme minuscule *)
eli           = ["https://www.legifrance.gouv.fr"] "/eli/" eli_kind "/" YEAR4 "/" /[0-9]{1,2}/ "/" /[0-9]{1,2}/ "/" eli_key "/" ("jo" | /[a-z]+/) "/" ("texte" | "article_" art_slug)
              | ["https://www.legifrance.gouv.fr"] "/eli/jo/" YEAR4 "/" /[0-9]{1,2}/ "/" /[0-9]{1,2}/ "/" /[0-9]{4}/ ;
eli_kind      = "loi" | "decret" | "ordonnance" | "arrete" | "decision" | "constitution" | /[a-z_]+/ ;
eli_key       = NOR | /[0-9]{4}-[0-9]+/ ;                                (* NOR attesté ; numéro : NON VÉRIFIÉ *)
lf_id         = lf_prefix /[0-9]{12}/ ["_" /[0-9]{2}-[0-9]{2}-[0-9]{4}/] ;
lf_prefix     = "LEGITEXT" | "LEGIARTI" | "LEGISCTA" | "JORFTEXT" | "JORFARTI" | "JORFCONT" | "JORFDOLE"
              | "JURITEXT" | "CETATEXT" | "CONSTEXT" | "CNILTEXT" | "KALITEXT" | "KALIARTI" | "KALICONT" | "KALISCTA" | "ACCOTEXT" ;
```

- JORF : `JORF n°0263 du 8 novembre 2025, texte n° 152` (`[FX]/consult_juri_constit.json:42`) ; `JORF n°0032 du 7 février 2026, texte n° 66` (TIERS). La loi n° 2016-1321 est parue au JO n° **0235** du 8 octobre 2016 d'après `numParution: "0235"` et `dateParution` = 2016-10-08 (`[FX]/consult_loda.json:17,19`) — l'exemple « JORF n° 0233 » de la tâche ne correspond pas à la fixture. Le JO électronique authentifié existe depuis juin 2004 ; avant, pas de HTML via l'API (`[R05]` §1.5).
- NOR : 12 caractères = 3 lettres ministère + 1 lettre direction + 2 chiffres année + 5 chiffres d'ordre + 1 lettre de nature (VÉRIFIÉ `[DILA]/lexique-api-lgf.md:225-237`) ; en vigueur depuis 1987 (`:227`). Exemples réels : `ECFI1524250L` (loi), `AFSA1629022D` (décret), `CSCX2531180S` (décision CC), `ASET2450455M` (texte KALI). Lettre finale `L`=loi, `R`=ordonnance, `C`=circulaire (WEB, https://wikimonde.com/article/Syst%C3%A8me_NOR et https://www.les-infostrateges.com/article/le-numero-nor, résumés) ; `D`=décret, `A`=arrêté, `S`, `M` : **cohérents avec les fixtures, sens NON VÉRIFIÉ**. Pas de clé de contrôle connue → validation purement syntaxique.
- ELI : exemples Swagger `/eli/decret/2021/7/13/PRMD2117108D/jo/article_1`, `/eli/decret/2018/2/13/JUSC1732516D/jo/texte`, JO `/eli/jo/2019/1/25/0021` (`[R05]` §1.4–1.5). L'API renvoie `eli`/`idEli` **null** pour LODA (`[PY]/DISCOVERY.md:34-40`, `citations.py:8-13`) : un ELI cité par un LLM se vérifie via `eliAndAliasRedirectionTexte`/`getArticleWithIdEliOrAlias`, jamais ne se fabrique.
- Identifiants Légifrance : préfixes de `citations.py:34-44` + `LEGISCTA`, `KALISCTA`, `KALICONT`, `JORFCONT`, `JORFDOLE`, `JORFARTI` vus dans les fixtures/le Swagger ; suffixe de version `_JJ-MM-AAAA` (`citations.py:55-63`). Les ids de titrage `JURINOME…`/`CETANOME…` ne sont pas des citations.

### 4.14 Hors périmètre reconnu (pour éviter les faux positifs)

```ebnf
eu_act        = ("directive" | "règlement" | "décision") [WS "(" ("UE" | "CE" | "CEE" | "Euratom") ")"] [WS NUM_SIGN]
                /[0-9]{2,4}\/[0-9]{1,4}(\/(CE|CEE|UE))?/ [WS "du Conseil" | WS "du Parlement européen et du Conseil"] [WS DATE_LINK] ;
cjue          = ("CJUE" | "CJCE" | "Trib. UE") SEP DATE SEP ["aff." WS] /[CT]-[0-9]{1,4}\/[0-9]{2}/ ;
cedh          = ("CEDH" | "Cour EDH") SEP DATE SEP [parties SEP] ["req." WS] [NUM_SIGN] /[0-9]{3,5}\/[0-9]{2}/ ;
doctrine      = "D." WS YEAR4 SEP ("p." | "somm." | "chron." | "jur.") ... | "JCP" [WS ("G" | "E" | "A" | "S" | "N")] WS YEAR4 SEP DIGIT+
              | "RTD civ." | "RTD com." | "Gaz. Pal." | "AJDA" | "RFDA" | "Dr. soc." | "Rev. crit. DIP" ... ;
```

Formes réelles : `directive 85/374/CEE du Conseil du 25 juillet 1985` (`[FX]/consult_juri.json:92`), `Règlement (UE) 2016/679` et `ECLI:EU:C:2024:216`, `Affaire C-740/22` (TIERS `[JL]/web/hub.html`) ; motifs CJUE `([CT])-(\d{1,4})/(\d{2})`, CEDH `(\d{3,5})/(\d{2})`, CELEX `6\d{4}[A-Z]{2}\d{4}` (TIERS `[JL]/prototypes/citation_router.py:44-51`). Résolution : non via Légifrance (EUR-Lex / HUDOC) → `resolvable = false`.

### 4.15 Énumérations, plages, anaphores et contexte

- **Énumérations** : « articles L. 104-1 et L. 104-2 du code de l'urbanisme », « articles 6, 8, 9 et 16 de la Déclaration… », « articles L. 122-4, L. 122-14 et L. 122-14-3 du Code du travail » → une `Citation` par numéro, chacune avec **son propre span** (celui du numéro) et un `enum_group` commun ; le `code_ref` final est distribué (idée commune à `[JL]/sources/citations.py:59-112` et `[JRX]` « Énumération partageant un code final », `verify.rs:790`).
- **Plages** : « articles 1100 à 1100-2 » → `range: (from, to)` ; la résolution énumère via la table des matières (pas d'arithmétique sur les numéros).
- **Anaphores de code** : « du même code », « dudit code », « du présent code » → code de la dernière citation de code du **même document** (P006, P007, P030, P033). « Son article 16 » → dernier texte cité. Sans antécédent → `ambiguity = anaphore_non_resolue`.
- **Anaphores de texte** : « susvisée », « mentionnée ci-dessus », « précitée » → appariement avec un texte de même nature et même date cité plus haut (visas) ; sinon `Text{num: None, date}` à résoudre par recherche.
- **Contexte implicite** : « l'article R. 132-5 » à l'intérieur d'une citation entre guillemets d'un article du Code de l'urbanisme (N41), « articles 141-2 et 141-4 » à l'intérieur d'un article du CPP (P009) → `code_via = contexte_document` ; le domaine expose un paramètre optionnel `default_text: Option<TextRef>` (ex. l'article consulté) pour ce cas.
- **Qualificatifs de version** : « ancien », « dans sa rédaction applicable au litige », « dans sa rédaction issue de la loi du 23 mars 2019 », « dans leur rédaction antérieure à celle issue de l'ordonnance n° 2016-131 du 10 février 2016 » (VÉRIFIÉ/TIERS) → `VersionQualifier`, utilisé par la résolution (date de version) et par `fr_diff_article`.

---

## 5. Dictionnaire des codes

### 5.1 Principes

- **Le domaine ne doit pas coder en dur les `LEGITEXT`** : la liste faisant foi est `POST /list/code` (`CodeListRequest {pageNumber, pageSize ≤ 100, sort: TITLE_ASC, states[], codeName}` → `results[{id, cid, titre, etat, dateDebut, dateFin, …}]`, `[R05]` §1.9), à mettre en cache par l'adapter et à exposer comme resource/completion MCP. Le **domaine** porte seulement : titre officiel (valeur exacte attendue par la facette `NOM_CODE`/`TEXT_NOM_CODE`, `[DILA]/exemples-d-utilisation-de-l-api.md:203-223`), abréviations, alias, et un `CodeKey` stable (énumération Rust).
- Le tableau ci-dessous sert à **construire le dictionnaire d'abréviations** et un jeu de tests d'intégration `/list/code` ↔ dictionnaire. Les `LEGITEXT` sont **TIERS** : `JL` = `[JL]/sources/legi.py` (`SUPPORTED_CODES_LEGITEXT`), `KM` = `[KM]/server.py` (`CODE_IDS`, qui se dit « source officielle DILA ») ; `JL+KM` = deux sources concordantes (71 identifiants sur 82) ; **FX**/**DILA** = vérifié par fixture / doc officielle.
- Titres : ceux de l'énumération `NomCode` de `[PLF]/pylegifrance/models/code/enum.py:4-76` (70 titres, forme Légifrance) ; `¹` = titre repris des commentaires `[JL]`/`[KM]`, libellé exact NON VÉRIFIÉ.
- Abréviations « attestées » : présentes dans au moins une liste d'abréviations universitaire/éditoriale (résumés WebSearch : https://www.universite-paris-saclay.fr/sites/default/files/2021-09/bu_liste_abreviations_juridiques.pdf, https://bu.univ-cotedazur.fr/liste-des-abreviations-juridiques, https://bibliotheques.u-paris2.fr/userfiles/file/20220410_liste_des_abreviations_et_acronymes_en_droit_0.pdf, https://fr.wiktionary.org/wiki/Annexe:Abr%C3%A9viations_en_droit_fran%C3%A7ais, https://www.labase-lextenso.fr/ouvrages/droit-de-la-famille-9782275159102/principales-abreviations-9782275159102-1, https://www.cabinet-zenou.fr/les-abrevations.php, https://www.avocatparis-cdd.org/PDF/liste_abreviation.pdf ; ces listes dérivent du « code des abréviations » du Syndicat national de l'édition, 1990, dernière mise à jour 2004 — WEB, https://www.clementfrancois.fr/abreviations-references-juridiques/). Les autres formes viennent de code tiers ou de l'usage (NON VÉRIFIÉ) : à accepter en entrée avec `confidence` réduite si **ambiguës**.
- Statut « en vigueur / abrogé » : à lire dans `/list/code` (`etat`, `states`) ; seuls les statuts indiqués par une source figurent ici.

### 5.2 Tableau (77 entrées)

| # | Titre officiel (valeur `NOM_CODE`) | `LEGITEXT` candidat (sources) | Abréviations attestées | Autres formes d'usage | Statut |
|---|---|---|---|---|---|
| 1 | Code civil | `LEGITEXT000006070721` (JL+KM+**FX**) | `C. civ.` (WEB Ref-Lex/BU) | `CC` (TIERS [JL], **ambigu** : Conseil constitutionnel / Cour de cassation), `Code Napoléon` (historique) | en vigueur (FX) |
| 2 | Code de commerce | `LEGITEXT000005634379` (JL+KM) | `C. com.` (WEB) | `C.com` (TIERS [JL]) | ? |
| 3 | Code du travail | `LEGITEXT000006072050` (JL+KM) | `C. trav.` (WEB) | `CT` (TIERS [JL], **ambigu**) | ? |
| 4 | Code pénal | `LEGITEXT000006070719` (JL+KM) | `C. pén.` (WEB Paris-Saclay) | `CP` (TIERS [JL], **ambigu**) | ? |
| 5 | Code de procédure civile | `LEGITEXT000006070716` (JL+KM) | `C. pr. civ.` (WEB Côte d'Azur), `CPC` (WEB, TIERS) | `NCPC` (historique, TIERS [JL]) | ? |
| 6 | Code de procédure pénale | `LEGITEXT000006071154` (JL+KM) | `C. pr. pén.` (WEB Côte d'Azur), `CPP` (WEB Lextenso) | — | ? |
| 7 | Code général des impôts | `LEGITEXT000006069577` (JL+KM) | `CGI` (TIERS [JL]/[JRX]) | — | ? |
| 8 | Code général des impôts, annexe I ¹ | `LEGITEXT000006069568` (JL+KM) | — | `CGI, ann. I` (NON VÉRIFIÉ) | ? |
| 9 | Code général des impôts, annexe II ¹ | `LEGITEXT000006069569` (JL+KM) | — | `CGI, ann. II` (NON VÉRIFIÉ) | ? |
| 10 | Code général des impôts, annexe III ¹ | `LEGITEXT000006069574` (JL+KM) | — | `CGI, ann. III` (NON VÉRIFIÉ) | ? |
| 11 | Code général des impôts, annexe IV | `LEGITEXT000006069576` (JL+KM) | — | `CGI, ann. IV` (NON VÉRIFIÉ) | ? |
| 12 | Livre des procédures fiscales | `LEGITEXT000006069583` (JL+KM) | `LPF` (WEB Paris 2) | — | ? |
| 13 | Code de la santé publique | `LEGITEXT000006072665` (JL+KM) | `CSP` (WEB Wiktionnaire) | `C. santé publ.` (TIERS [JL]/[JRX]) | ? |
| 14 | Code de la sécurité sociale | `LEGITEXT000006073189` (JL+KM) | `C. séc. soc.` / `C. sec. soc.` (WEB Zenou) | `CSS` (TIERS [JL]/[JRX]) | ? |
| 15 | Code de la consommation | `LEGITEXT000006069565` (JL+KM) | `C. consom.` (WEB Paris-Saclay) | `C. conso.` (TIERS [JRX]) | ? |
| 16 | Code de justice administrative | `LEGITEXT000006070933` (JL+KM) | — | `CJA` (TIERS [JL]/[KM]) | ? |
| 17 | Code de l'environnement | `LEGITEXT000006074220` (JL+KM) | `C. env.` / `C. envir.` (WEB) | — | ? |
| 18 | Code général des collectivités territoriales | `LEGITEXT000006070633` (JL+KM) | `CGCT` (WEB Paris 2) | — | ? |
| 19 | Code de l'urbanisme | `LEGITEXT000006074075` (JL+KM) | `C. urb.` (WEB Wiktionnaire) | `CU` (TIERS [JL], **ambigu**) | ? |
| 20 | Code de la construction et de l'habitation | `LEGITEXT000006074096` (JL+KM) | `CCH` (WEB Barreau de Paris) | — | ? |
| 21 | Code de la propriété intellectuelle | `LEGITEXT000006069414` (JL+KM) | `CPI` (WEB, **ambigu** : Cour pénale internationale) | — | ? |
| 22 | Code des assurances | `LEGITEXT000006073984` (JL+KM) | `C. assur.` (WEB Lextenso) | — | ? |
| 23 | Code monétaire et financier | `LEGITEXT000006072026` (JL+KM) | — | `CMF` (TIERS [JL]/[JRX]), `C. mon. fin.` (NON VÉRIFIÉ) | ? |
| 24 | Code rural et de la pêche maritime | **CONFLIT** : `LEGITEXT000006071367` ([JL]) vs `LEGITEXT000022197698` ([KM]) | — | `C. rur.` (TIERS [JRX]), `CRPM` (NON VÉRIFIÉ) | ? |
| 25 | Code rural (ancien) | `LEGITEXT000006071366` (JL+KM) | — | — | ancien (titre ; TIERS [JL] « obsolète ») |
| 26 | Code des relations entre le public et l'administration | `LEGITEXT000031366350` (JL+KM) | `CRPA` (WEB) | — | ? |
| 27 | Code de l'entrée et du séjour des étrangers et du droit d'asile | `LEGITEXT000006070158` (JL+KM) | `CESEDA` (WEB) | — | ? |
| 28 | Code de l'action sociale et des familles | `LEGITEXT000006074069` (JL+KM) | — | `CASF` (TIERS [JL]/[JRX]) | ? |
| 29 | Code de l'organisation judiciaire | `LEGITEXT000006071164` (JL+KM) | — | `COJ` (TIERS [JRX]) | ? |
| 30 | Code des postes et des communications électroniques | `LEGITEXT000006070987` (JL+KM+**DILA**) | — | `CPCE` (TIERS [JL]) | ? (id VÉRIFIÉ [DILA]:169) |
| 31 | Code de la sécurité intérieure | `LEGITEXT000025503132` (JL+KM) | — | `CSI` (TIERS [JRX]) | ? |
| 32 | Code général de la fonction publique | `LEGITEXT000044416551` (JL+KM) | — | `CGFP` (TIERS [JRX]/[KM]) | ? |
| 33 | Code de la justice pénale des mineurs | `LEGITEXT000039086952` (JL+KM) | — | `CJPM` (TIERS [JRX]) | ? |
| 34 | Code général de la propriété des personnes publiques | `LEGITEXT000006070299` (JL+KM) | — | `CG3P` / `CGPPP` (TIERS [JL]/[KM], NON VÉRIFIÉ) | ? |
| 35 | Code des impositions sur les biens et services | `LEGITEXT000044595989` (JL+KM) | — | `CIBS` (TIERS [JL]) | ? |
| 36 | Code de la commande publique | `LEGITEXT000037701019` (JL+KM) | — | `CCP` (TIERS [JL], **ambigu**) | ? |
| 37 | Code de l'éducation | `LEGITEXT000006071191` (JL+KM) | — | `C. éduc.` (TIERS [JL]/[JRX]) | ? |
| 38 | Code des procédures civiles d'exécution | `LEGITEXT000025024948` (JL+KM) | — | `CPCE` (**collision** avec postes), `CPC exéc.` (NON VÉRIFIÉ) | ? |
| 39 | Code de la route | `LEGITEXT000006074228` (JL+KM) | — | `C. route` (NON VÉRIFIÉ) | ? |
| 40 | Code électoral | `LEGITEXT000006070239` (JL+KM) | — | `C. élect.` (NON VÉRIFIÉ), `CE` (**ambigu**) | ? |
| 41 | Code de la défense | `LEGITEXT000006071307` (JL+KM) | — | `C. déf.` (NON VÉRIFIÉ) | ? |
| 42 | Code de l'énergie | `LEGITEXT000023983208` (JL+KM) | — | `C. énergie` (NON VÉRIFIÉ) | ? |
| 43 | Code des transports | `LEGITEXT000023086525` (JL+KM) | — | `C. transp.` (NON VÉRIFIÉ) | ? |
| 44 | Code du patrimoine | `LEGITEXT000006074236` (JL+KM) | — | `C. patr.` (NON VÉRIFIÉ) | ? |
| 45 | Code du sport | `LEGITEXT000006071318` (JL+KM) | — | `C. sport` (NON VÉRIFIÉ) | ? |
| 46 | Code du tourisme ¹ | `LEGITEXT000006074073` (JL+KM) | — | `C. tourisme` (NON VÉRIFIÉ) | ? |
| 47 | Code de la voirie routière | `LEGITEXT000006070667` (JL+KM) | — | — | ? |
| 48 | Code de la recherche | `LEGITEXT000006071190` (JL+KM) | — | — | ? |
| 49 | Code de la mutualité | `LEGITEXT000006074067` (JL+KM) | — | `C. mut.` (NON VÉRIFIÉ) | ? |
| 50 | Code des juridictions financières | `LEGITEXT000006070249` (JL+KM) | — | `CJF` (TIERS [JL]) | ? |
| 51 | Code pénitentiaire | `LEGITEXT000045476241` (JL+KM) | — | — | ? |
| 52 | Code des douanes | `LEGITEXT000006071570` (JL+KM) | — | `C. douanes` (NON VÉRIFIÉ) | ? |
| 53 | Code des douanes de Mayotte | `LEGITEXT000006071645` (JL) | — | — | ? |
| 54 | Code forestier (nouveau) | `LEGITEXT000025244092` (JL+KM) | — | `C. for.` (NON VÉRIFIÉ) | « nouveau » (titre) |
| 55 | Code minier (nouveau) | `LEGITEXT000023501962` (JL+KM) | — | — | « nouveau » (titre) |
| 56 | Code minier (ancien) ¹ | `LEGITEXT000006071785` (JL+KM) | — | — | ancien (TIERS [JL]) |
| 57 | Code de l'expropriation pour cause d'utilité publique | `LEGITEXT000006074224` (JL+KM) | — | — | ? |
| 58 | Code du cinéma et de l'image animée | `LEGITEXT000020908868` (JL+KM) | — | — | ? |
| 59 | Code de justice militaire (nouveau) | `LEGITEXT000006071360` (JL+KM) | — | — | « nouveau » (titre) |
| 60 | Code du service national | `LEGITEXT000006071335` (JL+KM) | — | — | ? |
| 61 | Code de l'aviation civile ¹ | `LEGITEXT000006074234` (JL+KM) | — | — | ? |
| 62 | Code de l'artisanat ¹ | `LEGITEXT000006075116` (JL+KM) | — | — | ? |
| 63 | Code des pensions civiles et militaires de retraite | `LEGITEXT000006070302` (JL+KM) | — | — | ? |
| 64 | Code des pensions militaires d'invalidité et des victimes de guerre | `LEGITEXT000031712069` (KM) | — | — | ? |
| 65 | Code des pensions de retraite des marins français du commerce, de pêche ou de plaisance | `LEGITEXT000006074066` (JL+KM) | — | — | ? |
| 66 | Code de la Légion d'honneur, de la Médaille militaire et de l'ordre national du Mérite | `LEGITEXT000037673300` (KM) | — | — | ? |
| 67 | Code du domaine public fluvial et de la navigation intérieure | `LEGITEXT000006074237` (JL+KM) | — | — | ? |
| 68 | Code du domaine de l'Etat | `LEGITEXT000006070208` (JL+KM) | — | — | ? |
| 69 | Code du domaine de l'Etat et des collectivités publiques applicable à la collectivité territoriale de Mayotte | `LEGITEXT000006074235` (JL) | — | — | ? |
| 70 | Code des ports maritimes | `LEGITEXT000006074233` (JL+KM) | — | — | ? (abrogé : NON VÉRIFIÉ) |
| 71 | Code du travail maritime | `LEGITEXT000006072051` (JL+KM) | — | — | ? (abrogé : NON VÉRIFIÉ) |
| 72 | Code disciplinaire et pénal de la marine marchande | `LEGITEXT000006071188` (JL+KM) | — | — | ? |
| 73 | Code des instruments monétaires et des médailles | `LEGITEXT000006070666` (JL) | — | — | ? |
| 74 | Code de déontologie des architectes | `LEGITEXT000006074232` (JL+KM) | — | — | ? |
| 75 | Code des communes | `LEGITEXT000006070162` (JL+KM) | — | — | remplacé par le CGCT (TIERS [JL]) |
| 76 | Code des communes de la Nouvelle-Calédonie | `LEGITEXT000006070300` (JL+KM) | — | — | ? |
| 77 | Code de la famille et de l'aide sociale | `LEGITEXT000006072637` (JL+KM) | — | `CFAS` (WEB) | remplacé par le CASF (TIERS [JL]) |


Codes **absents** de toutes les listes consultées mais susceptibles d'être cités (historiques, abrogés) : Code de l'industrie cinématographique, Code des marchés publics, Code de la nationalité française, Code du travail applicable à Mayotte, ancien Code de procédure civile (avant le NCPC) — à reconnaître comme `CodeKey::Other(titre)` et à résoudre dynamiquement (NON VÉRIFIÉ qu'ils soient présents dans LEGI).

### 5.3 Collisions et règles d'alias

| Sigle | Sens possibles | Règle proposée |
|---|---|---|
| `CE` | Conseil d'État ; Code électoral ; (rare) Code de l'environnement | jamais un code sans `art_kw` juste avant **et** contexte électoral ; défaut = juridiction |
| `CC` | Code civil ; Conseil constitutionnel ; Cour de cassation (« CIV. 1 CC » dans l'en-tête `[FX]/consult_juri.json:69`) | code seulement dans `art. N CC` ; sinon juridiction ou rien |
| `CP` | Code pénal ; Code des ports maritimes ; conseil de prud'hommes (CPH) | Code pénal en `confidence = Low` (N05) |
| `CT` | Code du travail ; Code du tourisme ; Code des transports | Low ; préférer `C. trav.` |
| `CU` | Code de l'urbanisme | Low |
| `CPI` | Code de la propriété intellectuelle ; Cour pénale internationale | code seulement après `art. N` |
| `CPCE` | postes et communications électroniques ; procédures civiles d'exécution | **refuser** sans titre complet |
| `CCP` | Code de la commande publique ; (Code de procédure pénale mal abrégé ?) | Low |
| `L.` / `D.` | préfixe d'article ; « loi » / « décret » abrégés ; `D.` = Recueil Dalloz | règles 1–2 du §4.2 |

---

## 6. Variantes typographiques à tolérer (récapitulatif)

| Dimension | Variantes à accepter | Forme canonique interne | Exemples / sources |
|---|---|---|---|
| Signe numéro | `n°`, `n° ` (espace normale, U+00A0, U+202F), `n°X` collé, `nº` (U+00BA), `no`, `No`, `N°`, `№`, `num.`, `numéro`, pluriel `nos`, `n°s`, `n os` ; mojibake `nÂ°` | `n°` | `n°0263` (`[FX]/consult_juri_constit.json:42`), `n°SAN-2026-009` (TIERS), `nos 2005064` (`[FX]/consult_juri_cetat.json:68`), `nÂ°` (`[FX]/consult_juri.json:92`) ; règle IN `no` (WEB) |
| Espaces | U+0020, U+00A0, U+202F, U+2009, tab, saut de ligne, runs multiples ; absence d'espace après `L.`/`art.`/`C.` (`C.civ.`, `L.1234-5`) | un ou plusieurs ` ` acceptés partout où `WS` | `M.\u00a0Amine` (FX) ; `L.1234-5` (TIERS [JRX] tests, idée) |
| Apostrophes | `'` U+0027, `’` U+2019, `‘`, `ʼ`, U+0060 (accent grave), `´` | `'` | `code de l’urbanisme` (`[FX]/search_cetat.json`) |
| Tirets | `-`, `‐` U+2010, `\u2011` U+2011, `–` U+2013, `—`, `−` U+2212 | `-` | `SAN–2026-003` (`[FX]/search_cnil.json:53`) |
| Casse | minuscules, Majuscule initiale, MAJUSCULES (`LOI n°`, `ARRÊT DE LA COUR DE CASSATION, PREMIÈRE CHAMBRE CIVILE, DU 4 JUIN 2025`) | comparaison sur vue clé | `[FX]/consult_loda.json` ; `[FX]/consult_juri.json:69` |
| Accents | présents / absents (`Conseil d'Etat`, `decision`, `fevrier`, `Delibération`), formes NFD | NFC + vue clé sans diacritiques | `Conseil d'Etat` et `Conseil d'État` dans la même fixture CE ; `citations.py:283-286,309,378` |
| Abréviations avec/sans point | `C. civ.` / `C civ` / `C.civ.`, `art.` / `art`, `Cass.` / `Cass`, `Civ.` / `civ`, `ch.` / `ch`, `sect.` / `Sect`, `déc.` / `dec.` | point optionnel dans les littéraux d'abréviation (jamais dans les nombres) | WEB (listes BU) ; corpus P041 |
| Ordinaux | `1re`, `1ère`, `1ere`, `1ʳᵉ`, `première`, `Première`, `PREMIÈRE` ; `2e`, `2ème`, `2eme`, `2è`, `deuxième`, `II` (bulletins) ; `1er`, `premier`, `1ᵉʳ` | entier | `2ème - 7ème` (`[FX]/consult_juri_cetat.json:29`) ; règle `1re/2e` (WEB) |
| Dates | `7 octobre 2016`, `07 novembre 2025`, `1er janvier 2018`, `1 janvier` (sortie Python), `7 oct. 2016`, `7 oct 2016`, `2 juill. 2014`, `17 déc. 2010`, `29/05/2026`, `07/10/2016`, `2026-04-30`, `DU 4 JUIN 2025`, `du 24 février 2005`, mois sans accent (`fevrier`, `aout`, `decembre`), calendrier républicain (`an IV`) | `LegalDate {y, m, d}` validée | fixtures ; WEB (OQLF) ; `[LEGIPY]/legi/fr_calendar.py` |
| Numéros d'articles | `L. 1234-5`, `L.1234-5`, `L 1234-5`, `L1234-5`, `L. 1234\u20115` (U+2011), `R*`, `R.*`, `LO.`, `LO`, `1er`, `premier`, `unique`, suffixes `bis…` collés ou non | `num_key` (§4.2) | `[DILA]/exemples-d-utilisation-de-l-api.md:189` ; `[LEGIPY]/legi/articles.py:18-31` |
| Numéros de pourvoi | `21-23.174`, `21-23174`, `2123174` (Judilibre), avec lettre-clé `N 24-13.470` | `AA-NN.NNN` + `Option<char>` | `[FX]/consult_juri.json:17-19,69` ; `[JDL]/src/modules/elastic/query.js:53-54` |
| Encodage | mojibake UTF-8→Latin-1/CP1252 (`Ã©`, `Ã` + U+00A0, `Â°`, `â€™` pour `’`) | réparé avant repli | fixtures LEGI/JURI (§2.2) ; forme `â€™` : NON VÉRIFIÉ dans nos fixtures |
| HTML | `<mark>…</mark>`, `<br/>`, `<sup>re</sup>`, entités (`&apos;` vu dans `[JL]`) | texte brut, offsets conservés | `[FX]/search_loda.json:9` ; `[FX]/search_juri.json` (`<br/>`) |

---

## 7. Stratégie de résolution vers les identifiants Légifrance

### 7.1 Principes communs

1. **Jamais d'identifiant fabriqué** (règle de la référence : `[PY]/src/fr_eli_mcp/server.py:47-48,65`). `fr_resolve` renvoie soit un identifiant **lu dans une réponse de l'API**, soit des **candidats**, soit `NotFound`/`Unsupported` avec la raison.
2. **Statuts de sortie** proposés (communs à `fr_resolve` et `fr_verify_citations`) : `Resolved{ids, matched_fields}`, `Candidates{list, reason}`, `Mismatch{field, cited, actual, actual_id}` (ex. ECLI à un jour près, N14), `NotFound{query}`, `Unverifiable{reason}` (hors périmètre, procédure non publique…), `UpstreamError`. Idée tierce utile : « ne jamais conclure introuvable sans preuve », concurrence bornée, cache par (code, numéro, jour) (`[JRX]/src-tauri/src/juridique/verify.rs:22-25,66-76,1071-1078`, via `[R05]` §1.13 ; AGPL → principe seulement).
3. **Recherche exacte** : `typeRecherche: "EXACTE"` sur le champ spécialisé, `pageSize` petit (5–10), tri `PERTINENCE` (CODE) ou `DATE_DESC` (jurisprudence) ; filtres de date au format accepté (`singleDate` epoch ms pour `DATE_VERSION`, VÉRIFIÉ live par la référence `server.py:135-136`). `fond` LODA/CODE exige `DATE_VERSION` (`[PY]/DISCOVERY.md:22-25` via `[R05]` §2.2).
4. **Facettes à valeurs fermées** : n'envoyer que des valeurs connues (§2.4) — un champ inconnu provoquerait un HTTP 500 (TIERS, NON VÉRIFIÉ, `[R05]` §2.2) ; le domaine fournit les énumérations, l'adapter les sérialise.
5. **Vérifier après résolution** : relire la ressource (`/consult/*`) et comparer les champs cités (date, chambre/formation, numéro, nature, ECLI natif) ; l'égalité ECLI est la preuve la plus forte pour la jurisprudence.

### 7.2 Tableau par famille

| Famille | Résolution primaire (endpoint, payload) | Repli | Vérification | Limites / NON VÉRIFIÉ |
|---|---|---|---|---|
| **Article de code** (`code`, `num_key`, `date?`) | `POST /search` `fond: CODE_DATE`, champ `NUM_ARTICLE` `EXACTE` `valeur: num_key`, filtres `NOM_CODE: [titre exact]` + `DATE_VERSION: singleDate` (= date citée ou aujourd'hui), `typePagination: ARTICLE` → `sections[].extracts[].id` LEGIARTI, puis `POST /consult/getArticle {id}` (recette officielle `[DILA]/exemples-d-utilisation-de-l-api.md:165-247`, exemple `L36-11` → `LEGIARTI000033219357`) | `POST /consult/getArticleWithIdAndNum {id: LEGITEXT, num}` (« seuls les articles sans version en vigueur différée », `[R05]` §1.4) ; `CODE_ETAT` + `TEXT_NOM_CODE` | `article.num` == `num_key`, `textTitles[].titre` == code, `etat` (VIGUEUR/ABROGE/TRANSFERE…), `dateDebut ≤ date < dateFin` ; versions via `/search/canonicalArticleVersion`, renumérotations via `/consult/concordanceLinksArticle` et `/consult/sameNumArticle` | forme `num` pour `R*`/`1er` NON VÉRIFIÉE ; `NOM_CODE` réputé fragile (500 si texte libre, TIERS) → titres issus de `/list/code` uniquement ; code anaphorique/absent → `Candidates` multi-codes |
| **Texte numéroté** (`nature`, `num`, `date?`) | `POST /search` `fond: LODA_DATE`, champ `NUM` `EXACTE` `valeur: "2019-290"` + `DATE_VERSION` (recette officielle `[DILA]/…:413-489` → `LEGITEXT000038359719`, puis `/consult/legiPart` ou `/consult/lawDecree`) ; filtre `NATURE` (§2.4) si la nature est connue | `fond: JORF` champ `NUM` (version initiale) ; `NOR` si cité : `POST /consult/getJoWithNor {nor}` (« tous les textes n'ont pas de NOR », `[DILA]/…:1209`) | `nature`, `dateTexte`/date de signature, titre (similarité sur `title_hint`), `nor` | numérotation partagée lois/décrets/ordonnances dans une même année : NON VÉRIFIÉ → toujours filtrer par nature |
| **Texte à date seule** (« loi du 17 juillet 1978 ») | `LODA_DATE` filtres `NATURE: [LOI]` + `DATE_SIGNATURE: {start: d, end: d}` + champ `TITLE`/`ALL` avec `title_hint` éventuel | `JORF` même filtre | unicité ; sinon `Candidates` | plusieurs textes de même nature le même jour → ambigu par construction |
| **Article d'un texte** (« article 57 de la loi du 17 juillet 1978 ») | même recherche + champ `NUM_ARTICLE` `EXACTE` dans la même requête (recette officielle `[DILA]/…:497-587` → `LEGIARTI000006528277` ; `6 nonies` de l'ordonnance 58-1100 : `[DILA]/…:255-405`) | table des matières `/consult/legi/tableMatieres {textId, date, nature}` puis recherche du `num` | comme article de code | versions différées, articles « unique » |
| **Constitution** | recette officielle : `fond: LODA_ETAT`, champs `TITLE` `EXACTE` "Constitution" **et** `NUM_ARTICLE` `EXACTE` "54", filtres `DATE_VERSION` + `TEXT_LEGAL_STATUS: VIGUEUR` → `LEGIARTI000006527539` (`[DILA]/…:591-693`) | id texte TIERS `JORFTEXT000000571356` (`[JL]/sources/legi.py:187`) | `etat` | le doc officiel utilise `"valeur"` (singulier) pour `TEXT_LEGAL_STATUS` : envoyer `valeurs: ["VIGUEUR"]` (`[R05]` §1.14 point 9) |
| **DDHC, Préambule 1946, Charte** | même schéma `LODA_ETAT` + `TITLE` "Déclaration des Droits de l'Homme et du Citoyen" / "Charte de l'environnement" + `NUM_ARTICLE` | `/suggest` | — | présence et titre exacts dans LODA : **NON VÉRIFIÉ** ; alinéas du Préambule non adressables par `NUM_ARTICLE` (texte unique ?) : NON VÉRIFIÉ |
| **Cour de cassation** (pourvoi, date, chambre) | `POST /search` `fond: JURI`, champ `NUM_AFFAIRE` `EXACTE` `valeur: "21-23.174"` (recette officielle avec `"17/030701"`, `sort: DATE_DESC`, `secondSort: ID`, `[DILA]/…:1121-1160`) ; filtres optionnels `CASSATION_FORMATION: [CHAMBRE_CIVILE_1]`, `DATE_DECISION` | si ECLI cité : filtre `ECLI` (facette JURI, `[R05]` §2.2) ; numéro d'arrêt + chambre + date → recherche + comparaison de l'ECLI dérivé (`C1` + `00399`) | `ecli` natif, `numeroAffaire[]` contient le pourvoi, `formation`, `dateTexte` ; `solution` si citée | forme `NUM_AFFAIRE` sans point (`21-23174`) : NON VÉRIFIÉ → essayer les deux ; mentions sans numéro (« Crim., 28 mars 2018 ») → `Candidates` ; liens `rapprochements` avec JURITEXT disponibles côté Judilibre (`[JDL]` `decisionLink`) |
| **Cours d'appel / TJ** | `fond: JURI`, `NUM_AFFAIRE` `EXACTE` "24/15215" + `JURIDICTION_JUDICIAIRE: ["Juridictions d'appel"]` + `APPEL_SIEGE_APPEL: ["PARIS"]` + `DATE_DECISION` | Judilibre (`/search`, `jurisdiction=ca`) — hors API Légifrance | `numeroAffaire`, siège, date | couverture JURI des CA partielle (sélection) ; TJ/T. com. quasi absents de Légifrance : **non résoluble** en général ; acceptation des valeurs de facette en filtre : NON VÉRIFIÉ |
| **Conseil d'État** | `POST /search` `fond: CETAT`, champ `NUM_DEC` `EXACTE` "506507" + `JURIDICTION_NATURE: [CONSEIL_ETAT]` (+ `DATE_DECISION`) ; ou facette `ECLI` | `/consult/getJuriWithAncienId {ancienId: "JG_L_2026_05_000000506507"}` si l'ancien id est connu (`[R05]` §1.7) | `ecli` natif == ECLI dérivé (`formation` + n° + date) ; `formation`, `publicationRecueil` vs mention Lebon | homonymies de numéros entre CE/CAA/TA impossibles par forme (6 chiffres vs `AA`+code+5 vs 7 chiffres) — forme 4–5 chiffres des requêtes anciennes (`74052`, `99522`) |
| **CAA / TA** | `fond: CETAT`, `NUM_DEC` `EXACTE` "24MA02429" + `JURIDICTION_NATURE: [COURS_APPEL]` + `multiValeurs: {"COURS_APPEL": ["Marseille"]}` ; TA : `TRIBUNAL_ADMINISTATIF` (sic) + `multiValeurs` | — | numéro, date, siège | couverture TA très partielle ; ECLI souvent `null` (`[PY]/DISCOVERY.md:80-82`) ; **jamais** dériver un ECLI CAA/TA |
| **Conseil constitutionnel** | `POST /search` `fond: CONSTIT`, champ `NUM_DEC` `EXACTE` "2025-1173" (+ filtre `NATURE_CONSTIT: ["Dispositions promulguées (QPC)"]` si besoin, libellés §2.4) | facette `NUMERO_DECISION` (`[R05]` §2.2) | `num`, `nature` (`QPC`/`DC`…), `dateTexte`, `ecli` natif (`AAAA.N.TYPE`), `nor` | numéros joints (`2010-14/22`) : forme de `num` côté API NON VÉRIFIÉE ; types rares (`R`, `SEN/QPC`) |
| **ECLI** | aiguillage par code : `CCASS` → `JURI` facette `ECLI` ; `CE*` → `CETAT` facette `ECLI` ; `CC` → `CONSTIT` `NUM_DEC` = `AAAA-N` tiré de l'ordinal + type ; `EU`/autres pays → `Unsupported` | dérivation inverse (CCASS : chambre + n° d'arrêt ; CE : n° + date) pour une recherche de repli | égalité stricte de la chaîne `ecli` renvoyée | CONSTIT n'expose pas de facette ECLI (`[R05]` §2.2) ; Judilibre accepte un ECLI en recherche (`[JDL]/src/modules/elastic/query.js:51-52,224-231`) |
| **CNIL** | `POST /search` `fond: CNIL`, champ `NUM_DELIB` `EXACTE` "SAN-2026-002" (tirets normalisés) + `DATE_DELIB` | facette `NUMERO_DELIB` ; `NOR` ; `/consult/getCnilWithAncienId` | `num`, `dateTexte`, `natureDelib` | procédure simplifiée non publique → `Unverifiable` ; format exact attendu par `NUM_DELIB` (avec/sans série) : NON VÉRIFIÉ |
| **Convention (IDCC)** | `POST /consult/kaliContIdcc {id: "1486"}` (`[R05]` §1.6) | `POST /search` `fond: KALI` champ `IDCC` (`TOUS_LES_MOTS_DANS_UN_CHAMP`, exemple officiel `[DILA]/…:1279-1300`) ; `/list/conventions {idcc}` | `num` du conteneur (`[FX]/consult_kali.json:149`) | **brochure** : aucun champ de recherche identifié (`Article.numeroBrochure[]` existe, `[R05]` §1.4) → non résoluble directement ; avenants : recherche `TITLE` "Avenant n° 18" + `IDCC` |
| **Accord ACCO** | `fond: ACCO` champs `RAISON_SOCIALE`, `IDCC`, `TITLE` | `/suggest/acco` | `raisonSociale`, `siret` | texte intégral seulement en .docx (`[PY]/SOURCES.md:15` via `[R05]`) |
| **JORF** (n°, date, texte n°) | `POST /consult/jorfCont {num, date, …}` → `JORFCONT` → items ; « texte n° 152 » = rang dans le sommaire | `fond: JORF` filtre `DATE_PUBLICATION` | `numParution`, `dateParution`, `idTexteJo` (`[FX]/consult_juri_constit.json` : `lienJo` → `JORFTEXT000052541898`) | sémantique exacte de `num` dans `JorfContConsultRequest` : NON VÉRIFIÉE ; pas de HTML avant juin 2004 |
| **NOR** | `POST /consult/getJoWithNor {nor}` | `LODA_DATE`/`JORF` champ `NOR` | `nor` == valeur (majuscules) | textes sans NOR (avant 1987, CC…) |
| **ELI** | texte : `POST /consult/eliAndAliasRedirectionTexte {idEliOrAlias}` ; article : `POST /consult/getArticleWithIdEliOrAlias` (`[R05]` §1.4–1.5) | décomposer l'ELI (type, date, NOR) → recherche NOR | — | ELI LODA renvoyés `null` par l'API (`[PY]/DISCOVERY.md:34-40`) |
| **Identifiant Légifrance** | consult direct selon préfixe (`citations.py:76-102` pour les URL ; `client.py:187-208` pour les endpoints) ; retirer `_JJ-MM-AAAA` | — | 404 → `NotFound` | `JORFARTI` via `getArticle` : NON VÉRIFIÉ (`[R05]` §1.4) |
| **Appellations** (« loi Macron ») | `POST /suggest {searchText, supplies: [LODA_LIST]}` → `appellations[]` (`[R05]` §1.10) | — | — | jamais par le parseur seul |

### 7.3 Ce qui n'est pas résoluble (réponse `Unsupported` ou `Unverifiable`, motif explicite)

- Actes de l'UE (directives, règlements), CJUE/Tribunal UE, CEDH, autres ECLI non `FR` → EUR-Lex/HUDOC, hors API Légifrance.
- Doctrine et recueils (`D.`, `JCP`, `RTD civ.`, `Gaz. Pal.`, `AJDA`, `Bull. civ.`, `Rec. Lebon p. …`) : pas d'index inverse ; seules les facettes JURI `NUM_BULLETIN`/`ANNEE_BULLETIN` (`[R05]` §2.2) permettent une recherche par bulletin, et pour le Lebon uniquement `PUBLICATION_RECUEIL` (publié / non publié).
- Décisions de TJ, tribunaux de commerce, CPH, la plupart des CA et TA (couverture Légifrance partielle) → `Unverifiable{reason: "hors couverture du fonds"}` + suggestion `fr_coverage` (instructions de la référence `server.py:46`).
- Délibérations CNIL de procédure simplifiée (non publiques), numéros de brochure KALI, accords ACCO autrement que par métadonnées.
- Anaphores sans antécédent (« dudit code » en début de texte), « article 12 » sans texte, sigles ambigus sans contexte.
- Arrêtés préfectoraux/municipaux, délibérations de collectivités (pas au JORF), circulaires (fonds `CIRC` existe : `/consult/circulaire`, hors périmètre actuel, `[R05]` §1.7).
- Textes anciens sans HTML (JO avant juin 2004) : métadonnées seulement.

### 7.4 Vérification anti-hallucination (`fr_verify_citations`) — contrôles par famille

| Famille | Contrôles purement syntaxiques (domaine, hors ligne) | Contrôles en ligne (adapter) |
|---|---|---|
| Article de code | préfixe compatible avec le code (ex. pas de `L` dans un code sans partie législative : NON VÉRIFIÉ, ne pas appliquer sans liste) ; suffixe latin valide | existence au `num_key` ; état à la date ; passage cité ≈ texte de l'article (BM25/similarité, cf. feature « focus ») ; renumérotation (`sameNumArticle`) |
| Texte | année du numéro vs année de la date (N07) ; nature cohérente avec la forme (`LO` → loi organique) | existence ; date ; titre |
| Cour de cassation | chambre existante (pas de `4e civ.`, N08) ; date valide ; pourvoi bien formé (N39) ; année du pourvoi ≤ année de la décision | pourvoi trouvé ; chambre, date, solution, ECLI natif |
| Conseil d'État | formation connue ; `CE` seul interdit en ECLI (N11) ; date de l'ordinal == date citée | ECLI natif, date, formation, mention Lebon |
| CC | type connu ; `year(ecli) ≥ year(num)` | num, type, date |
| ECLI | 5 composants ; longueurs ; jeu de caractères ; préfixes FR connus (N12, N13) | égalité exacte |
| CNIL / IDCC / JORF / NOR | formes | existence, date |

---

## 8. Corpus de tests

### 8.1 Format (JSON Lines, une ligne = un cas)

```text
{ "id": "P001",                       // P = positif, N = négatif / ambigu / suspect
  "src": "FX consult_juri_cetat.json:text.texte",   // provenance : FX | OFF | TP | WEB | SYN (+ chemin)
  "in": "texte d'entrée (UTF-8 ; caractères invisibles écrits \u00a0, \u2011, \u2013, \u2019…)",
  "expect": [                          // liste ordonnée des citations attendues (vide = aucune)
    { "kind": "CodeArticle",           // variante de l'enum Citation (§9.3)
      "match": "sous-chaîne EXACTE de `in`",   // le span attendu = position de `match` dans `in`
      "...": "champs attendus (seuls ceux présents sont comparés)" } ],
  "verdict": "…",                      // (négatifs) aucune_citation | ambigu | suspect | fragment | verification_echoue |
                                       //   verification_a_dater | non_resoluble_directement | hors_perimetre
  "note": "…" }
```

Conventions des champs attendus :

- `code` : titre officiel exact (colonne 2 du §5.2), `null` si inconnu ; `num` : `num_key` canonique (§4.2) ; `range` : `[from, to]` ; `sub` : liste ordonnée de `{"alinea": n}`, `{"phrase": n}`, `{"item": n}` (n°), `{"paragraphe": "I"}` ; `version` : `{"ancien": true}` ou `{"redaction": "applicable_au_litige" | "anterieure_a", "texte": "NATURE num"}` ; `code_via` : `anaphore:meme_code`, `anaphore:dudit`, `contexte_document`.
- `Text` : `nature` ∈ `LOI, LOI_ORGANIQUE, LOI_CONSTIT, ORDONNANCE, DECRET, DECRET_LOI, ARRETE, REGLEMENT, CONSTITUTION, DDHC, PREAMBULE_1946, CHARTE_ENVIRONNEMENT` ; `num` (`"AAAA-N"` tel qu'écrit, tirets normalisés) ; `year`/`seq` normalisés ; `date` ISO ; `title_hint` ; `anaphora`.
- `Decision` : `court` ∈ `CCASS, CA, TJ, CE, CAA, TA, CC` ; `chamber` (`CIV1, CIV2, CIV3, COM, SOC, CRIM, AP, MIXTE` ou libellé brut pour CA/CAA) ; `formation` CE (`ASSEMBLEE, SECTION, CHR, CHS, SSR, SSJS, ORDONNANCE`) ; `chambers` ; `seat` (majuscules sans accents) ; `date` ; `numbers[]` (`type` ∈ `pourvoi, rg, portalis, ce_requete, caa, ta, cc, inconnu`, `value`, `key_letter`, `cc_type`, `joined`) ; `publication` ; `solution` ; `arret_num` ; `decision_kind` ; `formation_code`/`publication_codes`.
- `Ecli` : `country`, `court`, `year`, `ordinal`, `fr` (interprétation : `cass_chamber`, `arret_seq`, `ce_formation`, `ce_requete`, `date`, `cc_num`, `cc_type`), `resolvable`.
- `resolve_hint` : identifiant attendu **en test d'intégration** (sandbox), jamais exigé du parseur ; `requires: "repair_mojibake"` : le cas n'est attendu qu'avec la réparation activée ; `confidence` (`low`/`medium`) ; `flags` ; `ambiguity`.
- `enum_group` : les éléments d'une même énumération partagent le groupe ; le validateur cherche leurs `match` **dans l'ordre** à partir de la fin du précédent.

Validation effectuée : `python3 -I /tmp/claude-0/-home-user-mcp-ligitfrance/3ad1dde4-43a2-5b14-875c-95ae68190d43/scratchpad/scripts07/validate.py corpus_pos.jsonl corpus_neg.jsonl` → `ok 214 bad 0` (tous les `match` sont des sous-chaînes de `in`). Répartition des attentes : CodeArticle 67, Decision 58, Text 26, TextArticle 17, Ecli 17, CnilDeliberation 8, Convention 7, LegifranceId 4, Jorf 3, Unsupported 3, Nor 2, Eli 2.

### 8.2 Transformation en tests Rust table-driven (esquisse)

```rust
// crates/domain/tests/citations_corpus.rs
use serde::Deserialize;

#[derive(Deserialize)]
struct Case { id: String, src: String, #[serde(rename = "in")] input: String,
              expect: Vec<serde_json::Value>, #[serde(default)] verdict: Option<String> }

fn cases(jsonl: &str) -> Vec<Case> {
    jsonl.lines().filter(|l| !l.trim().is_empty())
         .map(|l| serde_json::from_str(l).expect("ligne JSONL invalide")).collect()
}

#[test]
fn corpus_positif() {
    for c in cases(include_str!("data/corpus_pos.jsonl")) {
        let got = fr_eli_domain::citations::parse(&c.input, &ParseOptions::default_for(&c));
        assert_eq!(got.len(), c.expect.len(), "{}: nombre de citations", c.id);
        let mut from = 0;
        for (g, e) in got.iter().zip(&c.expect) {
            let m = e["match"].as_str().unwrap();
            let start = c.input[from..].find(m).map(|i| i + from).expect("match absent");
            assert_eq!((g.span.start, g.span.end), (start, start + m.len()), "{}: span", c.id);
            assert_subset(e, &serde_json::to_value(&g.value).unwrap(), &c.id); // compare les clés présentes
            if e.get("enum_group").is_some() { from = start + m.len(); }
        }
    }
}
```

`serde_json` n'est qu'une dépendance **de test** (`[dev-dependencies]`) ; le domaine n'en dépend pas. Les cas `requires: repair_mojibake` activent l'option ; les champs `resolve_hint` alimentent un test d'intégration séparé de l'adapter Légifrance (sandbox, marqué `#[ignore]` sans identifiants PISTE).

### 8.3 Cas positifs (170) — contenu intégral de `corpus_pos.jsonl`

```jsonl
{"id": "P001", "src": "FX consult_juri_cetat.json:text.texte", "in": "au titre de l'article L. 761-1 du code de justice administrative.", "expect": [{"kind": "CodeArticle", "match": "article L. 761-1 du code de justice administrative", "code": "Code de justice administrative", "num": "L761-1"}]}
{"id": "P002", "src": "FX consult_juri_cetat.json:text.texte", "in": "en méconnaissance des dispositions de l'article R. 741-7 du code de justice administrative, manque en fait.", "expect": [{"kind": "CodeArticle", "match": "article R. 741-7 du code de justice administrative", "code": "Code de justice administrative", "num": "R741-7"}]}
{"id": "P003", "src": "FX consult_juri_cetat.json:text.texte", "in": "4. Aux termes de l'article L. 600-4-1 du code de l'urbanisme : \" Lorsqu'elle annule", "expect": [{"kind": "CodeArticle", "match": "article L. 600-4-1 du code de l'urbanisme", "code": "Code de l'urbanisme", "num": "L600-4-1"}]}
{"id": "P004", "src": "FX consult_juri_cetat.json:text.texte", "in": "les documents d'urbanisme mentionnés aux articles L. 104-1 et L. 104-2 du code de l'urbanisme, au nombre desquels", "expect": [{"kind": "CodeArticle", "match": "L. 104-1", "code": "Code de l'urbanisme", "num": "L104-1", "enum_group": 1}, {"kind": "CodeArticle", "match": "L. 104-2", "code": "Code de l'urbanisme", "num": "L104-2", "enum_group": 1}]}
{"id": "P005", "src": "FX search_cetat.json:results[2].sections.extracts (marks retirées)", "in": "[...]et R. 104-26 du code de l\u2019urbanisme, selon lesquelles", "expect": [{"kind": "CodeArticle", "match": "R. 104-26 du code de l\u2019urbanisme", "code": "Code de l'urbanisme", "num": "R104-26"}]}
{"id": "P006", "src": "FX consult_juri_cetat.json:text.texte (2 phrases concaténées)", "in": "aux termes de l'article L. 104-7 du code de l'urbanisme : \" Les documents \" Aux termes de l'article R. 104-26 du même code : \" Lorsqu'un document", "expect": [{"kind": "CodeArticle", "match": "article L. 104-7 du code de l'urbanisme", "code": "Code de l'urbanisme", "num": "L104-7"}, {"kind": "CodeArticle", "match": "article R. 104-26 du même code", "code": "Code de l'urbanisme", "num": "R104-26", "code_via": "anaphore:meme_code"}]}
{"id": "P007", "src": "FX consult_juri_constit.json:text.texte", "in": "du premier alinéa de l\u2019article 396 du code de procédure pénale et de l\u2019article 397-1-1 du même code.", "expect": [{"kind": "CodeArticle", "match": "premier alinéa de l\u2019article 396 du code de procédure pénale", "code": "Code de procédure pénale", "num": "396", "sub": [{"alinea": 1}]}, {"kind": "CodeArticle", "match": "article 397-1-1 du même code", "code": "Code de procédure pénale", "num": "397-1-1", "code_via": "anaphore:meme_code"}]}
{"id": "P008", "src": "FX consult_juri_constit.json:text.texte", "in": "le renvoi opéré par la première phrase du troisième alinéa de l\u2019article 397-1-1 du code de procédure pénale aux mots", "expect": [{"kind": "CodeArticle", "match": "première phrase du troisième alinéa de l\u2019article 397-1-1 du code de procédure pénale", "code": "Code de procédure pénale", "num": "397-1-1", "sub": [{"alinea": 3}, {"phrase": 1}]}]}
{"id": "P009", "src": "FX consult_juri_constit.json:text.texte", "in": "les dispositions du deuxième alinéa de l\u2019article 141-2 et de l\u2019article 141-4 sont applicables", "expect": [{"kind": "CodeArticle", "match": "deuxième alinéa de l\u2019article 141-2", "code": null, "num": "141-2", "sub": [{"alinea": 2}], "code_via": "contexte_document"}, {"kind": "CodeArticle", "match": "article 141-4", "code": null, "num": "141-4", "code_via": "contexte_document"}]}
{"id": "P010", "src": "FX search_juri.json:results[0].text (mojibake réparé)", "in": "qu'aux termes de l'article 2270-1 ancien du code civil, les actions", "expect": [{"kind": "CodeArticle", "match": "article 2270-1 ancien du code civil", "code": "Code civil", "num": "2270-1", "version": {"ancien": true}}]}
{"id": "P011", "src": "FX search_juri.json:results[1].text", "in": "La limitation de responsabilité du manutentionnaire prévue à l'article L. 5422-23 du code des transports ne s'applique", "expect": [{"kind": "CodeArticle", "match": "article L. 5422-23 du code des transports", "code": "Code des transports", "num": "L5422-23"}]}
{"id": "P012", "src": "FX search_juri.json:results[1].text", "in": "Il résulte des articles L. 5422-13 et L. 5422-23 du code des transports", "expect": [{"kind": "CodeArticle", "match": "L. 5422-13", "code": "Code des transports", "num": "L5422-13", "enum_group": 1}, {"kind": "CodeArticle", "match": "L. 5422-23", "code": "Code des transports", "num": "L5422-23", "enum_group": 1}]}
{"id": "P013", "src": "FX consult_juri_cetat.json:text.texte", "in": "aux termes de l'article L. 123-6 du code de l'urbanisme dans sa rédaction applicable au litige :", "expect": [{"kind": "CodeArticle", "match": "article L. 123-6 du code de l'urbanisme dans sa rédaction applicable au litige", "code": "Code de l'urbanisme", "num": "L123-6", "version": {"redaction": "applicable_au_litige"}}]}
{"id": "P014", "src": "OFF exemples-d-utilisation-de-l-api.md:169,247 (forme reconstruite)", "in": "l'article L. 36-11 du code des postes et des communications électroniques", "expect": [{"kind": "CodeArticle", "match": "article L. 36-11 du code des postes et des communications électroniques", "code": "Code des postes et des communications électroniques", "num": "L36-11", "resolve_hint": {"date": "2018-01-01", "id": "LEGIARTI000033219357"}}]}
{"id": "P015", "src": "FX test_parse.py:69 (sortie de la réf. Python)", "in": "Code civil, art. 9", "expect": [{"kind": "CodeArticle", "match": "Code civil, art. 9", "code": "Code civil", "num": "9", "resolve_hint": {"id": "LEGIARTI000006419288"}}]}
{"id": "P016", "src": "SYN", "in": "C. civ., art. 1240", "expect": [{"kind": "CodeArticle", "match": "C. civ., art. 1240", "code": "Code civil", "num": "1240"}]}
{"id": "P017", "src": "SYN", "in": "(art. 1240 C. civ.)", "expect": [{"kind": "CodeArticle", "match": "art. 1240 C. civ.", "code": "Code civil", "num": "1240"}]}
{"id": "P018", "src": "SYN", "in": "Selon l'art. 1240 du Code civil, tout fait", "expect": [{"kind": "CodeArticle", "match": "art. 1240 du Code civil", "code": "Code civil", "num": "1240"}]}
{"id": "P019", "src": "SYN", "in": "art. L.1234-5 C. trav.", "expect": [{"kind": "CodeArticle", "match": "art. L.1234-5 C. trav.", "code": "Code du travail", "num": "L1234-5"}]}
{"id": "P020", "src": "SYN", "in": "en application de L. 1234-5 du code du travail", "expect": [{"kind": "CodeArticle", "match": "L. 1234-5 du code du travail", "code": "Code du travail", "num": "L1234-5"}]}
{"id": "P021", "src": "SYN (forme d'affichage Légifrance)", "in": "Article L1234-5 du Code du travail", "expect": [{"kind": "CodeArticle", "match": "Article L1234-5 du Code du travail", "code": "Code du travail", "num": "L1234-5"}]}
{"id": "P022", "src": "SYN", "in": "l'article R. 4127-1 du code de la santé publique", "expect": [{"kind": "CodeArticle", "match": "article R. 4127-1 du code de la santé publique", "code": "Code de la santé publique", "num": "R4127-1"}]}
{"id": "P023", "src": "SYN (préfixe R* : lexique-api-lgf.md:341-355)", "in": "article R* 111-1 du code de l'urbanisme", "expect": [{"kind": "CodeArticle", "match": "article R* 111-1 du code de l'urbanisme", "code": "Code de l'urbanisme", "num": "R*111-1", "prefix": "R*"}]}
{"id": "P024", "src": "SYN (préfixe D)", "in": "art. D. 1234-1 C. trav.", "expect": [{"kind": "CodeArticle", "match": "art. D. 1234-1 C. trav.", "code": "Code du travail", "num": "D1234-1", "prefix": "D"}]}
{"id": "P025", "src": "SYN (préfixe A)", "in": "article A. 422-1 du code des assurances", "expect": [{"kind": "CodeArticle", "match": "article A. 422-1 du code des assurances", "code": "Code des assurances", "num": "A422-1", "prefix": "A"}]}
{"id": "P026", "src": "SYN (préfixe LO)", "in": "l'article LO. 141 du code électoral", "expect": [{"kind": "CodeArticle", "match": "article LO. 141 du code électoral", "code": "Code électoral", "num": "LO141", "prefix": "LO"}]}
{"id": "P027", "src": "OFF lexique-api-lgf.md:129", "in": "c'est le cas, par exemple, de l'article 39 octies du code général des impôts.", "expect": [{"kind": "CodeArticle", "match": "article 39 octies du code général des impôts", "code": "Code général des impôts", "num": "39 octies"}]}
{"id": "P028", "src": "SYN (suffixe latin + lettre capitale)", "in": "art. 199 undecies B du CGI", "expect": [{"kind": "CodeArticle", "match": "art. 199 undecies B du CGI", "code": "Code général des impôts", "num": "199 undecies B"}]}
{"id": "P029", "src": "SYN", "in": "article 238 bis CGI", "expect": [{"kind": "CodeArticle", "match": "article 238 bis CGI", "code": "Code général des impôts", "num": "238 bis"}]}
{"id": "P030", "src": "OFF lexique-api-lgf.md:133", "in": "l'article L. 821-5-2 du code de commerce est devenu l'article L. 821-5-3 du même code", "expect": [{"kind": "CodeArticle", "match": "article L. 821-5-2 du code de commerce", "code": "Code de commerce", "num": "L821-5-2"}, {"kind": "CodeArticle", "match": "article L. 821-5-3 du même code", "code": "Code de commerce", "num": "L821-5-3", "code_via": "anaphore:meme_code"}]}
{"id": "P031", "src": "TP judilibre-search src/data/sample_detail_resolved.json:visa[0]", "in": "Article L. 721-3, 2°, du code de commerce.", "expect": [{"kind": "CodeArticle", "match": "Article L. 721-3, 2°, du code de commerce", "code": "Code de commerce", "num": "L721-3", "sub": [{"item": 2}]}]}
{"id": "P032", "src": "TP judilibre-search src/data/sample_detail_resolved.json:visa[1]", "in": "Article L. 221-4 du code de l'organisation judiciaire.", "expect": [{"kind": "CodeArticle", "match": "Article L. 221-4 du code de l'organisation judiciaire", "code": "Code de l'organisation judiciaire", "num": "L221-4"}]}
{"id": "P033", "src": "TP judilibre-search src/data/sample_detail_resolved.json:titlesAndSummaries[0].summary", "in": "au sens des articles 56 et 57 du code de procédure pénale, l'inventaire sommaire réalisé par un agent de police judiciaire, en application de l'article 20 dudit code,", "expect": [{"kind": "CodeArticle", "match": "56", "code": "Code de procédure pénale", "num": "56", "enum_group": 1}, {"kind": "CodeArticle", "match": "57", "code": "Code de procédure pénale", "num": "57", "enum_group": 1}, {"kind": "CodeArticle", "match": "article 20 dudit code", "code": "Code de procédure pénale", "num": "20", "code_via": "anaphore:dudit"}]}
{"id": "P034", "src": "TP justicelibre web/hub.html (extrait Cass. soc. 25 juin 2003)", "in": "Vu les articles L. 122-4, L. 122-14 et L. 122-14-3 du Code du travail ;", "expect": [{"kind": "CodeArticle", "match": "L. 122-4", "code": "Code du travail", "num": "L122-4", "enum_group": 1}, {"kind": "CodeArticle", "match": "L. 122-14", "code": "Code du travail", "num": "L122-14", "enum_group": 1}, {"kind": "CodeArticle", "match": "L. 122-14-3", "code": "Code du travail", "num": "L122-14-3", "enum_group": 1}]}
{"id": "P035", "src": "SYN (plage)", "in": "articles 1100 à 1100-2 du code civil", "expect": [{"kind": "CodeArticle", "match": "articles 1100 à 1100-2 du code civil", "code": "Code civil", "range": ["1100", "1100-2"]}]}
{"id": "P036", "src": "SYN", "in": "l'article 1er du code civil", "expect": [{"kind": "CodeArticle", "match": "article 1er du code civil", "code": "Code civil", "num": "1er", "num_keys": ["1er", "1"]}]}
{"id": "P037", "src": "SYN", "in": "au titre de l'article 700 du CPC", "expect": [{"kind": "CodeArticle", "match": "article 700 du CPC", "code": "Code de procédure civile", "num": "700"}]}
{"id": "P038", "src": "SYN", "in": "article 6, al. 2, du code civil", "expect": [{"kind": "CodeArticle", "match": "article 6, al. 2, du code civil", "code": "Code civil", "num": "6", "sub": [{"alinea": 2}]}]}
{"id": "P039", "src": "SYN", "in": "C. pr. pén., art. 63-4", "expect": [{"kind": "CodeArticle", "match": "C. pr. pén., art. 63-4", "code": "Code de procédure pénale", "num": "63-4"}]}
{"id": "P040", "src": "SYN (paragraphe romain + item)", "in": "le 3° du I de l'article L. 1111-2 du CGCT", "expect": [{"kind": "CodeArticle", "match": "3° du I de l'article L. 1111-2 du CGCT", "code": "Code général des collectivités territoriales", "num": "L1111-2", "sub": [{"paragraphe": "I"}, {"item": 3}]}]}
{"id": "P041", "src": "SYN (sans espace)", "in": "art. 1240 C.civ.", "expect": [{"kind": "CodeArticle", "match": "art. 1240 C.civ.", "code": "Code civil", "num": "1240"}]}
{"id": "P042", "src": "SYN (U+00A0 après « L. », U+2011 dans le numéro)", "in": "article L.\u00a01234\u20115 du code du travail", "expect": [{"kind": "CodeArticle", "match": "article L.\u00a01234\u20115 du code du travail", "code": "Code du travail", "num": "L1234-5"}]}
{"id": "P043", "src": "SYN", "in": "l'article L. 2212-2 du code général des collectivités territoriales", "expect": [{"kind": "CodeArticle", "match": "article L. 2212-2 du code général des collectivités territoriales", "code": "Code général des collectivités territoriales", "num": "L2212-2"}]}
{"id": "P044", "src": "SYN", "in": "C. consom., art. L. 121-1", "expect": [{"kind": "CodeArticle", "match": "C. consom., art. L. 121-1", "code": "Code de la consommation", "num": "L121-1"}]}
{"id": "P045", "src": "TP justicelibre web/hub.html (sommaire Cass. 1re civ. 27 nov. 2019)", "in": "Il résulte des dispositions des articles 1244 et 1253 du code civil, dans leur rédaction antérieure à celle issue de l'ordonnance n° 2016-131 du 10 février 2016, que", "expect": [{"kind": "CodeArticle", "match": "1244", "code": "Code civil", "num": "1244", "enum_group": 1, "version": {"redaction": "anterieure_a", "texte": "ORDONNANCE 2016-131"}}, {"kind": "CodeArticle", "match": "1253", "code": "Code civil", "num": "1253", "enum_group": 1, "version": {"redaction": "anterieure_a", "texte": "ORDONNANCE 2016-131"}}, {"kind": "Text", "match": "ordonnance n° 2016-131 du 10 février 2016", "nature": "ORDONNANCE", "num": "2016-131", "year": 2016, "seq": 131, "date": "2016-02-10"}]}
{"id": "P046", "src": "SYN (majuscules, sans accents)", "in": "ARTICLE L. 1234-5 DU CODE DU TRAVAIL", "expect": [{"kind": "CodeArticle", "match": "ARTICLE L. 1234-5 DU CODE DU TRAVAIL", "code": "Code du travail", "num": "L1234-5"}]}
{"id": "P047", "src": "SYN (CSS)", "in": "article L. 242-1 du code de la sécurité sociale", "expect": [{"kind": "CodeArticle", "match": "article L. 242-1 du code de la sécurité sociale", "code": "Code de la sécurité sociale", "num": "L242-1"}]}
{"id": "P048", "src": "SYN (CMF)", "in": "art. L. 511-1 CMF", "expect": [{"kind": "CodeArticle", "match": "art. L. 511-1 CMF", "code": "Code monétaire et financier", "num": "L511-1"}]}
{"id": "P050", "src": "FX consult_loda.json:title (mojibake réparé)", "in": "LOI n° 2016-1321 du 7 octobre 2016 pour une République numérique (1)", "expect": [{"kind": "Text", "match": "LOI n° 2016-1321 du 7 octobre 2016", "nature": "LOI", "num": "2016-1321", "year": 2016, "seq": 1321, "date": "2016-10-07", "title_hint": "pour une République numérique", "resolve_hint": {"text_id": "LEGITEXT000033205014", "cid": "JORFTEXT000033202746", "nor": "ECFI1524250L"}}]}
{"id": "P051", "src": "FX search_loda.json:results[1].titles[0].title", "in": "Décret n° 2016-1849 du 23 décembre 2016 relatif à la carte mobilité inclusion", "expect": [{"kind": "Text", "match": "Décret n° 2016-1849 du 23 décembre 2016", "nature": "DECRET", "num": "2016-1849", "date": "2016-12-23", "resolve_hint": {"text_id": "LEGITEXT000033696785", "nor": "AFSA1629022D"}}]}
{"id": "P052", "src": "FX search_loda.json:results[1].titles[0].title", "in": "pris en application de l'article 107 de la loi n° 2016-1321 du 7 octobre 2016 pour une République numérique", "expect": [{"kind": "TextArticle", "match": "article 107 de la loi n° 2016-1321 du 7 octobre 2016", "num": "107", "text": {"nature": "LOI", "num": "2016-1321", "date": "2016-10-07"}}]}
{"id": "P053", "src": "FX search_loda.json:results[1].titles[0].title", "in": "et en application de l'article 2 de la loi n° 93-1419 du 31 décembre 1993 relative à l'Imprimerie nationale", "expect": [{"kind": "TextArticle", "match": "article 2 de la loi n° 93-1419 du 31 décembre 1993", "num": "2", "text": {"nature": "LOI", "num": "93-1419", "year": 1993, "seq": 1419, "date": "1993-12-31"}}]}
{"id": "P054", "src": "FX consult_juri.json:text.texte", "in": "la loi n° 98-389 du 19 mai 1998 transposant cette directive", "expect": [{"kind": "Text", "match": "loi n° 98-389 du 19 mai 1998", "nature": "LOI", "num": "98-389", "year": 1998, "seq": 389, "date": "1998-05-19"}]}
{"id": "P055", "src": "FX consult_juri_constit.json:text.texte", "in": "- l\u2019ordonnance n° 58-1067 du 7 novembre 1958 portant loi organique sur le Conseil constitutionnel ;", "expect": [{"kind": "Text", "match": "ordonnance n° 58-1067 du 7 novembre 1958", "nature": "ORDONNANCE", "num": "58-1067", "year": 1958, "seq": 1067, "date": "1958-11-07", "title_hint": "portant loi organique sur le Conseil constitutionnel"}]}
{"id": "P056", "src": "FX consult_juri_constit.json:text.texte", "in": "- la loi n° 2019-222 du 23 mars 2019 de programmation 2018-2022 et de réforme pour la justice ;", "expect": [{"kind": "Text", "match": "loi n° 2019-222 du 23 mars 2019", "nature": "LOI", "num": "2019-222", "date": "2019-03-23", "title_hint": "de programmation 2018-2022 et de réforme pour la justice"}]}
{"id": "P057", "src": "FX consult_juri_constit.json:text.texte", "in": "- la loi n° 2023-1059 du 20 novembre 2023 d\u2019orientation et de programmation du ministère de la justice 2023-2027 ;", "expect": [{"kind": "Text", "match": "loi n° 2023-1059 du 20 novembre 2023", "nature": "LOI", "num": "2023-1059", "date": "2023-11-20"}]}
{"id": "P058", "src": "FX consult_juri_constit.json:text.texte", "in": "dans sa rédaction résultant de la loi du 20 novembre 2023 mentionnée ci-dessus", "expect": [{"kind": "Text", "match": "loi du 20 novembre 2023 mentionnée ci-dessus", "nature": "LOI", "num": null, "date": "2023-11-20", "anaphora": "mentionnee_ci_dessus"}]}
{"id": "P059", "src": "FX consult_juri_constit.json:text.texte", "in": "dans les conditions prévues à l\u2019article 23-11 de l\u2019ordonnance du 7 novembre 1958 susvisée.", "expect": [{"kind": "TextArticle", "match": "article 23-11 de l\u2019ordonnance du 7 novembre 1958 susvisée", "num": "23-11", "text": {"nature": "ORDONNANCE", "num": null, "date": "1958-11-07", "anaphora": "susvisee"}}]}
{"id": "P060", "src": "FX consult_juri_constit.json:text.texte", "in": "- le règlement du 4 février 2010 sur la procédure suivie devant le Conseil constitutionnel pour les questions prioritaires de constitutionnalité ;", "expect": [{"kind": "Text", "match": "règlement du 4 février 2010", "nature": "REGLEMENT", "num": null, "date": "2010-02-04", "title_hint": "sur la procédure suivie devant le Conseil constitutionnel pour les questions prioritaires de constitutionnalité"}]}
{"id": "P061", "src": "OFF exemples-d-utilisation-de-l-api.md:255", "in": "l\u2019article 6 nonies au 1er janvier 2018 de l\u2019ordonnance n°58-1100.", "expect": [{"kind": "TextArticle", "match": "article 6 nonies au 1er janvier 2018 de l\u2019ordonnance n°58-1100", "num": "6 nonies", "at_date": "2018-01-01", "text": {"nature": "ORDONNANCE", "num": "58-1100", "year": 1958}}]}
{"id": "P062", "src": "OFF exemples-d-utilisation-de-l-api.md:257", "in": "l\u2019article 3-1 au 1er janvier 2018 de la loi n° 86-1067", "expect": [{"kind": "TextArticle", "match": "article 3-1 au 1er janvier 2018 de la loi n° 86-1067", "num": "3-1", "at_date": "2018-01-01", "text": {"nature": "LOI", "num": "86-1067", "year": 1986}}]}
{"id": "P063", "src": "OFF exemples-d-utilisation-de-l-api.md:413,489", "in": "la loi n°2019-290 en vigueur à la date d'aujourd'hui", "expect": [{"kind": "Text", "match": "loi n°2019-290", "nature": "LOI", "num": "2019-290", "resolve_hint": {"text_id": "LEGITEXT000038359719"}}]}
{"id": "P064", "src": "OFF exemples-d-utilisation-de-l-api.md:497,587", "in": "article 57 de la loi du 17 juillet 1978 en vigueur aujourd'hui", "expect": [{"kind": "TextArticle", "match": "article 57 de la loi du 17 juillet 1978", "num": "57", "text": {"nature": "LOI", "num": null, "date": "1978-07-17"}, "resolve_hint": {"id": "LEGIARTI000006528277"}}]}
{"id": "P065", "src": "SYN (loi organique)", "in": "loi organique n° 2009-1523 du 10 décembre 2009", "expect": [{"kind": "Text", "match": "loi organique n° 2009-1523 du 10 décembre 2009", "nature": "LOI_ORGANIQUE", "num": "2009-1523", "date": "2009-12-10"}]}
{"id": "P066", "src": "SYN (loi constitutionnelle)", "in": "la loi constitutionnelle n° 2008-724 du 23 juillet 2008", "expect": [{"kind": "Text", "match": "loi constitutionnelle n° 2008-724 du 23 juillet 2008", "nature": "LOI_CONSTIT", "num": "2008-724", "date": "2008-07-23"}]}
{"id": "P067", "src": "SYN", "in": "le décret n° 2019-1333 du 11 décembre 2019", "expect": [{"kind": "Text", "match": "décret n° 2019-1333 du 11 décembre 2019", "nature": "DECRET", "num": "2019-1333", "date": "2019-12-11"}]}
{"id": "P068", "src": "SYN (abréviations D. et mois)", "in": "D. n° 2019-1333 du 11 déc. 2019", "expect": [{"kind": "Text", "match": "D. n° 2019-1333 du 11 déc. 2019", "nature": "DECRET", "num": "2019-1333", "date": "2019-12-11"}]}
{"id": "P069", "src": "SYN (abréviation L.)", "in": "L. n° 2016-1321 du 7 oct. 2016", "expect": [{"kind": "Text", "match": "L. n° 2016-1321 du 7 oct. 2016", "nature": "LOI", "num": "2016-1321", "date": "2016-10-07"}]}
{"id": "P070", "src": "SYN", "in": "Ord. n° 2016-131 du 10 févr. 2016", "expect": [{"kind": "Text", "match": "Ord. n° 2016-131 du 10 févr. 2016", "nature": "ORDONNANCE", "num": "2016-131", "date": "2016-02-10"}]}
{"id": "P071", "src": "SYN (arrêté non numéroté)", "in": "l'arrêté du 12 février 2024 relatif aux modalités", "expect": [{"kind": "Text", "match": "arrêté du 12 février 2024", "nature": "ARRETE", "num": null, "date": "2024-02-12"}]}
{"id": "P072", "src": "SYN", "in": "le décret-loi du 30 octobre 1935", "expect": [{"kind": "Text", "match": "décret-loi du 30 octobre 1935", "nature": "DECRET_LOI", "num": null, "date": "1935-10-30"}]}
{"id": "P073", "src": "SYN (texte ancien non numéroté)", "in": "la loi du 29 juillet 1881 sur la liberté de la presse", "expect": [{"kind": "Text", "match": "loi du 29 juillet 1881", "nature": "LOI", "num": null, "date": "1881-07-29", "title_hint": "sur la liberté de la presse"}]}
{"id": "P074", "src": "SYN", "in": "loi n° 78-17 du 6 janvier 1978 relative à l'informatique, aux fichiers et aux libertés", "expect": [{"kind": "Text", "match": "loi n° 78-17 du 6 janvier 1978", "nature": "LOI", "num": "78-17", "year": 1978, "seq": 17, "date": "1978-01-06"}]}
{"id": "P075", "src": "SYN (« no »)", "in": "loi no 2016-1321 du 7 octobre 2016", "expect": [{"kind": "Text", "match": "loi no 2016-1321 du 7 octobre 2016", "nature": "LOI", "num": "2016-1321", "date": "2016-10-07"}]}
{"id": "P076", "src": "SYN (U+2011, date numérique)", "in": "loi n°2016\u20111321 du 07/10/2016", "expect": [{"kind": "Text", "match": "loi n°2016\u20111321 du 07/10/2016", "nature": "LOI", "num": "2016-1321", "date": "2016-10-07"}]}
{"id": "P077", "src": "FX search_loda.json:results[0].titles[0].title (octets bruts, mojibake)", "in": "LOI n\u00c2° 2016-1321 du 7 octobre 2016 pour une <mark>R\u00c3\u00a9publique</mark>", "expect": [{"kind": "Text", "match": "LOI n\u00c2° 2016-1321 du 7 octobre 2016", "nature": "LOI", "num": "2016-1321", "date": "2016-10-07", "requires": "repair_mojibake"}]}
{"id": "P078", "src": "SYN (« n\u00ba » U+00BA, U+202F)", "in": "décret n\u00ba\u202f2016-1849 du 23\u00a0décembre 2016", "expect": [{"kind": "Text", "match": "décret n\u00ba\u202f2016-1849 du 23\u00a0décembre 2016", "nature": "DECRET", "num": "2016-1849", "date": "2016-12-23"}]}
{"id": "P080", "src": "FX consult_juri_constit.json:text.texte", "in": "dans les conditions prévues à l\u2019article 61-1 de la Constitution, d\u2019une question prioritaire", "expect": [{"kind": "TextArticle", "match": "article 61-1 de la Constitution", "num": "61-1", "text": {"nature": "CONSTITUTION", "date": "1958-10-04"}}]}
{"id": "P081", "src": "OFF exemples-d-utilisation-de-l-api.md:595,693", "in": "article 54 de la Constitution", "expect": [{"kind": "TextArticle", "match": "article 54 de la Constitution", "num": "54", "text": {"nature": "CONSTITUTION"}, "resolve_hint": {"id": "LEGIARTI000006527539"}}]}
{"id": "P082", "src": "FX consult_juri_constit.json:text.texte", "in": "il résulte de la combinaison des articles 6, 8, 9 et 16 de la Déclaration des droits de l\u2019homme et du citoyen de 1789 que", "expect": [{"kind": "TextArticle", "match": "6", "num": "6", "text": {"nature": "DDHC"}, "enum_group": 1}, {"kind": "TextArticle", "match": "8", "num": "8", "text": {"nature": "DDHC"}, "enum_group": 1}, {"kind": "TextArticle", "match": "9", "num": "9", "text": {"nature": "DDHC"}, "enum_group": 1}, {"kind": "TextArticle", "match": "16", "num": "16", "text": {"nature": "DDHC"}, "enum_group": 1}]}
{"id": "P083", "src": "FX consult_juri_constit.json:text.texte", "in": "En second lieu, selon l\u2019article 6 de la Déclaration de 1789, la loi est", "expect": [{"kind": "TextArticle", "match": "article 6 de la Déclaration de 1789", "num": "6", "text": {"nature": "DDHC"}}]}
{"id": "P084", "src": "SYN", "in": "art. 16 DDHC", "expect": [{"kind": "TextArticle", "match": "art. 16 DDHC", "num": "16", "text": {"nature": "DDHC"}}]}
{"id": "P085", "src": "SYN", "in": "le onzième alinéa du Préambule de la Constitution du 27 octobre 1946", "expect": [{"kind": "TextArticle", "match": "onzième alinéa du Préambule de la Constitution du 27 octobre 1946", "num": null, "sub": [{"alinea": 11}], "text": {"nature": "PREAMBULE_1946", "date": "1946-10-27"}}]}
{"id": "P086", "src": "SYN", "in": "l'article 5 de la Charte de l'environnement", "expect": [{"kind": "TextArticle", "match": "article 5 de la Charte de l'environnement", "num": "5", "text": {"nature": "CHARTE_ENVIRONNEMENT"}}]}
{"id": "P090", "src": "FX consult_juri.json:text.citationJp (mojibake réparé)", "in": "à rapprocher : 1re Civ., 25 mai 2023, pourvoi n° 21-23.174 (cassation partielle) et l'arrêt cité.", "expect": [{"kind": "Decision", "match": "1re Civ., 25 mai 2023, pourvoi n° 21-23.174 (cassation partielle)", "court": "CCASS", "chamber": "CIV1", "date": "2023-05-25", "numbers": [{"type": "pourvoi", "value": "21-23.174"}], "solution": "cassation partielle"}]}
{"id": "P091", "src": "FX consult_juri.json:text.titre (mojibake réparé)", "in": "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin", "expect": [{"kind": "Decision", "match": "Cour de cassation, civile, Chambre civile 1, 4 juin 2025, 24-13.470, Publié au bulletin", "court": "CCASS", "chamber": "CIV1", "date": "2025-06-04", "numbers": [{"type": "pourvoi", "value": "24-13.470"}], "publication": {"bulletin": true}, "resolve_hint": {"id": "JURITEXT000051743650", "ecli": "ECLI:FR:CCASS:2025:C100399"}}]}
{"id": "P092", "src": "FX search_juri.json:results[1].titles[0].title", "in": "Cour de cassation, civile, Chambre commerciale, 22 novembre 2023, 22-17.843, Publié au bulletin", "expect": [{"kind": "Decision", "match": "Cour de cassation, civile, Chambre commerciale, 22 novembre 2023, 22-17.843, Publié au bulletin", "court": "CCASS", "chamber": "COM", "date": "2023-11-22", "numbers": [{"type": "pourvoi", "value": "22-17.843"}], "publication": {"bulletin": true}, "resolve_hint": {"id": "JURITEXT000048465611"}}]}
{"id": "P093", "src": "TP judilibre-search src/data/sample_detail_resolved.json:rapprochements[0].title", "in": "Crim., 28 mars 2018, pourvoi n° 17-82.138, Bull. crim. 2018, n° 59 (cassation).", "expect": [{"kind": "Decision", "match": "Crim., 28 mars 2018, pourvoi n° 17-82.138, Bull. crim. 2018, n° 59 (cassation)", "court": "CCASS", "chamber": "CRIM", "date": "2018-03-28", "numbers": [{"type": "pourvoi", "value": "17-82.138"}], "publication": {"bulletin": "Bull. crim. 2018, n° 59"}, "solution": "cassation", "resolve_hint": {"id": "JURITEXT000036780011"}}]}
{"id": "P094", "src": "TP justicelibre (texte Cass., citationJp)", "in": "le droit à une nationalité (1re Civ., 25 avril 2007, pourvoi n° 04-17.632, Bull. 2007, I, n° 159 (rejet)).", "expect": [{"kind": "Decision", "match": "1re Civ., 25 avril 2007, pourvoi n° 04-17.632, Bull. 2007, I, n° 159 (rejet)", "court": "CCASS", "chamber": "CIV1", "date": "2007-04-25", "numbers": [{"type": "pourvoi", "value": "04-17.632"}], "publication": {"bulletin": "Bull. 2007, I, n° 159"}, "solution": "rejet"}]}
{"id": "P095", "src": "WEB bu.univ-lorraine.fr Guide biblio Droit 2025-2026 (résumé WebSearch)", "in": "Cass. Civ. 1ère, 17 déc. 2010, n°10-10.906, Bull. civ., I, n°270", "expect": [{"kind": "Decision", "match": "Cass. Civ. 1ère, 17 déc. 2010, n°10-10.906, Bull. civ., I, n°270", "court": "CCASS", "chamber": "CIV1", "date": "2010-12-17", "numbers": [{"type": "pourvoi", "value": "10-10.906"}], "publication": {"bulletin": "Bull. civ., I, n°270"}}]}
{"id": "P096", "src": "WEB (résumé WebSearch, guide BU)", "in": "Cass. 1re civ., 2 juill. 2014, n° 13-21.929", "expect": [{"kind": "Decision", "match": "Cass. 1re civ., 2 juill. 2014, n° 13-21.929", "court": "CCASS", "chamber": "CIV1", "date": "2014-07-02", "numbers": [{"type": "pourvoi", "value": "13-21.929"}]}]}
{"id": "P097", "src": "TP justicelibre scratchpad/audit", "in": "Cass. 2e civ., 26 septembre 2019, n° 18-14.708", "expect": [{"kind": "Decision", "match": "Cass. 2e civ., 26 septembre 2019, n° 18-14.708", "court": "CCASS", "chamber": "CIV2", "date": "2019-09-26", "numbers": [{"type": "pourvoi", "value": "18-14.708"}]}]}
{"id": "P098", "src": "TP justicelibre scratchpad/audit", "in": "Cass. 3e civ., 2 juillet 2003, n° 02-70.047, Bull. 2003, III, n° 140", "expect": [{"kind": "Decision", "match": "Cass. 3e civ., 2 juillet 2003, n° 02-70.047, Bull. 2003, III, n° 140", "court": "CCASS", "chamber": "CIV3", "date": "2003-07-02", "numbers": [{"type": "pourvoi", "value": "02-70.047"}], "publication": {"bulletin": "Bull. 2003, III, n° 140"}, "resolve_hint": {"id": "JURITEXT000007049013"}}]}
{"id": "P099", "src": "TP justicelibre scratchpad/audit", "in": "Cass. 2e civ., AVIS du 27 juin 2019, n° 19-70.006", "expect": [{"kind": "Decision", "match": "Cass. 2e civ., AVIS du 27 juin 2019, n° 19-70.006", "court": "CCASS", "chamber": "CIV2", "decision_kind": "AVIS", "date": "2019-06-27", "numbers": [{"type": "pourvoi", "value": "19-70.006"}], "resolve_hint": {"id": "JURITEXT000038734270"}}]}
{"id": "P100", "src": "FX consult_juri.json:text.texte (en-tête d'arrêt)", "in": "Arrêt n° 399 F Pourvoi n° N 24-13.470 R É P U B L I Q U E", "expect": [{"kind": "Decision", "match": "Arrêt n° 399 F Pourvoi n° N 24-13.470", "court": "CCASS", "arret_num": 399, "formation_code": "F", "numbers": [{"type": "pourvoi", "value": "24-13.470", "key_letter": "N"}]}]}
{"id": "P101", "src": "TP justicelibre web/hub.html (titre Légifrance ancien format)", "in": "Cour de Cassation, Chambre civile 2, du 24 février 2005, 04-10.362, Publié au bulletin", "expect": [{"kind": "Decision", "match": "Cour de Cassation, Chambre civile 2, du 24 février 2005, 04-10.362, Publié au bulletin", "court": "CCASS", "chamber": "CIV2", "date": "2005-02-24", "numbers": [{"type": "pourvoi", "value": "04-10.362"}], "publication": {"bulletin": true}, "resolve_hint": {"id": "JURITEXT000007052697"}}]}
{"id": "P102", "src": "SYN (codes formation/publication)", "in": "Cass. soc., 12 févr. 2025, n° 23-12.345, FS-B", "expect": [{"kind": "Decision", "match": "Cass. soc., 12 févr. 2025, n° 23-12.345, FS-B", "court": "CCASS", "chamber": "SOC", "date": "2025-02-12", "numbers": [{"type": "pourvoi", "value": "23-12.345"}], "formation_code": "FS", "publication_codes": ["B"]}]}
{"id": "P103", "src": "SYN", "in": "Cass. ass. plén., 17 nov. 2000, n° 99-13.701", "expect": [{"kind": "Decision", "match": "Cass. ass. plén., 17 nov. 2000, n° 99-13.701", "court": "CCASS", "chamber": "AP", "date": "2000-11-17", "numbers": [{"type": "pourvoi", "value": "99-13.701"}]}]}
{"id": "P104", "src": "SYN", "in": "Cass., ch. mixte, 6 sept. 2002, n° 98-22.981", "expect": [{"kind": "Decision", "match": "Cass., ch. mixte, 6 sept. 2002, n° 98-22.981", "court": "CCASS", "chamber": "MIXTE", "date": "2002-09-06", "numbers": [{"type": "pourvoi", "value": "98-22.981"}]}]}
{"id": "P105", "src": "FX consult_juri_constit.json:text.texte", "in": "SAISI le 8 août 2025 par la Cour de cassation (chambre criminelle, arrêt n° 1133 du 6 août 2025), dans les conditions", "expect": [{"kind": "Decision", "match": "Cour de cassation (chambre criminelle, arrêt n° 1133 du 6 août 2025)", "court": "CCASS", "chamber": "CRIM", "date": "2025-08-06", "arret_num": 1133}]}
{"id": "P106", "src": "SYN (ordinaux en exposant, « no », U+2011)", "in": "Cass., 1\u02b3\u1d49 civ., 4 juin 2025, pourvoi no 24\u201113.470", "expect": [{"kind": "Decision", "match": "Cass., 1\u02b3\u1d49 civ., 4 juin 2025, pourvoi no 24\u201113.470", "court": "CCASS", "chamber": "CIV1", "date": "2025-06-04", "numbers": [{"type": "pourvoi", "value": "24-13.470"}]}]}
{"id": "P107", "src": "SYN", "in": "Première chambre civile, 4 juin 2025, 24-13.470", "expect": [{"kind": "Decision", "match": "Première chambre civile, 4 juin 2025, 24-13.470", "court": "CCASS", "chamber": "CIV1", "date": "2025-06-04", "numbers": [{"type": "pourvoi", "value": "24-13.470"}]}]}
{"id": "P108", "src": "SYN", "in": "Cass. civ. 1re, 4 juin 2025, n° 24-13.470", "expect": [{"kind": "Decision", "match": "Cass. civ. 1re, 4 juin 2025, n° 24-13.470", "court": "CCASS", "chamber": "CIV1", "date": "2025-06-04", "numbers": [{"type": "pourvoi", "value": "24-13.470"}]}]}
{"id": "P109", "src": "FX consult_juri.json:text.citationJp (octets bruts, mojibake)", "in": "1re Civ., 25 mai 2023, pourvoi n\u00c2° 21-23.174 (cassation partielle)", "expect": [{"kind": "Decision", "match": "1re Civ., 25 mai 2023, pourvoi n\u00c2° 21-23.174 (cassation partielle)", "court": "CCASS", "chamber": "CIV1", "date": "2023-05-25", "numbers": [{"type": "pourvoi", "value": "21-23.174"}], "requires": "repair_mojibake"}]}
{"id": "P110", "src": "WEB labase-lextenso.fr (résumé WebSearch)", "in": "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215", "expect": [{"kind": "Decision", "match": "Cour d'appel de Paris, Pôle 1 - Chambre 12, 23 juin 2025, n°24/15215", "court": "CA", "seat": "PARIS", "chamber": "Pôle 1 - Chambre 12", "date": "2025-06-23", "numbers": [{"type": "rg", "value": "24/15215"}]}]}
{"id": "P111", "src": "WEB labase-lextenso.fr (résumé WebSearch)", "in": "Cour d'appel de Versailles, 6e Chambre, 4 juin 2020, n°17/04940", "expect": [{"kind": "Decision", "match": "Cour d'appel de Versailles, 6e Chambre, 4 juin 2020, n°17/04940", "court": "CA", "seat": "VERSAILLES", "chamber": "6e Chambre", "date": "2020-06-04", "numbers": [{"type": "rg", "value": "17/04940"}]}]}
{"id": "P112", "src": "TP justicelibre web/hub.html (titre Légifrance JURI)", "in": "Cour d'appel de Limoges, 21 mai 2015, 14/00554", "expect": [{"kind": "Decision", "match": "Cour d'appel de Limoges, 21 mai 2015, 14/00554", "court": "CA", "seat": "LIMOGES", "date": "2015-05-21", "numbers": [{"type": "rg", "value": "14/00554"}], "resolve_hint": {"id": "JURITEXT000030629569"}}]}
{"id": "P113", "src": "SYN (d'après TP justicelibre : CA Paris 2024-04-04, Pôle 5 - Chambre 9, n° 23/13690)", "in": "CA Paris, pôle 5, ch. 9, 4 avr. 2024, n° 23/13690", "expect": [{"kind": "Decision", "match": "CA Paris, pôle 5, ch. 9, 4 avr. 2024, n° 23/13690", "court": "CA", "seat": "PARIS", "chamber": "pôle 5, ch. 9", "date": "2024-04-04", "numbers": [{"type": "rg", "value": "23/13690"}]}]}
{"id": "P114", "src": "TP justicelibre web/hub.html (en-tête CA Paris)", "in": "Numéro d'inscription au répertoire général : N° RG 23/13690 - N° Portalis 35L7-V-B7H-CIDHB", "expect": [{"kind": "Decision", "match": "N° RG 23/13690 - N° Portalis 35L7-V-B7H-CIDHB", "court": null, "numbers": [{"type": "rg", "value": "23/13690"}, {"type": "portalis", "value": "35L7-V-B7H-CIDHB"}]}]}
{"id": "P115", "src": "TP justicelibre web/hub.html (titre, date ISO)", "in": "Tribunal judiciaire de Lille, 2026-04-30, n° 25/00497", "expect": [{"kind": "Decision", "match": "Tribunal judiciaire de Lille, 2026-04-30, n° 25/00497", "court": "TJ", "seat": "LILLE", "date": "2026-04-30", "numbers": [{"type": "rg", "value": "25/00497"}]}]}
{"id": "P120", "src": "FX consult_juri_cetat.json:text.titre", "in": "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507", "expect": [{"kind": "Decision", "match": "Conseil d'État, 2ème - 7ème chambres réunies, 29/05/2026, 506507", "court": "CE", "formation": "CHR", "chambers": [2, 7], "date": "2026-05-29", "numbers": [{"type": "ce_requete", "value": "506507"}], "resolve_hint": {"id": "CETATEXT000054178503", "ecli": "ECLI:FR:CECHR:2026:506507.20260529"}}]}
{"id": "P121", "src": "FX search_cetat.json:results[0].titles[0].title", "in": "CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429", "expect": [{"kind": "Decision", "match": "CAA de MARSEILLE, 4ème chambre, 09/06/2026, 24MA02429", "court": "CAA", "seat": "MARSEILLE", "chamber": "4ème chambre", "date": "2026-06-09", "numbers": [{"type": "caa", "value": "24MA02429", "yy": 24, "court_code": "MA", "seq": 2429}], "resolve_hint": {"id": "CETATEXT000054242861", "ecli": null}}]}
{"id": "P122", "src": "FX search_cetat.json:results[1].titles[0].title", "in": "CAA de NANCY, 3ème chambre, 22/05/2025, 22NC02259", "expect": [{"kind": "Decision", "match": "CAA de NANCY, 3ème chambre, 22/05/2025, 22NC02259", "court": "CAA", "seat": "NANCY", "date": "2025-05-22", "numbers": [{"type": "caa", "value": "22NC02259"}], "resolve_hint": {"id": "CETATEXT000051655317"}}]}
{"id": "P123", "src": "FX consult_juri_cetat.json:text.texte", "in": "Par un arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy a, sur appel", "expect": [{"kind": "Decision", "match": "arrêt n° 22NC02259 du 22 mai 2025 la cour administrative d'appel de Nancy", "court": "CAA", "seat": "NANCY", "date": "2025-05-22", "numbers": [{"type": "caa", "value": "22NC02259"}]}]}
{"id": "P124", "src": "FX consult_juri_cetat.json:text.texte", "in": "Par un jugement nos 2005064, 2006015, 2006016 du 30 juin 2022, le tribunal administratif a rejeté sa demande.", "expect": [{"kind": "Decision", "match": "jugement nos 2005064, 2006015, 2006016 du 30 juin 2022, le tribunal administratif", "court": "TA", "seat": null, "date": "2022-06-30", "numbers": [{"type": "ta", "value": "2005064"}, {"type": "ta", "value": "2006015"}, {"type": "ta", "value": "2006016"}]}]}
{"id": "P125", "src": "WEB legifrance.gouv.fr/ceta/id/CETATEXT000008089130 (titre via WebSearch)", "in": "Conseil d'Etat, 7 / 5 SSR, du 28 décembre 2001, 216642, mentionné aux tables du recueil Lebon", "expect": [{"kind": "Decision", "match": "Conseil d'Etat, 7 / 5 SSR, du 28 décembre 2001, 216642, mentionné aux tables du recueil Lebon", "court": "CE", "formation": "SSR", "chambers": [7, 5], "date": "2001-12-28", "numbers": [{"type": "ce_requete", "value": "216642"}], "publication": {"lebon": "tables"}, "resolve_hint": {"id": "CETATEXT000008089130"}}]}
{"id": "P126", "src": "WEB juricaf.org (résumé WebSearch)", "in": "CE, 14 avril 2023, n° 462797, mentionné aux tables du Recueil Lebon", "expect": [{"kind": "Decision", "match": "CE, 14 avril 2023, n° 462797, mentionné aux tables du Recueil Lebon", "court": "CE", "formation": null, "date": "2023-04-14", "numbers": [{"type": "ce_requete", "value": "462797"}], "publication": {"lebon": "tables"}}]}
{"id": "P127", "src": "WEB droit-oi.univ-reunion.fr charte RJOI (résumé WebSearch)", "in": "CE, Sect., 22 février 2007, req. n° 264541, Leb., p. 541", "expect": [{"kind": "Decision", "match": "CE, Sect., 22 février 2007, req. n° 264541, Leb., p. 541", "court": "CE", "formation": "SECTION", "date": "2007-02-22", "numbers": [{"type": "ce_requete", "value": "264541"}], "publication": {"lebon": "p. 541"}}]}
{"id": "P128", "src": "SYN (de mémoire, NON VÉRIFIÉ)", "in": "CE, ass., 3 févr. 1989, Compagnie Alitalia, n° 74052, Rec. p. 44", "expect": [{"kind": "Decision", "match": "CE, ass., 3 févr. 1989, Compagnie Alitalia, n° 74052, Rec. p. 44", "court": "CE", "formation": "ASSEMBLEE", "date": "1989-02-03", "parties": "Compagnie Alitalia", "numbers": [{"type": "ce_requete", "value": "74052"}], "publication": {"lebon": "p. 44"}}]}
{"id": "P129", "src": "SYN", "in": "CE, 5e et 6e ch. réunies, 10 mars 2025, n° 489701", "expect": [{"kind": "Decision", "match": "CE, 5e et 6e ch. réunies, 10 mars 2025, n° 489701", "court": "CE", "formation": "CHR", "chambers": [5, 6], "date": "2025-03-10", "numbers": [{"type": "ce_requete", "value": "489701"}]}]}
{"id": "P130", "src": "SYN cohérent avec TP ECLI:FR:CEORD:2021:454852.20210727", "in": "CE, ord., 27 juillet 2021, n° 454852", "expect": [{"kind": "Decision", "match": "CE, ord., 27 juillet 2021, n° 454852", "court": "CE", "formation": "ORDONNANCE", "date": "2021-07-27", "numbers": [{"type": "ce_requete", "value": "454852"}], "ecli_expected": "ECLI:FR:CEORD:2021:454852.20210727"}]}
{"id": "P131", "src": "TP justicelibre web/hub.html (début de texte Ariane)", "in": "Conseil d'État N° 99522 ECLI:FR:CESSR:1976:99522.19761215 Publié au recueil Lebon Section du Contentieux", "expect": [{"kind": "Decision", "match": "Conseil d'État N° 99522", "court": "CE", "numbers": [{"type": "ce_requete", "value": "99522"}]}, {"kind": "Ecli", "match": "ECLI:FR:CESSR:1976:99522.19761215", "country": "FR", "court": "CESSR", "year": 1976, "ordinal": "99522.19761215"}]}
{"id": "P132", "src": "SYN cohérent avec FX consult_juri_cetat.json (TA Strasbourg, 30 juin 2022, n° 2005064)", "in": "TA Strasbourg, 30 juin 2022, n° 2005064", "expect": [{"kind": "Decision", "match": "TA Strasbourg, 30 juin 2022, n° 2005064", "court": "TA", "seat": "STRASBOURG", "date": "2022-06-30", "numbers": [{"type": "ta", "value": "2005064"}]}]}
{"id": "P133", "src": "SYN cohérent avec FX search_cetat.json", "in": "CAA Nancy, 3e ch., 22 mai 2025, n° 22NC02259", "expect": [{"kind": "Decision", "match": "CAA Nancy, 3e ch., 22 mai 2025, n° 22NC02259", "court": "CAA", "seat": "NANCY", "date": "2025-05-22", "numbers": [{"type": "caa", "value": "22NC02259"}]}]}
{"id": "P140", "src": "FX test_parse.py:116 (sortie réf. Python, « decision » sans accent)", "in": "Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025 - Absence de publicité des débats", "expect": [{"kind": "Decision", "match": "Cons. const., decision n° 2025-1173 QPC du 7 novembre 2025", "court": "CC", "date": "2025-11-07", "numbers": [{"type": "cc", "value": "2025-1173", "cc_type": "QPC"}], "resolve_hint": {"id": "CONSTEXT000052555275"}}]}
{"id": "P141", "src": "FX consult_juri_constit.json:text.titre (U+00A0 après « M. »)", "in": "Décision 2025-1173 QPC - 07 novembre 2025 - M.\u00a0Amine G. [Absence de publicité des débats devant le juge des libertés et de la détention statuant sur la détention provisoire du prévenu] - Conformité", "expect": [{"kind": "Decision", "match": "Décision 2025-1173 QPC - 07 novembre 2025", "court": "CC", "date": "2025-11-07", "numbers": [{"type": "cc", "value": "2025-1173", "cc_type": "QPC"}], "parties": "M. Amine G.", "subject": "Absence de publicité des débats devant le juge des libertés et de la détention statuant sur la détention provisoire du prévenu", "solution": "Conformité"}]}
{"id": "P142", "src": "FX search_constit.json:results[0].titles[0].title (marks retirées)", "in": "Décision 2021-817 DC - 20 mai 2021 - Loi pour une sécurité globale préservant les libertés  - Non conformité partielle - réserve", "expect": [{"kind": "Decision", "match": "Décision 2021-817 DC - 20 mai 2021", "court": "CC", "date": "2021-05-20", "numbers": [{"type": "cc", "value": "2021-817", "cc_type": "DC"}], "resolve_hint": {"id": "CONSTEXT000043596386"}}]}
{"id": "P143", "src": "FX search_constit.json:results[1].titles[0].title (marks retirées)", "in": "Décision 2018-769 DC - 04 septembre 2018 - Loi pour la liberté de choisir son avenir professionnel  - Non conformité partielle", "expect": [{"kind": "Decision", "match": "Décision 2018-769 DC - 04 septembre 2018", "court": "CC", "date": "2018-09-04", "numbers": [{"type": "cc", "value": "2018-769", "cc_type": "DC"}], "resolve_hint": {"id": "CONSTEXT000037434572"}}]}
{"id": "P144", "src": "FX consult_juri_constit.json:text.texte", "in": "Elle a été enregistrée au secrétariat général du Conseil constitutionnel sous le n° 2025-1173 QPC.", "expect": [{"kind": "Decision", "match": "n° 2025-1173 QPC", "court": "CC", "numbers": [{"type": "cc", "value": "2025-1173", "cc_type": "QPC"}]}]}
{"id": "P145", "src": "SYN (numéros joints, de mémoire)", "in": "Cons. const., 30 juill. 2010, n° 2010-14/22 QPC", "expect": [{"kind": "Decision", "match": "Cons. const., 30 juill. 2010, n° 2010-14/22 QPC", "court": "CC", "date": "2010-07-30", "numbers": [{"type": "cc", "value": "2010-14/22", "cc_type": "QPC", "joined": [14, 22]}]}]}
{"id": "P146", "src": "SYN", "in": "CC, déc. n° 2009-580 DC du 10 juin 2009", "expect": [{"kind": "Decision", "match": "CC, déc. n° 2009-580 DC du 10 juin 2009", "court": "CC", "date": "2009-06-10", "numbers": [{"type": "cc", "value": "2009-580", "cc_type": "DC"}]}]}
{"id": "P147", "src": "TP justicelibre web/hub.html (texte CONSTIT)", "in": "tendant à la rectification pour erreur matérielle de la décision n° 2015-491 QPC du 14 octobre 2015 par laquelle", "expect": [{"kind": "Decision", "match": "décision n° 2015-491 QPC du 14 octobre 2015", "court": "CC", "date": "2015-10-14", "numbers": [{"type": "cc", "value": "2015-491", "cc_type": "QPC"}]}]}
{"id": "P148", "src": "FX consult_juri_constit.json:text.natureNumero", "in": "QPC 2025-1173", "expect": [{"kind": "Decision", "match": "QPC 2025-1173", "court": "CC", "numbers": [{"type": "cc", "value": "2025-1173", "cc_type": "QPC"}]}]}
{"id": "P149", "src": "SYN (type ELEC/AN)", "in": "Cons. const., 18 janv. 2024, n° 2023-6130 AN", "expect": [{"kind": "Decision", "match": "Cons. const., 18 janv. 2024, n° 2023-6130 AN", "court": "CC", "date": "2024-01-18", "numbers": [{"type": "cc", "value": "2023-6130", "cc_type": "AN"}]}]}
{"id": "P150", "src": "FX consult_juri.json:text.ecli", "in": "ECLI:FR:CCASS:2025:C100399", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CCASS:2025:C100399", "country": "FR", "court": "CCASS", "year": 2025, "ordinal": "C100399", "fr": {"cass_chamber": "C1", "arret_seq": 399}}]}
{"id": "P151", "src": "FX consult_juri_constit.json:text.ecli", "in": "Rendu public le 7 novembre 2025. ECLI:FR:CC:2025:2025.1173.QPC", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CC:2025:2025.1173.QPC", "country": "FR", "court": "CC", "year": 2025, "ordinal": "2025.1173.QPC", "fr": {"cc_num": "2025-1173", "cc_type": "QPC"}}]}
{"id": "P152", "src": "FX consult_juri_cetat.json:text.ecli", "in": "ECLI:FR:CECHR:2026:506507.20260529", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CECHR:2026:506507.20260529", "country": "FR", "court": "CECHR", "year": 2026, "ordinal": "506507.20260529", "fr": {"ce_formation": "CHR", "ce_requete": "506507", "date": "2026-05-29"}}]}
{"id": "P153", "src": "TP justicelibre web/hub.html", "in": "ECLI:FR:CCASS:2022:OR31631", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CCASS:2022:OR31631", "court": "CCASS", "year": 2022, "ordinal": "OR31631", "fr": {"cass_chamber": "OR"}}]}
{"id": "P154", "src": "TP justicelibre sources/ariane.py:118", "in": "ECLI:FR:CEORD:2021:454852.20210727", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CEORD:2021:454852.20210727", "court": "CEORD", "year": 2021, "ordinal": "454852.20210727", "fr": {"ce_formation": "ORDONNANCE", "ce_requete": "454852", "date": "2021-07-27"}}]}
{"id": "P155", "src": "TP justicelibre web/hub.html", "in": "ECLI:FR:CECHS:2019:420427.20190430", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CECHS:2019:420427.20190430", "court": "CECHS", "year": 2019, "ordinal": "420427.20190430", "fr": {"ce_formation": "CHS"}}]}
{"id": "P156", "src": "TP justicelibre tests/test_parse_dila_champs.py:130 (année ECLI ≠ année du numéro)", "in": "<ECLI>ECLI:FR:CC:2026:2025.1180.QPC</ECLI>", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CC:2026:2025.1180.QPC", "court": "CC", "year": 2026, "ordinal": "2025.1180.QPC", "fr": {"cc_num": "2025-1180", "cc_type": "QPC"}}]}
{"id": "P157", "src": "WEB e-justice.europa.eu (ECLI France, résumé WebSearch)", "in": "ECLI:FR:CESSR:2013:355099.20130301", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CESSR:2013:355099.20130301", "court": "CESSR", "year": 2013, "ordinal": "355099.20130301"}]}
{"id": "P158", "src": "WEB e-justice.europa.eu (ECLI France, résumé WebSearch)", "in": "ECLI:FR:CCASS:2013:CR00710", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CCASS:2013:CR00710", "court": "CCASS", "year": 2013, "ordinal": "CR00710", "fr": {"cass_chamber": "CR"}}]}
{"id": "P159", "src": "TP justicelibre web/hub.html (CJUE)", "in": "ECLI:EU:C:2024:216", "expect": [{"kind": "Ecli", "match": "ECLI:EU:C:2024:216", "country": "EU", "court": "C", "year": 2024, "ordinal": "216", "resolvable": false}]}
{"id": "P160", "src": "SYN (minuscules)", "in": "ecli:fr:ccass:2025:c100399", "expect": [{"kind": "Ecli", "match": "ecli:fr:ccass:2025:c100399", "court": "CCASS", "year": 2025, "ordinal": "C100399"}]}
{"id": "P161", "src": "WEB reflex.sne.fr (résumé WebSearch)", "in": "ECLI:FR:CCASS:2015:AP00623", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CCASS:2015:AP00623", "court": "CCASS", "year": 2015, "ordinal": "AP00623", "fr": {"cass_chamber": "AP"}}]}
{"id": "P170", "src": "FX consult_cnil.json:text.titre", "in": "Délibération SAN-2026-002 du 8 janvier 2026", "expect": [{"kind": "CnilDeliberation", "match": "Délibération SAN-2026-002 du 8 janvier 2026", "series": "SAN", "year": 2026, "seq": 2, "num": "SAN-2026-002", "date": "2026-01-08", "resolve_hint": {"id": "CNILTEXT000053352643"}}]}
{"id": "P171", "src": "FX consult_cnil.json:text.titreLong", "in": "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026 prononçant une sanction pécuniaire à l'encontre de la société FREE", "expect": [{"kind": "CnilDeliberation", "match": "Délibération de la formation restreinte n° SAN-2026-002 du 8 janvier 2026", "series": "SAN", "num": "SAN-2026-002", "date": "2026-01-08", "formation": "formation restreinte"}]}
{"id": "P172", "src": "FX search_cnil.json:results[1].titles[0].title (tiret demi-cadratin U+2013)", "in": "Délibération de la formation restreinte n° SAN\u20132026-003 du 22 janvier 2026 prononçant une sanction", "expect": [{"kind": "CnilDeliberation", "match": "Délibération de la formation restreinte n° SAN\u20132026-003 du 22 janvier 2026", "series": "SAN", "num": "SAN-2026-003", "date": "2026-01-22", "resolve_hint": {"id": "CNILTEXT000053408671"}}]}
{"id": "P173", "src": "FX citations.py:378 (sortie réf. Python, « deliberation » sans accent)", "in": "CNIL, deliberation n° SAN-2026-002 du 8 janvier 2026", "expect": [{"kind": "CnilDeliberation", "match": "CNIL, deliberation n° SAN-2026-002 du 8 janvier 2026", "num": "SAN-2026-002", "date": "2026-01-08"}]}
{"id": "P174", "src": "SYN d'après TP justicelibre (« Délibération n°2023-062 du 13 avril 2023 », numéro CNIL)", "in": "CNIL, délibération n°2023-062 du 13 avril 2023", "expect": [{"kind": "CnilDeliberation", "match": "CNIL, délibération n°2023-062 du 13 avril 2023", "series": null, "year": 2023, "seq": 62, "num": "2023-062", "date": "2023-04-13"}]}
{"id": "P175", "src": "SYN d'après TP justicelibre (numéro MED-2018-024)", "in": "CNIL, décision n° MED-2018-024", "expect": [{"kind": "CnilDeliberation", "match": "CNIL, décision n° MED-2018-024", "series": "MED", "year": 2018, "seq": 24, "num": "MED-2018-024"}]}
{"id": "P176", "src": "SYN (exemple de la tâche)", "in": "délib. CNIL n° 2024-012 du 1er févr. 2024", "expect": [{"kind": "CnilDeliberation", "match": "délib. CNIL n° 2024-012 du 1er févr. 2024", "num": "2024-012", "date": "2024-02-01"}]}
{"id": "P180", "src": "FX consult_kali.json:conteneurs[0].titre", "in": "Convention collective nationale de l'édition du 14 janvier 2000\n", "expect": [{"kind": "Convention", "match": "Convention collective nationale de l'édition du 14 janvier 2000", "conv_kind": "CCN", "title_hint": "de l'édition", "date": "2000-01-14", "resolve_hint": {"kalicont": "KALICONT000005635096", "idcc": 2121}}]}
{"id": "P181", "src": "FX search_kali.json:results[1].titles[0].title", "in": "Avenant n° 18 du 19 juillet 2002 relatif à la médaille d'honneur du travail", "expect": [{"kind": "Convention", "match": "Avenant n° 18 du 19 juillet 2002", "conv_kind": "AVENANT", "avenant_num": 18, "date": "2002-07-19", "resolve_hint": {"id": "KALITEXT000005661030"}}]}
{"id": "P182", "src": "SYN d'après WEB (Syntec : IDCC 1486, brochure 3018)", "in": "CCN Syntec (IDCC 1486, brochure n° 3018)", "expect": [{"kind": "Convention", "match": "CCN Syntec (IDCC 1486, brochure n° 3018)", "conv_kind": "CCN", "idcc": 1486, "brochure": 3018, "title_hint": "Syntec"}]}
{"id": "P183", "src": "SYN", "in": "IDCC n° 1486", "expect": [{"kind": "Convention", "match": "IDCC n° 1486", "idcc": 1486}]}
{"id": "P184", "src": "SYN (IDCC FX consult_kali.json conteneurs[0].num = 2121)", "in": "la convention collective nationale de l'édition (IDCC 2121)", "expect": [{"kind": "Convention", "match": "convention collective nationale de l'édition (IDCC 2121)", "conv_kind": "CCN", "idcc": 2121}]}
{"id": "P185", "src": "FX consult_acco.json:acco.codeIdcc (sur 3 chiffres)", "in": "IDCC 275", "expect": [{"kind": "Convention", "match": "IDCC 275", "idcc": 275}]}
{"id": "P190", "src": "FX consult_juri_constit.json:text.titreJo", "in": "JORF n°0263 du 8 novembre 2025, texte n° 152", "expect": [{"kind": "Jorf", "match": "JORF n°0263 du 8 novembre 2025, texte n° 152", "jo_num": 263, "date": "2025-11-08", "texte_num": 152, "resolve_hint": {"id": "JORFTEXT000052541898"}}]}
{"id": "P191", "src": "TP justicelibre tests/test_parse_dila_champs.py (TITRE_JO)", "in": "JORF n°0032 du 7 février 2026, texte n° 66", "expect": [{"kind": "Jorf", "match": "JORF n°0032 du 7 février 2026, texte n° 66", "jo_num": 32, "date": "2026-02-07", "texte_num": 66}]}
{"id": "P192", "src": "FX consult_loda.json (numParution 0235, dateParution 2016-10-08)", "in": "JORF n° 0235 du 8 octobre 2016", "expect": [{"kind": "Jorf", "match": "JORF n° 0235 du 8 octobre 2016", "jo_num": 235, "date": "2016-10-08"}]}
{"id": "P193", "src": "FX consult_loda.json:nor", "in": "NOR\u00a0: ECFI1524250L", "expect": [{"kind": "Nor", "match": "ECFI1524250L", "value": "ECFI1524250L", "ministere": "ECF", "service": "I", "yy": 15, "seq": 24250, "nature_letter": "L"}]}
{"id": "P194", "src": "FX search_constit.json:results[2].nor (minuscules)", "in": "NOR: cscx2531180s", "expect": [{"kind": "Nor", "match": "cscx2531180s", "value": "CSCX2531180S"}]}
{"id": "P195", "src": "OFF Swagger 2.4.2 (exemple ArticleIdEliOrAliasRequest, via rapport 05)", "in": "/eli/decret/2021/7/13/PRMD2117108D/jo/article_1", "expect": [{"kind": "Eli", "match": "/eli/decret/2021/7/13/PRMD2117108D/jo/article_1", "eli_type": "decret", "date": "2021-07-13", "nor": "PRMD2117108D", "version": "jo", "part": "article_1"}]}
{"id": "P196", "src": "SYN (hôte + chemin de l'exemple Swagger JorfConsultWithIdEliAliasRequest)", "in": "https://www.legifrance.gouv.fr/eli/decret/2018/2/13/JUSC1732516D/jo/texte", "expect": [{"kind": "Eli", "match": "https://www.legifrance.gouv.fr/eli/decret/2018/2/13/JUSC1732516D/jo/texte", "eli_type": "decret", "date": "2018-02-13", "nor": "JUSC1732516D", "version": "jo", "part": "texte"}]}
{"id": "P197", "src": "FX consult_article.json:article.id", "in": "LEGIARTI000006419288", "expect": [{"kind": "LegifranceId", "match": "LEGIARTI000006419288", "prefix": "LEGIARTI", "num": "000006419288"}]}
{"id": "P198", "src": "FX consult_loda.json:id", "in": "LEGITEXT000033205014_08-09-2023", "expect": [{"kind": "LegifranceId", "match": "LEGITEXT000033205014_08-09-2023", "prefix": "LEGITEXT", "num": "000033205014", "version_date": "2023-09-08"}]}
{"id": "P199", "src": "TP judilibre-search sample_detail_resolved.json:rapprochements[0].url", "in": "https://www.legifrance.gouv.fr/juri/id/JURITEXT000036780011", "expect": [{"kind": "LegifranceId", "match": "JURITEXT000036780011", "prefix": "JURITEXT", "url_kind": "juri"}]}
{"id": "P200", "src": "FX consult_kali.json:conteneurs[0].cid", "in": "KALICONT000005635096", "expect": [{"kind": "LegifranceId", "match": "KALICONT000005635096", "prefix": "KALICONT"}]}
```

### 8.4 Cas négatifs, ambigus et suspects (42) — contenu intégral de `corpus_neg.jsonl`

```jsonl
{"id": "N01", "src": "FX consult_juri.json:text.liens[0].title", "in": "1 du code civil.", "expect": [], "verdict": "fragment", "note": "Titre de lien tronqué par l'amont : nombre sans ancre « article »/« art. »/préfixe L|R|D|A. Ne rien émettre en mode texte libre ; un mode 'liens' dédié peut l'accepter avec confidence=Low."}
{"id": "N02", "src": "SYN", "in": "Le préavis est fixé par L. 1234-5.", "expect": [{"kind": "CodeArticle", "match": "L. 1234-5", "code": null, "num": "L1234-5", "ambiguity": "code_manquant"}], "verdict": "ambigu", "note": "Préfixe L. = article de code, mais code inconnu : fr_resolve doit refuser de deviner (le même numéro existe dans plusieurs codes)."}
{"id": "N03", "src": "SYN", "in": "conformément à l'article 12", "expect": [{"kind": "CodeArticle", "match": "article 12", "code": null, "num": "12", "ambiguity": "texte_manquant"}], "verdict": "ambigu"}
{"id": "N04", "src": "SYN", "in": "art. L. 52-8 CE", "expect": [{"kind": "CodeArticle", "match": "art. L. 52-8 CE", "code": null, "num": "L52-8", "ambiguity": "sigle_ambigu:CE"}], "verdict": "ambigu", "note": "« CE » = Conseil d'État, Code électoral ou (rare) Code de l'environnement : ne pas résoudre sans contexte."}
{"id": "N05", "src": "SYN", "in": "article 121-3 CP", "expect": [{"kind": "CodeArticle", "match": "article 121-3 CP", "code": "Code pénal", "num": "121-3", "confidence": "low", "ambiguity": "sigle_ambigu:CP"}], "verdict": "ambigu", "note": "« CP » est utilisé pour Code pénal (tiers justicelibre sources/citations.py:34) mais reste ambigu ; confidence basse."}
{"id": "N06", "src": "SYN", "in": "pour la période 2016-1321", "expect": [], "verdict": "aucune_citation", "note": "Un motif AAAA-NNNN sans « loi/décret/ordonnance … n° » n'est pas une citation."}
{"id": "N07", "src": "SYN", "in": "loi n° 2016-1321 du 7 octobre 2015", "expect": [{"kind": "Text", "match": "loi n° 2016-1321 du 7 octobre 2015", "nature": "LOI", "num": "2016-1321", "date": "2015-10-07", "flags": ["annee_numero_differente_annee_date"]}], "verdict": "suspect", "note": "Syntaxe valide mais année du numéro (2016) ≠ année de la date (2015) : signal d'hallucination pour fr_verify_citations."}
{"id": "N08", "src": "SYN", "in": "Cass. civ. 4e, 3 mars 2020, n° 18-12.345", "expect": [{"kind": "Decision", "match": "Cass. civ. 4e, 3 mars 2020, n° 18-12.345", "court": "CCASS", "chamber": null, "flags": ["chambre_inexistante:civ4"]}], "verdict": "suspect", "note": "Il n'existe que trois chambres civiles numérotées (taxon Judilibre civ1..civ3)."}
{"id": "N09", "src": "SYN", "in": "CE, 31 février 2020, n° 123456", "expect": [{"kind": "Decision", "match": "CE, 31 février 2020, n° 123456", "court": "CE", "date": null, "flags": ["date_invalide"]}], "verdict": "suspect"}
{"id": "N10", "src": "SYN", "in": "ECLI:FR:CCASS:2025", "expect": [], "verdict": "aucune_citation", "note": "ECLI tronqué (4 composants au lieu de 5)."}
{"id": "N11", "src": "TP pylegifrance/pylegifrance/fonds/juri.py:865 (exemple de documentation tiers)", "in": "ECLI:FR:CE:2024:123456.20240101", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CE:2024:123456.20240101", "court": "CE", "flags": ["code_juridiction_CE_seul_jamais_utilise"]}], "verdict": "suspect", "note": "Syntaxe ECLI valide mais le code « CE » n'est jamais utilisé seul (e-Justice, résumé WebSearch)."}
{"id": "N12", "src": "TP pylegifrance/pylegifrance/fonds/juri.py:864", "in": "ECLI:FR:CC:2023:123.QPC", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CC:2023:123.QPC", "court": "CC", "flags": ["ordinal_cc_non_conforme"]}], "verdict": "suspect", "note": "Forme attendue AAAA.N.TYPE (FX ECLI:FR:CC:2025:2025.1173.QPC)."}
{"id": "N13", "src": "TP pylegifrance/tests/unit/fonds/test_juri_verification_endpoints.py:247", "in": "ECLI:FR:CCASS:2099:XX99999", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CCASS:2099:XX99999", "court": "CCASS", "flags": ["chambre_cass_inconnue:XX", "annee_future"]}], "verdict": "suspect"}
{"id": "N14", "src": "SYN (dérivé de FX consult_juri_cetat.json, date altérée)", "in": "ECLI:FR:CECHR:2026:506507.20260530", "expect": [{"kind": "Ecli", "match": "ECLI:FR:CECHR:2026:506507.20260530", "court": "CECHR", "year": 2026, "ordinal": "506507.20260530"}], "verdict": "verification_echoue", "note": "Syntaxe valide ; la décision 506507 existe mais avec l'ECLI natif ...20260529 : fr_verify_citations doit répondre « ne correspond pas » et proposer l'ECLI natif."}
{"id": "N15", "src": "SYN", "in": "délibération SAN-2026", "expect": [], "verdict": "aucune_citation", "note": "Numéro CNIL incomplet (SÉRIE-AAAA-NNN attendu)."}
{"id": "N16", "src": "SYN (numéro TP justicelibre sans contexte CNIL)", "in": "Délibération n° 2023-062 du 13 avril 2023 du conseil municipal", "expect": [{"kind": "CnilDeliberation", "match": "Délibération n° 2023-062 du 13 avril 2023", "confidence": "low", "ambiguity": "emetteur_inconnu"}], "verdict": "ambigu", "note": "Sans « CNIL » ni série SAN/MED, l'émetteur est inconnu (les délibérations municipales sont aussi numérotées AAAA-NNN)."}
{"id": "N17", "src": "SYN", "in": "l'avenant n° 18", "expect": [{"kind": "Convention", "match": "avenant n° 18", "conv_kind": "AVENANT", "avenant_num": 18, "ambiguity": "convention_inconnue"}], "verdict": "ambigu"}
{"id": "N18", "src": "SYN", "in": "la Soc. Générale a interjeté appel", "expect": [], "verdict": "aucune_citation", "note": "« Soc. » n'est une chambre que suivi de « , <date> »."}
{"id": "N19", "src": "SYN", "in": "Com. de communes du Pays Haut", "expect": [], "verdict": "aucune_citation"}
{"id": "N20", "src": "SYN", "in": "Crim., 28 mars 2018", "expect": [{"kind": "Decision", "match": "Crim., 28 mars 2018", "court": "CCASS", "chamber": "CRIM", "date": "2018-03-28", "numbers": [], "ambiguity": "numero_manquant"}], "verdict": "ambigu", "note": "Résolution par recherche JURI (formation+date) possible mais plusieurs décisions le même jour : retourner des candidats, jamais un id unique."}
{"id": "N21", "src": "SYN", "in": "Décision 2025-1173", "expect": [{"kind": "Decision", "match": "Décision 2025-1173", "court": null, "numbers": [{"type": "inconnu", "value": "2025-1173"}], "ambiguity": "juridiction_et_type_manquants"}], "verdict": "ambigu"}
{"id": "N22", "src": "SYN", "in": "NOR ECFI152425L", "expect": [], "verdict": "aucune_citation", "note": "11 caractères au lieu de 12."}
{"id": "N23", "src": "SYN", "in": "LEGIARTI00000641928", "expect": [], "verdict": "aucune_citation", "note": "11 chiffres au lieu de 12."}
{"id": "N24", "src": "SYN", "in": "article 9999 du code civil", "expect": [{"kind": "CodeArticle", "match": "article 9999 du code civil", "code": "Code civil", "num": "9999"}], "verdict": "verification_echoue", "note": "Parse OK ; fr_verify_citations → Introuvable (le Code civil s'arrête bien avant ; à confirmer par la recherche NUM_ARTICLE, jamais par une borne codée en dur)."}
{"id": "N25", "src": "SYN (connaissance générale NON VÉRIFIÉE : renumérotation par l'ordonnance n° 2016-131)", "in": "la responsabilité délictuelle de l'article 1382 du code civil", "expect": [{"kind": "CodeArticle", "match": "article 1382 du code civil", "code": "Code civil", "num": "1382"}], "verdict": "verification_a_dater", "note": "Le numéro existe en vigueur mais son contenu a changé (piège de renumérotation) : verify doit comparer le passage cité ou la date de référence, et signaler les versions."}
{"id": "N26", "src": "SYN", "in": "l'art. 1er", "expect": [{"kind": "CodeArticle", "match": "art. 1er", "code": null, "num": "1er", "ambiguity": "texte_manquant"}], "verdict": "ambigu"}
{"id": "N27", "src": "SYN", "in": "l'article 6 de la Convention", "expect": [{"kind": "TextArticle", "match": "article 6 de la Convention", "num": "6", "text": null, "ambiguity": "convention_non_identifiee"}], "verdict": "ambigu", "note": "CEDH ? convention collective ? convention fiscale ?"}
{"id": "N28", "src": "SYN", "in": "sous le n° 506507", "expect": [{"kind": "Decision", "match": "n° 506507", "court": null, "numbers": [{"type": "inconnu", "value": "506507"}], "ambiguity": "juridiction_manquante"}], "verdict": "ambigu"}
{"id": "N29", "src": "SYN", "in": "n° 21-23.174", "expect": [{"kind": "Decision", "match": "n° 21-23.174", "court": "CCASS", "numbers": [{"type": "pourvoi", "value": "21-23.174"}], "confidence": "medium", "court_via": "forme_du_numero"}], "verdict": "ambigu", "note": "La forme NN-NN.NNN est caractéristique des pourvois (Judilibre la détecte seule : query.js:53-54) mais la juridiction est inférée."}
{"id": "N30", "src": "SYN", "in": "le TA Paris a été saisi", "expect": [], "verdict": "aucune_citation", "note": "Mention de juridiction sans date ni numéro."}
{"id": "N31", "src": "SYN", "in": "Bull. civ. I, n° 270", "expect": [{"kind": "Decision", "match": "Bull. civ. I, n° 270", "court": "CCASS", "publication": {"bulletin": "Bull. civ. I, n° 270"}, "ambiguity": "reference_de_publication_seule"}], "verdict": "non_resoluble_directement"}
{"id": "N32", "src": "FX consult_juri.json:text.citationJp", "in": "à la lumière de la directive 85/374/CEE du Conseil du 25 juillet 1985", "expect": [{"kind": "Unsupported", "match": "directive 85/374/CEE du Conseil du 25 juillet 1985", "family": "UE"}], "verdict": "hors_perimetre", "note": "Acte UE : reconnu pour ne pas être confondu, non résoluble via Légifrance (EUR-Lex)."}
{"id": "N33", "src": "SYN", "in": "note sous D. 2016, p. 1234", "expect": [{"kind": "Unsupported", "match": "D. 2016, p. 1234", "family": "doctrine"}], "verdict": "hors_perimetre", "note": "« D. » = Recueil Dalloz ici, PAS un article D. 2016 : règle « D. + année(4) + , p. » prioritaire."}
{"id": "N34", "src": "SYN", "in": "JCP G 2016, 123", "expect": [{"kind": "Unsupported", "match": "JCP G 2016, 123", "family": "doctrine"}], "verdict": "hors_perimetre"}
{"id": "N35", "src": "SYN", "in": "la loi Macron", "expect": [{"kind": "Text", "match": "loi Macron", "nature": "LOI", "num": null, "appellation": "Macron", "ambiguity": "appellation"}], "verdict": "ambigu", "note": "Résolution possible via /suggest (appellations), jamais par le parseur seul."}
{"id": "N36", "src": "SYN (idée tiers juridix verify.rs:2021)", "in": "art. L. 1234-5a C. trav.", "expect": [], "verdict": "aucune_citation", "note": "Suffixe minuscule collé : ne pas tronquer en L1234-5 (risque d'inventer)."}
{"id": "N37", "src": "SYN", "in": "CA Paris, 31/06/2024, n° 23/13690", "expect": [{"kind": "Decision", "match": "CA Paris, 31/06/2024, n° 23/13690", "court": "CA", "date": null, "flags": ["date_invalide"]}], "verdict": "suspect"}
{"id": "N38", "src": "FX consult_juri_cetat.json:text.titre (fragment)", "in": "2ème - 7ème chambres réunies", "expect": [], "verdict": "aucune_citation"}
{"id": "N39", "src": "SYN", "in": "pourvoi n° 21-23.1745", "expect": [], "verdict": "aucune_citation", "note": "Numéro de pourvoi malformé (4 chiffres après le point)."}
{"id": "N40", "src": "FX consult_juri_cetat.json:text.texte", "in": "classement en zone 1AU du secteur", "expect": [], "verdict": "aucune_citation", "note": "Code de zonage PLU, pas un article."}
{"id": "N41", "src": "FX consult_juri_cetat.json:text.texte", "in": "aux consultations prévues à l'article R. 132-5.", "expect": [{"kind": "CodeArticle", "match": "article R. 132-5", "code": null, "num": "R132-5", "code_via": "contexte_document", "ambiguity": "code_implicite"}], "verdict": "ambigu", "note": "Renvoi interne d'un article cité (Code de l'urbanisme dans le contexte) : résolution seulement si le contexte de citation englobante est connu."}
{"id": "N42", "src": "FX search_kali.json:results[1].sections[0].extracts[0].num", "in": "8.4 de la convention)", "expect": [], "verdict": "aucune_citation", "note": "Valeur de champ num KALI malformée par l'amont, à ne pas parser comme citation en texte libre."}
```

---

## 9. Recommandation d'implémentation (crate `domain`)

### 9.1 Outillage : `winnow` (+ `regex`/`aho-corasick` pour le balayage), pas `regex` seul

| Option | Faits vérifiés | Avantages | Inconvénients | Verdict |
|---|---|---|---|---|
| `regex` 1.13.1 seul | MSRV 1.65 ; 1.13.1 publié le 2026-07-15 (`[RGX]/CHANGELOG.md:11`) ; **pas de look-around ni de backreferences** (`[RGX]/README.md:5-7`) | temps linéaire garanti, spans natifs | les motifs tiers utilisent look-behind (`(?<![\w-])`, `[JL]/sources/citations.py:41,95`) et backreference `\1` (`[LEGIPY]/legi/articles.py:71` `article_num_multi_sub`) ; énumérations, anaphores, priorités → code spaghetti | **pour le balayage et les jetons atomiques seulement** (ECLI, NOR, ids, numéros) |
| `nom` 8.0.0 + `nom_locate` 5.0.0 | nom 8.0.0 MSRV 1.65.0, dépend de `memchr` ; nom_locate 5.0.0 requiert `nom ^8` (index crates.io) | mature, connu | spans via crate supplémentaire ; erreurs moins riches | acceptable |
| **`winnow` 1.0.4** | MSRV 1.65.0 ; 1.0.0 du 2026-03-17, 1.0.4 du 2026-07-13 (`[WIN]/CHANGELOG.md:10-34`) ; MIT ; features `default = [std, ascii, binary]`, `parser`, `simd` (→ `memchr`), `debug` (index crates.io `features2`) ; `LocatingSlice` + `Parser::span()` / `Parser::with_span()` → `Range<usize>` (`[WIN]/src/stream/locating.rs:31`, `[WIN]/src/parser.rs:448,490`) ; `ascii::Caseless` ASCII uniquement (`[WIN]/src/ascii/caseless.rs:7-25`) ; `alt` limité à 10 branches par tuple depuis 1.0 (imbriquer) (`[WIN]/CHANGELOG.md:46,52`) | combinateurs + spans natifs, erreurs contextuelles, `seq!` | courbe d'apprentissage ; insensibilité aux accents à gérer via la vue clé | **recommandé** |
| `aho-corasick` 1.1.5 | MSRV 1.60.0 (index) | recherche simultanée de ~300 ancres (abréviations de codes, chambres, mots-clés) en un passage | — | **recommandé** pour le scanner |
| `unicode-normalization` 0.1.25 | MSRV 1.36 ; `UnicodeNormalization::nfc()`, `char::is_combining_mark` (`[UN]/src/lib.rs:83-114`) ; licence (MIT OR Apache-2.0) AND Unicode-3.0 (`[UN]/Cargo.toml:14`) | NFC + retrait de diacritiques pour la vue clé | vérifier la licence Unicode-3.0 dans `cargo-deny` | **recommandé** |

Dates : implémenter un `LegalDate` maison (validation jour/mois/bissextile) dans le domaine plutôt que `time` 0.3.55 (MSRV **1.88.0**, index) — conversion vers le type de date des adapters à la frontière. Dépendances de test : `serde_json`, `rstest` 0.27.0 (MSRV 1.85.0), `insta` 1.49.0 (MSRV 1.66.0), `proptest` 1.12.0 (MSRV 1.88) — versions lues dans l'index ; dates de publication autres que winnow/regex : NON VÉRIFIÉES (static.crates.io bloqué).

### 9.2 Architecture du module

```text
crates/domain/src/citations/
  mod.rs            API : parse(text, &ParseOptions) -> ParseOutput { citations: Vec<Located<Citation>>, diagnostics: Vec<Diagnostic> }
  normalize.rs      pipeline §3 (HTML, mojibake, NFC, repli, vue clé) + OffsetMap
  lexicon.rs        dates, mois, ordinaux, n°, romains, adverbes latins
  dict/codes.rs     CodeKey + titres + abréviations + alias (données statiques, §5)
  dict/courts.rs    sièges CA (35/36), CAA (9), TA (42), chambres Cass., formations CE, types CC
  scan.rs           ancres : aho-corasick (dictionnaires) + RegexSet (ECLI, NOR, ids, ELI, pourvoi, AA-NNNN)
  grammar/          code_article.rs, text.rs, constitution.rs, cass.rs, juri_fond.rs, ce.rs, admin.rs, cc.rs,
                    ecli.rs, cnil.rs, convention.rs, jorf.rs, ids.rs, unsupported.rs   (winnow, sur LocatingSlice<&str>)
  post.rs           énumérations, plages, anaphores (pile de contexte), qualificatifs de version, drapeaux
  ids.rs            newtypes stricts + FromStr/Display : Ecli, Nor, LegifranceId, Pourvoi, CaaNumber, TaNumber,
                    RgNumber, CcNumber, CnilNumber, Idcc, Eli
  format.rs         forme canonique (human_readable_citation) — corrige D-27 (accents, 1er, mois)
```

Port côté `application` (async, implémenté par `adapter-legifrance`, puis par l'adapter « dumps DILA ») :

```rust
#[async_trait] // ou fn -> impl Future selon les choix de l'axe 04
pub trait CitationResolver: Send + Sync {
    async fn resolve(&self, c: &Located<Citation>, ctx: &ResolveContext) -> Result<ResolveOutcome, ResolveError>;
}
pub enum ResolveOutcome {
    Resolved { ids: Vec<LegifranceId>, ecli: Option<Ecli>, matched: Vec<Field> },
    Candidates { items: Vec<Candidate>, reason: CandidateReason },
    Mismatch { field: Field, cited: String, actual: String, actual_id: Option<LegifranceId> },
    NotFound { query: String },
    Unverifiable { reason: UnverifiableReason },   // HorsPerimetre, NonPublic, HorsCouverture, AnaphoreNonResolue…
}
```

### 9.3 Types du domaine (proposition)

```rust
/// Offsets en OCTETS dans le texte d'ORIGINE (avant normalisation), frontières UTF-8 garanties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Span { pub start: usize, pub end: usize }

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Located<T> {
    pub value: T,
    pub span: Span,
    pub confidence: Confidence,          // High | Medium | Low
    pub ambiguity: Option<Ambiguity>,    // CodeManquant, TexteManquant, SigleAmbigu(&'static str), NumeroManquant,
                                         // JuridictionManquante, AnaphoreNonResolue, Appellation, EmetteurInconnu…
    pub flags: Vec<Flag>,                // DateInvalide, AnneeNumeroDifferente, ChambreInexistante, EcliCodeCeSeul,
                                         // EcliOrdinalNonConforme, ChambreCassInconnue, AnneeFuture, MojibakeRepare…
    pub enum_group: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum Citation {
    CodeArticle(CodeArticleCite),
    TextArticle(TextArticleCite),
    Text(TextCite),
    Decision(DecisionCite),
    Ecli(Ecli),
    CnilDeliberation(CnilCite),
    Convention(ConventionCite),
    Jorf(JorfCite),
    Nor(Nor),
    Eli(Eli),
    LegifranceId(LegifranceId),
    Unsupported(UnsupportedCite),        // { family: Ue | Cjue | Cedh | Doctrine | Recueil, raw }
}

pub struct CodeArticleCite { pub code: CodeRef, pub article: ArticleRef, pub version: Option<VersionQualifier> }
pub enum CodeRef { Known(CodeKey), Unknown(String), Anaphora(Anaphora), Missing }
pub struct ArticleRef { pub num: ArticleNum, pub sub: Vec<Subdivision>, pub range_to: Option<ArticleNum> }
pub struct ArticleNum {
    pub prefix: Option<ArticlePrefix>,   // L | LO | R | RStar | RStarStar | D | DStar | DStarStar | A
    pub special: Option<SpecialNum>,     // Premier | Unique | Liminaire | Preliminaire
    pub base: Vec<u32>,                  // [1234, 5] pour 1234-5
    pub latin: Option<LatinAdverb>,      // Bis, Ter, … , Other(String)
    pub letters: Option<String>,         // "B" dans 199 undecies B
}
impl ArticleNum { pub fn num_key(&self) -> String { /* "L1234-5", "199 undecies B" */ todo!() }
                  pub fn key_variants(&self) -> Vec<String> { /* "1er" -> ["1er","1"], "R*111-1" -> ["R*111-1","R111-1"] */ todo!() } }
pub enum Subdivision { Alinea(u16), DernierAlinea, Phrase(u16), Item(u16), Paragraphe(String /* I, II */), Lettre(char), Para(u16) }
pub enum VersionQualifier { Ancien, Nouveau, RedactionApplicableAuLitige, RedactionIssueDe(Box<TextCite>),
                            RedactionAnterieureA(Box<TextCite>), EnVigueurAu(LegalDate) }

pub struct TextCite { pub nature: TextNature, pub number: Option<TextNumber>, pub date: Option<LegalDate>,
                      pub title_hint: Option<String>, pub nor: Option<Nor>, pub anaphora: Option<TextAnaphora>,
                      pub appellation: Option<String> }
pub enum TextNature { Loi, LoiOrganique, LoiConstitutionnelle, LoiDeFinances, LoiDuPays, Ordonnance, Decret, DecretLoi,
                      Arrete, Reglement, Deliberation, Circulaire, Constitution, Ddhc, Preambule1946, CharteEnvironnement }
pub struct TextNumber { pub year: u16, pub seq: u32, pub raw: String }      // "93-1419" -> 1993/1419
pub struct TextArticleCite { pub text: Option<TextCite>, pub article: ArticleRef, pub at_date: Option<LegalDate> }

pub struct DecisionCite { pub court: Court, pub date: Option<LegalDate>, pub numbers: Vec<CaseNumber>,
                          pub kind: Option<DecisionKind>, pub publication: Option<Publication>,
                          pub solution: Option<String>, pub parties: Option<String>,
                          pub arret_num: Option<u32>, pub formation_code: Option<String> }
pub enum Court {
    CourDeCassation { chamber: Option<CassChamber> },   // Civ1 | Civ2 | Civ3 | Com | Soc | Crim | AssPlen | Mixte | Reunies | CivUnique | Requetes | PremierPresident
    CourAppel { seat: Option<CaSeat>, chamber: Option<String> },
    TribunalJudiciaire { seat: Option<String> },
    AutreJudiciaire { label: String },                    // T. com., CPH, TSA…
    ConseilEtat { formation: Option<CeFormation>, chambers: Vec<u8> },   // Assemblee | Section | ChambresReunies | ChambreSeule | SousSectionsReunies | SousSectionSeule | Ordonnance | Avis
    Caa { seat: Option<CaaSeat>, chamber: Option<String> },
    Ta { seat: Option<String>, formation: Option<String> },
    ConseilConstitutionnel,
    TribunalDesConflits,
}
pub enum CaseNumber { Pourvoi(Pourvoi), Rg(RgNumber), Portalis(String), CeRequete(u32), Caa(CaaNumber),
                      Ta(TaNumber), Cc(CcNumber), Unknown(String) }
pub struct Pourvoi { pub yy: u8, pub mid: u8, pub seq: u16, pub key: Option<char> }   // Display: "21-23.174"
pub struct CaaNumber { pub yy: u8, pub court: CaaCode, pub seq: u32 }                  // "24MA02429"
pub struct CcNumber { pub year: u16, pub seq: u32, pub joined: Vec<u32>, pub kind: CcType }
pub enum CcType { Dc, Qpc, L, Lp, Lom, Fnr, Rip, Ar16, D, I, Of, Orga, Pdr, An, Sen, Elec, Ref, Autr, Other(String) }

pub struct Ecli { pub country: String, pub court: String, pub year: u16, pub ordinal: String }
impl Ecli { pub fn fr(&self) -> Option<FrEcli> { todo!() } }   // Cass{chamber, seq} | Ce{formation, requete, date} | Cc{num} | Other
pub struct CnilCite { pub series: Option<String>, pub year: u16, pub seq: u16, pub date: Option<LegalDate>, pub formation_restreinte: bool }
pub struct ConventionCite { pub kind: ConventionKind, pub idcc: Option<Idcc>, pub brochure: Option<u16>,
                            pub avenant: Option<u32>, pub title_hint: Option<String>, pub date: Option<LegalDate> }
pub struct JorfCite { pub number: Option<u16>, pub date: LegalDate, pub texte: Option<u32>, pub page: Option<u32> }
pub struct LegalDate { y: u16, m: u8, d: u8 }   // constructeur validant ; Display ISO ; format_fr() « 1er janvier 2026 »
```

- `Serialize`/`#[serde(tag = "kind")]` (alignés sur le champ `kind` du corpus) : à placer derrière une feature `serde` optionnelle du crate `domain`, ou à remplacer par des DTO dans `adapter-mcp` si l'on veut un domaine sans aucune dépendance de sérialisation (choix d'architecture à trancher avec l'axe DTO, `…/research/03-dto.md`).
- Les identifiants stricts (`Ecli`, `Nor`, `LegifranceId`, `Pourvoi`, `CcNumber`, `CnilNumber`, `Idcc`, `CaaNumber`) implémentent `FromStr` (erreurs `thiserror`) et `Display` canonique : ils servent aussi à **valider les paramètres d'outils MCP** (`fr_resolve(ecli=…)`), en cohérence avec la décision « IDs typés ».
- `LegifranceId { prefix: LfPrefix, digits: u64, version: Option<LegalDate> }` se convertit vers les newtypes `TextId`/`ArticleId`/`DecisionId` déjà prévus par les autres axes.
- `CodeKey` : enum fermée (§5.2) + `Other(String)` ; la correspondance `CodeKey → LEGITEXT` vit dans l'adapter (cache `/list/code`), jamais dans le domaine.

### 9.4 Spans et offsets

1. `normalize()` renvoie `Normalized { text: String, key: String, map: OffsetMap }` ; `OffsetMap::to_orig(norm_byte) -> usize` et `to_orig_end(norm_byte_end) -> usize` (fin du caractère d'origine). Implémentation compacte : vecteur de segments `(norm_start, orig_start, norm_len, orig_len)` où chaque segment est « identique » (copie) ou « remplacé » (repli, mojibake, NFC) ; recherche dichotomique.
2. Les parseurs `winnow` travaillent sur `LocatingSlice<&str>` du texte **normalisé** et produisent des `Range<usize>` normalisés, convertis en `Span` d'origine par l'`OffsetMap` à la fin (une seule conversion).
3. Garantie testée : `&input[span.start..span.end]` est valide UTF-8 et, pour les cas du corpus, égal au champ `match`.
4. Sortie MCP : `{"start": 12, "end": 61, "text": "<extrait d'origine>"}` ; fournir aussi `char_start`/`char_end` (scalaires Unicode) car les clients JS comptent en UTF-16 — le champ `text` lève toute ambiguïté.
5. Énumérations : span = celui du **numéro** (comme `[JL]/sources/citations.py:64-68` qui renvoie la position du numéro) + `group_span` optionnel couvrant « articles … du code … ».
6. **Bornes des spans** (convention du corpus) : le span exclut les déterminants et prépositions de tête (`le`, `la`, `l'`, `du`, `de la`, `des`, `selon`, `en application de`) et la ponctuation finale (`.`, `,`, `;`, `)` non appariée), **sauf** le point qui termine une abréviation (`art. 1240 C. civ.`, P017) ; il inclut les subdivisions de tête (« premier alinéa de l'article … », « 3° du I de l'article … »), les qualificatifs de version accolés (« ancien », « dans sa rédaction applicable au litige »), les mentions de publication et la solution entre parenthèses pour les décisions ; pour une décision introduite par sa nature (« arrêt n° … du … la cour administrative d'appel de Nancy »), le span va de la nature à la juridiction.
7. Limites : taille d'entrée bornée (ex. 1 Mio par appel, à fixer par l'application), fenêtre de parsing par ancre bornée (ex. 400 caractères), aucune récursion non bornée → pas de dénis de service.

### 9.5 Algorithme de `parse()`

```text
1. n = normalize(input, opts)                         // §3
2. anchors = scan(n.key)                               // aho-corasick + RegexSet ; ancres triées par position
3. pour chaque ancre (gauche→droite) :
       essayer les grammaires compatibles avec l'ancre (ordre de priorité §4.0), sur une fenêtre [ancre-80, ancre+400]
       garder le résultat le plus long ; ignorer les ancres couvertes par un span déjà accepté
4. post :
       a. éclater les énumérations ; distribuer code_ref / text_ref finaux
       b. résoudre les anaphores avec la pile de contexte (dernier code, dernier texte par nature+date, dernière décision)
          — paramètre opts.default_text pour « l'article R. 132-5 » interne (N41)
       c. attacher les qualificatifs de version
       d. calculer flags (dates, cohérences ECLI, chambres, années) et confidence
5. convertir les spans en offsets d'origine ; produire diagnostics (fragments ignorés, ambiguïtés)
```

### 9.6 Qualité et tests

- **Corpus JSONL** (§8) en `tests/data/` + test table-driven (§8.2) ; `insta` pour figer l'extraction complète des textes de fixtures (`consult_juri_cetat.json` `texte` : ~15 citations ; `consult_juri_constit.json` `texte` : visas + articles) ; `proptest` : `parse(format(c)) == c` pour toutes les variantes produites par `format.rs` (y compris les sorties « historiques » de la référence : `Cons. const., decision n° …`, mois sans accents) ; fuzzing (`cargo fuzz`) de `parse` et des `FromStr` : aucune panique, spans toujours valides.
- Tests de non-régression des pièges : mojibake (P077, P109), U+00A0/U+202F (P042, P078), U+2011/U+2013 (P076, P172), `2018-2022` dans un titre (P056), `D. 2016, p.` (N33), `L. n°` vs `L. 1234` (P069 vs P020).
- Test d'intégration adapter (sandbox, `#[ignore]` sans secrets) : pour chaque `resolve_hint` du corpus, `fr_resolve` doit retrouver l'identifiant indiqué (≈ 25 cas FX/OFF/TP).

### 9.7 Application aux « liens entre décisions »

- Entrées : `citationJp` (« à rapprocher : 1re Civ., … »), `texte` (visas « Vu les articles … », renvois aux juges du fond « Par un arrêt n° 22NC02259 du 22 mai 2025 … »), `sommaire`, `liens[].title` (mode « liens » tolérant aux fragments, N01).
- Pour chaque `Decision` extraite : relation typée selon le contexte gauche (`à rapprocher`, `contra`, `cf.`, `décision attaquée`, `jugement de première instance`) — liste de marqueurs à constituer (NON VÉRIFIÉ, seul « à rapprocher » est observé dans `[FX]/consult_juri.json:92` et « Sur … à rapprocher : Com., 16 janvier … » dans `[FX]/search_juri.json`).
- Résolution via §7 ; Judilibre fournit déjà des `rapprochements` avec `JURITEXT` (`[JDL]/src/data/sample_detail_resolved.json`) — piste d'adapter complémentaire (hors API Légifrance).

---

## 10. Questions ouvertes et points NON VÉRIFIÉS

1. **Forme exacte de `NUM_ARTICLE`** pour `R*`/`D*` (avec ou sans `*`), `LO`, `1er`/`1`, suffixes latins (`39 octies` avec espace ?), lettres (`199 undecies B`) — à tester en sandbox (`fond: CODE_DATE`, `EXACTE`).
2. **`getArticleWithIdAndNum`** : `num` = `5-8` (Swagger) ou `L5-8` (xlsx) ? (`[R05]` §1.4).
3. **`NUM_AFFAIRE`** : la recherche `EXACTE` accepte-t-elle `21-23.174` **et** `21-23174` ? Les dumps DILA semblent stocker `19-19311` (TIERS `[JL]`).
4. **Code ECLI de la chambre mixte** (`MI` ?) et des chambres réunies ; codes ECLI des **CAA, TA, CA** (aucun exemple trouvé ; e-Justice ne liste que CC, CCASS, CE*) ; ECLI des avis contentieux du Conseil d'État.
5. **`caa_code` `VE`** (Versailles) jamais observé ; liste exhaustive des codes à deux lettres des CAA.
6. **Conflit d'identifiant** du Code rural et de la pêche maritime : `LEGITEXT000006071367` (`[JL]`) vs `LEGITEXT000022197698` (`[KM]`) ; plus généralement les 82 `LEGITEXT` tiers doivent être recoupés avec `/list/code`.
7. **Abréviations non attestées** : `C. mon. fin.`, `C. rur.`, `CRPM`, `C. santé publ.`, `CJA`, `CMF`, `CASF`, `COJ`, `CSI`, `CGFP`, `CG3P`/`CGPPP`, `C. route`, `C. élect.`, `C. transp.`… (usage tiers/courant seulement) ; la liste SNE/Ref-Lex n'a pas pu être lue directement.
8. **Numérotation des textes** : date de bascule de l'année sur 2 → 4 chiffres (2000 ?) ; numérotation commune ou séparée entre lois, ordonnances et décrets d'une même année ; numérotation des textes antérieurs à 1945.
9. **Sens du deuxième bloc du numéro de pourvoi** (`AA-NN.NNN`) et de la lettre-clé (`N`, `G`, `B`) ; observations : `40` pour des QPC, `70` pour un avis, `8x` au pénal (TIERS).
10. **Types de décisions du CC** : sens exact de `OF`, `ORGA`, `AR16`, `ELEC`, `REF`, `AUTR` ; forme des numéros joints (`2010-14/22`) et des rectifications (`2015-491R`) côté API ; filtres `NATURE_CONSTIT` par **libellé** (les valeurs de facette sont des libellés, §2.4) — acceptés en filtre ?
11. **CNIL** : format attendu par `NUM_DELIB` (avec série `SAN-` ?) ; signification de `MED` ; numérotation des décisions de mise en demeure publiques.
12. **Lettres de nature du NOR** autres que `L`, `R`, `C` (`D`, `A`, `S`, `M`…) — cohérentes avec les fixtures mais non sourcées.
13. **DDHC, Préambule de 1946, Charte de l'environnement** : présence, titre exact et numérotation dans LODA ; adressage des alinéas du Préambule.
14. **Brochures KALI** : aucun champ de recherche identifié ; borne supérieure de l'IDCC.
15. **`JorfContConsultRequest.num`** : correspond-il au numéro de parution (`0235`) ? Les « texte n° » sont-ils les rangs du sommaire ?
16. **Cause du mojibake** des fixtures LEGI/JURI (amont sandbox ou enregistrement) — détermine si la réparation doit être activée par défaut sur les réponses API ou seulement sur les entrées utilisateur.
17. Les dates de publication des crates autres que `winnow` 1.0.4 (2026-07-13) et `regex` 1.13.1 (2026-07-15) n'ont pas pu être confirmées (static.crates.io/docs.rs bloqués) ; vérifier la règle « ≥ 2 semaines » avant épinglage.
18. Les cas `SYN` « de mémoire » (P103 Perruche, P104, P128 Alitalia, P145 2010-14/22, P065–P068, N25 renumérotation de l'article 1382) sont **à valider** avant d'être utilisés en test d'intégration ; ils restent valables comme tests de **syntaxe**.
