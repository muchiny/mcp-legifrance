# ADR-0029 — BOFiP : précisions et corrections d'ADR-0028 après la mesure du jeu entier

- **Statut** : **Accepté** (2026-11-15, à la fusion de la PR #84)
- **Date** : 2026-11-15
- **Jalons concernés** : MB (BOFiP), dans la version mineure qui suit la 1.2.0 (numéro à décider par le propriétaire)
- **Questions liées** : Q32 (départage de deux publications), Q33 (plafond des instructions), Q34 (nature cherchée par défaut) : **décidées par le propriétaire le 2026-11-15, A pour chacune** (voir « Questions tranchées »)
- Amendements : AM-93 (00 §14). Révise les décisions 2, 4, 5, 6, 8, 9, 10 et 12 d'[ADR-0028](0028-bofip-doctrine-fiscale.md), sans le remplacer : le port distinct, le crate, la lecture en direct, les deux outils et la coupure en mode hors ligne restent en vigueur. Révise aussi le plafond des instructions d'[ADR-0006](0006-sortie-compacte-retour-progressif.md) (décision 7) et la phrase d'[ADR-0023](0023-article-entier-et-plafond-tools-list.md) qui le maintenait à 3 500. Amende [00-decisions.md](../plan/00-decisions.md) §1, §2, §3, §6 et §7.

## Contexte

ADR-0028 a été écrit sur une sonde d'une trentaine de requêtes. Avant d'écrire le code, le jeu `bofip-vigueur` a été lu en entier le 2026-11-15 (trois exports lus à la volée, rien gardé sur disque, environ 150 requêtes en tout, une par seconde), et la conception a été relue deux fois (sécurité, architecture). Plusieurs points d'ADR-0028 ne tiennent pas devant ces mesures.

| Sujet | ADR-0028 | Mesure du 2026-11-15 |
|---|---|---|
| Identifiants à deux publications | cinq, à départager par le début de validité | **23** (jamais plus de deux publications) ; pour **9** d'entre eux, les deux publications ont le même début de validité (`BOI-IF-TH-10-50-40` : deux titres, même date 2020-12-22) |
| Adresse d'une publication | permalien rendu s'il commence par `https://bofip.impots.gouv.fr/` | les 9 173 permaliens suivent `https://bofip.impots.gouv.fr/bofip/<numéro>-PGP.html`, suivi d'au plus un segment ; le numéro (15 227 au plus) est **unique** ; le plus long permalien fait 97 octets |
| Taille | non lue (V-222) | texte : médiane 2 435 caractères, 90 % sous 25 115, maximum **167 535** (`BOI-ANNX-000050`) ; 110 publications dépassent 45 000 ; ligne JSON la plus lourde : 181 668 octets |
| Publications sans texte | non relevé | **50** (26 annexes, 11 formulaires, 9 lettres, 4 cartes) |
| Entités dans `contenu` | `&#13;` | `&#13;`, `&nbsp;`, `&gt;`, `&amp;`, `&lt;`, et elles seules ; aucune balise |
| Recherche | « littéral de chaîne échappé » | un guillemet non échappé permet de **changer le filtre** (6 010 résultats au lieu de quelques-uns) ; tous les mots sont exigés, le dernier vaut préfixe, `OR`, `AND` et `-` ne sont pas des opérateurs ; un mot à trait d'union qui n'est pas le dernier ne trouve rien (« plus-value immobilière » : 0 ; « plus value immobilière » : 357) ; une entrée Unicode décomposée trouve moins (18 contre 75) |
| Nature des publications | non distinguée | 6 367 pages de doctrine, 2 806 actualités (annonces de mise à jour), mêlées dans les résultats |
| Quota | 50 000 requêtes par jour, 2 requêtes par seconde au plus | 2 requêtes par seconde font 172 800 requêtes par jour, 3,5 fois le quota lu |
| Instructions du serveur | « restent sous 3 500 caractères » | elles font 3 499 caractères, et un test exige qu'elles citent chaque outil : la ligne des deux outils ne tient pas |

Faits du code relevés par la relecture d'architecture (`main` @ `e743eeb`) : `lf-adapter-piste` ne déclare pas `rustls` (la pile TLS vient de la feature `rustls` de `reqwest`) ; `attribution` de `fr_coverage` est une constante testée mot pour mot (05 §5.8) ; tout outil enregistré doit figurer dans une famille ou dans `general_tools` de `fr_coverage` (D-34) ; les lacunes FR-102 et FR-104 disent « every tool except fr_coverage fails », ce qui devient faux dès qu'une source marche sans PISTE.

## Décision

1. **Départage par le numéro de publication** (Q32 = A). `fr_get_bofip` reçoit `publication_id?` à la place de `valid_from?` : un entier, lu dans le permalien (`…/bofip/<numéro>-PGP.html`). `fr_search_bofip` le rend pour chaque résultat. Sans lui, un identifiant à deux publications rend la liste des deux (numéro, titre, début de validité, adresse), jamais un texte ; avec un numéro qui ne correspond pas, `[not_found]`.
2. **Doctrine seule par défaut** (Q34 = A). `fr_search_bofip` reçoit `kind?` : `doctrine` (défaut), `news` ou `all`.
3. **Mots de la requête.** Avant tout appel : forme NFC ; tout caractère qui n'est ni une lettre ni un chiffre devient un espace ; une requête vide après ce nettoyage est refusée (`[invalid_arg]`), sans réseau. Une seule fonction écrit les littéraux de la requête amont : elle n'accepte que des valeurs typées par le domaine (mots, identifiant, série, nature) et échappe `\` puis `"`. La recherche reste `search(contenu, …)` (décision inchangée, mesurée : voir « À surveiller », V-221). La description de l'outil dit la règle : tous les mots, dernier mot en préfixe, pas d'opérateur.
4. **Texte.** Les cinq entités mesurées sont traitées en une seule passe (`&amp;lt;` reste `&lt;`) : `&#13;` et le retour chariot sont retirés, `&nbsp;`, l'espace insécable et la tabulation deviennent un espace, `&lt;`, `&gt;` et `&amp;` sont décodés. Toute autre entité est laissée telle quelle et comptée. Une publication sans texte rend ses métadonnées, `text_length` à 0 et une note qui renvoie à `source_url`.
5. **Adresse rendue.** `source_url` n'est rendu que si le permalien suit la grammaire stricte : `https://bofip.impots.gouv.fr/bofip/<1 à 7 chiffres>-PGP.html`, puis au plus un segment de lettres, chiffres et `. _ % = -`, 512 octets au plus. Sinon le champ est omis. L'adresse n'est jamais appelée.
6. **Requête fermée, compléments.** `https` seul, **sans** l'exception de boucle locale du client PISTE ; seule une base de test compilée sous `#[cfg(test)]` dans le crate peut viser un autre hôte. Réponse plafonnée après décompression : 4 Mio pour une lecture, 1 Mio pour une recherche. Chaque identifiant rendu par le portail est validé, et doit être celui qui a été demandé en lecture ; jamais plus de résultats que demandé ; sinon l'appel échoue (`[upstream_error]`). D'un corps d'erreur, on ne garde que le code d'erreur, s'il a la forme attendue. Aucune erreur ne porte l'adresse de la requête, ses mots, ni un message du portail (en GET, l'adresse contient les mots cherchés).
7. **Débit et quota.** Deux départs de requête par seconde et deux lectures en vol au plus ; 10 000 requêtes par jour (UTC) et par processus, au-delà `[rate_limited]` sans réseau ; après un refus 429, la source se repose sans réseau pendant la durée de `Retry-After`, une heure au plus, quinze minutes si l'en-tête manque. Toujours aucun réessai, ni cache.
8. **`LF_BOFIP`, échec fermé.** Toute valeur autre que `on` ou `off` (casse ignorée) **coupe** la source, avec un avis : un refus mal écrit (`false`, `0`, `no`) ne doit pas envoyer les mots d'une recherche à un tiers. Un `LF_DILA_DIR` défini coupe la source d'office, qu'il soit utilisable ou non. `check-config` n'affiche la ligne `LF_BOFIP` que si la variable est définie ou si la source est coupée d'office : sa sortie par défaut ne change pas.
9. **Porte `rustls` non ouverte.** Seule la porte de `reqwest` s'ouvre à `lf-adapter-bofip` dans `CONFINED`.
10. **Couverture.** `fr_coverage` rend la famille `BOFIP` **toujours**, source active ou coupée (D-34) ; son libellé dit la source et la licence. `attribution` et `source` ne changent pas : la mention « Source : BOFiP-Impôts / DGFiP — Licence Ouverte 2.0 » est portée par chaque réponse des deux outils. FR-009 précise que la doctrine fiscale passe par les outils BOFiP. FR-102 et FR-104 ne disent plus que tous les outils échouent, mais ceux qui lisent Légifrance. Deux lacunes calculées s'ajoutent, sans effet sur `status` : FR-106 quand la source est active (publications en vigueur seulement, aucune version passée) et FR-107 quand elle est coupée. Le catalogue statique garde neuf lacunes.
11. **Instructions du serveur** (Q33 = A). Le plafond passe de 3 500 à **3 700** caractères à la livraison des deux outils (Claude Code coupe à 4 096 : 396 caractères de marge). La variante v0.3, seule servie, gagne une ligne qui cite les deux outils ; les variantes v0.1 et v0.2 ne changent pas ; aucune variante nouvelle. Jusqu'à la livraison, le code et ses tests gardent 3 500.
12. **Rien d'autre ne change dans ADR-0028** : pas de variable d'adresse, pas de version passée, pas de citations `BOI-…` dans `fr_resolve` et `fr_verify_citations`, format du journal d'audit inchangé, plafond de `tools/list` recalculé par la règle d'AM-63.

