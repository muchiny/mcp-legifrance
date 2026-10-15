# Pipeline local de mcp-legifrance. Preuve de complétion : sortie de `make ci`.
#
# WSL (~/.claude/rules/wsl-safety.md) : parallélisme cargo par défaut ; cargo-mutants
# toujours `-j 1`, un fichier à la fois, scratch dans /var/tmp (/tmp est un tmpfs en RAM).
# La mutation de tout le workspace d'un coup tourne en CI (weekly.yml), jamais ici.

COV_FLAGS := --workspace --lib --summary-only \
	--fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100
MUTANTS_SAFE := TMPDIR=/var/tmp NEXTEST_TEST_THREADS=2 cargo mutants -j 1
BASE ?= origin/main

.PHONY: fmt fmt-check lint test doc arch deny machete cov mem-check mutants mutants-diff ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo nextest run --workspace
	cargo test --workspace --doc

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

arch:
	cargo xtask check-arch

deny:
	cargo deny check

machete:
	cargo machete

# Couverture des tests unitaires (--lib) : 100 % lignes, régions et fonctions.
cov:
	cargo llvm-cov $(COV_FLAGS)

# Contrôle mémoire obligatoire avant cargo-mutants : au moins 8 Gio disponibles.
mem-check:
	@free -m | awk 'NR==2 { if ($$7 >= 8*1024) print "OK: -j 1 safe"; else { print "BLOCK: not enough RAM, wait"; exit 1 } }'

# Un fichier : make mutants FILE=crates/lf-domain/src/id.rs
mutants: mem-check
	@test -n "$(FILE)" || { echo "usage: make mutants FILE=<path>"; exit 2; }
	$(MUTANTS_SAFE) --file $(FILE)

# Chaque fichier .rs modifié depuis $(BASE), l'un après l'autre.
mutants-diff: mem-check
	@set -e; files=$$(git diff --name-only $(BASE)...HEAD -- '*.rs'); \
	for f in $$files; do \
		if [ -f "$$f" ]; then $(MAKE) --no-print-directory mem-check; $(MUTANTS_SAFE) --file "$$f"; fi; \
	done

ci: fmt-check lint test doc arch deny machete cov
