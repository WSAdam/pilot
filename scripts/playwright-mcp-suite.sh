#!/bin/bash
# Runs Playwright's own tests/mcp suite against pilot, one worker, and writes the
# JSON report. Every test drives pilot: PLAYWRIGHT_MCP_EXECUTABLE_PATH reaches both
# the MCP server and the CLI daemon, and compat/playwright-mcp-suite/pilot.config.ts
# leaves mcpArgs unset so no test's flags leak into the next.
#
# Usage: scripts/playwright-mcp-suite.sh <checkout-dir> <report.json> [playwright test args...]
#   <checkout-dir>  where microsoft/playwright is (or will be) checked out
#   extra args      e.g. a spec file or --grep, to run part of the suite
# Env: PILOT_BIN (default target/release/pilot), PILOT_WORKERS (default 1),
#      PLAYWRIGHT_COMMIT (default the one compat/playwright-mcp-suite.json records).
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
checkout="${1:?usage: $0 <checkout-dir> <report.json> [playwright test args...]}"
report="${2:?usage: $0 <checkout-dir> <report.json> [playwright test args...]}"
shift 2
commit="${PLAYWRIGHT_COMMIT:-018adb570c757898e7035f23e29ebb689d1d6ead}"
pilot="${PILOT_BIN:-$repo/target/release/pilot}"

if [ ! -d "$checkout/.git" ]; then
  echo "🚀 fetching microsoft/playwright at ${commit:0:9} into $checkout"
  mkdir -p "$checkout"
  git -C "$checkout" init -q
  git -C "$checkout" remote add origin https://github.com/microsoft/playwright.git
  git -C "$checkout" fetch -q --depth 1 origin "$commit"
  git -C "$checkout" checkout -q FETCH_HEAD
fi
if [ ! -d "$checkout/node_modules" ]; then
  echo "🚀 npm ci (no browser download)"
  (cd "$checkout" && PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm ci --no-audit --no-fund)
fi
echo "🚀 building Playwright's packages"
(cd "$checkout" && npm run build > /dev/null)
cp "$repo/compat/playwright-mcp-suite/pilot.config.ts" "$checkout/tests/mcp/pilot.config.ts"

echo "🔍 running tests/mcp against $pilot"
cd "$checkout"
PLAYWRIGHT_MCP_EXECUTABLE_PATH="$pilot" PILOT_NO_WINDOW=1 PILOT_RESULTS="$report" \
  PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 PILOT_WORKERS="${PILOT_WORKERS:-1}" \
  npx playwright test --config tests/mcp/pilot.config.ts "$@" && status=0 || status=$?
echo "✅ report at $report (playwright exit $status)"
exit "$status"
