#!/usr/bin/env bash
# Test du lanceur MCPB (packaging/mcpb/launcher, 08 §4.3) : table de dispatch, arguments,
# refus des plateformes inconnues, aucune écriture sur stdout hors du binaire lancé.
#
# `uname` est remplacé par un faux placé en tête du PATH ; chaque « binaire » est un script qui
# affiche son répertoire d'origine et ses arguments. Aucun réseau, aucun binaire réel.
#
# Usage : packaging/tests/test-launcher.sh   (depuis n'importe quel répertoire)
set -euo pipefail

root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
launcher_src="$root/packaging/mcpb/launcher"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
  echo "ECHEC : $*" >&2
  failures=$((failures + 1))
}

# Arborescence du bundle (08 §4.2), dans un chemin avec une espace : le lanceur doit la tolérer.
bundle="$work/bundle dir"
mkdir -p "$bundle/server" "$work/stubs"
cp "$launcher_src" "$bundle/server/launcher"
chmod +x "$bundle/server/launcher"
for dir in darwin-aarch64 darwin-x86_64 linux-aarch64 linux-x86_64; do
  mkdir -p "$bundle/server/$dir"
  printf '#!/bin/sh\necho "ran %s $*"\n' "$dir" >"$bundle/server/$dir/mcp-legifrance"
  chmod +x "$bundle/server/$dir/mcp-legifrance"
done

cat >"$work/stubs/uname" <<'STUB'
#!/bin/sh
case "$1" in
  -s) printf '%s\n' "$FAKE_UNAME_S" ;;
  -m) printf '%s\n' "$FAKE_UNAME_M" ;;
  *) exit 2 ;;
esac
STUB
chmod +x "$work/stubs/uname"

# run <système> <machine> [arguments du lanceur] : remplit $out, $err et $code.
run() {
  local s="$1" m="$2"
  shift 2
  local rc=0
  FAKE_UNAME_S="$s" FAKE_UNAME_M="$m" PATH="$work/stubs:$PATH" \
    "$bundle/server/launcher" "$@" >"$work/out" 2>"$work/err" || rc=$?
  out="$(cat "$work/out")"
  err="$(cat "$work/err")"
  code=$rc
}

expect_dispatch() { # système machine répertoire attendu
  run "$1" "$2" serve
  if [ "$code" != 0 ] || [ "$out" != "ran $3 serve" ]; then
    fail "$1-$2 : attendu 'ran $3 serve' (code 0), obtenu '$out' (code $code) ; stderr='$err'"
  fi
  if [ -n "$err" ]; then
    fail "$1-$2 : le lanceur n'écrit rien sur stderr en cas de succès, obtenu '$err'"
  fi
}

expect_dispatch Darwin arm64 darwin-aarch64
expect_dispatch Darwin x86_64 darwin-x86_64
expect_dispatch Linux aarch64 linux-aarch64
expect_dispatch Linux arm64 linux-aarch64
expect_dispatch Linux x86_64 linux-x86_64

# Les arguments sont transmis tels quels, y compris ceux qui contiennent une espace.
run Linux x86_64 serve --flag "a b"
[ "$out" = "ran linux-x86_64 serve --flag a b" ] || fail "arguments non transmis : '$out'"

# Plateforme inconnue : code 1, message sur stderr, stdout vide (stdout appartient au JSON-RPC).
for pair in "Linux riscv64" "Darwin ppc64" "FreeBSD amd64" "Linux i686"; do
  # shellcheck disable=SC2086 # la paire est volontairement scindée en deux arguments
  run $pair serve
  if [ "$code" != 1 ]; then fail "$pair : code attendu 1, obtenu $code"; fi
  if [ -n "$out" ]; then fail "$pair : stdout doit rester vide, obtenu '$out'"; fi
  case "$err" in
    "mcp-legifrance: unsupported platform "*) ;;
    *) fail "$pair : message inattendu '$err'" ;;
  esac
done

# Binaire absent ou non exécutable : échec net (code 1) avec un message, jamais un exec muet.
rm "$bundle/server/linux-x86_64/mcp-legifrance"
run Linux x86_64 serve
if [ "$code" != 1 ] || [ -n "$out" ]; then fail "binaire absent : code=$code stdout='$out'"; fi
case "$err" in
  "mcp-legifrance: missing or non-executable binary "*) ;;
  *) fail "binaire absent : message inattendu '$err'" ;;
esac
printf '#!/bin/sh\necho ran\n' >"$bundle/server/linux-x86_64/mcp-legifrance"
run Linux x86_64 serve
if [ "$code" != 1 ]; then fail "binaire non exécutable : code attendu 1, obtenu $code"; fi

# Lancé par un lien symbolique ou par un chemin relatif : le répertoire du bundle reste trouvé.
chmod +x "$bundle/server/linux-x86_64/mcp-legifrance"
(
  cd "$bundle/server"
  FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 PATH="$work/stubs:$PATH" ./launcher serve
) >"$work/out" 2>"$work/err" || fail "lancement par chemin relatif en échec"
[ "$(cat "$work/out")" = "ran" ] || fail "chemin relatif : sortie '$(cat "$work/out")'"

if [ "$failures" -ne 0 ]; then
  echo "$failures échec(s)" >&2
  exit 1
fi
echo "test-launcher : ok"