## Questions tranchées

Posées au propriétaire avec une recommandation ; réponse du 2026-11-15 : A pour chacune.

| Question | Décision | Option écartée |
|---|---|---|
| **Q32** Départager deux publications de même identifiant | **A** : paramètre `publication_id` (numéro de la publication sur le site du BOFiP) | B : rendre les deux textes l'un après l'autre (réponses deux fois plus longues, positions de lecture fragiles) |
| **Q33** Instructions pleines à un caractère près | **A** : plafond relevé à 3 700 | B : garder 3 500 et raccourcir de 150 caractères des consignes validées par le test de bout en bout de la 1.1.0 |
| **Q34** Nature cherchée par défaut | **A** : la doctrine seule, `kind` pour le reste | B : doctrine et actualités mêlées (les actualités, courtes, masquent les pages de fond) |

## Conséquences

### Positives

- Les 23 identifiants à deux publications se lisent sans ambiguïté, y compris les 9 que la date ne départageait pas.
- Une recherche avec un trait d'union, une apostrophe ou un guillemet trouve ce qu'elle doit, et ne peut pas modifier le filtre envoyé au portail.
- Le quota lu ne peut plus être dépassé par un seul processus, et un refus du portail n'est pas suivi d'une rafale.
- Une faute de frappe dans `LF_BOFIP` coupe la source au lieu de la laisser active.

