# compat

Measurements of how much of the Playwright and Playwright MCP surface, and of the real web, pilot handles. Each file records a date and the pilot binary it ran against. `v1-suite.md` says what pilot v1 has to pass.

| File | What it measures | How to refresh |
|---|---|---|
| `mcp-tools.json` | Every Playwright MCP tool called once against pilot (headless, `--caps vision,pdf,testing`) on `pilot/conformance/fixtures/sweep.html`, with a check of the page state each tool should leave | `cd pilot/conformance && npm run sweep` |
| `todomvc.json` | The TodoMVC flows (add, complete, filter, clear, edit) in every framework example hosted on todomvc.com | `cd pilot/conformance && npm run todomvc` |
| `playwright-mcp-suite.json` | Playwright's own `tests/mcp` suite (Playwright commit 018adb570), every test driving pilot, with failures split into harness causes and pilot gaps (each gap names its build and its conformance test) | `scripts/playwright-mcp-suite.sh <checkout-dir> <report.json>`: it fetches Playwright at that commit, installs and builds it, copies `playwright-mcp-suite/pilot.config.ts` in, and runs the suite with one worker. pilot is passed through `PLAYWRIGHT_MCP_EXECUTABLE_PATH`, which reaches both the MCP server and the CLI daemon. |
| `open-web.json` | 30 public sites loaded as an agent would: goto, settle, AI snapshot, click a link | `cd pilot/conformance && npm run open-web` |

The scripts use `target/release/pilot` unless `PILOT_BIN` is set. A failing entry is a lead: when it turns out to be a pilot bug, its fix gets a regression test in `pilot/conformance`.
