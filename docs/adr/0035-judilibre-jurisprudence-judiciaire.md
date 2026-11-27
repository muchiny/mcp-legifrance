# ADR-0035 — Judilibre : la jurisprudence judiciaire en open data derrière un port distinct, lue par l'adapter PISTE avec le même jeton et le même débit, sans rien qui permette d'étudier un juge

- **Statut** : **Proposé** (2026-11-27), accepté à la fusion de sa PR. Choix du propriétaire du 2026-11-27 : « Ok pour tout ce que tu recommandes » (Q42 à Q45 = A).
- **Date** : 2026-11-27
- **Jalons concernés** : MJ (Judilibre), après la 1.5.0. Deux outils de plus font une version mineure : la **1.6.0**, proposée ici ; `lock-descriptors --compare v1.5.0` la confirmera.
- **Questions liées** : Q42 (forme), Q43 (place du code), Q44 (activation) et Q45 (citations de cours d'appel). Réponse **A** pour chacune, le 2026-11-27.
- **Amendements** : AM-103 (00 §14).
  - Applique la décision 7 d'[ADR-0004](0004-piste-puis-dila.md) (« derrière le même port ou un port distinct justifié ») : le port est distinct, et le code va dans l'adapter PISTE, qui sert déjà ce portail.
  - Rien ne change dans les ADR du BOFiP ni dans [ADR-0033](0033-service-public-fiches-pratiques.md).

## Contexte

- **Le manque.** Légifrance (fond JURI) ne publie qu'une sélection d'arrêts de cours d'appel et presque aucune décision de première instance (lacune FR-005, 05 §5.8). Au test de bout en bout du 2026-11-02 (1.1.0, production), trois des treize questions de patrimoine manquaient de jurisprudence des juges du fond : Q09 (donation entre parent et enfant), Q10 (nue-propriété et fictivité) et Q11 (requalification d'une assurance-vie).
- **La source.** L'API Judilibre de la Cour de cassation, sur PISTE : `https://api.piste.gouv.fr/cassation/judilibre/v1.0` (bac à sable : `sandbox-api.piste.gouv.fr`).
  - Elle est gratuite : on accepte ses CGU pour chaque environnement, puis on la coche dans l'application PISTE.
  - Licence Ouverte 2.0, plus les CGU open data de la Cour de cassation (V8, 2022-01-06).
  - Bêta. OpenAPI 1.2.5 (`Cour-de-cassation/judilibre-search` @ `f773a6b`).
- **Mesures.** Sondes du 2026-11-27, en production, lancées par le propriétaire : 16 requêtes GET à 1 req/s, aucun refus (journal de 09 §2.20).
  - **Accès** : le jeton OAuth du serveur suffit, sans l'en-tête `KeyId` que déclare l'OpenAPI.
  - **Juridictions** : `cc`, `ca`, `tj`, `tcom`, `cph`. Aucune juridiction administrative.
  - **Stock** : historique pour la Cour de cassation (« contrat » : 148 213 décisions, depuis 1947). Pour le même mot : 290 175 décisions de cours d'appel, dont certaines de 2006, et 336 114 décisions de tribunaux judiciaires, jusqu'à fin septembre 2026.
  - **Texte** : de 3 774 à 38 902 caractères. Les `zones` (introduction, exposé, moyens, motifs, dispositif, annexes) ont leurs indices en unités UTF-16. Le dernier `end` dépasse le texte de 1. Les fragments ne se suivent pas, et les zones manquent sur une décision ancienne.
  - **Liens** : `visa` et `rapprochements` portent des balises `<a>` ; les rapprochements pointent vers des identifiants Judilibre. Un identifiant a 24 caractères hexadécimaux.
  - **Anomalies** : une date `0201-02-24` pour un arrêt de 2017 ; une page vide malgré un `total` de 336 114 (tri par date sur `tj`).
  - **Limites et durées** : `416` au-delà de 10 000 résultats. De 30 à 167 ms par recherche, 6,6 s pour une recherche limitée aux motifs.
  - **Pertinence** : 240 décisions pour Q11 et 58 pour Q10, dont des arrêts de la cour d'appel de Paris (pôle 3, chambre 1, successions).
