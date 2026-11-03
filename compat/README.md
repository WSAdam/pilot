# compat

Measurements of how much of the Playwright and Playwright MCP surface pilot supports. Each file records a date, the tool versions and the pilot binary it ran against.

| File | What it measures | How to refresh |
|---|---|---|
| `mcp-tools.json` | Every Playwright MCP tool called once against pilot (headless, `--caps vision,pdf,testing`) on `pilot/conformance/fixtures/sweep.html`, with a check of the page state each tool should leave | `cd pilot/conformance && npm run sweep` (uses `target/release/pilot` unless `PILOT_BIN` is set) |

A failing entry is a gap to close; its fix gets a regression test in `pilot/conformance`.
