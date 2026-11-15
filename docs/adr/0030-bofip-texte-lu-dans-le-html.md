# ADR-0030 — BOFiP : le texte des publications lu dans le champ HTML du jeu, par le lecteur HTML du domaine

- **Statut** : **Accepté** à la fusion de la PR qui le porte (choix du propriétaire du 2026-11-15, pris avant l'écriture du code)
- **Date** : 2026-11-15
- **Jalons concernés** : MB (BOFiP), premier correctif après la 1.3.0 (numéro à décider par le propriétaire ; la règle de 05 §9.5 donne un correctif tant qu'aucun descripteur ne change)
- **Questions liées** : Q35 (source du texte des publications) : **décidée par le propriétaire le 2026-11-15, A** (voir « Question tranchée »)
- Amendements : AM-97 (00 §14). Révise, dans [ADR-0028](0028-bofip-doctrine-fiscale.md), la dernière phrase de la décision 4 (« `contenu_html` n'est jamais demandé »), la phrase de la décision 5 sur l'origine du texte, et l'alternative écartée « lire `contenu_html` pour les numéros de paragraphe ». La décision 4 d'[ADR-0029](0029-bofip-precisions-apres-mesure.md) et AM-95 restent en vigueur comme **lecture de secours**. Rien d'autre ne change dans ces deux ADR. Amende [00-decisions.md](../plan/00-decisions.md) §2.

## Contexte

Mesures du 2026-11-15 sur le jeu `bofip-vigueur` entier (9 173 publications ; trois exports et une lecture `records`, quatre requêtes), hors dépôt.

- **Le défaut.** Le rejeu des 13 questions (07 §2.11, T-MB-44) a relevé que les publications récentes sont illisibles par paragraphe (09 §2.20). Le champ `contenu` n'a **aucun saut de ligne** dans 1 821 publications : les 1 737 publications de 2024 à 2026, toutes, et 84 plus anciennes. Chacune est rendue en un seul paragraphe (pour celles depuis 2024 : médiane 3 587 caractères, 85 932 au plus). Les numéros de paragraphe y sont collés au texte (« …(CGI).I. Activités d'enseignementA. Établissements privés d'enseignement10Les établissements… »), les cellules de tableau aussi (« 7 000 €35 000 €5 000 € »), et un jeton brut `[node:date:…]` reste dans 868 publications.
- **Deviner la découpe ne suffit pas.** Quatre règles (numéro collé des deux côtés, ponctuation suivie d'une majuscule, minuscule suivie d'une majuscule, tiret de liste), mesurées contre les vrais blocs sur 886 publications sans jeton : 62,3 % des frontières retrouvées, 3,8 % de coupes à tort (« DGFi|P », « CoMo|Fi », identifiants ECLI) ; les cellules de tableau ne se retrouvent pas.
- **Le champ `contenu_html` garde la mise en page.** Présent pour toutes les publications qui ont un texte. Blocs `p` (851 846 balises), `td` et `th`, `li`, `h1` à `h6`, `blockquote` ; 2 197 tableaux dans 843 publications ; aucune balise `script` ni `style`, aucun commentaire. Le numéro de paragraphe a son bloc (`<p class="numero-de-paragraphe-western">10</p>`). Le jeton ne figure dans aucun des 9 173 champs : à sa place, le HTML porte une date (« 03/07/2024 ») ou, pour les 116 jetons `[node:date:self]`, la mention « [date de publication insérée automatiquement] ». Le champ pèse environ 1,45 fois le texte brut, en octets de la réponse.
- **Deux défauts du champ.** Chaque guillemet double y est **doublé**, dans les balises comme dans le texte (`class=""paragraphe-western""`, `""roaming""`) : 1 704 693 suites de deux guillemets, 63 de quatre, aucune suite de longueur impaire, sur les 9 173 publications ; le point d'accès `records`, celui du serveur, rend la même forme. Trois publications (`ACTU-2020-00272`, `ACTU-2020-00302`, `ACTU-2021-00194`) commencent par un reste d'en-tête de document (`YPE html PUBLIC "-//W3C//DTD HTML 4.0 Transitional//EN" …>`), suivi de `<html><head><meta>`.
- **Le lecteur existe.** `lf-domain::text::html` (03 §7.2) lit déjà le HTML distant de Légifrance : écrit à la main, sans crate HTML, linéaire, mémoire bornée, sans récursion ni panique (audits M-1 et M-2, cible de fuzz `from_html`). Sonde jetable : `LongText::from_html` appliqué aux 9 173 champs, guillemets dédoublés. Aucune publication refusée. Mêmes mots que la lecture actuelle pour 9 124 des 9 127 publications à identifiant unique, jeton et séparateurs de tableau mis à part ; les trois autres sont celles à en-tête. Le texte issu du HTML fait de 0,994 à 1,859 fois la longueur du texte brut, jamais moins de 0,99.

| Époque | Publications | Paragraphes, lecture actuelle | Paragraphes, lecture HTML | Coupures propres à la lecture actuelle | Coupures propres au HTML |
|---|---|---|---|---|---|
| 2012 à 2020 | 5 993 | 351 704 | 284 957 | 67 078 | 327 |
| 2021 à 2023 | 1 398 | 80 306 | 75 742 | 6 771 | 635 |
| 2024 à 2026 | 1 736 | 1 736 | 112 828 | 0 | 111 092 |

Les coupures propres à la lecture actuelle sont les sauts au milieu d'une phrase que la règle d'AM-95 laisse (devant un chiffre, une majuscule, une parenthèse) et les cellules de tableau, que le lecteur HTML réunit par ligne (« Zones | I bis | I | II | III » ; 14 073 lignes rendues avec ce séparateur). Depuis 2024, les 21 788 numéros de paragraphe sont chacun seuls dans leur paragraphe. Le tableau porte sur les publications à identifiant unique ; les coupures de ses deux premières lignes sont comptées sur celles dont les mots sont identiques dans les deux lectures (5 991 et 1 379). Les 23 identifiants à deux publications (46 enregistrements) n'ont pas été comparés par la sonde.

## Décision

1. **Le texte d'une publication vient de `contenu_html`.** `fr_get_bofip` demande `contenu_html` en plus de `contenu`, dans la même requête. La recherche ne demande toujours ni l'un ni l'autre.
2. **Un seul lecteur HTML dans le projet** : celui du domaine (`LongText::from_html`), sans changement de sa grammaire. Aucun fichier de `crates/lf-domain/src/text/` n'est modifié : la lecture de Légifrance ne peut pas changer. Aucune dépendance nouvelle, aucune porte ouverte dans `CONFINED`, aucun paquet de plus dans le verrou.
3. **Préparation du champ, propre au BOFiP** (fonction pure de `lf-domain::doctrine`), avant le lecteur :
   - **guillemets** : si toutes les suites de guillemets doubles du champ ont une longueur paire, chacune est réduite de moitié ; sinon le champ est lu tel quel (le portail aurait cessé de doubler) ;
   - **en-tête** : si le champ porte une balise ouvrante `html`, ce qui la précède est écarté.
4. **Garde et repli.** La lecture HTML n'est servie que si son texte compte au moins 90 % des caractères de la lecture du texte brut : c'est la règle de 03 §7.4, déjà appliquée aux champs `texteHtml` et `texte` de Légifrance. Sinon, ou si `contenu_html` est absent ou blanc, le texte brut est servi par la lecture d'ADR-0029 (décision 4) et d'AM-95, inchangée. Si `contenu` est absent ou blanc et `contenu_html` présent, le HTML est lu sans garde. Un repli décidé par la garde est compté et journalisé comme les autres anomalies de l'adapter : un compteur, jamais une valeur.
5. **Le texte reste une donnée.** Aucune balise n'est rendue ; mêmes retraits des caractères de contrôle et invisibles, même plafond dur de 45 000 caractères par réponse (05 §6.4).
6. **Rien d'autre ne change** : hôte, adresse, `https` seul, redirections refusées, plafond de 4 Mio après décompression, validation des identifiants, erreurs, débit, quota du jour, repos après un 429, audit, absence de cache ; les quinze descripteurs (si l'un devait changer, le travail s'arrête et le propriétaire est prévenu) ; les treize outils de Légifrance.

## Question tranchée

Posée au propriétaire avec une recommandation ; réponse du 2026-11-15 : A.

| Question | Décision | Options écartées |
|---|---|---|
| **Q35** Source du texte des publications | **A** : `contenu_html` pour toutes les publications, texte brut en secours | B : `contenu_html` pour les seules publications sans saut de ligne (deux méthodes à entretenir ; les 67 078 coupures et les tableaux des publications anciennes restent). C : pas de HTML, découpe devinée dans le texte brut (62,3 % des frontières, 3,8 % de coupes à tort, tableaux illisibles) |

## Conséquences

### Positives

- Les 1 737 publications depuis 2024 se lisent par paragraphe : la lecture ciblée (`focus`) rend des passages, et les numéros de paragraphe redeviennent citables (« § 20 »).
- Les tableaux se lisent ligne par ligne dans 843 publications, dont les barèmes et les plafonds.
- La date remplace le jeton `[node:date:…]`.
- Les publications de 2012 à 2020 perdent 67 078 coupures de trop.
- Plus aucune règle devinée sur le chemin principal : la règle d'AM-95 ne sert plus qu'en secours.

### Négatives

- **Une décision de sécurité d'ADR-0028 est révisée** : un lecteur HTML lit une seconde entrée distante. C'est le code qui lit déjà Légifrance, mais il reçoit désormais les données d'un second éditeur.
- **Une lecture télécharge 2,45 fois plus** : 350 980 octets au plus pour un identifiant (`BOI-ANNX-000050`), contre 171 042. Chaque fenêtre d'une longue publication retélécharge le tout (ADR-0029, « Négatives » ; toujours aucun cache).
- **Le texte de toutes les publications change** : `text_length` et les positions (`offset`) ne sont pas ceux de la 1.3.0. Une lecture commencée avec une version et poursuivie avec l'autre ne tombe pas au même endroit.
- Deux lectures à entretenir, la principale et celle de secours.
- La règle des guillemets tient à un défaut du portail, que personne n'explique.

### À surveiller

- **NON VÉRIFIÉ** [V-226 · bofip-html-guillemets-doubles] : cause et durée du doublement des guillemets. Si le portail le corrige, une seule suite impaire suffit à lire le champ tel quel. Cas limite : un champ corrigé dont toutes les suites seraient paires (seulement des valeurs vides) serait réduit à tort ; la garde ne le rattrape que si la perte dépasse 10 %. Le test en direct hebdomadaire contrôle que la forme doublée est toujours servie.
- Écarts mesurés jusqu'à 2023 : la lecture HTML a 962 coupures que la lecture actuelle n'a pas, 696 là où le texte brut n'a aucun saut de ligne et 266 là où la règle d'AM-95 a recollé deux blocs. Le lecteur ne connaît pas `caption`, `dl`, `article` ni `font` : il les traite en ligne, sans perte de texte.
- Condition de révision : si la mesure du jeu entier refaite avec le code de la branche s'écarte de la sonde (publication refusée, repli pris, mots perdus), le travail s'arrête avant la fusion et ce document est repris.

### Tests qui la gardent

À écrire avec le code : réduction des guillemets (suites paires, suite impaire, propriété « doubler puis réduire rend l'entrée ») ; en-tête écarté, et rien d'écarté sans balise `html` ; garde (HTML tronqué, HTML vide, `contenu` vide, les deux vides) ; une zone `script` qui avale le texte déclenche le repli ; fixtures réelles réenregistrées avec les deux champs, dont une publication depuis 2024 avec tableau et date, et une de 2012 à 2020 ; numéros de paragraphe seuls dans leur paragraphe ; aucune balise dans le texte rendu ; budgets de sortie ; cible de fuzz de la préparation ; mesure du jeu entier avec le code de la branche ; 0 mutant survivant sur les fichiers touchés.

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Deviner la découpe dans le texte brut (Q35 = C) | 62,3 % des frontières, 3,8 % de coupes à tort, tableaux et date perdus ; des règles de plus à entretenir. |
| HTML pour les seules publications sans saut de ligne (Q35 = B) | Deux méthodes selon la date ; les défauts mesurés des publications anciennes restent. |
| Un crate HTML (`html5ever`, `scraper`) | Interdit dans `lf-domain` (00 §2 et §4) ; des paquets de plus ; le lecteur du domaine lit déjà les 9 173 champs. |
| Demander `contenu_html` seul | Ni garde ni secours si le champ est abîmé ou retiré du jeu. |
| Corriger les guillemets et l'en-tête dans le lecteur du domaine | Changerait la lecture de Légifrance pour deux défauts propres au portail. |
| Une seconde requête, pour le HTML, quand le texte brut n'a pas de saut de ligne | Deux requêtes sur le débit et le quota, et toujours deux méthodes. |

## Références et sources

- Jeu et API : ceux d'[ADR-0028](0028-bofip-doctrine-fiscale.md). Mesures du 2026-11-15 : exports `jsonl` du jeu (`contenu`, puis `contenu_html` avant et depuis 2024) et une lecture `records` ; filtre de date d'après la documentation d'Opendatasoft, « Explore API v2.1 » (prédicats de date d'ODSQL), lue le même jour.
- [03-domaine.md](../plan/03-domaine.md) §7.2 (lecteur HTML) et §7.4 (choix entre HTML et texte brut) ; [05-outils-mcp.md](../plan/05-outils-mcp.md) §5.15 et §6.4 ; [07-feuille-de-route.md](../plan/07-feuille-de-route.md) §2.11 (tâches T-MB-50 à T-MB-57) ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) §2.20 et §2.22 (V-226).
- [00-decisions.md](../plan/00-decisions.md) §2, §14 (AM-95, AM-97) ; [ADR-0029](0029-bofip-precisions-apres-mesure.md) décision 4.
