# 10 — Dumps open data DILA : reconnaissance du fonds LEGI (T-M9-01)

Rapport de reconnaissance de l'unité V1-16 (plan de fin de projet, tâche T-M9-01), rédigé le 2026-10-29. Il lève V-185, V-186 et V-187 et consigne V-188 et V-189 (09 §2.15), pour le fonds LEGI, par mesure directe sur le serveur et sur l'archive réels de la DILA. La recherche 05 §6 avait travaillé sans accès à ces hôtes ; ce rapport la remplace là où les deux divergent (§9).

Légende des statuts :

- **MESURÉ** : relevé le 2026-10-29 sur le serveur ou sur les archives réelles (méthode au §2, commandes de contrôle au §12).
- **DOC DILA** : lu dans un document publié par la DILA (fiche de présentation LEGI du 2017-08-24, note de migration du 2023-10-04, DTD du 2018-10-17).
- **CALCUL** : déduit des mesures ci-dessus, sans mesure propre.
- **TIERS** : repris de la recherche 05 (code de projets tiers), non remesuré.

Périmètre : le fonds LEGI. Les autres fonds ne figurent qu'au §3.4 (volumes du listing) et dans quelques constats de structure (§5.8), hors périmètre M9 par défaut (exception D16 : ce rapport).

---

## 1. Résumé et verdicts

| Id | Question | Verdict | Valeur retenue |
|---|---|---|---|
| V-185 | Volumétrie des archives | **levée** (MESURÉ) | global LEGI 1 171 027 316 octets compressés ; 5 253 903 entrées, 2 557 045 fichiers, 9,51 Go de contenu, environ 25,9 Go une fois extrait sur disque ; 480 incréments, 2,05 Go au total, médiane 2,9 Mo (§3) |
| V-186 | Rythme de publication des globaux | **levée** avec un constat négatif (MESURÉ + DOC DILA) | un seul global sur le serveur, daté du 2025-07-13, soit 473 jours le 2026-10-29 ; la fiche promet « au minimum une fois par an » : non tenu. Incréments quotidiens, week-ends compris (§4) |
| V-187 | Arborescence et balises sur archive réelle | **levée** (MESURÉ) | l'arborescence et les listes de suppression diffèrent de l'annexe C §7.4-7.5 sur six points (§5, §6, §9) |
| V-188 | Encodage des fichiers XML | **consignée** (MESURÉ, LEGI) | UTF-8 déclaré et valide dans les 2 556 950 fichiers non vides du global, sans BOM ni DOCTYPE ni entité nommée (§7) |
| V-189 | Licence exacte par fonds | **consignée**, point juridique **ouvert** | les 12 fiches DILA lues disent « licence ouverte v2.0 » ; data.gouv.fr déclare l'identifiant `fr-lo` (Licence Ouverte de 2014) pour 10 des 11 fonds interrogés, ACCO étant introuvable (§8) |

Six faits qui changent la conception de `lf-adapter-dila` (détail au §10) :

