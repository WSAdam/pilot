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

// 2026-11-18 (Build 11): pilot's frames follow the display's refresh rate, as
// Chrome's do (build11-frames.test.mjs), so this file pins pilot to 60 Hz with
// PILOT_FRAME_RATE to keep testing the 60 Hz grid on any display.
before(async () => {
  browser = await chromium.launch({
    executablePath: pilotBin,
    headless: true,
    env: { ...noWindowEnv, PILOT_FRAME_RATE: '60' },
    timeout: 30_000,
  });
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

/** Waits a random 25-40 ms, longer than a 60 Hz frame, so no frame is due. */
const idlePause = () => new Promise((settled) => setTimeout(settled, 25 + Math.random() * 15));

// 2026-11-18: Build 9's gate failed the mean-delay test above on a loaded macOS
// runner (13.4 ms), and Chromium 1223 (headless shell and new headless) showed
// why pilot sits so close to its limit. When no frame is running, Chrome serves
// a requested animation frame at once (viz's missed begin-frame), stamped with
// the latest vsync; pilot made it wait for the next boundary, 8.3 ms on average.
// Chrome on a page idle for over a frame: median 0.1 ms; pilot: about 9 ms.
test('an animation frame requested on an idle page runs at once, stamped with the current frame', async () => {
  const samples = await page.evaluate(async (pause) => {
    const idle = new Function(`return (${pause})`)();
    const results = [];
    for (let index = 0; index < 20; index++) {
      await idle();
      results.push(
        await new Promise((done) => {
          const requested = performance.now();
          requestAnimationFrame((stamp) => done({ delay: performance.now() - requested, stampAfterRequest: stamp - requested }));
        }),
      );
    }
    return results;
  }, idlePause.toString());
  const delays = samples.map((sample) => sample.delay);
  assert.ok(median(delays) < 4, `median delay ${median(delays).toFixed(1)} ms (samples ${delays.map((d) => d.toFixed(1)).join(' ')})`);
  // The frame began before the request, as a vsync-stamped frame does in Chrome.
  const stamps = samples.map((sample) => sample.stampAfterRequest);
  assert.ok(median(stamps) <= 0, `median stamp minus request ${median(stamps).toFixed(1)} ms (samples ${stamps.map((d) => d.toFixed(1)).join(' ')})`);
});

// 2026-11-18: with the first frame served at once, the two-frame wait
// Playwright's click pays after an idle moment is about one frame: Chrome 1223
// measured a median of 9-11 ms at 120 Hz, pilot about 26 ms.
test("Playwright's two-frame stability wait after an idle moment costs about one frame", async () => {
  const waits = await page.evaluate(async (pause) => {
    const idle = new Function(`return (${pause})`)();
    const button = document.getElementById('go');
    const results = [];
    for (let index = 0; index < 21; index++) {
      await idle();
      results.push(
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
    return results;
  }, idlePause.toString());
  assert.ok(median(waits) < 20, `median two-frame wait ${median(waits).toFixed(1)} ms (samples ${waits.map((d) => d.toFixed(1)).join(' ')})`);
});