### Négatives

- **`fr_coverage` change pour tout le monde** : une famille de plus, FR-009, FR-102 et FR-104 reformulées, une lacune calculée de plus. Son descripteur ne change pas, sa réponse si.
- **Coût fixe par session** : environ 150 caractères d'instructions de plus, en plus des deux descripteurs.
- Un usage très intensif (plus de 10 000 requêtes par jour dans un processus) est refusé localement, alors que le portail l'aurait peut-être accepté.
- Chaque fenêtre de lecture d'une longue publication retélécharge le texte entier (181 Ko au plus) : ADR-0028 exclut tout cache.

### À surveiller

- **V-219** (partielle) : portée du quota de 50 000 requêtes par jour (par adresse IP ?) et forme du refus 429, jamais observé. Le code ne suppose rien du corps d'un 429.
- **V-221** (levée, sur un petit échantillon) : sur les 13 questions patrimoniales du test de bout en bout de la 1.1.0 (une formulation par question, doctrine seule, pertinence jugée sur les titres), `search(contenu, …)` place la page attendue dans les cinq premiers résultats pour 10 questions ; `search(titre, contenu, …)` aussi pour 10, mais pas les mêmes. Une question (bail commercial) ne relève pas du BOFiP. Avec le filtre `serie`, le texte seul en retrouve une de plus. L'écart ne justifie pas de changer le réglage d'ADR-0028 ; l'échantillon est petit.
- **V-223** (partielle : le reste n'est pas publié) : le portail est géré par le secrétariat général du ministère et hébergé par la société Opendatasoft ; ni le pays d'hébergement ni la durée de conservation des journaux ne sont publiés. `PRIVACY.md` dira cela et rien de plus.
- **V-225** (partielle) : la phrase d'opposabilité est rédigée d'après `BOI-SJ-RES-10-10-10` (garantie du troisième alinéa de l'article L. 80 A du LPF, pour les interprétations publiées au BOFiP). L'article lui-même reste à relire dans Légifrance avant le verrou des descripteurs.
- Condition de révision : si le quota du jour est atteint en usage réel, un mémo court de la dernière publication lue (300 s, la durée de cache annoncée par le portail) viendra par un nouvel ADR.

### Tests qui la gardent

À écrire avec le code, en plus de ceux d'ADR-0028 : un test de propriété sur le nombre de littéraux de la requête, quelle que soit l'entrée ; requête de ponctuation seule refusée avant le réseau ; entités décodées en une passe ; grammaire du permalien sur formes hostiles ; identifiant rendu différent de l'identifiant demandé ; aucune erreur de transport ne contient un mot de la requête (mot sentinelle, cinq pannes) ; client de production qui refuse `http` ; second appel après un 429 sans réseau ; budget du jour épuisé sans réseau ; `LF_BOFIP` invalide et `LF_DILA_DIR` inutilisable coupent la source ; mode hors ligne et source coupée prouvés par un proxy témoin, avec un contraste (source active : exactement une connexion vers `data.economie.gouv.fr:443`).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Garder `valid_from` et rendre les deux textes en cas d'égalité | 9 identifiants sur 23 restent indécidables ; deux règles au lieu d'une. |
| Chercher dans le titre et le texte | Même score sur 13 questions, avec d'autres ratés ; ADR-0028 reste inchangé sur ce point. |
| Une nouvelle variante d'instructions (v0.4) | Seule la variante v0.3 est servie ; une variante de plus touche une vingtaine de sites pour un texte que personne d'autre ne lit. |
| Une lacune statique de plus dans `fr_coverage` | Elle s'afficherait même quand la source est coupée, et grossirait la réponse de la surface v0.1, bornée par un test. |
| Changer `attribution` de `fr_coverage` | Constante du contrat (05 §5.8), testée mot pour mot ; la mention de la DGFiP est mieux placée dans les réponses qui portent sa doctrine. |
| Porte `governor` pour le débit | Un espacement simple suffit ; pas de porte à ouvrir. |

## Références et sources

- Jeu et API : ceux d'[ADR-0028](0028-bofip-doctrine-fiscale.md) ; mesures du 2026-11-15 (exports `jsonl` lus en flux, requêtes `records`). Documentation de `search()` : aide en ligne d'Opendatasoft, « Explore API v2.1 », lue le 2026-11-15.
- Coupure des instructions par Claude Code à 4 096 caractères : documentation de Claude Code (page « MCP »), lue le 2026-11-15.
- `BOI-SJ-RES-10-10` et `BOI-SJ-RES-10-10-10` (2020-03-04), lus dans le jeu.
- [00-decisions.md](../plan/00-decisions.md) §1, §2, §3, §6, §7, §14 (AM-93) ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §4.2, §5.8, §5.14, §5.15 ; [07-feuille-de-route.md](../plan/07-feuille-de-route.md) §2.11 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.20, §2.22.
