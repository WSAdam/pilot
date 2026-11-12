// Record→replay engine: fills each request from the plan, checks every fresh
// response against the recording, stops at the first mismatch, and never
// repeats a write without allow_writes (docs/record-replay.md, "When a replay
// counts as failed" and "Safety"). 2026-11-11: written before the engine.
import { test } from "node:test";
import assert from "node:assert/strict";

import { analyzeRecording } from "../src/analyze.ts";
import { replay } from "../src/replay.ts";
import type { ConcreteRequest, ReplayResponse } from "../src/replay.ts";
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

function recording(requests: RecordedRequest[], extra: Partial<Recording> = {}): Recording {
  return { name: "fixture", inputs: {}, steps: [], requests, ...extra };
}

/** A fake site: answers each request by its URL path, and logs what was sent. */
function site(routes: Record<string, (sent: ConcreteRequest) => ReplayResponse>) {
  const sent: ConcreteRequest[] = [];
  return {
    sent,
    send: async (concrete: ConcreteRequest): Promise<ReplayResponse> => {
      sent.push(concrete);
      const route = routes[`${concrete.method} ${new URL(concrete.url).pathname}`];
      if (route === undefined) {
        return { status: 404, headers: [], mimeType: "text/plain", body: "no route" };
      }
      return route(concrete);
    },
  };
}

const formRecording = recording([
  request({
    id: "r1",
    step: 1,
    url: "https://shop.test/orders/new",
    response: {
      status: 200,
      headers: [],
      mimeType: "text/html",
      body: '<form><input type="hidden" name="authenticity_token" value="OLDtoken0000000000000001"></form>',
    },
  }),
  request({
    id: "r2",
    step: 2,
    method: "POST",
    url: "https://shop.test/orders",
    headers: [["content-type", "application/x-www-form-urlencoded"]],
    body: "authenticity_token=OLDtoken0000000000000001&qty=3",
    response: { status: 302, headers: [["location", "/orders/77"]], mimeType: "text/html", body: "" },
  }),
]);

test("a form POST carries the CSRF token from the fresh page, not the recorded one", async () => {
  const fake = site({
    "GET /orders/new": () => ({
      status: 200,
      headers: [],
      mimeType: "text/html",
      body: '<form><input type="hidden" name="authenticity_token" value="NEWtoken9999999999999999"></form>',
    }),
    // fetch() followed the recorded 302 to the new order (redirected: true since 2026-11-12).
    "POST /orders": () => ({
      status: 200,
      headers: [],
      mimeType: "text/html; charset=utf-8",
      body: "<p>Order 78</p>",
      redirected: true,
      finalUrl: "https://shop.test/orders/78",
    }),
  });
  const outcome = await replay(formRecording, analyzeRecording(formRecording), fake.send, { inputs: {}, allowWrites: true });
  assert.equal(outcome.ok, true, JSON.stringify(outcome));
  assert.equal(fake.sent[1]?.body, "authenticity_token=NEWtoken9999999999999999&qty=3");
});

test("a task with writes sends nothing unless allow_writes is set, and names the writes", async () => {
  const fake = site({});
  const outcome = await replay(formRecording, analyzeRecording(formRecording), fake.send, { inputs: {}, allowWrites: false });
  assert.equal(outcome.ok, false);
  assert.equal(fake.sent.length, 0);
  assert.match(outcome.ok ? "" : outcome.reason, /POST https:\/\/shop\.test\/orders/);
  assert.match(outcome.ok ? "" : outcome.reason, /allow_writes/);
});

test("a recording that isn't replayable sends nothing", async () => {
  const signed = recording([
    request({
      id: "r1",
      step: 1,
      method: "POST",
      url: "https://bank.test/api/transfer",
      resourceType: "fetch",
      headers: [["content-type", "application/json"]],
      body: JSON.stringify({ sig: "9f2c4e6a8b0d1f3e5a7c9b1d3f5e7a9c" }),
    }),
  ]);
  const fake = site({});
  const outcome = await replay(signed, analyzeRecording(signed), fake.send, { inputs: {}, allowWrites: true });
  assert.equal(outcome.ok, false);
  assert.equal(fake.sent.length, 0);
  assert.match(outcome.ok ? "" : outcome.reason, /computed by page JavaScript/);
});

