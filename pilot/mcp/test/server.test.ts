// pilot-mcp as an agent sees it: Playwright MCP's tools plus task_start,
// task_finish and task_replay, against real pilot (docs/record-replay.md,
// "What a run looks like"). 2026-11-12: written before the server.
import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";

import { createPilotMcp } from "../src/server.ts";
import type { PilotMcp } from "../src/server.ts";
import { pilotBin } from "./support/pilot.ts";
import { startShop } from "./support/shop.ts";
import type { Shop } from "./support/shop.ts";

let shop: Shop;
let pilotMcp: PilotMcp;
let client: Client;

before(async () => {
  shop = await startShop();
  const scratch = await mkdtemp(join(tmpdir(), "pilot-mcp-server-"));
  pilotMcp = await createPilotMcp({
    pilotBin,
    headless: true,
    recordingsDir: join(scratch, "recordings"),
    playwright: { outputDir: join(scratch, "output"), filePaths: "absolute" },
  });
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  await pilotMcp.server.connect(serverTransport);
  client = new Client({ name: "pilot-mcp-test", version: "0.0.0" });
  await client.connect(clientTransport);
});

after(async () => {
  // Each step is guarded so a failed setup still frees the shop's port.
  await client?.close().catch(() => undefined);
  await pilotMcp?.close().catch(() => undefined);
  await shop?.close();
});

interface ToolText {
  text: string;
  isError: boolean;
}

async function call(name: string, args: Record<string, unknown> = {}): Promise<ToolText> {
  const result = await client.callTool({ name, arguments: args });
  const content = (result.content ?? []) as { type: string; text?: string }[];
  const text = content.map((part) => part.text ?? "").join("\n");
  // Playwright MCP links the page snapshot as a file; inline it for the test.
  const linked = /\[Snapshot\]\(([^)]+\.yml)\)/.exec(text)?.[1];
  return {
    text: linked === undefined ? text : `${text}\n${await readFile(linked, "utf8")}`,
    isError: result.isError === true,
  };
}

function refFor(snapshot: string, pattern: RegExp): string {
  const line = snapshot.split("\n").find((entry) => pattern.test(entry));
  const ref = /\[ref=(\w+)\]/.exec(line ?? "")?.[1];
  assert.ok(ref, `no ref for ${pattern} in\n${snapshot}`);
  return ref;
}

test("pilot-mcp lists Playwright MCP's tools and its own task tools", async () => {
  const names = (await client.listTools()).tools.map((tool) => tool.name);
  for (const name of ["browser_navigate", "browser_click", "browser_type", "task_start", "task_finish", "task_replay"]) {
    assert.ok(names.includes(name), `${name} in ${names.join(", ")}`);
  }
});

test("task_finish without task_start is an error", async () => {
  const finished = await call("task_finish");
  assert.equal(finished.isError, true);
  assert.match(finished.text, /task_start/);
});

test("a task recorded through Playwright MCP's tools replays with new inputs, and writes need allow_writes", async () => {
  assert.equal((await call("task_start", { name: "order" })).isError, false);
  await call("browser_navigate", { url: `${shop.base}/login` });
  const form = await call("browser_navigate", { url: `${shop.base}/orders/new` });
  await call("browser_type", { element: "Item", target: refFor(form.text, /textbox "Item"/), text: "Blue kettle" });
  await call("browser_type", { element: "Qty", target: refFor(form.text, /textbox "Qty"/), text: "3" });

  const duringRecording = await call("task_replay", { name: "order" });
  assert.equal(duringRecording.isError, true, "no replay while a task is being recorded");
  assert.match(duringRecording.text, /being recorded/);

  await call("browser_click", { element: "Place order", target: refFor(form.text, /button "Place order"/) });
  const finished = await call("task_finish");
  assert.equal(finished.isError, false, finished.text);
  const summary = JSON.parse(finished.text) as {
    inputs: Record<string, string>;
    writes: string[];
    replayable: boolean;
  };
  assert.deepEqual(summary.inputs, { step3_text: "Blue kettle", step4_text: "3" });
  assert.deepEqual(summary.writes, [`POST ${shop.base}/orders`]);
  assert.equal(summary.replayable, true);

  const refused = await call("task_replay", { name: "order", inputs: { step3_text: "Green mug", step4_text: "5" } });
  assert.equal(refused.isError, true);
  assert.match(refused.text, /allow_writes/);
  assert.equal(shop.orders().length, 1, "a refused replay sends nothing");

  const replayed = await call("task_replay", {
    name: "order",
    inputs: { step3_text: "Green mug", step4_text: "5" },
    allow_writes: true,
  });
  assert.equal(replayed.isError, false, replayed.text);
  const outcome = JSON.parse(replayed.text) as { ok: boolean; result: { text: string }[] };
  assert.equal(outcome.ok, true);
  assert.match(outcome.result[0]?.text ?? "", /Order 101 Green mug x 5/);
  assert.deepEqual(
    shop.orders().map((order) => `${order.item} x ${order.qty}`),
    ["Blue kettle x 3", "Green mug x 5"],
  );
});

test("replaying a task that was never recorded says so", async () => {
  const missing = await call("task_replay", { name: "never-recorded" });
  assert.equal(missing.isError, true);
  assert.match(missing.text, /no recording named "never-recorded"/);
});
