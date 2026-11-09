# pilot build plan

Every remaining piece of work, grouped by the build it needs. Written 2026-11-09.

**Since 2026-11-09 every build runs on GitHub, in `.github/workflows/pilot-gate.yml`. Your Mac compiles nothing.** The reason is in DECISIONS.md under "pilot compiles on GitHub, not on the Mac".

## Ledger

| Build | Kind | Status | Units | Peak RSS | Wall time | target/ after | Commits |
|---|---|---|---|---|---|---|---|
| 0 | none | done; open-web re-measure moved to Build 1 (72 MB free on 2026-11-09) | host-OS identity, TODO, this file | — | — | 5.2 GB (release only) | daa26d280, 06f7f448c, this commit |
| 1 | heavy | running on GitHub (`gate/build1`), the first gate run there. It was on hold locally: 5.1 GB free, about 7 GB needed. | pilot gate workflow; console and uncaught errors; open-web re-measure | | | | |
| triage | none | queued | 45 Playwright MCP suite candidates | — | | — | |
| 2 | light | queued | protocol-side suite gaps | | | | |
| 3 | heavy | queued | renderer-side suite gaps, meta viewport | | | | |
| 4 | heavy | queued | WebMCP | | | | |
| 5 | heavy | queued | injected-script cache, Live click cost | | | | |
| 6 | light or heavy | queued | tall screenshot tiling | | | | |
| 7 | light | queued | record→replay | | | | |
| opt | heavy | only on request | network-fingerprint leftovers | | | | |

**How a build moves through it:**
- **Status:** `queued → ready → running → done`, or `failed` with the step that failed.
- **ready** means every fix in the batch is written, and its conformance tests are red against `target/release/pilot`.
- **Run:**
  1. Run `cargo fmt --all` locally. It only reformats files and compiles nothing.
  2. Commit each unit on a `gate/<label>` branch, then push the branch. The push starts `pilot-gate.yml`.
  3. **Linux job** (16 GB runner): `cargo fmt --check`, clippy `-D warnings`, nextest on the whole workspace, the release build with 16 codegen units, and the CDP smoke.
  4. **macOS job** (macos-15): the same release build, then the conformance suite against it. It uploads `pilot` and `moli` as the `pilot-macos-aarch64` artifact.
  5. A background `gh run watch` reports back when both jobs finish. It uses no real memory.
- **Both green:**
  - the macOS binaries are downloaded into `target/release/`;
  - the MCP sweep, plus open-web when the batch needs it, runs locally against them (pilot uses about 200 MB);
  - main is fast-forwarded to the gate branch and pushed, and the gate branch is deleted;
  - TODO lines are ticked and this row is filled in with the run link and the job times.
- **Either red:** main doesn't move. The failing unit is fixed or pulled out, and the branch is pushed again.
- `scripts/pilot-build-gate.sh` stays as a local fallback for when GitHub is unavailable. It needs about 7 GB free.

## What a build costs

| Kind | When it's needed | Peak RAM | Time (`-j 1`) |
|---|---|---|---|
| **None** | Docs, `compat/` notes, conformance tests (JS), sweeps run against the existing release binary | pilot about 200 MB, plus node | minutes |
| **Light** | Changes only above the renderer: `moli-core`, `moli-protocol*`, `moli`, `pilot`, `pilot-window`. The renderer is reused, so only those crates recompile and relink. | not measured yet; well under a heavy build | not measured yet |
| **Heavy** | Any change in `moli-renderer-v8` or a crate below it: dom, layout, fetch, curl, vendored curl-sys, cookie jar, paint, image, webmcp, browser profile | **about 6–7 GB per rustc of the renderer**. A full gate compiles it about 4 times, one after another: clippy, dev lib, dev test target, release. | **about 50 min** for the full gate |

**On GitHub, light and heavy cost the same.** Each gate run compiles everything from scratch, and your Mac uses nothing either way. Batching still matters, because one gate run takes a couple of hours to report back. The peak-RAM figures above only apply to the local fallback.

**Local rules (the fallback, and the history behind the move)**
- One build at a time, `CARGO_BUILD_JOBS=1`, with no sweep, test run or other cargo command alongside it.
- Before a heavy build:
  - at least 7 GB free or reclaimable (`top -l 1 | grep PhysMem`);
  - your go, ideally while you're away from the Mac.
- Every cargo step runs under `/usr/bin/time -l`, so the real peak RSS gets recorded in this file.
- **Red proofs need no build.** A bug is proven red with a conformance test against the current release binary; the build comes only once the fix is written.
- **Batch heavy work.** Fixes for several units go into one heavy gate, then each unit gets its own commit. If one unit stays red, it's pulled out, which costs one more build.
- **Disk: one build on disk at a time.** Cargo never deletes stale artifacts. When a crate below the renderer changes identity (vendoring curl-sys, a Cargo.lock bump), every crate above it gets a new hash. The old test executables stay behind, at 300–550 MB each, because each carries the whole engine plus V8. That is how `target/debug` reached 29 GB; it was cleaned on 2026-11-09, leaving 55 GB free. Run `cargo clean --profile dev` after any Cargo.lock or `vendor/` change, and whenever `target/debug` passes about 30 GB. The first gate after a clean also rebuilds the roughly 100 small crates, which costs time, not RAM.

## The builds, in order