1. **Une suppression est un déplacement, pas un retrait.** Dans l'incrément du 2026-10-28, 57 des 58 fichiers supprimés sont ré-ajoutés dans la même archive sous `non_vigueur` avec le même identifiant. Un magasin indexé par l'identifiant seul perdrait ces documents ; la clé doit contenir le chemin (§6).
2. **Les listes de suppression sont lues avant le contenu** dans le flux (elles sont les deux premiers fichiers de l'archive) mais doivent s'appliquer après lui.
3. **Les doublons existent dans le global lui-même** : 13 138 articles, 1 610 sections et 497 textes sont présents sous `en_vigueur` et sous `non_vigueur` (§5.4).
4. **Le répertoire `en_vigueur` n'indique pas l'état de l'article** : il contient 109 530 articles `ABROGE` et 374 300 `MODIFIE` ; la règle de vigueur reste `debut <= D < fin` sur les dates (§5.6).
5. **L'archive contient des fichiers vides (95) et des liens symboliques (6 842)**, tous dans l'arbre `eli/` ; les incréments de 2025 et de février 2026 en contiennent aussi (§5.1).
6. **Le global a 15,5 mois** : toute réponse doit porter la date du dernier incrément appliqué ; l'ingestion complète exige le global et 479 incréments, soit 2,05 Go de plus (§3.5).

---

## 2. Conditions de mesure

- L'hôte `echanges.dila.gouv.fr` et l'API `www.data.gouv.fr` répondent depuis cette machine (HTTP 200, sans proxy). La recherche 05 les avait trouvés injoignables ; la question Q6 (annexe C §7) est donc levée pour cette machine à cette date. MESURÉ.
- Politesse : User-Agent `mcp-legifrance-recon/0.1 (reconnaissance M9)`, une seule connexion à la fois, 0,8 s entre deux requêtes de la boucle `HEAD`. Au total : 19 pages de listing, 481 requêtes `HEAD` sur le répertoire LEGI (aucune erreur, aucun refus), le global téléchargé une fois (205,9 s, 5,69 Mo/s), cinq incréments LEGI (50,5 Mo), neuf incréments d'autres fonds (1,4 Mo), seize documents (15 PDF et le 7z des DTD, 2,4 Mo) et quelques appels à l'API data.gouv.fr.
- Archives LEGI téléchargées (octets exacts par `Content-Length`, empreintes calculées) :

| Archive | Octets | sha256 |
|---|---|---|
| `Freemium_legi_global_20250713-140000.tar.gz` | 1 171 027 316 | `f862b4a2238e11ec3c48ec70f3b1c68bac0bb1985907841666b98608653d4eca` |
| `LEGI_20250712-211706.tar.gz` | 961 478 | `3431992de9da3d8358be410273ff4874f46fc191cc62d4e92319856547ef7adb` |
| `LEGI_20250713-205013.tar.gz` | 929 133 | `dc091d7a66e7a04a6aef1975f8d73de7d70f07edb8f47b256bb0eeffa4d26d90` |
| `LEGI_20260223-224003.tar.gz` | 43 561 156 | `0fc3e170d18f3058a64c4de69ea787289c99355841234646bf5339d0d64df82c` |
| `LEGI_20261021-133354.tar.gz` | 174 220 | `75b3e74f04a38f99480d0f4b27575c7a1839c626ea894e4255ee9f8dd830622e` |
| `LEGI_20261028-212713.tar.gz` | 4 915 959 | `2daf08951a6928497d6b827961971447f8f94e266339aa6e893eadace9197ff2` |

- Les archives et les listings sont restés hors du dépôt (`/var/tmp/lf-fin/dila/`), traités comme des données non fiables (scripts Python en mode isolé `-I`, aucune extraction complète). Les mesures sur les 5,25 millions d'entrées viennent d'un parcours en flux de l'archive. Le dépôt ne contient que des extraits (les fixtures, §11).

---

## 3. Volumétrie (V-185)

### 3.1 Le serveur

- Index `https://echanges.dila.gouv.fr/OPENDATA/LEGI/` : page HTML 3.2 d'Apache (`autoindex`), `Content-Type: text/html;charset=ISO-8859-1`, 76 211 octets, 482 entrées (1 global, 480 incréments, la fiche PDF de présentation). Les dates du listing sont à l'heure de Paris, les tailles sont arrondies (`K`, `M`, `G`) : les octets exacts demandent un `HEAD` par archive. MESURÉ.
- Les archives répondent `Content-Type: application/x-gzip`, `Accept-Ranges: bytes`, `Last-Modified` et `ETag` stables ; une requête `Range: bytes=0-99` rend `206`, un `Range` au-delà de la fin rend `416` (corps HTML). La reprise sur interruption est donc possible. MESURÉ.
- Le répertoire racine `/OPENDATA/` liste 36 sous-répertoires (dont ACCO, CAPP, CASS, CNIL, CONSTIT, DOLE, INCA, JADE, JORF, KALI, LEGI, SARDE, `DTD_LEGIFRANCE`). MESURÉ.

### 3.2 Le global

| Mesure | Valeur |
|---|---|
| Nom, date de modification | `Freemium_legi_global_20250713-140000.tar.gz`, `Last-Modified: Sun, 13 Jul 2025 08:14:21 GMT` |
| Taille compressée | 1 171 027 316 octets (1,09 Gio) |
| Entrées du TAR | 5 253 903 : 2 690 016 dossiers, 2 557 045 fichiers (dont 95 vides), 6 842 liens symboliques |
| Contenu non compressé (fichiers) | 9 511 095 449 octets (8,86 Gio) ; taille moyenne 3 720 octets ; 77,8 % des fichiers font 4 Kio ou moins |
| Occupation estimée sur disque (blocs de 4 Kio) | 25,9 Go : 14,9 Go pour les fichiers et 11,0 Go pour les 2,69 millions de dossiers (CALCUL) |
| Plus gros fichier | 2 872 940 octets (un `ARTICLE` de `TNC_en_vigueur`) ; plus gros `TEXTE_VERSION` 1 066 867 octets |
| Chemin le plus long, profondeur max | 159 caractères ; 20 séparateurs `/` (21 composantes) |
| Téléchargement | 205,9 s en une connexion, 5,69 Mo/s |
| `tar -tz` seul (décompression et lecture des en-têtes) | 60,4 s, 4 Mo de mémoire résidente |
| Deux parcours Python monothreads (indicatifs) | 347 s pour les comptages sur tous les membres ; 405 s pour les statistiques de balises sur 2 276 109 fichiers XML |

Conséquence : extraire l'archive coûterait environ 26 Go et 5,25 millions d'inodes. L'ingestion se fait en flux (`flate2` → `tar` → `quick-xml`), sans écriture sur disque. Le parcours de mesure a gardé tous les identifiants en mémoire (4,2 Gio de pointe) : l'adapter ne doit pas faire de même.

Répartition des fichiers par type de racine (MESURÉ) :

| Racine | Fichiers | Remarque |
|---|---|---|
| `ARTICLE` | 1 750 418 | 1 737 280 identifiants distincts |
| `SECTION_TA` | 236 589 | 234 979 identifiants distincts |
| `TEXTELR` | 144 551 | 144 054 identifiants distincts |
| `TEXTE_VERSION` | 144 551 | idem |
| `VERSIONS` (arbre `eli/`) | 276 088 | |
| `ID` (arbre `eli/`) | 4 753 | |
| fichiers vides (arbre `eli/`) | 95 | |

Taille par catégorie, les cinq premières : `TNC_en_vigueur` articles 904 887 fichiers et 3,71 Go ; `code_en_vigueur` articles 488 906 fichiers et 3,07 Go ; `TNC_non_vigueur` articles 339 805 fichiers et 1,43 Go ; `TNC_en_vigueur` `texte/version` 114 098 fichiers et 0,38 Go ; `code_en_vigueur` `section_ta` 102 226 fichiers et 0,23 Go. L'arbre `eli/` pèse 280 936 fichiers mais environ 40 Mo (0,4 % des octets).

### 3.3 Les incréments LEGI

Mesure par `HEAD` sur les 480 archives (octets exacts) :

| Mesure | Valeur |
|---|---|
| Nombre, période | 480, du `LEGI_20250712-211706` au `LEGI_20261028-212713` |
| Total | 2 050 189 766 octets (1 955 Mio, 2,05 Go) ; dont 2 049 228 288 pour les 479 postérieurs au global |
| Plus petit, plus grand | 5 408 octets (`LEGI_20260601-104919`) ; 43 561 156 octets (`LEGI_20260223-224003`, 82 203 entrées) |
| Médiane, moyenne | 2 920 559 octets ; 4 271 229 octets |
| 90e, 99e centile | 8 861 582 octets ; 23 723 217 octets |
| Archives de moins de 2 Kio | 0 (le « vide < 200 octets » de l'annexe C §7.3 n'a pas été vu) |

Total par mois (Mio, arrondis) : 2025-07 64 (19 archives) ; 08 112 ; 09 100 ; 10 79 ; 11 119 ; 12 207 ; 2026-01 133 ; 02 179 ; 03 93 ; 04 120 ; 05 88 ; 06 159 ; 07 185 ; 08 148 ; 09 89 ; 10 78 (jusqu'au 28).

### 3.4 Les autres fonds (taille du listing, arrondie)

| Fonds | Global | Incréments | Total des incréments | Dernier incrément |
|---|---|---|---|---|
| LEGI | 1,1 Go (exact : 1 171 027 316 octets) | 480 | 1 957 Mio (exact : 1 955) | 2026-10-28 |
| JORF | 1,6 Go | 873 | 2 273 Mio | 2026-10-29 |
| JADE | 1,2 Go | 358 | 234 Mio | 2026-10-28 |
| KALI | 173 Mo | 266 | 98 Mio | 2026-10-28 |
| INCA | 655 Mo | 60 | 9 Mio | 2026-10-16 |
| CASS | 248 Mo | 56 | 11 Mio | 2026-09-30 |
| CAPP | 279 Mo | 21 | moins de 1 Mio | 2026-10-28 |
| CNIL | 18 Mo | 146 | 2 Mio | 2026-10-28 |
| CONSTIT | 12 Mo | 23 | 2 Mio | 2026-10-19 |
| DOLE | 18 Mo | 275 | 15 Mio | 2026-10-28 |
| ACCO | 45 Go | 63 | 13 811 Mio | 2026-10-19 |

Les onze fonds ont un global de même date, `20250713-140000` : le global est produit en une seule opération pour tous les fonds. ACCO (accords d'entreprise) pèse à lui seul 45 Go de global.

### 3.5 Budgets pour M9 (CALCUL)

- Premier amorçage de LEGI : télécharger 1,17 Go (environ 3,5 min à 5,7 Mo/s) puis 479 incréments pour 2,05 Go (environ 6 min), soit environ 3,2 Go et 10 minutes de réseau, plus le temps de décompression et d'analyse (non mesuré en Rust).
- Disque : prévoir les archives conservées (3,2 Go) ou la seule base. Pour la base, des tiers rapportent 3,8 Go pour LEGI complet avec 1 780 693 versions d'articles (annexe C §7.9, TIERS), cohérent avec les 1 750 418 articles du global.
- Mémoire : le plus gros membre fait 2,9 Mo ; une borne de 16 Mio par membre est très large. Aucun besoin de lire plusieurs membres à la fois, sauf les deux listes de suppression (§6).

---

## 4. Rythme de publication (V-186)

### 4.1 Les globaux

- Un seul global est listé : `Freemium_legi_global_20250713-140000.tar.gz`. Le serveur ne conserve pas les globaux antérieurs : l'historique du rythme n'est pas observable. MESURÉ.
- La fiche LEGI (2017-08-24) dit : « Régulièrement, et au minimum une fois par an, un nouveau stock est constitué : il contient les données du stock précédent et les modifications apportées […]. L'utilisateur qui souhaite intégrer et/ou rediffuser les données pour la première fois s'oblige à utiliser le dernier stock mis à disposition. » DOC DILA.
- Au 2026-10-29, le global a 473 jours (15,5 mois) : l'engagement « au moins une fois par an » n'est pas tenu. Le « environ deux fois par an » de l'annexe C §7.3 n'est pas confirmé non plus. Il ne faut donc pas compter sur un nouveau global ; si un global plus récent apparaît, c'est un événement à détecter à chaque exécution (le nom change) et qui impose de tout reconstruire (annexe C §7.4-2).
- Le nom du global n'est pas son horodatage réel : `140000` dans le nom, mais `Last-Modified` à 08:14:21 GMT, soit 10:14 à Paris. La date de nom sert de coupure : un incrément s'applique s'il est postérieur au nom du global.

### 4.2 Les incréments

- 480 archives sur 474 jours (2025-07-12 au 2026-10-28), 467 jours distincts. Les 7 jours sans archive : 2025-07-14, 2025-08-07, 2025-08-16, 2025-11-02, 2025-12-25, 2026-04-06 et 2026-05-09. 11 jours ont plus d'une archive (au plus 4 le 2026-10-21, dont 3 hors horaire à 12 h 29, 13 h 04 et 13 h 33). MESURÉ.
- Répartition par jour de la semaine (lundi à dimanche) : 68, 69, 73, 68, 68, 66, 68. Les publications sont **quotidiennes, week-ends compris** : « à la fin de chaque jour ouvré » (annexe C §7.3, tiers) est inexact.
- Heure du nom : 21 h pour 349 archives sur 480, 20 h pour 63, 22 h pour 52, 13 h pour 5, et 11 archives à d'autres heures (7 h, 10 h à 12 h, 23 h). L'heure est celle de Paris (heure d'hiver ou d'été) : pour l'archive du 2026-10-28, nom `212713`, `Last-Modified` 20:31:40 GMT soit 21:31:40 à Paris. Sur les 480 archives, la fin d'écriture (`Last-Modified`, converti en heure de Paris) suit l'heure du nom de 0,7 à 12,5 minutes (médiane 5,1) : aucun écart négatif, aucun supérieur à 15 minutes, en heure d'hiver comme en heure d'été, ce qui confirme le fuseau.
- Fiche : « mises à jour au plus tard dans les 3 jours ouvrés après publication au JORF, sauf textes particulièrement volumineux » (DOC DILA).
- L'ordre lexicographique des noms est l'ordre chronologique (champs de largeur fixe) : trier les noms suffit, sans conversion de fuseau.

### 4.3 Le recoupement global et incréments

- Un incrément **antérieur** au nom du global existe encore au listing : `LEGI_20250712-211706` ; ses 2 564 fichiers sont tous dans le global avec la même taille. Il est inutile.
- Le premier incrément postérieur, `LEGI_20250713-205013`, recoupe le global : sur ses 3 516 fichiers, 2 992 sont dans le global (dont 2 697 de même taille). Plus tard, la proportion de fichiers déjà présents reste forte : 37 361 sur 40 541 le 2026-02-23 (24 560 de même taille) et 1 967 sur 2 483 le 2026-10-28 (614 de même taille) ; aucun n'a la même date de modification (à la minute près) que sa copie du global, y compris quand la taille est identique. Appliquer un incrément n'est donc pas « n'ajouter que du nouveau » : il **remplace** des fichiers par chemin. Un incrément doit être idempotent. 479 incréments sur 480 sont postérieurs au global (CALCUL à partir du nom).

---

## 5. Arborescence, format et balises (V-187)

### 5.1 Le conteneur TAR

- Format GNU tar (magic `ustar  \0`), noms de plus de 100 caractères portés par des entrées `L` de GNU (près de la moitié des en-têtes lus en tête d'archive), aucun en-tête PAX (`pax_members` 0). Le crate `tar` lit ce format ; il faut utiliser le chemin en octets (`path_bytes`) plutôt que le décoder d'office. MESURÉ.
- Aucun nom de membre invalide en UTF-8 dans le global (l'annexe C §7.4-6 parle de noms en octets non UTF-8, observés chez un tiers : pas dans LEGI).
- Le global commence par `legi/` puis `legi/global/` ; **ordre des groupes** : `code_et_TNC_non_vigueur` (entrées 3 à 949 209), puis `code_et_TNC_en_vigueur` (949 210 à 4 622 752), puis `eli` (4 622 753 à 5 253 903). Les copies `non_vigueur` précèdent donc leurs doublons `en_vigueur` dans le flux.
- Un incrément a un préfixe `AAAAMMJJ-HHMMSS/` (un dossier) et la même arborescence en dessous. Il commence, s'il y en a, par `liste_suppression_legi.dat` et `liste_suppression_legi_dossier.dat`, **avant** tout contenu (§6).
- Les incréments peuvent contenir l'arbre `eli/` et des liens symboliques : `LEGI_20250712` en a 3 846 entrées et 40 liens, `LEGI_20250713` 5 903 et 57, `LEGI_20260223` 39 188 et 262 ; `LEGI_20261021-133354` et `LEGI_20261028` n'en ont aucun. Cinq incréments seulement ont été regardés : l'absence d'`eli/` dans un incrément n'est pas garantie, sa présence non plus.

### 5.2 Arborescence réelle

```text
[<AAAAMMJJ-HHMMSS>/]legi/global/
  code_et_TNC_(en|non)_vigueur/(code|TNC)_(en|non)_vigueur/(JORF|LEGI)/TEXT/NN/NN/NN/NN/NN/<CID>/
      article/LEGI/ARTI/NN/NN/NN/NN/NN/<LEGIARTI000000000000>.xml
      section_ta/LEGI/SCTA/NN/NN/NN/NN/NN/<LEGISCTA000000000000>.xml
      texte/struct/<LEGITEXT000000000000>.xml
      texte/version/<LEGITEXT000000000000>.xml
  eli/<nature>/AAAA/M/J/<référence>/...        (fichiers versions.xml, liens symboliques)
[<AAAAMMJJ-HHMMSS>/liste_suppression_legi.dat et liste_suppression_legi_dossier.dat]
```

- `NN` = deux chiffres, cinq niveaux (les dix premiers des douze chiffres de l'identifiant, par paires : `LEGIARTI000006842411` est sous `00/00/06/84/24`) ; `<CID>` = `JORFTEXT` ou `LEGITEXT` plus 12 chiffres (20 caractères, comme tous les identifiants). Les dossiers `code_*_vigueur` ne contiennent que des `LEGI/TEXT` ; les `TNC_*_vigueur` contiennent surtout des `JORF/TEXT` (894 997 articles sous `en_vigueur`, 320 177 sous `non_vigueur`) et aussi des `LEGI/TEXT` (9 890 et 19 628 articles).
- Positions des segments pour un chemin du global sans préfixe : `parts[11]` = CID, `parts[12]` = `article`, `section_ta` ou `texte` ; pour `texte`, `parts[13]` = `struct` ou `version`. Avec le préfixe d'incrément, tout est décalé de 1.
- Les fichiers `texte/struct/…` et `texte/version/…` ne sont **pas** rangés dans un sous-arbre `NN/NN/…`, contrairement aux articles et aux sections : deux fichiers de même nom existent par texte, dans deux dossiers.
- `code_en_vigueur` contient 80 codes, `code_non_vigueur` 31 (111 textes de nature `CODE` au total).
- Dans `eli/`, mois et jour ne sont pas complétés par un zéro (`eli/arrete/2025/4/11/…`). Exemples réels : `eli/jo/2013/10/30/versions.xml` (racine `ID`, 69 octets) ; `eli/arrete/2025/4/11/TECM2419022A/jo/article_1/versions.xml` (racine `VERSIONS`, 143 octets) ; le lien `eli/loi_organique/2004/2/27/2004-192 -> DOMX0300085L`. L'arbre `eli/` n'est pas nécessaire au corpus de textes (il ne contient que des index de versions et des identifiants) : les 95 fichiers vides y sont tous (`eli/ordonnance/2023/8/31/ECOT2316929P/jo/texte/versions.xml`, par exemple).

### 5.3 Les quatre racines de LEGI

Les balises de l'annexe C §7.6 sont confirmées sur des fichiers réels. Exemple d'`ARTICLE` (`LEGIARTI000006842411`, extrait condensé : les vrais fichiers ont des lignes vides entre les balises de métadonnées) :

```xml
<ARTICLE>
<META><META_COMMUN><ID>LEGIARTI000006842411</ID><ANCIEN_ID>UHAXXXXXXXX5X001000AXXAA</ANCIEN_ID><ORIGINE>LEGI</ORIGINE>
  <URL>article/LEGI/ARTI/00/00/06/84/24/LEGIARTI000006842411.xml</URL><NATURE>Article</NATURE></META_COMMUN>
<META_SPEC><META_ARTICLE><NUM>1</NUM><ETAT>VIGUEUR</ETAT><DATE_DEBUT>1980-03-25</DATE_DEBUT>
  <DATE_FIN>2999-01-01</DATE_FIN><TYPE>AUTONOME</TYPE></META_ARTICLE></META_SPEC></META>
<CONTEXTE><TEXTE cid="LEGITEXT000006074232" nature="CODE" ...><TITRE_TXT ...>...</TITRE_TXT><TM><TITRE_TM id="LEGISCTA000006108694" ...>...</TITRE_TM></TM></TEXTE></CONTEXTE>
<VERSIONS><VERSION etat="VIGUEUR"><LIEN_ART debut="1980-03-25" etat="VIGUEUR" fin="2999-01-01" id="LEGIARTI000006842411" num="1" origine="LEGI"/></VERSION></VERSIONS>
<NOTA><CONTENU/></NOTA><BLOC_TEXTUEL><CONTENU><p>…</p><br/></CONTENU></BLOC_TEXTUEL>
<LIENS><LIEN cidtexte="JORFTEXT000000519770" datesignatexte="1980-03-20" id="JORFTEXT000000519770" naturetexte="DECRET" sens="source" typelien="CODIFICATION">Décret 80-217 1980-03-20</LIEN></LIENS>
</ARTICLE>
```

Le `TEXTELR` porte `META/META_SPEC/META_TEXTE_CHRONICLE` (`CID`, `NUM`, `NUM_SEQUENCE`, `NOR`, `DATE_PUBLI`, `DATE_TEXTE`, `DERNIERE_MODIFICATION`, `VERSIONS_A_VENIR`, …), `VERSIONS/VERSION/LIEN_TXT` et `STRUCT/LIEN_SECTION_TA` (`cid`, `debut`, `etat`, `fin`, `id`, `niv`, `url`). Le `SECTION_TA` porte `ID`, `TITRE_TA`, `STRUCTURE_TA/LIEN_ART` et `CONTEXTE`. `VERSIONS_A_VENIR` est présent dans 363 textes.

### 5.4 Doublons entre `en_vigueur` et `non_vigueur`

Dans le global : 13 138 identifiants d'articles, 1 610 de sections, 497 de textes (`texte/struct` et `texte/version` chacun) existent dans les deux répertoires, toujours en deux exemplaires. Les deux exemplaires ont la même taille pour 2 487 des 13 138 articles (19 %), sans que le contenu ait été comparé octet à octet sur l'ensemble (il est identique pour `LEGIARTI000006284410`, différent pour la section `LEGISCTA000021023789`) ; les dates de modification diffèrent. La copie la plus récente est tantôt celle de `non_vigueur` (l'article `LEGIARTI000006284410`, 2023-12-31 contre 2023-11-21), tantôt celle de `en_vigueur` (la section `LEGISCTA000047818200`). Règle de legi.py (annexe C §7.4-5) : garder la plus récente par date de modification ; elle reste valable. Les deux cas sont dans les fixtures.

### 5.5 Valeurs observées

Articles (1 750 418) :

| Balise | Valeurs et effectifs |
|---|---|
| `ORIGINE`, `NATURE` | `LEGI` et `Article`, partout |
| `ETAT` | `VIGUEUR` 655 162 ; `MODIFIE` 445 870 ; `ABROGE` 363 151 ; `PERIME` 19 792 ; `TRANSFERE` 15 151 ; `ABROGE_DIFF` 9 860 ; `MODIFIE_MORT_NE` 6 949 ; `VIGUEUR_DIFF` 2 477 ; `ANNULE` 1 963 ; `DISJOINT` 84 ; `DEPLACE` 18 ; **vide** (`<ETAT/>`) 229 941 |
| `TYPE` | `AUTONOME` 1 510 111 ; `ENTIEREMENT_MODIF` 229 955 ; `PARTIELLEMENT_MODIF` 10 289 ; absent 63 |
| `NUM` | vide (`<NUM/>`) dans 25 032 articles ; des valeurs non numériques existent (`D510-1`, `Annexe IV`, `1419 à 1446`, `TABLEAU ANNEXE`) |
| `DATE_FIN` | `2999-01-01` 887 771 ; `2222-02-22` 829 |
| `DATE_DEBUT` | `2222-02-22` 446 ; `2999-01-01` pour les `ENTIEREMENT_MODIF` |

`TEXTE_VERSION` (144 551) : `ETAT` `VIGUEUR` 110 791, `ABROGE` 25 389, `MODIFIE` 3 814, `PERIME` 3 601, `ABROGE_DIFF` 626, `ANNULE` 211, `VIGUEUR_DIFF` 89, `MODIFIE_MORT_NE` 30. Le `TEXTELR` n'a pas d'`ETAT`. `NATURE` : `ARRETE` 83 936, `DECRET` 55 130, `LOI` 3 619, `ORDONNANCE` 1 472, `CODE` 111, `LOI_ORGANIQUE` 111, `DECRET_LOI` 33, `DELIBERATION` 38, `ACCORD_FONCTION_PUBLIQUE` 27, `DECISION` 25, `AVIS` 22, `LOI_CONSTIT` 19, `CONSTITUTION` 3, `CONVENTION`, `DECLARATION` et `CIRCULAIRE` 1 chacune, absente 2. Les arrêtés sont 58 % des textes.

Liens (`LIEN`) : `sens` vaut `cible` (4 214 011) ou `source` (3 912 014). `typelien` prend **31 valeurs** : `CITATION` 4 485 469, `MODIFIE` 620 308, `ABROGE` 508 270, `MODIFICATION` 385 914, `CODIFICATION` 383 894, `CONCORDANCE` 351 833, `CREE` 285 984, `TXT_SOURCE` 258 795, `ABROGATION` 236 925, `CONCORDE` 225 875, `CREATION` 175 725, `SPEC_APPLI` 86 769, `TXT_ASSOCIE` 46 601, `TRANSFERE` 17 688, `HISTO` 14 488, `DEPLACE` 13 292, `TRANSFERT` 10 556, `PEREMPTION` 6 946, `PILOTE_SUIVEUR` 4 836, `ANNULATION` 1 877, `RECTIFICATION` 1 539, `PERIME` 1 075, `APPLICATION` 414, `DEPLACEMENT` 374, `RATIFICATION` 261, `RATIFIE` 97, `DISJONCTION` 77, `DISJOINT` 48, `ETEND` 43, `CODIFIE` 29, `ANNULE` 23. Neuf d'entre elles n'ont pas de paire dans la table d'inversion de l'annexe C §7.6 : `APPLICATION`, `CITATION`, `ETEND`, `HISTO`, `PILOTE_SUIVEUR`, `RECTIFICATION`, `SPEC_APPLI`, `TXT_ASSOCIE`, `TXT_SOURCE`.

### 5.6 Dates et états

- `2999-01-01` = pas de fin. `2222-02-22` apparaît comme début (446 articles ; état `VIGUEUR_DIFF` dans les deux fixtures) et comme fin (829 articles ; état `ABROGE_DIFF` dans la fixture) : entrée en vigueur ou abrogation différée à une date indéterminée (annexe C §7.8, confirmée).
- Les 229 955 articles `ENTIEREMENT_MODIF` n'ont pas d'état et portent `DATE_DEBUT` = `DATE_FIN` = `2999-01-01` (ou `DATE_DEBUT` seul à `2999-01-01` avec une vraie date de fin). La règle `debut <= D < fin` les exclut naturellement.
- **Fin avant début** : 6 960 articles ; l'état `MODIFIE_MORT_NE` : 6 949. Les deux critères ne coïncident pas (un article `ENTIEREMENT_MODIF` à `DATE_DEBUT` `2999-01-01` et fin réelle a une fin avant début sans être `MODIFIE_MORT_NE`). Détecter le mort-né par les dates, pas par l'état.
- **Le répertoire n'est pas l'état.** Articles par répertoire et par état : sous `en_vigueur` (1 393 793) : `VIGUEUR` 654 849, `MODIFIE` 374 300, `ABROGE` 109 530, état vide 219 834, `TRANSFERE` 13 793 ; sous `non_vigueur` (356 625) : `ABROGE` 253 621, `MODIFIE` 71 570, `PERIME` 16 524 mais aussi 313 `VIGUEUR` et 587 `VIGUEUR_DIFF`. Seuls 47 % des articles du répertoire `en_vigueur` sont `VIGUEUR`.
- L'exemple réel de la fixture (§11) montre le mécanisme : dans l'incrément du 2026-10-28, les articles `LEGIARTI000024586531` et `LEGIARTI000030860648` passent de `VIGUEUR` à `ABROGE` avec une `DATE_FIN` au 2026-10-29, le lendemain de la date de l'archive, et leur dossier passe de `TNC_en_vigueur` à `TNC_non_vigueur`. Un incrément publié le soir de J anticipe donc les effets de J+1.

### 5.7 Le contenu HTML

Mesure sur les 2 276 109 fichiers à racines `ARTICLE`, `SECTION_TA`, `TEXTELR` et `TEXTE_VERSION` :

| Particularité | Fichiers |
|---|---|
| Lignes vides multiples (retours à la ligne ajoutés) | 1 895 191 (83 %) |
| Tableau HTML `<table>` | 54 045 |
| Espace insécable U+00A0 | 24 863 |
| Caractères de format invisibles (catégorie Cf, par exemple U+200E) | 8 701 |
| Césure molle U+00AD | 858 |
| Caractère de remplacement U+FFFD déjà présent à la source | 12 |
| `&lt;` littéral | 2 197 |

Références numériques de caractères (occurrences, hors les cinq entités prédéfinies) : `&#13;` 147 303, `&#10;` 65 456, `&#8232;` (U+2028) 70, et 403 références à des contrôles C1 (U+0080 à U+009F, 16 valeurs distinctes dont `&#156;` 171 fois et `&#129;` 118 fois ; `&#156;` est probablement un « œ » de la page de code Windows-1252 pris pour un code Unicode). Elles sont légales en XML 1.0 mais produisent de vrais caractères de contrôle : à nettoyer avant toute sortie dans un journal en lignes ou une trame JSON.

Les lignes vides s'expliquent par la note de migration du 2023-10-04 de la DILA (« Modifications dans les xml mis à disposition en open data »), dont les exemples avant et après montrent quatre changements : ordre des éléments (par exemple `SYNDICATS`), ajout de retours à la ligne, résolution des liens dans les contenus et modifications dans les liens. DOC DILA ; la note n'est faite que d'extraits XML, sans texte explicatif.

### 5.8 Les autres fonds (échantillon d'un incrément récent chacun)

| Fonds | Racines vues | Arborescence (après le préfixe) |
|---|---|---|
| CASS, INCA, CAPP | `TEXTE_JURI_JUDI` | `juri/<fonds>/global/[civile\|sociale\|criminelle\|commerciale/]JURI/TEXT/N/N/N/N/N` |
| JADE | `TEXTE_JURI_ADMIN` | `jade/global/(inedit\|publie)/CETA/TEXT/N/N/N/N/N` |
| CONSTIT | `TEXTE_JURI_CONSTIT` | `constit/global/CONS/TEXT/N/N/N/N/N` |
| CNIL | `TEXTE_CNIL` | `cnil/global/CNIL/TEXT/N/N/N/N/N` |
| DOLE | `DOSSIER_LEGISLATIF` | `dole/global/JORF/DOLE/N/N/N/N/N` |
| JORF | `ARTICLE`, `TEXTELR`, `TEXTE_VERSION`, `SECTION_TA`, `JO` | `jorf/global/{article,section_ta,texte/struct,texte/version}/JORF/{ARTI,SCTA,TEXT}/N/N/N/N/N` |
| KALI | `IDCC`, `TEXTEKALI`, `TEXTE_VERSION`, `SECTION_TA`, `ARTICLE` | `kali/global/{article,section_ta,texte/struct,texte/version}/KALI/{ARTI,SCTA,TEXT}/N/N/N/N/N` |

Ces neuf incréments sont en UTF-8, sans liste de suppression. La structure de LEGI n'est donc pas celle des autres fonds : ne pas généraliser le parcours (annexe C §7.11-6).

---

## 6. Les fichiers de suppression

Forme (MESURÉ sur les incréments du 2026-02-23 et du 2026-10-28) :

- `liste_suppression_legi.dat` : un chemin par ligne, séparateur LF seul (pas de CR), dernière ligne terminée par LF, ASCII pur, aucun espace interne, aucun doublon. Le chemin est celui du membre **sans le préfixe d'incrément et sans le suffixe `.xml`** : `legi/global/code_et_TNC_en_vigueur/TNC_en_vigueur/JORF/TEXT/00/00/24/58/38/JORFTEXT000024583863/article/LEGI/ARTI/00/00/30/86/06/LEGIARTI000030860648`. L'annexe C §7.4-4 parle de chemins « séparés par des espaces » : ils sont séparés par des sauts de ligne.
- `liste_suppression_legi_dossier.dat` : une ligne `<chemin du dossier de texte> D`, par exemple `legi/global/code_et_TNC_en_vigueur/TNC_en_vigueur/JORF/TEXT/00/00/24/58/38/JORFTEXT000024583863 D`.
- Les deux fichiers sont les premiers de l'archive, avant `legi/`. Ils sont facultatifs : trois des cinq incréments téléchargés n'en ont pas (`20250712`, `20250713`, `20261021-133354`).

| Incrément | Fichiers listés | Dossiers listés | Déplacements | Suppressions réelles |
|---|---|---|---|---|
| 2026-10-28 | 58 | 3 | 57 (même identifiant ré-ajouté sous `non_vigueur`) | 1 (`LEGIARTI000055022598`, code `LEGITEXT000006069577`) |
| 2026-02-23 | 81 | 4 (dont 1 sous `TNC_non_vigueur`) | 27 | 54 (37 articles de `LEGITEXT000006069577` ; 17 fichiers de deux textes `JORFTEXT000053523844` et `JORFTEXT000053523855`, hors de tout dossier `D`) |

Mesure de recoupement : dans l'incrément du 2026-10-28, aucun chemin supprimé ne figure comme contenu de la même archive (le déplacement change de répertoire), et les trois dossiers supprimés (`JORFTEXT000024583863`, `JORFTEXT000022415217`, `JORFTEXT000000349118`, 6, 32 et 38 fichiers dans le global) sont ré-ajoutés sous `TNC_non_vigueur` (16 176, 96 640 et 157 656 octets). Le même constat vaut pour le 2026-02-23 (aucun chemin supprimé n'est présent à l'identique dans l'archive). Les fichiers `texte/struct` et `texte/version` d'un texte portent le même nom `LEGITEXT…xml` dans deux dossiers : une clé réduite au nom de fichier les confondrait aussi.

La liste de fichiers n'énumère pas tous les fichiers d'un dossier `D` : le 2026-10-28, 57 des 58 chemins listés sont sous l'un des trois dossiers `D` (le 58e, `LEGIARTI000055022598`, est la suppression réelle) mais 19 des 76 fichiers que le global contient sous ces dossiers ne sont pas listés (1 sur 6, 4 sur 32, 14 sur 38). Exemple : `LEGIARTI000024586527`, absent de la liste, est présent sous `TNC_en_vigueur` au global et sous `TNC_non_vigueur` dans l'incrément. Le 2026-02-23, 27 chemins listés sont sous un dossier `D` et 54 hors de tout dossier `D` ; deux des quatre dossiers (`JORFTEXT000051429850`, 7 fichiers, et `JORFTEXT000036526027`, 1 fichier) n'ont aucun fichier listé.

Sémantique à respecter :

1. La suppression vise un **chemin**, jamais un identifiant seul. Une suppression suivie d'un ajout sous un autre chemin ne doit pas effacer l'ajout.
2. Les listes se lisent d'abord (elles sont en tête de flux) mais s'appliquent après le contenu, comme le fait legi.py (annexe C §7.4-4). Cela reste correct parce que le chemin ré-ajouté est différent.
3. Une ligne de dossier vise le sous-arbre entier de ce dossier de texte dans ce répertoire (`article`, `section_ta`, `texte/struct`, `texte/version`), y compris des fichiers que la liste de fichiers n'énumère pas (mesure ci-dessus). C'est la lecture d'un tiers (annexe C §7.4-4 : suppression d'un répertoire `JORFTEXT` entier) ; la DILA ne la documente pas dans les pièces lues. Elle est cohérente avec la mesure : les six fichiers du premier dossier, `LEGIARTI000024586527` compris, se retrouvent sous `non_vigueur` après l'incrément. À valider sur un cas où le fichier non listé n'est pas ré-ajouté.
4. Une suppression peut viser un chemin absent du magasin (cas des fixtures, qui ne contiennent qu'un sous-ensemble) : ne pas échouer.

---

## 7. Encodage et DTD (V-188)

- **Encodage** (MESURÉ, global entier) : 2 556 950 fichiers non vides, tous avec la déclaration `<?xml version="1.0" encoding="UTF-8"?>`, tous valides en UTF-8, aucun BOM, aucun CR (`crlf_files` 0), aucun octet NUL, aucun motif de mojibake (`Ã©`, `Ã¨`, `Ã `, `â\x80`), aucun `DOCTYPE`, aucune entité nommée autre que les cinq prédéfinies. Les neuf incréments d'autres fonds sont aussi en UTF-8. Les 95 fichiers vides ne sont pas du XML.
- Conséquence : pas de transcodage ; `quick-xml` peut rester sur UTF-8 sans la feature `encoding` (à garder en réserve). **Rejeter tout `DOCTYPE` et toute déclaration d'entité** est sans coût et ferme la porte aux attaques par entités : 0 fichier réel n'en a.
- **DTD** : `DTD_LEGIFRANCE_20181017.7z` (347 255 octets ; 141 fichiers, 2,06 Mo décompressés) contient un dossier `LEGIFRANCE_20181018_dtd` (daté du 2018-10-18) avec les sous-dossiers `acco`, `cnil`, `constit`, `dole`, `jade`, `jorf`, `jorf_simple`, `juri`, `kali`, `legi` et `sarde`. Les DTD LEGI sont `legi_article.dtd`, `legi_section_ta.dtd`, `legi_texte_struct.dtd`, `legi_texte_version.dtd`, avec `lien_art.dtd`, `lien_section_ta.dtd`, `lien_txt.dtd`, `lien.dtd`, `meta_commun.dtd`, `meta_texte_chronicle.dtd`, `contenu.dtd` et le jeu XHTML. Fichiers en UTF-8 avec fins de ligne CRLF.
- **La DTD de 2018 valide encore les fichiers de 2026** : sur les 50 fichiers XML non vides retenus en fixtures (extraits du global de 2025 et de l'incrément de 2026), `xmllint --dtdvalid` ne trouve aucun écart (33 `ARTICLE`, 7 `SECTION_TA`, 5 `TEXTELR`, 5 `TEXTE_VERSION`). `VERSIONS_A_VENIR` est déjà dans `meta_texte_chronicle.dtd`. Ce n'est pas une validation de l'archive entière.
- Le répertoire `/OPENDATA/DTD_LEGIFRANCE/` (modifié le 2024-07-10) contient aussi la note de migration du 2023-10-04 et une note technique sur les NOR du JORF (2024-07-10).

---

## 8. Licences (V-189)

- Les **12 fiches de présentation** lues (LEGI, JORF, KALI, CASS, INCA, CAPP, CONSTIT, JADE, CNIL, ACCO, DOLE, CIRCULAIRES ; dates 2017-08-24 à 2018-10-18) disent chacune : « Les données sont réutilisables gratuitement sous licence ouverte v2.0. Les réutilisateurs s'obligent à mentionner : la paternité des données (DILA) ; l'URL d'accès longue de téléchargement ; le nom du fichier téléchargé ainsi que la date du fichier. » DOC DILA.
- **data.gouv.fr** (API `datasets/<slug>/`, 2026-10-29) déclare `license: fr-lo` pour LEGI, JORF, KALI, CASS, INCA, CAPP, CONSTIT, JADE, CNIL et DOLE ; ACCO n'a pas été retrouvé par la recherche. Le registre de data.gouv.fr distingue `fr-lo` (« Licence Ouverte / Open Licence », texte de 2014) de `lov2` (« Licence Ouverte / Open Licence version 2.0 »). Écart non tranché : les fiches DILA, plus anciennes, citent la v2.0 ; les métadonnées du portail pointent la version précédente. MESURÉ pour le constat, juridique pour la suite.
- Producteurs déclarés par data.gouv.fr : « Premier ministre » (LEGI, JORF, KALI, CASS, INCA, CAPP, JADE, DOLE), Conseil constitutionnel (CONSTIT), CNIL (CNIL). Fréquence déclarée de LEGI : quotidienne, couverture depuis 1945-01-01.
- Avertissement DILA sur les données personnelles (`AVERTISSEMENT-Donnees_a_caractere_personnel.pdf`) : la réutilisation d'informations publiques à données personnelles est subordonnée à la loi « Informatique et libertés » (article L322-2 du code des relations entre le public et l'administration) et ne peut avoir pour effet de réidentifier des personnes anonymisées. Peu de portée pour LEGI (textes de loi) ; réelle pour les fonds de jurisprudence et ACCO.
- Attribution à appliquer, dans le doute la plus stricte : « Source : DILA — base LEGI, Légifrance — fichier `<nom>` du `<date>` — `<URL de téléchargement>` ». C'est le champ `attribution` des sidecars des fixtures.

Décision juridique en attente : retenir `lov2` (fiches DILA) ou `fr-lo` (portail) dans le registre des licences et dans la NOTICE ; les obligations d'attribution ci-dessus couvrent les deux cas.

---

## 9. Écarts avec le plan (corrections demandées à la fusion F6)

Ces corrections portent sur des fichiers interdits à cette unité (annexe C, 09, NOTICE, README de `docs/recherche`) : elles sont transmises en demande partagée.

| Réf. | Texte actuel | Constat | Correction proposée |
|---|---|---|---|
| Annexe C §7, chapeau | « Les hôtes de la DILA étaient injoignables » | `echanges.dila.gouv.fr` et `data.gouv.fr` répondent le 2026-10-29 | Dater la remarque ; Q6 levée pour cette machine |
| §7.2 | Licence « d'après un résumé » ; V-189 « NON VÉRIFIÉ » | 12 fiches DILA lues ; écart `fr-lo` / v2.0 | Citer les fiches, consigner l'écart (§8), V-189 consignée |
| §7.3 | Dernier global 13/07/2025, « environ deux fois par an » | un seul global listé, 473 jours ; engagement DILA : au moins une fois par an | Remplacer par la mesure (§4.1), V-186 levée |
| §7.3 | « à la fin de chaque jour ouvré » | publications quotidiennes, week-ends compris ; 7 jours sans archive en 474 | Corriger (§4.2) |
| §7.3 | « archives vides (< 200 octets) possibles » | aucune archive de moins de 5 408 octets | Garder la tolérance, retirer l'affirmation de fait |
| §7.4-4 | « chemins séparés par des espaces » | un chemin par ligne (LF), sans `.xml` ; dossier : `<chemin> D` | Corriger (§6) |
| §7.4-4 | « s'appliquent après le contenu » | exact, mais les listes sont les premiers fichiers de l'archive ; 57/58 suppressions sont des déplacements | Ajouter le piège de la clé d'identifiant (§6) |
| §7.4-5 | doublons « legi.py garde le plus récent » | 13 138 articles, 1 610 sections, 497 textes dans le global lui-même | Ajouter les nombres (§5.4) |
| §7.4-6 | chemins parfois non UTF-8 | 0 dans le global LEGI | Réserver à l'observation de tiers |
| §7.5 | `AAAA/BB/CC/DD/EE/FF/GG/<CID>` à sept niveaux | `JORF\|LEGI/TEXT/NN/NN/NN/NN/NN/<CID>` à cinq niveaux ; `texte/struct` et `texte/version` sans niveaux ; arbre `eli/` absent du schéma | Remplacer par le schéma du §5.2 ; V-187 levée |
| §7.5 | « un tiers ne garde que `en_vigueur` » | le répertoire n'est pas l'état de l'article (§5.6) | Préciser |
| §7.6 | table des types de liens (11 paires) | 31 valeurs de `typelien`, dont 9 hors table | Ajouter les 9 valeurs (§5.5) |
| §7.6, V-188 | encodage « NON VÉRIFIÉ » | UTF-8 partout, sans BOM, DOCTYPE ni entité nommée | V-188 consignée (§7) |
| §7.8 | `ETAT` : `MODIFIE_MORT_NE` = fin avant début | 6 960 fin<début contre 6 949 `MODIFIE_MORT_NE` ; 229 941 articles à `ETAT` vide | Mort-né par les dates ; ajouter l'état vide |
| §7.9 | « 745-796 Mo compressée, 9-15 Go extraite (2017) » | 1 171 027 316 octets compressés, 9,51 Go de contenu, 25,9 Go sur disque | Remplacer ; V-185 levée |
| 09 §2.15 | V-185 à V-189 | voir §1 | V-185, V-186, V-187 levées ; V-188 et V-189 consignées |

---

## 10. Conséquences pour `lf-adapter-dila`

1. **Identité d'un fichier = son chemin sans préfixe et sans `.xml`.** L'identité d'un document (`LEGIARTI…`) est dérivée ; quand plusieurs chemins portent le même identifiant, retenir la copie la plus récente par date de modification du membre, et à égalité `en_vigueur`. Une suppression retire un chemin, pas un identifiant (§6).
2. **Ordre d'application** : global, puis incréments dont le nom est strictement supérieur à celui du global, dans l'ordre lexicographique des noms. Chaque incrément est idempotent (§4.3). Un nouveau nom de global déclenche la reconstruction.
3. **Flux** : `flate2` → `tar` (noms longs GNU, `path_bytes`) → `quick-xml`, sans extraction. Lire d'abord les deux listes de suppression (quelques dizaines de Kio), appliquer le contenu, puis les suppressions. Ne pas supposer que les listes existent.
4. **Tolérances** : membres vides (0 octet), liens symboliques (ne jamais les suivre), dossiers, arbre `eli/` (à ignorer pour le corpus de textes), chemins supprimés absents, `typelien`, `ETAT`, `NATURE` et `TYPE` inconnus (garder la valeur brute, ne pas échouer).
5. **Sécurité** : rejeter `DOCTYPE` et `ENTITY` ; borner la taille d'un membre (16 Mio, le maximum observé est 2,9 Mo) et la longueur des chemins (le maximum observé est 159) ; refuser un chemin avec `..` ou absolu ; ne rien écrire hors du magasin.
6. **Dates** : `2999-01-01` = fin ouverte ; `2222-02-22` = date différée indéterminée ; `debut <= D < fin`. Un article `ENTIEREMENT_MODIF` n'est jamais en vigueur. Le mort-né se détecte par fin < début, pas par l'état.
7. **Texte** : ignorer les nœuds de texte faits d'espaces dans les métadonnées (lignes vides partout) ; le contenu HTML contient des références `&#13;`, `&#10;`, U+2028 et des contrôles C1 : les nettoyer avant journal ou sortie JSON.
8. **Fraîcheur** : chaque réponse porte la date du dernier incrément appliqué et l'âge du global (473 jours au 2026-10-29) ; avertir au-delà d'un seuil à fixer.
9. **Réseau** : une connexion, `Range` pour reprendre, `HEAD` pour apprendre les tailles, User-Agent identifiant, 0,8 s ou plus entre deux requêtes ; ne pas lancer un téléchargement en parallèle sans l'avoir mesuré.
10. **Licence** : attribution complète (§8), à porter dans chaque réponse issue du dump.

---

## 11. Fixtures versionnées

Dossier `crates/lf-adapter-dila/tests/fixtures/` (Licence Ouverte) : 57 fichiers de données (55 XML ou `.dat` plus 2 `.dat` de février 2026), chacun avec un sidecar `.meta.toml`, et deux mini-archives. Tout est expliqué dans `crates/lf-adapter-dila/tests/fixtures/README.md`. Les scénarios couverts :

- le déplacement d'un dossier entre `TNC_en_vigueur` (global) et `TNC_non_vigueur` (incrément), avec ses listes de suppression ;
- le remplacement d'un fichier par un incrément (Code de l'environnement) et la chaîne de versions de l'article D510-1 ;
- un petit code complet (Code de déontologie des architectes) ;
- deux doublons `en_vigueur` / `non_vigueur` ;
- l'arbre `eli/` : fichiers `ID`, `VERSIONS`, fichier vide et lien symbolique ;
- les valeurs limites : `2222-02-22`, mort-né, `ETAT` vide, `NUM` vide, césure molle, tableau, caractère invisible, dix états d'article ;
- les listes de suppression du 2026-02-23 (suppressions réelles et dossier sous `non_vigueur`).

`build_archives.py` reconstruit les deux mini-archives et `--check` vérifie les empreintes. Les XML sont validés par DTD (§7). Aucun test Rust n'est livré par cette unité.

---

## 12. Limites et contrôle

Non mesuré ou non tranché :

- Le débit d'ingestion en Rust, le coût d'un index `tantivy` et la taille de la base finale : seules les mesures de lecture (60 s pour `tar -tz`) existent.
- Le contenu des 475 autres incréments : cinq ont été ouverts. Une structure ou une valeur nouvelle (par exemple une autre racine) pourrait y figurer.
- La tolérance du serveur à plusieurs connexions parallèles, le support de `If-Modified-Since` ou `If-None-Match` (l'`ETag` est présent) et l'éventuel plafond de débit.
- Le sens exact du classement `en_vigueur` / `non_vigueur` du répertoire (par texte ou par article) : seule la non-équivalence avec l'état est établie.
- L'historique des globaux antérieurs : le serveur n'en garde pas.
- La validation par DTD de l'archive entière (seules les fixtures ont été validées) et la licence exacte à retenir (§8).

Contrôles rapides à refaire (aucun n'écrit hors d'un répertoire de travail jetable) :

```text
curl -sS -I -A "<identifiant>" https://echanges.dila.gouv.fr/OPENDATA/LEGI/Freemium_legi_global_20250713-140000.tar.gz
    -> Content-Length: 1171027316, Last-Modified: Sun, 13 Jul 2025 08:14:21 GMT
curl -sS -A "<identifiant>" https://echanges.dila.gouv.fr/OPENDATA/LEGI/ | grep -c 'tar.gz">'
    -> nombre d'archives du listing (482 le 2026-10-29, le PDF exclu : 481)
tar -tzvf Freemium_legi_global_20250713-140000.tar.gz | wc -l
    -> 5253903
tar -tzvf LEGI_20261028-212713.tar.gz | head -3
    -> le dossier du préfixe puis les deux listes de suppression
python3 -I crates/lf-adapter-dila/tests/fixtures/build_archives.py --check
```
