#!/usr/bin/env bash
# Installe un outil de la chaîne de release à une version fixe et vérifiée (F4, SUP-02).
#
# Usage : .github/scripts/install-release-tool.sh <outil> <version>
#         outil : dist | cargo-auditable | cargo-cyclonedx
#
# Le gabarit de dist installait ces outils par `curl ... | sh` (dist, cargo-cyclonedx) ou par une
# commande du plan de dist qui télécharge `latest` (cargo-auditable), sans aucune somme. Le script
# d'installation d'une release est un asset modifiable : s'il était remplacé, il s'exécutait dans
# des jobs qui ont le jeton du dépôt, puis son `dist` piégé était réutilisé par `host`, qui atteste
# (`id-token: write`) : les binaires altérés auraient porté une attestation valide. Ici :
#   1. seule une version dont la somme SHA-256 est épinglée ci-dessous s'installe, pour la
#      plateforme de l'exécuteur ; toute autre combinaison est refusée, ce qui force à relever la
#      table (et à la relire) à chaque montée ;
#   2. on télécharge l'ARCHIVE de la release (pas l'installeur) et sa somme doit égaler celle
#      épinglée avant toute extraction.
# Les archives Linux sont les `musl` (statiques) : aucune dépendance à la glibc de l'exécuteur.
#
# Le binaire est copié dans $RELEASE_TOOL_BIN_DIR (défaut ${CARGO_HOME:-$HOME/.cargo}/bin) et ce
# dossier est ajouté à $GITHUB_PATH quand il existe.
#
# Pour relever une version (la plus récente a au moins 14 jours, comme les dépendances cargo) :
#   gh release view <tag> -R <dépôt> --json assets --jq '.assets[] | "\(.digest) \(.name)"'
# (dist : axodotdev/cargo-dist, tag v<version> ; cargo-auditable : rust-secure-code/cargo-auditable,
# tag v<version> ; cargo-cyclonedx : CycloneDX/cyclonedx-rust-cargo, tag cargo-cyclonedx-<version>,
# dont les assets n'ont pas de digest : télécharger l'archive, calculer la somme et la comparer au
# fichier `.sha256` publié à côté). Puis ajouter les lignes à `pinned_sha256`.
#
# Crochets de test : RELEASE_TOOL_BASE_URL remplace le dossier des releases (https:// ou file://),
# RELEASE_TOOL_TRIPLE force le triplet de la plateforme.
set -euo pipefail

usage() {
  echo "usage: $0 <dist|cargo-auditable|cargo-cyclonedx> <version>   (for example dist 0.33.0)" >&2
  exit 2
}

[[ "$#" -eq 2 ]] || usage
tool="$1"
version="$2"
# X.Y.Z strict : la version entre dans une URL et dans un nom de fichier.
printf '%s' "$version" | grep -Eqx '[0-9]+[.][0-9]+[.][0-9]+' || usage

case "$tool" in
  dist)
    stem="cargo-dist"
    bin="dist"
    release_dir="https://github.com/axodotdev/cargo-dist/releases/download/v${version}"
    ;;
  cargo-auditable)
    stem="cargo-auditable"
    bin="cargo-auditable"
    release_dir="https://github.com/rust-secure-code/cargo-auditable/releases/download/v${version}"
    ;;
  cargo-cyclonedx)
    stem="cargo-cyclonedx"
    bin="cargo-cyclonedx"
    release_dir="https://github.com/CycloneDX/cyclonedx-rust-cargo/releases/download/cargo-cyclonedx-${version}"
    ;;
  *) usage ;;
esac

if [[ -n "${RELEASE_TOOL_TRIPLE:-}" ]]; then
  triple="$RELEASE_TOOL_TRIPLE"
else
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64 | Linux/amd64) triple=x86_64-unknown-linux-musl ;;
    Linux/aarch64 | Linux/arm64) triple=aarch64-unknown-linux-musl ;;
    Darwin/x86_64) triple=x86_64-apple-darwin ;;
    Darwin/arm64 | Darwin/aarch64) triple=aarch64-apple-darwin ;;
    MINGW* | MSYS* | CYGWIN*) triple=x86_64-pc-windows-msvc ;;
    *) triple="" ;;
  esac
fi
if [[ -z "$triple" ]]; then
  echo "install-release-tool: unsupported platform $(uname -s)-$(uname -m)" >&2
  exit 1
fi
case "$triple" in
  *-windows-msvc) ext=zip ;;
  *) ext=tar.xz ;;
esac

