// Recordings on disk: one private JSON file per task, under names that can't
// leave the recordings folder (docs/record-replay.md, "Safety"). 2026-11-12.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { listRecordings, loadRecording, saveRecording } from "../src/store.ts";
import type { Recording } from "../src/recording.ts";

const recording: Recording = {
  name: "monthly-orders",
  inputs: { month: "2026-10" },
  steps: [{ index: 1, tool: "browser_navigate", args: { url: "https://shop.test/orders" } }],
  requests: [],
};

test("a saved recording loads back unchanged and is readable only by its owner", async () => {
  const dir = join(await mkdtemp(join(tmpdir(), "pilot-mcp-store-")), "recordings");
  const path = await saveRecording(dir, recording);
  assert.deepEqual(await loadRecording(dir, "monthly-orders"), recording);
  assert.equal((await stat(path)).mode & 0o777, 0o600);
  assert.equal((await stat(dir)).mode & 0o777, 0o700);
  assert.deepEqual(await listRecordings(dir), ["monthly-orders"]);
});

test("a task name that could leave the recordings folder is refused", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pilot-mcp-store-"));
  for (const name of ["../escape", "a/b", "", ".hidden", "x".repeat(65)]) {
    await assert.rejects(saveRecording(dir, { ...recording, name }), /task name/, name);
    await assert.rejects(loadRecording(dir, name), /task name/, name);
  }
});

test("loading a task that was never recorded says so", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pilot-mcp-store-"));
  await assert.rejects(loadRecording(dir, "never"), /no recording named "never"/);
  assert.deepEqual(await listRecordings(join(dir, "missing")), []);
});
