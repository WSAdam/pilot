// Full-page screenshots of pages taller than one raster surface (Build 6).
// pilot's rasterizer draws at most 65,535 device pixels a side and holds at
// most 256 MB of pixels at once, so `fullPage: true` failed on a long page
// ("raster dimensions 400x70016 exceed backend limit 65535"). Chrome returns
// the whole page.
// 2026-11-12: proven red against the release binary of 2026-11-10.
import assert from 'node:assert/strict';
import { inflateSync } from 'node:zlib';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin } from './support.mjs';

let browser;

before(async () => {
  browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: 30_000 });
});
after(async () => {
  await browser?.close();
});

/** Decodes an 8-bit RGBA, non-interlaced PNG: enough to read pilot's screenshots. */
function decodePng(png) {
  assert.equal(png.subarray(1, 4).toString('latin1'), 'PNG');
  let offset = 8;
  let width = 0;
  let height = 0;
  const data = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.subarray(offset + 4, offset + 8).toString('latin1');
    const body = png.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      assert.deepEqual([body[8], body[9], body[12]], [8, 6, 0], 'RGBA8, not interlaced');
    } else if (type === 'IDAT') {
      data.push(body);
    }
    offset += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(data));
  const stride = width * 4;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const left = x >= 4 ? pixels[y * stride + x - 4] : 0;
      const up = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const upLeft = x >= 4 && y > 0 ? pixels[(y - 1) * stride + x - 4] : 0;
      let predictor = 0;
      if (filter === 1) predictor = left;
      else if (filter === 2) predictor = up;
      else if (filter === 3) predictor = (left + up) >> 1;
      else if (filter === 4) {
        const estimate = left + up - upLeft;
        const [a, b, c] = [Math.abs(estimate - left), Math.abs(estimate - up), Math.abs(estimate - upLeft)];
        predictor = a <= b && a <= c ? left : b <= c ? up : upLeft;
      }
      pixels[y * stride + x] = (line[x] + predictor) & 0xff;
    }
  }
  return { width, height, pixel: (x, y) => [...pixels.subarray((y * width + x) * 4, (y * width + x) * 4 + 4)] };
}

async function tallPageScreenshot(width, height) {
  const page = await (await browser.newContext({ viewport: { width, height: 300 } })).newPage();
  try {
    await page.setContent(
      `<body style="margin:0;background:white">` +
        `<div style="height:${height}px;background:rgb(0,0,255)"></div>` +
        `<div style="height:40px;background:rgb(0,255,0)"></div></body>`,
    );
    return decodePng(await page.screenshot({ fullPage: true, timeout: 120_000 }));
  } finally {
    await page.context().close();
  }
}

test('a full-page screenshot of a page taller than 65,535 px has every row, down to the last', async () => {
  const image = await tallPageScreenshot(400, 70_000);
  assert.equal(image.width, 400);
  assert.equal(image.height, 70_040);
  assert.deepEqual(image.pixel(200, 10), [0, 0, 255, 255]);
  assert.deepEqual(image.pixel(200, 65_600), [0, 0, 255, 255], 'just past the old limit');
  assert.deepEqual(image.pixel(200, 70_020), [0, 255, 0, 255], 'the last block, at the bottom of the page');
});

test('a full-page screenshot over the PNG encoder budget of 128 MB succeeds at a desktop width', async () => {
  // 1280 x 30,040 x 4 bytes is 154 MB: one raster surface holds it, the encoder refused it.
  const image = await tallPageScreenshot(1280, 30_000);
  assert.equal(image.height, 30_040);
  assert.deepEqual(image.pixel(640, 30_020), [0, 255, 0, 255]);
});

test('a full-page screenshot over 256 MB of pixels succeeds at a desktop width', async () => {
  // 1280 x 55,040 x 4 bytes is 282 MB: under 65,535 px, but over the raster's byte budget.
  const image = await tallPageScreenshot(1280, 55_000);
  assert.equal(image.width, 1280);
  assert.equal(image.height, 55_040);
  assert.deepEqual(image.pixel(640, 55_020), [0, 255, 0, 255]);
});
