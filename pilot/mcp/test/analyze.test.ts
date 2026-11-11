// Record→replay analyzer: every value in a recorded request is traced to the
// place a later run gets it from (docs/record-replay.md, "Values that change
// between runs"). 2026-11-11: written before the analyzer exists.
import { test } from "node:test";
import assert from "node:assert/strict";

import { analyzeRecording } from "../src/analyze.ts";
import type { Recording, RecordedRequest } from "../src/recording.ts";

function request(partial: Partial<RecordedRequest> & Pick<RecordedRequest, "id" | "step" | "url">): RecordedRequest {
  return {
    method: "GET",
    headers: [],
    resourceType: "document",
    response: { status: 200, headers: [], mimeType: "text/html", body: "" },
    ...partial,
  };
}

function recording(requests: RecordedRequest[], extra: Partial<Recording> = {}): Recording {
  return {
    name: "fixture",
    inputs: {},
    steps: [],
    requests,
    ...extra,
  };
}

test("a CSRF token posted by a form comes from the earlier page's hidden input", () => {
  const token = "a8Kq2vX9pLmN3rT7wY1zB4cD6eF0gH5j";
  const plan = analyzeRecording(recording([
    request({
      id: "r1",
      step: 1,
      url: "https://shop.test/orders/new",
      response: {
        status: 200,
        headers: [],
        mimeType: "text/html",
        body: `<form method=post action=/orders><input type="hidden" name="authenticity_token" value="${token}"><input name=qty></form>`,
      },
    }),
    request({
      id: "r2",
      step: 2,
      method: "POST",
      url: "https://shop.test/orders",
      headers: [["content-type", "application/x-www-form-urlencoded"]],
      body: `authenticity_token=${token}&qty=3`,
    }),
  ]));
  const post = plan.requests.find((entry) => entry.id === "r2");
  assert.ok(post, "the POST stays in the plan");
  const slot = post.slots.find((entry) => entry.location.kind === "form" && entry.location.name === "authenticity_token");
  assert.deepEqual(slot?.source, {
    kind: "extract",
    fromRequest: "r1",
    rule: { in: "html-input", name: "authenticity_token" },
  });
});

test("an id returned by an earlier JSON response is extracted by its path", () => {
  const plan = analyzeRecording(recording([
    request({
      id: "r1",
      step: 1,
      method: "POST",
      url: "https://api.shop.test/carts",
      resourceType: "fetch",
      headers: [["content-type", "application/json"]],
      body: "{}",
      response: {
        status: 201,
        headers: [],
        mimeType: "application/json",
        body: JSON.stringify({ cart: { id: "c_7f3a91d2", items: [] } }),
      },
    }),
    request({
      id: "r2",
      step: 2,
      url: "https://api.shop.test/carts/c_7f3a91d2/summary?cart=c_7f3a91d2",
      resourceType: "fetch",
      response: { status: 200, headers: [], mimeType: "application/json", body: "{}" },
    }),
  ]));
  const summary = plan.requests.find((entry) => entry.id === "r2");
  const query = summary?.slots.find((entry) => entry.location.kind === "query" && entry.location.name === "cart");
  assert.deepEqual(query?.source, {
    kind: "extract",
    fromRequest: "r1",
    rule: { in: "json", path: ["cart", "id"] },
  });
  const segment = summary?.slots.find((entry) => entry.location.kind === "path" && entry.location.index === 1);
  assert.deepEqual(segment?.source, {
    kind: "extract",
    fromRequest: "r1",
    rule: { in: "json", path: ["cart", "id"] },
  });
});

test("a request header comes from an earlier response header", () => {
  const plan = analyzeRecording(recording([
    request({
      id: "r1",
      step: 1,
      url: "https://app.test/session",
      resourceType: "fetch",
      response: {
        status: 200,
        headers: [["x-csrf-token", "tok-5d1e9b77c0a4"]],
        mimeType: "application/json",
        body: "{}",
      },
    }),
    request({
      id: "r2",
      step: 2,
      method: "DELETE",
      url: "https://app.test/items/42",
      resourceType: "fetch",
      headers: [["x-csrf-token", "tok-5d1e9b77c0a4"]],
    }),
  ]));
  const remove = plan.requests.find((entry) => entry.id === "r2");
  const header = remove?.slots.find((entry) => entry.location.kind === "header" && entry.location.name === "x-csrf-token");
  assert.deepEqual(header?.source, {
    kind: "extract",
    fromRequest: "r1",
    rule: { in: "header", name: "x-csrf-token" },
  });
});

