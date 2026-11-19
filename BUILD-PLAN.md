# pilot build plan

Every remaining piece of work, grouped by the build it needs. Written 2026-11-09.

**Since 2026-11-09 every build runs on GitHub, in `.github/workflows/pilot-gate.yml`. Your Mac compiles nothing.** The reason is in DECISIONS.md under "pilot compiles on GitHub, not on the Mac".

## Ledger

| Build | Kind | Status | Units | Peak RSS | Wall time | target/ after | Commits |
|---|---|---|---|---|---|---|---|
| 0 | none | done; open-web re-measure moved to Build 1 (72 MB free on 2026-11-09) | host-OS identity, TODO, this file | — | — | 5.2 GB (release only) | daa26d280, 06f7f448c, this commit |
| 1 | heavy | done 2026-11-10 on GitHub, gate run 42733332378 (the fourth on `gate/build1`). Earlier runs caught a missing Linux library, a Log/Console regression from the fix itself, and inline and handler errors arriving without the error object. Open-web re-measure deferred: 186 MB free on the Mac. | pilot gate workflow; console and uncaught errors; MCP tools re-measure (39/39) | Linux: clippy 8.6 GB, nextest 11.5 GB, release 6.9 GB. Mac release 3.5 GB. Your Mac: 0. | 1 h 58 min. Linux: clippy 11 min, nextest 35 + 2 min, release 15 min, smoke 1 min. Mac: release 48 min, conformance 25/25 | release binaries only (downloaded) | 5a841c923 … 2ed48764c, this commit |
| triage | none | done 2026-11-10. The 2026-11-03 suite run was flawed: about 256 CLI tests drove installed Chrome, and each test's flags leaked into the next. Re-measured with every test on pilot: 569 of 830 pass. Of the 108 failures, 68 are harness and 40 come from 12 pilot gaps, each with a red `todo` conformance test (`pilot/conformance/suite-gaps.test.mjs`). 20 of the 48 old candidates now pass. | Playwright MCP suite re-measured; 12 gaps sorted into Builds 2–4 | test runner 374 MB plus one pilot; your Mac compiled nothing | full suite 13.8 min, one worker | release binaries only | this commit |
| 2 | light | done 2026-11-10 on GitHub, gate run 42832542717 (`gate/build2`, first run green) | protocol-side suite gaps: cookies by domain, postDataEntries, WebP screenshots, proxy flags, pipe plus port, Moli-path millisecond timestamps; MCP tools re-measure (39/39) | Your Mac: 0 | 1 h 49 min. Linux: clippy 9 min, nextest 28 + 2 min, release 12 min, smoke 1 min. Mac: release 47 min, conformance | release binaries only (downloaded) | … fe160c54a |
| 3 | heavy | done 2026-11-13, landed with Builds 4–7 in gate run 43442789601 (`gate/build4-7`). Five red runs on `gate/build3` (2026-11-10) shaped it; its ResizeObserver unit, the nested-frame commit order and beforeunload moved to Build 3b, whose 3 conformance tests stay `todo` | popover, listbox options, file:// navigation, iframe frame ids, Live reflow on viewport resize | see Build 4–7 | see Build 4–7 | see Build 4–7 | d0f9db259 … 9ab44944f |
| 4 | heavy | done 2026-11-13 in run 43442789601. Its first run, 43434788241 (`gate/build4`), failed one upstream WPT case: getTools() through another window's modelContext named the caller's window. Fixed in 3523847a9 | WebMCP tools seen from Playwright's utility world, and from other windows | see Build 4–7 | see Build 4–7 | see Build 4–7 | 0f6c30fef, 3523847a9 |
| 5 | heavy | done 2026-11-13 in run 43442789601 (re-scoped: frame grid; code cache and Live re-layout deferred with measurements) | animation frames on a 60 Hz grid; also the form-POST redirect announced as a GET | see Build 4–7 | see Build 4–7 | see Build 4–7 | 804d134af, 12c0d81cd (redirect) |
| 6 | heavy | done 2026-11-13 in run 43442789601 | banded full-page PNG screenshots | see Build 4–7 | see Build 4–7 | see Build 4–7 | fd7e3f183 |
| 7 | light | done 2026-11-13 in run 43442789601: the gate's macOS job runs pilot-mcp's 45 tests against the pilot it built (45/45) | record→replay: pilot-mcp analyzer, replay engine, recorder, store, MCP server and CLI | see Build 4–7 | see Build 4–7 | see Build 4–7 | f2e61912c … 94ae913a3 |
| 4–7 | run | gate run 43442789601, 27 commits on `gate/build4-7`; main fast-forwarded to 94ae913a3 | Builds 3–7 together, one run instead of four, after GitHub paused Actions on the fork for its usage (2026-11-10 to 2026-11-13) | Linux: clippy 8.6 GB, nextest 10.9 GB, release 6.9 GB. Mac release 3.5 GB. Your Mac: 0 | 2 h 21 min. Linux: clippy 14 min, nextest 49 + 3 min (22,095 + 216 tests), release 20 min, smoke 2 min. Mac: release 43 min, conformance 43 pass and 3 Build 3b todo, pilot-mcp 45/45. MCP tools re-measure on the Mac binary: 39/39 | release binaries only (downloaded) | this commit |
| 3b | heavy | done 2026-11-17, gate run 44124162484 (the second on `gate/build3b`); main fast-forwarded to e6217329d. The first run, 44111028067, failed two upstream Playwright-over-CDP tests whose helper expected a child frame's new context before its commit; Chrome sends the commit first, so the helper now waits for the context after it | a viewport resize reaches ResizeObservers; a child frame's commit is sent before its own child frames attach (nested iframes list all 3 frames); beforeunload and its dialog on Playwright-started navigations | Linux: clippy 8.6 GB, nextest 11.4 GB, release 6.9 GB. Mac release 3.4 GB. Your Mac: 0 | 1 h 53 min. Linux: clippy 11 min, nextest 38 + 3 min (22,103 + 216 tests; 2 upstream fetch-deadline tests flaky, passed on retry), release 16 min, smoke 2 min. Mac: release 38 min, conformance 46 pass and 0 todo, pilot-mcp 45/45 | release binaries only (downloaded) | 91887b4c7 … e6217329d |
| 8 | heavy | done 2026-11-18, gate run 44296600166 (the third on `gate/build8`); main fast-forwarded to 602b9f6d1. Run 1 (44155204410) failed the emulation layout-metrics test, which now expects Chrome's page scale of 1, and a worker test that overflowed its stack. Run 2 (44196996026) failed the same worker test. Five diagnosis runs on throwaway `gate/diag-b8-*` branches found the cause: in a debug build, tokio's spawn_local passed a large page-task future by value through about 12 frames. Runs 44164466610 (bisect), 44180352795 (gdb), 44209002268 (core dumps), 44216554117 (gdb on the workspace build), and 44220737570 (the new test overflows without the fix and passes with it). The future is now boxed before it is spawned | file:// subresources, listbox option offset metrics, 980px layout without a viewport meta tag on phones, beforeunload for page-started navigations, inserted-script page errors; also legacy BeforeUnloadEvent return values, Chrome's page scale in getLayoutMetrics (whole-viewport screenshots at DPR 2 or 3), and the boxed owner-task future | Linux: clippy 8.6 GB, nextest 10.2 GB, release 6.9 GB. Mac release 3.5 GB. Your Mac: 0 | 1 h 50 min. Linux: clippy 10 min, nextest 30 + 2.5 min (22,135 + 216 tests, no flakes), release 14 min, smoke 1 min. Mac: release 48 min, conformance 55 pass and 0 todo, pilot-mcp 45/45 | release binaries only (downloaded) | 7c7742d17 … 602b9f6d1 |
| 9 | heavy | done 2026-11-18, gate run 44357519409 (the second on `gate/build9`); main fast-forwarded to d713d45f4. 6 conformance tests red on the Build 8 binary (`build9.test.mjs`), 6 pilot-core patches; the code cache parked on `park/code-cache`. Run 1 (44329657407): Linux green; macOS first got no runner (GitHub capacity), then on the re-run passed all 8 Build 9 tests but failed Build 5's first-frame mean (13.4 ms against 13 on a loaded runner). Chromium 1223 serves an idle page's frame at once, so that gap was fixed in this build too (2 more conformance tests) | history.back() returns before its beforeunload dialog, createEvent("BeforeUnloadEvent"), screencast pageScaleFactor on phones, dialog events to every CDP session, a second session's navigation aborted behind a dismissed dialog or winning behind an accepted one; an idle page's first animation frame runs at once. Probe on the Mac binary: first-frame delay median 6.8 ms (Build 8: 12.6), Playwright's two-frame click wait median 21.8 ms (Build 8: 27.6; Chrome 9-11 at 120 Hz, a new TODO line) | Linux: clippy 8.6 GB, nextest 10.9 GB, release 6.9 GB. Mac release 3.5 GB. Your Mac: 0 | Run 2: 1 h 52 min. Linux: clippy 9 min, nextest 28.5 + 2.5 min (22,150 + 216 tests, no flakes), release 13 min, smoke 1 min (54/54). Mac: release 43 min, conformance 65 pass and 0 todo, pilot-mcp 45/45. Run 1 Linux: 1 h 4 min | release binaries only (downloaded) | e2e8674c0 … d713d45f4 |
| 10 | heavy | done 2026-11-19, gate run 44420480817 (the fourth on `gate/build10`); main fast-forwarded to ac13d69e4. 2 conformance files (`build10-errors.test.mjs`, `build10-runtime.test.mjs`), red on the Build 9 binary apart from one guard; 4 pilot-core patches, the fourth added after run 3 (an event's objects registered for its session). Runs 1–3 red, see the Build 10 section. The Deno helper (lines 91 and 92) landed on main earlier, with no build needed | page errors reach DevTools as the error object, the main world's context name is "", a Runtime command is parsed once in the renderer, an event's object stays usable when another session holds its id. Probe on the Mac binary: Playwright's 346 KB evaluate round trip median 6.2 ms (Build 9: 8.0; Chrome 4.7–5.4) | Linux: clippy 8.6 GB, nextest 11.1 GB, release 6.9 GB. Mac release 3.5 GB. Your Mac: 0 | Run 4: 1 h 49 min. Linux: clippy 10 min, nextest 35.5 + 2.5 min (22,157 + 216 tests, no flakes), release 15.5 min, smoke 1.5 min (54/54). Mac: release 39 min, conformance 68 pass and 0 todo, pilot-mcp 45/45. Runs 1–3: 39 min, 1 h 1 min and 1 h 42 min | release binaries only (downloaded) | 367d8ba9e … ac13d69e4 |
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

### Triage pass: none (done 2026-11-10)
Went through the remaining 45 suite candidates against the current binary:
- sort each into a pilot gap (light or heavy) or a Chrome-only or test-harness difference;
- harness differences get a note in `compat/` and need no build;
- every gap gets a conformance test, proven red.

The batches below are the split triage confirmed. Each gap's conformance test is in `pilot/conformance/suite-gaps.test.mjs`, marked `todo` with its build until that build removes the mark. Rerun the suite with `scripts/playwright-mcp-suite.sh`.

### Build 2: light batch (protocol and pilot crates only; confirmed by triage)
| Item | Where | Suite tests |
|---|---|---|
| A cookie set by domain alone for a site the page never visited is dropped: `Storage.setCookies` uses the page URL instead of building one from the cookie's domain | `moli-protocol` `storage/normalize.rs` | 2 |
| Request bodies are missing: `Network.requestWillBeSent` carries `postData` but not `postDataEntries`, which is all current Playwright reads | `moli-protocol` network events (3 builders) | 6 |
| WebP screenshots: re-encode the renderer's PNG with the `image` crate's WebP encoder, which is already in the build, so the renderer doesn't change | `moli-protocol` `page/capture.rs` | 2 |
| `--proxy-server` and `--proxy-bypass-list` are dropped as unknown switches; map them to moli's `--http-proxy` and `--http-no-proxy`. Proxy credentials from the URL also fail in Fetch interception; recheck once the switch is mapped. | `pilot/src/argv.rs` | 3 |
| A launch with both `--remote-debugging-pipe` and `--remote-debugging-port` serves only the pipe and never prints `DevTools listening on`, so Playwright's launch hangs | `pilot/src/argv.rs`, `pilot/src/serve.rs` | 1 |
| Protocol TODOs: about:blank start page, `HeadlessChrome` user agent, execution-context `auxData`, refs for a second MCP session | — | — |
| Moli's own `Runtime.exceptionThrown` and `Runtime.consoleAPICalled` carry milliseconds since the epoch, not monotonic seconds (`moli-protocol` `observable_output` `emit_background_events`). After Build 1 this path carries only internal dispatch failures. | `moli-protocol` | TODO bug |

Triage moved `file://` and the navigate-back dialog to Build 3: both need renderer work. It dropped the rest of the old list: those tests pass now, or they fail only in the harness.

### Build 3: heavy batch, renderer gaps (confirmed by triage)
| Item | Where | Suite tests |
|---|---|---|
| `hidePopover()` on a closed popover throws `InvalidStateError` (the old spec; Chrome returns quietly). Playwright's highlight overlay calls it on a fresh element, so the recorder never installs. Upstream's test `show_and_hide_popover_throw_on_redundant_state_changes` pins the old spec; it changes with the fix, because pilot's requirement is Chrome's behaviour. | `moli-renderer-v8` `native_bridge/element/popover.rs` | 5 (recorder) |
| An iframe nested in a `data:` URL iframe never becomes a frame, and the page snapshot fails with "Maximum call stack size exceeded" | renderer frames | 2 |
| Options of a `<select multiple>` listbox have 0×0 boxes, so they get no refs | layout | 1 |
| A ResizeObserver on `document.body` doesn't fire on a viewport resize (`innerWidth` does change) | renderer | 1 |
| `beforeunload` never runs for navigations Playwright starts (goto, back, forward), so a cancelling page opens no dialog | renderer navigation | 3 |
| Every `file://` navigation is refused and no local-file loader exists. Chrome loads local files; MCP gates them itself. | `moli-url-policy` callers, renderer navigation loader | 1 |
| Phone emulation honours `<meta name=viewport>` | | small item |

Triage dropped `browser_mouse_click_xy`, `--init-script` with `--isolated`, and the download link: they pass now.

### Build 4: heavy, WebMCP
- 13 suite tests (7 MCP, 6 CLI): page tools listed as MCP tools, calling them, iframes, switching tabs.
- Cause found in triage: pilot already has `document.modelContext` and the WebMCP CDP domain. But Playwright calls `getTools()` from its utility world, and pilot keeps one tool list per JS world, where Chrome keeps one per document.
- `moli-webmcp` sits below the renderer.
- It's feature work, so it gets its own build to keep Build 3 small.

### Build 5: heavy, speed (two units, one build)
- Playwright's injected script compiled once per process, not once per page. Target: the first snapshot on a new page within 25 ms of a repeat snapshot.
- Cheaper clicks under Live layout. Target: bench click median at most 47 ms.
- The bench itself runs on the release binary and needs no build.

**Ready 2026-11-12, re-scoped from measurements** (`bench/results/2026-11-12-build5-before.json`; the Mac had about 150 MB free, so medians run high):
- **Where a click goes.** The bench click (53 ms) is 37 ms in Playwright's stability check, 10 ms in mouse press and release, and 6 ms finding the button by role.
  - The stability check waits for two animation frames. pilot timed each frame `now + 16 ms`, so that wait was always 36.5 ms. Chrome's frames follow vsync, which averages about 25 ms.
  - **Unit: animation frames follow a 60 Hz grid** (`fce80872f`). Expected click: about 42 ms, under the target. It's red against the 2026-11-10 binary in `pilot/conformance/frames.test.mjs`.
- **Deferred, measured:**
  - **The Live re-layout after a focus or hover change**, about 8 ms per click. Focus and hover bump the DOM version, so the whole layout rebuilds even when no style depends on them. Skipping that safely needs the style engine to report layout-affecting damage. The TODO line stays open.
  - **The injected-script cache**, at most about 10 ms per new page. The script's `Runtime.evaluate` costs 9–10 ms. The rest of the first-snapshot gap (34 ms) is the page's first layout (about 20 ms) and first-run JavaScript (about 4 ms), which a code cache doesn't touch. The TODO line stays open.

### Build 6: light or heavy (triage decides)
- Full-page screenshots taller than 65,535 px, by tiling.
- The flaky window test needs no build: loop the conformance test only.

**Ready 2026-11-12, heavy** (`moli-paint` and `moli-image` sit below the renderer). Unit `e1e9825c5`:
- Full-page PNGs that don't fit one raster surface are drawn in bands of up to 8,192 rows, streamed into a row encoder.
- The red tests found two more limits besides 65,535 px. At 1280 px wide, the PNG encoder's 128 MB RGBA budget failed pages from about 26,000 px tall, and the raster's 256 MB budget failed them from about 52,000 px.
- Red in `pilot/conformance/screenshot-tall.test.mjs`: three cases, at 400 × 70,040, 1280 × 30,040 and 1280 × 55,040.
- The flaky-window loop opens real windows, so it waits until you're away from the Mac.

### Build 7: light expected, record→replay fast path
- The first run of a task drives pilot and records the requests each action fires. Later runs replay them through pilot's network stack, and fall back to the browser when a replay fails.
- It's light as long as `moli-fetch` and `moli-curl` are used as they are. Any change to them makes it heavy.
- Design first, then size the build.

### Build 8: heavy, the leftovers (prep 2026-11-17)
Five gaps left open after Builds 1–7 and 3b, each with a conformance test in `pilot/conformance/leftovers.test.mjs`, all red against the 3b binary:
| Item | Where | TODO |
|---|---|---|
| A `file://` page's relative stylesheet, script and image load (Chrome loads them; pilot loaded only the document) | network transport check, file loader from Build 3 | line 146 |
| A listbox `<option>` has a principal box, so offsetWidth/Height/Top and ResizeObserver see its row | layout | line 147 |
| Under phone emulation a page without `<meta name=viewport>` lays out at 980px; `width=device-width` at the device width | emulation, layout viewport | line 94 |
| beforeunload and its dialog for navigations the page starts (location.href, history.back()) | renderer navigation, 3b's beforeunload machinery | line 174 |
| A throwing `<script>` inserted from page.evaluate reports a page error | uncaught-error reporting from Build 1 | line 130 |

Left open: the beforeunload timeout for a hung page (line 176) and the cross-session dialog case (line 177).

**Status:** done 2026-11-18 (ledger row 8). All five items landed, plus three found along the way: legacy BeforeUnloadEvent return values (line 178), Chrome's page scale in getLayoutMetrics (line 180), and the boxed owner-task future behind the worker-test overflow (lines 183–184). Still open from this build:
- history.back() opens its dialog inside the call; Chrome returns to the script first (line 179).
- Screencast pageScaleFactor under phone emulation (line 181).
- document.createEvent("BeforeUnloadEvent") (line 182).

**After Build 8:** the two deferred speed items from Build 5 (the Live re-layout after focus or hover, and the injected-script code cache).

### Build 9: heavy, Build 8's open lines and the code cache (prep 2026-11-18)
**Measured first, on the Build 8 binary** (`bench/results/2026-11-18-build8-after.json`, 2 contexts because the Mac had about 180 MB free):
- Bench click median: 45.1 ms. That's under Build 5's 47 ms target and at the 45 ms Moli baseline, so Build 5's frame grid already met the goal of the hover/focus re-layout skip (TODO line 86). Line 86 stays open and isn't built: the most it could save is about 8 ms, and it needs the style engine to report layout damage.
- First snapshot on a new document: 54.5 ms, against 15.8 ms for a repeat snapshot, a 38.7 ms gap.
  - About 22 ms of the gap is the document's first layout.
  - The injected script and its first run take 17 ms; a code cache can remove about 10 ms of that.

**Checked against Chromium headless shell 1223** (PILOT_BIN pointed at it):
- Line 176 needs no change. Under Playwright's `--disable-hang-monitor`, Chrome waits out a busy beforeunload handler too, and pilot matches it.
- Line 177's premise was wrong. Chrome also queues a second session's navigation behind an open beforeunload dialog, then aborts it when the dialog is dismissed; the corrected line is at the end of TODO.md.

Each item has a conformance test in `pilot/conformance/build9.test.mjs`, green on Chrome and red on the Build 8 binary (the desktop screencast test is a guard):
| Item | Where | TODO |
|---|---|---|
| history.back() returns before the beforeunload dialog opens (the traversal is queued, as in Chrome) | renderer navigation traversal | line 179 |
| document.createEvent("BeforeUnloadEvent") returns a BeforeUnloadEvent | renderer events | line 182 |
| Page.screencastFrame metadata carries the page scale on a phone | moli-protocol screencast output | line 181 |
| Page.javascriptDialogOpening reaches every session with Page enabled | moli-protocol dialogs | line 43 |
| A second session's navigation queued behind a dismissed beforeunload dialog is aborted (ERR_ABORTED), and the page stays | moli-protocol page navigation | the corrected line at the end of TODO.md |
| Playwright's injected script compiled once per process (a V8 code cache), so the first snapshot on a new document drops about 10 ms | renderer and Runtime.evaluate | line 103 |

**Code cache: parked, not in this build.**
- Each page has its own V8 isolate, so V8's own compilation cache starts empty on every new page.
- Playwright JSON-encodes its options into the injected source, including `frameSeq`, which is reallocated on every new-document navigation of the main frame, and `isUtilityWorld`. So an MCP tab sends a different source after every navigation, and no cache can hit.
- The agent's working patch warms the isolate cache from a process-wide code cache: it compiles the same source as an unbound script under the Inspector's origin before the Inspector dispatches. It saves an estimated 3–10 ms per new page, from the third page on, which only helps test suites.
- It is parked on `park/code-cache` (384428fca) rather than adding a pre-compile step in front of every large Runtime.evaluate.
- **The agent's lead for MCP:** each 346 KB evaluate is parsed into JSON about five times in the renderer, an estimated 1–3 ms on every navigation. It's a new TODO line.

### Build 10: heavy, page error objects, the main world's name, one parse per evaluate (prep 2026-11-18)
The scope is the cc-decide default: TODO lines 131, 188, 189, 91 and 92.

**Lines 91 and 92 (Deno) needed no build.**
- Line 91 doesn't reproduce: with Deno 2.9.8, 20 of 20 `chromium.launch` calls of the Build 9 binary succeeded.
- Line 92 is `launchPilot()` in `pilot/deno/mod.ts`: it starts pilot on a free port and returns a connected Playwright browser. It's on main as 6db71df64, with 7 Deno tests green against Build 9.

Each Rust item has a conformance test, green on Chromium headless shell 1223 and red on the Build 9 binary (the 346 KB evaluate test is a guard, green on both):
| Item | Where | TODO |
|---|---|---|
| An error thrown by page code reaches Runtime.exceptionThrown as the error object, so Playwright's pageerror gets the real Error. V8 15's Inspector sends the object only when the script that created the error was compiled shared cross-origin, so page scripts are now compiled that way (`build10-errors.test.mjs`, 8 cases) | renderer script origins: util.rs, native_module.rs, mutation_commands.rs, eval_exec.rs | line 131 |
| The main world's Runtime.executionContextCreated has an empty name, in the top frame and child frames (`build10-runtime.test.mjs`) | renderer inspector | line 188 |
| The renderer reads a Runtime command's id, method and steering params once, skipping the script text, instead of parsing the whole command about five times. On Build 9 a 346 KB evaluate takes 7.9–9.0 ms against Chrome's 4.7–5.4 ms; the estimated saving is 2–2.5 ms on every navigation of an MCP tab, to be re-measured on the Build 10 binary | renderer inspector session (`command_head.rs`, new) | line 189 |

**A changed requirement, said out loud:** 7 protocol-test assertions that expected the document URL as the main world's name now expect "", which is Chrome's value.

**Gaps left as new TODO lines:**
- thrown values that aren't Errors;
- module script errors;
- muted errors from cross-origin scripts;
- exceptionDetails.text "Uncaught";
- no url for eval code;
- the utility world's context fields;
- an evaluate serialized again at the frontend router.

**Gate runs.**
- Run 1 (44380599045) and run 2 (44388492700), both Linux:
  - the source-boundary test wanted the page-error unit test to run its script through the shared entry point;
  - two more main-world-name assertions in the background-staging tests (the count is 9 now, not 7). Run 1's failure hid the second.
- Run 3 (44401266844): Linux green. macOS conformance failed one case of `build10-errors`: `Runtime.callFunctionOn` on the attribute handler's error object was refused with "Cannot find object with given id".
  - **The bug is older than Build 10.** V8 object ids are per Inspector session. Moli refuses an id the commanding session never registered when another session on the target did. Objects that reach a session in a Runtime event were never registered.
  - **How Build 10 exposed it.** Playwright's page session held the same wire id for its UtilityScript, so the test's CDP session lost its error object. Probes on the run 3 binary: Build 9 loses a `console.log` argument the same way.
  - **Fix:** a fourth pilot-core patch registers an event's objects for its session (`runtime/activity.rs`, TODO line 202). It has its own unit fixture.
- Run 4 (44420480817): green. Linux 22,157 + 216 tests, smoke 54/54; macOS conformance 68/68, pilot-mcp 45/45.
- **Re-measured on the Build 10 Mac binary** (`evaluate-split.mjs`, 40 rounds, medians): Playwright's 346 KB injected script takes 6.2 ms per round trip, against 8.0 ms on Build 9 and Chrome's 4.7–5.4 ms. The saving is 1.8 ms, a little under the 2–2.5 ms estimate. The rest of the gap is mostly the router's second serialization and moli-protocol's id rewrite, which Build 11 takes on (line 201).

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
