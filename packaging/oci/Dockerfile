# syntax=docker/dockerfile:1
# packaging/oci/Dockerfile : image OCI de mcp-legifrance (08 §5.2, T-MD-05). Contexte de build :
# la racine du dépôt (`docker build -f packaging/oci/Dockerfile .`).
#
# Deux voies vers le binaire, choisies par BIN_STAGE :
#   bin-source   (défaut) compile depuis les sources : build local, annuaires tiers ;
#   bin-prebuilt (CI)     reprend les binaires musl de la release, déjà construits et attestés
#                         par dist, posés dans packaging/oci/prebuilt/{amd64,arm64}/ par
#                         publish-oci.yml. Aucune étape RUN dans ce chemin : pas d'émulation.
#
# Images épinglées par digest d'index multi-architecture (V-203 : relevés le 2026-10-29 ; à
# relever à chaque release, `docker buildx imagetools inspect <image>`).
ARG RUST_IMAGE=docker.io/library/rust:1.99.0-slim-bookworm@sha256:2c3a22f0a5533ea2dd5a16627bc841228151faa2d4de2644ac9987e4a2f1f2fa
ARG RUNTIME_IMAGE=gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
ARG BIN_STAGE=bin-source

# --- Binaire compilé depuis les sources --------------------------------------------------------
FROM ${RUST_IMAGE} AS bin-source
RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# Mêmes drapeaux que dist (smoke-targets.yml) : binaire musl statique. cc 1.6.0 ne cherche que
# `aarch64-linux-musl-gcc` pour aarch64 musl : CC_<cible> force `musl-gcc` (tls-spike.yml).
# aws-lc-sys (option A, ADR-0013) se construit avec ce seul compilateur C ; cmake n'est ajouté que
# si la matrice l'exige [V-02 · tls-fournisseur-cross].
RUN set -eu; \
    case "$(uname -m)" in \
      x86_64) T=x86_64-unknown-linux-musl ;; \
      aarch64) T=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported build architecture: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    rustup target add "$T"; \
    export "CC_$(echo "$T" | tr '-' '_')=musl-gcc"; \
    RUSTFLAGS="-C target-feature=+crt-static -C link-self-contained=yes" \
      cargo build --locked --profile dist --target "$T" -p mcp-legifrance; \
    cp "target/$T/dist/mcp-legifrance" /mcp-legifrance

# --- Binaire de la release (CI) ----------------------------------------------------------------
FROM scratch AS bin-prebuilt
ARG TARGETARCH
COPY packaging/oci/prebuilt/${TARGETARCH}/mcp-legifrance /mcp-legifrance

FROM ${BIN_STAGE} AS bin

# --- Image finale ------------------------------------------------------------------------------
FROM ${RUNTIME_IMAGE}
ARG VERSION=0.0.0-dev
ARG REVISION=unknown
# Le registre MCP lit ce label dans la configuration de l'image, en accès anonyme : l'image doit
# être publique (08 §5.1).
LABEL io.modelcontextprotocol.server.name="io.github.muchiny/mcp-legifrance" \
      org.opencontainers.image.title="mcp-legifrance" \
      org.opencontainers.image.description="Unofficial MCP server for French law (Légifrance API via PISTE)" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.source="https://github.com/muchiny/mcp-legifrance" \
      org.opencontainers.image.licenses="Apache-2.0"
COPY --from=bin --chmod=0755 /mcp-legifrance /usr/local/bin/mcp-legifrance
COPY LICENSE NOTICE THIRD_PARTY_LICENSES.md /usr/share/doc/mcp-legifrance/
# Répertoire d'audit inscriptible par nonroot (aucun RUN possible dans distroless). LF_AUDIT_DIR
# est fixé : le défaut `dirs::data_dir()` dépend de HOME, que l'image ne garantit pas [V-167].
COPY --chown=65532:65532 packaging/oci/data/ /data/
ENV LF_AUDIT_DIR=/data/audit
# Utilisateur numérique non root : les contrôles `runAsNonRoot` l'exigent. Pas de HEALTHCHECK
# (transport stdio) ni de VOLUME (un volume anonyme serait recréé à chaque `docker run --rm`).
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/mcp-legifrance"]
CMD ["serve"]
