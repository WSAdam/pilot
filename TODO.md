# pilot TODO

Work list for the pilot fork (see DECISIONS.md and FORK.md). One item per line; items are never reworded, only their checkbox changes.

## M0: Fork, build and baseline
- [x] Fork lexmount/moli to WSAdam/moli, clone here with an upstream remote, GitHub Actions disabled on the fork
- [x] Release build of moli succeeds on macOS arm64 (cargo build --release -p moli)
- [x] Pinned baseline of workspace nextest and moli-cdp-smoke results recorded in docs/baseline.md
- [x] DECISIONS.md, FORK.md and docs/playwright-contract.md committed
- [x] Walking skeleton: Playwright MCP --cdp-endpoint drives `moli serve --layout` on a static page — navigate, snapshot, click (needs live run)
- [ ] Baseline benchmark in bench/: CDP-ready time, RSS for 1 and 25 contexts, goto-to-first-MCP-snapshot time, compared with chrome-headless-shell

## M1: Launchable by Playwright (pilot binary crate)
- [x] pilot runs without a subcommand and ignores unknown Chromium switches (pilot/src/argv.rs)
- [x] pilot maps --user-data-dir to the Moli profile dir and writes a SingletonLock symlink there
- [x] pilot accepts --headless, --remote-debugging-port and --remote-debugging-address
- [ ] pilot opens about:blank as the start page, or no page with --no-startup-window
- [x] pilot always runs with real layout on (Moli --layout)
- [x] CDP over --remote-debugging-pipe: fd 3 in, fd 4 out, NUL-delimited JSON, exit on fd 3 EOF (pilot-core patch)
- [x] Over the pipe, the start page is created at launch and auto-attached with waitingForDebugger
- [x] Target.attachedToTarget arrives before the createTarget reply, and the main frame id equals the targetId
- [x] Port mode prints "DevTools listening on ws://…" to stderr and writes DevToolsActivePort into the profile dir
- [ ] Browser.getVersion userAgent contains HeadlessChrome when pilot runs headless
- [x] Playwright chromium.launch({executablePath: pilot}) runs newPage, goto, evaluate, close and the pilot process exits (moli-cdp-smoke pilot_launch group)
- [x] Playwright launchPersistentContext(userDataDir) works with pilot (moli-cdp-smoke pilot_launch group)
- [x] Playwright MCP drives pilot in default, --isolated and --cdp-endpoint modes: navigate, snapshot, click, type, screenshot (pilot_mcp group)

