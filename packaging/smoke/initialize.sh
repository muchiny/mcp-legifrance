#!/usr/bin/env bash
# Test fumée `initialize` (08 §4.5) : lance une commande qui parle MCP sur stdio (binaire, lanceur
# MCPB ou `docker run -i --rm <image>`), lui envoie une requête `initialize` puis ferme stdin, et
# vérifie la réponse. Sans réseau et sans identifiant : le démarrage n'appelle pas PISTE (00 §6).
#
# Usage : packaging/smoke/initialize.sh <version attendue> <commande> [arguments...]
#
# Attendu : code de sortie 0 sur EOF, et stdout = une seule ligne JSON-RPC, la réponse à l'id 1,
# avec result.serverInfo.name == "mcp-legifrance" et result.serverInfo.version == <version>.
# Portable bash 3.2 (macOS) et Git Bash (Windows) ; exige jq.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <expected version> <command> [arguments...]" >&2
  exit 2
fi
version="$1"
shift

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"release-smoke","version":"0"}}}' >"$work/request"

code=0
"$@" <"$work/request" >"$work/stdout" 2>"$work/stderr" || code=$?

echo "--- stderr"
cat "$work/stderr"
echo "--- stdout"
cat "$work/stdout"

if [ "$code" -ne 0 ]; then
  echo "::error::the server exited with code $code (expected 0 on EOF)"
  exit 1
fi

# Fins de ligne Windows retirées ; une seule ligne de réponse, rien d'autre sur stdout.
tr -d '\r' <"$work/stdout" >"$work/stdout.lf"
lines="$(wc -l <"$work/stdout.lf" | tr -d ' ')"
if [ "$lines" != 1 ]; then
  echo "::error::stdout carries $lines lines instead of one JSON-RPC response"
  exit 1
fi
if ! jq -e . "$work/stdout.lf" >/dev/null 2>&1; then
  echo "::error::stdout is not a JSON-RPC message"
  exit 1
fi
if ! jq -e --arg version "$version" \
  '.jsonrpc == "2.0" and .id == 1 and .result.serverInfo.name == "mcp-legifrance" and .result.serverInfo.version == $version' \
  "$work/stdout.lf" >/dev/null; then
  echo "::error::unexpected initialize response: expected id 1, serverInfo.name mcp-legifrance and serverInfo.version $version"
  exit 1
fi
echo "ok : initialize, mcp-legifrance $version"
