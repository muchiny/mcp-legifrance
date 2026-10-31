#!/usr/bin/env bash
# Télécharge les archives du fonds LEGI (open data de la DILA) dans un dossier, pour
# `mcp-legifrance build-dila-index` (M9, mode hors ligne). Hors du binaire : il ne contacte jamais
# la DILA lui-même (docs/recherche/10-dumps-dila.md §10).
#
# Usage : scripts/fetch-dila-dumps.sh <dossier-des-archives>
#
# - Reprise : un fichier déjà complet n'est pas retéléchargé ; un téléchargement interrompu reprend
#   (`-C -`) dans `<nom>.part`, que `build-dila-index` ignore, et n'est renommé qu'une fois complet.
# - Politesse (rapport §12.9) : une seule connexion, un User-Agent qui s'identifie, une pause
#   entre deux requêtes. Le global pèse environ 1,2 Go, les incréments environ 2 Go en tout.
# - Seuls le global le plus récent et les incréments qui le suivent sont téléchargés.
# - `DRY_RUN=1` : liste ce qui serait téléchargé, sans rien télécharger.
#
# Source : DILA, base LEGI, Légifrance, Licence Ouverte (voir NOTICE et README, « Attribution »).

set -euo pipefail

BASE_URL="https://echanges.dila.gouv.fr/OPENDATA/LEGI"
USER_AGENT="mcp-legifrance-fetch/1.0 (+https://github.com/muchiny/mcp-legifrance)"
PAUSE_SECONDS=1

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  echo "usage : $0 <dossier-des-archives>" >&2
  exit 2
fi
dest="$1"
mkdir -p "$dest"

fetch() {
  curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
    --user-agent "$USER_AGENT" "$@"
}

listing="$(fetch "$BASE_URL/")"

# Noms de la forme exacte attendue par le registre (archive.rs) ; tout le reste est ignoré.
global="$(printf '%s\n' "$listing" \
  | { grep -oE 'Freemium_legi_global_[0-9]{8}-[0-9]{6}\.tar\.gz' || true; } | sort -u | tail -n 1)"
if [ -z "$global" ]; then
  echo "aucun global LEGI trouvé dans $BASE_URL/" >&2
  exit 1
fi
# Le global et l'incrément partagent le même format d'horodatage : on compare les chaînes.
stamp="${global#Freemium_legi_global_}"
stamp="${stamp%.tar.gz}"

mapfile -t increments < <(printf '%s\n' "$listing" \
  | { grep -oE 'LEGI_[0-9]{8}-[0-9]{6}\.tar\.gz' || true; } | sort -u \
  | while read -r name; do
      s="${name#LEGI_}"
      s="${s%.tar.gz}"
      if [[ "$s" > "$stamp" ]]; then printf '%s\n' "$name"; fi
    done)

echo "global : $global ; incréments à appliquer : ${#increments[@]}"

download() {
  local name="$1" remote_size=""
  # Un HEAD refusé n'arrête pas le script : sans taille distante, on télécharge (avec reprise).
  remote_size="$(fetch --head "$BASE_URL/$name" 2>/dev/null \
    | tr -d '\r' | awk 'tolower($1) == "content-length:" { size = $2 } END { print size }' || true)"
  sleep "$PAUSE_SECONDS"
  if [ -f "$dest/$name" ] && [ -n "$remote_size" ] \
    && [ "$(wc -c < "$dest/$name")" = "$remote_size" ]; then
    echo "déjà complet : $name"
    return
  fi
  echo "téléchargement : $name"
  fetch -C - --output "$dest/$name.part" "$BASE_URL/$name"
  if [ -n "$remote_size" ] && [ "$(wc -c < "$dest/$name.part")" != "$remote_size" ]; then
    echo "taille inattendue pour $name : relancez le script pour reprendre" >&2
    exit 1
  fi
  mv "$dest/$name.part" "$dest/$name"
  sleep "$PAUSE_SECONDS"
}

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf 'à télécharger : %s\n' "$global" "${increments[@]}"
  exit 0
fi

download "$global"
for name in "${increments[@]}"; do
  download "$name"
done

echo "terminé : $dest"
echo "étape suivante : mcp-legifrance build-dila-index --archives '$dest' --index <dossier-de-l-index>"
