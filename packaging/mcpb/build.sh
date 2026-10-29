#!/usr/bin/env bash
# Construit le bundle MCPB (08 §4.5 job `assemble`, étapes 3 et 4) : arborescence (stage.sh),
# `mcpb validate`, `mcpb pack`, puis contrôle des bits d'exécution dans le ZIP produit. Partagé par
# la release (vrais binaires) et le contrôle de PR (binaires factices, STAGE_SKIP_FORMAT_CHECK=1).
#
# Usage : packaging/mcpb/build.sh <racine des binaires> <répertoire de sortie> <version>
# Prérequis : `npm ci --ignore-scripts --prefix packaging/mcpb/cli` (outil mcpb épinglé par son
# package-lock.json). Sortie : <répertoire de sortie>/mcp-legifrance-<version>.mcpb. Le bundle n'est
# pas signé (Q5, 08 §4.6) : son intégrité repose sur fileSha256 et sur l'attestation.
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <binaries root> <output directory> <version>" >&2
  exit 2
fi
bins="$1"
out="$2"
version="$3"
root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
mcpb="$root/packaging/mcpb/cli/node_modules/.bin/mcpb"

if [ ! -x "$mcpb" ]; then
  echo "build: mcpb is not installed: run 'npm ci --ignore-scripts --prefix packaging/mcpb/cli' first" >&2
  exit 1
fi
manifest_version="$(jq -r .version "$root/packaging/mcpb/manifest.json")"
if [ "$manifest_version" != "$version" ]; then
  echo "build: packaging/mcpb/manifest.json is at version $manifest_version, not $version (cargo xtask lock-descriptors --update)" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
"$root/packaging/mcpb/stage.sh" "$work/bundle" "$bins"
"$mcpb" validate "$work/bundle/manifest.json"

mkdir -p "$out"
file="$out/mcp-legifrance-$version.mcpb"
"$mcpb" pack "$work/bundle" "$file"

# `mcpb pack` ne doit pas avoir perdu les bits d'exécution (08 §4.2) : le lanceur et les binaires
# POSIX doivent rester exécutables dans le ZIP, sinon le client ne peut pas les lancer.
listing="$(unzip -Z "$file")"
for path in server/launcher server/darwin-aarch64/mcp-legifrance server/darwin-x86_64/mcp-legifrance \
  server/linux-aarch64/mcp-legifrance server/linux-x86_64/mcp-legifrance; do
  if ! printf '%s\n' "$listing" | grep -Eq "rwxr-xr-x.* $path\$"; then
    echo "build: $path is not executable in $file" >&2
    exit 1
  fi
done
echo "build: $file ($(wc -c <"$file" | tr -d ' ') bytes)"
