# Table des codes (77 entrées)

> **Rôle** : données du dictionnaire des codes du parseur de citations (M6). Règles d'emploi, conflits et sigles ambigus : [annexe B §5](../B-citations.md). Tableau repris de la [recherche 07](../../../recherche/07-citations.md) §5.2 ; statuts selon sa légende (§0.1), abréviations de chemins selon son §0.2, colonnes selon son §5.1.
> **Lecture par les tests** : `dict_matches_codes_table` (annexe B §5.5) ne lit que les lignes numérotées du tableau ; ce titre, cette légende, l'en-tête du tableau et la ligne finale sont ignorés. Toute modification d'une ligne met à jour les effectifs de l'annexe B §5.1.
> **Fiabilité** : la table vivante vient de `/list/code`, jamais de ce fichier (annexe B §5.4). Les `LEGITEXT` de la colonne 3 sont des données tierces, réservées aux tests d'intégration : **NON VÉRIFIÉ** [V-126 · codes-legitext-tiers] (V-126). Les autres formes d'usage (colonne 5) : **NON VÉRIFIÉ** [V-159 · abreviations-codes] (V-159). Les statuts « abrogé » et la présence des codes anciens dans `/list/code` : **NON VÉRIFIÉ** [V-124 · list-code-etats] (V-124). Registre : [09-risques-et-questions.md](../../09-risques-et-questions.md) §2.9 et §2.11.

