#!/usr/bin/env bash
# Test de packaging/smoke/handshake.sh (08 §5.1, V-204) avec de faux serveurs MCP : le script doit
# accepter un refus d'identifiants (preuve qu'une poignée de main TLS a abouti) et refuser tout
# autre résultat. Aucun réseau, aucun binaire réel.
#
# Usage : packaging/tests/test-handshake.sh   (depuis n'importe quel répertoire ; exige jq)
set -euo pipefail

root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
handshake="$root/packaging/smoke/handshake.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
  echo "ECHEC : $*" >&2
  failures=$((failures + 1))
}

# Faux serveur : répond à initialize (id 1) et à tools/call (id 2), comme le serveur réel, puis
# attend l'EOF de stdin. FAKE_TEXT est le texte de l'erreur d'outil ; FAKE_MODE sélectionne un
# comportement : error (défaut, isError), success (isError faux), rpc-error, silent.
cat >"$work/fake-server" <<'FAKE'
#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"id":1'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"mcp-legifrance","version":"0.1.0"}}}'
      ;;
    *'"id":2'*)
      case "${FAKE_MODE:-error}" in
        error) printf '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"%s"}],"isError":true}}\n' "$FAKE_TEXT" ;;
        success) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"ok"}],"isError":false}}' ;;
        rpc-error) printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"bad"}}' ;;
        silent) ;;
      esac
      ;;
  esac
done
FAKE
chmod +x "$work/fake-server"

export HANDSHAKE_ATTEMPTS=2 HANDSHAKE_WAIT=1 HANDSHAKE_PAUSE=0

run() { # code attendu, extrait attendu ; variables FAKE_* dans l'environnement
  local want="$1" needle="$2" rc=0
  bash "$handshake" "$work/fake-server" >"$work/out" 2>&1 || rc=$?
  local out
  out="$(cat "$work/out")"
  if [ "$rc" != "$want" ]; then fail "FAKE_MODE=${FAKE_MODE:-error} FAKE_TEXT=${FAKE_TEXT:-}: code attendu $want, obtenu $rc ($out)"; fi
  case "$out" in
    *"$needle"*) ;;
    *) fail "FAKE_MODE=${FAKE_MODE:-error}: extrait attendu '$needle', obtenu '$out'" ;;
  esac
}

# Identifiants refusés par PISTE : la poignée de main TLS a abouti.
FAKE_TEXT="[config_error] PISTE refused the credentials." run 0 "TLS handshake ok"
# Panne amont (TLS ou réseau), après les tentatives : échec.
FAKE_TEXT="[upstream_error] PISTE is unavailable." run 1 "attempt 2/2"
FAKE_TEXT="[upstream_error] PISTE is unavailable." run 1 "[upstream_error]"
# Répertoire d'audit non inscriptible dans l'image : message dédié (V-167).
FAKE_TEXT="[audit_error] audit log unavailable." run 1 "audit"
# Succès inattendu, erreur JSON-RPC, serveur muet : tous refusés.
FAKE_MODE=success run 1 "unexpected"
FAKE_MODE=rpc-error run 1 "JSON-RPC error"
FAKE_MODE=silent run 1 "no response"

# Sans commande : usage.
rc=0
bash "$handshake" >"$work/out" 2>&1 || rc=$?
[ "$rc" = 2 ] || fail "sans argument : code attendu 2, obtenu $rc"

if [ "$failures" -ne 0 ]; then
  echo "$failures échec(s)" >&2
  exit 1
fi
echo "test-handshake : ok"
