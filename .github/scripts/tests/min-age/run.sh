#!/usr/bin/env bash
# Auto-test de check-min-age.sh (`make min-age-selftest`, dans `make ci`).
#
# Verrous et index sparse factices, servis en file:// ; horloge figée par MIN_AGE_NOW ;
# environnement vidé (`env -i`) : ni réseau, ni configuration git de l'utilisateur.
# Chaque cas vérifie le code de sortie, puis des motifs présents (ou absents, « ! ») dans la
# sortie (stdout et stderr).
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/check-min-age.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
now=2026-10-16T12:00:00Z
dir=$work
output=""
code=0
cases=0
failures=0

# Verrou factice : un membre du workspace et une dépendance git, que le contrôle ignore, puis
# les paquets du registre crates.io donnés en couples « nom version ».
lock() { # fichier [nom version]…
  local file=$work/$1
  shift
  {
    printf 'version = 4\n\n[[package]]\nname = "lf-domain"\nversion = "0.0.0"\n'
    printf '\n[[package]]\nname = "from-git"\nversion = "0.1.0"\nsource = "git+https://example.invalid/from-git#0123abcd"\n'
    while (($# >= 2)); do
      printf '\n[[package]]\nname = "%s"\nversion = "%s"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "00"\n' "$1" "$2"
      shift 2
    done
  } >"$file"
}

# Version publiée dans l'index factice, au chemin écrit en clair (règle des préfixes).
publish() { # chemin nom version pubtime
  mkdir -p "$(dirname "$work/index/$1")"
  printf '{"name":"%s","vers":"%s","deps":[],"cksum":"00","features":{},"yanked":false,"pubtime":"%s"}\n' \
    "$2" "$3" "$4" >>"$work/index/$1"
}

# Réponse factice de l'API crates.io (`/crates/<nom>/<version>`).
api() { # nom version created_at
  mkdir -p "$work/api/crates/$1"
  printf '{"version":{"crate":"%s","num":"%s","created_at":"%s"}}\n' "$1" "$2" "$3" >"$work/api/crates/$1/$2"
}

# Lance le script depuis $dir dans un environnement vidé : réglages communs (MIN_AGE_DAYS garde
# son défaut, 14), puis ceux donnés (« VAR=valeur », le dernier l'emporte). Garde la sortie et le
# code de sortie.
run() {
  code=0
  output=$(cd "$dir" && env -i PATH="$PATH" MIN_AGE_NOW="$now" \
    MIN_AGE_INDEX_URL="file://$work/index" MIN_AGE_API_URL="file://$work/api" \
    "$@" "$script" 2>&1) || code=$?
}

# Compare le verrou de tête $2 au verrou de base $1 (fichiers de $work, sans git).
against() { # base tête [VAR=valeur]…
  run MIN_AGE_BASE_LOCKFILE="$work/$1" MIN_AGE_LOCKFILE="$work/$2" "${@:3}"
}

# Vérifie le dernier `run` : code de sortie, puis chaque motif.
expect() { # nom code [motif]…
  local name=$1 want=$2 pattern ok=true
  shift 2
  cases=$((cases + 1))
  [[ $code == "$want" ]] || ok=false
  for pattern in "$@"; do
    if [[ $pattern == '!'* ]]; then
      [[ $output != *"${pattern#!}"* ]] || ok=false
    else
      [[ $output == *"$pattern"* ]] || ok=false
    fi
  done
  if [[ $ok == true ]]; then
    echo "ok     $name (code $code)"
  else
    echo "ÉCHEC  $name : code $code (attendu $want), sortie :"
    echo "       ${output//$'\n'/$'\n       '}"
    failures=$((failures + 1))
  fi
}

publish 1/a a 1.0.0 2021-03-03T10:48:58Z
publish 2/ab ab 1.0.0 2023-12-31T23:59:59Z
publish 3/a/abc abc 1.0.0 2025-06-01T00:00:00Z
publish 3/a/abc abc 1.3.0 hier
publish in/fl/inflector Inflector 0.11.4 2019-02-12T14:05:51Z
publish ol/d-/old-crate old-crate 0.1.0 2019-05-05T00:00:00Z
publish se/rd/serde serde 1.0.200 2025-04-27T00:00:00Z
publish se/rd/serde serde 1.0.228 2026-10-02T12:00:00Z
publish se/rd/serde_json serde_json 1.0.154 2026-10-02T12:00:01Z
publish th/is/thiserror thiserror 2.0.99 2026-10-13T08:00:00Z
# ab 2.0.0 est dans l'index sans `pubtime` ; abc 1.1.0 n'y est pas ; xyzw n'a pas de fichier.
printf '{"name":"ab","vers":"2.0.0","deps":[],"cksum":"00","features":{},"yanked":false}\n' >>"$work/index/2/ab"
api ab 2.0.0 2025-11-20T10:00:00Z
api abc 1.1.0 2026-01-15T09:30:00.123456Z
api xyzw 0.1.0 2024-07-01T06:30:00Z
mkdir -p "$work/api/crates/abc"
printf '{"version":{"crate":"abc","num":"1.4.0"}}\n' >"$work/api/crates/abc/1.4.0"

printf 'version = 4\n' >"$work/vide.lock"
lock base.lock a 1.0.0 ab 1.0.0 old-crate 0.1.0 serde 1.0.200
lock anciens.lock a 1.0.0 ab 1.0.0 abc 1.0.0 Inflector 0.11.4
lock recent.lock a 1.0.0 ab 1.0.0 old-crate 0.1.0 serde 1.0.200 thiserror 2.0.99
lock limite.lock a 1.0.0 ab 1.0.0 old-crate 0.1.0 serde 1.0.200 serde_json 1.0.154
lock montee.lock a 1.0.0 ab 1.0.0 old-crate 0.1.0 serde 1.0.228
lock retire.lock a 1.0.0 ab 1.0.0 serde 1.0.200
lock api.lock ab 2.0.0 abc 1.1.0 xyzw 0.1.0
lock introuvable.lock abc 1.2.0
lock illisible.lock abc 1.3.0
lock sans-date.lock abc 1.4.0

against base.lock base.lock
expect "rien de nouveau" 0 "aucun paquet ajouté ou changé depuis $work/base.lock" "!âge="

against vide.lock anciens.lock
expect "paquets anciens ; préfixes de l'index (1, 2, 3, 4+ caractères, majuscules)" 0 \
  "check-min-age : 4 paquet(s) ajouté(s) ou changé(s) depuis $work/vide.lock, âge minimal 14 j" \
  "a 1.0.0 2021-03-03T10:48:58Z âge=2053j éligible=2021-03-17" \
  "ab 1.0.0 2023-12-31T23:59:59Z âge=1019j éligible=2024-01-14" \
  "abc 1.0.0 2025-06-01T00:00:00Z âge=502j éligible=2025-06-15" \
  "Inflector 0.11.4 2019-02-12T14:05:51Z âge=2802j éligible=2019-02-26" \
  "check-min-age : tous les paquets ont au moins 14 j" "!lf-domain" "!from-git"

against base.lock recent.lock
expect "paquet récent" 1 \
  "thiserror 2.0.99 2026-10-13T08:00:00Z âge=3j éligible=2026-10-27" \
  "erreur : thiserror 2.0.99 publié il y a 3 j (minimum 14 j), éligible le 2026-10-27" \
  "check-min-age : 1 paquet(s) trop récent(s)" "!serde"

against base.lock recent.lock MIN_AGE_DAYS=3
expect "MIN_AGE_DAYS=3 : le même paquet est éligible" 0 \
  "thiserror 2.0.99 2026-10-13T08:00:00Z âge=3j éligible=2026-10-16" \
  "check-min-age : tous les paquets ont au moins 3 j" "!erreur"

against base.lock limite.lock
expect "13 j 23 h 59 min 59 s : trop récent" 1 \
  "serde_json 1.0.154 2026-10-02T12:00:01Z âge=13j éligible=2026-10-16" \
  "erreur : serde_json 1.0.154"

against base.lock montee.lock
expect "montée de version comptée comme ajout ; 14 j pile : éligible" 0 \
  "serde 1.0.228 2026-10-02T12:00:00Z âge=14j éligible=2026-10-16" "!serde 1.0.200" "!erreur"

against base.lock recent.lock SECURITY_FIX=true
expect "SECURITY_FIX=true : avertissement" 0 \
  "avertissement : thiserror 2.0.99 publié il y a 3 j (minimum 14 j)" \
  "acceptés par l'exception security-fix" "!erreur"

against base.lock recent.lock GITHUB_ACTIONS=true
expect "annotation GitHub en CI" 1 "::error::thiserror 2.0.99 publié il y a 3 j (minimum 14 j)"

against base.lock retire.lock
expect "paquet retiré ignoré" 0 "aucun paquet ajouté ou changé" "!old-crate"

against vide.lock api.lock
expect "repli sur l'API crates.io (version absente, sans pubtime, crate absent de l'index)" 0 \
  "ab 2.0.0 2025-11-20T10:00:00Z âge=330j éligible=2025-12-04" \
  "abc 1.1.0 2026-01-15T09:30:00.123456Z âge=274j éligible=2026-01-29" \
  "xyzw 0.1.0 2024-07-01T06:30:00Z âge=837j éligible=2024-07-15"

against vide.lock introuvable.lock
expect "index sans la version et API absente" 2 "date de publication introuvable : abc 1.2.0"

against vide.lock sans-date.lock
expect "réponse de l'API sans created_at" 2 "date de publication introuvable : abc 1.4.0"

against vide.lock illisible.lock
expect "date de publication illisible" 2 "date de publication illisible : abc 1.3.0 (hier)"

against vide.lock absent.lock
expect "verrou de tête absent" 2 "verrou illisible : $work/absent.lock"

against absent.lock vide.lock
expect "verrou de base absent" 2 "verrou de base illisible : $work/absent.lock"

against vide.lock anciens.lock MIN_AGE_DAYS=deux
expect "MIN_AGE_DAYS invalide" 2 "MIN_AGE_DAYS doit être un nombre de jours (reçu : deux)"

against vide.lock anciens.lock MIN_AGE_NOW=demain
expect "MIN_AGE_NOW illisible" 2 "MIN_AGE_NOW illisible : demain"

# Base lue par git (sans MIN_AGE_BASE_LOCKFILE) : un dépôt jetable, isolé de la configuration
# de l'utilisateur, avec un commit sans verrou puis un commit qui ajoute le verrou de base,
# visé aussi par origin/main (défaut de MIN_AGE_BASE). Cargo.lock (défaut de MIN_AGE_LOCKFILE)
# est ensuite modifié sans commit.
repo=$work/repo
mkdir "$repo"
git_() {
  env -i PATH="$PATH" HOME="$work" GIT_CONFIG_NOSYSTEM=1 git -C "$repo" \
    -c user.name=selftest -c user.email=selftest@example.invalid "$@"
}
git_ init -q -b selftest
git_ commit -q --allow-empty -m "sans verrou"
sans=$(git_ rev-parse HEAD)
cp "$work/base.lock" "$repo/Cargo.lock"
git_ add Cargo.lock
git_ commit -q -m "verrou de base"
git_ update-ref refs/remotes/origin/main HEAD
cp "$work/montee.lock" "$repo/Cargo.lock"
dir=$repo

run
expect "base git (origin/main) : verrou de base lu par git show" 0 \
  "1 paquet(s) ajouté(s) ou changé(s) depuis origin/main" \
  "serde 1.0.228 2026-10-02T12:00:00Z âge=14j" "!old-crate"

run MIN_AGE_BASE="$sans"
expect "base git sans Cargo.lock : tout paquet compte comme nouveau" 0 \
  "a 1.0.0 2021-03-03T10:48:58Z" "ab 1.0.0 2023-12-31T23:59:59Z" \
  "old-crate 0.1.0 2019-05-05T00:00:00Z" "serde 1.0.228 2026-10-02T12:00:00Z"

run MIN_AGE_BASE=absente
expect "base introuvable" 2 "révision de base introuvable : absente"

echo "min-age-selftest : $cases cas, $failures échec(s)"
[[ $failures == 0 ]]
