# Record → replay: design (draft for review)

Build 7 in BUILD-PLAN.md. Written 2026-11-11. Nothing is built yet.

## The idea

A task an agent runs more than once costs the same every time. Examples: pull this month's orders, check a dashboard, submit the same form. Each run takes one LLM call per step, plus a full page load, JavaScript and layout for every page.

Record → replay makes the second run cheap:
- **The first run** goes through pilot as it does today, and pilot records the HTTP requests each step fires.
- **Later runs** send those requests straight through pilot's network stack, with no rendering and no LLM steps. They carry the same cookies and the same Chrome TLS and HTTP/2 fingerprint as the browser.
- **When a replay goes wrong**, the task runs in the browser again and the recording is refreshed.

## What a run looks like

```
task_start  name="orders"  inputs={"month": "2026-10"}
  ... the agent drives as usual: browser_navigate, browser_click, browser_type, browser_snapshot ...
task_finish                                   → saves the recording "orders"

task_replay name="orders"  inputs={"month": "2026-11"}
  → { ok: true, result: <the response(s) the agent read its answer from> }     about 1 s, no LLM steps
  → { ok: false, failedStep: 3, reason: "POST /api/orders returned 403" }     the agent re-runs the task in the browser
```

## Where it lives

**v1 needs no Rust.** It's a small MCP server, `pilot-mcp` (Node, in `pilot/mcp/`), that sits in front of Playwright MCP:
- It forwards every Playwright MCP tool unchanged.
- It adds three tools: `task_start`, `task_finish` and `task_replay`.
- Every forwarded tool call marks a step boundary, so each recorded request is tied to the step that fired it.

Because no Rust changes, v1 can be written and tested against the current release binary, with or without GitHub. pilot itself only changes if the spike below shows that a recording source or replay path needs it. That would be a light build: the `pilot` crate only, using `moli-fetch` as it is.

### Recording source (the spike picks one)

1. **A second CDP client.** `pilot-mcp` launches pilot on a port and points Playwright MCP at it with `--cdp-endpoint`. It then attaches to each page target itself, enables `Network`, and reads bodies with `Network.getResponseBody`. This is the cleanest option, but pilot's support for two clients on one page is untested; TODO has an open line about refs for a second MCP session.
2. **Playwright's own listeners.** At `task_start`, `pilot-mcp` installs `context.on('request'/'response')` listeners through Playwright MCP's run-code tool. Those listeners run in Playwright MCP's Node process with real `Response.body()`. This works with any browser, but depends on the run-code tool being enabled.
3. **A pilot-side recorder** that pilot writes from its own network layer. It has the highest fidelity (exact wire headers, and the CDP command that caused each request), but needs a heavy build because it touches `moli-protocol` and the renderer. This is the fallback only.

Playwright's `recordHar` context option doesn't work here: the HAR is written only when the context closes, not when a task ends.

### Replay path

- **v1: `fetch()` inside a pilot page on the task's origin.** pilot opens a cheap same-origin document, such as `/robots.txt`, and runs each recorded request there with `fetch(url, {method, headers, body, credentials: 'include', redirect: 'manual'})`.
  - It uses pilot's live cookie jar, TLS, HTTP/2 and proxy, so there are no profile-lock problems while the browser is running.
  - CORS behaves exactly as it did for the page that made the request originally.
- **The limit:** document navigations, such as form POSTs and link clicks, go out with `Sec-Fetch-Mode: cors` and `Sec-Fetch-Dest: empty` instead of `navigate` and `document`. Pages also can't set forbidden headers. Most sites don't check these; bot-walled ones might.
- **v2, only if the spike or the benchmark shows it's needed:** a `pilot replay` path that sends navigations with Chrome's exact headers through `moli-fetch`. That's a light build.

## What gets recorded

