# Réponses réelles du portail, enregistrées

Ces fichiers sont des réponses du jeu `bofip-vigueur`, enregistrées telles quelles (aucun octet
modifié) pour les tests de `lf-adapter-bofip`. Ils ne contiennent aucun secret : le portail est
ouvert, sans compte ni clé.

- Adresse : `https://data.economie.gouv.fr/api/explore/v2.1/catalog/datasets/bofip-vigueur/records`
- Date de l'enregistrement : 2026-11-15
- Éditeur : Direction générale des finances publiques (DGFiP)
- Licence : Licence Ouverte 2.0 (Etalab). Mention : « Source : BOFiP-Impôts / DGFiP ».
- Débit de l'enregistrement : une requête par seconde, quatorze requêtes.

Le portail change chaque semaine : ces fichiers sont une photographie du 2026-11-15, pas l'état
courant de la doctrine. Ne pas les éditer à la main ; pour les refaire, rejouer les requêtes
ci-dessous et mettre à jour les tailles. Le test `fixtures_are_listed_with_their_source` compare
cette liste (nom et taille en octets) au contenu du dossier.

## Fichiers

Recherche : `select=identifiant_juridique,titre,type,serie,debut_de_validite,permalien`. Lecture :
le même `select`, plus `division` et `contenu`, et `limit=10`.

- `error-400-limit.json` (152 octets) : refus HTTP 400, `where=type = "Contenu"`, `limit=101` (au-delà du maximum du portail).
- `error-400-syntax.json` (178 octets) : refus HTTP 400, clause tronquée `where=search(contenu,`, `limit=1`.
- `get-double-same-date.json` (6272 octets) : lecture de `BOI-IF-TH-10-50-40`, deux publications de même date.
- `get-longest.json` (181718 octets) : lecture de `BOI-ANNX-000050`, la publication la plus longue du jeu.
- `get-no-text.json` (564 octets) : lecture de `BOI-LETTRE-000150`, publication sans texte (`contenu` nul).
- `get-single.json` (34252 octets) : lecture de `BOI-ENR-DMTG-10-50-20`, une publication.
- `get-unknown.json` (33 octets) : lecture de `BOI-ZZ-INCONNU-99`, identifiant inconnu, aucun résultat.
- `get-wrapped.json` (2984 octets) : lecture de `BOI-PAT-IFI-40-10`, publication de 2018 dont les lignes sont coupées au milieu des phrases.
- `search-all.json` (2080 octets) : `where=search(contenu, "donation")`, sans filtre de type, `limit=5`, `offset=0`.
- `search-donation-page1.json` (4113 octets) : `where=search(contenu, "donation abattement") and type = "Contenu"`, `limit=10`, `offset=0`.
- `search-donation-page2.json` (4458 octets) : la même recherche, `offset=10`.
- `search-empty.json` (33 octets) : `where=search(contenu, "zzyzxqqw") and type = "Contenu"`, aucun résultat.
- `search-news.json` (2229 octets) : `where=search(contenu, "donation") and type = "Actualité"`, `limit=5`, `offset=0`.
- `search-serie-enr.json` (2013 octets) : `where=search(contenu, "déclaration de succession délai dépôt") and type = "Contenu" and serie = "ENR"`, `limit=5`, `offset=0`.
