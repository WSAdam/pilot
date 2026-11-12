# pilot-mcp

An MCP server in front of [Playwright MCP](https://github.com/microsoft/playwright-mcp) that drives pilot and can **record a task once and replay it** without the browser steps. The design and its decisions are in [docs/record-replay.md](../../docs/record-replay.md).

Every Playwright MCP tool works through it unchanged. It adds three:

| Tool | What it does |
|---|---|
| `task_start` `{name, inputs?}` | Starts recording. Each `browser_*` call after it is one step, with the requests it fired. |
| `task_finish` | Saves the recording and reports its inputs (text you typed counts), the writes a replay would repeat, and whether it can be replayed. |
| `task_replay` `{name, inputs?, allow_writes?}` | Sends the recorded requests again through pilot: fresh CSRF tokens and IDs, your new inputs, and pilot's cookies and Chrome TLS. Returns the task's result, or the step where it stopped. A task with writes (form posts, orders) needs `allow_writes: true`. |

## Running it

It needs Node 23.6 or later, which runs the TypeScript directly.

```sh
cd pilot/mcp && npm ci

# For an agent: MCP on stdio
node src/main.ts --pilot ../../target/release/pilot --user-data-dir ~/.pilot/profile

# For cron and scripts: replay a recorded task and print its result as JSON
node src/main.ts replay monthly-orders --input month=2026-11 --user-data-dir ~/.pilot/profile
node src/main.ts replay place-order --input step3_text="Green mug" --allow-writes

# The recorded tasks
node src/main.ts list --user-data-dir ~/.pilot/profile
```

An MCP client config entry:

```json
{ "command": "node", "args": ["/path/to/pilot/mcp/src/main.ts", "--user-data-dir", "/Users/you/.pilot/profile"], "env": { "PILOT_BIN": "/path/to/pilot" } }
```

`replay` exits 0 when the replay succeeded, 1 when it stopped or was refused, and 2 on bad arguments.

**The profile.**
- With `--user-data-dir`, logins persist and recordings live in `<user-data-dir>/pilot-recordings`. Without it, recordings live in `~/.pilot/recordings`.
- A profile can be open in one pilot at a time, so a CLI `replay` can't share a profile with a running `pilot-mcp` server.

**Recordings never store cookie values.** They do hold response bodies, so they're written owner-only (0600).

## Environment variables

- `PILOT_BIN`: the pilot executable, used when `--pilot` isn't given. The tests use it too, and fall back to `target/release/pilot`.

## Tests

```sh
npm test                # node --test; the record-and-replay tests launch real pilot
npx tsc --noEmit -p .   # strict type check
```
