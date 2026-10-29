#!/usr/bin/env bash
# Test de packaging/oci/verify-image.sh (08 §5.3, §6.5) : refus des références mal formées, et, avec
# VERIFY_TEST_ONLINE=1, lecture anonyme d'une image publique réelle (l'image distroless de base :
# utilisateur 65532, deux plateformes, aucun label du registre MCP).
#
# Usage : packaging/tests/test-verify-image.sh   (depuis n'importe quel répertoire ; exige jq, curl)
set -euo pipefail

root="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
script="$root/packaging/oci/verify-image.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
  echo "ECHEC : $*" >&2
  failures=$((failures + 1))
}

# check <code attendu> <extrait attendu> <commande...> : remplit $out ; l'extrait est cherché dans
# stdout et stderr réunis.
check() {
  local want="$1" needle="$2"
  shift 2
  local rc=0
  "$@" >"$work/out" 2>&1 || rc=$?
  out="$(cat "$work/out")"
  if [ "$rc" != "$want" ]; then fail "$*: code attendu $want, obtenu $rc ($out)"; fi
  case "$out" in
    *"$needle"*) ;;
    *) fail "$*: extrait attendu '$needle', obtenu '$out'" ;;
  esac
}

check 2 "usage" bash "$script"
check 2 "no tag" bash "$script" ghcr.io/muchiny/mcp-legifrance
check 2 "invalid tag" bash "$script" 'ghcr.io/muchiny/mcp-legifrance:0.1.0;id'
# shellcheck disable=SC2016 # l'expression de commande est volontairement littérale
check 2 "invalid repository" bash "$script" 'ghcr.io/Muchiny/$(id):0.1.0'
check 2 "invalid registry" bash "$script" 'gh cr.io/muchiny/x:1'

if [ "${VERIFY_TEST_ONLINE:-}" = 1 ]; then
  base=gcr.io/distroless/static-debian12:nonroot
  # Utilisateur, plateformes : conformes, sans contrôle de label. L'image de base déclare
  # `User: 65532` ; l'image du projet, qui écrit `USER 65532:65532`, déclare `65532:65532` (défaut).
  EXPECT_USER=65532 EXPECT_LABEL_NAME="" check 0 "linux/amd64:" bash "$script" "$base"
  EXPECT_USER=65532 EXPECT_LABEL_NAME="" check 0 "linux/arm64:" bash "$script" "$base"
  # Le label du registre MCP est absent de l'image de base : échec attendu, avec le bon message.
  EXPECT_USER=65532 check 1 "label io.modelcontextprotocol.server.name is ''" bash "$script" "$base"
  # Mauvais utilisateur attendu, plateforme absente.
  EXPECT_LABEL_NAME="" EXPECT_USER=0 check 1 "runs as '65532'" bash "$script" "$base"
  EXPECT_LABEL_NAME="" EXPECT_PLATFORMS="linux/riscv64" check 1 "no linux/riscv64 manifest" bash "$script" "$base"
  # Étiquette inexistante : lecture impossible.
  EXPECT_USER=65532 EXPECT_LABEL_NAME="" check 1 "cannot read the manifest" bash "$script" gcr.io/distroless/static-debian12:does-not-exist
  # Paquet GHCR inexistant ou privé : jeton anonyme refusé ou manifeste illisible, jamais un succès.
  rc=0
  bash "$script" ghcr.io/muchiny/mcp-legifrance-does-not-exist:0.0.0 >"$work/out" 2>&1 || rc=$?
  [ "$rc" = 1 ] || fail "paquet GHCR inexistant : code attendu 1, obtenu $rc"
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures échec(s)" >&2
  exit 1
fi
echo "test-verify-image : ok"
