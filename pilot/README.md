# pilot

pilot is the browser in this repository: Moli (Rust, V8) launched with Chromium's command line, so Playwright and Playwright MCP can start it the way they start Chrome.

## Use it with Playwright MCP

Build it, then point MCP at the binary:

```sh
cargo build --release -p pilot
claude mcp add pilot -- npx @playwright/mcp@latest --executable-path "$PWD/target/release/pilot"
```

MCP's default mode is headed, so pilot opens its watch-the-agent window (below). Add `--headless` to MCP's arguments for no window, `--isolated` for a fresh profile each time, or start pilot yourself (`pilot --remote-debugging-port=9222`) and pass MCP `--cdp-endpoint http://127.0.0.1:9222`.

From Playwright directly:

```js
const browser = await chromium.launch({ executablePath: '/path/to/target/release/pilot' });
```

## The window

Without `--headless`, pilot opens a window that shows the page the agent is driving:

- a tab per page, following the agent to new pages;
- a URL bar with back, forward and reload;
- an "Agent active" badge while the agent is sending commands;
- your mouse and keyboard reach the page, so you can take over at any point.

The agent owns the page's size; the window scales it to fit. Closing the window quits pilot, as closing Chrome's last window does; Playwright MCP starts it again on its next tool call. On a machine without a display, pilot runs headless.

## Environment variables

- `PILOT_NO_WINDOW`: set to any value to run without the window even when `--headless` is absent. The test suites set it so they do not open windows on a desktop.
- `PILOT_BIN`: used by `pilot/conformance` only. It selects the pilot binary under test; the default is `target/debug/pilot`.

## Tests

- `cargo nextest run -p pilot -p pilot-window`: command line, profile lock, launch contract over real pipes, and the window's logic against a real in-process browser.
- `cd pilot/conformance && npm test`: Playwright and Playwright MCP launch pilot by executable path and drive pages end to end.

See `FORK.md` for how pilot differs from upstream Moli, and `DECISIONS.md` for what it does on purpose.
