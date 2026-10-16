# ADR-0013 — Distribution : cargo-dist, MCPB, OCI, signature

- **Statut** : **Proposé**. L'acceptation dépend de Q5 (certificats), de Q1 (nom, avant toute publication immuable) et du spike TLS (tâche 0 de M2, 00 §11.1). Q1 est décidée le 2026-10-16 (nom `mcp-legifrance`).
- **Date** : 2026-10-15
- **Jalons concernés** : M2 (spike TLS, tâche 0), M4 (`dist init`), transverse « Distribution » dès v0.1.0, M8 et M9 (features `http` et `dila`)
- **Questions liées** : Q1 (décidée le 2026-10-16), Q2 (décidée le 2026-10-16), Q5 (et Q4 pour l'éventuel mode HTTP partagé)

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

1. **Un seul binaire** `mcp-legifrance`, construit par **cargo-dist 0.33.0** pour 7 cibles : Linux x86_64 et aarch64 (gnu et musl), macOS x86_64 et aarch64, Windows x86_64 msvc (00 §12). Sont publiés sur GitHub Releases :
   - les archives, avec `sha256.sum` ;
   - les installeurs `sh` et `ps1` ;
   - les attestations de provenance GitHub ;
   - `cargo-auditable` et un SBOM `cargo-cyclonedx`.

   `install-updater = false` ([ADR-0005](0005-pas-de-configuration-distante.md)).
2. **Bundle MCPB unique multi-plateforme**, seul référencé par `server.json`. Il contient les binaires et un lanceur POSIX qui choisit l'architecture. Des bundles par architecture peuvent être publiés en complément, hors registre (08 §4.1).
3. **Image OCI** distroless, `nonroot`, sur `ghcr.io/muchiny/mcp-legifrance` (linux/amd64 et linux/arm64), avec `LABEL io.modelcontextprotocol.server.name`.
4. **`cargo install`** (`registryType: cargo`) : le jeton `mcp-name:` est visible dans le README. Ce canal impose de publier les sept crates `lf-*` ; s'il est jugé trop coûteux, on se limite à `cargo install --git`, sans entrée au registre (08 §12.1 n° 2).
5. **Registre MCP** : nom `io.github.muchiny/mcp-legifrance` (Q1).
   - Publication par `mcp-publisher` avec OIDC GitHub, **après** publication du MCPB, de l'image et du crate.
   - `server.json` déclare `LF_PISTE_CLIENT_ID` et `LF_PISTE_CLIENT_SECRET` en `environmentVariables` avec `isSecret: true`.
6. **Signature selon Q5** (08 §3.8) :

   | Q5 | Windows | macOS |
   |---|---|---|
   | certificat Windows et compte Apple | Authenticode (SSL.com **ou** Azure Artifact Signing) | `macos-sign = true` et notarisation par un job dédié (`notarytool`) |
   | certificat Windows seul | signé | non signé ; installeur shell recommandé |
   | compte Apple seul | non signé, avec avertissement et repli OCI ou WSL | signé et notarisé |
   | aucun (défaut de 00 §13) | non signé | non signé, avec avertissement en tête du README et des notes de release |

   Ne **jamais** conseiller de désactiver Smart App Control ou Gatekeeper.
7. **Fournisseur TLS** : il sera consigné ici après le spike TLS de M2. Règle de choix : si aws-lc impose cmake ou NASM sur les postes des utilisateurs (`cargo install`) ou fait échouer une cible musl, on retient `ring` ; sinon, on garde le défaut aws-lc.
8. **Chaîne CI durcie** (corrige D-44) :
   - actions épinglées par SHA ;
   - aucune clé PISTE dans les artefacts ;
   - versions alignées : crate, verrou des descripteurs, `server.json` et manifeste MCPB, vérifiés par `release-gates`.
9. **Licence** : Apache-2.0, avec un `NOTICE` qui crédite la référence et ses fixtures (Q2, décidée le 2026-10-16 ; titulaire « Loïc Wernert » dans NOTICE ; la vérification V-214 sur le crédit du titulaire de la référence reste ouverte). Les données sont sous Licence Ouverte 2.0, avec la mention « Légifrance / DILA ».

## Conséquences

### Positives

- **Installation en un clic** (MCPB) pour des juristes non développeurs, plus des canaux pour les développeurs (cargo) et pour l'isolation (OCI).
- Un binaire signé n'est pas bloqué par Smart App Control (si Q5 le permet). Intégrité vérifiable par les sommes, les attestations et le SBOM.

### Négatives

- **Coût et dépendance externe** : certificat EV ou Azure Artifact Signing, compte Apple Developer payant. Sans eux, l'expérience Windows est dégradée.
- **Versions immuables au registre** : une erreur de publication coûte une nouvelle version ; d'où les vérifications bloquantes avant publication.
- Pipeline de release plus complexe : job de notarisation personnalisé, job `build-mcpb`, publication OCI.
- Le paquet `lf-app` donne son nom aux archives (`lf-app-<triple>`). Le renommer en `mcp-legifrance` est recommandé avec Q1 (08 §12.1 n° 1).

### À surveiller

- Points **NON VÉRIFIÉS** :
  - [v:sac-signature-suffit] : un binaire signé est-il accepté par Smart App Control, et un binaire non signé bloqué ;
  - [v:tls-fournisseur-cross] : aws-lc et ring sur les 7 cibles ;
  - [v:mcpb-multi-architecture] : bundle unique avec lanceur ;
  - [v:registre-mcp-cargo] : `registryType: cargo` accepté en production ;
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
| MCPB par architecture seulement | `server.json` et MCPB n'ont pas de champ architecture : un client ne saurait pas lequel installer (08 §4.1). |
| Binaire universel macOS (`lipo`) | Alternative possible, mais la conservation des signatures est **NON VÉRIFIÉE** [v:lipo-signatures]. |
| MSI, Homebrew, winget, deb ou rpm dès v0.1 | Hors périmètre v0.1. Possibles plus tard (08 §1.3, §12.2). |
| Programme de mise à jour intégré | Appel réseau non sollicité ([ADR-0005](0005-pas-de-configuration-distante.md)). |

## Références et sources

- Plan : [00-decisions.md](../plan/00-decisions.md) §11 (ligne « Transverse Distribution »), §12, §13 (Q1, Q2, Q5) ; [08-distribution.md](../plan/08-distribution.md) §0 à §7, §12 ; [04-adapter-piste.md](../plan/04-adapter-piste.md) §4.1 ; [06-qualite.md](../plan/06-qualite.md) §7, §9 ; [09-risques-et-questions.md](../plan/09-risques-et-questions.md) (risques TLS et Smart App Control).
- Recherche : [01-parite.md](../recherche/01-parite.md) §12 ; [06-mcp-spec.md](../recherche/06-mcp-spec.md) §9 ; [04-rmcp-rust.md](../recherche/04-rmcp-rust.md) §4.1, §4.2, §6 (points 5 et 6) ; [05-piste-dila.md](../recherche/05-piste-dila.md) §4 ; [02-defauts.md](../recherche/02-defauts.md) D-44.
- Référence Python (`d3b5b13`) : `src/fr_eli_mcp/__main__.py:1-9` ; `README.md:3, 22-42` ; `Dockerfile:1-5` ; `.github/workflows/release.yml:1-91`.
- Outils : https://github.com/axodotdev/cargo-dist (tag `v0.33.0`) ; https://github.com/modelcontextprotocol/mcpb ; https://github.com/modelcontextprotocol/registry.