test("replay inputs replace typed text, and an input left out keeps its recorded value", async () => {
  const search = recording(
    [
      request({ id: "r1", step: 3, url: "https://shop.test/search?q=blue+kettle&page=1", resourceType: "fetch" }),
      request({ id: "r2", step: 4, url: "https://shop.test/related?q=green+mug", resourceType: "fetch" }),
    ],
    {
      steps: [
        { index: 2, tool: "browser_type", args: { ref: "e12", text: "blue kettle" } },
        { index: 3, tool: "browser_type", args: { ref: "e13", text: "green mug" } },
      ],
    },
  );
  const fake = site({
    "GET /search": () => ({ status: 200, headers: [], mimeType: "text/html", body: "" }),
    "GET /related": () => ({ status: 200, headers: [], mimeType: "text/html", body: "" }),
  });
  const outcome = await replay(search, analyzeRecording(search), fake.send, {
    inputs: { step2_text: "red teapot" },
    allowWrites: false,
  });
  assert.equal(outcome.ok, true, JSON.stringify(outcome));
  assert.equal(fake.sent[0]?.url, "https://shop.test/search?q=red+teapot&page=1");
  assert.equal(fake.sent[1]?.url, "https://shop.test/related?q=green+mug");
});

test("an id from a fresh JSON response fills the path, the query and a JSON body", async () => {
  const cart = recording([
    request({
      id: "r1",
      step: 1,
      method: "POST",
      url: "https://api.shop.test/carts",
      resourceType: "fetch",
      headers: [["content-type", "application/json"]],
      body: "{}",
      response: { status: 201, headers: [], mimeType: "application/json", body: JSON.stringify({ cart: { id: "c_7f3a91d2" } }) },
    }),
    request({
      id: "r2",
      step: 2,
      method: "POST",
      url: "https://api.shop.test/carts/c_7f3a91d2/items?cart=c_7f3a91d2",
      resourceType: "fetch",
      headers: [["content-type", "application/json"]],
      body: JSON.stringify({ cart: "c_7f3a91d2", sku: "kettle-blue" }),
      response: { status: 200, headers: [], mimeType: "application/json", body: JSON.stringify({ ok: true }) },
    }),
  ]);
  const fake = site({
    "POST /carts": () => ({ status: 201, headers: [], mimeType: "application/json", body: JSON.stringify({ cart: { id: "c_0000beef" } }) }),
    "POST /carts/c_0000beef/items": () => ({ status: 200, headers: [], mimeType: "application/json", body: JSON.stringify({ ok: true }) }),
  });
  const outcome = await replay(cart, analyzeRecording(cart), fake.send, { inputs: {}, allowWrites: true });
  assert.equal(outcome.ok, true, JSON.stringify(outcome));
  assert.equal(fake.sent[1]?.url, "https://api.shop.test/carts/c_0000beef/items?cart=c_0000beef");
  assert.deepEqual(JSON.parse(fake.sent[1]?.body ?? "null"), { cart: "c_0000beef", sku: "kettle-blue" });
});

test("timestamps are regenerated from the clock", async () => {
  const feed = recording([
    request({ id: "r1", step: 1, url: "https://shop.test/feed?_=1762889000123&s=1762889000", resourceType: "xhr" }),
  ]);
  const fake = site({ "GET /feed": () => ({ status: 200, headers: [], mimeType: "text/html", body: "" }) });
  const outcome = await replay(feed, analyzeRecording(feed), fake.send, {
    inputs: {},
    allowWrites: false,
    now: () => 1800000000456,
  });
  assert.equal(outcome.ok, true, JSON.stringify(outcome));
  assert.equal(fake.sent[0]?.url, "https://shop.test/feed?_=1800000000456&s=1800000000");
});

test("the first response whose status class differs stops the replay, and later requests aren't sent", async () => {
  const fake = site({
    "GET /orders/new": () => ({ status: 403, headers: [], mimeType: "text/html", body: "denied" }),
  });
  const outcome = await replay(formRecording, analyzeRecording(formRecording), fake.send, { inputs: {}, allowWrites: true });
  assert.deepEqual(outcome, {
    ok: false,
    failedStep: 1,
    failedRequest: "r1",
    reason: "GET https://shop.test/orders/new returned 403; the recording had 200",
  });
  assert.equal(fake.sent.length, 1);
});

test("a token missing from the fresh page stops the replay before the POST", async () => {
  const fake = site({
    "GET /orders/new": () => ({ status: 200, headers: [], mimeType: "text/html", body: "<p>Please sign in</p>" }),
  });
  const outcome = await replay(formRecording, analyzeRecording(formRecording), fake.send, { inputs: {}, allowWrites: true });
  assert.equal(outcome.ok, false);
  assert.equal(outcome.ok ? 0 : outcome.failedStep, 2);
  assert.match(outcome.ok ? "" : outcome.reason, /authenticity_token/);
  assert.equal(fake.sent.length, 1);
});

