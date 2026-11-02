# The Playwright contract pilot implements

This is what Playwright and Playwright MCP expect from a browser they drive as Chromium. It was taken from the source of playwright-core 1.65 and @playwright/mcp 0.0.84 on 2026-11-02. All paths below are under `microsoft/playwright/packages/playwright-core/src/`:

- `server/chromium/{chromium,chromiumSwitches,crBrowser,crPage,crConnection,crExecutionContext,crInput,crNetworkManager}.ts`
- `server/{pipeTransport,browserType,dom,frames,screenshotter}.ts`
- `tools/mcp/{browserFactory,config}.ts` and `tools/backend/tab.ts`

## How Playwright MCP gets a browser

| MCP mode | Playwright call | What pilot must support |
|---|---|---|
| default | `launchPersistentContext(userDataDir)`, then `contexts()[0]` | Launch over a pipe, a persistent profile, a startup `about:blank` |
| `--isolated` | `launch()` then `newContext()` | Launch over a pipe, `Target.createBrowserContext` |
| `--cdp-endpoint <url>` | `chromium.connectOverCDP(url, {noDefaults:true})` | `GET /json/version/` and a WebSocket CDP endpoint |

- `--executable-path` (or `PLAYWRIGHT_MCP_EXECUTABLE_PATH`) points MCP at the pilot binary.
- MCP launches **headed** by default. It adds `--headless` itself on Linux when there is no `DISPLAY`.
- The headless viewport is 1280×720. When headed, the viewport is `null` (no emulation).
- The default profile is `~/Library/Caches/ms-playwright-mcp/mcp-<channel>-<hash>`. MCP checks its `SingletonLock` symlink to decide whether the profile is in use.

## Launch

**argv** is these, in order:
1. About 45 fixed Chromium switches (`--disable-features=…`, `--no-first-run`, `--password-store=basic`, …).
2. If headless: `--headless --hide-scrollbars --mute-audio --blink-settings=…`.
3. The user's args, then `--user-data-dir=<dir>`, then `--remote-debugging-pipe`.
4. Finally `about:blank` (persistent launch) or `--no-startup-window` (non-persistent).

pilot must ignore switches it doesn't recognise and stay alive with zero windows.

**Pipe transport**
- stdio is `[ignore, pipe, pipe, pipe, pipe]`. Read CDP on **fd 3** and write on **fd 4**. Each message is UTF-8 JSON followed by `\0`.
- There is no readiness handshake: `Browser.getVersion` arrives immediately.

**Shutdown**
- Playwright sends `Browser.close` with id −9999 and ignores the reply. It expects the process to exit, and otherwise SIGKILLs the process group.
- pilot should also exit when fd 3 reaches EOF.

**Port mode** (`--remote-debugging-port=N`, where 0 means pick a free port)
- Print `DevTools listening on ws://…` to stderr.
- Serve `GET /json/version/` (**with the trailing slash**), returning `webSocketDebuggerUrl`.
- Write `DevToolsActivePort` into the profile directory.

**`Browser.getVersion`**
- Playwright takes the text after the `/` in `product` as the version.
- If `userAgent` contains `Headless`, Playwright treats the browser as headless and sends `Page.setFontFamilies`.
- If it contains `Macintosh`, Playwright sends mac editing commands along with key events.

## Session invariants

- Sessions are flattened, and **every reply carries the `sessionId` of its request**. A reply with an unknown `id` trips an assert in Playwright.
- Commands are pipelined; process them in order per session.
- Events caused by a command go out before the reply to any later command. Playwright sends `Page.enable` after input purely as a flush barrier.
- `Target.setAutoAttach{autoAttach:true, waitForDebuggerOnStart:true, flatten:true}` must lead to `Target.attachedToTarget{sessionId, targetInfo{targetId, type:"page", url, browserContextId, openerId?}, waitingForDebugger:true}` for every page.
  - **`browserContextId` is mandatory.**
  - Page scripts don't run until `Runtime.runIfWaitingForDebugger`.