- **Les règles** (CGU V8, lues en entier) :
  - ne pas altérer les données ni la pseudonymisation ;
  - pour **chaque** décision, mentionner la juridiction, la formation, le siège et la date de prononcé ;
  - mentionner la source et la date de dernière mise à jour ;
  - tenir une base copiée à jour sous 72 h (recommandation) ;
  - **profilage interdit** : l'identité des magistrats et des greffiers ne peut servir à « évaluer, analyser, comparer ou prédire leurs pratiques professionnelles réelles ou supposées » (L. 111-13 al. 3 COJ ; peines des articles 226-18, 226-24 et 226-31 du code pénal). Leurs noms restent lisibles dans les textes.
- **Le projet.**
  - `lf-adapter-piste` a déjà le jeton mono-vol, le client partagé, les réessais (ADR-0017), le limiteur `governor` (5 req/s), le sémaphore et `Deadline`.
  - Les instructions v0.3 font 3 937 caractères sur 4 000, et le test de dérive exige qu'elles citent chaque outil.

## Décision

1. **Port distinct `JudicialOpenData`** (`lf-application::ports`) :
   - `search(&CaseLawQuery) -> CaseLawPage` et `get(&JudilibreId) -> CaseLawDoc` ;
   - dispatch statique (ADR-0012) ; source active ou coupée choisie au démarrage par `AnyCaseLaw` (`Piste`, `Off`, dans `lf-app`) ;
   - le port entre dans `LegalService` par un paramètre générique muni d'un défaut (`NoCaseLaw`) ;
   - les codes d'erreur restent ceux en place (ADR-0010).
2. **Code dans `lf-adapter-piste`** (Q43 = A) : `PisteJudilibre: JudicialOpenData` partage avec `PisteCorpus` :
   - le jeton et l'application ;
   - le client ;
   - le limiteur et le sémaphore : `LF_PISTE_RPS`, `LF_PISTE_BURST` et `LF_PISTE_CONCURRENCY` comptent les deux API ensemble, ce qui tient le plafond maison de 5 req/s cumulées ;
   - les réessais, `Deadline` et le plafond de corps (16 Mio).

   Aucun crate ni paquet nouveau ; `RULES` et `CONFINED` ne changent pas. S'il fallait un paquet, le travail s'arrêterait et le propriétaire déciderait.
3. **Requêtes fermées.**
   - `GET` sur deux chemins seulement : `/cassation/judilibre/v1.0/search` et `…/decision`.
   - L'hôte est celui de l'API PISTE de la configuration en vigueur. Avec `LF_PISTE_BASE_URL` (`CustomHost`), Judilibre suit cet hôte, ce qui sert aux tests. Aucune variable nouvelle.
   - Paramètres envoyés, tirés de valeurs typées :
     - `query`, `operator`, `field`, `date_start`, `date_end`, `sort`, `order` ;
     - `jurisdiction`, **toujours explicite**, car vide il ne vise que la Cour de cassation ;
     - `page`, égal à la page de l'outil moins 1, et `page_size` ;
     - `resolve_references=true`, si V-241 le confirme.
   - Jamais appelés : `/stats`, `/export`, `/scan`, `/transactionalhistory`, `/taxonomy`, `/healthcheck`.
   - Jamais envoyés : `chamber`, `formation`, `location`, `theme`, `withFileOfType`.