| Marque | Sens |
|---|---|
| `JL`, `[JL]` | justicelibre (`sources/legi.py`, `SUPPORTED_CODES_LEGITEXT`), MIT |
| `KM`, `[KM]` | mcp-legifrance de Ktulu-Analog (`server.py`, `CODE_IDS`), AGPL-3.0 : faits à recouper seulement, aucune copie ([08 §9.4](../../08-distribution.md)) |
| `JL+KM` | identifiant donné identique par les deux sources |
| `[JRX]` | juridix de Ktulu-Analog, AGPL-3.0 : faits à recouper seulement (08 §9.4) |
| `FX` | vérifié par une fixture réelle de la référence Python (consult_article.json) |
| `DILA`, `[DILA]:169` | vérifié dans la documentation officielle DILA (`exemples-d-utilisation-de-l-api.md:169`) |
| `[PLF]` | pylegifrance, énumération `NomCode` (`pylegifrance/models/code/enum.py:4-76`, 70 titres, MIT), source des titres ; la dernière ligne du fichier liste ses titres absents du tableau |
| `WEB` (suivi du nom court de la liste) | abréviation lue dans le résumé de recherche web d'une liste d'abréviations universitaire ou éditoriale (Ref-Lex/BU, Paris-Saclay, Côte d'Azur, Paris 2, Wiktionnaire, Lextenso, Zenou, Barreau de Paris ; URL dans la recherche 07 §5.1) ; pages non ouvertes, donc moins fiable |
| `TIERS` | lu dans du code ou des données tiers (`[JL]`, `[KM]`, `[JRX]`) |
| `¹` | titre repris des commentaires de `[JL]` ou `[KM]`, absent de `NomCode` ; libellé exact **NON VÉRIFIÉ** [V-123 · list-code-forme] (V-123) |
| **ambigu**, **collision** | sigle à plusieurs sens : règles de l'annexe B §5.3 |
| **CONFLIT** | identifiants divergents selon la source : tranché par `/list/code` (annexe B §5.2) |
| `historique` | forme ancienne, encore citée |
| `VÉRIFIÉ` | lu dans un fichier (chemin:ligne), au sens de la recherche 07 §0.1 |
| Statut `?` | aucune source consultée ne donne le statut : à lire dans `/list/code` (`etat`) |

| # | Titre officiel (valeur `NOM_CODE`) | `LEGITEXT` candidat (sources) | Abréviations attestées | Autres formes d'usage | Statut |
|---|---|---|---|---|---|
| 1 | Code civil | `LEGITEXT000006070721` (JL+KM+**FX**) | `C. civ.` (WEB Ref-Lex/BU) | `CC` (TIERS [JL], **ambigu** : Conseil constitutionnel / Cour de cassation), `Code Napoléon` (historique) | en vigueur (FX) |
| 2 | Code de commerce | `LEGITEXT000005634379` (JL+KM) | `C. com.` (WEB) | `C.com` (TIERS [JL]) | ? |
| 3 | Code du travail | `LEGITEXT000006072050` (JL+KM) | `C. trav.` (WEB) | `CT` (TIERS [JL], **ambigu**) | ? |
| 4 | Code pénal | `LEGITEXT000006070719` (JL+KM) | `C. pén.` (WEB Paris-Saclay) | `CP` (TIERS [JL], **ambigu**) | ? |
| 5 | Code de procédure civile | `LEGITEXT000006070716` (JL+KM) | `C. pr. civ.` (WEB Côte d'Azur), `CPC` (WEB, TIERS) | `NCPC` (historique, TIERS [JL]) | ? |
| 6 | Code de procédure pénale | `LEGITEXT000006071154` (JL+KM) | `C. pr. pén.` (WEB Côte d'Azur), `CPP` (WEB Lextenso) | — | ? |
| 7 | Code général des impôts | `LEGITEXT000006069577` (JL+KM) | `CGI` (TIERS [JL]/[JRX]) | — | ? |
| 8 | Code général des impôts, annexe I ¹ | `LEGITEXT000006069568` (JL+KM) | — | `CGI, ann. I` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 9 | Code général des impôts, annexe II ¹ | `LEGITEXT000006069569` (JL+KM) | — | `CGI, ann. II` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 10 | Code général des impôts, annexe III ¹ | `LEGITEXT000006069574` (JL+KM) | — | `CGI, ann. III` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 11 | Code général des impôts, annexe IV | `LEGITEXT000006069576` (JL+KM) | — | `CGI, ann. IV` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
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
| 23 | Code monétaire et financier | `LEGITEXT000006072026` (JL+KM) | — | `CMF` (TIERS [JL]/[JRX]), `C. mon. fin.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 24 | Code rural et de la pêche maritime | **CONFLIT** : `LEGITEXT000006071367` ([JL]) vs `LEGITEXT000022197698` ([KM]) | — | `C. rur.` (TIERS [JRX]), `CRPM` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 25 | Code rural (ancien) | `LEGITEXT000006071366` (JL+KM) | — | — | ancien (titre ; TIERS [JL] « obsolète ») |
| 26 | Code des relations entre le public et l'administration | `LEGITEXT000031366350` (JL+KM) | `CRPA` (WEB) | — | ? |
| 27 | Code de l'entrée et du séjour des étrangers et du droit d'asile | `LEGITEXT000006070158` (JL+KM) | `CESEDA` (WEB) | — | ? |
| 28 | Code de l'action sociale et des familles | `LEGITEXT000006074069` (JL+KM) | — | `CASF` (TIERS [JL]/[JRX]) | ? |
| 29 | Code de l'organisation judiciaire | `LEGITEXT000006071164` (JL+KM) | — | `COJ` (TIERS [JRX]) | ? |
| 30 | Code des postes et des communications électroniques | `LEGITEXT000006070987` (JL+KM+**DILA**) | — | `CPCE` (TIERS [JL]) | ? (id VÉRIFIÉ [DILA]:169) |
| 31 | Code de la sécurité intérieure | `LEGITEXT000025503132` (JL+KM) | — | `CSI` (TIERS [JRX]) | ? |
| 32 | Code général de la fonction publique | `LEGITEXT000044416551` (JL+KM) | — | `CGFP` (TIERS [JRX]/[KM]) | ? |
| 33 | Code de la justice pénale des mineurs | `LEGITEXT000039086952` (JL+KM) | — | `CJPM` (TIERS [JRX]) | ? |
| 34 | Code général de la propriété des personnes publiques | `LEGITEXT000006070299` (JL+KM) | — | `CG3P` / `CGPPP` (TIERS [JL]/[KM], **NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 35 | Code des impositions sur les biens et services | `LEGITEXT000044595989` (JL+KM) | — | `CIBS` (TIERS [JL]) | ? |
| 36 | Code de la commande publique | `LEGITEXT000037701019` (JL+KM) | — | `CCP` (TIERS [JL], **ambigu**) | ? |
| 37 | Code de l'éducation | `LEGITEXT000006071191` (JL+KM) | — | `C. éduc.` (TIERS [JL]/[JRX]) | ? |
| 38 | Code des procédures civiles d'exécution | `LEGITEXT000025024948` (JL+KM) | — | `CPCE` (**collision** avec postes), `CPC exéc.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 39 | Code de la route | `LEGITEXT000006074228` (JL+KM) | — | `C. route` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 40 | Code électoral | `LEGITEXT000006070239` (JL+KM) | — | `C. élect.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]), `CE` (**ambigu**) | ? |
| 41 | Code de la défense | `LEGITEXT000006071307` (JL+KM) | — | `C. déf.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 42 | Code de l'énergie | `LEGITEXT000023983208` (JL+KM) | — | `C. énergie` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 43 | Code des transports | `LEGITEXT000023086525` (JL+KM) | — | `C. transp.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 44 | Code du patrimoine | `LEGITEXT000006074236` (JL+KM) | — | `C. patr.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 45 | Code du sport | `LEGITEXT000006071318` (JL+KM) | — | `C. sport` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 46 | Code du tourisme ¹ | `LEGITEXT000006074073` (JL+KM) | — | `C. tourisme` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 47 | Code de la voirie routière | `LEGITEXT000006070667` (JL+KM) | — | — | ? |
| 48 | Code de la recherche | `LEGITEXT000006071190` (JL+KM) | — | — | ? |
| 49 | Code de la mutualité | `LEGITEXT000006074067` (JL+KM) | — | `C. mut.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 50 | Code des juridictions financières | `LEGITEXT000006070249` (JL+KM) | — | `CJF` (TIERS [JL]) | ? |
| 51 | Code pénitentiaire | `LEGITEXT000045476241` (JL+KM) | — | — | ? |
| 52 | Code des douanes | `LEGITEXT000006071570` (JL+KM) | — | `C. douanes` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | ? |
| 53 | Code des douanes de Mayotte | `LEGITEXT000006071645` (JL) | — | — | ? |
| 54 | Code forestier (nouveau) | `LEGITEXT000025244092` (JL+KM) | — | `C. for.` (**NON VÉRIFIÉ** [V-159 · abreviations-codes]) | « nouveau » (titre) |
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
| 70 | Code des ports maritimes | `LEGITEXT000006074233` (JL+KM) | — | — | ? (abrogé : **NON VÉRIFIÉ** [V-124 · list-code-etats]) |
| 71 | Code du travail maritime | `LEGITEXT000006072051` (JL+KM) | — | — | ? (abrogé : **NON VÉRIFIÉ** [V-124 · list-code-etats]) |
| 72 | Code disciplinaire et pénal de la marine marchande | `LEGITEXT000006071188` (JL+KM) | — | — | ? |
| 73 | Code des instruments monétaires et des médailles | `LEGITEXT000006070666` (JL) | — | — | ? |
| 74 | Code de déontologie des architectes | `LEGITEXT000006074232` (JL+KM) | — | — | ? |
| 75 | Code des communes | `LEGITEXT000006070162` (JL+KM) | — | — | remplacé par le CGCT (TIERS [JL]) |
| 76 | Code des communes de la Nouvelle-Calédonie | `LEGITEXT000006070300` (JL+KM) | — | — | ? |
| 77 | Code de la famille et de l'aide sociale | `LEGITEXT000006072637` (JL+KM) | — | `CFAS` (WEB) | remplacé par le CASF (TIERS [JL]) |

Titres présents dans [PLF] mais absents du tableau : []
