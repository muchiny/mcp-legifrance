#!/usr/bin/env bash
# Test de .github/scripts/install-mcp-publisher.sh (08 §3.7, V-206) : le script n'installe que la
# version dont il connaît la somme SHA-256, et refuse tout fichier qui ne la porte pas.
#
# Hors ligne par défaut (faux dépôt de releases via file://). Avec INSTALL_TEST_ONLINE=1, installe
# aussi la vraie version épinglée depuis GitHub et vérifie que le binaire démarre.
#
# Usage : packaging/tests/test-install-mcp-publisher.sh   (depuis n'importe quel répertoire)
set -euo pipefail

root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
script="$root/.github/scripts/install-mcp-publisher.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
  echo "ECHEC : $*" >&2
  failures=$((failures + 1))
}

# run <arguments> : remplit $out, $err, $code ; installe dans $work/bin, jamais dans /usr/local/bin.
run() {
  local rc=0
  : >"$work/out"
  : >"$work/err"
  MCP_PUBLISHER_BIN_DIR="$work/bin" "$@" >"$work/out" 2>"$work/err" || rc=$?
  out="$(cat "$work/out")"
  err="$(cat "$work/err")"
  code=$rc
}

expect_ko() { # code attendu, extrait du message ; puis la commande
  local want_code="$1" needle="$2"
  shift 2
  run "$@"
  if [ "$code" != "$want_code" ]; then fail "$*: code attendu $want_code, obtenu $code ($err)"; fi
  case "$err$out" in
    *"$needle"*) ;;
    *) fail "$*: message attendu '$needle', obtenu '$err$out'" ;;
  esac
  if [ -e "$work/bin/mcp-publisher" ]; then fail "$*: rien ne doit être installé après un refus"; fi
}

# Usage et entrées invalides : code 2.
expect_ko 2 "usage" bash "$script"
expect_ko 2 "usage" bash "$script" 1.8
expect_ko 2 "usage" bash "$script" latest
expect_ko 2 "usage" bash "$script" "1.8.1; rm -rf /"

# Version dont la somme n'est pas épinglée : refus, même si le téléchargement serait possible.
expect_ko 1 "no pinned checksum" bash "$script" 9.9.9

# Archive falsifiée : faux dépôt de releases, somme différente de celle épinglée.
fake="$work/releases/v1.8.1"
mkdir -p "$fake" "$work/payload"
printf '#!/bin/sh\necho evil\n' >"$work/payload/mcp-publisher"
tar -czf "$fake/mcp-publisher_linux_amd64.tar.gz" -C "$work/payload" mcp-publisher
cp "$fake/mcp-publisher_linux_amd64.tar.gz" "$fake/mcp-publisher_linux_arm64.tar.gz"
cp "$fake/mcp-publisher_linux_amd64.tar.gz" "$fake/mcp-publisher_darwin_amd64.tar.gz"
cp "$fake/mcp-publisher_linux_amd64.tar.gz" "$fake/mcp-publisher_darwin_arm64.tar.gz"
# Un fichier de sommes « cohérent » avec l'archive falsifiée ne doit pas suffire à la faire passer.
(cd "$fake" && sha256sum mcp-publisher_*.tar.gz >registry_1.8.1_checksums.txt)
export MCP_PUBLISHER_BASE_URL="file://$work/releases"
expect_ko 1 "checksum mismatch" bash "$script" 1.8.1

# Plateforme non prise en charge : refus net.
mkdir -p "$work/stubs"
cat >"$work/stubs/uname" <<'STUB'
#!/bin/sh
case "$1" in -s) echo Plan9 ;; -m) echo mips ;; esac
STUB
chmod +x "$work/stubs/uname"
PATH="$work/stubs:$PATH" expect_ko 1 "unsupported platform" bash "$script" 1.8.1
unset MCP_PUBLISHER_BASE_URL

if [ "${INSTALL_TEST_ONLINE:-}" = 1 ]; then
  run bash "$script" 1.8.1
  if [ "$code" != 0 ]; then fail "installation en ligne : code $code ($err)"; fi
  if [ ! -x "$work/bin/mcp-publisher" ]; then fail "installation en ligne : binaire absent"; fi
  # Le binaire épinglé démarre (aide, sans réseau ni identifiant).
  if ! "$work/bin/mcp-publisher" --help >/dev/null 2>&1 && ! "$work/bin/mcp-publisher" help >/dev/null 2>&1; then
    fail "installation en ligne : le binaire ne démarre pas"
  fi
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures échec(s)" >&2
  exit 1
fi
echo "test-install-mcp-publisher : ok"