4. **`fr_search_judilibre`.**
   - Paramètres :
     - `query` ;
     - `match` : `any`, `all` (défaut, `operator=and`) ou `exact` ;
     - `courts` : parmi `cc`, `ca`, `tj`, `tcom`, `cph`, toutes par défaut ;
     - `part` : `motivations`, `dispositif`, `expose`, `moyens`, `summary` ou `visa` ; texte entier par défaut, et la description dit que c'est plus lent ;
     - `date_from` et `date_to` ;
     - `sort` : `relevance` (`scorepub`, défaut) ou `date` ;
     - `page` (à partir de 1) et `page_size` (1 à 50, 10 par défaut), avec `page × page_size ≤ 10 000` vérifié avant le réseau.
   - Chaque résultat donne :
     - l'identifiant ;
     - les mentions (décision 6) ;
     - le numéro, `ecli?`, la solution et la publication ;
     - `summary?` ou un extrait sans balises ;
     - trois thèmes au plus ;
     - `citation` et `source_url?` (V-239).
   - `has_more` passe à faux à 10 000. Une page vide malgré un `total` positif porte une `note`.
5. **`fr_get_judilibre`.**
   - Paramètres :
     - `id` : 24 caractères hexadécimaux, mis en minuscules ;
     - `part?` : `introduction`, `expose`, `moyens`, `motivations`, `dispositif` ou `annexes` ;
     - les paramètres communs des lectures longues (05 §5.0.2, ADR-0006).
   - Découpage par le domaine :
     - indices UTF-16 bornés au texte et ramenés à des limites de caractère ;
     - fragments pris par début croissant et joints par une ligne vide ;
     - zones ou partie absentes : le texte entier, avec une `note`.
   - Champs rendus :
     - `visa` : intitulés sans balises, que `fr_resolve` lit ;
     - `related` : rapprochements, avec l'identifiant Judilibre ou le `JURITEXT` donné par `resolve_references` ;
     - `contested` : juridiction, chambre, date et solution de la décision attaquée ;
     - une `note` si `partial`.
   - `to_be_deleted` rend `not_found`.
   - Texte tel que publié : seules les balises HTML de `visa`, des rapprochements et des extraits sont retirées.
