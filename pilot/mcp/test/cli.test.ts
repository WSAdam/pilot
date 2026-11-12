// The pilot-mcp command: an MCP server on stdio for agents, and `replay` for
// cron and scripts (docs/record-replay.md, decision 3). 2026-11-12: written
// before main.ts.
import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

import { TaskRecorder } from "../src/recorder.ts";
import { saveRecording } from "../src/store.ts";
import { launchPilot, pilotBin } from "./support/pilot.ts";
import { startShop } from "./support/shop.ts";
import type { Shop } from "./support/shop.ts";

const main = fileURLToPath(new URL("../src/main.ts", import.meta.url));
const run = promisify(execFile);

let shop: Shop;
let recordingsDir: string;

before(async () => {
  shop = await startShop();
  recordingsDir = join(await mkdtemp(join(tmpdir(), "pilot-mcp-cli-")), "recordings");
  // A task that signs in first, so a replay works from a fresh profile.
  const browser = await launchPilot();
  try {
    const context = await browser.newContext();
    const recorder = new TaskRecorder(context);
    const page = await context.newPage();
    recorder.start("order", {});
    recorder.beginStep("browser_navigate", { url: `${shop.base}/login` });
    await page.goto(`${shop.base}/login`);
    recorder.beginStep("browser_navigate", { url: `${shop.base}/orders/new` });
    await page.goto(`${shop.base}/orders/new`);
    recorder.beginStep("browser_type", { target: "e5", text: "Blue kettle" });
    await page.fill('input[name="item"]', "Blue kettle");
    recorder.beginStep("browser_type", { target: "e6", text: "3" });
    await page.fill('input[name="qty"]', "3");
    recorder.beginStep("browser_click", { target: "e7" });
    await Promise.all([page.waitForURL(/\/orders\/\d+$/), page.click("button")]);
    await saveRecording(recordingsDir, await recorder.finish(await page.locator("body").ariaSnapshot()));
  } finally {
    await browser.close();
  }
});

after(async () => {
  await shop?.close();
});

const common = (): string[] => ["--pilot", pilotBin, "--recordings", recordingsDir, "--headless"];

test("pilot-mcp replay places a new order from a fresh profile and prints the result as JSON", async () => {
  const { stdout } = await run(process.execPath, [
    main,
    "replay",
    "order",
    ...common(),
    "--input",
    "step3_text=Green mug",
    "--input",
    "step4_text=5",
    "--allow-writes",
  ]);
  const outcome = JSON.parse(stdout) as { ok: boolean; result: { text: string }[] };
  assert.equal(outcome.ok, true, stdout);
  assert.match(outcome.result[0]?.text ?? "", /Green mug x 5/);
  assert.ok(shop.orders().some((order) => order.item === "Green mug" && order.qty === 5));
});

test("pilot-mcp replay without --allow-writes exits 1 and names the writes", async () => {
  const ordersBefore = shop.orders().length;
  await assert.rejects(
    run(process.execPath, [main, "replay", "order", ...common()]),
    (error: { code?: number; stdout?: string }) => {
      assert.equal(error.code, 1);
      assert.match(error.stdout ?? "", /allow_writes/);
      return true;
    },
  );
  assert.equal(shop.orders().length, ordersBefore);
});

test("pilot-mcp list names the recorded tasks", async () => {
  const { stdout } = await run(process.execPath, [main, "list", "--recordings", recordingsDir]);
  assert.equal(stdout.trim(), "order");
});

test("pilot-mcp with no command serves MCP on stdio with the task tools", async () => {
  const transport = new StdioClientTransport({ command: process.execPath, args: [main, ...common()], stderr: "ignore" });
  const client = new Client({ name: "pilot-mcp-cli-test", version: "0.0.0" });
  await client.connect(transport);
  try {
    const names = (await client.listTools()).tools.map((tool) => tool.name);
    assert.ok(names.includes("browser_navigate"));
    assert.ok(names.includes("task_replay"));
  } finally {
    await client.close();
  }
});
