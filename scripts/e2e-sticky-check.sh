#!/usr/bin/env bash
# Objective gate for WI #83: selected project is sticky across navigation.
# Requires a korg-api serving the freshly-built web bundle.
# Override the target with KORG_E2E_URL (defaults to the isolated dev server).
set -euo pipefail
cd "$(dirname "$0")/../web"
export KORG_E2E_URL="${KORG_E2E_URL:-http://127.0.0.1:8091}"
exec npx playwright test work-item-project-sticky --project=chromium
