# Décisions d'architecture (ADR) — `mcp-legifrance`

> Index et mode d'emploi des *Architecture Decision Records* du projet `mcp-legifrance`. Il s'agit de la réécriture en Rust, en architecture hexagonale, du serveur MCP Python « fr-eli-mcp » (API Légifrance via PISTE). Les ADR s'adressent à des développeurs, humains ou agents, qui n'ont pas eu accès aux échanges qui ont mené aux décisions.

## 1. Rôle des ADR

- [00-decisions.md](../plan/00-decisions.md) est **normatif** (révision 2 du 2026-10-15, amendements AM-01 à AM-43 journalisés en 00 §14, avec les corrections du contrôle final du 2026-10-16). Il fixe ce qui est décidé : noms, crates, ports, outils, erreurs, protocole, jalons M0 à M9, questions Q1 à Q15 (00 §13 ; contexte, options et réponses : 09 §3). Un ADR dit **pourquoi** : le contexte sourcé, les options écartées et le prix à payer. Il ne contredit jamais 00. Si une source semble justifier un autre choix, l'ADR applique la décision et signale l'écart (section « À surveiller » ou « Alternatives »).
- Les documents du plan ([02-architecture.md](../plan/02-architecture.md) à [09-risques-et-questions.md](../plan/09-risques-et-questions.md), annexes A à D) détaillent la mise en œuvre et renvoient aux ADR par leur numéro (`ADR-00NN`).
- Un ADR est **requis** (06 §13.2, [06-qualite.md](../plan/06-qualite.md)) pour :
  - toute modification d'une décision de 00 ;
  - toute nouvelle dépendance structurante : SDK, TLS, base de données, crate de validation de jetons en M8 ;
  - tout changement de protocole servi ;
  - tout changement de posture de sécurité : mode d'audit, empreinte salée, mode HTTP partagé.

## 2. Cycle de vie

```mermaid
stateDiagram-v2
    state "Proposé" as Propose
    state "Accepté" as Accepte
    state "Rejeté" as Rejete
    state "Déprécié" as Deprecie
    state "Remplacé par ADR-NNNN" as Remplace
    [*] --> Propose
    Propose --> Accepte: revue humaine, questions liées tranchées ou proposition par défaut adoptée
    Propose --> Rejete: alternative retenue
    Accepte --> Deprecie: décision devenue sans objet
    Accepte --> Remplace: nouvel ADR qui le remplace
```

| Statut | Sens | Règle |
|---|---|---|
| **Proposé** | décision rédigée dont le contenu attend encore une condition (question Q, spike, mesure) qui le complétera | peut encore être modifié sur le fond ; le code qui l'applique peut exister derrière une configuration par défaut prudente |
| **Accepté** | décision en vigueur, éventuellement sur la proposition adoptée par défaut d'une question encore à confirmer | doit être accepté **avant** la fusion du code qui l'applique ; n'est plus modifié sur le fond : on le remplace |
| **Déprécié** | décision sans objet (fonction retirée) | reste dans l'index |
| **Remplacé par ADR-NNNN** | une décision plus récente le remplace | lien dans les deux sens |
| Rejeté | proposition abandonnée | conservé pour mémoire |

Corrections admises sur un ADR accepté : coquilles, liens, ajout d'une référence. Tout le reste passe par un nouvel ADR. Dans la même PR, on coche la case « Décision de 00 ou d'un ADR modifiée ? » du modèle de PR (06 §13.1) et on met à jour 00-decisions.md.

