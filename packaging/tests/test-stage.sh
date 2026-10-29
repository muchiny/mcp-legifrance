#!/usr/bin/env bash
# Test de packaging/mcpb/stage.sh (08 §4.2) : disposition du bundle, bits d'exécution, refus d'un
# binaire absent, d'un binaire de mauvais format et d'une destination non vide.
# Binaires factices ; aucun réseau.
#
# Usage : packaging/tests/test-stage.sh   (depuis n'importe quel répertoire)
set -euo pipefail

root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
stage="$root/packaging/mcpb/stage.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
  echo "ECHEC : $*" >&2
  failures=$((failures + 1))
}

bins="$work/bins"
for dir in darwin-aarch64 darwin-x86_64 linux-aarch64 linux-x86_64; do
  mkdir -p "$bins/$dir"
  printf '#!/bin/sh\nexit 0\n' >"$bins/$dir/mcp-legifrance"
  chmod 0644 "$bins/$dir/mcp-legifrance" # le script doit poser +x lui-même
done
mkdir -p "$bins/win32-x86_64"
printf 'MZ' >"$bins/win32-x86_64/mcp-legifrance.exe"

# Cas nominal (format non contrôlé : les binaires sont factices).
STAGE_SKIP_FORMAT_CHECK=1 bash "$stage" "$work/out" "$bins" >"$work/log" 2>&1 || fail "cas nominal : $(cat "$work/log")"
for f in manifest.json icon.png LICENSE NOTICE README.md server/launcher \
  server/darwin-aarch64/mcp-legifrance server/darwin-x86_64/mcp-legifrance \
  server/linux-aarch64/mcp-legifrance server/linux-x86_64/mcp-legifrance \
  server/win32-x86_64/mcp-legifrance.exe; do
  [ -f "$work/out/$f" ] || fail "fichier absent du bundle : $f"
done
for f in server/launcher server/darwin-aarch64/mcp-legifrance server/linux-x86_64/mcp-legifrance \
  server/win32-x86_64/mcp-legifrance.exe; do
  [ -x "$work/out/$f" ] || fail "bit d'exécution manquant : $f"
done
cmp -s "$root/packaging/mcpb/launcher" "$work/out/server/launcher" || fail "le lanceur copié diffère de la source"

# Format contrôlé : un binaire factice est refusé avec un message qui nomme le fichier.
rc=0
bash "$stage" "$work/out-format" "$bins" >"$work/log" 2>&1 || rc=$?
[ "$rc" = 1 ] || fail "format : code attendu 1, obtenu $rc"
grep -q "darwin-aarch64/mcp-legifrance has an unexpected format" "$work/log" || fail "format : message inattendu : $(cat "$work/log")"

# Binaire absent.
rm "$bins/linux-aarch64/mcp-legifrance"
rc=0
STAGE_SKIP_FORMAT_CHECK=1 bash "$stage" "$work/out-missing" "$bins" >"$work/log" 2>&1 || rc=$?
[ "$rc" = 1 ] || fail "binaire absent : code attendu 1, obtenu $rc"
grep -q "missing binary" "$work/log" || fail "binaire absent : message inattendu : $(cat "$work/log")"

# Destination non vide : refus, rien n'est écrasé.
rc=0
STAGE_SKIP_FORMAT_CHECK=1 bash "$stage" "$work/out" "$bins" >"$work/log" 2>&1 || rc=$?
[ "$rc" = 1 ] || fail "destination non vide : code attendu 1, obtenu $rc"

# Usage.
rc=0
bash "$stage" >"$work/log" 2>&1 || rc=$?
[ "$rc" = 2 ] || fail "usage : code attendu 2, obtenu $rc"

if [ "$failures" -ne 0 ]; then
  echo "$failures échec(s)" >&2
  exit 1
fi
echo "test-stage : ok"
