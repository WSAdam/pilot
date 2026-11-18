// Build 11 (2026-11-18): animation frames follow the display's refresh rate,
// as Chrome's do. Each test pins Chrome's behaviour (checked against Chromium
// headless shell 1223 by pointing PILOT_BIN at it) and was proven red against
// the release binary of Build 9 before its fix was written.
//
// Chrome's compositor (viz) drives animation frames from the display's vsync.
// On macOS that is a CVDisplayLink for the display the compositor names
// (components/viz/service/frame_sinks/external_begin_frame_source_mac.cc).
// Headless Chrome, the shell and new headless alike, has a screen of its own
// whose first display has id 1 (ui/display/headless/headless_screen_manager.cc),
// and macOS gives the built-in panel CGDirectDisplayID 1, so headless Chrome's
// frames step at that panel's rate: 8.33 ms on a 120 Hz ProMotion panel. When
// display 1 has no display link, and on Linux, where headless ozone has no
// vsync provider, viz falls back to a 60 Hz timer
// (BeginFrameArgs::DefaultInterval). Build 9 kept a fixed 60 Hz grid, so on a
// 120 Hz Mac each of Playwright's waits for a frame cost twice Chrome's.
//
// The expected rate follows the machine: PILOT_EXPECTED_FRAME_RATE when set;
// on macOS, display 1's nominal refresh period read through CoreVideo the way
// Chrome reads it (python3 with ctypes; no display link means 60 Hz); else 60 Hz.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin } from './support.mjs';

const DEFAULT_FRAME_RATE = 60;
const usableRate = (rate) => Number.isFinite(rate) && rate >= 1 && rate <= 1000;

// CVDisplayLinkMac::GetForDisplay and GetRefreshInterval
// (ui/display/mac/cv_display_link_mac.mm): a link whose current display is 0
// counts as none. Prints 0 when display 1 has no usable display link.
const DISPLAY_ONE_RATE_PY = `
import ctypes
cv = ctypes.CDLL('/System/Library/Frameworks/CoreVideo.framework/CoreVideo')
class CVTime(ctypes.Structure):
    _fields_ = [('value', ctypes.c_int64), ('scale', ctypes.c_int32), ('flags', ctypes.c_int32)]
cv.CVDisplayLinkCreateWithCGDisplay.argtypes = [ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p)]
cv.CVDisplayLinkCreateWithCGDisplay.restype = ctypes.c_int32
cv.CVDisplayLinkGetCurrentCGDisplay.argtypes = [ctypes.c_void_p]
cv.CVDisplayLinkGetCurrentCGDisplay.restype = ctypes.c_uint32
cv.CVDisplayLinkGetNominalOutputVideoRefreshPeriod.argtypes = [ctypes.c_void_p]
cv.CVDisplayLinkGetNominalOutputVideoRefreshPeriod.restype = CVTime
cv.CVDisplayLinkRelease.argtypes = [ctypes.c_void_p]
link = ctypes.c_void_p()
rate = 0.0
if cv.CVDisplayLinkCreateWithCGDisplay(1, ctypes.byref(link)) == 0:
    if cv.CVDisplayLinkGetCurrentCGDisplay(link) != 0:
        period = cv.CVDisplayLinkGetNominalOutputVideoRefreshPeriod(link)
        if not (period.flags & 1) and period.value > 0:
            rate = period.scale / period.value
    cv.CVDisplayLinkRelease(link)
print(rate)
`;

/** The rate headless Chrome's animation frames run at on this machine. */
function expectedFrameRate() {
  const fromEnv = process.env.PILOT_EXPECTED_FRAME_RATE;
  if (fromEnv !== undefined && usableRate(Number(fromEnv))) {
    return { rate: Number(fromEnv), source: 'PILOT_EXPECTED_FRAME_RATE' };
  }
  if (process.platform !== 'darwin') {
    return { rate: DEFAULT_FRAME_RATE, source: 'no display: the 60 Hz timer' };
  }
  try {
    const rate = Number(execFileSync('python3', ['-I', '-c', DISPLAY_ONE_RATE_PY], { encoding: 'utf8', timeout: 15_000 }).trim());
    if (usableRate(rate)) return { rate, source: "display 1's CVDisplayLink" };
    return { rate: DEFAULT_FRAME_RATE, source: 'display 1 has no display link: the 60 Hz timer' };
  } catch (error) {
    console.log(`⚠️ could not read display 1's refresh rate (${error.message}); expecting 60 Hz`);
    return { rate: DEFAULT_FRAME_RATE, source: 'unreadable display: 60 Hz' };
  }
}

