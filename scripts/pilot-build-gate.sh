#!/bin/bash
# One build at a time: sequential, fail-fast, each step time-limited, peak rustc RSS sampled.
# Usage: scripts/pilot-build-gate.sh <label> <nextest -p list...>
# Env: SKIP_OPEN_WEB=1 to skip the open-web sweep.
set -u
LABEL="$1"; shift
NEXTEST_PACKAGES=("$@")
REPO=$(cd "$(dirname "$0")/.." && pwd)
LOGS=${BUILD_LOGS:-$REPO/target/build-logs}/$LABEL
mkdir -p "$LOGS"
SUMMARY="$LOGS/summary.txt"
: > "$SUMMARY"
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TZ=UTC

say() { echo "$*" | tee -a "$SUMMARY"; }

# run_step <name> <limit-seconds> <command...>
run_step() {
  local name="$1" limit="$2"; shift 2
  local log="$LOGS/$name.log" started peak=0 elapsed=0
  started=$(date +%s)
  say "🚀 $name (limit $((limit / 60)) min)"
  ( cd "$REPO" && exec "$@" ) > "$log" 2>&1 &
  local pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    sleep 10
    elapsed=$(( $(date +%s) - started ))
    local rss
    rss=$(ps -axo rss=,comm= | awk '/rustc|clippy-driver/ {if ($1 > m) m = $1} END {print m + 0}')
    [ "$rss" -gt "$peak" ] && peak=$rss
    if [ "$elapsed" -gt "$limit" ]; then
      pkill -TERM -P "$pid" 2>/dev/null; kill -TERM "$pid" 2>/dev/null
      pkill -f "$REPO/target/(release|debug)/(pilot|moli)" 2>/dev/null
      say "❌ $name killed after $((elapsed / 60)) min (limit $((limit / 60)) min); peak rustc $((peak / 1024)) MB"
      exit 1
    fi
  done
  wait "$pid"; local status=$?
  elapsed=$(( $(date +%s) - started ))
  if [ "$status" -ne 0 ]; then
    say "❌ $name failed (exit $status) after $((elapsed / 60)) min; peak rustc $((peak / 1024)) MB; log $log"
    tail -30 "$log" | tee -a "$SUMMARY"
    exit 1
  fi
  say "✅ $name in $((elapsed / 60)) min; peak rustc $((peak / 1024)) MB"
}

say "🔍 $LABEL started $(date '+%Y-%m-%d %H:%M'); $(top -l 1 | grep PhysMem); disk free $(df -h /System/Volumes/Data | awk 'NR==2 {print $4}')"
run_step fmt 600 cargo fmt --all
run_step clippy 7200 cargo clippy --workspace --all-targets --all-features -- -D warnings
NEXTEST_ARGS=()
for package in "${NEXTEST_PACKAGES[@]}"; do NEXTEST_ARGS+=(-p "$package"); done
# Known macOS-only failures (TODO.md): excluded by name so a real failure still stops the gate.
run_step nextest 10800 cargo nextest run --no-fail-fast "${NEXTEST_ARGS[@]}" \
  -E 'not (test(cli_connection_failure_reports_curl_code_and_cause) | test(font_face_unicode_range_limits_layout_and_updates_after_loading) | test(context_locale_override_applies_to_loaded_background_page_without_activation))'
# Release with 16 codegen units instead of Cargo.toml's 1: LLVM then optimizes the
# 1.35M-line renderer in pieces, one at a time at -j 1, instead of as one module.
run_step release 7200 env -u CARGO_INCREMENTAL -u CARGO_PROFILE_DEV_DEBUG -u CARGO_PROFILE_TEST_DEBUG CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release -p moli -p pilot
run_step conformance 1800 bash -c 'cd pilot/conformance && node --test --test-concurrency=1 --test-reporter=tap'
run_step smoke 2400 bash -c "cd moli-cdp-smoke && MOLI_BIN=$REPO/target/release/moli uv run moli-cdp-smoke --strict-full-suite --jobs 2 --output-dir $LOGS/smoke"
run_step mcp-sweep 1800 bash -c 'cd pilot/conformance && npm run sweep'
if [ "${SKIP_OPEN_WEB:-0}" != 1 ]; then
  run_step open-web 1800 bash -c 'cd pilot/conformance && npm run open-web'
fi
say "🔍 target/ $(du -sh "$REPO/target" | cut -f1); disk free $(df -h /System/Volumes/Data | awk 'NR==2 {print $4}')"
say "✅ $LABEL done $(date '+%H:%M')"
