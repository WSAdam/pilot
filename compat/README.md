# compat

Measurements of how much of the Playwright and Playwright MCP surface, and of the real web, pilot handles. Each file records a date and the pilot binary it ran against. `v1-suite.md` says what pilot v1 has to pass.

| File | What it measures | How to refresh |
|---|---|---|
| `mcp-tools.json` | Every Playwright MCP tool called once against pilot (headless, `--caps vision,pdf,testing`) on `pilot/conformance/fixtures/sweep.html`, with a check of the page state each tool should leave | `cd pilot/conformance && npm run sweep` |
| `todomvc.json` | The TodoMVC flows (add, complete, filter, clear, edit) in every framework example hosted on todomvc.com | `cd pilot/conformance && npm run todomvc` |
| `open-web.json` | 30 public sites loaded as an agent would: goto, settle, AI snapshot, click a link | `cd pilot/conformance && npm run open-web` |

The scripts use `target/release/pilot` unless `PILOT_BIN` is set. A failing entry is a lead: when it turns out to be a pilot bug, its fix gets a regression test in `pilot/conformance`.
