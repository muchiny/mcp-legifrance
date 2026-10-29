#!/usr/bin/env bash
# Installe `mcp-publisher` (registre MCP) à une version fixe et vérifiée (08 §3.7, T-MD-03, V-206).
#
# Usage : .github/scripts/install-mcp-publisher.sh <version>      (par exemple 1.8.1)
#
# La référence téléchargeait `latest` sans somme (D-44). Ici :
#   1. seule une version dont la somme SHA-256 est épinglée ci-dessous s'installe ; une autre
#      version est refusée, ce qui force à relever la table (et à la relire) à chaque montée ;
#   2. l'archive est téléchargée depuis la release du tag et sa somme doit égaler celle épinglée,
#      puis celle du fichier `registry_<version>_checksums.txt` publié par goreleaser.
# Le fichier de sommes porte aussi une signature cosign (`.sigstore.json`), non vérifiée ici : la
# table épinglée tient lieu d'ancre de confiance.
#
# Le binaire est copié dans $MCP_PUBLISHER_BIN_DIR (défaut /usr/local/bin, avec sudo si besoin) :
# il est donc sur le PATH dans la même étape, ce que $GITHUB_PATH n'offrirait pas.
#
# Pour relever la version (la plus récente a au moins 14 jours, comme les dépendances cargo) :
#   gh release view v<version> -R modelcontextprotocol/registry --json assets \
#     --jq '.assets[] | select(.name | test("^mcp-publisher_(linux|darwin)_(amd64|arm64)[.]tar[.]gz$")) | "\(.digest) \(.name)"'
# puis ajouter les quatre lignes à `pinned_sha256`.
#
# Crochet de test : MCP_PUBLISHER_BASE_URL remplace l'URL des releases (file:// accepté).
set -euo pipefail

usage() {
  echo "usage: $0 <version>   (for example 1.8.1)" >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
version="$1"
# X.Y.Z strict : la version entre dans une URL et dans un nom de fichier.
printf '%s' "$version" | grep -Eqx '[0-9]+[.][0-9]+[.][0-9]+' || usage

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) os="" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=amd64 ;;
  aarch64 | arm64) arch=arm64 ;;
  *) arch="" ;;
esac
if [ -z "$os" ] || [ -z "$arch" ]; then
  echo "install-mcp-publisher: unsupported platform $(uname -s)-$(uname -m)" >&2
  exit 1
fi
platform="${os}_${arch}"

# Sommes SHA-256 des archives `mcp-publisher_<os>_<arch>.tar.gz` (digest des assets de la release).
pinned_sha256() { # version plateforme
  case "$1/$2" in
    1.8.1/darwin_amd64) echo 88126981225e7714fcc6b7a10cdba4a80ae5901e9740a8c06d0d5195c8bc294c ;;
    1.8.1/darwin_arm64) echo e45e520892460732a4bdf37255576415d4a53ec171f8b913faf15bb1aef7cb77 ;;
    1.8.1/linux_amd64) echo a06c9096dcb9727c13555b6be26c7effa707b01f06a4c561ba7a3635443cf2cc ;;
    1.8.1/linux_arm64) echo 8dd75a6cf6845688b5d4e46df58d3ca26d5c8d233bb0626606e1db82c5e883e4 ;;
    *) return 1 ;;
  esac
}

if ! expected="$(pinned_sha256 "$version" "$platform")"; then
  echo "install-mcp-publisher: no pinned checksum for mcp-publisher $version ($platform); add it to this script after checking the release (V-206)" >&2
  exit 1
fi

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

base="${MCP_PUBLISHER_BASE_URL:-https://github.com/modelcontextprotocol/registry/releases/download}"
archive="mcp-publisher_${platform}.tar.gz"
checksums="registry_${version}_checksums.txt"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fetch() { # nom
  case "$base" in
    https://*) curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$work/$1" "$base/v${version}/$1" ;;
    file://*) curl --proto '=file' -fsSL -o "$work/$1" "$base/v${version}/$1" ;;
    *)
      echo "install-mcp-publisher: refusing a non-https base URL" >&2
      exit 1
      ;;
  esac
}
fetch "$archive"
fetch "$checksums"

actual="$(sha256_of "$work/$archive")"
if [ "$actual" != "$expected" ]; then
  echo "install-mcp-publisher: checksum mismatch for $archive: expected $expected, got $actual" >&2
  exit 1
fi
listed="$(awk -v name="$archive" '$2 == name { print $1 }' "$work/$checksums")"
if [ "$listed" != "$expected" ]; then
  echo "install-mcp-publisher: $checksums does not list the pinned checksum for $archive (got '${listed:-nothing}')" >&2
  exit 1
fi

tar -xzf "$work/$archive" -C "$work" mcp-publisher
bin_dir="${MCP_PUBLISHER_BIN_DIR:-/usr/local/bin}"
if [ -d "$bin_dir" ] && [ -w "$bin_dir" ]; then
  install -m 0755 "$work/mcp-publisher" "$bin_dir/mcp-publisher"
elif [ ! -d "$bin_dir" ] && mkdir -p "$bin_dir" 2>/dev/null; then
  install -m 0755 "$work/mcp-publisher" "$bin_dir/mcp-publisher"
else
  sudo -n install -m 0755 "$work/mcp-publisher" "$bin_dir/mcp-publisher"
fi
echo "mcp-publisher $version ($platform) installed in $bin_dir, sha256 $actual"
