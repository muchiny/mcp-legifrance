#!/usr/bin/env bash
# Tests de packaging/resolve-release.sh et de packaging/check-server-json.sh (08 §6.2, §6.5, §7.3) :
# étiquette et version tirées du plan de dist, et cohérence de server.json avec la version, avec
# le manifeste MCPB et avec les noms de la release. Aucun réseau.
#
# Usage : packaging/tests/test-release-scripts.sh   (depuis n'importe quel répertoire ; exige jq)
set -euo pipefail

root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
resolve="$root/packaging/resolve-release.sh"
check="$root/packaging/check-server-json.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
  echo "ECHEC : $*" >&2
  failures=$((failures + 1))
}

# ok / ko <code> <extrait> -- <commande...> : lance la commande, tout dans $out.
run() {
  local rc=0
  "$@" >"$work/out" 2>&1 || rc=$?
  out="$(cat "$work/out")"
  code=$rc
}
expect() { # code attendu, extrait, commande...
  local want="$1" needle="$2"
  shift 2
  run "$@"
  if [ "$code" != "$want" ]; then fail "$*: code attendu $want, obtenu $code ($out)"; fi
  case "$out" in
    *"$needle"*) ;;
    *) fail "$*: extrait attendu '$needle', obtenu '$out'" ;;
  esac
}

# --- resolve-release.sh ---------------------------------------------------------------------
plan() { # étiquette préversion version-du-paquet
  printf '{"announcement_tag":"%s","announcement_is_prerelease":%s,"releases":[{"app_name":"mcp-legifrance","app_version":"%s"}]}' "$1" "$2" "$3"
}

PLAN="$(plan v0.1.0 false 0.1.0)" expect 0 "tag=v0.1.0" bash "$resolve"
PLAN="$(plan v0.1.0 false 0.1.0)" expect 0 "version=0.1.0" bash "$resolve"
PLAN="$(plan v0.1.0 false 0.1.0)" expect 0 "prerelease=false" bash "$resolve"
PLAN="$(plan v0.2.0-rc.1 true 0.2.0-rc.1)" expect 0 "prerelease=true" bash "$resolve"
PLAN="$(plan v0.2.0-rc.1 true 0.2.0-rc.1)" expect 0 "version=0.2.0-rc.1" bash "$resolve"
# Étiquette seule (workflow_dispatch) : la préversion se lit dans le suffixe.
TAG=v1.2.3 expect 0 "prerelease=false" bash "$resolve"
TAG=v1.2.3-rc.2 expect 0 "prerelease=true" bash "$resolve"
# Refus : étiquette mal formée, injection, plan incohérent, ni plan ni étiquette.
TAG=0.1.0 expect 1 "tag" bash "$resolve"
TAG='v0.1.0
evil=1' expect 1 "tag" bash "$resolve"
TAG='v0.1.0;id' expect 1 "tag" bash "$resolve"
PLAN="$(plan v0.1.0 false 0.2.0)" expect 1 "does not match" bash "$resolve"
PLAN="$(plan 'mcp-legifrance-v0.1.0' false 0.1.0)" expect 1 "tag" bash "$resolve"
PLAN='{"announcement_is_prerelease":false}' expect 1 "announcement_tag" bash "$resolve"
PLAN='pas du json' expect 1 "announcement_tag" bash "$resolve"
expect 1 "PLAN or TAG" bash "$resolve"
# Un plan réel de dist (forme relevée avec `dist plan`), s'il est fourni.
if [ -n "${DIST_PLAN_FILE:-}" ]; then
  PLAN="$(cat "$DIST_PLAN_FILE")" TAG="" expect 0 "tag=v" bash "$resolve"
fi

# --- check-server-json.sh -------------------------------------------------------------------
good_sha=$(printf 'a%.0s' $(seq 1 64))
base="$work/server.json"
manifest="$work/manifest.json"
jq --arg sha "$good_sha" '(.packages[] | select(.registryType == "mcpb") | .fileSha256) = $sha' "$root/server.json" >"$base"
cp "$root/packaging/mcpb/manifest.json" "$manifest"
version="$(jq -r .version "$base")"

expect 0 "ok" bash "$check" "$base" "$version" "$manifest"
expect 0 "ok" bash "$check" "$base" "$version"
# Le fichier du dépôt porte le fileSha256 provisoire (64 zéros) : accepté seulement avec --provisional.
expect 1 "fileSha256" bash "$check" "$root/server.json" "$version"
expect 0 "ok" bash "$check" --provisional "$root/server.json" "$version" "$manifest"

mutate() { # nom filtre-jq : écrit $work/<nom>.json
  jq "$2" "$base" >"$work/$1.json"
}
mutate version '.version = "9.9.9"'
expect 1 ".version" bash "$check" "$work/version.json" "$version"
mutate pkgversion '(.packages[] | select(.registryType == "mcpb") | .version) = "9.9.9"'
expect 1 "version" bash "$check" "$work/pkgversion.json" "$version"
mutate mcpburl '(.packages[] | select(.registryType == "mcpb") | .identifier) = "https://github.com/muchiny/mcp-legifrance/releases/download/v9.9.9/mcp-legifrance-9.9.9.mcpb"'
expect 1 "identifier" bash "$check" "$work/mcpburl.json" "$version"
mutate ocitag '(.packages[] | select(.registryType == "oci") | .identifier) = "ghcr.io/muchiny/mcp-legifrance:9.9.9"'
expect 1 "identifier" bash "$check" "$work/ocitag.json" "$version"
mutate ocidigest '(.packages[] | select(.registryType == "oci") | .version) = "0.1.0"'
expect 1 "oci" bash "$check" "$work/ocidigest.json" "$version"
mutate cargo '.packages += [{"registryType":"cargo","identifier":"mcp-legifrance","version":"0.1.0","transport":{"type":"stdio"}}]'
expect 1 "package types" bash "$check" "$work/cargo.json" "$version"
mutate name '.name = "io.github.other/mcp-legifrance"'
expect 1 ".name" bash "$check" "$work/name.json" "$version"
mutate badsha '(.packages[] | select(.registryType == "mcpb") | .fileSha256) = "xyz"'
expect 1 "fileSha256" bash "$check" "$work/badsha.json" "$version"
mutate longdesc '.description = ("x" * 101)'
expect 1 "description" bash "$check" "$work/longdesc.json" "$version"
mutate remotes '.remotes = [{"type":"streamable-http","url":"https://example.org/mcp"}]'
expect 1 "remotes" bash "$check" "$work/remotes.json" "$version"
mutate advanced '(.packages[0].environmentVariables) += [{"name":"LF_PISTE_RPS","description":"x"}]'
expect 1 "LF_PISTE_RPS" bash "$check" "$work/advanced.json" "$version"
# Le manifeste MCPB doit porter la même version.
jq '.version = "9.9.9"' "$manifest" >"$work/manifest-bad.json"
expect 1 "manifest" bash "$check" "$base" "$version" "$work/manifest-bad.json"
expect 2 "usage" bash "$check"

if [ "$failures" -ne 0 ]; then
  echo "$failures échec(s)" >&2
  exit 1
fi
echo "test-release-scripts : ok"
