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
# - Bornes : taille maximale par fichier et délais de connexion et de débit sur chaque requête.
# - Intégrité : la DILA ne publie aucune empreinte ; celle du global du 2025-07-13, relevée par
#   le projet (docs/recherche/10-dumps-dila.md), est contrôlée quand ce fichier est téléchargé.
#   Pour tout autre fichier, seuls TLS et la taille annoncée vous protègent.
#
# Source : DILA, base LEGI, Légifrance, Licence Ouverte (voir NOTICE et README, « Attribution »).

set -euo pipefail

BASE_URL="https://echanges.dila.gouv.fr/OPENDATA/LEGI"
USER_AGENT="mcp-legifrance-fetch/1.0 (+https://github.com/muchiny/mcp-legifrance)"
PAUSE_SECONDS=1
MAX_LISTING_BYTES=4194304        # le listing fait environ 76 Ko
MAX_GLOBAL_BYTES=2147483648      # le global du 2025-07-13 fait 1,17 Go
MAX_INCREMENT_BYTES=536870912    # le plus gros incrément mesuré fait 43,5 Mo
KNOWN_GLOBAL="Freemium_legi_global_20250713-140000.tar.gz"
KNOWN_GLOBAL_SHA256="f862b4a2238e11ec3c48ec70f3b1c68bac0bb1985907841666b98608653d4eca"

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  echo "usage : $0 <dossier-des-archives>" >&2
  exit 2
fi
dest="$1"
mkdir -p -- "$dest"
# Les archives sont ingérées telles quelles : le dossier ne doit appartenir qu'à vous.
if [ ! -O "$dest" ]; then
  echo "le dossier $dest n'appartient pas à l'utilisateur courant : choisissez-en un autre" >&2
  exit 1
fi

fetch() {
  curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
    --user-agent "$USER_AGENT" "$@"
}

listing="$(fetch --max-filesize "$MAX_LISTING_BYTES" --connect-timeout 30 "$BASE_URL/")"

# Noms de la forme exacte attendue par le registre (archive.rs) ; tout le reste est ignoré.
global="$(printf '%s\n' "$listing" \
  | { grep -oE 'Freemium_legi_global_20[0-9]{2}[01][0-9][0-3][0-9]-[0-2][0-9][0-5][0-9][0-5][0-9]\.tar\.gz' || true; } | sort -u | tail -n 1)"
if [ -z "$global" ]; then
  echo "aucun global LEGI trouvé dans $BASE_URL/" >&2
  exit 1
fi
# Le global et l'incrément partagent le même format d'horodatage : on compare les chaînes.
stamp="${global#Freemium_legi_global_}"
stamp="${stamp%.tar.gz}"

increments=()
while IFS= read -r name; do
  increments+=("$name")
done < <(printf '%s\n' "$listing" \
  | { grep -oE 'LEGI_20[0-9]{2}[01][0-9][0-3][0-9]-[0-2][0-9][0-5][0-9][0-5][0-9]\.tar\.gz' || true; } \
  | sort -u \
  | while read -r name; do
      s="${name#LEGI_}"
      s="${s%.tar.gz}"
      if [[ "$s" > "$stamp" ]]; then printf '%s\n' "$name"; fi
    done)

echo "global : $global ; incréments à appliquer : ${#increments[@]}"

sha256_matches() {
  local file="$1" expected="$2" actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$file" | cut -d' ' -f1)"
  else
    actual="$(shasum -a 256 "$file" | cut -d' ' -f1)"
  fi
  [ "$actual" = "$expected" ]
}

download() {
  local name="$1" remote_size="" limit="$MAX_INCREMENT_BYTES"
  case "$name" in Freemium_*) limit="$MAX_GLOBAL_BYTES" ;; esac
  if [ -L "$dest/$name" ] || [ -L "$dest/$name.part" ]; then
    echo "$dest/$name est un lien symbolique : supprimez-le et relancez" >&2
    exit 1
  fi
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
  fetch -C - --max-filesize "$limit" --connect-timeout 30 --speed-limit 1024 --speed-time 120 \
    --output "$dest/$name.part" "$BASE_URL/$name"
  if [ -n "$remote_size" ] && [ "$(wc -c < "$dest/$name.part")" != "$remote_size" ]; then
    echo "taille inattendue pour $name : relancez le script pour reprendre" >&2
    exit 1
  fi
  if [ "$name" = "$KNOWN_GLOBAL" ] && ! sha256_matches "$dest/$name.part" "$KNOWN_GLOBAL_SHA256"; then
    echo "empreinte SHA-256 inattendue pour $name : fichier laissé dans $name.part" >&2
    exit 1
  fi
  mv -f -- "$dest/$name.part" "$dest/$name"
  sleep "$PAUSE_SECONDS"
}

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf 'à télécharger : %s\n' "$global" ${increments[@]+"${increments[@]}"}
  exit 0
fi

download "$global"
for name in ${increments[@]+"${increments[@]}"}; do
  download "$name"
done

echo "terminé : $dest"
printf 'étape suivante : mcp-legifrance build-dila-index --archives %q --index <dossier-de-l-index>\n' "$dest"