For each step, `pilot-mcp` records the requests the step caused:
- **Kept:** documents, XHR and fetch requests, plus every non-GET request.
- **Dropped:** images, CSS, fonts, scripts, media, and requests to known analytics and ads hosts.
- **Per request:** method, URL, headers (without `Cookie`, which the jar supplies), body, status, response headers, response body, and timing.

## Values that change between runs

The analyzer runs at `task_finish`, offline. It traces every value in every kept request:

| The value… | Becomes |
|---|---|
| appears in an earlier response (a body, a header, or a `Set-Cookie`). Examples: CSRF tokens, session-bound IDs, IDs returned by a create | an **extraction rule**: a JSON path, an HTML form-field name, or a regex anchored on the text around it. At replay, it's read from that step's fresh response. |
| equals text the agent typed (`browser_type`, `browser_fill_form`) or a `task_start` input | a **parameter**, filled from `task_replay` inputs |
| looks like a timestamp or a random nonce | **regenerated** in the same format |
| is in none of the above | a **constant**. If it sits in a non-GET request and looks computed (a long hex or base64 string), the step is flagged as **JS-computed**. |

A recording whose required requests contain JS-computed values isn't replayable. `task_finish` says so, and `task_replay` refuses it instead of sending requests that are bound to fail. Signed API calls and anti-bot sensor payloads land here.

## Knowing which response is the answer

At `task_finish`, `pilot-mcp` compares the agent's last snapshot text with the recorded response bodies. The response or responses that contain that text are marked as the **result**. `task_replay` returns them:
- JSON as JSON;
- HTML as text, with tags stripped and the length capped.

If the answer came from page JavaScript combining several responses, all of them are returned.

## When a replay counts as failed

Each step is checked against the recording:
- the status class is the same;
- the content type is the same;
- for JSON, every top-level key from the recording is present;
- every extraction rule finds its value;
- the result responses aren't empty.

The first check that fails stops the replay and returns `ok: false` with the step and the reason. v1 doesn't resume mid-task, because the browser isn't sitting at that step. The agent runs the task again in the browser, and its `task_finish` replaces the recording.

## Safety

- **A recording with any non-GET step contains writes.** `task_replay` requires `allow_writes: true` for it, and lists the writes it's about to repeat. Repeating a purchase or a post is the caller's decision, never a default.
- **Recordings stay local and are never logged.** They're stored in `<profile>/pilot-recordings/<name>.json`, kept out of git, and can contain response bodies with personal data. Cookie values are never stored, because the jar supplies them.

## Out of scope for v1

WebSockets, file uploads and downloads, flows whose requests are signed by page JavaScript (the JS-computed flag above), and multi-tab tasks.

## Plan

| Step | What | Build |
|---|---|---|
| 7a spike | Pick the recording source (option 1 or 2) and prove `fetch()` replay against pilot, including cookies and a CSRF form. Needs about 1 GB free on the Mac. | none |
| 7b recorder and analyzer | Step boundaries, filtering, the dataflow tracing, and result detection. `node --test` on saved recordings. | none |
| 7c replay | Replay, checks and fallback. Conformance tests in `pilot/conformance/` against a local fixture site (a login, a CSRF-protected form, and a JSON API whose IDs change on every run). | none |
| 7d packaging | `pilot-mcp` packaging, README, and a benchmark of 3–5 real tasks comparing first-run and replay time and LLM steps, plus the replay success rate over a week. | none |
| 7e (only if needed) | Navigations with Chrome's exact headers (`pilot replay`), or the pilot-side recorder | light or heavy |

## Questions for you

1. **Which tasks will you repeat?** Read-only ones (dashboards, lists, scraping) make a simpler v1. If writes matter from the start, the `allow_writes` gate and its tests come first.
2. **Is a wrapper MCP OK?** Agents would point at `pilot-mcp` instead of `@playwright/mcp`. Every Playwright tool still works through it.
3. **Who triggers replays?** Only the agent through MCP, or also a CLI or cron job with no agent in the loop? A CLI adds a `pilot-mcp replay <name>` command, which is small.