# Sommes SHA-256 des archives `<stem>-<triplet>.<ext>`.
pinned_sha256() { # outil version triplet
  case "$1/$2/$3" in
    dist/0.33.0/aarch64-apple-darwin) echo 7b3cbe25511de01d74c0f5fcb7909edabd379bea9cfa284d93af5a3cdfa3247c ;;
    dist/0.33.0/x86_64-apple-darwin) echo 6a49bfb61bd86770d79c27f3d2b40c6b2e71cde940d3d31a6ccaaffc124d7a29 ;;
    dist/0.33.0/aarch64-unknown-linux-musl) echo 4761cff5fc547ad66d1449abbf321380b0e6bd8093b1fe6593852a3314fd0c19 ;;
    dist/0.33.0/x86_64-unknown-linux-musl) echo b8e95bc76c63375958173ef5ae2dbd8e9211cc1ed03cdee0899702766b4c2a2e ;;
    dist/0.33.0/x86_64-pc-windows-msvc) echo 9a36d70795e14326a5ec4bf17aee085df00ab85a322739291ea5d4b1b5f693cd ;;
    cargo-auditable/0.7.7/aarch64-apple-darwin) echo a66ad6392861ca1abfe0b02b522622d91edf63e5d7b60f4d3088b5c23d240c72 ;;
    cargo-auditable/0.7.7/x86_64-apple-darwin) echo 4b87f06ea13d32466bbc68b1f5715ac11753a052d9935245380bddce8a3f0e50 ;;
    cargo-auditable/0.7.7/aarch64-unknown-linux-musl) echo c324af56990601354bbf40080f582ba46f8ec8b807a1388e25bb7e429d285c18 ;;
    cargo-auditable/0.7.7/x86_64-unknown-linux-musl) echo 7676443367c5e33d9fb22acef63cccb5e9202de12dae1b0579995eb3e2bc7a18 ;;
    cargo-auditable/0.7.7/x86_64-pc-windows-msvc) echo 717ba2d3ed7997c501e9987ee7c94bebf9d7219b18b185a03009420ec7029c7d ;;
    cargo-cyclonedx/0.5.5/aarch64-apple-darwin) echo c296b0821c02ad13cbea6a7173ba6c37fa777acc7d066c5ff8570a600aeb62a7 ;;
    cargo-cyclonedx/0.5.5/x86_64-apple-darwin) echo 98beb669323396428b5c6c7003adc4235207442c7b025743366ee98759b2c3a6 ;;
    cargo-cyclonedx/0.5.5/x86_64-unknown-linux-musl) echo e0a872a5a8e32b9621334636e9da444d70527fb138aa75380e0dc20db8b0714b ;;
    cargo-cyclonedx/0.5.5/x86_64-pc-windows-msvc) echo 7b4507cb8fb260df35bed0ad59f675d569f9bde3423228e8b44c7c378876a410 ;;
    *) return 1 ;;
  esac
}

if ! expected="$(pinned_sha256 "$tool" "$version" "$triple")"; then
  echo "install-release-tool: no pinned checksum for $tool $version ($triple); add it to this script after checking the release (SUP-02)" >&2
  exit 1
fi

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

base="${RELEASE_TOOL_BASE_URL:-$release_dir}"
archive="${stem}-${triple}.${ext}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

case "$base" in
  https://*) curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$work/$archive" "$base/$archive" ;;
  file://*) curl --proto '=file' -fsSL -o "$work/$archive" "$base/$archive" ;;
  *)
    echo "install-release-tool: refusing a non-https base URL" >&2
    exit 1
    ;;
esac

actual="$(sha256_of "$work/$archive")"
if [[ "$actual" != "$expected" ]]; then
  echo "install-release-tool: checksum mismatch for $archive: expected $expected, got $actual" >&2
  exit 1
fi

mkdir "$work/x"
case "$ext" in
  tar.xz) tar -xJf "$work/$archive" -C "$work/x" ;;
  zip)
    if command -v unzip >/dev/null 2>&1; then
      unzip -q "$work/$archive" -d "$work/x"
    elif command -v 7z >/dev/null 2>&1; then
      7z x -y "-o$work/x" "$work/$archive" >/dev/null
    else
      powershell.exe -NoProfile -Command "Expand-Archive -LiteralPath '$(cygpath -w "$work/$archive")' -DestinationPath '$(cygpath -w "$work/x")'"
    fi
    ;;
esac

found="$(find "$work/x" -type f \( -name "$bin" -o -name "$bin.exe" \) | head -n 1)"
if [[ -z "$found" ]]; then
  echo "install-release-tool: $archive does not contain $bin" >&2
  exit 1
fi

bin_dir="${RELEASE_TOOL_BIN_DIR:-${CARGO_HOME:-$HOME/.cargo}/bin}"
mkdir -p "$bin_dir"
install -m 0755 "$found" "$bin_dir/$(basename "$found")"
if [[ -n "${GITHUB_PATH:-}" ]]; then
  printf '%s\n' "$bin_dir" >>"$GITHUB_PATH"
fi
echo "$tool $version ($triple) installed in $bin_dir, sha256 $actual"
