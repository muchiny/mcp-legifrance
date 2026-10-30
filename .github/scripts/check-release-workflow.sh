#!/usr/bin/env bash
# Garde-fous du workflow de release (F4, SUP-01 et SUP-02) : échoue si `release.yml` retombe dans
# l'état dangereux du gabarit de dist.
#
# Usage : .github/scripts/check-release-workflow.sh [fichier]     (défaut .github/workflows/release.yml)
#
# Règles :
#   1. `permissions:` du workflow vaut `{}` : aucun droit hérité par un job qui n'en déclare pas ;
#   2. chaque job déclare ses `permissions:` ;
#   3. seul le job `host` a `contents: write` (création de la release) ;
#   4. `GH_TOKEN` n'entre dans l'environnement que de `plan` (lecture seule) et de `host` : un
#      build.rs ou une macro procédurale compilés par `build-local-artifacts` ou
#      `build-global-artifacts` ne voient jamais le jeton ;
#   5. aucune installation par `| sh`, `| bash` ou `| iex`, ni par la commande `install_*.run` du
#      plan de dist : les outils passent par install-release-tool.sh (sommes épinglées).
# Le fichier est analysé ligne à ligne (indentation du YAML généré par dist), sans dépendance.
set -euo pipefail

file="${1:-.github/workflows/release.yml}"
[[ -f "$file" ]] || {
  echo "check-release-workflow: $file not found" >&2
  exit 2
}

awk '
  function fail(msg) { printf "%s:%d: %s\n", FILENAME, NR, msg; bad = 1 }
  function unquote(s) { gsub(/["\047 ]/, "", s); return s }
  BEGIN { job = ""; in_perms = 0; top_perms = 0; top_block = 0 }
  /^[[:space:]]*#/ { next }
  # Niveau workflow.
  /^permissions:/ {
    top_perms = 1
    v = $0; sub(/^permissions:[[:space:]]*/, "", v)
    if (v == "{}") { top_block = 0 } else { top_block = 1; fail("workflow-level permissions must be {}") }
    next
  }
  /^jobs:/ { in_jobs = 1; in_perms = 0; top_block = 0; next }
  /^[^[:space:]]/ { in_perms = 0; top_block = 0; in_jobs = 0 }
  top_block && /write/ { fail("workflow-level write permission") }
  # Un job : deux espaces puis son nom.
  in_jobs && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ {
    job = $0; sub(/^  /, "", job); sub(/:.*/, "", job)
    jobs[job] = 1; in_perms = 0
    next
  }
  in_jobs && /^    permissions:/ { has_perms[job] = 1; in_perms = 1; next }
  in_jobs && /^    [A-Za-z]/ { in_perms = 0 }
  in_jobs && in_perms && /^      / {
    line = $0; gsub(/["\047]/, "", line)
    if (line ~ /contents:[[:space:]]*write/ && job != "host") fail("job " job " has contents: write (only host may)")
  }
  in_jobs && /GH_TOKEN:/ && job != "host" && job != "plan" { fail("GH_TOKEN in the environment of job " job " (only plan and host)") }
  /\|[[:space:]]*(sh|bash|iex)([[:space:]"]|$)/ { fail("pipe into a shell: install tools with install-release-tool.sh") }
  /matrix\.install_[a-z_]+\.run/ { fail("install command taken from the dist plan: use install-release-tool.sh") }
  END {
    if (!top_perms) { print FILENAME ": missing workflow-level permissions: {}"; bad = 1 }
    for (j in jobs) if (!(j in has_perms)) { print FILENAME ": job " j " has no permissions block"; bad = 1 }
    exit bad
  }
' "$file"
