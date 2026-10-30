#!/usr/bin/env bash
# Poignée de main TLS en conteneur (08 §5.1, V-204) : prouve que le binaire musl statique de
# l'image distroless trouve un magasin de racines CA et négocie TLS avec le vrai endpoint OAuth de
# PISTE. Aucun identifiant réel : le serveur reçoit des identifiants factices et un appel d'outil
# provoque la requête de jeton. Un refus d'identifiants (`[config_error]`) prouve que la
# connexion TLS a abouti et que PISTE a répondu ; `[upstream_error]` est une panne TLS ou réseau.
# L'appel passe aussi par le journal d'audit en mode strict : `[audit_error]` dénonce un
# répertoire d'audit non inscriptible pour l'utilisateur non root de l'image (V-167).
#
# Usage : packaging/smoke/handshake.sh <commande> [arguments...]
#   par exemple : packaging/smoke/handshake.sh docker run -i --rm \
#                   -e LF_PISTE_CLIENT_ID=ci-smoke -e LF_PISTE_CLIENT_SECRET=ci-smoke <image>
# Réglages : HANDSHAKE_ATTEMPTS (3), HANDSHAKE_WAIT (20 s : stdin reste ouvert, car un EOF pendant
# un appel l'annule), HANDSHAKE_PAUSE (10 s entre deux tentatives). Une requête de jeton au plus
# par tentative : c'est tout ce que PISTE reçoit. Exige jq.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <command> [arguments...]" >&2
  exit 2
fi
attempts="${HANDSHAKE_ATTEMPTS:-3}"
wait_s="${HANDSHAKE_WAIT:-20}"
pause_s="${HANDSHAKE_PAUSE:-10}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

init='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"release-smoke","version":"0"}}}'
initialized='{"jsonrpc":"2.0","method":"notifications/initialized"}'
call='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fr_search","arguments":{"query":"code civil"}}}'

last=""
for attempt in $(seq 1 "$attempts"); do
  code=0
  { printf '%s\n%s\n%s\n' "$init" "$initialized" "$call"; sleep "$wait_s"; } | "$@" >"$work/stdout" 2>"$work/stderr" || code=$?

  rpc_error="$(jq -r 'select(.id == 2) | .error.message // empty' "$work/stdout" 2>/dev/null || true)"
  text="$(jq -r 'select(.id == 2) | .result.content[0].text // empty' "$work/stdout" 2>/dev/null || true)"
  is_error="$(jq -r 'select(.id == 2) | (.result.isError | tostring)' "$work/stdout" 2>/dev/null || true)"

  if [ -n "$rpc_error" ]; then
    last="JSON-RPC error on tools/call: $rpc_error"
  elif [ -z "$text" ]; then
    last="no response to tools/call (server exit code $code)"
  else
    case "$text" in
      "[config_error]"*)
        echo "TLS handshake ok: PISTE answered and refused the dummy credentials ($text)"
        exit 0
        ;;
      "[audit_error]"*)
        last="audit log unavailable inside the image (V-167): $text"
        # Inutile de réessayer : un répertoire non inscriptible ne se répare pas seul.
        echo "attempt $attempt/$attempts: $last" >&2
        echo "--- stderr" >&2
        cat "$work/stderr" >&2
        exit 1
        ;;
      "[upstream_error]"*) last="TLS or network failure (V-204): $text" ;;
      *)
        if [ "$is_error" = false ]; then
          last="unexpected success with dummy credentials: $text"
        else
          last="unexpected tool result: $text"
        fi
        ;;
    esac
  fi
  echo "attempt $attempt/$attempts: $last" >&2
  if [ "$attempt" -lt "$attempts" ]; then
    sleep "$pause_s"
  fi
done

echo "--- stderr" >&2
cat "$work/stderr" >&2
echo "handshake: $last" >&2
exit 1