6. **Mentions des CGU.**
   - **Par décision** :
     - `court` : juridiction et siège, par exemple « Cour d'appel de Paris » ;
     - `chamber`, et `formation` pour la Cour de cassation ;
     - `date`, telle que publiée.

     Les intitulés viennent de `resolve_references` (V-241). À défaut, une table du domaine couvre `cc` et les 36 cours d'appel ; ailleurs, le code brut.
   - **Par réponse** :
     - `attribution` : « Source : Cour de cassation, base Open Data des décisions judiciaires (Judilibre) — Licence Ouverte 2.0 » ;
     - `updated` (`update_date`) pour une lecture, `consulted_on` (jour de l'appel, Europe/Paris) pour une recherche ;
     - une note fixe : décisions pseudonymisées, telles que publiées ; ne pas réidentifier ; interdiction d'évaluer, comparer ou prédire les pratiques d'un magistrat ou d'un greffier (L. 111-13 COJ).

     Forme à confirmer : V-242.
7. **Profilage : règles fixes, pas options.**
   - Aucun paramètre ne désigne une personne, une chambre, une formation ni un siège.
   - Aucun regroupement, compte ni statistique par personne ou par chambre ; jamais `/stats`.
   - Aucun nom extrait, indexé ni journalisé.
   - L'interdiction figure dans les deux descriptions.
   - Les mots d'une recherche appartiennent à l'appelant et ne sont pas filtrés, comme sur le site de la Cour de cassation.
8. **Aucun cache.** Un appel coûte une requête, plus un jeton s'il faut le renouveler. Rien n'est gardé : la règle des 72 h est tenue d'office.
9. **Erreurs.**
   - Identifiants absents ou configuration invalide : `[config_error]`, comme pour Légifrance.
   - **403** : `[config_error]` « Your PISTE application is not subscribed to the Judilibre API (free): accept its terms on piste.gouv.fr and tick it in your application. » Ce refus est retenu 10 minutes, sans requête pendant ce temps ; un redémarrage l'efface.
   - 404 : `not_found`.
   - 400, 416 et 423 : `upstream_error`, sans nouvelle tentative. Le 416 est empêché avant l'envoi.
   - 429 et 5xx : règles en place (00 §8).
   - Aucun message ne porte un mot de la recherche.
10. **Active par défaut** (Q44 = A), comme le BOFiP et Service-Public.
    - `LF_JUDILIBRE=on|off`, `on` par défaut, casse ignorée.
    - Coupée d'office quand `LF_DILA_DIR` est défini, sauf `on` explicite.
    - Valeur invalide : source coupée, avec un avis de niveau problème (`InvalidJudilibre`) qui ne dégrade pas Légifrance.
    - Coupée : `[config_error]` à texte fixe, sans E/S.
    - Identifiants PISTE requis.
    - Réglage MCPB `judilibre`, `on` par défaut ; `server.json` ne la déclare pas.
11. **Domaine, types purs** :
    - `JudilibreId`, `Court`, `DecisionPart`, `SearchPart` ;
    - le découpage, le retrait des balises et la table d'intitulés ;
    - l'adresse publique, à grammaire stricte et jamais appelée : `https://www.courdecassation.fr/decision/{id}` si V-239 la confirme, sinon omise ;
    - la citation, par exemple « Cour d'appel de Paris, Pôle 3 - Chambre 1, 18 janvier 2023, n° 20/17369 ».
12. **Couverture**, à la surface v0.3 seulement :
    - famille `JUDILIBRE` après `SERVICE_PUBLIC` ;
    - FR-005 renvoie à `fr_search_judilibre` ;
    - lacunes calculées FR-110 (source active : bêta, juridictions administratives absentes, couverture selon la juridiction et la date) et FR-111 (source coupée).

    Les réponses v0.1 et v0.2 restent identiques à l'octet, par un filtre de la vue, comme au jalon MS.
13. **Instructions.**
    - La v0.3 gagne une ligne 7, d'environ 165 caractères : les deux outils, la couverture et l'interdiction.
    - Le plafond **reste à 4 000**. Les lignes 5 et 6 (BOFiP, Service-Public) sont resserrées sans changer de sens, pour un total estimé à 3 992.
    - Les v0.1 et v0.2 ne changent pas. Si la ligne ne tient pas, le travail s'arrête et le propriétaire décide.
    - Budget de `tools/list` recalculé par la règle d'AM-63.
14. **Audit et débit.**
    - Une ligne d'audit par appel, au format inchangé. Métriques : nombre de résultats, caractères renvoyés.
    - `fr_search_all`, `fr_resolve` et `fr_verify_citations` ne changent pas (Q45 = A).
15. **Hors de cette version** :
    - les citations de cours d'appel vérifiées dans Judilibre (Q45, après V-240). Un numéro RG n'est pas unique : il faudra filtrer par siège et par date ;
    - les filtres de chambre, de formation, de siège et de thème ;
    - les fichiers joints ;
    - toute copie de la base ;
    - un cache.
16. **Version mineure** : rubrique « Tool surface » du CHANGELOG ; dix-neuf outils.

## Questions tranchées

| Question | Décision (2026-11-27) | Options écartées |
|---|---|---|
| **Q42** Forme | **A** : deux outils, `fr_search_judilibre` et `fr_get_judilibre` | B : `fond=JUDILIBRE` dans `fr_search` et lecture par `fr_get_decision`. Un outil commun prendrait des paramètres propres à une source, et deux formes d'identifiant s'y mêleraient |
| **Q43** Place du code | **A** : `lf-adapter-piste`, port distinct, un jeton, un limiteur | B : crate `lf-adapter-judilibre`. Jeton dupliqué, `rustls` et `governor` ouverts à un second crate, deux limiteurs vers PISTE |
| **Q44** Activation | **A** : active par défaut ; un 403 rend un `config_error` qui dit d'abonner l'application | B : coupée par défaut, donc oubliée et inconnue du modèle |
| **Q45** Citations de cours d'appel | **A** : plus tard, après V-240 | B : dans la même version, plus grosse, qui touche deux outils verrouillés |

## Conséquences

- **Positives** :
  - les arrêts de cours d'appel et les jugements de première instance deviennent lisibles, motifs seuls à la demande ;
  - aucun compte, crate ni paquet de plus ;
  - un seul plafond de débit vers PISTE ;
  - mentions et interdiction de profilage tenues par construction.
- **Négatives** :
  - abonnement à faire par le propriétaire, pour chaque environnement ;
  - source en bêta, aux données parfois mal formées ;
  - textes longs ;
  - `lf-adapter-piste` grossit ;
  - 8 caractères de marge sous le plafond des instructions ;
  - recherche lente quand elle est limitée aux motifs.
- **À surveiller** (09 §2.24) : **NON VÉRIFIÉ** [V-236 · judilibre-quotas], [V-239 · judilibre-adresse-publique], [V-240 · judilibre-apport], [V-241 · judilibre-intitules], [V-242 · judilibre-mentions].
  - Levées par les sondes : V-182, V-183, V-237 et V-238.
  - V-181 est levée par la lecture des CGU.
- **Tests qui la gardent** :
  - faux serveur : deux chemins et une liste fermée de paramètres, `jurisdiction` toujours présent, aucune requête dans les 10 minutes qui suivent un 403 ;
  - fixtures tirées des relevés du 2026-11-27, réduites ; les noms des magistrats et des greffiers y sont remplacés, car ce sont des données de test, jamais servies ;
  - découpage : bornes, zones absentes, propriété sur textes arbitraires ;
  - limiteur partagé entre les deux API ;
  - binaire coupé ou hors ligne : aucune connexion ;
  - les dix-sept descripteurs existants gardent leur `sha256` ;
  - snapshots et parité des treize outils de Légifrance inchangés.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Options B de Q42 à Q45 | Tableau des questions |
| En-tête `KeyId` | Inutile (V-182) ; une clé de plus à garder |
| Cache des décisions | Règle des 72 h, mémoire, retrait des `to_be_deleted` ; gain faible pour un usage conversationnel |
| Base locale par `/export` | Copie de toute la base, à tenir à jour sous 72 h ; hors du besoin |
| Filtres de chambre ou de formation | Pas demandés ; une chambre de cour d'appel peut se réduire à quelques magistrats (profilage) |
| Lecture du site de la Cour de cassation | Protection anti-robot ; l'API est le canal prévu |

## Références et sources

- Étude du 2026-11-27 et relevés des sondes (hors dépôt).
- `Cour-de-cassation/judilibre-search` @ `f773a6b` : README, OpenAPI 1.2.5.
- CGU open data de la Cour de cassation, V8 du 6 janvier 2022 : `https://piste.gouv.fr/images/cgu/CGU_open_data_V8.pdf`.
- Rapport Ludet sur l'open data des décisions de justice (juillet 2025).
- ADR : [ADR-0004](0004-piste-puis-dila.md), [ADR-0006](0006-sortie-compacte-retour-progressif.md), [ADR-0010](0010-erreurs-in-band.md), [ADR-0012](0012-ports-async-rpitit.md), [ADR-0017](0017-reessais-sans-backon.md), [ADR-0028](0028-bofip-doctrine-fiscale.md), [ADR-0033](0033-service-public-fiches-pratiques.md).
- Plan : [00-decisions.md](../plan/00-decisions.md) §0, §1, §2, §3, §6, §7, §8, §11, §14 (AM-63, AM-88, AM-103) ; [05](../plan/05-outils-mcp.md) §4.2, §5.8, §5.18, §5.19 ; [07](../plan/07-feuille-de-route.md) §2.13 ; [09](../plan/09-risques-et-questions.md) §2.14, §2.20, §2.24 ; annexe C §6.