- `Target.createTarget` replies only **after** `attachedToTarget` has been sent.
- **The main frame id equals the targetId.**
- `Runtime.enable` replays the execution contexts that already exist.
- `Page.setLifecycleEventsEnabled` replays `DOMContentLoaded` and `load` for a document that has already loaded.
- `executionContextCreated.auxData` is `{frameId, isDefault, type}`, with `type` set to `"default"` or `"isolated"`, plus `name` for isolated worlds.
- **Isolated worlds** come from `Page.createIsolatedWorld` and from the `worldName` of `addScriptToEvaluateOnNewDocument`. Each is a separate V8 context that shares DOM nodes with the page but has its own JS wrappers.

## Per-page setup (sent in parallel; any failure makes `newPage` fail)

- `Page.enable`, `Page.getFrameTree`, `Log.enable`, `Page.setLifecycleEventsEnabled`, `Runtime.enable`
- `Page.addScriptToEvaluateOnNewDocument{source:"", worldName:"__playwright_utility_world_<guid>"}`
- `Network.enable`, `Target.setAutoAttach`, `Emulation.setFocusEmulationEnabled`, `Emulation.setEmulatedMedia`
- `Emulation.setDeviceMetricsOverride` (when there is a viewport)
- `Page.setFontFamilies` (when headless)
- init scripts (`runImmediately:true`)
- `Runtime.runIfWaitingForDebugger`

`Browser.getWindowForTarget` and `Inspector.enable` may fail without harm.

## Navigation (`page.goto`)

`Page.navigate{url, frameId, referrerPolicy}` returns `{loaderId, errorText?}`. Then emit these, in order:
1. `Network.requestWillBeSent` (`requestId == loaderId`, `type:"Document"`)
2. `Network.responseReceived`
3. Old contexts are destroyed
4. `Page.frameNavigated{frame{id, loaderId, url}}`
5. `executionContextCreated` for the main world, then for the utility world
6. `Page.lifecycleEvent` `DOMContentLoaded`, then `load`
7. `Network.loadingFinished`

Playwright works out `networkidle` itself from Network events. A same-document navigation emits `Page.navigatedWithinDocument`, and the navigate reply has no `loaderId`.

## Actions

- **evaluate**
  - Playwright installs a helper once per context with `Runtime.evaluate`.
  - Every evaluation is then `Runtime.callFunctionOn{functionDeclaration, objectId, arguments, returnByValue, awaitPromise:true, userGesture:true}`.
  - Element handles are RemoteObjects with `subtype:"node"`.
- **click**
  1. In-page checks that the element is visible, enabled and stable.
  2. `DOM.scrollIntoViewIfNeeded`. Playwright expects the exact errors `"Node does not have a layout object"` and `"Node is detached from document"`.
  3. `DOM.getContentQuads`.
  4. An in-page `elementsFromPoint` hit-target check.
  5. `Input.dispatchMouseEvent`: moved, then pressed, then released.
  6. `Page.enable` as a flush barrier.
- **hover** goes through `Input.setInterceptDrags`, which must succeed.
- **fill / type**
  - `fill`: in-page select, then `Input.insertText`.
  - `type` / `press`: `Input.dispatchKeyEvent` `keyDown`, then `keyUp`.
- **screenshot:** `Page.getLayoutMetrics`, then `Page.captureScreenshot{format, quality, clip, captureBeyondViewport}`.
- **resize:** `Emulation.setDeviceMetricsOverride`.
- **close**
  - page: `Target.closeTarget`, after which pilot emits `Target.detachedFromTarget`.
  - context: `Target.disposeBrowserContext`.
  - browser: `Browser.close`.

## Snapshot and actionability needs

`browser_snapshot` is `page.ariaSnapshot({mode:"ai"})`. It is computed by Playwright's injected script (`packages/injected/src/{injectedScript,ariaSnapshot,roleUtils,domUtils}.ts`), **not** the CDP Accessibility domain. That script needs:

- **At construction:** `MutationObserver`, capture-phase listeners, `CustomEvent`.
- **Tree walking:**
  - `childNodes`, `getRootNode`
  - shadow DOM (`shadowRoot`, `host`, `assignedNodes()`, `assignedSlot`)
  - `closest`, `matches`, `querySelectorAll`
- **Style and geometry:**
  - `getComputedStyle(el, "::before"/"::after")`
  - `Element.checkVisibility()`, which is called unconditionally
  - `getBoundingClientRect` on elements and on `Range`
  - `elementsFromPoint` on both Document **and ShadowRoot**
  - `scrollIntoView(options)`
  - `requestAnimationFrame`, which **must keep firing while headless**
- **Names, roles and state:** ARIA attributes, `labels`, form-control state, `isContentEditable`, `innerText`, `inert`, `activeElement`, `CSS.escape`.

**What actionability needs from layout:**
- **visible:** `checkVisibility()` is true and the bounding rect is non-empty.
- **stable:** the same rect on two consecutive `requestAnimationFrame` callbacks.
- **receives events:** `elementsFromPoint` at the centre of the content quad returns the target or one of its descendants.
- Rects, quads, hit-testing and `clientX/Y` must all come from the same **current, scroll-aware** layout.
- Input events are `isTrusted`.

## Minimum CDP surface

**R** = required. **S** = must return success; a no-op is fine. **C** = needed only for a particular feature.

| Domain | R | S | C |
|---|---|---|---|
| Browser | getVersion, close | setDownloadBehavior | getWindowForTarget, setWindowBounds (errors are tolerated) |
| Target | setAutoAttach, createTarget, closeTarget, createBrowserContext, disposeBrowserContext; events attachedToTarget, detachedFromTarget | getTargetInfo | attachToTarget (newCDPSession) |
| Page | enable, getFrameTree, setLifecycleEventsEnabled, navigate, addScriptToEvaluateOnNewDocument, createIsolatedWorld, reload, getNavigationHistory, navigateToHistoryEntry, getLayoutMetrics, captureScreenshot, handleJavaScriptDialog; events frameNavigated, lifecycleEvent, navigatedWithinDocument, frameAttached/Detached, javascriptDialogOpening/Closed | setFontFamilies, bringToFront | printToPDF, startScreencast, setInterceptFileChooserDialog |
| Runtime | enable, runIfWaitingForDebugger, evaluate, callFunctionOn; events executionContextCreated/Destroyed/Cleared, consoleAPICalled, exceptionThrown | releaseObject | getProperties, addBinding |
| DOM | describeNode, resolveNode, getContentQuads, scrollIntoViewIfNeeded | | getFrameOwner (iframes), setFileInputFiles |
| Input | dispatchMouseEvent, dispatchKeyEvent, insertText | setInterceptDrags | dispatchDragEvent, dispatchTouchEvent |
| Emulation | setDeviceMetricsOverride (changes `innerWidth`, layout and media queries) | setFocusEmulationEnabled, setEmulatedMedia | setUserAgentOverride, locale, timezone, geolocation, touch |
| Network | requestWillBeSent / responseReceived / loadingFinished / loadingFailed (strongly recommended) | enable | getResponseBody, setExtraHTTPHeaders, setCacheDisabled |
| Fetch | | | enable, requestPaused, continueRequest, fulfillRequest, failRequest (routes, `--allowed-origins`) |
| Log | | enable | |

Playwright never uses Accessibility.*, Performance.*, Overlay.*, DOM.getDocument or DOM.querySelector.

## Compatibility oracles

- **Playwright's page tests:**
  - `CRPATH=<abs>/pilot npx playwright test --config=tests/library/playwright.config.ts --project=chromium-page --reporter=json`, run from a playwright checkout.
  - Use the `chromium-library` project for the launch and context tests.
- **MCP tests:** `npm run ctest-mcp`, adding a project with `mcpArgs: ['--executable-path=<abs>/pilot']`.
- **Golden transcripts:** run with `DEBUG=pw:protocol` against real Chromium and against pilot, then diff the CDP traffic for each MCP tool.