test("text the agent typed becomes a replay input, ahead of an earlier response that echoes it", () => {
  const plan = analyzeRecording(recording(
    [
      request({
        id: "r1",
        step: 1,
        url: "https://shop.test/search",
        response: { status: 200, headers: [], mimeType: "text/html", body: "<p>Recent: blue kettle</p>" },
      }),
      request({
        id: "r2",
        step: 3,
        url: "https://shop.test/search?q=blue+kettle",
      }),
    ],
    {
      steps: [
        { index: 1, tool: "browser_navigate", args: { url: "https://shop.test/search" } },
        { index: 2, tool: "browser_type", args: { ref: "e12", text: "blue kettle" } },
        { index: 3, tool: "browser_press_key", args: { key: "Enter" } },
      ],
    },
  ));
  const search = plan.requests.find((entry) => entry.id === "r2");
  const query = search?.slots.find((entry) => entry.location.kind === "query" && entry.location.name === "q");
  assert.deepEqual(query?.source, { kind: "input", name: "step2_text" });
  assert.deepEqual(plan.inputs, { step2_text: "blue kettle" });
});

test("a task input named at task_start keeps its name", () => {
  const plan = analyzeRecording(recording(
    [request({ id: "r1", step: 1, url: "https://shop.test/orders?month=2026-10", resourceType: "fetch" })],
    { inputs: { month: "2026-10" } },
  ));
  const query = plan.requests[0]?.slots.find((entry) => entry.location.kind === "query" && entry.location.name === "month");
  assert.deepEqual(query?.source, { kind: "input", name: "month" });
  assert.deepEqual(plan.inputs, { month: "2026-10" });
});

test("a cache-busting timestamp is regenerated", () => {
  const plan = analyzeRecording(recording([
    request({ id: "r1", step: 1, url: "https://shop.test/feed?_=1762889000123&s=1762889000", resourceType: "xhr" }),
  ]));
  const slots = plan.requests[0]?.slots ?? [];
  assert.deepEqual(slots.find((entry) => entry.location.kind === "query" && entry.location.name === "_")?.source, {
    kind: "regenerate",
    format: "epoch-ms",
  });
  assert.deepEqual(slots.find((entry) => entry.location.kind === "query" && entry.location.name === "s")?.source, {
    kind: "regenerate",
    format: "epoch-s",
  });
});

test("a value seen nowhere before stays a constant", () => {
  const plan = analyzeRecording(recording([
    request({ id: "r1", step: 1, url: "https://shop.test/list?view=compact", resourceType: "fetch" }),
  ]));
  const view = plan.requests[0]?.slots.find((entry) => entry.location.kind === "query" && entry.location.name === "view");
  assert.deepEqual(view?.source, { kind: "constant", value: "compact" });
  assert.equal(plan.replayable, true);
});

test("a write carrying a long value found in no earlier response or input is JS-computed and blocks replay", () => {
  const signature = "9f2c4e6a8b0d1f3e5a7c9b1d3f5e7a9c";
  const plan = analyzeRecording(recording([
    request({ id: "r1", step: 1, url: "https://bank.test/transfer" }),
    request({
      id: "r2",
      step: 2,
      method: "POST",
      url: "https://bank.test/api/transfer",
      resourceType: "fetch",
      headers: [["content-type", "application/json"]],
      body: JSON.stringify({ amount: 10, sig: signature }),
    }),
  ]));
  const post = plan.requests.find((entry) => entry.id === "r2");
  const sig = post?.slots.find((entry) => entry.location.kind === "json" && entry.location.path.join(".") === "sig");
  assert.deepEqual(sig?.source, { kind: "js-computed", value: signature });
  assert.equal(plan.replayable, false);
  assert.match(plan.notReplayableReasons.join("\n"), /r2.*sig/);
});

test("every non-GET request is listed as a write the replay would repeat", () => {
  const plan = analyzeRecording(recording([
    request({ id: "r1", step: 1, url: "https://shop.test/cart" }),
    request({
      id: "r2",
      step: 2,
      method: "POST",
      url: "https://shop.test/cart/items",
      resourceType: "fetch",
      headers: [["content-type", "application/json"]],
      body: JSON.stringify({ sku: "kettle-blue", qty: 1 }),
    }),
    request({ id: "r3", step: 3, method: "PATCH", url: "https://shop.test/cart/items/1", resourceType: "fetch" }),
  ]));
  assert.deepEqual(plan.writes, [
    { id: "r2", step: 2, method: "POST", url: "https://shop.test/cart/items" },
    { id: "r3", step: 3, method: "PATCH", url: "https://shop.test/cart/items/1" },
  ]);
});
