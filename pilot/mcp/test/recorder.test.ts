// The recorder against real pilot (docs/record-replay.md, "What gets
// recorded" and the 7a spike results). 2026-11-12: written before the recorder.
import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import type { Browser } from "playwright-core";

import { TaskRecorder } from "../src/recorder.ts";
import { launchPilot } from "./support/pilot.ts";
import { startShop } from "./support/shop.ts";
import type { Shop } from "./support/shop.ts";

let browser: Browser;
let shop: Shop;

before(async () => {
  shop = await startShop();
  browser = await launchPilot();
});

after(async () => {
  await browser.close();
  await shop.close();
});

test("a recording keeps the task's pages, form POST and redirect hop, each tagged with its step, and nothing else", async () => {
  const context = await browser.newContext();
  try {
    const recorder = new TaskRecorder(context);
    const page = await context.newPage();
    await page.goto(`${shop.base}/login`);

    recorder.start("order", {});
    recorder.beginStep("browser_navigate", { url: `${shop.base}/orders/new` });
    await page.goto(`${shop.base}/orders/new`);
    recorder.beginStep("browser_type", { target: "e5", text: "Blue kettle" });
    await page.fill('input[name="item"]', "Blue kettle");
    recorder.beginStep("browser_type", { target: "e6", text: "3" });
    await page.fill('input[name="qty"]', "3");
    recorder.beginStep("browser_click", { target: "e7" });
    await Promise.all([page.waitForURL(/\/orders\/\d+$/), page.click("button")]);
    const recording = await recorder.finish("- heading \"Order 100\" [level=1]");

    const path = (url: string): string => new URL(url).pathname;
    assert.deepEqual(
      recording.requests.map((request) => `${request.step} ${request.method} ${path(request.url)}`),
      ["1 GET /orders/new", "4 POST /orders", `4 ${recording.requests[2]?.method} /orders/100`],
      "the login before task_start, the stylesheet and the image are left out",
    );
    const [form, post, hop] = recording.requests;
    assert.match(form?.response.body ?? "", /name="csrf" value="[\w-]+"/, "the form page's body is kept");
    assert.match(post?.body ?? "", /^csrf=[\w-]+&item=Blue\+kettle&qty=3$/);
    assert.equal(post?.response.status, 302);
    assert.equal(hop?.redirectedFrom, post?.id);
    assert.match(hop?.response.body ?? "", /Order 100/);
    assert.equal(form?.fetchOrigin, shop.base);
    assert.deepEqual(recording.steps.map((step) => step.tool), [
      "browser_navigate",
      "browser_type",
      "browser_type",
      "browser_click",
    ]);
    assert.equal(recording.finalSnapshot, "- heading \"Order 100\" [level=1]");
  } finally {
    await context.close();
  }
});

test("a recording never stores the session cookie, sent or set", async () => {
  const context = await browser.newContext();
  try {
    const recorder = new TaskRecorder(context);
    const page = await context.newPage();
    recorder.start("login", {});
    recorder.beginStep("browser_navigate", { url: `${shop.base}/login` });
    await page.goto(`${shop.base}/login`);
    recorder.beginStep("browser_navigate", { url: `${shop.base}/orders/new` });
    await page.goto(`${shop.base}/orders/new`);
    const recording = await recorder.finish();

    const headerNames = recording.requests.flatMap((request) => [
      ...request.headers.map(([name]) => name.toLowerCase()),
      ...request.response.headers.map(([name]) => name.toLowerCase()),
    ]);
    assert.ok(!headerNames.includes("cookie"), "no Cookie header is stored");
    assert.ok(!headerNames.includes("set-cookie"), "no Set-Cookie header is stored");
    assert.ok(
      shop.hits.some((hit) => hit.path === "/orders/new" && hit.hasSession),
      "the cookie did go out on the wire",
    );
  } finally {
    await context.close();
  }
});