## M2: Live geometry
- [x] A node inserted after the first geometry read gets a real box, so Playwright check/hover/selectOption succeed (#1080 #1289 #2069 fixtures, pilot-core live-layout patch)
- [x] Geometry query timing benchmark in bench/ guards against #1617-style slowdowns

## M3: Watch-the-agent window (pilot-window crate)
- [x] pilot opens a watch-the-agent window whenever --headless is absent
- [x] Window tab strip tracks page targets from Target events
- [x] Window URL bar, back, forward and reload drive the active tab
- [x] Window shows live screencast frames of the active tab, paused while the window is hidden
- [x] Human mouse, wheel and keyboard input in the window reaches the page at the right CSS-pixel coordinates
- [x] Window shows an indicator while the agent is sending commands
- [ ] Headful window verified by hand while Playwright MCP drives pilot (needs live run)

## M4: MCP protocol gaps and Playwright suite pass rates
- [ ] Emulation.setDeviceMetricsOverride with mobile:true works for MCP --device (#1291)
- [ ] JavaScript dialog events reach every attached CDP session, not only the opener (#2063)
- [ ] Full-page screenshots taller than 65,535 px succeed via tiling
- [ ] Runtime.executionContextCreated auxData carries type default/isolated
- [ ] Fetch interception edge cases needed for MCP --allowed-origins pass (#280)
- [x] Playwright tests/mcp pass rate against pilot recorded in compat/
- [ ] Playwright chromium-page test pass rate against pilot (CRPATH) recorded in compat/
- [ ] Golden CDP transcripts: each MCP tool's traffic on pilot diffed against real Chromium

## M5: Fast & lean
- [ ] V8 startup snapshot cuts pilot CDP-ready time, measured before and after in bench/
- [x] Screencast is throttled or paused when no window is watching
- [x] bench/ thresholds gate CDP-ready time, RSS per context, MCP snapshot latency and 25 parallel contexts

## M6: v1 compatibility suite and the open-web ladder
- [x] compat/v1-suite.md defines the v1 compatibility suite
- [x] TodoMVC React, Vue, Angular and Svelte flows pass through Playwright MCP tools on pilot
- [ ] The user's own sprig/Fresh apps pass through Playwright MCP tools on pilot
- [ ] Open-web corpus (~100 sites incl. Gmail, Docs, YouTube) pass rate tracked per release in compat/report.json
- [ ] Linux headless: M1 and M2 tests pass in a Linux container (needs live run)

## Bugs found
- [x] Button appended after page load gets no ref in the Playwright MCP snapshot and cannot be clicked (frozen layout; walking-skeleton fixture)
- [ ] A second Playwright MCP --cdp-endpoint session against the same moli process gets f1-prefixed refs, as if the page were a child frame
- [x] A node appended or restyled after the first geometry read keeps its old (0x0) box under OnDemand layout, so Playwright cannot see or click it
- [ ] On macOS a file picked through the file chooser reports size 8 instead of 15 (moli-cdp-smoke dom-input group fails 3/3)
- [ ] On macOS moli::fetch_cli cli_connection_failure_reports_curl_code_and_cause fails: connecting to a bound non-listening port times out (curl 28) instead of being refused (curl 7)
- [x] Browser.close sent with a negative command id (Playwright's -9999) is rejected as an invalid request, so a browser Playwright launched never exits
- [x] Playwright launchPersistentContext with pilot hangs: Page.setLifecycleEventsEnabled replays load before the earlier Page.getFrameTree replies, so Playwright drops the event
- [ ] On macOS (US Eastern time) moli-protocol emulation test context_locale_override_applies_to_loaded_background_page_without_activation fails; it passes with TZ=UTC
- [ ] On macOS moli-renderer-v8 test font_face_unicode_range_limits_layout_and_updates_after_loading fails: the test font measures the same as the system monospace fallback

## M1 additions
- [x] Playwright launch/persistent and MCP tests live in pilot/conformance (node --test), not moli-cdp-smoke groups; see DECISIONS.md
- [ ] Claude Code drives pilot through Playwright MCP (claude mcp add with --executable-path), headless and headed (needs live run)
- [x] Decided: pilot keeps its normal Chrome user agent when headless (DECISIONS.md), so the HeadlessChrome item above stays intentionally undone
- [x] A second Playwright MCP on a profile pilot already holds reports "Browser is already in use" (SingletonLock, exit code 21)

## M0 additions
- [x] Scoped Rust baseline (nextest for moli and moli-protocol-server) and the moli-cdp-smoke result recorded in docs/baseline.md
- [x] Moli-only baseline benchmark (startup, warm actions, RSS for 1 and 25 contexts) in bench/results/2026-11-02-moli-baseline.json
- [ ] Chromium comparison for the smoke suite and benchmarks, run with --use-mock-keychain --password-store=basic so macOS never shows a Keychain prompt
- [x] Playwright cannot click, check or fill a form control inside an inline <label>: hit testing returns the label above the control, so "<label> intercepts pointer events" (port upstream PR #395)
- [x] Playwright cannot click an element below the fold: after a viewport scroll (script or DOM.scrollIntoViewIfNeeded), getBoundingClientRect and DOM.getContentQuads keep the pre-scroll position, so Playwright reports "element is outside of the viewport"
- [ ] Under Live layout a Playwright click re-lays out the page only when a hover or focus change affects some style, bringing the bench click median back near the 45 ms Moli baseline (M5)
- [x] A headed pilot with its window open never exits after Browser.close: the window controller was cancelled with the browser runtime before it could close the window
- [x] Set PILOT_NO_WINDOW=1 to run pilot without its window when --headless is absent (automated runs on a desktop); document it where users will find it
- [x] A mouse press on non-focusable content (text, a div) scrolls a long page to the bottom, because focus falls back to the body and the body is scrolled into view; Chrome does not scroll
- [x] Playwright MCP tool sweep (pilot/conformance npm run sweep) passes every tool in compat/mcp-tools.json, including browser_mouse_drag_xy
- [ ] chromium.launch({ executablePath: pilot }) from Deno (npm:playwright-core) hangs during launch; from Node it works, and connectOverCDP to pilot --remote-debugging-port works from Deno
- [ ] A Deno helper module starts pilot on a free port and returns a connected Playwright browser, so a Deno project drives pilot in one call
- [x] Playwright contexts with a phone device descriptor (MCP --device) open on pilot: Emulation.setDeviceMetricsOverride accepts mobile=true under Live, laying pages out at the device width
- [ ] Mobile emulation processes <meta name=viewport>, so a page without one lays out at 980px and scales down like Chrome
- [x] f1/f2-prefixed snapshot refs after a page navigates are Playwright 1.65's own numbering (it gives the main frame a new seq on each new-document navigation), not a pilot bug
- [ ] "chromium.launch drives a page with pilot's window open" failed once in a full conformance run on 2026-11-03 (message not kept); it passed 0-failures in 12 loops and 2 full runs afterwards; find the race if it recurs
- [x] Correction to the Deno line above: chromium.launch({ executablePath: pilot }) works from Deno (launch, newPage, goto, close); the "hang" was a probe waiting for an <h1> that example.com no longer has. pilot/deno/launch_test.ts (deno task test) guards it, so no Deno helper module is needed
- [x] pilot fetched no images or web fonts (Moli's default), so images laid out at 0x0 and links positioned against them landed off-screen (weather.gov header); pilot now passes --image --font
- [ ] Open-web corpus (pilot/conformance npm run open-web, compat/open-web.json): sites that block pilot as a bot (Stack Overflow, Etsy, Reuters, Amazon, IMDb, Craigslist) load like they do in Chrome
- [x] Pressing Enter in a text input does not fire change, so apps that commit on change (TodoMVC's vanilla ES6 app, many forms) ignore Enter; Chrome fires change, then submits implicitly
- [x] TodoMVC flows (pilot/conformance npm run todomvc, compat/todomvc.json) pass in every hosted framework example
- [x] Measured where pilot startup goes (2026-11-03, release, agent-page fixture): launch 51 ms, new context 18 ms, new page 6 ms, first AI snapshot on a page 70 ms against 17 ms for the next one; a V8 startup snapshot would save little, the per-page compile of Playwright's injected script is the cost
- [ ] Playwright's injected script is compiled once per process instead of once per page (a V8 code cache for large Runtime.evaluate sources, which today go through V8's inspector), so the first MCP snapshot on a new page costs about what a repeat snapshot does
- [x] Correction to the macOS file chooser line: the browser reports the right size; moli-cdp-smoke wrote upload.TXT over upload.txt on macOS's case-insensitive file system. The group now uses its own directory for those files and smoke is 54/54 on macOS
- [ ] Navigation requests match Chrome's header order and Accept-Encoding (gzip, deflate, br, zstd), with Connection: keep-alive on HTTP/1.1; today libcurl's order and "deflate, gzip, br" show (a lead for the bot-walled sites; TLS and HTTP/2 fingerprints are the bigger signal)
- [x] Playwright's own tests/mcp suite runs against pilot (PLAYWRIGHT_MCP_EXECUTABLE_PATH) with its pass rate in compat/; needs permission to fetch microsoft/playwright at 018adb570 and run its npm ci
- [x] Playwright's own tests/mcp suite ran against pilot (compat/playwright-mcp-suite.json): 553 passed, 124 failed, 153 skipped; 76 failures are harness causes (test-mcp server, browser-type lookup, bundled Chromium), 48 are candidates for pilot gaps
- [ ] The 48 candidate failures in compat/playwright-mcp-suite.json are triaged: each is a pilot gap (fixed with a conformance test) or a legitimate Chrome-only difference (noted)
- [x] A server that ends a close-delimited response (no Content-Length) by closing TCP without a TLS close_notify fails the navigation with curl error 56 (Craigslist's 302); Chrome treats the response as complete
- [x] pilot's TLS ClientHello is Chrome's (JA4 t13d1516h2_8daaf6152771_d8a2da3f94cd, GREASE, shuffled extensions, brotli certificate compression, ALPS, ECH GREASE); libcurl's own hello read as a script
- [x] pilot's HTTP/2 connection preface matches Chrome's (SETTINGS 1:65536;2:0;4:6291456;6:262144, WINDOW_UPDATE 15663105, pseudo-headers m,a,s,p), which needs a vendored libcurl
- [x] The window's agent badge shows idle grey or active blue even when no system UI font loads; it was drawn only with a font (pilot-window/src/view.rs)
- [x] The composed window (tab strip, toolbar, agent badge, letterboxed page) is checked pixel by pixel against a real in-process browser in pilot-window's tests, replacing the hand check of the headful window
- [x] Claude Code's Playwright MCP launch (stdio @playwright/mcp with --executable-path pilot, as claude mcp add in pilot/README.md sets it up) is covered headless, isolated and headed with the window by pilot/conformance/mcp.test.mjs, replacing the hand check
- [ ] HTTP/2 HEADERS frames carry Chrome's priority (exclusive, weight 256 for a navigation); nghttp2 1.68 ignores pri_spec, so this needs a patched or older nghttp2 (not part of the Akamai fingerprint)
- [ ] eBay (open-web corpus) answers 403 since the HTTP/2 preface became Chrome's; it loaded in both earlier runs. Likely the mismatch of Chrome's TLS and HTTP/2 fingerprints with libcurl's header order; re-check after Chrome's header order lands
- [x] Navigation requests go out in Chrome's header order, with User-Agent and Accept-Encoding (gzip, deflate, br) in moli-fetch's own list and priority: u=0, i on top-level navigations over HTTP/2 only (moli-fetch/src/blocking/chrome_order.rs)
- [ ] HTTP/1.1 requests send Connection: keep-alive right after Host, as Chrome does; libcurl writes Connection last, so this needs libcurl's request header writer reordered
- [ ] Subresource requests (scripts, styles, images, fetch) go out in Chrome's header order; today only navigations are sorted, because Chrome's subresource order has not been checked against a real Chrome
- [ ] eBay still answers 403 with Chrome's header order too (2026-11-04): Akamai refuses the first request from a fresh profile before any script runs (compat/bot-walls.md)
- [x] pilot's browser identity names the host's OS: on a Mac the User-Agent, Sec-CH-UA-Platform and navigator.platform say macOS, matching the TCP/IP fingerprint, instead of Moli's default Windows
- [ ] Runtime.consoleAPICalled carries a timestamp in milliseconds since the epoch, as Chrome does; pilot sends 0, so Playwright MCP's console log reads [-1793731413422ms] and drops the warning line (console.spec)
- [ ] Runtime.exceptionThrown carries its timestamp in milliseconds since the epoch; pilot sends seconds
- [ ] An uncaught error thrown by an inline script while the page loads reaches Playwright as a pageerror and Runtime.exceptionThrown (Playwright MCP console.spec "page error")
- [ ] An uncaught error thrown by an event handler (onclick, addEventListener) reaches Playwright as a pageerror and Runtime.exceptionThrown
- [ ] A page error's message is the error's own message (timer blank), not Moli's dispatch text (timer callback dispatch failed: callback `Window Web IDL callback` threw: <unknown>: Uncaught Error: timer blank)
- [ ] Record→replay fast path: the first run of a task drives pilot and records the requests each action fires; later runs replay them through pilot's network stack (cookies, Chrome TLS/HTTP/2), falling back to the browser when a replay fails (planned after the MCP gaps, 2026-11-04)
- [ ] pilot builds run on GitHub (.github/workflows/pilot-gate.yml): the Linux and macOS jobs pass on gate/build1 and the macOS binary downloads into target/release (needs live run)
