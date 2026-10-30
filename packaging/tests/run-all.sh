#!/usr/bin/env bash
# Lance les tests de packaging qui ne demandent aucun réseau (build-mcpb.yml, job `checks`).
# En ligne : INSTALL_TEST_ONLINE=1 et VERIFY_TEST_ONLINE=1 activent les essais contre les vrais
# registres ; DIST_PLAN_FILE=<plan.json> ajoute un plan réel de dist (`dist plan --output-format=json`).
#
# Usage : packaging/tests/run-all.sh
set -euo pipefail

here="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
for test in test-launcher test-smoke test-handshake test-stage test-release-scripts test-install-mcp-publisher test-verify-image; do
  bash "$here/$test.sh"
done
echo "run-all : ok"
