# Pipeline local de mcp-legifrance. Preuve de complétion : sortie de `make ci`.
#
# WSL (~/.claude/rules/wsl-safety.md) : parallélisme cargo par défaut ; cargo-mutants
# toujours `-j 1`, un fichier à la fois, scratch dans /var/tmp (/tmp est un tmpfs en RAM).
# La mutation de tout le workspace d'un coup tourne en CI (weekly.yml), jamais ici.
#
# `--locked` partout (06 §9.4) : une commande échoue si `Cargo.lock` n'est pas à jour,
# au lieu de le réécrire. `--all-features` sur lint, test et doc (sans effet tant
# qu'aucune feature n'existe ; 06 §7.1).

COV_FLAGS := --workspace --lib --locked --summary-only \
	--fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100
MUTANTS_SAFE := TMPDIR=/var/tmp NEXTEST_TEST_THREADS=2 cargo mutants -j 1
BASE ?= origin/main
# Contrôle mémoire (>= 8 Gio disponibles), expansé directement dans les recettes
# pour que `make -n` reste un vrai dry run (aucun $(MAKE) récursif).
MEM_CHECK := free -m | awk 'NR==2 { if ($$7 >= 8*1024) print "OK: -j 1 safe"; else { print "BLOCK: not enough RAM, wait"; exit 1 } }'
# Analyse des workflows : zizmor 1.30.1 et actionlint 1.7.12 (versions de workflow-lint.yml).
# En local, par exemple : make workflow-lint ZIZMOR="uvx --from zizmor==1.30.1 zizmor"
ZIZMOR ?= zizmor
ACTIONLINT ?= actionlint
# Règle d'âge des dépendances (00 §2, 06 §9.4) : base de comparaison de Cargo.lock et âge
# minimal en jours. SECURITY_FIX=true (libellé `security-fix` de la PR) vient de l'environnement.
MIN_AGE_BASE ?= origin/main
MIN_AGE_DAYS ?= 14
# Golden de parité (06 §3.2) : répertoire du golden reproduit et données du plan.
SURFACE_DIR ?= /var/tmp/mcp-legifrance-surface
DONNEES := docs/plan/annexes/donnees

.PHONY: fmt fmt-check lint test doc arch purity deny audit-fuzz min-age min-age-selftest machete cov cov-cobertura clippy-json mem-check mutants mutants-diff workflow-lint surface ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
	cargo nextest run --workspace --all-features --locked
	cargo test --workspace --doc --all-features --locked

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked

arch:
	cargo xtask check-arch

# Pureté de lf-application (02 §4.4) : compilé seul, ses features sont résolues pour son
# seul graphe (tokio time/macros/rt, chrono sans clock) ; une feature qu'un autre membre
# active ne masque plus un manque.
purity:
	cargo check -p lf-application --locked

# Deux graphes sous un seul deny.toml (M1-D16) : le workspace, puis fuzz/ (hors workspace, son propre
# Cargo.lock). Chaque run signalerait des avertissements propres à l'autre graphe : `-A` les masque
# (licence ou wrapper non rencontrés, exception NCSA non employée), sans configuration par graphe.
deny:
	cargo deny --locked check -A license-exception-not-encountered
	cargo deny --locked --manifest-path fuzz/Cargo.toml check -A license-not-encountered -A unused-wrapper

# Avis RustSec sur fuzz/Cargo.lock (M1-D16), hors `make ci` : réseau (base d'avis). Le binaire
# `cargo-audit audit` et non `cargo audit` : la sous-commande passe par rustup et téléchargerait la
# chaîne épinglée par rust-toolchain.toml. Hors ligne : ajouter `--no-fetch`.
audit-fuzz:
	cargo-audit audit --deny warnings --file fuzz/Cargo.lock

# Âge minimal des paquets du registre ajoutés ou montés dans Cargo.lock depuis $(MIN_AGE_BASE)
# (réseau : index sparse de crates.io). Étape `min-age` du job deny de la CI, sur PR seulement.
min-age:
	MIN_AGE_BASE="$(MIN_AGE_BASE)" MIN_AGE_DAYS="$(MIN_AGE_DAYS)" .github/scripts/check-min-age.sh

# Auto-test du contrôle d'âge, hors réseau (verrous et index factices) : shellcheck, puis les cas.
min-age-selftest:
	shellcheck .github/scripts/check-min-age.sh .github/scripts/tests/min-age/run.sh
	.github/scripts/tests/min-age/run.sh

machete:
	cargo machete

# Couverture des tests unitaires (--lib) : 100 % lignes, régions et fonctions.
cov:
	cargo llvm-cov $(COV_FLAGS)

# Rapport Cobertura de SonarQube Cloud (sonar-project.properties), relu depuis les profils que
# `make cov` vient d'écrire : sans recompilation, et sans seuil (le seuil est celui de `cov`).
cov-cobertura:
	cargo llvm-cov report --cobertura --output-path cobertura.xml

# Rapport Clippy JSON de SonarQube Cloud, hors `make ci`. Même périmètre que `lint`, mais sans
# `-D warnings` : le rapport liste tous les avertissements au lieu de s'arrêter au premier.
clippy-json:
	cargo clippy --workspace --all-targets --all-features --locked --message-format=json > clippy-report.json

# Contrôle mémoire obligatoire avant cargo-mutants : au moins 8 Gio disponibles.
mem-check:
	@$(MEM_CHECK)

# Un fichier : make mutants FILE=crates/lf-domain/src/id.rs
mutants: mem-check
	@test -n "$(FILE)" || { echo "usage: make mutants FILE=<path>"; exit 2; }
	$(MUTANTS_SAFE) --file "$(FILE)"

# Chaque fichier .rs modifié depuis $(BASE), l'un après l'autre.
mutants-diff: mem-check
	@set -e; files=$$(git diff --name-only "$(BASE)...HEAD" -- '*.rs'); \
	for f in $$files; do \
		if [ -f "$$f" ]; then $(MEM_CHECK) || exit 1; $(MUTANTS_SAFE) --file "$$f"; fi; \
	done

# Workflows GitHub (zizmor puis actionlint) : hors `make ci`, lancé par workflow-lint.yml
# quand `.github/` change (06 §7.1).
workflow-lint:
	$(ZIZMOR) .github/
	$(ACTIONLINT)

# Golden de parité : reproduit par son script versionné depuis les fixtures copiées dans le crate,
# comparé à l'octet au fichier du plan (et la copie du crate aussi), puis contrôlé par
# `check_golden.py`. Python 3.13 en `python3 -I` (00 §10).
surface:
	mkdir -p "$(SURFACE_DIR)"
	python3 -I $(DONNEES)/scripts/golden.py crates/lf-adapter-piste/tests/fixtures/reference "$(SURFACE_DIR)/parite-golden.json"
	cmp "$(SURFACE_DIR)/parite-golden.json" $(DONNEES)/parite-golden.json
	cmp crates/lf-adapter-piste/tests/fixtures/reference/parite-golden.json $(DONNEES)/parite-golden.json
	python3 -I $(DONNEES)/scripts/check_golden.py $(DONNEES)/parite-golden.json

ci: fmt-check lint test doc arch purity deny min-age-selftest machete cov surface
