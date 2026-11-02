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
pilot-window attaches to the in-process protocol server over loopback CDP. It shows Page.startScreencast frames and forwards human input through Input.*. It is not a full daily-driver browser UI.
Why: this needs no changes to Moli's core; multiple CDP clients are already supported upstream, and upstream treats "no GUI" as a deliberate boundary.
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
