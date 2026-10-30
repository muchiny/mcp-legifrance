#!/usr/bin/env bash
# Cohérence de server.json avec la version de la release, le manifeste MCPB et les noms des assets
# (08 §6.2, §7.3 : les identifiants ne sont PAS réécrits par `lock-descriptors --update`, qui ne
# pose que les champs `version`). Un décalage échoue ici, avant le registre : une version publiée
# est immuable (§6.6).
#
# Usage : packaging/check-server-json.sh [--provisional] <server.json> <version> [<manifest.json>]
#   --provisional  accepte le fileSha256 provisoire (64 zéros) du fichier versionné ; sans lui, le
#                  fichier doit porter la somme réelle du MCPB (job build-mcpb, registre).
# Exige jq.
set -euo pipefail

provisional=0
if [ "${1:-}" = "--provisional" ]; then
  provisional=1
  shift
fi
if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
  echo "usage: $0 [--provisional] <server.json> <version> [<manifest.json>]" >&2
  exit 2
fi
file="$1"
version="$2"
manifest="${3:-}"

repo="muchiny/mcp-legifrance"
status=0
bad() {
  echo "check-server-json: $*" >&2
  status=1
}

if ! jq -e type "$file" >/dev/null 2>&1; then
  echo "check-server-json: $file is not valid JSON" >&2
  exit 1
fi

q() { jq -r "$1" "$file"; }

[ "$(q .name)" = "io.github.muchiny/mcp-legifrance" ] || bad ".name is '$(q .name)', expected io.github.muchiny/mcp-legifrance"
[ "$(q .version)" = "$version" ] || bad ".version is '$(q .version)', expected '$version'"
description_length="$(q '.description | length')"
if [ "$description_length" -lt 1 ] || [ "$description_length" -gt 100 ]; then
  bad ".description has $description_length characters (1 to 100 allowed)"
fi
[ "$(q '.remotes // [] | length')" = 0 ] || bad "remotes must be absent: there is no hosted service (08 §6.2)"

# Types de paquets : exactement mcpb et oci, une fois chacun (08 §6.2 ; Q13 : pas de paquet cargo).
types="$(q '[.packages[].registryType] | sort | join(",")')"
[ "$types" = "mcpb,oci" ] || bad "unexpected package types '$types': exactly mcpb and oci are allowed (Q13)"

mcpb='.packages[] | select(.registryType == "mcpb")'
oci='.packages[] | select(.registryType == "oci")'
[ "$(q "$mcpb | .version")" = "$version" ] || bad "mcpb package: version is '$(q "$mcpb | .version")', expected '$version'"
expected_url="https://github.com/$repo/releases/download/v$version/mcp-legifrance-$version.mcpb"
[ "$(q "$mcpb | .identifier")" = "$expected_url" ] || bad "mcpb package: identifier is '$(q "$mcpb | .identifier")', expected '$expected_url'"
expected_image="ghcr.io/$repo:$version"
[ "$(q "$oci | .identifier")" = "$expected_image" ] || bad "oci package: identifier is '$(q "$oci | .identifier")', expected '$expected_image'"
# Le registre rejette un champ `version` sur un paquet OCI (la version est dans l'étiquette).
[ "$(q "$oci | has(\"version\")")" = false ] || bad "oci package must not carry a version field (the tag carries it)"

sha="$(q "$mcpb | .fileSha256")"
zeros="$(printf '0%.0s' $(seq 1 64))"
if ! printf '%s' "$sha" | grep -Eqx '[a-f0-9]{64}'; then
  bad "mcpb package: fileSha256 '$sha' is not 64 lowercase hexadecimal characters"
elif [ "$provisional" = 0 ] && [ "$sha" = "$zeros" ]; then
  bad "mcpb package: fileSha256 is still the provisional value (all zeros)"
fi

# Variables d'environnement : les trois seules déclarées, identifiants secrets ; ni réglages
# avancés ni LF_AUDIT_* (08 §6.2).
for package in "$mcpb" "$oci"; do
  names="$(q "[$package | .environmentVariables[].name] | sort | join(\",\")")"
  if [ "$names" != "LF_PISTE_CLIENT_ID,LF_PISTE_CLIENT_SECRET,LF_PISTE_ENV" ]; then
    bad "environment variables are '$names': only LF_PISTE_CLIENT_ID, LF_PISTE_CLIENT_SECRET and LF_PISTE_ENV are declared (a variable such as LF_PISTE_RPS belongs to the README)"
  fi
  secrets="$(q "[$package | .environmentVariables[] | select(.name != \"LF_PISTE_ENV\") | .isSecret] | all")"
  [ "$secrets" = true ] || bad "the PISTE credentials must be isSecret"
done

if [ -n "$manifest" ]; then
  [ "$(jq -r .version "$manifest")" = "$version" ] || bad "manifest $manifest: version is '$(jq -r .version "$manifest")', expected '$version'"
fi

if [ "$status" -eq 0 ]; then
  echo "check-server-json: ok ($version)"
fi
exit "$status"
