#!/usr/bin/env bash
# Test de packaging/smoke/initialize.sh (08 §4.5) avec de faux serveurs : le script doit accepter
# la réponse attendue et refuser toute dérive (nom, version, ligne en trop, sortie en échec).
# Aucun réseau, aucun binaire réel.
#
# Usage : packaging/tests/test-smoke.sh   (depuis n'importe quel répertoire ; exige jq)
set -euo pipefail

root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
smoke="$root/packaging/smoke/initialize.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
  echo "ECHEC : $*" >&2
  failures=$((failures + 1))
}

# fake <nom> <corps du script> : crée un faux serveur qui lit stdin jusqu'à l'EOF.
fake() {
  printf '#!/bin/sh\n%s\n' "$2" >"$work/$1"
  chmod +x "$work/$1"
}

reply='{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"mcp-legifrance","version":"0.1.0"}}}'

fake good "cat >/dev/null; printf '%s\n' '$reply'"
fake good-crlf "cat >/dev/null; printf '%s\r\n' '$reply'"
fake good-args "cat >/dev/null; [ \"\$1\" = serve ] || exit 9; printf '%s\n' '$reply'"
fake wrong-version "cat >/dev/null; printf '%s\n' '${reply/0.1.0/0.2.0}'"
fake wrong-name "cat >/dev/null; printf '%s\n' '${reply/mcp-legifrance/other}'"
fake wrong-id "cat >/dev/null; printf '%s\n' '${reply/\"id\":1/\"id\":2}'"
fake two-lines "cat >/dev/null; printf '%s\n%s\n' 'log line on stdout' '$reply'"
fake no-output 'cat >/dev/null'
fake not-json "cat >/dev/null; echo 'hello'"
fake exits-1 "cat >/dev/null; printf '%s\n' '$reply'; exit 1"
fake stderr-only "cat >/dev/null; echo 'boom' >&2; exit 3"

expect_ok() { # nom version [arguments]
  local name="$1" version="$2"
  shift 2
  if ! bash "$smoke" "$version" "$work/$name" "$@" >"$work/out" 2>"$work/err"; then
    fail "$name devrait réussir : $(cat "$work/out" "$work/err")"
  fi
}
expect_ko() { # nom version message attendu dans la sortie
  local name="$1" version="$2" needle="$3"
  if bash "$smoke" "$version" "$work/$name" >"$work/out" 2>"$work/err"; then
    fail "$name devrait échouer"
  elif ! grep -qF -- "$needle" "$work/out" "$work/err"; then
    fail "$name : message attendu '$needle', obtenu : $(cat "$work/out" "$work/err")"
  fi
}

expect_ok good 0.1.0
expect_ok good-crlf 0.1.0
expect_ok good-args 0.1.0 serve
expect_ko good 0.1.1 "version"
expect_ko wrong-version 0.1.0 "version"
expect_ko wrong-name 0.1.0 "serverInfo"
expect_ko wrong-id 0.1.0 "serverInfo"
expect_ko two-lines 0.1.0 "2 lines"
expect_ko no-output 0.1.0 "0 lines"
expect_ko not-json 0.1.0 "not a JSON-RPC"
expect_ko exits-1 0.1.0 "exited with code 1"
expect_ko stderr-only 0.1.0 "boom"

# Arguments manquants : usage, code 2.
rc=0
bash "$smoke" >"$work/out" 2>"$work/err" || rc=$?
[ "$rc" = 2 ] || fail "sans argument : code attendu 2, obtenu $rc"

# Commande introuvable : échec explicite.
rc=0
bash "$smoke" 0.1.0 "$work/does-not-exist" >"$work/out" 2>"$work/err" || rc=$?
[ "$rc" != 0 ] || fail "commande introuvable : devrait échouer"

if [ "$failures" -ne 0 ]; then
  echo "$failures échec(s)" >&2
  exit 1
fi
echo "test-smoke : ok"
