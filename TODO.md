# pilot TODO

Work list for the pilot fork (see DECISIONS.md and FORK.md). One item per line; items are never reworded, only their checkbox changes.

## M0: Fork, build and baseline
- [x] Fork lexmount/moli to WSAdam/moli, clone here with an upstream remote, GitHub Actions disabled on the fork
- [x] Release build of moli succeeds on macOS arm64 (cargo build --release -p moli)
- [ ] Pinned baseline of workspace nextest and moli-cdp-smoke results recorded in docs/baseline.md
- [x] DECISIONS.md, FORK.md and docs/playwright-contract.md committed
- [x] Walking skeleton: Playwright MCP --cdp-endpoint drives `moli serve --layout` on a static page — navigate, snapshot, click (needs live run)
- [ ] Baseline benchmark in bench/: CDP-ready time, RSS for 1 and 25 contexts, goto-to-first-MCP-snapshot time, compared with chrome-headless-shell

## M1: Launchable by Playwright (pilot binary crate)
- [ ] pilot runs without a subcommand and ignores unknown Chromium switches (pilot/src/argv.rs)
- [ ] pilot maps --user-data-dir to the Moli profile dir and writes a SingletonLock symlink there
- [ ] pilot accepts --headless, --remote-debugging-port and --remote-debugging-address
- [ ] pilot opens about:blank as the start page, or no page with --no-startup-window
- [ ] pilot always runs with real layout on (Moli --layout)
- [ ] CDP over --remote-debugging-pipe: fd 3 in, fd 4 out, NUL-delimited JSON, exit on fd 3 EOF (pilot-core patch)
- [ ] Over the pipe, the start page is created at launch and auto-attached with waitingForDebugger
- [ ] Target.attachedToTarget arrives before the createTarget reply, and the main frame id equals the targetId
- [ ] Port mode prints "DevTools listening on ws://…" to stderr and writes DevToolsActivePort into the profile dir
- [ ] Browser.getVersion userAgent contains HeadlessChrome when pilot runs headless
- [ ] Playwright chromium.launch({executablePath: pilot}) runs newPage, goto, evaluate, close and the pilot process exits (moli-cdp-smoke pilot_launch group)
- [ ] Playwright launchPersistentContext(userDataDir) works with pilot (moli-cdp-smoke pilot_launch group)
- [ ] Playwright MCP drives pilot in default, --isolated and --cdp-endpoint modes: navigate, snapshot, click, type, screenshot (pilot_mcp group)

## M2: Live geometry
- [ ] A node inserted after the first geometry read gets a real box, so Playwright check/hover/selectOption succeed (#1080 #1289 #2069 fixtures, pilot-core live-layout patch)
- [ ] Geometry query timing benchmark in bench/ guards against #1617-style slowdowns

## M3: Watch-the-agent window (pilot-window crate)
- [ ] pilot opens a watch-the-agent window whenever --headless is absent
- [ ] Window tab strip tracks page targets from Target events
- [ ] Window URL bar, back, forward and reload drive the active tab
- [ ] Window shows live screencast frames of the active tab, paused while the window is hidden
- [ ] Human mouse, wheel and keyboard input in the window reaches the page at the right CSS-pixel coordinates
- [ ] Window shows an indicator while the agent is sending commands
- [ ] Headful window verified by hand while Playwright MCP drives pilot (needs live run)

## M4: MCP protocol gaps and Playwright suite pass rates
- [ ] Emulation.setDeviceMetricsOverride with mobile:true works for MCP --device (#1291)
- [ ] JavaScript dialog events reach every attached CDP session, not only the opener (#2063)
- [ ] Full-page screenshots taller than 65,535 px succeed via tiling
- [ ] Runtime.executionContextCreated auxData carries type default/isolated
- [ ] Fetch interception edge cases needed for MCP --allowed-origins pass (#280)
- [ ] Playwright tests/mcp pass rate against pilot recorded in compat/
- [ ] Playwright chromium-page test pass rate against pilot (CRPATH) recorded in compat/
- [ ] Golden CDP transcripts: each MCP tool's traffic on pilot diffed against real Chromium

## M5: Fast & lean
- [ ] V8 startup snapshot cuts pilot CDP-ready time, measured before and after in bench/
- [ ] Screencast is throttled or paused when no window is watching
- [ ] bench/ thresholds gate CDP-ready time, RSS per context, MCP snapshot latency and 25 parallel contexts

## M6: v1 compatibility suite and the open-web ladder
- [ ] compat/v1-suite.md defines the v1 compatibility suite
- [ ] TodoMVC React, Vue, Angular and Svelte flows pass through Playwright MCP tools on pilot
- [ ] The user's own sprig/Fresh apps pass through Playwright MCP tools on pilot
- [ ] Open-web corpus (~100 sites incl. Gmail, Docs, YouTube) pass rate tracked per release in compat/report.json
- [ ] Linux headless: M1 and M2 tests pass in a Linux container (needs live run)

## Bugs found
- [ ] Button appended after page load gets no ref in the Playwright MCP snapshot and cannot be clicked (frozen layout; walking-skeleton fixture)
- [ ] A second Playwright MCP --cdp-endpoint session against the same moli process gets f1-prefixed refs, as if the page were a child frame
- [ ] On macOS a file picked through the file chooser reports size 8 instead of 15 (moli-cdp-smoke dom-input group fails 3/3)
- [ ] On macOS moli::fetch_cli cli_connection_failure_reports_curl_code_and_cause fails: connecting to a bound non-listening port times out (curl 28) instead of being refused (curl 7)

## M0 additions
- [x] Scoped Rust baseline (nextest for moli and moli-protocol-server) and the moli-cdp-smoke result recorded in docs/baseline.md
- [x] Moli-only baseline benchmark (startup, warm actions, RSS for 1 and 25 contexts) in bench/results/2026-11-02-moli-baseline.json
- [ ] Chromium comparison for the smoke suite and benchmarks, run with --use-mock-keychain --password-store=basic so macOS never shows a Keychain prompt
