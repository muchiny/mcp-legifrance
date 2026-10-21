#!/usr/bin/env bash
# Règle d'âge des dépendances (docs/plan/00-decisions.md §2 ; 06-qualite.md §9.4).
#
# Chaque paquet du registre crates.io ajouté ou changé dans Cargo.lock par rapport à la base
# doit être publié depuis MIN_AGE_DAYS jours au moins : `pubtime` de l'index sparse, à défaut
# `created_at` de l'API crates.io. Un paquet retiré est ignoré ; une montée de version compte
# comme un ajout. Lancé par `make min-age` (étape min-age du job deny, sur PR seulement) ;
# auto-test : `make min-age-selftest`.
#
# Entrées (environnement) :
#   MIN_AGE_BASE           révision git de base (défaut : origin/main)
#   MIN_AGE_DAYS           âge minimal en jours (défaut : 14)
#   SECURITY_FIX           "true" : violations en avertissements, code 0 (libellé security-fix,
#                          justification exigée dans la description de la PR)
#   MIN_AGE_NOW            instant de référence, lu par `date -d` (défaut : maintenant, UTC)
#   MIN_AGE_INDEX_URL      index sparse (défaut : https://index.crates.io)
#   MIN_AGE_API_URL        API crates.io (défaut : https://crates.io/api/v1)
#   MIN_AGE_LOCKFILE       verrou de la tête (défaut : Cargo.lock)
#   MIN_AGE_BASE_LOCKFILE  verrou de base déjà extrait, à la place de `git show` (auto-test)
#   GITHUB_ACTIONS         "true" : messages en annotations ::error:: et ::warning::
#
# Sortie : une ligne par paquet, « nom version pubtime âge=Nj éligible=AAAA-MM-JJ ».
# Codes : 0 rien de nouveau ou tout est éligible ; 1 violation ; 2 erreur d'usage, de git,
# de réseau ou d'analyse (le contrôle échoue fermé).
set -euo pipefail

base=${MIN_AGE_BASE:-origin/main}
days=${MIN_AGE_DAYS:-14}
index_url=${MIN_AGE_INDEX_URL:-https://index.crates.io}
api_url=${MIN_AGE_API_URL:-https://crates.io/api/v1}
lockfile=${MIN_AGE_LOCKFILE:-Cargo.lock}
user_agent="mcp-legifrance check-min-age (https://github.com/muchiny/mcp-legifrance)"
registry="registry+https://github.com/rust-lang/crates.io-index"

# Message de niveau error ou warning ; annotation GitHub en CI.
say() { # niveau message
  if [[ ${GITHUB_ACTIONS:-} == true ]]; then
    echo "::$1::$2"
  elif [[ $1 == error ]]; then
    echo "erreur : $2"
  else
    echo "avertissement : $2"
  fi
}

# Erreur d'usage, de git, de réseau ou d'analyse : code 2.
die() {
  say error "$1" >&2
  exit 2
}

# Couples « nom version » des paquets du registre crates.io d'un verrou (entrée standard),
# triés ; les membres du workspace (sans source) et les dépendances git sont écartés.
registry_packages() {
  awk -v registry="$registry" '
    /^name = "/ { name = $3; gsub(/"/, "", name) }
    /^version = "/ { version = $3; gsub(/"/, "", version) }
    index($0, "source = \"" registry) == 1 { print name, version }
  ' | LC_ALL=C sort -u
}

# Chemin d'un crate dans l'index sparse, nom en minuscules : 1/<n>, 2/<n>, 3/<c1>/<n>, sinon
# <c1c2>/<c3c4>/<n> (https://doc.rust-lang.org/cargo/reference/registry-index.html).
index_path() {
  local n=${1,,}
  case ${#n} in
    1) echo "1/$n" ;;
    2) echo "2/$n" ;;
    3) echo "3/${n:0:1}/$n" ;;
    *) echo "${n:0:2}/${n:2:2}/$n" ;;
  esac
}

# Date de publication d'une version selon l'index sparse (`pubtime`), vide si absente.
index_pubtime() { # nom version
  curl -fsS --retry 3 -A "$user_agent" "$index_url/$(index_path "$1")" |
    jq -r --arg v "$2" 'select(.vers == $v) | .pubtime // empty'
}

# Date de publication selon l'API crates.io (`created_at`), vide si absente.
api_pubtime() { # nom version
  curl -fsS --retry 3 -A "$user_agent" "$api_url/crates/$1/$2" |
    jq -r '.version.created_at // empty'
}

[[ $days =~ ^[0-9]+$ ]] || die "MIN_AGE_DAYS doit être un nombre de jours (reçu : $days)"
if [[ -n ${MIN_AGE_NOW:-} ]]; then
  now=$(date -u -d "$MIN_AGE_NOW" +%s) || die "MIN_AGE_NOW illisible : $MIN_AGE_NOW"
else
  now=$(date -u +%s)
fi

[[ -r $lockfile ]] || die "verrou illisible : $lockfile"
head_packages=$(registry_packages <"$lockfile")

if [[ -n ${MIN_AGE_BASE_LOCKFILE:-} ]]; then
  base=$MIN_AGE_BASE_LOCKFILE
  [[ -r $base ]] || die "verrou de base illisible : $base"
  base_packages=$(registry_packages <"$base")
else
  git rev-parse --verify --quiet "$base^{commit}" >/dev/null ||
    die "révision de base introuvable : $base (historique complet requis, fetch-depth: 0)"
  if git cat-file -e "$base:$lockfile" 2>/dev/null; then
    base_packages=$(git show "$base:$lockfile" | registry_packages)
  else
    base_packages="" # verrou absent de la base : tout paquet compte comme nouveau
  fi
fi

new=$(LC_ALL=C comm -13 <(echo "$base_packages") <(echo "$head_packages"))
if [[ -z $new ]]; then
  echo "check-min-age : aucun paquet ajouté ou changé depuis $base"
  exit 0
fi
echo "check-min-age : $(wc -l <<<"$new") paquet(s) ajouté(s) ou changé(s) depuis $base, âge minimal $days j"

violations=0
while read -r name version <&3; do
  pubtime=$(index_pubtime "$name" "$version") || pubtime=""
  if [[ -z $pubtime ]]; then
    pubtime=$(api_pubtime "$name" "$version") || pubtime=""
  fi
  [[ -n $pubtime ]] || die "date de publication introuvable : $name $version (index et API)"
  published=$(date -u -d "$pubtime" +%s) || die "date de publication illisible : $name $version ($pubtime)"
  age=$(((now - published) / 86400))
  eligible=$(date -u -d "@$((published + days * 86400))" +%F)
  echo "$name $version $pubtime âge=${age}j éligible=$eligible"
  if ((age < days)); then
    violations=$((violations + 1))
    message="$name $version publié il y a $age j (minimum $days j), éligible le $eligible"
    if [[ ${SECURITY_FIX:-} == true ]]; then
      say warning "$message ; exception security-fix : justification exigée dans la description de la PR"
    else
      say error "$message"
    fi
  fi
done 3<<<"$new"

if ((violations == 0)); then
  echo "check-min-age : tous les paquets ont au moins $days j"
elif [[ ${SECURITY_FIX:-} == true ]]; then
  echo "check-min-age : $violations paquet(s) trop récent(s), acceptés par l'exception security-fix"
else
  echo "check-min-age : $violations paquet(s) trop récent(s)"
  exit 1
fi
