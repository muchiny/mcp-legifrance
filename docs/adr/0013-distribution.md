# ADR-0013 — Distribution : cargo-dist, MCPB, OCI, signature

- **Statut** : **Proposé**. L'acceptation dépend de Q5 (certificats, avant v0.1.0), de Q1 (nom, avant toute publication immuable), de Q13 (canal `cargo install`, avant v0.1.0) et du spike TLS de M0. Les propositions adoptées par défaut de ces questions sont appliquées d'ici là (00 §13).
- **Date** : 2026-10-15
- **Jalons concernés** : M0 (spike TLS, `dist init`), transverse « Distribution » dès v0.1.0, M8 et M9 (features `http` et `dila`), v1.0.0 (réexamen de la publication crates.io)
- **Questions liées** : Q1, Q2, Q5, Q13 (et Q4 pour l'éventuel mode HTTP partagé, Q15 pour la mention d'attribution)
- Révisé le 2026-10-15 (revue) : paquet Cargo `mcp-legifrance` (AM-32, Q1) ; `cargo install --git` en v0.x, sans crates.io (AM-33, Q13) ; signature et notarisation par défaut, bundle MCPB unique sans bundle par architecture (AM-34, Q5) ; attribution des données (AM-39, Q15).

## Contexte

- **Chaîne de la référence.** Elle publie un paquet PyPI (`uvx`) et une image `python:3.11-slim` exécutée en root, sans version épinglée. Elle télécharge `mcp-publisher` en `latest` par `curl | tar`, sans somme de contrôle, avec des actions épinglées par tag (D-44 ; recherche 01 §12).
- **Smart App Control.** Il bloque les exécutables non signés « sans demander et sans exception par programme » : `fr-eli-mcp.exe`, `uvx.exe`, `pip.exe` (référence `__main__.py:1-9`, `README.md:22-42`). Un binaire Rust n'a pas d'interpréteur signé derrière lequel s'abriter (08 §3.2).
- **Registre MCP** (recherche 01 §12 ; recherche 06 §9 ; 08 §6) :
  - `registryType` `mcpb` : asset de release dont l'URL contient « mcp », avec `fileSha256` obligatoire ;
  - `registryType` `oci` : label `io.modelcontextprotocol.server.name` ;
  - `registryType` `cargo` : jeton `mcp-name:` en **texte visible** dans le README, car crates.io supprime les commentaires HTML ;
  - `environmentVariables` avec `isSecret`.

  La galerie de VS Code n'installe que `oci` (08 §12.1 n° 10).
- **cargo-dist 0.33.0** (08 §2, §3) :
  - il signe sous Windows avec SSL.com eSigner **ou** Azure Artifact Signing, les deux sur HSM en nuage ;
  - il signe sous macOS (option expérimentale), mais **ne notarise pas** (`sign/macos.rs:21`) ;
  - MCPB n'a **aucun sélecteur d'architecture** : `platform_overrides` ne connaît que `darwin`, `win32` et `linux` (08 §4.1).
- **TLS.** `reqwest` 0.13 utilise rustls avec aws-lc par défaut. Compiler `aws-lc-sys` en croisé, notamment en musl, peut exiger cmake et NASM. L'option B est `rustls-no-provider` + `ring` (08 §2.4, recherche 04 §4.2).
- **CGU.** Les clés sont « strictement réservées à votre application » : aucune clé de démonstration ne peut être distribuée (recherche 05 §4.1).

## Décision (proposée)

1. **Un seul binaire** `mcp-legifrance`, construit par **cargo-dist 0.33.0** pour 7 cibles : Linux x86_64 et aarch64 (gnu et musl), macOS x86_64 et aarch64, Windows x86_64 msvc (00 §12).
   - Le paquet Cargo du binaire s'appelle `mcp-legifrance` (exception au préfixe `lf-`, répertoire `crates/lf-app`, 00 §2), car dist nomme archives et installeurs d'après le paquet, soit `mcp-legifrance-<triple>` (proposition adoptée par défaut, Q1).
   - Q1 est tranchée avant toute publication immuable (registre MCP, MCPB, crates.io, GHCR), au vu de l'homonymie avec `Ktulu-Analog/mcp-legifrance` (AGPL) et du statut de la marque « Légifrance » (V-215).

   Sont publiés sur GitHub Releases :
   - les archives, avec `sha256.sum` ;
   - les installeurs `sh` et `ps1` ;
   - les attestations de provenance GitHub ;
   - `cargo-auditable` et un SBOM `cargo-cyclonedx`.

   `install-updater = false` ([ADR-0005](0005-pas-de-configuration-distante.md)).
