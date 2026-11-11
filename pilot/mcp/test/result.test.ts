// Record→replay result detection: the responses holding the text of the
// agent's last snapshot are the task's result (docs/record-replay.md,
// "Knowing which response is the answer"). 2026-11-11: written first.
import { test } from "node:test";
import assert from "node:assert/strict";

import { analyzeRecording } from "../src/analyze.ts";
import { replay } from "../src/replay.ts";
import type { RecordedRequest, Recording } from "../src/recording.ts";

function request(partial: Partial<RecordedRequest> & Pick<RecordedRequest, "id" | "step" | "url">): RecordedRequest {
  return {
    method: "GET",
    headers: [],
    resourceType: "document",
    response: { status: 200, headers: [], mimeType: "text/html", body: "" },
    ...partial,
  };
}

const shell = request({
  id: "r1",
  step: 1,
  url: "https://shop.test/orders",
  response: {
    status: 200,
    headers: [],
    mimeType: "text/html",
    body: "<html><body><nav>Home Orders Account</nav><div id=app></div></body></html>",
  },
});

const ordersApi = request({
  id: "r2",
  step: 1,
  url: "https://api.shop.test/orders?month=2026-10",
  resourceType: "fetch",
  response: {
    status: 200,
    headers: [],
    mimeType: "application/json",
    body: JSON.stringify({
      orders: [
        { id: 77, item: "Blue kettle", total: "$42.10" },
        { id: 78, item: "Green mug", total: "$9.50" },
      ],
    }),
  },
});

const accountApi = request({
  id: "r3",
  step: 1,
  url: "https://api.shop.test/account",
  resourceType: "fetch",
  response: {
    status: 200,
    headers: [],
    mimeType: "application/json",
    body: JSON.stringify({ name: "Avery Quinn", tier: "Gold member" }),
  },
});

const snapshot = [
  '- navigation: Home Orders Account',
  '- heading "Your orders" [level=1] [ref=e3]',
  '- table [ref=e4]:',
  '  - row "Blue kettle $42.10" [ref=e5]',
  '  - row "Green mug $9.50" [ref=e6]',
].join("\n");

test("the JSON response holding the rows the agent read is the result, not the page shell", () => {
  const recording: Recording = {
    name: "orders",
    inputs: {},
    steps: [],
    requests: [shell, ordersApi],
    finalSnapshot: snapshot,
  };
  assert.deepEqual(analyzeRecording(recording).resultRequests, ["r2"]);
});

test("when the answer spans two responses, both are results, in recorded order", () => {
  const recording: Recording = {
    name: "orders",
    inputs: {},
    steps: [],
    requests: [shell, ordersApi, accountApi],
    finalSnapshot: `${snapshot}\n- text: "Avery Quinn, Gold member"`,
  };
  assert.deepEqual(analyzeRecording(recording).resultRequests, ["r2", "r3"]);
});

test("without a final snapshot the last response with a body is the result", () => {
  const recording: Recording = { name: "orders", inputs: {}, steps: [], requests: [shell, ordersApi, accountApi] };
  assert.deepEqual(analyzeRecording(recording).resultRequests, ["r3"]);
});

test("a replay returns the fresh result responses: JSON as JSON text, HTML as plain text", async () => {
  const page = request({
    id: "r1",
    step: 1,
    url: "https://shop.test/status",
    response: { status: 200, headers: [], mimeType: "text/html", body: "<h1>Status</h1><p>All 3 systems up</p>" },
  });
  const recording: Recording = {
    name: "status",
    inputs: {},
    steps: [],
    requests: [page],
    finalSnapshot: '- heading "Status" [level=1]\n- paragraph: All 3 systems up',
  };
  const outcome = await replay(recording, analyzeRecording(recording), async () => ({
    status: 200,
    headers: [],
    mimeType: "text/html; charset=utf-8",
    body: "<html><head><style>p{}</style></head><body><h1>Status</h1><p>All 4 systems &amp; queues up</p><script>x()</script></body></html>",
  }), { inputs: {}, allowWrites: false });
  assert.equal(outcome.ok, true, JSON.stringify(outcome));
  assert.deepEqual(outcome.ok ? outcome.result : [], [
    { id: "r1", url: "https://shop.test/status", contentType: "text/html", text: "Status All 4 systems & queues up" },
  ]);
});