const { rate: FRAME_RATE, source: FRAME_RATE_SOURCE } = expectedFrameRate();
const FRAME_MS = 1000 / FRAME_RATE;
// pilot reads the display itself; an override in the caller's environment
// would make it ignore the display this test compares against.
const launchEnv = { ...noWindowEnv };
delete launchEnv.PILOT_FRAME_RATE;

let browser;
let page;

before(async () => {
  console.log(`🔍 expecting ${FRAME_RATE.toFixed(3)} Hz frames (${FRAME_MS.toFixed(3)} ms) from ${FRAME_RATE_SOURCE}`);
  browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: launchEnv, timeout: 30_000 });
  page = await (await browser.newContext()).newPage();
  await page.setContent('<button id="go">Go</button>');
});
after(async () => {
  await browser?.close();
});

const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
const percentile = (values, fraction) =>
  [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.floor(values.length * fraction))];
const list = (values) => values.map((value) => value.toFixed(2)).join(' ');

// 2026-11-18: Build 9's frames were 16.67 ms apart on a 120 Hz Mac, where
// Chromium 1223's are 8.33 ms apart (median of a 12-frame chain: 8.30).
test("animation frames step at the display's refresh interval", async () => {
  const stamps = await page.evaluate(
    () =>
      new Promise((done) => {
        const collected = [];
        const tick = (stamp) => {
          collected.push(stamp);
          if (collected.length < 12) requestAnimationFrame(tick);
          else done(collected);
        };
        requestAnimationFrame(tick);
      }),
  );
  const gaps = stamps.slice(1).map((stamp, index) => stamp - stamps[index]);
  assert.ok(
    Math.abs(median(gaps) - FRAME_MS) < 1,
    `median frame gap ${median(gaps).toFixed(2)} ms, display frame ${FRAME_MS.toFixed(2)} ms (gaps ${list(gaps)})`,
  );
});

// 2026-11-18: Build 9 served an idle page's frame at once but stamped it with
// the start of the current 60 Hz frame, up to 16.7 ms before the request; on a
// 120 Hz Mac, Chromium 1223 stamps it with the latest vsync, under 8.33 ms
// before (largest of 21 samples: 7.5-8.2 ms).
test("an animation frame requested on an idle page runs at once, stamped with the display's latest frame", async () => {
  const samples = await page.evaluate(async (frameMs) => {
    const results = [];
    for (let index = 0; index < 20; index++) {
      // Two to four display frames with nothing to draw: the page is idle.
      await new Promise((settled) => setTimeout(settled, frameMs * (2 + Math.random() * 2)));
      results.push(
        await new Promise((done) => {
          const requested = performance.now();
          requestAnimationFrame((stamp) => done({ delay: performance.now() - requested, lag: requested - stamp }));
        }),
      );
    }
    return results;
  }, FRAME_MS);
  const delays = samples.map((sample) => sample.delay);
  const lags = samples.map((sample) => sample.lag);
  assert.ok(median(delays) < 4, `median delay to the frame ${median(delays).toFixed(2)} ms (samples ${list(delays)})`);
  // The frame began at or before the request, and less than a display frame before.
  assert.ok(median(lags) > -0.2, `median request minus stamp ${median(lags).toFixed(2)} ms (samples ${list(lags)})`);
  assert.ok(
    percentile(lags, 0.9) < FRAME_MS + 1,
    `90th percentile of request minus stamp ${percentile(lags, 0.9).toFixed(2)} ms, display frame ${FRAME_MS.toFixed(2)} ms (samples ${list(lags)})`,
  );
});

// 2026-11-18: Playwright's click waits for two animation frames to see the
// element is not moving. Between actions, after a pause of up to two display
// frames, Build 9 measured a median of 21.8-23.6 ms on a 120 Hz Mac and
// Chromium 1223 6.7 ms. The bound, 1.7 display frames (14.2 ms at 120 Hz,
// 28.3 ms at 60 Hz), scales with the display as Chrome's wait does.
test("Playwright's two-frame stability wait between actions costs about one display frame", async () => {
  const waits = await page.evaluate(async (frameMs) => {
    const button = document.getElementById('go');
    const results = [];
    for (let index = 0; index < 21; index++) {
      await new Promise((settled) => setTimeout(settled, Math.random() * 2 * frameMs));
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
  }, FRAME_MS);
  assert.ok(
    median(waits) < 1.7 * FRAME_MS,
    `median two-frame wait ${median(waits).toFixed(2)} ms, bound ${(1.7 * FRAME_MS).toFixed(2)} ms (samples ${list(waits)})`,
  );
});