test("a JSON response missing a recorded top-level key fails the step", async () => {
  const list = recording([
    request({
      id: "r1",
      step: 1,
      url: "https://api.shop.test/orders",
      resourceType: "fetch",
      response: { status: 200, headers: [], mimeType: "application/json", body: JSON.stringify({ orders: [], total: 0 }) },
    }),
  ]);
  const fake = site({
    "GET /orders": () => ({ status: 200, headers: [], mimeType: "application/json", body: JSON.stringify({ error: "session expired" }) }),
  });
  const outcome = await replay(list, analyzeRecording(list), fake.send, { inputs: {}, allowWrites: false });
  assert.equal(outcome.ok, false);
  assert.match(outcome.ok ? "" : outcome.reason, /orders, total/);
});

test("an input the task doesn't take is refused, so a typo can't silently replay the recorded value", async () => {
  const orders = recording(
    [request({ id: "r1", step: 1, url: "https://shop.test/orders?month=2026-10", resourceType: "fetch" })],
    { inputs: { month: "2026-10" } },
  );
  const fake = site({});
  const outcome = await replay(orders, analyzeRecording(orders), fake.send, {
    inputs: { monht: "2026-11" },
    allowWrites: false,
  });
  assert.equal(outcome.ok, false);
  assert.equal(fake.sent.length, 0);
  assert.match(outcome.ok ? "" : outcome.reason, /monht.*month/);
});

// 2026-11-12, from the 7a spike: pilot follows a form POST's 302 itself, and
// fetch() does too, so the redirect hop is never sent on its own. pilot also
// reports the hop with the POST's method (a CDP bug, TODO.md), which must not
// turn it into a write.
const redirectRecording = recording([
  request({
    id: "r1",
    step: 1,
    method: "POST",
    url: "https://shop.test/orders",
    headers: [["content-type", "application/x-www-form-urlencoded"]],
    body: "item=Blue+kettle",
    response: { status: 302, headers: [["location", "/orders/100"]], mimeType: "" },
  }),
  request({
    id: "r2",
    step: 1,
    method: "POST",
    url: "https://shop.test/orders/100",
    redirectedFrom: "r1",
    response: { status: 200, headers: [], mimeType: "text/html", body: "<h1>Order 100</h1><p>Blue kettle</p>" },
  }),
], { finalSnapshot: '- heading "Order 100" [level=1]\n- paragraph: Blue kettle' });

test("a redirect hop is neither sent nor listed as a write", () => {
  const plan = analyzeRecording(redirectRecording);
  assert.deepEqual(plan.writes.map((write) => write.id), ["r1"]);
  const hop = plan.requests.find((entry) => entry.id === "r2");
  assert.equal(hop?.redirectedFrom, "r1");
  assert.deepEqual(hop?.slots, []);
});

test("a followed redirect answers its hop, so the fresh order page is the result", async () => {
  const fake = site({
    "POST /orders": () => ({
      status: 200,
      headers: [],
      mimeType: "text/html",
      body: "<h1>Order 101</h1><p>Blue kettle</p>",
      redirected: true,
      finalUrl: "https://shop.test/orders/101",
    }),
  });
  const outcome = await replay(redirectRecording, analyzeRecording(redirectRecording), fake.send, {
    inputs: {},
    allowWrites: true,
  });
  assert.equal(outcome.ok, true, JSON.stringify(outcome));
  assert.equal(fake.sent.length, 1);
  assert.deepEqual(outcome.ok ? outcome.result : [], [
    { id: "r2", url: "https://shop.test/orders/101", contentType: "text/html", text: "Order 101 Blue kettle" },
  ]);
});

test("a POST recorded as a redirect fails when the fresh one answers without redirecting", async () => {
  const fake = site({
    "POST /orders": () => ({ status: 200, headers: [], mimeType: "text/html", body: "<p>Bad token</p>", redirected: false }),
  });
  const outcome = await replay(redirectRecording, analyzeRecording(redirectRecording), fake.send, {
    inputs: {},
    allowWrites: true,
  });
  assert.equal(outcome.ok, false);
  assert.match(outcome.ok ? "" : outcome.reason, /didn't redirect; the recording had 302/);
});
