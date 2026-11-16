# ADR-0028 — BOFiP : la doctrine fiscale derrière un port distinct, lue en direct sur le jeu ouvert de la DGFiP

- **Statut** : **Accepté** (2026-11-15, à la fusion de la PR #83)
- **Date** : 2026-11-15
- **Jalons concernés** : la version mineure qui suit la 1.2.0, soit la 1.3.0 (numéro décidé par le propriétaire le 2026-11-15)
- **Questions liées** : Q28 (mode d'accès), Q29 (activation), Q30 (forme des outils), Q31 (citations BOI) : **décidées par le propriétaire le 2026-11-15, A pour chacune** (voir « Questions tranchées »)
- Applique la décision 7 d'[ADR-0004](0004-piste-puis-dila.md) (« Judilibre et les autres sources (BOFiP, AN, Sénat) sont hors périmètre. Leur ajout passe par un nouvel ADR, derrière le même port ou un port distinct justifié »). Amende [00-decisions.md](../plan/00-decisions.md) §1, §2, §3 et §7 (AM-92).
- **Note du 2026-11-15** (sans réécrire ce qui suit) : [ADR-0029](0029-bofip-precisions-apres-mesure.md) révise les décisions 2, 4, 5, 6, 8, 9, 10 et 12 après la mesure du jeu entier (AM-93). Les points changés : départage par `publication_id` et non par le début de validité (23 identifiants à deux publications, pas cinq), filtre `kind`, porte `rustls` non ouverte, `LF_BOFIP` invalide qui coupe la source, quota du jour, couverture, plafond des instructions à 3 700. Les mentions correspondantes ci-dessous sont celles de l'origine.
- **Seconde note du 2026-11-15** (sans réécrire ce qui suit) : [ADR-0030](0030-bofip-texte-lu-dans-le-html.md) révise la dernière phrase de la décision 4 (« `contenu_html` n'est jamais demandé »), la phrase de la décision 5 sur l'origine du texte, et l'alternative écartée « lire `contenu_html` pour les numéros de paragraphe » (AM-97). Mesure du jeu entier : depuis 2024, le champ `contenu` n'a plus de saut de ligne et les numéros de paragraphe y sont collés au texte. Le texte vient désormais de `contenu_html`, lu par le lecteur HTML du domaine ; `contenu` sert de secours.
- **Note du 2026-11-16** (sans réécrire ce qui suit) : [ADR-0031](0031-bofip-recherche-mots-exacts.md) précise la recherche (AM-98). La ligne « L'API » du contexte et la conséquence « La recherche est celle du portail » ne disaient pas que `search()` retrouve aussi les mots approchants. Une recherche demande désormais les mots exacts (`search(contenu, …) and contenu like …`) ; la recherche approchée ne sert plus que si aucune publication ne les porte tous, et la réponse le dit. Aucune décision de ce document n'est changée.
- **Seconde note du 2026-11-16** (sans réécrire ce qui suit) : [ADR-0032](0032-bofip-articles-cites.md) complète la décision 6 (AM-99) : une publication lue porte aussi `cited_articles`, les articles de code que cite son texte (50 au plus), et sa note de doctrine dit de les vérifier avec `fr_verify_citations` puis `fr_diff_article`. L'outil n'appelle pas Légifrance ; aucune décision de ce document n'est changée.

## Contexte

- **Le manque.** Le BOFiP (Bulletin officiel des finances publiques, impôts) réunit les commentaires par lesquels l'administration fiscale dit comment elle applique la loi. Ce n'est pas la loi : Légifrance ne le contient pas, et `fr_coverage` le déclare non couvert (lacune FR-009, « doctrine »). Le test de bout en bout de la 1.1.0 en production (2026-11-02, rapport hors dépôt) a posé 13 questions patrimoniales : le BOFiP manquait dans 11.
- **La source.** Jeu `bofip-vigueur` du portail `data.economie.gouv.fr`, éditeur DGFiP, Licence Ouverte 2.0, mise à jour hebdomadaire. Sonde manuelle du 2026-11-14 (une trentaine de requêtes) : 9 173 publications (6 367 « Contenu », 2 806 « Actualité »), 31 séries, débuts de validité du 2012-09-12 au 2026-11-04, dernière mise à jour le 2026-11-12. Champs : `identifiant_juridique` (`BOI-RFPI-PVI-20-20`, `ACTU-2017-00047`), `titre`, `type`, `serie`, `division`, `debut_de_validite`, `permalien` (vers `bofip.impots.gouv.fr`), `contenu` (texte entier : 50 161 caractères pour `BOI-RFPI-PVI-20-20`) et `contenu_html` (68 277).
- **L'API.** Opendatasoft « Explore v2.1 », sans compte ni clé : recherche plein texte (`search(contenu, "…")`), lecture exacte par `identifiant_juridique`, arborescence par préfixe (`startswith`, 44 publications sous `BOI-RFPI-PVI`). Quota lu dans les en-têtes de réponse : 50 000 requêtes par jour, une de moins à chaque appel.
- **Défauts relevés.** Cinq identifiants désignent chacun deux publications (`BOI-BA-BASE-30-20-20` : deux titres, deux permaliens) ; six publications n'ont pas de série ; `contenu` porte des entités `&#13;` ; le jeu ne contient que les publications en vigueur ; l'en-tête de quota peut venir d'un cache et porter le compteur d'un autre client (49 383 lu entre deux lectures à 49 974).
- **Les archives.** Le jeu voisin `bofip-impots` liste une archive complète (`bofip_stock_live_20261028.tgz`) et six flux hebdomadaires sur `bofip.impots.gouv.fr/opendata/`. Ni leur taille (pas de `Content-Length`) ni leur format n'ont été lus.
- **Les contraintes du projet.** `LegalCorpus` a douze méthodes, toutes sur des entités de Légifrance (00 §3). `reqwest` et `rustls` sont confinés à `lf-adapter-piste` (`CONFINED`, `xtask/src/arch.rs`). `tools/list` pèse 35 424 octets pour un plafond de 36 500 (AM-73). Le mode hors ligne promet qu'aucune requête ne part sur le réseau (ADR-0019, décision 8). La liste des variables d'environnement est fermée (00 §1).

## Décision

1. **Un port distinct, `TaxDoctrine`** (`lf-application::ports`), à deux opérations : `search` (mots, série facultative, page) et `get` (un identifiant). `LegalCorpus` n'est pas touché : aucune de ses méthodes ne convient (pas d'identifiant Légifrance, pas de version à une date, pas de fonds), et la doctrine reste ainsi séparée de la loi jusque dans le code. Dispatch statique, sans `dyn` (ADR-0012). Les erreurs réemploient les codes en place (ADR-0010).
2. **Un crate, `lf-adapter-bofip`** : dépendances internes `lf-application` et `lf-domain`, aucune vers un autre adapter ; une ligne dans `RULES` ; les portes de `reqwest` et de `rustls` s'ouvrent à lui (même pile TLS que PISTE), et à lui seul. Aucune dépendance nouvelle dans le verrou n'est attendue ; s'il en fallait une, elle suivrait la règle d'âge.
3. **Lecture en direct de l'API du jeu `bofip-vigueur`** (Q28 = A). Aucun compte, aucun téléchargement, aucun index : les deux outils marchent dès l'installation, même sans identifiants PISTE. Le port laisse la place à un second adapter sur les archives, comme DILA est venu après PISTE (ADR-0004).
4. **Une requête fermée.** Hôte `data.economie.gouv.fr` et chemin du jeu fixés dans le code, `https` seul, aucune variable de surcharge, aucune redirection suivie, délai et échéance de l'appel d'outil respectés (`LF_HTTP_TIMEOUT_S`, `Deadline`), réponse plafonnée en octets. Les mots de l'appelant n'entrent dans la requête que comme un littéral de chaîne échappé ; la série est validée par le domaine. `contenu_html` n'est jamais demandé : aucun analyseur HTML ne lit une entrée distante.
5. **Deux types de domaine, purs** : `BoiId` (majuscules, chiffres et tirets, 64 octets au plus, validé avant tout appel) et la publication (identifiant, titre, nature, série, division, début de validité, permalien, texte). Le texte vient de `contenu`, nettoyé de ses entités de fin de ligne. Le permalien n'est rendu que s'il commence par `https://bofip.impots.gouv.fr/` ; il n'est jamais appelé.
6. **Deux outils, `fr_search_bofip` et `fr_get_bofip`** (Q30 = A), en lecture seule. La recherche rend identifiant, titre, série, début de validité et permalien, jamais le texte. La lecture suit le retour progressif de `fr_get_text` (résumé, `detail`, `offset`, `focus`, ADR-0006). Un identifiant qui désigne plusieurs publications les rend en liste, à départager par leur début de validité. `fr_search_all` reste sur les fonds de Légifrance.
7. **La nature du texte est dite à chaque réponse** : doctrine de l'administration fiscale, pas la loi ; publication en vigueur à la date de la dernière mise à jour du jeu ; **aucune version passée n'est servie**, alors que la doctrine applicable est celle du jour des faits. La phrase sur l'opposabilité (article L. 80 A du livre des procédures fiscales) est écrite sur le texte de l'article, relu avant le verrou des descripteurs.
8. **Active par défaut, sans réglage** (Q29 = A) : une variable `LF_BOFIP` (`on` ou `off`, défaut `on`) permet de la couper. Quand `LF_DILA_DIR` est défini, elle est coupée d'office et ne revient que par `LF_BOFIP=on` explicite : le mode hors ligne ne contacte aucun hôte que l'utilisateur n'a pas demandé. Coupée, chaque outil répond `[config_error]` avec un texte fixe, sans appel réseau.
9. **Couverture et attribution.** `fr_coverage` gagne une famille BOFiP (état, hôte, mention « Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0 ») et une lacune sur l'absence de versions passées ; FR-009 ne dit plus « doctrine » sans réserve. Le README (hôtes contactés, attribution), `PRIVACY.md` et `SECURITY.md` nomment le nouvel hôte dans la PR du code.
10. **Audit, débit, cache.** Une ligne d'audit par appel, comme tout outil ; le format du journal ne change pas (l'outil nomme la source, `env` garde l'environnement PISTE configuré). Deux requêtes par seconde au plus, valeur fixe ; un refus 429 est rendu `[rate_limited]`, sans nouvelle tentative. Pas de cache dans cette version.
11. **Hors de cette version** (Q31 = A) : la reconnaissance des références `BOI-…` par `fr_resolve` et `fr_verify_citations`, l'index local des archives, les versions passées, tout calcul d'impôt.
12. **Version mineure, surface élargie** : deux outils de plus, rubrique « Tool surface » du CHANGELOG, `lock-descriptors --update`. Le plafond de `tools/list` est recalculé par la règle d'AM-63 (mesure finale + 3 %, arrondie à la centaine supérieure) ; les instructions du serveur restent sous 3 500 caractères.

## Questions tranchées

Posées au propriétaire avec une recommandation ; réponse du 2026-11-15 : A pour chacune.

| Question | Décision | Option écartée |
|---|---|---|
| **Q28** Mode d'accès | **A** : API en direct | B : index local construit depuis les archives (hors ligne et sans hôte tiers, mais téléchargement, construction et beaucoup plus de code) |
| **Q29** Activation | **A** : active par défaut, coupable, coupée d'office en mode hors ligne | B : inactive tant que `LF_BOFIP=on` n'est pas posé |
| **Q30** Forme des outils | **A** : deux outils dédiés | B : une valeur `BOFIP` dans `fr_search` et `fr_get_text` (moins d'octets, mais la doctrine se mêle à la loi) |
| **Q31** Citations `BOI-…` vérifiées | **A** : version suivante, après mesure de l'apport | B : dans la même version (touche l'analyseur de citations) |

## Conséquences

### Positives

- La doctrine fiscale en vigueur devient lisible et citable, avec son identifiant, sa date et son adresse officielle, sans compte ni installation de plus.
- Rien ne change pour Légifrance : ni le port, ni les treize outils, ni le journal d'audit.
- Les outils BOFiP marchent sans identifiants PISTE : première fonction utilisable par quelqu'un qui n'a pas encore de compte.

### Négatives

- **Un hôte de plus reçoit les mots des recherches** : `data.economie.gouv.fr`, portail du ministère sur une plateforme Opendatasoft. Aucun identifiant de l'utilisateur n'est envoyé, mais l'adresse IP et la requête le sont.
- **Pas de version passée** : pour des faits antérieurs au début de validité d'une publication, l'outil ne donne pas le texte applicable ; il le dit, il ne le remplace pas.
- **La recherche est celle du portail** : classement et traitement du français ne sont pas les nôtres et ne sont pas mesurés.
- `tools/list` grossit d'environ 5 000 octets si chaque outil pèse la moyenne des treize (35 424 / 13) : un coût de contexte payé par chaque session, même sans question fiscale.

### À surveiller

- **NON VÉRIFIÉ** [V-219 · bofip-quota-anonyme] : le quota de 50 000 requêtes par jour est lu dans des en-têtes, sans texte officiel ; sa portée (par adresse IP ?) est déduite d'un décompte.
- **NON VÉRIFIÉ** [V-220 · bofip-archives-format] : format, taille et contenu des archives (portent-elles les versions passées ?).
- **NON VÉRIFIÉ** [V-221 · bofip-recherche-pertinence] : qualité du classement de `search()`, relevée sur deux requêtes seulement.
- **NON VÉRIFIÉ** [V-222 · bofip-taille-max] : taille de la plus longue publication (la fonction `length()` de l'API plafonne à 1 024).
- **NON VÉRIFIÉ** [V-223 · bofip-hebergeur-journaux] : qui exploite le portail et ce qu'il journalise.
- **NON VÉRIFIÉ** [V-224 · bofip-identifiants-doubles] : cause des cinq identifiants à deux publications, et stabilité de la règle de départage.
- **NON VÉRIFIÉ** [V-225 · bofip-opposabilite-texte] : formulation exacte de la phrase d'opposabilité, à écrire sur l'article L. 80 A du LPF lu dans Légifrance.
- Condition de révision : si V-219 ou V-221 tournent mal, ou si les versions passées deviennent nécessaires, l'index local (Q28 = B) vient par un nouvel ADR, derrière le même port.

### Tests qui la gardent

À écrire avec le code, sur des réponses réelles du jeu gardées en fixtures (données publiques, attribution jointe) : aucune requête réseau dans `make ci` ; refus de tout hôte, redirection ou permalien hors liste ; échappement du littéral de recherche sur entrées hostiles ; `BoiId` refusé avant tout appel ; identifiant à deux publications ; `LF_DILA_DIR` coupe la source ; une ligne d'audit par issue ; `bofip_member_has_a_rule` (`xtask/src/arch.rs`) ; un test live hebdomadaire, non bloquant.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Index local des archives dès maintenant | Format et taille non lus (V-220) ; téléchargement et construction à la charge de l'utilisateur ; l'adapter DILA, de même nature, pèse 9 600 lignes. Reste ouvert derrière le même port. |
| Méthodes ajoutées à `LegalCorpus` | Douze méthodes sur des entités Légifrance ; PISTE et DILA devraient refuser les nouvelles, et la doctrine partagerait les types de la loi. |
| Lire `contenu_html` pour les numéros de paragraphe | Un analyseur HTML sur une entrée distante, pour des numéros déjà présents dans `contenu`. |
| Extraire les pages de `bofip.impots.gouv.fr` | Pas d'API ; un jeu ouvert sous Licence Ouverte existe. |
| Passer par un service tiers qui relaie le BOFiP | Un intermédiaire verrait les requêtes ; le projet parle aux sources publiques elles-mêmes. |
| Variable de surcharge de l'adresse | Surface d'attaque (requête vers un hôte choisi) sans besoin : l'adresse du jeu est publique et unique. |

## Références et sources

- Jeu : <https://data.economie.gouv.fr/explore/dataset/bofip-vigueur/> ; métadonnées et requêtes de la sonde : `https://data.economie.gouv.fr/api/explore/v2.1/catalog/datasets/bofip-vigueur` (`/records`, `/exports`), relevées le 2026-11-14. Archives : jeu `bofip-impots` du même portail.
- [ADR-0004](0004-piste-puis-dila.md) décision 7 ; [ADR-0006](0006-sortie-compacte-retour-progressif.md) ; [ADR-0010](0010-erreurs-in-band.md) ; [ADR-0012](0012-ports-async-rpitit.md) ; [ADR-0019](0019-dilacorpus-hors-ligne.md) décision 8 ; [ADR-0023](0023-article-entier-et-plafond-tools-list.md).
- [00-decisions.md](../plan/00-decisions.md) §1, §2, §3, §7, §14 (AM-63, AM-73, AM-92) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.22 (V-219 à V-225).
