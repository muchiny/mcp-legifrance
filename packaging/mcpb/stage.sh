#!/usr/bin/env bash
# Assemble l'arborescence du bundle MCPB (08 §4.2) dans un répertoire, prête pour `mcpb validate`
# et `mcpb pack`. Partagé par le job de release (build-mcpb.yml, vrais binaires) et par le contrôle
# de PR (binaires factices), pour que la PR éprouve la même disposition que la release.
#
# Usage : packaging/mcpb/stage.sh <répertoire destination> <racine des binaires>
#
# La racine des binaires porte déjà la disposition finale :
#   darwin-aarch64/mcp-legifrance   darwin-x86_64/mcp-legifrance
#   linux-aarch64/mcp-legifrance    linux-x86_64/mcp-legifrance
#   win32-x86_64/mcp-legifrance.exe
# Les bits d'exécution sont posés ici, AVANT l'empaquetage : `mcpb pack` ne conserve les
# permissions Unix que sous Unix et ne force `+x` que sur `entry_point` (08 §4.2).
#
# Contrôle de format (commande `file`) : chaque binaire a la bonne architecture, et les binaires
# Linux sont statiques (musl, aucune dépendance à la glibc de l'hôte). STAGE_SKIP_FORMAT_CHECK=1
# le désactive, pour les binaires factices du contrôle de PR.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <destination directory> <binaries root>" >&2
  exit 2
fi
dest="$1"
bins="$2"
root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"

if [ -e "$dest" ] && [ -n "$(ls -A "$dest" 2>/dev/null)" ]; then
  echo "stage: destination $dest is not empty" >&2
  exit 1
fi

# répertoire:binaire:motif de `file` (expression régulière étendue)
layout=(
  "darwin-aarch64:mcp-legifrance:Mach-O 64-bit.*arm64"
  "darwin-x86_64:mcp-legifrance:Mach-O 64-bit.*x86_64"
  "linux-aarch64:mcp-legifrance:ELF 64-bit.*ARM aarch64.*(statically linked|static-pie linked)"
  "linux-x86_64:mcp-legifrance:ELF 64-bit.*x86-64.*(statically linked|static-pie linked)"
  "win32-x86_64:mcp-legifrance.exe:PE32\\+ executable.*x86-64"
)

for entry in "${layout[@]}"; do
  dir="${entry%%:*}"
  rest="${entry#*:}"
  name="${rest%%:*}"
  pattern="${rest#*:}"
  src="$bins/$dir/$name"
  if [ ! -f "$src" ]; then
    echo "stage: missing binary $src" >&2
    exit 1
  fi
  if [ "${STAGE_SKIP_FORMAT_CHECK:-}" != 1 ]; then
    described="$(file -b "$src")"
    if ! printf '%s' "$described" | grep -Eq "$pattern"; then
      echo "stage: $dir/$name has an unexpected format: $described" >&2
      exit 1
    fi
  fi
  mkdir -p "$dest/server/$dir"
  cp "$src" "$dest/server/$dir/$name"
  chmod 0755 "$dest/server/$dir/$name"
done

cp "$root/packaging/mcpb/manifest.json" "$root/packaging/mcpb/icon.png" "$dest/"
cp "$root/packaging/mcpb/launcher" "$dest/server/launcher"
chmod 0755 "$dest/server/launcher"
cp "$root/LICENSE" "$root/NOTICE" "$root/README.md" "$dest/"
if [ -f "$root/THIRD_PARTY_LICENSES.md" ]; then
  cp "$root/THIRD_PARTY_LICENSES.md" "$dest/"
else
  # release-gates.yml exige ce fichier à jour avant tout build : ici, un simple avertissement.
  echo "::warning::THIRD_PARTY_LICENSES.md is absent: the bundle will not carry it"
fi
echo "stage: bundle tree ready in $dest"
