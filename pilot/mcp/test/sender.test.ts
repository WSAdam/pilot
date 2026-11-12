// Record in pilot, replay through fetch() in pilot pages: the whole loop
// against real pilot (docs/record-replay.md, 7a spike). 2026-11-12: written
// before the page sender.
import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import type { Browser } from "playwright-core";

import { analyzeRecording } from "../src/analyze.ts";
import { TaskRecorder } from "../src/recorder.ts";
import { replay } from "../src/replay.ts";
import { pageSender } from "../src/sender.ts";
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

test("a replay through pilot pages places a new order with a fresh CSRF token, the session cookie and new inputs", async () => {
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
    const recording = await recorder.finish(await page.locator("body").ariaSnapshot());
    const plan = analyzeRecording(recording);
    assert.deepEqual(plan.inputs, { step2_text: "Blue kettle", step3_text: "3" });

    const hitsBefore = shop.hits.length;
    const sender = pageSender(context);
    const outcome = await replay(recording, plan, sender.send, {
      inputs: { step2_text: "Green mug", step3_text: "5" },
      allowWrites: true,
    });
    await sender.close();

    assert.equal(outcome.ok, true, JSON.stringify(outcome));
    assert.deepEqual(
      shop.orders().map((order) => `${order.item} x ${order.qty}`),
      ["Blue kettle x 3", "Green mug x 5"],
    );
    assert.match(outcome.ok ? (outcome.result[0]?.text ?? "") : "", /Order 101 Green mug x 5/);
    const replayHits = shop.hits.slice(hitsBefore).filter((hit) => hit.path !== "/robots.txt");
    assert.deepEqual(
      replayHits.map((hit) => `${hit.method} ${hit.path} session=${hit.hasSession}`),
      ["GET /orders/new session=true", "POST /orders session=true", "GET /orders/101 session=true"],
    );
  } finally {
    await context.close();
  }
});
