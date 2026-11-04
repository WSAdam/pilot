# Decisions

Deliberate choices for the pilot fork. Each is on purpose; ask before changing one.

## Fork Moli instead of writing an engine from scratch
pilot is a fork of lexmount/moli (Rust, raw V8, own DOM, Stylo, Taffy, Parley, vello_cpu). Moli already speaks CDP; pilot adds what Playwright MCP needs on top.
Why: Moli already covers most of a V8 + Servo-crate engine. Writing our own would take months just to reach where Moli is.
Date: 2026-11-02

## "Optimized for Playwright MCP" means fast and lean
Priorities are startup time, memory per context, and many parallel contexts. Native accessibility-tree work and agent-specific CDP extras are out of scope for now.
Why: chosen by the user. MCP snapshots are computed by Playwright's injected JS, so a native accessibility tree would not help them anyway.
Date: 2026-11-02

## v1 is a fixed modern-SPA suite; the open web is a ladder
v1 is done when compat/v1-suite.md passes: TodoMVC flows, the user's own apps, and Playwright pass-rate thresholds. Heavy sites (Gmail, Docs, YouTube) are tracked as a pass rate per release, not a v1 gate.
Why: an engine that isn't Chromium can't promise the whole open web at v1.
Date: 2026-11-02

## Headful mode is a watch-the-agent window, built as an extra CDP client
pilot-window is a second browser-level CDP client of the in-process protocol server, connected over channels (`LocalCdpConnector`), not a socket. It shows Page.startScreencast frames and forwards human input through Input.*. It is not a full daily-driver browser UI.
Why: multiple CDP clients are already supported upstream, and upstream treats "no GUI" as a deliberate boundary. The first version of this entry said loopback CDP with no core changes, but MCP's default launch uses the pipe and has no listener, so either transport needs a small core hook; channels open no port and copy nothing through a socket. Changed 2026-11-03 after a Shepherd question (default taken).
Date: 2026-11-02

## pilot always runs with real layout
The pilot binary always turns on Moli's `--layout`. It never uses the mock geometry mode.
Why: Playwright's actionability checks and coordinate input need real geometry; in mock mode Input.dispatchMouseEvent fails.
Date: 2026-11-02

## Patches stay in the fork; no pull requests to lexmount/moli
Core fixes (pipe transport, live layout, protocol gaps) are carried in this fork only.
Why: chosen by the user.
Date: 2026-11-02

## Fork code lives in pilot-* crates; core patches are few and listed
New behaviour goes in fork-owned crates (`pilot`, `pilot-window`) that use Moli's library API. Each change to an upstream file is a single `pilot-core:` commit, listed in FORK.md with the test that guards it.
Why: upstream lands about 1,300 commits a month, so a small, listed set of patches keeps upstream merges manageable.
Date: 2026-11-02

## Follow upstream by merging release tags, never by rebasing
Upstream updates come from `git merge <tag>` onto our main. The merge lands only when the full suite is green.
Why: our main is published on GitHub, and a release tag is a more stable point to merge from than upstream main.
Date: 2026-11-02

## Fork base is upstream main at 51d6d1ee3, not the v1.1.20 tag
The fork started from upstream main as of 2026-11-02 (51d6d1ee3, 113 commits past v1.1.20). The first upstream merge will be the next release tag after that.
Why: resetting the fork's main to v1.1.20 required a force-push, which was declined.
Date: 2026-11-02

## pilot keeps its normal Chrome user agent when headless
pilot does not report `HeadlessChrome` in `Browser.getVersion` or anywhere else, even with `--headless`.
Why: many sites block the HeadlessChrome user agent, which matters for an agent's browser. Playwright only uses it to decide whether to send `Page.setFontFamilies`. Filed as a Shepherd question with this as the default.
Date: 2026-11-02

## Playwright and MCP launch tests are a Node package (pilot/conformance), not moli-cdp-smoke groups
`pilot/conformance` runs `node --test` with pinned playwright-core 1.65 and @playwright/mcp 0.0.84. It launches pilot by executable path in every Playwright and MCP launch mode and drives it end to end.
Why: Playwright MCP is a Node program that spawns the browser itself. moli-cdp-smoke is built around a managed `moli serve` endpoint, strict full-suite rules and a Chromium oracle for every group, and none of that fits "launch this executable as Chromium".
Date: 2026-11-02

## pilot runs Moli's layout under a new `Live` policy; upstream's `OnDemand` stays frozen
Under `LayoutPolicy::Live`, a geometry read or input event after any DOM change (including hover and focus state) rebuilds the page layout, as Chromium does. `moli serve --layout` still uses `OnDemand`, which keeps upstream's frozen snapshot until a screenshot or paint.
Why: upstream deliberately freezes geometry until the next paint, and 21 upstream tests assert it. Upstream's own fix (PR #2084) changes `OnDemand` and fails those tests. An opt-in policy gives Playwright live geometry while leaving every upstream test and file it guards unchanged, so upstream merges stay clean. Cost: a click can re-lay out once per mouse event, measured in bench/ and owned by M5.
Date: 2026-11-02

## pilot emulates phones at the device width, without viewport-meta layout
Under pilot's Live layout, `Emulation.setDeviceMetricsOverride` with `mobile=true` lays the page out at the requested device width and pixel ratio. `<meta name=viewport>` is not processed yet, so a page without one is laid out at the device width instead of Chrome's 980px-and-scale.
Why: Playwright MCP's `--device` and Playwright's phone descriptors failed outright, since Moli rejects mobile emulation until it has viewport-meta layout. Responsive pages (`width=device-width`, nearly all modern sites) come out the same as in Chrome. Asked through Shepherd; the default was taken.
Date: 2026-11-03

## pilot fetches images and web fonts, but not audio, video or text tracks
pilot always passes Moli `--image --font`. Moli's own default fetches no optional resources.
Why: images without dimensions lay out at 0x0 until they load, and web fonts change text metrics, so without them elements sit where they would not in Chrome and Playwright clicks miss (weather.gov's header link landed off-screen). MCP screenshots also need images. Media is left off to keep pilot lean. Asked through Shepherd; the default was taken.
Date: 2026-11-03

## pilot's TLS ClientHello is Chrome's
Every TLS connection pilot makes offers Chrome's cipher suites, groups, signature algorithms and extensions, with GREASE and per-connection extension shuffling, so its JA4 fingerprint is Chrome's. It also accepts a TCP close without a TLS close_notify at the end of a close-delimited response, as Chrome does.
Why: sites behind bot defenses fingerprint the ClientHello, and libcurl's own hello identifies pilot as a script. The user chose bot walls as the next focus on 2026-11-03. HTTP/2 SETTINGS and header order are the next layers; they need libcurl changes.
Date: 2026-11-03

## pilot's HTTP/2 connection preface is Chrome's
pilot's libcurl opens HTTP/2 connections with Chrome's SETTINGS (values and order), Chrome's connection window and stream windows, and Chrome's pseudo-header order, so its Akamai HTTP/2 fingerprint is Chrome's. curl-sys is vendored in vendor/curl-sys to carry the patch to libcurl's http2.c and http.c.
Why: bot defenses fingerprint the HTTP/2 preface alongside the TLS ClientHello; libcurl's own (MAX_CONCURRENT_STREAMS 100, a 1 GB window, :scheme before :authority) marks pilot as curl. Chrome's per-request HEADERS priority is not sent: nghttp2 1.68 no longer sends RFC 7540 priorities.
Date: 2026-11-03
