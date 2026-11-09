#!/bin/bash
# What's running: pilot's GitHub build gate (latest run, or the run id given) and any
# cargo/rustc on this Mac. Live view of a run: gh run watch <run-id> -R WSAdam/pilot
# Usage: scripts/gate-status.sh [run-id]
set -euo pipefail
REPO=WSAdam/pilot

run_id="${1:-$(gh run list -R "$REPO" --workflow pilot-gate.yml --limit 1 --json databaseId -q '.[0].databaseId')}"
if [ -z "$run_id" ]; then
  echo "🔍 no pilot gate run yet"
else
  gh run view "$run_id" -R "$REPO" --json databaseId,status,conclusion,headBranch,headSha,createdAt,url,jobs -q '
    def mins(from; to): (((to // now) - (from | sub("\\.[0-9]+"; "") | fromdateiso8601)) / 60 | floor);
    def stamp: if . == null then null else (sub("\\.[0-9]+"; "") | fromdateiso8601) end;
    def icon: if .status != "completed" then "▶" elif .conclusion == "success" then "✅" elif .conclusion == "skipped" then "·" else "❌" end;
    "🔍 pilot gate run \(.databaseId) on \(.headBranch) (\(.headSha[0:9])): \(if .status == "completed" then .conclusion else .status end), started \(mins(.createdAt; null)) min ago",
    "   \(.url)",
    (.jobs[] |
      "\(icon) \(.name): \(if .status == "completed" then .conclusion else .status end)",
      (if .conclusion == "failure" then "   📄 log: gh api repos/WSAdam/pilot/actions/jobs/\(.databaseId)/logs" else empty end),
      (.steps[] | select(.name | test("^(Set up job|Complete job|Post |Checkout)") | not) |
        if .conclusion == "skipped" then
          "   · \(.name) (skipped)"
        elif .status == "completed" then
          "   \(icon) \(.name) (\(mins(.startedAt; .completedAt | stamp)) min)"
        elif .status == "in_progress" then
          "   ▶ \(.name) (running \(mins(.startedAt; null)) min)"
        else
          "   · \(.name) (waiting)"
        end))'
fi

local_builds=$(ps -axo pid=,rss=,comm= | awk '$3 ~ /(^|\/)(cargo|rustc|clippy-driver|cargo-nextest)$/ {printf "   %s %d MB %s\n", $1, $2 / 1024, $3}')
if [ -n "$local_builds" ]; then
  echo "⚠️ compiling on this Mac:"; echo "$local_builds"
else
  echo "✅ nothing compiling on this Mac"
fi
