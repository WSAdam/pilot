// Animation frames run on a fixed 60 Hz grid, as Chrome's do (Build 5, speed).
// Playwright's click waits for two animation frames to see that the element
// isn't moving. pilot set each frame 16 ms after the request, so that wait
// always cost about 36 ms, where Chrome's averages about 25 ms: the first frame
// comes at the next vsync, a random 0-16.7 ms away.
// 2026-11-12: proven red against the release binary of 2026-11-10. The frame
// timestamps were `now + 16`, off the grid, and a two-frame wait measured 36.5 ms.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin } from './support.mjs';

const FRAME_MS = 1000 / 60;
let browser;
let page;

before(async () => {
  browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: 30_000 });
  page = await (await browser.newContext()).newPage();
  await page.setContent('<button id="go">Go</button>');
});
after(async () => {
  await browser?.close();
});

const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];

test('animation frame timestamps land on the 60 Hz frame grid', async () => {
  const timestamps = await page.evaluate(
    () =>
      new Promise((done) => {
        const stamps = [];
        const tick = (timestamp) => {
          stamps.push(timestamp);
          if (stamps.length < 6) requestAnimationFrame(tick);
          else done(stamps);
        };
        requestAnimationFrame(tick);
      }),
  );
  for (const timestamp of timestamps) {
    const frames = timestamp / FRAME_MS;
    assert.ok(Math.abs(frames - Math.round(frames)) < 0.001, `${timestamp} ms is not a whole number of frames (${frames})`);
  }
  for (let index = 1; index < timestamps.length; index++) {
    const gap = (timestamps[index] - timestamps[index - 1]) / FRAME_MS;
    assert.ok(gap >= 0.999 && Math.abs(gap - Math.round(gap)) < 0.001, `frame gap ${timestamps[index] - timestamps[index - 1]} ms`);
  }
});

test('the first animation frame comes at the next frame boundary, not a whole frame after the request', async () => {
  // Requests at random points within a frame, as a click's are.
  const delays = await page.evaluate(async () => {
    const samples = [];
    for (let index = 0; index < 30; index++) {
      await new Promise((settled) => setTimeout(settled, Math.random() * 17));
      samples.push(
        await new Promise((done) => {
          const requested = performance.now();
          requestAnimationFrame(() => done(performance.now() - requested));
        }),
      );
    }
    return samples;
  });
  const mean = delays.reduce((sum, delay) => sum + delay, 0) / delays.length;
  // A grid gives about 8.3 ms plus timer lateness; `now + 16` gives about 17.5.
  assert.ok(mean < 13, `mean delay to the first frame ${mean.toFixed(1)} ms (samples ${delays.map((d) => d.toFixed(1)).join(' ')})`);
});

test("Playwright's two-frame stability wait costs about one and a half frames", async () => {
  const waits = await page.evaluate(async () => {
    const button = document.getElementById('go');
    const samples = [];
    for (let index = 0; index < 21; index++) {
      await new Promise((settled) => setTimeout(settled, Math.random() * 17));
      samples.push(
        await new Promise((done) => {
          const started = performance.now();
          let frames = 0;
          const tick = () => {
            button.getBoundingClientRect();
            if (++frames < 2) requestAnimationFrame(tick);
            else done(performance.now() - started);
          };
          requestAnimationFrame(tick);
        }),
      );
    }
    return samples;
  });
  // Chrome: about 25 ms. pilot with `now + 16` frames: 36.5 ms every time.
  assert.ok(median(waits) < 31, `median two-frame wait ${median(waits).toFixed(1)} ms`);
});
