# Confidentialité

Aucun programme du dépôt ne contacte encore le réseau. Cette page décrit le serveur prévu (`docs/plan/08-distribution.md` §1.4 et §9.8) ; elle sera complétée en T-MD-13 (rubrique `privacy_policies` du bundle MCPB).

## Données envoyées

Les requêtes de recherche et les identifiants de documents, avec les identifiants PISTE, partent vers l'API Légifrance (DILA) par PISTE (AIFE). Rien d'autre.

## Données conservées localement

Un journal d'audit JSONL dans `LF_AUDIT_DIR`, avec des **empreintes** SHA-256 des entrées, jamais les requêtes en clair (00 §7).

## Ni télémétrie, ni compte, ni service tiers

Aucune télémétrie, aucun compte, aucun service tiers (ADR-0005). Chaque utilisateur apporte ses identifiants.

## Hôtes contactés

| Composant | Hôtes | Quand |
|---|---|---|
| Binaire, `LF_PISTE_ENV=sandbox` (défaut) | `sandbox-oauth.piste.gouv.fr`, `sandbox-api.piste.gouv.fr` | Au premier appel d'outil, jamais au démarrage |
| Binaire, `LF_PISTE_ENV=production` | `oauth.piste.gouv.fr`, `api.piste.gouv.fr` | Au premier appel d'outil |
| Surcharges `LF_PISTE_OAUTH_URL` / `LF_PISTE_BASE_URL` | L'hôte fourni (`https` obligatoire ; `http` admis vers 127.0.0.1, ::1 et localhost seulement) | Au premier appel d'outil ; avertissement au démarrage hors des quatre hôtes officiels |
| M9 (`dila`) | Aucun | Jamais |
| Script de téléchargement des dumps (M9) | `echanges.dila.gouv.fr` | À l'exécution du script |
| Installeurs sh/ps1 | `github.com` et l'hôte de redirection des assets | À l'installation |
| `cargo install --git` | `github.com`, `index.crates.io`, `static.crates.io` | À l'installation |
| Image OCI | `ghcr.io` | Au téléchargement de l'image |

Derrière un proxy d'entreprise, le binaire utilisera le proxy système.

## Contacts

Issues du dépôt, ou signalement privé décrit dans [SECURITY.md](SECURITY.md). Pour les données traitées par PISTE et Légifrance : DILA, `rgpd@dila.gouv.fr`.
