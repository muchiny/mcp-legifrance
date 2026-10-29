#!/usr/bin/env bash
# Contrôle d'une image OCI publiée, par lecture ANONYME du registre (08 §5.3 étape 4, §6.5 étape 3,
# V-203, V-205) : c'est ce que fait le registre MCP à la publication (`oci.go:96`, `:128-140`).
#
# Usage : packaging/oci/verify-image.sh <image>      (par exemple ghcr.io/muchiny/mcp-legifrance:0.1.0)
#
# Pour chaque plateforme attendue, le manifeste est lu sans identifiant, puis la configuration de
# l'image : label du registre, utilisateur numérique non root. Un paquet GHCR privé échoue ici
# (jeton anonyme refusé), exactement comme il échouerait au registre MCP.
#
# Réglages par variables d'environnement (défauts : l'image de ce dépôt) :
#   EXPECT_LABEL_NAME   label à trouver, vide pour ne pas le contrôler
#                       (io.modelcontextprotocol.server.name)
#   EXPECT_LABEL_VALUE  sa valeur (io.github.muchiny/mcp-legifrance)
#   EXPECT_USER         utilisateur de l'image (65532:65532)
#   EXPECT_PLATFORMS    plateformes attendues, séparées par des espaces (linux/amd64 linux/arm64)
#
# Exige curl et jq.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <image>   (for example ghcr.io/muchiny/mcp-legifrance:0.1.0)" >&2
  exit 2
fi
ref="$1"
label_name="${EXPECT_LABEL_NAME-io.modelcontextprotocol.server.name}"
label_value="${EXPECT_LABEL_VALUE-io.github.muchiny/mcp-legifrance}"
want_user="${EXPECT_USER-65532:65532}"
want_platforms="${EXPECT_PLATFORMS-linux/amd64 linux/arm64}"

# <registre>/<dépôt>:<étiquette> ; l'étiquette est obligatoire (pas de digest ni de latest implicite).
registry="${ref%%/*}"
rest="${ref#*/}"
case "$rest" in
  *:*) repo="${rest%:*}"; tag="${rest##*:}" ;;
  *)
    echo "verify-image: '$ref' has no tag" >&2
    exit 2
    ;;
esac
case "$registry" in
  *[!a-zA-Z0-9.:-]* | "") echo "verify-image: invalid registry in '$ref'" >&2; exit 2 ;;
esac
case "$repo" in
  *[!a-z0-9._/-]* | "") echo "verify-image: invalid repository in '$ref'" >&2; exit 2 ;;
esac
case "$tag" in
  *[!a-zA-Z0-9._-]* | "") echo "verify-image: invalid tag in '$ref'" >&2; exit 2 ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

api="https://$registry/v2"
curl_api() { curl --proto '=https' --tlsv1.2 -fsSL --retry 3 "$@"; }

# Jeton anonyme : le registre répond 401 avec le défi `WWW-Authenticate: Bearer realm=...,service=...`.
token=""
challenge="$(curl --proto '=https' --tlsv1.2 -sS -D - -o /dev/null "$api/" | tr -d '\r' | grep -i '^www-authenticate:' || true)"
if [ -n "$challenge" ]; then
  realm="$(printf '%s' "$challenge" | sed -n 's/.*realm="\([^"]*\)".*/\1/p')"
  service="$(printf '%s' "$challenge" | sed -n 's/.*service="\([^"]*\)".*/\1/p')"
  case "$realm" in
    https://*) ;;
    *) echo "verify-image: unexpected authentication challenge from $registry" >&2; exit 1 ;;
  esac
  if ! curl_api -G --data-urlencode "service=$service" --data-urlencode "scope=repository:$repo:pull" \
    "$realm" -o "$work/token.json" 2>"$work/token.err"; then
    echo "verify-image: $ref is not publicly readable (anonymous token refused): make the package public (V-205)" >&2
    cat "$work/token.err" >&2
    exit 1
  fi
  token="$(jq -r '.token // .access_token // empty' "$work/token.json")"
  if [ -z "$token" ]; then
    echo "verify-image: no anonymous token for $ref" >&2
    exit 1
  fi
fi

get() { # url sortie [Accept]
  local accept="${3:-application/json}"
  if [ -n "$token" ]; then
    curl_api -H "Authorization: Bearer $token" -H "Accept: $accept" "$1" -o "$2"
  else
    curl_api -H "Accept: $accept" "$1" -o "$2"
  fi
}

manifest_types='application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json'
if ! get "$api/$repo/manifests/$tag" "$work/index.json" "$manifest_types" 2>"$work/index.err"; then
  echo "verify-image: cannot read the manifest of $ref anonymously" >&2
  cat "$work/index.err" >&2
  exit 1
fi
if ! jq -e '.manifests | type == "array"' "$work/index.json" >/dev/null 2>&1; then
  echo "verify-image: $ref is not a multi-platform index" >&2
  exit 1
fi

status=0
for platform in $want_platforms; do
  os="${platform%%/*}"
  arch="${platform#*/}"
  digest="$(jq -r --arg os "$os" --arg arch "$arch" \
    '[.manifests[] | select(.platform.os == $os and .platform.architecture == $arch)] | .[0].digest // empty' \
    "$work/index.json")"
  if [ -z "$digest" ]; then
    echo "verify-image: $ref has no $platform manifest" >&2
    status=1
    continue
  fi
  get "$api/$repo/manifests/$digest" "$work/manifest.json" "$manifest_types"
  config_digest="$(jq -r '.config.digest' "$work/manifest.json")"
  get "$api/$repo/blobs/$config_digest" "$work/config.json"
  size="$(jq '[.layers[].size] | add' "$work/manifest.json")"
  got_user="$(jq -r '.config.User // ""' "$work/config.json")"
  got_arch="$(jq -r '.architecture' "$work/config.json")"
  got_os="$(jq -r '.os' "$work/config.json")"
  echo "$platform: $digest, user '$got_user', layers $size bytes"
  if [ "$got_os/$got_arch" != "$platform" ]; then
    echo "verify-image: $platform manifest carries a $got_os/$got_arch image" >&2
    status=1
  fi
  if [ -n "$want_user" ] && [ "$got_user" != "$want_user" ]; then
    echo "verify-image: $platform runs as '$got_user' instead of '$want_user'" >&2
    status=1
  fi
  if [ -n "$label_name" ]; then
    got_label="$(jq -r --arg name "$label_name" '.config.Labels[$name] // ""' "$work/config.json")"
    if [ "$got_label" != "$label_value" ]; then
      echo "verify-image: $platform: label $label_name is '$got_label' instead of '$label_value'" >&2
      status=1
    fi
  fi
done
exit "$status"
