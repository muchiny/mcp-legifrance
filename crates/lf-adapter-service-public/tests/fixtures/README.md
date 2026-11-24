# Entrées réelles des archives Service-Public, enregistrées

Ces fichiers sont des entrées de `vosdroits-latest.zip`, l'archive quotidienne du flux 3.5 de la
DILA, gardées telles quelles (aucun octet modifié) pour les tests du domaine et de
`lf-adapter-service-public` (ADR-0033). Ils ne contiennent aucun secret : le serveur est ouvert,
sans compte ni clé.

- Adresses : `https://lecomarquage.service-public.gouv.fr/vdd/3.5/part/zip/vosdroits-latest.zip`
  (Particuliers) et `https://lecomarquage.service-public.gouv.fr/vdd/3.5/pro/zip/vosdroits-latest.zip`
  (Entreprendre).
- Date de l'enregistrement : archives téléchargées le 2026-11-23, entrées datées du 2026-11-22.
  Les mêmes fiches, lues une à une sous `/vdd/3.5/{part,pro}/xml/` le 2026-11-23, étaient
  identiques à l'octet ; seul `menu.xml` diffère, et c'est celui de l'archive qui est gardé.
- Éditeur : Direction de l'information légale et administrative (DILA).
- Licence : Licence Ouverte 2.0 (Etalab). Mentions : « Source : Service-Public.gouv.fr / DILA —
  Licence Ouverte 2.0 » pour `part`, « Source : Entreprendre.Service-Public.gouv.fr / DILA —
  Licence Ouverte 2.0 » pour `pro`.
- Débit de l'enregistrement : deux téléchargements le 2026-11-23, deux requêtes `HEAD` le
  2026-11-24 (en-têtes ci-dessous), une par seconde au plus ; aucun refus, aucun cookie.

Le flux change chaque jour : ces fichiers sont une photographie du 2026-11-22, pas l'état courant
des fiches. Ne pas les éditer à la main.

## Format mesuré des vraies archives

- Particuliers : 5 555 entrées ; Entreprendre : 2 665 entrées. Noms à plat : `F…`, `N…` et `R…`
  suivis de `.xml`, plus neuf fichiers nommés (`menu.xml`, `arborescence.xml`,
  `idxDateModifImp.xml`, `questionsReponses.xml`, `commentFaireSi.xml`, `accueil.xml`,
  `centresDeContact.xml`, `servicesEnLigne.xml`, `redirections.xml`).
- Chaque entrée : méthode 8 (`deflate`), drapeaux à zéro (ni chiffrement, ni descripteur de
  données), système d'origine Unix, version 3.0 (créée) et 2.0 (pour extraire) ; champs extra
  `UT` (0x5455, 9 octets en local, 5 au central) et `ux` (0x7875, 11 octets).
- Fin de répertoire dans les 22 derniers octets : un seul disque, aucun commentaire, aucun zip64.
- En-têtes servis le 2026-11-24 (`HEAD`, HTTP/1.1) : `Content-Type: application/zip`,
  `Content-Length` (24 332 119 et 11 058 942 octets), `Last-Modified` (Tue, 24 Nov 2026 14:26:09
  GMT et 14:26:24 GMT), `ETag` à la façon de nginx, date et taille en hexadécimal
  (`"6b059e81-1734757"`, `"6b059e90-a8befe"`), `Accept-Ranges: bytes`,
  `Strict-Transport-Security` ; aucun `Content-Encoding`, aucun `Set-Cookie`.

## Mini-archives

`archives/part-vosdroits-mini.zip` et `archives/pro-vosdroits-mini.zip` contiennent exactement les
fichiers de `xml/part/` et de `xml/pro/`. `build_archives.py` les construit en recopiant, pour
chaque entrée, l'en-tête local et les données compressées de la vraie archive tels quels ; seul le
décalage de l'en-tête local change dans le répertoire central, et la fin de répertoire est neuve.
Rien n'est recompressé : la construction donne les mêmes octets sur toute machine.

- Refaire : télécharger les deux archives (deux requêtes), extraire les mêmes entrées dans `xml/`,
  puis `python3 -I build_archives.py --from PART_ZIP PRO_ZIP`, et mettre à jour les tailles
  ci-dessous.
- Vérifier, sans réseau : `python3 -I build_archives.py --check` (sortie 1 en cas d'écart).

## Fichiers

- `archives/part-vosdroits-mini.zip` (30018 octets) : les onze entrées de `xml/part/`, dans l'ordre de la vraie archive.
- `archives/pro-vosdroits-mini.zip` (7590 octets) : les trois entrées de `xml/pro/`, dans l'ordre de la vraie archive.
- `build_archives.py` : construction et vérification des mini-archives.
- `xml/part/F10041.xml` (4168 octets) : fiche avec liens externes, sans référence Légifrance.
- `xml/part/F1239.xml` (6002 octets) : question-réponse conditionnée, encadré « À savoir », références.
- `xml/part/F1437.xml` (10479 octets) : fiche d'information conditionnée à situations et cas, références.
- `xml/part/F1903.xml` (25634 octets) : fiche d'information conditionnée longue, encadrés « À noter » et « Attention ».
- `xml/part/F2408.xml` (6629 octets) : question-réponse à tableaux, commune aux deux publics (version Particuliers).
- `xml/part/F31613.xml` (8606 octets) : fiche « comment faire si ».
- `xml/part/F34680.xml` (6027 octets) : recherche guidée.
- `xml/part/F468.xml` (8872 octets) : fiche d'information conditionnée à tableaux, références.
- `xml/part/N19804.xml` (1950 octets) : thème.
- `xml/part/N31675.xml` (1635 octets) : dossier.
- `xml/part/menu.xml` (31763 octets) : menu de Particuliers (thèmes, sous-thèmes, dossiers).
- `xml/pro/F2408.xml` (6661 octets) : la même question-réponse, version Entreprendre (même texte ; thème et contributeur propres au public).
- `xml/pro/F37954.xml` (8714 octets) : fiche d'information conditionnée d'Entreprendre, références.
- `xml/pro/menu.xml` (9884 octets) : menu d'Entreprendre.
