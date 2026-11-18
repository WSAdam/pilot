# pilot from Deno

## launchPilot

`mod.ts` starts pilot and hands back a connected Playwright browser in one call:

```ts
import { launchPilot } from "/path/to/pilot/deno/mod.ts";

await using pilot = await launchPilot();
const page = await pilot.browser.newPage();
await page.goto("https://example.com");
console.log(await page.title());
```

It starts pilot with `--remote-debugging-port=0`, so the system picks a free port. It reads the `DevTools listening on ws://…` line pilot prints and connects Playwright over CDP.

- `pilot.wsEndpoint` is the browser's CDP endpoint. Other clients can share it, for example Playwright MCP with `--cdp-endpoint`.
- `pilot.close()` (or the end of an `await using` block) asks pilot to close over CDP and waits for it to exit. If pilot is still running after 5 seconds, it is killed.
- If the script exits without calling `close()`, pilot is killed.

Options, all optional:

| Option | Default | What it does |
| --- | --- | --- |
| `executablePath` | `PILOT_BIN`, then `pilot` on the `PATH` | The pilot binary. |
| `headless` | `true` | `false` opens pilot's window, unless `PILOT_NO_WINDOW` is set. |
| `port` | `0` (a free port) | The CDP port. |
| `userDataDir` | none (profile in memory) | A profile directory that keeps cookies and storage between runs. |
| `args` | `[]` | More Chromium switches, such as `--proxy-server=…`. |
| `env` | `{}` | Extra environment variables for pilot. |
| `timeout` | `30000` | Milliseconds to wait for pilot to announce its endpoint and for Playwright to connect. |
| `onLog` | none | Receives each line of pilot's stderr log. |

It needs `-A`, or `--allow-run --allow-env --allow-net --allow-read --allow-write`. The module pins `npm:playwright-core@1.65.0-alpha-2026-10-30`, the version pilot is tested with.

## Plain Playwright launch

`chromium.launch({ executablePath })` also works from Deno (`launch_test.ts`). Use it when you don't need the endpoint.

## Tests

```sh
cd pilot/deno
PILOT_BIN=/path/to/pilot deno task test
```

Without `PILOT_BIN`, the tests use `target/release/pilot`.