2. **Bundle MCPB unique multi-plateforme** par version (registryType `mcpb`, asset de release dont l'URL contient « mcp », `fileSha256`), seul référencé par `server.json`. Il contient les binaires et un lanceur POSIX qui choisit le binaire selon `uname -m`, avec l'override `win32` pour Windows. **Aucun bundle par architecture**, même hors registre, tant que MCPB n'a pas de sélecteur d'architecture (00 §12, V-197 ; 08 §4.1).
3. **Image OCI** distroless, `nonroot`, sur `ghcr.io/muchiny/mcp-legifrance` (linux/amd64 et linux/arm64), avec `LABEL io.modelcontextprotocol.server.name`.
4. **Canal Cargo** (00 §12, adoptée par défaut, Q13) : en v0.x, le README documente `cargo install --git https://github.com/<dépôt> --tag vX.Y.Z --locked mcp-legifrance` (nom du paquet requis, le dépôt en contenant plusieurs, 08 §6.4). Ni publication crates.io, ni paquet `cargo` dans `server.json`, ni jeton `mcp-name:` dans le README.
   - La publication des crates (les six `lf-*` et `mcp-legifrance`, dont les API internes deviendraient des engagements semver publics) et le `registryType: cargo` sont réexaminés à v1.0.0 (V-172, V-207 ; 08 §12.1 n° 2).
5. **Registre MCP** : nom `io.github.muchiny/mcp-legifrance` (Q1).
   - Publication par `mcp-publisher` avec OIDC GitHub, **après** publication du MCPB et de l'image.
   - `server.json` déclare `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` en `environmentVariables` avec `isSecret: true`.
6. **Signature selon Q5** (00 §12, 08 §3.8) :

   | Q5 | Windows | macOS |
   |---|---|---|
   | certificat Windows et compte Apple (proposition adoptée par défaut, 00 §12-§13) | Authenticode (Smart App Control) par Azure Artifact Signing si le projet est éligible, sinon SSL.com eSigner, seuls fournisseurs pris en charge par dist | `codesign` (`macos-sign = true`, option expérimentale de dist) puis notarisation par un job dédié (`notarytool`) |
   | certificat Windows seul | signé | non signé ; installeur shell recommandé |
   | compte Apple seul | non signé, avec avertissement et repli OCI ou WSL | signé et notarisé |
   | aucun (repli si aucun certificat n'est obtenu) | non signé | non signé, avec avertissement en tête du README et des notes de release ; repli OCI ou WSL documenté |

   - Ne **jamais** conseiller de désactiver Smart App Control ou Gatekeeper.
   - Tant que Q5 n'est pas tranchée, `dist-workspace.toml` n'active aucune signature ; Q5 n'est pas un critère de sortie de M0 (échéance : avant v0.1.0, 08 §2.2, §10).
7. **Fournisseur TLS** : il sera consigné ici après le spike M0. Règle de choix : si aws-lc impose cmake ou NASM sur les postes des utilisateurs (`cargo install`) ou fait échouer une cible musl, on retient `ring` ; sinon, on garde le défaut aws-lc.
8. **Chaîne CI durcie** (corrige D-44) :
   - actions épinglées par SHA ;
   - aucune clé PISTE dans les artefacts ;
   - versions alignées : crate, verrou des descripteurs, `server.json` et manifeste MCPB, vérifiés par `release-gates`.
9. **Licence** : Apache-2.0, avec un `NOTICE` qui crédite la référence et ses fixtures (Q2). Les données sont sous Licence Ouverte 2.0 : `fr_coverage`, les instructions serveur et le README portent « Source : Légifrance / DILA — Licence Ouverte 2.0 » et la date de consultation (00 §12, adoptée par défaut, Q15).

## Conséquences

### Positives

- **Installation en un clic** (MCPB) pour des juristes non développeurs, plus des canaux pour les développeurs (cargo) et pour l'isolation (OCI).
- Un binaire signé n'est pas bloqué par Smart App Control (si Q5 le permet). Intégrité vérifiable par les sommes, les attestations et le SBOM.

### Négatives

- **Coût et dépendance externe** : certificat EV ou Azure Artifact Signing, compte Apple Developer payant. Sans eux, l'expérience Windows est dégradée.
- **Versions immuables au registre** : une erreur de publication coûte une nouvelle version ; d'où les vérifications bloquantes avant publication.
- Pipeline de release plus complexe : job de notarisation personnalisé, job `build-mcpb`, publication OCI.
- Le paquet du binaire déroge au préfixe `lf-` des crates (00 §1) : sans cela, dist nommerait les archives `lf-app-<triple>` et la commande serait `cargo install lf-app` (08 §12.1 n° 1).
- En v0.x, `cargo install --git` compile chez l'utilisateur, avec la toolchain et la chaîne C du fournisseur TLS retenu ; le binaire obtenu n'est pas signé.

### À surveiller

- Points **NON VÉRIFIÉS** :
  - [v:sac-signature-suffit] : un binaire signé est-il accepté par Smart App Control, et un binaire non signé bloqué ;
  - [v:tls-fournisseur-cross] : aws-lc et ring sur les 7 cibles ;
  - [v:mcpb-multi-architecture] : bundle unique avec lanceur ;
  - [v:registre-mcp-cargo] : `registryType: cargo` accepté en production (réexamen à v1.0.0, Q13) ;
  - [v:cargo-publish-workspace] : publication ordonnée des sept crates (réexamen à v1.0.0, Q13) ;
  - [v:marque-legifrance] : statut de la marque « Légifrance », avant toute publication immuable (Q1) ;
  - [v:macos-notarisation-binaire-nu] : notarisation d'un exécutable nu ;
  - [v:cargo-dist-macos-sign] : signature macOS expérimentale de dist ;
  - [v:azure-artifact-signing-eligibilite] : conditions d'éligibilité ;
  - [v:taille-binaire] : taille des binaires et du MCPB.

### Tests qui la gardent

- `dist plan` en PR.
- `release-gates` : verrou, versions alignées, CHANGELOG, licences.
- `Get-AuthenticodeSignature` sur l'exécutable Windows, et `codesign --verify` sur macOS.
- Exécution d'un `initialize` sur chaque binaire du MCPB (08 §4.5, §7.5).

## Alternatives envisagées

| Option | Raison du rejet |
|---|---|
| Paquet PyPI ou `uvx` | Le produit est un binaire Rust (00 §12). |
| Image Docker seule | Exclut Claude Desktop (MCPB) et les postes sans Docker. |
| Workflows GitHub Actions écrits à la main, sans cargo-dist | Réinvente archives, installeurs, sommes et attestations. Plus de code de CI à maintenir. |
| MCPB par architecture, seuls ou en complément hors registre | `server.json` et MCPB n'ont pas de champ architecture : un client ne saurait pas lequel installer (00 §12, V-197, 08 §4.1). |
| Publier les sept crates sur crates.io dès v0.1 (`registryType: cargo`) | Engagement semver public sur des API internes, limites de création de crates ; `cargo install --git` suffit en v0.x (00 §12, Q13 ; réexamen à v1.0.0). |
| Garder le paquet `lf-app` (règle du préfixe sans exception) | Archives, installeurs et commande `cargo install` porteraient un nom autre que celui du produit (08 §12.1 n° 1). |
| Binaire universel macOS (`lipo`) | Alternative possible, mais la conservation des signatures est **NON VÉRIFIÉE** [v:lipo-signatures]. |
| MSI, Homebrew, winget, deb ou rpm dès v0.1 | Hors périmètre v0.1. Possibles plus tard (08 §1.3, §12.2). |
| Programme de mise à jour intégré | Appel réseau non sollicité ([ADR-0005](0005-pas-de-configuration-distante.md)). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §1, §2, §11 (ligne « Transverse Distribution »), §12, §13 (Q1, Q2, Q5, Q13, Q15), §14 (AM-32 à AM-34) ; [08-distribution.md](../plan/08-distribution.md) §0 à §7, §12 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §4.1 ; [06-qualite.md](../plan/06-qualite.md) §7, §9 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) (risques TLS et Smart App Control).
- Recherche : [01-parite.md](../recherche/01-parite.md) §12 ; [06-mcp-spec.md](../recherche/06-mcp-spec.md) §9 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §4.1, §4.2, §6 (points 5 et 6) ; [05-piste-dila.md](../recherche/05-piste-dila.md) §4 ; [02-defauts.md](../recherche/02-defauts.md) D-44.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/__main__.py:1-9` ; `README.md:3, 22-42` ; `Dockerfile:1-5` ; `.github/workflows/release.yml:1-91`.
- Outils : https://github.com/axodotdev/cargo-dist (tag `v0.33.0`) ; https://github.com/modelcontextprotocol/mcpb ; https://github.com/modelcontextprotocol/registry.