**Questions adoptées par défaut.** Chaque question Q1 à Q15 porte une proposition adoptée par défaut, appliquée dans 00 et dans le plan jusqu'à la réponse de l'utilisateur (00 §13). Un ADR peut être accepté sur cette proposition : la question reste « à confirmer » et figure au §5. Sans réponse de l'utilisateur à l'ouverture de M0, M0 démarre sous le nom actuel du dépôt avec les propositions par défaut de Q1 à Q15 ; le renommage éventuel du dépôt (Q1) se fait ensuite, redirection GitHub vérifiée, et en tout cas avant toute publication immuable (00 §13). Une réponse conforme est consignée dans 09 §3 et ajoutée à l'ADR comme référence (correction admise) ; une réponse contraire produit un nouvel ADR qui remplace l'ancien (09 §3). De même, un spike ou une mesure qui invalide un ADR accepté produit un nouvel ADR (par exemple le repli `Parameters<T>` d'ADR-0008). Un ADR reste « Proposé » quand une partie de son contenu n'est pas encore écrite : choix TLS d'ADR-0013, défaut et mesures d'ADR-0014.

**Révision du 2026-10-15.** Avant l'ouverture de M0, aucun code n'applique encore ces ADR. La revue du plan les a donc alignés en place sur 00 révision 2, au lieu de les remplacer. Chaque ADR modifié porte, sous son en-tête, une ligne « Révisé le 2026-10-15 (revue) : … » qui résume les changements et cite les amendements. Les alignements du contrôle final du 2026-10-16 (00 §14, sans nouvel amendement) sont ajoutés à la fin de cette même ligne, après « contrôle final du 2026-10-16 : ». À partir de l'ouverture de M0, la règle ci-dessus s'applique sans exception.

## 3. Conventions

- Fichier `docs/adr/NNNN-titre-en-kebab-case.md`, numéro sur 4 chiffres. Ce plan crée les ADR 0001 à 0014 ; la numérotation continue à 0015 et un numéro n'est **jamais** réutilisé (06 §13.2).
- Langue : français ; identifiants techniques tels quels.
- Longueur : 40 à 120 lignes. Un ADR est court ; le détail est dans le plan, vers lequel il renvoie.
- Sources : chaque fait non trivial est sourcé, par exemple (recherche 03 §4), (référence `citations.py:309`), (fixture `consult_juri_cetat.json`) ou une URL. Les défauts de la référence sont cités `D-01…D-47` ([recherche 02](../recherche/02-defauts.md)), les points de parité `P-xxx` et les écarts `E-xx` ([recherche 01](../recherche/01-parite.md)).
- Ce qui n'est pas vérifié est marqué **NON VÉRIFIÉ** et suivi d'une étiquette `[v:…]`. Les étiquettes sont reprises dans le registre V-xx de [09-risques-et-questions.md](../plan/09-risques-et-questions.md). Un ADR emploie l'étiquette de référence, jamais un alias (09 §2.1, index §2.19) ; au remplacement des étiquettes (T-M0-12), le marqueur prend la forme finale de 09 §2.19.
- Les versions de crates citées (par exemple moka 0.12.16, fs4 1.1, chrono-tz 0.10.4) sont des **versions de référence** relevées les 2026-10-15 et 2026-10-16, pas des planchers : `[workspace.dependencies]` écrit l'exigence au niveau mineur, sauf `rmcp` et `rmcp-macros`, épinglés `"=3.5.1"`, et le verrou initial retient la plus haute version éligible à la règle d'âge (00 §2).
- Tout extrait de code Rust est « non compilé, à valider en M0 ».

## 4. Modèle

```markdown
# ADR-00NN — Titre court à l'infinitif ou nominal

- **Statut** : Proposé | Accepté | Déprécié | Remplacé par ADR-NNNN
- **Date** : AAAA-MM-JJ
- **Jalons concernés** : M…
- **Questions liées** : Q… (préciser « adoptée par défaut, à confirmer » tant que l'utilisateur n'a pas répondu)
- Révisé le AAAA-MM-JJ (motif) : … (une ligne par révision de fond d'un ADR proposé, ou pour la révision du 2026-10-15 décrite au §2)

## Contexte
Faits sourcés (D-xx, P-xxx, E-xx, rapports de recherche, URL), contraintes, ce qui n'est pas vérifié.

## Décision
Ce qui est décidé, sous forme de règles vérifiables ; renvoi à 00-decisions.md et au document du plan qui détaille.

## Conséquences
### Positives
### Négatives
### À surveiller
Points NON VÉRIFIÉS [v:…], mesures à faire, conditions de révision.
### Tests qui la gardent

## Alternatives envisagées
| Option | Raison du rejet |

## Références et sources
Documents du plan, rapports de recherche, référence Python, URL.
```

## 5. Index

Toutes les questions de la colonne « Questions » sont adoptées par défaut et restent à confirmer (00 §13, 09 §3) ; « Révisé » renvoie à la révision du 2026-10-15 (§2).

| ADR | Titre | Statut | Jalons | Questions | Documents principaux |
|---|---|---|---|---|---|
| [ADR-0001](0001-reecriture-en-rust.md) | Réécriture en Rust | Accepté (révisé) | M0 → M9 | Q1, Q2, Q6 | [01-existant](../plan/01-existant.md), [A-parite](../plan/annexes/A-parite.md) |
| [ADR-0002](0002-hexagonal-workspace.md) | Architecture hexagonale en workspace multi-crates | Accepté (révisé) | M0, M3 | Q1, Q13 | [02-architecture](../plan/02-architecture.md) |
| [ADR-0003](0003-mcp-2026-07-28-dual-era-rmcp.md) | MCP 2026-07-28 en dual-era avec `rmcp =3.5.1` | Accepté (révisé) | M0, M4, M7, M8 | Q4, Q7, Q8, Q14 | [annexe D](../plan/annexes/D-mcp-2026-07-28.md), [05-outils-mcp](../plan/05-outils-mcp.md) |
| [ADR-0004](0004-piste-puis-dila.md) | PISTE d'abord, DILA ensuite derrière le même port | Accepté (révisé) | M2, M9 | Q3, Q6, Q15 | [04-adapter-piste](../plan/04-adapter-piste.md), [annexe C](../plan/annexes/C-api-legifrance.md) |
| [ADR-0005](0005-pas-de-configuration-distante.md) | Aucune configuration distante ni télémétrie | Accepté (révisé) | M2, M4 | — | [02-architecture](../plan/02-architecture.md) §8.2, [08-distribution](../plan/08-distribution.md) §1.4 |
| [ADR-0006](0006-sortie-compacte-retour-progressif.md) | Sortie compacte via `structuredContent` et retour progressif | Accepté (révisé) | M4, M5 | Q11 | [05-outils-mcp](../plan/05-outils-mcp.md) §1, §6 |
| [ADR-0007](0007-cache-memoire-port.md) | Cache mémoire au niveau du port | Accepté (révisé) | M3, M9 | — | [02-architecture](../plan/02-architecture.md) §7.2 |
| [ADR-0008](0008-audit-decorateur-strict.md) | Audit obligatoire en décorateur, strict par défaut | Accepté (révisé) | M3, M4, M7 | Q9 | [02-architecture](../plan/02-architecture.md) §7.1, [06-qualite](../plan/06-qualite.md) §11 |
| [ADR-0009](0009-contrat-de-citation-v2.md) | Contrat de citation v2 et corrections typographiques | Accepté (révisé) | M1, M2, M4, M6 | Q12 | [03-domaine](../plan/03-domaine.md) §5 |
| [ADR-0010](0010-erreurs-in-band.md) | Erreurs métier in-band `[code]` | Accepté (révisé) | M2, M3, M4 | — | [02-architecture](../plan/02-architecture.md) §9, [05-outils-mcp](../plan/05-outils-mcp.md) §7 |
| [ADR-0011](0011-dates-chrono-paris.md) | Dates : chrono, Europe/Paris, sentinelles | Accepté (révisé) | M1, M2, M3 | — | [03-domaine](../plan/03-domaine.md) §6, [04-adapter-piste](../plan/04-adapter-piste.md) §6.3 |
| [ADR-0012](0012-ports-async-rpitit.md) | Ports en async natif (RPITIT) et génériques | Accepté (révisé) | M0, M3, M9 | — | [02-architecture](../plan/02-architecture.md) §5, §7.1, §10 |
| [ADR-0013](0013-distribution.md) | Distribution : cargo-dist, MCPB, OCI, signature | **Proposé** (révisé ; Q5, Q1, Q13, spike TLS M0) | M0, transverse dès v0.1.0, v1.0.0 | Q1, Q2, Q5, Q13, Q15 | [08-distribution](../plan/08-distribution.md) |
| [ADR-0014](0014-recherche-mode-pagination.md) | Recherche : mode par défaut et pagination | **Proposé** (révisé ; Q10 avant le verrou de M4, mesures M5) | M2, M3, M4, M5 | Q10 | [05-outils-mcp](../plan/05-outils-mcp.md) §5.1, [04-adapter-piste](../plan/04-adapter-piste.md) §6 |

Conditions d'acceptation des ADR proposés :

- [ ] **ADR-0013** : réponse à Q5 (proposition par défaut : Azure Artifact Signing si éligible, sinon SSL.com eSigner ; compte Apple Developer pour `codesign` et `notarytool` ; bundle MCPB unique), avant v0.1.0 ; réponse à Q1 (nom du produit, du dépôt et du paquet, avant toute publication immuable) ; réponse à Q13 (proposition par défaut : `cargo install --git` en v0.x, sans crates.io), avant v0.1.0 ; résultat du spike TLS de M0 (aws-lc ou ring) consigné dans l'ADR.
- [ ] **ADR-0014** : réponse à Q10 (proposition par défaut : `any` jusqu'à V-79, puis `all` si V-79 confirme la sémantique et un rappel acceptable sur les 9 scénarios de la référence), consignée avant le verrou des descripteurs de M4 ; mesures de M5 sur la pagination profonde (V-83) et sur `typePagination` en page 2 (V-82).

Questions et vérifications à confirmer des ADR acceptés (une réponse contraire produit un nouvel ADR, §2) :

- [ ] **ADR-0001** : Q1 (réponse avant le premier commit de M0 ; sans réponse, démarrage sous le nom actuel et renommage éventuel ensuite, en tout cas avant toute publication immuable) et Q2 (premier commit de M0) ; Q6 (réseau de construction), avant M0.
- [ ] **ADR-0002** : Q1 (paquet `mcp-legifrance`), réponse avant le premier commit de M0 ; Q13 (canal `cargo install`), avant v0.1.0, la publication des crates étant réexaminée à v1.0.0.
- [ ] **ADR-0003** : spikes V-28 et V-29 en M0 ; Q7 et Q14 avant M4 ; Q8 en M4 (listes) et M7 (resources) ; V-34 et V-35 en M4 ; Q4 avant M8.
- [ ] **ADR-0004** : Q3 (sandbox avant M2, production avant v0.1.0) ; Q15 avant v0.1.0 ; Q6 (DILA) avant M9.
- [ ] **ADR-0006** : Q11 (V-53), avant v0.1.0.
- [ ] **ADR-0008** : Q9 avant M3 ; spike V-28 (`JsonObject`) en M0.
- [ ] **ADR-0009** : Q12 en M1, avant la génération des golden.

## 6. Dépendances entre décisions

```mermaid
graph LR
    A1[0001 Rust] --> A2[0002 hexagonal]
    A2 --> A12[0012 RPITIT]
    A2 --> A7[0007 cache]
    A2 --> A8[0008 audit]
    A1 --> A3[0003 MCP dual-era]
    A3 --> A6[0006 sortie compacte]
    A3 --> A10[0010 erreurs in-band]
    A6 --> A14[0014 recherche]
    A6 --> A9[0009 citation v2]
    A12 --> A4[0004 PISTE puis DILA]
    A11[0011 dates] --> A7
    A5[0005 pas de config distante] --> A13[0013 distribution]
    A8 --> A10
```

## 7. Sources

- [00-decisions.md](../plan/00-decisions.md) (normatif, révision 2), §0 à §14.
- [06-qualite.md](../plan/06-qualite.md) §13.1 (modèle de PR, revue renforcée) et §13.2 (gabarit, numérotation, règle « accepté avant la fusion »).
- [02-architecture.md](../plan/02-architecture.md) §13 (décisions, alternatives écartées et renvois aux ADR) et §13.1 (divergences entre sources).
- [09-risques-et-questions.md](../plan/09-risques-et-questions.md) (registres R-xx, V-xx dont les étiquettes des ADR, questions Q1 à Q15 et leurs réponses au §3).
- Rapports de recherche : [README](../recherche/README.md) et rapports 01 à 09.
- Michael Nygard, « Documenting Architecture Decisions » (2011) : https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions — origine du format.
