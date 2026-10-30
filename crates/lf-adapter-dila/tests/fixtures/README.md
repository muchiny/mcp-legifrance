# Fixtures LEGI de `lf-adapter-dila`

Extraits RÉELS du fonds LEGI de la DILA (Licence Ouverte 2.0, voir « Licence » plus bas), prélevés le
2026-10-29 par l'unité V1-16 (tâche T-M9-01, reconnaissance des dumps). Le rapport complet, avec les
mesures sur l'archive entière, est `docs/recherche/10-dumps-dila.md`.

Aucun octet n'est inventé ni reformaté : chaque fichier XML ou `.dat` est identique, octet pour octet, au
membre de l'archive d'origine. Seuls les deux fichiers de `archives/` sont reconstruits (ré-empaquetage),
à partir de ces membres.

## Contenu

| Répertoire | Contenu |
|---|---|
| `xml/global-20250713/` | membres de `Freemium_legi_global_20250713-140000.tar.gz`, rangés par dossier de rang 4 (`code_en_vigueur`, `TNC_en_vigueur`, `TNC_non_vigueur`, `eli`) |
| `xml/incr-20261028/` | membres de `LEGI_20261028-212713.tar.gz`, même rangement |
| `suppression/incr-20261028/` | `liste_suppression_legi.dat` (58 chemins) et `liste_suppression_legi_dossier.dat` (3 dossiers) du même incrément |
| `suppression/incr-20260223/` | les deux mêmes listes de l'incrément `LEGI_20260223-224003.tar.gz` (81 chemins, 4 dossiers), hors mini-archive : 54 suppressions réelles et un dossier sous `TNC_non_vigueur` |
| `archives/` | `legi-global-mini.tar.gz` et `legi-incr-20261028-mini.tar.gz` : mini-archives à la structure des vraies (GNU tar, noms longs `L`, dossiers, lien symbolique), reconstruites par `build_archives.py` |

Chaque fichier a un sidecar `<nom>.meta.toml` voisin (même convention que les fixtures de
`lf-adapter-piste`, plan 06 §4) : `source`, `archive`, `archive_sha256`, `member` (chemin exact dans
l'archive), `member_mtime`, `captured_at`, `bytes_raw`, `sha256_raw`, `sha256_file`, `known_issues`,
`license`, `attribution`. `in_mini_archive` dit si le fichier figure dans une mini-archive.

Les noms de fichiers sont ceux des membres, avec deux exceptions pour éviter les collisions : les
`texte/struct/LEGITEXTxxx.xml` et `texte/version/LEGITEXTxxx.xml` (même nom dans deux dossiers) sont
préfixés `struct_` et `version_` ; les trois fichiers `eli/**/versions.xml` (tous nommés `versions.xml`)
reçoivent un nom aplati qui garde la date et l'identifiant.

## Scénarios couverts

1. **Déplacement entre répertoires** : le dossier `JORFTEXT000024583863` (6 fichiers) est dans
   `TNC_en_vigueur` au global et passe dans `TNC_non_vigueur` dans l'incrément du 2026-10-28, qui le
   supprime par ses deux listes (`.dat`) et le rajoute sous l'autre répertoire, mêmes identifiants. Une
   clé de stockage limitée à l'identifiant perdrait ces fichiers (rapport §6).
2. **Remplacement d'un fichier par un incrément** : `LEGITEXT000006074220` (Code de l'environnement),
   son `TEXTELR`, son `TEXTE_VERSION` et l'article `LEGIARTI000045418514` existent dans le global et dans
   l'incrément avec des octets différents.
3. **Chaîne de versions d'un article** : D510-1 du Code de l'environnement, `LEGIARTI000038247261`,
   `…043574906`, `…045418514`, `…050813430` (trois `MODIFIE`, puis `VIGUEUR` à fin `2999-01-01`), plus les
   deux `SECTION_TA` parentes.
4. **Petit code complet** : `LEGITEXT000006074232` (Code de déontologie des architectes) : structure,
   version, deux sections, trois articles.
5. **Doublons entre `en_vigueur` et `non_vigueur`** : `LEGIARTI000006284410` (mêmes octets, dates de
   modification différentes) et, hors mini-archive, `LEGISCTA000021023789` (octets différents).
6. **Arbre `eli/`** : un `ID` de 69 octets, un `VERSIONS` de 143 octets, un fichier VIDE de 0 octet (95 dans
   le global), et, dans la mini-archive du global, le lien symbolique
   `eli/loi_organique/2004/2/27/2004-192 -> DOMX0300085L` (pendant : sa cible n'est pas dans l'archive).
7. **Valeurs limites** (hors mini-archive, un fichier par cas) : `DATE_DEBUT` `2222-02-22`
   (`VIGUEUR_DIFF`) et `DATE_FIN` `2222-02-22` (`ABROGE_DIFF`) ; article mort-né (`MODIFIE_MORT_NE`, début
   et fin égaux, intervalle vide) ;
   article `ENTIEREMENT_MODIF` à `ETAT` vide (`<ETAT/>`) et dates `2999-01-01` des deux côtés ; autre
   `ENTIEREMENT_MODIF` dont `DATE_DEBUT` vaut `2999-01-01` et `DATE_FIN` une date réelle (fin avant début
   sans être `MODIFIE_MORT_NE`) ; `NUM` vide ; césure molle U+00AD ; tableau HTML ; un article pour chacun
   des états `ABROGE`, `ANNULE`, `PERIME`, `TRANSFERE`, `DISJOINT`, `DEPLACE` ; une `SECTION_TA` qui contient
   un caractère de format invisible (U+200E, marque gauche-droite).

Les 50 fichiers XML non vides ont été validés contre les DTD LEGI de 2018 (`xmllint --dtdvalid` :
33 `ARTICLE`, 7 `SECTION_TA`, 5 `TEXTELR`, 5 `TEXTE_VERSION`, aucun écart). Les trois fichiers `eli/` n'ont
pas de DTD ; deux sont bien formés, le troisième est vide.

## Reconstruire ou vérifier les archives

```text
python3 -I build_archives.py          # régénère archives/*.tar.gz et leurs .meta.toml
python3 -I build_archives.py --check  # vérifie les sha256 des fixtures et le TAR des archives
```

Python 3.11 ou plus, bibliothèque standard seule. Le TAR (non compressé) est déterministe et son sha256
(`sha256_tar`) est dans le sidecar de l'archive ; le sha256 du `.tar.gz` dépend de l'implémentation de zlib
et n'est qu'indicatif.

## À ne pas faire

- Ne pas modifier un fichier de `xml/` ou de `suppression/` : le sha256 du sidecar ne correspondrait plus.
  Pour un nouveau cas, prélever un nouveau membre et générer son sidecar.
- Ne pas ajouter de capture de l'archive complète (1,1 Go) : seuls des extraits sont versionnés.

## Licence

Données : Licence Ouverte 2.0 selon la fiche DILA (data.gouv.fr déclare l'identifiant `fr-lo`, Licence Ouverte
de 2014 : écart consigné au rapport §8), source DILA, base LEGI (Légifrance). La fiche de présentation LEGI de la DILA
demande de mentionner la paternité (DILA), l'URL d'accès longue de téléchargement, le nom du fichier
téléchargé et sa date ; c'est ce que contient le champ `attribution` de chaque sidecar. Le code
(`build_archives.py`, ce fichier) est sous la licence du dépôt.