### Build 0: none (ready now)
- **Commit the host-OS identity unit.** pilot reports Chrome on macOS here instead of Windows. Its gate passed on 2026-11-04: nextest 59/59, conformance 23/23, MCP tools 39/39.
- **Re-run the open-web sweep.** It should take about 10–15 minutes; kill it if it passes 30. It was cut off at site 24 of 30.
- Push.

### Build 1: heavy, the console and uncaught-error fix
- Written, but not compiled. Its 2 conformance tests are red against the current binary.
- Fixes 5 TODO bugs:
  - console timestamps are 0;
  - exception timestamps are in seconds;
  - errors thrown by inline scripts and event handlers don't reach Playwright;
  - page errors carry Moli's wrapper message.
- Should clear 3 Playwright MCP suite candidates (`console.spec`).

### Triage pass: none
Go through the remaining 45 suite candidates against the current binary:
- sort each into a pilot gap (light or heavy) or a Chrome-only or test-harness difference;
- harness differences get a note in `compat/` and need no build;
- every gap gets a conformance test, proven red.

The batches below are my expected split. Triage confirms it.

### Build 2: light batch (expected to be protocol or pilot crates only)
| Item | Candidates |
|---|---|
| `browser_cookie_list` and `browser_cookie_delete` for cookies on other domains | 2 |
| `file://` navigation when `allowUnrestrictedFileAccess` is on | 1 |
| WebP screenshots; a viewport screenshot without a snapshot | 2 (heavy if the WebP encoder is missing from `moli-image`) |
| `browser_network_request` full request and response details | 2 |
| Request blocking for origins that aren't listed, plus Fetch interception edge cases (TODO) | 2 |
| A dialog during navigate-back, plus dialog events reaching every CDP session (TODO) | 1 |
| Launch: executable path, closing once when stdin ends, the missing-executable message | 3 |
| Webstorage capability | 1 |
| CLI isolated and persistent sessions, config, roots, secrets, tracing, file-upload roots | about 14, mostly expected to be harness and need no build |
| Protocol TODOs: about:blank start page, `HeadlessChrome` user agent, execution-context `auxData`, refs for a second MCP session | — |
| Moli's own `Runtime.exceptionThrown` and `Runtime.consoleAPICalled` carry milliseconds since the epoch, not monotonic seconds (`moli-protocol` `observable_output` `emit_background_events`). After Build 1 this path carries only internal dispatch failures. | TODO bug |

### Build 3: heavy batch, renderer gaps
| Item | Candidates |
|---|---|
| Options in a `<select multiple>` get refs | 1 |
| `browser_resize` changes `innerWidth` and fires ResizeObserver | 1 |
| `browser_mouse_click_xy` hits the right element | 1 |
| Stitched iframe snapshot (times out today) | 1 |
| `--init-script` with `--isolated` | 1 |
| A download link fires a download | 1 |
| Recorder (4 tests) | 4 |
| Phone emulation honours `<meta name=viewport>` | small item |

### Build 4: heavy, WebMCP
- 7 suite candidates: page tools listed as MCP tools, calling them, iframes, switching tabs.
- `moli-webmcp` sits below the renderer.
- It's feature work, so it gets its own build to keep Build 3 small.

### Build 5: heavy, speed (two units, one build)
- Playwright's injected script compiled once per process, not once per page. Target: the first snapshot on a new page within 25 ms of a repeat snapshot.
- Cheaper clicks under Live layout. Target: bench click median at most 47 ms.
- The bench itself runs on the release binary and needs no build.

### Build 6: light or heavy (triage decides)
- Full-page screenshots taller than 65,535 px, by tiling.
- The flaky window test needs no build: loop the conformance test only.

### Build 7: light expected, record→replay fast path
- The first run of a task drives pilot and records the requests each action fires. Later runs replay them through pilot's network stack, and fall back to the browser when a replay fails.
- It's light as long as `moli-fetch` and `moli-curl` are used as they are. Any change to them makes it heavy.
- Design first, then size the build.

### Optional, heavy: network-fingerprint leftovers (low payoff)
- The items:
  - HTTP/2 priority (needs an nghttp2 patch);
  - where `Connection: keep-alive` goes on HTTP/1.1;
  - Chrome's header order for subresources;
  - zstd.
- All four touch curl or fetch, below the renderer, so they share one heavy build if done at all.
- The remaining walls (eBay, Reuters, Stack Overflow) look like IP reputation or JavaScript challenges, which these changes don't address.

## Needs you, not a build
- **Chrome comparison** on the bot-wall sites from this IP. It runs Chromium with `--use-mock-keychain --password-store=basic`, so there's no Keychain prompt. It needs your OK.
- **Your sprig and Fresh apps** for the v1 suite: I need their paths.
- **Linux headless check:** needs Docker running. A build inside Docker is heavy and uses the Docker VM's memory.
- **Deno `chromium.launch` hang**, plus a Deno helper module. Light or no build.
- **4 macOS-only test failures:** test-side fixes, low priority.
- **V8 startup snapshot:** measured as low value in `docs/baseline.md`. I propose dropping it.
- **JavaScript fingerprint layer** for Cloudflare: weeks of work, not planned.

## Totals
- **Heavy:** 4 sessions (Builds 1, 3, 4, 5), plus Build 6 if triage says so and the optional network one. About 50 min each, with 6–7 GB peaks during the compile steps.
- **Light:** 2–3 (Builds 2 and 7, maybe 6).
- **None:** Build 0, the triage pass, the sweeps, the bench, and the flaky-test loop.
