#!/usr/bin/env bash
# Auto-test de check-release-workflow.sh et de install-release-tool.sh (`make release-selftest`,
# dans `make ci`), hors réseau : workflows factices, archives servies en file://.
set -euo pipefail

scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo="$(cd "$scripts/../.." && pwd)"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cases=0
failures=0

# expect <code attendu> <motif présent dans la sortie, ou vide> <commande…>
expect() {
  local want=$1 pattern=$2 out code=0
  shift 2
  cases=$((cases + 1))
  out=$("$@" 2>&1) || code=$?
  if [ "$code" != "$want" ] || { [ -n "$pattern" ] && ! grep -Eq -- "$pattern" <<<"$out"; }; then
    failures=$((failures + 1))
    printf 'FAIL case %d: want exit %s with /%s/, got exit %s:\n%s\n' "$cases" "$want" "$pattern" "$code" "$out"
  fi
}

# --- check-release-workflow.sh ---------------------------------------------------------------
good() { # fichier
  cat >"$1" <<'YAML'
name: Release
permissions: {}
on:
  push:
jobs:
  plan:
    runs-on: ubuntu-22.04
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    permissions:
      "contents": "read"
    steps:
      - run: .github/scripts/install-release-tool.sh dist 0.33.0
  build-local-artifacts:
    runs-on: ubuntu-22.04
    permissions:
      "contents": "read"
    steps:
      - run: dist build
  host:
    runs-on: ubuntu-22.04
    env:
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    permissions:
      "attestations": "write"
      "contents": "write"
      "id-token": "write"
    steps:
      - run: gh release create
YAML
}
check="$scripts/check-release-workflow.sh"

good "$work/good.yml"
expect 0 '' "$check" "$work/good.yml"
expect 0 '' "$check" "$repo/.github/workflows/release.yml"
expect 2 'not found' "$check" "$work/absent.yml"

good "$work/a.yml"
sed -i 's/^permissions: {}$/permissions:\n  "contents": "write"/' "$work/a.yml"
expect 1 'workflow-level permissions must be' "$check" "$work/a.yml"

good "$work/b.yml"
sed -i '/^permissions: {}$/d' "$work/b.yml"
expect 1 'missing workflow-level permissions' "$check" "$work/b.yml"

good "$work/c.yml"
sed -i '/^  build-local-artifacts:/,/^  host:/{/permissions:/d;/"contents": "read"/d}' "$work/c.yml"
expect 1 'job build-local-artifacts has no permissions block' "$check" "$work/c.yml"

good "$work/d.yml"
sed -i '/^  build-local-artifacts:/,/^  host:/s/"contents": "read"/"contents": "write"/' "$work/d.yml"
expect 1 'job build-local-artifacts has contents: write' "$check" "$work/d.yml"

good "$work/e.yml"
sed -i 's/^    runs-on: ubuntu-22.04$/&\n    env:\n      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}/;' "$work/e.yml"
expect 1 'GH_TOKEN in the environment of job' "$check" "$work/e.yml"

good "$work/f.yml"
sed -i 's|dist build|curl -LsSf https://example.invalid/installer.sh \| sh|' "$work/f.yml"
expect 1 'pipe into a shell' "$check" "$work/f.yml"

good "$work/g.yml"
sed -i 's|dist build|${{ matrix.install_dist.run }}|' "$work/g.yml"
expect 1 'install command taken from the dist plan' "$check" "$work/g.yml"

# --- install-release-tool.sh -------------------------------------------------------------------
install="$scripts/install-release-tool.sh"
mkdir -p "$work/rel" "$work/bin"
printf 'not the real archive' >"$work/rel/cargo-dist-x86_64-unknown-linux-musl.tar.xz"
run_install() { # code motif outil version [triplet]
  local want=$1 pattern=$2 tool=$3 version=$4 triple=${5:-x86_64-unknown-linux-musl}
  expect "$want" "$pattern" env RELEASE_TOOL_BASE_URL="file://$work/rel" RELEASE_TOOL_BIN_DIR="$work/bin" \
    RELEASE_TOOL_TRIPLE="$triple" "$install" "$tool" "$version"
}
run_install 1 'checksum mismatch for cargo-dist-x86_64-unknown-linux-musl.tar.xz' dist 0.33.0
run_install 1 'no pinned checksum for dist 9.9.9' dist 9.9.9
run_install 1 'no pinned checksum for dist 0.33.0 \(s390x-unknown-linux-gnu\)' dist 0.33.0 s390x-unknown-linux-gnu
run_install 1 'no pinned checksum for cargo-cyclonedx 0.5.5 \(aarch64-unknown-linux-musl\)' cargo-cyclonedx 0.5.5 aarch64-unknown-linux-musl
run_install 2 'usage' unknown-tool 1.2.3
run_install 2 'usage' dist '1.2.3; rm -rf /'
expect 2 'usage' "$install" dist
expect 1 'refusing a non-https base URL' env RELEASE_TOOL_BASE_URL=http://example.invalid \
  RELEASE_TOOL_TRIPLE=x86_64-unknown-linux-musl "$install" dist 0.33.0
# Rien n'est installé quand la somme diffère.
cases=$((cases + 1))
if [ -n "$(ls -A "$work/bin")" ]; then
  failures=$((failures + 1))
  echo "FAIL: a binary was installed despite a checksum mismatch"
fi

printf '%d cases, %d failures\n' "$cases" "$failures"
[ "$failures" -eq 0 ]
