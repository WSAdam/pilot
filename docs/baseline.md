# Baseline before any pilot changes

Recorded 2026-11-02 on macOS 26.2, Apple Silicon (arm64, 10 cores, 16 GB RAM), at fork base `51d6d1ee3` (upstream main, moli 1.1.20). Every pilot change is compared against this: a test that was red here is a known failure, and a test that was green here and goes red is a regression.

## Build

- `cargo build --release -p moli` takes **10m 37s** cold and produces `target/release/moli`, a 131 MB unstripped binary.
- `cmake` must be installed (`brew install cmake`). Rust 1.96.1 comes from `rust-toolchain`.
- **Disk:** a debug test build of six crates (`moli`, `moli-protocol`, `moli-protocol-server`, `moli-core`, `moli-browser-profile`, `moli-renderer-v8`) grew `target/debug` past **18 GB** before the 5 GiB watchdog stopped it. Nearly every test binary links V8. Run the Rust suites one crate group at a time with `CARGO_PROFILE_TEST_DEBUG=0`, and clear `target/debug` afterwards (`cargo clean --profile dev`).

## CDP smoke suite (`moli-cdp-smoke`, Playwright 1.58)

Command:

```sh
MOLI_BIN=$PWD/target/release/moli uv run --project moli-cdp-smoke moli-cdp-smoke --strict-full-suite --jobs 4 --output-dir <dir>
```

**Result: 53 of 54 groups pass** (36.9 s wall-clock).

| Group | Result | Notes |
|---|---|---|
| `dom-input` | **FAIL, 3 of 3 runs** | `file chooser selected file size: expected 15, got 8` (`groups/dom_input.py:152`). Uploading the same file directly with `set_input_files` passes. Probably specific to macOS, since upstream CI runs on Linux. Tracked in TODO.md under "Bugs found". |
| every other default group | pass | |

**Chromium oracle: not run.** The harness launches Chrome Headless Shell without `--use-mock-keychain`, which makes macOS show a Keychain prompt. The user asked to skip Chromium comparisons for now. Any later run must pass `--use-mock-keychain --password-store=basic`, using a wrapper given through `--chromium-bin`.

## Walking skeleton: Playwright MCP through `--cdp-endpoint`

Setup: `moli serve --layout --port 9333` driven by `@playwright/mcp@0.0.84 --cdp-endpoint http://127.0.0.1:9333`. The test page has a text box, a button that writes a greeting, and a button that appends a new button to the page.

| MCP tool | Result | Time |
|---|---|---|
| `browser_navigate` | pass | 235 ms |
| `browser_snapshot` | pass: roles, names and refs are correct | 8–24 ms |
| `browser_type` (fill) | pass | 186 ms |
| `browser_click` on an element from the initial load | pass; the page updated | 590 ms |
| `browser_click` on the button that appends a button | pass | 662 ms |
| clicking the **appended** button | **impossible**: the snapshot lists it without a `ref` | |
| `browser_take_screenshot` | pass | 57–72 ms |

- **Late-added elements:** the appended button gets no ref because Moli's layout is frozen after the first geometry read. The new node has a 0×0 box, so Playwright considers it not visible. This is the M2 fix.
- **Reconnecting:** a second MCP session against the same `moli` process saw refs prefixed `f1`, as if the page were a child frame.

Both are tracked in TODO.md.

## Rust test suites

Command (scoped to the crates M1 changes, debug info off to save disk):

```sh
CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 cargo nextest run --no-fail-fast -p moli -p moli-protocol-server
```

**Result: 1,059 of 1,060 tests pass**, with 2 skipped. The build and test run took 6m28s; the tests alone took 38.4 s. `target/` reached 16 GB.

| Test | Result | Notes |
|---|---|---|
| `moli::fetch_cli cli_connection_failure_reports_curl_code_and_cause` | **FAIL** | The test expects connecting to a bound but non-listening port to be refused at once (curl `[7]`). On macOS the connection timed out instead (curl `[28]` after 7.8 s). The test assumes Linux socket behaviour. |

The rest of the workspace (`moli-core`, `moli-renderer-v8`, and others) gets a baseline before the milestone that changes it, once there is enough disk space.

## Benchmark (`bench/`, Moli only)

Command: `node bench/bench.mjs --bin target/release/moli --runs 10 --contexts 25 --label moli-baseline`. Full results are in [bench/results/2026-11-02-moli-baseline.json](../bench/results/2026-11-02-moli-baseline.json).

The script connects over CDP to `moli serve --layout` and uses Playwright 1.65's `page.ariaSnapshot({mode:"ai"})`, the call behind MCP's `browser_snapshot`. The fixture is `bench/fixtures/agent-page.html`: a form plus 50 cards built by script.

| Metric | Median |
|---|---|
| Spawn until `/json/version/` answers | **27.3 ms** |
| `connectOverCDP` | 7.4 ms |
| Spawn until the first AI snapshot is done (new context, page, goto, snapshot) | **130.9 ms** |
| `Browser.close` until the process exits | 4.9 ms |
| Warm `page.goto` | 5.9 ms |
| Warm AI snapshot | 50.1 ms |
| `fill` | 4.6 ms |
| `click` | 45.1 ms |
| RSS when idle | 41.6 MiB |
| RSS with 1 context and page | 86.5 MiB |
| RSS with 25 contexts and pages | 360.4 MiB (**about 11.4 MiB per extra context**) |

There's no Chromium comparison yet, because Chromium oracle runs are on hold (see above).
