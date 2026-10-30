#!/usr/bin/env bash
# Étiquette, version et caractère « préversion » d'une release (08 §6.5), pour les workflows
# réutilisables appelés par dist (build-mcpb, publish-oci, publish-mcp-registry).
#
# Usage : packaging/resolve-release.sh >> "$GITHUB_OUTPUT"
#   PLAN  plan de dist au format JSON (entrée `plan` des workflows réutilisables) ; ou
#   TAG   étiquette seule (déclenchement manuel `workflow_dispatch`).
# Sortie : trois lignes `tag=vX.Y.Z`, `version=X.Y.Z`, `prerelease=true|false`. Les valeurs sont
# validées avant d'être écrites : aucun caractère de contrôle ne peut atteindre $GITHUB_OUTPUT.
set -euo pipefail

plan="${PLAN:-}"
tag="${TAG:-}"
prerelease=""

if [ -n "$plan" ]; then
  tag="$(printf '%s' "$plan" | jq -er '.announcement_tag' 2>/dev/null)" || {
    echo "resolve-release: announcement_tag is missing from the dist plan" >&2
    exit 1
  }
  # Pas de `-e` : `false` est une valeur valide, mais jq -e sortirait en échec sur elle.
  prerelease="$(printf '%s' "$plan" | jq -r 'if (.announcement_is_prerelease | type) == "boolean" then .announcement_is_prerelease else error("not a boolean") end' 2>/dev/null)" || {
    echo "resolve-release: announcement_is_prerelease is missing from the dist plan" >&2
    exit 1
  }
elif [ -z "$tag" ]; then
  echo "resolve-release: PLAN or TAG is required" >&2
  exit 1
fi

# Étiquette vX.Y.Z, avec suffixe de préversion facultatif (-rc.1) : la forme que posent
# release-gates.yml (« v$VERSION ») et dist.
re='^v[0-9]+[.][0-9]+[.][0-9]+(-[0-9A-Za-z.-]+)?$'
if ! [[ "$tag" =~ $re ]]; then
  printf 'resolve-release: invalid tag %q (expected vX.Y.Z or vX.Y.Z-suffix)\n' "$tag" >&2
  exit 1
fi
version="${tag#v}"

if [ -n "$plan" ]; then
  planned="$(printf '%s' "$plan" | jq -r '[.releases[]? | select(.app_name == "mcp-legifrance") | .app_version][0] // empty')"
  if [ -n "$planned" ] && [ "$planned" != "$version" ]; then
    printf 'resolve-release: tag %s does not match the version %q of the dist plan\n' "$tag" "$planned" >&2
    exit 1
  fi
else
  case "$version" in
    *-*) prerelease=true ;;
    *) prerelease=false ;;
  esac
fi

printf 'tag=%s\nversion=%s\nprerelease=%s\n' "$tag" "$version" "$prerelease"
